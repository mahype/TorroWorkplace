//! The commands of the tool `file_contents` (issue #705, "Werkzeug Dateiinhalte" in `docs/assistent.md`): read a text
//! file page by page, search in what text files hold, write a new file and change one by find and replace – in the
//! folders a person allowed and with every lock of the tool `files` ([`super::folders::Allowed`]).
//!
//! - **Text only** (decision 4 of the plan): a regular file of at most [`MAX_TEXT_FILE_BYTES`] bytes, UTF-8 without a
//!   NUL byte; anything else is `unsupported`. PDF and office files stay with the document tools (#303).
//! - **Pages** ([`read`]): whole lines up to [`MAX_PAGE_CHARS`] characters, as answers to the model are cut at
//!   16,000; only a single longer line is cut. The answer carries the file's `version`, the SHA-256 of its bytes.
//! - **Never replace when writing** ([`write`]): a taken name gets a free one, as the tool `files` does
//!   (`super::organize::free_names`), made with `create_new`.
//! - **Change only what was read** ([`edit`]): the request names the `version` it read; another one is `conflict`.
//!   The change is planned first ([`plan`]) – the new text and its difference to the old for the approval's card
//!   ([`Plan::diff`]) – and the approval is pinned to the path, the entry and the version ([`Plan::fingerprint`]).
//!   The run plans again on the blocking thread that then writes, under the tools' lock, and writes only while the
//!   fingerprint is the approved one. The new text goes to a temporary file in the same folder, which then replaces the
//!   old one (`fs::rename`), so a reader never sees half of it. A hard link to the old file keeps the old text.
//! - **Search** ([`search`]): within the limits of `tools.find_files` (depth, hits, entries looked at) and a budget of
//!   bytes read ([`SEARCH_BYTES`]); locked, binary and too big files are passed over without a word.
//!
//! Everything runs on a blocking thread ([`Tools::plan_contents`], [`Tools::contents`]). Nothing here logs a path, a
//! text searched for or what a file holds.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use sha2::{Digest, Sha256};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{AllowedFolder, FolderAccess};
use torro_ipc::Request;
use torro_ipc::Response;
use torro_ipc::tools::{
    FileMatch, FileText, MAX_FIND_DEPTH, MAX_FOUND, MAX_MATCH_CHARS, MAX_PAGE_CHARS, MAX_TEXT_FILE_BYTES, MAX_VISITED,
    MAX_WRITE_BYTES,
};

use super::Tools;
use super::folders::{Allowed, Failure, FolderError};
use super::organize::{EntryId, free_names};
use crate::approvals::PathFacts;

const NOT_A_FOLDER: &str = "Give a folder here, not a file.";
const NOT_TEXT: &str = "Only text files can be read or changed: UTF-8, without binary data.";
const TOO_BIG: &str = "The file is too big to be read or changed here (more than 10 MiB).";
const NOT_A_FILE: &str = "Only files can be read or changed, not folders or links.";
const BAD_TEXT: &str = "Give a text of 1 to 200 characters to look for, in one line.";
const LONG_WRITE: &str = "The text is too long for one file (more than 1 MiB).";
const EMPTY_FIND: &str = "Give the text to replace; it cannot be empty.";
const NOT_IN_FILE: &str = "The text to replace is not in the file.";
const ALL_TAKEN: &str = "Every free name for it is taken.";
const CHANGED: &str = "The file changed since it was read; read it again, nothing was changed.";
/// A request that is no command of `file_contents` (the core never sends one here).
const NOT_CONTENTS: &str = "This command is not supported.";

/// At most this many bytes `tools.search_in_files` reads in all (256 MiB), so a folder of big files cannot keep the
/// core busy; beyond them it says `truncated`.
pub const SEARCH_BYTES: u64 = 256 * 1024 * 1024;

/// At most this many lines of difference the approval's card shows ([`Plan::diff`]); a line then says how many more.
pub const MAX_DIFF_LINES: usize = 60;

/// A byte order mark at the start of a UTF-8 file: left out when the file is read, kept when it is changed.
const BOM: &str = "\u{feff}";

/// Why a command of `file_contents` cannot run: a refusal of the bounds of `files` ([`FolderError`]) or one of its own;
/// [`ContentError::response`] is its answer, without a path or text from the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentError {
    /// The bounds of the tool `files`: outside, locked, read only, not a file …
    Folder(FolderError),
    /// `conflict`: the file is not at the version the request read.
    Changed,
    /// `not-found`: the text to replace is not in the file.
    NotInFile,
    /// `error`: the text to replace is in the file this many times, and the request did not ask for every one.
    Ambiguous(usize),
}

impl ContentError {
    /// The answer to the client: a fixed code and a message in the suite's language.
    pub fn response(&self) -> Response {
        let (code, message) = match self {
            ContentError::Folder(err) => return err.response(),
            ContentError::Changed => ("conflict", tr(CHANGED).to_string()),
            ContentError::NotInFile => ("not-found", tr(NOT_IN_FILE).to_string()),
            ContentError::Ambiguous(count) => (
                "error",
                trf(
                    "The text to replace is in the file {n} times; give more of it, or replace every one.",
                    &[("n", &count.to_string())],
                ),
            ),
        };
        Response::Error { code: code.into(), message }
    }
}

impl From<FolderError> for ContentError {
    fn from(err: FolderError) -> ContentError {
        ContentError::Folder(err)
    }
}

/// What a command of `file_contents` that changes something will do, resolved as it will run ([`plan`]): for the
/// approval's card and to pin the approval.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// The file changed or the new file before a free name is picked, as the card shows it.
    pub paths: PathFacts,
    /// For a change: the lines it changes, old ones with `- `, new ones with `+ `, a line of context with two spaces
    /// before and after, at most [`MAX_DIFF_LINES`]. Empty for a new file.
    pub diff: Vec<String>,
    /// What the approval is pinned to: the path, the entry (or the folder of a new file), the version and what is
    /// replaced (or written), hashed. Never shown.
    pub fingerprint: String,
}

