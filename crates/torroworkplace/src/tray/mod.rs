//! The tray icon of the daemon. Only Linux has an implementation so far (ksni, over the
//! StatusNotifierItem protocol on D-Bus); `main` reports "tray not supported yet" elsewhere.

#[cfg(target_os = "linux")]
mod icon;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::run;
