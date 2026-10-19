//! `torroworkplace hook claude-code|codex|gemini`: coding agents report to the inbox (task E2.2).
//!
//! Claude Code, Codex and Gemini CLI run this command on their hook events and write a JSON
//! object to its stdin. The command reads it ([`STDIN_MAX`] at most), reads `.git/HEAD` of the
//! payload's `cwd` for the branch ([`git_head`], no process is started), lets
//! [`torro_inbox::hook::interpret`] decide what it means, adds the chain of parent processes on
//! Linux and macOS ([`parent_pids`], for jumping to the agent's window in E2.4 and E8.9; macOS
//! runs `/bin/ps` once for it, only for a report) and sends one request with
//! the hook client's key: `inbox.post_item` for "waiting" and "done", `inbox.resolve_items` when
//! the session goes on or ends.
//!
//! **Where the key comes from** (task E2.3b): `torroworkplace hook <agent> --key-file <file>`
//! reads it from a file ([`read_key_file`]); that is what the "Connect agent" wizard writes, so
//! the key is neither in the agent's configuration nor in the command line `ps` shows. The file
//! must be private: on Unix nobody but its owner may have any access to it (mode `0600` or
//! stricter), and on Linux it must belong to the user the hook runs as (the effective uid from
//! `/proc/self/status`; std has no `getuid` without `unsafe`, and other Unix systems have no
//! `/proc`, so there only the mode is checked, which already keeps every other user out). A
//! file that is missing, unreadable, too open, too long ([`KEY_FILE_MAX`]) or blank means
//! nothing is reported. With `--key-file` the file is the only source; without it the key comes
//! from `TORRO_TOKEN`, for hooks set up by hand.
//!
//! **Answering permission requests** (task E2.5): `torroworkplace hook claude-code|codex --answer
//! --key-file <file>` is what the wizard sets up for the event `PermissionRequest`. It puts the
//! request to the person as a question (`approvals.ask_permission`, [`answer`]), looks every
//! [`ANSWER_POLL`] whether the person decided (`approvals.approval_status` of exactly that
//! question, the only approval its key can read), for at most [`ANSWER_WAIT`], and prints the
//! agent's decision JSON once the person allowed or denied it
//! ([`torro_inbox::hook::permission_answer`]). Without a decision in time, when the question
//! expired, and on any error it prints nothing; the agent then asks in its own terminal as
//! always, and the hook reports what the plain hook would (Codex: "waiting"). It never allows by
//! itself. For any other event `--answer` changes nothing. Claude Code's `AskUserQuestion` is no
//! permission but a choice in its terminal: it is not put as a question, and the hook reports
//! "waiting" with the questions as text (task E3.8, [`torro_inbox::hook::ASK_TOOL`]).
//!
//! **The session's name** (task E2.6): for a report and a question, and only then, the hook looks
//! up the name the agent gave the session ([`session_name`]): the end of Claude Code's transcript,
//! or of Codex's `session_index.jsonl`, at most [`torro_inbox::hook::TITLE_TAIL_MAX`] bytes of one
//! file ([`read_tail`]). Anything missing or unreadable means no name; the report goes out as
//! before.
//!
//! The hook must never hold up or change the agent, so:
//!
//! - **nothing on stdout**, except the decision of a person just described. Claude Code reads a
//!   hook's stdout (as JSON with decisions, or as context for the model on `UserPromptSubmit`),
//!   Codex and Gemini CLI parse it too. Only [`write_decision`] writes there;
//!   `nothing_here_writes_to_stdout` checks the source.
//! - **exit code 0, always**, also on bad arguments and on a panic (the panic hook is silenced).
//! - **at most [`BUDGET`] (500 ms) for connecting and the answer** of each call; stdin gets
//!   [`STDIN_WAIT`]. Only a question waits longer for the person, [`ANSWER_WAIT`] at most, and the
//!   wizard gives that hook a timeout to match.
//! - **a core that is not running is left alone**: nothing is started, nothing is reported.
//!   Without a key (no usable key file, no `TORRO_TOKEN`) nothing is sent either.
//! - **quiet stderr.** With exit code 0 Claude Code keeps a hook's stderr out of the
//!   conversation (it shows stderr as a hook error for other codes and hands it to the model for
//!   code 2); Codex and Gemini CLI may log or show it. As that is not certain for every agent and
//!   version, operational errors stay silent unless `TORRO_HOOK_DEBUG` is set, then they come as
//!   one line. Only a broken command
//!   line (no or an unknown agent) always gives one line, because that is a setup mistake the
//!   person who set the hook up needs to see.
//!
//! The client behind the key should be of the kind `hook` (security rule 3 in
//! `docs/tasks/eingang.md`); other clients' "waiting" and "done" become plain notices. Until the
//! "Connect agent" wizard sets hooks up (E2.3), such a client is created by hand:
//! `torroworkplace ctl clients add "Claude Code hooks" --kind hook`.

use std::ffi::OsString;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use torro_core::i18n::{tr, trf};
use torro_inbox::hook::{
    HookIntent, PermissionQuestion, TITLE_TAIL_MAX, TitleSource, interpret, permission_answer, permission_question,
    title_in_codex_index, title_in_transcript, title_source,
};
use torro_inbox::{AgentKind, Kind};
use torro_ipc::{ApprovalInfo, ApprovalState, IpcError, Key, Request, Response};

/// The largest payload read from stdin. A larger one is dropped whole rather than cut, because a
/// cut JSON object cannot be read anyway.
pub const STDIN_MAX: u64 = 1024 * 1024;

/// How long connecting to the core and waiting for its answer may take together: 500 ms, so an
/// agent never waits on a hook. Tests run many cores at once, and several test runs may share the
/// machine; there a call gets ten seconds, or a slow answer fails a test that has nothing to do
/// with timing (three seconds were seen to run out with three runs at once).
pub const BUDGET: Duration = if cfg!(test) { Duration::from_secs(10) } else { Duration::from_millis(500) };

/// How long to wait for stdin to end. Agents write the payload and close the pipe at once; this
/// only guards against a pipe left open.
pub const STDIN_WAIT: Duration = Duration::from_millis(500);

/// Set (to anything but blank) to get one line on stderr when a report fails.
pub const DEBUG_VAR: &str = "TORRO_HOOK_DEBUG";

/// The most bytes read from `HEAD` or a `.git` file; both hold one short line.
const SMALL_FILE_MAX: u64 = 4096;

/// The most parent processes collected; deeper chains are cut, nearest kept.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
const PARENTS_MAX: usize = 32;

/// The option that names the file holding the hook client's key (task E2.3b).
pub const KEY_FILE_ARG: &str = "--key-file";

/// The most bytes a key file may hold. A key is about 50 characters; a longer file is not a key
/// file, and reading it whole would only cost time.
pub const KEY_FILE_MAX: u64 = 512;

/// The option that makes the hook answer `PermissionRequest` with the person's decision (task
/// E2.5).
pub const ANSWER_ARG: &str = "--answer";

/// How long the hook waits for the person's decision on a question at most: the 30 seconds of
/// task E2.5, and a little more than the question lives
/// ([`crate::approvals::QUESTION_LIFETIME`]), so the hook sees it expire rather than giving up
/// just before. The wizard's timeout for this hook is longer still.
pub const ANSWER_WAIT: Duration = Duration::from_secs(34);

/// How often the hook looks whether the person decided.
pub const ANSWER_POLL: Duration = Duration::from_millis(500);

/// What the command line of `hook` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookArgs {
    /// The agent the payload comes from.
    pub agent: AgentKind,
    /// The file with the key (`--key-file`); `None`: the key comes from `TORRO_TOKEN`.
    pub key_file: Option<PathBuf>,
    /// `--answer`: put a `PermissionRequest` to the person and print their decision (task E2.5).
    pub answer: bool,
}

/// Why a key file gave no key. Never holds the key or a part of it.
#[derive(Debug)]
pub enum KeyFileError {
    /// The file is missing or cannot be opened or read.
    Unreadable(io::Error),
    /// Group or others have some access to the file (Unix); `mode` is its permission bits.
    #[cfg_attr(not(unix), allow(dead_code))]
    TooOpen { mode: u32 },
    /// The file belongs to another user (Linux).
    #[cfg_attr(not(unix), allow(dead_code))]
    NotOwner,
    /// Not a regular file, longer than [`KEY_FILE_MAX`], not UTF-8 or blank.
    NoKey,
}

/// What a hook call did when nothing went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The event means nothing to the inbox, or the payload could not be read.
    Ignored,
    /// The core took the request and gave this answer (`posted` or `ok`).
    Sent(Box<Response>),
}

