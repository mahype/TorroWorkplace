//! Shared ground for every TorroWorkplace app.
//!
//! Each app is its own process and its own window. What they share lives here: the
//! app id a window manager matches on, where an app keeps its files on each system,
//! the accounts all apps share, the types of AI accounts ([`ai`]), the words for a mail's category ([`category`]), translations (English first, German as a translation), the
//! Torro theme tokens, the design packs that hold the whole look ([`design`]), the keyboard
//! shortcuts of the mail window ([`shortcuts`]) and the timestamps that measure how fast the windows
//! start ([`timing`]).

pub mod account;
pub mod ai;
pub mod app;
pub mod category;
pub mod design;
pub mod i18n;
pub mod paths;
pub mod settings;
pub mod shortcuts;
pub mod theme;
pub mod timing;

pub use account::Account;
pub use app::AppId;
