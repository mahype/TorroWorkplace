//! Inferring the tiling layout from window rectangles.
//!
//! Task 1.10 in `docs/tasks/phase-1.md`. Reference:
//! `apps/workspace-setups/workspace_setups/layout.py`. Works for every window manager
//! because it only needs the rectangles, not the window manager's own tree.

use torro_wm::Rect;

use crate::LayoutTree;

/// The tree over tiled windows given as `(template window id, rectangle)`. Tries every
/// window edge as a cut, left-right and top-bottom; a cut is valid when it separates the
/// windows into two non-empty groups (1 px tolerance). Prefers the most balanced cut, on a
/// tie left-right. Ratio = cut position within the group's extent, rounded to four
/// decimals. Groups that cannot be cut become [`LayoutTree::Fallback`] (ids sorted).
/// `None` for no windows.
pub fn infer_tree(windows: &[(String, Rect)]) -> Option<LayoutTree> {
    let _ = windows;
    None
}
