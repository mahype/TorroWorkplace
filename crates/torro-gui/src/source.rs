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
//!
//! On macOS the programs of the suite live in bundles (decisions 6 and 7 in `docs/lebenszyklus.md`):
//! [`bundle_program`] finds an entry's program in its bundle, [`service_program`] the core's in the
//! agent bundle "Torro Workplace Service.app" inside "Torro Workplace.app". Both only decide paths, so
//! they are tested on every system; only their callers ask the file system on macOS.
//!
//! Every app brings a copy of the service along (issue #411), so on macOS several bundles may hold one,
//! of different versions: [`newest_service`] picks the one to start, the highest version
//! ([`choose_service`], [`plist_version`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use torro_core::timing;
use torro_core::version::Version;
use torro_ipc::pim::PimClient;
use torro_ipc::{Request, Response};
use torro_pim::demo::DemoSource;
use torro_pim::model::SeriesScope;
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
    /// starting a second one. Answer of "Open Tasks ↗" and the other jumps to an app.
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

/// How long an app waits at its start for the core to answer ([`Source::ready`]), which
/// [`ensure_core`] may have just started: on every system, since Windows starts it too (issue #279,
/// decision 8 in `docs/lebenszyklus.md`).
pub const CORE_START_WAIT: Duration = Duration::from_secs(10);

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
/// app then retries until the core answers ([`Source::ready`]). On Unix and Windows (issue #279).
///
/// The program is [`core_program`]: on macOS the core's own bundle first, else the binary next to
/// this one or on `PATH`.
///
/// Not when a program of the suite tried to start it within the last [`CORE_START_GAP`]
/// ([`claim_core_start`], issue #279): that start is still under way, or the core failed at once,
/// and starting it again would only repeat that.
pub fn ensure_core(socket: &std::path::Path) {
    #[cfg(any(unix, windows))]
    {
        if !nobody_listens(socket) {
            return;
        }
        if !claim_core_start(socket, Utc::now()) {
            return;
        }
        let exe = std::env::current_exe().ok();
        let home = home_dir();
        let program = core_program(exe.as_deref(), home.as_deref());
        match daemon_command(&program, home, &[]).spawn() {
            // Reap the daemon if it ever exits while the app still runs.
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(err) => eprintln!("cannot start {}: {err}", program.display()),
        }
    }
    #[cfg(not(any(unix, windows)))]
    let _ = socket;
}

/// Whether no core listens on the socket `socket`, so [`ensure_core`] should start one. Connecting
/// is the test; the core drops a connection that sends nothing.
#[cfg(unix)]
fn nobody_listens(socket: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(socket).is_err()
}

/// Whether no core listens on the named pipe `socket` (`\\.\pipe\torro-workplace-<SID>`), so
/// [`ensure_core`] should start one. Opening the pipe is the test, as connecting on Unix. Only "no
/// such pipe" means nobody: a busy pipe (all instances connected) has a core, and a name held by
/// another program would refuse a new core as well. A socket that is no pipe name (the user's SID
/// unknown) is one no core can listen on, so none is started for it.
///
/// Opened with the security quality of service "identification", as every client of the suite opens
/// a pipe (`torro_ipc::client`, `torro_gui::instance`): a program holding the name may learn who asks,
/// but not act as the user. std adds `SECURITY_SQOS_PRESENT` itself.
#[cfg(windows)]
fn nobody_listens(socket: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;

    /// `SECURITY_IDENTIFICATION` from `winnt.h`, as a number like in `instance.rs`.
    const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;
    const PIPES: &str = r"\\.\pipe\";
    let is_pipe =
        socket.to_str().and_then(|path| path.get(..PIPES.len())).is_some_and(|start| start.eq_ignore_ascii_case(PIPES));
    if !is_pipe {
        return false;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).security_qos_flags(SECURITY_IDENTIFICATION);
    match options.open(socket) {
        Ok(_) => false,
        Err(err) => err.kind() == std::io::ErrorKind::NotFound,
    }
}

/// The user's home directory from the environment (`HOME`, on Windows `USERPROFILE`); `None` when
/// unset or empty.
pub fn home_dir() -> Option<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(name).filter(|home| !home.is_empty()).map(PathBuf::from)
}

