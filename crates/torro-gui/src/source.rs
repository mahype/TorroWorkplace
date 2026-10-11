//! The data source an app runs on.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use torro_ipc::pim::PimClient;
use torro_pim::demo::DemoSource;
use torro_pim::{AccountState, Collection, Event, PimSource, ProviderError, Todo};

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
}

/// Starts `torroworkplace daemon` in the background when nothing listens on `socket`. The
/// app then retries until the core answers. Unix only for now.
pub fn ensure_core(socket: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return;
        }
        let program = sibling(torro_core::AppId::WORKPLACE.binary);
        match Command::new(&program)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
        {
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
    async fn account(&self) -> Result<AccountState, ProviderError> {
        delegate!(self, s => s.account())
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
}
