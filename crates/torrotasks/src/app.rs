//! The tasks window: state, loading, quick add, saving, and the Linear window around it (task
//! T.1): `torro_gui::shell` with the sidebar (filters, lists, "Open Calendar", "Settings") in the
//! wide layout and a navigation menu ("Today ▾") plus drawer in the medium and narrow ones. In the
//! wide layout the sidebar collapses to a rail of icons (U.2: panel button or `[`, remembered in
//! `ui.json`); the medium layout always shows that rail. The
//! list (`list.rs`) and the detail panel (`detail.rs`) only render and report clicks as
//! [`Message`]s.
//!
//! The detail (T.4, U.5) is the side panel of `torro_gui::panel`: beside the list in the wide
//! layout (where it takes the place of the properties column), over the list in the medium and
//! narrow ones (full width in narrow); the list keeps its width. It has no Save button: it hands
//! out saves as it changes, and while title or notes wait to be saved a one-second timer sends
//! `Flush` ([`AUTOSAVE`]).
//!
//! The header (T.2) has one primary button, "New task", that focuses the quick add row (the same
//! message as "New" at the app switcher); "Show completed" lives in the small Display menu. There
//! is no filter function yet, so no "Filter" button.
//!
//! The window size picks the layout ([`layout::of`], design skill section 6): mini and strip are
//! glances without any frame (`glance.rs`: the numbers "3 overdue · 5 today" and the next open
//! tasks, to be ticked off and nothing else); narrow is the list with "Today ▾" in the header and
//! the detail over the list; medium and wide are as described above. The size is kept in the
//! state ([`Message::Resized`]), so the keys can be switched off in a glance; filter, selection,
//! open detail and quick add text survive every change of size.
//!
//! The keyboard (T.3): ↑/↓ move a cursor through the rows as the list shows them (groups and
//! subtasks included), Enter opens the task under it, Space ticks it, N focuses the quick add
//! row, Escape closes menu, drawer or detail (see [`Message::Escape`]). The cursor is the
//! highlighted row; the list scrolls to keep it in view. Plain keys are ignored while a text
//! field has the focus ([`shortcut`], [`typing`]) or a menu is open.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, NaiveDate, NaiveDateTime};
use iced::theme::Mode;
use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{Space, column, container, row, stack, svg, text, text_input};
use iced::{Alignment, Color, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::connect::{self, Connect};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::panel;
use torro_gui::shell::{self, SidebarMode, Switcher};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail, collection_color};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{style, typo};
use torro_ipc::{Request, Response};
use torro_pim::demo::DemoSource;
use torro_pim::{AccountState, Collection, EventTime, PimAccount, PimSource, Todo};

use crate::detail::{self, Detail};
use crate::glance::{self, GlanceData};
use crate::list::{self, ListData};

/// Which tasks the list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// Due today or overdue, from all lists.
    Today,
    /// With a due date, from all lists.
    Planned,
    /// Everything.
    All,
    /// One list ([`Collection::id`]).
    List(String),
}

/// The menu that is open over the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// The app switcher at the top of the sidebar.
    Switcher,
    /// The navigation menu of the medium and narrow layouts ("Today ▾").
    Nav,
    /// The Display menu in the header ("Show completed").
    Display,
}

/// A state the window can start in (`--demo --open <state>`), so that every state can be
/// photographed from a script without clicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// The detail of the first task in the list.
    Detail,
    /// The navigation menu ("Today ▾"); only the medium and narrow header have it.
    Menu,
    /// The Display menu in the header.
    Display,
    /// The sidebar drawer of the medium and narrow layouts.
    Drawer,
    /// The app switcher at the top of the sidebar (in the drawer where there is no sidebar).
    Switcher,
    /// The detail of the first task with its due date card open.
    Due,
    /// The error state with its "Retry".
    Error,
    /// No tasks at all.
    Empty,
    /// The loading state, kept for good.
    Loading,
}

impl Open {
    const ALL: [(&'static str, Open); 9] = [
        ("detail", Open::Detail),
        ("menu", Open::Menu),
        ("display", Open::Display),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("due", Open::Due),
        ("error", Open::Error),
        ("empty", Open::Empty),
        ("loading", Open::Loading),
    ];
}

/// The start parameters of the window besides `--demo` (which [`Source`] reads):
///
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
/// - `--now <YYYY-MM-DDTHH:MM>`: a fixed "now" (only its day counts here) and demo data around
///   it, for pictures that can be made again; needs `--demo`. The detail panel still reads the
///   clock itself (`detail.rs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
    pub now: Option<NaiveDateTime>,
}

/// Why the start parameters were refused; `main` prints it and exits with 64, as `ctl` does.
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
    /// not know are left to others (`--demo`).
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
        Ok(Launch { open, appearance, now })
    }

    /// The data source: demo data around the fixed day with `--now`, else what
    /// [`Source::from_args`] makes of `args`.
    pub fn source(&self, args: &[String]) -> Source {
        match self.now {
            Some(now) => Source::Demo(Arc::new(DemoSource::new(now.date()))),
            None => Source::from_args(args.iter().cloned()),
        }
    }
}

/// The text of the forced error state.
const DEMO_ERROR: &str = "The server did not answer (demo)";

/// The id of the quick add field, so "New" can focus it.
const QUICK_ADD: &str = "quick-add";
/// Where the quick add text starts: past the "+" icon, which sits 12 in from the edge.
const QUICK_ADD_START: f32 = space::M + icon::SIZE_SM + space::S;
/// Where the quick add text ends: before the key hint.
const QUICK_ADD_END: f32 = space::XXL + space::S;
/// How long after the last keystroke typed title or notes are saved.
const AUTOSAVE: Duration = Duration::from_secs(1);

pub struct App {
    source: Source,
    /// The start parameters; `open` also keeps the forced states (error, empty, loading) alive.
    launch: Launch,
    /// The one-time part of `launch.open` (the detail) has been done.
    opened: bool,
    today: NaiveDate,
    /// The window size, for the size class ([`layout::of`]). Kept from the window events rather
    /// than asked of the view, so that subscriptions and tests know the class too.
    size: Size,
    account: Option<AccountState>,
    /// Every account behind the data, for grouping the lists in the sidebar (task M1.3).
    accounts: Vec<PimAccount>,
    /// Accounts whose lists the sidebar has folded away. Window state, nothing is stored.
    folded: HashSet<String>,
    connect: Connect,
    collections: Vec<Collection>,
    todos: Vec<Todo>,
    filter: Filter,
    show_completed: bool,
    /// Whether tasks have been loaded once; until then the list shows loading (or the error).
    loaded: bool,
    quick_add: String,
    /// Index into `todos` of the task open in the detail panel.
    selected: Option<usize>,
    /// The uid of the task under the keyboard cursor: the highlighted row. A uid, not an index,
    /// so it survives reloads; clicking a row or opening a task moves it there.
    cursor: Option<String>,
    /// The list's scroll offset and visible height, as last reported; none until it scrolls.
    scroll: Option<(f32, f32)>,
    detail: Option<Detail>,
    error: Option<String>,
    appearance: Appearance,
    system_mode: Mode,
    /// The open menu, if any.
    menu: Option<Menu>,
    /// Whether the sidebar drawer is open (medium and narrow layouts).
    drawer: bool,
    /// The sidebar the user chose for the wide layout, kept in `ui.json`.
    sidebar: SidebarMode,
    /// The suite-wide zoom (Ctrl + Plus / Minus / 0), the factor of the whole window.
    zoom: Zoom,
}

#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "a message carries a whole task and is handled once; boxing would only add an allocation"
)]
pub enum Message {
    /// The window has this size (at start and after every resize).
    Resized(Size),
    AccountLoaded(Result<Vec<PimAccount>, String>),
    /// An account's title in the sidebar: fold or unfold its lists.
    ToggleAccount(String),
    Connect(connect::Message),
    Loaded(Result<(Vec<Collection>, Vec<Todo>), String>),
    SetFilter(Filter),
    ShowCompleted(bool),
    QuickAddChanged(String),
    QuickAddSubmit,
    /// From the list: tick or untick `todos[index]`.
    ToggleDone(usize),
    /// From the list: open `todos[index]` in the detail panel.
    Select(usize),
    Detail(detail::Message),
    Saved(Result<Todo, String>),
    Deleted(Result<(), String>),
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
    /// Open or close the navigation menu ("Today ▾").
    ToggleNav,
    /// Open or close the Display menu in the header.
    ToggleDisplay,
    /// Open the sidebar drawer (medium and narrow layouts).
    ShowDrawer,
    /// The panel button at the top of the sidebar, or `[`: collapses or expands the sidebar (wide),
    /// opens or closes the drawer (medium, narrow).
    ToggleSidebar,
    /// The view state stored in `ui.json`, loaded at start.
    UiState(UiState),
    /// Close the open menu and the drawer (click beside them).
    CloseMenu,
    /// Escape: closes the innermost open thing – menu or drawer, the detail's editor, the detail.
    Escape,
    /// Start or raise another app of the suite ([`AppId::binary`]) through the core.
    OpenApp(&'static str),
    /// The core's answer to [`Message::OpenApp`].
    AppOpened(Result<(), String>),
    /// Open the control centre.
    OpenSettings,
    /// The list scrolled (or was laid out): its offset and visible height.
    Scrolled(f32, f32),
    /// A key from [`shortcut`]; whether a text field is being typed in is checked first.
    Shortcut(Shortcut),
    /// The answer of that check: `true` if a text field has the focus, which swallows the key.
    ShortcutChecked(Shortcut, bool),
}

/// What a plain key does in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    /// ↑: the previous row.
    Up,
    /// ↓: the next row.
    Down,
    /// Enter: open the task under the cursor.
    Open,
    /// Space: tick or untick the task under the cursor.
    Toggle,
    /// N: a new task, in the quick add row.
    New,
    /// `[`: collapse or expand the sidebar ([`Message::ToggleSidebar`]).
    Sidebar,
}

