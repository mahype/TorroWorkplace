//! `torrotasks` — tasks for the tiled desktop.
//!
//! An Iced window with the fixed app id `torro-tasks`. Data comes from the core
//! (`torroworkplace daemon`) over its socket; `--demo` runs on sample data instead.
//! Lists, smart lists (Heute, Geplant, Alle), quick add, subtasks, due dates, priorities.
//!
//! With `--demo`, `--open <state>` starts in a given state and `--now <YYYY-MM-DDTHH:MM>` fixes
//! the day; `--appearance light|dark|system` overrides the stored appearance everywhere (see
//! [`app::Launch`]).
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

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use torro_core::AppId;
use torro_core::paths::AppPaths;

const APP: AppId = AppId::TASKS;

/// Holds the error of the last failed start, in the app's state directory (Windows only).
const START_ERROR_FILE: &str = "start-error.log";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
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
    let result =
        iced::application(move || app::App::new(source.clone(), launch.clone()), app::App::update, app::App::view)
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
}
