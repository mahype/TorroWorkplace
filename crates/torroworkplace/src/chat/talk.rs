//! Nextcloud Talk and Nextcloud's user status behind [`super::remote::Remote`]: every call the
//! chat and presence services make, in the terms of `torro-nextcloud`, and its answers and
//! refusals in theirs.
//!
//! - Features are told by capabilities, never by version numbers ([`caps`]).
//! - A token is checked against Talk's pattern before it names anything in an address; one that
//!   does not fit is `not-found`.
//! - A refusal becomes one of the fixed codes of `docs/chat.md` ("Fehler"); the text is this
//!   file's own, never the server's.
//! - System messages that only change another message are marked hidden and carry the change
//!   ([`shown`]); the cache applies it.

use chrono::{DateTime, Utc};
use torro_chat::{ChatError, ConversationKind, FileRef, Mention, MessageKind, NotifyLevel, Preview, Status};
use torro_core::i18n::tr;
use torro_nextcloud::{
    ActorType, ChatMessage, MentionKind, Nextcloud, NotificationLevel, OcsError, Room, RoomKind, RoomToken,
    SendOptions, StatusKind, TalkError, UserStatus,
};

use super::remote::{
    Caps, Patch, RemoteHit, RemoteMessage, RemotePerson, RemoteQuote, RemoteRoom, RemoteStatus, Sending,
};

/// The most statuses asked for in one page, and the most pages: 2,000 users with a status.
const STATUS_PAGE: u32 = 200;
const STATUS_PAGES: u32 = 10;

/// The events of system messages that only change another message and are not shown themselves.
const CHANGES: [&str; 5] = ["message_deleted", "message_edited", "reaction", "reaction_revoked", "reaction_deleted"];

fn unsupported(text: &'static str) -> ChatError {
    ChatError::Unsupported(tr(text).to_string())
}

/// The error of the area for a refusal of any OCS call.
fn ocs(err: OcsError) -> ChatError {
    match err {
        OcsError::Refused => ChatError::Auth,
        OcsError::Forbidden => unsupported("The server does not allow this."),
        // Maintenance passes; for the caller it is the same as no connection: try again later.
        OcsError::Maintenance | OcsError::Offline => ChatError::Offline,
        OcsError::Throttled { .. } => ChatError::Busy,
        OcsError::MissingApp => ChatError::NotConnected,
        OcsError::Other(_) => ChatError::Other(tr("The server answered in a way that cannot be read.").to_string()),
    }
}

/// The error of the area for a refusal of a Talk call.
fn talk(err: TalkError) -> ChatError {
    match err {
        TalkError::Ocs(err) => ocs(err),
        TalkError::NotFound => ChatError::NotFound,
        TalkError::ReadOnly => unsupported("This conversation is read-only."),
        TalkError::Lobby => unsupported("This conversation is waiting in the lobby."),
        TalkError::TooLong => ChatError::Other(tr("The message is too long for the server.").to_string()),
        TalkError::TooLate => unsupported("Messages can only be changed for a limited time after sending."),
        TalkError::Invalid => ChatError::Other(tr("The server does not take this.").to_string()),
    }
}

fn token(token: &str) -> Result<RoomToken, ChatError> {
    RoomToken::parse(token).ok_or(ChatError::NotFound)
}

fn status_kind(status: Status) -> StatusKind {
    match status {
        Status::Online => StatusKind::Online,
        Status::Away => StatusKind::Away,
        Status::Busy => StatusKind::Busy,
        Status::Dnd => StatusKind::Dnd,
        Status::Invisible => StatusKind::Invisible,
        Status::Offline => StatusKind::Offline,
    }
}

fn status(status: UserStatus) -> RemoteStatus {
    RemoteStatus {
        user: status.user_id,
        status: match status.kind {
            StatusKind::Online => Status::Online,
            StatusKind::Away => Status::Away,
            StatusKind::Busy => Status::Busy,
            StatusKind::Dnd => Status::Dnd,
            StatusKind::Invisible => Status::Invisible,
            StatusKind::Offline => Status::Offline,
        },
        message: status.message.filter(|message| !message.trim().is_empty()),
        icon: status.icon.filter(|icon| !icon.trim().is_empty()),
        clear_at: status.clear_at,
    }
}