/// A text file as it is now: what it holds without a byte order mark, whether it had one, and its version.
struct TextFile {
    text: String,
    bom: bool,
    version: String,
}

/// A command planned, with what it runs on.
enum Planned {
    /// `tools.read_file` of this file from this line on.
    Read(PathBuf, u32),
    /// `tools.search_in_files` in this folder for this text.
    Search(PathBuf, String),
    /// `tools.write_file` of this text to this new path.
    Write(PathBuf, String),
    /// `tools.edit_file` of this file: the new text, the file as it was read and the plan.
    Edit { file: PathBuf, new_text: String, old: TextFile, plan: Plan },
}

impl Planned {
    /// Whether running it changes something (and so takes the tools' lock).
    fn changes(request: &Request) -> bool {
        matches!(request, Request::WriteFile { .. } | Request::EditFile { .. })
    }
}

/// `request` resolved within `allowed` as it will run, with every check; for a change the new text and its
/// difference ([`Plan::diff`]). `Ok(None)` for a request that changes nothing or is no command of `file_contents`.
/// Changes nothing.
pub fn plan(allowed: &Allowed, request: &Request) -> Result<Option<Plan>, ContentError> {
    Ok(match planned(allowed, request)? {
        Some(Planned::Edit { plan, .. }) => Some(plan),
        Some(Planned::Write(target, text)) => Some(write_plan(&target, &text)),
        _ => None,
    })
}

/// Runs `request` within `allowed` as [`plan`] resolves it: [`Response::FileText`] for reading,
/// [`Response::FileMatches`] for searching, [`Response::FilePath`] for a new file, [`Response::FileEdited`] for a
/// change. With `approved`, a change runs only when the plan made now has that fingerprint, else `conflict`.
/// `Ok(None)` for a request that is no command of `file_contents`.
pub fn run(allowed: &Allowed, request: &Request, approved: Option<&str>) -> Result<Option<Response>, ContentError> {
    let Some(planned) = planned(allowed, request)? else { return Ok(None) };
    if let Some(approved) = approved {
        let now = match &planned {
            Planned::Edit { plan, .. } => Some(plan.fingerprint.clone()),
            Planned::Write(target, text) => Some(write_plan(target, text).fingerprint),
            Planned::Read(..) | Planned::Search(..) => None,
        };
        if now.is_some_and(|now| now != approved) {
            return Ok(Some(conflict()));
        }
    }
    let response = match planned {
        Planned::Read(file, from_line) => Response::FileText { file: read(&file, from_line)? },
        Planned::Search(folder, text) => {
            let (matches, truncated) = search(allowed, &folder, &text)?;
            Response::FileMatches { matches, truncated, untrusted: true }
        }
        Planned::Write(target, text) => {
            Response::FilePath { path: write(allowed, &target, &text)?.to_string_lossy().into_owned() }
        }
        Planned::Edit { file, new_text, old, .. } => {
            let version = replace_file(allowed, &file, &new_text, old.bom)?;
            Response::FileEdited { path: file.to_string_lossy().into_owned(), version }
        }
    };
    Ok(Some(response))
}

fn planned(allowed: &Allowed, request: &Request) -> Result<Option<Planned>, ContentError> {
    let planned = match request {
        Request::ReadFile { path, from_line } => {
            let file = allowed.existing(path, FolderAccess::Read)?;
            if !file.is_file() {
                return Err(FolderError::Unsupported(NOT_A_FILE).into());
            }
            Planned::Read(file, from_line.unwrap_or(1).max(1))
        }
        Request::SearchInFiles { folder, text } => {
            let fits = !text.trim().is_empty()
                && text.chars().count() <= MAX_MATCH_CHARS
                && !text.chars().any(char::is_control);
            if !fits {
                return Err(FolderError::Invalid(BAD_TEXT).into());
            }
            let folder = allowed.existing(folder, FolderAccess::Read)?;
            if !folder.is_dir() {
                return Err(FolderError::Invalid(NOT_A_FOLDER).into());
            }
            Planned::Search(folder, text.clone())
        }
        Request::WriteFile { path, text } => {
            if text.len() > MAX_WRITE_BYTES {
                return Err(FolderError::Invalid(LONG_WRITE).into());
            }
            Planned::Write(allowed.new_target(path, FolderAccess::ReadWrite)?, text.clone())
        }
        Request::EditFile { path, version, find, replace, all } => {
            let file = allowed.entry(path, FolderAccess::ReadWrite)?;
            let meta = fs::symlink_metadata(&file).map_err(|err| FolderError::of_io(&err))?;
            if !meta.is_file() {
                return Err(FolderError::Unsupported(NOT_A_FILE).into());
            }
            let old = text_file(&file)?;
            if old.version != *version {
                return Err(ContentError::Changed);
            }
            if find.is_empty() {
                return Err(FolderError::Invalid(EMPTY_FIND).into());
            }
            let hits: Vec<(usize, usize)> =
                old.text.match_indices(find.as_str()).map(|(at, found)| (at, at + found.len())).collect();
            match hits.len() {
                0 => return Err(ContentError::NotInFile),
                1 => {}
                many if !all => return Err(ContentError::Ambiguous(many)),
                _ => {}
            }
            let new_text = old.text.replace(find.as_str(), replace);
            if new_text.len() as u64 > MAX_TEXT_FILE_BYTES {
                return Err(FolderError::Unsupported(TOO_BIG).into());
            }
            let entry = EntryId::of(&file).map(|id| format!("{id:?}")).unwrap_or_default();
            let paths = PathFacts { from: Some(file.to_string_lossy().into_owned()), to: None };
            let what = crate::audit::hashed(&format!("{find}\0{replace}\0{all}"));
            let fingerprint =
                crate::audit::hashed(&format!("{}\0{entry}\0{}\0{what}", paths.fingerprint(), old.version));
            let plan = Plan { paths, diff: diff(&old.text, &hits, replace), fingerprint };
            Planned::Edit { file, new_text, old, plan }
        }
        _ => return Ok(None),
    };
    Ok(Some(planned))
}

