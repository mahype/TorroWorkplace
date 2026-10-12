//! The inbox of the core (task E1.2): where entries and rules are kept, and the handlers of the
//! `inbox.*` requests.
//!
//! Everything lives in `inbox.sqlite` in the core's state directory (mode 0600, WAL). SQLite
//! already now, because mail (E4) brings thousands of entries; full-text search (FTS5) comes with
//! E4. Each entry is stored as the JSON of its [`Item`], next to the few columns the queries need
//! (who posted it, thread key, source, state). The database is opened on first use, so building
//! the core reads nothing.
//!
//! The rules themselves (merging, lanes, waking, clean-up, order) are `torro_inbox::rules`; the
//! security rules that depend on who asks are `torro_ipc::api` (`admit_post`, `post_source`,
//! `check_item_change`, `check_source_change`). This module applies them in the order of
//! `docs/tasks/eingang.md`:
//!
//! 1. Approvals, account problems and the source `suite` are changed only by the app key
//!    ([`changeable`], [`changeable_source`]): `forbidden`.
//! 2. The source and the namespace of `thread_key` come from the client: entries are kept per
//!    poster ([`owner`]), so a client merges into and resolves only its own ([`post`], [`resolve`]).
//! 3. `agent_waiting` and `agent_done` only from hooks, anything else becomes a notice; only the
//!    core files approvals ([`post`] through `api::admit_post`).
//! 4. Hook clients see nothing: `list_items` and `get_item` answer `not-found` ([`answer`]), and
//!    every query is limited to the client's `inbox_sources` before it reads a row ([`Scope`]).
//!
//! Requests never come from the core itself, so every entry they make is `untrusted`. New threads
//! are throttled per poster and hour ([`throttle`]); merging into an existing thread and
//! resolving never are. What a post may carry is limited ([`THREAD_KEY_MAX`], [`URL_MAX`],
//! [`AGENT_FIELD_MAX`], [`PIDS_MAX`]), and only links that are safe to open are kept (security
//! rule 6). A client sees the `thread_key` of its own entries only ([`Stored::shown_to`]).
//!
//! Failures of the file reach clients without paths or details; those go to the core's log.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use torro_core::AppId;
use torro_core::i18n::trf;
use torro_core::paths::AppPaths;
use torro_inbox::rules::{self, KEEP_DONE_DAYS, Poster};
use torro_inbox::{
    AgentInfo, AgentKind, Item, ItemFilter, Kind, Lane, Link, Rule, RuleEffect, SourceInfo, SourceRef, State,
    StateFilter,
};
use torro_ipc::api::{self, HOOK_CLIENT, InboxForbidden, Selection};
use torro_ipc::{ClientInfo, Request, Response};

use crate::approvals::InboxFacts;

/// The file in the core's state directory.
pub const FILE: &str = "inbox.sqlite";

/// The layout of the tables, kept in SQLite's `user_version`. A file with a higher number was
/// written by a newer version and is left alone; an older one is brought up to date
/// ([`migrate`]).
pub const SCHEMA_VERSION: i64 = 2;

/// The longest title, in characters; longer ones are cut and end in "…".
pub const TITLE_MAX: usize = 200;

/// The longest body, in characters; agents would otherwise send their whole last answer.
pub const BODY_MAX: usize = 4000;

/// New threads one client may open per hour.
pub const POSTS_PER_HOUR: u32 = 30;

/// New threads one hook client may open per hour: every session of an agent is a thread.
pub const HOOK_POSTS_PER_HOUR: u32 = 120;

/// The longest `thread_key`, in bytes; a longer one is `error`, since cutting it would merge
/// matters that differ only at the end.
pub const THREAD_KEY_MAX: usize = 200;

/// The longest URL of a link, in bytes; a longer one is `error`.
pub const URL_MAX: usize = 2000;

/// The longest `session`, `project` and `branch` of an agent, in bytes; longer ones are cut at a
/// character boundary, without "…", so the same text always gives the same cut.
pub const AGENT_FIELD_MAX: usize = 200;

/// The most process ids of an agent kept, nearest first; the window is found among the first
/// few anyway (E2.4).
pub const PIDS_MAX: usize = 16;

/// The owner of the entries the app key posts (`torroworkplace notify`). Client ids are hex
/// digits, so this never meets one.
const APP_OWNER: &str = "app";

/// Tables and indexes of schema version 1.
///
/// - `items`: one row per entry. `owner` is who posted it (a client id or [`APP_OWNER`]) and,
///   with `thread_key`, unique: the namespace of thread keys (security rule 2). `source` is
///   [`SourceRef::key`], `state` one of `new`, `seen`, `done`, `snoozed` with `state_at` the time
///   it was done or until when it is snoozed. `received` counts new threads for the throttle.
///   `data` is the whole [`Item`] as JSON and the only column read back besides `owner`.
/// - `rules`: one row per lasting rule, at most one mute and one lane per source; `effect` is
///   `muted` or `lane`, `data` the [`Rule`] as JSON.
///
/// Times are RFC 3339 in UTC with microseconds and `Z`, so they compare as text.
const SCHEMA_V1: &str = "
CREATE TABLE items (
    id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    thread_key TEXT NOT NULL,
    source TEXT NOT NULL,
    state TEXT NOT NULL,
    state_at TEXT,
    received TEXT NOT NULL,
    data TEXT NOT NULL,
    UNIQUE (owner, thread_key)
);
CREATE INDEX items_by_source ON items (source);
CREATE INDEX items_by_state ON items (state, state_at);
CREATE INDEX items_by_owner ON items (owner, received);
CREATE TABLE rules (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    effect TEXT NOT NULL,
    data TEXT NOT NULL,
    UNIQUE (source, effect)
);
";

/// From schema version 1 to 2: the columns `inbox.list_items` filters and sorts by in SQL, as
/// [`ItemFilter`] and [`rules::order`] do in memory. `lane` is `for_me` or `info`, `urgent` 0 or
/// 1, `updated` a time like the others. Filled from `data` for rows of version 1 ([`backfill`]).
const SCHEMA_V2: &str = "
ALTER TABLE items ADD COLUMN lane TEXT NOT NULL DEFAULT 'info';
ALTER TABLE items ADD COLUMN urgent INTEGER NOT NULL DEFAULT 0;
ALTER TABLE items ADD COLUMN updated TEXT NOT NULL DEFAULT '';
CREATE INDEX items_by_order ON items (urgent, updated);
";

/// The inbox: `inbox.sqlite`, opened on first use.
pub struct Inbox {
    /// `None` without a home directory (and in tests that forget their own file): every request
    /// then fails with `error` instead of touching the user's state directory.
    file: Option<PathBuf>,
    /// The open database; `None` until first used or after opening failed, then tried again.
    /// Requests are short local queries, so they run under this lock on the caller's thread.
    db: Mutex<Option<Connection>>,
}

