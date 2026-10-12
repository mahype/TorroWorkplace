//! GlazeWM backend (Windows).
//!
//! Task X.7 in `docs/tasks/plattformen.md`. GlazeWM v3 serves a WebSocket on
//! `ws://127.0.0.1:6123` that takes its command line as text (`query monitors`,
//! `command --id <uuid> close`, `sub --events …`) and answers in JSON (see [`json`]). Windows are
//! addressed by GlazeWM's container ids (UUIDs), workspaces by their config name.
//!
//! The module builds on every system so its tests run everywhere; [`crate::detect`] only looks
//! for GlazeWM on Windows.
//!
//! GlazeWM splits every message at whitespace before parsing it, so workspace names with spaces
//! cannot be addressed; such calls return [`WmError::Unsupported`].

pub mod events;
pub mod json;
mod pid;
pub mod socket;

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use crate::{Events, Monitor, Split, SplitDirection, Window, WindowId, WindowManager, WmError, WorkspaceId};

/// GlazeWM's IPC port (`DEFAULT_IPC_PORT` in `wm-common`); GlazeWM offers no setting for it.
pub const DEFAULT_PORT: u16 = 6123;

/// Environment variable that overrides [`DEFAULT_PORT`], for tests against a fake server.
pub const PORT_VARIABLE: &str = "TORRO_GLAZEWM_PORT";

/// How long [`GlazeWm::probe`] waits for the port. Local connects answer at once; this only
/// bounds a stuck network stack, because detection runs synchronously at daemon start.
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

/// A connection target for the running GlazeWM instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlazeWm {
    /// Port of the IPC WebSocket on 127.0.0.1.
    pub port: u16,
}

impl GlazeWm {
    /// The target from the environment: [`PORT_VARIABLE`] if it holds a port, else
    /// [`DEFAULT_PORT`]. Does not check that GlazeWM runs.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> GlazeWm {
        let port = env(PORT_VARIABLE).and_then(|value| value.trim().parse().ok()).unwrap_or(DEFAULT_PORT);
        GlazeWm { port }
    }

    /// [`from_env`](Self::from_env), but only if something accepts connections on the port.
    /// Synchronous and bounded by a short timeout so `detect` can call it.
    pub fn probe(env: impl Fn(&str) -> Option<String>) -> Option<GlazeWm> {
        let wm = GlazeWm::from_env(env);
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, wm.port));
        TcpStream::connect_timeout(&address, PROBE_TIMEOUT).ok().map(|_| wm)
    }

    /// Sends one `command …` line and checks that GlazeWM accepted it.
    async fn command(&self, line: &str) -> Result<(), WmError> {
        socket::request(self.port, line).await.map(|_| ())
    }
}

/// The container id of `window` as GlazeWM takes it after `--id`/`--container-id`: a UUID, so
/// only hex digits and dashes. Anything else cannot be a GlazeWM window.
fn container_id(window: &WindowId) -> Result<&str, WmError> {
    let id = window.0.as_str();
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if valid { Ok(id) } else { Err(WmError::NotFound(format!("{id:?} is not a GlazeWM container id"))) }
}

/// `workspace` as one token GlazeWM can parse: not empty, no whitespace (GlazeWM splits there)
/// and no leading `-` (its parser would read a flag).
fn workspace_name(workspace: &WorkspaceId) -> Result<&str, WmError> {
    let name = workspace.0.as_str();
    let valid = !name.is_empty() && !name.starts_with('-') && !name.chars().any(char::is_whitespace);
    if valid {
        Ok(name)
    } else {
        Err(WmError::Unsupported(format!("GlazeWM cannot address the workspace name {name:?}")))
    }
}

/// `command focus --container-id <id>`: focuses the window (and shows its workspace).
pub fn focus_command(window: &WindowId) -> Result<String, WmError> {
    Ok(format!("command focus --container-id {}", container_id(window)?))
}

