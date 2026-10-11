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

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use torro_setups::LoadMode;

use crate::Request;

/// The part of the suite a person grants rights for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
}

impl Area {
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
    /// `area.verb_object` in snake_case: `calendar.delete_event`. Stable like the wire format.
    pub name: &'static str,
    /// The `method` tag of the request on the wire (`delete-event`). It predates the catalogue
    /// and differs from the name for some commands; renaming it would break v1 clients.
    pub method: &'static str,
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
    /// tool and of `POST /v1/<area>/<command>` later. `None` when no request has
    /// [`Command::method`], which the catalogue tests rule out.
    pub fn input_schema(&self) -> Option<Schema> {
        let schema = request_schema();
        let variants = schema.get("oneOf")?.as_array()?;
        let mut variant = variants.iter().find(|v| method_of(v) == Some(self.method))?.clone();
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

/// Schema for fields whose types live in crates without `schemars` (`Event`, `Todo`,
/// `Settings`): an object, described by the field's doc comment.
pub(crate) fn object_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "object" })
}

/// Schema stand-in for [`LoadMode`], which lives in `torro-setups` without `schemars`. Only its
/// schema is used; a test keeps the variants in step with `LoadMode`.
#[derive(JsonSchema)]
#[schemars(rename_all = "lowercase")]
#[allow(dead_code)]
pub(crate) enum LoadModeSchema {
    Empty,
    Add,
    Replace,
}

/// Every command, grouped by area. The API reference and the MCP tools are built from it.
pub static CATALOGUE: &[&Command] = &[
    &PING,
    &STATUS,
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
];

static PING: Command = Command {
    name: "system.ping",
    method: "ping",
    area: Area::System,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Checks that the Torro Workplace core is running and returns its version.",
};

static STATUS: Command = Command {
    name: "system.status",
    method: "status",
    area: Area::System,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Returns a short status: version, window manager, active workspace, number of templates and \
                  whether a template is being saved or loaded.",
};

static LIST_TEMPLATES: Command = Command {
    name: "workspaces.list_templates",
    method: "list-templates",
    area: Area::Workspaces,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the saved workspace templates with id, name, number of windows and time of the last change.",
};

static SAVE_TEMPLATE: Command = Command {
    name: "workspaces.save_template",
    method: "save-template",
    area: Area::Workspaces,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Saves the windows of the active workspace as a new template and returns it. Without a name the \
                  core picks one from the workspace and the time.",
};

static UPDATE_TEMPLATE: Command = Command {
    name: "workspaces.update_template",
    method: "update-template",
    area: Area::Workspaces,
    access: Access::Delete,
    mcp: Mcp::Tool,
    description: "Overwrites the template with the windows of the active workspace, keeping its name. The previous \
                  content of the template is lost.",
};

static DELETE_TEMPLATE: Command = Command {
    name: "workspaces.delete_template",
    method: "delete-template",
    area: Area::Workspaces,
    access: Access::Delete,
    mcp: Mcp::Tool,
    description: "Deletes the template. It cannot be restored.",
};

static LOAD_TEMPLATE: Command = Command {
    name: "workspaces.load_template",
    method: "load-template",
    area: Area::Workspaces,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Starts the template's apps on the active workspace and arranges their windows. Mode `empty` \
                  only loads into an empty workspace, `add` keeps the windows there, `replace` closes them first \
                  and therefore needs delete access.",
};

static RENAME_TEMPLATE: Command = Command {
    name: "workspaces.rename_template",
    method: "rename-template",
    area: Area::Workspaces,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Gives the template a new name; its id stays the same.",
};

static LIST_CALENDARS: Command = Command {
    name: "calendar.list_calendars",
    method: "calendar-collections",
    area: Area::Calendar,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the calendars shown in the suite with id, name, colour and whether they can be written \
                  to. Events refer to their calendar by this id.",
};

