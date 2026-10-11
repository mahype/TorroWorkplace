//! Suite-wide settings, set in the control centre (`torroworkplace ui`) and read by every
//! part of the suite: `settings.json` in the core's config directory.
//!
//! Task S.1 in `docs/tasks/steuerzentrale.md` fills in [`load`] and [`save`]. The type is the
//! contract: add fields with `#[serde(default)]` so older files keep loading.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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
    match settings_file() {
        Some(path) => load_from(&path),
        None => Settings::default(),
    }
}

/// Writes the settings atomically (task S.1).
pub fn save(settings: &Settings) -> Result<(), String> {
    let path = settings_file().ok_or("no home directory, so no settings file")?;
    save_to(&path, settings)
}

/// Reads `path`. A missing file is normal (first start); any other problem is reported once
/// and the defaults are used, so a broken file never keeps the suite from starting.
fn load_from(path: &Path) -> Settings {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            if err.kind() != io::ErrorKind::NotFound {
                eprintln!("torroworkplace: cannot read {}: {err}", path.display());
            }
            return Settings::default();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|err| {
        eprintln!("torroworkplace: ignoring {}, it is not valid settings JSON: {err}", path.display());
        Settings::default()
    })
}

fn save_to(path: &Path, settings: &Settings) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("{} has no directory", path.display()))?;
    fs::create_dir_all(dir).map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
    let mut json = serde_json::to_string_pretty(settings).map_err(|err| err.to_string())?;
    json.push('\n');
    write_atomically(path, &json).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

/// Writes to a temporary file next to `path` and renames it over `path`, so a crash never
/// leaves half a file behind. The process id keeps two writers from sharing a temporary file.
fn write_atomically(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let written = write_synced(&tmp, content).and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn write_synced(path: &Path, content: &str) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory under the system temp dir, removed again on drop. torro-core has no
    /// `tempfile` dependency, so the tests use this small helper instead.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("torro-settings-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("create temp dir");
            TempDir(dir)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = TempDir::new("missing");
        assert_eq!(load_from(&dir.file("settings.json")), Settings::default());
    }

    #[test]
    fn invalid_json_loads_defaults() {
        let dir = TempDir::new("invalid");
        let path = dir.file("settings.json");
        fs::write(&path, "{ not json").expect("write");
        assert_eq!(load_from(&path), Settings::default());
    }

    #[test]
    fn valid_file_loads_its_values() {
        let dir = TempDir::new("valid");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"language":"german","startAtLogin":true,"hiddenCollections":["work"]}"#).expect("write");
        let expected =
            Settings { language: Language::German, start_at_login: true, hidden_collections: vec!["work".into()] };
        assert_eq!(load_from(&path), expected);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let dir = TempDir::new("unknown");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"language":"english","someFutureField":42}"#).expect("write");
        let loaded = load_from(&path);
        assert_eq!(loaded.language, Language::English);
        assert!(!loaded.start_at_login);
    }

    #[test]
    fn missing_fields_get_defaults() {
        let dir = TempDir::new("missing-fields");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"startAtLogin":true}"#).expect("write");
        let expected = Settings { start_at_login: true, ..Settings::default() };
        assert_eq!(load_from(&path), expected);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new("round-trip");
        // The directory does not exist yet; saving creates it.
        let path = dir.file("nested/settings.json");
        let first = Settings {
            language: Language::English,
            start_at_login: true,
            hidden_collections: vec!["a".into(), "b".into()],
        };
        save_to(&path, &first).expect("save");
        assert_eq!(load_from(&path), first);

        let second = Settings::default();
        save_to(&path, &second).expect("overwrite");
        assert_eq!(load_from(&path), second);
    }

    #[test]
    fn save_leaves_no_temporary_file_behind() {
        let dir = TempDir::new("no-tmp");
        let path = dir.file("settings.json");
        save_to(&path, &Settings::default()).expect("save");
        let entries: Vec<_> = fs::read_dir(&dir.0).expect("read dir").map(|e| e.expect("entry").file_name()).collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("settings.json")]);
    }

    #[test]
    fn saved_file_is_pretty_json() {
        let dir = TempDir::new("pretty");
        let path = dir.file("settings.json");
        save_to(&path, &Settings::default()).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("\n  \"language\""), "expected indented JSON, got {text:?}");
        assert!(text.contains("\"startAtLogin\""));
    }
}
