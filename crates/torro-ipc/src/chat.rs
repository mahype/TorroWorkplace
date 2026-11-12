//! The API areas `chat` and `presence` (`docs/chat.md`, issue #643): limits of their commands, and
//! [`ChatClient`], the chat windows' and the Notification Hub's way to the core. The answers carry
//! the types of `torro_chat` as they are; every conversation, message and person says
//! `untrusted: true`. Files and media (stage 4) are in [`files`].

pub mod files;

pub use files::*;

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use torro_chat::{
    ChatError, ChatSource, Conversation, Mention, MessagePage, NotifyLevel, Outgoing, OwnStatus, Person, SearchHit,
    StatusChange,
};

use crate::{Request, Response};

/// How many messages `chat.get_messages` returns when it is not told.
pub const MESSAGES: u32 = 50;

/// The most messages `chat.get_messages` returns.
pub const MAX_MESSAGES: u32 = 100;

/// How many conversations `chat.list_conversations` returns when it is not told.
pub const CONVERSATIONS: u32 = 200;

/// The most conversations `chat.list_conversations` returns.
pub const MAX_CONVERSATIONS: u32 = 500;

/// The most people `chat.find_people` and `presence.find_people` return, and the default.
pub const MAX_PEOPLE: u32 = 20;

/// The fewest characters a search for people or messages takes.
pub const MIN_QUERY: usize = 2;

/// The most hits `chat.search_messages` returns; 20 when it is not told.
pub const MAX_HITS: u32 = 50;

/// The most characters of a status message (the limit of Nextcloud's user status).
pub const MAX_STATUS_MESSAGE: usize = 80;

/// A call in a conversation as `chat.get_call` tells it (stage 6 of `docs/chat.md`): no names, no
/// devices, no pictures; who takes part is in the conversation itself.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ChatCall {
    /// The conversation (`<account>/<token>`).
    pub conversation: String,
    /// A call runs.
    pub active: bool,
    /// Someone calls the person and it still rings for them: not declined, not answered on another
    /// device, at most 45 seconds.
    pub ringing: bool,
    /// When the call began, when the server says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,
    /// How many people are in the call, each once however many devices they joined with.
    pub people: u32,
    /// The person is in the call, on any of their devices.
    pub joined: bool,
    /// Where the person takes part in the call in the browser (`<server>/call/<token>`), until the
    /// suite has its own call window: what "Accept" opens.
    pub link: String,
}

/// Talks to the core at its socket: [`ChatSource`] for the chat windows, one request per call.
#[derive(Debug, Clone)]
pub struct ChatClient {
    pub socket: PathBuf,
}

impl Default for ChatClient {
    /// The core of this user session ([`crate::socket_path`]).
    fn default() -> ChatClient {
        ChatClient { socket: crate::socket_path().unwrap_or_else(|| PathBuf::from("core.sock")) }
    }
}

/// An answer of another kind than asked; named by its kind only, never with its contents (messages
/// end up in logs otherwise).
fn unexpected(response: Response) -> ChatError {
    let kind = serde_json::to_value(&response)
        .ok()
        .and_then(|value| value.get("result").and_then(|r| r.as_str()).map(str::to_string))
        .unwrap_or_default();
    ChatError::Other(format!("unexpected answer: {kind}"))
}

/// The [`ChatError`] of an error code of the core.
pub fn error_of(code: &str, message: String) -> ChatError {
    match code {
        "auth" => ChatError::Auth,
        "offline" => ChatError::Offline,
        "not-connected" => ChatError::NotConnected,
        "not-found" => ChatError::NotFound,
        "unsupported" => ChatError::Unsupported(message),
        "forbidden" => ChatError::Forbidden(message),
        "busy" => ChatError::Busy,
        _ => ChatError::Other(message),
    }
}

impl ChatClient {
    /// Sends `request`; an error of the core comes back as the [`ChatError`] of its code.
    async fn ask(&self, request: Request) -> Result<Response, ChatError> {
        match crate::client::call(&self.socket, &request).await {
            Ok(Response::Error { code, message }) => Err(error_of(&code, message)),
            Ok(Response::Pending { .. }) => Err(ChatError::Pending),
            Ok(response) => Ok(response),
            Err(err) => Err(ChatError::Other(err.to_string())),
        }
    }

    async fn ok(&self, request: Request) -> Result<(), ChatError> {
        match self.ask(request).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }
}

