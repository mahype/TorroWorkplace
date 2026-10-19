//! The register of the files the assistant's own tools made (task WZ.4, "Register" in `docs/assistent.md` "Eigene
//! Werkzeuge"): `<data>/tools/files.json`, the folder 0700 and the file 0600.
//!
//! Each file has an id (8 hex digits, as bots have), its path, title, template, when and by whom it was made, its
//! size and its SHA-256. At most [`MAX_FILES`] are kept; the oldest goes first. Only files of the register can be
//! opened, saved elsewhere and attached to drafts, and only while they are still what the core wrote: a real file
//! (no symbolic link) ending in `.pdf` with the same size and checksum ([`FileEntry::is_intact`]). A client reaches
//! only the files it made itself ([`Maker`]); the app key all of them.
//!
//! The register is read and written whole, under a lock, on the caller's thread: the core calls it on a blocking
//! thread. Nothing here logs a title or a path.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use torro_core::settings::PdfTemplateId;
use torro_ipc::tools::FileInfo;

/// The register keeps at most this many files; the oldest falls out first.
pub const MAX_FILES: usize = 500;

/// The register's file in the tools' data folder.
pub const FILE: &str = "files.json";

/// Who asks for a file of the register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maker<'a> {
    /// The app key, a person: every file.
    App,
    /// The client with this id: only the files it made.
    Client(&'a str),
}

impl Maker<'_> {
    /// The id the register keeps for files made this way: `None` for the app key.
    pub fn client(self) -> Option<String> {
        match self {
            Maker::App => None,
            Maker::Client(id) => Some(id.to_string()),
        }
    }

    /// Whether this maker reaches `entry`: the app key every file, a client those it made.
    fn reaches(self, entry: &FileEntry) -> bool {
        match self {
            Maker::App => true,
            Maker::Client(id) => entry.client.as_deref() == Some(id),
        }
    }
}

/// One file of the register.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    /// 8 hex digits ([`new_id`]).
    pub id: String,
    /// Where the core wrote it, an absolute path.
    pub path: PathBuf,
    /// The title it was made with: the client's text.
    pub title: String,
    /// The template its pages were set with.
    pub template: PdfTemplateId,
    /// When it was made.
    pub created: DateTime<Utc>,
    /// The client that made it; `None` for the app key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256, 64 lowercase hex digits.
    pub sha256: String,
}

impl FileEntry {
    /// The entry of a file the core just wrote with `bytes` at `path`.
    pub fn of(
        id: String,
        path: PathBuf,
        title: &str,
        template: PdfTemplateId,
        maker: Maker<'_>,
        bytes: &[u8],
    ) -> FileEntry {
        FileEntry {
            id,
            path,
            title: title.to_string(),
            template,
            created: Utc::now(),
            client: maker.client(),
            size: bytes.len() as u64,
            sha256: sha256(bytes),
        }
    }

    /// Whether `bytes` are the file as the core wrote it: the same size and checksum.
    pub fn holds(&self, bytes: &[u8]) -> bool {
        bytes.len() as u64 == self.size && sha256(bytes) == self.sha256
    }

    /// Whether the file is still what the core wrote: a real file (no symbolic link) ending in `.pdf`, of the same
    /// size and checksum. Reads the whole file.
    pub fn is_intact(&self) -> bool {
        let pdf = self.path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
        let real = fs::symlink_metadata(&self.path).is_ok_and(|meta| meta.is_file() && meta.len() == self.size);
        pdf && real && fs::read(&self.path).is_ok_and(|bytes| self.holds(&bytes))
    }

    /// The file as `tools.create_pdf` answers it.
    pub fn info(&self) -> FileInfo {
        FileInfo {
            id: self.id.clone(),
            name: self.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            path: self.path.to_string_lossy().into_owned(),
            title: self.title.clone(),
            template: self.template,
            size: self.size,
            created: self.created,
        }
    }
}

/// Why the register could not keep a file. Names no path: the log may show it.
#[derive(Debug)]
pub enum RegisterError {
    /// There is no data folder (no home directory).
    NoFolder,
    /// The register cannot be read or written.
    Io(io::Error),
    /// No random numbers for an id.
    Random,
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterError::NoFolder => f.write_str("the register has no folder (no home directory)"),
            RegisterError::Io(err) => write!(f, "the register cannot be written ({})", err.kind()),
            RegisterError::Random => f.write_str("no random numbers for a file id"),
        }
    }
}

/// The register at `<data>/tools/files.json`. Cloning shares its lock.
#[derive(Debug, Clone)]
pub struct Register {
    /// `files.json`; `None` without a data folder: then nothing is kept and nothing is found.
    file: Option<PathBuf>,
    /// One reader or writer at a time, so two files made at once both stay.
    lock: Arc<Mutex<()>>,
}

