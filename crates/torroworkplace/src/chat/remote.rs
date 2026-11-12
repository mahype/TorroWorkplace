//! What the chat service asks of an account's server, in neutral terms: [`Remote`] is the one
//! door, with Nextcloud Talk behind it ([`super::talk`]) and a server in memory for the tests
//! ([`super::fake`]).
//!
//! The types here hold what the service needs and nothing of Talk's wire format; ids are the
//! server's own (a token, a message number, a user id), and the service makes the suite's ids of
//! them. Every text in them is foreign and was made plain by the provider.
//!
//! Files (stage 4) are named by checked paths ([`DavPath`]) and come back as [`Uploaded`],
//! [`Picture`] and [`Reference`] of `torro-nextcloud`: plain WebDAV and pictures, no wire format of
//! Talk. Their refusals are [`FileError`].

use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Arc;

use chrono::{DateTime, Utc};
use torro_chat::{ChatError, ConversationKind, FileRef, Mention, MessageKind, NotifyLevel, Preview, Status};
pub use torro_nextcloud::{DavPath, Picture, Reference, ShareMeta, Uploaded};

/// Why a call about a file failed: what every chat call can say, and what only files can.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    Chat(ChatError),
    /// A file of that name is there, or the file changed since the `ETag` was given.
    Exists,
    /// The person's storage on the server is full.
    NoSpace,
    /// The file is larger than asked for.
    TooLarge,
}

impl From<ChatError> for FileError {
    fn from(err: ChatError) -> FileError {
        FileError::Chat(err)
    }
}

/// What an account's server can do, read from its capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Caps {
    /// The server has a chat at all (Talk is installed and enabled for the person).
    pub chat: bool,
    /// The read marker can be set.
    pub read_marker: bool,
    /// A conversation can be marked unread again.
    pub unread: bool,
    /// Reading messages can leave the notifications alone.
    pub keep_notifications: bool,
    /// The most characters of a message.
    pub max_length: usize,
    /// Messages can be sent without notifying.
    pub silent: bool,
    pub reactions: bool,
    pub edit: bool,
    pub delete: bool,
    /// Messages can be searched.
    pub search: bool,
    /// The server tells the status of its users.
    pub status: bool,
    /// The person may share files into conversations (`attachments.allowed`).
    pub attachments: bool,
    /// The person's folder for shared files (`attachments.folder`, `/Talk` when not told).
    pub attachments_folder: String,
    /// Files go into a folder per conversation and person below it.
    pub subfolders: bool,
    /// The largest piece of a chunked upload; `None` (or not positive on the server) means one `PUT`.
    pub chunk_max: Option<u64>,
    /// A file can carry a text and be shared silently (`media-caption`).
    pub caption: bool,
    /// Shared items can be listed (`rich-object-list-media`).
    pub shared: bool,
    /// Messages and shares can carry an id against repeats (`chat-reference-id`).
    pub reference_ids: bool,
    /// Messages around one can be read (`chat-get-context`).
    pub context: bool,
}

/// One conversation as the server lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRoom {
    pub token: String,
    pub name: String,
    pub kind: ConversationKind,
    pub unread: u64,
    pub mentioned: bool,
    pub mentioned_directly: bool,
    pub last_message: Option<Preview>,
    /// The number of the last message, when the server names it.
    pub last_number: Option<u64>,
    pub last_activity: DateTime<Utc>,
    /// The number of the message the person read last.
    pub last_read: Option<u64>,
    /// In a direct chat: the other person's user id.
    pub other_user: Option<String>,
    pub has_call: bool,
    pub favorite: bool,
    pub read_only: bool,
    pub federated: bool,
    pub notify: NotifyLevel,
}

/// A change one message makes to another ([`RemoteMessage::patch`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// The message that changes.
    pub number: u64,
    /// Its new text, after an edit.
    pub text: Option<String>,
    /// It was deleted.
    pub deleted: bool,
}

