//! The processes of the assistant's terminal (issue #754): a command run until its end ([`run`]) and the commands in
//! the background ([`Background`]). Each starts with an empty environment and then only the basics and the `PATH`
//! the core sets ([`environment`]), with nothing on its stdin, as the leader of a process group (Unix) or in a Job
//! Object without a console window (Windows), like the MCP servers (`crate::extensions::supervisor`): stopping it
//! stops everything it started, and so does the service ending. stdout and stderr are read together.
//!
//! What a command wrote is kept as it came; hiding secrets and cutting to the answer's limit is the caller's
//! (`super::Terminal`). Nothing here logs a command, a folder or output.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
#[cfg(windows)]
use process_wrap::tokio::{CreationFlags, JobObject};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::oneshot;

/// What runs: a program with its arguments, without a shell, or one line for the system's shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// A program by name or path, and its arguments, each one word.
    Program { program: String, args: Vec<String> },
    /// One line for `sh -c` (on Windows `cmd /C`).
    Shell(String),
}

/// The limits of the terminal; [`Limits::default`] are those of `torro_ipc::tools`, tests set shorter ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How long [`run`] waits before it stops the command.
    pub timeout: Duration,
    /// Characters of output in one answer.
    pub max_chars: usize,
    /// Bytes kept of a command in the background.
    pub ring_bytes: usize,
    /// Commands in the background of one conversation at a time.
    pub per_conversation: usize,
    /// Commands in the background in all at a time.
    pub total: usize,
    /// How long a stopped command may take to end by itself before it is killed.
    pub grace: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            timeout: Duration::from_secs(torro_ipc::tools::COMMAND_TIMEOUT_SECS),
            max_chars: torro_ipc::tools::MAX_OUTPUT_CHARS,
            ring_bytes: torro_ipc::tools::MAX_BACKGROUND_BYTES,
            per_conversation: torro_ipc::tools::MAX_BACKGROUND_PER_CONVERSATION,
            total: torro_ipc::tools::MAX_BACKGROUND,
            grace: Duration::from_secs(3),
        }
    }
}

/// The environment of a command's process: the [`crate::extensions::env::BASICS`] (on Windows also
/// [`crate::extensions::env::WINDOWS_BASICS`]) that `base` has, then `PATH` from `path_dirs`. Nothing else of the
/// service comes along (no `TORRO_*`, no key the service happens to have), and the model adds nothing (decision 7).
pub fn environment(base: &BTreeMap<String, String>, path_dirs: &[PathBuf]) -> Vec<(String, String)> {
    use crate::extensions::env::{BASICS, WINDOWS_BASICS};
    let same = |a: &str, b: &str| if cfg!(windows) { a.eq_ignore_ascii_case(b) } else { a == b };
    let mut out: Vec<(String, String)> = Vec::new();
    for wanted in BASICS.iter().chain(if cfg!(windows) { WINDOWS_BASICS } else { &[] }) {
        if let Some((name, value)) = base.iter().find(|(name, _)| same(name, wanted)) {
            out.push((name.clone(), value.clone()));
        }
    }
    if let Some(path) = crate::extensions::env::joined_path(path_dirs) {
        out.push(("PATH".into(), path));
    }
    out
}