impl Register {
    /// The register in the tools' data folder `dir` (`<data>/tools`); `None` keeps nothing.
    pub fn in_dir(dir: Option<PathBuf>) -> Register {
        Register { file: dir.map(|dir| dir.join(FILE)), lock: Arc::new(Mutex::new(())) }
    }

    /// Keeps `entry` with the caller's id as the newest file, for tests that need fixed ids; a file the core made
    /// takes [`Register::add_new`], which draws an id the register does not hold.
    #[cfg(test)]
    pub fn add(&self, entry: FileEntry) -> Result<(), RegisterError> {
        let file = self.file.as_ref().ok_or(RegisterError::NoFolder)?;
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut entries = read(file);
        entries.push(entry);
        write(file, entries)
    }

    /// Keeps the entry `made` builds from a new id as the newest file and answers it; the oldest beyond
    /// [`MAX_FILES`] fall out. Makes the folder (0700) and writes the file (0600) whole, so a crash never leaves half
    /// a register. The id is drawn under the register's lock and is none the register holds: two files with one id
    /// would make one of them unreachable (the first wins in [`Register::get`]).
    pub fn add_new(&self, made: impl FnOnce(String) -> FileEntry) -> Result<FileEntry, RegisterError> {
        let file = self.file.as_ref().ok_or(RegisterError::NoFolder)?;
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut entries = read(file);
        let entry = made(unused_id(&entries, new_id)?);
        entries.push(entry.clone());
        write(file, entries).map(|()| entry)
    }

    /// The file `id` when `maker` reaches it and it is still intact ([`FileEntry::is_intact`]); `None` otherwise,
    /// alike for an unknown id, another client's file and a file that was changed, moved or removed.
    pub fn get(&self, id: &str, maker: Maker<'_>) -> Option<FileEntry> {
        self.entries()
            .into_iter()
            .find(|entry| entry.id == id)
            .filter(|entry| maker.reaches(entry) && entry.is_intact())
    }

    /// The file at `path` when the register made one there, `maker` reaches it and it is still intact; `None`
    /// otherwise. For attachments of drafts, which name a file by its path.
    pub fn contains(&self, path: &Path, maker: Maker<'_>) -> Option<FileEntry> {
        // The newest first: a path written again after its file was removed belongs to the new one.
        self.entries()
            .into_iter()
            .rev()
            .find(|entry| entry.path == path)
            .filter(|entry| maker.reaches(entry) && entry.is_intact())
    }

    /// Every entry, oldest first.
    fn entries(&self) -> Vec<FileEntry> {
        let Some(file) = &self.file else { return Vec::new() };
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        read(file)
    }
}

/// The entries in `file`; none when it is missing or unreadable (with a line in the log that names no path).
fn read(file: &Path) -> Vec<FileEntry> {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            eprintln!("torroworkplace: cannot read the register of the tools' files ({})", err.kind());
            return Vec::new();
        }
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        eprintln!("torroworkplace: the register of the tools' files is no register; it starts anew");
        Vec::new()
    })
}

/// `entries` without the oldest beyond [`MAX_FILES`], written to `file` (0600) in a folder of its own (0700).
fn write(file: &Path, mut entries: Vec<FileEntry>) -> Result<(), RegisterError> {
    let over = entries.len().saturating_sub(MAX_FILES);
    entries.drain(..over);
    let dir = file.parent().ok_or(RegisterError::NoFolder)?;
    crate::clients::create_private_dir(dir).map_err(RegisterError::Io)?;
    let json = serde_json::to_vec_pretty(&entries).map_err(|err| RegisterError::Io(io::Error::other(err)))?;
    crate::clients::write_private(file, &json).map_err(|err| RegisterError::Io(io::Error::other(err.to_string())))
}

/// How often [`unused_id`] draws before it gives up. Among 2³² ids and at most [`MAX_FILES`] files a second draw is
/// rare already; one that keeps finding a held id means the random numbers are not random.
const DRAWS: usize = 8;

/// An id from `draw` that none of `entries` holds; [`RegisterError::Random`] after [`DRAWS`] held ones.
fn unused_id(
    entries: &[FileEntry],
    mut draw: impl FnMut() -> Result<String, RegisterError>,
) -> Result<String, RegisterError> {
    for _ in 0..DRAWS {
        let id = draw()?;
        if !entries.iter().any(|entry| entry.id == id) {
            return Ok(id);
        }
    }
    Err(RegisterError::Random)
}

