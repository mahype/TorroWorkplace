//! Hyprland backend.
//!
//! Talks to Hyprland over its two Unix sockets in
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`: `.socket.sock` for requests
//! (`j/clients`, `dispatch …`) and `.socket2.sock` for the event stream. Under Omarchy 4,
//! Hyprland 0.56 takes dispatchers as Lua expressions (`hl.dsp.focus({ … })`).
//!
//! Tasks 1.3–1.8 in `docs/tasks/phase-1.md` fill in the submodules.

pub mod dispatch;
pub mod events;
pub mod json;
pub mod socket;

use std::path::PathBuf;

use crate::{Events, KeyBind, Monitor, Split, SplitDirection, Window, WindowId, WindowManager, WmError, WorkspaceId};

/// A connection to the running Hyprland instance.
#[derive(Debug, Clone)]
pub struct Hyprland {
    /// Directory holding `.socket.sock` and `.socket2.sock`.
    pub socket_dir: PathBuf,
}

impl Hyprland {
    /// Finds the instance from `XDG_RUNTIME_DIR` and `HYPRLAND_INSTANCE_SIGNATURE`.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Result<Hyprland, WmError> {
        let dir = socket::socket_dir(env("XDG_RUNTIME_DIR").as_deref(), env("HYPRLAND_INSTANCE_SIGNATURE").as_deref())?;
        Ok(Hyprland { socket_dir: dir })
    }

    /// Sends one dispatcher expression (`hl.dsp…`) as `dispatch <expression>` and checks the answer.
    async fn dispatch(&self, expression: &str) -> Result<(), WmError> {
        let answer = socket::request(&self.socket_dir, &format!("dispatch {expression}")).await?;
        dispatch::check_answer(&answer)
    }

    /// Runs one Lua expression (`hl.bind(…)`, `hl.window_rule(…)`) as `eval <expression>`, what
    /// `hyprctl eval` does, and checks the answer like a dispatch.
    async fn eval(&self, expression: &str) -> Result<(), WmError> {
        let answer = socket::request(&self.socket_dir, &format!("eval {expression}")).await?;
        dispatch::check_answer(&answer)
    }
}

/// The normal windows from the answers to `j/clients` and `j/monitors`, with monitor names
/// filled in and, if given, only those on `workspace`. Split out of [`Hyprland::windows`] so it
/// can be tested without a socket.
fn windows_from_answers(
    clients: &str,
    monitors: &str,
    workspace: Option<&WorkspaceId>,
) -> Result<Vec<Window>, WmError> {
    let names = json::monitor_names(monitors)?;
    let windows = json::join_monitor_names(json::parse_clients_with_monitor(clients)?, &names);
    Ok(match workspace {
        Some(workspace) => windows.into_iter().filter(|window| &window.workspace == workspace).collect(),
        None => windows,
    })
}

impl WindowManager for Hyprland {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    async fn active_workspace(&self) -> Result<WorkspaceId, WmError> {
        let answer = socket::request(&self.socket_dir, "j/activeworkspace").await?;
        json::parse_active_workspace(&answer)
    }

    async fn windows(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>, WmError> {
        let clients = socket::request(&self.socket_dir, "j/clients").await?;
        let monitors = socket::request(&self.socket_dir, "j/monitors").await?;
        windows_from_answers(&clients, &monitors, workspace)
    }

    async fn monitors(&self) -> Result<Vec<Monitor>, WmError> {
        let answer = socket::request(&self.socket_dir, "j/monitors").await?;
        json::parse_monitors(&answer)
    }

    /// Task ER2.2: one rule per monitor, sent as `eval <expression>` (what `hyprctl eval` does).
    /// Rules registered at run time go with a reload of the configuration, so the core registers
    /// them before every start of the window.
    async fn cover_monitors(&self, covers: &[crate::Cover]) -> Result<(), WmError> {
        for cover in covers {
            self.eval(&dispatch::cover_rule(cover)).await?;
        }
        Ok(())
    }

    /// Task WS.1: `hl.dsp.focus({ monitor = … })`.
    async fn focus_monitor(&self, monitor: &str) -> Result<(), WmError> {
        self.dispatch(&dispatch::focus_monitor(monitor)).await
    }

    /// Task WS.1: from `j/version`; `None` when Hyprland does not answer.
    async fn version(&self) -> Option<String> {
        let answer = socket::request(&self.socket_dir, "j/version").await.ok()?;
        json::parse_version(&answer)
    }

    /// Task WS.1: one `eval hl.bind(…)` per bind. Hyprland drops run-time binds when it reloads its
    /// configuration ([`crate::WmEvent::ConfigReloaded`]).
    async fn bind_keys(&self, binds: &[KeyBind]) -> Result<(), WmError> {
        for bind in binds {
            self.eval(&dispatch::bind_key(bind)?).await?;
        }
        Ok(())
    }

    /// Task WS.1: one `eval hl.unbind(…)` per key text.
    async fn unbind_keys(&self, keys: &[String]) -> Result<(), WmError> {
        for keys in keys {
            self.eval(&dispatch::unbind_key(keys)).await?;
        }
        Ok(())
    }

    /// Task WS.1: one named rule per class ([`dispatch::float_rule`]).
    async fn float_dialogs(&self, class: &str) -> Result<(), WmError> {
        self.eval(&dispatch::float_rule(class)).await
    }

    /// Task 1.6.
    async fn focus_window(&self, window: &WindowId) -> Result<(), WmError> {
        self.dispatch(&dispatch::focus_window(window)?).await
    }

    /// Task 1.6.
    async fn move_to_workspace(&self, window: &WindowId, workspace: &WorkspaceId) -> Result<(), WmError> {
        self.dispatch(&dispatch::move_to_workspace(window, workspace)?).await
    }

    /// Task 1.6.
    async fn close_window(&self, window: &WindowId) -> Result<(), WmError> {
        self.dispatch(&dispatch::close_window(window)?).await
    }

    /// Task 1.8.
    async fn prepare_insert(&self, sibling: &WindowId, direction: SplitDirection) -> Result<(), WmError> {
        self.dispatch(&dispatch::focus_window(sibling)?).await?;
        self.dispatch(&dispatch::preselect(direction)).await
    }

    /// Task 1.8.
    async fn place(&self, window: &WindowId, sibling: &WindowId, split: Split) -> Result<(), WmError> {
        // Hyprland already put `window` next to `sibling` when it opened (see prepare_insert),
        // so only the ratio of that split is left to set here.
        let _ = sibling;
        self.dispatch(&dispatch::focus_window(window)?).await?;
        self.dispatch(&dispatch::split_ratio(split.ratio)).await
    }

    /// Task 1.7.
    async fn events(&self) -> Result<Events, WmError> {
        events::subscribe(&self.socket_dir).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENTS: &str = include_str!("../../tests/fixtures/hyprland-clients.json");
    const MONITORS: &str = include_str!("../../tests/fixtures/hyprland-monitors.json");

    #[test]
    fn windows_carry_monitor_name_from_monitors_answer() {
        let windows = windows_from_answers(CLIENTS, MONITORS, None).expect("fixtures parse");
        assert_eq!(windows.len(), 7);
        assert!(windows.iter().all(|w| w.monitor.as_deref() == Some("HDMI-A-1")));
    }

    #[test]
    fn workspace_filter_keeps_only_that_workspace() {
        let all = windows_from_answers(CLIENTS, MONITORS, None).expect("fixtures parse");
        let on_one = windows_from_answers(CLIENTS, MONITORS, Some(&WorkspaceId("1".into()))).expect("fixtures parse");
        assert!(!on_one.is_empty());
        assert!(on_one.len() < all.len());
        assert!(on_one.iter().all(|w| w.workspace == WorkspaceId("1".into())));
        let expected: Vec<&Window> = all.iter().filter(|w| w.workspace == WorkspaceId("1".into())).collect();
        assert_eq!(on_one.iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn workspace_without_windows_gives_empty_list() {
        let windows = windows_from_answers(CLIENTS, MONITORS, Some(&WorkspaceId("42".into()))).expect("fixtures parse");
        assert!(windows.is_empty());
    }

    /// A temporary directory standing in for the Hyprland instance directory.
    #[cfg(unix)]
    fn temp_instance_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        // /tmp, not temp_dir(): macOS puts that under /var/folders/…, too long for a socket path (104 bytes).
        let dir = std::path::Path::new("/tmp").join(format!("torro-wm-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp instance dir");
        dir
    }

    /// Stands in for Hyprland's request socket: takes one request and answers `answer`.
    /// The returned task yields the request text.
    #[cfg(unix)]
    async fn answer_one_request(dir: &std::path::Path, answer: &'static str) -> tokio::task::JoinHandle<String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::UnixListener::bind(dir.join(".socket.sock")).expect("bind fake socket");
        tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.expect("accept");
            let mut received = String::new();
            conn.read_to_string(&mut received).await.expect("read request");
            conn.write_all(answer.as_bytes()).await.expect("write answer");
            received
        })
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn focus_sends_dispatch_line_and_accepts_ok() {
        let dir = temp_instance_dir("focus-ok");
        let server = answer_one_request(&dir, "ok\n").await;
        let wm = Hyprland { socket_dir: dir.clone() };
        let result = wm.focus_window(&WindowId("0xABC".into())).await;
        let received = server.await.expect("server task");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(received, r#"dispatch hl.dsp.focus({ window = "address:0xabc" })"#);
        assert_eq!(result, Ok(()));
    }

    /// Stands in for Hyprland's request socket for `count` requests, answering each with `ok`.
    /// Every request uses its own connection; the returned task yields the request texts in order.
    #[cfg(unix)]
    async fn answer_requests(dir: &std::path::Path, count: usize) -> tokio::task::JoinHandle<Vec<String>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::UnixListener::bind(dir.join(".socket.sock")).expect("bind fake socket");
        tokio::spawn(async move {
            let mut received = Vec::new();
            for _ in 0..count {
                let (mut conn, _) = listener.accept().await.expect("accept");
                let mut request = String::new();
                conn.read_to_string(&mut request).await.expect("read request");
                conn.write_all(b"ok\n").await.expect("write answer");
                received.push(request);
            }
            received
        })
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_insert_focuses_sibling_then_preselects() {
        let dir = temp_instance_dir("prepare-insert");
        let server = answer_requests(&dir, 2).await;
        let wm = Hyprland { socket_dir: dir.clone() };
        let result = wm.prepare_insert(&WindowId("0x51b".into()), SplitDirection::TopBottom).await;
        let received = server.await.expect("server task");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            received,
            vec![
                r#"dispatch hl.dsp.focus({ window = "address:0x51b" })"#.to_owned(),
                r#"dispatch hl.dsp.layout("preselect d")"#.to_owned(),
            ]
        );
        assert_eq!(result, Ok(()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn place_focuses_new_window_then_sets_ratio() {
        let dir = temp_instance_dir("place");
        let server = answer_requests(&dir, 2).await;
        let wm = Hyprland { socket_dir: dir.clone() };
        let split = Split { direction: SplitDirection::LeftRight, ratio: 0.35 };
        let result = wm.place(&WindowId("0xe0e".into()), &WindowId("0x51b".into()), split).await;
        let received = server.await.expect("server task");
        let _ = std::fs::remove_dir_all(&dir);
        // The sibling is not focused again: Hyprland placed the window when it opened.
        assert_eq!(
            received,
            vec![
                r#"dispatch hl.dsp.focus({ window = "address:0xe0e" })"#.to_owned(),
                r#"dispatch hl.dsp.layout("splitratio 0.7000 exact")"#.to_owned(),
            ]
        );
        assert_eq!(result, Ok(()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn binds_go_out_as_eval_lines_one_per_bind() {
        let dir = temp_instance_dir("binds");
        let server = answer_requests(&dir, 4).await;
        let wm = Hyprland { socket_dir: dir.clone() };
        let bind = |keys: &str, id: &str| KeyBind {
            keys: keys.into(),
            command: vec!["/usr/bin/torroworkplace".into(), "load".into(), id.into()],
        };
        let bound = wm.bind_keys(&[bind("SUPER + ALT + 1", "a"), bind("SUPER + ALT + 0", "b")]).await;
        let unbound = wm.unbind_keys(&["SUPER + ALT + 1".into()]).await;
        let floated = wm.float_dialogs("torro-dialog").await;
        let received = server.await.expect("server task");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!((bound, unbound, floated), (Ok(()), Ok(()), Ok(())));
        assert_eq!(
            received,
            vec![
                r#"eval hl.bind("SUPER + ALT + 1", hl.dsp.exec_cmd("'/usr/bin/torroworkplace' 'load' 'a'"))"#.to_owned(),
                r#"eval hl.bind("SUPER + ALT + 0", hl.dsp.exec_cmd("'/usr/bin/torroworkplace' 'load' 'b'"))"#.to_owned(),
                r#"eval hl.unbind("SUPER + ALT + 1")"#.to_owned(),
                r#"eval hl.window_rule({ name = "torro-float-torro-dialog", match = { class = "^torro-dialog$" }, float = true, center = true })"#.to_owned(),
            ]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn focus_monitor_dispatches_and_version_reads_j_version() {
        let dir = temp_instance_dir("monitor");
        let server = answer_one_request(&dir, "ok\n").await;
        let wm = Hyprland { socket_dir: dir.clone() };
        let focused = wm.focus_monitor("DP-2").await;
        let received = server.await.expect("server task");
        assert_eq!(received, r#"dispatch hl.dsp.focus({ monitor = "DP-2" })"#);
        assert_eq!(focused, Ok(()));

        let _ = std::fs::remove_file(dir.join(".socket.sock"));
        let server = answer_one_request(&dir, r#"{"version": "0.56.0", "tag": "v0.56.0"}"#).await;
        let version = wm.version().await;
        let received = server.await.expect("server task");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(received, "j/version");
        assert_eq!(version.as_deref(), Some("0.56.0"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn no_hyprland_has_no_version() {
        let dir = temp_instance_dir("no-version");
        let wm = Hyprland { socket_dir: dir.clone() };
        let version = wm.version().await;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(version, None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refused_dispatch_becomes_protocol_error() {
        let dir = temp_instance_dir("close-refused");
        let server = answer_one_request(&dir, "err: no such window\n").await;
        let wm = Hyprland { socket_dir: dir.clone() };
        let result = wm.close_window(&WindowId("0x1f".into())).await;
        let received = server.await.expect("server task");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(received, r#"dispatch hl.dsp.window.close({ window = "address:0x1f" })"#);
        assert_eq!(result, Err(WmError::Protocol("err: no such window".into())));
    }

    /// Needs a running Hyprland on this machine: `cargo test -p torro-wm -- --ignored`.
    /// Focuses the window that already has focus, so nothing visibly changes.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "needs a running Hyprland instance"]
    async fn live_focus_on_the_focused_window_again() {
        let wm = Hyprland::from_env(|key| std::env::var(key).ok()).expect("Hyprland environment");
        let answer = socket::request(&wm.socket_dir, "j/activewindow").await.expect("active window");
        let active: serde_json::Value = serde_json::from_str(&answer).expect("j/activewindow is JSON");
        let address = active["address"].as_str().filter(|address| !address.is_empty());
        let address = address.expect("no focused window; focus one and run the test again");
        wm.focus_window(&WindowId(address.into())).await.expect("focus the focused window");
    }

    /// Needs a running Hyprland on this machine: `cargo test -p torro-wm -- --ignored`.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "needs a running Hyprland instance"]
    async fn live_windows_of_active_workspace_carry_monitor_names() {
        let wm = Hyprland::from_env(|key| std::env::var(key).ok()).expect("Hyprland environment");
        let active = wm.active_workspace().await.expect("active workspace");
        let windows = wm.windows(Some(&active)).await.expect("windows of active workspace");
        eprintln!("active workspace {active}: {} window(s)", windows.len());
        assert!(windows.iter().all(|w| w.workspace == active), "window on another workspace");
        for window in &windows {
            assert!(window.monitor.is_some(), "{} ({}) has no monitor name", window.id, window.class);
        }
    }
}
