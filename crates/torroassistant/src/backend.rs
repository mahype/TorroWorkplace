//! Where the window's requests go: the core over its socket, or the demo in memory (`--demo`).
//!
//! The window reads and changes nothing itself (`CLAUDE.md`, "Jede Funktion dreimal"): every
//! action is a [`torro_ipc::Request`] – `assistant.*`, `ai.list_models`, `approvals.*`,
//! `clients.list_clients` – that [`Backend::ask`] sends. The core holds the conversations and runs
//! the answers; the window asks again on its timer to follow them.
//!
//! The settings in the content area (task S.18) take the same two ways, through the trait of
//! `torro-settings-ui` ([`Backend::settings`]): the core over its socket, or
//! [`torro_settings_ui::demo::Demo`] with `--demo`. A demo window never reaches the socket – the
//! assistant's own demo answers no settings request, and `torro_ipc::client` would read the app
//! key and really change rights and settings.

use std::path::PathBuf;
use std::sync::Arc;

use torro_ipc::{Request, Response};

use crate::demo::Demo;

/// Why a request did not go through: the code of an error answer (`not-found`, `busy` …, see
/// `Response::Error`) and its message for people. A core that cannot be reached is `offline`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

impl Refusal {
    /// A refusal with `code` and `message`.
    pub fn new(code: &str, message: impl Into<String>) -> Refusal {
        Refusal { code: code.to_owned(), message: message.into() }
    }

    /// The core answered with something the request does not answer with.
    pub fn unexpected(response: &Response) -> Refusal {
        Refusal::new("error", format!("unexpected answer: {response:?}"))
    }
}

/// The core over its socket, or the demo. Cheap to clone, so every task can own one.
#[derive(Debug, Clone)]
pub enum Backend {
    /// The core of this user session, at this socket.
    Core(PathBuf),
    /// Sample conversations in memory.
    Demo(Arc<Demo>),
}

impl Backend {
    /// The core of this user session ([`torro_ipc::socket_path`]).
    pub fn core() -> Backend {
        Backend::Core(torro_ipc::socket_path().unwrap_or_else(|| PathBuf::from("core.sock")))
    }

    /// Sends `request` and returns the answer; an error answer, or a core that cannot be reached,
    /// is a [`Refusal`], so `Ok` never holds `Response::Error`.
    pub async fn ask(&self, request: Request) -> Result<Response, Refusal> {
        let answer = match self {
            Backend::Core(socket) => torro_ipc::client::call(socket, &request)
                .await
                .map_err(|err| Refusal::new("offline", err.to_string()))?,
            Backend::Demo(demo) => demo.answer(request),
        };
        match answer {
            Response::Error { code, message } => Err(Refusal { code, message }),
            other => Ok(other),
        }
    }

    /// The way the settings of this window take to the core (task S.18,
    /// [`torro_settings_ui::Host::new`]): this socket, or the settings library's own demo, which
    /// answers sample data from memory and never opens a socket.
    pub fn settings(&self) -> Arc<dyn torro_settings_ui::Backend> {
        match self {
            Backend::Core(socket) => Arc::new(Socket { socket: socket.clone() }),
            Backend::Demo(_) => Arc::new(torro_settings_ui::demo::Demo::new()),
        }
    }
}

/// The [`torro_settings_ui::Backend`] of a window on the core: one request, one answer over the
/// socket. An error answer comes back as its message for people, as the library asks.
struct Socket {
    socket: PathBuf,
}

impl torro_settings_ui::Backend for Socket {
    fn call(&self, request: Request) -> torro_settings_ui::Answer {
        let socket = self.socket.clone();
        Box::pin(async move {
            match torro_ipc::client::call(&socket, &request).await {
                Ok(Response::Error { message, .. }) => Err(message),
                Ok(answer) => Ok(answer),
                Err(err) => Err(err.to_string()),
            }
        })
    }
}
