//! The clients' side of the socket.
//!
//! Task 1.17 in `docs/tasks/phase-1.md`; the key in every request since A.1 in
//! `docs/tasks/api-mcp.md`.
//!
//! Each call opens its own connection, sends one [`Envelope`] line and reads one response line.
//! The suite's own programs use [`call`], which reads the app key next to the socket, so they
//! never deal with keys; scripts and the MCP server bring their own key to [`call_with_key`].

use std::path::Path;

use crate::{IpcError, Key, Request, Response, app_key_path};

/// Sends `request` to the core at `path` with the app key and waits for its response.
/// [`IpcError::NotRunning`] when nothing listens there.
///
/// The key is read on every call from `app.key` next to the socket ([`app_key_path`]), because
/// the core creates it on its first start, which may be after this program started. Without a
/// readable key the request goes out without one; the core then answers only `system.ping` and
/// everything else with `unauthorized`.
pub async fn call(path: &Path, request: &Request) -> Result<Response, IpcError> {
    let key = read_app_key(path);
    call_with_key(path, key.as_ref(), request).await
}

/// Sends `request` with `key` (or none) to the core at `path` and waits for its response.
/// [`IpcError::NotRunning`] when nothing listens there.
#[cfg(unix)]
pub async fn call_with_key(path: &Path, key: Option<&Key>, request: &Request) -> Result<Response, IpcError> {
    use std::io::ErrorKind;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    use crate::{API_VERSION, Envelope};

    // A stale socket file with no listener behind it refuses the connection, like a missing one.
    let stream = match UnixStream::connect(path).await {
        Ok(stream) => stream,
        Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
            return Err(IpcError::NotRunning);
        }
        Err(err) => return Err(IpcError::Io(err)),
    };

    let envelope = Envelope { v: API_VERSION, key: key.cloned(), request: request.clone() };
    let mut bytes = serde_json::to_vec(&envelope).map_err(|err| IpcError::Protocol(err.to_string()))?;
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
pub async fn call_with_key(path: &Path, key: Option<&Key>, request: &Request) -> Result<Response, IpcError> {
    let _ = (path, key, request);
    Err(IpcError::Unsupported)
}

/// The app key next to the socket at `socket`, `None` when it is missing, unreadable or empty.
fn read_app_key(socket: &Path) -> Option<Key> {
    let key = std::fs::read_to_string(app_key_path(socket)).ok()?;
    let key = key.trim();
    (!key.is_empty()).then(|| Key::new(key))
}

#[cfg(all(test, unix))]
mod tests {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    use super::*;

    #[tokio::test]
    async fn missing_socket_is_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("core.sock");

        let result = call(&path, &Request::Ping).await;
        assert!(matches!(result, Err(IpcError::NotRunning)), "got {result:?}");
    }

    /// Accepts one connection on `path`, answers `Ok` and returns the line the client sent.
    async fn receive_one(listener: UnixListener) -> String {
        let (stream, _) = listener.accept().await.expect("a client connects");
        let (read, mut write) = stream.into_split();
        let mut line = String::new();
        BufReader::new(read).read_line(&mut line).await.expect("reads the line");
        write.write_all(b"{\"result\":\"ok\"}\n").await.expect("answers");
        line
    }

    #[tokio::test]
    async fn sends_the_app_key_next_to_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("core.sock");
        std::fs::write(dir.path().join("app.key"), "torro_app_abc\n").unwrap();
        let server = tokio::spawn(receive_one(UnixListener::bind(&path).unwrap()));

        assert_eq!(call(&path, &Request::Status).await.unwrap(), Response::Ok);
        let sent: serde_json::Value = serde_json::from_str(&server.await.unwrap()).unwrap();
        assert_eq!(sent, serde_json::json!({"v": 1, "key": "torro_app_abc", "request": {"method": "system.status"}}));
    }

    #[tokio::test]
    async fn without_app_key_the_request_goes_out_without_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("core.sock");
        let server = tokio::spawn(receive_one(UnixListener::bind(&path).unwrap()));

        call(&path, &Request::Ping).await.unwrap();
        let sent: serde_json::Value = serde_json::from_str(&server.await.unwrap()).unwrap();
        assert_eq!(sent, serde_json::json!({"v": 1, "request": {"method": "system.ping"}}));
    }
}