impl Inbox {
    /// The user's inbox: [`FILE`] in the core's state directory. Opens nothing until first used.
    /// In tests there is no file, so a test that forgets its own ([`Inbox::at`]) gets `error`
    /// rather than touching the user's state.
    pub fn system() -> Inbox {
        if cfg!(test) {
            return Inbox { file: None, db: Mutex::new(None) };
        }
        let file = AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join(FILE));
        Inbox { file, db: Mutex::new(None) }
    }

    /// The inbox in `file`, created with its directory on first use. For tests; the core uses
    /// [`Inbox::system`].
    #[cfg(test)]
    pub(crate) fn at(file: PathBuf) -> Inbox {
        Inbox { file: Some(file), db: Mutex::new(None) }
    }

    /// Answers one `inbox.*` request of `client` (`None` for the app key) at `now`.
    ///
    /// The key check and the rights of the area must have passed ([`crate::core::Core::handle_from`]);
    /// this applies the security rules of the inbox (module docs). Any other request is `error`.
    pub fn handle(&self, request: Request, client: Option<&ClientInfo>, now: DateTime<Utc>) -> Response {
        self.with(|db| answer(db, request, client, now)).unwrap_or_else(Failure::response)
    }

    /// What `client` gets for `request` before the core asks a person to approve it: the
    /// `not-found` or `forbidden` the request would end in anyway, or `None` when it may go on.
    ///
    /// So nobody is asked to approve muting a source the client cannot see or moving an approval
    /// away. The app key (`None`) is never refused here. The handler checks everything again
    /// when the command runs ([`Inbox::handle`]).
    pub fn refusal(&self, request: &Request, client: Option<&ClientInfo>) -> Option<Response> {
        let client = client?;
        let checked = self.with(|db| match request {
            Request::PostItem { kind, agent, .. } => api::admit_post(*kind, Some(client))
                .and_then(|_| api::post_source(Some(client), agent.as_ref()))
                .map(drop)
                .map_err(Failure::Forbidden),
            Request::SetItemDone { id, .. } | Request::SnoozeItem { id, .. } | Request::SetItemLane { id, .. } => {
                changeable(db, id, Some(client)).map(drop)
            }
            Request::MuteSource { source, .. } => changeable_source(source, Some(client)),
            Request::RemoveRule { id } => removable_rule(db, id, Some(client)).map(drop),
            _ => Ok(()),
        });
        checked.err().map(Failure::response)
    }

    /// What the inbox holds about the entry or rule `request` names, for the summary of its
    /// approval ([`crate::approvals::inbox_summary`]): looked up by the id the command runs with.
    /// Call it only after [`Inbox::refusal`] let the request on, so nothing is named the client
    /// cannot see. Empty for other requests, and when the file cannot be read (logged).
    pub fn facts(&self, request: &Request) -> InboxFacts {
        let looked_up = self.with(|db| {
            let by_id = |id: &str| vec![text(id)];
            Ok(match request {
                Request::SetItemDone { id, .. } | Request::SnoozeItem { id, .. } | Request::SetItemLane { id, .. } => {
                    let item = read_items(db, &Scope::All, "id = ?", by_id(id), "")?.pop().map(|stored| stored.item);
                    InboxFacts { item, rule: None }
                }
                Request::RemoveRule { id } => {
                    InboxFacts { item: None, rule: read_rules(db, &Scope::All, "id = ?", by_id(id))?.pop() }
                }
                _ => InboxFacts::default(),
            })
        });
        looked_up.unwrap_or_else(|failure| {
            eprintln!("torroworkplace: inbox: cannot look up an approval's entry: {failure}");
            InboxFacts::default()
        })
    }

    /// The daemon's routine at `now`: wakes snoozed entries that are due and, with `clean_up`,
    /// removes entries done [`KEEP_DONE_DAYS`] or more ago. Returns how many woke and how many
    /// were removed; `Err` holds the message for the log.
    pub fn tend(&self, now: DateTime<Utc>, clean_up: bool) -> Result<(usize, usize), String> {
        self.with(|db| {
            let woken = wake_due(db, now)?;
            let removed = if clean_up { remove_expired(db, now)? } else { 0 };
            Ok((woken, removed))
        })
        .map_err(|failure| failure.to_string())
    }

    /// Runs `work` on the open database, opening it first if needed.
    fn with<T>(&self, work: impl FnOnce(&mut Connection) -> Result<T, Failure>) -> Result<T, Failure> {
        let mut db = self.lock();
        if db.is_none() {
            let file = self.file.as_deref().ok_or(StoreError::NoHome)?;
            *db = Some(open(file)?);
        }
        match db.as_mut() {
            Some(db) => work(db),
            None => Err(StoreError::NoHome.into()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Connection>> {
        // A panic in a query leaves the connection usable; SQLite rolled back what was open.
        self.db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Why the inbox file could not be used.
#[derive(Debug)]
pub enum StoreError {
    /// No home directory, so no state directory.
    NoHome,
    Io {
        path: PathBuf,
        err: io::Error,
    },
    Sqlite(rusqlite::Error),
    /// A stored entry or rule is no valid JSON of its type.
    Json(serde_json::Error),
    /// The file was written by a newer version (its schema version).
    Newer(i64),
    /// The system has no randomness to make an id from.
    Random(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NoHome => f.write_str("no home directory for the inbox"),
            StoreError::Io { path, err } => write!(f, "{}: {err}", path.display()),
            StoreError::Sqlite(err) => write!(f, "inbox database: {err}"),
            StoreError::Json(err) => write!(f, "inbox database holds a damaged entry: {err}"),
            StoreError::Newer(version) => {
                write!(f, "the inbox was written by a newer version (schema {version}); update Torro Workplace")
            }
            StoreError::Random(err) => write!(f, "no randomness for an id: {err}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> StoreError {
        StoreError::Sqlite(err)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(err: serde_json::Error) -> StoreError {
        StoreError::Json(err)
    }
}

/// Why a request failed, by its error code.
#[derive(Debug)]
enum Failure {
    /// `not-found`: unknown, or of a source the client may not see.
    NotFound(String),
    /// `forbidden`: a security rule of the inbox refused it.
    Forbidden(InboxForbidden),
    /// `busy`: too many new threads in the last hour.
    Busy(String),
    /// `error`: the request itself does not fit (an empty title, a limit).
    Invalid(String),
    /// `error`: the file could not be used. The client is told so without the details, which
    /// may name paths of the user's home; those go to the log.
    Store(StoreError),
}

impl Failure {
    fn response(self) -> Response {
        let (code, message) = match self {
            Failure::Forbidden(refused) => return refused.response(),
            Failure::NotFound(message) => ("not-found", message),
            Failure::Busy(message) => ("busy", message),
            Failure::Invalid(message) => ("error", message),
            Failure::Store(err) => {
                eprintln!("torroworkplace: inbox: {err}");
                ("error", "the inbox could not be read or written; the log of Torro Workplace says why".into())
            }
        };
        Response::Error { code: code.into(), message }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Forbidden(refused) => f.write_str(refused.0),
            Failure::NotFound(message) | Failure::Busy(message) | Failure::Invalid(message) => f.write_str(message),
            Failure::Store(err) => err.fmt(f),
        }
    }
}

impl From<StoreError> for Failure {
    fn from(err: StoreError) -> Failure {
        Failure::Store(err)
    }
}

impl From<rusqlite::Error> for Failure {
    fn from(err: rusqlite::Error) -> Failure {
        Failure::Store(err.into())
    }
}

impl From<serde_json::Error> for Failure {
    fn from(err: serde_json::Error) -> Failure {
        Failure::Store(err.into())
    }
}

fn no_item(id: &str) -> Failure {
    Failure::NotFound(format!("no inbox entry {id:?}"))
}

/// The answer to `request` on the open database; see [`Inbox::handle`].
fn answer(
    db: &mut Connection,
    request: Request,
    client: Option<&ClientInfo>,
    now: DateTime<Utc>,
) -> Result<Response, Failure> {
    match request {
        // Security rule 4: a hook's key sits in files the agent reads, so it reads nothing.
        Request::ListItems { .. } if is_hook(client) => Err(Failure::NotFound("no inbox entries for this key".into())),
        Request::GetItem { id } if is_hook(client) => Err(no_item(&id)),
        Request::ListItems { lane, source, state, since, limit } => {
            wake_due(db, now)?;
            let filter = ItemFilter { lane, source, state, since, limit };
            Ok(Response::Items { items: list_items(db, &filter, client)? })
        }
        Request::GetItem { id } => {
            wake_due(db, now)?;
            Ok(Response::Item { item: visible_item(db, &id, client)?.shown_to(client) })
        }
        Request::ListSources => {
            wake_due(db, now)?;
            Ok(Response::Sources { sources: list_sources(db, client)? })
        }
        Request::PostItem { kind, title, body, link, lane, thread_key, agent } => {
            let id = post(db, Post { kind, title, body, link, lane, thread_key, agent }, client, now)?;
            Ok(Response::Posted { id })
        }
        Request::ResolveItems { thread_key } => {
            resolve(db, &thread_key, client, now)?;
            Ok(Response::Ok)
        }
        Request::SetItemDone { id, done } => {
            let stored = change(db, &id, client, |item| rules::set_done(item, done, now))?;
            Ok(Response::Item { item: stored.shown_to(client) })
        }
        Request::SnoozeItem { id, until } => {
            let stored = change(db, &id, client, |item| rules::snooze(item, until))?;
            Ok(Response::Item { item: stored.shown_to(client) })
        }
        Request::SetItemLane { id, lane, remember } => {
            let tx = db.transaction()?;
            let stored = change(&tx, &id, client, |item| rules::set_lane(item, lane))?;
            if remember {
                remember_lane(&tx, &stored.item.source, lane, now)?;
            }
            tx.commit()?;
            Ok(Response::Item { item: stored.shown_to(client) })
        }
        Request::MuteSource { source, muted } => {
            changeable_source(&source, client)?;
            set_muted(db, &source, muted, now)?;
            Ok(Response::Ok)
        }
        Request::ListRules => Ok(Response::Rules { rules: list_rules(db, client)? }),
        Request::RemoveRule { id } => {
            removable_rule(db, &id, client)?;
            db.execute("DELETE FROM rules WHERE id = ?1", params![id])?;
            Ok(Response::Ok)
        }
        other => Err(Failure::Invalid(format!("{} is no inbox command", other.operation().name()))),
    }
}

fn is_hook(client: Option<&ClientInfo>) -> bool {
    client.is_some_and(|client| client.kind == HOOK_CLIENT)
}

/// Who owns what `client` posts, the namespace of its thread keys: its id, or [`APP_OWNER`].
fn owner(client: Option<&ClientInfo>) -> &str {
    client.map_or(APP_OWNER, |client| client.id.as_str())
}

/// The sources a caller may see, applied to every query before it reads a row.
enum Scope {
    All,
    /// These source keys only ([`SourceRef::key`]); empty sees nothing.
    Only(Vec<String>),
}

impl Scope {
    /// The app key sees all; a client its `inbox_sources`; a hook client nothing, whatever its
    /// rights say (security rule 4; `Rights::for_kind` keeps them empty anyway).
    fn of(client: Option<&ClientInfo>) -> Scope {
        match client {
            None => Scope::All,
            Some(client) if client.kind == HOOK_CLIENT => Scope::Only(Vec::new()),
            Some(client) => match &client.rights.inbox_sources {
                Selection::All => Scope::All,
                Selection::Only(keys) => Scope::Only(keys.clone()),
            },
        }
    }

    fn sees(&self, source: &SourceRef) -> bool {
        match self {
            Scope::All => true,
            Scope::Only(keys) => keys.contains(&source.key()),
        }
    }

    /// The SQL condition on `source` and its parameters; `None` when nothing is visible, so the
    /// query need not run at all.
    fn condition(&self) -> Option<(String, Vec<Value>)> {
        match self {
            Scope::All => Some(("1".into(), Vec::new())),
            Scope::Only(keys) if keys.is_empty() => None,
            Scope::Only(keys) => {
                let marks = vec!["?"; keys.len()].join(", ");
                Some((format!("source IN ({marks})"), keys.iter().cloned().map(Value::Text).collect()))
            }
        }
    }
}

/// An entry as stored, with who posted it ([`owner`]).
struct Stored {
    owner: String,
    item: Item,
}

impl Stored {
    /// The entry as `client` may see it: the `thread_key` of another poster's entry is empty, so
    /// nobody learns the keys of others (the app key, `None`, sees them all).
    fn shown_to(self, client: Option<&ClientInfo>) -> Item {
        let mut item = self.item;
        if client.is_some_and(|client| client.id != self.owner) {
            item.thread_key.clear();
        }
        item
    }
}

/// The rows of `table` (`items` or `rules`) in `scope` that meet `condition` (with `?` for
/// `values`), as `columns` gives them to `row`; `tail` follows the condition (order, limit).
fn read<T>(
    db: &Connection,
    table: &str,
    columns: &str,
    scope: &Scope,
    (condition, values): (&str, Vec<Value>),
    tail: &str,
    row: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, Failure> {
    let Some((visible, mut all)) = scope.condition() else {
        return Ok(Vec::new());
    };
    all.extend(values);
    let sql = format!("SELECT {columns} FROM {table} WHERE {visible} AND ({condition}){tail}");
    let mut statement = db.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(all), row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<T>>>()?)
}

/// The entries in `scope` that meet `condition`, with their owners; `tail` as in [`read`].
fn read_items(
    db: &Connection,
    scope: &Scope,
    condition: &str,
    values: Vec<Value>,
    tail: &str,
) -> Result<Vec<Stored>, Failure> {
    let rows = read(db, "items", "owner, data", scope, (condition, values), tail, |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.into_iter().map(|(owner, data)| Ok(Stored { owner, item: serde_json::from_str(&data)? })).collect()
}

fn read_rules(db: &Connection, scope: &Scope, condition: &str, values: Vec<Value>) -> Result<Vec<Rule>, Failure> {
    let rows = read(db, "rules", "data", scope, (condition, values), "", |row| row.get::<_, String>(0))?;
    rows.iter().map(|data| Ok(serde_json::from_str(data)?)).collect()
}

fn text(value: impl Into<String>) -> Value {
    Value::Text(value.into())
}

/// The entry `id` if `client` sees its source, else `not-found`.
fn visible_item(db: &Connection, id: &str, client: Option<&ClientInfo>) -> Result<Stored, Failure> {
    read_items(db, &Scope::of(client), "id = ?", vec![text(id)], "")?.pop().ok_or_else(|| no_item(id))
}

/// The entry `id` if `client` sees it (`not-found`) and may change it (`forbidden`, security
/// rule 1).
fn changeable(db: &Connection, id: &str, client: Option<&ClientInfo>) -> Result<Stored, Failure> {
    let stored = visible_item(db, id, client)?;
    api::check_item_change(&stored.item, client).map_err(Failure::Forbidden)?;
    Ok(stored)
}

/// `client` sees `source` (`not-found`) and may mute it or remove its rules (`forbidden` for
/// `suite`, security rule 1).
fn changeable_source(source: &SourceRef, client: Option<&ClientInfo>) -> Result<(), Failure> {
    if !Scope::of(client).sees(source) {
        return Err(Failure::NotFound(format!("no inbox source {:?}", source.key())));
    }
    api::check_source_change(source, client).map_err(Failure::Forbidden)
}

/// The rule `id` if `client` sees its source and may change it, as [`changeable_source`].
fn removable_rule(db: &Connection, id: &str, client: Option<&ClientInfo>) -> Result<Rule, Failure> {
    let no_rule = || Failure::NotFound(format!("no inbox rule {id:?}"));
    let rule = read_rules(db, &Scope::of(client), "id = ?", vec![text(id)])?.pop().ok_or_else(no_rule)?;
    api::check_source_change(&rule.source, client).map_err(Failure::Forbidden)?;
    Ok(rule)
}

/// The entries `client` sees that `filter` selects, sorted and cut as [`ItemFilter::apply`]
/// does, but in SQL: with thousands of entries only those asked for are read.
fn list_items(db: &Connection, filter: &ItemFilter, client: Option<&ClientInfo>) -> Result<Vec<Item>, Failure> {
    let scope = Scope::of(client);
    let mut conditions = vec!["1"];
    let mut values = Vec::new();
    if let Some(source) = &filter.source {
        if !scope.sees(source) {
            return Ok(Vec::new());
        }
        conditions.push("source = ?");
        values.push(text(source.key()));
    }
    if let Some(lane) = filter.lane {
        conditions.push("lane = ?");
        values.push(text(lane_column(lane)));
    }
    if let Some(state) = filter.state {
        conditions.push(match state {
            StateFilter::Open => "state IN ('new', 'seen')",
            StateFilter::Snoozed => "state = 'snoozed'",
            StateFilter::Done => "state = 'done'",
        });
    }
    if let Some(since) = filter.since {
        conditions.push("updated >= ?");
        values.push(text(stamp(since)));
    }
    // The order of `rules::order`: urgent first, then the most recently updated, then by id.
    let mut tail = String::from(" ORDER BY urgent DESC, updated DESC, id");
    if let Some(limit) = filter.limit {
        tail.push_str(" LIMIT ?");
        values.push(Value::Integer(i64::from(limit)));
    }
    let rows = read_items(db, &scope, &conditions.join(" AND "), values, &tail)?;
    Ok(rows.into_iter().map(|stored| stored.shown_to(client)).collect())
}

/// The `lane` column of `lane`, as serde writes it.
fn lane_column(lane: Lane) -> &'static str {
    match lane {
        Lane::ForMe => "for_me",
        Lane::Info => "info",
    }
}

/// Every source `client` sees that has entries or is muted, with its open entries, in the order
/// of [`SourceRef`].
fn list_sources(db: &Connection, client: Option<&ClientInfo>) -> Result<Vec<SourceInfo>, Failure> {
    let scope = Scope::of(client);
    let Some((visible, values)) = scope.condition() else {
        return Ok(Vec::new());
    };
    let mut sources = BTreeMap::new();
    let sql = format!("SELECT source, SUM(state IN ('new', 'seen')) FROM items WHERE {visible} GROUP BY source");
    let mut statement = db.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))?;
    for row in rows {
        let (key, open): (String, u32) = row?;
        // A key no version of the core writes is skipped, not shown half.
        if let Ok(source) = key.parse::<SourceRef>() {
            sources.insert(source.clone(), SourceInfo { source, open, muted: false });
        }
    }
    for rule in read_rules(db, &scope, "effect = 'muted'", Vec::new())? {
        let source = rule.source.clone();
        sources.entry(source.clone()).or_insert(SourceInfo { source, open: 0, muted: false }).muted = true;
    }
    Ok(sources.into_values().collect())
}

/// The rules about sources `client` sees, oldest first.
fn list_rules(db: &Connection, client: Option<&ClientInfo>) -> Result<Vec<Rule>, Failure> {
    let mut rules = read_rules(db, &Scope::of(client), "1", Vec::new())?;
    rules.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id)));
    Ok(rules)
}

