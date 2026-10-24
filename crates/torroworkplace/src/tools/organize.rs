//! The commands of the tool `files` (issue #304, tasks WD.2 and WD.3, "Werkzeug Dateien" in `docs/assistent.md`):
//! list a folder, find by name, make a folder, copy a file, move or rename a file or folder, move one to the system's
//! trash – only in the folders a person allowed, as [`super::folders::Allowed`] bounds them. Nothing here reads or
//! changes what a file holds.
//!
//! - **Plan, then run** ([`plan`], [`run`]): every command is first resolved as it will run – which entry, which
//!   target – with every check of the bounds. The core plans before it asks a person to approve, shows the paths of
//!   the plan and pins the approval to them (`Core::ask_approval`); the run plans again and the core answers
//!   `conflict` when the plan changed meanwhile (a link exchanged, a folder moved).
//! - **Never replace** (decision 21): a taken target gets a free name, `Report-2.pdf` up to `-999` as the PDF tool's
//!   files ([`super::place::MAX_NUMBER`]), a folder `Invoices-2`; the answer names the path that came to be. Files
//!   are made with `create_new` and moved with `fs::hard_link`, which never replaces either; a rename (folders,
//!   links, drives without hard links) looks at its target just before, since `fs::rename` replaces one.
//! - **Copy only files** (decision 22): a folder or a symbolic link is `unsupported`. Moving takes a link itself, not
//!   what it points to; a folder or a link only on the same drive.
//! - **Trash, never delete** (decision 23, task WD.3): an entry goes to the system's trash through the tools'
//!   [`super::Trasher`] ([`move_to_trash`]), the entry itself, also a link; the core always asks a person first.
//! - **Limits** ([`torro_ipc::tools`]): listing one level of at most `MAX_LIST_ENTRIES`, finding at most
//!   `MAX_FIND_DEPTH` levels, `MAX_FOUND` hits and `MAX_VISITED` entries, without following links; reaching one says
//!   `truncated`.
//!
//! Everything runs on a blocking thread ([`Tools::plan`], [`Tools::organize`]). Nothing here logs a path or a name.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use torro_core::settings::{AllowedFolder, FolderAccess};
use torro_ipc::Request;
use torro_ipc::Response;
use torro_ipc::tools::{
    EntryKind, FolderEntry, MAX_FIND_DEPTH, MAX_FOUND, MAX_LIST_ENTRIES, MAX_NAME_PATTERN_CHARS, MAX_VISITED,
};

use super::Tools;
use super::folders::{Allowed, FolderError, PROTECTED};
use super::place::MAX_NUMBER;
use crate::approvals::PathFacts;

const NOT_A_FOLDER: &str = "Give a folder here, not a file.";
const BAD_PATTERN: &str = "Give a name of 1 to 200 characters to look for, in one line.";
const ONLY_FILES: &str = "Only files can be copied, not folders or links.";
const SAME_DRIVE: &str = "A folder or a link can only be moved on the same drive.";
const INTO_ITSELF: &str = "A folder cannot be moved into itself.";
const ALL_TAKEN: &str = "Every free name for it is taken.";
/// A request that is no command of `files` (the core never sends one here).
const NOT_FILES: &str = "This command is not supported.";

/// The paths a command of `files` touches, resolved as it will run ([`plan`]): the entry it reads or moves and the
/// target it makes, before a free name is picked. For the approval's card ([`Plan::facts`]) and to pin the approval.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// The folder listed or searched, the file copied, the entry moved or put in the trash; `None` for making a folder
    /// and for listing the allowed folders.
    pub from: Option<PathBuf>,
    /// The folder made, where the copy or the moved entry goes (a free name may follow); `None` for listing, finding
    /// and the trash.
    pub to: Option<PathBuf>,
}

impl Plan {
    /// The paths as the approval's card shows them and its fingerprint takes them ([`PathFacts::fingerprint`]).
    pub fn facts(&self) -> PathFacts {
        let shown = |path: &Option<PathBuf>| path.as_ref().map(|path| path.to_string_lossy().into_owned());
        PathFacts { from: shown(&self.from), to: shown(&self.to) }
    }
}

/// A command planned, with what it runs on.
enum Planned {
    /// `tools.list_folder` without a path: the allowed folders.
    Roots,
    /// `tools.list_folder` of this folder.
    List(PathBuf),
    /// `tools.find_files` in this folder, for this name.
    Find(PathBuf, String),
    /// `tools.create_folder` of this folder.
    Create(PathBuf),
    /// `tools.copy_file` of this file to this target.
    Copy(PathBuf, PathBuf),
    /// `tools.move_file` of this entry to this target.
    Move(PathBuf, PathBuf),
    /// `tools.trash_file` of this entry.
    Trash(PathBuf),
}

impl Planned {
    fn plan(&self) -> Plan {
        let (from, to) = match self {
            Planned::Roots => (None, None),
            Planned::List(folder) | Planned::Find(folder, _) | Planned::Trash(folder) => (Some(folder.clone()), None),
            Planned::Create(target) => (None, Some(target.clone())),
            Planned::Copy(from, to) | Planned::Move(from, to) => (Some(from.clone()), Some(to.clone())),
        };
        Plan { from, to }
    }
}

/// `request` resolved within `allowed` as it will run, with every check: the source of a copy a file in an allowed
/// folder, the target of making, copying and moving and the source of moving and trashing in one with `read_write`
/// access, a moved or trashed entry that is or holds neither an allowed folder nor anything locked, a folder not moved
/// into itself, a name to find of 1 to [`MAX_NAME_PATTERN_CHARS`] characters. `Ok(None)` for every other request.
/// Changes nothing.
pub fn plan(allowed: &Allowed, request: &Request) -> Result<Option<Plan>, FolderError> {
    Ok(planned(allowed, request)?.map(|planned| planned.plan()))
}

