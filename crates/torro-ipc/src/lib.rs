//! How clients talk to the core.
//!
//! `torroworkplace daemon` owns the window manager connection, the templates and, later,
//! accounts and sync. Every client — `torroworkplace ctl`, hotkeys, later the control
//! centre and the apps — sends [`Request`]s over a local socket and gets one [`Response`]
//! each: `core.sock` in the core's state directory (a Unix domain socket; Windows named
//! pipes follow with the GlazeWM backend).
//!
//! Wire format: one JSON object per line in each direction, one request per connection. A
//! client sends an [`Envelope`] with the API version, its [`Key`] and the [`Request`], whose
//! `method` is the command name from the catalogue:
//!
//! ```json
//! {"v":1,"key":"torro_app_…","request":{"method":"calendar.list_events","start":"…","end":"…"}}
//! ```
//!
//! The core answers with one bare [`Response`]. Without a valid key it answers only
//! `system.ping`; everything else gets the error code `unauthorized`.
//!
//! Task 1.17 in `docs/tasks/phase-1.md` fills in [`server`] and [`client`]. The request set is
//! the API of the suite (`docs/api.md`); [`api`] describes every request for API and MCP.

pub mod api;
pub mod client;
pub mod pim;
pub mod server;

use std::fmt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use torro_core::AppId;
pub use torro_core::account::Secret;
use torro_core::paths::AppPaths;
use torro_core::settings::Settings;
use torro_pim::{AccountState, Collection, Event, Todo};
use torro_setups::LoadMode;
use torro_setups::TemplateSummary;
use torro_setups::restore::RestoreReport;

use crate::api::{Rights, RightsTemplate};

/// Where the core listens: `core.sock` in its state directory.
pub fn socket_path() -> Option<PathBuf> {
    AppPaths::for_app(AppId::WORKPLACE).map(|p| p.state.join("core.sock"))
}

/// The app key of the core listening at `socket`: `app.key` next to it, in the core's state
/// directory. The core creates it on its first start (mode 0600); the suite's own programs read
/// it with [`client::call`] and never handle it themselves.
pub fn app_key_path(socket: &Path) -> PathBuf {
    socket.with_file_name("app.key")
}

/// The API version this crate speaks, `v` in every [`Envelope`]. New commands and new optional
/// fields keep it; renaming, removing or new required fields make version 2.
pub const API_VERSION: u32 = 1;

/// A key as a client sends it: the app key (`torro_app_<64 hex>`) or a client key
/// (`torro_<client id>_<64 hex>`).
///
/// Deliberately without `PartialEq` and with a `Debug` that hides the value: the core compares
/// SHA-256 hashes in constant time, and a key must never end up in a log or an error message.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Key(String);

impl Key {
    pub fn new(key: impl Into<String>) -> Key {
        Key(key.into())
    }

    /// The key itself, for hashing and for sending. Never print it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(…)")
    }
}

/// One line from a client: API version, key and request.
///
/// `key` may be missing; then the core answers only `system.ping`. The core answers with a bare
/// [`Response`], not with an envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// [`API_VERSION`]; a core answers other versions with the error code `unsupported`.
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Key>,
    pub request: Request,
}

/// What a client can ask the core for.
///
/// Every case has an entry in the command catalogue ([`api::CATALOGUE`]), reached through
/// [`Request::operation`]: its API name, area, access and whether agents get it as an MCP tool.
/// A new case does not build until `operation` names its command. On the wire, `method` is the
/// command name (`calendar.list_events`); the `#[serde(rename)]` of each case must equal its
/// [`api::Command::name`], which the catalogue tests check.
///
/// The JSON Schema of every field comes from the types themselves (`schemars`, see
/// [`api::request_schema`]); the doc comments of those types are the descriptions for agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method")]
pub enum Request {
    /// Version and health; also how a client finds out the core is up.
    #[serde(rename = "system.ping")]
    Ping,
    /// Short status for bars and scripts.
    #[serde(rename = "system.status")]
    Status,
    /// Stops the core cleanly, as the tray's Quit does. Answered with [`Response::Ok`] before the
    /// core ends. Only the app key may send it; a client key gets `forbidden`.
    #[serde(rename = "system.quit")]
    Quit,
    /// All templates.
    #[serde(rename = "workspaces.list_templates")]
    ListTemplates,
    /// Saves the active workspace as a new template; `None` picks an automatic name.
    #[serde(rename = "workspaces.save_template")]
    SaveTemplate { name: Option<String> },
    /// Overwrites a template with the active workspace, keeping its name and settings.
    #[serde(rename = "workspaces.update_template")]
    UpdateTemplate { id: String },
    #[serde(rename = "workspaces.delete_template")]
    DeleteTemplate { id: String },
    /// Loads a template onto the active workspace.
    #[serde(rename = "workspaces.load_template")]
    LoadTemplate {
        id: String,
        /// `empty` (only into an empty workspace), `add` or `replace` (closes the windows there).
        mode: LoadMode,
    },

