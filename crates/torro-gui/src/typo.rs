//! The type scale: every piece of text in the apps uses one of these levels.
//!
//! Values from the design skill (`.claude/skills/torro-design`, section 3). A level fixes size,
//! absolute line height and Inter cut together, so text lines up on the same grid everywhere and
//! apps never write `.size(…)` by hand. The base size is 13, as in Linear; a larger interface
//! comes later from Iced's `scale_factor`, never from other sizes. Hierarchy comes from the
//! level *and* the colour (`text` → `text_2` → `text_3`); colour stays the caller's choice and
//! always comes from the theme.
//!
//! ```ignore
//! use torro_gui::{theme, typo};
//! typo::text(typo::HEADLINE, tr("Review API concept"));
//! typo::text(typo::META, tr("Sunday")).style(theme::text_3);
//! typo::text(typo::META.tabular(), "09:30"); // digits that change: clocks, axes, counters
//! typo::text(typo::MICRO, "14:00").style(theme::text_2); // time axis, times in events
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

/// 11/14, Regular, always tabular: the calendar's time axis, times inside events, weekday
/// letters in the week strip. The only level below 12; never for running text.
pub const MICRO: Level = level(11.0, 14.0, theme::FONT_TABULAR);
/// 12/16, Medium: group titles (sentence case, colour `text_3`), buttons, chips, badges.
pub const CAPTION: Level = level(12.0, 16.0, theme::FONT_MEDIUM);
/// 12/16, Regular: metadata, counters, due dates.
pub const META: Level = level(12.0, 16.0, theme::FONT);
/// 13/20, Regular: the default – lists, sidebar, inputs, running text. Also the apps' default
/// text size.
pub const BODY: Level = level(13.0, 20.0, theme::FONT);
/// 13/20, Medium: header titles, row titles, event titles.
pub const BODY_STRONG: Level = level(13.0, 20.0, theme::FONT_MEDIUM);
/// 16/22, SemiBold: card and dialog titles.
pub const TITLE: Level = level(16.0, 22.0, theme::FONT_SEMIBOLD);
/// 22/28, SemiBold: detail titles ("Review API concept"), view titles without a header bar.
pub const HEADLINE: Level = level(22.0, 28.0, theme::FONT_SEMIBOLD);
/// 28/32, SemiBold: the big number in a mini tile (the time, the count of open tasks).
pub const DISPLAY: Level = level(28.0, 32.0, theme::FONT_SEMIBOLD);

/// All levels, smallest first.
pub const ALL: [Level; 8] = [MICRO, CAPTION, META, BODY, BODY_STRONG, TITLE, HEADLINE, DISPLAY];

/// A `text` widget in `level`: size, line height and font are set; colour, wrapping and
/// alignment stay with the caller (`.style(theme::text_2)` …).
///
/// Use it for every visible text instead of `iced::widget::text(…).size(…)`. `content` is
/// anything `text` takes: `&str`, `String`, the result of `tr(…)`/`trf(…)`.
pub fn text<'a>(level: Level, content: impl IntoFragment<'a>) -> Text<'a> {
    iced::widget::text(content).size(level.size).line_height(level.line_height).font(level.font)
}

/// What [`ellipsize`] puts where it cut.
pub const ELLIPSIS: &str = "…";

/// `text` cut to fit `max_width` in `level`, with "…" where it was cut; empty when not even the
/// first letter fits.
///
/// Iced cannot measure text in a canvas, nor cut it with an ellipsis, so this estimates Inter's
/// letter widths a little generously: a cut comes a letter early rather than running over (the
/// views clip as well).
pub fn ellipsize(text: &str, level: Level, max_width: f32) -> String {
    let size = level.size.0;
    if text_width(text, level) <= max_width {
        return text.to_string();
    }
    let room = max_width - text_width(ELLIPSIS, level);
    let mut kept = String::new();
    let mut width = 0.0;
    for c in text.chars() {
        width += char_em(c) * size;
        if width > room {
            break;
        }
        kept.push(c);
    }
    let kept = kept.trim_end();
    if kept.is_empty() { String::new() } else { format!("{kept}{ELLIPSIS}") }
}

/// The estimated width of `text` in `level` (see [`ellipsize`]).
pub fn text_width(text: &str, level: Level) -> f32 {
    text.chars().map(char_em).sum::<f32>() * level.size.0
}

/// Estimated advance of `c` in Inter, in em, rounded up a little (Medium is wider than Regular).
fn char_em(c: char) -> f32 {
    match c {
        ' ' => 0.28,
        'i' | 'j' | 'l' | 'I' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' => 0.30,
        'f' | 't' | 'r' | '(' | ')' | '-' | '/' => 0.40,
        'm' | 'w' => 0.88,
        'M' | 'W' => 0.95,
        '0'..='9' => 0.64,
        c if c.len_utf8() >= 3 => 1.0,
        c if c.is_uppercase() => 0.72,
        c if c.is_lowercase() => 0.58,
        _ => 0.75,
    }
}

/// The absolute line height of `level` in logical pixels.
pub fn line_height(level: Level) -> f32 {
    level.line_height.to_absolute(level.size).0
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
            (MICRO, 11.0, 14.0, theme::FONT_TABULAR),
            (CAPTION, 12.0, 16.0, theme::FONT_MEDIUM),
            (META, 12.0, 16.0, theme::FONT),
            (BODY, 13.0, 20.0, theme::FONT),
            (BODY_STRONG, 13.0, 20.0, theme::FONT_MEDIUM),
            (TITLE, 16.0, 22.0, theme::FONT_SEMIBOLD),
            (HEADLINE, 22.0, 28.0, theme::FONT_SEMIBOLD),
            (DISPLAY, 28.0, 32.0, theme::FONT_SEMIBOLD),
        ];
        assert_eq!(expected.len(), ALL.len());
        for (level, size, height, font) in expected {
            assert_eq!(level.size.0, size);
            assert_eq!(line(level), height, "line height of {size}px level");
            assert_eq!(level.font, font, "font of {size}px level");
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
    fn only_micro_is_smaller_than_12_and_nothing_is_bold() {
        for level in ALL {
            assert!(level.size.0 >= 12.0 || level == MICRO);
            assert!(matches!(level.font.weight, Weight::Normal | Weight::Medium | Weight::Semibold));
        }
        assert_eq!(MICRO.font, theme::FONT_TABULAR, "micro is for figures, so it is always tabular");
        assert_eq!(MICRO.tabular(), MICRO);
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

    #[test]
    fn ellipsize_cuts_long_text_only() {
        assert_eq!(ellipsize("Standup", CAPTION, 200.0), "Standup");
        let cut = ellipsize("Quarterly planning with the whole team", CAPTION, 100.0);
        assert!(cut.ends_with(ELLIPSIS), "{cut}");
        assert!(text_width(&cut, CAPTION) <= 100.0);
        assert!(!cut.contains(" …"), "no space before the ellipsis");
        assert_eq!(ellipsize("Standup", CAPTION, 5.0), "");
    }

    #[test]
    fn line_height_is_the_absolute_one() {
        assert_eq!(line_height(BODY), 20.0);
        assert_eq!(line_height(DISPLAY), 32.0);
    }
}
