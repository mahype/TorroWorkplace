//! `torroworkplace daemon`: the background service.
//!
//! One process per session. A multi-threaded tokio runtime runs:
//!
//! - the socket (`torro_ipc::server`) for `ctl`, hotkeys, the control centre, the apps and
//!   agents; every request there passes the key check ([`Core::handle_from`]),
//! - the tray icon (see `tray`),
//! - a refresher that rebuilds the tray menu after every command, on window manager events and
//!   when the open approvals change ([`watch_approvals`]) or anything is written to the inbox
//!   ([`watch_inbox`]), all debounced into one refresh; on macOS and Windows the template keys
//!   ([`template_keys`]) go with each new menu, so a change of a key or the modifiers refreshes too,
//! - the inbox routine: wakes snoozed entries every minute and removes old done ones once a day
//!   ([`tend_inbox`]),
//!
//! and ends on "Quit", Ctrl+C, SIGTERM on Unix (`systemctl --user stop`, `kill`; [`stop_signals`]) or
//! when the socket server fails; Ctrl+C and SIGTERM during the start end the process at once. Every
//! command, from the tray or the socket, goes through [`Core::handle`]. One service per user: it locks a file in its
//! state directory before the core starts ([`claim_service`]), and when it ends it stops the socket
//! server and releases the template keys it bound on Hyprland ([`Core::release_template_keys`]), after
//! which nothing binds them again, and ends the MCP servers it started with their process trees
//! ([`crate::extensions::supervisor::Supervisor::stop_all`]).
//!
//! Where the main thread goes depends on the system. On Linux it waits for the service
//! (`block_on`); ksni runs its D-Bus task on the same runtime. On macOS and Windows the tray
//! needs a native event loop on the main thread, so the service runs as a task on the runtime's
//! worker threads and the main thread runs [`tray::Shell`] until the service ends.
//! `--headless` runs no tray and no event loop on any system.
//!
//! A person's quit (tray › Quit, `ctl quit`) leaves the marker [`QUIT_MARK`] next to the socket, which
//! the next start removes (issue #279, decision 4 in `docs/lebenszyklus.md`): while it stands, no MCP
//! server starts the service again. The first-run setup opens only once the socket answers
//! ([`once_listening`]), so a service that fails on its socket opens no window that would start the
//! next one. Then, too, the service removes the marker of the last try to start it
//! ([`torro_gui::source::CORE_START_MARK`]): it runs, so a quit right after may be followed by a start
//! at once.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_core::timing;
use torro_ipc::{Door, Key, Request, Response, app_key_path, client, server, socket_path};
use torro_setups::storage::Store;

use crate::clients::{Clients, SystemKeychain};
use crate::core::{Core, Snapshot};
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use crate::tray;

/// The way back into the daemon for front ends like the tray menu.
pub struct Daemon {
    core: Arc<Core>,
    runtime: tokio::runtime::Handle,
    refresh: mpsc::UnboundedSender<()>,
    quit: mpsc::UnboundedSender<()>,
    /// Where a person's quit is marked ([`QUIT_MARK`]); `None` without a state directory.
    quit_mark: Option<PathBuf>,
}

impl Daemon {
    /// Runs `request` in the background and refreshes the menu before (to show "busy") and
    /// after. How it went comes as a notification, except an app or a dialog that opened: the
    /// window is the answer ([`shows_itself`]).
    pub fn run(&self, request: Request) {
        let core = Arc::clone(&self.core);
        let refresh = self.refresh.clone();
        let quit = self.quit.clone();
        let quit_mark = self.quit_mark.clone();
        self.runtime.spawn(async move {
            let _ = refresh.send(());
            run_from_tray(&core, request, &quit, quit_mark.as_deref(), report).await;
            let _ = refresh.send(());
        });
    }

    /// "Save workspace…" in the tray (task WS.7): saves the active workspace now and opens the dialog
    /// for the new template ([`Core::save_workspace`]), refreshing the menu before and after like
    /// [`Daemon::run`]. The dialog shows the result; only a failed save, or a save whose dialog could
    /// not start, comes as a notification.
    pub fn save_workspace(&self) {
        let core = Arc::clone(&self.core);
        let refresh = self.refresh.clone();
        self.runtime.spawn(async move {
            let what = describe(&Request::SaveTemplate { name: None });
            let _ = refresh.send(());
            let saved = core.save_workspace(&torro_setups::launch::SystemLauncher).await;
            match saved.dialog {
                true => log(&what, &saved.response),
                false => report(&what, &saved.response),
            }
            let _ = refresh.send(());
        });
    }

    /// "Manage workspaces…" in the tray (task WS.7): the control centre on its page Workspaces
    /// ([`Core::show_section`]). A failure is only logged, as for [`Daemon::open_app`].
    pub fn manage_workspaces(&self) {
        let core = Arc::clone(&self.core);
        self.runtime.spawn(async move {
            if let Err(err) = core.show_section(crate::ui::Section::Workspaces).await {
                eprintln!("torroworkplace: cannot open the workspaces page: {err}");
            }
        });
    }

    /// Tells the core how the template keys the tray registered stand (task WS.4), for
    /// `system.status` ([`Core::report_shortcuts`]).
    #[cfg(any(target_os = "macos", windows))]
    pub fn report_shortcuts(&self, report: crate::workspace_keys::Report) {
        self.core.report_shortcuts(report.active, report.problem);
    }

    /// Opens or focuses one of the suite's apps.
    pub fn open_app(&self, app: torro_core::AppId) {
        let core = Arc::clone(&self.core);
        self.runtime.spawn(async move {
            if let Err(err) = core.open_app(app).await {
                eprintln!("torroworkplace: cannot open {}: {err}", app.binary);
            }
        });
    }
}

/// How long a quit waits before it ends the service. The socket server writes the answer only
/// after the handler returned, so ending at once could cut the `Ok` off; the grace time lets it
/// go out first.
const ANSWER_GRACE: Duration = Duration::from_millis(200);

/// How long the start waits for an answer to "is a core already running?".
const START_PING_TIMEOUT: Duration = Duration::from_secs(2);

/// The marker a person's quit leaves in the core's state directory, next to the socket (issue #279,
/// decision 4 in `docs/lebenszyklus.md`): the time only, 0600. While it stands no MCP server starts
/// the core ([`stopped_by_person`]); the core's next start removes it. An end by a signal, by
/// `launchctl`/`systemctl stop` or by a crash leaves none.
pub const QUIT_MARK: &str = "quit";

/// Whether a person stopped the core at `socket` ([`QUIT_MARK`] stands next to it): an MCP server
/// then leaves it stopped. `false` without a state directory.
pub fn stopped_by_person(socket: &Path) -> bool {
    torro_gui::source::core_state_file(socket, QUIT_MARK).is_some_and(|mark| mark.is_file())
}

/// Removes the marker `mark`: a person's quit ([`QUIT_MARK`]) as the service starts, since whoever
/// started it wants it; the last try to start it ([`torro_gui::source::CORE_START_MARK`]) once its
/// socket answers ([`once_listening`]). A marker that cannot be removed is only logged: a quit marker
/// keeps nothing but MCP from starting the core, a start marker no start for longer than
/// [`torro_gui::source::CORE_START_GAP`].
fn remove_mark(mark: Option<&Path>) {
    let Some(mark) = mark else { return };
    match std::fs::remove_file(mark) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => eprintln!("torroworkplace: cannot remove {}: {err}", mark.display()),
    }
}

/// What the tray does with `request` ([`Daemon::run`]): runs it, tells how it went – `tell`
/// ([`report`] in the service), only logged for a window that opened ([`shows_itself`]) – and ends
/// the service after a quit ([`end_after_answer`]).
async fn run_from_tray(
    core: &Core,
    request: Request,
    quit: &mpsc::UnboundedSender<()>,
    quit_mark: Option<&Path>,
    tell: fn(&str, &Response),
) {
    let what = describe(&request);
    let ends = matches!(request, Request::Quit);
    let quiet = shows_itself(&request);
    let response = core.handle(request).await;
    match quiet && !matches!(response, Response::Error { .. }) {
        true => log(&what, &response),
        false => tell(&what, &response),
    }
    if ends && matches!(response, Response::Ok) {
        end_after_answer(quit.clone(), quit_mark);
    }
}

