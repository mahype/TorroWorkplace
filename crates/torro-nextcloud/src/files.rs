//! Files for the chat (#643, stage 4 of `docs/chat.md`): uploading a file into the person's own
//! storage over WebDAV, sharing it into a Talk conversation, reading it back, previews, link cards
//! and what a conversation shared.
//!
//! - **WebDAV** below `/remote.php/dav/files/<user id>/` (the user id from `cloud/user`, never the
//!   login name). A new file is one `PUT` with `If-None-Match: *`, so it never replaces one
//!   ([`Nextcloud::put_file`]); a name that is taken is found first with `PROPFIND`
//!   ([`Nextcloud::free_name`]: „ (2)“, „ (3)“ … as Talk's sharing does). Folders are made with
//!   `MKCOL` one level at a time ([`Nextcloud::make_folders`]). Deleting names the `ETag` the upload
//!   got (`If-Match`), so only the file this core wrote goes ([`Nextcloud::delete_file`]).
//!   <https://docs.nextcloud.com/server/latest/developer_manual/client_apis/WebDAV/basic.html>
//! - **Chunking v2** for large files: `MKCOL /remote.php/dav/uploads/<user id>/<upload id>`, `PUT` of
//!   the pieces `1` to `10000`, `MOVE` of `.file` to the target with `Overwrite: F`; the header
//!   `Destination` on each step and `OC-Total-Length` for the quota. Pieces are 5 MiB to 5 GiB, only the
//!   last one smaller ([`CHUNK_MIN`], [`CHUNK_MAX`]). Cancelling is `DELETE` of the upload folder.
//!   <https://docs.nextcloud.com/server/latest/developer_manual/client_apis/WebDAV/chunking.html>
//! - **Sharing into a conversation:** `POST /ocs/v2.php/apps/files_sharing/api/v1/shares` with
//!   `shareType=10`, `shareWith=<token>`, `path` and `talkMetaData` (a JSON text with `caption`,
//!   `silent`, `replyTo`, `referenceId`; [`ShareMeta`]).
//!   <https://nextcloud-talk.readthedocs.io/en/latest/chat/#share-a-file-to-the-chat>
//! - **Previews:** `GET /index.php/core/preview?fileId=…&x=…&y=…&a=1`, never with `mimeFallback`
//!   (which would answer with an SVG icon); only PNG, JPEG, GIF and WebP are taken
//!   ([`Nextcloud::preview`]). The picture of a link card: `GET /index.php/core/references/preview/<id>`
//!   ([`Nextcloud::reference_preview`]).
//! - **Link cards:** `GET /ocs/v2.php/references/resolve?reference=<url>` ([`Nextcloud::resolve_reference`]):
//!   the server fetches the address; its `thumb` is taken only when it is a reference preview on the
//!   account's own origin.
//! - **Shared items:** `GET …/api/v1/chat/{token}/share?objectType=…` and `…/share/overview`
//!   (capability `rich-object-list-media`).
//!
//! Names and paths in answers are foreign text: a path from a message is split into checked segments
//! ([`DavPath`]) before it names anything in an address, and no error holds a name, a path or the
//! server's words.

use std::fmt;
use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Method, Url};
use serde_json::{Map, Value, json};

use crate::ocs::{Nextcloud, OcsError, TalkError, refusal, unreachable};
use crate::talk::{ChatMessage, RoomToken, read_message};

/// The smallest piece of a chunked upload but the last (the server refuses smaller ones).
pub const CHUNK_MIN: u64 = 5 * 1024 * 1024;

/// The largest piece of a chunked upload.
pub const CHUNK_MAX: u64 = 5 * 1024 * 1024 * 1024;

/// The most pieces of one chunked upload.
pub const CHUNKS_MAX: u32 = 10_000;

/// How long one upload, piece or download may take before it counts as lost: far more than an
/// OCS call, a piece of 100 MiB on a slow line needs minutes.
const TRANSFER_WAIT: Duration = Duration::from_secs(15 * 60);

/// The most bytes of a preview picture.
pub const PREVIEW_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// The highest number [`Nextcloud::free_name`] tries before it gives up (`Name (99).ext`).
const MAX_COPY: u32 = 99;

/// The longest path in the person's files, in bytes.
const MAX_PATH_BYTES: usize = 4096;

/// The longest segment of a path, in bytes.
const MAX_SEGMENT_BYTES: usize = 255;

const SHARES: &str = "/ocs/v2.php/apps/files_sharing/api/v1/shares";
const CHAT: &str = "/ocs/v2.php/apps/spreed/api/v1/chat";

/// Why a call of this file failed. As [`OcsError`], no message holds a name, a path or the
/// server's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesError {
    /// What every call can say: 401, 429, maintenance, offline, an unreadable answer.
    Ocs(OcsError),
    /// 404, or 409 for a folder that is missing on the way.
    NotFound,
    /// 412: there is a file of that name already (`If-None-Match`, `Overwrite: F`), or the file is
    /// no longer the one the `ETag` named (`If-Match`).
    Exists,
    /// 507: the person's storage is full.
    NoSpace,
    /// 413, or more bytes than asked for.
    TooLarge,
    /// 403: the server does not allow it (sharing switched off, no right on the file).
    Forbidden,
    /// 400 or 405, or a path or name that cannot be one: the server does not take it.
    Invalid,
}