/// The message another one answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteQuote {
    pub number: u64,
    pub author: String,
    pub text: String,
}

/// One message as the server sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessage {
    pub number: u64,
    /// The author's user id when it is a user of this server.
    pub author: Option<String>,
    pub author_name: String,
    pub at: DateTime<Utc>,
    pub text: String,
    pub kind: MessageKind,
    /// Not shown: it only changes another message (an edit, a deletion, a reaction).
    pub hidden: bool,
    pub patch: Option<Patch>,
    pub mentions: Vec<Mention>,
    pub files: Vec<FileRef>,
    pub reply_to: Option<RemoteQuote>,
    pub reactions: BTreeMap<String, u64>,
    pub own_reactions: Vec<String>,
    pub edited: bool,
    /// Where each file of [`RemoteMessage::files`] lies in the person's files, by the file's id, as the
    /// server named it: the only source of a path to fetch a file by (stage 4).
    pub paths: BTreeMap<String, String>,
}

/// A person of the server: user id and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePerson {
    pub id: String,
    pub name: String,
}

/// A user's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteStatus {
    pub user: String,
    pub status: Status,
    pub message: Option<String>,
    pub icon: Option<String>,
    pub clear_at: Option<DateTime<Utc>>,
}

/// A message a search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHit {
    pub token: String,
    pub number: u64,
    pub title: String,
    pub text: String,
}

/// A message to send.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sending {
    pub text: String,
    pub reply_to: Option<u64>,
    /// A random id of this send, so the server drops a repeat.
    pub reference: String,
    pub silent: bool,
}

/// One account's chat server.
#[derive(Debug, Clone)]
pub enum Remote {
    Talk(Box<torro_nextcloud::Nextcloud>),
    #[cfg(test)]
    Fake(Arc<super::fake::Server>),
}

/// Hands every call to the provider behind `self`.
macro_rules! each {
    ($self:ident, $name:ident ( $($arg:expr),* )) => {
        match $self {
            Remote::Talk(client) => super::talk::$name(client $(, $arg)*).await,
            #[cfg(test)]
            Remote::Fake(server) => server.$name($($arg),*).await,
        }
    };
}

impl Remote {
    /// The person's user id and name on the server.
    pub async fn user(&self) -> Result<RemotePerson, ChatError> {
        each!(self, user())
    }

    pub async fn caps(&self) -> Result<Caps, ChatError> {
        each!(self, caps())
    }

    pub async fn rooms(&self) -> Result<Vec<RemoteRoom>, ChatError> {
        each!(self, rooms())
    }

    /// Up to `limit` messages before `before` (else the newest), the oldest first. Changes
    /// nothing on the server.
    pub async fn messages(
        &self,
        token: &str,
        before: Option<u64>,
        limit: u32,
        caps: &Caps,
    ) -> Result<Vec<RemoteMessage>, ChatError> {
        each!(self, messages(token, before, limit, caps))
    }

    /// Waits up to `seconds` for messages after `after`; empty when none came.
    pub async fn wait(
        &self,
        token: &str,
        after: u64,
        seconds: u32,
        caps: &Caps,
    ) -> Result<Vec<RemoteMessage>, ChatError> {
        each!(self, wait(token, after, seconds, caps))
    }

    pub async fn send(&self, token: &str, sending: &Sending) -> Result<RemoteMessage, ChatError> {
        each!(self, send(token, sending))
    }

    pub async fn mark_read(&self, token: &str, up_to: u64) -> Result<(), ChatError> {
        each!(self, mark_read(token, up_to))
    }

    pub async fn mark_unread(&self, token: &str) -> Result<(), ChatError> {
        each!(self, mark_unread(token))
    }

    pub async fn mentions(&self, token: &str, query: &str, limit: u32) -> Result<Vec<Mention>, ChatError> {
        each!(self, mentions(token, query, limit))
    }

