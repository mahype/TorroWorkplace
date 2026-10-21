//! The value rules of the concept (`docs/design-pakete.md`, "Regeln"), one set per part (task L.19):
//! [`design`] checks type, scales, layout gaps and sizes, [`colors`] the contrasts, [`font`] the size
//! adjustment and the width of the figures. No rule looks at two parts, so every combination of valid
//! parts is valid; where two meet (the figures on the time axis), each is held to the fixed bound
//! [`DIGIT_MAX_EM`]. Pure checks on merged values; the file rules live in `files.rs`.
//!
//! Two rules differ from the table on purpose, because the standard design must pass its own
//! rules and its values are fixed (task L.1):
//!
//! - `line_selected` is not held to 3:1 on `panel` (the standard has 2.5 dark, 2.2 light); it must
//!   stand out at least three times as much as `line_strong` and less than `focus_ring`, the rule
//!   `torro_gui::theme`'s tests have held it to so far. A chosen chip also shows a check mark and
//!   the `selected` surface, so the edge is never the only sign.
//! - Status colours are held to 3:1 on `panel` in dark mode only. In light mode the standard's
//!   `ok` (2.1) and `attention` (2.2) are lower; there the derived text colours carry the
//!   meaning: `attention_text` and `error_text` must reach 4.5:1 on every surface.
//!
//! The chart colours (task E8.8b) are graphics: the four series colours and the grey of "Other"
//! must reach 3:1 on `panel`, as the calendar colours. That they stay apart for colour-blind eyes
//! cannot be checked by a contrast; the standard's were checked with the dataviz validator, and
//! `docs/design-pakete.md` asks pack authors to do the same.
//!
//! The tokens of the inbox's comfortable density (task E8.2) have rules of their own: `read` is
//! never smaller than `body` (it is the text read longest), and the avatars and source tiles grow
//! from small to large, so "large" always means larger.
//!
//! `layout.density` itself has no rule here (task E13.1): it is a word, not a number, so there is
//! no range to hold it to. Anything but `compact` or `comfortable` is already refused while
//! `design.toml` is read, with its line (`pack.rs`), as `type.*.font` and `weight` are.
//!
//! Weights have no rule in a design any more (task L.19): a font without a weight draws the nearest
//! ([`super::Font::nearest`]); only the text font's regular face is required, when the font is read.

use super::{Colors, DIGIT_MAX_EM, Design, DesignError, Mode, Palette, Rgba};

/// Text contrast on every surface (WCAG AA).
const TEXT_MIN: f32 = 4.5;
/// Contrast of graphics and controls (WCAG 1.4.11).
const CONTROL_MIN: f32 = 3.0;

/// The size adjustment a font may set itself (`size_adjust`).
pub(super) const SIZE_ADJUST: (f32, f32) = (0.85, 1.25);
/// The bounds of the size adjustment computed from the x-height.
pub(super) const SIZE_ADJUST_AUTO: (f32, f32) = (0.90, 1.15);
/// "00:00" on the time axis counts as this many figures: four, and a colon of at most half a figure
/// (every sample's colon is: at most 0.35 em after adjustment, half of [`DIGIT_MAX_EM`] is 0.36).
const TIME_FIGURES: f32 = 4.5;

/// Every value rule of the form `design` breaks, in a stable order.
pub(super) fn design(design: &Design) -> Vec<DesignError> {
    let mut problems = Vec::new();
    typo(design, &mut problems);
    scales(design, &mut problems);
    layout(design, &mut problems);
    sizes(design, &mut problems);
    problems
}

/// Every value rule the colours of both modes break, in a stable order.
pub(super) fn colors(colors: &Colors) -> Vec<DesignError> {
    let mut problems = Vec::new();
    for mode in [Mode::Dark, Mode::Light] {
        colours(mode, colors.get(mode), &mut problems);
    }
    problems
}