/// Calls (stage 6b of `docs/chat.md`), for the ringing window. Not part of [`ChatSource`]: only the
/// chat app's ringing window asks them, and the demo has no calls.
impl ChatClient {
    /// `chat.get_call`: the call of `conversation`.
    pub async fn call(&self, conversation: String) -> Result<ChatCall, ChatError> {
        match self.ask(Request::ChatGetCall { conversation }).await? {
            Response::ChatCall { call } => Ok(call),
            other => Err(unexpected(other)),
        }
    }

    /// `chat.decline_call`: stops `conversation` ringing for the person.
    pub async fn decline_call(&self, conversation: String) -> Result<(), ChatError> {
        self.ok(Request::ChatDeclineCall { conversation }).await
    }

    /// `chat.leave_call`: leaves the call of `conversation`.
    pub async fn leave_call(&self, conversation: String) -> Result<(), ChatError> {
        self.ok(Request::ChatLeaveCall { conversation }).await
    }
}

impl ChatSource for ChatClient {
    async fn conversations(&self, account: Option<String>) -> Result<Vec<Conversation>, ChatError> {
        let request = Request::ChatListConversations { account, unread_only: false, query: None, limit: None };
        match self.ask(request).await? {
            Response::ChatConversations { conversations } => Ok(conversations),
            other => Err(unexpected(other)),
        }
    }

    async fn messages(
        &self,
        conversation: String,
        before: Option<String>,
        after: Option<String>,
        limit: Option<u32>,
    ) -> Result<MessagePage, ChatError> {
        match self.ask(Request::ChatGetMessages { conversation, before, after, around: None, limit }).await? {
            Response::ChatMessages(page) => Ok(page),
            other => Err(unexpected(other)),
        }
    }

    async fn send(&self, message: Outgoing) -> Result<String, ChatError> {
        let Outgoing { conversation, text, reply_to, silent } = message;
        let files = Vec::new();
        match self
            .ask(Request::ChatSendMessage {
                conversation,
                text,
                reply_to,
                silent,
                files,
                items: Vec::new(),
                fields: None,
            })
            .await?
        {
            Response::ChatSent { id } => Ok(id),
            other => Err(unexpected(other)),
        }
    }

    async fn mark_read(&self, conversation: String, up_to: Option<String>) -> Result<(), ChatError> {
        self.ok(Request::ChatMarkRead { conversation, up_to }).await
    }

    async fn mark_unread(&self, conversation: String) -> Result<(), ChatError> {
        self.ok(Request::ChatMarkUnread { conversation }).await
    }

    async fn suggest_mentions(&self, conversation: String, query: String) -> Result<Vec<Mention>, ChatError> {
        match self.ask(Request::ChatSuggestMentions { conversation, query, limit: None }).await? {
            Response::ChatMentions { mentions } => Ok(mentions),
            other => Err(unexpected(other)),
        }
    }

    async fn edit_message(&self, id: String, text: String) -> Result<(), ChatError> {
        self.ok(Request::ChatEditMessage { id, text }).await
    }

    async fn delete_message(&self, id: String) -> Result<(), ChatError> {
        self.ok(Request::ChatDeleteMessage { id }).await
    }

    async fn react(&self, id: String, emoji: String, remove: bool) -> Result<(), ChatError> {
        self.ok(Request::ChatReact { id, emoji, remove }).await
    }

    async fn set_favorite(&self, conversation: String, favorite: bool) -> Result<(), ChatError> {
        self.ok(Request::ChatSetFavorite { conversation, favorite }).await
    }

    async fn set_notification_level(&self, conversation: String, level: NotifyLevel) -> Result<(), ChatError> {
        self.ok(Request::ChatSetNotificationLevel { conversation, level }).await
    }