/// Keyboard shortcuts of the list, like the calendar's: keys a widget already used (typing in a
/// text field, Enter in quick add) are ignored, and so is anything with Ctrl, Alt or the logo key.
fn shortcut(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event as Key, Key as Name, key::Named};
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(Key::KeyPressed { key, modifiers, .. }) = event else { return None };
    if shell::is_sidebar_key(&key, modifiers) {
        return Some(Message::Shortcut(Shortcut::Sidebar));
    }
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    let shortcut = match key.as_ref() {
        Name::Named(Named::ArrowUp) => Shortcut::Up,
        Name::Named(Named::ArrowDown) => Shortcut::Down,
        Name::Named(Named::Enter) => Shortcut::Open,
        Name::Named(Named::Space) => Shortcut::Toggle,
        Name::Character("n") => Shortcut::New,
        _ => return None,
    };
    Some(Message::Shortcut(shortcut))
}

/// The ids of the text fields that can have the focus next to the list. Some keys (↑/↓, Enter in a
/// field without a submit action) are not taken by a field, so the status of the event alone does
/// not tell that someone is typing; the fields are asked instead. A new text field in this window
/// belongs here. (The title and notes editors take every key they get, so they need no entry.)
const TEXT_FIELDS: [&str; 2] = [QUICK_ADD, detail::DUE_INPUT];

/// Whether one of [`TEXT_FIELDS`] has the focus. A field that is not on screen does not answer.
fn typing() -> Task<bool> {
    Task::batch(TEXT_FIELDS.map(operation::is_focused)).collect().map(|answers| answers.contains(&true))
}

impl App {
    pub fn new(source: Source, launch: Launch) -> (App, Task<Message>) {
        let open = launch.open;
        let zoom = Zoom::for_source(&source);
        let app = App {
            source,
            today: launch.now.map_or_else(|| Local::now().date_naive(), |now| now.date()),
            size: torro_gui::window(AppId::TASKS).size,
            account: None,
            accounts: Vec::new(),
            folded: HashSet::new(),
            connect: Connect,
            collections: Vec::new(),
            todos: Vec::new(),
            filter: Filter::Today,
            show_completed: false,
            loaded: false,
            quick_add: String::new(),
            selected: None,
            cursor: None,
            scroll: None,
            detail: None,
            error: None,
            appearance: Appearance::System,
            system_mode: Mode::None,
            menu: match open {
                Some(Open::Menu) => Some(Menu::Nav),
                Some(Open::Display) => Some(Menu::Display),
                Some(Open::Switcher) => Some(Menu::Switcher),
                _ => None,
            },
            // Without a sidebar the app switcher lives in the drawer.
            drawer: matches!(open, Some(Open::Drawer | Open::Switcher)),
            sidebar: SidebarMode::default(),
            zoom,
            launch,
            opened: false,
        };
        let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
        let task = Task::batch([
            ui_state::load_task(AppId::TASKS).map(Message::UiState),
            app.fetch_account(),
            app.fetch_appearance(),
            app.zoom.fetch().map(Message::Zoom),
            iced::system::theme().map(Message::SystemMode),
            size,
        ]);
        (app, task)
    }

