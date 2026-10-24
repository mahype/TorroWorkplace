//! The processes and connections of the MCP servers (task MA.4): one entry per server that starts it when it
//! is needed, keeps it within its time limits, ends it when idle, starts it again after a crash a few times,
//! and ends the whole tree of processes with it.
//!
//! The rules (`docs/tasks/mcp-anbindungen.md`, "Prozess"), with the times in [`Timing`] so tests can make them
//! short:
//!
//! - **Start when needed:** the first call or tool list ([`Supervisor::call`], [`Supervisor::tools`]) starts it;
//!   `initialize` and the first tool list must come within 30 s, 180 s when installing or updating
//!   ([`Purpose::Install`]: the first `npx`, `uvx` or `docker run` downloads the package).
//! - **Calls** answer within 60 s, else the call fails and the server is told it is cancelled.
//! - **Idle:** 10 minutes without a call end it; the next call starts it again.
//! - **Crashes:** a server that ends by itself, fails to start or breaks the protocol starts again on the next
//!   need; the third failure within 5 minutes leaves it [`Status::Crashed`] until a person switches it on again
//!   ([`Supervisor::reset`]).
//! - **Stopped on purpose:** a server ended by [`Supervisor::stop`] or [`Supervisor::forget`] (switched off,
//!   removed) is not started again, not even by a call that began before, until [`Supervisor::reset`]
//!   ([`SupervisorError::Stopped`]). So a call that read the store just before a person switched the server off
//!   or removed it cannot start it again with the old secrets, nor make its deleted folder again.
//! - **The process tree:** a program starts with an empty environment and exactly `super::env::child_env`, in
//!   its working folder `<data>/extensions/<id>/work` (the caller's [`Launch::work_dir`], from
//!   `super::store::Store::work_path`, made here right before the start), as the leader of a process group on Unix and in a Job Object on Windows
//!   (without a console window). Ending it closes its stdin, waits [`Timing::stop_grace`] and then kills the
//!   whole group or job, so `docker run -i` ends its container and a `sleep` it started goes too. When the
//!   program ends by itself, what it left behind is killed at once.
//! - **Output:** what it writes on stderr stays in memory, the last [`OUTPUT_LINES`] lines and at most
//!   [`OUTPUT_BYTES`] bytes ([`Supervisor::output`], only for people: `extensions.read_logs`), never in a log.
//!
//! The log names a server's id, a tool name and the kind of failure, never arguments, answers, values or
//! output. Which tools a person allowed, rights and approvals are the caller's (task MA.5); the supervisor runs
//! what it is given.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
#[cfg(windows)]
use process_wrap::tokio::{CreationFlags, JobObject};
use rmcp::model::JsonObject;
use tokio::io::AsyncReadExt;
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{oneshot, watch};
use torro_core::i18n::tr;
use torro_ipc::Secret;
use torro_ipc::extensions::{CrashReason, ServerSpec, Status, ToolResult};

use super::client::{self, ClientError, Connection};
use super::env::{self, EnvError};
use super::prereq::{self, Roots};
use super::store::ReportedTool;

/// At most this many lines of a server's stderr are kept ([`Supervisor::output`]).
pub const OUTPUT_LINES: usize = 200;

/// At most this many bytes of a server's stderr are kept, all lines together; a longer line is cut.
pub const OUTPUT_BYTES: usize = 64 * 1024;

/// The times and limits of the supervisor. [`Timing::default`] has the ones of the task; tests set short ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How long a start may take, `initialize` and the first tool list: 30 s.
    pub start: Duration,
    /// The same when installing or updating ([`Purpose::Install`]): 180 s.
    pub install_start: Duration,
    /// How long a call may take: 60 s.
    pub call: Duration,
    /// Without a call for this long a server is ended: 10 minutes.
    pub idle: Duration,
    /// [`Timing::max_failures`] failures within this window leave a server crashed: 5 minutes.
    pub restart_window: Duration,
    /// How many failures within [`Timing::restart_window`] leave a server crashed: 3.
    pub max_failures: usize,
    /// How long ending a program waits for it to end by itself after its stdin closed, before the whole tree
    /// is killed: 3 s.
    pub stop_grace: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            start: Duration::from_secs(30),
            install_start: Duration::from_secs(180),
            call: Duration::from_secs(60),
            idle: Duration::from_secs(10 * 60),
            restart_window: Duration::from_secs(5 * 60),
            max_failures: 3,
            stop_grace: Duration::from_secs(3),
        }
    }
}

/// Why a server is started: for a call or a tool list ([`Timing::start`]), or while installing or updating
/// ([`Timing::install_start`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Use,
    Install,
}

/// What it takes to start one server, as the caller (task MA.5) puts it together from the store.
///
/// A running server that is asked for with another `Launch` (an update, a new secret) is ended and started
/// with the new one. `Debug` shows no secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Its id ([`torro_ipc::extensions::valid_id`]).
    pub id: String,
    /// How to reach it, as stored (`super::store::StoredExtension::server`).
    pub server: ServerSpec,
    /// Its stored secrets by name (`super::store::read_secrets`); they go to this server only.
    pub secrets: BTreeMap<String, Secret>,
    /// Where its program runs: `super::store::Store::work_path`. The supervisor makes it (0700) right before
    /// it starts the program, after it checked the server is not stopped on purpose, so a server removed
    /// meanwhile gets no folder again. Not used for an address.
    pub work_dir: PathBuf,
}

