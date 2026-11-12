//! The API areas `chat` and `presence` (`docs/chat.md`, issue #643): limits of their commands, and
//! [`ChatClient`], the chat windows' and the Notification Hub's way to the core. The answers carry
//! the types of `torro_chat` as they are; every conversation, message and person says
//! `untrusted: true`. Files and media (stage 4) are in [`files`].

pub mod files;

pub use files::*;

use std::path::PathBuf;

use base64::Engine;
use chrono::{DateTime, Utc};
use torro_chat::{
    ChatError, ChatSource, Conversation, Fetched, FilePiece, LinkCard, Mention, MessagePage, NotifyLevel, Outgoing,
    OwnStatus, Person, Picture, PreparedFile as WindowFile, PreviewSize as WindowSize, SaveTo, SearchHit, Sending,
    SharedItem as WindowItem, SharedKind as WindowKind, SharedPage, StatusChange, UploadProgress as WindowProgress,
    UploadState as WindowState, Uploaded,
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

    async fn messages_around(
        &self,
        conversation: String,
        around: String,
        limit: Option<u32>,
    ) -> Result<MessagePage, ChatError> {
        let request = Request::ChatGetMessages { conversation, before: None, after: None, around: Some(around), limit };
        match self.ask(request).await? {
            Response::ChatMessages(page) => Ok(page),
            other => Err(unexpected(other)),
        }
    }

    async fn send_files(&self, message: Outgoing, files: Vec<String>) -> Result<Sending, ChatError> {
        let Outgoing { conversation, text, reply_to, silent } = message;
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
            Response::ChatSending { id } => Ok(Sending::Uploading(id)),
            Response::ChatSent { id } => Ok(Sending::Sent(id)),
            other => Err(unexpected(other)),
        }
    }

    async fn prepare_file(&self, account: String, path: String) -> Result<WindowFile, ChatError> {
        match self.ask(Request::ChatPrepareFile { account, source: FileSource::Path { path } }).await? {
            Response::ChatPreparedFile { file } => Ok(window_file(*file)),
            other => Err(unexpected(other)),
        }
    }

    async fn upload_file(&self, piece: FilePiece) -> Result<Uploaded, ChatError> {
        let FilePiece { account, upload, offset, bytes, name, last } = piece;
        let base64 = FileData(base64::engine::general_purpose::STANDARD.encode(bytes));
        match self.ask(Request::ChatUploadFile { account, upload, offset, base64, name, finish: last }).await? {
            Response::ChatFileUpload { upload, received } => Ok(Uploaded::Piece { upload, received }),
            Response::ChatPreparedFile { file } => Ok(Uploaded::File(window_file(*file))),
            other => Err(unexpected(other)),
        }
    }

    async fn release_files(&self, account: String, ids: Vec<String>) -> Result<(), ChatError> {
        self.ok(Request::ChatReleaseFiles { account, ids }).await
    }

    async fn upload(&self, id: String) -> Result<WindowProgress, ChatError> {
        match self.ask(Request::ChatGetUpload { id }).await? {
            Response::ChatUpload { upload } => Ok(window_progress(upload)),
            other => Err(unexpected(other)),
        }
    }

    async fn cancel_upload(&self, id: String) -> Result<(), ChatError> {
        self.ok(Request::ChatCancelUpload { id }).await
    }

    async fn preview(
        &self,
        id: String,
        file: Option<String>,
        card: bool,
        size: WindowSize,
    ) -> Result<Picture, ChatError> {
        let size = match size {
            WindowSize::Small => PreviewSize::Small,
            WindowSize::Large => PreviewSize::Large,
        };
        match self.ask(Request::ChatGetPreview { id, file, card, size }).await? {
            Response::ChatPicture { picture } => window_picture(picture),
            other => Err(unexpected(other)),
        }
    }

    async fn attachment(&self, id: String, file: String) -> Result<Fetched, ChatError> {
        match self.ask(Request::ChatGetAttachment { id, file }).await? {
            Response::ChatAttachment { attachment } => Ok(Fetched {
                path: attachment.path,
                name: attachment.name,
                size: attachment.size,
                content_type: attachment.content_type,
            }),
            other => Err(unexpected(other)),
        }
    }

    async fn save_attachment(&self, id: String, file: String, to: SaveTo) -> Result<String, ChatError> {
        let to = match to {
            SaveTo::Downloads => crate::mail::SaveTo::Downloads,
            SaveTo::Target(target) => crate::mail::SaveTo::Target(target),
            SaveTo::File(path) => crate::mail::SaveTo::File(path),
            SaveTo::Folder(path) => crate::mail::SaveTo::Folder(path),
        };
        match self.ask(Request::ChatSaveAttachment { id, file, to }).await? {
            Response::ChatSaved { path } => Ok(path),
            other => Err(unexpected(other)),
        }
    }

    async fn shared(
        &self,
        conversation: String,
        kind: Option<WindowKind>,
        before: Option<String>,
        limit: Option<u32>,
    ) -> Result<SharedPage, ChatError> {
        let kind = kind.map(wire_kind);
        match self.ask(Request::ChatListShared { conversation, kind, before, limit }).await? {
            Response::ChatShared { items, more } => {
                Ok(SharedPage { items: items.into_iter().map(window_item).collect(), more })
            }
            other => Err(unexpected(other)),
        }
    }

    async fn show_card(&self, id: String) -> Result<Option<LinkCard>, ChatError> {
        match self.ask(Request::ChatShowCard { id }).await? {
            Response::ChatCard { card } => Ok(card),
            other => Err(unexpected(other)),
        }
    }
}