    pub fn title(&self) -> String {
        trf("Tasks · {filter}", &[("filter", &self.filter_name(&self.filter))])
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let every = if self.connected() { Duration::from_secs(60) } else { Duration::from_secs(2) };
        let mut subscriptions = vec![
            iced::time::every(every).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            // Zoom keys work everywhere, glance views and the open detail included.
            self.zoom.subscription().map(Message::Zoom),
        ];
        // Mini and strip are displays: no keys, and nothing open that Escape could close. (A key
        // like Space must not tick off a task nobody sees.)
        if !self.is_glance() {
            // Escape closes an open menu, drawer or the detail; nothing listens while none is open.
            if self.menu.is_some() || self.drawer || self.detail.is_some() {
                subscriptions.push(widgets::escape().map(|()| Message::Escape));
            }
            if self.connected() {
                subscriptions.push(iced::event::listen_with(shortcut));
            }
        }
        if self.detail.as_ref().is_some_and(Detail::wants_autosave) {
            subscriptions.push(iced::time::every(AUTOSAVE).map(|_| Message::Detail(detail::Message::Flush)));
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

    /// Asks the core for the stored appearance. Done on every tick, so a change in the control
    /// centre reaches the open window within a minute (two seconds while no account is set up).
    fn fetch_appearance(&self) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { theme::stored_appearance(&source).await }, Message::Appearance)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Resized(size) => self.size = size,
            Message::AccountLoaded(Ok(accounts)) => {
                let was_connected = self.connected();
                self.account = Some(torro_pim::combined_state(&accounts));
                self.accounts = accounts;
                if self.connected() && !was_connected {
                    return self.load();
                }
            }
            Message::AccountLoaded(Err(err)) => self.error = Some(load_failed(&err)),
            Message::Connect(message) => match self.connect.update(message) {
                // The account is set up in the control centre; the polling in `Tick` picks it up.
                connect::Action::OpenSettings => connect::open_settings(),
            },
            Message::Loaded(result) => match result {
                Ok((collections, todos)) => {
                    let place = self.cursor_place();
                    self.collections = collections;
                    self.todos = todos;
                    self.loaded = true;
                    self.error = None;
                    self.keep_cursor(place);
                    // Keep the detail panel on the same task after a reload.
                    let selected_uid = self.detail.as_ref().map(|d| d.original.uid.clone());
                    self.selected = selected_uid.and_then(|uid| self.todos.iter().position(|t| t.uid == uid));
                    let lists = self.writable_lists();
                    match (self.selected, self.detail.as_mut()) {
                        (Some(_), Some(detail)) => detail.refresh(&lists, &self.todos),
                        _ => self.detail = None,
                    }
                    return self.open_launched();
                }
                Err(err) => self.error = Some(load_failed(&err)),
            },
            Message::ToggleAccount(id) => {
                if !self.folded.remove(&id) {
                    self.folded.insert(id);
                }
            }
            Message::SetFilter(filter) => {
                self.filter = filter;
                self.cursor = None;
                self.scroll = None;
                self.close_menus();
                return self.close_detail();
            }
            Message::ShowCompleted(show) => self.show_completed = show,
            Message::QuickAddChanged(value) => self.quick_add = value,
            Message::QuickAddSubmit => {
                let summary = self.quick_add.trim().to_string();
                let Some(collection) = self.target_list() else {
                    self.error = Some(tr("No task list yet").into());
                    return Task::none();
                };
                if summary.is_empty() {
                    return Task::none();
                }
                self.quick_add.clear();
                let todo = Todo {
                    uid: String::new(),
                    collection,
                    href: None,
                    etag: None,
                    summary,
                    due: (self.filter == Filter::Today).then_some(EventTime::Date(self.today)),
                    priority: None,
                    completed: false,
                    parent: None,
                    description: None,
                    url: None,
                    categories: Vec::new(),
                    sort_order: None,
                    link: None,
                };
                return self.save(todo);
            }
            Message::ToggleDone(index) => {
                if let Some(todo) = self.toggled(index) {
                    return self.save(todo);
                }
            }
            Message::Select(index) => return self.select(index),
            Message::Detail(message) => {
                let Some(detail) = self.detail.as_mut() else { return Task::none() };
                let action = detail.update(message);
                return self.detail_action(action);
            }
            Message::Saved(result) => match result {
                Ok(saved) => {
                    // The panel keeps editing on top of the stored version, with its new etag,
                    // and sends what was typed meanwhile.
                    let follow_up = match self.detail.as_mut() {
                        Some(detail) if detail.original.uid == saved.uid => detail.stored(&saved),
                        _ => detail::Action::None,
                    };
                    return Task::batch([self.detail_action(follow_up), self.load()]);
                }
                Err(err) => {
                    if let Some(detail) = self.detail.as_mut() {
                        detail.save_failed();
                    }
                    self.error = Some(trf("Save failed: {error}", &[("error", &err)]));
                }
            },
            Message::Deleted(result) => match result {
                Ok(()) => return self.load(),
                Err(err) => self.error = Some(trf("Delete failed: {error}", &[("error", &err)])),
            },
            Message::Refresh => {
                let source = self.source.clone();
                let sync =
                    Task::perform(async move { source.sync().await.map_err(|e| e.to_string()) }, |_| Message::Tick);
                return sync.chain(self.load());
            }
            Message::Tick => {
                self.today = self.launch.now.map_or_else(|| Local::now().date_naive(), |now| now.date());
                let appearance = Task::batch([self.fetch_appearance(), self.zoom.fetch().map(Message::Zoom)]);
                // The accounts come with every tick: one may have been added or removed.
                let accounts = self.fetch_account();
                if !self.connected() {
                    return Task::batch([appearance, accounts]);
                }
                // An open detail keeps what it holds until it is closed or saved; but a glance
                // shows no detail and must not go stale, and the detail follows a reload anyway.
                if self.detail.is_none() || self.is_glance() {
                    return Task::batch([appearance, accounts, self.load()]);
                }
                return Task::batch([appearance, accounts]);
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
            Message::Switcher(Switcher::OpenApp(app)) => return self.update(Message::OpenApp(app)),
            Message::Switcher(Switcher::New) => {
                // "New" is a new task: the quick add field takes it.
                self.close_menus();
                return operation::focus(QUICK_ADD);
            }
            // The app has no search yet; the button only closes what is open.
            Message::Switcher(Switcher::Search) => self.close_menus(),
            Message::ToggleNav => self.toggle_menu(Menu::Nav),
            Message::ToggleDisplay => self.toggle_menu(Menu::Display),
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
                    return self.update(Message::Detail(detail::Message::Escape));
                }
            }
            Message::OpenApp(app) => {
                self.close_menus();
                let source = self.source.clone();
                return Task::perform(open_app(source, app), Message::AppOpened);
            }
            Message::AppOpened(Ok(())) => {}
            Message::AppOpened(Err(err)) => {
                self.error = Some(trf("Could not open the app: {error}", &[("error", &err)]))
            }
            Message::OpenSettings => {
                self.close_menus();
                connect::open_settings();
            }
            Message::Scrolled(offset, height) => self.scroll = Some((offset, height)),
            Message::Shortcut(key) => {
                return typing().map(move |typing| Message::ShortcutChecked(key, typing));
            }
            Message::ShortcutChecked(key, typing) => {
                // `[` works wherever the sidebar is (it also closes the drawer it opened).
                if !typing && key == Shortcut::Sidebar {
                    return self.toggle_sidebar();
                }
                // Menus and the drawer are in front of the list; the keys are not for the list then.
                if !typing && self.menu.is_none() && !self.drawer {
                    return self.run_shortcut(key);
                }
            }
        }
        Task::none()
    }

    /// Carries out a plain key of the list.
    fn run_shortcut(&mut self, key: Shortcut) -> Task<Message> {
        match key {
            Shortcut::Up => self.step(false),
            Shortcut::Down => self.step(true),
            Shortcut::Open => match self.cursor_index() {
                Some(index) => self.select(index),
                None => Task::none(),
            },
            Shortcut::Toggle => match self.cursor_index() {
                Some(index) => self.update(Message::ToggleDone(index)),
                None => Task::none(),
            },
            // The same as "New task" in the header.
            Shortcut::New => self.update(Message::Switcher(Switcher::New)),
            Shortcut::Sidebar => self.toggle_sidebar(),
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
        if self.source.is_demo() {
            return Task::none();
        }
        ui_state::save_task(AppId::TASKS, UiState { sidebar: self.sidebar })
    }

    /// The layout class of the window.
    fn class(&self) -> SizeClass {
        layout::of(self.size)
    }

    /// Mini or strip: a display without header, sidebar, menus or dialogs.
    fn is_glance(&self) -> bool {
        matches!(self.class(), SizeClass::Mini | SizeClass::Strip)
    }

    /// `todos[index]` with its done state flipped, ready to be saved; none if there is no such
    /// task. The one thing a glance lets you do.
    fn toggled(&self, index: usize) -> Option<Todo> {
        let mut todo = self.todos.get(index)?.clone();
        todo.completed = !todo.completed;
        Some(todo)
    }

    /// Collection id → its colour.
    fn colors(&self) -> HashMap<String, Color> {
        self.collections.iter().map(|c| (c.id.clone(), collection_color(c.color.as_deref()))).collect()
    }

    /// What the list shows, without the colours and names only drawing needs.
    fn list_data(&self, show_list: bool) -> ListData<'_> {
        ListData {
            todos: &self.todos,
            colors: self.colors(),
            names: self.collections.iter().map(|c| (c.id.clone(), c.name.clone())).collect(),
            filter: &self.filter,
            show_completed: self.show_completed,
            today: self.today,
            selected: self.cursor_index(),
            show_list,
            attention: palette(&self.theme()).attention_text,
        }
    }

    /// Index into `todos` of the task under the cursor, if it is in the list.
    fn cursor_index(&self) -> Option<usize> {
        let uid = self.cursor.as_deref()?;
        self.todos.iter().position(|t| t.uid == uid)
    }

    /// Where the cursor row is among the shown rows.
    fn cursor_place(&self) -> Option<usize> {
        let index = self.cursor_index()?;
        list::order(&self.list_data(false)).iter().position(|&i| i == index)
    }

    /// After a reload: a cursor whose task is gone (ticked off, deleted elsewhere) moves to the row
    /// that took its place, so ticking tasks one after the other with Space just works.
    fn keep_cursor(&mut self, place: Option<usize>) {
        let (Some(place), true) = (place, self.cursor_index().is_none()) else { return };
        let order = list::order(&self.list_data(false));
        self.cursor = order.get(place.min(order.len().saturating_sub(1))).map(|&i| self.todos[i].uid.clone());
    }

    /// Moves the cursor one row down or up, and stops at the first and last row. With no cursor
    /// yet, ↓ chooses the first row and ↑ the last.
    fn step(&mut self, forward: bool) -> Task<Message> {
        let order = list::order(&self.list_data(false));
        let Some(last) = order.len().checked_sub(1) else { return Task::none() };
        let next = match (self.cursor_place(), forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(place), true) => (place + 1).min(last),
            (Some(place), false) => place.saturating_sub(1),
        };
        self.cursor = Some(self.todos[order[next]].uid.clone());
        self.reveal(order[next])
    }

    /// Scrolls the list the least that shows `todos[index]` completely.
    fn reveal(&mut self, index: usize) -> Task<Message> {
        let Some((offset, height)) = self.scroll else { return Task::none() };
        let Some((top, bottom)) = list::row_span(&self.list_data(false), index) else { return Task::none() };
        let target = if top < offset {
            top
        } else if bottom > offset + height {
            bottom - height
        } else {
            return Task::none();
        };
        // The viewport is only reported on the next frame; keys can repeat faster than that.
        self.scroll = Some((target, height));
        operation::scroll_to(list::SCROLL, AbsoluteOffset { x: None, y: Some(target) })
    }

    /// The window with the zoom badge over it.
    pub fn view(&self) -> Element<'_, Message> {
        widgets::ctrl_guard(self.zoom.view(self.content()))
    }

    fn content(&self) -> Element<'_, Message> {
        match &self.account {
            None => {
                return match &self.error {
                    Some(err) => widgets::error_state(err.as_str(), Message::Tick),
                    None => widgets::loading(),
                };
            }
            Some(account @ (AccountState::NotConnected | AccountState::SigningIn { .. })) => {
                return self.connect.view(account, tr("Tasks")).map(Message::Connect);
            }
            Some(AccountState::Connected { .. }) => {}
        }
        let class = self.class();
        if matches!(class, SizeClass::Mini | SizeClass::Strip) {
            // The open detail, menu and drawer stay in the state and come back with a larger window.
            return self.glance(class);
        }
        self.window(class)
    }

    /// Mini or strip: loading and the error of a first load as in every layout, then the glance at
    /// what is due (`glance.rs`).
    fn glance(&self, class: SizeClass) -> Element<'_, Message> {
        if let Some(unloaded) = self.unloaded() {
            return unloaded;
        }
        let data =
            GlanceData { todos: &self.todos, colors: self.colors(), today: self.today, error: self.error.as_deref() };
        match class {
            SizeClass::Strip => glance::strip(&data, self.size),
            _ => glance::mini(&data, self.size),
        }
    }

    /// Loading, or the error of the first load, while there are no tasks yet; none afterwards.
    fn unloaded(&self) -> Option<Element<'_, Message>> {
        if self.loaded {
            return None;
        }
        Some(match &self.error {
            Some(err) => widgets::error_state(err.as_str(), Message::Refresh),
            None => widgets::loading(),
        })
    }

    /// The whole window in the layout of `class`.
    fn window(&self, class: SizeClass) -> Element<'_, Message> {
        let (switcher, nav, display, property) = (Anchor::new(), Anchor::new(), Anchor::new(), Anchor::new());
        let sidebar = self.sidebar(SidebarMode::Expanded, &switcher);
        let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
            .then(|| self.sidebar(SidebarMode::Rail, &switcher));
        let header = self.header(class, &nav, &display);
        let menu = self.menu.map(|menu| match menu {
            Menu::Switcher => widgets::floating(
                &switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(AppId::TASKS, Message::Switcher),
                Message::CloseMenu,
            ),
            Menu::Nav => widgets::floating(&nav, Placement::Below(Align::Start), self.nav_menu(), Message::CloseMenu),
            Menu::Display => {
                widgets::floating(&display, Placement::Below(Align::End), self.display_menu(), Message::CloseMenu)
            }
        });
        let editor = || self.detail.as_ref()?.layer(&property).map(|layer| layer.map(Message::Detail));
        let layer = menu.or_else(editor);
        let detail =
            self.detail.as_ref().map(|detail| detail.panel(self.crumb_filter(detail), &property, Message::Detail));
        // The panel takes the place of the properties column while it is beside the list.
        let placed = panel::place(class, self.body(class), detail, layer);
        shell::shell(class, sidebar, header, placed.body, None)
            .rail(rail)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(placed.overlay)
            .into()
    }

    /// The header: the filter's title (wide) or the navigation menu (medium, narrow), then Display,
    /// refresh and the one primary button "New task" ("+" alone in the narrow layouts).
    fn header<'a>(&'a self, class: SizeClass, nav: &Anchor, display: &Anchor) -> Element<'a, Message> {
        let name = self.filter_name(&self.filter);
        let open = trf("{n} open", &[("n", &list::open_count(&self.todos, &self.filter, self.today))]);
        let icon = filter_icon(&self.filter);
        let new_task = Message::Switcher(Switcher::New);
        let (show_display, new_button) = match class {
            SizeClass::Wide => (
                widgets::text_button(
                    Some(Icon::SlidersHorizontal),
                    tr("Display"),
                    style::button::ghost,
                    Some(Message::ToggleDisplay),
                ),
                widgets::text_button(Some(Icon::Plus), tr("New task"), style::button::primary, Some(new_task)),
            ),
            SizeClass::Medium => (
                widgets::icon_button(Icon::SlidersHorizontal, tr("Display"), Some(Message::ToggleDisplay)),
                widgets::text_button(Some(Icon::Plus), tr("New task"), style::button::primary, Some(new_task)),
            ),
            SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => (
                widgets::icon_button(Icon::SlidersHorizontal, tr("Display"), Some(Message::ToggleDisplay)),
                widgets::compact_button(Icon::Plus, tr("New task"), style::button::primary, Some(new_task)),
            ),
        };
        let trailing = [
            widgets::anchored(display, show_display),
            widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::Refresh)),
            new_button,
        ];
        match class {
            SizeClass::Wide => shell::header_bar([], shell::header_title(icon, name, Some(open)), trailing),
            SizeClass::Medium => {
                let menu = widgets::menu_button(nav, icon, name, Message::ToggleNav);
                let title =
                    row![menu, typo::text(typo::BODY, open).style(theme::text_3).wrapping(text::Wrapping::None)]
                        .spacing(space::S)
                        .align_y(Alignment::Center);
                // The rail at the left has the panel button.
                shell::header_bar([], title, trailing)
            }
            SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => shell::header_bar(
                [shell::sidebar_toggle(Message::ShowDrawer)],
                widgets::menu_button(nav, icon, name, Message::ToggleNav),
                trailing,
            ),
        }
    }

    /// The Display menu: "Show completed" as a check, the choice that used to be a switch.
    fn display_menu(&self) -> Element<'_, Message> {
        let check = Mark::Check(palette(&self.theme()).text_2, self.show_completed);
        widgets::menu(vec![MenuSection::new(vec![MenuItem::new(
            check,
            tr("Show completed"),
            Message::ShowCompleted(!self.show_completed),
        )])])
    }

    /// The content below the header: the list at its own width, whatever the detail does; the
    /// detail is the side panel `window` places beside or over it.
    fn body(&self, class: SizeClass) -> Element<'_, Message> {
        widgets::content(size::CONTENT_MAX, self.list(class))
    }

    /// The quick add row, an error line if there is one, and the grouped list; loading and the
    /// error of a first load stand in for all three.
    fn list(&self, class: SizeClass) -> Element<'_, Message> {
        if let Some(unloaded) = self.unloaded() {
            return unloaded;
        }
        let mut top = column![self.quick_add_row()].spacing(space::S);
        if let Some(err) = &self.error {
            top = top.push(
                typo::text(typo::META, err.clone())
                    .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
            );
        }
        let show_list = matches!(class, SizeClass::Wide | SizeClass::Medium) && !matches!(self.filter, Filter::List(_));
        let data = self.list_data(show_list);
        column![container(top).padding(Padding::new(space::S).bottom(space::XS)), list::view(data)]
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// The quick add row: a calm line like a list row ("+ New task …", the key hint "N" at the
    /// end) that is a text input underneath. The icon and the hint lie over it, so the input keeps
    /// its hover and focus states; the hint goes away as soon as something is typed.
    fn quick_add_row(&self) -> Element<'_, Message> {
        let input = text_input(tr("New task …"), &self.quick_add)
            .id(QUICK_ADD)
            .on_input(Message::QuickAddChanged)
            .on_submit(Message::QuickAddSubmit)
            .padding(Padding { top: space::S, bottom: space::S, left: QUICK_ADD_START, right: QUICK_ADD_END })
            .style(style::text_input);
        let plus = icon::icon(Icon::Plus, icon::SIZE_SM).style(|t, _| svg::Style { color: Some(palette(t).text_3) });
        let mut over =
            row![plus, Space::new().width(Fill)].align_y(Alignment::Center).height(Fill).padding([0.0, space::M]);
        if self.quick_add.is_empty() {
            over = over.push(widgets::kbd("N"));
        }
        // 8 above and below the 20-high text make the row `CONTROL_LG` (36) high.
        stack![input, over].height(size::CONTROL_LG).into()
    }

    /// The filters with their icon and open count, then one entry per task list (ring, count).
    fn nav_entries(&self) -> (Vec<NavEntry>, Vec<NavEntry>) {
        let entry = |filter: Filter, count: bool| NavEntry {
            mark: filter_icon(&filter).map_or(Mark::Icon(Icon::ListTodo), Mark::Icon),
            label: self.filter_name(&filter),
            count: count.then(|| list::open_count(&self.todos, &filter, self.today)),
            active: filter == self.filter,
            filter,
        };
        let views = vec![entry(Filter::Today, true), entry(Filter::Planned, true), entry(Filter::All, false)];
        let lists = self
            .todo_lists()
            .into_iter()
            .map(|collection| {
                let filter = Filter::List(collection.id.clone());
                NavEntry {
                    mark: Mark::Ring(collection_color(collection.color.as_deref())),
                    label: collection.name,
                    count: Some(list::open_count(&self.todos, &filter, self.today)),
                    active: filter == self.filter,
                    filter,
                }
            })
            .collect();
        (views, lists)
    }

    /// The sidebar in `mode`: expanded (wide, drawer) or as the rail of icons (U.2).
    fn sidebar<'a>(&'a self, mode: SidebarMode, switcher: &Anchor) -> Element<'a, Message> {
        let (views, lists) = self.nav_entries();
        let row = |e: NavEntry| {
            let trail = e.trail();
            widgets::sidebar_item_in(mode, e.mark, e.label, trail, e.active, Message::SetFilter(e.filter))
        };
        let mut items: Vec<Element<'a, Message>> = views.into_iter().map(row).collect();
        match shell::account_groups(&self.accounts, lists, NavEntry::list_id) {
            // One account: as it always was, a flat list under "Lists".
            Err(lists) => {
                if !lists.is_empty() {
                    items.push(widgets::sidebar_group_in(mode, tr("Lists"), None));
                    items.extend(lists.into_iter().map(row));
                }
            }
            Ok(groups) => {
                for group in groups {
                    let Some(account) = group.account else {
                        items.push(widgets::sidebar_group_in(mode, tr("Lists"), None));
                        items.extend(group.items.into_iter().map(row));
                        continue;
                    };
                    let unfolded = !self.folded.contains(&account.id);
                    let toggle = Some((unfolded, Message::ToggleAccount(account.id.clone())));
                    items.push(widgets::sidebar_group_in(mode, account.label.as_str(), toggle));
                    // The rail has no title to unfold with, so it shows every list.
                    if unfolded || mode == SidebarMode::Rail {
                        items.extend(group.items.into_iter().map(row));
                    }
                }
            }
        }
        let footer = vec![
            widgets::sidebar_item_in(
                mode,
                Icon::Calendar,
                tr("Open Calendar"),
                Trail::Icon(Icon::ArrowUpRight),
                false,
                Message::OpenApp(AppId::CALENDAR.binary),
            ),
            widgets::sidebar_item_in(mode, Icon::Settings, tr("Settings"), Trail::None, false, Message::OpenSettings),
        ];
        let head = shell::sidebar_head(mode, AppId::TASKS, switcher, Message::Switcher, Message::ToggleSidebar);
        shell::sidebar(head, items, footer)
    }

    /// The navigation menu of the medium and narrow layouts: the sidebar's rows as a menu.
    fn nav_menu(&self) -> Element<'_, Message> {
        let (views, lists) = self.nav_entries();
        let item = |e: NavEntry| {
            let trail = e.trail();
            MenuItem::new(e.mark, e.label, Message::SetFilter(e.filter)).trail(trail).current(e.active)
        };
        let mut sections = vec![MenuSection::new(views.into_iter().map(item).collect())];
        match shell::account_groups(&self.accounts, lists, NavEntry::list_id) {
            Err(lists) => {
                if !lists.is_empty() {
                    sections.push(MenuSection::titled(tr("Lists"), lists.into_iter().map(item).collect()));
                }
            }
            // A menu has no folding: every account is a section named after it.
            Ok(groups) => sections.extend(groups.into_iter().map(|group| {
                let title = group.account.map_or_else(|| tr("Lists"), |account| account.label.as_str());
                MenuSection::titled(title, group.items.into_iter().map(item).collect())
            })),
        }
        sections.push(MenuSection::new(vec![
            MenuItem::new(Icon::Calendar, tr("Open Calendar"), Message::OpenApp(AppId::CALENDAR.binary))
                .trail(Trail::Icon(Icon::ArrowUpRight)),
            MenuItem::new(Icon::Settings, tr("Settings"), Message::OpenSettings),
        ]));
        widgets::menu(sections)
    }

    /// The filter name for the detail's crumb ("Work › Today"); none when the filter is the
    /// task's own list, which the crumb already names.
    fn crumb_filter(&self, detail: &Detail) -> Option<String> {
        match &self.filter {
            Filter::List(id) if *id == detail.original.collection => None,
            filter => Some(self.filter_name(filter)),
        }
    }

    /// Opens `todos[index]` in the detail, saving what the open detail still holds.
    fn select(&mut self, index: usize) -> Task<Message> {
        let Some(todo) = self.todos.get(index).cloned() else { return Task::none() };
        let flush = self.close_detail();
        self.selected = Some(index);
        self.cursor = Some(todo.uid.clone());
        self.detail = Some(Detail::new(&todo, &self.writable_lists(), &self.todos));
        flush
    }

    /// Closes the detail; a draft that still waits to be saved is saved.
    fn close_detail(&mut self) -> Task<Message> {
        self.selected = None;
        match self.detail.take().and_then(|detail| detail.leave()) {
            Some(todo) => self.save(todo),
            None => Task::none(),
        }
    }

    /// Carries out what the detail asked for.
    fn detail_action(&mut self, action: detail::Action) -> Task<Message> {
        match action {
            detail::Action::None => Task::none(),
            detail::Action::Save(todo) => self.save(todo),
            detail::Action::Close => self.close_detail(),
            detail::Action::Delete(todo) => {
                self.detail = None;
                self.selected = None;
                let source = self.source.clone();
                Task::perform(
                    async move { source.delete_todo(todo).await.map_err(|e| e.to_string()) },
                    Message::Deleted,
                )
            }
            detail::Action::ToggleDone(uid) => match self.todos.iter().position(|t| t.uid == uid) {
                Some(index) => self.update(Message::ToggleDone(index)),
                None => Task::none(),
            },
            detail::Action::Show(uid) => match self.todos.iter().position(|t| t.uid == uid) {
                Some(index) => self.select(index),
                None => Task::none(),
            },
            detail::Action::Focus(id) => iced::widget::operation::focus(id),
            detail::Action::OpenUrl(url) => {
                // Like the control centre: a browser that cannot be started is only logged.
                if let Err(err) = webbrowser::open(&url) {
                    eprintln!("cannot open the link in a browser: {err}");
                }
                Task::none()
            }
        }
    }

    fn toggle_menu(&mut self, menu: Menu) {
        self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
    }

    fn close_menus(&mut self) {
        self.menu = None;
        self.drawer = false;
    }

    fn filter_name(&self, filter: &Filter) -> String {
        match filter {
            Filter::Today => tr("Today").into(),
            Filter::Planned => tr("Planned").into(),
            Filter::All => tr("All").into(),
            Filter::List(id) => {
                self.collections.iter().find(|c| &c.id == id).map_or_else(|| tr("List").into(), |c| c.name.clone())
            }
        }
    }

    fn todo_lists(&self) -> Vec<Collection> {
        self.collections.iter().filter(|c| c.kind.has_todos()).cloned().collect()
    }

    /// Where quick add puts a new task: the open list when it is writable, else the first
    /// writable task list.
    fn target_list(&self) -> Option<String> {
        let writable = self.writable_lists();
        match &self.filter {
            Filter::List(id) if writable.iter().any(|c| &c.id == id) => Some(id.clone()),
            _ => writable.first().map(|c| c.id.clone()),
        }
    }

    fn writable_lists(&self) -> Vec<Collection> {
        self.todo_lists().into_iter().filter(|c| c.writable).collect()
    }

    fn connected(&self) -> bool {
        matches!(self.account, Some(AccountState::Connected { .. }))
    }

    fn fetch_account(&self) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { source.accounts().await.map_err(|e| e.to_string()) }, Message::AccountLoaded)
    }

    fn save(&self, todo: Todo) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { source.save_todo(todo).await.map_err(|e| e.to_string()) }, Message::Saved)
    }

    /// The detail states of `--open`, once the first tasks are in: a detail needs a task to open.
    fn open_launched(&mut self) -> Task<Message> {
        if std::mem::replace(&mut self.opened, true) {
            return Task::none();
        }
        let Some(open @ (Open::Detail | Open::Due)) = self.launch.open else { return Task::none() };
        let Some(first) = list::order(&self.list_data(false)).first().copied() else { return Task::none() };
        let select = self.select(first);
        if open == Open::Due {
            let card = self.update(Message::Detail(detail::Message::Open(detail::Editor::Due)));
            return Task::batch([select, card]);
        }
        select
    }

    fn load(&self) -> Task<Message> {
        // The forced states: loading never ends, the error comes at once, empty has no tasks.
        match self.launch.open {
            Some(Open::Loading) => return Task::none(),
            Some(Open::Error) => return Task::done(Message::Loaded(Err(DEMO_ERROR.into()))),
            _ => {}
        }
        let empty = self.launch.open == Some(Open::Empty);
        let source = self.source.clone();
        Task::perform(
            async move {
                let collections = source.collections().await.map_err(|e| e.to_string())?;
                let todos = if empty { Vec::new() } else { source.todos().await.map_err(|e| e.to_string())? };
                Ok((collections, todos))
            },
            Message::Loaded,
        )
    }
}