/// The core's program for [`ensure_core`] (decision 6 in `docs/lebenszyklus.md`): on macOS the newest
/// copy of the service's agent bundle in the bundles of the suite ([`newest_service`], issue #411),
/// beside this program's bundle or in an Applications folder, so the core runs with its own bundle id and
/// without a Dock icon; else, and without any such bundle (development, Homebrew), `torroworkplace` as
/// [`sibling`] finds it.
fn core_program(exe: Option<&Path>, home: Option<&Path>) -> PathBuf {
    // macOS: the newest of the bundles' copies; Windows under Scoop: the newest of the apps' copies
    // (issue #411). Elsewhere the one program beside this one, as [`sibling`] finds it.
    #[cfg(any(target_os = "macos", windows))]
    if let Some((program, _)) = newest_service(exe, home) {
        return program;
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (exe, home);
    sibling(torro_core::AppId::WORKPLACE.binary)
}

/// The command [`ensure_core`] starts the core with: `program daemon` and the arguments `extra` (a
/// service handing over to a newer copy adds `--takeover`, issue #411), detached from the caller's
/// terminal and process group (Unix) or console and process group (Windows: `DETACHED_PROCESS`,
/// `CREATE_NEW_PROCESS_GROUP`), so it outlives the app or MCP server that started it and a Ctrl+C
/// in the caller's console does not reach it.
///
/// It runs in `home` (else `/`), not in the caller's directory, which it would otherwise keep
/// busy for as long as it runs. `TORRO_TOKEN` is not passed on: it is the key of the agent whose
/// MCP server started the core, and the core has no use for it.
#[cfg(any(unix, windows))]
pub fn daemon_command(
    program: &std::path::Path,
    home: Option<std::path::PathBuf>,
    extra: &[&str],
) -> std::process::Command {
    use std::process::{Command, Stdio};

    let mut command = Command::new(program);
    command
        .arg("daemon")
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("TORRO_TOKEN")
        .current_dir(home.unwrap_or_else(|| std::path::PathBuf::from("/")));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // From `winbase.h`; as numbers, so no binding crate is needed for two flags.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
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

/// `name` next to the running binary (an install puts the suite in one directory); on macOS else the
/// program `name` in its bundle ([`bundled_program_file`], issue #279: an app in its own bundle has no
/// other program beside it, so the mail program finds `torroinbox` for the compose window in the
/// bundle of the Notification Hub); else `name` from `PATH`.
pub fn sibling(name: &str) -> std::path::PathBuf {
    let exe = std::env::current_exe().ok();
    let beside = exe.as_deref().and_then(Path::parent).map(|dir| dir.join(name)).filter(|path| path.exists());
    #[cfg(target_os = "macos")]
    let beside = beside.or_else(|| bundled_program_file(name, exe.as_deref(), home_dir().as_deref(), Path::is_file));
    beside.unwrap_or_else(|| std::path::PathBuf::from(name))
}

/// The bundle of the core's service inside the bundle of the control centre, relative to the folder
/// that holds "Torro Workplace.app" (decision 7 in `docs/lebenszyklus.md`): an agent bundle without a
/// Dock icon, so a double click on "Torro Workplace" opens the control centre even while the core runs.
/// In `Contents/Library/LoginItems`, where Apple wants helper apps: a bundle directly in
/// `Contents/Library` is sealed as a resource of the outer one, which notarisation and `spctl` may
/// reject.
pub const SERVICE_PROGRAM: &str =
    "Torro Workplace.app/Contents/Library/LoginItems/Torro Workplace Service.app/Contents/MacOS/torroworkplace";

/// The folders the suite's bundles are looked for in (issue #279, decision 6 in
/// `docs/lebenszyklus.md`), in this order: the folder holding each bundle that `exe` (this program)
/// runs in, innermost first, then `/Applications`, then `<home>/Applications`.
///
/// "Each bundle", because the core's service runs in a bundle inside "Torro Workplace.app"
/// ([`SERVICE_PROGRAM`]): beside its own bundle is `Torro Workplace.app/Contents/Library/LoginItems`, and the
/// apps lie beside the outer bundle. A program in no bundle (development, Homebrew) adds none.
/// Only paths from the root, so nothing is ever looked up relative to the working directory, and
/// each folder once.
pub fn bundle_folders(exe: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let beside = exe
        .into_iter()
        .flat_map(Path::ancestors)
        .filter(|folder| folder.extension().is_some_and(|ext| ext == "app"))
        .filter_map(Path::parent);
    let applications = [Some(PathBuf::from("/Applications")), home.map(|home| home.join("Applications"))];
    let mut folders: Vec<PathBuf> = Vec::new();
    for folder in beside.map(Path::to_path_buf).chain(applications.into_iter().flatten()) {
        if folder.has_root() && !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    folders
}

/// `inside` (a path below a folder, `Torro Reminder.app/Contents/MacOS/torro-reminder`) in the first of
/// [`bundle_folders`] where `exists` says it is; `None` when in none. Pure: `exists` is
/// [`Path::is_file`] for real and a fake in tests.
pub fn find_bundled(
    inside: &Path,
    exe: Option<&Path>,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    bundle_folders(exe, home).into_iter().map(|folder| folder.join(inside)).find(|program| exists(program))
}

/// The program of the entry `app` in its bundle (issue #279, decision 6 in `docs/lebenszyklus.md`):
/// `<display_name>.app/Contents/MacOS/<program_file>` ([`torro_core::AppId::display_name`],
/// [`torro_core::AppId::program_file`]), so `Torro Workplace Mail.app/Contents/MacOS/torro-inbox-mail`,
/// looked for with [`find_bundled`]: beside the bundle `exe` runs in, in `/Applications`, in
/// `<home>/Applications`. `None` when there is none.
///
/// For [`torro_core::AppId::WORKPLACE`] this is the control centre's bundle, `Torro Workplace.app`; the
/// core's service is [`service_program`]. Pure, tested on every system; only callers on macOS ask the
/// file system (`exists` = [`Path::is_file`]).
pub fn bundle_program(
    app: torro_core::AppId,
    exe: Option<&Path>,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let inside = Path::new(&format!("{}.app", app.display_name)).join("Contents/MacOS").join(app.program_file());
    find_bundled(&inside, exe, home, exists)
}

/// The core's program in its agent bundle ([`SERVICE_PROGRAM`]), looked for as [`bundle_program`]
/// looks for an app; `None` when there is none. What [`ensure_core`] starts first on macOS.
pub fn service_program(exe: Option<&Path>, home: Option<&Path>, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    find_bundled(Path::new(SERVICE_PROGRAM), exe, home, exists)
}

/// Where the service's program lies inside the bundle of any entry of the suite (issue #411): every app
/// brings the agent bundle along, at the place [`SERVICE_PROGRAM`] names inside "Torro Workplace.app".
pub const SERVICE_IN_BUNDLE: &str =
    "Contents/Library/LoginItems/Torro Workplace Service.app/Contents/MacOS/torroworkplace";

/// The service's program in the bundle of `app` ([`SERVICE_IN_BUNDLE`]), relative to the folder holding
/// that bundle: `Torro Calendar.app/Contents/Library/LoginItems/…/torroworkplace`; for
/// [`torro_core::AppId::WORKPLACE`] this is [`SERVICE_PROGRAM`].
pub fn service_in(app: torro_core::AppId) -> PathBuf {
    Path::new(&format!("{}.app", app.display_name)).join(SERVICE_IN_BUNDLE)
}

/// The `Info.plist` of the bundle `program` runs in (`…/Contents/MacOS/<program>` →
/// `…/Contents/Info.plist`); `None` for a path too short to lie in a bundle.
pub fn info_plist_of(program: &Path) -> Option<PathBuf> {
    let contents = program.parent()?.parent()?;
    Some(contents.join("Info.plist"))
}

/// The version in the XML property list `text` (an `Info.plist`): the suite's whole version `TorroVersion`
/// (pre-release included, `0.2.0-rc.1`), else Apple's `CFBundleShortVersionString` (digits and dots
/// only, so a pre-release reads as its release; bundles from before #411's review). `None` when the
/// text is no XML or has neither as a version. A binary property list is not read: the suite's bundles
/// carry XML (`packaging/macos/Info.plist.in`).
pub fn plist_version(text: &str) -> Option<Version> {
    // The bundles' lists carry Apple's DOCTYPE, which `roxmltree` refuses unless told otherwise.
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..roxmltree::ParsingOptions::default() };
    let document = roxmltree::Document::parse_with_options(text, options).ok()?;
    let dict = document.root_element().children().find(|node| node.has_tag_name("dict"))?;
    let value_of = |wanted: &str| -> Option<Version> {
        // In a `dict` each `key` is followed by its value, the next element.
        let mut elements = dict.children().filter(roxmltree::Node::is_element);
        while let Some(node) = elements.next() {
            if node.has_tag_name("key") && node.text() == Some(wanted) {
                let value = elements.next().filter(|value| value.has_tag_name("string"))?;
                return value.text()?.parse().ok();
            }
        }
        None
    };
    value_of("TorroVersion").or_else(|| value_of("CFBundleShortVersionString"))
}

/// The copy of the service to start among those in the bundles of the suite (issue #411): each entry of
/// [`torro_core::AppId::ALL`] in each of [`bundle_folders`] is a candidate ([`service_in`]) where `exists`
/// says it is. The highest version wins (`version_of` reads it from the program's path; a copy whose
/// version cannot be read counts as `0.0.0`, so it is started only when there is no other); with equal
/// versions the one inside "Torro Workplace.app", then the one in the bundle `exe` runs in, then the first
/// found. `None` when there is none.
///
/// Pure: `exists` and `version_of` stand in for the file system, so it is tested on every system;
/// [`newest_service`] passes the real one on macOS.
pub fn choose_service(
    exe: Option<&Path>,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
    version_of: impl Fn(&Path) -> Option<Version>,
) -> Option<(PathBuf, Version)> {
    let unknown = Version { major: 0, minor: 0, patch: 0, pre: Vec::new() };
    // The bundles `exe` runs in; a candidate below one of them lies in "the own bundle".
    let own: Vec<&Path> = exe
        .into_iter()
        .flat_map(Path::ancestors)
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
        .collect();
    let mut found: Vec<(PathBuf, Version, bool, bool)> = Vec::new();
    for folder in bundle_folders(exe, home) {
        for app in torro_core::AppId::ALL.iter().copied() {
            let program = folder.join(service_in(app));
            if found.iter().any(|(seen, ..)| *seen == program) || !exists(&program) {
                continue;
            }
            let version = version_of(&program).unwrap_or_else(|| unknown.clone());
            let in_workplace = app == torro_core::AppId::WORKPLACE;
            let in_own = own.iter().any(|bundle| program.starts_with(bundle));
            found.push((program, version, in_workplace, in_own));
        }
    }
    // `max_by` keeps the last of equals; reversed, the first found of full equals wins.
    found
        .into_iter()
        .rev()
        .max_by(|a, b| (&a.1, a.2, a.3).cmp(&(&b.1, b.2, b.3)))
        .map(|(program, version, ..)| (program, version))
}

/// The version of the service's copy `program` from its bundle's `Info.plist` ([`info_plist_of`],
/// [`plist_version`]); `None` when it cannot be read.
pub fn service_version(program: &Path) -> Option<Version> {
    let text = std::fs::read_to_string(info_plist_of(program)?).ok()?;
    plist_version(&text)
}

/// The newest copy of the core's service and its version (issue #411): what [`ensure_core`] starts, and
/// what the core hands over to after an install or update. `exe` is the program asking (the bundles it
/// runs in are looked in first), `home` the user's home (for `~/Applications`).
///
/// On macOS [`choose_service`] over the bundles of the suite, on the file system; `None` without any
/// (development, Homebrew). Under Scoop every app brings a copy of its own: [`choose_scoop_service`].
/// Otherwise a package installs the service once, as the program `torroworkplace` beside the suite's
/// other programs: that program beside `exe`, with the caller's own version ([`Version::current`]),
/// since a package installs the suite's programs of one version together; `None` when it is not there.
pub fn newest_service(exe: Option<&Path>, home: Option<&Path>) -> Option<(PathBuf, Version)> {
    newest_service_except(exe, home, None)
}

/// [`newest_service`] without the copies below `except`: the copy left once an app is uninstalled,
/// which the core checks before it removes the bundle its service runs from.
pub fn newest_service_except(
    exe: Option<&Path>,
    home: Option<&Path>,
    except: Option<&Path>,
) -> Option<(PathBuf, Version)> {
    let counts = |program: &Path| program.is_file() && !except.is_some_and(|except| program.starts_with(except));
    #[cfg(target_os = "macos")]
    {
        choose_service(exe, home, counts, service_version)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = home;
        let folders = |apps: &Path| -> Vec<PathBuf> {
            std::fs::read_dir(apps)
                .map(|entries| entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect())
                .unwrap_or_default()
        };
        if let Some(found) = choose_scoop_service(exe, counts, folders, scoop_version) {
            return Some(found);
        }
        let name = format!("{}{}", torro_core::AppId::WORKPLACE.binary, std::env::consts::EXE_SUFFIX);
        let program = exe?.parent()?.join(name);
        counts(&program).then(|| (program, Version::current()))
    }
}

/// The newest copy of the service among the apps Scoop installed (issue #411): each app's manifest
/// brings `torroworkplace.exe` into its own folder, and Scoop updates apps one by one, so the copies'
/// versions can differ. `exe` must lie in Scoop's layout `<root>/apps/<app>/<version>/`, which Scoop's
/// own folder `<root>/apps/scoop` marks (the root may be anywhere, `SCOOP`); candidates
/// are `<scoop>/apps/*/current/torroworkplace.exe` where `exists` says so, with the version
/// `version_of` reads (Scoop's `manifest.json` beside it; unreadable counts as `0.0.0`). The highest
/// wins, with equal ones the one in the app `exe` belongs to. `None` outside Scoop's layout.
///
/// Pure: `exists`, `folders` (the folders in a folder) and `version_of` stand in for the file system.
pub fn choose_scoop_service(
    exe: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
    folders: impl Fn(&Path) -> Vec<PathBuf>,
    version_of: impl Fn(&Path) -> Option<Version>,
) -> Option<(PathBuf, Version)> {
    let own = exe?.parent()?.parent()?;
    let apps = own.parent()?;
    if apps.file_name()? != "apps" {
        return None;
    }
    let folders = folders(apps);
    if !folders.iter().any(|folder| folder.file_name().is_some_and(|name| name == "scoop")) {
        return None;
    }
    let unknown = Version { major: 0, minor: 0, patch: 0, pre: Vec::new() };
    let mut found: Vec<(PathBuf, Version, bool)> = folders
        .into_iter()
        .map(|app| (app.join("current").join("torroworkplace.exe"), app == own))
        .filter(|(program, _)| exists(program))
        .map(|(program, mine)| {
            let version = version_of(&program).unwrap_or_else(|| unknown.clone());
            (program, version, mine)
        })
        .collect();
    found.sort_by(|a, b| (&a.1, a.2).cmp(&(&b.1, b.2)));
    found.pop().map(|(program, version, _)| (program, version))
}

/// The version Scoop noted for the app whose copy of the service is `program`: the key `version` of
/// `manifest.json` beside it.
#[cfg_attr(target_os = "macos", allow(dead_code, reason = "Scoop is a package manager of Windows"))]
fn scoop_version(program: &Path) -> Option<Version> {
    let text = std::fs::read_to_string(program.parent()?.join("manifest.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest.get("version")?.as_str()?.parse().ok()
}

/// The program file `name` (`torroinbox`) in the bundle of the first entry of
/// [`torro_core::AppId::ALL`] whose [`torro_core::AppId::program_file`] it is ([`bundle_program`]): the
/// Notification Hub's bundle for `torroinbox`, the control centre's for `torroworkplace`. `None` for a
/// name of no entry or without its bundle. What [`sibling`] falls back on, on macOS.
pub fn bundled_program_file(
    name: &str,
    exe: Option<&Path>,
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let app = torro_core::AppId::ALL.iter().find(|app| app.program_file() == name)?;
    bundle_program(*app, exe, home, exists)
}

/// The bundle a program in it runs from: `…/Torro Calendar.app` for
/// `…/Torro Calendar.app/Contents/MacOS/torrocalendar`; `None` for a program in no bundle.
pub fn bundle_of(program: &Path) -> Option<&Path> {
    program.ancestors().nth(3).filter(|bundle| bundle.extension().is_some_and(|ext| ext == "app"))
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

    async fn save_event(&self, event: Event, scope: Option<SeriesScope>) -> Result<Event, ProviderError> {
        delegate!(self, s => s.save_event(event, scope))
    }

    async fn delete_event(&self, event: Event, scope: Option<SeriesScope>) -> Result<(), ProviderError> {
        delegate!(self, s => s.delete_event(event, scope))
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

    async fn set_focus(
        &self,
        todo: Todo,
        focus: torro_pim::Focus,
        release: Option<Todo>,
    ) -> Result<Vec<Todo>, ProviderError> {
        delegate!(self, s => s.set_focus(todo, focus, release))
    }

    async fn plan_todo(
        &self,
        todo: Todo,
        day: chrono::NaiveDate,
        before: Option<Todo>,
    ) -> Result<Vec<Todo>, ProviderError> {
        delegate!(self, s => s.plan_todo(todo, day, before))
    }

    async fn postpone_focus_plan(&self) -> Result<Vec<Todo>, ProviderError> {
        delegate!(self, s => s.postpone_focus_plan())
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

    /// Issue #411: under Scoop every app has a copy of the service; the newest starts, with equals the
    /// app's own, and nothing is chosen outside Scoop's layout.
    #[test]
    fn the_newest_scoop_copy_of_the_service_starts() {
        let apps = PathBuf::from("/d/pkgs/apps");
        let exe = apps.join("torrotasks/0.1.0/torrotasks.exe");
        let folders = |_: &Path| {
            vec![apps.join("scoop"), apps.join("torrocalendar"), apps.join("torrotasks"), apps.join("torroinbox")]
        };
        let copy = |app: &str| apps.join(app).join("current/torroworkplace.exe");
        let version = |program: &Path| match program.to_string_lossy() {
            text if text.contains("torrocalendar") => "0.2.0".parse().ok(),
            text if text.contains("torroinbox") => None,
            _ => "0.1.0".parse().ok(),
        };
        let all = |program: &Path| program.ends_with("torroworkplace.exe");
        let chosen = choose_scoop_service(Some(&exe), all, folders, version).expect("a copy");
        assert_eq!(chosen, (copy("torrocalendar"), "0.2.0".parse().expect("version")));
        let without_calendar = |program: &Path| all(program) && !program.starts_with(apps.join("torrocalendar"));
        let chosen = choose_scoop_service(Some(&exe), without_calendar, folders, version).expect("a copy");
        assert_eq!(chosen.0, copy("torrotasks"), "0.1.0 beats an unreadable version");
        let no_scoop = |_: &Path| vec![apps.join("torrotasks")];
        assert_eq!(choose_scoop_service(Some(&exe), all, no_scoop, version), None, "no folder of Scoop itself");
    }

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

    #[cfg(any(unix, windows))]
    #[test]
    fn the_core_starts_in_home_without_the_agent_key() {
        use std::ffi::OsStr;
        use std::path::{Path, PathBuf};

        let program = Path::new("/opt/torro/torroworkplace");
        let command = daemon_command(program, Some(PathBuf::from("/home/anna")), &[]);
        assert_eq!(command.get_program(), program.as_os_str());
        assert_eq!(command.get_args().collect::<Vec<_>>(), [OsStr::new("daemon")]);
        assert_eq!(command.get_current_dir(), Some(Path::new("/home/anna")));
        assert!(command.get_envs().any(|(name, value)| name == "TORRO_TOKEN" && value.is_none()));

        let homeless = daemon_command(program, None, &[]);
        assert_eq!(homeless.get_current_dir(), Some(Path::new("/")));

        // Issue #411: a hand-over to a newer copy adds its argument after `daemon`.
        let takeover = daemon_command(program, None, &["--takeover"]);
        assert_eq!(takeover.get_args().collect::<Vec<_>>(), [OsStr::new("daemon"), OsStr::new("--takeover")]);
    }

    /// The program of `app` in its bundle in `folder`, as [`bundle_program`] names it.
    fn in_bundle(folder: &str, app: torro_core::AppId) -> PathBuf {
        Path::new(folder).join(format!("{}.app/Contents/MacOS/{}", app.display_name, app.program_file()))
    }

    /// Issue #279: every entry is found beside the bundle the caller runs in first, then in
    /// `/Applications`, then in `~/Applications`; without any, `None`.
    #[test]
    fn every_entry_is_found_in_its_bundle_beside_then_in_the_applications_folders() {
        let exe = Path::new("/Volumes/Apps/Torro Calendar.app/Contents/MacOS/torrocalendar");
        let home = Path::new("/Users/anna");
        for app in torro_core::AppId::ALL.iter().copied() {
            let beside = in_bundle("/Volumes/Apps", app);
            let system = in_bundle("/Applications", app);
            let own = in_bundle("/Users/anna/Applications", app);
            let all = |path: &Path| path == beside || path == system || path == own;
            assert_eq!(bundle_program(app, Some(exe), Some(home), all), Some(beside.clone()), "{}", app.window_class);
            let installed = |path: &Path| path == system || path == own;
            assert_eq!(bundle_program(app, Some(exe), Some(home), installed), Some(system.clone()));
            assert_eq!(bundle_program(app, Some(exe), Some(home), |path| path == own), Some(own.clone()));
            assert_eq!(bundle_program(app, Some(exe), None, |path| path == own), None, "no home, no ~/Applications");
            assert_eq!(bundle_program(app, Some(exe), Some(home), |_| false), None);
        }
        let mail = in_bundle("/Applications", torro_core::AppId::INBOX_MAIL);
        assert!(mail.ends_with("Torro Workplace Mail.app/Contents/MacOS/torro-inbox-mail"), "{mail:?}");
        let control = in_bundle("/Applications", torro_core::AppId::WORKPLACE);
        assert!(control.ends_with("Torro Workplace.app/Contents/MacOS/torroworkplace"), "{control:?}");
    }

    /// Issue #279 (decision 7): the core's service in its agent bundle inside "Torro Workplace.app",
    /// found the same way; and the service, running there, finds the apps beside the outer bundle.
    #[test]
    fn the_service_lies_in_its_agent_bundle_inside_torro_workplace() {
        let home = Path::new("/Users/anna");
        let calendar = Path::new("/Volumes/Apps/Torro Calendar.app/Contents/MacOS/torrocalendar");
        let service = |folder: &str| Path::new(folder).join(SERVICE_PROGRAM);
        let (beside, system, own) =
            (service("/Volumes/Apps"), service("/Applications"), service("/Users/anna/Applications"));
        let all = |path: &Path| path == beside || path == system || path == own;
        assert_eq!(service_program(Some(calendar), Some(home), all), Some(beside.clone()));
        assert_eq!(service_program(Some(calendar), Some(home), |path| path == system || path == own), Some(system));
        assert_eq!(service_program(Some(calendar), Some(home), |path| path == own), Some(own));
        assert_eq!(service_program(Some(calendar), Some(home), |_| false), None);
        assert!(
            beside.ends_with(
                "Torro Workplace.app/Contents/Library/LoginItems/Torro Workplace Service.app/Contents/MacOS/torroworkplace"
            ),
            "where Apple wants helper apps: {beside:?}"
        );

        // The control centre finds the service in its own bundle.
        let control = Path::new("/Volumes/Apps/Torro Workplace.app/Contents/MacOS/torroworkplace");
        assert_eq!(service_program(Some(control), None, |path| path == beside), Some(beside.clone()));

        // The service runs in Torro Workplace.app/Contents/Library/LoginItems: the apps lie beside the outer
        // bundle.
        assert_eq!(
            bundle_folders(Some(&beside), Some(home)),
            [
                PathBuf::from("/Volumes/Apps/Torro Workplace.app/Contents/Library/LoginItems"),
                PathBuf::from("/Volumes/Apps"),
                PathBuf::from("/Applications"),
                PathBuf::from("/Users/anna/Applications"),
            ]
        );
        let tasks = in_bundle("/Volumes/Apps", torro_core::AppId::TASKS);
        assert_eq!(bundle_program(torro_core::AppId::TASKS, Some(&beside), None, |path| path == tasks), Some(tasks));
    }

    /// Issue #279: a program in no bundle looks only in the Applications folders, and never relative
    /// to the working directory.
    #[test]
    fn a_program_in_no_bundle_looks_in_the_applications_folders_only() {
        let dev = Path::new("/home/anna/torro/target/debug/torrocalendar");
        assert_eq!(bundle_folders(Some(dev), None), [PathBuf::from("/Applications")]);
        assert_eq!(bundle_folders(None, Some(Path::new("relative"))), [PathBuf::from("/Applications")]);
        let inside = Path::new("Torro Reminder.app/Contents/MacOS/torro-reminder");
        let installed = Path::new("/Applications").join(inside);
        assert_eq!(find_bundled(inside, Some(dev), None, |path| path == installed), Some(installed.clone()));
    }

    /// Issue #279: on macOS `sibling` finds a program in its entry's bundle, `torroinbox` (compose
    /// from the mail program) in the Notification Hub's; a name of no entry has none.
    #[test]
    fn a_program_file_is_found_in_the_bundle_of_its_entry() {
        let mail = Path::new("/Applications/Torro Workplace Mail.app/Contents/MacOS/torro-inbox-mail");
        let hub = in_bundle("/Applications", torro_core::AppId::INBOX);
        assert!(hub.ends_with("Torro Notification Hub.app/Contents/MacOS/torroinbox"), "{hub:?}");
        assert_eq!(bundled_program_file("torroinbox", Some(mail), None, |path| path == hub), Some(hub.clone()));
        assert_eq!(bundled_program_file("torro-inbox-mail", Some(mail), None, |path| path == mail), Some(mail.into()));
        assert_eq!(bundled_program_file("bash", Some(mail), None, |_| true), None);
    }

    fn version(text: &str) -> Version {
        text.parse().expect("a version")
    }

    /// An `Info.plist` as `packaging/macos/Info.plist.in` builds it, with `version` in it: Apple's key
    /// without the pre-release (as `make-app.sh` cuts it), the whole one in `TorroVersion`.
    fn info_plist(version: &str) -> String {
        let short = version.split('-').next().unwrap_or(version);
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n<dict>\n\
             \t<key>CFBundleIdentifier</key>\n\t<string>io.github.mahype.torro-workplace-service</string>\n\
             \t<key>CFBundleVersion</key>\n\t<string>9.9.9</string>\n\
             \t<key>CFBundleShortVersionString</key>\n\t<string>{short}</string>\n\
             \t<key>TorroVersion</key>\n\t<string>{version}</string>\n\
             \t<key>LSUIElement</key>\n\t<true/>\n\
             </dict>\n</plist>\n"
        )
    }

    /// Issue #411: the version is `CFBundleShortVersionString`, not another key; anything else gives none.
    #[test]
    fn the_version_of_a_bundle_is_read_from_its_property_list() {
        assert_eq!(plist_version(&info_plist("0.2.0")), Some(version("0.2.0")));
        assert_eq!(plist_version(&info_plist("not a version")), None);
        assert_eq!(plist_version("<plist><dict><key>CFBundleVersion</key><string>1.0.0</string></dict></plist>"), None);
        let wrong_type = "<plist><dict><key>CFBundleShortVersionString</key><true/></dict></plist>";
        assert_eq!(plist_version(wrong_type), None);
        let last = "<plist><dict><key>CFBundleShortVersionString</key></dict></plist>";
        assert_eq!(plist_version(last), None);
        assert_eq!(plist_version("bplist00…"), None, "a binary list is not read");
        // Review of #411: Apple's key cuts a pre-release off; the suite's own key keeps it.
        let both = "<plist><dict><key>CFBundleShortVersionString</key><string>0.2.0</string>\
                    <key>TorroVersion</key><string>0.2.0-rc.1</string></dict></plist>";
        assert_eq!(plist_version(both), Some(version("0.2.0-rc.1")));
        let program = Path::new("/Applications/Torro Calendar.app").join(SERVICE_IN_BUNDLE);
        assert_eq!(
            info_plist_of(&program),
            Some(PathBuf::from(
                "/Applications/Torro Calendar.app/Contents/Library/LoginItems/Torro Workplace Service.app/Contents/Info.plist"
            ))
        );
        assert_eq!(service_in(torro_core::AppId::WORKPLACE), PathBuf::from(SERVICE_PROGRAM));
    }

    /// Issue #411: of several copies of the service the highest version starts; with equal versions the
    /// one inside "Torro Workplace.app", then the one in the caller's own bundle.
    #[test]
    fn the_newest_copy_of_the_service_is_chosen() {
        let home = Path::new("/Users/anna");
        let calendar_exe = Path::new("/Applications/Torro Calendar.app/Contents/MacOS/torrocalendar");
        let copy = |folder: &str, app: torro_core::AppId| Path::new(folder).join(service_in(app));
        let workplace = copy("/Applications", torro_core::AppId::WORKPLACE);
        let calendar = copy("/Applications", torro_core::AppId::CALENDAR);
        let tasks = copy("/Users/anna/Applications", torro_core::AppId::TASKS);
        assert!(calendar.ends_with(
            "Torro Calendar.app/Contents/Library/LoginItems/Torro Workplace Service.app/Contents/MacOS/torroworkplace"
        ));
        let copies = |list: Vec<(PathBuf, &'static str)>| {
            move |program: &Path| list.iter().find(|(path, _)| path == program).map(|(_, v)| version(v))
        };
        let choose = |exe: &Path, list: Vec<(PathBuf, &'static str)>| {
            let paths: Vec<PathBuf> = list.iter().map(|(path, _)| path.clone()).collect();
            choose_service(Some(exe), Some(home), |program| paths.iter().any(|path| path == program), copies(list))
        };

        // The highest version wins, wherever it lies.
        let newer_tasks = vec![(workplace.clone(), "0.1.0"), (calendar.clone(), "0.2.0"), (tasks.clone(), "0.3.0")];
        assert_eq!(choose(calendar_exe, newer_tasks), Some((tasks.clone(), version("0.3.0"))));
        let pre = vec![(workplace.clone(), "0.2.0"), (tasks.clone(), "0.2.0-rc.1")];
        assert_eq!(choose(calendar_exe, pre), Some((workplace.clone(), version("0.2.0"))), "a release beats its rc");

        // Equal versions: Torro Workplace first, then the own bundle, then the first found.
        let equal = vec![(workplace.clone(), "0.2.0"), (calendar.clone(), "0.2.0"), (tasks.clone(), "0.2.0")];
        assert_eq!(choose(calendar_exe, equal), Some((workplace.clone(), version("0.2.0"))));
        let without_workplace = vec![(calendar.clone(), "0.2.0"), (tasks.clone(), "0.2.0")];
        let tasks_exe = Path::new("/Users/anna/Applications/Torro Tasks.app/Contents/MacOS/torrotasks");
        assert_eq!(choose(tasks_exe, without_workplace.clone()), Some((tasks.clone(), version("0.2.0"))));
        assert_eq!(choose(calendar_exe, without_workplace.clone()), Some((calendar.clone(), version("0.2.0"))));
        let elsewhere = Path::new("/Users/anna/torro/target/debug/torroworkplace");
        assert_eq!(choose(elsewhere, without_workplace), Some((calendar.clone(), version("0.2.0"))), "first found");

        // A copy without a readable version starts only when there is no other.
        let unreadable = vec![(calendar.clone(), "0.1.0")];
        let found = |program: &Path| *program == calendar || *program == workplace;
        let only_calendar = choose_service(Some(calendar_exe), Some(home), found, copies(unreadable));
        assert_eq!(only_calendar, Some((calendar.clone(), version("0.1.0"))));
        let none_readable = choose_service(Some(calendar_exe), Some(home), |program| *program == workplace, |_| None);
        assert_eq!(none_readable, Some((workplace.clone(), version("0.0.0"))));

        // The service, running inside the calendar's bundle, finds the copies beside it as well.
        let in_calendar = calendar.clone();
        let both = vec![(workplace.clone(), "0.2.0"), (calendar.clone(), "0.3.0")];
        assert_eq!(choose(&in_calendar, both), Some((calendar.clone(), version("0.3.0"))));

        assert_eq!(choose_service(Some(calendar_exe), Some(home), |_| false, |_| Some(version("1.0.0"))), None);
    }

    #[test]
    fn the_bundle_of_a_program_is_three_folders_up() {
        let program = Path::new("/Applications/Torro Calendar.app/Contents/MacOS/torrocalendar");
        assert_eq!(bundle_of(program), Some(Path::new("/Applications/Torro Calendar.app")));
        assert_eq!(bundle_of(Path::new("/home/anna/torro/target/debug/torrocalendar")), None);
    }
}
