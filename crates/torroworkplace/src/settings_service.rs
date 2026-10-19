//! The core's side of the settings: reading, saving and applying them.
//!
//! Task S.1 in `docs/tasks/steuerzentrale.md`. `torro_core::settings` holds the type and the
//! file; this module applies changes: start at login (task X.3). It is one entry per system, all
//! starting the running program with `daemon`: an XDG autostart entry on Linux
//! (`~/.config/autostart/torro-workplace.desktop`), a LaunchAgent on macOS
//! (`~/Library/LaunchAgents/io.github.mahype.torro-workplace.plist`) and the value
//! `TorroWorkplace` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` on Windows.
//! Hidden collections are applied by `pim.rs`, which reads [`SettingsStore::current`] on every
//! request. A design pack (task L.3) is checked against the rules of `torro_core::design` before it
//! is stored; the apps follow the setting themselves.
//!
//! The file is not looked up here but handed in ([`SettingsStore`]): the daemon uses the user's
//! `settings.json`, tests a file in a temp directory, so no test reads or writes the real one
//! (task A.0d). Likewise only the daemon's store applies "start at login" to the system: a test
//! store keeps the setting in its file only, so no test points the user's login item at a test
//! program (task X.12).
//!
//! Changes go through one lock: requests run concurrently, and two targeted changes
//! ([`SettingsStore::set`], [`SettingsStore::set_collection_shown`]) must not lose each other in
//! read, change, write.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use torro_core::design;
use torro_core::settings::{
    self, AssistantDefault, ComponentSwitch, Language, ListOrderEntry, MailAppearance, MailFormat, Night, PdfSettings,
    PdfTemplate, PdfTemplateId, Reminders, Retention, Settings, SetupState, ToolSwitch, Voice, Zoom,
};
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
    /// The folder of the design packs (`torro_core::design::dir`), where a chosen design must be
    /// found and valid; `None` without a home directory, then only the standard can be chosen.
    designs: Option<PathBuf>,
    /// Whether a change of `start_at_login` is applied to the system's login item. Only the
    /// daemon's store does: the entry points at the running program, and from a test that is the
    /// test binary, which the next login would start instead of the daemon (task X.12).
    applies_login_item: bool,
}

impl SettingsStore {
    /// The user's `settings.json` (`torro_core::settings::settings_file`), as the daemon uses it.
    /// Changing `start_at_login` here writes or removes the system's login item.
    pub fn system() -> SettingsStore {
        SettingsStore { file: settings::settings_file(), designs: design::dir(), applies_login_item: true }
    }

    /// The settings in `file`, for tests. `start_at_login` is only stored in `file`; the
    /// system's login item is never touched, so tests may switch it freely. The design packs are
    /// the folder `designs` next to `file`.
    #[cfg(test)]
    pub fn at(file: PathBuf) -> SettingsStore {
        let designs = file.parent().map(|dir| dir.join("designs"));
        SettingsStore { file: Some(file), designs, applies_login_item: false }
    }

    /// No file at all, for tests that do not look at the settings: reads give the defaults,
    /// changes fail. Like [`SettingsStore::at`], it never touches the system's login item.
    #[cfg(test)]
    pub fn without_file() -> SettingsStore {
        SettingsStore { file: None, designs: None, applies_login_item: false }
    }

