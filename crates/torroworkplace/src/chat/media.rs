//! Sending files in the chat (stage 4 of `docs/chat.md`): files prepared in the core's memory
//! (`chat.prepare_file`, `chat.upload_file`, `chat.release_files`), and sending them with a message
//! (`chat.send_message` with `files`, `chat.get_upload`, `chat.cancel_upload`).
//!
//! - **Two steps.** Preparing keeps a file in memory under a lease ([`Store`]): its bytes (or, for a
//!   file a person chose on the computer, its path with size and time of change), its kind read from
//!   its first bytes ([`sniff`]) and a small preview of a picture. Nothing goes to the server until
//!   `chat.send_message` runs, after its approval: then the core uploads into the person's Talk
//!   folder (one `PUT`, or in pieces above the server's limit), shares each file into the
//!   conversation and deletes an upload whose sharing failed ([`ChatService::send_files`]).
//! - **Leases.** Unfinished uploads end after [`UPLOAD_OPEN_FOR`], prepared files [`PREPARED_FOR`]
//!   after their last use; all of them share [`HELD_MAX`] bytes (`busy` beyond). Only their owner
//!   (the client, or the app key) reaches them; another's are `not-found`.
//! - **Drossel.** A client sends at most [`FILES_PER_HOUR`] files and [`BYTES_PER_HOUR`] an hour.
//! - **Nothing in logs:** no name, path or byte; `Debug` of what holds them says only sizes.

use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};
use torro_chat::{ChatError, message_id};
use torro_core::i18n::{tr, trf};
use torro_ipc::Response;
use torro_ipc::chat::{
    FileData, MAX_APP_FILE_BYTES, MAX_FILES, MAX_OPEN_UPLOADS, MAX_UPLOAD_BYTES, PreparedFile, UPLOAD_PIECE_BYTES,
    UploadProgress, UploadState,
};

use super::ChatService;
use super::remote::{Caps, DavPath, FileError, Remote, ShareMeta, Uploaded};
use super::service::{Caller, error_response};

/// The most bytes prepared files and unfinished uploads hold together.
pub const HELD_MAX: u64 = 100 * 1024 * 1024;

/// The most bytes one client holds of [`HELD_MAX`].
pub const CLIENT_HELD_MAX: u64 = 50 * 1024 * 1024;

/// How long an unfinished upload waits for its next piece.
pub const UPLOAD_OPEN_FOR: Duration = Duration::from_secs(10 * 60);

/// How long a prepared file stays after its last use.
pub const PREPARED_FOR: Duration = Duration::from_secs(60 * 60);

/// How long the state of a sending stays readable after it ended.
pub const SENDING_KEPT: Duration = Duration::from_secs(60 * 60);

/// The most files a client sends in an hour.
pub const FILES_PER_HOUR: usize = 20;

/// The most bytes of files a client sends in an hour.
pub const BYTES_PER_HOUR: u64 = 100 * 1024 * 1024;

const HOUR: Duration = Duration::from_secs(60 * 60);

/// The longer side of the preview of a prepared picture.
const PREVIEW_EDGE: u32 = 256;

/// The most pixels of a picture the core draws a preview of.
const PREVIEW_PIXELS: u64 = 32_000_000;

/// The most bytes of a picture read for its preview.
const PREVIEW_SOURCE_BYTES: u64 = 25 * 1024 * 1024;

/// The longest file name, in bytes, as it goes to the server.
const NAME_MAX: usize = 120;

/// Where a file to prepare comes from, as the core resolved it from the request.
#[derive(Clone, PartialEq, Eq)]
pub enum Source {
    /// A file a person chose on the computer.
    Path(PathBuf),
    /// A file of a tool's register (`tools.create_pdf`), checked by the core: its register id, where
    /// it lies, its title and its SHA-256 as the register keeps them.
    Tool { register: String, path: PathBuf, title: String, sha256: String },
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Path(_) => f.write_str("Source::Path(…)"),
            Source::Tool { register, .. } => write!(f, "Source::Tool({register})"),
        }
    }
}

/// Where a prepared file came from, for the approval ("uploaded by this agent").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A person chose it on the computer.
    Path,
    /// Uploaded in pieces (`chat.upload_file`).
    Upload,
    /// Made with the PDF tool.
    Tool,
    /// The card of shared items, made by the core from its own store (stage 5, [`super::share`]).
    Card,
}

/// The bytes of a prepared file.
#[derive(Clone)]
enum Body {
    Bytes(Arc<Vec<u8>>),
    /// A file on the computer, read only when it is sent; it must still have this size and time.
    Path {
        path: PathBuf,
        modified: Option<SystemTime>,
    },
}

/// A file prepared for sending. See the module docs.
#[derive(Clone)]
pub struct Prepared {
    pub id: String,
    pub account: String,
    /// The client that prepared it; `None` for the app key.
    pub owner: Option<String>,
    /// Its name, cleaned ([`clean_name`]); foreign text.
    pub name: String,
    /// The name is the uploader's text, not the file's.
    pub named_by_caller: bool,
    pub size: u64,
    /// What it is by its first bytes.
    pub kind: String,
    /// SHA-256 of the bytes, 64 hex digits; empty for a file on the computer.
    pub sha256: String,
    pub origin: Origin,
    /// The register id of a tool's file, for checking it again when it is sent.
    pub register: Option<String>,
    body: Body,
    preview: Option<Vec<u8>>,
    width: Option<u32>,
    height: Option<u32>,
    used: Instant,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared").field("id", &self.id).field("size", &self.size).finish_non_exhaustive()
    }
}

impl Prepared {
    /// The bytes it holds in memory (0 for a file on the computer).
    fn held(&self) -> u64 {
        match &self.body {
            Body::Bytes(bytes) => bytes.len() as u64,
            Body::Path { .. } => 0,
        }
    }

    /// As `chat.prepare_file` answers it.
    pub fn shown(&self) -> PreparedFile {
        PreparedFile {
            id: self.id.clone(),
            name: self.name.clone(),
            size: self.size,
            content_type: self.kind.clone(),
            width: self.width,
            height: self.height,
            preview: self.preview.as_ref().map(|png| FileData(STANDARD.encode(png))),
            untrusted: true,
        }
    }
}

/// An upload of `chat.upload_file` that is not complete yet.
struct Partial {
    account: String,
    owner: Option<String>,
    name: Option<String>,
    bytes: Vec<u8>,
    touched: Instant,
}

/// A sending of files ([`ChatService::send_files`]) as `chat.get_upload` reads it.
#[derive(Debug)]
struct Sending {
    account: String,
    owner: Option<String>,
    progress: UploadProgress,
    cancel: Arc<AtomicBool>,
    ended: Option<Instant>,
}

/// The prepared files, unfinished uploads and sendings of every account, and what clients sent in
/// the last hour. Time comes in from the caller, so the rules are tested without waiting.
#[derive(Default)]
pub struct Store {
    prepared: BTreeMap<String, Prepared>,
    partial: BTreeMap<String, Partial>,
    sendings: BTreeMap<String, Sending>,
    sent: BTreeMap<String, VecDeque<(Instant, u64)>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("prepared", &self.prepared.len())
            .field("partial", &self.partial.len())
            .field("sendings", &self.sendings.len())
            .finish_non_exhaustive()
    }
}

/// An error answer with `code` and the message for people.
pub fn refused(code: &str, message: &str) -> Response {
    Response::Error { code: code.into(), message: message.to_string() }
}

fn not_found() -> Response {
    refused("not-found", tr("No such file."))
}

impl Store {
    /// Drops what ran out at `now`: unfinished uploads, unused prepared files, sendings long over.
    pub fn tidy(&mut self, now: Instant) {
        self.partial.retain(|_, partial| now.duration_since(partial.touched) < UPLOAD_OPEN_FOR);
        self.prepared.retain(|_, prepared| now.duration_since(prepared.used) < PREPARED_FOR);
        self.sendings.retain(|_, sending| sending.ended.is_none_or(|ended| now.duration_since(ended) < SENDING_KEPT));
    }

    /// The bytes held in memory.
    fn held(&self) -> u64 {
        self.prepared.values().map(Prepared::held).sum::<u64>()
            + self.partial.values().map(|partial| partial.bytes.len() as u64).sum::<u64>()
    }

    /// The bytes `owner` holds in memory.
    fn held_by(&self, owner: Option<&str>) -> u64 {
        self.prepared.values().filter(|p| p.owner.as_deref() == owner).map(Prepared::held).sum::<u64>()
            + self.partial.values().filter(|p| p.owner.as_deref() == owner).map(|p| p.bytes.len() as u64).sum::<u64>()
    }

