//! The Torro theme for Iced.
//!
//! Task K.1 in `docs/tasks/kalender-aufgaben.md` refines it (light variant, extended
//! palette, text sizes). Colours come from `torro_core::theme` and the calendar prototype
//! (`docs/prototypes/calendar/tagesspirale.html`). Torro red is the brand accent
//! (selection, the primary button), never a status colour.
//!
//! The named colour constants below are the dark variant. The light variant only changes the
//! Iced palette ([`light`]); apps that need a light-specific surface must pick their own colour.

use iced::{Color, Theme};
use torro_core::theme::{Rgb, STATUS_ATTENTION, STATUS_ERROR, STATUS_OK, TORRO_RED};

pub fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.0, rgb.1, rgb.2)
}

/// Window background (dark).
pub const BACKGROUND: Color = Color::from_rgb(0x0B as f32 / 255.0, 0x0E as f32 / 255.0, 0x13 as f32 / 255.0);
/// Cards, sidebars.
pub const SURFACE: Color = Color::from_rgb(0x12 as f32 / 255.0, 0x17 as f32 / 255.0, 0x1F as f32 / 255.0);
/// Raised surfaces inside a card: selected rows, hovered items, the active view tab (dark).
pub const SURFACE_2: Color = Color::from_rgb(0x18 as f32 / 255.0, 0x1E as f32 / 255.0, 0x29 as f32 / 255.0);
/// Grid lines, separators.
pub const LINE: Color = Color::from_rgb(0x23 as f32 / 255.0, 0x2B as f32 / 255.0, 0x39 as f32 / 255.0);
pub const TEXT: Color = Color::from_rgb(0xE9 as f32 / 255.0, 0xED as f32 / 255.0, 0xF5 as f32 / 255.0);
/// Secondary text.
pub const MUTED: Color = Color::from_rgb(0x8C as f32 / 255.0, 0x96 as f32 / 255.0, 0xA8 as f32 / 255.0);
/// Tertiary text and disabled hints: dates outside the month, placeholders (dark).
pub const FAINT: Color = Color::from_rgb(0x46 as f32 / 255.0, 0x50 as f32 / 255.0, 0x62 as f32 / 255.0);
/// The "now" line in the day and week views (dark).
pub const NOW: Color = Color::from_rgb(1.0, 1.0, 1.0);

/// Light background, from the light block of the calendar prototype.
const LIGHT_BACKGROUND: Color = Color::from_rgb(0xF2 as f32 / 255.0, 0xF4 as f32 / 255.0, 0xF8 as f32 / 255.0);
/// Light text, from the light block of the calendar prototype.
const LIGHT_TEXT: Color = Color::from_rgb(0x11 as f32 / 255.0, 0x17 as f32 / 255.0, 0x22 as f32 / 255.0);

/// The attention colour (waiting for the user, unchecked, deviating) as an Iced colour.
pub fn attention() -> Color {
    color(STATUS_ATTENTION)
}

/// The dark app theme.
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

/// The light app theme, with the palette values from the light block of the calendar prototype.
pub fn light() -> Theme {
    Theme::custom(
        "Torro Light",
        iced::theme::Palette {
            background: LIGHT_BACKGROUND,
            text: LIGHT_TEXT,
            primary: color(TORRO_RED),
            success: color(STATUS_OK),
            warning: color(STATUS_ATTENTION),
            danger: color(STATUS_ERROR),
        },
    )
}

/// The theme that follows the system's light or dark setting.
///
/// Light detection (the desktop's colour-scheme setting) is not built yet, so this returns the
/// dark theme for now. When detection lands, it switches between [`theme`] and [`light`] here
/// and every app picks it up.
pub fn for_system() -> Theme {
    theme()
}
