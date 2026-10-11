//! Suite-wide settings, set in the control centre (`torroworkplace ui`) and read by every
//! part of the suite: `settings.json` in the core's config directory.
//!
//! Task S.1 in `docs/tasks/steuerzentrale.md` fills in [`load`] and [`save`]. The type is the
//! contract: add fields with `#[serde(default)]` so older files keep loading.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::AppId;
use crate::paths::AppPaths;

/// The language the suite speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Follow the system (`LANG` and friends).
    #[default]
    System,
    English,
    German,
}

/// All suite-wide settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub language: Language,
    /// Start `torroworkplace daemon` when the user logs in.
    #[serde(default)]
    pub start_at_login: bool,
    /// Collections (calendar and task list ids) the apps do not show.
    #[serde(default)]
    pub hidden_collections: Vec<String>,
}

/// `settings.json` in the core's config directory; `None` without a home directory.
pub fn settings_file() -> Option<PathBuf> {
    AppPaths::for_app(AppId::WORKPLACE).map(|p| p.config.join("settings.json"))
}

/// The stored settings; defaults when there is no file or it cannot be read (task S.1).
pub fn load() -> Settings {
    Settings::default()
}

/// Writes the settings atomically (task S.1).
pub fn save(settings: &Settings) -> Result<(), String> {
    let _ = settings;
    Err("settings::save not implemented yet (task S.1)".into())
}