    /// Starts Login Flow v2 for a Nextcloud; the core opens the browser and polls in the
    /// background ([`Response::LoginStarted`]). The account is added to the others; signing in
    /// to an account that exists already replaces its app password, the others stay.
    #[serde(rename = "accounts.connect_nextcloud")]
    ConnectNextcloud { server: String },
    /// Connects the GitHub account the GitHub CLI (`gh`) is signed in to (task G.1). The core runs
    /// `gh auth token` itself, checks the token with GitHub (GraphQL `viewer { login }`, scopes
    /// from `X-OAuth-Scopes`) and keeps it in the system keychain only; the account is
    /// `github:<login>`, added to the others or, for the same login, replacing its token.
    /// Answered with [`Response::Account`]. Error codes: `not-connected` when `gh` is missing or
    /// not signed in (the message says what to do), `auth` when GitHub refuses the token,
    /// `offline`, `error`.
    ///
    /// A client key never takes over the CLI's sign-in: for it the core opens the control
    /// centre's accounts page (`torroworkplace ui --section accounts`), where a person finishes,
    /// and answers [`Response::Opened`].
    #[serde(rename = "accounts.connect_github")]
    ConnectGithub,
    /// Connects a GitHub account with a token a person pasted: checked and stored like
    /// [`Request::ConnectGithub`], answered with [`Response::Account`]; error codes `auth`,
    /// `offline`, `error`. Only the app key may send it: every client key gets `forbidden`,
    /// whatever its rights, because the token is a secret (`docs/api.md`, "Was nie nach außen
    /// geht").
    #[serde(rename = "accounts.set_github_token")]
    SetGithubToken {
        /// A personal access token from github.com, classic (`ghp_…`) or fine-grained
        /// (`github_pat_…`). It goes into the system keychain only and is never part of an answer.
        token: Secret,
    },
    /// Fetch calendars and tasks from the servers of all accounts now. Each account syncs on
    /// its own: one that fails keeps its cache and its problem, the others are updated. The
    /// answer is the first failure, else [`Response::Ok`].
    #[serde(rename = "accounts.sync")]
    Sync,
    /// The calendars the suite shows, i.e. the collections with events that are not hidden
    /// ([`Response::Collections`]). Collection ids are `<account-id>/<collection URL>`; events
    /// and tasks name their collection by this id, writes find their account through it.
    #[serde(rename = "calendar.list_calendars")]
    CalendarCollections,
    /// The task lists the suite shows, i.e. the collections with todos that are not hidden
    /// ([`Response::Collections`]). A collection with both appears in both lists.
    #[serde(rename = "tasks.list_lists")]
    TaskCollections,
    /// Events overlapping `[start, end)`.
    #[serde(rename = "calendar.list_events")]
    Events { start: DateTime<Utc>, end: DateTime<Utc> },
    #[serde(rename = "tasks.list_todos")]
    Todos,
    /// Creates (`href: None`) or updates; answered with [`Response::Event`]. `collection` must be
    /// the exact id of a listed calendar, else `not-found`. Changing `collection` moves it to
    /// another calendar of the same account; moving to another account fails with `unsupported`.
    #[serde(rename = "calendar.save_event")]
    SaveEvent { event: Event },
    #[serde(rename = "calendar.delete_event")]
    DeleteEvent { event: Event },
    /// Creates (`href: None`) or updates; answered with [`Response::Todo`]. `collection` must be
    /// the exact id of a listed task list, else `not-found`. Changing `collection` moves it to
    /// another calendar of the same account; moving to another account fails with `unsupported`.
    #[serde(rename = "tasks.save_todo")]
    SaveTodo { todo: Todo },
    #[serde(rename = "tasks.delete_todo")]
    DeleteTodo { todo: Todo },