impl fmt::Display for FilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilesError::Ocs(error) => error.fmt(f),
            FilesError::NotFound => f.write_str("the file or folder is not there"),
            FilesError::Exists => f.write_str("a file of that name is there, or the file changed"),
            FilesError::NoSpace => f.write_str("the storage on the server is full"),
            FilesError::TooLarge => f.write_str("the file is too large"),
            FilesError::Forbidden => f.write_str("Nextcloud does not allow this"),
            FilesError::Invalid => f.write_str("Nextcloud does not accept this request"),
        }
    }
}

impl std::error::Error for FilesError {}

impl From<OcsError> for FilesError {
    fn from(error: OcsError) -> FilesError {
        FilesError::Ocs(error)
    }
}

/// A path in the person's files as checked segments: no empty, `.` or `..` segment, no control
/// character, at most 255 bytes a segment and 4096 in all. The only way to name a file or folder to
/// this file, so no foreign text gets into an address as anything but one segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DavPath(Vec<String>);

impl DavPath {
    /// The path `path` (`/Talk/Plan.pdf` or `Talk/Plan.pdf`; `/` is the root); `None` when a segment
    /// breaks a rule.
    pub fn parse(path: &str) -> Option<DavPath> {
        if path.len() > MAX_PATH_BYTES {
            return None;
        }
        let segments: Vec<String> = path.split('/').filter(|segment| !segment.is_empty()).map(str::to_string).collect();
        segments.iter().all(|segment| is_segment(segment)).then_some(DavPath(segments))
    }

    /// The root of the person's files.
    pub fn root() -> DavPath {
        DavPath(Vec::new())
    }

    /// This folder with `name` below it; `None` when `name` is no one segment.
    pub fn join(&self, name: &str) -> Option<DavPath> {
        let mut segments = self.0.clone();
        segments.push(name.to_string());
        let joined = DavPath(segments);
        (is_segment(name) && joined.as_path().len() <= MAX_PATH_BYTES).then_some(joined)
    }

    /// The segments, the first below the root first.
    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// The last segment; empty for the root.
    pub fn name(&self) -> &str {
        self.0.last().map_or("", String::as_str)
    }

    /// The path as Nextcloud names it in a share: `/` and the segments.
    pub fn as_path(&self) -> String {
        format!("/{}", self.0.join("/"))
    }

    /// The folders from the first below the root down to this one, each a path of its own.
    fn levels(&self) -> Vec<DavPath> {
        (1..=self.0.len()).map(|n| DavPath(self.0[..n].to_vec())).collect()
    }
}

/// Whether `segment` may be one segment of a [`DavPath`].
fn is_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= MAX_SEGMENT_BYTES
        && !matches!(segment, "." | "..")
        && !segment.contains(['/', '\\'])
        && !segment.chars().any(char::is_control)
}

/// What an upload got back: the file's id and its `ETag`, as far as the server named them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Uploaded {
    /// The file id (`OC-FileId`, its leading digits), for previews and for finding the message.
    pub file_id: Option<u64>,
    /// The `ETag` (`OC-ETag`, else `ETag`), exactly as sent, for deleting only this file.
    pub etag: Option<String>,
}

/// A picture the server drew: its bytes and media type (`image/png`, `image/jpeg`, `image/gif`,
/// `image/webp`).
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

impl fmt::Debug for Picture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Picture").field("bytes", &self.bytes.len()).field("content_type", &self.content_type).finish()
    }
}

/// What `talkMetaData` of a share carries: the text under the file, whether nobody is notified, the
/// message it answers and an id against a second post of the same share.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShareMeta {
    /// The text under the file (capability `media-caption`).
    pub caption: Option<String>,
    /// Shared without notifying (with `media-caption`).
    pub silent: bool,
    /// The message the share answers.
    pub reply_to: Option<u64>,
    /// A random id of this share (capability `chat-reference-id`).
    pub reference_id: Option<String>,
}

/// A link card as the server resolved it. Everything in it is foreign text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The title (`openGraphObject.name`), one line.
    pub title: String,
    /// The description, one line, when there is one.
    pub description: Option<String>,
    /// The id of its picture on the account's own server (`/index.php/core/references/preview/<id>`),
    /// only when `thumb` points there; never another address.
    pub image: Option<String>,
}