/// The plan of a new file at `target` with `text`: the path, the folder that holds it as it is now and the text.
fn write_plan(target: &Path, text: &str) -> Plan {
    let folder = target.parent().and_then(EntryId::of).map(|id| format!("{id:?}")).unwrap_or_default();
    let paths = PathFacts { from: None, to: Some(target.to_string_lossy().into_owned()) };
    let fingerprint =
        crate::audit::hashed(&format!("{}\0{folder}\0{}", paths.fingerprint(), crate::audit::hashed(text)));
    Plan { paths, diff: Vec::new(), fingerprint }
}

/// `conflict`: the file of an approved change is not the one the person was shown any more.
fn conflict() -> Response {
    let message = tr("The file changed after the approval was asked for; nothing was changed.").into();
    Response::Error { code: "conflict".into(), message }
}

/// The text file at `file` (already resolved): `unsupported` when it is too big, holds a NUL byte or is no UTF-8.
fn text_file(file: &Path) -> Result<TextFile, FolderError> {
    let failed = |err: io::Error| FolderError::of_io(&err);
    let opened = fs::File::open(file).map_err(failed)?;
    let meta = opened.metadata().map_err(failed)?;
    if !meta.is_file() || !same_file(&meta, file) {
        return Err(FolderError::Unsupported(NOT_A_FILE));
    }
    if meta.len() > MAX_TEXT_FILE_BYTES {
        return Err(FolderError::Unsupported(TOO_BIG));
    }
    let mut bytes = Vec::new();
    // One byte more than allowed: a file that grew since its size was read is still refused.
    opened.take(MAX_TEXT_FILE_BYTES + 1).read_to_end(&mut bytes).map_err(failed)?;
    if bytes.len() as u64 > MAX_TEXT_FILE_BYTES {
        return Err(FolderError::Unsupported(TOO_BIG));
    }
    let version = version_of(&bytes);
    let text = as_text(bytes).ok_or(FolderError::Unsupported(NOT_TEXT))?;
    let (text, bom) = match text.strip_prefix(BOM) {
        Some(rest) => (rest.to_string(), true),
        None => (text, false),
    };
    Ok(TextFile { text, bom, version })
}

/// Whether the file opened with `opened` is what is at `path` itself, without following a link: on Unix the same drive
/// and number. `File::open` follows a link, so an entry exchanged for a link (to something locked) between the check of
/// the bounds and the opening would be read otherwise. `path` of `tools.read_file` is resolved whole, so it is never a
/// link; for the search and a change it is the entry itself. Elsewhere only that `path` is no link.
fn same_file(opened: &fs::Metadata, path: &Path) -> bool {
    let Ok(at) = fs::symlink_metadata(path) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        at.dev() == opened.dev() && at.ino() == opened.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = opened;
        !at.file_type().is_symlink()
    }
}

/// `bytes` as text: UTF-8 without a NUL byte, which no text file holds.
fn as_text(bytes: Vec<u8>) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// The version of a file with `bytes`: their SHA-256, hex.
fn version_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One page of the text file `file` from line `from_line` (counting from 1): whole lines, each with its line break, up
/// to [`MAX_PAGE_CHARS`] characters; a first line longer than that is cut. Past the last line the text is empty.
fn read(file: &Path, from_line: u32) -> Result<FileText, FolderError> {
    let TextFile { text, version, .. } = text_file(file)?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let total = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    let start = usize::try_from(from_line.saturating_sub(1)).unwrap_or(usize::MAX);
    let (mut page, mut chars, mut end) = (String::new(), 0usize, start);
    for line in lines.iter().skip(start) {
        let count = line.chars().count();
        if chars + count > MAX_PAGE_CHARS {
            if end == start {
                // A single line longer than a page: cut, and the next page starts after it.
                page.extend(line.chars().take(MAX_PAGE_CHARS));
                end += 1;
            }
            break;
        }
        page.push_str(line);
        chars += count;
        end += 1;
    }
    let more = end < lines.len();
    Ok(FileText::new(file.to_string_lossy().into_owned(), page, from_line, total, more, version))
}