    /// People of the server who can be written to, by name.
    pub async fn find_people(&self, query: &str, limit: u32) -> Result<Vec<RemotePerson>, ChatError> {
        each!(self, find_people(query, limit))
    }

    /// The direct chat with `user`, made when there is none.
    pub async fn create_direct(&self, user: &str) -> Result<RemoteRoom, ChatError> {
        each!(self, create_direct(user))
    }

    pub async fn edit(&self, token: &str, number: u64, text: &str) -> Result<(), ChatError> {
        each!(self, edit(token, number, text))
    }

    pub async fn delete(&self, token: &str, number: u64) -> Result<(), ChatError> {
        each!(self, delete(token, number))
    }

    pub async fn react(&self, token: &str, number: u64, emoji: &str, remove: bool) -> Result<(), ChatError> {
        each!(self, react(token, number, emoji, remove))
    }

    pub async fn set_favorite(&self, token: &str, favorite: bool) -> Result<(), ChatError> {
        each!(self, set_favorite(token, favorite))
    }

    pub async fn set_notify(&self, token: &str, level: NotifyLevel) -> Result<(), ChatError> {
        each!(self, set_notify(token, level))
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<RemoteHit>, ChatError> {
        each!(self, search(query, limit))
    }

    /// The status of every user the server tells, page by page until it has no more.
    pub async fn statuses(&self) -> Result<Vec<RemoteStatus>, ChatError> {
        each!(self, statuses())
    }

    /// One user's status; `None` when the server tells none.
    pub async fn status_of(&self, user: &str) -> Result<Option<RemoteStatus>, ChatError> {
        each!(self, status_of(user))
    }

    pub async fn own_status(&self) -> Result<RemoteStatus, ChatError> {
        each!(self, own_status())
    }

    pub async fn set_status(&self, status: Status) -> Result<(), ChatError> {
        each!(self, set_status(status))
    }

    /// Sets the status message; `None` clears it.
    pub async fn set_status_message(
        &self,
        message: Option<&str>,
        icon: Option<&str>,
        clear_at: Option<DateTime<Utc>>,
    ) -> Result<(), ChatError> {
        each!(self, set_status_message(message, icon, clear_at))
    }

    /// Tells the server the person is at the computer (`online`) or idle (`away`).
    pub async fn heartbeat(&self, status: Status) -> Result<(), ChatError> {
        each!(self, heartbeat(status))
    }

    /// People of the server by name, whether or not they can be written to.
    pub async fn find_users(&self, query: &str, limit: u32) -> Result<Vec<RemotePerson>, ChatError> {
        each!(self, find_users(query, limit))
    }

    /// The server's base address (for telling links to it apart).
    pub fn server(&self) -> String {
        match self {
            Remote::Talk(client) => client.server().to_string(),
            #[cfg(test)]
            Remote::Fake(_) => super::fake::SERVER.to_string(),
        }
    }

    /// Free bytes in the person's storage; `None` when unlimited or not told.
    pub async fn free_space(&self) -> Result<Option<u64>, ChatError> {
        each!(self, free_space())
    }

    /// Up to `limit` messages before and after the message `number`, the oldest first.
    pub async fn around(&self, token: &str, number: u64, limit: u32) -> Result<Vec<RemoteMessage>, ChatError> {
        each!(self, around(token, number, limit))
    }

    /// `name` in `folder` of `user`, or the first free „name (2)“ …
    pub async fn free_name(&self, user: &str, folder: &DavPath, name: &str) -> Result<String, FileError> {
        each!(self, free_name(user, folder, name))
    }

    /// Makes `folder` and the folders above it that are missing.
    pub async fn make_folders(&self, user: &str, folder: &DavPath) -> Result<(), FileError> {
        each!(self, make_folders(user, folder))
    }

    /// Uploads `bytes` as the new file `path`; never replaces one ([`FileError::Exists`]).
    pub async fn put_file(&self, user: &str, path: &DavPath, bytes: Vec<u8>) -> Result<Uploaded, FileError> {
        each!(self, put_file(user, path, bytes))
    }

    /// Starts the chunked upload `upload` of `total` bytes for `target`.
    pub async fn start_upload(&self, user: &str, upload: &str, target: &DavPath, total: u64) -> Result<(), FileError> {
        each!(self, start_upload(user, upload, target, total))
    }

    /// Uploads the piece `number` (from 1) of the chunked upload `upload`.
    pub async fn put_chunk(
        &self,
        user: &str,
        upload: &str,
        number: u32,
        bytes: Vec<u8>,
        target: &DavPath,
        total: u64,
    ) -> Result<(), FileError> {
        each!(self, put_chunk(user, upload, number, bytes, target, total))
    }

    /// Puts the pieces together as `target`; never replaces a file ([`FileError::Exists`]).
    pub async fn finish_upload(
        &self,
        user: &str,
        upload: &str,
        target: &DavPath,
        total: u64,
    ) -> Result<Uploaded, FileError> {
        each!(self, finish_upload(user, upload, target, total))
    }

    /// Drops the chunked upload `upload` with its pieces.
    pub async fn cancel_upload(&self, user: &str, upload: &str) -> Result<(), FileError> {
        each!(self, cancel_upload(user, upload))
    }

    /// Deletes the file `path` only while it has `etag`.
    pub async fn delete_file(&self, user: &str, path: &DavPath, etag: &str) -> Result<(), FileError> {
        each!(self, delete_file(user, path, etag))
    }

    /// The bytes of the file `path`, at most `max` ([`FileError::TooLarge`]).
    pub async fn get_file(&self, user: &str, path: &DavPath, max: u64) -> Result<Vec<u8>, FileError> {
        each!(self, get_file(user, path, max))
    }

    /// The server's preview of the file `file`, at most `edge` pixels.
    pub async fn preview(&self, file: u64, edge: u32) -> Result<Picture, FileError> {
        each!(self, preview(file, edge))
    }

    /// The picture of a link card by its id on the server.
    pub async fn card_picture(&self, id: &str) -> Result<Picture, FileError> {
        each!(self, card_picture(id))
    }

    /// The card of `link` as the server resolves it (the server fetches the address).
    pub async fn resolve(&self, link: &str) -> Result<Option<Reference>, FileError> {
        each!(self, resolve(link))
    }

    /// Shares the file `path` into the conversation `token`.
    pub async fn share(&self, token: &str, path: &DavPath, meta: &ShareMeta) -> Result<(), FileError> {
        each!(self, share(token, path, meta))
    }

    /// Up to `limit` messages of the conversation that shared an item of `kind`, the newest first.
    pub async fn shared(
        &self,
        token: &str,
        kind: &str,
        before: Option<u64>,
        limit: u32,
    ) -> Result<Vec<RemoteMessage>, ChatError> {
        each!(self, shared(token, kind, before, limit))
    }

    /// The newest shared items of each kind, with their kind.
    pub async fn overview(&self, token: &str, limit: u32) -> Result<Vec<(String, RemoteMessage)>, ChatError> {
        each!(self, overview(token, limit))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A plain message of `ben` with `number` and `text`, sent at second `number`.
    pub(crate) fn message(number: u64, text: &str) -> RemoteMessage {
        RemoteMessage {
            number,
            author: Some("ben".into()),
            author_name: "Ben Koch".into(),
            at: DateTime::from_timestamp(1_800_000_000 + number as i64, 0).unwrap_or_default(),
            text: text.into(),
            kind: MessageKind::Message,
            hidden: false,
            patch: None,
            mentions: Vec::new(),
            files: Vec::new(),
            reply_to: None,
            reactions: BTreeMap::new(),
            own_reactions: Vec::new(),
            edited: false,
            paths: BTreeMap::new(),
        }
    }
}