impl Nextcloud {
    /// Whether a file or folder `path` of the user `user` is there (`PROPFIND`, `Depth: 0`).
    pub async fn file_exists(&self, user: &str, path: &DavPath) -> Result<bool, FilesError> {
        let body = r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/></d:prop></d:propfind>"#;
        let request = self
            .plain(method(b"PROPFIND")?, self.dav_url(user, path)?)
            .header("Depth", "0")
            .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(body);
        match status(request, None).await {
            Ok(_) => Ok(true),
            Err(FilesError::NotFound) => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// `name` in `folder` when no file has it, else the first of „name (2).ext“, „name (3).ext“ … up
    /// to 99 that is free. [`FilesError::Exists`] when all are taken, [`FilesError::Invalid`] for a
    /// name that is no one segment.
    pub async fn free_name(&self, user: &str, folder: &DavPath, name: &str) -> Result<String, FilesError> {
        for copy in 1..=MAX_COPY {
            let candidate = numbered(name, copy);
            let path = folder.join(&candidate).ok_or(FilesError::Invalid)?;
            if !self.file_exists(user, &path).await? {
                return Ok(candidate);
            }
        }
        Err(FilesError::Exists)
    }

    /// Makes `folder` and every folder above it that is missing (`MKCOL`, one level at a time); a
    /// folder that is there already is fine.
    pub async fn make_folders(&self, user: &str, folder: &DavPath) -> Result<(), FilesError> {
        for level in folder.levels() {
            let request = self.plain(method(b"MKCOL")?, self.dav_url(user, &level)?);
            match status(request, None).await {
                // 405: there is one already.
                Ok(_) | Err(FilesError::Invalid) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    /// Uploads `bytes` as the new file `path` in one `PUT` with `If-None-Match: *`: it never
    /// replaces a file ([`FilesError::Exists`]). [`FilesError::NoSpace`] when the storage is full.
    pub async fn put_file(&self, user: &str, path: &DavPath, bytes: Vec<u8>) -> Result<Uploaded, FilesError> {
        let request = self
            .plain(Method::PUT, self.dav_url(user, path)?)
            .header(header::IF_NONE_MATCH, "*")
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(bytes);
        status(request, Some(TRANSFER_WAIT)).await.map(|headers| uploaded(&headers))
    }

    /// Starts the chunked upload `upload` (letters, digits and `-`) of `total` bytes for the file
    /// `target`: `MKCOL` of the upload folder with `Destination`.
    pub async fn start_upload(&self, user: &str, upload: &str, target: &DavPath, total: u64) -> Result<(), FilesError> {
        let request = self.plain(method(b"MKCOL")?, self.upload_url(user, upload, None)?);
        status(self.destined(request, user, target, total)?, None).await.map(|_| ())
    }

    /// Uploads the piece `number` (1 to [`CHUNKS_MAX`]) of the chunked upload `upload`.
    pub async fn put_chunk(
        &self,
        user: &str,
        upload: &str,
        number: u32,
        bytes: Vec<u8>,
        target: &DavPath,
        total: u64,
    ) -> Result<(), FilesError> {
        if !(1..=CHUNKS_MAX).contains(&number) {
            return Err(FilesError::Invalid);
        }
        let url = self.upload_url(user, upload, Some(&number.to_string()))?;
        let request = self.plain(Method::PUT, url).header(header::CONTENT_TYPE, "application/octet-stream").body(bytes);
        status(self.destined(request, user, target, total)?, Some(TRANSFER_WAIT)).await.map(|_| ())
    }

    /// Puts the pieces of the chunked upload `upload` together as the file `target`: `MOVE` of
    /// `.file` with `Overwrite: F`, so it never replaces a file ([`FilesError::Exists`]).
    pub async fn finish_upload(
        &self,
        user: &str,
        upload: &str,
        target: &DavPath,
        total: u64,
    ) -> Result<Uploaded, FilesError> {
        let request =
            self.plain(method(b"MOVE")?, self.upload_url(user, upload, Some(".file"))?).header("Overwrite", "F");
        status(self.destined(request, user, target, total)?, Some(TRANSFER_WAIT))
            .await
            .map(|headers| uploaded(&headers))
    }

    /// Drops the chunked upload `upload` with what it holds (`DELETE` of its folder); one that is not
    /// there is fine.
    pub async fn cancel_upload(&self, user: &str, upload: &str) -> Result<(), FilesError> {
        let request = self.plain(Method::DELETE, self.upload_url(user, upload, None)?);
        match status(request, None).await {
            Ok(_) | Err(FilesError::NotFound) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Deletes the file `path` only while it still has `etag` (`If-Match`): a file changed since is
    /// [`FilesError::Exists`] and stays. One that is gone already is fine.
    pub async fn delete_file(&self, user: &str, path: &DavPath, etag: &str) -> Result<(), FilesError> {
        let etag = HeaderValue::from_str(etag).map_err(|_| FilesError::Invalid)?;
        let request = self.plain(Method::DELETE, self.dav_url(user, path)?).header(header::IF_MATCH, etag);
        match status(request, None).await {
            Ok(_) | Err(FilesError::NotFound) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// The bytes of the file `path`, at most `max` ([`FilesError::TooLarge`] beyond, before they are
    /// all read).
    pub async fn get_file(&self, user: &str, path: &DavPath, max: u64) -> Result<Vec<u8>, FilesError> {
        let request = self.plain(Method::GET, self.dav_url(user, path)?).timeout(TRANSFER_WAIT);
        read_body(request, max).await.map(|(bytes, _)| bytes)
    }

    /// The preview of the file `file_id`, at most `edge` pixels on its longer side, with its aspect
    /// kept (`a=1`). [`FilesError::NotFound`] when the server has none; an answer that is no PNG, JPEG,
    /// GIF or WebP (an SVG above all) is [`FilesError::Invalid`].
    pub async fn preview(&self, file_id: u64, edge: u32) -> Result<Picture, FilesError> {
        let edge = edge.clamp(16, 4096).to_string();
        let query = [("fileId", file_id.to_string()), ("x", edge.clone()), ("y", edge), ("a", "1".into())];
        let url = self.endpoint("/index.php/core/preview", &[], &query)?;
        picture(self.plain(Method::GET, url)).await
    }

    /// The picture of a link card by its id on the account's server ([`Reference::image`]).
    pub async fn reference_preview(&self, id: &str) -> Result<Picture, FilesError> {
        if !is_reference_id(id) {
            return Err(FilesError::Invalid);
        }
        let url = self.endpoint("/index.php/core/references/preview", &[id], &[] as &[(&str, &str)])?;
        picture(self.plain(Method::GET, url)).await
    }

    /// The link card of `link` as the server resolves it: the server fetches the address itself.
    /// `None` when it knows none.
    pub async fn resolve_reference(&self, link: &str) -> Result<Option<Reference>, FilesError> {
        let url = self.endpoint("/ocs/v2.php/references/resolve", &[], &[("reference", link)])?;
        let data = self.exchange(Method::GET, url, None, None).await?.success()?;
        let found = data.as_ref().and_then(|data| data.pointer("/references")).and_then(|all| all.get(link));
        Ok(found.and_then(|reference| self.read_reference(reference)))
    }

    /// Shares the file `path` into the conversation `token` (share type 10) with `meta` as
    /// `talkMetaData`. [`FilesError::NotFound`] for a file or conversation that is not there,
    /// [`FilesError::Forbidden`] when the server does not allow it.
    pub async fn share_to_room(&self, token: &RoomToken, path: &DavPath, meta: &ShareMeta) -> Result<(), FilesError> {
        let mut talk = Map::new();
        if let Some(caption) = meta.caption.as_deref().filter(|caption| !caption.trim().is_empty()) {
            talk.insert("caption".into(), caption.into());
        }
        if meta.silent {
            talk.insert("silent".into(), true.into());
        }
        if let Some(reply_to) = meta.reply_to {
            talk.insert("replyTo".into(), reply_to.into());
        }
        if let Some(reference) = meta.reference_id.as_deref().filter(|id| !id.is_empty()) {
            talk.insert("referenceId".into(), reference.into());
        }
        let mut body = json!({ "shareType": 10, "shareWith": token.as_str(), "path": path.as_path() });
        if !talk.is_empty() {
            body["talkMetaData"] = Value::String(Value::Object(talk).to_string());
        }
        let url = self.endpoint::<&str>(SHARES, &[], &[])?;
        let answer = self.exchange(Method::POST, url, Some(&body), None).await?;
        match answer.status {
            200..=299 => Ok(()),
            403 => Err(FilesError::Forbidden),
            404 => Err(FilesError::NotFound),
            400 | 405 => Err(FilesError::Invalid),
            status => Err(FilesError::Ocs(OcsError::Other(format!("Nextcloud answered with HTTP {status}")))),
        }
    }

    /// Up to `limit` (1 to 200) messages of the conversation `token` that shared an item of `kind`
    /// (`media`, `file`, `voice` …), the newest first, older than the message `before` when given
    /// (capability `rich-object-list-media`).
    pub async fn shared_items(
        &self,
        token: &RoomToken,
        kind: &str,
        before: Option<u64>,
        limit: u32,
    ) -> Result<Vec<ChatMessage>, TalkError> {
        if !is_kind(kind) {
            return Err(TalkError::Invalid);
        }
        let mut query = vec![("objectType", kind.to_string()), ("limit", limit.clamp(1, 200).to_string())];
        if let Some(before) = before {
            query.push(("lastKnownMessageId", before.to_string()));
        }
        let url = self.endpoint(&format!("{CHAT}/{token}/share"), &[], &query)?;
        let data = shared_answer(self.exchange(Method::GET, url, None, None).await?)?;
        Ok(newest_first(messages_in(data.as_ref())))
    }

    /// The newest `limit` (1 to 20) shared items of each kind of the conversation `token`, by kind
    /// (capability `rich-object-list-media`).
    pub async fn shared_overview(
        &self,
        token: &RoomToken,
        limit: u32,
    ) -> Result<Vec<(String, ChatMessage)>, TalkError> {
        let query = [("limit", limit.clamp(1, 20).to_string())];
        let url = self.endpoint(&format!("{CHAT}/{token}/share/overview"), &[], &query)?;
        let data = shared_answer(self.exchange(Method::GET, url, None, None).await?)?;
        let Some(kinds) = data.as_ref().and_then(Value::as_object) else { return Ok(Vec::new()) };
        Ok(kinds
            .iter()
            .filter(|(kind, _)| is_kind(kind))
            .flat_map(|(kind, items)| {
                newest_first(messages_in(Some(items))).into_iter().map(move |message| (kind.clone(), message))
            })
            .collect())
    }

    /// The WebDAV address of `path` of `user`.
    fn dav_url(&self, user: &str, path: &DavPath) -> Result<Url, FilesError> {
        if !is_segment(user) {
            return Err(FilesError::Invalid);
        }
        let mut segments = vec![user];
        segments.extend(path.segments().iter().map(String::as_str));
        Ok(self.endpoint::<&str>("/remote.php/dav/files", &segments, &[])?)
    }

    /// The address of the chunked upload `upload` of `user`, or of `part` in it.
    fn upload_url(&self, user: &str, upload: &str, part: Option<&str>) -> Result<Url, FilesError> {
        let upload_fits =
            (1..=64).contains(&upload.len()) && upload.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !is_segment(user) || !upload_fits {
            return Err(FilesError::Invalid);
        }
        let mut segments = vec![user, upload];
        segments.extend(part);
        Ok(self.endpoint::<&str>("/remote.php/dav/uploads", &segments, &[])?)
    }

    /// `request` with the headers every step of a chunked upload carries: `Destination` (the target's
    /// full address) and `OC-Total-Length`.
    fn destined(
        &self,
        request: reqwest::RequestBuilder,
        user: &str,
        target: &DavPath,
        total: u64,
    ) -> Result<reqwest::RequestBuilder, FilesError> {
        let destination =
            HeaderValue::from_str(self.dav_url(user, target)?.as_str()).map_err(|_| FilesError::Invalid)?;
        Ok(request.header("Destination", destination).header("OC-Total-Length", total.to_string()))
    }

    /// A link card of the answer of `references/resolve`; `None` without a title.
    fn read_reference(&self, reference: &Value) -> Option<Reference> {
        let card = reference.get("openGraphObject")?;
        let text = |key: &str| {
            card.get(key)
                .and_then(Value::as_str)
                .map(|text| crate::rich::line(text, &Value::Null))
                .filter(|text| !text.is_empty())
        };
        let title = text("name")?;
        let image = card
            .get("thumb")
            .and_then(Value::as_str)
            .and_then(|thumb| self.own_link(thumb))
            .and_then(|thumb| Url::parse(&thumb).ok())
            .and_then(|thumb| {
                let id = thumb
                    .path()
                    .strip_suffix('/')
                    .unwrap_or(thumb.path())
                    .rsplit_once("/core/references/preview/")?
                    .1
                    .to_string();
                is_reference_id(&id).then_some(id)
            });
        Some(Reference { title, description: text("description"), image })
    }
}

/// The method of the bytes `name` (`PROPFIND`, `MKCOL`, `MOVE`).
fn method(name: &[u8]) -> Result<Method, FilesError> {
    Method::from_bytes(name).map_err(|_| FilesError::Invalid)
}

/// Sends `request` (with `wait` instead of the client's time limit) and gives the headers of a
/// success, else the error of its status.
async fn status(request: reqwest::RequestBuilder, wait: Option<Duration>) -> Result<HeaderMap, FilesError> {
    let request = match wait {
        Some(wait) => request.timeout(wait),
        None => request,
    };
    let response = request.send().await.map_err(unreachable)?;
    let code = response.status().as_u16();
    match code {
        200..=299 => Ok(response.headers().clone()),
        code => Err(dav_refusal(code, response.headers())),
    }
}

/// The body of `request`'s answer, at most `max` bytes, with its media type.
async fn read_body(request: reqwest::RequestBuilder, max: u64) -> Result<(Vec<u8>, String), FilesError> {
    let mut response = request.send().await.map_err(unreachable)?;
    let code = response.status().as_u16();
    if !(200..=299).contains(&code) {
        return Err(dav_refusal(code, response.headers()));
    }
    if response.content_length().is_some_and(|length| length > max) {
        return Err(FilesError::TooLarge);
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or_default().trim().to_ascii_lowercase())
        .unwrap_or_default();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(unreachable)? {
        if bytes.len() as u64 + chunk.len() as u64 > max {
            return Err(FilesError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, content_type))
}

/// A picture of the server: PNG, JPEG, GIF or WebP by the answer's media type, at most
/// [`PREVIEW_MAX_BYTES`].
async fn picture(request: reqwest::RequestBuilder) -> Result<Picture, FilesError> {
    let (bytes, content_type) = read_body(request, PREVIEW_MAX_BYTES).await?;
    match content_type.as_str() {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" if !bytes.is_empty() => {
            Ok(Picture { bytes, content_type })
        }
        _ => Err(FilesError::Invalid),
    }
}

/// The error of a WebDAV answer with `code`: the ones every call shares through [`refusal`], the
/// rest by what they mean for files.
fn dav_refusal(code: u16, headers: &HeaderMap) -> FilesError {
    match code {
        403 => FilesError::Forbidden,
        404 | 409 => FilesError::NotFound,
        412 => FilesError::Exists,
        413 => FilesError::TooLarge,
        507 => FilesError::NoSpace,
        400 | 405 | 415 => FilesError::Invalid,
        code => FilesError::Ocs(refusal(code, headers)),
    }
}

/// The id and `ETag` an upload's answer names.
fn uploaded(headers: &HeaderMap) -> Uploaded {
    let text = |name: &str| headers.get(name).and_then(|value| value.to_str().ok()).map(str::trim);
    // `OC-FileId` is the id padded to eight digits with the instance id behind (`00000123oc1a2b3c`).
    let file_id = text("oc-fileid").and_then(|id| {
        let digits: String = id.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok().filter(|id| *id > 0)
    });
    let etag = text("oc-etag").or_else(|| text("etag")).filter(|etag| !etag.is_empty()).map(str::to_string);
    Uploaded { file_id, etag }
}

/// `name` for the `copy`-th file of that name: itself for 1, else „stem (copy).ext“; the extension
/// is what follows the last `.` that is not the first character.
pub fn numbered(name: &str, copy: u32) -> String {
    if copy <= 1 {
        return name.to_string();
    }
    match name.rfind('.').filter(|at| *at > 0) {
        Some(at) => format!("{} ({copy}){}", &name[..at], &name[at..]),
        None => format!("{name} ({copy})"),
    }
}

/// Whether `id` can be the id of a reference preview: 1 to 128 of `a`–`z`, `A`–`Z`, `0`–`9`.
fn is_reference_id(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Whether `kind` is a plain word of Talk's shared item types (`media`, `deckcard` …).
fn is_kind(kind: &str) -> bool {
    (1..=32).contains(&kind.len()) && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_' || b == b'-')
}

/// The `data` of a shared items answer, or the refusal as a [`TalkError`].
fn shared_answer(answer: crate::ocs::Answer) -> Result<Option<Value>, TalkError> {
    match answer.status {
        200..=299 | 304 => Ok(answer.data),
        404 => Err(TalkError::NotFound),
        400 | 405 => Err(TalkError::Invalid),
        403 => Err(TalkError::Ocs(OcsError::Forbidden)),
        status => Err(TalkError::Ocs(OcsError::Other(format!("Nextcloud answered with HTTP {status}")))),
    }
}

/// The messages of a map (id → message, as PHP sends it) or a list; nothing for anything else.
fn messages_in(data: Option<&Value>) -> Vec<ChatMessage> {
    match data {
        Some(Value::Object(map)) => map.values().filter_map(read_message).collect(),
        Some(Value::Array(list)) => list.iter().filter_map(read_message).collect(),
        _ => Vec::new(),
    }
}

/// `messages` the newest first, each id once.
fn newest_first(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    messages.sort_by_key(|message| std::cmp::Reverse(message.id));
    messages.dedup_by_key(|message| message.id);
    messages
}

#[cfg(test)]
mod tests {
    use httpmock::Method::{DELETE, GET, POST, PUT};
    use httpmock::MockServer;

    use super::*;
    use crate::ocs::tests::{AUTHORIZATION, account};

    const SHARED: &str = include_str!("../tests/fixtures/talk_shared.json");
    const RESOLVED: &str = include_str!("../tests/fixtures/references_resolve.json");

    fn path(text: &str) -> DavPath {
        DavPath::parse(text).expect("a path")
    }

    fn token() -> RoomToken {
        RoomToken::parse("x7k2m9qa").expect("a token")
    }

    #[test]
    fn paths_are_checked_segments() {
        assert_eq!(path("/Talk/Plan.pdf").segments(), ["Talk", "Plan.pdf"]);
        assert_eq!(path("Talk//Plan.pdf/").as_path(), "/Talk/Plan.pdf");
        assert_eq!(DavPath::parse("/").map(|p| p.as_path()), Some("/".into()));
        for bad in ["/Talk/../etc", "/./x", "/a\u{0}b", "/a\nb", "/a\\b"] {
            assert_eq!(DavPath::parse(bad), None, "{bad:?}");
        }
        assert_eq!(path("/Talk").join("a/b"), None);
        assert_eq!(path("/Talk").join(".."), None);
        assert_eq!(path("/Talk").join("x.png").map(|p| p.as_path()), Some("/Talk/x.png".into()));
        assert_eq!(path("/a/b").levels().iter().map(DavPath::as_path).collect::<Vec<_>>(), ["/a", "/a/b"]);
    }

    #[test]
    fn copies_are_numbered_before_the_extension() {
        assert_eq!(numbered("Plan.pdf", 1), "Plan.pdf");
        assert_eq!(numbered("Plan.pdf", 2), "Plan (2).pdf");
        assert_eq!(numbered("archive.tar.gz", 3), "archive.tar (3).gz");
        assert_eq!(numbered(".hidden", 2), ".hidden (2)");
        assert_eq!(numbered("README", 2), "README (2)");
    }

    #[tokio::test]
    async fn a_new_file_never_replaces_one() {
        let server = MockServer::start_async().await;
        let put = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/remote.php/dav/files/anna/Talk/Plan%20A.pdf")
                    .header("authorization", AUTHORIZATION)
                    .header("if-none-match", "*")
                    .body("%PDF-1.7");
                then.status(201).header("OC-FileId", "00000123oc1a2b3c").header("OC-ETag", "\"e1\"");
            })
            .await;
        let done = account(&server).put_file("anna", &path("/Talk/Plan A.pdf"), b"%PDF-1.7".to_vec()).await;
        assert_eq!(done, Ok(Uploaded { file_id: Some(123), etag: Some("\"e1\"".into()) }));
        assert_eq!(put.calls_async().await, 1);

        let taken = MockServer::start_async().await;
        taken
            .mock_async(|when, then| {
                when.method(PUT).path("/remote.php/dav/files/anna/Talk/x.png");
                then.status(412);
            })
            .await;
        assert_eq!(account(&taken).put_file("anna", &path("/Talk/x.png"), vec![1]).await, Err(FilesError::Exists));
        let full = MockServer::start_async().await;
        full.mock_async(|when, then| {
            when.method(PUT).path("/remote.php/dav/files/anna/Talk/x.png");
            then.status(507);
        })
        .await;
        assert_eq!(account(&full).put_file("anna", &path("/Talk/x.png"), vec![1]).await, Err(FilesError::NoSpace));
    }

    #[tokio::test]
    async fn a_free_name_is_found_with_propfind() {
        let server = MockServer::start_async().await;
        // PROPFIND is no method httpmock names: the paths tell the calls apart.
        let first = server
            .mock_async(|when, then| {
                when.path("/remote.php/dav/files/anna/Talk/Plan.pdf").header("depth", "0");
                then.status(207).body("<d:multistatus xmlns:d=\"DAV:\"/>");
            })
            .await;
        let second = server
            .mock_async(|when, then| {
                when.path("/remote.php/dav/files/anna/Talk/Plan%20(2).pdf").header("depth", "0");
                then.status(404);
            })
            .await;
        let free = account(&server).free_name("anna", &path("/Talk"), "Plan.pdf").await;
        assert_eq!(free, Ok("Plan (2).pdf".into()));
        assert_eq!((first.calls_async().await, second.calls_async().await), (1, 1));
    }

    #[tokio::test]
    async fn folders_are_made_level_by_level() {
        let server = MockServer::start_async().await;
        let talk = server
            .mock_async(|when, then| {
                when.path("/remote.php/dav/files/anna/Talk");
                then.status(405);
            })
            .await;
        let sub = server
            .mock_async(|when, then| {
                when.path("/remote.php/dav/files/anna/Talk/Team-abcd");
                then.status(201);
            })
            .await;
        assert_eq!(account(&server).make_folders("anna", &path("/Talk/Team-abcd")).await, Ok(()));
        assert_eq!((talk.calls_async().await, sub.calls_async().await), (1, 1));
    }

    #[tokio::test]
    async fn a_chunked_upload_names_its_destination_and_never_overwrites() {
        let server = MockServer::start_async().await;
        let base = server.base_url();
        let destination = format!("{base}/remote.php/dav/files/anna/Talk/Film.mp4");
        let start = server
            .mock_async(|when, then| {
                when.path("/remote.php/dav/uploads/anna/torro-1a2b")
                    .header("destination", &destination)
                    .header("oc-total-length", "12");
                then.status(201);
            })
            .await;
        let piece = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/remote.php/dav/uploads/anna/torro-1a2b/1")
                    .header("destination", &destination)
                    .header("oc-total-length", "12")
                    .body("twelve bytes");
                then.status(201);
            })
            .await;
        let finish = server
            .mock_async(|when, then| {
                when.path("/remote.php/dav/uploads/anna/torro-1a2b/.file")
                    .header("destination", &destination)
                    .header("overwrite", "F");
                then.status(201).header("OC-FileId", "00000777ocx").header("ETag", "\"e7\"");
            })
            .await;
        let nextcloud = account(&server);
        let target = path("/Talk/Film.mp4");
        nextcloud.start_upload("anna", "torro-1a2b", &target, 12).await.expect("started");
        nextcloud.put_chunk("anna", "torro-1a2b", 1, b"twelve bytes".to_vec(), &target, 12).await.expect("a piece");
        let done = nextcloud.finish_upload("anna", "torro-1a2b", &target, 12).await;
        assert_eq!(done, Ok(Uploaded { file_id: Some(777), etag: Some("\"e7\"".into()) }));
        assert_eq!((start.calls_async().await, piece.calls_async().await, finish.calls_async().await), (1, 1, 1));
        assert_eq!(nextcloud.put_chunk("anna", "torro-1a2b", 0, vec![1], &target, 1).await, Err(FilesError::Invalid));
        assert_eq!(nextcloud.start_upload("anna", "../x", &target, 1).await, Err(FilesError::Invalid));
    }

    #[tokio::test]
    async fn only_the_file_with_its_etag_is_deleted() {
        let server = MockServer::start_async().await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE).path("/remote.php/dav/files/anna/Talk/x.png").header("if-match", "\"e1\"");
                then.status(204);
            })
            .await;
        let cancel = server
            .mock_async(|when, then| {
                when.method(DELETE).path("/remote.php/dav/uploads/anna/torro-1a2b");
                then.status(404);
            })
            .await;
        let nextcloud = account(&server);
        assert_eq!(nextcloud.delete_file("anna", &path("/Talk/x.png"), "\"e1\"").await, Ok(()));
        assert_eq!(nextcloud.cancel_upload("anna", "torro-1a2b").await, Ok(()), "gone already is fine");
        assert_eq!((delete.calls_async().await, cancel.calls_async().await), (1, 1));
        let changed = MockServer::start_async().await;
        changed
            .mock_async(|when, then| {
                when.method(DELETE).path("/remote.php/dav/files/anna/Talk/x.png");
                then.status(412);
            })
            .await;
        assert_eq!(
            account(&changed).delete_file("anna", &path("/Talk/x.png"), "\"e1\"").await,
            Err(FilesError::Exists)
        );
    }

    #[tokio::test]
    async fn a_download_stops_at_its_limit() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/remote.php/dav/files/anna/Talk/notes.txt")
                    .header("authorization", AUTHORIZATION);
                then.status(200).header("content-type", "text/plain").body("0123456789");
            })
            .await;
        let nextcloud = account(&server);
        assert_eq!(nextcloud.get_file("anna", &path("/Talk/notes.txt"), 10).await, Ok(b"0123456789".to_vec()));
        assert_eq!(nextcloud.get_file("anna", &path("/Talk/notes.txt"), 9).await, Err(FilesError::TooLarge));
    }

