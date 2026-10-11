//! Shared pieces for the Iced apps of the suite (calendar, tasks, later the control
//! centre), so they look and behave like one family when tiled side by side.
//!
//! - [`source::Source`]: where data comes from (the core, or demo data with `--demo`).
//! - [`theme`]: the Torro theme from the design tokens.
//! - [`connect`]: the "connect your Nextcloud" screen every app shows without an account.
//! - [`datetime`]: German date and time parsing and formatting, week arithmetic.
//! - [`widgets`]: small building blocks (colour dots, toolbar buttons, empty states).

pub mod connect;
pub mod datetime;
pub mod source;
pub mod theme;
pub mod widgets;

/// Window settings for an app: its fixed app id (Wayland app id / X11 class, which
/// Hyprland rules and workspace templates match on) and a sensible default size.
pub fn window(app: torro_core::AppId) -> iced::window::Settings {
    iced::window::Settings {
        size: iced::Size::new(1100.0, 760.0),
        min_size: Some(iced::Size::new(360.0, 320.0)),
        #[cfg(target_os = "linux")]
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: app.window_class.to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}