/// Runs `request` within `allowed` as [`plan`] resolves it: [`Response::FolderEntries`] for listing and finding,
/// [`Response::FilePath`] with the path that came to be for making, copying and moving. `Ok(None)` for a request
/// that is no command of `files` this runs – `tools.trash_file` needs the system's trash: [`move_to_trash`].
pub fn run(allowed: &Allowed, request: &Request) -> Result<Option<Response>, FolderError> {
    let Some(planned) = planned(allowed, request)? else { return Ok(None) };
    let listed = |(entries, truncated)| Response::FolderEntries { entries, truncated };
    let made = |path: PathBuf| Response::FilePath { path: path.to_string_lossy().into_owned() };
    Ok(Some(match planned {
        Planned::Roots => listed(list_roots(allowed)),
        Planned::List(folder) => listed(list_folder(allowed, &folder)?),
        Planned::Find(folder, name) => listed(find_files(allowed, &folder, &name)?),
        Planned::Create(target) => made(create_folder(&target)?),
        Planned::Copy(from, to) => made(copy_file(&from, &to)?),
        Planned::Move(from, to) => made(move_file(&from, &to)?),
        Planned::Trash(_) => return Ok(None),
    }))
}

/// Runs `tools.trash_file` within `allowed` as [`plan`] resolves it: hands the entry itself (a symbolic link, not what
/// it points to) to `trash`, the tools' [`super::Trasher`], and answers [`Response::Ok`]. The system's refusal is
/// `error` with its kind ([`FolderError::Failed`]), never a path. `Ok(None)` for every other request.
pub fn move_to_trash(
    allowed: &Allowed,
    request: &Request,
    trash: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<Option<Response>, FolderError> {
    let Some(Planned::Trash(entry)) = planned(allowed, request)? else { return Ok(None) };
    trash(&entry).map_err(FolderError::Failed)?;
    Ok(Some(Response::Ok))
}

fn planned(allowed: &Allowed, request: &Request) -> Result<Option<Planned>, FolderError> {
    let planned = match request {
        Request::ListFolder { path: None } => Planned::Roots,
        Request::ListFolder { path: Some(path) } => Planned::List(folder(allowed, path)?),
        Request::FindFiles { folder: path, name } => {
            let fits = !name.trim().is_empty()
                && name.chars().count() <= MAX_NAME_PATTERN_CHARS
                && !name.chars().any(char::is_control);
            if !fits {
                return Err(FolderError::Invalid(BAD_PATTERN));
            }
            Planned::Find(folder(allowed, path)?, name.clone())
        }
        Request::CreateFolder { path } => Planned::Create(allowed.new_target(path, FolderAccess::ReadWrite)?),
        Request::CopyFile { from, to } => {
            let from = allowed.entry(from, FolderAccess::Read)?;
            let meta = fs::symlink_metadata(&from).map_err(|err| FolderError::of_io(&err))?;
            if !meta.is_file() {
                return Err(FolderError::Unsupported(ONLY_FILES));
            }
            let to = target(allowed, to, &from)?;
            Planned::Copy(from, to)
        }
        Request::MoveFile { from, to } => {
            let from = allowed.entry(from, FolderAccess::ReadWrite)?;
            if allowed.holds_protected(&from) {
                return Err(FolderError::Forbidden(PROTECTED));
            }
            let to = target(allowed, to, &from)?;
            let folder = fs::symlink_metadata(&from).is_ok_and(|meta| meta.is_dir());
            if folder && to != from && to.starts_with(&from) {
                return Err(FolderError::Invalid(INTO_ITSELF));
            }
            Planned::Move(from, to)
        }
        // The entry itself, with the same locks as moving (decision 24): never an allowed folder or what holds one,
        // else an agent would reach an inner "Read" folder through an outer one.
        Request::TrashFile { path } => {
            let entry = allowed.entry(path, FolderAccess::ReadWrite)?;
            if allowed.holds_protected(&entry) {
                return Err(FolderError::Forbidden(PROTECTED));
            }
            Planned::Trash(entry)
        }
        _ => return Ok(None),
    };
    Ok(Some(planned))
}

/// The folder `path` to list or search in: existing in an allowed folder, and a folder.
fn folder(allowed: &Allowed, path: &str) -> Result<PathBuf, FolderError> {
    let folder = allowed.existing(path, FolderAccess::Read)?;
    match folder.is_dir() {
        true => Ok(folder),
        false => Err(FolderError::Invalid(NOT_A_FOLDER)),
    }
}

/// Where `from` goes for the target `to` of a copy or a move: into `to` with its own name when `to` is an existing
/// folder, else `to` itself as a new path (a taken one gets a free name when it runs). Needs `read_write`.
fn target(allowed: &Allowed, to: &str, from: &Path) -> Result<PathBuf, FolderError> {
    match allowed.existing(to, FolderAccess::ReadWrite) {
        Ok(folder) if folder.is_dir() => {
            let name = from.file_name().ok_or(FolderError::NotFound)?;
            let inside = folder.join(name);
            allowed.within(&inside, FolderAccess::ReadWrite)?;
            Ok(inside)
        }
        Ok(_) | Err(FolderError::NotFound) => allowed.new_target(to, FolderAccess::ReadWrite),
        Err(err) => Err(err),
    }
}

/// The allowed folders themselves, each with its access; one that is locked is left out.
fn list_roots(allowed: &Allowed) -> (Vec<FolderEntry>, bool) {
    let entries = allowed
        .roots()
        .iter()
        .filter(|(root, _)| !allowed.is_locked(root))
        .filter_map(|(root, access)| Some(FolderEntry { access: Some(*access), ..entry_of(root)? }))
        .collect();
    (entries, false)
}

/// One level of `folder`: folders first, then by name (without regard to case), at most [`MAX_LIST_ENTRIES`];
/// locked entries left out, symbolic links listed as links.
fn list_folder(allowed: &Allowed, folder: &Path) -> Result<(Vec<FolderEntry>, bool), FolderError> {
    let mut found: Vec<(bool, String, PathBuf)> = Vec::new();
    for item in fs::read_dir(folder).map_err(|err| FolderError::of_io(&err))? {
        let Ok(item) = item else { continue };
        let path = item.path();
        let Some(name) = item.file_name().to_str().map(str::to_string) else { continue };
        if allowed.is_locked(&path) {
            continue;
        }
        // `DirEntry::file_type` does not follow a link: a link to a folder sorts with the files.
        let is_folder = item.file_type().is_ok_and(|kind| kind.is_dir());
        found.push((!is_folder, name, path));
    }
    found.sort_by(|a, b| (a.0, a.1.to_lowercase(), &a.1).cmp(&(b.0, b.1.to_lowercase(), &b.1)));
    let truncated = found.len() > MAX_LIST_ENTRIES;
    let entries = found.into_iter().take(MAX_LIST_ENTRIES).filter_map(|(_, _, path)| entry_of(&path)).collect();
    Ok((entries, truncated))
}

/// Files and folders in `folder` and below whose name matches `name` ([`Pattern`]), by path; without following
/// links, leaving locked entries and what is below them out. Stops at [`MAX_FOUND`] hits, [`MAX_VISITED`] entries
/// looked at and [`MAX_FIND_DEPTH`] levels; `truncated` when one of them left something out.
fn find_files(allowed: &Allowed, folder: &Path, name: &str) -> Result<(Vec<FolderEntry>, bool), FolderError> {
    let pattern = Pattern::of(name);
    let first = fs::read_dir(folder).map_err(|err| FolderError::of_io(&err))?;
    let (mut found, mut visited, mut truncated) = (Vec::new(), 0usize, false);
    // Folders still to look into, with how deep their entries are; the first level is 1.
    let mut stack = vec![(first, 1usize)];
    'search: while let Some((items, depth)) = stack.pop() {
        for item in items {
            let Ok(item) = item else { continue };
            visited += 1;
            if visited > MAX_VISITED {
                truncated = true;
                break 'search;
            }
            let path = item.path();
            if allowed.is_locked(&path) {
                continue;
            }
            if item.file_name().to_str().is_some_and(|name| pattern.matches(name)) {
                if found.len() == MAX_FOUND {
                    truncated = true;
                    break 'search;
                }
                found.extend(entry_of(&path));
            }
            if item.file_type().is_ok_and(|kind| kind.is_dir()) {
                match depth < MAX_FIND_DEPTH {
                    // A folder the system does not let in is passed over.
                    true => stack.extend(fs::read_dir(&path).ok().map(|items| (items, depth + 1))),
                    false => truncated = true,
                }
            }
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((found, truncated))
}

/// What `tools.find_files` looks for: without regard to case, with `*` or `?` a pattern for the whole name, else a
/// part of it.
struct Pattern {
    chars: Vec<char>,
    wild: bool,
}

impl Pattern {
    fn of(name: &str) -> Pattern {
        let lower = name.to_lowercase();
        Pattern { wild: lower.contains(['*', '?']), chars: lower.chars().collect() }
    }

    fn matches(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        match self.wild {
            false => name.contains(&self.chars.iter().collect::<String>()),
            true => wildcard(&self.chars, &name.chars().collect::<Vec<_>>()),
        }
    }
}

/// Whether `name` matches `pattern` whole: `*` any characters, `?` one. Goes back only to the last `*`, so it takes
/// time in proportion to both lengths at most squared.
fn wildcard(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some(&'*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == '?' || c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((at, from)) => {
                    p = at + 1;
                    n = from + 1;
                    star = Some((at, from + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p.min(pattern.len())..].iter().all(|c| *c == '*')
}

/// The entry at `path` as the tool answers it, read without following a link; `None` when it is gone or its path is
/// no Unicode (it could not be named in a request anyway).
fn entry_of(path: &Path) -> Option<FolderEntry> {
    let meta = fs::symlink_metadata(path).ok()?;
    let kind = match meta.file_type() {
        kind if kind.is_symlink() => EntryKind::Link,
        kind if kind.is_dir() => EntryKind::Folder,
        _ => EntryKind::File,
    };
    Some(FolderEntry {
        name: path.file_name()?.to_str()?.to_string(),
        path: path.to_str()?.to_string(),
        kind,
        size: (kind == EntryKind::File).then_some(meta.len()),
        modified: meta.modified().ok().map(DateTime::<Utc>::from),
        access: None,
    })
}

/// Makes the folder `target`, or one with a free name next to it ([`free_names`]); answers the folder made.
fn create_folder(target: &Path) -> Result<PathBuf, FolderError> {
    for candidate in free_names(target, true) {
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(FolderError::of_io(&err)),
        }
    }
    Err(FolderError::Invalid(ALL_TAKEN))
}

/// Copies the file `from` to `target` or a free name next to it, made with `create_new` so nothing is replaced,
/// with the rights of `from`; answers the copy. A copy that could not be written whole is removed again.
fn copy_file(from: &Path, target: &Path) -> Result<PathBuf, FolderError> {
    let failed = |err: io::Error| FolderError::of_io(&err);
    let mut source = fs::File::open(from).map_err(failed)?;
    // Opened by its path: still a file, not a link or folder put there since it was planned.
    let meta = source.metadata().map_err(failed)?;
    if !meta.is_file() || fs::symlink_metadata(from).map_err(failed)?.file_type().is_symlink() {
        return Err(FolderError::Unsupported(ONLY_FILES));
    }
    for candidate in free_names(target, false) {
        let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(failed(err)),
        };
        let written = io::copy(&mut source, &mut file)
            .and_then(|_| file.sync_all())
            .and_then(|()| fs::set_permissions(&candidate, meta.permissions()));
        if let Err(err) = written {
            drop(file);
            let _ = fs::remove_file(&candidate);
            return Err(failed(err));
        }
        return Ok(candidate);
    }
    Err(FolderError::Invalid(ALL_TAKEN))
}

/// Moves the entry `from` to `target` or a free name next to it; answers where it went. Moving it where it is
/// changes nothing. A file goes by `fs::hard_link`, which never replaces, and then `from` is removed; where hard links
/// do not work, by a rename to a target looked at just before; to another drive as a copy ([`copy_file`]), then
/// `from` is removed. A folder or a link goes by a rename only, on the same drive.
fn move_file(from: &Path, target: &Path) -> Result<PathBuf, FolderError> {
    if from == target {
        return Ok(from.to_path_buf());
    }
    let meta = fs::symlink_metadata(from).map_err(|err| FolderError::of_io(&err))?;
    if !meta.is_file() {
        return match rename_free(from, target, meta.is_dir()) {
            Err(err) if err.kind() == io::ErrorKind::CrossesDevices => Err(FolderError::Unsupported(SAME_DRIVE)),
            moved => moved.map_err(|err| FolderError::of_io(&err))?.ok_or(FolderError::Invalid(ALL_TAKEN)),
        };
    }
    for candidate in free_names(target, false) {
        match fs::hard_link(from, &candidate) {
            Ok(()) => return remove_source(from, candidate),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) if err.kind() == io::ErrorKind::CrossesDevices => return copy_then_remove(from, target),
            // No hard links on this drive (FAT, some network shares): rename instead.
            Err(_) => {
                return match rename_free(from, target, false) {
                    Err(err) if err.kind() == io::ErrorKind::CrossesDevices => copy_then_remove(from, target),
                    moved => moved.map_err(|err| FolderError::of_io(&err))?.ok_or(FolderError::Invalid(ALL_TAKEN)),
                };
            }
        }
    }
    Err(FolderError::Invalid(ALL_TAKEN))
}

/// Renames `from` to the first free name for `target` ([`free_names`], `whole`: a folder's name), looking at each
/// just before (a rename replaces a file there on every system); `None` when all are taken.
fn rename_free(from: &Path, target: &Path, whole: bool) -> io::Result<Option<PathBuf>> {
    for candidate in free_names(target, whole) {
        if fs::symlink_metadata(&candidate).is_ok() {
            continue;
        }
        fs::rename(from, &candidate)?;
        return Ok(Some(candidate));
    }
    Ok(None)
}

/// Copies the file `from` to another drive ([`copy_file`]) and removes it there; if it cannot be removed, the copy
/// goes again, so the file is never in two places.
fn copy_then_remove(from: &Path, target: &Path) -> Result<PathBuf, FolderError> {
    let copy = copy_file(from, target)?;
    remove_source(from, copy)
}

/// Removes `from` after it reached `made`; if it cannot be removed, `made` goes again.
fn remove_source(from: &Path, made: PathBuf) -> Result<PathBuf, FolderError> {
    match fs::remove_file(from) {
        Ok(()) => Ok(made),
        Err(err) => {
            let _ = fs::remove_file(&made);
            Err(FolderError::of_io(&err))
        }
    }
}

/// `target`, then the same with `-2`, `-3` … up to [`MAX_NUMBER`] before its extension (`Report-2.pdf`), or after
/// the whole name for a folder (`whole`, `Invoices.2026-2`) and a name without extension.
fn free_names(target: &Path, whole: bool) -> impl Iterator<Item = PathBuf> + '_ {
    (1..=MAX_NUMBER).map(move |number| match (number, target.file_name()) {
        (1, _) | (_, None) => target.to_path_buf(),
        (number, Some(name)) => target.with_file_name(numbered(name, number, whole)),
    })
}

