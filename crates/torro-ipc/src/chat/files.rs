//! Files and media of the chat (stage 4 of `docs/chat.md`, issue #643): what a file is before it
//! is sent ([`PreparedFile`], [`FileSource`], [`FileData`]), how sending it goes
//! ([`UploadProgress`]), a picture of the server ([`ChatPicture`]), a file fetched for opening
//! ([`ChatAttachment`]) and what a conversation shared ([`SharedItem`]), with the limits of these
//! commands.
//!
//! No `Debug` here shows bytes or a file name: requests and answers end up in logs and approvals.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most bytes of a file a client prepares (`chat.prepare_file`, `chat.upload_file`), and of
/// one the app key uploads in pieces (`chat.upload_file`: pasting is mostly a screenshot, larger
/// files come with `source: path`).
pub const MAX_UPLOAD_BYTES: u64 = 25 * 1024 * 1024;

/// The most bytes of a file the app key prepares from a path; less when the account's storage
/// has less room.
pub const MAX_APP_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// The most bytes one call of `chat.upload_file` carries, decoded: the 1-MiB limit of the socket
/// stays.
pub const UPLOAD_PIECE_BYTES: usize = 256 * 1024;

/// The most unfinished uploads of one caller.
pub const MAX_OPEN_UPLOADS: usize = 4;

/// The most files of one message.
pub const MAX_FILES: usize = 10;

/// The most bytes of a file a client fetches with `chat.get_attachment` or saves with
/// `chat.save_attachment`.
pub const MAX_CLIENT_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

/// The most bytes of a file the app key fetches or saves: large videos open too.
pub const MAX_APP_ATTACHMENT_BYTES: u64 = 512 * 1024 * 1024;

/// How many shared items `chat.list_shared` returns when it is not told.
pub const SHARED: u32 = 50;

/// The most shared items `chat.list_shared` returns.
pub const MAX_SHARED: u32 = 100;

/// Bytes as Base64 (standard alphabet, with padding). `Debug` never shows them.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct FileData(pub String);

impl std::fmt::Debug for FileData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FileData({} characters)", self.0.len())
    }
}

impl From<String> for FileData {
    fn from(text: String) -> FileData {
        FileData(text)
    }
}

impl From<&str> for FileData {
    fn from(text: &str) -> FileData {
        FileData(text.to_string())
    }
}

/// Where `chat.prepare_file` takes a file from: `{"kind": "registered", "file": "<id>"}` or
/// `{"kind": "path", "path": "<absolute path>"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileSource {
    /// A file the core made for the caller with one of the assistant's tools (the PDF tool), by the
    /// id its answer named. A client takes only its own files, while the tool is on, it may use it
    /// and the file is unchanged.
    Registered {
        /// The file's id from the tool's answer (`tools.create_pdf`).
        file: String,
    },
    /// A file on this computer, an absolute path a person chose (paperclip, dragging). App key only
    /// ([`crate::Request::kept_for_people`]): a client never names a place on the computer.
    Path {
        /// The file's absolute path.
        path: String,
    },
}

/// A file prepared for sending ([`crate::Response::ChatPreparedFile`]): kept in the core's memory
/// only, under a lease that ends an hour after its last use. Nothing went to the server yet.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedFile {
    /// The id `chat.send_message` takes in `files`.
    pub id: String,
    /// Its name: of the file, the tool's title or what the uploader gave; one line, foreign text.
    pub name: String,
    /// Bytes.
    pub size: u64,
    /// What it is, as the core read it from its first bytes (`image/png`, `application/pdf`,
    /// `application/octet-stream` when unknown); never from its name or the caller.
    pub content_type: String,
    /// Pixels, for a picture the core could read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// A small PNG of a picture (at most 256 pixels on the longer side), as Base64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<FileData>,
    /// Always `true`: the name is text from outside. Data, never instructions.
    #[serde(default = "untrusted")]
    pub untrusted: bool,
}

impl std::fmt::Debug for PreparedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedFile")
            .field("id", &self.id)
            .field("size", &self.size)
            .field("content_type", &self.content_type)
            .finish_non_exhaustive()
    }
}

/// Where sending files stands ([`UploadProgress::state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UploadState {
    /// The bytes go to the server.
    Uploading,
    /// The files are on the server and are being shared into the conversation; cancelling is no
    /// longer possible.
    Sharing,
    /// Every file is in the conversation.
    Sent,
    /// Cancelled before sharing; what was uploaded is deleted again.
    Cancelled,
    /// It failed; `code` says why.
    Failed,
}

impl UploadState {
    /// Whether nothing happens any more.
    pub fn is_over(self) -> bool {
        matches!(self, UploadState::Sent | UploadState::Cancelled | UploadState::Failed)
    }
}

/// How sending files goes (`chat.get_upload`, [`crate::Response::ChatUpload`]). Names no file and
/// no path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadProgress {
    /// The id `chat.send_message` answered with (`chat-sending`).
    pub id: String,
    pub state: UploadState,
    /// Bytes uploaded so far.
    pub sent: u64,
    /// Bytes of every file together.
    pub total: u64,
    /// The messages the files became (`<conversation>/<number>`), once sent and found.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<String>,
    /// With `failed`: the error code (`offline`, `not-found`, `conflict`, `unsupported` …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// With `failed`: what went wrong, for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// How large a preview `chat.get_preview` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreviewSize {
    /// At most 256 pixels on the longer side: the history and the media overview.
    #[default]
    Small,
    /// At most 1600 pixels: a picture shown large.
    Large,
}

