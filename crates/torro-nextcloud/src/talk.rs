//! Talk (`spreed`): the person's conversations with what is unread, read for the inbox source
//! `talk:<login>@<host>` (task E5.9), marking a conversation read, and the chat of a conversation
//! for the chat app (#643): reading, waiting for, sending, editing, deleting and reacting to
//! messages, mentions, one-to-one conversations, favourites, notification levels and searching.
//!
//! - Conversations: `GET /ocs/v2.php/apps/spreed/api/v4/room?noStatusUpdate=1` (operation
//!   `room-get-rooms`). `noStatusUpdate=1` keeps polling from setting the person "online". One:
//!   `GET …/api/v4/room/{token}`; a new one-to-one: `POST …/api/v4/room` with `roomType=1` and
//!   `invite=<user id>` (200 with the one that exists, 201 with a new one). Favourite:
//!   `POST`/`DELETE …/room/{token}/favorite`; notification level: `POST …/room/{token}/notify`
//!   with `level`. <https://nextcloud-talk.readthedocs.io/en/latest/conversation/>
//! - Read marker: `POST /ocs/v2.php/apps/spreed/api/v1/chat/{token}/read` with
//!   `{"lastReadMessage": <id>}` (operation `chat-set-read-marker`, capability `chat-read-marker`);
//!   unread: `DELETE` there (capability `chat-unread`).
//! - Messages: `GET …/api/v1/chat/{token}` with `lookIntoFuture=0` (history, newest first, page by
//!   `lastKnownMessageId` = the header `X-Chat-Last-Given`) or `lookIntoFuture=1` with `timeout`
//!   (waits on the server for new ones, `304` when none came). Reading never changes anything:
//!   always `setReadMarker=0` and `noStatusUpdate=1`, with `chat-keep-notifications` also
//!   `markNotificationsAsRead=0`. Around one message: `GET …/chat/{token}/{id}/context`
//!   (capability `chat-get-context`). Send: `POST …/chat/{token}` with `message`, `replyTo`,
//!   `referenceId`, `silent`; edit: `PUT …/chat/{token}/{id}` (24 h); delete: `DELETE` there
//!   (6 h without `delete-messages-unlimited`). Mentions: `GET …/chat/{token}/mentions`.
//!   <https://nextcloud-talk.readthedocs.io/en/latest/chat/>
//! - Reactions: `POST`/`DELETE …/api/v1/reaction/{token}/{id}` with `reaction` (in the body to
//!   add, in the query to remove). <https://nextcloud-talk.readthedocs.io/en/latest/reaction/>
//! - Search: the unified search provider `talk-message`
//!   (`GET /ocs/v2.php/search/providers/talk-message/search?term=…&limit=…`), whose entries name
//!   the conversation and the message in `attributes` (`lib/Search/MessageSearch.php` of Talk).
//!
//! Schemas `Room`, `RoomLastMessage`, `ChatMessage`, `ChatMessageWithParent`,
//! `DeletedChatMessage`, `RichObjectParameter`, `ChatMentionSuggestion` and the token pattern
//! `^[a-z0-9]{4,30}$` in <https://github.com/nextcloud/spreed/blob/main/openapi.json>;
//! conversation types, actor types and notification levels in
//! <https://nextcloud-talk.readthedocs.io/en/latest/constants/>.
//!
//! The long poll: the chat docs still say "60 at most", but Talk's controller (and its
//! openapi.json) cut `timeout` to 30 s, so [`WAIT_MAX_SECS`] is 30.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::notifications::Person;
use crate::ocs::{Answer, Nextcloud, OcsError, Reply, TalkError};
use crate::rich;

const ROOMS_PATH: &str = "/ocs/v2.php/apps/spreed/api/v4/room?noStatusUpdate=1";
const ROOM: &str = "/ocs/v2.php/apps/spreed/api/v4/room";
const CHAT: &str = "/ocs/v2.php/apps/spreed/api/v1/chat";
const REACTION: &str = "/ocs/v2.php/apps/spreed/api/v1/reaction";
const SEARCH: &str = "/ocs/v2.php/search/providers/talk-message/search";

/// The most messages one read asks for (Talk's limit).
pub const MESSAGES_MAX: u32 = 200;

/// The most messages [`Nextcloud::messages_around`] asks for in each direction (Talk's limit).
pub const CONTEXT_MAX: u32 = 100;

/// The longest a long poll waits on the server, in seconds (Talk cuts longer ones to this).
pub const WAIT_MAX_SECS: u32 = 30;

/// How much longer than the server's wait the HTTP request may take before it counts as lost.
const WAIT_GRACE: Duration = Duration::from_secs(15);

/// The token of a conversation: 4 to 30 of `a`–`z` and `0`–`9`, the pattern of Talk's API. The
/// only way to name a conversation to [`Nextcloud::mark_read`], so no other text gets into its
/// address; also what the link `<server>/call/<token>` is built from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoomToken(String);

impl RoomToken {
    /// `token` as a conversation token, or `None` when it does not fit the pattern.
    pub fn parse(token: &str) -> Option<RoomToken> {
        let fits =
            (4..=30).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        fits.then(|| RoomToken(token.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RoomToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The kind of a conversation (`type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoomKind {
    /// 1: two people.
    OneToOne,
    /// 2: invited people.
    Group,
    /// 3: open by link, also to guests.
    Public,
    /// 4: Talk's own news.
    Changelog,
    /// 5: a one-to-one conversation whose other person was deleted.
    FormerOneToOne,
    /// 6: notes to oneself.
    NoteToSelf,
    /// A type this code does not know.
    Other,
}

impl RoomKind {
    /// The kind of Talk's number for it.
    pub fn from_talk(number: i64) -> RoomKind {
        match number {
            1 => RoomKind::OneToOne,
            2 => RoomKind::Group,
            3 => RoomKind::Public,
            4 => RoomKind::Changelog,
            5 => RoomKind::FormerOneToOne,
            6 => RoomKind::NoteToSelf,
            _ => RoomKind::Other,
        }
    }
}

/// When the person wants to hear of a conversation (`notificationLevel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationLevel {
    /// 0: the default, which is "always" for one-to-one conversations and "mention" for others.
    Default,
    /// 1: every message.
    Always,
    /// 2: only when mentioned.
    Mention,
    /// 3: never.
    Never,
    /// A level this code does not know.
    Other,
}

impl NotificationLevel {
    /// The level of Talk's number for it.
    pub fn from_talk(number: i64) -> NotificationLevel {
        match number {
            0 => NotificationLevel::Default,
            1 => NotificationLevel::Always,
            2 => NotificationLevel::Mention,
            3 => NotificationLevel::Never,
            _ => NotificationLevel::Other,
        }
    }

    /// Talk's number for the level; `None` for [`NotificationLevel::Other`].
    pub fn to_talk(self) -> Option<i64> {
        match self {
            NotificationLevel::Default => Some(0),
            NotificationLevel::Always => Some(1),
            NotificationLevel::Mention => Some(2),
            NotificationLevel::Never => Some(3),
            NotificationLevel::Other => None,
        }
    }
}

/// One conversation as checked from the server's answer ([`read_rooms`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub token: RoomToken,
    pub kind: RoomKind,
    /// The name to show, one line: `displayName` (for one-to-one conversations the other person's
    /// name), else `name`; foreign text.
    pub name: String,
    /// Unread messages (`unreadMessages`, 0 when missing).
    pub unread_messages: u64,
    /// The person was mentioned since their last visit, also by `@all` (`unreadMention`).
    pub unread_mention: bool,
    /// The person was mentioned by name (`unreadMentionDirect`, `false` on servers without it).
    pub unread_mention_direct: bool,
    pub notification_level: NotificationLevel,
    /// The last message, when the server sent one.
    pub last_message: Option<LastMessage>,
    /// When something last happened in the conversation (`lastActivity`).
    pub last_activity: DateTime<Utc>,
    /// The id of the last message the person read (`lastReadMessage`), when positive.
    pub last_read: Option<u64>,
    /// A call is running (`hasCall`).
    pub has_call: bool,
    /// What the call carries, all participants together (`callFlag`: 1 in call, 2 audio,
    /// 4 video, 8 phone; 0 without a call).
    pub call_flag: u32,
    /// The person marked the conversation a favourite (`isFavorite`).
    pub favorite: bool,
    /// The conversation is locked, nobody writes (`readOnly` 1).
    pub read_only: bool,
    /// The lobby keeps the person out (`lobbyState` 1, and the person is no owner or moderator
    /// and may not ignore the lobby).
    pub lobby: bool,
    /// For a one-to-one conversation the other person's user id (Talk's `name` there), one line;
    /// `None` for every other kind.
    pub other_user: Option<String>,
    /// The conversation lives on another server (`remoteServer` is set).
    pub federated: bool,
    /// The description in one line (`description`, empty when there is none); foreign text.
    pub description: String,
    /// The person may start a call (`canStartCall`, `true` on servers without the flag).
    pub can_start_call: bool,
}

/// The last message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastMessage {
    /// The message id; `None` for a message of a federated conversation (a proxy message has none).
    pub id: Option<u64>,
    /// The author when it is a user of this server (`actorType` `users`).
    pub author: Option<Person>,
    /// The author's name to show, one line, whoever wrote it (also guests, bots, federated users).
    pub author_name: String,
    /// The text with its placeholders replaced ([`rich::plain`]).
    pub text: String,
    /// A message Talk wrote itself (`messageType` `system`: "Anna started a call").
    pub system: bool,
}

/// Who wrote a message (`actorType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActorType {
    /// `users`: a user of this server.
    Users,
    /// `guests` (also `emails`): a guest.
    Guests,
    /// `bots`: a bot, a command or the changelog.
    Bots,
    /// `federated_users`: a user of another server.
    Federated,
    /// `bridged`: someone on another chat service, through Matterbridge.
    Bridged,
    /// `deleted_users`: a user who no longer exists.
    Deleted,
    /// Anything else.
    Other,
}

impl ActorType {
    /// The actor type of Talk's word for it.
    pub fn from_talk(word: &str) -> ActorType {
        match word {
            "users" => ActorType::Users,
            "guests" | "emails" => ActorType::Guests,
            "bots" => ActorType::Bots,
            "federated_users" => ActorType::Federated,
            "bridged" => ActorType::Bridged,
            "deleted_users" => ActorType::Deleted,
            _ => ActorType::Other,
        }
    }
}

/// What kind of message it is (`messageType`, for system messages also `systemMessage`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageKind {
    /// `comment`: what someone wrote, also a shared file (`{file}`).
    Comment,
    /// `system`: Talk wrote it. `event` is the untranslated `systemMessage` (`call_started`,
    /// `call_missed`, `message_deleted`, `message_edited`, `reaction`, `reaction_revoked`,
    /// `reaction_deleted` …), only ASCII letters, digits, `_` and `-`; empty for anything else.
    /// `message_deleted` and `message_edited` carry the changed message as [`ChatMessage::parent`].
    System { event: String },
    /// `comment_deleted`: a deleted message, its text is Talk's "Message deleted by …".
    Deleted,
    /// `voice-message`: a shared voice recording (the file in [`ChatMessage::files`]).
    Voice,
    /// `command`: the answer of a bot command.
    Command,
    /// Anything else (also call recordings, `record-audio`, `record-video`).
    Other,
}

/// What a mention names (the `type` of a `mention-…` parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MentionKind {
    /// `user`: a user of this server.
    User,
    /// `user` with a `server`: a user of another server.
    Federated,
    /// `guest`.
    Guest,
    /// `user-group`: a group.
    Group,
    /// `call`: everyone in the conversation (`@all`); the id is the conversation's token.
    All,
}

