//! `torrocalendar` — calendar for the tiled desktop.
//!
//! An Iced window with the fixed app id `torro-calendar`. Data comes from the core
//! (`torroworkplace daemon`) over its socket; `--demo` runs on sample data instead.
//! Day, week, month and agenda views; events can be created, edited and deleted.
//!
//! With `--demo`, `--open <state>` starts in a given state and `--now <YYYY-MM-DDTHH:MM>` fixes the
//! time; `--appearance light|dark|system` overrides the stored appearance and `--design <id>` the
//! design pack (see [`app::Launch`]).
//!
//! `torrocalendar --settings <section>` opens the window with its settings in the content area
//! (task S.19): Calendars, Reminders and Night, over the window's own way to the core – with
//! `--demo` over the settings library's demo, which never opens a socket. Works without `--demo`
//! too.
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
mod editor;
mod reminder;
mod views;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use iced::{Element, Task};
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_core::timing;

const APP: AppId = AppId::CALENDAR;

/// Holds the error of the last failed start, in the app's state directory (Windows only).
const START_ERROR_FILE: &str = "start-error.log";

fn main() -> ExitCode {
    timing::start();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // The reminder over the whole screen (task ER2.1): a window of its own, started by the core,
    // also as the agent app "Torro Reminder.app", whose binary is this one named `torro-reminder`
    // (task ER2.2, `packaging/macos/make-app.sh`).
    let mut args = args;
    if started_as_reminder() && !args.iter().any(|arg| arg == "--reminders") {
        args.insert(0, "--reminders".to_string());
    }
    if reminder::Launch::wanted(&args) {
        return match reminder::run(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                report_start_error(&err);
                ExitCode::from(64)
            }
        };
    }
    // Refuse bad parameters before anything starts (the core, a window).
    let launch = match app::Launch::parse(&args) {
        Ok(launch) => launch,
        Err(err) => {
            report_start_error(&err.to_string());
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

/// The binary runs as `torro-reminder`: the reminder window's agent app on macOS.
fn started_as_reminder() -> bool {
    std::env::current_exe().ok().and_then(|exe| exe.file_stem().map(|stem| stem == reminder::BINARY)).unwrap_or(false)
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
        let root = std::env::temp_dir().join(format!("torrocalendar-start-error-{}", std::process::id()));
        let dir = root.join("state");

        write_start_error(&dir, "first").expect("first write");
        let file = write_start_error(&dir, "second").expect("second write");

        assert_eq!(file, dir.join(START_ERROR_FILE));
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "second\n");
        std::fs::remove_dir_all(&root).expect("clean up");
    }
}
