//! Where an app keeps its files.
//!
//! Linux follows XDG (`~/.config`, `~/.local/share`, `~/.local/state`), macOS keeps all
//! three under `~/Library/Application Support/<app>`, Windows under `%APPDATA%\<app>`.
//! `TORRO_{CONFIG,DATA,STATE}_HOME` override the roots, which tests and portable
//! installs use.

use std::env;
use std::path::PathBuf;

use crate::AppId;

/// The three directories of one app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// Settings the user edits: accounts, keymap.
    pub config: PathBuf,
    /// What the app owns: the offline cache.
    pub data: PathBuf,
    /// What may be lost: locks, sockets, last view.
    pub state: PathBuf,
}

impl AppPaths {
    /// The directories of `app`, or `None` when no home directory is known.
    pub fn for_app(app: AppId) -> Option<AppPaths> {
        let home = home_dir()?;
        let (config, data, state) = roots(&home);
        Some(AppPaths {
            config: env_root("TORRO_CONFIG_HOME").unwrap_or(config).join(app.dir),
            data: env_root("TORRO_DATA_HOME").unwrap_or(data).join(app.dir),
            state: env_root("TORRO_STATE_HOME").unwrap_or(state).join(app.dir),
        })
    }
}

fn env_root(name: &str) -> Option<PathBuf> {
    env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env_root(var)
}

#[cfg(target_os = "macos")]
fn roots(home: &std::path::Path) -> (PathBuf, PathBuf, PathBuf) {
    let support = home.join("Library/Application Support");
    (support.clone(), support.clone(), support)
}

#[cfg(windows)]
fn roots(home: &std::path::Path) -> (PathBuf, PathBuf, PathBuf) {
    let appdata = env_root("APPDATA").unwrap_or_else(|| home.join("AppData/Roaming"));
    let local = env_root("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData/Local"));
    (appdata.clone(), appdata, local)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn roots(home: &std::path::Path) -> (PathBuf, PathBuf, PathBuf) {
    (
        env_root("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
        env_root("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")),
        env_root("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state")),
    )
}
