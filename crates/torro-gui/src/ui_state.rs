//! What a window remembers about its own look between starts: whether the sidebar is collapsed
//! (task U.2, [`UiState`]), in the tasks app the repository last chosen for a GitHub filter list
//! (task G.11, [`last_repo_from`]), and which foldable parts the person unfolded – "Info" in the
//! columns of the inbox's overview (task E3.11, [`unfolded_from`]), and in which order each window
//! of the inbox lists by arrival
//! (task E8.15, [`order_from`]), how wide the list beside the reader is (issue #130,
//! [`split_from`]), and the mail searches last made (issue #136, [`searches_from`]). `ui.json` in
//! the app's state directory
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

use crate::shell::SidebarMode;

/// The name of the file in the app's state directory.
pub const FILE: &str = "ui.json";

/// The key of the repository last chosen for a GitHub filter list (`owner/name`), beside the
/// fields of [`UiState`]. One value for all GitHub accounts of the app.
const LAST_REPO: &str = "lastRepo";

/// The key of the parts unfolded ([`unfolded_from`]): a list of names the app chooses.
const UNFOLDED: &str = "unfolded";

/// The key of the orders chosen ([`order_from`]): an object from a window's key to its
/// [`Order`] word.
const ORDER: &str = "order";

/// The key of the split widths chosen ([`split_from`]): an object from a window's key to the width
/// of its first pane in layout units.
const SPLIT: &str = "split";

/// The key of the searches last made ([`searches_from`]): a list of what was searched for, newest
/// first.
const SEARCHES: &str = "recentSearches";

/// How many searches [`save_searches_to`] keeps at most.
pub const SEARCHES_MAX: usize = 8;

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

/// The order by arrival remembered for the window `key` in `path` (task E8.15), as
/// [`save_order_to`] stored it; `None` when there is none, the file cannot be read or the value is
/// no order word ([`Order::word`]).
///
/// `key` names the window, not the app: the three windows of the inbox share one state directory,
/// and one window per slice (`--lane info`, `--source agent:claude-code`) keeps an order of its
/// own. The app builds it from what tells its windows apart, e.g. `mail`, `hub/lane:info`.
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

/// The width of the first pane of a split ([`crate::split`], issue #130) remembered for the window
/// `key` in `path`, as [`save_split_to`] stored it; `None` when there is none, the file cannot be
/// read or the value is no positive number. Windows are keyed as for [`order_from`]; the app
/// clamps the width to its window ([`crate::split::Limits`]).
pub fn split_from(path: &Path, key: &str) -> Option<f32> {
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let width = value.get(SPLIT)?.get(key)?.as_f64()?;
    (width.is_finite() && width > 0.0).then_some(width as f32)
}

/// Stores `width` for the window `key` in `path` – `None`, back to the default, removes it –,
/// keeping the widths of the other windows and everything else in the file.
pub fn save_split_to(path: &Path, key: &str, width: Option<f32>) -> Result<(), Error> {
    let mut widths = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get(SPLIT).and_then(serde_json::Value::as_object).cloned())
        .unwrap_or_default();
    match width.and_then(|width| serde_json::Number::from_f64(f64::from(width.round()))) {
        Some(number) => widths.insert(key.to_string(), serde_json::Value::Number(number)),
        None => widths.remove(key),
    };
    let mut fields = serde_json::Map::new();
    fields.insert(SPLIT.into(), serde_json::Value::Object(widths));
    merge_into(path, fields)
}

/// [`split_from`] for the window `key` of `app` in the background, for the start of a window:
/// `ui_state::load_split_task(app, key).map(Message::StoredSplit)`. Like [`load_task`], tests that
/// only build the app never run it.
pub fn load_split_task(app: AppId, key: String) -> Task<Option<f32>> {
    Task::perform(async move { file(app).and_then(|path| split_from(&path, &key)) }, |width| width)
}