/// The value rules of a font: `given`, the `size_adjust` it sets, within [`SIZE_ADJUST`]; its widest
/// figure in `numbers` (`widest_figure` in em, plain) after the adjustment `factor` within
/// [`DIGIT_MAX_EM`]. A factor below 1 counts as 1 there, because the time axis is drawn in the smallest
/// size, which is never made smaller ([`super::MIN_TEXT_SIZE`]).
pub(super) fn font(given: Option<f32>, factor: f32, numbers: &str, widest_figure: Option<f32>) -> Vec<DesignError> {
    let mut problems = Vec::new();
    if let Some(value) = given {
        range("size_adjust", value, SIZE_ADJUST.0, SIZE_ADJUST.1, &mut problems);
    }
    if let Some(widest) = widest_figure {
        let width = widest * factor.max(1.0);
        if width > DIGIT_MAX_EM {
            problems.push(DesignError::DigitsTooWide { family: numbers.to_string(), width, max: DIGIT_MAX_EM });
        }
    }
    problems
}

/// The narrowest a time axis (`size.axis`, `size.axis_narrow`) may be in `design`: "00:00" in figures
/// of [`DIGIT_MAX_EM`] at the size of `micro`, with `space.xs` on either side (the now label of the
/// calendar's week view sits there), in whole pixels.
pub(super) fn axis_min(design: &Design) -> f32 {
    (TIME_FIGURES * DIGIT_MAX_EM * design.typo.micro.size + 2.0 * design.space.xs).ceil()
}

fn colours(mode: Mode, p: &Palette, problems: &mut Vec<DesignError>) {
    let key = |name: &str| format!("colors.{mode}.{name}");
    let mut text = vec![("text", p.text), ("text_2", p.text_2), ("text_3", p.text_3)];
    text.extend([("attention_text", p.attention_text(mode)), ("error_text", p.error_text(mode))]);
    for (name, fg) in text {
        for (surface, bg) in p.surfaces() {
            let ratio = fg.contrast(bg);
            if ratio < TEXT_MIN {
                problems.push(DesignError::TextContrast { fg: key(name), on: key(surface), ratio });
            }
        }
    }

    let mut controls = vec![("focus_ring", p.focus_ring), ("merged", p.merged)];
    if mode == Mode::Dark {
        controls.extend([("ok", p.ok), ("attention", p.attention), ("error", p.error), ("inactive", p.inactive)]);
    }
    let calendar = ["calendar[1]", "calendar[2]", "calendar[3]", "calendar[4]", "calendar[5]"];
    controls.extend(calendar.into_iter().zip(p.calendar));
    // Chart marks are graphics on the panel too (task E8.8b).
    let chart = ["chart[1]", "chart[2]", "chart[3]", "chart[4]"];
    controls.extend(chart.into_iter().zip(p.chart));
    controls.push(("chart_other", p.chart_other));
    // The dot and the border of a category pill are marks on the panel too (task E12.7).
    let category = ["category[1]", "category[2]", "category[3]", "category[4]"];
    controls.extend(category.into_iter().zip(p.category));
    for (name, fg) in controls {
        let ratio = fg.contrast(p.panel);
        if ratio < CONTROL_MIN {
            problems.push(DesignError::ControlContrast { fg: key(name), on: key("panel"), ratio });
        }
    }

    // Unread mail uses the same dot on rows and cards, including hover and selection surfaces.
    for (name, surface) in [("panel", p.panel), ("raised", p.raised), ("hover", p.hover), ("selected", p.selected)] {
        let ratio = p.unread.contrast(surface);
        if ratio < CONTROL_MIN {
            problems.push(DesignError::ControlContrast { fg: key("unread"), on: key(name), ratio });
        }
    }

    if !chosen_edge_stands_out(p) {
        problems.push(DesignError::ChosenEdge { mode });
    }

    let ratio = p.on_accent.contrast(p.accent);
    if ratio < TEXT_MIN {
        problems.push(DesignError::AccentContrast { mode, ratio });
    }

    if !(0.0..=1.0).contains(&p.tint) {
        problems.push(DesignError::OutOfRange { key: key("tint"), value: p.tint, min: 0.0, max: 1.0 });
    }

    sidebar(mode, p, problems);
}

