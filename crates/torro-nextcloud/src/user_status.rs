//! The user status app (`user_status`, #643): who is online, away, busy or not to be disturbed,
//! with an emoji and a message, and the person's own status with its heartbeat.
//!
//! - Others: `GET /ocs/v2.php/apps/user_status/api/v1/statuses?limit=…&offset=…` (operation
//!   `statuses-find-all`; only users who ever had a status, and the admin's sharing settings may
//!   hide people) and `GET …/statuses/{userId}` (`statuses-find`, 404 when there is none).
//! - Own: `GET …/user_status` (404 while the person never had one), `PUT …/user_status/status`
//!   with `statusType`, `PUT …/user_status/message/custom` with `statusIcon`, `message`, `clearAt`,
//!   `DELETE …/user_status/message`.
//! - Heartbeat: `PUT …/heartbeat` with `status` `online` or `away` (204 when there is no status to
//!   keep alive). Without it Nextcloud lets "online" fall to offline after some minutes.
//!
//! <https://docs.nextcloud.com/server/latest/developer_manual/client_apis/OCS/ocs-status-api.html>,
//! schemas `Public`, `Private` and `Type` in
//! <https://github.com/nextcloud/server/blob/master/apps/user_status/openapi.json>.
//!
//! Status messages and icons are other people's text: made one line ([`rich::line`]).

use chrono::{DateTime, Utc};
use reqwest::Method;
use serde_json::{Value, json};

use crate::ocs::{Nextcloud, OcsError};
use crate::rich;

const BASE: &str = "/ocs/v2.php/apps/user_status/api/v1";

/// The most statuses one page asks for.
const PAGE_MAX: u32 = 500;

/// What someone's status says (`status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusKind {
    Online,
    Away,
    /// "Busy" (newer servers, capability `supports_busy`).
    Busy,
    /// "Do not disturb".
    Dnd,
    /// Online, but shown as offline to others; only the person's own status says so.
    Invisible,
    Offline,
}

impl StatusKind {
    /// The status of Nextcloud's word for it; a word this code does not know is
    /// [`StatusKind::Offline`].
    pub fn parse(word: &str) -> StatusKind {
        match word {
            "online" => StatusKind::Online,
            "away" => StatusKind::Away,
            "busy" => StatusKind::Busy,
            "dnd" => StatusKind::Dnd,
            "invisible" => StatusKind::Invisible,
            _ => StatusKind::Offline,
        }
    }

    /// Nextcloud's word for the status.
    pub fn as_str(self) -> &'static str {
        match self {
            StatusKind::Online => "online",
            StatusKind::Away => "away",
            StatusKind::Busy => "busy",
            StatusKind::Dnd => "dnd",
            StatusKind::Invisible => "invisible",
            StatusKind::Offline => "offline",
        }
    }
}

/// Someone's status as checked from the server's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserStatus {
    /// The user id (`userId`), one line.
    pub user_id: String,
    pub kind: StatusKind,
    /// The status message, one line; `None` when empty. Foreign text.
    pub message: Option<String>,
    /// The emoji of the status, one line; `None` when empty.
    pub icon: Option<String>,
    /// When the message clears itself (`clearAt`).
    pub clear_at: Option<DateTime<Utc>>,
}

impl Nextcloud {
    /// One page of statuses of other users: up to `limit` (1 to 500) from `offset` on. Entries
    /// without a user id are left out. [`OcsError::MissingApp`] without the user status app.
    pub async fn statuses(&self, limit: u32, offset: u32) -> Result<Vec<UserStatus>, OcsError> {
        let query = [("limit", limit.clamp(1, PAGE_MAX).to_string()), ("offset", offset.to_string())];
        let url = self.endpoint(&format!("{BASE}/statuses"), &[], &query)?;
        let data = self.exchange(Method::GET, url, None, None).await?.success()?;
        let list = data.as_ref().and_then(Value::as_array);
        Ok(list.map(|list| list.iter().filter_map(read_status).collect()).unwrap_or_default())
    }

    /// The status of the user `user_id`; `None` when they have none (404). The id goes into the
    /// address as one path segment of its own.
    pub async fn status_of(&self, user_id: &str) -> Result<Option<UserStatus>, OcsError> {
        let url = self.endpoint::<&str>(&format!("{BASE}/statuses"), &[user_id], &[])?;
        let answer = self.exchange(Method::GET, url, None, None).await?;
        if answer.status == 404 {
            return Ok(None);
        }
        Ok(answer.success()?.as_ref().and_then(read_status))
    }

    /// The person's own status; `None` while they never had one (404: Nextcloud makes it on the
    /// first heartbeat or change).
    pub async fn own_status(&self) -> Result<Option<UserStatus>, OcsError> {
        let url = self.endpoint::<&str>(&format!("{BASE}/user_status"), &[], &[])?;
        let answer = self.exchange(Method::GET, url, None, None).await?;
        if answer.status == 404 {
            return Ok(None);
        }
        Ok(answer.success()?.as_ref().and_then(read_status))
    }

