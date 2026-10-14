//! AeroSpace backend (macOS).
//!
//! Task X.6 in `docs/tasks/plattformen.md`. AeroSpace is an i3-like tiling window manager;
//! everything goes through its command line program `aerospace` ([`cli`]): `list-windows`,
//! `list-workspaces` and `list-monitors` with `--json`, `focus`, `move-node-to-workspace`,
//! `close`, `join-with` and `resize` with `--window-id`. Events come from
//! `aerospace subscribe` ([`events`]); rectangles, which the program does not print, from
//! CoreGraphics ([`geometry`]).
//!
//! The module builds on every system so its tests run everywhere; [`crate::detect`] only looks
//! for AeroSpace on macOS. [`apps`] holds what macOS offers besides AeroSpace for finding and
//! raising the window of a process (task E8.9).

pub mod apps;
pub mod cli;
pub mod events;
pub mod geometry;
pub mod json;

use std::path::PathBuf;
use std::time::Duration;

use geometry::Geometry;

use crate::{Events, Monitor, Split, SplitDirection, Window, WindowId, WindowManager, WmError, WorkspaceId};

/// How long [`AeroSpace::answers`] waits; a running server answers in a few milliseconds.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// A connection to the running AeroSpace server, through its command line program.
#[derive(Debug, Clone)]
pub struct AeroSpace {
    /// The `aerospace` program.
    pub program: PathBuf,
    /// `osascript`, to read window and screen rectangles ([`geometry`]). `None` leaves every
    /// rectangle zero; a failing run does the same, so geometry never makes a call fail.
    pub osascript: Option<PathBuf>,
}

impl AeroSpace {
    /// Finds the program ([`cli::find_program`]: `TORRO_AEROSPACE_BIN`, `PATH`, Homebrew).
    /// Geometry is read with `/usr/bin/osascript` on macOS and not at all elsewhere.
    /// `NotRunning` when there is no program; whether the server runs is [`Self::answers`].
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Result<AeroSpace, WmError> {
        let program = cli::find_program(env)?;
        let osascript = cfg!(target_os = "macos").then(|| PathBuf::from(geometry::OSASCRIPT));
        Ok(AeroSpace { program, osascript })
    }

    /// Whether the server answers `list-workspaces --focused` within two seconds. Blocks the
    /// calling thread; meant for [`crate::detect`], once at start.
    pub fn answers(&self) -> bool {
        cli::answers(&self.program, PROBE_TIMEOUT)
    }

    async fn run(&self, args: &[&str]) -> Result<String, WmError> {
        cli::run(&self.program, args).await
    }

    /// Every window AeroSpace lists, normal or not.
    async fn raw_windows(&self) -> Result<Vec<json::RawWindow>, WmError> {
        let answer = self.run(&["list-windows", "--all", "--json", "--format", json::WINDOW_FORMAT]).await?;
        json::parse_raw_windows(&answer)
    }

    /// The rectangles, or none when there is no `osascript` or it fails.
    async fn geometry(&self) -> Geometry {
        match &self.osascript {
            Some(osascript) => geometry::read(osascript).await.unwrap_or_default(),
            None => Geometry::default(),
        }
    }

    /// Gives `sibling` the share `split.ratio` of the pair it forms with `window`. AeroSpace has
    /// no split ratio, only `resize` in pixels, so the pixels come from the rectangles.
    async fn set_ratio(&self, window: &str, sibling: &str, split: Split) -> Result<(), WmError> {
        let geometry = self.geometry().await;
        let rect = |id: &str| id.parse::<u32>().ok().and_then(|id| geometry.windows.get(&id).copied());
        let (Some(window_rect), Some(sibling_rect)) = (rect(window), rect(sibling)) else {
            return Err(WmError::Unsupported("split ratio: no window geometry".into()));
        };
        let Some((dimension, pixels)) = geometry::ratio_resize(split.direction, split.ratio, sibling_rect, window_rect)
        else {
            return Err(WmError::Unsupported("split ratio: the windows are not side by side".into()));
        };
        self.run(&["resize", "--window-id", sibling, dimension, &pixels.to_string()]).await.map(|_| ())
    }
}

