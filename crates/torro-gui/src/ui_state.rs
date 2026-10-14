//! What a window remembers about its own look between starts: whether the sidebar is collapsed
//! (task U.2, [`UiState`]), in the tasks app the repository last chosen for a GitHub filter list
//! (task G.11, [`last_repo_from`]), and which foldable parts the person unfolded – "Info" in the
//! columns of the inbox's overview (task E3.11, [`unfolded_from`]), and the density each window of the
//! inbox shows its entries in (task E8.2, [`Density`], [`density_from`]) and in which order by arrival
//! (task E8.15, [`order_from`]). `ui.json` in the app's state directory
//! ([`torro_core::paths`]), because it is view state that may be lost – like the scroll position it
//! is no suite setting and goes through no API.
//!
//! A missing or broken file gives the defaults, so it never keeps an app from starting. Writes are
//! atomic (temporary file, then rename) and keep the keys they do not write, so saving the sidebar
//! never forgets the repository and the other way round. Apps load it in the background at start
//! and save after a change:
//!
//! ```ignore
//! // new(): Task::batch([…, ui_state::load_task(AppId::TASKS).map(Message::UiState)])
//! // update(Message::UiState(state)) => self.sidebar = state.sidebar,
//! // update(Message::ToggleSidebar) =>
//! self.sidebar = self.sidebar.toggled();
//! return ui_state::save_task(AppId::TASKS, UiState { sidebar: self.sidebar });
//! ```

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use iced::Task;
use serde::{Deserialize, Serialize};
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_ipc::Order;

use crate::layout::SizeClass;
use crate::shell::SidebarMode;

/// The name of the file in the app's state directory.
pub const FILE: &str = "ui.json";

/// The key of the repository last chosen for a GitHub filter list (`owner/name`), beside the
/// fields of [`UiState`]. One value for all GitHub accounts of the app.
const LAST_REPO: &str = "lastRepo";

/// The key of the parts unfolded ([`unfolded_from`]): a list of names the app chooses.
const UNFOLDED: &str = "unfolded";

/// The key of the densities chosen ([`density_from`]): an object from a window's key to its
/// [`Density`].
const DENSITY: &str = "density";

/// The key of the orders chosen ([`order_from`]): an object from a window's key to its
/// [`Order`] word.
const ORDER: &str = "order";

/// How close together a window shows its entries (task E8.2, `docs/eingang.md` "Zwei Dichten"):
/// one row each, or cards with room, a longer preview and their buttons always shown. Chosen with
/// `widgets::density_switch` in the header bar, remembered per window ([`save_density_to`]); a
/// `--density compact|comfortable` at start wins over what is remembered, so a workspace template
/// brings its windows back as they were. Mini and strip are glances and know no density.
///
/// Stored as `"compact"` / `"comfortable"`, the words of `--density` ([`Density::ALL`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    /// One row per entry, the actions at the row under the pointer or the cursor: the look up to
    /// task E3.11, good for small tiles.
    Compact,
    /// Cards: more room, a preview over two or three lines, chips (attachment, "Answer by Fri") and
    /// every button shown at once.
    Comfortable,
}

impl Density {
    /// The words of `--density` and of `ui.json`, in the order the error message lists them.
    pub const ALL: [(&'static str, Density); 2] =
        [("compact", Density::Compact), ("comfortable", Density::Comfortable)];

    /// The density of the word `word` (`"compact"`); `None` for any other word.
    pub fn parse(word: &str) -> Option<Density> {
        Density::ALL.iter().find(|(name, _)| *name == word).map(|&(_, density)| density)
    }

    /// The word of this density, as `--density` and `ui.json` write it.
    pub fn word(self) -> &'static str {
        match self {
            Density::Compact => "compact",
            Density::Comfortable => "comfortable",
        }
    }

    /// The density of a window whose person chose none (no `--density`, nothing remembered), in
    /// the layout `class`: compact in the narrow one, where a card would leave room for two
    /// entries, comfortable elsewhere. The choice is not stored, so the window follows its size
    /// until the person picks one.
    pub fn default_for(class: SizeClass) -> Density {
        match class {
            SizeClass::Narrow => Density::Compact,
            _ => Density::Comfortable,
        }
    }
}

/// The remembered view state of one app. Add fields with `#[serde(default)]` so older files keep
/// loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    /// The sidebar the user chose for the wide layout (`"expanded"` or `"rail"`).
    #[serde(default)]
    pub sidebar: SidebarMode,
}

