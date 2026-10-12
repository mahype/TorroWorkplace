//! Approvals: client commands that wait for a person (`docs/api.md`, "Freigaben: der Mensch
//! entscheidet").
//!
//! When a client's rights say a command needs a person's approval
//! ([`torro_ipc::api::Rights::needs_approval`]), the core does not run it but keeps it here as an
//! open approval and answers `pending` with its id and a summary in words ([`summary`]). The
//! approvals live in `approvals.json` in the core's state directory (mode 0600), so a restart loses
//! none; an open one expires ten minutes after it was asked for ([`LIFETIME`]). Decided and expired
//! ones are kept a day ([`KEPT`]) so the client can still read the outcome, then dropped.
//!
//! Only the app key decides ([`torro_ipc::Request::DecideApproval`]): the control centre,
//! `torroworkplace ctl allow|deny <id>`, or the desktop notification with "Allow" and "Deny" that
//! every new approval shows ([`Approvals::system`]), whose click goes to the core with the app key.
//! The notification closes once the approval is decided by any of these ways, and the core asks
//! again about the open ones when it starts ([`Approvals::announce_open`]). The agent never gets a
//! code it could confirm with. A command kept from agents ([`Mcp::Never`]: taking secrets,
//! managing clients, deciding approvals) never becomes an approval, so a secret never reaches
//! `approvals.json`.
//!
//! The summary says what really runs: titles, dates and names come from the core's own caches,
//! looked up by the keys the command runs with ([`Facts`]), never from the titles the client sent
//! along. What is stored is the request itself, with the client's text in it; the file is for the
//! owner only. Changes are made under one lock and written before they count: an approval that
//! cannot be written is not opened (the core refuses the command), a decision that cannot be
//! written is not taken.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::paths::AppPaths;
use torro_gui::datetime::{format_short_date, format_time, to_local};
use torro_inbox::{Item, Lane, Rule, RuleEffect, SourceRef};
use torro_ipc::api::Mcp;
use torro_ipc::{ApprovalInfo, ApprovalState, Request, Response};
use torro_pim::{Event, EventTime, Todo};
use torro_setups::LoadMode;

/// The file in the core's state directory.
pub const FILE: &str = "approvals.json";

/// How long an approval waits for a person.
pub const LIFETIME: chrono::Duration = chrono::Duration::minutes(10);

/// How long a decided or expired approval is kept, so its client can still read the outcome.
pub const KEPT: chrono::Duration = chrono::Duration::days(1);

/// The longest piece of a client's text in a summary, in characters.
const QUOTE_MAX: usize = 60;

/// How many approvals one client may have open at a time. Each one asks the person with a
/// notification, so an agent in a loop must not be able to flood the desktop.
pub const OPEN_MAX: usize = 10;

/// How the person hears of approvals: asked once for every new one after it was stored (and
/// again when the core starts, [`Approvals::announce_open`]), and told when one was decided, so
/// the question can leave the desktop.
#[derive(Clone)]
pub struct Notifier {
    ask: Arc<dyn Fn(&ApprovalInfo) + Send + Sync>,
    settled: Arc<dyn Fn(&str) + Send + Sync>,
}

impl Notifier {
    /// Calls `ask` for every approval to ask about and `settled` with the id of every approval a
    /// person decided (in the control centre, with `ctl`, or in the notification itself). Both
    /// must return at once: they run in the core's request handling.
    pub fn new(
        ask: impl Fn(&ApprovalInfo) + Send + Sync + 'static,
        settled: impl Fn(&str) + Send + Sync + 'static,
    ) -> Notifier {
        Notifier { ask: Arc::new(ask), settled: Arc::new(settled) }
    }

    /// Calls `ask` and has nothing to take back when an approval is decided; for tests.
    pub fn asking(ask: impl Fn(&ApprovalInfo) + Send + Sync + 'static) -> Notifier {
        Notifier::new(ask, |_: &str| {})
    }
}

/// What the core itself holds about the things a request names, for [`summary`]. The core looks
/// everything up by the keys the command will run with (the `href` of an event or task, resolved
/// the way the write resolves it, and the ids of collections, templates and accounts), never in
/// the titles and names the client sent along: a client must not be able to show "Lunch" and
/// delete the board meeting.
///
/// Only filled after the client's reach was checked (`Core::handle_from`, step 2), so it names
/// nothing the client could not read anyway.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// The event or task that is changed, moved or deleted, as the core caches it; for
    /// `tasks.move_todo` the task moved, for `tasks.unpin_issue` the pinned item. `None` when the
    /// request names none or the core does not hold it: the summary then says "unknown".
    pub item: Option<Known>,
    /// The display name of what the request works on: the calendar or task list it writes to
    /// (for a move, where the item goes), the filter list, the collection shown or hidden, the
    /// template or the account. `None` when the core does not know it: the summary then shows the
    /// id the command runs with, which then finds nothing either.
    pub name: Option<String>,
    /// For a save that moves an item to another collection: the display name of the collection
    /// that holds it now and loses it. `None` when nothing moves.
    pub moved_from: Option<String>,
}

/// An event or task as the core caches it, for [`Facts::item`].
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub title: String,
    /// When an event starts (its first cached occurrence); `None` for a task.
    pub start: Option<EventTime>,
}

/// One approval in `approvals.json`: what the API shows, and the request that runs when a person
/// allows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    info: ApprovalInfo,
    request: Request,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ApprovalsFile {
    #[serde(default)]
    approvals: Vec<Entry>,
}

/// Why an approval could not be opened, read or decided.
#[derive(Debug)]
pub enum ApprovalsError {
    /// No home directory, so nowhere to keep approvals.
    NoHome,
    Io {
        path: PathBuf,
        err: io::Error,
    },
    /// The system has no randomness to make an id from.
    Random(String),
    /// No approval with this id for this caller (also one of another client, or one dropped
    /// after a day).
    NotFound(String),
    /// The approval was decided already or expired.
    NotOpen(String),
    /// The command is kept from agents ([`Mcp::Never`]) and never waits for approval.
    KeptBack(&'static str),
    /// The client has [`OPEN_MAX`] approvals open already.
    TooMany,
}

impl ApprovalsError {
    /// The error code of the API answer.
    pub fn code(&self) -> &'static str {
        match self {
            ApprovalsError::NotFound(_) => "not-found",
            ApprovalsError::NotOpen(_) => "conflict",
            ApprovalsError::KeptBack(_) => "forbidden",
            ApprovalsError::TooMany => "busy",
            _ => "error",
        }
    }

    /// The error as an API answer. A failure to read or write the file names no path towards the
    /// client (it would tell an agent where the user's home is); the whole of it goes to the log.
    pub fn response(&self) -> Response {
        let message = match self {
            ApprovalsError::NoHome | ApprovalsError::Io { .. } | ApprovalsError::Random(_) => {
                eprintln!("torroworkplace: {self}");
                "the approvals cannot be read or stored on this device; the core's log says why".to_string()
            }
            _ => self.to_string(),
        };
        Response::Error { code: self.code().into(), message }
    }
}

impl std::fmt::Display for ApprovalsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalsError::NoHome => f.write_str("no home directory to keep approvals in"),
            ApprovalsError::Io { path, err } => write!(f, "{}: {err}", path.display()),
            ApprovalsError::Random(err) => write!(f, "cannot make an approval id: {err}"),
            ApprovalsError::NotFound(id) => write!(f, "no approval {id:?}"),
            ApprovalsError::NotOpen(id) => write!(f, "approval {id:?} was decided already or expired"),
            ApprovalsError::KeptBack(command) => write!(f, "{command} never waits for approval"),
            ApprovalsError::TooMany => write!(
                f,
                "{OPEN_MAX} commands of this client wait for a person's approval already; wait for their decision"
            ),
        }
    }
}

impl std::error::Error for ApprovalsError {}