    async fn search_messages(
        &self,
        query: String,
        account: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<SearchHit>, ChatError> {
        match self.ask(Request::ChatSearchMessages { query, account, limit }).await? {
            Response::ChatFound { hits } => Ok(hits),
            other => Err(unexpected(other)),
        }
    }

    async fn create_conversation(&self, account: String, with: String) -> Result<Conversation, ChatError> {
        match self.ask(Request::ChatCreateConversation { account, with }).await? {
            Response::ChatConversation { conversation } => Ok(*conversation),
            other => Err(unexpected(other)),
        }
    }

    async fn people(&self, account: Option<String>) -> Result<Vec<Person>, ChatError> {
        match self.ask(Request::PresenceListPeople { account }).await? {
            Response::PresencePeople { people } => Ok(people),
            other => Err(unexpected(other)),
        }
    }

    async fn find_people(&self, account: String, query: String) -> Result<Vec<Person>, ChatError> {
        match self.ask(Request::PresenceFindPeople { account, query, limit: None }).await? {
            Response::PresencePeople { people } => Ok(people),
            other => Err(unexpected(other)),
        }
    }

    async fn add_person(&self, account: String, person: String) -> Result<(), ChatError> {
        self.ok(Request::PresenceAddPerson { account, person }).await
    }

    async fn remove_person(&self, account: String, person: String) -> Result<(), ChatError> {
        self.ok(Request::PresenceRemovePerson { account, person }).await
    }

    async fn own_status(&self, account: String) -> Result<OwnStatus, ChatError> {
        match self.ask(Request::PresenceGetOwnStatus { account }).await? {
            Response::PresenceOwnStatus { status } => Ok(*status),
            other => Err(unexpected(other)),
        }
    }

    async fn set_own_status(&self, account: String, change: StatusChange) -> Result<(), ChatError> {
        let StatusChange { status, message, icon, clear_at } = change;
        self.ok(Request::PresenceSetOwnStatus { account, status, message, icon, clear_at }).await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use torro_chat::Status;
    use torro_chat::demo::{ACCOUNT, BEN, DemoSource};

    use super::*;
    use crate::{client, server};

    /// A test core that answers the chat and presence commands from the demo data, as the core
    /// would.
    async fn answer(demo: &DemoSource, request: Request) -> Response {
        let error = |err: ChatError| {
            let code = match err {
                ChatError::NotFound => "not-found",
                ChatError::Unsupported(_) => "unsupported",
                ChatError::Offline => "offline",
                ChatError::Auth => "auth",
                ChatError::NotConnected => "not-connected",
                ChatError::Forbidden(_) => "forbidden",
                ChatError::Busy => "busy",
                ChatError::Pending | ChatError::Other(_) => "error",
            };
            Response::Error { code: code.into(), message: err.to_string() }
        };
        let done = |result: Result<(), ChatError>| result.map(|()| Response::Ok);
        let answer = match request {
            Request::ChatListConversations { account, .. } => {
                demo.conversations(account).await.map(|conversations| Response::ChatConversations { conversations })
            }
            Request::ChatGetMessages { conversation, before, after, limit, .. } => {
                demo.messages(conversation, before, after, limit).await.map(Response::ChatMessages)
            }
            Request::ChatSendMessage { conversation, text, reply_to, silent, .. } => {
                demo.send(Outgoing { conversation, text, reply_to, silent }).await.map(|id| Response::ChatSent { id })
            }
            Request::ChatMarkRead { conversation, up_to } => done(demo.mark_read(conversation, up_to).await),
            Request::ChatMarkUnread { conversation } => done(demo.mark_unread(conversation).await),
            Request::ChatSuggestMentions { conversation, query, .. } => {
                demo.suggest_mentions(conversation, query).await.map(|mentions| Response::ChatMentions { mentions })
            }
            Request::ChatEditMessage { id, text } => done(demo.edit_message(id, text).await),
            Request::ChatDeleteMessage { id } => done(demo.delete_message(id).await),
            Request::ChatReact { id, emoji, remove } => done(demo.react(id, emoji, remove).await),
            Request::ChatSetFavorite { conversation, favorite } => {
                done(demo.set_favorite(conversation, favorite).await)
            }
            Request::ChatSetNotificationLevel { conversation, level } => {
                done(demo.set_notification_level(conversation, level).await)
            }
            Request::ChatSearchMessages { query, account, limit } => {
                demo.search_messages(query, account, limit).await.map(|hits| Response::ChatFound { hits })
            }
            Request::ChatCreateConversation { account, with } => demo
                .create_conversation(account, with)
                .await
                .map(|conversation| Response::ChatConversation { conversation: Box::new(conversation) }),
            Request::PresenceListPeople { account } => {
                demo.people(account).await.map(|people| Response::PresencePeople { people })
            }
            Request::PresenceFindPeople { account, query, .. } => {
                demo.find_people(account, query).await.map(|people| Response::PresencePeople { people })
            }
            Request::PresenceAddPerson { account, person } => done(demo.add_person(account, person).await),
            Request::PresenceRemovePerson { account, person } => done(demo.remove_person(account, person).await),
            Request::PresenceGetOwnStatus { account } => {
                demo.own_status(account).await.map(|status| Response::PresenceOwnStatus { status: Box::new(status) })
            }
            Request::PresenceSetOwnStatus { account, status, message, icon, clear_at } => {
                done(demo.set_own_status(account, StatusChange { status, message, icon, clear_at }).await)
            }
            _ => Ok(Response::Pong { version: "test".into(), api: 0 }),
        };
        answer.unwrap_or_else(error)
    }

    /// Starts the test core on a socket of its own and returns the client for it.
    async fn core() -> (ChatClient, tokio::task::JoinHandle<Result<(), crate::IpcError>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temporary directory");
        let socket = dir.path().join("core.sock");
        let demo = Arc::new(DemoSource::default());
        let serving = tokio::spawn({
            let socket = socket.clone();
            async move {
                server::serve(&socket, move |request| {
                    let demo = Arc::clone(&demo);
                    async move { answer(&demo, request).await }
                })
                .await
            }
        });
        for _ in 0..200 {
            if client::call(&socket, &Request::Ping).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        (ChatClient { socket }, serving, dir)
    }

    #[tokio::test]
    async fn the_windows_read_and_write_over_the_socket() {
        let (chat, serving, _dir) = core().await;
        let conversations = chat.conversations(None).await.expect("conversations");
        assert_eq!(conversations.first().map(|c| c.id.as_str()), Some(BEN));
        let page = chat.messages(BEN.into(), None, None, Some(2)).await.expect("messages");
        assert_eq!(page.messages.len(), 2);
        assert!(page.messages.iter().all(|message| message.untrusted));
        let sent = chat
            .send(Outgoing { conversation: BEN.into(), text: "On it.".into(), ..Outgoing::default() })
            .await
            .expect("sent");
        assert!(sent.starts_with(BEN));
        chat.mark_unread(BEN.into()).await.expect("unread");
        chat.mark_read(BEN.into(), None).await.expect("read");
        assert_eq!(chat.messages("nextcloud:x@y/nope".into(), None, None, None).await, Err(ChatError::NotFound));

        let people = chat.people(Some(ACCOUNT.into())).await.expect("people");
        assert_eq!(people.len(), 6);
        let change = StatusChange { status: Some(Status::Away), ..StatusChange::default() };
        chat.set_own_status(ACCOUNT.into(), change).await.expect("status set");
        assert_eq!(chat.own_status(ACCOUNT.into()).await.expect("own status").status, Status::Away);
        serving.abort();
    }

    /// Stage 7: each method is one request, and the core's answer comes back as the demo gave it.
    #[tokio::test]
    async fn the_windows_change_messages_and_conversations_over_the_socket() {
        let (chat, serving, _dir) = core().await;
        let team = torro_chat::demo::TEAM;
        let own = torro_chat::message_id(team, 4);
        chat.edit_message(own.clone(), "Tomorrow at nine.".into()).await.expect("edited");
        chat.react(torro_chat::message_id(team, 3), "🎉".into(), false).await.expect("reacted");
        chat.react(torro_chat::message_id(team, 3), "👍".into(), true).await.expect("taken back");
        let page = chat.messages(team.into(), None, None, None).await.expect("messages");
        let edited = page.messages.iter().find(|message| message.id == own).expect("the own message");
        assert_eq!((edited.text.as_str(), edited.edited), ("Tomorrow at nine.", true));
        let cem = page.messages.iter().find(|message| message.number() == 3).expect("Cem's message");
        assert_eq!(cem.own_reactions, ["🎉"]);
        let old = chat.edit_message(torro_chat::message_id(BEN, 2), "x".into()).await;
        assert!(matches!(old, Err(ChatError::Unsupported(text)) if !text.is_empty()), "the reason comes along");
        chat.delete_message(own).await.expect("deleted");

        chat.set_favorite(BEN.into(), true).await.expect("favourite");
        chat.set_notification_level(BEN.into(), NotifyLevel::Never).await.expect("level");
        let ben = chat.conversations(None).await.expect("list").into_iter().find(|c| c.id == BEN).expect("Ben");
        assert_eq!((ben.favorite, ben.notify), (true, NotifyLevel::Never));

        let hits = chat.search_messages("release".into(), None, None).await.expect("hits");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].untrusted);
        assert!(matches!(chat.search_messages("r".into(), None, None).await, Err(ChatError::Other(_))));
        serving.abort();
    }

    #[test]
    fn error_codes_become_the_errors_the_windows_show() {
        assert_eq!(error_of("not-connected", String::new()), ChatError::NotConnected);
        assert_eq!(error_of("busy", String::new()), ChatError::Busy);
        assert_eq!(error_of("unsupported", "read-only".into()), ChatError::Unsupported("read-only".into()));
        assert_eq!(error_of("something-new", "x".into()), ChatError::Other("x".into()));
    }
}
