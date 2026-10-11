//! Inferring the tiling layout from window rectangles.
//!
//! Task 1.10 in `docs/tasks/phase-1.md`. Reference:
//! `apps/workspace-setups/workspace_setups/layout.py`. Works for every window manager
//! because it only needs the rectangles, not the window manager's own tree.

use torro_wm::{Rect, SplitDirection};

use crate::LayoutTree;

/// Slack in pixels when deciding which side of a cut a window lies on. Window managers
/// round geometry, so a window may overlap its neighbour by a pixel.
const TOLERANCE: i32 = 1;

/// Tried in this order; on equal balance the earlier one wins, so left-right beats top-bottom.
const DIRECTIONS: [SplitDirection; 2] = [SplitDirection::LeftRight, SplitDirection::TopBottom];

/// The tree over tiled windows given as `(template window id, rectangle)`. Tries every
/// window edge as a cut, left-right and top-bottom; a cut is valid when it separates the
/// windows into two non-empty groups (1 px tolerance). Prefers the most balanced cut, on a
/// tie left-right. Ratio = cut position within the group's extent, rounded to four
/// decimals. Groups that cannot be cut become [`LayoutTree::Fallback`] (ids sorted).
/// `None` for no windows.
pub fn infer_tree(windows: &[(String, Rect)]) -> Option<LayoutTree> {
    if windows.is_empty() {
        return None;
    }
    Some(split(windows))
}

/// Windows of one group of the layout, as `(id, rectangle)`.
type Group = Vec<(String, Rect)>;

/// A valid cut of a group into two non-empty halves.
struct Cut {
    balance: usize,
    direction: SplitDirection,
    first: Group,
    second: Group,
}

fn split(items: &[(String, Rect)]) -> LayoutTree {
    if let [(id, _)] = items {
        return LayoutTree::Window { id: id.clone() };
    }
    let mut best: Option<Cut> = None;
    for direction in DIRECTIONS {
        for edge in edges(items, direction) {
            let Some((first, second)) = partition(items, direction, edge) else {
                continue;
            };
            let balance = first.len().abs_diff(second.len());
            if best.as_ref().is_none_or(|cut| balance < cut.balance) {
                best = Some(Cut { balance, direction, first, second });
            }
        }
    }
    match best {
        Some(cut) => {
            let ratio = ratio(items, cut.direction, &cut.first, &cut.second);
            LayoutTree::Split {
                direction: cut.direction,
                ratio,
                first: Box::new(split(&cut.first)),
                second: Box::new(split(&cut.second)),
            }
        }
        None => {
            let mut ids: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
            ids.sort();
            LayoutTree::Fallback { ids }
        }
    }
}

/// Start and end of a rectangle along the axis that `direction` divides.
fn span(rect: &Rect, direction: SplitDirection) -> (i32, i32) {
    match direction {
        SplitDirection::LeftRight => (rect.x, rect.x + rect.width),
        SplitDirection::TopBottom => (rect.y, rect.y + rect.height),
    }
}

/// Smallest start and largest end over the windows of a group.
fn extent(items: &[(String, Rect)], direction: SplitDirection) -> (i32, i32) {
    items.iter().fold((i32::MAX, i32::MIN), |(lo, hi), (_, rect)| {
        let (start, end) = span(rect, direction);
        (lo.min(start), hi.max(end))
    })
}

/// Every window end is a candidate cut, each once, ascending.
fn edges(items: &[(String, Rect)], direction: SplitDirection) -> Vec<i32> {
    let mut edges: Vec<i32> = items.iter().map(|(_, rect)| span(rect, direction).1).collect();
    edges.sort_unstable();
    edges.dedup();
    edges
}

/// Splits the windows at `edge`; `None` when some window straddles it or one side is empty.
/// Gaps between windows are fine: the cut only has to fall between the two groups.
fn partition(items: &[(String, Rect)], direction: SplitDirection, edge: i32) -> Option<(Group, Group)> {
    let mut first = Vec::new();
    let mut second = Vec::new();
    for item in items {
        let (start, end) = span(&item.1, direction);
        if end <= edge + TOLERANCE {
            first.push(item.clone());
        } else if start >= edge - TOLERANCE {
            second.push(item.clone());
        } else {
            return None;
        }
    }
    (!first.is_empty() && !second.is_empty()).then_some((first, second))
}

