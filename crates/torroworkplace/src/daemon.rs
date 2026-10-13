//! `torroworkplace daemon`: the background service.
//!
//! One process per session. A multi-threaded tokio runtime runs:
//!
//! - the socket (`torro_ipc::server`) for `ctl`, hotkeys, the control centre, the apps and
//!   agents; every request there passes the key check ([`Core::handle_from`]),
//! - the tray icon (see `tray`),
//! - a refresher that rebuilds the tray menu after every command, on window manager events and
//!   when the open approvals change ([`watch_approvals`]) or anything is written to the inbox
//!   ([`watch_inbox`]), all debounced into one refresh,
//! - the inbox routine: wakes snoozed entries every minute and removes old done ones once a day
//!   ([`tend_inbox`]),
//!
//! and ends on "Quit", Ctrl+C or when the socket server fails. Every command, from the
//! tray or the socket, goes through [`Core::handle`].
//!
//! Where the main thread goes depends on the system. On Linux it waits for the service
//! (`block_on`); ksni runs its D-Bus task on the same runtime. On macOS and Windows the tray
//! needs a native event loop on the main thread, so the service runs as a task on the runtime's
//! worker threads and the main thread runs [`tray::Shell`] until the service ends.
//! `--headless` runs no tray and no event loop on any system.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_ipc::{Request, Response, app_key_path, client, server, socket_path};
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
}

impl Daemon {
    /// Runs `request` in the background and refreshes the menu before (to show "busy") and
    /// after.
    pub fn run(&self, request: Request) {
        let core = Arc::clone(&self.core);
        let refresh = self.refresh.clone();
        let quit = self.quit.clone();
        self.runtime.spawn(async move {
            let what = describe(&request);
            let ends = matches!(request, Request::Quit);
            let _ = refresh.send(());
            let response = core.handle(request).await;
            report(&what, &response);
            if ends && matches!(response, Response::Ok) {
                end_after_answer(quit);
            }
            let _ = refresh.send(());
        });
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

/// Ends the service once the answer to a `system.quit` is on its way (see [`ANSWER_GRACE`]).
fn end_after_answer(quit: mpsc::UnboundedSender<()>) {
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
    let socket = socket_path().ok_or("no home directory")?;
    // Bounded: a program that holds the socket or pipe name but never answers must not stall
    // the start; the server then fails on the taken name with a clear error instead.
    let ping = tokio::time::timeout(START_PING_TIMEOUT, client::call(&socket, &Request::Ping)).await;
    if let Ok(Ok(Response::Pong { .. })) = ping {
        return Err("already running".into());
    }
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
    let store = Store::open_default().map_err(|err| err.to_string())?;
    // The first start of the suite brings the example design packs (task L.5); later starts find
    // the folder and leave it alone.
    let _ = tokio::task::spawn_blocking(crate::designs::install_examples).await;
    let backend = torro_wm::detect(|key| std::env::var(key).ok());
    let pim = Arc::new(crate::pim::PimService::new());
    pim.start();
    let core = Arc::new(Core::new(backend, store, pim, clients));
    core.start();
    tokio::spawn(tend_inbox(Arc::clone(&core)));
    // Popups for new entries "for me" (task E3.4), also when headless: they need no tray.
    tokio::spawn(crate::inbox_notify::run(Arc::clone(&core), Arc::new(crate::inbox_notify::SystemDesktop)));

    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel();
    let (quit_tx, mut quit_rx) = mpsc::unbounded_channel();

    let ipc = {
        let core = Arc::clone(&core);
        let refresh = refresh_tx.clone();
        let quit = quit_tx.clone();
        let socket = socket.clone();
        tokio::spawn(async move {
            server::serve_with_key(&socket, move |key, request| {
                let core = Arc::clone(&core);
                let refresh = refresh.clone();
                let quit = quit.clone();
                async move {
                    // Hotkeys go through `ctl`, so commands from the socket get the same
                    // notifications as clicks in the tray.
                    let changes = changes_state(&request).then(|| describe(&request));
                    let ends = matches!(request, Request::Quit);
                    // A refused request ran nothing: no notification, no new menu, so a foreign
                    // client cannot flood the desktop.
                    let response = match core.handle_from(key.as_ref(), request).await {
                        Ok(response) => response,
                        Err(refused) => return refused.response(),
                    };
                    if ends && matches!(response, Response::Ok) {
                        end_after_answer(quit);
                    }
                    if let Some(what) = changes {
                        report(&what, &response);
                        let _ = refresh.send(());
                    }
                    response
                }
            })
            .await
        })
    };

    let daemon = Arc::new(Daemon {
        core: Arc::clone(&core),
        runtime: tokio::runtime::Handle::current(),
        refresh: refresh_tx.clone(),
        quit: quit_tx,
    });
    // Taken before the first snapshot, so an approval opened or an entry posted in between still
    // refreshes the menu.
    let approval_changes = core.approval_changes();
    let inbox_changes = core.inbox().changes();
    let front = Front::start(&daemon, shows, core.snapshot().await).await?;
    tokio::spawn(refresher(Arc::clone(&core), refresh_rx, front.clone()));
    tokio::spawn(watch_approvals(Arc::clone(&core), approval_changes, refresh_tx.clone()));
    tokio::spawn(watch_inbox(inbox_changes, refresh_tx.clone()));

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

    let result = tokio::select! {
        _ = quit_rx.recv() => Ok(()),
        _ = tokio::signal::ctrl_c() => Ok(()),
        ended = ipc => match ended {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err.to_string()),
            Err(err) => Err(format!("socket task: {err}")),
        },
    };
    let _ = std::fs::remove_file(&socket);
    result
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
                handle.start(Arc::clone(daemon), snapshot);
                Ok(Front::Desktop(handle))
            }
        }
    }

    async fn show(&self, snapshot: Snapshot) {
        match self {
            #[cfg(target_os = "linux")]
            Front::Tray(handle) => {
                handle.update(|tray| tray.snapshot = snapshot).await;
            }
            #[cfg(any(target_os = "macos", windows))]
            Front::Desktop(handle) => handle.show(snapshot),
            Front::None => {
                let _ = snapshot;
            }
        }
    }
}

