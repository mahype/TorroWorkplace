//! The command catalogue: every [`Request`] as an API command (`docs/api.md`, "Der
//! Befehlskatalog").
//!
//! The catalogue is the single source for the three ways into the suite: the control centre and
//! the apps send requests, scripts call the API, agents call MCP tools. Each command has a name
//! (`area.verb_object`; the MCP tool is the same with `_`), the [`Area`] rights are granted for,
//! the [`Access`] it needs, whether agents get it as a tool ([`Mcp`]) and an English description
//! that doubles as the tool description.
//!
//! [`Request::operation`] is an exhaustive `match`, so a new request does not build until it has
//! a command here, and the access it needs can depend on the request's fields (loading a template
//! with `mode: replace` closes windows and needs [`Access::Delete`]).
//!
//! The rights a person grants a client (`docs/api.md`, "Rechte") are described here too
//! ([`Rights`]), next to the areas and access levels they are made of; the core checks them.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Serialize};
use torro_setups::LoadMode;

use crate::Request;

/// The part of the suite a person grants rights for.
///
/// Ordered as the control centre lists them; [`Rights::areas`] is keyed by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Area {
    /// Health and status of the core.
    System,
    /// Workspace templates.
    Workspaces,
    Calendar,
    Tasks,
    /// Connected accounts (Nextcloud, later GitHub, Slack …) and sync.
    Accounts,
    /// Suite-wide settings.
    Settings,
    /// The suite's apps.
    Apps,
    /// The agents and scripts that may use the API, their keys and rights. Only the app key may
    /// use it, whatever a client's rights say: no agent may give itself more rights.
    Clients,
}

impl Area {
    /// Every area, in the order of the enum.
    pub const ALL: [Area; 8] = [
        Area::System,
        Area::Workspaces,
        Area::Calendar,
        Area::Tasks,
        Area::Accounts,
        Area::Settings,
        Area::Apps,
        Area::Clients,
    ];

    /// Rights for this area can be granted to a client. `system` is open to every valid key, except
    /// its admin commands, and `clients` to the app key only, so neither has a level in
    /// [`Rights::areas`].
    pub fn grantable(self) -> bool {
        !matches!(self, Area::System | Area::Clients)
    }

    /// The name in commands and rights: `calendar`. Every command name starts with it.
    pub fn name(self) -> &'static str {
        match self {
            Area::System => "system",
            Area::Workspaces => "workspaces",
            Area::Calendar => "calendar",
            Area::Tasks => "tasks",
            Area::Accounts => "accounts",
            Area::Settings => "settings",
            Area::Apps => "apps",
            Area::Clients => "clients",
        }
    }
}

/// What a command does to the data of its area. Rights are granted per area and level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    /// Changes nothing.
    Read,
    /// Creates, changes or triggers something.
    Write,
    /// Removes or overwrites something for good.
    Delete,
    /// Accounts, settings, connections: a separate switch per area, and an agent always needs a
    /// person's approval for it.
    Admin,
}

impl Access {
    /// Whether holding `self` allows a command that needs `needed`.
    ///
    /// `Read` < `Write` < `Delete`: each covers the ones before it. `Admin` stands apart: it
    /// covers only itself, and no other access covers it, because it is a switch of its own.
    pub fn covers(self, needed: Access) -> bool {
        match (self, needed) {
            (Access::Admin, needed) => needed == Access::Admin,
            (_, Access::Admin) => false,
            (held, needed) => held.rank() >= needed.rank(),
        }
    }

    /// The order of `Read`, `Write` and `Delete`; `Admin` is kept out of it by [`Access::covers`].
    fn rank(self) -> u8 {
        match self {
            Access::Read => 0,
            Access::Write => 1,
            Access::Delete | Access::Admin => 2,
        }
    }
}

/// The level a client has in one area: no access, read, read and write, or full. Ordered from
/// least to most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// No command of the area.
    #[serde(rename = "none")]
    NoAccess,
    /// Only commands that change nothing.
    Read,
    /// Read, create, change and trigger.
    Write,
    /// Everything but the admin commands, deleting and overwriting included.
    Full,
}

impl Level {
    /// The most this level allows, `None` for [`Level::NoAccess`]. Never [`Access::Admin`].
    pub fn access(self) -> Option<Access> {
        match self {
            Level::NoAccess => None,
            Level::Read => Some(Access::Read),
            Level::Write => Some(Access::Write),
            Level::Full => Some(Access::Delete),
        }
    }
}

/// A client's rights in one area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AreaRights {
    pub level: Level,
    /// The admin commands of the area (connect or remove accounts, change settings). Off unless
    /// a person switches it on; even then every admin command of a client needs approval.
    pub admin: bool,
}

impl AreaRights {
    /// No access and no admin commands: what an area not listed in [`Rights::areas`] gets.
    pub const NONE: AreaRights = AreaRights { level: Level::NoAccess, admin: false };

    /// Whether these rights allow a command that needs `access`.
    pub fn allows(self, access: Access) -> bool {
        match access {
            Access::Admin => self.admin,
            needed => self.level.access().is_some_and(|held| held.covers(needed)),
        }
    }
}

/// Which calendars or task lists a client may see: all, or the ones with these ids.
///
/// A collection outside the selection behaves for the client as if it did not exist
/// (`not-found`). Collections hidden in the suite are left out for every client anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Selection {
    /// Every collection of this kind, also ones added later.
    All,
    /// Only the collections with these ids (from `accounts.list_accounts`).
    Only(Vec<String>),
}

impl Selection {
    /// `id` is part of the selection.
    pub fn contains(&self, id: &str) -> bool {
        match self {
            Selection::All => true,
            Selection::Only(ids) => ids.iter().any(|selected| selected == id),
        }
    }

    /// The collections both selections contain.
    pub fn common(&self, other: &Selection) -> Selection {
        match (self, other) {
            (Selection::All, selection) | (selection, Selection::All) => selection.clone(),
            (Selection::Only(ids), other) => {
                Selection::Only(ids.iter().filter(|id| other.contains(id)).cloned().collect())
            }
        }
    }
}

