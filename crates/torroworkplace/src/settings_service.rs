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

use torro_core::settings::{self, Language, ListOrderEntry, Night, Settings, Zoom};
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
    ///
    /// A hidden collection named by a bare URL, as before several accounts existed, is refused
    /// and nothing is written (task M1.4): the old ids were rewritten once when the core started,
    /// and one coming back later would otherwise belong to no account, or by chance to the first.
    pub fn update(&self, new: Settings) -> Result<(), String> {
        let ids = new.hidden_collections.iter().chain(new.list_orders.keys());
        if let Some(bare) = ids.into_iter().find(|id| torro_pim::account::is_legacy_id(id)) {
            return Err(format!("{bare:?} is not a collection id; use the ids from accounts.list_accounts"));
        }
        self.change(|settings| *settings = new)
    }

    /// Like [`SettingsStore::update`], but the hidden collections and the orders of the task
    /// lists stay as stored, read under the same lock as the write, so a collection hidden
    /// meanwhile is not shown again. For clients, which never see the hidden collections nor the
    /// orders of lists outside their selection, and so cannot send them back; they change those
    /// with `set_collection_shown` and `set_setting`.
    pub fn update_keeping_hidden(&self, new: Settings) -> Result<(), String> {
        self.change(|settings| {
            let hidden = std::mem::take(&mut settings.hidden_collections);
            let list_orders = std::mem::take(&mut settings.list_orders);
            *settings = Settings { hidden_collections: hidden, list_orders, ..new };
        })
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

    /// Takes `collections` out of the hidden ones and the list orders, for an account that is
    /// gone: its ids mean nothing any more, and an account added later must not start with them.
    /// Writes nothing when none of them was named.
    pub fn forget_collections(&self, collections: &[String]) -> Result<(), String> {
        self.change(|settings| {
            settings.hidden_collections.retain(|id| !collections.contains(id));
            settings.list_orders.retain(|id, _| !collections.contains(id));
        })
    }

    /// Puts `owner` in front of every hidden collection whose id is still a bare URL from before
    /// several accounts existed (task M1.1, [`torro_pim::account::qualify_legacy_id`]): back then
    /// there was one account, `owner`. Runs once when the core starts; writes the file (atomically,
    /// like every change) only when something changed.
    pub fn qualify_legacy_collections(&self, owner: &str) -> Result<(), String> {
        self.change(|settings| {
            for id in &mut settings.hidden_collections {
                if let Some(qualified) = torro_pim::account::qualify_legacy_id(id, owner) {
                    *id = qualified;
                }
            }
        })
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
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    Language(Language),
    Appearance(Appearance),
    StartAtLogin(bool),
    Zoom(Zoom),
    Night(Night),
    /// Whether the list is known is the caller's check (`Core`), like for hidden collections.
    ListOrder(ListOrderEntry),
    QuietWhenBusy(bool),
}

impl Change {
    /// `value` as the type `key` takes, or why it does not fit.
    fn parse(key: SettingKey, value: &serde_json::Value) -> Result<Change, String> {
        let invalid = |err: serde_json::Error| format!("{value} is not a value for {key:?}: {err}");
        Ok(match key {
            SettingKey::Language => Change::Language(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::Appearance => Change::Appearance(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::StartAtLogin => Change::StartAtLogin(serde_json::from_value(value.clone()).map_err(invalid)?),
            // Not through `Zoom`'s own reading, which clamps any number for the sake of old files:
            // a value from outside must be in range.
            SettingKey::Zoom => Change::Zoom(
                value
                    .as_f64()
                    .and_then(Zoom::from_factor)
                    .ok_or_else(|| format!("{value} is not a zoom: send a number from 0.67 to 2.0"))?,
            ),
            // Strict as well: the file's reading falls back to 22:00–06:00 instead.
            SettingKey::Night => Change::Night(Night::from_value(value)?),
            SettingKey::ListOrder => Change::ListOrder(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::QuietWhenBusy => Change::QuietWhenBusy(serde_json::from_value(value.clone()).map_err(invalid)?),
        })
    }

    /// Puts the value into its field.
    fn apply(self, settings: &mut Settings) {
        match self {
            Change::Language(language) => settings.language = language,
            Change::Appearance(appearance) => settings.appearance = appearance,
            Change::StartAtLogin(enabled) => settings.start_at_login = enabled,
            Change::Zoom(zoom) => settings.zoom = zoom,
            Change::Night(night) => settings.night = night,
            Change::ListOrder(entry) => settings.set_list_order(&entry.list, entry.order),
            Change::QuietWhenBusy(quiet) => settings.quiet_when_busy = quiet,
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
        Change::parse(SettingKey::Zoom, &json!(1.25)).expect("valid zoom").apply(&mut settings);
        let night = json!({"from": "23:30", "to": "07:00"});
        Change::parse(SettingKey::Night, &night).expect("valid night").apply(&mut settings);
        Change::parse(SettingKey::QuietWhenBusy, &json!(false)).expect("valid flag").apply(&mut settings);
        assert_eq!(
            settings,
            Settings {
                language: Language::German,
                appearance: Appearance::Dark,
                start_at_login: true,
                hidden_collections: vec!["work".into()],
                zoom: Zoom::nearest(1.25),
                night: Night::from_value(&night).expect("valid night"),
                list_orders: Default::default(),
                quiet_when_busy: false,
            }
        );
        assert!(Change::parse(SettingKey::QuietWhenBusy, &json!("off")).is_err(), "only true or false");
    }

    #[test]
    fn a_list_order_is_set_and_reset_and_kept_from_clients() {
        use torro_core::settings::ListOrder;
        let (_dir, store) = store();
        store.set(SettingKey::ListOrder, &json!({"list": "work", "order": "manual"})).expect("manual");
        assert_eq!(store.current().list_order("work"), ListOrder::Manual);
        assert!(store.set(SettingKey::ListOrder, &json!({"list": "work", "order": "random"})).is_err());
        assert!(store.set(SettingKey::ListOrder, &json!("manual")).is_err(), "the list is missing");
        // A client sends back what it saw, without the orders: they stay.
        store.update_keeping_hidden(Settings::default()).expect("updated");
        assert_eq!(store.current().list_order("work"), ListOrder::Manual);
        store.forget_collections(&["work".into()]).expect("account gone");
        assert!(store.current().list_orders.is_empty());
        store.set(SettingKey::ListOrder, &json!({"list": "home", "order": "manual"})).expect("manual");
        store.set(SettingKey::ListOrder, &json!({"list": "home", "order": "due_date"})).expect("back");
        assert!(store.current().list_orders.is_empty(), "the default is not stored");
    }

    #[test]
    fn a_list_order_with_a_bare_url_is_refused() {
        use torro_core::settings::ListOrder;
        let (_dir, store) = store();
        let mut settings = Settings::default();
        settings.set_list_order("https://cloud.example.org/remote.php/dav/calendars/anna/tasks/", ListOrder::Manual);
        assert!(store.update(settings).is_err());
        assert!(!file_exists(&store));
    }

    #[test]
    fn an_update_from_a_client_keeps_the_hidden_collections() {
        let (_dir, store) = store();
        store.set_collection_shown("secret", false).expect("hidden");
        let new = Settings { language: Language::German, hidden_collections: vec![], ..Settings::default() };
        store.update_keeping_hidden(new).expect("updated");
        let stored = store.current();
        assert_eq!((stored.language, stored.hidden_collections), (Language::German, vec!["secret".to_string()]));
    }

    #[test]
    fn a_value_that_does_not_fit_is_refused() {
        assert!(Change::parse(SettingKey::Language, &json!("klingon")).is_err());
        assert!(Change::parse(SettingKey::StartAtLogin, &json!("yes")).is_err());
        assert!(Change::parse(SettingKey::Appearance, &json!(1)).is_err());
        for bad in [json!(0.5), json!(2.5), json!("1.1"), json!(true), json!(null)] {
            assert!(Change::parse(SettingKey::Zoom, &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_zoom_is_rounded_to_the_nearest_step_and_stored() {
        let (_dir, store) = store();
        store.set(SettingKey::Zoom, &json!(1.3)).expect("in range");
        assert_eq!(store.current().zoom.percent(), 125);
        store.set(SettingKey::Zoom, &json!(2)).expect("an integer is a number too");
        assert_eq!(store.current().zoom, Zoom::MAX);
        assert!(store.set(SettingKey::Zoom, &json!(3.0)).is_err());
        assert_eq!(store.current().zoom, Zoom::MAX, "a refused value changes nothing");
    }

    #[test]
    fn a_night_is_stored_and_a_bad_one_refused_with_its_reason() {
        let (_dir, store) = store();
        store.set(SettingKey::Night, &json!({"from": "23:30", "to": "07:00"})).expect("valid night");
        let stored = store.current().night;
        assert_eq!((stored.from.to_string(), stored.to.to_string()), ("23:30".into(), "07:00".into()));
        store.set(SettingKey::Night, &json!({"from": "06:00", "to": "06:00"})).expect("no night");
        assert!(store.current().night.is_none());
        let refused = store.set(SettingKey::Night, &json!({"from": "22:10", "to": "06:00"}));
        assert!(refused.as_ref().is_err_and(|reason| reason.contains("quarter hour")), "{refused:?}");
        for bad in [json!("22:00-06:00"), json!(22), json!({"from": "22:00"})] {
            assert!(store.set(SettingKey::Night, &bad).is_err(), "{bad}");
        }
        assert!(store.current().night.is_none(), "a refused value changes nothing");
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
    fn hidden_collections_from_before_several_accounts_get_their_account() {
        let (_dir, store) = store();
        let owner = "nextcloud:anna@cloud.example.org";
        let new = format!("{owner}/https://cloud.example.org/dav/calendars/anna/home/");
        store.set_collection_shown("https://cloud.example.org/dav/calendars/anna/work/", false).expect("old id");
        store.set_collection_shown(&new, false).expect("new id");
        store.qualify_legacy_collections(owner).expect("rewritten");
        assert_eq!(
            store.current().hidden_collections,
            [format!("{owner}/https://cloud.example.org/dav/calendars/anna/work/"), new]
        );
        let written = fs::metadata(store.file.as_deref().expect("file")).expect("written").modified().ok();
        store.qualify_legacy_collections(owner).expect("nothing left to do");
        let again = fs::metadata(store.file.as_deref().expect("file")).expect("still there").modified().ok();
        assert_eq!(written, again, "a second start writes nothing");
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