/// What a person's decision leaves to do.
#[derive(Debug)]
pub enum Decision {
    /// Denied; nothing runs.
    Denied(ApprovalInfo),
    /// Allowed: the approval is [`ApprovalState::Running`]. Run `request` for the client `client`
    /// (rights and collections checked again) and store its answer with [`Approvals::finish`].
    Run { client: String, request: Box<Request> },
}

/// The open and recent approvals of the core.
pub struct Approvals {
    /// `approvals.json`; `None` without a home directory, then no approval can be opened.
    file: Option<PathBuf>,
    notify: Notifier,
    /// Also held while the file is written, so the file always holds the newest state.
    state: tokio::sync::Mutex<State>,
}

#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    /// The file was read; it is read on first use, so building the core reads nothing.
    loaded: bool,
}

impl Approvals {
    /// The user's approvals: `approvals.json` in the core's state directory, and a desktop
    /// notification with "Allow" and "Deny" for each new one. A click there sends
    /// `approvals.decide_approval` to the core's socket with the app key, as the control centre
    /// would. Reads nothing until first used.
    ///
    /// In tests there is no such file and no notification: a test that forgets to pass its own
    /// approvals ([`crate::core::Core::with_approvals`]) gets `forbidden` for every approval
    /// instead of touching the user's state directory or desktop.
    pub fn system() -> Approvals {
        if cfg!(test) {
            return Approvals::new(None, Notifier::asking(|_: &ApprovalInfo| {}));
        }
        let file = AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join(FILE));
        Approvals::new(file, desktop_notifier(torro_ipc::socket_path()))
    }

    /// Approvals kept in `file`, announced with `notify`. Tests pass a temporary file and a
    /// notifier that shows nothing.
    pub fn new(file: Option<PathBuf>, notify: Notifier) -> Approvals {
        Approvals { file, notify, state: tokio::sync::Mutex::new(State::default()) }
    }

    /// Opens an approval for `request` of the client `client` (named `client_name`, or shown by
    /// its id when the name is empty), stores it and announces it. `summary` says in words what
    /// would happen ([`summary`]).
    ///
    /// Refuses a command kept from agents ([`ApprovalsError::KeptBack`]) before anything is
    /// stored: such a request may carry a secret. A client with [`OPEN_MAX`] open approvals gets
    /// [`ApprovalsError::TooMany`]. When the file cannot be written nothing is opened, and the
    /// caller must refuse the command.
    pub async fn open(
        &self,
        client: &str,
        client_name: &str,
        request: Request,
        summary: String,
        now: DateTime<Utc>,
    ) -> Result<ApprovalInfo, ApprovalsError> {
        let command = request.operation().command;
        if command.mcp == Mcp::Never {
            return Err(ApprovalsError::KeptBack(command.name));
        }
        let mut state = self.state(now).await?;
        let open = state.entries.iter().filter(|e| e.info.client == client && e.info.state.is_pending()).count();
        if open >= OPEN_MAX {
            return Err(ApprovalsError::TooMany);
        }
        let info = ApprovalInfo {
            id: new_id()?,
            client: client.to_string(),
            // "… asks for approval" needs someone who asks; the id is what the person can look up.
            client_name: if client_name.trim().is_empty() { client } else { client_name }.to_string(),
            command: command.name.to_string(),
            summary,
            created: now,
            expires: now + LIFETIME,
            decided: None,
            state: ApprovalState::Pending,
        };
        state.entries.push(Entry { info: info.clone(), request });
        if let Err(err) = self.write(&state).await {
            state.entries.pop();
            return Err(err);
        }
        drop(state);
        (self.notify.ask)(&info);
        Ok(info)
    }

    /// Asks the person again about every approval that is still open, as when it was opened, and
    /// returns how many. For the core's start: the questions of the core before went with its
    /// process. Approvals that expired meanwhile are not asked about. Without a file (no home
    /// directory, tests without their own approvals) there is nothing to ask about.
    pub async fn announce_open(&self, now: DateTime<Utc>) -> Result<usize, ApprovalsError> {
        if self.file.is_none() {
            return Ok(0);
        }
        let open: Vec<ApprovalInfo> = {
            let state = self.state(now).await?;
            state.entries.iter().filter(|entry| entry.info.state.is_pending()).map(|entry| entry.info.clone()).collect()
        };
        for info in &open {
            (self.notify.ask)(info);
        }
        Ok(open.len())
    }

    /// The approvals `client` may see, newest first: its own, or all for `None` (the app key).
    pub async fn list(&self, client: Option<&str>, now: DateTime<Utc>) -> Result<Vec<ApprovalInfo>, ApprovalsError> {
        let state = self.state(now).await?;
        let mut list: Vec<ApprovalInfo> =
            state.entries.iter().filter(|entry| visible(entry, client)).map(|entry| entry.info.clone()).collect();
        list.sort_by_key(|info| std::cmp::Reverse(info.created));
        Ok(list)
    }

    /// The approval `id`, if `client` may see it ([`Approvals::list`]).
    pub async fn status(
        &self,
        client: Option<&str>,
        id: &str,
        now: DateTime<Utc>,
    ) -> Result<ApprovalInfo, ApprovalsError> {
        let state = self.state(now).await?;
        state
            .entries
            .iter()
            .find(|entry| entry.info.id == id && visible(entry, client))
            .map(|entry| entry.info.clone())
            .ok_or_else(|| ApprovalsError::NotFound(id.to_string()))
    }

    /// A person's decision on the open approval `id`, stored before it counts. Allowed, the
    /// approval becomes [`ApprovalState::Running`] and the caller runs the request
    /// ([`Decision::Run`]); denied, it is done. An approval that is no longer open is
    /// [`ApprovalsError::NotOpen`].
    pub async fn decide(&self, id: &str, allow: bool, now: DateTime<Utc>) -> Result<Decision, ApprovalsError> {
        let mut state = self.state(now).await?;
        let index = state
            .entries
            .iter()
            .position(|entry| entry.info.id == id)
            .ok_or_else(|| ApprovalsError::NotFound(id.to_string()))?;
        if !state.entries[index].info.state.is_pending() {
            return Err(ApprovalsError::NotOpen(id.to_string()));
        }
        let before = state.entries[index].info.clone();
        let entry = &mut state.entries[index];
        entry.info.decided = Some(now);
        entry.info.state = if allow { ApprovalState::Running } else { ApprovalState::Denied };
        let decision = match allow {
            true => Decision::Run { client: entry.info.client.clone(), request: Box::new(entry.request.clone()) },
            false => Decision::Denied(entry.info.clone()),
        };
        if let Err(err) = self.write(&state).await {
            state.entries[index].info = before;
            return Err(err);
        }
        (self.notify.settled)(id);
        Ok(decision)
    }

    /// Stores the answer of an allowed approval that ran ([`ApprovalState::Done`]). Kept in memory
    /// even when the file cannot be written, so the client still reads it until the core stops.
    pub async fn finish(
        &self,
        id: &str,
        response: Response,
        now: DateTime<Utc>,
    ) -> Result<ApprovalInfo, ApprovalsError> {
        let mut state = self.state(now).await?;
        let entry = state
            .entries
            .iter_mut()
            .find(|entry| entry.info.id == id)
            .ok_or_else(|| ApprovalsError::NotFound(id.to_string()))?;
        entry.info.state = ApprovalState::Done { response: Box::new(response) };
        let info = entry.info.clone();
        if let Err(err) = self.write(&state).await {
            eprintln!("torroworkplace: cannot store the outcome of approval {id}: {err}");
        }
        Ok(info)
    }

    /// The state, read from the file on first use, with expired approvals marked and old ones
    /// dropped as of `now`. Both follow from the time alone, so they need no write of their own.
    async fn state(&self, now: DateTime<Utc>) -> Result<tokio::sync::MutexGuard<'_, State>, ApprovalsError> {
        let mut state = self.state.lock().await;
        if !state.loaded {
            let file = self.file.clone().ok_or(ApprovalsError::NoHome)?;
            state.entries = blocking(move || read(&file)).await?;
            state.loaded = true;
        }
        tidy(&mut state.entries, now);
        Ok(state)
    }

    /// Writes `state` to the file (0600), on a blocking thread.
    async fn write(&self, state: &State) -> Result<(), ApprovalsError> {
        let file = self.file.clone().ok_or(ApprovalsError::NoHome)?;
        let content = ApprovalsFile { approvals: state.entries.clone() };
        blocking(move || {
            let bytes = serde_json::to_vec_pretty(&content)
                .map_err(|err| ApprovalsError::Io { path: file.clone(), err: io::Error::other(err) })?;
            if let Some(dir) = file.parent() {
                crate::clients::create_private_dir(dir)
                    .map_err(|err| ApprovalsError::Io { path: file.clone(), err })?;
            }
            crate::clients::write_private(&file, &bytes)
                .map_err(|err| ApprovalsError::Io { path: file.clone(), err: io::Error::other(err.to_string()) })
        })
        .await
    }
}

