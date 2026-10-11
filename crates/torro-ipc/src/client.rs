//! The clients' side of the socket.
//!
//! Task 1.17 in `docs/tasks/phase-1.md`.
//!
//! Each call opens its own connection, sends one request line and reads one response line.

use std::path::Path;

use crate::{IpcError, Request, Response};

/// Sends `request` to the core at `path` and waits for its response.
/// [`IpcError::NotRunning`] when nothing listens there.
#[cfg(unix)]
pub async fn call(path: &Path, request: &Request) -> Result<Response, IpcError> {
    use std::io::ErrorKind;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    // A stale socket file with no listener behind it refuses the connection, like a missing one.
    let stream = match UnixStream::connect(path).await {
        Ok(stream) => stream,
        Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
            return Err(IpcError::NotRunning);
        }
        Err(err) => return Err(IpcError::Io(err)),
    };

    let mut bytes = serde_json::to_vec(request).map_err(|err| IpcError::Protocol(err.to_string()))?;
    bytes.push(b'\n');
    let (read, mut write) = stream.into_split();
    write.write_all(&bytes).await.map_err(IpcError::Io)?;

    let mut line = Vec::new();
    if BufReader::new(read).read_until(b'\n', &mut line).await.map_err(IpcError::Io)? == 0 {
        return Err(IpcError::Protocol("core closed the connection without answering".into()));
    }
    serde_json::from_slice(&line).map_err(|err| IpcError::Protocol(err.to_string()))
}

/// Unix only for now; named pipes on Windows follow with the GlazeWM backend.
#[cfg(not(unix))]
pub async fn call(path: &Path, request: &Request) -> Result<Response, IpcError> {
    let _ = (path, request);
    Err(IpcError::Unsupported)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_socket_is_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("core.sock");

        let result = call(&path, &Request::Ping).await;
        assert!(matches!(result, Err(IpcError::NotRunning)), "got {result:?}");
    }
}
