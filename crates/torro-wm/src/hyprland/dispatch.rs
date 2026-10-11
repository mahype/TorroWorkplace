//! Building dispatcher expressions.
//!
//! Tasks 1.6 and 1.8 in `docs/tasks/phase-1.md`. Hyprland 0.56 under Omarchy 4 reads
//! dispatchers as Lua: `hl.dsp.focus({ window = "address:0x55d0…" })`. Every string that
//! goes into an expression passes through [`lua_string`]. The builders only return the
//! expression; the backend sends it as `dispatch <expression>`.

use crate::{SplitDirection, WindowId, WmError, WorkspaceId};

/// `address:0x…` for a window id; rejects anything that is not a hex address.
pub fn window_selector(window: &WindowId) -> Result<String, WmError> {
    let _ = window;
    Err(WmError::NotImplemented("hyprland window_selector (task 1.6)"))
}

/// A quoted Lua string literal with `\\`, `"`, newlines, tabs and control characters escaped.
pub fn lua_string(value: &str) -> String {
    let _ = value;
    String::new()
}

/// `hl.dsp.focus({ window = "address:0x…" })`
pub fn focus_window(window: &WindowId) -> Result<String, WmError> {
    let _ = window;
    Err(WmError::NotImplemented("hyprland focus_window expression (task 1.6)"))
}

/// `hl.dsp.window.move({ window = "address:0x…", workspace = "3", follow = false })`
pub fn move_to_workspace(window: &WindowId, workspace: &WorkspaceId) -> Result<String, WmError> {
    let _ = (window, workspace);
    Err(WmError::NotImplemented("hyprland move expression (task 1.6)"))
}

/// `hl.dsp.window.close({ window = "address:0x…" })`
pub fn close_window(window: &WindowId) -> Result<String, WmError> {
    let _ = window;
    Err(WmError::NotImplemented("hyprland close expression (task 1.6)"))
}

/// `hl.dsp.layout("preselect r")` for `LeftRight`, `"preselect d"` for `TopBottom`: the next
/// window opens right of / below the focused one.
pub fn preselect(direction: SplitDirection) -> String {
    let _ = direction;
    String::new()
}

/// `hl.dsp.layout("splitratio 1.3000 exact")`. Hyprland's dwindle ratio runs from 0.1 to
/// 1.9 with 1.0 as an even split, so the share of the first child is doubled.
pub fn split_ratio(ratio: f32) -> String {
    let _ = ratio;
    String::new()
}
