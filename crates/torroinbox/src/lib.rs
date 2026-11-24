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
//! `--demo --shots <dir>` writes the view at each size class as PNG files instead of opening a window
//! (the visual check, [`torro_gui::shots`], issue #492).
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
//!
//! On Windows the three programs are GUI programs without a console window (`windows_subsystem` in
//! each `main.rs`, never here: it only counts in a program's own crate root). A failed start is
//! written to [`START_ERROR_FILE`] in the inbox's state directory instead (see [`report_start_error`]).

mod app;
mod ask;
mod compose;
mod contact;
mod detail;
#[cfg(test)]
mod empty;
mod glance;
mod hub;
mod insights;
mod keys;
mod list;
mod mail;
mod mail_overview;
mod mail_statistics;
mod news;
mod notifications;
mod overview;
mod search;
mod suggest;
mod talk;
mod workflows;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use iced::{Element, Task};
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_core::timing;
use torro_gui::instance::Claim;
use torro_gui::shots::{RunError, Shots};
use torro_gui::source::Source;
use torro_inbox::InboxSource;
use torro_inbox::demo::DemoSource;
use torro_ipc::inbox::InboxClient;

use crate::app::{App, Launch, Message};
use crate::compose::ComposeSource;
use crate::hub::HubSource;
use crate::insights::StatsSource;
use crate::mail::MailSource;
use crate::news::NewsSource;
use crate::suggest::SuggestSource;

/// Holds the error of the last failed start, in the inbox's state directory (Windows only). One file for
/// the three programs, which share that directory; each line names the program that failed.
const START_ERROR_FILE: &str = "start-error.log";

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
/// Messages name the program by `app.program_file()`; on Windows a failed start is also written to
/// [`START_ERROR_FILE`] ([`report_start_error`]).
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
            report_start_error(app, &err.to_string());
            return ExitCode::from(64);
        }
    };
    // The mail to show (tasks E8.7a, E8.7b): read and checked now, so the core's command line never
    // fails; the window opens on it once its first entries are in.
    launch.message = match start_message(&args) {
        Ok(message) => message,
        Err(err) => {
            report_start_error(app, &err);
            return ExitCode::from(64);
        }
    };
    // The compose window is a webview (issue #969): it has no pictures without a window. Take them from the window.
    if launch.compose.is_some() && args.iter().any(|arg| arg == "--shots" || arg.starts_with("--shots=")) {
        report_start_error(app, "the compose window is a web view and has no --shots; take a screenshot of its window");
        return ExitCode::from(64);
    }
    // Linux with NVIDIA: WebKitGTK needs a variable set before it starts, so the window starts again with it.
    if launch.compose.is_some()
        && let Some(code) = compose::web::relaunch()
    {
        return code;
    }
    // `--demo --shots <dir>` (issue #492): the views as pictures, without a window.
    let name = if launch.compose.is_some() { format!("{}-compose", app.binary) } else { app.binary.to_string() };
    let shots = match Shots::from_args(&name, &args) {
        Ok(shots) => shots,
        Err(err) => {
            report_start_error(app, &err.to_string());
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
        (true, true) => {
            compose(Arc::new(DemoSource::new(launch.start())), suite, launch).map_err(|err| err.to_string())
        }
        (true, false) => compose(Arc::new(InboxClient::default()), suite, launch).map_err(|err| err.to_string()),
        // `--open pinned-one` has a demo of one mail account (issue #476).
        (false, true) => {
            run(Arc::new(launch.demo_source()), suite, launch, shots.as_ref()).map_err(|err| err.to_string())
        }
        (false, false) => {
            run(Arc::new(InboxClient::default()), suite, launch, shots.as_ref()).map_err(|err| err.to_string())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            report_start_error(app, &err.to_string());
            ExitCode::FAILURE
        }
    }
}

/// Reports why the program of `app` could not start: on stderr, and on Windows also in [`START_ERROR_FILE`] of the
/// inbox's state directory, because a GUI program started by a double click has no console that would show stderr.
fn report_start_error(app: AppId, message: &str) {
    let line = format!("{}: {message}", app.program_file());
    eprintln!("{line}");
    if cfg!(windows)
        && let Some(paths) = AppPaths::for_app(app)
    {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        // Nowhere is left to report a failed write to; the stderr line above is all we can do.
        let _ = write_start_error(&paths.state, &format!("{stamp} {line}"));
    }
}

