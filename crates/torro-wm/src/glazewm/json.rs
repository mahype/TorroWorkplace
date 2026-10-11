//! Reading GlazeWM's JSON messages.
//!
//! Task X.7 in `docs/tasks/plattformen.md`. The shapes follow the serde types of GlazeWM v3.10.1
//! (`packages/wm-common/src/ipc.rs`, `wm_event.rs`, `dtos/*.rs` in glzr-io/glazewm): every frame
//! is a `ServerMessage` tagged by `messageType`; containers are tagged by `type` and nest
//! monitor → workspace → split → window. Fixtures live in `tests/fixtures/glazewm-*.json`.
//!
//! Only the fields we use are declared; serde ignores the rest, so new fields in a later
//! GlazeWM do not break parsing.

use serde::Deserialize;
use serde_json::Value;

use crate::{Monitor, Rect, Window, WindowId, WmError, WorkspaceId};

/// One frame from GlazeWM's WebSocket, as far as we read it.
#[derive(Debug, Deserialize)]
#[serde(tag = "messageType", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ServerMessage {
    /// The answer to a message we sent; `client_message` repeats it verbatim.
    ClientResponse { client_message: String, data: Option<Value>, error: Option<String>, success: bool },
    /// An event of a subscription made with `sub --events …`.
    EventSubscription { data: Option<Value>, error: Option<String>, success: bool },
}

/// Reads one WebSocket text frame.
pub fn parse_message(text: &str) -> Result<ServerMessage, WmError> {
    serde_json::from_str(text).map_err(|err| protocol("message", &err))
}

/// The `data` of a successful client response. A refused message becomes an error: `NotFound`
/// when GlazeWM did not know the container id, `Protocol` with GlazeWM's text otherwise.
pub fn response_data(data: Option<Value>, error: Option<String>, success: bool) -> Result<Value, WmError> {
    if success {
        return Ok(data.unwrap_or(Value::Null));
    }
    let error = error.unwrap_or_else(|| "GlazeWM refused the message without a reason".into());
    // GlazeWM's text for an unknown `--id` (wm.rs, `process_commands`).
    if error.starts_with("No container found") { Err(WmError::NotFound(error)) } else { Err(WmError::Protocol(error)) }
}

/// A workspace from `query workspaces`, reduced to what the backend needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub name: WorkspaceId,
    /// The workspace holds the focus (it or a descendant is focused).
    pub has_focus: bool,
    /// Container ids of all windows in it, also those in split containers.
    pub windows: Vec<WindowId>,
}

/// The workspaces in the `data` of a `query workspaces` answer.
pub fn parse_workspaces(data: Value) -> Result<Vec<WorkspaceInfo>, WmError> {
    let data: WorkspacesData = serde_json::from_value(data).map_err(|err| protocol("workspaces", &err))?;
    Ok(data.workspaces.into_iter().filter_map(RawContainer::into_workspace).map(WorkspaceInfo::from).collect())
}

/// The focused workspace among `workspaces`.
pub fn focused_workspace(workspaces: &[WorkspaceInfo]) -> Result<WorkspaceId, WmError> {
    workspaces
        .iter()
        .find(|workspace| workspace.has_focus)
        .map(|workspace| workspace.name.clone())
        .ok_or_else(|| WmError::Protocol("workspaces: none has focus".into()))
}

/// The workspace holding the window `id`, if any.
pub fn workspace_of(workspaces: &[WorkspaceInfo], id: &WindowId) -> Option<WorkspaceId> {
    workspaces.iter().find(|workspace| workspace.windows.contains(id)).map(|workspace| workspace.name.clone())
}

/// The monitors in the `data` of a `query monitors` answer, each with the workspace it shows.
pub fn parse_monitors(data: Value) -> Result<Vec<Monitor>, WmError> {
    Ok(monitor_tree(data)?.into_iter().map(Monitor::from).collect())
}

