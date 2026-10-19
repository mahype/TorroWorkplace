//! The core's side of the socket (a named pipe on Windows).
//!
//! Task 1.17 in `docs/tasks/phase-1.md`; the envelope with version and key since A.1 in
//! `docs/tasks/api-mcp.md`; Windows since X.1 in `docs/tasks/plattformen.md`.
//!
//! One [`crate::Envelope`] line in, one [`Response`] line out, per connection. Every connection
//! runs in its own task, so a slow request does not hold up the others. This module only
//! carries the key and the way the request came ([`Door`], `via` in the envelope) to the handler;
//! deciding what a key may do is the core's job.
//!
//! Only the user who runs the core may connect: on Unix the socket is 0600, on Windows the pipe
//! carries an access list for that user alone. On Windows the pipe is created with
//! [`interprocess`], whose safe API covers the security descriptor; tokio's own call for that is
//! `unsafe`, which the workspace forbids.

use std::future::Future;
use std::path::Path;

use crate::{Door, IpcError, Key, Request, Response};

/// The longest request line the core reads, newline included. Longer lines are refused with
/// `error` and the connection is closed, so a client cannot make the core buffer without end.
pub const MAX_LINE: usize = 1024 * 1024;

/// Listens on `path` and answers each connection's request with `handler`, which gets the
/// request without its key.
///
/// Only for the test cores of this crate: a server built on this one lets every client in, so
/// nothing outside the tests may use it. The core uses [`serve_with_key`].
#[cfg(all(test, any(unix, windows)))]
pub(crate) async fn serve<H, F>(path: &Path, handler: H) -> Result<(), IpcError>
where
    H: Fn(Request) -> F + Send + Sync + 'static,
    F: Future<Output = Response> + Send + 'static,
{
    serve_with_key(path, move |_key, _door, request| handler(request)).await
}

/// Listens on `path` and answers each connection's request with `handler`, concurrently.
/// `handler` gets the key the client sent (`None` without one), the way it says the request came
/// ([`crate::Envelope::door`]: [`Door::Api`] when the envelope has no `via`) and must decide itself
/// whether that key may run the request that way. Runs until the task is dropped; failing to accept one client
/// is logged, not fatal.
///
/// Before the handler runs, the line must be an envelope of [`crate::API_VERSION`]: another
/// version is answered with `unsupported`, anything else that is not a request (or longer than
/// [`MAX_LINE`]) with `error`. The message never repeats the key.
///
/// Single instance: if a core answers a [`Request::Ping`] at `path`, returns
/// [`IpcError::AlreadyRunning`].
///
/// - Unix: `path` is a socket file. A stale one is removed. Creates the parent directory for the
///   owner only (0700) and makes the socket 0600.
/// - Windows: `path` is a named pipe (`\\.\pipe\…`, see [`crate::socket_path`]); any other path
///   is refused with [`IpcError::Io`] (`InvalidInput`). The pipe's access list lets only the
///   user running the core in (SDDL `D:P(A;;GA;;;<user SID>)`), remote clients are rejected, and
///   the first instance is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`: if another program
///   already holds the name, the core does not join it but fails with [`IpcError::Io`]
///   (`PermissionDenied`), so it never shares its clients and their keys with that program.
///   After each connection the next instance is opened, so many clients can connect at once.
/// - Elsewhere [`IpcError::Unsupported`].
#[cfg(unix)]
pub async fn serve_with_key<H, F>(path: &Path, handler: H) -> Result<(), IpcError>
where
    H: Fn(Option<Key>, Door, Request) -> F + Send + Sync + 'static,
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
            let (read, write) = stream.into_split();
            // A client that hangs up mid-request only ends its own connection.
            let _ = handle_connection(read, write, &*handler).await;
        });
    }
}

/// See the Unix variant above for the contract; this is the named pipe side of it.
#[cfg(windows)]
pub async fn serve_with_key<H, F>(path: &Path, handler: H) -> Result<(), IpcError>
where
    H: Fn(Option<Key>, Door, Request) -> F + Send + Sync + 'static,
    F: Future<Output = Response> + Send + 'static,
{
    use std::sync::Arc;

    if !crate::pipe::is_pipe_name(path) {
        let message = format!("{} is not a named pipe (\\\\.\\pipe\\…)", path.display());
        return Err(IpcError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, message)));
    }
    // Without a key: a ping needs none, and the app key stays out of a pipe that may be foreign.
    // Bounded, so a program that holds the name and never answers cannot stall the start; the
    // first-instance check below then refuses that name.
    let ping = crate::client::call_with_key(path, None, &Request::Ping);
    if let Ok(Ok(Response::Pong { .. })) = tokio::time::timeout(PING_WAIT, ping).await {
        return Err(IpcError::AlreadyRunning);
    }

    let listener = pipe_listener(path)?;
    let handler = Arc::new(handler);
    loop {
        // Waits for a client, then opens the next instance before handing this one out.
        let stream = match listener.accept().await {
            Ok(stream) => stream,
            // Out of handles and the like pass; keep serving instead of ending the core.
            Err(err) => {
                eprintln!("torroworkplace: accepting a client failed: {err}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
        };
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            // A client that hangs up mid-request only ends its own connection.
            let _ = handle_connection(&stream, &stream, &*handler).await;
        });
    }
}

