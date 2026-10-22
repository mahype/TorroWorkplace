//! `torroinbox` — the inbox for the tiled desktop (`docs/eingang.md`), as a library: [`start`] runs it
//! with a program's arguments.
//!
//! Three programs run it (issue #279, decision 2 in `docs/lebenszyklus.md`): `torroinbox` (the
//! Notification Hub, and every window with `--view`), `torro-inbox-mail` and `torro-inbox-notifications`,
//! each in a crate of its own, because Windows shows the icon built into a program and `winresource`
//! builds the same one into every program of a crate. The two start their window kind without
//! `--view` ([`launch_args`]); the code of the windows stays one.
//!
//! Three windows from one program (task E8.1, [`app::Kind`]): `--view mail` (app id
//! `torro-inbox-mail`), `--view notifications` (`torro-inbox-notifications`) and `--view hub` or no
//! `--view` (`torro-inbox`), each its own entry of `torro_core::AppId::ALL`; an unknown `--view` is
//! refused with the valid ones (exit 64). How close their entries stand is not theirs to choose
//! (task E13.2, `torro_core::design::Density`): all three show the density of the active design,
//! one row per entry under "Torro", cards only with a pack that asks for them.
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
//! `--message=m:…` names the mail the mail window starts on (task E8.7a): the core passes it when
//! `apps.open_app` opens a mail (`mail:<address>:<Message-ID>`); a value that is no mail id of the
//! core is refused (exit 64). The window shows it (task E8.7b): the mail's entry in the panel when
//! there is one, else the mail in its folder.
//!
//! `--compose` starts the compose window instead of the inbox (task E4.3, [`compose`]): its own
//! window with the class `torro-compose`, started by the panel's Reply, Reply all and Forward and
//! by "New" ([`mail::compose_command`]).
//!
//! `--settings <section>` opens the window with its settings in the content area (task S.20,
//! `docs/settings.md`): the Notification Hub and the App Feed take `notifications`, `sources` and
//! `rules`, the mail window `accounts` and `reading`; a word the window does not know is refused
//! with the ones it does (exit 64). Only a person opens the settings – no entry of the inbox and
//! no command of the core can ([`app::App::open_entry`]).
//!
//! One window of each kind per person (issue #279, [`torro_gui::instance`], channel by the kind's
//! window class): a second plain start of a kind brings its running window to the front and ends;
//! a start on a mail (`--message`), a slice (`--lane`, `--source`), the settings (`--settings`), the
//! compose window (`--compose`) or the demo opens its window as before ([`instance_of`]).
//!
//! `TORRO_TIMING=1` writes when the window first draws, when its first data came and every
//! `update` or `view` that takes a frame or longer ([`torro_core::timing`], issue #206): the hulls
//! [`update`] and [`view`] here measure, so the app's own code stays as it is.

mod app;
mod compose;
mod detail;
#[cfg(test)]
mod empty;
mod glance;
mod hub;
mod insights;
mod keys;
mod list;
mod mail;
mod notifications;
mod overview;
mod search;
mod suggest;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use iced::{Element, Task};
use torro_core::AppId;
use torro_core::timing;
use torro_gui::instance::Claim;
use torro_gui::source::Source;
use torro_inbox::InboxSource;
use torro_inbox::demo::DemoSource;
use torro_ipc::inbox::InboxClient;

use crate::app::{App, Launch, Message};
use crate::compose::{ComposeSource, Window};
use crate::hub::HubSource;
use crate::insights::StatsSource;
use crate::mail::MailSource;
use crate::suggest::SuggestSource;

