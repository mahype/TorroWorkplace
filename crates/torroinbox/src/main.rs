//! `torroinbox` — the inbox for the tiled desktop (`docs/eingang.md`).
//!
//! Three windows from one program (task E8.1, [`app::Kind`]): `--view mail` (app id
//! `torro-inbox-mail`), `--view notifications` (`torro-inbox-notifications`) and `--view hub` or no
//! `--view` (`torro-inbox`), each its own entry of `torro_core::AppId::ALL`; an unknown `--view` is
//! refused with the valid ones (exit 64). Each shows its entries in one of two densities (task
//! E8.2, `torro_gui::ui_state::Density`): `--density compact|comfortable` at start, else the one
//! the window remembers in `ui.json`, else comfortable (compact when narrow); an unknown one is
//! refused the same way.
//!
//! An Iced window with a fixed app id per kind: everything that wants the user's attention
//! in one place – waiting coding agents, approvals, account problems, notices from scripts – in
//! the lanes "For me" and "Info". The app reads and changes entries only through
//! [`torro_inbox::InboxSource`]; the core checks rights and the security rules.
//!
//! `--demo` runs on sample data ([`DemoSource`]). Without it the app talks to the core over its
//! socket ([`InboxClient`], task E1.3); [`Source::from_args`] starts the core first when it is not
//! running, and until it answers the app shows "isn't responding" and asks again on its timer.
//! With `--demo`, `--open <state>` starts in a given state and `--now <YYYY-MM-DDTHH:MM>` fixes
//! "now"; `--appearance light|dark|system` overrides the stored appearance and `--design <id>` the
//! design pack (see [`app::Launch`]).
//!
//! One window per slice (E3.6): `--lane for-me|info` or `--source <key>` (`suite`,
//! `agent:claude-code`, `script:backup`) starts on that view. The core opens the app without
//! arguments (tray, control centre); a workspace template stores the window's whole command line,
//! so `torroinbox --lane info` comes back as an "Info" window.
//!
//! `--compose` starts the compose window instead of the inbox (task E4.3, [`compose`]): its own
//! window with the class `torro-compose`, started by the panel's Reply, Reply all and Forward and
//! by "New" ([`mail::compose_command`]).

mod app;
mod compose;
mod detail;
#[cfg(test)]
mod empty;
mod glance;
mod keys;
mod list;
mod mail;
mod overview;

use std::process::ExitCode;
use std::sync::Arc;

use torro_gui::source::Source;
use torro_inbox::InboxSource;
use torro_inbox::demo::DemoSource;
use torro_ipc::inbox::InboxClient;

use crate::app::{App, INBOX, Launch};
use crate::compose::{ComposeSource, Window};
use crate::mail::MailSource;

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
    // Before the settings load the fonts: the first frame is drawn with the chosen design.
    torro_gui::design::start_for(&suite, launch.design.as_deref());
    let result = match (launch.compose.is_some(), launch.demo) {
        (true, true) => compose(Arc::new(DemoSource::new(launch.start())), suite, launch),
        (true, false) => compose(Arc::new(InboxClient::default()), suite, launch),
        (false, true) => run(Arc::new(DemoSource::new(launch.start())), suite, launch),
        (false, false) => run(Arc::new(InboxClient::default()), suite, launch),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}: {err}", INBOX.binary);
            ExitCode::FAILURE
        }
    }
}

/// The window on `inbox`: the core's client or the demo source. Its window class is the one of its
/// kind (`--view`, task E8.1), so tiling rules and templates tell the three windows apart.
fn run<S: InboxSource + MailSource + 'static>(inbox: Arc<S>, suite: Source, launch: Launch) -> iced::Result {
    let window = torro_gui::window(launch.kind.app());
    iced::application(move || App::new(inbox.clone(), suite.clone(), launch.clone()), App::update, App::view)
        .settings(torro_gui::settings())
        .title(App::title)
        .theme(App::theme)
        .scale_factor(App::scale_factor)
        .subscription(App::subscription)
        .window(window)
        .run()
}

/// The compose window on `source`: the core's client or the demo source.
fn compose<S: ComposeSource + 'static>(source: Arc<S>, suite: Source, launch: Launch) -> iced::Result {
    iced::application(move || Window::new(source.clone(), suite.clone(), launch.clone()), Window::update, Window::view)
        .settings(torro_gui::settings())
        .title(Window::title)
        .theme(Window::theme)
        .scale_factor(Window::scale_factor)
        .subscription(Window::subscription)
        .window(compose::window_settings())
        .run()
}
