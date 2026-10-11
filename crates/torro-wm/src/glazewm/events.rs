//! GlazeWM's event subscription.
//!
//! Task X.7 in `docs/tasks/plattformen.md`. After `sub --events …` GlazeWM sends one
//! `event_subscription` frame per event, the event tagged by `eventType` (GlazeWM v3.10.1,
//! `packages/wm-common/src/wm_event.rs`). Its window objects name no workspace, and focus events
//! do not say which workspace has focus, so for those events the reader asks `query workspaces`
//! on a second connection and maps the answer onto [`WmEvent`].
//!
//! GlazeWM has no event for a window that is moved while it does not have focus; only moves of
//! the focused window (`focused_container_moved`) become [`WmEvent::WindowMoved`].

use serde::Deserialize;

use super::json::{self, RawContainer, RawWindow, ServerMessage, WorkspaceInfo};
use super::socket;
use crate::{Events, WindowId, WmError, WmEvent, WorkspaceId};

/// The subscription message: the events [`parse_event`] reads.
pub const SUBSCRIBE: &str = "sub --events window_managed window_unmanaged focus_changed focused_container_moved";

/// Capacity of the channel between the reader task and the core; see the Hyprland backend.
const EVENT_QUEUE: usize = 256;

/// An event as GlazeWM reports it, before the workspace is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// A window became managed (opened, or shown again after GlazeWM ignored it).
    Managed {
        window: WindowId,
        class: String,
    },
    Unmanaged {
        window: WindowId,
    },
    /// The focused window moved, possibly to another workspace.
    Moved {
        window: WindowId,
    },
    /// Focus changed; the focused workspace may be another one now.
    Focus,
}

impl Trigger {
    /// Whether mapping this trigger needs the current workspaces.
    fn needs_workspaces(&self) -> bool {
        !matches!(self, Trigger::Unmanaged { .. })
    }
}

/// One frame of the subscription as a [`Trigger`]; `None` for other messages, events we do
/// not use and frames we cannot read.
pub fn parse_event(text: &str) -> Option<Trigger> {
    let ServerMessage::EventSubscription { data: Some(data), success: true, .. } = json::parse_message(text).ok()?
    else {
        return None;
    };
    match serde_json::from_value::<RawEvent>(data).ok()? {
        RawEvent::WindowManaged { managed_window } => {
            Some(Trigger::Managed { window: WindowId(managed_window.id), class: managed_window.process_name })
        }
        RawEvent::WindowUnmanaged { unmanaged_id } => Some(Trigger::Unmanaged { window: WindowId(unmanaged_id) }),
        RawEvent::FocusedContainerMoved { focused_container: RawContainer::Window(window) } => {
            Some(Trigger::Moved { window: WindowId(window.id) })
        }
        RawEvent::FocusChanged {} => Some(Trigger::Focus),
        RawEvent::FocusedContainerMoved { .. } | RawEvent::Other => None,
    }
}

/// Maps `trigger` onto a [`WmEvent`] with the current `workspaces`. `last_focused` is the
/// workspace reported focused before; a focus change within it gives no event, like Hyprland's
/// `workspacev2`. `None` when there is nothing to report or the window is gone already.
pub fn resolve(
    trigger: Trigger,
    workspaces: &[WorkspaceInfo],
    last_focused: &mut Option<WorkspaceId>,
) -> Option<WmEvent> {
    match trigger {
        Trigger::Managed { window, class } => {
            let workspace = json::workspace_of(workspaces, &window)?;
            Some(WmEvent::WindowOpened { window, class, workspace })
        }
        Trigger::Unmanaged { window } => Some(WmEvent::WindowClosed { window }),
        Trigger::Moved { window } => {
            let workspace = json::workspace_of(workspaces, &window)?;
            Some(WmEvent::WindowMoved { window, workspace })
        }
        Trigger::Focus => {
            let workspace = json::focused_workspace(workspaces).ok()?;
            if last_focused.as_ref() == Some(&workspace) {
                return None;
            }
            *last_focused = Some(workspace.clone());
            Some(WmEvent::WorkspaceFocused { workspace })
        }
    }
}