/// A mention in a message, from its `messageParameters`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Mention {
    /// The id of what is mentioned (user id, `guest/…`, group id, token), one line; foreign text.
    pub id: String,
    /// The name shown in the text, one line; foreign text.
    pub name: String,
    pub kind: MentionKind,
}

/// A file shared into a conversation (a `file` parameter of a message). No duration: Talk sends
/// none, not even for voice messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SharedFile {
    /// The file id on the server (for previews: `/index.php/core/preview?fileId=…`).
    pub id: u64,
    /// The file name, one line; foreign text.
    pub name: String,
    /// The size in bytes (0 when the server sent none).
    pub size: u64,
    /// The media type (`image/jpeg`); `application/octet-stream` when the server sent none that
    /// looks like one.
    pub mimetype: String,
    /// The server can make a preview image (`preview-available` `yes`).
    pub preview_available: bool,
    /// The sharer does not want it downloaded (`hide-download` `yes`).
    pub hide_download: bool,
    /// Width and height in pixels, only for images.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// A small blurred stand-in for the image (BlurHash), only for images.
    pub blurhash: Option<String>,
    /// The path in the person's files (`Talk/Plan.pdf`), one line; foreign text, only for
    /// showing, never for addressing a file.
    pub path: Option<String>,
}

/// The message a message answers (`parent`), as far as the chat shows it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Parent {
    pub id: u64,
    /// The author's name, one line (the actor id when the name is empty).
    pub author_name: String,
    /// The text with its placeholders replaced; empty when only the id is known.
    pub text: String,
    /// The message was deleted (`deleted`, or `messageType` `comment_deleted`).
    pub deleted: bool,
}

/// One chat message as checked from the server's answer ([`read_messages`]). All its texts are
/// foreign: made plain, never instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: u64,
    /// When it was written (`timestamp`).
    pub at: DateTime<Utc>,
    pub actor_type: ActorType,
    /// The author's id (`actorId`), one line.
    pub actor_id: String,
    /// The author's name to show, one line: `actorDisplayName`; for users, federated users, bots
    /// and bridged users the actor id when that is empty, for guests and deleted users empty.
    pub author_name: String,
    /// The text with its placeholders replaced ([`rich::plain`]), line breaks kept.
    pub text: String,
    pub kind: MessageKind,
    /// Who the text mentions (parameters `mention-…` of type `user`, `call`, `guest`,
    /// `user-group`).
    pub mentions: Vec<Mention>,
    /// The files it shares (parameters of type `file`).
    pub files: Vec<SharedFile>,
    /// The message it answers, or for `message_deleted` and `message_edited` the changed one.
    pub parent: Option<Box<Parent>>,
    /// The id the sender gave it (`referenceId`), when set and plain ASCII of at most 128.
    pub reference_id: Option<String>,
    /// How often each emoji was given (`reactions`).
    pub reactions: BTreeMap<String, u64>,
    /// The emojis the person gave (`reactionsSelf`).
    pub own_reactions: Vec<String>,
    /// When it was last edited (`lastEditTimestamp`).
    pub edited_at: Option<DateTime<Utc>>,
    /// Sent without notifying anyone (`silent`).
    pub silent: bool,
    /// The text is meant as Markdown (`markdown`).
    pub markdown: bool,
}

/// What a read of messages returned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Messages {
    /// The messages, oldest first, each id once.
    pub messages: Vec<ChatMessage>,
    /// `X-Chat-Last-Given`: where the next page starts (the `before` of the next
    /// [`Nextcloud::messages`], the `after` of the next [`Nextcloud::wait_messages`]). It may name a
    /// message that is not in the list (one the person cannot see).
    pub last_given: Option<u64>,
    /// `X-Chat-Last-Common-Read`: the last message everyone with public read status has read;
    /// absent when the person keeps their own read status private.
    pub last_common_read: Option<u64>,
}

/// How a message is sent ([`Nextcloud::send_message`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendOptions {
    /// The message this one answers (`replyTo`).
    pub reply_to: Option<u64>,
    /// An id of the sender's own to find the message again (`referenceId`, capability
    /// `chat-reference-id`; Talk suggests a random SHA-256 in hex).
    pub reference_id: Option<String>,
    /// Without notifications, even for mentions (`silent`, capability `silent-send`).
    pub silent: bool,
}

/// Someone or something that can be mentioned in a conversation ([`Nextcloud::mention_suggestions`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MentionSuggestion {
    /// The id (`id`), one line: the user id for users, for avatars and status.
    pub id: String,
    /// What goes into the text as `@"<mention_id>"` (`mentionId`, else `id`), one line.
    pub mention_id: String,
    /// The name to show (`label`), one line.
    pub label: String,
    /// Where it comes from (`source`: `users`, `federated_users`, `groups`, `guests`, `calls` …),
    /// only ASCII letters, digits, `_` and `-`; empty for anything else.
    pub source: String,
}

/// A message found by [`Nextcloud::search_messages`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundMessage {
    pub token: RoomToken,
    pub message_id: u64,
    /// "Author in conversation" as the server words it, one line.
    pub title: String,
    /// The part of the text around the term, one line.
    pub text: String,
    /// When it was written, when the server says.
    pub at: Option<DateTime<Utc>>,
}

/// What a refused request was meant to do: Talk's 400 means "too late" only for changing a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    Other,
    Edit,
    Delete,
}

impl Nextcloud {
    /// The person's conversations, each token once. [`OcsError::MissingApp`] when Talk is not
    /// there; other errors as every OCS call.
    pub async fn rooms(&self) -> Result<Vec<Room>, OcsError> {
        match self.get(ROOMS_PATH, None).await? {
            Reply::Data { data, .. } => read_rooms(&data),
            Reply::Empty => Ok(Vec::new()),
            Reply::NotModified => Err(OcsError::Other("Nextcloud sent no conversations".into())),
        }
    }

    /// Marks the conversation `token` read up to and including the message `message_id` (needs
    /// the capability `chat-read-marker`); later messages stay unread. [`OcsError::MissingApp`]
    /// when Talk is not there or the person is no longer in the conversation; other errors as
    /// every OCS call.
    pub async fn mark_read(&self, token: &RoomToken, message_id: u64) -> Result<(), OcsError> {
        let path = format!("/ocs/v2.php/apps/spreed/api/v1/chat/{token}/read");
        self.call(Method::POST, &path, Some(&json!({ "lastReadMessage": message_id }))).await.map(|_| ())
    }

    /// Marks the conversation `token` unread (capability `chat-unread`).
    pub async fn mark_unread(&self, token: &RoomToken) -> Result<(), TalkError> {
        self.talk(Method::DELETE, &format!("{CHAT}/{token}/read"), &[], None, Intent::Other).await.map(|_| ())
    }

    /// The conversation `token`. [`TalkError::NotFound`] when it is gone or the person is no
    /// longer in it.
    pub async fn room(&self, token: &RoomToken) -> Result<Room, TalkError> {
        let data = self.talk(Method::GET, &format!("{ROOM}/{token}"), &[], None, Intent::Other).await?;
        data.as_ref().and_then(read_room).ok_or_else(|| unreadable("conversation"))
    }