static LIST_EVENTS: Command = Command {
    name: "calendar.list_events",
    method: "events",
    area: Area::Calendar,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the events of all shown calendars that overlap the range from `start` (inclusive) to \
                  `end` (exclusive), both RFC 3339 times.",
};

static SAVE_EVENT: Command = Command {
    name: "calendar.save_event",
    method: "save-event",
    area: Area::Calendar,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Creates an event (without `href`) or changes one (with `href` and `etag` from list_events) on \
                  the server and returns it as stored. Recurring events cannot be changed yet.",
};

static DELETE_EVENT: Command = Command {
    name: "calendar.delete_event",
    method: "delete-event",
    area: Area::Calendar,
    access: Access::Delete,
    mcp: Mcp::Tool,
    description: "Deletes the event on the server; it cannot be restored. Recurring events cannot be deleted yet.",
};

static LIST_LISTS: Command = Command {
    name: "tasks.list_lists",
    method: "task-collections",
    area: Area::Tasks,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the task lists shown in the suite with id, name, colour and whether they can be written \
                  to. Tasks refer to their list by this id.",
};

static LIST_TODOS: Command = Command {
    name: "tasks.list_todos",
    method: "todos",
    area: Area::Tasks,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the tasks of all shown task lists.",
};

static SAVE_TODO: Command = Command {
    name: "tasks.save_todo",
    method: "save-todo",
    area: Area::Tasks,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Creates a task (without `href`) or changes one (with `href` and `etag` from list_todos) on the \
                  server and returns it as stored. Marking a task done is a change.",
};

static DELETE_TODO: Command = Command {
    name: "tasks.delete_todo",
    method: "delete-todo",
    area: Area::Tasks,
    access: Access::Delete,
    mcp: Mcp::Tool,
    description: "Deletes the task on the server; it cannot be restored.",
};

static LIST_ACCOUNTS: Command = Command {
    name: "accounts.list_accounts",
    method: "accounts",
    area: Area::Accounts,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the connected accounts with their state (connected, signing in, last sync, problem) and \
                  all their calendars and task lists, hidden ones included. Passwords and tokens are never part \
                  of the answer.",
};

static SYNC: Command = Command {
    name: "accounts.sync",
    method: "sync",
    area: Area::Accounts,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Fetches calendars, events and tasks from the server now instead of at the next automatic sync, \
                  which runs every five minutes.",
};

static CONNECT_NEXTCLOUD: Command = Command {
    name: "accounts.connect_nextcloud",
    method: "connect-nextcloud",
    area: Area::Accounts,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Starts signing in to a Nextcloud server and returns the login URL. A person finishes the sign-in \
                  in the browser; the core then stores the account and syncs.",
};

static REMOVE_ACCOUNT: Command = Command {
    name: "accounts.remove_account",
    method: "remove-account",
    area: Area::Accounts,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Removes the account from this device: revokes its app password on the server where possible \
                  and deletes the stored secret and the cached calendars and tasks.",
};

static GET_SETTINGS: Command = Command {
    name: "settings.get_settings",
    method: "settings",
    area: Area::Settings,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Returns the suite-wide settings: language, appearance, start at login and the hidden calendars \
                  and task lists.",
};

