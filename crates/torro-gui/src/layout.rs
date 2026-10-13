//! Size classes for tiled windows. The apps run for hours as tiles of very different sizes, so
//! every view asks [`of`] which layout fits instead of comparing its own thresholds. The rules
//! are checked in order and the thresholds are shared constants, so views and tests agree.
//! Design rules per class: `.claude/skills/torro-design`, section 6. The thresholds are fixed;
//! design packs do not change them.
//!
//! # Layout gaps by role
//!
//! The second half of this module holds the margins, paddings and gaps that have a *role* rather
//! than a place on the spacing scale: the margin around the inset content panel ([`inset`]), the
//! padding of pages, panels, list rows and controls, and the gaps between and inside groups. They
//! come from the active design (`[layout]` in a pack, concept `docs/design-pakete.md`), so a pack
//! can give the whole suite more or less room in one place. Use them exactly where the role fits;
//! everything else stays on [`crate::theme::space`].
//!
//! ```ignore
//! container(page).padding(layout::page_padding());            // a settings page
//! column(groups).spacing(layout::section_gap());               // groups on that page
//! row![panel].padding(Padding::new(layout::inset()).left(0.0)); // the inset content panel
//! ```

use iced::Padding;
use torro_core::design::Pad;

use crate::design;

/// Window width below which a window is [`SizeClass::Mini`] (and, with the height rule, a tile
/// too narrow for a sidebar or a second column).
pub const MINI_MAX_WIDTH: f32 = 360.0;

/// Window height below which a window is flat: [`SizeClass::Strip`] if it is also wide enough,
/// otherwise [`SizeClass::Mini`].
pub const STRIP_MAX_HEIGHT: f32 = 280.0;

/// Window width below which a window is [`SizeClass::Narrow`]. A window at least this wide and
/// [`STRIP_MAX_HEIGHT`] high is a [`SizeClass::Strip`] instead.
pub const NARROW_MAX_WIDTH: f32 = 600.0;

/// Window width below which a window is [`SizeClass::Medium`]; wider windows are
/// [`SizeClass::Wide`].
pub const MEDIUM_MAX_WIDTH: f32 = 1024.0;

/// The layout class of a window, decided by [`of`] from its size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeClass {
    /// A glance at a small tile: the essentials, read from a distance, no editing.
    Mini,
    /// A flat, wide strip: a timeline or a single row of items, read at a glance.
    Strip,
    /// One narrow column with full operation; navigation moves into a menu.
    Narrow,
    /// The main view in full: all functions, navigation in the header.
    Medium,
    /// The main view with context beside it (sidebar, properties, detail).
    Wide,
}

/// The class for a window of `size` (logical pixels). The rules are checked in this order:
///
/// 1. height below [`STRIP_MAX_HEIGHT`] and width at least [`NARROW_MAX_WIDTH`]: `Strip`
/// 2. width below [`MINI_MAX_WIDTH`] or height below [`STRIP_MAX_HEIGHT`]: `Mini`
/// 3. width below [`NARROW_MAX_WIDTH`]: `Narrow`
/// 4. width below [`MEDIUM_MAX_WIDTH`]: `Medium`
/// 5. otherwise: `Wide`
pub fn of(size: iced::Size) -> SizeClass {
    let (width, height) = (size.width, size.height);
    if height < STRIP_MAX_HEIGHT && width >= NARROW_MAX_WIDTH {
        SizeClass::Strip
    } else if width < MINI_MAX_WIDTH || height < STRIP_MAX_HEIGHT {
        SizeClass::Mini
    } else if width < NARROW_MAX_WIDTH {
        SizeClass::Narrow
    } else if width < MEDIUM_MAX_WIDTH {
        SizeClass::Medium
    } else {
        SizeClass::Wide
    }
}

/// How many of `count` rows to show in a glance tile (mini, strip) when `fit` rows fit: all of
/// them, or one row less and the number left out, for a closing "+3 more". With room for a single
/// row that row is worth more than the count, so it is shown without one.
///
/// Returns `(rows to show, Some(left out))`; `shown + left out == count` whenever a count is
/// given. The caller measures `fit` from its own row height.
pub fn shown_rows(count: usize, fit: usize) -> (usize, Option<usize>) {
    if count <= fit || fit <= 1 {
        (count.min(fit), None)
    } else {
        let shown = fit - 1;
        (shown, Some(count - shown))
    }
}

/// The margin around the inset content panel beside the sidebar or rail (top, right, bottom):
/// 8 in the standard design.
pub fn inset() -> f32 {
    design::with(|t| t.design.layout.inset)
}