/// [`save_split_to`] for the window `key` of `app` in the background, after a drag ended or the
/// divider was reset. A failure is reported on stderr and otherwise ignored, as in [`save_task`].
pub fn save_split_task<T: Send + 'static>(app: AppId, key: String, width: Option<f32>) -> Task<T> {
    Task::future(async move {
        let saved = file(app).ok_or(Error::NoHome).and_then(|path| save_split_to(&path, &key, width));
        if let Err(err) = saved {
            eprintln!("{}: {err}", app.binary);
        }
    })
    .discard()
}

/// The searches last made, newest first, as [`save_searches_to`] stored them (issue #136): what the
/// person searched for, kept on this device only, for the suggestions under the search field.
/// Empty when nothing is stored, the file cannot be read or the value is no list (other entries
/// of a list are skipped); never more than [`SEARCHES_MAX`].
pub fn searches_from(path: &Path) -> Vec<String> {
    let Some(value) =
        fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return Vec::new();
    };
    let Some(list) = value.get(SEARCHES).and_then(serde_json::Value::as_array) else { return Vec::new() };
    list.iter().filter_map(serde_json::Value::as_str).map(str::to_string).take(SEARCHES_MAX).collect()
}

/// Stores `searches` (newest first, the first [`SEARCHES_MAX`] of them) as the searches last made
/// in `path`, replacing the list before and keeping everything else in the file. An empty list
/// forgets them.
pub fn save_searches_to(path: &Path, searches: &[String]) -> Result<(), Error> {
    let mut fields = serde_json::Map::new();
    let list = searches.iter().take(SEARCHES_MAX).cloned().map(serde_json::Value::String).collect();
    fields.insert(SEARCHES.into(), serde_json::Value::Array(list));
    merge_into(path, fields)
}

/// [`searches_from`] for `app` in the background, for the start of a window:
/// `ui_state::load_searches_task(app).map(Message::RecentSearches)`. Like [`load_task`], tests that
/// only build the app never run it.
pub fn load_searches_task(app: AppId) -> Task<Vec<String>> {
    Task::perform(async move { file(app).map(|path| searches_from(&path)).unwrap_or_default() }, |list| list)
}

