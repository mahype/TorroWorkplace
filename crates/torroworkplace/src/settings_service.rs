//! The core's side of the settings: reading, saving and applying them.
//!
//! Task S.1 in `docs/tasks/steuerzentrale.md`. `torro_core::settings` holds the type and the
//! file; this module applies changes: start at login (an XDG autostart entry on Linux,
//! `~/.config/autostart/torro-workplace.desktop`; macOS and Windows later). Hidden collections
//! are applied by `pim.rs`, which reads [`SettingsStore::current`] on every request.
//!
//! The file is not looked up here but handed in ([`SettingsStore`]): the daemon uses the user's
//! `settings.json`, tests a file in a temp directory, so no test reads or writes the real one
//! (task A.0d).
//!
//! Changes go through one lock: requests run concurrently, and two targeted changes
//! ([`SettingsStore::set`], [`SettingsStore::set_collection_shown`]) must not lose each other in
//! read, change, write.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use torro_core::settings::{self, Language, Settings};
use torro_core::theme::Appearance;
use torro_ipc::SettingKey;

/// Held while the settings file is read, changed and written. One lock for every store: in
/// production there is only the one file, and serialising test stores costs nothing.
static WRITING: Mutex<()> = Mutex::new(());

/// The settings file the core reads and changes. Cheap to clone; the core and the PIM service
/// hold one each for the same file (`Core::new` takes the PIM service's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsStore {
    /// `None` without a home directory: reads give the defaults, changes fail.
    file: Option<PathBuf>,
}

impl SettingsStore {
    /// The user's `settings.json` (`torro_core::settings::settings_file`), as the daemon uses it.
    pub fn system() -> SettingsStore {
        SettingsStore { file: settings::settings_file() }
    }

    /// The settings in `file`, for tests. Turning `start_at_login` on would still write the real
    /// autostart entry, so tests leave that setting alone.
    #[cfg(test)]
    pub fn at(file: PathBuf) -> SettingsStore {
        SettingsStore { file: Some(file) }
    }

    /// No file at all, for tests that do not look at the settings: reads give the defaults,
    /// changes fail.
    #[cfg(test)]
    pub fn without_file() -> SettingsStore {
        SettingsStore { file: None }
    }

    /// The settings as stored now; the defaults when there is no file or it cannot be read.
    pub fn current(&self) -> Settings {
        self.file.as_deref().map(load_from).unwrap_or_default()
    }

    /// Saves `new` and applies what changed compared to the stored settings.
    pub fn update(&self, new: Settings) -> Result<(), String> {
        self.change(|settings| *settings = new)
    }

    /// Sets one setting (`Request::SetSetting`). The value is checked against the key before the
    /// file is read; a value that does not fit is an error and changes nothing.
    pub fn set(&self, key: SettingKey, value: &serde_json::Value) -> Result<(), String> {
        let change = Change::parse(key, value)?;
        self.change(|settings| change.apply(settings))
    }

    /// Shows or hides one collection in all apps (`Request::SetCollectionShown`). Whether the id
    /// belongs to an account is the caller's check.
    pub fn set_collection_shown(&self, collection: &str, shown: bool) -> Result<(), String> {
        self.change(|settings| show_in(settings, collection, shown))
    }

    /// Takes `collections` out of the hidden ones, for an account that is gone: its ids mean
    /// nothing any more, and an account added later must not start with them hidden. Writes
    /// nothing when none of them was hidden.
    pub fn forget_collections(&self, collections: &[String]) -> Result<(), String> {
        self.change(|settings| settings.hidden_collections.retain(|id| !collections.contains(id)))
    }

    /// Reads the stored settings, lets `edit` change them, saves and applies the result. An edit
    /// that changes nothing writes nothing.
    fn change(&self, edit: impl FnOnce(&mut Settings)) -> Result<(), String> {
        // A panic while holding the lock leaves no half-written state behind (the file is written
        // atomically), so a poisoned lock is still good to use.
        let _writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
        let old = self.current();
        let mut new = old.clone();
        edit(&mut new);
        if new == old {
            return Ok(());
        }
        let file = self.file.as_deref().ok_or("no home directory, so no settings file")?;
        save_to(file, &new)?;
        if old.start_at_login != new.start_at_login {
            apply_start_at_login(new.start_at_login)?;
        }
        Ok(())
    }
}

