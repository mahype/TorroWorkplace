//! The host of an app's settings: it shows the sections in the app's content area.
//!
//! ```text
//! ┌ Sidebar of the app ──┐┌ Content ─────────────────────────────────────────────────────┐
//! │ T Calendar ▾         ││ ← Settings › Reminders                                    ⚙  │
//! │ Today                │├────────────────┬─────────────────────────────────────────────┤
//! │ Week                 ││ Calendars      │ Reminders                                   │
//! │ Calendars            ││ Reminders      │  Show reminders                      [on]   │
//! │  ● Work              ││ Night          │  Default reminder         15 min before ▾   │
//! │                      ││ ────────────── │ Which events                                │
//! │                      ││ Appearance ↗   │  Only events I accepted              [on]   │
//! │                      ││ All settings ↗ │                                             │
//! └──────────────────────┘└────────────────┴─────────────────────────────────────────────┘
//! ```
//!
//! The gear turns the content area into the settings and back; the app's sidebar stays, and a
//! click in it closes the settings (`docs/settings.md`, "Wie es aussieht"). No window of its own,
//! no dimmed dialog.
//!
//! # What the app does
//!
//! ```ignore
//! // The gear was pressed: a host for this app's sections, and the first load.
//! let (host, load) = Host::new(backend, settings_ui::calendar::SECTIONS.to_vec(), wanted);
//! self.settings = Some(host);
//! return load.map(Message::Settings);
//!
//! // update: the host's own messages, and what a section returned.
//! Message::Settings(message) => match host.update(message) {
//!     Outcome::None => Task::none(),
//!     Outcome::Work(task) => task.map(Message::Settings),
//!     // A change went through: load what this app shows, as a tick would.
//!     Outcome::Changed => self.load(),
//!     Outcome::Close => { self.settings = None; Task::none() }
//!     Outcome::OpenUrl(url) => { links::open(&url); Task::none() }
//!     Outcome::ControlCentre(section) => { connect::open_section(&section); Task::none() }
//!     Outcome::Answer(result) => { settings_ui::assistant::answered(&mut self.sections, &result); Task::none() }
//!     Outcome::App(app) => self.app_action(app),
//! },
//!
//! // view: the host fills the three places of the window's frame.
//! let frame = host.view(class, page, Message::Settings);
//! shell::shell(class, sidebar, frame.header, frame.body, None).overlay(frame.overlay).into()
//! ```
//!
//! # The suite's pages (issue #408)
//!
//! While the main application is not installed the app says so when it makes the host
//! ([`Host::with_memory`]): the host then adds the sections Appearance and General after the app's
//! own ([`SUITE_SECTIONS`]), in place of the two links into the control centre, and shows and runs
//! them itself ([`crate::suite`]) – the app's page for them is not drawn. What they change goes
//! through the app's [`Backend`] like everything else; the zoom, a design, a font or a colour scheme
//! is one `settings.set_setting`, which the app's windows follow as they follow the control centre.
//!
//! Escape: the app hands it to the open section first (a dialog or a page below it swallows it)
//! and only then to the host, which closes the settings – like the control centre routes it.
//!
//! # Answer by answer (issue #394)
//!
//! The host sends every request of a load on its own ([`Message::Part`]) and puts each answer in
//! as it comes, so one slow answer – the statistics over a mail store of gigabytes – holds up
//! nothing but its own part of the page. The page is there once every request that is not
//! [`crate::background`] is back, or at once when the app's [`Memory`] still knows the last
//! state; until a request's first answer the part that shows it says "Updating…"
//! ([`Data::updating`]). A request still on its way is not sent again by the next load, and every
//! answer carries the load (its generation) it was asked for, so an older one never covers a
//! newer one.
//!
//! The sections and apps that listen for a load or a send ([`Message::Loaded`],
//! [`Message::Sent`]) still hear them: once a load's every request that is not in the background
//! is back, the host hands one of them back through the app, with the data as it stands.

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, rule, scrollable, svg};
use iced::{Alignment, Element, Fill, Padding, Task, Theme};

use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, MenuItem, MenuSection, Placement, Trail};
use torro_gui::{shell, style, typo};
use torro_ipc::{Request, Response};

use crate::parts::notice;
use crate::{Action, Backend, Data, Memory, background, suite};

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The loads of every host of the process, counted together (issue #394): an answer of settings
/// already closed that comes to the next host of the same window is older than any of that host's
/// own loads and never mistaken for one of them.
static GENERATIONS: AtomicU64 = AtomicU64::new(1);

/// The one-pixel lines of the design.
const LINE: f32 = 1.0;

/// The two links at the foot of the section list, as the control centre has them: its own section
/// word (`--section`), its icon there and its name, so the same thing looks the same in both
/// windows. Appearance (colour, design pack, zoom) and General are the whole suite's, so an app
/// has a page of its own for them only while the main application is not installed
/// ([`SUITE_SECTIONS`]).
const LINKS: [(&str, Icon, &str); 2] =
    [("appearance", Icon::Sun, "Appearance"), ("general", Icon::Settings, "All settings")];

/// The suite's pages as sections of an app without the main application (issue #408), after its own:
/// the same words, icons and names as the two [`LINKS`] they stand in for.
pub const SUITE_SECTIONS: [Section; 2] = [
    Section { id: suite::APPEARANCE, icon: Icon::Sun, label: "Appearance" },
    Section { id: suite::GENERAL, icon: Icon::Settings, label: "General" },
];

/// One section of an app's settings: what the list, the menu and the path show, and the word
/// `--settings` takes.
///
/// ```ignore
/// pub const SECTIONS: [Section; 3] = [
///     Section { id: "calendars", icon: Icon::Calendar, label: "Calendars" },
///     Section { id: "reminders", icon: Icon::Bell, label: "Reminders" },
///     Section { id: "night", icon: Icon::Moon, label: "Night" },
/// ];
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    /// What `--settings <id>` takes: lowercase ASCII, no spaces ("reminders").
    pub id: &'static str,
    /// Its icon in the list, the menu and the path (`torro_gui::icon`).
    pub icon: Icon,
    /// Its name, English as written in the code; the host puts it through
    /// [`torro_core::i18n::tr`], so a section lists it the way every other UI text is written.
    pub label: &'static str,
}

impl Section {
    /// The translated name, as the list, the menu and the path show it.
    pub fn name(&self) -> &'static str {
        tr(self.label)
    }
}

/// How the host lays its sections out in a window of this size (design skill, section 6): the
/// layout changes, it does not shrink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Wide: the sections as a quiet list left in the content area, the page beside it.
    List,
    /// Medium: the sections as a menu in the header bar ("Reminders ▾"), the page below it.
    Menu,
    /// Narrow and smaller: the list is a page of its own, and a section opens over it with
    /// "‹ Back". Mini and strip tiles show a glance instead of settings; they get this form so a
    /// window that is resized while the settings are open keeps them reachable.
    Steps,
}

/// Which form a window of `class` uses.
pub fn form(class: SizeClass) -> Form {
    match class {
        SizeClass::Wide => Form::List,
        SizeClass::Medium => Form::Menu,
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => Form::Steps,
    }
}

/// The open section's page, as the app builds it from the section module's `view` (and its
/// optional `header`).
pub struct Page<'a, M> {
    /// The page itself: `torro_gui::widgets::group`s of [`crate::parts`] rows.
    pub body: Element<'a, M>,
    /// A page below the section (one bot, one account): its name follows the section in the path,
    /// and the arrow in front of the path goes back to the section's own page instead of closing
    /// the settings. `None` on the section's own page.
    pub sub_page: Option<(String, M)>,
    /// The page's buttons at the right of the header bar, left to right; at most one primary.
    pub actions: Vec<Element<'a, M>>,
    /// The bar that saves a page that does not save itself (`torro_gui::widgets::save_bar`), only
    /// while something is changed; it floats over the bottom of the page.
    pub save_bar: Option<Element<'a, M>>,
    /// Optional wider reading width for pages with several useful columns.
    pub max_width: Option<f32>,
}

impl<'a, M> Page<'a, M> {
    /// The section's own page.
    pub fn new(body: impl Into<Element<'a, M>>) -> Self {
        Page { body: body.into(), sub_page: None, actions: Vec::new(), save_bar: None, max_width: None }
    }

