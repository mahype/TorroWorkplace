//! Suite-wide settings, set in the control centre (`torroworkplace ui`) and read by every
//! part of the suite: `settings.json` in the core's config directory.
//!
//! Task S.1 in `docs/tasks/steuerzentrale.md` fills in [`load`] and [`save`]. The type is the
//! contract: add fields with `#[serde(default)]` so older files keep loading.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::AppId;
use crate::paths::AppPaths;
use crate::theme::Appearance;

/// The language the suite speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Follow the system (`LANG` and friends).
    #[default]
    System,
    English,
    German,
}

/// All suite-wide settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Language of the suite: `system` follows the desktop, `english` and `german` fix it.
    /// Applies to processes started afterwards.
    #[serde(default)]
    pub language: Language,
    /// Light or dark colours for every app; `System` follows the desktop.
    #[serde(default)]
    pub appearance: Appearance,
    /// Start `torroworkplace daemon` when the user logs in.
    #[serde(default)]
    pub start_at_login: bool,
    /// Collections (calendar and task list ids) the apps do not show.
    #[serde(default)]
    pub hidden_collections: Vec<String>,
    /// How large every window of the suite draws, like the zoom of a browser (task Z.1). One
    /// value for all apps, so tiled windows stay alike; stored as the factor (`1.1`).
    #[serde(default)]
    pub zoom: Zoom,
    /// When the night is (task K.27): the calendar's timeline folds it, the radar shades it, and
    /// neither counts it as free time. Stored as `{"from": "22:00", "to": "06:00"}`.
    #[serde(default)]
    pub night: Night,
    /// How each task list is ordered, by list id (`<account-id>/<list URL>`, task P.3a). A list
    /// that is not named here is grouped by due date ([`ListOrder::DueDate`]), so only lists in
    /// [`ListOrder::Manual`] are stored.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub list_orders: BTreeMap<String, ListOrder>,
    /// "Quiet during busy events" (task E3.4): while a busy event of a shown calendar runs (not
    /// all-day, not marked free), only urgent inbox entries pop up; the rest is told in one
    /// message when it ends. On unless turned off.
    #[serde(default = "on")]
    pub quiet_when_busy: bool,
    /// The design pack every window draws with (task L.3, `docs/design-pakete.md`): its id, the
    /// folder name under `<data>/designs/`, or `"torro"` ([`crate::design::STANDARD_ID`]) for the
    /// standard. Kept as written even when the pack is gone or broken: the apps then draw with
    /// the standard, and the control centre shows why.
    #[serde(default = "standard_design")]
    pub design: String,
    /// The font every window draws with (task L.22, `docs/design-pakete.md` "Wählen"): its id, the folder
    /// name under `<data>/fonts/`, or `"torro"` ([`crate::design::STANDARD_ID`]) for the standard. Kept as
    /// written like [`Settings::design`]; choosing a design never changes it.
    ///
    /// A settings file without it reads as the standard. Left out of `settings.update_settings` (or sent
    /// as `""`), the stored one stays.
    // Read from JSON without it, it is empty until `Settings::fill_unnamed_parts` fills it in: `load`
    // with the standard, the core with the stored one.
    #[serde(default)]
    pub font: String,
    /// The colour scheme every window draws with (task L.22): its id, the folder name under
    /// `<data>/color-schemes/`, or `"torro"` for the standard. Kept as written; choosing a design never
    /// changes it. A settings file without it reads as the standard. Left out of
    /// `settings.update_settings` (or sent as `""`), the stored one stays.
    // Empty until filled in, like `font`.
    #[serde(default)]
    pub color_scheme: String,
    /// How long the assistant keeps a conversation after its last entry (task C.1, decision 8 in
    /// `docs/assistent.md`): 30 days unless changed, or forever. Pinned conversations stay anyway.
    #[serde(default)]
    pub assistant_retention: Retention,
    /// Talking to the assistant by voice (task C.9, `docs/assistent.md`, "Sprache"): off unless a
    /// person switches it on in the control centre ([`Voice`]).
    #[serde(default)]
    pub voice: Voice,
    /// What a new chat of the assistant starts with (issue #131): engine, model and effort, set in
    /// the control centre › Assistant. `None` (the default): the engine and model used last. Read
    /// leniently: a value that does not read is `None`, never a settings file that fails.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient_default")]
    pub assistant_default: Option<AssistantDefault>,
    /// Where the first-run setup stands (task W.1, `docs/einrichtung.md`): not finished yet,
    /// put off or done, and the version of the setup last finished or put off. Read leniently: a
    /// value that does not read is the default, never a settings file that fails.
    #[serde(default, deserialize_with = "lenient")]
    pub setup: Setup,
    /// The components a person turned off (task W.1); every other one is on. Only the ones turned
    /// off are stored, so a component added later starts on, and so do installations from before.
    /// A name this version does not know is dropped when the file is read.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty", deserialize_with = "known_names")]
    pub components_off: BTreeSet<Component>,
    /// How a mail that has HTML first shows in the reader (task M9.5, decision 9 in
    /// `docs/mail.md`): formatted ([`MailFormat::Html`], the default) or as plain text. The reader
    /// can switch a mail it shows; this is where it starts. Read leniently: a value that does not
    /// read is the default.
    #[serde(default, deserialize_with = "lenient")]
    pub mail_view: MailFormat,
    /// The colours of a mail's content in the reader (task M9.5): like the suite
    /// ([`MailAppearance::App`], the default), or always light or dark. Only the mail itself, never
    /// the rest of the window. Read leniently, like [`Settings::mail_view`].
    #[serde(default, deserialize_with = "lenient")]
    pub mail_appearance: MailAppearance,
    /// Which AI account and model each function of the suite uses (task E12.5, `docs/ki.md`
    /// "Zuordnung je Funktion"), by the function. Only functions a person set up are stored, so a
    /// new one starts unassigned. Read leniently ([`known_assignments`]): an entry that does not
    /// fit, or names a function this version does not know, is dropped and never makes the file
    /// fail.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "known_assignments")]
    pub ai_assignments: BTreeMap<crate::ai::AiFunction, crate::ai::AiAssignment>,
    /// The mail categories the sidebar leaves out (task E12.10, `docs/kategorien.md`). Only the
    /// ones hidden are stored, so a category added later starts shown. A key this version does
    /// not know is dropped when the file is read.
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "known_categories")]
    pub hidden_categories: Vec<torro_mail::Category>,
    /// The data rule for mail (task C.17, `docs/ki.md` principle 4): `false` (the default) keeps
    /// mail with models on the person's own computers; `true` lets it go to models elsewhere
    /// (OpenRouter, a provider's server) as well. Only a person switches it on
    /// (`settings.set_setting mail_to_cloud`, app key only); any client allowed to change settings
    /// may switch it off.
    #[serde(default)]
    pub mail_to_cloud: bool,
    /// Reminders of events (task ER1.2, `docs/erinnerungen.md`): whether they come, which events,
    /// and per calendar as a notification or over the whole screen. Read leniently: a value that
    /// does not read gives the defaults, never a settings file that fails.
    #[serde(default, deserialize_with = "lenient_reminders")]
    pub reminders: Reminders,
    /// "Allow API keys" (#231, `docs/api.md` "Transport"): while off, the core refuses every key of
    /// the kind `script` (`unauthorized`). On unless a person turned it off; only a person turns it
    /// on again (`settings.set_setting allow_api_keys`, app key only), any client allowed to change
    /// settings may turn it off.
    #[serde(default = "on")]
    pub allow_api_keys: bool,
    /// "Allow MCP" (#231): while off, the core refuses every key of an MCP client and of a hook
    /// (`unauthorized`). On unless a person turned it off; turned on again only by a person, like
    /// [`Settings::allow_api_keys`].
    #[serde(default = "on")]
    pub allow_mcp: bool,
    /// The assistant's own tools (issue #225, `docs/assistent.md` "Eigene Werkzeuge"): which ones a
    /// person switched on and how the PDF tool sets its pages ([`Tools`]). Read leniently: a value
    /// that does not read gives the defaults (every tool off), never a settings file that fails.
    #[serde(default, deserialize_with = "lenient")]
    pub tools: Tools,
    /// Workspace templates (issue #243): the modifiers held with the digit keys that load them
    /// ([`WorkspaceSettings`]). Read leniently: a value that does not read gives the defaults of
    /// this system.
    #[serde(default, deserialize_with = "lenient")]
    pub workspaces: WorkspaceSettings,
}

/// What the suite does with workspace templates (issue #243, `settings.set_setting
/// workspace_shortcuts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSettings {
    /// The modifiers held with a digit key `1` … `0` to load the template that has the key, one
    /// choice for all templates; the default of this system unless set.
    #[serde(default)]
    pub modifiers: ShortcutModifiers,
}

/// The modifiers of the template keys (issue #243): pressed with a digit, they load the template
/// with that key. Names as on Linux; on macOS Super is Command and Alt is Option, on Windows Super
/// is the Windows key. The default is per system ([`ShortcutModifiers::default`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutModifiers {
    /// `"super_alt"`: Super + Alt, the default on Linux.
    SuperAlt,
    /// `"super_ctrl"`: Super + Ctrl.
    SuperCtrl,
    /// `"ctrl_alt"`: Ctrl + Alt (Control + Option on macOS), the default on macOS and Windows.
    CtrlAlt,
    /// `"ctrl_shift"`: Ctrl + Shift.
    CtrlShift,
}

impl ShortcutModifiers {
    /// Every choice, in the order the control centre offers them.
    pub const ALL: [ShortcutModifiers; 4] = [
        ShortcutModifiers::SuperAlt,
        ShortcutModifiers::SuperCtrl,
        ShortcutModifiers::CtrlAlt,
        ShortcutModifiers::CtrlShift,
    ];
}

impl Default for ShortcutModifiers {
    /// Super + Alt on Linux (Hyprland binds Super + digit itself), Ctrl + Option on macOS (Command
    /// + Option + digit is taken by many apps), Ctrl + Alt on Windows.
    fn default() -> ShortcutModifiers {
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            ShortcutModifiers::CtrlAlt
        } else {
            ShortcutModifiers::SuperAlt
        }
    }
}

/// How a mail that has HTML first shows in the reader (`settings.set_setting mail_view`, task
/// M9.5). A mail without HTML always shows its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MailFormat {
    /// `"html"`: formatted, as the sender wrote it (the cleaned HTML part).
    #[default]
    Html,
    /// `"text"`: the plain text of the mail.
    Text,
}

/// The colours of a mail's content in the reader (`settings.set_setting mail_appearance`, task
/// M9.5). Dark recolours the mail's HTML for a dark background; pictures stay as they are. It
/// can be switched off because recoloured newsletters often look worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MailAppearance {
    /// `"app"`: light or dark as the suite draws now ([`Settings::appearance`], so `System`
    /// follows the desktop).
    #[default]
    App,
    /// `"light"`: always the mail's own colours.
    Light,
    /// `"dark"`: always recoloured for a dark background.
    Dark,
}

/// How reminders of events come (task ER1.2, `settings.set_setting reminders`), as one object:
/// `{"enabled": true, "defaultLead": 1, "calendars": {"<calendar id>": "full_screen"}, …}`. Read
/// from a file, fields left out have their defaults; a change ([`Reminders::merged`]) touches only
/// the fields it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Reminders {
    /// Reminders come at all. On unless switched off.
    pub enabled: bool,
    /// Minutes before the start for events without a reminder of their own (0 to
    /// [`Reminders::MAX_LEAD`]); `null` for none. One minute unless changed.
    pub default_lead: Option<u32>,
    /// How each calendar shows its reminders, by calendar id from `calendar.list_calendars`. A
    /// calendar that is not named here shows them as a notification.
    pub calendars: BTreeMap<String, CalendarReminders>,
    /// Only events the person accepted (or organises); declined ones never remind. On.
    pub only_accepted: bool,
    /// All-day events remind too. Off.
    pub all_day: bool,
    /// Events marked free (`TRANSP:TRANSPARENT`) remind too. Off.
    pub free_events: bool,
    /// Events outside the work hours remind too. On; it only matters once work hours exist (K.33).
    pub outside_work_hours: bool,
    /// A full-screen reminder covers every display, not only the main one. On.
    pub all_displays: bool,
    /// While an event with a call link runs, a full-screen reminder comes as a notification. On.
    pub never_during_call: bool,
    /// The sound of a reminder. The system's unless changed.
    pub sound: ReminderSound,
}

/// How a calendar shows its reminders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarReminders {
    /// No reminders from this calendar.
    Off,
    /// A desktop notification and an inbox entry, the default.
    #[default]
    Notification,
    /// Over the whole screen until the person reacts.
    FullScreen,
}

