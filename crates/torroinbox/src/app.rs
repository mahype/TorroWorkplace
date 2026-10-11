//! The inbox window (task E3.1 in `docs/tasks/eingang.md`): state, loading through
//! [`InboxSource`], and the Linear window around it – `torro_gui::shell` with the sidebar ("For
//! me" with its count, "Info", "Snoozed", "Done", the group "Sources") in the wide layout, the
//! rail of icons in the medium one (and in the wide one when the user collapses it: panel button
//! or `[`, remembered in `ui.json`), a navigation menu ("For me ▾") plus drawer in the medium and
//! narrow ones. The list (`list.rs`) and the panel (`detail.rs`) only render and report clicks as
//! [`Message`]s.
//!
//! The panel is `torro_gui::panel`: beside the list in the wide layout, over it in the medium and
//! narrow ones (full width in narrow). Mini and strip get their glance with E3.3; until then they
//! show the narrow layout, as the shell does for every app without a glance.
//!
//! Entries are asked for on a timer ([`REFRESH`]) and after every change; the core has no
//! `subscribe` yet (E1.2). The keyboard of E3.2 (`j`/`k`, `e`, `s` …) comes in `keys.rs`; here only
//! Escape (closes menu, drawer, then the panel) and `[` (sidebar) work.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use iced::theme::Mode;
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::connect;
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::panel;
use torro_gui::shell::{self, SidebarMode, Switcher};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{datetime, typo};
use torro_inbox::{InboxSource, Item, ItemFilter, Lane, Link, SourceInfo, SourceRef, StateFilter};

use crate::{detail, list};

/// The app's identity: window class `torro-inbox`, binary `torroinbox`, state in `torroinbox/`.
pub const INBOX: AppId = AppId::INBOX;

/// How often the window asks the source again. Short, because a waiting agent should show up
/// within seconds; the source is local and cheap to ask, and nothing moves on screen unless an
/// entry or a time in the list changed.
const REFRESH: Duration = Duration::from_secs(10);

/// What the list shows. Each view is one [`ItemFilter`] ([`View::filter`]), so the app asks the
/// source exactly what the API offers (`inbox.list_items`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// Open entries in the lane "For me"; the only view with a count.
    ForMe,
    /// Open entries in the lane "Info".
    Info,
    /// Snoozed entries, both lanes.
    Snoozed,
    /// Done entries, both lanes.
    Done,
    /// Open entries of one source, both lanes.
    Source(SourceRef),
}

impl View {
    /// The filter for `inbox.list_items` that selects the entries of this view.
    pub fn filter(&self) -> ItemFilter {
        let (lane, source, state) = match self {
            View::ForMe => (Some(Lane::ForMe), None, StateFilter::Open),
            View::Info => (Some(Lane::Info), None, StateFilter::Open),
            View::Snoozed => (None, None, StateFilter::Snoozed),
            View::Done => (None, None, StateFilter::Done),
            View::Source(source) => (None, Some(source.clone()), StateFilter::Open),
        };
        ItemFilter { lane, source, state: Some(state), ..ItemFilter::default() }
    }

    /// The name in the sidebar, the header and the panel's path.
    pub fn name(&self) -> String {
        match self {
            View::ForMe => tr("For me").into(),
            View::Info => tr("Info").into(),
            View::Snoozed => tr("Snoozed").into(),
            View::Done => tr("Done").into(),
            View::Source(source) => source.label(),
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            View::ForMe => Icon::Inbox,
            View::Info => Icon::Bell,
            View::Snoozed => Icon::Clock,
            View::Done => Icon::CircleCheck,
            View::Source(source) => list::source_icon(source),
        }
    }
}

/// The menu that is open over the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// The app switcher at the top of the sidebar.
    Switcher,
    /// The navigation menu of the medium and narrow layouts ("For me ▾").
    Nav,
}

/// A state the window can start in (`--demo --open <state>`), so every state can be photographed
/// from a script without clicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// The panel with the first entry of "For me" (a waiting agent in the demo).
    Detail,
    /// The panel with the first approval.
    Approval,
    /// The view "Info".
    Info,
    /// The view "Snoozed".
    Snoozed,
    /// The view "Done".
    Done,
    /// The navigation menu ("For me ▾"); only the medium and narrow header have it.
    Menu,
    /// The sidebar drawer of the medium and narrow layouts.
    Drawer,
    /// The app switcher at the top of the sidebar (in the drawer where there is no sidebar).
    Switcher,
    /// The error state with its "Retry".
    Error,
    /// No entries at all.
    Empty,
    /// The loading state, kept for good.
    Loading,
}

impl Open {
    const ALL: [(&'static str, Open); 11] = [
        ("detail", Open::Detail),
        ("approval", Open::Approval),
        ("info", Open::Info),
        ("snoozed", Open::Snoozed),
        ("done", Open::Done),
        ("menu", Open::Menu),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("error", Open::Error),
        ("empty", Open::Empty),
        ("loading", Open::Loading),
    ];
}

/// The start parameters of the window:
///
/// - `--demo`: sample data ([`torro_inbox::demo::DemoSource`]) instead of the core.
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
/// - `--now <YYYY-MM-DDTHH:MM>` (local time): a fixed "now" and demo data around it, for pictures
///   that can be made again; needs `--demo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub demo: bool,
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
    pub now: Option<NaiveDateTime>,
}

