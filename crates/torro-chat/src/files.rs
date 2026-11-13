//! Files and media of a conversation as the windows see them (stage 4 of `docs/chat.md`, issue
//! #643): a file prepared for sending ([`PreparedFile`]), a piece of an upload ([`FilePiece`]), how
//! sending files goes ([`UploadProgress`]), a picture of the server ([`Picture`]), a file fetched for
//! opening ([`Fetched`]), where a file is saved ([`SaveTo`]) and what a conversation shared
//! ([`SharedItem`]).
//!
//! These are the windows' side of the commands `chat.prepare_file` … `chat.list_shared`. The wire
//! types of the API (`torro_ipc::chat`) carry the same facts with bytes as Base64; the core's client
//! turns the one into the other, so nothing here depends on the socket. Bytes here are the bytes
//! themselves (PNG, JPEG …), never Base64.
//!
//! No `Debug` here shows bytes, a file name or a path: they end up in logs.

use chrono::{DateTime, Utc};

/// How large a preview [`crate::ChatSource::preview`] asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PreviewSize {
    /// At most 256 pixels on the longer side: link cards and the media overview.
    #[default]
    Small,
    /// At most 1024 pixels: a picture in the chat window's history.
    Medium,
    /// At most 1600 pixels: a picture shown large.
    Large,
}

/// A file prepared for sending (`chat.prepare_file`, the last piece of `chat.upload_file`): it is in
/// the core's memory under a lease, nothing went to the server yet. [`crate::ChatSource::send_files`]
/// takes its `id`; [`crate::ChatSource::release_files`] drops it.
#[derive(Clone, PartialEq, Eq)]
pub struct PreparedFile {
    /// The id `send_files` takes.
    pub id: String,
    /// Its name, one line; foreign text when a client gave it.
    pub name: String,
    /// Bytes.
    pub size: u64,
    /// What it is, as the core read it from its first bytes (`image/png`, `application/pdf`,
    /// `application/octet-stream` when unknown).
    pub content_type: String,
    /// Pixels of a picture the core could read.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// A small PNG of a picture (at most 256 pixels on the longer side), the bytes themselves.
    pub preview: Option<Vec<u8>>,
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

/// One piece of a file uploaded into the core's memory ([`crate::ChatSource::upload_file`]): the
/// window's way to send a picture it holds itself (pasted from the clipboard).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct FilePiece {
    /// The account the file will be sent with (`nextcloud:<login>@<host>`).
    pub account: String,
    /// The upload the first piece started ([`Uploaded::Piece`]); `None` for the first piece.
    pub upload: Option<String>,
    /// Where this piece starts in the file, in bytes.
    pub offset: u64,
    /// The bytes of this piece, at most `torro_ipc::chat::UPLOAD_PIECE_BYTES`.
    pub bytes: Vec<u8>,
    /// The file's name, with the first piece.
    pub name: Option<String>,
    /// This is the last piece: the file is complete.
    pub last: bool,
}

impl std::fmt::Debug for FilePiece {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePiece")
            .field("upload", &self.upload)
            .field("offset", &self.offset)
            .field("bytes", &self.bytes.len())
            .field("last", &self.last)
            .finish_non_exhaustive()
    }
}

/// What the core answered to a piece ([`crate::ChatSource::upload_file`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uploaded {
    /// The piece arrived: the upload's id for the next piece, and how many bytes it holds.
    Piece { upload: String, received: u64 },
    /// The last piece arrived: the prepared file.
    File(PreparedFile),
}

/// What [`crate::ChatSource::send_files`] started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sending {
    /// The message went out at once (no files): its id.
    Sent(String),
    /// Uploading the files began: the id for [`crate::ChatSource::upload`] and
    /// [`crate::ChatSource::cancel_upload`].
    Uploading(String),
}

/// Where sending files stands ([`UploadProgress::state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UploadState {
    /// The bytes go to the server.
    Uploading,
    /// The files are on the server and are being shared; cancelling is no longer possible.
    Sharing,
    /// Every file is in the conversation.
    Sent,
    /// Cancelled before sharing; what was uploaded is deleted again.
    Cancelled,
    /// It failed; [`UploadProgress::code`] says why.
    Failed,
}

impl UploadState {
    /// Whether nothing happens any more: the window stops asking.
    pub fn is_over(self) -> bool {
        matches!(self, UploadState::Sent | UploadState::Cancelled | UploadState::Failed)
    }
}

