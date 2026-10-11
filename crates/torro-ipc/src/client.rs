//! The clients' side of the socket.
//!
//! Task 1.17 in `docs/tasks/phase-1.md`.

use std::path::Path;

use crate::{IpcError, Request, Response};

/// Sends `request` to the core at `path` and waits for its response.
/// [`IpcError::NotRunning`] when nothing listens there.
pub async fn call(path: &Path, request: &Request) -> Result<Response, IpcError> {
    let _ = (path, request);
    Err(IpcError::NotImplemented("ipc call (task 1.17)"))
}
