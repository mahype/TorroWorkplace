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
//!
//! **Opening an entry** (task E2.4, `inbox.open_item`) is split: the inbox decides what may be
//! opened ([`Inbox::opening`], security rule 6) and marks a new entry seen once it was
//! ([`Inbox::opened`]); the core carries it out with the window manager, the browser or the apps.
//! A hook's agent states link to the agent's window ([`Link::Window`]) when the hook sent the
//! processes it runs under; the window is looked up only when the entry is opened, because window
//! ids change. A chain of processes that passes a tmux server gets no such link
//! ([`behind_tmux`]): the agent's terminal window is not among them, so there would be nothing to
//! jump to and the app shows no "Go to window".
//!
//! **The source `suite`** (task E2.1) is filed by the core itself, not through a request: every
//! open approval ([`file_approvals`]) and every account that needs the user
//! ([`Inbox::file_account_problems`]) is one entry, owned by [`CORE_OWNER`], so no client and not
//! even the app key's own reports can merge into or resolve it. The core brings them in line with
//! what stands now ([`reconcile`]) after an approval opened or was decided, when it starts and
//! every [`SUITE_EVERY`] ([`tend_suite`]): that is how an expired approval, a fixed account or a
//! restart are caught up with. Only the app key changes these entries (security rule 1).
//!
//! **The tray** (task E3.3b) counts the open entries "for me" and lists the newest
//! ([`Inbox::for_me`]). It hears of every change through [`Inbox::changes`]: whatever writes to
//! the file (a post, a merge, resolving, done, snooze, lane, waking, the clean-up, the suite's
//! reconciliation, rules) marks it, so no write path can forget to and nothing polls.

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
use torro_core::i18n::{tr, trf};
use torro_core::paths::AppPaths;
use torro_inbox::rules::{self, KEEP_DONE_DAYS, Poster};
use torro_inbox::{
    Action, AgentInfo, AgentKind, Item, ItemFilter, Kind, Lane, Link, Rule, RuleEffect, SourceInfo, SourceRef, State,
    StateFilter,
};
use torro_ipc::api::{self, HOOK_CLIENT, InboxForbidden, Selection};
use torro_ipc::{ApprovalInfo, ClientInfo, Request, Response};

use crate::account::github::GithubAccounts;
use crate::approvals::{Approvals, InboxFacts};
use crate::pim::PimService;

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

/// The owner of the entries the core files itself (task E2.1): approvals and account problems.
/// Neither a client id nor [`APP_OWNER`], so no request merges into or resolves one of them.
const CORE_OWNER: &str = "core";

/// The start of the thread keys of approval entries, `approval:<approval id>`.
const APPROVAL_THREADS: &str = "approval:";

/// The start of the thread keys of account problems, `account:<account id>`: one entry per
/// account, whatever its problem.
const ACCOUNT_THREADS: &str = "account:";

/// How often the core brings the entries of the suite in line ([`tend_suite`]): an approval that
/// expired or an account that was fixed is done at most this long after.
pub const SUITE_EVERY: Duration = Duration::from_secs(60);

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
    /// Marked after every piece of work that wrote to the file ([`Inbox::with`]), for the tray.
    changed: tokio::sync::watch::Sender<()>,
}

/// The open entries "for me" as the tray shows them (task E3.3b, [`Inbox::for_me`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForMe {
    /// Open entries (new or seen; snoozed and done ones are not) in the lane "for me".
    pub open: usize,
    /// How many of them are urgent: approvals and agents waiting ([`rules::placement`]).
    pub urgent: usize,
    /// The most recently updated of them, newest first. Titles of foreign sources are
    /// `untrusted` text: whoever shows them must not let them read as markup or mnemonics.
    pub newest: Vec<Item>,
}

