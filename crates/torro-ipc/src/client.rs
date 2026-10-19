//! The clients' side of the socket (a named pipe on Windows).
//!
//! Task 1.17 in `docs/tasks/phase-1.md`; the key in every request since A.1 in
//! `docs/tasks/api-mcp.md`; Windows since X.1 in `docs/tasks/plattformen.md`.
//!
//! Each call opens its own connection, sends one [`crate::Envelope`] line and reads one response
//! line. The suite's own programs use [`call`], which reads the app key of the core, so they
//! never deal with keys; scripts bring their own key to [`call_with_key`], the MCP server and the
//! hooks of agents theirs to [`call_with_key_via`] with [`Door::Mcp`] (#231).
//!
//! On Windows the client checks who serves the pipe before it sends anything (X.1b): the pipe
//! name is fixed and known, so while the core is not running another user could hold it.

use std::path::Path;

use crate::{Door, IpcError, Key, Request, Response, app_key_path};

/// Sends `request` to the core at `path` with the app key and waits for its response.
/// [`IpcError::NotRunning`] when nothing listens there.
///
/// The key is read on every call from `app.key` of the core ([`app_key_path`]), because
/// the core creates it on its first start, which may be after this program started. Without a
/// readable key the request goes out without one; the core then answers only `system.ping` and
/// everything else with `unauthorized`.
pub async fn call(path: &Path, request: &Request) -> Result<Response, IpcError> {
    let key = read_app_key(path);
    call_with_key(path, key.as_ref(), request).await
}

/// Sends `request` with `key` (or none) to the core at `path` and waits for its response.
/// [`IpcError::NotRunning`] when nothing listens there. The envelope names no way (`via`), so the
/// core takes it as come through the API ([`Door::Api`]).
///
/// `path` is a socket file on Unix and a named pipe (`\\.\pipe\…`, see [`crate::socket_path`])
/// on Windows; on Windows any other path counts as nothing listening, so a stray path never
/// opens a file or device, and a pipe whose server runs as another user (or whose owner cannot be
/// found out) gets nothing, not even a request without key: [`IpcError::Io`] with
/// [`std::io::ErrorKind::PermissionDenied`]. Elsewhere [`IpcError::Unsupported`].
pub async fn call_with_key(path: &Path, key: Option<&Key>, request: &Request) -> Result<Response, IpcError> {
    send(path, key, None, request).await
}

/// [`call_with_key`] that tells the core the way the request came (`via`, #231): `torroworkplace
/// mcp` and `torroworkplace hook` send [`Door::Mcp`], so the core lets their agents' keys in only
/// while "Allow MCP" is on and refuses a script's key there. Everything else is as for
/// [`call_with_key`].
pub async fn call_with_key_via(
    path: &Path,
    key: Option<&Key>,
    door: Door,
    request: &Request,
) -> Result<Response, IpcError> {
    send(path, key, Some(door), request).await
}

/// Connects to the core at `path` and exchanges one envelope with `key`, `via` and `request` for
/// its response; see [`call_with_key`].
#[cfg(unix)]
async fn send(path: &Path, key: Option<&Key>, via: Option<Door>, request: &Request) -> Result<Response, IpcError> {
    use std::io::ErrorKind;

    use tokio::net::UnixStream;

    // A stale socket file with no listener behind it refuses the connection, like a missing one.
    let stream = match UnixStream::connect(path).await {
        Ok(stream) => stream,
        Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {
            return Err(IpcError::NotRunning);
        }
        Err(err) => return Err(IpcError::Io(err)),
    };
    let (read, write) = stream.into_split();
    exchange(read, write, key, via, request).await
}

/// The named pipe side of [`send`] above.
#[cfg(windows)]
async fn send(path: &Path, key: Option<&Key>, via: Option<Door>, request: &Request) -> Result<Response, IpcError> {
    let pipe = open_pipe(path).await?;
    let (read, write) = tokio::io::split(pipe);
    exchange(read, write, key, via, request).await
}

/// Neither Unix nor Windows: [`IpcError::Unsupported`].
#[cfg(not(any(unix, windows)))]
async fn send(path: &Path, key: Option<&Key>, via: Option<Door>, request: &Request) -> Result<Response, IpcError> {
    let _ = (path, key, via, request);
    Err(IpcError::Unsupported)
}

/// How long a client waits while every instance of the pipe is connected. The core opens the
/// next instance right after each connection, so this only bridges that moment.
#[cfg(windows)]
const PIPE_BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// `ERROR_PIPE_BUSY` from `winerror.h`: the pipe exists, but all its instances are connected.
#[cfg(windows)]
const ERROR_PIPE_BUSY: i32 = 231;

/// Connects to the named pipe `path`, waiting up to [`PIPE_BUSY_WAIT`] while it is busy, and
/// hands the pipe out only when its server runs as this user ([`check_owner`]).
///
/// The client keeps tokio's default security quality of service (identification only), so a
/// program that holds the pipe name cannot act as this user through the connection.
#[cfg(windows)]
async fn open_pipe(path: &Path) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, IpcError> {
    use std::io::ErrorKind;
    use std::time::Duration;

    use tokio::net::windows::named_pipe::ClientOptions;
    use tokio::time::Instant;

    if !crate::pipe::is_pipe_name(path) {
        return Err(IpcError::NotRunning);
    }
    let deadline = Instant::now() + PIPE_BUSY_WAIT;
    loop {
        match ClientOptions::new().open(path) {
            Ok(pipe) => return check_owner(&pipe, path).map(|()| pipe),
            Err(err) if err.kind() == ErrorKind::NotFound => return Err(IpcError::NotRunning),
            Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(err) => return Err(IpcError::Io(err)),
        }
    }
}