/// The lines of the text files in `folder` and below that hold `text`, without regard to case, by path and line; the
/// walk of `tools.find_files` (no links followed, locked entries left out, [`MAX_FIND_DEPTH`] levels, [`MAX_VISITED`]
/// entries) with at most [`MAX_FOUND`] hits and [`SEARCH_BYTES`] read. Files that are locked, too big, binary or do not
/// open are passed over; `truncated` when a limit left something out or a folder below did not open.
fn search(allowed: &Allowed, folder: &Path, text: &str) -> Result<(Vec<FileMatch>, bool), FolderError> {
    let needle = text.to_lowercase();
    let first = fs::read_dir(folder).map_err(|err| FolderError::of_io(&err))?;
    let (mut found, mut visited, mut read_bytes, mut truncated) = (Vec::new(), 0usize, 0u64, false);
    // Folders still to look into, opened only when it is their turn (see `find_files` in `organize`).
    let mut stack: Vec<(PathBuf, usize)> = Vec::new();
    let mut next = Some((first, 1usize));
    'search: while let Some((items, depth)) = next.take().or_else(|| open_next(&mut stack, &mut truncated)) {
        for item in items {
            let Ok(item) = item else {
                truncated = true;
                continue;
            };
            visited += 1;
            if visited > MAX_VISITED {
                truncated = true;
                break 'search;
            }
            let path = item.path();
            if allowed.is_locked(&path) {
                continue;
            }
            match item.file_type() {
                Ok(kind) if kind.is_dir() && depth < MAX_FIND_DEPTH => stack.push((path, depth + 1)),
                Ok(kind) if kind.is_dir() => truncated = true,
                Ok(kind) if kind.is_file() => {
                    let Ok(meta) = item.metadata() else { continue };
                    if meta.len() > MAX_TEXT_FILE_BYTES {
                        continue;
                    }
                    if read_bytes + meta.len() > SEARCH_BYTES {
                        truncated = true;
                        break 'search;
                    }
                    read_bytes += meta.len();
                    let Ok(file) = text_file(&path) else { continue };
                    let Some(shown) = path.to_str() else { continue };
                    for (number, line) in file.text.lines().enumerate() {
                        if !line.to_lowercase().contains(&needle) {
                            continue;
                        }
                        if found.len() == MAX_FOUND {
                            truncated = true;
                            break 'search;
                        }
                        let line_number = u32::try_from(number + 1).unwrap_or(u32::MAX);
                        found.push(FileMatch {
                            path: shown.to_string(),
                            line: line_number,
                            text: around(line, &needle),
                        });
                    }
                }
                // A link is never followed, and anything else holds no text.
                Ok(_) => {}
                Err(_) => truncated = true,
            }
        }
    }
    found.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    Ok((found, truncated))
}

/// The next folder of `stack` opened, with how deep its entries are; one that does not open sets `truncated`.
fn open_next(stack: &mut Vec<(PathBuf, usize)>, truncated: &mut bool) -> Option<(fs::ReadDir, usize)> {
    while let Some((folder, depth)) = stack.pop() {
        match fs::read_dir(&folder) {
            Ok(items) => return Some((items, depth)),
            Err(_) => *truncated = true,
        }
    }
    None
}

/// `line` without a carriage return at its end, at most [`MAX_MATCH_CHARS`] characters around where `needle` (in lower
/// case) is found in it.
fn around(line: &str, needle: &str) -> String {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= MAX_MATCH_CHARS {
        return line.to_string();
    }
    // Where the hit starts, counted in characters of the line in lower case: close enough for the few letters whose
    // lower case has another length.
    let lower = line.to_lowercase();
    let at = lower.find(needle).map_or(0, |byte| lower[..byte].chars().count());
    let start = at.saturating_sub(MAX_MATCH_CHARS / 4).min(chars.len() - MAX_MATCH_CHARS);
    chars[start..start + MAX_MATCH_CHARS].iter().collect()
}

/// Writes `text` as a new file at `target` or a free name next to it ([`free_names`]), made with `create_new` so
/// nothing is replaced; answers the file made. One that could not be written whole is removed again, and so is one the
/// system finds under a locked path (`not-found`).
fn write(allowed: &Allowed, target: &Path, text: &str) -> Result<PathBuf, FolderError> {
    let failed = |err: io::Error| FolderError::of_io(&err);
    for candidate in free_names(target, false) {
        let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(failed(err)),
        };
        let written = file.write_all(text.as_bytes()).and_then(|()| file.sync_all());
        drop(file);
        if let Err(err) = written {
            let _ = fs::remove_file(&candidate);
            return Err(failed(err));
        }
        if allowed.hits_a_lock(&candidate) {
            return match fs::remove_file(&candidate) {
                Ok(()) => Err(FolderError::NotFound),
                Err(err) => Err(FolderError::Failed(Failure::of_kind(err.kind()))),
            };
        }
        return Ok(candidate);
    }
    Err(FolderError::Invalid(ALL_TAKEN))
}

/// Replaces the text file `file` with `text` (after a byte order mark when `bom`): written whole to a temporary file in
/// the same folder with the rights of the old one, then renamed over it, so the file is never half written. Answers
/// the new version.
fn replace_file(allowed: &Allowed, file: &Path, text: &str, bom: bool) -> Result<String, FolderError> {
    let failed = |err: io::Error| FolderError::of_io(&err);
    let (Some(folder), Some(name)) = (file.parent(), file.file_name()) else { return Err(FolderError::NotFound) };
    let permissions = fs::metadata(file).map_err(failed)?.permissions();
    let bytes = match bom {
        true => [BOM.as_bytes(), text.as_bytes()].concat(),
        false => text.as_bytes().to_vec(),
    };
    for number in 1..=99u32 {
        let mut temporary_name = std::ffi::OsString::from(".");
        temporary_name.push(name);
        temporary_name.push(format!(".torro-edit-{}-{number}", std::process::id()));
        let temporary = folder.join(temporary_name);
        let mut out = match fs::OpenOptions::new().write(true).create_new(true).open(&temporary) {
            Ok(out) => out,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(failed(err)),
        };
        let written = out
            .write_all(&bytes)
            .and_then(|()| out.sync_all())
            .and_then(|()| fs::set_permissions(&temporary, permissions.clone()));
        drop(out);
        // A temporary name the system finds as a locked path is never renamed over anything.
        let renamed = written.and_then(|()| match allowed.hits_a_lock(&temporary) {
            true => Err(io::Error::from(io::ErrorKind::NotFound)),
            false => fs::rename(&temporary, file),
        });
        if let Err(err) = renamed {
            let _ = fs::remove_file(&temporary);
            return Err(failed(err));
        }
        return Ok(version_of(&bytes));
    }
    Err(FolderError::Invalid(ALL_TAKEN))
}

/// Lines touched by hits next to each other ([`diff`]): the first and the last line, and the hits (byte ranges) in them.
type Group = (usize, usize, Vec<(usize, usize)>);