/// When a client's command waits for a person's approval.
///
/// Admin commands always wait, whatever is set here (decision 3 in `docs/api.md`). Ordered from
/// least to most asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Confirm {
    /// Never, except for admin commands.
    Never,
    /// Before deleting or overwriting something.
    #[default]
    OnDelete,
    /// Before every command that changes something.
    OnEveryChange,
}

/// What a client may do. Kept per client in `clients.json`; only the app key may change it.
///
/// The core checks in this order: [`Rights::allows`] (level and admin switch of the area), then
/// the collections ([`Rights::calendars`], [`Rights::task_lists`]), then
/// [`Rights::needs_approval`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Rights {
    /// Level and admin switch per area. An area not listed has no access, so areas added later
    /// start closed. Entries for `system` and `clients` are ignored ([`Area::grantable`]).
    pub areas: BTreeMap<Area, AreaRights>,
    /// The calendars the client sees and may write to.
    pub calendars: Selection,
    /// The task lists the client sees and may write to.
    pub task_lists: Selection,
    /// When a command waits for a person's approval.
    pub confirm: Confirm,
}

impl Rights {
    /// The rights in `area`; [`AreaRights::NONE`] when it is not listed.
    pub fn area(&self, area: Area) -> AreaRights {
        self.areas.get(&area).copied().unwrap_or(AreaRights::NONE)
    }

    /// Whether level and admin switch allow `operation`. Every `system` command is allowed except
    /// the admin ones (`system.quit`), and no `clients` command: those are for the app key only,
    /// so no client can switch the service off or give itself rights.
    ///
    /// Collections and approval are checked after this, by the core.
    pub fn allows(&self, operation: Operation) -> bool {
        match operation.area() {
            Area::System => operation.access != Access::Admin,
            Area::Clients => false,
            area => self.area(area).allows(operation.access),
        }
    }

    /// Whether a command that needs `access` must wait for a person's approval: every admin
    /// command, deleting unless [`Confirm::Never`], every change with [`Confirm::OnEveryChange`].
    ///
    /// Until approvals exist (A.3 in `docs/tasks/api-mcp.md`) the core refuses these commands
    /// with `forbidden` instead of running them without asking.
    pub fn needs_approval(&self, access: Access) -> bool {
        match access {
            Access::Admin => true,
            Access::Delete => self.confirm != Confirm::Never,
            Access::Write => self.confirm == Confirm::OnEveryChange,
            Access::Read => false,
        }
    }

    /// What both `self` and `other` allow: the lower level and both admin switches per area, the
    /// collections in both selections, the stricter approval. The core keeps these when it
    /// cannot store a change of rights, so a failed write never leaves the wider ones in force.
    pub fn common(&self, other: &Rights) -> Rights {
        let areas = self
            .areas
            .keys()
            .map(|&area| {
                let (mine, theirs) = (self.area(area), other.area(area));
                (area, AreaRights { level: mine.level.min(theirs.level), admin: mine.admin && theirs.admin })
            })
            .collect();
        Rights {
            areas,
            calendars: self.calendars.common(&other.calendars),
            task_lists: self.task_lists.common(&other.task_lists),
            confirm: self.confirm.max(other.confirm),
        }
    }

    /// The template these rights equal exactly; `None` once a row was changed.
    pub fn template(&self) -> Option<RightsTemplate> {
        RightsTemplate::ALL.into_iter().find(|template| template.rights() == *self)
    }
}

/// The rights a new client starts with; afterwards every row can be changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RightsTemplate {
    /// Read everything, change nothing.
    ReadOnly,
    /// Read, write and delete everything; deleting waits for a person's approval.
    #[default]
    ReadAndWrite,
    /// Read, write and delete everything without asking.
    FullAccess,
}

impl RightsTemplate {
    pub const ALL: [RightsTemplate; 3] =
        [RightsTemplate::ReadOnly, RightsTemplate::ReadAndWrite, RightsTemplate::FullAccess];

    /// The rights of this template: the same level in every grantable area, all calendars and
    /// task lists. No template switches on admin commands; a person does that per area.
    pub fn rights(self) -> Rights {
        let (level, confirm) = match self {
            RightsTemplate::ReadOnly => (Level::Read, Confirm::OnDelete),
            RightsTemplate::ReadAndWrite => (Level::Full, Confirm::OnDelete),
            RightsTemplate::FullAccess => (Level::Full, Confirm::Never),
        };
        let areas = Area::ALL
            .into_iter()
            .filter(|area| area.grantable())
            .map(|area| (area, AreaRights { level, admin: false }))
            .collect();
        Rights { areas, calendars: Selection::All, task_lists: Selection::All, confirm }
    }
}

/// Whether agents get a command as an MCP tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mcp {
    /// An MCP tool named [`Command::tool_name`].
    Tool,
    /// Never offered to agents. Only for the list "Was nie nach außen geht" in `docs/api.md`.
    Never,
}

/// One command of the API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// `area.verb_object` in snake_case: `calendar.delete_event`. It is also the request's
    /// `method` on the wire (decision 6 in `docs/api.md`), so it is stable like the wire format.
    pub name: &'static str,
    pub area: Area,
    /// The access the command needs at least. [`Request::operation`] may ask for more.
    pub access: Access,
    pub mcp: Mcp,
    /// English, one to three sentences: what it does, when to use it, side effects. Agents read
    /// it as the tool description, so it is not translated.
    pub description: &'static str,
}

impl Command {
    /// The MCP tool name: the command name with `_` for `.` (`calendar_delete_event`).
    pub fn tool_name(&self) -> String {
        self.name.replace('.', "_")
    }