    /// Sets the person's own status. [`OcsError::Other`] when the server does not take it (for
    /// example "busy" on an older server).
    pub async fn set_status(&self, kind: StatusKind) -> Result<(), OcsError> {
        self.status_call(Method::PUT, "/user_status/status", Some(json!({ "statusType": kind.as_str() }))).await
    }

    /// Sets the person's own status message with an optional emoji and an optional time when it
    /// clears itself. [`OcsError::Other`] when the server does not take it (too long, an icon that
    /// is no single emoji, a time in the past).
    pub async fn set_status_message(
        &self,
        icon: Option<&str>,
        message: Option<&str>,
        clear_at: Option<DateTime<Utc>>,
    ) -> Result<(), OcsError> {
        let body = json!({
            "statusIcon": icon,
            "message": message,
            "clearAt": clear_at.map(|at| at.timestamp()),
        });
        self.status_call(Method::PUT, "/user_status/message/custom", Some(body)).await
    }

    /// Clears the person's own status message (the status stays).
    pub async fn clear_status_message(&self) -> Result<(), OcsError> {
        self.status_call(Method::DELETE, "/user_status/message", None).await
    }

    /// Tells the server the person is there: `kind` is [`StatusKind::Online`] or
    /// [`StatusKind::Away`], anything else is [`OcsError::Other`] without asking.
    pub async fn heartbeat(&self, kind: StatusKind) -> Result<(), OcsError> {
        if !matches!(kind, StatusKind::Online | StatusKind::Away) {
            return Err(OcsError::Other("a heartbeat is only online or away".into()));
        }
        self.status_call(Method::PUT, "/heartbeat", Some(json!({ "status": kind.as_str() }))).await
    }

    async fn status_call(&self, method: Method, path: &str, body: Option<Value>) -> Result<(), OcsError> {
        let url = self.endpoint::<&str>(&format!("{BASE}{path}"), &[], &[])?;
        self.exchange(method, url, body.as_ref(), None).await?.success().map(|_| ())
    }
}

/// One status; `None` without a user id.
pub(crate) fn read_status(entry: &Value) -> Option<UserStatus> {
    let line = |key: &str| {
        let text = entry.get(key).and_then(Value::as_str).map(|text| rich::line(text, &Value::Null));
        text.filter(|text| !text.is_empty())
    };
    let user_id = line("userId")?;
    let clear_at = entry.get("clearAt").and_then(Value::as_i64).filter(|at| *at > 0);
    Some(UserStatus {
        user_id,
        kind: StatusKind::parse(entry.get("status").and_then(Value::as_str).unwrap_or_default()),
        message: line("message"),
        icon: line("icon"),
        clear_at: clear_at.and_then(|at| DateTime::from_timestamp(at, 0)),
    })
}

#[cfg(test)]
mod tests {
    use httpmock::Method::{DELETE, GET, PUT};
    use httpmock::MockServer;

    use super::*;
    use crate::ocs::envelope;
    use crate::ocs::tests::{AUTHORIZATION, account};

    const FIXTURE: &str = include_str!("../tests/fixtures/user_statuses.json");

    fn ocs(data: Value) -> String {
        json!({"ocs": {"meta": {"status": "ok", "statuscode": 200}, "data": data}}).to_string()
    }

    fn fixture() -> Vec<UserStatus> {
        let data = envelope(serde_json::from_str(FIXTURE).expect("fixture is JSON")).expect("an envelope");
        data.as_array().expect("a list").iter().filter_map(read_status).collect()
    }

    #[test]
    fn reads_statuses_with_messages_icons_and_their_end() {
        let statuses = fixture();
        assert_eq!(statuses.len(), 4, "the entry without a user id is left out");
        assert_eq!(
            statuses[0],
            UserStatus {
                user_id: "bert".into(),
                kind: StatusKind::Away,
                message: Some("In a meeting until noon".into()),
                icon: Some("📅".into()),
                clear_at: DateTime::from_timestamp(1_791_885_600, 0),
            }
        );
        assert_eq!((statuses[1].kind, statuses[1].message.as_deref()), (StatusKind::Dnd, Some("Focus time")));
        assert_eq!(
            (statuses[2].kind, statuses[2].message.as_deref(), statuses[2].icon.as_deref()),
            (StatusKind::Online, None, None)
        );
        assert_eq!(statuses[3].kind, StatusKind::Offline, "an unknown word is offline");
    }

    #[test]
    fn status_words_go_both_ways() {
        for kind in [
            StatusKind::Online,
            StatusKind::Away,
            StatusKind::Busy,
            StatusKind::Dnd,
            StatusKind::Invisible,
            StatusKind::Offline,
        ] {
            assert_eq!(StatusKind::parse(kind.as_str()), kind);
        }
        assert_eq!(StatusKind::parse("ONLINE"), StatusKind::Offline);
    }

