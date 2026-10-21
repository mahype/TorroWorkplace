//! Publish a verified mail ZIP in the core's private backup directory without exposing partial files.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use torro_core::i18n::tr;
use torro_ipc::mail::{BackupJob, BackupPhase, BackupScope, RestoreFolderPlan, RestorePlan, RestoreResult};
use torro_mail_backup::format::{Account, Counts, FORMAT, Folder, Inventory, Manifest, Source, Status, VERSION};
use torro_mail_backup::restore::RestoreArchive;
use torro_mail_backup::writer::{ArchiveWriter, MessageMetadata};
use torro_mail_backup::{ArchiveError, VerifiedArchive, verify_archive};
use torro_mail_imap::folders;
use torro_mail_imap::imap::{AppendUid, ImapError, Search, backup_flag_transferable};

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
    #[cfg(test)]
    pub(crate) fn new(dirs: &MailDirs, filename: &str) -> Result<Self, BackupFileError> {
        Self::new_in(dirs, filename, None)
    }

    pub(crate) fn new_in(dirs: &MailDirs, filename: &str, selected: Option<&Path>) -> Result<Self, BackupFileError> {
        if !valid_filename(filename) {
            return Err(BackupFileError::InvalidName);
        }
        let backup_dir = if let Some(selected) = selected {
            if !selected.is_absolute() {
                return Err(BackupFileError::InvalidName);
            }
            reject_symlink(selected)?;
            let path = selected.canonicalize()?;
            if !path.is_dir() {
                return Err(BackupFileError::InvalidName);
            }
            path
        } else {
            reject_symlink(&dirs.data)?;
            crate::clients::create_private_dir(&dirs.data)?;
            let path = dirs.data.join("backups");
            reject_symlink(&path)?;
            crate::clients::create_private_dir(&path)?;
            path
        };
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
            Self::Incomplete(reason) => match *reason {
                "message disappeared during metadata fetch"
                | "message disappeared during body fetch"
                | "messages disappeared during backup" => {
                    tr("Mail backup is incomplete: a source message disappeared while reading.").into()
                }
                "server returned a short body chunk" => {
                    tr("Mail backup is incomplete: the server returned an incomplete message body.").into()
                }
                "source returned a different UID"
                | "UIDVALIDITY changed during backup"
                | "UIDVALIDITY changed during final inventory" => {
                    tr("Mail backup is incomplete: IMAP message identifiers changed.").into()
                }
                "folder kept changing during backup"
                | "folder list changed during backup"
                | "messages changed during final inventory" => {
                    tr("Mail backup is incomplete: the folder inventory changed while reading.").into()
                }
                "local original disappeared" | "local original was truncated" | "local original hash changed" => {
                    tr("Mail backup is incomplete: a local original is missing or damaged.").into()
                }
                "local folder changed" | "local originals changed during backup" => {
                    tr("Mail backup is incomplete: the local mail inventory changed while reading.").into()
                }
                _ => tr("Mail backup is incomplete because the source changed.").into(),
            },
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
    owner: Option<String>,
}

/// In-process progress and cancellation. A clean shutdown drops partial files through
/// `NamedTempFile`; after a crash, hidden partial files may remain but cannot claim completion.
#[derive(Clone, Default)]
pub(crate) struct BackupJobs {
    entries: Arc<Mutex<HashMap<String, JobEntry>>>,
}

pub(crate) struct BackupSelection {
    pub source: BackupScope,
    pub since: Option<NaiveDate>,
    pub filename: Option<String>,
    pub target_dir: Option<PathBuf>,
}

