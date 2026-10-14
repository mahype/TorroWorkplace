//! One line of ntfy's JSON stream (`/<topics>/json`, documented at
//! <https://docs.ntfy.sh/subscribe/api/>) read into an [`Event`].
//!
//! Every line is a JSON object with an `event`: `open` when the subscription starts, `keepalive`
//! now and then, `message` for a message; anything else (`poll_request`, events added later) is
//! left out. A [`Message`] keeps only checked fields, so nothing the server sends goes further
//! unchecked: the fields `actions`, `attachment` and `icon` are not even read.

use std::fmt;

use chrono::{DateTime, Utc};
use reqwest::Url;
use serde_json::{Map, Value};

use crate::server::Topic;

/// The priority of a message without one, or with one outside 1–5.
pub const DEFAULT_PRIORITY: u8 = 3;

/// How many tags a [`Message`] keeps at most; further ones are left out.
pub const MAX_TAGS: usize = 10;

/// How many characters a tag may have; a longer one is left out.
pub const MAX_TAG_CHARS: usize = 64;

/// The longest message id taken; ntfy's own have 12 characters.
const MAX_ID: usize = 32;

/// What one line of the stream says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The subscription is open.
    Open,
    /// The server is still there; nothing happened.
    Keepalive,
    /// A message in one of the subscribed topics.
    Message(Message),
}

/// A message as checked from the stream ([`read_event`]).
///
/// `title` and `text` are foreign text: shown, never followed. `Debug` shows no topic name
/// ([`Topic`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: MessageId,
    /// When the server received it (`time`, Unix seconds).
    pub time: DateTime<Utc>,
    /// The topic it was sent to.
    pub topic: Topic,
    /// The title, `None` when it is missing or blank.
    pub title: Option<String>,
    /// The message text (`message`), empty when there is none.
    pub text: String,
    /// 1 (min) to 5 (max, urgent); [`DEFAULT_PRIORITY`] when missing or outside that range.
    pub priority: u8,
    /// The tags as sent, at most [`MAX_TAGS`], each 1 to [`MAX_TAG_CHARS`] characters; tags that
    /// are not strings, empty or longer are left out (not cut, so no tag reads as another).
    pub tags: Vec<String>,
    /// The page the sender asks to open (`click`), only an absolute `http`/`https` address with a
    /// host and without a login in it ([`checked_link`]); `None` otherwise.
    pub click: Option<String>,
}

/// The id of a message: 1–32 letters and digits (ntfy's ids are 12 of them). The value to send as
/// `since` when the stream is opened again ([`crate::Ntfy::subscribe`]), so no other text ever ends
/// up in that address.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId(String);

