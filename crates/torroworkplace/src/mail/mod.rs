//! Mail in the core (tasks M2.5 to M3.3): what the core keeps of a mail account on this computer.
//!
//! - [`store`]: one SQLite file per account (folders, message headers, texts, full-text index,
//!   who wrote which draft) and the opaque ids (`f:…`, `m:…`) that clients hold on to.
//! - [`service`]: the mail accounts of the core, each with its IDLE connection, its command
//!   connection and its store (task M2.6); the commands (M3.1 to M3.3) start there.
//! - [`read`]: the reading commands (`mail.list_folders`, `mail.search_messages`,
//!   `mail.index_status`, `mail.get_message`, `mail.get_thread`, `mail.get_attachment`, task M3.1,
//!   issue #136).
//! - [`query`], [`fold`]: the query language of the search, and text as the search compares it
//!   (folding, highlights; issue #136).
//! - [`sync`]: how one account's folders are brought in line with its server (task M2.6).
//! - [`inbox_feed`]: new mail as entries of the inbox, and done entries back as read mail (task
//!   E4.1).
//! - [`write`]: changing mail: flags, moving, deleting and drafts (task M3.2).
//! - [`send`]: sending a draft under the send rule, its copy in the Sent folder (task M3.3).
//! - [`MailDirs`], [`AttachmentCache`]: where the files live, and the managed folder for
//!   attachments that were fetched on demand.
//!
//! **What is kept (the core decides, not the person):** the headers of every message of every
//! folder, the texts to read of the last [`store::BODY_RETENTION_DAYS`] days, the words of every
//! text in the search index (older texts are fetched for it in the background, issue #136),
//! attachments only when someone asks for them and only for [`ATTACHMENT_MAX_AGE`].
//!
//! **Threads.** The store is synchronous. A [`store::MailStore`] is `Send` but not `Sync`: keep it
//! in an `Arc<Mutex<…>>` and call it from async code through `tokio::task::spawn_blocking`, so a
//! slow disk never blocks the runtime. Each account has its own file and its own store, so one
//! account never waits for another.
//!
//! **Files** (security rule 5 of task M2.1): `<data>/mail/` and `<cache>/mail-attachments/` are
//! 0700, every file in them (the SQLite files with `-wal` and `-shm`, fetched attachments) 0600.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use torro_core::AppId;
use torro_core::paths::AppPaths;

pub mod fold;
pub mod inbox_feed;
pub mod query;
pub mod read;
pub mod send;
pub mod service;
pub mod store;
pub mod sync;
pub mod write;

/// Fetched attachments are deleted after this long (task M2.5).
pub const ATTACHMENT_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// The longest file name an attachment is stored under, in bytes.
const FILE_NAME_MAX: usize = 120;

/// Where mail files live. Tests pass a temporary directory; the core uses [`MailDirs::for_workplace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDirs {
    /// `<data>/mail/`: one `<account hash>.sqlite` per account.
    pub data: PathBuf,
    /// `<cache>/mail-attachments/`: the managed folder for fetched attachments.
    pub attachments: PathBuf,
}

impl MailDirs {
    /// `data_root/mail` and `cache_root/mail-attachments`. Creates nothing.
    pub fn new(data_root: &Path, cache_root: &Path) -> MailDirs {
        MailDirs { data: data_root.join("mail"), attachments: cache_root.join("mail-attachments") }
    }

    /// The directories of the core: `TORRO_DATA_HOME` (or the system's data directory) for the
    /// stores. `AppPaths` has no cache directory, so the cache root is `<state>/cache`: state is
    /// "what may be lost", which fetched attachments are. `None` when no home directory is known.
    pub fn for_workplace() -> Option<MailDirs> {
        let paths = AppPaths::for_app(AppId::WORKPLACE)?;
        Some(MailDirs::new(&paths.data, &paths.state.join("cache")))
    }
}

/// Why the mail store failed. The text never holds a path, a header or a message text: it may
/// reach a client; details go to the core's own log.
#[derive(Debug)]
pub enum MailStoreError {
    /// A file or directory could not be made, read or removed.
    Io(io::Error),
    /// SQLite refused.
    Sql(rusqlite::Error),
    /// A stored value is not what this version writes (a broken file).
    Corrupt(String),
    /// No random numbers for an id.
    Random(String),
    /// The bundled SQLite lacks the FTS5 module, so there could be no search.
    NoFullText,
    /// The file was made by a newer version of the core.
    NewerSchema(i64),
    /// The folder id is not one of this account's folders.
    UnknownFolder,
    /// The caller worked with a UIDVALIDITY the folder no longer has (or does not have yet):
    /// the UIDs it holds mean something else now. Re-read the folder with
    /// [`store::MailStore::note_uidvalidity`].
    StaleUidValidity,
}

