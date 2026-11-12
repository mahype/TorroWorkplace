//! Where the chat windows get their data from, and why a call fails.
//!
//! The windows only know [`ChatSource`]. In production that is the core over the socket
//! (`torro_ipc::chat::ChatClient`); for building and trying the windows it is
//! [`crate::demo::DemoSource`]. Each method is one command of the API areas `chat` or `presence`.
//!
//! Stage 4 (files and media) adds the methods from [`ChatSource::messages_around`] on, with the
//! types of [`crate::files`].

use std::fmt;
use std::future::Future;

use chrono::{DateTime, Utc};

use crate::files::{
    Fetched, FilePiece, Picture, PreparedFile, PreviewSize, SaveTo, Sending, SharedKind, SharedPage, UploadProgress,
    Uploaded,
};
use crate::model::{Conversation, LinkCard, Mention, MessagePage, NotifyLevel, OwnStatus, Person, SearchHit, Status};

/// Why a call failed, in the terms the core answers with (`docs/chat.md`, "Fehler").
///
/// `Debug` and `Display` never show text of a conversation or of the server: errors end up in
/// logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatError {
    /// Credentials missing or refused (`auth`).
    Auth,
    /// Server not reachable (`offline`); nothing is sent later.
    Offline,
    /// The account has no chat on its server (`not-connected`).
    NotConnected,
    /// No such conversation, message or person, or the person is no longer part of it
    /// (`not-found`).
    NotFound,
    /// Not possible here; the text says why, in English, for people (`unsupported`).
    Unsupported(String),
    /// The caller may not do this (`forbidden`).
    Forbidden(String),
    /// Too many calls; try again later (`busy`).
    Busy,
    /// The command waits for the person's approval (`pending`); only clients ever see it.
    Pending,
    /// Anything else; the text is ours, never the server's (`error`).
    Other(String),
}

impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatError::Auth => f.write_str("sign-in refused"),
            ChatError::Offline => f.write_str("server not reachable"),
            ChatError::NotConnected => f.write_str("the account has no chat"),
            ChatError::NotFound => f.write_str("no such conversation, message or person"),
            ChatError::Busy => f.write_str("too many calls"),
            ChatError::Pending => f.write_str("waits for approval"),
            ChatError::Unsupported(text) | ChatError::Forbidden(text) | ChatError::Other(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for ChatError {}

/// A message to send with [`ChatSource::send`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outgoing {
    /// The conversation (`<account>/<token>`).
    pub conversation: String,
    pub text: String,
    /// The message this one answers (`<conversation>/<number>`).
    pub reply_to: Option<String>,
    /// Send without notifying the others.
    pub silent: bool,
}

/// The person's own status to set with [`ChatSource::set_own_status`]. `None` leaves a part as it
/// is; an empty `message` clears the status message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusChange {
    pub status: Option<Status>,
    pub message: Option<String>,
    pub icon: Option<String>,
    pub clear_at: Option<DateTime<Utc>>,
}

pub trait ChatSource: Send + Sync {
    /// `chat.list_conversations`: of one account or all, the most recent first.
    fn conversations(
        &self,
        account: Option<String>,
    ) -> impl Future<Output = Result<Vec<Conversation>, ChatError>> + Send;

    /// `chat.get_messages`: the last `limit` messages, those before `before` or those after
    /// `after` (message ids). Reading never changes what is read.
    fn messages(
        &self,
        conversation: String,
        before: Option<String>,
        after: Option<String>,
        limit: Option<u32>,
    ) -> impl Future<Output = Result<MessagePage, ChatError>> + Send;

    /// `chat.send_message`: the id of the sent message.
    fn send(&self, message: Outgoing) -> impl Future<Output = Result<String, ChatError>> + Send;

    /// `chat.mark_read`: up to the message `up_to`, else up to the newest.
    fn mark_read(
        &self,
        conversation: String,
        up_to: Option<String>,
    ) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `chat.mark_unread`.
    fn mark_unread(&self, conversation: String) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `chat.suggest_mentions`: who `@query` could mean in the conversation.
    fn suggest_mentions(
        &self,
        conversation: String,
        query: String,
    ) -> impl Future<Output = Result<Vec<Mention>, ChatError>> + Send;

