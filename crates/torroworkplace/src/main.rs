//! `torroworkplace` — the core of the suite.
//!
//! - `daemon`: the background service with the tray icon. Owns the window manager
//!   connection and the workspace templates, and the socket clients talk to; later also
//!   accounts, sync and the app registry. `--headless` runs it without a tray icon.
//! - `status --json`, `ctl`: for bars, hotkeys and scripts.
//! - later: `ui` (control centre, Iced) and `mcp` (one MCP server for the whole suite).

use std::process::ExitCode;

use torro_core::AppId;

mod cli;
mod core;
mod daemon;
mod notify;
mod tray;

const APP: AppId = AppId::WORKPLACE;

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("daemon") => daemon::run(std::env::args().any(|arg| arg == "--headless")),
        Some("ctl" | "status") => cli::from_args(&std::env::args().skip(1).collect::<Vec<_>>()),
        None | Some("ui") | Some("mcp") => {
            eprintln!("{}: not implemented yet ({} scaffold)", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::from(2)
        }
        Some("--version") | Some("-V") => {
            println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("{}: unknown command {other:?}; try daemon, status or ctl", APP.binary);
            ExitCode::from(64)
        }
    }
}
