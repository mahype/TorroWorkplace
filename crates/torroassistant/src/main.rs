//! `torroassistant` — the assistant of the tiled desktop (task C.4, `docs/assistent.md`).
//!
//! An Iced window to chat with the suite: "What is on tomorrow, and which mails about it are still
//! open?". The core holds the conversations and runs the answers with the assistant's own rights;
//! this window only sends requests (`assistant.*`, `ai.list_models`, `approvals.*`) and follows the
//! answers ([`app`]). Without `--demo` it talks to the core over its socket and starts the core
//! when it is not running ([`Source::from_args`]).
//!
//! Two kinds of window from one program: `torroassistant` is the main window (class
//! `torro-assistant`), `torroassistant --chat <id>|new` a single chat window (class
//! `torro-assistant-chat`), which `assistant.open_window` starts and a workspace template brings
//! back with its argument. `--bot <id>` is for stage 2 and refused until then. The start
//! parameters are [`launch::Launch`]'s.
//!
//! Without a window: `--listen` waits for the wake word and talks with the assistant by voice
//! (task C.9); `--voice-status` and `--voice-test` answer the core's `settings.voice_status` and
//! `settings.test_voice` ([`voice::cli`]).

mod app;
mod backend;
mod chat;
mod demo;
mod launch;
mod view;
mod voice;

use std::process::ExitCode;
use std::sync::Arc;

use torro_gui::source::Source;

use crate::app::{App, CHAT, CHAT_SIZE, MAIN};
use crate::backend::Backend;
use crate::launch::{Launch, Open, Window};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{} {}", MAIN.binary, env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // The voice modes run without a window (task C.9).
    match voice::cli::Mode::parse(&args) {
        Ok(Some(mode)) => return mode.run(),
        Ok(None) => {}
        Err(err) => {
            eprintln!("{}: {err}", MAIN.binary);
            return ExitCode::from(64);
        }
    }
    // Refuse bad parameters before anything starts (the core, a window).
    let launch = match Launch::parse(&args) {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("{}: {err}", MAIN.binary);
            return ExitCode::from(64);
        }
    };
    // Appearance, zoom, design and "open another app" come from the core; with `--demo` there is
    // none and they stay in this process.
    let suite = Source::from_args(args.iter().cloned());
    torro_gui::design::start_for(&suite, launch.design.as_deref());
    let backend = if launch.demo { Backend::Demo(Arc::new(demo_for(launch.open))) } else { Backend::core() };
    match run(backend, suite, launch) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}: {err}", MAIN.binary);
            ExitCode::FAILURE
        }
    }
}

/// The demo for a start state: the first start without the assistant's client, no AI account, no
/// conversations; else everything.
fn demo_for(open: Option<Open>) -> demo::Demo {
    let setup = match open {
        Some(Open::Setup) => demo::Setup { client: false, ..demo::Setup::default() },
        Some(Open::NoEngines) => demo::Setup { engines: false, ..demo::Setup::default() },
        Some(Open::Empty) => demo::Setup { conversations: false, ..demo::Setup::default() },
        _ => demo::Setup::default(),
    };
    demo::Demo::new(chrono::Utc::now(), setup)
}

/// The window on `backend`: the main window or a chat window, as `launch` says.
fn run(backend: Backend, suite: Source, launch: Launch) -> iced::Result {
    let mut window = match launch.window {
        Window::Main => torro_gui::window(MAIN),
        Window::Chat(_) => iced::window::Settings { size: CHAT_SIZE, ..torro_gui::window(CHAT) },
    };
    if let Some((width, height)) = launch.size {
        window.size = iced::Size::new(f32::from(width), f32::from(height));
    }
    iced::application(move || App::new(backend.clone(), suite.clone(), launch.clone()), App::update, App::view)
        .settings(torro_gui::settings())
        .title(App::title)
        .theme(App::theme)
        .scale_factor(App::scale_factor)
        .subscription(App::subscription)
        .window(window)
        .run()
}