    /// Whether `more` bytes of `owner` still fit: [`HELD_MAX`] for all, and for a client at most
    /// [`CLIENT_HELD_MAX`], so no client fills the memory the person's windows need.
    fn fits(&self, owner: Option<&str>, more: u64) -> bool {
        let all = self.held().saturating_add(more) <= HELD_MAX;
        let own = owner.is_none() || self.held_by(owner).saturating_add(more) <= CLIENT_HELD_MAX;
        all && own
    }

    /// Keeps `prepared`; `busy` when its bytes do not fit.
    fn keep(&mut self, prepared: Prepared) -> Result<(), Response> {
        if !self.fits(prepared.owner.as_deref(), prepared.held()) {
            return Err(error_response(&ChatError::Busy));
        }
        self.prepared.insert(prepared.id.clone(), prepared);
        Ok(())
    }

    /// Takes the piece `bytes` at `offset` of the upload `upload` (a new one without it), and answers
    /// its id and how many bytes it holds now. See `chat.upload_file`.
    #[allow(clippy::too_many_arguments)]
    fn piece(
        &mut self,
        account: &str,
        owner: Option<&str>,
        upload: Option<&str>,
        offset: u64,
        bytes: Vec<u8>,
        name: Option<String>,
        now: Instant,
    ) -> Result<(String, u64), Response> {
        let Some(id) = upload else {
            if offset != 0 {
                return Err(refused("conflict", tr("A new upload starts at offset 0.")));
            }
            let open = self.partial.values().filter(|partial| partial.owner.as_deref() == owner).count();
            if open >= MAX_OPEN_UPLOADS || !self.fits(owner, bytes.len() as u64) {
                return Err(error_response(&ChatError::Busy));
            }
            let id = format!("u{}", super::service::reference());
            let received = bytes.len() as u64;
            let partial =
                Partial { account: account.into(), owner: owner.map(str::to_string), name, bytes, touched: now };
            self.partial.insert(id.clone(), partial);
            return Ok((id, received));
        };
        let fits = self.fits(owner, bytes.len() as u64);
        let partial = self
            .partial
            .get_mut(id)
            .filter(|partial| partial.account == account && partial.owner.as_deref() == owner)
            .ok_or_else(not_found)?;
        let held = partial.bytes.len() as u64;
        let end = offset.saturating_add(bytes.len() as u64);
        if offset < held {
            // The same piece again is fine; another one at a taken offset is not.
            let start = offset as usize;
            let same = end <= held && partial.bytes[start..end as usize] == bytes[..];
            return match same {
                true => Ok((id.to_string(), held)),
                false => Err(refused("conflict", tr("These bytes differ from the ones uploaded before."))),
            };
        }
        if offset > held {
            return Err(refused("conflict", tr("A piece is missing before this offset.")));
        }
        if end > MAX_UPLOAD_BYTES {
            return Err(refused("error", &too_large(MAX_UPLOAD_BYTES)));
        }
        if !fits {
            return Err(error_response(&ChatError::Busy));
        }
        partial.bytes.extend_from_slice(&bytes);
        partial.touched = now;
        Ok((id.to_string(), end))
    }

    /// The complete upload `id` of `owner` in `account`, taken out.
    fn finish(&mut self, id: &str, account: &str, owner: Option<&str>) -> Option<(Option<String>, Vec<u8>)> {
        let partial = self.partial.get(id)?;
        if partial.account != account || partial.owner.as_deref() != owner {
            return None;
        }
        self.partial.remove(id).map(|partial| (partial.name, partial.bytes))
    }

    /// Drops the prepared files and uploads `ids` of `owner` in `account`; others pass over. `conflict`
    /// for one in `waiting` (an approval waits for it), and then nothing is dropped.
    pub fn release(
        &mut self,
        account: &str,
        owner: Option<&str>,
        ids: &[String],
        waiting: &[String],
    ) -> Result<(), Response> {
        let own = |id: &String| {
            self.prepared.get(id).is_some_and(|p| p.account == account && p.owner.as_deref() == owner)
                || self.partial.get(id).is_some_and(|p| p.account == account && p.owner.as_deref() == owner)
        };
        if ids.iter().any(|id| own(id) && waiting.contains(id)) {
            return Err(refused("conflict", tr("An approval still waits for this file.")));
        }
        let mine: Vec<String> = ids.iter().filter(|id| own(id)).cloned().collect();
        for id in mine {
            self.prepared.remove(&id);
            self.partial.remove(&id);
        }
        Ok(())
    }

    /// The prepared files `ids` of `owner` in `account`, in this order, each used now; `not-found`
    /// when one is not there, of another owner or another account.
    pub fn lease(
        &mut self,
        account: &str,
        owner: Option<&str>,
        ids: &[String],
        now: Instant,
    ) -> Result<Vec<Prepared>, Response> {
        let mut found = Vec::new();
        for id in ids {
            let prepared = self
                .prepared
                .get_mut(id)
                .filter(|prepared| prepared.account == account && prepared.owner.as_deref() == owner)
                .ok_or_else(not_found)?;
            prepared.used = now;
            found.push(prepared.clone());
        }
        Ok(found)
    }

    /// The prepared files `ids` as they are, for the approval's facts; unknown ones are left out.
    pub fn peek(&self, ids: &[String]) -> Vec<Prepared> {
        ids.iter().filter_map(|id| self.prepared.get(id).cloned()).collect()
    }

    /// Whether `client` could send `count` files of `bytes` more at `now`, without counting them.
    pub fn would_admit(&mut self, client: &str, count: usize, bytes: u64, now: Instant) -> bool {
        let sent = self.sent.entry(client.to_string()).or_default();
        while sent.front().is_some_and(|(at, _)| now.duration_since(*at) >= HOUR) {
            sent.pop_front();
        }
        let total: u64 = sent.iter().map(|(_, bytes)| bytes).sum();
        sent.len() + count <= FILES_PER_HOUR && total.saturating_add(bytes) <= BYTES_PER_HOUR
    }

    /// Whether `client` may send `count` files of `bytes` more at `now` ([`FILES_PER_HOUR`],
    /// [`BYTES_PER_HOUR`]); counts them when it may.
    pub fn admit(&mut self, client: &str, count: usize, bytes: u64, now: Instant) -> bool {
        if !self.would_admit(client, count, bytes, now) {
            return false;
        }
        let sent = self.sent.entry(client.to_string()).or_default();
        sent.extend(std::iter::repeat_n((now, 0), count.saturating_sub(1)));
        sent.push_back((now, bytes));
        true
    }

    /// Drops everything of `account`; its sendings are cancelled.
    pub fn forget_account(&mut self, account: &str) {
        self.prepared.retain(|_, prepared| prepared.account != account);
        self.partial.retain(|_, partial| partial.account != account);
        for sending in self.sendings.values().filter(|sending| sending.account == account) {
            sending.cancel.store(true, Ordering::Release);
        }
    }
}

/// "This file is larger than {limit}." with the limit as people read it.
fn too_large(limit: u64) -> String {
    trf("This file is larger than {limit}.", &[("limit", &torro_gui::units::format_bytes(limit))])
}

/// What a file is by its first bytes; never by its name or what anyone says. Unknown is
/// `application/octet-stream`.
pub fn sniff(bytes: &[u8]) -> &'static str {
    let starts = |prefix: &[u8]| bytes.starts_with(prefix);
    let at = |offset: usize, part: &[u8]| bytes.get(offset..offset + part.len()) == Some(part);
    if starts(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if starts(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "image/gif"
    } else if starts(b"RIFF") && at(8, b"WEBP") {
        "image/webp"
    } else if starts(b"RIFF") && at(8, b"WAVE") {
        "audio/wav"
    } else if starts(b"RIFF") && at(8, b"AVI ") {
        "video/x-msvideo"
    } else if at(4, b"ftyp") {
        match bytes.get(8..12) {
            Some(b"heic" | b"heix" | b"mif1" | b"msf1") => "image/heic",
            Some(b"avif") => "image/avif",
            Some(b"qt  ") => "video/quicktime",
            Some(b"M4A ") => "audio/mp4",
            _ => "video/mp4",
        }
    } else if starts(&[0x1a, 0x45, 0xdf, 0xa3]) {
        "video/webm"
    } else if starts(b"OggS") {
        "audio/ogg"
    } else if starts(b"fLaC") {
        "audio/flac"
    } else if starts(b"ID3") || starts(&[0xff, 0xfb]) || starts(&[0xff, 0xf3]) {
        "audio/mpeg"
    } else if starts(b"%PDF-") {
        "application/pdf"
    } else if starts(b"PK\x03\x04") {
        "application/zip"
    } else if is_markup(bytes) {
        // SVG and HTML can run scripts and call other servers: never a picture to show.
        "text/html"
    } else if !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

/// Whether `bytes` start like markup (`<svg`, `<?xml`, `<html`, `<!DOCTYPE`), after white space.
fn is_markup(bytes: &[u8]) -> bool {
    let start: Vec<u8> =
        bytes.iter().skip_while(|b| b.is_ascii_whitespace()).take(9).map(u8::to_ascii_lowercase).collect();
    [&b"<svg"[..], b"<?xml", b"<html", b"<!doctype"].iter().any(|tag| start.starts_with(tag))
}

/// Whether a file of the media type `kind` is picture content (stage 4, "Bildinhalte"): a picture,
/// a video, or a kind that cannot be told (a format nobody knows is no way around the right).
pub fn is_picture_content(kind: &str) -> bool {
    let kind = kind.trim().to_ascii_lowercase();
    kind.is_empty() || kind.starts_with("image/") || kind.starts_with("video/") || kind == "application/octet-stream"
}

/// `name` as a file name for the server: its last part after `/` or `\`, one line without control
/// characters, at most [`NAME_MAX`] bytes with the extension kept; `file` when nothing is left.
pub fn clean_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let line: String = last.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    let line = line.trim_matches('.').trim().to_string();
    if line.is_empty() {
        return tr("file").to_string();
    }
    if line.len() <= NAME_MAX {
        return line;
    }
    let (stem, extension) = match line.rfind('.') {
        Some(at) if at > 0 && line.len() - at <= 16 => (&line[..at], &line[at..]),
        _ => (line.as_str(), ""),
    };
    let mut cut = NAME_MAX - extension.len();
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{extension}", &stem[..cut])
}

