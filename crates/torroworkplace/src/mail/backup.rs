//! Mail backups (#267): the server and the local source, the jobs that run them, and the one way a
//! finished archive is published (verified, in the chosen folder, never over an existing file).
//! Restore planning and applying follow below; both halves share the helpers here: an IMAP session
//! of their own ([`OwnSession`]), the chunk reader ([`BodyChunks`]), the reasons a run cannot vouch
//! for its result ([`SourceProblem`]), warnings kept English in the archive ([`SourceWarning`]) and
//! random ids ([`random_id`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use torro_core::i18n::{tr, trf};
use torro_ipc::mail::{BackupJob, BackupPhase, BackupScope, RestoreFolderPlan, RestorePlan, RestoreResult};
use torro_mail::Role;
use torro_mail_backup::format::{
    Account, Counts, FORMAT, Folder, Inventory, MAX_TEXT_BYTES, MAX_WARNINGS, Manifest, Source, Status, VERSION,
};
use torro_mail_backup::restore::RestoreArchive;
use torro_mail_backup::writer::{ArchiveWriter, MessageMetadata};
use torro_mail_backup::{ArchiveError, VerifiedArchive, hex_digest};
use torro_mail_imap::folders;
use torro_mail_imap::imap::{
    AppendError, AppendUid, BackupMessage, BodyChunks, BodyError, ImapError, ListedFolder, Search, Selected, Session,
    backup_flag_transferable, encode_mailbox,
};

use super::service::{MailError, MailService, SharedStore};
use super::store::{FolderInfo, LocalOriginal};
use super::{MailDirs, MailStoreError};

#[derive(Debug)]
pub(crate) enum BackupFileError {
    Io(io::Error),
    Archive(ArchiveError),
    InvalidName,
    Incomplete,
    /// Something named like the archive is in the backup folder already (a file, a link, a
    /// folder): a backup never replaces it.
    Exists(String),
}

impl fmt::Display for BackupFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot publish mail backup: {error}"),
            Self::Archive(error) => write!(f, "mail backup failed verification: {error}"),
            Self::InvalidName => f.write_str("invalid mail backup filename"),
            Self::Incomplete => f.write_str("incomplete mail backup cannot be published"),
            Self::Exists(_) => f.write_str("the mail backup file exists already"),
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

/// Writes a ZIP byte by byte (fixtures) into a [`PendingBackup`] and publishes it after an offline
/// check; failure or invalid output removes the temporary file.
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

/// A private in-progress archive file: a temporary file (0600) in the destination folder, removed
/// when this is dropped before it is persisted. In the service only [`OpenArchive`] writes and
/// publishes one.
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

    /// The temporary file for `filename` in `selected` (absolute, existing, no symlink) or in the
    /// managed folder `<data>/backups` ([`backup_folder`]). Blocking: call it on the blocking pool.
    pub(crate) fn new_in(dirs: &MailDirs, filename: &str, selected: Option<&Path>) -> Result<Self, BackupFileError> {
        if !valid_filename(filename) {
            return Err(BackupFileError::InvalidName);
        }
        let backup_dir = backup_folder(dirs, selected)?;
        let destination = backup_dir.join(filename);
        let temporary = tempfile::Builder::new().prefix(".backup-").suffix(".partial").tempfile_in(&backup_dir)?;
        Ok(Self { backup_dir, destination, temporary })
    }

    #[cfg(test)]
    pub(crate) fn file_mut(&mut self) -> &mut File {
        self.temporary.as_file_mut()
    }

    /// Checks a ZIP written byte by byte (fixtures) offline and publishes it when it is complete.
    #[cfg(test)]
    pub(crate) fn publish(mut self) -> Result<(PathBuf, VerifiedArchive), BackupFileError> {
        self.temporary.as_file_mut().seek(SeekFrom::Start(0))?;
        let report = torro_mail_backup::verify_archive(self.temporary.as_file_mut())?;
        if !report.complete {
            return Err(BackupFileError::Incomplete);
        }
        Ok((self.persist()?.0, report))
    }

    /// Makes the temporary file durable, gives it its final name (which must not exist yet) and
    /// makes that name durable too, so a crash right after a reported success cannot lose the
    /// archive. Blocking.
    ///
    /// The second value says whether the name is durable. Once the file has its name the archive
    /// is verified and published, so a failed `fsync` of the folder after that is no failed backup;
    /// it only means a crash right now could still lose the new name, which the job then says.
    fn persist(self) -> Result<(PathBuf, bool), BackupFileError> {
        self.temporary.as_file().sync_all()?;
        // The last guard: a file that appeared under the name after the job's check stays as it is.
        self.temporary.persist_noclobber(&self.destination).map_err(|error| match error.error.kind() {
            io::ErrorKind::AlreadyExists => BackupFileError::Exists(
                self.destination.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            ),
            _ => BackupFileError::Io(error.error),
        })?;
        #[cfg(unix)]
        let durable = File::open(&self.backup_dir).and_then(|folder| folder.sync_all()).is_ok();
        #[cfg(not(unix))]
        let durable = true;
        Ok((self.destination, durable))
    }
}

/// The folder a backup writes into: `selected` (absolute, an existing folder, no symlink),
/// canonicalized, or the managed folder `<data>/backups`, made private when missing. Blocking.
fn backup_folder(dirs: &MailDirs, selected: Option<&Path>) -> Result<PathBuf, BackupFileError> {
    if let Some(selected) = selected {
        if !selected.is_absolute() {
            return Err(BackupFileError::InvalidName);
        }
        reject_symlink(selected)?;
        let path = selected.canonicalize()?;
        if !path.is_dir() {
            return Err(BackupFileError::InvalidName);
        }
        return Ok(path);
    }
    reject_symlink(&dirs.data)?;
    crate::clients::create_private_dir(&dirs.data)?;
    let path = dirs.data.join("backups");
    reject_symlink(&path)?;
    crate::clients::create_private_dir(&path)?;
    Ok(path)
}

/// What a backup checks before its job starts (#267), blocking: the destination folder
/// ([`backup_folder`]), that nothing named `filename` is there yet (no file, no link, no folder),
/// and that its disk keeps [`DISK_RESERVE`] free ([`DiskSpace`]). Free space that cannot be
/// learned does not stop it. [`PendingBackup::persist`] stays the last guard against a file that
/// appears under the name meanwhile.
fn preflight(
    dirs: &MailDirs,
    filename: &str,
    selected: Option<&Path>,
    space: &DiskSpace,
) -> Result<(), BackupRunError> {
    let folder = backup_folder(dirs, selected)?;
    match fs::symlink_metadata(folder.join(filename)) {
        Ok(_) => return Err(BackupRunError::File(BackupFileError::Exists(filename.to_string()))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if let Some(free) = space.free(&folder)
        && free < DISK_RESERVE
    {
        return Err(BackupRunError::Space(SpaceShortage::AtStart { free }));
    }
    Ok(())
}

/// Free disk space a backup always leaves (#267): it does not start with less, a local backup does
/// not start when its mail would leave less, and a running one stops when the disk gets there.
/// Agents with write access may start backup after backup; this keeps them from filling the disk.
/// A fixed amount rather than a share of the disk: 2 GiB keeps the system and the other apps
/// working on any disk, while a share would refuse backups on a large, mostly free disk for no
/// reason (10 % of 4 TB is 400 GB) and leave too little on a small one.
const DISK_RESERVE: u64 = 2 * 1024 * 1024 * 1024;

/// How many bytes a running backup writes (spool files and archive) between two looks at the free
/// space: often enough that the reserve holds, rarely enough that asking costs nothing.
const SPACE_CHECK_EVERY: u64 = 64 * 1024 * 1024;

/// The free bytes on the disk of a folder; `None` when unknown. Blocking.
type FreeBytes = dyn Fn(&Path) -> Option<u64> + Send + Sync;

/// How a backup learns how much space is left on the disk of a folder, and how often it asks while
/// writing ([`SpaceWatch`]). The core asks the system ([`DiskSpace::system`]); tests give their own
/// answers instead of filling a disk.
#[derive(Clone)]
pub(crate) struct DiskSpace {
    free: Arc<FreeBytes>,
    every: u64,
}

impl DiskSpace {
    /// The free space as the system reports it for the disk the folder is on ([`system_free_space`]).
    pub(crate) fn system() -> Self {
        Self { free: Arc::new(system_free_space), every: SPACE_CHECK_EVERY }
    }

    /// Never knows the free space: nothing is refused for it.
    pub(crate) fn unknown() -> Self {
        Self { free: Arc::new(|_: &Path| None), every: SPACE_CHECK_EVERY }
    }

    /// `free` answers for every folder; while writing it is asked every `every` bytes.
    #[cfg(test)]
    pub(crate) fn fake(every: u64, free: impl Fn(&Path) -> Option<u64> + Send + Sync + 'static) -> Self {
        Self { free: Arc::new(free), every }
    }

    /// The free bytes on the disk of `dir`; `None` when unknown. Blocking.
    fn free(&self, dir: &Path) -> Option<u64> {
        (self.free)(dir)
    }
}

impl Default for DiskSpace {
    /// The system's answer; in tests none, so no test depends on the free space of the machine it
    /// runs on (tests that need an answer give one with [`DiskSpace::fake`]).
    fn default() -> Self {
        if cfg!(test) { Self::unknown() } else { Self::system() }
    }
}

/// The free space of the disk `dir` is on, from the disks the system lists: the one whose mount
/// point is the longest prefix of the canonical `dir`, if it really holds `dir` ([`free_on`]).
/// `None` when the folder or no disk matches, or when the match is another file system.
///
/// The disk list is not complete: on Linux sysinfo leaves out network and virtual file systems
/// (nfs, cifs, autofs, tmpfs) and everything under `/run` (an SMB share opened through gvfs), on
/// macOS volumes that are not local. A folder on a NAS then matches `/` by its path, and the
/// system disk's free space would refuse or allow its backup. On Unix the device of the folder
/// and of the matched mount point must therefore be the same; when they differ, the free space
/// is unknown and nothing is refused for it. Windows lists no network drives either; there a
/// folder on one matches no mount point and is unknown already.
/// Blocking: listing disks asks every mounted file system.
fn system_free_space(dir: &Path) -> Option<u64> {
    let dir = without_verbatim(dir.canonicalize().ok()?);
    let refresh = sysinfo::DiskRefreshKind::nothing().with_storage();
    let disks = sysinfo::Disks::new_with_refreshed_list_specifics(refresh);
    free_on(disks.list().iter().map(|disk| (disk.mount_point(), disk.available_space())), &dir, device_of)
}

/// The free bytes of the disk among `disks` (mount point, free bytes) whose mount point is the
/// longest prefix of `dir`, by whole path components, when `device` (the file system a path is
/// on, `None` when unknown) gives the same answer for `dir` and that mount point; otherwise
/// `None`, since the list then lacks the disk `dir` is really on ([`system_free_space`]).
fn free_on<'d>(
    disks: impl IntoIterator<Item = (&'d Path, u64)>,
    dir: &Path,
    device: impl Fn(&Path) -> Option<u64>,
) -> Option<u64> {
    let (mount, free) = disks
        .into_iter()
        .filter(|(mount, _)| dir.starts_with(mount))
        .max_by_key(|(mount, _)| mount.components().count())?;
    (device(dir)? == device(mount)?).then_some(free)
}

/// The device id of the file system `path` is on; `None` when it cannot be read.
#[cfg(unix)]
fn device_of(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|metadata| metadata.dev())
}

/// Windows has no device id in the standard library: every path that exists counts as the same
/// device, so the mount point alone decides. Network drives are not in the disk list there and
/// match no mount point ([`system_free_space`]).
#[cfg(not(unix))]
fn device_of(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|_| 0)
}

/// On Windows `canonicalize` gives `\\?\C:\…`, while disks are mounted at `C:\`: the plain form,
/// so the two compare. Network paths (`\\?\UNC\…`) stay as they are.
#[cfg(windows)]
fn without_verbatim(path: PathBuf) -> PathBuf {
    let plain = {
        let text = path.to_string_lossy();
        text.strip_prefix(r"\\?\").filter(|rest| !rest.starts_with("UNC\\")).map(PathBuf::from)
    };
    plain.unwrap_or(path)
}

#[cfg(not(windows))]
fn without_verbatim(path: PathBuf) -> PathBuf {
    path
}

/// Too little free disk space for a backup ([`DISK_RESERVE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpaceShortage {
    /// Before the job starts: less than the reserve is free.
    AtStart { free: u64 },
    /// A local backup counted `mail` bytes of originals, and the reserve would not stay free.
    ForMail { free: u64, mail: u64 },
    /// While the archive was written, the free space fell under the reserve.
    WhileWriting { free: u64 },
}

impl SpaceShortage {
    /// For the person.
    fn user_message(self) -> String {
        let reserve = size_words(DISK_RESERVE);
        match self {
            Self::AtStart { free } => trf(
                "Not enough free disk space for a backup: {free} free, at least {reserve} must stay free.",
                &[("free", &size_words(free)), ("reserve", &reserve)],
            ),
            Self::ForMail { free, mail } => trf(
                "Not enough free disk space for this backup: {mail} of mail, {free} free, and at least {reserve} must stay free.",
                &[("mail", &size_words(mail)), ("free", &size_words(free)), ("reserve", &reserve)],
            ),
            Self::WhileWriting { free } => trf(
                "Mail backup stopped because the disk is almost full: {free} free, at least {reserve} must stay free. The partial archive was removed.",
                &[("free", &size_words(free)), ("reserve", &reserve)],
            ),
        }
    }
}

/// A size for people, in GiB from one GiB on, else in MiB, with one decimal.
fn size_words(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    let bytes = bytes as f64;
    if bytes >= 1024.0 * MIB { format!("{:.1} GiB", bytes / (1024.0 * MIB)) } else { format!("{:.1} MiB", bytes / MIB) }
}

/// The free space of the folder a running backup writes into ([`DiskSpace`]): writers note what
/// they wrote, and every [`DiskSpace`]'s `every` bytes the free space is looked at again. Shared by
/// the archive and its spool files; one writes at a time.
pub(crate) struct SpaceWatch {
    space: DiskSpace,
    dir: PathBuf,
    unchecked: AtomicU64,
}

impl SpaceWatch {
    /// The folder spool files go into: the archive's, on the same disk.
    fn dir(&self) -> &Path {
        &self.dir
    }

    /// Notes `bytes` more on the disk; once enough came together, stops the run
    /// ([`SpaceShortage::WhileWriting`]) when less than [`DISK_RESERVE`] is free. Blocking.
    fn wrote(&self, bytes: u64) -> Result<(), BackupRunError> {
        let unchecked = self.unchecked.fetch_add(bytes, Ordering::Relaxed).saturating_add(bytes);
        if unchecked < self.space.every {
            return Ok(());
        }
        self.unchecked.store(0, Ordering::Relaxed);
        match self.space.free(&self.dir) {
            Some(free) if free < DISK_RESERVE => Err(BackupRunError::Space(SpaceShortage::WhileWriting { free })),
            _ => Ok(()),
        }
    }

    /// Whether `mail` more bytes still leave [`DISK_RESERVE`] free ([`SpaceShortage::ForMail`]).
    /// Blocking.
    fn room_for(&self, mail: u64) -> Result<(), BackupRunError> {
        match self.space.free(&self.dir) {
            Some(free) if free < mail.saturating_add(DISK_RESERVE) => {
                Err(BackupRunError::Space(SpaceShortage::ForMail { free, mail }))
            }
            _ => Ok(()),
        }
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

/// Which finished archives [`OpenArchive::publish`] publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Publish {
    /// Only a complete one: the server source has no known gaps, a gap means the run failed.
    CompleteOnly,
    /// Also one whose manifest names its known gaps (`incomplete`, the local source, MB.4).
    WithGaps,
}

/// An archive a backup source is writing: the [`ArchiveWriter`] on the temporary file of a
/// [`PendingBackup`]. Copying originals into the ZIP and the final verification run on tokio's
/// blocking pool. What is published is exactly the file the writer finished and verified in
/// [`ArchiveWriter::finish`]; it is not read a second time. Dropping it removes the temporary file.
pub(crate) struct OpenArchive {
    /// `None` only after a blocking step panicked; every later call fails then.
    inner: Option<(PendingBackup, ArchiveWriter<File>)>,
    /// The free space of the archive's folder, also for its spool files.
    space: Arc<SpaceWatch>,
}

impl OpenArchive {
    /// Creates the temporary file `filename` will be published from, in `target_dir` or in the
    /// managed folder of `dirs`, and starts the archive with `manifest` (unfinished and without
    /// entries, see [`ArchiveWriter::new`]). What it and its spool files write is watched with
    /// `space` ([`SpaceWatch`]).
    pub(crate) async fn create(
        dirs: MailDirs,
        filename: String,
        target_dir: Option<PathBuf>,
        manifest: Manifest,
        space: DiskSpace,
    ) -> Result<Self, BackupRunError> {
        blocking(move || {
            let pending = PendingBackup::new_in(&dirs, &filename, target_dir.as_deref())?;
            // A second handle on the same file: the writer owns it, the pending file keeps the name.
            let writer = ArchiveWriter::new(pending.temporary.as_file().try_clone()?, manifest)?;
            let space = SpaceWatch { space, dir: pending.backup_dir.clone(), unchecked: AtomicU64::new(0) };
            Ok(OpenArchive { inner: Some((pending, writer)), space: Arc::new(space) })
        })
        .await
    }

    /// The writer, for the manifest, aliases and lookups; none of these touch the disk.
    pub(crate) fn writer(&mut self) -> Result<&mut ArchiveWriter<File>, BackupRunError> {
        self.inner.as_mut().map(|(_, writer)| writer).ok_or(BackupRunError::Archive(INTERRUPTED))
    }

    /// The free space of the archive's folder; spool files go there too ([`Spool::new`]).
    pub(crate) fn space(&self) -> &Arc<SpaceWatch> {
        &self.space
    }

    /// [`ArchiveWriter::add_message`] from the start of `spool`, on the blocking pool. A rejected
    /// entry leaves the archive usable; the caller decides whether the run goes on.
    pub(crate) async fn add_message(
        &mut self,
        folder_id: &str,
        mut spool: File,
        metadata: MessageMetadata,
    ) -> Result<(), BackupRunError> {
        let (pending, mut writer) = self.inner.take().ok_or(BackupRunError::Archive(INTERRUPTED))?;
        let folder_id = folder_id.to_string();
        let space = Arc::clone(&self.space);
        let (inner, added) = tokio::task::spawn_blocking(move || {
            let added = (|| {
                spool.seek(SeekFrom::Start(0))?;
                let length = spool.metadata()?.len();
                writer.add_message(&folder_id, spool, metadata)?;
                // The ZIP grows by about the spool's length.
                space.wrote(length)
            })();
            ((pending, writer), added)
        })
        .await
        .map_err(|_| BackupRunError::Io(io::Error::other("backup worker stopped")))?;
        self.inner = Some(inner);
        added
    }

    /// Finishes the archive with `status` (writer and offline verification on the blocking pool),
    /// then — unless the job was cancelled meanwhile, see `state` — publishes the verified file
    /// under its final name. Afterwards a cancellation can no longer relabel it. The report's
    /// warnings are `warnings` in the person's language, and one more when the new name could not
    /// be made durable ([`PendingBackup::persist`]).
    async fn publish(
        mut self,
        status: Status,
        rule: Publish,
        state: Arc<AtomicU8>,
        warnings: &[SourceWarning],
    ) -> Result<VerifiedArchive, BackupRunError> {
        let (pending, writer) = self.inner.take().ok_or(BackupRunError::Archive(INTERRUPTED))?;
        let (mut report, durable) = blocking(move || {
            let (_, report) = writer.finish(status, Utc::now())?;
            if rule == Publish::CompleteOnly && !report.complete {
                return Err(BackupRunError::File(BackupFileError::Incomplete));
            }
            state
                .compare_exchange(RUNNING, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| BackupRunError::Cancelled)?;
            let (_, durable) = pending.persist()?;
            Ok((report, durable))
        })
        .await?;
        report.warnings = warnings.iter().map(SourceWarning::user_text).collect();
        if !durable {
            report.warnings.push(
                tr("The archive was saved and verified, but the disk did not confirm its new name; check that it is still there after a system crash.")
                    .into(),
            );
        }
        Ok(report)
    }
}

const INTERRUPTED: ArchiveError = ArchiveError::Invalid("archive writer was interrupted");

/// Runs `work` on tokio's blocking pool: file I/O must not stall the async workers that speak IMAP
/// and answer requests. A panic there is an I/O error here.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, BackupRunError> + Send + 'static,
) -> Result<T, BackupRunError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| BackupRunError::Io(io::Error::other("backup worker stopped")))?
}

/// One original on its way into the archive: an anonymous temporary file (0600, gone when dropped)
/// next to the archive, with the SHA-256 and length of what was written. Writes go to the blocking
/// pool.
struct Spool {
    file: File,
    digest: Sha256,
    bytes: u64,
    space: Arc<SpaceWatch>,
}

impl Spool {
    /// A new spool file in the archive's folder, its writes noted in `space`.
    async fn new(space: &Arc<SpaceWatch>) -> Result<Spool, BackupRunError> {
        let space = Arc::clone(space);
        blocking(move || {
            let file = tempfile::tempfile_in(space.dir())?;
            Ok(Spool { file, digest: Sha256::new(), bytes: 0, space })
        })
        .await
    }

    async fn write(mut self, piece: Vec<u8>) -> Result<Spool, BackupRunError> {
        blocking(move || {
            self.file.write_all(&piece)?;
            self.digest.update(&piece);
            self.bytes += piece.len() as u64;
            self.space.wrote(piece.len() as u64)?;
            Ok(self)
        })
        .await
    }

    /// The file (not rewound), and the lowercase hex SHA-256 and the length of its content.
    fn finish(self) -> (File, String, u64) {
        (self.file, hex_digest(&self.digest.finalize()), self.bytes)
    }
}

/// Why a backup source cannot vouch for what it read (MB.3, MB.4): the run ends `incomplete` and
/// publishes nothing. Each reason has its fixed text for the person ([`SourceProblem::user_message`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceProblem {
    /// A message of the inventory is no longer on the server.
    Vanished,
    /// The server returned a body that does not match its `RFC822.SIZE`: shorter, or with bytes
    /// after it.
    BodyMismatch,
    /// A new UIDVALIDITY, or answers for other UIDs: the inventory names other messages now.
    IdentifiersChanged,
    /// The folder list changed (a folder appeared, vanished or changed selectability), or new mail
    /// kept arriving through every final pass.
    InventoryChanged,
    /// A local original disappeared, was cut short or no longer matches its hash.
    LocalDamaged,
    /// Local originals changed place or metadata while they were read.
    LocalChanged,
}

impl SourceProblem {
    /// For the job and the person.
    pub(crate) fn user_message(self) -> &'static str {
        match self {
            Self::Vanished => tr("Mail backup is incomplete: a source message disappeared while reading."),
            Self::BodyMismatch => tr("Mail backup is incomplete: the server returned an incomplete message body."),
            Self::IdentifiersChanged => tr("Mail backup is incomplete: IMAP message identifiers changed."),
            Self::InventoryChanged => tr("Mail backup is incomplete: the folder inventory changed while reading."),
            Self::LocalDamaged => tr("Mail backup is incomplete: a local original is missing or damaged."),
            Self::LocalChanged => tr("Mail backup is incomplete: the local mail inventory changed while reading."),
        }
    }

    /// For logs: English and fixed.
    fn as_str(self) -> &'static str {
        match self {
            Self::Vanished => "a source message disappeared",
            Self::BodyMismatch => "a message body does not match its RFC822.SIZE",
            Self::IdentifiersChanged => "IMAP identifiers changed",
            Self::InventoryChanged => "the folder inventory changed",
            Self::LocalDamaged => "a local original is missing or damaged",
            Self::LocalChanged => "the local inventory changed",
        }
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
    /// A backup source found a change or a gap it cannot vouch for.
    Source(SourceProblem),
    /// A restore cannot go on as planned.
    Restore(RestoreProblem),
    /// Too little free disk space for a backup.
    Space(SpaceShortage),
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
            Self::Source(problem) => write!(f, "backup source: {}", problem.as_str()),
            Self::Restore(problem) => write!(f, "restore: {}", problem.as_str()),
            Self::Space(_) => f.write_str("not enough free disk space for a backup"),
            Self::Busy => f.write_str("another backup for this account is already running"),
            Self::Random => f.write_str("cannot create a backup job id"),
            Self::Cancelled => f.write_str("backup cancelled"),
        }
    }
}