    /// `chat.edit_message`: replaces the text of one of the person's own messages (`id` is
    /// `<conversation>/<number>`); the old text is gone and the message counts as edited.
    ///
    /// Must fail with [`ChatError::NotFound`] for an unknown message and with
    /// [`ChatError::Unsupported`] (a sentence for people) for a message of someone else, one that
    /// was deleted or is no message of a person, and one older than the provider allows (24 hours
    /// on Talk). `text` goes out as it is: mentions written as `@"<id>"`.
    fn edit_message(&self, id: String, text: String) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `chat.delete_message`: deletes one of the person's own messages for everyone; it cannot be
    /// restored. Afterwards the message is still there with [`crate::MessageKind::Deleted`], an
    /// empty text and neither files nor reactions.
    ///
    /// Errors as [`ChatSource::edit_message`] (6 hours on Talk).
    fn delete_message(&self, id: String) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `chat.react`: gives the reaction `emoji` (one emoji) to the message `id`, or with `remove`
    /// takes the person's own back. Giving one the person gave already, or taking back one they did
    /// not give, changes nothing and is no error.
    ///
    /// Must fail with [`ChatError::NotFound`] for an unknown message and [`ChatError::Unsupported`]
    /// for a message nobody can react to (deleted, written by the provider).
    fn react(&self, id: String, emoji: String, remove: bool) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `chat.set_favorite`: makes the conversation a favourite or no longer one; lists show
    /// favourites first. [`ChatError::NotFound`] for an unknown conversation.
    fn set_favorite(&self, conversation: String, favorite: bool) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `chat.set_notification_level`: when the person hears of the conversation; it also decides
    /// where its messages land in the inbox. [`ChatError::NotFound`] for an unknown conversation.
    fn set_notification_level(
        &self,
        conversation: String,
        level: NotifyLevel,
    ) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `chat.search_messages`: messages of one account or all that hold `query`, the newest first,
    /// at most `limit` (1 to 50, 20 when `None`). Each hit names its message by id, so the
    /// conversation is the part before the last `/` ([`crate::split_message`]); its title names the
    /// author and the conversation ("Dana Erb in Team Alpha").
    ///
    /// Must fail with [`ChatError::Other`] for a query under two characters (after trimming),
    /// [`ChatError::NotFound`] for an unknown account and [`ChatError::Unsupported`] when the
    /// server cannot search. Deleted messages and messages of the provider are never found.
    fn search_messages(
        &self,
        query: String,
        account: Option<String>,
        limit: Option<u32>,
    ) -> impl Future<Output = Result<Vec<SearchHit>, ChatError>> + Send;

    /// `chat.create_conversation`: the direct chat with the person `with` (`<account>/<user id>`),
    /// the existing one when there is one.
    fn create_conversation(
        &self,
        account: String,
        with: String,
    ) -> impl Future<Output = Result<Conversation, ChatError>> + Send;

    /// `presence.list_people`: of one account or all.
    fn people(&self, account: Option<String>) -> impl Future<Output = Result<Vec<Person>, ChatError>> + Send;

    /// `presence.find_people`: people of the server by name, at least two characters.
    fn find_people(
        &self,
        account: String,
        query: String,
    ) -> impl Future<Output = Result<Vec<Person>, ChatError>> + Send;

    /// `presence.add_person`: onto the person's own list.
    fn add_person(&self, account: String, person: String) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `presence.remove_person`: off the person's own list.
    fn remove_person(&self, account: String, person: String) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `presence.get_own_status`.
    fn own_status(&self, account: String) -> impl Future<Output = Result<OwnStatus, ChatError>> + Send;

    /// `presence.set_own_status`.
    fn set_own_status(
        &self,
        account: String,
        change: StatusChange,
    ) -> impl Future<Output = Result<(), ChatError>> + Send;

    // -- Files and media (stage 4) ------------------------------------------------------------

    /// `chat.get_messages` with `around`: the message `around` (`<conversation>/<number>`) with the
    /// messages before and after it, half each, at most `limit` (1 to 100, 50 when `None`), the
    /// oldest first – for "Show in chat". `more` says whether older messages exist.
    ///
    /// Must fail with [`ChatError::NotFound`] for an unknown conversation, [`ChatError::Other`] for
    /// a message that is not one of the conversation's, and [`ChatError::Unsupported`] when the
    /// server cannot.
    fn messages_around(
        &self,
        conversation: String,
        around: String,
        limit: Option<u32>,
    ) -> impl Future<Output = Result<MessagePage, ChatError>> + Send;

