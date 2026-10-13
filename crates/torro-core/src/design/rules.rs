//! The value rules of the concept (`docs/design-pakete.md`, "Regeln"): contrasts, type, scales,
//! layout gaps and sizes. Pure checks on a merged [`Design`]; the file rules live in `files.rs`.
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

use super::{Design, DesignError, Mode, Palette, Rgba};

/// Text contrast on every surface (WCAG AA).
const TEXT_MIN: f32 = 4.5;
/// Contrast of graphics and controls (WCAG 1.4.11).
const CONTROL_MIN: f32 = 3.0;

/// Every value rule `design` breaks, in a stable order.
pub(super) fn check(design: &Design) -> Vec<DesignError> {
    let mut problems = Vec::new();
    for mode in [Mode::Dark, Mode::Light] {
        colours(mode, design.colors.get(mode), &mut problems);
    }
    typo(design, &mut problems);
    scales(design, &mut problems);
    layout(design, &mut problems);
    sizes(design, &mut problems);
    problems
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
    for (name, fg) in controls {
        let ratio = fg.contrast(p.panel);
        if ratio < CONTROL_MIN {
            problems.push(DesignError::ControlContrast { fg: key(name), on: key("panel"), ratio });
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
        let (min, max) = if name == "body" { (12.0, 18.0) } else { (11.0, 40.0) };
        range(&format!("{key}.size"), level.size, min, max, problems);
        if !(level.size..=2.0 * level.size).contains(&level.line) {
            problems.push(DesignError::LineHeight { key: key.clone(), line: level.line, size: level.size });
        }
        if design.fonts.get(level.font).face(level.weight).is_none() {
            problems.push(DesignError::MissingWeight { key, font: level.font, weight: level.weight });
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
        "control_sm" | "control" | "control_lg" | "row_group" => (24.0, 96.0),
        "row" => (32.0, 64.0),
        "row_two" => (32.0, 96.0),
        "row_sidebar" => (24.0, 40.0),
        "header" => (36.0, 64.0),
        "sidebar" => (180.0, 320.0),
        "props" | "panel" | "list" | "list_medium" => (280.0, 520.0),
        "content_max" | "form_max" => (560.0, 1200.0),
        // Calendar grid: an hour and the time axis stay readable and fit a tile.
        _ => (24.0, 120.0),
    }
}

fn sizes(design: &Design, problems: &mut Vec<DesignError>) {
    for (name, value) in design.size.entries() {
        let (min, max) = size_range(name);
        range(&format!("size.{name}"), value, min, max, problems);
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

    #[test]
    fn the_standard_passes_its_own_rules() {
        assert_eq!(check(&Design::standard()), Vec::new());
    }

    #[test]
    fn the_standard_has_the_measured_text_contrasts() {
        // The skill's measured minimums (section 4), 0.05 below for the rounding there.
        let d = Design::standard();
        for (mode, floors) in [(Mode::Dark, [14.4, 8.4, 5.1]), (Mode::Light, [14.6, 6.4, 4.8])] {
            let p = d.colors.get(mode);
            for (fg, floor) in [p.text, p.text_2, p.text_3].into_iter().zip(floors) {
                let worst = p.surfaces().iter().map(|(_, s)| fg.contrast(*s)).fold(f32::INFINITY, f32::min);
                assert!(worst >= floor - 0.05, "{mode}: {worst:.2} below {floor}");
            }
        }
    }

    #[test]
    fn low_text_contrast_is_refused() {
        let mut d = Design::standard();
        d.colors.dark.text_3 = Rgba::hex(0x3A3A3A);
        let problems = check(&d);
        assert!(!problems.is_empty());
        assert!(
            problems.iter().all(|p| matches!(p, DesignError::TextContrast { fg, .. } if fg == "colors.dark.text_3"))
        );
    }

    #[test]
    fn a_weak_on_accent_is_refused() {
        let mut d = Design::standard();
        d.colors.light.on_accent = Rgba::hex(0x000000);
        assert!(matches!(check(&d)[..], [DesignError::AccentContrast { mode: Mode::Light, .. }]));
    }

    #[test]
    fn a_faint_focus_ring_and_calendar_colour_are_refused() {
        let mut d = Design::standard();
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
        let mut d = Design::standard();
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
        let problems = check(&d);
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
        assert!(problems.iter().any(|p| matches!(p, DesignError::MissingWeight { key, .. } if key == "type.title")));
        assert_eq!(problems.len(), 4, "{problems:?}");
    }

    #[test]
    fn scales_must_grow_and_stay_in_range() {
        let mut d = Design::standard();
        d.space.m = 6.0;
        d.radius.lg = 20.0;
        let problems = check(&d);
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
        let keys: Vec<String> = check(&d)
            .into_iter()
            .map(|p| match p {
                DesignError::OutOfRange { key, .. } => key,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(keys, ["layout.inset", "layout.item_gap", "size.control_sm", "size.row", "size.sidebar"]);
    }
}
