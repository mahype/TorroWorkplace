//! Where the windows get their data from: the core over its socket, or the demo in memory
//! (`--demo`). Every method is one command of the API areas `chat` and `presence`; the windows hold
//! no logic of their own about conversations or people.
//!
//! `torro_chat::ChatSource` is not object safe (its methods return `impl Future`), so the two
//! sources sit in an enum that is cheap to clone into every Iced task.

use std::sync::Arc;

use torro_chat::demo::DemoSource;
use torro_chat::{
    ChatError, ChatSource, Conversation, Mention, MessagePage, NotifyLevel, Outgoing, OwnStatus, Person, SearchHit,
    StatusChange,
};
use torro_ipc::chat::ChatClient;
use torro_ipc::{Request, Response};

/// The core, or the sample of `torro_chat::demo` (`--demo`, nothing is written anywhere).
#[derive(Debug, Clone)]
pub enum Backend {
    Core(ChatClient),
    Demo(Arc<DemoSource>),
}

impl Backend {
    /// The core of this user session.
    pub fn core() -> Backend {
        Backend::Core(ChatClient::default())
    }

    /// The demo's sample.
    pub fn demo() -> Backend {
        Backend::Demo(Arc::new(DemoSource::default()))
    }

    pub fn is_demo(&self) -> bool {
        matches!(self, Backend::Demo(_))
    }

    /// `chat.list_conversations` of every account.
    pub async fn conversations(self) -> Result<Vec<Conversation>, ChatError> {
        match self {
            Backend::Core(core) => core.conversations(None).await,
            Backend::Demo(demo) => demo.conversations(None).await,
        }
    }

    /// `chat.get_messages`: the last page, the one `before` a message, or what came `after` one.
    pub async fn messages(
        self,
        conversation: String,
        before: Option<String>,
        after: Option<String>,
    ) -> Result<MessagePage, ChatError> {
        match self {
            Backend::Core(core) => core.messages(conversation, before, after, None).await,
            Backend::Demo(demo) => demo.messages(conversation, before, after, None).await,
        }
    }

    /// `chat.send_message`: the id of the sent message.
    pub async fn send(self, message: Outgoing) -> Result<String, ChatError> {
        match self {
            Backend::Core(core) => core.send(message).await,
            Backend::Demo(demo) => demo.send(message).await,
        }
    }

