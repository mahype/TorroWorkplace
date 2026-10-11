//! Size classes for tiled windows. The apps run for hours as tiles of very different sizes, so
//! every view asks [`of`] which layout fits instead of comparing its own thresholds. The rules
//! are checked in order and the thresholds are shared constants, so views and tests agree.
//! Design rules per class: `.claude/skills/torro-design`, section 6.

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

#[cfg(test)]
mod tests {
    use super::*;

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
