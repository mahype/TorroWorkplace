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
//! back with its argument; `--bot <id>` is the chat with a bot in such a window (task B.3). The
//! start parameters are [`launch::Launch`]'s.
//!
//! `torroassistant --settings <section>` opens the main or chat window with the assistant's
//! settings in its content area (task S.18): the sections of `torro_settings_ui::assistant`, over
//! the window's own way to the core ([`Backend::settings`]) – with `--demo` over the settings
//! library's demo, which never opens a socket.
//!
//! Without a window: `--listen` waits for the wake word and talks with the assistant by voice
//! (task C.9); `--voice-status` and `--voice-test` answer the core's `settings.voice_status` and
//! `settings.test_voice` ([`voice::cli`]).
//!
//! `TORRO_TIMING=1` writes when the window first draws, when the conversations first came and every
//! `update` or `view` that takes a frame or longer ([`torro_core::timing`], issue #206), measured by
//! the hulls [`update`] and [`view`] here.

mod app;
mod backend;
mod bots;
mod chat;
mod demo;
mod launch;
mod stream;
mod view;
mod voice;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use iced::{Element, Task};
use torro_core::timing;
use torro_gui::source::Source;

use crate::app::{App, CHAT, CHAT_SIZE, MAIN, Message};
use crate::backend::Backend;
use crate::launch::{Launch, Open, Window};

fn main() -> ExitCode {
    timing::start();
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
        Window::Chat(_) | Window::Bot(_) => iced::window::Settings { size: CHAT_SIZE, ..torro_gui::window(CHAT) },
    };
    if let Some((width, height)) = launch.size {
        window.size = iced::Size::new(f32::from(width), f32::from(height));
    }
    iced::application(move || App::new(backend.clone(), suite.clone(), launch.clone()), update, view)
        .settings(torro_gui::settings())
        .title(App::title)
        .theme(App::theme)
        .scale_factor(App::scale_factor)
        .subscription(App::subscription)
        .window(window)
        .run()
}

/// [`App::update`], timed: the first list of conversations marks "first data", a slow update is
/// written.
fn update(app: &mut App, message: Message) -> Task<Message> {
    if matches!(message, Message::Listed(Ok(_))) {
        timing::once("first data");
    }
    let started = Instant::now();
    let task = app.update(message);
    timing::slow("update", started);
    task
}

/// [`App::view`], timed: the first call marks "first frame", a slow one is written.
fn view(app: &App) -> Element<'_, Message> {
    let started = Instant::now();
    let element = app.view();
    timing::once("first frame");
    timing::slow("view", started);
    element
}