/// `text` as one segment of a folder name: no `/`, `\` or control character, at most 64 characters.
fn segment(text: &str) -> String {
    let cleaned: String =
        text.chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '-' } else { c }).take(64).collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() { "-".to_string() } else { cleaned }
}

/// A PNG of at most [`PREVIEW_EDGE`] pixels of the picture `bytes`, with its size; `None` for what
/// is no PNG, JPEG, GIF or WebP, or too large to draw.
fn picture_preview(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Gif | image::ImageFormat::WebP)
    ) {
        return None;
    }
    let (width, height) = reader.into_dimensions().ok()?;
    if u64::from(width) * u64::from(height) > PREVIEW_PIXELS {
        return None;
    }
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let picture = reader.decode().ok()?.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE);
    let mut png = Vec::new();
    picture.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some((png, width, height))
}

/// SHA-256 of `bytes` as 64 lowercase hex digits.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Size and time of change of the file at `path`; `None` when it is no file one can read.
fn stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let meta = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file)?;
    Some((meta.len(), meta.modified().ok()))
}

/// The first `limit` bytes of the file at `path`.
fn head(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// What the background work of one sending needs.
struct Job {
    id: String,
    account: String,
    conversation: String,
    token: String,
    user: String,
    remote: Remote,
    caps: Caps,
    folder: DavPath,
    files: Vec<Prepared>,
    text: String,
    reply_to: Option<u64>,
    silent: bool,
    cancel: Arc<AtomicBool>,
}

/// Why sending stopped: an answer's code and message, or cancelled.
enum Stop {
    Failed(Box<Response>),
    Cancelled,
}

impl From<FileError> for Stop {
    fn from(err: FileError) -> Stop {
        Stop::Failed(Box::new(file_refusal(&err)))
    }
}

impl From<ChatError> for Stop {
    fn from(err: ChatError) -> Stop {
        Stop::Failed(Box::new(error_response(&err)))
    }
}

/// The answer for a refused call about a file.
pub fn file_refusal(err: &FileError) -> Response {
    match err {
        FileError::Chat(err) => error_response(err),
        FileError::Exists => refused("conflict", tr("A file of that name is on the server already.")),
        FileError::NoSpace => refused("error", tr("There is not enough space in your Nextcloud.")),
        FileError::TooLarge => refused("error", tr("This file is too large.")),
    }
}

impl ChatService {
    /// The store of files; a lock a panicking thread left is taken over.
    pub(super) fn media(&self) -> std::sync::MutexGuard<'_, Store> {
        let mut store = self.media.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        store.tidy(Instant::now());
        store
    }

    /// `chat.prepare_file` with `source` as the core resolved it: the file read as far as needed,
    /// kept under a lease of `caller`.
    pub async fn prepare(self: &Arc<Self>, account: &str, source: Source, caller: Caller<'_>) -> Response {
        let Ok(remote) = self.remote(account).await else { return error_response(&ChatError::NotFound) };
        let limit = match caller.client {
            Some(_) => MAX_UPLOAD_BYTES,
            // The app key: the free space of the account, when the server tells it, and 512 MiB.
            None => match remote.free_space().await {
                Ok(Some(free)) if free < MAX_APP_FILE_BYTES => free,
                _ => MAX_APP_FILE_BYTES,
            },
        };
        let space_limited = limit < MAX_APP_FILE_BYTES && caller.client.is_none();
        let read = tokio::task::spawn_blocking(move || read_source(source, limit, space_limited)).await;
        let file = match read {
            Ok(Ok(file)) => file,
            Ok(Err(refusal)) => return refusal,
            Err(_) => return refused("error", tr("The file cannot be read.")),
        };
        let prepared = Prepared {
            id: format!("f{}", super::service::reference()),
            account: account.to_string(),
            owner: caller.client.map(str::to_string),
            used: Instant::now(),
            ..file
        };
        let shown = prepared.shown();
        match self.media().keep(prepared) {
            Ok(()) => Response::ChatPreparedFile { file: Box::new(shown) },
            Err(refusal) => refusal,
        }
    }

    /// `chat.upload_file`: one piece; the last one makes the prepared file.
    #[allow(clippy::too_many_arguments)]
    pub async fn upload(
        self: &Arc<Self>,
        account: &str,
        upload: Option<&str>,
        offset: u64,
        base64: &FileData,
        name: Option<String>,
        finish: bool,
        caller: Caller<'_>,
    ) -> Response {
        if self.remote(account).await.is_err() {
            return error_response(&ChatError::NotFound);
        }
        // A piece of 256 KiB is about 350 000 characters of Base64; more is refused before decoding.
        if base64.0.len() > UPLOAD_PIECE_BYTES.div_ceil(3) * 4 {
            return refused("error", tr("A piece is larger than 256 KiB."));
        }
        let Ok(bytes) = STANDARD.decode(base64.0.as_bytes()) else {
            return refused("error", tr("The piece is no Base64."));
        };
        if bytes.len() > UPLOAD_PIECE_BYTES {
            return refused("error", tr("A piece is larger than 256 KiB."));
        }
        let owner = caller.client;
        let now = Instant::now();
        let (id, received) = match self.media().piece(account, owner, upload, offset, bytes, name, now) {
            Ok(done) => done,
            Err(refusal) => return refusal,
        };
        if !finish {
            return Response::ChatFileUpload { upload: id, received };
        }
        let Some((name, bytes)) = self.media().finish(&id, account, owner) else { return not_found() };
        let named = name.is_some();
        let name = clean_name(name.as_deref().unwrap_or_default());
        let made = tokio::task::spawn_blocking(move || of_bytes(bytes, name, Origin::Upload, None)).await;
        let Ok(file) = made else { return refused("error", tr("The file cannot be read.")) };
        let prepared = Prepared {
            id,
            account: account.to_string(),
            owner: owner.map(str::to_string),
            named_by_caller: named,
            used: now,
            ..file
        };
        let shown = prepared.shown();
        match self.media().keep(prepared) {
            Ok(()) => Response::ChatPreparedFile { file: Box::new(shown) },
            Err(refusal) => refusal,
        }
    }

    /// Keeps the card file `bytes` named `name` (stage 5, [`super::share`]) as a prepared file of
    /// `caller` in `account`, so it is sent as any other file; answers its id. `busy` when its bytes
    /// do not fit.
    pub fn prepare_card(
        &self,
        account: &str,
        name: &str,
        bytes: Vec<u8>,
        caller: Caller<'_>,
    ) -> Result<String, Response> {
        let file = of_bytes(bytes, clean_name(name), Origin::Card, None);
        let id = format!("f{}", super::service::reference());
        let prepared = Prepared {
            id: id.clone(),
            account: account.to_string(),
            owner: caller.client.map(str::to_string),
            used: Instant::now(),
            ..file
        };
        self.media().keep(prepared)?;
        Ok(id)
    }

    /// `chat.release_files`: `waiting` are the ids an approval of the caller waits for.
    pub fn release(&self, account: &str, ids: &[String], waiting: &[String], caller: Caller<'_>) -> Response {
        match self.media().release(account, caller.client, ids, waiting) {
            Ok(()) => Response::Ok,
            Err(refusal) => refusal,
        }
    }

    /// The register ids of the tool's files among the prepared files `ids` of `owner`, for the core
    /// to check them again before they are sent.
    pub fn tool_files(&self, ids: &[String], owner: Option<&str>) -> Vec<(String, String)> {
        self.media()
            .peek(ids)
            .into_iter()
            .filter(|prepared| prepared.owner.as_deref() == owner)
            .filter_map(|prepared| Some((prepared.register?, prepared.sha256)))
            .collect()
    }

    /// For a client's request before anyone is asked to approve it: `not-found` for a prepared file
    /// of `chat.send_message` that is not its own in the conversation's account, `error` for too many,
    /// `busy` when its files would go beyond what it may send in an hour (counted when they go).
    pub fn files_refusal(&self, account: &str, ids: &[String], owner: Option<&str>) -> Option<Response> {
        if ids.len() > MAX_FILES {
            return Some(refused("error", tr("A message takes at most 10 files.")));
        }
        let now = Instant::now();
        let mut store = self.media();
        let files = match store.lease(account, owner, ids, now) {
            Ok(files) => files,
            Err(refusal) => return Some(refusal),
        };
        let bytes = files.iter().map(|file| file.size).sum();
        match owner {
            Some(client) if !store.would_admit(client, files.len(), bytes, now) => {
                Some(error_response(&ChatError::Busy))
            }
            _ => None,
        }
    }

    /// `chat.send_message` with `files`: checks them and the server, then uploads and shares them in
    /// the background ([`ChatService::run`]) and answers `chat-sending` with the sending's id.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn send_files(
        self: &Arc<Self>,
        conversation: &str,
        text: &str,
        reply_to: Option<u64>,
        silent: bool,
        ids: &[String],
        caller: Caller<'_>,
    ) -> Result<Response, Response> {
        let (account, token) = super::service::conversation_of(conversation).map_err(|err| error_response(&err))?;
        if let Some(refusal) = self.files_refusal(account, ids, caller.client) {
            return Err(refusal);
        }
        let (remote, caps, user) = self.ready(account).await.map_err(|err| error_response(&err))?;
        if !caps.attachments {
            return Err(error_response(&unsupported("This server does not let you share files.")));
        }
        let length = text.chars().count();
        if length > caps.max_length {
            return Err(refused(
                "error",
                &trf(
                    "The message is too long: {length} of at most {max} characters.",
                    &[("length", &length), ("max", &caps.max_length)],
                ),
            ));
        }
        if silent && !caps.silent {
            return Err(error_response(&unsupported("This server cannot send without notifying.")));
        }
        let room = self.room(account, &token).await.map_err(|err| error_response(&err))?;
        if room.read_only {
            return Err(error_response(&unsupported("This conversation is read-only.")));
        }
        let files = self.media().lease(account, caller.client, ids, Instant::now())?;
        // A file a person chose must not have changed since: what was approved goes out.
        for file in &files {
            if let Body::Path { path, modified } = &file.body
                && stamp(path) != Some((file.size, *modified))
            {
                return Err(refused("conflict", tr("The file changed since it was prepared.")));
            }
        }
        let total: u64 = files.iter().map(|file| file.size).sum();
        if let Some(client) = caller.client
            && !self.media().admit(client, files.len(), total, Instant::now())
        {
            return Err(error_response(&ChatError::Busy));
        }
        let folder = self.folder_for(account, &caps, &room.name, token.as_str(), &user)?;
        let id = format!("s{}", super::service::reference());
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut store = self.media();
            // Taken and checked under one lock: of two sends with the same files at once, only the
            // first gets them; the other finds them gone, as after a send.
            if files.iter().any(|file| !store.prepared.contains_key(&file.id)) {
                return Err(error_response(&ChatError::NotFound));
            }
            for file in &files {
                store.prepared.remove(&file.id);
            }
            let progress = UploadProgress {
                id: id.clone(),
                state: UploadState::Uploading,
                sent: 0,
                total,
                messages: Vec::new(),
                code: None,
                message: None,
            };
            let owner = caller.client.map(str::to_string);
            let sending =
                Sending { account: account.into(), owner, progress, cancel: Arc::clone(&cancel), ended: None };
            store.sendings.insert(id.clone(), sending);
        }
        let job = Job {
            id: id.clone(),
            account: account.to_string(),
            conversation: conversation.to_string(),
            token: token.as_str().to_string(),
            user,
            remote,
            caps,
            folder,
            files,
            text: text.to_string(),
            reply_to,
            silent,
            cancel,
        };
        let service = Arc::clone(self);
        tokio::spawn(async move { service.run(job).await });
        Ok(Response::ChatSending { id })
    }

    /// The folder files of `account` go to: Talk's attachment folder, and with `subfolders` one per
    /// conversation and person below it (`<name>-<token>/<own name>-<user id>`).
    fn folder_for(&self, account: &str, caps: &Caps, room: &str, token: &str, user: &str) -> Result<DavPath, Response> {
        let broken = || refused("error", tr("The server names no usable folder for files."));
        let mut folder = DavPath::parse(&caps.attachments_folder).ok_or_else(broken)?;
        if caps.subfolders {
            let own = self.own_name(account).unwrap_or_else(|| user.to_string());
            folder = folder.join(&segment(&format!("{room}-{token}"))).ok_or_else(broken)?;
            folder = folder.join(&segment(&format!("{own}-{user}"))).ok_or_else(broken)?;
        }
        Ok(folder)
    }

    /// Updates the sending `id` with `change`.
    fn progress(&self, id: &str, change: impl FnOnce(&mut UploadProgress)) {
        if let Some(sending) = self.media().sendings.get_mut(id) {
            change(&mut sending.progress);
        }
    }

    /// Ends the sending `id` as `state`, with the code and message of `refusal` when it failed.
    fn end(&self, id: &str, state: UploadState, refusal: Option<Response>) {
        if let Some(sending) = self.media().sendings.get_mut(id) {
            sending.progress.state = state;
            if let Some(Response::Error { code, message }) = refusal {
                sending.progress.code = Some(code);
                sending.progress.message = Some(message);
            }
            sending.ended = Some(Instant::now());
        }
    }

    /// The background work of one sending: the text first when the server cannot put it under a
    /// file, then each file uploaded and shared; the messages they became at the end.
    async fn run(self: Arc<Self>, job: Job) {
        let mut shared_ids = Vec::new();
        let outcome = self.upload_and_share(&job, &mut shared_ids).await;
        match outcome {
            Ok(()) => {
                let messages = self.messages_of(&job, &shared_ids).await;
                self.progress(&job.id, |progress| progress.messages = messages);
                self.end(&job.id, UploadState::Sent, None);
            }
            Err(Stop::Cancelled) => self.end(&job.id, UploadState::Cancelled, None),
            Err(Stop::Failed(refusal)) => {
                let refusal = *refusal;
                eprintln!("torroworkplace: sending a chat file of {} failed ({})", job.account, code_of(&refusal));
                self.end(&job.id, UploadState::Failed, Some(refusal));
            }
        }
    }

    async fn upload_and_share(&self, job: &Job, shared_ids: &mut Vec<u64>) -> Result<(), Stop> {
        let remote = &job.remote;
        remote.make_folders(&job.user, &job.folder).await?;
        let caption = !job.text.trim().is_empty();
        if caption && !job.caps.caption {
            let sending = super::remote::Sending {
                text: job.text.clone(),
                reply_to: job.reply_to,
                reference: super::service::reference(),
                silent: job.silent,
            };
            remote.send(&job.token, &sending).await?;
        }
        let last = job.files.len().saturating_sub(1);
        for (index, file) in job.files.iter().enumerate() {
            if job.cancel.load(Ordering::Acquire) {
                return Err(Stop::Cancelled);
            }
            let (path, uploaded) = self.upload_one(job, file).await?;
            let etag = uploaded.etag.clone().unwrap_or_default();
            if job.cancel.load(Ordering::Acquire) {
                // Only the file just uploaded, known by its ETag: without one an empty `If-Match`
                // would count as none, and the delete could take a file someone else put there.
                if !etag.is_empty() {
                    let _ = remote.delete_file(&job.user, &path, &etag).await;
                }
                return Err(Stop::Cancelled);
            }
            // From the first share on the state stays `sharing`: a message is out, cancelling is over.
            self.progress(&job.id, |progress| progress.state = UploadState::Sharing);
            // The text goes under the last file, as Talk's own window does.
            let with_text = index == last && job.caps.caption;
            let meta = ShareMeta {
                caption: (with_text && caption).then(|| job.text.clone()),
                silent: job.silent && job.caps.caption,
                reply_to: if with_text { job.reply_to } else { None },
                reference_id: job.caps.reference_ids.then(super::service::reference),
            };
            if let Err(err) = remote.share(&job.token, &path, &meta).await {
                // The share failed: the file just uploaded goes again, only while it is ours.
                if !etag.is_empty() {
                    let _ = remote.delete_file(&job.user, &path, &etag).await;
                }
                return Err(err.into());
            }
            shared_ids.extend(uploaded.file_id);
        }
        Ok(())
    }

    /// Uploads `file` under a free name in the job's folder: one `PUT`, or in pieces of the server's
    /// size (at least 5 MiB, at most 5 GiB) above it. A name taken in between is tried again.
    async fn upload_one(&self, job: &Job, file: &Prepared) -> Result<(DavPath, Uploaded), Stop> {
        let remote = &job.remote;
        let bytes = match &file.body {
            Body::Bytes(bytes) => Arc::clone(bytes),
            Body::Path { path, modified } => {
                let (path, expected) = (path.clone(), (file.size, *modified));
                let read = tokio::task::spawn_blocking(move || {
                    let bytes = std::fs::read(&path).ok()?;
                    (stamp(&path) == Some(expected) && bytes.len() as u64 == expected.0).then_some(bytes)
                });
                match read.await.ok().flatten() {
                    Some(bytes) => Arc::new(bytes),
                    None => {
                        return Err(Stop::Failed(Box::new(refused(
                            "conflict",
                            tr("The file changed since it was prepared."),
                        ))));
                    }
                }
            }
        };
        let chunk = job
            .caps
            .chunk_max
            .filter(|max| *max > 0 && (bytes.len() as u64) > *max)
            .map(|max| max.clamp(torro_nextcloud::CHUNK_MIN, torro_nextcloud::CHUNK_MAX));
        for _ in 0..3 {
            let name = remote.free_name(&job.user, &job.folder, &file.name).await?;
            let path = job
                .folder
                .join(&name)
                .ok_or_else(|| Stop::Failed(Box::new(refused("error", tr("The file name cannot be used.")))))?;
            let done = match chunk {
                None => remote.put_file(&job.user, &path, bytes.to_vec()).await,
                Some(size) => self.upload_pieces(job, &path, &bytes, size).await,
            };
            match done {
                Ok(uploaded) => {
                    self.progress(&job.id, |progress| progress.sent += bytes.len() as u64);
                    return Ok((path, uploaded));
                }
                // Someone took the name in between: another free one.
                Err(FileError::Exists) => continue,
                Err(FileError::Chat(ChatError::Other(word))) if word == CANCELLED => return Err(Stop::Cancelled),
                Err(err) => return Err(err.into()),
            }
        }
        Err(FileError::Exists.into())
    }

    /// A chunked upload of `bytes` to `path` in pieces of `size`; cancelled between pieces, and then
    /// its pieces are dropped on the server.
    async fn upload_pieces(&self, job: &Job, path: &DavPath, bytes: &[u8], size: u64) -> Result<Uploaded, FileError> {
        let remote = &job.remote;
        let upload = format!("torro-{}", super::service::reference());
        let total = bytes.len() as u64;
        remote.start_upload(&job.user, &upload, path, total).await?;
        for (index, piece) in bytes.chunks(usize::try_from(size).unwrap_or(usize::MAX)).enumerate() {
            if job.cancel.load(Ordering::Acquire) {
                self.clean_up(job, &upload).await;
                return Err(FileError::Chat(ChatError::Other(CANCELLED.into())));
            }
            let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            if let Err(err) = remote.put_chunk(&job.user, &upload, number, piece.to_vec(), path, total).await {
                self.clean_up(job, &upload).await;
                return Err(err);
            }
            self.progress(&job.id, |progress| progress.sent += piece.len() as u64);
        }
        match remote.finish_upload(&job.user, &upload, path, total).await {
            Ok(uploaded) => {
                // The pieces counted already; `upload_one` adds the whole file once more.
                self.progress(&job.id, |progress| progress.sent = progress.sent.saturating_sub(total));
                Ok(uploaded)
            }
            Err(err) => {
                self.clean_up(job, &upload).await;
                Err(err)
            }
        }
    }

    /// Drops the pieces of the chunked upload `upload` on the server; failing is only logged (the
    /// server drops pieces after a day by itself).
    async fn clean_up(&self, job: &Job, upload: &str) {
        if job.remote.cancel_upload(&job.user, upload).await.is_err() {
            eprintln!("torroworkplace: a cancelled chat upload of {} could not be removed", job.account);
        }
    }

    /// The messages the shared files `ids` became, found among the newest messages of the
    /// conversation; none when they cannot be read.
    async fn messages_of(&self, job: &Job, ids: &[u64]) -> Vec<String> {
        let Ok(token) = torro_nextcloud::RoomToken::parse(&job.token).ok_or(()) else { return Vec::new() };
        if self.fetch_newest(&job.account, &token, &job.remote, &job.caps).await.is_err() {
            return Vec::new();
        }
        let wanted: Vec<String> = ids.iter().map(u64::to_string).collect();
        let cache = self.cache();
        let Some(thread) = cache.peek(&job.conversation) else { return Vec::new() };
        thread
            .last(50)
            .0
            .into_iter()
            .filter(|message| message.files.iter().any(|file| wanted.contains(&file.id)))
            .map(|message| message_id(&job.conversation, message.number))
            .collect()
    }

    /// `chat.get_upload`: the sending `id` of `caller`.
    pub fn upload_state(&self, id: &str, caller: Caller<'_>) -> Response {
        let store = self.media();
        match store.sendings.get(id).filter(|sending| sending.owner.as_deref() == caller.client) {
            Some(sending) => Response::ChatUpload { upload: sending.progress.clone() },
            None => error_response(&ChatError::NotFound),
        }
    }

    /// `chat.cancel_upload`: stops the sending `id` of `caller` at its next step; `conflict` once its
    /// files are being shared.
    pub fn cancel(&self, id: &str, caller: Caller<'_>) -> Response {
        let store = self.media();
        let Some(sending) = store.sendings.get(id).filter(|sending| sending.owner.as_deref() == caller.client) else {
            return error_response(&ChatError::NotFound);
        };
        match sending.progress.state {
            UploadState::Uploading => {
                sending.cancel.store(true, Ordering::Release);
                Response::Ok
            }
            UploadState::Sharing | UploadState::Sent => {
                refused("conflict", tr("The files are being shared already. Delete the message instead."))
            }
            UploadState::Cancelled | UploadState::Failed => Response::Ok,
        }
    }

    /// What an approval of sending `ids` shows of each file, from the store.
    pub fn file_facts(&self, ids: &[String]) -> Vec<FileFact> {
        self.media()
            .peek(ids)
            .into_iter()
            .map(|file| FileFact {
                name: file.name,
                size: file.size,
                kind: file.kind,
                origin: file.origin,
                named_by_caller: file.named_by_caller,
            })
            .collect()
    }
}