    /// The one-to-one conversation with the user `user_id`: the one there is, else a new one.
    /// [`TalkError::NotFound`] when there is no such user; [`TalkError::Invalid`] for an empty id.
    pub async fn create_one_to_one(&self, user_id: &str) -> Result<Room, TalkError> {
        if !is_line(user_id) {
            return Err(TalkError::Invalid);
        }
        let body = json!({ "roomType": 1, "invite": user_id });
        let data = self.talk(Method::POST, ROOM, &[], Some(&body), Intent::Other).await?;
        data.as_ref().and_then(read_room).ok_or_else(|| unreadable("conversation"))
    }

    /// Marks the conversation `token` a favourite, or no longer one.
    pub async fn set_favorite(&self, token: &RoomToken, favorite: bool) -> Result<(), TalkError> {
        let method = if favorite { Method::POST } else { Method::DELETE };
        self.talk(method, &format!("{ROOM}/{token}/favorite"), &[], None, Intent::Other).await.map(|_| ())
    }

    /// Sets when the person hears of the conversation `token` (capability `notification-levels`).
    /// [`TalkError::Invalid`] for [`NotificationLevel::Other`].
    pub async fn set_notification_level(&self, token: &RoomToken, level: NotificationLevel) -> Result<(), TalkError> {
        let level = level.to_talk().ok_or(TalkError::Invalid)?;
        let body = json!({ "level": level });
        self.talk(Method::POST, &format!("{ROOM}/{token}/notify"), &[], Some(&body), Intent::Other).await.map(|_| ())
    }

    /// Up to `limit` (1 to [`MESSAGES_MAX`]) messages of the conversation `token` before the
    /// message `before` (the newest without it), oldest first. Nothing changes on the server:
    /// neither the read marker nor the status, and with `keep_notifications` (only when the server
    /// has `chat-keep-notifications`) neither the notifications. No older messages is an empty list.
    pub async fn messages(
        &self,
        token: &RoomToken,
        before: Option<u64>,
        limit: u32,
        keep_notifications: bool,
    ) -> Result<Messages, TalkError> {
        let mut query = reading(limit.clamp(1, MESSAGES_MAX), keep_notifications);
        query.push(("lookIntoFuture", "0".into()));
        if let Some(before) = before {
            query.push(("lastKnownMessageId", before.to_string()));
        }
        let url = self.endpoint(&format!("{CHAT}/{token}"), &[], &query)?;
        read_answer(self.exchange(Method::GET, url, None, None).await?)
    }

    /// Waits up to `timeout_secs` (at most [`WAIT_MAX_SECS`]) on the server for messages after the
    /// message `after` in the conversation `token`, and returns up to `limit` of them, oldest
    /// first; none came is an empty list. Changes nothing on the server, as [`Nextcloud::messages`].
    pub async fn wait_messages(
        &self,
        token: &RoomToken,
        after: u64,
        timeout_secs: u32,
        limit: u32,
        keep_notifications: bool,
    ) -> Result<Messages, TalkError> {
        let timeout = timeout_secs.min(WAIT_MAX_SECS);
        let mut query = reading(limit.clamp(1, MESSAGES_MAX), keep_notifications);
        query.push(("lookIntoFuture", "1".into()));
        query.push(("lastKnownMessageId", after.to_string()));
        query.push(("timeout", timeout.to_string()));
        let url = self.endpoint(&format!("{CHAT}/{token}"), &[], &query)?;
        let wait = Duration::from_secs(u64::from(timeout)) + WAIT_GRACE;
        read_answer(self.exchange(Method::GET, url, None, Some(wait)).await?)
    }

    /// The message `id` of the conversation `token` with up to `limit` (1 to [`CONTEXT_MAX`])
    /// messages before and after it, oldest first (capability `chat-get-context`).
    pub async fn messages_around(&self, token: &RoomToken, id: u64, limit: u32) -> Result<Messages, TalkError> {
        let query = [("limit", limit.clamp(1, CONTEXT_MAX).to_string())];
        let url = self.endpoint(&format!("{CHAT}/{token}/{id}/context"), &[], &query)?;
        read_answer(self.exchange(Method::GET, url, None, None).await?)
    }

    /// Sends `text` into the conversation `token` and returns the message as the server keeps it.
    /// [`TalkError::Invalid`] for an empty text, [`TalkError::TooLong`] above the server's limit,
    /// [`TalkError::ReadOnly`] and [`TalkError::Lobby`] when the conversation takes no messages.
    pub async fn send_message(
        &self,
        token: &RoomToken,
        text: &str,
        options: &SendOptions,
    ) -> Result<ChatMessage, TalkError> {
        if text.trim().is_empty() {
            return Err(TalkError::Invalid);
        }
        let mut body = Map::new();
        body.insert("message".into(), text.into());
        if let Some(reply_to) = options.reply_to {
            body.insert("replyTo".into(), reply_to.into());
        }
        if let Some(reference_id) = options.reference_id.as_deref().filter(|id| !id.is_empty()) {
            body.insert("referenceId".into(), reference_id.into());
        }
        if options.silent {
            body.insert("silent".into(), true.into());
        }
        let (path, body) = (format!("{CHAT}/{token}"), Value::Object(body));
        let data = self.talk(Method::POST, &path, &[], Some(&body), Intent::Other).await?;
        data.as_ref().and_then(read_message).ok_or_else(|| unreadable("message"))
    }

    /// Replaces the text of the message `id` (capability `edit-messages`). [`TalkError::TooLate`]
    /// after 24 hours, [`TalkError::Ocs`] with [`OcsError::Forbidden`] for someone else's message.
    pub async fn edit_message(&self, token: &RoomToken, id: u64, text: &str) -> Result<(), TalkError> {
        if text.trim().is_empty() {
            return Err(TalkError::Invalid);
        }
        let body = json!({ "message": text });
        self.talk(Method::PUT, &format!("{CHAT}/{token}/{id}"), &[], Some(&body), Intent::Edit).await.map(|_| ())
    }

    /// Deletes the message `id` (capability `delete-messages`). [`TalkError::TooLate`] after 6
    /// hours on servers without `delete-messages-unlimited`.
    pub async fn delete_message(&self, token: &RoomToken, id: u64) -> Result<(), TalkError> {
        self.talk(Method::DELETE, &format!("{CHAT}/{token}/{id}"), &[], None, Intent::Delete).await.map(|_| ())
    }

    /// Gives the message `id` the reaction `emoji` (capability `reactions`); giving it twice is
    /// fine. [`TalkError::Invalid`] for an empty emoji or one with spaces or control characters.
    pub async fn react(&self, token: &RoomToken, id: u64, emoji: &str) -> Result<(), TalkError> {
        let emoji = checked_emoji(emoji)?;
        let body = json!({ "reaction": emoji });
        self.talk(Method::POST, &format!("{REACTION}/{token}/{id}"), &[], Some(&body), Intent::Other).await.map(|_| ())
    }

    /// Takes the reaction `emoji` back from the message `id`.
    pub async fn unreact(&self, token: &RoomToken, id: u64, emoji: &str) -> Result<(), TalkError> {
        let query = [("reaction", checked_emoji(emoji)?.to_string())];
        self.talk(Method::DELETE, &format!("{REACTION}/{token}/{id}"), &query, None, Intent::Other).await.map(|_| ())
    }

    /// Up to `limit` (1 to 100) people, groups or "everyone" that `search` finds to mention in the
    /// conversation `token`.
    pub async fn mention_suggestions(
        &self,
        token: &RoomToken,
        search: &str,
        limit: u32,
    ) -> Result<Vec<MentionSuggestion>, TalkError> {
        let query = [("search", search.to_string()), ("limit", limit.clamp(1, 100).to_string())];
        let data = self.talk(Method::GET, &format!("{CHAT}/{token}/mentions"), &query, None, Intent::Other).await?;
        Ok(data
            .as_ref()
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(read_suggestion).collect())
            .unwrap_or_default())
    }

    /// Up to `limit` (1 to 100) messages in any of the person's conversations that hold `term`,
    /// through the unified search. [`TalkError::Ocs`] with [`OcsError::MissingApp`] when the server
    /// has no message search (no Talk).
    pub async fn search_messages(&self, term: &str, limit: u32) -> Result<Vec<FoundMessage>, TalkError> {
        let query = [("term", term.to_string()), ("limit", limit.clamp(1, 100).to_string())];
        let url = self.endpoint(SEARCH, &[], &query)?;
        let data = self.exchange(Method::GET, url, None, None).await?.success()?;
        let entries = data.as_ref().and_then(|data| data.get("entries")).and_then(Value::as_array);
        Ok(entries.map(|list| list.iter().filter_map(read_found).collect()).unwrap_or_default())
    }

    /// A Talk request: `method` on `path` (fixed parts and checked ids) with `query` and `body`;
    /// the `data` of a success, or the refusal as a [`TalkError`].
    async fn talk(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        intent: Intent,
    ) -> Result<Option<Value>, TalkError> {
        let url = self.endpoint(path, &[], query)?;
        let answer = self.exchange(method, url, body, None).await?;
        if answer.is_success() { Ok(answer.data) } else { Err(refusal(&answer, intent)) }
    }
}