/// The sound of a reminder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReminderSound {
    /// The system's notification sound.
    #[default]
    System,
    /// Silent.
    None,
}

impl Default for Reminders {
    fn default() -> Reminders {
        Reminders {
            enabled: true,
            default_lead: Some(1),
            calendars: BTreeMap::new(),
            only_accepted: true,
            all_day: false,
            free_events: false,
            outside_work_hours: true,
            all_displays: true,
            never_during_call: true,
            sound: ReminderSound::System,
        }
    }
}

impl Reminders {
    /// The longest default lead: one week, in minutes.
    pub const MAX_LEAD: u32 = 7 * 24 * 60;

    /// How the calendar `id` shows its reminders.
    pub fn calendar(&self, id: &str) -> CalendarReminders {
        self.calendars.get(id).copied().unwrap_or_default()
    }

    /// The reminders in `value`, or why they do not fit: an object of the fields above (left-out
    /// ones keep their defaults), a lead of at most [`Reminders::MAX_LEAD`] minutes. Calendars
    /// that show notifications, the default, are not stored. For `settings.set_setting`.
    pub fn from_value(value: &serde_json::Value) -> Result<Reminders, String> {
        let mut reminders: Reminders = serde_json::from_value(value.clone())
            .map_err(|err| format!("{value} is not a setting for reminders ({err})"))?;
        if reminders.default_lead.is_some_and(|lead| lead > Self::MAX_LEAD) {
            return Err(format!("the default reminder is at most {} minutes before", Self::MAX_LEAD));
        }
        reminders.calendars.retain(|_, shown| *shown != CalendarReminders::Notification);
        Ok(reminders)
    }

    /// The reminders as `settings.set_setting` takes them.
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// These reminders with the fields `change` names, or why it does not fit: an object whose
    /// fields replace the ones here (left-out fields stay as they are), and whose `calendars` change
    /// only the calendars it names (`"notification"` sets one back to the default). Checked like
    /// [`Reminders::from_value`]. Two clients changing different fields so never undo each other.
    pub fn merged(&self, change: &serde_json::Value) -> Result<Reminders, String> {
        let serde_json::Value::Object(change) = change else {
            return Err(format!("{change} is not a setting for reminders: send an object"));
        };
        let mut merged = match self.to_value() {
            serde_json::Value::Object(current) => current,
            _ => serde_json::Map::new(),
        };
        for (field, value) in change {
            match (field.as_str(), merged.get_mut(field), value) {
                ("calendars", Some(serde_json::Value::Object(calendars)), serde_json::Value::Object(named)) => {
                    calendars.extend(named.iter().map(|(id, shown)| (id.clone(), shown.clone())));
                }
                _ => {
                    merged.insert(field.clone(), value.clone());
                }
            }
        }
        Reminders::from_value(&serde_json::Value::Object(merged))
    }
}

/// `reminders` as written, or the defaults when it does not read.
fn lenient_reminders<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Reminders, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(Reminders::from_value(&value).unwrap_or_default())
}

/// What a new chat of the assistant starts with (issue #131, `settings.set_setting
/// assistant_default`): `{"engine": "ai:ollama@localhost:11434", "model": "qwen3:8b", "effort":
/// "high"}`. The chat window takes it while the engine is offered (`assistant.list_engines`) and
/// otherwise falls back to the one used last; the person can still pick another per message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AssistantDefault {
    /// The engine's id from `assistant.list_engines`.
    pub engine: String,
    /// The model of that engine; `None` lets the engine pick one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How hard the model thinks; `None` leaves it to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<crate::ai::Effort>,
}

impl AssistantDefault {
    /// The longest engine or model id it takes, as `assistant.send_message` does.
    pub const MAX_ID_BYTES: usize = 300;

    /// The default in `value`, `None` for `null`, or why it does not fit: an object with a
    /// non-blank `engine`, ids of at most [`AssistantDefault::MAX_ID_BYTES`] on one line, an effort
    /// of [`crate::ai::Effort`]. For `settings.set_setting`.
    pub fn from_value(value: &serde_json::Value) -> Result<Option<AssistantDefault>, String> {
        if value.is_null() {
            return Ok(None);
        }
        let default: AssistantDefault = serde_json::from_value(value.clone()).map_err(|err| {
            format!("{value} is not a default for the assistant: send engine, model and effort, or null ({err})")
        })?;
        let fits = |id: &str| !id.trim().is_empty() && id.len() <= Self::MAX_ID_BYTES && !id.contains(['\n', '\r']);
        if !fits(&default.engine) || default.model.as_deref().is_some_and(|model| !fits(model)) {
            return Err(format!("an engine or model id is one line of 1 to {} bytes", Self::MAX_ID_BYTES));
        }
        Ok(Some(default))
    }

    /// The default as `settings.set_setting` takes it; `null` for none.
    pub fn to_value(default: Option<&AssistantDefault>) -> serde_json::Value {
        default.and_then(|default| serde_json::to_value(default).ok()).unwrap_or(serde_json::Value::Null)
    }
}

/// `assistantDefault` as written, or `None` when it does not read: a damaged default must not
/// cost the other settings.
fn lenient_default<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<AssistantDefault>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(AssistantDefault::from_value(&value).ok().flatten())
}

/// The value as written, or the default when it does not read: one damaged setting must not cost
/// the others.
fn lenient<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned + Default>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// The assignments as written (task E12.5), without the functions this version does not know (a
/// file from a newer one) and without an entry whose value does not fit: one damaged assignment
/// must not stop the suite from starting.
fn known_assignments<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<crate::ai::AiFunction, crate::ai::AiAssignment>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let entries = value.as_object().cloned().unwrap_or_default();
    Ok(entries
        .into_iter()
        .filter_map(|(key, value)| {
            let function = crate::ai::AiFunction::from_key(&key)?;
            let assignment: crate::ai::AiAssignment = serde_json::from_value(value).ok()?;
            Some((function, assignment))
        })
        .collect())
}

/// The categories in a list as written (task E12.10), without the keys this version does not
/// know and without anything that is not a key.
fn known_categories<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<torro_mail::Category>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let keys = value.as_array().map(Vec::as_slice).unwrap_or_default();
    Ok(keys.iter().filter_map(|key| serde_json::from_value(key.clone()).ok()).collect())
}

/// The components or tools in a list as written, without the names this version does not know (a
/// file written by a newer one) and without anything that is not a name.
fn known_names<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned + Ord>(
    deserializer: D,
) -> Result<BTreeSet<T>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let names = value.as_array().map(Vec::as_slice).unwrap_or_default();
    Ok(names.iter().filter_map(|name| serde_json::from_value(name.clone()).ok()).collect())
}

/// The id of the standard design, font and colour scheme, for settings from before design packs.
fn standard_design() -> String {
    crate::design::STANDARD_ID.to_string()
}

impl Settings {
    /// Puts the font and the colour scheme of `stored` where these settings name none (an empty id: the
    /// field was left out of the JSON they were read from, task L.22). For the settings file `stored` is
    /// [`Settings::default`], so a file without them draws with the standard; for the settings a client
    /// sends it is the stored settings, so a client that does not know the fields keeps the person's
    /// choice.
    pub fn fill_unnamed_parts(&mut self, stored: &Settings) {
        if self.font.is_empty() {
            self.font = stored.font.clone();
        }
        if self.color_scheme.is_empty() {
            self.color_scheme = stored.color_scheme.clone();
        }
    }
}

/// `true`, for settings that are on unless the file says otherwise.
fn on() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            language: Language::default(),
            appearance: Appearance::default(),
            start_at_login: false,
            hidden_collections: Vec::new(),
            zoom: Zoom::default(),
            night: Night::default(),
            list_orders: BTreeMap::new(),
            quiet_when_busy: on(),
            design: standard_design(),
            font: standard_design(),
            color_scheme: standard_design(),
            assistant_retention: Retention::default(),
            voice: Voice::default(),
            assistant_default: None,
            setup: Setup::default(),
            components_off: BTreeSet::new(),
            mail_view: MailFormat::default(),
            mail_appearance: MailAppearance::default(),
            mail_to_cloud: false,
            reminders: Reminders::default(),
            ai_assignments: BTreeMap::new(),
            hidden_categories: Vec::new(),
            allow_api_keys: on(),
            allow_mcp: on(),
            tools: Tools::default(),
            workspaces: WorkspaceSettings::default(),
        }
    }
}

/// A part of the suite a person can turn off (task W.1, `docs/einrichtung.md`, "Komponenten an
/// und aus"). Off means: not in the app switcher, the tray and the overview, and nothing runs in
/// the background only for it. Its accounts and data stay, and its requests keep answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    /// The calendar app and the calendars of the accounts.
    Calendar,
    /// The tasks app and the task lists of the accounts.
    Tasks,
    /// Mail: the mail accounts and their fetching.
    Mail,
    /// The inbox and its sources.
    Inbox,
    /// The assistant, its chat window and listening by voice.
    Assistant,
    /// Saving and loading workspace templates of the tiling manager.
    Workspaces,
}

impl Component {
    /// Every component, in the order the setup and the Apps page show them.
    pub const ALL: [Component; 6] = [
        Component::Calendar,
        Component::Tasks,
        Component::Mail,
        Component::Inbox,
        Component::Assistant,
        Component::Workspaces,
    ];
}

/// The `value` of the setting `component` (`settings.set_setting`, task W.1): one component and
/// whether it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentSwitch {
    /// The component: `calendar`, `tasks`, `mail`, `inbox`, `assistant` or `workspaces`.
    pub component: Component,
    /// `true` turns it on, `false` off.
    pub enabled: bool,
}

/// Where the first-run setup stands (task W.1, `docs/einrichtung.md`, "Wann sie erscheint").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Setup {
    /// Not finished yet, put off or done.
    #[serde(default)]
    pub state: SetupState,
    /// The version of the setup ([`Setup::VERSION`]) that was last finished or put off; 0 before.
    /// When it is lower than the current one, the setup has steps the person has not seen.
    #[serde(default)]
    pub version: u32,
}

impl Setup {
    /// The version of the setup this build brings. Raised when a release adds a step the person
    /// should be told about ("New in setup: AI").
    pub const VERSION: u32 = 1;

    /// The setup after a person moved it to `state`: finishing or putting it off records this
    /// build's version; starting it again keeps the version of the last run.
    pub fn moved_to(self, state: SetupState) -> Setup {
        let version = match state {
            SetupState::Pending => self.version,
            SetupState::Skipped | SetupState::Done => Setup::VERSION,
        };
        Setup { state, version }
    }

    /// The setup was finished or put off with an older version, so it has steps the person has not
    /// seen. Never while it is still pending: then all of it is new anyway.
    pub fn has_new_steps(self) -> bool {
        self.state != SetupState::Pending && self.version < Setup::VERSION
    }
}

/// How far the first-run setup got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupState {
    /// Not finished: on a first start without accounts the core opens it.
    #[default]
    Pending,
    /// Put off with "Set up later": the overview offers to continue it.
    Skipped,
    /// Finished.
    Done,
}

/// A time of day on the quarter hour, `00:00` to `23:45`, the grid of [`Night`]. Kept as minutes
/// since midnight; written and read as `"HH:MM"` (its `Display`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Quarter(u16);

impl Quarter {
    /// Minutes in a day.
    const DAY: u16 = 24 * 60;

    /// `hour:minute`, or `None` when the hour is 24 or more or the minute is not 0, 15, 30 or 45.
    pub fn new(hour: u32, minute: u32) -> Option<Quarter> {
        (hour < 24 && minute < 60 && minute.is_multiple_of(15)).then(|| Quarter((hour * 60 + minute) as u16))
    }

    /// Every quarter of a day in order, `00:00` to `23:45` (96), for the time fields.
    pub fn all() -> impl Iterator<Item = Quarter> {
        (0..Quarter::DAY).step_by(15).map(Quarter)
    }

    /// `"22:00"` as a quarter; `None` for anything else, also `"22:10"` or `"7:00"`.
    pub fn parse(text: &str) -> Option<Quarter> {
        let (hour, minute) = text.split_once(':')?;
        let digits = |part: &str| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit());
        if !digits(hour) || !digits(minute) {
            return None;
        }
        Quarter::new(hour.parse().ok()?, minute.parse().ok()?)
    }

    pub fn hour(self) -> u32 {
        u32::from(self.0 / 60)
    }

    pub fn minute(self) -> u32 {
        u32::from(self.0 % 60)
    }

    /// Minutes since midnight.
    pub fn minutes(self) -> u32 {
        u32::from(self.0)
    }
}

