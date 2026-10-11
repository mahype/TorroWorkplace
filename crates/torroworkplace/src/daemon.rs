//! `torroworkplace daemon`: the background service.
//!
//! One process per session. The main task owns a multi-threaded tokio runtime and runs:
//!
//! - the socket (`torro_ipc::server`) for `ctl`, hotkeys and later the control centre,
//! - the tray icon (Linux: ksni; it runs its own D-Bus task on the same runtime),
//! - a refresher that rebuilds the tray menu after every command and on window manager
//!   events (debounced),
//!
//! and ends on "Beenden", Ctrl+C or when the socket server fails. Every command, from the
//! tray or the socket, goes through [`Core::handle`].

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use torro_ipc::{Request, Response, client, server, socket_path};
use torro_setups::storage::Store;

use crate::core::{Core, Snapshot};
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
        self.runtime.spawn(async move {
            let what = describe(&request);
            let _ = refresh.send(());
            let response = core.handle(request).await;
            report(&what, &response);
            let _ = refresh.send(());
        });
    }

    pub fn quit(&self) {
        let _ = self.quit.send(());
    }
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
    match runtime.block_on(serve(headless)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("torroworkplace: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(headless: bool) -> Result<(), String> {
    let socket = socket_path().ok_or("no home directory")?;
    if let Ok(Response::Pong { .. }) = client::call(&socket, &Request::Ping).await {
        return Err("already running".into());
    }
    let store = Store::open_default().map_err(|err| err.to_string())?;
    let backend = torro_wm::detect(|key| std::env::var(key).ok());
    let core = Arc::new(Core::new(backend, store));

    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel();
    let (quit_tx, mut quit_rx) = mpsc::unbounded_channel();

    let ipc = {
        let core = Arc::clone(&core);
        let refresh = refresh_tx.clone();
        let socket = socket.clone();
        tokio::spawn(async move {
            server::serve(&socket, move |request| {
                let core = Arc::clone(&core);
                let refresh = refresh.clone();
                async move {
                    let changes = changes_state(&request);
                    let response = core.handle(request).await;
                    if changes {
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
    let front = Front::start(&daemon, headless, core.snapshot().await).await?;
    tokio::spawn(refresher(Arc::clone(&core), refresh_rx, front.clone()));

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

/// What shows the state to the user: the tray icon, or nothing when headless or not
/// available on this system yet.
#[derive(Clone)]
enum Front {
    #[cfg(target_os = "linux")]
    Tray(tray::TrayHandle),
    None,
}

impl Front {
    async fn start(daemon: &Arc<Daemon>, headless: bool, snapshot: Snapshot) -> Result<Front, String> {
        if headless {
            return Ok(Front::None);
        }
        #[cfg(target_os = "linux")]
        {
            let handle = tray::spawn(Arc::clone(daemon), snapshot).await.map_err(|err| err.to_string())?;
            Ok(Front::Tray(handle))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (daemon, snapshot);
            eprintln!("torroworkplace: no tray icon on this system yet; running headless");
            Ok(Front::None)
        }
    }

    async fn show(&self, snapshot: Snapshot) {
        match self {
            #[cfg(target_os = "linux")]
            Front::Tray(handle) => {
                handle.update(|tray| tray.snapshot = snapshot).await;
            }
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

/// Requests after which the menu may look different.
fn changes_state(request: &Request) -> bool {
    !matches!(request, Request::Ping | Request::Status | Request::ListTemplates)
}

/// A short name for a request, for messages.
fn describe(request: &Request) -> String {
    match request {
        Request::Ping => "ping".into(),
        Request::Status => "status".into(),
        Request::ListTemplates => "list".into(),
        Request::SaveTemplate { .. } => "save".into(),
        Request::UpdateTemplate { id } => format!("update {id}"),
        Request::DeleteTemplate { id } => format!("delete {id}"),
        Request::LoadTemplate { id, .. } => format!("load {id}"),
    }
}

/// Tells the user how a tray command went. Desktop notifications follow with task 1.20.
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
        _ => eprintln!("torroworkplace: {what}: ok"),
    }
}