impl fmt::Display for MailStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MailStoreError::Io(err) => write!(f, "cannot use the mail store: {}", err.kind()),
            MailStoreError::Sql(err) => write!(f, "the mail store failed: {err}"),
            MailStoreError::Corrupt(what) => write!(f, "the mail store holds a broken value: {what}"),
            MailStoreError::Random(err) => write!(f, "cannot make an id: {err}"),
            MailStoreError::NoFullText => f.write_str("this build of SQLite has no full-text search (FTS5)"),
            MailStoreError::NewerSchema(version) => write!(f, "the mail store is from a newer version ({version})"),
            MailStoreError::UnknownFolder => f.write_str("unknown mail folder"),
            MailStoreError::StaleUidValidity => f.write_str("the folder was renumbered by the server"),
        }
    }
}

impl std::error::Error for MailStoreError {}

impl From<io::Error> for MailStoreError {
    fn from(err: io::Error) -> Self {
        MailStoreError::Io(err)
    }
}

impl From<rusqlite::Error> for MailStoreError {
    fn from(err: rusqlite::Error) -> Self {
        MailStoreError::Sql(err)
    }
}

impl From<serde_json::Error> for MailStoreError {
    fn from(err: serde_json::Error) -> Self {
        MailStoreError::Corrupt(err.to_string())
    }
}

/// Creates `file` if missing, readable and writable only by the owner on Unix, and makes an
/// existing one so. Creating it with the mode (rather than tightening it afterwards) leaves no
/// moment in which another user could open it.
pub(crate) fn create_private(file: &Path) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(file)?;
    make_private(file)
}

/// Mode 0600 on Unix; Windows keeps files of the profile private already.
pub(crate) fn make_private(file: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(())
    }
}

/// The managed folder for attachments fetched on demand (`mail.get_attachment`, task M3.1). It is
/// also the only place a client key may attach files from (security rule 1 of task M2.1).
///
/// Each attachment gets a folder of its own (`<dir>/<random>/<file name>`), so two attachments
/// with the same name never meet and the age of a file is the age of its folder.
#[derive(Debug, Clone)]
pub struct AttachmentCache {
    dir: PathBuf,
}

impl AttachmentCache {
    /// The cache in `dirs.attachments`; nothing is created before the first [`store`](Self::store).
    pub fn new(dirs: &MailDirs) -> AttachmentCache {
        AttachmentCache { dir: dirs.attachments.clone() }
    }

    /// The managed folder.
    #[allow(dead_code, reason = "used from attachments of mail.get_attachment and drafts, tasks M3.1 and M3.2")]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `bytes` as a new file named after `file_name` (made safe by [`safe_file_name`]) and
    /// returns its absolute path. The folders are 0700, the file 0600. The caller enforces the
    /// size limit (50 MiB for `mail.get_attachment`).
    pub fn store(&self, file_name: &str, bytes: &[u8]) -> Result<PathBuf, MailStoreError> {
        use std::io::Write;
        crate::clients::create_private_dir(&self.dir)?;
        let folder = self.dir.join(random_hex(12)?);
        crate::clients::create_private_dir(&folder)?;
        let path = folder.join(safe_file_name(file_name));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path)?.write_all(bytes)?;
        make_private(&path)?;
        Ok(path)
    }

    /// `path` is a regular file inside the managed folder: not a symbolic link, and not outside
    /// after the system resolved `..` and links on the way. A path that is not there is not
    /// inside.
    #[allow(dead_code, reason = "used from attachments of drafts, task M3.2")]
    pub fn contains(&self, path: &Path) -> bool {
        let is_file = fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file());
        let inside = match (path.canonicalize(), self.dir.canonicalize()) {
            (Ok(path), Ok(dir)) => path.starts_with(dir),
            _ => false,
        };
        is_file && inside
    }

    /// Deletes what is older than `max_age` ([`ATTACHMENT_MAX_AGE`]); returns how many attachments
    /// went. Run it when the core starts and now and then. A missing folder is nothing to do.
    pub fn purge(&self, max_age: Duration) -> Result<usize, MailStoreError> {
        self.purge_at(SystemTime::now(), max_age)
    }

    /// [`purge`](Self::purge) as of `now` (tests).
    pub(crate) fn purge_at(&self, now: SystemTime, max_age: Duration) -> Result<usize, MailStoreError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(err) => return Err(err.into()),
        };
        let mut removed = 0;
        for entry in entries {
            let entry = entry?;
            // `symlink_metadata`: a link planted here is removed, never followed.
            let meta = fs::symlink_metadata(entry.path())?;
            let age = now.duration_since(meta.modified()?).unwrap_or_default();
            if age <= max_age {
                continue;
            }
            if meta.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
            removed += 1;
        }
        Ok(removed)
    }
}

/// A file name from the outside made safe to store: only the last part after `/` or `\`, no
/// control or invisible characters, characters Windows refuses replaced by `_`, no leading dots or
/// spaces (so never `.` or `..`), at most 120 bytes; `attachment` when nothing is left.
pub fn safe_file_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !is_invisible(*c))
        .map(|c| if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let mut cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    while cleaned.len() > FILE_NAME_MAX {
        cleaned.pop();
    }
    let cleaned = cleaned.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if cleaned.is_empty() { "attachment".to_string() } else { cleaned.to_string() }
}