    /// The folder of the design packs, for `settings.list_designs`; `None` without a home
    /// directory (only the standard exists then).
    pub fn designs(&self) -> Option<&Path> {
        self.designs.as_deref()
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

    /// Like [`SettingsStore::update`], but the hidden collections, the orders of the task lists and
    /// the reminder styles of calendars (task ER1.2) stay as stored, read under the same lock as the write, so a collection hidden
    /// meanwhile is not shown again. For clients, which never see the hidden collections nor the
    /// orders of lists outside their selection, and so cannot send them back; they change those
    /// with `set_collection_shown` and `set_setting`.
    ///
    /// Nor does a client switch the microphone on (point 6 of "Was nie nach außen geht",
    /// task C.9): voice stays off when it is off as stored now, whatever `new` says. The core
    /// refuses such a request before (`forbidden`); this keeps it so when a person switched voice
    /// off between that check and the write. The same holds for letting mail go to cloud models
    /// (`mail_to_cloud`, task C.17): a client may keep it local, never let it out.
    ///
    /// Nor which model does which function (`ai_assignments`, task E12.5): those are checked
    /// against the accounts, the models and the data rule when they are set
    /// (`Core::set_ai_assignment`), and a whole-settings write would go round that check. They
    /// change only through [`SettingsStore::set_ai_assignment`].
    ///
    /// Nor does a client switch on one of the assistant's own tools (point 9, issue #225): a tool
    /// stays on only when it is on as stored and in `new`, the PDF folder and the logos of the
    /// templates stay as stored (`torro_core::settings::Tools::from_client`); the other fields of
    /// the templates come from `new`.
    pub fn update_keeping_hidden(&self, new: Settings) -> Result<(), String> {
        self.change(|settings| {
            let hidden = std::mem::take(&mut settings.hidden_collections);
            let assignments = std::mem::take(&mut settings.ai_assignments);
            let list_orders = std::mem::take(&mut settings.list_orders);
            // Nor the reminder styles of calendars outside its selection (task ER1.2): it changes
            // them with `set_setting reminders`, which names each calendar.
            let calendars = std::mem::take(&mut settings.reminders.calendars);
            let enabled = settings.voice.enabled && new.voice.enabled;
            let voice = Voice { enabled, ..new.voice.clone() };
            let mail_to_cloud = settings.mail_to_cloud && new.mail_to_cloud;
            let reminders = Reminders { calendars, ..new.reminders.clone() };
            let tools = settings.tools.from_client(&new.tools);
            *settings = Settings {
                hidden_collections: hidden,
                ai_assignments: assignments,
                list_orders,
                voice,
                mail_to_cloud,
                reminders,
                tools,
                ..new
            };
        })
    }

    /// Sets which account and model `function` uses, or takes the assignment back with `None`
    /// (task E12.5). The one way `ai_assignments` changes: `Core::set_ai_assignment` checks the
    /// account, the model and the data rule before it calls this.
    pub fn set_ai_assignment(
        &self,
        function: torro_core::ai::AiFunction,
        assignment: Option<torro_core::ai::AiAssignment>,
    ) -> Result<(), String> {
        self.change(|settings| match &assignment {
            Some(held) => {
                settings.ai_assignments.insert(function, held.clone());
            }
            None => {
                settings.ai_assignments.remove(&function);
            }
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

    /// Takes `collections` out of the hidden ones, the list orders and the reminder styles, for an
    /// account that is gone: its ids mean nothing any more, and an account added later must not
    /// start with them.
    /// Writes nothing when none of them was named.
    pub fn forget_collections(&self, collections: &[String]) -> Result<(), String> {
        self.change(|settings| {
            settings.hidden_collections.retain(|id| !collections.contains(id));
            settings.list_orders.retain(|id, _| !collections.contains(id));
            settings.reminders.calendars.retain(|id, _| !collections.contains(id));
        })
    }

    /// Hides and orders every collection of the account `from` under the id it has under the
    /// account `to` as well (`<from>/<URL>` → `<to>/<URL>`), keeping the old ids: the first step
    /// when a mail account takes over the CalDAV account of the same sign-in (task M2.7), taken
    /// before the takeover is stored. Hidden collections keep agents out too, so a calendar the
    /// person hid must never show under its new id, not even for a moment; the old ids go once
    /// the takeover is stored ([`SettingsStore::forget_account`]). One atomic write, only when
    /// something changed.
    pub fn copy_collections(&self, from: &str, to: &str) -> Result<(), String> {
        let renamed =
            |id: &str| torro_pim::account::local_id(id, from).map(|url| torro_pim::account::collection_id(to, url));
        self.change(|settings| {
            let hidden: Vec<String> = settings.hidden_collections.iter().filter_map(|id| renamed(id)).collect();
            for id in hidden {
                if !settings.hidden_collections.contains(&id) {
                    settings.hidden_collections.push(id);
                }
            }
            let orders: Vec<_> =
                settings.list_orders.iter().filter_map(|(id, order)| Some((renamed(id)?, *order))).collect();
            for (id, order) in orders {
                settings.list_orders.entry(id).or_insert(order);
            }
            // A calendar shown over the whole screen must stay so under its new id (task ER1.2).
            let styles: Vec<_> =
                settings.reminders.calendars.iter().filter_map(|(id, shown)| Some((renamed(id)?, *shown))).collect();
            for (id, shown) in styles {
                settings.reminders.calendars.entry(id).or_insert(shown);
            }
        })
    }

    /// Takes every collection of the account `from` out of the hidden ones and the list orders
    /// ([`SettingsStore::forget_collections`] for all ids `<from>/…`): for an account that is gone,
    /// such as a CalDAV account a mail account took over (task M2.7) after
    /// [`SettingsStore::copy_collections`].
    pub fn forget_account(&self, from: &str) -> Result<(), String> {
        let settings = self.current();
        let of_account = |id: &&String| torro_pim::account::local_id(id, from).is_some();
        let ids: Vec<String> = settings
            .hidden_collections
            .iter()
            .chain(settings.list_orders.keys())
            .chain(settings.reminders.calendars.keys())
            .filter(of_account)
            .cloned()
            .collect();
        self.forget_collections(&ids)
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
        // Checked on every way in (set_setting, update_settings), but only when it changes: a pack
        // that broke after it was chosen must not block changing anything else.
        if new.design != old.design {
            self.check_design(&new.design)?;
        }
        let file = self.file.as_deref().ok_or("no home directory, so no settings file")?;
        save_to(file, &new)?;
        if let Some(enabled) = self.login_item_change(&old, &new) {
            apply_start_at_login(enabled)?;
        }
        Ok(())
    }

    /// Whether the design `id` can be chosen: the standard, or a pack in [`SettingsStore::designs`]
    /// that passes every rule. Otherwise every reason, for the person or agent who chose it.
    fn check_design(&self, id: &str) -> Result<(), String> {
        let refused = |reasons: String| format!("the design {id:?} can't be used: {reasons}");
        let Some(dir) = self.designs.as_deref() else {
            return match id == design::STANDARD_ID {
                true => Ok(()),
                false => Err(refused("there is no designs folder without a home directory".into())),
            };
        };
        design::load(dir, id)
            .map(|_| ())
            .map_err(|problems| refused(problems.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")))
    }

    /// The state the system's login item must be switched to after `old` became `new`, or
    /// `None` when it stays as it is: the setting did not change, or this store must not touch
    /// the system (a test store, see [`SettingsStore::at`]). Kept apart from [`Self::change`] so
    /// tests can check the decision without the system call.
    fn login_item_change(&self, old: &Settings, new: &Settings) -> Option<bool> {
        (self.applies_login_item && old.start_at_login != new.start_at_login).then_some(new.start_at_login)
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
    /// Whether the pack exists and is valid is [`SettingsStore::check_design`]'s check.
    Design(String),
    /// How long the assistant keeps conversations (task C.1).
    AssistantRetention(Retention),
    /// Voice on or off (task C.9). Who may switch it on is the core's check
    /// (`Request::kept_for_people`).
    VoiceEnabled(bool),
    WakeWord(String),
    Microphone(Option<String>),
    SpeakAnswers(bool),
    SttModel(Option<String>),
    Voice(Option<String>),
    /// What a new chat of the assistant starts with (issue #131); `None` for the one used last.
    AssistantDefault(Option<AssistantDefault>),
    /// Where the first-run setup stands (task W.1); the version is recorded on the way in.
    Setup(SetupState),
    /// One component on or off (task W.1).
    Component(ComponentSwitch),
    /// How a mail with HTML first shows in the reader (task M9.5).
    MailView(MailFormat),
    /// The colours of a mail's content (task M9.5).
    MailAppearance(MailAppearance),
    /// The data rule for mail (task C.17). Who may switch it on is the core's check
    /// (`Request::kept_for_people`).
    MailToCloud(bool),
    /// Which mail categories the sidebar leaves out (task E12.10).
    HiddenCategories(Vec<torro_mail::Category>),
    /// A change of the reminders of events (task ER1.2): the fields it names, checked against the
    /// defaults when parsed and merged onto the stored ones when applied ([`Reminders::merged`]).
    Reminders(serde_json::Value),
    /// One of the assistant's own tools on or off (issue #225). Who may switch it on is the core's
    /// check (`Request::kept_for_people`).
    ToolEnabled(ToolSwitch),
    /// The folder of new PDFs; `None` for the desktop. Who may set one is the core's check
    /// (`Core::switches_on_for_people`).
    PdfFolder(Option<String>),
    /// One PDF template, replaced whole. Who may set its logo is the core's check, as for the folder.
    PdfTemplate(PdfTemplateId, PdfTemplate),
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
            SettingKey::Design => Change::Design(serde_json::from_value(value.clone()).map_err(invalid)?),
            // Strict, like the night: the file's reading falls back to 30 days instead.
            SettingKey::AssistantRetention => Change::AssistantRetention(Retention::from_value(value)?),
            SettingKey::VoiceEnabled => Change::VoiceEnabled(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::WakeWord => Change::WakeWord(Voice::wake_word_from(value)?),
            SettingKey::Microphone => Change::Microphone(Voice::microphone_from(value)?),
            SettingKey::SpeakAnswers => Change::SpeakAnswers(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::SttModel => Change::SttModel(Voice::file_from(value, "bin")?),
            SettingKey::Voice => Change::Voice(Voice::file_from(value, "onnx")?),
            SettingKey::AssistantDefault => Change::AssistantDefault(AssistantDefault::from_value(value)?),
            SettingKey::Setup => Change::Setup(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::Component => Change::Component(serde_json::from_value(value.clone()).map_err(invalid)?),
            // Strict: only the file's reading falls back to the default.
            SettingKey::MailView => Change::MailView(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::MailAppearance => {
                Change::MailAppearance(serde_json::from_value(value.clone()).map_err(invalid)?)
            }
            SettingKey::MailToCloud => Change::MailToCloud(serde_json::from_value(value.clone()).map_err(invalid)?),
            // Strict, like the night: the file's reading falls back to the defaults instead.
            SettingKey::HiddenCategories => {
                Change::HiddenCategories(serde_json::from_value(value.clone()).map_err(invalid)?)
            }
            SettingKey::Reminders => {
                Reminders::default().merged(value)?;
                Change::Reminders(value.clone())
            }
            SettingKey::ToolEnabled => Change::ToolEnabled(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::PdfFolder => Change::PdfFolder(PdfSettings::folder_from(value)?),
            SettingKey::PdfTemplate => {
                let (id, template) = PdfTemplate::from_value(value)?;
                Change::PdfTemplate(id, template)
            }
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
            Change::Design(id) => settings.design = id,
            Change::AssistantRetention(retention) => settings.assistant_retention = retention,
            Change::VoiceEnabled(enabled) => settings.voice.enabled = enabled,
            Change::WakeWord(phrase) => settings.voice.wake_word = phrase,
            Change::Microphone(name) => settings.voice.microphone = name,
            Change::SpeakAnswers(speak) => settings.voice.speak_answers = speak,
            Change::SttModel(path) => settings.voice.stt_model = path,
            Change::Voice(path) => settings.voice.voice = path,
            Change::AssistantDefault(default) => settings.assistant_default = default,
            Change::Setup(state) => settings.setup = settings.setup.moved_to(state),
            Change::Component(switch) => settings.set_component(switch.component, switch.enabled),
            Change::MailView(format) => settings.mail_view = format,
            Change::MailAppearance(appearance) => settings.mail_appearance = appearance,
            Change::MailToCloud(allowed) => settings.mail_to_cloud = allowed,
            Change::HiddenCategories(hidden) => settings.hidden_categories = hidden,
            // Checked when parsed: what the stored ones hold cannot make the fields it names unfit.
            Change::Reminders(change) => {
                if let Ok(merged) = settings.reminders.merged(&change) {
                    settings.reminders = merged;
                }
            }
            Change::ToolEnabled(switch) => settings.tools.set_on(switch.tool, switch.enabled),
            Change::PdfFolder(folder) => settings.tools.pdf.folder = folder,
            Change::PdfTemplate(id, template) => *settings.tools.pdf.template_mut(id) = template,
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

/// Turns the start-at-login entry of this system on or off (see the module docs for where it is).
/// The entry points at the running program, so switching the setting off and on again after the
/// program moved updates it.
#[cfg(target_os = "linux")]
fn apply_start_at_login(enabled: bool) -> Result<(), String> {
    autostart::set_enabled(enabled)
}

#[cfg(target_os = "macos")]
fn apply_start_at_login(enabled: bool) -> Result<(), String> {
    launch_agent::set_enabled(enabled)
}

#[cfg(windows)]
fn apply_start_at_login(enabled: bool) -> Result<(), String> {
    run_key::set_enabled(enabled)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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

/// The LaunchAgent (macOS): `launchd` starts every agent in `~/Library/LaunchAgents` at login.
///
/// Switching it off only deletes the file, without `launchctl bootout`: the running daemon is the
/// one the user is talking to right now and should not be killed by a settings change, and
/// `launchd` reads the file only at the next login, so a registration left over until then cannot
/// start anything. The same goes for switching on: no `launchctl bootstrap`, the daemon is
/// already running.
#[cfg(target_os = "macos")]
mod launch_agent {
    use std::env;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    const LABEL: &str = "io.github.mahype.torro-workplace";

    /// Creates the agent when `enabled`, removes it otherwise. Uses the real home directory and
    /// the running program, which inside an `.app` bundle is `Contents/MacOS/torroworkplace`.
    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let home = env::var_os("HOME").map(PathBuf::from);
        let path = plist_path(home.as_deref()).ok_or("no home directory, so no login item")?;
        if !enabled {
            return remove_plist(&path);
        }
        let exe = env::current_exe().map_err(|err| format!("cannot find the executable: {err}"))?;
        let content = plist(&exe)?;
        write_plist(&path, &content).map_err(|err| format!("cannot write {}: {err}", path.display()))
    }

    /// `~/Library/LaunchAgents/io.github.mahype.torro-workplace.plist`. An empty or relative home
    /// counts as none: a relative path would land wherever the process happens to run.
    fn plist_path(home: Option<&Path>) -> Option<PathBuf> {
        let home = home.filter(|home| home.is_absolute())?;
        Some(home.join("Library").join("LaunchAgents").join(format!("{LABEL}.plist")))
    }

    /// The property list: runs `<exe> daemon` at login. `ProcessType` Interactive keeps the
    /// system from throttling the daemon like a background job. Fails for a path that is not
    /// UTF-8, which a property list cannot hold.
    fn plist(exe: &Path) -> Result<String, String> {
        let exe = exe.to_str().ok_or_else(|| format!("{} is not valid UTF-8", exe.display()))?;
        let exe = xml_escape(exe);
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \t<key>Label</key>\n\
             \t<string>{LABEL}</string>\n\
             \t<key>ProgramArguments</key>\n\
             \t<array>\n\
             \t\t<string>{exe}</string>\n\
             \t\t<string>daemon</string>\n\
             \t</array>\n\
             \t<key>RunAtLoad</key>\n\
             \t<true/>\n\
             \t<key>ProcessType</key>\n\
             \t<string>Interactive</string>\n\
             </dict>\n\
             </plist>\n"
        ))
    }

    /// `text` as XML character data: the characters XML reserves are replaced.
    fn xml_escape(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                _ => out.push(c),
            }
        }
        out
    }

    fn write_plist(path: &Path, content: &str) -> io::Result<()> {
        let dir = path.parent().ok_or_else(|| io::Error::other("login item has no directory"))?;
        fs::create_dir_all(dir)?;
        fs::write(path, content)
    }

    /// Removing a file that is not there is fine: the goal is that it is gone.
    fn remove_plist(path: &Path) -> Result<(), String> {
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
        fn path_is_in_the_users_launch_agents() {
            let path = plist_path(Some(Path::new("/Users/anna")));
            assert_eq!(
                path,
                Some(PathBuf::from("/Users/anna/Library/LaunchAgents/io.github.mahype.torro-workplace.plist"))
            );
        }

        #[test]
        fn no_home_or_a_relative_one_gives_no_path() {
            assert_eq!(plist_path(None), None);
            assert_eq!(plist_path(Some(Path::new(""))), None);
            assert_eq!(plist_path(Some(Path::new("anna"))), None);
        }

        #[test]
        fn plist_has_the_expected_content() {
            let content = plist(Path::new("/Applications/Torro.app/Contents/MacOS/torroworkplace")).expect("plist");
            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                <plist version=\"1.0\">\n\
                <dict>\n\
                \t<key>Label</key>\n\
                \t<string>io.github.mahype.torro-workplace</string>\n\
                \t<key>ProgramArguments</key>\n\
                \t<array>\n\
                \t\t<string>/Applications/Torro.app/Contents/MacOS/torroworkplace</string>\n\
                \t\t<string>daemon</string>\n\
                \t</array>\n\
                \t<key>RunAtLoad</key>\n\
                \t<true/>\n\
                \t<key>ProcessType</key>\n\
                \t<string>Interactive</string>\n\
                </dict>\n\
                </plist>\n";
            assert_eq!(content, expected);
        }

        #[test]
        fn special_characters_in_the_path_are_escaped() {
            let content = plist(Path::new("/Users/a&b/<x>/\"y\"/it's/torroworkplace")).expect("plist");
            assert!(
                content.contains("<string>/Users/a&amp;b/&lt;x&gt;/&quot;y&quot;/it&apos;s/torroworkplace</string>"),
                "{content}"
            );
        }

        #[test]
        fn a_path_with_spaces_stays_one_argument() {
            let content = plist(Path::new("/Users/Anna Meyer/Torro Apps/torroworkplace")).expect("plist");
            assert!(content.contains("<string>/Users/Anna Meyer/Torro Apps/torroworkplace</string>"));
        }

        #[test]
        fn a_path_that_is_not_utf8_is_refused() {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            let path = Path::new(OsStr::from_bytes(b"/tmp/\xff/torroworkplace"));
            assert!(plist(path).is_err());
        }

        #[test]
        fn plist_is_created_and_removed() {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = plist_path(Some(dir.path())).expect("path");
            let content = plist(Path::new("/usr/local/bin/torroworkplace")).expect("plist");

            write_plist(&path, &content).expect("create plist, with the missing directories");
            assert_eq!(fs::read_to_string(&path).expect("read plist"), content);

            remove_plist(&path).expect("remove plist");
            assert!(!path.exists());
            remove_plist(&path).expect("removing a missing plist is fine");
        }
    }
}

/// The `Run` key (Windows): Explorer starts every value under
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` as a command line at login. Per user,
/// so it needs no administrator rights.
#[cfg(windows)]
mod run_key {
    use std::env;
    use std::io;
    use std::path::Path;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "TorroWorkplace";

    /// Sets the value when `enabled`, deletes it otherwise. Uses the real `Run` key and the
    /// running program.
    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let exe = env::current_exe().map_err(|err| format!("cannot find the executable: {err}"))?;
        set_in(RUN_KEY, enabled, &exe).map_err(|err| format!(r"cannot change HKCU\{RUN_KEY}: {err}"))
    }

    /// `set_enabled` on the key `subkey` of `HKEY_CURRENT_USER`, which tests point at a key of
    /// their own so they never touch `Run`.
    fn set_in(subkey: &str, enabled: bool, exe: &Path) -> io::Result<()> {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        if enabled {
            // Opens the key, or creates it (the test keys do not exist yet).
            let (key, _) = user.create_subkey(subkey)?;
            return key.set_value(VALUE_NAME, &command_line(exe));
        }
        // Removing what is not there is fine: the goal is that it is gone.
        let key = match user.open_subkey_with_flags(subkey, KEY_SET_VALUE) {
            Ok(key) => key,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        match key.delete_value(VALUE_NAME) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }

    /// `"<exe>" daemon`. The path is quoted so one with spaces (`C:\Program Files\…`) stays the
    /// program; Windows paths cannot contain a quote, so there is nothing to escape.
    fn command_line(exe: &Path) -> String {
        format!("\"{}\" daemon", exe.display())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A key of its own for one test, removed again when the test ends, even by a failure.
        struct TestKey(String);

        impl TestKey {
            fn new(name: &str) -> TestKey {
                TestKey(format!(r"Software\TorroWorkplaceTest\{name}-{}", std::process::id()))
            }

            fn value(&self) -> io::Result<String> {
                let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&self.0)?;
                key.get_value(VALUE_NAME)
            }
        }

        impl Drop for TestKey {
            fn drop(&mut self) {
                let user = RegKey::predef(HKEY_CURRENT_USER);
                let _ = user.delete_subkey_all(&self.0);
                // Only succeeds when no other test key is left in it; the parent stays otherwise.
                let _ = user.delete_subkey(r"Software\TorroWorkplaceTest");
            }
        }

        #[test]
        fn command_line_quotes_the_path() {
            assert_eq!(
                command_line(Path::new(r"C:\Program Files\Torro Workplace\torroworkplace.exe")),
                r#""C:\Program Files\Torro Workplace\torroworkplace.exe" daemon"#
            );
        }

        #[test]
        fn value_is_set_replaced_and_deleted() {
            let key = TestKey::new("set");
            set_in(&key.0, true, Path::new(r"C:\one\torroworkplace.exe")).expect("set");
            assert_eq!(key.value().expect("value"), r#""C:\one\torroworkplace.exe" daemon"#);

            set_in(&key.0, true, Path::new(r"C:\two words\torroworkplace.exe")).expect("replace");
            assert_eq!(key.value().expect("value"), r#""C:\two words\torroworkplace.exe" daemon"#);

            set_in(&key.0, false, Path::new(r"C:\ignored.exe")).expect("delete");
            assert!(key.value().is_err());
        }

        #[test]
        fn deleting_what_is_not_there_is_fine() {
            let missing_key = TestKey::new("missing");
            set_in(&missing_key.0, false, Path::new(r"C:\x.exe")).expect("no key");

            let missing_value = TestKey::new("novalue");
            RegKey::predef(HKEY_CURRENT_USER).create_subkey(&missing_value.0).expect("create key");
            set_in(&missing_value.0, false, Path::new(r"C:\x.exe")).expect("no value");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;
    use torro_core::settings::{Component, Setup};

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

    /// Writes a design pack `id` with `toml` as its `design.toml` into the store's designs folder.
    fn pack(store: &SettingsStore, id: &str, toml: &str) {
        let folder = store.designs().expect("a test store has a designs folder").join(id);
        fs::create_dir_all(&folder).expect("pack folder");
        fs::write(folder.join("design.toml"), toml).expect("design.toml");
    }

    #[test]
    fn a_valid_design_is_stored_and_read_back() {
        let (_dir, store) = store();
        pack(
            &store,
            "teal",
            "format = 1\n[about]\nname = \"Teal\"\nversion = \"1.0.0\"\n[colors.dark]\naccent = \"#0E7C7B\"\n",
        );
        assert_eq!(store.current().design, "torro", "the standard until one is chosen");
        store.set(SettingKey::Design, &json!("teal")).expect("a valid pack");
        assert_eq!(store.current().design, "teal");
        store.set(SettingKey::Design, &json!("torro")).expect("the standard is always there");
        assert_eq!(store.current().design, "torro");
    }

    #[test]
    fn a_missing_or_broken_design_is_refused_with_its_reasons() {
        let (_dir, store) = store();
        pack(
            &store,
            "pale",
            "format = 1\n[about]\nname = \"Pale\"\nversion = \"1.0.0\"\n[colors.dark]\ntext = \"#202020\"\n",
        );
        let broken = store.set(SettingKey::Design, &json!("pale")).expect_err("too little contrast");
        assert!(broken.contains("\"pale\" can't be used"), "{broken}");
        assert!(broken.contains("colors.dark.text"), "names the broken key: {broken}");
        let missing = store.set(SettingKey::Design, &json!("plex")).expect_err("no such pack");
        assert!(missing.contains("\"plex\""), "{missing}");
        assert!(store.set(SettingKey::Design, &json!(7)).is_err(), "an id is a string");
        assert!(!file_exists(&store), "nothing was written");
        // update_settings takes the same check.
        let all = Settings { design: "plex".into(), ..Settings::default() };
        assert!(store.update(all).is_err());
        assert_eq!(store.current().design, "torro");
    }

    #[test]
    fn a_design_that_broke_later_does_not_block_other_changes() {
        let (_dir, store) = store();
        pack(&store, "teal", "format = 1\n[about]\nname = \"Teal\"\nversion = \"1.0.0\"\n");
        store.set(SettingKey::Design, &json!("teal")).expect("valid");
        fs::remove_dir_all(store.designs().expect("folder").join("teal")).expect("remove the pack");
        store.set(SettingKey::Appearance, &json!("dark")).expect("the design is not checked again");
        assert_eq!(store.current().design, "teal", "kept as chosen; the apps fall back to the standard");
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
        Change::parse(SettingKey::Design, &json!("plex")).expect("an id").apply(&mut settings);
        Change::parse(SettingKey::AssistantRetention, &json!("forever")).expect("forever").apply(&mut settings);
        let default = json!({"engine": "claude-code", "model": "opus", "effort": "medium"});
        Change::parse(SettingKey::AssistantDefault, &default).expect("a default").apply(&mut settings);
        Change::parse(SettingKey::Setup, &json!("skipped")).expect("a state").apply(&mut settings);
        let mail_off = json!({"component": "mail", "enabled": false});
        Change::parse(SettingKey::Component, &mail_off).expect("a switch").apply(&mut settings);
        let reminders = json!({"allDay": true, "calendars": {"a/work": "full_screen"}});
        Change::parse(SettingKey::Reminders, &reminders).expect("valid reminders").apply(&mut settings);
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
                design: "plex".into(),
                assistant_retention: Retention::Forever,
                voice: Voice::default(),
                assistant_default: AssistantDefault::from_value(&default).expect("a default"),
                setup: Setup { state: SetupState::Skipped, version: Setup::VERSION },
                components_off: BTreeSet::from([Component::Mail]),
                mail_view: MailFormat::Html,
                mail_appearance: MailAppearance::App,
                mail_to_cloud: false,
                reminders: Reminders::from_value(&reminders).expect("valid reminders"),
                ai_assignments: std::collections::BTreeMap::new(),
                hidden_categories: Vec::new(),
                tools: Default::default(),
            }
        );
        Change::parse(SettingKey::MailToCloud, &json!(true)).expect("valid flag").apply(&mut settings);
        assert!(settings.mail_to_cloud);
        assert!(Change::parse(SettingKey::MailToCloud, &json!("yes")).is_err(), "only true or false");
        assert!(settings.reminders.all_day && settings.reminders.enabled, "the rest keeps its defaults");
        Change::parse(SettingKey::Reminders, &json!({"defaultLead": 5})).expect("valid").apply(&mut settings);
        assert!(settings.reminders.all_day, "a later change keeps what it does not name");
        assert_eq!(settings.reminders.default_lead, Some(5));
        assert!(Change::parse(SettingKey::Reminders, &json!({"defaultLead": -1})).is_err(), "a lead of minutes");
        Change::parse(SettingKey::AssistantDefault, &json!(null)).expect("none").apply(&mut settings);
        assert_eq!(settings.assistant_default, None, "null goes back to the one used last");
        assert!(Change::parse(SettingKey::AssistantDefault, &json!({"engine": " "})).is_err(), "an engine");
        assert!(Change::parse(SettingKey::QuietWhenBusy, &json!("off")).is_err(), "only true or false");
        assert!(Change::parse(SettingKey::AssistantRetention, &json!(0)).is_err(), "at least a day");
        assert!(Change::parse(SettingKey::Setup, &json!("later")).is_err(), "pending, skipped or done");
        let unknown = json!({"component": "whiteboard", "enabled": false});
        assert!(Change::parse(SettingKey::Component, &unknown).is_err(), "a component this version knows");
        let extra = json!({"component": "mail", "enabled": true, "why": "x"});
        assert!(Change::parse(SettingKey::Component, &extra).is_err(), "nothing else in the switch");
    }

    #[test]
    fn components_go_off_and_on_again_through_the_store() {
        let (_dir, store) = store();
        store.set(SettingKey::Component, &json!({"component": "inbox", "enabled": false})).expect("off");
        assert!(!store.current().component_on(Component::Inbox));
        store.set(SettingKey::Component, &json!({"component": "inbox", "enabled": true})).expect("on");
        assert!(store.current().component_on(Component::Inbox));
        store.set(SettingKey::Setup, &json!("done")).expect("done");
        assert_eq!(store.current().setup.state, SetupState::Done);
    }

    /// Task M9.5: how mail shows is set and read back by its key; a value that does not fit is
    /// refused and changes nothing.
    #[test]
    fn mail_view_and_appearance_are_set_and_checked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore::at(dir.path().join("settings.json"));
        let current = store.current();
        assert_eq!((current.mail_view, current.mail_appearance), (MailFormat::Html, MailAppearance::App));
        store.set(SettingKey::MailView, &json!("text")).expect("text");
        store.set(SettingKey::MailAppearance, &json!("dark")).expect("dark");
        let current = store.current();
        assert_eq!((current.mail_view, current.mail_appearance), (MailFormat::Text, MailAppearance::Dark));
        for (key, value) in [
            (SettingKey::MailView, json!("rich")),
            (SettingKey::MailView, json!("HTML")),
            (SettingKey::MailView, json!(true)),
            (SettingKey::MailAppearance, json!("system")),
            (SettingKey::MailAppearance, json!(null)),
        ] {
            assert!(store.set(key, &value).is_err(), "{key:?} {value}");
        }
        let current = store.current();
        assert_eq!((current.mail_view, current.mail_appearance), (MailFormat::Text, MailAppearance::Dark));
        store.set(SettingKey::MailView, &json!("html")).expect("html");
        store.set(SettingKey::MailAppearance, &json!("light")).expect("light");
        let current = store.current();
        assert_eq!((current.mail_view, current.mail_appearance), (MailFormat::Html, MailAppearance::Light));
    }

    /// Task C.9: the voice settings are set one by one and checked; a client's whole settings
    /// never switch the microphone on, but may switch it off.
    #[test]
    fn voice_settings_are_set_and_a_client_never_switches_the_microphone_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore::at(dir.path().join("settings.json"));
        let model = if cfg!(windows) { r"C:\voice\ggml-base.bin" } else { "/voice/ggml-base.bin" };
        for (key, value) in [
            (SettingKey::WakeWord, json!("Hey Torro")),
            (SettingKey::Microphone, json!("alsa_input.usb")),
            (SettingKey::SpeakAnswers, json!(false)),
            (SettingKey::SttModel, json!(model)),
        ] {
            store.set(key, &value).expect("a valid value");
        }
        let voice = store.current().voice;
        assert_eq!(
            voice,
            Voice {
                microphone: Some("alsa_input.usb".into()),
                speak_answers: false,
                stt_model: Some(model.into()),
                ..Voice::default()
            }
        );
        for (key, value) in [
            (SettingKey::WakeWord, json!("ok computer")),
            (SettingKey::Microphone, json!("--help")),
            (SettingKey::SttModel, json!("ggml-base.bin")),
            (SettingKey::Voice, json!("/voices/thorsten.bin")),
            (SettingKey::VoiceEnabled, json!("yes")),
        ] {
            assert!(store.set(key, &value).is_err(), "{key:?} {value}");
        }
        store.set(SettingKey::Microphone, &json!(null)).expect("back to the default");
        assert_eq!(store.current().voice.microphone, None);

        // A client sends the settings with voice on: it stays off.
        let mut sent = store.current();
        sent.voice.enabled = true;
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(!store.current().voice.enabled);
        // Switched on by a person, a client may switch it off.
        store.set(SettingKey::VoiceEnabled, &json!(true)).expect("the person");
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(store.current().voice.enabled, "on stays on");
        sent.voice.enabled = false;
        store.update_keeping_hidden(sent).expect("written");
        assert!(!store.current().voice.enabled);
    }

    /// Task C.17: a client's whole settings never let mail go to the cloud, but may keep it local.
    #[test]
    fn a_client_never_lets_mail_out_but_may_keep_it_local() {
        let (_dir, store) = store();
        let mut sent = store.current();
        sent.mail_to_cloud = true;
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(!store.current().mail_to_cloud);
        store.set(SettingKey::MailToCloud, &json!(true)).expect("the person");
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(store.current().mail_to_cloud, "on stays on");
        sent.mail_to_cloud = false;
        store.update_keeping_hidden(sent).expect("written");
        assert!(!store.current().mail_to_cloud);
    }

    /// An absolute path on this system, for folders and logos.
    fn absolute(path: &str) -> String {
        match cfg!(windows) {
            true => format!(r"C:{}", path.replace('/', r"\")),
            false => path.to_string(),
        }
    }

    /// Issue #225: a tool is switched by its key, the PDF folder and a template are set and checked;
    /// a value that does not fit is refused and changes nothing.
    #[test]
    fn tools_are_switched_and_the_pdf_settings_set_and_checked() {
        use torro_core::settings::{Paper, ToolId};
        let (_dir, store) = store();
        store.set(SettingKey::ToolEnabled, &json!({"tool": "pdf", "enabled": true})).expect("on");
        assert!(store.current().tools.is_on(ToolId::Pdf));
        store.set(SettingKey::ToolEnabled, &json!({"tool": "pdf", "enabled": false})).expect("off");
        assert!(!store.current().tools.is_on(ToolId::Pdf));
        let folder = absolute("/Users/anna/PDFs");
        store.set(SettingKey::PdfFolder, &json!(folder)).expect("a folder");
        assert_eq!(store.current().tools.pdf.folder, Some(folder.clone()));
        let mut letter = PdfTemplate::standard(PdfTemplateId::Letter);
        (letter.paper, letter.footer) = (Paper::Letter, "ACME".into());
        store.set(SettingKey::PdfTemplate, &letter.to_value(PdfTemplateId::Letter)).expect("a template");
        let stored = store.current().tools;
        assert_eq!(stored.pdf.letter, letter);
        assert_eq!(stored.pdf.report, PdfTemplate::standard(PdfTemplateId::Report), "the others stay");

        let mut too_small = letter.to_value(PdfTemplateId::Letter);
        too_small["fontSizePt"] = json!(7);
        let mut relative_logo = letter.to_value(PdfTemplateId::Letter);
        relative_logo["logo"] = json!("logo.png");
        let mut unknown = letter.to_value(PdfTemplateId::Letter);
        unknown["template"] = json!("invoice");
        for (key, value) in [
            (SettingKey::ToolEnabled, json!({"tool": "whiteboard", "enabled": true})),
            (SettingKey::ToolEnabled, json!({"tool": "pdf", "enabled": true, "why": "x"})),
            (SettingKey::ToolEnabled, json!(true)),
            (SettingKey::PdfFolder, json!("PDFs")),
            (SettingKey::PdfTemplate, too_small),
            (SettingKey::PdfTemplate, relative_logo),
            (SettingKey::PdfTemplate, unknown),
        ] {
            assert!(store.set(key, &value).is_err(), "{key:?} {value}");
        }
        assert_eq!(store.current().tools, stored, "a refused value changes nothing");
        store.set(SettingKey::PdfFolder, &json!(null)).expect("back to the desktop");
        assert_eq!(store.current().tools.pdf.folder, None);
    }

    /// Issue #225, point 9: a client's whole settings never switch a tool on, set a folder or a
    /// logo; they may switch a tool off and change the other fields of a template, and the paths a
    /// person set stay.
    #[test]
    fn a_client_never_switches_a_tool_on_nor_sets_its_paths() {
        use torro_core::settings::ToolId;
        let (_dir, store) = store();
        let folder = absolute("/sync/Out");
        let logo = absolute("/pictures/logo.png");
        let mut sent = store.current();
        sent.tools.set_on(ToolId::Pdf, true);
        sent.tools.pdf.folder = Some(folder.clone());
        sent.tools.pdf.note.logo = Some(logo.clone());
        sent.tools.pdf.note.font_size_pt = 12;
        store.update_keeping_hidden(sent.clone()).expect("written");
        let kept = store.current().tools;
        assert!(!kept.is_on(ToolId::Pdf), "off stays off");
        assert_eq!((kept.pdf.folder, kept.pdf.note.logo), (None, None), "no path of a client");
        assert_eq!(kept.pdf.note.font_size_pt, 12, "the other fields are taken");

        // The person switches it on and sets the paths: on stays on, the paths stay as they are.
        store.set(SettingKey::ToolEnabled, &json!({"tool": "pdf", "enabled": true})).expect("the person");
        store.set(SettingKey::PdfFolder, &json!(absolute("/Users/anna/PDFs"))).expect("the person");
        let mut note = PdfTemplate::standard(PdfTemplateId::Note);
        note.logo = Some(absolute("/pictures/anna.png"));
        store.set(SettingKey::PdfTemplate, &note.to_value(PdfTemplateId::Note)).expect("the person");
        let person = store.current().tools;
        store.update_keeping_hidden(sent.clone()).expect("written");
        let kept = store.current().tools;
        assert!(kept.is_on(ToolId::Pdf), "on stays on");
        assert_eq!((&kept.pdf.folder, &kept.pdf.note.logo), (&person.pdf.folder, &person.pdf.note.logo));
        sent.tools.set_on(ToolId::Pdf, false);
        store.update_keeping_hidden(sent).expect("written");
        assert!(!store.current().tools.is_on(ToolId::Pdf), "a client may switch it off");
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
    fn reminder_styles_move_with_a_takeover_and_go_with_the_account() {
        use torro_core::settings::CalendarReminders;
        let (_dir, store) = store();
        let (from, to) = ("caldav:anna@cloud.example.org", "mail:anna@cloud.example.org");
        let url = "https://cloud.example.org/dav/calendars/anna/work/";
        let (old, new) = (format!("{from}/{url}"), format!("{to}/{url}"));
        store.set(SettingKey::Reminders, &json!({"calendars": {old.clone(): "full_screen"}})).expect("set");
        store.copy_collections(from, to).expect("copied");
        assert_eq!(store.current().reminders.calendar(&new), CalendarReminders::FullScreen, "under its new id");
        store.forget_account(from).expect("forgotten");
        let calendars = store.current().reminders.calendars;
        assert_eq!(calendars.keys().cloned().collect::<Vec<_>>(), [new], "the old id is gone");
    }

    #[test]
    fn forgetting_what_is_not_hidden_writes_nothing() {
        let (_dir, store) = store();
        store.forget_collections(&["work".into()]).expect("nothing to forget");
        assert!(!file_exists(&store));
    }

    #[test]
    fn a_test_store_switches_start_at_login_without_touching_the_login_item() {
        let (_dir, store) = store();
        let (off, on) = (Settings::default(), Settings { start_at_login: true, ..Settings::default() });
        assert_eq!(store.login_item_change(&off, &on), None, "a test store never reaches the system");
        assert_eq!(SettingsStore::without_file().login_item_change(&off, &on), None);

        // Through the real path of a change: stored in the file, nothing else.
        store.set(SettingKey::StartAtLogin, &json!(true)).expect("on");
        assert!(store.current().start_at_login);
        store.set(SettingKey::StartAtLogin, &json!(false)).expect("off");
        assert!(!store.current().start_at_login);
    }

    #[test]
    fn the_daemons_store_applies_only_a_changed_start_at_login() {
        // Built like `system()` but on a temp file, so the decision is checked without a call.
        let dir = tempfile::tempdir().expect("temp dir");
        let store =
            SettingsStore { file: Some(dir.path().join("settings.json")), designs: None, applies_login_item: true };
        assert!(SettingsStore::system().applies_login_item);
        let (off, on) = (Settings::default(), Settings { start_at_login: true, ..Settings::default() });
        assert_eq!(store.login_item_change(&off, &on), Some(true));
        assert_eq!(store.login_item_change(&on, &off), Some(false));
        let german = Settings { language: Language::German, ..on.clone() };
        assert_eq!(store.login_item_change(&on, &german), None, "start at login did not change");
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
