//! The window of an app job (issue #411, sub-issue #480): `torroworkplace ui --app-job=<target>`, a
//! small borderless window [`size::panel()`] wide, like a start screen. It shows installing,
//! updating or uninstalling apps step by step – Download · Check signature · Install · Connect to the
//! service, each with its mark and a detail at the right –, a progress bar and "Cancel" while the
//! download runs; done, "Installed 0.2.0" with "Close" and "Open <app>".
//!
//! Three targets ([`Target`]):
//!
//! - `<id>`: follow the job `id`, which the control centre started (`apps.install_app`,
//!   `apps.update_apps`) and whose answer gave the id.
//! - `uninstall:<app>`: ask first – "Uninstall Torro App Feed?" with "Also remove its data on this
//!   computer" (accounts stay) –, then `apps.uninstall_app` and follow its job.
//! - `update:torroworkplace`: the update of the main application – "Torro Workplace 0.2.0 is
//!   available", what restarts, "What's new", "Later" and "Update and restart" –, then
//!   `apps.update_apps` with `torroworkplace` and follow its job.
//!
//! It asks `apps.app_job` every 500 ms while the job runs. A job that ends with `restart` (the main
//! application was updated) restarts the service: the window waits until `system.ping` answers
//! with the new version, then has the core open the control centre (`apps.open_app torroworkplace`)
//! and ends. The old control centre ends itself when it sees the service change (`Ui::restarting`).
//!
//! The window reads and writes nothing itself; every step is a request (skill `torro-api`). Closing
//! it never stops a job: "Show" on the app's card opens it again. With `--demo` it shows the demo's
//! job or question and sends nothing.

use std::time::{Duration, Instant};

use iced::keyboard::{self, Key, key::Named};
use iced::theme::Mode as SystemMode;
use iced::widget::text::Wrapping;
use iced::widget::{Space, checkbox, column, container, mouse_area, row, space as blank, svg};
use iced::{Alignment, Element, Fill, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::theme::Appearance;
use torro_gui::icon::{self, Icon};
use torro_gui::shell;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, TileFace, TileSize};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{style, typo};
use torro_ipc::{
    AppCatalog, AppJobInfo, AppJobKind, AppJobState, AppJobStep, AppStep, Request, Response, StepStatus, client,
};

use super::apps::{self, MAIN_APP};
use super::launch::{Launch, Open};

/// How often the window asks how the job stands, and whether the restarted service answers.
pub const POLL: Duration = Duration::from_millis(500);

/// How long the window waits for the service to come back after a restart before it says so.
pub const RESTART_WAIT: Duration = Duration::from_secs(90);

/// How long the window waits once the new service answered before it opens the control centre: the
/// old control centre sees the change on its next reload (every second while a job runs) and ends,
/// so the core starts the new one instead of bringing the old one to the front.
pub const HANDOVER: Duration = Duration::from_secs(2);

/// What `--app-job=` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `<id>`: follow this job.
    Follow(String),
    /// `uninstall:<app>`: ask, then uninstall the app (window class).
    Uninstall(String),
    /// `update:torroworkplace`: offer the update of the main application, then run it.
    SelfUpdate,
}

impl Target {
    /// The target `value` names; `None` for an empty one or an update of anything but the main
    /// application (apps are updated from their cards without asking).
    pub fn parse(value: &str) -> Option<Target> {
        let value = value.trim();
        if let Some(app) = value.strip_prefix("uninstall:") {
            return (!app.is_empty()).then(|| Target::Uninstall(app.to_string()));
        }
        if let Some(app) = value.strip_prefix("update:") {
            return (app == MAIN_APP).then_some(Target::SelfUpdate);
        }
        (!value.is_empty()).then(|| Target::Follow(value.to_string()))
    }

    /// The value of `--app-job=` for this target.
    pub fn arg(&self) -> String {
        match self {
            Target::Follow(id) => id.clone(),
            Target::Uninstall(app) => format!("uninstall:{app}"),
            Target::SelfUpdate => format!("update:{MAIN_APP}"),
        }
    }
}

/// Starts the window of `target` as a process of its own (`torroworkplace ui --app-job=<target>`),
/// from the control centre. A local start of this program, like the dialog of a template; the window
/// itself sends the requests. Returns what went wrong, for the notice.
pub fn open(target: &Target) -> Result<(), String> {
    let program = std::env::current_exe().map_err(|err| err.to_string())?;
    let mut child = std::process::Command::new(program)
        .arg("ui")
        .arg(format!("--app-job={}", target.arg()))
        .spawn()
        .map_err(|err| err.to_string())?;
    // Waited for on a thread of its own, so it leaves no zombie behind.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Where the window stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    /// The catalog is on its way (for names and versions), or the first answer about the job.
    Loading,
    /// "Uninstall …?" with the check box for the app's data.
    Ask { remove_data: bool },
    /// "Torro Workplace 0.2.0 is available".
    Offer,
    /// The request that starts the job is on its way.
    Starting,
    /// The job as the core answered last.
    Job(AppJobInfo),
    /// The main application was updated: waiting for the service with the new version.
    Restarting {
        /// The version the new service answers with, from the catalog; any other than this
        /// program's own when the catalog had none.
        expected: Option<String>,
        since: Instant,
        /// When the new service first answered.
        came: Option<Instant>,
    },
    /// The job could not start, failed or is no longer known; in words for people.
    Failed(String),
}

/// What the window does next; [`Window::update`] returns it, `run` turns it into a task.
#[derive(Debug, Clone, PartialEq)]
#[expect(clippy::large_enum_variant, reason = "one step at a time, handed on at once")]
pub enum Step {
    None,
    /// Send `request`; its answer comes back as `reply`.
    Send(Request, Reply),
    OpenUrl(String),
    Exit,
}

/// Which answer a request of the window waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Started,
    Polled,
    Pinged,
    Opened,
}