impl BackupRunError {
    /// What a failed backup job, or a refused start, tells the person.
    pub(crate) fn user_message(&self) -> String {
        match self {
            Self::Mail(error) => error.message(),
            Self::Imap(ImapError::Auth(_)) => tr("Password was refused").into(),
            Self::Imap(ImapError::Offline(_)) => tr("The server cannot be reached.").into(),
            Self::Imap(_) => tr("The mail server answered unexpectedly.").into(),
            Self::Source(problem) => problem.user_message().into(),
            Self::Restore(problem) => problem.user_message().into(),
            Self::Cancelled => tr("Mail backup was cancelled.").into(),
            Self::Space(shortage) => shortage.user_message(),
            Self::File(BackupFileError::Exists(name)) => {
                trf("A file named {name} already exists in the backup folder. Choose another name.", &[("name", name)])
            }
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

impl From<BodyError> for BackupRunError {
    fn from(error: BodyError) -> Self {
        match error {
            BodyError::Imap(error) => Self::Imap(error),
            BodyError::Vanished => Self::Source(SourceProblem::Vanished),
            BodyError::Truncated | BodyError::Oversized => Self::Source(SourceProblem::BodyMismatch),
        }
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

/// A random 128-bit id in lowercase hex, for backup jobs and restore plans.
fn random_id() -> Result<String, BackupRunError> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|_| BackupRunError::Random)?;
    Ok(hex_digest(&random))
}

/// The address of the mail account `account` (`mail:<address>`).
fn mail_address(account: &str) -> Result<&str, BackupRunError> {
    account.strip_prefix(torro_mail::ACCOUNT_PREFIX).ok_or(BackupRunError::Mail(MailError::NotFound))
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
    hex_digest(&hash.finalize())
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
    /// How the jobs learn the free disk space ([`DISK_RESERVE`]).
    space: DiskSpace,
}

pub(crate) struct BackupSelection {
    pub source: BackupScope,
    pub since: Option<NaiveDate>,
    pub filename: Option<String>,
    pub target_dir: Option<PathBuf>,
}

impl BackupJobs {
    /// Jobs that learn the free disk space from `space` instead of the system.
    #[cfg(test)]
    pub(crate) fn with_space(space: DiskSpace) -> Self {
        Self { space, ..Self::default() }
    }

    /// Starts a backup of `account` in a task of its own and answers its job at once. Refused
    /// before any job exists ([`preflight`]): `File(Exists)` when something named like the archive
    /// is in the destination folder already, `Space(AtStart)` when its disk has less than
    /// [`DISK_RESERVE`] free (unknown free space refuses nothing). `Busy` while another job of the
    /// account has not ended ([`BackupPhase::is_final`]), cancelling ones too. A second task
    /// watches the first: whatever way it ends (also a panic), the job reaches a final phase.
    pub(crate) async fn start(
        &self,
        mail: Arc<MailService>,
        dirs: MailDirs,
        account: String,
        selection: BackupSelection,
        owner: Option<String>,
    ) -> Result<BackupJob, BackupRunError> {
        let BackupSelection { source, since, filename, target_dir } = selection;
        let id = random_id()?;
        let filename = filename.unwrap_or_else(|| format!("mail-{}-{id}.zip", Utc::now().format("%Y%m%dT%H%M%S")));
        if !valid_filename(&filename) {
            return Err(BackupRunError::File(BackupFileError::InvalidName));
        }
        {
            let (dirs, filename, target_dir, space) =
                (dirs.clone(), filename.clone(), target_dir.clone(), self.space.clone());
            blocking(move || preflight(&dirs, &filename, target_dir.as_deref(), &space)).await?;
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
            if entries.values().any(|entry| entry.job.account == account && !entry.job.phase.is_final()) {
                return Err(BackupRunError::Busy);
            }
            if entries.len() >= 128 {
                entries.retain(|_, entry| !entry.job.phase.is_final());
            }
            entries.insert(id.clone(), JobEntry { job: job.clone(), state: Arc::clone(&state), owner });
        }
        let (jobs, work_id, work_state, work_file) = (self.clone(), id.clone(), Arc::clone(&state), filename.clone());
        let work = tokio::spawn(async move {
            let progress = BackupProgress { jobs: &jobs, id: &work_id, state: work_state };
            let target = target_dir.as_deref();
            match source {
                BackupScope::Server => {
                    run_server_backup(mail, dirs, &account, since, &work_file, target, progress).await
                }
                BackupScope::Local => run_local_backup(mail, dirs, &account, since, &work_file, target, progress).await,
            }
        });
        let jobs = self.clone();
        tokio::spawn(async move {
            let outcome = work.await;
            jobs.finish(&id, &state, filename, outcome);
        });
        Ok(job)
    }

    /// Notes how the task of job `id` ended. A cancelled job ends `cancelled` only now, when its
    /// task has stopped; a task that panicked ends `incomplete` with a warning instead of staying
    /// active forever (its temporary files went with its stack).
    fn finish(
        &self,
        id: &str,
        state: &AtomicU8,
        archive: String,
        outcome: Result<Result<VerifiedArchive, BackupRunError>, tokio::task::JoinError>,
    ) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(entry) = entries.get_mut(id) else { return };
        let cancelled = state.load(Ordering::Acquire) == CANCELLED;
        match outcome {
            Ok(Ok(verified)) => {
                // A published archive with known gaps (local source) ends incomplete.
                entry.job.phase = if verified.complete { BackupPhase::Complete } else { BackupPhase::Incomplete };
                entry.job.archive = Some(archive);
                entry.job.folders = verified.folders;
                entry.job.messages = verified.messages;
                entry.job.bytes = verified.bytes;
                entry.job.warnings = verified.warnings;
            }
            Ok(Err(BackupRunError::Cancelled)) => entry.job.phase = BackupPhase::Cancelled,
            _ if cancelled => entry.job.phase = BackupPhase::Cancelled,
            Ok(Err(error)) => {
                entry.job.phase = BackupPhase::Incomplete;
                entry.job.warnings.push(error.user_message());
            }
            Err(_) => {
                entry.job.phase = BackupPhase::Incomplete;
                entry.job.warnings.push(tr("Mail backup could not be completed.").into());
            }
        }
    }

    pub(crate) fn status(&self, account: &str, id: &str, caller: Option<&str>) -> Option<BackupJob> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .filter(|entry| entry.job.account == account && (caller.is_none() || entry.owner.as_deref() == caller))
            .map(|entry| entry.job.clone())
    }

    /// Asks job `id` to stop: it is `cancelling` until its task has stopped and removed its files
    /// ([`BackupJobs::finish`]). A job already publishing (`COMMITTING`) or ended stays as it is.
    pub(crate) fn cancel(&self, account: &str, id: &str, caller: Option<&str>) -> Option<BackupJob> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries
            .get_mut(id)
            .filter(|entry| entry.job.account == account && (caller.is_none() || entry.owner.as_deref() == caller))?;
        if !entry.job.phase.is_final()
            && entry.state.compare_exchange(RUNNING, CANCELLED, Ordering::AcqRel, Ordering::Acquire).is_ok()
        {
            entry.job.phase = BackupPhase::Cancelling;
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

/// Where a running source reports its phase and counts, and learns of a cancellation.
struct BackupProgress<'a> {
    jobs: &'a BackupJobs,
    id: &'a str,
    state: Arc<AtomicU8>,
}

impl BackupProgress<'_> {
    fn phase(&self, phase: BackupPhase) {
        self.jobs.phase(self.id, phase);
    }

    fn counts(&self, counts: &Counts) {
        self.jobs.progress(self.id, counts.folders, counts.messages, counts.bytes);
    }

    /// `Err(Cancelled)` once the job was asked to stop.
    fn go_on(&self) -> Result<(), BackupRunError> {
        check_cancel(&self.state)
    }
}

/// A warning a backup source reports (MB.3, MB.4). The archive keeps it in English, so
/// `manifest.json` reads the same wherever and in whatever language it is opened; the job shows it
/// in the person's language. `template` is the English text with `{name}` placeholders, as [`trf`]
/// takes it and the German table keys it.
///
/// The pattern for every source: collect `SourceWarning`s, pass them through [`capped_warnings`],
/// write [`SourceWarning::archive_text`] into `Manifest::warnings`, and after publishing replace
/// `VerifiedArchive::warnings` (what the job shows) by [`SourceWarning::user_text`] of the same
/// list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceWarning {
    template: &'static str,
    args: Vec<(&'static str, String)>,
}

impl SourceWarning {
    pub(crate) fn new(template: &'static str, args: &[(&'static str, &dyn fmt::Display)]) -> Self {
        Self { template, args: args.iter().map(|(name, value)| (*name, value.to_string())).collect() }
    }

    /// For the archive: English, whatever language the suite speaks, and cut to the manifest's
    /// text limit with "…" (a long folder name must not make the archive invalid).
    pub(crate) fn archive_text(&self) -> String {
        let mut text = self
            .args
            .iter()
            .fold(self.template.to_string(), |text, (name, value)| text.replace(&format!("{{{name}}}"), value));
        if text.len() > MAX_TEXT_BYTES {
            let mut end = MAX_TEXT_BYTES - '…'.len_utf8();
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push('…');
        }
        text
    }

    /// For the job and the person: translated.
    pub(crate) fn user_text(&self) -> String {
        let args: Vec<(&str, &dyn fmt::Display)> =
            self.args.iter().map(|(name, value)| (*name, value as &dyn fmt::Display)).collect();
        trf(self.template, &args)
    }
}

/// `warnings` within the manifest's limit of [`MAX_WARNINGS`]: beyond it the last place says how
/// many more were left out.
pub(crate) fn capped_warnings(warnings: Vec<SourceWarning>) -> Vec<SourceWarning> {
    capped_to(warnings, MAX_WARNINGS)
}

/// How many UIDs one metadata `UID FETCH` asks for: one round trip for hundreds of messages instead
/// of one each, while an answer stays far below the parser's limits.
const METADATA_BATCH: usize = 500;
/// How many final passes may still find (and take) new mail before the run gives up as unstable.
const FINAL_PASSES: usize = 3;
/// How long an own session waits for its `LOGOUT` before it just closes the connection.
const LOGOUT_WAIT: Duration = Duration::from_secs(10);

/// An IMAP session a backup or a restore has to itself ([`MailService::own_session`]), so hours of
/// reading never hold up the sync, reading and sending, which share the command connection. Signed
/// in by the service like every connection of the account; the password stays in the core.
///
/// End it with [`OwnSession::close`] after the work, also after an error or a cancellation, so the
/// server sees a `LOGOUT`; dropping it only cuts the connection.
pub(crate) struct OwnSession {
    session: Session,
}

impl OwnSession {
    pub(crate) async fn open(mail: &MailService, account: &str) -> Result<Self, BackupRunError> {
        Ok(Self { session: mail.own_session(account).await? })
    }

    pub(crate) fn session(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Signs out (`LOGOUT`) and closes the connection, waiting at most [`LOGOUT_WAIT`]. Its outcome
    /// does not matter: the work is done or failed already, and the connection closes either way.
    pub(crate) async fn close(self) {
        let _ = tokio::time::timeout(LOGOUT_WAIT, self.session.logout()).await;
    }
}

/// The archive's name for a role (`Folder::role`), spelled out so the format never follows the
/// names of Rust enum variants.
fn role_name(role: Role) -> &'static str {
    match role {
        Role::Inbox => "inbox",
        Role::Drafts => "drafts",
        Role::Sent => "sent",
        Role::Archive => "archive",
        Role::Junk => "junk",
        Role::Trash => "trash",
    }
}

/// Attributes of a folder that shows all mail of the account (archive role `all`): `\All`
/// (RFC 6154) and the older `\AllMail`. Such a folder only collects, on every server.
const ALL_MAIL: [&str; 2] = ["\\All", "\\AllMail"];

/// Attributes that mark a collecting folder only on Gmail, with their archive role. RFC 8457 lets
/// any server put `\Flagged` or `\Important` on a real folder whose mail lives nowhere else; taking
/// that for a collection would leave its mail out of the archive.
const GMAIL_COLLECTIONS: [(&str, &str); 2] = [("\\Flagged", "flagged"), ("\\Important", "important")];

/// The capability that tells a Gmail server (and its virtual folders) apart.
const GMAIL: &str = "X-GM-EXT-1";

/// The role of a listed folder in the archive, and its rank: 0 for a real folder, 1 for "All
/// Mail", 2 for Gmail's other collections. Folders are read by rank, so "All Mail" comes before
/// starred and important mail whatever order the server lists them in.
fn archive_role(folder: &ListedFolder, gmail: bool) -> (Option<&'static str>, u8) {
    if ALL_MAIL.iter().any(|attribute| folder.has_attribute(attribute)) {
        return (Some("all"), 1);
    }
    if gmail && let Some((_, role)) = GMAIL_COLLECTIONS.iter().find(|(attribute, _)| folder.has_attribute(attribute)) {
        return (Some(role), 2);
    }
    let role = folders::role_of_attributes(&folder.attributes)
        .map(role_name)
        .or_else(|| folder.path.eq_ignore_ascii_case("INBOX").then_some("inbox"));
    (role, 0)
}

/// One selectable folder of a server backup.
struct ServerFolder {
    /// Its place in the manifest's folder table.
    index: usize,
    id: String,
    path: String,
    /// Collects mail of other folders: read last, with known originals as aliases.
    collection: bool,
    uid_validity: u32,
    /// Every UID an inventory of the folder found, inside the date limit or not.
    seen: HashSet<u32>,
    /// The UIDs in the archive, as message or alias: what its inventory counts.
    taken: HashSet<u32>,
}

/// The first view of the server: the manifest with its folder table (collections last), the
/// selectable folders to read, warnings about the others, and the selectability of every path for
/// the final comparison.
struct ServerPlan {
    manifest: Manifest,
    folders: Vec<ServerFolder>,
    warnings: Vec<SourceWarning>,
    selectability: HashMap<String, bool>,
}

fn selectability(listed: &[ListedFolder]) -> HashMap<String, bool> {
    listed.iter().map(|folder| (folder.path.clone(), folder.selectable())).collect()
}

fn server_plan(listed: Vec<ListedFolder>, gmail: bool, address: &str, since: Option<NaiveDate>) -> ServerPlan {
    let selectability = selectability(&listed);
    let mut paths = HashSet::new();
    let mut warnings = Vec::new();
    let mut entries = Vec::new();
    for folder in listed {
        if !paths.insert(folder.path.clone()) {
            continue;
        }
        let (role, rank) = archive_role(&folder, gmail);
        let selectable = folder.selectable();
        if !selectable {
            let reason = if folder.has_attribute("\\NonExistent") { "\\NonExistent" } else { "\\Noselect" };
            warnings.push(SourceWarning::new(
                "The IMAP server marks folder {path} as {reason}; its messages cannot be inventoried.",
                &[("path", &folder.path), ("reason", &reason)],
            ));
        }
        let entry = Folder {
            id: String::new(),
            name: folders::display_name(&folder.path, folder.delimiter),
            path: folder.path,
            delimiter: folder.delimiter.map(|delimiter| delimiter.to_string()),
            selectable,
            role: role.map(str::to_string),
            uid_validity: None,
            inventory: None,
        };
        entries.push((entry, rank));
    }
    // Collections last: then their messages that also lie in an ordinary folder are known already.
    entries.sort_by_key(|(_, rank)| *rank);
    let mut folders = Vec::new();
    let mut table = Vec::with_capacity(entries.len());
    for (index, (mut entry, rank)) in entries.into_iter().enumerate() {
        entry.id = format!("f{:06}", index + 1);
        if entry.selectable {
            folders.push(ServerFolder {
                index,
                id: entry.id.clone(),
                path: entry.path.clone(),
                collection: rank > 0,
                uid_validity: 0,
                seen: HashSet::new(),
                taken: HashSet::new(),
            });
        }
        table.push(entry);
    }
    let warnings = capped_warnings(warnings);
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        status: Status::Incomplete,
        source: Source::Server,
        since,
        started_at: Utc::now(),
        finished_at: None,
        account: Account { address: address.to_string(), server: None },
        counts: Counts { folders: table.len() as u64, messages: 0, bytes: 0 },
        folders: table,
        messages: Vec::new(),
        aliases: Vec::new(),
        warnings: warnings.iter().map(SourceWarning::archive_text).collect(),
    };
    ServerPlan { manifest, folders, warnings, selectability }
}

/// A server backup (MB.3) on an IMAP session of its own ([`OwnSession`]), read-only throughout:
/// `LIST`, `EXAMINE`, `UID SEARCH` and `UID FETCH`.
///
/// Every selectable folder is read: metadata (flags at the time of reading, `INTERNALDATE`,
/// `RFC822.SIZE`) in batches, the inclusive date limit applied to the calendar day of
/// `INTERNALDATE` in its own zone before any body is fetched, then each body by the chunk reader
/// into a spool file. Real folders come first, with a final inventory of their own; collections
/// (`\All`, then Gmail's `\Flagged` and `\Important`) follow: a message whose bytes are archived
/// from another folder already becomes an alias; a known `X-GM-MSGID` of the same size makes one
/// without downloading it. A final inventory of all folders then takes mail that arrived
/// meanwhile and fails on anything it cannot vouch for ([`SourceProblem`]); only then the archive
/// is finished, verified and published. Mail that a collection took as its own EML before it
/// showed up in its real folder becomes an alias there, so no bytes are archived twice.
async fn run_server_backup(
    mail: Arc<MailService>,
    dirs: MailDirs,
    account: &str,
    since: Option<NaiveDate>,
    filename: &str,
    target_dir: Option<&Path>,
    progress: BackupProgress<'_>,
) -> Result<VerifiedArchive, BackupRunError> {
    let address = mail_address(account)?;
    progress.go_on()?;
    progress.phase(BackupPhase::Counting);
    let mut own = OwnSession::open(&mail, account).await?;
    let target = target_dir.map(Path::to_path_buf);
    let result = server_backup(own.session(), dirs, address, since, filename, target, progress).await;
    own.close().await;
    result
}

async fn server_backup(
    session: &mut Session,
    dirs: MailDirs,
    address: &str,
    since: Option<NaiveDate>,
    filename: &str,
    target_dir: Option<PathBuf>,
    progress: BackupProgress<'_>,
) -> Result<VerifiedArchive, BackupRunError> {
    let gmail = session.capabilities().has(GMAIL);
    let ServerPlan { manifest, mut folders, warnings, selectability } =
        server_plan(session.list().await?, gmail, address, since);
    progress.counts(&manifest.counts);
    let space = progress.jobs.space.clone();
    let archive = OpenArchive::create(dirs, filename.to_string(), target_dir, manifest, space).await?;
    let mut run = ServerRun { session, archive, since, progress, by_msgid: HashMap::new(), collected: HashSet::new() };
    run.progress.phase(BackupPhase::Reading);
    let real = folders.iter().position(|folder| folder.collection).unwrap_or(folders.len());
    // Mail that arrived in a real folder while the others were read is taken there before any
    // collection is read: otherwise the collection would archive it as its own.
    run.read(&mut folders[..real]).await?;
    run.final_inventory(&mut folders[..real], &selectability).await?;
    run.read(&mut folders[real..]).await?;
    run.final_inventory(&mut folders, &selectability).await?;
    let checked_at = Utc::now();
    let manifest = run.archive.writer()?.manifest_mut();
    for folder in &folders {
        manifest.folders[folder.index].inventory = Some(Inventory {
            checked_at,
            eligible_messages: folder.taken.len() as u64,
            uid_set_sha256: Some(uid_inventory_hash(&folder.taken)),
        });
    }
    run.progress.go_on()?;
    run.progress.phase(BackupPhase::Verifying);
    let state = Arc::clone(&run.progress.state);
    run.archive.publish(Status::Complete, Publish::CompleteOnly, state, &warnings).await
}

/// `EXAMINE` of `path`; a folder that no longer exists means the folder list changed.
async fn examine(session: &mut Session, path: &str) -> Result<Selected, BackupRunError> {
    match session.examine(path).await {
        Err(ImapError::NotFound(_)) => Err(BackupRunError::Source(SourceProblem::InventoryChanged)),
        other => Ok(other?),
    }
}

/// What a server backup reads with and writes into.
struct ServerRun<'a, 'p> {
    session: &'a mut Session,
    archive: OpenArchive,
    since: Option<NaiveDate>,
    progress: BackupProgress<'p>,
    /// Gmail: the first archived message (its place in the manifest) and its length by
    /// `X-GM-MSGID`, from any folder.
    by_msgid: HashMap<u64, (usize, u64)>,
    /// The ZIP paths of the EMLs a collection archived as its own.
    collected: HashSet<String>,
}

impl ServerRun<'_, '_> {
    /// Reads `folders` one after the other: `EXAMINE`, the inventory, then every message.
    async fn read(&mut self, folders: &mut [ServerFolder]) -> Result<(), BackupRunError> {
        for folder in folders {
            self.progress.go_on()?;
            let selected = examine(self.session, &folder.path).await?;
            folder.uid_validity = selected.uid_validity;
            self.archive.writer()?.manifest_mut().folders[folder.index].uid_validity = Some(selected.uid_validity);
            let uids = self.session.uid_search(&Search::All).await?;
            folder.seen = uids.iter().copied().collect();
            self.take(folder, &uids).await?;
        }
        Ok(())
    }

    /// Archives the messages `uids` (ascending) of `folder`, which is open: metadata in batches,
    /// then each message inside the date limit. A UID the server no longer names has vanished.
    async fn take(&mut self, folder: &mut ServerFolder, uids: &[u32]) -> Result<(), BackupRunError> {
        for batch in uids.chunks(METADATA_BATCH) {
            self.progress.go_on()?;
            let mut found: HashMap<u32, BackupMessage> = HashMap::with_capacity(batch.len());
            for meta in self.session.fetch_backup_metadata(batch).await? {
                // A server may add unasked answers; only the asked UIDs count, each once.
                if batch.binary_search(&meta.uid).is_ok() && found.insert(meta.uid, meta).is_some() {
                    return Err(BackupRunError::Source(SourceProblem::IdentifiersChanged));
                }
            }
            for uid in batch {
                let meta = found.remove(uid).ok_or(BackupRunError::Source(SourceProblem::Vanished))?;
                if self.since.is_some_and(|day| meta.internal_date.date_naive() < day) {
                    continue;
                }
                self.take_one(folder, meta).await?;
                folder.taken.insert(*uid);
                let counts = self.archive.writer()?.manifest_mut().counts.clone();
                self.progress.counts(&counts);
            }
        }
        Ok(())
    }

    /// One message of the open `folder`, as an alias of an archived message with the same bytes
    /// or as a new EML. A collection aliases any archived message of another folder. A real folder
    /// aliases only an EML a collection archived as its own (the mail arrived after the real
    /// folder was read); identical mail in two real folders stays two EMLs.
    async fn take_one(&mut self, folder: &ServerFolder, meta: BackupMessage) -> Result<(), BackupRunError> {
        let (uid, bytes, msgid) = (meta.uid, meta.bytes, meta.gm_msgid);
        let metadata = MessageMetadata {
            uid: Some(uid),
            uid_validity: Some(folder.uid_validity),
            internal_date: Some(meta.internal_date),
            flags: meta.flags,
            expected_bytes: Some(bytes),
        };
        // Gmail names a message alike in every label: one known with the same size needs no download.
        let known = msgid.and_then(|msgid| self.by_msgid.get(&msgid)).filter(|(_, known)| *known == bytes);
        if let Some(&(index, _)) = known {
            let writer = self.archive.writer()?;
            let same_as = writer
                .manifest_mut()
                .messages
                .get(index)
                .filter(|message| message.folder_id != folder.id)
                .map(|message| message.path.clone())
                .filter(|path| folder.collection || self.collected.contains(path));
            if let Some(same_as) = same_as {
                writer.add_alias(&folder.id, &same_as, metadata)?;
                return Ok(());
            }
        }
        let (spool, sha256, length) = self.spool(uid, bytes).await?;
        let writer = self.archive.writer()?;
        let same_as = writer
            .archived_path(&folder.id, &sha256, length)
            .filter(|path| folder.collection || self.collected.contains(*path))
            .map(str::to_string);
        if let Some(same_as) = same_as {
            writer.add_alias(&folder.id, &same_as, metadata)?;
            return Ok(());
        }
        self.archive.add_message(&folder.id, spool, metadata).await?;
        let messages = &self.archive.writer()?.manifest_mut().messages;
        let index = messages.len().saturating_sub(1);
        if folder.collection
            && let Some(message) = messages.last()
        {
            self.collected.insert(message.path.clone());
        }
        if let Some(msgid) = msgid {
            self.by_msgid.entry(msgid).or_insert((index, bytes));
        }
        Ok(())
    }

    /// The body of message `uid` (`size` by `RFC822.SIZE`) in a spool file, by the chunk reader,
    /// which also checks that nothing follows `size`.
    async fn spool(&mut self, uid: u32, size: u64) -> Result<(File, String, u64), BackupRunError> {
        let mut spool = Spool::new(self.archive.space()).await?;
        let mut body = BodyChunks::new(uid, size);
        loop {
            self.progress.go_on()?;
            match body.next(self.session).await? {
                Some(piece) => spool = spool.write(piece).await?,
                None => return Ok(spool.finish()),
            }
        }
    }

    /// The final inventory, after every body was read: an earlier folder may change while a later
    /// one is read, so each folder is examined again. New UIDs under the same UIDVALIDITY are taken
    /// now (the archive is still open); a pass that finds nothing new ends it. After
    /// [`FINAL_PASSES`] passes that all found new mail, or on a vanished UID, a new UIDVALIDITY or
    /// a changed folder list, the run is incomplete.
    async fn final_inventory(
        &mut self,
        folders: &mut [ServerFolder],
        listed: &HashMap<String, bool>,
    ) -> Result<(), BackupRunError> {
        for _ in 0..FINAL_PASSES {
            self.progress.go_on()?;
            if selectability(&self.session.list().await?) != *listed {
                return Err(BackupRunError::Source(SourceProblem::InventoryChanged));
            }
            let mut found_new = false;
            for folder in folders.iter_mut() {
                self.progress.go_on()?;
                if examine(self.session, &folder.path).await?.uid_validity != folder.uid_validity {
                    return Err(BackupRunError::Source(SourceProblem::IdentifiersChanged));
                }
                let now: HashSet<u32> = self.session.uid_search(&Search::All).await?.into_iter().collect();
                if !folder.seen.is_subset(&now) {
                    return Err(BackupRunError::Source(SourceProblem::Vanished));
                }
                let mut new: Vec<u32> = now.difference(&folder.seen).copied().collect();
                if new.is_empty() {
                    continue;
                }
                new.sort_unstable();
                found_new = true;
                folder.seen.extend(new.iter().copied());
                self.take(folder, &new).await?;
            }
            if !found_new {
                return Ok(());
            }
        }
        Err(BackupRunError::Source(SourceProblem::InventoryChanged))
    }
}

/// What the first scan of the original depot found: the originals this backup takes and their
/// folders. Kept as `seq`s only, so even a large depot costs eight bytes per original here.
#[derive(Default)]
struct LocalInventory {
    /// Taken originals by rising `seq`.
    included: Vec<i64>,
    /// Their metadata without flags ([`hash_local_metadata`]).
    digest: Sha256,
    /// Taken originals per folder path.
    folders: BTreeMap<String, u64>,
    /// The one UIDVALIDITY of a folder's taken originals; `None` when they span several.
    validities: HashMap<String, Option<u32>>,
    /// Originals left out because their mail is no longer on the server.
    gone: u64,
    /// The bytes of the taken originals.
    bytes: u64,
}

