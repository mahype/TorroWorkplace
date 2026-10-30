//! `torroworkplace` — the core of the suite.
//!
//! - `daemon`: the background service with the tray icon. Owns the window manager
//!   connection and the workspace templates, and the socket clients talk to; later also
//!   accounts, sync and the app registry. `--headless` runs it without a tray icon, and so does a
//!   system without the main application (issue #408, [`daemon_headless`]).
//! - `status --json`, `ctl`: for bars, hotkeys and scripts; `notify`: a notice to the inbox.
//! - `hook claude-code|codex|gemini`: run by the hooks of coding agents; reports waiting and
//!   finished sessions to the inbox, silently and always with exit code 0 (see `hook`).
//! - `workspace_keys`: the template keys (modifiers + digit), bound by the daemon on Hyprland
//!   and by the tray on macOS and Windows.
//! - `ui` (also without a command): the control centre, an Iced window for accounts,
//!   workspaces, apps and general settings. `ui --demo [--open <state>]` shows sample data without
//!   the service (see `ui::Launch`); `--section` and `--appearance light|dark|system` work too.
//!   `ui --template=<id> [--saved]` shows only the dialog of one workspace template, in a small
//!   window of its own (the tray and `apps.open_app` start it).
//! - `mcp`: one MCP server for the whole suite, on stdio, with the agent's key from
//!   `TORRO_TOKEN`.
//!
//! On Windows the binary is a GUI program, so a double click opens no console window. Commands
//! attach to the console of the shell that started them instead (see [`attach_console`]).

// Not for the test harness: its output has to reach the console.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]
// `torro_ipc::Response` is the reply the core hands back on every path, an error reply included;
// it is returned by value throughout, and boxing it would change every handler (clippy 1.98).
#![allow(clippy::result_large_err)]

use std::process::ExitCode;

use torro_core::AppId;

mod account;
mod approvals;
mod apps;
mod assistant;
mod audit;
mod bots;
mod categorize;
mod cli;
mod client_setup;
mod clients;
mod contacts;
mod core;
mod daemon;
mod designs;
mod extensions;
mod hook;
mod inbox;
mod inbox_notify;
mod mail;
mod mcp;
mod notify;
mod pim;
mod places;
mod reminders;
mod settings_service;
mod sources;
// Loading templates at login (issue #243, task WS.5).
mod startup_templates;
mod suggest;
// The assistant's own tools (issue #225): the PDF tool, its register and its commands.
mod tools;
mod tray;
mod ui;
// The template keys (issue #243): what they are, and binding them on Hyprland (task WS.3).
mod workspace_keys;

const APP: AppId = AppId::WORKPLACE;

fn main() -> ExitCode {
    torro_core::timing::start();
    let command = std::env::args().nth(1);
    if needs_console(command.as_deref()) {
        attach_console();
    }
    match command.as_deref() {
        Some("daemon") => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            daemon::run(daemon_headless(&args, apps::main_app_installed))
        }
        None | Some("ui") => {
            // Refuse bad parameters before anything starts (the core, a window).
            let launch = match ui::Launch::parse(&std::env::args().skip(2).collect::<Vec<_>>()) {
                Ok(launch) => launch,
                Err(err) => {
                    // Started from a shell, the error belongs there (Windows; elsewhere a no-op).
                    attach_console();
                    eprintln!("{}: {err}", APP.binary);
                    return ExitCode::from(64);
                }
            };
            match ui::run(launch) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    attach_console();
                    eprintln!("{}: {err}", APP.binary);
                    ExitCode::FAILURE
                }
            }
        }
        Some("ctl" | "status" | "notify") => cli::from_args(&std::env::args().skip(1).collect::<Vec<_>>()),
        Some("hook") => hook::from_args(&std::env::args().skip(2).collect::<Vec<_>>()),
        Some("mcp") => mcp::run(),
        Some("--version") | Some("-V") => {
            println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("{}: unknown command {other:?}; try daemon, status, ctl, notify, hook or mcp", APP.binary);
            ExitCode::from(64)
        }
    }
}

/// Whether `command` (the first argument) is a command-line command rather than the control centre.
///
/// Everything except `ui` and no command at all prints to a terminal or speaks over stdio: `daemon`,
/// `status`, `ctl`, `mcp`, `--version` and unknown commands (for their error message).
fn needs_console(command: Option<&str>) -> bool {
    !matches!(command, None | Some("ui"))
}

/// Whether `daemon` runs without a tray icon and setup: with `--headless`, or when the main application
/// is not installed (issue #408, decision 1 in `docs/einzelne-apps.md`), whoever started the service: an
/// app, an MCP server, the login or a terminal. `main_app` is asked only without `--headless`.
fn daemon_headless(args: &[String], main_app: impl FnOnce() -> bool) -> bool {
    args.iter().any(|arg| arg == "--headless") || !main_app()
}

/// Windows: makes `println!`/`eprintln!` reach the console of the shell that started us.
///
/// A GUI-subsystem program gets no console, so its standard handles are empty when it is started
/// from PowerShell or `cmd`, and the output would vanish. Then we attach to the parent's console,
/// which fills the empty handles; Rust's stdio asks for the handle on every write, so printing
/// works from here on.
///
/// When any standard handle is already there, the caller redirected it (pipes from an MCP client
/// starting `torroworkplace mcp`, `status > file`, a CI runner) and all three are left alone:
/// attaching must not swap a pipe for the console and break the protocol on stdout.
///
/// Without a parent console (double click, autostart) nothing happens. Shells do not wait for GUI
/// programs, so the prompt may come back before the output.
#[cfg(windows)]
fn attach_console() {
    use std::io;
    use std::os::windows::io::{AsRawHandle, RawHandle};

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

    // `as_raw_handle` returns what `GetStdHandle` holds: null when the process has no such handle.
    // Checking this in safe Rust keeps the unsafe part down to the one call below.
    let present = |handle: RawHandle| !handle.is_null() && handle != INVALID_HANDLE_VALUE;
    let handles = [io::stdin().as_raw_handle(), io::stdout().as_raw_handle(), io::stderr().as_raw_handle()];
    if handles.into_iter().any(present) {
        return;
    }
    // The only unsafe code in this crate (see the `[lints]` note in Cargo.toml): Rust's std has no
    // safe way to attach to a console.
    #[allow(unsafe_code)]
    // SAFETY: `AttachConsole` takes a process id by value and touches no memory of ours. It fails
    // when the parent has no console or this process already has one; both leave nothing to do,
    // so the result is ignored.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// Other systems: a program started from a terminal already writes to it; nothing to do.
#[cfg(not(windows))]
fn attach_console() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_need_the_console_and_the_control_centre_does_not() {
        for command in ["daemon", "status", "ctl", "mcp", "--version", "-V", "bogus"] {
            assert!(needs_console(Some(command)), "{command}");
        }
        assert!(!needs_console(None));
        assert!(!needs_console(Some("ui")));
    }

    #[test]
    fn the_service_is_silent_without_the_main_app() {
        let headless = ["--headless".to_string()];
        assert!(!daemon_headless(&[], || true), "with the main app as before");
        assert!(daemon_headless(&[], || false), "without it no tray, no setup");
        assert!(daemon_headless(&headless, || true));
        assert!(daemon_headless(&headless, || panic!("not asked with --headless")));
    }
}
