//! `torrocalendar` — calendar for the tiled desktop.
//!
//! One binary, four faces: the TUI (default), `status --json` and `ctl` for bars and
//! scripts, and `mcp` so AI agents can read and write calendar. Scaffold only.

use std::process::ExitCode;

use torro_core::AppId;

const APP: AppId = AppId::CALENDAR;

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        None | Some("tui") | Some("status") | Some("ctl") | Some("mcp") => {
            eprintln!("{}: not implemented yet ({} scaffold)", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::from(2)
        }
        Some("--version") | Some("-V") => {
            println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("{}: unknown command {other:?}; try tui, status, ctl or mcp", APP.binary);
            ExitCode::from(64)
        }
    }
}