#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant, reason = "one event at a time, as the shell's `Message::Sent`")]
pub enum Event {
    /// The catalog and the settings at the start.
    Loaded(Box<Option<AppCatalog>>, Option<Appearance>),
    Answer(Reply, Result<Response, String>),
    /// Every [`POLL`] while the job runs or the service restarts.
    Tick,
    RemoveData(bool),
    /// "Uninstall app" or "Update and restart".
    Confirm,
    /// "Cancel" while the download runs.
    Cancel,
    /// "Close", "Later", "Cancel" of a question, Escape, or the window manager.
    Close,
    /// "Open <app>" once it is installed.
    OpenApp,
    /// "What's new".
    WhatsNew,
    /// "Try again" after a failed install or update.
    Retry,
    SystemMode(SystemMode),
    Zoom(zoom::Message),
    DragWindow,
}

pub struct Window {
    target: Target,
    demo: bool,
    forced: Option<Appearance>,
    appearance: Appearance,
    system_mode: SystemMode,
    zoom: Zoom,
    pub stage: Stage,
    catalog: Option<AppCatalog>,
    /// The job's id once known.
    job: Option<String>,
    /// The last job the core reported, also after the core restarted.
    last: Option<AppJobInfo>,
    /// A poll or ping is on its way: the next tick waits for it.
    asking: bool,
}

/// Runs the window of `launch.app_job` (or of the demo state `launch.open`).
pub fn run(launch: Launch) -> iced::Result {
    let socket = torro_ipc::socket_path().filter(|_| !launch.demo);
    if let Some(socket) = &socket {
        torro_gui::source::ensure_core(socket);
    }
    torro_gui::design::start(socket.as_deref(), &launch.forced());
    iced::application(move || Window::start(&launch), Window::perform_update, Window::view)
        .settings(torro_gui::settings())
        .title(Window::title)
        .theme(Window::theme)
        .scale_factor(|window: &Window| window.zoom.factor())
        .subscription(Window::subscription)
        .window(window_settings())
        .run()
}

/// The window: [`size::panel()`] square, borderless, in the middle of the screen, not resizable; on
/// Linux with the dialog's class, which Hyprland floats.
pub fn window_settings() -> iced::window::Settings {
    let mut settings = torro_gui::window(AppId::WORKPLACE);
    settings.size = Size::new(size::panel(), size::panel());
    settings.decorations = false;
    settings.resizable = false;
    settings.position = iced::window::Position::Centered;
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = torro_core::app::WORKPLACE_DIALOG_CLASS.to_string();
    }
    settings
}

/// The target of a demo state of the window (`--demo --open apps-install` …).
pub fn demo_target(open: Option<Open>) -> Option<Target> {
    match open? {
        Open::AppsInstall | Open::AppsInstalled => Some(Target::Follow("demo-install".into())),
        Open::AppsUninstall => Some(Target::Uninstall(AppId::INBOX_NOTIFICATIONS.window_class.into())),
        Open::AppsSelfUpdate => Some(Target::SelfUpdate),
        _ => None,
    }
}

impl Window {
    /// The window for `target`, before anything was loaded.
    pub fn new(target: Target, demo: bool, forced: Option<Appearance>) -> Window {
        Window {
            target,
            demo,
            forced,
            appearance: Appearance::System,
            system_mode: SystemMode::None,
            zoom: Zoom::new(torro_ipc::socket_path().filter(|_| !demo)),
            stage: Stage::Loading,
            catalog: None,
            job: None,
            last: None,
            asking: false,
        }
    }

    fn start(launch: &Launch) -> (Window, Task<Event>) {
        let target = launch
            .app_job
            .as_deref()
            .and_then(Target::parse)
            .or_else(|| demo_target(launch.open))
            .unwrap_or(Target::Follow(String::new()));
        let mut window = Window::new(target, launch.demo, launch.appearance);
        let first = if launch.demo {
            let mut catalog = apps::demo_catalog(chrono::Utc::now());
            // The demo's main application is up to date on the page; its window offers the update.
            catalog.main.installed = Some("0.1.0".into());
            catalog.main.state = torro_ipc::CatalogState::Update;
            let step = window.update(Event::Loaded(Box::new(Some(catalog)), None));
            let done = launch.open == Some(Open::AppsInstalled);
            if matches!(window.target, Target::Follow(_)) {
                let _ = window.update(Event::Answer(Reply::Polled, Ok(Response::AppJob { job: apps::demo_job(done) })));
            }
            window.perform(step)
        } else {
            load()
        };
        let tasks =
            Task::batch([first, window.zoom.fetch().map(Event::Zoom), iced::system::theme().map(Event::SystemMode)]);
        (window, tasks)
    }

    fn title(&self) -> String {
        match &self.target {
            Target::Uninstall(_) => tr("Uninstall app").to_string(),
            Target::SelfUpdate => tr("Update Torro Workplace").to_string(),
            Target::Follow(_) => tr("Apps").to_string(),
        }
    }

    fn theme(&self) -> Theme {
        theme::for_appearance(self.forced.unwrap_or(self.appearance), self.system_mode)
    }

    fn subscription(&self) -> Subscription<Event> {
        let mut all = vec![
            iced::system::theme_changes().map(Event::SystemMode),
            iced::event::listen_with(keys),
            iced::window::close_requests().map(|_| Event::Close),
            self.zoom.subscription().map(Event::Zoom),
        ];
        if self.ticks() && !self.demo {
            all.push(iced::time::every(POLL).map(|_| Event::Tick));
        }
        Subscription::batch(all)
    }

    /// Whether the window asks again and again: while the job runs and while the service restarts.
    pub fn ticks(&self) -> bool {
        match &self.stage {
            Stage::Job(job) => job.state == AppJobState::Running,
            Stage::Restarting { .. } => true,
            Stage::Loading => matches!(self.target, Target::Follow(_)),
            _ => false,
        }
    }

    fn perform_update(&mut self, event: Event) -> Task<Event> {
        let task = match event {
            Event::SystemMode(mode) => {
                self.system_mode = mode;
                return Task::none();
            }
            Event::Zoom(message) => return self.zoom.update(message).map(Event::Zoom),
            Event::DragWindow => return torro_gui::drag_window(),
            event => self.update(event),
        };
        self.perform(task)
    }

