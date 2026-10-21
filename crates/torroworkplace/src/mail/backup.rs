//! Publish a verified mail ZIP in the core's private backup directory without exposing partial files.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{NaiveDate, Utc};
use sha2::{Digest, Sha256};
use torro_core::i18n::tr;
use torro_ipc::mail::{BackupJob, BackupPhase, BackupScope};
use torro_mail_backup::format::{Account, Counts, FORMAT, Folder, Inventory, Manifest, Source, Status, VERSION};
use torro_mail_backup::writer::{ArchiveWriter, MessageMetadata};
use torro_mail_backup::{ArchiveError, VerifiedArchive, verify_archive};
use torro_mail_imap::folders;
use torro_mail_imap::imap::{ImapError, Search};

use super::service::{MailError, MailService};
use super::store::LocalOriginal;
use super::{MailDirs, MailStoreError};

#[derive(Debug)]
pub(crate) enum BackupFileError {
    Io(io::Error),
    Archive(ArchiveError),
    InvalidName,
    Incomplete,
}

impl fmt::Display for BackupFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot publish mail backup: {error}"),
            Self::Archive(error) => write!(f, "mail backup failed verification: {error}"),
            Self::InvalidName => f.write_str("invalid mail backup filename"),
            Self::Incomplete => f.write_str("incomplete mail backup cannot be published"),
        }
    }
}

impl std::error::Error for BackupFileError {}

impl From<io::Error> for BackupFileError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ArchiveError> for BackupFileError {
    fn from(error: ArchiveError) -> Self {
        Self::Archive(error)
    }
}

/// The only publication route for generated backups. The builder writes to a private temporary
/// file in the destination directory; failure, cancellation or invalid output removes that file.
#[cfg(test)]
pub(crate) fn write_verified_backup(
    dirs: &MailDirs,
    filename: &str,
    build: impl FnOnce(&mut File) -> Result<(), ArchiveError>,
) -> Result<(PathBuf, VerifiedArchive), BackupFileError> {
    let mut pending = PendingBackup::new(dirs, filename)?;
    build(pending.file_mut())?;
    pending.publish()
}

/// A private in-progress archive that can stay open while an async IMAP job fetches chunks.
/// Dropping it before `publish` removes the partial ZIP.
pub(crate) struct PendingBackup {
    backup_dir: PathBuf,
    destination: PathBuf,
    temporary: tempfile::NamedTempFile,
}

impl PendingBackup {
    pub(crate) fn new(dirs: &MailDirs, filename: &str) -> Result<Self, BackupFileError> {
        if !valid_filename(filename) {
            return Err(BackupFileError::InvalidName);
        }
        reject_symlink(&dirs.data)?;
        crate::clients::create_private_dir(&dirs.data)?;
        let backup_dir = dirs.data.join("backups");
        reject_symlink(&backup_dir)?;
        crate::clients::create_private_dir(&backup_dir)?;
        let destination = backup_dir.join(filename);
        let temporary = tempfile::Builder::new().prefix(".backup-").suffix(".partial").tempfile_in(&backup_dir)?;
        Ok(Self { backup_dir, destination, temporary })
    }

    pub(crate) fn file_mut(&mut self) -> &mut File {
        self.temporary.as_file_mut()
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.backup_dir
    }

    pub(crate) fn publish(mut self) -> Result<(PathBuf, VerifiedArchive), BackupFileError> {
        self.temporary.as_file_mut().seek(SeekFrom::Start(0))?;
        let report = verify_archive(self.temporary.as_file_mut())?;
        if !report.complete {
            return Err(BackupFileError::Incomplete);
        }
        self.temporary.as_file().sync_all()?;
        self.temporary.persist_noclobber(&self.destination).map_err(|error| BackupFileError::Io(error.error))?;
        Ok((self.destination, report))
    }
}

fn valid_filename(name: &str) -> bool {
    name.len() <= 120
        && !name.starts_with('.')
        && name.ends_with(".zip")
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::other("mail backup directory is a symlink")),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Debug)]
pub(crate) enum BackupRunError {
    Mail(MailError),
    Store(MailStoreError),
    Imap(ImapError),
    Archive(ArchiveError),
    File(BackupFileError),
    Io(io::Error),
    Incomplete(&'static str),
    Busy,
    Random,
    Cancelled,
}

impl fmt::Display for BackupRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mail(error) => write!(f, "mail account: {error}"),
            Self::Store(error) => write!(f, "local originals: {error}"),
            Self::Imap(error) => write!(f, "IMAP source: {error}"),
            Self::Archive(error) => write!(f, "archive: {error}"),
            Self::File(error) => write!(f, "backup file: {error}"),
            Self::Io(error) => write!(f, "backup I/O: {error}"),
            Self::Incomplete(reason) => write!(f, "source inventory changed: {reason}"),
            Self::Busy => f.write_str("another backup for this account is already running"),
            Self::Random => f.write_str("cannot create a backup job id"),
            Self::Cancelled => f.write_str("backup cancelled"),
        }
    }
}

