//! The core's side of the socket.
//!
//! Task 1.17 in `docs/tasks/phase-1.md`; the envelope with version and key since A.1 in
//! `docs/tasks/api-mcp.md`.
//!
//! One [`crate::Envelope`] line in, one [`Response`] line out, per connection. Every connection
//! runs in its own task, so a slow request does not hold up the others. This module only
//! carries the key to the handler; deciding what a key may do is the core's job.

use std::future::Future;
use std::path::Path;

use crate::{IpcError, Key, Request, Response};

/// The longest request line the core reads, newline included. Longer lines are refused with
/// `error` and the connection is closed, so a client cannot make the core buffer without end.
pub const MAX_LINE: usize = 1024 * 1024;

/// Listens on `path` and answers each connection's request with `handler`, which gets the
/// request without its key.
///
/// Only for the test cores of this crate: a server built on this one lets every client in, so
/// nothing outside the tests may use it. The core uses [`serve_with_key`].
#[cfg(all(test, unix))]
pub(crate) async fn serve<H, F>(path: &Path, handler: H) -> Result<(), IpcError>
where
    H: Fn(Request) -> F + Send + Sync + 'static,
    F: Future<Output = Response> + Send + 'static,
{
    serve_with_key(path, move |_key, request| handler(request)).await
}

/// Listens on `path` and answers each connection's request with `handler`, concurrently.
/// `handler` gets the key the client sent (`None` without one) and must decide itself whether
/// that key may run the request. Runs until the task is dropped; failing to accept one client
/// is logged, not fatal.
///
/// Before the handler runs, the line must be an envelope of [`crate::API_VERSION`]: another
/// version is answered with `unsupported`, anything else that is not a request (or longer than
/// [`MAX_LINE`]) with `error`. The message never repeats the key.
///
/// Single instance: if `path` exists and a core answers a [`Request::Ping`] there, returns
/// [`IpcError::AlreadyRunning`]; a stale socket file is removed. Creates the parent directory
/// for the owner only (0700) and makes the socket 0600. Unix only; elsewhere
/// [`IpcError::Unsupported`].
#[cfg(unix)]
pub async fn serve_with_key<H, F>(path: &Path, handler: H) -> Result<(), IpcError>
where
    H: Fn(Option<Key>, Request) -> F + Send + Sync + 'static,
    F: Future<Output = Response> + Send + 'static,
{
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::sync::Arc;

    use tokio::net::UnixListener;

    if let Some(dir) = path.parent() {
        // Only directories created here are restricted; an existing one is the owner's choice.
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(IpcError::Io)?;
    }
    // `symlink_metadata` rather than `exists`: a dangling symlink is stale too.
    if std::fs::symlink_metadata(path).is_ok() {
        match crate::client::call(path, &Request::Ping).await {
            Ok(Response::Pong { .. }) => return Err(IpcError::AlreadyRunning),
            _ => std::fs::remove_file(path).map_err(IpcError::Io)?,
        }
    }

    let listener = UnixListener::bind(path).map_err(IpcError::Io)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(IpcError::Io)?;
    let handler = Arc::new(handler);
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            // Out of file descriptors and the like pass; keep serving instead of ending the core.
            Err(err) => {
                eprintln!("torroworkplace: accepting a client failed: {err}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
        };
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            // A client that hangs up mid-request only ends its own connection.
            let _ = handle_connection(stream, &*handler).await;
        });
    }
}

/// Unix only; elsewhere [`IpcError::Unsupported`].
#[cfg(not(unix))]
pub async fn serve_with_key<H, F>(path: &Path, handler: H) -> Result<(), IpcError>
where
    H: Fn(Option<Key>, Request) -> F + Send + Sync + 'static,
    F: Future<Output = Response> + Send + 'static,
{
    let _ = (path, handler);
    Err(IpcError::Unsupported)
}

/// The envelope with the request still unparsed, so that a malformed request gets serde's
/// helpful message without the risk of quoting the key, which sits outside `request`.
#[cfg(unix)]
#[derive(serde::Deserialize)]
struct RawEnvelope {
    v: u32,
    #[serde(default)]
    key: Option<Key>,
    request: serde_json::Value,
}

/// The key and request of one line, or the code and message of the error response for it.
#[cfg(unix)]
fn parse(line: &[u8]) -> Result<(Option<Key>, Request), (&'static str, String)> {
    use crate::API_VERSION;

    // serde's message could quote the key (a key in the wrong field), so it is not passed on.
    let envelope: RawEnvelope = serde_json::from_slice(line).map_err(|_| {
        let expected = format!("{{\"v\":{API_VERSION},\"key\":\"torro_…\",\"request\":{{\"method\":…}}}}");
        ("error", format!("malformed message: expected {expected}"))
    })?;
    if envelope.v != API_VERSION {
        let message = format!("API version {} is not supported; this core speaks version {API_VERSION}", envelope.v);
        return Err(("unsupported", message));
    }
    let request =
        serde_json::from_value(envelope.request).map_err(|err| ("error", format!("malformed request: {err}")))?;
    Ok((envelope.key, request))
}