/// A client sees its own approvals, the app key (`None`) all.
fn visible(entry: &Entry, client: Option<&str>) -> bool {
    client.is_none_or(|client| entry.info.client == client)
}

/// Marks open approvals past their time as expired (decided at that time) and drops the ones
/// decided longer than [`KEPT`] ago.
fn tidy(entries: &mut Vec<Entry>, now: DateTime<Utc>) {
    for entry in entries.iter_mut() {
        if entry.info.state.is_pending() && entry.info.expires <= now {
            entry.info.state = ApprovalState::Expired;
            entry.info.decided = Some(entry.info.expires);
        }
    }
    entries.retain(|entry| entry.info.decided.is_none_or(|decided| now - decided < KEPT));
}

/// The approvals in `file`; none when it does not exist. A file that is not ours is moved aside
/// (`approvals.json.damaged`) and reported, so approvals keep working.
///
/// An approval that was running when the core stopped is done with an error: nobody knows whether
/// its command took effect, and it must never run twice.
fn read(file: &Path) -> Result<Vec<Entry>, ApprovalsError> {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(ApprovalsError::Io { path: file.to_path_buf(), err }),
    };
    let mut entries = match serde_json::from_slice::<ApprovalsFile>(&bytes) {
        Ok(content) => content.approvals,
        Err(err) => {
            let aside = file.with_extension("json.damaged");
            eprintln!("torroworkplace: {} is damaged ({err}); moved to {}", file.display(), aside.display());
            fs::rename(file, &aside).map_err(|err| ApprovalsError::Io { path: file.to_path_buf(), err })?;
            Vec::new()
        }
    };
    for entry in &mut entries {
        if matches!(entry.info.state, ApprovalState::Running) {
            let message = "the core stopped while the command ran; check whether it took effect".to_string();
            entry.info.state =
                ApprovalState::Done { response: Box::new(Response::Error { code: "error".into(), message }) };
        }
    }
    Ok(entries)
}

/// Runs file work on a blocking thread, so other requests are not held up.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApprovalsError> + Send + 'static,
) -> Result<T, ApprovalsError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| ApprovalsError::Io { path: PathBuf::from(FILE), err: io::Error::other(err.to_string()) })?
}

/// 16 random hex digits.
fn new_id() -> Result<String, ApprovalsError> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|err| ApprovalsError::Random(err.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The notifier of [`Approvals::system`]: a desktop notification whose "Allow" or "Deny" goes to
/// the core at `socket` as `approvals.decide_approval` with the app key, which
/// `torro_ipc::client::call` reads from next to the socket. It runs in the core's own process, so
/// the decision has the app's authority and the agent never sees a code.
///
/// Each question closes when its approval is decided, whichever way ([`Notifier::new`]'s
/// `settled`), and when it expires ([`crate::notify::ask`]).
fn desktop_notifier(socket: Option<PathBuf>) -> Notifier {
    // The questions on the desktop by approval id; sending closes one.
    let open: Arc<std::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>> = Arc::default();
    let asking = Arc::clone(&open);
    let ask = move |approval: &ApprovalInfo| {
        let (close, settled) = tokio::sync::oneshot::channel();
        {
            let mut open = asking.lock().unwrap_or_else(PoisonError::into_inner);
            // Questions that went away on their own (expired, closed by hand) need no closing.
            open.retain(|_, close| !close.is_closed());
            open.insert(approval.id.clone(), close);
        }
        let runtime = tokio::runtime::Handle::try_current().ok();
        let (socket, id) = (socket.clone(), approval.id.clone());
        crate::notify::ask(approval, settled, move |allow| {
            let (Some(socket), Some(runtime)) = (socket, runtime) else { return };
            runtime.spawn(async move {
                let request = Request::DecideApproval { id, allow };
                if let Err(err) = torro_ipc::client::call(&socket, &request).await {
                    eprintln!("torroworkplace: cannot pass on the decision: {err}");
                }
            });
        });
    };
    let settled = move |id: &str| {
        let close = open.lock().unwrap_or_else(PoisonError::into_inner).remove(id);
        if let Some(close) = close {
            // Gone already when the person answered in the notification itself.
            let _ = close.send(());
        }
    };
    Notifier::new(ask, settled)
}

/// What `request` would do, in words, for the person who decides: `Delete event "Team meeting" on
/// Tue, 14 Oct 09:00 from "Work"`. In the core's language.
///
/// What is changed, moved or deleted is named from `facts`, which the core looked up by the keys
/// the command runs with ([`Facts`]); a changed title or start shows as `"old" → "new"`. The
/// client's own text appears only where it is what will be written: the title and start of a new
/// or changed item, a new name. An item the core does not hold is "an unknown event" or "an
/// unknown task", never the title the client claims.
///
/// All text, the client's and the cached one, is made harmless first ([`quote`]): one line, no
/// invisible format characters, no quotes that could end the quoted part, no placeholders, at
/// most 60 characters. Front ends still show the summary as plain text only.
pub fn summary(request: &Request, facts: &Facts) -> String {
    let today = Local::now().date_naive();
    // The display name the core knows, else the id the command runs with.
    let named = |id: &str| quote(facts.name.as_deref().unwrap_or(id));
    match request {
        Request::DeleteEvent { event } => match &facts.item {
            Some(item) => trf(
                "Delete event \"{title}\" on {when} from \"{calendar}\"",
                &[
                    ("title", &quote(&item.title)),
                    ("when", &item.start.map(|start| when(&start, today)).unwrap_or_default()),
                    ("calendar", &named(&event.collection)),
                ],
            ),
            None => trf("Delete an unknown event from \"{calendar}\"", &[("calendar", &named(&event.collection))]),
        },
        Request::SaveEvent { event } => event_saved(event, facts, today),
        Request::DeleteTodo { todo } => match &facts.item {
            Some(item) => trf(
                "Delete task \"{title}\" from \"{list}\"",
                &[("title", &quote(&item.title)), ("list", &named(&todo.collection))],
            ),
            None => trf("Delete an unknown task from \"{list}\"", &[("list", &named(&todo.collection))]),
        },
        Request::SaveTodo { todo } => todo_saved(todo, facts),
        Request::MoveTodo { .. } => match (&facts.item, &facts.name) {
            (Some(item), Some(list)) => {
                trf("Reorder task \"{title}\" in \"{list}\"", &[("title", &quote(&item.title)), ("list", &quote(list))])
            }
            _ => tr("Reorder an unknown task").to_string(),
        },
        Request::DeleteTemplate { id } => trf("Delete template \"{name}\"", &[("name", &named(id))]),
        Request::UpdateTemplate { id } => {
            trf("Overwrite template \"{name}\" with the active workspace", &[("name", &named(id))])
        }
        Request::LoadTemplate { id, mode: LoadMode::Replace } => {
            trf("Load template \"{name}\" and close the windows on the workspace", &[("name", &named(id))])
        }
        Request::LoadTemplate { id, .. } => trf("Load template \"{name}\"", &[("name", &named(id))]),
        Request::SaveTemplate { name: Some(name) } => {
            trf("Save the active workspace as template \"{name}\"", &[("name", &quote(name))])
        }
        Request::SaveTemplate { name: None } => tr("Save the active workspace as a template").to_string(),
        Request::RenameTemplate { id, name } => {
            trf("Rename template \"{name}\" to \"{new}\"", &[("name", &named(id)), ("new", &quote(name))])
        }
        Request::ConnectNextcloud { server } => trf("Connect the Nextcloud at {server}", &[("server", &quote(server))]),
        Request::ConnectGithub => tr("Connect a GitHub account").to_string(),
        Request::RemoveAccount { id } => trf("Remove the account {account}", &[("account", &named(id))]),
        Request::Sync => tr("Sync all accounts").to_string(),
        Request::SetSetting { key, value } => trf(
            "Change the setting {key} to {value}",
            &[("key", &quote(&format!("{key:?}"))), ("value", &quote(&value.to_string()))],
        ),
        Request::SetCollectionShown { collection, shown: true } => {
            trf("Show \"{collection}\" in the suite", &[("collection", &named(collection))])
        }
        Request::SetCollectionShown { collection, shown: false } => {
            trf("Hide \"{collection}\" in the suite", &[("collection", &named(collection))])
        }
        Request::UpdateSettings { .. } => tr("Replace all settings").to_string(),
        Request::OpenApp { app } => trf("Open {app}", &[("app", &quote(app))]),
        Request::CreateFilterList { name, .. } => trf("Create filter list \"{name}\"", &[("name", &quote(name))]),
        Request::UpdateFilterList { collection, name, .. } => {
            let (old, new) = (named(collection), quote(name));
            match old == new {
                true => trf("Change filter list \"{name}\"", &[("name", &old)]),
                false => trf("Change filter list \"{name}\" → \"{new}\"", &[("name", &old), ("new", &new)]),
            }
        }
        Request::DeleteFilterList { collection } => {
            trf("Delete filter list \"{list}\"", &[("list", &named(collection))])
        }
        Request::UnpinIssue { collection, url } => {
            // The item as the core will match it: repository and number from the address.
            let issue = torro_pim_github::address::issue_ref(url)
                .map_or_else(|| quote(url), |issue| quote(&format!("{}#{}", issue.repo, issue.number)));
            let list = named(collection);
            match &facts.item {
                Some(item) => trf(
                    "Unpin \"{title}\" ({item}) from filter list \"{list}\"",
                    &[("title", &quote(&item.title)), ("item", &issue), ("list", &list)],
                ),
                None => trf("Unpin {item} from filter list \"{list}\"", &[("item", &issue), ("list", &list)]),
            }
        }
        // Reads and the commands only the app key may send never wait for approval; say at least
        // which command it is.
        other => trf("Run {command}", &[("command", &other.operation().name())]),
    }
}

/// What the inbox holds about what an `inbox.*` request names, for [`inbox_summary`]. Looked up
/// in the inbox by the id the command runs with ([`crate::inbox::Inbox::facts`]), never taken
/// from the client's text, and only after the inbox's own checks passed
/// ([`crate::inbox::Inbox::refusal`]), so it names nothing the client could not see.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InboxFacts {
    /// The entry that `inbox.set_item_done`, `inbox.snooze_item` or `inbox.set_item_lane` would
    /// change, as stored. `None` when the request names none or there is none with its id: the
    /// summary then says "an unknown inbox entry".
    pub item: Option<Item>,
    /// The rule `inbox.remove_rule` would remove, as stored; `None` when unknown.
    pub rule: Option<Rule>,
}

