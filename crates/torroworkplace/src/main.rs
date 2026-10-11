//! `torroworkplace` — the core of the suite.
//!
//! - `daemon`: the background service with the tray icon. Owns accounts and their
//!   passwords, the CalDAV sync and offline cache, the registry of suite apps (start,
//!   focus), the window sets of workspace-setups, and the socket the apps talk to.
//!   `--headless` runs it without a tray icon.
//! - `tui` (default): the control centre — apps, accounts, sync, window sets.
//! - `status --json`, `ctl`: for bars and scripts.
//! - `mcp`: one MCP server for the whole suite, so AI agents talk to the core only.
//!
//! Scaffold only.

use std::process::ExitCode;

use torro_core::AppId;

mod cli;
mod tray;

const APP: AppId = AppId::WORKPLACE;

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("daemon") => daemon(),
        Some("ctl" | "status") => cli::from_args(&std::env::args().skip(1).collect::<Vec<_>>()),
        None | Some("tui") | Some("mcp") => {
            eprintln!("{}: not implemented yet ({} scaffold)", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::from(2)
        }
        Some("--version") | Some("-V") => {
            println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("{}: unknown command {other:?}; try tui, daemon, status, ctl or mcp", APP.binary);
            ExitCode::from(64)
        }
    }
}

/// Runs the background service. For now that is the tray icon only.
#[cfg(target_os = "linux")]
fn daemon() -> ExitCode {
    match tray::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}: {err}", APP.binary);
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn daemon() -> ExitCode {
    eprintln!("{}: tray not supported yet", APP.binary);
    ExitCode::FAILURE
}