impl WindowManager for AeroSpace {
    fn name(&self) -> &'static str {
        "aerospace"
    }

    async fn active_workspace(&self) -> Result<WorkspaceId, WmError> {
        let answer = self.run(&["list-workspaces", "--focused", "--json"]).await?;
        json::parse_focused_workspace(&answer)
    }

    async fn windows(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>, WmError> {
        let answer = self.run(&["list-windows", "--all", "--json", "--format", json::WINDOW_FORMAT]).await?;
        json::parse_windows(&answer, &self.geometry().await, workspace)
    }

    async fn monitors(&self) -> Result<Vec<Monitor>, WmError> {
        let monitors = self.run(&["list-monitors", "--json", "--format", json::MONITOR_FORMAT]).await?;
        let workspaces = self.run(&["list-workspaces", "--all", "--json", "--format", json::WORKSPACE_FORMAT]).await?;
        json::parse_monitors(&monitors, &workspaces, &self.geometry().await)
    }

    async fn focus_window(&self, window: &WindowId) -> Result<(), WmError> {
        self.run(&["focus", "--window-id", &cli::window_arg(window)?]).await.map(|_| ())
    }

    async fn move_to_workspace(&self, window: &WindowId, workspace: &WorkspaceId) -> Result<(), WmError> {
        // Without `--focus-follows-window` the focus stays; `--` lets names start with `-`.
        let window = cli::window_arg(window)?;
        self.run(&["move-node-to-workspace", "--window-id", &window, "--", &workspace.0]).await.map(|_| ())
    }

    async fn close_window(&self, window: &WindowId) -> Result<(), WmError> {
        self.run(&["close", "--window-id", &cli::window_arg(window)?]).await.map(|_| ())
    }

    /// Focuses `sibling`: AeroSpace puts a new tiled window right after the focused window in
    /// its container. The direction is set in [`place`](Self::place) with `join-with`, because
    /// `split` has no effect under AeroSpace's default normalization.
    async fn prepare_insert(&self, sibling: &WindowId, direction: SplitDirection) -> Result<(), WmError> {
        let _ = direction;
        self.focus_window(sibling).await
    }

    /// If the container holding `window` runs the other way than `split.direction`,
    /// `join-with` towards the sibling (left or up, where [`prepare_insert`](Self::prepare_insert)
    /// put it) wraps both into a new container that runs the right way, the sibling first. Then
    /// `resize` sets the ratio. `Unsupported` for a floating window, or when the ratio cannot be
    /// set (no geometry, windows not side by side); the windows then keep AeroSpace's sizes.
    async fn place(&self, window: &WindowId, sibling: &WindowId, split: Split) -> Result<(), WmError> {
        let window_id = cli::window_arg(window)?;
        let sibling_id = cli::window_arg(sibling)?;
        let raw = self.raw_windows().await?;
        let found = raw.iter().find(|raw| raw.id() == WindowId(window_id.clone()));
        let layout = found.ok_or_else(|| WmError::NotFound(window.0.clone()))?.window_parent_container_layout.clone();
        match json::orientation(&layout) {
            None => return Err(WmError::Unsupported(format!("window {window} is not tiled ({layout})"))),
            Some(direction) if direction == split.direction => {}
            Some(SplitDirection::LeftRight) => {
                self.run(&["join-with", "--window-id", &window_id, "left"]).await?;
            }
            Some(SplitDirection::TopBottom) => {
                self.run(&["join-with", "--window-id", &window_id, "up"]).await?;
            }
        }
        self.set_ratio(&window_id, &sibling_id, split).await
    }

    async fn events(&self) -> Result<Events, WmError> {
        events::subscribe(self.program.clone()).await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::{Rect, WmEvent};

    const WINDOWS: &str = include_str!("../../tests/fixtures/aerospace-windows.json");
    const WORKSPACES: &str = include_str!("../../tests/fixtures/aerospace-workspaces.json");
    const FOCUSED: &str = include_str!("../../tests/fixtures/aerospace-focused-workspace.json");
    const MONITORS: &str = include_str!("../../tests/fixtures/aerospace-monitors.json");
    const GEOMETRY: &str = include_str!("../../tests/fixtures/aerospace-geometry.json");

    /// A fake `aerospace`: logs every call to `calls`, answers the list commands from files next
    /// to it (`windows-<n>.json` for the n-th `list-windows`, else `windows.json`), prints
    /// `events.jsonl` for `subscribe` and stays alive, hangs if `hang` exists, and fails with
    /// the text of `stderr` if that file exists. "Next to it" is the directory of the path it
    /// was started by, so one script serves every test through a symlink in the test's own
    /// directory.
    const FAKE_AEROSPACE: &str = r#"#!/bin/sh
dir="$(dirname "$0")"
printf '%s\n' "$*" >> "$dir/calls"
if [ -f "$dir/hang" ]; then exec sleep 5; fi
if [ -f "$dir/stderr" ]; then cat "$dir/stderr" >&2; exit 1; fi
case "$1" in
  list-windows)
    n=$(( $(cat "$dir/count" 2>/dev/null || echo 0) + 1 )); echo "$n" > "$dir/count"
    if [ -f "$dir/windows-$n.json" ]; then cat "$dir/windows-$n.json"; else cat "$dir/windows.json"; fi ;;
  list-workspaces)
    if [ "$2" = "--focused" ]; then cat "$dir/focused.json"; else cat "$dir/workspaces.json"; fi ;;
  list-monitors) cat "$dir/monitors.json" ;;
  subscribe) cat "$dir/events.jsonl"; exec sleep 5 ;;
