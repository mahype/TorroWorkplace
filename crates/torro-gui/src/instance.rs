//! One process per kind of window (issue #279, decision 5 in `docs/lebenszyklus.md`).
//!
//! Before its first window an app asks whether its kind of window already runs ([`claim`]). The way
//! to ask is a local channel per [`AppId::window_class`] ([`channel_path`]): a Unix socket
//! `<window class>.instance` in the app's state directory, on Windows the named pipe
//! `\\.\pipe\torro-<window class>-<user SID>`. When someone answers there, the new start sends the
//! line `show`, gets `ok` back and ends with exit code 0 without a window ([`Claim::Shown`]); the
//! running app brings its window to the front ([`subscription`], [`bring_to_front`]). Otherwise the
//! new start listens itself ([`Claim::First`]). A socket file nobody listens on (an app that crashed)
//! is replaced.
//!
//! The channel can only ever bring a window to the front. The listening app reads at most
//! [`MAX_LINE`] bytes, takes exactly `show` and drops everything else; arguments, targets, paths and
//! environment never go over it, and its answer holds no state. Only the user reaches it: on Unix the
//! socket is 0600 in a 0700 directory, on Windows the pipe has the access list of the core's pipe
//! (the user alone, [`torro_ipc::pipe::user_only_listener`]) and refuses remote clients, and the
//! client connects with the security quality of service "identification", so a program holding the
//! name cannot act as the user. Whoever holds the name can keep a window from opening, nothing more.
//! Anything that goes wrong (no state directory, a path too long for a socket, a name held by a
//! program that does not answer) starts the app without a channel ([`Claim::Without`]), as before.
//!
//! The channel runs on a thread of its own with a small tokio runtime, so it answers a second start
//! at once, also while the window is still opening; its `show` waits in [`subscription`] until the
//! window takes it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::futures::Stream;
use iced::{Subscription, Task};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use torro_core::AppId;

/// The only line the channel takes: bring the window to the front.
const SHOW: &[u8] = b"show\n";

/// The listening app's answer to [`SHOW`]: it took it. Carries no state.
const OK: &[u8] = b"ok\n";

/// The most bytes the listening app reads from one connection; anything longer is not [`SHOW`].
pub const MAX_LINE: usize = 16;

/// How long either side waits for the other's line before it gives up on the connection.
const WAIT: Duration = Duration::from_secs(1);

/// How often a start tries to either reach a running app or listen itself: another start may take
/// the name between one look and the next.
const TRIES: usize = 3;

/// What [`claim`] found out about this kind of window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// It already runs in another process, which got `show` and answered. The caller opens no
    /// window and ends with exit code 0.
    Shown,
    /// This process is the first: it listens now, and [`subscription`] reports each `show`.
    First,
    /// No channel (see the module docs for why that can happen): start as before. A second start
    /// then opens a second window.
    Without,
}

/// Asks whether `app`'s kind of window already runs and, if not, listens for later starts of it.
/// Call it in `main` before the first window and before the core is started, and only for starts
/// that show the app's plain window: not for one with a target (`--task`, `--message`, …), a helper
/// (`--compose`, `--listen`, …) or `--demo`.
///
/// [`Claim::Shown`]: the running app got `show`; end with exit code 0. [`Claim::First`] and
/// [`Claim::Without`]: open the window. Never fails; problems are written to stderr and give
/// [`Claim::Without`]. Each `show` that comes in later reaches [`subscription`] of this process.
pub fn claim(app: AppId) -> Claim {
    match channel_path(app) {
        Some(path) => claim_at(&path, || SHOWN.notify_one()),
        None => Claim::Without,
    }
}

/// The channel of `app`'s kind of window: on Unix `<window class>.instance` in the state directory
/// of `app` ([`torro_core::paths::AppPaths::for_app`]), on Windows the named pipe
/// `\\.\pipe\torro-<window class>-<user SID>`. `None` without a home directory or a SID.
pub fn channel_path(app: AppId) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        torro_ipc::pipe::user_sid().map(|sid| PathBuf::from(format!(r"\\.\pipe\torro-{}-{sid}", app.window_class)))
    }
    #[cfg(not(windows))]
    {
        let paths = torro_core::paths::AppPaths::for_app(app)?;
        Some(paths.state.join(format!("{}.instance", app.window_class)))
    }
}