/// The query every read of messages sends so it changes nothing on the server.
fn reading(limit: u32, keep_notifications: bool) -> Vec<(&'static str, String)> {
    let mut query = vec![
        ("limit", limit.to_string()),
        ("setReadMarker", "0".to_string()),
        ("noStatusUpdate", "1".to_string()),
        ("includeLastKnown", "0".to_string()),
    ];
    if keep_notifications {
        query.push(("markNotificationsAsRead", "0".into()));
    }
    query
}

/// The messages of a read's answer, with its paging headers; `304` is none.
fn read_answer(answer: Answer) -> Result<Messages, TalkError> {
    if !answer.is_success() {
        return Err(refusal(&answer, Intent::Other));
    }
    let messages = match &answer.data {
        Some(data) => read_messages(data)?,
        None => Vec::new(),
    };
    Ok(Messages {
        messages,
        last_given: answer.number("X-Chat-Last-Given"),
        last_common_read: answer.number("X-Chat-Last-Common-Read"),
    })
}

/// The [`TalkError`] for a refused Talk request. Only Talk's short error word is looked at
/// (`permission`, `age`); it never goes into the error.
fn refusal(answer: &Answer, intent: Intent) -> TalkError {
    match answer.status {
        404 => TalkError::NotFound,
        403 if answer.error_word() == Some("permission") => TalkError::Ocs(OcsError::Forbidden),
        403 => TalkError::ReadOnly,
        412 => TalkError::Lobby,
        413 => TalkError::TooLong,
        400 if intent == Intent::Delete => TalkError::TooLate,
        400 if intent == Intent::Edit && answer.error_word() == Some("age") => TalkError::TooLate,
        400 | 405 => TalkError::Invalid,
        status => TalkError::Ocs(OcsError::Other(format!("Nextcloud answered with HTTP {status}"))),
    }
}

fn unreadable(what: &str) -> TalkError {
    TalkError::Ocs(OcsError::Other(format!("Nextcloud sent no readable {what}")))
}

/// `emoji` when it is one short reaction: not empty, at most 32 bytes, no white space or control
/// characters.
fn checked_emoji(emoji: &str) -> Result<&str, TalkError> {
    let fits = !emoji.is_empty()
        && emoji.len() <= 32
        && !emoji.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\u{202A}'..='\u{202E}'));
    fits.then_some(emoji).ok_or(TalkError::Invalid)
}

/// Whether `text` is one non-empty line without control characters (an id to send in a body).
fn is_line(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= 255 && !text.chars().any(char::is_control)
}

/// Whether `word` is a plain word of Talk's (`call_started`, `federated_users`).
fn is_word(word: &str) -> bool {
    (1..=64).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

/// The conversations in the `data` of the room list (a JSON list), each token once. An entry is
/// left out when its token is no [`RoomToken`], it has no `type` or no `lastActivity`.
/// [`OcsError::Other`] when `data` is no list.
pub(crate) fn read_rooms(data: &Value) -> Result<Vec<Room>, OcsError> {
    let entries =
        data.as_array().ok_or_else(|| OcsError::Other("Nextcloud sent conversations that are no list".into()))?;
    let mut seen = HashSet::new();
    Ok(entries.iter().filter_map(read_room).filter(|room| seen.insert(room.token.clone())).collect())
}

fn read_room(entry: &Value) -> Option<Room> {
    let text = |key: &str| entry.get(key).and_then(Value::as_str).unwrap_or_default();
    let flag = |key: &str| entry.get(key).and_then(Value::as_bool).unwrap_or(false);
    let number = |key: &str| entry.get(key).and_then(Value::as_i64).unwrap_or(0);
    let token = RoomToken::parse(entry.get("token")?.as_str()?)?;
    let kind = RoomKind::from_talk(entry.get("type")?.as_i64()?);
    let display = rich::line(text("displayName"), &Value::Null);
    let name = if display.is_empty() { rich::line(text("name"), &Value::Null) } else { display };
    let last_activity = DateTime::from_timestamp(entry.get("lastActivity")?.as_i64()?, 0)?;
    // Owners (1), moderators (2) and guest moderators (6) pass the lobby, as does whoever has
    // the permission "can ignore lobby" (8).
    let passes_lobby = matches!(number("participantType"), 1 | 2 | 6) || number("permissions") & 8 != 0;
    let other_user = (kind == RoomKind::OneToOne).then(|| rich::line(text("name"), &Value::Null));
    Some(Room {
        token,
        kind,
        name,
        unread_messages: entry.get("unreadMessages").and_then(Value::as_u64).unwrap_or(0),
        unread_mention: flag("unreadMention"),
        unread_mention_direct: flag("unreadMentionDirect"),
        notification_level: NotificationLevel::from_talk(number("notificationLevel")),
        last_message: entry.get("lastMessage").and_then(read_last_message),
        last_activity,
        last_read: entry.get("lastReadMessage").and_then(Value::as_u64).filter(|id| *id > 0),
        has_call: flag("hasCall"),
        call_flag: entry.get("callFlag").and_then(Value::as_u64).and_then(|f| u32::try_from(f).ok()).unwrap_or(0),
        favorite: flag("isFavorite"),
        read_only: number("readOnly") == 1,
        lobby: number("lobbyState") == 1 && !passes_lobby,
        other_user: other_user.filter(|id| !id.is_empty()),
        federated: !text("remoteServer").trim().is_empty(),
        description: rich::line(text("description"), &Value::Null),
        can_start_call: entry.get("canStartCall").and_then(Value::as_bool).unwrap_or(true),
    })
}

/// The last message; `None` for PHP's empty `[]` or anything else that is no message.
fn read_last_message(message: &Value) -> Option<LastMessage> {
    message.as_object()?;
    let text = |key: &str| message.get(key).and_then(Value::as_str).unwrap_or_default();
    let author_name = rich::line(text("actorDisplayName"), &Value::Null);
    let actor = rich::line(text("actorId"), &Value::Null);
    let author = (text("actorType") == "users" && !actor.is_empty())
        .then(|| Person { name: if author_name.is_empty() { actor.clone() } else { author_name.clone() }, id: actor });
    let parameters = message.get("messageParameters").unwrap_or(&Value::Null);
    Some(LastMessage {
        id: message.get("id").and_then(Value::as_u64).filter(|id| *id > 0),
        author,
        author_name,
        text: rich::plain(text("message"), parameters),
        system: text("messageType") == "system",
    })
}

/// The messages in the `data` of a read (a JSON list), oldest first and each id once. A message
/// whose shape is broken is left out ([`read_message`]); [`TalkError::Ocs`] when `data` is no list.
pub(crate) fn read_messages(data: &Value) -> Result<Vec<ChatMessage>, TalkError> {
    let entries = data.as_array().ok_or_else(|| OcsError::Other("Nextcloud sent messages that are no list".into()))?;
    let mut seen = HashSet::new();
    let mut messages: Vec<ChatMessage> =
        entries.iter().filter_map(read_message).filter(|m| seen.insert(m.id)).collect();
    messages.sort_by_key(|message| message.id);
    Ok(messages)
}

/// One message; `None` without a positive numeric `id`, a `timestamp`, `actorType`, `actorId`,
/// `message` or `messageType`.
pub(crate) fn read_message(entry: &Value) -> Option<ChatMessage> {
    let text = |key: &str| entry.get(key).and_then(Value::as_str);
    let id = entry.get("id")?.as_u64().filter(|id| *id > 0)?;
    let at = DateTime::from_timestamp(entry.get("timestamp")?.as_i64()?, 0)?;
    let actor_type = ActorType::from_talk(text("actorType")?);
    let actor_id = rich::line(text("actorId")?, &Value::Null);
    let message = text("message")?;
    let message_type = text("messageType")?;
    let parameters = entry.get("messageParameters").unwrap_or(&Value::Null);
    let deleted = entry.get("deleted").and_then(Value::as_bool).unwrap_or(false);
    let kind = match message_type {
        _ if deleted => MessageKind::Deleted,
        "comment" => MessageKind::Comment,
        "system" => {
            let event = text("systemMessage").filter(|event| is_word(event)).unwrap_or_default();
            MessageKind::System { event: event.to_string() }
        }
        "comment_deleted" => MessageKind::Deleted,
        "voice-message" => MessageKind::Voice,
        "command" => MessageKind::Command,
        _ => MessageKind::Other,
    };
    let reference_id = text("referenceId")
        .filter(|id| !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_graphic()))
        .map(str::to_string);
    let edited_at = entry
        .get("lastEditTimestamp")
        .and_then(Value::as_i64)
        .filter(|at| *at > 0)
        .and_then(|at| DateTime::from_timestamp(at, 0));
    Some(ChatMessage {
        id,
        at,
        author_name: author_name(entry, actor_type, &actor_id),
        actor_type,
        actor_id,
        text: rich::plain(message, parameters),
        kind,
        mentions: read_mentions(parameters),
        files: parameters.as_object().map(|map| map.values().filter_map(read_file).collect()).unwrap_or_default(),
        parent: entry.get("parent").and_then(read_parent).map(Box::new),
        reference_id,
        reactions: read_reactions(entry.get("reactions")),
        own_reactions: entry
            .get("reactionsSelf")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).map(|r| rich::line(r, &Value::Null)))
            .map(|list| list.filter(|r| !r.is_empty()).collect())
            .unwrap_or_default(),
        edited_at,
        silent: entry.get("silent").and_then(Value::as_bool).unwrap_or(false),
        markdown: entry.get("markdown").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// The name to show for the author of `entry`: its display name, else the id of an author that
/// has a meaningful one.
fn author_name(entry: &Value, actor_type: ActorType, actor_id: &str) -> String {
    let display = rich::line(entry.get("actorDisplayName").and_then(Value::as_str).unwrap_or_default(), &Value::Null);
    let by_id = matches!(actor_type, ActorType::Users | ActorType::Federated | ActorType::Bots | ActorType::Bridged);
    if display.is_empty() && by_id { actor_id.to_string() } else { display }
}

/// The mentions among `parameters`: the keys `mention-…` of a type that names someone.
fn read_mentions(parameters: &Value) -> Vec<Mention> {
    let Some(parameters) = parameters.as_object() else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    parameters
        .iter()
        .filter(|(key, _)| key.starts_with("mention-"))
        .filter_map(|(_, parameter)| {
            let field = |key: &str| parameter.get(key).and_then(Value::as_str);
            let kind = match field("type")? {
                "user" if field("server").is_some_and(|server| !server.is_empty()) => MentionKind::Federated,
                "user" => MentionKind::User,
                "guest" => MentionKind::Guest,
                "user-group" => MentionKind::Group,
                "call" => MentionKind::All,
                _ => return None,
            };
            let id = rich::line(field("id")?, &Value::Null);
            let name = rich::line(field("name").unwrap_or_default(), &Value::Null);
            (!id.is_empty()).then(|| Mention { name: if name.is_empty() { id.clone() } else { name }, id, kind })
        })
        .filter(|mention| seen.insert((mention.kind, mention.id.clone())))
        .collect()
}

/// A parameter of type `file`; `None` for any other or one without a numeric `id`.
fn read_file(parameter: &Value) -> Option<SharedFile> {
    let field = |key: &str| parameter.get(key).and_then(Value::as_str);
    let yes = |key: &str| field(key) == Some("yes") || parameter.get(key).and_then(Value::as_bool) == Some(true);
    let pixels = |key: &str| parameter.get(key).and_then(number).and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0);
    if field("type")? != "file" {
        return None;
    }
    let id = number(parameter.get("id")?).filter(|id| *id > 0)?;
    let mimetype = field("mimetype").map(str::trim).filter(|m| is_mimetype(m)).unwrap_or("application/octet-stream");
    Some(SharedFile {
        id,
        name: rich::line(field("name").unwrap_or_default(), &Value::Null),
        size: parameter.get("size").and_then(number).unwrap_or(0),
        mimetype: mimetype.to_ascii_lowercase(),
        preview_available: yes("preview-available"),
        hide_download: yes("hide-download"),
        width: pixels("width"),
        height: pixels("height"),
        blurhash: field("blurhash")
            .filter(|hash| (6..=128).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_graphic()))
            .map(str::to_string),
        path: field("path").map(|path| rich::line(path, &Value::Null)).filter(|path| !path.is_empty()),
    })
}

