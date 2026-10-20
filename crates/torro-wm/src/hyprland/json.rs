//! Reading Hyprland's JSON answers (`j/clients`, `j/activeworkspace`, `j/monitors`, `j/version`).
//!
//! Task 1.4 in `docs/tasks/phase-1.md`. Fixtures live in `tests/fixtures/hyprland-*.json`.
//!
//! Hyprland's JSON has many more fields than we need; serde ignores the rest, so a new
//! Hyprland version that adds fields does not break parsing.

use std::collections::HashMap;

use serde::Deserialize;

use crate::{Monitor, Rect, Window, WindowId, WmError, WorkspaceId};

/// The normal windows in a `j/clients` answer: mapped, not hidden, with a class, on a
/// regular workspace (id ≠ 0, name not `special:…`). `monitor` is left `None`; see
/// [`join_monitor_names`].
pub fn parse_clients(json: &str) -> Result<Vec<Window>, WmError> {
    Ok(parse_clients_with_monitor(json)?.into_iter().map(|(window, _)| window).collect())
}

/// Like [`parse_clients`], but also returns each window's numeric monitor id from the
/// `monitor` field of `j/clients`.
pub fn parse_clients_with_monitor(json: &str) -> Result<Vec<(Window, Option<i64>)>, WmError> {
    let clients: Vec<RawClient> = serde_json::from_str(json).map_err(|err| protocol("clients", &err))?;
    Ok(clients
        .into_iter()
        .filter(RawClient::is_normal)
        .map(|client| {
            let monitor_id = client.monitor;
            (Window::from(client), monitor_id)
        })
        .collect())
}

/// Maps monitor ids to monitor names from a `j/monitors` answer.
pub fn monitor_names(json: &str) -> Result<HashMap<i64, String>, WmError> {
    let monitors: Vec<RawMonitorId> = serde_json::from_str(json).map_err(|err| protocol("monitors", &err))?;
    Ok(monitors.into_iter().map(|monitor| (monitor.id, monitor.name)).collect())
}

/// Sets `Window::monitor` from the monitor id of each window. Ids that are not in `names`
/// (for example a monitor that was unplugged meanwhile) leave the monitor `None`.
pub fn join_monitor_names(clients: Vec<(Window, Option<i64>)>, names: &HashMap<i64, String>) -> Vec<Window> {
    clients
        .into_iter()
        .map(|(mut window, monitor_id)| {
            window.monitor = monitor_id.and_then(|id| names.get(&id).cloned());
            window
        })
        .collect()
}

/// The workspace name in a `j/activeworkspace` answer.
pub fn parse_active_workspace(json: &str) -> Result<WorkspaceId, WmError> {
    let workspace: RawWorkspace = serde_json::from_str(json).map_err(|err| protocol("activeworkspace", &err))?;
    if workspace.name.is_empty() {
        return Err(WmError::Protocol("activeworkspace: no workspace name".into()));
    }
    Ok(WorkspaceId(workspace.name))
}

/// The monitors in a `j/monitors` answer.
pub fn parse_monitors(json: &str) -> Result<Vec<Monitor>, WmError> {
    let monitors: Vec<RawMonitor> = serde_json::from_str(json).map_err(|err| protocol("monitors", &err))?;
    monitors.into_iter().map(Monitor::try_from).collect()
}

/// The version in a `j/version` answer, e.g. `0.56.0` (task WS.1). Hyprland writes it in `version`;
/// older ones only have `tag` (`v0.40.0-12-gabc`), whose leading `v` is dropped. `None` for an answer
/// without either or one that is not JSON.
pub fn parse_version(json: &str) -> Option<String> {
    let version: RawVersion = serde_json::from_str(json).ok()?;
    let from_tag = || version.tag.as_deref().map(|tag| tag.trim().trim_start_matches('v').to_owned());
    version
        .version
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .or_else(from_tag)
        .filter(|text| !text.is_empty())
}

/// The fields of `j/version` we read.
#[derive(Debug, Deserialize)]
struct RawVersion {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    tag: Option<String>,
}

fn protocol(what: &str, err: &serde_json::Error) -> WmError {
    WmError::Protocol(format!("{what}: {err}"))
}