impl BackupRunError {
    fn user_message(&self) -> String {
        match self {
            Self::Mail(error) => error.message(),
            Self::Imap(ImapError::Auth(_)) => tr("Password was refused").into(),
            Self::Imap(ImapError::Offline(_)) => tr("The server cannot be reached.").into(),
            Self::Imap(_) => tr("The mail server answered unexpectedly.").into(),
            Self::Incomplete(_) => tr("Mail backup is incomplete because the source changed.").into(),
            Self::Cancelled => tr("Mail backup was cancelled.").into(),
            Self::File(_) | Self::Archive(_) | Self::Store(_) | Self::Io(_) | Self::Random => {
                tr("Mail backup could not be completed.").into()
            }
            Self::Busy => tr("A backup for this mail account is already running.").into(),
        }
    }
}

impl From<MailError> for BackupRunError {
    fn from(error: MailError) -> Self {
        Self::Mail(error)
    }
}

impl From<MailStoreError> for BackupRunError {
    fn from(error: MailStoreError) -> Self {
        Self::Store(error)
    }
}

impl From<ImapError> for BackupRunError {
    fn from(error: ImapError) -> Self {
        Self::Imap(error)
    }
}

impl From<ArchiveError> for BackupRunError {
    fn from(error: ArchiveError) -> Self {
        Self::Archive(error)
    }
}

impl From<BackupFileError> for BackupRunError {
    fn from(error: BackupFileError) -> Self {
        Self::File(error)
    }
}

impl From<io::Error> for BackupRunError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

const RUNNING: u8 = 0;
const CANCELLED: u8 = 1;
const COMMITTING: u8 = 2;

fn check_cancel(state: &AtomicU8) -> Result<(), BackupRunError> {
    if state.load(Ordering::Acquire) == CANCELLED { Err(BackupRunError::Cancelled) } else { Ok(()) }
}