/// Runs the inbox as the program of `app`, one of `AppId::INBOX` (`torroinbox`), `AppId::INBOX_MAIL`
/// (`torro-inbox-mail`) and `AppId::INBOX_NOTIFICATIONS` (`torro-inbox-notifications`), with `args`,
/// the words after the program name; what each program's `main` does and all it does (issue #279).
///
/// The words are first completed by [`launch_args`] (`--view mail` for the mail program when none is
/// given); from there everything is as `torroinbox` always did it: `--version` prints
/// `<program> <version>`, bad parameters are refused before anything starts (exit 64), a second plain
/// start of a window kind brings the running window forward and ends with exit 0
/// ([`torro_gui::instance`], channel by the kind's window class, so `torro-inbox-mail` and
/// `torroinbox --view mail` share one), and the window or the compose window runs until it closes.
/// Messages name the program by `app.program_file()`.
pub fn start(app: AppId, args: Vec<String>) -> ExitCode {
    timing::start();
    let program = app.program_file();
    let args = launch_args(app, args);
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{program} {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // Refuse bad parameters before anything starts (the core, a window).
    let mut launch = match Launch::parse(&args) {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("{program}: {err}");
            return ExitCode::from(64);
        }
    };
    // The mail to show (tasks E8.7a, E8.7b): read and checked now, so the core's command line never
    // fails; the window opens on it once its first entries are in.
    launch.message = match start_message(&args) {
        Ok(message) => message,
        Err(err) => {
            eprintln!("{program}: {err}");
            return ExitCode::from(64);
        }
    };
    // Before the core is started: a second start only brings the first window of its kind forward.
    if let Some(app) = instance_of(&launch)
        && torro_gui::instance::claim(app) == Claim::Shown
    {
        return ExitCode::SUCCESS;
    }
    // The suite's settings (appearance, zoom) and "open another app" come from the core; with
    // `--demo` there is none and they stay in this process.
    let suite = Source::from_args(args.iter().cloned());
    // Before the settings load the fonts: the first frame is drawn with the chosen design.
    torro_gui::design::start_for(&suite, &launch.forced());
    let result = match (launch.compose.is_some(), launch.demo) {
        (true, true) => compose(Arc::new(DemoSource::new(launch.start())), suite, launch),
        (true, false) => compose(Arc::new(InboxClient::default()), suite, launch),
        (false, true) => run(Arc::new(DemoSource::new(launch.start())), suite, launch),
        (false, false) => run(Arc::new(InboxClient::default()), suite, launch),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{program}: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The words the program of `app` runs with (issue #279): `args` behind the entry's own arguments
/// (`AppId::args`, `--view mail` for `torro-inbox-mail`) when `args` hold no `--view` (neither
/// `--view <kind>` nor `--view=<kind>`), else `args` unchanged, so a `--view` given wins and is checked as
/// always. `torroinbox` (`AppId::INBOX`) has no own arguments: its words stay as they are.
pub fn launch_args(app: AppId, args: Vec<String>) -> Vec<String> {
    let has_view = args.iter().any(|arg| arg == "--view" || arg.starts_with("--view="));
    if has_view {
        return args;
    }
    app.args.iter().map(|arg| (*arg).to_string()).chain(args).collect()
}

/// The kind of window whose single instance this start is ([`torro_gui::instance::claim`], issue
/// #279): the entry of `--view` (hub, mail, notifications) for a plain start; `None` for a start on a
/// mail (`--message`), on a slice (`--lane`, `--source`, which a workspace template brings back as
/// windows of their own), the settings (`--settings`), the compose window (`--compose`) and the demo.
fn instance_of(launch: &Launch) -> Option<AppId> {
    let own_window = launch.demo
        || launch.compose.is_some()
        || launch.message.is_some()
        || launch.view.is_some()
        || matches!(launch.shown, notifications::Shown::Only(_))
        || launch.settings.is_some();
    (!own_window).then_some(launch.kind.app())
}

/// The window on `inbox`: the core's client or the demo source. Its window class is the one of its
/// kind (`--view`, task E8.1), so tiling rules and templates tell the three windows apart.
fn run<S: InboxSource + MailSource + StatsSource + HubSource + SuggestSource + 'static>(
    inbox: Arc<S>,
    suite: Source,
    launch: Launch,
) -> iced::Result {
    let window = torro_gui::window(launch.kind.app());
    iced::application(move || App::new(inbox.clone(), suite.clone(), launch.clone()), update, view)
        .settings(torro_gui::settings())
        .title(App::title)
        .theme(App::theme)
        .scale_factor(App::scale_factor)
        .subscription(App::subscription)
        .window(window)
        .run()
}

/// [`App::update`], timed: the first entries in mark "first data", a slow update is written.
fn update<S: InboxSource + MailSource + StatsSource + HubSource + SuggestSource + 'static>(
    app: &mut App<S>,
    message: Message,
) -> Task<Message> {
    if matches!(message, Message::Loaded(_, Ok(_))) {
        timing::once("first data");
    }
    let started = Instant::now();
    let task = app.update(message);
    timing::slow("update", started);
    task
}

/// [`App::view`], timed: the first call marks "first frame", a slow one is written.
fn view<S: InboxSource + MailSource + StatsSource + HubSource + SuggestSource + 'static>(
    app: &App<S>,
) -> Element<'_, Message> {
    let started = Instant::now();
    let element = app.view();
    timing::once("first frame");
    timing::slow("view", started);
    element
}

