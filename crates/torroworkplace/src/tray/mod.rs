//! The tray icon of the daemon. Only Linux has an implementation so far (ksni, over the
//! StatusNotifierItem protocol on D-Bus); elsewhere the daemon runs without an icon until the
//! `tray-icon` front end for macOS and Windows follows.

#[cfg(target_os = "linux")]
mod icon;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{TrayHandle, spawn};
