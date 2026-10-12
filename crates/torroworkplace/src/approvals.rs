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
//! Only the app key decides ([`torro_ipc::Request::DecideApproval`]): the control centre, or the
//! desktop notification with "Allow" and "Deny" that every new approval shows ([`Approvals::system`]),
//! whose click goes to the core with the app key. The agent never gets a code it could confirm
//! with. A command kept from agents ([`Mcp::Never`]: taking secrets, managing clients, deciding
//! approvals) never becomes an approval, so a secret never reaches `approvals.json`.
//!
//! What is stored is the request itself, with the client's text in it; the file is for the owner
//! only. Changes are made under one lock and written before they count: an approval that cannot be
//! written is not opened (the core refuses the command), a decision that cannot be written is not
//! taken.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::paths::AppPaths;
use torro_gui::datetime::{format_short_date, format_time, to_local};
use torro_ipc::api::Mcp;
use torro_ipc::{ApprovalInfo, ApprovalState, Request, Response};
use torro_pim::EventTime;
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

/// Called once for every new approval, after it was stored: shows the person the notification.
pub type Notifier = Arc<dyn Fn(&ApprovalInfo) + Send + Sync>;

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

    /// The error as an API answer.
    pub fn response(&self) -> Response {
        Response::Error { code: self.code().into(), message: self.to_string() }
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
            return Approvals::new(None, Arc::new(|_: &ApprovalInfo| {}));
        }
        let file = AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join(FILE));
        Approvals::new(file, desktop_notifier(torro_ipc::socket_path()))
    }

    /// Approvals kept in `file`, announced with `notify`. Tests pass a temporary file and a
    /// notifier that shows nothing.
    pub fn new(file: Option<PathBuf>, notify: Notifier) -> Approvals {
        Approvals { file, notify, state: tokio::sync::Mutex::new(State::default()) }
    }

    /// Opens an approval for `request` of the client `client` (named `client_name`), stores it and
    /// announces it. `summary` says in words what would happen ([`summary`]).
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
            client_name: client_name.to_string(),
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
        (self.notify)(&info);
        Ok(info)
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
fn desktop_notifier(socket: Option<PathBuf>) -> Notifier {
    Arc::new(move |approval: &ApprovalInfo| {
        let runtime = tokio::runtime::Handle::try_current().ok();
        let (socket, id) = (socket.clone(), approval.id.clone());
        crate::notify::ask(approval, move |allow| {
            let (Some(socket), Some(runtime)) = (socket, runtime) else { return };
            runtime.spawn(async move {
                let request = Request::DecideApproval { id, allow };
                if let Err(err) = torro_ipc::client::call(&socket, &request).await {
                    eprintln!("torroworkplace: cannot pass on the decision: {err}");
                }
            });
        });
    })
}