/// `room` in the service's terms. A direct chat whose other person was deleted stays a direct
/// chat nobody can write in; a lobby the person waits in is read-only too.
pub(crate) fn room(room: Room) -> RemoteRoom {
    let kind = match room.kind {
        RoomKind::OneToOne | RoomKind::FormerOneToOne => ConversationKind::Direct,
        RoomKind::Group => ConversationKind::Group,
        RoomKind::Public => ConversationKind::Public,
        RoomKind::NoteToSelf => ConversationKind::NoteToSelf,
        RoomKind::Changelog | RoomKind::Other => ConversationKind::Other,
    };
    RemoteRoom {
        token: room.token.as_str().to_string(),
        name: room.name,
        kind,
        unread: room.unread_messages,
        mentioned: room.unread_mention,
        mentioned_directly: room.unread_mention_direct,
        last_number: room.last_message.as_ref().and_then(|last| last.id),
        last_message: room.last_message.map(|last| Preview {
            author: if last.system { String::new() } else { last.author_name },
            text: last.text,
            system: last.system,
        }),
        last_activity: room.last_activity,
        last_read: room.last_read,
        other_user: room.other_user.filter(|_| room.kind == RoomKind::OneToOne),
        has_call: room.has_call,
        favorite: room.favorite,
        read_only: room.read_only || room.lobby || room.kind == RoomKind::FormerOneToOne,
        federated: room.federated,
        notify: match room.notification_level {
            NotificationLevel::Always => NotifyLevel::Always,
            NotificationLevel::Mention => NotifyLevel::Mention,
            NotificationLevel::Never => NotifyLevel::Never,
            NotificationLevel::Default | NotificationLevel::Other => NotifyLevel::Default,
        },
    }
}

/// `message` in the service's terms.
fn shown(message: ChatMessage) -> RemoteMessage {
    let event = match &message.kind {
        torro_nextcloud::MessageKind::System { event } => Some(event.as_str()),
        _ => None,
    };
    let changes = event.is_some_and(|event| CHANGES.contains(&event));
    let kind = match (&message.kind, event) {
        (_, Some("call_missed")) => MessageKind::MissedCall,
        (torro_nextcloud::MessageKind::System { .. }, _) => MessageKind::System,
        (torro_nextcloud::MessageKind::Deleted, _) => MessageKind::Deleted,
        (torro_nextcloud::MessageKind::Voice, _) => MessageKind::Voice,
        _ => MessageKind::Message,
    };
    // An edit or a deletion names the message it changed as its parent.
    let patch = match (event, message.parent.as_deref()) {
        (Some("message_deleted"), Some(parent)) => Some(Patch { number: parent.id, text: None, deleted: true }),
        (Some("message_edited"), Some(parent)) => {
            Some(Patch { number: parent.id, text: Some(parent.text.clone()), deleted: false })
        }
        _ => None,
    };
    let reply_to = match (changes, message.parent) {
        (false, Some(parent)) => Some(RemoteQuote {
            number: parent.id,
            author: parent.author_name,
            text: if parent.deleted { String::new() } else { parent.text },
        }),
        _ => None,
    };
    let deleted = kind == MessageKind::Deleted;
    RemoteMessage {
        number: message.id,
        author: (message.actor_type == ActorType::Users).then_some(message.actor_id),
        author_name: message.author_name,
        at: message.at,
        // What a deleted message says is Talk's own note about it; the window writes its own.
        text: if deleted { String::new() } else { message.text },
        kind,
        hidden: changes,
        patch,
        mentions: message
            .mentions
            .into_iter()
            .map(|mention| Mention {
                id: if mention.kind == MentionKind::All { "all".to_string() } else { mention.id },
                name: mention.name,
            })
            .collect(),
        files: if deleted {
            Vec::new()
        } else {
            message
                .files
                .into_iter()
                .map(|file| FileRef {
                    id: file.id.to_string(),
                    name: file.name,
                    size: file.size,
                    content_type: file.mimetype,
                    preview: file.preview_available,
                    hide_download: file.hide_download,
                    width: file.width,
                    height: file.height,
                    blurhash: file.blurhash,
                })
                .collect()
        },
        reply_to,
        reactions: message.reactions,
        own_reactions: message.own_reactions,
        edited: message.edited_at.is_some(),
    }
}

