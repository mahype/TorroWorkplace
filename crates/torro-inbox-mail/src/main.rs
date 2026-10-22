//! `torro-inbox-mail` — the mail window: what came in by mail
//! (issue #279, decision 2 in `docs/lebenszyklus.md`).
//!
//! The same window as `torroinbox --view mail`, as a program of its own so the Dock, the taskbar and the
//! launcher show it with its own name and icon. It starts the inbox library with its window kind
//! ([`torroinbox::start`]): without `--view` it opens `--view mail`, a `--view` given stays, and every other
//! parameter means what it means to `torroinbox`.
//! Its window class is `torro-inbox-mail`, the name of this program.
//!
//! On Windows it is a GUI program without a console window; the library writes a failed start to
//! `start-error.log` in the inbox's state directory instead.

// Not for the test harness: its output has to reach the console.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

use std::process::ExitCode;

use torro_core::AppId;

/// The entry of the suite this program opens.
const APP: AppId = AppId::INBOX_MAIL;

fn main() -> ExitCode {
    torroinbox::start(APP, std::env::args().skip(1).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The program is named after its window class (`AppId::program_file`), and `build.rs` gives Windows the
    /// entry's display name.
    #[test]
    fn the_program_is_its_entry() {
        assert_eq!(env!("CARGO_PKG_NAME"), APP.program_file());
        assert_eq!(APP.program_file(), APP.window_class);
        assert_eq!(env!("TORRO_PRODUCT_NAME"), APP.display_name);
    }

    /// Without `--view` the program opens its own window kind; a given one stays.
    #[test]
    fn it_adds_its_view_unless_one_is_given() {
        assert_eq!(torroinbox::launch_args(APP, Vec::new()), ["--view", "mail"]);
        let given = vec!["--view".to_string(), "hub".to_string()];
        assert_eq!(torroinbox::launch_args(APP, given.clone()), given);
    }
}