fn numbered(name: &OsStr, number: u32, whole: bool) -> OsString {
    let path = Path::new(name);
    let mut numbered = OsString::new();
    match (whole, path.file_stem(), path.extension()) {
        (false, Some(stem), Some(extension)) => {
            numbered.push(stem);
            numbered.push(format!("-{number}."));
            numbered.push(extension);
        }
        _ => {
            numbered.push(name);
            numbered.push(format!("-{number}"));
        }
    }
    numbered
}

impl Tools {
    /// What a command of `files` will touch ([`plan`]), checked within the allowed `folders` of now and the locked
    /// paths of [`super::Roots::excluded`], on a blocking thread: for the core before it asks a person to approve
    /// and to pin the approval. `Err` is the refusal to answer; `Ok(None)` for every other request.
    pub async fn plan(&self, request: &Request, folders: &[AllowedFolder]) -> Result<Option<Plan>, Response> {
        let (request, folders, excluded) = (request.clone(), folders.to_vec(), self.roots.excluded.clone());
        let planned = tokio::task::spawn_blocking(move || plan(&Allowed::of(&folders, &excluded), &request));
        match planned.await {
            Ok(planned) => planned.map_err(|err| err.response()),
            Err(err) => Err(FolderError::Failed(err.to_string()).response()),
        }
    }