/// One request from the socket ([`serve`]): passes the key check ([`Core::handle_from`]), tells
/// about template commands on the desktop as the tray does, asks for a new menu, and ends the
/// service after a quit, which only the app key may send ([`end_after_answer`]).
async fn answer(
    core: Arc<Core>,
    key: Option<Key>,
    door: Door,
    request: Request,
    refresh: mpsc::UnboundedSender<()>,
    quit: mpsc::UnboundedSender<()>,
    quit_mark: Option<PathBuf>,
) -> Response {
    // Hotkeys go through `ctl`, so commands from the socket get the same
    // notifications as clicks in the tray.
    let changes = changes_state(&request).then(|| describe(&request));
    let notifies = tells_on_desktop(&request);
    // The tray on macOS and Windows registers the template keys with each new menu.
    let rekeys = crate::workspace_keys::rebinds_after(&request);
    let ends = matches!(request, Request::Quit);
    // A refused request ran nothing: no notification, no new menu, so a foreign
    // client cannot flood the desktop.
    let response = match core.handle_from(key.as_ref(), door, request).await {
        Ok(response) => response,
        Err(refused) => return refused.response(),
    };
    if ends && matches!(response, Response::Ok) {
        end_after_answer(quit, quit_mark.as_deref());
    }
    if let Some(what) = changes {
        if notifies {
            report(&what, &response);
        }
        let _ = refresh.send(());
    } else if rekeys {
        let _ = refresh.send(());
    }
    response
}

/// Ends the service once the answer to a `system.quit` is on its way (see [`ANSWER_GRACE`]). The one
/// place a person's quit passes, from the tray and from the socket alike, so it marks the quit here
/// ([`QUIT_MARK`] at `mark`) before anything ends. A marker that cannot be written is only logged:
/// the quit still ends the service.
fn end_after_answer(quit: mpsc::UnboundedSender<()>, mark: Option<&Path>) {
    if let Some(mark) = mark
        && let Err(err) = torro_gui::source::write_mark(mark, chrono::Utc::now())
    {
        eprintln!("torroworkplace: cannot write {}: {err}", mark.display());
    }
    tokio::spawn(async move {
        tokio::time::sleep(ANSWER_GRACE).await;
        let _ = quit.send(());
    });
}

/// Runs the service until it is told to quit. `headless` skips the tray icon.
pub fn run(headless: bool) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("torroworkplace: cannot start the async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };
    if headless {
        return finish(runtime.block_on(serve(Shows::Nothing)));
    }
    #[cfg(target_os = "linux")]
    {
        finish(runtime.block_on(serve(Shows::Ksni)))
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        run_with_event_loop(runtime)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        eprintln!("torroworkplace: no tray icon on this system; running headless");
        finish(runtime.block_on(serve(Shows::Nothing)))
    }
}

/// How long the main thread waits for the service once the event loop has ended. Normally the
/// loop ends because the service did ([`tray::DesktopHandle::exit`]), so the result is there
/// at once; if the system ended the loop instead, the service does not get to tidy up.
#[cfg(any(target_os = "macos", windows))]
const SERVICE_WAIT: Duration = Duration::from_secs(2);

/// macOS and Windows: the service as a task on the runtime, the tray's event loop on this (the
/// main) thread. Without an event loop the service runs headless.
#[cfg(any(target_os = "macos", windows))]
fn run_with_event_loop(runtime: tokio::runtime::Runtime) -> ExitCode {
    let shell = match tray::Shell::new() {
        Ok(shell) => shell,
        Err(err) => {
            eprintln!("torroworkplace: {err}; running headless");
            return finish(runtime.block_on(serve(Shows::Nothing)));
        }
    };
    let handle = shell.handle();
    let service = runtime.spawn(async move {
        let result = serve(Shows::Desktop(handle.clone())).await;
        handle.exit();
        result
    });
    if let Err(err) = shell.run() {
        // The service does not need the icon: keep it running until it is told to quit.
        eprintln!("torroworkplace: {err}; running headless");
        return finish(runtime.block_on(service).unwrap_or_else(|err| Err(format!("service task: {err}"))));
    }
    // The timer must be made inside the runtime, hence the async block.
    match runtime.block_on(async { tokio::time::timeout(SERVICE_WAIT, service).await }) {
        Ok(Ok(result)) => finish(result),
        Ok(Err(err)) => finish(Err(format!("service task: {err}"))),
        Err(_) => finish(Err("the tray event loop ended before the service".into())),
    }
}

/// The exit code for the service's result; the error goes to stderr.
fn finish(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("torroworkplace: {err}");
            ExitCode::FAILURE
        }
    }
}

/// What the service shows its state with, decided before it starts.
enum Shows {
    /// Nothing: `--headless`, or no tray on this system.
    Nothing,
    /// The ksni tray, started by [`serve`] on its runtime.
    #[cfg(target_os = "linux")]
    Ksni,
    /// The tray on the main thread's event loop, reached through its handle.
    #[cfg(any(target_os = "macos", windows))]
    Desktop(tray::DesktopHandle),
}

