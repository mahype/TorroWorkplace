//! Test data: the token values `torro_gui` had as constants before design packs, copied verbatim
//! from `theme.rs`, `typo.rs`, `style.rs` and `shell.rs` at commit 86d9efc (task L.2).
//!
//! The test "same picture" in [`super`] compares them with what the standard design gives now, so
//! a change to `Design::standard()` or to the conversion in `torro_gui` that would alter the look
//! fails here. Never edit these values to make that test pass.
//!
//! Since issue #291 (task L.34) the standard has new measures on purpose (text 14/22, flush content,
//! rails of its own), read from `designs/torro/design.toml`: the type scale, spacing, radii, sizes
//! and paddings of before were removed here, and the tests hold the tokens to the file instead. The
//! colours and the fonts did not change and are still held to the values below.

use iced::font::Weight;
use iced::{Color, Font, Shadow, Vector};
use torro_core::theme::{Rgb, STATUS_ATTENTION, STATUS_ERROR, STATUS_INACTIVE, STATUS_OK, TORRO_RED};

use crate::theme::Palette;

pub const FONT: Font = Font::with_name("Inter");
pub const FONT_MEDIUM: Font = Font { weight: Weight::Medium, ..FONT };
pub const FONT_SEMIBOLD: Font = Font { weight: Weight::Semibold, ..FONT };
pub const FONT_TABULAR: Font = Font::with_name("Inter Tabular");
pub const FONT_TABULAR_MEDIUM: Font = Font { weight: Weight::Medium, ..FONT_TABULAR };

const fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.0, rgb.1, rgb.2)
}

const fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t)
}

const fn hex(rgb: u32) -> Color {
    Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

const fn white(alpha: f32) -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, alpha)
}

const fn black(alpha: f32) -> Color {
    Color::from_rgba(0.0, 0.0, 0.0, alpha)
}

const fn shadow(alpha: f32) -> Shadow {
    Shadow { color: black(alpha), offset: Vector::new(0.0, 12.0), blur_radius: 32.0 }
}

pub const DARK: Palette = Palette {
    bg: hex(0x08090A),
    panel: hex(0x0F1011),
    raised: hex(0x141518),
    hover: hex(0x16171A),
    selected: hex(0x1C1E22),
    menu: hex(0x17181B),
    text: hex(0xEDEEF0),
    text_2: hex(0xB4B8BF),
    text_3: hex(0x8A8F98),
    line: white(0.06),
    line_strong: white(0.10),
    line_selected: white(0.28),
    focus_ring: white(0.35),
    accent: color(TORRO_RED),
    on_accent: Color::WHITE,
    ok: color(STATUS_OK),
    attention: color(STATUS_ATTENTION),
    attention_text: color(STATUS_ATTENTION),
    error: color(STATUS_ERROR),
    error_text: color(STATUS_ERROR),
    inactive: color(STATUS_INACTIVE),
    merged: hex(0xA78BFA),
    unread: hex(0x66A3FF),
    calendar: [hex(0x5B8CFF), hex(0x3FB984), hex(0xE8A33D), hex(0xD96CA8), hex(0xA78BFA)],
    tint: 0.18,
    // New with E8.8b, nothing drew charts before: the standard's chart colours.
    chart: [hex(0x3B78D8), hex(0xC9701E), hex(0x2E9E6B), hex(0xB5509A)],
    category: [hex(0x4C8DF0), hex(0xCF8A2C), hex(0x2BA39A), hex(0xA99BD0)],
    chart_other: hex(0x5C6068),
    shadow: shadow(0.5),
    scrim: black(0.62),
    night: hex(0x121626),
    // New with K.33, nothing drew working hours before: the standard's grey.
    working_hours: white(0.28),
    // Before L.6 the sidebar drew with `bg`, `hover`, `selected`, the text colours and `line`.
    sidebar: hex(0x08090A),
    sidebar_hover: hex(0x16171A),
    sidebar_selected: hex(0x1C1E22),
    sidebar_text: hex(0xEDEEF0),
    sidebar_text_2: hex(0xB4B8BF),
    sidebar_text_3: hex(0x8A8F98),
    sidebar_line: white(0.06),
};

pub const LIGHT: Palette = Palette {
    bg: hex(0xF4F5F6),
    panel: hex(0xFFFFFF),
    raised: hex(0xF8F9FA),
    hover: hex(0xF0F1F3),
    selected: hex(0xE9EAED),
    menu: hex(0xFFFFFF),
    text: hex(0x18191B),
    text_2: hex(0x4F535A),
    text_3: hex(0x62666D),
    line: black(0.07),
    line_strong: black(0.11),
    line_selected: black(0.32),
    focus_ring: black(0.5),
    accent: color(TORRO_RED),
    on_accent: Color::WHITE,
    ok: color(STATUS_OK),
    attention: color(STATUS_ATTENTION),
    attention_text: mix(color(STATUS_ATTENTION), hex(0x18191B), 0.5),
    error: color(STATUS_ERROR),
    error_text: mix(color(STATUS_ERROR), hex(0x18191B), 0.3),
    inactive: color(STATUS_INACTIVE),
    merged: hex(0x7C5CE6),
    unread: hex(0x2563D8),
    calendar: [hex(0x3B6EF0), hex(0x1F9D68), hex(0xB06F12), hex(0xC2508F), hex(0x7C5CE6)],
    tint: 0.14,
    // New with E8.8b, as in the dark palette.
    chart: [hex(0x2F6BC9), hex(0xB8621A), hex(0x2A9466), hex(0xA8448C)],
    category: [hex(0x3D77CE), hex(0xB1741F), hex(0x238A82), hex(0x756990)],
    chart_other: hex(0x50545B),
    shadow: shadow(0.12),
    scrim: black(0.3),
    night: hex(0xE3E7F1),
    // New with K.33, as in the dark palette.
    working_hours: black(0.32),
    sidebar: hex(0xF4F5F6),
    sidebar_hover: hex(0xF0F1F3),
    sidebar_selected: hex(0xE9EAED),
    sidebar_text: hex(0x18191B),
    sidebar_text_2: hex(0x4F535A),
    sidebar_text_3: hex(0x62666D),
    sidebar_line: black(0.07),
};

/// The Iced palette the old `iced_theme` built from a [`Palette`].
pub fn iced_palette(p: &Palette) -> iced::theme::Palette {
    iced::theme::Palette {
        background: p.bg,
        text: p.text,
        primary: p.accent,
        success: p.ok,
        warning: p.attention,
        danger: p.error,
    }
}
