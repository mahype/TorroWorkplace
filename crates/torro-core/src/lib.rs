//! Shared ground for every TorroWorkplace app.
//!
//! Each app is its own process and its own window. What they share lives here: the
//! app id a window manager matches on, where an app keeps its files on each system,
//! the accounts all apps share, the types of AI accounts ([`ai`]), translations (English first, German as a translation), the
//! Torro theme tokens and the design packs that hold the whole look ([`design`]).

pub mod account;
pub mod ai;
pub mod app;
pub mod design;
pub mod i18n;
pub mod paths;
pub mod settings;
pub mod theme;

pub use account::Account;
pub use app::AppId;
