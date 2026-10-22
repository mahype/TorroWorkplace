//! `torroinbox` — the program of the Notification Hub, and of every inbox window started with `--view`
//! or `--compose` (issue #279). All it does is in the library ([`torroinbox::start`]); Mail and App Feed
//! have programs of their own (`torro-inbox-mail`, `torro-inbox-notifications`).
//!
//! On Windows it is a GUI program without a console window; the library writes a failed start to
//! `start-error.log` in the inbox's state directory instead.

// Here and not in `lib.rs`: the subsystem belongs to a program's own crate root. Not for the test harness:
// its output has to reach the console.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

use std::process::ExitCode;

use torro_core::AppId;

fn main() -> ExitCode {
    torroinbox::start(AppId::INBOX, std::env::args().skip(1).collect())
}