    /// Give this page more room on a normal desktop; narrow windows still use their available width.
    pub fn max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    /// A page below the section: `name` follows in the path, `back` leads up to the section.
    pub fn sub_page(mut self, name: impl Into<String>, back: M) -> Self {
        self.sub_page = Some((name.into(), back));
        self
    }

    /// The buttons at the right of the header bar.
    pub fn actions(mut self, actions: Vec<Element<'a, M>>) -> Self {
        self.actions = actions;
        self
    }

    /// The save bar over the bottom of the page (task DS.5): for a page that does not save itself,
    /// and only while something is changed. The page scrolls far enough to come out from under it.
    pub fn save_bar(mut self, bar: impl Into<Element<'a, M>>) -> Self {
        self.save_bar = Some(bar.into());
        self
    }
}

/// What the host draws, in the three places `torro_gui::shell::shell` takes them.
pub struct Frame<'a, M> {
    /// The 44-high header bar of the content area: the arrow, the path, the page's buttons.
    pub header: Element<'a, M>,
    /// The content area: the section list and the page, as [`Form`] says.
    pub body: Element<'a, M>,
    /// The section menu of the medium form while it is open; pass it to
    /// `torro_gui::shell::Shell::overlay`, stacked under a dialog of the app.
    pub overlay: Option<Element<'a, M>>,
}

/// What the host reports; the app wraps it in its own message (`Message::Settings(…)`).
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A section was chosen, in the list or in the menu.
    Select(&'static str),
    /// The section menu of the medium form was pressed.
    Menu,
    /// A click next to the open menu.
    Close,
    /// Narrow: "‹ Back" from a section to the list of sections.
    List,
    /// The arrow in front of the path, or Escape: close the settings.
    Back,
    /// One of the two links at the foot of the list ("Appearance ↗", "All settings ↗"), as an
    /// index into [`LINKS`].
    Link(usize),
    /// A load came back.
    Loaded(Box<Data>),
    /// What a widget of the suite's pages reported ([`SUITE_SECTIONS`]).
    Suite(suite::Message),
    /// The designs, fonts and colour schemes for the suite's page Appearance came back.
    Looks(Box<suite::Lists>),
    /// A send came back: the fresh data, or what went wrong.
    ///
    /// `changed` says whether it really carried a change. An [`Action::Send`] with no requests is
    /// a plain reload of the sections' data – "Installed something?" in Voice and the clock that
    /// follows the listener use it – and leaves the app's own data untouched, so it answers with
    /// [`Outcome::None`] instead of [`Outcome::Changed`]. Without that, every such ask would pull
    /// a full reload of the window behind it.
    Sent { changed: bool, result: Box<Result<Data, String>> },
    /// The answer to an [`Action::Ask`] came back; the host hands it on as [`Outcome::Answer`].
    Answered(Box<Result<Response, String>>),
    /// The answer to one request of a load came back (issue #394): `generation` is the load it was
    /// sent for, `request` what was asked.
    Part { generation: u64, request: Box<Request>, result: Box<Result<Response, String>> },
    /// The requests of an [`Action::Send`] went through or not (issue #394); the data is then asked
    /// for anew, answer by answer. `changed` as in [`Message::Sent`].
    Done { changed: bool, result: Result<(), String> },
}

/// What the host asks the app to do. `A` is the app's own part of an [`Action`].
pub enum Outcome<A> {
    /// Nothing.
    None,
    /// Run this task and feed its messages back into [`Host::update`].
    Work(Task<Message>),
    /// A change went through: the daemon stored it and the sections' own data is fresh again. The
    /// app now loads **its own** data – its entries, its view, the night, the look – so what was
    /// just changed is on screen at once.
    ///
    /// Without this the app would only notice on its next tick, which is up to a minute away in
    /// the calendar and the tasks app and ten seconds in the inbox: a calendar hidden under
    /// Calendars would stay in the sidebar and in the week all that time. Every app answers it
    /// with the loads that belong to it, like a tick does, and nothing else.
    Changed,
    /// Close the settings: show what the content area showed before.
    Close,
    /// Open this web page in the browser (`torro_gui::links::open`).
    OpenUrl(String),
    /// The answer to an [`Action::Ask`], as it came. The app hands it to the open section's
    /// `answered` (`assistant::answered`), which is the only place that knows what to make of it;
    /// the sections' data is not loaded again, and the host shows no notice – a page that asks
    /// says itself what it heard.
    Answer(Box<Result<Response, String>>),
    /// Open the control centre at this section (`torro_gui::connect::open_section`).
    ControlCentre(String),
    /// What only the app itself can do ([`Action::App`]).
    App(A),
}

/// The settings of one app in its content area: its sections, the data they show, and the way to
/// the daemon.
pub struct Host {
    backend: Arc<dyn Backend>,
    sections: Vec<Section>,
    /// What one load asks for; sent again after every change.
    wanted: Vec<Request>,
    /// The section on screen, an index into `sections`.
    current: usize,
    /// [`Form::Steps`] only: the list of sections instead of a section's page.
    list: bool,
    /// [`Form::Menu`]: the section menu in the header is open.
    menu: bool,
    data: Data,
    /// The page can be shown: the app's [`Memory`] knew the last state, or every request of the
    /// first load that is not in the background is back. Until then the page shows the loading
    /// state.
    loaded: bool,
    /// Why the last change failed, over the page until the next one.
    notice: Option<String>,
    /// Where the data goes when the settings close, for the next host of the same app.
    memory: Memory,
    /// The load the host asks for now; every reload and every change starts a new one.
    generation: u64,
    /// What the current load has sent already: a request is asked once per load.
    asked: Vec<Request>,
    /// What is on its way, each with the load it was sent for; never sent a second time meanwhile.
    flight: Vec<(Request, u64)>,
    /// The load of the newest answer put in, per request: an older answer is dropped.
    applied: Vec<(Request, u64)>,
    /// The first failure of the current load; it stands over the page once the load's every
    /// request that is not in the background is back.
    problem: Option<String>,
    /// The requests answered into `data` – in this host, or in the one the app's [`Memory`] kept
    /// it from: what the next host may show at once ([`Drop`]).
    answered: Vec<Request>,
    /// The current load has been reported as [`Message::Loaded`] or [`Message::Sent`] already.
    announced: bool,
    /// A send went through and its load is not reported yet: `changed` as in [`Message::Sent`].
    change: Option<bool>,
    /// How many [`Message::Loaded`] and [`Message::Sent`] the host handed back and has not seen
    /// again: their data is a copy of its own, which may be older by now than what it holds.
    echoes: usize,
    /// The state of the suite's pages while the host shows them ([`SUITE_SECTIONS`]); `None` with the
    /// main application, where the two links lead to it instead.
    suite: Option<suite::State>,
}

impl Host {
    /// A host for `sections` (the first one is shown) that talks to the daemon through `backend`
    /// and loads `wanted` – what its sections need, nothing more – now and after every change.
    /// Run the task it returns.
    ///
    /// `sections` must not be empty; an app always has at least one section.
    pub fn new(backend: Arc<dyn Backend>, sections: Vec<Section>, wanted: Vec<Request>) -> (Host, Task<Message>) {
        Host::with_memory(backend, sections, wanted, Memory::default(), false)
    }

    /// [`Host::new`], starting from what the app's `memory` kept of the last settings it showed
    /// (issue #394): the page is there at once with that state and says "Updating…" until the
    /// fresh answers are in – when that state holds every part the page waits for, else the page
    /// waits for those. When the host goes, it leaves its data there for the next one, with the
    /// requests that answered into it, once at least one did ([`Drop`]).
    ///
    /// `suite`: the suite's pages are the app's too, because the main application is not installed
    /// (issue #408): the host adds [`SUITE_SECTIONS`] and the settings they show to `sections` and
    /// `wanted`, and lists the designs for Appearance.
    pub fn with_memory(
        backend: Arc<dyn Backend>,
        mut sections: Vec<Section>,
        mut wanted: Vec<Request>,
        memory: Memory,
        suite: bool,
    ) -> (Host, Task<Message>) {
        if suite {
            sections.extend(SUITE_SECTIONS);
            if !wanted.contains(&Request::Settings) {
                wanted.push(Request::Settings);
            }
        }
        let kept = memory.recall();
        // At once only when the last state holds every part the page waits for: a part whose
        // request failed back then would stand with its empty default as if it were known.
        let loaded = kept.as_ref().is_some_and(|kept| {
            wanted.iter().filter(|request| !background(request)).all(|request| kept.answered.contains(request))
        });
        let (data, answered) = kept.map_or_else(Default::default, |kept| (kept.data, kept.answered));
        let mut host = Host {
            backend,
            sections,
            wanted,
            current: 0,
            // Narrow starts at the list: the window is then too small for list and page together,
            // and `--settings <abschnitt>` opens its section with `open`.
            list: true,
            menu: false,
            data,
            loaded,
            notice: None,
            memory,
            generation: 0,
            asked: Vec::new(),
            flight: Vec::new(),
            applied: Vec::new(),
            problem: None,
            answered,
            announced: false,
            change: None,
            echoes: 0,
            suite: suite.then(suite::State::default),
        };
        let load = host.reload();
        let load = match suite {
            true => Task::batch([load, host.list_looks()]),
            false => load,
        };
        (host, load)
    }