/// One entry of `j/clients`. Only the fields we use; defaults follow what Hyprland means
/// when a field is missing (for example a client is mapped unless it says otherwise).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawClient {
    address: String,
    #[serde(default = "default_true")]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    workspace: RawWorkspace,
    /// Position on the desktop, logical pixels.
    #[serde(default)]
    at: [i32; 2],
    /// Size, logical pixels.
    #[serde(default)]
    size: [i32; 2],
    #[serde(default)]
    floating: bool,
    /// Id of the monitor the window is on; its name is in `j/monitors`.
    #[serde(default)]
    monitor: Option<i64>,
    /// `-1` (or missing) when Hyprland does not know the process.
    #[serde(default)]
    pid: Option<i64>,
    /// Hyprland 0.5x sends a number (`0` = not fullscreen, `1` = maximized, `2` = fullscreen),
    /// other versions a bool. Kept as a raw value and read by [`fullscreen_flag`].
    #[serde(default)]
    fullscreen: serde_json::Value,
}

fn default_true() -> bool {
    true
}

impl RawClient {
    /// The filter from the module doc: mapped, not hidden, with a class, on a regular
    /// workspace. Regular workspaces have positive ids; special ones (scratchpad) negative.
    fn is_normal(&self) -> bool {
        self.mapped && !self.hidden && !self.class.is_empty() && self.workspace.id > 0 && !self.workspace.is_special()
    }
}

impl From<RawClient> for Window {
    fn from(client: RawClient) -> Window {
        Window {
            id: WindowId(client.address),
            class: client.class,
            title: client.title,
            pid: client.pid.and_then(|pid| u32::try_from(pid).ok()).filter(|&pid| pid > 0),
            workspace: WorkspaceId(client.workspace.name),
            // `j/clients` only gives a monitor id, not its name; the name is in `j/monitors`.
            monitor: None,
            rect: Rect { x: client.at[0], y: client.at[1], width: client.size[0], height: client.size[1] },
            floating: client.floating,
            fullscreen: fullscreen_flag(&client.fullscreen),
        }
    }
}

/// `true` for a non-zero number or a `true` bool; anything else (including a missing field)
/// counts as not fullscreen.
fn fullscreen_flag(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        _ => false,
    }
}

/// A workspace as Hyprland writes it: `{ "id": 2, "name": "2" }`. Also used for the
/// `workspace` of a client and the `activeWorkspace` of a monitor.
#[derive(Debug, Default, Deserialize)]
struct RawWorkspace {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: String,
}

impl RawWorkspace {
    /// Special workspaces (`special:scratchpad`, or the unnamed `special`) live outside the
    /// normal workspaces and have negative ids.
    fn is_special(&self) -> bool {
        self.name == "special" || self.name.starts_with("special:")
    }
}

/// The id and name of one entry of `j/monitors`, for the join with `j/clients`.
#[derive(Debug, Deserialize)]
struct RawMonitorId {
    id: i64,
    name: String,
}

/// One entry of `j/monitors`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMonitor {
    name: String,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    /// Physical pixels of the current mode, not logical ones.
    #[serde(default)]
    width: i32,
    #[serde(default)]
    height: i32,
    scale: f32,
    #[serde(default)]
    active_workspace: Option<RawWorkspace>,
}

impl TryFrom<RawMonitor> for Monitor {
    type Error = WmError;