fn uid_inventory_hash(uids: &HashSet<u32>) -> String {
    let mut sorted: Vec<u32> = uids.iter().copied().collect();
    sorted.sort_unstable();
    let mut hash = Sha256::new();
    for uid in sorted {
        hash.update(uid.to_be_bytes());
    }
    hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

struct JobEntry {
    job: BackupJob,
    state: Arc<AtomicU8>,
}

/// In-process progress and cancellation. A clean shutdown drops partial files through
/// `NamedTempFile`; after a crash, hidden partial files may remain but cannot claim completion.
#[derive(Clone, Default)]
pub(crate) struct BackupJobs {
    entries: Arc<Mutex<HashMap<String, JobEntry>>>,
}

impl BackupJobs {
    pub(crate) fn start(
        &self,
        mail: Arc<MailService>,
        dirs: MailDirs,
        account: String,
        source: BackupScope,
        since: Option<NaiveDate>,
        filename: Option<String>,
    ) -> Result<BackupJob, BackupRunError> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| BackupRunError::Random)?;
        let id = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let filename = filename.unwrap_or_else(|| format!("mail-{}-{id}.zip", Utc::now().format("%Y%m%dT%H%M%S")));
        if !valid_filename(&filename) {
            return Err(BackupRunError::File(BackupFileError::InvalidName));
        }
        let state = Arc::new(AtomicU8::new(RUNNING));
        let job = BackupJob {
            id: id.clone(),
            account: account.clone(),
            source,
            phase: BackupPhase::Queued,
            folders: 0,
            messages: 0,
            bytes: 0,
            archive: None,
            warnings: Vec::new(),
        };
        {
            let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
            if entries.values().any(|entry| {
                entry.job.account == account
                    && matches!(
                        entry.job.phase,
                        BackupPhase::Queued | BackupPhase::Counting | BackupPhase::Reading | BackupPhase::Verifying
                    )
            }) {
                return Err(BackupRunError::Busy);
            }
            if entries.len() >= 128 {
                entries.retain(|_, entry| {
                    !matches!(entry.job.phase, BackupPhase::Complete | BackupPhase::Incomplete | BackupPhase::Cancelled)
                });
            }
            entries.insert(id.clone(), JobEntry { job: job.clone(), state: Arc::clone(&state) });
        }
        let jobs = self.clone();
        tokio::spawn(async move {
            let progress = BackupProgress { jobs: &jobs, id: &id, state: &state };
            let result = match source {
                BackupScope::Server => run_server_backup(mail, dirs, &account, since, &filename, progress).await,
                BackupScope::Local => run_local_backup(mail, dirs, &account, since, &filename, progress).await,
            };
            let mut entries = jobs.entries.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(entry) = entries.get_mut(&id) {
                match result {
                    _ if state.load(Ordering::Acquire) == CANCELLED => {
                        entry.job.phase = BackupPhase::Cancelled;
                    }
                    Ok(verified) => {
                        entry.job.phase = BackupPhase::Complete;
                        entry.job.archive = Some(filename);
                        entry.job.folders = verified.folders;
                        entry.job.messages = verified.messages;
                        entry.job.bytes = verified.bytes;
                        entry.job.warnings = verified.warnings;
                    }
                    Err(BackupRunError::Cancelled) => {
                        entry.job.phase = BackupPhase::Cancelled;
                    }
                    Err(error) => {
                        entry.job.phase = BackupPhase::Incomplete;
                        entry.job.warnings.push(error.user_message());
                    }
                }
            }
        });
        Ok(job)
    }

    pub(crate) fn status(&self, account: &str, id: &str) -> Option<BackupJob> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .filter(|entry| entry.job.account == account)
            .map(|entry| entry.job.clone())
    }

    pub(crate) fn cancel(&self, account: &str, id: &str) -> Option<BackupJob> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries.get_mut(id).filter(|entry| entry.job.account == account)?;
        if !matches!(entry.job.phase, BackupPhase::Complete | BackupPhase::Incomplete | BackupPhase::Cancelled)
            && entry.state.compare_exchange(RUNNING, CANCELLED, Ordering::AcqRel, Ordering::Acquire).is_ok()
        {
            entry.job.phase = BackupPhase::Cancelled;
        }
        Some(entry.job.clone())
    }

    fn phase(&self, id: &str, phase: BackupPhase) {
        if let Some(entry) = self.entries.lock().unwrap_or_else(PoisonError::into_inner).get_mut(id)
            && entry.state.load(Ordering::Acquire) != CANCELLED
        {
            entry.job.phase = phase;
        }
    }

    fn progress(&self, id: &str, folders: u64, messages: u64, bytes: u64) {
        if let Some(entry) = self.entries.lock().unwrap_or_else(PoisonError::into_inner).get_mut(id) {
            entry.job.folders = folders;
            entry.job.messages = messages;
            entry.job.bytes = bytes;
        }
    }
}

/// Build a complete server archive from a read-only folder inventory. Every body is fetched by
/// IMAP partial FETCH into a same-directory spool file, then streamed into the ZIP.
struct BackupProgress<'a> {
    jobs: &'a BackupJobs,
    id: &'a str,
    state: &'a AtomicU8,
}