/// Why a hook call reported nothing. Only shown with [`DEBUG_VAR`]; the agent never learns.
#[derive(Debug)]
pub enum HookError {
    /// `TORRO_TOKEN` is missing or blank (and no key file was named).
    NoToken,
    /// The key file named with [`KEY_FILE_ARG`] gave no key; nothing was sent.
    KeyFile { file: PathBuf, reason: KeyFileError },
    /// No state directory, so no socket to look for.
    NoSocket,
    /// The async runtime could not start.
    Runtime(io::Error),
    /// The socket call failed; [`IpcError::NotRunning`] when the core is not running.
    Ipc(IpcError),
    /// The core did not answer within [`BUDGET`].
    TimedOut,
    /// The core answered with an error (unknown key, throttle, …).
    Refused { code: String, message: String },
    /// The core answered a question or its status with something else than a question of this
    /// hook (an older core, say); nothing is printed.
    Unexpected,
}

impl std::fmt::Display for HookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookError::NoToken => f.write_str(tr("TORRO_TOKEN is not set, so nothing was reported.")),
            HookError::KeyFile { file, reason } => f.write_str(&key_file_message(file, reason)),
            HookError::NoSocket => f.write_str(tr("No state directory, so no core to report to.")),
            HookError::Runtime(err) => write!(f, "{err}"),
            HookError::Ipc(err) => write!(f, "{err}"),
            HookError::TimedOut => f.write_str(tr("Torro Workplace did not answer in time, so nothing was reported.")),
            HookError::Refused { code, message } => write!(f, "{code}: {message}"),
            HookError::Unexpected => f.write_str(tr("Torro Workplace gave an unexpected answer to the question.")),
        }
    }
}

impl std::error::Error for HookError {}

/// The debug line for a key file that gave no key; names the file, never its content.
fn key_file_message(file: &Path, reason: &KeyFileError) -> String {
    let file = file.display();
    match reason {
        KeyFileError::Unreadable(err) => trf(
            "The key file {file} could not be read ({error}), so nothing was reported.",
            &[("file", &file), ("error", err)],
        ),
        KeyFileError::TooOpen { mode } => trf(
            "The key file {file} is open to other users (mode {mode}), so it was not used. Make it private with chmod 600.",
            &[("file", &file), ("mode", &format!("{mode:o}"))],
        ),
        KeyFileError::NotOwner => {
            trf("The key file {file} belongs to another user, so it was not used.", &[("file", &file)])
        }
        KeyFileError::NoKey => trf("The key file {file} holds no key, so nothing was reported.", &[("file", &file)]),
    }
}

/// Entry point from `main`: `args` are the arguments after `hook`, one agent id (`claude-code`,
/// `codex`, `gemini`), optionally followed by `--key-file <file>` and `--answer` ([`parse_args`]).
/// Reads stdin, reports (with `--answer` asks first) and returns [`ExitCode::SUCCESS`] whatever
/// happens; see the module docs for what goes to stdout and stderr.
pub fn from_args(args: &[String]) -> ExitCode {
    // A panic must not turn into exit code 101 and a backtrace in the agent's terminal.
    std::panic::set_hook(Box::new(|_| {}));
    let _ = std::panic::catch_unwind(|| report_from_args(args));
    ExitCode::SUCCESS
}

/// The body of [`from_args`], which may write to stderr but never to stdout.
fn report_from_args(args: &[String]) {
    let mut stderr = io::stderr();
    let Some(args) = parse_args(args) else {
        let _ = writeln!(
            stderr,
            "{}: {}",
            crate::APP.binary,
            tr("the hook needs one agent: torroworkplace hook claude-code|codex|gemini [--key-file <file>]")
        );
        return;
    };
    let payload = read_stdin();
    let debug = std::env::var(DEBUG_VAR).is_ok_and(|value| !value.trim().is_empty());
    let mut note = |err: &HookError| {
        if debug {
            let _ = writeln!(stderr, "{} hook: {}", crate::APP.binary, one_line(&err.to_string()));
        }
    };
    let token = match key_for(&args, std::env::var(crate::mcp::TOKEN_VAR).ok()) {
        Ok(token) => token,
        Err(err) => return note(&err),
    };
    let socket = torro_ipc::socket_path();
    if args.answer
        && let (Some(payload), Some(token)) = (payload.as_deref(), token.as_ref())
        && let Some(mut question) = permission_question(args.agent, payload)
    {
        question.title = session_name(args.agent, payload);
        match answer(args.agent, &question, socket.as_deref(), token, ANSWER_WAIT) {
            Ok(Some(decision)) => return write_decision(&decision),
            Ok(None) => {}
            Err(err) => note(&err),
        }
        // No decision: the agent asks in its own terminal now, and the hook reports as without
        // `--answer` (Codex: "waiting").
    }
    if let Err(err) = run(args.agent, payload.as_deref(), socket.as_deref(), token.as_ref()) {
        note(&err);
    }
}

/// The one place that writes to stdout: the decision of a person for the agent ([`answer`]), one
/// line. A failed write is ignored; the agent then asks in its own terminal.
fn write_decision(decision: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{decision}");
    let _ = out.flush();
}

/// The command line after `hook`: `<agent>`, then optionally `--key-file <file>` and `--answer`
/// in any order, each once, nothing else (an unknown option is a setup mistake, and guessing
/// could send with the wrong key). `None` for any other shape, an unknown agent or an empty file
/// name.
pub fn parse_args(args: &[String]) -> Option<HookArgs> {
    let (id, mut rest) = args.split_first()?;
    let mut parsed = HookArgs { agent: AgentKind::from_id(id)?, key_file: None, answer: false };
    while let Some((flag, after)) = rest.split_first() {
        rest = match (flag.as_str(), after) {
            (ANSWER_ARG, after) if !parsed.answer => {
                parsed.answer = true;
                after
            }
            (KEY_FILE_ARG, [file, after @ ..]) if parsed.key_file.is_none() && !file.is_empty() => {
                parsed.key_file = Some(PathBuf::from(file));
                after
            }
            _ => return None,
        };
    }
    Some(parsed)
}

/// The key to send with: from the key file when the command line names one ([`read_key_file`];
/// `TORRO_TOKEN` is then not looked at, so a broken file never falls back to another key), else
/// from `env_token`, the content of `TORRO_TOKEN` (`None` when it is missing or blank; [`run`]
/// then sends nothing).
pub fn key_for(args: &HookArgs, env_token: Option<String>) -> Result<Option<Key>, HookError> {
    match &args.key_file {
        Some(file) => read_key_file(file).map(Some).map_err(|reason| HookError::KeyFile { file: file.clone(), reason }),
        None => Ok(crate::cli::token_from(env_token)),
    }
}

/// The key in `file`, trimmed. Refused before the content is read when the file is not a regular
/// file, when group or others have any access to it (Unix) or when it belongs to another user
/// (Linux, see the module docs); content longer than [`KEY_FILE_MAX`], not UTF-8 or blank is
/// [`KeyFileError::NoKey`]. The checks look at the opened file, so it cannot be swapped between
/// checking and reading.
pub fn read_key_file(file: &Path) -> Result<Key, KeyFileError> {
    // Looked at before opening too: opening a named pipe would wait for a writer.
    if !std::fs::metadata(file).map_err(KeyFileError::Unreadable)?.is_file() {
        return Err(KeyFileError::NoKey);
    }
    let opened = std::fs::File::open(file).map_err(KeyFileError::Unreadable)?;
    let metadata = opened.metadata().map_err(KeyFileError::Unreadable)?;
    if !metadata.is_file() {
        return Err(KeyFileError::NoKey);
    }
    check_private(&metadata)?;
    let text = read_bounded(opened, KEY_FILE_MAX).ok_or(KeyFileError::NoKey)?;
    crate::cli::token_from(Some(text)).ok_or(KeyFileError::NoKey)
}

/// Unix: nobody but the owner has any access, and on Linux the owner is this process's user.
#[cfg(unix)]
fn check_private(metadata: &std::fs::Metadata) -> Result<(), KeyFileError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(KeyFileError::TooOpen { mode });
    }
    match effective_uid() {
        Some(uid) if uid != metadata.uid() => Err(KeyFileError::NotOwner),
        _ => Ok(()),
    }
}

/// Windows keeps files in the profile private to the user; there is no mode to check.
#[cfg(not(unix))]
fn check_private(_metadata: &std::fs::Metadata) -> Result<(), KeyFileError> {
    Ok(())
}