/// The folders of the `PATH` of a command: the service's `PATH` and the usual folders, each once, absolute only.
pub fn path_dirs() -> Vec<PathBuf> {
    let roots = crate::extensions::prereq::Roots::system();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in roots.path.iter().cloned().chain(roots.usual()) {
        if dir.is_absolute() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// How a command ended ([`run`]): what it wrote, as text (bytes that are no UTF-8 replaced), cut after `cut` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub output: String,
    /// Its exit code; `None` when it was stopped or ended by a signal.
    pub exit_code: Option<i32>,
    /// It wrote more than was kept.
    pub cut: bool,
    /// It ran longer than [`Limits::timeout`] and was stopped.
    pub timed_out: bool,
}

/// The started process, killed as a whole when dropped unreaped (the service ends).
struct Tree {
    child: Box<dyn ChildWrapper>,
    reaped: bool,
}

impl Tree {
    /// Kills what is left of the group or job and reaps it.
    async fn end(&mut self) {
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

/// Starts `run` in `folder` with exactly `env`, nothing on its stdin, stdout and stderr piped.
fn spawn(
    run: &Run,
    folder: &Path,
    env: &[(String, String)],
) -> std::io::Result<(Tree, impl AsyncRead + Unpin + Send + 'static, impl AsyncRead + Unpin + Send + 'static)> {
    let mut command = match run {
        Run::Program { program, args } => {
            let mut command = tokio::process::Command::new(program);
            command.args(args);
            command
        }
        #[cfg(windows)]
        Run::Shell(line) => {
            let mut command = tokio::process::Command::new("cmd");
            command.arg("/C").arg(line);
            command
        }
        #[cfg(not(windows))]
        Run::Shell(line) => {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.arg("-c").arg(line);
            command
        }
    };
    command
        .env_clear()
        .envs(env.iter().cloned())
        .current_dir(folder)
        .stdin(Stdio::null())
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
    let (Some(stdout), Some(stderr)) = (tree.child.stdout().take(), tree.child.stderr().take()) else {
        return Err(std::io::Error::other("the program has no pipes"));
    };
    Ok((tree, stdout, stderr))
}

/// Where the output of a command goes.
trait Sink: Send + 'static {
    fn push(&mut self, bytes: &[u8]);
}

/// The start of the output of a command run until its end, up to `cap` bytes.
struct Head {
    bytes: Vec<u8>,
    cap: usize,
    cut: bool,
}

impl Sink for Head {
    fn push(&mut self, bytes: &[u8]) {
        let room = self.cap.saturating_sub(self.bytes.len());
        self.bytes.extend_from_slice(&bytes[..bytes.len().min(room)]);
        self.cut |= bytes.len() > room;
    }
}

/// Reads `from` into `sink` until it closes.
async fn read_into<S: Sink>(mut from: impl AsyncRead + Unpin, sink: Arc<Mutex<S>>) {
    let mut chunk = [0u8; 8192];
    loop {
        match from.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => sink.lock().unwrap_or_else(PoisonError::into_inner).push(&chunk[..read]),
        }
    }
}

/// Runs `run` in `folder` with `env` until it ends, at most [`Limits::timeout`]; then stops it and everything it
/// started. Keeps the first `cap` bytes of what it wrote. `Err` when it cannot start (no such program, a folder gone).
pub async fn run(
    run: &Run,
    folder: &Path,
    env: &[(String, String)],
    limits: &Limits,
    cap: usize,
) -> std::io::Result<Finished> {
    let (mut tree, stdout, stderr) = spawn(run, folder, env)?;
    let head = Arc::new(Mutex::new(Head { bytes: Vec::new(), cap, cut: false }));
    let readers = [tokio::spawn(read_into(stdout, head.clone())), tokio::spawn(read_into(stderr, head.clone()))];
    // The program itself, not the group: one that left a child behind would never seem to end.
    let ended = tokio::time::timeout(limits.timeout, tree.child.inner_mut().wait()).await;
    let (exit_code, timed_out) = match ended {
        Ok(Ok(status)) => (status.code(), false),
        Ok(Err(_)) => (None, false),
        Err(_) => (None, true),
    };
    // What it left running ends with it, so the pipes close.
    tree.end().await;
    for reader in readers {
        let _ = tokio::time::timeout(Duration::from_secs(2), reader).await;
    }
    let head = std::mem::replace(
        &mut *head.lock().unwrap_or_else(PoisonError::into_inner),
        Head { bytes: Vec::new(), cap: 0, cut: false },
    );
    Ok(Finished { output: String::from_utf8_lossy(&head.bytes).into_owned(), exit_code, cut: head.cut, timed_out })
}

/// The last bytes of a command in the background, counted from its start, so a reader can go on where it stopped.
#[derive(Debug, Default)]
struct Ring {
    data: VecDeque<u8>,
    /// The position of `data[0]` in all the command wrote.
    start: u64,
    cap: usize,
}

impl Sink for Ring {
    fn push(&mut self, bytes: &[u8]) {
        self.data.extend(bytes);
        let over = self.data.len().saturating_sub(self.cap);
        if over > 0 {
            self.data.drain(..over);
            self.start += over as u64;
        }
    }
}

impl Ring {
    /// From `from` on (the start of what is kept when left out or no longer kept), at most `max_chars` characters as
    /// text, where to go on, and whether something was left out (before or after). Always whole lines where it can
    /// (review of #754): a secret on a line is never split between two reads, where the patterns that hide it would
    /// see neither half whole. So a read that may start in the middle of a line (`aligned` false: the start of what
    /// is kept after older output went, a position of the core's own) starts after its first line break, and one that
    /// ends before the end of a line – because more follows, or the command still `running` may write the rest of
    /// it – ends after its last one. A line longer than `max_chars` is cut where it must be.
    fn read(&self, from: Option<u64>, max_chars: usize, running: bool, aligned: bool) -> (String, u64, bool) {
        let end = self.start + self.data.len() as u64;
        let wanted = from.unwrap_or(self.start);
        let mut from = wanted.clamp(self.start, end);
        let gone = wanted < self.start;
        let skip = usize::try_from(from - self.start).unwrap_or(usize::MAX);
        let mut bytes: Vec<u8> = self.data.iter().skip(skip).copied().collect();
        // Older output went (or the position is the core's own): its first line may be the rest of one.
        let mid_line = (!aligned && from > 0) || gone || (from == self.start && self.start > 0);
        if mid_line && let Some(at) = bytes.iter().position(|byte| *byte == b'\n') {
            from += at as u64 + 1;
            bytes.drain(..=at);
        }
        // The text, how many bytes of the ring it took, and both as they were after the last line break.
        let (mut text, mut used, mut chars) = (String::new(), 0usize, 0usize);
        let mut line_end: Option<(usize, usize)> = None;
        'chunks: for chunk in bytes.utf8_chunks() {
            for char in chunk.valid().chars() {
                if chars == max_chars {
                    break 'chunks;
                }
                text.push(char);
                used += char.len_utf8();
                chars += 1;
                if char == '\n' {
                    line_end = Some((text.len(), used));
                }
            }
            if !chunk.invalid().is_empty() {
                if chars == max_chars {
                    break;
                }
                text.push(char::REPLACEMENT_CHARACTER);
                used += chunk.invalid().len();
                chars += 1;
            }
        }
        let open_line = !text.ends_with('\n') && (from + (used as u64) < end || running);
        if open_line {
            match line_end {
                Some((text_len, raw)) => {
                    text.truncate(text_len);
                    used = raw;
                }
                // Only the start of a line that is still being written: wait for its end.
                None if running && from + (used as u64) == end => {
                    text.clear();
                    used = 0;
                }
                None => {}
            }
        }
        let next = from + used as u64;
        (text, next, gone || next < end)
    }
}