/// What an `inbox.*` request would do, in words, like [`summary`]: `Unmute "Codex"
/// (agent:codex) in the inbox`. In the core's language.
///
/// Entries and rules are named from `facts`, looked up by the id the command runs with; a source
/// is named by its label and key, the key being what the command runs with. The client's own
/// text appears only where it is what will be written: the title of a report and a thread key.
/// All of it goes through [`quote`]. Requests that never wait for approval (reads) get the
/// general sentence of [`summary`].
pub fn inbox_summary(request: &Request, facts: &InboxFacts) -> String {
    let today = Local::now().date_naive();
    let entry = || match &facts.item {
        Some(item) => format!("\"{}\"", quote(&item.title)),
        None => tr("an unknown inbox entry").to_string(),
    };
    match request {
        // A hook's agent state is stored under a title the core writes, not the one sent along.
        Request::PostItem { kind: torro_inbox::Kind::AgentWaiting | torro_inbox::Kind::AgentDone, .. } => {
            tr("Report the state of a coding agent to the inbox").to_string()
        }
        Request::PostItem { title, .. } => trf("Report \"{title}\" to the inbox", &[("title", &quote(title))]),
        Request::ResolveItems { thread_key } => {
            trf("Mark its own inbox entries of thread {thread} as done", &[("thread", &quote(thread_key))])
        }
        Request::SetItemDone { done: true, .. } => trf("Mark {entry} as done", &[("entry", &entry())]),
        Request::SetItemDone { done: false, .. } => trf("Open {entry} again", &[("entry", &entry())]),
        Request::SnoozeItem { until: Some(until), .. } => trf(
            "Snooze {entry} until {when}",
            &[("entry", &entry()), ("when", &when(&EventTime::DateTime(*until), today))],
        ),
        Request::SnoozeItem { until: None, .. } => trf("Stop snoozing {entry}", &[("entry", &entry())]),
        Request::SetItemLane { lane, remember: false, .. } => {
            trf("Move {entry} to {lane}", &[("entry", &entry()), ("lane", &lane_name(*lane))])
        }
        Request::SetItemLane { lane, remember: true, .. } => {
            let source =
                facts.item.as_ref().map_or_else(|| tr("its source").to_string(), |item| source_name(&item.source));
            trf(
                "Move {entry} to {lane} and send later notices of {source} there too",
                &[("entry", &entry()), ("lane", &lane_name(*lane)), ("source", &source)],
            )
        }
        Request::MuteSource { source, muted: true } => {
            trf("Mute {source} in the inbox", &[("source", &source_name(source))])
        }
        Request::MuteSource { source, muted: false } => {
            trf("Unmute {source} in the inbox", &[("source", &source_name(source))])
        }
        Request::RemoveRule { .. } => match &facts.rule {
            Some(Rule { source, effect: RuleEffect::Muted, .. }) => {
                trf("Unmute {source} in the inbox", &[("source", &source_name(source))])
            }
            Some(Rule { source, effect: RuleEffect::Lane { lane }, .. }) => trf(
                "Stop sending notices of {source} to {lane}",
                &[("source", &source_name(source)), ("lane", &lane_name(*lane))],
            ),
            None => tr("Remove an unknown inbox rule").to_string(),
        },
        other => summary(other, &Facts::default()),
    }
}

/// A source for a summary: its label and its key, `"Codex" (agent:codex)`, both quoted.
fn source_name(source: &SourceRef) -> String {
    format!("\"{}\" ({})", quote(&source.label()), quote(&source.key()))
}

/// The name of a lane as the Inbox app shows it.
fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::ForMe => tr("For me"),
        Lane::Info => tr("Info"),
    }
}

/// `"old"` when the title stays, `"old" → "new"` when it changes; quoted, without the outer
/// quotes the sentence puts around it.
fn title_change(old: &str, new: &str) -> (String, Option<String>) {
    let (old, new) = (quote(old), quote(new));
    if old == new { (old, None) } else { (old, Some(new)) }
}