/// Subscribes on `port` and forwards mapped events into the returned channel from a background
/// task until GlazeWM closes the connection or the receiver is dropped.
pub async fn subscribe(port: u16) -> Result<Events, WmError> {
    let mut connection = socket::subscribe(port, SUBSCRIBE).await?;
    let (tx, rx) = tokio::sync::mpsc::channel(EVENT_QUEUE);
    tokio::spawn(async move {
        let mut last_focused = None;
        while let Some(text) = socket::next_text(&mut connection).await {
            let Some(trigger) = parse_event(&text) else {
                continue;
            };
            let workspaces = if trigger.needs_workspaces() {
                // A failed lookup drops this one event, not the subscription.
                match current_workspaces(port).await {
                    Ok(workspaces) => workspaces,
                    Err(_) => continue,
                }
            } else {
                Vec::new()
            };
            let Some(event) = resolve(trigger, &workspaces, &mut last_focused) else {
                continue;
            };
            if tx.send(event).await.is_err() {
                break;
            }
        }
    });
    Ok(rx)
}

async fn current_workspaces(port: u16) -> Result<Vec<WorkspaceInfo>, WmError> {
    json::parse_workspaces(socket::request(port, "query workspaces").await?)
}

/// GlazeWM's `WmEvent`, only the events in [`SUBSCRIBE`] and only the fields we use.
#[derive(Debug, Deserialize)]
#[serde(tag = "eventType", rename_all = "snake_case", rename_all_fields = "camelCase")]
enum RawEvent {
    WindowManaged {
        managed_window: RawWindow,
    },
    WindowUnmanaged {
        unmanaged_id: String,
    },
    FocusedContainerMoved {
        focused_container: RawContainer,
    },
    FocusChanged {},
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glazewm::socket::tests::{FakeGlazeWm, echo};

    const EVENTS: &str = include_str!("../../tests/fixtures/glazewm-events.json");
    const WORKSPACES: &str = include_str!("../../tests/fixtures/glazewm-workspaces.json");

    fn id(n: u8) -> WindowId {
        WindowId(format!("0f9e8d7c-6b5a-4938-8271-6a5b4c3d2e{n:02}"))
    }

    fn workspace(name: &str) -> WorkspaceId {
        WorkspaceId(name.to_owned())
    }

    /// The frames of the events fixture, each as the text GlazeWM sends.
    fn event_frames() -> Vec<String> {
        let frames: Vec<serde_json::Value> = serde_json::from_str(EVENTS).expect("fixture is a JSON array");
        frames.iter().map(serde_json::Value::to_string).collect()
    }

    fn workspaces() -> Vec<WorkspaceInfo> {
        let ServerMessage::ClientResponse { data, error, success, .. } =
            json::parse_message(WORKSPACES).expect("fixture parses")
        else {
            panic!("expected a client response");
        };
        json::parse_workspaces(json::response_data(data, error, success).expect("success")).expect("workspaces")
    }

    #[test]
    fn frames_become_triggers() {
        let triggers: Vec<Option<Trigger>> = event_frames().iter().map(|frame| parse_event(frame)).collect();
        assert_eq!(
            triggers,
            vec![
                None, // the answer to `sub`, not an event
                Some(Trigger::Managed { window: id(3), class: "Code".into() }),
                Some(Trigger::Focus),
                Some(Trigger::Focus),
                Some(Trigger::Moved { window: id(4) }),
                None, // tiling_direction_changed is not used
                Some(Trigger::Unmanaged { window: id(2) }),
            ]
        );
    }

