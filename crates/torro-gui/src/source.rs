//! The data source an app runs on.
//!
//! Without `--demo` an app starts the core when it is not running ([`ensure_core`]) and waits for
//! it once before its first load ([`Source::ready`], [`wait_for_core`], issue #206): a core started
//! just now does not listen yet, and asking at once would show an error until the app's next timer.
//!
//! Every program of the suite that starts the core leaves a marker ([`CORE_START_MARK`]) in the
//! core's state directory first, and none starts it again within [`CORE_START_GAP`] of that (issue
//! #279): a core that fails right after its start must not be started over and over by the windows
//! it opened.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use torro_core::timing;
use torro_ipc::pim::PimClient;
use torro_ipc::{Request, Response};
use torro_pim::demo::DemoSource;
use torro_pim::source::{FilterDefinition, FilterPreview, GitHubRepo, GitHubRepoDetails};
use torro_pim::{Collection, Event, PimAccount, PimSource, ProviderError, Todo};

/// The core over the socket, or demo data in memory (`--demo`). Cheap to clone, so every
/// Iced task can own one.
#[derive(Debug, Clone)]
pub enum Source {
    Core(PimClient),
    Demo(Arc<DemoSource>),
}

impl Source {
    /// `--demo` among the arguments picks demo data, anything else the core.
    /// Without `--demo`, starts the core when it is not running, so opening an app is enough.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Source {
        if args.into_iter().any(|arg| arg == "--demo") {
            Source::Demo(Arc::new(DemoSource::new(chrono::Local::now().date_naive())))
        } else {
            let client = PimClient::default();
            ensure_core(&client.socket);
            Source::Core(client)
        }
    }

    pub fn is_demo(&self) -> bool {
        matches!(self, Source::Demo(_))
    }

    /// Waits until the core answers, at most [`CORE_START_WAIT`]; `true` once it does, at once for
    /// demo data. An app awaits it before its first load only (issue #206): a core that runs
    /// answers within a millisecond, one that is starting within its start, and one that does not
    /// answer in time leaves the app where it was before, with its error and its timer.
    pub async fn ready(&self) -> bool {
        match self {
            Source::Core(client) => wait_for_core(&client.socket, CORE_START_WAIT).await,
            Source::Demo(_) => true,
        }
    }

    /// Opens or raises another app of the suite by its binary name (`torrotasks`), through the
    /// core's `Request::OpenApp`, so the core can focus a window that is already open instead of
    /// starting a second one. Answer of the app switcher and of "Open Tasks ↗".
    ///
    /// Demo data has no core: nothing is opened and the result is `Ok(())`. An `Err` carries the
    /// core's message (unknown app, cannot start, core not running).
    pub async fn open_app(&self, binary: &str) -> Result<(), String> {
        let Source::Core(client) = self else { return Ok(()) };
        let request = torro_ipc::Request::OpenApp { app: binary.to_owned(), target: None };
        match torro_ipc::client::call(&client.socket, &request).await {
            Ok(torro_ipc::Response::Ok) => Ok(()),
            Ok(torro_ipc::Response::Error { message, .. }) => Err(message),
            Ok(other) => Err(format!("unexpected answer: {other:?}")),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// How long an app waits at its start for the core to answer ([`Source::ready`]): on Unix, where
/// [`ensure_core`] has just started it.
#[cfg(unix)]
pub const CORE_START_WAIT: Duration = Duration::from_secs(10);

/// Elsewhere no app starts the core, so waiting would only hold back the error: one ask.
#[cfg(not(unix))]
pub const CORE_START_WAIT: Duration = Duration::ZERO;

/// How often [`wait_for_core`] asks.
const CORE_POLL: Duration = Duration::from_millis(50);

/// How long one ask of [`wait_for_core`] may take: a program that holds the socket but never
/// answers must not stretch the wait beyond its limit.
const PING_WAIT: Duration = Duration::from_millis(200);

/// Asks the core on `socket` for `system.ping` every [`CORE_POLL`] until it answers or `limit` is
/// up; `true` when it answered. With `TORRO_TIMING` the answer is marked ("core ready").
pub async fn wait_for_core(socket: &Path, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        let ping = tokio::time::timeout(PING_WAIT, torro_ipc::client::call(socket, &Request::Ping)).await;
        if matches!(ping, Ok(Ok(Response::Pong { .. }))) {
            timing::once("core ready");
            return true;
        }
        if tokio::time::Instant::now() + CORE_POLL >= deadline {
            return false;
        }
        tokio::time::sleep(CORE_POLL).await;
    }
}

/// Starts `torroworkplace daemon` in the background when nothing listens on `socket`. The
/// app then retries until the core answers. Unix only for now.
///
/// Not when a program of the suite tried to start it within the last [`CORE_START_GAP`]
/// ([`claim_core_start`], issue #279): that start is still under way, or the core failed at once,
/// and starting it again would only repeat that.
pub fn ensure_core(socket: &std::path::Path) {
    #[cfg(unix)]
    {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return;
        }
        if !claim_core_start(socket, Utc::now()) {
            return;
        }
        let program = sibling(torro_core::AppId::WORKPLACE.binary);
        let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).map(std::path::PathBuf::from);
        match daemon_command(&program, home).spawn() {
            // Reap the daemon if it ever exits while the app still runs.
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(err) => eprintln!("cannot start {}: {err}", program.display()),
        }
    }
    #[cfg(not(unix))]
    let _ = socket;
}

