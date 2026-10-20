//! The colour scheme this process draws with (task L.21, concept `docs/design-pakete.md`): the
//! colours of both modes and the logo. One per process, beside the design ([`crate::design`]) and the
//! font ([`crate::font`]); [`active`] reads it, [`set`] replaces it.
//!
//! Views never read it directly: [`crate::theme::palette`] gives its colours in the mode of the theme,
//! and [`crate::shell`] draws its logo.

use std::sync::Arc;

pub use torro_core::design::ColorScheme;

use crate::design;

/// The colour scheme this process draws with. Cheap (an `Arc`); read it again after a switch instead
/// of keeping it.
pub fn active() -> Arc<ColorScheme> {
    design::with(|t| Arc::clone(&t.scheme))
}

/// Makes `scheme` the active one for the whole process: every colour token and the Iced themes follow
/// from the next call on; design and font stay. Takes a colour scheme that
/// `torro_core::design::load_color_scheme` accepted; nothing is checked here.
pub fn set(scheme: ColorScheme) {
    design::replace(None, None, Some(scheme));
}
