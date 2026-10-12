//! Reading AeroSpace's JSON answers (`list-windows`, `list-workspaces`, `list-monitors`).
//!
//! Task X.6 in `docs/tasks/plattformen.md`. Fixtures live in `tests/fixtures/aerospace-*.json`.
//!
//! `--json` alone only prints a few fields; `--format` with the variables below selects what
//! goes into each JSON object. AeroSpace knows no window or monitor geometry, so rectangles
//! come from [`super::geometry`] and are joined in here.

use serde::Deserialize;

use super::geometry::Geometry;
use crate::{Monitor, Rect, SplitDirection, Window, WindowId, WmError, WorkspaceId};

/// `--format` for `list-windows --json`: every field [`RawWindow`] reads.
pub const WINDOW_FORMAT: &str = "%{window-id}%{app-bundle-id}%{app-name}%{app-pid}%{window-title}%{workspace}\
%{monitor-name}%{window-is-fullscreen}%{window-parent-container-layout}";

/// `--format` for `list-workspaces --json`: every field [`RawWorkspace`] reads.
pub const WORKSPACE_FORMAT: &str = "%{workspace}%{workspace-is-visible}%{monitor-id}";

/// `--format` for `list-monitors --json`: every field [`RawMonitor`] reads.
pub const MONITOR_FORMAT: &str = "%{monitor-id}%{monitor-name}%{monitor-appkit-nsscreen-screens-id}";

/// One entry of `list-windows --json --format WINDOW_FORMAT`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RawWindow {
    /// The macOS window number; the same number CoreGraphics uses.
    pub window_id: u32,
    #[serde(default)]
    pub app_bundle_id: String,
    #[serde(default)]
    pub app_name: String,
    #[serde(default)]
    pub app_pid: Option<i64>,
    #[serde(default)]
    pub window_title: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub monitor_name: Option<String>,
    #[serde(default)]
    pub window_is_fullscreen: bool,
    /// `h_tiles`, `v_tiles`, `h_accordion`, `v_accordion`, `floating`, or for windows AeroSpace
    /// does not lay out: `macos_native_minimized`, `macos_native_fullscreen`,
    /// `macos_native_window_of_hidden_app`; `NULL-WINDOW-LAYOUT` for a window without parent.
    #[serde(default)]
    pub window_parent_container_layout: String,
}

impl RawWindow {
    /// Normal windows: on a workspace and laid out by AeroSpace. Minimized windows, windows of
    /// hidden apps, windows in macOS fullscreen (each on its own space) and windows without a
    /// parent are left out, like hidden and special windows in the Hyprland backend.
    pub fn is_normal(&self) -> bool {
        let layout = self.window_parent_container_layout.as_str();
        !self.workspace.is_empty() && !layout.starts_with("macos_native") && !layout.starts_with("NULL-")
    }

    pub fn id(&self) -> WindowId {
        WindowId(self.window_id.to_string())
    }

    /// The bundle id; apps without one (rare, e.g. bare executables) fall back to the app name.
    /// AeroSpace prints `NULL-APP-BUNDLE-ID` for those.
    pub fn class(&self) -> String {
        let id = self.app_bundle_id.as_str();
        if id.is_empty() || id.starts_with("NULL-") { self.app_name.clone() } else { id.to_owned() }
    }

    pub fn workspace(&self) -> WorkspaceId {
        WorkspaceId(self.workspace.clone())
    }

    /// The neutral window, with the rectangle from `geometry` (zero when it has none).
    pub fn to_window(&self, geometry: &Geometry) -> Window {
        Window {
            id: self.id(),
            class: self.class(),
            title: self.window_title.clone(),
            pid: self.app_pid.and_then(|pid| u32::try_from(pid).ok()).filter(|&pid| pid > 0),
            workspace: self.workspace(),
            monitor: self.monitor_name.clone().filter(|name| !name.is_empty()),
            rect: geometry.windows.get(&self.window_id).copied().unwrap_or_default(),
            floating: self.window_parent_container_layout == "floating",
            fullscreen: self.window_is_fullscreen,
        }
    }
}

