//! The bounds of the tool `files` (issue #304, task WD.2, decisions 20 and 24 in `docs/assistent.md`): which paths
//! its commands reach in the folders a person allowed ([`Allowed`]), how far they may go there, and what stays out
//! whatever folder holds it.
//!
//! - A path from a request is absolute, at most [`MAX_PATH_BYTES`] bytes, of one line and without `.` or `..`
//!   parts, else `error` ([`FolderError::Invalid`]).
//! - It is resolved (`canonicalize`) and compared with the resolved allowed folders: a symbolic link that leads out
//!   of them is outside. The innermost folder that holds a path counts. Outside, missing or locked is `not-found`,
//!   so an agent cannot feel out what there is beyond the allowed folders; writing where only reading is allowed is
//!   `forbidden`.
//! - Locked are the paths of `excluded` (the suite's own folders, the files with client keys and hooks, the system's
//!   key stores, other known stores of credentials and the browsers' profiles, [`key_stores`]) and every name ending
//!   in [`BACKUP_SUFFIX`], also inside an allowed folder; their parts are compared without regard to case, since
//!   `canonicalize` keeps the case a request gave on macOS and Windows. Listing and finding leave them out. A locked
//!   path is `not-found` also when the system refuses the core there.
//! - An entry that is or holds an allowed folder or something locked is not moved (and in WD.3 not trashed): else an
//!   agent would reach what is in an inner "Read" folder through an outer one ([`Allowed::holds_protected`]).
//!
//! Every root comes in as a parameter, so tests never touch the person's files. Nothing here logs or answers a path
//! from a request.

use std::cmp::Reverse;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use torro_core::i18n::{tr, trf};
use torro_core::settings::{AllowedFolder, FolderAccess};
use torro_ipc::Response;

use crate::client_setup::BACKUP_SUFFIX;

/// A path in a request is at most this many bytes (as the setting `allowed_folder` takes it).
pub const MAX_PATH_BYTES: usize = 4096;

/// The name a new entry gets (the last part of a new path) is at most this many bytes, what Linux and APFS take.
pub const MAX_NAME_BYTES: usize = 255;