/// Where a command in the background stands.
#[derive(Debug, Default)]
struct State {
    ring: Ring,
    running: bool,
    exit_code: Option<i32>,
}

impl Sink for State {
    fn push(&mut self, bytes: &[u8]) {
        self.ring.push(bytes);
    }
}

/// Whose a command in the background is: the client and its conversation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Owner {
    pub client: String,
    pub conversation: String,
}

/// One command in the background.
struct Entry {
    owner: Owner,
    /// It runs a program that reads from outside the computer (`super::patterns::reads_outside`).
    outside: bool,
    state: Arc<Mutex<State>>,
    /// Tells the watcher to stop it, after the grace sent.
    stop: Option<oneshot::Sender<Duration>>,
    done: Option<tokio::task::JoinHandle<()>>,
}

/// What [`Background::read`] and [`Background::stop`] answer: as text, not hidden or cut yet but to the characters
/// of [`Limits::max_chars`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The command reads from outside the computer, as it was started.
    pub outside: bool,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub output: String,
    pub next: u64,
    pub truncated: bool,
}

/// Why a command could not start in the background.
#[derive(Debug)]
pub enum StartError {
    /// Too many run already, in its conversation or in all.
    Busy,
    /// It could not start.
    Io(std::io::Error),
}

/// The commands in the background, by their id (8 hex digits). Finished ones stay readable until stopped or their
/// conversation ends; at most [`KEPT`] are kept in all, the oldest finished going first.
pub struct Background {
    entries: Mutex<HashMap<String, Entry>>,
    order: Mutex<Vec<String>>,
    limits: Limits,
}

/// At most this many commands, running and finished, are kept in the background in all.
const KEPT: usize = 32;