/// [`Window::update`] of the compose window, timed like [`update`].
fn compose_update<S: ComposeSource + 'static>(
    window: &mut Window<S>,
    message: compose::Message,
) -> Task<compose::Message> {
    if matches!(message, compose::Message::Loaded(Ok(_))) {
        timing::once("first data");
    }
    let started = Instant::now();
    let task = window.update(message);
    timing::slow("update", started);
    task
}

/// [`Window::view`] of the compose window, timed like [`view`].
fn compose_view<S: ComposeSource + 'static>(window: &Window<S>) -> Element<'_, compose::Message> {
    let started = Instant::now();
    let element = window.view();
    timing::once("first frame");
    timing::slow("view", started);
    element
}

/// The longest mail id `--message` takes, as the core's mail commands.
const MESSAGE_MAX: usize = 300;

/// The mail the window starts on (task E8.7a): `--message=m:…` or `--message m:…`, a mail id of the
/// core's store, as the core builds it for `apps.open_app` with a `mail:` target. `None` without
/// the flag; `Err` for a value that is no such id (it must start with `m:`, at most
/// [`MESSAGE_MAX`] bytes, no spaces or control characters) or a flag without a value.
fn start_message(args: &[String]) -> Result<Option<String>, String> {
    let mut found = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = match arg.strip_prefix("--message") {
            Some("") => rest.next().cloned(),
            Some(value) if value.starts_with('=') => Some(value[1..].to_string()),
            _ => continue,
        };
        let value = value.ok_or_else(|| "--message needs a mail id (m:…)".to_string())?;
        let valid = value.starts_with("m:")
            && value.len() > 2
            && value.len() <= MESSAGE_MAX
            && !value.chars().any(|c| c.is_whitespace() || c.is_control());
        if !valid {
            return Err(format!("--message takes a mail id (m:…), not {value:?}"));
        }
        found = Some(value);
    }
    Ok(found)
}

