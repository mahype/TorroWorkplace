//! Hyprland backend.
//!
//! Talks to Hyprland over its two Unix sockets in
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`: `.socket.sock` for requests
//! (`j/clients`, `dispatch …`) and `.socket2.sock` for the event stream. Under Omarchy 4,
//! Hyprland 0.56 takes dispatchers as Lua expressions (`hl.dsp.focus({ … })`).
//!
//! Tasks 1.3–1.8 in `docs/tasks/phase-1.md` fill in the submodules.

pub mod dispatch;
pub mod events;
pub mod json;
pub mod socket;

use std::path::PathBuf;

use crate::{Events, Monitor, Split, SplitDirection, Window, WindowId, WindowManager, WmError, WorkspaceId};

/// A connection to the running Hyprland instance.
#[derive(Debug, Clone)]
pub struct Hyprland {
    /// Directory holding `.socket.sock` and `.socket2.sock`.
    pub socket_dir: PathBuf,
}

impl Hyprland {
    /// Finds the instance from `XDG_RUNTIME_DIR` and `HYPRLAND_INSTANCE_SIGNATURE`.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Result<Hyprland, WmError> {
        let dir = socket::socket_dir(env("XDG_RUNTIME_DIR").as_deref(), env("HYPRLAND_INSTANCE_SIGNATURE").as_deref())?;
        Ok(Hyprland { socket_dir: dir })
    }
}

/// The normal windows from the answers to `j/clients` and `j/monitors`, with monitor names
/// filled in and, if given, only those on `workspace`. Split out of [`Hyprland::windows`] so it
/// can be tested without a socket.
fn windows_from_answers(
    clients: &str,
    monitors: &str,
    workspace: Option<&WorkspaceId>,
) -> Result<Vec<Window>, WmError> {
    let names = json::monitor_names(monitors)?;
    let windows = json::join_monitor_names(json::parse_clients_with_monitor(clients)?, &names);
    Ok(match workspace {
        Some(workspace) => windows.into_iter().filter(|window| &window.workspace == workspace).collect(),
        None => windows,
    })
}

impl WindowManager for Hyprland {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    async fn active_workspace(&self) -> Result<WorkspaceId, WmError> {
        let answer = socket::request(&self.socket_dir, "j/activeworkspace").await?;
        json::parse_active_workspace(&answer)
    }

    async fn windows(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>, WmError> {
        let clients = socket::request(&self.socket_dir, "j/clients").await?;
        let monitors = socket::request(&self.socket_dir, "j/monitors").await?;
        windows_from_answers(&clients, &monitors, workspace)
    }

    async fn monitors(&self) -> Result<Vec<Monitor>, WmError> {
        let answer = socket::request(&self.socket_dir, "j/monitors").await?;
        json::parse_monitors(&answer)
    }

    /// Task 1.6.
    async fn focus_window(&self, window: &WindowId) -> Result<(), WmError> {
        let _ = window;
        Err(WmError::NotImplemented("hyprland focus_window (task 1.6)"))
    }

    /// Task 1.6.
    async fn move_to_workspace(&self, window: &WindowId, workspace: &WorkspaceId) -> Result<(), WmError> {
        let _ = (window, workspace);
        Err(WmError::NotImplemented("hyprland move_to_workspace (task 1.6)"))
    }

    /// Task 1.6.
    async fn close_window(&self, window: &WindowId) -> Result<(), WmError> {
        let _ = window;
        Err(WmError::NotImplemented("hyprland close_window (task 1.6)"))
    }

    /// Task 1.8.
    async fn prepare_insert(&self, sibling: &WindowId, direction: SplitDirection) -> Result<(), WmError> {
        let _ = (sibling, direction);
        Err(WmError::NotImplemented("hyprland prepare_insert (task 1.8)"))
    }

    /// Task 1.8.
    async fn place(&self, window: &WindowId, sibling: &WindowId, split: Split) -> Result<(), WmError> {
        let _ = (window, sibling, split);
        Err(WmError::NotImplemented("hyprland place (task 1.8)"))
    }

    /// Task 1.7.
    async fn events(&self) -> Result<Events, WmError> {
        events::subscribe(&self.socket_dir).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENTS: &str = include_str!("../../tests/fixtures/hyprland-clients.json");
    const MONITORS: &str = include_str!("../../tests/fixtures/hyprland-monitors.json");

    #[test]
    fn windows_carry_monitor_name_from_monitors_answer() {
        let windows = windows_from_answers(CLIENTS, MONITORS, None).expect("fixtures parse");
        assert_eq!(windows.len(), 7);
        assert!(windows.iter().all(|w| w.monitor.as_deref() == Some("HDMI-A-1")));
    }

    #[test]
    fn workspace_filter_keeps_only_that_workspace() {
        let all = windows_from_answers(CLIENTS, MONITORS, None).expect("fixtures parse");
        let on_one = windows_from_answers(CLIENTS, MONITORS, Some(&WorkspaceId("1".into()))).expect("fixtures parse");
        assert!(!on_one.is_empty());
        assert!(on_one.len() < all.len());
        assert!(on_one.iter().all(|w| w.workspace == WorkspaceId("1".into())));
        let expected: Vec<&Window> = all.iter().filter(|w| w.workspace == WorkspaceId("1".into())).collect();
        assert_eq!(on_one.iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn workspace_without_windows_gives_empty_list() {
        let windows = windows_from_answers(CLIENTS, MONITORS, Some(&WorkspaceId("42".into()))).expect("fixtures parse");
        assert!(windows.is_empty());
    }

    /// Needs a running Hyprland on this machine: `cargo test -p torro-wm -- --ignored`.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "needs a running Hyprland instance"]
    async fn live_windows_of_active_workspace_carry_monitor_names() {
        let wm = Hyprland::from_env(|key| std::env::var(key).ok()).expect("Hyprland environment");
        let active = wm.active_workspace().await.expect("active workspace");
        let windows = wm.windows(Some(&active)).await.expect("windows of active workspace");
        eprintln!("active workspace {active}: {} window(s)", windows.len());
        assert!(windows.iter().all(|w| w.workspace == active), "window on another workspace");
        for window in &windows {
            assert!(window.monitor.is_some(), "{} ({}) has no monitor name", window.id, window.class);
        }
    }
}