/// How long a starting core waits for an answer from a core already on the pipe.
#[cfg(windows)]
const PING_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Neither Unix nor Windows: [`IpcError::Unsupported`].
#[cfg(not(any(unix, windows)))]
pub async fn serve_with_key<H, F>(path: &Path, handler: H) -> Result<(), IpcError>
where
    H: Fn(Option<Key>, Door, Request) -> F + Send + Sync + 'static,
    F: Future<Output = Response> + Send + 'static,
{
    let _ = (path, handler);
    Err(IpcError::Unsupported)
}

/// A pipe server on `path` that only the current user can reach, as the first instance of that
/// name (see [`serve_with_key`]).
#[cfg(windows)]
fn pipe_listener(
    path: &Path,
) -> Result<interprocess::os::windows::named_pipe::tokio::PipeListener<Bytes, Bytes>, IpcError> {
    use std::io::{Error, ErrorKind};

    use interprocess::os::windows::ToWtf16;
    use interprocess::os::windows::named_pipe::{PipeListenerOptions, PipeMode};
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;

    let nul = |_| IpcError::Io(Error::new(ErrorKind::InvalidInput, "a NUL character in the pipe name"));
    let sid = crate::pipe::user_sid().ok_or_else(|| IpcError::Io(Error::other("the user's SID is unknown")))?;
    let sddl = owner_only_sddl(sid).to_wtf_16().map_err(nul)?;
    let descriptor = SecurityDescriptor::deserialize(&sddl).map_err(IpcError::Io)?;
    let name = path.to_wtf_16().map_err(nul)?;

    // `create_tokio_duplex` sets FILE_FLAG_FIRST_PIPE_INSTANCE on this first instance, and
    // `accept_remote(false)` sets PIPE_REJECT_REMOTE_CLIENTS on every instance.
    PipeListenerOptions::new()
        .path(name)
        .mode(PipeMode::Bytes)
        .accept_remote(false)
        .security_descriptor(Some(descriptor))
        .create_tokio_duplex::<Bytes>()
        .map_err(|err| match err.kind() {
            ErrorKind::PermissionDenied => {
                let message = format!("the pipe {} is held by another program", path.display());
                IpcError::Io(Error::new(ErrorKind::PermissionDenied, message))
            }
            _ => IpcError::Io(err),
        })
}

/// Byte stream mode for both directions of the pipe: lines, as on the Unix socket.
#[cfg(windows)]
type Bytes = interprocess::os::windows::named_pipe::pipe_mode::Bytes;

/// The access list of the pipe in SDDL: protected (`P`, nothing inherited), one entry allowing
/// everything (`GA`) to `sid`, nothing to anyone else.
#[cfg(windows)]
fn owner_only_sddl(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
}

/// The envelope with the request still unparsed, so that a malformed request can be told apart
/// from a malformed envelope, and neither error quotes the key or a value of the request.
#[cfg(any(unix, windows))]
#[derive(serde::Deserialize)]
struct RawEnvelope {
    v: u32,
    #[serde(default)]
    key: Option<Key>,
    /// Missing: [`Door::Api`]. A value other than `api` or `mcp` makes the envelope malformed.
    #[serde(default)]
    via: Option<Door>,
    request: serde_json::Value,
}

/// The key, way and request of one line, or the code and message of the error response for it.
#[cfg(any(unix, windows))]
fn parse(line: &[u8]) -> Result<(Option<Key>, Door, Request), (&'static str, String)> {
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
    // Like the key above, no serde message: it can quote a value, e.g. a token in the wrong field.
    let request = serde_json::from_value(envelope.request).map_err(|_| {
        (
            "error",
            "malformed request: unknown method or arguments that do not fit it (see the API reference)".to_string(),
        )
    })?;
    Ok((envelope.key, envelope.via.unwrap_or_default(), request))
}