/// Why a server could not be started or called. `Display` says it for people in the suite's language (the
/// `message` of an error); it never holds the server's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorError {
    /// Its program is not on this computer (`super::prereq::find`); the caller shows
    /// [`Status::Missing`]. Not counted as a failure.
    Missing,
    /// It cannot be set up as stored (a blocked variable, a missing secret, an address that is not allowed).
    /// Not counted as a failure.
    Setup(EnvError),
    /// It failed [`Timing::max_failures`] times within [`Timing::restart_window`] and is not started until
    /// [`Supervisor::reset`].
    Crashed(CrashReason),
    /// Starting it failed now, for this reason; counted.
    Failed(CrashReason),
    /// The call did not answer within [`Timing::call`]; the server keeps running.
    CallTimeout,
    /// The server ended during the call; counted as a crash ([`CrashReason::Exited`]).
    Closed,
    /// The server answered the call with something that is no answer; it keeps running.
    Protocol,
    /// The server was stopped on purpose ([`Supervisor::stop`], [`Supervisor::forget`]: switched off,
    /// removed) and is not started until [`Supervisor::reset`]. Not counted as a failure.
    Stopped,
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            SupervisorError::Missing => {
                tr("The program this MCP server needs isn't installed on this computer.").into()
            }
            SupervisorError::Setup(err) => err.to_string(),
            SupervisorError::Crashed(_) => {
                tr("The MCP server failed three times within five minutes. Switch it off and on again to try again.")
                    .into()
            }
            SupervisorError::Failed(reason) => reason_text(*reason),
            SupervisorError::CallTimeout => tr("The MCP server didn't answer in time.").into(),
            SupervisorError::Closed => reason_text(CrashReason::Exited),
            SupervisorError::Protocol => reason_text(CrashReason::Protocol),
            SupervisorError::Stopped => tr("The MCP server is switched off.").into(),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for SupervisorError {}

fn reason_text(reason: CrashReason) -> String {
    match reason {
        CrashReason::Exited => tr("The MCP server stopped unexpectedly."),
        CrashReason::StartTimeout => tr("The MCP server didn't start in time."),
        CrashReason::SpawnFailed => tr("The MCP server's program couldn't be started."),
        CrashReason::Protocol => tr("The MCP server sent something Torro Workplace doesn't understand."),
    }
    .into()
}

/// The kind of a failure for the log, in English.
fn reason_kind(reason: CrashReason) -> &'static str {
    match reason {
        CrashReason::Exited => "ended",
        CrashReason::StartTimeout => "start timed out",
        CrashReason::SpawnFailed => "could not be started",
        CrashReason::Protocol => "broke the protocol",
    }
}

/// The MCP servers of the service. One per service ([`Supervisor::service`]); tests make their own with
/// [`Supervisor::new`].
pub struct Supervisor {
    roots: Roots,
    /// The service's environment, of which a program gets only the basics (`super::env::child_env`).
    base_env: BTreeMap<String, String>,
    timing: Timing,
    servers: Mutex<HashMap<String, Arc<Server>>>,
    /// The ids stopped on purpose ([`Supervisor::stop`], [`Supervisor::forget`]) and not reset since: not
    /// started. Kept apart from `servers`, so a server forgotten stays stopped for a call that still holds it.
    stopped: Mutex<HashSet<String>>,
    changes: Arc<watch::Sender<u64>>,
}

impl fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Supervisor").field("timing", &self.timing).finish_non_exhaustive()
    }
}

impl Supervisor {
    /// A supervisor that looks for programs in `roots`, gives them the basics of `base_env` and keeps `timing`.
    /// Nothing starts before a call.
    pub fn new(roots: Roots, base_env: BTreeMap<String, String>, timing: Timing) -> Supervisor {
        Supervisor {
            roots,
            base_env,
            timing,
            servers: Mutex::new(HashMap::new()),
            stopped: Mutex::new(HashSet::new()),
            changes: Arc::new(watch::channel(0).0),
        }
    }

    /// The service's supervisor, made on first use: programs where the service looks for them
    /// ([`Roots::system`]), the service's environment, [`Timing::default`]. The core uses it
    /// (`super::Extensions::system`) and the end of the service stops it ([`Supervisor::stop_all`]).
    pub fn service() -> Arc<Supervisor> {
        static SERVICE: OnceLock<Arc<Supervisor>> = OnceLock::new();
        SERVICE
            .get_or_init(|| Arc::new(Supervisor::new(Roots::system(), env::service_env(), Timing::default())))
            .clone()
    }

    /// The tools of the server as it reports them now, starting it first when it does not run
    /// ([`Purpose`] sets the time limit of the start). A running server's list is the one it last reported,
    /// read again after each `notifications/tools/list_changed`. Fails with [`SupervisorError::Crashed`]
    /// while it is crashed and with [`SupervisorError::Stopped`] while it is stopped on purpose.
    pub async fn tools(&self, launch: &Launch, purpose: Purpose) -> Result<Vec<ReportedTool>, SupervisorError> {
        let (server, generation, _) = self.ensure(launch, purpose).await?;
        let state = lock(&server.state);
        match &state.run {
            Some(run) if run.generation == generation => Ok(run.tools.clone()),
            // It ended right after it started: counted already.
            _ => Err(SupervisorError::Closed),
        }
    }