    /// What `step` does: the demo sends nothing and opens nothing, it only ends.
    fn perform(&mut self, step: Step) -> Task<Event> {
        match step {
            Step::None => Task::none(),
            Step::Exit => iced::exit(),
            Step::Send(..) | Step::OpenUrl(_) if self.demo => Task::none(),
            Step::Send(request, reply) => {
                self.asking = matches!(reply, Reply::Polled | Reply::Pinged);
                Task::perform(call(request), move |answer| Event::Answer(reply, answer))
            }
            Step::OpenUrl(url) => {
                if let Err(err) = webbrowser::open(&url) {
                    eprintln!("torroworkplace: cannot open {url}: {err}");
                }
                Task::none()
            }
        }
    }

    /// The window's logic, without Iced: what happens with `event`, and what to do next.
    pub fn update(&mut self, event: Event) -> Step {
        match event {
            Event::Loaded(catalog, appearance) => {
                self.catalog = *catalog;
                if let Some(appearance) = appearance {
                    self.appearance = appearance;
                }
                if self.stage == Stage::Loading {
                    match &self.target {
                        Target::Uninstall(_) => self.stage = Stage::Ask { remove_data: false },
                        Target::SelfUpdate => self.stage = Stage::Offer,
                        Target::Follow(id) if id.is_empty() => {
                            self.stage = Stage::Failed(tr("This job is no longer known. It may have finished.").into())
                        }
                        Target::Follow(id) => {
                            self.job = Some(id.clone());
                            return self.poll();
                        }
                    }
                }
                Step::None
            }
            Event::RemoveData(on) => {
                if let Stage::Ask { remove_data } = &mut self.stage {
                    *remove_data = on;
                }
                Step::None
            }
            Event::Confirm => match (&self.stage, &self.target) {
                (Stage::Ask { remove_data }, Target::Uninstall(app)) => {
                    let request = Request::UninstallApp { app: app.clone(), remove_data: *remove_data };
                    self.stage = Stage::Starting;
                    Step::Send(request, Reply::Started)
                }
                (Stage::Offer, Target::SelfUpdate) => {
                    self.stage = Stage::Starting;
                    Step::Send(Request::UpdateApps { apps: vec![MAIN_APP.into()] }, Reply::Started)
                }
                _ => Step::None,
            },
            Event::Cancel => match (&self.stage, &self.job) {
                (Stage::Job(job), Some(id)) if cancellable(job) => {
                    Step::Send(Request::CancelAppJob { id: id.clone() }, Reply::Polled)
                }
                _ => Step::None,
            },
            // Closing never stops a job; while the service restarts the window stays, so it can open
            // the new control centre.
            Event::Close => match self.stage {
                Stage::Restarting { .. } => Step::None,
                _ => Step::Exit,
            },
            Event::OpenApp => match self.last.as_ref().and_then(|job| job.apps.first()) {
                Some(app) => Step::Send(Request::OpenApp { app: app.clone(), target: None }, Reply::Opened),
                None => Step::Exit,
            },
            Event::WhatsNew => match self.catalog.as_ref().and_then(|catalog| catalog.release_url.clone()) {
                Some(url) => Step::OpenUrl(url),
                None => Step::None,
            },
            Event::Retry => match self.last.as_ref().and_then(retry_request) {
                Some(request) => {
                    self.stage = Stage::Starting;
                    Step::Send(request, Reply::Started)
                }
                None => Step::None,
            },
            Event::Tick if self.asking => Step::None,
            Event::Tick => match &self.stage {
                Stage::Restarting { since, came: Some(came), .. }
                    if came.elapsed() >= HANDOVER || since.elapsed() >= RESTART_WAIT =>
                {
                    let open = Request::OpenApp { app: MAIN_APP.into(), target: None };
                    Step::Send(open, Reply::Opened)
                }
                Stage::Restarting { since, came: None, .. } if since.elapsed() >= RESTART_WAIT => {
                    self.stage =
                        Stage::Failed(tr("Torro Workplace didn't start again. Open it from your apps.").into());
                    Step::None
                }
                Stage::Restarting { .. } => Step::Send(Request::Ping, Reply::Pinged),
                _ => self.poll(),
            },
            Event::Answer(reply, answer) => {
                self.asking = false;
                self.answered(reply, answer)
            }
            Event::SystemMode(_) | Event::Zoom(_) | Event::DragWindow => Step::None,
        }
    }

    /// `apps.app_job` for the job, once its id is known.
    fn poll(&mut self) -> Step {
        match &self.job {
            Some(id) => Step::Send(Request::AppJob { id: id.clone() }, Reply::Polled),
            None => Step::None,
        }
    }

    fn answered(&mut self, reply: Reply, answer: Result<Response, String>) -> Step {
        match (reply, answer) {
            (Reply::Opened, _) => Step::Exit,
            (Reply::Started | Reply::Polled, Ok(Response::AppJob { job })) => {
                self.job = Some(job.id.clone());
                self.followed(job)
            }
            (Reply::Started, Ok(Response::Error { code, message })) => {
                eprintln!("torroworkplace: the app job was refused ({code}): {message}");
                self.stage = Stage::Failed(apps::refusal(&code).into());
                Step::None
            }
            (Reply::Started, other) => {
                eprintln!("torroworkplace: no job came back: {other:?}");
                self.stage = Stage::Failed(tr("Something went wrong. Try again.").into());
                Step::None
            }
            // The core no longer knows the job, or does not answer: after the main application's update
            // that is the restart; else the job is gone.
            (Reply::Polled, answer) => {
                let gone = matches!(&answer, Ok(Response::Error { code, .. }) if code == "not-found");
                if self.last.as_ref().is_some_and(|job| job.restart) {
                    self.restarting();
                } else if gone {
                    self.stage = Stage::Failed(tr("This job is no longer known. It may have finished.").into());
                }
                Step::None
            }
            (Reply::Pinged, Ok(Response::Pong { version, .. })) => {
                if let Stage::Restarting { expected, came, .. } = &mut self.stage
                    && came.is_none()
                    && is_new(&version, expected.as_deref())
                {
                    *came = Some(Instant::now());
                }
                Step::None
            }
            (Reply::Pinged, _) => Step::None,
        }
    }