/// A row of the sidebar and the navigation menu.
struct NavEntry {
    mark: Mark,
    label: String,
    filter: Filter,
    /// Open tasks, where the row shows a counter.
    count: Option<usize>,
    active: bool,
}

impl NavEntry {
    fn trail(&self) -> Trail {
        self.count.map_or(Trail::None, Trail::Count)
    }

    /// The collection id of a list entry (what names its account); empty for the smart lists.
    fn list_id(&self) -> &str {
        match &self.filter {
            Filter::List(id) => id,
            _ => "",
        }
    }
}

/// The icon of a filter; lists have none (they show their colour as a ring).
fn filter_icon(filter: &Filter) -> Option<Icon> {
    match filter {
        Filter::Today => Some(Icon::Sun),
        Filter::Planned => Some(Icon::CalendarClock),
        Filter::All => Some(Icon::Layers),
        Filter::List(_) => None,
    }
}

/// Asks the core to start or raise `app`. Demo data has no core, so there it does nothing.
async fn open_app(source: Source, app: &'static str) -> Result<(), String> {
    let Source::Core(client) = source else {
        return Ok(());
    };
    match torro_ipc::client::call(&client.socket, &Request::OpenApp { app: app.to_string() }).await {
        Ok(Response::Ok) => Ok(()),
        Ok(Response::Error { message, .. }) => Err(message),
        Ok(other) => Err(format!("unexpected answer: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

/// The error text when the account or the tasks could not be loaded; the error state's "Retry"
/// is what the user can do about it. Like the calendar's `load_failed`.
fn load_failed(err: &str) -> String {
    trf("Tasks could not be loaded: {error}", &[("error", &err)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_app() -> App {
        App::new(Source::from_args(["--demo".to_string()]), Launch::default()).0
    }

    #[test]
    fn the_sidebar_collapses_in_wide_and_opens_the_drawer_elsewhere() {
        let mut app = demo_app();
        assert_eq!(app.sidebar, SidebarMode::Expanded, "expanded until ui.json says otherwise");
        let _ = app.update(Message::UiState(UiState { sidebar: SidebarMode::Rail }));
        assert_eq!(app.sidebar, SidebarMode::Rail, "the stored choice comes back");
        let _ = app.update(Message::Resized(Size::new(1600.0, 1000.0)));
        let _ = app.update(Message::ToggleSidebar);
        assert_eq!((app.sidebar, app.drawer), (SidebarMode::Expanded, false), "wide: the choice flips");
        let _ = app.update(Message::ShortcutChecked(Shortcut::Sidebar, false));
        assert_eq!(app.sidebar, SidebarMode::Rail, "[ does the same");
        let _ = app.update(Message::ShortcutChecked(Shortcut::Sidebar, true));
        assert_eq!(app.sidebar, SidebarMode::Rail, "not while typing");
        for size in [Size::new(800.0, 600.0), Size::new(420.0, 780.0)] {
            let _ = app.update(Message::Resized(size));
            let _ = app.update(Message::ToggleSidebar);
            assert!(app.drawer, "{size:?}: the panel button opens the drawer");
            let _ = app.update(Message::ShortcutChecked(Shortcut::Sidebar, false));
            assert!(!app.drawer, "{size:?}: [ closes it again");
            assert_eq!(app.sidebar, SidebarMode::Rail, "{size:?}: the wide choice stays");
        }
    }

    #[test]
    fn the_window_builds_with_rail_and_drawer_in_every_layout() {
        let mut app = demo_app();
        let _ = app.update(Message::AccountLoaded(Ok(vec![account("work")])));
        for sidebar in [SidebarMode::Expanded, SidebarMode::Rail] {
            let _ = app.update(Message::UiState(UiState { sidebar }));
            for size in [Size::new(1600.0, 1000.0), Size::new(800.0, 600.0), Size::new(420.0, 780.0)] {
                let _ = app.update(Message::Resized(size));
                let _ = app.view();
            }
        }
    }

    #[test]
    fn two_accounts_group_the_lists_but_not_the_smart_lists() {
        let mut app = launched(&["--demo"]);
        let grouped = |app: &App| {
            let (views, lists) = app.nav_entries();
            let groups = shell::account_groups(&app.accounts, lists, NavEntry::list_id);
            let names = groups.ok().map(|groups| {
                groups.iter().map(|g| g.items.iter().map(|e| e.label.clone()).collect::<Vec<_>>()).collect::<Vec<_>>()
            });
            (views.len(), names)
        };
        let _ = app.update(Message::AccountLoaded(Ok(two_accounts())));
        assert_eq!(
            grouped(&app),
            (3, Some(vec![vec!["Tasks".to_string()], vec!["Groceries".to_string(), "Family".to_string()]])),
            "Today, Planned and All stay above the groups, each account has its lists"
        );
        let _ = app.update(Message::AccountLoaded(Ok(vec![account(torro_pim::demo::WORK_ACCOUNT)])));
        assert_eq!(grouped(&app).1, None, "a single account is not grouped");
    }

    #[test]
    fn an_account_title_folds_and_unfolds_its_lists() {
        let mut app = launched(&["--demo"]);
        let work = torro_pim::demo::WORK_ACCOUNT.to_string();
        let _ = app.update(Message::ToggleAccount(work.clone()));
        assert!(app.folded.contains(&work));
        // Folding hides rows in the sidebar only; the smart lists still count every task.
        assert_eq!(app.filter, Filter::Today);
        let _ = app.update(Message::ToggleAccount(work));
        assert!(app.folded.is_empty());
    }

    #[test]
    fn the_grouped_sidebar_and_menu_build_in_every_layout() {
        for accounts in [vec![account("work")], two_accounts()] {
            let mut app = launched(&["--demo"]);
            let _ = app.update(Message::AccountLoaded(Ok(accounts)));
            let _ = app.update(Message::ToggleAccount(torro_pim::demo::HOME_ACCOUNT.into()));
            for sidebar in [SidebarMode::Expanded, SidebarMode::Rail] {
                let _ = app.update(Message::UiState(UiState { sidebar }));
                for size in [WIDE, MEDIUM, NARROW] {
                    let _ = app.update(Message::Resized(size));
                    let _ = app.update(Message::ShowDrawer);
                    let _ = app.view();
                    let _ = app.nav_menu();
                }
            }
        }
    }

    #[test]
    fn theme_follows_appearance_and_desktop() {
        let mut app = demo_app();
        let _ = app.update(Message::SystemMode(Mode::Light));
        assert_eq!(app.theme(), theme::light(), "System follows the desktop");
        let _ = app.update(Message::Appearance(Ok(Appearance::Dark)));
        assert_eq!(app.theme(), theme::theme(), "Dark ignores a light desktop");
        let _ = app.update(Message::Appearance(Ok(Appearance::Light)));
        let _ = app.update(Message::SystemMode(Mode::Dark));
        assert_eq!(app.theme(), theme::light(), "Light ignores a dark desktop");
    }

    #[test]
    fn a_failed_settings_request_keeps_the_appearance() {
        let mut app = demo_app();
        let _ = app.update(Message::Appearance(Ok(Appearance::Light)));
        let _ = app.update(Message::Appearance(Err("core is not running".into())));
        assert_eq!(app.theme(), theme::light());
    }

    #[test]
    fn menus_toggle_and_a_filter_closes_them() {
        let mut app = demo_app();
        let _ = app.update(Message::ToggleNav);
        assert_eq!(app.menu, Some(Menu::Nav));
        let _ = app.update(Message::ToggleNav);
        assert_eq!(app.menu, None, "a second click closes the menu");
        let _ = app.update(Message::Switcher(Switcher::Toggle));
        assert_eq!(app.menu, Some(Menu::Switcher));
        let _ = app.update(Message::ShowDrawer);
        assert!(app.drawer && app.menu.is_none(), "the drawer replaces an open menu");
        let _ = app.update(Message::SetFilter(Filter::Planned));
        assert!(!app.drawer && app.menu.is_none());
        assert_eq!(app.filter, Filter::Planned);
    }

    #[test]
    fn escape_closes_menu_and_drawer() {
        let mut app = demo_app();
        let _ = app.update(Message::ToggleNav);
        let _ = app.update(Message::CloseMenu);
        assert_eq!(app.menu, None);
        let _ = app.update(Message::ShowDrawer);
        let _ = app.update(Message::CloseMenu);
        assert!(!app.drawer);
    }

    #[test]
    fn opening_an_app_in_demo_needs_no_core() {
        let source = Source::from_args(["--demo".to_string()]);
        let opened = iced::futures::executor::block_on(open_app(source, AppId::CALENDAR.binary));
        assert_eq!(opened, Ok(()));
    }

    #[test]
    fn filters_have_icons_and_lists_rings() {
        assert_eq!(filter_icon(&Filter::Today), Some(Icon::Sun));
        assert_eq!(filter_icon(&Filter::List("work".into())), None);
    }

    fn task(uid: &str) -> Todo {
        Todo {
            uid: uid.into(),
            collection: "work".into(),
            href: None,
            etag: None,
            summary: uid.into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
        }
    }

    fn day(d: u32) -> Option<EventTime> {
        Some(EventTime::Date(NaiveDate::from_ymd_opt(2026, 10, d).expect("valid date")))
    }

    /// An app on the "All" list with, in the order the list shows them: overdue (with its subtask
    /// child), today, later, undated. Today is 11 October 2026.
    fn app_with_rows() -> App {
        let mut app = demo_app();
        app.today = NaiveDate::from_ymd_opt(2026, 10, 11).expect("valid date");
        app.filter = Filter::All;
        app.loaded = true;
        app.todos = vec![
            Todo { due: day(12), ..task("later") },
            Todo { parent: Some("overdue".into()), ..task("child") },
            task("undated"),
            Todo { due: day(11), ..task("today") },
            Todo { due: day(9), ..task("overdue") },
        ];
        app
    }

    fn press(app: &mut App, key: Shortcut) -> Task<Message> {
        app.update(Message::ShortcutChecked(key, false))
    }

    fn cursor(app: &App) -> Option<&str> {
        app.cursor.as_deref()
    }

    #[test]
    fn arrows_walk_the_rows_in_the_order_shown_and_stop_at_the_ends() {
        let mut app = app_with_rows();
        let mut seen = Vec::new();
        for _ in 0..5 {
            let _ = press(&mut app, Shortcut::Down);
            seen.push(cursor(&app).map(str::to_string));
        }
        let expected = ["overdue", "child", "today", "later", "undated"].map(|uid| Some(uid.to_string()));
        assert_eq!(seen, expected, "groups and subtasks in display order");
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(cursor(&app), Some("undated"), "down stops at the last row");
        for uid in ["later", "today", "child", "overdue", "overdue"] {
            let _ = press(&mut app, Shortcut::Up);
            assert_eq!(cursor(&app), Some(uid));
        }
    }

    #[test]
    fn without_a_cursor_down_takes_the_first_row_and_up_the_last() {
        let mut app = app_with_rows();
        let _ = press(&mut app, Shortcut::Up);
        assert_eq!(cursor(&app), Some("undated"));
        app.cursor = None;
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(cursor(&app), Some("overdue"));
        let mut empty = demo_app();
        let _ = press(&mut empty, Shortcut::Down);
        assert_eq!(cursor(&empty), None, "an empty list has no cursor");
    }

    #[test]
    fn the_cursor_row_is_the_highlighted_row() {
        let mut app = app_with_rows();
        assert_eq!(app.list_data(false).selected, None);
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(app.list_data(false).selected.map(|i| app.todos[i].uid.as_str()), Some("overdue"));
        let _ = app.update(Message::Select(0));
        assert_eq!(cursor(&app), Some("later"), "clicking a row moves the cursor there");
    }

    #[test]
    fn enter_opens_the_task_under_the_cursor() {
        let mut app = app_with_rows();
        let _ = press(&mut app, Shortcut::Open);
        assert!(app.detail.is_none(), "nothing to open without a cursor");
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Open);
        assert_eq!(app.detail.as_ref().map(|d| d.original.uid.as_str()), Some("child"));
        let _ = app.update(Message::Escape);
        assert!(app.detail.is_none());
        assert_eq!(cursor(&app), Some("child"), "the cursor stays when the detail closes");
    }

    #[test]
    fn space_ticks_the_task_under_the_cursor() {
        let mut app = app_with_rows();
        assert_eq!(press(&mut app, Shortcut::Toggle).units(), 0, "nothing to tick without a cursor");
        let _ = press(&mut app, Shortcut::Down);
        assert!(press(&mut app, Shortcut::Toggle).units() > 0, "the save of the ticked task is started");
    }

    #[test]
    fn n_focuses_quick_add_and_closes_menus() {
        let mut app = app_with_rows();
        let _ = app.update(Message::ToggleDisplay);
        assert!(app.menu.is_some());
        app.menu = None;
        assert!(press(&mut app, Shortcut::New).units() > 0);
    }

    #[test]
    fn keys_do_nothing_while_typing_or_behind_a_menu() {
        let mut app = app_with_rows();
        let _ = app.update(Message::ShortcutChecked(Shortcut::Down, true));
        assert_eq!(cursor(&app), None, "a text field has the focus");
        assert_eq!(app.update(Message::ShortcutChecked(Shortcut::New, true)).units(), 0);
        let _ = app.update(Message::ToggleNav);
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(cursor(&app), None, "a menu is in front of the list");
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::ShowDrawer);
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(cursor(&app), None, "so is the drawer");
    }

    #[test]
    fn only_plain_unused_keys_become_shortcuts() {
        use iced::event::Status;
        use iced::keyboard::{Event, Key, Location, Modifiers, key::Named, key::Physical};
        let press = |key: Key, modifiers: Modifiers| {
            iced::Event::Keyboard(Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
                location: Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        let id = iced::window::Id::unique();
        let keys = [
            (Key::Named(Named::ArrowDown), Shortcut::Down),
            (Key::Named(Named::ArrowUp), Shortcut::Up),
            (Key::Named(Named::Enter), Shortcut::Open),
            (Key::Named(Named::Space), Shortcut::Toggle),
            (Key::Character("n".into()), Shortcut::New),
        ];
        for (key, expected) in keys {
            let got = shortcut(press(key.clone(), Modifiers::empty()), Status::Ignored, id);
            assert!(matches!(got, Some(Message::Shortcut(s)) if s == expected), "{key:?}");
            assert!(shortcut(press(key.clone(), Modifiers::empty()), Status::Captured, id).is_none(), "{key:?} taken");
            assert!(shortcut(press(key, Modifiers::CTRL), Status::Ignored, id).is_none(), "ctrl");
        }
        assert!(shortcut(press(Key::Character("x".into()), Modifiers::empty()), Status::Ignored, id).is_none());
    }

    #[test]
    fn after_a_reload_the_cursor_moves_to_the_row_that_took_the_place() {
        let mut app = app_with_rows();
        for _ in 0..3 {
            let _ = press(&mut app, Shortcut::Down);
        }
        assert_eq!(cursor(&app), Some("today"));
        // "today" is ticked off elsewhere: the next row ("later") is now third.
        let todos: Vec<Todo> = app.todos.iter().filter(|t| t.uid != "today").cloned().collect();
        let _ = app.update(Message::Loaded(Ok((Vec::new(), todos.clone()))));
        assert_eq!(cursor(&app), Some("later"));
        // Same task still there: the cursor stays.
        let _ = app.update(Message::Loaded(Ok((Vec::new(), todos))));
        assert_eq!(cursor(&app), Some("later"));
        let _ = app.update(Message::Loaded(Ok((Vec::new(), Vec::new()))));
        assert_eq!(cursor(&app), None);
        // A new filter starts without a cursor.
        let mut app = app_with_rows();
        let _ = press(&mut app, Shortcut::Down);
        let _ = app.update(Message::SetFilter(Filter::Planned));
        assert_eq!(cursor(&app), None);
    }

    #[test]
    fn the_list_scrolls_the_least_that_shows_the_cursor_row() {
        let mut app = app_with_rows();
        // Heads are 36 high (the first only 31: it has no gap and no line above), rows 40: overdue 31–71,
        // child 71–111, then the "Today" head.
        let _ = app.update(Message::Scrolled(0.0, 100.0));
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(app.scroll, Some((11.0, 100.0)), "child ends at 111, so 11 scrolls it in");
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(app.scroll, Some((87.0, 100.0)), "today ends at 187");
        let _ = press(&mut app, Shortcut::Up);
        assert_eq!(app.scroll, Some((71.0, 100.0)), "child starts at 71, above the view");
        let _ = press(&mut app, Shortcut::Up);
        assert_eq!(app.scroll, Some((0.0, 100.0)), "the first row brings its group head along");
        let _ = app.update(Message::Scrolled(0.0, 400.0));
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(app.scroll, Some((0.0, 400.0)), "a row that is in view needs no scrolling");
    }

    #[test]
    fn escape_closes_the_menu_before_the_detail() {
        let mut app = demo_app();
        app.todos = vec![task("a"), task("b")];
        let _ = app.update(Message::Select(1));
        assert_eq!(app.detail.as_ref().map(|d| d.original.uid.as_str()), Some("b"));
        let _ = app.update(Message::ToggleNav);
        let _ = app.update(Message::Escape);
        assert!(app.menu.is_none() && app.detail.is_some(), "the menu goes first");
        let _ = app.update(Message::Escape);
        assert!(app.detail.is_none() && app.selected.is_none());
    }

    #[test]
    fn a_reload_keeps_the_detail_on_its_task() {
        let mut app = demo_app();
        app.todos = vec![task("a"), task("b")];
        let _ = app.update(Message::Select(1));
        let _ = app.update(Message::Loaded(Ok((Vec::new(), vec![task("b")]))));
        assert_eq!(app.selected, Some(0));
        let _ = app.update(Message::Loaded(Ok((Vec::new(), vec![task("a")]))));
        assert!(app.detail.is_none(), "a task deleted elsewhere closes the detail");
    }

    #[test]
    fn without_a_preference_the_system_appearance_is_dark() {
        assert_eq!(demo_app().theme(), theme::theme());
    }

    #[test]
    fn the_display_menu_holds_show_completed_and_closes_with_escape() {
        let mut app = demo_app();
        let _ = app.update(Message::ToggleDisplay);
        assert_eq!(app.menu, Some(Menu::Display));
        let _ = app.update(Message::ShowCompleted(true));
        assert!(app.show_completed && app.menu == Some(Menu::Display), "ticking keeps the menu open");
        let _ = app.update(Message::Escape);
        assert_eq!(app.menu, None);
        let _ = app.update(Message::ToggleDisplay);
        let _ = app.update(Message::ToggleNav);
        assert_eq!(app.menu, Some(Menu::Nav), "one menu at a time");
    }

    #[test]
    fn the_list_shows_loading_until_the_first_load() {
        let mut app = demo_app();
        assert!(!app.loaded);
        let _ = app.update(Message::Loaded(Err("offline".into())));
        assert!(!app.loaded, "a failed first load keeps the error state");
        assert_eq!(app.error, Some(load_failed("offline")));
        let _ = app.update(Message::Loaded(Ok((Vec::new(), Vec::new()))));
        assert!(app.loaded && app.error.is_none());
        let _ = app.update(Message::Loaded(Err("offline".into())));
        assert!(app.loaded, "a later error does not throw the list away");
    }

    const MINI: Size = Size::new(240.0, 160.0);
    const STRIP: Size = Size::new(1200.0, 200.0);
    const NARROW: Size = Size::new(420.0, 780.0);
    const MEDIUM: Size = Size::new(900.0, 700.0);
    const WIDE: Size = Size::new(1600.0, 1000.0);

    /// A connected account with this id, named like it.
    fn account(id: &str) -> PimAccount {
        let state = AccountState::Connected {
            server: "demo".into(),
            login_name: "demo".into(),
            last_sync: None,
            problem: None,
        };
        PimAccount { id: id.into(), label: id.into(), kind: "nextcloud".into(), state }
    }

    /// The accounts of `--demo`: the sample spreads over two.
    fn two_accounts() -> Vec<PimAccount> {
        vec![account(torro_pim::demo::WORK_ACCOUNT), account(torro_pim::demo::HOME_ACCOUNT)]
    }

    fn connected(app: &mut App) {
        app.account = Some(AccountState::Connected {
            server: "https://cloud.example".into(),
            login_name: "sven".into(),
            last_sync: None,
            problem: None,
        });
    }

    #[test]
    fn the_class_follows_the_window_size() {
        let mut app = demo_app();
        assert_eq!(app.class(), SizeClass::Wide, "the default window is 1100 wide");
        for (size, class, glance) in [
            (MINI, SizeClass::Mini, true),
            (Size::new(320.0, 480.0), SizeClass::Mini, true),
            (STRIP, SizeClass::Strip, true),
            (NARROW, SizeClass::Narrow, false),
            (Size::new(800.0, 600.0), SizeClass::Medium, false),
            (WIDE, SizeClass::Wide, false),
        ] {
            let _ = app.update(Message::Resized(size));
            assert_eq!((app.class(), app.is_glance()), (class, glance), "{size:?}");
        }
    }

    #[test]
    fn filter_detail_cursor_and_quick_add_survive_a_trip_through_mini_and_strip() {
        let mut app = app_with_rows();
        let _ = app.update(Message::SetFilter(Filter::Planned));
        let _ = app.update(Message::QuickAddChanged("Buy oat milk".into()));
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Open);
        let open = app.detail.as_ref().map(|d| d.original.uid.clone());
        assert!(open.is_some());
        for size in [MINI, STRIP, NARROW, WIDE] {
            let _ = app.update(Message::Resized(size));
            assert_eq!(app.filter, Filter::Planned, "{size:?}");
            assert_eq!(app.quick_add, "Buy oat milk");
            assert_eq!(app.detail.as_ref().map(|d| d.original.uid.clone()), open);
            assert_eq!(app.cursor, open);
            // Every size builds its view from this state.
            let _ = app.view();
        }
    }

    #[test]
    fn the_detail_leaves_the_list_its_width_in_every_layout() {
        let mut app = app_with_rows();
        connected(&mut app);
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Open);
        let _ = app.update(Message::Detail(detail::Message::Open(detail::Editor::Due)));
        for size in [WIDE, MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            assert_eq!(panel::mode_for(app.class()) == panel::PanelMode::Beside, size == WIDE, "{size:?}");
            // The panel, the list and the open card are built from the one state.
            let _ = app.view();
        }
        assert!(app.detail.is_some(), "a resize does not close the panel");
    }

    #[test]
    fn a_glance_has_no_keys_and_no_escape() {
        let mut app = app_with_rows();
        connected(&mut app);
        let _ = app.update(Message::Resized(WIDE));
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Open);
        let wide = app.subscription().units();
        for size in [MINI, STRIP] {
            let _ = app.update(Message::Resized(size));
            assert_eq!(app.subscription().units(), wide - 2, "no shortcuts and no Escape at {size:?}");
        }
        let _ = app.update(Message::Resized(NARROW));
        assert_eq!(app.subscription().units(), wide);
    }

    #[test]
    fn ticking_a_task_off_in_mini_saves_it_done() {
        let mut app = app_with_rows();
        let _ = app.update(Message::Resized(MINI));
        let picks = glance::queue(&app.todos, app.today);
        let first = picks.first().expect("an open task with a due date");
        assert_eq!(app.todos[first.index].uid, "overdue", "the check on the first row ticks the overdue task");
        let todo = app.toggled(first.index).expect("the task");
        assert!(todo.completed && todo.uid == "overdue");
        assert!(!app.todos[first.index].completed, "the shown task changes only when the save comes back");
        assert!(app.update(Message::ToggleDone(first.index)).units() > 0, "the save is started");
        assert_eq!(app.update(Message::ToggleDone(99)).units(), 0, "a task that is gone does nothing");
        // And back: ticking a done task opens it again.
        app.todos[first.index].completed = true;
        assert!(app.toggled(first.index).is_some_and(|todo| !todo.completed));
    }

    #[test]
    fn a_glance_reloads_on_the_tick_even_with_an_open_detail() {
        let mut app = app_with_rows();
        connected(&mut app);
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Open);
        assert!(app.detail.is_some());
        let _ = app.update(Message::Resized(WIDE));
        let wide = app.update(Message::Tick).units();
        let _ = app.update(Message::Resized(MINI));
        let mini = app.update(Message::Tick).units();
        assert!(mini > wide, "the glance has no detail to protect, so it loads as well");
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    /// A demo app started with these words, its account and its first tasks loaded.
    fn launched(words: &[&str]) -> App {
        let launch = Launch::parse(&args(words)).expect("valid parameters");
        let (mut app, _) = App::new(launch.source(&args(words)), launch);
        let (collections, _, todos) = torro_pim::demo::sample(app.today);
        connected(&mut app);
        let _ = app.update(Message::Loaded(Ok((collections, todos))));
        app
    }

    #[test]
    fn every_state_of_open_is_parsed_in_both_spellings() {
        for (name, open) in Open::ALL {
            let expected = Launch { open: Some(open), ..Launch::default() };
            assert_eq!(Launch::parse(&args(&["--demo", "--open", name])), Ok(expected.clone()), "{name}");
            assert_eq!(Launch::parse(&args(&["--demo", &format!("--open={name}")])), Ok(expected), "{name}");
        }
        assert_eq!(Launch::parse(&args(&["--demo"])), Ok(Launch::default()));
        assert_eq!(Launch::parse(&[]), Ok(Launch::default()));
    }

    #[test]
    fn appearance_and_now_are_parsed() {
        let launch = Launch::parse(&args(&["--demo", "--appearance", "light", "--now", "2026-10-12T10:30"]))
            .expect("valid parameters");
        assert_eq!(launch.appearance, Some(Appearance::Light));
        assert_eq!(launch.now, NaiveDate::from_ymd_opt(2026, 10, 12).and_then(|day| day.and_hms_opt(10, 30, 0)));
        for (word, appearance) in
            [("light", Appearance::Light), ("dark", Appearance::Dark), ("system", Appearance::System)]
        {
            let launch = Launch::parse(&args(&["--appearance", word])).expect("works without --demo");
            assert_eq!(launch.appearance, Some(appearance));
        }
    }

    #[test]
    fn bad_parameters_name_the_valid_values() {
        let err = Launch::parse(&args(&["--demo", "--open", "popup"])).expect_err("unknown state");
        let text = err.to_string();
        assert!(text.contains("\"popup\"") && text.contains("--open"), "{text}");
        for (name, _) in Open::ALL {
            assert!(text.contains(name), "{name} is listed in: {text}");
        }
        let err = Launch::parse(&args(&["--appearance", "sepia"])).expect_err("unknown appearance");
        assert!(err.to_string().ends_with("valid values: light, dark, system"), "{err}");
        assert_eq!(Launch::parse(&args(&["--demo", "--open"])), Err(LaunchError::MissingValue("--open")));
        assert_eq!(Launch::parse(&args(&["--open", "detail"])), Err(LaunchError::NeedsDemo("--open")));
        assert_eq!(Launch::parse(&args(&["--now", "2026-10-12T10:30"])), Err(LaunchError::NeedsDemo("--now")));
        for bad in ["tomorrow", "2026-10-12", "2026-10-12 10:30", "2026-13-01T10:30"] {
            assert_eq!(Launch::parse(&args(&["--demo", "--now", bad])), Err(LaunchError::BadNow(bad.into())), "{bad}");
        }
    }

    #[test]
    fn the_menu_states_start_with_the_menu_open() {
        for (word, menu, drawer) in [
            ("menu", Some(Menu::Nav), false),
            ("display", Some(Menu::Display), false),
            ("drawer", None, true),
            // Without a sidebar the switcher is in the drawer.
            ("switcher", Some(Menu::Switcher), true),
        ] {
            let app = launched(&["--demo", "--open", word]);
            assert_eq!((app.menu, app.drawer), (menu, drawer), "{word}");
        }
    }

    #[test]
    fn the_detail_states_open_the_first_task_once() {
        let detail = launched(&["--demo", "--open", "detail"]);
        let open = detail.detail.as_ref().expect("the detail is open");
        let first = list::order(&detail.list_data(false))[0];
        assert_eq!(open.original.uid, detail.todos[first].uid);
        assert!(open.layer(&Anchor::new()).is_none(), "no property editor");

        let due = launched(&["--demo", "--open", "due"]);
        assert!(due.detail.as_ref().expect("the detail is open").layer(&Anchor::new()).is_some(), "the due card");

        // Closing it does not bring it back with the next reload.
        let mut closed = launched(&["--demo", "--open", "detail"]);
        let _ = closed.update(Message::Detail(detail::Message::Close));
        let (collections, _, todos) = torro_pim::demo::sample(closed.today);
        let _ = closed.update(Message::Loaded(Ok((collections, todos))));
        assert!(closed.detail.is_none());
    }

    #[test]
    fn loading_never_ends_and_the_error_comes_at_once() {
        assert_eq!(launched(&["--demo", "--open", "loading"]).load().units(), 0, "nothing is asked for");
        let words = ["--demo", "--open", "error"];
        let launch = Launch::parse(&args(&words)).expect("valid parameters");
        let (mut error, _) = App::new(launch.source(&args(&words)), launch);
        assert_eq!(error.load().units(), 1, "the error is the answer");
        let _ = error.update(Message::Loaded(Err(DEMO_ERROR.into())));
        assert_eq!(error.error, Some(load_failed(DEMO_ERROR)));
        assert!(!error.loaded);
        assert!(launched(&["--demo"]).load().units() > 0, "an ordinary demo loads");
    }

    #[test]
    fn now_fixes_the_day_and_the_demo_data_around_it() {
        let words = ["--demo", "--now", "2026-03-04T08:15"];
        let mut app = launched(&words);
        let pinned = NaiveDate::from_ymd_opt(2026, 3, 4).expect("valid date");
        assert_eq!(app.today, pinned);
        let _ = app.update(Message::Tick);
        assert_eq!(app.today, pinned, "the clock does not run on");
        assert!(app.launch.source(&args(&words)).is_demo());
    }

    #[test]
    fn the_appearance_parameter_beats_the_stored_one_in_this_process_only() {
        let mut app = launched(&["--appearance", "light", "--demo"]);
        let _ = app.update(Message::Appearance(Ok(Appearance::Dark)));
        assert_eq!(app.theme(), theme::light());
        assert_eq!(app.appearance, Appearance::Dark, "the stored value is kept as it is");
        let mut system = launched(&["--appearance", "system", "--demo"]);
        let _ = system.update(Message::Appearance(Ok(Appearance::Dark)));
        let _ = system.update(Message::SystemMode(Mode::Light));
        assert_eq!(system.theme(), theme::light(), "system follows the desktop whatever is stored");
    }

    /// Zooming in shrinks the window in logical pixels, so the layout changes like a browser's:
    /// 1440 wide at 150 % is 960, the medium layout. The demo keeps the zoom in the process.
    #[test]
    fn zooming_in_switches_the_size_class_like_a_browser() {
        use torro_gui::zoom::Step;
        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::Resized(Size::new(1440.0, 900.0)));
        assert_eq!(app.class(), SizeClass::Wide);
        for _ in 0..3 {
            let _ = app.update(Message::Zoom(zoom::Message::Step(Step::In)));
        }
        assert_eq!(app.scale_factor(), 1.5);
        assert!((app.size.width - 960.0).abs() < 0.01, "{:?}", app.size);
        assert_eq!(app.class(), SizeClass::Medium);
        let _ = app.update(Message::Zoom(zoom::Message::Step(Step::Reset)));
        assert_eq!((app.scale_factor(), app.class()), (1.0, SizeClass::Wide));
    }
}