/// Reads the request line, answers it, closes the connection.
///
/// A line that is not a request gets an error response instead of a silent close, so the
/// client learns what went wrong.
#[cfg(unix)]
async fn handle_connection<H, F>(stream: tokio::net::UnixStream, handler: &H) -> Result<(), IpcError>
where
    H: Fn(Option<Key>, Request) -> F,
    F: Future<Output = Response>,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let (read, mut write) = stream.into_split();
    let mut line = Vec::new();
    // One byte more than allowed tells a line at the limit from one beyond it.
    let limit = u64::try_from(MAX_LINE).unwrap_or(u64::MAX).saturating_add(1);
    if BufReader::new(read.take(limit)).read_until(b'\n', &mut line).await.map_err(IpcError::Io)? == 0 {
        // Connected and went away without sending anything.
        return Ok(());
    }

    let response = if line.len() > MAX_LINE {
        Response::Error { code: "error".into(), message: format!("request longer than {MAX_LINE} bytes") }
    } else {
        match parse(&line) {
            Ok((key, request)) => handler(key, request).await,
            Err((code, message)) => Response::Error { code: code.into(), message },
        }
    };
    let mut bytes = serde_json::to_vec(&response).map_err(|err| IpcError::Protocol(err.to_string()))?;
    bytes.push(b'\n');
    write.write_all(&bytes).await.map_err(IpcError::Io)
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use tokio::sync::Barrier;

    use super::*;
    use crate::client;

    /// Pong to `Ping`, `Ok` to everything else.
    async fn answer(request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "test".into() },
            _ => Response::Ok,
        }
    }

    fn socket_in(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("core.sock")
    }

    /// Pings until the server answers, so the tests do not race the bind.
    async fn wait_until_listening(path: &Path) {
        for _ in 0..200 {
            if matches!(client::call(path, &Request::Ping).await, Ok(Response::Pong { .. })) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("server did not start listening on {}", path.display());
    }

    #[tokio::test]
    async fn ping_gets_pong() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(&path).await;

        let response = client::call(&path, &Request::Ping).await.unwrap();
        assert_eq!(response, Response::Pong { version: "test".into() });
        server.abort();
    }

    #[tokio::test]
    async fn two_clients_are_served_concurrently() {
        // The handler waits until both requests are in flight at once. A server that handled
        // connections one after the other would never get past the barrier.
        let barrier = Arc::new(Barrier::new(2));
        let handler = move |request: Request| {
            let barrier = Arc::clone(&barrier);
            async move {
                match request {
                    Request::Ping => Response::Pong { version: "test".into() },
                    _ => {
                        barrier.wait().await;
                        Response::Ok
                    }
                }
            }
        };

        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, handler).await }
        });
        wait_until_listening(&path).await;

        let (first, second) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(client::call(&path, &Request::Status), client::call(&path, &Request::ListTemplates))
        })
        .await
        .expect("requests were handled one after the other");
        assert_eq!(first.unwrap(), Response::Ok);
        assert_eq!(second.unwrap(), Response::Ok);
        server.abort();
    }

    #[tokio::test]
    async fn second_server_reports_already_running() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(&path).await;

        let second = serve(&path, answer).await;
        assert!(matches!(second, Err(IpcError::AlreadyRunning)), "got {second:?}");

        // The first core is still the one answering.
        let response = client::call(&path, &Request::Ping).await.unwrap();
        assert_eq!(response, Response::Pong { version: "test".into() });
        server.abort();
    }

    #[tokio::test]
    async fn stale_plain_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        std::fs::write(&path, b"left over").unwrap();

        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(&path).await;
        server.abort();
    }

    #[tokio::test]
    async fn stale_socket_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        // Binding and dropping leaves the socket file behind with nothing listening on it.
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        assert!(path.exists());

        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(&path).await;
        server.abort();
    }

    #[tokio::test]
    async fn creates_missing_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("core.sock");

        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(&path).await;
        server.abort();
    }

    #[tokio::test]
    async fn socket_and_its_new_directory_are_for_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("core.sock");
        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(&path).await;

        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        server.abort();
    }

    #[tokio::test]
    async fn an_overlong_line_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(&path).await;

        let (read, mut write) = UnixStream::connect(&path).await.unwrap().into_split();
        // The server stops reading at the limit; whether the rest still gets out does not matter.
        let sender = tokio::spawn(async move {
            let _ = write.write_all(&vec![b'x'; MAX_LINE + 16]).await;
            write
        });
        let mut line = String::new();
        BufReader::new(read).read_line(&mut line).await.unwrap();
        let response: Response = serde_json::from_str(&line).unwrap();
        assert!(
            matches!(&response, Response::Error { code, message } if code == "error" && message.contains("longer")),
            "{response:?}"
        );
        drop(sender.await);

        // A line right at the limit is still read and answered (here as malformed).
        let mut at_limit = vec![b' '; MAX_LINE - 1];
        at_limit.push(b'\n');
        let (read, mut write) = UnixStream::connect(&path).await.unwrap().into_split();
        write.write_all(&at_limit).await.unwrap();
        let mut line = String::new();
        BufReader::new(read).read_line(&mut line).await.unwrap();
        let response: Response = serde_json::from_str(&line).unwrap();
        assert!(
            matches!(&response, Response::Error { code, message } if code == "error" && message.contains("malformed")),
            "{response:?}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn malformed_line_gets_error_response() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(&path).await;

        let (read, mut write) = UnixStream::connect(&path).await.unwrap().into_split();
        write.write_all(b"this is not a request\n").await.unwrap();
        let mut line = String::new();
        BufReader::new(read).read_line(&mut line).await.unwrap();
        let response: Response = serde_json::from_str(&line).unwrap();
        assert!(matches!(&response, Response::Error { code, .. } if code == "error"), "got {response:?}");

        // The malformed client did not take the server down with it.
        let response = client::call(&path, &Request::Ping).await.unwrap();
        assert_eq!(response, Response::Pong { version: "test".into() });
        server.abort();
    }

    /// Sends one raw line to the server at `path` and returns its answer.
    async fn send_line(path: &Path, line: &str) -> Response {
        let (read, mut write) = UnixStream::connect(path).await.expect("connects").into_split();
        write.write_all(format!("{line}\n").as_bytes()).await.expect("sends");
        let mut answer = String::new();
        BufReader::new(read).read_line(&mut answer).await.expect("reads the answer");
        serde_json::from_str(&answer).expect("the answer is a response")
    }

    /// A server whose handler answers with the key it was handed, as the error message.
    async fn echo_key_server(dir: &tempfile::TempDir) -> (PathBuf, tokio::task::JoinHandle<Result<(), IpcError>>) {
        let path = socket_in(dir);
        let handler = |key: Option<Key>, request: Request| async move {
            match request {
                Request::Ping => Response::Pong { version: "test".into() },
                _ => Response::Error {
                    code: "echo".into(),
                    message: key.map(|k| k.expose().to_string()).unwrap_or_default(),
                },
            }
        };
        let server = tokio::spawn({
            let path = path.clone();
            async move { serve_with_key(&path, handler).await }
        });
        wait_until_listening(&path).await;
        (path, server)
    }

    #[tokio::test]
    async fn the_handler_gets_the_key_of_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let (path, server) = echo_key_server(&dir).await;

        let key = Key::new("torro_c1_abc");
        let response = client::call_with_key(&path, Some(&key), &Request::Status).await.unwrap();
        assert_eq!(response, Response::Error { code: "echo".into(), message: "torro_c1_abc".into() });
        let response = client::call_with_key(&path, None, &Request::Status).await.unwrap();
        assert_eq!(response, Response::Error { code: "echo".into(), message: String::new() });
        server.abort();
    }

    #[tokio::test]
    async fn other_api_versions_and_bare_requests_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (path, server) = echo_key_server(&dir).await;

        let response = send_line(&path, r#"{"v":2,"request":{"method":"system.status"}}"#).await;
        assert!(
            matches!(&response, Response::Error { code, message } if code == "unsupported" && message.contains("version 2"))
        );
        // The request without an envelope, as cores before API v1 took it.
        let response = send_line(&path, r#"{"method":"system.status"}"#).await;
        assert!(matches!(&response, Response::Error { code, .. } if code == "error"), "{response:?}");
        let response = send_line(&path, r#"{"v":1,"request":{"method":"status"}}"#).await;
        assert!(
            matches!(&response, Response::Error { code, message } if code == "error" && message.contains("status"))
        );
        server.abort();
    }

    #[tokio::test]
    async fn errors_never_repeat_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let (path, server) = echo_key_server(&dir).await;

        for line in [
            r#"{"v":"torro_c1_secret","request":{"method":"system.status"}}"#,
            r#"{"v":1,"key":"torro_c1_secret","request":{"method":"nope"}}"#,
            r#"{"v":7,"key":"torro_c1_secret","request":{"method":"system.status"}}"#,
        ] {
            let response = send_line(&path, line).await;
            let Response::Error { code, message } = response else { panic!("{line} is refused") };
            assert!(code == "error" || code == "unsupported", "{line}: {code}");
            assert!(!message.contains("secret"), "{line}: {message}");
        }
        server.abort();
    }
}