/// The rule "Seitenleiste": the sidebar's three text colours on its three surfaces, and – when the
/// sidebar has a surface of its own ([`Palette::own_sidebar`]) – what is drawn there as a graphic
/// (focus ring, accent, merged, calendar colours, in dark mode the status colours) on `sidebar`.
/// Checked on the merged values, so a key that fell back counts with the value it fell back to.
///
/// The derived attention and error text are not held to the sidebar: it shows status as dots and
/// icons, and one red that reads as text on white and on a dark sidebar barely exists.
fn sidebar(mode: Mode, p: &Palette, problems: &mut Vec<DesignError>) {
    let key = |name: &str| format!("colors.{mode}.{name}");
    let s = p.in_sidebar();
    let surfaces = [("sidebar", s.bg), ("sidebar_hover", s.hover), ("sidebar_selected", s.selected)];
    // Each sidebar colour with the main one it stands in for: a pair the main rule has already
    // checked (a fallback on a surface the content has too) is not reported twice.
    let text = [
        ("sidebar_text", s.text, p.text),
        ("sidebar_text_2", s.text_2, p.text_2),
        ("sidebar_text_3", s.text_3, p.text_3),
    ];
    let checked = |fg: Rgba, main: Rgba, bg: Rgba| fg == main && p.surfaces().iter().any(|(_, surface)| *surface == bg);
    for (name, fg, main) in text {
        for (surface, bg) in surfaces {
            let ratio = fg.contrast(bg);
            if ratio < TEXT_MIN && !checked(fg, main, bg) {
                problems.push(DesignError::TextContrast { fg: key(name), on: key(surface), ratio });
            }
        }
    }

    // Graphics only where the sidebar has a surface of its own: on `bg` they never had a rule, and
    // packs made before sidebar keys must stay valid.
    if !p.own_sidebar() {
        return;
    }
    let mut controls = vec![("focus_ring", s.focus_ring), ("accent", s.accent), ("merged", s.merged)];
    if mode == Mode::Dark {
        controls.extend([("ok", s.ok), ("attention", s.attention), ("error", s.error), ("inactive", s.inactive)]);
    }
    let calendar = ["calendar[1]", "calendar[2]", "calendar[3]", "calendar[4]", "calendar[5]"];
    controls.extend(calendar.into_iter().zip(s.calendar));
    for (name, fg) in controls {
        let ratio = fg.contrast(s.bg);
        // Too faint on `panel` already: `colours` named it; the accent is held to `sidebar` only.
        let named = name != "accent" && fg.contrast(p.panel) < CONTROL_MIN;
        if ratio < CONTROL_MIN && !named {
            problems.push(DesignError::ControlContrast { fg: key(name), on: key("sidebar"), ratio });
        }
    }
}

/// The edge of a chosen chip (on `selected`, `panel` or `menu`) stands out at least three times as
/// much as a plain frame (distance from 1:1) and less than the focus ring.
fn chosen_edge_stands_out(p: &Palette) -> bool {
    [p.selected, p.panel, p.menu].into_iter().all(|surface| {
        let ratio = |edge: Rgba| edge.contrast(surface);
        let (frame, chosen, focus) = (ratio(p.line_strong), ratio(p.line_selected), ratio(p.focus_ring));
        chosen - 1.0 >= 3.0 * (frame - 1.0) && chosen < focus
    })
}

fn typo(design: &Design, problems: &mut Vec<DesignError>) {
    for (name, level) in design.typo.levels() {
        let key = format!("type.{name}");
        let (min, max) = match name {
            "body" => (12.0, 18.0),
            // Reading text: at least `body` (a `body` out of its range is its own problem), and no
            // headline.
            "read" => {
                let body = design.typo.body.size;
                (if (12.0..=18.0).contains(&body) { body } else { 12.0 }, 20.0)
            }
            // A reminder over the whole screen (task ER1.5): read from across the room, but no poster.
            "hero" => (32.0, 72.0),
            "countdown" => (20.0, 48.0),
            _ => (11.0, 40.0),
        };
        range(&format!("{key}.size"), level.size, min, max, problems);
        if !(level.size..=2.0 * level.size).contains(&level.line) {
            problems.push(DesignError::LineHeight { key, line: level.line, size: level.size });
        }
    }
}

