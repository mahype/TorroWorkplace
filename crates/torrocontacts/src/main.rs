//! `torrocontacts` — the contacts of the tiled desktop (issue #352, `docs/kontakte.md`, draft D2).
//!
//! The address books of the person's accounts in one window (class `torro-contacts`): a sidebar with
//! all contacts, favourites, the ones used last, the birthdays and the books with their groups; the
//! contacts grouped by group, town or book in a list; the contact in a side panel, where it is edited.
//! As a mini tile the favourites with Mail and Call, as a strip the next birthday.
//!
//! The core holds the contacts; the window only sends requests of the area `contacts`
//! ([`backend::Backend`]). Without `--demo` it talks to the core over its socket and starts the core
//! when it is not running ([`Source::from_args`]).
//!
//! `--demo` runs on the sample of `torro_contacts::demo`; `--open <state>` starts in a state (`list`,
//! `contact`, `edit`, …, see [`launch::Open`]); `--demo --shots <dir>` writes the view at each size
//! class as PNG files instead of opening a window ([`torro_gui::shots`], issue #492).
//!
//! One window per person (issue #279): a second plain start brings the open window to the front; the
//! demo starts as before ([`instance_of`]).

// Not for the test harness: its output has to reach the console.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod app;
mod backend;
mod contact;
mod edit;
mod glance;
mod launch;
mod list;
mod view;

use std::process::ExitCode;

use torro_core::AppId;
use torro_gui::instance::Claim;
use torro_gui::shots::{RunError, Shots};
use torro_gui::source::Source;

use crate::app::{APP, App};
use crate::backend::Backend;
use crate::launch::Launch;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{} {}", APP.binary, env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // Refuse bad parameters before anything starts (the core, a window).
    let launch = match Launch::parse(&args) {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("{}: {err}", APP.binary);
            return ExitCode::from(64);
        }
    };
    // `--demo --shots <dir>` (issue #492): the views as pictures, without a window.
    let shots = match Shots::from_args(APP.binary, &args) {
        Ok(shots) => shots,
        Err(err) => {
            eprintln!("{}: {err}", APP.binary);
            return ExitCode::from(64);
        }
    };
    // Before the core is started: a second start only brings the open window to the front.
    if let Some(app) = instance_of(&launch)
        && torro_gui::instance::claim(app) == Claim::Shown
    {
        return ExitCode::SUCCESS;
    }
    // Appearance, zoom and design come from the core; with `--demo` there is none.
    let suite = Source::from_args(args.iter().cloned());
    torro_gui::design::start_for(&suite, &launch.forced());
    let backend = if launch.demo { Backend::demo() } else { Backend::core() };
    match run(backend, suite, launch, shots.as_ref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}: {err}", APP.binary);
            ExitCode::FAILURE
        }
    }
}

/// The window whose single instance this start is ([`torro_gui::instance`], issue #279): the window of
/// a plain start; `None` for the demo, which the visual checks start next to the real window.
fn instance_of(launch: &Launch) -> Option<AppId> {
    (!launch.demo).then_some(APP)
}

/// The window on `backend`; with `shots` its pictures instead ([`torro_gui::shots::run`]).
fn run(backend: Backend, suite: Source, launch: Launch, shots: Option<&Shots>) -> Result<(), RunError> {
    let application =
        iced::application(move || App::new(backend.clone(), suite.clone(), launch.clone()), App::update, App::view)
            .settings(torro_gui::settings())
            .title(App::title)
            .theme(App::theme)
            .scale_factor(App::scale_factor)
            .subscription(App::subscription)
            .window(torro_gui::window(APP));
    torro_gui::shots::run(application, shots)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #279: a plain start is one per person, the demo is not.
    #[test]
    fn the_demo_claims_no_window() {
        let parse = |words: &[&str]| Launch::parse(&words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>());
        assert_eq!(instance_of(&parse(&[]).expect("valid")), Some(AppId::CONTACTS));
        assert_eq!(instance_of(&parse(&["--demo", "--open", "contact"]).expect("valid")), None);
    }
}
