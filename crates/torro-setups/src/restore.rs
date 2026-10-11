//! Rebuilding a template on the active workspace.
//!
//! Tasks 1.14–1.16 in `docs/tasks/phase-1.md`. Reference:
//! `apps/workspace-setups/workspace_setups/restore.py`.
//!
//! # Order of work
//!
//! 1. Remember the target workspace first; starting programs may move the focus.
//! 2. Apply the [`LoadMode`]: fail, keep or close what is there.
//! 3. Build the tree **top-down**. For a split, the first leaf of `second` is inserted next
//!    to the first leaf of `first` (`prepare_insert`, open/move, `place` with the split's
//!    ratio); then both halves are built the same way. Example: `TopBottom(LeftRight(A, B),
//!    C)` → A; C below A; B right of A. This way every split is created at the right level
//!    of the tree, in Hyprland's dwindle as well as in i3-like trees.
//! 4. Windows of a [`LayoutTree::Fallback`] and floating windows open without layout
//!    instructions.
//!
//! A window that fails (program missing, no window within the timeout) is reported and
//! skipped; the rest of the template still loads.

use std::time::Duration;

use torro_wm::WindowManager;

use crate::launch::Launcher;
use crate::{LoadMode, SetupsError, Template};

/// Knobs for a load.
#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// How long to wait for a started program's window.
    pub window_timeout: Duration,
}

impl Default for RestoreOptions {
    fn default() -> RestoreOptions {
        RestoreOptions { window_timeout: Duration::from_secs(15) }
    }
}

/// What a load did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreReport {
    /// Template window ids that are in place.
    pub restored: Vec<String>,
    /// Template window ids that failed, with the reason.
    pub failed: Vec<(String, String)>,
}

/// Loads `template` onto the active workspace.
pub async fn restore(
    wm: &impl WindowManager,
    launcher: &impl Launcher,
    template: &Template,
    mode: LoadMode,
    options: &RestoreOptions,
) -> Result<RestoreReport, SetupsError> {
    let _ = (wm, launcher, template, mode, options);
    Err(SetupsError::NotImplemented("restore (tasks 1.14-1.16)"))
}
