//! Where the chat windows get their data from, and why a call fails.
//!
//! The windows only know [`ChatSource`]. In production that is the core over the socket
//! (`torro_ipc::chat::ChatClient`); for building and trying the windows it is
//! [`crate::demo::DemoSource`]. Each method is one command of the API areas `chat` or `presence`.

use std::fmt;
use std::future::Future;

use chrono::{DateTime, Utc};

use crate::model::{Conversation, Mention, MessagePage, OwnStatus, Person, Status};

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
}