/// A new file id: 8 random hex digits.
pub fn new_id() -> Result<String, RegisterError> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).map_err(|_| RegisterError::Random)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The SHA-256 of `bytes` in lowercase hex.
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A file `name` with `bytes` in `dir`, registered as made by `maker`.
    pub(crate) fn registered(register: &Register, dir: &Path, name: &str, bytes: &[u8], maker: Maker<'_>) -> FileEntry {
        let path = dir.join(name);
        fs::write(&path, bytes).expect("written");
        let entry = FileEntry::of(new_id().expect("an id"), path, name, PdfTemplateId::Report, maker, bytes);
        register.add(entry.clone()).expect("kept");
        entry
    }

    #[test]
    fn a_file_is_found_only_while_intact_and_by_its_maker() {
        let dir = tempfile::tempdir().expect("temp dir");
        let register = Register::in_dir(Some(dir.path().join("tools")));
        let mine = registered(&register, dir.path(), "Mine.pdf", b"%PDF mine", Maker::Client("c1"));
        let theirs = registered(&register, dir.path(), "Theirs.pdf", b"%PDF theirs", Maker::Client("c2"));
        let persons = registered(&register, dir.path(), "Person.pdf", b"%PDF person", Maker::App);

        assert_eq!(register.get(&mine.id, Maker::Client("c1")), Some(mine.clone()));
        assert_eq!(register.get(&theirs.id, Maker::Client("c1")), None, "another client's file");
        assert_eq!(register.get(&persons.id, Maker::Client("c1")), None, "the person's file");
        assert!(register.get(&theirs.id, Maker::App).is_some() && register.get(&persons.id, Maker::App).is_some());
        assert_eq!(register.get("00000000", Maker::App), None);
        assert_eq!(register.contains(&mine.path, Maker::Client("c1")), Some(mine.clone()));
        assert_eq!(register.contains(&theirs.path, Maker::Client("c1")), None);
        assert_eq!(register.contains(&dir.path().join("Other.pdf"), Maker::App), None);

        // Changed: the same size, other bytes.
        fs::write(&mine.path, b"%PDF MINE").expect("changed");
        assert_eq!(register.get(&mine.id, Maker::Client("c1")), None);
        assert_eq!(register.contains(&mine.path, Maker::Client("c1")), None);
        fs::remove_file(&theirs.path).expect("removed");
        assert_eq!(register.get(&theirs.id, Maker::App), None);
        #[cfg(unix)]
        {
            fs::rename(&persons.path, dir.path().join("moved.pdf")).expect("moved");
            std::os::unix::fs::symlink(dir.path().join("moved.pdf"), &persons.path).expect("linked");
            assert_eq!(register.get(&persons.id, Maker::App), None, "a link is not the file");
        }
    }

    #[test]
    fn the_register_keeps_the_newest_and_is_private() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tools = dir.path().join("tools");
        let register = Register::in_dir(Some(tools.clone()));
        let first = registered(&register, dir.path(), "0.pdf", b"0", Maker::App);
        for n in 1..=MAX_FILES {
            let entry =
                FileEntry::of(format!("{n:08x}"), dir.path().join("x.pdf"), "x", PdfTemplateId::Note, Maker::App, b"x");
            register.add(entry).expect("kept");
        }
        assert_eq!(register.entries().len(), MAX_FILES);
        assert!(register.entries().iter().all(|entry| entry.id != first.id), "the oldest fell out");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).expect("there").permissions().mode() & 0o777;
            assert_eq!((mode(&tools), mode(&tools.join(FILE))), (0o700, 0o600));
        }
        let none = Register::in_dir(None);
        assert!(matches!(none.add(first.clone()), Err(RegisterError::NoFolder)));
        assert_eq!(none.get(&first.id, Maker::App), None);
    }

    /// A new file never takes an id the register holds: the draw is repeated until it is unused.
    #[test]
    fn a_new_file_gets_an_id_the_register_does_not_hold() {
        let dir = tempfile::tempdir().expect("temp dir");
        let register = Register::in_dir(Some(dir.path().join("tools")));
        let taken = registered(&register, dir.path(), "Taken.pdf", b"%PDF taken", Maker::App);
        let mut draws = vec![Ok(taken.id.clone()), Ok(taken.id.clone()), Ok("0000beef".to_string())].into_iter();
        let id = unused_id(&register.entries(), || draws.next().unwrap_or(Err(RegisterError::Random)));
        assert_eq!(id.ok().as_deref(), Some("0000beef"));
        let always = || Ok(taken.id.clone());
        assert!(matches!(unused_id(&register.entries(), always), Err(RegisterError::Random)), "it gives up");

        let path = dir.path().join("New.pdf");
        fs::write(&path, b"%PDF new").expect("written");
        let made = |id| FileEntry::of(id, path.clone(), "New", PdfTemplateId::Note, Maker::Client("c1"), b"%PDF new");
        let kept = register.add_new(made).expect("kept");
        assert_ne!(kept.id, taken.id);
        assert_eq!(register.get(&kept.id, Maker::Client("c1")), Some(kept));
        assert_eq!(register.entries().len(), 2);
    }

    #[test]
    fn an_id_is_eight_hex_digits() {
        let id = new_id().expect("an id");
        assert_eq!(id.len(), 8);
        assert!(id.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')));
    }
}