/// Writes `line` to [`START_ERROR_FILE`] in `dir`, creating `dir` if needed, and returns the file.
///
/// Replaces an earlier report, so the file stays small and always shows the latest failed start.
fn write_start_error(dir: &Path, line: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join(START_ERROR_FILE);
    std::fs::write(&file, format!("{line}\n"))?;
    Ok(file)
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
/// kind (`--view`, task E8.1), so tiling rules and templates tell the three windows apart. With `shots`
/// its pictures instead ([`torro_gui::shots::run`]).
fn run<S: InboxSource + MailSource + StatsSource + NewsSource + HubSource + SuggestSource + 'static>(
    inbox: Arc<S>,
    suite: Source,
    launch: Launch,
    shots: Option<&Shots>,
) -> Result<(), RunError> {
    let window = torro_gui::window(launch.kind.app());
    let application = iced::application(move || App::new(inbox.clone(), suite.clone(), launch.clone()), update, view)
        .settings(torro_gui::settings())
        .title(App::title)
        .theme(App::theme)
        .scale_factor(App::scale_factor)
        .subscription(App::subscription)
        .window(window);
    torro_gui::shots::run(application, shots)
}

/// [`App::update`], timed: the first entries in mark "first data", a slow update is written.
fn update<S: InboxSource + MailSource + StatsSource + NewsSource + HubSource + SuggestSource + 'static>(
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
fn view<S: InboxSource + MailSource + StatsSource + NewsSource + HubSource + SuggestSource + 'static>(
    app: &App<S>,
) -> Element<'_, Message> {
    let started = Instant::now();
    let element = app.view();
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

/// The compose window on `source`: the core's client or the demo source (issue #969, [`compose::web`]): a window with
/// the system's webview and the editor Squire, not an Iced window.
fn compose<S: ComposeSource + 'static>(
    source: Arc<S>,
    suite: Source,
    launch: Launch,
) -> Result<(), compose::web::WebError> {
    let start = launch.compose.clone().unwrap_or(compose::Start {
        kind: compose::Kind::New,
        account: None,
        open: None,
        attach: None,
    });
    compose::web::run(source, suite, start)
}

/// A test's `iced_test::Simulator` of `element` at `size`, with the lock that lets only one exist at a time
/// (issue #697): each builds a wgpu instance, and the Vulkan loader crashes the test process (SIGSEGV in
/// `vkEnumerateInstanceExtensionProperties`) when several threads do that at once, as torro-gui found before
/// (`shell::tests::one_simulator`). Bind both, `let (_one, mut ui) = …`, so the lock is let go after the
/// simulator; a test holds one simulator at a time.
#[cfg(test)]
pub(crate) fn simulator<'a, Message>(
    size: impl Into<iced::Size>,
    element: impl Into<Element<'a, Message>>,
) -> (std::sync::MutexGuard<'static, ()>, iced_test::Simulator<'a, Message>) {
    let one = one_renderer();
    (one, iced_test::Simulator::with_size(iced::Settings::default(), size, element))
}

/// The lock of [`simulator`], for a test that builds a headless renderer of its own: held while the
/// renderer lives, so no two threads set up wgpu at once.
#[cfg(test)]
pub(crate) fn one_renderer() -> std::sync::MutexGuard<'static, ()> {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test that failed while holding it leaves it poisoned; the next one may still run.
    ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &[&str]) -> Vec<String> {
        line.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn the_start_error_replaces_the_last_one_in_a_new_directory() {
        let root = std::env::temp_dir().join(format!("torroinbox-start-error-{}", std::process::id()));
        let dir = root.join("state");

        write_start_error(&dir, "first").expect("first write");
        let file = write_start_error(&dir, "second").expect("second write");

        assert_eq!(file, dir.join(START_ERROR_FILE));
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "second\n");
        std::fs::remove_dir_all(&root).expect("clean up");
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
