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
//! - Talk's settings (#643): `capabilities.spreed.config` with `chat.max-length`,
//!   `attachments.allowed`, `attachments.folder`, `attachments.conversation-subfolders` and
//!   `call.enabled` (<https://nextcloud-talk.readthedocs.io/en/latest/capabilities/>).
//! - User status (#643): `capabilities.user_status` with `enabled`, `supports_emoji`,
//!   `supports_busy` (schema `Capabilities` in
//!   <https://github.com/nextcloud/server/blob/master/apps/user_status/openapi.json>).
//! - Chunked upload (#643): `capabilities.files.chunked_upload.max_size` (schema `Capabilities` in
//!   <https://github.com/nextcloud/server/blob/master/apps/files/openapi.json>).
//! - Deck and Notes (task W.1, only whether they are there, for the first-run setup): an object
//!   under `capabilities.deck` and `capabilities.notes`
//!   (<https://deck.readthedocs.io/en/latest/API/#capabilities>,
//!   <https://github.com/nextcloud/notes/blob/main/docs/api/README.md#capabilities>).

use std::collections::BTreeSet;

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
    /// The user status app, when it names itself (`user_status`).
    pub user_status: Option<UserStatusCapability>,
    /// The largest piece of a chunked upload in bytes (`files.chunked_upload.max_size`), when the
    /// server names a positive one.
    pub chunked_upload_max_size: Option<i64>,
}

/// The notifications app of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationsCapability {
    /// Single notifications can be deleted (`ocs-endpoints` holds `delete`).
    pub delete: bool,
}

/// The longest chat message when the server does not say (Talk's limit before Nextcloud 16.0.1;
/// every supported server names its own, today 32 000).
pub const DEFAULT_MAX_LENGTH: usize = 1000;

/// Talk's attachment folder when the server does not say.
pub const DEFAULT_ATTACHMENTS_FOLDER: &str = "/Talk";

/// Talk on a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TalkCapability {
    /// A conversation can be marked read up to a message (feature `chat-read-marker`).
    pub read_marker: bool,
    /// Every feature `spreed.features` names (`chat-keep-notifications`, `edit-messages` …).
    pub features: BTreeSet<String>,
    /// The longest chat message in characters (`config.chat.max-length`, else
    /// [`DEFAULT_MAX_LENGTH`]).
    pub max_length: usize,
    /// The person may share files into conversations (`config.attachments.allowed`, `false`
    /// when missing).
    pub attachments_allowed: bool,
    /// The person's folder for shared files (`config.attachments.folder`, else
    /// [`DEFAULT_ATTACHMENTS_FOLDER`]); foreign text, a path inside their files, one line.
    pub attachments_folder: String,
    /// Files go into a folder per conversation and person below the attachment folder
    /// (`config.attachments.conversation-subfolders`).
    pub conversation_subfolders: bool,
    /// Calls are switched on on the server (`config.call.enabled`, `true` when missing).
    pub call_enabled: bool,
}

impl TalkCapability {
    /// Whether `spreed.features` names `feature`.
    pub fn has(&self, feature: &str) -> bool {
        self.features.contains(feature)
    }
}

/// The user status app of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserStatusCapability {
    /// The app is enabled (`enabled`).
    pub enabled: bool,
    /// A status can carry an emoji (`supports_emoji`).
    pub supports_emoji: bool,
    /// The status "busy" exists (`supports_busy`, newer servers only).
    pub supports_busy: bool,
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
    let talk = read_talk(capabilities);
    let push = capabilities
        .pointer("/notify_push/endpoints/websocket")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|address| !address.is_empty())
        .map(str::to_string);
    let app = |name: &str| capabilities.get(name).is_some_and(Value::is_object);
    let user_status = capabilities.get("user_status").filter(|c| c.is_object()).map(|status| {
        let flag = |key: &str| status.get(key).and_then(Value::as_bool).unwrap_or(false);
        UserStatusCapability {
            enabled: flag("enabled"),
            supports_emoji: flag("supports_emoji"),
            supports_busy: flag("supports_busy"),
        }
    });
    let chunked_upload_max_size =
        capabilities.pointer("/files/chunked_upload/max_size").and_then(Value::as_i64).filter(|size| *size > 0);
    Ok(Capabilities {
        notifications,
        talk,
        push,
        deck: app("deck"),
        notes: app("notes"),
        user_status,
        chunked_upload_max_size,
    })
}