/// The fields of `inbox.post_item`.
struct Post {
    kind: Kind,
    title: String,
    body: Option<String>,
    link: Option<Link>,
    lane: Option<Lane>,
    thread_key: Option<String>,
    agent: Option<AgentInfo>,
}

/// Files the report of `client`: merged into its own entry with the same `thread_key`, or a new
/// entry, which the throttle counts. Returns the entry's id; `None` when the source is muted and
/// the report was dropped.
fn post(
    db: &mut Connection,
    post: Post,
    client: Option<&ClientInfo>,
    now: DateTime<Utc>,
) -> Result<Option<String>, Failure> {
    let kind = api::admit_post(post.kind, client).map_err(Failure::Forbidden)?;
    // A hook reports for its own agent only (`forbidden` otherwise), so muting holds.
    let source = api::post_source(client, post.agent.as_ref()).map_err(Failure::Forbidden)?;
    let given = post.title.trim();
    if given.is_empty() {
        return Err(Failure::Invalid("a report needs a title".into()));
    }
    let thread_key = checked_thread_key(post.thread_key)?;
    if let Some(Link::Url { url }) = &post.link
        && url.len() > URL_MAX
    {
        return Err(Failure::Invalid(format!("a link has at most {URL_MAX} bytes")));
    }
    let poster = api::inbox_poster(client);
    // Only hooks know an agent session, and only their entries may jump to its window (E2.4).
    let agent = post.agent.filter(|_| poster == Poster::Hook).map(capped_agent);
    let own_agent = client.and_then(|client| client.agent).filter(|_| poster == Poster::Hook);
    let rules = read_rules(db, &Scope::All, "source = ?", vec![text(source.key())])?;
    if rules.iter().any(|rule| rule.effect == RuleEffect::Muted) {
        return Ok(None);
    }
    // A lane the person remembered for the source beats the one the poster asks for.
    let remembered = rules.iter().find_map(|rule| match rule.effect {
        RuleEffect::Lane { lane } => Some(lane),
        RuleEffect::Muted => None,
    });
    let (lane, urgent) = rules::placement(kind, remembered.or(post.lane));
    let title = agent_title(kind, own_agent).unwrap_or_else(|| given.to_string());
    let body = post.body.as_deref().map(str::trim).filter(|body| !body.is_empty()).map(|body| cap(body, BODY_MAX));
    let owner = owner(client);

    let tx = db.transaction()?;
    let existing = match &thread_key {
        Some(key) => read_owned(&tx, owner, key)?,
        None => None,
    };
    let id = match &existing {
        Some(existing) => existing.id.clone(),
        None => new_id()?,
    };
    let mut incoming = Item {
        id: id.clone(),
        source,
        thread_key: thread_key.unwrap_or_else(|| format!("item:{id}")),
        kind,
        lane,
        urgent,
        title: cap(&title, TITLE_MAX),
        body,
        from: None,
        agent,
        received: now,
        updated: now,
        count: 1,
        state: State::New,
        link: post.link,
        actions: Vec::new(),
        // Requests never come from the core: whatever they carry is foreign text (rule 5).
        untrusted: true,
    };
    // Security rule 6: only what is safe to open is kept (web pages, apps of the suite, the
    // window of a hook's agent); anything else is dropped, not refused.
    if !rules::may_open(&incoming, is_app_of_the_suite) {
        incoming.link = None;
    }
    match existing {
        Some(existing) => update_item(&tx, &rules::merge(existing, incoming))?,
        None => {
            throttle(&tx, owner, poster, now)?;
            insert_item(&tx, owner, &incoming)?;
        }
    }
    tx.commit()?;
    Ok(Some(id))
}