impl std::fmt::Display for Quarter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}", self.hour(), self.minute())
    }
}

/// The night of the suite (task K.27): from `from` to `to`, mostly over midnight (22:00–06:00).
/// `from == to` means no night at all: nothing is folded or shaded.
///
/// Reading a settings file is lenient, like [`Zoom`]: a night that is not two quarter hours gives
/// [`Night::DEFAULT`], so a hand-edited file never keeps the suite from starting. A value sent to
/// the core is checked with [`Night::from_value`], which refuses it with a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Night {
    pub from: Quarter,
    pub to: Quarter,
}

impl Night {
    /// 22:00 to 06:00.
    pub const DEFAULT: Night = Night { from: Quarter(22 * 60), to: Quarter(6 * 60) };

    /// No night (both ends at `at`).
    pub fn none_at(at: Quarter) -> Night {
        Night { from: at, to: at }
    }

    /// There is no night: `from == to`.
    pub fn is_none(self) -> bool {
        self.from == self.to
    }

    /// How long the night lasts, in minutes (480 for 22:00–06:00, 0 for none).
    pub fn minutes(self) -> u32 {
        (self.to.minutes() + u32::from(Quarter::DAY) - self.from.minutes()) % u32::from(Quarter::DAY)
    }

    /// The night in `value`, which must be `{"from": "HH:MM", "to": "HH:MM"}` on the quarter hour;
    /// otherwise why not. For `settings.set_setting`.
    pub fn from_value(value: &serde_json::Value) -> Result<Night, String> {
        let shape = || format!("{value} is not a night: send {{\"from\": \"22:00\", \"to\": \"06:00\"}}");
        let object = value.as_object().ok_or_else(shape)?;
        if object.keys().any(|key| key != "from" && key != "to") {
            return Err(shape());
        }
        let end = |name: &str| {
            let text = object.get(name).and_then(|v| v.as_str()).ok_or_else(shape)?;
            Quarter::parse(text).ok_or_else(|| {
                format!("{text:?} is not a time for {name}: use HH:MM on the quarter hour (00, 15, 30, 45)")
            })
        };
        Ok(Night { from: end("from")?, to: end("to")? })
    }

    /// The night as `settings.set_setting` takes it: `{"from": "22:00", "to": "06:00"}`.
    pub fn to_value(self) -> serde_json::Value {
        serde_json::json!({ "from": self.from.to_string(), "to": self.to.to_string() })
    }
}

impl Default for Night {
    fn default() -> Night {
        Night::DEFAULT
    }
}

impl Serialize for Night {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Night {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Night, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Night::from_value(&value).unwrap_or_default())
    }
}

impl JsonSchema for Night {
    fn schema_name() -> Cow<'static, str> {
        "Night".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        let time = json_schema!({ "type": "string", "pattern": "^([01][0-9]|2[0-3]):(00|15|30|45)$" });
        json_schema!({
            "type": "object",
            "properties": { "from": time, "to": time },
            "required": ["from", "to"],
            "additionalProperties": false,
            "description": "The night as two times of day on the quarter hour, usually over midnight \
                            ({\"from\": \"22:00\", \"to\": \"06:00\"}). The same time twice means no night."
        })
    }
}

/// How long the assistant keeps a conversation after its last entry (task C.1): a number of days,
/// or forever. Written as the number (`30`) or `"forever"`.
///
/// Reading a settings file is lenient, like [`Night`], but towards keeping: a value that does not
/// read gives [`Retention::Forever`], because a damaged file must never delete conversations the
/// person meant to keep. A file without the field has the default of 30 days. A value sent to the
/// core is checked with [`Retention::from_value`], which refuses it with a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Retention {
    /// This many days, from 1 to [`Retention::MAX_DAYS`].
    Days(u16),
    /// Conversations are kept until they are deleted.
    Forever,
}

impl Retention {
    /// The longest number of days: ten years. Longer is "forever".
    pub const MAX_DAYS: u16 = 3650;

    /// 30 days (decision 8 in `docs/assistent.md`).
    pub const DEFAULT: Retention = Retention::Days(30);

    /// The retention in `value`: a whole number of days from 1 to [`Retention::MAX_DAYS`], or
    /// `"forever"`; otherwise why not. For `settings.set_setting`.
    pub fn from_value(value: &serde_json::Value) -> Result<Retention, String> {
        if value.as_str() == Some("forever") {
            return Ok(Retention::Forever);
        }
        value
            .as_u64()
            .filter(|days| (1..=u64::from(Retention::MAX_DAYS)).contains(days))
            .and_then(|days| u16::try_from(days).ok())
            .map(Retention::Days)
            .ok_or_else(|| {
                format!("{value} is not a retention: send days from 1 to {} or \"forever\"", Retention::MAX_DAYS)
            })
    }

    /// The retention as `settings.set_setting` takes it: the number of days or `"forever"`.
    pub fn to_value(self) -> serde_json::Value {
        match self {
            Retention::Days(days) => serde_json::Value::from(days),
            Retention::Forever => serde_json::Value::from("forever"),
        }
    }

    /// How many days after its last entry a conversation is kept; `None` for forever.
    pub fn days(self) -> Option<u16> {
        match self {
            Retention::Days(days) => Some(days),
            Retention::Forever => None,
        }
    }
}

impl Default for Retention {
    fn default() -> Retention {
        Retention::DEFAULT
    }
}

impl Serialize for Retention {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Retention {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Retention, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Retention::from_value(&value).unwrap_or(Retention::Forever))
    }
}

impl JsonSchema for Retention {
    fn schema_name() -> Cow<'static, str> {
        "Retention".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "anyOf": [
                { "type": "integer", "minimum": 1, "maximum": Retention::MAX_DAYS },
                { "const": "forever" }
            ],
            "description": "How many days the assistant keeps a conversation after its last entry, or \"forever\". \
                            Pinned conversations are always kept."
        })
    }
}

/// The wake word while it cannot be chosen yet (decision 11 in `docs/assistent.md`).
pub const WAKE_WORD: &str = "hey torro";

/// Talking to the assistant by voice (task C.9): the listener `torroassistant --listen` waits for
/// the wake word, records what follows, turns it into text and sends it to the assistant; the
/// answer is shown and, if wanted, spoken. Everything runs on this computer with programs the
/// person installed (openWakeWord, whisper.cpp, Piper).
///
/// Switching it on is a person's switch (point 6 of "Was nie nach außen geht" in `docs/api.md`):
/// only the app key may set [`Voice::enabled`] to `true`; anyone allowed to change settings may
/// switch it off. A file without the entry, or one that does not read, leaves it off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    /// The listener runs: the microphone is open for the wake word. Off unless switched on.
    #[serde(default)]
    pub enabled: bool,
    /// The phrase that wakes the assistant; fixed to [`WAKE_WORD`] for now.
    #[serde(default = "wake_word")]
    pub wake_word: String,
    /// The microphone by the name the recording program knows it (`pactl list sources`); `None`
    /// for the system's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microphone: Option<String>,
    /// Speak the answers with Piper as well as showing them. On unless turned off.
    #[serde(default = "on")]
    pub speak_answers: bool,
    /// The whisper.cpp model file (`ggml-base.bin`), an absolute path; `None` for the first
    /// model found in the voice folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stt_model: Option<String>,
    /// The Piper voice (`de_DE-thorsten-medium.onnx`, its `.onnx.json` next to it), an absolute
    /// path; `None` for the first voice found in the voice folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

fn wake_word() -> String {
    WAKE_WORD.to_string()
}

impl Default for Voice {
    fn default() -> Voice {
        Voice {
            enabled: false,
            wake_word: wake_word(),
            microphone: None,
            speak_answers: on(),
            stt_model: None,
            voice: None,
        }
    }
}

/// The longest microphone name `settings.set_setting` takes, in characters.
const MAX_MICROPHONE_CHARS: usize = 200;

/// The longest model or voice path `settings.set_setting` takes, in bytes.
const MAX_PATH_BYTES: usize = 4096;

impl Voice {
    /// The wake word in `value`: [`WAKE_WORD`] in any case and spacing; anything else is refused
    /// until the wake word can be chosen (decision 11). For `settings.set_setting`.
    pub fn wake_word_from(value: &serde_json::Value) -> Result<String, String> {
        let words = value.as_str().map(|text| text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase());
        match words.as_deref() {
            Some(WAKE_WORD) => Ok(WAKE_WORD.to_string()),
            _ => Err(format!("{value} is not a wake word: only \"{WAKE_WORD}\" can be used for now")),
        }
    }

    /// The microphone in `value`: a name, or `null`, `""` or `"default"` for the system's default.
    /// A name is one line of at most 200 characters and does not start with `-`, so a recording
    /// program never reads it as an option. For `settings.set_setting`.
    pub fn microphone_from(value: &serde_json::Value) -> Result<Option<String>, String> {
        let refused = || {
            format!(
                "{value} is not a microphone: send its name (one line, at most {MAX_MICROPHONE_CHARS} characters) or null"
            )
        };
        let name = match value {
            serde_json::Value::Null => return Ok(None),
            serde_json::Value::String(name) => name.trim(),
            _ => return Err(refused()),
        };
        if name.is_empty() || name == "default" {
            return Ok(None);
        }
        let fits = name.chars().count() <= MAX_MICROPHONE_CHARS
            && !name.starts_with('-')
            && !name.chars().any(char::is_control);
        fits.then(|| Some(name.to_string())).ok_or_else(refused)
    }

    /// A model or voice file in `value`: an absolute path ending in `.{extension}`, or `null` or
    /// `""` for the first one found in the voice folder. Whether the file exists is the
    /// listener's check: it may be on a drive that is not mounted yet. For `settings.set_setting`.
    pub fn file_from(value: &serde_json::Value, extension: &str) -> Result<Option<String>, String> {
        let refused = || format!("{value} is not a .{extension} file: send an absolute path or null");
        let path = match value {
            serde_json::Value::Null => return Ok(None),
            serde_json::Value::String(path) => path.trim(),
            _ => return Err(refused()),
        };
        if path.is_empty() {
            return Ok(None);
        }
        let fits = path.len() <= MAX_PATH_BYTES
            && !path.chars().any(char::is_control)
            && Path::new(path).is_absolute()
            && Path::new(path).extension().is_some_and(|found| found.eq_ignore_ascii_case(extension));
        fits.then(|| Some(path.to_string())).ok_or_else(refused)
    }
}

/// One of the assistant's own tools (issue #225, `docs/assistent.md` "Eigene Werkzeuge"): a skill
/// that only the assistant and its bots have, with a switch of its own ([`Tools::enabled`]) and a
/// place in the rights (`torro_ipc::api::Rights::tools`). `torro_ipc::tools` passes it on as the
/// API's type; it lives here because the settings name it.
///
/// A command of a tool runs only when the tool is switched on, the client's rights in the area
/// `tools` cover it and the tool is in the client's selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolId {
    /// `pdf`: sets a PDF from Markdown after one of three templates and saves it
    /// (`tools.create_pdf`); its files are opened and saved elsewhere with `tools.open_file` and
    /// `tools.save_file_as`.
    Pdf,
}

impl ToolId {
    /// Every tool, in the order the settings and the control centre show them.
    pub const ALL: [ToolId; 1] = [ToolId::Pdf];

    /// The id on the wire and in a selection of the rights: `pdf`.
    pub fn name(self) -> &'static str {
        match self {
            ToolId::Pdf => "pdf",
        }
    }

    /// What people call it, the same in every language: `PDF`.
    pub fn title(self) -> &'static str {
        match self {
            ToolId::Pdf => "PDF",
        }
    }

    /// The tool the catalogue command `name` belongs to: `tools.create_pdf`, `tools.open_file` and
    /// `tools.save_file_as` to [`ToolId::Pdf`] (every file a tool makes is a PDF for now). `None`
    /// for `tools.list_tools`, which belongs to no tool, and for every other name. The core checks
    /// the switch and the selection of exactly this tool before such a command runs.
    pub fn of_command(name: &str) -> Option<ToolId> {
        match name {
            "tools.create_pdf" | "tools.open_file" | "tools.save_file_as" => Some(ToolId::Pdf),
            _ => None,
        }
    }
}

/// The value of the setting `tool_enabled` (`settings.set_setting`): one tool and whether it is
/// on. Only a person switches a tool on (point 9 of "Was nie nach außen geht" in `docs/api.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolSwitch {
    /// The tool: `pdf`.
    pub tool: ToolId,
    /// `true` switches it on, `false` off.
    pub enabled: bool,
}