/// A local backup reads only retained RFC-822 originals, never the server (MB.4). It takes what
/// the depot holds when it starts ([`MailStore::original_snapshot`](super::store::MailStore::original_snapshot)):
/// originals taken later, flags changed meanwhile and mail leaving the server meanwhile do not
/// disturb it; flags are those at the time each original is read. It fails only when an original
/// it takes disappears or its bytes change. Each BLOB is read through SQLite in bounded chunks.
/// Originals of mail no longer on the server are left out and counted; headers without an original
/// leave the archive `incomplete`, with their number as a warning.
async fn run_local_backup(
    mail: Arc<MailService>,
    dirs: MailDirs,
    account: &str,
    since: Option<NaiveDate>,
    filename: &str,
    target_dir: Option<&Path>,
    progress: BackupProgress<'_>,
) -> Result<VerifiedArchive, BackupRunError> {
    let address = mail_address(account)?;
    progress.go_on()?;
    progress.phase(BackupPhase::Counting);
    let store = mail.store(account).await?;
    let started_at = Utc::now();
    // One store call, so the bound and the gap count describe the same moment.
    let (snapshot, missing, infos) = store
        .run(move |db| Ok((db.original_snapshot()?, db.known_missing_originals(since)?, db.folder_infos()?)))
        .await?;
    let excluded = |original: &LocalOriginal| since.is_some_and(|day| original.internal_date.date_naive() < day);
    let mut inventory = LocalInventory::default();
    let mut after = 0i64;
    'scan: loop {
        progress.go_on()?;
        let page = store.run(move |db| db.local_originals_page(after, 256)).await?;
        if page.is_empty() {
            break;
        }
        for original in page {
            if original.seq > snapshot {
                break 'scan;
            }
            after = original.seq;
            if excluded(&original) {
                continue;
            }
            if original.gone {
                inventory.gone += 1;
                continue;
            }
            inventory.included.push(original.seq);
            inventory.bytes = inventory.bytes.saturating_add(original.bytes);
            *inventory.folders.entry(original.folder_path.clone()).or_default() += 1;
            inventory
                .validities
                .entry(original.folder_path.clone())
                .and_modify(|validity| {
                    if *validity != Some(original.uid_validity) {
                        *validity = None;
                    }
                })
                .or_insert(Some(original.uid_validity));
            hash_local_metadata(&mut inventory.digest, &original);
        }
    }
    let infos: HashMap<String, FolderInfo> = infos.into_iter().map(|info| (info.path.clone(), info)).collect();
    let mut folder_ids = HashMap::new();
    let folders = inventory
        .folders
        .keys()
        .enumerate()
        .map(|(at, path)| {
            let id = format!("f{:06}", at + 1);
            folder_ids.insert(path.clone(), id.clone());
            local_folder(id, path, infos.get(path), inventory.validities.get(path).copied().flatten())
        })
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    if missing > 0 {
        warnings
            .push(SourceWarning::new("{n} local mail headers without an original were omitted.", &[("n", &missing)]));
    }
    if inventory.gone > 0 {
        warnings.push(SourceWarning::new(
            "{n} originals of mail no longer on the server were skipped.",
            &[("n", &inventory.gone)],
        ));
    }
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
        aliases: Vec::new(),
        warnings: warnings.iter().map(SourceWarning::archive_text).collect(),
    };
    progress.counts(&manifest.counts);
    let target = target_dir.map(Path::to_path_buf);
    let space = progress.jobs.space.clone();
    let mut archive = OpenArchive::create(dirs, filename.to_string(), target, manifest, space).await?;
    // The originals' bytes are known now: the archive needs about as much room besides the reserve.
    let watch = Arc::clone(archive.space());
    let mail_bytes = inventory.bytes;
    blocking(move || watch.room_for(mail_bytes)).await?;
    let mut read_digest = Sha256::new();
    let mut next = 0usize;
    let mut after = 0i64;
    progress.phase(BackupPhase::Reading);
    'read: loop {
        progress.go_on()?;
        let page = store.run(move |db| db.local_originals_page(after, 256)).await?;
        if page.is_empty() {
            break;
        }
        for original in page {
            after = original.seq;
            let Some(&wanted) = inventory.included.get(next) else { break 'read };
            if original.seq < wanted {
                continue;
            }
            if original.seq > wanted {
                // A taken original is no longer in the depot.
                return Err(BackupRunError::Source(SourceProblem::LocalDamaged));
            }
            next += 1;
            hash_local_metadata(&mut read_digest, &original);
            let spool = read_local_original(&store, &original, archive.space(), &progress).await?;
            let id =
                folder_ids.get(&original.folder_path).ok_or(BackupRunError::Source(SourceProblem::LocalChanged))?;
            let provenance =
                inventory.validities.get(&original.folder_path).copied().flatten() == Some(original.uid_validity);
            let metadata = MessageMetadata {
                uid: provenance.then_some(original.uid),
                uid_validity: provenance.then_some(original.uid_validity),
                internal_date: Some(original.internal_date),
                flags: original.flags,
                expected_bytes: Some(original.bytes),
            };
            archive.add_message(id, spool, metadata).await?;
            let counts = archive.writer()?.manifest_mut().counts.clone();
            progress.counts(&counts);
        }
    }
    if next != inventory.included.len() {
        return Err(BackupRunError::Source(SourceProblem::LocalDamaged));
    }
    // Same originals, same places, same bytes as counted: else what the folder table and the
    // inventory say would not be what was read.
    if inventory.digest.finalize() != read_digest.finalize() {
        return Err(BackupRunError::Source(SourceProblem::LocalChanged));
    }
    let checked_at = Utc::now();
    for folder in &mut archive.writer()?.manifest_mut().folders {
        let count = inventory.folders.get(&folder.path).copied().unwrap_or(0);
        folder.inventory = Some(Inventory { checked_at, eligible_messages: count, uid_set_sha256: None });
    }
    progress.go_on()?;
    progress.phase(BackupPhase::Verifying);
    // Known gaps are reported in the archive itself; it is never called complete then.
    let status = if missing == 0 { Status::Complete } else { Status::Incomplete };
    archive.publish(status, Publish::WithGaps, Arc::clone(&progress.state), &warnings).await
}

/// The archive's entry for a folder of local originals: name, delimiter and role as the sync last
/// listed the folder, else its path alone.
fn local_folder(id: String, path: &str, info: Option<&FolderInfo>, uid_validity: Option<u32>) -> Folder {
    Folder {
        id,
        name: info.map_or_else(|| path.to_string(), |info| info.name.clone()),
        path: path.to_string(),
        delimiter: info.and_then(|info| info.delimiter).map(|delimiter| delimiter.to_string()),
        selectable: true,
        role: info
            .and_then(|info| info.role.clone())
            .or_else(|| path.eq_ignore_ascii_case("INBOX").then(|| "inbox".to_string())),
        uid_validity,
        inventory: None,
    }
}

/// Copies original `original` from the depot into a spool file next to the archive (`space`), 64
/// KiB at a time, and checks its length and SHA-256.
async fn read_local_original(
    store: &SharedStore,
    original: &LocalOriginal,
    space: &Arc<SpaceWatch>,
    progress: &BackupProgress<'_>,
) -> Result<File, BackupRunError> {
    let mut spool = Spool::new(space).await?;
    let mut offset = 0u64;
    while offset < original.bytes {
        progress.go_on()?;
        let count = (original.bytes - offset).min(64 * 1024) as usize;
        let seq = original.seq;
        let chunk = store.run(move |db| db.original_chunk(seq, offset, count)).await?;
        let Some(chunk) = chunk else {
            return Err(BackupRunError::Source(SourceProblem::LocalDamaged));
        };
        if chunk.len() != count {
            return Err(BackupRunError::Source(SourceProblem::LocalDamaged));
        }
        spool = spool.write(chunk).await?;
        offset += count as u64;
    }
    let (file, hash, _) = spool.finish();
    if hash != original.sha256 {
        return Err(BackupRunError::Source(SourceProblem::LocalDamaged));
    }
    Ok(file)
}

/// Feeds what identifies a taken original into `hash`: its `seq`, place, `INTERNALDATE`, length
/// and SHA-256, each text with its length so no two lists hash alike. Flags are left out on
/// purpose: the sync refreshes them while a backup runs, and the archive keeps them as read.
fn hash_local_metadata(hash: &mut Sha256, original: &LocalOriginal) {
    hash.update(original.seq.to_be_bytes());
    hash.update(original.uid_validity.to_be_bytes());
    hash.update(original.uid.to_be_bytes());
    hash.update(original.bytes.to_be_bytes());
    let date = original.internal_date.to_rfc3339();
    for text in [original.folder_path.as_str(), date.as_str(), original.sha256.as_str()] {
        hash.update((text.len() as u64).to_be_bytes());
        hash.update(text.as_bytes());
    }
}

// ----- Restore (MB.5): test run and real restore -----

/// Why a restore cannot go on as planned (#267). Each reason has its fixed error code and text for
/// the person ([`RestoreProblem::code`], [`RestoreProblem::user_message`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestoreProblem {
    /// The archive was made from another account than the target.
    OtherAccount,
    /// A folder name in the archive cannot be sent safely (line break or NUL).
    UnsafeFolderName,
    /// No plan with this id for this caller and account, or it expired.
    PlanGone,
    /// The archive's manifest is not the one the plan was made from.
    ArchiveChanged,
    /// The target lost mail or folders the plan counted on, a folder got a new UIDVALIDITY, or the
    /// writes would now go beyond the reviewed plan.
    TargetChanged,
    /// The archive is incomplete and the request did not accept that (`allow_incomplete`).
    IncompleteNotAccepted,
    /// Uploads of an earlier restore were never confirmed and the request did not ask to send them
    /// again (`retry_unconfirmed`).
    UnconfirmedNotAccepted,
    /// Some mail cannot be placed safely (the plan's `uncertain`).
    Uncertain,
    /// The restore journal of this archive is unreadable, unsafe or belongs to another archive.
    JournalDamaged,
}

impl RestoreProblem {
    /// The API's error code.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::OtherAccount | Self::PlanGone => "not-found",
            Self::UnsafeFolderName | Self::JournalDamaged => "error",
            Self::ArchiveChanged
            | Self::TargetChanged
            | Self::IncompleteNotAccepted
            | Self::UnconfirmedNotAccepted
            | Self::Uncertain => "conflict",
        }
    }

    /// For the person.
    pub(crate) fn user_message(self) -> &'static str {
        match self {
            // The words of a missing archive: a client must not tell the two apart (#267).
            Self::OtherAccount => tr("Backup archive is not available."),
            Self::UnsafeFolderName => tr("The archive names a folder that cannot be restored safely."),
            Self::PlanGone => tr("Restore plan is no longer available."),
            Self::ArchiveChanged => tr("The archive changed after the restore test. Run the restore test again."),
            Self::TargetChanged => tr("The mail account changed after the restore test. Run the restore test again."),
            Self::IncompleteNotAccepted => tr("Confirm restoring this incomplete archive first."),
            Self::UnconfirmedNotAccepted => {
                tr("An earlier restore left uploads that were never confirmed. Confirm uploading them again first.")
            }
            Self::Uncertain => tr("Some mail cannot be restored safely. Check the restore test first."),
            Self::JournalDamaged => tr("The restore journal of this archive is damaged."),
        }
    }

    /// For logs: English and fixed.
    fn as_str(self) -> &'static str {
        match self {
            Self::OtherAccount => "the archive belongs to another account",
            Self::UnsafeFolderName => "unsafe folder name in the archive",
            Self::PlanGone => "restore plan expired or unknown",
            Self::ArchiveChanged => "the archive changed after the plan",
            Self::TargetChanged => "the target changed after the plan",
            Self::IncompleteNotAccepted => "incomplete archive not accepted",
            Self::UnconfirmedNotAccepted => "unconfirmed uploads not accepted",
            Self::Uncertain => "uncertain mail in the plan",
            Self::JournalDamaged => "restore journal damaged",
        }
    }
}

/// What a request to apply a plan accepts besides the plan itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RestoreChoice {
    /// Restore an archive whose manifest is `incomplete`.
    pub allow_incomplete: bool,
    /// Upload again the mail an earlier restore could not confirm and the target still lacks.
    pub retry_unconfirmed: bool,
}

/// Which plan [`RestorePlans::apply`] applies, from which archive file, for whom.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ApplyRequest<'a> {
    /// The archive the plan was made from, resolved for the caller.
    pub path: &'a Path,
    pub account: &'a str,
    /// The plan's id.
    pub plan: &'a str,
    /// The client that asks; `None` for the app. Only its own plans count.
    pub caller: Option<&'a str>,
    pub choice: RestoreChoice,
}

/// At most this many plans per client key; making another drops its oldest.
const CLIENT_PLANS: usize = 4;
/// At most this many plans of the app (the person's own window and CLI); the oldest goes first.
const APP_PLANS: usize = 16;
/// At most this many plans in all; beyond that a new plan is `Busy`.
const ALL_PLANS: usize = 128;
/// How long a plan may be applied after its test run.
const PLAN_VALID: Duration = Duration::from_secs(3600);
/// At most this many warnings in a plan or result answer; the last says how many were left out.
const ANSWER_WARNINGS: usize = 50;
/// At most this many folder rows in a plan answer; the totals always cover all folders.
const ANSWER_FOLDERS: usize = 500;
/// Folder names in warnings are cut to this many characters.
const SHOWN_PATH: usize = 200;

/// Archive roles of folders that only collected mail of other folders ([`archive_role`]).
fn aggregate_role(role: Option<&str>) -> bool {
    matches!(role, Some("all" | "flagged" | "important"))
}

/// The target folder that fills a folder of archive role `role` by itself: a selectable folder
/// with the matching attribute (`\All`/`\AllMail` anywhere, `\Flagged` only on Gmail). Only there
/// may the aliases of such a folder be left to the server. Gmail's `\Important` is never one: Gmail
/// sets it by its own rules, so an upload to the inbox does not bring the mark back.
fn collecting_folder<'l>(role: Option<&str>, listed: &'l [ListedFolder], gmail: bool) -> Option<&'l ListedFolder> {
    let attributes: &[&str] = match role {
        Some("all") => &ALL_MAIL,
        Some("flagged") if gmail => &["\\Flagged"],
        _ => return None,
    };
    listed
        .iter()
        .find(|folder| folder.selectable() && attributes.iter().any(|attribute| folder.has_attribute(attribute)))
}

/// A folder name as people read it: the mailbox name on the wire ([`encode_mailbox`], so raw text
/// in an archive stays as it is) with its modified UTF-7 decoded. `Entw&APw-rfe` is `Entwürfe`.
fn display_name(path: &str) -> String {
    folders::decode_utf7(&encode_mailbox(path))
}

/// A folder name for a warning: [`display_name`], cut to [`SHOWN_PATH`] characters with "…".
/// The name comes from the archive or the server: data for the person, never an instruction.
fn shown_path(path: &str) -> String {
    let text = display_name(path);
    if text.chars().count() <= SHOWN_PATH {
        return text;
    }
    let mut cut: String = text.chars().take(SHOWN_PATH - 1).collect();
    cut.push('…');
    cut
}

/// `warnings` cut to `max`: beyond it the last place says how many more were left out.
fn capped_to(mut warnings: Vec<SourceWarning>, max: usize) -> Vec<SourceWarning> {
    if warnings.len() > max {
        let left_out = warnings.len() - (max - 1);
        warnings.truncate(max - 1);
        warnings.push(SourceWarning::new("{n} further warnings were left out.", &[("n", &left_out)]));
    }
    warnings
}

/// Archive warnings for a verify answer: at most [`ANSWER_WARNINGS`], the last saying how many more
/// were left out. They come from the archive (English, each within the format's text limit): data
/// for the person, never instructions.
pub(crate) fn shown_archive_warnings(mut warnings: Vec<String>) -> Vec<String> {
    if warnings.len() > ANSWER_WARNINGS {
        let left_out = warnings.len() - (ANSWER_WARNINGS - 1);
        warnings.truncate(ANSWER_WARNINGS - 1);
        warnings.push(trf("{n} further warnings were left out.", &[("n", &left_out)]));
    }
    warnings
}

/// Why the mail of an archive folder cannot be placed on the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocked {
    /// The target folder exists but cannot be opened (`\Noselect`, `\NonExistent`, `EXAMINE` refused).
    NotSelectable,
    /// Archive and target use different hierarchy delimiters, and the path holds one of them: the
    /// same name would land in another place of the hierarchy, so it is not guessed.
    Delimiter,
    /// An earlier archive folder maps to the same target folder (e.g. `INBOX` and `inbox`).
    Duplicate,
}

/// Where one archive folder goes on the target, decided from the target's folder list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FolderMap {
    /// The path on the target that commands use: the archive path, `INBOX` in any case as the
    /// target spells it, and a path with characters outside ASCII in modified UTF-7.
    target: String,
    /// A collection in the archive that a target folder fills by itself too ([`collecting_folder`]):
    /// it maps onto that folder whatever its name, and its aliases are left to the server.
    aggregate: bool,
    /// Missing on the target and selectable in the archive: the restore creates it.
    create: bool,
    blocked: Option<Blocked>,
}

/// Maps every archive folder (in manifest order) onto the target folders `listed` of a server
/// that is Gmail or not (`gmail`). Paths are kept exactly; only `INBOX` matches in any case
/// (RFC 3501), and a non-ASCII path is sent in modified UTF-7 ([`encode_mailbox`]).
fn map_folders(manifest: &Manifest, listed: &[ListedFolder], gmail: bool) -> Vec<FolderMap> {
    let target_delimiter = listed
        .iter()
        .find(|folder| folder.path.eq_ignore_ascii_case("INBOX"))
        .and_then(|folder| folder.delimiter)
        .or_else(|| listed.iter().find_map(|folder| folder.delimiter));
    let mut taken = HashSet::new();
    manifest
        .folders
        .iter()
        .map(|folder| {
            let collecting = collecting_folder(folder.role.as_deref(), listed, gmail);
            let target = if let Some(collecting) = collecting {
                collecting.path.clone()
            } else if folder.path.eq_ignore_ascii_case("INBOX") {
                listed
                    .iter()
                    .find(|listed| listed.path.eq_ignore_ascii_case("INBOX"))
                    .map_or_else(|| "INBOX".to_string(), |listed| listed.path.clone())
            } else {
                encode_mailbox(&folder.path).into_owned()
            };
            let on_target = listed.iter().find(|listed| listed.path == target);
            let archive_delimiter = folder.delimiter.as_deref().and_then(|text| text.chars().next());
            let blocked = if !taken.insert(target.clone()) {
                Some(Blocked::Duplicate)
            } else if collecting.is_none()
                && let (Some(ours), Some(theirs)) = (archive_delimiter, target_delimiter)
                && ours != theirs
                && (folder.path.contains(ours) || folder.path.contains(theirs))
            {
                Some(Blocked::Delimiter)
            } else if on_target.is_some_and(|listed| !listed.selectable()) {
                Some(Blocked::NotSelectable)
            } else {
                None
            };
            FolderMap {
                create: folder.selectable && on_target.is_none() && blocked.is_none(),
                aggregate: collecting.is_some(),
                target,
                blocked,
            }
        })
        .collect()
}

/// One mail a restore places in a target folder: an archived message, or an alias whose folder the
/// target does not fill by itself, uploaded with the bytes of the message it names.
#[derive(Debug, Clone)]
struct RestoreItem<'m> {
    /// Its entry in the journal: the message's ZIP path, or `alias:<n>` for the n-th alias.
    key: String,
    /// An alias, not the archived message itself.
    alias: bool,
    /// Its archive folder (place in the manifest's folder table).
    at: usize,
    /// The archived message whose bytes go up (place in `Manifest::messages`).
    source: usize,
    bytes: u64,
    sha256: &'m str,
    flags: &'m [String],
    internal_date: Option<DateTime<FixedOffset>>,
}

/// What a restore does with the entries of an archive, given where its folders go.
#[derive(Debug, Default)]
struct RestoreItems<'m> {
    /// The mail to place: real folders first, then collections, each in archive order. A target
    /// like Gmail shows mail uploaded to a real folder in "All Mail" too; the other way round it
    /// would get it twice.
    items: Vec<RestoreItem<'m>>,
    /// Aliases the target shows by itself ([`FolderMap::aggregate`]): their archive folder and the
    /// message they name. Each counts as restored once that message is.
    covered: Vec<(usize, usize)>,
    /// Entries naming a folder or message the manifest lacks: never placed, always uncertain.
    lost: u64,
}

/// The [`RestoreItems`] of `manifest` with its folders mapped as `maps`.
fn restore_items<'m>(manifest: &'m Manifest, maps: &[FolderMap]) -> RestoreItems<'m> {
    let places: HashMap<&str, usize> =
        manifest.folders.iter().enumerate().map(|(at, folder)| (folder.id.as_str(), at)).collect();
    let sources: HashMap<&str, usize> =
        manifest.messages.iter().enumerate().map(|(index, message)| (message.path.as_str(), index)).collect();
    let mut found = RestoreItems::default();
    for (index, message) in manifest.messages.iter().enumerate() {
        let Some(&at) = places.get(message.folder_id.as_str()) else {
            found.lost += 1;
            continue;
        };
        found.items.push(RestoreItem {
            key: message.path.clone(),
            alias: false,
            at,
            source: index,
            bytes: message.bytes,
            sha256: &message.sha256,
            flags: &message.flags,
            internal_date: message.internal_date,
        });
    }
    for (number, alias) in manifest.aliases.iter().enumerate() {
        let (Some(&at), Some(&source)) = (places.get(alias.folder_id.as_str()), sources.get(alias.same_as.as_str()))
        else {
            found.lost += 1;
            continue;
        };
        // Gmail stars a mail by its flag: a starred copy is covered only if the mail it names is
        // uploaded with `\Flagged`, else it goes up into the starred folder.
        let flagged = manifest.folders[at].role.as_deref() != Some("flagged")
            || manifest.messages[source].flags.iter().any(|flag| flag.eq_ignore_ascii_case("\\Flagged"));
        if flagged && maps.get(at).is_some_and(|map| map.aggregate) {
            found.covered.push((at, source));
            continue;
        }
        found.items.push(RestoreItem {
            key: format!("alias:{number}"),
            alias: true,
            at,
            source,
            bytes: manifest.messages[source].bytes,
            sha256: &manifest.messages[source].sha256,
            flags: &alias.flags,
            internal_date: alias.internal_date,
        });
    }
    found.items.sort_by_key(|item| aggregate_role(manifest.folders[item.at].role.as_deref()));
    found
}

/// The sizes of the mail to place in each archive folder (by its place in the manifest): only
/// target messages of such a size can be one of them, so only those are hashed.
fn item_sizes(items: &[RestoreItem<'_>]) -> HashMap<usize, HashSet<u64>> {
    let mut sizes: HashMap<usize, HashSet<u64>> = HashMap::new();
    for item in items {
        sizes.entry(item.at).or_default().insert(item.bytes);
    }
    sizes
}

/// One message of a target folder as the read-only look found it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetMessage {
    /// SHA-256 of its raw bytes; only for a message whose size is the size of an archived message
    /// of the folder (any other cannot be one of them).
    sha256: Option<String>,
    internal_date: DateTime<FixedOffset>,
    /// Its transferable flags and keywords in canonical spelling ([`transferable_flags`]).
    flags: Vec<String>,
}

/// One target folder the restore places mail in, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetFolder {
    uid_validity: u32,
    messages: BTreeMap<u32, TargetMessage>,
    /// How many messages carry each hash.
    copies: HashMap<String, u64>,
    /// `PERMANENTFLAGS` as sent; `None` when the server did not say.
    permanent_flags: Option<Vec<String>>,
}

impl TargetFolder {
    fn new(uid_validity: u32, permanent_flags: Option<Vec<String>>) -> Self {
        Self { uid_validity, messages: BTreeMap::new(), copies: HashMap::new(), permanent_flags }
    }

    fn insert(&mut self, uid: u32, message: TargetMessage) {
        if let Some(sha256) = &message.sha256 {
            *self.copies.entry(sha256.clone()).or_default() += 1;
        }
        if let Some(old) = self.messages.insert(uid, message) {
            self.forget_copy(old.sha256.as_deref());
        }
    }

    fn remove(&mut self, uid: u32) {
        if let Some(old) = self.messages.remove(&uid) {
            self.forget_copy(old.sha256.as_deref());
        }
    }

    fn forget_copy(&mut self, sha256: Option<&str>) {
        if let Some(count) = sha256.and_then(|sha256| self.copies.get_mut(sha256)) {
            *count = count.saturating_sub(1);
        }
    }

    /// Copies of the raw bytes with `sha256` in the folder.
    fn copies(&self, sha256: &str) -> u64 {
        self.copies.get(sha256).copied().unwrap_or(0)
    }

    /// The folder keeps `flag`: system flags always (the restore sends only transferable ones),
    /// keywords when `PERMANENTFLAGS` is unknown, holds `\*` or names them. A keyword the folder
    /// does not keep is not sent: strict servers refuse the whole `APPEND` for it.
    fn keeps(&self, flag: &str) -> bool {
        flag.starts_with('\\')
            || self
                .permanent_flags
                .as_ref()
                .is_none_or(|kept| kept.iter().any(|kept| kept == "\\*" || kept.eq_ignore_ascii_case(flag)))
    }
}

/// What a read-only look at the target found.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetSnapshot {
    /// Every listed path and whether it is selectable.
    listed: HashMap<String, bool>,
    /// The folders mail goes into, by target path.
    folders: HashMap<String, TargetFolder>,
}

/// The transferable flags of `flags` (no `\Recent`, `\Deleted`, unknown system flags or unsafe
/// atoms), system flags in canonical spelling, sorted.
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

/// Archive folder names must be safe to send as IMAP strings.
fn validate_restore_paths(manifest: &Manifest) -> Result<(), BackupRunError> {
    if manifest.folders.iter().any(|folder| folder.path.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))) {
        return Err(BackupRunError::Restore(RestoreProblem::UnsafeFolderName));
    }
    Ok(())
}

/// How hashing one target message ended.
enum Hashed {
    Sha(String),
    /// Gone from the server meanwhile.
    Vanished,
    /// The body did not match its `RFC822.SIZE`: it cannot be an archived message of that size.
    Unreadable,
}

/// The SHA-256 of message `uid` (`bytes` long) of the open folder, read in pieces by `BODY.PEEK[]`.
async fn hash_target_message(session: &mut Session, uid: u32, bytes: u64) -> Result<Hashed, ImapError> {
    let mut hash = Sha256::new();
    let mut body = BodyChunks::new(uid, bytes);
    loop {
        match body.next(session).await {
            Ok(Some(piece)) => hash.update(&piece),
            Ok(None) => return Ok(Hashed::Sha(hex_digest(&hash.finalize()))),
            Err(BodyError::Imap(error)) => return Err(error),
            Err(BodyError::Vanished) => return Ok(Hashed::Vanished),
            Err(BodyError::Truncated | BodyError::Oversized) => return Ok(Hashed::Unreadable),
        }
    }
}

/// Adds the messages `uids` (ascending) of the open folder to `folder`: metadata in batches, and
/// the hash of each whose size is one of `sizes`. A UID gone meanwhile is left out.
async fn add_target_messages(
    session: &mut Session,
    folder: &mut TargetFolder,
    uids: &[u32],
    sizes: &HashSet<u64>,
) -> Result<(), ImapError> {
    for batch in uids.chunks(METADATA_BATCH) {
        let mut found: HashMap<u32, BackupMessage> = HashMap::with_capacity(batch.len());
        for meta in session.fetch_backup_metadata(batch).await? {
            // A server may add unasked answers; only the asked UIDs count.
            if batch.binary_search(&meta.uid).is_ok() {
                found.insert(meta.uid, meta);
            }
        }
        for uid in batch {
            let Some(meta) = found.remove(uid) else { continue };
            let sha256 = if sizes.contains(&meta.bytes) {
                match hash_target_message(session, *uid, meta.bytes).await? {
                    Hashed::Sha(sha256) => Some(sha256),
                    Hashed::Vanished => continue,
                    Hashed::Unreadable => None,
                }
            } else {
                None
            };
            let message =
                TargetMessage { sha256, internal_date: meta.internal_date, flags: transferable_flags(&meta.flags) };
            folder.insert(*uid, message);
        }
    }
    Ok(())
}

