//! `torroworkplace` — the core of the suite.
//!
//! - `daemon`: the background service with the tray icon. Owns the window manager
//!   connection and the workspace templates, and the socket clients talk to; later also
//!   accounts, sync and the app registry. `--headless` runs it without a tray icon.
//! - `status --json`, `ctl`: for bars, hotkeys and scripts.
//! - `ui` (also without a command): the control centre, an Iced window for accounts,
//!   workspaces, apps and general settings. `ui --demo [--open <state>]` shows sample data without
//!   the service (see `ui::Launch`); `--section` and `--appearance light|dark|system` work too.
//! - `mcp`: one MCP server for the whole suite, on stdio, with the agent's key from
//!   `TORRO_TOKEN`.

use std::process::ExitCode;

use torro_core::AppId;

mod account;
mod approvals;
mod apps;
mod cli;
mod clients;
mod core;
mod daemon;
mod mcp;
mod notify;
mod pim;
mod settings_service;
mod tray;
mod ui;

const APP: AppId = AppId::WORKPLACE;

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("daemon") => daemon::run(std::env::args().any(|arg| arg == "--headless")),
        None | Some("ui") => {
            // Refuse bad parameters before anything starts (the core, a window).
            let launch = match ui::Launch::parse(&std::env::args().skip(2).collect::<Vec<_>>()) {
                Ok(launch) => launch,
                Err(err) => {
                    eprintln!("{}: {err}", APP.binary);
                    return ExitCode::from(64);
                }
            };
            match ui::run(launch) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("{}: {err}", APP.binary);
                    ExitCode::FAILURE
                }
            }
        }
        Some("ctl" | "status") => cli::from_args(&std::env::args().skip(1).collect::<Vec<_>>()),
        Some("mcp") => mcp::run(),
        Some("--version") | Some("-V") => {
            println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("{}: unknown command {other:?}; try daemon, status, ctl or mcp", APP.binary);
            ExitCode::from(64)
        }
    }
}