    /// Runs a command of `files` ([`run`], [`move_to_trash`] with the tools' [`super::Trasher`]) within the allowed
    /// `folders` of now, on a blocking thread, planned and checked again. The switch, the rights and the approval are
    /// the core's to check before; `unsupported` for a request that is no command of `files`.
    pub async fn organize(&self, request: Request, folders: &[AllowedFolder]) -> Response {
        let (folders, excluded, trash) = (folders.to_vec(), self.roots.excluded.clone(), self.trash.clone());
        let ran = tokio::task::spawn_blocking(move || {
            let allowed = Allowed::of(&folders, &excluded);
            match matches!(request, Request::TrashFile { .. }) {
                true => move_to_trash(&allowed, &request, &*trash),
                false => run(&allowed, &request),
            }
        });
        match ran.await {
            Ok(Ok(Some(response))) => response,
            Ok(Ok(None)) => FolderError::Unsupported(NOT_FILES).response(),
            Ok(Err(err)) => err.response(),
            Err(err) => FolderError::Failed(err.to_string()).response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::folders::READ_ONLY;
    use super::super::folders::tests::Place;
    use super::*;

    fn entries(response: Option<Response>) -> (Vec<FolderEntry>, bool) {
        match response {
            Some(Response::FolderEntries { entries, truncated }) => (entries, truncated),
            other => panic!("expected entries, got {other:?}"),
        }
    }

    fn made(response: Option<Response>) -> PathBuf {
        match response {
            Some(Response::FilePath { path }) => PathBuf::from(path),
            other => panic!("expected a path, got {other:?}"),
        }
    }

    fn names(entries: &[FolderEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    fn write(place: &Place, relative: &str, bytes: &[u8]) {
        fs::write(place.dir.path().join(relative), bytes).expect("written");
    }

    fn resolved(place: &Place, relative: &str) -> PathBuf {
        fs::canonicalize(place.dir.path().join(relative)).expect("resolved")
    }

    #[test]
    fn listing_shows_the_allowed_folders_then_one_level() {
        let place = Place::new();
        let allowed = place.allowed();
        let (roots, truncated) = entries(run(&allowed, &Request::ListFolder { path: None }).expect("listed"));
        assert_eq!(names(&roots), ["Documents", "Private"]);
        assert_eq!((roots[0].access, roots[1].access), (Some(FolderAccess::ReadWrite), Some(FolderAccess::Read)));
        assert!(!truncated && roots[0].kind == EntryKind::Folder);
        assert_eq!(PathBuf::from(&roots[0].path), resolved(&place, "home/Documents"));

        write(&place, "home/Documents/b.txt", b"12345");
        write(&place, "home/Documents/A.txt", b"1");
        write(&place, "home/Documents/x.torroworkplace-backup", b"1");
        fs::create_dir(place.dir.path().join("home/Documents/zeta")).expect("made");
        let list = Request::ListFolder { path: Some(place.path("home/Documents")) };
        let (listed, truncated) = entries(run(&allowed, &list).expect("listed"));
        assert_eq!(names(&listed), ["Private", "zeta", "A.txt", "b.txt"], "folders first, backups left out");
        assert!(!truncated);
        assert_eq!((listed[3].kind, listed[3].size, listed[3].access), (EntryKind::File, Some(5), None));
        assert!(listed[3].modified.is_some() && listed[0].size.is_none());

        let file = Request::ListFolder { path: Some(place.path("home/Documents/b.txt")) };
        assert_eq!(run(&allowed, &file).err(), Some(FolderError::Invalid(NOT_A_FOLDER)));
        let outside = Request::ListFolder { path: Some(place.path("outside")) };
        assert_eq!(run(&allowed, &outside).err(), Some(FolderError::NotFound));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_listed_as_a_link() {
        let place = Place::new();
        std::os::unix::fs::symlink(place.dir.path().join("outside"), place.dir.path().join("home/Documents/out"))
            .expect("linked");
        let list = Request::ListFolder { path: Some(place.path("home/Documents")) };
        let (listed, _) = entries(run(&place.allowed(), &list).expect("listed"));
        let link = listed.iter().find(|entry| entry.name == "out").expect("listed");
        assert_eq!((link.kind, link.size), (EntryKind::Link, None));
    }

    #[test]
    fn listing_stops_at_its_limit() {
        let place = Place::new();
        let folder = place.dir.path().join("home/Documents/many");
        fs::create_dir(&folder).expect("made");
        for n in 0..=MAX_LIST_ENTRIES {
            fs::write(folder.join(format!("{n:04}.txt")), b"").expect("written");
        }
        let list = Request::ListFolder { path: Some(place.path("home/Documents/many")) };
        let (listed, truncated) = entries(run(&place.allowed(), &list).expect("listed"));
        assert_eq!((listed.len(), truncated), (MAX_LIST_ENTRIES, true));
        assert_eq!(listed[0].name, "0000.txt");
    }

    #[test]
    fn finding_matches_parts_and_patterns_without_case() {
        let place = Place::new();
        fs::create_dir_all(place.dir.path().join("home/Documents/2026/Invoices")).expect("made");
        write(&place, "home/Documents/2026/Invoices/Invoice-03.PDF", b"x");
        write(&place, "home/Documents/2026/notes.txt", b"x");
        write(&place, "home/Documents/Private/invoice.pdf", b"x");
        write(&place, "home/Documents/invoice.pdf.torroworkplace-backup", b"x");
        let allowed = place.allowed();
        let find = |name: &str| Request::FindFiles { folder: place.path("home/Documents"), name: name.into() };

        let (found, truncated) = entries(run(&allowed, &find("invoice")).expect("found"));
        assert_eq!(names(&found), ["Invoices", "Invoice-03.PDF", "invoice.pdf"]);
        assert!(!truncated);
        let (found, _) = entries(run(&allowed, &find("*.pdf")).expect("found"));
        assert_eq!(names(&found), ["Invoice-03.PDF", "invoice.pdf"]);
        let (found, _) = entries(run(&allowed, &find("invoice-0?.pdf")).expect("found"));
        assert_eq!(names(&found), ["Invoice-03.PDF"]);
        let (found, _) = entries(run(&allowed, &find("*.txt*")).expect("found"));
        assert_eq!(names(&found), ["notes.txt"]);

        for refused in ["", "  ", &"a".repeat(MAX_NAME_PATTERN_CHARS + 1), "a\nb"] {
            assert_eq!(run(&allowed, &find(refused)).err(), Some(FolderError::Invalid(BAD_PATTERN)), "{refused:?}");
        }
        let outside = Request::FindFiles { folder: place.path("outside"), name: "a".into() };
        assert_eq!(run(&allowed, &outside).err(), Some(FolderError::NotFound));
    }

    #[test]
    fn a_pattern_matches_the_whole_name() {
        let matches = |pattern: &str, name: &str| Pattern::of(pattern).matches(name);
        assert!(matches("*", "anything") && matches("a*b*c", "aXbYc") && matches("a*", "a"));
        assert!(!matches("a*b", "aXbY") && !matches("?", "ab") && !matches("*.pdf", "a.pdf.txt"));
        assert!(matches("**.PDF", "a.pdf") && matches("r?port*", "Report 2026.pdf"));
        assert!(matches("port", "Report") && !matches("port", "Repo"));
    }

    #[test]
    fn finding_stops_at_its_limits() {
        let place = Place::new();
        let allowed = place.allowed();
        let many = place.dir.path().join("home/Documents/many");
        fs::create_dir(&many).expect("made");
        for n in 0..=MAX_FOUND {
            fs::write(many.join(format!("hit {n:03}")), b"").expect("written");
        }
        let find = Request::FindFiles { folder: place.path("home/Documents/many"), name: "hit".into() };
        let (found, truncated) = entries(run(&allowed, &find).expect("found"));
        assert_eq!((found.len(), truncated), (MAX_FOUND, true));

        let mut deep = place.dir.path().join("home/Documents/deep");
        for level in 1..=MAX_FIND_DEPTH + 1 {
            deep = deep.join(format!("level {level}"));
        }
        fs::create_dir_all(&deep).expect("made");
        let find = Request::FindFiles { folder: place.path("home/Documents/deep"), name: "level".into() };
        let (found, truncated) = entries(run(&allowed, &find).expect("found"));
        assert_eq!((found.len(), truncated), (MAX_FIND_DEPTH, true), "{:?}", names(&found));
    }

    #[test]
    fn a_folder_is_made_with_a_free_name() {
        let place = Place::new();
        let allowed = place.allowed();
        let create = Request::CreateFolder { path: place.path("home/Documents/Invoices.2026") };
        let first = made(run(&allowed, &create).expect("made"));
        assert_eq!(first, resolved(&place, "home/Documents").join("Invoices.2026"));
        write(&place, "home/Documents/Invoices.2026/kept.txt", b"kept");
        let second = made(run(&allowed, &create).expect("made"));
        assert_eq!(second, resolved(&place, "home/Documents").join("Invoices.2026-2"), "the whole name numbered");
        assert_eq!(fs::read(first.join("kept.txt")).expect("kept"), b"kept", "the first one as it was");

        let in_read = Request::CreateFolder { path: place.path("home/Documents/Private/New") };
        assert_eq!(run(&allowed, &in_read).err(), Some(FolderError::Forbidden(READ_ONLY)));
        let no_parent = Request::CreateFolder { path: place.path("home/Documents/a/b") };
        assert_eq!(run(&allowed, &no_parent).err(), Some(FolderError::NotFound));
    }

    #[test]
    fn a_file_is_copied_never_over_another_with_its_rights() {
        let place = Place::new();
        let allowed = place.allowed();
        write(&place, "home/Documents/Private/Report.pdf", b"report");
        write(&place, "home/Documents/Report.pdf", b"other");
        let docs = resolved(&place, "home/Documents");
        let copy =
            |to: &str| Request::CopyFile { from: place.path("home/Documents/Private/Report.pdf"), to: to.into() };

        // Into a folder that has a file of that name: a free name, the other one as it was.
        let into = made(run(&allowed, &copy(&place.path("home/Documents"))).expect("copied"));
        assert_eq!(into, docs.join("Report-2.pdf"));
        assert_eq!(fs::read(&into).expect("read"), b"report");
        assert_eq!(fs::read(docs.join("Report.pdf")).expect("read"), b"other", "never replaced");
        // To a new path, and to one that is taken.
        let renamed = made(run(&allowed, &copy(&place.path("home/Documents/Q3.pdf"))).expect("copied"));
        assert_eq!(renamed, docs.join("Q3.pdf"));
        let taken = made(run(&allowed, &copy(&place.path("home/Documents/Q3.pdf"))).expect("copied"));
        assert_eq!(taken, docs.join("Q3-2.pdf"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let source = place.dir.path().join("home/Documents/Private/Report.pdf");
            fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).expect("set");
            let copied = made(run(&allowed, &copy(&place.path("home/Documents/Rights.pdf"))).expect("copied"));
            assert_eq!(fs::metadata(copied).expect("read").permissions().mode() & 0o777, 0o640);
        }

        // Into the read-only folder: forbidden; a folder: unsupported; outside: not found.
        let back = Request::CopyFile {
            from: place.path("home/Documents/Report.pdf"),
            to: place.path("home/Documents/Private"),
        };
        assert_eq!(run(&allowed, &back).err(), Some(FolderError::Forbidden(READ_ONLY)));
        let folder = Request::CopyFile { from: place.path("home/Documents/Private"), to: place.path("home/Documents") };
        assert_eq!(run(&allowed, &folder).err(), Some(FolderError::Unsupported(ONLY_FILES)));
        let out = Request::CopyFile { from: place.path("home/Documents/Report.pdf"), to: place.path("outside") };
        assert_eq!(run(&allowed, &out).err(), Some(FolderError::NotFound));
        assert_eq!(fs::read_dir(place.dir.path().join("outside")).expect("listed").count(), 0, "nothing written");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_not_copied() {
        let place = Place::new();
        write(&place, "home/Documents/a.txt", b"a");
        std::os::unix::fs::symlink(
            place.dir.path().join("home/Documents/a.txt"),
            place.dir.path().join("home/Documents/link"),
        )
        .expect("linked");
        let copy =
            Request::CopyFile { from: place.path("home/Documents/link"), to: place.path("home/Documents/b.txt") };
        assert_eq!(run(&place.allowed(), &copy).err(), Some(FolderError::Unsupported(ONLY_FILES)));
    }

    #[test]
    fn files_and_folders_are_moved_and_renamed_never_over_another() {
        let place = Place::new();
        let allowed = place.allowed();
        let docs = resolved(&place, "home/Documents");
        fs::create_dir(docs.join("Archive")).expect("made");
        write(&place, "home/Documents/a.txt", b"a");
        write(&place, "home/Documents/Archive/a.txt", b"there");

        let into =
            Request::MoveFile { from: place.path("home/Documents/a.txt"), to: place.path("home/Documents/Archive") };
        let moved = made(run(&allowed, &into).expect("moved"));
        assert_eq!(moved, docs.join("Archive/a-2.txt"));
        assert_eq!(fs::read(&moved).expect("read"), b"a");
        assert_eq!(fs::read(docs.join("Archive/a.txt")).expect("read"), b"there", "never replaced");
        assert!(!docs.join("a.txt").exists(), "moved, not copied");

        // A rename in the same folder, a folder moved with what it holds.
        let rename = Request::MoveFile {
            from: place.path("home/Documents/Archive/a-2.txt"),
            to: place.path("home/Documents/Archive/b.txt"),
        };
        assert_eq!(made(run(&allowed, &rename).expect("renamed")), docs.join("Archive/b.txt"));
        fs::create_dir(docs.join("Old")).expect("made");
        let folder =
            Request::MoveFile { from: place.path("home/Documents/Archive"), to: place.path("home/Documents/Old") };
        assert_eq!(made(run(&allowed, &folder).expect("moved")), docs.join("Old/Archive"));
        assert_eq!(fs::read(docs.join("Old/Archive/b.txt")).expect("read"), b"a");
        // A folder into one that has a folder of its name, and into itself.
        fs::create_dir_all(docs.join("New/Old")).expect("made");
        let taken = Request::MoveFile { from: place.path("home/Documents/Old"), to: place.path("home/Documents/New") };
        assert_eq!(made(run(&allowed, &taken).expect("moved")), docs.join("New/Old-2"));
        let itself =
            Request::MoveFile { from: place.path("home/Documents/New"), to: place.path("home/Documents/New/Old") };
        assert_eq!(run(&allowed, &itself).err(), Some(FolderError::Invalid(INTO_ITSELF)));
        // Where it is already: nothing changes.
        let stay = Request::MoveFile { from: place.path("home/Documents/New"), to: place.path("home/Documents") };
        assert_eq!(made(run(&allowed, &stay).expect("kept")), docs.join("New"));
        assert!(docs.join("New/Old-2/Archive/b.txt").exists());
    }

    /// Moving takes `read_write` on both ends, and never what is or holds an allowed folder or something locked.
    #[test]
    fn moving_out_of_read_or_what_holds_a_folder_is_forbidden() {
        let place = Place::new();
        write(&place, "home/Documents/Private/secret.txt", b"s");
        fs::create_dir(place.dir.path().join("home/Documents/Box")).expect("made");
        let allowed = place.allowed();
        let out_of_read = Request::MoveFile {
            from: place.path("home/Documents/Private/secret.txt"),
            to: place.path("home/Documents"),
        };
        assert_eq!(run(&allowed, &out_of_read).err(), Some(FolderError::Forbidden(READ_ONLY)));
        // The inner read-only folder itself, and the allowed folder that holds it.
        let inner =
            Request::MoveFile { from: place.path("home/Documents/Private"), to: place.path("home/Documents/Box") };
        assert_eq!(run(&allowed, &inner).err(), Some(FolderError::Forbidden(READ_ONLY)));
        let mut outer_folders = place.folders();
        outer_folders.insert(0, AllowedFolder { path: place.path("home"), access: FolderAccess::ReadWrite });
        let outer = Allowed::of(&outer_folders, &[]);
        let docs = Request::MoveFile { from: place.path("home/Documents"), to: place.path("home/Moved") };
        assert_eq!(run(&outer, &docs).err(), Some(FolderError::Forbidden(PROTECTED)));
        let around = Request::MoveFile { from: place.path("home"), to: place.path("outside") };
        assert_eq!(run(&outer, &around).err(), Some(FolderError::Forbidden(PROTECTED)), "an allowed folder itself");
        // Into the read-only folder.
        write(&place, "home/Documents/a.txt", b"a");
        let into_read =
            Request::MoveFile { from: place.path("home/Documents/a.txt"), to: place.path("home/Documents/Private") };
        assert_eq!(run(&allowed, &into_read).err(), Some(FolderError::Forbidden(READ_ONLY)));
        assert!(place.dir.path().join("home/Documents/a.txt").exists(), "left where it was");
    }

    /// A folder holding a locked path is not moved; one inside the suite's folders is not found.
    #[test]
    fn moving_what_holds_the_suites_files_is_forbidden() {
        let place = Place::new();
        let home = place.dir.path().join("home");
        fs::create_dir_all(home.join("Library/Application Support/torroworkplace")).expect("made");
        let folders = [AllowedFolder { path: place.path("home"), access: FolderAccess::ReadWrite }];
        let allowed = Allowed::of(&folders, &[home.join("Library/Application Support/torroworkplace")]);
        let library = Request::MoveFile { from: place.path("home/Library"), to: place.path("home/Documents") };
        assert_eq!(run(&allowed, &library).err(), Some(FolderError::Forbidden(PROTECTED)));
        let suite = Request::MoveFile {
            from: place.path("home/Library/Application Support/torroworkplace"),
            to: place.path("home/Documents"),
        };
        assert_eq!(run(&allowed, &suite).err(), Some(FolderError::NotFound));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_moved_itself() {
        let place = Place::new();
        let docs = place.dir.path().join("home/Documents");
        std::os::unix::fs::symlink(place.dir.path().join("outside"), docs.join("out")).expect("linked");
        let moved =
            Request::MoveFile { from: place.path("home/Documents/out"), to: place.path("home/Documents/elsewhere") };
        let path = made(run(&place.allowed(), &moved).expect("moved"));
        assert!(fs::symlink_metadata(&path).expect("there").file_type().is_symlink());
        assert!(place.dir.path().join("outside").is_dir(), "what it points to stays");
    }

    /// What a test's trash was handed, in order; it records and never touches the person's trash.
    struct TrashBin(std::cell::RefCell<Vec<PathBuf>>);

    impl TrashBin {
        fn new() -> TrashBin {
            TrashBin(std::cell::RefCell::new(Vec::new()))
        }

        fn put(&self, request: &Request, allowed: &Allowed) -> Result<Option<Response>, FolderError> {
            move_to_trash(allowed, request, &|path: &Path| {
                self.0.borrow_mut().push(path.to_path_buf());
                Ok(())
            })
        }

        fn taken(&self) -> Vec<PathBuf> {
            self.0.borrow().clone()
        }
    }

    /// Task WD.3: a file or a folder in a `read_write` folder goes to the trash as resolved; nothing else is handed to
    /// it, and `run` leaves the trash to [`move_to_trash`].
    #[test]
    fn a_file_or_folder_goes_to_the_trash() {
        let place = Place::new();
        let allowed = place.allowed();
        let docs = resolved(&place, "home/Documents");
        write(&place, "home/Documents/a.txt", b"a");
        fs::create_dir(docs.join("Box")).expect("made");
        let bin = TrashBin::new();
        let trash = |relative: &str| Request::TrashFile { path: place.path(relative) };

        assert_eq!(bin.put(&trash("home/Documents/a.txt"), &allowed), Ok(Some(Response::Ok)));
        assert_eq!(bin.put(&trash("home/Documents/Box"), &allowed), Ok(Some(Response::Ok)));
        assert_eq!(bin.taken(), [docs.join("a.txt"), docs.join("Box")]);
        assert_eq!(run(&allowed, &trash("home/Documents/a.txt")), Ok(None), "only with the trash");
        let list = Request::ListFolder { path: None };
        assert_eq!(bin.put(&list, &allowed), Ok(None), "no other command");
        assert_eq!(bin.taken().len(), 2);
    }

    /// Task WD.3: in a `read` folder `forbidden`, an allowed folder itself and what holds one `forbidden`, outside,
    /// missing and backups `not-found` – and nothing is handed to the trash.
    #[test]
    fn the_trash_takes_nothing_protected() {
        let place = Place::new();
        write(&place, "home/Documents/Private/secret.txt", b"s");
        write(&place, "home/Documents/a.txt.torroworkplace-backup", b"b");
        let allowed = place.allowed();
        let bin = TrashBin::new();
        let trash = |relative: &str| Request::TrashFile { path: place.path(relative) };
        let read_only = Some(FolderError::Forbidden(READ_ONLY));
        assert_eq!(bin.put(&trash("home/Documents/Private/secret.txt"), &allowed).err(), read_only);
        assert_eq!(bin.put(&trash("home/Documents/Private"), &allowed).err(), read_only, "the inner read folder");
        let protected = Some(FolderError::Forbidden(PROTECTED));
        assert_eq!(bin.put(&trash("home/Documents"), &allowed).err(), protected, "an allowed folder itself");
        let mut outer_folders = place.folders();
        outer_folders.insert(0, AllowedFolder { path: place.path("home"), access: FolderAccess::ReadWrite });
        let outer = Allowed::of(&outer_folders, &[]);
        assert_eq!(bin.put(&trash("home/Documents"), &outer).err(), protected, "what holds an allowed folder");
        for missing in ["outside", "home/Documents/missing.txt", "home/Documents/a.txt.torroworkplace-backup"] {
            assert_eq!(bin.put(&trash(missing), &allowed).err(), Some(FolderError::NotFound), "{missing}");
        }
        assert_eq!(
            bin.put(&trash("home/Documents/../Documents/a.txt"), &allowed).err().map(|err| err.response()),
            Some(FolderError::Invalid("Give a full path of one line, without . or .. parts.").response())
        );
        assert!(bin.taken().is_empty(), "nothing handed to the trash");
    }

    /// Task WD.3: the suite's folders inside an allowed folder are not found, and what holds them is `forbidden`.
    #[test]
    fn the_trash_takes_nothing_of_the_suite() {
        let place = Place::new();
        let home = place.dir.path().join("home");
        let suite = home.join("Library/Application Support/torroworkplace");
        fs::create_dir_all(&suite).expect("made");
        let folders = [AllowedFolder { path: place.path("home"), access: FolderAccess::ReadWrite }];
        let allowed = Allowed::of(&folders, std::slice::from_ref(&suite));
        let bin = TrashBin::new();
        let trash = |relative: &str| Request::TrashFile { path: place.path(relative) };
        let suite_data = trash("home/Library/Application Support/torroworkplace");
        assert_eq!(bin.put(&suite_data, &allowed).err(), Some(FolderError::NotFound));
        let library = trash("home/Library");
        assert_eq!(bin.put(&library, &allowed).err(), Some(FolderError::Forbidden(PROTECTED)));
        assert!(bin.taken().is_empty());
    }

    /// Task WD.3: a symbolic link goes to the trash itself, not what it points to – also one that leads out of the
    /// allowed folders.
    #[cfg(unix)]
    #[test]
    fn a_link_goes_to_the_trash_itself() {
        let place = Place::new();
        let docs = place.dir.path().join("home/Documents");
        write(&place, "home/Documents/a.txt", b"a");
        std::os::unix::fs::symlink(place.dir.path().join("outside"), docs.join("out")).expect("linked");
        std::os::unix::fs::symlink(docs.join("a.txt"), docs.join("to a")).expect("linked");
        let allowed = place.allowed();
        let bin = TrashBin::new();
        for link in ["home/Documents/out", "home/Documents/to a"] {
            assert_eq!(bin.put(&Request::TrashFile { path: place.path(link) }, &allowed), Ok(Some(Response::Ok)));
        }
        let resolved_docs = resolved(&place, "home/Documents");
        assert_eq!(bin.taken(), [resolved_docs.join("out"), resolved_docs.join("to a")]);
    }

    /// Task WD.3: the system's refusal is `error` with its kind, never a path.
    #[test]
    fn a_refusing_trash_is_an_error_of_its_kind() {
        let place = Place::new();
        write(&place, "home/Documents/a.txt", b"a");
        let request = Request::TrashFile { path: place.path("home/Documents/a.txt") };
        let refused = move_to_trash(&place.allowed(), &request, &|_: &Path| Err("permission denied".to_string()));
        assert_eq!(refused, Err(FolderError::Failed("permission denied".into())));
        let Response::Error { code, message } = FolderError::Failed("permission denied".into()).response() else {
            panic!("an error")
        };
        assert!(code == "error" && !message.contains("a.txt"), "{message}");
    }

    #[test]
    fn a_plan_names_what_the_command_touches() {
        let place = Place::new();
        let allowed = place.allowed();
        write(&place, "home/Documents/a.txt", b"a");
        let docs = resolved(&place, "home/Documents");
        let copy =
            Request::CopyFile { from: place.path("home/Documents/a.txt"), to: place.path("home/Documents/Private") };
        assert_eq!(plan(&allowed, &copy).err(), Some(FolderError::Forbidden(READ_ONLY)));
        let copy = Request::CopyFile { from: place.path("home/Documents/a.txt"), to: place.path("home/Documents") };
        let planned = plan(&allowed, &copy).expect("planned").expect("a command of files");
        assert_eq!(planned, Plan { from: Some(docs.join("a.txt")), to: Some(docs.join("a.txt")) });
        let facts = planned.facts();
        assert_eq!(facts.from.as_deref(), Some(docs.join("a.txt").to_string_lossy().as_ref()));
        let trash = Request::TrashFile { path: place.path("home/Documents/a.txt") };
        assert_eq!(plan(&allowed, &trash), Ok(Some(Plan { from: Some(docs.join("a.txt")), to: None })));
        assert_eq!(plan(&allowed, &Request::ListTools), Ok(None));
        assert_eq!(fs::read_dir(&docs).expect("listed").count(), 2, "planning changes nothing");
    }

    #[test]
    fn free_names_number_before_the_extension() {
        let names: Vec<PathBuf> = free_names(Path::new("/d/Report.pdf"), false).take(3).collect();
        assert_eq!(names, [PathBuf::from("/d/Report.pdf"), "/d/Report-2.pdf".into(), "/d/Report-3.pdf".into()]);
        assert_eq!(free_names(Path::new("/d/.bashrc"), false).nth(1), Some(PathBuf::from("/d/.bashrc-2")));
        assert_eq!(free_names(Path::new("/d/a.tar.gz"), false).nth(1), Some(PathBuf::from("/d/a.tar-2.gz")));
        assert_eq!(free_names(Path::new("/d/v1.2"), true).nth(1), Some(PathBuf::from("/d/v1.2-2")));
        assert_eq!(free_names(Path::new("/d/x"), false).count(), MAX_NUMBER as usize);
    }
}