/// Reads the target folder `path` read-only (`EXAMINE`, `UID SEARCH`, `UID FETCH`). Hashes of
/// `known` are reused for its UIDs under the same UIDVALIDITY: a UID names the same bytes as long
/// as that holds. `None` when the server refuses to open it.
async fn read_target_folder(
    session: &mut Session,
    path: &str,
    sizes: &HashSet<u64>,
    known: Option<&TargetFolder>,
) -> Result<Option<TargetFolder>, ImapError> {
    let selected = match session.examine(path).await {
        Ok(selected) => selected,
        Err(ImapError::NotFound(_) | ImapError::Protocol(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let known = known.filter(|known| known.uid_validity == selected.uid_validity);
    let mut folder = TargetFolder::new(selected.uid_validity, selected.permanent_flags);
    let mut unknown = Vec::new();
    for uid in session.uid_search(&Search::All).await? {
        match known.and_then(|known| known.messages.get(&uid)) {
            Some(message) => folder.insert(uid, message.clone()),
            None => unknown.push(uid),
        }
    }
    add_target_messages(session, &mut folder, &unknown, sizes).await?;
    Ok(Some(folder))
}

/// A read-only look at the target for `manifest`: the folder list, the mapping of every archive
/// folder, and each existing target folder that gets mail, reusing what `known` read already.
/// A folder the server refuses to open counts as not selectable.
async fn look_at_target(
    session: &mut Session,
    manifest: &Manifest,
    known: Option<&TargetSnapshot>,
) -> Result<(Vec<FolderMap>, TargetSnapshot), BackupRunError> {
    let listed = session.list().await?;
    let mut maps = map_folders(manifest, &listed, session.capabilities().has(GMAIL));
    let sizes = item_sizes(&restore_items(manifest, &maps).items);
    let mut folders = HashMap::new();
    for (at, map) in maps.iter_mut().enumerate() {
        let Some(sizes) = sizes.get(&at) else { continue };
        if map.blocked.is_some() || map.create || folders.contains_key(&map.target) {
            continue;
        }
        let earlier = known.and_then(|known| known.folders.get(&map.target));
        match read_target_folder(session, &map.target, sizes, earlier).await? {
            Some(folder) => {
                folders.insert(map.target.clone(), folder);
            }
            None => map.blocked = Some(Blocked::NotSelectable),
        }
    }
    let listed = listed
        .into_iter()
        .map(|folder| {
            let selectable = folder.selectable();
            (folder.path, selectable)
        })
        .collect();
    Ok((maps, TargetSnapshot { listed, folders }))
}

/// Since the plan, nothing the plan counted on is gone: every listed folder is still there (and
/// still selectable if it was), and every read folder has its UIDVALIDITY and all its UIDs. New
/// folders and new mail are fine (a collection such as Gmail's "All Mail" grows with every upload
/// elsewhere); they can only lower what is left to do, which [`within_plan`] checks.
fn target_preserves_plan(planned: &TargetSnapshot, current: &TargetSnapshot) -> bool {
    planned.listed.iter().all(|(path, selectable)| current.listed.get(path).is_some_and(|now| *now || !selectable))
        && planned.folders.iter().all(|(path, old)| {
            current.folders.get(path).is_some_and(|now| {
                now.uid_validity == old.uid_validity && old.messages.keys().all(|uid| now.messages.contains_key(uid))
            })
        })
}

/// The writes `preview` needs stay within the reviewed `planned`: no more folders to create, no
/// more mail to add or upload again, folder by folder the same targets.
fn within_plan(preview: &RestorePlan, planned: &RestorePlan) -> bool {
    preview.folders_to_create <= planned.folders_to_create
        && preview.messages_to_add <= planned.messages_to_add
        && preview.unconfirmed <= planned.unconfirmed
        && preview.folders.len() == planned.folders.len()
        && preview.folders.iter().zip(&planned.folders).all(|(now, old)| {
            now.target_path == old.target_path
                && (!now.create || old.create)
                && now.messages_to_add <= old.messages_to_add
        })
}

/// What a restore does with one archived message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// Already in its target folder.
    Present,
    /// To be appended.
    Add,
    /// An earlier restore sent it without a confirmation and the target lacks it: sent again only
    /// with `retry_unconfirmed`.
    Unconfirmed,
    /// Cannot be placed safely.
    Uncertain(Unsure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unsure {
    /// Its folder is blocked ([`Blocked`]).
    Blocked,
    /// An earlier unconfirmed upload does not match the copies now in the folder.
    Unmatched,
}

/// Where `item` stands on the target. `folder` is its target folder as read (`None` while it is
/// still to be created); `used` counts the copies of its bytes in that folder that earlier items
/// already stand for, so identical archived occurrences are counted, not collapsed.
///
/// An open journal entry (copies before its `APPEND`) decides alone: one copy more means the
/// upload arrived; as many as before or fewer (someone removed an identical copy meanwhile) means
/// it cannot be shown to have arrived, so it is unconfirmed and goes up again only with
/// `retry_unconfirmed`; more than one copy more cannot be told. An upload the server stored
/// changed ([`RestoreJournal::altered`]) counts as present while its UID is there.
fn place(
    item: &RestoreItem<'_>,
    map: &FolderMap,
    folder: Option<&TargetFolder>,
    journal: Option<&RestoreJournal>,
    used: &mut u64,
) -> Placement {
    if map.blocked.is_some() {
        return Placement::Uncertain(Unsure::Blocked);
    }
    let pending = journal.and_then(|journal| journal.pending.get(&item.key)).copied();
    let Some(folder) = folder else {
        return match (map.create, pending) {
            (true, None) => Placement::Add,
            _ => Placement::Uncertain(Unsure::Unmatched),
        };
    };
    if let Some(landed) = journal.and_then(|journal| journal.altered.get(&item.key))
        && landed.uid_validity == folder.uid_validity
        && folder.messages.contains_key(&landed.uid)
    {
        return Placement::Present;
    }
    let copies = folder.copies(item.sha256);
    match pending {
        Some(before) if copies == before.saturating_add(1) => {
            *used += 1;
            Placement::Present
        }
        Some(before) if copies <= before => Placement::Unconfirmed,
        Some(_) => Placement::Uncertain(Unsure::Unmatched),
        None if copies > *used => {
            *used += 1;
            Placement::Present
        }
        None => Placement::Add,
    }
}

/// What a restore would do now: per archive folder a row, the totals and the warnings, from the
/// target as `target` shows it and the journal's open entries. Rows are complete (one per archive
/// folder); an answer cuts them ([`shown_plan`]). Aliases in a collection the target fills by
/// itself are `covered`; every other alias is placed like a message ([`restore_items`]).
fn summarize_restore(
    manifest: &Manifest,
    maps: &[FolderMap],
    items: &RestoreItems<'_>,
    target: &TargetSnapshot,
    journal: Option<&RestoreJournal>,
    id: String,
    account: &str,
) -> (RestorePlan, Vec<SourceWarning>) {
    let mut plan = RestorePlan {
        id,
        account: account.into(),
        archive: String::new(),
        complete_archive: manifest.status == Status::Complete,
        folders_to_create: 0,
        messages_to_add: 0,
        already_present: 0,
        uncertain: items.lost,
        unconfirmed: 0,
        bytes_to_add: 0,
        folders: Vec::with_capacity(manifest.folders.len()),
        warnings: Vec::new(),
    };
    let mut warnings = Vec::new();
    if manifest.status != Status::Complete {
        warnings.push(SourceWarning::new("This archive is incomplete; restoring it needs separate confirmation.", &[]));
    }
    for (folder, map) in manifest.folders.iter().zip(maps) {
        plan.folders.push(RestoreFolderPlan {
            archive_path: folder.path.clone(),
            target_path: map.target.clone(),
            archive_display: display_name(&folder.path),
            target_display: display_name(&map.target),
            create: map.create,
            messages_to_add: 0,
            already_present: 0,
            uncertain: 0,
            bytes_to_add: 0,
            aggregate: map.aggregate,
            covered: 0,
        });
        let path = shown_path(&folder.path);
        if map.create {
            plan.folders_to_create += 1;
            warnings.push(SourceWarning::new("Restore will create folder {path}.", &[("path", &path)]));
        }
    }
    let mut used: HashMap<(usize, &str), u64> = HashMap::new();
    // Date and flags of the target copies not yet matched, per folder and hash.
    let mut details: HashMap<(usize, &str), Vec<&TargetMessage>> = HashMap::new();
    let mut unconfirmed: BTreeMap<usize, u64> = BTreeMap::new();
    let mut unmatched: BTreeMap<usize, u64> = BTreeMap::new();
    let (mut dropped_flags, mut different) = (0u64, 0u64);
    for item in &items.items {
        let at = item.at;
        let map = &maps[at];
        let folder = target.folders.get(&map.target);
        let counter = used.entry((at, item.sha256)).or_default();
        let placement = place(item, map, folder, journal, counter);
        let row = &mut plan.folders[at];
        match placement {
            Placement::Present => {
                plan.already_present += 1;
                row.already_present += 1;
                let Some(folder) = folder else { continue };
                let copies = details.entry((at, item.sha256)).or_insert_with(|| {
                    folder.messages.values().filter(|copy| copy.sha256.as_deref() == Some(item.sha256)).collect()
                });
                let flags: Vec<String> =
                    transferable_flags(item.flags).into_iter().filter(|flag| folder.keeps(flag)).collect();
                let same = copies.iter().position(|copy| {
                    item.internal_date.is_none_or(|date| date.timestamp() == copy.internal_date.timestamp())
                        && copy.flags == flags
                });
                match same {
                    Some(index) => {
                        copies.swap_remove(index);
                    }
                    None if copies.pop().is_some() => different += 1,
                    None => {}
                }
            }
            Placement::Add => {
                plan.messages_to_add += 1;
                plan.bytes_to_add = plan.bytes_to_add.saturating_add(item.bytes);
                row.messages_to_add += 1;
                row.bytes_to_add = row.bytes_to_add.saturating_add(item.bytes);
                dropped_flags += item
                    .flags
                    .iter()
                    .filter(|flag| !backup_flag_transferable(flag) || folder.is_some_and(|folder| !folder.keeps(flag)))
                    .count() as u64;
            }
            Placement::Unconfirmed => {
                plan.unconfirmed += 1;
                *unconfirmed.entry(at).or_default() += 1;
            }
            Placement::Uncertain(why) => {
                plan.uncertain += 1;
                row.uncertain += 1;
                if why == Unsure::Unmatched {
                    *unmatched.entry(at).or_default() += 1;
                }
            }
        }
    }
    for (at, _) in &items.covered {
        plan.folders[*at].covered += 1;
    }
    for (at, (folder, map)) in manifest.folders.iter().zip(maps).enumerate() {
        let path = shown_path(&folder.path);
        let has_mail = plan.folders[at].uncertain > 0;
        match map.blocked {
            Some(Blocked::NotSelectable) if has_mail => warnings.push(SourceWarning::new(
                "Folder {path} exists on the target but cannot be opened; its mail is left out.",
                &[("path", &path)],
            )),
            Some(Blocked::Delimiter) if has_mail => warnings.push(SourceWarning::new(
                "Folder {path} uses another hierarchy delimiter than the target; its mail is left out.",
                &[("path", &path)],
            )),
            Some(Blocked::Duplicate) if has_mail => warnings.push(SourceWarning::new(
                "Folder {path} maps to the same target folder as another archive folder; its mail is left out.",
                &[("path", &path)],
            )),
            _ => {}
        }
        if let Some(n) = unconfirmed.get(&at) {
            warnings.push(SourceWarning::new(
                "{n} uploads to {path} of an earlier restore were never confirmed; they are sent again only with your confirmation.",
                &[("n", n), ("path", &path)],
            ));
        }
        if let Some(n) = unmatched.get(&at) {
            warnings.push(SourceWarning::new(
                "{n} uploads to {path} of an earlier restore do not match the mail now in the folder; check the folder.",
                &[("n", n), ("path", &path)],
            ));
        }
    }
    if dropped_flags > 0 {
        warnings.push(SourceWarning::new(
            "{n} IMAP flags or keywords will be left out because the target cannot keep them.",
            &[("n", &dropped_flags)],
        ));
    }
    if different > 0 {
        warnings.push(SourceWarning::new(
            "{n} messages already in the target have another date or other flags; they stay as they are.",
            &[("n", &different)],
        ));
    }
    (plan, warnings)
}

/// The plan as an answer shows it: at most [`ANSWER_FOLDERS`] folder rows (those with something
/// to do first, in archive order) and at most [`ANSWER_WARNINGS`] warnings, in the person's
/// language. The totals always cover every folder.
fn shown_plan(full: &RestorePlan, mut warnings: Vec<SourceWarning>) -> RestorePlan {
    let mut shown = full.clone();
    if shown.folders.len() > ANSWER_FOLDERS {
        let busy = |row: &RestoreFolderPlan| row.create || row.messages_to_add > 0 || row.uncertain > 0;
        let (mut rows, rest): (Vec<_>, Vec<_>) = shown.folders.drain(..).partition(busy);
        rows.extend(rest);
        let left_out = rows.len() - ANSWER_FOLDERS;
        rows.truncate(ANSWER_FOLDERS);
        shown.folders = rows;
        warnings.push(SourceWarning::new("{n} further folders are not listed here.", &[("n", &left_out)]));
    }
    shown.warnings = capped_to(warnings, ANSWER_WARNINGS).iter().map(SourceWarning::user_text).collect();
    shown
}

/// Where an upload the server stored changed landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Landed {
    uid_validity: u32,
    uid: u32,
}

/// The private journal of the restores of one archive into one account (0600, under
/// `<mail data>/restore-journals/`), named by the SHA-256 of the account and the archive's raw
/// `manifest.json` ([`RestoreArchive::manifest_sha256`]). It is written only by a real restore;
/// a test run only reads it. An empty journal is removed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RestoreJournal {
    account: String,
    manifest_sha256: String,
    /// Uploads not yet confirmed: archive path → copies of its bytes in its target folder just
    /// before its `APPEND`.
    #[serde(default)]
    pending: BTreeMap<String, u64>,
    /// Uploads the server stored changed (`APPENDUID` named them, their bytes differ from the
    /// archive): archive path → where they landed. They count as restored while that UID exists.
    #[serde(default)]
    altered: BTreeMap<String, Landed>,
}

impl RestoreJournal {
    fn directory(dirs: &MailDirs) -> PathBuf {
        dirs.data.join("restore-journals")
    }

    fn file(dirs: &MailDirs, account: &str, digest: &str) -> PathBuf {
        let key = Sha256::digest(format!("{account}\0{digest}").as_bytes());
        Self::directory(dirs).join(format!("{}.json", hex_digest(&key)))
    }

    /// The journal of this archive and account, if there is one. Creates nothing (a test run
    /// must not leave a file). Blocking.
    fn read(dirs: &MailDirs, account: &str, digest: &str) -> Result<Option<Self>, BackupRunError> {
        reject_symlink(&dirs.data)?;
        reject_symlink(&Self::directory(dirs))?;
        let path = Self::file(dirs, account, digest);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let damaged = BackupRunError::Restore(RestoreProblem::JournalDamaged);
        if !metadata.is_file() || metadata.len() > 10 * 1024 * 1024 {
            return Err(damaged);
        }
        let journal: Self = serde_json::from_slice(&fs::read(&path)?).map_err(|_| damaged)?;
        if journal.account != account || journal.manifest_sha256 != digest {
            return Err(BackupRunError::Restore(RestoreProblem::JournalDamaged));
        }
        Ok(Some(journal))
    }

    /// The journal of this archive and account (empty when new) and its file, in the private
    /// directory, which is made when missing. Blocking.
    fn open(dirs: &MailDirs, account: &str, digest: &str) -> Result<(PathBuf, Self), BackupRunError> {
        reject_symlink(&dirs.data)?;
        crate::clients::create_private_dir(&dirs.data)?;
        let directory = Self::directory(dirs);
        reject_symlink(&directory)?;
        crate::clients::create_private_dir(&directory)?;
        let journal = Self::read(dirs, account, digest)?.unwrap_or_else(|| Self {
            account: account.into(),
            manifest_sha256: digest.into(),
            ..Self::default()
        });
        Ok((Self::file(dirs, account, digest), journal))
    }

    /// Writes the journal durably (temporary file, `fsync`, rename, `fsync` of the folder), or
    /// removes it when nothing is open. Blocking.
    fn save(&self, path: &Path) -> Result<(), BackupRunError> {
        let directory = path.parent().ok_or(BackupRunError::Restore(RestoreProblem::JournalDamaged))?;
        if self.pending.is_empty() && self.altered.is_empty() {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        } else {
            let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
            serde_json::to_writer(temporary.as_file_mut(), self).map_err(io::Error::other)?;
            temporary.as_file_mut().sync_all()?;
            temporary.persist(path).map_err(|error| BackupRunError::Io(error.error))?;
        }
        #[cfg(unix)]
        File::open(directory)?.sync_all()?;
        Ok(())
    }
}

/// Opens and fully verifies the archive at `path`, on the blocking pool.
async fn open_restore_archive(path: &Path) -> Result<RestoreArchive<File>, BackupRunError> {
    let path = path.to_path_buf();
    blocking(move || Ok(RestoreArchive::open(File::open(path)?)?)).await
}

/// Whether the archive at `path` is of another mail account than `account` (`mail:<address>`),
/// from its manifest alone ([`torro_mail_backup::archive_account`]), before the costly full
/// verification (#267): a client must neither tell an archive of another account from a missing
/// one nor make the core check such an archive in full. `false` when the owner cannot be read (no
/// file, no manifest, damaged): the full verification after it then says why. Blocking.
pub(crate) fn of_other_account(path: &Path, account: &str) -> bool {
    let Ok(file) = File::open(path) else { return false };
    torro_mail_backup::archive_account(file).is_ok_and(|address| !same_account(&address, account))
}

/// The address `address` from an archive names the account `account`: as the id the core makes of
/// it ([`torro_mail::account_id`], as `mail.backup_verify` compares) or exactly (as a restore
/// compares, [`check_restore_archive`]). Either counts, so the early check never refuses what the
/// full one accepts.
fn same_account(address: &str, account: &str) -> bool {
    torro_mail::account_id(address).is_ok_and(|id| id == account)
        || account.strip_prefix(torro_mail::ACCOUNT_PREFIX) == Some(address)
}

/// [`of_other_account`] on the blocking pool: [`RestoreProblem::OtherAccount`] for an archive of
/// another account, before it is verified.
async fn refuse_other_account(path: &Path, account: &str) -> Result<(), BackupRunError> {
    let (path, account) = (path.to_path_buf(), account.to_string());
    if blocking(move || Ok(of_other_account(&path, &account))).await? {
        return Err(BackupRunError::Restore(RestoreProblem::OtherAccount));
    }
    Ok(())
}

/// Checks what every restore checks first: safe folder names and the account the archive is of.
fn check_restore_archive(manifest: &Manifest, account: &str) -> Result<(), BackupRunError> {
    validate_restore_paths(manifest)?;
    if account.strip_prefix(torro_mail::ACCOUNT_PREFIX) != Some(manifest.account.address.as_str()) {
        return Err(BackupRunError::Restore(RestoreProblem::OtherAccount));
    }
    Ok(())
}

struct PlannedRestore {
    /// The client that made it; `None` for the app. Only the same caller sees and applies it.
    owner: Option<String>,
    file: String,
    account: String,
    manifest_sha256: String,
    /// The whole plan (every folder row), as reviewed.
    plan: RestorePlan,
    target: TargetSnapshot,
    created: std::time::Instant,
}

impl PlannedRestore {
    fn valid_for(&self, account: &str, caller: Option<&str>) -> bool {
        self.account == account && self.owner.as_deref() == caller && self.created.elapsed() < PLAN_VALID
    }
}

/// The kept plans and the test runs under way, under one lock so that both count against the
/// same limits ([`RestorePlans::reserve`]).
#[derive(Default)]
struct PlanBook {
    kept: HashMap<String, PlannedRestore>,
    /// Test runs under way per caller (`None`: the app); each holds a [`PlanSlot`].
    running: HashMap<Option<String>, usize>,
}

impl PlanBook {
    fn forget_expired(&mut self) {
        self.kept.retain(|_, item| item.created.elapsed() < PLAN_VALID);
    }

    fn kept_of(&self, caller: Option<&str>) -> usize {
        self.kept.values().filter(|item| item.owner.as_deref() == caller).count()
    }

    fn running_of(&self, caller: Option<&str>) -> usize {
        self.running.get(&caller.map(str::to_string)).copied().unwrap_or(0)
    }

    fn running_total(&self) -> usize {
        self.running.values().sum()
    }
}

/// The place a test run holds in [`RestorePlans`] from before its costly work until its plan is
/// kept ([`RestorePlans::keep`]) or the run ends without one: dropping the slot gives the place
/// back, also after an error or when the request's future is dropped.
struct PlanSlot {
    book: Arc<Mutex<PlanBook>>,
    owner: Option<String>,
    /// Still counted in `PlanBook::running`.
    held: bool,
}

impl PlanSlot {
    /// Gives the place back under the held lock of `book`; afterwards dropping the slot does not.
    fn release_in(&mut self, book: &mut PlanBook) {
        if !std::mem::replace(&mut self.held, false) {
            return;
        }
        if let Some(count) = book.running.get_mut(&self.owner) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                book.running.remove(&self.owner);
            }
        }
    }
}

impl Drop for PlanSlot {
    fn drop(&mut self) {
        if self.held {
            let book = Arc::clone(&self.book);
            let mut book = book.lock().unwrap_or_else(PoisonError::into_inner);
            self.release_in(&mut book);
        }
    }
}

/// The restore plans of the core and the one restore that may run at a time.
#[derive(Clone, Default)]
pub(crate) struct RestorePlans {
    book: Arc<Mutex<PlanBook>>,
    apply_lock: Arc<tokio::sync::Mutex<()>>,
}

impl RestorePlans {
    /// A test run (MB.5): verifies the archive at `path` offline, reads the target `account` on an
    /// IMAP session of its own with read-only commands only, reads the restore journal without
    /// writing it, and keeps the plan for `caller` (`None`: the app) for an hour. `file` is the
    /// archive as the caller named it (the plan's `archive`).
    ///
    /// A client keeps at most [`CLIENT_PLANS`] plans, the app [`APP_PLANS`]: a new one drops the
    /// caller's oldest. Test runs under way count too, from before the archive is opened
    /// ([`RestorePlans::reserve`]): with as many runs of the caller under way as it may keep plans,
    /// or with [`ALL_PLANS`] plans and runs of all callers and no plan of its own to drop, it is
    /// [`BackupRunError::Busy`] at once.
    pub(crate) async fn plan(
        &self,
        mail: &MailService,
        dirs: &MailDirs,
        file: String,
        path: &Path,
        account: &str,
        caller: Option<&str>,
    ) -> Result<RestorePlan, BackupRunError> {
        let slot = self.reserve(caller)?;
        refuse_other_account(path, account).await?;
        let archive = open_restore_archive(path).await?;
        check_restore_archive(archive.manifest(), account)?;
        let digest = archive.manifest_sha256().to_string();
        let journal = {
            let (dirs, account, digest) = (dirs.clone(), account.to_string(), digest.clone());
            blocking(move || RestoreJournal::read(&dirs, &account, &digest)).await?
        };
        let mut own = OwnSession::open(mail, account).await?;
        let looked = look_at_target(own.session(), archive.manifest(), None).await;
        own.close().await;
        let (maps, target) = looked?;
        let id = random_id()?;
        let items = restore_items(archive.manifest(), &maps);
        let (mut plan, warnings) =
            summarize_restore(archive.manifest(), &maps, &items, &target, journal.as_ref(), id.clone(), account);
        plan.archive = file.clone();
        let shown = shown_plan(&plan, warnings);
        plan.warnings = shown.warnings.clone();
        let entry = PlannedRestore {
            owner: caller.map(str::to_string),
            file,
            account: account.into(),
            manifest_sha256: digest,
            plan,
            target,
            created: std::time::Instant::now(),
        };
        self.keep(slot, id, entry)?;
        Ok(shown)
    }

    /// Takes a place for a test run of `caller` before its costly work, or `Busy`: when the caller
    /// already runs as many test runs as it may keep plans, or when plans and runs of all callers
    /// fill [`ALL_PLANS`] and the caller has no kept plan left that its new one could replace
    /// (each of its runs under way may replace one).
    fn reserve(&self, caller: Option<&str>) -> Result<PlanSlot, BackupRunError> {
        let mut book = self.book.lock().unwrap_or_else(PoisonError::into_inner);
        book.forget_expired();
        let limit = if caller.is_some() { CLIENT_PLANS } else { APP_PLANS };
        let running = book.running_of(caller);
        let full = book.kept.len() + book.running_total() >= ALL_PLANS;
        if running >= limit || (full && book.kept_of(caller) <= running) {
            return Err(BackupRunError::Busy);
        }
        let owner = caller.map(str::to_string);
        *book.running.entry(owner.clone()).or_default() += 1;
        Ok(PlanSlot { book: Arc::clone(&self.book), owner, held: true })
    }

    /// Keeps the plan of the run that holds `slot`: drops the caller's oldest plans beyond its
    /// limit, and while all places are taken the caller's oldest plan for the new one.
    fn keep(&self, mut slot: PlanSlot, id: String, entry: PlannedRestore) -> Result<(), BackupRunError> {
        let mut book = self.book.lock().unwrap_or_else(PoisonError::into_inner);
        // Under the same lock as the insert below: the run's place becomes its plan's.
        slot.release_in(&mut book);
        book.forget_expired();
        let limit = if entry.owner.is_some() { CLIENT_PLANS } else { APP_PLANS };
        let mut own: Vec<(String, std::time::Instant)> = book
            .kept
            .iter()
            .filter(|(_, item)| item.owner == entry.owner)
            .map(|(id, item)| (id.clone(), item.created))
            .collect();
        own.sort_by_key(|(_, created)| *created);
        let mut own = own.into_iter();
        let full = |book: &PlanBook| book.kept.len() + book.running_total() >= ALL_PLANS;
        while book.kept_of(entry.owner.as_deref()) >= limit || full(&book) {
            let Some((old, _)) = own.next() else { break };
            book.kept.remove(&old);
        }
        if full(&book) {
            return Err(BackupRunError::Busy);
        }
        book.kept.insert(id, entry);
        Ok(())
    }

    /// The archive as named and the stored plan `id` of `account`, for `caller` only (`None`: the
    /// app); `None` when it is another caller's, expired or unknown.
    pub(crate) fn planned_for(&self, account: &str, id: &str, caller: Option<&str>) -> Option<(String, RestorePlan)> {
        self.book
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .kept
            .get(id)
            .filter(|item| item.valid_for(account, caller))
            .map(|item| (item.file.clone(), item.plan.clone()))
    }