impl Background {
    pub fn new(limits: Limits) -> Background {
        Background { entries: Mutex::default(), order: Mutex::default(), limits }
    }

    /// Starts `run` for `owner`; `Busy` beyond [`Limits::per_conversation`] or [`Limits::total`] running commands.
    /// `outside`: it reads from outside the computer, what its answers say.
    pub fn start(
        &self,
        owner: Owner,
        (run, outside): (&Run, bool),
        folder: &Path,
        env: &[(String, String)],
    ) -> Result<String, StartError> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let running = |entry: &&Entry| entry.state.lock().unwrap_or_else(PoisonError::into_inner).running;
        let all = entries.values().filter(running).count();
        let own = entries.values().filter(running).filter(|entry| entry.owner == owner).count();
        if all >= self.limits.total || own >= self.limits.per_conversation {
            return Err(StartError::Busy);
        }
        let id = random_id().map_err(StartError::Io)?;
        let (mut tree, stdout, stderr) = spawn(run, folder, env).map_err(StartError::Io)?;
        let state = Arc::new(Mutex::new(State {
            ring: Ring { cap: self.limits.ring_bytes, ..Ring::default() },
            running: true,
            exit_code: None,
        }));
        let readers = [tokio::spawn(read_into(stdout, state.clone())), tokio::spawn(read_into(stderr, state.clone()))];
        let (stop, stopped) = oneshot::channel::<Duration>();
        let watched = state.clone();
        let done = tokio::spawn(async move {
            let grace = tokio::select! {
                status = tree.child.inner_mut().wait() => {
                    let mut state = watched.lock().unwrap_or_else(PoisonError::into_inner);
                    state.exit_code = status.ok().and_then(|status| status.code());
                    None
                }
                grace = stopped => grace.ok(),
            };
            if let Some(grace) = grace {
                polite_stop(tree.child.as_ref());
                let _ = tokio::time::timeout(grace, tree.child.inner_mut().wait()).await;
            }
            tree.end().await;
            for reader in readers {
                let _ = tokio::time::timeout(Duration::from_secs(2), reader).await;
            }
            watched.lock().unwrap_or_else(PoisonError::into_inner).running = false;
        });
        entries.insert(id.clone(), Entry { owner, outside, state, stop: Some(stop), done: Some(done) });
        let mut order = self.order.lock().unwrap_or_else(PoisonError::into_inner);
        order.push(id.clone());
        // Beyond what is kept the oldest finished go; a running one stays.
        while order.len() > KEPT {
            let Some(at) = order.iter().position(|old| entries.get(old).is_none_or(|entry| !running(&entry))) else {
                break;
            };
            let old = order.remove(at);
            entries.remove(&old);
        }
        Ok(id)
    }

    /// What the command `id` of `owner` wrote from `from` on; `None` for one of another owner or none.
    pub fn read(&self, owner: &Owner, id: &str, from: Option<u64>) -> Option<Snapshot> {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries.get(id).filter(|entry| entry.owner == *owner)?;
        let state = entry.state.lock().unwrap_or_else(PoisonError::into_inner);
        let (output, next, truncated) = state.ring.read(from, self.limits.max_chars, state.running, true);
        Some(Snapshot {
            outside: entry.outside,
            running: state.running,
            exit_code: state.exit_code,
            output,
            next,
            truncated,
        })
    }

    /// Stops the command `id` of `owner` (politely, after [`Limits::grace`] for good), waits for it and answers the
    /// last of what it wrote; it is forgotten then. `None` as for [`Background::read`].
    pub async fn stop(&self, owner: &Owner, id: &str) -> Option<Snapshot> {
        let entry = {
            let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
            if entries.get(id).is_none_or(|entry| entry.owner != *owner) {
                return None;
            }
            self.order.lock().unwrap_or_else(PoisonError::into_inner).retain(|kept| kept != id);
            entries.remove(id)?
        };
        let (state, outside) = (entry.state.clone(), entry.outside);
        self.end(entry).await;
        let state = state.lock().unwrap_or_else(PoisonError::into_inner);
        let end = state.ring.start + state.ring.data.len() as u64;
        // The last of it: about as many bytes as an answer has characters, from the start of a line.
        let from = end.saturating_sub(self.limits.max_chars as u64).max(state.ring.start);
        let (output, next, truncated) = state.ring.read(Some(from), self.limits.max_chars, false, false);
        let truncated = truncated || from > 0;
        Some(Snapshot { outside, running: false, exit_code: state.exit_code, output, next, truncated })
    }

    /// Stops every command of `client` in `conversation` (it was deleted or its answer stopped).
    pub async fn end_conversation(&self, client: &str, conversation: &str) {
        let ended = self.take(|owner| owner.client == client && owner.conversation == conversation);
        // All at once: each may take its grace (review of #754).
        futures_util::future::join_all(ended.into_iter().map(|entry| self.end(entry))).await;
    }

    /// Stops every command (the service ends).
    pub async fn stop_all(&self) {
        futures_util::future::join_all(self.take(|_| true).into_iter().map(|entry| self.end(entry))).await;
    }

    /// Takes the entries whose owner `which` names out of the register.
    fn take(&self, which: impl Fn(&Owner) -> bool) -> Vec<Entry> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let ids: Vec<String> =
            entries.iter().filter(|(_, entry)| which(&entry.owner)).map(|(id, _)| id.clone()).collect();
        self.order.lock().unwrap_or_else(PoisonError::into_inner).retain(|kept| !ids.contains(kept));
        ids.iter().filter_map(|id| entries.remove(id)).collect()
    }

    /// Ends one taken entry and waits for its watcher.
    async fn end(&self, mut entry: Entry) {
        if let Some(stop) = entry.stop.take() {
            let _ = stop.send(self.limits.grace);
        }
        if let Some(done) = entry.done.take() {
            let _ = done.await;
        }
    }

    /// How many commands are kept, for tests.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

