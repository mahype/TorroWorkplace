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

impl WindowManager for Hyprland {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    /// Task 1.5.
    async fn active_workspace(&self) -> Result<WorkspaceId, WmError> {
        Err(WmError::NotImplemented("hyprland active_workspace (task 1.5)"))
    }

    /// Task 1.5.
    async fn windows(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>, WmError> {
        let _ = workspace;
        Err(WmError::NotImplemented("hyprland windows (task 1.5)"))
    }

    /// Task 1.5.
    async fn monitors(&self) -> Result<Vec<Monitor>, WmError> {
        Err(WmError::NotImplemented("hyprland monitors (task 1.5)"))
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