    /// `chat.mark_read` up to the newest message.
    pub async fn mark_read(self, conversation: String) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.mark_read(conversation, None).await,
            Backend::Demo(demo) => demo.mark_read(conversation, None).await,
        }
    }

    /// `chat.mark_unread`.
    pub async fn mark_unread(self, conversation: String) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.mark_unread(conversation).await,
            Backend::Demo(demo) => demo.mark_unread(conversation).await,
        }
    }

    /// `chat.suggest_mentions`: who `@query` could mean in `conversation`.
    pub async fn suggest_mentions(self, conversation: String, query: String) -> Result<Vec<Mention>, ChatError> {
        match self {
            Backend::Core(core) => core.suggest_mentions(conversation, query).await,
            Backend::Demo(demo) => demo.suggest_mentions(conversation, query).await,
        }
    }

    /// `chat.edit_message`: the person's own message `id` gets `text` (mentions as `@"<id>"`).
    pub async fn edit_message(self, id: String, text: String) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.edit_message(id, text).await,
            Backend::Demo(demo) => demo.edit_message(id, text).await,
        }
    }

    /// `chat.delete_message`: the person's own message `id`, for everyone.
    pub async fn delete_message(self, id: String) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.delete_message(id).await,
            Backend::Demo(demo) => demo.delete_message(id).await,
        }
    }

    /// `chat.react`: gives `emoji` to the message `id`, or takes it back with `remove`.
    pub async fn react(self, id: String, emoji: String, remove: bool) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.react(id, emoji, remove).await,
            Backend::Demo(demo) => demo.react(id, emoji, remove).await,
        }
    }

    /// `chat.set_favorite`.
    pub async fn set_favorite(self, conversation: String, favorite: bool) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.set_favorite(conversation, favorite).await,
            Backend::Demo(demo) => demo.set_favorite(conversation, favorite).await,
        }
    }

    /// `chat.set_notification_level`.
    pub async fn set_notification_level(self, conversation: String, level: NotifyLevel) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.set_notification_level(conversation, level).await,
            Backend::Demo(demo) => demo.set_notification_level(conversation, level).await,
        }
    }

    /// `chat.search_messages` in every account, as many hits as the core gives without being told.
    pub async fn search_messages(self, query: String) -> Result<Vec<SearchHit>, ChatError> {
        match self {
            Backend::Core(core) => core.search_messages(query, None, None).await,
            Backend::Demo(demo) => demo.search_messages(query, None, None).await,
        }
    }

    /// `chat.find_people`: people of the account's server to start a direct chat with. The demo has
    /// no such search of its own; its `presence.find_people` finds the same people.
    pub async fn find_chat_people(self, account: String, query: String) -> Result<Vec<Person>, ChatError> {
        match self {
            Backend::Core(core) => {
                let request = Request::ChatFindPeople { account, query, limit: None };
                match torro_ipc::client::call(&core.socket, &request).await {
                    Ok(Response::ChatPeople { people }) => Ok(people),
                    Ok(Response::Error { code, message }) => Err(torro_ipc::chat::error_of(&code, message)),
                    Ok(Response::Pending { .. }) => Err(ChatError::Pending),
                    // Named by kind only: an answer may carry names of people.
                    Ok(_) => Err(ChatError::Other("unexpected answer".into())),
                    Err(err) => Err(ChatError::Other(err.to_string())),
                }
            }
            Backend::Demo(demo) => demo.find_people(account, query).await,
        }
    }

    /// `chat.create_conversation`: the direct chat with `with`, the existing one when there is one.
    pub async fn create_conversation(self, account: String, with: String) -> Result<Conversation, ChatError> {
        match self {
            Backend::Core(core) => core.create_conversation(account, with).await,
            Backend::Demo(demo) => demo.create_conversation(account, with).await,
        }
    }

    /// `presence.list_people` of every account.
    pub async fn people(self) -> Result<Vec<Person>, ChatError> {
        match self {
            Backend::Core(core) => core.people(None).await,
            Backend::Demo(demo) => demo.people(None).await,
        }
    }

    /// `presence.remove_person`: off the person's own list (only people they added themselves).
    pub async fn remove_person(self, account: String, person: String) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.remove_person(account, person).await,
            Backend::Demo(demo) => demo.remove_person(account, person).await,
        }
    }

    /// `presence.get_own_status` of `account`.
    pub async fn own_status(self, account: String) -> Result<OwnStatus, ChatError> {
        match self {
            Backend::Core(core) => core.own_status(account).await,
            Backend::Demo(demo) => demo.own_status(account).await,
        }
    }

    /// `presence.set_own_status`, then the status as the source has it now.
    pub async fn set_own_status(self, account: String, change: StatusChange) -> Result<OwnStatus, ChatError> {
        match &self {
            Backend::Core(core) => core.set_own_status(account.clone(), change).await?,
            Backend::Demo(demo) => demo.set_own_status(account.clone(), change).await?,
        }
        self.own_status(account).await
    }

    /// `apps.open_app` with the target `chat:<conversation>`: the core opens the conversation's chat
    /// window, or brings it to the front. The demo has no core: it starts this program once more on
    /// the conversation, with the demo, so the window can be tried.
    pub async fn open_chat(self, conversation: String, appearance: Option<&'static str>) -> Result<(), String> {
        match self {
            Backend::Core(core) => {
                let request = Request::OpenApp {
                    app: torro_core::AppId::CHAT.binary.to_owned(),
                    target: Some(format!("chat:{conversation}")),
                };
                match torro_ipc::client::call(&core.socket, &request).await {
                    Ok(Response::Ok) => Ok(()),
                    Ok(Response::Error { message, .. }) => Err(message),
                    Ok(_) => Err("unexpected answer".into()),
                    Err(err) => Err(err.to_string()),
                }
            }
            Backend::Demo(_) => {
                let program = std::env::current_exe().map_err(|err| err.to_string())?;
                let mut command = std::process::Command::new(program);
                command.arg("--demo").arg(format!("--conversation={conversation}"));
                if let Some(appearance) = appearance {
                    command.arg("--appearance").arg(appearance);
                }
                command.spawn().map(|_| ()).map_err(|err| err.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use iced::futures::executor::block_on;
    use torro_chat::Status;
    use torro_chat::demo::{ACCOUNT, BEN, TEAM};

    use super::*;

    #[test]
    fn the_demo_answers_every_command_the_windows_send() {
        let backend = Backend::demo();
        let conversations = block_on(backend.clone().conversations()).expect("conversations");
        assert_eq!(conversations.first().map(|c| c.id.as_str()), Some(BEN));
        let page = block_on(backend.clone().messages(BEN.into(), None, None)).expect("messages");
        assert!(!page.messages.is_empty());
        let found = block_on(backend.clone().find_chat_people(ACCOUNT.into(), "ce".into())).expect("people");
        assert!(found.iter().any(|person| person.name == "Cem Demir"));
        let change = StatusChange { status: Some(Status::Busy), ..StatusChange::default() };
        let own = block_on(backend.clone().set_own_status(ACCOUNT.into(), change)).expect("status");
        assert_eq!(own.status, Status::Busy, "the answer is the new status");
        assert!(backend.is_demo());

        // Stage 7.
        let own = torro_chat::message_id(TEAM, 4);
        block_on(backend.clone().edit_message(own.clone(), "Nine.".into())).expect("edited");
        block_on(backend.clone().react(own.clone(), "🎉".into(), false)).expect("reacted");
        block_on(backend.clone().delete_message(own)).expect("deleted");
        block_on(backend.clone().set_favorite(BEN.into(), true)).expect("favourite");
        block_on(backend.clone().set_notification_level(BEN.into(), NotifyLevel::Never)).expect("level");
        let hits = block_on(backend.search_messages("release".into())).expect("hits");
        assert_eq!(hits.len(), 1);
    }
}