    /// Lists the designs, fonts and colour schemes for the suite's page Appearance ([`suite::list`]).
    fn list_looks(&self) -> Task<Message> {
        Task::perform(suite::list(Arc::clone(&self.backend)), |lists| Message::Looks(Box::new(lists)))
    }

    /// Whether the section on screen is one of the suite's pages the host shows itself.
    fn on_suite_page(&self) -> bool {
        self.suite.is_some() && SUITE_SECTIONS.iter().any(|section| section.id == self.current().id)
    }

    /// The one request a suite page's `action` sends, if it sends one: a setting or `system.quit`
    /// as given, the zoom as the setting one step on, a design, font or colour scheme as
    /// [`suite::choose_request`]. Never anything else, and never a client's key or rights.
    fn suite_request(&self, action: &suite::Action) -> Option<Request> {
        match action {
            suite::Action::Send(request) => Some(request.clone()),
            suite::Action::Zoom(step) => {
                let value = serde_json::to_value(step.apply(self.data.settings.zoom)).unwrap_or_default();
                Some(Request::SetSetting { key: torro_ipc::SettingKey::Zoom, value })
            }
            suite::Action::Choose(part, id) => Some(suite::choose_request(*part, id)),
            suite::Action::None | suite::Action::Focus(_) | suite::Action::ListLooks | suite::Action::OpenFolder(_) => {
                None
            }
        }
    }

    /// Carries out what a suite page returned: each change is one request through the backend
    /// ([`Host::suite_request`]); a folder opens here, never in the demo.
    fn suite_act<A>(&mut self, action: suite::Action) -> Outcome<A> {
        if let Some(request) = self.suite_request(&action) {
            return self.act(Action::send(request));
        }
        match action {
            suite::Action::Focus(field) => self.act(Action::Focus(field)),
            suite::Action::ListLooks => Outcome::Work(self.list_looks()),
            suite::Action::OpenFolder(part) => {
                if !self.backend.demo()
                    && let Err(problem) = suite::reveal_folder(part)
                {
                    self.notice = Some(problem);
                }
                Outcome::None
            }
            suite::Action::None | suite::Action::Send(_) | suite::Action::Zoom(_) | suite::Action::Choose(..) => {
                Outcome::None
            }
        }
    }

    /// Shows the section with this id, as `--settings <abschnitt>` asks for it; `false` and
    /// nothing changed when no section has that id (the app then reports the ids it knows).
    pub fn open(&mut self, id: &str) -> bool {
        let Some(at) = self.sections.iter().position(|section| section.id == id) else { return false };
        // Leaving the suite's page closes its dropdowns and its question.
        if at != self.current
            && let Some(state) = self.suite.as_mut()
        {
            suite::leave(state);
        }
        self.current = at;
        self.list = false;
        self.menu = false;
        true
    }

    /// The section on screen.
    pub fn current(&self) -> Section {
        // `new` refuses an empty list, so there is always one.
        self.sections.get(self.current).copied().unwrap_or(Section { id: "", icon: Icon::Settings, label: "Settings" })
    }

    /// Its sections, in the order of the list.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Everything the sections show, as far as it is loaded.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// The first load has come back (successfully or not).
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// Loads the sections' data again: after a change the app made itself, on its tick, or on a
    /// retry. Every request goes on its own ([`Message::Part`]); one still on its way from an
    /// earlier load is not sent again but once its answer is in.
    pub fn reload(&mut self) -> Task<Message> {
        self.generation = GENERATIONS.fetch_add(1, Ordering::Relaxed);
        self.asked.clear();
        self.problem = None;
        self.announced = false;
        Task::batch(self.pump())
    }

    /// Sends what the current load still has to ask – the app's requests and those only the data
    /// can name ([`Data::follow_up`]) – save what is on its way, and reports the load once nothing
    /// but the background is missing.
    fn pump(&mut self) -> Vec<Task<Message>> {
        let mut wanted = self.wanted.clone();
        for request in self.data.follow_up() {
            if !wanted.contains(&request) {
                wanted.push(request);
            }
        }
        let generation = self.generation;
        let mut tasks = Vec::new();
        for request in wanted {
            let on_its_way = self.flight.iter().any(|(held, _)| *held == request);
            if self.asked.contains(&request) || on_its_way {
                continue;
            }
            // Never answered since the settings opened: its part says "Updating…" until it is.
            let answered = self.applied.iter().any(|(held, _)| *held == request);
            if !answered && !self.data.updating(&request) {
                self.data.updating.push(request.clone());
            }
            self.asked.push(request.clone());
            self.flight.push((request.clone(), generation));
            let backend = Arc::clone(&self.backend);
            tasks.push(Task::perform(crate::ask(backend, request.clone()), move |result| Message::Part {
                generation,
                request: Box::new(request.clone()),
                result: Box::new(result),
            }));
        }
        let waiting = self.flight.iter().any(|(request, _)| !background(request));
        if !self.announced && !waiting {
            self.announced = true;
            self.loaded = true;
            self.echoes += 1;
            let data = Box::new(self.data.clone());
            let report = match self.change.take() {
                Some(changed) => Message::Sent { changed, result: Box::new(Ok(*data)) },
                None => Message::Loaded(data),
            };
            tasks.push(Task::done(report));
        }
        tasks
    }

    /// Puts in the answer to one request of the load `generation` ([`Message::Part`]) and sends
    /// what it opens up.
    fn part(&mut self, generation: u64, request: Request, result: Result<Response, String>) -> Vec<Task<Message>> {
        if let Some(at) = self.flight.iter().position(|(held, sent)| *held == request && *sent == generation) {
            self.flight.remove(at);
        }
        let newer = self.applied.iter().any(|(held, put)| *held == request && *put > generation);
        if !newer {
            match result {
                Ok(response) => {
                    self.data.put(&request, response);
                    if !self.answered.contains(&request) {
                        self.answered.push(request.clone());
                    }
                }
                // A failure of an older load counts for nothing: the request is asked again.
                Err(problem) if generation == self.generation => {
                    self.problem.get_or_insert(problem);
                }
                Err(_) => {}
            }
            self.applied.retain(|(held, _)| *held != request);
            self.applied.push((request.clone(), generation));
        }
        // An answer of closed settings does not end "Updating…" while this host's own is still out.
        if !self.flight.iter().any(|(held, _)| *held == request) {
            self.data.updating.retain(|held| *held != request);
        }
        let tasks = self.pump();
        // Nothing but the background is out: the load's first failure stands over the page now, or
        // none does. Not only once all is back: a slow AI server asked again on every tick would
        // keep the page's error as it was for good.
        if !self.flight.iter().any(|(held, _)| !background(held)) {
            self.data.error = self.problem.clone();
        }
        tasks
    }

