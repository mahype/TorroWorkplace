//! Talk (`spreed`): the person's conversations with what is unread, read for the inbox source
//! `talk:<login>@<host>` (task E5.9), and marking a conversation read.
//!
//! - Conversations: `GET /ocs/v2.php/apps/spreed/api/v4/room?noStatusUpdate=1` (operation
//!   `room-get-rooms`). `noStatusUpdate=1` keeps polling from setting the person "online".
//! - Read marker: `POST /ocs/v2.php/apps/spreed/api/v1/chat/{token}/read` with
//!   `{"lastReadMessage": <id>}` (operation `chat-set-read-marker`, capability `chat-read-marker`).
//!
//! Schemas `Room`, `RoomLastMessage`, `ChatMessage` and the token pattern `^[a-z0-9]{4,30}$` in
//! <https://github.com/nextcloud/spreed/blob/main/openapi.json>; conversation types and
//! notification levels in <https://nextcloud-talk.readthedocs.io/en/latest/constants/>.

use std::collections::HashSet;
use std::fmt;

use chrono::{DateTime, Utc};
use reqwest::Method;
use serde_json::{Value, json};

use crate::notifications::Person;
use crate::ocs::{Nextcloud, OcsError, Reply};
use crate::rich;

const ROOMS_PATH: &str = "/ocs/v2.php/apps/spreed/api/v4/room?noStatusUpdate=1";

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
    let token = RoomToken::parse(entry.get("token")?.as_str()?)?;
    let kind = RoomKind::from_talk(entry.get("type")?.as_i64()?);
    let display = rich::line(text("displayName"), &Value::Null);
    let name = if display.is_empty() { rich::line(text("name"), &Value::Null) } else { display };
    let last_activity = DateTime::from_timestamp(entry.get("lastActivity")?.as_i64()?, 0)?;
    Some(Room {
        token,
        kind,
        name,
        unread_messages: entry.get("unreadMessages").and_then(Value::as_u64).unwrap_or(0),
        unread_mention: flag("unreadMention"),
        unread_mention_direct: flag("unreadMentionDirect"),
        notification_level: NotificationLevel::from_talk(
            entry.get("notificationLevel").and_then(Value::as_i64).unwrap_or(0),
        ),
        last_message: entry.get("lastMessage").and_then(read_last_message),
        last_activity,
        last_read: entry.get("lastReadMessage").and_then(Value::as_u64).filter(|id| *id > 0),
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

#[cfg(test)]
mod tests {
    use httpmock::Method::{GET, POST};
    use httpmock::MockServer;

    use super::*;
    use crate::ocs::envelope;
    use crate::ocs::tests::{AUTHORIZATION, account};

    const FIXTURE: &str = include_str!("../tests/fixtures/talk_rooms.json");

    fn fixture() -> Vec<Room> {
        read_rooms(&envelope(serde_json::from_str(FIXTURE).expect("fixture is JSON")).expect("an envelope"))
            .expect("a list")
    }

    fn room<'a>(rooms: &'a [Room], token: &str) -> &'a Room {
        rooms.iter().find(|room| room.token.as_str() == token).expect("the room")
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
}