async fn serve(shows: Shows) -> Result<(), String> {
    // Before anything starts: two services started at once would both start the core (bind the
    // template keys, load the templates of the login) before one of them failed on the socket.
    let _service = claim_service()?;
    let socket = socket_path().ok_or("no home directory")?;
    // Bounded: a program that holds the socket or pipe name but never answers must not stall
    // the start; the server then fails on the taken name with a clear error instead.
    let ping = tokio::time::timeout(START_PING_TIMEOUT, client::call(&socket, &Request::Ping)).await;
    if let Ok(Ok(Response::Pong { .. })) = ping {
        return Err("already running".into());
    }
    // A person who quit and starts the service again (or logs in) wants it: MCP may start it again.
    let quit_mark = torro_gui::source::core_state_file(&socket, QUIT_MARK);
    remove_mark(quit_mark.as_deref());
    // The app key goes next to the socket, where `torro_ipc::client` looks for it.
    let config = AppPaths::for_app(AppId::WORKPLACE).ok_or("no home directory")?.config;
    // Reading or creating the files may wait for another core starting at the same time.
    let app_key = app_key_path(&socket);
    let clients = tokio::task::spawn_blocking(move || {
        Clients::open(&app_key, &config.join("clients.json"), Box::new(SystemKeychain))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(|err| err.to_string())?;
    // With `TORRO_TIMING`, each step before the socket listens (issue #206, `docs/tempo.md`).
    timing::mark("clients");
    let store = Store::open_default().map_err(|err| err.to_string())?;
    timing::mark("store");
    // The first start of the suite brings the example design packs (task L.5); later starts find
    // the folder and leave it alone.
    let _ = tokio::task::spawn_blocking(crate::designs::install_examples).await;
    timing::mark("designs");
    let backend = torro_wm::detect(|key| std::env::var(key).ok());
    let pim = Arc::new(crate::pim::PimService::new());
    pim.start();
    timing::mark("pim");
    let core = Arc::new(Core::new(backend, store, pim, clients));
    core.start();
    timing::mark("core");
    tokio::spawn(tend_inbox(Arc::clone(&core)));
    // Popups for new entries "for me" (task E3.4), also when headless: they need no tray.
    tokio::spawn(crate::inbox_notify::run(Arc::clone(&core), Arc::new(crate::inbox_notify::SystemDesktop)));
    // Reminders of events (tasks ER1.3, ER1.4): planned and fired here, filed in the inbox and shown
    // on the desktop.
    let desktop = Arc::new(crate::reminders::Desktop::new(&core));
    tokio::spawn(crate::reminders::run(Arc::clone(&core), desktop));

    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel();
    let (quit_tx, mut quit_rx) = mpsc::unbounded_channel();

    let mut ipc = {
        let core = Arc::clone(&core);
        let refresh = refresh_tx.clone();
        let quit = quit_tx.clone();
        let quit_mark = quit_mark.clone();
        let socket = socket.clone();
        tokio::spawn(async move {
            server::serve_with_key(&socket, move |key, door, request| {
                answer(Arc::clone(&core), key, door, request, refresh.clone(), quit.clone(), quit_mark.clone())
            })
            .await
        })
    };
    timing::mark("socket task started");

    let daemon = Arc::new(Daemon {
        core: Arc::clone(&core),
        runtime: tokio::runtime::Handle::current(),
        refresh: refresh_tx.clone(),
        quit: quit_tx,
        quit_mark,
    });
    // Taken before the first snapshot, so an approval opened or an entry posted in between still
    // refreshes the menu.
    let approval_changes = core.approval_changes();
    let inbox_changes = core.inbox().changes();
    // The tray shows whether the voice listener listens or records (task C.9).
    let voice_changes = core.voice_changes();
    let front = Front::start(&daemon, shows, core.snapshot().await).await?;
    // Once the socket answers (issue #279) the start marker goes, so a start after a quit is not held
    // back, and a first start without accounts opens the setup (task W.9); a headless core opens no
    // window. The setup starts a core of its own when it finds none, so a service that fails on its
    // socket must not open it.
    {
        let core = Arc::clone(&core);
        let socket = socket.clone();
        let start_mark = torro_gui::source::core_state_file(&socket, torro_gui::source::CORE_START_MARK);
        let setup = !matches!(front, Front::None);
        tokio::spawn(async move {
            let offer = async {
                if setup {
                    core.offer_setup().await;
                }
            };
            once_listening(&socket, SETUP_WAIT, start_mark.as_deref(), offer).await
        });
    }
    tokio::spawn(refresher(Arc::clone(&core), refresh_rx, front.clone()));
    tokio::spawn(watch_approvals(Arc::clone(&core), approval_changes, refresh_tx.clone()));
    tokio::spawn(watch_inbox(inbox_changes, refresh_tx.clone()));
    tokio::spawn(watch_inbox(voice_changes, refresh_tx.clone()));
    // "Next" and the pause of the reminders (task ER2.5).
    tokio::spawn(watch_reminders(core.reminders().changes(), refresh_tx.clone()));

    match core.events().await {
        Ok(mut events) => {
            let refresh = refresh_tx.clone();
            tokio::spawn(async move {
                while events.recv().await.is_some() {
                    let _ = refresh.send(());
                }
            });
        }
        Err(err) => eprintln!("torroworkplace: no window manager events: {err}"),
    }

    // Only now, with nothing left to wait for before the `select!`: during the start Ctrl+C and SIGTERM
    // end the process at once, as without a handler (the keys bound by then are written down, the next
    // start releases them); a handler installed earlier would hold them until the start is through.
    let stopped = stop_signals();
    let result = tokio::select! {
        _ = quit_rx.recv() => Ok(()),
        () = stopped => Ok(()),
        ended = &mut ipc => match ended {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err.to_string()),
            Err(err) => Err(format!("socket task: {err}")),
        },
    };
    // The socket takes no more requests before the keys go: a change that came in now would bind its
    // key again. A request already being answered may still finish; after the release it binds nothing
    // (`BoundKeys::release`), and neither does a reload of Hyprland.
    ipc.abort();
    // Bounded: a window manager that does not answer must not keep the service from ending; what
    // was not released by then stays written down for the next core.
    if tokio::time::timeout(RELEASE_WAIT, core.release_template_keys()).await.is_err() {
        eprintln!("torroworkplace: the template keys were not released in time; the next start releases them");
    }
    // The MCP servers (task MA.4) go with the service, each with everything it started. Bounded too: each gets
    // its grace time, then its tree is killed; a tree that is not killed by then dies with this process at the
    // latest through its kill-on-drop.
    let servers = crate::extensions::supervisor::Supervisor::service();
    if tokio::time::timeout(EXTENSIONS_WAIT, servers.stop_all()).await.is_err() {
        eprintln!("torroworkplace: the MCP servers did not end in time");
    }
    let _ = std::fs::remove_file(&socket);
    result
}

/// The file in the core's state directory that the running service holds locked ([`claim_service`]).
const SERVICE_LOCK: &str = "service.lock";

/// How long the end of the service waits for the template keys to be released.
const RELEASE_WAIT: Duration = Duration::from_secs(2);

/// How long the end of the service waits for its MCP servers: their grace time of 3 s and a little.
const EXTENSIONS_WAIT: Duration = Duration::from_secs(5);

/// How long the start waits for its own socket to answer before it gives up on the setup
/// ([`once_listening`]).
const SETUP_WAIT: Duration = Duration::from_secs(10);

/// Once the core at `socket` answers `system.ping` within `limit` (issue #279): removes the marker of
/// the last try to start it at `start_mark` ([`remove_mark`]), then runs `then`; `true`. A socket that
/// does not answer in time does neither: `false`, with a line on stderr. The marker then stays, so the
/// apps do not start a failing core over and over.
async fn once_listening(
    socket: &Path,
    limit: Duration,
    start_mark: Option<&Path>,
    then: impl Future<Output = ()>,
) -> bool {
    if !torro_gui::source::wait_for_core(socket, limit).await {
        eprintln!("torroworkplace: the socket does not answer; not opening the setup");
        return false;
    }
    remove_mark(start_mark);
    then.await;
    true
}

/// Resolves when the service is told from outside to end: Ctrl+C (SIGINT on Unix) on every system and,
/// on Unix, SIGTERM, which `systemctl --user stop` and `kill` send. Both then take the same way out as
/// "Quit": the socket stops and the template keys are released ([`serve`]).
///
/// Both handlers are installed by the call, not when the future is first polled (as
/// `tokio::signal::ctrl_c` would), so a signal between the call and the first poll is kept, not deadly.
/// Until the call both signals end the process the default way, at once; tokio never takes a handler
/// back, so from the call on they no longer do. [`serve`] therefore calls it only once the service
/// runs, right before it waits. Must be called inside the runtime. A handler that cannot be installed
/// is logged, and that signal keeps ending the process at once.
fn stop_signals() -> impl Future<Output = ()> + Send + 'static {
    #[cfg(unix)]
    {
        use tokio::signal::unix::SignalKind;
        let interrupted = on_signal(SignalKind::interrupt(), "Ctrl+C");
        let terminated = on_signal(SignalKind::terminate(), "SIGTERM");
        async move {
            tokio::select! {
                () = interrupted => {}
                () = terminated => {}
            }
        }
    }
    #[cfg(windows)]
    {
        let ctrl_c = tokio::signal::windows::ctrl_c()
            .inspect_err(|err| {
                eprintln!("torroworkplace: cannot listen for Ctrl+C: {err}; it ends the service at once")
            })
            .ok();
        async move {
            match ctrl_c {
                Some(mut ctrl_c) => {
                    ctrl_c.recv().await;
                }
                None => std::future::pending().await,
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::future::pending::<()>()
    }
}

/// Unix: resolves on the signal `kind` (`name` for the log), with the handler installed by the call;
/// never when it cannot be installed (logged), so that signal keeps its default ([`stop_signals`]).
#[cfg(unix)]
fn on_signal(kind: tokio::signal::unix::SignalKind, name: &str) -> impl Future<Output = ()> + Send + 'static {
    let signal = tokio::signal::unix::signal(kind)
        .inspect_err(|err| eprintln!("torroworkplace: cannot listen for {name}: {err}; it ends the service at once"))
        .ok();
    async move {
        match signal {
            Some(mut signal) => {
                signal.recv().await;
            }
            None => std::future::pending().await,
        }
    }
}

/// Makes this process the one service of the user: locks [`SERVICE_LOCK`] for as long as the
/// returned file lives (the system drops the lock when the process ends, also when it is killed).
/// "already running" while another service holds it.
///
/// The socket alone cannot tell: it is bound only after the core started (the server binds it when
/// it starts serving), and a second service started at the same time would get that far as well. A
/// file system without locks is only logged; then the socket's check is all there is, as before.
fn claim_service() -> Result<Option<std::fs::File>, String> {
    let state = AppPaths::for_app(AppId::WORKPLACE).ok_or("no home directory")?.state;
    let path = state.join(SERVICE_LOCK);
    let opened = std::fs::create_dir_all(&state)
        .and_then(|()| std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path));
    let file = match opened {
        Ok(file) => file,
        Err(err) => {
            eprintln!("torroworkplace: cannot open {}: {err}; not checking for a second service", path.display());
            return Ok(None);
        }
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Err("already running".into()),
        Err(std::fs::TryLockError::Error(err)) => {
            eprintln!("torroworkplace: cannot lock {}: {err}; not checking for a second service", path.display());
            Ok(None)
        }
    }
}