esac
"#;

    /// A fake `osascript` that prints `geometry.json` next to it, or fails if `geometry-fails`
    /// exists.
    const FAKE_OSASCRIPT: &str = r#"#!/bin/sh
dir="$(dirname "$0")"
if [ -f "$dir/geometry-fails" ]; then echo 'execution error' >&2; exit 1; fi
cat "$dir/geometry.json"
"#;

    /// The two fake scripts, written once per test run and run once right away. macOS scans a
    /// new executable on its first start, which can take seconds and would hit the timeouts;
    /// later starts, also through symlinks, are fast.
    fn shared_scripts() -> &'static Path {
        static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        DIR.get_or_init(|| {
            use std::os::unix::fs::PermissionsExt;

            let dir = std::env::temp_dir().join(format!("torro-wm-aerospace-fakes-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("create fakes dir");
            for (name, content) in [("aerospace", FAKE_AEROSPACE), ("osascript", FAKE_OSASCRIPT)] {
                let path = dir.join(name);
                std::fs::write(&path, content).expect("write fake script");
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("make fake executable");
                let _ = std::process::Command::new(&path).arg("warm-up").output();
            }
            dir
        })
    }

    /// A temporary directory with both fakes (symlinks to [`shared_scripts`]) and the fixtures.
    struct Fake {
        dir: PathBuf,
    }

    impl Fake {
        fn new(name: &str) -> Fake {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default();
            let dir = std::env::temp_dir().join(format!("torro-wm-aerospace-{name}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("create fake dir");
            for script in ["aerospace", "osascript"] {
                std::os::unix::fs::symlink(shared_scripts().join(script), dir.join(script)).expect("link fake");
            }
            let fake = Fake { dir };
            for (file, content) in [
                ("windows.json", WINDOWS),
                ("workspaces.json", WORKSPACES),
                ("focused.json", FOCUSED),
                ("monitors.json", MONITORS),
                ("geometry.json", GEOMETRY),
                ("events.jsonl", ""),
            ] {
                fake.write(file, content);
            }
            fake
        }

        fn write(&self, file: &str, content: &str) {
            std::fs::write(self.dir.join(file), content).expect("write fake file");
        }

        fn wm(&self) -> AeroSpace {
            AeroSpace { program: self.dir.join("aerospace"), osascript: Some(self.dir.join("osascript")) }
        }

        /// The calls that are not list commands, i.e. the commands that change something.
        fn commands(&self) -> Vec<String> {
            let calls = std::fs::read_to_string(self.dir.join("calls")).unwrap_or_default();
            calls.lines().filter(|line| !line.starts_with("list-")).map(str::to_owned).collect()
        }

        fn path(&self) -> &Path {
            &self.dir
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn from_env_uses_the_variable() {
        let wm = AeroSpace::from_env(|key| (key == cli::PROGRAM_VARIABLE).then(|| "/opt/fake/aerospace".into()))
            .expect("variable set");
        assert_eq!(wm.program, PathBuf::from("/opt/fake/aerospace"));
    }

    #[test]
    fn answers_only_when_the_server_does() {
        let fake = Fake::new("answers");
        assert!(fake.wm().answers());
        fake.write("stderr", "Can't connect to AeroSpace server. Is AeroSpace.app running?");
        assert!(!fake.wm().answers());
    }

    #[test]
    fn a_hanging_program_does_not_answer() {
        let fake = Fake::new("hangs");
        fake.write("hang", "");
        let started = std::time::Instant::now();
        assert!(!cli::answers(&fake.wm().program, Duration::from_millis(200)));
        assert!(started.elapsed() < Duration::from_secs(2), "the probe must give up after its timeout");
    }

    #[tokio::test]
    async fn reads_active_workspace_windows_and_monitors() {
        let fake = Fake::new("read");
        let wm = fake.wm();
        assert_eq!(wm.active_workspace().await, Ok(WorkspaceId("1".into())));

        let windows = wm.windows(Some(&WorkspaceId("1".into()))).await.expect("windows");
        assert_eq!(windows.iter().map(|w| w.id.0.as_str()).collect::<Vec<_>>(), ["101", "102", "103"]);
        assert_eq!(windows[0].rect, Rect { x: 8, y: 41, width: 856, height: 1068 });
        assert_eq!(windows[0].class, "com.apple.finder");

        let monitors = wm.monitors().await.expect("monitors");
        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[1].rect, Rect { x: 1728, y: 0, width: 2560, height: 1440 });
        assert_eq!(monitors[1].active_workspace, Some(WorkspaceId("2".into())));
        assert!(fake.commands().is_empty(), "reading must not send commands: {:?}", fake.commands());
    }

    #[tokio::test]
    async fn failing_geometry_leaves_rectangles_empty() {
        let fake = Fake::new("no-geometry");
        fake.write("geometry-fails", "");
        let windows = fake.wm().windows(None).await.expect("windows still listed");
        assert_eq!(windows.len(), 5);
        assert!(windows.iter().all(|w| w.rect == Rect::default()));
    }

    #[tokio::test]
    async fn commands_name_the_window_and_do_not_follow_it() {
        let fake = Fake::new("commands");
        let wm = fake.wm();
        wm.focus_window(&WindowId("102".into())).await.expect("focus");
        wm.move_to_workspace(&WindowId("103".into()), &WorkspaceId("-web".into())).await.expect("move");
        wm.close_window(&WindowId("101".into())).await.expect("close");
        assert_eq!(
            fake.commands(),
            ["focus --window-id 102", "move-node-to-workspace --window-id 103 -- -web", "close --window-id 101"]
        );
    }

    #[tokio::test]
    async fn refused_command_and_stopped_server() {
        let fake = Fake::new("refused");
        let wm = fake.wm();
        fake.write("stderr", "Invalid <window-id> 999 passed to --window-id\n");
        assert_eq!(
            wm.close_window(&WindowId("999".into())).await,
            Err(WmError::Protocol("Invalid <window-id> 999 passed to --window-id".into()))
        );
        fake.write("stderr", "Can't connect to AeroSpace server. Is AeroSpace.app running?\n");
        assert!(matches!(wm.active_workspace().await, Err(WmError::NotRunning(_))));
        assert!(matches!(wm.focus_window(&WindowId("0xabc".into())).await, Err(WmError::NotFound(_))));
    }

    #[tokio::test]
    async fn prepare_insert_focuses_the_sibling() {
        let fake = Fake::new("prepare");
        fake.wm().prepare_insert(&WindowId("101".into()), SplitDirection::TopBottom).await.expect("prepare");
        assert_eq!(fake.commands(), ["focus --window-id 101"]);
    }

    #[tokio::test]
    async fn place_joins_when_the_container_runs_the_other_way_then_resizes() {
        let fake = Fake::new("place-join");
        // 102 sits in a v_tiles container; left-right needs a new container: join with the
        // window above. 101 is left of 102 in the geometry, 856 + gap 8 + 848 wide together.
        let split = Split { direction: SplitDirection::LeftRight, ratio: 0.25 };
        fake.wm().place(&WindowId("102".into()), &WindowId("101".into()), split).await.expect("place");
        assert_eq!(fake.commands(), ["join-with --window-id 102 up", "resize --window-id 101 width 420"]);
    }

    #[tokio::test]
    async fn place_only_resizes_when_the_container_runs_the_right_way() {
        let fake = Fake::new("place-resize");
        // 103 is below 102 in a v_tiles container: 530 + gap 8 + 530 high together.
        let split = Split { direction: SplitDirection::TopBottom, ratio: 0.5 };
        fake.wm().place(&WindowId("103".into()), &WindowId("102".into()), split).await.expect("place");
        assert_eq!(fake.commands(), ["resize --window-id 102 height 526"]);
    }

    #[tokio::test]
    async fn place_floating_window_is_unsupported() {
        let fake = Fake::new("place-floating");
        let split = Split { direction: SplitDirection::LeftRight, ratio: 0.5 };
        let result = fake.wm().place(&WindowId("104".into()), &WindowId("101".into()), split).await;
        assert!(matches!(result, Err(WmError::Unsupported(_))), "{result:?}");
        assert!(fake.commands().is_empty());
    }

    #[tokio::test]
    async fn place_without_geometry_structures_but_reports_the_ratio_unsupported() {
        let fake = Fake::new("place-no-geometry");
        let wm = AeroSpace { osascript: None, ..fake.wm() };
        let split = Split { direction: SplitDirection::TopBottom, ratio: 0.5 };
        let result = wm.place(&WindowId("101".into()), &WindowId("103".into()), split).await;
        assert!(matches!(result, Err(WmError::Unsupported(_))), "{result:?}");
        assert_eq!(fake.commands(), ["join-with --window-id 101 left"]);
    }

    #[tokio::test]
    async fn events_from_notices_and_listing_changes() {
        let fake = Fake::new("events");
        // First listing: the fixture. Then 101 closes, 103 moves to 3, 120 opens.
        let changed = WINDOWS.replace("\"window-id\" : 101", "\"window-id\" : 120").replace(
            "\"window-title\" : \"Start page\",\n    \"workspace\" : \"1\"",
            "\"window-title\" : \"Start page\",\n    \"workspace\" : \"3\"",
        );
        fake.write("windows-1.json", WINDOWS);
        fake.write("windows.json", &changed);
        fake.write(
            "events.jsonl",
            concat!(
                "{\"_event\":\"mode-changed\",\"mode\":\"main\"}\n",
                "{\"_event\":\"focus-changed\",\"windowId\":102,\"workspace\":\"1\"}\n",
                "{\"_event\":\"focused-workspace-changed\",\"prevWorkspace\":\"1\",\"workspace\":\"3\"}\n",
                "{\"_event\":\"window-detected\",\"windowId\":130,\"workspace\":\"3\",\"appBundleId\":\"com.apple.Notes\",\"appName\":\"Notes\"}\n",
            ),
        );
        let mut events = fake.wm().events().await.expect("subscribe");
        let mut received = Vec::new();
        for _ in 0..5 {
            let event = tokio::time::timeout(Duration::from_secs(5), events.recv()).await;
            received.push(event.expect("event in time").expect("stream open"));
        }
        let window = |id: &str| WindowId(id.into());
        let workspace = |name: &str| WorkspaceId(name.into());
        assert_eq!(
            received,
            vec![
                WmEvent::WindowOpened {
                    window: window("120"),
                    class: "com.apple.finder".into(),
                    workspace: workspace("1")
                },
                WmEvent::WindowMoved { window: window("103"), workspace: workspace("3") },
                WmEvent::WindowClosed { window: window("101") },
                WmEvent::WorkspaceFocused { workspace: workspace("3") },
                // The listing does not show 130 yet; the notice itself reports it.
                WmEvent::WindowOpened {
                    window: window("130"),
                    class: "com.apple.Notes".into(),
                    workspace: workspace("3")
                },
            ]
        );
        let calls = std::fs::read_to_string(fake.path().join("calls")).unwrap_or_default();
        assert!(calls.contains("subscribe --no-send-initial window-detected focus-changed focused-workspace-changed"));
    }

    /// Needs AeroSpace on this Mac: `cargo test -p torro-wm -- --ignored live_aerospace`.
    /// Only reads; prints what the backend sees.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "needs a running AeroSpace"]
    async fn live_aerospace_read_only() {
        let detected = crate::detect(|key| std::env::var(key).ok());
        assert!(matches!(detected, crate::Backend::AeroSpace(_)), "detect found {detected:?}");
        let wm = AeroSpace::from_env(|key| std::env::var(key).ok()).expect("aerospace program");
        assert!(wm.answers(), "AeroSpace does not answer");
        // The subscription only listens; on an idle desktop nothing arrives within the second.
        let mut events = wm.events().await.expect("subscribe");
        let first = tokio::time::timeout(Duration::from_secs(1), events.recv()).await;
        eprintln!("first event within 1 s: {first:?}");
        let active = wm.active_workspace().await.expect("active workspace");
        eprintln!("active workspace: {active}");
        for monitor in wm.monitors().await.expect("monitors") {
            eprintln!("monitor: {monitor:?}");
        }
        for window in wm.windows(None).await.expect("windows") {
            eprintln!(
                "window {} {} ws={} monitor={:?} rect={:?} floating={} fullscreen={} pid={:?}",
                window.id,
                window.class,
                window.workspace,
                window.monitor,
                window.rect,
                window.floating,
                window.fullscreen,
                window.pid
            );
        }
    }
}
