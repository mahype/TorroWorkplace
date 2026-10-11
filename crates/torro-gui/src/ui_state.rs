//! What a window remembers about its own look between starts: today whether the sidebar is
//! collapsed (task U.2). `ui.json` in the app's state directory ([`torro_core::paths`]), because it
//! is view state that may be lost – like the scroll position it is no suite setting and goes
//! through no API.
//!
//! A missing or broken file gives the defaults, so it never keeps an app from starting. Writes are
//! atomic (temporary file, then rename). Apps load it in the background at start and save after a
//! change:
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

use crate::shell::SidebarMode;

/// The name of the file in the app's state directory.
pub const FILE: &str = "ui.json";

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

/// Writes `state` to `path` atomically, creating its directory if needed.
pub fn save_to(path: &Path, state: &UiState) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|err| Error::Io(dir.to_path_buf(), err))?;
    }
    let mut json = serde_json::to_string_pretty(state).map_err(Error::Json)?;
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
