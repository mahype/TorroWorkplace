//! The notifications app: what a Nextcloud tells its person (shares, mentions, calls, updates),
//! read for the inbox source `nextcloud:<login>@<host>` (task E5.8).
//!
//! - List: `GET /ocs/v2.php/apps/notifications/api/v2/notifications`. The answer carries an `ETag`
//!   (a hash of the notification ids); sent back as `If-None-Match`, Nextcloud answers `304` when
//!   nothing changed. `204` means no app on the server sends notifications.
//! - Delete one: `DELETE /ocs/v2.php/apps/notifications/api/v2/notifications/{id}`, `id` digits.
//!
//! Operations `endpoint-list-notifications` and `endpoint-delete-notification`, schema
//! `Notification`, in <https://github.com/nextcloud/notifications/blob/master/openapi.json>;
//! `lib/Controller/EndpointController.php` there for the ETag.

use std::collections::HashSet;
use std::fmt;

use chrono::{DateTime, Utc};
use reqwest::Method;
use serde_json::Value;

use crate::ocs::{Nextcloud, OcsError, Reply};
use crate::rich;

const LIST_PATH: &str = "/ocs/v2.php/apps/notifications/api/v2/notifications";

/// The id of a notification: a positive number, as Nextcloud's route requires (`\d+`). The only
/// way to name one to [`Nextcloud::delete_notification`], so no other text gets into its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NotificationId(u64);

impl NotificationId {
    /// `id` as a notification id, or `None` when it is empty, holds anything but the digits `0`–`9`,
    /// is 0 or does not fit `u64` (to read back an id stored as text).
    pub fn parse(id: &str) -> Option<NotificationId> {
        let digits = !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit());
        digits.then(|| id.parse().ok()).flatten().filter(|n| *n > 0).map(NotificationId)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for NotificationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A Nextcloud user named in a rich text (a parameter of type `user`) or as the author of a Talk
/// message: the user id and the display name, both foreign text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Person {
    pub id: String,
    pub name: String,
}

/// One notification as checked from the server's answer ([`read_notifications`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: NotificationId,
    /// The app that sent it (`files_sharing`, `spreed`, `updatenotification` …): ASCII letters,
    /// digits, `_`, `-` and `.`, at most 64 of them.
    pub app: String,
    /// What it is about, as the app names it (`share`, `chat`, `call` …); foreign text, raw.
    pub object_type: String,
    /// Which one of those (a share id, a Talk token, a path …); foreign text, raw.
    pub object_id: String,
    /// When the notification was made (`datetime`).
    pub time: DateTime<Utc>,
    /// The title in one line: `subjectRich` with its parameters ([`rich::line`]), else `subject`.
    pub subject: String,
    /// The text: `messageRich` with its parameters ([`rich::plain`]), else `message`; may be empty
    /// and may hold line breaks.
    pub message: String,
    /// The page to open, absolute and only on the account's own origin; `None` otherwise.
    pub link: Option<String>,
    /// A parameter of type `user` in the subject, else in the message; the parameter `user` comes first.
    pub person: Option<Person>,
}

/// What [`Nextcloud::notifications`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing {
    /// `304`: nothing changed since the ETag that was sent, which stays the one to send.
    NotModified,
    /// The person's notifications, newest first as the server lists them.
    List {
        notifications: Vec<Notification>,
        /// The answer's `ETag`, to send next time; `None` when there was none (also after a `204`).
        etag: Option<String>,
    },
}

impl Nextcloud {
    /// The person's notifications. With `etag` (the [`Listing::List`] `etag` of the last call) the
    /// server may answer [`Listing::NotModified`]. A `204` (no app sends notifications) is an empty
    /// list. [`OcsError::MissingApp`] when the notifications app is not there; other errors as
    /// every OCS call.
    pub async fn notifications(&self, etag: Option<&str>) -> Result<Listing, OcsError> {
        match self.get(LIST_PATH, etag).await? {
            Reply::NotModified => Ok(Listing::NotModified),
            Reply::Empty => Ok(Listing::List { notifications: Vec::new(), etag: None }),
            Reply::Data { data, etag } => Ok(Listing::List { notifications: read_notifications(&data, self)?, etag }),
        }
    }