/// What shows the state to the user: the tray icon, or nothing when headless or without a
/// tray on this system.
#[derive(Clone)]
enum Front {
    #[cfg(target_os = "linux")]
    Tray(tray::TrayHandle),
    #[cfg(any(target_os = "macos", windows))]
    Desktop(tray::DesktopHandle),
    None,
}

impl Front {
    /// Shows `snapshot` first; on macOS and Windows with the template keys ([`template_keys`]).
    async fn start(daemon: &Arc<Daemon>, shows: Shows, snapshot: Snapshot) -> Result<Front, String> {
        match shows {
            Shows::Nothing => {
                let _ = (daemon, snapshot);
                Ok(Front::None)
            }
            #[cfg(target_os = "linux")]
            Shows::Ksni => {
                let handle = tray::spawn(Arc::clone(daemon), snapshot).await.map_err(|err| err.to_string())?;
                Ok(Front::Tray(handle))
            }
            #[cfg(any(target_os = "macos", windows))]
            Shows::Desktop(handle) => {
                let keys = template_keys(&daemon.core, &snapshot).await;
                handle.start(Arc::clone(daemon), snapshot, keys);
                Ok(Front::Desktop(handle))
            }
        }
    }

    /// Shows `snapshot`; on macOS and Windows with the template keys `core` names now.
    async fn show(&self, core: &Core, snapshot: Snapshot) {
        match self {
            #[cfg(target_os = "linux")]
            Front::Tray(handle) => {
                let _ = core;
                handle.update(|tray| tray.snapshot = snapshot).await;
            }
            #[cfg(any(target_os = "macos", windows))]
            Front::Desktop(handle) => {
                let keys = template_keys(core, &snapshot).await;
                handle.show(snapshot, keys);
            }
            Front::None => {
                let _ = (core, snapshot);
            }
        }
    }
}

/// The template keys the tray on macOS and Windows registers (task WS.4): the keys of the templates in
/// `snapshot`, held with the modifiers of the settings. `None` without a window manager to load them
/// (`Snapshot::unsupported`), so the keys stay free for other programs. Unreadable settings give the
/// system's default modifiers, as everywhere else in the core.
///
/// When the templates could not be read, `snapshot` lists none and the keys are released until the
/// next menu finds them again.
#[cfg(any(target_os = "macos", windows))]
async fn template_keys(core: &Core, snapshot: &Snapshot) -> Option<crate::workspace_keys::TemplateKeys> {
    if snapshot.unsupported.is_some() {
        return None;
    }
    let modifiers = match core.handle(Request::Settings).await {
        Response::Settings { settings } => settings.workspaces.modifiers,
        _ => torro_core::settings::ShortcutModifiers::default(),
    };
    let slots = crate::workspace_keys::summary_slots(&snapshot.templates);
    Some(crate::workspace_keys::TemplateKeys { modifiers, slots })
}

/// Rebuilds the menu when asked; bursts of requests (a load opens many windows) collapse
/// into one refresh.
async fn refresher(core: Arc<Core>, mut requests: mpsc::UnboundedReceiver<()>, front: Front) {
    while requests.recv().await.is_some() {
        tokio::time::sleep(Duration::from_millis(150)).await;
        while requests.try_recv().is_ok() {}
        front.show(&core, core.snapshot().await).await;
    }
}

/// Asks for a new menu whenever the count of open approvals may have changed: one was opened (by a
/// client from the socket) or decided (control centre, `ctl`, notification), or one expired by the
/// clock ([`Core::approvals_changed`]). Their inbox entries (task E2.1) mostly tell the tray
/// themselves ([`watch_inbox`]); this keeps [`Snapshot::approvals_waiting`] right to the second.
async fn watch_approvals(
    core: Arc<Core>,
    mut changes: tokio::sync::watch::Receiver<()>,
    refresh: mpsc::UnboundedSender<()>,
) {
    while core.approvals_changed(&mut changes).await {
        if refresh.send(()).is_err() {
            return;
        }
    }
}

/// Asks for a new menu whenever something was written to the inbox (task E3.3b,
/// [`crate::inbox::Inbox::changes`]): a post from a client, a change in the Inbox app, waking, the
/// clean-up, the suite's approvals and account problems. So "Inbox (n)" follows without polling;
/// the refresher collapses bursts into one new menu.
async fn watch_inbox(mut changes: tokio::sync::watch::Receiver<()>, refresh: mpsc::UnboundedSender<()>) {
    while changes.changed().await.is_ok() {
        if refresh.send(()).is_err() {
            return;
        }
    }
}

/// Refreshes the tray after every change of the reminders: one fired, was dismissed, the pause.
async fn watch_reminders(mut changes: tokio::sync::watch::Receiver<u64>, refresh: mpsc::UnboundedSender<()>) {
    while changes.changed().await.is_ok() {
        if refresh.send(()).is_err() {
            return;
        }
    }
}

/// How often the inbox wakes snoozed entries that are due. The apps poll at about the same pace,
/// and listing wakes entries too, so a minute is late enough.
const INBOX_WAKE_EVERY: Duration = Duration::from_secs(60);

/// Wakes snoozed inbox entries every [`INBOX_WAKE_EVERY`] and removes the ones done 30 days ago
/// once a day, the first time right at start. A failure is logged once until the next success,
/// not every minute.
async fn tend_inbox(core: Arc<Core>) {
    let mut cleaned: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut failing = false;
    loop {
        let now = chrono::Utc::now();
        let clean_up = cleaned.is_none_or(|at| now - at >= chrono::TimeDelta::days(1));
        match core.inbox().tend(now, clean_up) {
            Ok((_, removed)) => {
                if clean_up {
                    cleaned = Some(now);
                    if removed > 0 {
                        eprintln!("torroworkplace: removed {removed} inbox entries done 30 days ago");
                    }
                }
                failing = false;
            }
            Err(err) if !failing => {
                eprintln!("torroworkplace: inbox: {err}");
                failing = true;
            }
            Err(_) => {}
        }
        tokio::time::sleep(INBOX_WAKE_EVERY).await;
    }
}

/// Template commands: they change the menu, and most get a notification ([`tells_on_desktop`]).
/// Calendar and task requests from the apps get neither; the apps show their own feedback. A
/// template's key and startup (issue #243) change the menu too: it shows the key.
fn changes_state(request: &Request) -> bool {
    matches!(
        request,
        Request::SaveTemplate { .. }
            | Request::UpdateTemplate { .. }
            | Request::DeleteTemplate { .. }
            | Request::LoadTemplate { .. }
            | Request::SetTemplateShortcut { .. }
            | Request::SetTemplateStartup { .. }
    )
}

/// Of [`changes_state`], the commands whose result comes as a desktop notification: what the tray
/// and the keys start. A key and a startup are set in the core's dialog, which shows its own
/// result; a notification ("Template saved") after each of its saves would only repeat it.
fn tells_on_desktop(request: &Request) -> bool {
    changes_state(request)
        && !matches!(request, Request::SetTemplateShortcut { .. } | Request::SetTemplateStartup { .. })
}

