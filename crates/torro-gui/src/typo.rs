//! The type scale: every piece of text in the apps uses one of these levels.
//!
//! The levels come from the active design ([`crate::design`]); the standard design gives the values
//! of the design skill (`.claude/skills/torro-design`, section 3). A level fixes size, absolute line
//! height and font cut together, so text lines up on the same grid everywhere and apps never write
//! `.size(…)` by hand. The base size is 13 in the standard, as in Linear; a larger interface comes
//! from Iced's `scale_factor` (the zoom), never from other sizes. Hierarchy comes from the
//! level *and* the colour (`text` → `text_2` → `text_3`); colour stays the caller's choice and
//! always comes from the theme.
//!
//! ```ignore
//! use torro_gui::{theme, typo};
//! typo::text(typo::headline(), tr("Review API concept"));
//! typo::text(typo::meta(), tr("Sunday")).style(theme::text_3);
//! typo::text(typo::meta().tabular(), "09:30"); // digits that change: clocks, axes, counters
//! typo::text(typo::micro(), "14:00").style(theme::text_2); // time axis, times in events
//! ```
//!
//! Widgets that take text settings themselves (`text_input`, `pick_list`) get them from the
//! level's fields: `let body = typo::body(); input.size(body.size).line_height(body.line_height).font(body.font)`.
//!
//! The levels are functions of the active design, not constants: never keep one in a `const` or
//! `static`; derive a level in a function (`fn day_number() -> Level { typo::body_strong().tabular() }`).

use iced::widget::text::{IntoFragment, LineHeight, Text};
use iced::{Font, Pixels};
use torro_core::design;

/// One level of the type scale. Use the functions ([`body`], [`title`] …), never build one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    /// Font size in logical pixels.
    pub size: Pixels,
    /// Line height, always absolute (Iced's default is 1.3 × size, which falls off the 4-px grid).
    pub line_height: LineHeight,
    /// The font cut: family and weight (400, 500 or 600 in the standard).
    pub font: Font,
}

impl Level {
    /// The same level with tabular figures (the design's `numbers` font, "Inter Tabular" in the
    /// standard), for digits that change: clocks, time axes, counters, dates in columns. Keeps size
    /// and line height.
    ///
    /// The standard ships only Regular and Medium as tabular cuts, so a SemiBold level ([`title`],
    /// [`headline`], [`display`]) becomes tabular Medium (500): the nearest lighter weight the
    /// figures font has.
    pub fn tabular(self) -> Self {
        Self { font: crate::design::tabular_font(self.font.weight), ..self }
    }
}

/// The level `pick` chooses from the active design's type scale.
fn level(pick: fn(&design::TypeScale) -> design::Level) -> Level {
    let level = crate::design::with(|t| pick(&t.design.typo));
    Level {
        size: Pixels(level.size),
        line_height: LineHeight::Absolute(Pixels(level.line)),
        font: crate::design::font(level.font, level.weight),
    }
}

/// 11/14, Regular, always tabular: the calendar's time axis, times inside events, weekday
/// letters in the week strip. The only level below 12; never for running text.
pub fn micro() -> Level {
    level(|t| t.micro)
}
/// 12/16, Medium: group titles (sentence case, colour `text_3`), buttons, chips, badges.
pub fn caption() -> Level {
    level(|t| t.caption)
}
/// 12/16, Regular: metadata, counters, due dates.
pub fn meta() -> Level {
    level(|t| t.meta)
}
/// 13/20, Regular: the default – lists, sidebar, inputs, running text. Also the apps' default
/// text size.
pub fn body() -> Level {
    level(|t| t.body)
}
/// 13/20, Medium: header titles, row titles, event titles.
pub fn body_strong() -> Level {
    level(|t| t.body_strong)
}
/// 14/22, Regular: reading text – the whole of a mail in the inbox's reader (task E8.2). One step
/// above [`body`] with more line height, because it is read for longer than a list; never for
/// rows, labels or inputs, which stay [`body`]. A design pack keeps it at least as large as `body`.
pub fn read() -> Level {
    level(|t| t.read)
}
/// 16/22, SemiBold: card and dialog titles (the design's heading font).
pub fn title() -> Level {
    level(|t| t.title)
}
/// 22/28, SemiBold: detail titles ("Review API concept"), view titles without a header bar (the
/// design's heading font).
pub fn headline() -> Level {
    level(|t| t.headline)
}
/// 28/32, SemiBold: the big number in a mini tile (the time, the count of open tasks).
pub fn display() -> Level {
    level(|t| t.display)
}

