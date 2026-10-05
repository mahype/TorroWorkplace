//! Torro theme tokens.
//!
//! The values come from `torro-design/tokens/tokens.json` (W3C design tokens). TUI and
//! GUI read them from here, so both look like the same family. Red is brand, never
//! status: status uses the system-like colours below.

/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Brand accent: selection, counters, the one loud moment per window.
pub const TORRO_RED: Rgb = Rgb(0xD5, 0x0C, 0x0C);
/// Hover and depth next to the accent.
pub const TORRO_RED_DEEP: Rgb = Rgb(0xA5, 0x0A, 0x0A);

/// Checked, connected, fine.
pub const STATUS_OK: Rgb = Rgb(0x28, 0xCD, 0x41);
/// Waiting for the user, unchecked, deviating.
pub const STATUS_ATTENTION: Rgb = Rgb(0xFF, 0x95, 0x00);
/// Broken, refused.
pub const STATUS_ERROR: Rgb = Rgb(0xFF, 0x3B, 0x30);
/// Not set up, not present.
pub const STATUS_INACTIVE: Rgb = Rgb(0x8E, 0x8E, 0x93);