/// Rebuilds the menu when asked; bursts of requests (a load opens many windows) collapse
/// into one refresh.
async fn refresher(core: Arc<Core>, mut requests: mpsc::UnboundedReceiver<()>, front: Front) {
    while requests.recv().await.is_some() {
        tokio::time::sleep(Duration::from_millis(150)).await;
        while requests.try_recv().is_ok() {}
        front.show(core.snapshot().await).await;
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

/// Template commands: they change the menu and get a notification. Calendar and task
/// requests from the apps get neither; the apps show their own feedback.
fn changes_state(request: &Request) -> bool {
    matches!(
        request,
        Request::SaveTemplate { .. }
            | Request::UpdateTemplate { .. }
            | Request::DeleteTemplate { .. }
            | Request::LoadTemplate { .. }
    )
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
        Request::SaveTodo { .. } => "save todo".into(),
        Request::DeleteTodo { .. } => "delete todo".into(),
        Request::MoveTodo { .. } => "move todo".into(),
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
        Request::OpenApp { app } => format!("open {app}"),
        Request::ListApps => "apps".into(),
        Request::ListDesigns => "designs".into(),
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
        Request::ListSources => "inbox sources".into(),
        Request::PostItem { .. } => "post to inbox".into(),
        Request::ResolveItems { .. } => "resolve inbox items".into(),
        Request::SetItemDone { id, .. } => format!("set inbox item {id} done"),
        Request::SnoozeItem { id, .. } => format!("snooze inbox item {id}"),
        Request::SetItemLane { id, .. } => format!("move inbox item {id}"),
        Request::MuteSource { source, muted } => format!("{} {source}", if *muted { "mute" } else { "unmute" }),
        // Never the address or the domain: they would name who writes to the person.
        Request::HideSender { id, scope: torro_inbox::HideScope::Conversation } => format!("hide inbox item {id}"),
        Request::HideSender { id, .. } => format!("hide the sender of inbox item {id}"),
        Request::ListRules => "inbox rules".into(),
        Request::RemoveRule { id } => format!("remove inbox rule {id}"),
        // Never an address, a password, a search, a subject or a folder name (security rule 3 of
        // task M2.1); message and folder ids would let lines be matched with a mailbox.
        Request::ConnectMail { .. } => "connect mail".into(),
        Request::UpdateMailAccount { .. } => "update mail account".into(),
        Request::TorroMailAccounts => "torromail accounts".into(),
        Request::ImportTorroMailAccount { .. } => "import torromail account".into(),
        // Never the address or the key (task KI.1a).
        Request::ConnectAi { .. } => "connect ai".into(),
        Request::AiModels { .. } => "ai models".into(),
        Request::FindAiServers => "find ai servers".into(),
        Request::MailFolders { .. } => "mail folders".into(),
        Request::SearchMail { .. } => "search mail".into(),
        Request::GetMail { .. } => "get mail".into(),
        Request::GetMailThread { .. } => "get mail thread".into(),
        Request::GetMailAttachment { .. } => "get mail attachment".into(),
        Request::SetMailFlags { ids, .. } => format!("set flags of {} messages", ids.len()),
        Request::MoveMail { ids, .. } => format!("move {} messages", ids.len()),
        Request::DeleteMail { ids, .. } => format!("delete {} messages", ids.len()),
        Request::SaveDraft { .. } => "save draft".into(),
        Request::SendDraft { .. } => "send draft".into(),
    }
}

/// Tells the user how a tray command went: on stderr, and as a desktop notification.
fn report(what: &str, response: &Response) {
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
    if let Some((summary, body)) = crate::notify::message(what, response) {
        crate::notify::show(summary, body);
    }
}