    #[tokio::test]
    async fn lists_statuses_page_by_page() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!("{BASE}/statuses"))
                    .query_param("limit", "100")
                    .query_param("offset", "200")
                    .header("authorization", AUTHORIZATION)
                    .header("ocs-apirequest", "true");
                then.status(200).body(FIXTURE);
            })
            .await;
        assert_eq!(account(&server).statuses(100, 200).await.expect("statuses").len(), 4);
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn one_status_with_its_id_as_one_segment_and_none_is_none() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{BASE}/statuses/bert")).header("authorization", AUTHORIZATION);
                then.status(200).body(ocs(
                    json!({"userId": "bert", "status": "busy", "message": null, "icon": null, "clearAt": null}),
                ));
            })
            .await;
        let odd = server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{BASE}/statuses/a%3Fb%2Fc"));
                then.status(404).body(ocs(json!([])));
            })
            .await;
        let account = account(&server);
        let bert = account.status_of("bert").await.expect("an answer").expect("a status");
        assert_eq!(bert.kind, StatusKind::Busy);
        assert_eq!(account.status_of("a?b/c").await, Ok(None));
        assert_eq!(odd.calls_async().await, 1, "the id stays one segment");
        assert!(account.status_of("..").await.is_err(), "no id leaves its segment");
    }

    #[tokio::test]
    async fn own_status_and_none_yet() {
        let server = MockServer::start_async().await;
        let own = json!({"userId": "anna", "status": "invisible", "message": "Back on Monday", "icon": "🌴",
            "clearAt": null, "messageId": null, "messageIsPredefined": false, "statusIsUserDefined": true});
        server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{BASE}/user_status")).header("authorization", AUTHORIZATION);
                then.status(200).body(ocs(own));
            })
            .await;
        let status = account(&server).own_status().await.expect("an answer").expect("a status");
        assert_eq!((status.kind, status.message.as_deref()), (StatusKind::Invisible, Some("Back on Monday")));
        let none = MockServer::start_async().await;
        none.mock_async(|when, then| {
            when.method(GET).path(format!("{BASE}/user_status"));
            then.status(404).body(ocs(json!([])));
        })
        .await;
        assert_eq!(account(&none).own_status().await, Ok(None));
    }

    #[tokio::test]
    async fn sets_the_status_and_its_message() {
        let server = MockServer::start_async().await;
        let status = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!("{BASE}/user_status/status"))
                    .header("authorization", AUTHORIZATION)
                    .json_body(json!({"statusType": "dnd"}));
                then.status(200).body(ocs(json!({})));
            })
            .await;
        let message = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!("{BASE}/user_status/message/custom"))
                    .json_body(json!({"statusIcon": "🎧", "message": "Focus time", "clearAt": 1_791_885_600}));
                then.status(200).body(ocs(json!({})));
            })
            .await;
        let clear = server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{BASE}/user_status/message"));
                then.status(200).body(ocs(json!([])));
            })
            .await;
        let account = account(&server);
        assert_eq!(account.set_status(StatusKind::Dnd).await, Ok(()));
        let until = DateTime::from_timestamp(1_791_885_600, 0);
        assert_eq!(account.set_status_message(Some("🎧"), Some("Focus time"), until).await, Ok(()));
        assert_eq!(account.clear_status_message().await, Ok(()));
        let calls = (status.calls_async().await, message.calls_async().await, clear.calls_async().await);
        assert_eq!(calls, (1, 1, 1));
    }

    #[tokio::test]
    async fn a_message_without_icon_or_end_sends_nulls_and_a_refusal_is_an_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!("{BASE}/user_status/message/custom"))
                    .json_body(json!({"statusIcon": null, "message": "Lunch", "clearAt": null}));
                then.status(400).body(ocs(json!([])));
            })
            .await;
        let refused = account(&server).set_status_message(None, Some("Lunch"), None).await;
        assert!(matches!(refused, Err(OcsError::Other(_))), "{refused:?}");
    }

    #[tokio::test]
    async fn heartbeat_is_online_or_away_only() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(format!("{BASE}/heartbeat"))
                    .header("authorization", AUTHORIZATION)
                    .json_body(json!({"status": "away"}));
                then.status(204);
            })
            .await;
        let account = account(&server);
        assert_eq!(account.heartbeat(StatusKind::Away).await, Ok(()));
        assert!(account.heartbeat(StatusKind::Dnd).await.is_err());
        assert_eq!(mock.calls_async().await, 1, "only the allowed one went out");
    }

    #[tokio::test]
    async fn without_the_app_it_is_missing() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{BASE}/statuses"));
                then.status(404).body(ocs(json!([])));
            })
            .await;
        assert_eq!(account(&server).statuses(10, 0).await, Err(OcsError::MissingApp));
    }
}