    /// The suite-wide settings ([`Response::Settings`]).
    #[serde(rename = "settings.get_settings")]
    Settings,
    /// Replaces the settings; the core applies what changed (start at login, hidden
    /// collections; the language applies to processes started afterwards). A hidden collection
    /// named by a bare URL instead of its id is answered with `error` and nothing changes.
    #[serde(rename = "settings.update_settings")]
    UpdateSettings { settings: Settings },
    /// Changes one setting and applies it like [`Request::UpdateSettings`]. `value` must fit
    /// the key (see [`SettingKey`]); otherwise the answer is the error code `error` and
    /// nothing changes. Answered with [`Response::Ok`].
    #[serde(rename = "settings.set_setting")]
    SetSetting {
        key: SettingKey,
        /// The new value; its type depends on `key`.
        value: serde_json::Value,
    },
    /// Shows (`shown: true`) or hides a calendar or task list in every app of the suite.
    /// `collection` is a collection id of a connected account ([`AccountInfo::collections`]);
    /// an unknown id is answered with `not-found`. Answered with [`Response::Ok`].
    #[serde(rename = "settings.set_collection_shown")]
    SetCollectionShown { collection: String, shown: bool },
    /// All accounts with their state and collections ([`Response::Accounts`]), in the order they
    /// were added; a sign-in that runs is listed last with an empty id.
    #[serde(rename = "accounts.list_accounts")]
    Accounts,
    /// Removes the account `id` (from [`Request::Accounts`]), its secret, its cached data and,
    /// where the server allows, its app password there. Other accounts stay as they are.
    /// Answered with [`Response::Ok`]; an unknown id is `not-found`, a keychain or file that
    /// cannot be changed `error`.
    #[serde(rename = "accounts.remove_account")]
    RemoveAccount { id: String },
    /// Renames a template; its id stays.
    #[serde(rename = "workspaces.rename_template")]
    RenameTemplate { id: String, name: String },
    /// Opens or focuses one of the suite's apps by binary name (`torrocalendar`, `torrotasks`).
    #[serde(rename = "apps.open_app")]
    OpenApp { app: String },
    /// The suite's apps, the core itself excepted, and whether each is installed
    /// ([`Response::Apps`]).
    #[serde(rename = "apps.list_apps")]
    ListApps,

    // Agents and scripts. Only the app key may send these; a client key gets `forbidden`.
    /// All clients with their rights, never with keys ([`Response::Clients`]).
    #[serde(rename = "clients.list_clients")]
    ListClients,
    /// Connects a new client with the rights of `template` and answers with
    /// [`Response::ClientKey`], the only time its key leaves the core. An empty `name` or `kind`
    /// is `error`.
    #[serde(rename = "clients.create_client")]
    CreateClient {
        /// What the person calls it: `Claude Code`.
        name: String,
        /// Which program: `claude-code`, `codex`, `claude-desktop`, `cursor` or `script`.
        kind: String,
        template: RightsTemplate,
    },
    /// Replaces the rights of the client `id`; they apply from its next request. Answered with
    /// [`Response::Ok`]; an unknown id is `not-found`. A selection that names a collection by a
    /// bare URL instead of its id is `error` and changes nothing.
    #[serde(rename = "clients.update_client")]
    UpdateClient { id: String, rights: Rights },
    /// Gives the client `id` a new key, answered with [`Response::ClientKey`]; the old key stops
    /// working at once. An unknown id is `not-found`.
    #[serde(rename = "clients.new_key")]
    NewClientKey { id: String },
    /// Removes the client `id` and its key; the key stops working at once. Answered with
    /// [`Response::Ok`]; an unknown id is `not-found`.
    #[serde(rename = "clients.remove_client")]
    RemoveClient { id: String },
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
    /// How large every window draws, like a browser's zoom: a number from `0.67` to `2.0`
    /// (1.0 = 100 %), rounded to the nearest step (`torro_core::settings::Zoom::STEPS`). Open
    /// windows follow.
    Zoom,
    /// When the night is, for the calendar's timeline and radar: `{"from": "22:00", "to":
    /// "06:00"}`, two times on the quarter hour (`torro_core::settings::Night`), usually over
    /// midnight; the same time twice means no night. Open calendar windows follow.
    Night,
}

