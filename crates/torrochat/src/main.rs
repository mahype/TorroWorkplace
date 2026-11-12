//! `torrochat` — the chat of the tiled desktop (issue #643, `docs/chat.md`, stage 3).
//!
//! A contact list in a narrow window of its own (class `torro-chat`): who is there with their
//! status, the group chats, the own status. A double click on a name opens the chat window of the
//! conversation, `torrochat --conversation=<id>` (class `torro-chat-conversation`), which floats;
//! a second start on the same conversation brings that window to the front and ends
//! ([`torro_gui::instance::claim_named`], decision 2). The core holds conversations, people and
//! status; the windows only send requests of `chat` and `presence` ([`backend::Backend`]). Without
//! `--demo` the app talks to the core over its socket and starts the core when it is not running.
//! A picture of a chat opens large in a window of its own, `torrochat --viewer=<message>/<file>`
//! (class `torro-chat-viewer`, stage 4). An incoming call rings in a small window,
//! `torrochat --ring=<conversation>` (class `torro-chat-ring`, stage 6b), which the core starts.
//!
//! `--demo` runs on the sample of `torro_chat::demo` and writes nothing; `--open <state>` starts in a
//! state (`contacts`, `chat`, …, see [`launch::Open`]); `--demo --shots <dir>` writes the view at each
//! size class as PNG files instead of opening a window ([`torro_gui::shots`], issue #492).
//!
//! One contact list per person and one window per conversation (issue #279): a second plain start
//! brings the contact list to the front; the demo starts as before ([`instance_of`]).

// Not for the test harness: its output has to reach the console.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod app;
mod backend;
mod launch;
mod people;
mod system;
mod timeline;
mod view;

use std::process::ExitCode;

use torro_gui::instance::Claim;
use torro_gui::shots::{RunError, Shots};
use torro_gui::source::Source;

use crate::app::{App, CONTACTS, CONTACTS_SIZE, CONVERSATION, CONVERSATION_SIZE, RING, RING_SIZE, VIEWER, VIEWER_SIZE};
use crate::backend::Backend;
use crate::launch::{Launch, Window};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{} {}", CONTACTS.binary, env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // Refuse bad parameters before anything starts (the core, a window).
    let launch = match Launch::parse(&args) {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("{}: {err}", CONTACTS.binary);
            return ExitCode::from(64);
        }
    };
    // `--demo --shots <dir>` (issue #492): the views as pictures, without a window.
    let shots = match Shots::from_args(CONTACTS.binary, &args) {
        Ok(shots) => shots,
        Err(err) => {
            eprintln!("{}: {err}", CONTACTS.binary);
            return ExitCode::from(64);
        }
    };
    // Before the core is started: a second start only brings the open window to the front.
    if let Some(claim) = instance_of(&launch)
        && claim() == Claim::Shown
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
            eprintln!("{}: {err}", CONTACTS.binary);
            ExitCode::FAILURE
        }
    }
}

/// How this start claims its window ([`torro_gui::instance`], issue #279): the contact list by its
/// class, a chat window by its conversation (decision 2 of `docs/chat.md`); `None` for the demo,
/// which the visual checks start next to the real windows.
fn instance_of(launch: &Launch) -> Option<Box<dyn FnOnce() -> Claim>> {
    if launch.demo {
        return None;
    }
    Some(match &launch.window {
        Window::Contacts => Box::new(|| torro_gui::instance::claim(CONTACTS)),
        Window::Conversation(id) => {
            let id = id.clone();
            Box::new(move || torro_gui::instance::claim_named(CONTACTS, &id))
        }
        // The same picture twice brings its window to the front.
        Window::Viewer { message, file } => {
            let name = format!("viewer:{message}/{file}");
            Box::new(move || torro_gui::instance::claim_named(CONTACTS, &name))
        }
        // One ringing window per conversation, however often the core starts it.
        Window::Ring(id) => {
            let name = format!("ring:{id}");
            Box::new(move || torro_gui::instance::claim_named(CONTACTS, &name))
        }
    })
}

/// The window on `backend`: the contact list or a chat window, as `launch` says; with `shots` its
/// pictures instead ([`torro_gui::shots::run`]).
fn run(backend: Backend, suite: Source, launch: Launch, shots: Option<&Shots>) -> Result<(), RunError> {
    let window = match launch.window {
        Window::Contacts => iced::window::Settings { size: CONTACTS_SIZE, ..torro_gui::window(CONTACTS) },
        Window::Conversation(_) => {
            iced::window::Settings { size: CONVERSATION_SIZE, ..torro_gui::window(CONVERSATION) }
        }
        Window::Viewer { .. } => iced::window::Settings { size: VIEWER_SIZE, ..torro_gui::window(VIEWER) },
        // In the middle, over the tiles (decision 24); without a tiling manager it floats by itself.
        Window::Ring(_) => iced::window::Settings {
            size: RING_SIZE,
            position: iced::window::Position::Centered,
            level: iced::window::Level::AlwaysOnTop,
            ..torro_gui::window(RING)
        },
    };
    let application =
        iced::application(move || App::new(backend.clone(), suite.clone(), launch.clone()), App::update, App::view)
            .settings(torro_gui::settings())
            .title(App::title)
            .theme(App::theme)
            .scale_factor(App::scale_factor)
            .subscription(App::subscription)
            .window(window);
    torro_gui::shots::run(application, shots)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(words: &[&str]) -> Launch {
        let args: Vec<String> = words.iter().map(|word| (*word).to_string()).collect();
        Launch::parse(&args).expect("valid parameters")
    }

    /// Issue #279 and decision 2 of the chat: the contact list and each chat window claim a channel;
    /// the demo none.
    #[test]
    fn the_demo_claims_no_window() {
        assert!(instance_of(&launch(&[])).is_some());
        assert!(instance_of(&launch(&["--conversation=nextcloud:a@b/tok"])).is_some());
        assert!(instance_of(&launch(&["--demo"])).is_none());
        assert!(instance_of(&launch(&["--demo", "--open", "chat"])).is_none());
    }
}