    /// The core reported `job`: the window follows it; done, it ends for an uninstall or a cancel,
    /// waits for the restart after the main application's update, or shows "Installed".
    fn followed(&mut self, job: AppJobInfo) -> Step {
        self.last = Some(job.clone());
        match job.state {
            AppJobState::Cancelled => Step::Exit,
            AppJobState::Done if job.restart => {
                self.restarting();
                Step::None
            }
            AppJobState::Done if job.kind == AppJobKind::Uninstall => Step::Exit,
            _ => {
                self.stage = Stage::Job(job);
                Step::None
            }
        }
    }

    /// From now on wait for the service with the new version.
    fn restarting(&mut self) {
        if matches!(self.stage, Stage::Restarting { .. }) {
            return;
        }
        let expected =
            self.catalog.as_ref().and_then(|catalog| catalog.main.available.clone().or(catalog.latest.clone()));
        self.stage = Stage::Restarting { expected, since: Instant::now(), came: None };
    }

    /// The app the window is about: the job's first, the target's, or the main application.
    fn app(&self) -> AppId {
        let class = match (&self.target, &self.last) {
            (Target::Uninstall(app), _) => Some(app.as_str()),
            (_, Some(job)) => job.apps.first().map(String::as_str),
            (Target::SelfUpdate, None) => None,
            (Target::Follow(_), None) => None,
        };
        class.and_then(AppId::find).unwrap_or(AppId::WORKPLACE)
    }

    /// The version the job brings: from the catalog's entry of the app (or the main application's).
    fn version(&self) -> Option<String> {
        let catalog = self.catalog.as_ref()?;
        let app = self.app();
        if app == AppId::WORKPLACE {
            return catalog.main.available.clone().or(catalog.latest.clone());
        }
        let entry = catalog.apps.iter().find(|entry| entry.window_class == app.window_class)?;
        entry.available.clone().or(catalog.latest.clone())
    }