/// Reads the request line from `read`, answers it on `write`; the caller closes the connection.
///
/// A line that is not a request gets an error response instead of a silent close, so the
/// client learns what went wrong.
#[cfg(any(unix, windows))]
async fn handle_connection<R, W, H, F>(read: R, mut write: W, handler: &H) -> Result<(), IpcError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
    H: Fn(Option<Key>, Door, Request) -> F,
    F: Future<Output = Response>,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

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
            Ok((key, door, request)) => handler(key, door, request).await,
            Err((code, message)) => Response::Error { code: code.into(), message },
        }
    };
    let mut bytes = serde_json::to_vec(&response).map_err(|err| IpcError::Protocol(err.to_string()))?;
    bytes.push(b'\n');
    write.write_all(&bytes).await.map_err(IpcError::Io)?;
    // On a pipe the answer must reach the client before the instance closes; on a socket this
    // returns at once.
    write.flush().await.map_err(IpcError::Io)
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
            Request::Ping => Response::Pong { version: "test".into(), api: crate::API_VERSION },
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
        assert_eq!(response, Response::Pong { version: "test".into(), api: crate::API_VERSION });
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
                    Request::Ping => Response::Pong { version: "test".into(), api: crate::API_VERSION },
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
        assert_eq!(response, Response::Pong { version: "test".into(), api: crate::API_VERSION });
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
        assert_eq!(response, Response::Pong { version: "test".into(), api: crate::API_VERSION });
        server.abort();
    }

    #[tokio::test]
    async fn a_malformed_request_does_not_repeat_what_was_sent() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(&path).await;

        // A token in a field the method does not have, in place of a string, and as the method.
        for request in [
            r#"{"method":"accounts.set_github_token","tokn":"ghp_SECRET"}"#,
            r#"{"method":"accounts.connect_nextcloud","server":{"x":"ghp_SECRET"}}"#,
            r#"{"method":"ghp_SECRET"}"#,
        ] {
            let line = format!(r#"{{"v":{},"request":{request}}}"#, crate::API_VERSION);
            let response = send_line(&path, &line).await;
            let Response::Error { code, message } = response else { panic!("an error for {request}") };
            assert_eq!(code, "error");
            assert!(message.starts_with("malformed request"), "{message}");
            assert!(!message.contains("SECRET"), "{message}");
        }
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

    /// A server whose handler answers with the key it was handed as the error message, and with
    /// the way the request came as the code (`echo` for the API, `echo-mcp` for MCP).
    async fn echo_key_server(dir: &tempfile::TempDir) -> (PathBuf, tokio::task::JoinHandle<Result<(), IpcError>>) {
        let path = socket_in(dir);
        let handler = |key: Option<Key>, door: Door, request: Request| async move {
            match request {
                Request::Ping => Response::Pong { version: "test".into(), api: crate::API_VERSION },
                _ => Response::Error {
                    code: if door == Door::Mcp { "echo-mcp" } else { "echo" }.into(),
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

    /// #231: the handler learns the way the request came; without `via` it is the API.
    #[tokio::test]
    async fn the_handler_gets_the_way_of_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let (path, server) = echo_key_server(&dir).await;

        let key = Key::new("torro_c1_abc");
        let response = client::call_with_key_via(&path, Some(&key), Door::Mcp, &Request::Status).await.unwrap();
        assert_eq!(response, Response::Error { code: "echo-mcp".into(), message: "torro_c1_abc".into() });
        let response = client::call_with_key_via(&path, Some(&key), Door::Api, &Request::Status).await.unwrap();
        assert_eq!(response, Response::Error { code: "echo".into(), message: "torro_c1_abc".into() });
        let response = send_line(&path, r#"{"v":1,"via":"mcp","request":{"method":"system.status"}}"#).await;
        assert!(matches!(&response, Response::Error { code, .. } if code == "echo-mcp"), "{response:?}");
        let response = send_line(&path, r#"{"v":1,"request":{"method":"system.status"}}"#).await;
        assert!(matches!(&response, Response::Error { code, .. } if code == "echo"), "{response:?}");
        let response = send_line(&path, r#"{"v":1,"via":"http","request":{"method":"system.status"}}"#).await;
        assert!(matches!(&response, Response::Error { code, .. } if code == "error"), "an unknown way: {response:?}");
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
            matches!(&response, Response::Error { code, message } if code == "error" && message.contains("unknown method"))
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

#[cfg(all(test, windows))]
mod windows_tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};
    use tokio::sync::Barrier;
    use tokio::task::JoinHandle;

    use super::*;
    use crate::client;

    /// A pipe name of its own for every test, so tests running at once never meet.
    fn unique_pipe() -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        PathBuf::from(format!(r"\\.\pipe\torro-workplace-test-{}-{n}", std::process::id()))
    }

    fn pong() -> Response {
        Response::Pong { version: "test".into(), api: crate::API_VERSION }
    }

    /// Pong to `Ping`, `Ok` to everything else.
    async fn answer(request: Request) -> Response {
        match request {
            Request::Ping => pong(),
            _ => Response::Ok,
        }
    }

    /// Pings (without a key, so no app key of the real user is read) until the server answers.
    async fn wait_until_listening(path: &Path) {
        for _ in 0..200 {
            if matches!(client::call_with_key(path, None, &Request::Ping).await, Ok(Response::Pong { .. })) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("server did not start listening on {}", path.display());
    }

    /// A test core on a fresh pipe, already listening.
    async fn start(path: &Path) -> JoinHandle<Result<(), IpcError>> {
        let server = tokio::spawn({
            let path = path.to_path_buf();
            async move { serve(&path, answer).await }
        });
        wait_until_listening(path).await;
        server
    }

    #[tokio::test]
    async fn requests_go_there_and_back_with_their_key() {
        let path = unique_pipe();
        let handler = |key: Option<Key>, _door: Door, request: Request| async move {
            match request {
                Request::Ping => pong(),
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

        assert_eq!(client::call_with_key(&path, None, &Request::Ping).await.expect("a pong"), pong());
        let key = Key::new("torro_c1_abc");
        let response = client::call_with_key(&path, Some(&key), &Request::Status).await.expect("an answer");
        assert_eq!(response, Response::Error { code: "echo".into(), message: "torro_c1_abc".into() });
        server.abort();
    }

    #[tokio::test]
    async fn many_clients_one_after_the_other() {
        let path = unique_pipe();
        let server = start(&path).await;

        for _ in 0..20 {
            assert_eq!(client::call_with_key(&path, None, &Request::Status).await.expect("an answer"), Response::Ok);
        }
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
                    Request::Ping => pong(),
                    _ => {
                        barrier.wait().await;
                        Response::Ok
                    }
                }
            }
        };
        let path = unique_pipe();
        let server = tokio::spawn({
            let path = path.clone();
            async move { serve(&path, handler).await }
        });
        wait_until_listening(&path).await;

        let (first, second) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                client::call_with_key(&path, None, &Request::Status),
                client::call_with_key(&path, None, &Request::ListTemplates)
            )
        })
        .await
        .expect("requests were handled one after the other");
        assert_eq!(first.expect("an answer"), Response::Ok);
        assert_eq!(second.expect("an answer"), Response::Ok);
        server.abort();
    }

    #[tokio::test]
    async fn many_clients_at_once() {
        let path = unique_pipe();
        let server = start(&path).await;

        let calls = (0..16).map(|_| {
            let path = path.clone();
            tokio::spawn(async move { client::call_with_key(&path, None, &Request::Status).await })
        });
        for call in calls.collect::<Vec<_>>() {
            assert_eq!(call.await.expect("the task ends").expect("an answer"), Response::Ok);
        }
        server.abort();
    }

    #[tokio::test]
    async fn second_server_reports_already_running() {
        let path = unique_pipe();
        let server = start(&path).await;

        let second = serve(&path, answer).await;
        assert!(matches!(second, Err(IpcError::AlreadyRunning)), "got {second:?}");

        // The first core is still the one answering.
        assert_eq!(client::call_with_key(&path, None, &Request::Ping).await.expect("a pong"), pong());
        server.abort();
    }

    #[tokio::test]
    async fn a_name_held_by_another_program_is_refused() {
        // Another program holds the name and never answers.
        let path = unique_pipe();
        let squatter = ServerOptions::new().first_pipe_instance(true).create(&path).expect("creates the pipe");

        let result = tokio::time::timeout(Duration::from_secs(10), serve(&path, answer))
            .await
            .expect("the core gives up instead of waiting");
        assert!(
            matches!(&result, Err(IpcError::Io(err)) if err.kind() == std::io::ErrorKind::PermissionDenied),
            "got {result:?}"
        );
        drop(squatter);
    }

    #[tokio::test]
    async fn a_path_that_is_no_pipe_is_refused() {
        let dir = tempfile::tempdir().expect("a temp directory");
        let result = serve(&dir.path().join("core.sock"), answer).await;
        assert!(
            matches!(&result, Err(IpcError::Io(err)) if err.kind() == std::io::ErrorKind::InvalidInput),
            "got {result:?}"
        );
    }

    #[tokio::test]
    async fn malformed_line_gets_error_response() {
        let path = unique_pipe();
        let server = start(&path).await;

        let (read, mut write) = tokio::io::split(ClientOptions::new().open(&path).expect("connects"));
        write.write_all(b"this is not a request\n").await.expect("sends");
        let mut line = String::new();
        BufReader::new(read).read_line(&mut line).await.expect("reads the answer");
        let response: Response = serde_json::from_str(&line).expect("the answer is a response");
        assert!(matches!(&response, Response::Error { code, .. } if code == "error"), "got {response:?}");

        // The malformed client did not take the server down with it.
        assert_eq!(client::call_with_key(&path, None, &Request::Ping).await.expect("a pong"), pong());
        server.abort();
    }

    #[test]
    fn the_access_list_names_only_the_user() {
        assert_eq!(owner_only_sddl("S-1-5-21-1-2-3-1001"), "D:P(A;;GA;;;S-1-5-21-1-2-3-1001)");
    }
}