/// Zero-width and bidi characters, which change what a person reads without showing.
pub(crate) fn is_invisible(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}')
}

/// `bytes` random bytes as lowercase hex.
pub(crate) fn random_hex(bytes: usize) -> Result<String, MailStoreError> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|err| MailStoreError::Random(err.to_string()))?;
    Ok(buffer.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> (tempfile::TempDir, AttachmentCache) {
        let tmp = tempfile::tempdir().expect("temp dir");
        let dirs = MailDirs::new(&tmp.path().join("data"), &tmp.path().join("cache"));
        (tmp, AttachmentCache::new(&dirs))
    }

    #[test]
    fn dirs_are_below_the_given_roots() {
        let dirs = MailDirs::new(Path::new("/d"), Path::new("/c"));
        assert_eq!((dirs.data, dirs.attachments), (PathBuf::from("/d/mail"), PathBuf::from("/c/mail-attachments")));
    }

    #[test]
    fn names_from_outside_become_safe_file_names() {
        assert_eq!(safe_file_name("report.pdf"), "report.pdf");
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("..\\..\\win.ini"), "win.ini");
        assert_eq!(safe_file_name(".."), "attachment");
        assert_eq!(safe_file_name("/"), "attachment");
        assert_eq!(safe_file_name(""), "attachment");
        assert_eq!(safe_file_name(" .hidden"), "hidden");
        assert_eq!(safe_file_name("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(safe_file_name("in\u{202E}voice\u{0}.pdf"), "invoice.pdf");
        assert_eq!(safe_file_name("Rechnung für Müller.pdf"), "Rechnung für Müller.pdf");
        assert!(safe_file_name(&"ä".repeat(200)).len() <= FILE_NAME_MAX);
    }

    #[test]
    fn a_stored_attachment_lands_in_the_managed_folder_even_with_a_hostile_name() {
        let (_tmp, cache) = cache();
        let path = cache.store("../../escape.txt", b"hello").expect("stored");
        assert!(path.starts_with(cache.dir()), "{path:?}");
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("escape.txt"));
        assert_eq!(fs::read(&path).expect("read back"), b"hello");
        assert!(cache.contains(&path));
        let other = cache.store("escape.txt", b"again").expect("stored twice");
        assert_ne!(path, other, "the same name never meets itself");
    }

    #[test]
    fn contains_refuses_paths_outside_missing_files_and_links() {
        let (tmp, cache) = cache();
        let inside = cache.store("a.txt", b"x").expect("stored");
        let outside = tmp.path().join("outside.txt");
        fs::write(&outside, b"x").expect("write");
        assert!(!cache.contains(&outside));
        assert!(!cache.contains(&inside.with_file_name("missing.txt")));
        assert!(!cache.contains(cache.dir()), "the folder itself is not an attachment");
        let sneaky = inside.parent().expect("folder").join("..").join("..").join("..").join("outside.txt");
        assert!(!cache.contains(&sneaky));
        #[cfg(unix)]
        {
            let link = inside.parent().expect("folder").join("link.txt");
            std::os::unix::fs::symlink(&outside, &link).expect("link");
            assert!(!cache.contains(&link), "a link out is refused");
            let inner = inside.parent().expect("folder").join("inner.txt");
            std::os::unix::fs::symlink(&inside, &inner).expect("link");
            assert!(!cache.contains(&inner), "a link is refused even to a file inside");
        }
    }

    #[test]
    fn purge_removes_only_what_is_older_than_a_day() {
        let (_tmp, cache) = cache();
        assert_eq!(cache.purge(ATTACHMENT_MAX_AGE).expect("no folder yet"), 0);
        let path = cache.store("a.txt", b"x").expect("stored");
        assert_eq!(cache.purge(ATTACHMENT_MAX_AGE).expect("fresh"), 0);
        assert!(path.exists());
        let in_23_hours = SystemTime::now() + Duration::from_secs(23 * 3600);
        assert_eq!(cache.purge_at(in_23_hours, ATTACHMENT_MAX_AGE).expect("young"), 0);
        let in_25_hours = SystemTime::now() + Duration::from_secs(25 * 3600);
        assert_eq!(cache.purge_at(in_25_hours, ATTACHMENT_MAX_AGE).expect("old"), 1);
        assert!(!path.exists() && !path.parent().expect("folder").exists(), "the attachment's folder goes with it");
        assert!(cache.dir().exists(), "the managed folder stays");
    }

    #[cfg(unix)]
    #[test]
    fn attachment_folders_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, cache) = cache();
        let path = cache.store("a.txt", b"x").expect("stored");
        let mode = |p: &Path| fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().expect("attachment folder")), 0o700);
        assert_eq!(mode(cache.dir()), 0o700);
    }
}