/// The padding of content and pages (a settings page, a page without a sidebar beside it):
/// 24 × 24 in the standard design.
pub fn page_padding() -> Padding {
    padding(design::with(|t| t.design.layout.page_padding))
}

/// The padding of panels, floating cards and dialogs: 16 × 16 in the standard design.
pub fn panel_padding() -> Padding {
    padding(design::with(|t| t.design.layout.panel_padding))
}

/// The padding of list rows (tasks, settings rows, group heads): 0 × 16 in the standard design. The
/// height of a row comes from `size::row()` and friends; the content is centred in it.
pub fn row_padding() -> Padding {
    padding(design::with(|t| t.design.layout.row_padding))
}

/// The padding of buttons and segments: 0 × 10 in the standard design (`style::button_padding`).
pub fn control_padding() -> Padding {
    padding(design::with(|t| t.design.layout.control_padding))
}

/// The gap between groups and sections of a page: 24 in the standard design.
pub fn section_gap() -> f32 {
    design::with(|t| t.design.layout.section_gap)
}

/// The gap between the elements inside a group (a group's title, its card, its footer): 8 in the
/// standard design.
pub fn item_gap() -> f32 {
    design::with(|t| t.design.layout.item_gap)
}

fn padding(pad: Pad) -> Padding {
    Padding { top: pad.vertical, right: pad.horizontal, bottom: pad.vertical, left: pad.horizontal }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_gaps_of_the_standard_design() {
        assert_eq!(inset(), 8.0);
        assert_eq!(page_padding(), Padding::new(24.0));
        assert_eq!(panel_padding(), Padding::new(16.0));
        assert_eq!(row_padding(), Padding::new(0.0).left(16.0).right(16.0));
        assert_eq!(control_padding(), Padding::new(0.0).left(10.0).right(10.0));
        assert_eq!((section_gap(), item_gap()), (24.0, 8.0));
    }

    fn class(width: f32, height: f32) -> SizeClass {
        of(iced::Size::new(width, height))
    }

    #[test]
    fn design_example_sizes_get_their_class() {
        // The sizes `shoot.sh` uses for the visual check (torro-design, section 9).
        assert_eq!(class(240.0, 160.0), SizeClass::Mini);
        assert_eq!(class(320.0, 480.0), SizeClass::Mini);
        assert_eq!(class(1200.0, 200.0), SizeClass::Strip);
        assert_eq!(class(480.0, 900.0), SizeClass::Narrow);
        assert_eq!(class(800.0, 600.0), SizeClass::Medium);
        assert_eq!(class(1600.0, 1000.0), SizeClass::Wide);
    }

    #[test]
    fn width_boundary_at_360() {
        assert_eq!(class(359.0, 600.0), SizeClass::Mini);
        assert_eq!(class(360.0, 600.0), SizeClass::Narrow);
    }

    #[test]
    fn width_boundary_at_600() {
        assert_eq!(class(599.0, 600.0), SizeClass::Narrow);
        assert_eq!(class(600.0, 600.0), SizeClass::Medium);
    }

    #[test]
    fn width_boundary_at_1024() {
        assert_eq!(class(1023.0, 600.0), SizeClass::Medium);
        assert_eq!(class(1024.0, 600.0), SizeClass::Wide);
    }

    #[test]
    fn height_boundary_at_280_for_width_599() {
        // Below 600 wide, a short window is a Mini, not a Strip.
        assert_eq!(class(599.0, 279.0), SizeClass::Mini);
        assert_eq!(class(599.0, 280.0), SizeClass::Narrow);
    }

    #[test]
    fn height_boundary_at_280_for_width_600() {
        assert_eq!(class(600.0, 279.0), SizeClass::Strip);
        assert_eq!(class(600.0, 280.0), SizeClass::Medium);
    }

    #[test]
    fn short_window_below_mini_width_is_mini_not_strip() {
        // Rule 1 needs a width of at least 600, so a short, narrow tile stays a Mini.
        assert_eq!(class(359.0, 200.0), SizeClass::Mini);
    }

    #[test]
    fn rows_that_do_not_fit_become_a_count() {
        assert_eq!(shown_rows(3, 5), (3, None));
        assert_eq!(shown_rows(5, 5), (5, None));
        assert_eq!(shown_rows(6, 3), (2, Some(4)), "the last slot says how many are left out");
        assert_eq!(shown_rows(2, 0), (0, None));
        assert_eq!(shown_rows(3, 1), (1, None), "one row rather than only a count");
    }
}