/// The core's answer to one [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "result")]
pub enum Response {
    Pong {
        version: String,
        /// The API version the core speaks ([`API_VERSION`]); 0 from cores older than this
        /// field.
        #[serde(default)]
        api: u32,
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
    Clients {
        clients: Vec<ClientInfo>,
    },
    /// A new client or a new key: the only answer that ever carries a client key, and only to
    /// the app key. Show or store it now; the core cannot tell it again.
    ClientKey {
        client: ClientInfo,
        key: IssuedKey,
    },
    /// The account a sign-in just connected ([`Request::ConnectGithub`],
    /// [`Request::SetGithubToken`]), as [`Request::Accounts`] lists it; never with its secret.
    Account {
        account: AccountInfo,
    },
    /// The control centre was opened so a person can finish the request there (a client's
    /// [`Request::ConnectGithub`]); nothing has changed yet.
    Opened,
    /// The request failed; `code` is stable for scripts, `message` is for people. The codes:
    ///
    /// - `not-found`: no such template, account, collection or app (also a collection the
    ///   client may not see),
    /// - `workspace-not-empty`: a load with mode `empty` found windows,
    /// - `busy`: another save or load is running,
    /// - `unsupported`: no supported window manager, or an API version ([`Envelope::v`]) the
    ///   core does not speak,
    /// - `not-connected`: no account to read from or write to, or no GitHub CLI sign-in to take
    ///   over,
    /// - `offline`, `auth`, `conflict`: the server could not be reached, refused the sign-in,
    ///   or the item changed there meanwhile,
    /// - `unauthorized`: no key or a key the core does not know (only `ping` works without),
    /// - `forbidden`: the client's rights do not cover the command, it is for the app key only
    ///   (`clients` commands, `system.quit`, `accounts.set_github_token`), or it would need a
    ///   person's approval, which the core cannot ask for yet,
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
    /// `torro_core::Account::id`, e.g. `nextcloud:anna@cloud.example.org`; the front part of
    /// each of its collection ids (`<id>/<collection URL>`). Empty for a sign-in that runs.
    pub id: String,
    /// Provider: `nextcloud` or `github` (later `slack`, …).
    pub kind: String,
    /// `anna@cloud.example.org`; for GitHub the login.
    pub label: String,
    pub state: AccountState,
    /// All collections of the account, hidden ones included. None for GitHub yet.
    pub collections: Vec<Collection>,
    /// What the account's token may do, as the provider reported it when the core last checked
    /// it: for GitHub the OAuth scopes of a classic token or of the GitHub CLI's sign-in
    /// (`X-OAuth-Scopes`: `repo`, `read:org`, …; empty for a token without scopes). `None` when
    /// the provider does not tell: Nextcloud, and fine-grained GitHub tokens, whose permissions
    /// are set per repository on GitHub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
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

/// One client as [`Request::ListClients`] reports it: everything in `clients.json` but the hash
/// of its key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// Short random id, also part of its key: `3fa9c1d2`.
    pub id: String,
    /// `Claude Code`
    pub name: String,
    /// `claude-code`, `codex`, `claude-desktop`, `cursor` or `script`.
    pub kind: String,
    pub created: DateTime<Utc>,
    /// The last request with its key, to the minute.
    pub last_used: Option<DateTime<Utc>>,
    pub rights: Rights,
}

/// A client key handed out by [`Response::ClientKey`].
///
/// Like [`Key`], its `Debug` hides the value. Unlike `Key` it can be compared, so answers can
/// be: the comparison looks at every byte, whatever differs first. The core never compares keys
/// this way; it checks hashes.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IssuedKey(Key);

impl IssuedKey {
    pub fn new(key: Key) -> IssuedKey {
        IssuedKey(key)
    }

    /// The key itself, to store it in the client's configuration or show it once. Never log it.
    pub fn expose(&self) -> &str {
        self.0.expose()
    }

    pub fn into_key(self) -> Key {
        self.0
    }
}

impl fmt::Debug for IssuedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IssuedKey(…)")
    }
}

impl PartialEq for IssuedKey {
    fn eq(&self, other: &IssuedKey) -> bool {
        let (a, b) = (self.expose().as_bytes(), other.expose().as_bytes());
        let difference = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
        a.len() == b.len() && difference == 0
    }
}

impl Eq for IssuedKey {}