    /// A real restore of plan `id` (MB.5) for `caller`, on an IMAP session of its own. Before any
    /// write it verifies the archive again (the same raw manifest), reads the target again and
    /// refuses (`conflict`) what the reviewed plan does not cover: lost mail or folders, a new
    /// UIDVALIDITY, more writes, uncertain mail, an incomplete archive without
    /// `allow_incomplete`, unconfirmed uploads without `retry_unconfirmed`. Then it creates and
    /// subscribes the missing folders and appends the missing mail in archive order, each upload
    /// journaled before and confirmed after.
    ///
    /// Once a write was sent the answer is always the result so far: a later failure ends the run
    /// with a warning saying why, and the mail not confirmed in the target counts as `uncertain`.
    /// A refused upload (`NO`) is left out and the run goes on. The plan is used up by a result.
    pub(crate) async fn apply(
        &self,
        mail: &MailService,
        dirs: &MailDirs,
        request: ApplyRequest<'_>,
    ) -> Result<RestoreResult, BackupRunError> {
        let ApplyRequest { path, account, plan: id, caller, choice } = request;
        let _serial = self.apply_lock.lock().await;
        let (planned_digest, planned_plan, planned_target) = {
            let book = self.book.lock().unwrap_or_else(PoisonError::into_inner);
            let item = book.kept.get(id).filter(|item| item.valid_for(account, caller));
            let item = item.ok_or(BackupRunError::Restore(RestoreProblem::PlanGone))?;
            (item.manifest_sha256.clone(), item.plan.clone(), item.target.clone())
        };
        refuse_other_account(path, account).await?;
        let archive = open_restore_archive(path).await?;
        if archive.manifest_sha256() != planned_digest {
            return Err(BackupRunError::Restore(RestoreProblem::ArchiveChanged));
        }
        let manifest = archive.manifest().clone();
        check_restore_archive(&manifest, account)?;
        if manifest.status != Status::Complete && !choice.allow_incomplete {
            return Err(BackupRunError::Restore(RestoreProblem::IncompleteNotAccepted));
        }
        if planned_plan.uncertain > 0 {
            return Err(BackupRunError::Restore(RestoreProblem::Uncertain));
        }
        if planned_plan.unconfirmed > 0 && !choice.retry_unconfirmed {
            return Err(BackupRunError::Restore(RestoreProblem::UnconfirmedNotAccepted));
        }
        let journal = {
            let (dirs, account, digest) = (dirs.clone(), account.to_string(), planned_digest.clone());
            blocking(move || RestoreJournal::read(&dirs, &account, &digest)).await?
        };
        let mut own = OwnSession::open(mail, account).await?;
        let setup = RestoreSetup {
            dirs,
            account,
            digest: &planned_digest,
            manifest: &manifest,
            planned: &planned_plan,
            planned_target: &planned_target,
            journal,
            choice,
        };
        // Boxed, as the steps below: a restore a person allowed runs deep in the core's stack
        // (the approval runs the request again), and unboxed futures add up there.
        let outcome = Box::pin(restore_on(own.session(), archive, setup)).await;
        own.close().await;
        if outcome.is_ok() {
            self.book.lock().unwrap_or_else(PoisonError::into_inner).kept.remove(id);
        }
        outcome
    }
}

/// What [`restore_on`] works from.
struct RestoreSetup<'a> {
    dirs: &'a MailDirs,
    account: &'a str,
    digest: &'a str,
    manifest: &'a Manifest,
    planned: &'a RestorePlan,
    planned_target: &'a TargetSnapshot,
    journal: Option<RestoreJournal>,
    choice: RestoreChoice,
}

/// The checks before the first write, then the writes ([`RestorePlans::apply`]).
async fn restore_on(
    session: &mut Session,
    archive: RestoreArchive<File>,
    setup: RestoreSetup<'_>,
) -> Result<RestoreResult, BackupRunError> {
    let manifest = setup.manifest;
    let (maps, target) = Box::pin(look_at_target(session, manifest, Some(setup.planned_target))).await?;
    if !target_preserves_plan(setup.planned_target, &target) {
        return Err(BackupRunError::Restore(RestoreProblem::TargetChanged));
    }
    let items = restore_items(manifest, &maps);
    let (preview, warnings) = summarize_restore(
        manifest,
        &maps,
        &items,
        &target,
        setup.journal.as_ref(),
        setup.planned.id.clone(),
        setup.account,
    );
    if preview.uncertain > 0 {
        return Err(BackupRunError::Restore(RestoreProblem::Uncertain));
    }
    if preview.unconfirmed > 0 && !setup.choice.retry_unconfirmed {
        return Err(BackupRunError::Restore(RestoreProblem::UnconfirmedNotAccepted));
    }
    if !within_plan(&preview, setup.planned) {
        return Err(BackupRunError::Restore(RestoreProblem::TargetChanged));
    }
    let (journal_path, journal) = {
        let (dirs, account, digest) = (setup.dirs.clone(), setup.account.to_string(), setup.digest.to_string());
        blocking(move || RestoreJournal::open(&dirs, &account, &digest)).await?
    };
    let spool_dir = RestoreJournal::directory(setup.dirs);
    let mut run = RestoreRun {
        session,
        maps,
        planned: setup.planned_target,
        target,
        sizes: item_sizes(&items.items),
        archive: Some(archive),
        journal,
        journal_path,
        spool_dir,
        retry: setup.choice.retry_unconfirmed,
        wrote: false,
        added: 0,
        skipped: 0,
        refused: 0,
        altered: 0,
        restored: HashSet::new(),
        warnings,
    };
    let outcome = match Box::pin(run.create_folders()).await {
        Ok(()) => Box::pin(run.upload_all(&items.items)).await,
        Err(stop) => Err(stop),
    };
    match outcome {
        Ok(()) => {}
        Err(stop) if !run.wrote => return Err(stop.into_error()),
        Err(stop) => run.warnings.push(stop.warning()),
    }
    if run.refused > 0 {
        run.warnings.push(SourceWarning::new(
            "{n} messages were refused by the server and not restored; run the restore test again to retry them.",
            &[("n", &run.refused)],
        ));
    }
    if run.altered > 0 {
        run.warnings.push(SourceWarning::new(
            "The server stored {n} restored messages changed; they count as restored and are not uploaded again.",
            &[("n", &run.altered)],
        ));
    }
    // A covered alias is restored only once the message it names is in its own folder: an upload
    // that was refused or never made leaves the copy uncertain as well.
    let covered = items.covered.iter().filter(|(_, source)| run.restored.contains(source)).count() as u64;
    let skipped = run.skipped + covered;
    let total = (manifest.messages.len() + manifest.aliases.len()) as u64;
    let warnings = std::mem::take(&mut run.warnings);
    Ok(RestoreResult {
        added: run.added,
        skipped,
        uncertain: total.saturating_sub(run.added + skipped),
        warnings: capped_to(warnings, ANSWER_WARNINGS).iter().map(SourceWarning::user_text).collect(),
    })
}

/// Why a real restore ended before it was done. Before the first write it becomes an error
/// ([`Stop::into_error`]), after it a warning of the result ([`Stop::warning`]).
#[derive(Debug)]
enum Stop {
    /// The connection failed or the server answered unexpectedly while reading the target.
    Imap(ImapError),
    /// The server refused to open the folder (target path) to check it.
    Unreadable(String),
    /// The archive could not be read or the journal not saved.
    Local(BackupRunError),
    /// The folder (target path) lost mail the plan counted on or got a new UIDVALIDITY.
    TargetChanged(String),
    /// The folder could not be created, or was created but cannot be opened.
    NotCreated(String),
    /// An upload to the folder went out but its answer did not come back.
    Unconfirmed(String),
    /// The connection broke before an upload to the folder was complete.
    NotSent,
    /// The server confirmed an upload to the folder, but no identical copy and no `APPENDUID`
    /// show which mail it stored.
    Differs(String),
    /// More than one new identical copy appeared in the folder after an upload.
    Ambiguous(String),
}

impl Stop {
    fn into_error(self) -> BackupRunError {
        match self {
            Self::Imap(error) => BackupRunError::Imap(error),
            Self::Local(error) => error,
            _ => BackupRunError::Restore(RestoreProblem::TargetChanged),
        }
    }

    fn warning(&self) -> SourceWarning {
        match self {
            Self::Imap(_) | Self::NotSent => SourceWarning::new(
                "The restore stopped: the connection to the server was lost. Run the restore test again to continue.",
                &[],
            ),
            Self::Local(_) => {
                SourceWarning::new("The restore stopped: the archive could not be read or its journal not saved.", &[])
            }
            Self::TargetChanged(path) => SourceWarning::new(
                "The restore stopped: {path} lost mail or was reset meanwhile. Run the restore test again.",
                &[("path", &shown_path(path))],
            ),
            Self::NotCreated(path) => SourceWarning::new(
                "The restore stopped: folder {path} could not be created or opened.",
                &[("path", &shown_path(path))],
            ),
            Self::Unreadable(path) => SourceWarning::new(
                "The restore stopped: the server refused to open folder {path}. Check the folder, then run the restore test again.",
                &[("path", &shown_path(path))],
            ),
            Self::Unconfirmed(path) => SourceWarning::new(
                "The restore stopped: the server did not confirm an upload to {path}. Run the restore test again; it shows whether the mail arrived.",
                &[("path", &shown_path(path))],
            ),
            Self::Differs(path) => SourceWarning::new(
                "The restore stopped: the server accepted an upload to {path}, but the mail there cannot be matched to the archive; it may have arrived changed. Check the folder before uploading it again.",
                &[("path", &shown_path(path))],
            ),
            Self::Ambiguous(path) => SourceWarning::new(
                "The restore stopped: {path} changed while mail was uploaded. Run the restore test again.",
                &[("path", &shown_path(path))],
            ),
        }
    }
}

/// Reads a spool file for an `APPEND` without blocking the async workers: each piece is read on
/// tokio's blocking pool (the workspace's tokio has no `fs` feature).
struct SpoolReader {
    /// `None` while a piece is being read, or after a blocking step panicked.
    file: Option<File>,
    reading: Option<tokio::task::JoinHandle<(File, io::Result<Vec<u8>>)>>,
    /// Bytes read but not yet taken (a later poll may offer a smaller buffer).
    left: Vec<u8>,
}

impl SpoolReader {
    fn new(file: File) -> Self {
        Self { file: Some(file), reading: None, left: Vec::new() }
    }

    fn hand_out(&mut self, buf: &mut tokio::io::ReadBuf<'_>) {
        let count = self.left.len().min(buf.remaining());
        buf.put_slice(&self.left[..count]);
        self.left.drain(..count);
    }
}

impl tokio::io::AsyncRead for SpoolReader {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        use std::future::Future;
        use std::task::Poll;
        let this = &mut *self;
        if !this.left.is_empty() {
            this.hand_out(buf);
            return Poll::Ready(Ok(()));
        }
        if this.reading.is_none() {
            let Some(mut file) = this.file.take() else {
                return Poll::Ready(Err(io::Error::other("restore spool is gone")));
            };
            let want = buf.remaining().clamp(1, 64 * 1024);
            this.reading = Some(tokio::task::spawn_blocking(move || {
                let mut piece = vec![0u8; want];
                let read = io::Read::read(&mut file, &mut piece).map(|count| {
                    piece.truncate(count);
                    piece
                });
                (file, read)
            }));
        }
        let Some(reading) = this.reading.as_mut() else {
            return Poll::Ready(Err(io::Error::other("restore spool is gone")));
        };
        match std::pin::Pin::new(reading).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(joined) => {
                this.reading = None;
                let (file, read) = joined.map_err(|_| io::Error::other("restore spool reader stopped"))?;
                this.file = Some(file);
                this.left = read?;
                this.hand_out(buf);
                Poll::Ready(Ok(()))
            }
        }
    }
}

/// How one upload ended when the server answered `OK` or `NO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Uploaded {
    /// An identical copy is in the folder now.
    Identical,
    /// The server stored it changed ([`RestoreJournal::altered`]); it counts as restored.
    Altered,
    /// The server refused it (`NO`); nothing was stored.
    Refused,
}

/// A real restore while it writes.
struct RestoreRun<'a> {
    session: &'a mut Session,
    maps: Vec<FolderMap>,
    /// The target as the reviewed plan saw it.
    planned: &'a TargetSnapshot,
    target: TargetSnapshot,
    sizes: HashMap<usize, HashSet<u64>>,
    /// `None` only while a blocking step holds it.
    archive: Option<RestoreArchive<File>>,
    journal: RestoreJournal,
    journal_path: PathBuf,
    spool_dir: PathBuf,
    retry: bool,
    /// A write command (`CREATE`, `APPEND`) was sent.
    wrote: bool,
    added: u64,
    skipped: u64,
    refused: u64,
    altered: u64,
    /// The archived messages (places in `Manifest::messages`) now confirmed in their own folder:
    /// present before, or uploaded. The aliases a target fills by itself count only for these.
    restored: HashSet<usize>,
    warnings: Vec<SourceWarning>,
}