/// The normal windows in the `data` of a `query monitors` answer: every window of every
/// workspace except minimized ones, with workspace name and monitor name filled in. Windows on
/// workspaces that are not shown count as well; GlazeWM merely hides them.
pub fn parse_windows(data: Value) -> Result<Vec<Window>, WmError> {
    let mut windows = Vec::new();
    for monitor in monitor_tree(data)? {
        for workspace in monitor.children.into_iter().filter_map(RawContainer::into_workspace) {
            let name = WorkspaceId(workspace.name);
            let mut raw = Vec::new();
            collect_windows(workspace.children, &mut raw);
            windows.extend(
                raw.into_iter()
                    .filter(|window| window.state.kind != "minimized")
                    .map(|window| window.into_window(name.clone(), monitor.device_name.clone())),
            );
        }
    }
    Ok(windows)
}

fn monitor_tree(data: Value) -> Result<Vec<RawMonitor>, WmError> {
    let data: MonitorsData = serde_json::from_value(data).map_err(|err| protocol("monitors", &err))?;
    Ok(data
        .monitors
        .into_iter()
        .filter_map(|container| match container {
            RawContainer::Monitor(monitor) => Some(monitor),
            _ => None,
        })
        .collect())
}

/// Appends every window below `children` to `out`, depth first, in GlazeWM's order.
fn collect_windows(children: Vec<RawContainer>, out: &mut Vec<RawWindow>) {
    for child in children {
        match child {
            RawContainer::Window(window) => out.push(window),
            RawContainer::Split(split) => collect_windows(split.children, out),
            RawContainer::Workspace(workspace) => collect_windows(workspace.children, out),
            RawContainer::Monitor(monitor) => collect_windows(monitor.children, out),
            RawContainer::Other => {}
        }
    }
}

fn protocol(what: &str, err: &serde_json::Error) -> WmError {
    WmError::Protocol(format!("{what}: {err}"))
}

#[derive(Debug, Deserialize)]
struct MonitorsData {
    monitors: Vec<RawContainer>,
}

#[derive(Debug, Deserialize)]
struct WorkspacesData {
    workspaces: Vec<RawContainer>,
}

/// GlazeWM's `ContainerDto`. The root container never shows up in our answers; it and any
/// container type a later version adds land in `Other`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum RawContainer {
    Monitor(RawMonitor),
    Workspace(RawWorkspace),
    Split(RawSplit),
    Window(RawWindow),
    #[serde(other)]
    Other,
}