    /// The JSON Schema of the request's fields without its `method` tag: the input of the MCP
    /// tool and of `POST /v1/<area>/<command>` later. `None` when no request has the `method`
    /// [`Command::name`], which the catalogue tests rule out.
    ///
    /// The schemas of all commands are built once per process ([`input_schemas`]), not on every
    /// call: generating the request schema walks every request type.
    pub fn input_schema(&self) -> Option<Schema> {
        input_schemas().get(self.name).cloned()
    }
}

/// The input schema of every request, by its `method` (the command name), built from
/// [`request_schema`] on first use.
fn input_schemas() -> &'static HashMap<&'static str, Schema> {
    static SCHEMAS: OnceLock<HashMap<&'static str, Schema>> = OnceLock::new();
    SCHEMAS.get_or_init(|| {
        let schema = request_schema();
        let variants = schema.get("oneOf").and_then(|v| v.as_array());
        CATALOGUE
            .iter()
            .filter_map(|command| {
                let variant = variants?.iter().find(|v| method_of(v) == Some(command.name))?;
                Some((command.name, input_of(variant.clone())?))
            })
            .collect()
    })
}

/// One `oneOf` branch of [`request_schema`] as a tool input: without the request's doc comment
/// and without the `method` tag.
fn input_of(mut variant: serde_json::Value) -> Option<Schema> {
    let object = variant.as_object_mut()?;
    // The catalogue describes the command; the request's doc comment is for Rust readers.
    object.remove("description");
    if let Some(properties) = object.get_mut("properties").and_then(|p| p.as_object_mut()) {
        properties.remove("method");
    }
    if let Some(required) = object.get_mut("required").and_then(|r| r.as_array_mut()) {
        required.retain(|name| name != "method");
        if required.is_empty() {
            object.remove("required");
        }
    }
    Schema::try_from(variant).ok()
}

/// What one request needs: its command and the access it asks for, which is the command's
/// access or more (loading a template with `mode: replace` needs [`Access::Delete`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operation {
    pub command: &'static Command,
    pub access: Access,
}

impl Operation {
    pub fn area(&self) -> Area {
        self.command.area
    }

    pub fn name(&self) -> &'static str {
        self.command.name
    }
}

impl Request {
    /// The catalogue entry and access of this request; the rights check in the core starts here.
    ///
    /// Deliberately one exhaustive `match` without a catch-all: a new request does not build
    /// until it names its command, so no function reaches the UI without API and MCP.
    pub fn operation(&self) -> Operation {
        let command = match self {
            Request::Ping => &PING,
            Request::Status => &STATUS,
            Request::Quit => &QUIT,
            Request::ListTemplates => &LIST_TEMPLATES,
            Request::SaveTemplate { .. } => &SAVE_TEMPLATE,
            Request::UpdateTemplate { .. } => &UPDATE_TEMPLATE,
            Request::DeleteTemplate { .. } => &DELETE_TEMPLATE,
            Request::LoadTemplate { .. } => &LOAD_TEMPLATE,
            Request::RenameTemplate { .. } => &RENAME_TEMPLATE,
            Request::ConnectNextcloud { .. } => &CONNECT_NEXTCLOUD,
            Request::Sync => &SYNC,
            Request::Accounts => &LIST_ACCOUNTS,
            Request::RemoveAccount { .. } => &REMOVE_ACCOUNT,
            Request::CalendarCollections => &LIST_CALENDARS,
            Request::Events { .. } => &LIST_EVENTS,
            Request::SaveEvent { .. } => &SAVE_EVENT,
            Request::DeleteEvent { .. } => &DELETE_EVENT,
            Request::TaskCollections => &LIST_LISTS,
            Request::Todos => &LIST_TODOS,
            Request::SaveTodo { .. } => &SAVE_TODO,
            Request::DeleteTodo { .. } => &DELETE_TODO,
            Request::Settings => &GET_SETTINGS,
            Request::UpdateSettings { .. } => &UPDATE_SETTINGS,
            Request::SetSetting { .. } => &SET_SETTING,
            Request::SetCollectionShown { .. } => &SET_COLLECTION_SHOWN,
            Request::ListApps => &LIST_APPS,
            Request::OpenApp { .. } => &OPEN_APP,
            Request::ListClients => &LIST_CLIENTS,
            Request::CreateClient { .. } => &CREATE_CLIENT,
            Request::UpdateClient { .. } => &UPDATE_CLIENT,
            Request::NewClientKey { .. } => &NEW_KEY,
            Request::RemoveClient { .. } => &REMOVE_CLIENT,
        };
        let access = match self {
            // Replacing closes the windows on the workspace: nothing brings them back.
            Request::LoadTemplate { mode: LoadMode::Replace, .. } => Access::Delete,
            _ => command.access,
        };
        Operation { command, access }
    }
}

/// The JSON Schema of [`Request`]: `oneOf` with one object per request, told apart by its
/// `method`. Subschemas are inlined, so each branch stands on its own ([`Command::input_schema`]).
pub fn request_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<Request>()
}

/// The `method` constant of one `oneOf` branch of [`request_schema`].
fn method_of(variant: &serde_json::Value) -> Option<&str> {
    variant.pointer("/properties/method/const")?.as_str()
}

/// Every command, grouped by area. The API reference and the MCP tools are built from it.
pub static CATALOGUE: &[&Command] = &[
    &PING,
    &STATUS,
    &QUIT,
    &LIST_TEMPLATES,
    &SAVE_TEMPLATE,
    &UPDATE_TEMPLATE,
    &DELETE_TEMPLATE,
    &LOAD_TEMPLATE,
    &RENAME_TEMPLATE,
    &LIST_CALENDARS,
    &LIST_EVENTS,
    &SAVE_EVENT,
    &DELETE_EVENT,
    &LIST_LISTS,
    &LIST_TODOS,
    &SAVE_TODO,
    &DELETE_TODO,
    &LIST_ACCOUNTS,
    &SYNC,
    &CONNECT_NEXTCLOUD,
    &REMOVE_ACCOUNT,
    &GET_SETTINGS,
    &SET_SETTING,
    &SET_COLLECTION_SHOWN,
    &UPDATE_SETTINGS,
    &LIST_APPS,
    &OPEN_APP,
    &LIST_CLIENTS,
    &CREATE_CLIENT,
    &UPDATE_CLIENT,
    &NEW_KEY,
    &REMOVE_CLIENT,
];