/// One entry of `list-workspaces --json --format WORKSPACE_FORMAT`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawWorkspace {
    workspace: String,
    #[serde(default)]
    workspace_is_visible: bool,
    #[serde(default)]
    monitor_id: Option<u32>,
}

/// One entry of `list-monitors --json --format MONITOR_FORMAT`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawMonitor {
    monitor_id: u32,
    #[serde(default)]
    monitor_name: String,
    /// 1-based index into `NSScreen.screens`, the order [`Geometry::screens`] has.
    #[serde(default)]
    monitor_appkit_nsscreen_screens_id: Option<usize>,
}

/// All entries of a `list-windows` answer, normal or not.
pub fn parse_raw_windows(json: &str) -> Result<Vec<RawWindow>, WmError> {
    serde_json::from_str(json).map_err(|err| protocol("list-windows", &err))
}

/// The normal windows (see [`RawWindow::is_normal`]) of a `list-windows` answer, with
/// rectangles from `geometry`, only those on `workspace` if given.
pub fn parse_windows(json: &str, geometry: &Geometry, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>, WmError> {
    Ok(parse_raw_windows(json)?
        .iter()
        .filter(|raw| raw.is_normal())
        .map(|raw| raw.to_window(geometry))
        .filter(|window| workspace.is_none_or(|workspace| &window.workspace == workspace))
        .collect())
}

/// The workspace in a `list-workspaces --focused --json` answer.
pub fn parse_focused_workspace(json: &str) -> Result<WorkspaceId, WmError> {
    let workspaces: Vec<RawWorkspace> = serde_json::from_str(json).map_err(|err| protocol("list-workspaces", &err))?;
    workspaces
        .into_iter()
        .map(|raw| raw.workspace)
        .find(|name| !name.is_empty())
        .map(WorkspaceId)
        .ok_or_else(|| WmError::Protocol("list-workspaces: no focused workspace".into()))
}

/// The monitors of a `list-monitors` answer, each with the visible workspace from a
/// `list-workspaces --all` answer and rectangle and scale from `geometry` (zero and `1.0`
/// when it has no screen for the monitor).
pub fn parse_monitors(monitors: &str, workspaces: &str, geometry: &Geometry) -> Result<Vec<Monitor>, WmError> {
    let monitors: Vec<RawMonitor> = serde_json::from_str(monitors).map_err(|err| protocol("list-monitors", &err))?;
    let workspaces: Vec<RawWorkspace> =
        serde_json::from_str(workspaces).map_err(|err| protocol("list-workspaces", &err))?;
    Ok(monitors
        .into_iter()
        .map(|monitor| {
            let screen = monitor
                .monitor_appkit_nsscreen_screens_id
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| geometry.screens.get(index));
            let active = workspaces
                .iter()
                .find(|ws| ws.workspace_is_visible && ws.monitor_id == Some(monitor.monitor_id))
                .map(|ws| WorkspaceId(ws.workspace.clone()));
            Monitor {
                name: monitor.monitor_name,
                rect: screen.map(|screen| screen.rect).unwrap_or(Rect::default()),
                scale: screen.map_or(1.0, |screen| screen.scale),
                active_workspace: active,
            }
        })
        .collect())
}

/// Which way the container holding a window divides its space: `h_*` left-right, `v_*`
/// top-bottom; `None` for floating and not laid out windows.
pub fn orientation(layout: &str) -> Option<SplitDirection> {
    if layout.starts_with("h_") {
        Some(SplitDirection::LeftRight)
    } else if layout.starts_with("v_") {
        Some(SplitDirection::TopBottom)
    } else {
        None
    }
}

