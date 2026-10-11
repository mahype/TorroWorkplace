//! The Torro theme for Iced.
//!
//! Task K.1 in `docs/tasks/kalender-aufgaben.md` refines it (light variant, extended
//! palette, text sizes). Colours come from `torro_core::theme` and the calendar prototype
//! (`docs/prototypes/calendar/tagesspirale.html`). Torro red is the brand accent
//! (selection, the primary button), never a status colour.

use iced::{Color, Theme};
use torro_core::theme::{Rgb, STATUS_ATTENTION, STATUS_ERROR, STATUS_OK, TORRO_RED};

pub fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.0, rgb.1, rgb.2)
}

/// Window background (dark).
pub const BACKGROUND: Color = Color::from_rgb(0x0B as f32 / 255.0, 0x0E as f32 / 255.0, 0x13 as f32 / 255.0);
/// Cards, sidebars.
pub const SURFACE: Color = Color::from_rgb(0x12 as f32 / 255.0, 0x17 as f32 / 255.0, 0x1F as f32 / 255.0);
/// Grid lines, separators.
pub const LINE: Color = Color::from_rgb(0x23 as f32 / 255.0, 0x2B as f32 / 255.0, 0x39 as f32 / 255.0);
pub const TEXT: Color = Color::from_rgb(0xE9 as f32 / 255.0, 0xED as f32 / 255.0, 0xF5 as f32 / 255.0);
/// Secondary text.
pub const MUTED: Color = Color::from_rgb(0x8C as f32 / 255.0, 0x96 as f32 / 255.0, 0xA8 as f32 / 255.0);

/// The app theme.
pub fn theme() -> Theme {
    Theme::custom(
        "Torro",
        iced::theme::Palette {
            background: BACKGROUND,
            text: TEXT,
            primary: color(TORRO_RED),
            success: color(STATUS_OK),
            warning: color(STATUS_ATTENTION),
            danger: color(STATUS_ERROR),
        },
    )
}