/// Why the state could not be saved.
#[derive(Debug)]
pub enum Error {
    /// No home directory, so no state directory.
    NoHome,
    /// The directory or the file could not be written.
    Io(PathBuf, io::Error),
    /// The state could not be turned into JSON.
    Json(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoHome => write!(f, "no home directory, so no state directory"),
            Error::Io(path, err) => write!(f, "cannot write {}: {err}", path.display()),
            Error::Json(err) => write!(f, "cannot encode the view state: {err}"),
        }
    }
}

impl std::error::Error for Error {}

/// `ui.json` in the state directory of `app` (`TORRO_STATE_HOME` overrides the root); `None`
/// without a home directory.
pub fn file(app: AppId) -> Option<PathBuf> {
    AppPaths::for_app(app).map(|paths| paths.state.join(FILE))
}

/// The stored state of `app`; the defaults when there is none or it cannot be read.
pub fn load(app: AppId) -> UiState {
    file(app).map_or_else(UiState::default, |path| load_from(&path))
}

/// Writes the state of `app` atomically, creating the state directory if needed.
pub fn save(app: AppId, state: &UiState) -> Result<(), Error> {
    save_to(&file(app).ok_or(Error::NoHome)?, state)
}

/// [`load`] in the background, for the start of an app: `ui_state::load_task(app).map(Message::UiState)`.
/// Tests that only build the app never run it, so they never read the real home directory.
pub fn load_task(app: AppId) -> Task<UiState> {
    Task::perform(async move { load(app) }, |state| state)
}

/// [`save`] in the background, after a change. A failure is reported on stderr and otherwise
/// ignored: the window keeps its state, only the next start will not know it.
pub fn save_task<T: Send + 'static>(app: AppId, state: UiState) -> Task<T> {
    Task::future(async move {
        if let Err(err) = save(app, &state) {
            eprintln!("{}: {err}", app.binary);
        }
    })
    .discard()
}

/// Reads `path`. A missing file is normal (first start); any other problem is reported once and
/// the defaults are used.
pub fn load_from(path: &Path) -> UiState {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            if err.kind() != io::ErrorKind::NotFound {
                eprintln!("cannot read {}: {err}", path.display());
            }
            return UiState::default();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|err| {
        eprintln!("ignoring {}, it is not valid view state: {err}", path.display());
        UiState::default()
    })
}

/// Writes `state` to `path` atomically, creating its directory if needed. Keys of the file that
/// are no field of [`UiState`] (the last repository, fields of a later version) stay.
pub fn save_to(path: &Path, state: &UiState) -> Result<(), Error> {
    match serde_json::to_value(state).map_err(Error::Json)? {
        serde_json::Value::Object(fields) => merge_into(path, fields),
        _ => Ok(()),
    }
}

/// The repository (`owner/name`) last chosen for a GitHub filter list, as [`save_last_repo_to`]
/// stored it in `path`; `None` when there is none or the file cannot be read.
pub fn last_repo_from(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let repo = value.get(LAST_REPO)?.as_str()?.trim();
    (!repo.is_empty()).then(|| repo.to_string())
}

/// Stores `repo` (`owner/name`) as the repository last chosen for a GitHub filter list in `path`,
/// keeping everything else in the file.
pub fn save_last_repo_to(path: &Path, repo: &str) -> Result<(), Error> {
    let mut fields = serde_json::Map::new();
    fields.insert(LAST_REPO.into(), serde_json::Value::String(repo.to_string()));
    merge_into(path, fields)
}

/// [`last_repo_from`] for `app` in the background, for the start of an app:
/// `ui_state::load_last_repo_task(app).map(Message::LastRepo)`. Like [`load_task`], tests that only
/// build the app never run it.
pub fn load_last_repo_task(app: AppId) -> Task<Option<String>> {
    Task::perform(async move { file(app).and_then(|path| last_repo_from(&path)) }, |repo| repo)
}

/// [`save_last_repo_to`] for `app` in the background, when a filter list was created or saved. A
/// failure is reported on stderr and otherwise ignored, as in [`save_task`].
pub fn save_last_repo_task<T: Send + 'static>(app: AppId, repo: String) -> Task<T> {
    Task::future(async move {
        let saved = file(app).ok_or(Error::NoHome).and_then(|path| save_last_repo_to(&path, &repo));
        if let Err(err) = saved {
            eprintln!("{}: {err}", app.binary);
        }
    })
    .discard()
}