    /// `chat.send_message` with `files` (ids of [`ChatSource::prepare_file`] or
    /// [`ChatSource::upload_file`], at most 10): the core uploads them into the person's Talk folder
    /// and shares them, the text (`message.text`, may be empty) as the caption. The answer is
    /// [`Sending::Uploading`] as soon as the upload begins; without files it is the same as
    /// [`ChatSource::send`] ([`Sending::Sent`]). From then on the prepared files are used up.
    ///
    /// Must fail with [`ChatError::NotFound`] for a prepared file that is unknown or of another
    /// account, [`ChatError::Unsupported`] when the server does not let the person share files.
    fn send_files(
        &self,
        message: Outgoing,
        files: Vec<String>,
    ) -> impl Future<Output = Result<Sending, ChatError>> + Send;

    /// `chat.prepare_file` with `source: path`: the file at the absolute `path` a person chose
    /// (paperclip, dragging, a file on the clipboard), for sending with `account`. The core reads only
    /// what it needs for kind and preview; nothing goes to the server.
    ///
    /// Must fail with [`ChatError::Other`] (a sentence for people) for a file larger than the limit
    /// or one that cannot be read, [`ChatError::Busy`] when the core holds too much already.
    fn prepare_file(
        &self,
        account: String,
        path: String,
    ) -> impl Future<Output = Result<PreparedFile, ChatError>> + Send;

    /// `chat.upload_file`: one piece of a file the window holds itself (a pasted picture), at most
    /// 25 MiB in all. The first piece starts at offset 0 without `upload`, the next name the upload
    /// the answer gave; the last one (`last`) answers with the prepared file.
    fn upload_file(&self, piece: FilePiece) -> impl Future<Output = Result<Uploaded, ChatError>> + Send;

    /// `chat.release_files`: drops prepared files and unfinished uploads of the caller; unknown ids
    /// are passed over without an error.
    fn release_files(&self, account: String, ids: Vec<String>) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `chat.get_upload`: how the sending `id` of [`Sending::Uploading`] goes.
    /// [`ChatError::NotFound`] for an unknown one.
    fn upload(&self, id: String) -> impl Future<Output = Result<UploadProgress, ChatError>> + Send;

    /// `chat.cancel_upload`: stops the sending `id` and deletes what of it is on the server already.
    /// Must fail with [`ChatError::Other`] (the core's sentence) once the files are being shared.
    fn cancel_upload(&self, id: String) -> impl Future<Output = Result<(), ChatError>> + Send;

    /// `chat.get_preview`: a preview of the file `file` of the message `id`, or with `card` the
    /// picture of the message's link card (then `file` is `None`). [`ChatError::NotFound`] for a
    /// file that is no file of the message or has no preview.
    fn preview(
        &self,
        id: String,
        file: Option<String>,
        card: bool,
        size: PreviewSize,
    ) -> impl Future<Output = Result<Picture, ChatError>> + Send;

    /// `chat.get_attachment`: fetches the file `file` of the message `id` for opening.
    /// [`ChatError::Forbidden`] for a file shared so that it may not be downloaded,
    /// [`ChatError::NotFound`] for a file that is no file of the message.
    fn attachment(&self, id: String, file: String) -> impl Future<Output = Result<Fetched, ChatError>> + Send;

    /// `chat.save_attachment`: saves the file `file` of the message `id` to `to`; the answer is the
    /// absolute path it went to. Errors as [`ChatSource::attachment`], and
    /// [`ChatError::NotFound`] for a folder or target that does not exist.
    fn save_attachment(
        &self,
        id: String,
        file: String,
        to: SaveTo,
    ) -> impl Future<Output = Result<String, ChatError>> + Send;

    /// `chat.list_shared`: what the conversation shared, the newest first: of `kind`, or the newest
    /// of each kind without it; only items older than the message `before`; at most `limit` (1 to
    /// 100, 50 when `None`). [`ChatError::Unsupported`] when the server cannot list them.
    fn shared(
        &self,
        conversation: String,
        kind: Option<SharedKind>,
        before: Option<String>,
        limit: Option<u32>,
    ) -> impl Future<Output = Result<SharedPage, ChatError>> + Send;

    /// `chat.show_card` ("Show preview"): the card of the first link of the message `id`, fetched by
    /// the account's server; `None` when there is none. Later pages of `chat.get_messages` carry it
    /// in `cards`.
    fn show_card(&self, id: String) -> impl Future<Output = Result<Option<LinkCard>, ChatError>> + Send;
}