/// The start of an event for a summary, `old → new` when a change moves it.
fn when_change(old: Option<EventTime>, new: &EventTime, today: chrono::NaiveDate) -> String {
    match old {
        Some(old) if old != *new => format!("{} → {}", when(&old, today), when(new, today)),
        Some(old) => when(&old, today),
        None => when(new, today),
    }
}

/// The summary of `calendar.save_event`: create, change, or move to another calendar.
fn event_saved(event: &Event, facts: &Facts, today: chrono::NaiveDate) -> String {
    let calendar = quote(facts.name.as_deref().unwrap_or(&event.collection));
    let new = quote(&event.summary);
    if event.href.is_none() {
        return trf(
            "Create event \"{title}\" on {when} in \"{calendar}\"",
            &[("title", &new), ("when", &when(&event.start, today)), ("calendar", &calendar)],
        );
    }
    let Some(item) = &facts.item else {
        return match &facts.moved_from {
            Some(from) => trf(
                "Move an unknown event from \"{from}\" to \"{calendar}\"",
                &[("from", &quote(from)), ("calendar", &calendar)],
            ),
            None => trf(
                "Change an unknown event in \"{calendar}\" to \"{new}\" on {when}",
                &[("calendar", &calendar), ("new", &new), ("when", &when(&event.start, today))],
            ),
        };
    };
    let when = when_change(item.start, &event.start, today);
    let (title, renamed) = title_change(&item.title, &event.summary);
    let from = facts.moved_from.as_deref().map(quote).unwrap_or_default();
    let renamed_to = renamed.as_deref().unwrap_or_default();
    let args: [(&str, &dyn std::fmt::Display); 5] =
        [("title", &title), ("new", &renamed_to), ("when", &when), ("from", &from), ("calendar", &calendar)];
    let text = match (renamed.is_some(), facts.moved_from.is_some()) {
        (false, false) => "Change event \"{title}\" on {when} in \"{calendar}\"",
        (true, false) => "Change event \"{title}\" → \"{new}\" on {when} in \"{calendar}\"",
        (false, true) => "Move event \"{title}\" on {when} from \"{from}\" to \"{calendar}\"",
        (true, true) => "Move event \"{title}\" → \"{new}\" on {when} from \"{from}\" to \"{calendar}\"",
    };
    trf(text, &args)
}

/// The summary of `tasks.save_todo`: create, change, or move to another list.
fn todo_saved(todo: &Todo, facts: &Facts) -> String {
    let list = quote(facts.name.as_deref().unwrap_or(&todo.collection));
    let new = quote(&todo.summary);
    if todo.href.is_none() {
        return trf("Create task \"{title}\" in \"{list}\"", &[("title", &new), ("list", &list)]);
    }
    let Some(item) = &facts.item else {
        return match &facts.moved_from {
            Some(from) => {
                trf("Move an unknown task from \"{from}\" to \"{list}\"", &[("from", &quote(from)), ("list", &list)])
            }
            None => trf("Change an unknown task in \"{list}\" to \"{new}\"", &[("list", &list), ("new", &new)]),
        };
    };
    let (title, renamed) = title_change(&item.title, &todo.summary);
    let from = facts.moved_from.as_deref().map(quote).unwrap_or_default();
    let renamed_to = renamed.as_deref().unwrap_or_default();
    let args: [(&str, &dyn std::fmt::Display); 4] =
        [("title", &title), ("new", &renamed_to), ("from", &from), ("list", &list)];
    let text = match (renamed.is_some(), facts.moved_from.is_some()) {
        (false, false) => "Change task \"{title}\" in \"{list}\"",
        (true, false) => "Change task \"{title}\" → \"{new}\" in \"{list}\"",
        (false, true) => "Move task \"{title}\" from \"{from}\" to \"{list}\"",
        (true, true) => "Move task \"{title}\" → \"{new}\" from \"{from}\" to \"{list}\"",
    };
    trf(text, &args)
}

/// The start of an event for a summary: `Tue, 14 Oct`, with the local time for a timed one.
fn when(start: &EventTime, today: chrono::NaiveDate) -> String {
    match start {
        EventTime::Date(date) => format_short_date(*date, today),
        EventTime::DateTime(at) => {
            let local = to_local(*at);
            format!("{} {}", format_short_date(local.date(), today), format_time(local.time()))
        }
    }
}