/// Refuses `pipe` unless its server process runs as this user, before anything is sent.
///
/// The access list of the core's pipe keeps other users from connecting, but while the core is
/// not running, a program of another user could create a pipe of the same name and receive the
/// request with the app key. The name alone proves nothing, so the server's process decides:
/// [`IpcError::Io`] with [`std::io::ErrorKind::PermissionDenied`] (as the core reports a name held
/// by another program) when it belongs to someone else or its owner cannot be found out.
#[cfg(windows)]
fn check_owner(pipe: &tokio::net::windows::named_pipe::NamedPipeClient, path: &Path) -> Result<(), IpcError> {
    use std::io::{Error, ErrorKind};

    let server = server_process_id(pipe).and_then(crate::pipe::process_sid);
    if is_same_user(crate::pipe::user_sid(), server.as_deref()) {
        return Ok(());
    }
    let message = format!("the pipe {} belongs to another user", path.display());
    Err(IpcError::Io(Error::new(ErrorKind::PermissionDenied, message)))
}

/// The id of the process that serves the other end of `pipe`, `None` when Windows cannot tell.
///
/// The one FFI call of this crate: neither tokio nor `sysinfo` offers it, and `interprocess`,
/// which does, opens client pipes without security quality of service, so its server could act
/// as this user.
#[cfg(windows)]
fn server_process_id(pipe: &tokio::net::windows::named_pipe::NamedPipeClient) -> Option<u32> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;

    let mut pid: u32 = 0;
    #[allow(unsafe_code)]
    // SAFETY: the handle belongs to `pipe`, which the borrow keeps open for the whole call, and
    // `pid` is a valid, writable `u32` that outlives it. The call only writes that one value.
    let ok = unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) };
    (ok != 0 && pid != 0).then_some(pid)
}

/// Whether the server of a pipe runs as this user: `own` (this process's SID) and `server` (the
/// SID of the pipe's server process) are both known, not empty and equal. Anything unknown counts
/// as another user, so a client that cannot tell sends nothing.
#[cfg(any(windows, test))]
fn is_same_user(own: Option<&str>, server: Option<&str>) -> bool {
    matches!((own, server), (Some(own), Some(server)) if !own.is_empty() && own == server)
}

/// Writes the envelope line of `request` to `write` and reads the one response line from `read`.
#[cfg(any(unix, windows))]
async fn exchange<R, W>(
    read: R,
    mut write: W,
    key: Option<&Key>,
    via: Option<Door>,
    request: &Request,
) -> Result<Response, IpcError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use crate::{API_VERSION, Envelope};

    let envelope = Envelope { v: API_VERSION, key: key.cloned(), via, request: request.clone() };
    let mut bytes = serde_json::to_vec(&envelope).map_err(|err| IpcError::Protocol(err.to_string()))?;
    bytes.push(b'\n');
    write.write_all(&bytes).await.map_err(IpcError::Io)?;

    let mut line = Vec::new();
    if BufReader::new(read).read_until(b'\n', &mut line).await.map_err(IpcError::Io)? == 0 {
        return Err(IpcError::Protocol("core closed the connection without answering".into()));
    }
    serde_json::from_slice(&line).map_err(|err| IpcError::Protocol(err.to_string()))
}

/// The app key of the core at `socket` ([`app_key_path`]), `None` when it is missing,
/// unreadable or empty.
fn read_app_key(socket: &Path) -> Option<Key> {
    let key = std::fs::read_to_string(app_key_path(socket)).ok()?;
    let key = key.trim();
    (!key.is_empty()).then(|| Key::new(key))
}

#[cfg(test)]
mod owner_tests {
    use super::is_same_user;

    #[test]
    fn only_the_same_known_user_passes() {
        let me = "S-1-5-21-3623811015-3361044348-30300820-1013";
        assert!(is_same_user(Some(me), Some(me)));

        let other = "S-1-5-21-3623811015-3361044348-30300820-1014";
        assert!(!is_same_user(Some(me), Some(other)));
        assert!(!is_same_user(Some(me), None), "an owner that cannot be found out");
        assert!(!is_same_user(None, Some(me)), "an own SID that cannot be found out");
        assert!(!is_same_user(None, None));
        assert!(!is_same_user(Some(""), Some("")));
    }
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

#[cfg(all(test, windows))]
mod windows_tests {
    use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

    use super::*;

    #[tokio::test]
    async fn a_pipe_of_this_user_passes_the_owner_check() {
        let name = format!(r"\\.\pipe\torro-workplace-test-owner-{}", std::process::id());
        let server = ServerOptions::new().first_pipe_instance(true).create(&name).expect("creates the pipe");
        let client = ClientOptions::new().open(&name).expect("connects");

        assert_eq!(server_process_id(&client), Some(std::process::id()));
        check_owner(&client, Path::new(&name)).expect("this process serves the pipe");
        drop(server);
    }

    #[tokio::test]
    async fn missing_pipe_is_not_running() {
        let name = format!(r"\\.\pipe\torro-workplace-test-missing-{}", std::process::id());

        let result = call_with_key(Path::new(&name), None, &Request::Ping).await;
        assert!(matches!(result, Err(IpcError::NotRunning)), "got {result:?}");
    }

    #[tokio::test]
    async fn a_path_that_is_no_pipe_counts_as_not_running() {
        // Even an existing file: the client never opens anything but a pipe.
        let dir = tempfile::tempdir().expect("a temp directory");
        let file = dir.path().join("core.sock");
        std::fs::write(&file, b"not a pipe").expect("writes the file");

        let result = call_with_key(&file, None, &Request::Ping).await;
        assert!(matches!(result, Err(IpcError::NotRunning)), "got {result:?}");
    }
}