/// The effective uid of this process from `/proc/self/status` on Linux; `None` on other systems
/// or when it cannot be read (the owner is then not checked; the mode check still keeps others
/// out).
#[cfg(unix)]
fn effective_uid() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        read_small(Path::new("/proc/self/status")).and_then(|status| effective_uid_in(&status))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// The effective uid in the text of `/proc/<pid>/status`: the second number of its `Uid:` line
/// (real, effective, saved, file system).
#[cfg(target_os = "linux")]
fn effective_uid_in(status: &str) -> Option<u32> {
    let line = status.lines().find_map(|line| line.strip_prefix("Uid:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Reports the hook payload `payload` of `agent` to the core at `socket` with `token`.
///
/// Does nothing without a token ([`HookError::NoToken`], checked first, before any file is read)
/// and for payloads that mean nothing ([`Outcome::Ignored`]); otherwise sends the request
/// [`prepare`] builds, within [`BUDGET`]. Never starts the core and never writes to stdout.
pub fn run(
    agent: AgentKind,
    payload: Option<&str>,
    socket: Option<&Path>,
    token: Option<&Key>,
) -> Result<Outcome, HookError> {
    let token = token.ok_or(HookError::NoToken)?;
    let Some(request) = payload.and_then(|payload| prepare(agent, payload)) else {
        return Ok(Outcome::Ignored);
    };
    let socket = socket.ok_or(HookError::NoSocket)?;
    match send(socket, token, &request)? {
        Response::Error { code, message } => Err(HookError::Refused { code, message }),
        response => Ok(Outcome::Sent(Box::new(response))),
    }
}

/// The request for `payload`, with the branch from `.git/HEAD` of its `cwd` ([`git_head`]) and,
/// for a report, the parent processes ([`parent_pids`]) and the session's name
/// ([`session_name`]); `None` when the event means nothing.
pub fn prepare(agent: AgentKind, payload: &str) -> Option<Request> {
    let head = payload_cwd(payload).and_then(|cwd| git_head(&cwd));
    let mut intent = interpret(agent, payload, head.as_deref());
    // Reading /proc and the agent's files only pays off for a report: the pids find the window to
    // jump to, the name says which job it is. `PostToolUse` comes often and reads nothing.
    let pids = match &mut intent {
        HookIntent::Report { agent: info, .. } => {
            info.title = session_name(agent, payload);
            parent_pids()
        }
        _ => Vec::new(),
    };
    request_for(intent, pids)
}

/// Codex's index of sessions, in [`codex_home`].
pub const CODEX_INDEX: &str = "session_index.jsonl";

/// The name `agent` gave the session of `payload` (task E2.6), read from the end of one file
/// ([`title_source`]): Claude Code's transcript, Codex's index in [`codex_home`] (from
/// `CODEX_HOME` and the home directory of this process). `None` for Gemini CLI and whenever the
/// file is missing, unreadable or names no session; never fails.
pub fn session_name(agent: AgentKind, payload: &str) -> Option<String> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    session_name_in(agent, payload, codex_home(std::env::var_os("CODEX_HOME"), home).as_deref())
}

/// [`session_name`] with Codex's home directory given (`None`: unknown, so Codex has no name).
fn session_name_in(agent: AgentKind, payload: &str, codex_home: Option<&Path>) -> Option<String> {
    match title_source(agent, payload)? {
        TitleSource::Transcript(path) => title_in_transcript(&read_tail(&path, TITLE_TAIL_MAX)?),
        TitleSource::CodexIndex { session } => {
            title_in_codex_index(&read_tail(&codex_home?.join(CODEX_INDEX), TITLE_TAIL_MAX)?, &session)
        }
    }
}

/// Where Codex keeps its files: `codex_home_var` (the content of `CODEX_HOME`) when it is set and
/// not empty, else `.codex` in `home`; `None` without either.
pub fn codex_home(codex_home_var: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let not_empty = |value: &OsString| !value.is_empty();
    match codex_home_var.filter(not_empty) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => home.filter(not_empty).map(|home| PathBuf::from(home).join(".codex")),
    }
}

/// The last `max` bytes of the regular file `path` as text, from the first whole line on (when
/// the file is longer, the cut falls inside a line, which is dropped); bytes that are no UTF-8
/// become U+FFFD, so a cut through a character spoils only that character. `None` when the file
/// is missing, no regular file (a named pipe would wait for a writer) or cannot be read.
pub fn read_tail(path: &Path, max: u64) -> Option<String> {
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let start = file.metadata().ok()?.len().saturating_sub(max);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(max).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
    Some(text.to_string())
}

/// The request an intent becomes. A report is `inbox.post_item` with `pids` in its agent; both
/// resolving intents are `inbox.resolve_items`, because one session is one entry (posts merge by
/// `thread_key`), so "the session went on" and "the session ended" resolve the same entry.
///
/// The title of a report is only a fallback: the core writes its own for a hook client. For any
/// other client "waiting" and "done" become notices, which then carry this title.
pub fn request_for(intent: HookIntent, pids: Vec<u32>) -> Option<Request> {
    match intent {
        HookIntent::Report { kind, mut agent, thread_key, body } => {
            let name = agent.agent.label();
            let title = match kind {
                Kind::AgentWaiting => trf("{agent} is waiting for you", &[("agent", &name)]),
                _ => trf("{agent} finished", &[("agent", &name)]),
            };
            agent.pids = pids;
            Some(Request::PostItem {
                kind,
                title,
                body,
                link: None,
                lane: None,
                thread_key: Some(thread_key),
                agent: Some(agent),
            })
        }
        HookIntent::ResolveWaiting { thread_key } | HookIntent::ResolveSession { thread_key } => {
            Some(Request::ResolveItems { thread_key })
        }
        HookIntent::Nothing => None,
    }
}

/// Sends `request` with `key` to the core at `socket`; connecting and the answer share [`BUDGET`].
fn send(socket: &Path, key: &Key, request: &Request) -> Result<Response, HookError> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(HookError::Runtime)?;
    runtime.block_on(call(socket, key, request))
}

/// One call to the core within [`BUDGET`], on the caller's runtime. It comes through MCP (#231,
/// `via: mcp`): the hooks belong to an agent's connection, and without a key file `TORRO_TOKEN`
/// may hold the agent's own key, which the core takes only that way.
async fn call(socket: &Path, key: &Key, request: &Request) -> Result<Response, HookError> {
    let call = torro_ipc::client::call_with_key_via(socket, Some(key), torro_ipc::Door::Mcp, request);
    tokio::time::timeout(BUDGET, call).await.map_err(|_| HookError::TimedOut)?.map_err(HookError::Ipc)
}

/// Puts `question` of `agent` to the person through the core at `socket` with `token` and waits
/// up to `wait` for the decision (task E2.5). `Ok(Some(json))` is what to print for the agent
/// once the person allowed or denied ([`permission_answer`]); `Ok(None)` when nobody decided in
/// time, the question expired, or it cannot be answered (Gemini CLI, a question the core could
/// not settle); `Err` when the core refused the question or could not be reached.
///
/// Reads only the approval the core answered the question with, by its id: the hook takes no
/// other approval's decision for its own ([`decision_of`]). Each call has [`BUDGET`]; between
/// looks it sleeps [`ANSWER_POLL`]. A look that runs out of time is tried again while the wait
/// lasts; only a refused or failed one ends the wait.
pub fn answer(
    agent: AgentKind,
    question: &PermissionQuestion,
    socket: Option<&Path>,
    token: &Key,
    wait: Duration,
) -> Result<Option<String>, HookError> {
    if permission_answer(agent, true).is_none() {
        return Ok(None);
    }
    let socket = socket.ok_or(HookError::NoSocket)?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(HookError::Runtime)?;
    let allowed = runtime.block_on(ask_and_wait(question, socket, token, wait))?;
    Ok(allowed.and_then(|allow| permission_answer(agent, allow)))
}

/// The question and the looks of [`answer`]: `Some(true)` allowed, `Some(false)` denied, `None`
/// no decision.
async fn ask_and_wait(
    question: &PermissionQuestion,
    socket: &Path,
    token: &Key,
    wait: Duration,
) -> Result<Option<bool>, HookError> {
    let deadline = tokio::time::Instant::now() + wait;
    let ask = Request::AskPermission {
        tool: question.tool.clone(),
        details: question.details.clone(),
        project: question.project.clone(),
        title: question.title.clone(),
    };
    let asked = match call(socket, token, &ask).await? {
        Response::Pending { approval } if crate::approvals::is_question(&approval) => approval,
        Response::Error { code, message } => return Err(HookError::Refused { code, message }),
        _ => return Err(HookError::Unexpected),
    };
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(ANSWER_POLL.min(deadline - now)).await;
        let status = Request::ApprovalStatus { id: asked.id.clone() };
        // One slow look is no reason to give up on the person: look again until the deadline.
        let looked = match call(socket, token, &status).await {
            Err(HookError::TimedOut) => continue,
            looked => looked?,
        };
        match looked {
            Response::Approval { approval } => match decision_of(&approval, &asked.id) {
                Decided::Waiting => {}
                Decided::Allowed => return Ok(Some(true)),
                Decided::Denied => return Ok(Some(false)),
                Decided::Never => return Ok(None),
            },
            Response::Error { code, message } => return Err(HookError::Refused { code, message }),
            _ => return Err(HookError::Unexpected),
        }
    }
}

/// Where a question stands for the hook, from its approval ([`decision_of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decided {
    /// Nobody decided yet: look again.
    Waiting,
    /// The person allowed it, and the core settled it (`done` with `ok`).
    Allowed,
    /// The person denied it.
    Denied,
    /// It will never be decided: expired, settled with an error (the hook client was removed
    /// meanwhile, say), or not this hook's question.
    Never,
}

