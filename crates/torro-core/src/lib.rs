//! Shared ground for every TorroWorkplace app.
//!
//! Each app is its own process and its own window. What they share lives here: the
//! app id a window manager matches on, where an app keeps its files on each system,
//! and the Torro theme tokens.

pub mod app;
pub mod paths;
pub mod theme;

pub use app::AppId;
