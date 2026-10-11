//! Reading Hyprland's JSON answers (`j/clients`, `j/activeworkspace`, `j/monitors`).
//!
//! Task 1.4 in `docs/tasks/phase-1.md`. Fixtures live in `tests/fixtures/hyprland-*.json`.

use crate::{Monitor, Window, WmError, WorkspaceId};

/// The normal windows in a `j/clients` answer: mapped, not hidden, with a class, on a
/// regular workspace (id ≠ 0, name not `special:…`).
pub fn parse_clients(json: &str) -> Result<Vec<Window>, WmError> {
    let _ = json;
    Err(WmError::NotImplemented("hyprland parse_clients (task 1.4)"))
}

/// The workspace name in a `j/activeworkspace` answer.
pub fn parse_active_workspace(json: &str) -> Result<WorkspaceId, WmError> {
    let _ = json;
    Err(WmError::NotImplemented("hyprland parse_active_workspace (task 1.4)"))
}

/// The monitors in a `j/monitors` answer.
pub fn parse_monitors(json: &str) -> Result<Vec<Monitor>, WmError> {
    let _ = json;
    Err(WmError::NotImplemented("hyprland parse_monitors (task 1.4)"))
}