/// What `status --json` prints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub version: String,
    /// Backend name (`hyprland`) or `unsupported`.
    pub backend: String,
    /// The active workspace; `None` too for a client key without read access to `workspaces`.
    pub workspace: Option<String>,
    /// Number of templates; 0 too for a client key without read access to `workspaces`.
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
        assert_eq!(json, r#"{"method":"workspaces.load_template","id":"dev","mode":"add"}"#);
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), req);
    }

    #[test]
    fn envelope_carries_version_key_and_request() {
        let envelope = Envelope { v: API_VERSION, key: Some(Key::new("torro_app_00")), request: Request::Ping };
        let json = serde_json::to_string(&envelope).unwrap();
        assert_eq!(json, r#"{"v":1,"key":"torro_app_00","request":{"method":"system.ping"}}"#);

        let without_key = Envelope { key: None, ..envelope };
        assert_eq!(serde_json::to_string(&without_key).unwrap(), r#"{"v":1,"request":{"method":"system.ping"}}"#);
        let read: Envelope = serde_json::from_str(r#"{"v":1,"request":{"method":"system.status"}}"#).unwrap();
        assert!(read.key.is_none());
        assert_eq!(read.request, Request::Status);
    }

    #[test]
    fn keys_do_not_show_in_debug_output() {
        let envelope = Envelope { v: 1, key: Some(Key::new("torro_app_secret")), request: Request::Ping };
        assert!(!format!("{envelope:?}").contains("secret"));
    }

    #[test]
    fn a_github_token_does_not_show_in_debug_output() {
        let request = Request::SetGithubToken { token: Secret::new("ghp_secret") };
        let envelope = Envelope { v: 1, key: Some(Key::new("torro_app_00")), request: request.clone() };
        assert!(!format!("{envelope:?}").contains("secret"), "{envelope:?}");
        let json = serde_json::to_string(&request).expect("serializes");
        assert_eq!(json, r#"{"method":"accounts.set_github_token","token":"ghp_secret"}"#);
        assert_eq!(serde_json::from_str::<Request>(&json).expect("parses"), request);
    }

    #[test]
    fn accounts_without_scopes_read_and_write_as_before() {
        let json = r#"{"id":"nextcloud:anna@cloud","kind":"nextcloud","label":"anna@cloud",
            "state":{"state":"not-connected"},"collections":[]}"#;
        let account: AccountInfo = serde_json::from_str(json).expect("an answer of an older core");
        assert_eq!(account.scopes, None);
        assert!(!serde_json::to_string(&account).expect("json").contains("scopes"));
        let github = AccountInfo { scopes: Some(vec!["repo".into()]), kind: "github".into(), ..account };
        assert_eq!(serde_json::to_value(&github).expect("json")["scopes"], serde_json::json!(["repo"]));
    }

    #[test]
    fn pong_names_the_api_version_and_reads_older_cores() {
        let pong = Response::Pong { version: "0.1.0".into(), api: API_VERSION };
        assert_eq!(serde_json::to_string(&pong).unwrap(), r#"{"result":"pong","version":"0.1.0","api":1}"#);
        let old: Response = serde_json::from_str(r#"{"result":"pong","version":"0.0.9"}"#).unwrap();
        assert_eq!(old, Response::Pong { version: "0.0.9".into(), api: 0 });
    }

    #[test]
    fn issued_keys_hide_in_debug_output_and_compare_whole() {
        let key = IssuedKey::new(Key::new("torro_c1_secret"));
        let response = Response::ClientKey {
            client: ClientInfo {
                id: "c1".into(),
                name: "Script".into(),
                kind: "script".into(),
                created: Utc::now(),
                last_used: None,
                rights: api::RightsTemplate::ReadOnly.rights(),
            },
            key: key.clone(),
        };
        assert!(!format!("{response:?}").contains("secret"));
        assert_eq!(serde_json::to_value(&response).unwrap()["key"], "torro_c1_secret");
        assert_eq!(key, IssuedKey::new(Key::new("torro_c1_secret")));
        assert_ne!(key, IssuedKey::new(Key::new("torro_c1_secreT")));
        assert_ne!(key, IssuedKey::new(Key::new("torro_c1_secret2")));
    }

    #[test]
    fn app_key_lies_next_to_the_socket() {
        assert_eq!(app_key_path(Path::new("/run/torro/core.sock")), Path::new("/run/torro/app.key"));
    }
}
