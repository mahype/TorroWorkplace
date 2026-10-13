//! Suite-wide settings, set in the control centre (`torroworkplace ui`) and read by every
//! part of the suite: `settings.json` in the core's config directory.
//!
//! Task S.1 in `docs/tasks/steuerzentrale.md` fills in [`load`] and [`save`]. The type is the
//! contract: add fields with `#[serde(default)]` so older files keep loading.

use std::borrow::Cow;
use std::collections::BTreeMap;
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
    /// How long the assistant keeps a conversation after its last entry (task C.1, decision 8 in
    /// `docs/assistent.md`): 30 days unless changed, or forever. Pinned conversations stay anyway.
    #[serde(default)]
    pub assistant_retention: Retention,
}

/// The id of the standard design, for settings from before design packs.
fn standard_design() -> String {
    crate::design::STANDARD_ID.to_string()
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
            assistant_retention: Retention::default(),
        }
    }
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

impl Settings {
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
            assistant_retention: Retention::DEFAULT,
        };
        assert_eq!(load_from(&path), expected);
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
    fn the_design_is_the_standard_unless_one_is_chosen() {
        let dir = TempDir::new("design");
        let path = dir.file("settings.json");
        assert_eq!(Settings::default().design, crate::design::STANDARD_ID);
        fs::write(&path, r#"{"language":"german"}"#).expect("write");
        assert_eq!(load_from(&path).design, "torro", "a file from before design packs");
        fs::write(&path, r#"{"design":"plex"}"#).expect("write");
        assert_eq!(load_from(&path).design, "plex", "kept as written, whether the pack exists or not");
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
            assistant_retention: Retention::Days(90),
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
}
