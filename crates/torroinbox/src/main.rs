//! `torroinbox` — the inbox for the tiled desktop (`docs/eingang.md`).
//!
//! An Iced window with the fixed app id `torro-inbox`: everything that wants the user's attention
//! in one place – waiting coding agents, approvals, account problems, notices from scripts – in
//! the lanes "For me" and "Info". The app reads and changes entries only through
//! [`torro_inbox::InboxSource`]; the core checks rights and the security rules.
//!
//! `--demo` runs on sample data ([`DemoSource`]). Without it the app runs on an empty source for
//! now ([`empty::EmptySource`]); the client for the core comes with task E1.3 and is chosen here.
//! With `--demo`, `--open <state>` starts in a given state and `--now <YYYY-MM-DDTHH:MM>` fixes
//! "now"; `--appearance light|dark|system` overrides the stored appearance (see [`app::Launch`]).

mod app;
mod detail;
mod empty;
mod list;

use std::process::ExitCode;
use std::sync::Arc;

use torro_gui::source::Source;
use torro_inbox::InboxSource;
use torro_inbox::demo::DemoSource;

use crate::app::{App, INBOX, Launch};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{} {}", INBOX.binary, env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // Refuse bad parameters before anything starts (the core, a window).
    let launch = match Launch::parse(&args) {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("{}: {err}", INBOX.binary);
            return ExitCode::from(64);
        }
    };
    // The suite's settings (appearance, zoom) and "open another app" come from the core; with
    // `--demo` there is none and they stay in this process.
    let suite = Source::from_args(args.iter().cloned());
    let result = if launch.demo {
        run(Arc::new(DemoSource::new(launch.start())), suite, launch)
    } else {
        run(Arc::new(empty::EmptySource), suite, launch)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}: {err}", INBOX.binary);
            ExitCode::FAILURE
        }
    }
}

/// The window on `inbox`. Generic, so a new source (the core's client, E1.3) only changes `main`.
fn run<S: InboxSource + 'static>(inbox: Arc<S>, suite: Source, launch: Launch) -> iced::Result {
    iced::application(move || App::new(inbox.clone(), suite.clone(), launch.clone()), App::update, App::view)
        .settings(torro_gui::settings())
        .title(App::title)
        .theme(App::theme)
        .scale_factor(App::scale_factor)
        .subscription(App::subscription)
        .window(torro_gui::window(INBOX))
        .run()
}