/// The names of the foldable parts unfolded in `path`, as [`save_unfolded_to`] stored them (task
/// E3.11): names the app chooses, such as `overview.mail.info`. Empty when nothing is stored, the
/// file cannot be read or the value is no list of names (other entries of a list are skipped).
pub fn unfolded_from(path: &Path) -> Vec<String> {
    let Some(value) =
        fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return Vec::new();
    };
    let Some(names) = value.get(UNFOLDED).and_then(serde_json::Value::as_array) else { return Vec::new() };
    names.iter().filter_map(serde_json::Value::as_str).map(str::to_string).collect()
}

/// Stores `names` as the foldable parts unfolded in `path` – all of them, replacing the list
/// before –, keeping everything else in the file. Folding a part is leaving its name out.
pub fn save_unfolded_to(path: &Path, names: &[String]) -> Result<(), Error> {
    let mut fields = serde_json::Map::new();
    let list = names.iter().cloned().map(serde_json::Value::String).collect();
    fields.insert(UNFOLDED.into(), serde_json::Value::Array(list));
    merge_into(path, fields)
}

/// [`unfolded_from`] for `app` in the background, for the start of an app:
/// `ui_state::load_unfolded_task(app).map(Message::Unfolded)`. Like [`load_task`], tests that only
/// build the app never run it.
pub fn load_unfolded_task(app: AppId) -> Task<Vec<String>> {
    Task::perform(async move { file(app).map(|path| unfolded_from(&path)).unwrap_or_default() }, |names| names)
}

/// [`save_unfolded_to`] for `app` in the background, after the person folded or unfolded a part. A
/// failure is reported on stderr and otherwise ignored, as in [`save_task`].
pub fn save_unfolded_task<T: Send + 'static>(app: AppId, names: Vec<String>) -> Task<T> {
    Task::future(async move {
        let saved = file(app).ok_or(Error::NoHome).and_then(|path| save_unfolded_to(&path, &names));
        if let Err(err) = saved {
            eprintln!("{}: {err}", app.binary);
        }
    })
    .discard()
}

/// The density remembered for the window `key` in `path`, as [`save_density_to`] stored it; `None`
/// when there is none, the file cannot be read or the value is no density word.
///
/// `key` names the window, not the app: the three windows of the inbox share one state directory,
/// and one window per slice (`--lane info`, `--source agent:claude-code`) keeps a density of its
/// own. The app builds it from what tells its windows apart, e.g. `mail`, `hub/lane:info`.
pub fn density_from(path: &Path, key: &str) -> Option<Density> {
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Density::parse(value.get(DENSITY)?.get(key)?.as_str()?)
}

/// Stores `density` for the window `key` in `path`, keeping the densities of the other windows and
/// everything else in the file.
pub fn save_density_to(path: &Path, key: &str, density: Density) -> Result<(), Error> {
    let mut densities = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get(DENSITY).and_then(serde_json::Value::as_object).cloned())
        .unwrap_or_default();
    densities.insert(key.to_string(), serde_json::Value::String(density.word().to_string()));
    let mut fields = serde_json::Map::new();
    fields.insert(DENSITY.into(), serde_json::Value::Object(densities));
    merge_into(path, fields)
}

/// [`density_from`] for the window `key` of `app` in the background, for the start of a window
/// that got no `--density`: `ui_state::load_density_task(app, key).map(Message::Density)`. Like
/// [`load_task`], tests that only build the app never run it.
pub fn load_density_task(app: AppId, key: String) -> Task<Option<Density>> {
    Task::perform(async move { file(app).and_then(|path| density_from(&path, &key)) }, |density| density)
}

/// [`save_density_to`] for the window `key` of `app` in the background, after the person chose a
/// density. A failure is reported on stderr and otherwise ignored, as in [`save_task`].
pub fn save_density_task<T: Send + 'static>(app: AppId, key: String, density: Density) -> Task<T> {
    Task::future(async move {
        let saved = file(app).ok_or(Error::NoHome).and_then(|path| save_density_to(&path, &key, density));
        if let Err(err) = saved {
            eprintln!("{}: {err}", app.binary);
        }
    })
    .discard()
}

