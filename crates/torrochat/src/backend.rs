//! Where the windows get their data from: the core over its socket, or the demo in memory
//! (`--demo`). Every method is one command of the API areas `chat` and `presence`; the windows hold
//! no logic of their own about conversations or people.
//!
//! `torro_chat::ChatSource` is not object safe (its methods return `impl Future`), so the two
//! sources sit in an enum that is cheap to clone into every Iced task.

use std::sync::Arc;

use torro_chat::demo::DemoSource;
use torro_chat::{
    Call, ChatError, ChatSource, Conversation, Fetched, FilePiece, LinkCard, Mention, MessagePage, NotifyLevel,
    Outgoing, OwnStatus, Person, Picture, PreparedFile, PreviewSize, SaveTo, SearchHit, Sending, SharedKind,
    SharedPage, StatusChange, UploadProgress, Uploaded,
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

    // -- Files and media (stage 4) ------------------------------------------------------------

    /// `chat.get_messages` with `around`: "Show in chat".
    pub async fn messages_around(self, conversation: String, around: String) -> Result<MessagePage, ChatError> {
        match self {
            Backend::Core(core) => core.messages_around(conversation, around, None).await,
            Backend::Demo(demo) => demo.messages_around(conversation, around, None).await,
        }
    }

    /// `chat.send_message` with `files`.
    pub async fn send_files(self, message: Outgoing, files: Vec<String>) -> Result<Sending, ChatError> {
        match self {
            Backend::Core(core) => core.send_files(message, files).await,
            Backend::Demo(demo) => demo.send_files(message, files).await,
        }
    }

    /// `chat.prepare_file` with the file at `path` a person chose.
    pub async fn prepare_file(self, account: String, path: String) -> Result<PreparedFile, ChatError> {
        match self {
            Backend::Core(core) => core.prepare_file(account, path).await,
            Backend::Demo(demo) => demo.prepare_file(account, path).await,
        }
    }

    /// `chat.upload_file`, one piece.
    async fn upload_piece(&self, piece: FilePiece) -> Result<Uploaded, ChatError> {
        match self {
            Backend::Core(core) => core.upload_file(piece).await,
            Backend::Demo(demo) => demo.upload_file(piece).await,
        }
    }

    /// A file the window holds (a pasted picture) uploaded into the core's memory in pieces of
    /// [`torro_ipc::chat::UPLOAD_PIECE_BYTES`] (`chat.upload_file`, one request per piece, as the mail
    /// window uploads pasted pictures); the prepared file. A failure releases what arrived.
    pub async fn upload_bytes(self, account: String, name: String, bytes: Vec<u8>) -> Result<PreparedFile, ChatError> {
        let pieces: Vec<&[u8]> = bytes.chunks(torro_ipc::chat::UPLOAD_PIECE_BYTES).collect();
        let count = pieces.len().max(1);
        let mut upload = None;
        for at in 0..count {
            let piece = FilePiece {
                account: account.clone(),
                upload: upload.clone(),
                offset: (at * torro_ipc::chat::UPLOAD_PIECE_BYTES) as u64,
                bytes: pieces.get(at).map(|piece| piece.to_vec()).unwrap_or_default(),
                name: (at == 0).then(|| name.clone()),
                last: at + 1 == count,
            };
            match self.upload_piece(piece).await {
                Ok(Uploaded::File(file)) => return Ok(file),
                Ok(Uploaded::Piece { upload: id, .. }) => upload = Some(id),
                Err(err) => {
                    if let Some(id) = upload {
                        let _ = self.clone().release_files(account, vec![id]).await;
                    }
                    return Err(err);
                }
            }
        }
        Err(ChatError::Other("unexpected answer".into()))
    }

    /// `chat.release_files`.
    pub async fn release_files(self, account: String, ids: Vec<String>) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.release_files(account, ids).await,
            Backend::Demo(demo) => demo.release_files(account, ids).await,
        }
    }

    /// `chat.get_upload`.
    pub async fn upload(self, id: String) -> Result<UploadProgress, ChatError> {
        match self {
            Backend::Core(core) => core.upload(id).await,
            Backend::Demo(demo) => demo.upload(id).await,
        }
    }

    /// `chat.cancel_upload`.
    pub async fn cancel_upload(self, id: String) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => core.cancel_upload(id).await,
            Backend::Demo(demo) => demo.cancel_upload(id).await,
        }
    }

    /// `chat.get_preview` of a file (`file`) or of the message's link card (`file` `None`).
    pub async fn preview(self, id: String, file: Option<String>, size: PreviewSize) -> Result<Picture, ChatError> {
        let card = file.is_none();
        match self {
            Backend::Core(core) => core.preview(id, file, card, size).await,
            Backend::Demo(demo) => demo.preview(id, file, card, size).await,
        }
    }

    /// `chat.get_attachment`.
    pub async fn attachment(self, id: String, file: String) -> Result<Fetched, ChatError> {
        match self {
            Backend::Core(core) => core.attachment(id, file).await,
            Backend::Demo(demo) => demo.attachment(id, file).await,
        }
    }

    /// `chat.save_attachment`: the path the file went to.
    pub async fn save_attachment(self, id: String, file: String, to: SaveTo) -> Result<String, ChatError> {
        match self {
            Backend::Core(core) => core.save_attachment(id, file, to).await,
            Backend::Demo(demo) => demo.save_attachment(id, file, to).await,
        }
    }

    /// `chat.list_shared` of one kind, older than `before`.
    pub async fn shared(
        self,
        conversation: String,
        kind: SharedKind,
        before: Option<String>,
    ) -> Result<SharedPage, ChatError> {
        match self {
            Backend::Core(core) => core.shared(conversation, Some(kind), before, None).await,
            Backend::Demo(demo) => demo.shared(conversation, Some(kind), before, None).await,
        }
    }

    /// `chat.show_card` ("Show preview").
    pub async fn show_card(self, id: String) -> Result<Option<LinkCard>, ChatError> {
        match self {
            Backend::Core(core) => core.show_card(id).await,
            Backend::Demo(demo) => demo.show_card(id).await,
        }
    }

    /// `chat.get_call` (stage 6b): the call of `conversation`, for the ringing window.
    pub async fn call(self, conversation: String) -> Result<Call, ChatError> {
        match self {
            Backend::Core(core) => ChatSource::call(&core, conversation).await,
            Backend::Demo(demo) => demo.call(conversation).await,
        }
    }

    /// `chat.decline_call`: `conversation` stops ringing for the person.
    pub async fn decline_call(self, conversation: String) -> Result<(), ChatError> {
        match self {
            Backend::Core(core) => ChatSource::decline_call(&core, conversation).await,
            Backend::Demo(demo) => demo.decline_call(conversation).await,
        }
    }

    /// The demo's sending that stays at 42 % (`--open upload`); `None` on the core.
    pub fn stalled_upload(&self, conversation: &str) -> Option<String> {
        match self {
            Backend::Core(_) => None,
            Backend::Demo(demo) => Some(demo.stalled_upload(conversation)),
        }
    }

    /// Opens the window "Picture large" of the file `file` of the message `message`: this program
    /// once more with `--viewer=<message>/<file>` (decision 24), with the demo and `appearance` when
    /// this one runs on them. The core has no target for it; a second start on the same picture
    /// brings its window to the front.
    pub fn open_viewer(&self, message: &str, file: &str, appearance: Option<&'static str>) -> Result<(), String> {
        let program = std::env::current_exe().map_err(|err| err.to_string())?;
        let mut command = std::process::Command::new(program);
        command.arg(format!("--viewer={message}/{file}"));
        if self.is_demo() {
            command.arg("--demo");
            if let Some(appearance) = appearance {
                command.arg("--appearance").arg(appearance);
            }
        }
        command.spawn().map(|_| ()).map_err(|err| err.to_string())
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
        let hits = block_on(backend.clone().search_messages("release".into())).expect("hits");
        assert_eq!(hits.len(), 1);

        // Stage 4: a pasted picture goes up in pieces, larger than one piece.
        let design = torro_chat::demo::DESIGN;
        let bytes = vec![7u8; torro_ipc::chat::UPLOAD_PIECE_BYTES + 10];
        let file =
            block_on(backend.clone().upload_bytes(ACCOUNT.into(), "Pasted image.png".into(), bytes)).expect("prepared");
        assert_eq!(file.size, torro_ipc::chat::UPLOAD_PIECE_BYTES as u64 + 10);
        let outgoing = Outgoing { conversation: design.into(), ..Outgoing::default() };
        let sending = block_on(backend.clone().send_files(outgoing, vec![file.id])).expect("sending");
        assert!(matches!(sending, Sending::Uploading(_)));
        let picture = torro_chat::message_id(design, 3);
        let preview = block_on(backend.clone().preview(picture.clone(), Some("5103".into()), PreviewSize::Small));
        assert!(preview.is_ok());
        let shared = block_on(backend.clone().shared(design.into(), SharedKind::Media, None)).expect("shared");
        assert!(!shared.items.is_empty());
        assert!(backend.stalled_upload(design).is_some(), "the demo's stalled upload");
        assert!(Backend::core().stalled_upload(design).is_none());
    }
}