impl RawContainer {
    fn into_workspace(self) -> Option<RawWorkspace> {
        match self {
            RawContainer::Workspace(workspace) => Some(workspace),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawMonitor {
    #[serde(default)]
    children: Vec<RawContainer>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    #[serde(default = "default_scale")]
    scale_factor: f32,
    /// `\\.\DISPLAY1` on Windows.
    device_name: String,
}

fn default_scale() -> f32 {
    1.0
}

impl From<RawMonitor> for Monitor {
    fn from(monitor: RawMonitor) -> Monitor {
        // A monitor shows exactly one workspace at a time; GlazeWM marks it `isDisplayed`.
        let active_workspace = monitor
            .children
            .iter()
            .find_map(|child| match child {
                RawContainer::Workspace(workspace) if workspace.is_displayed => Some(workspace.name.clone()),
                _ => None,
            })
            .map(WorkspaceId);
        Monitor {
            name: monitor.device_name,
            rect: Rect { x: monitor.x, y: monitor.y, width: monitor.width, height: monitor.height },
            scale: monitor.scale_factor,
            active_workspace,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawWorkspace {
    /// The name from the user config (`"1"`), which `move --workspace` takes; not `displayName`.
    name: String,
    #[serde(default)]
    children: Vec<RawContainer>,
    #[serde(default)]
    has_focus: bool,
    #[serde(default)]
    is_displayed: bool,
}

impl From<RawWorkspace> for WorkspaceInfo {
    fn from(workspace: RawWorkspace) -> WorkspaceInfo {
        let mut windows = Vec::new();
        collect_windows(workspace.children, &mut windows);
        WorkspaceInfo {
            name: WorkspaceId(workspace.name),
            has_focus: workspace.has_focus,
            windows: windows.into_iter().map(|window| WindowId(window.id)).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawSplit {
    #[serde(default)]
    children: Vec<RawContainer>,
}

/// GlazeWM's `WindowDto`. It carries no process id and no workspace: the workspace follows from
/// the tree, the process only from the window handle, which we cannot query here.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawWindow {
    /// Container id, a UUID; what `--id` and `--container-id` take.
    pub(crate) id: String,
    #[serde(default)]
    title: String,
    /// The executable's file name without extension (`firefox`), GlazeWM's `process_name`.
    #[serde(default)]
    pub(crate) process_name: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    state: RawWindowState,
}

impl RawWindow {
    fn into_window(self, workspace: WorkspaceId, monitor: String) -> Window {
        Window {
            id: WindowId(self.id),
            class: self.process_name,
            title: self.title,
            pid: None,
            workspace,
            monitor: Some(monitor),
            rect: Rect { x: self.x, y: self.y, width: self.width, height: self.height },
            floating: self.state.kind == "floating",
            fullscreen: self.state.kind == "fullscreen",
        }
    }
}

/// `WindowState`, tagged by `type`: `tiling`, `floating`, `fullscreen`, `minimized`. Kept as
/// text so a new state does not make the whole answer unreadable.
#[derive(Debug, Deserialize)]
struct RawWindowState {
    #[serde(rename = "type")]
    kind: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITORS: &str = include_str!("../../tests/fixtures/glazewm-monitors.json");
    const WORKSPACES: &str = include_str!("../../tests/fixtures/glazewm-workspaces.json");
    const COMMAND_OK: &str = include_str!("../../tests/fixtures/glazewm-command-ok.json");
    const COMMAND_ERROR: &str = include_str!("../../tests/fixtures/glazewm-command-error.json");

    /// The `data` of a fixture holding one successful client response.
    fn data_of(fixture: &str) -> Value {
        match parse_message(fixture).expect("fixture parses") {
            ServerMessage::ClientResponse { data, error, success, .. } => {
                response_data(data, error, success).expect("fixture is a success")
            }
            other => panic!("expected a client response, got {other:?}"),
        }
    }

    fn id(n: u8) -> WindowId {
        WindowId(format!("0f9e8d7c-6b5a-4938-8271-6a5b4c3d2e{n:02}"))
    }

    #[test]
    fn monitors_carry_name_rect_scale_and_shown_workspace() {
        let monitors = parse_monitors(data_of(MONITORS)).expect("monitors parse");
        assert_eq!(
            monitors,
            vec![
                Monitor {
                    name: r"\\.\DISPLAY1".into(),
                    rect: Rect { x: 0, y: 0, width: 2560, height: 1440 },
                    scale: 1.0,
                    active_workspace: Some(WorkspaceId("1".into())),
                },
                Monitor {
                    name: r"\\.\DISPLAY2".into(),
                    rect: Rect { x: 2560, y: 0, width: 1920, height: 1080 },
                    scale: 1.25,
                    active_workspace: Some(WorkspaceId("3".into())),
                },
            ]
        );
    }

    #[test]
    fn windows_come_from_all_workspaces_without_minimized_ones() {
        let windows = parse_windows(data_of(MONITORS)).expect("windows parse");
        let ids: Vec<&WindowId> = windows.iter().map(|w| &w.id).collect();
        // Notepad (07) is minimized and left out; Spotify (04) sits on a hidden workspace and stays.
        assert_eq!(ids, vec![&id(1), &id(2), &id(3), &id(4), &id(6), &id(5)]);
    }

    #[test]
    fn window_in_split_container_maps_onto_neutral_fields() {
        let windows = parse_windows(data_of(MONITORS)).expect("windows parse");
        let code = windows.iter().find(|w| w.id == id(3)).expect("Code window");
        assert_eq!(
            code,
            &Window {
                id: id(3),
                class: "Code".into(),
                title: "main.rs - TorroWorkplace - Visual Studio Code".into(),
                pid: None,
                workspace: WorkspaceId("1".into()),
                monitor: Some(r"\\.\DISPLAY1".into()),
                rect: Rect { x: 1280, y: 700, width: 1280, height: 700 },
                floating: false,
                fullscreen: false,
            }
        );
    }

    #[test]
    fn floating_window_and_second_monitor() {
        let windows = parse_windows(data_of(MONITORS)).expect("windows parse");
        let calculator = windows.iter().find(|w| w.id == id(5)).expect("Calculator window");
        assert!(calculator.floating);
        assert!(!calculator.fullscreen);
        assert_eq!(calculator.workspace, WorkspaceId("3".into()));
        assert_eq!(calculator.monitor.as_deref(), Some(r"\\.\DISPLAY2"));
    }

    #[test]
    fn workspaces_know_focus_and_their_windows() {
        let workspaces = parse_workspaces(data_of(WORKSPACES)).expect("workspaces parse");
        assert_eq!(workspaces.len(), 3);
        assert_eq!(focused_workspace(&workspaces), Ok(WorkspaceId("1".into())));
        assert_eq!(workspaces[0].windows, vec![id(1), id(2), id(3)]);
        assert_eq!(workspace_of(&workspaces, &id(4)), Some(WorkspaceId("2".into())));
        assert_eq!(workspace_of(&workspaces, &id(7)), Some(WorkspaceId("3".into())));
        assert_eq!(workspace_of(&workspaces, &WindowId("nope".into())), None);
    }

    #[test]
    fn no_focused_workspace_is_a_protocol_error() {
        let workspaces = vec![WorkspaceInfo { name: WorkspaceId("1".into()), has_focus: false, windows: vec![] }];
        assert!(matches!(focused_workspace(&workspaces), Err(WmError::Protocol(_))));
    }

    #[test]
    fn command_answer_is_success() {
        let data = data_of(COMMAND_OK);
        assert_eq!(data["subjectContainerId"], id(3).0.as_str());
    }

    #[test]
    fn refused_command_with_unknown_id_is_not_found() {
        let ServerMessage::ClientResponse { data, error, success, .. } =
            parse_message(COMMAND_ERROR).expect("fixture parses")
        else {
            panic!("expected a client response");
        };
        let err = response_data(data, error, success).expect_err("refused");
        assert!(matches!(err, WmError::NotFound(ref text) if text.contains("No container found")), "{err:?}");
    }

    #[test]
    fn other_refusals_are_protocol_errors() {
        let err = response_data(None, Some("Workspace 'x' not found.".into()), false).expect_err("refused");
        assert_eq!(err, WmError::Protocol("Workspace 'x' not found.".into()));
        let err = response_data(None, None, false).expect_err("refused");
        assert!(matches!(err, WmError::Protocol(_)));
    }

    #[test]
    fn garbage_is_a_protocol_error() {
        assert!(matches!(parse_message("not json"), Err(WmError::Protocol(_))));
        assert!(matches!(parse_message(r#"{"messageType":"other"}"#), Err(WmError::Protocol(_))));
        assert!(matches!(parse_monitors(Value::Null), Err(WmError::Protocol(_))));
        assert!(matches!(parse_workspaces(serde_json::json!({"workspaces": 3})), Err(WmError::Protocol(_))));
    }

    #[test]
    fn unknown_container_types_and_fields_are_ignored() {
        let data = serde_json::json!({ "workspaces": [
            { "type": "root", "id": "r" },
            { "type": "workspace", "name": "9", "hasFocus": true, "future": 1, "children": [
                { "type": "something_new" },
                { "type": "window", "id": "w", "x": 0, "y": 0, "width": 1, "height": 1,
                  "state": { "type": "tiling" }, "newField": true }
            ]}
        ]});
        let workspaces = parse_workspaces(data).expect("parses");
        assert_eq!(
            workspaces,
            vec![WorkspaceInfo { name: WorkspaceId("9".into()), has_focus: true, windows: vec![WindowId("w".into())] }]
        );
    }
}