/// A client's text made harmless for a summary: invisible format characters go ([`is_format`]),
/// control characters and line breaks become spaces, runs of spaces one, double quotes single ones
/// (so the text cannot end the quoted part and pretend to be the core's words), braces round ones
/// (so it cannot fill a placeholder), and at most [`QUOTE_MAX`] characters with `…`.
fn quote(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !is_format(*c))
        .map(|c| match c {
            '"' | '“' | '”' | '„' | '‟' | '＂' => '\'',
            '{' => '(',
            '}' => ')',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let one_line = words.join(" ");
    match one_line.char_indices().nth(QUOTE_MAX) {
        Some((cut, _)) => format!("{}…", &one_line[..cut]),
        None => one_line,
    }
}

/// A Unicode format character (general category Cf, Unicode 16): invisible, yet able to change
/// what a person reads. Bidi controls turn `Delete "gnihton"` around on screen, zero-width ones
/// join or split words unseen, tag characters carry hidden text. Rust's standard library has no
/// category lookup, so the ranges are listed here.
fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use chrono::TimeZone;

    use super::*;

    /// Approvals in `dir/state/approvals.json` that show no notification.
    pub(crate) fn quiet_in(dir: &Path) -> Approvals {
        Approvals::new(Some(file_in(dir)), Notifier::asking(|_: &ApprovalInfo| {}))
    }

    pub(crate) fn file_in(dir: &Path) -> PathBuf {
        dir.join("state").join(FILE)
    }

    /// What a fake notifier saw: the ids asked about and the ids settled, in order.
    #[derive(Default)]
    struct Seen {
        asked: Mutex<Vec<String>>,
        settled: Mutex<Vec<String>>,
    }

    /// Approvals in `dir` with a notifier that only records into the returned [`Seen`].
    fn recorded_in(dir: &Path) -> (Approvals, Arc<Seen>) {
        let seen = Arc::new(Seen::default());
        let (asked, settled) = (Arc::clone(&seen), Arc::clone(&seen));
        let notify = Notifier::new(
            move |info: &ApprovalInfo| asked.asked.lock().expect("lock").push(info.id.clone()),
            move |id: &str| settled.settled.lock().expect("lock").push(id.to_string()),
        );
        (Approvals::new(Some(file_in(dir)), notify), seen)
    }

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 9, minute, 0).single().expect("time")
    }

    fn delete(id: &str) -> Request {
        Request::DeleteTemplate { id: id.into() }
    }

    #[tokio::test]
    async fn an_approval_survives_a_restart_and_expires_after_ten_minutes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let opened = approvals.open("c1", "Claude Code", delete("dev"), "Delete".into(), at(0)).await.expect("opened");
        assert_eq!(opened.state, ApprovalState::Pending);
        assert_eq!(opened.expires, at(10));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(file_in(dir.path())).expect("written").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // A new core reads it back, still open a minute before it expires.
        let restarted = quiet_in(dir.path());
        assert_eq!(restarted.status(Some("c1"), &opened.id, at(9)).await.expect("kept"), opened);
        let expired = restarted.status(Some("c1"), &opened.id, at(10)).await.expect("kept");
        assert_eq!((expired.state, expired.decided), (ApprovalState::Expired, Some(at(10))));
        assert!(matches!(restarted.decide(&opened.id, true, at(11)).await, Err(ApprovalsError::NotOpen(_))));

        // A day after it expired it is gone.
        let later = at(10) + KEPT;
        assert!(matches!(restarted.status(None, &opened.id, later).await, Err(ApprovalsError::NotFound(_))));
    }

    #[tokio::test]
    async fn a_client_sees_only_its_own_approvals_and_the_app_key_all() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let mine = approvals.open("c1", "One", delete("a"), "a".into(), at(0)).await.expect("opened");
        let theirs = approvals.open("c2", "Two", delete("b"), "b".into(), at(1)).await.expect("opened");

        let ids = |list: Vec<ApprovalInfo>| list.into_iter().map(|info| info.id).collect::<Vec<_>>();
        assert_eq!(ids(approvals.list(Some("c1"), at(2)).await.expect("list")), std::slice::from_ref(&mine.id));
        assert_eq!(ids(approvals.list(None, at(2)).await.expect("list")), [theirs.id.clone(), mine.id], "newest first");
        assert!(matches!(approvals.status(Some("c1"), &theirs.id, at(2)).await, Err(ApprovalsError::NotFound(_))));
    }

    #[tokio::test]
    async fn a_decision_is_taken_once_and_the_outcome_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let first = approvals.open("c1", "One", delete("a"), "a".into(), at(0)).await.expect("opened");
        let second = approvals.open("c1", "One", delete("b"), "b".into(), at(0)).await.expect("opened");

        let Ok(Decision::Run { client, request }) = approvals.decide(&first.id, true, at(1)).await else {
            panic!("allowed runs");
        };
        assert_eq!((client.as_str(), *request), ("c1", delete("a")));
        assert_eq!(approvals.status(None, &first.id, at(1)).await.expect("kept").state, ApprovalState::Running);
        let done = approvals.finish(&first.id, Response::Ok, at(1)).await.expect("finished");
        assert_eq!(done.state, ApprovalState::Done { response: Box::new(Response::Ok) });
        assert!(matches!(approvals.decide(&first.id, false, at(2)).await, Err(ApprovalsError::NotOpen(_))));

        let Ok(Decision::Denied(denied)) = approvals.decide(&second.id, false, at(2)).await else { panic!("denied") };
        assert_eq!((denied.state, denied.decided), (ApprovalState::Denied, Some(at(2))));
        assert!(matches!(approvals.decide("nope", true, at(2)).await, Err(ApprovalsError::NotFound(_))));

        // Outcomes survive a restart too.
        let restarted = quiet_in(dir.path());
        assert_eq!(restarted.status(None, &first.id, at(3)).await.expect("kept"), done);
    }

    #[tokio::test]
    async fn a_command_running_when_the_core_stopped_never_runs_again() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let opened = approvals.open("c1", "One", delete("a"), "a".into(), at(0)).await.expect("opened");
        assert!(matches!(approvals.decide(&opened.id, true, at(1)).await, Ok(Decision::Run { .. })));

        let restarted = quiet_in(dir.path());
        let info = restarted.status(None, &opened.id, at(2)).await.expect("kept");
        let ApprovalState::Done { response } = info.state else { panic!("done, got {:?}", info.state) };
        assert!(matches!(*response, Response::Error { ref code, .. } if code == "error"));
    }

    #[tokio::test]
    async fn a_secret_taking_command_never_becomes_an_approval() {
        let dir = tempfile::tempdir().expect("temp dir");
        let shown = Arc::new(Mutex::new(0));
        let counter = Arc::clone(&shown);
        let notify = Notifier::asking(move |_: &ApprovalInfo| *counter.lock().expect("lock") += 1);
        let approvals = Approvals::new(Some(file_in(dir.path())), notify);

        let token = Request::SetGithubToken { token: torro_ipc::Secret::new("ghp_secret") };
        let refused = approvals.open("c1", "One", token, "x".into(), at(0)).await;
        assert!(matches!(refused, Err(ApprovalsError::KeptBack("accounts.set_github_token"))));
        let decide = Request::DecideApproval { id: "x".into(), allow: true };
        assert!(matches!(
            approvals.open("c1", "One", decide, "x".into(), at(0)).await,
            Err(ApprovalsError::KeptBack(_))
        ));
        assert!(!file_in(dir.path()).exists(), "nothing stored");
        assert_eq!(*shown.lock().expect("lock"), 0, "nothing announced");

        approvals.open("c1", "One", delete("a"), "a".into(), at(0)).await.expect("opened");
        assert_eq!(*shown.lock().expect("lock"), 1, "a new approval is announced once");
        let stored = fs::read_to_string(file_in(dir.path())).expect("stored");
        assert!(!stored.contains("ghp_secret"));
    }

    #[tokio::test]
    async fn a_client_cannot_flood_the_person_with_questions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        for n in 0..OPEN_MAX {
            approvals.open("c1", "One", delete(&n.to_string()), "x".into(), at(0)).await.expect("opened");
        }
        let refused = approvals.open("c1", "One", delete("more"), "x".into(), at(0)).await;
        assert!(matches!(refused, Err(ApprovalsError::TooMany)));
        assert_eq!(ApprovalsError::TooMany.code(), "busy");
        approvals.open("c2", "Two", delete("other"), "x".into(), at(0)).await.expect("other clients still ask");
        // Once they expired it may ask again.
        approvals.open("c1", "One", delete("later"), "x".into(), at(10)).await.expect("opened after expiry");
    }

    #[tokio::test]
    async fn an_approval_that_cannot_be_stored_is_not_opened() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A file where the state directory should be: nothing can be written below it.
        fs::write(dir.path().join("state"), "not a directory").expect("blocker");
        let approvals = quiet_in(dir.path());
        let refused = approvals.open("c1", "One", delete("a"), "a".into(), at(0)).await;
        assert!(matches!(refused, Err(ApprovalsError::Io { .. })), "{refused:?}");
    }

    #[tokio::test]
    async fn a_damaged_file_is_moved_aside_and_approvals_go_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::create_dir_all(dir.path().join("state")).expect("dir");
        fs::write(file_in(dir.path()), "{ not json").expect("damage");
        let approvals = quiet_in(dir.path());
        assert!(approvals.list(None, at(0)).await.expect("list").is_empty());
        assert!(dir.path().join("state").join("approvals.json.damaged").exists());
        approvals.open("c1", "One", delete("a"), "a".into(), at(0)).await.expect("opened");
    }

    #[tokio::test]
    async fn open_approvals_are_asked_about_again_when_the_core_starts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let early = approvals.open("c1", "One", delete("a"), "a".into(), at(0)).await.expect("opened");
        let open = approvals.open("c1", "One", delete("b"), "b".into(), at(5)).await.expect("opened");
        let decided = approvals.open("c1", "One", delete("c"), "c".into(), at(5)).await.expect("opened");
        approvals.decide(&decided.id, false, at(6)).await.expect("denied");

        // A new core asks again about the one still open; `early` expired at minute 10.
        let (restarted, seen) = recorded_in(dir.path());
        assert_eq!(restarted.announce_open(at(11)).await.expect("announced"), 1);
        assert_eq!(*seen.asked.lock().expect("lock"), std::slice::from_ref(&open.id));
        assert_ne!(early.id, open.id);
        assert_eq!(restarted.announce_open(at(15)).await.expect("announced"), 0, "expired too by now");

        // Without a file there is nothing to ask about, and nothing fails.
        assert_eq!(Approvals::system().announce_open(at(0)).await.expect("nothing"), 0);
    }

    #[tokio::test]
    async fn a_decision_takes_the_question_off_the_desktop() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (approvals, seen) = recorded_in(dir.path());
        let first = approvals.open("c1", "One", delete("a"), "a".into(), at(0)).await.expect("opened");
        let second = approvals.open("c1", "One", delete("b"), "b".into(), at(0)).await.expect("opened");
        assert_eq!(*seen.asked.lock().expect("lock"), [first.id.clone(), second.id.clone()]);
        assert!(seen.settled.lock().expect("lock").is_empty());

        approvals.decide(&second.id, true, at(1)).await.expect("allowed");
        approvals.decide(&first.id, false, at(1)).await.expect("denied");
        assert_eq!(*seen.settled.lock().expect("lock"), [second.id.clone(), first.id.clone()]);
        // One that is no longer open settles nothing again.
        assert!(approvals.decide(&first.id, true, at(2)).await.is_err());
        assert_eq!(seen.settled.lock().expect("lock").len(), 2);
    }

    #[tokio::test]
    async fn a_client_without_a_name_is_shown_by_its_id() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let nameless = approvals.open("c1", " ", delete("a"), "a".into(), at(0)).await.expect("opened");
        assert_eq!(nameless.client_name, "c1");
        let named = approvals.open("c1", "Claude Code", delete("b"), "b".into(), at(0)).await.expect("opened");
        assert_eq!(named.client_name, "Claude Code");
    }

    #[test]
    fn file_errors_reach_the_client_without_a_path() {
        let err = ApprovalsError::Io {
            path: PathBuf::from("/home/anna/.local/state/torroworkplace/approvals.json"),
            err: io::Error::other("disk full"),
        };
        let Response::Error { code, message } = err.response() else { panic!("an error") };
        assert_eq!(code, "error");
        assert!(!message.contains("/home/anna") && !message.contains("approvals.json"), "{message}");
        assert!(ApprovalsError::NotFound("x".into()).to_string().contains("\"x\""), "other errors stay as they are");
    }

    #[test]
    fn client_text_in_a_summary_stays_one_harmless_line() {
        assert_eq!(quote("Team\nmeeting \"now\"  {when}"), "Team meeting 'now' (when)");
        let long = "x".repeat(100);
        assert_eq!(quote(&long), format!("{}…", "x".repeat(QUOTE_MAX)));
        assert_eq!(quote("äöü"), "äöü");
        assert_eq!(quote("Ａ＂Ｂ"), "Ａ'Ｂ", "a fullwidth quote ends nothing either");
    }

    #[test]
    fn invisible_format_characters_are_removed() {
        // Bidi controls would show "Delete 'gnihton'" turned around; each one goes.
        for control in ['\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}']
        {
            assert_eq!(quote(&format!("Lunch{control}gnihton\u{2069}")), "Lunchgnihton", "{:04X}", u32::from(control));
        }
        // Zero-width characters, joiners and marks, the byte order mark, soft hyphen, tag text.
        for hidden in ['\u{200B}', '\u{200C}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{2060}', '\u{FEFF}', '\u{00AD}'] {
            assert_eq!(quote(&format!("Team{hidden}meeting")), "Teammeeting", "{:04X}", u32::from(hidden));
        }
        assert_eq!(quote("ok\u{E0001}\u{E0069}\u{E0067}\u{E007F}"), "ok");
        assert_eq!(quote("\u{200B}\u{200B}"), "", "nothing visible stays nothing");
        // Visible text that only looks unusual stays.
        assert_eq!(quote("Café ☕ 東京"), "Café ☕ 東京");
    }

    fn event(collection: &str, href: Option<&str>, title: &str, start: EventTime) -> Event {
        Event {
            uid: "u".into(),
            collection: collection.into(),
            href: href.map(str::to_string),
            etag: Some("1".into()),
            summary: title.into(),
            start,
            end: None,
            location: None,
            description: None,
            recurring: false,
        }
    }

    fn todo(collection: &str, href: Option<&str>, title: &str) -> Todo {
        Todo {
            uid: "t".into(),
            collection: collection.into(),
            href: href.map(str::to_string),
            etag: Some("1".into()),
            summary: title.into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
        }
    }

    fn day(day: u32) -> EventTime {
        EventTime::Date(chrono::NaiveDate::from_ymd_opt(2026, 10, day).expect("date"))
    }

    fn cached(title: &str, start: Option<EventTime>) -> Option<Known> {
        Some(Known { title: title.into(), start })
    }

    fn facts(item: Option<Known>, name: &str) -> Facts {
        Facts { item, name: Some(name.into()), moved_from: None }
    }

    #[test]
    fn a_summary_names_the_item_that_runs_not_the_one_the_client_claims() {
        let today = Local::now().date_naive();
        let claimed = event("c", Some("/cal/board.ics"), "Lunch", day(20));
        let real = facts(cached("Board meeting", Some(day(14))), "Work");
        assert_eq!(
            summary(&Request::DeleteEvent { event: claimed.clone() }, &real),
            trf(
                "Delete event \"{title}\" on {when} from \"{calendar}\"",
                &[("title", &"Board meeting"), ("when", &when(&day(14), today)), ("calendar", &"Work")]
            )
        );
        let unknown = summary(&Request::DeleteEvent { event: claimed }, &facts(None, "Work"));
        assert_eq!(unknown, trf("Delete an unknown event from \"{calendar}\"", &[("calendar", &"Work")]));
        assert!(!unknown.contains("Lunch"));

        let claimed = todo("l", Some("/tasks/taxes.ics"), "Water plants");
        let real = facts(cached("File taxes", None), "Home");
        assert_eq!(
            summary(&Request::DeleteTodo { todo: claimed.clone() }, &real),
            trf("Delete task \"{title}\" from \"{list}\"", &[("title", &"File taxes"), ("list", &"Home")])
        );
        assert_eq!(
            summary(&Request::DeleteTodo { todo: claimed }, &facts(None, "Home")),
            trf("Delete an unknown task from \"{list}\"", &[("list", &"Home")])
        );
        let reorder = Request::MoveTodo { href: "/tasks/taxes.ics".into(), etag: "1".into(), before: None };
        assert_eq!(
            summary(&reorder, &real),
            trf("Reorder task \"{title}\" in \"{list}\"", &[("title", &"File taxes"), ("list", &"Home")])
        );
        assert_eq!(summary(&reorder, &Facts::default()), tr("Reorder an unknown task"));
    }

    #[test]
    fn a_change_shows_old_and_new() {
        let today = Local::now().date_naive();
        let changed = event("c", Some("/cal/board.ics"), "Board meeting (moved)", day(15));
        let real = facts(cached("Board meeting", Some(day(14))), "Work");
        let moved_day = format!("{} → {}", when(&day(14), today), when(&day(15), today));
        assert_eq!(
            summary(&Request::SaveEvent { event: changed.clone() }, &real),
            trf(
                "Change event \"{title}\" → \"{new}\" on {when} in \"{calendar}\"",
                &[
                    ("title", &"Board meeting"),
                    ("new", &"Board meeting (moved)"),
                    ("when", &moved_day),
                    ("calendar", &"Work")
                ]
            )
        );
        let same = event("c", Some("/cal/board.ics"), "Board meeting", day(14));
        assert_eq!(
            summary(&Request::SaveEvent { event: same.clone() }, &real),
            trf(
                "Change event \"{title}\" on {when} in \"{calendar}\"",
                &[("title", &"Board meeting"), ("when", &when(&day(14), today)), ("calendar", &"Work")]
            )
        );
        let moving = Facts { moved_from: Some("Home".into()), ..real.clone() };
        assert_eq!(
            summary(&Request::SaveEvent { event: same.clone() }, &moving),
            trf(
                "Move event \"{title}\" on {when} from \"{from}\" to \"{calendar}\"",
                &[
                    ("title", &"Board meeting"),
                    ("when", &when(&day(14), today)),
                    ("from", &"Home"),
                    ("calendar", &"Work")
                ]
            )
        );
        assert_eq!(
            summary(&Request::SaveEvent { event: same }, &facts(None, "Work")),
            trf(
                "Change an unknown event in \"{calendar}\" to \"{new}\" on {when}",
                &[("calendar", &"Work"), ("new", &"Board meeting"), ("when", &when(&day(14), today))]
            )
        );
        let new = event("c", None, "Lunch", day(14));
        assert_eq!(
            summary(&Request::SaveEvent { event: new }, &facts(None, "Work")),
            trf(
                "Create event \"{title}\" on {when} in \"{calendar}\"",
                &[("title", &"Lunch"), ("when", &when(&day(14), today)), ("calendar", &"Work")]
            )
        );

        let renamed = todo("l", Some("/tasks/taxes.ics"), "File taxes today");
        let real = facts(cached("File taxes", None), "Home");
        assert_eq!(
            summary(&Request::SaveTodo { todo: renamed.clone() }, &real),
            trf(
                "Change task \"{title}\" → \"{new}\" in \"{list}\"",
                &[("title", &"File taxes"), ("new", &"File taxes today"), ("list", &"Home")]
            )
        );
        let moving = Facts { moved_from: Some("Work".into()), ..real };
        assert_eq!(
            summary(&Request::SaveTodo { todo: renamed }, &moving),
            trf(
                "Move task \"{title}\" → \"{new}\" from \"{from}\" to \"{list}\"",
                &[("title", &"File taxes"), ("new", &"File taxes today"), ("from", &"Work"), ("list", &"Home")]
            )
        );
    }

    #[test]
    fn collections_templates_and_accounts_are_shown_by_name() {
        let named = |name: &str| Facts { name: Some(name.into()), ..Facts::default() };
        assert_eq!(
            summary(&delete("dev"), &named("Development")),
            trf("Delete template \"{name}\"", &[("name", &"Development")])
        );
        assert_eq!(summary(&delete("dev"), &Facts::default()), trf("Delete template \"{name}\"", &[("name", &"dev")]));
        let rename = Request::RenameTemplate { id: "dev".into(), name: "Review".into() };
        assert_eq!(
            summary(&rename, &named("Development")),
            trf("Rename template \"{name}\" to \"{new}\"", &[("name", &"Development"), ("new", &"Review")])
        );
        let hide = Request::SetCollectionShown { collection: "anna/https://x/cal/work/".into(), shown: false };
        assert_eq!(
            summary(&hide, &named("Work")),
            trf("Hide \"{collection}\" in the suite", &[("collection", &"Work")])
        );
        let remove = Request::RemoveAccount { id: "nextcloud:anna@cloud.example.org".into() };
        assert_eq!(
            summary(&remove, &named("anna@cloud.example.org")),
            trf("Remove the account {account}", &[("account", &"anna@cloud.example.org")])
        );
        let update = Request::UpdateFilterList {
            collection: "github:anna/1".into(),
            revision: 1,
            name: "Bugs".into(),
            color: None,
            queries: vec![],
        };
        assert_eq!(
            summary(&update, &named("Open bugs")),
            trf("Change filter list \"{name}\" → \"{new}\"", &[("name", &"Open bugs"), ("new", &"Bugs")])
        );
        let unpin = Request::UnpinIssue {
            collection: "github:anna/1".into(),
            url: "https://github.com/torro/app/issues/12".into(),
        };
        assert_eq!(
            summary(&unpin, &Facts { item: cached("Crash on start", None), ..named("Open bugs") }),
            trf(
                "Unpin \"{title}\" ({item}) from filter list \"{list}\"",
                &[("title", &"Crash on start"), ("item", &"torro/app#12"), ("list", &"Open bugs")]
            )
        );
        assert_eq!(summary(&Request::Sync, &Facts::default()), tr("Sync all accounts"));
        assert_eq!(
            summary(&Request::Todos, &Facts::default()),
            trf("Run {command}", &[("command", &"tasks.list_todos")])
        );
    }

    #[test]
    fn cached_text_is_made_harmless_too() {
        let real = facts(cached("Board\u{202E}gniteem \"x\"", Some(day(14))), "Wo{rk}");
        let text = summary(&Request::DeleteEvent { event: event("c", Some("/a.ics"), "Lunch", day(14)) }, &real);
        assert!(text.contains("Boardgniteem 'x'") && text.contains("Wo(rk)"), "{text}");
    }

    /// An inbox entry of the script `backup`, as the inbox stores it.
    fn inbox_entry(title: &str) -> Item {
        let at = Utc.with_ymd_and_hms(2026, 10, 12, 8, 0, 0).single().expect("time");
        Item {
            id: "e1".into(),
            source: SourceRef::script("backup"),
            thread_key: "t".into(),
            kind: torro_inbox::Kind::Notice,
            lane: Lane::Info,
            urgent: false,
            title: title.into(),
            body: None,
            from: None,
            agent: None,
            received: at,
            updated: at,
            count: 1,
            state: torro_inbox::State::New,
            link: None,
            actions: Vec::new(),
            untrusted: true,
        }
    }

    #[test]
    fn inbox_summaries_tell_each_command_apart_and_name_what_the_inbox_holds() {
        let source = SourceRef::agent("codex");
        let named = "\"Codex\" (agent:codex)";
        let mute =
            |muted| inbox_summary(&Request::MuteSource { source: source.clone(), muted }, &InboxFacts::default());
        assert_eq!(mute(true), trf("Mute {source} in the inbox", &[("source", &named)]));
        assert_eq!(mute(false), trf("Unmute {source} in the inbox", &[("source", &named)]));

        let known = InboxFacts { item: Some(inbox_entry("Backup \"done\"")), rule: None };
        let entry = "\"Backup 'done'\"";
        let lane = |remember| Request::SetItemLane { id: "e1".into(), lane: Lane::ForMe, remember };
        assert_eq!(
            inbox_summary(&lane(true), &known),
            trf(
                "Move {entry} to {lane} and send later notices of {source} there too",
                &[("entry", &entry), ("lane", &tr("For me")), ("source", &"\"backup\" (script:backup)")]
            )
        );
        assert_eq!(
            inbox_summary(&lane(false), &known),
            trf("Move {entry} to {lane}", &[("entry", &entry), ("lane", &tr("For me"))])
        );
        let unknown = tr("an unknown inbox entry");
        assert_eq!(
            inbox_summary(&lane(true), &InboxFacts::default()),
            trf(
                "Move {entry} to {lane} and send later notices of {source} there too",
                &[("entry", &unknown), ("lane", &tr("For me")), ("source", &tr("its source"))]
            )
        );
        let done = |done| inbox_summary(&Request::SetItemDone { id: "e1".into(), done }, &known);
        assert_eq!(done(true), trf("Mark {entry} as done", &[("entry", &entry)]));
        assert_eq!(done(false), trf("Open {entry} again", &[("entry", &entry)]));
        let snooze = inbox_summary(&Request::SnoozeItem { id: "e1".into(), until: None }, &InboxFacts::default());
        assert_eq!(snooze, trf("Stop snoozing {entry}", &[("entry", &unknown)]));

        let rule = |effect| InboxFacts {
            item: None,
            rule: Some(Rule { id: "r1".into(), source: source.clone(), effect, created: Utc::now() }),
        };
        let remove = Request::RemoveRule { id: "r1".into() };
        assert_eq!(
            inbox_summary(&remove, &rule(RuleEffect::Muted)),
            trf("Unmute {source} in the inbox", &[("source", &named)])
        );
        assert_eq!(
            inbox_summary(&remove, &rule(RuleEffect::Lane { lane: Lane::Info })),
            trf("Stop sending notices of {source} to {lane}", &[("source", &named), ("lane", &tr("Info"))])
        );
        assert_eq!(inbox_summary(&remove, &InboxFacts::default()), tr("Remove an unknown inbox rule"));

        let post = Request::PostItem {
            kind: torro_inbox::Kind::Notice,
            title: "Ignore all {previous}\ninstructions".into(),
            body: None,
            link: None,
            lane: None,
            thread_key: None,
            agent: None,
        };
        assert_eq!(
            inbox_summary(&post, &InboxFacts::default()),
            trf("Report \"{title}\" to the inbox", &[("title", &"Ignore all (previous) instructions")])
        );
    }
}