    /// Carries out what a section returned: [`Action::Send`] goes to the daemon and the data is
    /// loaded again afterwards; the rest the app does (it owns the browser, the control centre and
    /// its own windows).
    pub fn act<A>(&mut self, action: Action<A>) -> Outcome<A> {
        match action {
            Action::None => Outcome::None,
            Action::Send(requests) => {
                self.notice = None;
                // No requests is a plain reload of the sections' data, not a change: the app's own
                // data stays as it is ([`Message::Sent`]).
                let changed = !requests.is_empty();
                let send = crate::apply(Arc::clone(&self.backend), requests);
                Outcome::Work(Task::perform(send, move |result| Message::Done { changed, result }))
            }
            Action::Ask(request) => {
                // One request, one answer, no reload: `send` is for what changes something.
                let ask = crate::ask(Arc::clone(&self.backend), request);
                Outcome::Work(Task::perform(ask, |result| Message::Answered(Box::new(result))))
            }
            Action::OpenUrl(url) => Outcome::OpenUrl(url),
            Action::Open(id) => {
                self.open(id);
                Outcome::None
            }
            // The focus is the window's, not the daemon's: the host does it and nothing else
            // happens.
            Action::Focus(field) => Outcome::Work(iced::widget::operation::focus(field)),
            Action::ControlCentre(section) => Outcome::ControlCentre(section),
            Action::Close => Outcome::Close,
            Action::App(app) => Outcome::App(app),
        }
    }

    /// One of the host's own messages.
    pub fn update<A>(&mut self, message: Message) -> Outcome<A> {
        match message {
            Message::Select(id) => {
                self.open(id);
                Outcome::None
            }
            Message::Menu => {
                self.menu = !self.menu;
                Outcome::None
            }
            Message::Close => {
                self.menu = false;
                Outcome::None
            }
            Message::List => {
                self.list = true;
                self.menu = false;
                Outcome::None
            }
            // Escape on a suite page closes its question or dropdown first.
            Message::Back => match self.suite.as_ref().filter(|_| self.on_suite_page()).and_then(suite::escape) {
                Some(close) => self.update(Message::Suite(close)),
                None => Outcome::Close,
            },
            Message::Suite(message) => {
                let Some(state) = self.suite.as_mut() else { return Outcome::None };
                let action = suite::update(state, message, &self.data.settings);
                self.suite_act(action)
            }
            Message::Looks(lists) => {
                if let Some(state) = self.suite.as_mut() {
                    suite::listed(state, *lists);
                }
                Outcome::None
            }
            Message::Link(at) => match LINKS.get(at) {
                Some((section, ..)) => Outcome::ControlCentre((*section).to_string()),
                None => Outcome::None,
            },
            Message::Loaded(data) => {
                // The host's own report comes back with a copy of what it holds: nothing to put in.
                if !self.echoed() {
                    self.data = *data;
                }
                self.loaded = true;
                self.suite_loaded();
                Outcome::None
            }
            Message::Sent { changed, result } => {
                self.loaded = true;
                match *result {
                    Ok(data) => {
                        if !self.echoed() {
                            self.data = data;
                        }
                        self.suite_loaded();
                        // The sections are fresh; the app's own data follows, but only after a
                        // real change ([`Outcome::Changed`], [`Message::Sent`]).
                        if changed { Outcome::Changed } else { Outcome::None }
                    }
                    Err(problem) => {
                        // Nothing changed, so there is nothing new for the app to load.
                        self.notice = Some(problem);
                        Outcome::None
                    }
                }
            }
            // The answer belongs to the section that asked for it, failures included.
            Message::Answered(result) => Outcome::Answer(result),
            Message::Part { generation, request, result } => work(self.part(generation, *request, *result)),
            Message::Done { changed, result: Ok(()) } => {
                // The change is stored: a new load, reported as the send once the page is fresh. A
                // plain reload that comes back meanwhile does not hide a real change before it.
                self.change = Some(self.change.unwrap_or(false) || changed);
                Outcome::Work(self.reload())
            }
            // Through the app as before, so the sections that wait for the send hear how it ended.
            Message::Done { changed, result: Err(problem) } => {
                Outcome::Work(Task::done(Message::Sent { changed, result: Box::new(Err(problem)) }))
            }
        }
    }

    /// A load is in: the suite's page Appearance names what the core switched with a design chosen
    /// there ([`suite::loaded`]).
    fn suite_loaded(&mut self) {
        if let Some(state) = self.suite.as_mut() {
            suite::loaded(state, &self.data.settings, false);
        }
    }

    /// Whether a [`Message::Loaded`] or [`Message::Sent`] that came in is one the host handed back
    /// itself ([`Host::pump`]); then its data is a copy and may be older than the host's.
    fn echoed(&mut self) -> bool {
        let echo = self.echoes > 0;
        self.echoes = self.echoes.saturating_sub(1);
        echo
    }

