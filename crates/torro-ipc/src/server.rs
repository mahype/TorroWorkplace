//! The core's side of the socket.
//!
//! Task 1.17 in `docs/tasks/phase-1.md`.

use std::future::Future;
use std::path::Path;

use crate::{IpcError, Request, Response};

/// Listens on `path` and answers each connection's request with `handler`, concurrently,
/// until the listener fails.
///
/// Single instance: if `path` exists and a core answers a [`Request::Ping`] there, returns
/// [`IpcError::AlreadyRunning`]; a stale socket file is removed. Creates the parent
/// directory. Unix only; elsewhere [`IpcError::Unsupported`].
pub async fn serve<H, F>(path: &Path, handler: H) -> Result<(), IpcError>
where
    H: Fn(Request) -> F + Send + Sync + 'static,
    F: Future<Output = Response> + Send,
{
    let _ = (path, handler);
    Err(IpcError::NotImplemented("ipc serve (task 1.17)"))
}