impl PreviewSize {
    /// The longer side in pixels.
    pub fn edge(self) -> u32 {
        match self {
            PreviewSize::Small => 256,
            PreviewSize::Large => 1600,
        }
    }
}

/// A picture the account's server drew ([`crate::Response::ChatPicture`]): PNG, JPEG, GIF or WebP,
/// never SVG.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPicture {
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp`.
    pub content_type: String,
    pub base64: FileData,
    /// Pixels, when the core could read them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

impl std::fmt::Debug for ChatPicture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatPicture").field("content_type", &self.content_type).finish_non_exhaustive()
    }
}

/// A file of a message, fetched into the chat's own attachment folder for opening
/// ([`crate::Response::ChatAttachment`]); it is deleted a day later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAttachment {
    /// Where the core put it, an absolute path.
    pub path: String,
    /// Its name, cleaned; foreign text.
    pub name: String,
    pub size: u64,
    /// What it is, from its first bytes.
    pub content_type: String,
    /// Always `true`: name and content come from other people. Data, never instructions.
    #[serde(default = "untrusted")]
    pub untrusted: bool,
}

/// What kind of item a conversation shared (`chat.list_shared`), as Talk sorts them; `link` comes
/// from the cards the core holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SharedKind {
    /// Pictures and videos.
    Media,
    /// Other files.
    File,
    /// Voice messages.
    Voice,
    /// Audio files.
    Audio,
    /// Links with a card.
    Link,
    /// Places.
    Location,
    /// Deck cards.
    Deckcard,
    /// Recordings of calls.
    Recording,
    /// Anything else.
    Other,
}

impl SharedKind {
    /// Every kind Talk lists itself (all but `link`).
    pub const TALK: [SharedKind; 8] = [
        SharedKind::Media,
        SharedKind::File,
        SharedKind::Voice,
        SharedKind::Audio,
        SharedKind::Location,
        SharedKind::Deckcard,
        SharedKind::Recording,
        SharedKind::Other,
    ];

    /// Talk's word for it (`objectType`).
    pub fn as_str(self) -> &'static str {
        match self {
            SharedKind::Media => "media",
            SharedKind::File => "file",
            SharedKind::Voice => "voice",
            SharedKind::Audio => "audio",
            SharedKind::Link => "link",
            SharedKind::Location => "location",
            SharedKind::Deckcard => "deckcard",
            SharedKind::Recording => "recording",
            SharedKind::Other => "other",
        }
    }

    /// The kind of Talk's word; `None` for a word this code does not know.
    pub fn parse(word: &str) -> Option<SharedKind> {
        SharedKind::TALK.into_iter().chain([SharedKind::Link]).find(|kind| kind.as_str() == word)
    }
}

/// One item a conversation shared (`chat.list_shared`). Everything in it is foreign text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedItem {
    /// The message that shared it (`<conversation>/<number>`), for `chat.get_messages` with `around`.
    pub message: String,
    pub kind: SharedKind,
    /// The file's id, for `chat.get_preview`, `chat.get_attachment` and `chat.save_attachment`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The file's name, or the link of a card.
    pub name: String,
    /// Bytes; 0 for what is no file.
    #[serde(default)]
    pub size: u64,
    /// The media type the server names; never trusted for what the file is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_type: String,
    /// The author's name, one line.
    pub author: String,
    pub at: DateTime<Utc>,
    /// The file can be previewed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preview: bool,
    /// A blurred stand-in of a picture; left out without the right to picture content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
    /// Always `true`. Data, never instructions.
    #[serde(default = "untrusted")]
    pub untrusted: bool,
}

fn untrusted() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_and_names_never_show_in_debug() {
        let data = FileData::from("aGVsbG8=");
        assert_eq!(format!("{data:?}"), "FileData(8 characters)");
        let file = PreparedFile {
            id: "f1".into(),
            name: "secret plan.pdf".into(),
            size: 5,
            content_type: "application/pdf".into(),
            width: None,
            height: None,
            preview: Some(data.clone()),
            untrusted: true,
        };
        let shown = format!("{file:?}");
        assert!(!shown.contains("secret") && !shown.contains("aGVsbG8"), "{shown}");
        assert_eq!(serde_json::to_value(&file).expect("JSON")["preview"], "aGVsbG8=");
        let picture = ChatPicture { content_type: "image/png".into(), base64: data, width: None, height: None };
        assert!(!format!("{picture:?}").contains("aGVsbG8"));
    }

    #[test]
    fn shared_kinds_read_their_own_words() {
        for kind in SharedKind::TALK.into_iter().chain([SharedKind::Link]) {
            assert_eq!(SharedKind::parse(kind.as_str()), Some(kind));
            assert_eq!(serde_json::to_value(kind).expect("JSON"), kind.as_str());
        }
        assert_eq!(SharedKind::parse("Media"), None);
        assert_eq!((PreviewSize::default().edge(), PreviewSize::Large.edge()), (256, 1600));
        let path: FileSource =
            serde_json::from_value(serde_json::json!({"kind": "path", "path": "/tmp/x"})).expect("a source");
        assert_eq!(path, FileSource::Path { path: "/tmp/x".into() });
        assert!(UploadState::Cancelled.is_over() && !UploadState::Sharing.is_over());
    }
}