async fn run_server_backup(
    mail: Arc<MailService>,
    dirs: MailDirs,
    account: &str,
    since: Option<NaiveDate>,
    filename: &str,
    progress: BackupProgress<'_>,
) -> Result<VerifiedArchive, BackupRunError> {
    let BackupProgress { jobs, id: job_id, state } = progress;
    let address =
        account.strip_prefix(torro_mail::ACCOUNT_PREFIX).ok_or(BackupRunError::Incomplete("invalid account id"))?;
    check_cancel(state)?;
    jobs.phase(job_id, BackupPhase::Counting);
    let mut command = mail.command(account).await?;
    let listed = command.session().list().await?;
    command.release();
    let mut paths = HashSet::new();
    let mut aggregate_paths = HashSet::new();
    let mut folders = Vec::new();
    for folder in listed {
        if !paths.insert(folder.path.clone()) {
            continue;
        }
        // The aggregate may hold archived messages absent from every ordinary folder. Preserve
        // every UID: identical RFC-822 bytes do not prove that two UIDs are one message.
        let aggregate = folder.has_attribute("\\All") || folder.has_attribute("\\AllMail");
        if aggregate {
            aggregate_paths.insert(folder.path.clone());
        }
        let role = folders::role_of_attributes(&folder.attributes)
            .map(|role| format!("{role:?}").to_ascii_lowercase())
            .or_else(|| folder.path.eq_ignore_ascii_case("INBOX").then(|| "inbox".to_string()))
            .or_else(|| aggregate.then(|| "all".to_string()));
        let selectable = folder.selectable();
        folders.push(Folder {
            id: String::new(),
            name: folders::display_name(&folder.path, folder.delimiter),
            path: folder.path,
            delimiter: folder.delimiter.map(|delimiter| delimiter.to_string()),
            selectable,
            role,
            uid_validity: None,
            inventory: None,
        });
    }
    folders.sort_by_key(|folder| aggregate_paths.contains(&folder.path));
    for (index, folder) in folders.iter_mut().enumerate() {
        folder.id = format!("f{:06}", index + 1);
    }
    let started_at = Utc::now();
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        status: Status::Incomplete,
        source: Source::Server,
        since,
        started_at,
        finished_at: None,
        account: Account { address: address.to_string(), server: None },
        counts: Counts { folders: folders.len() as u64, messages: 0, bytes: 0 },
        folders,
        messages: Vec::new(),
        warnings: Vec::new(),
    };
    jobs.progress(job_id, manifest.counts.folders, 0, 0);
    let mut pending = PendingBackup::new(&dirs, filename)?;
    let spool_dir = pending.directory().to_path_buf();
    let mut writer = ArchiveWriter::new(pending.file_mut(), manifest)?;
    let folder_count = writer.manifest_mut().folders.len();
    let mut source_inventory = HashMap::new();
    for index in 0..folder_count {
        check_cancel(state)?;
        let folder = writer.manifest_mut().folders[index].clone();
        if !folder.selectable {
            continue;
        }
        jobs.phase(job_id, BackupPhase::Reading);
        let mut command = mail.command(account).await?;
        let session = command.session();
        let selected = session.examine(&folder.path).await?;
        writer.manifest_mut().folders[index].uid_validity = Some(selected.uid_validity);
        let mut seen: HashSet<u32> = session.uid_search(&Search::All).await?.into_iter().collect();
        let mut pending_uids: Vec<u32> = seen.iter().copied().collect();
        pending_uids.sort_unstable();
        let mut eligible = HashSet::new();
        let mut stable = false;
        for attempt in 0..=2 {
            for uid in pending_uids.drain(..) {
                check_cancel(state)?;
                let metadata = session.fetch_backup_metadata(&[uid]).await?;
                let [metadata] = metadata.as_slice() else {
                    return Err(BackupRunError::Incomplete("message disappeared during metadata fetch"));
                };
                if metadata.uid != uid {
                    return Err(BackupRunError::Incomplete("source returned a different UID"));
                }
                if since.is_some_and(|day| metadata.internal_date.date_naive() < day) {
                    continue;
                }
                let mut spool = tempfile::tempfile_in(&spool_dir)?;
                let mut offset = 0u64;
                while offset < metadata.bytes {
                    check_cancel(state)?;
                    let count = (metadata.bytes - offset).min(64 * 1024) as usize;
                    let chunk = session.fetch_backup_chunk(uid, offset, count).await?;
                    let Some(chunk) = chunk else {
                        return Err(BackupRunError::Incomplete("message disappeared during body fetch"));
                    };
                    if chunk.len() != count {
                        return Err(BackupRunError::Incomplete("server returned a short body chunk"));
                    }
                    spool.write_all(&chunk)?;
                    offset += count as u64;
                }
                spool.seek(SeekFrom::Start(0))?;
                writer.add_message(
                    &folder.id,
                    spool,
                    MessageMetadata {
                        uid: Some(uid),
                        uid_validity: Some(selected.uid_validity),
                        internal_date: Some(metadata.internal_date),
                        flags: metadata.flags.clone(),
                        expected_bytes: Some(metadata.bytes),
                    },
                )?;
                eligible.insert(uid);
                let counts = &writer.manifest_mut().counts;
                jobs.progress(job_id, counts.folders, counts.messages, counts.bytes);
            }
            check_cancel(state)?;
            let checked = session.examine(&folder.path).await?;
            if checked.uid_validity != selected.uid_validity {
                return Err(BackupRunError::Incomplete("UIDVALIDITY changed during backup"));
            }
            let final_uids: HashSet<u32> = session.uid_search(&Search::All).await?.into_iter().collect();
            if final_uids == seen {
                stable = true;
                break;
            }
            if !seen.is_subset(&final_uids) {
                return Err(BackupRunError::Incomplete("messages disappeared during backup"));
            }
            if attempt == 2 {
                break;
            }
            pending_uids = final_uids.difference(&seen).copied().collect();
            pending_uids.sort_unstable();
            seen = final_uids;
        }
        if !stable {
            return Err(BackupRunError::Incomplete("folder kept changing during backup"));
        }
        writer.manifest_mut().folders[index].inventory = Some(Inventory {
            checked_at: Utc::now(),
            eligible_messages: eligible.len() as u64,
            uid_set_sha256: Some(uid_inventory_hash(&eligible)),
        });
        source_inventory.insert(folder.path.clone(), (selected.uid_validity, seen));
        command.release();
    }
    // Revisit the whole account after every body has been read. An earlier folder may change
    // while a later one is downloaded, so a per-folder check alone cannot prove completeness.
    let mut command = mail.command(account).await?;
    let final_list = command.session().list().await?;
    let final_paths: HashSet<String> = final_list.into_iter().map(|folder| folder.path).collect();
    if final_paths != paths {
        return Err(BackupRunError::Incomplete("folder list changed during backup"));
    }
    for (path, (validity, uids)) in &source_inventory {
        check_cancel(state)?;
        let selected = command.session().examine(path).await?;
        if selected.uid_validity != *validity {
            return Err(BackupRunError::Incomplete("UIDVALIDITY changed during final inventory"));
        }
        let final_uids: HashSet<u32> = command.session().uid_search(&Search::All).await?.into_iter().collect();
        if final_uids != *uids {
            return Err(BackupRunError::Incomplete("messages changed during final inventory"));
        }
    }
    command.release();
    check_cancel(state)?;
    jobs.phase(job_id, BackupPhase::Verifying);
    writer.finish(Status::Complete, Utc::now())?;
    state
        .compare_exchange(RUNNING, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| BackupRunError::Cancelled)?;
    let (_, published) = pending.publish()?;
    Ok(published)
}

