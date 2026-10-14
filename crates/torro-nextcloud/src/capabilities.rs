//! Which apps a Nextcloud offers, from `GET /ocs/v2.php/cloud/capabilities`: the notifications
//! app, Talk (`spreed`) and the push service (`notify_push`). Features are told by capabilities,
//! never by version numbers (skill `nextcloud`).
//!
//! - Notifications: `capabilities.notifications.ocs-endpoints`, a list of strings such as `list`,
//!   `get`, `delete`, `delete-all` (schema `Capabilities` in
//!   <https://github.com/nextcloud/notifications/blob/master/openapi.json>).
//! - Talk: `capabilities.spreed.features`, e.g. `chat-v2`, `chat-read-marker`
//!   (<https://github.com/nextcloud/spreed/blob/main/openapi.json>, schema `Capabilities`;
//!   <https://nextcloud-talk.readthedocs.io/en/latest/capabilities/>).
//! - Push: `capabilities.notify_push.endpoints.websocket`
//!   (<https://github.com/nextcloud/notify_push#capabilities>).
//! - Deck and Notes (task W.1, only whether they are there, for the first-run setup): an object
//!   under `capabilities.deck` and `capabilities.notes`
//!   (<https://deck.readthedocs.io/en/latest/API/#capabilities>,
//!   <https://github.com/nextcloud/notes/blob/main/docs/api/README.md#capabilities>).

use serde_json::Value;

use crate::ocs::{Nextcloud, OcsError, Reply};

/// What the server offers of what the inbox reads. PHP sends an empty map as `[]`; a part that
/// is missing or not of the documented shape counts as not there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// The notifications app, only when it can list (`ocs-endpoints` holds `list`).
    pub notifications: Option<NotificationsCapability>,
    /// Talk, when `spreed` names any features.
    pub talk: Option<TalkCapability>,
    /// The push service's WebSocket address (`wss://…/push/ws`) exactly as the server sends it.
    /// Read only: it is checked by nobody here, so it must never be connected to (or sent the app
    /// password) unless it lies on the account's own origin (rule 15, task E5.10).
    pub push: Option<String>,
    /// The Deck app (boards and cards) is enabled.
    pub deck: bool,
    /// The Notes app is enabled.
    pub notes: bool,
}

/// The notifications app of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationsCapability {
    /// Single notifications can be deleted (`ocs-endpoints` holds `delete`).
    pub delete: bool,
}

/// Talk on a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TalkCapability {
    /// A conversation can be marked read up to a message (feature `chat-read-marker`).
    pub read_marker: bool,
}

impl Nextcloud {
    /// The server's capabilities (`GET /ocs/v2.php/cloud/capabilities`). Errors as every OCS call
    /// ([`OcsError`]); an answer without `capabilities` is [`OcsError::Other`].
    pub async fn capabilities(&self) -> Result<Capabilities, OcsError> {
        match self.get("/ocs/v2.php/cloud/capabilities", None).await? {
            Reply::Data { data, .. } => read_capabilities(&data),
            Reply::Empty | Reply::NotModified => Err(OcsError::Other("Nextcloud sent no capabilities".into())),
        }
    }
}

/// The capabilities in the `data` of the capabilities answer (`{"version": …, "capabilities": …}`).
pub(crate) fn read_capabilities(data: &Value) -> Result<Capabilities, OcsError> {
    let capabilities = data
        .get("capabilities")
        .filter(|c| c.is_object() || c.as_array().is_some_and(Vec::is_empty))
        .ok_or_else(|| OcsError::Other("Nextcloud sent no capabilities".into()))?;
    let endpoints = strings(capabilities.pointer("/notifications/ocs-endpoints"));
    let notifications =
        endpoints.contains(&"list").then(|| NotificationsCapability { delete: endpoints.contains(&"delete") });
    let features = strings(capabilities.pointer("/spreed/features"));
    let talk = (!features.is_empty()).then(|| TalkCapability { read_marker: features.contains(&"chat-read-marker") });
    let push = capabilities
        .pointer("/notify_push/endpoints/websocket")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|address| !address.is_empty())
        .map(str::to_string);
    let app = |name: &str| capabilities.get(name).is_some_and(Value::is_object);
    Ok(Capabilities { notifications, talk, push, deck: app("deck"), notes: app("notes") })
}

