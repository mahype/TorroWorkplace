//! `torrotasks` — tasks for the tiled desktop.
//!
//! A client of the core (`torroworkplace daemon`): the core owns accounts and sync, this
//! app shows and edits tasks. Faces: the TUI (default), `status --json` and `ctl` for bars
//! and scripts. AI agents reach tasks through the core's MCP server. Scaffold only.

use std::process::ExitCode;

use torro_core::AppId;

const APP: AppId = AppId::TASKS;

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        None | Some("tui") | Some("status") | Some("ctl") => {
            eprintln!("{}: not implemented yet ({} scaffold)", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::from(2)
        }
        Some("--version") | Some("-V") => {
            println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("{}: unknown command {other:?}; try tui, status or ctl", APP.binary);
            ExitCode::from(64)
        }
    }
}