    fn view(&self) -> Element<'_, Event> {
        let body = match &self.stage {
            Stage::Loading | Stage::Starting => {
                self.frame(self.hero(self.status_line()), None, vec![], widgets::loading())
            }
            Stage::Ask { remove_data } => self.ask(*remove_data),
            Stage::Offer => self.offer(),
            Stage::Job(job) => self.job_view(job),
            Stage::Restarting { came, .. } => self.restart_view(came.is_some()),
            Stage::Failed(why) => self.failed(why),
        };
        let content = container(body).padding(space::xl()).width(Fill).height(Fill).style(style::container::menu);
        self.zoom.view(content)
    }

    /// The name of the window's app, as its card shows it.
    fn name(&self) -> String {
        let app = self.app();
        if app == AppId::WORKPLACE { "Torro Workplace".into() } else { format!("Torro {}", shell::app_name(app)) }
    }

    /// The big icon and the name, and the line under it; the window is dragged by it.
    fn hero<'a>(&self, line: String) -> Element<'a, Event> {
        let tile = widgets::source_tile(TileFace::Icon(shell::app_icon(self.app())), TileSize::Large, None);
        let words = column![
            typo::text(typo::title(), self.headline()).wrapping(Wrapping::WordOrGlyph),
            typo::text(typo::body(), line).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(space::xs())
        .width(Fill);
        let hero = row![tile, words].spacing(space::l()).align_y(Alignment::Start);
        mouse_area(hero).on_press(Event::DragWindow).into()
    }

    /// The window's title line: the app's name, the question, or the offer of the main application.
    fn headline(&self) -> String {
        match (&self.stage, &self.target) {
            (Stage::Ask { .. }, _) => trf("Uninstall {app}?", &[("app", &self.name())]),
            (Stage::Offer, _) => match self.version() {
                Some(version) => trf("Torro Workplace {version} is available", &[("version", &version)]),
                None => tr("An update of Torro Workplace is available").to_string(),
            },
            _ => self.name(),
        }
    }

    /// The line under the name: "Installing 0.2.0", "Installed 0.2.0" and so on.
    fn status_line(&self) -> String {
        let version = self.version().unwrap_or_default();
        let kind = self.last.as_ref().map(|job| job.kind).unwrap_or(match self.target {
            Target::Uninstall(_) => AppJobKind::Uninstall,
            Target::SelfUpdate => AppJobKind::Update,
            Target::Follow(_) => AppJobKind::Install,
        });
        let done = self.last.as_ref().is_some_and(|job| job.state == AppJobState::Done);
        let with = |text: &'static str| trf(text, &[("version", &version)]);
        match (kind, done) {
            (AppJobKind::Install, false) => with("Installing {version}"),
            (AppJobKind::Install, true) => with("Installed {version}"),
            (AppJobKind::Update, false) => with("Updating to {version}"),
            (AppJobKind::Update, true) => with("Updated to {version}"),
            (AppJobKind::Uninstall, false) => tr("Uninstalling…").to_string(),
            (AppJobKind::Uninstall, true) => tr("Uninstalled").to_string(),
        }
    }

    /// The layout of every stage: the hero at the top, then `middle`, `note` and the `buttons` at the
    /// bottom.
    fn frame<'a>(
        &self,
        hero: Element<'a, Event>,
        note: Option<String>,
        buttons: Vec<Element<'a, Event>>,
        middle: Element<'a, Event>,
    ) -> Element<'a, Event> {
        let note: Element<'a, Event> = match note {
            Some(note) => {
                typo::text(typo::meta(), note).style(theme::text_3).wrapping(Wrapping::WordOrGlyph).width(Fill).into()
            }
            None => blank::horizontal().into(),
        };
        let bottom = row![note].extend(buttons).spacing(space::s()).align_y(Alignment::Center);
        column![hero, container(middle).height(Fill).width(Fill), bottom].spacing(space::l()).into()
    }

    /// "Uninstall Torro App Feed?": the check box for its data, "Cancel" and "Uninstall app".
    fn ask(&self, remove_data: bool) -> Element<'_, Event> {
        let check = checkbox(remove_data)
            .size(style::CHECK_SIZE)
            .style(|t: &Theme, status| style::checkbox(palette(t).text_2)(t, status))
            .on_toggle(Event::RemoveData);
        let words = column![
            typo::text(typo::body_strong(), tr("Also remove its data on this computer")),
            typo::text(
                typo::meta(),
                tr("Its settings and what it kept offline. Your accounts stay: other apps of the suite use them.")
            )
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph),
        ]
        .spacing(space::xs())
        .width(Fill);
        let option = mouse_area(row![check, words].spacing(space::m()).align_y(Alignment::Start))
            .on_press(Event::RemoveData(!remove_data));
        let buttons = vec![
            widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Event::Close)),
            widgets::text_button(None, tr("Uninstall app"), style::button::danger, Some(Event::Confirm)),
        ];
        let line = tr("It moves to the trash and leaves the app switcher.").to_string();
        self.frame(self.hero(line), None, buttons, option.into())
    }

    /// "Torro Workplace 0.2.0 is available": what restarts, "What's new", "Later", "Update and restart".
    fn offer(&self) -> Element<'_, Event> {
        let line = tr("The service and this window restart; your apps stay open and reconnect.").to_string();
        let whats_new = self.catalog.as_ref().and_then(|catalog| catalog.release_url.as_ref()).map(|_| Event::WhatsNew);
        let buttons = vec![
            widgets::text_button(Some(Icon::ArrowUpRight), tr("What's new"), style::button::ghost, whats_new),
            blank::horizontal().into(),
            widgets::text_button(None, tr("Later"), style::button::secondary, Some(Event::Close)),
            widgets::text_button(None, tr("Update and restart"), style::button::primary, Some(Event::Confirm)),
        ];
        self.frame(self.hero(line), None, buttons, Space::new().into())
    }

    /// A running or finished job: its steps, the progress, and "Cancel" or "Close" and "Open <app>".
    fn job_view<'a>(&'a self, job: &'a AppJobInfo) -> Element<'a, Event> {
        let failed = job.state == AppJobState::Failed;
        let line = match (&job.error, failed) {
            (Some(code), true) => failure(code).to_string(),
            (None, true) => tr("Something went wrong. Try again.").to_string(),
            _ => self.status_line(),
        };
        let progress = column![
            row![
                typo::text(typo::meta(), doing(job)).style(theme::text_2).width(Fill),
                typo::text(typo::meta().tabular(), trf("{n}%", &[("n", &job.percent)])).style(theme::text_2),
            ],
            widgets::progress_line(u64::from(job.percent), 100),
        ]
        .spacing(space::s());
        let middle = column![steps(job), progress].spacing(space::l());
        let (note, buttons) = match job.state {
            AppJobState::Running => (
                Some(tr("You can keep working; this window closes when it is done.").to_string()),
                vec![widgets::text_button(
                    None,
                    tr("Cancel"),
                    style::button::secondary,
                    cancellable(job).then_some(Event::Cancel),
                )],
            ),
            AppJobState::Done => {
                let open = trf("Open {app}", &[("app", &self.name())]);
                (
                    None,
                    vec![
                        widgets::text_button(None, tr("Close"), style::button::secondary, Some(Event::Close)),
                        widgets::text_button(None, open, style::button::primary, Some(Event::OpenApp)),
                    ],
                )
            }
            AppJobState::Failed | AppJobState::Cancelled => {
                let mut buttons =
                    vec![widgets::text_button(None, tr("Close"), style::button::secondary, Some(Event::Close))];
                if retry_request(job).is_some() {
                    buttons.push(widgets::text_button(
                        None,
                        tr("Try again"),
                        style::button::primary,
                        Some(Event::Retry),
                    ));
                }
                (None, buttons)
            }
        };
        self.frame(self.hero(line), note, buttons, middle.into())
    }

    /// The main application restarts: its steps done, and that the control centre opens again.
    fn restart_view(&self, came: bool) -> Element<'_, Event> {
        let line = if came { tr("Opening Torro Workplace…") } else { tr("Restarting…") };
        let middle: Element<'_, Event> = match &self.last {
            Some(job) => steps(job),
            None => Space::new().into(),
        };
        let note = Some(tr("Your apps stay open and reconnect by themselves.").to_string());
        self.frame(self.hero(line.to_string()), note, vec![], middle)
    }

    /// The job could not start or is lost: why, "Close", and "Try again" where it can.
    fn failed<'a>(&'a self, why: &'a str) -> Element<'a, Event> {
        let mut buttons = vec![widgets::text_button(None, tr("Close"), style::button::secondary, Some(Event::Close))];
        if self.last.as_ref().and_then(retry_request).is_some() {
            buttons.push(widgets::text_button(None, tr("Try again"), style::button::primary, Some(Event::Retry)));
        }
        let mark = icon::icon(Icon::CircleAlert, icon::SIZE)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
        let middle =
            row![mark, typo::text(typo::body(), why).style(theme::text_2).wrapping(Wrapping::WordOrGlyph).width(Fill)]
                .spacing(space::s())
                .align_y(Alignment::Start);
        self.frame(self.hero(self.name()), None, buttons, middle.into())
    }
}

/// The steps of `job`, one row each: its mark, its name and its detail at the right. A job with
/// several apps ("Update all") shows a row per app instead, with the step it is at.
fn steps<'a>(job: &'a AppJobInfo) -> Element<'a, Event> {
    let rows: Vec<Element<'a, Event>> = if job.apps.len() > 1 {
        job.apps
            .iter()
            .map(|app| {
                let of: Vec<&AppJobStep> = job.steps.iter().filter(|step| &step.app == app).collect();
                let status = app_status(&of);
                let at = of
                    .iter()
                    .find(|step| step.status != StepStatus::Done)
                    .or(of.last())
                    .map(|step| step_label(step.step, job.kind));
                let name = AppId::find(app).map_or_else(|| app.clone(), |id| format!("Torro {}", shell::app_name(id)));
                step_row(status, name, at.map(str::to_string))
            })
            .collect()
    } else {
        job.steps
            .iter()
            .map(|step| step_row(step.status, step_label(step.step, job.kind).to_string(), step.detail.clone()))
            .collect()
    };
    column(rows).spacing(space::xs()).into()
}