/// Where the question `id` stands by `approval`, as the core answered `approvals.approval_status`.
/// Only an approval with exactly this id that is a question counts; anything else is
/// [`Decided::Never`]. Allowed means settled with `ok`: `running` (allowed, not stored yet) is
/// still [`Decided::Waiting`], and `done` with an error is no permission.
pub fn decision_of(approval: &ApprovalInfo, id: &str) -> Decided {
    if approval.id != id || !crate::approvals::is_question(approval) {
        return Decided::Never;
    }
    match &approval.state {
        ApprovalState::Pending | ApprovalState::Running => Decided::Waiting,
        ApprovalState::Done { response } if **response == Response::Ok => Decided::Allowed,
        ApprovalState::Denied => Decided::Denied,
        ApprovalState::Done { .. } | ApprovalState::Expired => Decided::Never,
    }
}

/// Stdin as text, read on a thread so a pipe left open costs at most [`STDIN_WAIT`]; the thread
/// ends with the process. `None` when stdin does not end in time or is too long or not UTF-8.
fn read_stdin() -> Option<String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("hook-stdin".into())
        .spawn(move || {
            let _ = sender.send(read_bounded(io::stdin().lock(), STDIN_MAX));
        })
        .ok()?;
    receiver.recv_timeout(STDIN_WAIT).ok().flatten()
}

/// All of `reader` as UTF-8 text when it holds at most `limit` bytes, else `None`.
pub fn read_bounded(reader: impl Read, limit: u64) -> Option<String> {
    let mut bytes = Vec::new();
    reader.take(limit.saturating_add(1)).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// The `cwd` field of a payload; `None` when it is missing or the payload is no JSON object.
fn payload_cwd(payload: &str) -> Option<PathBuf> {
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;
    json.get("cwd")?.as_str().filter(|cwd| !cwd.is_empty()).map(PathBuf::from)
}

/// The content of `HEAD` of the repository `cwd` lies in: the nearest `.git` in `cwd` or above.
/// A `.git` directory holds `HEAD`; a `.git` file (a linked worktree or a submodule) names the
/// git directory as `gitdir: <path>`, relative to the file's directory or absolute. `None`
/// outside a repository or when a file cannot be read. Reads files only, starts no process.
pub fn git_head(cwd: &Path) -> Option<String> {
    let (dir, dot_git) = cwd.ancestors().map(|dir| (dir, dir.join(".git"))).find(|(_, dot_git)| dot_git.exists())?;
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let link = read_small(&dot_git)?;
        let target = link.trim().strip_prefix("gitdir:")?.trim();
        if target.is_empty() {
            return None;
        }
        dir.join(target)
    };
    read_small(&git_dir.join("HEAD"))
}

/// A short text file, at most [`SMALL_FILE_MAX`] bytes; `None` when it is longer or unreadable.
fn read_small(path: &Path) -> Option<String> {
    read_bounded(std::fs::File::open(path).ok()?, SMALL_FILE_MAX)
}

/// The parent processes of this process, nearest first, at most [`PARENTS_MAX`], without init
/// (pid 1, `launchd` on macOS). Read from `/proc/<pid>/stat` on Linux and from one run of
/// `/bin/ps` on macOS ([`parents_from_ps`]); empty on other systems and when reading fails.
pub fn parent_pids() -> Vec<u32> {
    #[cfg(target_os = "linux")]
    {
        parent_chain(Path::new("/proc"), std::process::id())
    }
    #[cfg(target_os = "macos")]
    {
        parents_from_ps(std::process::id())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Vec::new()
    }
}

/// The chain of parents of `pid` under the proc file system at `proc` ([`walk_parents`] over
/// each `stat`).
#[cfg(target_os = "linux")]
fn parent_chain(proc: &Path, pid: u32) -> Vec<u32> {
    walk_parents(pid, |current| {
        read_small(&proc.join(current.to_string()).join("stat")).and_then(|stat| parent_of(&stat))
    })
}

/// The chain of parents of `pid` from `ps -A -o pid= -o ppid=` (macOS has no `/proc`; one run
/// lists every process, cheaper than one run per parent). Empty when `ps` cannot run.
#[cfg(target_os = "macos")]
fn parents_from_ps(pid: u32) -> Vec<u32> {
    let listed = std::process::Command::new("/bin/ps")
        .args(["-A", "-o", "pid=", "-o", "ppid="])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match listed {
        Ok(output) if output.status.success() => {
            let table = parent_table(&String::from_utf8_lossy(&output.stdout));
            walk_parents(pid, |current| table.get(&current).copied())
        }
        _ => Vec::new(),
    }
}

/// Parent by process from the lines `<pid> <ppid>` that `ps -o pid= -o ppid=` prints; lines that
/// are not two numbers are skipped.
#[cfg(any(target_os = "macos", test))]
fn parent_table(ps: &str) -> std::collections::HashMap<u32, u32> {
    ps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let parent = fields.next()?.parse().ok()?;
            Some((pid, parent))
        })
        .collect()
}

/// The parents of `pid`, nearest first, following `parent_of` until pid 1 or 0, an unknown
/// process or [`PARENTS_MAX`] entries.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn walk_parents(pid: u32, parent_of: impl Fn(u32) -> Option<u32>) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut current = pid;
    while chain.len() < PARENTS_MAX {
        let Some(parent) = parent_of(current) else {
            break;
        };
        // Init adopts orphans and is no window's process; a loop would be a broken process table.
        if parent <= 1 || chain.contains(&parent) {
            break;
        }
        chain.push(parent);
        current = parent;
    }
    chain
}

/// The parent id in the content of `/proc/<pid>/stat`: `pid (comm) state ppid …`. The command
/// name may hold spaces and parentheses, so the fields count from the last `)`.
#[cfg(target_os = "linux")]
fn parent_of(stat: &str) -> Option<u32> {
    let (_, fields) = stat.rsplit_once(')')?;
    fields.split_whitespace().nth(1)?.parse().ok()
}

