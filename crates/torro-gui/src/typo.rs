//! The type scale: every piece of text in the apps uses one of these levels.
//!
//! Values from the design skill (`.claude/skills/torro-design`, section 3). A level fixes size,
//! absolute line height and Inter cut together, so text lines up on the same grid everywhere and
//! apps never write `.size(…)` by hand. Hierarchy comes from the level *and* the colour
//! (`theme::TEXT` → `TEXT_2` → `TEXT_3`); colour stays the caller's choice.
//!
//! ```ignore
//! use torro_gui::typo;
//! typo::text(typo::HEADLINE, tr("Today"));
//! typo::text(typo::META, tr("Sunday")).color(theme::MUTED);
//! typo::text(typo::META.tabular(), "09:30"); // digits that change: clocks, axes, counters
//! ```
//!
//! Widgets that take text settings themselves (`text_input`, `pick_list`) get them from the
//! level's fields: `.size(typo::BODY.size).line_height(typo::BODY.line_height).font(typo::BODY.font)`.

use iced::font::Weight;
use iced::widget::text::{IntoFragment, LineHeight, Text};
use iced::{Font, Pixels};

use crate::theme;

/// One level of the type scale. Use the constants ([`BODY`], [`TITLE`] …), never build one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    /// Font size in logical pixels.
    pub size: Pixels,
    /// Line height, always absolute (Iced's default is 1.3 × size, which falls off the 4-px grid).
    pub line_height: LineHeight,
    /// The Inter cut: family and weight (400, 500 or 600 only).
    pub font: Font,
}

impl Level {
    /// The same level with tabular figures ("Inter Tabular"), for digits that change: clocks,
    /// time axes, counters, dates in columns. Keeps size and line height.
    ///
    /// Only Regular and Medium are shipped as tabular cuts, so a SemiBold level ([`TITLE`],
    /// [`HEADLINE`], [`DISPLAY`]) becomes tabular Medium (500).
    pub const fn tabular(self) -> Self {
        let font = match self.font.weight {
            Weight::Normal => theme::FONT_TABULAR,
            _ => theme::FONT_TABULAR_MEDIUM,
        };
        Self { font, ..self }
    }
}

const fn level(size: f32, line_height: f32, font: Font) -> Level {
    Level { size: Pixels(size), line_height: LineHeight::Absolute(Pixels(line_height)), font }
}

/// 12/16, Medium: section titles (sentence case, colour `TEXT_2`), badges.
pub const CAPTION: Level = level(12.0, 16.0, theme::FONT_MEDIUM);
/// 12/16, Regular: metadata, times in cells, axes.
pub const META: Level = level(12.0, 16.0, theme::FONT);
/// 14/20, Regular: the default – lists, inputs, running text. Also the apps' default text size.
pub const BODY: Level = level(14.0, 20.0, theme::FONT);
/// 14/20, Medium: row titles, button labels, event titles.
pub const BODY_STRONG: Level = level(14.0, 20.0, theme::FONT_MEDIUM);
/// 16/22, SemiBold: dialog, group and detail titles.
pub const TITLE: Level = level(16.0, 22.0, theme::FONT_SEMIBOLD);
/// 20/26, SemiBold: view titles ("October 2026", "Today").
pub const HEADLINE: Level = level(20.0, 26.0, theme::FONT_SEMIBOLD);
/// 28/32, SemiBold: the big number in a mini tile (the time, the count of open tasks).
pub const DISPLAY: Level = level(28.0, 32.0, theme::FONT_SEMIBOLD);

/// All levels, smallest first.
pub const ALL: [Level; 7] = [CAPTION, META, BODY, BODY_STRONG, TITLE, HEADLINE, DISPLAY];

/// A `text` widget in `level`: size, line height and font are set; colour, wrapping and
/// alignment stay with the caller (`.color(theme::MUTED)` …).
///
/// Use it for every visible text instead of `iced::widget::text(…).size(…)`. `content` is
/// anything `text` takes: `&str`, `String`, the result of `tr(…)`/`trf(…)`.
pub fn text<'a>(level: Level, content: impl IntoFragment<'a>) -> Text<'a> {
    iced::widget::text(content).size(level.size).line_height(level.line_height).font(level.font)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: Level) -> f32 {
        match level.line_height {
            LineHeight::Absolute(px) => px.0,
            LineHeight::Relative(_) => f32::NAN,
        }
    }

    #[test]
    fn levels_match_the_design_skill() {
        let expected = [
            (CAPTION, 12.0, 16.0, Weight::Medium),
            (META, 12.0, 16.0, Weight::Normal),
            (BODY, 14.0, 20.0, Weight::Normal),
            (BODY_STRONG, 14.0, 20.0, Weight::Medium),
            (TITLE, 16.0, 22.0, Weight::Semibold),
            (HEADLINE, 20.0, 26.0, Weight::Semibold),
            (DISPLAY, 28.0, 32.0, Weight::Semibold),
        ];
        for (level, size, height, weight) in expected {
            assert_eq!(level.size.0, size);
            assert_eq!(line(level), height, "line height of {size}px level");
            assert_eq!(level.font.weight, weight);
            assert_eq!(level.font.family, theme::FONT.family, "{size}px level is not Inter");
        }
    }

    #[test]
    fn line_heights_are_absolute_and_on_the_grid() {
        for level in ALL {
            assert!(matches!(level.line_height, LineHeight::Absolute(_)));
            assert_eq!(line(level) % 2.0, 0.0, "line height {} is odd", line(level));
            assert!(line(level) > level.size.0);
        }
    }

    #[test]
    fn nothing_is_smaller_than_12_or_bold() {
        for level in ALL {
            assert!(level.size.0 >= 12.0);
            assert!(matches!(level.font.weight, Weight::Normal | Weight::Medium | Weight::Semibold));
        }
    }

    #[test]
    fn tabular_switches_family_and_keeps_size() {
        assert_eq!(META.tabular().font, theme::FONT_TABULAR);
        assert_eq!(BODY_STRONG.tabular().font, theme::FONT_TABULAR_MEDIUM);
        assert_eq!(DISPLAY.tabular().font, theme::FONT_TABULAR_MEDIUM);
        assert_eq!(DISPLAY.tabular().size, DISPLAY.size);
        assert_eq!(DISPLAY.tabular().line_height, DISPLAY.line_height);
    }

    #[test]
    fn text_builds_in_every_level() {
        for level in ALL {
            let _: Text<'_> = text(level, "Today");
        }
    }
}