/// Share of the first group: the cut sits in the middle of the gap between the groups,
/// measured from the start of the whole group to its end, rounded to four decimals.
fn ratio(
    items: &[(String, Rect)],
    direction: SplitDirection,
    first: &[(String, Rect)],
    second: &[(String, Rect)],
) -> f32 {
    let (start, end) = extent(items, direction);
    let (_, first_end) = extent(first, direction);
    let (second_start, _) = extent(second, direction);
    let total = f64::from(end) - f64::from(start);
    if total <= 0.0 {
        return 0.5;
    }
    let cut = (f64::from(first_end) + f64::from(second_start)) / 2.0;
    let raw = (cut - f64::from(start)) / total;
    ((raw * 10_000.0).round() / 10_000.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: &str, x: i32, y: i32, width: i32, height: i32) -> (String, Rect) {
        (id.to_string(), Rect { x, y, width, height })
    }

    fn leaf(id: &str) -> LayoutTree {
        LayoutTree::Window { id: id.to_string() }
    }

    fn node(direction: SplitDirection, ratio: f32, first: LayoutTree, second: LayoutTree) -> LayoutTree {
        LayoutTree::Split { direction, ratio, first: Box::new(first), second: Box::new(second) }
    }

    #[test]
    fn no_windows_gives_none() {
        assert_eq!(infer_tree(&[]), None);
    }

    #[test]
    fn one_window_is_a_leaf() {
        assert_eq!(infer_tree(&[win("window-1", 0, 0, 1920, 1080)]), Some(leaf("window-1")));
    }

    #[test]
    fn two_side_by_side_split_evenly() {
        let windows = [win("window-1", 0, 0, 500, 1000), win("window-2", 500, 0, 500, 1000)];
        assert_eq!(
            infer_tree(&windows),
            Some(node(SplitDirection::LeftRight, 0.5, leaf("window-1"), leaf("window-2")))
        );
    }

    #[test]
    fn two_side_by_side_35_65() {
        let windows = [win("window-1", 0, 0, 350, 1000), win("window-2", 350, 0, 650, 1000)];
        assert_eq!(
            infer_tree(&windows),
            Some(node(SplitDirection::LeftRight, 0.35, leaf("window-1"), leaf("window-2")))
        );
    }

    #[test]
    fn nested_left_one_right_two_stacked() {
        let windows =
            [win("window-1", 0, 0, 350, 1000), win("window-2", 350, 0, 650, 500), win("window-3", 350, 500, 650, 500)];
        assert_eq!(
            infer_tree(&windows),
            Some(node(
                SplitDirection::LeftRight,
                0.35,
                leaf("window-1"),
                node(SplitDirection::TopBottom, 0.5, leaf("window-2"), leaf("window-3")),
            ))
        );
    }

    #[test]
    fn three_in_a_row() {
        let windows =
            [win("window-1", 0, 0, 100, 100), win("window-2", 100, 0, 100, 100), win("window-3", 200, 0, 100, 100)];
        assert_eq!(
            infer_tree(&windows),
            Some(node(
                SplitDirection::LeftRight,
                0.3333,
                leaf("window-1"),
                node(SplitDirection::LeftRight, 0.5, leaf("window-2"), leaf("window-3")),
            ))
        );
    }

    #[test]
    fn grid_prefers_left_right_on_tie() {
        let windows = [
            win("window-1", 0, 0, 500, 500),
            win("window-2", 500, 0, 500, 500),
            win("window-3", 0, 500, 500, 500),
            win("window-4", 500, 500, 500, 500),
        ];
        assert_eq!(
            infer_tree(&windows),
            Some(node(
                SplitDirection::LeftRight,
                0.5,
                node(SplitDirection::TopBottom, 0.5, leaf("window-1"), leaf("window-3")),
                node(SplitDirection::TopBottom, 0.5, leaf("window-2"), leaf("window-4")),
            ))
        );
    }

    #[test]
    fn pinwheel_falls_back_with_sorted_ids() {
        // Four windows turning around an empty centre: no straight line separates them.
        let windows = [
            win("window-3", 40, 60, 60, 40),
            win("window-1", 0, 0, 60, 40),
            win("window-4", 0, 40, 40, 60),
            win("window-2", 60, 0, 40, 60),
        ];
        assert_eq!(
            infer_tree(&windows),
            Some(LayoutTree::Fallback {
                ids: vec!["window-1".into(), "window-2".into(), "window-3".into(), "window-4".into()],
            })
        );
    }

    #[test]
    fn gaps_between_windows_still_split() {
        // Hyprland-style gaps of 4 px: the cut lies in the middle of each gap.
        let windows =
            [win("window-1", 0, 0, 348, 1000), win("window-2", 352, 0, 648, 300), win("window-3", 352, 304, 648, 696)];
        assert_eq!(
            infer_tree(&windows),
            Some(node(
                SplitDirection::LeftRight,
                0.35,
                leaf("window-1"),
                node(SplitDirection::TopBottom, 0.302, leaf("window-2"), leaf("window-3")),
            ))
        );
    }

    #[test]
    fn ratio_is_measured_within_the_group() {
        // The lower group does not reach the left edge, so its extent is 100..1000, not 0..1000.
        let windows = [
            win("window-1", 0, 0, 1000, 300),
            win("window-2", 100, 304, 300, 696),
            win("window-3", 404, 304, 596, 696),
        ];
        assert_eq!(
            infer_tree(&windows),
            Some(node(
                SplitDirection::TopBottom,
                0.302,
                leaf("window-1"),
                node(SplitDirection::LeftRight, 0.3356, leaf("window-2"), leaf("window-3")),
            ))
        );
    }

    #[test]
    fn one_pixel_overlap_counts_as_touching() {
        let windows = [win("window-1", 0, 0, 501, 100), win("window-2", 500, 0, 500, 100)];
        assert_eq!(
            infer_tree(&windows),
            Some(node(SplitDirection::LeftRight, 0.5005, leaf("window-1"), leaf("window-2")))
        );
    }
}
