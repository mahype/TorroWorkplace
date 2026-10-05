//! App identity.
//!
//! The app id is the one name a window carries everywhere: the Wayland app id and X11
//! class on Linux (Hyprland window rules match on it), the terminal `--class` a TUI is
//! started with, the key in AeroSpace's `on-window-detected`, and what workspace-setups
//! stores to find the window again. It never changes once an app has shipped.

/// An app of the suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppId {
    /// Window class / app id, e.g. `torro-tasks`.
    pub window_class: &'static str,
    /// Binary name, e.g. `torrotasks`.
    pub binary: &'static str,
    /// Directory name below the per-OS config, data and state roots.
    pub dir: &'static str,
    /// Human name for titles and launchers.
    pub display_name: &'static str,
}

impl AppId {
    pub const TASKS: AppId =
        AppId { window_class: "torro-tasks", binary: "torrotasks", dir: "torrotasks", display_name: "Torro Tasks" };
    pub const CALENDAR: AppId = AppId {
        window_class: "torro-calendar",
        binary: "torrocalendar",
        dir: "torrocalendar",
        display_name: "Torro Calendar",
    };

    /// Every app of the suite, in launcher order.
    pub const ALL: &'static [AppId] = &[AppId::TASKS, AppId::CALENDAR];
}