/// How one app of a job with several stands: failed when a step failed, done when all are, at
/// work while one is, else waiting.
fn app_status(steps: &[&AppJobStep]) -> StepStatus {
    if steps.iter().any(|step| step.status == StepStatus::Failed) {
        StepStatus::Failed
    } else if !steps.is_empty() && steps.iter().all(|step| step.status == StepStatus::Done) {
        StepStatus::Done
    } else if steps.iter().any(|step| matches!(step.status, StepStatus::Now | StepStatus::Done)) {
        StepStatus::Now
    } else {
        StepStatus::Todo
    }
}

/// One step: its mark – done a check in `ok`, at work the dashed dot in `attention`, waiting a
/// dashed ring in `text_3`, failed a cross in `error` (the marks of checks, design skill section 5;
/// nothing turns) –, its name and its detail in `text_3` at the right.
fn step_row<'a>(status: StepStatus, label: String, detail: Option<String>) -> Element<'a, Event> {
    let (which, colour): (Icon, fn(&theme::Palette) -> iced::Color) = match status {
        StepStatus::Done => (Icon::CircleCheck, |p| p.ok),
        StepStatus::Now => (Icon::CircleDotDashed, |p| p.attention_text),
        StepStatus::Todo => (Icon::CircleDashed, |p| p.text_3),
        StepStatus::Failed => (Icon::CircleX, |p| p.error_text),
    };
    let mark = icon::icon(which, icon::SIZE).style(move |t: &Theme, _| svg::Style { color: Some(colour(&palette(t))) });
    let tone: fn(&Theme) -> iced::widget::text::Style =
        if status == StepStatus::Todo { theme::text_3 } else { theme::text };
    let mut line = row![mark, typo::text(typo::body(), label).style(tone).wrapping(Wrapping::None).width(Fill)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if let Some(detail) = detail {
        line = line.push(typo::text(typo::meta(), detail).style(theme::text_3).wrapping(Wrapping::None));
    }
    container(line).center_y(size::row_sidebar()).width(Fill).into()
}

/// The name of a step in the window.
pub fn step_label(step: AppStep, kind: AppJobKind) -> &'static str {
    match step {
        AppStep::Download => tr("Download"),
        // A signature on macOS, the SHA-256 of the release elsewhere.
        AppStep::Verify if cfg!(target_os = "macos") => tr("Check signature"),
        AppStep::Verify => tr("Check download"),
        AppStep::Install => tr("Install"),
        AppStep::Connect => tr("Connect to the service"),
        AppStep::Remove => tr("Remove the app"),
        AppStep::Launcher if kind == AppJobKind::Uninstall => tr("Remove it from the launcher"),
        AppStep::Launcher => tr("Add it to the launcher"),
    }
}

/// What the job does now, over the progress bar ("Copying files…"); "Ready" once it is done.
pub fn doing(job: &AppJobInfo) -> &'static str {
    match job.state {
        AppJobState::Done => return tr("Ready"),
        AppJobState::Failed => return tr("Stopped"),
        AppJobState::Cancelled => return tr("Cancelled"),
        AppJobState::Running => {}
    }
    let now = job.steps.iter().find(|step| step.status == StepStatus::Now).map(|step| step.step);
    match now {
        Some(AppStep::Download) => tr("Downloading…"),
        Some(AppStep::Verify) => tr("Checking the download…"),
        Some(AppStep::Install) => tr("Copying files…"),
        Some(AppStep::Connect) => tr("Connecting to the service…"),
        Some(AppStep::Remove) => tr("Moving to the trash…"),
        Some(AppStep::Launcher) => tr("Updating the launcher…"),
        None => tr("Starting…"),
    }
}

/// Whether "Cancel" can stop the job: only while it downloads (`apps.cancel_app_job`); once it
/// checks or installs it runs to its end, so nothing is left half installed.
pub fn cancellable(job: &AppJobInfo) -> bool {
    job.state == AppJobState::Running
        && job.steps.iter().any(|step| step.step == AppStep::Download && step.status == StepStatus::Now)
}

/// The request that starts `job` again: install and update only (an uninstall asks first).
pub fn retry_request(job: &AppJobInfo) -> Option<Request> {
    if !matches!(job.state, AppJobState::Failed | AppJobState::Cancelled) {
        return None;
    }
    match job.kind {
        AppJobKind::Install => job.apps.first().map(|app| Request::InstallApp { app: app.clone() }),
        AppJobKind::Update => Some(Request::UpdateApps { apps: job.apps.clone() }),
        AppJobKind::Uninstall => None,
    }
}

/// Why a job failed, in words for people (`AppJobInfo::error`).
pub fn failure(code: &str) -> &'static str {
    match code {
        "offline" => tr("Couldn't reach GitHub."),
        "checksum" => tr("The download was damaged. Nothing was installed."),
        "signature" => tr("The download isn't signed by Torro. Nothing was installed."),
        "no-space" => tr("There isn't enough space on this computer."),
        "permission" => tr("Torro Workplace may not write to the folder of the apps."),
        "cancelled" => tr("Cancelled"),
        _ => tr("Something went wrong. Try again."),
    }
}

/// Whether the service that answered with `version` is the restarted one: the expected version,
/// or – without one – any other than this program's.
pub fn is_new(version: &str, expected: Option<&str>) -> bool {
    match expected {
        Some(expected) => version == expected,
        None => version != env!("CARGO_PKG_VERSION"),
    }
}

/// Escape closes the window, as "Close" does.
fn keys(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Event> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Escape), .. }) => Some(Event::Close),
        _ => None,
    }
}

/// The catalog and the appearance at the start; a core that does not answer leaves both out, and
/// the window goes on with what the job says.
fn load() -> Task<Event> {
    Task::perform(
        async {
            let catalog = match call(Request::ListAppCatalog).await {
                Ok(Response::AppCatalog { catalog }) => Some(catalog),
                _ => None,
            };
            let appearance = match call(Request::Settings).await {
                Ok(Response::Settings { settings }) => Some(settings.appearance),
                _ => None,
            };
            (catalog, appearance)
        },
        |(catalog, appearance)| Event::Loaded(Box::new(catalog), appearance),
    )
}