pub async fn user(client: &Nextcloud) -> Result<RemotePerson, ChatError> {
    let user = client.user().await.map_err(ocs)?;
    Ok(RemotePerson { id: user.id, name: user.display_name })
}

pub async fn caps(client: &Nextcloud) -> Result<Caps, ChatError> {
    let capabilities = client.capabilities().await.map_err(ocs)?;
    let status = capabilities.user_status.is_some_and(|status| status.enabled);
    let Some(talk) = capabilities.talk else {
        return Ok(Caps { status, ..Caps::default() });
    };
    Ok(Caps {
        chat: true,
        read_marker: talk.read_marker,
        unread: talk.has("chat-unread"),
        keep_notifications: talk.has("chat-keep-notifications"),
        max_length: talk.max_length,
        silent: talk.has("silent-send"),
        reactions: talk.has("reactions"),
        edit: talk.has("edit-messages"),
        delete: talk.has("delete-messages"),
        search: talk.has("unified-search"),
        status,
    })
}

pub async fn rooms(client: &Nextcloud) -> Result<Vec<RemoteRoom>, ChatError> {
    Ok(client.rooms().await.map_err(ocs)?.into_iter().map(room).collect())
}

pub async fn messages(
    client: &Nextcloud,
    conversation: &str,
    before: Option<u64>,
    limit: u32,
    caps: &Caps,
) -> Result<Vec<RemoteMessage>, ChatError> {
    let found = client.messages(&token(conversation)?, before, limit, caps.keep_notifications).await.map_err(talk)?;
    Ok(found.messages.into_iter().map(shown).collect())
}

pub async fn wait(
    client: &Nextcloud,
    conversation: &str,
    after: u64,
    seconds: u32,
    caps: &Caps,
) -> Result<Vec<RemoteMessage>, ChatError> {
    let limit = torro_nextcloud::MESSAGES_MAX;
    let token = token(conversation)?;
    let found = client.wait_messages(&token, after, seconds, limit, caps.keep_notifications).await.map_err(talk)?;
    Ok(found.messages.into_iter().map(shown).collect())
}

pub async fn send(client: &Nextcloud, conversation: &str, sending: &Sending) -> Result<RemoteMessage, ChatError> {
    let options = SendOptions {
        reply_to: sending.reply_to,
        reference_id: Some(sending.reference.clone()),
        silent: sending.silent,
    };
    client.send_message(&token(conversation)?, &sending.text, &options).await.map(shown).map_err(talk)
}

pub async fn mark_read(client: &Nextcloud, conversation: &str, up_to: u64) -> Result<(), ChatError> {
    client.mark_read(&token(conversation)?, up_to).await.map_err(|err| match err {
        // On this call a 404 is the conversation, not the app: Talk's presence was checked before.
        OcsError::MissingApp => ChatError::NotFound,
        err => ocs(err),
    })
}

pub async fn mark_unread(client: &Nextcloud, conversation: &str) -> Result<(), ChatError> {
    client.mark_unread(&token(conversation)?).await.map_err(talk)
}