impl Inbox {
    /// The user's inbox: [`FILE`] in the core's state directory. Opens nothing until first used.
    /// In tests there is no file, so a test that forgets its own ([`Inbox::at`]) gets `error`
    /// rather than touching the user's state.
    pub fn system() -> Inbox {
        if cfg!(test) {
            return Inbox::in_file(None);
        }
        Inbox::in_file(AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join(FILE)))
    }

    fn in_file(file: Option<PathBuf>) -> Inbox {
        Inbox { file, db: Mutex::new(None), changed: tokio::sync::watch::channel(()).0 }
    }

    /// The inbox in `file`, created with its directory on first use. For tests; the core uses
    /// [`Inbox::system`].
    #[cfg(test)]
    pub(crate) fn at(file: PathBuf) -> Inbox {
        Inbox::in_file(Some(file))
    }

    /// Marked as changed each time something was written to the inbox, after it was stored: an
    /// entry posted, merged, resolved, done, snoozed, moved, woken or removed, the suite's entries
    /// brought in line, a rule kept or removed. Reading changes nothing, unless it woke a snoozed
    /// entry. The daemon refreshes the tray on it, whose count reads [`Inbox::for_me`] anew.
    pub fn changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.changed.subscribe()
    }

    /// The open entries "for me" at `now` as the app key sees them (everything), with the
    /// `newest` most recently updated, for the tray (task E3.3b). Wakes snoozed entries that are
    /// due first, as listing does. `Err` holds the message for the log.
    pub fn for_me(&self, now: DateTime<Utc>, newest: usize) -> Result<ForMe, String> {
        self.with(|db| {
            wake_due(db, now)?;
            let open = "lane = 'for_me' AND state IN ('new', 'seen')";
            let (count, urgent): (i64, i64) = db.query_row(
                &format!("SELECT COUNT(*), COALESCE(SUM(urgent), 0) FROM items WHERE {open}"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let limit = vec![Value::Integer(i64::try_from(newest).unwrap_or(i64::MAX))];
            let rows = read_items(db, &Scope::All, open, limit, " ORDER BY updated DESC, id LIMIT ?")?;
            Ok(ForMe {
                open: usize::try_from(count).unwrap_or_default(),
                urgent: usize::try_from(urgent).unwrap_or_default(),
                newest: rows.into_iter().map(|stored| stored.item).collect(),
            })
        })
        .map_err(|failure| failure.to_string())
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
            Request::OpenItem { id } => visible_item(db, id, Some(client)).map(drop),
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
                Request::OpenItem { id }
                | Request::SetItemDone { id, .. }
                | Request::SnoozeItem { id, .. }
                | Request::SetItemLane { id, .. } => {
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

    /// How to open the entry `id` for `client` (`None` for the app key), for `inbox.open_item`:
    /// what its link allows under security rule 6 ([`rules::may_open`]). Changes nothing.
    ///
    /// `Err` holds the answer: `not-found` for an unknown entry or one of a source the client may
    /// not see (a hook client sees none), `unsupported` when the entry has no link that may be
    /// opened, or a window link without processes to look for.
    pub fn opening(&self, id: &str, client: Option<&ClientInfo>, now: DateTime<Utc>) -> Result<Opening, Box<Response>> {
        self.with(|db| {
            wake_due(db, now)?;
            opening(&visible_item(db, id, client)?.item)
        })
        .map_err(|failure| Box::new(failure.response()))
    }

    /// The entry `id` was opened for `client` at `now`: a new one is seen from now on, if the
    /// client may change it (security rule 1; an agent opening an approval leaves it new). Nothing
    /// else changes, `updated` neither. A failure is only logged: the entry was opened anyway.
    pub fn opened(&self, id: &str, client: Option<&ClientInfo>, now: DateTime<Utc>) {
        let seen = self.with(|db| {
            wake_due(db, now)?;
            let stored = visible_item(db, id, client)?;
            if stored.item.state == State::New && api::check_item_change(&stored.item, client).is_ok() {
                let mut item = stored.item;
                item.state = State::Seen;
                update_item(db, &item)?;
            }
            Ok(())
        });
        if let Err(failure) = seen {
            eprintln!("torroworkplace: inbox: cannot mark an opened entry seen: {failure}");
        }
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

    /// Files the approvals in `open` (pending ones; others are skipped) as entries of the source
    /// `suite` at `now` and marks done the entries of approvals no longer among them (task E2.1):
    /// kind `approval`, urgent, "for me", with "Allow" and "Deny" that carry the approval's id,
    /// the summary the core wrote as title and `untrusted` (the summary quotes the client). One
    /// entry per approval (thread key `approval:<id>`); calling it again with the same approvals
    /// changes nothing. `open` must be all approvals open at `now`; the core passes them under the
    /// approvals' lock ([`file_approvals`]). `Err` holds the message for the log.
    pub fn file_approvals(&self, open: &[ApprovalInfo], now: DateTime<Utc>) -> Result<(), String> {
        let matters = open.iter().filter(|info| info.state.is_pending()).map(approval_matter).collect();
        self.with(|db| reconcile(db, APPROVAL_THREADS, matters, |_| true, now)).map_err(|f| f.to_string())
    }

    /// Files the accounts in `checks` that need the user as entries of the source `suite` at
    /// `now` and marks done the entries of accounts that are fine again (task E2.1): kind
    /// `account_problem`, "for me", not urgent, linking to the control centre. One entry per
    /// account (thread key `account:<id>`); a new problem of the same account changes its texts.
    ///
    /// An account whose state is [`Health::Unsure`] keeps its entry as it is, open or done. With
    /// `complete`, `checks` names every account the core has, and the entries of accounts missing
    /// from it (removed meanwhile) are done too; without, they stay, since a service that has not
    /// loaded its accounts yet would otherwise resolve them all. `Err` holds the message for the
    /// log.
    pub fn file_account_problems(
        &self,
        checks: &[AccountCheck],
        complete: bool,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let matters = checks
            .iter()
            .filter_map(|check| match check.health {
                Health::Problem { trouble, since } => Some(account_matter(check, trouble, since)),
                Health::Fine | Health::Unsure => None,
            })
            .collect();
        let health = |thread_key: &str| checks.iter().find(|check| check.thread_key() == thread_key).map(|c| c.health);
        // An entry without a problem is done when its account is fine, or gone from a complete list.
        let fixed = |thread_key: &str| match health(thread_key) {
            Some(health) => health == Health::Fine,
            None => complete,
        };
        self.with(|db| reconcile(db, ACCOUNT_THREADS, matters, fixed, now)).map_err(|f| f.to_string())
    }

    /// Runs `work` on the open database, opening it first if needed, and marks
    /// [`Inbox::changes`] when it wrote rows. SQLite counts every row inserted, updated or deleted
    /// on the connection, so each write path is covered without marking it by hand; work that
    /// failed and rolled back may mark it too, which costs only a needless refresh.
    fn with<T>(&self, work: impl FnOnce(&mut Connection) -> Result<T, Failure>) -> Result<T, Failure> {
        let mut db = self.lock();
        if db.is_none() {
            let file = self.file.as_deref().ok_or(StoreError::NoHome)?;
            *db = Some(open(file)?);
        }
        let Some(connection) = db.as_mut() else {
            return Err(StoreError::NoHome.into());
        };
        let before = connection.total_changes();
        let result = work(connection);
        let wrote = connection.total_changes() != before;
        drop(db);
        if wrote {
            self.changed.send_replace(());
        }
        result
    }

    fn lock(&self) -> MutexGuard<'_, Option<Connection>> {
        // A panic in a query leaves the connection usable; SQLite rolled back what was open.
        self.db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// What opening an entry does, as [`Inbox::opening`] decides it; the core carries it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opening {
    /// Bring the window of one of these processes to the front: the hook's parent processes,
    /// nearest first ([`AgentInfo::pids`]). The first that has a window is the agent's terminal.
    Window { pids: Vec<u32> },
    /// Open this `http`/`https` page in the system browser.
    Url(String),
    /// Start or raise this app of the suite.
    App(AppId),
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
    /// `unsupported`: nothing on the entry may be opened (`inbox.open_item`).
    Unsupported(String),
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
            Failure::Unsupported(message) => ("unsupported", message),
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
            Failure::NotFound(message)
            | Failure::Busy(message)
            | Failure::Unsupported(message)
            | Failure::Invalid(message) => f.write_str(message),
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
        // Opening needs the window manager, the browser or the apps: the core answers it with
        // `Inbox::opening` and `Inbox::opened`.
        Request::OpenItem { .. } => Err(Failure::Invalid("inbox.open_item is answered by the core".into())),
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
    // The window of a hook's agent is the core's to link, never the poster's (task E2.4).
    let link = match post.link {
        None | Some(Link::Window) => window_link(kind, agent.as_ref()),
        link => link,
    };
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
        link,
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

/// The link of a report of `kind` about `agent`: the agent's window for a hook's agent state
/// (only hooks keep `agent`, [`post`]) that names the processes it runs under, unless they pass a
/// tmux server ([`behind_tmux`]). `None` otherwise: without processes (macOS, Windows) there is
/// no window to look for, so the app offers none.
fn window_link(kind: Kind, agent: Option<&AgentInfo>) -> Option<Link> {
    let pids = &agent?.pids;
    let agent_state = matches!(kind, Kind::AgentWaiting | Kind::AgentDone);
    (agent_state && !pids.is_empty() && !behind_tmux(pids)).then_some(Link::Window)
}

/// One of `pids` is a tmux server (its process name starts with `tmux`). An agent in tmux runs
/// under the server, which belongs to no window; the terminal showing it runs a tmux client that
/// is not among the agent's parents, so its window cannot be found. Read from `/proc` while the
/// hook still runs and its parents live; a process that is gone counts as no tmux.
#[cfg(target_os = "linux")]
fn behind_tmux(pids: &[u32]) -> bool {
    pids.iter().any(|pid| fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|name| name.starts_with("tmux")))
}

/// Only Linux sends processes ([`AgentInfo::pids`]); elsewhere there is nothing to look at.
#[cfg(not(target_os = "linux"))]
fn behind_tmux(_pids: &[u32]) -> bool {
    false
}

/// What opening `item` does, as security rule 6 allows ([`rules::may_open`]): a web page, an app
/// of the suite, or the window of a hook's agent with processes to look for; else `unsupported`.
fn opening(item: &Item) -> Result<Opening, Failure> {
    let refused = |why: &str| Err(Failure::Unsupported(format!("inbox entry {:?}: {why}", item.id)));
    if !rules::may_open(item, is_app_of_the_suite) {
        return refused("nothing on it may be opened");
    }
    match &item.link {
        Some(Link::Url { url }) => Ok(Opening::Url(url.clone())),
        Some(Link::App { app }) => match AppId::ALL.iter().find(|known| known.binary == app) {
            Some(app) => Ok(Opening::App(*app)),
            None => refused("no app of the suite"),
        },
        Some(Link::Window) => match item.agent.as_ref().map(|agent| agent.pids.clone()) {
            Some(pids) if !pids.is_empty() => Ok(Opening::Window { pids }),
            _ => refused("the agent's processes are unknown"),
        },
        None => refused("it has no link"),
    }
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

/// How one account stands for the inbox (task E2.1), as its service sees it now
/// ([`PimService::account_checks`], [`GithubAccounts::account_checks`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountCheck {
    /// The account's id, as `accounts.list_accounts` names it.
    pub account: String,
    /// The name the user knows the account by: `anna@cloud.example.org`, a GitHub login.
    pub label: String,
    pub health: Health,
}

impl AccountCheck {
    /// The thread key of the account's entry.
    fn thread_key(&self) -> String {
        format!("{ACCOUNT_THREADS}{}", self.account)
    }
}

/// Whether an account needs the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// It works: its last sync or check succeeded. Its entry, if open, is done.
    Fine,
    /// Not known yet, or failing too briefly to bother the user (not synced since the core
    /// started, offline for a few minutes). Its entry stays as it is.
    Unsure,
    /// It needs the user since `since`. An entry done before `since` opens again (the problem came
    /// back); one done at or after it stays done (the user put this problem away).
    Problem { trouble: Trouble, since: DateTime<Utc> },
}

/// What is wrong with an account; decides the texts of its entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// The server or GitHub refused the app password or token.
    SignInRefused,
    /// This device has no usable sign-in: the secret is missing from the keychain, or the token
    /// belongs to another account now.
    SignInNeeded,
    /// Every sync failed for longer than [`crate::pim::SYNC_PROBLEM_AFTER`], for another reason
    /// than a refused sign-in (offline, server errors).
    SyncFailing,
}

/// Files the open approvals of `approvals` in `inbox` ([`Inbox::file_approvals`]) as of `now`,
/// under the approvals' lock ([`Approvals::with_open`]), so an older view never overtakes a newer
/// one. The core calls it after an approval was opened or decided, and [`tend_suite`] every
/// minute, which catches expired approvals and those that vanished while the core was down.
/// Failures go to the log; the approval itself works without its entry.
pub async fn file_approvals(inbox: &Inbox, approvals: &Approvals, now: DateTime<Utc>) {
    match approvals.with_open(now, |open| inbox.file_approvals(open, now)).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => eprintln!("torroworkplace: inbox: cannot file the approvals: {err}"),
        Err(err) => eprintln!("torroworkplace: inbox: cannot read the approvals to file them: {err}"),
    }
}

/// The routine of the source `suite` at `now`: files the open approvals ([`file_approvals`]) and
/// the accounts of the calendar service and the GitHub accounts that need the user
/// ([`Inbox::file_account_problems`]). The list counts as complete only once both services loaded
/// their accounts. The core runs it when it starts and every [`SUITE_EVERY`].
pub async fn tend_suite(
    inbox: &Inbox,
    approvals: &Approvals,
    pim: &PimService,
    github: &GithubAccounts,
    now: DateTime<Utc>,
) {
    file_approvals(inbox, approvals, now).await;
    let (nextcloud, github) = (pim.account_checks(now).await, github.account_checks().await);
    let complete = nextcloud.is_some() && github.is_some();
    let checks: Vec<AccountCheck> = nextcloud.into_iter().chain(github).flatten().collect();
    if let Err(err) = inbox.file_account_problems(&checks, complete, now) {
        eprintln!("torroworkplace: inbox: cannot file the account problems: {err}");
    }
}

/// A matter the core files itself, as it stands now.
struct Matter {
    thread_key: String,
    kind: Kind,
    title: String,
    body: Option<String>,
    link: Option<Link>,
    actions: Vec<Action>,
    untrusted: bool,
    /// Since when it stands ([`Health::Problem`]).
    since: DateTime<Utc>,
}

impl Matter {
    /// The matter as a new entry `id` of the suite at `now`.
    fn into_item(self, id: String, now: DateTime<Utc>) -> Item {
        let (lane, urgent) = rules::placement(self.kind, None);
        Item {
            id,
            source: SourceRef::suite(),
            thread_key: self.thread_key,
            kind: self.kind,
            lane,
            urgent,
            title: cap(&self.title, TITLE_MAX),
            body: self.body.map(|body| cap(&body, BODY_MAX)),
            from: None,
            agent: None,
            received: now,
            updated: now,
            count: 1,
            state: State::New,
            link: self.link,
            actions: self.actions,
            untrusted: self.untrusted,
        }
    }
}

/// The entry of an open approval: its summary as title, who asks as body.
fn approval_matter(approval: &ApprovalInfo) -> Matter {
    Matter {
        thread_key: format!("{APPROVAL_THREADS}{}", approval.id),
        kind: Kind::Approval,
        // Built by the core and made harmless (`approvals::summary`), but it quotes the client.
        title: approval.summary.clone(),
        body: Some(trf("{client} asks for approval", &[("client", &approval.client_name)])),
        link: None,
        actions: vec![Action::Allow { approval: approval.id.clone() }, Action::Deny { approval: approval.id.clone() }],
        untrusted: true,
        since: approval.created,
    }
}

/// The entry of an account that needs the user: written by the core in its language, so trusted;
/// opening it opens the control centre, where the account can be signed in again.
fn account_matter(check: &AccountCheck, trouble: Trouble, since: DateTime<Utc>) -> Matter {
    let account = [("account", &check.label as &dyn fmt::Display)];
    let (title, body) = match trouble {
        Trouble::SignInRefused => (
            trf("Sign-in to {account} was refused", &account),
            tr("The service no longer accepts the sign-in of this device. Sign in again under Accounts."),
        ),
        Trouble::SignInNeeded => (
            trf("Sign in to {account} again", &account),
            tr("This device has no valid sign-in for the account. Sign in again under Accounts."),
        ),
        Trouble::SyncFailing => (
            trf(
                "{account} has not synced for {minutes} minutes",
                &[account[0], ("minutes", &crate::pim::SYNC_PROBLEM_AFTER.num_minutes())],
            ),
            tr("Calendars and tasks show what was synced last. Check the connection, or open Accounts."),
        ),
    };
    Matter {
        thread_key: check.thread_key(),
        kind: Kind::AccountProblem,
        title,
        body: Some(body.to_string()),
        // `Link::App` names an app, not a page in it: the control centre opens where it opens.
        link: Some(Link::App { app: AppId::WORKPLACE.binary.to_string() }),
        actions: Vec::new(),
        untrusted: false,
        since,
    }
}

/// Brings the core's entries whose thread key starts with `family` in line with `matters`, all
/// matters of that family as they stand at `now`:
///
/// - a matter without an entry gets one, unless the user muted the suite;
/// - an open or snoozed entry keeps its state, its texts follow the matter;
/// - an entry done before the matter began ([`Matter::since`]) opens again ([`rules::merge`],
///   unless muted): the matter came back. One done since stays done: the user put it away;
/// - an open or snoozed entry without a matter is done when `fixed` says so for its thread key.
///
/// Nothing is throttled ([`Poster::Core`]); all in one transaction.
fn reconcile(
    db: &mut Connection,
    family: &str,
    matters: Vec<Matter>,
    fixed: impl Fn(&str) -> bool,
    now: DateTime<Utc>,
) -> Result<(), Failure> {
    let tx = db.transaction()?;
    let condition = "owner = ? AND instr(thread_key, ?) = 1";
    let stored = read_items(&tx, &Scope::All, condition, vec![text(CORE_OWNER), text(family)], "")?;
    let mut entries: BTreeMap<String, Item> =
        stored.into_iter().map(|stored| (stored.item.thread_key.clone(), stored.item)).collect();
    let suite = vec![text(SourceRef::suite().key())];
    let muted = !read_rules(&tx, &Scope::All, "source = ? AND effect = 'muted'", suite)?.is_empty();
    for matter in matters {
        match entries.remove(&matter.thread_key) {
            None if muted => {}
            None => insert_item(&tx, CORE_OWNER, &matter.into_item(new_id()?, now))?,
            Some(entry) => match entry.state {
                State::Done { at } if at < matter.since && !muted => {
                    let incoming = matter.into_item(entry.id.clone(), now);
                    update_item(&tx, &rules::merge(entry, incoming))?;
                }
                State::Done { .. } => {}
                State::New | State::Seen | State::Snoozed { .. } => {
                    let current = Item {
                        title: cap(&matter.title, TITLE_MAX),
                        body: matter.body.map(|body| cap(&body, BODY_MAX)),
                        link: matter.link,
                        actions: matter.actions,
                        untrusted: matter.untrusted,
                        ..entry.clone()
                    };
                    if current != entry {
                        update_item(&tx, &current)?;
                    }
                }
            },
        }
    }
    for (thread_key, mut entry) in entries {
        if !matches!(entry.state, State::Done { .. }) && fixed(&thread_key) {
            rules::set_done(&mut entry, true, now);
            update_item(&tx, &entry)?;
        }
    }
    tx.commit()?;
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

    /// Processes no system has (above the largest pid Linux hands out), so `/proc` knows none of
    /// them and no test depends on what runs on the machine.
    const AGENT_PIDS: [u32; 3] = [4_500_001, 4_500_002, 4_500_003];

    /// A waiting agent as its hook reports it, with `pids` and `link`; returns the entry's id.
    fn hook_reports(inbox: &Inbox, pids: &[u32], link: Option<Link>, thread: &str) -> String {
        let request = Request::PostItem {
            kind: Kind::AgentWaiting,
            title: "ignored".into(),
            body: None,
            link,
            lane: None,
            thread_key: Some(thread.into()),
            agent: Some(AgentInfo { pids: pids.to_vec(), ..session(AgentKind::ClaudeCode, thread) }),
        };
        posted(inbox.handle(request, Some(&hook()), at(9, 0)))
    }

    /// `opening` of `item` as the answer a client would get.
    fn refused(item: &Item) -> Response {
        opening(item).map_err(Failure::response).expect_err("not to be opened")
    }

    /// Task E2.4, security rule 6: the core links a hook's agent to its window, and opening
    /// follows the stored link, whatever was posted.
    #[test]
    fn opening_follows_what_is_safe_to_open() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let get = |id: &str| item(inbox.handle(Request::GetItem { id: id.into() }, None, at(9, 1)));

        // A hook's agent with processes jumps to its window, also when the hook sent no link.
        let waiting = hook_reports(&inbox, &AGENT_PIDS, None, "s1");
        assert_eq!(get(&waiting).link, Some(Link::Window));
        let pids = AGENT_PIDS.to_vec();
        assert_eq!(inbox.opening(&waiting, None, at(9, 2)), Ok(Opening::Window { pids }));
        // Without processes (macOS, Windows) there is no window to look for: no link, nothing to open.
        let blind_agent = hook_reports(&inbox, &[], Some(Link::Window), "s2");
        assert_eq!(get(&blind_agent).link, None);
        assert_eq!(code(&inbox.opening(&blind_agent, None, at(9, 2)).expect_err("no link")), "unsupported");

        // `window` on an entry that is no hook's is ignored: dropped when posted, refused when opened.
        let post = |link: Link| Request::PostItem {
            kind: Kind::Notice,
            title: "Linked".into(),
            body: None,
            link: Some(link),
            lane: None,
            thread_key: None,
            agent: Some(AgentInfo { pids: AGENT_PIDS.to_vec(), ..session(AgentKind::ClaudeCode, "s3") }),
        };
        let notice = posted(inbox.handle(post(Link::Window), Some(&script()), at(9, 3)));
        assert_eq!(get(&notice).link, None);
        assert_eq!(code(&inbox.opening(&notice, None, at(9, 4)).expect_err("no window")), "unsupported");
        let stored = Item { link: Some(Link::Window), ..get(&notice) };
        assert_eq!(code(&refused(&stored)), "unsupported", "a window only on a hook's entry");

        // Only `http`/`https` pages and apps of the suite.
        let web = Link::Url { url: "https://ci.example.org/1".into() };
        let page = posted(inbox.handle(post(web), Some(&script()), at(9, 5)));
        assert_eq!(inbox.opening(&page, None, at(9, 6)), Ok(Opening::Url("https://ci.example.org/1".into())));
        let app = posted(inbox.handle(post(Link::App { app: "torrotasks".into() }), Some(&script()), at(9, 5)));
        assert_eq!(inbox.opening(&app, None, at(9, 6)), Ok(Opening::App(AppId::TASKS)));
        let file = Item { link: Some(Link::Url { url: "file:///etc/passwd".into() }), ..stored.clone() };
        assert_eq!(code(&refused(&file)), "unsupported", "file:");
        let unknown_app = Item { link: Some(Link::App { app: "rm".into() }), ..stored };
        assert_eq!(code(&refused(&unknown_app)), "unsupported", "no app of the suite");
        assert_eq!(code(&inbox.opening("nope", None, at(9, 7)).expect_err("unknown")), "not-found");
    }

    #[test]
    fn a_client_opens_only_what_it_sees() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let waiting = hook_reports(&inbox, &AGENT_PIDS, None, "s1");
        // A client without the source, and the hook itself (rule 4), find nothing to open.
        for client in [blind(), hook()] {
            assert_eq!(code(&inbox.opening(&waiting, Some(&client), at(9, 1)).expect_err("hidden")), "not-found");
            let open = Request::OpenItem { id: waiting.clone() };
            assert_eq!(inbox.refusal(&open, Some(&client)).as_ref().map(code), Some("not-found"), "before approval");
        }
        assert!(inbox.opening(&waiting, Some(&script()), at(9, 1)).is_ok());
        assert_eq!(inbox.refusal(&Request::OpenItem { id: waiting }, Some(&script())), None);
        // The inbox never answers it alone: the core opens.
        assert_eq!(code(&inbox.handle(Request::OpenItem { id: "x".into() }, None, at(9, 2))), "error");
    }

    #[test]
    fn an_opened_entry_is_seen_if_the_opener_may_change_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let get = |id: &str| item(inbox.handle(Request::GetItem { id: id.into() }, None, at(9, 5)));
        let waiting = hook_reports(&inbox, &AGENT_PIDS, None, "s1");
        let before = get(&waiting);
        inbox.opened(&waiting, Some(&script()), at(9, 2));
        let after = get(&waiting);
        assert_eq!(after.state, State::Seen);
        assert_eq!((after.updated, after.count), (before.updated, before.count), "only seen, nothing else");
        // An approval belongs to the person (rule 1): an agent opening it leaves it new.
        file_from_core(&inbox, &approval(at(9, 0)));
        inbox.opened("approval-1", Some(&script()), at(9, 4));
        assert_eq!(get("approval-1").state, State::New);
        inbox.opened("approval-1", None, at(9, 4));
        assert_eq!(get("approval-1").state, State::Seen);
        // A done entry stays done; an unknown or hidden one is only logged.
        inbox.handle(Request::SetItemDone { id: waiting.clone(), done: true }, None, at(9, 5));
        inbox.opened(&waiting, None, at(9, 6));
        assert!(matches!(get(&waiting).state, State::Done { .. }));
        inbox.opened("nope", Some(&blind()), at(9, 6));
    }

    #[test]
    fn only_a_hooks_agent_state_with_processes_links_its_window() {
        assert!(!behind_tmux(&AGENT_PIDS), "unknown processes are no tmux");
        assert!(!behind_tmux(&[std::process::id()]), "this test is no tmux server");
        let with_pids = AgentInfo { pids: vec![AGENT_PIDS[0]], ..session(AgentKind::Codex, "s") };
        assert_eq!(window_link(Kind::AgentDone, Some(&with_pids)), Some(Link::Window));
        assert_eq!(window_link(Kind::Notice, Some(&with_pids)), None, "no agent state");
        assert_eq!(window_link(Kind::AgentDone, None), None, "no agent");
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

    /// The entries of the source `suite`, as the app key lists them.
    fn suite(inbox: &Inbox, now: DateTime<Utc>) -> Vec<Item> {
        let request =
            Request::ListItems { lane: None, source: Some(SourceRef::suite()), state: None, since: None, limit: None };
        let mut items = items(inbox.handle(request, None, now));
        items.sort_by(|a, b| a.thread_key.cmp(&b.thread_key));
        items
    }

    async fn ask(approvals: &Approvals, template: &str, now: DateTime<Utc>) -> ApprovalInfo {
        let request = Request::DeleteTemplate { id: template.into() };
        let summary = format!("Delete template \"{template}\"");
        approvals.open("c1", "Claude Code", request, summary, now).await.expect("opened")
    }

    fn entry_of<'a>(items: &'a [Item], approval: &ApprovalInfo) -> &'a Item {
        let key = format!("approval:{}", approval.id);
        items.iter().find(|item| item.thread_key == key).expect("an entry for the approval")
    }

    #[tokio::test]
    async fn an_open_approval_is_an_entry_until_decided_or_expired() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (inbox, approvals) = (inbox_in(dir.path()), crate::approvals::tests::quiet_in(dir.path()));
        let first = ask(&approvals, "dev", at(8, 0)).await;
        let second = ask(&approvals, "ops", at(8, 0)).await;
        file_approvals(&inbox, &approvals, at(8, 0)).await;
        file_approvals(&inbox, &approvals, at(8, 0)).await;

        let items = suite(&inbox, at(8, 0));
        assert_eq!(items.len(), 2, "one entry per approval, filing twice adds none: {items:?}");
        let entry = entry_of(&items, &first);
        assert_eq!((entry.kind, entry.lane, entry.urgent, entry.untrusted), (Kind::Approval, Lane::ForMe, true, true));
        assert_eq!(entry.title, first.summary);
        let asks = trf("{client} asks for approval", &[("client", &"Claude Code")]);
        assert_eq!(entry.body.as_deref(), Some(asks.as_str()));
        assert_eq!(
            entry.actions,
            [Action::Allow { approval: first.id.clone() }, Action::Deny { approval: first.id.clone() }]
        );
        assert_eq!((entry.state, entry.count, entry.link.clone()), (State::New, 1, None));

        // Decided: done at once. The other one expires ten minutes after it was asked for.
        approvals.decide(&first.id, false, at(8, 1)).await.expect("decided");
        file_approvals(&inbox, &approvals, at(8, 1)).await;
        let items = suite(&inbox, at(8, 1));
        assert_eq!(entry_of(&items, &first).state, State::Done { at: at(8, 1) });
        assert_eq!(entry_of(&items, &second).state, State::New);

        file_approvals(&inbox, &approvals, at(8, 11)).await;
        assert_eq!(entry_of(&suite(&inbox, at(8, 11)), &second).state, State::Done { at: at(8, 11) });
    }

    #[tokio::test]
    async fn a_restart_files_what_is_missing_and_resolves_what_vanished() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (inbox, approvals) = (inbox_in(dir.path()), crate::approvals::tests::quiet_in(dir.path()));
        let filed = ask(&approvals, "dev", at(8, 0)).await;
        file_approvals(&inbox, &approvals, at(8, 0)).await;
        // The core stopped right after storing this one, before its entry was filed.
        let unfiled = ask(&approvals, "ops", at(8, 1)).await;
        // An entry whose approval is gone (dropped with a damaged approvals.json, say).
        let vanished = Item { id: "gone".into(), thread_key: "approval:gone".into(), ..approval(at(7, 0)) };
        file_from_core(&inbox, &vanished);
        drop((inbox, approvals));

        let (inbox, approvals) = (inbox_in(dir.path()), crate::approvals::tests::quiet_in(dir.path()));
        file_approvals(&inbox, &approvals, at(8, 2)).await;
        let items = suite(&inbox, at(8, 2));
        assert_eq!(items.len(), 3, "{items:?}");
        let kept = entry_of(&items, &filed);
        assert_eq!((kept.state, kept.count), (State::New, 1), "no duplicate");
        assert_eq!(entry_of(&items, &unfiled).state, State::New);
        let gone = items.iter().find(|item| item.id == "gone").expect("kept");
        assert_eq!(gone.state, State::Done { at: at(8, 2) });
    }

    fn check(account: &str, health: Health) -> AccountCheck {
        AccountCheck { account: account.into(), label: format!("{account}@cloud.example.org"), health }
    }

    fn problem(trouble: Trouble, since: DateTime<Utc>) -> Health {
        Health::Problem { trouble, since }
    }

    fn account_entry<'a>(items: &'a [Item], account: &str) -> &'a Item {
        let key = format!("account:{account}");
        items.iter().find(|item| item.thread_key == key).expect("an entry for the account")
    }

    #[test]
    fn an_account_problem_comes_and_goes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let refused = [check("anna", problem(Trouble::SignInRefused, at(8, 0))), check("bob", Health::Fine)];
        inbox.file_account_problems(&refused, true, at(8, 0)).expect("filed");
        inbox.file_account_problems(&refused, true, at(8, 1)).expect("filed");

        let items = suite(&inbox, at(8, 1));
        assert_eq!(items.len(), 1, "only anna needs the user, once: {items:?}");
        let entry = account_entry(&items, "anna");
        assert_eq!(
            (entry.kind, entry.lane, entry.urgent, entry.untrusted),
            (Kind::AccountProblem, Lane::ForMe, false, false)
        );
        assert_eq!(entry.title, trf("Sign-in to {account} was refused", &[("account", &"anna@cloud.example.org")]));
        assert_eq!(entry.link, Some(Link::App { app: "torroworkplace".into() }));
        assert!(rules::may_open(entry, is_app_of_the_suite), "the control centre may be opened");
        assert_eq!((entry.state, entry.count), (State::New, 1));

        // Another problem of the same account changes its entry; fixed, it is done.
        let failing = [check("anna", problem(Trouble::SyncFailing, at(8, 2)))];
        inbox.file_account_problems(&failing, true, at(8, 3)).expect("filed");
        let entry = account_entry(&suite(&inbox, at(8, 3)), "anna").clone();
        let minutes = crate::pim::SYNC_PROBLEM_AFTER.num_minutes();
        let args: [(&str, &dyn fmt::Display); 2] = [("account", &"anna@cloud.example.org"), ("minutes", &minutes)];
        let title = trf("{account} has not synced for {minutes} minutes", &args);
        assert_eq!((entry.title, entry.state, entry.count), (title, State::New, 1));
        inbox.file_account_problems(&[check("anna", Health::Fine)], true, at(8, 4)).expect("filed");
        assert_eq!(account_entry(&suite(&inbox, at(8, 4)), "anna").state, State::Done { at: at(8, 4) });

        // It comes back: the entry opens again. Put away by the person, it stays away while
        // this problem lasts.
        let back = [check("anna", problem(Trouble::SignInNeeded, at(9, 0)))];
        inbox.file_account_problems(&back, true, at(9, 0)).expect("filed");
        let entry = account_entry(&suite(&inbox, at(9, 0)), "anna").clone();
        assert_eq!((entry.state, entry.count), (State::New, 2));
        inbox.handle(Request::SetItemDone { id: entry.id.clone(), done: true }, None, at(9, 5));
        inbox.file_account_problems(&back, true, at(9, 6)).expect("filed");
        assert_eq!(account_entry(&suite(&inbox, at(9, 6)), "anna").state, State::Done { at: at(9, 5) });
    }

    #[test]
    fn an_unsure_or_unlisted_account_keeps_its_entry_until_the_list_is_complete() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let both = [
            check("anna", problem(Trouble::SyncFailing, at(8, 0))),
            check("bob", problem(Trouble::SignInNeeded, at(8, 0))),
        ];
        inbox.file_account_problems(&both, true, at(8, 0)).expect("filed");

        // After a restart nothing is known yet: anna has not synced, bob's service has not loaded.
        inbox.file_account_problems(&[check("anna", Health::Unsure)], false, at(8, 1)).expect("filed");
        let items = suite(&inbox, at(8, 1));
        assert!(items.iter().all(|item| item.state == State::New), "{items:?}");

        // Once every service has loaded, an account that is not listed was removed.
        inbox.file_account_problems(&[check("anna", Health::Unsure)], true, at(8, 2)).expect("filed");
        let items = suite(&inbox, at(8, 2));
        assert_eq!(account_entry(&items, "anna").state, State::New);
        assert_eq!(account_entry(&items, "bob").state, State::Done { at: at(8, 2) });
    }

    #[tokio::test]
    async fn an_agent_cannot_put_away_an_approval_or_an_account_problem() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (inbox, approvals) = (inbox_in(dir.path()), crate::approvals::tests::quiet_in(dir.path()));
        let asked = ask(&approvals, "dev", at(8, 0)).await;
        file_approvals(&inbox, &approvals, at(8, 0)).await;
        let problems = [check("anna", problem(Trouble::SignInRefused, at(8, 0)))];
        inbox.file_account_problems(&problems, true, at(8, 0)).expect("filed");
        let items = suite(&inbox, at(8, 0));
        assert_eq!(items.len(), 2);

        // A client that sees the suite reads them but changes nothing, nor resolves their threads.
        let script = script();
        for entry in &items {
            let done = Request::SetItemDone { id: entry.id.clone(), done: true };
            assert_eq!(code(&inbox.handle(done.clone(), Some(&script), at(8, 1))), "forbidden");
            assert_eq!(inbox.refusal(&done, Some(&script)).as_ref().map(code), Some("forbidden"));
        }
        for thread_key in [format!("approval:{}", asked.id), "account:anna".to_string()] {
            let resolve = Request::ResolveItems { thread_key };
            assert_eq!(inbox.handle(resolve.clone(), Some(&script), at(8, 2)), Response::Ok);
            // Not even the app key's own reports share the core's threads.
            assert_eq!(inbox.handle(resolve, None, at(8, 2)), Response::Ok);
        }
        assert!(suite(&inbox, at(8, 3)).iter().all(|item| item.state == State::New));
    }

    #[test]
    fn a_muted_suite_gets_no_new_entries_but_old_ones_still_resolve() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let anna = [check("anna", problem(Trouble::SignInNeeded, at(8, 0)))];
        inbox.file_account_problems(&anna, true, at(8, 0)).expect("filed");
        let mute = Request::MuteSource { source: SourceRef::suite(), muted: true };
        assert_eq!(inbox.handle(mute, None, at(8, 1)), Response::Ok);

        let checks = [check("anna", Health::Fine), check("bob", problem(Trouble::SignInNeeded, at(8, 2)))];
        inbox.file_account_problems(&checks, true, at(8, 2)).expect("filed");
        let items = suite(&inbox, at(8, 2));
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(account_entry(&items, "anna").state, State::Done { at: at(8, 2) });
    }

    /// A notice "for me" from `client` at `now` in thread `key`.
    fn for_me_notice(inbox: &Inbox, title: &str, key: &str, now: DateTime<Utc>) -> String {
        let request = Request::PostItem {
            kind: Kind::Notice,
            title: title.into(),
            body: None,
            link: None,
            lane: Some(Lane::ForMe),
            thread_key: Some(key.into()),
            agent: None,
        };
        posted(inbox.handle(request, Some(&script()), now))
    }

    /// Task E3.3b: the tray hears of every write, and of nothing else.
    #[test]
    fn every_write_tells_the_tray_and_reading_does_not() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let mut changes = inbox.changes();
        let mut wrote = || {
            let changed = changes.has_changed().expect("the inbox is alive");
            changes.mark_unchanged();
            changed
        };

        inbox.handle(all(), None, at(8, 0));
        assert!(!wrote(), "listing changes nothing");
        let id = for_me_notice(&inbox, "Build failed", "build", at(8, 0));
        assert!(wrote(), "a post");
        for_me_notice(&inbox, "Build failed again", "build", at(8, 1));
        assert!(wrote(), "a merge");
        inbox.handle(Request::SnoozeItem { id: id.clone(), until: Some(at(9, 0)) }, None, at(8, 2));
        assert!(wrote(), "a snooze");
        inbox.handle(all(), None, at(8, 30));
        assert!(!wrote(), "listing before the entry is due wakes nothing");
        assert_eq!(inbox.for_me(at(9, 1), 5).map(|for_me| for_me.open), Ok(1));
        assert!(wrote(), "the tray's own read woke the entry");
        inbox.handle(Request::SetItemLane { id: id.clone(), lane: Lane::Info, remember: false }, None, at(9, 2));
        assert!(wrote(), "a lane");
        inbox.handle(Request::SetItemDone { id: id.clone(), done: true }, None, at(9, 3));
        assert!(wrote(), "done");

        for_me_notice(&inbox, "Deploy waits", "deploy", at(9, 4));
        wrote();
        let resolve = Request::ResolveItems { thread_key: "deploy".into() };
        assert_eq!(inbox.handle(resolve.clone(), Some(&script()), at(9, 5)), Response::Ok);
        assert!(wrote(), "resolving");
        inbox.handle(resolve, Some(&script()), at(9, 6));
        assert!(!wrote(), "resolving what is done already writes nothing");

        file_from_core(&inbox, &approval(at(9, 7)));
        wrote();
        let done = Request::SetItemDone { id: "approval-1".into(), done: true };
        assert_eq!(code(&inbox.handle(done, Some(&script()), at(9, 8))), "forbidden");
        assert!(!wrote(), "a refused change");

        let later = at(9, 9) + TimeDelta::days(KEEP_DONE_DAYS + 1);
        assert_eq!(inbox.tend(later, true), Ok((0, 2)));
        assert!(wrote(), "the clean-up");
        assert_eq!(inbox.tend(later, true), Ok((0, 0)));
        assert!(!wrote(), "a clean-up that removes nothing");
    }

    /// Task E2.1 meets E3.3b: the suite's reconciliation tells the tray only when it changed
    /// something, so its minute routine does not refresh the menu for nothing.
    #[test]
    fn the_suite_tells_the_tray_only_of_real_changes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        let mut changes = inbox.changes();
        let anna = [check("anna", problem(Trouble::SignInNeeded, at(8, 0)))];
        inbox.file_account_problems(&anna, true, at(8, 0)).expect("filed");
        assert!(changes.has_changed().expect("alive"), "a new account problem");
        changes.mark_unchanged();
        inbox.file_account_problems(&anna, true, at(8, 1)).expect("filed");
        assert!(!changes.has_changed().expect("alive"), "the same problem again");
        inbox.file_account_problems(&[check("anna", Health::Fine)], true, at(8, 2)).expect("filed");
        assert!(changes.has_changed().expect("alive"), "the account is fine again");
    }

    /// Task E3.3b: the tray counts the open entries "for me", the urgent among them, and lists
    /// the newest first; info, snoozed and done entries are not counted.
    #[test]
    fn the_tray_counts_open_entries_for_me_and_lists_the_newest() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = inbox_in(dir.path());
        assert_eq!(inbox.for_me(at(8, 0), 5), Ok(ForMe::default()));

        file_from_core(&inbox, &approval(at(8, 0)));
        for minute in 1..=6 {
            for_me_notice(&inbox, &format!("Notice {minute}"), &format!("n{minute}"), at(8, minute));
        }
        posted(inbox.handle(notice("Backup finished", None), Some(&script()), at(8, 10)));
        let snoozed = for_me_notice(&inbox, "Later", "later", at(8, 11));
        inbox.handle(Request::SnoozeItem { id: snoozed, until: Some(at(12, 0)) }, None, at(8, 11));
        let done = for_me_notice(&inbox, "Done", "done", at(8, 12));
        inbox.handle(Request::SetItemDone { id: done, done: true }, None, at(8, 12));

        let for_me = inbox.for_me(at(8, 20), 5).expect("read");
        assert_eq!((for_me.open, for_me.urgent), (7, 1), "six notices and the approval");
        let titles: Vec<&str> = for_me.newest.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["Notice 6", "Notice 5", "Notice 4", "Notice 3", "Notice 2"]);
        assert_eq!(for_me.newest[0].source, SourceRef::script("script aa01"));

        let woken = inbox.for_me(at(12, 1), 1).expect("read");
        assert_eq!((woken.open, woken.newest[0].title.as_str()), (8, "Later"), "woken entries count again");
    }
}