/// The eight levels of the Linear scale, smallest first: everything but [`read`], which came with
/// the inbox's reader (task E8.2). Kept at eight because the test "same picture" of design packs
/// (`crate::design`) holds them to the constants from before the packs; the tests here check
/// [`read`] on its own.
pub fn all() -> [Level; 8] {
    [micro(), caption(), meta(), body(), body_strong(), title(), headline(), display()]
}

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
    use crate::theme;
    use iced::font::Weight;

    fn line(level: Level) -> f32 {
        match level.line_height {
            LineHeight::Absolute(px) => px.0,
            LineHeight::Relative(_) => f32::NAN,
        }
    }

    #[test]
    fn levels_match_the_design_skill() {
        let expected = [
            (micro(), 11.0, 14.0, theme::font_tabular()),
            (caption(), 12.0, 16.0, theme::font_medium()),
            (meta(), 12.0, 16.0, theme::font()),
            (body(), 13.0, 20.0, theme::font()),
            (body_strong(), 13.0, 20.0, theme::font_medium()),
            (title(), 16.0, 22.0, theme::font_semibold()),
            (headline(), 22.0, 28.0, theme::font_semibold()),
            (display(), 28.0, 32.0, theme::font_semibold()),
        ];
        assert_eq!(expected.len(), all().len());
        for (level, size, height, font) in expected.into_iter().chain([(read(), 14.0, 22.0, theme::font())]) {
            assert_eq!(level.size.0, size);
            assert_eq!(line(level), height, "line height of {size}px level");
            assert_eq!(level.font, font, "font of {size}px level");
        }
    }

    #[test]
    fn line_heights_are_absolute_and_on_the_grid() {
        for level in all().into_iter().chain([read()]) {
            assert!(matches!(level.line_height, LineHeight::Absolute(_)));
            assert_eq!(line(level) % 2.0, 0.0, "line height {} is odd", line(level));
            assert!(line(level) > level.size.0);
        }
    }

    #[test]
    fn only_micro_is_smaller_than_12_and_nothing_is_bold() {
        for level in all().into_iter().chain([read()]) {
            assert!(level.size.0 >= 12.0 || level == micro());
            assert!(matches!(level.font.weight, Weight::Normal | Weight::Medium | Weight::Semibold));
        }
        assert_eq!(micro().font, theme::font_tabular(), "micro is for figures, so it is always tabular");
        assert_eq!(micro().tabular(), micro());
    }

    #[test]
    fn tabular_switches_family_and_keeps_size() {
        assert_eq!(meta().tabular().font, theme::font_tabular());
        assert_eq!(body_strong().tabular().font, theme::font_tabular_medium());
        assert_eq!(display().tabular().font, theme::font_tabular_medium());
        assert_eq!(display().tabular().size, display().size);
        assert_eq!(display().tabular().line_height, display().line_height);
    }

    #[test]
    fn text_builds_in_every_level() {
        for level in all().into_iter().chain([read()]) {
            let _: Text<'_> = text(level, "Today");
        }
    }

    #[test]
    fn ellipsize_cuts_long_text_only() {
        assert_eq!(ellipsize("Standup", caption(), 200.0), "Standup");
        let cut = ellipsize("Quarterly planning with the whole team", caption(), 100.0);
        assert!(cut.ends_with(ELLIPSIS), "{cut}");
        assert!(text_width(&cut, caption()) <= 100.0);
        assert!(!cut.contains(" …"), "no space before the ellipsis");
        assert_eq!(ellipsize("Standup", caption(), 5.0), "");
    }

    #[test]
    fn line_height_is_the_absolute_one() {
        assert_eq!(line_height(body()), 20.0);
        assert_eq!(line_height(read()), 22.0, "reading text has more room between its lines");
        assert_eq!(line_height(display()), 32.0);
    }
}