/// The command [`ensure_core`] starts the core with: `program daemon`, detached from the
/// caller's terminal and process group, so it outlives the app or MCP server that started it.
///
/// It runs in `home` (else `/`), not in the caller's directory, which it would otherwise keep
/// busy for as long as it runs. `TORRO_TOKEN` is not passed on: it is the key of the agent whose
/// MCP server started the core, and the core has no use for it.
#[cfg(unix)]
fn daemon_command(program: &std::path::Path, home: Option<std::path::PathBuf>) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut command = Command::new(program);
    command
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("TORRO_TOKEN")
        .current_dir(home.unwrap_or_else(|| std::path::PathBuf::from("/")))
        .process_group(0);
    command
}

/// The marker in the core's state directory ([`core_state_file`]) that tells when a program of the
/// suite last tried to start the core ([`claim_core_start`], issue #279). It holds the time only.
pub const CORE_START_MARK: &str = "core-start";

/// How long after one try to start the core no program of the suite tries again
/// ([`start_due`]): about as long as a core needs to answer once started.
pub const CORE_START_GAP: Duration = Duration::from_secs(10);

/// The file `name` in the state directory of the core listening at `socket`, where its app key lies
/// too ([`torro_ipc::app_key_path`]): next to `core.sock`, on Windows in the state directory of
/// [`torro_core::paths::AppPaths`]. `None` when there is no such directory (a socket path without a
/// directory, no home directory), so no marker ever lands in the current directory.
pub fn core_state_file(socket: &Path, name: &str) -> Option<PathBuf> {
    let key = torro_ipc::app_key_path(socket);
    key.parent().filter(|dir| !dir.as_os_str().is_empty())?;
    Some(key.with_file_name(name))
}

/// Writes the marker `path`: the time `at` (RFC 3339) and nothing else, readable by its owner only
/// (0600 on Unix). Creates the directory if needed and replaces an older marker.
pub fn write_mark(path: &Path, at: DateTime<Utc>) -> std::io::Result<()> {
    use std::io::Write;

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        let mut file = options.open(path)?;
        // `mode` applies to a new file only; one left by an older version gets it here.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(at.to_rfc3339().as_bytes())
    }
    #[cfg(not(unix))]
    {
        options.open(path)?.write_all(at.to_rfc3339().as_bytes())
    }
}

/// The time in the marker `path` ([`write_mark`]); `None` when there is none or it holds no time.
pub fn read_mark(path: &Path) -> Option<DateTime<Utc>> {
    let text = std::fs::read_to_string(path).ok()?;
    DateTime::parse_from_rfc3339(text.trim()).ok().map(|at| at.with_timezone(&Utc))
}

/// Whether a start of the core at `now` is due after the last try at `last`: always without one,
/// else only [`CORE_START_GAP`] or more apart. Apart in either direction: a try stamped a moment
/// after `now` by another program running at the same time counts as recent too, and a clock set
/// back blocks no longer than the gap.
pub fn start_due(last: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let gap = chrono::TimeDelta::from_std(CORE_START_GAP).unwrap_or(chrono::TimeDelta::MAX);
    last.is_none_or(|at| (now - at).abs() >= gap)
}

/// Claims a start of the core at `socket` for `now` (issue #279): `false` when a program of the suite
/// tried within [`CORE_START_GAP`] ([`CORE_START_MARK`], [`start_due`]), else records `now` there and
/// answers `true`. Without a state directory, or when the marker cannot be written, the start goes
/// ahead as it did before the marker: a missing marker must never keep the core from starting.
pub fn claim_core_start(socket: &Path, now: DateTime<Utc>) -> bool {
    let Some(path) = core_state_file(socket, CORE_START_MARK) else { return true };
    if !start_due(read_mark(&path), now) {
        return false;
    }
    if let Err(err) = write_mark(&path, now) {
        eprintln!("cannot write {}: {err}", path.display());
    }
    true
}