/// The compose window on `source`: the core's client or the demo source.
fn compose<S: ComposeSource + 'static>(source: Arc<S>, suite: Source, launch: Launch) -> iced::Result {
    iced::application(move || Window::new(source.clone(), suite.clone(), launch.clone()), compose_update, compose_view)
        .settings(torro_gui::settings())
        .title(Window::title)
        .theme(Window::theme)
        .scale_factor(Window::scale_factor)
        .subscription(Window::subscription)
        .window(compose::window_settings())
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &[&str]) -> Vec<String> {
        line.iter().map(|arg| (*arg).to_string()).collect()
    }

    /// Task E8.7a: the core's `--message=m:…` is read in both forms; anything else is refused.
    #[test]
    fn the_start_mail_is_a_mail_id_of_the_core() {
        assert_eq!(start_message(&args(&["--view", "mail", "--message=m:0a1b"])), Ok(Some("m:0a1b".into())));
        assert_eq!(start_message(&args(&["--message", "m:0a1b"])), Ok(Some("m:0a1b".into())));
        assert_eq!(start_message(&args(&["--view", "mail", "--messages"])), Ok(None), "another flag");
        assert_eq!(start_message(&args(&[])), Ok(None));
        for bad in
            [&["--message"][..], &["--message=x"], &["--message=m:"], &["--message=m:a b"], &["--message", "--demo"]]
        {
            assert!(start_message(&args(bad)).is_err(), "{bad:?}");
        }
        assert!(start_message(&args(&[&format!("--message=m:{}", "a".repeat(MESSAGE_MAX))])).is_err(), "too long");
    }

    /// Issue #279: each of the three plain windows is one per person, by its own window class.
    #[test]
    fn only_plain_starts_are_single_instances_of_their_kind() {
        let of = |line: &[&str]| {
            let args = args(line);
            let mut launch = Launch::parse(&args).expect("valid parameters");
            launch.message = start_message(&args).expect("a valid mail id");
            instance_of(&launch)
        };
        assert_eq!(of(&[]), Some(AppId::INBOX));
        assert_eq!(of(&["--view", "hub"]), Some(AppId::INBOX));
        assert_eq!(of(&["--view", "mail"]), Some(AppId::INBOX_MAIL));
        assert_eq!(of(&["--view", "notifications"]), Some(AppId::INBOX_NOTIFICATIONS));
        for exception in [
            &["--demo"][..],
            &["--view", "mail", "--demo"],
            &["--view", "mail", "--message=m:0a1b"],
            &["--view", "mail", "--compose"],
            &["--lane", "info"],
            &["--view", "notifications", "--source", "script:backup"],
            &["--view", "mail", "--settings", "reading"],
            &["--settings", "rules"],
        ] {
            assert_eq!(of(exception), None, "{exception:?}");
        }
    }

    /// Issue #279: the programs of Mail and App Feed add their `--view` in front when none is given;
    /// one given stays as it is, and `torroinbox` adds nothing.
    #[test]
    fn the_own_programs_add_their_view_unless_one_is_given() {
        let mail = AppId::INBOX_MAIL;
        assert_eq!(launch_args(mail, args(&[])), ["--view", "mail"]);
        assert_eq!(launch_args(mail, args(&["--message=m:0a1b"])), ["--view", "mail", "--message=m:0a1b"]);
        assert_eq!(launch_args(mail, args(&["--view", "hub"])), ["--view", "hub"], "a given view wins");
        assert_eq!(launch_args(mail, args(&["--demo", "--view=hub"])), ["--demo", "--view=hub"]);
        let feed = AppId::INBOX_NOTIFICATIONS;
        assert_eq!(launch_args(feed, args(&["--lane", "info"])), ["--view", "notifications", "--lane", "info"]);
        assert_eq!(launch_args(feed, args(&["--view", "mail"])), ["--view", "mail"]);
        assert_eq!(launch_args(AppId::INBOX, args(&[])), Vec::<String>::new(), "the hub");
        assert_eq!(launch_args(AppId::INBOX, args(&["--view", "mail"])), ["--view", "mail"]);
        // Not a `--view`: another flag that only starts alike.
        assert_eq!(launch_args(mail, args(&["--views"])), ["--view", "mail", "--views"]);
    }

    /// Issue #279: a plain start of each program opens its own window kind ([`run`] builds the window
    /// of `launch.kind.app()`, on Linux with that entry's window class as app id) and claims the
    /// single-instance channel of that class.
    #[test]
    fn each_program_opens_the_window_of_its_entry() {
        for app in [AppId::INBOX, AppId::INBOX_MAIL, AppId::INBOX_NOTIFICATIONS] {
            let launch = Launch::parse(&launch_args(app, Vec::new())).expect("valid parameters");
            assert_eq!(launch.kind.app(), app);
            #[cfg(target_os = "linux")]
            assert_eq!(torro_gui::window(launch.kind.app()).platform_specific.application_id, app.window_class);
            assert_eq!(instance_of(&launch), Some(app));
        }
    }
}