/// One of the three fixed page templates of the PDF tool (decision 16 in `docs/assistent.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PdfTemplateId {
    /// `letter`: a letter, wide margins, no page numbers.
    Letter,
    /// `report`: a report with the title in the header and page numbers; what `tools.create_pdf`
    /// uses when it names no template.
    #[default]
    Report,
    /// `note`: a short note, narrow margins, small type, nothing in header or footer.
    Note,
}

impl PdfTemplateId {
    /// Every template, in the order the settings show them.
    pub const ALL: [PdfTemplateId; 3] = [PdfTemplateId::Letter, PdfTemplateId::Report, PdfTemplateId::Note];

    /// The id on the wire: `letter`, `report`, `note`.
    pub fn name(self) -> &'static str {
        match self {
            PdfTemplateId::Letter => "letter",
            PdfTemplateId::Report => "report",
            PdfTemplateId::Note => "note",
        }
    }
}

/// The paper a PDF template sets its pages on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Paper {
    /// `a4`: 210 × 297 mm.
    #[default]
    A4,
    /// `a5`: 148 × 210 mm.
    A5,
    /// `letter`: US Letter, 8.5 × 11 in.
    Letter,
}

/// How one PDF template sets its pages (setting `pdf_template`). The font is always the text and
/// heading font of the chosen font ([`Settings::font`], task L.22), so a PDF looks like the suite.
///
/// Checked by [`PdfTemplate::check`] whenever it comes from outside; a hand-edited file is read as
/// it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PdfTemplate {
    /// The paper: `a4`, `a5` or `letter`.
    pub paper: Paper,
    /// The margins in millimetres, top, right, bottom, left (as in CSS), each from
    /// [`PdfTemplate::MIN_MARGIN_MM`] to [`PdfTemplate::MAX_MARGIN_MM`].
    pub margins_mm: [u8; 4],
    /// The size of the body text in points, from [`PdfTemplate::MIN_FONT_PT`] to
    /// [`PdfTemplate::MAX_FONT_PT`]; headings scale with it.
    pub font_size_pt: u8,
    /// The line at the top of every page, at most [`PdfTemplate::MAX_LINE_CHARS`] characters on
    /// one line; `{title}` becomes the document's title and `{date}` the day it was made. Empty for
    /// none.
    #[serde(default)]
    pub header: String,
    /// The line at the bottom of every page, like [`PdfTemplate::header`]. Empty for none.
    #[serde(default)]
    pub footer: String,
    /// "Page 1 of 3" at the bottom of every page.
    #[serde(default)]
    pub page_numbers: bool,
    /// A picture in the header: an absolute path to a `.png`, `.jpg` or `.jpeg` file; `None` for
    /// none. Only a person sets one (point 9 of "Was nie nach außen geht": it reads a file into the
    /// PDF); anyone allowed to change settings may remove it. Whether the file exists is checked
    /// when a PDF is made: it may be on a drive that is not mounted yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
}

impl PdfTemplate {
    /// The narrowest margin, in millimetres.
    pub const MIN_MARGIN_MM: u8 = 5;
    /// The widest margin, in millimetres.
    pub const MAX_MARGIN_MM: u8 = 50;
    /// The smallest body text, in points.
    pub const MIN_FONT_PT: u8 = 8;
    /// The largest body text, in points.
    pub const MAX_FONT_PT: u8 = 16;
    /// The longest header or footer, in characters.
    pub const MAX_LINE_CHARS: usize = 200;

    /// The template `id` as it comes: Letter A4, margins 25, 11 pt, nothing in header and footer;
    /// Report A4, margins 20, 11 pt, the title in the header, page numbers; Note A4, margins 15,
    /// 10 pt, nothing in header and footer. No logo.
    pub fn standard(id: PdfTemplateId) -> PdfTemplate {
        let (margin, font_size_pt, header, page_numbers) = match id {
            PdfTemplateId::Letter => (25, 11, "", false),
            PdfTemplateId::Report => (20, 11, "{title}", true),
            PdfTemplateId::Note => (15, 10, "", false),
        };
        PdfTemplate {
            paper: Paper::A4,
            margins_mm: [margin; 4],
            font_size_pt,
            header: header.to_string(),
            footer: String::new(),
            page_numbers,
            logo: None,
        }
    }

    /// Whether the template fits, else why not: every margin within 5 and 50 mm, a body text of 8
    /// to 16 pt, header and footer one line of at most 200 characters, a logo an absolute path to a
    /// `.png`, `.jpg` or `.jpeg` file.
    pub fn check(&self) -> Result<(), String> {
        let margins = Self::MIN_MARGIN_MM..=Self::MAX_MARGIN_MM;
        if !self.margins_mm.iter().all(|margin| margins.contains(margin)) {
            return Err(format!(
                "margins are {} to {} mm each (top, right, bottom, left)",
                Self::MIN_MARGIN_MM,
                Self::MAX_MARGIN_MM
            ));
        }
        if !(Self::MIN_FONT_PT..=Self::MAX_FONT_PT).contains(&self.font_size_pt) {
            return Err(format!("the font size is {} to {} pt", Self::MIN_FONT_PT, Self::MAX_FONT_PT));
        }
        for (name, line) in [("header", &self.header), ("footer", &self.footer)] {
            if line.chars().count() > Self::MAX_LINE_CHARS || line.chars().any(char::is_control) {
                return Err(format!("the {name} is one line of at most {} characters", Self::MAX_LINE_CHARS));
            }
        }
        let picture = |path: &str| {
            Path::new(path)
                .extension()
                .and_then(|found| found.to_str())
                .is_some_and(|found| ["png", "jpg", "jpeg"].iter().any(|known| found.eq_ignore_ascii_case(known)))
        };
        match self.logo.as_deref() {
            Some(logo) if !absolute_path(logo) || !picture(logo) => {
                Err(format!("{logo:?} is not a logo: send an absolute path to a .png, .jpg or .jpeg file, or null"))
            }
            _ => Ok(()),
        }
    }

    /// The template in `value`, or why it does not fit: `{"template": "letter" | "report" | "note",
    /// "paper": …, "marginsMm": […], "fontSizePt": …, "header": …, "footer": …, "pageNumbers": …,
    /// "logo": …}`; header, footer, page numbers and logo may be left out (empty, off, none), and a
    /// logo of `""` is none. It replaces the template whole. Checked by [`PdfTemplate::check`]. For
    /// `settings.set_setting pdf_template`.
    pub fn from_value(value: &serde_json::Value) -> Result<(PdfTemplateId, PdfTemplate), String> {
        let refused = |err: String| format!("{value} is not a PDF template: {err}");
        let serde_json::Value::Object(fields) = value else {
            return Err(refused("send an object with `template` and the template's fields".into()));
        };
        let mut fields = fields.clone();
        let id = fields.remove("template").ok_or_else(|| refused("`template` is missing".into()))?;
        let id: PdfTemplateId = serde_json::from_value(id).map_err(|err| refused(err.to_string()))?;
        let mut template: PdfTemplate =
            serde_json::from_value(serde_json::Value::Object(fields)).map_err(|err| refused(err.to_string()))?;
        if template.logo.as_deref().is_some_and(|logo| logo.trim().is_empty()) {
            template.logo = None;
        }
        template.check().map_err(refused)?;
        Ok((id, template))
    }

    /// The template `id` as `settings.set_setting pdf_template` takes it.
    pub fn to_value(&self, id: PdfTemplateId) -> serde_json::Value {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let serde_json::Value::Object(fields) = &mut value {
            fields.insert("template".into(), serde_json::Value::String(id.name().into()));
        }
        value
    }
}

/// The settings of the PDF tool: where its files go and the three templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PdfSettings {
    /// The folder new PDFs are saved in, an absolute path; `None` for the desktop (or the home
    /// folder when there is none). Only a person sets one (point 9 of "Was nie nach außen geht":
    /// a folder in a synchronised directory would carry content off the computer); anyone allowed
    /// to change settings may remove it. A folder that does not exist when a PDF is made is passed
    /// over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The template `letter`.
    pub letter: PdfTemplate,
    /// The template `report`.
    pub report: PdfTemplate,
    /// The template `note`.
    pub note: PdfTemplate,
}

impl Default for PdfSettings {
    fn default() -> PdfSettings {
        PdfSettings {
            folder: None,
            letter: PdfTemplate::standard(PdfTemplateId::Letter),
            report: PdfTemplate::standard(PdfTemplateId::Report),
            note: PdfTemplate::standard(PdfTemplateId::Note),
        }
    }
}

impl PdfSettings {
    /// The template `id`.
    pub fn template(&self, id: PdfTemplateId) -> &PdfTemplate {
        match id {
            PdfTemplateId::Letter => &self.letter,
            PdfTemplateId::Report => &self.report,
            PdfTemplateId::Note => &self.note,
        }
    }

    /// The template `id`, to change it.
    pub fn template_mut(&mut self, id: PdfTemplateId) -> &mut PdfTemplate {
        match id {
            PdfTemplateId::Letter => &mut self.letter,
            PdfTemplateId::Report => &mut self.report,
            PdfTemplateId::Note => &mut self.note,
        }
    }

    /// The folder in `value`: an absolute path, or `null` or `""` for the desktop. Whether it
    /// exists is checked when a PDF is made. For `settings.set_setting pdf_folder`.
    pub fn folder_from(value: &serde_json::Value) -> Result<Option<String>, String> {
        let refused = || format!("{value} is not a folder: send an absolute path or null");
        let path = match value {
            serde_json::Value::Null => return Ok(None),
            serde_json::Value::String(path) => path.trim(),
            _ => return Err(refused()),
        };
        match path.is_empty() {
            true => Ok(None),
            false => absolute_path(path).then(|| Some(path.to_string())).ok_or_else(refused),
        }
    }

    /// Whether the folder and the templates that differ from `stored` fit, else why not: a folder
    /// an absolute path, each template as [`PdfTemplate::check`] wants it. What equals `stored` is
    /// not checked again: a hand-edited file is read as it is and must not block changing anything
    /// else. For `settings.update_settings`, which carries these settings whole.
    pub fn check_changes(&self, stored: &PdfSettings) -> Result<(), String> {
        if self.folder != stored.folder
            && let Some(folder) = &self.folder
            && !absolute_path(folder)
        {
            return Err(format!("{folder:?} is not a folder: send an absolute path or null"));
        }
        for id in PdfTemplateId::ALL {
            let template = self.template(id);
            if template != stored.template(id) {
                template.check().map_err(|err| format!("the PDF template {:?} does not fit: {err}", id.name()))?;
            }
        }
        Ok(())
    }
}

/// The assistant's own tools (issue #225): which are switched on, and the settings of each.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Tools {
    /// The tools a person switched on; every other one is off, also one added later. A name this
    /// version does not know is dropped when the file is read.
    #[serde(skip_serializing_if = "BTreeSet::is_empty", deserialize_with = "known_names")]
    pub enabled: BTreeSet<ToolId>,
    /// The PDF tool. Boxed: three templates would make [`Settings`], which requests carry whole
    /// (`settings.update_settings`), much larger than every other request.
    pub pdf: Box<PdfSettings>,
}

impl Tools {
    /// Whether `tool` is switched on.
    pub fn is_on(&self, tool: ToolId) -> bool {
        self.enabled.contains(&tool)
    }

    /// Switches `tool` on or off.
    pub fn set_on(&mut self, tool: ToolId, on: bool) {
        if on {
            self.enabled.insert(tool);
        } else {
            self.enabled.remove(&tool);
        }
    }

    /// Whether going from these tools (as stored) to `new` does what only a person may (point 9 of
    /// "Was nie nach außen geht"): switch a tool on, set a folder other than the stored one, or set
    /// a logo other than the stored one. Removing a folder or a logo is no such change. The core
    /// refuses a client's `settings.update_settings` with it.
    pub fn needs_a_person(&self, new: &Tools) -> bool {
        let switches_on = !new.enabled.is_subset(&self.enabled);
        let set_anew = |stored: &Option<String>, new: &Option<String>| new.is_some() && new != stored;
        let folder = set_anew(&self.pdf.folder, &new.pdf.folder);
        let logo =
            PdfTemplateId::ALL.into_iter().any(|id| set_anew(&self.pdf.template(id).logo, &new.pdf.template(id).logo));
        switches_on || folder || logo
    }

    /// `sent` as a client may store it over these tools (as stored), for `settings.update_settings`:
    /// a tool stays on only when it is on in both; the folder and each logo are removed when `sent`
    /// removes them and otherwise stay as stored; the other fields of the templates are taken from
    /// `sent`. So a client removes a path like any admin change, and a person who switched a tool
    /// off or removed a path between the core's check and the write is not undone.
    pub fn from_client(&self, sent: &Tools) -> Tools {
        let mut tools = sent.clone();
        tools.enabled = self.enabled.intersection(&sent.enabled).copied().collect();
        tools.pdf.folder = kept_or_removed(&self.pdf.folder, &sent.pdf.folder);
        for id in PdfTemplateId::ALL {
            tools.pdf.template_mut(id).logo = kept_or_removed(&self.pdf.template(id).logo, &sent.pdf.template(id).logo);
        }
        tools
    }
}

