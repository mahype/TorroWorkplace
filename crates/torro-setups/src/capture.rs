//! Making a template from the active workspace.
//!
//! Task 1.13 in `docs/tasks/phase-1.md`. Reference:
//! `apps/workspace-setups/workspace_setups/capture.py`.

use chrono::{DateTime, Local, Utc};
use torro_core::AppId;
use torro_core::app::{WORKPLACE_BUNDLE_ID, WORKPLACE_DIALOG_CLASS};
use torro_wm::{Rect, Window, WindowManager};

use crate::layout::infer_tree;
use crate::{SCHEMA_VERSION, SetupsError, Source, Strategy, Template, TemplateWindow};

/// Classes of programs that only run once; their windows default to [`Strategy::Move`].
pub const SINGLE_INSTANCE: &[&str] = &["spotify", "slack", "signal", "discord", "obsidian", "1password"];

/// Windows that never go into a template (issue #243): the control centre and the core's dialog,
/// which is open right while "Save workspace…" captures, by every name a window manager gives them
/// ([`never_captured`]): their window classes on Linux (Hyprland), the bundle id of the core's app on
/// macOS (AeroSpace names windows by it), and the core's binary, which AeroSpace takes for a program
/// without a bundle and GlazeWM on Windows as the executable's name.
pub const NEVER_CAPTURED: &[&str] =
    &[AppId::WORKPLACE.window_class, WORKPLACE_DIALOG_CLASS, WORKPLACE_BUNDLE_ID, AppId::WORKPLACE.binary];

/// Whether a window of `class` ([`Window::class`]) is one of [`NEVER_CAPTURED`], ignoring case:
/// Windows does not tell `TorroWorkplace.exe` from `torroworkplace.exe`.
pub fn never_captured(class: &str) -> bool {
    NEVER_CAPTURED.iter().any(|never| never.eq_ignore_ascii_case(class))
}

/// The active workspace as a template with id `id` (callers get it from
/// `Store::unique_id`), without the windows of [`NEVER_CAPTURED`]. Windows get ids `window-1`,
/// `window-2`, … in the order the window manager lists them; tiled ones go into the tree (`layout::infer_tree`); rectangles are
/// stored relative to the monitor. `resolve` finds the start command of a window (the
/// daemon passes `launch::command_for`). The name is [`auto_name`].
pub async fn capture(
    wm: &impl WindowManager,
    id: String,
    resolve: impl Fn(&Window) -> Vec<String>,
    now: DateTime<Local>,
) -> Result<Template, SetupsError> {
    let workspace = wm.active_workspace().await?;
    let mut windows = wm.windows(Some(&workspace)).await?;
    windows.retain(|window| !never_captured(&window.class));
    let monitors = wm.monitors().await?;

    // The monitor that shows the workspace; failing that, the one the first window is on.
    let monitor = monitors.iter().find(|m| m.active_workspace.as_ref() == Some(&workspace)).or_else(|| {
        let name = windows.first()?.monitor.as_ref()?;
        monitors.iter().find(|m| &m.name == name)
    });
    let (origin_x, origin_y) = monitor.map_or((0, 0), |m| (m.rect.x, m.rect.y));

    let mut template_windows = Vec::with_capacity(windows.len());
    for (index, window) in windows.iter().enumerate() {
        let command = resolve(window);
        template_windows.push(TemplateWindow {
            id: format!("window-{}", index + 1),
            class: window.class.clone(),
            title: window.title.clone(),
            strategy: strategy_for(&window.class, &command),
            command,
            floating: window.floating,
            rect: Rect {
                x: window.rect.x - origin_x,
                y: window.rect.y - origin_y,
                width: window.rect.width,
                height: window.rect.height,
            },
        });
    }

    let tiled: Vec<(String, Rect)> =
        template_windows.iter().filter(|w| !w.floating).map(|w| (w.id.clone(), w.rect)).collect();
    let tree = infer_tree(&tiled);

    let timestamp = now.with_timezone(&Utc);
    Ok(Template {
        schema_version: SCHEMA_VERSION,
        id,
        name: auto_name(&workspace.0, now),
        wm: wm.name().to_string(),
        created: timestamp,
        updated: timestamp,
        source: Source {
            workspace: workspace.0.clone(),
            monitor: monitor.map(|m| m.name.clone()),
            // Rectangles are relative to the monitor, so its own rectangle starts at 0,0.
            monitor_rect: monitor.map(|m| Rect { x: 0, y: 0, width: m.rect.width, height: m.rect.height }),
        },
        tree,
        windows: template_windows,
        shortcut: None,
        startup: None,
    })
}