// The wire types of stage 4 and the windows' own (`torro_chat::files`): bytes travel as Base64 on the
// wire, the windows hold the bytes themselves.

/// Bytes of Base64 text; `None` when it is none (a broken preview shows no picture, it fails nothing).
fn decoded(data: &FileData) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(&data.0).ok()
}

/// A prepared file as the windows hold it.
fn window_file(file: PreparedFile) -> WindowFile {
    WindowFile {
        preview: file.preview.as_ref().and_then(decoded),
        id: file.id,
        name: file.name,
        size: file.size,
        content_type: file.content_type,
        width: file.width,
        height: file.height,
    }
}

/// How sending goes, as the windows read it.
fn window_progress(upload: UploadProgress) -> WindowProgress {
    WindowProgress {
        id: upload.id,
        state: match upload.state {
            UploadState::Uploading => WindowState::Uploading,
            UploadState::Sharing => WindowState::Sharing,
            UploadState::Sent => WindowState::Sent,
            UploadState::Cancelled => WindowState::Cancelled,
            UploadState::Failed => WindowState::Failed,
        },
        sent: upload.sent,
        total: upload.total,
        messages: upload.messages,
        code: upload.code,
        message: upload.message,
    }
}

/// A picture of the server with its bytes; an error when they are no Base64.
fn window_picture(picture: ChatPicture) -> Result<Picture, ChatError> {
    let bytes = decoded(&picture.base64).ok_or_else(|| ChatError::Other("The picture could not be read.".into()))?;
    Ok(Picture { content_type: picture.content_type, bytes, width: picture.width, height: picture.height })
}

/// The wire's word for a kind of shared item.
fn wire_kind(kind: WindowKind) -> SharedKind {
    match kind {
        WindowKind::Media => SharedKind::Media,
        WindowKind::File => SharedKind::File,
        WindowKind::Voice => SharedKind::Voice,
        WindowKind::Audio => SharedKind::Audio,
        WindowKind::Link => SharedKind::Link,
        WindowKind::Location => SharedKind::Location,
        WindowKind::Deckcard => SharedKind::Deckcard,
        WindowKind::Recording => SharedKind::Recording,
        WindowKind::Other => SharedKind::Other,
    }
}

