//! Shared ground for every TorroWorkplace app.
//!
//! Each app is its own process and its own window. What they share lives here: the
//! app id a window manager matches on, where an app keeps its files on each system,
//! the accounts all apps share, the types of AI accounts ([`ai`]), the words for a mail's category ([`category`]), translations (English first, German as a translation), the
//! Torro theme tokens, the design packs that hold the whole look ([`design`]), the links of "Open in Maps"
//! ([`maps`]), the keyboard shortcuts of the mail window ([`shortcuts`]), the timestamps that measure how fast the windows
//! start ([`timing`]), the checks on someone else's text before a person reads it ([`text`]), whether content of other
//! providers is loaded ([`privacy`]) and how
//! versions of the programs compare ([`version`]).

pub mod account;
pub mod ai;
pub mod app;
pub mod category;
pub mod design;
pub mod i18n;
pub mod maps;
pub mod paths;
pub mod privacy;
pub mod settings;
pub mod shortcuts;
pub mod text;
pub mod theme;
pub mod timing;
pub mod version;

pub use account::Account;
pub use app::AppId;