/// `Workspace 3, 11.10. 14:20`
pub fn auto_name(workspace: &str, now: DateTime<Local>) -> String {
    format!("Workspace {workspace}, {}", now.format("%d.%m. %H:%M"))
}

/// [`Strategy::Move`] for [`SINGLE_INSTANCE`] classes (case-insensitive), else [`Strategy::New`].
pub fn default_strategy(class: &str) -> Strategy {
    // Hyprland reports reverse-DNS app ids such as `com.slack.Slack`; the last segment is the name.
    let class = class.to_ascii_lowercase();
    let last = class.rsplit('.').next().unwrap_or_default();
    if SINGLE_INSTANCE.contains(&last) { Strategy::Move } else { Strategy::New }
}

/// The strategy a captured window gets: [`Strategy::Move`] when its program only runs once – a
/// [`SINGLE_INSTANCE`] class ([`default_strategy`]) or a plain start of an app of the suite
/// ([`suite_single_instance`]) – else [`Strategy::New`].
pub fn strategy_for(class: &str, command: &[String]) -> Strategy {
    if suite_single_instance(class, command) { Strategy::Move } else { default_strategy(class) }
}

/// The app of the suite whose windows a window manager names `class` (issue #279), ignoring case:
/// its window class (Hyprland, `torro-calendar`), its bundle id `io.github.mahype.<window class>`
/// (AeroSpace on macOS), its program file or binary, with or without `.exe` (AeroSpace for a
/// program without bundle, GlazeWM on Windows). `None` for any other program.
///
/// Under AeroSpace and GlazeWM every window of a program carries that one name, the side windows
/// too (a chat, a single task, the settings); only the start command tells them apart
/// ([`suite_single_instance`]).
pub fn suite_app(class: &str) -> Option<AppId> {
    let class = class.to_ascii_lowercase();
    let program = class.strip_suffix(".exe").unwrap_or(&class);
    let bundled = class.strip_prefix(BUNDLE_ID_PREFIX);
    AppId::ALL
        .iter()
        .find(|app| {
            app.window_class == class
                || bundled == Some(app.window_class)
                || app.program_file() == program
                || app.binary == program
        })
        .copied()
}

/// What `io.github.mahype.<window class>` starts with: the bundle ids of the suite's apps on macOS
/// (`packaging/macos/Info.plist.in`, [`WORKPLACE_BUNDLE_ID`]).
const BUNDLE_ID_PREFIX: &str = "io.github.mahype.";

/// Whether a window of `class` started with `command` is the one window of an app of the suite
/// ([`suite_app`]) that runs only once (issue #279, `torro_gui::instance`): its command starts the
/// app without a target, nothing but the arguments of one of the entries of its binary
/// ([`AppId::args`], `--view mail` also as `--view=mail`).
///
/// A second process of such a start only brings the running window to the front and ends, so a
/// template load must move that window instead of waiting for a new one. A start with a target
/// (`--task=…`, `--message=…`, `--lane`, `--source`, `--chat`, `--settings`, …) opens a window of its
/// own and is `false`, as is an empty command (nothing is known about the start).
pub fn suite_single_instance(class: &str, command: &[String]) -> bool {
    let (Some(app), Some(args)) = (suite_app(class), start_args(command)) else {
        return false;
    };
    AppId::ALL.iter().filter(|entry| entry.binary == app.binary).any(|entry| args == entry.args)
}