/// `command --id <id> move --workspace <name>`: moves without following; focus stays.
pub fn move_command(window: &WindowId, workspace: &WorkspaceId) -> Result<String, WmError> {
    Ok(format!("command --id {} move --workspace {}", container_id(window)?, workspace_name(workspace)?))
}

/// `command --id <id> close`.
pub fn close_command(window: &WindowId) -> Result<String, WmError> {
    Ok(format!("command --id {} close", container_id(window)?))
}

/// `command --id <id> set-tiling-direction horizontal|vertical`. GlazeWM puts a new window right
/// after the focused one in its parent; with the direction set on that window (GlazeWM wraps it
/// in a split container if needed) the new window ends up beside or below it.
pub fn tiling_direction_command(window: &WindowId, direction: SplitDirection) -> Result<String, WmError> {
    let direction = match direction {
        SplitDirection::LeftRight => "horizontal",
        SplitDirection::TopBottom => "vertical",
    };
    Ok(format!("command --id {} set-tiling-direction {direction}", container_id(window)?))
}

/// `command --id <id> size --width|--height <n>%`: gives the first child of a split
/// `split.ratio` of the split's length. GlazeWM takes whole percent only, kept within 1–99.
pub fn size_command(first: &WindowId, split: Split) -> Result<String, WmError> {
    let axis = match split.direction {
        SplitDirection::LeftRight => "width",
        SplitDirection::TopBottom => "height",
    };
    let percent = if split.ratio.is_finite() { (split.ratio * 100.0).round().clamp(1.0, 99.0) } else { 50.0 };
    Ok(format!("command --id {} size --{axis} {percent:.0}%", container_id(first)?))
}

impl WindowManager for GlazeWm {
    fn name(&self) -> &'static str {
        "glazewm"
    }

    async fn active_workspace(&self) -> Result<WorkspaceId, WmError> {
        let workspaces = json::parse_workspaces(socket::request(self.port, "query workspaces").await?)?;
        json::focused_workspace(&workspaces)
    }

    async fn windows(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>, WmError> {
        // `query windows` names no workspace or monitor; the monitor tree has both.
        let windows = json::parse_windows(socket::request(self.port, "query monitors").await?, pid::process_id)?;
        Ok(match workspace {
            Some(workspace) => windows.into_iter().filter(|window| &window.workspace == workspace).collect(),
            None => windows,
        })
    }

    async fn monitors(&self) -> Result<Vec<Monitor>, WmError> {
        json::parse_monitors(socket::request(self.port, "query monitors").await?)
    }

    async fn focus_window(&self, window: &WindowId) -> Result<(), WmError> {
        self.command(&focus_command(window)?).await
    }

    async fn move_to_workspace(&self, window: &WindowId, workspace: &WorkspaceId) -> Result<(), WmError> {
        self.command(&move_command(window, workspace)?).await
    }

    async fn close_window(&self, window: &WindowId) -> Result<(), WmError> {
        self.command(&close_command(window)?).await
    }

    async fn prepare_insert(&self, sibling: &WindowId, direction: SplitDirection) -> Result<(), WmError> {
        self.command(&focus_command(sibling)?).await?;
        self.command(&tiling_direction_command(sibling, direction)?).await
    }

    async fn place(&self, window: &WindowId, sibling: &WindowId, split: Split) -> Result<(), WmError> {
        // GlazeWM inserted `window` after the focused `sibling` when it appeared (prepare_insert),
        // so only the share is left. The ratio belongs to the first child, the sibling.
        let _ = window;
        self.command(&size_command(sibling, split)?).await
    }

    async fn events(&self) -> Result<Events, WmError> {
        events::subscribe(self.port).await
    }
}

#[cfg(test)]
mod tests {
    use super::socket::tests::{FakeGlazeWm, echo};
    use super::*;

    const MONITORS: &str = include_str!("../../tests/fixtures/glazewm-monitors.json");
    const WORKSPACES: &str = include_str!("../../tests/fixtures/glazewm-workspaces.json");
    const COMMAND_OK: &str = include_str!("../../tests/fixtures/glazewm-command-ok.json");
    const COMMAND_ERROR: &str = include_str!("../../tests/fixtures/glazewm-command-error.json");

