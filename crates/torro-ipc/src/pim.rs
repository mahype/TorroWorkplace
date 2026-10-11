//! [`PimSource`] over the socket: how the apps reach calendars and tasks in the core.
//!
//! Task K.7 in `docs/tasks/kalender-aufgaben.md`. Each method sends one [`Request`] with
//! [`crate::client::call`] and maps the [`Response`]: the expected variant → `Ok`,
//! `Response::Error { code, message }` → [`ProviderError`] (`auth` → `Auth`, `offline` →
//! `Offline`, `conflict` → `Conflict`, everything else → `Other(message)`), a core that is
//! not running → `ProviderError::Other` with the `IpcError` message, an unexpected
//! variant → `Other("unexpected answer …")`.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use torro_pim::{AccountState, Collection, Event, PimSource, ProviderError, Todo};

/// Talks to the core at its socket.
#[derive(Debug, Clone)]
pub struct PimClient {
    pub socket: PathBuf,
}

impl Default for PimClient {
    /// The core of this user session ([`crate::socket_path`]).
    fn default() -> PimClient {
        PimClient { socket: crate::socket_path().unwrap_or_else(|| PathBuf::from("core.sock")) }
    }
}

fn todo_k7() -> ProviderError {
    ProviderError::Other("PimClient not implemented yet (task K.7)".into())
}

impl PimSource for PimClient {
    async fn account(&self) -> Result<AccountState, ProviderError> {
        Err(todo_k7())
    }

    async fn connect_nextcloud(&self, server: String) -> Result<String, ProviderError> {
        let _ = server;
        Err(todo_k7())
    }

    async fn sync(&self) -> Result<(), ProviderError> {
        Err(todo_k7())
    }

    async fn collections(&self) -> Result<Vec<Collection>, ProviderError> {
        Err(todo_k7())
    }

    async fn events(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Event>, ProviderError> {
        let _ = (start, end);
        Err(todo_k7())
    }

    async fn todos(&self) -> Result<Vec<Todo>, ProviderError> {
        Err(todo_k7())
    }

    async fn save_event(&self, event: Event) -> Result<Event, ProviderError> {
        let _ = event;
        Err(todo_k7())
    }

    async fn delete_event(&self, event: Event) -> Result<(), ProviderError> {
        let _ = event;
        Err(todo_k7())
    }

    async fn save_todo(&self, todo: Todo) -> Result<Todo, ProviderError> {
        let _ = todo;
        Err(todo_k7())
    }

    async fn delete_todo(&self, todo: Todo) -> Result<(), ProviderError> {
        let _ = todo;
        Err(todo_k7())
    }
}