/// Reports each `show` that reached this process's channel ([`claim`]), at most one at a time: two
/// that come before the window takes the first count once. Nothing without a channel. Apps add it
/// to their subscriptions and answer with [`bring_to_front`].
pub fn subscription() -> Subscription<()> {
    Subscription::run(shows)
}

/// The task that brings the app's window to the front: out of the Dock or taskbar if it is
/// minimized, then focused (`iced::window::gain_focus`). Whether the system lets an app take the
/// focus is the system's choice (macOS may only make the icon bounce).
pub fn bring_to_front<T: Send + 'static>() -> Task<T> {
    iced::window::latest().and_then(|id| iced::window::minimize(id, false).chain(iced::window::gain_focus(id)))
}

/// Where [`claim`]'s channel hands each `show` to [`subscription`]. A permit waits there until the
/// window takes it, so a `show` during the start is not lost.
static SHOWN: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// The stream of [`subscription`]: one item per wake-up of [`SHOWN`].
fn shows() -> impl Stream<Item = ()> {
    iced::futures::stream::unfold((), |()| async {
        SHOWN.notified().await;
        Some(((), ()))
    })
}

/// [`claim`] on the channel at `path`, calling `on_show` (on the channel's thread) for each `show`.
pub(crate) fn claim_at(path: &Path, on_show: impl Fn() + Send + 'static) -> Claim {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => return without(path, &err.to_string()),
    };
    let listener = match runtime.block_on(channel::open(path)) {
        Opening::Shown => return Claim::Shown,
        Opening::Without(reason) => return without(path, &reason),
        Opening::Listening(listener) => listener,
    };
    let path_for_thread = path.to_path_buf();
    let thread = std::thread::Builder::new()
        .name("torro-instance".into())
        .spawn(move || runtime.block_on(channel::serve(&path_for_thread, listener, on_show)));
    match thread {
        Ok(_) => Claim::First,
        Err(err) => without(path, &err.to_string()),
    }
}

/// Writes why the channel at `path` is not used and answers [`Claim::Without`].
fn without(path: &Path, reason: &str) -> Claim {
    eprintln!("starting without the single-instance channel {}: {reason}", path.display());
    Claim::Without
}

/// What a start found at the channel.
enum Opening<L> {
    /// A running app took `show`.
    Shown,
    /// Nobody was there; this process listens with `L` now.
    Listening(L),
    /// The channel cannot be used, and why.
    Without(String),
}

/// The new start's side: sends [`SHOW`] over `stream` and answers whether [`OK`] came back in time.
async fn knock<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S) -> bool {
    let exchange = async {
        stream.write_all(SHOW).await.ok()?;
        let mut answer = Vec::new();
        BufReader::new((&mut stream).take(OK.len() as u64)).read_until(b'\n', &mut answer).await.ok()?;
        Some(answer == OK)
    };
    matches!(tokio::time::timeout(WAIT, exchange).await, Ok(Some(true)))
}

/// What a knock that reached someone means: [`Opening::Shown`] when they answered, else the channel
/// is held by something that does not, and the app starts without it.
fn knocked<L>(answered: bool) -> Opening<L> {
    if answered { Opening::Shown } else { Opening::Without("the program there does not answer".into()) }
}

/// The running app's side of one connection: reads at most [`MAX_LINE`] bytes up to a newline,
/// and only for exactly [`SHOW`] calls `on_show` and answers [`OK`]. Anything else, or nothing
/// within [`WAIT`], gets no answer.
async fn answer<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, on_show: &(impl Fn() + ?Sized)) {
    let exchange = async {
        let mut line = Vec::new();
        BufReader::new((&mut stream).take(MAX_LINE as u64)).read_until(b'\n', &mut line).await.ok()?;
        if line != SHOW {
            return None;
        }
        on_show();
        stream.write_all(OK).await.ok()
    };
    let _ = tokio::time::timeout(WAIT, exchange).await;
}