/// A path as a client may store it: `None` when it sends none (removing is an admin change), else
/// the stored one (setting a path is a person's, point 9).
pub fn kept_or_removed(stored: &Option<String>, sent: &Option<String>) -> Option<String> {
    sent.as_ref().and(stored.clone())
}

/// Whether `path` can be a path from outside: one line of at most [`MAX_PATH_BYTES`], absolute on
/// this system.
fn absolute_path(path: &str) -> bool {
    path.len() <= MAX_PATH_BYTES && !path.chars().any(char::is_control) && Path::new(path).is_absolute()
}

impl Settings {
    /// Whether `component` is on (task W.1): every component is, unless a person turned it off.
    pub fn component_on(&self, component: Component) -> bool {
        !self.components_off.contains(&component)
    }

    /// Turns `component` on or off.
    pub fn set_component(&mut self, component: Component, on: bool) {
        if on {
            self.components_off.remove(&component);
        } else {
            self.components_off.insert(component);
        }
    }

    /// How the task list `list` (an id) is ordered: as stored, else grouped by due date.
    pub fn list_order(&self, list: &str) -> ListOrder {
        self.list_orders.get(list).copied().unwrap_or_default()
    }

    /// Orders the task list `list` by `order`. The default ([`ListOrder::DueDate`]) is not
    /// stored, so it removes the entry.
    pub fn set_list_order(&mut self, list: &str, order: ListOrder) {
        match order {
            ListOrder::DueDate => {
                self.list_orders.remove(list);
            }
            ListOrder::Manual => {
                self.list_orders.insert(list.to_string(), order);
            }
        }
    }
}

/// How the tasks of one list are shown (task P.3a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListOrder {
    /// Grouped by due date (overdue, today, later, no date): how every list starts.
    #[default]
    DueDate,
    /// In the order the user put them by hand: by `X-APPLE-SORT-ORDER`, as Nextcloud Tasks
    /// shows a list sorted manually (`torro_pim::source::manual_order`).
    Manual,
}

/// The `value` of the setting `list_order` (`settings.set_setting`, task P.3a): one task list and
/// how it is to be ordered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListOrderEntry {
    /// Id of the task list, as `tasks.list_lists` reports it.
    pub list: String,
    /// `"due_date"` (grouped by due date, the default) or `"manual"` (by hand).
    pub order: ListOrder,
}

/// The zoom of the suite's windows: one of the steps of a browser ([`Zoom::STEPS`], 67 % to
/// 200 %). The apps scale the whole window with it (Iced `scale_factor`), so text, spacing and
/// layout grow together.
///
/// Kept as a whole percentage, so the steps compare exactly and `Settings` stays `Eq`. In JSON it
/// is the factor (`1.1` for 110 %); reading rounds any number to the nearest step
/// ([`Zoom::nearest`]), so a hand-edited file never keeps the suite from starting. Whether a
/// value sent to the core is in range is [`Zoom::from_factor`]'s check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Zoom(u16);

impl Zoom {
    /// The steps in percent, as in Firefox and Chrome.
    pub const STEPS: [u16; 10] = [67, 75, 80, 90, 100, 110, 125, 150, 175, 200];
    /// 100 %.
    pub const DEFAULT: Zoom = Zoom(100);
    /// The smallest step (67 %).
    pub const MIN: Zoom = Zoom(Zoom::STEPS[0]);
    /// The largest step (200 %).
    pub const MAX: Zoom = Zoom(Zoom::STEPS[Zoom::STEPS.len() - 1]);

    /// The step nearest to `factor` (`1.1` → 110 %), or `None` when `factor` is not a number
    /// between 0.67 and 2.0. Compared in whole percent, so `0.666` still counts as 67 %. For
    /// values from outside (`settings.set_setting`), which are refused when out of range.
    pub fn from_factor(factor: f64) -> Option<Zoom> {
        let percent = (factor * 100.0).round();
        let range = f64::from(Zoom::MIN.0)..=f64::from(Zoom::MAX.0);
        range.contains(&percent).then(|| Zoom::nearest(factor))
    }

    /// The step nearest to `factor`; out of range it is the smallest or largest step, a value
    /// that is not a number gives [`Zoom::DEFAULT`]. Halfway between two steps the smaller wins.
    pub fn nearest(factor: f64) -> Zoom {
        if !factor.is_finite() {
            return Zoom::DEFAULT;
        }
        let percent = factor * 100.0;
        let distance = |step: u16| (f64::from(step) - percent).abs();
        let mut best = Zoom::MIN.0;
        for step in Zoom::STEPS {
            if distance(step) < distance(best) {
                best = step;
            }
        }
        Zoom(best)
    }

    /// The factor for Iced's `scale_factor` (1.1 for 110 %).
    pub fn factor(self) -> f32 {
        f32::from(self.0) / 100.0
    }

    /// The whole percentage (110), for showing it.
    pub fn percent(self) -> u16 {
        self.0
    }

    /// The next larger step; the largest stays.
    pub fn larger(self) -> Zoom {
        Zoom::STEPS.into_iter().find(|&step| step > self.0).map_or(Zoom::MAX, Zoom)
    }

    /// The next smaller step; the smallest stays.
    pub fn smaller(self) -> Zoom {
        Zoom::STEPS.into_iter().rev().find(|&step| step < self.0).map_or(Zoom::MIN, Zoom)
    }
}

impl Default for Zoom {
    fn default() -> Zoom {
        Zoom::DEFAULT
    }
}

impl Serialize for Zoom {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(f64::from(self.0) / 100.0)
    }
}

impl<'de> Deserialize<'de> for Zoom {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Zoom, D::Error> {
        f64::deserialize(deserializer).map(Zoom::nearest)
    }
}

