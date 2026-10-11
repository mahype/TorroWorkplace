//! Hyprland's request socket `.socket.sock`.
//!
//! Task 1.3 in `docs/tasks/phase-1.md`. One request per connection: connect, write the
//! command (e.g. `j/clients`, `dispatch hl.dsp.focus({ … })`), read until EOF.

use std::path::{Path, PathBuf};

use crate::WmError;

/// File name of the request socket inside the instance directory.
#[cfg(unix)]
const REQUEST_SOCKET: &str = ".socket.sock";

/// How long a request may take; a Hyprland that accepts but never answers must not block
/// the daemon.
#[cfg(unix)]
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// `<runtime_dir>/hypr/<signature>`; an error when either is missing or empty.
pub fn socket_dir(runtime_dir: Option<&str>, signature: Option<&str>) -> Result<PathBuf, WmError> {
    let runtime_dir = non_empty(runtime_dir, "XDG_RUNTIME_DIR")?;
    let signature = non_empty(signature, "HYPRLAND_INSTANCE_SIGNATURE")?;
    Ok(Path::new(runtime_dir).join("hypr").join(signature))
}

fn non_empty<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, WmError> {
    match value {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err(WmError::NotRunning(format!("{name} is not set"))),
    }
}

/// Sends `command` to `<dir>/.socket.sock` and returns the whole answer. Unix only; on other
/// systems an `Unsupported` error.
#[cfg(unix)]
pub async fn request(dir: &Path, command: &str) -> Result<String, WmError> {
    tokio::time::timeout(TIMEOUT, request_unbounded(dir, command))
        .await
        .map_err(|_| WmError::Io(format!("Hyprland did not answer {command:?} within {} s", TIMEOUT.as_secs())))?
}

#[cfg(unix)]
async fn request_unbounded(dir: &Path, command: &str) -> Result<String, WmError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let path = dir.join(REQUEST_SOCKET);
    let mut stream = UnixStream::connect(&path).await.map_err(|err| match err.kind() {
        // A missing socket file or a stale one left by a Hyprland that has exited.
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
            WmError::NotRunning(format!("no Hyprland socket at {}", path.display()))
        }
        _ => WmError::Io(format!("connect to {}: {err}", path.display())),
    })?;

    stream.write_all(command.as_bytes()).await.map_err(|err| WmError::Io(format!("write request: {err}")))?;
    // Half-close so the server sees the end of the request; it answers and closes.
    stream.shutdown().await.map_err(|err| WmError::Io(format!("finish request: {err}")))?;

    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).await.map_err(|err| WmError::Io(format!("read answer: {err}")))?;
    String::from_utf8(answer).map_err(|err| WmError::Protocol(format!("answer is not UTF-8: {err}")))
}

/// Sends `command` to `<dir>/.socket.sock` and returns the whole answer. Unix only; on other
/// systems an `Unsupported` error.
#[cfg(not(unix))]
pub async fn request(dir: &Path, command: &str) -> Result<String, WmError> {
    let _ = (dir, command);
    Err(WmError::Unsupported("Hyprland sockets exist only on Unix systems".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_dir_joins_runtime_dir_and_signature() {
        let dir = socket_dir(Some("/run/user/1000"), Some("abc_123")).expect("both set");
        assert_eq!(dir, Path::new("/run/user/1000/hypr/abc_123"));
    }

    #[test]
    fn socket_dir_fails_without_runtime_dir() {
        let err = socket_dir(None, Some("abc")).expect_err("XDG_RUNTIME_DIR missing");
        assert!(matches!(err, WmError::NotRunning(ref what) if what.contains("XDG_RUNTIME_DIR")), "{err:?}");
    }

    #[test]
    fn socket_dir_fails_without_signature() {
        let err = socket_dir(Some("/run/user/1000"), None).expect_err("HYPRLAND_INSTANCE_SIGNATURE missing");
        assert!(
            matches!(err, WmError::NotRunning(ref what) if what.contains("HYPRLAND_INSTANCE_SIGNATURE")),
            "{err:?}"
        );
    }

    #[test]
    fn socket_dir_fails_on_empty_values() {
        assert!(socket_dir(Some(""), Some("abc")).is_err());
        assert!(socket_dir(Some("/run/user/1000"), Some("")).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn request_without_socket_file_is_not_running() {
        let dir = std::env::temp_dir().join("torro-wm-no-such-instance-socket");
        let err = request(&dir, "j/clients").await.expect_err("no socket there");
        assert!(matches!(err, WmError::NotRunning(_)), "{err:?}");
    }

    /// A stand-in for Hyprland: reads the request until EOF, then answers in two chunks.
    #[cfg(unix)]
    #[tokio::test]
    async fn request_sends_command_and_reads_whole_answer() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::UnixListener;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!("torro-wm-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp instance dir");

        let listener = UnixListener::bind(dir.join(REQUEST_SOCKET)).expect("bind fake socket");
        let server = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.expect("accept");
            let mut received = String::new();
            conn.read_to_string(&mut received).await.expect("read request until EOF");
            conn.write_all(b"[{\"class\":").await.expect("write first chunk");
            conn.write_all(b"\"kitty\"}]\n").await.expect("write second chunk");
            received
        });

        let answer = request(&dir, "j/clients").await;
        let received = server.await.expect("server task");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(received, "j/clients");
        assert_eq!(answer.expect("answer"), "[{\"class\":\"kitty\"}]\n");
    }

    /// Needs a running Hyprland on this machine: `cargo test -p torro-wm -- --ignored`.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "needs a running Hyprland instance"]
    async fn live_active_workspace_answer() {
        let runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
        let dir = socket_dir(runtime.as_deref(), signature.as_deref()).expect("Hyprland environment");
        let answer = request(&dir, "j/activeworkspace").await.expect("request to Hyprland");
        assert!(answer.trim_start().starts_with('{'), "unexpected answer: {answer}");
    }
}