/// What an approval shows of one file to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFact {
    pub name: String,
    pub size: u64,
    pub kind: String,
    pub origin: Origin,
    pub named_by_caller: bool,
}

/// The word a cancelled chunked upload stops with inside [`ChatService::upload_pieces`].
const CANCELLED: &str = "cancelled";

/// The code of an error answer, for the log.
fn code_of(response: &Response) -> &str {
    match response {
        Response::Error { code, .. } => code,
        _ => "",
    }
}

fn unsupported(text: &'static str) -> ChatError {
    ChatError::Unsupported(tr(text).to_string())
}

/// A prepared file of `bytes` (without id, account and owner): kind, hash and the preview of a picture.
fn of_bytes(bytes: Vec<u8>, name: String, origin: Origin, register: Option<String>) -> Prepared {
    let kind = sniff(&bytes).to_string();
    let preview = (kind.starts_with("image/") && bytes.len() as u64 <= PREVIEW_SOURCE_BYTES)
        .then(|| picture_preview(&bytes))
        .flatten();
    Prepared {
        id: String::new(),
        account: String::new(),
        owner: None,
        name,
        named_by_caller: false,
        size: bytes.len() as u64,
        sha256: sha256(&bytes),
        kind,
        origin,
        register,
        width: preview.as_ref().map(|(_, width, _)| *width),
        height: preview.as_ref().map(|(_, _, height)| *height),
        preview: preview.map(|(png, ..)| png),
        body: Body::Bytes(Arc::new(bytes)),
        used: Instant::now(),
    }
}