/// The `thread_key` of a post: `None` when left out or empty, `error` when longer than
/// [`THREAD_KEY_MAX`] bytes.
fn checked_thread_key(thread_key: Option<String>) -> Result<Option<String>, Failure> {
    match thread_key {
        Some(key) if key.len() > THREAD_KEY_MAX => {
            Err(Failure::Invalid(format!("a thread_key has at most {THREAD_KEY_MAX} bytes")))
        }
        key => Ok(key.filter(|key| !key.is_empty())),
    }
}

/// `agent` with its texts cut to [`AGENT_FIELD_MAX`] bytes and at most [`PIDS_MAX`] pids.
fn capped_agent(mut agent: AgentInfo) -> AgentInfo {
    agent.session = cap_bytes(&agent.session, AGENT_FIELD_MAX);
    agent.project = agent.project.map(|project| cap_bytes(&project, AGENT_FIELD_MAX));
    agent.branch = agent.branch.map(|branch| cap_bytes(&branch, AGENT_FIELD_MAX));
    agent.pids.truncate(PIDS_MAX);
    agent
}

/// `text` cut to at most `max` bytes at a character boundary.
fn cap_bytes(text: &str, max: usize) -> String {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// `app` is the binary of an app of the suite ([`AppId::ALL`]), so a link may open it.
fn is_app_of_the_suite(app: &str) -> bool {
    AppId::ALL.iter().any(|known| known.binary == app)
}

/// The title the core writes itself for an agent's state (`agent_waiting`, `agent_done`), in the
/// core's language: about `agent`, the agent the hook client was made for. `None` for other
/// kinds or without an agent.
fn agent_title(kind: Kind, agent: Option<AgentKind>) -> Option<String> {
    let name = agent?.label();
    match kind {
        Kind::AgentWaiting => Some(trf("{agent} is waiting for your permission", &[("agent", &name)])),
        Kind::AgentDone => Some(trf("{agent} finished", &[("agent", &name)])),
        Kind::Notice | Kind::Approval | Kind::AccountProblem => None,
    }
}

/// `text` cut to `max` characters, at a character boundary, ending in "…" when it was longer.
pub fn cap(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut capped: String = text.chars().take(max.saturating_sub(1)).collect();
    capped.push('…');
    capped
}

/// `busy` when `owner` opened its hour's worth of new threads already: [`POSTS_PER_HOUR`], for
/// hooks [`HOOK_POSTS_PER_HOUR`]. Counted from the entries it created in the last hour, so a
/// restart forgets nothing; reports merged into a thread created earlier do not count.
fn throttle(db: &Connection, owner: &str, poster: Poster, now: DateTime<Utc>) -> Result<(), Failure> {
    let limit = match poster {
        Poster::Hook => HOOK_POSTS_PER_HOUR,
        Poster::Client => POSTS_PER_HOUR,
        Poster::Core => return Ok(()),
    };
    let since = stamp(now - TimeDelta::hours(1));
    let created: u32 =
        db.query_row("SELECT COUNT(*) FROM items WHERE owner = ?1 AND received > ?2", params![owner, since], |row| {
            row.get(0)
        })?;
    if created >= limit {
        return Err(Failure::Busy(format!("more than {limit} new inbox entries within an hour; try again later")));
    }
    Ok(())
}

/// The matter of `thread_key` went away: `client`'s own entry with it is done, snoozed or not
/// (it should not come back about a matter that is gone). Entries of others are never touched.
fn resolve(
    db: &mut Connection,
    thread_key: &str,
    client: Option<&ClientInfo>,
    now: DateTime<Utc>,
) -> Result<(), Failure> {
    checked_thread_key(Some(thread_key.to_string()))?;
    let tx = db.transaction()?;
    if let Some(mut item) = read_owned(&tx, owner(client), thread_key)?
        && !matches!(item.state, State::Done { .. })
    {
        rules::set_done(&mut item, true, now);
        update_item(&tx, &item)?;
    }
    tx.commit()?;
    Ok(())
}

/// Changes the entry `id` with `apply` if `client` may ([`changeable`]) and returns it as stored.
fn change(
    db: &Connection,
    id: &str,
    client: Option<&ClientInfo>,
    apply: impl FnOnce(&mut Item),
) -> Result<Stored, Failure> {
    let mut stored = changeable(db, id, client)?;
    apply(&mut stored.item);
    update_item(db, &stored.item)?;
    Ok(stored)
}

/// Keeps `lane` for later notices of `source`, replacing a lane remembered before.
fn remember_lane(db: &Connection, source: &SourceRef, lane: Lane, now: DateTime<Utc>) -> Result<(), Failure> {
    db.execute("DELETE FROM rules WHERE source = ?1 AND effect = 'lane'", params![source.key()])?;
    insert_rule(db, Rule { id: new_id()?, source: source.clone(), effect: RuleEffect::Lane { lane }, created: now })
}

/// Mutes `source` (once; muting twice keeps the first rule) or unmutes it.
fn set_muted(db: &Connection, source: &SourceRef, muted: bool, now: DateTime<Utc>) -> Result<(), Failure> {
    if !muted {
        db.execute("DELETE FROM rules WHERE source = ?1 AND effect = 'muted'", params![source.key()])?;
        return Ok(());
    }
    insert_rule(db, Rule { id: new_id()?, source: source.clone(), effect: RuleEffect::Muted, created: now })
}

/// Stores `rule`; a rule of the same effect for the same source stays as it is.
fn insert_rule(db: &Connection, rule: Rule) -> Result<(), Failure> {
    let effect = match rule.effect {
        RuleEffect::Lane { .. } => "lane",
        RuleEffect::Muted => "muted",
    };
    db.execute(
        "INSERT INTO rules (id, source, effect, data) VALUES (?1, ?2, ?3, ?4) ON CONFLICT (source, effect) DO NOTHING",
        params![rule.id, rule.source.key(), effect, serde_json::to_string(&rule)?],
    )?;
    Ok(())
}

/// `owner`'s entry with `thread_key`, whatever its state.
fn read_owned(db: &Connection, owner: &str, thread_key: &str) -> Result<Option<Item>, Failure> {
    let data: Option<String> = db
        .query_row("SELECT data FROM items WHERE owner = ?1 AND thread_key = ?2", params![owner, thread_key], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(data.map(|data| serde_json::from_str(&data)).transpose()?)
}

/// Stores a new entry of `owner`.
fn insert_item(db: &Connection, owner: &str, item: &Item) -> Result<(), Failure> {
    let (state, state_at) = state_columns(&item.state);
    db.execute(
        "INSERT INTO items (id, owner, thread_key, source, state, state_at, received, data, lane, urgent, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            item.id,
            owner,
            item.thread_key,
            item.source.key(),
            state,
            state_at,
            stamp(item.received),
            serde_json::to_string(item)?,
            lane_column(item.lane),
            item.urgent,
            stamp(item.updated)
        ],
    )?;
    Ok(())
}

/// Stores the changed `item` over its row. Owner, thread key, source and `received` never change
/// after it was created ([`rules::merge`] keeps them).
fn update_item(db: &Connection, item: &Item) -> Result<(), StoreError> {
    let (state, state_at) = state_columns(&item.state);
    db.execute(
        "UPDATE items SET state = ?2, state_at = ?3, data = ?4, lane = ?5, urgent = ?6, updated = ?7 WHERE id = ?1",
        params![
            item.id,
            state,
            state_at,
            serde_json::to_string(item)?,
            lane_column(item.lane),
            item.urgent,
            stamp(item.updated)
        ],
    )?;
    Ok(())
}

/// Wakes every snoozed entry due at `now` ([`rules::wake`]); returns how many.
fn wake_due(db: &mut Connection, now: DateTime<Utc>) -> Result<usize, Failure> {
    let due = read_items(db, &Scope::All, "state = 'snoozed' AND state_at <= ?", vec![text(stamp(now))], "")?;
    if due.is_empty() {
        return Ok(0);
    }
    let tx = db.transaction()?;
    let mut woken = 0;
    for Stored { mut item, .. } in due {
        if rules::wake(&mut item, now) {
            update_item(&tx, &item)?;
            woken += 1;
        }
    }
    tx.commit()?;
    Ok(woken)
}

/// Removes the entries done [`KEEP_DONE_DAYS`] or more before `now` ([`rules::expired`]);
/// returns how many.
fn remove_expired(db: &Connection, now: DateTime<Utc>) -> Result<usize, Failure> {
    let before = stamp(now - TimeDelta::days(KEEP_DONE_DAYS));
    Ok(db.execute("DELETE FROM items WHERE state = 'done' AND state_at <= ?1", params![before])?)
}

/// The `state` and `state_at` columns of `state`.
fn state_columns(state: &State) -> (&'static str, Option<String>) {
    match state {
        State::New => ("new", None),
        State::Seen => ("seen", None),
        State::Done { at } => ("done", Some(stamp(*at))),
        State::Snoozed { until } => ("snoozed", Some(stamp(*until))),
    }
}

/// `time` as stored: RFC 3339, UTC, microseconds, so that text order is time order.
fn stamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// 16 random hex digits.
fn new_id() -> Result<String, Failure> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|err| StoreError::Random(err.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Opens `file`, creating it (0600) and its directory (0700) if needed, in WAL mode, with the
/// tables of [`SCHEMA_VERSION`].
fn open(file: &Path) -> Result<Connection, StoreError> {
    let io_error = |err| StoreError::Io { path: file.to_path_buf(), err };
    if let Some(dir) = file.parent() {
        crate::clients::create_private_dir(dir).map_err(io_error)?;
    }
    create_private(file).map_err(io_error)?;
    let mut db = Connection::open(file)?;
    // The daemon is the only writer; the wait covers a second core starting at the same time.
    db.busy_timeout(Duration::from_secs(5))?;
    let _mode: String = db.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    migrate(&mut db)?;
    // SQLite gives its side files the mode of the database; tighten them anyway, in case the
    // database was looser before this version opened it. The directory was made 0700 above.
    for side in ["-wal", "-shm"] {
        let mut path = file.as_os_str().to_owned();
        path.push(side);
        let path = PathBuf::from(path);
        if path.exists() {
            make_private(&path).map_err(|err| StoreError::Io { path, err })?;
        }
    }
    Ok(db)
}

/// Creates the tables in a new file and brings an older one up to [`SCHEMA_VERSION`], step by
/// step in one transaction; a file of this version stays as it is, a newer one is refused.
fn migrate(db: &mut Connection) -> Result<(), StoreError> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::Newer(version));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let tx = db.transaction()?;
    if version < 1 {
        tx.execute_batch(SCHEMA_V1)?;
    }
    if version < 2 {
        tx.execute_batch(SCHEMA_V2)?;
        backfill(&tx)?;
    }
    tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    tx.commit()?;
    Ok(())
}