impl JsonSchema for Zoom {
    fn schema_name() -> Cow<'static, str> {
        "Zoom".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "number",
            "minimum": 0.67,
            "maximum": 2.0,
            "description": "Zoom of every window as a factor (1.0 = 100 %), rounded to the nearest step: 0.67, 0.75, \
                            0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0."
        })
    }
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
    let mut settings: Settings = serde_json::from_str(&text).unwrap_or_else(|err| {
        eprintln!("torroworkplace: ignoring {}, it is not valid settings JSON: {err}", path.display());
        Settings::default()
    });
    settings.fill_unnamed_parts(&Settings::default());
    settings
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
    /// Issue #243: the modifiers of the template keys default per system, read their names and
    /// fall back to the default when a file holds something else.
    #[test]
    fn the_template_keys_modifiers_are_read_leniently() {
        use super::{Settings, ShortcutModifiers};

        let mac_or_windows = cfg!(any(target_os = "macos", target_os = "windows"));
        let default = if mac_or_windows { ShortcutModifiers::CtrlAlt } else { ShortcutModifiers::SuperAlt };
        assert_eq!(Settings::default().workspaces.modifiers, default);
        let old: Settings = serde_json::from_str("{}").expect("old file");
        assert_eq!(old.workspaces.modifiers, default, "a file from before issue #243");
        let set: Settings =
            serde_json::from_value(serde_json::json!({"workspaces": {"modifiers": "ctrl_shift"}})).expect("chosen");
        assert_eq!(set.workspaces.modifiers, ShortcutModifiers::CtrlShift);
        let odd: Settings =
            serde_json::from_value(serde_json::json!({"workspaces": {"modifiers": "hyper"}, "zoom": 1.1}))
                .expect("odd value");
        assert_eq!(odd.workspaces.modifiers, default);
        assert_eq!(serde_json::to_value(ShortcutModifiers::SuperCtrl).expect("json"), "super_ctrl");
    }

    /// Task E12.5: the assignments are read leniently – an entry this version cannot use is
    /// dropped, the rest of the settings survive.
    #[test]
    fn assignments_are_read_leniently() {
        use crate::ai::{AiAssignment, AiFunction};

        let file = serde_json::json!({
            "aiAssignments": {
                "inbox.categorize": {
                    "function": "inbox.categorize",
                    "engine": "ai:ollama@localhost:11434",
                    "model": "nimble",
                    "enabled": true
                },
                "inbox.someday": {"function": "inbox.someday", "engine": "ai:x", "enabled": true},
                "inbox.categorize.broken": 42
            },
            "language": "german"
        });
        let settings: super::Settings = serde_json::from_value(file).expect("the file is read");
        assert_eq!(settings.language, super::Language::German, "the rest of the file survives");
        assert_eq!(settings.ai_assignments.len(), 1, "only the function this version knows");
        let kept = settings.ai_assignments.get(&AiFunction::InboxCategorize).expect("the assignment");
        assert_eq!(kept.engine.as_deref(), Some("ai:ollama@localhost:11434"));
        assert_eq!(kept.model.as_deref(), Some("nimble"));
        assert!(kept.is_ready());
        assert!(!AiAssignment::none(AiFunction::InboxCategorize).is_ready());

        // Nothing stored: no field on the wire at all.
        let empty = super::Settings::default();
        let json = serde_json::to_value(&empty).expect("json");
        assert!(json.get("aiAssignments").is_none(), "an empty map is left out");
    }

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

    /// Task C.1: 30 days unless changed; days or "forever" from outside, anything else refused
    /// there and read as "forever" from a file, so a damaged file deletes nothing.
    #[test]
    fn the_assistant_keeps_conversations_30_days_or_as_set() {
        assert_eq!(Settings::default().assistant_retention, Retention::Days(30));
        assert_eq!(Retention::from_value(&serde_json::json!(7)), Ok(Retention::Days(7)));
        assert_eq!(Retention::from_value(&serde_json::json!("forever")), Ok(Retention::Forever));
        for refused in
            [serde_json::json!(0), serde_json::json!(3651), serde_json::json!(1.5), serde_json::json!("never")]
        {
            assert!(Retention::from_value(&refused).is_err(), "{refused}");
        }
        assert_eq!(Retention::Forever.days(), None);
        assert_eq!(Retention::Days(7).days(), Some(7));
        for retention in [Retention::Days(90), Retention::Forever] {
            let written = serde_json::to_value(retention).expect("serializes");
            assert_eq!(written, retention.to_value());
            assert_eq!(serde_json::from_value::<Retention>(written).expect("reads"), retention);
        }
        let old: Settings = serde_json::from_str("{}").expect("a file from before the assistant");
        assert_eq!(old.assistant_retention, Retention::DEFAULT);
        let edited: Settings = serde_json::from_str(r#"{"assistantRetention": -3}"#).expect("lenient");
        assert_eq!(edited.assistant_retention, Retention::Forever);
    }

    /// Issue #131: the assistant's default is none unless set, checked from outside and read
    /// leniently from a file.
    #[test]
    fn the_assistants_default_is_checked_and_read_leniently() {
        use serde_json::json;
        assert_eq!(Settings::default().assistant_default, None);
        let set = AssistantDefault::from_value(&json!({"engine": "claude-code", "model": "opus", "effort": "high"}));
        let expected = AssistantDefault {
            engine: "claude-code".into(),
            model: Some("opus".into()),
            effort: Some(crate::ai::Effort::High),
        };
        assert_eq!(set, Ok(Some(expected.clone())));
        assert_eq!(AssistantDefault::to_value(Some(&expected))["model"], "opus");
        assert_eq!(AssistantDefault::to_value(None), json!(null));
        assert_eq!(AssistantDefault::from_value(&json!(null)), Ok(None));
        for refused in [
            json!({"engine": ""}),
            json!({"engine": "a\nb"}),
            json!({"engine": "x", "model": "a".repeat(301)}),
            json!({"engine": "x", "effort": "max"}),
            json!("claude-code"),
        ] {
            assert!(AssistantDefault::from_value(&refused).is_err(), "{refused}");
        }
        let settings = Settings { assistant_default: Some(expected.clone()), ..Settings::default() };
        let written = serde_json::to_value(&settings).expect("json");
        assert_eq!(written["assistantDefault"]["effort"], "high");
        let read: Settings = serde_json::from_value(written).expect("reads");
        assert_eq!(read.assistant_default, Some(expected));
        let damaged: Settings = serde_json::from_str(r#"{"assistantDefault": 7, "zoom": 1.1}"#).expect("lenient");
        assert_eq!(damaged.assistant_default, None);
        assert!(serde_json::to_value(Settings::default()).expect("json").get("assistantDefault").is_none());
    }

    /// Task C.9: voice is off unless switched on, answers are spoken unless turned off, and the
    /// values from outside are checked.
    #[test]
    fn voice_is_off_and_checked_from_outside() {
        use serde_json::json;
        let old: Settings = serde_json::from_str("{}").expect("a file from before voice");
        assert_eq!(old.voice, Voice::default());
        assert!(!old.voice.enabled && old.voice.speak_answers);
        assert_eq!(old.voice.wake_word, WAKE_WORD);
        let partial: Settings = serde_json::from_str(r#"{"voice": {"enabled": true}}"#).expect("reads");
        assert!(partial.voice.enabled && partial.voice.speak_answers);

        assert_eq!(Voice::wake_word_from(&json!("Hey  Torro")), Ok(WAKE_WORD.to_string()));
        assert!(Voice::wake_word_from(&json!("hey jarvis")).is_err(), "fixed for now");
        assert!(Voice::wake_word_from(&json!(true)).is_err());

        assert_eq!(Voice::microphone_from(&json!(null)), Ok(None));
        assert_eq!(Voice::microphone_from(&json!("default")), Ok(None));
        assert_eq!(Voice::microphone_from(&json!(" USB mic ")), Ok(Some("USB mic".into())));
        for refused in [json!("-D hw:0"), json!("two\nlines"), json!(7), json!("m".repeat(201))] {
            assert!(Voice::microphone_from(&refused).is_err(), "{refused}");
        }

        let model = if cfg!(windows) { r"C:\models\ggml-base.bin" } else { "/models/ggml-base.bin" };
        assert_eq!(Voice::file_from(&json!(model), "bin"), Ok(Some(model.into())));
        assert_eq!(Voice::file_from(&json!(""), "bin"), Ok(None));
        for refused in [json!("ggml-base.bin"), json!("/models/ggml-base.onnx"), json!(1)] {
            assert!(Voice::file_from(&refused, "bin").is_err(), "{refused}");
        }
    }

    /// Task M9.5: mail shows as HTML in the suite's colours unless set otherwise; the values are
    /// lowercase words, and a file from before them, or with a value that does not read, keeps
    /// the defaults and the other settings.
    #[test]
    fn mail_shows_as_html_in_the_apps_colours_unless_set() {
        let defaults = Settings::default();
        assert_eq!((defaults.mail_view, defaults.mail_appearance), (MailFormat::Html, MailAppearance::App));
        let written = serde_json::to_value(&defaults).expect("json");
        assert_eq!(
            (&written["mailView"], &written["mailAppearance"]),
            (&serde_json::json!("html"), &serde_json::json!("app"))
        );
        for (format, word) in [(MailFormat::Html, "html"), (MailFormat::Text, "text")] {
            assert_eq!(serde_json::to_value(format).expect("json"), serde_json::json!(word));
        }
        for (appearance, word) in
            [(MailAppearance::App, "app"), (MailAppearance::Light, "light"), (MailAppearance::Dark, "dark")]
        {
            assert_eq!(serde_json::to_value(appearance).expect("json"), serde_json::json!(word));
        }
        let set: Settings = serde_json::from_str(r#"{"mailView": "text", "mailAppearance": "dark"}"#).expect("reads");
        assert_eq!((set.mail_view, set.mail_appearance), (MailFormat::Text, MailAppearance::Dark));
        let old: Settings = serde_json::from_str(r#"{"language": "german"}"#).expect("a file from before");
        assert_eq!((old.mail_view, old.mail_appearance), (MailFormat::Html, MailAppearance::App));
        let damaged: Settings =
            serde_json::from_str(r#"{"mailView": "rich", "mailAppearance": 3, "zoom": 1.1}"#).expect("lenient");
        assert_eq!((damaged.mail_view, damaged.mail_appearance), (MailFormat::Html, MailAppearance::App));
        assert_eq!(damaged.zoom.percent(), 110, "the other settings still read");
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
        fs::write(
            &path,
            r#"{"language":"german","appearance":"dark","startAtLogin":true,"hiddenCollections":["work"]}"#,
        )
        .expect("write");
        let expected = Settings {
            language: Language::German,
            appearance: Appearance::Dark,
            start_at_login: true,
            hidden_collections: vec!["work".into()],
            zoom: Zoom::DEFAULT,
            night: Night::DEFAULT,
            list_orders: BTreeMap::new(),
            quiet_when_busy: true,
            design: "torro".into(),
            font: "torro".into(),
            color_scheme: "torro".into(),
            assistant_retention: Retention::DEFAULT,
            voice: Voice::default(),
            assistant_default: None,
            setup: Setup::default(),
            components_off: BTreeSet::new(),
            mail_view: MailFormat::Html,
            mail_appearance: MailAppearance::App,
            mail_to_cloud: false,
            reminders: Reminders::default(),
            ai_assignments: BTreeMap::new(),
            hidden_categories: Vec::new(),
            allow_api_keys: true,
            allow_mcp: true,
            tools: Tools::default(),
            workspaces: WorkspaceSettings::default(),
        };
        assert_eq!(load_from(&path), expected);
    }

    /// Task C.17: mail stays with local models unless a person switched it on.
    #[test]
    fn mail_stays_local_unless_switched_on() {
        let dir = TempDir::new("mail-to-cloud");
        let path = dir.file("settings.json");
        assert!(!Settings::default().mail_to_cloud);
        fs::write(&path, r#"{"language":"german"}"#).expect("write");
        assert!(!load_from(&path).mail_to_cloud, "a file from before the setting keeps mail local");
        fs::write(&path, r#"{"mailToCloud":true}"#).expect("write");
        assert!(load_from(&path).mail_to_cloud);
    }

    /// #231: API keys and MCP work unless a person turned them off, also with a file from before.
    #[test]
    fn api_keys_and_mcp_are_allowed_unless_turned_off() {
        let dir = TempDir::new("doors");
        let path = dir.file("settings.json");
        assert!(Settings::default().allow_api_keys && Settings::default().allow_mcp);
        fs::write(&path, r#"{"language":"german"}"#).expect("write");
        let before = load_from(&path);
        assert!(before.allow_api_keys && before.allow_mcp, "a file from before the switches keeps both ways open");
        fs::write(&path, r#"{"allowApiKeys":false,"allowMcp":false}"#).expect("write");
        let off = load_from(&path);
        assert!(!off.allow_api_keys && !off.allow_mcp);
    }

    #[test]
    fn quiet_when_busy_is_on_unless_turned_off() {
        let dir = TempDir::new("quiet");
        let path = dir.file("settings.json");
        assert!(Settings::default().quiet_when_busy);
        fs::write(&path, r#"{"language":"german"}"#).expect("write");
        assert!(load_from(&path).quiet_when_busy, "a file from before the setting keeps quiet on");
        fs::write(&path, r#"{"quietWhenBusy":false}"#).expect("write");
        assert!(!load_from(&path).quiet_when_busy);
    }

    #[test]
    fn components_are_on_unless_turned_off() {
        let dir = TempDir::new("components");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"language":"german"}"#).expect("write");
        let before = load_from(&path);
        assert!(Component::ALL.iter().all(|c| before.component_on(*c)), "a file from before has all on");

        let mut settings = Settings::default();
        settings.set_component(Component::Mail, false);
        settings.set_component(Component::Mail, false);
        assert!(!settings.component_on(Component::Mail) && settings.component_on(Component::Tasks));
        let json = serde_json::to_value(&settings).expect("json");
        assert_eq!(json["componentsOff"], serde_json::json!(["mail"]), "only the ones off, once");
        settings.set_component(Component::Mail, true);
        assert!(serde_json::to_value(&settings).expect("json").get("componentsOff").is_none());
    }

    #[test]
    fn unknown_components_and_a_damaged_setup_do_not_cost_the_file() {
        let dir = TempDir::new("setup-lenient");
        let path = dir.file("settings.json");
        let written = r#"{"language":"german","componentsOff":["mail","whiteboard",3],"setup":{"state":"later"}}"#;
        fs::write(&path, written).expect("write");
        let settings = load_from(&path);
        assert_eq!(settings.language, Language::German, "the rest of the file is kept");
        assert_eq!(settings.components_off, BTreeSet::from([Component::Mail]));
        assert_eq!(settings.setup, Setup::default());
    }

    #[test]
    fn setup_records_the_version_it_was_finished_or_put_off_with() {
        let pending = Setup::default();
        assert_eq!((pending.state, pending.version), (SetupState::Pending, 0));
        assert!(!pending.has_new_steps(), "a setup never run is new as a whole");
        let done = pending.moved_to(SetupState::Done);
        assert_eq!(done, Setup { state: SetupState::Done, version: Setup::VERSION });
        assert!(!done.has_new_steps());
        let again = done.moved_to(SetupState::Pending);
        assert_eq!(again.version, Setup::VERSION, "running it again keeps the version of the last run");
        let old = Setup { state: SetupState::Skipped, version: 0 };
        assert!(old.has_new_steps());
        assert_eq!(old.moved_to(SetupState::Skipped).version, Setup::VERSION);
    }

    #[test]
    fn the_design_is_the_standard_unless_one_is_chosen() {
        let dir = TempDir::new("design");
        let path = dir.file("settings.json");
        assert_eq!(Settings::default().design, crate::design::STANDARD_ID);
        fs::write(&path, r#"{"language":"german"}"#).expect("write");
        assert_eq!(load_from(&path).design, "torro", "a file from before design packs");
        fs::write(&path, r#"{"design":"plex"}"#).expect("write");
        assert_eq!(load_from(&path).design, "plex", "kept as written, whether the pack exists or not");
    }

    /// Task L.22: font and colour scheme are settings of their own, the standard unless chosen, never
    /// taken over from the design; settings sent without them keep the stored ones.
    #[test]
    fn font_and_colour_scheme_are_the_standard_unless_chosen() {
        let dir = TempDir::new("parts");
        let path = dir.file("settings.json");
        let standard = Settings::default();
        assert_eq!((standard.font.as_str(), standard.color_scheme.as_str()), ("torro", "torro"));
        fs::write(&path, r#"{"design":"plex"}"#).expect("write");
        let read = load_from(&path);
        assert_eq!((read.font.as_str(), read.color_scheme.as_str()), ("torro", "torro"), "missing fields");
        fs::write(&path, r#"{"design":"plex","font":"channelr","colorScheme":"nord"}"#).expect("write");
        let read = load_from(&path);
        assert_eq!(
            (read.design.as_str(), read.font.as_str(), read.color_scheme.as_str()),
            ("plex", "channelr", "nord")
        );
        let json = serde_json::to_value(&read).expect("json");
        assert_eq!((json["font"].as_str(), json["colorScheme"].as_str()), (Some("channelr"), Some("nord")));

        let mut sent: Settings = serde_json::from_str(r#"{"language":"german"}"#).expect("a client's settings");
        assert!(sent.font.is_empty() && sent.color_scheme.is_empty(), "left out: not named yet");
        sent.fill_unnamed_parts(&read);
        assert_eq!((sent.font.as_str(), sent.color_scheme.as_str()), ("channelr", "nord"), "the stored ones stay");
        let mut chosen = Settings { font: "torro".into(), ..read.clone() };
        chosen.fill_unnamed_parts(&read);
        assert_eq!(chosen.font, "torro", "a font that is named is kept");
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
    fn a_file_from_before_appearance_loads_as_system() {
        let dir = TempDir::new("no-appearance");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"language":"german","startAtLogin":true,"hiddenCollections":[]}"#).expect("write");
        let loaded = load_from(&path);
        assert_eq!(loaded.appearance, Appearance::System);
        assert_eq!(loaded.language, Language::German);
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
            appearance: Appearance::Light,
            start_at_login: true,
            hidden_collections: vec!["a".into(), "b".into()],
            zoom: Zoom::nearest(1.25),
            night: Night { from: q(23, 30), to: q(7, 0) },
            list_orders: BTreeMap::from([("work".to_string(), ListOrder::Manual)]),
            quiet_when_busy: false,
            design: "plex".into(),
            font: "channelr".into(),
            color_scheme: "nord".into(),
            assistant_retention: Retention::Days(90),
            voice: Voice {
                enabled: true,
                microphone: Some("alsa_input.usb-Rode".into()),
                speak_answers: false,
                stt_model: Some("/models/ggml-base.bin".into()),
                voice: Some("/voices/de_DE-thorsten-medium.onnx".into()),
                ..Voice::default()
            },
            assistant_default: None,
            setup: Setup { state: SetupState::Skipped, version: 1 },
            components_off: BTreeSet::from([Component::Mail, Component::Assistant]),
            mail_view: MailFormat::Text,
            mail_appearance: MailAppearance::Dark,
            mail_to_cloud: true,
            reminders: Reminders {
                default_lead: None,
                calendars: BTreeMap::from([("work".to_string(), CalendarReminders::FullScreen)]),
                sound: ReminderSound::None,
                ..Reminders::default()
            },
            ai_assignments: BTreeMap::new(),
            hidden_categories: Vec::new(),
            allow_api_keys: false,
            allow_mcp: false,
            tools: Tools {
                enabled: BTreeSet::from([ToolId::Pdf]),
                pdf: Box::new(PdfSettings { folder: Some("/Users/anna/PDFs".into()), ..PdfSettings::default() }),
            },
            workspaces: WorkspaceSettings { modifiers: ShortcutModifiers::CtrlShift },
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

    #[test]
    fn a_file_from_before_zoom_loads_at_100_percent() {
        let dir = TempDir::new("no-zoom");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"language":"german","appearance":"dark","startAtLogin":true}"#).expect("write");
        let loaded = load_from(&path);
        assert_eq!(loaded.zoom, Zoom::DEFAULT);
        assert_eq!(loaded.appearance, Appearance::Dark, "the other fields still load");
    }

    #[test]
    fn zoom_is_stored_as_its_factor() {
        for (percent, json) in [(67, "0.67"), (100, "1.0"), (110, "1.1"), (125, "1.25"), (200, "2.0")] {
            let zoom = Zoom::nearest(f64::from(percent) / 100.0);
            assert_eq!(zoom.percent(), percent);
            assert_eq!(serde_json::to_string(&zoom).expect("serialize"), json);
            assert_eq!(serde_json::from_str::<Zoom>(json).expect("parse"), zoom);
        }
        // Integers are numbers too.
        assert_eq!(serde_json::from_str::<Zoom>("2").expect("parse"), Zoom::MAX);
    }

    #[test]
    fn zoom_steps_go_up_and_down_and_stop_at_the_ends() {
        let mut zoom = Zoom::MIN;
        let mut seen = vec![zoom.percent()];
        while zoom != Zoom::MAX {
            zoom = zoom.larger();
            seen.push(zoom.percent());
        }
        assert_eq!(seen, Zoom::STEPS);
        assert_eq!(Zoom::MAX.larger(), Zoom::MAX);
        assert_eq!(Zoom::MIN.smaller(), Zoom::MIN);
        assert_eq!(Zoom::DEFAULT.larger().percent(), 110);
        assert_eq!(Zoom::DEFAULT.smaller().percent(), 90);
        assert_eq!(Zoom::nearest(1.5).smaller().percent(), 125);
        assert_eq!(Zoom::DEFAULT.factor(), 1.0);
        assert_eq!(Zoom::nearest(1.5).factor(), 1.5);
    }

    #[test]
    fn zoom_rounds_to_the_nearest_step() {
        let percent = |factor: f64| Zoom::nearest(factor).percent();
        assert_eq!(percent(1.0), 100);
        assert_eq!(percent(1.04), 100);
        assert_eq!(percent(1.06), 110);
        assert_eq!(percent(1.3), 125);
        assert_eq!(percent(0.7), 67);
        assert_eq!(percent(0.72), 75);
        // Halfway between 100 and 110: the smaller step.
        assert_eq!(percent(1.05), 100);
        // Out of range or not a number: clamped, or the default.
        assert_eq!(percent(0.1), 67);
        assert_eq!(percent(9.0), 200);
        assert_eq!(percent(f64::NAN), 100);
        assert_eq!(percent(f64::INFINITY), 100);
    }

    #[test]
    fn zoom_from_outside_must_be_in_range() {
        assert_eq!(Zoom::from_factor(1.1), Some(Zoom::nearest(1.1)));
        assert_eq!(Zoom::from_factor(1.3).map(Zoom::percent), Some(125));
        assert_eq!(Zoom::from_factor(0.67), Some(Zoom::MIN));
        assert_eq!(Zoom::from_factor(0.666), Some(Zoom::MIN), "rounds to 67 %");
        assert_eq!(Zoom::from_factor(2.0), Some(Zoom::MAX));
        for bad in [0.5, 0.66, 2.01, 3.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(Zoom::from_factor(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_zoom_in_a_file_is_rounded_to_a_step() {
        let dir = TempDir::new("zoom-rounded");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"zoom":1.3}"#).expect("write");
        assert_eq!(load_from(&path).zoom.percent(), 125);
        fs::write(&path, r#"{"zoom":7}"#).expect("write");
        assert_eq!(load_from(&path).zoom, Zoom::MAX, "out of range is clamped, not a broken file");
    }

    fn q(hour: u32, minute: u32) -> Quarter {
        Quarter::new(hour, minute).expect("a quarter hour")
    }

    #[test]
    fn the_night_is_ten_to_six_by_default_and_from_before_k27() {
        assert_eq!(Night::default(), Night { from: q(22, 0), to: q(6, 0) });
        assert_eq!(Night::DEFAULT.minutes(), 8 * 60, "over midnight is the normal case");
        let dir = TempDir::new("no-night");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"language":"german","zoom":1.1}"#).expect("write");
        let loaded = load_from(&path);
        assert_eq!(loaded.night, Night::DEFAULT);
        assert_eq!(loaded.zoom.percent(), 110, "the other fields still load");
    }

    #[test]
    fn the_night_is_stored_as_two_times() {
        let night = Night { from: q(23, 30), to: q(7, 0) };
        assert_eq!(
            serde_json::to_value(night).expect("serialize"),
            serde_json::json!({"from": "23:30", "to": "07:00"})
        );
        assert_eq!(serde_json::from_str::<Night>(r#"{"from":"23:30","to":"07:00"}"#).expect("parse"), night);
        assert_eq!(night.minutes(), 7 * 60 + 30);
        let day_sleeper = Night { from: q(1, 0), to: q(9, 15) };
        assert_eq!(day_sleeper.minutes(), 8 * 60 + 15, "a night after midnight only");
    }

    #[test]
    fn the_same_time_twice_is_no_night() {
        let none = Night::none_at(q(22, 0));
        assert!(none.is_none());
        assert_eq!(none.minutes(), 0);
        assert!(!Night::DEFAULT.is_none());
        assert_eq!(
            Night::from_value(&serde_json::json!({"from": "06:00", "to": "06:00"})).map(Night::is_none),
            Ok(true)
        );
    }

    #[test]
    fn a_night_from_outside_must_be_two_quarter_hours() {
        let night = |value: serde_json::Value| Night::from_value(&value);
        assert_eq!(night(serde_json::json!({"from": "22:00", "to": "06:00"})), Ok(Night::DEFAULT));
        for bad in [
            serde_json::json!({"from": "22:10", "to": "06:00"}),
            serde_json::json!({"from": "24:00", "to": "06:00"}),
            serde_json::json!({"from": "7:00", "to": "06:00"}),
            serde_json::json!({"from": "22:00"}),
            serde_json::json!({"from": "22:00", "to": "06:00", "extra": 1}),
            serde_json::json!({"from": 22, "to": 6}),
            serde_json::json!("22:00-06:00"),
            serde_json::json!(null),
        ] {
            let refused = night(bad.clone());
            assert!(refused.as_ref().is_err_and(|reason| !reason.is_empty()), "{bad}: {refused:?}");
        }
        let reason = night(serde_json::json!({"from": "22:10", "to": "06:00"})).expect_err("refused");
        assert!(reason.contains("quarter hour"), "the reason says what is wrong: {reason}");
    }

    #[test]
    fn a_broken_night_in_a_file_is_the_default() {
        let dir = TempDir::new("night-broken");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"night":{"from":"25:00","to":"x"},"startAtLogin":true}"#).expect("write");
        let loaded = load_from(&path);
        assert_eq!(loaded.night, Night::DEFAULT);
        assert!(loaded.start_at_login, "not a broken file");
    }

    #[test]
    fn quarters_cover_the_day_and_print_as_hh_mm() {
        let all: Vec<Quarter> = Quarter::all().collect();
        assert_eq!(all.len(), 96);
        assert_eq!(all[0].to_string(), "00:00");
        assert_eq!(all[95].to_string(), "23:45");
        assert_eq!(Quarter::parse("06:45"), Some(q(6, 45)));
        assert_eq!(Quarter::new(6, 50), None);
    }

    #[test]
    fn a_list_is_ordered_by_due_date_unless_set_to_manual() {
        let mut settings = Settings::default();
        assert_eq!(settings.list_order("work"), ListOrder::DueDate);
        settings.set_list_order("work", ListOrder::Manual);
        assert_eq!(settings.list_order("work"), ListOrder::Manual);
        assert_eq!(settings.list_order("home"), ListOrder::DueDate);
        let json = serde_json::to_value(&settings).expect("serialize");
        assert_eq!(json["listOrders"], serde_json::json!({"work": "manual"}));
        settings.set_list_order("work", ListOrder::DueDate);
        assert!(settings.list_orders.is_empty(), "the default is not stored");
        let json = serde_json::to_string(&settings).expect("serialize");
        assert!(!json.contains("listOrders"), "{json}");
    }

    #[test]
    fn a_list_order_entry_is_read_strictly() {
        let entry: ListOrderEntry =
            serde_json::from_value(serde_json::json!({"list": "work", "order": "manual"})).expect("valid");
        assert_eq!(entry, ListOrderEntry { list: "work".into(), order: ListOrder::Manual });
        for bad in [
            serde_json::json!({"list": "work"}),
            serde_json::json!({"list": "work", "order": "alphabetical"}),
            serde_json::json!({"list": "work", "order": "manual", "extra": 1}),
            serde_json::json!("manual"),
        ] {
            assert!(serde_json::from_value::<ListOrderEntry>(bad.clone()).is_err(), "{bad}");
        }
    }

    // Reminders (task ER1.2).

    #[test]
    fn reminders_come_one_minute_before_as_notifications_by_default() {
        let reminders = Settings::default().reminders;
        assert!(reminders.enabled && reminders.only_accepted && reminders.never_during_call && reminders.all_displays);
        assert!(!reminders.all_day && !reminders.free_events);
        assert_eq!(reminders.default_lead, Some(1));
        assert_eq!(reminders.calendar("any"), CalendarReminders::Notification);
        assert_eq!(reminders.sound, ReminderSound::System);
        let dir = TempDir::new("no-reminders");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"language":"german"}"#).expect("write");
        assert_eq!(load_from(&path).reminders, Reminders::default(), "settings from before ER1.2");
    }

    #[test]
    fn reminders_are_set_as_one_object_with_defaults_for_the_rest() {
        let value = serde_json::json!({
            "defaultLead": null,
            "calendars": {"a/work": "full_screen", "a/home": "notification", "a/birthdays": "off"},
            "allDay": true,
        });
        let reminders = Reminders::from_value(&value).expect("valid");
        assert_eq!(reminders.default_lead, None, "null is no default reminder");
        assert!(reminders.all_day && reminders.enabled, "set and left-out fields");
        assert_eq!(reminders.calendar("a/work"), CalendarReminders::FullScreen);
        assert_eq!(reminders.calendar("a/birthdays"), CalendarReminders::Off);
        assert!(!reminders.calendars.contains_key("a/home"), "the default is not stored");
        assert_eq!(Reminders::from_value(&reminders.to_value()), Ok(reminders.clone()));

        // A change touches only what it names, calendar by calendar.
        let changed = reminders
            .merged(&serde_json::json!({"freeEvents": true, "calendars": {"a/home": "full_screen", "a/work": "notification"}}))
            .expect("valid change");
        assert!(changed.free_events && changed.all_day, "the new field and the one set before");
        assert_eq!(changed.default_lead, None, "left out, so as it was");
        assert_eq!(changed.calendar("a/home"), CalendarReminders::FullScreen);
        assert_eq!(changed.calendar("a/birthdays"), CalendarReminders::Off, "not named, so as it was");
        assert!(!changed.calendars.contains_key("a/work"), "back to the default, so not stored");
        assert!(reminders.merged(&serde_json::json!({"defaultLead": "soon"})).is_err());
        assert!(reminders.merged(&serde_json::json!([1])).is_err(), "an object");
    }

    #[test]
    fn reminders_that_do_not_fit_are_refused_and_read_as_defaults() {
        for wrong in [
            serde_json::json!({"defaultLead": Reminders::MAX_LEAD + 1}),
            serde_json::json!({"calendars": {"a/work": "loud"}}),
            serde_json::json!({"enabled": "yes"}),
            serde_json::json!("on"),
        ] {
            assert!(Reminders::from_value(&wrong).is_err(), "{wrong}");
        }
        let dir = TempDir::new("bad-reminders");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"language":"german","reminders":{"defaultLead":"soon"}}"#).expect("write");
        let loaded = load_from(&path);
        assert_eq!(loaded.language, Language::German, "the other settings stay");
        assert_eq!(loaded.reminders, Reminders::default());
    }

    /// An absolute path on this system, for the tests of folders and logos.
    fn absolute(path: &str) -> String {
        match cfg!(windows) {
            true => format!(r"C:\{}", path.trim_start_matches('/').replace('/', r"\")),
            false => path.to_string(),
        }
    }

    /// Issue #225: every tool is off, the PDF folder unset and the three templates as planned
    /// until a person changes them; a file from before the tools reads so too.
    #[test]
    fn tools_start_off_with_the_three_standard_templates() {
        let tools = Settings::default().tools;
        assert!(tools.enabled.is_empty() && !tools.is_on(ToolId::Pdf));
        assert_eq!(tools.pdf.folder, None);
        let letter = tools.pdf.template(PdfTemplateId::Letter);
        assert_eq!((letter.paper, letter.margins_mm, letter.font_size_pt), (Paper::A4, [25; 4], 11));
        assert!(letter.header.is_empty() && !letter.page_numbers && letter.logo.is_none());
        let report = tools.pdf.template(PdfTemplateId::Report);
        assert_eq!((report.paper, report.margins_mm, report.font_size_pt), (Paper::A4, [20; 4], 11));
        assert_eq!((report.header.as_str(), report.page_numbers), ("{title}", true));
        let note = tools.pdf.template(PdfTemplateId::Note);
        assert_eq!((note.paper, note.margins_mm, note.font_size_pt), (Paper::A4, [15; 4], 10));
        assert!(note.header.is_empty() && note.footer.is_empty() && !note.page_numbers);
        assert_eq!(PdfTemplateId::default(), PdfTemplateId::Report, "the template without a name");
        for id in PdfTemplateId::ALL {
            assert_eq!(tools.pdf.template(id).check(), Ok(()), "{id:?}");
        }
        let old: Settings = serde_json::from_str("{}").expect("a file from before the tools");
        assert_eq!(old.tools, Tools::default());
        assert_eq!(serde_json::to_value(ToolId::Pdf).expect("json"), "pdf");
        assert_eq!(ToolId::Pdf.name(), "pdf");
        assert_eq!(serde_json::to_value(PdfTemplateId::Note).expect("json"), PdfTemplateId::Note.name());
    }

    /// The tool of a command: the three file commands are the PDF tool's, listing belongs to none.
    #[test]
    fn commands_belong_to_their_tool() {
        for name in ["tools.create_pdf", "tools.open_file", "tools.save_file_as"] {
            assert_eq!(ToolId::of_command(name), Some(ToolId::Pdf), "{name}");
        }
        for name in ["tools.list_tools", "settings.set_setting", "pdf", ""] {
            assert_eq!(ToolId::of_command(name), None, "{name}");
        }
    }

    /// A damaged tools entry costs the defaults (every tool off), never the other settings; a tool
    /// this version does not know is dropped.
    #[test]
    fn tools_are_read_leniently() {
        let dir = TempDir::new("tools");
        let path = dir.file("settings.json");
        fs::write(&path, r#"{"language":"german","tools":{"enabled":["pdf","whiteboard"]}}"#).expect("write");
        let loaded = load_from(&path);
        assert_eq!(loaded.tools.enabled, BTreeSet::from([ToolId::Pdf]), "the unknown one is dropped");
        assert_eq!(*loaded.tools.pdf, PdfSettings::default(), "left out, so the defaults");
        fs::write(&path, r#"{"language":"german","tools":{"enabled":["pdf"],"pdf":{"letter":7}}}"#).expect("write");
        let loaded = load_from(&path);
        assert_eq!(loaded.language, Language::German, "the other settings stay");
        assert_eq!(loaded.tools, Tools::default(), "a damaged entry is the defaults: every tool off");
        let mut tools = Tools::default();
        tools.set_on(ToolId::Pdf, true);
        tools.pdf.folder = Some(absolute("/docs"));
        let settings = Settings { tools: tools.clone(), ..Settings::default() };
        let written = serde_json::to_string(&settings).expect("json");
        assert_eq!(serde_json::from_str::<Settings>(&written).expect("reads back").tools, tools);
    }

    /// `pdf_template`: a whole template, checked against its limits; a wrong template, a relative
    /// or wrong logo is refused with the reason.
    #[test]
    fn a_pdf_template_is_read_and_checked() {
        let logo = absolute("/pictures/logo.PNG");
        let value = serde_json::json!({
            "template": "letter",
            "paper": "letter",
            "marginsMm": [5, 50, 20, 20],
            "fontSizePt": 16,
            "header": "{title} · {date}",
            "footer": "ACME",
            "pageNumbers": true,
            "logo": logo,
        });
        let (id, template) = PdfTemplate::from_value(&value).expect("a valid template");
        assert_eq!(id, PdfTemplateId::Letter);
        assert_eq!(
            template,
            PdfTemplate {
                paper: Paper::Letter,
                margins_mm: [5, 50, 20, 20],
                font_size_pt: 16,
                header: "{title} · {date}".into(),
                footer: "ACME".into(),
                page_numbers: true,
                logo: Some(logo.clone()),
            }
        );
        assert_eq!(PdfTemplate::from_value(&template.to_value(id)), Ok((id, template.clone())));
        let short = serde_json::json!({"template": "note", "paper": "a5", "marginsMm": [10, 10, 10, 10], "fontSizePt": 9, "logo": ""});
        let (id, note) = PdfTemplate::from_value(&short).expect("header, footer, numbers and logo may be left out");
        assert_eq!(
            (id, note.paper, note.header.as_str(), note.page_numbers, note.logo),
            (PdfTemplateId::Note, Paper::A5, "", false, None)
        );

        let with = |field: &str, wrong: serde_json::Value| {
            let mut value = value.clone();
            value[field] = wrong;
            PdfTemplate::from_value(&value)
        };
        for (field, wrong) in [
            ("template", serde_json::json!("invoice")),
            ("paper", serde_json::json!("a3")),
            ("marginsMm", serde_json::json!([4, 20, 20, 20])),
            ("marginsMm", serde_json::json!([20, 51, 20, 20])),
            ("marginsMm", serde_json::json!([20, 20, 20])),
            ("fontSizePt", serde_json::json!(7)),
            ("fontSizePt", serde_json::json!(17)),
            ("header", serde_json::json!("a".repeat(PdfTemplate::MAX_LINE_CHARS + 1))),
            ("footer", serde_json::json!("two\nlines")),
            ("logo", serde_json::json!("pictures/logo.png")),
            ("logo", serde_json::json!(absolute("/pictures/logo.gif"))),
        ] {
            assert!(with(field, wrong.clone()).is_err(), "{field}: {wrong}");
        }
        assert!(with("header", serde_json::json!("a".repeat(PdfTemplate::MAX_LINE_CHARS))).is_ok(), "200 fit");
        let mut missing = value.clone();
        missing.as_object_mut().expect("object").remove("template");
        assert!(PdfTemplate::from_value(&missing).is_err(), "the template is named");
        assert!(PdfTemplate::from_value(&serde_json::json!("report")).is_err(), "an object");
        let relative = with("logo", serde_json::json!("logo.png")).expect_err("relative");
        assert!(relative.contains("absolute path"), "{relative}");
    }

    /// `pdf_folder`: an absolute path, or null or "" for the desktop.
    #[test]
    fn a_pdf_folder_is_an_absolute_path_or_none() {
        let folder = absolute("/Users/anna/Documents/PDFs");
        assert_eq!(PdfSettings::folder_from(&serde_json::json!(folder)), Ok(Some(folder.clone())));
        assert_eq!(PdfSettings::folder_from(&serde_json::json!(null)), Ok(None));
        assert_eq!(PdfSettings::folder_from(&serde_json::json!(" ")), Ok(None));
        for wrong in
            [serde_json::json!("Documents"), serde_json::json!(7), serde_json::json!(format!("{folder}\nmore"))]
        {
            let refused = PdfSettings::folder_from(&wrong);
            assert!(refused.as_ref().is_err_and(|reason| reason.contains("absolute path")), "{wrong}: {refused:?}");
        }
        assert!(PdfSettings::folder_from(&serde_json::json!(format!("/{}", "a".repeat(MAX_PATH_BYTES)))).is_err());
    }

    /// Whole PDF settings are checked where they differ from the stored ones, nowhere else.
    #[test]
    fn only_the_changed_pdf_settings_are_checked() {
        let stored = PdfSettings::default();
        assert_eq!(stored.check_changes(&stored), Ok(()));
        let mut sent = stored.clone();
        sent.folder = Some("PDFs".into());
        assert!(sent.check_changes(&stored).is_err_and(|reason| reason.contains("absolute path")));
        sent.folder = Some(absolute("/Users/anna/PDFs"));
        sent.note.font_size_pt = 12;
        assert_eq!(sent.check_changes(&stored), Ok(()));
        sent.report.header = "a".repeat(PdfTemplate::MAX_LINE_CHARS + 1);
        assert!(sent.check_changes(&stored).is_err_and(|reason| reason.contains("\"report\"")));

        // Stored out of bounds (a file edited by hand): not checked again while it stays.
        let mut by_hand = stored.clone();
        (by_hand.folder, by_hand.letter.margins_mm) = (Some("PDFs".into()), [99; 4]);
        let mut sent = by_hand.clone();
        sent.note.font_size_pt = 12;
        assert_eq!(sent.check_changes(&by_hand), Ok(()));
    }

    /// Point 9: switching a tool on and setting a folder or a logo other than the stored ones is a
    /// person's; switching off, removing and every other field are not. A client's whole settings
    /// keep the switches it could not turn on and the stored paths.
    #[test]
    fn only_people_switch_tools_on_and_set_their_paths() {
        let stored = Tools::default();
        let mut on = stored.clone();
        on.set_on(ToolId::Pdf, true);
        assert!(stored.needs_a_person(&on) && !on.needs_a_person(&stored), "on, but off is anyone's");
        let mut folder = stored.clone();
        folder.pdf.folder = Some(absolute("/sync/Out"));
        assert!(stored.needs_a_person(&folder) && !folder.needs_a_person(&folder), "a new folder, not the same");
        assert!(!folder.needs_a_person(&stored), "removing it");
        let mut logo = stored.clone();
        logo.pdf.template_mut(PdfTemplateId::Note).logo = Some(absolute("/pictures/logo.png"));
        assert!(stored.needs_a_person(&logo) && !logo.needs_a_person(&stored));
        let mut fields = stored.clone();
        let report = fields.pdf.template_mut(PdfTemplateId::Report);
        (report.paper, report.font_size_pt, report.footer) = (Paper::A5, 9, "ACME".into());
        assert!(!stored.needs_a_person(&fields), "the other fields are anyone's with admin rights");

        // What a client's whole settings leave of it.
        let mut person = stored.clone();
        person.set_on(ToolId::Pdf, true);
        person.pdf.folder = Some(absolute("/Users/anna/PDFs"));
        person.pdf.template_mut(PdfTemplateId::Letter).logo = Some(absolute("/pictures/letter.png"));
        let mut sent = fields.clone();
        sent.set_on(ToolId::Pdf, true);
        sent.pdf.folder = Some(absolute("/sync/Out"));
        sent.pdf.template_mut(PdfTemplateId::Note).logo = Some(absolute("/pictures/other.png"));
        let kept = stored.from_client(&sent);
        assert!(!kept.is_on(ToolId::Pdf), "off stays off");
        assert_eq!((kept.pdf.folder.as_deref(), kept.pdf.template(PdfTemplateId::Note).logo.as_deref()), (None, None));
        assert_eq!(kept.pdf.template(PdfTemplateId::Report), fields.pdf.template(PdfTemplateId::Report), "the fields");
        let kept = person.from_client(&sent);
        assert_eq!(kept.pdf.folder, person.pdf.folder, "the folder stays, not the client's");
        assert_eq!(kept.pdf.letter.logo, None, "removed, as the client sent none");
        assert_eq!(kept.pdf.note.logo, None, "and no logo of the client's");
        let kept = person.from_client(&Tools::default());
        assert!(!kept.is_on(ToolId::Pdf), "a client may switch it off");
        assert_eq!((kept.pdf.folder, kept.pdf.letter.logo), (None, None), "and remove the paths");
        let kept = person.from_client(&person);
        assert!(kept.is_on(ToolId::Pdf), "on stays on");
        assert_eq!(kept, person, "the stored paths stay");
    }
}