/// `name` next to the running binary (an install puts the suite in one directory), else
/// `name` from `PATH`.
pub fn sibling(name: &str) -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|path| path.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(name))
}

macro_rules! delegate {
    ($self:ident, $source:ident => $call:expr) => {
        match $self {
            Source::Core($source) => $call.await,
            Source::Demo($source) => $call.await,
        }
    };
}

impl PimSource for Source {
    async fn accounts(&self) -> Result<Vec<PimAccount>, ProviderError> {
        delegate!(self, s => s.accounts())
    }

    async fn connect_nextcloud(&self, server: String) -> Result<String, ProviderError> {
        delegate!(self, s => s.connect_nextcloud(server))
    }

    async fn sync(&self) -> Result<(), ProviderError> {
        delegate!(self, s => s.sync())
    }

    async fn collections(&self) -> Result<Vec<Collection>, ProviderError> {
        delegate!(self, s => s.collections())
    }

    async fn events(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Event>, ProviderError> {
        delegate!(self, s => s.events(start, end))
    }

    async fn todos(&self) -> Result<Vec<Todo>, ProviderError> {
        delegate!(self, s => s.todos())
    }

    async fn save_event(&self, event: Event) -> Result<Event, ProviderError> {
        delegate!(self, s => s.save_event(event))
    }

    async fn delete_event(&self, event: Event) -> Result<(), ProviderError> {
        delegate!(self, s => s.delete_event(event))
    }

    async fn save_todo(&self, todo: Todo) -> Result<Todo, ProviderError> {
        delegate!(self, s => s.save_todo(todo))
    }

    async fn delete_todo(&self, todo: Todo) -> Result<(), ProviderError> {
        delegate!(self, s => s.delete_todo(todo))
    }

    async fn move_todo(&self, todo: Todo, before: Option<Todo>) -> Result<Vec<Todo>, ProviderError> {
        delegate!(self, s => s.move_todo(todo, before))
    }

    async fn unpin_issue(&self, collection: String, item: String) -> Result<(), ProviderError> {
        delegate!(self, s => s.unpin_issue(collection, item))
    }

    async fn add_link(&self, collection: String, url: String) -> Result<Todo, ProviderError> {
        delegate!(self, s => s.add_link(collection, url))
    }

    async fn create_task_list(
        &self,
        account: String,
        name: String,
        color: Option<String>,
    ) -> Result<Collection, ProviderError> {
        delegate!(self, s => s.create_task_list(account, name, color))
    }

    async fn create_filter_list(
        &self,
        account: String,
        definition: FilterDefinition,
    ) -> Result<Collection, ProviderError> {
        delegate!(self, s => s.create_filter_list(account, definition))
    }

    async fn update_filter_list(
        &self,
        collection: String,
        revision: u64,
        definition: FilterDefinition,
    ) -> Result<Collection, ProviderError> {
        delegate!(self, s => s.update_filter_list(collection, revision, definition))
    }

    async fn delete_filter_list(&self, collection: String) -> Result<(), ProviderError> {
        delegate!(self, s => s.delete_filter_list(collection))
    }

    async fn preview_filter(&self, account: String, queries: Vec<String>) -> Result<FilterPreview, ProviderError> {
        delegate!(self, s => s.preview_filter(account, queries))
    }

    async fn list_github_repos(&self, account: String) -> Result<Vec<GitHubRepo>, ProviderError> {
        delegate!(self, s => s.list_github_repos(account))
    }

    async fn describe_github_repo(&self, account: String, repo: String) -> Result<GitHubRepoDetails, ProviderError> {
        delegate!(self, s => s.describe_github_repo(account, repo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_data_opens_no_app() {
        let demo = Source::from_args(["--demo".to_string()]);
        assert_eq!(iced::futures::executor::block_on(demo.open_app("torrotasks")), Ok(()));
    }

    #[test]
    fn demo_data_is_ready_at_once() {
        let demo = Source::from_args(["--demo".to_string()]);
        assert!(iced::futures::executor::block_on(demo.ready()));
    }

    /// A socket path of its own for each test, short enough for a Unix socket.
    #[cfg(unix)]
    fn socket(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("torro-gui-{}-{name}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// Issue #206: a core that starts listening a little later is waited for.
    #[cfg(unix)]
    #[tokio::test]
    async fn waits_until_a_starting_core_answers() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let path = socket("late");
        let listening = path.clone();
        let core = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let listener = tokio::net::UnixListener::bind(&listening).expect("binds");
            let (stream, _) = listener.accept().await.expect("a client connects");
            let (read, mut write) = stream.into_split();
            let mut line = String::new();
            BufReader::new(read).read_line(&mut line).await.expect("reads the request");
            let pong = Response::Pong { version: "0.1.0".into(), api: torro_ipc::API_VERSION };
            let mut answer = serde_json::to_vec(&pong).expect("serializes");
            answer.push(b'\n');
            write.write_all(&answer).await.expect("answers");
        });
        assert!(wait_for_core(&path, Duration::from_secs(5)).await);
        core.await.expect("the core task ends");
        let _ = std::fs::remove_file(&path);
    }

    /// Issue #206: without a core the wait ends after its limit.
    #[cfg(unix)]
    #[tokio::test]
    async fn without_a_core_the_wait_ends_after_its_limit() {
        let path = socket("none");
        let started = std::time::Instant::now();
        assert!(!wait_for_core(&path, Duration::from_millis(200)).await);
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(150) && waited < Duration::from_secs(2), "{waited:?}");
    }

    /// A directory under the system temp dir, removed again on drop (torro-gui has no `tempfile`).
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("torro-gui-source-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000 + seconds, 0).expect("a time")
    }

    /// Issue #279: one try per [`CORE_START_GAP`], whoever tries.
    #[test]
    fn a_start_is_due_only_ten_seconds_after_the_last_try() {
        assert!(start_due(None, at(0)), "never tried");
        assert!(!start_due(Some(at(0)), at(0)));
        assert!(!start_due(Some(at(0)), at(9)));
        assert!(start_due(Some(at(0)), at(10)));
        assert!(start_due(Some(at(0)), at(3600)));
        assert!(!start_due(Some(at(5)), at(0)), "stamped a moment later by another program");
        assert!(start_due(Some(at(60)), at(0)), "a clock set back blocks no longer than the gap");
    }

    /// Issue #279: the marker lies next to the socket, and a second try within ten seconds starts
    /// nothing, whichever program of the suite makes it.
    #[cfg(unix)]
    #[test]
    fn a_second_start_within_ten_seconds_is_left_out() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new("claim");
        let socket = dir.0.join("state").join("core.sock");
        let mark = dir.0.join("state").join(CORE_START_MARK);
        assert_eq!(core_state_file(&socket, CORE_START_MARK), Some(mark.clone()));

        assert!(claim_core_start(&socket, at(0)), "the first start");
        assert_eq!(read_mark(&mark), Some(at(0)));
        let mode = std::fs::metadata(&mark).expect("written").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        assert!(!claim_core_start(&socket, at(4)), "a second program four seconds later");
        assert_eq!(read_mark(&mark), Some(at(0)), "a left-out start leaves the time as it was");
        assert!(claim_core_start(&socket, at(10)), "ten seconds on, it may try again");
        assert_eq!(read_mark(&mark), Some(at(10)));
    }

    #[test]
    fn a_marker_without_a_time_or_a_directory_blocks_nothing() {
        let dir = TempDir::new("broken");
        let mark = dir.0.join(CORE_START_MARK);
        assert_eq!(read_mark(&mark), None, "no marker");
        std::fs::write(&mark, "soon").expect("write");
        assert_eq!(read_mark(&mark), None, "no time in it");
        assert!(start_due(read_mark(&mark), at(0)));
        assert_eq!(core_state_file(Path::new("core.sock"), CORE_START_MARK), None, "never the current directory");
    }

    #[cfg(unix)]
    #[test]
    fn the_core_starts_in_home_without_the_agent_key() {
        use std::ffi::OsStr;
        use std::path::{Path, PathBuf};

        let program = Path::new("/opt/torro/torroworkplace");
        let command = daemon_command(program, Some(PathBuf::from("/home/anna")));
        assert_eq!(command.get_program(), program.as_os_str());
        assert_eq!(command.get_args().collect::<Vec<_>>(), [OsStr::new("daemon")]);
        assert_eq!(command.get_current_dir(), Some(Path::new("/home/anna")));
        assert!(command.get_envs().any(|(name, value)| name == "TORRO_TOKEN" && value.is_none()));

        let homeless = daemon_command(program, None);
        assert_eq!(homeless.get_current_dir(), Some(Path::new("/")));
    }
}
