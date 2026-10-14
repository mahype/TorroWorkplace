//! Where the window's requests go: the core over its socket, or the demo in memory (`--demo`).
//!
//! The window reads and changes nothing itself (`CLAUDE.md`, "Jede Funktion dreimal"): every
//! action is a [`torro_ipc::Request`] – `assistant.*`, `ai.list_models`, `approvals.*`,
//! `clients.list_clients` – that [`Backend::ask`] sends. The core holds the conversations and runs
//! the answers; the window asks again on its timer to follow them.

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
}