    /// The settings in the window's frame: header bar, content area and the layer of an open menu.
    /// `page` is the open section's page, `wrap` the app's message around the host's.
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        class: SizeClass,
        page: Page<'a, M>,
        wrap: fn(Message) -> M,
    ) -> Frame<'a, M> {
        let form = form(class);
        // The menu button registers where it is, the layer floats there: both in this one frame.
        let anchor = Anchor::new();
        let on_list = form == Form::Steps && self.list;
        // The suite's pages are the host's own: the app's page for them is not drawn.
        let page = match self.suite.as_ref().filter(|_| self.on_suite_page()) {
            Some(state) => {
                let settings = &self.data.settings;
                let body = match self.current().id {
                    suite::APPEARANCE => suite::appearance(state, settings),
                    _ => suite::general(settings),
                };
                Page::new(body.map(move |message| wrap(Message::Suite(message))))
            }
            None => page,
        };
        let Page { body, sub_page, actions, save_bar, max_width } = page;
        // The object open below the section, as the list and the menu show it: its name and the
        // way back up.
        let child = sub_page.clone();
        let header = self.header(form, on_list, sub_page, actions, &anchor, wrap);
        let body = match (form, on_list) {
            (Form::Steps, true) => self.page_area(class, self.list_rows(false, None, wrap), None, None),
            (Form::Steps | Form::Menu, _) => self.page_area(class, body, save_bar, max_width),
            (Form::List, _) => {
                let list = container(self.list_rows(true, child.clone(), wrap))
                    .width(size::sidebar())
                    .height(Fill)
                    .padding(Padding::new(space::s()));
                row![list, rule::vertical(LINE).style(style::rule), self.page_area(class, body, save_bar, max_width)]
                    .into()
            }
        };
        let overlay = (form == Form::Menu && self.menu).then(|| {
            widgets::floating(
                &anchor,
                Placement::Below(Align::Start),
                self.menu_card(child, wrap),
                wrap(Message::Close),
            )
        });
        // A suite page's dropdown or question, when the section menu is not open over it.
        let overlay = overlay.or_else(|| {
            let state = self.suite.as_ref().filter(|_| self.on_suite_page() && !on_list)?;
            let layer = suite::overlay(state, &self.data.settings)?;
            Some(layer.map(move |message| wrap(Message::Suite(message))))
        });
        Frame { header, body, overlay }
    }

    /// The header bar: the arrow back, the path, the page's buttons.
    fn header<'a, M: Clone + 'a>(
        &'a self,
        form: Form,
        on_list: bool,
        sub_page: Option<(String, M)>,
        actions: Vec<Element<'a, M>>,
        anchor: &Anchor,
        wrap: fn(Message) -> M,
    ) -> Element<'a, M> {
        let section = self.current();
        // One arrow, one step back: up from a page below the section, in the narrow form back to
        // the list of sections, and from the section itself out of the settings.
        let (label, back) = match (&sub_page, form, on_list) {
            (Some((_, back)), _, _) => (tr("Back"), back.clone()),
            (None, Form::Steps, false) => (tr("Back"), wrap(Message::List)),
            _ => (tr("Close settings"), wrap(Message::Back)),
        };
        let leading = vec![widgets::icon_button(Icon::ChevronLeft, label, Some(back))];
        let title: Element<'a, M> = match (form, on_list) {
            // Narrow: the path has no room; the name of what is open is enough.
            (Form::Steps, true) => shell::header_title(Some(Icon::Settings), tr("Settings"), None),
            (Form::Steps, false) => match sub_page {
                Some((name, _)) => shell::header_title(None, name, None),
                None => shell::header_title(Some(section.icon), section.name(), None),
            },
            (Form::Menu, _) => match sub_page {
                Some((name, _)) => shell::header_title(None, name, None),
                None => widgets::menu_button(anchor, Some(section.icon), section.name(), wrap(Message::Menu)),
            },
            (Form::List, _) => path(section, sub_page.map(|(name, _)| name)),
        };
        shell::header_bar(leading, title, actions)
    }

    /// The sections as rows, and under them the two ways into the control centre. `highlight`
    /// marks the open one – in the narrow list nothing is open yet.
    ///
    /// With a `child` (a page below the open section, task DS.5) its name stands under the section
    /// as the active row of a quiet branch, with the section's icon; the section itself is then not
    /// active, and a click on it goes back up to its own page (the `back` of the page below).
    fn list_rows<'a, M: Clone + 'a>(
        &'a self,
        highlight: bool,
        child: Option<(String, M)>,
        wrap: fn(Message) -> M,
    ) -> Element<'a, M> {
        let rows = self.section_rows(highlight, child.is_some()).into_iter().filter_map(|row| match row {
            SectionRow::Section { at, active, back } => {
                let section = self.sections.get(at).copied()?;
                let press = match (&child, back) {
                    (Some((_, up)), true) => up.clone(),
                    _ => wrap(Message::Select(section.id)),
                };
                Some(widgets::sidebar_item(section.icon, section.name(), Trail::None, active, press))
            }
            SectionRow::Child => {
                // The open page itself: a click on it stays where it is.
                let (name, _) = child.clone()?;
                let section = self.current();
                let row =
                    widgets::sidebar_child(section.icon, name, Trail::None, true, wrap(Message::Select(section.id)));
                Some(widgets::sidebar_branch_quiet(vec![row]))
            }
        });
        let link = |(at, (_, icon, label)): (usize, &(&str, Icon, &'static str))| {
            widgets::sidebar_item(*icon, tr(label), Trail::Icon(Icon::ArrowUpRight), false, wrap(Message::Link(at)))
        };
        let sections = column(rows).spacing(space::xs());
        // The suite's pages stand among the sections then: no links into the control centre.
        if self.suite.is_some() {
            return sections.width(Fill).into();
        }
        let links = column(LINKS.iter().enumerate().map(link)).spacing(space::xs());
        column![
            sections,
            container(rule::horizontal(LINE).style(style::rule)).padding(Padding::new(space::s()).horizontal(0.0)),
            links,
        ]
        .spacing(space::xs())
        .width(Fill)
        .into()
    }

    /// The rows of the section list: one per section, the open one `active` while `highlight`; with
    /// a `child` open below it (task DS.5) the section is not active but leads `back` up, and the
    /// child follows it.
    fn section_rows(&self, highlight: bool, child: bool) -> Vec<SectionRow> {
        let mut rows = Vec::new();
        for at in 0..self.sections.len() {
            let open = highlight && at == self.current;
            rows.push(SectionRow::Section { at, active: open && !child, back: open && child });
            if open && child {
                rows.push(SectionRow::Child);
            }
        }
        rows
    }

    /// The section menu of the medium form, with the two links in a part of their own – the same
    /// rows as the list, as a menu; a `child` open below the section follows it as a nested entry.
    fn menu_card<'a, M: Clone + 'a>(&'a self, child: Option<(String, M)>, wrap: fn(Message) -> M) -> Element<'a, M> {
        let current = self.current().id;
        let mut entries = Vec::new();
        for section in &self.sections {
            let open = section.id == current;
            match child.clone().filter(|_| open) {
                // Both entries stay on the open page: a choice must close the menu, which only the
                // host's own message does; the way up is the arrow in front of the path.
                Some((name, _)) => {
                    let stay = wrap(Message::Select(section.id));
                    entries.push(MenuItem::new(section.icon, section.name(), stay.clone()));
                    entries.push(MenuItem::new(section.icon, name, stay).current(true).nested());
                }
                None => entries
                    .push(MenuItem::new(section.icon, section.name(), wrap(Message::Select(section.id))).current(open)),
            }
        }
        let link = |(at, (_, icon, label)): (usize, &(&str, Icon, &'static str))| {
            MenuItem::new(*icon, tr(label), wrap(Message::Link(at))).trail(Trail::Icon(Icon::ArrowUpRight))
        };
        if self.suite.is_some() {
            return widgets::menu(vec![MenuSection::new(entries)]);
        }
        widgets::menu(vec![MenuSection::new(entries), MenuSection::new(LINKS.iter().enumerate().map(link).collect())])
    }

    /// The page in the content area: the loading state until the first answer, else the notices
    /// over it, at most `content_max` wide, scrolling inside the margin of the size class. A
    /// `save_bar` floats over its bottom, and the page gets the room to scroll out from under it.
    fn page_area<'a, M: Clone + 'a>(
        &'a self,
        class: SizeClass,
        body: Element<'a, M>,
        save_bar: Option<Element<'a, M>>,
        max_width: Option<f32>,
    ) -> Element<'a, M> {
        if !self.loaded {
            return widgets::loading();
        }
        let notices = [&self.notice, &self.data.error].into_iter().flatten().map(|problem| notice(problem.clone()));
        let page = column(notices).push(body).spacing(space::l());
        let mut margin = page_margin(class);
        if save_bar.is_some() {
            margin.bottom = widgets::floating_bar_clearance(1);
        }
        let page = container(widgets::content(max_width.unwrap_or_else(size::content_max), page)).padding(margin);
        let page = scrollable(page)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill);
        match save_bar {
            Some(bar) => widgets::over_bar(page, bar),
            None => page.into(),
        }
    }
}

/// What the settings knew when they closed stays with the app for the next host (issue #394),
/// without what was still on its way and without the failure of that moment, and with the
/// requests that answered into it: the next host shows it at once only when those cover its page
/// ([`Host::with_memory`]). Settings that never got an answer leave the last state as it was.
impl Drop for Host {
    fn drop(&mut self) {
        if self.answered.is_empty() {
            return;
        }
        let mut data = std::mem::take(&mut self.data);
        data.updating.clear();
        data.error = None;
        self.memory.keep(crate::Kept { data, answered: std::mem::take(&mut self.answered) });
    }
}

/// The tasks of a load as the host's work, nothing when there are none.
fn work<A>(tasks: Vec<Task<Message>>) -> Outcome<A> {
    if tasks.is_empty() { Outcome::None } else { Outcome::Work(Task::batch(tasks)) }
}

/// A row of the section list ([`Host::section_rows`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionRow {
    /// The section at `at`; `back` sends the open page below it back up instead of selecting it.
    Section { at: usize, active: bool, back: bool },
    /// The page open below the section above, as the active row of a quiet branch.
    Child,
}

/// "Settings › Reminders › Work": what is open in `BODY_STRONG`, the way to it in `text_3`.
fn path<'a, M: 'a>(section: Section, sub_page: Option<String>) -> Element<'a, M> {
    let quiet = |text: &'static str| typo::text(typo::body(), text).style(theme::text_3).wrapping(Wrapping::None);
    let step = || {
        icon::icon(Icon::ChevronRight, icon::SIZE_SM)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) })
    };
    let mut crumbs = row![quiet(tr("Settings")), step()].spacing(space::xs()).align_y(Alignment::Center);
    match sub_page {
        Some(name) => {
            crumbs = crumbs.push(quiet(section.name())).push(step());
            crumbs = crumbs.push(typo::text(typo::body_strong(), name).wrapping(Wrapping::None));
        }
        None => crumbs = crumbs.push(typo::text(typo::body_strong(), section.name()).wrapping(Wrapping::None)),
    }
    crumbs.into()
}