impl BackupJobs {
    pub(crate) fn start(
        &self,
        mail: Arc<MailService>,
        dirs: MailDirs,
        account: String,
        selection: BackupSelection,
        owner: Option<String>,
    ) -> Result<BackupJob, BackupRunError> {
        let BackupSelection { source, since, filename, target_dir } = selection;
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
            entries.insert(id.clone(), JobEntry { job: job.clone(), state: Arc::clone(&state), owner });
        }
        let jobs = self.clone();
        tokio::spawn(async move {
            let progress = BackupProgress { jobs: &jobs, id: &id, state: &state };
            let result = match source {
                BackupScope::Server => {
                    run_server_backup(mail, dirs, &account, since, &filename, target_dir.as_deref(), progress).await
                }
                BackupScope::Local => {
                    run_local_backup(mail, dirs, &account, since, &filename, target_dir.as_deref(), progress).await
                }
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

    pub(crate) fn status(&self, account: &str, id: &str, caller: Option<&str>) -> Option<BackupJob> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .filter(|entry| entry.job.account == account && (caller.is_none() || entry.owner.as_deref() == caller))
            .map(|entry| entry.job.clone())
    }

    pub(crate) fn cancel(&self, account: &str, id: &str, caller: Option<&str>) -> Option<BackupJob> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries
            .get_mut(id)
            .filter(|entry| entry.job.account == account && (caller.is_none() || entry.owner.as_deref() == caller))?;
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
    target_dir: Option<&Path>,
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
    let mut warnings = Vec::new();
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
        if !selectable {
            let reason = if folder.has_attribute("\\NonExistent") { "\\NonExistent" } else { "\\Noselect" };
            warnings.push(format!(
                "{}: {} ({reason})",
                tr("The IMAP server marks this folder as non-selectable; its messages cannot be inventoried"),
                folder.path
            ));
        }
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
        warnings,
    };
    jobs.progress(job_id, manifest.counts.folders, 0, 0);
    let mut pending = PendingBackup::new_in(&dirs, filename, target_dir)?;
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
    let final_selectability: HashMap<String, bool> = final_list
        .into_iter()
        .map(|folder| {
            let selectable = folder.selectable();
            (folder.path, selectable)
        })
        .collect();
    let initial_selectability: HashMap<String, bool> =
        writer.manifest_mut().folders.iter().map(|folder| (folder.path.clone(), folder.selectable)).collect();
    if final_selectability != initial_selectability {
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
    target_dir: Option<&Path>,
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
    let mut pending = PendingBackup::new_in(&dirs, filename, target_dir)?;
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

#[derive(Clone, PartialEq, Eq)]
struct TargetFolder {
    uid_validity: u32,
    uids: HashSet<u32>,
    hashes: HashMap<String, u64>,
    metadata: HashMap<String, Vec<TargetMetadata>>,
}

#[derive(Clone, PartialEq, Eq)]
struct TargetMetadata {
    internal_date: DateTime<FixedOffset>,
    flags: Vec<String>,
}

fn transferable_flags(flags: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = flags
        .iter()
        .filter(|flag| backup_flag_transferable(flag))
        .map(|flag| {
            ["\\Seen", "\\Answered", "\\Flagged", "\\Draft"]
                .iter()
                .find(|system| flag.eq_ignore_ascii_case(system))
                .map_or_else(|| flag.clone(), |system| (*system).into())
        })
        .collect();
    kept.sort();
    kept
}

#[derive(Clone, PartialEq, Eq)]
struct TargetSnapshot {
    existing: HashSet<String>,
    selectable: HashMap<String, TargetFolder>,
}

fn validate_restore_paths(manifest: &Manifest) -> Result<(), BackupRunError> {
    if manifest
        .folders
        .iter()
        .any(|folder| folder.path.len() > 4096 || folder.path.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)))
    {
        return Err(BackupRunError::Incomplete("unsafe restore folder name"));
    }
    Ok(())
}

/// Read-only target inventory. Every target body is hashed by partial `BODY.PEEK[]`; a
/// Message-ID alone cannot establish whether an archived occurrence is already present.
async fn inspect_restore_target(
    mail: &MailService,
    account: &str,
    manifest: &Manifest,
) -> Result<TargetSnapshot, BackupRunError> {
    let mut command = mail.command(account).await?;
    let session = command.session();
    let listed = session.list().await?;
    let existing: HashSet<String> = listed.iter().map(|folder| folder.path.clone()).collect();
    let mut selectable = HashMap::new();
    for archived in &manifest.folders {
        if !archived.selectable || !existing.contains(&archived.path) {
            continue;
        }
        let Some(target) = listed.iter().find(|folder| folder.path == archived.path) else {
            continue;
        };
        if !target.selectable() {
            continue;
        }
        let selected = session.examine(&archived.path).await?;
        let uids: HashSet<u32> = session.uid_search(&Search::All).await?.into_iter().collect();
        let mut ordered: Vec<u32> = uids.iter().copied().collect();
        ordered.sort_unstable();
        let mut hashes = HashMap::new();
        let mut by_hash: HashMap<String, Vec<TargetMetadata>> = HashMap::new();
        for uid in ordered {
            let metadata = session.fetch_backup_metadata(&[uid]).await?;
            let [metadata] = metadata.as_slice() else {
                return Err(BackupRunError::Incomplete("target message changed while reading"));
            };
            if metadata.uid != uid {
                return Err(BackupRunError::Incomplete("target returned another UID"));
            }
            let hash = hash_imap_message(session, uid, metadata.bytes).await?;
            *hashes.entry(hash.clone()).or_insert(0) += 1;
            by_hash.entry(hash).or_default().push(TargetMetadata {
                internal_date: metadata.internal_date,
                flags: transferable_flags(&metadata.flags),
            });
        }
        let checked = session.examine(&archived.path).await?;
        let final_uids: HashSet<u32> = session.uid_search(&Search::All).await?.into_iter().collect();
        if checked.uid_validity != selected.uid_validity || final_uids != uids {
            return Err(BackupRunError::Incomplete("target folder changed while reading"));
        }
        selectable.insert(
            archived.path.clone(),
            TargetFolder { uid_validity: selected.uid_validity, uids, hashes, metadata: by_hash },
        );
    }
    let final_paths: HashSet<String> = session.list().await?.into_iter().map(|folder| folder.path).collect();
    if final_paths != existing {
        return Err(BackupRunError::Incomplete("target folder list changed while reading"));
    }
    command.release();
    Ok(TargetSnapshot { existing, selectable })
}

async fn hash_imap_message(
    session: &mut torro_mail_imap::imap::Session,
    uid: u32,
    bytes: u64,
) -> Result<String, BackupRunError> {
    let mut hash = Sha256::new();
    let mut offset = 0u64;
    while offset < bytes {
        let count = (bytes - offset).min(64 * 1024) as usize;
        let Some(chunk) = session.fetch_backup_chunk(uid, offset, count).await? else {
            return Err(BackupRunError::Incomplete("target message disappeared"));
        };
        if chunk.len() != count {
            return Err(BackupRunError::Incomplete("target message was truncated"));
        }
        hash.update(&chunk);
        offset += count as u64;
    }
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// A cheap inventory check before each write. The expensive raw hashes are established at plan
/// time; stable UIDVALIDITY and UID sets mean those same occurrences remain in the folder.
async fn target_folder_unchanged(
    mail: &MailService,
    account: &str,
    path: &str,
    expected: &TargetFolder,
) -> Result<bool, BackupRunError> {
    let mut command = mail.command(account).await?;
    let session = command.session();
    let selected = session.examine(path).await?;
    let uids: HashSet<u32> = session.uid_search(&Search::All).await?.into_iter().collect();
    command.release();
    Ok(selected.uid_validity == expected.uid_validity && uids == expected.uids)
}

/// Confirm exactly one new UID and hash its complete RFC-822 body. UIDPLUS identifies the UID
/// directly when present; without it the difference from the pre-APPEND inventory identifies it.
/// Concurrent target changes leave the journal pending instead of guessing which mail landed.
async fn confirm_restore_append(
    mail: &MailService,
    account: &str,
    path: &str,
    before: &TargetFolder,
    append_uid: Option<AppendUid>,
    expected_hash: &str,
    expected_bytes: u64,
) -> Result<TargetFolder, BackupRunError> {
    let mut command = mail.command(account).await?;
    let session = command.session();
    let selected = session.examine(path).await?;
    if selected.uid_validity != before.uid_validity
        || append_uid.is_some_and(|appended| appended.uid_validity != selected.uid_validity)
    {
        return Err(BackupRunError::Incomplete("restore target UIDVALIDITY changed"));
    }
    let current: HashSet<u32> = session.uid_search(&Search::All).await?.into_iter().collect();
    if !before.uids.is_subset(&current) {
        return Err(BackupRunError::Incomplete("restore target lost a UID after append"));
    }
    let added: Vec<u32> = current.difference(&before.uids).copied().collect();
    let [uid] = added.as_slice() else {
        return Err(BackupRunError::Incomplete("restore append has no unique new UID"));
    };
    if append_uid.is_some_and(|appended| appended.uid != *uid) {
        return Err(BackupRunError::Incomplete("restore APPENDUID disagrees with target"));
    }
    let metadata = session.fetch_backup_metadata(&[*uid]).await?;
    let [metadata] = metadata.as_slice() else {
        return Err(BackupRunError::Incomplete("restored mail disappeared before verification"));
    };
    if metadata.uid != *uid
        || metadata.bytes != expected_bytes
        || hash_imap_message(session, *uid, metadata.bytes).await? != expected_hash
    {
        return Err(BackupRunError::Incomplete("restored mail differs from archive"));
    }
    let mut after = before.clone();
    after.uids = current;
    *after.hashes.entry(expected_hash.into()).or_default() += 1;
    after
        .metadata
        .entry(expected_hash.into())
        .or_default()
        .push(TargetMetadata { internal_date: metadata.internal_date, flags: transferable_flags(&metadata.flags) });
    command.release();
    Ok(after)
}

fn summarize_restore(manifest: &Manifest, target: &TargetSnapshot, id: String, account: &str) -> RestorePlan {
    let mut available: HashMap<String, HashMap<String, u64>> =
        target.selectable.iter().map(|(path, folder)| (path.clone(), folder.hashes.clone())).collect();
    let mut available_metadata: HashMap<String, HashMap<String, Vec<TargetMetadata>>> =
        target.selectable.iter().map(|(path, folder)| (path.clone(), folder.metadata.clone())).collect();
    let paths: HashMap<&str, usize> =
        manifest.folders.iter().enumerate().map(|(at, folder)| (folder.id.as_str(), at)).collect();
    let mut plan = RestorePlan {
        id,
        account: account.into(),
        complete_archive: manifest.status == Status::Complete,
        folders_to_create: 0,
        messages_to_add: 0,
        already_present: 0,
        uncertain: 0,
        bytes_to_add: 0,
        folders: Vec::with_capacity(manifest.folders.len()),
        warnings: Vec::new(),
    };
    if manifest.status != Status::Complete {
        plan.warnings.push(tr("This archive is incomplete; restoring it needs separate confirmation.").into());
    }
    for folder in &manifest.folders {
        let create = folder.selectable && !target.existing.contains(&folder.path);
        plan.folders.push(RestoreFolderPlan {
            archive_path: folder.path.clone(),
            target_path: folder.path.clone(),
            create,
            messages_to_add: 0,
            already_present: 0,
            uncertain: 0,
            bytes_to_add: 0,
        });
        if create {
            plan.folders_to_create += 1;
            plan.warnings.push(format!("{} {}", tr("Restore will create folder:"), folder.path));
        }
    }
    let mut dropped_flags = 0u64;
    let mut different_metadata = 0u64;
    for message in &manifest.messages {
        let Some(&index) = paths.get(message.folder_id.as_str()) else {
            plan.uncertain += 1;
            continue;
        };
        let folder = &manifest.folders[index];
        let row = &mut plan.folders[index];
        dropped_flags += message.flags.iter().filter(|flag| !backup_flag_transferable(flag)).count() as u64;
        if target.existing.contains(&folder.path) && !target.selectable.contains_key(&folder.path) {
            plan.uncertain += 1;
            row.uncertain += 1;
        } else if let Some(count) = available.get_mut(&folder.path).and_then(|hashes| hashes.get_mut(&message.sha256))
            && *count > 0
        {
            *count -= 1;
            plan.already_present += 1;
            row.already_present += 1;
            if let Some(details) =
                available_metadata.get_mut(&folder.path).and_then(|hashes| hashes.get_mut(&message.sha256))
            {
                let flags = transferable_flags(&message.flags);
                let matching = details.iter().position(|existing| {
                    message.internal_date.is_none_or(|date| date.timestamp() == existing.internal_date.timestamp())
                        && existing.flags == flags
                });
                if let Some(index) = matching {
                    details.swap_remove(index);
                } else {
                    details.pop();
                    different_metadata += 1;
                }
            }
        } else {
            plan.messages_to_add += 1;
            plan.bytes_to_add = plan.bytes_to_add.saturating_add(message.bytes);
            row.messages_to_add += 1;
            row.bytes_to_add = row.bytes_to_add.saturating_add(message.bytes);
        }
    }
    if dropped_flags > 0 {
        plan.warnings.push(format!("{} {dropped_flags}", tr("Nontransferable IMAP flags will be omitted:")));
    }
    if different_metadata > 0 {
        plan.warnings.push(format!(
            "{} {different_metadata}",
            tr("Existing mail has different date or flags; it will not be changed:")
        ));
    }
    if plan.uncertain > 0 {
        plan.warnings.push(tr("Some destination folders cannot be read or selected.").into());
    }
    plan
}

struct PlannedRestore {
    file: String,
    account: String,
    manifest_sha256: String,
    plan: RestorePlan,
    target: TargetSnapshot,
    created: std::time::Instant,
}

fn target_preserves_plan(before: &TargetSnapshot, current: &TargetSnapshot) -> bool {
    before.existing.is_subset(&current.existing)
        && before.selectable.iter().all(|(path, old)| {
            current.selectable.get(path).is_some_and(|now| {
                now.uid_validity == old.uid_validity
                    && old.uids.is_subset(&now.uids)
                    && old.hashes.iter().all(|(hash, count)| now.hashes.get(hash).is_some_and(|found| found >= count))
            })
        })
}

#[derive(Clone, Default)]
pub(crate) struct RestorePlans {
    entries: Arc<Mutex<HashMap<String, PlannedRestore>>>,
    apply_lock: Arc<tokio::sync::Mutex<()>>,
}

fn manifest_digest(manifest: &Manifest) -> Result<String, BackupRunError> {
    let encoded =
        serde_json::to_vec(manifest).map_err(|_| BackupRunError::Incomplete("archive manifest cannot be encoded"))?;
    Ok(Sha256::digest(encoded).iter().map(|byte| format!("{byte:02x}")).collect())
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RestoreJournal {
    account: String,
    manifest_sha256: String,
    /// The observed number of copies before an APPEND whose result was not yet confirmed.
    pending: BTreeMap<String, u64>,
}

impl RestoreJournal {
    fn path(dirs: &MailDirs, account: &str, digest: &str) -> Result<PathBuf, BackupRunError> {
        reject_symlink(&dirs.data)?;
        crate::clients::create_private_dir(&dirs.data)?;
        let directory = dirs.data.join("restore-journals");
        reject_symlink(&directory)?;
        crate::clients::create_private_dir(&directory)?;
        let key = Sha256::digest(format!("{account}\0{digest}").as_bytes());
        let name: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(directory.join(format!("{name}.json")))
    }

    fn open(path: &Path, account: &str, digest: &str) -> Result<Self, BackupRunError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 10 * 1024 * 1024 {
                    return Err(BackupRunError::Incomplete("restore journal is unsafe or oversized"));
                }
                let journal: Self = serde_json::from_slice(&fs::read(path)?)
                    .map_err(|_| BackupRunError::Incomplete("restore journal is damaged"))?;
                if journal.account != account || journal.manifest_sha256 != digest {
                    return Err(BackupRunError::Incomplete("restore journal belongs to another archive"));
                }
                Ok(journal)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(Self { account: account.into(), manifest_sha256: digest.into(), pending: BTreeMap::new() })
            }
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, path: &Path) -> Result<(), BackupRunError> {
        let directory = path.parent().ok_or(BackupRunError::Incomplete("restore journal has no directory"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(temporary.as_file_mut(), self)
            .map_err(|_| BackupRunError::Incomplete("restore journal cannot be written"))?;
        temporary.as_file_mut().sync_all()?;
        temporary.persist(path).map_err(|error| BackupRunError::Io(error.error))?;
        #[cfg(unix)]
        File::open(directory)?.sync_all()?;
        Ok(())
    }
}

impl RestorePlans {
    pub(crate) async fn plan(
        &self,
        mail: &MailService,
        file: String,
        path: &Path,
        account: &str,
    ) -> Result<RestorePlan, BackupRunError> {
        let opened = File::open(path)?;
        let archive = RestoreArchive::open(opened)?;
        validate_restore_paths(archive.manifest())?;
        if account.strip_prefix(torro_mail::ACCOUNT_PREFIX) != Some(archive.manifest().account.address.as_str()) {
            return Err(BackupRunError::Incomplete("restore account does not match archive"));
        }
        let target = inspect_restore_target(mail, account, archive.manifest()).await?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| BackupRunError::Random)?;
        let id = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let plan = summarize_restore(archive.manifest(), &target, id.clone(), account);
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|_, item| item.created.elapsed() < std::time::Duration::from_secs(3600));
        if entries.len() >= 128 {
            return Err(BackupRunError::Busy);
        }
        entries.insert(
            id,
            PlannedRestore {
                file,
                account: account.into(),
                manifest_sha256: manifest_digest(archive.manifest())?,
                plan: plan.clone(),
                target,
                created: std::time::Instant::now(),
            },
        );
        Ok(plan)
    }

    pub(crate) fn file_for(&self, account: &str, id: &str) -> Option<String> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .filter(|item| item.account == account && item.created.elapsed() < std::time::Duration::from_secs(3600))
            .map(|item| item.file.clone())
    }

