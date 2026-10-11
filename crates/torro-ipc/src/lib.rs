//! How clients talk to the core.
//!
//! `torroworkplace daemon` owns the window manager connection, the templates and, later,
//! accounts and sync. Every client — `torroworkplace ctl`, hotkeys, later the control
//! centre and the apps — sends [`Request`]s over a local socket and gets one [`Response`]
//! each: `core.sock` in the core's state directory (a Unix domain socket; Windows named
//! pipes follow with the GlazeWM backend).
//!
//! Wire format: one JSON object per line in each direction, one request per connection.
//!
//! Task 1.17 in `docs/tasks/phase-1.md` fills in [`server`] and [`client`].

pub mod client;
pub mod pim;
pub mod server;

use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_core::settings::Settings;
use torro_pim::{AccountState, Collection, Event, Todo};
use torro_setups::LoadMode;
use torro_setups::TemplateSummary;
use torro_setups::restore::RestoreReport;

/// Where the core listens: `core.sock` in its state directory.
pub fn socket_path() -> Option<PathBuf> {
    AppPaths::for_app(AppId::WORKPLACE).map(|p| p.state.join("core.sock"))
}

/// What a client can ask the core for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "method")]
pub enum Request {
    /// Version and health; also how a client finds out the core is up.
    Ping,
    /// Short status for bars and scripts.
    Status,
    /// All templates.
    ListTemplates,
    /// Saves the active workspace as a new template; `None` picks an automatic name.
    SaveTemplate {
        name: Option<String>,
    },
    /// Overwrites a template with the active workspace, keeping its name and settings.
    UpdateTemplate {
        id: String,
    },
    DeleteTemplate {
        id: String,
    },
    /// Loads a template onto the active workspace.
    LoadTemplate {
        id: String,
        mode: LoadMode,
    },

    /// The account state ([`Response::Account`]).
    Account,
    /// Starts Login Flow v2 for a Nextcloud; the core opens the browser and polls in the
    /// background ([`Response::LoginStarted`]).
    ConnectNextcloud {
        server: String,
    },
    /// Forgets the account and its secret.
    DisconnectAccount,
    /// Fetch calendars and tasks from the server now.
    Sync,
    Collections,
    /// Events overlapping `[start, end)`.
    Events {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
    Todos,
    /// Creates (`href: None`) or updates; answered with [`Response::Event`].
    SaveEvent {
        event: Event,
    },
    DeleteEvent {
        event: Event,
    },
    SaveTodo {
        todo: Todo,
    },
    DeleteTodo {
        todo: Todo,
    },

    /// The suite-wide settings ([`Response::Settings`]).
    Settings,
    /// Replaces the settings; the core applies what changed (start at login, hidden
    /// collections; the language applies to processes started afterwards).
    UpdateSettings {
        settings: Settings,
    },
    /// All accounts with their state and collections ([`Response::Accounts`]).
    Accounts,
    /// Removes an account, its secret and, where the server allows, its app password there.
    RemoveAccount {
        id: String,
    },
    /// Renames a template; its id stays.
    RenameTemplate {
        id: String,
        name: String,
    },
    /// Opens or focuses one of the suite's apps by binary name (`torrocalendar`, `torrotasks`).
    OpenApp {
        app: String,
    },
}

/// The core's answer to one [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "result")]
pub enum Response {
    Pong {
        version: String,
    },
    Status(Status),
    Templates {
        templates: Vec<TemplateSummary>,
    },
    Saved {
        template: TemplateSummary,
    },
    Loaded {
        restored: usize,
        failed: Vec<(String, String)>,
    },
    /// Done, nothing to report.
    Ok,
    Account {
        account: AccountState,
    },
    LoginStarted {
        login_url: String,
    },
    Collections {
        collections: Vec<Collection>,
    },
    Events {
        events: Vec<Event>,
    },
    Todos {
        todos: Vec<Todo>,
    },
    Event {
        event: Event,
    },
    Todo {
        todo: Todo,
    },
    Settings {
        settings: Settings,
    },
    Accounts {
        accounts: Vec<AccountInfo>,
    },
    /// The request failed; `code` is stable for scripts (`workspace-not-empty`, `not-found`,
    /// `unsupported`, `busy`, `not-connected`, `offline`, `auth`, `conflict`, `error`),
    /// `message` is for people.
    Error {
        code: String,
        message: String,
    },
}

impl Response {
    pub fn loaded(report: &RestoreReport) -> Response {
        Response::Loaded { restored: report.restored.len(), failed: report.failed.clone() }
    }
}

/// One account as the control centre shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    /// `torro_core::Account::id`, e.g. `nextcloud:anna@cloud.example.org`.
    pub id: String,
    /// Provider: `nextcloud` (later `github`, `slack`, …).
    pub kind: String,
    /// `anna@cloud.example.org`
    pub label: String,
    pub state: AccountState,
    /// All collections of the account, hidden ones included.
    pub collections: Vec<Collection>,
}

/// What `status --json` prints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub version: String,
    /// Backend name (`hyprland`) or `unsupported`.
    pub backend: String,
    pub workspace: Option<String>,
    pub templates: usize,
    /// A load or save is running.
    pub busy: bool,
}

/// Why talking to the core failed.
#[derive(Debug)]
pub enum IpcError {
    /// Another core already listens on the socket.
    AlreadyRunning,
    /// No core listens on the socket.
    NotRunning,
    /// Not available on this system yet (Windows).
    Unsupported,
    Io(std::io::Error),
    /// A line that is not a request/response.
    Protocol(String),
    /// Part of the scaffold that a task in `docs/tasks/` still has to fill in.
    NotImplemented(&'static str),
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpcError::AlreadyRunning => f.write_str("torroworkplace is already running"),
            IpcError::NotRunning => {
                f.write_str("torroworkplace is not running (start it with `torroworkplace daemon`)")
            }
            IpcError::Unsupported => f.write_str("not supported on this system yet"),
            IpcError::Io(err) => write!(f, "socket: {err}"),
            IpcError::Protocol(err) => write!(f, "unexpected message: {err}"),
            IpcError::NotImplemented(what) => write!(f, "not implemented yet: {what}"),
        }
    }
}

impl std::error::Error for IpcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_tagged_json() {
        let req = Request::LoadTemplate { id: "dev".into(), mode: LoadMode::Add };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"method":"load-template","id":"dev","mode":"add"}"#);
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), req);
    }
}