/// One setting with its value, checked against its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Language(Language),
    Appearance(Appearance),
    StartAtLogin(bool),
}

impl Change {
    /// `value` as the type `key` takes, or why it does not fit.
    fn parse(key: SettingKey, value: &serde_json::Value) -> Result<Change, String> {
        let invalid = |err: serde_json::Error| format!("{value} is not a value for {key:?}: {err}");
        Ok(match key {
            SettingKey::Language => Change::Language(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::Appearance => Change::Appearance(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::StartAtLogin => Change::StartAtLogin(serde_json::from_value(value.clone()).map_err(invalid)?),
        })
    }

    /// Puts the value into its field.
    fn apply(self, settings: &mut Settings) {
        match self {
            Change::Language(language) => settings.language = language,
            Change::Appearance(appearance) => settings.appearance = appearance,
            Change::StartAtLogin(enabled) => settings.start_at_login = enabled,
        }
    }
}

/// Removes `collection` from the hidden ones, or adds it once.
fn show_in(settings: &mut Settings, collection: &str, shown: bool) {
    let hidden = &mut settings.hidden_collections;
    hidden.retain(|id| id != collection);
    if !shown {
        hidden.push(collection.to_string());
    }
}

// `load_from` and `save_to` follow the private helpers of `torro_core::settings`, which only work
// on the user's file. Same format, same rules: a missing or broken file gives the defaults, writes
// are atomic.

/// Reads `path`. A missing file is normal (first start); any other problem is reported and the
/// defaults are used, so a broken file never keeps the suite from starting.
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

/// Writes `settings` to a temporary file next to `path` and renames it over `path`, so a crash
/// never leaves half a file behind. The process id keeps two writers from sharing the temporary.
fn save_to(path: &Path, settings: &Settings) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("{} has no directory", path.display()))?;
    fs::create_dir_all(dir).map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
    let mut json = serde_json::to_string_pretty(settings).map_err(|err| err.to_string())?;
    json.push('\n');
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let written = write_synced(&tmp, &json).and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn write_synced(path: &Path, content: &str) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()
}

#[cfg(target_os = "linux")]
fn apply_start_at_login(enabled: bool) -> Result<(), String> {
    autostart::set_enabled(enabled)
}

#[cfg(not(target_os = "linux"))]
fn apply_start_at_login(_enabled: bool) -> Result<(), String> {
    eprintln!("start at login is not supported on this system yet");
    Ok(())
}

/// The XDG autostart entry (Linux): desktop environments and Hyprland's session start every
/// `.desktop` file found in `autostart/` at login.
#[cfg(target_os = "linux")]
mod autostart {
    use std::env;
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    const ENTRY_FILE: &str = "torro-workplace.desktop";

