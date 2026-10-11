//! Making a template from the active workspace.
//!
//! Task 1.13 in `docs/tasks/phase-1.md`. Reference:
//! `apps/workspace-setups/workspace_setups/capture.py`.

use chrono::{DateTime, Local};
use torro_wm::{Window, WindowManager};

use crate::{SetupsError, Strategy, Template};

/// Classes of programs that only run once; their windows default to [`Strategy::Move`].
pub const SINGLE_INSTANCE: &[&str] = &["spotify", "slack", "signal", "discord", "obsidian", "1password"];

/// The active workspace as a template with id `id` (callers get it from
/// `Store::unique_id`). Windows get ids `window-1`, `window-2`, … in the order the window
/// manager lists them; tiled ones go into the tree (`layout::infer_tree`); rectangles are
/// stored relative to the monitor. `resolve` finds the start command of a window (the
/// daemon passes `launch::command_for`). The name is [`auto_name`].
pub async fn capture(
    wm: &impl WindowManager,
    id: String,
    resolve: impl Fn(&Window) -> Vec<String>,
    now: DateTime<Local>,
) -> Result<Template, SetupsError> {
    let _ = (wm, id, resolve, now);
    Err(SetupsError::NotImplemented("capture (task 1.13)"))
}

/// `Workspace 3, 11.10. 14:20`
pub fn auto_name(workspace: &str, now: DateTime<Local>) -> String {
    let _ = (workspace, now);
    String::new()
}

/// [`Strategy::Move`] for [`SINGLE_INSTANCE`] classes (case-insensitive), else [`Strategy::New`].
pub fn default_strategy(class: &str) -> Strategy {
    let _ = class;
    Strategy::New
}
