//! Hyprland's event socket `.socket2.sock`.
//!
//! Task 1.7 in `docs/tasks/phase-1.md`. Hyprland writes one line per event,
//! `name>>data`, e.g. `openwindow>>55d0a1b2c3d4,3,foot,~`. Addresses come without `0x`;
//! [`crate::WindowId`] carries them with `0x`, as `j/clients` does.

use std::path::Path;

use crate::{Events, WindowId, WmError, WmEvent, WorkspaceId};

/// File name of the event socket inside the instance directory.
#[cfg(unix)]
const EVENT_SOCKET: &str = ".socket2.sock";

/// Capacity of the channel between the reader task and the core. Events are small and the
/// core drains them as they come, so this only absorbs short bursts.
#[cfg(unix)]
const EVENT_QUEUE: usize = 256;

/// One event line as a [`WmEvent`]; `None` for events we do not use.
///
/// Only the v2 lines of `movewindow` and `workspace` are read. The v1 lines describe the same
/// change, and reading both would report every move twice.
pub fn parse_event(line: &str) -> Option<WmEvent> {
    let (name, data) = line.split_once(">>")?;
    match name {
        // The title is the last field and may contain commas, so only the first three commas
        // split. It is not part of the event, but its field must be present.
        "openwindow" => {
            let [address, workspace, class, _title] = split_fields::<4>(data)?;
            Some(WmEvent::WindowOpened {
                window: window_id(address)?,
                class: class.to_owned(),
                workspace: workspace_name(workspace)?,
            })
        }
        "closewindow" => Some(WmEvent::WindowClosed { window: window_id(data)? }),
        // `movewindowv2>>ADDRESS,WORKSPACEID,WORKSPACENAME`. The name may contain commas.
        "movewindowv2" => {
            let [address, id, workspace] = split_fields::<3>(data)?;
            check_workspace_id(id)?;
            Some(WmEvent::WindowMoved { window: window_id(address)?, workspace: workspace_name(workspace)? })
        }
        // `workspacev2>>WORKSPACEID,WORKSPACENAME`. The name may contain commas.
        "workspacev2" => {
            let [id, workspace] = split_fields::<2>(data)?;
            check_workspace_id(id)?;
            Some(WmEvent::WorkspaceFocused { workspace: workspace_name(workspace)? })
        }
        // `configreloaded>>` with empty data (task WS.1): run-time binds and rules are gone.
        "configreloaded" => Some(WmEvent::ConfigReloaded),
        _ => None,
    }
}

/// Splits `data` into exactly `N` comma-separated fields; the last one keeps any further commas.
fn split_fields<const N: usize>(data: &str) -> Option<[&str; N]> {
    let mut parts = data.splitn(N, ',');
    let mut fields = [""; N];
    for field in &mut fields {
        *field = parts.next()?;
    }
    Some(fields)
}

/// A Hyprland window address without `0x` (hex digits only) as a [`WindowId`] with `0x`.
fn window_id(address: &str) -> Option<WindowId> {
    let is_hex = !address.is_empty() && address.bytes().all(|byte| byte.is_ascii_hexdigit());
    is_hex.then(|| WindowId(format!("0x{address}")))
}

fn workspace_name(name: &str) -> Option<WorkspaceId> {
    (!name.is_empty()).then(|| WorkspaceId(name.to_owned()))
}

/// Hyprland's numeric workspace id; negative for special workspaces. Only checked, not kept:
/// the core addresses workspaces by name.
fn check_workspace_id(id: &str) -> Option<()> {
    id.parse::<i64>().ok().map(|_| ())
}

/// Connects to `<dir>/.socket2.sock` and forwards parsed events into the returned channel
/// from a background task until the socket closes or the receiver is dropped.
#[cfg(unix)]
pub async fn subscribe(dir: &Path) -> Result<Events, WmError> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::UnixStream;
    use tokio::sync::mpsc;

    let path = dir.join(EVENT_SOCKET);
    let stream = UnixStream::connect(&path).await.map_err(|err| match err.kind() {
        // A missing socket file or a stale one left by a Hyprland that has exited.
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
            WmError::NotRunning(format!("no Hyprland event socket at {}", path.display()))
        }
        _ => WmError::Io(format!("connect to {}: {err}", path.display())),
    })?;

    let (tx, rx) = mpsc::channel(EVENT_QUEUE);
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        // Hyprland closing the socket ends the stream; so does a read error. Both simply stop
        // the task, and the receiver then sees the channel close.
        while let Ok(Some(line)) = lines.next_line().await {
            let Some(event) = parse_event(&line) else {
                continue;
            };
            if tx.send(event).await.is_err() {
                break;
            }
        }
    });
    Ok(rx)
}