/// The message a message answers; `None` for anything that has no positive `id`.
fn read_parent(parent: &Value) -> Option<Parent> {
    let text = |key: &str| parent.get(key).and_then(Value::as_str);
    let id = parent.get("id")?.as_u64().filter(|id| *id > 0)?;
    let deleted = parent.get("deleted").and_then(Value::as_bool).unwrap_or(false)
        || text("messageType") == Some("comment_deleted");
    let actor_type = ActorType::from_talk(text("actorType").unwrap_or_default());
    let actor_id = rich::line(text("actorId").unwrap_or_default(), &Value::Null);
    let parameters = parent.get("messageParameters").unwrap_or(&Value::Null);
    Some(Parent {
        id,
        author_name: author_name(parent, actor_type, &actor_id),
        text: rich::plain(text("message").unwrap_or_default(), parameters),
        deleted,
    })
}

/// The reactions map (emoji → count); PHP's empty `[]` and counts of 0 are none.
fn read_reactions(reactions: Option<&Value>) -> BTreeMap<String, u64> {
    let Some(reactions) = reactions.and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    reactions
        .iter()
        .filter_map(|(emoji, count)| Some((rich::line(emoji, &Value::Null), count.as_u64().filter(|n| *n > 0)?)))
        .filter(|(emoji, _)| !emoji.is_empty())
        .collect()
}

fn read_suggestion(entry: &Value) -> Option<MentionSuggestion> {
    let field = |key: &str| rich::line(entry.get(key).and_then(Value::as_str).unwrap_or_default(), &Value::Null);
    let id = field("id");
    if id.is_empty() {
        return None;
    }
    let mention_id = Some(field("mentionId")).filter(|m| !m.is_empty()).unwrap_or_else(|| id.clone());
    let label = Some(field("label")).filter(|l| !l.is_empty()).unwrap_or_else(|| id.clone());
    let source = Some(field("source")).filter(|s| is_word(s)).unwrap_or_default();
    Some(MentionSuggestion { id, mention_id, label, source })
}

fn read_found(entry: &Value) -> Option<FoundMessage> {
    let attributes = entry.get("attributes")?;
    let token = RoomToken::parse(attributes.get("conversation")?.as_str()?)?;
    let message_id = number(attributes.get("messageId")?).filter(|id| *id > 0)?;
    let field = |key: &str| rich::line(entry.get(key).and_then(Value::as_str).unwrap_or_default(), &Value::Null);
    let at = attributes
        .get("timestamp")
        .and_then(number)
        .and_then(|at| i64::try_from(at).ok())
        .and_then(|at| DateTime::from_timestamp(at, 0));
    Some(FoundMessage { token, message_id, title: field("title"), text: field("subline"), at })
}

/// A number sent as a JSON number or as a string of digits (rich object parameters are strings).
fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let digits = value.as_str()?.trim();
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok()).flatten()
    })
}

/// Whether `mimetype` looks like `type/subtype` of ASCII letters, digits and `+.-_`.
fn is_mimetype(mimetype: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-' | b'_'))
    };
    mimetype.len() <= 128 && mimetype.split_once('/').is_some_and(|(kind, sub)| part(kind) && part(sub))
}

#[cfg(test)]
mod tests {
    use httpmock::Method::{DELETE, GET, POST, PUT};
    use httpmock::MockServer;

    use super::*;
    use crate::ocs::envelope;
    use crate::ocs::tests::{AUTHORIZATION, account};

    const FIXTURE: &str = include_str!("../tests/fixtures/talk_rooms.json");
    const MESSAGES: &str = include_str!("../tests/fixtures/talk_messages.json");
    const TOKEN: &str = "x7k2m9qa";

    fn fixture() -> Vec<Room> {
        read_rooms(&envelope(serde_json::from_str(FIXTURE).expect("fixture is JSON")).expect("an envelope"))
            .expect("a list")
    }