/// The difference `replace` at `hits` (byte ranges in `old`, in order, not overlapping) makes, for the approval's card:
/// per group of lines touched, a line of context before (`  `), the old lines (`- `), the new ones (`+ `) and a line of
/// context after; groups apart by more than that with `…` between them. At most [`MAX_DIFF_LINES`], then a line with
/// how many more.
pub fn diff(old: &str, hits: &[(usize, usize)], replace: &str) -> Vec<String> {
    // Where each line starts, and one past the end.
    let mut starts: Vec<usize> = std::iter::once(0).chain(old.match_indices('\n').map(|(at, _)| at + 1)).collect();
    if starts.last() != Some(&old.len()) {
        starts.push(old.len());
    }
    let line_of = |byte: usize| starts.partition_point(|start| *start <= byte).saturating_sub(1);
    let line_text = |line: usize| {
        let (from, to) = (starts[line], starts.get(line + 1).copied().unwrap_or(old.len()));
        old[from..to].trim_end_matches('\n').trim_end_matches('\r').to_string()
    };
    let lines = starts.len() - 1;
    // Groups of whole lines, each with the hits in it.
    let mut groups: Vec<Group> = Vec::new();
    for &(from, to) in hits {
        let (first, last) = (line_of(from), line_of(to.saturating_sub(1).max(from)));
        match groups.last_mut() {
            Some((_, end, inside)) if first <= *end + 1 => {
                *end = (*end).max(last);
                inside.push((from, to));
            }
            _ => groups.push((first, last, vec![(from, to)])),
        }
    }
    let mut shown: Vec<String> = Vec::new();
    let mut previous_end: Option<usize> = None;
    for (first, last, inside) in groups {
        if let Some(end) = previous_end
            && first > end + 2
        {
            shown.push("…".into());
        }
        if first > 0 && previous_end.is_none_or(|end| first > end + 2) {
            shown.push(format!("  {}", line_text(first - 1)));
        }
        let (from, to) = (starts[first], starts.get(last + 1).copied().unwrap_or(old.len()));
        let mut new = String::new();
        let mut at = from;
        for (hit_from, hit_to) in inside {
            new.push_str(&old[at..hit_from]);
            new.push_str(replace);
            at = hit_to;
        }
        new.push_str(&old[at..to]);
        for line in first..=last.min(lines.saturating_sub(1)) {
            shown.push(format!("- {}", line_text(line)));
        }
        if !new.is_empty() {
            for line in new.split_inclusive('\n') {
                shown.push(format!("+ {}", line.trim_end_matches('\n').trim_end_matches('\r')));
            }
        }
        if last + 1 < lines {
            shown.push(format!("  {}", line_text(last + 1)));
        }
        previous_end = Some(last + 1);
    }
    if shown.len() > MAX_DIFF_LINES {
        let more = shown.len() - MAX_DIFF_LINES;
        shown.truncate(MAX_DIFF_LINES);
        shown.push(trf("… {n} more lines", &[("n", &more.to_string())]));
    }
    shown
}

impl Tools {
    /// What a command of `file_contents` that changes something will do ([`plan`]), checked within the allowed
    /// `folders` of now and the locked paths, on a blocking thread: for the core before it asks a person to approve
    /// and to pin the approval. `Err` is the refusal to answer; `Ok(None)` for every other request.
    pub async fn plan_contents(&self, request: &Request, folders: &[AllowedFolder]) -> Result<Option<Plan>, Response> {
        let _guard = self.protection().map(|registry| registry.read()).transpose().map_err(|err| err.response())?;
        let (request, folders, excluded) =
            (request.clone(), folders.to_vec(), self.excluded_now().map_err(|err| err.response())?);
        let planned = tokio::task::spawn_blocking(move || plan(&Allowed::of(&folders, &excluded), &request));
        match planned.await {
            Ok(planned) => planned.map_err(|err| err.response()),
            Err(_) => Err(FolderError::Failed(Failure::Other).response()),
        }
    }