/// A shared item as the windows read it.
fn window_item(item: SharedItem) -> WindowItem {
    let kind = match item.kind {
        SharedKind::Media => WindowKind::Media,
        SharedKind::File => WindowKind::File,
        SharedKind::Voice => WindowKind::Voice,
        SharedKind::Audio => WindowKind::Audio,
        SharedKind::Link => WindowKind::Link,
        SharedKind::Location => WindowKind::Location,
        SharedKind::Deckcard => WindowKind::Deckcard,
        SharedKind::Recording => WindowKind::Recording,
        SharedKind::Other => WindowKind::Other,
    };
    WindowItem {
        message: item.message,
        kind,
        file: item.file,
        name: item.name,
        size: item.size,
        content_type: item.content_type,
        author: item.author,
        at: item.at,
        preview: item.preview,
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
            Request::ChatGetMessages { conversation, around: Some(around), limit, .. } => {
                demo.messages_around(conversation, around, limit).await.map(Response::ChatMessages)
            }
            Request::ChatGetMessages { conversation, before, after, limit, .. } => {
                demo.messages(conversation, before, after, limit).await.map(Response::ChatMessages)
            }
            Request::ChatSendMessage { conversation, text, reply_to, silent, files, .. } if !files.is_empty() => {
                let message = Outgoing { conversation, text, reply_to, silent };
                demo.send_files(message, files).await.map(|sending| match sending {
                    Sending::Uploading(id) => Response::ChatSending { id },
                    Sending::Sent(id) => Response::ChatSent { id },
                })
            }
            Request::ChatSendMessage { conversation, text, reply_to, silent, .. } => {
                demo.send(Outgoing { conversation, text, reply_to, silent }).await.map(|id| Response::ChatSent { id })
            }
            Request::ChatPrepareFile { account, source: FileSource::Path { path } } => {
                demo.prepare_file(account, path).await.map(|file| Response::ChatPreparedFile { file: wire_file(file) })
            }
            Request::ChatUploadFile { account, upload, offset, base64, name, finish } => {
                let bytes = base64::engine::general_purpose::STANDARD.decode(&base64.0).expect("Base64");
                let piece = FilePiece { account, upload, offset, bytes, name, last: finish };
                demo.upload_file(piece).await.map(|uploaded| match uploaded {
                    Uploaded::Piece { upload, received } => Response::ChatFileUpload { upload, received },
                    Uploaded::File(file) => Response::ChatPreparedFile { file: wire_file(file) },
                })
            }
            Request::ChatReleaseFiles { account, ids } => done(demo.release_files(account, ids).await),
            Request::ChatGetUpload { id } => demo.upload(id).await.map(|progress| Response::ChatUpload {
                upload: UploadProgress {
                    id: progress.id,
                    state: match progress.state {
                        WindowState::Uploading => UploadState::Uploading,
                        WindowState::Sharing => UploadState::Sharing,
                        WindowState::Sent => UploadState::Sent,
                        WindowState::Cancelled => UploadState::Cancelled,
                        WindowState::Failed => UploadState::Failed,
                    },
                    sent: progress.sent,
                    total: progress.total,
                    messages: progress.messages,
                    code: progress.code,
                    message: progress.message,
                },
            }),
            Request::ChatCancelUpload { id } => done(demo.cancel_upload(id).await),
            Request::ChatGetPreview { id, file, card, size } => {
                let size = if size == PreviewSize::Large { WindowSize::Large } else { WindowSize::Small };
                demo.preview(id, file, card, size).await.map(|picture| Response::ChatPicture {
                    picture: ChatPicture {
                        content_type: picture.content_type,
                        base64: FileData(base64::engine::general_purpose::STANDARD.encode(picture.bytes)),
                        width: picture.width,
                        height: picture.height,
                    },
                })
            }
            Request::ChatGetAttachment { id, file } => {
                demo.attachment(id, file).await.map(|fetched| Response::ChatAttachment {
                    attachment: ChatAttachment {
                        path: fetched.path,
                        name: fetched.name,
                        size: fetched.size,
                        content_type: fetched.content_type,
                        untrusted: true,
                    },
                })
            }
            Request::ChatSaveAttachment { id, file, to } => {
                let to = match to {
                    crate::mail::SaveTo::Downloads => SaveTo::Downloads,
                    crate::mail::SaveTo::Target(target) => SaveTo::Target(target),
                    crate::mail::SaveTo::File(path) => SaveTo::File(path),
                    crate::mail::SaveTo::Folder(path) => SaveTo::Folder(path),
                };
                demo.save_attachment(id, file, to).await.map(|path| Response::ChatSaved { path })
            }
            Request::ChatListShared { conversation, kind, before, limit } => {
                let kind = kind.map(|kind| match kind {
                    SharedKind::Media => WindowKind::Media,
                    SharedKind::File => WindowKind::File,
                    SharedKind::Voice => WindowKind::Voice,
                    SharedKind::Link => WindowKind::Link,
                    _ => WindowKind::Other,
                });
                demo.shared(conversation, kind, before, limit).await.map(|page| Response::ChatShared {
                    items: page
                        .items
                        .into_iter()
                        .map(|item| SharedItem {
                            message: item.message,
                            kind: wire_kind(item.kind),
                            file: item.file,
                            name: item.name,
                            size: item.size,
                            content_type: item.content_type,
                            author: item.author,
                            at: item.at,
                            preview: item.preview,
                            blurhash: None,
                            untrusted: true,
                        })
                        .collect(),
                    more: page.more,
                })
            }
            Request::ChatShowCard { id } => demo.show_card(id).await.map(|card| Response::ChatCard { card }),
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

    /// A prepared file of the demo as the core sends it.
    fn wire_file(file: WindowFile) -> Box<PreparedFile> {
        Box::new(PreparedFile {
            preview: file.preview.map(|png| FileData(base64::engine::general_purpose::STANDARD.encode(png))),
            id: file.id,
            name: file.name,
            size: file.size,
            content_type: file.content_type,
            width: file.width,
            height: file.height,
            untrusted: true,
        })
    }

    /// Stage 4: each file method is one request, bytes cross as Base64 and come back as bytes.
    #[tokio::test]
    async fn the_windows_send_and_read_files_over_the_socket() {
        let (chat, serving, _dir) = core().await;
        let design = torro_chat::demo::DESIGN;
        let picture = torro_chat::message_id(design, 3);
        let preview =
            chat.preview(picture.clone(), Some("5103".into()), false, WindowSize::Small).await.expect("small");
        assert!(preview.bytes.starts_with(&[0x89, b'P', b'N', b'G']), "the bytes themselves, not Base64");
        let shared = chat.shared(design.into(), Some(WindowKind::Media), None, None).await.expect("shared");
        assert!(shared.items.iter().any(|item| item.message == picture));
        let around = chat.messages_around(design.into(), picture.clone(), Some(2)).await.expect("around");
        assert!(around.messages.iter().any(|message| message.id == picture));
        let hidden = chat.attachment(torro_chat::message_id(design, 9), "5109".into()).await;
        assert!(matches!(hidden, Err(ChatError::Forbidden(_))));
        let saved = chat.save_attachment(torro_chat::message_id(design, 7), "5107".into(), SaveTo::Downloads).await;
        assert!(saved.expect("saved").ends_with("Icon guide.pdf"));
        let card = chat.show_card(torro_chat::message_id(design, 11)).await.expect("asked");
        assert_eq!(card.map(|card| card.domain), Some("example.com".into()));

        let png = torro_chat::demo::picture(2, 12, 8);
        let piece = FilePiece { account: ACCOUNT.into(), bytes: png, last: true, ..FilePiece::default() };
        let Uploaded::File(file) = chat.upload_file(piece).await.expect("uploaded") else { panic!("one piece") };
        assert_eq!((file.width, file.height), (Some(12), Some(8)));
        let message = Outgoing { conversation: design.into(), ..Outgoing::default() };
        let Sending::Uploading(id) = chat.send_files(message, vec![file.id]).await.expect("sending") else {
            panic!("files upload first");
        };
        let mut states = Vec::new();
        for _ in 0..6 {
            states.push(chat.upload(id.clone()).await.expect("progress").state);
        }
        assert_eq!(states[0], WindowState::Uploading, "a quarter at a time");
        assert_eq!(states[4], WindowState::Sharing);
        assert_eq!(states[5], WindowState::Sent);
        let prepared = chat.prepare_file(ACCOUNT.into(), "/home/anna/a.pdf".into()).await.expect("prepared");
        chat.release_files(ACCOUNT.into(), vec![prepared.id]).await.expect("released");
        let stalled = chat.prepare_file(ACCOUNT.into(), "/home/anna/b.pdf".into()).await.expect("prepared");
        let message = Outgoing { conversation: design.into(), ..Outgoing::default() };
        let Sending::Uploading(id) = chat.send_files(message, vec![stalled.id]).await.expect("sending") else {
            panic!("files upload first");
        };
        chat.cancel_upload(id.clone()).await.expect("cancelled");
        assert_eq!(chat.upload(id).await.expect("progress").state, WindowState::Cancelled);
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
