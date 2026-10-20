//! The font this process draws with (task L.21, concept `docs/design-pakete.md`): the families of the
//! three roles text, heading and numbers, and the size adjustment that makes every level of the design
//! look as large as in Inter. One per process, beside the design ([`crate::design`]) and the colour
//! scheme ([`crate::scheme`]); [`active`] reads it, [`set`] replaces it.
//!
//! The levels of [`crate::typo`] and [`crate::theme::font`] take their families from it. Its files
//! are loaded once per font: at start by [`crate::settings`], on a switch by
//! [`crate::design::load_fonts`] (which [`crate::design::Follow`] and [`crate::design::show`] call).

use std::sync::Arc;

pub use torro_core::design::FontSet;

use crate::design;

/// The font this process draws with. Cheap (an `Arc`); read it again after a switch instead of
/// keeping it.
pub fn active() -> Arc<FontSet> {
    design::with(|t| Arc::clone(&t.font))
}

/// Makes `font` the active one for the whole process: every level is drawn in it from the next call
/// on; design and colour scheme stay. Takes a font that `torro_core::design::load_font` accepted;
/// nothing is checked here.
///
/// Its files are not loaded here: return [`crate::design::load_fonts`] from `update` as well, or use
/// [`crate::design::show`], which does both.
pub fn set(font: FontSet) {
    design::replace(None, Some(font), None);
}
