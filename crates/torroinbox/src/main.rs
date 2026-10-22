//! `torroinbox` — the program of the Notification Hub, and of every inbox window started with `--view`
//! or `--compose` (issue #279). All it does is in the library ([`torroinbox::start`]); Mail and App Feed
//! have programs of their own (`torro-inbox-mail`, `torro-inbox-notifications`).

use std::process::ExitCode;

use torro_core::AppId;

fn main() -> ExitCode {
    torroinbox::start(AppId::INBOX, std::env::args().skip(1).collect())
}