static PING: Command = Command {
    name: "system.ping",
    area: Area::System,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Checks that the Torro Workplace core is running and returns its version. The only command that \
                  works without a valid key.",
};

static STATUS: Command = Command {
    name: "system.status",
    area: Area::System,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Returns a short status: version, window manager, active workspace, number of templates and \
                  whether a template is being saved or loaded.",
};

// Never an MCP tool: an agent must not switch off the service that checks and logs its requests
// (`docs/api.md`, "Was nie nach außen geht", 4). Only the app key may send it.
static QUIT: Command = Command {
    name: "system.quit",
    area: Area::System,
    access: Access::Admin,
    mcp: Mcp::Never,
    description: "Stops the Torro Workplace core, as the tray's Quit does; the socket closes and the tray icon \
                  disappears. Only the app key may use it.",
};

static LIST_TEMPLATES: Command = Command {
    name: "workspaces.list_templates",
    area: Area::Workspaces,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the saved workspace templates with id, name, number of windows and time of the last change.",
};

static SAVE_TEMPLATE: Command = Command {
    name: "workspaces.save_template",
    area: Area::Workspaces,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Saves the windows of the active workspace as a new template and returns it. Without a name the \
                  core picks one from the workspace and the time.",
};

static UPDATE_TEMPLATE: Command = Command {
    name: "workspaces.update_template",
    area: Area::Workspaces,
    access: Access::Delete,
    mcp: Mcp::Tool,
    description: "Overwrites the template with the windows of the active workspace, keeping its name. The previous \
                  content of the template is lost.",
};

static DELETE_TEMPLATE: Command = Command {
    name: "workspaces.delete_template",
    area: Area::Workspaces,
    access: Access::Delete,
    mcp: Mcp::Tool,
    description: "Deletes the template. It cannot be restored.",
};

static LOAD_TEMPLATE: Command = Command {
    name: "workspaces.load_template",
    area: Area::Workspaces,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Starts the template's apps on the active workspace and arranges their windows. Mode `empty` \
                  only loads into an empty workspace, `add` keeps the windows there, `replace` closes them first \
                  and therefore needs delete access.",
};

static RENAME_TEMPLATE: Command = Command {
    name: "workspaces.rename_template",
    area: Area::Workspaces,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Gives the template a new name; its id stays the same.",
};

static LIST_CALENDARS: Command = Command {
    name: "calendar.list_calendars",
    area: Area::Calendar,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the calendars shown in the suite with id, name, colour and whether they can be written \
                  to. Events refer to their calendar by this id.",
};

static LIST_EVENTS: Command = Command {
    name: "calendar.list_events",
    area: Area::Calendar,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the events of all shown calendars that overlap the range from `start` (inclusive) to \
                  `end` (exclusive), both RFC 3339 times.",
};

static SAVE_EVENT: Command = Command {
    name: "calendar.save_event",
    area: Area::Calendar,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Creates an event (without `href`) or changes one (with `href` and `etag` from list_events) on \
                  the server and returns it as stored. Recurring events cannot be changed yet.",
};

static DELETE_EVENT: Command = Command {
    name: "calendar.delete_event",
    area: Area::Calendar,
    access: Access::Delete,
    mcp: Mcp::Tool,
    description: "Deletes the event on the server; it cannot be restored. Pass the event as returned by \
                  list_events (needs `href` and `etag`). Recurring events cannot be deleted yet.",
};

static LIST_LISTS: Command = Command {
    name: "tasks.list_lists",
    area: Area::Tasks,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the task lists shown in the suite with id, name, colour and whether they can be written \
                  to. Tasks refer to their list by this id.",
};

static LIST_TODOS: Command = Command {
    name: "tasks.list_todos",
    area: Area::Tasks,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the tasks of all shown task lists.",
};

static SAVE_TODO: Command = Command {
    name: "tasks.save_todo",
    area: Area::Tasks,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Creates a task (without `href`) or changes one (with `href` and `etag` from list_todos) on the \
                  server and returns it as stored. Marking a task done is a change.",
};

static DELETE_TODO: Command = Command {
    name: "tasks.delete_todo",
    area: Area::Tasks,
    access: Access::Delete,
    mcp: Mcp::Tool,
    description: "Deletes the task on the server; it cannot be restored. Pass the task as returned by list_todos \
                  (needs `href` and `etag`).",
};

static LIST_ACCOUNTS: Command = Command {
    name: "accounts.list_accounts",
    area: Area::Accounts,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the connected accounts with their state (connected, signing in, last sync, problem) and \
                  their calendars and task lists. Passwords and tokens are never part of the answer.",
};

static SYNC: Command = Command {
    name: "accounts.sync",
    area: Area::Accounts,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Fetches calendars, events and tasks from the server now instead of at the next automatic sync, \
                  which runs every five minutes. If the server refuses the sign-in and has marked this device for \
                  a remote wipe, the account and its cached data are removed from this device.",
};

static CONNECT_NEXTCLOUD: Command = Command {
    name: "accounts.connect_nextcloud",
    area: Area::Accounts,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Starts signing in to a Nextcloud server and returns the login URL. A person finishes the sign-in \
                  in the browser; the core then stores the account and syncs. Show the URL to the person; never \
                  open it yourself.",
};

static REMOVE_ACCOUNT: Command = Command {
    name: "accounts.remove_account",
    area: Area::Accounts,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Removes the account from this device: revokes its app password on the server where possible \
                  and deletes the stored secret and the cached calendars and tasks.",
};

