//! The tray icon of the daemon. `menu.rs` builds the menu as data, the same on every system;
//! the front ends only draw it: ksni on Linux (StatusNotifierItem over D-Bus, `linux.rs`) and
//! tray-icon on macOS and Windows (`desktop.rs`, on an event loop in the main thread). Other
//! systems run the daemon without an icon.

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod icon;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod menu;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{TrayHandle, spawn};

#[cfg(any(target_os = "macos", windows))]
mod desktop;
#[cfg(any(target_os = "macos", windows))]
pub use desktop::{Handle as DesktopHandle, Shell};