/// Connects to `<dir>/.socket2.sock` and forwards parsed events into the returned channel
/// from a background task until the socket closes or the receiver is dropped.
#[cfg(not(unix))]
pub async fn subscribe(dir: &Path) -> Result<Events, WmError> {
    let _ = dir;
    Err(WmError::Unsupported("Hyprland sockets exist only on Unix systems".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(address: &str) -> WindowId {
        WindowId(format!("0x{address}"))
    }

    fn workspace(name: &str) -> WorkspaceId {
        WorkspaceId(name.to_owned())
    }

    #[test]
    fn parses_open_window() {
        assert_eq!(
            parse_event("openwindow>>55d0a1b2c3d4,3,foot,~"),
            Some(WmEvent::WindowOpened {
                window: window("55d0a1b2c3d4"),
                class: "foot".into(),
                workspace: workspace("3"),
            })
        );
    }

    #[test]
    fn open_window_title_may_contain_commas() {
        assert_eq!(
            parse_event("openwindow>>55d0a1b2c3d4,web,firefox,Inbox (3), Re: plans, final"),
            Some(WmEvent::WindowOpened {
                window: window("55d0a1b2c3d4"),
                class: "firefox".into(),
                workspace: workspace("web"),
            })
        );
    }

    #[test]
    fn open_window_accepts_empty_title() {
        assert_eq!(
            parse_event("openwindow>>abc,1,kitty,"),
            Some(WmEvent::WindowOpened { window: window("abc"), class: "kitty".into(), workspace: workspace("1") })
        );
    }

    #[test]
    fn parses_close_window() {
        assert_eq!(
            parse_event("closewindow>>55d0a1b2c3d4"),
            Some(WmEvent::WindowClosed { window: window("55d0a1b2c3d4") })
        );
    }

    #[test]
    fn parses_move_window_v2() {
        assert_eq!(
            parse_event("movewindowv2>>55d0a1b2c3d4,2,2"),
            Some(WmEvent::WindowMoved { window: window("55d0a1b2c3d4"), workspace: workspace("2") })
        );
    }

    #[test]
    fn move_window_v2_keeps_special_workspace_names() {
        assert_eq!(
            parse_event("movewindowv2>>55d0a1b2c3d4,-99,special:scratch"),
            Some(WmEvent::WindowMoved { window: window("55d0a1b2c3d4"), workspace: workspace("special:scratch") })
        );
    }

    #[test]
    fn parses_workspace_v2() {
        assert_eq!(parse_event("workspacev2>>4,4"), Some(WmEvent::WorkspaceFocused { workspace: workspace("4") }));
        assert_eq!(
            parse_event("workspacev2>>7,dev, docs"),
            Some(WmEvent::WorkspaceFocused { workspace: workspace("dev, docs") })
        );
    }

    #[test]
    fn parses_config_reloaded() {
        assert_eq!(parse_event("configreloaded>>"), Some(WmEvent::ConfigReloaded));
        // The data is empty today; should a version send some, it is still a reload.
        assert_eq!(parse_event("configreloaded>>1"), Some(WmEvent::ConfigReloaded));
        assert_eq!(parse_event("configreloaded"), None, "no event without `>>`");
        assert_eq!(parse_event("configreloadedx>>"), None);
    }

    #[test]
    fn v1_and_unused_events_are_ignored() {
        assert_eq!(parse_event("movewindow>>55d0a1b2c3d4,2"), None);
        assert_eq!(parse_event("workspace>>2"), None);
        assert_eq!(parse_event("activewindow>>foot,~"), None);
        assert_eq!(parse_event("focusedmon>>DP-1,2"), None);
    }

    #[test]
    fn garbage_is_ignored() {
        for line in [
            "",
            "garbage",
            "openwindow",
            "openwindow>>",
            "openwindow>>abc,1,foot",
            "openwindow>>zz,1,foot,title",
            "openwindow>>,1,foot,title",
            "openwindow>>abc,,foot,title",
            "closewindow>>",
            "closewindow>>zz,yy",
            "movewindowv2>>abc,2",
            "movewindowv2>>abc,x,2",
            "workspacev2>>4",
            "workspacev2>>x,4",
            "workspacev2>>4,",
        ] {
            assert_eq!(parse_event(line), None, "line {line:?}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn subscribe_without_socket_is_not_running() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        // /tmp, not temp_dir(): macOS puts that under /var/folders/…, too long for a socket path (104 bytes).
        let dir = std::path::Path::new("/tmp").join(format!("torro-wm-no-events-{}-{nanos}", std::process::id()));
        let Err(err) = subscribe(&dir).await else {
            panic!("subscribing without a socket must fail");
        };
        assert!(matches!(err, WmError::NotRunning(_)), "{err:?}");
    }

    /// A stand-in for Hyprland: writes four event lines and closes the socket.
    #[cfg(unix)]
    #[tokio::test]
    async fn subscribe_forwards_parsed_events_from_socket() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::UnixListener;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        // /tmp, not temp_dir(): macOS puts that under /var/folders/…, too long for a socket path (104 bytes).
        let dir = std::path::Path::new("/tmp").join(format!("torro-wm-events-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp instance dir");

        let listener = UnixListener::bind(dir.join(EVENT_SOCKET)).expect("bind fake socket");
        let server = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.expect("accept");
            conn.write_all(
                b"openwindow>>55d0a1b2c3d4,3,foot,~\nclosewindow>>55d0a1b2c3d4\nworkspacev2>>4,4\nconfigreloaded>>\n",
            )
            .await
            .expect("write events");
            // Dropping the connection closes the socket, which ends the stream.
        });

        let mut events = subscribe(&dir).await.expect("subscribe to fake socket");
        let mut received = Vec::new();
        while let Some(event) =
            tokio::time::timeout(std::time::Duration::from_secs(5), events.recv()).await.expect("event within timeout")
        {
            received.push(event);
        }
        server.await.expect("server task");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            received,
            vec![
                WmEvent::WindowOpened {
                    window: window("55d0a1b2c3d4"),
                    class: "foot".into(),
                    workspace: workspace("3")
                },
                WmEvent::WindowClosed { window: window("55d0a1b2c3d4") },
                WmEvent::WorkspaceFocused { workspace: workspace("4") },
                WmEvent::ConfigReloaded,
            ]
        );
    }

    /// Needs a running Hyprland on this machine: `cargo test -p torro-wm -- --ignored`.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "needs a running Hyprland instance"]
    async fn live_event_socket_connects() {
        let runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
        let dir = crate::hyprland::socket::socket_dir(runtime.as_deref(), signature.as_deref())
            .expect("Hyprland environment");
        let _events = subscribe(&dir).await.expect("connect to .socket2.sock");
    }
}