    /// Runs a command of `file_contents` ([`run`]) within the allowed `folders` of now, on a blocking thread, planned
    /// and checked again. The rights and the approval are the core's to check before; `unsupported` for a request that
    /// is no command of `file_contents`. `approved` is the fingerprint of the plan a person was shown
    /// ([`Plan::fingerprint`]): a change then runs only while the plan made just before has it, else `conflict`.
    /// Writing and changing hold the tools' lock (shared with the tool `files`) from planning to their end; reading
    /// and searching do not wait.
    pub async fn contents(&self, request: Request, folders: &[AllowedFolder], approved: Option<&str>) -> Response {
        let guard = match self.protection().map(|registry| registry.read()).transpose() {
            Ok(guard) => guard,
            Err(err) => return err.response(),
        };
        let excluded = match self.excluded_now() {
            Ok(paths) => paths,
            Err(err) => return err.response(),
        };
        let folders = folders.to_vec();
        let approved = approved.map(str::to_string);
        let changes = Planned::changes(&request).then(|| std::sync::Arc::clone(&self.changes));
        let ran = tokio::task::spawn_blocking(move || {
            // Moved into the blocking task: cancellation of the async caller must not release it early.
            let _protection = guard;
            let _turn = changes.as_deref().map(|lock| lock.lock().unwrap_or_else(PoisonError::into_inner));
            run(&Allowed::of(&folders, &excluded), &request, approved.as_deref())
        });
        match ran.await {
            Ok(Ok(Some(response))) => response,
            Ok(Ok(None)) => FolderError::Unsupported(NOT_CONTENTS).response(),
            Ok(Err(err)) => err.response(),
            Err(_) => FolderError::Failed(Failure::Other).response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::folders::READ_ONLY;
    use super::super::folders::tests::Place;
    use super::*;

    fn write_raw(place: &Place, relative: &str, bytes: &[u8]) {
        fs::write(place.dir.path().join(relative), bytes).expect("written");
    }

    fn read_page(place: &Place, relative: &str, from_line: Option<u32>) -> Result<FileText, ContentError> {
        let request = Request::ReadFile { path: place.path(relative), from_line };
        match run(&place.allowed(), &request, None)? {
            Some(Response::FileText { file }) => Ok(file),
            other => panic!("expected a page, got {other:?}"),
        }
    }

    fn edit(place: &Place, relative: &str, version: &str, find: &str, replace: &str, all: bool) -> Request {
        let (version, find, replace) = (version.into(), find.into(), replace.into());
        Request::EditFile { path: place.path(relative), version, find, replace, all }
    }

    fn code(err: ContentError) -> String {
        match err.response() {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn a_text_file_is_read_in_pages_of_whole_lines() {
        let place = Place::new();
        let line = format!("{}\n", "a".repeat(999));
        write_raw(&place, "home/Documents/long.md", line.repeat(25).as_bytes());
        let first = read_page(&place, "home/Documents/long.md", None).expect("read");
        assert_eq!((first.from_line, first.total_lines, first.more), (1, 25, true));
        assert_eq!(first.text.chars().count(), 10_000, "ten whole lines");
        assert!(first.untrusted && first.version == version_of(line.repeat(25).as_bytes()));
        let last = read_page(&place, "home/Documents/long.md", Some(21)).expect("read");
        assert_eq!((last.text.lines().count(), last.more), (5, false));
        let past = read_page(&place, "home/Documents/long.md", Some(99)).expect("read");
        assert!(past.text.is_empty() && !past.more);

        // One line longer than a page is cut, the next page starts after it.
        let single = format!("{}\nend\n", "b".repeat(MAX_PAGE_CHARS + 5));
        write_raw(&place, "home/Documents/one.txt", single.as_bytes());
        let cut = read_page(&place, "home/Documents/one.txt", None).expect("read");
        assert_eq!((cut.text.chars().count(), cut.more), (MAX_PAGE_CHARS, true));
        assert_eq!(read_page(&place, "home/Documents/one.txt", Some(2)).expect("read").text, "end\n");

        // A byte order mark is left out of the text, not of the version.
        write_raw(&place, "home/Documents/bom.txt", "\u{feff}Hallo\n".as_bytes());
        let bom = read_page(&place, "home/Documents/bom.txt", None).expect("read");
        assert_eq!(bom.text, "Hallo\n");
        assert_eq!(bom.version, version_of("\u{feff}Hallo\n".as_bytes()));
        // From the read-only folder too.
        write_raw(&place, "home/Documents/Private/note.txt", b"private\n");
        assert_eq!(read_page(&place, "home/Documents/Private/note.txt", None).expect("read").text, "private\n");
    }

    #[test]
    fn binary_big_and_folders_are_refused() {
        let place = Place::new();
        write_raw(&place, "home/Documents/a.bin", b"abc\0def");
        write_raw(&place, "home/Documents/latin1.txt", b"Gr\xfc\xdfe");
        let big = fs::File::create(place.dir.path().join("home/Documents/big.txt")).expect("made");
        big.set_len(MAX_TEXT_FILE_BYTES + 1).expect("sized");
        for refused in ["home/Documents/a.bin", "home/Documents/latin1.txt", "home/Documents/big.txt", "home/Documents"]
        {
            let err = read_page(&place, refused, None).expect_err(refused);
            assert_eq!(code(err), "unsupported", "{refused}");
        }
    }

    #[test]
    fn what_is_locked_or_outside_is_not_found() {
        let place = Place::new();
        let home = place.dir.path().join("home");
        fs::create_dir_all(home.join("Documents/.ssh")).expect("made");
        fs::write(home.join("Documents/.ssh/id_ed25519"), b"KEY").expect("written");
        fs::write(home.join("Documents/a.md.torroworkplace-backup"), b"old").expect("written");
        fs::write(place.dir.path().join("outside/a.md"), b"out").expect("written");
        let allowed = Allowed::of(&place.folders(), &[home.join("Documents/.ssh")]);
        for path in ["home/Documents/.ssh/id_ed25519", "home/Documents/a.md.torroworkplace-backup", "outside/a.md"] {
            let read = Request::ReadFile { path: place.path(path), from_line: None };
            assert_eq!(code(run(&allowed, &read, None).expect_err(path)), "not-found", "{path}");
            let changed = edit(&place, path, "v", "a", "b", false);
            assert_eq!(code(run(&allowed, &changed, None).expect_err(path)), "not-found", "{path}");
        }
        let into = Request::WriteFile { path: place.path("home/Documents/.ssh/config"), text: "x".into() };
        assert_eq!(code(run(&allowed, &into, None).expect_err("locked")), "not-found");
        let search = Request::SearchInFiles { folder: place.path("home/Documents"), text: "key".into() };
        let Ok(Some(Response::FileMatches { matches, .. })) = run(&allowed, &search, None) else { panic!("searched") };
        assert!(matches.is_empty(), "the locked file is passed over: {matches:?}");
    }

    #[test]
    fn searching_finds_lines_without_case_within_its_limits() {
        let place = Place::new();
        fs::create_dir_all(place.dir.path().join("home/Documents/2026")).expect("made");
        write_raw(&place, "home/Documents/2026/list.md", b"eggs\nOat Milk\nbread\nmilk\n");
        write_raw(&place, "home/Documents/Private/milk.txt", b"milk again\r\n");
        write_raw(&place, "home/Documents/milk.bin", b"milk\0");
        let allowed = place.allowed();
        let search = |text: &str| Request::SearchInFiles { folder: place.path("home/Documents"), text: text.into() };
        let Ok(Some(Response::FileMatches { matches, truncated, untrusted })) = run(&allowed, &search("MILK"), None)
        else {
            panic!("searched")
        };
        let found: Vec<(&str, u32, &str)> = matches
            .iter()
            .map(|hit| (Path::new(&hit.path).file_name().and_then(|n| n.to_str()).unwrap_or(""), hit.line, &*hit.text))
            .collect();
        assert_eq!(found, [("list.md", 2, "Oat Milk"), ("list.md", 4, "milk"), ("milk.txt", 1, "milk again")]);
        assert!(!truncated && untrusted);
        for refused in ["", "  ", "a\nb", &"a".repeat(MAX_MATCH_CHARS + 1)] {
            assert_eq!(code(run(&allowed, &search(refused), None).expect_err(refused)), "error", "{refused:?}");
        }
        let in_file = Request::SearchInFiles { folder: place.path("home/Documents/2026/list.md"), text: "a".into() };
        assert_eq!(code(run(&allowed, &in_file, None).expect_err("a file")), "error");

        let many = "hit\n".repeat(MAX_FOUND + 1);
        write_raw(&place, "home/Documents/2026/many.txt", many.as_bytes());
        let hits = Request::SearchInFiles { folder: place.path("home/Documents/2026"), text: "hit".into() };
        let Ok(Some(Response::FileMatches { matches, truncated, .. })) = run(&allowed, &hits, None) else {
            panic!("searched")
        };
        assert_eq!((matches.len(), truncated), (MAX_FOUND, true));
        let long = format!("{}needle{}", "x".repeat(500), "y".repeat(500));
        assert_eq!(around(&long, "needle").chars().count(), MAX_MATCH_CHARS);
        assert!(around(&long, "needle").contains("needle"));
    }

    #[test]
    fn a_new_file_never_replaces_another() {
        let place = Place::new();
        let allowed = place.allowed();
        let docs = fs::canonicalize(place.dir.path().join("home/Documents")).expect("resolved");
        write_raw(&place, "home/Documents/notes.md", b"kept");
        let new = Request::WriteFile { path: place.path("home/Documents/notes.md"), text: "# New\n".into() };
        assert_eq!(plan(&allowed, &new).expect("planned").map(|plan| plan.paths.to.is_some()), Some(true));
        let Ok(Some(Response::FilePath { path })) = run(&allowed, &new, None) else { panic!("written") };
        assert_eq!(PathBuf::from(&path), docs.join("notes-2.md"));
        assert_eq!(fs::read(&path).expect("read"), b"# New\n");
        assert_eq!(fs::read(docs.join("notes.md")).expect("read"), b"kept", "never replaced");

        let in_read = Request::WriteFile { path: place.path("home/Documents/Private/a.md"), text: "x".into() };
        assert_eq!(run(&allowed, &in_read, None).expect_err("read only"), FolderError::Forbidden(READ_ONLY).into());
        let long =
            Request::WriteFile { path: place.path("home/Documents/b.md"), text: "x".repeat(MAX_WRITE_BYTES + 1) };
        assert_eq!(code(run(&allowed, &long, None).expect_err("too long")), "error");
        assert!(!docs.join("b.md").exists());
    }

    #[test]
    fn a_change_needs_the_version_it_read_and_one_clear_hit() {
        let place = Place::new();
        let allowed = place.allowed();
        write_raw(&place, "home/Documents/list.md", "\u{feff}eggs\nmilk\nbread\nmilk\n".as_bytes());
        let version = read_page(&place, "home/Documents/list.md", None).expect("read").version;
        let file = place.dir.path().join("home/Documents/list.md");

        let stale = edit(&place, "home/Documents/list.md", "0000", "eggs", "ham", false);
        assert_eq!(run(&allowed, &stale, None).expect_err("stale"), ContentError::Changed);
        let twice = edit(&place, "home/Documents/list.md", &version, "milk", "oat milk", false);
        assert_eq!(run(&allowed, &twice, None).expect_err("twice"), ContentError::Ambiguous(2));
        let missing = edit(&place, "home/Documents/list.md", &version, "cheese", "x", false);
        assert_eq!(code(run(&allowed, &missing, None).expect_err("missing")), "not-found");
        let empty = edit(&place, "home/Documents/list.md", &version, "", "x", false);
        assert_eq!(code(run(&allowed, &empty, None).expect_err("empty")), "error");
        assert_eq!(fs::read_to_string(&file).expect("read"), "\u{feff}eggs\nmilk\nbread\nmilk\n", "unchanged");

        let every = edit(&place, "home/Documents/list.md", &version, "milk", "oat milk", true);
        let Ok(Some(Response::FileEdited { version: new, .. })) = run(&allowed, &every, None) else { panic!("edited") };
        let now = fs::read(&file).expect("read");
        assert_eq!(String::from_utf8_lossy(&now), "\u{feff}eggs\noat milk\nbread\noat milk\n", "the mark kept");
        assert_eq!(new, version_of(&now));
        // The old version is gone now.
        assert_eq!(run(&allowed, &every, None).expect_err("again"), ContentError::Changed);
        let leftovers = fs::read_dir(place.dir.path().join("home/Documents")).expect("listed").count();
        assert_eq!(leftovers, 2, "no temporary file left: list.md and Private");

        write_raw(&place, "home/Documents/Private/a.md", b"a");
        let version = version_of(b"a");
        let in_read = edit(&place, "home/Documents/Private/a.md", &version, "a", "b", false);
        assert_eq!(run(&allowed, &in_read, None).expect_err("read only"), FolderError::Forbidden(READ_ONLY).into());
    }

    #[cfg(unix)]
    #[test]
    fn a_change_keeps_the_rights_and_never_goes_through_a_link() {
        use std::os::unix::fs::PermissionsExt;
        let place = Place::new();
        let allowed = place.allowed();
        let file = place.dir.path().join("home/Documents/a.md");
        write_raw(&place, "home/Documents/a.md", b"one\n");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).expect("set");
        let changed = edit(&place, "home/Documents/a.md", &version_of(b"one\n"), "one", "two", false);
        assert!(matches!(run(&allowed, &changed, None), Ok(Some(Response::FileEdited { .. }))));
        assert_eq!(fs::metadata(&file).expect("read").permissions().mode() & 0o777, 0o640);

        std::os::unix::fs::symlink(&file, place.dir.path().join("home/Documents/link.md")).expect("linked");
        let through = edit(&place, "home/Documents/link.md", &version_of(b"two\n"), "two", "three", false);
        assert_eq!(code(run(&allowed, &through, None).expect_err("a link")), "unsupported");
        assert_eq!(fs::read(&file).expect("read"), b"two\n");
        // A file exchanged for a link after the bounds were checked is not read through it.
        let link = place.dir.path().join("home/Documents/link.md");
        let opened = fs::File::open(&link).and_then(|opened| opened.metadata()).expect("opened");
        assert!(!same_file(&opened, &link) && same_file(&opened, &file));
        assert!(text_file(&link).is_err());
    }

    /// The approval is pinned to the file as it was read: changed while the person decides, it is `conflict`.
    #[test]
    fn an_approved_change_runs_only_on_the_file_the_card_showed() {
        let place = Place::new();
        let allowed = place.allowed();
        write_raw(&place, "home/Documents/a.md", b"one\ntwo\n");
        let request = edit(&place, "home/Documents/a.md", &version_of(b"one\ntwo\n"), "two", "2", false);
        let planned = plan(&allowed, &request).expect("planned").expect("a change");
        assert_eq!(planned.diff, ["  one", "- two", "+ 2"]);
        let other = edit(&place, "home/Documents/a.md", &version_of(b"one\ntwo\n"), "one", "1", false);
        let other_plan = plan(&allowed, &other).expect("planned").expect("a change");
        assert_ne!(planned.fingerprint, other_plan.fingerprint, "pinned to what is replaced");

        write_raw(&place, "home/Documents/a.md", b"one\ntwo\nthree\n");
        let ran = run(&allowed, &request, Some(&planned.fingerprint));
        assert_eq!(ran.expect_err("changed"), ContentError::Changed);
        assert_eq!(fs::read(place.dir.path().join("home/Documents/a.md")).expect("read"), b"one\ntwo\nthree\n");

        write_raw(&place, "home/Documents/a.md", b"one\ntwo\n");
        let wrong = run(&allowed, &request, Some("not the one"));
        assert!(matches!(wrong, Ok(Some(Response::Error { ref code, .. })) if code == "conflict"), "{wrong:?}");
        let right = plan(&allowed, &request).expect("planned").expect("a change").fingerprint;
        assert!(matches!(run(&allowed, &request, Some(&right)), Ok(Some(Response::FileEdited { .. }))));
    }

    #[test]
    fn the_difference_shows_old_and_new_lines_with_context() {
        let hits = |text: &str, find: &str| -> Vec<(usize, usize)> {
            text.match_indices(find).map(|(at, found)| (at, at + found.len())).collect()
        };
        let text = "a\nb\nc\nd\ne\nf\ng\nh\n";
        assert_eq!(diff(text, &hits(text, "a"), "A"), ["- a", "+ A", "  b"]);
        assert_eq!(diff(text, &hits(text, "d"), "D\nD2"), ["  c", "- d", "+ D", "+ D2", "  e"]);
        // Two hits far apart: two groups with … between; close ones share their context.
        assert_eq!(diff(text, &hits(text, "b"), "B").len(), 4);
        let both: Vec<(usize, usize)> = [hits(text, "b"), hits(text, "g")].concat();
        assert_eq!(diff(text, &both, "X"), ["  a", "- b", "+ X", "  c", "…", "  f", "- g", "+ X", "  h"]);
        // A whole line removed shows no new line; a hit over two lines shows both.
        assert_eq!(diff(text, &hits(text, "c\n"), ""), ["  b", "- c", "  d"]);
        assert_eq!(diff(text, &hits(text, "e\nf"), "ef"), ["  d", "- e", "- f", "+ ef", "  g"]);
        // Without a line break at the end, and at most 60 lines.
        assert_eq!(diff("x", &hits("x", "x"), "y"), ["- x", "+ y"]);
        let many = "z\n".repeat(100);
        let shown = diff(&many, &hits(&many, "z"), "Z");
        assert_eq!(shown.len(), MAX_DIFF_LINES + 1);
        assert_eq!(shown[MAX_DIFF_LINES], trf("… {n} more lines", &[("n", &"140")]));
    }

    #[tokio::test]
    async fn the_tools_run_contents_within_the_folders_of_now() {
        let place = Place::new();
        let tools = Tools::new(Default::default());
        write_raw(&place, "home/Documents/a.md", b"hello\n");
        let read = Request::ReadFile { path: place.path("home/Documents/a.md"), from_line: None };
        assert!(matches!(tools.contents(read.clone(), &place.folders(), None).await, Response::FileText { .. }));
        assert!(matches!(tools.contents(read, &[], None).await, Response::Error { code, .. } if code == "not-found"));
        let other = Request::ListFolder { path: None };
        let refused = tools.contents(other, &place.folders(), None).await;
        assert!(matches!(refused, Response::Error { code, .. } if code == "unsupported"));
        let changed = edit(&place, "home/Documents/a.md", "old", "hello", "bye", false);
        let refused = tools.plan_contents(&changed, &place.folders()).await.expect_err("stale");
        assert!(matches!(refused, Response::Error { code, .. } if code == "conflict"));
    }
}