    const CODE: &str = "0f9e8d7c-6b5a-4938-8271-6a5b4c3d2e03";
    const FIREFOX: &str = "0f9e8d7c-6b5a-4938-8271-6a5b4c3d2e01";

    fn window(id: &str) -> WindowId {
        WindowId(id.to_owned())
    }

    fn workspace(name: &str) -> WorkspaceId {
        WorkspaceId(name.to_owned())
    }

    /// A fake GlazeWM answering queries from the fixtures and every command with success.
    async fn fake() -> FakeGlazeWm {
        FakeGlazeWm::start(|message| {
            let fixture = match message {
                "query monitors" => MONITORS,
                "query workspaces" => WORKSPACES,
                _ => COMMAND_OK,
            };
            vec![echo(fixture, message)]
        })
        .await
    }

    #[test]
    fn port_comes_from_environment_or_default() {
        assert_eq!(GlazeWm::from_env(|_| None).port, DEFAULT_PORT);
        assert_eq!(GlazeWm::from_env(|key| (key == PORT_VARIABLE).then(|| "7001".into())).port, 7001);
        assert_eq!(GlazeWm::from_env(|_| Some("not a port".into())).port, DEFAULT_PORT);
    }

    #[test]
    fn probe_finds_listening_port_only() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("local address").port().to_string();
        let env = |key: &str| (key == PORT_VARIABLE).then(|| port.clone());
        assert_eq!(GlazeWm::probe(env).map(|wm| wm.port.to_string()), Some(port.clone()));
        drop(listener);
        assert_eq!(GlazeWm::probe(env), None);
    }

    #[test]
    fn commands_follow_glazewm_syntax() {
        let code = window(CODE);
        assert_eq!(focus_command(&code), Ok(format!("command focus --container-id {CODE}")));
        assert_eq!(move_command(&code, &workspace("2")), Ok(format!("command --id {CODE} move --workspace 2")));
        assert_eq!(close_command(&code), Ok(format!("command --id {CODE} close")));
        assert_eq!(
            tiling_direction_command(&code, SplitDirection::LeftRight),
            Ok(format!("command --id {CODE} set-tiling-direction horizontal"))
        );
        assert_eq!(
            tiling_direction_command(&code, SplitDirection::TopBottom),
            Ok(format!("command --id {CODE} set-tiling-direction vertical"))
        );
    }

    #[test]
    fn size_uses_whole_percent_of_the_split_axis() {
        let code = window(CODE);
        let split = |direction, ratio| Split { direction, ratio };
        assert_eq!(
            size_command(&code, split(SplitDirection::LeftRight, 0.35)),
            Ok(format!("command --id {CODE} size --width 35%"))
        );
        assert_eq!(
            size_command(&code, split(SplitDirection::TopBottom, 0.666)),
            Ok(format!("command --id {CODE} size --height 67%"))
        );
        assert_eq!(
            size_command(&code, split(SplitDirection::LeftRight, 0.0)),
            Ok(format!("command --id {CODE} size --width 1%"))
        );
        assert_eq!(
            size_command(&code, split(SplitDirection::LeftRight, f32::NAN)),
            Ok(format!("command --id {CODE} size --width 50%"))
        );
    }

    #[test]
    fn ids_and_names_glazewm_cannot_parse_are_refused() {
        assert!(matches!(focus_command(&window("0x55d0 --x")), Err(WmError::NotFound(_))));
        assert!(matches!(close_command(&window("")), Err(WmError::NotFound(_))));
        for name in ["", "dev docs", "-1", "tab\tname"] {
            assert!(
                matches!(move_command(&window(CODE), &workspace(name)), Err(WmError::Unsupported(_))),
                "workspace {name:?}"
            );
        }
    }

    #[tokio::test]
    async fn queries_map_fixtures_onto_neutral_types() {
        let fake = fake().await;
        let wm = GlazeWm { port: fake.port };
        assert_eq!(wm.active_workspace().await, Ok(workspace("1")));
        let on_one = wm.windows(Some(&workspace("1"))).await.expect("windows of workspace 1");
        assert_eq!(on_one.len(), 3);
        assert!(on_one.iter().all(|w| w.workspace == workspace("1")));
        assert_eq!(wm.windows(None).await.expect("all windows").len(), 6);
        assert!(wm.windows(Some(&workspace("42"))).await.expect("empty workspace").is_empty());
        let monitors = wm.monitors().await.expect("monitors");
        assert_eq!(monitors.len(), 2);
        assert_eq!(
            fake.received(),
            vec!["query workspaces", "query monitors", "query monitors", "query monitors", "query monitors"]
        );
    }

    #[tokio::test]
    async fn commands_are_sent_and_accepted() {
        let fake = fake().await;
        let wm = GlazeWm { port: fake.port };
        assert_eq!(wm.focus_window(&window(CODE)).await, Ok(()));
        assert_eq!(wm.move_to_workspace(&window(CODE), &workspace("3")).await, Ok(()));
        assert_eq!(wm.close_window(&window(CODE)).await, Ok(()));
        assert_eq!(
            fake.received(),
            vec![
                format!("command focus --container-id {CODE}"),
                format!("command --id {CODE} move --workspace 3"),
                format!("command --id {CODE} close"),
            ]
        );
    }

    #[tokio::test]
    async fn prepare_insert_focuses_sibling_then_sets_direction_and_place_sizes_it() {
        let fake = fake().await;
        let wm = GlazeWm { port: fake.port };
        assert_eq!(wm.prepare_insert(&window(FIREFOX), SplitDirection::TopBottom).await, Ok(()));
        let split = Split { direction: SplitDirection::TopBottom, ratio: 0.4 };
        assert_eq!(wm.place(&window(CODE), &window(FIREFOX), split).await, Ok(()));
        assert_eq!(
            fake.received(),
            vec![
                format!("command focus --container-id {FIREFOX}"),
                format!("command --id {FIREFOX} set-tiling-direction vertical"),
                format!("command --id {FIREFOX} size --height 40%"),
            ]
        );
    }

    #[tokio::test]
    async fn refused_command_is_an_error() {
        let fake = FakeGlazeWm::start(|message| vec![echo(COMMAND_ERROR, message)]).await;
        let wm = GlazeWm { port: fake.port };
        let err = wm.close_window(&window("0f9e8d7c-6b5a-4938-8271-6a5b4c3d2eff")).await.expect_err("refused");
        assert!(matches!(err, WmError::NotFound(_)), "{err:?}");

        let fake = FakeGlazeWm::start(|message| {
            vec![
                serde_json::json!({
                    "messageType": "client_response", "clientMessage": message,
                    "data": null, "error": "Workspace '9' not found in config.", "success": false
                })
                .to_string(),
            ]
        })
        .await;
        let wm = GlazeWm { port: fake.port };
        let err = wm.move_to_workspace(&window(CODE), &workspace("9")).await.expect_err("refused");
        assert_eq!(err, WmError::Protocol("Workspace '9' not found in config.".into()));
    }

    /// Needs a running GlazeWM on this Windows machine: `cargo test -p torro-wm -- --ignored`.
    /// Only reads; nothing on screen changes.
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "needs a running GlazeWM instance"]
    async fn live_queries_against_glazewm() {
        let wm = GlazeWm::probe(|key| std::env::var(key).ok()).expect("GlazeWM IPC port answers");
        let active = wm.active_workspace().await.expect("active workspace");
        let windows = wm.windows(Some(&active)).await.expect("windows of active workspace");
        let monitors = wm.monitors().await.expect("monitors");
        eprintln!("workspace {active}: {} window(s), {} monitor(s)", windows.len(), monitors.len());
        assert!(monitors.iter().any(|m| m.active_workspace.as_ref() == Some(&active)));
        let _events = wm.events().await.expect("subscribe");
    }
}
