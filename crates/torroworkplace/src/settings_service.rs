//! The core's side of the settings: reading, saving and applying them.
//!
//! Task S.1 in `docs/tasks/steuerzentrale.md`. `torro_core::settings` holds the type and the
//! file; this module applies changes: start at login (an XDG autostart entry on Linux,
//! `~/.config/autostart/torro-workplace.desktop`; macOS and Windows later). Hidden collections
//! are applied by `pim.rs`, which reads [`current`] on every request.
//!
//! Changes go through one lock: requests run concurrently, and two targeted changes
//! ([`set`], [`set_collection_shown`]) must not lose each other in read, change, write.

use std::sync::{Mutex, PoisonError};

use torro_core::settings::{self, Settings};
use torro_ipc::SettingKey;

/// Held while the settings file is read, changed and written.
static WRITING: Mutex<()> = Mutex::new(());

/// The settings as stored now.
pub fn current() -> Settings {
    settings::load()
}

/// Saves `new` and applies what changed compared to the stored settings.
pub fn update(new: Settings) -> Result<(), String> {
    change(|settings| {
        *settings = new;
        Ok(())
    })
}

/// Sets one setting (`Request::SetSetting`). A value that does not fit the key changes nothing.
pub fn set(key: SettingKey, value: &serde_json::Value) -> Result<(), String> {
    change(|settings| set_in(settings, key, value))
}

/// Shows or hides one collection in all apps (`Request::SetCollectionShown`).
pub fn set_collection_shown(collection: &str, shown: bool) -> Result<(), String> {
    change(|settings| {
        show_in(settings, collection, shown);
        Ok(())
    })
}

/// Reads the stored settings, lets `edit` change them, saves and applies the result.
fn change(edit: impl FnOnce(&mut Settings) -> Result<(), String>) -> Result<(), String> {
    // A panic while holding the lock leaves no half-written state behind (the file is written
    // atomically), so a poisoned lock is still good to use.
    let _writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
    let old = settings::load();
    let mut new = old.clone();
    edit(&mut new)?;
    settings::save(&new)?;
    if old.start_at_login != new.start_at_login {
        apply_start_at_login(new.start_at_login)?;
    }
    Ok(())
}

/// Puts `value` into the field `key` names.
fn set_in(settings: &mut Settings, key: SettingKey, value: &serde_json::Value) -> Result<(), String> {
    let invalid = |err: serde_json::Error| format!("{value} is not a value for {key:?}: {err}");
    match key {
        SettingKey::Language => settings.language = serde_json::from_value(value.clone()).map_err(invalid)?,
        SettingKey::Appearance => settings.appearance = serde_json::from_value(value.clone()).map_err(invalid)?,
        SettingKey::StartAtLogin => settings.start_at_login = serde_json::from_value(value.clone()).map_err(invalid)?,
    }
    Ok(())
}

/// Removes `collection` from the hidden ones, or adds it once.
fn show_in(settings: &mut Settings, collection: &str, shown: bool) {
    let hidden = &mut settings.hidden_collections;
    hidden.retain(|id| id != collection);
    if !shown {
        hidden.push(collection.to_string());
    }
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
    use torro_core::settings::Language;
    use torro_core::theme::Appearance;

    use super::*;

    #[test]
    fn set_changes_only_the_named_setting() {
        let mut settings = Settings { hidden_collections: vec!["work".into()], ..Settings::default() };
        set_in(&mut settings, SettingKey::Language, &json!("german")).expect("valid language");
        set_in(&mut settings, SettingKey::Appearance, &json!("dark")).expect("valid appearance");
        set_in(&mut settings, SettingKey::StartAtLogin, &json!(true)).expect("valid flag");
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
    fn a_value_that_does_not_fit_changes_nothing() {
        let mut settings = Settings::default();
        assert!(set_in(&mut settings, SettingKey::Language, &json!("klingon")).is_err());
        assert!(set_in(&mut settings, SettingKey::StartAtLogin, &json!("yes")).is_err());
        assert!(set_in(&mut settings, SettingKey::Appearance, &json!(1)).is_err());
        assert_eq!(settings, Settings::default());
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