    fn room<'a>(rooms: &'a [Room], token: &str) -> &'a Room {
        rooms.iter().find(|room| room.token.as_str() == token).expect("the room")
    }

    fn messages() -> Vec<ChatMessage> {
        read_messages(&envelope(serde_json::from_str(MESSAGES).expect("fixture is JSON")).expect("an envelope"))
            .expect("a list")
    }

    fn message(messages: &[ChatMessage], id: u64) -> &ChatMessage {
        messages.iter().find(|message| message.id == id).expect("the message")
    }

    fn token() -> RoomToken {
        RoomToken::parse(TOKEN).expect("a token")
    }

    /// An OCS answer around `data`.
    fn ocs(data: Value) -> String {
        json!({"ocs": {"meta": {"status": "ok", "statuscode": 200, "message": "OK"}, "data": data}}).to_string()
    }

    /// The message `id` of the fixture.
    fn entry(id: u64) -> Value {
        let data = envelope(serde_json::from_str(MESSAGES).expect("fixture is JSON")).expect("an envelope");
        let entry = data.as_array().and_then(|list| list.iter().find(|m| m["id"] == json!(id)).cloned());
        entry.expect("in the fixture")
    }

    /// The message `id` of the fixture as the `data` of an answer.
    fn one(id: u64) -> String {
        ocs(entry(id))
    }

    /// The message `id` of the fixture as a list in the `data` of an answer.
    fn listed(id: u64) -> String {
        ocs(json!([entry(id)]))
    }

    #[test]
    fn reads_a_one_to_one_conversation_with_its_last_message() {
        let rooms = fixture();
        let direct = room(&rooms, "a1b2c3d4");
        assert_eq!(direct.kind, RoomKind::OneToOne);
        assert_eq!(direct.name, "Bert Example");
        assert_eq!(direct.unread_messages, 2);
        assert!(!direct.unread_mention);
        assert_eq!(direct.notification_level, NotificationLevel::Default);
        assert_eq!(direct.last_activity.timestamp(), 1_791_880_500);
        assert_eq!(direct.last_read, Some(5120));
        let last = direct.last_message.as_ref().expect("a last message");
        assert_eq!(last.id, Some(5122));
        assert_eq!(last.author, Some(Person { id: "bert".into(), name: "Bert Example".into() }));
        assert_eq!(last.text, "Could you look at Plan.pdf?");
        assert!(!last.system);
    }

    #[test]
    fn a_one_to_one_conversation_names_the_other_user_and_its_call() {
        let rooms = fixture();
        let direct = room(&rooms, "a1b2c3d4");
        assert_eq!(direct.other_user.as_deref(), Some("bert"));
        assert!(direct.has_call && direct.favorite && direct.can_start_call);
        assert_eq!(direct.call_flag, 7);
        assert!(!direct.read_only && !direct.lobby && !direct.federated);
        assert_eq!(direct.description, "");
        assert_eq!(room(&rooms, "x7k2m9qa").other_user, None, "only one-to-one conversations name a user");
    }

    #[test]
    fn read_only_lobby_and_federation() {
        let rooms = fixture();
        let group = room(&rooms, "x7k2m9qa");
        assert!(!group.lobby, "a moderator passes the lobby");
        assert_eq!(group.description, "Plans and more");
        assert!(!group.has_call && group.call_flag == 0 && group.can_start_call);
        let public = room(&rooms, "pub1ic00");
        assert!(public.read_only && public.lobby && !public.can_start_call);
        let federated = room(&rooms, "fed3rat3d");
        assert!(federated.federated && !federated.favorite);
    }

    #[test]
    fn reads_a_group_with_a_direct_mention() {
        let rooms = fixture();
        let group = room(&rooms, "x7k2m9qa");
        assert_eq!(group.kind, RoomKind::Group);
        assert_eq!(group.name, "Team Alpha");
        assert_eq!((group.unread_messages, group.unread_mention, group.unread_mention_direct), (5, true, true));
        assert_eq!(group.notification_level, NotificationLevel::Mention);
        let last = group.last_message.as_ref().expect("a last message");
        assert_eq!(last.text, "Hi Anna Example, can you check the plan?");
        assert_eq!(last.author_name, "Carla Example");
    }

    #[test]
    fn system_messages_guests_and_empty_last_messages() {
        let rooms = fixture();
        let public = room(&rooms, "pub1ic00");
        assert_eq!(public.kind, RoomKind::Public);
        assert_eq!(public.notification_level, NotificationLevel::Never);
        let last = public.last_message.as_ref().expect("a last message");
        assert!(last.system);
        assert_eq!(last.text, "Guest Dora started a call");
        assert_eq!(last.author, None, "a guest is no user of the server");
        assert_eq!(last.author_name, "Guest Dora");
        let notes = room(&rooms, "n0tes2me");
        assert_eq!(notes.kind, RoomKind::NoteToSelf);
        assert_eq!(notes.last_message, None, "PHP's empty [] is no message");
        assert_eq!(notes.name, "Note to self");
    }

    #[test]
    fn a_federated_message_has_no_id_and_bad_rooms_are_left_out() {
        let rooms = fixture();
        let federated = room(&rooms, "fed3rat3d");
        assert_eq!(federated.last_message.as_ref().and_then(|m| m.id), None);
        assert!(!federated.unread_mention_direct, "an old server without the flag");
        let tokens: Vec<&str> = rooms.iter().map(|room| room.token.as_str()).collect();
        assert_eq!(tokens, vec!["a1b2c3d4", "x7k2m9qa", "pub1ic00", "n0tes2me", "fed3rat3d"]);
    }

    #[test]
    fn tokens_have_talks_pattern() {
        assert!(RoomToken::parse("abcd").is_some());
        assert!(RoomToken::parse("a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5").is_some());
        assert!(RoomToken::parse("abc").is_none());
        assert!(RoomToken::parse("a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p").is_none());
        assert!(RoomToken::parse("ABCD").is_none());
        assert!(RoomToken::parse("ab/../cd").is_none());
        assert!(RoomToken::parse("abcd?x=1").is_none());
    }

    #[test]
    fn notification_levels_go_both_ways() {
        for level in [
            NotificationLevel::Default,
            NotificationLevel::Always,
            NotificationLevel::Mention,
            NotificationLevel::Never,
        ] {
            let number = level.to_talk().expect("a number");
            assert_eq!(NotificationLevel::from_talk(number), level);
        }
        assert_eq!(NotificationLevel::Other.to_talk(), None);
    }

    #[test]
    fn messages_come_oldest_first_each_once_and_broken_ones_are_left_out() {
        let ids: Vec<u64> = messages().iter().map(|message| message.id).collect();
        assert_eq!(ids, vec![5131, 5132, 5133, 5134, 5135, 5136, 5137, 5138, 5139, 5140]);
        assert!(read_messages(&json!({"no": "list"})).is_err());
    }

    #[test]
    fn a_message_with_mentions_reactions_and_its_reference() {
        let messages = messages();
        let hello = message(&messages, 5131);
        assert_eq!(hello.kind, MessageKind::Comment);
        assert_eq!(
            (hello.actor_type, hello.actor_id.as_str(), hello.author_name.as_str()),
            (ActorType::Users, "carla", "Carla Example")
        );
        assert_eq!(hello.at.timestamp(), 1_791_880_000);
        assert_eq!(hello.text, "Hi Anna Example, can you check the plan? Team Alpha Sales Erik Example Dora");
        let mention = |id: &str, name: &str, kind| Mention { id: id.into(), name: name.into(), kind };
        let mut mentions = hello.mentions.clone();
        mentions.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(
            mentions,
            vec![
                mention("anna", "Anna Example", MentionKind::User),
                mention("erik", "Erik Example", MentionKind::Federated),
                mention("guest/4a6f8b2c", "Dora", MentionKind::Guest),
                mention("sales", "Sales", MentionKind::Group),
                mention("x7k2m9qa", "Team Alpha", MentionKind::All),
            ]
        );
        assert!(hello.files.is_empty());
        let reactions: Vec<(&str, u64)> = hello.reactions.iter().map(|(e, n)| (e.as_str(), *n)).collect();
        assert_eq!(reactions, vec![("🎉", 1), ("👍", 2)], "empty emojis and counts of 0 are left out");
        assert_eq!(hello.own_reactions, vec!["👍".to_string()]);
        assert_eq!(
            hello.reference_id.as_deref(),
            Some("3f2a9c1b7e5d4a6f8b0c2d4e6f8a0b1c3d5e7f9a1b3c5d7e9f0a2b4c6d8e0f1a")
        );
        assert!(hello.markdown && !hello.silent && hello.edited_at.is_none() && hello.parent.is_none());
    }

    #[test]
    fn a_reply_names_its_parent_and_was_edited_silently() {
        let messages = messages();
        let reply = message(&messages, 5132);
        let parent = reply.parent.as_deref().expect("a parent");
        assert_eq!(
            parent,
            &Parent {
                id: 5131,
                author_name: "Carla Example".into(),
                text: "Hi Anna Example, can you check the plan?".into(),
                deleted: false,
            }
        );
        assert!(reply.silent);
        assert_eq!(reply.edited_at.map(|at| at.timestamp()), Some(1_791_880_900));
    }

    #[test]
    fn system_messages_carry_their_event_and_the_changed_message() {
        let messages = messages();
        let call = message(&messages, 5133);
        assert_eq!(call.kind, MessageKind::System { event: "call_started".into() });
        assert_eq!(call.text, "Guest started a call");
        assert_eq!((call.actor_type, call.author_name.as_str()), (ActorType::Guests, ""), "a guest has no name");
        assert!(call.mentions.is_empty(), "the actor is no mention");
        let deleted = message(&messages, 5135);
        assert_eq!(deleted.kind, MessageKind::System { event: "message_deleted".into() });
        let parent = deleted.parent.as_deref().expect("the deleted message");
        assert!(parent.deleted && parent.id == 5134);
        let edited = message(&messages, 5140);
        assert_eq!(edited.kind, MessageKind::System { event: "message_edited".into() });
        let parent = edited.parent.as_deref().expect("the edited message");
        assert_eq!((parent.id, parent.text.as_str(), parent.deleted), (5132, "Sure, after lunch (edited)", false));
    }

    #[test]
    fn deleted_voice_command_and_federated_messages() {
        let messages = messages();
        assert_eq!(message(&messages, 5134).kind, MessageKind::Deleted);
        let voice = message(&messages, 5137);
        assert_eq!(voice.kind, MessageKind::Voice);
        assert_eq!(voice.files.len(), 1);
        let file = &voice.files[0];
        assert_eq!((file.id, file.size, file.mimetype.as_str()), (4712, 48213, "audio/mp4"));
        assert!(!file.preview_available && file.hide_download);
        assert_eq!((file.width, file.height, file.blurhash.as_deref()), (None, None, None));
        let command = message(&messages, 5139);
        assert_eq!(command.kind, MessageKind::Command);
        assert_eq!((command.actor_type, command.author_name.as_str()), (ActorType::Bots, "bot-1a2b3c"));
        let federated = message(&messages, 5138);
        assert_eq!(federated.actor_type, ActorType::Federated);
        assert_eq!(federated.author_name, "Erik Example Partner", "one line without direction marks");
        assert_eq!(federated.text, "See you tomorrow\nbye");
        assert_eq!(federated.reference_id, None, "a reference with control characters is dropped");
    }

    #[test]
    fn a_shared_image_has_its_size_and_blurhash() {
        let messages = messages();
        let image = message(&messages, 5136);
        assert_eq!(image.text, "Holiday.jpg");
        assert_eq!(
            image.files,
            vec![SharedFile {
                id: 4711,
                name: "Holiday.jpg".into(),
                size: 2_048_576,
                mimetype: "image/jpeg".into(),
                preview_available: true,
                hide_download: false,
                width: Some(1920),
                height: Some(1080),
                blurhash: Some("LEHV6nWB2yk8pyo0adR*.7kCMdnj".into()),
                path: Some("Talk/Holiday.jpg".into()),
            }]
        );
    }

    #[test]
    fn files_need_a_numeric_id_and_get_a_safe_media_type() {
        let file = |parameter: Value| read_file(&parameter);
        assert_eq!(file(json!({"type": "file", "id": "12a", "name": "x"})), None);
        assert_eq!(file(json!({"type": "deck-card", "id": "1", "name": "x"})), None);
        let odd =
            file(json!({"type": "file", "id": 3, "name": "x", "mimetype": "text/html\"><script>"})).expect("a file");
        assert_eq!((odd.mimetype.as_str(), odd.size, odd.path), ("application/octet-stream", 0, None));
    }

    #[test]
    fn suggestions_and_search_results() {
        let suggestion = read_suggestion(&json!({"id": "anna", "label": "Anna Example", "source": "users"}));
        assert_eq!(
            suggestion,
            Some(MentionSuggestion {
                id: "anna".into(),
                mention_id: "anna".into(),
                label: "Anna Example".into(),
                source: "users".into(),
            })
        );
        let guest = read_suggestion(&json!({"id": "guest/1", "mentionId": "guest/1", "label": "", "source": "a b"}))
            .expect("a suggestion");
        assert_eq!((guest.label.as_str(), guest.source.as_str()), ("guest/1", ""));
        assert_eq!(read_suggestion(&json!({"label": "No id"})), None);
        let found = read_found(&json!({
            "title": "Carla Example in Team Alpha", "subline": "…check the\nplan?",
            "attributes": {"conversation": "x7k2m9qa", "messageId": "5131", "timestamp": "1791880000"},
        }))
        .expect("a hit");
        assert_eq!(
            (found.token.as_str(), found.message_id, found.text.as_str()),
            ("x7k2m9qa", 5131, "…check the plan?")
        );
        assert_eq!(found.at.map(|at| at.timestamp()), Some(1_791_880_000));
        assert_eq!(read_found(&json!({"attributes": {"conversation": "../x", "messageId": 1}})), None);
    }

    #[tokio::test]
    async fn lists_rooms_without_touching_the_status() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/ocs/v2.php/apps/spreed/api/v4/room")
                    .query_param("noStatusUpdate", "1")
                    .header("authorization", AUTHORIZATION)
                    .header("ocs-apirequest", "true")
                    .header("accept", "application/json");
                then.status(200).body(FIXTURE);
            })
            .await;
        assert_eq!(account(&server).rooms().await.expect("rooms").len(), 5);
    }

    #[tokio::test]
    async fn without_talk_the_app_is_missing() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/ocs/v2.php/apps/spreed/api/v4/room");
                then.status(404).body(r#"{"ocs":{"meta":{"status":"failure","statuscode":998},"data":[]}}"#);
            })
            .await;
        assert_eq!(account(&server).rooms().await, Err(OcsError::MissingApp));
    }

    #[tokio::test]
    async fn marks_read_up_to_a_message() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/ocs/v2.php/apps/spreed/api/v1/chat/x7k2m9qa/read")
                    .header("authorization", AUTHORIZATION)
                    .header("ocs-apirequest", "true")
                    .json_body(json!({"lastReadMessage": 5131}));
                then.status(200).header("X-Chat-Last-Common-Read", "5120").body(r#"{"ocs":{"meta":{},"data":{}}}"#);
            })
            .await;
        let token = RoomToken::parse("x7k2m9qa").expect("a token");
        assert_eq!(account(&server).mark_read(&token, 5131).await, Ok(()));
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn marking_read_when_refused_is_an_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/ocs/v2.php/apps/spreed/api/v1/chat/x7k2m9qa/read");
                then.status(401);
            })
            .await;
        let token = RoomToken::parse("x7k2m9qa").expect("a token");
        assert_eq!(account(&server).mark_read(&token, 1).await, Err(OcsError::Refused));
    }

    #[tokio::test]
    async fn marks_unread() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{CHAT}/{TOKEN}/read")).header("authorization", AUTHORIZATION);
                then.status(200).body(ocs(json!([])));
            })
            .await;
        assert_eq!(account(&server).mark_unread(&token()).await, Ok(()));
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn reads_history_without_changing_anything() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!("{CHAT}/{TOKEN}"))
                    .header("authorization", AUTHORIZATION)
                    .header("ocs-apirequest", "true")
                    .query_param("lookIntoFuture", "0")
                    .query_param("limit", "200")
                    .query_param("lastKnownMessageId", "5141")
                    .query_param("setReadMarker", "0")
                    .query_param("noStatusUpdate", "1")
                    .query_param("markNotificationsAsRead", "0")
                    .query_param_missing("timeout");
                then.status(200)
                    .header("X-Chat-Last-Given", "5131")
                    .header("X-Chat-Last-Common-Read", "5130")
                    .body(MESSAGES);
            })
            .await;
        let read = account(&server).messages(&token(), Some(5141), 500, true).await.expect("messages");
        assert_eq!(read.messages.len(), 10);
        assert_eq!(read.messages.first().map(|m| m.id), Some(5131));
        assert_eq!((read.last_given, read.last_common_read), (Some(5131), Some(5130)));
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn without_keep_notifications_the_flag_is_not_sent_and_no_more_is_empty() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!("{CHAT}/{TOKEN}"))
                    .query_param("limit", "1")
                    .query_param("setReadMarker", "0")
                    .query_param("noStatusUpdate", "1")
                    .query_param_missing("markNotificationsAsRead")
                    .query_param_missing("lastKnownMessageId");
                then.status(304);
            })
            .await;
        let read = account(&server).messages(&token(), None, 0, false).await;
        assert_eq!(read, Ok(Messages::default()));
    }

    #[tokio::test]
    async fn waits_for_new_messages_at_most_thirty_seconds() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!("{CHAT}/{TOKEN}"))
                    .query_param("lookIntoFuture", "1")
                    .query_param("lastKnownMessageId", "5140")
                    .query_param("timeout", "30")
                    .query_param("limit", "50")
                    .query_param("setReadMarker", "0")
                    .query_param("noStatusUpdate", "1");
                then.status(200).header("X-Chat-Last-Given", "5136").body(listed(5136));
            })
            .await;
        let read = account(&server).wait_messages(&token(), 5140, 60, 50, false).await.expect("messages");
        assert_eq!(read.messages.iter().map(|m| m.id).collect::<Vec<_>>(), vec![5136]);
        assert_eq!((read.last_given, read.last_common_read), (Some(5136), None));
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn an_answer_that_is_no_list_is_an_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{CHAT}/{TOKEN}"));
                then.status(200).body(one(5136));
            })
            .await;
        let read = account(&server).messages(&token(), None, 10, false).await;
        assert!(matches!(read, Err(TalkError::Ocs(OcsError::Other(_)))), "{read:?}");
    }

    #[tokio::test]
    async fn waiting_without_news_is_empty() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!("{CHAT}/{TOKEN}"))
                    .query_param("lookIntoFuture", "1")
                    .query_param("timeout", "1");
                then.status(304);
            })
            .await;
        assert_eq!(account(&server).wait_messages(&token(), 1, 1, 10, false).await, Ok(Messages::default()));
    }

    #[tokio::test]
    async fn reads_around_a_message() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!("{CHAT}/{TOKEN}/5135/context"))
                    .query_param("limit", "100")
                    .header("authorization", AUTHORIZATION);
                then.status(200).header("X-Chat-Last-Given", "5131").body(MESSAGES);
            })
            .await;
        let read = account(&server).messages_around(&token(), 5135, 1000).await.expect("messages");
        assert_eq!((read.messages.len(), read.last_given), (10, Some(5131)));
    }

    #[tokio::test]
    async fn sends_a_reply_silently() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(format!("{CHAT}/{TOKEN}"))
                    .header("authorization", AUTHORIZATION)
                    .header("ocs-apirequest", "true")
                    .json_body(json!({"message": "Sure, after lunch", "replyTo": 5131, "referenceId": "abc123", "silent": true}));
                then.status(201).body(one(5132));
            })
            .await;
        let options = SendOptions { reply_to: Some(5131), reference_id: Some("abc123".into()), silent: true };
        let sent = account(&server).send_message(&token(), "Sure, after lunch", &options).await.expect("sent");
        assert_eq!(sent.id, 5132);
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn sends_a_plain_message_and_refuses_an_empty_one() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path(format!("{CHAT}/{TOKEN}")).json_body(json!({"message": "Hello"}));
                then.status(201).body(one(5138));
            })
            .await;
        let account = account(&server);
        let sent = account.send_message(&token(), "Hello", &SendOptions::default()).await.expect("sent");
        assert_eq!(sent.id, 5138);
        assert_eq!(account.send_message(&token(), "  \n", &SendOptions::default()).await, Err(TalkError::Invalid));
    }

    /// What `send_message` gives for a refusal with `status` and `body`.
    async fn sending_refused(status: u16, body: &str) -> Result<ChatMessage, TalkError> {
        let server = MockServer::start_async().await;
        let body = body.to_string();
        server
            .mock_async(|when, then| {
                when.method(POST).path(format!("{CHAT}/{TOKEN}"));
                then.status(status).body(body);
            })
            .await;
        account(&server).send_message(&token(), "Hello", &SendOptions::default()).await
    }

    #[tokio::test]
    async fn refusals_become_talk_errors() {
        let error = r#"{"ocs":{"meta":{"status":"failure","message":"Server words"},"data":{"error":"message"}}}"#;
        assert_eq!(sending_refused(404, error).await, Err(TalkError::NotFound));
        assert_eq!(sending_refused(403, "").await, Err(TalkError::ReadOnly));
        assert_eq!(sending_refused(412, error).await, Err(TalkError::Lobby));
        assert_eq!(sending_refused(413, error).await, Err(TalkError::TooLong));
        assert_eq!(sending_refused(400, error).await, Err(TalkError::Invalid));
        assert_eq!(sending_refused(401, "").await, Err(TalkError::Ocs(OcsError::Refused)));
        assert_eq!(sending_refused(503, "").await, Err(TalkError::Ocs(OcsError::Maintenance)));
        let throttled = sending_refused(429, "").await;
        assert!(matches!(throttled, Err(TalkError::Ocs(OcsError::Throttled { .. }))), "{throttled:?}");
        let other = sending_refused(502, error).await.expect_err("an error");
        assert!(!format!("{other} {other:?}").contains("Server words"), "the server's words stay out");
    }

    #[tokio::test]
    async fn edits_and_tells_too_late_and_someone_elses_apart() {
        let server = MockServer::start_async().await;
        let path = |id: u64| format!("{CHAT}/{TOKEN}/{id}");
        server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(path(5132))
                    .header("authorization", AUTHORIZATION)
                    .json_body(json!({"message": "Sure, after lunch (edited)"}));
                then.status(200).body(one(5140));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(PUT).path(path(5100));
                then.status(400).body(ocs(json!({"error": "age"})));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(PUT).path(path(5101));
                then.status(403).body(ocs(json!({"error": "permission"})));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(PUT).path(path(5102));
                then.status(400).body(ocs(json!({"error": "message"})));
            })
            .await;
        let (account, token) = (account(&server), token());
        let edit = |id| account.edit_message(&token, id, "Sure, after lunch (edited)");
        assert_eq!(edit(5132).await, Ok(()));
        assert_eq!(edit(5100).await, Err(TalkError::TooLate));
        assert_eq!(edit(5101).await, Err(TalkError::Ocs(OcsError::Forbidden)));
        assert_eq!(edit(5102).await, Err(TalkError::Invalid));
    }

    #[tokio::test]
    async fn deletes_and_a_400_is_too_late() {
        let server = MockServer::start_async().await;
        let ok = server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{CHAT}/{TOKEN}/5134")).header("authorization", AUTHORIZATION);
                then.status(202).body(one(5135));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{CHAT}/{TOKEN}/5000"));
                then.status(400).body(ocs(json!({"error": "message"})));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{CHAT}/{TOKEN}/5001"));
                then.status(405).body(ocs(json!({"error": "message"})));
            })
            .await;
        let account = account(&server);
        assert_eq!(account.delete_message(&token(), 5134).await, Ok(()));
        assert_eq!(account.delete_message(&token(), 5000).await, Err(TalkError::TooLate));
        assert_eq!(account.delete_message(&token(), 5001).await, Err(TalkError::Invalid));
        assert_eq!(ok.calls_async().await, 1);
    }

    #[tokio::test]
    async fn reacts_in_the_body_and_takes_it_back_in_the_query() {
        let server = MockServer::start_async().await;
        let add = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(format!("{REACTION}/{TOKEN}/5131"))
                    .header("authorization", AUTHORIZATION)
                    .json_body(json!({"reaction": "👍"}));
                then.status(201).body(ocs(json!({"👍": []})));
            })
            .await;
        let remove = server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{REACTION}/{TOKEN}/5131")).query_param("reaction", "👍");
                then.status(200).body(ocs(json!([])));
            })
            .await;
        let account = account(&server);
        assert_eq!(account.react(&token(), 5131, "👍").await, Ok(()));
        assert_eq!(account.unreact(&token(), 5131, "👍").await, Ok(()));
        assert_eq!((add.calls_async().await, remove.calls_async().await), (1, 1));
        assert_eq!(account.react(&token(), 5131, "").await, Err(TalkError::Invalid));
        assert_eq!(account.react(&token(), 5131, "a b").await, Err(TalkError::Invalid));
    }

    #[tokio::test]
    async fn suggests_mentions() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!("{CHAT}/{TOKEN}/mentions"))
                    .query_param("search", "an na")
                    .query_param("limit", "20")
                    .header("authorization", AUTHORIZATION);
                then.status(200).body(ocs(json!([
                    {"id": "anna", "label": "Anna Example", "source": "users", "mentionId": "anna"},
                    {"id": "all", "label": "Team Alpha", "source": "calls", "mentionId": "all"},
                    {"label": "broken"},
                ])));
            })
            .await;
        let found = account(&server).mention_suggestions(&token(), "an na", 20).await.expect("suggestions");
        let ids: Vec<&str> = found.iter().map(|s| s.mention_id.as_str()).collect();
        assert_eq!(ids, vec!["anna", "all"]);
    }

    #[tokio::test]
    async fn opens_a_one_to_one_conversation() {
        let server = MockServer::start_async().await;
        let entry = envelope(serde_json::from_str(FIXTURE).expect("JSON")).expect("an envelope")[0].clone();
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(ROOM)
                    .header("authorization", AUTHORIZATION)
                    .json_body(json!({"roomType": 1, "invite": "bert"}));
                then.status(200).body(ocs(entry));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(POST).path(ROOM).json_body(json!({"roomType": 1, "invite": "nobody"}));
                then.status(404).body(ocs(json!({"error": "invite"})));
            })
            .await;
        let account = account(&server);
        let direct = account.create_one_to_one("bert").await.expect("a conversation");
        assert_eq!((direct.token.as_str(), direct.other_user.as_deref()), ("a1b2c3d4", Some("bert")));
        assert_eq!(account.create_one_to_one("nobody").await, Err(TalkError::NotFound));
        assert_eq!(account.create_one_to_one(" ").await, Err(TalkError::Invalid));
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn reads_one_room_and_a_gone_one_is_not_found() {
        let server = MockServer::start_async().await;
        let entry = envelope(serde_json::from_str(FIXTURE).expect("JSON")).expect("an envelope")[1].clone();
        server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{ROOM}/{TOKEN}")).header("authorization", AUTHORIZATION);
                then.status(200).body(ocs(entry));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{ROOM}/g0ne"));
                then.status(404).body(ocs(json!([])));
            })
            .await;
        let account = account(&server);
        assert_eq!(account.room(&token()).await.map(|room| room.name), Ok("Team Alpha".into()));
        let gone = RoomToken::parse("g0ne").expect("a token");
        assert_eq!(account.room(&gone).await, Err(TalkError::NotFound));
    }

    #[tokio::test]
    async fn favourites_and_notification_levels() {
        let server = MockServer::start_async().await;
        let on = server
            .mock_async(|when, then| {
                when.method(POST).path(format!("{ROOM}/{TOKEN}/favorite")).header("authorization", AUTHORIZATION);
                then.status(200).body(ocs(json!([])));
            })
            .await;
        let off = server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{ROOM}/{TOKEN}/favorite"));
                then.status(200).body(ocs(json!([])));
            })
            .await;
        let level = server
            .mock_async(|when, then| {
                when.method(POST).path(format!("{ROOM}/{TOKEN}/notify")).json_body(json!({"level": 2}));
                then.status(200).body(ocs(json!([])));
            })
            .await;
        let account = account(&server);
        assert_eq!(account.set_favorite(&token(), true).await, Ok(()));
        assert_eq!(account.set_favorite(&token(), false).await, Ok(()));
        assert_eq!(account.set_notification_level(&token(), NotificationLevel::Mention).await, Ok(()));
        assert_eq!(account.set_notification_level(&token(), NotificationLevel::Other).await, Err(TalkError::Invalid));
        assert_eq!((on.calls_async().await, off.calls_async().await, level.calls_async().await), (1, 1, 1));
    }

    #[tokio::test]
    async fn searches_messages_through_the_unified_search() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(SEARCH)
                    .query_param("term", "plan")
                    .query_param("limit", "5")
                    .header("authorization", AUTHORIZATION);
                then.status(200).body(ocs(json!({
                    "name": "Messages", "isPaginated": true, "cursor": 5,
                    "entries": [
                        {"title": "Carla Example in Team Alpha", "subline": "can you check the plan?", "thumbnailUrl": "",
                         "resourceUrl": "https://evil.example.com/", "icon": "icon-talk", "rounded": true,
                         "attributes": {"conversation": "x7k2m9qa", "messageId": "5131", "actorType": "users"}},
                        {"title": "No attributes", "subline": ""},
                    ],
                })));
            })
            .await;
        let found = account(&server).search_messages("plan", 5).await.expect("hits");
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].message_id, found[0].title.as_str()), (5131, "Carla Example in Team Alpha"));
    }

    #[tokio::test]
    async fn without_a_message_search_the_app_is_missing() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(SEARCH);
                then.status(404).body(ocs(json!([])));
            })
            .await;
        assert_eq!(account(&server).search_messages("plan", 5).await, Err(TalkError::Ocs(OcsError::MissingApp)));
    }
}
