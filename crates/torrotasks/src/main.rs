//! `torrotasks` — tasks for the tiled desktop.
//!
//! An Iced window with the fixed app id `torro-tasks`. Data comes from the core
//! (`torroworkplace daemon`) over its socket; `--demo` runs on sample data instead.
//! Lists, smart lists (Heute, Geplant, Alle), quick add, subtasks, due dates, priorities.
//!
//! With `--demo`, `--open <state>` starts in a given state and `--now <YYYY-MM-DDTHH:MM>` fixes
//! the day; `--appearance light|dark|system` overrides the stored appearance everywhere and
//! `--design <id>` the design pack (see [`app::Launch`]).
//!
//! `--task=<uid>` names the task the app starts on (task E8.7a): the core passes it when
//! `apps.open_app` opens a task (`task:<uid>`); an empty uid is refused (exit 64). The app shows it
//! in the detail, on its list (task E8.7b).
//!
//! `TORRO_TIMING=1` writes when the window first draws, when its first data came and every
//! `update` or `view` that takes a frame or longer ([`torro_core::timing`], issue #206), measured by
//! the hulls [`update`] and [`view`] here.
//!
//! On Windows the app is a GUI program without a console window; a failed start is written to
//! [`START_ERROR_FILE`] in the state directory instead (see [`report_start_error`]).

// Not for the test harness: its output has to reach the console.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod app;
mod detail;
mod filter;
mod glance;
mod list;
mod new_list;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use iced::{Element, Task};
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_core::timing;

const APP: AppId = AppId::TASKS;

/// Holds the error of the last failed start, in the app's state directory (Windows only).
const START_ERROR_FILE: &str = "start-error.log";

fn main() -> ExitCode {
    timing::start();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // Refuse bad parameters before anything starts (the core, a window).
    let mut launch = match app::Launch::parse(&args) {
        Ok(launch) => launch,
        Err(err) => {
            report_start_error(&err.to_string());
            return ExitCode::from(64);
        }
    };
    // The task to show (tasks E8.7a, E8.7b): read and checked now, so the core's command line never
    // fails; the app opens it in the detail once the tasks are in.
    launch.task = match start_task(&args) {
        Ok(task) => task,
        Err(err) => {
            report_start_error(&err);
            return ExitCode::from(64);
        }
    };
    let source = launch.source(&args);
    // Before the settings load the fonts: the first frame is drawn with the chosen design.
    torro_gui::design::start_for(&source, launch.design.as_deref());
    let result = iced::application(move || app::App::new(source.clone(), launch.clone()), update, view)
        .settings(torro_gui::settings())
        .title(app::App::title)
        .theme(app::App::theme)
        .scale_factor(app::App::scale_factor)
        .subscription(app::App::subscription)
        .window(torro_gui::window(APP))
        .run();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            report_start_error(&err.to_string());
            ExitCode::FAILURE
        }
    }
}

/// The longest uid `--task` takes.
const TASK_MAX: usize = 1000;

/// The task the app starts on (task E8.7a): `--task=<uid>`, as the core builds it for
/// `apps.open_app` with a `task:` target (always with `=`, since a uid may start with `-`), or
/// `--task <uid>`. `None` without the flag; `Err` for an empty uid, one with control characters or
/// longer than [`TASK_MAX`] bytes.
fn start_task(args: &[String]) -> Result<Option<String>, String> {
    let mut found = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = match arg.strip_prefix("--task") {
            Some("") => rest.next().cloned(),
            Some(value) if value.starts_with('=') => Some(value[1..].to_string()),
            _ => continue,
        };
        let value = value.ok_or_else(|| "--task needs the task's uid".to_string())?;
        if value.is_empty() || value.len() > TASK_MAX || value.chars().any(char::is_control) {
            return Err(format!("--task takes a task's uid, not {value:?}"));
        }
        found = Some(value);
    }
    Ok(found)
}

/// Reports why the app could not start: on stderr, and on Windows also in [`START_ERROR_FILE`],
/// because a GUI program started by a double click has no console that would show stderr.
fn report_start_error(message: &str) {
    let line = format!("{}: {message}", APP.binary);
    eprintln!("{line}");
    if cfg!(windows)
        && let Some(paths) = AppPaths::for_app(APP)
    {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        // Nowhere is left to report a failed write to; the stderr line above is all we can do.
        let _ = write_start_error(&paths.state, &format!("{stamp} {line}"));
    }
}

/// Writes `line` to [`START_ERROR_FILE`] in `dir`, creating `dir` if needed, and returns the file.
///
/// Replaces an earlier report, so the file stays small and always shows the latest failed start.
fn write_start_error(dir: &Path, line: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join(START_ERROR_FILE);
    std::fs::write(&file, format!("{line}\n"))?;
    Ok(file)
}

/// [`app::App::update`], timed: the first data marks "first data", a slow update is written.
fn update(app: &mut app::App, message: app::Message) -> Task<app::Message> {
    if matches!(message, app::Message::Loaded(Ok(_))) {
        timing::once("first data");
    }
    let started = Instant::now();
    let task = app.update(message);
    timing::slow("update", started);
    task
}

/// [`app::App::view`], timed: the first call marks "first frame", a slow one is written.
fn view(app: &app::App) -> Element<'_, app::Message> {
    let started = Instant::now();
    let element = app.view();
    timing::once("first frame");
    timing::slow("view", started);
    element
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_error_replaces_the_last_one_in_a_new_directory() {
        let root = std::env::temp_dir().join(format!("torrotasks-start-error-{}", std::process::id()));
        let dir = root.join("state");

        write_start_error(&dir, "first").expect("first write");
        let file = write_start_error(&dir, "second").expect("second write");

        assert_eq!(file, dir.join(START_ERROR_FILE));
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "second\n");
        std::fs::remove_dir_all(&root).expect("clean up");
    }

    /// Task E8.7a: the core's `--task=<uid>` is read, also a uid that looks like a flag.
    #[test]
    fn the_start_task_is_a_uid() {
        let args = |line: &[&str]| line.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
        assert_eq!(start_task(&args(&["--task=7d1e-5b2a"])), Ok(Some("7d1e-5b2a".into())));
        assert_eq!(start_task(&args(&["--task=--demo"])), Ok(Some("--demo".into())));
        assert_eq!(start_task(&args(&["--task", "7d1e"])), Ok(Some("7d1e".into())));
        assert_eq!(start_task(&args(&["--demo", "--tasks"])), Ok(None));
        for bad in [&["--task"][..], &["--task="], &["--task=a\nb"]] {
            assert!(start_task(&args(bad)).is_err(), "{bad:?}");
        }
    }
}