static SET_SETTING: Command = Command {
    name: "settings.set_setting",
    method: "set-setting",
    area: Area::Settings,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Changes one suite-wide setting: `language` (\"system\", \"english\", \"german\"), `appearance` \
                  (\"system\", \"light\", \"dark\") or `start_at_login` (true, false).",
};

static SET_COLLECTION_SHOWN: Command = Command {
    name: "settings.set_collection_shown",
    method: "set-collection-shown",
    area: Area::Settings,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Shows or hides a calendar or task list (id from accounts.list_accounts) in every app of the \
                  suite. Hidden collections are hidden from agents too, so showing one widens what agents see.",
};

static UPDATE_SETTINGS: Command = Command {
    name: "settings.update_settings",
    method: "update-settings",
    area: Area::Settings,
    access: Access::Admin,
    mcp: Mcp::Tool,
    description: "Replaces all suite-wide settings at once. Prefer set_setting and set_collection_shown: they \
                  change one value and keep changes others made meanwhile.",
};

static LIST_APPS: Command = Command {
    name: "apps.list_apps",
    method: "list-apps",
    area: Area::Apps,
    access: Access::Read,
    mcp: Mcp::Tool,
    description: "Lists the apps of the suite with binary name, product name, window class and whether they are \
                  installed.",
};

static OPEN_APP: Command = Command {
    name: "apps.open_app",
    method: "open-app",
    area: Area::Apps,
    access: Access::Write,
    mcp: Mcp::Tool,
    description: "Opens an app of the suite by binary name (from list_apps), or brings its window to the front \
                  when it already runs.",
};

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::{TimeZone, Utc};
    use torro_core::settings::Settings;
    use torro_pim::{Event, EventTime, Todo};

    use super::*;
    use crate::SettingKey;

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
        let methods: HashSet<_> = CATALOGUE.iter().map(|c| c.method).collect();
        assert_eq!(methods.len(), CATALOGUE.len(), "every command has a request of its own");

        let mut from_schema = schema_methods();
        from_schema.sort();
        let mut from_catalogue: Vec<String> = CATALOGUE.iter().map(|c| c.method.to_string()).collect();
        from_catalogue.sort();
        assert_eq!(from_schema, from_catalogue, "every request has exactly one command and vice versa");
    }

    #[test]
    fn samples_cover_the_catalogue() {
        let methods: HashSet<_> = samples().iter().map(wire_method).collect();
        assert_eq!(methods.len(), CATALOGUE.len());
    }

    #[test]
    fn operation_names_the_command_of_the_request() {
        for request in samples() {
            let operation = request.operation();
            assert_eq!(operation.command.method, wire_method(&request), "{request:?}");
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
            // Rule from docs/api.md: only secrets, clients and approvals are kept from agents.
            assert_eq!(command.mcp, Mcp::Tool, "{}", command.name);
        }
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
    fn input_schemas_describe_the_fields() {
        let schema = LOAD_TEMPLATE.input_schema().expect("schema");
        assert_eq!(schema.get("required"), Some(&serde_json::json!(["id", "mode"])));
        let modes = schema.as_value().pointer("/properties/mode/enum").expect("mode is an enum");
        assert_eq!(modes, &serde_json::json!(["empty", "add", "replace"]));

        let schema = LIST_EVENTS.input_schema().expect("schema");
        assert_eq!(schema.as_value().pointer("/properties/start/format"), Some(&serde_json::json!("date-time")));

        let schema = SET_SETTING.input_schema().expect("schema");
        let keys =
            schema.as_value().pointer("/properties/key/oneOf").and_then(|k| k.as_array()).expect("key is an enum");
        let keys: Vec<_> = keys.iter().filter_map(|k| k.get("const")).collect();
        assert_eq!(keys, ["language", "appearance", "start_at_login"]);

        let schema = SAVE_EVENT.input_schema().expect("schema");
        assert_eq!(schema.as_value().pointer("/properties/event/type"), Some(&serde_json::json!("object")));

        let schema = PING.input_schema().expect("schema");
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn load_mode_schema_matches_load_mode() {
        let schema = schemars::schema_for!(LoadModeSchema);
        let names = schema.get("enum").expect("an enum of strings");
        let modes: Vec<_> = [LoadMode::Empty, LoadMode::Add, LoadMode::Replace]
            .iter()
            .map(|mode| serde_json::to_value(mode).expect("serializes"))
            .collect();
        assert_eq!(names, &serde_json::Value::Array(modes));
        // A new `LoadMode` without a schema twin fails to build here.
        match LoadMode::default() {
            LoadMode::Empty | LoadMode::Add | LoadMode::Replace => {}
        }
    }
}