/// How sending files goes (`chat.get_upload`). Names no file and no path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadProgress {
    /// The id [`Sending::Uploading`] named.
    pub id: String,
    pub state: UploadState,
    /// Bytes uploaded so far.
    pub sent: u64,
    /// Bytes of every file together.
    pub total: u64,
    /// The messages the files became (`<conversation>/<number>`), once sent.
    pub messages: Vec<String>,
    /// With [`UploadState::Failed`]: the error code of the core (`offline`, `conflict` …).
    pub code: Option<String>,
    /// With [`UploadState::Failed`]: what went wrong, in English for people.
    pub message: Option<String>,
}

/// A picture the account's server drew (`chat.get_preview`): PNG, JPEG, GIF or WebP, never SVG.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp`.
    pub content_type: String,
    /// The encoded picture.
    pub bytes: Vec<u8>,
    /// Pixels, when known.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture")
            .field("content_type", &self.content_type)
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// A file of a message fetched for opening (`chat.get_attachment`): the core put it into the chat's
/// own attachment folder and deletes it a day later. The window opens `path` with the system's
/// program for it.
#[derive(Clone, PartialEq, Eq)]
pub struct Fetched {
    /// Where the core put it, an absolute path.
    pub path: String,
    /// Its name, cleaned; foreign text.
    pub name: String,
    pub size: u64,
    /// What it is, from its first bytes.
    pub content_type: String,
}

impl std::fmt::Debug for Fetched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fetched")
            .field("size", &self.size)
            .field("content_type", &self.content_type)
            .finish_non_exhaustive()
    }
}

/// Where [`crate::ChatSource::save_attachment`] puts a file.
#[derive(Clone, PartialEq, Eq)]
pub enum SaveTo {
    /// The Downloads folder (setting `mail_downloads_folder`, else the system's); a taken name
    /// gets a number.
    Downloads,
    /// A save target of `mail.list_save_targets`, by its id.
    Target(String),
    /// Exactly this file, an absolute path a person chose in the system's save dialog; replaced
    /// when it exists (the dialog asked). App key only.
    File(String),
    /// An existing folder a person chose. App key only.
    Folder(String),
}

impl std::fmt::Debug for SaveTo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SaveTo::Downloads => "Downloads",
            SaveTo::Target(_) => "Target(…)",
            SaveTo::File(_) => "File(…)",
            SaveTo::Folder(_) => "Folder(…)",
        })
    }
}

/// What kind of item a conversation shared (`chat.list_shared`), as Talk sorts them; `Link` comes
/// from the cards the core holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// One item a conversation shared. Everything in it is foreign text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedItem {
    /// The message that shared it (`<conversation>/<number>`), for "Show in chat"
    /// ([`crate::ChatSource::messages_around`]).
    pub message: String,
    pub kind: SharedKind,
    /// The file's id, for [`crate::ChatSource::preview`] and its kin; `None` for a link.
    pub file: Option<String>,
    /// The file's name, or the link of a card.
    pub name: String,
    /// Bytes; 0 for what is no file.
    pub size: u64,
    /// The media type the server names; never trusted for what the file is.
    pub content_type: String,
    /// The author's name, one line.
    pub author: String,
    pub at: DateTime<Utc>,
    /// The server can draw a preview of it.
    pub preview: bool,
}

/// A page of [`SharedItem`]s, the newest first; `more` when older ones of the kind exist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SharedPage {
    pub items: Vec<SharedItem>,
    pub more: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_names_and_paths_never_show_in_debug() {
        let file = PreparedFile {
            id: "f1".into(),
            name: "secret plan.pdf".into(),
            size: 5,
            content_type: "application/pdf".into(),
            width: None,
            height: None,
            preview: Some(vec![1, 2, 3]),
        };
        assert!(!format!("{file:?}").contains("secret"));
        let piece = FilePiece { name: Some("secret.png".into()), bytes: vec![7; 9], ..FilePiece::default() };
        assert!(!format!("{piece:?}").contains("secret"));
        let fetched =
            Fetched { path: "/tmp/secret".into(), name: "secret".into(), size: 1, content_type: "text/plain".into() };
        assert!(!format!("{fetched:?}").contains("secret"));
        assert_eq!(format!("{:?}", SaveTo::File("/home/secret".into())), "File(…)");
        assert!(UploadState::Failed.is_over() && !UploadState::Uploading.is_over());
    }
}