/// Why the start parameters were refused; `main` prints it and exits with 64, like the other apps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// `--open` and friends without a value.
    MissingValue(&'static str),
    /// A value the flag does not know, with the list of valid ones.
    UnknownValue { flag: &'static str, value: String, valid: String },
    /// `--open` or `--now` without `--demo`: they only make sense on sample data.
    NeedsDemo(&'static str),
    /// `--now` is not a date and time like `2026-10-12T10:30`.
    BadNow(String),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::MissingValue(flag) => write!(f, "{flag} needs a value"),
            LaunchError::UnknownValue { flag, value, valid } => {
                write!(f, "unknown value {value:?} for {flag}; valid values: {valid}")
            }
            LaunchError::NeedsDemo(flag) => write!(f, "{flag} only works together with --demo"),
            LaunchError::BadNow(value) => {
                write!(f, "--now expects YYYY-MM-DDTHH:MM, e.g. 2026-10-12T10:30; got {value:?}")
            }
        }
    }
}

impl std::error::Error for LaunchError {}

/// The value of `--flag value` or `--flag=value`; `None` when the flag is not there.
fn flag_value<'a>(args: &'a [String], flag: &'static str) -> Result<Option<&'a str>, LaunchError> {
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == flag {
            return words.next().map(|value| Some(value.as_str())).ok_or(LaunchError::MissingValue(flag));
        }
        if let Some(value) = word.strip_prefix(flag).and_then(|rest| rest.strip_prefix('=')) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

impl Launch {
    /// Reads the start parameters from `args` (the words after the program name). Words it does
    /// not know are left alone.
    pub fn parse(args: &[String]) -> Result<Launch, LaunchError> {
        let demo = args.iter().any(|arg| arg == "--demo");
        let unknown = |flag, value: &str, valid: Vec<&str>| LaunchError::UnknownValue {
            flag,
            value: value.to_owned(),
            valid: valid.join(", "),
        };
        let open = match flag_value(args, "--open")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--open")),
            Some(word) => match Open::ALL.iter().find(|(name, _)| *name == word) {
                Some(&(_, open)) => Some(open),
                None => return Err(unknown("--open", word, Open::ALL.iter().map(|(name, _)| *name).collect())),
            },
        };
        let appearance = match flag_value(args, "--appearance")? {
            None => None,
            Some("light") => Some(Appearance::Light),
            Some("dark") => Some(Appearance::Dark),
            Some("system") => Some(Appearance::System),
            Some(other) => return Err(unknown("--appearance", other, vec!["light", "dark", "system"])),
        };
        let now = match flag_value(args, "--now")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--now")),
            Some(word) => Some(
                NaiveDateTime::parse_from_str(word, "%Y-%m-%dT%H:%M").map_err(|_| LaunchError::BadNow(word.into()))?,
            ),
        };
        Ok(Launch { demo, open, appearance, now })
    }

    /// "Now" at start: the fixed `--now`, else the clock. The demo data are built around it.
    pub fn start(&self) -> DateTime<Utc> {
        self.now.map_or_else(Utc::now, datetime::to_utc)
    }
}

/// The text of the forced error state.
const DEMO_ERROR: &str = "The server did not answer (demo)";

/// One answer of the source: the entries of a view, the count for "For me", the sources.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub items: Vec<Item>,
    /// Open entries in "For me", the number in the sidebar.
    pub for_me: usize,
    pub sources: Vec<SourceInfo>,
}

pub struct App<S> {
    inbox: Arc<S>,
    /// The suite behind the window: appearance, zoom, opening other apps (none with `--demo`).
    suite: Source,
    /// The start parameters; `open` also keeps the forced states (error, empty, loading) alive.
    launch: Launch,
    /// The one-time part of `launch.open` (opening a panel) has been done.
    opened: bool,
    /// How far "now" is ahead of the clock: zero, or `--now` minus the clock at start, so the
    /// times in the list match the demo data.
    offset: TimeDelta,
    now: DateTime<Utc>,
    /// The window size, for the size class ([`layout::of`]), kept from the window events.
    size: Size,
    view: View,
    items: Vec<Item>,
    for_me: usize,
    sources: Vec<SourceInfo>,
    /// Whether entries have been loaded once; until then the list shows loading (or the error).
    loaded: bool,
    error: Option<String>,
    /// The entry in the panel, as last loaded or changed. A copy, so the panel stays while the
    /// entry leaves the view (marked done in "For me"), and "Reopen" is one click away.
    detail: Option<Item>,
    appearance: Appearance,
    system_mode: Mode,
    menu: Option<Menu>,
    /// Whether the sidebar drawer is open (medium and narrow layouts).
    drawer: bool,
    /// The sidebar the user chose for the wide layout, kept in `ui.json`.
    sidebar: SidebarMode,
    /// The suite-wide zoom (Ctrl + Plus / Minus / 0), the factor of the whole window.
    zoom: Zoom,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The window has this size (at start and after every resize).
    Resized(Size),
    /// The source's answer for the view it was asked for; answers for another view are dropped.
    Loaded(View, Result<Loaded, String>),
    /// From the sidebar or the navigation menu.
    Show(View),
    /// From the list: open the entry with this id in the panel.
    Select(String),
    /// The panel's ✕, a click beside it, Escape.
    CloseDetail,
    /// From the panel: mark the entry done (`true`) or open again (`false`).
    SetDone(String, bool),
    /// The source's answer to a change: the entry as stored.
    Changed(Result<Box<Item>, String>),
    /// From the panel: open the entry's link (only what [`detail::openable`] offers).
    OpenLink(Link),
    Refresh,
    Tick,
    /// The desktop's light/dark preference (once at start, then whenever it changes).
    SystemMode(Mode),
    /// The appearance stored in the suite settings; an error keeps the one we have.
    Appearance(Result<Appearance, String>),
    /// The zoom keys, the stored zoom and its badge.
    Zoom(zoom::Message),
    /// From the app switcher at the top of the sidebar.
    Switcher(Switcher),
    /// Open or close the navigation menu ("For me ▾").
    ToggleNav,
    /// Open the sidebar drawer (narrow layout).
    ShowDrawer,
    /// The panel button at the top of the sidebar, or `[`: collapses or expands the sidebar (wide),
    /// opens or closes the drawer (medium, narrow).
    ToggleSidebar,
    /// The view state stored in `ui.json`, loaded at start.
    UiState(UiState),
    /// Close the open menu and the drawer (click beside them).
    CloseMenu,
    /// Escape: closes the innermost open thing – menu or drawer, then the panel.
    Escape,
    /// Start or raise another app of the suite ([`AppId::binary`]) through the core.
    OpenApp(String),
    /// The core's answer to [`Message::OpenApp`].
    AppOpened(Result<(), String>),
    /// Open the control centre.
    OpenSettings,
}

