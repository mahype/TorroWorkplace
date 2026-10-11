//! The core's side of the settings: reading, saving and applying them.
//!
//! Task S.1 in `docs/tasks/steuerzentrale.md`. `torro_core::settings` holds the type and the
//! file; this module applies changes: start at login (an XDG autostart entry on Linux,
//! `~/.config/autostart/torro-workplace.desktop`; macOS and Windows later). Hidden collections
//! are applied by `pim.rs`, which reads [`current`] on every request.

use torro_core::settings::{self, Settings};

/// The settings as stored now.
pub fn current() -> Settings {
    settings::load()
}

/// Saves `new` and applies what changed compared to the stored settings.
pub fn update(new: Settings) -> Result<(), String> {
    let _ = new;
    Err("settings_service::update not implemented yet (task S.1)".into())
}