    #[tokio::test]
    async fn previews_are_raster_pictures_only() {
        let server = MockServer::start_async().await;
        let png = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/index.php/core/preview")
                    .query_param("fileId", "123")
                    .query_param("x", "256")
                    .query_param("y", "256")
                    .query_param("a", "1")
                    .query_param_missing("mimeFallback");
                then.status(200).header("content-type", "image/png").body("\u{89}PNG");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/index.php/core/preview").query_param("fileId", "9");
                then.status(200).header("content-type", "image/svg+xml").body("<svg/>");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/index.php/core/preview").query_param("fileId", "8");
                then.status(404);
            })
            .await;
        let nextcloud = account(&server);
        let picture = nextcloud.preview(123, 256).await.expect("a preview");
        assert_eq!(picture.content_type, "image/png");
        assert_eq!(png.calls_async().await, 1);
        assert_eq!(nextcloud.preview(9, 256).await, Err(FilesError::Invalid), "never an SVG");
        assert_eq!(nextcloud.preview(8, 256).await, Err(FilesError::NotFound));
        assert_eq!(nextcloud.reference_preview("../x").await, Err(FilesError::Invalid));
    }

    #[tokio::test]
    async fn a_share_carries_its_meta_data_as_text() {
        let server = MockServer::start_async().await;
        let share = server
            .mock_async(|when, then| {
                when.method(POST).path(SHARES).header("ocs-apirequest", "true").json_body(json!({
                    "shareType": 10,
                    "shareWith": "x7k2m9qa",
                    "path": "/Talk/Plan.pdf",
                    "talkMetaData": "{\"caption\":\"The plan\",\"referenceId\":\"abc\",\"replyTo\":5,\"silent\":true}"
                }));
                then.status(200).body(r#"{"ocs":{"meta":{},"data":{"id":"7"}}}"#);
            })
            .await;
        let meta = ShareMeta {
            caption: Some("The plan".into()),
            silent: true,
            reply_to: Some(5),
            reference_id: Some("abc".into()),
        };
        assert_eq!(account(&server).share_to_room(&token(), &path("/Talk/Plan.pdf"), &meta).await, Ok(()));
        assert_eq!(share.calls_async().await, 1);
        let refused = MockServer::start_async().await;
        refused
            .mock_async(|when, then| {
                when.method(POST).path(SHARES);
                then.status(403).body(r#"{"ocs":{"meta":{},"data":[]}}"#);
            })
            .await;
        let plain = ShareMeta::default();
        assert_eq!(
            account(&refused).share_to_room(&token(), &path("/Talk/Plan.pdf"), &plain).await,
            Err(FilesError::Forbidden)
        );
    }

    #[tokio::test]
    async fn a_reference_keeps_only_a_picture_of_its_own_server() {
        let server = MockServer::start_async().await;
        let base = server.base_url();
        let answer = RESOLVED.replace("{base}", &base);
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/ocs/v2.php/references/resolve")
                    .query_param("reference", "https://example.org/a");
                then.status(200).body(&answer);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/ocs/v2.php/references/resolve")
                    .query_param("reference", "https://example.org/b");
                then.status(200).body(&answer);
            })
            .await;
        let nextcloud = account(&server);
        let own = nextcloud.resolve_reference("https://example.org/a").await.expect("asked").expect("a card");
        assert_eq!(own.title, "Example article");
        assert_eq!(own.description.as_deref(), Some("What it is about"));
        assert_eq!(own.image.as_deref(), Some("0f1e2d3c4b5a69788796a5b4c3d2e1f0"));
        let foreign = nextcloud.resolve_reference("https://example.org/b").await.expect("asked").expect("a card");
        assert_eq!(foreign.image, None, "a picture elsewhere is never taken");
    }

    #[tokio::test]
    async fn shared_items_come_newest_first() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/ocs/v2.php/apps/spreed/api/v1/chat/x7k2m9qa/share")
                    .query_param("objectType", "media")
                    .query_param("lastKnownMessageId", "200");
                then.status(200).body(SHARED);
            })
            .await;
        let found = account(&server).shared_items(&token(), "media", Some(200), 50).await.expect("items");
        assert_eq!(found.iter().map(|m| m.id).collect::<Vec<_>>(), [120, 110]);
        assert_eq!(found[0].files.first().map(|file| file.id), Some(4501));
        assert_eq!(account(&server).shared_items(&token(), "../x", None, 5).await, Err(TalkError::Invalid));
    }
}