fn scales(design: &Design, problems: &mut Vec<DesignError>) {
    let s = design.space;
    let r = design.radius;
    for (key, values, min, max) in [
        ("space.scale", &[s.xs, s.s, s.m, s.l, s.xl, s.xxl][..], 2.0, 64.0),
        ("radius.scale", &[r.xs, r.sm, r.md, r.lg][..], 0.0, 16.0),
    ] {
        for (i, value) in values.iter().enumerate() {
            range(&format!("{key}[{}]", i + 1), *value, min, max, problems);
        }
        if !values.windows(2).all(|pair| pair[0] < pair[1]) {
            problems.push(DesignError::NotAscending { key: key.to_string() });
        }
    }
}

fn layout(design: &Design, problems: &mut Vec<DesignError>) {
    let l = design.layout;
    range("layout.inset", l.inset, 0.0, 24.0, problems);
    for (key, pad) in [
        ("layout.page_padding", l.page_padding),
        ("layout.panel_padding", l.panel_padding),
        ("layout.row_padding", l.row_padding),
        ("layout.control_padding", l.control_padding),
    ] {
        range(&format!("{key}[1]"), pad.vertical, 0.0, 48.0, problems);
        range(&format!("{key}[2]"), pad.horizontal, 0.0, 48.0, problems);
    }
    range("layout.section_gap", l.section_gap, 8.0, 64.0, problems);
    range("layout.item_gap", l.item_gap, 2.0, 24.0, problems);
}

/// The ranges of the concept; sizes it does not name get a range around their role (marked).
fn size_range(key: &str) -> (f32, f32) {
    match key {
        // Click targets: at least 24 (the concept names no maximum; 96 keeps them controls).
        "control_sm" | "control" | "control_lg" | "control_xl" | "row_group" => (24.0, 96.0),
        "row" => (32.0, 64.0),
        "row_two" => (32.0, 96.0),
        // The inbox's rows (task E3.9): three and two lines of text with room around them.
        "row_message" => (72.0, 120.0),
        "row_notice" => (48.0, 96.0),
        "row_sidebar" => (24.0, 40.0),
        "header" => (36.0, 64.0),
        "sidebar" => (180.0, 320.0),
        "props" | "panel" | "list" | "list_medium" => (280.0, 520.0),
        "content_max" | "form_max" => (560.0, 1200.0),
        // The comfortable density of the inbox (task E8.2): a gap that still groups, avatars and
        // tiles that fit beside two or three lines of text.
        "card_gap" => (4.0, 32.0),
        "avatar" | "avatar_lg" => (24.0, 64.0),
        "tile_sm" => (16.0, 32.0),
        "tile" => (20.0, 48.0),
        "tile_lg" => (24.0, 64.0),
        // The mail window's large buttons (task E11.1): a command bar a step above an input and a
        // pill a step above a button, neither as tall as a row.
        "command" => (32.0, 64.0),
        "pill" => (28.0, 64.0),
        // Calendar grid: an hour and the time axis stay readable and fit a tile.
        _ => (24.0, 120.0),
    }
}

fn sizes(design: &Design, problems: &mut Vec<DesignError>) {
    for (name, value) in design.size.entries() {
        let (mut min, max) = size_range(name);
        // The figures of any valid font fit the time axis (task L.19).
        if name == "axis" || name == "axis_narrow" {
            min = min.max(axis_min(design));
        }
        range(&format!("size.{name}"), value, min, max, problems);
    }
    let s = design.size;
    for (key, values) in [
        ("size.avatar, size.avatar_lg", &[s.avatar, s.avatar_lg][..]),
        ("size.tile_sm, size.tile, size.tile_lg", &[s.tile_sm, s.tile, s.tile_lg][..]),
    ] {
        if !values.windows(2).all(|pair| pair[0] < pair[1]) {
            problems.push(DesignError::NotAscending { key: key.to_string() });
        }
    }
}