/// The margin around the page: the design's page padding (24 in the standard), in a narrow window
/// 16, in a tile 12 (design skill, section 6) – as the control centre's pages have it.
fn page_margin(class: SizeClass) -> Padding {
    match class {
        SizeClass::Wide | SizeClass::Medium => layout::page_padding(),
        SizeClass::Narrow => Padding::new(space::l()),
        SizeClass::Mini | SizeClass::Strip => Padding::new(space::m()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;
    use crate::demo::Demo;

    fn sections() -> Vec<Section> {
        vec![
            Section { id: "calendars", icon: Icon::Calendar, label: "Calendars" },
            Section { id: "reminders", icon: Icon::Bell, label: "Reminders" },
        ]
    }

    fn host() -> Host {
        let (mut host, _) = Host::new(Arc::new(Demo::new()), sections(), vec![Request::Settings]);
        host.update::<()>(Message::Loaded(Box::default()));
        host
    }

    /// An example section's page, as an app hands it over.
    fn page<'a>() -> Page<'a, Message> {
        Page::new(iced::widget::space::horizontal()).actions(vec![widgets::text_button(
            None,
            tr("Add"),
            style::button::secondary,
            Some(Message::Back),
        )])
    }

    /// DS.5: with a page open below the section, the list shows it as the section's child and the
    /// section leads back up; without one the list is as before.
    #[test]
    fn a_page_below_a_section_stands_as_its_child() {
        let mut host = host();
        host.open("reminders");
        let section = |at, active, back| SectionRow::Section { at, active, back };
        assert_eq!(host.section_rows(true, false), vec![section(0, false, false), section(1, true, false)]);
        assert_eq!(
            host.section_rows(true, true),
            vec![section(0, false, false), section(1, false, true), SectionRow::Child]
        );
        // The narrow list highlights nothing and so shows no child.
        assert_eq!(host.section_rows(false, true), vec![section(0, false, false), section(1, false, false)]);
        let below = page().sub_page("Work", Message::Back);
        let _: Frame<'_, Message> = host.view(SizeClass::Wide, below, |m| m);
        host.menu = true;
        let below = page().sub_page("Work", Message::Back);
        assert!(host.view(SizeClass::Medium, below, |m| m).overlay.is_some());
        // The section's entry in the menu selects it, so the menu closes.
        let _: Outcome<()> = host.update(Message::Select("reminders"));
        assert!(!host.menu, "a choice closes the menu");
    }

    /// DS.5: a page with a save bar builds in every form.
    #[test]
    fn a_page_with_a_save_bar_builds_in_every_form() {
        let host = host();
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            let bar = widgets::save_bar("Signature changed", Some(Message::Back), Some(Message::Back), false);
            let page = page().sub_page("Work", Message::Back).save_bar(bar);
            let _: Frame<'_, Message> = host.view(class, page, |m| m);
        }
    }

    /// Every size class builds its form, in every state the host can be in: the list, a section,
    /// and a page below it – nothing is left unbuilt in a layout nobody tried.
    #[test]
    fn every_form_builds_in_every_state() {
        let mut host = host();
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            for open in [false, true] {
                host.list = !open;
                for menu in [false, true] {
                    host.menu = menu;
                    let frame = host.view(class, page(), |m| m);
                    let _ = (frame.header, frame.body, frame.overlay);
                    let below = page().sub_page("Work", Message::Back);
                    let _: Frame<'_, Message> = host.view(class, below, |m| m);
                }
            }
        }
        // Before the first answer the page is the loading state, not an empty list.
        let (loading, _) = Host::new(Arc::new(Demo::new()), sections(), vec![Request::Settings]);
        let _: Frame<'_, Message> = loading.view(SizeClass::Wide, page(), |m| m);
        assert!(!loading.loaded());
    }

    /// The first section is the one on screen; `--settings <abschnitt>` opens another.
    #[test]
    fn a_word_opens_its_section() {
        let mut host = host();
        assert_eq!(host.current().id, "calendars");
        assert!(host.open("reminders"));
        assert_eq!(host.current().id, "reminders");
        assert!(!host.open("rules"), "a section the app does not have");
        assert_eq!(host.current().id, "reminders", "and nothing changed");
    }

    /// Choosing a section in the list or the menu shows it and closes the menu.
    #[test]
    fn choosing_a_section_closes_the_menu() {
        let mut host = host();
        host.update::<()>(Message::Menu);
        assert!(host.menu);
        host.update::<()>(Message::Select("reminders"));
        assert_eq!(host.current().id, "reminders");
        assert!(!host.menu);
    }

    /// Narrow: a section opens from the list, "‹ Back" leads to the list again.
    #[test]
    fn narrow_steps_between_list_and_section() {
        let mut host = host();
        assert!(host.list, "narrow starts at the list");
        host.update::<()>(Message::Select("reminders"));
        assert!(!host.list);
        host.update::<()>(Message::List);
        assert!(host.list);
    }

    /// Escape and the arrow close the settings.
    #[test]
    fn escape_closes_the_settings() {
        let mut host = host();
        assert!(matches!(host.update::<()>(Message::Back), Outcome::Close));
    }

    /// The two links at the foot of the list lead into the control centre, where Appearance and
    /// General live for the whole suite.
    #[test]
    fn the_links_lead_to_the_control_centre() {
        let mut host = host();
        let appearance: Outcome<()> = host.update(Message::Link(0));
        assert!(matches!(appearance, Outcome::ControlCentre(ref section) if section == "appearance"));
        let all: Outcome<()> = host.update(Message::Link(1));
        assert!(matches!(all, Outcome::ControlCentre(ref section) if section == "general"));
        assert_eq!(LINKS.len(), 2, "no app has an Appearance page of its own, only these two links");
    }

    /// Issue #408: without the main application the suite's pages stand among the app's sections in
    /// place of the two links, `--settings appearance|general` opens them, and the settings they show
    /// are loaded; with it nothing changes.
    #[test]
    fn the_suite_pages_stand_in_for_the_links_without_the_main_app() {
        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        let (mut alone, _) = Host::with_memory(Arc::clone(&backend), sections(), Vec::new(), Memory::default(), true);
        let ids: Vec<&str> = alone.sections().iter().map(|section| section.id).collect();
        assert_eq!(ids, ["calendars", "reminders", "appearance", "general"]);
        assert!(alone.wanted.contains(&Request::Settings), "the pages show the settings");
        assert!(alone.open("appearance") && alone.on_suite_page());
        assert!(alone.open("general") && alone.on_suite_page());
        assert!(alone.open("reminders") && !alone.on_suite_page());
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            for id in ["appearance", "general"] {
                alone.open(id);
                let _ = alone.view(class, page(), |message| message);
            }
        }

        let (mut with_main, _) = Host::with_memory(backend, sections(), Vec::new(), Memory::default(), false);
        assert_eq!(with_main.sections().len(), 2, "the app's own sections only");
        assert!(!with_main.open("appearance") && !with_main.open("general"));
        let link: Outcome<()> = with_main.update(Message::Link(0));
        assert!(matches!(link, Outcome::ControlCentre(ref section) if section == "appearance"));
    }

    /// Issue #408: every action of the suite's pages sends exactly its own request through the
    /// backend – a setting with its key, or `system.quit` after the question – and never one of
    /// `clients.*`; the rest (focus, lists, folders) sends nothing.
    #[test]
    fn every_suite_action_sends_only_its_own_request() {
        use torro_core::design::Part;
        use torro_core::settings::Language;
        use torro_core::theme::Appearance;
        use torro_gui::zoom::Step;
        use torro_ipc::SettingKey;

        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        let (host, _) = Host::with_memory(backend, sections(), Vec::new(), Memory::default(), true);
        let mut state = suite::State::default();
        let settings = host.data.settings.clone();
        let key = |request: &Request| match request {
            Request::SetSetting { key, .. } => Some(*key),
            _ => None,
        };
        let cases = [
            (suite::Message::Language(Language::German), Some(SettingKey::Language)),
            (suite::Message::Appearance(Appearance::Dark), Some(SettingKey::Appearance)),
            (suite::Message::StartAtLogin(true), Some(SettingKey::StartAtLogin)),
            (suite::Message::Zoom(Step::In), Some(SettingKey::Zoom)),
            (suite::Message::Choose(Part::Design, "compact".into()), Some(SettingKey::Design)),
            (suite::Message::Choose(Part::Font, "torro".into()), Some(SettingKey::Font)),
            (suite::Message::Choose(Part::ColorScheme, "torro".into()), Some(SettingKey::ColorScheme)),
        ];
        for (message, expected) in cases {
            let action = suite::update(&mut state, message.clone(), &settings);
            let request = host.suite_request(&action).expect("a change sends a request");
            assert_eq!(key(&request), expected, "{message:?}");
            assert!(!request.operation().name().starts_with("clients."), "{message:?}");
        }
        // "Quit" asks first and sends nothing; only "Quit service" sends `system.quit`.
        assert_eq!(host.suite_request(&suite::update(&mut state, suite::Message::Quit, &settings)), None);
        let quit = suite::update(&mut state, suite::Message::QuitConfirmed, &settings);
        assert_eq!(host.suite_request(&quit), Some(Request::Quit));
        for message in [suite::Message::ToggleFonts, suite::Message::Reload, suite::Message::OpenFolder(Part::Font)] {
            let action = suite::update(&mut state, message.clone(), &settings);
            assert_eq!(host.suite_request(&action), None, "{message:?}");
        }
    }

    /// Every size class gets its own form (design skill, section 6).
    #[test]
    fn each_size_class_has_its_form() {
        assert_eq!(form(SizeClass::Wide), Form::List);
        assert_eq!(form(SizeClass::Medium), Form::Menu);
        assert_eq!(form(SizeClass::Narrow), Form::Steps);
        assert_eq!(form(SizeClass::Mini), Form::Steps);
        assert_eq!(form(SizeClass::Strip), Form::Steps);
    }

    /// A section's `Send` reaches the daemon in order and ends in fresh data; `None` does nothing.
    #[tokio::test]
    async fn send_reaches_the_daemon_and_reloads() {
        let backend = Arc::new(Demo::new());
        let (mut host, _) = Host::new(Arc::clone(&backend) as Arc<dyn Backend>, sections(), vec![Request::Settings]);
        let zoom = Request::SetSetting { key: torro_ipc::SettingKey::Zoom, value: serde_json::json!(1.1) };
        let night = Request::SetSetting { key: torro_ipc::SettingKey::Night, value: serde_json::json!({}) };
        let action: Action<()> = Action::Send(vec![zoom.clone(), night.clone()]);
        assert!(matches!(host.act(action), Outcome::Work(_)));
        // The task the host builds does what `crate::send` does; run that directly.
        let way = Arc::clone(&backend) as Arc<dyn Backend>;
        let data = crate::send(way, vec![zoom.clone(), night.clone()], vec![Request::Settings])
            .await
            .expect("the demo answers");
        assert_eq!(backend.sent(), vec![zoom, night], "in the order the section gave them");
        let outcome: Outcome<()> = host.update(Message::Sent { changed: true, result: Box::new(Ok(data)) });
        assert!(matches!(outcome, Outcome::Changed), "the app loads its own data now, not in a minute");
        assert!(host.loaded(), "the fresh data is there");
        assert_eq!(host.data().settings.appearance, backend.settings().appearance);
        assert!(matches!(host.act::<()>(Action::None), Outcome::None));
    }

    /// A `Send` with no requests is a plain reload of the sections' data – "Installed something?"
    /// in Voice and the clock that follows the listener use it – and must not pull the app's own
    /// reload behind it: the second review of #210 found five full window reloads for the five
    /// asks after voice was switched on.
    #[tokio::test]
    async fn a_send_with_no_requests_changes_nothing() {
        let backend = Arc::new(Demo::new());
        let (mut host, _) = Host::new(Arc::clone(&backend) as Arc<dyn Backend>, sections(), vec![Request::Settings]);
        host.notice = Some("an older problem".to_string());

        assert!(matches!(host.act::<()>(Action::Send(Vec::new())), Outcome::Work(_)), "it still asks again");
        assert!(host.notice.is_none(), "and the old notice goes, as with every send");
        assert!(backend.sent().is_empty(), "nothing went out but the load");

        // The data comes back as fresh as after any send; only the app is left alone.
        let data = crate::send(Arc::clone(&backend) as Arc<dyn Backend>, Vec::new(), vec![Request::Settings])
            .await
            .expect("the demo answers");
        let outcome: Outcome<()> = host.update(Message::Sent { changed: false, result: Box::new(Ok(data)) });
        assert!(matches!(outcome, Outcome::None), "nothing changed, so the app loads nothing");
        assert!(host.loaded() && host.data().settings.appearance == backend.settings().appearance);
    }

    /// An `Ask` reaches the daemon and its answer comes back to the section that asked, without
    /// loading the sections' data again (task S.17: "Say something").
    #[tokio::test]
    async fn an_ask_brings_its_answer_back_without_a_reload() {
        let backend = Arc::new(Demo::new());
        let (mut host, _) = Host::new(Arc::clone(&backend) as Arc<dyn Backend>, sections(), vec![Request::Settings]);
        assert!(matches!(host.act::<()>(Action::Ask(Request::TestVoice)), Outcome::Work(_)));
        // The task the host builds does what `crate::ask` does; run that directly.
        let answer = crate::ask(Arc::clone(&backend) as Arc<dyn Backend>, Request::TestVoice).await;
        assert_eq!(backend.sent(), vec![Request::TestVoice], "it went out as it was given");
        let outcome: Outcome<()> = host.update(Message::Answered(Box::new(answer)));
        let Outcome::Answer(result) = outcome else { panic!("the answer belongs to the section") };
        assert!(result.is_ok());
        assert!(!host.loaded(), "an Ask loads nothing: it changed nothing");
        assert!(host.notice.is_none(), "and the page says itself what it heard");
    }

    /// A failed change stands over the page instead of being lost, and asks for no reload: nothing
    /// changed.
    #[test]
    fn a_failed_change_is_reported() {
        let mut host = host();
        let failed = Box::new(Err("not connected".to_string()));
        let outcome: Outcome<()> = host.update(Message::Sent { changed: true, result: failed });
        assert!(matches!(outcome, Outcome::None), "nothing to load again");
        assert_eq!(host.notice.as_deref(), Some("not connected"));
    }

    /// `Action::Open` switches the section, the rest is handed to the app.
    #[test]
    fn an_action_opens_a_section_or_goes_to_the_app() {
        let mut host = host();
        assert!(matches!(host.act::<()>(Action::Open("reminders")), Outcome::None));
        assert_eq!(host.current().id, "reminders");
        let url: Outcome<()> = host.act(Action::OpenUrl("https://example.org".to_string()));
        assert!(matches!(url, Outcome::OpenUrl(ref it) if it == "https://example.org"));
        let centre: Outcome<()> = host.act(Action::ControlCentre("accounts".to_string()));
        assert!(matches!(centre, Outcome::ControlCentre(ref it) if it == "accounts"));
        assert!(matches!(host.act(Action::App(7u8)), Outcome::App(7)));
    }

    /// The names come through the translation table, so the list reads in the system's language.
    #[test]
    fn section_names_are_translated() {
        assert_eq!(sections()[1].name(), tr("Reminders"));
    }

    /// Answers the first request the host has on its way, as the task it built would.
    async fn answer_one(host: &mut Host, backend: &Arc<dyn Backend>) -> Option<Request> {
        let (request, generation) = host.flight.first().cloned()?;
        let result = backend.call(request.clone()).await;
        let _: Outcome<()> =
            host.update(Message::Part { generation, request: Box::new(request.clone()), result: Box::new(result) });
        Some(request)
    }

    /// Answers everything the host sends, follow-ups included, until nothing is on its way.
    async fn answer_all(host: &mut Host, backend: &Arc<dyn Backend>) {
        while answer_one(host, backend).await.is_some() {}
    }

    /// The statistics of all mail, slow on a large store.
    fn stats() -> Request {
        Request::MailAccountStats { account: None }
    }

    /// A way to the daemon that is not there: every request fails.
    struct Down;

    impl Backend for Down {
        fn call(&self, _request: Request) -> crate::Answer {
            Box::pin(async { Err("The service is not running.".to_string()) })
        }
    }

    /// #394: the page is there once every request but the background is back; the statistics fill
    /// their part later and say "Updating…" until then.
    #[tokio::test]
    async fn the_page_does_not_wait_for_the_background() {
        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        let (mut host, _) = Host::new(Arc::clone(&backend), sections(), vec![stats(), Request::Settings]);
        assert!(!host.loaded(), "nothing is known yet");
        assert_eq!(host.data().updating, vec![stats(), Request::Settings], "everything is on its way");
        assert_eq!(host.flight.len(), 2, "each request goes on its own");

        // The settings answer first, the statistics are still out.
        let at = host.flight.iter().position(|(request, _)| *request == Request::Settings).expect("sent");
        host.flight.swap(0, at);
        assert_eq!(answer_one(&mut host, &backend).await, Some(Request::Settings));
        assert!(host.loaded(), "the page stands without the statistics");
        assert!(host.data().updating(&stats()) && !host.data().updating(&Request::Settings));
        assert!(host.data().mail_stats.is_empty());

        answer_all(&mut host, &backend).await;
        assert!(host.data().updating.is_empty(), "every first answer is in");
        assert!(!host.data().mail_stats.is_empty(), "the statistics filled their part");
        assert!(
            host.applied.iter().any(|(request, _)| matches!(request, Request::GetMailCachePolicy { .. })),
            "the follow-ups the statistics name were asked for as well"
        );
    }

    /// #394: a reload on the tick does not send a request again whose answer is still out; it goes
    /// once that answer is in, so the newer load gets an answer of its own.
    #[tokio::test]
    async fn a_request_on_its_way_is_not_sent_again() {
        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        let (mut host, _) = Host::new(Arc::clone(&backend), sections(), vec![stats()]);
        let first = host.generation;
        for _ in 0..3 {
            let _ = host.reload();
        }
        assert_eq!(host.flight, vec![(stats(), first)], "one on its way, from the first load");

        answer_one(&mut host, &backend).await;
        let again: Vec<&(Request, u64)> = host.flight.iter().filter(|(request, _)| *request == stats()).collect();
        assert_eq!(again, [&(stats(), host.generation)], "asked again, once, for the newest load");
        assert!(host.generation > first);
        assert!(!host.data().updating(&stats()), "it answered once, so it no longer says \"Updating…\"");
    }

    /// #394: every answer carries the load it was asked for; one of an older load never covers what
    /// a newer one brought.
    #[test]
    fn an_older_answer_never_covers_a_newer_one() {
        let (mut host, _) = Host::new(Arc::new(Demo::new()), sections(), vec![Request::Settings]);
        let with_zoom = |zoom: torro_core::settings::Zoom| {
            let settings = torro_core::settings::Settings { zoom, ..Default::default() };
            Box::new(Ok(Response::Settings { settings }))
        };
        let part = |generation, result| Message::Part { generation, request: Box::new(Request::Settings), result };
        let _: Outcome<()> = host.update(part(3, with_zoom(torro_core::settings::Zoom::MAX)));
        let _: Outcome<()> = host.update(part(2, with_zoom(torro_core::settings::Zoom::MIN)));
        assert_eq!(host.data().settings.zoom, torro_core::settings::Zoom::MAX, "the newer answer stays");
    }

    /// #394: the answer of settings already closed that reaches the next host of the window is
    /// older than any load of it, so it never covers that host's own answers.
    #[tokio::test]
    async fn an_answer_of_closed_settings_is_older_than_the_new_ones() {
        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        let (mut old, _) = Host::new(Arc::clone(&backend), sections(), vec![Request::Settings]);
        for _ in 0..5 {
            let _ = old.reload();
        }
        let (stale, generation) = old.flight.first().cloned().expect("on its way");
        let (mut new, _) = Host::new(Arc::clone(&backend), sections(), vec![Request::Settings]);
        assert!(new.generation > old.generation, "loads are counted across hosts");
        let result = Box::new(backend.call(stale.clone()).await);
        let _: Outcome<()> = new.update(Message::Part { generation, request: Box::new(stale), result });
        assert_eq!(new.flight.len(), 1, "its own request is still on its way");
        answer_all(&mut new, &backend).await;
        assert!(new.applied.iter().all(|(_, put)| *put == new.generation), "its own answer is put in");
    }

    /// #394: a plain reload that comes back while a change is not reported yet does not turn the
    /// change into a plain reload.
    #[test]
    fn a_plain_reload_does_not_hide_a_change() {
        let (mut host, _) = Host::new(Arc::new(Demo::new()), sections(), vec![Request::Settings]);
        let _: Outcome<()> = host.update(Message::Done { changed: true, result: Ok(()) });
        let _: Outcome<()> = host.update(Message::Done { changed: false, result: Ok(()) });
        assert_eq!(host.change, Some(true));
    }

    /// #394: a failure stands over the page once every request of the load that is not in the
    /// background is back, not before – a load that is half in does not blink an error that its
    /// next answer would take back.
    #[tokio::test]
    async fn a_failure_stands_with_the_last_answer() {
        let backend: Arc<dyn Backend> = Arc::new(Down);
        let (mut host, _) = Host::new(Arc::clone(&backend), sections(), vec![Request::Settings, Request::Accounts]);
        answer_one(&mut host, &backend).await;
        assert!(host.data().error.is_none(), "one answer is still out");
        answer_one(&mut host, &backend).await;
        assert_eq!(host.data().error.as_deref(), Some("The service is not running."));
        assert!(host.loaded() && host.data().updating.is_empty(), "the page shows what failed");
    }

    /// #394: closed and opened again, the settings start from what they showed – at once, with
    /// "Updating…" over it – and not from an empty page.
    #[tokio::test]
    async fn the_memory_shows_the_last_state_at_once() {
        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        let memory = Memory::default();
        let wanted = vec![Request::Settings, stats()];
        let (mut first, _) = Host::with_memory(Arc::clone(&backend), sections(), wanted.clone(), memory.clone(), false);
        answer_all(&mut first, &backend).await;
        let shown = first.data().clone();
        drop(first);

        // Closed before anything came: nothing is kept, so the next one does not stand empty.
        let untouched = Memory::default();
        drop(Host::with_memory(Arc::clone(&backend), sections(), wanted.clone(), untouched.clone(), false));
        assert!(untouched.recall().is_none());

        // The settings failed back then: kept, but no page at once with its empty defaults – it
        // waits for the settings, and the statistics that did answer stand meanwhile.
        let partly = Memory::default();
        let (mut failed, _) =
            Host::with_memory(Arc::clone(&backend), sections(), wanted.clone(), partly.clone(), false);
        while let Some((request, generation)) = failed.flight.first().cloned() {
            let result = match request {
                Request::Settings => Err("not running".to_string()),
                _ => backend.call(request.clone()).await,
            };
            let _: Outcome<()> =
                failed.update(Message::Part { generation, request: Box::new(request), result: Box::new(result) });
        }
        drop(failed);
        let (waiting, _) = Host::with_memory(Arc::clone(&backend), sections(), wanted.clone(), partly, false);
        assert!(!waiting.loaded(), "the settings never answered");
        assert_eq!(waiting.data().mail_stats, shown.mail_stats);

        let (again, _) = Host::with_memory(Arc::clone(&backend), sections(), wanted.clone(), memory, false);
        assert!(again.loaded(), "no loading state: the last state is there");
        assert_eq!(again.data().mail_stats, shown.mail_stats);
        assert_eq!(again.data().settings, shown.settings);
        assert!(wanted.iter().all(|request| again.data().updating(request)), "and it says it is updating");
        assert!(again.data().error.is_none());
    }

    /// #394: a change is sent on its own and the data then loaded answer by answer; once it is
    /// fresh the host reports the send through the app as before, and its copy of the data does
    /// not cover what came meanwhile.
    #[tokio::test]
    async fn a_change_reports_its_send_once_the_page_is_fresh() {
        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        let (mut host, _) = Host::new(Arc::clone(&backend), sections(), vec![Request::Settings, stats()]);
        answer_all(&mut host, &backend).await;
        let zoom = Request::SetSetting { key: torro_ipc::SettingKey::Zoom, value: serde_json::json!(1.1) };
        assert!(matches!(host.act::<()>(Action::send(zoom.clone())), Outcome::Work(_)));
        crate::apply(Arc::clone(&backend), vec![zoom]).await.expect("the demo answers");
        let echoes = host.echoes;

        let outcome: Outcome<()> = host.update(Message::Done { changed: true, result: Ok(()) });
        assert!(matches!(outcome, Outcome::Work(_)), "the data is asked for anew");
        assert_eq!(host.change, Some(true));
        answer_all(&mut host, &backend).await;
        assert_eq!((host.change, host.echoes), (None, echoes + 1), "the send is reported once");

        let held = host.data().clone();
        let outcome: Outcome<()> = host.update(Message::Sent { changed: true, result: Box::new(Ok(Data::default())) });
        assert!(matches!(outcome, Outcome::Changed), "the app loads its own data now");
        assert_eq!(*host.data(), held, "the report's copy is not put over the host's data");

        // A send that failed reaches the app as before and stands over the page.
        let failed: Outcome<()> = host.update(Message::Done { changed: true, result: Err("refused".to_string()) });
        assert!(matches!(failed, Outcome::Work(_)));
        let _: Outcome<()> = host.update(Message::Sent { changed: true, result: Box::new(Err("refused".to_string())) });
        assert_eq!(host.notice.as_deref(), Some("refused"));
    }
}