/// The Unix socket side of the channel.
#[cfg(unix)]
mod channel {
    use std::io::ErrorKind;
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
    use std::path::Path;

    use tokio::net::{UnixListener, UnixStream};

    use super::{Opening, TRIES, answer, knock, knocked};

    /// The longest socket path the system takes, without its closing NUL: `sun_path` holds 108
    /// bytes on Linux and 104 on macOS and the BSDs.
    const SUN_PATH: usize = if cfg!(any(target_os = "linux", target_os = "android")) { 108 } else { 104 };

    /// Whether `path` fits into `sun_path` with its closing NUL. A longer one cannot be bound, so the
    /// app starts without a channel instead of failing.
    pub(super) fn fits_sun_path(path: &Path) -> bool {
        path.as_os_str().len() < SUN_PATH
    }

    /// Reaches the app listening at `path` or listens there: knocks first; when nobody listens,
    /// removes what is left at `path` (a crashed app's socket, or anything that is no socket) and binds a socket of its own, 0600 in
    /// a 0700 directory ([`private_dir`]).
    pub(super) async fn open(path: &Path) -> Opening<UnixListener> {
        if !fits_sun_path(path) {
            return Opening::Without("the path is too long for a socket".into());
        }
        if let Some(dir) = path.parent()
            && let Err(err) = private_dir(dir)
        {
            return Opening::Without(err.to_string());
        }
        for _ in 0..TRIES {
            // Only a socket is knocked on; anything else at `path` (a file, a link) is replaced.
            let socket = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_socket());
            if socket {
                match UnixStream::connect(path).await {
                    Ok(stream) => return knocked(knock(stream).await),
                    // Gone meanwhile, or a socket with nobody behind it.
                    Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) => {}
                    Err(err) => return Opening::Without(err.to_string()),
                }
            }
            if let Err(err) = std::fs::remove_file(path)
                && err.kind() != ErrorKind::NotFound
            {
                return Opening::Without(err.to_string());
            }
            match UnixListener::bind(path) {
                Ok(listener) => {
                    let private = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
                    return match private {
                        Ok(()) => Opening::Listening(listener),
                        Err(err) => Opening::Without(err.to_string()),
                    };
                }
                // Another start bound it a moment ago: knock there.
                Err(err) if err.kind() == ErrorKind::AddrInUse => {}
                Err(err) => return Opening::Without(err.to_string()),
            }
        }
        Opening::Without("other starts keep taking the channel".into())
    }

    /// Creates `dir` for its owner only (0700) and takes group and others off it if it exists. It is
    /// the app's own state directory; some systems do not check a socket's own mode on connect, so
    /// the directory keeps other users out.
    fn private_dir(dir: &Path) -> std::io::Result<()> {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        let mode = std::fs::metadata(dir)?.permissions().mode();
        if mode & 0o077 != 0 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    /// Answers every connection to `listener` in turn ([`answer`]) until the process ends.
    pub(super) async fn serve(path: &Path, listener: UnixListener, on_show: impl Fn() + Send + 'static) {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => answer(stream, &on_show).await,
                // Out of file descriptors and the like pass; the window stays either way.
                Err(err) => {
                    eprintln!("the single-instance channel {} failed to accept: {err}", path.display());
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }
}

/// The named pipe side of the channel (Windows).
#[cfg(windows)]
mod channel {
    use std::io::ErrorKind;
    use std::path::Path;
    use std::time::Duration;

    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
    use tokio::time::Instant;
    use torro_ipc::pipe::UserOnlyListener;

    use super::{Opening, TRIES, WAIT, answer, knock, knocked};

    /// `SECURITY_IDENTIFICATION` from `winnt.h`: the server of the pipe may learn who the client is,
    /// but not act as the client. tokio adds `SECURITY_SQOS_PRESENT`.
    const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;

    /// `ERROR_PIPE_BUSY` from `winerror.h`: the pipe exists, but all its instances are connected.
    const ERROR_PIPE_BUSY: i32 = 231;

    /// Reaches the app listening on the pipe `path` or creates it: knocks first; when there is no
    /// such pipe, creates it for the user alone as its first instance
    /// ([`torro_ipc::pipe::user_only_listener`]: the access list of the core's pipe, remote clients
    /// refused, `FILE_FLAG_FIRST_PIPE_INSTANCE`), so a name someone else holds is never joined.
    pub(super) async fn open(path: &Path) -> Opening<UserOnlyListener> {
        for _ in 0..TRIES {
            match connect(path).await {
                Ok(Some(pipe)) => return knocked(knock(pipe).await),
                Ok(None) => {}
                Err(err) => return Opening::Without(err.to_string()),
            }
            match torro_ipc::pipe::user_only_listener(path) {
                Ok(listener) => return Opening::Listening(listener),
                // Another start created it a moment ago (or someone holds it): knock there.
                Err(err) if err.kind() == ErrorKind::PermissionDenied => {}
                Err(err) => return Opening::Without(err.to_string()),
            }
        }
        Opening::Without("the name is taken by a program that does not answer".into())
    }

    /// Opens the pipe `path` as a client with the security quality of service "identification";
    /// `None` when there is no such pipe. Waits up to [`WAIT`] while all its instances are busy.
    async fn connect(path: &Path) -> std::io::Result<Option<NamedPipeClient>> {
        let deadline = Instant::now() + WAIT;
        loop {
            match ClientOptions::new().security_qos_flags(SECURITY_IDENTIFICATION).open(path) {
                Ok(pipe) => return Ok(Some(pipe)),
                Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
                Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Answers every client of the pipe `path` in turn ([`answer`]) until the process ends. Each
    /// `accept` opens the next instance, with the same access list, before it hands out a client.
    pub(super) async fn serve(path: &Path, listener: UserOnlyListener, on_show: impl Fn() + Send + 'static) {
        loop {
            match listener.accept().await {
                Ok(client) => answer(&client, &on_show).await,
                // Out of handles and the like pass; the window stays either way.
                Err(err) => {
                    eprintln!("the single-instance channel {} failed to accept: {err}", path.display());
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
}

/// Neither Unix nor Windows: no channel.
#[cfg(not(any(unix, windows)))]
mod channel {
    use std::path::Path;

    use super::Opening;

    pub(super) async fn open(_path: &Path) -> Opening<()> {
        Opening::Without("no channel on this system".into())
    }

    pub(super) async fn serve(_path: &Path, _listener: (), _on_show: impl Fn() + Send + 'static) {}
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;

    use super::*;

    /// A directory under the system temp dir, removed again on drop (torro-gui has no `tempfile`).
    /// Short names: the socket path must fit into `sun_path`.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("ti-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Claims `path` and hands out the `show`s it gets.
    fn claim_with_inbox(path: &Path) -> (Claim, mpsc::Receiver<()>) {
        let (sender, inbox) = mpsc::channel();
        let claim = claim_at(path, move || {
            let _ = sender.send(());
        });
        (claim, inbox)
    }

    /// Writes `line` to the channel at `path` as a foreign program would and returns its answer.
    fn send_raw(path: &Path, line: &[u8]) -> Vec<u8> {
        use std::io::{Read, Write};

        let mut stream = std::os::unix::net::UnixStream::connect(path).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(3))).expect("timeout");
        stream.write_all(line).expect("write");
        let _ = stream.shutdown(std::net::Shutdown::Write);
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
        answer
    }

    #[test]
    fn the_first_start_listens_and_the_second_shows_it() {
        let dir = TempDir::new("two");
        let path = dir.0.join("torro-calendar.instance");

        let (first, shows) = claim_with_inbox(&path);
        assert_eq!(first, Claim::First);
        assert!(shows.try_recv().is_err(), "nothing shown yet");

        let (second, second_shows) = claim_with_inbox(&path);
        assert_eq!(second, Claim::Shown, "the second start ends");
        shows.recv_timeout(Duration::from_secs(3)).expect("the first got show");
        assert!(second_shows.try_recv().is_err(), "the second listens on nothing");

        assert_eq!(claim_with_inbox(&path).0, Claim::Shown, "and a third");
        shows.recv_timeout(Duration::from_secs(3)).expect("show again");
    }

    #[test]
    fn a_socket_left_by_a_crashed_app_is_replaced() {
        let dir = TempDir::new("stale");
        let path = dir.0.join("torro-tasks.instance");
        drop(std::os::unix::net::UnixListener::bind(&path).expect("a socket nobody listens on"));
        assert!(path.exists(), "the file stays behind");

        let (claim, shows) = claim_with_inbox(&path);
        assert_eq!(claim, Claim::First);
        assert_eq!(claim_with_inbox(&path).0, Claim::Shown);
        shows.recv_timeout(Duration::from_secs(3)).expect("the new listener got show");
    }

    #[test]
    fn a_plain_file_in_its_place_is_replaced() {
        let dir = TempDir::new("file");
        let path = dir.0.join("torro-inbox.instance");
        std::fs::write(&path, "left over").expect("a file");

        assert_eq!(claim_with_inbox(&path).0, Claim::First);
        assert_eq!(claim_with_inbox(&path).0, Claim::Shown);
    }

    #[test]
    fn different_window_classes_do_not_meet() {
        let dir = TempDir::new("kinds");
        let (hub, hub_shows) = claim_with_inbox(&dir.0.join("torro-inbox.instance"));
        let (mail, mail_shows) = claim_with_inbox(&dir.0.join("torro-inbox-mail.instance"));
        assert_eq!((hub, mail), (Claim::First, Claim::First));

        assert_eq!(claim_with_inbox(&dir.0.join("torro-inbox-mail.instance")).0, Claim::Shown);
        mail_shows.recv_timeout(Duration::from_secs(3)).expect("the mail window got show");
        assert!(hub_shows.try_recv().is_err(), "the hub did not");
    }

    #[test]
    fn only_the_exact_line_show_is_taken() {
        let dir = TempDir::new("lines");
        let path = dir.0.join("torro-assistant.instance");
        let (claim, shows) = claim_with_inbox(&path);
        assert_eq!(claim, Claim::First);

        let long = format!("{}\n", "show".repeat(MAX_LINE));
        for line in [&b"Show\n"[..], b"show", b"show \n", b"show now\n", b"--task=1\n", b"", long.as_bytes()] {
            assert_eq!(send_raw(&path, line), b"", "no answer to {line:?}");
        }
        assert!(shows.try_recv().is_err(), "nothing shown");

        assert_eq!(send_raw(&path, SHOW), OK, "the one line it takes, answered without state");
        shows.recv_timeout(Duration::from_secs(3)).expect("shown");
    }

    #[test]
    fn a_silent_listener_does_not_hold_up_the_start() {
        let dir = TempDir::new("silent");
        let path = dir.0.join("torro-calendar.instance");
        let _squatter = std::os::unix::net::UnixListener::bind(&path).expect("a listener that never answers");

        let started = std::time::Instant::now();
        assert_eq!(claim_with_inbox(&path).0, Claim::Without, "starts without a channel");
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    #[test]
    fn the_socket_is_the_owners_alone() {
        let dir = TempDir::new("mode");
        let state = dir.0.join("state");
        std::fs::create_dir(&state).expect("state dir");
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).expect("open dir");
        let path = state.join("torro-workplace.instance");

        assert_eq!(claim_with_inbox(&path).0, Claim::First);
        let mode = |path: &Path| std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&state), 0o700, "an existing directory is closed to others");

        let fresh = dir.0.join("n/d");
        assert_eq!(claim_with_inbox(&fresh.join("torro-tasks.instance")).0, Claim::First);
        assert_eq!(mode(&fresh), 0o700, "a new one is created closed");
    }

    #[test]
    fn a_path_too_long_for_a_socket_starts_without_a_channel() {
        let dir = TempDir::new("long");
        let path = dir.0.join("d".repeat(120)).join("torro-calendar.instance");
        assert!(!channel::fits_sun_path(&path));
        assert_eq!(claim_with_inbox(&path).0, Claim::Without);
        assert!(!path.exists(), "nothing was made");

        assert!(channel::fits_sun_path(Path::new("/tmp/torro-calendar.instance")));
        assert!(channel::fits_sun_path(&PathBuf::from(format!("/{}", "a".repeat(102)))));
        assert!(!channel::fits_sun_path(&PathBuf::from(format!("/{}", "a".repeat(108)))));
    }

    /// A `show` that comes while the window is still opening waits for the subscription.
    #[test]
    fn a_show_before_the_window_listens_is_kept() {
        use iced::futures::StreamExt;

        SHOWN.notify_one();
        let mut shows = Box::pin(shows());
        assert_eq!(iced::futures::executor::block_on(shows.next()), Some(()));
    }

    #[test]
    fn the_channel_lies_in_the_state_directory_per_window_class() {
        let path = channel_path(AppId::INBOX_MAIL);
        if let (Some(path), Some(paths)) = (path, torro_core::paths::AppPaths::for_app(AppId::INBOX_MAIL)) {
            assert_eq!(path, paths.state.join("torro-inbox-mail.instance"));
        }
        let paths: Vec<_> = AppId::ALL.iter().filter_map(|app| channel_path(*app)).collect();
        for (i, path) in paths.iter().enumerate() {
            assert!(paths[i + 1..].iter().all(|other| other != path), "{} twice", path.display());
        }
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::sync::mpsc;

    use super::*;

    /// A pipe name of its own per test and process, so parallel tests never meet.
    fn unique_pipe(name: &str) -> PathBuf {
        PathBuf::from(format!(r"\\.\pipe\torro-test-instance-{name}-{}", std::process::id()))
    }

    fn claim_with_inbox(path: &Path) -> (Claim, mpsc::Receiver<()>) {
        let (sender, inbox) = mpsc::channel();
        let claim = claim_at(path, move || {
            let _ = sender.send(());
        });
        (claim, inbox)
    }

    #[test]
    fn the_first_start_listens_and_the_second_shows_it() {
        let path = unique_pipe("two");
        let (first, shows) = claim_with_inbox(&path);
        assert_eq!(first, Claim::First);
        assert_eq!(claim_with_inbox(&path).0, Claim::Shown);
        shows.recv_timeout(Duration::from_secs(3)).expect("the first got show");
        assert_eq!(claim_with_inbox(&path).0, Claim::Shown, "and a third");
        shows.recv_timeout(Duration::from_secs(3)).expect("show again");
    }

    #[test]
    fn different_window_classes_do_not_meet() {
        let (hub, hub_shows) = claim_with_inbox(&unique_pipe("hub"));
        let (mail, mail_shows) = claim_with_inbox(&unique_pipe("mail"));
        assert_eq!((hub, mail), (Claim::First, Claim::First));
        assert_eq!(claim_with_inbox(&unique_pipe("mail")).0, Claim::Shown);
        mail_shows.recv_timeout(Duration::from_secs(3)).expect("the mail window got show");
        assert!(hub_shows.try_recv().is_err(), "the hub did not");
    }

    #[test]
    fn a_silent_listener_does_not_hold_up_the_start() {
        let path = unique_pipe("silent");
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        let _squatter = runtime
            .block_on(async { torro_ipc::pipe::user_only_listener(&path) })
            .expect("a listener that never answers");

        let started = std::time::Instant::now();
        assert_eq!(claim_with_inbox(&path).0, Claim::Without, "starts without a channel");
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    #[test]
    fn the_channel_is_a_pipe_of_the_user() {
        let path = channel_path(AppId::CALENDAR).expect("a SID");
        let name = path.to_str().expect("text");
        assert!(name.starts_with(r"\\.\pipe\torro-torro-calendar-S-"), "{name}");
    }
}