    /// Calls `tool` of the server with `arguments`, starting the server first when it does not run
    /// ([`Purpose::Use`]). The answer is the server's (`untrusted`; an error of the server is an answer with
    /// `is_error`, see [`Connection::call_tool`]). [`SupervisorError::CallTimeout`] after [`Timing::call`];
    /// [`SupervisorError::Closed`] when the server ended meanwhile (it starts again on the next call).
    pub async fn call(
        &self,
        launch: &Launch,
        tool: &str,
        arguments: JsonObject,
    ) -> Result<ToolResult, SupervisorError> {
        let (server, generation, connection) = self.ensure(launch, Purpose::Use).await?;
        server.use_run(generation, 1);
        let answer = connection.call_tool(tool, arguments, self.timing.call).await;
        server.use_run(generation, -1);
        match answer {
            Ok(result) => Ok(result),
            Err(ClientError::Timeout) => {
                eprintln!("torroworkplace: MCP server {}: {tool:?} did not answer in time", server.id);
                Err(SupervisorError::CallTimeout)
            }
            Err(ClientError::Closed) => {
                if let Some(run) = server.take_failed(generation, CrashReason::Exited, &self.timing) {
                    self.bump();
                    run.finish(self.timing.stop_grace).await;
                }
                Err(SupervisorError::Closed)
            }
            Err(ClientError::Protocol | ClientError::BadHeader) => {
                eprintln!("torroworkplace: MCP server {}: {tool:?} broke the protocol", server.id);
                Err(SupervisorError::Protocol)
            }
        }
    }

    /// How the server runs: [`Status::Stopped`], [`Status::Starting`], [`Status::Running`] or
    /// [`Status::Crashed`]. `review` and `missing` are the caller's to tell (they depend on the store and the
    /// computer). A server it never saw is stopped.
    pub fn status(&self, id: &str) -> Status {
        let Some(server) = self.get(id) else { return Status::Stopped };
        let state = lock(&server.state);
        match (&state.crashed, state.starting, &state.run) {
            (Some(reason), _, _) => Status::Crashed { reason: *reason },
            (None, true, _) => Status::Starting,
            (None, false, Some(_)) => Status::Running,
            (None, false, None) => Status::Stopped,
        }
    }

    /// The tools the running server reported last; `None` while it does not run.
    pub fn reported(&self, id: &str) -> Option<Vec<ReportedTool>> {
        let server = self.get(id)?;
        let state = lock(&server.state);
        state.run.as_ref().map(|run| run.tools.clone())
    }

    /// What the server wrote on stderr, over all its runs since the service started: the last
    /// [`OUTPUT_LINES`] lines, at most [`OUTPUT_BYTES`] bytes. Foreign text, only for people.
    pub fn output(&self, id: &str) -> Vec<String> {
        self.get(id).map(|server| lock(&server.output).lines.iter().cloned().collect()).unwrap_or_default()
    }

    /// Changes whenever a server starts, ends, crashes or reports another tool list, so the caller can read
    /// [`Supervisor::status`] and [`Supervisor::reported`] again.
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    /// Ends the server if it runs (switched off, installed and not yet allowed, updated while off); not a
    /// failure. It is not started again, by no call and no tool list, until [`Supervisor::reset`]: also not by
    /// a call that began before and starts it after this ([`SupervisorError::Stopped`]).
    pub async fn stop(&self, id: &str) {
        // Marked before the turn is taken: a start that waits for the turn sees it, one under way is ended.
        lock(&self.stopped).insert(id.to_string());
        let Some(server) = self.get(id) else { return };
        let _turn = server.turn.lock().await;
        let run = lock(&server.state).run.take();
        if let Some(run) = run {
            self.bump();
            run.finish(self.timing.stop_grace).await;
        }
    }

    /// Forgets the failures of the server, so a crashed one starts again on the next call, and lifts a
    /// [`Supervisor::stop`] or [`Supervisor::forget`]: when a person switches it on (`extensions.set_enabled`),
    /// allows its tools, or installs or updates it.
    pub fn reset(&self, id: &str) {
        lock(&self.stopped).remove(id);
        let Some(server) = self.get(id) else { return };
        let mut state = lock(&server.state);
        state.failures.clear();
        state.crashed = None;
        drop(state);
        self.bump();
    }

    /// Ends the server and forgets it with its output: when it is removed (`extensions.remove_extension`). Like
    /// [`Supervisor::stop`] it is not started again until [`Supervisor::reset`] (installing it again).
    pub async fn forget(&self, id: &str) {
        self.stop(id).await;
        lock(&self.servers).remove(id);
        self.bump();
    }

    /// Ends every server at once and waits for them (each at most [`Timing::stop_grace`] and a moment): when
    /// the service ends.
    pub async fn stop_all(&self) {
        let servers: Vec<Arc<Server>> = lock(&self.servers).values().cloned().collect();
        let grace = self.timing.stop_grace;
        let stops: Vec<_> = servers
            .into_iter()
            .filter_map(|server| lock(&server.state).run.take())
            .map(|run| tokio::spawn(run.finish(grace)))
            .collect();
        for stop in stops {
            let _ = stop.await;
        }
        self.bump();
    }

    fn get(&self, id: &str) -> Option<Arc<Server>> {
        lock(&self.servers).get(id).cloned()
    }

    fn bump(&self) {
        self.changes.send_modify(|count| *count += 1);
    }

