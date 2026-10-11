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
//! Task 1.17 in `docs/tasks/phase-1.md` fills in [`server`] and [`client`]. The request set is
//! the API of the suite (`docs/api.md`); [`api`] describes every request for API and MCP.

pub mod api;
pub mod client;
pub mod pim;
pub mod server;

use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
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
///
/// Every case has an entry in the command catalogue ([`api::CATALOGUE`]), reached through
/// [`Request::operation`]: its API name, area, access and whether agents get it as an MCP tool.
/// A new case does not build until `operation` names its command.
///
/// The JSON Schema of every field comes from the types themselves (`schemars`, see
/// [`api::request_schema`]); the doc comments of those types are the descriptions for agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
        /// `empty` (only into an empty workspace), `add` or `replace` (closes the windows there).
        mode: LoadMode,
    },

    /// Starts Login Flow v2 for a Nextcloud; the core opens the browser and polls in the
    /// background ([`Response::LoginStarted`]).
    ConnectNextcloud {
        server: String,
    },
    /// Fetch calendars and tasks from the server now.
    Sync,
    /// The calendars the suite shows, i.e. the collections with events that are not hidden
    /// ([`Response::Collections`]).
    CalendarCollections,
    /// The task lists the suite shows, i.e. the collections with todos that are not hidden
    /// ([`Response::Collections`]). A collection with both appears in both lists.
    TaskCollections,
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
    /// Changes one setting and applies it like [`Request::UpdateSettings`]. `value` must fit
    /// the key (see [`SettingKey`]); otherwise the answer is the error code `error` and
    /// nothing changes. Answered with [`Response::Ok`].
    SetSetting {
        key: SettingKey,
        /// The new value; its type depends on `key`.
        value: serde_json::Value,
    },
    /// Shows (`shown: true`) or hides a calendar or task list in every app of the suite.
    /// `collection` is a collection id of a connected account ([`AccountInfo::collections`]);
    /// an unknown id is answered with `not-found`. Answered with [`Response::Ok`].
    SetCollectionShown {
        collection: String,
        shown: bool,
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
    /// The suite's apps, the core itself excepted, and whether each is installed
    /// ([`Response::Apps`]).
    ListApps,
}

/// A setting [`Request::SetSetting`] can change, with the `value` it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingKey {
    /// The language of the suite: `"system"`, `"english"` or `"german"`. Applies to processes
    /// started afterwards.
    Language,
    /// Light or dark colours: `"system"`, `"light"` or `"dark"`.
    Appearance,
    /// Start the core when the user logs in: `true` or `false`.
    StartAtLogin,
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
    Apps {
        apps: Vec<AppInfo>,
    },
    /// The request failed; `code` is stable for scripts, `message` is for people. The codes:
    ///
    /// - `not-found`: no such template, account, collection or app (also a collection the
    ///   client may not see),
    /// - `workspace-not-empty`: a load with mode `empty` found windows,
    /// - `busy`: another save or load is running,
    /// - `unsupported`: no supported window manager,
    /// - `not-connected`: no account to read from or write to,
    /// - `offline`, `auth`, `conflict`: the server could not be reached, refused the sign-in,
    ///   or the item changed there meanwhile,
    /// - `unauthorized`: no key or a key the core does not know (only `ping` works without),
    /// - `forbidden`: the client's rights do not cover the command,
    /// - `pending`: the command waits for a person to approve it,
    /// - `error`: anything else.
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

/// One of the suite's apps, as [`Request::ListApps`] reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Binary name, also what [`Request::OpenApp`] takes: `torrocalendar`.
    pub binary: String,
    /// English product name: `Torro Calendar`. Front ends may show a translated name.
    pub name: String,
    /// Window class, for window rules: `torro-calendar`.
    pub window_class: String,
    /// The binary was found next to the core or on `PATH`, so `OpenApp` can start it.
    pub installed: bool,
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