/// `value` within `min..=max`; NaN and infinities are outside.
fn range(key: &str, value: f32, min: f32, max: f32, problems: &mut Vec<DesignError>) {
    if !(min..=max).contains(&value) {
        problems.push(DesignError::OutOfRange { key: key.to_string(), value, min, max });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::design::ColorScheme;

    /// The colour rules of a colour scheme.
    fn check(scheme: &ColorScheme) -> Vec<DesignError> {
        colors(&scheme.colors)
    }

    #[test]
    fn the_standard_passes_its_own_rules() {
        assert_eq!(design(&Design::standard()), Vec::new());
        assert_eq!(colors(&ColorScheme::standard().colors), Vec::new());
        assert_eq!(font(None, 1.0, "Inter Tabular", Some(1328.0 / 2048.0)), Vec::new());
    }

    /// Task L.19: the time axis holds "00:00" in the widest figures any font may have; the standard's
    /// narrow axis (44) just does.
    #[test]
    fn the_time_axis_fits_the_widest_figures() {
        let standard = Design::standard();
        assert_eq!(axis_min(&standard), 44.0, "4.5 × 0.72 × 11 + 2 × 4 = 43.64");
        let mut d = standard.clone();
        d.size.axis_narrow = 43.0;
        d.typo.micro = super::super::Level { size: 12.0, line: 16.0, ..d.typo.micro };
        assert_eq!(
            design(&d),
            [DesignError::OutOfRange { key: "size.axis_narrow".into(), value: 43.0, min: 47.0, max: 120.0 },],
            "axis 52 still fits micro 12 (needs 47)"
        );
    }

    /// Task L.19: a font's own size adjustment stays in 0.85–1.25, and its figures stay within the
    /// bound after it.
    #[test]
    fn font_rules_hold_the_adjustment_and_the_figures() {
        assert_eq!(
            font(Some(1.3), 1.3, "Sample", None),
            [DesignError::OutOfRange { key: "size_adjust".into(), value: 1.3, min: 0.85, max: 1.25 }]
        );
        let inter_tabular = 1328.0 / 2048.0;
        assert_eq!(font(Some(1.1), 1.1, "Inter Tabular", Some(inter_tabular)), Vec::new(), "0.713");
        assert!(matches!(&font(Some(1.2), 1.2, "Inter Tabular", Some(inter_tabular))[..],
            [DesignError::DigitsTooWide { family, width, max: DIGIT_MAX_EM }]
            if family == "Inter Tabular" && (*width - 0.778).abs() < 0.001));
        assert!(
            !font(Some(0.85), 0.85, "Wide", Some(0.75)).is_empty(),
            "a factor below 1 does not narrow the figures: the axis is drawn at the floor of 11"
        );
    }

    #[test]
    fn the_standard_has_the_measured_text_contrasts() {
        // The skill's measured minimums (section 4), 0.05 below for the rounding there.
        let d = ColorScheme::standard();
        for (mode, floors) in [(Mode::Dark, [14.4, 8.4, 5.1]), (Mode::Light, [14.6, 6.4, 4.8])] {
            let p = d.colors.get(mode);
            for (fg, floor) in [p.text, p.text_2, p.text_3].into_iter().zip(floors) {
                let worst = p.surfaces().iter().map(|(_, s)| fg.contrast(*s)).fold(f32::INFINITY, f32::min);
                assert!(worst >= floor - 0.05, "{mode}: {worst:.2} below {floor}");
            }
        }
    }

    #[test]
    fn unread_blue_stands_out_on_every_mail_surface() {
        let scheme = ColorScheme::standard();
        for mode in [Mode::Dark, Mode::Light] {
            let p = scheme.colors.get(mode);
            for (name, surface) in
                [("panel", p.panel), ("raised", p.raised), ("hover", p.hover), ("selected", p.selected)]
            {
                assert!(p.unread.contrast(surface) >= 3.0, "{mode}: unread on {name}");
            }
        }
    }

    #[test]
    fn a_faint_unread_colour_is_refused_on_every_mail_surface() {
        let mut scheme = ColorScheme::standard();
        scheme.colors.dark.unread = Rgba::hex(0x222222);
        let surfaces: Vec<String> = check(&scheme)
            .into_iter()
            .filter_map(|problem| match problem {
                DesignError::ControlContrast { fg, on, .. } if fg == "colors.dark.unread" => Some(on),
                _ => None,
            })
            .collect();
        assert_eq!(surfaces, ["colors.dark.panel", "colors.dark.raised", "colors.dark.hover", "colors.dark.selected"]);
    }

    #[test]
    fn low_text_contrast_is_refused() {
        let mut d = ColorScheme::standard();
        d.colors.dark.text_3 = Rgba::hex(0x3A3A3A);
        let problems = check(&d);
        assert!(!problems.is_empty());
        assert!(
            problems.iter().all(|p| matches!(p, DesignError::TextContrast { fg, .. } if fg == "colors.dark.text_3"))
        );
    }

    #[test]
    fn a_sidebar_of_its_own_holds_text_and_graphics_to_it() {
        let mut d = ColorScheme::standard();
        let p = &mut d.colors.light;
        (p.sidebar, p.sidebar_hover, p.sidebar_selected) =
            (Rgba::hex(0x24292F), Rgba::hex(0x2F353C), Rgba::hex(0x3A4149));
        let found = check(&d);
        let on_sidebar = |fg: &str| {
            found.iter().any(|problem| match problem {
                DesignError::TextContrast { fg: f, on, .. } | DesignError::ControlContrast { fg: f, on, .. } => {
                    f == &format!("colors.light.{fg}") && on.starts_with("colors.light.sidebar")
                }
                _ => false,
            })
        };
        // The dark text of the light content, the translucent black focus ring, Torro red.
        for fg in ["sidebar_text", "sidebar_text_2", "sidebar_text_3", "focus_ring", "accent"] {
            assert!(on_sidebar(fg), "{fg}: {found:?}");
        }
        assert!(!on_sidebar("ok"), "light-mode status colours are not held to 3:1, as on panel");
    }

    #[test]
    fn without_a_sidebar_of_its_own_only_its_text_is_checked() {
        let mut d = ColorScheme::standard();
        d.colors.dark.sidebar_text_3 = Rgba::hex(0x3A3A3A);
        assert!(matches!(&check(&d)[..], [DesignError::TextContrast { fg, on, .. }, ..]
            if fg == "colors.dark.sidebar_text_3" && on == "colors.dark.sidebar"));
        assert!(check(&d).iter().all(|p| matches!(p, DesignError::TextContrast { .. })));
    }

    #[test]
    fn a_weak_on_accent_is_refused() {
        let mut d = ColorScheme::standard();
        d.colors.light.on_accent = Rgba::hex(0x000000);
        assert!(matches!(check(&d)[..], [DesignError::AccentContrast { mode: Mode::Light, .. }]));
    }

    /// Task E8.8b: a chart colour or the grey of "Other" below 3:1 on `panel` is refused.
    #[test]
    fn a_faint_chart_colour_is_refused() {
        let mut d = ColorScheme::standard();
        d.colors.dark.chart[1] = Rgba::hex(0x202020);
        d.colors.light.chart_other = Rgba::hex(0xEEEEEE);
        let keys: Vec<String> = check(&d)
            .into_iter()
            .map(|p| match p {
                DesignError::ControlContrast { fg, .. } => fg,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["colors.dark.chart[2]", "colors.light.chart_other"]);
    }

    #[test]
    fn a_faint_focus_ring_and_calendar_colour_are_refused() {
        let mut d = ColorScheme::standard();
        d.colors.dark.focus_ring = Rgba::white(0.1);
        d.colors.light.calendar[2] = Rgba::hex(0xF0F0F0);
        let keys: Vec<String> = check(&d)
            .into_iter()
            .filter_map(|p| match p {
                DesignError::ControlContrast { fg, .. } => Some(fg),
                _ => None,
            })
            .collect();
        assert_eq!(keys, ["colors.dark.focus_ring", "colors.light.calendar[3]"]);
    }

    #[test]
    fn a_chosen_edge_like_a_frame_is_refused() {
        let mut d = ColorScheme::standard();
        d.colors.dark.line_selected = d.colors.dark.line_strong;
        assert!(check(&d).contains(&DesignError::ChosenEdge { mode: Mode::Dark }));
    }

    #[test]
    fn type_rules() {
        let mut d = Design::standard();
        d.typo.caption.size = 9.0;
        d.typo.body.size = 20.0;
        d.typo.body.line = 20.0;
        d.typo.meta.line = 40.0;
        d.typo.title.weight = super::super::Weight::Bold;
        let problems = design(&d);
        assert!(problems.contains(&DesignError::OutOfRange {
            key: "type.caption.size".into(),
            value: 9.0,
            min: 11.0,
            max: 40.0
        }));
        assert!(problems.contains(&DesignError::OutOfRange {
            key: "type.body.size".into(),
            value: 20.0,
            min: 12.0,
            max: 18.0
        }));
        assert!(problems.contains(&DesignError::LineHeight { key: "type.meta".into(), line: 40.0, size: 12.0 }));
        assert_eq!(problems.len(), 3, "a weight is no rule of a design any more (task L.19): {problems:?}");
    }

    #[test]
    fn scales_must_grow_and_stay_in_range() {
        let mut d = Design::standard();
        d.space.m = 6.0;
        d.radius.lg = 20.0;
        let problems = design(&d);
        assert!(problems.contains(&DesignError::NotAscending { key: "space.scale".into() }));
        assert!(problems.contains(&DesignError::OutOfRange {
            key: "radius.scale[4]".into(),
            value: 20.0,
            min: 0.0,
            max: 16.0
        }));
    }

    #[test]
    fn layout_and_sizes_stay_in_range() {
        let mut d = Design::standard();
        d.layout.inset = 30.0;
        d.layout.item_gap = 1.0;
        d.size.row = 20.0;
        d.size.control_sm = 20.0;
        d.size.sidebar = f32::NAN;
        let keys: Vec<String> = design(&d)
            .into_iter()
            .map(|p| match p {
                DesignError::OutOfRange { key, .. } => key,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(keys, ["layout.inset", "layout.item_gap", "size.control_sm", "size.row", "size.sidebar"]);
    }

    /// Task E8.2: the reading level stays at least `body`, the card tokens stay in their ranges and
    /// the avatars and tiles grow from small to large.
    #[test]
    fn comfortable_tokens_keep_their_rules() {
        let mut d = Design::standard();
        d.typo.read.size = 12.0;
        d.typo.read.line = 20.0;
        d.size.card_gap = 2.0;
        d.size.tile = 40.0;
        d.size.avatar_lg = 30.0;
        let problems = design(&d);
        assert!(problems.contains(&DesignError::OutOfRange {
            key: "type.read.size".into(),
            value: 12.0,
            min: 13.0,
            max: 20.0
        }));
        assert!(problems.contains(&DesignError::OutOfRange {
            key: "size.card_gap".into(),
            value: 2.0,
            min: 4.0,
            max: 32.0
        }));
        assert!(problems.contains(&DesignError::NotAscending { key: "size.avatar, size.avatar_lg".into() }));
        assert!(problems.contains(&DesignError::NotAscending { key: "size.tile_sm, size.tile, size.tile_lg".into() }));
        assert_eq!(problems.len(), 4, "{problems:?}");
    }

    /// Task E11.1: the command bar and the pills stay in their ranges.
    #[test]
    fn the_mail_sizes_stay_in_range() {
        assert!(design(&Design::standard()).is_empty(), "the standard is valid");
        let mut d = Design::standard();
        d.size.command = 30.0;
        d.size.pill = 70.0;
        let keys: Vec<(String, f32, f32)> = design(&d)
            .into_iter()
            .map(|p| match p {
                DesignError::OutOfRange { key, min, max, .. } => (key, min, max),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(keys, [("size.command".into(), 32.0, 64.0), ("size.pill".into(), 28.0, 64.0)]);
    }
}