/// [`save_searches_to`] for `app` in the background, after a search was made or the list
/// forgotten. A failure is reported on stderr and otherwise ignored, as in [`save_task`].
pub fn save_searches_task<T: Send + 'static>(app: AppId, searches: Vec<String>) -> Task<T> {
    Task::future(async move {
        let saved = file(app).ok_or(Error::NoHome).and_then(|path| save_searches_to(&path, &searches));
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

    /// Task E8.15: every window keeps its own order beside the other keys of the file – the proof
    /// that a write keeps what it does not write, which the density carried until task E13.3.
    #[test]
    fn orders_are_kept_per_window_beside_the_rest() {
        let dir = TempDir::new("order");
        let path = dir.0.join(FILE);
        assert_eq!(order_from(&path, "mail"), None, "nothing chosen yet");
        save_to(&path, &UiState { sidebar: SidebarMode::Rail }).expect("save the sidebar");
        save_unfolded_to(&path, &["overview.mail.info".to_string()]).expect("save the unfolded");
        save_order_to(&path, "mail", Order::OldestFirst).expect("save the mail window");
        save_order_to(&path, "notifications", Order::NewestFirst).expect("save another window");
        save_order_to(&path, "hub/lane:info", Order::OldestFirst).expect("save a slice of the hub");
        assert_eq!(order_from(&path, "mail"), Some(Order::OldestFirst));
        assert_eq!(order_from(&path, "notifications"), Some(Order::NewestFirst));
        assert_eq!(order_from(&path, "hub/lane:info"), Some(Order::OldestFirst));
        assert_eq!(order_from(&path, "hub"), None, "the hub without a slice is a window of its own");
        assert_eq!(load_from(&path).sidebar, SidebarMode::Rail, "the sidebar stays");
        assert_eq!(unfolded_from(&path), ["overview.mail.info"], "the unfolded parts stay");
        save_order_to(&path, "mail", Order::NewestFirst).expect("change the mail window");
        assert_eq!(order_from(&path, "mail"), Some(Order::NewestFirst));
        assert_eq!(order_from(&path, "notifications"), Some(Order::NewestFirst), "the other window stays");
        assert!(fs::read_to_string(&path).expect("read").contains(r#""mail": "newest-first""#));
        for content in ["{ not json", r#"{"order":"oldest-first"}"#, r#"{"order":{"mail":"oldest"}}"#] {
            fs::write(&path, content).expect("write");
            assert_eq!(order_from(&path, "mail"), None, "{content}");
        }
    }

    /// Task E13.3: `density` is no key of `ui.json` any more. An old file that still has one loads
    /// as before – its order and its split are read – and no write adds the key back; there is no
    /// migration, the key is simply left where it is.
    #[test]
    fn an_old_density_key_is_neither_read_nor_written() {
        let dir = TempDir::new("old-density");
        let path = dir.0.join(FILE);
        let old = r#"{"density":{"mail":"comfortable"},"order":{"mail":"oldest-first"},"split":{"mail":380.0}}"#;
        fs::write(&path, old).expect("write");
        assert_eq!(order_from(&path, "mail"), Some(Order::OldestFirst), "the order is still read");
        assert_eq!(split_from(&path, "mail"), Some(380.0), "the split is still read");

        save_order_to(&path, "mail", Order::NewestFirst).expect("save the order");
        let written = fs::read_to_string(&path).expect("read");
        assert!(written.contains(r#""mail": "newest-first""#), "the order was written");
        assert!(written.contains(r#""density""#), "the old key is kept, not migrated away");
        let value: serde_json::Value = serde_json::from_str(&written).expect("still json");
        assert_eq!(value.get("density").and_then(|d| d.get("mail")).and_then(|m| m.as_str()), Some("comfortable"));
    }

    /// Issue #130: every window keeps the width of its split beside the rest; going back to the
    /// default removes it.
    #[test]
    fn split_widths_are_kept_per_window_and_removed_by_a_reset() {
        let dir = TempDir::new("split");
        let path = dir.0.join(FILE);
        assert_eq!(split_from(&path, "mail"), None, "nothing chosen yet");
        save_order_to(&path, "mail", Order::OldestFirst).expect("save the order");
        save_split_to(&path, "mail", Some(522.4)).expect("save the mail window");
        save_split_to(&path, "hub", Some(400.0)).expect("save another window");
        assert_eq!(split_from(&path, "mail"), Some(522.0), "whole units");
        assert_eq!(split_from(&path, "hub"), Some(400.0));
        assert_eq!(order_from(&path, "mail"), Some(Order::OldestFirst), "the order stays");
        save_split_to(&path, "mail", None).expect("reset");
        assert_eq!((split_from(&path, "mail"), split_from(&path, "hub")), (None, Some(400.0)));
        for content in ["{ not json", r#"{"split":520}"#, r#"{"split":{"mail":"wide"}}"#, r#"{"split":{"mail":-3}}"#] {
            fs::write(&path, content).expect("write");
            assert_eq!(split_from(&path, "mail"), None, "{content}");
        }
    }

    /// Issue #136: the searches last made stay beside the rest, at most [`SEARCHES_MAX`], and an
    /// empty list forgets them.
    #[test]
    fn recent_searches_are_kept_beside_the_rest() {
        let dir = TempDir::new("searches");
        let path = dir.0.join(FILE);
        assert!(searches_from(&path).is_empty(), "nothing searched yet");
        save_split_to(&path, "mail", Some(500.0)).expect("save a split");
        let many: Vec<String> = (0..12).map(|n| format!("budget {n}")).collect();
        save_searches_to(&path, &many).expect("save the searches");
        assert_eq!(searches_from(&path), many[..SEARCHES_MAX].to_vec(), "newest first, at most the max");
        assert_eq!(split_from(&path, "mail"), Some(500.0), "the split stays");
        save_searches_to(&path, &[]).expect("forget");
        assert!(searches_from(&path).is_empty());
        for content in ["{ not json", r#"{"recentSearches":"budget"}"#] {
            fs::write(&path, content).expect("write");
            assert!(searches_from(&path).is_empty(), "{content}");
        }
        fs::write(&path, r#"{"recentSearches":["a",3,"b"]}"#).expect("write");
        assert_eq!(searches_from(&path), ["a", "b"], "other entries are skipped");
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