/// The order by arrival remembered for the window `key` in `path` (task E8.15), as
/// [`save_order_to`] stored it; `None` when there is none, the file cannot be read or the value is
/// no order word ([`Order::word`]). Windows are keyed as for [`density_from`].
pub fn order_from(path: &Path, key: &str) -> Option<Order> {
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Order::parse(value.get(ORDER)?.get(key)?.as_str()?)
}

/// Stores `order` for the window `key` in `path`, keeping the orders of the other windows and
/// everything else in the file.
pub fn save_order_to(path: &Path, key: &str, order: Order) -> Result<(), Error> {
    let mut orders = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get(ORDER).and_then(serde_json::Value::as_object).cloned())
        .unwrap_or_default();
    orders.insert(key.to_string(), serde_json::Value::String(order.word().to_string()));
    let mut fields = serde_json::Map::new();
    fields.insert(ORDER.into(), serde_json::Value::Object(orders));
    merge_into(path, fields)
}

/// [`order_from`] for the window `key` of `app` in the background, for the start of a window:
/// `ui_state::load_order_task(app, key).map(Message::StoredOrder)`. Like [`load_task`], tests that
/// only build the app never run it.
pub fn load_order_task(app: AppId, key: String) -> Task<Option<Order>> {
    Task::perform(async move { file(app).and_then(|path| order_from(&path, &key)) }, |order| order)
}

/// [`save_order_to`] for the window `key` of `app` in the background, after the person turned the
/// order. A failure is reported on stderr and otherwise ignored, as in [`save_task`].
pub fn save_order_task<T: Send + 'static>(app: AppId, key: String, order: Order) -> Task<T> {
    Task::future(async move {
        let saved = file(app).ok_or(Error::NoHome).and_then(|path| save_order_to(&path, &key, order));
        if let Err(err) = saved {
            eprintln!("{}: {err}", app.binary);
        }
    })
    .discard()
}

/// Writes `fields` into the JSON object in `path` – over the keys of the same name, beside the
/// others – atomically, creating the directory if needed. A missing or broken file starts empty.
fn merge_into(path: &Path, fields: serde_json::Map<String, serde_json::Value>) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|err| Error::Io(dir.to_path_buf(), err))?;
    }
    let mut object = match fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()) {
        Some(serde_json::Value::Object(object)) => object,
        _ => serde_json::Map::new(),
    };
    object.extend(fields);
    let mut json = serde_json::to_string_pretty(&serde_json::Value::Object(object)).map_err(Error::Json)?;
    json.push('\n');
    write_atomically(path, &json).map_err(|err| Error::Io(path.to_path_buf(), err))
}