/// Talk, when `spreed.features` names any feature.
fn read_talk(capabilities: &Value) -> Option<TalkCapability> {
    let features: BTreeSet<String> =
        strings(capabilities.pointer("/spreed/features")).into_iter().map(str::to_string).collect();
    if features.is_empty() {
        return None;
    }
    let config = |pointer: &str| capabilities.pointer(&format!("/spreed/config{pointer}"));
    let flag = |pointer: &str| config(pointer).and_then(Value::as_bool);
    let max_length = config("/chat/max-length")
        .and_then(Value::as_u64)
        .filter(|length| *length > 0)
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(DEFAULT_MAX_LENGTH);
    let attachments_folder = config("/attachments/folder")
        .and_then(Value::as_str)
        .map(|folder| crate::rich::line(folder, &Value::Null))
        .filter(|folder| !folder.is_empty())
        .unwrap_or_else(|| DEFAULT_ATTACHMENTS_FOLDER.to_string());
    Some(TalkCapability {
        read_marker: features.contains("chat-read-marker"),
        features,
        max_length,
        attachments_allowed: flag("/attachments/allowed").unwrap_or(false),
        attachments_folder,
        conversation_subfolders: flag("/attachments/conversation-subfolders").unwrap_or(false),
        call_enabled: flag("/call/enabled").unwrap_or(true),
    })
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
        let talk = capabilities.talk.as_ref().expect("Talk");
        assert!(talk.read_marker);
        assert!(talk.has("note-to-self") && talk.has("chat-v2") && !talk.has("chat-keep-notifications"));
        assert_eq!(talk.features.len(), 11);
        assert_eq!(talk.max_length, 32000);
        assert!(talk.attachments_allowed && talk.conversation_subfolders);
        assert_eq!(talk.attachments_folder, "/Chat files");
        assert!(!talk.call_enabled);
        assert_eq!(capabilities.push.as_deref(), Some("wss://cloud.example.org/push/ws"));
    }

    #[test]
    fn reads_user_status_and_the_chunk_size() {
        let capabilities = read_capabilities(&fixture()).expect("capabilities");
        let status = UserStatusCapability { enabled: true, supports_emoji: true, supports_busy: false };
        assert_eq!(capabilities.user_status, Some(status));
        assert_eq!(capabilities.chunked_upload_max_size, Some(104_857_600));
        let odd = json!({"capabilities": {"user_status": [], "files": {"chunked_upload": {"max_size": -1}}}});
        let odd = read_capabilities(&odd).expect("capabilities");
        assert_eq!((odd.user_status, odd.chunked_upload_max_size), (None, None));
    }

    #[test]
    fn talk_without_settings_has_the_defaults() {
        let data =
            json!({"capabilities": {"spreed": {"features": ["chat-v2"], "config": {"chat": {"max-length": 0}}}}});
        let talk = read_capabilities(&data).expect("capabilities").talk.expect("Talk");
        assert_eq!(talk.max_length, DEFAULT_MAX_LENGTH);
        assert_eq!(talk.attachments_folder, DEFAULT_ATTACHMENTS_FOLDER);
        assert!(!talk.attachments_allowed && !talk.conversation_subfolders && talk.call_enabled);
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
        let talk = capabilities.talk.expect("Talk");
        assert!(!talk.read_marker && talk.has("chat-v2"));
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