impl RestoreRun<'_> {
    /// Creates and subscribes each missing folder; afterwards only the folder list and the new
    /// folder are read again.
    async fn create_folders(&mut self) -> Result<(), Stop> {
        for at in 0..self.maps.len() {
            if !self.maps[at].create {
                continue;
            }
            let path = self.maps[at].target.clone();
            self.wrote = true;
            if self.session.create_folder(&path, None).await.is_err() {
                return Err(Stop::NotCreated(path));
            }
            // Other mail programs show a folder only once it is subscribed; not fatal without it.
            if self.session.subscribe(&path).await.is_err() {
                self.warnings.push(SourceWarning::new(
                    "Folder {path} was created but could not be subscribed.",
                    &[("path", &shown_path(&path))],
                ));
            }
            let listed: HashMap<String, bool> = self
                .session
                .list()
                .await
                .map_err(Stop::Imap)?
                .into_iter()
                .map(|folder| (folder.path.clone(), folder.selectable()))
                .collect();
            let kept = self
                .planned
                .listed
                .iter()
                .all(|(old, selectable)| listed.get(old).is_some_and(|now| *now || !selectable));
            if !kept {
                return Err(Stop::TargetChanged(path));
            }
            if listed.get(&path) != Some(&true) {
                return Err(Stop::NotCreated(path));
            }
            self.target.listed = listed;
            let sizes = self.sizes.get(&at).cloned().unwrap_or_default();
            match read_target_folder(self.session, &path, &sizes, None).await.map_err(Stop::Imap)? {
                Some(folder) => {
                    self.target.folders.insert(path, folder);
                }
                None => return Err(Stop::NotCreated(path)),
            }
        }
        Ok(())
    }

    /// Every item in order ([`RestoreItems::items`]): skipped when present, appended when missing.
    async fn upload_all(&mut self, items: &[RestoreItem<'_>]) -> Result<(), Stop> {
        let mut used: HashMap<(usize, &str), u64> = HashMap::new();
        // The folder an upload was just confirmed in: it is up to date for the next one.
        let mut fresh: Option<usize> = None;
        for item in items {
            let at = item.at;
            let target = self.maps[at].target.clone();
            let counter = used.entry((at, item.sha256)).or_default();
            let mut placement = self.placement(item, counter);
            if matches!(placement, Placement::Add | Placement::Unconfirmed) && fresh != Some(at) {
                // Others may have changed the folder since the last look: new mail may already be
                // this message, a lost UID ends the run.
                self.refresh(at).await?;
                placement = self.placement(item, counter);
            }
            match placement {
                Placement::Present => {
                    if self.journal.pending.remove(&item.key).is_some() {
                        self.save_journal().await?;
                    }
                    self.skipped += 1;
                    self.note_restored(item);
                    continue;
                }
                Placement::Unconfirmed if self.retry => {}
                Placement::Add => {}
                Placement::Unconfirmed | Placement::Uncertain(_) => return Err(Stop::Ambiguous(target)),
            }
            fresh = None;
            match Box::pin(self.upload(item)).await? {
                Uploaded::Identical => {
                    *counter += 1;
                    fresh = Some(at);
                    self.note_restored(item);
                }
                Uploaded::Altered => self.note_restored(item),
                Uploaded::Refused => {}
            }
        }
        Ok(())
    }

    /// Notes that the archived message behind `item` is in its own folder now (an alias item puts
    /// it elsewhere, so it does not count).
    fn note_restored(&mut self, item: &RestoreItem<'_>) {
        if !item.alias {
            self.restored.insert(item.source);
        }
    }

    fn placement(&self, item: &RestoreItem<'_>, used: &mut u64) -> Placement {
        let map = &self.maps[item.at];
        place(item, map, self.target.folders.get(&map.target), Some(&self.journal), used)
    }

    /// Uploads `item` into the folder of its archive folder: spool, journal entry, `APPEND`,
    /// confirmation. [`Uploaded::Identical`] stands for the item in later counting.
    async fn upload(&mut self, item: &RestoreItem<'_>) -> Result<Uploaded, Stop> {
        let target = self.maps[item.at].target.clone();
        let spool = self.spool(item.source).await?;
        let folder = self.target.folders.get(&target).ok_or_else(|| Stop::TargetChanged(target.clone()))?;
        let before = folder.copies(item.sha256);
        let flags: Vec<String> = transferable_flags(item.flags).into_iter().filter(|flag| folder.keeps(flag)).collect();
        self.journal.pending.insert(item.key.clone(), before);
        self.save_journal().await?;
        self.wrote = true;
        let mut input = SpoolReader::new(spool);
        let appended = self.session.append_backup(&target, &mut input, item.bytes, &flags, item.internal_date).await;
        let appended = match appended {
            Ok(appended) => appended,
            Err(AppendError::Refused(_)) => {
                // Certainly not stored and the session goes on: a later run may try it again.
                self.journal.pending.remove(&item.key);
                self.save_journal().await?;
                self.refused += 1;
                return Ok(Uploaded::Refused);
            }
            Err(AppendError::NotSent(_)) => {
                self.journal.pending.remove(&item.key);
                self.save_journal().await?;
                return Err(Stop::NotSent);
            }
            // The journal keeps the entry: the next test run tells whether it arrived.
            Err(AppendError::Unconfirmed(_)) => return Err(Stop::Unconfirmed(target)),
        };
        Box::pin(self.confirm(item, before, appended)).await
    }

    /// Reads the folder again after an `APPEND` and decides what the server stored.
    async fn confirm(
        &mut self,
        item: &RestoreItem<'_>,
        before: u64,
        appended: Option<AppendUid>,
    ) -> Result<Uploaded, Stop> {
        let target = self.maps[item.at].target.clone();
        if let Err(stop) = self.refresh(item.at).await {
            // Whatever the cause, the upload stays open in the journal.
            return Err(match stop {
                Stop::Imap(_) | Stop::NotSent | Stop::Unreadable(_) => Stop::Unconfirmed(target),
                other => other,
            });
        }
        let folder = self.target.folders.get(&target).ok_or_else(|| Stop::TargetChanged(target.clone()))?;
        let identical = match appended {
            Some(landed) => {
                if landed.uid_validity != folder.uid_validity {
                    return Err(Stop::TargetChanged(target));
                }
                match folder.messages.get(&landed.uid) {
                    None => return Err(Stop::Unconfirmed(target)),
                    Some(copy) if copy.sha256.as_deref() == Some(item.sha256) => true,
                    Some(_) => {
                        // Stored changed (some servers rewrite headers): it counts as restored, so
                        // no later run uploads a second copy.
                        let landed = Landed { uid_validity: landed.uid_validity, uid: landed.uid };
                        self.journal.altered.insert(item.key.clone(), landed);
                        false
                    }
                }
            }
            None => match folder.copies(item.sha256) {
                now if now == before + 1 => true,
                now if now == before => return Err(Stop::Differs(target)),
                _ => return Err(Stop::Ambiguous(target)),
            },
        };
        self.journal.pending.remove(&item.key);
        self.save_journal().await?;
        self.added += 1;
        if identical {
            Ok(Uploaded::Identical)
        } else {
            self.altered += 1;
            Ok(Uploaded::Altered)
        }
    }

    /// Brings the target folder of archive folder `at` up to date: new UIDs are read (and hashed
    /// when their size fits), UIDs gone are dropped. Stops when its UIDVALIDITY changed or a UID
    /// the plan counted on is gone.
    async fn refresh(&mut self, at: usize) -> Result<(), Stop> {
        let path = self.maps[at].target.clone();
        let sizes = self.sizes.get(&at).cloned().unwrap_or_default();
        let selected = match self.session.examine(&path).await {
            Ok(selected) => selected,
            Err(ImapError::NotFound(_)) => return Err(Stop::TargetChanged(path)),
            // `NO` (or `BAD`) keeps the connection: the folder, not the line, is the problem.
            Err(ImapError::Protocol(_)) => return Err(Stop::Unreadable(path)),
            Err(error) => return Err(Stop::Imap(error)),
        };
        let planned_target = self.planned;
        let planned = planned_target.folders.get(&path);
        let Some(folder) = self.target.folders.get_mut(&path) else { return Err(Stop::TargetChanged(path)) };
        if selected.uid_validity != folder.uid_validity {
            return Err(Stop::TargetChanged(path));
        }
        folder.permanent_flags = selected.permanent_flags;
        let highest = folder.messages.keys().next_back().copied().unwrap_or(0);
        let mut new: Vec<u32> = self
            .session
            .uid_search(&Search::UidsFrom(highest.saturating_add(1)))
            .await
            .map_err(Stop::Imap)?
            .into_iter()
            .filter(|uid| *uid > highest)
            .collect();
        if selected.exists as usize != folder.messages.len() + new.len() {
            // Something left (or the count is off): find out what, and stop only when the plan
            // counted on it.
            let now: HashSet<u32> =
                self.session.uid_search(&Search::All).await.map_err(Stop::Imap)?.into_iter().collect();
            let gone: Vec<u32> = folder.messages.keys().filter(|uid| !now.contains(uid)).copied().collect();
            if planned.is_some_and(|planned| gone.iter().any(|uid| planned.messages.contains_key(uid))) {
                return Err(Stop::TargetChanged(path));
            }
            for uid in gone {
                folder.remove(uid);
            }
            new = now.into_iter().filter(|uid| !folder.messages.contains_key(uid)).collect();
            new.sort_unstable();
        }
        add_target_messages(self.session, folder, &new, &sizes).await.map_err(Stop::Imap)
    }

    /// Archived message `index` in a private spool file next to the journal, checked against its
    /// hash while it is copied, on the blocking pool.
    async fn spool(&mut self, index: usize) -> Result<File, Stop> {
        let mut archive = self.archive.take().ok_or(Stop::Local(BackupRunError::Archive(INTERRUPTED)))?;
        let dir = self.spool_dir.clone();
        let (archive, spool) = tokio::task::spawn_blocking(move || {
            let spool = (|| -> Result<File, BackupRunError> {
                let mut spool = tempfile::tempfile_in(dir)?;
                archive.copy_message(index, &mut spool)?;
                spool.seek(SeekFrom::Start(0))?;
                Ok(spool)
            })();
            (archive, spool)
        })
        .await
        .map_err(|_| Stop::Local(BackupRunError::Io(io::Error::other("restore worker stopped"))))?;
        self.archive = Some(archive);
        spool.map_err(Stop::Local)
    }

    /// Saves the journal on the blocking pool.
    async fn save_journal(&self) -> Result<(), Stop> {
        let (journal, path) = (self.journal.clone(), self.journal_path.clone());
        blocking(move || journal.save(&path)).await.map_err(Stop::Local)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::{HashMap, HashSet};
    use std::fs;

    use std::io::Write;
    use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use chrono::{NaiveDate, Utc};
    use sha2::{Digest, Sha256};
    use torro_ipc::mail::{BackupJob, BackupPhase, BackupScope};
    use torro_mail::Role;
    use torro_mail_backup::format::{Manifest, Source, Status};
    use torro_mail_backup::restore::RestoreArchive;

    use torro_ipc::mail::{RestorePlan, RestoreResult};

    use super::{
        ALL_PLANS, ApplyRequest, BackupFileError, BackupJobs, BackupProgress, BackupRunError, BackupSelection,
        CANCELLED, CLIENT_PLANS, COMMITTING, DISK_RESERVE, DiskSpace, JobEntry, PendingBackup, RUNNING, RestoreChoice,
        RestoreJournal, RestorePlans, RestoreProblem, SourceProblem, SourceWarning, SpaceShortage, capped_warnings,
        run_local_backup, run_server_backup, uid_inventory_hash, write_verified_backup,
    };
    use crate::account::github::tests::MemoryTokens;
    use crate::mail::MailDirs;
    use crate::mail::service::MailService;
    use crate::mail::service::tests::{PASSWORD, eventually, mail_account, service_in, store_accounts};
    use crate::mail::store::{LocalOriginal, MailStore, NewMessage};
    use crate::mail::sync::fake::{AppendQuirk, Change, FakeImap};

    #[test]
    fn every_source_problem_has_its_own_text_for_the_person() {
        let problems = [
            SourceProblem::Vanished,
            SourceProblem::BodyMismatch,
            SourceProblem::IdentifiersChanged,
            SourceProblem::InventoryChanged,
            SourceProblem::LocalDamaged,
            SourceProblem::LocalChanged,
        ];
        let texts: HashSet<String> =
            problems.iter().map(|problem| BackupRunError::Source(*problem).user_message()).collect();
        assert_eq!(texts.len(), problems.len());
        assert_eq!(
            BackupRunError::from(torro_mail_imap::imap::BodyError::Oversized).user_message(),
            SourceProblem::BodyMismatch.user_message()
        );
    }

    #[test]
    fn warnings_stay_within_the_manifest_limits() {
        let long = "x".repeat(2000);
        let warning = SourceWarning::new("Folder {path} is odd.", &[("path", &long)]);
        let text = warning.archive_text();
        assert_eq!(text.len(), torro_mail_backup::format::MAX_TEXT_BYTES);
        assert!(text.ends_with('…'));
        let many = vec![warning; torro_mail_backup::format::MAX_WARNINGS + 5];
        let capped = capped_warnings(many);
        assert_eq!(capped.len(), torro_mail_backup::format::MAX_WARNINGS);
        assert_eq!(
            capped.last().map(SourceWarning::archive_text).as_deref(),
            Some("6 further warnings were left out.")
        );
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
        // The last guard, should a file appear under the name after the job's first check.
        assert!(matches!(
            write_verified_backup(&dirs, "mail-2026.zip", |file| {
                file.write_all(SAMPLE_ZIP)?;
                Ok(())
            }),
            Err(BackupFileError::Exists(name)) if name == "mail-2026.zip"
        ));
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
            ..BackupJobs::default()
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

    /// Anna's account on `server`, its workers paused: every line the server sees comes from the
    /// code under test.
    async fn server_service(root: &std::path::Path, server: &FakeImap) -> (Arc<MailService>, String) {
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(root, &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root, &secrets);
        mail.pause_from_start(true);
        mail.reload().await;
        (Arc::new(mail), account.id)
    }

    async fn server_backup(
        mail: &Arc<MailService>,
        root: &std::path::Path,
        account: &str,
        since: Option<NaiveDate>,
        name: &str,
    ) -> Result<torro_mail_backup::VerifiedArchive, BackupRunError> {
        let jobs = BackupJobs::default();
        let progress = BackupProgress { jobs: &jobs, id: name, state: Arc::new(AtomicU8::new(RUNNING)) };
        run_server_backup(Arc::clone(mail), dirs(root), account, since, name, None, progress).await
    }

    /// The backup folder holds exactly `names`: no partial archive and no spool file is left.
    fn backup_folder_holds(root: &std::path::Path, names: &[&str]) {
        let mut found: Vec<String> = fs::read_dir(root.join("data/mail/backups"))
            .map(|entries| {
                entries.map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned()).collect()
            })
            .unwrap_or_default();
        found.sort();
        assert_eq!(found, names, "nothing else in the backup folder");
    }

    /// The commands a backup may send: signing in, listing and reading. Never `SELECT`, `STORE`,
    /// `APPEND`, `CREATE`, `EXPUNGE`, `COPY` or `MOVE`.
    const READ_ONLY_COMMANDS: [&str; 15] = [
        "CAPABILITY",
        "LOGIN",
        "AUTHENTICATE",
        "ID",
        "ENABLE",
        "LIST",
        "LSUB",
        "XLIST",
        "NAMESPACE",
        "EXAMINE",
        "STATUS",
        "UID SEARCH",
        "UID FETCH",
        "NOOP",
        "LOGOUT",
    ];

    fn commands(server: &FakeImap) -> Vec<String> {
        server
            .lines()
            .iter()
            .map(|line| {
                let words: Vec<String> = line.split(' ').skip(1).take(2).map(str::to_ascii_uppercase).collect();
                match words.as_slice() {
                    [uid, command] if uid == "UID" => format!("UID {command}"),
                    [command, ..] => command.clone(),
                    [] => String::new(),
                }
            })
            .collect()
    }

    fn assert_read_only(server: &FakeImap) {
        for command in commands(server) {
            assert!(READ_ONLY_COMMANDS.contains(&command.as_str()), "a backup sent {command}");
        }
    }

    #[tokio::test]
    async fn server_backup_reads_every_folder_on_its_own_connection_and_aliases_all_mail() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let first = "From: a@example.org\r\nSubject: Grüße\r\n\r\nErster Text\r\n";
        let large = format!("From: b@example.org\r\nSubject: Large\r\n\r\n{}\r\n", "ü".repeat(75_000));
        let archived_only = "From: c@example.org\r\n\r\nOnly in All Mail\r\n";
        server.add("INBOX", first, &["\\Seen", "$Work"]);
        server.add("Archive", &large, &["\\Flagged"]);
        server.add_folder("All", "\\HasNoChildren \\All", 15);
        server.add("All", first, &["\\Seen"]);
        server.add("All", archived_only, &[]);
        let (mail, account) = server_service(root.path(), &server).await;
        // Sync, reading and sending keep their connection: the backup does not wait for it.
        let held = mail.command(&account).await.expect("the command connection");
        let report = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            server_backup(&mail, root.path(), &account, None, "all.zip"),
        )
        .await
        .expect("the backup does not need the command connection")
        .expect("complete server archive");
        held.release();
        assert!(report.complete);
        assert_eq!((report.folders, report.messages, report.aliases), (5, 3, 1));
        assert_eq!(report.bytes, (first.len() + large.len() + archived_only.len()) as u64, "All Mail stored once");

        let manifest = manifest_of(root.path(), "all.zip");
        let all = manifest.folders.iter().find(|folder| folder.path == "All").expect("All Mail");
        assert_eq!(all.role.as_deref(), Some("all"));
        assert_eq!(manifest.folders.last().map(|folder| folder.path.as_str()), Some("All"), "collections last");
        assert_eq!(all.inventory.as_ref().map(|inventory| inventory.eligible_messages), Some(2));
        let inbox_message = manifest.messages.iter().find(|message| message.folder_id == manifest.folders[0].id);
        assert_eq!(manifest.aliases[0].same_as, inbox_message.expect("inbox mail").path);
        assert_eq!(manifest.aliases[0].flags, ["\\Seen"], "the alias keeps its own flags");
        assert_eq!(manifest.folders[0].role.as_deref(), Some("inbox"));

        assert_read_only(&server);
        let lines = server.lines();
        assert!(lines.iter().any(|line| line.contains("BODY.PEEK[]<65536.65536>")), "large mail in pieces");
        assert!(lines.iter().any(|line| line.ends_with(&format!("BODY.PEEK[]<{}.1>)", large.len()))), "end check");
        assert!(lines.iter().any(|line| line.ends_with(" LOGOUT")), "the own session signs out");
        backup_folder_holds(root.path(), &["all.zip"]);
    }

    #[tokio::test]
    async fn server_backup_aliases_known_gmail_messages_without_downloading_them() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.act_as_gmail();
        server.add_folder("[Gmail]/All Mail", "\\HasNoChildren \\All", 15);
        server.add_folder("[Gmail]/Starred", "\\HasNoChildren \\Flagged", 16);
        let shared = "From: a@example.org\r\n\r\nIn the inbox and in All Mail\r\n";
        let only = "From: b@example.org\r\n\r\nArchived\r\n";
        let inbox = server.add("INBOX", shared, &[]);
        server.set_gm_msgid("INBOX", inbox, 7001);
        let all = server.add("[Gmail]/All Mail", shared, &[]);
        server.set_gm_msgid("[Gmail]/All Mail", all, 7001);
        let all_only = server.add("[Gmail]/All Mail", only, &[]);
        server.set_gm_msgid("[Gmail]/All Mail", all_only, 7002);
        let starred = server.add("[Gmail]/Starred", shared, &["\\Flagged"]);
        server.set_gm_msgid("[Gmail]/Starred", starred, 7001);
        let (mail, account) = server_service(root.path(), &server).await;
        let report = server_backup(&mail, root.path(), &account, None, "gmail.zip").await.expect("archive");
        assert!(report.complete);
        assert_eq!((report.messages, report.aliases), (2, 2));
        let downloads = server.lines().iter().filter(|line| line.contains("BODY.PEEK[]<0.")).count();
        assert_eq!(downloads, 2, "only the inbox mail and the one only in All Mail are fetched");
        let manifest = manifest_of(root.path(), "gmail.zip");
        let starred = manifest.folders.iter().find(|folder| folder.path == "[Gmail]/Starred").expect("starred");
        assert_eq!(starred.role.as_deref(), Some("flagged"));
        assert_read_only(&server);
    }

    #[tokio::test]
    async fn a_real_important_folder_off_gmail_keeps_its_own_mail_and_restores_it() {
        // RFC 8457 lets any server mark a real folder \Important; only Gmail's is a collection.
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add_folder("Important", "\\HasNoChildren \\Important", 15);
        let shared = "From: a@example.org\r\n\r\nIn the inbox and in Important\r\n";
        let only = "From: b@example.org\r\n\r\nOnly in Important\r\n";
        server.add("INBOX", shared, &[]);
        server.add("Important", shared, &[]);
        server.add("Important", only, &[]);
        let (mail, account) = server_service(root.path(), &server).await;
        let report = server_backup(&mail, root.path(), &account, None, "real.zip").await.expect("archive");
        assert!(report.complete);
        assert_eq!((report.messages, report.aliases), (3, 0), "each mail of the real folder has its own EML");
        let manifest = manifest_of(root.path(), "real.zip");
        let important = manifest.folders.iter().find(|folder| folder.path == "Important").expect("important");
        assert_eq!(important.role, None, "no collection role off Gmail");

        let zip = fs::read(root.path().join("data/mail/backups/real.zip")).expect("archive file");
        let target = FakeImap::start(PASSWORD).await;
        target.add_folder("Important", "\\HasNoChildren \\Important", 15);
        let restore_root = tempfile::tempdir().expect("tempdir");
        let (mail, account) = server_service(restore_root.path(), &target).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, restore_root.path(), &zip, "real.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (3, 0, 0, 0));
        let result =
            apply_of(&plans, &mail, restore_root.path(), "real.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (3, 0, 0));
        assert_eq!(target.messages("Important").len(), 2, "the real folder gets its mail back");
    }

    #[tokio::test]
    async fn on_gmail_all_mail_is_read_before_starred_and_important_whatever_the_list_order() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.act_as_gmail();
        // Listed before "All Mail", as a Gmail in another language may sort them.
        server.add_folder("[Gmail]/Important", "\\HasNoChildren \\Important", 15);
        server.add_folder("[Gmail]/Starred", "\\HasNoChildren \\Flagged", 16);
        server.add_folder("[Gmail]/All Mail", "\\HasNoChildren \\All", 17);
        let archived = "From: c@example.org\r\n\r\nArchived, starred and important\r\n";
        for folder in ["[Gmail]/Important", "[Gmail]/Starred", "[Gmail]/All Mail"] {
            let uid = server.add(folder, archived, &["\\Flagged"]);
            server.set_gm_msgid(folder, uid, 8001);
        }
        let (mail, account) = server_service(root.path(), &server).await;
        let report = server_backup(&mail, root.path(), &account, None, "labels.zip").await.expect("archive");
        assert!(report.complete);
        assert_eq!((report.messages, report.aliases), (1, 2));
        let downloads = server.lines().iter().filter(|line| line.contains("BODY.PEEK[]<0.")).count();
        assert_eq!(downloads, 1, "the labels know the mail of All Mail by its Gmail id");
        let manifest = manifest_of(root.path(), "labels.zip");
        let folder_of = |id: &str| manifest.folders.iter().find(|folder| folder.id == id).map(|f| f.path.clone());
        assert_eq!(folder_of(&manifest.messages[0].folder_id).as_deref(), Some("[Gmail]/All Mail"));
        let important = manifest.folders.iter().find(|folder| folder.path == "[Gmail]/Important").expect("important");
        assert_eq!(important.role.as_deref(), Some("important"));
    }

    #[tokio::test]
    async fn mail_that_arrives_just_before_all_mail_is_read_is_archived_once_and_restored() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add_folder("All", "\\HasNoChildren \\All", 15);
        let first = "From: a@example.org\r\n\r\nIn the inbox\r\n";
        let late = "From: d@example.org\r\n\r\nArrived while the collection was read\r\n";
        server.add("INBOX", first, &[]);
        server.add("All", first, &[]);
        // After the inbox and its own final inventory, right before All Mail is read, the mail
        // lands in both: All Mail sees it first and archives it as its own.
        server.on_examine("All", 1, Change::AddTo(vec!["INBOX".into(), "All".into()], late.into()));
        let (mail, account) = server_service(root.path(), &server).await;
        let report = server_backup(&mail, root.path(), &account, None, "arrival.zip").await.expect("archive");
        assert!(report.complete);
        assert_eq!((report.messages, report.aliases), (2, 2), "{report:?}");
        assert_eq!(report.bytes, (first.len() + late.len()) as u64, "no bytes archived twice");
        let manifest = manifest_of(root.path(), "arrival.zip");
        let inbox = manifest.folders.iter().find(|folder| folder.path == "INBOX").expect("inbox");
        let all = manifest.folders.iter().find(|folder| folder.path == "All").expect("all");
        let late_eml = manifest.messages.iter().find(|message| message.folder_id == all.id).expect("late EML");
        let inbox_alias = manifest.aliases.iter().find(|alias| alias.folder_id == inbox.id).expect("alias in inbox");
        assert_eq!(inbox_alias.same_as, late_eml.path);
        assert_eq!(inbox.inventory.as_ref().map(|inventory| inventory.eligible_messages), Some(2));

        // The alias in the inbox goes up with the bytes of the EML it names; All Mail then shows it.
        let zip = fs::read(root.path().join("data/mail/backups/arrival.zip")).expect("archive file");
        let target = FakeImap::start(PASSWORD).await;
        target.add_folder("All", "\\HasNoChildren \\All", 15);
        target.mirror_appends_into("All");
        let restore_root = tempfile::tempdir().expect("tempdir");
        let (mail, account) = server_service(restore_root.path(), &target).await;
        let plans = RestorePlans::default();
        let plan =
            plan_of(&plans, &mail, restore_root.path(), &zip, "arrival.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (3, 0, 0, 0));
        let result =
            apply_of(&plans, &mail, restore_root.path(), "arrival.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (2, 2, 0), "{:?}", result.warnings);
        assert_eq!(appends(&target), 2);
        assert_eq!(target.messages("INBOX").len(), 2);
        assert_eq!(target.messages("All").len(), 2);
    }

    #[tokio::test]
    async fn server_backup_rejects_a_body_longer_than_its_announced_size() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let raw = "From: a@example.org\r\n\r\nThe end is cut off by the size\r\n";
        let uid = server.add("INBOX", raw, &[]);
        server.misreport_size("INBOX", uid, raw.len() - 5);
        let (mail, account) = server_service(root.path(), &server).await;
        let outcome = server_backup(&mail, root.path(), &account, None, "short.zip").await;
        assert!(matches!(outcome, Err(BackupRunError::Source(SourceProblem::BodyMismatch))), "{outcome:?}");
        backup_folder_holds(root.path(), &[]);
        assert!(server.lines().iter().any(|line| line.ends_with(" LOGOUT")), "signed out after the failure");
    }

    #[tokio::test]
    async fn server_backup_takes_mail_that_arrives_during_the_final_inventory() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", "From: a@example.org\r\n\r\nFirst\r\n", &[]);
        server.on_examine("INBOX", 2, Change::Add("From: b@example.org\r\n\r\nLate\r\n".into()));
        let (mail, account) = server_service(root.path(), &server).await;
        let report = server_backup(&mail, root.path(), &account, None, "late.zip").await.expect("complete");
        assert!(report.complete);
        assert_eq!(report.messages, 2, "the late mail is in the archive");
        let manifest = manifest_of(root.path(), "late.zip");
        let inbox = manifest.folders.iter().find(|folder| folder.path == "INBOX").expect("inbox");
        assert_eq!(inbox.inventory.as_ref().map(|inventory| inventory.eligible_messages), Some(2));
    }

    #[tokio::test]
    async fn server_backup_is_incomplete_when_a_message_vanishes_or_uids_are_renumbered() {
        for (change, expected) in
            [(Change::Expunge(1), SourceProblem::Vanished), (Change::Renumber, SourceProblem::IdentifiersChanged)]
        {
            let root = tempfile::tempdir().expect("tempdir");
            let server = FakeImap::start(PASSWORD).await;
            server.add("INBOX", "From: a@example.org\r\n\r\nFirst\r\n", &[]);
            server.on_examine("INBOX", 2, change);
            let (mail, account) = server_service(root.path(), &server).await;
            let outcome = server_backup(&mail, root.path(), &account, None, "changed.zip").await;
            assert!(matches!(&outcome, Err(BackupRunError::Source(problem)) if *problem == expected), "{outcome:?}");
            backup_folder_holds(root.path(), &[]);
        }
    }

    #[tokio::test]
    async fn server_backup_filters_by_the_day_of_internal_date_in_its_own_zone() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        // Just after midnight in +02:00, the evening before in UTC: on the limit's day.
        let kept = date("2026-03-01T00:30:00+02:00");
        server.add_received("INBOX", b"From: a@example.org\r\n\r\nKept\r\n", &[], kept);
        // The evening before in -05:00, already the limit's day in UTC: before it.
        server.add_received("INBOX", b"From: b@example.org\r\n\r\nOlder\r\n", &[], date("2026-02-28T22:00:00-05:00"));
        let (mail, account) = server_service(root.path(), &server).await;
        let since = NaiveDate::from_ymd_opt(2026, 3, 1);
        let report = server_backup(&mail, root.path(), &account, since, "since.zip").await.expect("archive");
        assert_eq!(report.messages, 1);
        let manifest = manifest_of(root.path(), "since.zip");
        assert_eq!(manifest.messages[0].internal_date, Some(kept), "INTERNALDATE with its zone");
        let inbox = manifest.folders.iter().find(|folder| folder.path == "INBOX").expect("inbox");
        assert_eq!(inbox.inventory.as_ref().map(|inventory| inventory.eligible_messages), Some(1));
        let fetched = server.lines().iter().filter(|line| line.contains("BODY.PEEK[]<0.")).count();
        assert_eq!(fetched, 1, "the older body is never fetched");
    }

    #[tokio::test]
    async fn server_backup_reports_nonselectable_folders_in_the_archive() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add_folder("Container", "\\Noselect \\HasChildren", 15);
        let (mail, account) = server_service(root.path(), &server).await;
        let report = server_backup(&mail, root.path(), &account, None, "containers.zip").await.expect("backup");
        assert!(report.complete);
        let expected = torro_core::i18n::trf(
            "The IMAP server marks folder {path} as {reason}; its messages cannot be inventoried.",
            &[("path", &"Container"), ("reason", &"\\Noselect")],
        );
        assert_eq!(report.warnings, vec![expected]);
        let manifest = manifest_of(root.path(), "containers.zip");
        assert_eq!(
            manifest.warnings,
            vec!["The IMAP server marks folder Container as \\Noselect; its messages cannot be inventoried."],
            "English in the archive"
        );
    }

    #[tokio::test]
    async fn server_backup_catches_an_arrival_during_fetch_and_rejects_missing_body() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", "From: a@example.org\r\n\r\nFirst\r\n", &[]);
        server.add_on_backup_chunk("INBOX", "From: b@example.org\r\n\r\nLater\r\n");
        let (mail, account) = server_service(root.path(), &server).await;
        let report = server_backup(&mail, root.path(), &account, None, "arrival.zip").await.expect("caught up");
        assert_eq!(report.messages, 2);

        let uid = server.add("Archive", "From: c@example.org\r\n\r\nGone\r\n", &[]);
        server.withhold_text("Archive", uid);
        let outcome = server_backup(&mail, root.path(), &account, None, "missing.zip").await;
        assert!(matches!(outcome, Err(BackupRunError::Source(SourceProblem::Vanished))), "{outcome:?}");
        backup_folder_holds(root.path(), &["arrival.zip"]);
    }

    #[tokio::test]
    async fn server_backup_rejects_a_folder_that_becomes_selectable_mid_run() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", "From: a@example.org\r\n\r\nFirst\r\n", &[]);
        server.make_selectable_on_backup_chunk("Archive");
        let (mail, account) = server_service(root.path(), &server).await;
        let outcome = server_backup(&mail, root.path(), &account, None, "changed.zip").await;
        assert!(matches!(outcome, Err(BackupRunError::Source(SourceProblem::InventoryChanged))), "{outcome:?}");
        backup_folder_holds(root.path(), &[]);
    }

    #[tokio::test]
    async fn a_cancelled_job_is_cancelling_until_its_task_stops_and_leaves_no_files() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", "From: a@example.org\r\n\r\nFirst\r\n", &[]);
        let (mail, account) = server_service(root.path(), &server).await;
        server.hold_backup_chunks(true);
        let jobs = BackupJobs::default();
        let selection = |name: &str| BackupSelection {
            source: BackupScope::Server,
            since: None,
            filename: Some(name.into()),
            target_dir: None,
        };
        let job = jobs
            .start(Arc::clone(&mail), dirs(root.path()), account.clone(), selection("first.zip"), None)
            .await
            .expect("started");
        eventually("the job waits for a body", || async {
            server.lines().iter().any(|line| line.contains("BODY.PEEK[]<"))
        })
        .await;
        let cancelling = jobs.cancel(&account, &job.id, None).expect("the job");
        assert_eq!(cancelling.phase, BackupPhase::Cancelling);
        assert!(
            matches!(
                jobs.start(Arc::clone(&mail), dirs(root.path()), account.clone(), selection("second.zip"), None).await,
                Err(BackupRunError::Busy)
            ),
            "the account stays busy while the task still runs"
        );
        server.hold_backup_chunks(false);
        eventually("the task stops", || async {
            jobs.status(&account, &job.id, None).is_some_and(|job| job.phase.is_final())
        })
        .await;
        assert_eq!(jobs.status(&account, &job.id, None).expect("the job").phase, BackupPhase::Cancelled);
        backup_folder_holds(root.path(), &[]);
        assert!(server.lines().iter().any(|line| line.ends_with(" LOGOUT")), "signed out after the cancellation");
        let second = jobs
            .start(Arc::clone(&mail), dirs(root.path()), account.clone(), selection("second.zip"), None)
            .await
            .expect("a new job after the old one stopped");
        eventually("the second job ends", || async {
            jobs.status(&account, &second.id, None).is_some_and(|job| job.phase.is_final())
        })
        .await;
        assert_eq!(jobs.status(&account, &second.id, None).expect("the job").phase, BackupPhase::Complete);
    }

    #[tokio::test]
    async fn a_job_whose_task_panicked_ends_incomplete_with_a_warning() {
        let state = Arc::new(AtomicU8::new(RUNNING));
        let job = BackupJob {
            id: "job".into(),
            account: "mail:anna@example.org".into(),
            source: BackupScope::Server,
            phase: BackupPhase::Reading,
            folders: 0,
            messages: 0,
            bytes: 0,
            archive: None,
            warnings: Vec::new(),
        };
        let jobs = BackupJobs {
            entries: Arc::new(Mutex::new(HashMap::from([(
                job.id.clone(),
                JobEntry { job, state: Arc::clone(&state), owner: None },
            )]))),
            ..BackupJobs::default()
        };
        let task: tokio::task::JoinHandle<()> = tokio::spawn(async { std::panic::panic_any("a task that panicked") });
        let panicked = task.await.expect_err("the task panicked");
        jobs.finish("job", &state, "x.zip".into(), Err(panicked));
        let job = jobs.status("mail:anna@example.org", "job", None).expect("the job");
        assert_eq!(job.phase, BackupPhase::Incomplete);
        assert_eq!(job.warnings, vec![torro_core::i18n::tr("Mail backup could not be completed.").to_string()]);
    }

    /// A port that takes connections and only counts them: a local backup must never open one.
    async fn connection_trap() -> (torro_mail::Endpoint, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let port = listener.local_addr().expect("address").port();
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                seen.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        (torro_mail::Endpoint { host: "127.0.0.1".into(), port, security: torro_mail::Security::Tls }, count)
    }

    /// Anna's account on the trap, with its password, but without workers: whatever connects to
    /// the trap was the code under test.
    async fn local_service(root: &std::path::Path) -> (Arc<MailService>, String, Arc<AtomicUsize>) {
        let (endpoint, connections) = connection_trap().await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &endpoint);
        store_accounts(root, &secrets, &[(&account, PASSWORD)]);
        let mail = service_in(root, &secrets);
        mail.pause_from_start(true);
        mail.reload().await;
        (Arc::new(mail), account.id, connections)
    }

    fn header(uid: u32) -> NewMessage {
        NewMessage {
            id: None,
            uid,
            rfc_message_id: Some(format!("<{uid}@example.org>")),
            thread_id: format!("t{uid}"),
            from: None,
            to: Vec::new(),
            cc: Vec::new(),
            reply_to: Vec::new(),
            subject: format!("Mail {uid}"),
            date: Utc::now(),
            snippet: String::new(),
            flags: Vec::new(),
            has_attachments: false,
            traits: torro_mail::MessageTraits::default(),
            invitation: false,
        }
    }

    /// A folder at `path` the sync listed with `delimiter` and `role`, under UIDVALIDITY `validity`.
    fn listed(db: &MailStore, path: &str, name: &str, role: Option<Role>, validity: u32) -> String {
        let id = db.upsert_folder(path, name, role).expect("folder");
        db.set_folder_delimiter(path, Some('/')).expect("delimiter");
        db.note_uidvalidity(&id, validity).expect("uidvalidity");
        id
    }

    fn date(text: &str) -> chrono::DateTime<chrono::FixedOffset> {
        chrono::DateTime::parse_from_rfc3339(text).expect("a date")
    }

    async fn local_backup(
        mail: &Arc<MailService>,
        root: &std::path::Path,
        account: &str,
        since: Option<NaiveDate>,
        name: &str,
    ) -> Result<torro_mail_backup::VerifiedArchive, BackupRunError> {
        let jobs = BackupJobs::default();
        let progress = BackupProgress { jobs: &jobs, id: name, state: Arc::new(AtomicU8::new(RUNNING)) };
        run_local_backup(Arc::clone(mail), dirs(root), account, since, name, None, progress).await
    }

    fn manifest_of(root: &std::path::Path, name: &str) -> Manifest {
        let file = fs::File::open(root.join("data/mail/backups").join(name)).expect("published archive");
        RestoreArchive::open(file).expect("a valid archive").manifest().clone()
    }

    #[tokio::test]
    async fn local_backup_never_connects_and_keeps_date_folder_and_flags_of_each_original() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mail, account, connections) = local_service(root.path()).await;
        let large = format!("From: a@example.org\r\n\r\n{}\r\n", "x".repeat(140_000));
        let small = "From: b@example.org\r\nSubject: Gr\u{fc}\u{df}e\r\n\r\nText\r\n";
        let mut spool = tempfile::tempfile().expect("spool");
        spool.write_all(large.as_bytes()).expect("write original");
        let store = mail.store(&account).await.expect("local store");
        store
            .run(move |db| {
                let inbox = listed(db, "INBOX", "INBOX", Some(Role::Inbox), 1);
                let greetings = listed(db, "INBOX/Gr&APw-&AN8-e", "Grüße", None, 3);
                for (folder, validity, uid) in [(&inbox, 1, 5), (&greetings, 3, 7), (&greetings, 3, 8)] {
                    db.upsert_message(folder, validity, &header(uid))?;
                }
                // Received just after midnight in its own zone (the evening before in UTC): on
                // the day of the limit, as the server backup counts it.
                let first = date("2026-03-01T00:30:00+02:00");
                db.save_original_file("INBOX", 1, 5, first, &["\\Seen".into(), "$Work".into()], &mut spool)?;
                db.save_original(
                    "INBOX/Gr&APw-&AN8-e",
                    3,
                    7,
                    date("2026-03-02T09:00:00+00:00"),
                    &[],
                    small.as_bytes(),
                )?;
                db.save_original("INBOX/Gr&APw-&AN8-e", 3, 8, date("2026-02-28T23:00:00+00:00"), &[], b"old")
            })
            .await
            .expect("originals retained");

        let since = NaiveDate::from_ymd_opt(2026, 3, 1);
        let report = local_backup(&mail, root.path(), &account, since, "local.zip").await.expect("local archive");
        assert!(report.complete);
        assert_eq!((report.messages, report.bytes), (2, (large.len() + small.len()) as u64));
        assert_eq!(connections.load(Ordering::SeqCst), 0, "a local backup opens no connection");

        let manifest = manifest_of(root.path(), "local.zip");
        assert_eq!(manifest.source, Source::Local);
        let names: Vec<(&str, &str, Option<&str>, Option<&str>)> = manifest
            .folders
            .iter()
            .map(|folder| {
                (folder.path.as_str(), folder.name.as_str(), folder.delimiter.as_deref(), folder.role.as_deref())
            })
            .collect();
        assert_eq!(
            names,
            vec![("INBOX", "INBOX", Some("/"), Some("inbox")), ("INBOX/Gr&APw-&AN8-e", "Grüße", Some("/"), None)]
        );
        let dates: Vec<_> = manifest.messages.iter().map(|message| message.internal_date).collect();
        assert_eq!(dates, vec![Some(date("2026-03-01T00:30:00+02:00")), Some(date("2026-03-02T09:00:00+00:00"))]);
        assert_eq!(manifest.messages[0].flags, vec!["\\Seen", "$Work"]);
        assert_eq!(manifest.messages[1].folder_id, manifest.folders[1].id);

        // The trap does catch a connection: asking for the command connection reaches it.
        assert!(mail.command(&account).await.is_err(), "the trap speaks no IMAP");
        eventually("the trap sees the attempt", || async { connections.load(Ordering::SeqCst) > 0 }).await;
    }

    #[tokio::test]
    async fn local_backup_with_headers_lacking_an_original_is_published_as_incomplete() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mail, account, _) = local_service(root.path()).await;
        let store = mail.store(&account).await.expect("local store");
        store
            .run(|db| {
                let inbox = listed(db, "INBOX", "INBOX", Some(Role::Inbox), 1);
                db.upsert_message(&inbox, 1, &header(1))?;
                db.upsert_message(&inbox, 1, &header(2))?;
                db.save_original("INBOX", 1, 1, Utc::now().fixed_offset(), &[], b"From: a@example.org\r\n\r\nKept\r\n")
            })
            .await
            .expect("one original of two");
        let jobs = BackupJobs::default();
        let selection = BackupSelection {
            source: BackupScope::Local,
            since: None,
            filename: Some("gaps.zip".into()),
            target_dir: None,
        };
        let job =
            jobs.start(Arc::clone(&mail), dirs(root.path()), account.clone(), selection, None).await.expect("started");
        let done = || async { jobs.status(&account, &job.id, None).is_some_and(|job| job.phase.is_final()) };
        eventually("the job ends", done).await;
        let job = jobs.status(&account, &job.id, None).expect("the job");
        assert_eq!(job.phase, BackupPhase::Incomplete, "a known gap is never complete");
        assert_eq!((job.archive.as_deref(), job.messages), (Some("gaps.zip"), 1));
        let expected = torro_core::i18n::trf("{n} local mail headers without an original were omitted.", &[("n", &1)]);
        assert_eq!(job.warnings, vec![expected]);
        let manifest = manifest_of(root.path(), "gaps.zip");
        assert_eq!(manifest.status, Status::Incomplete);
        assert_eq!(
            manifest.warnings,
            vec!["1 local mail headers without an original were omitted."],
            "English in the archive"
        );
    }

    #[tokio::test]
    async fn local_backup_after_a_move_and_a_renumbering_holds_each_mail_once() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mail, account, _) = local_service(root.path()).await;
        let store = mail.store(&account).await.expect("local store");
        let moved: &[u8] = b"From: a@example.org\r\n\r\nMoved\r\n";
        let renumbered: &[u8] = b"From: b@example.org\r\n\r\nRenumbered\r\n";
        store
            .run(move |db| {
                let now = Utc::now().fixed_offset();
                let inbox = listed(db, "INBOX", "INBOX", Some(Role::Inbox), 1);
                let archive = listed(db, "Archive", "Archive", None, 4);
                let id = db.upsert_message(&inbox, 1, &header(1))?;
                db.save_original("INBOX", 1, 1, now, &[], moved)?;
                // Moved to the archive: the sync takes it again there.
                db.relocate_message(&id, &archive, 4, 9)?;
                db.save_original("Archive", 4, 9, now, &[], moved)?;
                // The server renumbers the inbox: the same mail comes back under new UIDs.
                db.upsert_message(&inbox, 1, &header(2))?;
                db.save_original("INBOX", 1, 2, now, &[], renumbered)?;
                db.note_uidvalidity(&inbox, 2)?;
                db.upsert_message(&inbox, 2, &header(1002))?;
                db.save_original("INBOX", 2, 1002, now, &[], renumbered)
            })
            .await
            .expect("the depot after a move and a renumbering");

        let report = local_backup(&mail, root.path(), &account, None, "once.zip").await.expect("archive");
        assert!(report.complete);
        assert_eq!(report.messages, 2, "each mail once");
        let expected =
            torro_core::i18n::trf("{n} originals of mail no longer on the server were skipped.", &[("n", &2)]);
        assert_eq!(report.warnings, vec![expected]);
        let manifest = manifest_of(root.path(), "once.zip");
        let places: Vec<(Option<u32>, Option<u32>)> =
            manifest.messages.iter().map(|message| (message.uid_validity, message.uid)).collect();
        assert_eq!(places, vec![(Some(4), Some(9)), (Some(2), Some(1002))]);
    }

    /// Anna's account on the trap with one original of `bytes` bytes in the inbox.
    async fn local_with_one_original(root: &std::path::Path, bytes: usize) -> (Arc<MailService>, String) {
        let (mail, account, _) = local_service(root).await;
        let store = mail.store(&account).await.expect("local store");
        let raw = format!("From: a@example.org\r\n\r\n{}\r\n", "x".repeat(bytes));
        store
            .run(move |db| {
                let inbox = listed(db, "INBOX", "INBOX", Some(Role::Inbox), 1);
                db.upsert_message(&inbox, 1, &header(1))?;
                db.save_original("INBOX", 1, 1, Utc::now().fixed_offset(), &[], raw.as_bytes())
            })
            .await
            .expect("one original");
        (mail, account)
    }

    fn local_selection(name: &str, target_dir: Option<std::path::PathBuf>) -> BackupSelection {
        BackupSelection { source: BackupScope::Local, since: None, filename: Some(name.into()), target_dir }
    }

    /// Starts a local backup into `name` on `jobs` and waits until its job has ended.
    async fn ended_job(
        jobs: &BackupJobs,
        mail: &Arc<MailService>,
        root: &std::path::Path,
        account: &str,
        name: &str,
    ) -> BackupJob {
        let job = jobs
            .start(Arc::clone(mail), dirs(root), account.into(), local_selection(name, None), None)
            .await
            .expect("started");
        eventually("the job ends", || async {
            jobs.status(account, &job.id, None).is_some_and(|job| job.phase.is_final())
        })
        .await;
        jobs.status(account, &job.id, None).expect("the job")
    }

    #[tokio::test]
    async fn a_backup_refuses_a_taken_name_at_once_in_the_managed_and_a_chosen_folder() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mail, account) = local_with_one_original(root.path(), 10).await;
        let jobs = BackupJobs::default();
        let managed = dirs(root.path()).data.join("backups");
        fs::create_dir_all(&managed).expect("managed folder");
        fs::write(managed.join("taken.zip"), b"an older archive").expect("older archive");
        let chosen = root.path().join("chosen");
        fs::create_dir(&chosen).expect("chosen folder");
        fs::write(chosen.join("taken.zip"), b"a file of the person").expect("their file");
        let said = torro_core::i18n::trf(
            "A file named {name} already exists in the backup folder. Choose another name.",
            &[("name", &"taken.zip")],
        );
        for target in [None, Some(chosen.clone())] {
            let refused = jobs
                .start(
                    Arc::clone(&mail),
                    dirs(root.path()),
                    account.clone(),
                    local_selection("taken.zip", target),
                    None,
                )
                .await;
            let Err(error) = refused else { panic!("a taken name starts no job") };
            assert!(matches!(&error, BackupRunError::File(BackupFileError::Exists(name)) if name == "taken.zip"));
            assert_eq!(error.user_message(), said);
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(chosen.join("taken.zip"), managed.join("link.zip")).expect("a link");
            let refused = jobs
                .start(Arc::clone(&mail), dirs(root.path()), account.clone(), local_selection("link.zip", None), None)
                .await;
            assert!(matches!(refused, Err(BackupRunError::File(BackupFileError::Exists(_)))), "a link is taken too");
        }
        assert_eq!(fs::read(managed.join("taken.zip")).expect("older archive"), b"an older archive");
        assert_eq!(fs::read(chosen.join("taken.zip")).expect("their file"), b"a file of the person");
        // No job was made for the refused ones: the account is free for another name.
        let job = ended_job(&jobs, &mail, root.path(), &account, "free.zip").await;
        assert_eq!(job.phase, BackupPhase::Complete);
    }

    #[tokio::test]
    async fn a_backup_does_not_start_without_the_disk_reserve_but_unknown_space_stops_nothing() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mail, account) = local_with_one_original(root.path(), 10).await;
        let low = BackupJobs::with_space(DiskSpace::fake(1, |_| Some(DISK_RESERVE - 1)));
        let refused = low
            .start(Arc::clone(&mail), dirs(root.path()), account.clone(), local_selection("low.zip", None), None)
            .await;
        let Err(error) = refused else { panic!("too little space starts no job") };
        assert!(matches!(error, BackupRunError::Space(SpaceShortage::AtStart { free }) if free == DISK_RESERVE - 1));
        let said = torro_core::i18n::trf(
            "Not enough free disk space for a backup: {free} free, at least {reserve} must stay free.",
            &[("free", &"2.0 GiB"), ("reserve", &"2.0 GiB")],
        );
        assert_eq!(error.user_message(), said);
        backup_folder_holds(root.path(), &[]);

        let unknown = BackupJobs::with_space(DiskSpace::fake(1, |_| None));
        let job = ended_job(&unknown, &mail, root.path(), &account, "unknown.zip").await;
        assert_eq!(job.phase, BackupPhase::Complete, "{:?}", job.warnings);
        backup_folder_holds(root.path(), &["unknown.zip"]);
    }

    #[tokio::test]
    async fn a_local_backup_stops_when_its_mail_or_the_writing_would_eat_the_reserve() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mail, account) = local_with_one_original(root.path(), 100).await;
        // Room at the start, but not for the counted mail besides the reserve.
        let tight = BackupJobs::with_space(DiskSpace::fake(1, |_| Some(DISK_RESERVE + 10)));
        let job = ended_job(&tight, &mail, root.path(), &account, "tight.zip").await;
        assert_eq!(job.phase, BackupPhase::Incomplete);
        let expected =
            BackupRunError::Space(SpaceShortage::ForMail { free: DISK_RESERVE + 10, mail: 125 }).user_message();
        assert_eq!(job.warnings, vec![expected]);
        backup_folder_holds(root.path(), &[]);

        // Room at the start and for the mail; then the disk fills up while the archive is written.
        let asked = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&asked);
        let filling = BackupJobs::with_space(DiskSpace::fake(1, move |_| {
            let free = if counter.fetch_add(1, Ordering::SeqCst) < 2 { 4 * DISK_RESERVE } else { DISK_RESERVE / 2 };
            Some(free)
        }));
        let job = ended_job(&filling, &mail, root.path(), &account, "filling.zip").await;
        assert_eq!(job.phase, BackupPhase::Incomplete);
        let said = torro_core::i18n::trf(
            "Mail backup stopped because the disk is almost full: {free} free, at least {reserve} must stay free. The partial archive was removed.",
            &[("free", &"1.0 GiB"), ("reserve", &"2.0 GiB")],
        );
        assert_eq!(job.warnings, vec![said]);
        assert!(asked.load(Ordering::SeqCst) > 2, "asked again while writing");
        backup_folder_holds(root.path(), &[]);
    }

    /// A share the disk list leaves out (an SMB share under `/run/user/…/gvfs`, an NFS mount) matches
    /// `/` by its path; its own device shows that the system disk's free space is not its own.
    #[test]
    fn a_folder_on_another_device_than_its_mount_point_has_unknown_free_space() {
        use std::path::Path;
        let disks = [(Path::new("/"), 1), (Path::new("/home"), 2)];
        let share = Path::new("/run/user/1000/gvfs/smb-share:server=nas,share=backup");
        let device = |path: &Path| Some(if path.starts_with("/run/user/1000/gvfs") { 9 } else { 1 });
        assert_eq!(super::free_on(disks, share, device), None, "the NAS is not the system disk");
        assert_eq!(super::free_on(disks, Path::new("/srv/backups"), device), Some(1), "same device: the disk's");
        let unreadable = |path: &Path| (path != share).then_some(1);
        assert_eq!(super::free_on(disks, share, unreadable), None, "an unknown device is unknown space");
        // A real folder and the mount point it is under, as the system lists them: when a disk
        // matches, it is the folder's own file system.
        let root = tempfile::tempdir().expect("tempdir");
        let dir = root.path().canonicalize().expect("canonical");
        assert_eq!(
            super::free_on([(dir.as_path(), 5)], &dir.join("missing"), super::device_of),
            None,
            "a missing folder has no device"
        );
        assert_eq!(super::free_on([(dir.as_path(), 5)], &dir, super::device_of), Some(5));
    }

    #[test]
    fn the_disk_of_a_folder_is_the_one_with_the_longest_mount_point() {
        use std::path::Path;
        let disks =
            [(Path::new("/"), 1), (Path::new("/home"), 2), (Path::new("/home/anna/media"), 3), (Path::new("/ho"), 4)];
        let one_device = |_: &Path| Some(7);
        assert_eq!(super::free_on(disks, Path::new("/home/anna/backups"), one_device), Some(2));
        assert_eq!(super::free_on(disks, Path::new("/home/anna/media/x"), one_device), Some(3));
        assert_eq!(super::free_on(disks, Path::new("/srv/backups"), one_device), Some(1), "whole components: not /ho");
        assert_eq!(super::free_on([], Path::new("/srv"), one_device), None);
        // A folder that does not exist has no disk; an existing one is looked up without a panic.
        let root = tempfile::tempdir().expect("tempdir");
        assert_eq!(DiskSpace::system().free(&root.path().join("missing")), None);
        let _ = DiskSpace::system().free(root.path());
        assert_eq!(
            (super::size_words(DISK_RESERVE), super::size_words(512 * 1024 * 1024)),
            ("2.0 GiB".into(), "512.0 MiB".into())
        );
    }

    #[test]
    fn the_local_inventory_ignores_flags_but_not_bytes() {
        let original = LocalOriginal {
            seq: 1,
            folder_path: "INBOX".into(),
            uid_validity: 1,
            uid: 2,
            internal_date: Utc::now().fixed_offset(),
            flags: vec!["\\Seen".into()],
            bytes: 3,
            sha256: "0".repeat(64),
            gone: false,
        };
        let digest = |original: &LocalOriginal| {
            let mut hash = Sha256::new();
            super::hash_local_metadata(&mut hash, original);
            hash.finalize()
        };
        let flagged = LocalOriginal { flags: vec!["$Work".into()], ..original.clone() };
        assert_eq!(digest(&original), digest(&flagged), "the sync may change flags meanwhile");
        let changed = LocalOriginal { sha256: "1".repeat(64), ..original.clone() };
        assert_ne!(digest(&original), digest(&changed));
    }

    #[test]
    fn source_warnings_are_english_in_the_archive_and_translated_for_the_job() {
        let warning = SourceWarning::new("{n} local mail headers without an original were omitted.", &[("n", &3)]);
        assert_eq!(warning.archive_text(), "3 local mail headers without an original were omitted.");
        assert_eq!(
            warning.user_text(),
            torro_core::i18n::trf("{n} local mail headers without an original were omitted.", &[("n", &3)])
        );
    }

    // ----- restore (MB.5) -----

    /// One folder of a test archive: path, archive role and hierarchy delimiter.
    pub(crate) type TestFolder<'a> = (&'a str, Option<&'a str>, &'a str);

    /// A local archive of anna@example.org with `folders`, the `messages` (folder, raw bytes,
    /// flags) and the `aliases` (folder, the message whose bytes it repeats), with `status`.
    pub(crate) fn test_archive(
        folders: &[TestFolder<'_>],
        messages: &[(usize, &[u8], &[&str])],
        aliases: &[(usize, usize)],
        status: Status,
    ) -> Vec<u8> {
        use torro_mail_backup::format::{Account, Counts, FORMAT, Folder, Inventory, VERSION};
        use torro_mail_backup::writer::{ArchiveWriter, MessageMetadata};

        let started_at = Utc::now();
        let id = |at: usize| format!("f{:06}", at + 1);
        let manifest = Manifest {
            format: FORMAT.into(),
            version: VERSION,
            status: Status::Incomplete,
            source: Source::Local,
            since: None,
            started_at,
            finished_at: None,
            account: Account { address: "anna@example.org".into(), server: None },
            folders: folders
                .iter()
                .enumerate()
                .map(|(at, (path, role, delimiter))| Folder {
                    id: id(at),
                    name: (*path).into(),
                    path: (*path).into(),
                    delimiter: Some((*delimiter).into()),
                    selectable: true,
                    role: role.map(str::to_string),
                    uid_validity: None,
                    inventory: None,
                })
                .collect(),
            messages: Vec::new(),
            aliases: Vec::new(),
            counts: Counts { folders: folders.len() as u64, messages: 0, bytes: 0 },
            warnings: Vec::new(),
        };
        let mut writer = ArchiveWriter::new(std::io::Cursor::new(Vec::new()), manifest).expect("writer");
        let date = date("2026-03-01T09:00:00+01:00");
        for (folder, raw, flags) in messages {
            let metadata = MessageMetadata {
                internal_date: Some(date),
                flags: flags.iter().map(|flag| flag.to_string()).collect(),
                ..MessageMetadata::default()
            };
            writer.add_message(&id(*folder), *raw, metadata).expect("message");
        }
        for (folder, message) in aliases {
            let same_as = writer.manifest_mut().messages[*message].path.clone();
            let metadata = MessageMetadata { internal_date: Some(date), ..MessageMetadata::default() };
            writer.add_alias(&id(*folder), &same_as, metadata).expect("alias");
        }
        for (at, folder) in writer.manifest_mut().folders.iter_mut().enumerate() {
            let count = messages.iter().filter(|message| message.0 == at).count()
                + aliases.iter().filter(|alias| alias.0 == at).count();
            folder.inventory =
                Some(Inventory { checked_at: started_at, eligible_messages: count as u64, uid_set_sha256: None });
        }
        let (zip, _) = writer.finish(status, Utc::now()).expect("ZIP");
        zip.into_inner()
    }

    /// The first message of the checked-in sample archive (INBOX), as text.
    fn sample_raw() -> String {
        let mut archive = RestoreArchive::open(std::io::Cursor::new(SAMPLE_ZIP)).expect("verified sample");
        let mut raw = Vec::new();
        archive.copy_message(0, &mut raw).expect("original");
        String::from_utf8(raw).expect("sample is text")
    }

    /// A test run of the archive `zip` (written as `name` into the test folder) by `caller`.
    async fn plan_of(
        plans: &RestorePlans,
        mail: &MailService,
        root: &std::path::Path,
        zip: &[u8],
        name: &str,
        account: &str,
        caller: Option<&str>,
    ) -> Result<RestorePlan, BackupRunError> {
        let path = root.join(name);
        fs::write(&path, zip).expect("archive");
        plans.plan(mail, &dirs(root), name.into(), &path, account, caller).await
    }

    /// Applies plan `id` of the app, made from the archive `name` in the test folder.
    async fn apply_of(
        plans: &RestorePlans,
        mail: &MailService,
        root: &std::path::Path,
        name: &str,
        account: &str,
        id: &str,
        choice: RestoreChoice,
    ) -> Result<RestoreResult, BackupRunError> {
        let path = root.join(name);
        let request = ApplyRequest { path: &path, account, plan: id, caller: None, choice };
        plans.apply(mail, &dirs(root), request).await
    }

    fn appends(server: &FakeImap) -> usize {
        server.lines().iter().filter(|line| line.contains(" APPEND ")).count()
    }

    fn journal_file(root: &std::path::Path, account: &str, zip: &[u8]) -> std::path::PathBuf {
        let archive = RestoreArchive::open(std::io::Cursor::new(zip)).expect("verified");
        RestoreJournal::file(&dirs(root), account, archive.manifest_sha256())
    }

    const PLAIN: RestoreChoice = RestoreChoice { allow_incomplete: false, retry_unconfirmed: false };
    const RETRY: RestoreChoice = RestoreChoice { allow_incomplete: false, retry_unconfirmed: true };

    fn counts(plan: &RestorePlan) -> (u64, u64, u64, u64) {
        (plan.messages_to_add, plan.already_present, plan.uncertain, plan.unconfirmed)
    }

    fn outcome(result: &RestoreResult) -> (u64, u64, u64) {
        (result.added, result.skipped, result.uncertain)
    }

    fn says(warnings: &[String], template: &'static str, args: &[(&str, &dyn std::fmt::Display)]) -> bool {
        let wanted = torro_core::i18n::trf(template, args);
        warnings.contains(&wanted)
    }

    #[tokio::test]
    async fn a_restore_test_run_only_reads_and_leaves_no_journal() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (1, 0, 0, 0));
        assert_eq!(plan.archive, "sample.zip", "the archive as the caller named it");
        let inbox = plan.folders.iter().find(|folder| folder.archive_path == "INBOX").expect("mapped inbox");
        assert_eq!((inbox.target_path.as_str(), inbox.messages_to_add, inbox.create), ("INBOX", 1, false));
        assert!(plans.planned_for(&account, &plan.id, None).is_some());

        server.add("INBOX", &sample_raw(), &["\\Seen"]);
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("again");
        assert_eq!(counts(&plan), (0, 1, 0, 0));
        assert!(
            says(
                &plan.warnings,
                "{n} messages already in the target have another date or other flags; they stay as they are.",
                &[("n", &1)]
            ),
            "{:?}",
            plan.warnings
        );
        assert_read_only(&server);
        assert!(server.lines().iter().any(|line| line.ends_with(" LOGOUT")), "its own session signs out");
        assert!(!dirs(root.path()).data.join("restore-journals").exists(), "a test run makes no journal");
    }

    #[tokio::test]
    async fn a_restore_does_not_wait_for_the_command_connection() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let held = mail.command(&account).await.expect("the command connection");
        let plans = RestorePlans::default();
        let planned = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None);
        let plan = tokio::time::timeout(std::time::Duration::from_secs(20), planned)
            .await
            .expect("the test run has a session of its own")
            .expect("plan");
        let applied = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN);
        let result = tokio::time::timeout(std::time::Duration::from_secs(20), applied)
            .await
            .expect("the restore has a session of its own")
            .expect("restored");
        held.release();
        assert_eq!(outcome(&result), (1, 0, 0));
    }

    #[tokio::test]
    async fn a_restore_fills_an_empty_and_a_partly_filled_target_once_and_uses_up_its_plan() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (a, b, c): (&[u8], &[u8], &[u8]) = (
            b"From: a@example.org\r\n\r\nA\r\n",
            b"From: b@example.org\r\n\r\nB\r\n",
            b"From: c@example.org\r\n\r\nC\r\n",
        );
        let zip = test_archive(
            &[("INBOX", Some("inbox"), "/"), ("Archive", None, "/"), ("Projects/2026", None, "/")],
            &[(0, a, &["\\Seen"]), (0, b, &[]), (1, c, &["$Work"]), (2, a, &["\\Flagged"])],
            &[],
            Status::Complete,
        );
        server.add_received("INBOX", a, &["\\Seen"], date("2026-03-01T09:00:00+01:00"));
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "mail.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (3, 1, 0, 0));
        assert_eq!(plan.folders_to_create, 1);
        assert!(plan.folders[2].create);
        assert!(!server.lines().iter().any(|line| line.contains(" CREATE ") || line.contains(" SUBSCRIBE ")));

        let result = apply_of(&plans, &mail, root.path(), "mail.zip", &account, &plan.id, PLAIN).await.expect("apply");
        assert_eq!(outcome(&result), (3, 1, 0));
        assert_eq!(appends(&server), 3);
        assert_eq!(server.messages("INBOX").len(), 2);
        assert_eq!(server.flags("Archive", 1), ["$Work"]);
        assert_eq!(server.messages("Projects/2026").len(), 1);
        let lines = server.lines();
        assert!(lines.iter().any(|line| line.contains(" CREATE \"Projects/2026\"")));
        assert!(
            lines.iter().any(|line| line.contains(" SUBSCRIBE \"Projects/2026\"")),
            "a created folder is subscribed"
        );
        assert!(lines.iter().any(|line| line.ends_with(" LOGOUT")));
        let again = apply_of(&plans, &mail, root.path(), "mail.zip", &account, &plan.id, PLAIN).await;
        assert!(matches!(again, Err(BackupRunError::Restore(RestoreProblem::PlanGone))), "{again:?}");

        let checked = plan_of(&plans, &mail, root.path(), &zip, "mail.zip", &account, None).await.expect("replan");
        assert_eq!(counts(&checked), (0, 4, 0, 0));
        assert!(checked.warnings.is_empty(), "date and flags were kept: {:?}", checked.warnings);
        assert!(
            !dirs(root.path()).data.join("restore-journals").read_dir().expect("dir").any(|_| true),
            "no open entry"
        );
    }

    #[tokio::test]
    async fn a_refused_upload_is_left_out_and_tried_again_by_a_later_restore() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        server.quirk_next_append(AppendQuirk::Refuse);
        let result = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (0, 0, 1), "refused mail is not confirmed");
        assert!(says(
            &result.warnings,
            "{n} messages were refused by the server and not restored; run the restore test again to retry them.",
            &[("n", &1)]
        ));
        assert!(!journal_file(root.path(), &account, SAMPLE_ZIP).exists(), "nothing is open");

        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (1, 0, 0, 0), "no dead end: the mail is simply to add again");
        let result = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (1, 0, 0));
        assert_eq!(server.messages("INBOX").len(), 1);
    }

    #[tokio::test]
    async fn an_upload_lost_on_the_way_is_unconfirmed_and_sent_again_only_when_asked() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        server.quirk_next_append(AppendQuirk::Lose);
        let result = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await;
        let result = result.expect("a result once an upload went out");
        assert_eq!(outcome(&result), (0, 0, 1));
        assert!(says(
            &result.warnings,
            "The restore stopped: the server did not confirm an upload to {path}. Run the restore test again; it shows whether the mail arrived.",
            &[("path", &"INBOX")]
        ));
        let journal = journal_file(root.path(), &account, SAMPLE_ZIP);
        let written = fs::read(&journal).expect("the journal keeps the upload open");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&journal).expect("journal mode").permissions().mode() & 0o777, 0o600);
        }

        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (0, 0, 0, 1));
        assert!(says(
            &plan.warnings,
            "{n} uploads to {path} of an earlier restore were never confirmed; they are sent again only with your confirmation.",
            &[("n", &1), ("path", &"INBOX")]
        ));
        assert_eq!(fs::read(&journal).expect("journal"), written, "a test run only reads the journal");

        let before = appends(&server);
        let refused = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await;
        assert!(matches!(refused, Err(BackupRunError::Restore(RestoreProblem::UnconfirmedNotAccepted))), "{refused:?}");
        assert_eq!(appends(&server), before, "nothing sent without the confirmation");
        let result =
            apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, RETRY).await.expect("retry");
        assert_eq!(outcome(&result), (1, 0, 0));
        assert_eq!(appends(&server), before + 1, "exactly one upload");
        assert_eq!(server.messages("INBOX").len(), 1);
        assert!(!journal.exists(), "nothing is open any more");
    }

    #[tokio::test]
    async fn an_upload_that_arrived_despite_a_lost_answer_counts_as_present() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        server.drop_after_next_append();
        let first = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&first), (0, 0, 1));
        assert_eq!(server.messages("INBOX").len(), 1, "the server stored the unanswered upload");

        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (0, 1, 0, 0), "one copy more than before: it arrived");
        let second = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&second), (0, 1, 0));
        assert_eq!(appends(&server), 1, "no second upload");
        assert!(!journal_file(root.path(), &account, SAMPLE_ZIP).exists(), "resolved");
    }

    #[tokio::test]
    async fn two_matching_copies_do_not_resolve_one_open_upload() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let archive = RestoreArchive::open(std::io::Cursor::new(SAMPLE_ZIP)).expect("verified sample");
        let (path, mut journal) =
            RestoreJournal::open(&dirs(root.path()), &account, archive.manifest_sha256()).expect("journal");
        journal.pending.insert(archive.manifest().messages[0].path.clone(), 0);
        journal.save(&path).expect("open upload");
        server.add("INBOX", &sample_raw(), &[]);
        server.add("INBOX", &sample_raw(), &[]);
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        assert_eq!((plan.uncertain, plan.unconfirmed), (1, 0));
        let refused = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, RETRY).await;
        assert!(matches!(refused, Err(BackupRunError::Restore(RestoreProblem::Uncertain))), "{refused:?}");
        assert_eq!(appends(&server), 0);
        assert!(path.exists(), "the open upload stays for the person");
    }

    #[tokio::test]
    async fn a_server_that_stores_mail_changed_never_gets_a_silent_duplicate() {
        // Without UIDPLUS nothing tells which mail the server stored: the run stops, the upload
        // stays open, and only a person's confirmation sends it again.
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        server.quirk_next_append(AppendQuirk::Alter);
        let result = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (0, 0, 1));
        assert!(says(
            &result.warnings,
            "The restore stopped: the server accepted an upload to {path}, but the mail there cannot be matched to the archive; it may have arrived changed. Check the folder before uploading it again.",
            &[("path", &"INBOX")]
        ));
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (0, 0, 0, 1));

        // With UIDPLUS the changed copy is known by its UID: it counts as restored.
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.act_with_uidplus();
        let (mail, account) = server_service(root.path(), &server).await;
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        server.quirk_next_append(AppendQuirk::Alter);
        let result = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (1, 0, 0));
        assert!(says(
            &result.warnings,
            "The server stored {n} restored messages changed; they count as restored and are not uploaded again.",
            &[("n", &1)]
        ));
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (0, 1, 0, 0));
        assert_eq!(appends(&server), 1);
    }

    #[tokio::test]
    async fn a_target_may_grow_during_a_restore_but_losing_planned_mail_stops_it_with_a_result() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let raws: [&[u8]; 3] = [
            b"From: a@example.org\r\n\r\nA\r\n",
            b"From: b@example.org\r\n\r\nB\r\n",
            b"From: c@example.org\r\n\r\nC\r\n",
        ];
        let zip = test_archive(
            &[("INBOX", Some("inbox"), "/")],
            &[(0, raws[0], &[]), (0, raws[1], &[]), (0, raws[2], &[])],
            &[],
            Status::Complete,
        );
        let kept = server.add("INBOX", "From: x@example.org\r\n\r\nAlready there\r\n", &[]);
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "grow.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (3, 0, 0, 0));
        // EXAMINE 1 was the test run; 2 the apply's look, 3 the check before the first upload,
        // 4 its confirmation: new mail arrives there.
        server.on_examine("INBOX", 4, Change::Add("From: y@example.org\r\n\r\nNew meanwhile\r\n".into()));
        let result = apply_of(&plans, &mail, root.path(), "grow.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (3, 0, 0), "{:?}", result.warnings);
        assert_eq!(server.messages("INBOX").len(), 5);
        let hashed = server.lines().iter().filter(|line| line.contains("BODY.PEEK[]<0.")).count();
        assert_eq!(hashed, 3, "only the uploads have an archived size; known UIDs are not read twice");

        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let kept_again = server.add("INBOX", "From: x@example.org\r\n\r\nAlready there\r\n", &[]);
        assert_eq!(kept, kept_again);
        let (mail, account) = server_service(root.path(), &server).await;
        let plan = plan_of(&plans, &mail, root.path(), &zip, "lose.zip", &account, None).await.expect("plan");
        // After the first upload is confirmed (EXAMINE 4), the planned mail goes before the second
        // one is confirmed (EXAMINE 5).
        server.on_examine("INBOX", 5, Change::Expunge(kept));
        let result = apply_of(&plans, &mail, root.path(), "lose.zip", &account, &plan.id, PLAIN).await;
        let result = result.expect("a result once mail was added");
        assert_eq!(outcome(&result), (1, 0, 2));
        assert!(says(
            &result.warnings,
            "The restore stopped: {path} lost mail or was reset meanwhile. Run the restore test again.",
            &[("path", &"INBOX")]
        ));
    }

    #[tokio::test]
    async fn a_removed_message_after_the_test_run_stops_before_any_write() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let uid = server.add("INBOX", &sample_raw(), &[]);
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        assert_eq!(plan.messages_to_add, 0);
        server.expunge("INBOX", uid);
        let refused = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await;
        assert!(matches!(refused, Err(BackupRunError::Restore(RestoreProblem::TargetChanged))), "{refused:?}");
        assert_eq!(appends(&server), 0);
    }

    #[tokio::test]
    async fn a_collection_restore_uploads_only_mail_found_in_no_other_folder() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.act_as_gmail();
        server.add_folder("[Gmail]/All Mail", "\\HasNoChildren \\All", 15);
        server.add_folder("[Gmail]/Starred", "\\HasNoChildren \\Flagged", 16);
        // As Gmail: every upload to a label shows up in "All Mail" too.
        server.mirror_appends_into("[Gmail]/All Mail");
        let (shared, only): (&[u8], &[u8]) =
            (b"From: a@example.org\r\n\r\nIn the inbox\r\n", b"From: b@example.org\r\n\r\nArchived only\r\n");
        let zip = test_archive(
            &[
                ("INBOX", Some("inbox"), "/"),
                ("[Gmail]/All Mail", Some("all"), "/"),
                ("[Gmail]/Starred", Some("flagged"), "/"),
            ],
            &[(0, shared, &[]), (1, only, &["\\Flagged"])],
            &[(1, 0), (2, 0), (2, 1)],
            Status::Complete,
        );
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "gmail.zip", &account, None).await.expect("plan");
        let all = &plan.folders[1];
        assert!(all.aggregate && plan.folders[2].aggregate && !plan.folders[0].aggregate);
        assert_eq!((all.messages_to_add, all.covered), (1, 1));
        // The inbox mail goes up without `\Flagged`: its starred copy has to go up itself.
        assert_eq!((plan.folders[2].messages_to_add, plan.folders[2].covered), (1, 1));
        assert_eq!(counts(&plan), (3, 0, 0, 0));
        let result = apply_of(&plans, &mail, root.path(), "gmail.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (3, 2, 0), "covered copies count as skipped: {:?}", result.warnings);
        assert_eq!(appends(&server), 3, "copies the target fills by itself are never uploaded");
        assert_eq!(server.messages("[Gmail]/Starred").len(), 1);
    }

    #[tokio::test]
    async fn important_mail_goes_up_as_real_mail_even_to_gmail() {
        let (inboxed, important): (&[u8], &[u8]) =
            (b"From: a@example.org\r\n\r\nIn the inbox\r\n", b"From: b@example.org\r\n\r\nImportant only\r\n");
        let zip = test_archive(
            &[("INBOX", Some("inbox"), "/"), ("[Gmail]/Important", Some("important"), "/")],
            &[(0, inboxed, &[]), (1, important, &[])],
            &[(1, 0)],
            Status::Complete,
        );
        for gmail in [false, true] {
            let root = tempfile::tempdir().expect("tempdir");
            let server = FakeImap::start(PASSWORD).await;
            if gmail {
                server.act_as_gmail();
            }
            server.add_folder("[Gmail]/Important", "\\HasNoChildren \\Important", 15);
            let (mail, account) = server_service(root.path(), &server).await;
            let plans = RestorePlans::default();
            let plan = plan_of(&plans, &mail, root.path(), &zip, "important.zip", &account, None).await.expect("plan");
            let row = &plan.folders[1];
            let result =
                apply_of(&plans, &mail, root.path(), "important.zip", &account, &plan.id, PLAIN).await.expect("run");
            // Gmail marks mail important by its own rules, so the copy goes up with the bytes of the
            // inbox mail there too; elsewhere the folder is real anyway.
            assert!(!row.aggregate, "gmail {gmail}");
            assert_eq!((row.messages_to_add, row.covered), (2, 0));
            assert_eq!(outcome(&result), (3, 0, 0), "{:?}", result.warnings);
            let stored: Vec<String> = server.messages("[Gmail]/Important").into_iter().map(|(_, _, raw)| raw).collect();
            assert!(stored.contains(&String::from_utf8_lossy(inboxed).into_owned()));
        }
    }

    #[tokio::test]
    async fn a_collection_maps_onto_the_target_folder_that_collects_whatever_its_name() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add_folder("Virtual/All", "\\HasNoChildren \\All", 15);
        let (inboxed, archived): (&[u8], &[u8]) =
            (b"From: a@example.org\r\n\r\nIn the inbox\r\n", b"From: b@example.org\r\n\r\nArchived only\r\n");
        let zip = test_archive(
            &[("INBOX", Some("inbox"), "/"), ("[Gmail]/All Mail", Some("all"), "/")],
            &[(0, inboxed, &[]), (1, archived, &[])],
            &[(1, 0)],
            Status::Complete,
        );
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "all.zip", &account, None).await.expect("plan");
        let all = &plan.folders[1];
        assert_eq!(all.target_path, "Virtual/All");
        assert!(all.aggregate && !all.create, "no real folder is made for a collection");
        assert_eq!((all.messages_to_add, all.covered), (1, 1));
        let result = apply_of(&plans, &mail, root.path(), "all.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (2, 1, 0), "{:?}", result.warnings);
        assert_eq!(server.messages("Virtual/All").len(), 1);
    }

    #[tokio::test]
    async fn covered_copies_count_only_once_the_mail_they_name_is_restored() {
        let (a, b): (&[u8], &[u8]) = (b"From: a@example.org\r\n\r\nA\r\n", b"From: b@example.org\r\n\r\nB\r\n");
        let zip = test_archive(
            &[("INBOX", Some("inbox"), "/"), ("All", Some("all"), "/")],
            &[(0, a, &[]), (0, b, &[])],
            &[(1, 0), (1, 1)],
            Status::Complete,
        );
        // The second upload is lost on the way (the run stops), or the first one is refused.
        for (plain, quirk) in [(1, AppendQuirk::Lose), (0, AppendQuirk::Refuse)] {
            let root = tempfile::tempdir().expect("tempdir");
            let server = FakeImap::start(PASSWORD).await;
            server.add_folder("All", "\\HasNoChildren \\All", 15);
            server.mirror_appends_into("All");
            let (mail, account) = server_service(root.path(), &server).await;
            let plans = RestorePlans::default();
            let plan = plan_of(&plans, &mail, root.path(), &zip, "covered.zip", &account, None).await.expect("plan");
            assert_eq!((counts(&plan), plan.folders[1].covered), ((2, 0, 0, 0), 2));
            server.quirk_append_after(plain, quirk);
            let result =
                apply_of(&plans, &mail, root.path(), "covered.zip", &account, &plan.id, PLAIN).await.expect("run");
            assert_eq!(outcome(&result), (1, 1, 2), "{quirk:?}: one mail and its copy are not restored");
        }
    }

    #[tokio::test]
    async fn an_open_upload_whose_identical_copy_was_removed_can_be_sent_again() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let archive = RestoreArchive::open(std::io::Cursor::new(SAMPLE_ZIP)).expect("verified sample");
        let (path, mut journal) =
            RestoreJournal::open(&dirs(root.path()), &account, archive.manifest_sha256()).expect("journal");
        // One identical copy was there before the upload; someone deleted it since.
        journal.pending.insert(archive.manifest().messages[0].path.clone(), 1);
        journal.save(&path).expect("open upload");
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (0, 0, 0, 1), "unconfirmed, not a dead end");
        let refused = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await;
        assert!(matches!(refused, Err(BackupRunError::Restore(RestoreProblem::UnconfirmedNotAccepted))), "{refused:?}");
        let result =
            apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, RETRY).await.expect("retry");
        assert_eq!(outcome(&result), (1, 0, 0));
        assert_eq!(server.messages("INBOX").len(), 1);
        assert!(!path.exists(), "nothing is open any more");
    }

    #[tokio::test]
    async fn a_folder_the_server_refuses_to_open_mid_restore_has_its_own_reason() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let zip = test_archive(
            &[("INBOX", Some("inbox"), "/"), ("Archive", Some("archive"), "/")],
            &[(0, b"From: a@example.org\r\n\r\nA\r\n", &[]), (1, b"From: b@example.org\r\n\r\nB\r\n", &[])],
            &[],
            Status::Complete,
        );
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "refused.zip", &account, None).await.expect("plan");
        // EXAMINE 1 was the test run, 2 the apply's look, 3 the check before the upload to Archive.
        server.on_examine("Archive", 3, Change::Refuse);
        let result = apply_of(&plans, &mail, root.path(), "refused.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (1, 0, 1));
        assert!(
            says(
                &result.warnings,
                "The restore stopped: the server refused to open folder {path}. Check the folder, then run the restore test again.",
                &[("path", &"Archive")]
            ),
            "{:?}",
            result.warnings
        );
    }

    #[tokio::test]
    async fn keywords_the_target_cannot_keep_are_not_sent() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.set_permanent_flags("INBOX", "\\Seen \\Flagged $Known");
        let zip = test_archive(
            &[("INBOX", Some("inbox"), "/")],
            &[(0, b"From: a@example.org\r\n\r\nKeywords\r\n", &["\\Seen", "$Work", "$Known", "\\Recent"])],
            &[],
            Status::Complete,
        );
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "flags.zip", &account, None).await.expect("plan");
        assert!(says(
            &plan.warnings,
            "{n} IMAP flags or keywords will be left out because the target cannot keep them.",
            &[("n", &2)]
        ));
        let result = apply_of(&plans, &mail, root.path(), "flags.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (1, 0, 0), "a strict server takes the upload: {:?}", result.warnings);
        assert_eq!(server.flags("INBOX", 1), ["$Known", "\\Seen"]);
        assert!(!server.lines().iter().any(|line| line.contains("$Work")));
    }

    #[tokio::test]
    async fn a_target_folder_that_cannot_be_opened_makes_its_mail_uncertain() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        server.add_folder("Projects", "\\Noselect \\HasChildren", 20);
        let zip = test_archive(
            &[("Projects", None, "/")],
            &[(0, b"From: a@example.org\r\n\r\nNowhere to go\r\n", &[])],
            &[],
            Status::Complete,
        );
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "noselect.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (0, 0, 1, 0));
        assert!(!plan.folders[0].create);
        assert!(says(
            &plan.warnings,
            "Folder {path} exists on the target but cannot be opened; its mail is left out.",
            &[("path", &"Projects")]
        ));
        let refused = apply_of(&plans, &mail, root.path(), "noselect.zip", &account, &plan.id, PLAIN).await;
        assert!(matches!(refused, Err(BackupRunError::Restore(RestoreProblem::Uncertain))), "{refused:?}");
        assert!(!server.lines().iter().any(|line| line.contains(" APPEND ") || line.contains(" CREATE ")));
    }

    #[tokio::test]
    async fn unicode_folders_are_created_and_filled_in_modified_utf7() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        // The first as raw text (an archive or a person may hold it), the second as a server
        // lists it, in modified UTF-7.
        let zip = test_archive(
            &[("Projekte/Grüße", None, "/"), ("Entw&APw-rfe", None, "/")],
            &[(0, b"From: a@example.org\r\n\r\nUnicode\r\n", &[])],
            &[],
            Status::Complete,
        );
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "unicode.zip", &account, None).await.expect("plan");
        assert_eq!(plan.folders[0].target_path, "Projekte/Gr&APwA3w-e");
        let shown: Vec<(&str, &str)> =
            plan.folders.iter().map(|row| (row.archive_shown(), row.target_shown())).collect();
        assert_eq!(shown, [("Projekte/Grüße", "Projekte/Grüße"), ("Entwürfe", "Entwürfe")], "decoded for people");
        assert_eq!(
            (plan.folders[1].archive_path.as_str(), plan.folders[1].target_path.as_str()),
            ("Entw&APw-rfe", "Entw&APw-rfe")
        );
        assert!(says(&plan.warnings, "Restore will create folder {path}.", &[("path", &"Entwürfe")]));
        assert!(plan.folders[0].create);
        assert!(says(&plan.warnings, "Restore will create folder {path}.", &[("path", &"Projekte/Grüße")]));
        let result = apply_of(&plans, &mail, root.path(), "unicode.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (1, 0, 0));
        assert_eq!(server.messages("Projekte/Gr&APwA3w-e").len(), 1);
        let lines = server.lines();
        assert!(lines.iter().any(|line| line.contains(" CREATE \"Projekte/Gr&APwA3w-e\"")));
        assert!(lines.iter().any(|line| line.contains(" APPEND \"Projekte/Gr&APwA3w-e\"")));
    }

    #[tokio::test]
    async fn inbox_matches_in_any_case_and_another_delimiter_is_left_to_the_person() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let zip = test_archive(
            &[("inbox", Some("inbox"), "."), ("Work.2026", None, "."), ("Flat", None, ".")],
            &[(0, b"From: a@example.org\r\n\r\nInbox\r\n", &[]), (1, b"From: b@example.org\r\n\r\nNested\r\n", &[])],
            &[],
            Status::Complete,
        );
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "dots.zip", &account, None).await.expect("plan");
        assert_eq!((plan.folders[0].target_path.as_str(), plan.folders[0].create), ("INBOX", false));
        assert_eq!(plan.folders[0].messages_to_add, 1);
        assert_eq!((plan.folders[1].uncertain, plan.folders[1].create), (1, false));
        assert!(plan.folders[2].create, "a name without either delimiter maps as it is");
        assert!(says(
            &plan.warnings,
            "Folder {path} uses another hierarchy delimiter than the target; its mail is left out.",
            &[("path", &"Work.2026")]
        ));
    }

    #[tokio::test]
    async fn an_incomplete_archive_needs_its_own_confirmation() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let zip = test_archive(
            &[("INBOX", Some("inbox"), "/")],
            &[(0, b"From: a@example.org\r\n\r\nPartial\r\n", &[])],
            &[],
            Status::Incomplete,
        );
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "partial.zip", &account, None).await.expect("plan");
        assert!(!plan.complete_archive);
        let refused = apply_of(&plans, &mail, root.path(), "partial.zip", &account, &plan.id, PLAIN).await;
        assert!(matches!(refused, Err(BackupRunError::Restore(RestoreProblem::IncompleteNotAccepted))), "{refused:?}");
        assert_eq!(appends(&server), 0);
        let accepted = RestoreChoice { allow_incomplete: true, retry_unconfirmed: false };
        let result = apply_of(&plans, &mail, root.path(), "partial.zip", &account, &plan.id, accepted).await;
        assert_eq!(outcome(&result.expect("confirmed")), (1, 0, 0));
    }

    #[tokio::test]
    async fn a_changed_archive_is_refused_before_any_write() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await.expect("plan");
        let other = test_archive(
            &[("INBOX", Some("inbox"), "/")],
            &[(0, b"From: a@example.org\r\n\r\nAnother\r\n", &[])],
            &[],
            Status::Complete,
        );
        fs::write(root.path().join("sample.zip"), other).expect("replaced archive");
        let refused = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await;
        assert!(matches!(refused, Err(BackupRunError::Restore(RestoreProblem::ArchiveChanged))), "{refused:?}");
        fs::write(root.path().join("sample.zip"), b"not a ZIP").expect("broken archive");
        let broken = apply_of(&plans, &mail, root.path(), "sample.zip", &account, &plan.id, PLAIN).await;
        assert!(matches!(broken, Err(BackupRunError::Archive(_))), "{broken:?}");
        assert!(!server.lines().iter().any(|line| line.contains(" APPEND ") || line.contains(" CREATE ")));
        assert!(!dirs(root.path()).data.join("restore-journals").exists());
    }

    #[tokio::test]
    async fn identical_archived_occurrences_are_counted_instead_of_collapsed() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let raw: &[u8] = b"From: a@example.org\r\n\r\nSame body\r\n";
        server.add_received("INBOX", raw, &[], date("2026-03-01T09:00:00+01:00"));
        let zip =
            test_archive(&[("INBOX", Some("inbox"), "/")], &[(0, raw, &[]), (0, raw, &[])], &[], Status::Complete);
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let plan = plan_of(&plans, &mail, root.path(), &zip, "twice.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (1, 1, 0, 0));
        let result = apply_of(&plans, &mail, root.path(), "twice.zip", &account, &plan.id, PLAIN).await.expect("run");
        assert_eq!(outcome(&result), (1, 1, 0));
        assert_eq!(server.messages("INBOX").len(), 2);
        let plan = plan_of(&plans, &mail, root.path(), &zip, "twice.zip", &account, None).await.expect("plan");
        assert_eq!(counts(&plan), (0, 2, 0, 0));
    }

    #[tokio::test]
    async fn a_client_keeps_four_plans_and_sees_only_its_own() {
        let root = tempfile::tempdir().expect("tempdir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, account) = server_service(root.path(), &server).await;
        let plans = RestorePlans::default();
        let mut ids = Vec::new();
        for _ in 0..5 {
            let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, Some("c1"))
                .await
                .expect("client plan");
            ids.push(plan.id);
        }
        assert!(plans.planned_for(&account, &ids[0], Some("c1")).is_none(), "the fifth drops the oldest");
        assert!(ids[1..].iter().all(|id| plans.planned_for(&account, id, Some("c1")).is_some()));
        assert!(plans.planned_for(&account, &ids[4], Some("c2")).is_none(), "another client's plan looks like none");
        assert!(plans.planned_for(&account, &ids[4], None).is_none(), "the app has plans of its own");
        let mut app = Vec::new();
        for _ in 0..5 {
            let plan = plan_of(&plans, &mail, root.path(), SAMPLE_ZIP, "sample.zip", &account, None).await;
            app.push(plan.expect("app plan").id);
        }
        assert!(app.iter().all(|id| plans.planned_for(&account, id, None).is_some()), "the app may keep more");
        assert!(plans.planned_for(&account, &app[0], Some("c1")).is_none());
        assert!(plans.planned_for("mail:bob@example.org", &app[0], None).is_none(), "bound to its account");
        let other = ApplyRequest {
            path: &root.path().join("sample.zip"),
            account: &account,
            plan: &ids[4],
            caller: Some("c2"),
            choice: PLAIN,
        };
        let refused = plans.apply(&mail, &dirs(root.path()), other).await;
        assert!(matches!(refused, Err(BackupRunError::Restore(RestoreProblem::PlanGone))), "{refused:?}");
    }

    #[tokio::test]
    async fn test_runs_under_way_count_against_the_plan_limits_and_give_their_place_back() {
        let root = tempfile::tempdir().expect("tempdir");
        let (mail, account, _) = local_service(root.path()).await;
        let missing = root.path().join("missing.zip");
        let plans = RestorePlans::default();
        let run = |plans: &RestorePlans, caller: Option<&'static str>| {
            let (plans, mail, root, missing, account) =
                (plans.clone(), Arc::clone(&mail), root.path().to_path_buf(), missing.clone(), account.clone());
            async move { plans.plan(&mail, &dirs(&root), "missing.zip".into(), &missing, &account, caller).await }
        };
        // Runs that fail (here: no archive) give their place back.
        for _ in 0..CLIENT_PLANS + 2 {
            assert!(matches!(run(&plans, Some("c1")).await, Err(BackupRunError::Io(_))));
        }
        // As many runs under way as the client may keep plans: the next is busy at once.
        let mut under_way: Vec<_> = (0..CLIENT_PLANS).map(|_| plans.reserve(Some("c1")).expect("a place")).collect();
        assert!(matches!(run(&plans, Some("c1")).await, Err(BackupRunError::Busy)));
        assert!(matches!(run(&plans, Some("c2")).await, Err(BackupRunError::Io(_))), "another client has its own");
        assert!(matches!(run(&plans, None).await, Err(BackupRunError::Io(_))), "and the app");
        drop(under_way.pop());
        assert!(matches!(run(&plans, Some("c1")).await, Err(BackupRunError::Io(_))), "a place came back");

        // All places of all callers taken by runs under way: every caller is busy.
        let plans = RestorePlans::default();
        let callers: Vec<String> = (0..ALL_PLANS / CLIENT_PLANS).map(|n| format!("c{n}")).collect();
        let mut all: Vec<_> = callers
            .iter()
            .flat_map(|caller| (0..CLIENT_PLANS).map(|_| plans.reserve(Some(caller.as_str())).expect("a place")))
            .collect();
        assert!(matches!(run(&plans, None).await, Err(BackupRunError::Busy)));
        assert!(matches!(run(&plans, Some("other")).await, Err(BackupRunError::Busy)));
        drop(all.pop());
        assert!(matches!(run(&plans, None).await, Err(BackupRunError::Io(_))));
    }

    /// #267: the early look at whose archive it is (verify, restore plan and apply) refuses only an
    /// archive that names another account; what it cannot read is left to the full verification.
    #[test]
    fn only_an_archive_that_names_another_account_is_refused_early() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = dir.path().join("anna.zip");
        fs::write(&archive, SAMPLE_ZIP).expect("archive");
        assert!(super::of_other_account(&archive, "mail:bob@example.org"));
        assert!(!super::of_other_account(&archive, "mail:anna@example.org"));
        assert!(!super::of_other_account(&dir.path().join("missing.zip"), "mail:bob@example.org"), "missing");
        let damaged = dir.path().join("damaged.zip");
        fs::write(&damaged, b"not a ZIP").expect("damaged");
        assert!(!super::of_other_account(&damaged, "mail:bob@example.org"), "left to the full check");
    }

    #[test]
    fn every_restore_problem_has_a_code_and_its_own_text() {
        let problems = [
            RestoreProblem::OtherAccount,
            RestoreProblem::UnsafeFolderName,
            RestoreProblem::PlanGone,
            RestoreProblem::ArchiveChanged,
            RestoreProblem::TargetChanged,
            RestoreProblem::IncompleteNotAccepted,
            RestoreProblem::UnconfirmedNotAccepted,
            RestoreProblem::Uncertain,
            RestoreProblem::JournalDamaged,
        ];
        let texts: HashSet<&str> = problems.iter().map(|problem| problem.user_message()).collect();
        assert_eq!(texts.len(), problems.len());
        for problem in problems {
            assert!(["not-found", "conflict", "error"].contains(&problem.code()), "{problem:?}");
        }
        assert_eq!(RestoreProblem::TargetChanged.code(), "conflict");
        // An archive of another account answers as a missing one does (#267).
        assert_eq!(
            RestoreProblem::OtherAccount.user_message(),
            torro_core::i18n::tr("Backup archive is not available.")
        );
    }

    #[test]
    fn plan_answers_cut_folder_rows_and_warnings_but_keep_the_totals() {
        use torro_ipc::mail::RestoreFolderPlan;
        let row = |create: bool| RestoreFolderPlan {
            archive_path: "F".into(),
            target_path: "F".into(),
            archive_display: "F".into(),
            target_display: "F".into(),
            create,
            messages_to_add: 0,
            already_present: 1,
            uncertain: 0,
            bytes_to_add: 0,
            aggregate: false,
            covered: 0,
        };
        let mut folders: Vec<RestoreFolderPlan> = (0..super::ANSWER_FOLDERS + 10).map(|_| row(false)).collect();
        folders.push(row(true));
        let full = RestorePlan {
            id: "p".into(),
            account: "mail:anna@example.org".into(),
            archive: "a.zip".into(),
            complete_archive: true,
            folders_to_create: 1,
            messages_to_add: 0,
            already_present: 510,
            uncertain: 0,
            unconfirmed: 0,
            bytes_to_add: 0,
            folders,
            warnings: Vec::new(),
        };
        let long = "x".repeat(5000);
        let warnings = (0..80)
            .map(|_| SourceWarning::new("Folder {path} is odd.", &[("path", &super::shown_path(&long))]))
            .collect();
        let shown = super::shown_plan(&full, warnings);
        assert_eq!(shown.folders.len(), super::ANSWER_FOLDERS);
        assert!(shown.folders[0].create, "rows with something to do come first");
        assert_eq!((shown.already_present, shown.folders_to_create), (510, 1));
        assert_eq!(shown.warnings.len(), super::ANSWER_WARNINGS);
        assert!(shown.warnings.iter().all(|warning| warning.chars().count() < 400), "folder names are cut");
        assert_eq!(
            shown.warnings.last(),
            Some(&torro_core::i18n::trf("{n} further warnings were left out.", &[("n", &32)]))
        );
    }
}