/// Why a command of the tool `files` cannot run; [`FolderError::response`] is its answer, for people and without a
/// path or name from the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderError {
    /// `error`: the request cannot work as given (a path that is not absolute, has `.` or `..` parts, a name too
    /// long, a file where a folder is needed …); the English message.
    Invalid(&'static str),
    /// `not-found`: outside the allowed folders, missing, or locked.
    NotFound,
    /// `forbidden`: writing where only reading is allowed, or moving what is or holds an allowed folder or
    /// something locked; the English message.
    Forbidden(&'static str),
    /// `unsupported`: copying a folder or a link, moving a folder or a link to another drive; the English message.
    Unsupported(&'static str),
    /// `error`: the system does not let the core in (`ErrorKind::PermissionDenied`), on macOS mostly until a person
    /// allows it under Privacy & Security › Files and Folders.
    Denied,
    /// `error`: the system failed otherwise; the kind of the error, never a path.
    Failed(String),
}

impl FolderError {
    /// The answer to the client: a fixed code and a message in the suite's language.
    pub fn response(&self) -> Response {
        let (code, message) = match self {
            FolderError::Invalid(message) => ("error", tr(message).to_string()),
            FolderError::NotFound => ("not-found", tr("No such file or folder in the allowed folders.").to_string()),
            FolderError::Forbidden(message) => ("forbidden", tr(message).to_string()),
            FolderError::Unsupported(message) => ("unsupported", tr(message).to_string()),
            FolderError::Denied => ("error", tr(DENIED).to_string()),
            FolderError::Failed(reason) => {
                ("error", trf("The files cannot be changed: {reason}", &[("reason", reason)]))
            }
        };
        Response::Error { code: code.into(), message }
    }

    /// The error for `err` from the system: `not-found` for a path that is gone, [`FolderError::Denied`] when the
    /// system does not let the core in, else its kind.
    pub fn of_io(err: &io::Error) -> FolderError {
        match err.kind() {
            io::ErrorKind::NotFound => FolderError::NotFound,
            io::ErrorKind::PermissionDenied => FolderError::Denied,
            kind => FolderError::Failed(kind.to_string()),
        }
    }
}

#[cfg(target_os = "macos")]
const DENIED: &str = "The system does not let Torro Workplace in there. Allow it under System Settings › Privacy & \
                      Security › Files and Folders.";
#[cfg(not(target_os = "macos"))]
const DENIED: &str = "The system does not let Torro Workplace in there.";

const BAD_PATH: &str = "Give a full path of one line, without . or .. parts.";
const NO_NAME: &str = "Give the new path with a name at its end.";
const LONG_NAME: &str = "A name may be at most 255 bytes long.";
/// Writing in a folder with `read` access.
pub const READ_ONLY: &str =
    "The tools may only read in this folder; a person can allow more in the assistant's settings.";
/// Moving (in WD.3 trashing) what is or holds an allowed folder or something locked.
pub const PROTECTED: &str = "This is or holds an allowed folder or files the suite protects; it stays where it is.";

/// The folders a person allowed, resolved as they are now, and what stays locked inside them. Made for each command
/// from the settings of now ([`Allowed::of`]), so a folder added, removed or mounted counts at once.
#[derive(Debug, Clone, Default)]
pub struct Allowed {
    /// The allowed folders that exist, resolved, with their access, in the order of the settings.
    roots: Vec<(PathBuf, FolderAccess)>,
    /// The parts of each of `roots`, in lower case, in the same order.
    folded_roots: Vec<Vec<String>>,
    /// The parts of each locked path in lower case: as given, with its folder resolved but its last part kept (a
    /// locked file may be a symbolic link: the link itself stays locked, as [`Allowed::entry`] names it), and resolved
    /// as far as it exists.
    locked: Vec<Vec<String>>,
}

impl Allowed {
    /// The allowed `folders` resolved with `fs::canonicalize`: a folder that does not exist (a drive not mounted) or
    /// is no folder falls away, and so does every path that is not absolute – checked before resolving, since the
    /// settings file is not checked again when it is read, and a relative path would be resolved from the core's
    /// working directory. `excluded` are the paths locked whatever folder holds them ([`super::Roots::excluded`]).
    pub fn of(folders: &[AllowedFolder], excluded: &[PathBuf]) -> Allowed {
        let roots: Vec<(PathBuf, FolderAccess)> = folders
            .iter()
            .filter(|folder| folder.check().is_ok())
            .filter_map(|folder| Some((fs::canonicalize(&folder.path).ok()?, folder.access)))
            .filter(|(root, _)| root.is_dir())
            .collect();
        let folded_roots = roots.iter().map(|(root, _)| folded(root)).collect();
        let locked = excluded
            .iter()
            .flat_map(|path| [folded(path), folded(&as_entry(path)), folded(&resolved_far(path))])
            .collect();
        Allowed { roots, folded_roots, locked }
    }

    /// The allowed folders that exist, resolved, with their access, in the order of the settings (also for the
    /// document tools of #303).
    pub fn roots(&self) -> &[(PathBuf, FolderAccess)] {
        &self.roots
    }

    /// `path` from a request resolved whole, following symbolic links, when it exists in an allowed folder whose
    /// access covers `need` and is not locked: what listing, finding and an existing target folder take.
    /// `error` for a path that is not absolute or has `.` or `..` parts, `not-found` outside, missing or locked,
    /// `forbidden` for [`FolderAccess::ReadWrite`] in a `read` folder, [`FolderError::Denied`] when the system does
    /// not let the core in.
    pub fn existing(&self, path: &str, need: FolderAccess) -> Result<PathBuf, FolderError> {
        let given = checked(path)?;
        let resolved = fs::canonicalize(&given).map_err(|err| self.missing(&given, &err))?;
        self.within(&resolved, need)?;
        Ok(resolved)
    }

    /// `path` from a request with only its folder resolved and its last part kept as it is: the entry itself, also
    /// when it is a symbolic link (moving one moves the link, not what it points to; the source of a copy must be a
    /// file, not a link). It must exist; the rules are those of [`Allowed::existing`].
    pub fn entry(&self, path: &str, need: FolderAccess) -> Result<PathBuf, FolderError> {
        let given = checked(path)?;
        let (Some(parent), Some(name)) = (given.parent(), given.file_name()) else { return Err(FolderError::NotFound) };
        let parent = fs::canonicalize(parent).map_err(|err| self.missing(parent, &err))?;
        let entry = parent.join(name);
        fs::symlink_metadata(&entry).map_err(|err| self.missing(&entry, &err))?;
        self.within(&entry, need)?;
        Ok(entry)
    }

    /// `path` from a request as a new entry: its folder must exist and is resolved, its last part is a plain name of
    /// at most [`MAX_NAME_BYTES`] bytes (`error` otherwise). Whether something of that name is there already is the
    /// command's to see (it then picks a free name). The other rules are those of [`Allowed::existing`].
    pub fn new_target(&self, path: &str, need: FolderAccess) -> Result<PathBuf, FolderError> {
        let given = checked(path)?;
        let (Some(parent), Some(name)) = (given.parent(), given.file_name()) else {
            return Err(FolderError::Invalid(NO_NAME));
        };
        if name.len() > MAX_NAME_BYTES {
            return Err(FolderError::Invalid(LONG_NAME));
        }
        let parent = fs::canonicalize(parent).map_err(|err| self.missing(parent, &err))?;
        let target = parent.join(name);
        self.within(&target, need)?;
        Ok(target)
    }

    /// Whether the resolved path `resolved` may be reached with `need`: not locked ([`Allowed::is_locked`]) and in
    /// an allowed folder (`not-found` otherwise), whose access – the innermost folder's – covers `need`
    /// (`forbidden` otherwise). For paths the other methods resolved, joined with a name from the disk.
    pub fn within(&self, resolved: &Path, need: FolderAccess) -> Result<(), FolderError> {
        if self.is_locked(resolved) {
            return Err(FolderError::NotFound);
        }
        let access = self.access_at(resolved).ok_or(FolderError::NotFound)?;
        match (need, access) {
            (FolderAccess::ReadWrite, FolderAccess::Read) => Err(FolderError::Forbidden(READ_ONLY)),
            _ => Ok(()),
        }
    }

    /// Whether `path` is locked: in or one of the `excluded` paths, or with a part that ends in [`BACKUP_SUFFIX`],
    /// compared without regard to case. Listing and finding leave such entries out.
    pub fn is_locked(&self, path: &Path) -> bool {
        let parts = folded(path);
        parts.iter().any(|part| part.ends_with(BACKUP_SUFFIX)) || self.locked.iter().any(|lock| parts.starts_with(lock))
    }

    /// Whether moving (or trashing) the entry `entry` would take along an allowed folder or something locked: it is
    /// one or holds one, compared without regard to case. Such a command is `forbidden` ([`PROTECTED`]).
    pub fn holds_protected(&self, entry: &Path) -> bool {
        let parts = folded(entry);
        self.locked.iter().chain(&self.folded_roots).any(|inner| inner.starts_with(&parts))
    }

    /// The access of the innermost allowed folder that holds `path`, `None` outside them all. A path counts as inside
    /// only when it starts with a resolved folder as it is; which folder is the innermost is decided without regard
    /// to case, and of two the same `read` wins: a request in another case must not slip past an inner `read`
    /// folder on a system that does not tell cases apart.
    fn access_at(&self, path: &Path) -> Option<FolderAccess> {
        if !self.roots.iter().any(|(root, _)| path.starts_with(root)) {
            return None;
        }
        let parts = folded(path);
        self.folded_roots
            .iter()
            .zip(&self.roots)
            .filter(|(root, _)| parts.starts_with(root))
            .max_by_key(|(root, (_, access))| (root.len(), Reverse(*access)))
            .map(|(_, (_, access))| *access)
    }

    /// The error for `path` that the system could not resolve: [`FolderError::Denied`] when it refused the core a
    /// path in an allowed folder, else `not-found` – also when it refused one outside, which is nobody's business, and
    /// one that is locked: the lock is looked at first, so the answer never tells that a locked path is there.
    fn missing(&self, path: &Path, err: &io::Error) -> FolderError {
        let resolved = resolved_far(path);
        if self.is_locked(path) || self.is_locked(&resolved) {
            return FolderError::NotFound;
        }
        match err.kind() {
            io::ErrorKind::PermissionDenied if self.access_at(&resolved).is_some() => FolderError::Denied,
            _ => FolderError::NotFound,
        }
    }
}

/// The system's key stores and other known stores of credentials below `home`, locked for the tool `files` whatever
/// folder holds them (decision 24): `~/.ssh`, `~/.gnupg`, the system's own (`~/Library/Keychains` on macOS, the
/// keyrings and KWallet on Linux, the credentials and the protected keys of Windows), the files of cloud, GitHub,
/// container and package registry tools that hold tokens, the password store `pass`, and the browsers' profiles
/// (cookies and saved passwords). Each is locked with all it holds.
pub fn key_stores(home: &Path) -> Vec<PathBuf> {
    let mut stores = vec![home.join(".ssh"), home.join(".gnupg")];
    stores.extend(
        [
            ".aws",
            ".azure",
            ".config/gcloud",
            ".config/gh",
            ".kube",
            ".docker/config.json",
            ".netrc",
            ".git-credentials",
            ".password-store",
            ".npmrc",
            ".pypirc",
            ".cargo/credentials.toml",
            ".cargo/credentials",
        ]
        .iter()
        .map(|store| in_home(home, store)),
    );
    #[cfg(target_os = "macos")]
    {
        stores.push(home.join("Library").join("Keychains"));
        let support = home.join("Library").join("Application Support");
        let browsers = ["Google/Chrome", "Chromium", "BraveSoftware", "Microsoft Edge", "Firefox"];
        stores.extend(browsers.iter().map(|browser| in_home(&support, browser)));
        stores.push(home.join("Library").join("Safari"));
        stores.push(home.join("Library").join("Cookies"));
    }
    #[cfg(target_os = "linux")]
    {
        stores.push(home.join(".local").join("share").join("keyrings"));
        stores.extend(dirs::data_dir().map(|data| data.join("keyrings")));
        stores.push(home.join(".local").join("share").join("kwalletd"));
        stores.push(home.join(".mozilla"));
        let browsers = ["google-chrome", "chromium", "BraveSoftware", "microsoft-edge"];
        stores.extend(browsers.iter().map(|browser| home.join(".config").join(browser)));
    }
    #[cfg(target_os = "windows")]
    {
        let roaming = dirs::config_dir().unwrap_or_else(|| home.join("AppData").join("Roaming"));
        let local = dirs::data_local_dir().unwrap_or_else(|| home.join("AppData").join("Local"));
        stores.push(roaming.join("Microsoft").join("Credentials"));
        stores.push(local.join("Microsoft").join("Credentials"));
        stores.push(roaming.join("Microsoft").join("Protect"));
        stores.push(local.join("Google").join("Chrome").join("User Data"));
        stores.push(local.join("Microsoft").join("Edge").join("User Data"));
        stores.push(local.join("BraveSoftware"));
        stores.push(roaming.join("Mozilla").join("Firefox"));
    }
    stores
}

/// `relative` (parts separated by `/`) below `base`, joined part by part so it has the system's separators.
fn in_home(base: &Path, relative: &str) -> PathBuf {
    relative.split('/').fold(base.to_path_buf(), |path, part| path.join(part))
}

/// `path` from a request as a path, when it is one the tool takes: absolute, at most [`MAX_PATH_BYTES`] bytes,
/// without control characters and without `.` or `..` parts. `Path::components` drops a `.` inside a path, so the
/// parts are read from the text as well.
fn checked(path: &str) -> Result<PathBuf, FolderError> {
    let dotted = path.split(std::path::is_separator).any(|part| part == "." || part == "..");
    let given = PathBuf::from(path);
    let parts_ok = given.components().all(|part| !matches!(part, Component::CurDir | Component::ParentDir));
    match path.len() <= MAX_PATH_BYTES && !path.chars().any(char::is_control) && !dotted && parts_ok {
        true if given.is_absolute() => Ok(given),
        _ => Err(FolderError::Invalid(BAD_PATH)),
    }
}

/// `path` resolved as far as it exists: the longest existing beginning resolved, the rest joined as it is. For locked
/// paths that do not exist (yet), and to tell where a path the system refused would be.
fn resolved_far(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut base = path;
    loop {
        if let Ok(resolved) = fs::canonicalize(base) {
            return rest.iter().rev().fold(resolved, |joined, name| joined.join(name));
        }
        match (base.parent(), base.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                base = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// `path` with its folder resolved as far as it exists and its last part kept: how [`Allowed::entry`] names a link.
fn as_entry(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => resolved_far(parent).join(name),
        _ => path.to_path_buf(),
    }
}

/// The parts of `path` in lower case, for comparisons that do not regard case.
fn folded(path: &Path) -> Vec<String> {
    path.components().map(|part| part.as_os_str().to_string_lossy().to_lowercase()).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use torro_ipc::Request;

    use super::*;

    /// A temporary folder `home` with `Documents` (read and write) and in it `Documents/Private` (read only), and a
    /// folder `outside` next to `home` that no allowed folder holds.
    pub(crate) struct Place {
        pub(crate) dir: tempfile::TempDir,
    }

    impl Place {
        pub(crate) fn new() -> Place {
            let dir = tempfile::tempdir().expect("temp dir");
            for folder in ["home/Documents/Private", "outside"] {
                fs::create_dir_all(dir.path().join(folder)).expect("made");
            }
            Place { dir }
        }

        /// `relative` below the temporary folder, as a string for a request.
        pub(crate) fn path(&self, relative: &str) -> String {
            self.dir.path().join(relative).to_string_lossy().into_owned()
        }

        pub(crate) fn folders(&self) -> Vec<AllowedFolder> {
            vec![
                AllowedFolder { path: self.path("home/Documents"), access: FolderAccess::ReadWrite },
                AllowedFolder { path: self.path("home/Documents/Private"), access: FolderAccess::Read },
            ]
        }

        pub(crate) fn allowed(&self) -> Allowed {
            Allowed::of(&self.folders(), &[])
        }
    }

    fn code(err: &FolderError) -> String {
        match err.response() {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn dots_relative_paths_and_control_characters_are_errors() {
        let place = Place::new();
        let allowed = place.allowed();
        let docs = place.path("home/Documents");
        let sep = std::path::MAIN_SEPARATOR;
        for path in [
            format!("{docs}{sep}..{sep}Documents"),
            format!("{docs}{sep}.{sep}Private"),
            format!("{docs}{sep}."),
            "Documents".to_string(),
            "./Documents".to_string(),
            String::new(),
            format!("{docs}{sep}a\nb"),
            format!("{docs}{sep}{}", "a".repeat(MAX_PATH_BYTES)),
        ] {
            for err in [
                allowed.existing(&path, FolderAccess::Read).expect_err("refused"),
                allowed.entry(&path, FolderAccess::Read).expect_err("refused"),
                allowed.new_target(&path, FolderAccess::Read).expect_err("refused"),
            ] {
                assert_eq!(code(&err), "error", "{path:?}");
            }
        }
        // A name that is too long for any system.
        let long = format!("{docs}{sep}{}", "a".repeat(MAX_NAME_BYTES + 1));
        assert_eq!(allowed.new_target(&long, FolderAccess::ReadWrite), Err(FolderError::Invalid(LONG_NAME)));
        let fits = format!("{docs}{sep}{}", "a".repeat(MAX_NAME_BYTES));
        assert!(allowed.new_target(&fits, FolderAccess::ReadWrite).is_ok());
    }

    /// The settings file is not checked again when read: a relative folder written there by hand falls away before
    /// it is resolved, so it is never taken from the core's working directory (where `src` exists in tests).
    #[test]
    fn a_relative_folder_in_the_settings_falls_away_before_it_is_resolved() {
        assert!(Path::new("src").is_dir(), "the crate's folder is the tests' working directory");
        let relative = [AllowedFolder { path: "src".into(), access: FolderAccess::ReadWrite }];
        assert!(Allowed::of(&relative, &[]).roots().is_empty());
    }

    #[test]
    fn a_missing_folder_falls_away_and_outside_is_not_found() {
        let place = Place::new();
        let mut folders = place.folders();
        folders.push(AllowedFolder { path: place.path("not mounted"), access: FolderAccess::ReadWrite });
        fs::write(place.dir.path().join("home/file"), b"x").expect("written");
        folders.push(AllowedFolder { path: place.path("home/file"), access: FolderAccess::ReadWrite });
        let allowed = Allowed::of(&folders, &[]);
        assert_eq!(allowed.roots().len(), 2, "only the folders that exist");
        assert_eq!(allowed.roots()[0].0, fs::canonicalize(place.path("home/Documents")).expect("resolved"));
        for path in [place.path("outside"), place.path("home"), place.path("home/Documents/missing")] {
            assert_eq!(allowed.existing(&path, FolderAccess::Read), Err(FolderError::NotFound), "{path}");
            assert_eq!(allowed.entry(&path, FolderAccess::Read), Err(FolderError::NotFound), "{path}");
        }
        assert_eq!(allowed.new_target(&place.path("outside/new"), FolderAccess::Read), Err(FolderError::NotFound));
        assert_eq!(
            allowed.new_target(&place.path("home/Documents/a/b"), FolderAccess::Read),
            Err(FolderError::NotFound)
        );
        assert!(allowed.existing(&place.path("home/Documents"), FolderAccess::Read).is_ok());
    }

    /// Decision 20: of folders one inside the other the innermost counts, `read` inside `read and write` and the
    /// other way round.
    #[test]
    fn the_innermost_folder_counts() {
        let place = Place::new();
        fs::create_dir(place.dir.path().join("home/Documents/Private/Open")).expect("made");
        let mut folders = place.folders();
        folders
            .push(AllowedFolder { path: place.path("home/Documents/Private/Open"), access: FolderAccess::ReadWrite });
        let allowed = Allowed::of(&folders, &[]);
        let need = FolderAccess::ReadWrite;
        assert!(allowed.new_target(&place.path("home/Documents/new"), need).is_ok());
        let in_private = allowed.new_target(&place.path("home/Documents/Private/new"), need);
        assert_eq!(in_private, Err(FolderError::Forbidden(READ_ONLY)));
        assert!(allowed.existing(&place.path("home/Documents/Private"), FolderAccess::Read).is_ok(), "reading is fine");
        assert!(allowed.new_target(&place.path("home/Documents/Private/Open/new"), need).is_ok());
        assert_eq!(code(&FolderError::Forbidden(READ_ONLY)), "forbidden");
    }

    /// The same folder in another case is still the inner `read` one: on macOS and Windows it is the same folder.
    #[test]
    fn the_inner_read_folder_holds_in_any_case() {
        let place = Place::new();
        let allowed = place.allowed();
        let private = allowed.roots()[1].0.clone();
        let other_case = private.with_file_name("PRIVATE").join("new");
        // Whatever the system makes of the case, the folder in it is never written in.
        assert!(allowed.within(&other_case, FolderAccess::ReadWrite).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_folder_is_outside_and_one_within_is_reached() {
        let place = Place::new();
        let docs = place.dir.path().join("home/Documents");
        std::os::unix::fs::symlink(place.dir.path().join("outside"), docs.join("out")).expect("linked");
        std::os::unix::fs::symlink(docs.join("Private"), docs.join("private link")).expect("linked");
        let allowed = place.allowed();
        assert_eq!(allowed.existing(&place.path("home/Documents/out"), FolderAccess::Read), Err(FolderError::NotFound));
        let through = allowed.new_target(&place.path("home/Documents/out/new"), FolderAccess::ReadWrite);
        assert_eq!(through, Err(FolderError::NotFound));
        let within = allowed.existing(&place.path("home/Documents/private link"), FolderAccess::Read).expect("reached");
        assert_eq!(within, fs::canonicalize(docs.join("Private")).expect("resolved"));
        // The link to the inner folder does not make it writable.
        let into = allowed.new_target(&place.path("home/Documents/private link/new"), FolderAccess::ReadWrite);
        assert_eq!(into, Err(FolderError::Forbidden(READ_ONLY)));
        // The link itself is an entry in the folder, wherever it points.
        let link = allowed.entry(&place.path("home/Documents/out"), FolderAccess::ReadWrite).expect("the link");
        assert_eq!(link.file_name().and_then(|name| name.to_str()), Some("out"));
    }

    /// Decision 24: the suite's folders are locked inside an allowed folder, also in another case, and so are the
    /// backups next to a client's file; the files with client keys and hooks too, and moving what holds one is
    /// `forbidden`.
    #[test]
    fn the_suites_folders_and_backups_are_locked() {
        let place = Place::new();
        let home = place.dir.path().join("home");
        let suite = home.join("Library/Application Support/torroworkplace");
        fs::create_dir_all(&suite).expect("made");
        fs::write(suite.join("clients.json"), b"{}").expect("written");
        fs::write(home.join(".claude.json"), b"{}").expect("written");
        fs::write(home.join(format!(".claude.json{BACKUP_SUFFIX}")), b"{}").expect("written");
        let folders = [AllowedFolder { path: place.path("home"), access: FolderAccess::ReadWrite }];
        let allowed = Allowed::of(&folders, &[suite.clone(), home.join(".claude.json")]);
        for path in [
            "home/Library/Application Support/torroworkplace",
            "home/Library/Application Support/torroworkplace/clients.json",
            "home/.claude.json",
            "home/.claude.json.torroworkplace-backup",
        ] {
            let path = place.path(path);
            assert_eq!(allowed.existing(&path, FolderAccess::Read), Err(FolderError::NotFound), "{path}");
            assert_eq!(allowed.entry(&path, FolderAccess::Read), Err(FolderError::NotFound), "{path}");
        }
        let new = place.path("home/Library/Application Support/torroworkplace/new.json");
        assert_eq!(allowed.new_target(&new, FolderAccess::ReadWrite), Err(FolderError::NotFound));
        let backup = place.path("home/x.TorroWorkplace-Backup");
        assert_eq!(allowed.new_target(&backup, FolderAccess::ReadWrite), Err(FolderError::NotFound));
        // In another case: the same folder on macOS and Windows.
        let resolved = fs::canonicalize(&home).expect("resolved");
        let shouted = resolved.join("LIBRARY/application support/TorroWorkplace/clients.json");
        assert!(allowed.is_locked(&shouted));
        assert_eq!(allowed.within(&shouted, FolderAccess::Read), Err(FolderError::NotFound));
        // Moving what holds them: forbidden, and so the allowed folder itself.
        assert!(allowed.holds_protected(&resolved.join("Library")));
        assert!(allowed.holds_protected(&resolved.join("library")));
        assert!(allowed.holds_protected(&resolved), "the allowed folder itself");
        assert!(!allowed.holds_protected(&resolved.join("Documents")));
    }

    /// Decision 24: with the whole home allowed, the files that carry client keys and hooks
    /// ([`crate::client_setup::protected_files`]) and their backups are not found, and listing leaves them out.
    #[test]
    fn the_client_files_are_locked_in_an_allowed_home() {
        let place = Place::new();
        let home = place.dir.path().join("home");
        let env = crate::client_setup::Env {
            home: home.clone(),
            app_config: home.join(".config"),
            path: Vec::new(),
            program: PathBuf::from("/opt/torro/torroworkplace"),
            config_dir: place.dir.path().join("config"),
        };
        fs::create_dir_all(home.join(".claude")).expect("made");
        for file in
            [".claude.json", ".claude/settings.json", ".claude/other.json", &format!(".claude.json{BACKUP_SUFFIX}")]
        {
            fs::write(home.join(file), b"{}").expect("written");
        }
        let folders = [AllowedFolder { path: place.path("home"), access: FolderAccess::ReadWrite }];
        let allowed = Allowed::of(&folders, &crate::client_setup::protected_files(&env));
        for locked in ["home/.claude.json", "home/.claude/settings.json", "home/.claude.json.torroworkplace-backup"] {
            assert_eq!(allowed.entry(&place.path(locked), FolderAccess::Read), Err(FolderError::NotFound), "{locked}");
        }
        assert!(allowed.existing(&place.path("home/.claude/other.json"), FolderAccess::Read).is_ok());
        let move_claude = Request::MoveFile { from: place.path("home/.claude"), to: place.path("home/Documents") };
        assert_eq!(super::super::organize::plan(&allowed, &move_claude), Err(FolderError::Forbidden(PROTECTED)));
        let list = Request::ListFolder { path: Some(place.path("home")) };
        let Ok(Some(Response::FolderEntries { entries, .. })) = super::super::organize::run(&allowed, &list) else {
            panic!("listed")
        };
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, [".claude", "Documents"], "the locked files left out");
    }

    /// Issue #304 (plan): a locked file that is a symbolic link stays locked as a link too, not only where it
    /// points.
    #[cfg(unix)]
    #[test]
    fn a_locked_link_is_locked_itself() {
        let place = Place::new();
        let home = place.dir.path().join("home");
        fs::write(home.join("dotfile"), b"{}").expect("written");
        std::os::unix::fs::symlink(home.join("dotfile"), home.join(".claude.json")).expect("linked");
        let folders = [AllowedFolder { path: place.path("home"), access: FolderAccess::ReadWrite }];
        let allowed = Allowed::of(&folders, &[home.join(".claude.json")]);
        let link = place.path("home/.claude.json");
        assert_eq!(allowed.entry(&link, FolderAccess::ReadWrite), Err(FolderError::NotFound));
        assert_eq!(allowed.existing(&place.path("home/dotfile"), FolderAccess::Read), Err(FolderError::NotFound));
    }

    #[test]
    fn the_key_stores_are_below_home() {
        let home = Path::new("/home/anna");
        let stores = key_stores(home);
        assert!(stores.contains(&home.join(".ssh")) && stores.contains(&home.join(".gnupg")));
        #[cfg(target_os = "macos")]
        assert!(stores.contains(&home.join("Library/Keychains")));
        #[cfg(target_os = "linux")]
        assert!(stores.contains(&home.join(".local/share/keyrings")));
        for store in [".aws", ".config/gcloud", ".config/gh", ".kube", ".docker/config.json", ".git-credentials"] {
            assert!(stores.contains(&home.join(store)), "{store}");
        }
        assert!(stores.contains(&home.join(".cargo").join("credentials.toml")));
        #[cfg(target_os = "macos")]
        for profile in
            ["Library/Application Support/Google/Chrome", "Library/Application Support/Firefox", "Library/Safari"]
        {
            assert!(stores.contains(&home.join(profile)), "{profile}");
        }
        #[cfg(target_os = "linux")]
        for profile in [".mozilla", ".config/google-chrome", ".local/share/kwalletd"] {
            assert!(stores.contains(&home.join(profile)), "{profile}");
        }
    }

    /// API review of issue #304: with the whole home allowed, the stores of credentials beyond the system's key
    /// stores (here `~/.aws`) are not found, what they hold neither, and listing leaves them out.
    #[test]
    fn more_credential_stores_are_locked_in_an_allowed_home() {
        let place = Place::new();
        let home = place.dir.path().join("home");
        fs::create_dir_all(home.join(".aws")).expect("made");
        fs::write(home.join(".aws/credentials"), b"[default]").expect("written");
        fs::write(home.join(".netrc"), b"machine x").expect("written");
        let folders = [AllowedFolder { path: place.path("home"), access: FolderAccess::ReadWrite }];
        let allowed = Allowed::of(&folders, &key_stores(&home));
        for locked in ["home/.aws", "home/.aws/credentials", "home/.netrc"] {
            let path = place.path(locked);
            assert_eq!(allowed.existing(&path, FolderAccess::Read), Err(FolderError::NotFound), "{locked}");
            assert_eq!(allowed.entry(&path, FolderAccess::Read), Err(FolderError::NotFound), "{locked}");
            assert_eq!(code(&allowed.entry(&path, FolderAccess::Read).expect_err("locked")), "not-found");
        }
        let list = Request::ListFolder { path: Some(place.path("home")) };
        let Ok(Some(Response::FolderEntries { entries, .. })) = super::super::organize::run(&allowed, &list) else {
            panic!("listed")
        };
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["Documents"], "the stores left out");
    }

    /// API review of issue #304: a locked path the system does not let the core into answers `not-found` like any
    /// locked path, never the `error` that would tell it is there; one that is not locked still says the system
    /// refused.
    #[cfg(unix)]
    #[test]
    fn a_locked_path_the_system_refuses_is_not_found() {
        use std::os::unix::fs::PermissionsExt;
        let place = Place::new();
        let home = place.dir.path().join("home");
        for folder in ["Vault/inner", "Closed/inner"] {
            fs::create_dir_all(home.join(folder)).expect("made");
        }
        let closed = |folder: &str, mode: u32| {
            fs::set_permissions(home.join(folder), fs::Permissions::from_mode(mode)).expect("permissions set");
        };
        closed("Vault", 0o000);
        closed("Closed", 0o000);
        // Run as root, the system lets the core in anyway: nothing to see then.
        let refused = fs::read_dir(home.join("Closed")).is_err();
        let folders = [AllowedFolder { path: place.path("home"), access: FolderAccess::ReadWrite }];
        let allowed = Allowed::of(&folders, &[home.join("Vault")]);
        let results = |folder: &str| {
            let path = place.path(&format!("home/{folder}/inner"));
            [
                allowed.existing(&path, FolderAccess::Read),
                allowed.entry(&path, FolderAccess::Read),
                allowed.new_target(&format!("{path}/new"), FolderAccess::ReadWrite),
            ]
        };
        let (vault, open) = (results("Vault"), results("Closed"));
        closed("Vault", 0o755);
        closed("Closed", 0o755);
        for result in vault {
            assert_eq!(result, Err(FolderError::NotFound));
        }
        if refused {
            for result in open {
                assert_eq!(result, Err(FolderError::Denied));
            }
        }
    }

    #[test]
    fn every_error_has_its_code_and_names_no_path() {
        let cases = [
            (FolderError::Invalid(BAD_PATH), "error"),
            (FolderError::NotFound, "not-found"),
            (FolderError::Forbidden(PROTECTED), "forbidden"),
            (FolderError::Unsupported("Only files can be copied, not folders or links."), "unsupported"),
            (FolderError::Denied, "error"),
            (FolderError::of_io(&io::Error::from(io::ErrorKind::PermissionDenied)), "error"),
            (FolderError::of_io(&io::Error::from(io::ErrorKind::NotFound)), "not-found"),
            (FolderError::of_io(&io::Error::from(io::ErrorKind::StorageFull)), "error"),
        ];
        for (err, expected) in cases {
            assert_eq!(code(&err), expected, "{err:?}");
        }
    }
}