/// A local backup reads only retained RFC-822 originals. Its two paged metadata scans detect
/// concurrent depot changes; each BLOB is read through SQLite in bounded chunks.
async fn run_local_backup(
    mail: Arc<MailService>,
    dirs: MailDirs,
    account: &str,
    since: Option<NaiveDate>,
    filename: &str,
    progress: BackupProgress<'_>,
) -> Result<VerifiedArchive, BackupRunError> {
    let BackupProgress { jobs, id: job_id, state } = progress;
    let address =
        account.strip_prefix(torro_mail::ACCOUNT_PREFIX).ok_or(BackupRunError::Incomplete("invalid account id"))?;
    check_cancel(state)?;
    jobs.phase(job_id, BackupPhase::Counting);
    let store = mail.store(account).await?;
    let started_at = Utc::now();
    let mut folder_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut folder_validities: HashMap<String, Option<u32>> = HashMap::new();
    let mut first_digest = Sha256::new();
    let mut after = 0i64;
    loop {
        check_cancel(state)?;
        let page = store.run(move |db| db.local_originals_page(after, 256)).await?;
        if page.is_empty() {
            break;
        }
        for original in page {
            after = original.seq;
            if since.is_some_and(|day| original.internal_date.date_naive() < day) {
                continue;
            }
            *folder_counts.entry(original.folder_path.clone()).or_default() += 1;
            folder_validities
                .entry(original.folder_path.clone())
                .and_modify(|validity| {
                    if *validity != Some(original.uid_validity) {
                        *validity = None;
                    }
                })
                .or_insert(Some(original.uid_validity));
            hash_local_metadata(&mut first_digest, &original);
        }
    }
    let missing = store.run(move |db| db.known_missing_originals(since)).await?;
    let mut folder_ids = HashMap::new();
    let folders = folder_counts
        .iter()
        .enumerate()
        .map(|(at, (path, _count))| {
            let id = format!("f{:06}", at + 1);
            folder_ids.insert(path.clone(), id.clone());
            Folder {
                id,
                name: path.clone(),
                path: path.clone(),
                delimiter: None,
                selectable: true,
                role: path.eq_ignore_ascii_case("INBOX").then(|| "inbox".into()),
                uid_validity: folder_validities.get(path).copied().flatten(),
                inventory: None,
            }
        })
        .collect::<Vec<_>>();
    let warnings = if missing == 0 {
        Vec::new()
    } else {
        vec![format!("{} {missing}", tr("Local mail headers without an original were omitted."))]
    };
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        status: Status::Incomplete,
        source: Source::Local,
        since,
        started_at,
        finished_at: None,
        account: Account { address: address.into(), server: None },
        counts: Counts { folders: folders.len() as u64, messages: 0, bytes: 0 },
        folders,
        messages: Vec::new(),
        warnings,
    };
    jobs.progress(job_id, manifest.counts.folders, 0, 0);
    let mut pending = PendingBackup::new(&dirs, filename)?;
    let spool_dir = pending.directory().to_path_buf();
    let mut writer = ArchiveWriter::new(pending.file_mut(), manifest)?;
    let mut second_digest = Sha256::new();
    let mut after = 0i64;
    jobs.phase(job_id, BackupPhase::Reading);
    loop {
        check_cancel(state)?;
        let page = store.run(move |db| db.local_originals_page(after, 256)).await?;
        if page.is_empty() {
            break;
        }
        for original in page {
            after = original.seq;
            if since.is_some_and(|day| original.internal_date.date_naive() < day) {
                continue;
            }
            hash_local_metadata(&mut second_digest, &original);
            let mut spool = tempfile::tempfile_in(&spool_dir)?;
            let mut digest = Sha256::new();
            let mut offset = 0u64;
            while offset < original.bytes {
                check_cancel(state)?;
                let count = (original.bytes - offset).min(64 * 1024) as usize;
                let seq = original.seq;
                let chunk = store.run(move |db| db.original_chunk(seq, offset, count)).await?;
                let Some(chunk) = chunk else {
                    return Err(BackupRunError::Incomplete("local original disappeared"));
                };
                if chunk.len() != count {
                    return Err(BackupRunError::Incomplete("local original was truncated"));
                }
                digest.update(&chunk);
                spool.write_all(&chunk)?;
                offset += count as u64;
            }
            let hash = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            if hash != original.sha256 {
                return Err(BackupRunError::Incomplete("local original hash changed"));
            }
            spool.seek(SeekFrom::Start(0))?;
            let id = folder_ids.get(&original.folder_path).ok_or(BackupRunError::Incomplete("local folder changed"))?;
            let provenance =
                folder_validities.get(&original.folder_path).copied().flatten() == Some(original.uid_validity);
            writer.add_message(
                id,
                spool,
                MessageMetadata {
                    uid: provenance.then_some(original.uid),
                    uid_validity: provenance.then_some(original.uid_validity),
                    internal_date: Some(original.internal_date),
                    flags: original.flags,
                    expected_bytes: Some(original.bytes),
                },
            )?;
            let counts = &writer.manifest_mut().counts;
            jobs.progress(job_id, counts.folders, counts.messages, counts.bytes);
        }
    }
    let mut final_digest = Sha256::new();
    let mut after = 0i64;
    loop {
        check_cancel(state)?;
        let page = store.run(move |db| db.local_originals_page(after, 256)).await?;
        if page.is_empty() {
            break;
        }
        for original in page {
            after = original.seq;
            if !since.is_some_and(|day| original.internal_date.date_naive() < day) {
                hash_local_metadata(&mut final_digest, &original);
            }
        }
    }
    let first = first_digest.finalize();
    if first != second_digest.finalize()
        || first != final_digest.finalize()
        || missing != store.run(move |db| db.known_missing_originals(since)).await?
    {
        return Err(BackupRunError::Incomplete("local originals changed during backup"));
    }
    let checked_at = Utc::now();
    for folder in &mut writer.manifest_mut().folders {
        let count = folder_counts.get(&folder.path).copied().unwrap_or(0);
        folder.inventory = Some(Inventory { checked_at, eligible_messages: count, uid_set_sha256: None });
    }
    check_cancel(state)?;
    jobs.phase(job_id, BackupPhase::Verifying);
    writer.finish(Status::Complete, Utc::now())?;
    state
        .compare_exchange(RUNNING, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| BackupRunError::Cancelled)?;
    let (_, published) = pending.publish()?;
    Ok(published)
}