fn protocol(what: &str, err: &serde_json::Error) -> WmError {
    WmError::Protocol(format!("{what}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS: &str = include_str!("../../tests/fixtures/aerospace-windows.json");
    const WORKSPACES: &str = include_str!("../../tests/fixtures/aerospace-workspaces.json");
    const FOCUSED: &str = include_str!("../../tests/fixtures/aerospace-focused-workspace.json");
    const MONITORS: &str = include_str!("../../tests/fixtures/aerospace-monitors.json");
    const GEOMETRY: &str = include_str!("../../tests/fixtures/aerospace-geometry.json");

    fn geometry() -> Geometry {
        super::super::geometry::parse(GEOMETRY).expect("geometry fixture parses")
    }

    #[test]
    fn windows_map_to_neutral_windows_with_geometry() {
        let windows = parse_windows(WINDOWS, &geometry(), None).expect("fixture parses");
        // Seven entries; the minimized window and the one of a hidden app are left out.
        assert_eq!(windows.len(), 5);
        let terminal = &windows[1];
        assert_eq!(terminal.id, WindowId("102".into()));
        assert_eq!(terminal.class, "com.mitchellh.ghostty");
        assert_eq!(terminal.title, "Terminal");
        assert_eq!(terminal.pid, Some(1201));
        assert_eq!(terminal.workspace, WorkspaceId("1".into()));
        assert_eq!(terminal.monitor.as_deref(), Some("Built-in Retina Display"));
        assert_eq!(terminal.rect, Rect { x: 872, y: 41, width: 848, height: 530 });
        assert!(!terminal.floating && !terminal.fullscreen);
    }

    #[test]
    fn floating_fullscreen_and_missing_bundle_id_are_read() {
        let windows = parse_windows(WINDOWS, &geometry(), None).expect("fixture parses");
        let notes = windows.iter().find(|w| w.id.0 == "104").expect("floating window");
        assert!(notes.floating);
        let player = windows.iter().find(|w| w.id.0 == "105").expect("fullscreen window");
        assert!(player.fullscreen);
        assert_eq!(player.class, "Player", "no bundle id: the app name stands in");
    }

    #[test]
    fn workspace_filter_and_missing_geometry() {
        let empty = Geometry::default();
        let on_one = parse_windows(WINDOWS, &empty, Some(&WorkspaceId("1".into()))).expect("fixture parses");
        assert_eq!(on_one.iter().map(|w| w.id.0.as_str()).collect::<Vec<_>>(), ["101", "102", "103"]);
        assert!(on_one.iter().all(|w| w.rect == Rect::default()));
        assert!(parse_windows(WINDOWS, &empty, Some(&WorkspaceId("9".into()))).expect("parses").is_empty());
    }

    #[test]
    fn focused_workspace() {
        assert_eq!(parse_focused_workspace(FOCUSED), Ok(WorkspaceId("1".into())));
        assert!(matches!(parse_focused_workspace("[]"), Err(WmError::Protocol(_))));
        assert!(matches!(parse_focused_workspace("not json"), Err(WmError::Protocol(_))));
    }

    #[test]
    fn monitors_get_visible_workspace_and_screen_geometry() {
        let monitors = parse_monitors(MONITORS, WORKSPACES, &geometry()).expect("fixtures parse");
        assert_eq!(
            monitors,
            vec![
                Monitor {
                    name: "Built-in Retina Display".into(),
                    rect: Rect { x: 0, y: 0, width: 1728, height: 1117 },
                    scale: 2.0,
                    active_workspace: Some(WorkspaceId("1".into())),
                },
                Monitor {
                    name: "DELL U2723QE".into(),
                    rect: Rect { x: 1728, y: 0, width: 2560, height: 1440 },
                    scale: 1.0,
                    active_workspace: Some(WorkspaceId("2".into())),
                },
            ]
        );
    }

    #[test]
    fn monitors_without_geometry_have_empty_rectangles() {
        let monitors = parse_monitors(MONITORS, WORKSPACES, &Geometry::default()).expect("fixtures parse");
        assert!(monitors.iter().all(|m| m.rect == Rect::default() && m.scale == 1.0));
    }

    #[test]
    fn orientation_of_layouts() {
        assert_eq!(orientation("h_tiles"), Some(SplitDirection::LeftRight));
        assert_eq!(orientation("h_accordion"), Some(SplitDirection::LeftRight));
        assert_eq!(orientation("v_tiles"), Some(SplitDirection::TopBottom));
        assert_eq!(orientation("floating"), None);
        assert_eq!(orientation("macos_native_minimized"), None);
    }
}