/// A short name for a request, for messages.
fn describe(request: &Request) -> String {
    match request {
        Request::Ping => "ping".into(),
        Request::Status => "status".into(),
        Request::Quit => "quit".into(),
        Request::ListTemplates => "list".into(),
        Request::SaveTemplate { .. } => "save".into(),
        Request::UpdateTemplate { id } => format!("update {id}"),
        Request::DeleteTemplate { id } => format!("delete {id}"),
        Request::LoadTemplate { id, .. } => format!("load {id}"),
        Request::ConnectNextcloud { server } => format!("connect {server}"),
        Request::ConnectGithub => "connect github".into(),
        // Never the category: the log and the notification name the entry only (task E12.4).
        Request::AiAssignments => "list ai assignments".into(),
        Request::SetAiAssignment { function, engine, .. } => match engine {
            Some(engine) => format!("use {engine} for {}", function.key()),
            None => format!("use no model for {}", function.key()),
        },
        Request::SetAiSlot { slot, engine, .. } => match engine {
            Some(engine) => format!("use {engine} for the slot {}", slot.key()),
            None => format!("use no model for the slot {}", slot.key()),
        },
        Request::AiRecommend => "recommend ai models".into(),
        Request::PullAiModel { account, model } => format!("download {model} to {account}"),
        Request::AiPulls => "list model downloads".into(),
        Request::CancelAiPull { id } => format!("cancel the download {id}"),
        Request::ListCategories { .. } => "list categories".into(),
        Request::SetItemCategory { id, remember, .. } => {
            format!("file {id}{}", if *remember { " and remember" } else { "" })
        }
        Request::ClassifyItem { id } => format!("classify {id}"),
        Request::GetCategorization => "categorization".into(),
        Request::CatchUpCategories { cancel, again } => {
            if *cancel {
                "stop sorting mail".into()
            } else if *again {
                "sort mail again".into()
            } else {
                "sort the mail backlog".into()
            }
        }
        Request::PreviewCategoryJob { .. } => "preview mail sorting".into(),
        Request::StartCategoryJob { .. } => "start mail sorting job".into(),
        Request::GetCategoryJob { .. } => "mail sorting job".into(),
        Request::ControlCategoryJob { id, action } => format!("{action:?} mail sorting job {id}"),
        Request::PreviewClearCategories => "preview removing mail categories".into(),
        Request::ClearCategories { .. } => "remove all mail categories".into(),
        Request::ControlClearCategories { id, action } => format!("{action:?} category reset {id}"),
        // Never a name, a description or a condition (task E15.1).
        Request::CreateCategory { .. } => "create a mail category".into(),
        Request::CreateCategoryGroup { .. } => "create a main mail category".into(),
        Request::UpdateCategoryGroup { .. } => "change a main mail category".into(),
        Request::UpdateCategory { .. } => "change a mail category".into(),
        Request::RemoveCategory { .. } => "remove a mail category".into(),
        Request::ListSortRules => "list sorting rules".into(),
        Request::CreateSortRule { .. } => "create a sorting rule".into(),
        Request::UpdateSortRule { id, .. } => format!("change sorting rule {id}"),
        Request::RemoveSortRule { id } => format!("remove sorting rule {id}"),
        Request::MoveSortRule { id, position } => format!("move sorting rule {id} to {position}"),
        // Never the token.
        Request::SetGithubToken { .. } => "set github token".into(),
        // Never the address or the password (task M7.1).
        Request::DiscoverServices { .. } => "discover services".into(),
        Request::ConnectCalDav { .. } => "connect caldav".into(),
        Request::Sync => "sync".into(),
        Request::CalendarCollections => "calendars".into(),
        Request::TaskCollections => "task lists".into(),
        Request::Events { .. } => "events".into(),
        Request::Todos => "todos".into(),
        Request::SaveEvent { .. } => "save event".into(),
        Request::DeleteEvent { .. } => "delete event".into(),
        // Never the query or the point (task TE.1).
        Request::SearchPlaces { .. } => "search places".into(),
        Request::MapPreview { .. } => "map preview".into(),
        Request::ListReminders { .. } => "reminders".into(),
        Request::SnoozeReminder { .. } => "snooze reminder".into(),
        Request::DismissReminder { .. } => "dismiss reminder".into(),
        Request::PauseReminders { until: Some(_) } => "pause reminders".into(),
        Request::PauseReminders { until: None } => "resume reminders".into(),
        Request::PreviewReminder { .. } => "preview reminder".into(),
        Request::SaveTodo { .. } => "save todo".into(),
        Request::DeleteTodo { .. } => "delete todo".into(),
        Request::MoveTodo { .. } => "move todo".into(),
        // The list only: never the mail account, the mail's id or its Message-ID (task E8.7a).
        Request::CreateTodoFromMail { collection, .. } => format!("make a task in {collection} of a mail"),
        // Ids only, never a search or a name (tasks G.2, G.7).
        Request::CreateTaskList { account, .. } => format!("create task list in {account}"),
        Request::CreateFilterList { account, .. } => format!("create filter list of {account}"),
        Request::UpdateFilterList { collection, .. } => format!("update filter list {collection}"),
        Request::DeleteFilterList { collection } => format!("delete filter list {collection}"),
        Request::PreviewFilter { account, .. } => format!("preview filter of {account}"),
        Request::ListGithubRepos { account } => format!("list github repos of {account}"),
        Request::DescribeGithubRepo { account, .. } => format!("describe a github repo of {account}"),
        Request::UnpinIssue { collection, .. } => format!("unpin from {collection}"),
        Request::AddLink { collection, .. } => format!("add link to {collection}"),
        Request::Settings => "settings".into(),
        Request::UpdateSettings { .. } => "update settings".into(),
        Request::SetSetting { key, .. } => format!("set {key:?}"),
        Request::SetCollectionShown { collection, shown } => {
            format!("{} {collection}", if *shown { "show" } else { "hide" })
        }
        Request::Accounts => "accounts".into(),
        Request::RemoveAccount { id } => format!("remove account {id}"),
        Request::RenameTemplate { id, .. } => format!("rename {id}"),
        Request::SetTemplateShortcut { id, key: Some(key) } => format!("key {key} for {id}"),
        Request::SetTemplateShortcut { id, key: None } => format!("no key for {id}"),
        // Never the screen: the name of a monitor says little, but it is the client's text.
        Request::SetTemplateStartup { id, startup: Some(_) } => format!("load {id} at login"),
        Request::SetTemplateStartup { id, startup: None } => format!("load {id} not at login"),
        Request::ListScreens => "screens".into(),
        // The app only, never the target: it names a mail or a task (task E8.7a).
        Request::OpenApp { app, .. } => format!("open {app}"),
        Request::ListApps => "apps".into(),
        Request::ListDesigns => "designs".into(),
        Request::ListFonts => "fonts".into(),
        Request::ListColorSchemes => "colour schemes".into(),
        Request::ListClients => "clients".into(),
        Request::CreateClient { .. } => "create client".into(),
        Request::UpdateClient { id, .. } => format!("update client {id}"),
        Request::NewClientKey { id } => format!("new key for client {id}"),
        Request::RemoveClient { id } => format!("remove client {id}"),
        Request::ClientActivity { id } => format!("activity of client {id}"),
        Request::ListApprovals => "approvals".into(),
        Request::ApprovalStatus { id } => format!("approval {id}"),
        Request::DecideApproval { id, allow } => format!("{} approval {id}", if *allow { "allow" } else { "deny" }),
        // Never the tool or the command: they are what the agent would run.
        Request::AskPermission { .. } => "ask for permission".into(),
        // Ids only, never a title, a body or a thread key (security rule 7 of the inbox).
        Request::ListItems { .. } => "inbox items".into(),
        Request::GetItem { id } => format!("inbox item {id}"),
        Request::OpenItem { id } => format!("open inbox item {id}"),
        Request::CreateTaskFromItem { id, collection, .. } => format!("make a task in {collection} of inbox item {id}"),
        // Never the title or the times (task E8.5b).
        Request::CreateEventFromItem { id, collection, .. } => {
            format!("make an event in {collection} of inbox item {id}")
        }
        // Never what the AI suggested (issue #120).
        Request::SuggestTaskFromItem { id, .. } => format!("suggest a task of inbox item {id}"),
        Request::SuggestEventFromItem { id } => format!("suggest an event of inbox item {id}"),
        Request::ListSources => "inbox sources".into(),
        Request::PostItem { .. } => "post to inbox".into(),
        Request::ResolveItems { .. } => "resolve inbox items".into(),
        Request::SetItemDone { id, .. } => format!("set inbox item {id} done"),
        Request::SnoozeItem { id, .. } => format!("snooze inbox item {id}"),
        Request::SetItemLane { id, .. } => format!("move inbox item {id}"),
        Request::MuteSource { source, muted } => format!("{} {source}", if *muted { "mute" } else { "unmute" }),
        // Never the trait or its value: they would name who writes to the person.
        Request::ListMuteOptions { id } => format!("mute options of inbox item {id}"),
        Request::MuteItem { id, .. } => format!("mute by inbox item {id}"),
        // Only the count: the ids would let lines be matched with a mailbox (task E19.2).
        Request::SetMailPile { ids, pile: Some(_), .. } => format!("put {} mails on a pile", ids.len()),
        Request::SetMailPile { ids, pile: None, .. } => format!("take {} mails off their pile", ids.len()),
        Request::ListRules => "inbox rules".into(),
        Request::GetStats { .. } => "inbox statistics".into(),
        Request::GetQuiet => "inbox quiet".into(),
        Request::RemoveRule { id } => format!("remove inbox rule {id}"),
        // Never an address, a password, a search, a subject or a folder name (security rule 3 of
        // task M2.1); message and folder ids would let lines be matched with a mailbox.
        Request::ConnectMail { .. } => "connect mail".into(),
        Request::ReconfigureMail { .. } => "check or change mail connection".into(),
        Request::UpdateMailAccount { .. } => "update mail account".into(),
        Request::TorroMailAccounts => "torromail accounts".into(),
        Request::ImportTorroMailAccount { .. } => "import torromail account".into(),
        // Never the address or the key (task KI.1a).
        Request::ConnectAi { .. } => "connect ai".into(),
        // Never a topic: on a public server its name is a password (task E5.12).
        Request::ConnectNtfy { .. } => "connect ntfy".into(),
        Request::SetNtfyLogin { .. } => "set ntfy login".into(),
        Request::SetNtfyTopic { account, .. } => format!("set a topic of {account}"),
        Request::AiModels { .. } => "ai models".into(),
        // Never the model's name: the log names the account and the switch only (task S.24).
        Request::SetModelEnabled { account, enabled: true, .. } => format!("turn on a model of {account}"),
        Request::SetModelEnabled { account, enabled: false, .. } => format!("turn off a model of {account}"),
        Request::FindAiServers => "find ai servers".into(),
        Request::MailFolders { .. } => "mail folders".into(),
        Request::MailAccountStats { .. } => "mail account statistics".into(),
        Request::MailSyncStatus { .. } => "mail sync status".into(),
        Request::MailOverview { .. } => "mail overview".into(),
        // Never a selected archive path, job id or target account address (#267).
        Request::MailBackupCreate { .. } => "create mail backup".into(),
        Request::MailBackupStatus { .. } => "mail backup status".into(),
        Request::MailBackupCancel { .. } => "cancel mail backup".into(),
        Request::MailBackupVerify { .. } => "verify mail backup".into(),
        Request::MailRestorePlan { .. } => "plan mail restore".into(),
        Request::MailRestoreApply { .. } => "apply mail restore".into(),
        Request::SearchMail { .. } => "search mail".into(),
        Request::MailIndexStatus { .. } => "mail index status".into(),
        Request::GetMail { .. } => "get mail".into(),
        Request::GetMailThread { .. } => "get mail thread".into(),
        Request::GetMailAttachment { .. } => "get mail attachment".into(),
        // Never a target's name, a file name or a path (issue #290).
        Request::ListSaveTargets { .. } => "list save targets".into(),
        Request::CreateSaveTarget { .. } => "create save target".into(),
        Request::UpdateSaveTarget { id, .. } => format!("change save target {id}"),
        Request::RemoveSaveTarget { id } => format!("remove save target {id}"),
        Request::SaveMailAttachments { parts, .. } if parts.is_empty() => "save all mail attachments".into(),
        Request::SaveMailAttachments { parts, .. } => format!("save {} mail attachments", parts.len()),
        // Never a filter's search, account or folder (issue #339).
        Request::ListMailFilters => "list mail filters".into(),
        Request::CreateMailFilter { .. } => "create a mail filter".into(),
        Request::UpdateMailFilter { id, .. } => format!("change mail filter {id}"),
        Request::RemoveMailFilter { id } => format!("remove mail filter {id}"),
        Request::PreviewMailFilter { .. } => "preview a mail filter".into(),
        Request::ApplyMailFilter { id } => format!("apply mail filter {id}"),
        Request::SetMailFlags { ids, .. } => format!("set flags of {} messages", ids.len()),
        Request::MoveMail { ids, .. } => format!("move {} messages", ids.len()),
        Request::DeleteMail { ids, .. } => format!("delete {} messages", ids.len()),
        Request::SaveDraft { .. } => "save draft".into(),
        Request::SendDraft { .. } => "send draft".into(),
        // Never the answer, the account or the mail (task E8.3b).
        Request::RespondToInvitation { .. } => "answer an invitation".into(),
        // Ids only, never a message or a title (task C.1).
        Request::ListEngines => "assistant engines".into(),
        Request::ListConversations => "assistant conversations".into(),
        Request::GetConversation { id, .. } => format!("conversation {id}"),
        // Never a file's title or path (issue #225).
        Request::GetCards { conversation } => format!("cards of conversation {conversation}"),
        Request::SendMessage { .. } => "message to the assistant".into(),
        Request::CancelAnswer { conversation } => format!("stop the answer in conversation {conversation}"),
        Request::RenameConversation { id, .. } => format!("rename conversation {id}"),
        Request::DeleteConversation { id } => format!("delete conversation {id}"),
        Request::PinConversation { id, pinned } => {
            format!("{} conversation {id}", if *pinned { "pin" } else { "unpin" })
        }
        Request::OpenChatWindow { .. } => "open an assistant window".into(),
        // Ids only, never a note or the personality (task C.10).
        Request::GetProfile => "assistant profile".into(),
        Request::UpdateProfile { .. } => "change the assistant profile".into(),
        Request::ListMemories => "assistant memory".into(),
        Request::MyAccess => "own access".into(),
        Request::Remember { replaces: Some(id), .. } => format!("change note {id}"),
        Request::Remember { .. } => "remember a note".into(),
        Request::Forget { id } => format!("forget note {id}"),
        // Ids only, never a bot's name or texts (task B.1).
        Request::ListBots => "bots".into(),
        Request::GetBot { id } => format!("bot {id}"),
        Request::SaveBot { id: Some(id), .. } => format!("change bot {id}"),
        Request::SaveBot { .. } => "make a bot".into(),
        Request::DeleteBot { id } => format!("delete bot {id}"),
        Request::AskBot { bot, .. } => format!("message to bot {bot}"),
        Request::ConfirmBot { bot, .. } => format!("confirm asking bot {bot}"),
        Request::VoiceStatus => "voice status".into(),
        Request::TestVoice => "voice test".into(),
        // Ids only, never a title, the content or a path (issue #225).
        Request::ListTools => "tools".into(),
        Request::CreatePdf { .. } => "make a PDF".into(),
        Request::OpenFile { file } => format!("open file {file}"),
        Request::SaveFileAs { file, .. } => format!("save file {file} elsewhere"),
        // MCP servers (issue #302): the server and the tool, never arguments, commands or secrets.
        Request::ListExtensions => "mcp servers".into(),
        Request::GetExtension { id } => format!("mcp server {id}"),
        Request::ListCatalog => "mcp catalog".into(),
        Request::ListExtensionTools => "mcp server tools".into(),
        Request::CallExtensionTool { extension, tool, .. } => format!("call {tool} of mcp server {extension}"),
        Request::SetExtensionEnabled { id, enabled: true } => format!("switch on mcp server {id}"),
        Request::SetExtensionEnabled { id, enabled: false } => format!("switch off mcp server {id}"),
        Request::InstallExtension { from: torro_ipc::extensions::InstallFrom::Catalog { entry, .. } } => {
            format!("install mcp server {entry} from the catalog")
        }
        Request::InstallExtension { from: torro_ipc::extensions::InstallFrom::Custom { id, .. } } => {
            format!("install own mcp server {id}")
        }
        Request::ConfirmExtensionTools { id, .. } => format!("allow tools of mcp server {id}"),
        Request::UpdateExtension { id, .. } => format!("update mcp server {id}"),
        Request::SetExtensionSecret { id, .. } => format!("secret of mcp server {id}"),
        Request::SetExtensionFolder { id, .. } => format!("choose folder of mcp server {id}"),
        Request::ReadExtensionLogs { id } => format!("output of mcp server {id}"),
        Request::RemoveExtension { id } => format!("remove mcp server {id}"),
        // Never a path or a name searched for (issue #304).
        Request::ListFolder { .. } => "list a folder".into(),
        Request::FindFiles { .. } => "find files".into(),
        Request::CreateFolder { .. } => "make a folder".into(),
        Request::CopyFile { .. } => "copy a file".into(),
        Request::MoveFile { .. } => "move a file".into(),
        Request::TrashFile { .. } => "trash a file".into(),
        // Never a name, a search, a group or a field: contacts are people (`docs/kontakte.md`).
        Request::ListAddressBooks { .. } => "list address books".into(),
        Request::ListContacts { .. } => "list contacts".into(),
        Request::GetContact { .. } => "read a contact".into(),
        Request::GetContactPhoto { .. } => "read a contact's picture".into(),
        Request::ListContactGroups { .. } => "list contact groups".into(),
        Request::UpcomingBirthdays { .. } => "list birthdays".into(),
        Request::CreateContact { .. } => "create a contact".into(),
        Request::AddToContact { .. } => "add to a contact".into(),
        Request::UpdateContact { .. } => "change a contact".into(),
        Request::SetContactFavorite { .. } => "mark a favourite".into(),
        Request::AddContactToGroup { .. } => "add a contact to a group".into(),
        Request::RemoveContactFromGroup { .. } => "take a contact out of a group".into(),
        Request::MarkContactUsed { .. } => "note a contact as used".into(),
        Request::SetContactPhoto { .. } => "change a contact's picture".into(),
        Request::MoveContact { .. } => "move a contact".into(),
        Request::DeleteContact { .. } => "delete a contact".into(),
        Request::ResolveContactConflict { .. } => "settle a contact's conflict".into(),
        // Never the search words or the address (issue #305), never a key.
        Request::WebSearch { .. } => "search the web".into(),
        Request::FetchPage { .. } => "read a web page".into(),
        Request::ListSearchProviders => "search providers".into(),
        Request::SetSearchKey { provider, .. } => format!("set the key of {}", provider.name()),
        Request::GetMailCachePolicy { account } => format!("cache policy of {account}"),
        Request::SetMailCachePolicy { policy } => format!("set cache policy of {}", policy.account),
        Request::PreviewMailCache { account, .. } => format!("cache estimate of {account}"),
        Request::StartMailCacheJob { account, .. } => format!("start cache job for {account}"),
        Request::MailCacheJobStatus { account } => format!("cache job of {account}"),
        Request::ControlMailCacheJob { account, .. } => format!("control cache job of {account}"),
    }
}