fn hash_local_metadata(hash: &mut Sha256, original: &LocalOriginal) {
    hash.update(format!("{original:?}\n").as_bytes());
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::io::Write;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::{Arc, Mutex};

    use chrono::{Duration, Utc};
    use torro_ipc::mail::{BackupJob, BackupPhase, BackupScope};

    use super::{
        BackupFileError, BackupJobs, BackupProgress, CANCELLED, COMMITTING, JobEntry, RUNNING, run_local_backup,
        run_server_backup, uid_inventory_hash, write_verified_backup,
    };
    use crate::account::github::tests::MemoryTokens;
    use crate::mail::MailDirs;
    use crate::mail::service::tests::{PASSWORD, mail_account, service_in, store_accounts};
    use crate::mail::sync::fake::FakeImap;

    const SAMPLE_ZIP: &[u8] = include_bytes!("../../../torro-mail-backup/tests/fixtures/sample-v1.zip");

    fn dirs(root: &std::path::Path) -> MailDirs {
        MailDirs::new(&root.join("data"), &root.join("cache"))
    }

    #[test]
    fn publishes_only_verified_archive_without_overwriting() {
        let root = tempfile::tempdir().expect("tempdir");
        let dirs = dirs(root.path());
        let (path, report) = write_verified_backup(&dirs, "mail-2026.zip", |file| {
            file.write_all(SAMPLE_ZIP)?;
            Ok(())
        })
        .expect("archive published");
        assert!(report.complete);
        assert_eq!(fs::read(&path).expect("backup"), SAMPLE_ZIP);
        assert!(
            write_verified_backup(&dirs, "mail-2026.zip", |file| {
                file.write_all(SAMPLE_ZIP)?;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).expect("original backup"), SAMPLE_ZIP);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).expect("file mode").permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn failed_or_invalid_backup_leaves_no_published_or_partial_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let dirs = dirs(root.path());
        assert!(
            write_verified_backup(&dirs, "bad.zip", |file| {
                file.write_all(b"partial ZIP")?;
                Err(torro_mail_backup::ArchiveError::Invalid("source read failed"))
            })
            .is_err()
        );
        assert!(
            write_verified_backup(&dirs, "invalid.zip", |file| {
                file.write_all(b"not ZIP")?;
                Ok(())
            })
            .is_err()
        );
        assert!(fs::read_dir(dirs.data.join("backups")).expect("backup folder").next().is_none());
        assert!(matches!(
            write_verified_backup(&dirs, "../outside.zip", |_| Ok(())),
            Err(BackupFileError::InvalidName)
        ));
    }

    #[test]
    fn cancellation_cannot_relabel_a_committed_archive() {
        let state = Arc::new(AtomicU8::new(RUNNING));
        let job = BackupJob {
            id: "job".into(),
            account: "mail:anna@example.org".into(),
            source: BackupScope::Server,
            phase: BackupPhase::Verifying,
            folders: 1,
            messages: 1,
            bytes: 4,
            archive: None,
            warnings: Vec::new(),
        };
        let jobs = BackupJobs {
            entries: Arc::new(Mutex::new(HashMap::from([(
                job.id.clone(),
                JobEntry { job, state: Arc::clone(&state) },
            )]))),
        };
        assert!(state.compare_exchange(RUNNING, COMMITTING, Ordering::AcqRel, Ordering::Acquire).is_ok());
        let current = jobs.cancel("mail:anna@example.org", "job").expect("the job exists");
        assert_eq!(current.phase, BackupPhase::Verifying);
        assert_eq!(state.load(Ordering::Acquire), COMMITTING);
        assert!(jobs.cancel("mail:other@example.org", "job").is_none());

        let pending = Arc::new(AtomicU8::new(RUNNING));
        assert!(pending.compare_exchange(RUNNING, CANCELLED, Ordering::AcqRel, Ordering::Acquire).is_ok());
        assert!(pending.compare_exchange(RUNNING, COMMITTING, Ordering::AcqRel, Ordering::Acquire).is_err());
    }

    #[test]
    fn uid_inventory_hash_does_not_depend_on_search_order() {
        let one = HashSet::from([9, 2, 5]);
        let another = HashSet::from([5, 9, 2]);
        assert_eq!(uid_inventory_hash(&one), uid_inventory_hash(&another));
        assert_ne!(uid_inventory_hash(&one), uid_inventory_hash(&HashSet::from([9, 2])));
    }

    #[tokio::test]
    async fn server_backup_reads_every_folder_and_large_original_without_writes() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let first = "From: a@example.org\r\nSubject: Grüße\r\n\r\nErster Text\r\n";
        let large = format!("From: b@example.org\r\nSubject: Large\r\n\r\n{}\r\n", "x".repeat(150_000));
        server.add("INBOX", first, &["\\Seen", "$Work"]);
        server.add("Archive", &large, &["\\Flagged"]);
        server.add_folder("All", "\\HasNoChildren \\All", 15);
        server.add("All", first, &["\\Seen"]);
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = Arc::new(service_in(root.path(), &secrets));
        mail.reload().await;
        let jobs = BackupJobs::default();
        let state = AtomicU8::new(RUNNING);
        let report = run_server_backup(
            mail,
            dirs(root.path()),
            &account.id,
            None,
            "all.zip",
            BackupProgress { jobs: &jobs, id: "test", state: &state },
        )
        .await
        .expect("complete server archive");
        assert!(report.complete);
        assert_eq!(report.folders, 5);
        assert_eq!(report.messages, 3);
        assert_eq!(report.bytes, (first.len() * 2 + large.len()) as u64);
        let lines = server.lines();
        assert!(lines.iter().any(|line| line.contains("EXAMINE")));
        assert!(lines.iter().any(|line| line.contains("BODY.PEEK[]<65536.65536>")));
        assert!(
            !lines
                .iter()
                .any(|line| { line.contains(" STORE ") || line.contains(" APPEND ") || line.contains(" CREATE ") })
        );
        assert!(root.path().join("data/mail/backups/all.zip").exists());
    }

    #[tokio::test]
    async fn server_backup_applies_inclusive_internal_date_filter() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", "From: a@example.org\r\n\r\nToday\r\n", &[]);
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = Arc::new(service_in(root.path(), &secrets));
        mail.reload().await;
        let jobs = BackupJobs::default();
        let state = AtomicU8::new(RUNNING);
        let today = Utc::now().date_naive();
        let next_state = AtomicU8::new(RUNNING);
        let report = run_server_backup(
            Arc::clone(&mail),
            dirs(root.path()),
            &account.id,
            Some(today),
            "today.zip",
            BackupProgress { jobs: &jobs, id: "today", state: &state },
        )
        .await
        .expect("today included");
        assert_eq!(report.messages, 1);
        let report = run_server_backup(
            mail,
            dirs(root.path()),
            &account.id,
            Some(today + Duration::days(1)),
            "tomorrow.zip",
            BackupProgress { jobs: &jobs, id: "tomorrow", state: &next_state },
        )
        .await
        .expect("tomorrow excludes today");
        assert_eq!(report.messages, 0);
    }

    #[tokio::test]
    async fn server_backup_catches_an_arrival_during_fetch_and_rejects_missing_body() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", "From: a@example.org\r\n\r\nFirst\r\n", &[]);
        server.add_on_backup_chunk("INBOX", "From: b@example.org\r\n\r\nLater\r\n");
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = Arc::new(service_in(root.path(), &secrets));
        mail.reload().await;
        let jobs = BackupJobs::default();
        let state = AtomicU8::new(RUNNING);
        let report = run_server_backup(
            Arc::clone(&mail),
            dirs(root.path()),
            &account.id,
            None,
            "arrival.zip",
            BackupProgress { jobs: &jobs, id: "arrival", state: &state },
        )
        .await
        .expect("arrival caught up");
        assert_eq!(report.messages, 2);

        let uid = server.add("Archive", "From: c@example.org\r\n\r\nGone\r\n", &[]);
        server.withhold_text("Archive", uid);
        let state = AtomicU8::new(RUNNING);
        assert!(
            run_server_backup(
                mail,
                dirs(root.path()),
                &account.id,
                None,
                "missing.zip",
                BackupProgress { jobs: &jobs, id: "missing", state: &state },
            )
            .await
            .is_err()
        );
        assert!(!root.path().join("data/mail/backups/missing.zip").exists());
    }

    #[tokio::test]
    async fn local_backup_works_with_no_reachable_imap_server_and_reads_originals_in_chunks() {
        use torro_mail::{Endpoint, Security};

        let root = tempfile::tempdir().expect("tempdir");
        let endpoint = Endpoint { host: "127.0.0.1".into(), port: 9, security: Security::Tls };
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = Arc::new(service_in(root.path(), &secrets));
        mail.reload().await;
        let original = format!("From: a@example.org\r\n\r\n{}\r\n", "x".repeat(140_000));
        let now = Utc::now();
        let store = mail.store(&account.id).await.expect("local store");
        store
            .run(move |db| {
                db.save_original(
                    "INBOX",
                    1,
                    5,
                    now.fixed_offset(),
                    &["\\Seen".into(), "$Work".into()],
                    original.as_bytes(),
                )
            })
            .await
            .expect("original retained");
        let jobs = BackupJobs::default();
        let state = AtomicU8::new(RUNNING);
        let report = run_local_backup(
            mail,
            dirs(root.path()),
            &account.id,
            None,
            "local.zip",
            BackupProgress { jobs: &jobs, id: "local", state: &state },
        )
        .await
        .expect("offline local archive");
        assert!(report.complete);
        assert_eq!(report.messages, 1);
        assert!(report.bytes > 140_000);
        assert!(root.path().join("data/mail/backups/local.zip").exists());
    }
}