    pub(crate) fn planned_for(&self, account: &str, id: &str) -> Option<(String, RestorePlan)> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .filter(|item| item.account == account && item.created.elapsed() < std::time::Duration::from_secs(3600))
            .map(|item| (item.file.clone(), item.plan.clone()))
    }

    /// Reverify the exact planned archive and target before writing. Only an explicitly requested
    /// apply reaches this method; a dry run never makes a journal or sends IMAP write commands.
    pub(crate) async fn apply(
        &self,
        mail: &MailService,
        dirs: &MailDirs,
        path: &Path,
        account: &str,
        id: &str,
        allow_incomplete: bool,
    ) -> Result<RestoreResult, BackupRunError> {
        let _serial = self.apply_lock.lock().await;
        let (planned_digest, planned_plan, planned_target) = {
            let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
            let item = entries.get(id).filter(|item| {
                item.account == account && item.created.elapsed() < std::time::Duration::from_secs(3600)
            });
            let item = item.ok_or(BackupRunError::Incomplete("restore plan expired"))?;
            (item.manifest_sha256.clone(), item.plan.clone(), item.target.clone())
        };
        let mut archive = RestoreArchive::open(File::open(path)?)?;
        let manifest = archive.manifest().clone();
        validate_restore_paths(&manifest)?;
        if manifest_digest(&manifest)? != planned_digest
            || account.strip_prefix(torro_mail::ACCOUNT_PREFIX) != Some(manifest.account.address.as_str())
        {
            return Err(BackupRunError::Incomplete("planned restore archive changed"));
        }
        if manifest.status != Status::Complete && !allow_incomplete {
            return Err(BackupRunError::Incomplete("incomplete restore archive was not confirmed"));
        }
        let mut target = inspect_restore_target(mail, account, &manifest).await?;
        let preview = summarize_restore(&manifest, &target, id.into(), account);
        if !target_preserves_plan(&planned_target, &target)
            || preview.folders_to_create > planned_plan.folders_to_create
            || preview.messages_to_add > planned_plan.messages_to_add
            || preview.folders.iter().zip(&planned_plan.folders).any(|(now, old)| {
                now.target_path != old.target_path
                    || (now.create && !old.create)
                    || now.messages_to_add > old.messages_to_add
            })
        {
            return Err(BackupRunError::Incomplete("restore target changed after the reviewed plan"));
        }
        if preview.uncertain > 0 {
            return Ok(RestoreResult {
                added: 0,
                skipped: 0,
                uncertain: preview.uncertain,
                warnings: preview.warnings,
            });
        }
        let journal_path = RestoreJournal::path(dirs, account, &planned_digest)?;
        let mut journal = RestoreJournal::open(&journal_path, account, &planned_digest)?;
        let mut result = RestoreResult { added: 0, skipped: 0, uncertain: 0, warnings: preview.warnings };
        for folder in &manifest.folders {
            if folder.selectable && !target.existing.contains(&folder.path) {
                let mut command = mail.command(account).await?;
                command.session().create_folder(&folder.path, None).await?;
                command.release();
                let refreshed = inspect_restore_target(mail, account, &manifest).await?;
                if !target_preserves_plan(&target, &refreshed) || !target_preserves_plan(&planned_target, &refreshed) {
                    return Err(BackupRunError::Incomplete("restore target changed after creating a folder"));
                }
                target = refreshed;
                if !target.selectable.contains_key(&folder.path) {
                    return Err(BackupRunError::Incomplete("created restore folder is not selectable"));
                }
            }
        }
        let paths: HashMap<&str, &str> =
            manifest.folders.iter().map(|folder| (folder.id.as_str(), folder.path.as_str())).collect();
        let mut consumed: HashMap<(String, String), u64> = HashMap::new();
        for (index, message) in manifest.messages.iter().enumerate() {
            let folder = paths
                .get(message.folder_id.as_str())
                .ok_or(BackupRunError::Incomplete("archive folder disappeared"))?;
            let key = ((*folder).to_string(), message.sha256.clone());
            let count = target
                .selectable
                .get(*folder)
                .and_then(|folder| folder.hashes.get(&message.sha256))
                .copied()
                .unwrap_or(0);
            let used = consumed.entry(key).or_default();
            if let Some(before) = journal.pending.get(&message.path).copied() {
                if count != before.saturating_add(1) {
                    result.uncertain += 1;
                    result
                        .warnings
                        .push(tr("An earlier mail upload is unconfirmed; inspect the target before retrying.").into());
                    break;
                }
                journal.pending.remove(&message.path);
                journal.save(&journal_path)?;
            }
            if count > *used {
                *used += 1;
                result.skipped += 1;
                continue;
            }
            let mut spool = tempfile::tempfile_in(
                journal_path.parent().ok_or(BackupRunError::Incomplete("restore spool directory missing"))?,
            )?;
            archive.copy_message(index, &mut spool)?;
            spool.seek(SeekFrom::Start(0))?;
            let flags = transferable_flags(&message.flags);
            let before = target
                .selectable
                .get(*folder)
                .cloned()
                .ok_or(BackupRunError::Incomplete("restore target folder disappeared"))?;
            if !target_folder_unchanged(mail, account, folder, &before).await? {
                return Err(BackupRunError::Incomplete("target changed before restore append"));
            }
            journal.pending.insert(message.path.clone(), count);
            journal.save(&journal_path)?;
            let append = async {
                let mut command = mail.command(account).await?;
                let appended = command
                    .session()
                    .append_backup(folder, &mut spool, message.bytes, &flags, message.internal_date)
                    .await?;
                command.release();
                Ok::<Option<AppendUid>, BackupRunError>(appended)
            }
            .await;
            let appended = match append {
                Ok(appended) => appended,
                Err(_) => {
                    result.uncertain += 1;
                    result.warnings.push(
                        tr("Mail upload was not confirmed; the restore journal prevents an automatic duplicate.")
                            .into(),
                    );
                    break;
                }
            };
            let checked =
                confirm_restore_append(mail, account, folder, &before, appended, &message.sha256, message.bytes).await;
            match checked {
                Ok(next) => {
                    target.selectable.insert((*folder).into(), next);
                    journal.pending.remove(&message.path);
                    journal.save(&journal_path)?;
                    *used += 1;
                    result.added += 1;
                }
                _ => {
                    result.uncertain += 1;
                    result.warnings.push(
                        tr("Mail upload could not be checked; the restore journal prevents an automatic duplicate.")
                            .into(),
                    );
                    break;
                }
            }
        }
        Ok(result)
    }
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
        BackupFileError, BackupJobs, BackupProgress, BackupRunError, CANCELLED, COMMITTING, JobEntry, PendingBackup,
        RUNNING, RestoreJournal, RestorePlans, manifest_digest, run_local_backup, run_server_backup,
        uid_inventory_hash, write_verified_backup,
    };
    use crate::account::github::tests::MemoryTokens;
    use crate::mail::MailDirs;
    use crate::mail::service::tests::{PASSWORD, mail_account, service_in, store_accounts};
    use crate::mail::sync::fake::FakeImap;

    #[test]
    fn incomplete_backup_reports_the_specific_source_problem() {
        let gone = BackupRunError::Incomplete("message disappeared during body fetch").user_message();
        let changed = BackupRunError::Incomplete("UIDVALIDITY changed during backup").user_message();
        let short = BackupRunError::Incomplete("server returned a short body chunk").user_message();
        assert_ne!(gone, changed);
        assert_ne!(gone, short);
        assert_ne!(changed, short);
    }

    #[test]
    fn transferable_system_flags_use_canonical_spelling() {
        assert_eq!(
            super::transferable_flags(&["\\seen".into(), "\\FLAGGED".into(), "$Work".into()]),
            vec!["$Work", "\\Flagged", "\\Seen"]
        );
    }

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
    fn selected_existing_folder_publishes_verified_archive_there() {
        let root = tempfile::tempdir().expect("tempdir");
        let selected = root.path().join("chosen");
        fs::create_dir(&selected).expect("selected folder");
        let mut pending = PendingBackup::new_in(&dirs(root.path()), "selected.zip", Some(&selected)).expect("pending");
        pending.file_mut().write_all(SAMPLE_ZIP).expect("write sample");
        let (path, report) = pending.publish().expect("published");
        assert!(report.complete);
        assert_eq!(path, selected.canonicalize().expect("canonical selected folder").join("selected.zip"));
        assert_eq!(fs::read(path).expect("archive"), SAMPLE_ZIP);
        assert!(!root.path().join("data/mail/backups").exists());
        let mut duplicate =
            PendingBackup::new_in(&dirs(root.path()), "selected.zip", Some(&selected)).expect("pending");
        duplicate.file_mut().write_all(SAMPLE_ZIP).expect("write duplicate");
        assert!(duplicate.publish().is_err(), "a selected archive is not overwritten");
        #[cfg(unix)]
        {
            let link = root.path().join("link");
            std::os::unix::fs::symlink(&selected, &link).expect("directory link");
            assert!(PendingBackup::new_in(&dirs(root.path()), "link.zip", Some(&link)).is_err());
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
                JobEntry { job, state: Arc::clone(&state), owner: Some("client-1".into()) },
            )]))),
        };
        assert!(state.compare_exchange(RUNNING, COMMITTING, Ordering::AcqRel, Ordering::Acquire).is_ok());
        let current = jobs.cancel("mail:anna@example.org", "job", Some("client-1")).expect("the job exists");
        assert_eq!(current.phase, BackupPhase::Verifying);
        assert_eq!(state.load(Ordering::Acquire), COMMITTING);
        assert!(jobs.cancel("mail:other@example.org", "job", Some("client-1")).is_none());
        assert!(jobs.status("mail:anna@example.org", "job", Some("client-2")).is_none());
        assert!(jobs.cancel("mail:anna@example.org", "job", Some("client-2")).is_none());
        assert!(jobs.status("mail:anna@example.org", "job", None).is_some(), "suite sees the job");

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
            None,
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
    async fn server_backup_reports_nonselectable_folders_in_the_archive() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add_folder("Container", "\\Noselect \\HasChildren", 15);
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
            "containers.zip",
            None,
            BackupProgress { jobs: &jobs, id: "containers", state: &state },
        )
        .await
        .expect("backup");
        assert!(report.complete);
        assert!(report.warnings.iter().any(|warning| warning.contains("Container") && warning.contains("\\Noselect")));
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
            None,
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
            None,
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
            None,
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
                None,
                BackupProgress { jobs: &jobs, id: "missing", state: &state },
            )
            .await
            .is_err()
        );
        assert!(!root.path().join("data/mail/backups/missing.zip").exists());
    }

    #[tokio::test]
    async fn server_backup_rejects_a_folder_that_becomes_selectable_mid_run() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", "From: a@example.org\r\n\r\nFirst\r\n", &[]);
        server.make_selectable_on_backup_chunk("Archive");
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = Arc::new(service_in(root.path(), &secrets));
        mail.reload().await;
        let jobs = BackupJobs::default();
        let state = AtomicU8::new(RUNNING);
        let outcome = run_server_backup(
            mail,
            dirs(root.path()),
            &account.id,
            None,
            "changed.zip",
            None,
            BackupProgress { jobs: &jobs, id: "changed", state: &state },
        )
        .await;
        assert!(outcome.is_err(), "newly selectable folder invalidates the first inventory");
        assert!(!root.path().join("data/mail/backups/changed.zip").exists());
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
        let mut spool = tempfile::tempfile().expect("spool");
        spool.write_all(original.as_bytes()).expect("write original");
        let now = Utc::now();
        let store = mail.store(&account.id).await.expect("local store");
        store
            .run(move |db| {
                db.save_original_file("INBOX", 1, 5, now.fixed_offset(), &["\\Seen".into(), "$Work".into()], &mut spool)
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
            None,
            BackupProgress { jobs: &jobs, id: "local", state: &state },
        )
        .await
        .expect("offline local archive");
        assert!(report.complete);
        assert_eq!(report.messages, 1);
        assert!(report.bytes > 140_000);
        assert!(root.path().join("data/mail/backups/local.zip").exists());
    }

    #[tokio::test]
    async fn restore_plan_reads_target_without_writing_and_matches_original_hashes() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let path = root.path().join("sample.zip");
        fs::write(&path, SAMPLE_ZIP).expect("sample");
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "sample.zip".into(), &path, &account.id).await.expect("plan");
        assert_eq!((plan.messages_to_add, plan.already_present, plan.uncertain), (1, 0, 0));
        let inbox = plan.folders.iter().find(|folder| folder.archive_path == "INBOX").expect("mapped inbox");
        assert_eq!((inbox.target_path.as_str(), inbox.messages_to_add, inbox.create), ("INBOX", 1, false));
        assert_eq!(plans.file_for(&account.id, &plan.id).as_deref(), Some("sample.zip"));
        let mut archive = torro_mail_backup::restore::RestoreArchive::open(std::io::Cursor::new(SAMPLE_ZIP))
            .expect("verified sample");
        let mut raw = Vec::new();
        archive.copy_message(0, &mut raw).expect("original");
        server.add("INBOX", &String::from_utf8(raw).expect("sample is text"), &["\\Seen"]);
        let plan = plans.plan(&mail, "sample.zip".into(), &path, &account.id).await.expect("plan again");
        assert_eq!((plan.messages_to_add, plan.already_present, plan.uncertain), (0, 1, 0));
        assert_eq!(plan.warnings.len(), 1, "existing metadata differs without changing the raw EML");
        let lines = server.lines();
        assert!(lines.iter().any(|line| line.contains("EXAMINE")));
        assert!(
            !lines
                .iter()
                .any(|line| line.contains(" APPEND ") || line.contains(" CREATE ") || line.contains(" STORE "))
        );
        assert!(!root.path().join("data/mail/restore-journals").exists(), "dry run makes no journal");
    }

    #[tokio::test]
    async fn uncertain_append_is_not_repeated_and_a_later_matching_copy_resolves_it() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let path = root.path().join("sample.zip");
        fs::write(&path, SAMPLE_ZIP).expect("sample");
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "sample.zip".into(), &path, &account.id).await.expect("plan");
        let archive = torro_mail_backup::restore::RestoreArchive::open(std::io::Cursor::new(SAMPLE_ZIP))
            .expect("verified sample");
        let digest = manifest_digest(archive.manifest()).expect("digest");
        let journal_path = RestoreJournal::path(&dirs(root.path()), &account.id, &digest).expect("private path");
        let mut journal = RestoreJournal::open(&journal_path, &account.id, &digest).expect("new journal");
        journal.pending.insert(archive.manifest().messages[0].path.clone(), 0);
        journal.save(&journal_path).expect("pending before append");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&journal_path).expect("journal mode").permissions().mode() & 0o777, 0o600);
        }
        let outcome =
            plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("safe stop");
        assert_eq!((outcome.added, outcome.uncertain), (0, 1));
        assert!(!server.lines().iter().any(|line| line.contains(" APPEND ")));

        let mut archive = torro_mail_backup::restore::RestoreArchive::open(std::io::Cursor::new(SAMPLE_ZIP))
            .expect("verified sample");
        let mut raw = Vec::new();
        archive.copy_message(0, &mut raw).expect("EML");
        server.add("INBOX", &String::from_utf8(raw).expect("sample text"), &[]);
        let outcome =
            plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("resolved");
        assert_eq!((outcome.added, outcome.skipped, outcome.uncertain), (0, 1, 0));
        assert!(RestoreJournal::open(&journal_path, &account.id, &digest).expect("journal").pending.is_empty());
        assert!(!server.lines().iter().any(|line| line.contains(" APPEND ")));
    }

    #[tokio::test]
    async fn two_matching_copies_do_not_resolve_one_uncertain_append() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let path = root.path().join("sample.zip");
        fs::write(&path, SAMPLE_ZIP).expect("sample");
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "sample.zip".into(), &path, &account.id).await.expect("plan");
        let mut archive = torro_mail_backup::restore::RestoreArchive::open(std::io::Cursor::new(SAMPLE_ZIP))
            .expect("verified sample");
        let digest = manifest_digest(archive.manifest()).expect("digest");
        let message_path = archive.manifest().messages[0].path.clone();
        let journal_path = RestoreJournal::path(&dirs(root.path()), &account.id, &digest).expect("private path");
        let mut journal = RestoreJournal::open(&journal_path, &account.id, &digest).expect("new journal");
        journal.pending.insert(message_path.clone(), 0);
        journal.save(&journal_path).expect("pending before append");
        let mut raw = Vec::new();
        archive.copy_message(0, &mut raw).expect("EML");
        let raw = String::from_utf8(raw).expect("sample text");
        server.add("INBOX", &raw, &[]);
        server.add("INBOX", &raw, &[]);
        let outcome =
            plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("safe stop");
        assert_eq!((outcome.added, outcome.uncertain), (0, 1));
        assert!(
            RestoreJournal::open(&journal_path, &account.id, &digest)
                .expect("journal")
                .pending
                .contains_key(&message_path)
        );
        assert!(!server.lines().iter().any(|line| line.contains(" APPEND ")));
    }

    #[tokio::test]
    async fn restore_apply_adds_raw_mail_once_and_rechecks_the_target() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let path = root.path().join("sample.zip");
        fs::write(&path, SAMPLE_ZIP).expect("sample");
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "sample.zip".into(), &path, &account.id).await.expect("plan");
        let result = plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("apply");
        assert_eq!((result.added, result.skipped, result.uncertain), (1, 0, 0));
        let result = plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("repeat");
        assert_eq!((result.added, result.skipped, result.uncertain), (0, 1, 0));
        assert_eq!(server.lines().iter().filter(|line| line.contains(" APPEND ")).count(), 1);
        assert!(server.lines().iter().any(|line| line.contains(" EXAMINE ")));
    }

    #[tokio::test]
    async fn restore_apply_rejects_a_removed_message_after_the_reviewed_plan() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let path = root.path().join("sample.zip");
        fs::write(&path, SAMPLE_ZIP).expect("sample");
        let mut archive = torro_mail_backup::restore::RestoreArchive::open(std::io::Cursor::new(SAMPLE_ZIP))
            .expect("verified sample");
        let mut raw = Vec::new();
        archive.copy_message(0, &mut raw).expect("original");
        let uid = server.add("INBOX", &String::from_utf8(raw).expect("sample text"), &[]);
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "sample.zip".into(), &path, &account.id).await.expect("reviewed plan");
        assert_eq!(plan.messages_to_add, 0);
        server.expunge("INBOX", uid);
        assert!(plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.is_err());
        assert!(!server.lines().iter().any(|line| line.contains(" APPEND ")));
    }

    #[tokio::test]
    async fn changed_archive_is_rejected_before_any_restore_write() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let path = root.path().join("sample.zip");
        fs::write(&path, SAMPLE_ZIP).expect("sample");
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "sample.zip".into(), &path, &account.id).await.expect("plan");
        fs::write(&path, b"changed after planning").expect("replace archive");
        assert!(plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.is_err());
        assert!(!server.lines().iter().any(|line| line.contains(" APPEND ") || line.contains(" CREATE ")));
        assert!(!root.path().join("data/mail/restore-journals").exists());
    }

    #[tokio::test]
    async fn connection_lost_after_append_resolves_from_target_without_a_second_upload() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let path = root.path().join("sample.zip");
        fs::write(&path, SAMPLE_ZIP).expect("sample");
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "sample.zip".into(), &path, &account.id).await.expect("plan");
        server.drop_after_next_append();
        let first =
            plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("safe stop");
        assert_eq!((first.added, first.uncertain), (0, 1));
        assert_eq!(server.messages("INBOX").len(), 1, "server committed the unacknowledged mail");
        let second =
            plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("reconciled");
        assert_eq!((second.added, second.skipped, second.uncertain), (0, 1, 0));
        assert_eq!(server.lines().iter().filter(|line| line.contains(" APPEND ")).count(), 1);
    }

    #[tokio::test]
    async fn restore_creates_a_missing_folder_before_appending_its_mail() {
        use torro_mail_backup::format::{
            Account, Counts, FORMAT, Folder, Inventory, Manifest, Source, Status, VERSION,
        };
        use torro_mail_backup::writer::{ArchiveWriter, MessageMetadata};

        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let started_at = Utc::now();
        let folder_path = "Projects/2026";
        let manifest = Manifest {
            format: FORMAT.into(),
            version: VERSION,
            status: Status::Incomplete,
            source: Source::Local,
            since: None,
            started_at,
            finished_at: None,
            account: Account { address: "anna@example.org".into(), server: None },
            folders: vec![Folder {
                id: "f000001".into(),
                name: folder_path.into(),
                path: folder_path.into(),
                delimiter: Some("/".into()),
                selectable: true,
                role: None,
                uid_validity: None,
                inventory: None,
            }],
            messages: Vec::new(),
            counts: Counts { folders: 1, messages: 0, bytes: 0 },
            warnings: Vec::new(),
        };
        let mut writer = ArchiveWriter::new(std::io::Cursor::new(Vec::new()), manifest).expect("writer");
        writer
            .add_message(
                "f000001",
                b"From: a@example.org\r\n\r\nNested\r\n".as_slice(),
                MessageMetadata {
                    internal_date: Some(started_at.fixed_offset()),
                    flags: vec!["\\Seen".into(), "$Work".into()],
                    ..MessageMetadata::default()
                },
            )
            .expect("message");
        writer.manifest_mut().folders[0].inventory =
            Some(Inventory { checked_at: started_at, eligible_messages: 1, uid_set_sha256: None });
        let (zip, _) = writer.finish(Status::Complete, Utc::now()).expect("complete ZIP");
        let path = root.path().join("nested.zip");
        fs::write(&path, zip.into_inner()).expect("ZIP");
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "nested.zip".into(), &path, &account.id).await.expect("plan");
        assert_eq!(plan.folders_to_create, 1);
        assert!(plan.folders[0].create);
        assert!(!server.lines().iter().any(|line| line.contains(" CREATE ")));
        let result = plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("apply");
        assert_eq!((result.added, result.uncertain), (1, 0));
        assert_eq!(server.messages(folder_path).len(), 1);
        assert_eq!(server.flags(folder_path, 1), ["$Work", "\\Seen"]);
        let checked = plans.plan(&mail, "nested.zip".into(), &path, &account.id).await.expect("post-restore plan");
        assert_eq!((checked.messages_to_add, checked.already_present), (0, 1));
        assert!(
            checked.warnings.is_empty(),
            "the fake server retained INTERNALDATE and keywords: {:?}",
            checked.warnings
        );
        let lines = server.lines();
        assert!(lines.iter().any(|line| line.contains(" CREATE \"Projects/2026\"")));
        assert_eq!(lines.iter().filter(|line| line.contains(" APPEND ")).count(), 1);
    }

    #[tokio::test]
    async fn incomplete_archive_needs_separate_apply_confirmation() {
        use torro_mail_backup::format::{Account, Counts, FORMAT, Folder, Manifest, Source, Status, VERSION};
        use torro_mail_backup::writer::{ArchiveWriter, MessageMetadata};

        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let started_at = Utc::now();
        let manifest = Manifest {
            format: FORMAT.into(),
            version: VERSION,
            status: Status::Incomplete,
            source: Source::Local,
            since: None,
            started_at,
            finished_at: None,
            account: Account { address: "anna@example.org".into(), server: None },
            folders: vec![Folder {
                id: "f000001".into(),
                name: "INBOX".into(),
                path: "INBOX".into(),
                delimiter: Some("/".into()),
                selectable: true,
                role: Some("inbox".into()),
                uid_validity: None,
                inventory: None,
            }],
            messages: Vec::new(),
            counts: Counts { folders: 1, messages: 0, bytes: 0 },
            warnings: Vec::new(),
        };
        let mut writer = ArchiveWriter::new(std::io::Cursor::new(Vec::new()), manifest).expect("writer");
        let raw = b"From: a@example.org\r\n\r\nPartial\r\n";
        writer.add_message("f000001", raw.as_slice(), MessageMetadata::default()).expect("message");
        let (zip, _) = writer.finish(Status::Incomplete, started_at).expect("valid incomplete ZIP");
        let path = root.path().join("incomplete.zip");
        fs::write(&path, zip.into_inner()).expect("ZIP");
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "incomplete.zip".into(), &path, &account.id).await.expect("plan");
        assert!(!plan.complete_archive);
        assert!(plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.is_err());
        assert!(!server.lines().iter().any(|line| line.contains(" APPEND ")));
        let result =
            plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, true).await.expect("confirmed");
        assert_eq!((result.added, result.uncertain), (1, 0));
    }

    #[tokio::test]
    async fn identical_raw_occurrences_are_counted_instead_of_collapsed() {
        use torro_mail_backup::format::{
            Account, Counts, FORMAT, Folder, Inventory, Manifest, Source, Status, VERSION,
        };
        use torro_mail_backup::writer::{ArchiveWriter, MessageMetadata};

        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let raw = "From: a@example.org\r\n\r\nSame body\r\n";
        server.add("INBOX", raw, &[]);
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root.path(), &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root.path(), &secrets);
        mail.reload().await;
        let started_at = Utc::now();
        let manifest = Manifest {
            format: FORMAT.into(),
            version: VERSION,
            status: Status::Incomplete,
            source: Source::Local,
            since: None,
            started_at,
            finished_at: None,
            account: Account { address: "anna@example.org".into(), server: None },
            folders: vec![Folder {
                id: "f000001".into(),
                name: "INBOX".into(),
                path: "INBOX".into(),
                delimiter: Some("/".into()),
                selectable: true,
                role: Some("inbox".into()),
                uid_validity: None,
                inventory: None,
            }],
            messages: Vec::new(),
            counts: Counts { folders: 1, messages: 0, bytes: 0 },
            warnings: Vec::new(),
        };
        let mut writer = ArchiveWriter::new(std::io::Cursor::new(Vec::new()), manifest).expect("writer");
        for _ in 0..2 {
            writer.add_message("f000001", raw.as_bytes(), MessageMetadata::default()).expect("occurrence");
        }
        writer.manifest_mut().folders[0].inventory =
            Some(Inventory { checked_at: started_at, eligible_messages: 2, uid_set_sha256: None });
        let (zip, _) = writer.finish(Status::Complete, Utc::now()).expect("complete ZIP");
        let path = root.path().join("duplicates.zip");
        fs::write(&path, zip.into_inner()).expect("ZIP");
        let plans = RestorePlans::default();
        let plan = plans.plan(&mail, "duplicates.zip".into(), &path, &account.id).await.expect("plan");
        assert_eq!((plan.messages_to_add, plan.already_present), (1, 1));
        let result = plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("apply");
        assert_eq!((result.added, result.skipped, result.uncertain), (1, 1, 0));
        assert_eq!(server.messages("INBOX").len(), 2);
        let repeat = plans.apply(&mail, &dirs(root.path()), &path, &account.id, &plan.id, false).await.expect("repeat");
        assert_eq!((repeat.added, repeat.skipped, repeat.uncertain), (0, 2, 0));
    }
}