/// Requests of the tray whose window is the answer (task WS.7): an app, or the template dialog
/// ("Edit…"), that opened. A notification "Done" on top would only repeat it; a failure still comes.
fn shows_itself(request: &Request) -> bool {
    matches!(request, Request::OpenApp { .. })
}

/// Tells the user how a tray command went: on stderr ([`log`]), and as a desktop notification.
fn report(what: &str, response: &Response) {
    log(what, response);
    if let Some((summary, body)) = crate::notify::message(what, response) {
        crate::notify::show(summary, body);
    }
}

/// How a tray command went, on stderr only.
fn log(what: &str, response: &Response) {
    match response {
        Response::Error { message, .. } => eprintln!("torroworkplace: {what} failed: {message}"),
        Response::Loaded { restored, failed } => {
            eprintln!("torroworkplace: {what}: {restored} windows restored, {} failed", failed.len());
            for (window, reason) in failed {
                eprintln!("  {window}: {reason}");
            }
        }
        Response::Saved { template } => eprintln!("torroworkplace: saved {:?} ({})", template.name, template.id),
        Response::Pending { approval } => eprintln!("torroworkplace: {what}: waits for approval {}", approval.id),
        _ => eprintln!("torroworkplace: {what}: ok"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A core over `dir` with nothing of the user's: keys, approvals and inbox in `dir`, no window
    /// manager, no notifications.
    fn core_in(dir: &Path) -> Arc<Core> {
        let settings = crate::settings_service::SettingsStore::at(dir.join("config").join("settings.json"));
        Arc::new(
            Core::new(
                torro_wm::Backend::Unsupported { name: "test".into() },
                Store::at(dir.to_path_buf()),
                Arc::new(crate::pim::PimService::with_settings(settings)),
                crate::clients::tests::open_in(dir),
            )
            .with_approvals(crate::approvals::tests::quiet_in(dir))
            .with_inbox(crate::inbox::tests::inbox_in(dir)),
        )
    }

    /// The marker holds the time only and is the owner's alone.
    fn assert_marked(mark: &Path) {
        assert!(torro_gui::source::read_mark(mark).is_some(), "{} holds a time", mark.display());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(mark).expect("marked").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    /// Issue #279: tray › Quit marks the quit and ends the service.
    #[tokio::test]
    async fn a_quit_from_the_tray_is_marked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core_in(dir.path());
        let mark = dir.path().join("state").join(QUIT_MARK);
        let (quit, mut ended) = mpsc::unbounded_channel();

        run_from_tray(&core, Request::Quit, &quit, Some(&mark), |_, _| {}).await;
        assert_marked(&mark);
        assert!(stopped_by_person(&dir.path().join("state").join("core.sock")));
        let end = tokio::time::timeout(Duration::from_secs(5), ended.recv()).await;
        assert_eq!(end, Ok(Some(())), "the service ends");

        // Anything else from the tray marks nothing.
        let other = tempfile::tempdir().expect("temp dir");
        let mark = other.path().join(QUIT_MARK);
        run_from_tray(&core_in(other.path()), Request::Status, &quit, Some(&mark), |_, _| {}).await;
        assert!(!mark.exists());
    }

    /// Issue #279: `ctl quit` (the app key over the socket) marks the quit; a request without the key
    /// is refused and marks nothing.
    #[tokio::test]
    async fn a_quit_over_the_socket_is_marked_with_the_app_key_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core_in(dir.path());
        let mark = dir.path().join("state").join(QUIT_MARK);
        let (refresh, _menu) = mpsc::unbounded_channel();
        let (quit, mut ended) = mpsc::unbounded_channel();
        let ask = |key: Option<Key>| {
            answer(Arc::clone(&core), key, Door::Api, Request::Quit, refresh.clone(), quit.clone(), Some(mark.clone()))
        };

        let refused = ask(None).await;
        assert!(matches!(&refused, Response::Error { code, .. } if code == "unauthorized"), "{refused:?}");
        assert!(!mark.exists(), "a refused quit marks nothing");

        let app_key = crate::clients::tests::app_key(dir.path());
        assert_eq!(ask(Some(app_key)).await, Response::Ok);
        assert_marked(&mark);
        let end = tokio::time::timeout(Duration::from_secs(5), ended.recv()).await;
        assert_eq!(end, Ok(Some(())), "the service ends");
    }

    /// Issue #279: the next start removes the marker, whoever starts the service.
    #[test]
    fn the_start_removes_the_marker() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("core.sock");
        let mark = dir.path().join(QUIT_MARK);
        assert!(!stopped_by_person(&socket));
        torro_gui::source::write_mark(&mark, chrono::Utc::now()).expect("written");
        assert!(stopped_by_person(&socket));

        remove_mark(Some(&mark));
        assert!(!mark.exists());
        assert!(!stopped_by_person(&socket));
        remove_mark(Some(&mark));
        remove_mark(None);
    }

    /// Issue #279: the setup opens only once the socket answers; a socket that takes connections but
    /// never answers (or none at all) opens nothing. The start marker goes when the socket answers, so
    /// a quit and a start a few seconds later start the core again; without an answer it stays.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_setup_waits_for_an_answering_socket() {
        use std::sync::atomic::{AtomicBool, Ordering};

        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let dir = tempfile::tempdir().expect("temp dir");
        let opened = AtomicBool::new(false);
        let open = || async { opened.store(true, Ordering::SeqCst) };

        let start = dir.path().join(torro_gui::source::CORE_START_MARK);
        torro_gui::source::write_mark(&start, chrono::Utc::now()).expect("written");

        let silent = dir.path().join("silent.sock");
        let _held = std::os::unix::net::UnixListener::bind(&silent).expect("binds");
        assert!(!once_listening(&silent, Duration::from_millis(500), Some(&start), open()).await);
        let none = dir.path().join("none.sock");
        assert!(!once_listening(&none, Duration::from_millis(200), Some(&start), open()).await);
        assert!(!opened.load(Ordering::SeqCst), "no setup without an answer");
        assert!(start.exists(), "a core that does not answer keeps the start marker");

        let answering = dir.path().join("core.sock");
        let listener = tokio::net::UnixListener::bind(&answering).expect("binds");
        let core = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { return };
                let (read, mut write) = stream.into_split();
                let mut line = String::new();
                let _ = BufReader::new(read).read_line(&mut line).await;
                let pong = Response::Pong { version: "0.1.0".into(), api: torro_ipc::API_VERSION };
                let mut reply = serde_json::to_vec(&pong).expect("serializes");
                reply.push(b'\n');
                let _ = write.write_all(&reply).await;
            }
        });
        assert_eq!(
            torro_gui::source::core_state_file(&answering, torro_gui::source::CORE_START_MARK),
            Some(start.clone())
        );
        assert!(once_listening(&answering, Duration::from_secs(5), Some(&start), open()).await);
        assert!(opened.load(Ordering::SeqCst), "the setup opens once the socket answers");
        assert!(!start.exists(), "the start marker is gone");
        assert!(
            torro_gui::source::claim_core_start(&answering, chrono::Utc::now()),
            "a start right after a quit is due again"
        );
        core.abort();
    }

    /// Issue #243: a key and a startup change the menu, but the dialog that sets them shows its own
    /// result, so no notification comes.
    #[test]
    fn keys_and_startup_refresh_the_menu_without_a_notification() {
        let key = Request::SetTemplateShortcut { id: "dev".into(), key: Some(2) };
        let startup = Request::SetTemplateStartup { id: "dev".into(), startup: None };
        for request in [&key, &startup] {
            assert!(changes_state(request) && !tells_on_desktop(request), "{request:?}");
        }
        assert!(tells_on_desktop(&Request::LoadTemplate { id: "dev".into(), mode: torro_setups::LoadMode::Add }));
        assert_eq!(describe(&key), "key 2 for dev");
        assert_eq!(describe(&Request::SetTemplateShortcut { id: "dev".into(), key: None }), "no key for dev");
        assert_eq!(describe(&startup), "load dev not at login");
        assert_eq!(describe(&Request::ListScreens), "screens");
    }

    /// Task WS.7: an app or the template dialog that opened is its own answer; other tray commands
    /// still come as a notification.
    #[test]
    fn an_opened_window_needs_no_notification() {
        let edit = Request::OpenApp { app: AppId::WORKPLACE.binary.into(), target: Some("template:dev".into()) };
        assert!(shows_itself(&edit));
        assert!(shows_itself(&Request::OpenApp { app: AppId::WORKPLACE.binary.into(), target: None }));
        assert!(!shows_itself(&Request::LoadTemplate { id: "dev".into(), mode: torro_setups::LoadMode::Add }));
        assert!(!shows_itself(&Request::SaveTemplate { name: None }));
    }

    /// Task E8.7a: messages about these requests name the list and the app, never the mail
    /// account, the mail's id, its Message-ID or what an app was opened on.
    /// Issue #304: the commands of the tool `files` are named by what they do, never with a path or the
    /// name searched for.
    #[test]
    fn file_commands_name_no_path() {
        let path = || "/Users/anna/Salary/2026.pdf".to_string();
        let named = [
            (Request::ListFolder { path: Some(path()) }, "list a folder"),
            (Request::FindFiles { folder: path(), name: "salary".into() }, "find files"),
            (Request::CreateFolder { path: path() }, "make a folder"),
            (Request::CopyFile { from: path(), to: path() }, "copy a file"),
            (Request::MoveFile { from: path(), to: path() }, "move a file"),
            (Request::TrashFile { path: path() }, "trash a file"),
        ];
        for (request, words) in named {
            assert_eq!(describe(&request), words);
        }
    }

    #[test]
    fn mail_and_targets_stay_out_of_messages() {
        let task = Request::CreateTodoFromMail {
            account: "mail:anna@mailbox.org".into(),
            id: "m:0a1b2c3d".into(),
            collection: "home".into(),
            title: Some("Answer Anna".into()),
            due: None,
        };
        assert_eq!(describe(&task), "make a task in home of a mail");
        let open = Request::OpenApp {
            app: "torro-inbox-mail".into(),
            target: Some("mail:anna@mailbox.org:<offer@keller.example>".into()),
        };
        assert_eq!(describe(&open), "open torro-inbox-mail");
        // Task E8.5b: an event of an entry names the calendar and the entry, never the title.
        let at = torro_pim::EventTime::DateTime(chrono::Utc::now());
        let event = Request::CreateEventFromItem {
            id: "e1".into(),
            collection: "work".into(),
            title: Some("Secret".into()),
            start: at,
            end: at,
            location: None,
            notes: None,
        };
        assert_eq!(describe(&event), "make an event in work of inbox item e1");
        // Task E8.3b: an answer to an invitation names nothing of the mail.
        let answer = Request::RespondToInvitation {
            account: "mail:anna@mailbox.org".into(),
            id: "m:0a1b2c3d".into(),
            answer: torro_mail::InvitationAnswer::Decline,
            collection: Some("work".into()),
        };
        assert_eq!(describe(&answer), "answer an invitation");
    }

    /// Set for this test binary as a child of [`a_stop_signal_ends_the_service_like_quit`]: only then
    /// does [`stop_signals_in_a_child`] wait for a signal.
    #[cfg(unix)]
    const STOP_CHILD: &str = "TORRO_TEST_STOP_SIGNALS";

    /// What the child prints once [`stop_signals`] is called.
    #[cfg(unix)]
    const STOP_READY: &str = "torro-stop-signals-ready";

    /// PR #259: `systemctl --user stop` and `kill` send SIGTERM, a terminal Ctrl+C sends SIGINT; both
    /// must end the service the way "Quit" does (releasing the template keys), not kill the process,
    /// also when they come before anything waits for them.
    ///
    /// The signal goes to a child, this test binary running only [`stop_signals_in_a_child`], never to
    /// the test binary itself: tokio never takes a handler back, so the test binary would ignore SIGTERM
    /// and Ctrl+C from then on, and `timeout`, the test runner or a person could no longer stop it.
    #[cfg(unix)]
    #[test]
    fn a_stop_signal_ends_the_service_like_quit() {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        let path = module_path!().split_once("::").map_or(module_path!(), |(_, path)| path);
        let test = format!("{path}::stop_signals_in_a_child");
        for signal in ["TERM", "INT"] {
            let mut child = Command::new(std::env::current_exe().expect("test binary"))
                .args([test.as_str(), "--exact", "--nocapture", "--test-threads=1"])
                .env(STOP_CHILD, "1")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("child starts");
            let mut lines = std::io::BufReader::new(child.stdout.take().expect("stdout")).lines();
            // On the line of the test runner's "test … ", which ends only after the test.
            let ready = lines.by_ref().map_while(Result::ok).any(|line| line.contains(STOP_READY));
            if ready {
                let sent = Command::new("kill")
                    .args([format!("-{signal}"), child.id().to_string()])
                    .status()
                    .expect("kill runs");
                assert!(sent.success(), "SIG{signal} not sent");
            }
            // Read to the end: the child still writes its result, a closed pipe would fail it.
            let rest: Vec<String> = lines.map_while(Result::ok).collect();
            let output = child.wait_with_output().expect("child ends");
            let errors = String::from_utf8_lossy(&output.stderr);
            assert!(ready, "the child never got ready: {rest:?} {errors}");
            assert!(
                output.status.success(),
                "SIG{signal} did not end it like Quit: {:?} {rest:?} {errors}",
                output.status
            );
        }
    }

    /// The child of [`a_stop_signal_ends_the_service_like_quit`]; without [`STOP_CHILD`] it does
    /// nothing. Waits a while before it polls [`stop_signals`]: the signal comes meanwhile and must be
    /// kept, not end the process.
    #[cfg(unix)]
    #[tokio::test]
    async fn stop_signals_in_a_child() {
        if std::env::var_os(STOP_CHILD).is_none() {
            return;
        }
        let stopped = stop_signals();
        println!("{STOP_READY}");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(tokio::time::timeout(Duration::from_secs(10), stopped).await.is_ok(), "the signal did not end it");
    }
}