/// The core's answer to `request` as it is (a refusal with its code); `Err` only when none came.
async fn call(request: Request) -> Result<Response, String> {
    let socket = torro_ipc::socket_path().ok_or_else(|| tr("No home directory").to_string())?;
    client::call(&socket, &request).await.map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(target: Target) -> Window {
        let mut window = Window::new(target, false, None);
        let _ = window.update(Event::Loaded(Box::new(Some(apps::demo_catalog(chrono::Utc::now()))), None));
        window
    }

    fn job(state: AppJobState, kind: AppJobKind, restart: bool) -> AppJobInfo {
        AppJobInfo { state, kind, restart, ..apps::demo_job(false) }
    }

    /// `--app-job=` names a job, an uninstall or the main application's update.
    #[test]
    fn the_target_is_read_from_its_word() {
        assert_eq!(Target::parse("a1b2"), Some(Target::Follow("a1b2".into())));
        assert_eq!(Target::parse("uninstall:torro-tasks"), Some(Target::Uninstall("torro-tasks".into())));
        assert_eq!(Target::parse("update:torroworkplace"), Some(Target::SelfUpdate));
        assert_eq!(Target::parse("update:torro-tasks"), None, "apps update from their cards");
        assert_eq!(Target::parse(""), None);
        assert_eq!(Target::parse("uninstall:"), None);
        for target in [Target::Follow("a1".into()), Target::Uninstall("torro-tasks".into()), Target::SelfUpdate] {
            assert_eq!(Target::parse(&target.arg()), Some(target));
        }
    }

    /// A followed job is asked for at once and every tick while it runs; done, the window offers to
    /// open the app, which ends it.
    #[test]
    fn a_followed_job_is_polled_until_it_is_done() {
        let mut window = Window::new(Target::Follow("j1".into()), false, None);
        let catalog = apps::demo_catalog(chrono::Utc::now());
        let first = window.update(Event::Loaded(Box::new(Some(catalog)), Some(Appearance::Dark)));
        assert_eq!(first, Step::Send(Request::AppJob { id: "j1".into() }, Reply::Polled));
        assert_eq!(window.appearance, Appearance::Dark);
        assert!(window.ticks());
        let running = job(AppJobState::Running, AppJobKind::Install, false);
        assert_eq!(window.update(Event::Answer(Reply::Polled, Ok(Response::AppJob { job: running }))), Step::None);
        assert_eq!(
            window.update(Event::Tick),
            Step::Send(Request::AppJob { id: "demo-install".into() }, Reply::Polled)
        );
        window.asking = true;
        assert_eq!(window.update(Event::Tick), Step::None, "one poll at a time");
        window.asking = false;
        let done = job(AppJobState::Done, AppJobKind::Install, false);
        let _ = window.update(Event::Answer(Reply::Polled, Ok(Response::AppJob { job: done })));
        assert!(!window.ticks(), "a finished job is not asked for any more");
        assert_eq!(window.status_line(), trf("Installed {version}", &[("version", &"0.2.0")]));
        assert_eq!(
            window.update(Event::OpenApp),
            Step::Send(Request::OpenApp { app: "torro-assistant".into(), target: None }, Reply::Opened)
        );
        assert_eq!(window.update(Event::Answer(Reply::Opened, Ok(Response::Ok))), Step::Exit);
    }

    /// Uninstalling asks first; the check box goes into the request, the window ends with the job.
    #[test]
    fn uninstalling_asks_first() {
        let mut window = window(Target::Uninstall("torro-inbox-notifications".into()));
        assert_eq!(window.stage, Stage::Ask { remove_data: false });
        assert!(!window.ticks());
        assert_eq!(window.headline(), trf("Uninstall {app}?", &[("app", &format!("Torro {}", tr("App Feed")))]));
        let _ = window.update(Event::RemoveData(true));
        assert_eq!(
            window.update(Event::Confirm),
            Step::Send(
                Request::UninstallApp { app: "torro-inbox-notifications".into(), remove_data: true },
                Reply::Started
            )
        );
        assert_eq!(window.stage, Stage::Starting);
        let running = job(AppJobState::Running, AppJobKind::Uninstall, false);
        let _ = window.update(Event::Answer(Reply::Started, Ok(Response::AppJob { job: running })));
        assert!(window.ticks());
        let done = job(AppJobState::Done, AppJobKind::Uninstall, false);
        assert_eq!(window.update(Event::Answer(Reply::Polled, Ok(Response::AppJob { job: done }))), Step::Exit);
        // "Cancel" of the question ends the window without a request.
        let mut asked = window_for_uninstall();
        assert_eq!(asked.update(Event::Close), Step::Exit);
    }

    fn window_for_uninstall() -> Window {
        window(Target::Uninstall("torro-tasks".into()))
    }

    /// The main application's update asks, then waits for the restarted service with the new
    /// version and has the core open the control centre.
    #[test]
    fn the_main_update_waits_for_the_new_service() {
        let mut window = window(Target::SelfUpdate);
        assert_eq!(window.stage, Stage::Offer);
        assert_eq!(
            window.update(Event::WhatsNew),
            Step::OpenUrl("https://github.com/mahype/TorroWorkplace/releases/tag/v0.2.0".into())
        );
        assert_eq!(
            window.update(Event::Confirm),
            Step::Send(Request::UpdateApps { apps: vec![MAIN_APP.into()] }, Reply::Started)
        );
        let done = AppJobInfo { apps: vec![MAIN_APP.into()], ..job(AppJobState::Done, AppJobKind::Update, true) };
        let _ = window.update(Event::Answer(Reply::Started, Ok(Response::AppJob { job: done })));
        assert!(
            matches!(window.stage, Stage::Restarting { ref expected, came: None, .. } if expected.as_deref() == Some("0.2.0"))
        );
        assert_eq!(window.update(Event::Close), Step::None, "the window stays to open the new control centre");
        assert_eq!(window.update(Event::Tick), Step::Send(Request::Ping, Reply::Pinged));
        let _ = window.update(Event::Answer(Reply::Pinged, Ok(Response::Pong { version: "0.1.0".into(), api: 1 })));
        assert!(matches!(window.stage, Stage::Restarting { came: None, .. }), "still the old one");
        let _ = window.update(Event::Answer(Reply::Pinged, Ok(Response::Pong { version: "0.2.0".into(), api: 1 })));
        let Stage::Restarting { came: Some(_), .. } = &mut window.stage else { panic!("the new one came") };
        // After the handover the control centre opens and the window ends.
        if let Stage::Restarting { came, .. } = &mut window.stage {
            *came = Instant::now().checked_sub(HANDOVER);
        }
        assert_eq!(
            window.update(Event::Tick),
            Step::Send(Request::OpenApp { app: MAIN_APP.into(), target: None }, Reply::Opened)
        );
        assert_eq!(window.update(Event::Answer(Reply::Opened, Ok(Response::Ok))), Step::Exit);
    }

    /// The service may go before the window saw the job done: a poll without answer after a job
    /// with `restart` is the restart too.
    #[test]
    fn a_lost_poll_after_a_restart_job_waits_for_the_service() {
        let mut window = window(Target::SelfUpdate);
        let _ = window.update(Event::Confirm);
        let running = job(AppJobState::Running, AppJobKind::Update, true);
        let _ = window.update(Event::Answer(Reply::Started, Ok(Response::AppJob { job: running })));
        let _ = window.update(Event::Answer(Reply::Polled, Err("connection refused".into())));
        assert!(matches!(window.stage, Stage::Restarting { .. }));
        // Without restart a job the core no longer knows is gone.
        let mut other = Window::new(Target::Follow("j9".into()), false, None);
        let _ = other.update(Event::Loaded(Box::new(None), None));
        let gone = Ok(Response::Error { code: "not-found".into(), message: "no such job".into() });
        let _ = other.update(Event::Answer(Reply::Polled, gone));
        assert!(matches!(other.stage, Stage::Failed(_)));
    }

    /// "Cancel" works while the download runs; a refused start says why and offers to try again
    /// where it can.
    #[test]
    fn cancel_and_refusals() {
        let mut window = Window::new(Target::Follow("j1".into()), false, None);
        let _ = window.update(Event::Loaded(Box::new(None), None));
        let mut downloading = job(AppJobState::Running, AppJobKind::Install, false);
        downloading.steps[0].status = StepStatus::Now;
        downloading.steps[1].status = StepStatus::Todo;
        downloading.steps[2].status = StepStatus::Todo;
        assert!(cancellable(&downloading));
        assert_eq!(doing(&downloading), tr("Downloading…"));
        let _ = window.update(Event::Answer(Reply::Polled, Ok(Response::AppJob { job: downloading })));
        assert_eq!(
            window.update(Event::Cancel),
            Step::Send(Request::CancelAppJob { id: "demo-install".into() }, Reply::Polled)
        );
        let cancelled = job(AppJobState::Cancelled, AppJobKind::Install, false);
        assert_eq!(window.update(Event::Answer(Reply::Polled, Ok(Response::AppJob { job: cancelled }))), Step::Exit);
        let installing = job(AppJobState::Running, AppJobKind::Install, false);
        assert!(!cancellable(&installing), "installing runs to its end");
        assert_eq!(doing(&installing), tr("Copying files…"));

        let mut refused = window_for_uninstall();
        let _ = refused.update(Event::Confirm);
        let busy = Ok(Response::Error { code: "busy".into(), message: "an app job runs".into() });
        let _ = refused.update(Event::Answer(Reply::Started, busy));
        assert_eq!(refused.stage, Stage::Failed(apps::refusal("busy").into()));

        let failed =
            AppJobInfo { error: Some("checksum".into()), ..job(AppJobState::Failed, AppJobKind::Install, false) };
        assert_eq!(retry_request(&failed), Some(Request::InstallApp { app: "torro-assistant".into() }));
        assert_eq!(failure("checksum"), tr("The download was damaged. Nothing was installed."));
        let uninstall = job(AppJobState::Failed, AppJobKind::Uninstall, false);
        assert_eq!(retry_request(&uninstall), None, "an uninstall asks again instead");
    }

    /// The restarted service is the one with the expected version, or any other than this one.
    #[test]
    fn the_new_service_is_told_by_its_version() {
        assert!(is_new("0.2.0", Some("0.2.0")));
        assert!(!is_new("0.1.0", Some("0.2.0")));
        assert!(!is_new(env!("CARGO_PKG_VERSION"), None));
        assert!(is_new("99.0.0", None));
    }

    /// The steps have their names; a job of several apps shows a row per app.
    #[test]
    fn every_stage_builds() {
        assert_eq!(step_label(AppStep::Launcher, AppJobKind::Uninstall), tr("Remove it from the launcher"));
        assert_eq!(step_label(AppStep::Connect, AppJobKind::Install), tr("Connect to the service"));
        let step = |status| AppJobStep { app: "a".into(), step: AppStep::Download, status, detail: None };
        assert_eq!(app_status(&[&step(StepStatus::Done)]), StepStatus::Done);
        assert_eq!(app_status(&[&step(StepStatus::Failed)]), StepStatus::Failed);
        assert_eq!(app_status(&[]), StepStatus::Todo);
        for target in
            [Target::Follow("demo-install".into()), Target::Uninstall("torro-tasks".into()), Target::SelfUpdate]
        {
            let window = window(target);
            drop(window.view());
        }
        let mut several = window(Target::Follow("j".into()));
        let mut job = apps::demo_job(false);
        job.apps = vec!["torro-calendar".into(), "torro-inbox-mail".into()];
        job.kind = AppJobKind::Update;
        let _ = several.update(Event::Answer(Reply::Polled, Ok(Response::AppJob { job })));
        drop(several.view());
        several.stage = Stage::Failed("x".into());
        drop(several.view());
        several.restarting();
        drop(several.view());
        assert_eq!(demo_target(Some(Open::AppsSelfUpdate)), Some(Target::SelfUpdate));
        assert_eq!(demo_target(Some(Open::Menu)), None);
        let settings = window_settings();
        assert!(!settings.decorations && !settings.resizable);
        assert_eq!(settings.size.width, size::panel());
    }
}