/// What `request` would do, in words, for the person who decides: `Delete event "Team meeting" on
/// Tue, 14 Oct 09:00`. In the core's language.
///
/// Titles, names and ids come from the client, so they are made harmless first ([`quote`]): one
/// line, no quotes that could end the quoted part, no placeholders, at most 60 characters. Front
/// ends still show the summary as plain text only.
pub fn summary(request: &Request) -> String {
    let today = Local::now().date_naive();
    match request {
        Request::DeleteEvent { event } => trf(
            "Delete event \"{title}\" on {when}",
            &[("when", &when(&event.start, today)), ("title", &quote(&event.summary))],
        ),
        Request::SaveEvent { event } => {
            let text = match event.href {
                Some(_) => "Change event \"{title}\" on {when}",
                None => "Create event \"{title}\" on {when}",
            };
            trf(text, &[("when", &when(&event.start, today)), ("title", &quote(&event.summary))])
        }
        Request::DeleteTodo { todo } => trf("Delete task \"{title}\"", &[("title", &quote(&todo.summary))]),
        Request::SaveTodo { todo } => {
            let text = if todo.href.is_some() { "Change task \"{title}\"" } else { "Create task \"{title}\"" };
            trf(text, &[("title", &quote(&todo.summary))])
        }
        Request::MoveTodo { .. } => tr("Move a task in its list").to_string(),
        Request::DeleteTemplate { id } => trf("Delete template \"{name}\"", &[("name", &quote(id))]),
        Request::UpdateTemplate { id } => {
            trf("Overwrite template \"{name}\" with the active workspace", &[("name", &quote(id))])
        }
        Request::LoadTemplate { id, mode: LoadMode::Replace } => {
            trf("Load template \"{name}\" and close the windows on the workspace", &[("name", &quote(id))])
        }
        Request::LoadTemplate { id, .. } => trf("Load template \"{name}\"", &[("name", &quote(id))]),
        Request::SaveTemplate { name: Some(name) } => {
            trf("Save the active workspace as template \"{name}\"", &[("name", &quote(name))])
        }
        Request::SaveTemplate { name: None } => tr("Save the active workspace as a template").to_string(),
        Request::RenameTemplate { id, name } => {
            trf("Rename template \"{name}\" to \"{new}\"", &[("name", &quote(id)), ("new", &quote(name))])
        }
        Request::ConnectNextcloud { server } => trf("Connect the Nextcloud at {server}", &[("server", &quote(server))]),
        Request::ConnectGithub => tr("Connect a GitHub account").to_string(),
        Request::RemoveAccount { id } => trf("Remove the account {account}", &[("account", &quote(id))]),
        Request::Sync => tr("Sync all accounts").to_string(),
        Request::SetSetting { key, value } => trf(
            "Change the setting {key} to {value}",
            &[("key", &quote(&format!("{key:?}"))), ("value", &quote(&value.to_string()))],
        ),
        Request::SetCollectionShown { collection, shown: true } => {
            trf("Show {collection} in the suite", &[("collection", &quote(collection))])
        }
        Request::SetCollectionShown { collection, shown: false } => {
            trf("Hide {collection} in the suite", &[("collection", &quote(collection))])
        }
        Request::UpdateSettings { .. } => tr("Replace all settings").to_string(),
        Request::OpenApp { app } => trf("Open {app}", &[("app", &quote(app))]),
        Request::CreateFilterList { name, .. } => trf("Create filter list \"{name}\"", &[("name", &quote(name))]),
        Request::UpdateFilterList { name, .. } => trf("Change filter list \"{name}\"", &[("name", &quote(name))]),
        Request::DeleteFilterList { collection } => trf("Delete filter list {list}", &[("list", &quote(collection))]),
        Request::UnpinIssue { collection, url } => {
            trf("Unpin {item} from filter list {list}", &[("item", &quote(url)), ("list", &quote(collection))])
        }
        // Reads and the commands only the app key may send never wait for approval; say at least
        // which command it is.
        other => trf("Run {command}", &[("command", &other.operation().name())]),
    }
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

/// A client's text made harmless for a summary: control characters and line breaks become
/// spaces, runs of spaces one, double quotes single ones (so the text cannot end the quoted part
/// and pretend to be the core's words), braces round ones (so it cannot fill a placeholder), and
/// at most [`QUOTE_MAX`] characters with `…`.
fn quote(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| match c {
            '"' | '“' | '”' | '„' => '\'',
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

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use chrono::TimeZone;

    use super::*;

    /// Approvals in `dir/state/approvals.json` that show no notification.
    pub(crate) fn quiet_in(dir: &Path) -> Approvals {
        Approvals::new(Some(file_in(dir)), Arc::new(|_: &ApprovalInfo| {}))
    }

    pub(crate) fn file_in(dir: &Path) -> PathBuf {
        dir.join("state").join(FILE)
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
        let notify: Notifier = Arc::new(move |_: &ApprovalInfo| *counter.lock().expect("lock") += 1);
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

    #[test]
    fn client_text_in_a_summary_stays_one_harmless_line() {
        assert_eq!(quote("Team\nmeeting \"now\"  {when}"), "Team meeting 'now' (when)");
        let long = "x".repeat(100);
        assert_eq!(quote(&long), format!("{}…", "x".repeat(QUOTE_MAX)));
        assert_eq!(quote("äöü"), "äöü");
    }

    #[test]
    fn summaries_say_what_would_happen() {
        let start = EventTime::Date(chrono::NaiveDate::from_ymd_opt(2026, 10, 14).expect("date"));
        let event = torro_pim::Event {
            uid: "u".into(),
            collection: "c".into(),
            href: Some("/cal/u.ics".into()),
            etag: Some("1".into()),
            summary: "Team meeting\" and remove all accounts".into(),
            start,
            end: None,
            location: None,
            description: None,
            recurring: false,
        };
        let today = Local::now().date_naive();
        assert_eq!(
            summary(&Request::DeleteEvent { event }),
            trf(
                "Delete event \"{title}\" on {when}",
                &[("when", &when(&start, today)), ("title", &"Team meeting' and remove all accounts")]
            )
        );
        assert_eq!(summary(&delete("dev")), trf("Delete template \"{name}\"", &[("name", &"dev")]));
        assert_eq!(summary(&Request::Sync), tr("Sync all accounts"));
        assert_eq!(summary(&Request::Todos), trf("Run {command}", &[("command", &"tasks.list_todos")]));
    }
}