    fn try_from(monitor: RawMonitor) -> Result<Monitor, WmError> {
        let valid_scale = monitor.scale.is_finite() && monitor.scale > 0.0;
        if !valid_scale {
            return Err(WmError::Protocol(format!("monitors: bad scale {} on {}", monitor.scale, monitor.name)));
        }
        // `Rect` is in logical pixels: Hyprland's x/y already are, the mode size is not.
        let logical = |physical: i32| (f64::from(physical) / f64::from(monitor.scale)).round() as i32;
        let active_workspace =
            monitor.active_workspace.map(|workspace| workspace.name).filter(|name| !name.is_empty()).map(WorkspaceId);
        Ok(Monitor {
            name: monitor.name,
            rect: Rect { x: monitor.x, y: monitor.y, width: logical(monitor.width), height: logical(monitor.height) },
            scale: monitor.scale,
            active_workspace,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENTS: &str = include_str!("../../tests/fixtures/hyprland-clients.json");
    const ACTIVE_WORKSPACE: &str = include_str!("../../tests/fixtures/hyprland-activeworkspace.json");
    const MONITORS: &str = include_str!("../../tests/fixtures/hyprland-monitors.json");

    /// Addresses of the hand-made entries in the clients fixture that must be filtered out.
    const FILTERED_OUT: [&str; 4] = ["0x5c719e000001", "0x5c719e000002", "0x5c719e000003", "0x5c719e000004"];

    #[test]
    fn clients_fixture_drops_scratchpad_hidden_classless_and_unmapped() {
        let windows = parse_clients(CLIENTS).expect("fixture parses");
        assert_eq!(windows.len(), 7);
        for window in &windows {
            assert!(!FILTERED_OUT.contains(&window.id.0.as_str()), "{} should be filtered", window.id);
            assert!(!window.class.is_empty());
            assert_ne!(window.workspace.0, "special:scratchpad");
        }
    }

    #[test]
    fn clients_fixture_maps_fields_of_a_real_window() {
        let windows = parse_clients(CLIENTS).expect("fixture parses");
        let first = windows.first().expect("first window");
        assert_eq!(first.id, WindowId("0x5c719dbfc640".into()));
        assert_eq!(first.class, "Hermes");
        assert_eq!(first.title, "Terminal");
        assert_eq!(first.pid, Some(1332792));
        assert_eq!(first.workspace, WorkspaceId("1".into()));
        assert_eq!(first.monitor, None);
        assert_eq!(first.rect, Rect { x: 12, y: 32, width: 1901, height: 1036 });
        assert!(!first.floating);
        assert!(!first.fullscreen);
    }

    #[test]
    fn clients_fixture_reads_fullscreen_number_and_missing_pid() {
        let windows = parse_clients(CLIENTS).expect("fixture parses");
        let fullscreen = windows.iter().find(|w| w.id.0 == "0x5c719e000005").expect("fullscreen entry");
        assert!(fullscreen.fullscreen);
        assert!(fullscreen.floating);
        assert_eq!(fullscreen.workspace, WorkspaceId("3".into()));

        let no_pid = windows.iter().find(|w| w.id.0 == "0x5c719e000006").expect("no-pid entry");
        assert_eq!(no_pid.pid, None);
    }

    #[test]
    fn clients_accept_bool_fullscreen_and_missing_fields() {
        let json = r#"[
            {"address": "0x1", "class": "a", "workspace": {"id": 1, "name": "1"}, "fullscreen": true},
            {"address": "0x2", "class": "b", "workspace": {"id": 1, "name": "1"}, "fullscreen": false},
            {"address": "0x3", "class": "c", "workspace": {"id": 1, "name": "1"}}
        ]"#;
        let windows = parse_clients(json).expect("parses");
        let flags: Vec<bool> = windows.iter().map(|w| w.fullscreen).collect();
        assert_eq!(flags, vec![true, false, false]);
        assert!(windows.iter().all(|w| w.pid.is_none()));
    }

    #[test]
    fn clients_treat_special_names_and_unnamed_special_as_filtered() {
        let json = r#"[
            {"address": "0x1", "class": "a", "workspace": {"id": -99, "name": "special"}},
            {"address": "0x2", "class": "b", "workspace": {"id": -98, "name": "special:notes"}},
            {"address": "0x3", "class": "c", "workspace": {"id": 4, "name": "specialist"}},
            {"address": "0x4", "class": "d", "workspace": {"id": 0, "name": ""}}
        ]"#;
        let windows = parse_clients(json).expect("parses");
        let addresses: Vec<&str> = windows.iter().map(|w| w.id.0.as_str()).collect();
        assert_eq!(addresses, vec!["0x3"]);
    }

    #[test]
    fn clients_reject_garbage() {
        assert!(matches!(parse_clients("{not json"), Err(WmError::Protocol(_))));
    }

    #[test]
    fn active_workspace_fixture_gives_name() {
        assert_eq!(parse_active_workspace(ACTIVE_WORKSPACE), Ok(WorkspaceId("2".into())));
    }

    #[test]
    fn active_workspace_rejects_empty_name() {
        assert!(matches!(parse_active_workspace(r#"{"id": 0, "name": ""}"#), Err(WmError::Protocol(_))));
    }

    #[test]
    fn monitors_fixture_gives_logical_rect_and_active_workspace() {
        let monitors = parse_monitors(MONITORS).expect("fixture parses");
        assert_eq!(monitors.len(), 1);
        let monitor = &monitors[0];
        assert_eq!(monitor.name, "HDMI-A-1");
        assert_eq!(monitor.scale, 2.0);
        // 7680x2160 physical at scale 2 is 3840x1080 logical.
        assert_eq!(monitor.rect, Rect { x: 0, y: 0, width: 3840, height: 1080 });
        assert_eq!(monitor.active_workspace, Some(WorkspaceId("2".into())));
    }

    #[test]
    fn monitors_round_fractional_scale_and_reject_bad_scale() {
        let json = r#"[{"name": "DP-1", "x": 3840, "y": 0, "width": 2560, "height": 1440, "scale": 1.5,
                        "activeWorkspace": {"id": 5, "name": "web"}}]"#;
        let monitors = parse_monitors(json).expect("parses");
        assert_eq!(monitors[0].rect, Rect { x: 3840, y: 0, width: 1707, height: 960 });
        assert_eq!(monitors[0].active_workspace, Some(WorkspaceId("web".into())));

        let zero = r#"[{"name": "DP-1", "width": 1, "height": 1, "scale": 0}]"#;
        assert!(matches!(parse_monitors(zero), Err(WmError::Protocol(_))));
    }

    #[test]
    fn monitor_names_fixture_maps_id_to_name() {
        let names = monitor_names(MONITORS).expect("fixture parses");
        assert_eq!(names.get(&0).map(String::as_str), Some("HDMI-A-1"));
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn join_gives_every_fixture_window_the_monitor_name() {
        let clients = parse_clients_with_monitor(CLIENTS).expect("fixture parses");
        let names = monitor_names(MONITORS).expect("fixture parses");
        let windows = join_monitor_names(clients, &names);
        assert_eq!(windows.len(), 7);
        for window in &windows {
            assert_eq!(window.monitor.as_deref(), Some("HDMI-A-1"), "{}", window.id);
        }
    }

    #[test]
    fn join_matches_each_window_to_its_own_monitor() {
        let clients = r#"[
            {"address": "0x1", "class": "a", "workspace": {"id": 1, "name": "1"}, "monitor": 1},
            {"address": "0x2", "class": "b", "workspace": {"id": 2, "name": "2"}, "monitor": 0},
            {"address": "0x3", "class": "c", "workspace": {"id": 2, "name": "2"}, "monitor": 7},
            {"address": "0x4", "class": "d", "workspace": {"id": 2, "name": "2"}}
        ]"#;
        let monitors = r#"[
            {"id": 0, "name": "DP-1", "width": 1, "height": 1, "scale": 1},
            {"id": 1, "name": "eDP-1", "width": 1, "height": 1, "scale": 1}
        ]"#;
        let names = monitor_names(monitors).expect("parses");
        let windows = join_monitor_names(parse_clients_with_monitor(clients).expect("parses"), &names);
        let monitor_of: Vec<Option<&str>> = windows.iter().map(|w| w.monitor.as_deref()).collect();
        // Unknown id 7 and a missing `monitor` field both stay unnamed.
        assert_eq!(monitor_of, vec![Some("eDP-1"), Some("DP-1"), None, None]);
    }

    #[test]
    fn version_comes_from_version_or_else_tag() {
        let current =
            r#"{"branch": "", "commit": "abc", "version": "0.56.0", "dirty": false, "tag": "v0.56.0", "flags": []}"#;
        assert_eq!(parse_version(current).as_deref(), Some("0.56.0"));
        assert_eq!(parse_version(r#"{"tag": "v0.40.0-12-gabc"}"#).as_deref(), Some("0.40.0-12-gabc"));
        assert_eq!(parse_version(r#"{"version": "", "tag": "v0.41.0"}"#).as_deref(), Some("0.41.0"));
        assert_eq!(parse_version(r#"{"commit": "abc"}"#), None);
        assert_eq!(parse_version("unknown request"), None);
    }

    #[test]
    fn monitor_names_reject_garbage() {
        assert!(matches!(monitor_names("{not json"), Err(WmError::Protocol(_))));
    }
}