/// `[` toggles the sidebar, as in the other apps. Keys a widget used are not for us.
fn shortcut(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::Event as Key;
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(Key::KeyPressed { key, modifiers, .. }) = event else { return None };
    shell::is_sidebar_key(&key, modifiers).then_some(Message::ToggleSidebar)
}

/// Asks `inbox` for the entries of `view`, the count for "For me" and the sources. `empty` (the
/// forced empty state) asks nothing and answers an empty inbox.
pub async fn fetch<S: InboxSource>(inbox: Arc<S>, view: View, empty: bool) -> Result<Loaded, String> {
    if empty {
        return Ok(Loaded { items: Vec::new(), for_me: 0, sources: Vec::new() });
    }
    let failed = |err: torro_inbox::InboxError| err.to_string();
    let items = inbox.list_items(view.filter()).await.map_err(failed)?;
    let for_me = match view {
        View::ForMe => items.len(),
        _ => inbox.list_items(View::ForMe.filter()).await.map_err(failed)?.len(),
    };
    let sources = inbox.list_sources().await.map_err(failed)?;
    Ok(Loaded { items, for_me, sources })
}

impl<S: InboxSource + 'static> App<S> {
    pub fn new(inbox: Arc<S>, suite: Source, launch: Launch) -> (App<S>, Task<Message>) {
        let open = launch.open;
        let offset = launch.now.map_or(TimeDelta::zero(), |now| datetime::to_utc(now) - Utc::now());
        let app = App {
            inbox,
            zoom: Zoom::for_source(&suite),
            suite,
            opened: false,
            offset,
            now: Utc::now() + offset,
            size: torro_gui::window(INBOX).size,
            view: match open {
                Some(Open::Info) => View::Info,
                Some(Open::Snoozed) => View::Snoozed,
                Some(Open::Done) => View::Done,
                _ => View::ForMe,
            },
            items: Vec::new(),
            for_me: 0,
            sources: Vec::new(),
            loaded: false,
            error: None,
            detail: None,
            appearance: Appearance::System,
            system_mode: Mode::None,
            menu: match open {
                Some(Open::Menu) => Some(Menu::Nav),
                Some(Open::Switcher) => Some(Menu::Switcher),
                _ => None,
            },
            // Without a sidebar the app switcher lives in the drawer.
            drawer: matches!(open, Some(Open::Drawer | Open::Switcher)),
            sidebar: SidebarMode::default(),
            launch,
        };
        let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
        let task = Task::batch([
            ui_state::load_task(INBOX).map(Message::UiState),
            app.load(),
            app.fetch_appearance(),
            app.zoom.fetch().map(Message::Zoom),
            iced::system::theme().map(Message::SystemMode),
            size,
        ]);
        (app, task)
    }

    pub fn title(&self) -> String {
        trf("Inbox · {view}", &[("view", &self.view.name())])
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::time::every(REFRESH).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            self.zoom.subscription().map(Message::Zoom),
            iced::event::listen_with(shortcut),
        ];
        // Escape closes an open menu, drawer or the panel; nothing listens while none is open.
        if self.menu.is_some() || self.drawer || self.detail.is_some() {
            subscriptions.push(widgets::escape().map(|()| Message::Escape));
        }
        Subscription::batch(subscriptions)
    }

    /// The Iced theme: the appearance of `--appearance`, else the stored one, with "System"
    /// following the desktop.
    pub fn theme(&self) -> Theme {
        theme::for_appearance(self.launch.appearance.unwrap_or(self.appearance), self.system_mode)
    }

    /// The factor the whole window draws at (`.scale_factor` in `main`): the suite-wide zoom.
    pub fn scale_factor(&self) -> f32 {
        self.zoom.factor()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Resized(size) => self.size = size,
            Message::Loaded(view, result) => {
                // An answer for a view the user has left since would show the wrong entries.
                if view != self.view {
                    return Task::none();
                }
                match result {
                    Ok(loaded) => {
                        self.items = loaded.items;
                        self.for_me = loaded.for_me;
                        self.sources = loaded.sources;
                        self.loaded = true;
                        self.error = None;
                        // The panel follows the entry when it changed (a new report, woken up).
                        if let Some(detail) = self.detail.as_mut()
                            && let Some(fresh) = self.items.iter().find(|item| item.id == detail.id)
                        {
                            *detail = fresh.clone();
                        }
                        self.open_launched();
                    }
                    Err(err) => self.error = Some(load_failed(&err)),
                }
            }
            Message::Show(view) => {
                self.close_menus();
                self.detail = None;
                if view != self.view {
                    self.view = view;
                    return self.load();
                }
            }
            Message::Select(id) => {
                self.close_menus();
                self.detail = self.items.iter().find(|item| item.id == id).cloned();
            }
            Message::CloseDetail => self.detail = None,
            Message::SetDone(id, done) => {
                let inbox = self.inbox.clone();
                return Task::perform(
                    async move { inbox.set_item_done(id, done).await.map(Box::new).map_err(|err| err.to_string()) },
                    Message::Changed,
                );
            }
            Message::Changed(Ok(item)) => {
                if let Some(detail) = self.detail.as_mut().filter(|detail| detail.id == item.id) {
                    *detail = *item;
                }
                return self.load();
            }
            Message::Changed(Err(err)) => {
                self.error = Some(trf("Could not change the entry: {error}", &[("error", &err)]));
            }
            Message::OpenLink(link) => return self.open_link(link),
            Message::Refresh => return self.load(),
            Message::Tick => {
                self.now = Utc::now() + self.offset;
                return Task::batch([self.fetch_appearance(), self.zoom.fetch().map(Message::Zoom), self.load()]);
            }
            Message::SystemMode(mode) => self.system_mode = mode,
            Message::Appearance(Ok(appearance)) => self.appearance = appearance,
            // The core may be starting up; keep the colours we have.
            Message::Appearance(Err(_)) => {}
            Message::Zoom(message) => {
                let before = self.zoom.factor();
                let task = self.zoom.update(message).map(Message::Zoom);
                if self.zoom.factor() == before {
                    return task;
                }
                // The window keeps its pixels, so its logical size changes with the zoom: at once
                // by arithmetic, then exactly from the window.
                self.size = zoom::rescale(self.size, before, self.zoom.factor());
                let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
                return Task::batch([task, size]);
            }
            Message::Switcher(Switcher::Toggle) => self.toggle_menu(Menu::Switcher),
            Message::Switcher(Switcher::OpenApp(app)) => return self.update(Message::OpenApp(app.to_owned())),
            // The inbox has nothing to create and no search yet; the buttons only close what is open.
            Message::Switcher(Switcher::New | Switcher::Search) => self.close_menus(),
            Message::ToggleNav => self.toggle_menu(Menu::Nav),
            Message::ShowDrawer => {
                self.menu = None;
                self.drawer = true;
            }
            Message::ToggleSidebar => return self.toggle_sidebar(),
            Message::UiState(state) => self.sidebar = state.sidebar,
            Message::CloseMenu => self.close_menus(),
            Message::Escape => {
                if self.menu.is_some() || self.drawer {
                    self.close_menus();
                } else {
                    self.detail = None;
                }
            }
            Message::OpenApp(app) => {
                self.close_menus();
                let suite = self.suite.clone();
                return Task::perform(async move { suite.open_app(&app).await }, Message::AppOpened);
            }
            Message::AppOpened(Ok(())) => {}
            Message::AppOpened(Err(err)) => {
                self.error = Some(trf("Could not open the app: {error}", &[("error", &err)]));
            }
            Message::OpenSettings => {
                self.close_menus();
                connect::open_settings();
            }
        }
        Task::none()
    }

    /// Opens `link` if the panel offers it: a web page in the browser, an app of the suite through
    /// the core. The panel only offers what [`detail::openable`] allows; this checks again, because
    /// a message can outlive the entry it came from.
    fn open_link(&mut self, link: Link) -> Task<Message> {
        let offered = self.detail.as_ref().and_then(detail::openable);
        if offered.as_ref() != Some(&link) {
            return Task::none();
        }
        match link {
            Link::Url { url } => {
                // Like the other apps: a browser that cannot be started is only logged.
                if let Err(err) = webbrowser::open(&url) {
                    eprintln!("cannot open the link in a browser: {err}");
                }
                Task::none()
            }
            Link::App { app } if app == AppId::WORKPLACE.binary => self.update(Message::OpenSettings),
            Link::App { app } => self.update(Message::OpenApp(app)),
            Link::Window => Task::none(),
        }
    }

    /// The panel button and `[`: in the wide layout the other sidebar mode, remembered for the
    /// next start (not in the demo, which stores nothing); else the drawer opens or closes.
    fn toggle_sidebar(&mut self) -> Task<Message> {
        if !shell::has_sidebar(self.class()) {
            self.menu = None;
            self.drawer = !self.drawer;
            return Task::none();
        }
        self.close_menus();
        self.sidebar = self.sidebar.toggled();
        if self.launch.demo {
            return Task::none();
        }
        ui_state::save_task(INBOX, UiState { sidebar: self.sidebar })
    }

    /// The layout class of the window.
    fn class(&self) -> SizeClass {
        layout::of(self.size)
    }

    /// The window with the zoom badge over it.
    pub fn view(&self) -> Element<'_, Message> {
        widgets::ctrl_guard(self.zoom.view(self.window(self.class())))
    }

    /// The whole window in the layout of `class`. Mini and strip have no glance before E3.3; the
    /// shell draws them like the narrow layout.
    fn window(&self, class: SizeClass) -> Element<'_, Message> {
        let (switcher, nav) = (Anchor::new(), Anchor::new());
        let sidebar = self.sidebar(SidebarMode::Expanded, &switcher);
        let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
            .then(|| self.sidebar(SidebarMode::Rail, &switcher));
        let header = self.header(class, &nav);
        let menu = self.menu.map(|menu| match menu {
            Menu::Switcher => widgets::floating(
                &switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(INBOX, Message::Switcher),
                Message::CloseMenu,
            ),
            Menu::Nav => widgets::floating(&nav, Placement::Below(Align::Start), self.nav_menu(), Message::CloseMenu),
        });
        let detail = self.detail.as_ref().map(|item| detail::panel(item, self.view.name(), self.now));
        let placed = panel::place(class, self.body(), detail, menu);
        shell::shell(class, sidebar, header, placed.body, None)
            .rail(rail)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(placed.overlay)
            .into()
    }

    /// The header: the view's title (wide) or the navigation menu (medium, narrow), its count, and
    /// "Refresh". The inbox creates nothing, so there is no primary button.
    fn header<'a>(&'a self, class: SizeClass, nav: &Anchor) -> Element<'a, Message> {
        let (name, icon) = (self.view.name(), self.view.icon());
        // Nothing listed: the empty state says so, a "0" beside the title would only repeat it.
        let note = (self.loaded && !self.items.is_empty()).then(|| self.count_note());
        let trailing = [widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::Refresh))];
        match class {
            SizeClass::Wide => shell::header_bar([], shell::header_title(Some(icon), name, note), trailing),
            SizeClass::Medium => {
                let mut title = row![widgets::menu_button(nav, Some(icon), name, Message::ToggleNav)]
                    .spacing(space::S)
                    .align_y(Alignment::Center);
                if let Some(note) = note {
                    title = title.push(typo::text(typo::BODY, note).style(theme::text_3).wrapping(Wrapping::None));
                }
                // The rail at the left has the panel button.
                shell::header_bar([], title, trailing)
            }
            SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => shell::header_bar(
                [shell::sidebar_toggle(Message::ShowDrawer)],
                widgets::menu_button(nav, Some(icon), name, Message::ToggleNav),
                trailing,
            ),
        }
    }

    /// The quiet number after the title: "6 for you" in "For me", the number of entries elsewhere.
    fn count_note(&self) -> String {
        match self.view {
            View::ForMe => trf("{n} for you", &[("n", &self.items.len())]),
            _ => self.items.len().to_string(),
        }
    }

    /// The content below the header: an error line if there is one, and the list at most
    /// `CONTENT_MAX` wide; loading and the error of a first load stand in for both.
    fn body(&self) -> Element<'_, Message> {
        if !self.loaded {
            return match &self.error {
                Some(err) => widgets::error_state(err.as_str(), Message::Refresh),
                None => widgets::loading(),
            };
        }
        let data = list::ListData {
            items: &self.items,
            view: &self.view,
            selected: self.detail.as_ref().map(|item| item.id.as_str()),
            now: self.now,
        };
        let mut content = column![];
        if let Some(err) = &self.error {
            let line = typo::text(typo::META, err.clone())
                .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).error_text) });
            content = content.push(container(line).padding(Padding::new(space::S).left(space::L)));
        }
        widgets::content(size::CONTENT_MAX, content.push(list::view(data)).width(Fill).height(Fill))
    }

    /// The views with their icon (and the count for "For me"), then one entry per source.
    fn nav_entries(&self) -> (Vec<NavEntry>, Vec<NavEntry>) {
        let entry = |view: View, count: Option<usize>| NavEntry {
            mark: Mark::Icon(view.icon()),
            label: view.name(),
            count,
            active: view == self.view,
            view,
        };
        let views = vec![
            // Like a badge: no "0" when nothing waits.
            entry(View::ForMe, Some(self.for_me).filter(|&n| n > 0)),
            entry(View::Info, None),
            entry(View::Snoozed, None),
            entry(View::Done, None),
        ];
        let sources = self
            .sources
            .iter()
            .map(|info| {
                let view = View::Source(info.source.clone());
                let label = if info.muted {
                    trf("{source} (muted)", &[("source", &info.source.label())])
                } else {
                    info.source.label()
                };
                let count = usize::try_from(info.open).ok().filter(|&open| open > 0);
                NavEntry { mark: Mark::Icon(view.icon()), label, count, active: view == self.view, view }
            })
            .collect();
        (views, sources)
    }

    /// The sidebar in `mode`: expanded (wide, drawer) or as the rail of icons.
    fn sidebar<'a>(&'a self, mode: SidebarMode, switcher: &Anchor) -> Element<'a, Message> {
        let (views, sources) = self.nav_entries();
        let row = |e: NavEntry| {
            let trail = e.trail();
            widgets::sidebar_item_in(mode, e.mark, e.label, trail, e.active, Message::Show(e.view))
        };
        let mut items: Vec<Element<'a, Message>> = views.into_iter().map(row).collect();
        if !sources.is_empty() {
            items.push(widgets::sidebar_group_in(mode, tr("Sources"), None));
            items.extend(sources.into_iter().map(row));
        }
        let footer = vec![widgets::sidebar_item_in(
            mode,
            Icon::Settings,
            tr("Settings"),
            Trail::None,
            false,
            Message::OpenSettings,
        )];
        let head = shell::sidebar_head(mode, INBOX, switcher, Message::Switcher, Message::ToggleSidebar);
        shell::sidebar(head, items, footer)
    }

    /// The navigation menu of the medium and narrow layouts: the sidebar's rows as a menu.
    fn nav_menu(&self) -> Element<'_, Message> {
        let (views, sources) = self.nav_entries();
        let item = |e: NavEntry| {
            let trail = e.trail();
            MenuItem::new(e.mark, e.label, Message::Show(e.view)).trail(trail).current(e.active)
        };
        let mut sections = vec![MenuSection::new(views.into_iter().map(item).collect())];
        if !sources.is_empty() {
            sections.push(MenuSection::titled(tr("Sources"), sources.into_iter().map(item).collect()));
        }
        sections.push(MenuSection::new(vec![MenuItem::new(Icon::Settings, tr("Settings"), Message::OpenSettings)]));
        widgets::menu(sections)
    }

    fn toggle_menu(&mut self, menu: Menu) {
        self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
    }

    fn close_menus(&mut self) {
        self.menu = None;
        self.drawer = false;
    }

    /// Asks the core for the stored appearance, on every tick, so a change in the control centre
    /// reaches the open window.
    fn fetch_appearance(&self) -> Task<Message> {
        let suite = self.suite.clone();
        Task::perform(async move { theme::stored_appearance(&suite).await }, Message::Appearance)
    }

    /// The panel states of `--open`, once the first entries are in: a panel needs an entry.
    fn open_launched(&mut self) {
        if std::mem::replace(&mut self.opened, true) {
            return;
        }
        let wanted: fn(&Item) -> bool = match self.launch.open {
            Some(Open::Detail) => |_| true,
            Some(Open::Approval) => detail::is_approval,
            _ => return,
        };
        self.detail = self.items.iter().find(|item| wanted(item)).cloned();
    }

    fn load(&self) -> Task<Message> {
        // The forced states: loading never ends, the error comes at once, empty has no entries.
        let view = self.view.clone();
        match self.launch.open {
            Some(Open::Loading) => return Task::none(),
            Some(Open::Error) => return Task::done(Message::Loaded(view, Err(DEMO_ERROR.into()))),
            _ => {}
        }
        let empty = self.launch.open == Some(Open::Empty);
        let inbox = self.inbox.clone();
        Task::perform(fetch(inbox, view.clone(), empty), move |result| Message::Loaded(view.clone(), result))
    }
}