impl MessageId {
    /// `id` as a message id, or `None` when it is empty, longer than 32 or holds anything but
    /// `A–Z a–z 0–9`. Also for an id kept from an earlier stream (the inbox's cursor).
    pub fn parse(id: &str) -> Option<MessageId> {
        let fits = !id.is_empty() && id.len() <= MAX_ID && id.bytes().all(|b| b.is_ascii_alphanumeric());
        fits.then(|| MessageId(id.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The event of one line (without its line end), or `None` when the line is no JSON object, has
/// an event this code does not know, or is a `message` that fails a check: an id that is no
/// [`MessageId`], a missing or wrong `time`, a `topic` that is no [`Topic`].
pub fn read_event(line: &[u8]) -> Option<Event> {
    let value: Value = serde_json::from_slice(line).ok()?;
    let object = value.as_object()?;
    match object.get("event")?.as_str()? {
        "open" => Some(Event::Open),
        "keepalive" => Some(Event::Keepalive),
        "message" => read_message(object).map(Event::Message),
        _ => None,
    }
}

fn read_message(object: &Map<String, Value>) -> Option<Message> {
    let text = |key: &str| object.get(key).and_then(Value::as_str);
    let id = MessageId::parse(text("id")?)?;
    let time = DateTime::from_timestamp(object.get("time")?.as_i64()?, 0)?;
    let topic = Topic::parse(text("topic")?).ok()?;
    let title = text("title").filter(|title| !title.trim().is_empty()).map(str::to_string);
    let priority = object
        .get("priority")
        .and_then(Value::as_u64)
        .and_then(|p| u8::try_from(p).ok())
        .filter(|p| (1..=5).contains(p))
        .unwrap_or(DEFAULT_PRIORITY);
    Some(Message {
        id,
        time,
        topic,
        title,
        text: text("message").unwrap_or_default().to_string(),
        priority,
        tags: object.get("tags").map(read_tags).unwrap_or_default(),
        click: text("click").and_then(checked_link),
    })
}

fn read_tags(tags: &Value) -> Vec<String> {
    let Some(tags) = tags.as_array() else {
        return Vec::new();
    };
    tags.iter()
        .filter_map(Value::as_str)
        .filter(|tag| !tag.is_empty() && tag.chars().count() <= MAX_TAG_CHARS)
        .take(MAX_TAGS)
        .map(str::to_string)
        .collect()
}

/// `link` as an address a person may open: absolute, `http` or `https`, with a host and without a
/// user name or password in it (`https://bank.example@evil.example` is refused). The address
/// comes back as the URL parser writes it; `None` for anything else (`javascript:`, `file:`,
/// relative addresses).
pub fn checked_link(link: &str) -> Option<String> {
    let url = Url::parse(link.trim()).ok()?;
    let web = matches!(url.scheme(), "http" | "https");
    let host = url.host_str().is_some_and(|host| !host.is_empty());
    let no_login = url.username().is_empty() && url.password().is_none();
    (web && host && no_login).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> &'static str {
        match name {
            "messages" => include_str!("../tests/fixtures/messages.ndjson"),
            "invalid" => include_str!("../tests/fixtures/invalid.ndjson"),
            other => panic!("no fixture {other}"),
        }
    }

    fn events(name: &str) -> Vec<Event> {
        fixture(name).lines().filter_map(|line| read_event(line.as_bytes())).collect()
    }

    fn messages(name: &str) -> Vec<Message> {
        events(name)
            .into_iter()
            .filter_map(|event| match event {
                Event::Message(message) => Some(message),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn open_keepalive_and_messages_are_read_and_other_events_ignored() {
        let kinds: Vec<&str> = events("messages")
            .iter()
            .map(|event| match event {
                Event::Open => "open",
                Event::Keepalive => "keepalive",
                Event::Message(_) => "message",
            })
            .collect();
        assert_eq!(kinds, ["open", "message", "keepalive", "message", "message", "message"]);
    }

    #[test]
    fn a_full_message_keeps_its_checked_fields() {
        let all = messages("messages");
        let full = &all[1];
        assert_eq!(full.id.as_str(), "h8t4Fgaw7w");
        assert_eq!(full.time, DateTime::from_timestamp(1_760_427_000, 0).expect("time"));
        assert_eq!(full.topic.as_str(), "backups_x7Qp2");
        assert_eq!(full.title.as_deref(), Some("Backup failed"));
        assert_eq!(full.text, "The nightly backup of nas-01 stopped at 03:12.\nDisk full.");
        assert_eq!(full.priority, 5);
        assert_eq!(full.tags, ["warning", "floppy_disk"]);
        assert_eq!(full.click.as_deref(), Some("https://nas.example.org/backups/42"));
    }

    #[test]
    fn a_bare_message_gets_the_defaults() {
        let bare = &messages("messages")[0];
        assert_eq!(bare.id.as_str(), "hwQ2YpKdmg");
        assert_eq!(bare.title, None);
        assert_eq!(bare.text, "Disk almost full");
        assert_eq!(bare.priority, DEFAULT_PRIORITY);
        assert!(bare.tags.is_empty());
        assert_eq!(bare.click, None);
    }

    #[test]
    fn priorities_tags_and_links_are_checked() {
        let all = messages("messages");
        let odd = &all[2];
        assert_eq!(odd.priority, DEFAULT_PRIORITY, "9 is outside 1-5");
        assert_eq!(odd.title, None, "a blank title is none");
        assert_eq!(odd.click, None, "javascript: is no link");
        assert_eq!(odd.tags.len(), MAX_TAGS, "at most ten tags");
        assert_eq!(odd.tags[0], "t1", "a tag of 65 characters, an empty one and a number are left out");
        assert!(odd.tags.iter().all(|tag| !tag.is_empty() && tag.chars().count() <= MAX_TAG_CHARS));
        let last = &all[3];
        assert_eq!(last.priority, 1);
        assert_eq!(last.click, None, "a login in the address is refused");
        assert_eq!(last.tags, ["ä".repeat(MAX_TAG_CHARS)], "characters are counted, not bytes");
    }

    #[test]
    fn broken_lines_and_messages_that_fail_a_check_are_left_out() {
        assert_eq!(events("invalid"), [], "every line of the fixture is broken in its own way");
        assert_eq!(read_event(b""), None);
        assert_eq!(read_event(b"   "), None);
    }

    #[test]
    fn links_are_web_addresses_only() {
        assert_eq!(checked_link(" https://example.org/a?b=1 ").as_deref(), Some("https://example.org/a?b=1"));
        assert_eq!(checked_link("http://example.org").as_deref(), Some("http://example.org/"));
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://example.org",
            "/relative",
            "mailto:a@example.org",
            "https://user:pw@example.org",
            "https://bank.example@evil.example",
            "data:text/html,hi",
        ] {
            assert_eq!(checked_link(bad), None, "{bad}");
        }
    }

    #[test]
    fn message_ids_are_checked_strictly() {
        for good in ["hwQ2YpKdmg", "1", &"a".repeat(32)] {
            assert!(MessageId::parse(good).is_some(), "{good}");
        }
        for bad in ["", &"a".repeat(33), "a-b", "a b", "a&since=all", "ä"] {
            assert!(MessageId::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_message_shows_no_topic_name() {
        let shown = format!("{:?}", messages("messages")[1]);
        assert!(!shown.contains("backups_x7Qp2"), "{shown}");
    }
}