pub async fn mentions(
    client: &Nextcloud,
    conversation: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<Mention>, ChatError> {
    let found = client.mention_suggestions(&token(conversation)?, query, limit).await.map_err(talk)?;
    Ok(found.into_iter().map(|suggestion| Mention { id: suggestion.mention_id, name: suggestion.label }).collect())
}

pub async fn find_people(client: &Nextcloud, query: &str, limit: u32) -> Result<Vec<RemotePerson>, ChatError> {
    let found = client.find_people(query, limit).await.map_err(ocs)?;
    Ok(found.into_iter().map(|person| RemotePerson { id: person.id, name: person.name }).collect())
}

pub async fn find_users(client: &Nextcloud, query: &str, limit: u32) -> Result<Vec<RemotePerson>, ChatError> {
    let found = client.find_sharees(query, limit).await.map_err(ocs)?;
    Ok(found.into_iter().map(|person| RemotePerson { id: person.id, name: person.name }).collect())
}

pub async fn create_direct(client: &Nextcloud, user: &str) -> Result<RemoteRoom, ChatError> {
    client.create_one_to_one(user).await.map(room).map_err(talk)
}

pub async fn edit(client: &Nextcloud, conversation: &str, number: u64, text: &str) -> Result<(), ChatError> {
    client.edit_message(&token(conversation)?, number, text).await.map_err(talk)
}

pub async fn delete(client: &Nextcloud, conversation: &str, number: u64) -> Result<(), ChatError> {
    client.delete_message(&token(conversation)?, number).await.map_err(talk)
}

pub async fn react(
    client: &Nextcloud,
    conversation: &str,
    number: u64,
    emoji: &str,
    remove: bool,
) -> Result<(), ChatError> {
    let token = token(conversation)?;
    let done = match remove {
        true => client.unreact(&token, number, emoji).await,
        false => client.react(&token, number, emoji).await,
    };
    done.map_err(talk)
}

pub async fn set_favorite(client: &Nextcloud, conversation: &str, favorite: bool) -> Result<(), ChatError> {
    client.set_favorite(&token(conversation)?, favorite).await.map_err(talk)
}

pub async fn set_notify(client: &Nextcloud, conversation: &str, level: NotifyLevel) -> Result<(), ChatError> {
    let level = match level {
        NotifyLevel::Default => NotificationLevel::Default,
        NotifyLevel::Always => NotificationLevel::Always,
        NotifyLevel::Mention => NotificationLevel::Mention,
        NotifyLevel::Never => NotificationLevel::Never,
    };
    client.set_notification_level(&token(conversation)?, level).await.map_err(talk)
}

pub async fn search(client: &Nextcloud, query: &str, limit: u32) -> Result<Vec<RemoteHit>, ChatError> {
    let found = client.search_messages(query, limit).await.map_err(talk)?;
    Ok(found
        .into_iter()
        .map(|hit| RemoteHit {
            token: hit.token.as_str().to_string(),
            number: hit.message_id,
            title: hit.title,
            text: hit.text,
        })
        .collect())
}

pub async fn statuses(client: &Nextcloud) -> Result<Vec<RemoteStatus>, ChatError> {
    let mut all = Vec::new();
    for page in 0..STATUS_PAGES {
        let found = client.statuses(STATUS_PAGE, page * STATUS_PAGE).await.map_err(ocs)?;
        let last = (found.len() as u32) < STATUS_PAGE;
        all.extend(found.into_iter().map(status));
        if last {
            break;
        }
    }
    Ok(all)
}

pub async fn status_of(client: &Nextcloud, user: &str) -> Result<Option<RemoteStatus>, ChatError> {
    Ok(client.status_of(user).await.map_err(ocs)?.map(status))
}

/// The person's own status; offline while the server never kept one for them.
pub async fn own_status(client: &Nextcloud) -> Result<RemoteStatus, ChatError> {
    let own = client.own_status().await.map_err(ocs)?.map(status);
    Ok(own.unwrap_or(RemoteStatus {
        user: String::new(),
        status: Status::Offline,
        message: None,
        icon: None,
        clear_at: None,
    }))
}

pub async fn set_status(client: &Nextcloud, status: Status) -> Result<(), ChatError> {
    client.set_status(status_kind(status)).await.map_err(ocs)
}

pub async fn set_status_message(
    client: &Nextcloud,
    message: Option<&str>,
    icon: Option<&str>,
    clear_at: Option<DateTime<Utc>>,
) -> Result<(), ChatError> {
    let done = match message {
        Some(message) => client.set_status_message(icon, Some(message), clear_at).await,
        None => client.clear_status_message().await,
    };
    done.map_err(ocs)
}

pub async fn heartbeat(client: &Nextcloud, status: Status) -> Result<(), ChatError> {
    client.heartbeat(status_kind(status)).await.map_err(ocs)
}