/// A row of the sidebar and the navigation menu.
struct NavEntry {
    mark: Mark,
    label: String,
    view: View,
    /// Shown as counter where there is one.
    count: Option<usize>,
    active: bool,
}

impl NavEntry {
    fn trail(&self) -> Trail {
        self.count.map_or(Trail::None, Trail::Count)
    }
}

/// The error text when the entries could not be loaded; the error state's "Retry" is what the
/// user can do about it.
fn load_failed(err: &str) -> String {
    trf("The inbox could not be loaded: {error}", &[("error", &err)])
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use iced::futures::executor::block_on;
    use torro_inbox::State;
    use torro_inbox::demo::DemoSource;

    use super::*;
    use crate::empty::EmptySource;

    const WIDE: Size = Size::new(1600.0, 1000.0);
    const MEDIUM: Size = Size::new(800.0, 600.0);
    const NARROW: Size = Size::new(480.0, 900.0);
    const SIZES: [Size; 6] =
        [Size::new(240.0, 160.0), Size::new(320.0, 480.0), Size::new(1200.0, 200.0), NARROW, MEDIUM, WIDE];

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn demo(words: &[&str]) -> App<DemoSource> {
        let launch = Launch::parse(&args(words)).expect("valid parameters");
        let inbox = Arc::new(DemoSource::new(launch.start()));
        App::new(inbox, Source::from_args(args(&["--demo"])), launch).0
    }

    /// What the source answers for the app's view, handed to `update` as the task would.
    fn load<S: InboxSource + 'static>(app: &mut App<S>) {
        let empty = app.launch.open == Some(Open::Empty);
        let result = block_on(fetch(app.inbox.clone(), app.view.clone(), empty));
        let _ = app.update(Message::Loaded(app.view.clone(), result));
    }

    /// A demo app at the demo's fixed time, loaded.
    fn loaded(words: &[&str]) -> App<DemoSource> {
        let mut all = vec!["--demo", "--now", "2026-10-13T10:30"];
        all.extend_from_slice(words);
        let mut app = demo(&all);
        load(&mut app);
        app
    }

    fn ids<S>(app: &App<S>) -> Vec<&str> {
        app.items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn launch_parameters_need_the_demo_and_known_values() {
        assert_eq!(Launch::parse(&args(&["--open", "detail"])), Err(LaunchError::NeedsDemo("--open")));
        assert_eq!(Launch::parse(&args(&["--now", "2026-10-13T10:30"])), Err(LaunchError::NeedsDemo("--now")));
        assert!(matches!(
            Launch::parse(&args(&["--demo", "--open", "nope"])),
            Err(LaunchError::UnknownValue { flag: "--open", .. })
        ));
        assert_eq!(Launch::parse(&args(&["--demo", "--now", "today"])), Err(LaunchError::BadNow("today".into())));
        assert_eq!(Launch::parse(&args(&["--demo", "--open"])), Err(LaunchError::MissingValue("--open")));
        let launch = Launch::parse(&args(&["--demo", "--open=approval", "--appearance", "light"])).expect("valid");
        assert_eq!(
            (launch.demo, launch.open, launch.appearance),
            (true, Some(Open::Approval), Some(Appearance::Light))
        );
        assert!(!Launch::parse(&args(&[])).expect("valid").demo);
    }

    #[test]
    fn a_fixed_now_starts_the_demo_there() {
        let launch = Launch::parse(&args(&["--demo", "--now", "2026-10-13T10:30"])).expect("valid");
        let local = chrono::Local.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid");
        assert_eq!(launch.start(), local.to_utc());
    }

    #[test]
    fn views_ask_for_their_entries() {
        let open = Some(StateFilter::Open);
        assert_eq!(View::ForMe.filter(), ItemFilter { lane: Some(Lane::ForMe), state: open, ..ItemFilter::default() });
        assert_eq!(View::Info.filter(), ItemFilter { lane: Some(Lane::Info), state: open, ..ItemFilter::default() });
        assert_eq!(View::Snoozed.filter().state, Some(StateFilter::Snoozed));
        assert_eq!(View::Done.filter().state, Some(StateFilter::Done));
        let codex = View::Source(SourceRef::agent("codex")).filter();
        assert_eq!((codex.lane, codex.source, codex.state), (None, Some(SourceRef::agent("codex")), open));
        assert_eq!(View::ForMe.name(), tr("For me"));
        assert_eq!(View::Source(SourceRef::agent("claude-code")).name(), "Claude Code");
    }

    #[test]
    fn for_me_shows_urgent_entries_first_and_counts_them() {
        let app = loaded(&[]);
        assert_eq!(ids(&app), ["demo-1", "demo-3", "demo-2", "demo-4", "demo-6", "demo-5"]);
        assert_eq!(app.for_me, 6);
        assert_eq!(app.count_note(), trf("{n} for you", &[("n", &6)]));
        assert_eq!(app.sources.len(), 7, "suite, two agents, four scripts");
        assert_eq!(app.title(), trf("Inbox · {view}", &[("view", &tr("For me"))]));
    }

    #[test]
    fn other_views_keep_the_for_me_count() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Show(View::Info));
        load(&mut app);
        assert_eq!(ids(&app), ["demo-8", "demo-9", "demo-7"]);
        assert_eq!(app.for_me, 6, "the sidebar still counts For me");
        assert_eq!(app.count_note(), "3");
        let _ = app.update(Message::Show(View::Source(SourceRef::suite())));
        load(&mut app);
        assert_eq!(ids(&app), ["demo-3", "demo-5"]);
    }

    #[test]
    fn an_answer_for_a_view_left_behind_is_dropped() {
        let mut app = loaded(&[]);
        let for_me = block_on(fetch(app.inbox.clone(), View::ForMe, false));
        let _ = app.update(Message::Show(View::Done));
        let _ = app.update(Message::Loaded(View::ForMe, for_me));
        assert_eq!(ids(&app).len(), 6, "still the old list, not taken for Done");
        load(&mut app);
        assert_eq!(ids(&app), ["demo-11"]);
    }

    #[test]
    fn selecting_opens_the_panel_and_a_view_closes_it() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Select("demo-2".into()));
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-2"));
        let _ = app.update(Message::Select("gone".into()));
        assert!(app.detail.is_none(), "an unknown id opens nothing");
        let _ = app.update(Message::Select("demo-2".into()));
        let _ = app.update(Message::Show(View::Info));
        assert!(app.detail.is_none());
    }

    #[test]
    fn escape_closes_menu_and_drawer_before_the_panel() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Select("demo-1".into()));
        let _ = app.update(Message::ToggleNav);
        let _ = app.update(Message::Escape);
        assert!(app.menu.is_none() && app.detail.is_some(), "the menu first");
        let _ = app.update(Message::ShowDrawer);
        let _ = app.update(Message::Escape);
        assert!(!app.drawer && app.detail.is_some(), "then the drawer");
        let _ = app.update(Message::Escape);
        assert!(app.detail.is_none(), "then the panel");
        let _ = app.update(Message::Select("demo-1".into()));
        let _ = app.update(Message::CloseDetail);
        assert!(app.detail.is_none());
    }

    #[test]
    fn marking_done_keeps_the_panel_and_takes_the_entry_off_the_list() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Select("demo-6".into()));
        // What the task of SetDone does, run here.
        let changed = block_on(app.inbox.set_item_done("demo-6".into(), true)).map(Box::new).map_err(|e| e.to_string());
        let _ = app.update(Message::Changed(changed));
        assert!(
            matches!(app.detail.as_ref().map(|item| item.state), Some(State::Done { .. })),
            "Reopen is one click away"
        );
        load(&mut app);
        assert!(!ids(&app).contains(&"demo-6"));
        assert_eq!(app.for_me, 5);
        let reopened =
            block_on(app.inbox.set_item_done("demo-6".into(), false)).map(Box::new).map_err(|e| e.to_string());
        let _ = app.update(Message::Changed(reopened));
        load(&mut app);
        assert!(ids(&app).contains(&"demo-6"));
    }

    #[test]
    fn a_failed_change_says_so() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Changed(Err("boom".into())));
        assert_eq!(app.error, Some(trf("Could not change the entry: {error}", &[("error", &"boom")])));
        load(&mut app);
        assert_eq!(app.error, None, "the next load clears it");
    }

    #[test]
    fn launch_states_open_their_view_or_panel() {
        let app = loaded(&["--open", "detail"]);
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-1"));
        let app = loaded(&["--open", "approval"]);
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-3"));
        let app = loaded(&["--open", "snoozed"]);
        assert_eq!((app.view.clone(), ids(&app)), (View::Snoozed, vec!["demo-10"]));
        let app = loaded(&["--open", "empty"]);
        assert!(app.loaded && app.items.is_empty() && app.sources.is_empty());
        let mut app = demo(&["--demo", "--open", "loading"]);
        let _ = app.update(Message::Tick);
        assert!(!app.loaded, "loading stays");
        let mut app = demo(&["--demo", "--open", "error"]);
        let _ = app.update(Message::Loaded(View::ForMe, Err(DEMO_ERROR.into())));
        assert_eq!(app.error, Some(load_failed(DEMO_ERROR)));
        assert_eq!(demo(&["--demo", "--open", "menu"]).menu, Some(Menu::Nav));
        assert!(demo(&["--demo", "--open", "drawer"]).drawer);
    }

    #[test]
    fn without_the_demo_the_inbox_is_empty() {
        let launch = Launch::default();
        let mut app = App::new(Arc::new(EmptySource), Source::from_args(args(&["--demo"])), launch).0;
        load(&mut app);
        assert!(app.loaded && app.items.is_empty() && app.for_me == 0);
        assert_eq!(app.nav_entries().0[0].trail(), Trail::None, "no 0 beside For me");
        assert_eq!(list::empty_text(&app.view).0, tr("Nothing for you right now."));
        let _ = app.view();
    }

    #[test]
    fn the_sidebar_lists_views_then_sources() {
        let mut app = loaded(&[]);
        let (views, sources) = app.nav_entries();
        let labels: Vec<&str> = views.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, [tr("For me"), tr("Info"), tr("Snoozed"), tr("Done")]);
        assert_eq!(
            views.iter().map(NavEntry::trail).collect::<Vec<_>>()[..2],
            [Trail::Count(6), Trail::None],
            "only For me counts"
        );
        assert_eq!(sources.first().map(|e| e.label.clone()), Some(tr("Suite").to_owned()));
        assert_eq!(sources.first().map(NavEntry::trail), Some(Trail::Count(2)));
        block_on(app.inbox.mute_source(SourceRef::script("backup"), true)).expect("mute");
        load(&mut app);
        let (_, sources) = app.nav_entries();
        assert!(sources.iter().any(|e| e.label == trf("{source} (muted)", &[("source", &"backup")])));
    }

    #[test]
    fn the_sidebar_collapses_in_wide_and_opens_the_drawer_elsewhere() {
        let mut app = loaded(&[]);
        assert_eq!(app.sidebar, SidebarMode::Expanded, "expanded until ui.json says otherwise");
        let _ = app.update(Message::UiState(UiState { sidebar: SidebarMode::Rail }));
        let _ = app.update(Message::Resized(WIDE));
        let _ = app.update(Message::ToggleSidebar);
        assert_eq!((app.sidebar, app.drawer), (SidebarMode::Expanded, false), "wide: the choice flips");
        for size in [MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            let _ = app.update(Message::ToggleSidebar);
            assert!(app.drawer, "{size:?}: the panel button opens the drawer");
            let _ = app.update(Message::ToggleSidebar);
            assert!(!app.drawer, "{size:?}: and closes it again");
            assert_eq!(app.sidebar, SidebarMode::Expanded, "{size:?}: the wide choice stays");
        }
    }

    #[test]
    fn menus_toggle_and_a_view_closes_them() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::ToggleNav);
        assert_eq!(app.menu, Some(Menu::Nav));
        let _ = app.update(Message::ToggleNav);
        assert_eq!(app.menu, None, "a second click closes the menu");
        let _ = app.update(Message::Switcher(Switcher::Toggle));
        assert_eq!(app.menu, Some(Menu::Switcher));
        let _ = app.update(Message::ShowDrawer);
        assert!(app.drawer && app.menu.is_none(), "the drawer replaces an open menu");
        let _ = app.update(Message::Show(View::Done));
        assert!(!app.drawer && app.menu.is_none());
        assert_eq!(app.view, View::Done);
    }

    #[test]
    fn links_open_only_from_the_open_panel() {
        let mut app = loaded(&[]);
        let link = Link::Url { url: "https://ci.example.org/runs/1842".into() };
        let _ = app.update(Message::OpenLink(link.clone()));
        assert!(app.detail.is_none(), "nothing open, nothing happens");
        let _ = app.update(Message::Select("demo-1".into()));
        assert_eq!(app.open_link(link).units(), 0, "not the link of the open entry");
    }

    #[test]
    fn the_window_builds_in_every_size_with_panel_menus_and_drawer() {
        for open in [&[][..], &["--open", "detail"], &["--open", "approval"], &["--open", "done"], &["--open", "empty"]]
        {
            let mut app = loaded(open);
            for sidebar in [SidebarMode::Expanded, SidebarMode::Rail] {
                let _ = app.update(Message::UiState(UiState { sidebar }));
                for size in SIZES {
                    let _ = app.update(Message::Resized(size));
                    let _ = app.view();
                    let _ = app.update(Message::ToggleNav);
                    let _ = app.view();
                    let _ = app.update(Message::ShowDrawer);
                    let _ = app.view();
                    let _ = app.update(Message::CloseMenu);
                }
            }
        }
        let mut unloaded = demo(&["--demo", "--open", "loading"]);
        let _ = unloaded.view();
        let _ = unloaded.update(Message::Loaded(View::ForMe, Err("x".into())));
        let _ = unloaded.view();
    }

    #[test]
    fn theme_follows_appearance_and_desktop() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::SystemMode(Mode::Light));
        assert_eq!(app.theme(), theme::light(), "System follows the desktop");
        let _ = app.update(Message::Appearance(Ok(Appearance::Dark)));
        assert_eq!(app.theme(), theme::theme());
        let _ = app.update(Message::Appearance(Err("core is not running".into())));
        assert_eq!(app.theme(), theme::theme(), "a failed request keeps the appearance");
        let forced = loaded(&["--appearance", "light"]);
        assert_eq!(forced.theme(), theme::light());
    }
}