/// The strings of a JSON list; nothing for anything else.
fn strings(value: Option<&Value>) -> Vec<&str> {
    value.and_then(Value::as_array).map(|list| list.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use serde_json::json;

    use super::*;
    use crate::ocs::envelope;
    use crate::ocs::tests::{AUTHORIZATION, account};

    const FIXTURE: &str = include_str!("../tests/fixtures/capabilities.json");

    fn fixture() -> Value {
        envelope(serde_json::from_str(FIXTURE).expect("fixture is JSON")).expect("an envelope")
    }

    #[test]
    fn reads_notifications_talk_and_push() {
        let capabilities = read_capabilities(&fixture()).expect("capabilities");
        assert_eq!(capabilities.notifications, Some(NotificationsCapability { delete: true }));
        assert_eq!(capabilities.talk, Some(TalkCapability { read_marker: true }));
        assert_eq!(capabilities.push.as_deref(), Some("wss://cloud.example.org/push/ws"));
    }

    #[test]
    fn missing_apps_are_none() {
        let bare = read_capabilities(&json!({"capabilities": {"core": {"pollinterval": 60}}})).expect("capabilities");
        assert_eq!(bare, Capabilities::default());
        let empty = read_capabilities(&json!({"capabilities": []})).expect("PHP's empty map");
        assert_eq!(empty, Capabilities::default());
        assert!(read_capabilities(&json!({"version": {}})).is_err());
    }

    #[test]
    fn deck_and_notes_are_there_when_they_name_themselves() {
        let data = json!({"capabilities": {
            "deck": {"version": "1.14.0", "canCreateBoards": true, "apiVersions": ["1.0", "1.1"]},
            "notes": {"api_version": ["1.3"], "version": "4.11.0"},
        }});
        let capabilities = read_capabilities(&data).expect("capabilities");
        assert!(capabilities.deck && capabilities.notes);
        assert!(!read_capabilities(&fixture()).expect("capabilities").deck, "the fixture's server has no Deck");
        let odd = read_capabilities(&json!({"capabilities": {"deck": true, "notes": []}})).expect("capabilities");
        assert!(!odd.deck && !odd.notes, "only an object counts");
    }

    #[test]
    fn notifications_that_cannot_list_are_none_and_old_talk_has_no_read_marker() {
        let data = json!({"capabilities": {
            "notifications": {"ocs-endpoints": ["get"]},
            "spreed": {"features": ["chat-v2"]},
            "notify_push": {"endpoints": {"websocket": ""}},
        }});
        let capabilities = read_capabilities(&data).expect("capabilities");
        assert_eq!(capabilities.notifications, None);
        assert_eq!(capabilities.talk, Some(TalkCapability { read_marker: false }));
        assert_eq!(capabilities.push, None);
    }

    #[tokio::test]
    async fn asks_the_capabilities_endpoint() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/ocs/v2.php/cloud/capabilities")
                    .header("authorization", AUTHORIZATION)
                    .header("ocs-apirequest", "true");
                then.status(200).body(FIXTURE);
            })
            .await;
        let capabilities = account(&server).capabilities().await.expect("capabilities");
        assert!(capabilities.notifications.is_some() && capabilities.talk.is_some());
    }

    #[tokio::test]
    async fn a_refusal_is_its_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/ocs/v2.php/cloud/capabilities");
                then.status(401);
            })
            .await;
        assert_eq!(account(&server).capabilities().await, Err(OcsError::Refused));
    }
}