    /// The running connection for `launch`, started now when there is none (or it runs with another launch,
    /// or it ended). One start at a time per server: calls at the same moment share it.
    async fn ensure(
        &self,
        launch: &Launch,
        purpose: Purpose,
    ) -> Result<(Arc<Server>, u64, Arc<Connection>), SupervisorError> {
        let server =
            lock(&self.servers).entry(launch.id.clone()).or_insert_with(|| Arc::new(Server::new(&launch.id))).clone();
        let _turn = server.turn.lock().await;
        // Under the turn: a `stop` marks before it takes the turn, so either it ends what starts here or this
        // sees the mark.
        if lock(&self.stopped).contains(&launch.id) {
            return Err(SupervisorError::Stopped);
        }
        let stale = {
            let mut state = lock(&server.state);
            if let Some(reason) = state.crashed {
                return Err(SupervisorError::Crashed(reason));
            }
            match state.run.as_mut() {
                Some(run) if run.launch == *launch && !run.connection.is_closed() => {
                    run.last_used = Instant::now();
                    return Ok((server.clone(), run.generation, run.connection.clone()));
                }
                Some(run) if run.connection.is_closed() => {
                    let generation = run.generation;
                    drop(state);
                    server.take_failed(generation, CrashReason::Exited, &self.timing)
                }
                Some(_) => state.run.take(),
                None => None,
            }
        };
        if let Some(run) = stale {
            self.bump();
            run.finish(self.timing.stop_grace).await;
        }
        if let Some(reason) = lock(&server.state).crashed {
            return Err(SupervisorError::Crashed(reason));
        }

        lock(&server.state).starting = true;
        self.bump();
        let limit = match purpose {
            Purpose::Use => self.timing.start,
            Purpose::Install => self.timing.install_start,
        };
        // A start that runs out of time is dropped: its process with it, which kills the tree.
        let started = tokio::time::timeout(limit, self.start(launch, &server.output, limit))
            .await
            .unwrap_or(Err(SupervisorError::Failed(CrashReason::StartTimeout)));
        let mut state = lock(&server.state);
        state.starting = false;
        let result = match started {
            Ok((connection, process, tools)) => {
                state.generation += 1;
                let generation = state.generation;
                let connection = Arc::new(connection);
                state.run = Some(Run {
                    generation,
                    launch: launch.clone(),
                    connection: connection.clone(),
                    process,
                    tools,
                    last_used: Instant::now(),
                    in_flight: 0,
                });
                let tended = tend(server.clone(), generation, connection.clone(), self.timing, self.changes.clone());
                tokio::spawn(tended);
                Ok((server.clone(), generation, connection))
            }
            Err(SupervisorError::Failed(reason)) => {
                eprintln!("torroworkplace: MCP server {} {}", server.id, reason_kind(reason));
                record_failure(&mut state, reason, &self.timing);
                Err(SupervisorError::Failed(reason))
            }
            Err(err) => Err(err),
        };
        drop(state);
        self.bump();
        result
    }