/// Asks the group of `child` to end (`SIGTERM` to the whole group); on Windows there is no polite way for a Job
/// Object, so the grace passes and it is killed.
fn polite_stop(child: &dyn ChildWrapper) {
    #[cfg(unix)]
    let _ = child.signal(15);
    #[cfg(not(unix))]
    let _ = child;
}

/// 8 random hex digits.
fn random_id() -> std::io::Result<String> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).map_err(|err| std::io::Error::other(err.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_last_bytes_and_reads_on() {
        let mut ring = Ring { cap: 8, ..Ring::default() };
        ring.push(b"hello ");
        assert_eq!(ring.read(None, 100, false, true), ("hello ".into(), 6, false));
        ring.push(b"world!");
        assert_eq!((ring.start, ring.data.len()), (4, 8));
        assert_eq!(ring.read(Some(0), 100, false, true), ("o world!".into(), 12, true), "what is gone is said");
        assert_eq!(ring.read(Some(6), 3, false, true), ("wor".into(), 9, true), "more follows, one long line");
        assert_eq!(ring.read(Some(12), 3, false, true), (String::new(), 12, false));
        let mut ring = Ring { cap: 100, ..Ring::default() };
        ring.push(b"one\ntoken=abcdefgh\n");
        assert_eq!(
            ring.read(None, 10, false, true),
            ("one\n".into(), 4, true),
            "ends at a line break when more follows"
        );
        ring.push(b"export API_TOKEN=ab");
        assert_eq!(ring.read(Some(4), 100, true, true), ("token=abcdefgh\n".into(), 19, true), "the open line waits");
        assert_eq!(ring.read(Some(19), 100, true, true), (String::new(), 19, true), "only an open line: nothing yet");
        let mut ring = Ring { cap: 100, ..Ring::default() };
        ring.push(b"\xe9\xe9\xe9\nPASSWORD=hunter2\nmore");
        let (text, next, _) = ring.read(None, 100, true, true);
        assert_eq!((text.chars().count(), next), (21, 21), "bytes of the ring, not of the text");
        let mut ring = Ring { cap: 12, ..Ring::default() };
        ring.push(b"KEY=secret1\nok\n");
        assert_eq!(ring.read(None, 100, false, true).0, "ok\n", "the rest of a line that went is left out");
        let mut ring = Ring { cap: 100, ..Ring::default() };
        ring.push("äö".as_bytes());
        assert_eq!(ring.read(None, 1, false, true), ("ä".into(), 2, true), "characters, not bytes");
    }

    #[test]
    fn the_environment_holds_only_the_basics_and_the_path() {
        let base = BTreeMap::from([
            ("HOME".to_string(), "/home/anna".to_string()),
            ("TORRO_TOKEN".to_string(), "secret".to_string()),
            ("OPENAI_API_KEY".to_string(), "sk-x".to_string()),
        ]);
        let env = environment(&base, &[PathBuf::from("/usr/bin")]);
        assert_eq!(env, [("HOME".to_string(), "/home/anna".to_string()), ("PATH".to_string(), "/usr/bin".to_string())]);
    }

    #[cfg(unix)]
    fn sh(line: &str) -> Run {
        Run::Shell(line.into())
    }

    #[cfg(unix)]
    fn env() -> Vec<(String, String)> {
        vec![("PATH".into(), "/usr/bin:/bin".into())]
    }

    /// Erledigt points 1 and 6 of issue #754: output and exit code; a command over the time limit is stopped, and
    /// output over the cap is cut.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_runs_to_its_end_or_is_stopped() {
        let dir = tempfile::tempdir().expect("dir");
        let limits = Limits { timeout: Duration::from_millis(300), ..Limits::default() };
        let done = run(&sh("echo out; echo err >&2; exit 3"), dir.path(), &env(), &limits, 1000).await.expect("runs");
        assert_eq!((done.exit_code, done.timed_out, done.cut), (Some(3), false, false));
        assert!(done.output.contains("out\n") && done.output.contains("err\n"), "{}", done.output);
        let program = Run::Program { program: "echo".into(), args: vec!["a b".into(), "c".into()] };
        let done = run(&program, dir.path(), &env(), &limits, 1000).await.expect("runs");
        assert_eq!((done.output.as_str(), done.exit_code), ("a b c\n", Some(0)));
        let started = std::time::Instant::now();
        let slow = run(&sh("sleep 5 & sleep 5"), dir.path(), &env(), &limits, 1000).await.expect("runs");
        assert!(slow.timed_out && slow.exit_code.is_none() && started.elapsed() < Duration::from_secs(4));
        let long = run(&sh("yes | head -c 5000"), dir.path(), &env(), &limits, 100).await.expect("runs");
        assert!(long.cut && long.output.len() == 100);
        assert!(
            run(&Run::Program { program: "no-such-program-x".into(), args: vec![] }, dir.path(), &env(), &limits, 10)
                .await
                .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn commands_in_the_background_are_their_conversations() {
        let dir = tempfile::tempdir().expect("dir");
        let limits = Limits { per_conversation: 1, total: 2, grace: Duration::from_millis(200), ..Limits::default() };
        let background = Background::new(limits);
        let owner = |conversation: &str| Owner { client: "a1".into(), conversation: conversation.into() };
        let id =
            background.start(owner("c1"), (&sh("echo started; sleep 30"), false), dir.path(), &env()).expect("starts");
        assert!(matches!(
            background.start(owner("c1"), (&sh("sleep 30"), false), dir.path(), &env()),
            Err(StartError::Busy)
        ));
        let other =
            background.start(owner("c2"), (&sh("sleep 30"), false), dir.path(), &env()).expect("another conversation");
        assert!(matches!(
            background.start(owner("c3"), (&sh("sleep 30"), false), dir.path(), &env()),
            Err(StartError::Busy)
        ));
        tokio::time::sleep(Duration::from_millis(200)).await;
        let read = background.read(&owner("c1"), &id, None).expect("its own");
        assert!(read.running && read.output == "started\n" && read.next == 8, "{read:?}");
        assert!(background.read(&owner("c2"), &id, None).is_none(), "another conversation's is none");
        let stopped = background.stop(&owner("c1"), &id).await.expect("stops");
        assert!(!stopped.running && stopped.output.contains("started"));
        assert!(background.read(&owner("c1"), &id, None).is_none(), "forgotten");
        background.end_conversation("a1", "c2").await;
        assert!(background.read(&owner("c2"), &other, None).is_none());
        let quick = background.start(owner("c4"), (&sh("echo hi"), true), dir.path(), &env()).expect("starts");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let ended = background.read(&owner("c4"), &quick, None).expect("still readable");
        assert_eq!(
            (ended.running, ended.exit_code, ended.output.as_str(), ended.outside),
            (false, Some(0), "hi\n", true)
        );
        background.stop_all().await;
        assert_eq!(background.len(), 0);
    }
}