/// Fills the columns of version 2 from the JSON of every entry written by version 1.
fn backfill(db: &Connection) -> Result<(), StoreError> {
    let mut statement = db.prepare("SELECT data FROM items")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    for data in rows {
        let item: Item = serde_json::from_str(&data?)?;
        update_item(db, &item)?;
    }
    Ok(())
}

/// Creates `file` if missing, readable and writable only by the owner on Unix, and makes an
/// existing one so.
fn create_private(file: &Path) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(file)?;
    make_private(file)
}

fn make_private(file: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = file;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use chrono::TimeZone;
    use torro_core::i18n::tr;
    use torro_inbox::{AgentKind, StateFilter};
    use torro_ipc::api::{Rights, RightsTemplate};

    use super::*;

    /// The inbox in `dir/state/inbox.sqlite`, nothing of the user's.
    pub(crate) fn inbox_in(dir: &Path) -> Inbox {
        Inbox::at(file_in(dir))
    }

    fn file_in(dir: &Path) -> PathBuf {
        dir.join("state").join(FILE)
    }

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 12, hour, minute, 0).single().expect("valid time")
    }

    fn client(id: &str, kind: &str, rights: Rights) -> ClientInfo {
        ClientInfo {
            id: id.into(),
            name: format!("{kind} {id}"),
            kind: kind.into(),
            created: at(0, 0),
            last_used: None,
            rights,
            agent: (kind == HOOK_CLIENT).then_some(AgentKind::ClaudeCode),
        }
    }

    /// A script client `backup` that sees every source.
    fn script() -> ClientInfo {
        client("aa01", "script", RightsTemplate::FullAccess.rights())
    }

    /// The hook client of Claude Code.
    fn hook() -> ClientInfo {
        client("bb02", HOOK_CLIENT, Rights::hook())
    }

    /// The hook client of `agent`.
    fn hook_for(agent: AgentKind) -> ClientInfo {
        ClientInfo { id: format!("bb-{}", agent.id()), agent: Some(agent), ..hook() }
    }

    /// A client that sees no source at all.
    fn blind() -> ClientInfo {
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.inbox_sources = Selection::Only(Vec::new());
        client("cc03", "codex", rights)
    }

    fn notice(title: &str, thread_key: Option<&str>) -> Request {
        Request::PostItem {
            kind: Kind::Notice,
            title: title.into(),
            body: None,
            link: None,
            lane: None,
            thread_key: thread_key.map(String::from),
            agent: None,
        }
    }

    fn session(agent: AgentKind, id: &str) -> AgentInfo {
        AgentInfo {
            agent,
            session: id.into(),
            project: Some("TorroWorkplace".into()),
            branch: Some("feat/inbox".into()),
            pids: vec![42],
        }
    }

    fn agent_post(kind: Kind, thread_key: &str, agent: AgentKind) -> Request {
        Request::PostItem {
            kind,
            title: "ignored".into(),
            body: Some("  Claude needs your permission to use Bash  ".into()),
            link: Some(Link::Window),
            lane: None,
            thread_key: Some(thread_key.into()),
            agent: Some(session(agent, "s1")),
        }
    }

    fn posted(response: Response) -> String {
        match response {
            Response::Posted { id: Some(id) } => id,
            other => panic!("expected a posted id, got {other:?}"),
        }
    }

    fn items(response: Response) -> Vec<Item> {
        match response {
            Response::Items { items } => items,
            other => panic!("expected items, got {other:?}"),
        }
    }

    fn item(response: Response) -> Item {
        match response {
            Response::Item { item } => item,
            other => panic!("expected an item, got {other:?}"),
        }
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn all() -> Request {
        Request::ListItems { lane: None, source: None, state: None, since: None, limit: None }
    }

    /// Files `item` as the core would (E2.1), for the entries no request may create.
    fn file_from_core(inbox: &Inbox, item: &Item) {
        inbox.with(|db| insert_item(db, "core", item)).expect("filed");
    }

    fn approval(now: DateTime<Utc>) -> Item {
        let (lane, urgent) = rules::placement(Kind::Approval, None);
        Item {
            id: "approval-1".into(),
            source: SourceRef::suite(),
            thread_key: "approval:1".into(),
            kind: Kind::Approval,
            lane,
            urgent,
            title: "Claude Desktop wants to delete an event".into(),
            body: None,
            from: None,
            agent: None,
            received: now,
            updated: now,
            count: 1,
            state: State::New,
            link: None,
            actions: Vec::new(),
            untrusted: true,
        }
    }

    #[test]
    fn a_post_becomes_an_untrusted_entry_of_the_posters_source() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let id = posted(inbox.handle(notice("  Backup finished  ", None), Some(&script()), at(8, 0)));

        let entry = item(inbox.handle(Request::GetItem { id: id.clone() }, None, at(8, 1)));
        assert_eq!(entry.source, SourceRef::script("script aa01"));
        assert_eq!((entry.kind, entry.lane, entry.urgent), (Kind::Notice, Lane::Info, false));
        assert_eq!(entry.title, "Backup finished");
        assert_eq!((entry.count, entry.state, entry.received), (1, State::New, at(8, 0)));
        assert!(entry.untrusted);
        assert_eq!(entry.thread_key, format!("item:{id}"), "a thread of its own");

        let by_app = posted(inbox.handle(notice("From the shell", None), None, at(8, 2)));
        assert_eq!(
            item(inbox.handle(Request::GetItem { id: by_app }, None, at(8, 3))).source,
            SourceRef::script("cli")
        );
        assert_eq!(code(&inbox.handle(notice(" ", None), None, at(8, 4))), "error", "no title");
    }

    #[test]
    fn reports_of_one_thread_merge_but_only_per_client() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let first = posted(inbox.handle(notice("Build failed", Some("build:main")), Some(&script()), at(8, 0)));
        let second = posted(inbox.handle(notice("Build failed again", Some("build:main")), Some(&script()), at(9, 0)));
        assert_eq!(first, second);
        let merged = item(inbox.handle(Request::GetItem { id: first.clone() }, None, at(9, 1)));
        assert_eq!(
            (merged.count, merged.title.as_str(), merged.updated, merged.received),
            (2, "Build failed again", at(9, 0), at(8, 0))
        );

        // Another client with the same key opens its own entry (security rule 2).
        let other = client("dd04", "script", RightsTemplate::FullAccess.rights());
        let foreign = posted(inbox.handle(notice("Mine", Some("build:main")), Some(&other), at(9, 2)));
        assert_ne!(foreign, first);
        assert_eq!(items(inbox.handle(all(), None, at(9, 3))).len(), 2);
    }

    #[test]
    fn done_entries_open_again_by_hand_or_by_a_new_report() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let id = posted(inbox.handle(notice("Deploy failed", Some("deploy")), Some(&script()), at(8, 0)));
        let done = item(inbox.handle(Request::SetItemDone { id: id.clone(), done: true }, Some(&script()), at(8, 5)));
        assert_eq!(done.state, State::Done { at: at(8, 5) });
        let listed = |now| {
            let open = Some(StateFilter::Open);
            let request = Request::ListItems { lane: None, source: None, state: open, since: None, limit: None };
            items(inbox.handle(request, None, now))
        };
        assert!(listed(at(8, 6)).is_empty());

        let again = item(inbox.handle(Request::SetItemDone { id: id.clone(), done: false }, None, at(8, 7)));
        assert_eq!(again.state, State::Seen);
        inbox.handle(Request::SetItemDone { id: id.clone(), done: true }, None, at(8, 8));
        posted(inbox.handle(notice("Deploy failed again", Some("deploy")), Some(&script()), at(9, 0)));
        assert_eq!(listed(at(9, 1)).iter().map(|i| (i.id.clone(), i.state)).collect::<Vec<_>>(), [(id, State::New)]);
    }

    #[test]
    fn snoozed_entries_wake_at_their_time() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let id = posted(inbox.handle(notice("Nightly build is flaky", None), Some(&script()), at(8, 0)));
        let snoozed =
            item(inbox.handle(Request::SnoozeItem { id: id.clone(), until: Some(at(13, 0)) }, None, at(8, 1)));
        assert_eq!(snoozed.state, State::Snoozed { until: at(13, 0) });

        assert_eq!(inbox.tend(at(12, 59), false), Ok((0, 0)));
        assert_eq!(item(inbox.handle(Request::GetItem { id: id.clone() }, None, at(12, 59))).state, snoozed.state);
        assert_eq!(inbox.tend(at(13, 0), false), Ok((1, 0)));
        let woken = item(inbox.handle(Request::GetItem { id: id.clone() }, None, at(13, 1)));
        assert_eq!((woken.state, woken.updated), (State::New, at(13, 0)));

        // Listing wakes too, without waiting for the daemon.
        inbox.handle(Request::SnoozeItem { id: id.clone(), until: Some(at(14, 0)) }, None, at(13, 2));
        let listed = items(inbox.handle(all(), None, at(14, 0)));
        assert_eq!(listed[0].state, State::New);
        let back = item(inbox.handle(Request::SnoozeItem { id: id.clone(), until: Some(at(20, 0)) }, None, at(14, 1)));
        assert_eq!(back.state, State::Snoozed { until: at(20, 0) });
        assert_eq!(item(inbox.handle(Request::SnoozeItem { id, until: None }, None, at(14, 2))).state, State::Seen);
    }

    #[test]
    fn done_entries_go_after_30_days_when_the_daemon_cleans_up() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let id = posted(inbox.handle(notice("Old", None), None, at(8, 0)));
        inbox.handle(Request::SetItemDone { id: id.clone(), done: true }, None, at(8, 0));
        let later = at(8, 0) + TimeDelta::days(KEEP_DONE_DAYS);
        assert_eq!(inbox.tend(later, false), Ok((0, 0)), "only when asked to clean up");
        assert_eq!(inbox.tend(later - TimeDelta::seconds(1), true), Ok((0, 0)));
        assert_eq!(inbox.tend(later, true), Ok((0, 1)));
        assert_eq!(code(&inbox.handle(Request::GetItem { id }, None, later)), "not-found");
    }

    #[test]
    fn a_restart_keeps_entries_and_rules() {
        let dir = tempfile::tempdir().expect("temp dir");
        let id = {
            let inbox = inbox_in(dir.path());
            let id = posted(inbox.handle(notice("Keep me", Some("k")), Some(&script()), at(8, 0)));
            inbox.handle(Request::SetItemLane { id: id.clone(), lane: Lane::ForMe, remember: true }, None, at(8, 1));
            inbox.handle(Request::MuteSource { source: SourceRef::script("noisy"), muted: true }, None, at(8, 2));
            id
        };
        let inbox = inbox_in(dir.path());
        let kept = item(inbox.handle(Request::GetItem { id: id.clone() }, None, at(9, 0)));
        assert_eq!((kept.title.as_str(), kept.lane), ("Keep me", Lane::ForMe));
        let Response::Rules { rules } = inbox.handle(Request::ListRules, None, at(9, 0)) else {
            panic!("rules answer with Rules");
        };
        assert_eq!(rules.len(), 2);
        // The thread is still the client's own after the restart.
        assert_eq!(posted(inbox.handle(notice("Again", Some("k")), Some(&script()), at(9, 1))), id);
    }

    #[test]
    fn the_file_is_for_the_owner_only_and_says_its_schema() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        posted(inbox.handle(notice("Hello", None), None, at(8, 0)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let state = dir.path().join("state");
            for path in [file_in(dir.path()), state.join(format!("{FILE}-wal")), state.join(format!("{FILE}-shm"))] {
                let mode = fs::metadata(&path).expect("exists").permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "{}", path.display());
            }
            assert_eq!(fs::metadata(&state).expect("dir").permissions().mode() & 0o777, 0o700);
        }
        let db = Connection::open(file_in(dir.path())).expect("opens");
        let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0)).expect("version");
        let journal: String = db.query_row("PRAGMA journal_mode", [], |row| row.get(0)).expect("journal");
        assert_eq!((version, journal.as_str()), (SCHEMA_VERSION, "wal"));
        db.execute_batch("PRAGMA user_version = 99").expect("written");
        drop(db);
        let newer = inbox_in(dir.path());
        let refused = newer.handle(all(), None, at(8, 1));
        assert_eq!(code(&refused), "error", "a newer file is left alone");
        let Response::Error { message, .. } = refused else { panic!("an error") };
        let home = dir.path().to_string_lossy();
        assert!(!message.contains(home.as_ref()) && !message.contains(FILE), "no path to clients: {message}");
    }

    #[test]
    fn a_file_of_schema_1_is_brought_up_to_date() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = file_in(dir.path());
        fs::create_dir_all(file.parent().expect("dir")).expect("dir");
        let mut old = approval(at(7, 0));
        old.id = "old".into();
        {
            let db = Connection::open(&file).expect("opens");
            db.execute_batch(SCHEMA_V1).expect("tables");
            db.execute_batch("PRAGMA user_version = 1").expect("version");
            db.execute(
                "INSERT INTO items (id, owner, thread_key, source, state, received, data)
                 VALUES ('old', 'core', 'approval:1', 'suite', 'new', ?1, ?2)",
                params![stamp(old.received), serde_json::to_string(&old).expect("json")],
            )
            .expect("row");
        }
        let inbox = inbox_in(dir.path());
        posted(inbox.handle(notice("newer", None), None, at(8, 0)));
        let for_me =
            Request::ListItems { lane: Some(Lane::ForMe), source: None, state: None, since: None, limit: None };
        assert_eq!(items(inbox.handle(for_me, None, at(9, 0))).into_iter().map(|i| i.id).collect::<Vec<_>>(), ["old"]);
        let info = |limit| Request::ListItems { lane: None, source: None, state: None, since: None, limit };
        let ordered = items(inbox.handle(info(Some(1)), None, at(9, 0)));
        assert_eq!(ordered[0].id, "old", "urgent first although older");
        let db = Connection::open(&file).expect("opens");
        let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0)).expect("version");
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn a_client_without_the_source_finds_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let id = posted(inbox.handle(notice("Secret", None), Some(&script()), at(8, 0)));
        let blind = blind();
        let source = SourceRef::script("script aa01");

        assert!(items(inbox.handle(all(), Some(&blind), at(8, 1))).is_empty());
        let named =
            Request::ListItems { lane: None, source: Some(source.clone()), state: None, since: None, limit: None };
        assert!(items(inbox.handle(named, Some(&blind), at(8, 1))).is_empty());
        assert_eq!(inbox.handle(Request::ListSources, Some(&blind), at(8, 1)), Response::Sources { sources: vec![] });
        for request in [
            Request::GetItem { id: id.clone() },
            Request::SetItemDone { id: id.clone(), done: true },
            Request::SnoozeItem { id: id.clone(), until: None },
            Request::SetItemLane { id: id.clone(), lane: Lane::ForMe, remember: false },
            Request::MuteSource { source: source.clone(), muted: true },
        ] {
            assert_eq!(code(&inbox.handle(request.clone(), Some(&blind), at(8, 2))), "not-found", "{request:?}");
            assert_eq!(
                inbox.refusal(&request, Some(&blind)).as_ref().map(code),
                match request {
                    Request::GetItem { .. } => None,
                    _ => Some("not-found"),
                }
            );
        }

        // Seeing just that source is enough.
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.inbox_sources = Selection::Only(vec![source.key()]);
        let reader = client("ee05", "claude-code", rights);
        assert_eq!(items(inbox.handle(all(), Some(&reader), at(8, 3))).len(), 1);
        let Response::Sources { sources } = inbox.handle(Request::ListSources, Some(&reader), at(8, 3)) else {
            panic!("sources answer with Sources");
        };
        assert_eq!(sources, [SourceInfo { source, open: 1, muted: false }]);
    }

    #[test]
    fn a_hook_reports_and_resolves_but_reads_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let hook = hook();
        let id = posted(inbox.handle(
            agent_post(Kind::AgentWaiting, "claude-code:s1", AgentKind::ClaudeCode),
            Some(&hook),
            at(8, 0),
        ));

        assert_eq!(code(&inbox.handle(all(), Some(&hook), at(8, 1))), "not-found");
        assert_eq!(code(&inbox.handle(Request::GetItem { id: id.clone() }, Some(&hook), at(8, 1))), "not-found");
        assert_eq!(inbox.handle(Request::ListSources, Some(&hook), at(8, 1)), Response::Sources { sources: vec![] });
        assert_eq!(
            code(&inbox.handle(Request::SetItemDone { id: id.clone(), done: true }, Some(&hook), at(8, 1))),
            "not-found"
        );

        let waiting = item(inbox.handle(Request::GetItem { id: id.clone() }, None, at(8, 2)));
        assert_eq!(waiting.source, SourceRef::agent("claude-code"));
        assert_eq!((waiting.kind, waiting.lane, waiting.urgent), (Kind::AgentWaiting, Lane::ForMe, true));
        assert_eq!(waiting.title, trf("{agent} is waiting for your permission", &[("agent", &"Claude Code")]));
        assert_eq!(waiting.body.as_deref(), Some("Claude needs your permission to use Bash"));
        assert_eq!((waiting.link, waiting.agent.map(|a| a.pids)), (Some(Link::Window), Some(vec![42])));
        assert!(waiting.untrusted, "the body is the agent's text");

        // `Stop` replaces the waiting entry of its session.
        posted(inbox.handle(
            agent_post(Kind::AgentDone, "claude-code:s1", AgentKind::ClaudeCode),
            Some(&hook),
            at(8, 3),
        ));
        let finished = item(inbox.handle(Request::GetItem { id: id.clone() }, None, at(8, 4)));
        assert_eq!((finished.kind, finished.urgent, finished.count), (Kind::AgentDone, false, 2));
        assert_eq!(finished.title, trf("{agent} finished", &[("agent", &"Claude Code")]));

        assert_eq!(
            inbox.handle(Request::ResolveItems { thread_key: "claude-code:s1".into() }, Some(&hook), at(8, 5)),
            Response::Ok
        );
        assert_eq!(item(inbox.handle(Request::GetItem { id }, None, at(8, 6))).state, State::Done { at: at(8, 5) });
    }

    #[test]
    fn a_client_cannot_resolve_a_foreign_thread() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let id = posted(inbox.handle(notice("Mine", Some("build:main")), Some(&script()), at(8, 0)));
        let other = client("dd04", "script", RightsTemplate::FullAccess.rights());
        assert_eq!(
            inbox.handle(Request::ResolveItems { thread_key: "build:main".into() }, Some(&other), at(8, 1)),
            Response::Ok
        );
        assert_eq!(
            inbox.handle(Request::ResolveItems { thread_key: "build:main".into() }, None, at(8, 1)),
            Response::Ok
        );
        assert_eq!(
            item(inbox.handle(Request::GetItem { id: id.clone() }, None, at(8, 2))).state,
            State::New,
            "untouched"
        );
        inbox.handle(Request::ResolveItems { thread_key: "build:main".into() }, Some(&script()), at(8, 3));
        assert_eq!(item(inbox.handle(Request::GetItem { id }, None, at(8, 4))).state, State::Done { at: at(8, 3) });
    }

    #[test]
    fn only_new_threads_are_throttled() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let script = script();
        for n in 0..POSTS_PER_HOUR {
            posted(inbox.handle(notice("Spam", Some(&format!("t{n}"))), Some(&script), at(8, 0)));
        }
        assert_eq!(code(&inbox.handle(notice("One more", Some("new")), Some(&script), at(8, 30))), "busy");
        assert_eq!(code(&inbox.handle(notice("One more", None), Some(&script), at(8, 30))), "busy");
        posted(inbox.handle(notice("Merged", Some("t0")), Some(&script), at(8, 31)));
        assert_eq!(
            inbox.handle(Request::ResolveItems { thread_key: "t1".into() }, Some(&script), at(8, 31)),
            Response::Ok
        );
        // Others are not held up, and an hour later it goes on.
        posted(inbox.handle(
            notice("Other", None),
            Some(&client("dd04", "script", RightsTemplate::FullAccess.rights())),
            at(8, 31),
        ));
        posted(inbox.handle(notice("Later", Some("new")), Some(&script), at(9, 0)));

        let hook = hook_for(AgentKind::Codex);
        for n in 0..HOOK_POSTS_PER_HOUR {
            posted(inbox.handle(
                agent_post(Kind::AgentDone, &format!("codex:{n}"), AgentKind::Codex),
                Some(&hook),
                at(10, 0),
            ));
        }
        assert_eq!(
            code(&inbox.handle(agent_post(Kind::AgentDone, "codex:x", AgentKind::Codex), Some(&hook), at(10, 1))),
            "busy"
        );
    }

    #[test]
    fn titles_and_bodies_are_cut_at_a_character() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let title = "ä".repeat(TITLE_MAX + 50);
        let body = "€".repeat(BODY_MAX + 1);
        let request = Request::PostItem {
            kind: Kind::Notice,
            title,
            body: Some(body),
            link: None,
            lane: None,
            thread_key: None,
            agent: None,
        };
        let id = posted(inbox.handle(request, None, at(8, 0)));
        let entry = item(inbox.handle(Request::GetItem { id }, None, at(8, 1)));
        assert_eq!(entry.title.chars().count(), TITLE_MAX);
        assert!(entry.title.ends_with("ä…"));
        let body = entry.body.expect("body");
        assert_eq!(body.chars().count(), BODY_MAX);
        assert!(body.ends_with('…'));
        assert_eq!(cap("short", 200), "short");
        assert_eq!(cap("abc", 3), "abc");
        assert_eq!(cap("abcd", 3), "ab…");
    }

    #[test]
    fn nobody_but_the_core_files_approvals_and_agent_states_need_a_hook() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let script = script();
        for kind in [Kind::Approval, Kind::AccountProblem] {
            let request = Request::PostItem {
                kind,
                title: "Allow me".into(),
                body: None,
                link: None,
                lane: None,
                thread_key: None,
                agent: None,
            };
            assert_eq!(code(&inbox.handle(request.clone(), Some(&script), at(8, 0))), "forbidden");
            assert_eq!(code(&inbox.handle(request.clone(), Some(&hook()), at(8, 0))), "forbidden");
            assert_eq!(code(&inbox.handle(request.clone(), None, at(8, 0))), "forbidden");
            assert_eq!(inbox.refusal(&request, Some(&script)).as_ref().map(code), Some("forbidden"));
        }
        let id = posted(inbox.handle(agent_post(Kind::AgentWaiting, "s", AgentKind::Codex), Some(&script), at(8, 1)));
        let entry = item(inbox.handle(Request::GetItem { id }, None, at(8, 2)));
        assert_eq!((entry.kind, entry.urgent, entry.lane), (Kind::Notice, false, Lane::Info));
        assert_eq!(entry.title, "ignored", "the poster's own title");
        assert_eq!((entry.agent, entry.link), (None, None), "no session, no window from a non-hook");
    }

    #[test]
    fn approvals_and_the_suite_belong_to_the_person() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        file_from_core(&inbox, &approval(at(8, 0)));
        let script = script();
        let id = "approval-1".to_string();

        assert_eq!(
            item(inbox.handle(Request::GetItem { id: id.clone() }, Some(&script), at(8, 1))).kind,
            Kind::Approval
        );
        for request in [
            Request::SetItemDone { id: id.clone(), done: true },
            Request::SnoozeItem { id: id.clone(), until: Some(at(9, 0)) },
            Request::SetItemLane { id: id.clone(), lane: Lane::Info, remember: false },
            Request::MuteSource { source: SourceRef::suite(), muted: true },
        ] {
            assert_eq!(code(&inbox.handle(request.clone(), Some(&script), at(8, 2))), "forbidden", "{request:?}");
            assert_eq!(inbox.refusal(&request, Some(&script)).as_ref().map(code), Some("forbidden"));
            assert_eq!(inbox.refusal(&request, None), None, "the app key is never refused");
        }
        assert_eq!(item(inbox.handle(Request::GetItem { id: id.clone() }, None, at(8, 3))).state, State::New);

        assert_eq!(
            item(inbox.handle(Request::SetItemDone { id, done: true }, None, at(8, 4))).state,
            State::Done { at: at(8, 4) }
        );
        assert_eq!(
            inbox.handle(Request::MuteSource { source: SourceRef::suite(), muted: true }, None, at(8, 5)),
            Response::Ok
        );
        let Response::Rules { rules } = inbox.handle(Request::ListRules, Some(&script), at(8, 6)) else {
            panic!("rules answer with Rules");
        };
        let remove = Request::RemoveRule { id: rules[0].id.clone() };
        assert_eq!(code(&inbox.handle(remove.clone(), Some(&script), at(8, 7))), "forbidden");
        assert_eq!(inbox.handle(remove, None, at(8, 7)), Response::Ok);
    }

    #[test]
    fn muted_sources_drop_new_reports_until_unmuted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let script = script();
        let source = SourceRef::script("script aa01");
        let id = posted(inbox.handle(notice("Before", Some("t")), Some(&script), at(8, 0)));
        assert_eq!(
            inbox.handle(Request::MuteSource { source: source.clone(), muted: true }, Some(&script), at(8, 1)),
            Response::Ok
        );
        assert_eq!(
            inbox.handle(Request::MuteSource { source: source.clone(), muted: true }, None, at(8, 2)),
            Response::Ok
        );

        assert_eq!(inbox.handle(notice("Dropped", None), Some(&script), at(8, 3)), Response::Posted { id: None });
        assert_eq!(
            inbox.handle(notice("Not merged", Some("t")), Some(&script), at(8, 3)),
            Response::Posted { id: None }
        );
        assert_eq!(item(inbox.handle(Request::GetItem { id: id.clone() }, None, at(8, 4))).count, 1, "the entry stays");
        let Response::Sources { sources } = inbox.handle(Request::ListSources, None, at(8, 4)) else {
            panic!("sources answer with Sources");
        };
        assert_eq!(sources, [SourceInfo { source: source.clone(), open: 1, muted: true }]);

        assert_eq!(inbox.handle(Request::MuteSource { source, muted: false }, Some(&script), at(8, 5)), Response::Ok);
        assert_eq!(posted(inbox.handle(notice("After", Some("t")), Some(&script), at(8, 6))), id);
    }

    #[test]
    fn a_remembered_lane_places_later_notices() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let script = script();
        let first = posted(inbox.handle(notice("CI red", None), Some(&script), at(8, 0)));
        let moved =
            item(inbox.handle(Request::SetItemLane { id: first, lane: Lane::ForMe, remember: true }, None, at(8, 1)));
        assert_eq!(moved.lane, Lane::ForMe);

        let next = posted(inbox.handle(notice("CI red again", None), Some(&script), at(8, 2)));
        let placed = item(inbox.handle(Request::GetItem { id: next.clone() }, None, at(8, 3)));
        assert_eq!((placed.lane, placed.urgent), (Lane::ForMe, false), "never urgent");

        // Remembering another lane replaces the rule.
        inbox.handle(Request::SetItemLane { id: next, lane: Lane::Info, remember: true }, None, at(8, 4));
        let Response::Rules { rules } = inbox.handle(Request::ListRules, None, at(8, 5)) else {
            panic!("rules answer with Rules");
        };
        assert_eq!(rules.iter().map(|rule| rule.effect).collect::<Vec<_>>(), [RuleEffect::Lane { lane: Lane::Info }]);
        let wanted = Request::PostItem {
            kind: Kind::Notice,
            title: "Wants for me".into(),
            body: None,
            link: None,
            lane: Some(Lane::ForMe),
            thread_key: None,
            agent: None,
        };
        let id = posted(inbox.handle(wanted, Some(&script), at(8, 6)));
        assert_eq!(
            item(inbox.handle(Request::GetItem { id }, None, at(8, 7))).lane,
            Lane::Info,
            "the person's rule wins"
        );

        assert_eq!(code(&inbox.handle(Request::RemoveRule { id: "nope".into() }, None, at(8, 8))), "not-found");
        assert_eq!(
            inbox.handle(Request::RemoveRule { id: rules[0].id.clone() }, Some(&script), at(8, 8)),
            Response::Ok
        );
        assert_eq!(inbox.handle(Request::ListRules, None, at(8, 9)), Response::Rules { rules: vec![] });
    }

    #[test]
    fn the_list_follows_the_filter_and_the_order() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let for_me = |title: &str| Request::PostItem {
            kind: Kind::Notice,
            title: title.into(),
            body: None,
            link: None,
            lane: Some(Lane::ForMe),
            thread_key: None,
            agent: None,
        };
        posted(inbox.handle(for_me("older"), None, at(8, 0)));
        posted(inbox.handle(notice("info", None), None, at(8, 1)));
        posted(inbox.handle(for_me("newer"), None, at(8, 2)));
        let gemini = hook_for(AgentKind::Gemini);
        posted(inbox.handle(agent_post(Kind::AgentWaiting, "w", AgentKind::Gemini), Some(&gemini), at(7, 0)));
        let titles =
            |request| items(inbox.handle(request, None, at(9, 0))).into_iter().map(|i| i.title).collect::<Vec<_>>();
        let waiting = trf("{agent} is waiting for your permission", &[("agent", &"Gemini CLI")]);
        assert_eq!(
            titles(Request::ListItems { lane: Some(Lane::ForMe), source: None, state: None, since: None, limit: None }),
            [waiting.as_str(), "newer", "older"]
        );
        assert_eq!(
            titles(Request::ListItems { lane: None, source: None, state: None, since: Some(at(8, 1)), limit: Some(1) }),
            ["newer"]
        );
        assert_eq!(tr("Suite"), SourceRef::suite().label(), "texts go through tr()");
    }

    #[test]
    fn a_hook_reports_for_its_own_agent_only_so_muting_holds() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let hook = hook();
        let disguised = agent_post(Kind::AgentWaiting, "codex:s1", AgentKind::Codex);
        assert_eq!(code(&inbox.handle(disguised.clone(), Some(&hook), at(8, 0))), "forbidden");
        assert_eq!(inbox.refusal(&disguised, Some(&hook)).as_ref().map(code), Some("forbidden"));

        // Without agent info the report still goes under the hook's own agent, with its title.
        let bare = Request::PostItem {
            kind: Kind::AgentDone,
            title: "Codex finished".into(),
            body: None,
            link: None,
            lane: None,
            thread_key: Some("s2".into()),
            agent: None,
        };
        let id = posted(inbox.handle(bare.clone(), Some(&hook), at(8, 1)));
        let entry = item(inbox.handle(Request::GetItem { id }, None, at(8, 2)));
        assert_eq!(entry.source, SourceRef::agent("claude-code"));
        assert_eq!(entry.title, trf("{agent} finished", &[("agent", &"Claude Code")]));

        let mute = Request::MuteSource { source: SourceRef::agent("claude-code"), muted: true };
        assert_eq!(inbox.handle(mute, None, at(8, 3)), Response::Ok);
        assert_eq!(inbox.handle(bare, Some(&hook), at(8, 4)), Response::Posted { id: None });

        let unbound = ClientInfo { agent: None, ..hook };
        assert_eq!(code(&inbox.handle(notice("Hi", None), Some(&unbound), at(8, 5))), "forbidden");
    }

    #[test]
    fn posts_keep_to_their_limits() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let keyed = |key: String| notice("Keyed", Some(&key));
        posted(inbox.handle(keyed("k".repeat(THREAD_KEY_MAX)), None, at(8, 0)));
        assert_eq!(code(&inbox.handle(keyed("k".repeat(THREAD_KEY_MAX + 1)), None, at(8, 0))), "error");
        let resolve = Request::ResolveItems { thread_key: "k".repeat(THREAD_KEY_MAX + 1) };
        assert_eq!(code(&inbox.handle(resolve, None, at(8, 0))), "error");

        let linked = |link: Link| Request::PostItem {
            kind: Kind::Notice,
            title: "Linked".into(),
            body: None,
            link: Some(link),
            lane: None,
            thread_key: None,
            agent: None,
        };
        let long = format!("https://example.org/{}", "a".repeat(URL_MAX));
        assert_eq!(code(&inbox.handle(linked(Link::Url { url: long }), None, at(8, 1))), "error");

        let mut agent = session(AgentKind::ClaudeCode, &"s".repeat(AGENT_FIELD_MAX + 10));
        agent.project = Some("ä".repeat(AGENT_FIELD_MAX));
        agent.branch = Some("b".repeat(AGENT_FIELD_MAX + 1));
        agent.pids = (1..=40).collect();
        let request = Request::PostItem {
            kind: Kind::AgentWaiting,
            title: "ignored".into(),
            body: None,
            link: None,
            lane: None,
            thread_key: Some("claude-code:long".into()),
            agent: Some(agent),
        };
        let id = posted(inbox.handle(request, Some(&hook()), at(8, 2)));
        let stored = item(inbox.handle(Request::GetItem { id }, None, at(8, 3))).agent.expect("agent");
        assert_eq!(stored.session.len(), AGENT_FIELD_MAX);
        let project = stored.project.expect("project");
        assert!(project.len() <= AGENT_FIELD_MAX && project.chars().all(|c| c == 'ä'), "cut at a character");
        assert_eq!(stored.branch.map(|b| b.len()), Some(AGENT_FIELD_MAX));
        assert_eq!(stored.pids, (1..=16).collect::<Vec<u32>>());
    }

    #[test]
    fn only_links_that_are_safe_to_open_are_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let link_of = |link: Link| {
            let request = Request::PostItem {
                kind: Kind::Notice,
                title: "Linked".into(),
                body: None,
                link: Some(link),
                lane: None,
                thread_key: None,
                agent: None,
            };
            let id = posted(inbox.handle(request, Some(&script()), at(8, 0)));
            item(inbox.handle(Request::GetItem { id }, None, at(8, 1))).link
        };
        let url = |url: &str| Link::Url { url: url.into() };
        assert_eq!(link_of(url("https://ci.example.org/run/1")), Some(url("https://ci.example.org/run/1")));
        assert_eq!(link_of(url("HTTP://example.org")), Some(url("HTTP://example.org")));
        for unsafe_url in ["file:///etc/passwd", "javascript:alert(1)", "ftp://example.org", "example.org"] {
            assert_eq!(link_of(url(unsafe_url)), None, "{unsafe_url}");
        }
        let app = |app: &str| Link::App { app: app.into() };
        assert_eq!(link_of(app("torrotasks")), Some(app("torrotasks")));
        assert_eq!(link_of(app("torroworkplace")), Some(app("torroworkplace")));
        assert_eq!(link_of(app("rm -rf")), None);
        assert_eq!(link_of(Link::Window), None, "no window from a non-hook");
    }

    #[test]
    fn only_the_poster_and_the_app_key_see_a_thread_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let poster = script();
        let id = posted(inbox.handle(notice("Build failed", Some("build:secret-branch")), Some(&poster), at(8, 0)));
        let reader = client("dd04", "codex", RightsTemplate::FullAccess.rights());

        let get = || Request::GetItem { id: id.clone() };
        assert_eq!(item(inbox.handle(get(), Some(&poster), at(8, 1))).thread_key, "build:secret-branch");
        assert_eq!(item(inbox.handle(get(), None, at(8, 1))).thread_key, "build:secret-branch");
        assert_eq!(item(inbox.handle(get(), Some(&reader), at(8, 1))).thread_key, "");
        assert_eq!(items(inbox.handle(all(), Some(&reader), at(8, 1)))[0].thread_key, "");
        let done = Request::SetItemDone { id: id.clone(), done: true };
        assert_eq!(item(inbox.handle(done, Some(&reader), at(8, 2))).thread_key, "");
        // Hiding it never touches what is stored.
        assert_eq!(item(inbox.handle(get(), Some(&poster), at(8, 3))).thread_key, "build:secret-branch");
    }

    #[test]
    fn a_hook_lists_no_rules_and_no_sources() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let hook = hook();
        posted(inbox.handle(agent_post(Kind::AgentWaiting, "s1", AgentKind::ClaudeCode), Some(&hook), at(8, 0)));
        let mute = Request::MuteSource { source: SourceRef::agent("claude-code"), muted: true };
        assert_eq!(inbox.handle(mute, None, at(8, 1)), Response::Ok);
        assert_eq!(inbox.handle(Request::ListRules, Some(&hook), at(8, 2)), Response::Rules { rules: vec![] });
        assert_eq!(inbox.handle(Request::ListSources, Some(&hook), at(8, 2)), Response::Sources { sources: vec![] });
    }

    #[test]
    fn facts_for_an_approval_come_from_the_inbox() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let id = posted(inbox.handle(notice("CI red", None), Some(&script()), at(8, 0)));
        let lane = Request::SetItemLane { id: id.clone(), lane: Lane::ForMe, remember: true };
        let facts = inbox.facts(&lane);
        assert_eq!(facts.item.map(|item| (item.id, item.title)), Some((id.clone(), "CI red".to_string())));
        inbox.handle(lane, None, at(8, 1));
        let Response::Rules { rules } = inbox.handle(Request::ListRules, None, at(8, 2)) else {
            panic!("rules answer with Rules");
        };
        let remove = Request::RemoveRule { id: rules[0].id.clone() };
        assert_eq!(inbox.facts(&remove).rule.as_ref(), rules.first());
        assert_eq!(inbox.facts(&Request::RemoveRule { id: "nope".into() }), InboxFacts::default());
        assert_eq!(inbox.facts(&Request::ListSources), InboxFacts::default());
    }

    #[test]
    fn without_a_file_every_request_fails_instead_of_touching_the_home() {
        let inbox = Inbox::system();
        assert_eq!(code(&inbox.handle(all(), None, at(8, 0))), "error");
        assert!(inbox.tend(at(8, 0), true).is_err());
    }
}