    /// Starts the program or connects to the address of `launch`, runs `initialize` and reads the tools.
    async fn start(
        &self,
        launch: &Launch,
        output: &Arc<Mutex<Output>>,
        limit: Duration,
    ) -> Result<(Connection, Option<Process>, Vec<ReportedTool>), SupervisorError> {
        let failed = |err: ClientError| {
            SupervisorError::Failed(match err {
                ClientError::Closed => CrashReason::Exited,
                ClientError::Timeout => CrashReason::StartTimeout,
                ClientError::Protocol | ClientError::BadHeader => CrashReason::Protocol,
            })
        };
        let (connection, process) = match &launch.server {
            ServerSpec::Stdio { command, args, .. } => {
                let found = prereq::find(command, &self.roots).ok_or(SupervisorError::Missing)?;
                let dirs = env::path_dirs(&found, &self.roots);
                let vars = env::child_env(&launch.server, &launch.secrets, &self.base_env, &dirs)
                    .map_err(SupervisorError::Setup)?;
                crate::clients::create_private_dir(&launch.work_dir)
                    .map_err(|_| SupervisorError::Failed(CrashReason::SpawnFailed))?;
                let (process, stdout, stdin) = Process::spawn(&found.program, args, vars, &launch.work_dir, output)
                    .map_err(|_| SupervisorError::Failed(CrashReason::SpawnFailed))?;
                (client::connect((stdout, stdin)).await.map_err(failed)?, Some(process))
            }
            ServerSpec::Http { url, .. } => {
                env::check_url(url).map_err(SupervisorError::Setup)?;
                let headers = env::headers(&launch.server, &launch.secrets).map_err(SupervisorError::Setup)?;
                (client::connect_http(url, &headers).await.map_err(failed)?, None)
            }
        };
        let tools = connection.list_tools(limit).await.map_err(failed)?;
        Ok((connection, process, tools))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One server the supervisor knows.
struct Server {
    id: String,
    /// Held while it starts or stops, so calls at the same moment start it once.
    turn: tokio::sync::Mutex<()>,
    state: Mutex<State>,
    output: Arc<Mutex<Output>>,
}

#[derive(Default)]
struct State {
    /// It runs.
    run: Option<Run>,
    starting: bool,
    /// When it failed within the restart window.
    failures: VecDeque<Instant>,
    /// It failed too often; why it failed last.
    crashed: Option<CrashReason>,
    /// Counts its starts, so what watches one run leaves the next alone.
    generation: u64,
}

/// One run of a server.
struct Run {
    generation: u64,
    launch: Launch,
    connection: Arc<Connection>,
    /// Its program; `None` for an address.
    process: Option<Process>,
    tools: Vec<ReportedTool>,
    last_used: Instant,
    in_flight: usize,
}

impl Run {
    /// Ends the connection (a program's stdin closes), then the process tree.
    async fn finish(self, grace: Duration) {
        self.connection.close(grace).await;
        if let Some(process) = self.process {
            process.stop(grace).await;
        }
    }
}

impl Server {
    fn new(id: &str) -> Server {
        Server {
            id: id.to_string(),
            turn: tokio::sync::Mutex::new(()),
            state: Mutex::new(State::default()),
            output: Arc::new(Mutex::new(Output::default())),
        }
    }

    /// Counts a call in (`1`) or out (`-1`) of the run `generation`; either way it was used now.
    fn use_run(&self, generation: u64, change: isize) {
        let mut state = lock(&self.state);
        if let Some(run) = state.run.as_mut().filter(|run| run.generation == generation) {
            run.in_flight = run.in_flight.saturating_add_signed(change);
            run.last_used = Instant::now();
        }
    }

    /// Takes the run `generation` out because it failed (`reason`) and counts the failure, unless it is gone
    /// already (stopped, or counted by whoever noticed first).
    fn take_failed(&self, generation: u64, reason: CrashReason, timing: &Timing) -> Option<Run> {
        let mut state = lock(&self.state);
        if state.run.as_ref().is_none_or(|run| run.generation != generation) {
            return None;
        }
        let run = state.run.take();
        eprintln!("torroworkplace: MCP server {} {}", self.id, reason_kind(reason));
        record_failure(&mut state, reason, timing);
        run
    }
}

/// Counts a failure now; the one that makes [`Timing::max_failures`] within the window leaves the server
/// crashed.
fn record_failure(state: &mut State, reason: CrashReason, timing: &Timing) {
    let now = Instant::now();
    state.failures.push_back(now);
    while state.failures.front().is_some_and(|first| now.duration_since(*first) > timing.restart_window) {
        state.failures.pop_front();
    }
    if state.failures.len() >= timing.max_failures {
        state.crashed = Some(reason);
    }
}

/// Watches the run `generation` of `server` until it ends: when the connection ends by itself it is a crash;
/// a new tool list is read after `tools/list_changed`; without a call for [`Timing::idle`] the run is ended.
async fn tend(
    server: Arc<Server>,
    generation: u64,
    connection: Arc<Connection>,
    timing: Timing,
    changes: Arc<watch::Sender<u64>>,
) {
    let bump = || changes.send_modify(|count| *count += 1);
    let mut tools_changed = Some(connection.tools_changed());
    loop {
        let deadline = {
            let state = lock(&server.state);
            match &state.run {
                Some(run) if run.generation == generation && run.in_flight > 0 => Instant::now() + timing.idle,
                Some(run) if run.generation == generation => run.last_used + timing.idle,
                _ => return,
            }
        };
        let list_changed = async {
            match tools_changed.as_mut() {
                Some(receiver) => receiver.changed().await.is_ok(),
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = connection.closed() => {
                if let Some(run) = server.take_failed(generation, CrashReason::Exited, &timing) {
                    bump();
                    run.finish(timing.stop_grace).await;
                }
                return;
            }
            heard = list_changed => {
                if !heard {
                    // The connection is ending; `closed` tells.
                    tools_changed = None;
                    continue;
                }
                if let Ok(tools) = connection.list_tools(timing.call).await {
                    let mut state = lock(&server.state);
                    if let Some(run) = state.run.as_mut().filter(|run| run.generation == generation) {
                        run.tools = tools;
                    }
                    drop(state);
                    bump();
                }
            }
            _ = tokio::time::sleep_until(deadline.into()) => {
                let run = {
                    let mut state = lock(&server.state);
                    let idle = state.run.as_ref().is_some_and(|run| {
                        run.generation == generation && run.in_flight == 0 && run.last_used.elapsed() >= timing.idle
                    });
                    if idle { state.run.take() } else { None }
                };
                if let Some(run) = run {
                    bump();
                    run.finish(timing.stop_grace).await;
                    return;
                }
            }
        }
    }
}

/// A server's program and what watches it.
struct Process {
    /// Tells the watcher to end the tree, after waiting the time sent for the program to end by itself. Dropped
    /// unsent, it ends the tree at once.
    stop: Option<oneshot::Sender<Duration>>,
    done: Option<tokio::task::JoinHandle<()>>,
}

impl Process {
    /// Starts `program` with `args`, exactly the environment `vars` (`env_clear` first) and `work_dir`, as the
    /// leader of a process group (Unix) or in a Job Object without a console window (Windows); its stderr goes
    /// to `output`. Returns it with its stdout and stdin for the MCP connection.
    fn spawn(
        program: &Path,
        args: &[String],
        vars: Vec<(String, String)>,
        work_dir: &Path,
        output: &Arc<Mutex<Output>>,
    ) -> std::io::Result<(Process, ChildStdout, ChildStdin)> {
        let mut command = tokio::process::Command::new(program);
        command
            .args(args)
            .env_clear()
            .envs(vars)
            .current_dir(work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut wrap = CommandWrap::from(command);
        #[cfg(unix)]
        wrap.wrap(ProcessGroup::leader());
        #[cfg(windows)]
        {
            wrap.wrap(JobObject);
            wrap.wrap(CreationFlags(windows::Win32::System::Threading::CREATE_NO_WINDOW));
        }
        wrap.wrap(KillOnDrop);
        let mut tree = Tree { child: wrap.spawn()?, reaped: false };
        let pipes = (tree.child.stdin().take(), tree.child.stdout().take(), tree.child.stderr().take());
        let (Some(stdin), Some(stdout), stderr) = pipes else {
            // Dropping the tree kills it.
            return Err(std::io::Error::other("the program has no pipes"));
        };
        if let Some(stderr) = stderr {
            tokio::spawn(read_output(stderr, output.clone()));
        }
        let (stop, stopped) = oneshot::channel();
        let done = tokio::spawn(tree.watch(stopped));
        Ok((Process { stop: Some(stop), done: Some(done) }, stdout, stdin))
    }

    /// Ends the tree: gives the program `grace` to end by itself (its stdin is closed by now), then kills the
    /// whole group or job and waits for it.
    async fn stop(mut self, grace: Duration) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(grace);
        }
        if let Some(done) = self.done.take() {
            let _ = done.await;
        }
    }
}

/// The started program with everything it starts: killed as a whole when the watcher ends it, or when it is
/// dropped unreaped (the service ends without [`Supervisor::stop_all`]).
struct Tree {
    child: Box<dyn ChildWrapper>,
    /// Killed and waited for: dropping it then kills nothing (the group's id may be another's by then).
    reaped: bool,
}

impl Tree {
    /// Waits until the program ends by itself or `stop` says to end it (then waits the time sent, or none
    /// when `stop` is dropped), kills what is left of the tree and reaps the program.
    async fn watch(mut self, stop: oneshot::Receiver<Duration>) {
        // The process group (Unix) and the Job Object (Windows) are the only wrappers around the child, so the
        // one inside is the program itself; its own `wait` would wait for the whole job on Windows, and a
        // program that left a child behind would never seem to end.
        let grace = tokio::select! {
            _ = self.child.inner_mut().wait() => None,
            grace = stop => grace.ok(),
        };
        if let Some(grace) = grace {
            let _ = tokio::time::timeout(grace, self.child.inner_mut().wait()).await;
        }
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
        self.reaped = true;
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.start_kill();
        }
    }
}

/// The kept end of a server's stderr.
#[derive(Debug, Default)]
struct Output {
    lines: VecDeque<String>,
    bytes: usize,
}

impl Output {
    /// Adds one line (without its line break), cut to [`OUTPUT_BYTES`], and drops the oldest lines beyond
    /// [`OUTPUT_LINES`] or [`OUTPUT_BYTES`].
    fn push(&mut self, line: &[u8]) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let mut text = String::from_utf8_lossy(line).into_owned();
        if text.len() > OUTPUT_BYTES {
            let mut end = OUTPUT_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        self.bytes += text.len();
        self.lines.push_back(text);
        while self.lines.len() > OUTPUT_LINES || self.bytes > OUTPUT_BYTES {
            match self.lines.pop_front() {
                Some(old) => self.bytes -= old.len(),
                None => break,
            }
        }
    }
}

/// Reads `stderr` into `output` until it closes. A line longer than [`OUTPUT_BYTES`] keeps only its start, so
/// a program that never writes a line break cannot fill the memory.
async fn read_output(mut stderr: ChildStderr, output: Arc<Mutex<Output>>) {
    let mut chunk = [0u8; 8192];
    let mut line: Vec<u8> = Vec::new();
    loop {
        let read = match stderr.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        for &byte in &chunk[..read] {
            if byte == b'\n' {
                lock(&output).push(&line);
                line.clear();
            } else if line.len() < OUTPUT_BYTES {
                line.push(byte);
            }
        }
    }
    if !line.is_empty() {
        lock(&output).push(&line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_keeps_the_last_lines_within_its_size() {
        let mut output = Output::default();
        for n in 0..300 {
            output.push(format!("line {n}\r").as_bytes());
        }
        assert_eq!(output.lines.len(), OUTPUT_LINES);
        assert_eq!(output.lines.front().map(String::as_str), Some("line 100"));
        assert_eq!(output.lines.back().map(String::as_str), Some("line 299"), "no carriage return");

        let mut output = Output::default();
        let long = vec![b'x'; 40 * 1024];
        output.push(&long);
        output.push(&long);
        assert_eq!(output.lines.len(), 1, "two long lines do not fit");
        assert!(output.bytes <= OUTPUT_BYTES);
        output.push(&vec![b'y'; 100 * 1024]);
        assert_eq!(output.lines.back().map(String::len), Some(OUTPUT_BYTES), "a longer line is cut");
        output.push("ä".repeat(OUTPUT_BYTES).as_bytes());
        assert!(output.lines.back().is_some_and(|line| line.len() <= OUTPUT_BYTES && line.ends_with('ä')));
    }

    #[test]
    fn three_failures_within_the_window_crash() {
        let timing = Timing { restart_window: Duration::from_millis(50), ..Timing::default() };
        let mut state = State::default();
        record_failure(&mut state, CrashReason::Exited, &timing);
        record_failure(&mut state, CrashReason::Exited, &timing);
        assert_eq!(state.crashed, None);
        std::thread::sleep(Duration::from_millis(80));
        record_failure(&mut state, CrashReason::Exited, &timing);
        assert_eq!(state.crashed, None, "the first two are out of the window");
        record_failure(&mut state, CrashReason::Protocol, &timing);
        record_failure(&mut state, CrashReason::StartTimeout, &timing);
        assert_eq!(state.crashed, Some(CrashReason::StartTimeout));
    }

    #[test]
    fn errors_say_what_happened_in_words() {
        assert_eq!(SupervisorError::CallTimeout.to_string(), tr("The MCP server didn't answer in time."));
        assert_eq!(SupervisorError::Closed.to_string(), tr("The MCP server stopped unexpectedly."));
        assert_eq!(SupervisorError::Stopped.to_string(), tr("The MCP server is switched off."));
        let setup = SupervisorError::Setup(EnvError::MissingSecret { name: "TOKEN".into() });
        assert!(setup.to_string().contains("TOKEN"));
        let defaults = Timing::default();
        assert_eq!(
            (defaults.start, defaults.install_start, defaults.call),
            (Duration::from_secs(30), Duration::from_secs(180), Duration::from_secs(60))
        );
        assert_eq!(
            (defaults.idle, defaults.restart_window, defaults.max_failures),
            (Duration::from_secs(600), Duration::from_secs(300), 3)
        );
    }

    /// The process tests run the stand-in server `tests/fixtures/extensions/server.sh` with `/bin/sh`.
    #[cfg(unix)]
    mod process {
        use super::*;

        const WAIT: Duration = Duration::from_secs(10);

        fn script() -> String {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/extensions/server.sh")
                .to_string_lossy()
                .into_owned()
        }

        fn supervisor(timing: Timing) -> Supervisor {
            let roots = Roots { path: vec!["/usr/bin".into(), "/bin".into()], home: None, fixed: Vec::new() };
            let base = BTreeMap::from([
                ("HOME".to_string(), "/nonexistent".to_string()),
                ("TORRO_TOKEN".to_string(), "torro_k1_service".to_string()),
            ]);
            Supervisor::new(roots, base, timing)
        }

        fn quick() -> Timing {
            Timing {
                start: WAIT,
                install_start: WAIT,
                call: WAIT,
                idle: Duration::from_secs(600),
                restart_window: Duration::from_secs(300),
                max_failures: 3,
                stop_grace: Duration::from_millis(500),
            }
        }

        fn launch(work: &Path, args: &[&str]) -> Launch {
            let mut all = vec![script()];
            all.extend(args.iter().map(|arg| arg.to_string()));
            Launch {
                id: "fixture".into(),
                server: ServerSpec::Stdio {
                    command: "/bin/sh".into(),
                    args: all,
                    env: BTreeMap::from([(
                        "FIXTURE_VALUE".to_string(),
                        torro_ipc::extensions::Value::Stored { secret: true },
                    )]),
                },
                secrets: BTreeMap::from([("FIXTURE_VALUE".to_string(), Secret::new("from-keychain"))]),
                work_dir: work.to_path_buf(),
            }
        }

        fn text(result: &ToolResult) -> &str {
            match result.content.first() {
                Some(torro_ipc::extensions::Content::Text { text }) => text,
                _ => "",
            }
        }

        /// Whether the process `pid` is alive, asked with `kill -0`.
        fn alive(pid: &str) -> bool {
            std::process::Command::new("/bin/kill")
                .args(["-0", pid])
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        }

        async fn gone(pid: &str) -> bool {
            for _ in 0..50 {
                if !alive(pid) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            false
        }

        #[tokio::test]
        async fn a_server_starts_on_the_first_call_and_answers_over_stdio() {
            let dir = tempfile::tempdir().expect("temp dir");
            let supervisor = supervisor(quick());
            let launch = launch(dir.path(), &[]);
            assert_eq!(supervisor.status("fixture"), Status::Stopped);
            let tools = supervisor.tools(&launch, Purpose::Install).await.expect("tools");
            assert_eq!(
                tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
                ["echo", "pwd", "slow", "crash"]
            );
            assert_eq!(supervisor.status("fixture"), Status::Running);

            let answer = supervisor.call(&launch, "echo", JsonObject::new()).await.expect("answer");
            assert_eq!(text(&answer), "pong from-keychain", "the secret reaches the program");
            assert!(answer.untrusted && !answer.is_error);
            let pwd = supervisor.call(&launch, "pwd", JsonObject::new()).await.expect("answer");
            let work = dir.path().canonicalize().expect("canonical");
            assert_eq!(Path::new(text(&pwd)).canonicalize().expect("canonical"), work, "runs in its working folder");
            let unknown = supervisor.call(&launch, "nothing", JsonObject::new()).await.expect("answer");
            assert!(unknown.is_error);

            supervisor.stop("fixture").await;
            assert_eq!(supervisor.status("fixture"), Status::Stopped);
            supervisor.reset("fixture");
            let again = supervisor.call(&launch, "echo", JsonObject::new()).await.expect("started again");
            assert_eq!(text(&again), "pong from-keychain");
            supervisor.stop_all().await;
        }

        /// F1 of the API review: a server stopped on purpose (switched off) or forgotten (removed) is not
        /// started again by a call that already holds its launch, and gets no working folder again, until
        /// `reset`.
        #[tokio::test]
        async fn a_stopped_or_forgotten_server_stays_stopped_until_reset() {
            let dir = tempfile::tempdir().expect("temp dir");
            let supervisor = supervisor(quick());
            let work = dir.path().join("fixture/work");
            let launch = launch(&work, &[]);
            supervisor.tools(&launch, Purpose::Use).await.expect("started, its folder made");
            assert!(work.is_dir());

            supervisor.stop("fixture").await;
            assert_eq!(supervisor.call(&launch, "echo", JsonObject::new()).await, Err(SupervisorError::Stopped));
            assert_eq!(supervisor.tools(&launch, Purpose::Use).await, Err(SupervisorError::Stopped));
            assert_eq!(supervisor.status("fixture"), Status::Stopped, "not started again");
            supervisor.reset("fixture");
            supervisor.call(&launch, "echo", JsonObject::new()).await.expect("switched on again");

            supervisor.forget("fixture").await;
            std::fs::remove_dir_all(dir.path().join("fixture")).expect("removed as `remove_files` does");
            assert_eq!(supervisor.call(&launch, "echo", JsonObject::new()).await, Err(SupervisorError::Stopped));
            assert!(!dir.path().join("fixture").exists(), "no folder made again");
            assert_eq!(supervisor.status("fixture"), Status::Stopped);
            supervisor.reset("fixture");
            supervisor.tools(&launch, Purpose::Install).await.expect("installed again");
            assert!(work.is_dir());
            supervisor.stop_all().await;
        }

        #[tokio::test]
        async fn a_call_that_takes_too_long_fails_and_the_server_stays() {
            let dir = tempfile::tempdir().expect("temp dir");
            let supervisor = supervisor(Timing { call: Duration::from_millis(300), ..quick() });
            let launch = launch(dir.path(), &[]);
            let started = Instant::now();
            let late = supervisor.call(&launch, "slow", JsonObject::new()).await;
            assert_eq!(late, Err(SupervisorError::CallTimeout));
            assert!(started.elapsed() < Duration::from_secs(5));
            assert_eq!(supervisor.status("fixture"), Status::Running);
            supervisor.stop_all().await;
        }

        #[tokio::test]
        async fn a_crash_starts_again_and_the_third_leaves_it_crashed() {
            let dir = tempfile::tempdir().expect("temp dir");
            let supervisor = supervisor(quick());
            let launch = launch(dir.path(), &[]);
            assert_eq!(supervisor.call(&launch, "crash", JsonObject::new()).await, Err(SupervisorError::Closed));
            assert_eq!(supervisor.status("fixture"), Status::Stopped, "starts again when needed");
            let answer = supervisor.call(&launch, "echo", JsonObject::new()).await.expect("started again");
            assert_eq!(text(&answer), "pong from-keychain");

            assert_eq!(supervisor.call(&launch, "crash", JsonObject::new()).await, Err(SupervisorError::Closed));
            assert_eq!(supervisor.call(&launch, "crash", JsonObject::new()).await, Err(SupervisorError::Closed));
            assert_eq!(supervisor.status("fixture"), Status::Crashed { reason: CrashReason::Exited });
            assert_eq!(
                supervisor.call(&launch, "echo", JsonObject::new()).await,
                Err(SupervisorError::Crashed(CrashReason::Exited)),
                "not started again"
            );

            supervisor.reset("fixture");
            let answer = supervisor.call(&launch, "echo", JsonObject::new()).await.expect("switched on again");
            assert_eq!(text(&answer), "pong from-keychain");
            supervisor.stop_all().await;
        }

        #[tokio::test]
        async fn ending_a_server_takes_its_grandchild_along() {
            let dir = tempfile::tempdir().expect("temp dir");
            let supervisor = supervisor(quick());
            let launch = launch(dir.path(), &["grandchild"]);
            supervisor.tools(&launch, Purpose::Use).await.expect("started");
            let pid = std::fs::read_to_string(dir.path().join("grandchild.pid")).expect("pid");
            let pid = pid.trim();
            assert!(alive(pid), "the grandchild runs");
            supervisor.stop("fixture").await;
            assert!(gone(pid).await, "the grandchild is gone");

            // Also when the server ends by itself, and when the whole service ends.
            supervisor.reset("fixture");
            supervisor.tools(&launch, Purpose::Use).await.expect("started");
            let pid = std::fs::read_to_string(dir.path().join("grandchild.pid")).expect("pid").trim().to_string();
            assert!(alive(&pid));
            let _ = supervisor.call(&launch, "crash", JsonObject::new()).await;
            assert!(gone(&pid).await, "gone after a crash");
            supervisor.tools(&launch, Purpose::Use).await.expect("started");
            let pid = std::fs::read_to_string(dir.path().join("grandchild.pid")).expect("pid").trim().to_string();
            supervisor.stop_all().await;
            assert!(gone(&pid).await, "gone when the service ends");
        }

        #[tokio::test]
        async fn an_idle_server_is_ended() {
            let dir = tempfile::tempdir().expect("temp dir");
            let supervisor = supervisor(Timing { idle: Duration::from_millis(300), ..quick() });
            let launch = launch(dir.path(), &[]);
            supervisor.call(&launch, "echo", JsonObject::new()).await.expect("answer");
            assert_eq!(supervisor.status("fixture"), Status::Running);
            let mut changes = supervisor.changes();
            let deadline = Instant::now() + WAIT;
            while supervisor.status("fixture") == Status::Running && Instant::now() < deadline {
                let _ = tokio::time::timeout(Duration::from_millis(200), changes.changed()).await;
            }
            assert_eq!(supervisor.status("fixture"), Status::Stopped);
            supervisor.call(&launch, "echo", JsonObject::new()).await.expect("starts again");
            supervisor.stop_all().await;
        }

        #[tokio::test]
        async fn stderr_is_kept_but_cut() {
            let dir = tempfile::tempdir().expect("temp dir");
            let supervisor = supervisor(quick());
            let launch = launch(dir.path(), &["stderr", "250"]);
            supervisor.tools(&launch, Purpose::Use).await.expect("started");
            let deadline = Instant::now() + WAIT;
            while supervisor.output("fixture").last().map(String::as_str) != Some("line 250 of the server's output")
                && Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let output = supervisor.output("fixture");
            assert_eq!(output.len(), OUTPUT_LINES);
            assert_eq!(output.first().map(String::as_str), Some("line 51 of the server's output"));
            supervisor.stop_all().await;
        }

        #[tokio::test]
        async fn a_missing_program_or_a_missing_secret_is_no_crash() {
            let dir = tempfile::tempdir().expect("temp dir");
            let supervisor = supervisor(quick());
            let missing = Launch {
                server: ServerSpec::Stdio { command: "no-such-program".into(), args: Vec::new(), env: BTreeMap::new() },
                ..launch(dir.path(), &[])
            };
            for _ in 0..4 {
                assert_eq!(supervisor.tools(&missing, Purpose::Use).await, Err(SupervisorError::Missing));
            }
            let no_secret = Launch { secrets: BTreeMap::new(), ..launch(dir.path(), &[]) };
            assert_eq!(
                supervisor.tools(&no_secret, Purpose::Use).await,
                Err(SupervisorError::Setup(EnvError::MissingSecret { name: "FIXTURE_VALUE".into() }))
            );
            assert_eq!(supervisor.status("fixture"), Status::Stopped);
        }

        #[tokio::test]
        async fn a_program_that_speaks_no_mcp_fails_to_start_in_time() {
            let dir = tempfile::tempdir().expect("temp dir");
            let supervisor = supervisor(Timing { start: Duration::from_millis(300), ..quick() });
            let silent = Launch {
                server: ServerSpec::Stdio {
                    command: "/bin/sleep".into(),
                    args: vec!["30".into()],
                    env: BTreeMap::new(),
                },
                ..launch(dir.path(), &[])
            };
            assert_eq!(
                supervisor.tools(&silent, Purpose::Use).await,
                Err(SupervisorError::Failed(CrashReason::StartTimeout))
            );
            assert_eq!(supervisor.status("fixture"), Status::Stopped);
        }
    }
}