    #[test]
    fn garbage_and_failed_events_are_ignored() {
        assert_eq!(parse_event("not json"), None);
        assert_eq!(
            parse_event(
                r#"{"messageType":"event_subscription","data":null,"error":"x","subscriptionId":"s","success":false}"#
            ),
            None
        );
        assert_eq!(
            parse_event(r#"{"messageType":"event_subscription","data":{"eventType":"window_managed"},"success":true}"#),
            None
        );
        // A focused workspace moved (not a window): nothing to report.
        assert_eq!(
            parse_event(
                r#"{"messageType":"event_subscription","success":true,
                    "data":{"eventType":"focused_container_moved","focusedContainer":{"type":"workspace","name":"1"}}}"#
            ),
            None
        );
    }

    #[test]
    fn resolve_fills_in_workspaces() {
        let workspaces = workspaces();
        let mut last = None;
        assert_eq!(
            resolve(Trigger::Managed { window: id(3), class: "Code".into() }, &workspaces, &mut last),
            Some(WmEvent::WindowOpened { window: id(3), class: "Code".into(), workspace: workspace("1") })
        );
        assert_eq!(
            resolve(Trigger::Moved { window: id(4) }, &workspaces, &mut last),
            Some(WmEvent::WindowMoved { window: id(4), workspace: workspace("2") })
        );
        assert_eq!(
            resolve(Trigger::Unmanaged { window: id(9) }, &[], &mut last),
            Some(WmEvent::WindowClosed { window: id(9) })
        );
        // Gone before we looked: no event.
        assert_eq!(resolve(Trigger::Managed { window: id(9), class: "x".into() }, &workspaces, &mut last), None);
    }

    #[test]
    fn focus_reports_only_workspace_changes() {
        let workspaces = workspaces();
        let mut last = None;
        assert_eq!(
            resolve(Trigger::Focus, &workspaces, &mut last),
            Some(WmEvent::WorkspaceFocused { workspace: workspace("1") })
        );
        assert_eq!(resolve(Trigger::Focus, &workspaces, &mut last), None);
        let mut moved = workspaces.clone();
        for info in &mut moved {
            info.has_focus = info.name == workspace("3");
        }
        assert_eq!(
            resolve(Trigger::Focus, &moved, &mut last),
            Some(WmEvent::WorkspaceFocused { workspace: workspace("3") })
        );
    }

    #[tokio::test]
    async fn subscription_forwards_mapped_events() {
        let fake = FakeGlazeWm::start(|message| {
            if message.starts_with("sub ") {
                let mut frames = event_frames();
                frames[0] = echo(&frames[0], message);
                frames
            } else {
                vec![echo(WORKSPACES, message)]
            }
        })
        .await;

        let mut events = subscribe(fake.port).await.expect("subscribe to fake GlazeWM");
        let mut received = Vec::new();
        while let Some(event) =
            tokio::time::timeout(std::time::Duration::from_secs(5), events.recv()).await.expect("event within timeout")
        {
            received.push(event);
        }

        assert_eq!(
            received,
            vec![
                WmEvent::WindowOpened { window: id(3), class: "Code".into(), workspace: workspace("1") },
                WmEvent::WorkspaceFocused { workspace: workspace("1") },
                WmEvent::WindowMoved { window: id(4), workspace: workspace("2") },
                WmEvent::WindowClosed { window: id(2) },
            ]
        );
        let received = fake.received();
        assert_eq!(received.first().map(String::as_str), Some(SUBSCRIBE));
        // One lookup each for the opened window, both focus changes and the move.
        assert_eq!(received.iter().filter(|m| *m == "query workspaces").count(), 4);
    }

    #[tokio::test]
    async fn refused_subscription_is_an_error() {
        let fake = FakeGlazeWm::start(|message| {
            vec![
                serde_json::json!({
                    "messageType": "client_response", "clientMessage": message,
                    "data": null, "error": "invalid value 'x' for '--events'", "success": false
                })
                .to_string(),
            ]
        })
        .await;
        let err = subscribe(fake.port).await.expect_err("refused");
        assert!(matches!(err, WmError::Protocol(_)), "{err:?}");
    }
}