/// Reads `source` as far as preparing needs it, at most `limit` bytes; `space` says the limit is the
/// free space of the account. Runs on a blocking thread.
fn read_source(source: Source, limit: u64, space: bool) -> Result<Prepared, Response> {
    let unreadable = || refused("error", tr("The file cannot be read."));
    let over = |size: u64| -> Result<(), Response> {
        match (size > limit, space) {
            (false, _) => Ok(()),
            (true, true) => Err(refused("error", tr("There is not enough space in your Nextcloud."))),
            (true, false) => Err(refused("error", &too_large(limit))),
        }
    };
    match source {
        Source::Tool { register, path, title, sha256: expected } => {
            let (size, _) = stamp(&path).ok_or_else(not_found)?;
            over(size)?;
            let bytes = std::fs::read(&path).map_err(|_| not_found())?;
            over(bytes.len() as u64)?;
            if sha256(&bytes) != expected {
                return Err(not_found());
            }
            let extension = path.extension().and_then(|e| e.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
            let name = clean_name(&format!("{}{extension}", title.trim()));
            Ok(of_bytes(bytes, name, Origin::Tool, Some(register)))
        }
        Source::Path(path) => {
            let (size, modified) = stamp(&path).ok_or_else(unreadable)?;
            over(size)?;
            let start = head(&path, 512).map_err(|_| unreadable())?;
            let kind = sniff(&start).to_string();
            let preview = match kind.starts_with("image/") && size <= PREVIEW_SOURCE_BYTES {
                true => std::fs::read(&path).ok().and_then(|bytes| picture_preview(&bytes)),
                false => None,
            };
            let name =
                clean_name(&path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default());
            Ok(Prepared {
                id: String::new(),
                account: String::new(),
                owner: None,
                name,
                named_by_caller: false,
                size,
                kind,
                sha256: String::new(),
                origin: Origin::Path,
                register: None,
                width: preview.as_ref().map(|(_, width, _)| *width),
                height: preview.as_ref().map(|(_, _, height)| *height),
                preview: preview.map(|(png, ..)| png),
                body: Body::Path { path, modified },
                used: Instant::now(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(width, height)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("a PNG");
        bytes
    }

    #[test]
    fn the_kind_comes_from_the_bytes() {
        assert_eq!(sniff(&png(2, 2)), "image/png");
        assert_eq!(sniff(b"\xff\xd8\xff\xe0rest"), "image/jpeg");
        assert_eq!(sniff(b"%PDF-1.7\n"), "application/pdf");
        assert_eq!(sniff(b"\0\0\0\x18ftypheic"), "image/heic");
        assert_eq!(sniff(b"\0\0\0\x18ftypisom"), "video/mp4");
        assert_eq!(sniff(b"  <svg xmlns=\"http://www.w3.org/2000/svg\">"), "text/html", "never a picture");
        assert_eq!(sniff(b"plain words"), "text/plain");
        assert_eq!(sniff(&[0, 1, 2, 3]), "application/octet-stream");
        for kind in ["image/heic", "video/mp4", "application/octet-stream", ""] {
            assert!(is_picture_content(kind), "{kind}");
        }
        assert!(!is_picture_content("application/pdf") && !is_picture_content("text/plain"));
    }

    #[test]
    fn names_are_one_safe_part() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("C:\\Users\\a\\plan.pdf"), "plan.pdf");
        assert_eq!(clean_name("a\nb.txt"), "ab.txt");
        assert_eq!(clean_name(".."), tr("file"));
        let long = format!("{}.pdf", "x".repeat(300));
        assert!(clean_name(&long).ends_with(".pdf") && clean_name(&long).len() <= NAME_MAX);
        assert_eq!(segment("Team/Alpha"), "Team-Alpha");
    }

    #[test]
    fn a_picture_gets_a_small_preview() {
        let (preview, width, height) = picture_preview(&png(800, 400)).expect("a preview");
        assert_eq!((width, height), (800, 400));
        let small = image::load_from_memory(&preview).expect("a PNG");
        assert_eq!((small.width(), small.height()), (256, 128));
        assert!(picture_preview(b"<svg/>").is_none());
    }

    #[test]
    fn uploads_come_in_order_and_repeats_are_harmless() {
        let now = Instant::now();
        let mut store = Store::default();
        let (id, received) = store.piece("acc", Some("c1"), None, 0, b"abc".to_vec(), None, now).expect("first");
        assert_eq!(received, 3);
        assert_eq!(store.piece("acc", Some("c1"), Some(&id), 3, b"de".to_vec(), None, now).map(|r| r.1).ok(), Some(5));
        assert_eq!(store.piece("acc", Some("c1"), Some(&id), 3, b"de".to_vec(), None, now).map(|r| r.1).ok(), Some(5));
        assert!(
            matches!(store.piece("acc", Some("c1"), Some(&id), 3, b"XX".to_vec(), None, now), Err(Response::Error { code, .. }) if code == "conflict")
        );
        assert!(
            matches!(store.piece("acc", Some("c1"), Some(&id), 9, b"x".to_vec(), None, now), Err(Response::Error { code, .. }) if code == "conflict")
        );
        assert!(
            matches!(store.piece("acc", Some("c2"), Some(&id), 5, b"x".to_vec(), None, now), Err(Response::Error { code, .. }) if code == "not-found"),
            "another client's"
        );
        assert!(store.piece("acc", None, None, 1, b"x".to_vec(), None, now).is_err(), "a start at offset 0");
        for _ in 1..MAX_OPEN_UPLOADS {
            store.piece("acc", Some("c1"), None, 0, b"x".to_vec(), None, now).expect("open");
        }
        assert!(
            matches!(store.piece("acc", Some("c1"), None, 0, b"x".to_vec(), None, now), Err(Response::Error { code, .. }) if code == "busy")
        );
        assert_eq!(store.finish(&id, "acc", Some("c1")).map(|(_, bytes)| bytes), Some(b"abcde".to_vec()));
        store.tidy(now + UPLOAD_OPEN_FOR);
        assert!(store.partial.is_empty(), "unfinished uploads run out");
    }

    #[test]
    fn a_client_sends_twenty_files_or_100_mib_an_hour() {
        let now = Instant::now();
        let mut store = Store::default();
        assert!(store.admit("agent", 19, 1000, now));
        assert!(!store.admit("agent", 2, 1000, now));
        assert!(store.admit("agent", 1, 1000, now));
        assert!(!store.admit("agent", 1, 1, now));
        assert!(store.admit("other", 1, BYTES_PER_HOUR, now));
        assert!(!store.admit("other", 1, 1, now), "the bytes count too");
        assert!(store.admit("agent", 1, 1, now + HOUR));
    }

    #[test]
    fn only_the_owner_releases_and_a_waiting_approval_keeps_a_file() {
        let now = Instant::now();
        let mut store = Store::default();
        let mut file = of_bytes(b"hello".to_vec(), "a.txt".into(), Origin::Upload, None);
        (file.id, file.account, file.owner) = ("f1".into(), "acc".into(), Some("c1".into()));
        store.keep(file).expect("kept");
        assert!(store.release("acc", Some("c2"), &["f1".into()], &[]).is_ok());
        assert_eq!(store.prepared.len(), 1, "another client's file stays, without a word");
        assert!(
            matches!(store.release("acc", Some("c1"), &["f1".into()], &["f1".into()]), Err(Response::Error { code, .. }) if code == "conflict")
        );
        assert!(store.lease("other", Some("c1"), &["f1".into()], now).is_err(), "another account");
        assert!(store.lease("acc", None, &["f1".into()], now).is_err(), "the app key is no owner of it");
        assert_eq!(store.lease("acc", Some("c1"), &["f1".into()], now).map(|files| files.len()).ok(), Some(1));
        assert!(store.release("acc", Some("c1"), &["f1".into(), "unknown".into()], &[]).is_ok());
        assert!(store.prepared.is_empty());
    }
}

/// Sending and reading files through the service, against the server in memory
/// ([`super::fake::Server`]): no network, files only in temporary folders.
#[cfg(test)]
mod flow {
    use std::sync::Arc;
    use std::time::Duration;

    use torro_chat::{conversation_id, message_id};
    use torro_ipc::api::{Rights, RightsTemplate, Selection};
    use torro_ipc::chat::{FileSource, PreviewSize, SharedKind, UploadProgress, UploadState};
    use torro_ipc::{Request, Response};

    use super::super::ChatService;
    use super::super::fake::Server;
    use super::super::remote::Picture;
    use super::super::service::Caller;
    use super::*;
    use crate::pim::Reach;

    const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
    const APP: Caller<'static> = Caller { reach: Reach::Suite, client: None };

    fn ben() -> String {
        conversation_id(ACCOUNT, "ben4chat")
    }

    fn service(dir: &std::path::Path) -> (Arc<ChatService>, Arc<Server>) {
        let server = Arc::new(Server::default());
        let chat = ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]).with_attachments(dir.join("chat"));
        (Arc::new(chat), server)
    }

    fn rights(pictures: bool) -> Rights {
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.chat_accounts = Selection::Only(vec![ACCOUNT.into()]);
        rights.image_content = pictures;
        rights
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            _ => "",
        }
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(4, 3)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("a PNG");
        bytes
    }

    /// Uploads `bytes` as one piece and answers the prepared file's id.
    async fn upload(chat: &Arc<ChatService>, bytes: &[u8], name: &str, caller: Caller<'_>) -> String {
        let request = Request::ChatUploadFile {
            account: ACCOUNT.into(),
            upload: None,
            offset: 0,
            base64: STANDARD.encode(bytes).into(),
            name: Some(name.into()),
            finish: true,
        };
        match chat.handle(request, caller).await {
            Response::ChatPreparedFile { file } => file.id,
            other => panic!("not prepared: {other:?}"),
        }
    }

    fn send(files: Vec<String>, text: &str) -> Request {
        Request::ChatSendMessage {
            conversation: ben(),
            text: text.into(),
            reply_to: None,
            silent: false,
            files,
            items: Vec::new(),
            fields: None,
        }
    }

    /// Waits until the sending `id` is over and answers how it ended.
    async fn ended(chat: &Arc<ChatService>, id: &str, caller: Caller<'_>) -> UploadProgress {
        for _ in 0..300 {
            if let Response::ChatUpload { upload } = chat.upload_state(id, caller)
                && upload.state.is_over()
            {
                return upload;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the sending did not end")
    }

    #[tokio::test]
    async fn a_file_is_uploaded_shared_with_its_text_and_found_as_a_message() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        let id = upload(&chat, &png(), "Plan.png", APP).await;
        let Response::ChatSending { id: sending } = chat.handle(send(vec![id.clone()], "The plan"), APP).await else {
            panic!("sending")
        };
        let done = ended(&chat, &sending, APP).await;
        assert_eq!(done.state, UploadState::Sent, "{done:?}");
        assert_eq!(done.sent, done.total);
        assert_eq!(done.messages, [message_id(&ben(), 6)]);
        {
            let state = server.state();
            assert!(state.files.contains_key("/Talk/Plan.png"));
            let (token, path, meta) = &state.shares[0];
            assert_eq!((token.as_str(), path.as_str()), ("ben4chat", "/Talk/Plan.png"));
            assert_eq!(meta.caption.as_deref(), Some("The plan"));
            assert_eq!(meta.reference_id.as_ref().map(String::len), Some(32), "against a second post");
            assert!(state.sent.is_empty(), "the text went with the file");
        }
        // The lease is used up, and the name is taken now: a second file gets a free one.
        assert_eq!(code(&chat.handle(send(vec![id], ""), APP).await), "not-found");
        let again = upload(&chat, &png(), "Plan.png", APP).await;
        let Response::ChatSending { id: sending } = chat.handle(send(vec![again], ""), APP).await else { panic!() };
        assert_eq!(ended(&chat, &sending, APP).await.state, UploadState::Sent);
        assert!(server.state().files.contains_key("/Talk/Plan (2).png"));
        // Once sent, cancelling is too late.
        assert_eq!(code(&chat.cancel(&sending, APP)), "conflict");
    }

    #[tokio::test]
    async fn without_captions_the_text_goes_first_and_large_files_go_in_pieces() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        {
            let mut state = server.state();
            state.caps.caption = false;
            state.caps.chunk_max = Some(1024);
            state.caps.subfolders = true;
        }
        // A file above the server's piece size goes in pieces of at least 5 MiB.
        let big = vec![7u8; 6 * 1024 * 1024];
        let id = upload_in_pieces(&chat, &big).await;
        let Response::ChatSending { id: sending } = chat.handle(send(vec![id], "Look"), APP).await else { panic!() };
        let done = ended(&chat, &sending, APP).await;
        assert_eq!((done.state, done.sent), (UploadState::Sent, big.len() as u64), "{done:?}");
        let state = server.state();
        assert_eq!(state.sent.first().map(|(_, sending)| sending.text.as_str()), Some("Look"), "the text first");
        assert!(state.shares[0].2.caption.is_none());
        assert_eq!(state.calls.iter().filter(|call| call.starts_with("put_chunk")).count(), 2);
        let path = &state.shares[0].1;
        assert!(path.starts_with("/Talk/Ben Koch-ben4chat/Anna Berg-anna/"), "{path}");
        assert_eq!(state.files.get(path).map(|file| file.bytes.len()), Some(big.len()));
    }

    /// Uploads `bytes` in pieces of 256 KiB (`chat.upload_file`) and answers the prepared file's id.
    async fn upload_in_pieces(chat: &Arc<ChatService>, bytes: &[u8]) -> String {
        let mut upload = None;
        let pieces: Vec<&[u8]> = bytes.chunks(UPLOAD_PIECE_BYTES).collect();
        for (index, piece) in pieces.iter().enumerate() {
            let request = Request::ChatUploadFile {
                account: ACCOUNT.into(),
                upload: upload.clone(),
                offset: (index * UPLOAD_PIECE_BYTES) as u64,
                base64: STANDARD.encode(piece).into(),
                name: Some("film.bin".into()),
                finish: index + 1 == pieces.len(),
            };
            match chat.handle(request, APP).await {
                Response::ChatFileUpload { upload: id, .. } => upload = Some(id),
                Response::ChatPreparedFile { file } => return file.id,
                other => panic!("piece {index}: {other:?}"),
            }
        }
        panic!("never complete")
    }

    #[tokio::test]
    async fn a_failed_share_takes_its_upload_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        server.state().refuse_shares = true;
        let id = upload(&chat, b"%PDF-1.7 hello", "a.pdf", APP).await;
        let Response::ChatSending { id: sending } = chat.handle(send(vec![id], ""), APP).await else { panic!() };
        let done = ended(&chat, &sending, APP).await;
        assert_eq!((done.state, done.code.as_deref()), (UploadState::Failed, Some("unsupported")));
        assert!(server.state().files.is_empty(), "deleted again");
        assert_eq!(server.count("delete_file"), 1);
    }

    #[tokio::test]
    async fn a_client_reaches_only_its_own_files_and_sendings() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        let rights = rights(false);
        let agent = Caller { reach: Reach::Client(&rights), client: Some("agent") };
        let other = Caller { reach: Reach::Client(&rights), client: Some("other") };
        let mine = upload(&chat, b"hello", "a.txt", agent).await;
        assert_eq!(code(&chat.handle(send(vec![mine.clone()], ""), other).await), "not-found");
        assert_eq!(code(&chat.handle(send(vec![mine.clone()], ""), APP).await), "not-found", "not the app's either");
        assert!(chat.files_refusal(ACCOUNT, std::slice::from_ref(&mine), Some("other")).is_some());
        assert!(chat.files_refusal(ACCOUNT, std::slice::from_ref(&mine), Some("agent")).is_none());
        let release = Request::ChatReleaseFiles { account: ACCOUNT.into(), ids: vec![mine.clone()] };
        assert_eq!(chat.handle(release, other).await, Response::Ok, "another's id passes without a word");
        let Response::ChatSending { id: sending } = chat.handle(send(vec![mine], "x"), agent).await else { panic!() };
        assert_eq!(code(&chat.upload_state(&sending, other)), "not-found");
        assert_eq!(ended(&chat, &sending, agent).await.state, UploadState::Sent);
        assert_eq!(server.state().shares.len(), 1);
        // A file from the computer is no client's.
        let path = Request::ChatPrepareFile { account: ACCOUNT.into(), source: FileSource::Path { path: "/x".into() } };
        assert!(path.kept_for_people());
    }

    #[tokio::test]
    async fn a_file_chosen_on_the_computer_must_not_change_before_it_goes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, b"first").expect("written");
        let Response::ChatPreparedFile { file: prepared } =
            chat.prepare(ACCOUNT, Source::Path(file.clone()), APP).await
        else {
            panic!("prepared")
        };
        assert_eq!((prepared.size, prepared.content_type.as_str()), (5, "text/plain"));
        std::fs::write(&file, b"changed!").expect("written");
        assert_eq!(code(&chat.handle(send(vec![prepared.id], ""), APP).await), "conflict");
        assert!(server.state().files.is_empty());
        // Larger than the free space of the account: refused before anything is uploaded.
        server.state().free = Some(3);
        let refused = chat.prepare(ACCOUNT, Source::Path(file), APP).await;
        assert_eq!(code(&refused), "error");
    }

    #[tokio::test]
    async fn only_files_of_the_message_are_read_and_pictures_need_their_right() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        let number = server.share_from_ben("ben4chat", "/Talk/Beach.png", "image/png", &png());
        let file_id = server.state().files["/Talk/Beach.png"].id;
        server.state().previews.insert(file_id, Picture { bytes: png(), content_type: "image/png".into() });
        let message = message_id(&ben(), number);
        let preview = |file: &str| Request::ChatGetPreview {
            id: message.clone(),
            file: Some(file.into()),
            card: false,
            size: PreviewSize::Small,
        };
        let Response::ChatPicture { picture } = chat.handle(preview(&file_id.to_string()), APP).await else {
            panic!("a preview")
        };
        assert_eq!((picture.content_type.as_str(), picture.width), ("image/png", Some(4)));
        // A file id of the person's storage that is no file of this message: not-found, nothing fetched.
        assert_eq!(code(&chat.handle(preview("4242"), APP).await), "not-found");
        let other_message = message_id(&ben(), 1);
        let foreign = Request::ChatGetAttachment { id: other_message, file: file_id.to_string() };
        assert_eq!(code(&chat.handle(foreign, APP).await), "not-found");
        assert_eq!(server.count("get_file"), 0);

        // A client without the right to picture content gets no picture, no file, no blurhash.
        let blind = rights(false);
        let agent = Caller { reach: Reach::Client(&blind), client: Some("agent") };
        assert_eq!(code(&chat.handle(preview(&file_id.to_string()), agent).await), "forbidden");
        let attachment = Request::ChatGetAttachment { id: message.clone(), file: file_id.to_string() };
        assert_eq!(code(&chat.handle(attachment.clone(), agent).await), "forbidden");
        let read =
            Request::ChatGetMessages { conversation: ben(), before: None, after: None, around: None, limit: None };
        let Response::ChatMessages(page) = chat.handle(read.clone(), agent).await else { panic!("messages") };
        let shared = page.messages.iter().find(|m| m.number() == number).expect("the message");
        assert!(shared.files[0].blurhash.is_none());
        let Response::ChatMessages(page) = chat.handle(read, APP).await else { panic!("messages") };
        assert!(page.messages.iter().any(|m| m.files.iter().any(|f| f.blurhash.is_some())), "the app sees it");

        // With the right: fetched into the chat's own folder, private.
        let seeing = rights(true);
        let agent = Caller { reach: Reach::Client(&seeing), client: Some("agent") };
        let Response::ChatAttachment { attachment } = chat.handle(attachment, agent).await else { panic!("fetched") };
        assert_eq!((attachment.name.as_str(), attachment.content_type.as_str()), ("Beach.png", "image/png"));
        assert!(attachment.path.starts_with(&dir.path().join("chat").to_string_lossy().into_owned()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&attachment.path).expect("there").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        chat.forget_account(ACCOUNT);
        assert!(!std::path::Path::new(&attachment.path).exists(), "an account takes its files with it");
    }

    #[tokio::test]
    async fn a_file_shared_for_viewing_only_is_neither_fetched_nor_saved() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        let number = server.share_from_ben("ben4chat", "/Talk/secret.pdf", "application/pdf", b"%PDF-1.7");
        let file_id = server.state().files["/Talk/secret.pdf"].id.to_string();
        if let Some(list) = server.state().messages.get_mut("ben4chat")
            && let Some(shared) = list.iter_mut().find(|m| m.number == number)
        {
            shared.files[0].hide_download = true;
        }
        let message = message_id(&ben(), number);
        let fetch = Request::ChatGetAttachment { id: message.clone(), file: file_id.clone() };
        assert_eq!(code(&chat.handle(fetch, APP).await), "forbidden", "for the app key too");
        let place = super::super::attachments::Place::Folder(dir.path().to_path_buf());
        assert_eq!(code(&chat.save_attachment(&message, &file_id, place, APP).await), "forbidden");
    }

    #[tokio::test]
    async fn a_saved_file_never_replaces_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        let number = server.share_from_ben("ben4chat", "/Talk/notes.txt", "text/plain", b"hello");
        let file_id = server.state().files["/Talk/notes.txt"].id.to_string();
        let message = message_id(&ben(), number);
        let downloads = dir.path().join("Downloads");
        std::fs::create_dir(&downloads).expect("a folder");
        std::fs::write(downloads.join("notes.txt"), b"mine").expect("written");
        let place = super::super::attachments::Place::Folder(downloads.clone());
        let Response::ChatSaved { path } = chat.save_attachment(&message, &file_id, place, APP).await else {
            panic!("saved")
        };
        assert!(path.ends_with("notes (2).txt"), "{path}");
        assert_eq!(std::fs::read(downloads.join("notes.txt")).expect("read"), b"mine");
    }

    #[tokio::test]
    async fn cards_of_the_own_server_come_by_themselves_and_others_only_on_a_click() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        let reference = |title: &str| torro_nextcloud::Reference {
            title: title.into(),
            description: None,
            image: Some("abc123".into()),
        };
        {
            let mut state = server.state();
            let own = "https://cloud.example.org/f/12";
            let foreign = "https://tracker.example.net/x";
            state.references.insert(own.into(), reference("Plan.pdf"));
            state.references.insert(foreign.into(), reference("Somewhere"));
            state.card_pictures.insert("abc123".into(), Picture { bytes: png(), content_type: "image/png".into() });
            let list = state.messages.get_mut("ben4chat").expect("Ben");
            list.push(super::super::remote::tests::message(6, &format!("see {own}")));
            list.push(super::super::remote::tests::message(7, &format!("look {foreign}")));
        }
        let read =
            Request::ChatGetMessages { conversation: ben(), before: None, after: None, around: None, limit: None };
        // A client's read resolves nothing.
        let seeing = rights(true);
        let agent = Caller { reach: Reach::Client(&seeing), client: Some("agent") };
        chat.handle(read.clone(), agent).await;
        assert_eq!(server.count("resolve"), 0);
        let Response::ChatMessages(page) = chat.handle(read.clone(), APP).await else { panic!("messages") };
        let card = |page: &torro_chat::MessagePage, n: u64| {
            page.messages.iter().find(|m| m.number() == n).and_then(|m| m.cards.first().cloned())
        };
        assert_eq!(
            card(&page, 6).map(|c| (c.title, c.domain, c.image)),
            Some(("Plan.pdf".into(), "cloud.example.org".into(), true))
        );
        assert!(card(&page, 7).is_none(), "a foreign link waits for a person");
        assert_eq!(server.count("resolve"), 1);
        let Response::ChatCard { card: Some(shown) } = chat.show_card(&message_id(&ben(), 7)).await else {
            panic!("a card")
        };
        assert_eq!(shown.title, "Somewhere");
        let Response::ChatMessages(page) = chat.handle(read, agent).await else { panic!("messages") };
        assert!(card(&page, 7).is_some(), "held for every reader now");
        let picture =
            Request::ChatGetPreview { id: message_id(&ben(), 7), file: None, card: true, size: PreviewSize::Small };
        assert!(matches!(chat.handle(picture.clone(), agent).await, Response::ChatPicture { .. }));
        let blind = rights(false);
        let blind = Caller { reach: Reach::Client(&blind), client: Some("agent") };
        assert_eq!(code(&chat.handle(picture, blind).await), "forbidden");
        // Links are listed from the cards.
        let links =
            Request::ChatListShared { conversation: ben(), kind: Some(SharedKind::Link), before: None, limit: None };
        let Response::ChatShared { items, .. } = chat.handle(links, APP).await else { panic!("links") };
        assert_eq!(
            items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(),
            ["https://tracker.example.net/x", "https://cloud.example.org/f/12"]
        );
    }

    #[tokio::test]
    async fn what_a_conversation_shared_is_listed_and_messages_around_one_are_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, server) = service(dir.path());
        let picture = server.share_from_ben("ben4chat", "/Talk/a.png", "image/png", &png());
        server.share_from_ben("ben4chat", "/Talk/b.pdf", "application/pdf", b"%PDF-1.7");
        let media =
            Request::ChatListShared { conversation: ben(), kind: Some(SharedKind::Media), before: None, limit: None };
        let Response::ChatShared { items, .. } = chat.handle(media, APP).await else { panic!("items") };
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].name.as_str(), items[0].message.clone()), ("a.png", message_id(&ben(), picture)));
        let all = Request::ChatListShared { conversation: ben(), kind: None, before: None, limit: None };
        let Response::ChatShared { items, .. } = chat.handle(all, APP).await else { panic!("items") };
        assert_eq!(items.len(), 2);
        server.state().caps.shared = false;
        chat.cache().account(ACCOUNT).known_at = None;
        let media =
            Request::ChatListShared { conversation: ben(), kind: Some(SharedKind::File), before: None, limit: None };
        assert_eq!(code(&chat.handle(media, APP).await), "unsupported");

        let around = Request::ChatGetMessages {
            conversation: ben(),
            before: None,
            after: None,
            around: Some(message_id(&ben(), 3)),
            limit: Some(2),
        };
        let Response::ChatMessages(page) = chat.handle(around, APP).await else { panic!("messages") };
        assert_eq!(page.messages.iter().map(|m| m.number()).collect::<Vec<_>>(), [2, 3, 4]);
    }

    #[tokio::test]
    async fn uploads_keep_to_their_limits() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, _) = service(dir.path());
        let piece = |offset: u64, upload: Option<String>, base64: String| Request::ChatUploadFile {
            account: ACCOUNT.into(),
            upload,
            offset,
            base64: base64.into(),
            name: None,
            finish: false,
        };
        assert_eq!(code(&chat.handle(piece(0, None, "not base64!".into()), APP).await), "error");
        let large = STANDARD.encode(vec![0u8; UPLOAD_PIECE_BYTES + 1]);
        assert_eq!(code(&chat.handle(piece(0, None, large), APP).await), "error");
        let unknown = Request::ChatUploadFile {
            account: "nextcloud:nobody@nowhere".into(),
            upload: None,
            offset: 0,
            base64: "aGk=".into(),
            name: None,
            finish: true,
        };
        assert_eq!(code(&chat.handle(unknown, APP).await), "not-found");
    }
}