    /// Creates the entry when `enabled`, removes it otherwise. Uses the real environment.
    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let xdg = env::var_os("XDG_CONFIG_HOME");
        let home = env::var_os("HOME").map(PathBuf::from);
        let path = entry_path(xdg.as_deref(), home.as_deref()).ok_or("no home directory, so no autostart entry")?;
        if !enabled {
            return remove_entry(&path);
        }
        let exe = env::current_exe().map_err(|err| format!("cannot find the executable: {err}"))?;
        write_entry(&path, &desktop_entry(&exe)).map_err(|err| format!("cannot write {}: {err}", path.display()))
    }

    /// `$XDG_CONFIG_HOME/autostart/torro-workplace.desktop`, else `~/.config/autostart/…`. The XDG
    /// spec requires `XDG_CONFIG_HOME` to be absolute, so an empty or relative value counts as unset.
    fn entry_path(xdg_config_home: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf> {
        let config = xdg_config_home
            .map(Path::new)
            .filter(|dir| dir.is_absolute())
            .map(Path::to_path_buf)
            .or_else(|| home.map(|home| home.join(".config")))?;
        Some(config.join("autostart").join(ENTRY_FILE))
    }

    /// The `.desktop` file: starts `<exe> daemon` at login and stays out of the launcher menus.
    fn desktop_entry(exe: &Path) -> String {
        let exec = exec_arg(exe);
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Torro Workplace\n\
             Exec={exec} daemon\n\
             X-GNOME-Autostart-enabled=true\n\
             NoDisplay=true\n"
        )
    }

    /// The program part of `Exec=`. A path with spaces or characters the spec reserves is quoted.
    /// Inside the quotes `"`, `` ` ``, `$`, `\` and `%` need escaping, and because the value is a
    /// string as well, every backslash is doubled once more.
    fn exec_arg(exe: &Path) -> String {
        const RESERVED: &str = " \t\n\"'\\><~|&;*?#()$`%";
        let path = exe.to_string_lossy();
        if !path.chars().any(|c| RESERVED.contains(c)) {
            return path.into_owned();
        }
        let mut out = String::from("\"");
        for c in path.chars() {
            match c {
                '%' => out.push_str("%%"),
                '"' | '`' | '$' => {
                    out.push_str("\\\\");
                    out.push(c);
                }
                '\\' => out.push_str("\\\\\\\\"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    fn write_entry(path: &Path, content: &str) -> io::Result<()> {
        let dir = path.parent().ok_or_else(|| io::Error::other("autostart entry has no directory"))?;
        fs::create_dir_all(dir)?;
        fs::write(path, content)
    }

    /// Removing an entry that is not there is fine: the goal is that it is gone.
    fn remove_entry(path: &Path) -> Result<(), String> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("cannot remove {}: {err}", path.display())),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn path_follows_xdg_config_home() {
            let path = entry_path(Some(OsStr::new("/xdg")), Some(Path::new("/home/anna")));
            assert_eq!(path, Some(PathBuf::from("/xdg/autostart/torro-workplace.desktop")));
        }

        #[test]
        fn path_falls_back_to_home_config() {
            let path = entry_path(None, Some(Path::new("/home/anna")));
            assert_eq!(path, Some(PathBuf::from("/home/anna/.config/autostart/torro-workplace.desktop")));
        }

        #[test]
        fn relative_or_empty_xdg_config_home_counts_as_unset() {
            let home = Some(Path::new("/home/anna"));
            let expected = Some(PathBuf::from("/home/anna/.config/autostart/torro-workplace.desktop"));
            assert_eq!(entry_path(Some(OsStr::new("relative")), home), expected);
            assert_eq!(entry_path(Some(OsStr::new("")), home), expected);
        }

        #[test]
        fn no_xdg_and_no_home_gives_no_path() {
            assert_eq!(entry_path(None, None), None);
        }

        #[test]
        fn desktop_entry_has_the_expected_content() {
            let content = desktop_entry(Path::new("/usr/bin/torroworkplace"));
            let expected = "[Desktop Entry]\n\
                Type=Application\n\
                Name=Torro Workplace\n\
                Exec=/usr/bin/torroworkplace daemon\n\
                X-GNOME-Autostart-enabled=true\n\
                NoDisplay=true\n";
            assert_eq!(content, expected);
        }

        #[test]
        fn exec_quotes_paths_with_spaces() {
            assert_eq!(exec_arg(Path::new("/opt/Torro Apps/torroworkplace")), "\"/opt/Torro Apps/torroworkplace\"");
        }

        #[test]
        fn exec_escapes_quotes_dollar_percent_and_backslashes() {
            assert_eq!(exec_arg(Path::new("/a\"b")), "\"/a\\\\\"b\"");
            assert_eq!(exec_arg(Path::new("/a b$c")), "\"/a b\\\\$c\"");
            assert_eq!(exec_arg(Path::new("/a b%c")), "\"/a b%%c\"");
            assert_eq!(exec_arg(Path::new("/a b\\c")), "\"/a b\\\\\\\\c\"");
        }

        #[test]
        fn entry_is_created_and_removed() {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = entry_path(Some(dir.path().as_os_str()), None).expect("path");
            let content = desktop_entry(Path::new("/usr/bin/torroworkplace"));

            write_entry(&path, &content).expect("create entry");
            assert_eq!(fs::read_to_string(&path).expect("read entry"), content);

            remove_entry(&path).expect("remove entry");
            assert!(!path.exists());
            remove_entry(&path).expect("removing a missing entry is fine");
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A store on `settings.json` in a fresh temp directory, which the caller keeps alive.
    fn store() -> (tempfile::TempDir, SettingsStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore::at(dir.path().join("settings.json"));
        (dir, store)
    }

    fn file_exists(store: &SettingsStore) -> bool {
        store.file.as_deref().expect("a test store has a file").exists()
    }

    #[test]
    fn set_changes_only_the_named_setting() {
        let mut settings = Settings { hidden_collections: vec!["work".into()], ..Settings::default() };
        Change::parse(SettingKey::Language, &json!("german")).expect("valid language").apply(&mut settings);
        Change::parse(SettingKey::Appearance, &json!("dark")).expect("valid appearance").apply(&mut settings);
        Change::parse(SettingKey::StartAtLogin, &json!(true)).expect("valid flag").apply(&mut settings);
        assert_eq!(
            settings,
            Settings {
                language: Language::German,
                appearance: Appearance::Dark,
                start_at_login: true,
                hidden_collections: vec!["work".into()],
            }
        );
    }

    #[test]
    fn a_value_that_does_not_fit_is_refused() {
        assert!(Change::parse(SettingKey::Language, &json!("klingon")).is_err());
        assert!(Change::parse(SettingKey::StartAtLogin, &json!("yes")).is_err());
        assert!(Change::parse(SettingKey::Appearance, &json!(1)).is_err());
    }

    #[test]
    fn a_refused_value_writes_nothing() {
        let (_dir, store) = store();
        assert!(store.set(SettingKey::Language, &json!("klingon")).is_err());
        assert!(store.set(SettingKey::StartAtLogin, &json!("yes")).is_err());
        assert!(!file_exists(&store));
    }

    #[test]
    fn a_setting_is_stored_in_the_given_file() {
        let (_dir, store) = store();
        assert_eq!(store.current(), Settings::default(), "no file yet");
        store.set(SettingKey::Appearance, &json!("light")).expect("valid appearance");
        assert!(file_exists(&store));
        let reopened = SettingsStore::at(store.file.clone().expect("a file"));
        assert_eq!(reopened.current(), Settings { appearance: Appearance::Light, ..Settings::default() });
    }

    #[test]
    fn forgotten_collections_are_no_longer_hidden() {
        let (_dir, store) = store();
        store.set_collection_shown("work", false).expect("hide work");
        store.set_collection_shown("other", false).expect("hide other");
        store.forget_collections(&["work".into(), "home".into()]).expect("forget");
        assert_eq!(store.current().hidden_collections, ["other"]);
    }

    #[test]
    fn forgetting_what_is_not_hidden_writes_nothing() {
        let (_dir, store) = store();
        store.forget_collections(&["work".into()]).expect("nothing to forget");
        assert!(!file_exists(&store));
    }

    #[test]
    fn without_a_file_reads_give_defaults_and_changes_fail() {
        let store = SettingsStore::without_file();
        assert_eq!(store.current(), Settings::default());
        assert!(store.set(SettingKey::Appearance, &json!("dark")).is_err());
    }

    #[test]
    fn showing_and_hiding_keeps_each_id_once() {
        let mut settings = Settings { hidden_collections: vec!["work".into()], ..Settings::default() };
        show_in(&mut settings, "home", false);
        show_in(&mut settings, "home", false);
        assert_eq!(settings.hidden_collections, ["work", "home"]);
        show_in(&mut settings, "work", true);
        assert_eq!(settings.hidden_collections, ["home"]);
        show_in(&mut settings, "unknown", true);
        assert_eq!(settings.hidden_collections, ["home"]);
    }
}