    /// Deletes the notification `id` for the person (also on their other devices). A `404` is
    /// success: the notification is already gone (or the app with it). Other errors as every OCS
    /// call.
    pub async fn delete_notification(&self, id: NotificationId) -> Result<(), OcsError> {
        match self.call(Method::DELETE, &format!("{LIST_PATH}/{id}"), None).await {
            Ok(_) | Err(OcsError::MissingApp) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// The notifications in the `data` of a list answer (a JSON list), each id once. An entry is left
/// out when its id is no [`NotificationId`], its app no app id, its `datetime` no RFC 3339 time or
/// `object_type`/`object_id` are missing. [`OcsError::Other`] when `data` is no list.
pub(crate) fn read_notifications(data: &Value, account: &Nextcloud) -> Result<Vec<Notification>, OcsError> {
    let entries =
        data.as_array().ok_or_else(|| OcsError::Other("Nextcloud sent notifications that are no list".into()))?;
    let mut seen = HashSet::new();
    Ok(entries.iter().filter_map(|entry| read_notification(entry, account)).filter(|n| seen.insert(n.id)).collect())
}

fn read_notification(entry: &Value, account: &Nextcloud) -> Option<Notification> {
    let text = |key: &str| entry.get(key).and_then(Value::as_str);
    let id = entry.get("notification_id").and_then(Value::as_u64).filter(|n| *n > 0).map(NotificationId)?;
    let app = text("app").filter(|app| is_app_id(app))?.to_string();
    let time = DateTime::parse_from_rfc3339(text("datetime")?).ok()?.with_timezone(&Utc);
    let object_type = text("object_type")?.to_string();
    let object_id = text("object_id")?.to_string();
    let subject_parameters = entry.get("subjectRichParameters").unwrap_or(&Value::Null);
    let message_parameters = entry.get("messageRichParameters").unwrap_or(&Value::Null);
    let subject = match text("subjectRich").filter(|s| !s.trim().is_empty()) {
        Some(rich) => rich::line(rich, subject_parameters),
        None => rich::line(text("subject").unwrap_or_default(), &Value::Null),
    };
    let message = match text("messageRich").filter(|s| !s.trim().is_empty()) {
        Some(rich) => rich::plain(rich, message_parameters),
        None => rich::plain(text("message").unwrap_or_default(), &Value::Null),
    };
    let link = text("link").and_then(|link| account.own_link(link));
    let person = first_user(subject_parameters).or_else(|| first_user(message_parameters));
    Some(Notification { id, app, object_type, object_id, time, subject, message, link, person })
}

/// The parameter `user` when it is of type `user`, else the first such parameter in key order
/// (serde_json keeps a map sorted, not in the order the server sent it); it needs an id.
fn first_user(parameters: &Value) -> Option<Person> {
    let parameters = parameters.as_object()?;
    let named = parameters.get("user").into_iter();
    named.chain(parameters.values()).find_map(|parameter| {
        if parameter.get("type").and_then(Value::as_str) != Some("user") {
            return None;
        }
        let id = rich::line(parameter.get("id").and_then(Value::as_str)?, &Value::Null);
        let name = parameter.get("name").and_then(Value::as_str).map(|name| rich::line(name, &Value::Null));
        let name = name.filter(|name| !name.is_empty()).unwrap_or_else(|| id.clone());
        (!id.is_empty()).then_some(Person { id, name })
    })
}

/// Whether `app` looks like a Nextcloud app id.
fn is_app_id(app: &str) -> bool {
    (1..=64).contains(&app.len()) && app.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

#[cfg(test)]
mod tests {
    use httpmock::Method::{DELETE, GET};
    use httpmock::MockServer;
    use torro_core::account::Secret;

    use super::*;
    use crate::ocs::envelope;
    use crate::ocs::tests::{AUTHORIZATION, PASSWORD, account};

    const FIXTURE: &str = include_str!("../tests/fixtures/notifications.json");
    const SERVER: &str = "https://cloud.example.org";

    fn fixture() -> Vec<Notification> {
        let account = Nextcloud::new(SERVER, "anna", Secret::new(PASSWORD)).expect("an account");
        let data = envelope(serde_json::from_str(FIXTURE).expect("fixture is JSON")).expect("an envelope");
        read_notifications(&data, &account).expect("a list")
    }

    fn list(listing: Listing) -> (Vec<Notification>, Option<String>) {
        match listing {
            Listing::List { notifications, etag } => (notifications, etag),
            Listing::NotModified => panic!("expected a list, got 304"),
        }
    }

    #[test]
    fn reads_a_share_with_its_person_and_link() {
        let notifications = fixture();
        let share = &notifications[0];
        assert_eq!(share.id, NotificationId(1043));
        assert_eq!(share.app, "files_sharing");
        assert_eq!((share.object_type.as_str(), share.object_id.as_str()), ("remote_share", "77"));
        assert_eq!(share.time.to_rfc3339(), "2026-10-13T08:15:00+00:00");
        assert_eq!(share.subject, "Bert Example shared Quarterly report.pdf with you");
        assert_eq!(share.message, "");
        assert_eq!(share.link.as_deref(), Some("https://cloud.example.org/apps/files/?fileid=4711"));
        assert_eq!(share.person, Some(Person { id: "bert".into(), name: "Bert Example".into() }));
    }

    #[test]
    fn a_talk_mention_has_its_message_and_placeholders_replaced() {
        let notifications = fixture();
        let mention = notifications.iter().find(|n| n.id == NotificationId(1042)).expect("the mention");
        assert_eq!(mention.app, "spreed");
        assert_eq!((mention.object_type.as_str(), mention.object_id.as_str()), ("chat", "x7k2m9qa"));
        assert_eq!(mention.subject, "Carla Example mentioned you in conversation Team Alpha");
        assert_eq!(mention.message, "Hi Anna Example, can you check the plan?");
        assert_eq!(mention.link.as_deref(), Some("https://cloud.example.org/call/x7k2m9qa"));
        assert_eq!(mention.person.as_ref().map(|p| p.id.as_str()), Some("carla"));
    }

    #[test]
    fn a_link_on_another_origin_is_dropped() {
        let notifications = fixture();
        let foreign = notifications.iter().find(|n| n.id == NotificationId(1041)).expect("the update");
        assert_eq!(foreign.link, None);
        assert_eq!(foreign.subject, "Update for Calendar to version 5.5.3 is available.");
        assert_eq!(foreign.person, None, "an empty PHP map has no parameters");
    }

    #[test]
    fn plain_subjects_are_used_without_rich_ones_and_bad_entries_are_left_out() {
        let notifications = fixture();
        let ids: Vec<u64> = notifications.iter().map(|n| n.id.get()).collect();
        assert_eq!(ids, vec![1043, 1042, 1041, 1040], "bad id, app or time left out, duplicate once");
        let plain = notifications.iter().find(|n| n.id == NotificationId(1040)).expect("the plain one");
        assert_eq!(plain.subject, "Backup finished");
        assert_eq!(plain.message, "All files are saved.\nNext backup tomorrow.");
    }

    #[test]
    fn ids_are_digits() {
        assert_eq!(NotificationId::parse("1043"), Some(NotificationId(1043)));
        assert_eq!(NotificationId::parse("0"), None);
        assert_eq!(NotificationId::parse(""), None);
        assert_eq!(NotificationId::parse("12a"), None);
        assert_eq!(NotificationId::parse("+12"), None);
        assert_eq!(NotificationId::parse("99999999999999999999999"), None);
        assert_eq!(NotificationId(7).to_string(), "7");
    }

    #[tokio::test]
    async fn lists_with_etag_and_keeps_links_on_the_server() {
        let server = MockServer::start_async().await;
        let body = FIXTURE.replace(SERVER, &server.base_url());
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(LIST_PATH)
                    .header("authorization", AUTHORIZATION)
                    .header("ocs-apirequest", "true")
                    .header("accept", "application/json")
                    .header_missing("if-none-match");
                then.status(200).header("ETag", "\"9f86d081\"").body(body);
            })
            .await;
        let (notifications, etag) = list(account(&server).notifications(None).await.expect("listed"));
        assert_eq!(etag.as_deref(), Some("\"9f86d081\""));
        assert_eq!(notifications.len(), 4);
        let link = format!("{}/apps/files/?fileid=4711", server.base_url());
        assert_eq!(notifications[0].link.as_deref(), Some(link.as_str()));
    }

    #[tokio::test]
    async fn the_etag_gets_a_304() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(LIST_PATH).header("if-none-match", "\"9f86d081\"");
                then.status(304);
            })
            .await;
        let listing = account(&server).notifications(Some("\"9f86d081\"")).await;
        assert_eq!(listing, Ok(Listing::NotModified));
    }

    #[tokio::test]
    async fn no_notifying_apps_is_an_empty_list() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(LIST_PATH);
                then.status(204);
            })
            .await;
        let listing = account(&server).notifications(None).await;
        assert_eq!(listing, Ok(Listing::List { notifications: Vec::new(), etag: None }));
    }

    #[tokio::test]
    async fn refusals_missing_app_and_maintenance_are_errors() {
        for (status, error) in [(401, OcsError::Refused), (404, OcsError::MissingApp), (503, OcsError::Maintenance)] {
            let server = MockServer::start_async().await;
            server
                .mock_async(|when, then| {
                    when.method(GET).path(LIST_PATH);
                    then.status(status).body(r#"{"ocs":{"meta":{"status":"failure"},"data":[]}}"#);
                })
                .await;
            assert_eq!(account(&server).notifications(None).await, Err(error), "HTTP {status}");
        }
    }

    #[tokio::test]
    async fn deletes_one_notification() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{LIST_PATH}/1043")).header("authorization", AUTHORIZATION);
                then.status(200).body(r#"{"ocs":{"meta":{"status":"ok"},"data":[]}}"#);
            })
            .await;
        assert_eq!(account(&server).delete_notification(NotificationId(1043)).await, Ok(()));
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn deleting_one_that_is_gone_is_fine_and_a_refusal_is_not() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{LIST_PATH}/5"));
                then.status(404).body(r#"{"ocs":{"meta":{"status":"failure"},"data":[]}}"#);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{LIST_PATH}/6"));
                then.status(401);
            })
            .await;
        let account = account(&server);
        assert_eq!(account.delete_notification(NotificationId(5)).await, Ok(()));
        assert_eq!(account.delete_notification(NotificationId(6)).await, Err(OcsError::Refused));
    }
}