static GET_SETTINGS: Command = Command {
    name: "settings.get_settings",
    area: Area::Settings,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Returns the suite-wide settings: language, appearance, start at login, zoom and the hidden \
                  calendars and task lists.",
};

static SET_SETTING: Command = Command {
    name: "settings.set_setting",
    area: Area::Settings,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Changes one suite-wide setting: `language` (\"system\", \"english\", \"german\"), `appearance` \
                  (\"system\", \"light\", \"dark\"), `start_at_login` (true, false) or `zoom` (a number from 0.67 to \
                  2.0, 1.0 = 100 %, rounded to the nearest browser zoom step; every open window follows).",
};

static SET_COLLECTION_SHOWN: Command = Command {
    name: "settings.set_collection_shown",
    area: Area::Settings,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Shows or hides a calendar or task list (id from accounts.list_accounts) in every app of the \
                  suite. Hidden collections are hidden from agents too, so showing one widens what agents see.",
};

static UPDATE_SETTINGS: Command = Command {
    name: "settings.update_settings",
    area: Area::Settings,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Replaces all suite-wide settings at once. Prefer set_setting and set_collection_shown: they \
                  change one value and keep changes others made meanwhile.",
};

static LIST_APPS: Command = Command {
    name: "apps.list_apps",
    area: Area::Apps,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the apps of the suite with binary name, product name, window class and whether they are \
                  installed.",
};

static OPEN_APP: Command = Command {
    name: "apps.open_app",
    area: Area::Apps,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Opens an app of the suite by binary name (from list_apps), or brings its window to the front \
                  when it already runs. `torroworkplace` opens the control centre, which list_apps does not list.",
};

// The `clients` commands are never MCP tools, and only the app key may use them (`docs/api.md`,
// "Was nie nach außen geht", 2): otherwise an agent could give itself more rights.

static LIST_CLIENTS: Command = Command {
    name: "clients.list_clients",
    area: Area::Clients,
    access: Access::Admin,
    mcp: Mcp::Never,
    description: "Lists the agents and scripts that may use the API with name, kind, rights and when they were last \
                  used. Keys are never part of the answer.",
};

static CREATE_CLIENT: Command = Command {
    name: "clients.create_client",
    area: Area::Clients,
    access: Access::Admin,
    mcp: Mcp::Never,
    description: "Connects a new agent or script with the rights of a template and returns its key. The key is in \
                  this answer only; the core keeps just its hash.",
};

static UPDATE_CLIENT: Command = Command {
    name: "clients.update_client",
    area: Area::Clients,
    access: Access::Admin,
    mcp: Mcp::Never,
    description: "Replaces the rights of a client. They apply from its next request.",
};

static NEW_KEY: Command = Command {
    name: "clients.new_key",
    area: Area::Clients,
    access: Access::Admin,
    mcp: Mcp::Never,
    description: "Makes a new key for a client and returns it once. The old key stops working at once.",
};

static REMOVE_CLIENT: Command = Command {
    name: "clients.remove_client",
    area: Area::Clients,
    access: Access::Admin,
    mcp: Mcp::Never,
    description: "Removes a client and its key from this device. The key stops working at once.",
};

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::{TimeZone, Utc};
    use torro_core::settings::Settings;
    use torro_pim::{Event, EventTime, Todo};

    use super::*;
    use crate::SettingKey;

    /// The values an enum schema allows: `enum` for plain enums, `oneOf` with `const` for enums
    /// whose variants have doc comments (schemars then keeps each variant's description).
    fn enum_values(schema: &serde_json::Value) -> Vec<serde_json::Value> {
        if let Some(values) = schema.get("enum").and_then(|e| e.as_array()) {
            return values.clone();
        }
        schema
            .get("oneOf")
            .and_then(|o| o.as_array())
            .expect("an enum schema")
            .iter()
            .map(|branch| branch.get("const").expect("every branch is a const").clone())
            .collect()
    }

    fn event() -> Event {
        Event {
            uid: "u".into(),
            collection: "/cal/work/".into(),
            href: None,
            etag: None,
            summary: "Standup".into(),
            start: EventTime::DateTime(Utc.with_ymd_and_hms(2026, 10, 12, 9, 0, 0).unwrap()),
            end: None,
            location: None,
            description: None,
            recurring: false,
        }
    }

    fn todo() -> Todo {
        Todo {
            uid: "t".into(),
            collection: "/tasks/home/".into(),
            href: None,
            etag: None,
            summary: "Buy milk".into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
        }
    }

    /// One request of every kind. Kept complete by `samples_cover_the_catalogue`.
    fn samples() -> Vec<Request> {
        let now = Utc::now();
        vec![
            Request::Ping,
            Request::Status,
            Request::Quit,
            Request::ListTemplates,
            Request::SaveTemplate { name: None },
            Request::UpdateTemplate { id: "dev".into() },
            Request::DeleteTemplate { id: "dev".into() },
            Request::LoadTemplate { id: "dev".into(), mode: LoadMode::Add },
            Request::RenameTemplate { id: "dev".into(), name: "Dev".into() },
            Request::ConnectNextcloud { server: "cloud.example.test".into() },
            Request::Sync,
            Request::Accounts,
            Request::RemoveAccount { id: "nextcloud:anna@cloud.example.test".into() },
            Request::CalendarCollections,
            Request::Events { start: now, end: now },
            Request::SaveEvent { event: event() },
            Request::DeleteEvent { event: event() },
            Request::TaskCollections,
            Request::Todos,
            Request::SaveTodo { todo: todo() },
            Request::DeleteTodo { todo: todo() },
            Request::Settings,
            Request::UpdateSettings { settings: Settings::default() },
            Request::SetSetting { key: SettingKey::StartAtLogin, value: serde_json::Value::Bool(true) },
            Request::SetCollectionShown { collection: "/cal/work/".into(), shown: false },
            Request::ListApps,
            Request::OpenApp { app: "torrocalendar".into() },
            Request::ListClients,
            Request::CreateClient {
                name: "Claude Code".into(),
                kind: "claude-code".into(),
                template: RightsTemplate::ReadAndWrite,
            },
            Request::UpdateClient { id: "3fa9c1d2".into(), rights: RightsTemplate::ReadOnly.rights() },
            Request::NewClientKey { id: "3fa9c1d2".into() },
            Request::RemoveClient { id: "3fa9c1d2".into() },
        ]
    }

    fn wire_method(request: &Request) -> String {
        let json = serde_json::to_value(request).expect("requests serialize");
        json["method"].as_str().expect("requests carry a method").to_string()
    }

    /// The `method` of every request kind, read from the schema, which `schemars` derives from
    /// the enum itself, so a new request shows up here without anyone listing it.
    fn schema_methods() -> Vec<String> {
        let schema = request_schema();
        let variants = schema.get("oneOf").and_then(|v| v.as_array()).expect("requests are a oneOf");
        variants.iter().map(|v| method_of(v).expect("every branch has a method").to_string()).collect()
    }

    #[test]
    fn names_are_unique_and_every_request_is_in_the_catalogue() {
        let names: HashSet<_> = CATALOGUE.iter().map(|c| c.name).collect();
        assert_eq!(names.len(), CATALOGUE.len(), "command names are unique");
        let tools: HashSet<_> = CATALOGUE.iter().map(|c| c.tool_name()).collect();
        assert_eq!(tools.len(), CATALOGUE.len(), "tool names are unique");

        let mut from_schema = schema_methods();
        from_schema.sort();
        let mut from_catalogue: Vec<String> = CATALOGUE.iter().map(|c| c.name.to_string()).collect();
        from_catalogue.sort();
        assert_eq!(from_schema, from_catalogue, "every request has exactly one command and vice versa");
    }

    #[test]
    fn samples_cover_the_catalogue() {
        let methods: HashSet<_> = samples().iter().map(wire_method).collect();
        assert_eq!(methods.len(), CATALOGUE.len());
    }

    /// Decision 6: one name per command. Every request goes on the wire with its catalogue name
    /// and comes back as the same request.
    #[test]
    fn every_request_is_serialized_with_its_catalogue_name() {
        for request in samples() {
            let name = request.operation().name();
            assert_eq!(wire_method(&request), name, "{request:?}");
            let json = serde_json::to_string(&request).expect("requests serialize");
            assert_eq!(serde_json::from_str::<Request>(&json).expect("and parse again"), request, "{name}");
        }
    }

    #[test]
    fn operation_names_the_command_of_the_request() {
        for request in samples() {
            let operation = request.operation();
            assert_eq!(operation.command.name, wire_method(&request), "{request:?}");
            assert!(CATALOGUE.iter().any(|c| std::ptr::eq(*c, operation.command)), "{request:?} not in CATALOGUE");
        }
    }

    #[test]
    fn names_follow_the_rules() {
        for command in CATALOGUE {
            let (area, verb) = command.name.split_once('.').expect("area.verb_object");
            assert_eq!(area, command.area.name(), "{}", command.name);
            assert!(!verb.is_empty() && verb.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", command.name);
            // MCP clients accept tool names up to 64 characters.
            assert!(command.tool_name().len() <= 64, "{}", command.name);
            assert!(!command.description.is_empty() && command.description.ends_with('.'), "{}", command.name);
            // Rule from docs/api.md, "Was nie nach außen geht": clients, approvals and quitting the
            // service are kept from agents.
            let kept_back = command.area == Area::Clients || command.name == "system.quit";
            assert_eq!(command.mcp == Mcp::Never, kept_back, "{}", command.name);
            // Only the app key may manage clients, so all of their commands are admin commands.
            if command.area == Area::Clients {
                assert_eq!(command.access, Access::Admin, "{}", command.name);
            }
        }
    }

    const ACCESSES: [Access; 4] = [Access::Read, Access::Write, Access::Delete, Access::Admin];
    const LEVELS: [Level; 4] = [Level::NoAccess, Level::Read, Level::Write, Level::Full];

    #[test]
    fn access_covers_lower_levels_and_admin_stands_apart() {
        for held in ACCESSES {
            for needed in ACCESSES {
                let expected = match (held, needed) {
                    (Access::Admin, Access::Admin) => true,
                    (Access::Admin, _) | (_, Access::Admin) => false,
                    (Access::Read, needed) => needed == Access::Read,
                    (Access::Write, needed) => needed != Access::Delete,
                    (Access::Delete, _) => true,
                };
                assert_eq!(held.covers(needed), expected, "{held:?} covers {needed:?}");
            }
        }
    }

    #[test]
    fn levels_and_the_admin_switch_allow_what_they_say() {
        let allows = |level, admin, access| AreaRights { level, admin }.allows(access);
        for admin in [false, true] {
            assert!(!allows(Level::NoAccess, admin, Access::Read));
            assert!(allows(Level::Read, admin, Access::Read) && !allows(Level::Read, admin, Access::Write));
            assert!(allows(Level::Write, admin, Access::Write) && !allows(Level::Write, admin, Access::Delete));
            assert!(allows(Level::Full, admin, Access::Delete));
            for level in LEVELS {
                assert_eq!(allows(level, admin, Access::Admin), admin, "{level:?}");
            }
        }
    }

    #[test]
    fn approval_is_needed_for_admin_always_and_otherwise_as_confirm_says() {
        let needs = |confirm, access| Rights { confirm, ..RightsTemplate::FullAccess.rights() }.needs_approval(access);
        for confirm in [Confirm::Never, Confirm::OnDelete, Confirm::OnEveryChange] {
            assert!(needs(confirm, Access::Admin), "{confirm:?}");
            assert!(!needs(confirm, Access::Read), "{confirm:?}");
        }
        assert!(!needs(Confirm::Never, Access::Delete) && !needs(Confirm::Never, Access::Write));
        assert!(needs(Confirm::OnDelete, Access::Delete) && !needs(Confirm::OnDelete, Access::Write));
        assert!(needs(Confirm::OnEveryChange, Access::Delete) && needs(Confirm::OnEveryChange, Access::Write));
        assert_eq!(Confirm::default(), Confirm::OnDelete);
    }

    #[test]
    fn templates_grant_every_grantable_area_without_admin() {
        for template in RightsTemplate::ALL {
            let rights = template.rights();
            assert_eq!(rights.template(), Some(template));
            for area in Area::ALL {
                assert_eq!(rights.areas.contains_key(&area), area.grantable(), "{template:?} {area:?}");
                assert!(!rights.area(area).admin, "{template:?} {area:?}");
            }
            assert_eq!((&rights.calendars, &rights.task_lists), (&Selection::All, &Selection::All));
        }
        let level = |template: RightsTemplate| template.rights().area(Area::Calendar).level;
        assert_eq!(level(RightsTemplate::ReadOnly), Level::Read);
        assert_eq!(level(RightsTemplate::ReadAndWrite), Level::Full);
        assert_eq!(level(RightsTemplate::FullAccess), Level::Full);
        assert_eq!(RightsTemplate::ReadAndWrite.rights().confirm, Confirm::OnDelete);
        assert_eq!(RightsTemplate::FullAccess.rights().confirm, Confirm::Never);
        assert_eq!(RightsTemplate::default(), RightsTemplate::ReadAndWrite);

        let mut changed = RightsTemplate::ReadOnly.rights();
        changed.calendars = Selection::Only(vec!["/cal/work/".into()]);
        assert_eq!(changed.template(), None);
    }

    /// The table: every command of the catalogue against every level and admin switch. A new
    /// command is covered without touching this test.
    #[test]
    fn rights_allow_each_command_by_level_and_admin_switch() {
        for command in CATALOGUE {
            for level in LEVELS {
                for admin in [false, true] {
                    let mut rights = RightsTemplate::ReadOnly.rights();
                    for area in Area::ALL {
                        rights.areas.insert(area, AreaRights { level, admin });
                    }
                    let operation = Operation { command, access: command.access };
                    let expected = match (command.area, command.access) {
                        (Area::System, Access::Admin) => false,
                        (Area::System, _) => true,
                        (Area::Clients, _) => false,
                        (_, Access::Admin) => admin,
                        (_, Access::Read) => level != Level::NoAccess,
                        (_, Access::Write) => matches!(level, Level::Write | Level::Full),
                        (_, Access::Delete) => level == Level::Full,
                    };
                    assert_eq!(rights.allows(operation), expected, "{} with {level:?}, admin {admin}", command.name);
                }
            }
        }
    }

    #[test]
    fn an_area_without_entry_has_no_access() {
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.areas.remove(&Area::Calendar);
        assert_eq!(rights.area(Area::Calendar), AreaRights::NONE);
        assert!(!rights.allows(Request::CalendarCollections.operation()));
        assert!(rights.allows(Request::Ping.operation()), "system needs no entry");
    }

    #[test]
    fn only_the_app_key_may_quit_the_service() {
        // Not a right a client can be given: even a full-access client may not quit.
        for template in RightsTemplate::ALL {
            assert!(!template.rights().allows(Request::Quit.operation()), "{template:?}");
        }
        assert!(RightsTemplate::ReadOnly.rights().allows(Request::Status.operation()));
    }

    #[test]
    fn rights_are_stored_readably() {
        let mut rights = RightsTemplate::ReadAndWrite.rights();
        rights.task_lists = Selection::Only(vec!["/tasks/home/".into()]);
        let json = serde_json::to_value(&rights).expect("serializes");
        assert_eq!(json["areas"]["calendar"], serde_json::json!({"level": "full", "admin": false}));
        assert_eq!(json["calendars"], serde_json::json!("all"));
        assert_eq!(json["taskLists"], serde_json::json!({"only": ["/tasks/home/"]}));
        assert_eq!(json["confirm"], serde_json::json!("on_delete"));
        assert_eq!(serde_json::from_value::<Rights>(json).expect("parses"), rights);
        assert_eq!(serde_json::to_value(Level::NoAccess).expect("serializes"), serde_json::json!("none"));
    }

    #[test]
    fn common_rights_are_what_both_allow() {
        let mut wide = RightsTemplate::FullAccess.rights();
        wide.areas.insert(Area::Settings, AreaRights { level: Level::Full, admin: true });
        wide.task_lists = Selection::Only(vec!["/tasks/a/".into(), "/tasks/b/".into()]);
        let mut narrow = RightsTemplate::ReadOnly.rights();
        narrow.areas.remove(&Area::Apps);
        narrow.calendars = Selection::Only(vec!["/cal/work/".into()]);
        narrow.task_lists = Selection::Only(vec!["/tasks/b/".into(), "/tasks/c/".into()]);

        for common in [wide.common(&narrow), narrow.common(&wide)] {
            assert_eq!(common.area(Area::Calendar), AreaRights { level: Level::Read, admin: false });
            assert_eq!(common.area(Area::Settings), AreaRights { level: Level::Read, admin: false });
            assert_eq!(common.area(Area::Apps), AreaRights::NONE);
            assert_eq!(common.calendars, Selection::Only(vec!["/cal/work/".into()]));
            assert_eq!(common.task_lists, Selection::Only(vec!["/tasks/b/".into()]));
            assert_eq!(common.confirm, Confirm::OnDelete);
        }
        assert_eq!(wide.common(&wide), wide);
    }

    #[test]
    fn selections_contain_their_ids() {
        assert!(Selection::All.contains("/cal/any/"));
        let only = Selection::Only(vec!["/cal/work/".into()]);
        assert!(only.contains("/cal/work/") && !only.contains("/cal/home/"));
    }

    #[test]
    fn access_follows_the_decisions() {
        let access = |request: Request| request.operation().access;
        assert_eq!(access(Request::UpdateTemplate { id: "dev".into() }), Access::Delete);
        assert_eq!(access(Request::LoadTemplate { id: "dev".into(), mode: LoadMode::Empty }), Access::Write);
        assert_eq!(access(Request::LoadTemplate { id: "dev".into(), mode: LoadMode::Add }), Access::Write);
        assert_eq!(access(Request::LoadTemplate { id: "dev".into(), mode: LoadMode::Replace }), Access::Delete);
        assert_eq!(
            access(Request::SetSetting { key: SettingKey::Language, value: serde_json::json!("german") }),
            Access::Admin
        );
        assert_eq!(access(Request::SetCollectionShown { collection: "x".into(), shown: true }), Access::Admin);
        assert_eq!(
            Request::SetCollectionShown { collection: "x".into(), shown: true }.operation().area(),
            Area::Settings
        );
        assert_eq!(Request::ListApps.operation().area(), Area::Apps);
        assert_eq!(access(Request::ListApps), Access::Read);
        assert_eq!(Request::Accounts.operation().name(), "accounts.list_accounts");
        assert_eq!(Request::ListClients.operation().area(), Area::Clients);
    }

    #[test]
    fn every_command_has_an_input_schema() {
        for command in CATALOGUE {
            let schema = command.input_schema().unwrap_or_else(|| panic!("no schema for {}", command.name));
            assert_eq!(schema.get("type").and_then(|t| t.as_str()), Some("object"), "{}", command.name);
            assert!(schema.get("properties").and_then(|p| p.get("method")).is_none(), "{}", command.name);
            assert!(!schema.as_value().to_string().contains("$ref"), "{} refers outside itself", command.name);
        }
    }

    #[test]
    fn input_schemas_are_built_once_and_match_the_request_schema() {
        assert!(std::ptr::eq(input_schemas(), input_schemas()), "one map per process");
        let schema = request_schema();
        let variants = schema.get("oneOf").and_then(|v| v.as_array()).expect("requests are a oneOf");
        for command in CATALOGUE {
            let variant = variants.iter().find(|v| method_of(v) == Some(command.name)).expect("a branch");
            assert_eq!(command.input_schema(), input_of(variant.clone()), "{}", command.name);
        }
    }

    #[test]
    fn input_schemas_describe_the_fields() {
        let schema = LOAD_TEMPLATE.input_schema().expect("schema");
        assert_eq!(schema.get("required"), Some(&serde_json::json!(["id", "mode"])));
        let mode = schema.as_value().pointer("/properties/mode").expect("mode is a field");
        assert_eq!(serde_json::Value::Array(enum_values(mode)), serde_json::json!(["empty", "add", "replace"]));

        let schema = LIST_EVENTS.input_schema().expect("schema");
        assert_eq!(schema.as_value().pointer("/properties/start/format"), Some(&serde_json::json!("date-time")));

        let schema = SET_SETTING.input_schema().expect("schema");
        let keys =
            schema.as_value().pointer("/properties/key/oneOf").and_then(|k| k.as_array()).expect("key is an enum");
        let keys: Vec<_> = keys.iter().filter_map(|k| k.get("const")).collect();
        assert_eq!(keys, ["language", "appearance", "start_at_login", "zoom"]);

        let schema = SAVE_EVENT.input_schema().expect("schema");
        assert_eq!(schema.as_value().pointer("/properties/event/type"), Some(&serde_json::json!("object")));

        let schema = PING.input_schema().expect("schema");
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn load_mode_schema_matches_its_serialization() {
        let schema = schemars::schema_for!(LoadMode);
        let modes: Vec<_> = [LoadMode::Empty, LoadMode::Add, LoadMode::Replace]
            .iter()
            .map(|mode| serde_json::to_value(mode).expect("serializes"))
            .collect();
        assert_eq!(enum_values(schema.as_value()), modes);
        // A new `LoadMode` variant without a schema entry fails to build here.
        match LoadMode::default() {
            LoadMode::Empty | LoadMode::Add | LoadMode::Replace => {}
        }
    }

    /// Write commands take an item or the settings: their schemas list the fields, not an
    /// empty object.
    #[test]
    fn write_commands_describe_their_fields() {
        let fields = |command: &Command, path: &str| {
            let schema = command.input_schema().expect("schema");
            schema.as_value().pointer(path).cloned().unwrap_or_default()
        };
        for command in [&SAVE_EVENT, &DELETE_EVENT] {
            let event = fields(command, "/properties/event/properties");
            for field in ["uid", "collection", "href", "etag", "summary", "start", "end", "recurring"] {
                assert!(event.get(field).is_some(), "{} lacks {field}", command.name);
            }
        }
        for command in [&SAVE_TODO, &DELETE_TODO] {
            let todo = fields(command, "/properties/todo/properties");
            for field in ["uid", "collection", "href", "etag", "summary", "due", "priority", "completed"] {
                assert!(todo.get(field).is_some(), "{} lacks {field}", command.name);
            }
        }
        let settings = fields(&UPDATE_SETTINGS, "/properties/settings/properties");
        for field in ["language", "appearance", "startAtLogin", "hiddenCollections", "zoom"] {
            assert!(settings.get(field).is_some(), "update_settings lacks {field}");
        }
    }

    /// Agents read the field descriptions, so the identifying fields must have one.
    #[test]
    fn identifying_fields_are_explained() {
        let described = |command: &Command, path: &str| {
            let schema = command.input_schema().expect("schema");
            let field = schema.as_value().pointer(path).cloned().unwrap_or_default();
            field.get("description").and_then(|d| d.as_str()).is_some_and(|d| !d.is_empty())
        };
        for field in ["href", "etag", "collection"] {
            assert!(described(&SAVE_EVENT, &format!("/properties/event/properties/{field}")), "event.{field}");
            assert!(described(&SAVE_TODO, &format!("/properties/todo/properties/{field}")), "todo.{field}");
        }
    }
}