/// Writes to a temporary file next to `path` and renames it over `path`, so a crash never leaves
/// half a file behind. The process id keeps two windows of the app from sharing a temporary file.
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
    use std::process::{Command, Stdio};

    /// A directory under the system temp dir, removed again on drop (torro-gui has no `tempfile`).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("torro-ui-state-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("create temp dir");
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_file_loads_expanded() {
        let dir = TempDir::new("missing");
        assert_eq!(load_from(&dir.0.join(FILE)), UiState { sidebar: SidebarMode::Expanded });
    }

    #[test]
    fn broken_or_unknown_content_loads_the_defaults() {
        let dir = TempDir::new("broken");
        let path = dir.0.join(FILE);
        for content in ["{ not json", r#"{"sidebar":"sideways"}"#, "[]"] {
            fs::write(&path, content).expect("write");
            assert_eq!(load_from(&path), UiState::default(), "{content}");
        }
    }

    #[test]
    fn older_and_newer_files_keep_loading() {
        let dir = TempDir::new("fields");
        let path = dir.0.join(FILE);
        fs::write(&path, "{}").expect("write");
        assert_eq!(load_from(&path), UiState::default(), "a file without the field");
        fs::write(&path, r#"{"sidebar":"rail","later":true}"#).expect("write");
        assert_eq!(load_from(&path).sidebar, SidebarMode::Rail, "a field from a later version is ignored");
    }

    #[test]
    fn saved_state_loads_again_and_creates_the_directory() {
        let dir = TempDir::new("roundtrip");
        let path = dir.0.join("torrotasks").join(FILE);
        let rail = UiState { sidebar: SidebarMode::Rail };
        save_to(&path, &rail).expect("save");
        assert_eq!(load_from(&path), rail);
        assert!(fs::read_to_string(&path).expect("read").contains(r#""sidebar": "rail""#));
        save_to(&path, &UiState::default()).expect("save again");
        assert_eq!(load_from(&path), UiState::default());
        let leftovers = fs::read_dir(path.parent().expect("dir")).expect("list").count();
        assert_eq!(leftovers, 1, "no temporary file stays behind");
    }

    #[test]
    fn the_last_repository_is_kept_beside_the_sidebar() {
        let dir = TempDir::new("last-repo");
        let path = dir.0.join(FILE);
        assert_eq!(last_repo_from(&path), None, "nothing chosen yet");
        save_to(&path, &UiState { sidebar: SidebarMode::Rail }).expect("save the sidebar");
        save_last_repo_to(&path, "mahype/TorroWorkplace").expect("save the repository");
        assert_eq!(last_repo_from(&path).as_deref(), Some("mahype/TorroWorkplace"));
        assert_eq!(load_from(&path).sidebar, SidebarMode::Rail, "the sidebar stays");
        save_to(&path, &UiState::default()).expect("save the sidebar again");
        assert_eq!(last_repo_from(&path).as_deref(), Some("mahype/TorroWorkplace"), "the repository stays");
        save_last_repo_to(&path, "mahype/notes").expect("another repository");
        assert_eq!(last_repo_from(&path).as_deref(), Some("mahype/notes"));
        for content in ["{ not json", r#"{"lastRepo":7}"#, r#"{"lastRepo":"  "}"#] {
            fs::write(&path, content).expect("write");
            assert_eq!(last_repo_from(&path), None, "{content}");
        }
    }

    #[test]
    fn unfolded_parts_are_kept_beside_the_sidebar_and_the_repository() {
        let dir = TempDir::new("unfolded");
        let path = dir.0.join(FILE);
        assert!(unfolded_from(&path).is_empty(), "nothing unfolded yet");
        save_to(&path, &UiState { sidebar: SidebarMode::Rail }).expect("save the sidebar");
        save_last_repo_to(&path, "mahype/TorroWorkplace").expect("save the repository");
        let names = vec!["overview.mail.info".to_string(), "overview.notices.info".to_string()];
        save_unfolded_to(&path, &names).expect("save");
        assert_eq!(unfolded_from(&path), names);
        assert_eq!(load_from(&path).sidebar, SidebarMode::Rail, "the sidebar stays");
        assert_eq!(last_repo_from(&path).as_deref(), Some("mahype/TorroWorkplace"), "the repository stays");
        save_to(&path, &UiState::default()).expect("save the sidebar again");
        assert_eq!(unfolded_from(&path), names, "saving the sidebar keeps them");
        save_unfolded_to(&path, &names[1..]).expect("fold one");
        assert_eq!(unfolded_from(&path), &names[1..]);
        for content in ["{ not json", r#"{"unfolded":"overview"}"#, r#"{"unfolded":[7,"a"]}"#] {
            fs::write(&path, content).expect("write");
            let expected: &[&str] = if content.contains('7') { &["a"] } else { &[] };
            assert_eq!(unfolded_from(&path), expected, "{content}");
        }
    }

    /// Task E8.2: every window keeps its own density beside the other keys of the file.
    #[test]
    fn densities_are_kept_per_window_beside_the_rest() {
        let dir = TempDir::new("density");
        let path = dir.0.join(FILE);
        assert_eq!(density_from(&path, "mail"), None, "nothing chosen yet");
        save_to(&path, &UiState { sidebar: SidebarMode::Rail }).expect("save the sidebar");
        save_unfolded_to(&path, &["overview.mail.info".to_string()]).expect("save the unfolded");
        save_density_to(&path, "mail", Density::Compact).expect("save the mail window");
        save_density_to(&path, "hub/lane:info", Density::Comfortable).expect("save a slice of the hub");
        assert_eq!(density_from(&path, "mail"), Some(Density::Compact));
        assert_eq!(density_from(&path, "hub/lane:info"), Some(Density::Comfortable));
        assert_eq!(density_from(&path, "hub"), None, "the hub without a slice is a window of its own");
        assert_eq!(load_from(&path).sidebar, SidebarMode::Rail, "the sidebar stays");
        assert_eq!(unfolded_from(&path), ["overview.mail.info"], "the unfolded parts stay");
        save_density_to(&path, "mail", Density::Comfortable).expect("change the mail window");
        assert_eq!(density_from(&path, "mail"), Some(Density::Comfortable));
        assert_eq!(density_from(&path, "hub/lane:info"), Some(Density::Comfortable), "the other window stays");
        assert!(fs::read_to_string(&path).expect("read").contains(r#""mail": "comfortable""#));
        for content in ["{ not json", r#"{"density":"compact"}"#, r#"{"density":{"mail":"roomy"}}"#] {
            fs::write(&path, content).expect("write");
            assert_eq!(density_from(&path, "mail"), None, "{content}");
        }
    }

    /// Task E8.15: every window keeps its own order beside its density and the other keys.
    #[test]
    fn orders_are_kept_per_window_beside_the_rest() {
        let dir = TempDir::new("order");
        let path = dir.0.join(FILE);
        assert_eq!(order_from(&path, "mail"), None, "nothing chosen yet");
        save_density_to(&path, "mail", Density::Compact).expect("save the density");
        save_order_to(&path, "mail", Order::OldestFirst).expect("save the mail window");
        save_order_to(&path, "notifications", Order::NewestFirst).expect("save another window");
        assert_eq!(order_from(&path, "mail"), Some(Order::OldestFirst));
        assert_eq!(order_from(&path, "notifications"), Some(Order::NewestFirst));
        assert_eq!(order_from(&path, "hub"), None);
        assert_eq!(density_from(&path, "mail"), Some(Density::Compact), "the density stays");
        assert!(fs::read_to_string(&path).expect("read").contains(r#""mail": "oldest-first""#));
        for content in ["{ not json", r#"{"order":"oldest-first"}"#, r#"{"order":{"mail":"oldest"}}"#] {
            fs::write(&path, content).expect("write");
            assert_eq!(order_from(&path, "mail"), None, "{content}");
        }
    }

    #[test]
    fn density_words_and_defaults() {
        assert_eq!(Density::parse("compact"), Some(Density::Compact));
        assert_eq!(Density::parse("comfortable"), Some(Density::Comfortable));
        assert_eq!(Density::parse("cozy"), None);
        for (word, density) in Density::ALL {
            assert_eq!(density.word(), word);
        }
        assert_eq!(Density::default_for(SizeClass::Narrow), Density::Compact, "narrow windows get rows");
        for class in [SizeClass::Medium, SizeClass::Wide, SizeClass::Mini, SizeClass::Strip] {
            assert_eq!(Density::default_for(class), Density::Comfortable, "{class:?}");
        }
    }

    /// The env var `TORRO_STATE_HOME` cannot be set inside a test without `unsafe`, so this test
    /// runs [`app_file_lives_in_the_state_home`] in a child process of the test binary with the
    /// variable pointing to a temp directory, and checks the file landed there.
    #[test]
    fn save_and_load_use_the_state_directory_of_the_app() {
        let dir = TempDir::new("state-home");
        let exe = std::env::current_exe().expect("test binary");
        let status = Command::new(exe)
            .args(["--ignored", "--exact", "ui_state::tests::app_file_lives_in_the_state_home", "--quiet"])
            .env("TORRO_STATE_HOME", &dir.0)
            .stdout(Stdio::null())
            .status()
            .expect("run child test");
        assert!(status.success(), "child test failed");
        let path = dir.0.join(AppId::TASKS.dir).join(FILE);
        assert_eq!(load_from(&path).sidebar, SidebarMode::Rail);
    }

    /// The child of [`save_and_load_use_the_state_directory_of_the_app`]; does nothing unless
    /// `TORRO_STATE_HOME` is set, so `cargo test -- --ignored` never writes into a real home.
    #[test]
    #[ignore = "run by save_and_load_use_the_state_directory_of_the_app with TORRO_STATE_HOME set"]
    fn app_file_lives_in_the_state_home() {
        let Some(home) = std::env::var_os("TORRO_STATE_HOME") else { return };
        let expected = PathBuf::from(home).join(AppId::TASKS.dir).join(FILE);
        assert_eq!(file(AppId::TASKS), Some(expected));
        assert_eq!(load(AppId::TASKS), UiState::default(), "nothing stored yet");
        save(AppId::TASKS, &UiState { sidebar: SidebarMode::Rail }).expect("save");
        assert_eq!(load(AppId::TASKS).sidebar, SidebarMode::Rail);
    }
}