/// The arguments `command` passes to the program it starts, `--view=mail` split into `--view`,
/// `mail`: everything after the program, or for a macOS bundle start (`open -n -a <Foo.app> [--args
/// …]`, `launch::macos::command_from_process`) everything after `--args`. `None` for an empty
/// command.
fn start_args(command: &[String]) -> Option<Vec<String>> {
    let (program, rest) = command.split_first()?;
    let rest = if program == "open" {
        rest.iter().position(|arg| arg == "--args").map_or(&[][..], |at| &rest[at + 1..])
    } else {
        rest
    };
    let mut args = Vec::with_capacity(rest.len());
    for arg in rest {
        match arg.strip_prefix("--view=") {
            Some(view) => args.extend(["--view".to_string(), view.to_string()]),
            None => args.push(arg.clone()),
        }
    }
    Some(args)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use torro_wm::fake::{FakeWm, window};
    use torro_wm::{Monitor, SplitDirection, WorkspaceId};

    use super::*;
    use crate::LayoutTree;

    fn fixed_now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 11, 14, 20, 0).unwrap()
    }

    #[tokio::test]
    async fn captures_active_workspace_with_relative_geometry() {
        let wm = FakeWm::new("3");
        wm.set_monitors(vec![
            Monitor {
                name: "DP-1".into(),
                rect: Rect { x: 1920, y: 0, width: 1920, height: 1080 },
                scale: 1.0,
                active_workspace: Some(WorkspaceId("3".into())),
            },
            Monitor {
                name: "DP-2".into(),
                rect: Rect { x: 0, y: 0, width: 1920, height: 1080 },
                scale: 1.0,
                active_workspace: Some(WorkspaceId("1".into())),
            },
        ]);
        // Left half, floating Spotify, right half; the window on workspace 1 is not captured.
        wm.add_window(window("0xa", "foot", "3", Rect { x: 1920, y: 0, width: 960, height: 1080 }));
        let mut spotify = window("0xc", "Spotify", "3", Rect { x: 2000, y: 100, width: 800, height: 600 });
        spotify.floating = true;
        wm.add_window(spotify);
        wm.add_window(window("0xb", "chromium", "3", Rect { x: 2880, y: 0, width: 960, height: 1080 }));
        wm.add_window(window("0xd", "obsidian", "1", Rect { x: 0, y: 0, width: 1920, height: 1080 }));

        let template = capture(&wm, "tpl-1".into(), |w| vec![format!("run-{}", w.class)], fixed_now()).await.unwrap();

        let now = fixed_now().with_timezone(&Utc);
        assert_eq!(template.schema_version, SCHEMA_VERSION);
        assert_eq!(template.id, "tpl-1");
        assert_eq!(template.name, "Workspace 3, 11.10. 14:20");
        assert_eq!(template.wm, "fake");
        assert_eq!((template.created, template.updated), (now, now));
        assert_eq!(
            template.source,
            Source {
                workspace: "3".into(),
                monitor: Some("DP-1".into()),
                monitor_rect: Some(Rect { x: 0, y: 0, width: 1920, height: 1080 }),
            }
        );
        let expected_windows = vec![
            TemplateWindow {
                id: "window-1".into(),
                class: "foot".into(),
                title: "foot window".into(),
                command: vec!["run-foot".into()],
                strategy: Strategy::New,
                floating: false,
                rect: Rect { x: 0, y: 0, width: 960, height: 1080 },
            },
            TemplateWindow {
                id: "window-2".into(),
                class: "Spotify".into(),
                title: "Spotify window".into(),
                command: vec!["run-Spotify".into()],
                strategy: Strategy::Move,
                floating: true,
                rect: Rect { x: 80, y: 100, width: 800, height: 600 },
            },
            TemplateWindow {
                id: "window-3".into(),
                class: "chromium".into(),
                title: "chromium window".into(),
                command: vec!["run-chromium".into()],
                strategy: Strategy::New,
                floating: false,
                rect: Rect { x: 960, y: 0, width: 960, height: 1080 },
            },
        ];
        assert_eq!(template.windows, expected_windows);
        // Only the tiled windows are in the tree.
        assert_eq!(
            template.tree,
            Some(LayoutTree::Split {
                direction: SplitDirection::LeftRight,
                ratio: 0.5,
                first: Box::new(LayoutTree::Window { id: "window-1".into() }),
                second: Box::new(LayoutTree::Window { id: "window-3".into() }),
            })
        );
    }

    #[tokio::test]
    async fn empty_workspace_gives_template_without_windows() {
        let wm = FakeWm::new("5");

        let template = capture(&wm, "tpl-2".into(), |_| Vec::new(), fixed_now()).await.unwrap();

        assert!(template.windows.is_empty());
        assert_eq!(template.tree, None);
        assert_eq!(template.name, "Workspace 5, 11.10. 14:20");
        assert_eq!(template.source.monitor.as_deref(), Some("DP-1"));
    }

    #[tokio::test]
    async fn monitor_falls_back_to_the_first_windows_monitor() {
        let wm = FakeWm::new("3");
        wm.set_monitors(vec![Monitor {
            name: "DP-2".into(),
            rect: Rect { x: 3840, y: 0, width: 1920, height: 1080 },
            scale: 1.0,
            active_workspace: Some(WorkspaceId("2".into())),
        }]);
        let mut on_dp2 = window("0xa", "foot", "3", Rect { x: 4000, y: 10, width: 1920, height: 1080 });
        on_dp2.monitor = Some("DP-2".into());
        wm.add_window(on_dp2);

        let template = capture(&wm, "tpl-3".into(), |_| Vec::new(), fixed_now()).await.unwrap();

        assert_eq!(template.source.monitor.as_deref(), Some("DP-2"));
        assert_eq!(template.windows[0].rect, Rect { x: 160, y: 10, width: 1920, height: 1080 });
    }

    #[tokio::test]
    async fn the_control_centre_and_the_dialog_are_never_captured() {
        let wm = FakeWm::new("3");
        let rect = Rect { x: 0, y: 0, width: 960, height: 1080 };
        wm.add_window(window("0xa", AppId::WORKPLACE.window_class, "3", rect));
        wm.add_window(window("0xb", "foot", "3", Rect { x: 960, ..rect }));
        let mut dialog = window("0xc", WORKPLACE_DIALOG_CLASS, "3", Rect { x: 600, y: 300, width: 640, height: 480 });
        dialog.floating = true;
        wm.add_window(dialog);

        let template = capture(&wm, "tpl-4".into(), |_| Vec::new(), fixed_now()).await.expect("captured");

        let classes: Vec<&str> = template.windows.iter().map(|w| w.class.as_str()).collect();
        assert_eq!(classes, ["foot"]);
        assert_eq!(template.windows[0].id, "window-1", "numbered without the left-out windows");
        assert_eq!(template.tree, Some(LayoutTree::Window { id: "window-1".into() }));
    }

    /// Review fix of issue #243: on macOS AeroSpace names the control centre and the dialog by the
    /// bundle id (or, without a bundle, by the binary), on Windows GlazeWM by the executable, in any
    /// case; neither goes into a template there either. Other programs still do.
    #[tokio::test]
    async fn torro_windows_are_never_captured_on_macos_and_windows_either() {
        let wm = FakeWm::new("3");
        let rect = Rect { x: 0, y: 0, width: 960, height: 1080 };
        for (id, class) in [
            ("0xa", "io.github.mahype.torro-workplace"),
            ("0xb", "torroworkplace"),
            ("0xc", "TorroWorkplace"),
            ("0xd", "com.mitchellh.ghostty"),
            ("0xe", "io.github.mahype.torro-calendar"),
        ] {
            wm.add_window(window(id, class, "3", rect));
        }

        let template = capture(&wm, "tpl-5".into(), |_| Vec::new(), fixed_now()).await.expect("captured");

        let classes: Vec<&str> = template.windows.iter().map(|w| w.class.as_str()).collect();
        assert_eq!(classes, ["com.mitchellh.ghostty", "io.github.mahype.torro-calendar"]);
        assert!(never_captured("TORRO-WORKPLACE-DIALOG"));
        assert!(!never_captured("torrocalendar"), "the suite's apps belong in templates");
    }

    #[test]
    fn auto_name_uses_two_digit_day_and_month_and_24h_time() {
        assert_eq!(auto_name("3", fixed_now()), "Workspace 3, 11.10. 14:20");
        let morning = Local.with_ymd_and_hms(2026, 3, 4, 9, 5, 0).unwrap();
        assert_eq!(auto_name("web", morning), "Workspace web, 04.03. 09:05");
    }

    fn words(command: &[&str]) -> Vec<String> {
        command.iter().map(|word| word.to_string()).collect()
    }

    /// Issue #279: an app of the suite runs once, so a template moves its window; by every name a
    /// window manager gives it. Windows with a target and other programs still start anew.
    #[tokio::test]
    async fn a_plain_start_of_a_suite_app_is_moved_and_a_start_with_a_target_is_new() {
        let wm = FakeWm::new("3");
        let rect = Rect { x: 0, y: 0, width: 960, height: 1080 };
        for (id, class) in [("0xa", "torro-calendar"), ("0xb", "torro-tasks"), ("0xc", "foot")] {
            wm.add_window(window(id, class, "3", rect));
        }
        let resolve = |w: &Window| match w.class.as_str() {
            "torro-calendar" => words(&["/usr/bin/torrocalendar"]),
            "torro-tasks" => words(&["/usr/bin/torrotasks", "--task=abc"]),
            _ => words(&["foot"]),
        };

        let template = capture(&wm, "tpl-6".into(), resolve, fixed_now()).await.expect("captured");

        let strategies: Vec<Strategy> = template.windows.iter().map(|w| w.strategy).collect();
        assert_eq!(strategies, [Strategy::Move, Strategy::New, Strategy::New]);
    }

    #[test]
    fn the_suite_is_known_by_every_name_a_window_manager_gives_it() {
        for class in [
            "torro-calendar",
            "io.github.mahype.torro-calendar",
            "IO.GITHUB.MAHYPE.TORRO-CALENDAR",
            "torrocalendar",
            "torrocalendar.exe",
            "TorroCalendar.EXE",
        ] {
            assert_eq!(suite_app(class), Some(AppId::CALENDAR), "{class}");
        }
        assert_eq!(suite_app("io.github.mahype.torro-inbox-mail"), Some(AppId::INBOX_MAIL));
        assert_eq!(suite_app("torro-inbox-mail.exe"), Some(AppId::INBOX_MAIL));
        assert_eq!(suite_app("torroinbox"), Some(AppId::INBOX));
        for other in ["foot", "com.mitchellh.ghostty", "torro-calendar-helper", "io.github.mahype.foot"] {
            assert_eq!(suite_app(other), None, "{other}");
        }
    }

    #[test]
    fn only_a_start_without_a_target_is_the_single_instance() {
        let plain = [
            ("torro-calendar", words(&["/usr/bin/torrocalendar"])),
            ("torro-inbox-mail", words(&["/usr/bin/torroinbox", "--view", "mail"])),
            ("torro-inbox-mail", words(&["/usr/bin/torroinbox", "--view=mail"])),
            ("torro-inbox-mail", words(&["/usr/bin/torro-inbox-mail"])),
            ("io.github.mahype.torro-tasks", words(&["open", "-n", "-a", "/Applications/Torro Tasks.app"])),
            ("torroassistant.exe", words(&["C:\\Torro\\torroassistant.exe"])),
        ];
        for (class, command) in plain {
            assert!(suite_single_instance(class, &command), "{class} {command:?}");
            assert_eq!(strategy_for(class, &command), Strategy::Move);
        }
        let own_window = [
            ("torro-tasks", words(&["/usr/bin/torrotasks", "--task=abc"])),
            ("torro-inbox", words(&["/usr/bin/torroinbox", "--lane", "info"])),
            ("torro-inbox", words(&["/usr/bin/torroinbox", "--source=github"])),
            ("torro-inbox-mail", words(&["/usr/bin/torroinbox", "--view", "mail", "--message=42"])),
            ("torro-assistant", words(&["/usr/bin/torroassistant", "--chat", "7"])),
            ("torro-calendar", words(&["/usr/bin/torrocalendar", "--settings"])),
            (
                "io.github.mahype.torro-tasks",
                words(&["open", "-n", "-a", "/Applications/Torro Tasks.app", "--args", "--task=abc"]),
            ),
            ("torro-calendar", Vec::new()),
            ("foot", words(&["foot"])),
        ];
        for (class, command) in own_window {
            assert!(!suite_single_instance(class, &command), "{class} {command:?}");
        }
        assert_eq!(strategy_for("torro-tasks", &words(&["torrotasks", "--task=abc"])), Strategy::New);
        assert_eq!(strategy_for("Spotify", &words(&["spotify"])), Strategy::Move, "other single instances stay");
    }

    #[test]
    fn default_strategy_moves_single_instance_programs() {
        assert_eq!(default_strategy("Spotify"), Strategy::Move);
        assert_eq!(default_strategy("spotify"), Strategy::Move);
        assert_eq!(default_strategy("com.slack.Slack"), Strategy::Move);
        assert_eq!(default_strategy("org.signal.Signal"), Strategy::Move);
        assert_eq!(default_strategy("foot"), Strategy::New);
        assert_eq!(default_strategy("chromium"), Strategy::New);
        // Only a whole name counts, not a prefix.
        assert_eq!(default_strategy("spotifyd"), Strategy::New);
        assert_eq!(default_strategy("slack.helper"), Strategy::New);
    }
}