/// `text` on one line: a debug message may quote the core, and stderr is the agent's terminal.
fn one_line(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use torro_inbox::AgentInfo;

    use super::*;

    const HEAD: &str = "ref: refs/heads/feat/inbox\n";

    /// How long a test waits for what a core does on its own (starting, showing a question)
    /// before it fails: far more than a loaded machine needs, and a passing test never waits it
    /// out, because it stops looking as soon as it sees what it waits for.
    #[cfg(unix)]
    const SETTLE: Duration = Duration::from_secs(30);

    /// The hook payloads of E2.2a, shared with `torro-inbox`.
    fn fixture(name: &str) -> String {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../torro-inbox/tests/fixtures/hooks");
        std::fs::read_to_string(dir.join(format!("{name}.json"))).expect("fixture exists")
    }

    fn request(agent: AgentKind, name: &str) -> Option<Request> {
        request_for(interpret(agent, &fixture(name), Some(HEAD)), vec![42, 7])
    }

    fn post(agent: AgentKind, session: &str, kind: Kind, body: &str) -> Option<Request> {
        let title = match kind {
            Kind::AgentWaiting => trf("{agent} is waiting for you", &[("agent", &agent.label())]),
            _ => trf("{agent} finished", &[("agent", &agent.label())]),
        };
        Some(Request::PostItem {
            kind,
            title,
            body: Some(body.into()),
            link: None,
            lane: None,
            thread_key: Some(format!("{}:{session}", agent.id())),
            agent: Some(AgentInfo {
                agent,
                session: session.into(),
                project: Some("my-project".into()),
                branch: Some("feat/inbox".into()),
                pids: vec![42, 7],
                title: None,
            }),
        })
    }

    fn resolve(key: &str) -> Option<Request> {
        Some(Request::ResolveItems { thread_key: key.into() })
    }

    #[test]
    fn claude_code_events_become_requests() {
        use AgentKind::ClaudeCode;
        let cases = [
            (
                "claude-code-notification-permission",
                post(ClaudeCode, "abc123", Kind::AgentWaiting, "Claude needs your permission to use Bash"),
            ),
            (
                "claude-code-notification-needs-input",
                post(ClaudeCode, "abc123", Kind::AgentWaiting, "Claude is waiting for your input"),
            ),
            (
                "claude-code-notification-elicitation",
                post(ClaudeCode, "abc123", Kind::AgentWaiting, "The MCP server asks for a value"),
            ),
            (
                "claude-code-stop",
                post(ClaudeCode, "abc123", Kind::AgentDone, "I've completed the refactoring. Here's a summary..."),
            ),
            ("claude-code-user-prompt-submit", resolve("claude-code:abc123")),
            ("claude-code-post-tool-use", resolve("claude-code:abc123")),
            ("claude-code-session-end", resolve("claude-code:abc123")),
            ("claude-code-notification-idle", None),
            ("claude-code-subagent-stop", None),
        ];
        for (name, expected) in cases {
            assert_eq!(request(ClaudeCode, name), expected, "{name}");
        }
    }

    #[test]
    fn codex_events_become_requests() {
        use AgentKind::Codex;
        let cases = [
            (
                "codex-permission-request",
                post(Codex, "thr_123", Kind::AgentWaiting, "Run the test suite outside the sandbox"),
            ),
            (
                "codex-stop",
                post(Codex, "thr_123", Kind::AgentDone, "All tests pass. I updated the parser and added two fixtures."),
            ),
            ("codex-user-prompt-submit", resolve("codex:thr_123")),
            ("codex-post-tool-use", resolve("codex:thr_123")),
            ("codex-session-end", resolve("codex:thr_123")),
        ];
        for (name, expected) in cases {
            assert_eq!(request(Codex, name), expected, "{name}");
        }
    }

    #[test]
    fn gemini_events_become_requests() {
        use AgentKind::Gemini;
        let cases = [
            ("gemini-notification", post(Gemini, "7f3a", Kind::AgentWaiting, "Gemini CLI wants to run: npm test")),
            (
                "gemini-after-agent",
                post(Gemini, "7f3a", Kind::AgentDone, "The test failed because of a missing import. It passes now."),
            ),
            ("gemini-before-agent", resolve("gemini:7f3a")),
            ("gemini-session-end", resolve("gemini:7f3a")),
        ];
        for (name, expected) in cases {
            assert_eq!(request(Gemini, name), expected, "{name}");
        }
    }

    #[test]
    fn unknown_events_send_nothing() {
        let unknown = r#"{"session_id":"s","hook_event_name":"SomethingNew","cwd":"/tmp"}"#;
        for agent in AgentKind::ALL {
            assert_eq!(prepare(agent, unknown), None);
            assert_eq!(prepare(agent, "not json"), None);
        }
        // Nothing to send, so not even a socket is needed: the run ends before connecting.
        let key = Key::new("torro_c1_x");
        assert_eq!(run(AgentKind::ClaudeCode, Some(unknown), None, Some(&key)).ok(), Some(Outcome::Ignored));
        assert_eq!(run(AgentKind::ClaudeCode, None, None, Some(&key)).ok(), Some(Outcome::Ignored));
    }

    /// A hook fixture of the payloads with `transcript_path` pointing at `transcript`.
    fn with_transcript(name: &str, transcript: &Path) -> String {
        let mut json: serde_json::Value = serde_json::from_str(&fixture(name)).expect("JSON");
        json["transcript_path"] = transcript.to_string_lossy().into_owned().into();
        json.to_string()
    }

    fn fixture_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../torro-inbox/tests/fixtures/hooks").join(name)
    }

    #[test]
    fn a_report_carries_the_session_name_of_claude_code() {
        let transcript = fixture_path("claude-code-transcript.jsonl");
        let payload = with_transcript("claude-code-stop", &transcript);
        let Some(Request::PostItem { agent: Some(agent), title, .. }) = prepare(AgentKind::ClaudeCode, &payload) else {
            panic!("a report");
        };
        assert_eq!(agent.title.as_deref(), Some("Login redirect (staging)"));
        assert_eq!(title, trf("{agent} finished", &[("agent", &"Claude Code")]), "the name is no part of the title");
        // Without a transcript the report goes out as before, without a name.
        let missing = with_transcript("claude-code-stop", &transcript.with_file_name("missing.jsonl"));
        let Some(Request::PostItem { agent: Some(agent), .. }) = prepare(AgentKind::ClaudeCode, &missing) else {
            panic!("a report");
        };
        assert_eq!(agent.title, None);
        // Going on reads nothing and sends no name: there is none in the request.
        let going_on = with_transcript("claude-code-post-tool-use", &transcript);
        assert_eq!(prepare(AgentKind::ClaudeCode, &going_on), resolve("claude-code:abc123"));
    }

    #[test]
    fn only_the_end_of_a_large_transcript_is_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let message = r#"{"type":"assistant","message":{"role":"assistant","content":"lots of work"}}"#;
        let filler = format!("{message}\n").repeat((TITLE_TAIL_MAX as usize / message.len()) * 2);
        let title = |name: &str| format!("{{\"type\":\"custom-title\",\"customTitle\":\"{name}\"}}\n");
        let payload = |file: &Path| with_transcript("claude-code-stop", file);

        let at_end = dir.path().join("end.jsonl");
        std::fs::write(&at_end, format!("{}{filler}{}", title("Old name"), title("Big job"))).expect("write");
        assert!(std::fs::metadata(&at_end).expect("file").len() > 2 * TITLE_TAIL_MAX);
        assert_eq!(session_name_in(AgentKind::ClaudeCode, &payload(&at_end), None).as_deref(), Some("Big job"));

        // A title further back than the tail is not looked for.
        let at_start = dir.path().join("start.jsonl");
        std::fs::write(&at_start, format!("{}{filler}", title("Too far back"))).expect("write");
        assert_eq!(session_name_in(AgentKind::ClaudeCode, &payload(&at_start), None), None);

        // A directory or an empty file is no transcript.
        assert_eq!(session_name_in(AgentKind::ClaudeCode, &payload(dir.path()), None), None);
        let empty = dir.path().join("empty.jsonl");
        std::fs::write(&empty, "").expect("write");
        assert_eq!(session_name_in(AgentKind::ClaudeCode, &payload(&empty), None), None);
    }

    #[test]
    fn the_tail_starts_at_a_whole_line() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("t.jsonl");
        std::fs::write(&file, "first line\nsecond\nthird\n").expect("write");
        assert_eq!(read_tail(&file, 1000).as_deref(), Some("first line\nsecond\nthird\n"), "short: all of it");
        assert_eq!(read_tail(&file, 10).as_deref(), Some("third\n"), "the cut line is dropped");
        // A cut through a character spoils only that character.
        std::fs::write(&file, "äöü\n").expect("write");
        assert_eq!(read_tail(&file, 5).as_deref(), Some(""), "one line, cut: nothing whole");
        assert_eq!(read_tail(&dir.path().join("missing"), 10), None);
    }

    #[test]
    fn a_codex_session_is_named_from_its_index() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::copy(fixture_path("codex-session-index.jsonl"), dir.path().join(CODEX_INDEX)).expect("copy");
        let payload = fixture("codex-stop");
        assert_eq!(
            session_name_in(AgentKind::Codex, &payload, Some(dir.path())).as_deref(),
            Some("Parser fixtures and docs")
        );
        assert_eq!(session_name_in(AgentKind::Codex, &payload, None), None, "no home, no index");
        let elsewhere = tempfile::tempdir().expect("temp dir");
        assert_eq!(session_name_in(AgentKind::Codex, &payload, Some(elsewhere.path())), None, "no index");
        // Gemini CLI names no session.
        assert_eq!(session_name_in(AgentKind::Gemini, &fixture("gemini-after-agent"), Some(dir.path())), None);
    }

    #[test]
    fn codex_lives_in_codex_home_or_in_the_home_directory() {
        let os = |text: &str| Some(OsString::from(text));
        assert_eq!(codex_home(os("/opt/codex"), os("/home/anna")), Some(PathBuf::from("/opt/codex")));
        assert_eq!(codex_home(os(""), os("/home/anna")), Some(PathBuf::from("/home/anna/.codex")));
        assert_eq!(codex_home(None, os("/home/anna")), Some(PathBuf::from("/home/anna/.codex")));
        assert_eq!(codex_home(None, os("")), None);
        assert_eq!(codex_home(None, None), None);
    }

    #[test]
    fn without_a_token_nothing_is_read_or_sent() {
        let payload = fixture("claude-code-stop");
        assert!(matches!(run(AgentKind::ClaudeCode, Some(&payload), None, None), Err(HookError::NoToken)));
    }

    #[test]
    fn the_agent_comes_from_the_first_argument() {
        let args = |words: &[&str]| words.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        let agent = |words: &[&str]| parse_args(&args(words)).map(|parsed| parsed.agent);
        assert_eq!(agent(&["claude-code"]), Some(AgentKind::ClaudeCode));
        assert_eq!(agent(&["codex"]), Some(AgentKind::Codex));
        assert_eq!(agent(&["gemini"]), Some(AgentKind::Gemini));
        for bad in [
            &[][..],
            &["claude"],
            &["codex", "extra"],
            &["codex", "--key-file"],
            &["codex", "--key-file", ""],
            &["codex", "--token", "/k"],
            &["nobody", "--key-file", "/k"],
            &["codex", "--key-file", "/k", "extra"],
        ] {
            assert_eq!(parse_args(&args(bad)), None, "{bad:?}");
        }
        assert_eq!(
            parse_args(&args(&["gemini", "--key-file", "/home/an na/k.key"])),
            Some(HookArgs {
                agent: AgentKind::Gemini,
                key_file: Some(PathBuf::from("/home/an na/k.key")),
                answer: false
            })
        );
        assert_eq!(
            parse_args(&args(&["codex"])),
            Some(HookArgs { agent: AgentKind::Codex, key_file: None, answer: false })
        );
    }

    #[test]
    fn answer_comes_with_or_without_a_key_file_once() {
        let args = |words: &[&str]| words.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        let file = Some(PathBuf::from("/c/hooks/h1.key"));
        let answering = HookArgs { agent: AgentKind::ClaudeCode, key_file: file.clone(), answer: true };
        assert_eq!(
            parse_args(&args(&["claude-code", "--key-file", "/c/hooks/h1.key", "--answer"])),
            Some(answering.clone())
        );
        assert_eq!(parse_args(&args(&["claude-code", "--answer", "--key-file", "/c/hooks/h1.key"])), Some(answering));
        assert_eq!(
            parse_args(&args(&["codex", "--answer"])),
            Some(HookArgs { agent: AgentKind::Codex, key_file: None, answer: true })
        );
        for bad in [
            &["codex", "--answer", "--answer"][..],
            &["codex", "--key-file", "/a", "--key-file", "/b"],
            &["codex", "--answer", "--key-file"],
            &["codex", "--answer=yes"],
            &["--answer", "codex"],
        ] {
            assert_eq!(parse_args(&args(bad)), None, "{bad:?}");
        }
    }

    /// A key file like the wizard writes it: `text` with mode `mode` (Unix).
    fn key_file(dir: &Path, text: &str, mode: u32) -> PathBuf {
        let file = dir.join("hook.key");
        std::fs::write(&file, text).expect("key file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).expect("chmod");
        }
        #[cfg(not(unix))]
        let _ = mode;
        file
    }

    fn with_file(file: &Path) -> HookArgs {
        HookArgs { agent: AgentKind::ClaudeCode, key_file: Some(file.to_path_buf()), answer: false }
    }

    #[test]
    fn the_key_comes_from_a_private_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = key_file(dir.path(), "torro_h1_secret\n", 0o600);
        let exposed = |key: Option<Key>| key.map(|key| key.expose().to_string());
        assert_eq!(exposed(read_key_file(&file).ok()).as_deref(), Some("torro_h1_secret"), "trimmed");
        // The file wins over TORRO_TOKEN; without a file the variable is used.
        let key = key_for(&with_file(&file), Some("torro_env".into())).expect("read");
        assert_eq!(exposed(key).as_deref(), Some("torro_h1_secret"));
        let plain = HookArgs { agent: AgentKind::ClaudeCode, key_file: None, answer: false };
        assert_eq!(exposed(key_for(&plain, Some(" torro_env ".into())).expect("env")).as_deref(), Some("torro_env"));
        assert!(key_for(&plain, None).expect("none").is_none());
        #[cfg(unix)]
        assert!(read_key_file(&key_file(dir.path(), "torro_h1_secret", 0o400)).is_ok(), "read-only is private too");
    }

    #[test]
    fn a_missing_long_or_blank_key_file_gives_no_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("gone.key");
        assert!(matches!(read_key_file(&missing), Err(KeyFileError::Unreadable(_))));
        // A broken file never falls back to TORRO_TOKEN.
        let refused = key_for(&with_file(&missing), Some("torro_env".into()));
        assert!(matches!(refused, Err(HookError::KeyFile { reason: KeyFileError::Unreadable(_), .. })));
        for text in ["", "  \n", &"k".repeat(KEY_FILE_MAX as usize + 1)] {
            let file = key_file(dir.path(), text, 0o600);
            assert!(matches!(read_key_file(&file), Err(KeyFileError::NoKey)), "{} bytes", text.len());
        }
        assert!(matches!(read_key_file(dir.path()), Err(KeyFileError::NoKey)), "a directory is no key file");
    }

    #[cfg(unix)]
    #[test]
    fn a_key_file_others_can_open_is_not_used() {
        let dir = tempfile::tempdir().expect("temp dir");
        for mode in [0o644, 0o640, 0o604, 0o620, 0o602, 0o610, 0o666] {
            let file = key_file(dir.path(), "torro_h1_secret", mode);
            let refused = read_key_file(&file);
            assert!(matches!(refused, Err(KeyFileError::TooOpen { mode: m }) if m == mode), "{mode:o}");
            // The debug line names the file and the mode, never the key.
            let err = key_for(&with_file(&file), None).expect_err("refused");
            let line = err.to_string();
            assert!(line.contains(&format!("{mode:o}")) && !line.contains("secret"), "{line}");
        }
    }

    /// The owner check: the uid comes from `/proc/self/status`, and a file this process created is
    /// its own. (A file of another user cannot be made without root.)
    #[cfg(target_os = "linux")]
    #[test]
    fn the_owner_is_the_user_the_hook_runs_as() {
        use std::os::unix::fs::MetadataExt;
        let status = "Name:\ttorroworkplace\nUid:\t1000\t1001\t1000\t1000\nGid:\t100\t100\t100\t100\n";
        assert_eq!(effective_uid_in(status), Some(1001), "the effective uid");
        assert_eq!(effective_uid_in("Name:\tx\n"), None);
        let dir = tempfile::tempdir().expect("temp dir");
        let file = key_file(dir.path(), "torro_h1_secret", 0o600);
        let owner = std::fs::metadata(&file).expect("metadata").uid();
        assert_eq!(effective_uid(), Some(owner));
    }

    #[test]
    fn without_a_usable_key_file_nothing_is_sent() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A socket that would answer is not even needed: the hook stops before connecting.
        let payload = fixture("claude-code-stop");
        let started = Instant::now();
        let result = key_for(&with_file(&dir.path().join("gone.key")), None)
            .and_then(|key| run(AgentKind::ClaudeCode, Some(&payload), None, key.as_ref()));
        assert!(matches!(result, Err(HookError::KeyFile { .. })), "{result:?}");
        assert!(started.elapsed() < BUDGET);
    }

    #[test]
    fn stdin_is_bounded() {
        assert_eq!(read_bounded(&b"{}"[..], 2).as_deref(), Some("{}"));
        assert_eq!(read_bounded(&b"{ }"[..], 2), None, "one byte too many is dropped whole");
        assert_eq!(read_bounded(&[0xff, 0xfe][..], 16), None, "not UTF-8");
        let big = vec![b' '; STDIN_MAX as usize + 1];
        assert_eq!(read_bounded(&big[..], STDIN_MAX), None);
        assert!(read_bounded(&big[1..], STDIN_MAX).is_some());
    }

    #[test]
    fn head_of_a_repository_and_of_a_linked_worktree() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".git/worktrees/side")).expect("git dir");
        std::fs::create_dir_all(repo.join("src/deep")).expect("sub dir");
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").expect("head");
        assert_eq!(git_head(&repo.join("src/deep")).as_deref(), Some("ref: refs/heads/main\n"));

        // A linked worktree: `.git` is a file naming its git directory, absolute …
        let side = dir.path().join("side");
        std::fs::create_dir_all(&side).expect("worktree");
        std::fs::write(repo.join(".git/worktrees/side/HEAD"), "ref: refs/heads/feat/side\n").expect("head");
        let gitdir = repo.join(".git/worktrees/side");
        std::fs::write(side.join(".git"), format!("gitdir: {}\n", gitdir.display())).expect("link");
        assert_eq!(git_head(&side).as_deref(), Some("ref: refs/heads/feat/side\n"));
        // … or relative to the worktree.
        std::fs::write(side.join(".git"), "gitdir: ../repo/.git/worktrees/side\n").expect("link");
        assert_eq!(git_head(&side).as_deref(), Some("ref: refs/heads/feat/side\n"));

        // The whole way: the payload's cwd in the worktree gives the branch. Written as JSON, so the
        // backslashes of a Windows path stay a valid string.
        let cwd = serde_json::to_string(side.to_str().expect("utf-8 temp path")).expect("json string");
        let payload = fixture("claude-code-stop").replace("\"/home/user/my-project\"", &cwd);
        let Some(Request::PostItem { agent: Some(agent), .. }) = prepare(AgentKind::ClaudeCode, &payload) else {
            panic!("a stop is a post");
        };
        assert_eq!((agent.project.as_deref(), agent.branch.as_deref()), (Some("side"), Some("feat/side")));

        std::fs::write(side.join(".git"), "not a link\n").expect("broken link");
        assert_eq!(git_head(&side), None);
        // A missing directory is walked up like any other.
        assert_eq!(git_head(&dir.path().join("nowhere")), git_head(dir.path()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parents_come_from_proc_stat() {
        let proc = tempfile::tempdir().expect("temp dir");
        let stat = |pid: u32, comm: &str, parent: u32| {
            let dir = proc.path().join(pid.to_string());
            std::fs::create_dir_all(&dir).expect("pid dir");
            std::fs::write(dir.join("stat"), format!("{pid} ({comm}) S {parent} {pid} 0 0 -1")).expect("stat");
        };
        stat(300, "torroworkplace", 200);
        stat(200, "sh -c (hook)", 100);
        stat(100, "claude", 1);
        assert_eq!(parent_chain(proc.path(), 300), [200, 100]);
        assert_eq!(parent_chain(proc.path(), 999), Vec::<u32>::new(), "unknown process");
        // A loop in a broken /proc ends, and a deep chain stops at the limit.
        stat(400, "a", 401);
        stat(401, "b", 400);
        assert_eq!(parent_chain(proc.path(), 400), [401, 400]);
        for pid in 1000..1100 {
            stat(pid, "deep", pid + 1);
        }
        assert_eq!(parent_chain(proc.path(), 1000).len(), PARENTS_MAX);
        // The real chain of this test reaches at least the test runner.
        assert!(!parent_pids().is_empty());
        assert_eq!(parent_of("12 (a) b) R 34 12"), Some(34));
    }

    /// Task E8.9: macOS reads the chain from one run of `ps`, which lists every process.
    #[test]
    fn parents_come_from_the_process_list() {
        let ps = "    1     0\n  410   1\n  413   410\n  700   413\n  701   700\n  702 701\n garbage\n 800 x\n";
        let table = parent_table(ps);
        assert_eq!(table.len(), 6, "only lines of two numbers");
        // The hook (702) under `sh -c`, Claude Code, the shell, login, Terminal; launchd is left out.
        assert_eq!(walk_parents(702, |pid| table.get(&pid).copied()), [701, 700, 413, 410]);
        assert_eq!(walk_parents(999, |pid| table.get(&pid).copied()), Vec::<u32>::new(), "unknown process");
        let looped = parent_table("5 6\n6 5\n");
        assert_eq!(walk_parents(5, |pid| looped.get(&pid).copied()), [6, 5]);
        assert_eq!(walk_parents(1000, |pid| Some(pid + 1)).len(), PARENTS_MAX);
        #[cfg(target_os = "macos")]
        assert!(!parent_pids().is_empty(), "the real chain reaches at least the test runner");
    }

    #[test]
    fn without_a_core_the_hook_ends_quickly() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("state").join("core.sock");
        let payload = fixture("claude-code-notification-permission");
        let started = Instant::now();
        let result = run(AgentKind::ClaudeCode, Some(&payload), Some(&socket), Some(&Key::new("torro_c1_x")));
        assert!(matches!(result, Err(HookError::Ipc(IpcError::NotRunning))), "{result:?}");
        assert!(started.elapsed() < BUDGET, "{:?}", started.elapsed());
        assert!(!socket.exists(), "no core was started");
    }

    /// A core that takes the connection but never answers costs [`BUDGET`], not more.
    #[cfg(unix)]
    #[test]
    fn a_core_that_does_not_answer_times_out() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("core.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("bound");
        let payload = fixture("claude-code-stop");
        let started = Instant::now();
        let result = run(AgentKind::ClaudeCode, Some(&payload), Some(&socket), Some(&Key::new("torro_c1_x")));
        assert!(matches!(result, Err(HookError::TimedOut)), "{result:?}");
        // A loaded machine wakes the timer late; twice the budget still tells it from hanging.
        assert!(started.elapsed() < 2 * BUDGET, "{:?}", started.elapsed());
    }

    /// The hook writes nothing to stdout but a person's decision: no print macro and no stdout
    /// handle in this file outside the tests and [`write_decision`], which is called once, with
    /// what [`answer`] returns. (stdout of a test cannot be captured without `unsafe`, so the
    /// source is checked.)
    #[test]
    fn nothing_here_writes_to_stdout() {
        // A Windows checkout (core.autocrlf) ends lines with CRLF.
        let source = include_str!("hook.rs").replace("\r\n", "\n");
        let code = source.split("#[cfg(test)]").next().expect("code before the tests");
        let (before, writer) = code.split_once("fn write_decision(").expect("the one writer");
        let after = writer.split_once("\n}\n").expect("its end").1;
        let forbidden = ["print!(", "println!(", "stdout()", "io::stdout", "dbg!("];
        for word in forbidden {
            let in_code = [before, after]
                .iter()
                .flat_map(|part| part.lines())
                .filter(|line| !line.trim_start().starts_with("//"))
                .any(|line| line.contains(word));
            assert!(!in_code, "{word} in hook.rs");
        }
        let calls: Vec<&str> = [before, after]
            .iter()
            .flat_map(|part| part.lines())
            .filter(|line| !line.trim_start().starts_with("//") && line.contains("write_decision("))
            .collect();
        assert_eq!(calls, ["            Ok(Some(decision)) => return write_decision(&decision),"]);
    }

    /// An approval as the core answers `approvals.approval_status`.
    fn approval_in(state: ApprovalState, command: &str) -> ApprovalInfo {
        let now = chrono::Utc::now();
        ApprovalInfo {
            id: "9c1f0a7e3b2d4c58".into(),
            client: "h1".into(),
            client_name: "Claude Code hooks".into(),
            command: command.into(),
            summary: "Claude Code wants to use Bash: ls".into(),
            details: Vec::new(),
            created: now,
            expires: now + crate::approvals::QUESTION_LIFETIME,
            decided: None,
            state,
        }
    }

    #[test]
    fn only_the_own_question_settled_by_a_person_is_a_decision() {
        let question = crate::approvals::QUESTION_COMMAND;
        let id = "9c1f0a7e3b2d4c58";
        let done = |response| ApprovalState::Done { response: Box::new(response) };
        let cases = [
            (approval_in(ApprovalState::Pending, question), Decided::Waiting),
            (approval_in(ApprovalState::Running, question), Decided::Waiting),
            (approval_in(done(Response::Ok), question), Decided::Allowed),
            (approval_in(ApprovalState::Denied, question), Decided::Denied),
            (approval_in(ApprovalState::Expired, question), Decided::Never),
            (
                approval_in(done(Response::Error { code: "forbidden".into(), message: "x".into() }), question),
                Decided::Never,
            ),
            // Another command's approval, even allowed, answers no question.
            (approval_in(done(Response::Ok), "workspaces.delete_template"), Decided::Never),
        ];
        for (approval, expected) in cases {
            assert_eq!(decision_of(&approval, id), expected, "{:?} {}", approval.state, approval.command);
        }
        assert_eq!(decision_of(&approval_in(done(Response::Ok), question), "another"), Decided::Never);
    }

    /// The hook against a core in a temporary directory, with a hook client's key: waiting is an
    /// urgent entry, going on resolves it, a stop brings it back as done, the end resolves it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_hook_client_reports_and_resolves() {
        use torro_inbox::{Lane, SourceRef, State};

        let dir = tempfile::tempdir().expect("temp dir");
        let (socket, hook_key) = core_on_socket(dir.path(), AgentKind::ClaudeCode).await;
        let hook = |name: &'static str| {
            let (socket, key) = (socket.clone(), hook_key.clone());
            async move {
                tokio::task::spawn_blocking(move || {
                    run(AgentKind::ClaudeCode, Some(&fixture(name)), Some(&socket), Some(&key))
                })
                .await
                .expect("ran")
            }
        };
        let entries = || {
            let socket = socket.clone();
            async move {
                let all = Request::ListItems {
                    lane: None,
                    source: None,
                    state: None,
                    since: None,
                    limit: None,
                    order: None,
                    dismissed: None,
                    categories: Vec::new(),
                };
                match torro_ipc::client::call(&socket, &all).await {
                    Ok(Response::Items { items }) => items,
                    other => panic!("the app key lists the inbox: {other:?}"),
                }
            }
        };

        let posted = hook("claude-code-notification-permission").await;
        assert!(
            matches!(&posted, Ok(Outcome::Sent(answer)) if matches!(**answer, Response::Posted { id: Some(_) })),
            "{posted:?}"
        );
        let items = entries().await;
        let [waiting] = items.as_slice() else { panic!("one entry: {items:?}") };
        assert_eq!((waiting.kind, waiting.lane, waiting.urgent), (Kind::AgentWaiting, Lane::ForMe, true));
        assert_eq!(waiting.source, SourceRef::agent("claude-code"));
        assert_eq!(waiting.body.as_deref(), Some("Claude needs your permission to use Bash"));
        let agent = waiting.agent.as_ref().expect("agent info");
        assert_eq!((agent.session.as_str(), agent.project.as_deref()), ("abc123", Some("my-project")));
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert!(!agent.pids.is_empty(), "the parent chain is sent");

        // The same report with the key from a private file, as the wizard sets it up (E2.3b).
        let file = key_file(dir.path(), &format!("{}\n", hook_key.expose()), 0o600);
        let from_file = {
            let socket = socket.clone();
            tokio::task::spawn_blocking(move || {
                let key = key_for(&with_file(&file), None)?;
                run(
                    AgentKind::ClaudeCode,
                    Some(&fixture("claude-code-notification-permission")),
                    Some(&socket),
                    key.as_ref(),
                )
            })
            .await
            .expect("ran")
        };
        assert!(matches!(&from_file, Ok(Outcome::Sent(_))), "{from_file:?}");
        assert_eq!(entries().await.len(), 1, "same session, same entry");

        let resolved = hook("claude-code-post-tool-use").await;
        assert!(matches!(&resolved, Ok(Outcome::Sent(answer)) if **answer == Response::Ok), "{resolved:?}");
        assert!(matches!(entries().await[0].state, State::Done { .. }), "going on resolves the waiting");

        hook("claude-code-stop").await.expect("posted");
        let items = entries().await;
        assert_eq!(items.len(), 1, "same session, same entry");
        assert_eq!((items[0].kind, items[0].state.is_open()), (Kind::AgentDone, true));

        assert_eq!(hook("claude-code-notification-idle").await.ok(), Some(Outcome::Ignored));
        hook("claude-code-session-end").await.expect("resolved");
        assert!(matches!(entries().await[0].state, State::Done { .. }), "the end resolves the session");

        // An unknown key is refused by the core; the hook only notes it.
        let refused = tokio::task::spawn_blocking(move || {
            let unknown = Key::new("torro_00000000_nope");
            run(AgentKind::ClaudeCode, Some(&fixture("claude-code-stop")), Some(&socket), Some(&unknown))
        })
        .await
        .expect("ran");
        assert!(matches!(refused, Err(HookError::Refused { .. })), "{refused:?}");
    }

    /// #231: set up by hand, the hook takes `TORRO_TOKEN`, which may be the agent's own key: it
    /// comes through MCP, so the core takes it there, while the same key through the API is refused;
    /// with MCP turned off the hook is refused too.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_hook_reports_with_the_agents_own_key_through_mcp() {
        use torro_ipc::api::RightsTemplate;

        let dir = tempfile::tempdir().expect("temp dir");
        let (socket, _) = core_on_socket(dir.path(), AgentKind::ClaudeCode).await;
        let create = Request::CreateClient {
            name: "Claude Code".into(),
            kind: "claude-code".into(),
            template: RightsTemplate::ReadAndWrite,
            agent: None,
            rights: None,
        };
        let Ok(Response::ClientKey { key, .. }) = torro_ipc::client::call(&socket, &create).await else {
            panic!("the app key connects the agent");
        };
        let token = key.expose().to_string();
        let report = |token: String| {
            let socket = socket.clone();
            tokio::task::spawn_blocking(move || {
                let args = HookArgs { agent: AgentKind::ClaudeCode, key_file: None, answer: false };
                let key = key_for(&args, Some(token))?;
                run(AgentKind::ClaudeCode, Some(&fixture("claude-code-stop")), Some(&socket), key.as_ref())
            })
        };
        let sent = report(token.clone()).await.expect("ran");
        assert!(matches!(&sent, Ok(Outcome::Sent(_))), "{sent:?}");
        let api = torro_ipc::client::call_with_key(&socket, Some(&Key::new(token.as_str())), &Request::Ping).await;
        assert!(matches!(&api, Ok(Response::Error { code, .. }) if code == "unauthorized"), "{api:?}");

        let off = Request::SetSetting { key: torro_ipc::SettingKey::AllowMcp, value: serde_json::json!(false) };
        assert!(matches!(torro_ipc::client::call(&socket, &off).await, Ok(Response::Ok)));
        let refused = report(token).await.expect("ran");
        assert!(matches!(&refused, Err(HookError::Refused { code, .. }) if code == "unauthorized"), "{refused:?}");
    }

    /// A core in `dir` serving its socket (`dir/state/core.sock`, the app key next to it) with a
    /// hook client of `agent`; returns the socket and the hook client's key. Nothing of the
    /// user's: approvals, inbox and clients all live in `dir`, and no notification is shown.
    #[cfg(unix)]
    async fn core_on_socket(dir: &Path, agent: AgentKind) -> (PathBuf, Key) {
        use std::sync::Arc;

        use torro_ipc::api::Rights;
        use torro_setups::storage::Store;

        let clients = crate::clients::tests::open_in(dir);
        // A hook client is bound to one agent (task E1.2b).
        let name = format!("{} hooks", agent.label());
        let (_, hook_key) = clients.add_client(&name, "hook", Some(agent), Rights::hook()).await.expect("added");
        let settings = crate::settings_service::SettingsStore::at(dir.join("config").join("settings.json"));
        let core = Arc::new(
            crate::core::Core::new(
                torro_wm::Backend::Unsupported { name: "test".into() },
                Store::at(dir.to_path_buf()),
                Arc::new(crate::pim::PimService::with_settings(settings)),
                clients,
            )
            .with_approvals(crate::approvals::tests::quiet_in(dir))
            .with_inbox(crate::inbox::tests::inbox_in(dir)),
        );
        let socket = dir.join("state").join("core.sock");
        let path = socket.clone();
        tokio::spawn(async move {
            torro_ipc::server::serve_with_key(&path, move |key, door, request| {
                let core = Arc::clone(&core);
                async move { core.handle_from(key.as_ref(), door, request).await.unwrap_or_else(|r| r.response()) }
            })
            .await
        });
        // Ready once it answers, however long a loaded machine takes to bind the socket; a core
        // that never comes up fails here rather than in whatever the test asks first.
        let started = Instant::now();
        while !matches!(torro_ipc::client::call(&socket, &Request::Ping).await, Ok(Response::Pong { .. })) {
            assert!(started.elapsed() < SETTLE, "the core in {} did not start", dir.display());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        (socket, hook_key)
    }

    /// The open questions as the app key lists them.
    #[cfg(unix)]
    async fn open_questions(socket: &Path) -> Vec<ApprovalInfo> {
        match torro_ipc::client::call(socket, &Request::ListApprovals).await {
            Ok(Response::Approvals { approvals }) => approvals.into_iter().filter(|a| a.state.is_pending()).collect(),
            other => panic!("the app key lists the approvals: {other:?}"),
        }
    }

    /// Task E2.5 against a core: the hook asks, the person decides with the app key (as the
    /// inbox, the control centre, the notification and `ctl` do), and the hook hands the agent
    /// exactly the agent's decision JSON. Without a decision it hands over nothing.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_permission_request_is_answered_with_the_persons_decision() {
        for (agent, fixture_name) in
            [(AgentKind::ClaudeCode, "claude-code-permission-request"), (AgentKind::Codex, "codex-permission-request")]
        {
            for allow in [true, false] {
                let dir = tempfile::tempdir().expect("temp dir");
                let (socket, key) = core_on_socket(dir.path(), agent).await;
                let question = permission_question(agent, &fixture(fixture_name)).expect("a question");
                let hook = {
                    let (socket, key, question) = (socket.clone(), key.clone(), question.clone());
                    tokio::task::spawn_blocking(move || answer(agent, &question, Some(&socket), &key, ANSWER_WAIT))
                };
                // The person sees the question with the whole command, and decides.
                let started = Instant::now();
                let mut open = open_questions(&socket).await;
                while open.is_empty() && started.elapsed() < SETTLE {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    open = open_questions(&socket).await;
                }
                let [asked] = open.as_slice() else { panic!("one question: {open:?}") };
                assert!(crate::approvals::is_question(asked));
                assert!(asked.summary.contains(&question.details), "{}", asked.summary);
                assert!(asked.summary.starts_with(agent.label()), "{}", asked.summary);
                let decide = Request::DecideApproval { id: asked.id.clone(), allow };
                let decided = torro_ipc::client::call(&socket, &decide).await.expect("decided");
                assert!(matches!(decided, Response::Approval { .. }), "{decided:?}");

                let printed = hook.await.expect("ran").expect("answered");
                assert_eq!(printed, permission_answer(agent, allow), "{agent:?} allow={allow}");
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn without_a_decision_the_hook_hands_over_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (socket, key) = core_on_socket(dir.path(), AgentKind::ClaudeCode).await;
        let mut question =
            permission_question(AgentKind::ClaudeCode, &fixture("claude-code-permission-request")).expect("question");
        // The session's name goes with the question (task E2.6).
        question.title = Some("Login bug".into());
        let started = Instant::now();
        let waited = Duration::from_millis(1200);
        let printed = {
            let socket = socket.clone();
            tokio::task::spawn_blocking(move || answer(AgentKind::ClaudeCode, &question, Some(&socket), &key, waited))
                .await
                .expect("ran")
        };
        assert!(matches!(printed, Ok(None)), "{printed:?}");
        // It waited its own time out ...
        assert!(started.elapsed() >= waited, "{:?}", started.elapsed());
        // ... and not until the question expired (QUESTION_LIFETIME): the question is still open.
        // That tells the two apart without a bound on how slow a loaded machine may be.
        let open = open_questions(&socket).await;
        assert_eq!(open.len(), 1, "the question stays until it expires; nobody allowed it");
        assert!(open[0].summary.contains("Login bug"), "named: {}", open[0].summary);

        // Gemini CLI takes no answer: nothing is asked.
        let gemini = PermissionQuestion { tool: "Bash".into(), details: "ls".into(), project: None, title: None };
        let nobody = Key::new("torro_00000000_nope");
        assert!(matches!(answer(AgentKind::Gemini, &gemini, None, &nobody, Duration::ZERO), Ok(None)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_overlong_question_is_refused_by_the_core() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (socket, key) = core_on_socket(dir.path(), AgentKind::Codex).await;
        let long = PermissionQuestion {
            tool: "Bash".into(),
            details: "x".repeat(crate::approvals::QUESTION_DETAILS_MAX + 1),
            project: None,
            title: None,
        };
        let refused = {
            let socket = socket.clone();
            tokio::task::spawn_blocking(move || {
                answer(AgentKind::Codex, &long, Some(&socket), &key, Duration::from_secs(1))
            })
            .await
            .expect("ran")
        };
        assert!(matches!(&refused, Err(HookError::Refused { code, .. }) if code == "error"), "{refused:?}");
        assert!(open_questions(&socket).await.is_empty(), "nothing was put to the person");
    }
}
