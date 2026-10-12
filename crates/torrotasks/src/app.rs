//! The tasks window: state, loading, quick add, saving, and the Linear window around it (task
//! T.1): `torro_gui::shell` with the sidebar (filters, lists, "Open Calendar", "Settings") in the
//! wide layout and a navigation menu ("Today ▾") plus drawer in the narrow one. In the wide layout
//! the sidebar collapses to a rail of icons (U.2: panel button or `[`, remembered in `ui.json`);
//! the medium layout always shows that rail. The list (`list.rs`) and the detail (`detail.rs`)
//! only render and report clicks as [`Message`]s.
//!
//! The columns (redesign R.2 in `docs/tasks/design.md`, mock-up
//! `docs/prototypes/tasks/redesign.html`) – see [`Columns`]: without an open task the list stands
//! alone at the left, at most [`size::CONTENT_MAX`] wide. With one open, wide windows whose content
//! is at least [`THREE_COLUMNS_FROM`] wide show three columns – the list ([`size::LIST`]), the
//! detail and the properties ([`size::PROPS`]) –, narrower wide and medium windows the list
//! ([`size::LIST_MEDIUM`]) and the detail, narrow windows the detail instead of the list. A
//! line separates the columns. Each column has its own head in the window's header bar: the list's
//! (filter, "{n} open", Display, "New task") over the list, the detail's (path, ↑ ↓, close) over
//! detail and properties; the header line runs under both.
//!
//! Detail and properties come from `detail.rs` (task R.3) and are put in at three places:
//! [`App::detail_head`] (in the header bar), [`App::detail_column`] and
//! [`App::properties_column`]; [`App::fit`] tells the detail which layout it is in. Its editors
//! and its "…" menu float over the window like the app's menus.
//!
//! The detail has no Save button: it hands out saves as it changes, and while title or notes wait
//! to be saved a one-second timer sends `Flush` ([`AUTOSAVE`]).
//!
//! The list head has one primary button, "New task", that focuses the quick add row (the same
//! message as "New" at the app switcher). The Display menu groups the list (due date, list,
//! priority, none), shows or hides completed tasks and syncs ("Sync now", with the time of the
//! last sync). There is no filter function yet, so no "Filter" button, and no "Refresh" button:
//! syncing lives in the Display menu and on `R`.
//!
//! The window size picks the layout ([`layout::of`], design skill section 6): mini and strip are
//! glances without any frame (`glance.rs`: the numbers "3 overdue · 5 today" and the next open
//! tasks, to be ticked off and nothing else); narrow, medium and wide are as described above. The
//! size is kept in the state ([`Message::Resized`]), so the keys can be switched off in a glance;
//! filter, selection, open detail, folded groups and quick add text survive every change of size.
//!
//! The keyboard (T.3): ↑/↓ or j/k move a cursor through the rows as the list shows them (groups
//! and subtasks included) – with a task open they open the next one –, Enter opens the task under
//! it, Space ticks it, N focuses the quick add row, R syncs, Escape closes menu, drawer or detail
//! (see [`Message::Escape`]). The cursor is the highlighted row; the list scrolls to keep it in
//! view. Plain keys are ignored while a text field has the focus ([`shortcut`], [`typing`]) or a
//! menu is open.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, Utc};
use iced::theme::Mode;
use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{Space, column, container, row, rule, stack, svg, text, text_input};
use iced::{Alignment, Background, Color, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::connect::{self, Connect};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, SidebarMode, Switcher};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail, collection_color};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{style, typo};
use torro_ipc::{Request, Response};
use torro_pim::account::account_of;
use torro_pim::demo::DemoSource;
use torro_pim::{AccountState, Collection, EventTime, PimAccount, PimSource, Todo};

use crate::detail::{self, Detail, Fit};
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
    /// The detail of the first open pull request in the list (a GitHub item with checks and
    /// review), so its detail can be photographed as well.
    PullRequest,
    /// The detail of the first task with its "…" menu open (Copy link, Delete task).
    More,
    /// The error state with its "Retry".
    Error,
    /// No tasks at all.
    Empty,
    /// The loading state, kept for good.
    Loading,
}

impl Open {
    const ALL: [(&'static str, Open); 11] = [
        ("detail", Open::Detail),
        ("menu", Open::Menu),
        ("display", Open::Display),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("due", Open::Due),
        ("pr", Open::PullRequest),
        ("more", Open::More),
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
///   it, for pictures that can be made again; needs `--demo`. The detail counts from that day as
///   well ([`Detail::on_day`]).
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
/// Where the quick add text starts: on the axis of the rows' titles – past the leading symbol
/// (16 in, 16 wide, 12 after it) and the priority column (14, 8 after it).
const QUICK_ADD_START: f32 = space::L + icon::SIZE + space::M + icon::SIZE_SM + space::S;
/// Where the quick add text ends: before the key hint "N" at the right.
const QUICK_ADD_END: f32 = space::L + space::XL + space::S;
/// Wide windows show the properties column beside the detail from this content width on (the
/// panel next to the sidebar or rail): list 460 + detail + properties 300 (mock-up).
pub const THREE_COLUMNS_FROM: f32 = 1320.0;
/// List ([`size::LIST_MEDIUM`]) and detail side by side from this content width on: the detail
/// then gets at least the width of the properties column ([`size::PROPS`]), the narrowest column
/// the design gives a task's facts. Below it (medium windows under about 740) the detail beside
/// the list would be a sliver of wrapped words – there it replaces the list as in narrow windows.
/// The content's two border lines and the line between the columns count as well.
pub const LIST_DETAIL_FROM: f32 = size::LIST_MEDIUM + size::PROPS + 3.0 * LINE;
/// The list head keeps the label of "Display" from this list width on; narrower it is an icon
/// with a tooltip (mock-up: `@container list (max-width:439px)`).
const HEAD_LABELS_FROM: f32 = 440.0;
/// The one-pixel lines between the columns.
const LINE: f32 = size::HAIRLINE;
/// How long after the last keystroke typed title or notes are saved.
const AUTOSAVE: Duration = Duration::from_secs(1);

/// The columns below the header bar (mock-up `redesign.html`), chosen by [`columns`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Columns {
    /// No task open: the list alone at the left, at most [`size::CONTENT_MAX`] wide (narrow
    /// windows: the full width).
    List,
    /// Narrow windows with a task open: the detail instead of the list.
    Detail,
    /// The list ([`size::LIST_MEDIUM`]) and the detail; the properties are chips in the detail.
    ListDetail,
    /// The list ([`size::LIST`]), the detail and the properties column ([`size::PROPS`]).
    ListDetailProps,
}

/// The columns for a window of `class` whose content (the panel beside sidebar or rail; the whole
/// window in narrow ones) is `content` wide, with a task open in the detail or not. Mini and strip
/// show a glance and no columns; they count as narrow here. A medium window too small for list
/// and detail side by side ([`LIST_DETAIL_FROM`]) shows the detail instead of the list, as a
/// narrow one does.
pub fn columns(class: SizeClass, content: f32, detail_open: bool) -> Columns {
    match (class, detail_open) {
        (_, false) => Columns::List,
        (SizeClass::Wide, true) if content >= THREE_COLUMNS_FROM => Columns::ListDetailProps,
        (SizeClass::Wide | SizeClass::Medium, true) if content >= LIST_DETAIL_FROM => Columns::ListDetail,
        (_, true) => Columns::Detail,
    }
}

/// The width of the content for a window `width` wide in `class`: the window less the sidebar
/// (`sidebar`: expanded or rail) and the inset margin in the wide and medium layouts, the whole
/// window otherwise.
pub fn content_width(class: SizeClass, sidebar: SidebarMode, width: f32) -> f32 {
    let side = match (class, sidebar) {
        (SizeClass::Wide, SidebarMode::Expanded) => size::SIDEBAR,
        (SizeClass::Wide, SidebarMode::Rail) | (SizeClass::Medium, _) => shell::RAIL,
        (SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip, _) => return width,
    };
    (width - side - size::INSET).max(0.0)
}

/// The width of the list column in `columns`, inside a content `content` wide: alone at most
/// [`size::CONTENT_MAX`] (inside the panel's border; the whole panel when less than
/// [`space::XXL`] would be left beside it) – the whole width in narrow windows –, beside
/// the detail [`size::LIST`] or [`size::LIST_MEDIUM`]. Rows shorten after it ([`list::RowParts`]).
pub fn list_width(class: SizeClass, columns: Columns, content: f32) -> f32 {
    match columns {
        Columns::List if matches!(class, SizeClass::Wide | SizeClass::Medium) => {
            let inner = content - 2.0 * LINE;
            // A sliver of empty panel beside the list would only look like a mistake: then the
            // list takes the whole panel.
            if inner - size::CONTENT_MAX < space::XXL { inner } else { size::CONTENT_MAX }
        }
        Columns::List | Columns::Detail => content,
        Columns::ListDetail => size::LIST_MEDIUM,
        Columns::ListDetailProps => size::LIST,
    }
}

/// What quick add gives a new task when "+" of `group` was pressed (`None`: the quick add row
/// itself): `(due, priority, list)`. A due group sets its day – yesterday for "Overdue", today,
/// tomorrow for "Upcoming", none for "No date" –, a priority group its priority, a list group its
/// list. Otherwise a task added under "Today" is due today, as before.
pub fn quick_add_fields(
    group: Option<&list::Group>,
    filter: &Filter,
    today: NaiveDate,
) -> (Option<EventTime>, Option<u8>, Option<String>) {
    use list::Group;
    let day = |date: Option<NaiveDate>| date.map(EventTime::Date);
    let by_filter = day((*filter == Filter::Today).then_some(today));
    match group {
        Some(Group::Overdue) => (day(today.pred_opt()), None, None),
        Some(Group::Today) => (day(Some(today)), None, None),
        Some(Group::Upcoming) => (day(today.succ_opt()), None, None),
        Some(Group::NoDate) => (None, None, None),
        Some(Group::List(id)) => (by_filter, None, Some(id.clone())),
        Some(Group::Priority(prio)) => (by_filter, prio.ical(), None),
        Some(Group::Completed | Group::All) | None => (by_filter, None, None),
    }
}

/// Minutes in an hour and in a day, for [`ago`].
const HOUR: i64 = 60;
const DAY: i64 = 24 * HOUR;

/// How long ago `then` was, for "Synced 2 min ago": "just now", "5 min ago", "3 h ago", "2 d ago".
pub fn ago(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (now - then).num_minutes();
    if minutes < 1 {
        tr("just now").into()
    } else if minutes < HOUR {
        trf("{n} min ago", &[("n", &minutes)])
    } else if minutes < DAY {
        trf("{n} h ago", &[("n", &(minutes / HOUR))])
    } else {
        trf("{n} d ago", &[("n", &(minutes / DAY))])
    }
}

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
    /// How the list groups its rows (Display menu). Window state, nothing is stored.
    group_by: list::GroupBy,
    /// Groups folded away in the list. Window state.
    collapsed: HashSet<list::Group>,
    /// The group whose "+" was pressed: the next quick add task goes there ([`quick_add_fields`]).
    quick_group: Option<list::Group>,
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
    /// A group head: fold or unfold the group.
    ToggleGroup(list::Group),
    /// "+" in a group head: the next quick add task goes into this group ([`quick_add_fields`]).
    AddInGroup(list::Group),
    /// The Display menu: group the list like this.
    SetGroupBy(list::GroupBy),
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
    /// R: sync now ("Sync now" in the Display menu).
    Sync,
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
        Name::Named(Named::ArrowUp) | Name::Character("k") => Shortcut::Up,
        Name::Named(Named::ArrowDown) | Name::Character("j") => Shortcut::Down,
        Name::Named(Named::Enter) => Shortcut::Open,
        Name::Named(Named::Space) => Shortcut::Toggle,
        Name::Character("n") => Shortcut::New,
        Name::Character("r") => Shortcut::Sync,
        _ => return None,
    };
    Some(Message::Shortcut(shortcut))
}

/// The ids of the text fields that can have the focus next to the list. Some keys (↑/↓, Enter in a
/// field without a submit action) are not taken by a field, so the status of the event alone does
/// not tell that someone is typing; the fields are asked instead. A new text field in this window
/// belongs here. (The title and notes editors take every key they get, so they need no entry.)
const TEXT_FIELDS: [&str; 3] = [QUICK_ADD, detail::DUE_INPUT, detail::SUBTASK_INPUT];

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
            group_by: list::GroupBy::default(),
            collapsed: HashSet::new(),
            quick_group: None,
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
                self.quick_group = None;
                self.close_menus();
                return self.close_detail();
            }
            Message::ShowCompleted(show) => self.show_completed = show,
            Message::QuickAddChanged(value) => self.quick_add = value,
            Message::QuickAddSubmit => {
                let summary = self.quick_add.trim().to_string();
                let (due, priority, list) = quick_add_fields(self.quick_group.as_ref(), &self.filter, self.today);
                let writable = list.filter(|id| self.writable_lists().iter().any(|c| &c.id == id));
                let Some(collection) = writable.or_else(|| self.target_list()) else {
                    self.error = Some(tr("No task list yet").into());
                    return Task::none();
                };
                if summary.is_empty() {
                    return Task::none();
                }
                self.quick_add.clear();
                self.quick_group = None;
                let todo = Todo {
                    uid: String::new(),
                    collection,
                    href: None,
                    etag: None,
                    summary,
                    due,
                    priority,
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
            Message::ToggleGroup(group) => {
                if !self.collapsed.remove(&group) {
                    self.collapsed.insert(group);
                }
            }
            Message::AddInGroup(group) => {
                self.quick_group = Some(group);
                self.close_menus();
                return operation::focus(QUICK_ADD);
            }
            Message::SetGroupBy(group_by) => {
                self.group_by = group_by;
                // Folds and the "+" group belong to the old groups.
                self.collapsed.clear();
                self.quick_group = None;
                self.scroll = None;
                self.menu = None;
            }
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
                // "Sync now" in the Display menu: the menu has done its job.
                self.menu = None;
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
            Shortcut::Sync => self.update(Message::Refresh),
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

    /// The columns the window shows now.
    fn columns(&self) -> Columns {
        let class = self.class();
        columns(class, content_width(class, self.sidebar, self.size.width), self.detail.is_some())
    }

    /// The width of the list column now.
    fn list_width(&self) -> f32 {
        let class = self.class();
        list_width(class, self.columns(), content_width(class, self.sidebar, self.size.width))
    }

    /// What the list shows. Rows name their list (pill or dot) unless the filter is one list.
    fn list_data(&self) -> ListData<'_> {
        ListData {
            todos: &self.todos,
            colors: self.colors(),
            names: self.collections.iter().map(|c| (c.id.clone(), c.name.clone())).collect(),
            filter: &self.filter,
            show_completed: self.show_completed,
            today: self.today,
            selected: self.cursor_index(),
            show_list: !matches!(self.filter, Filter::List(_)),
            group_by: self.group_by,
            collapsed: &self.collapsed,
            width: self.list_width(),
            palette: palette(&self.theme()),
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
        list::order(&self.list_data()).iter().position(|&i| i == index)
    }

    /// After a reload: a cursor whose task is gone (ticked off, deleted elsewhere) moves to the row
    /// that took its place, so ticking tasks one after the other with Space just works.
    fn keep_cursor(&mut self, place: Option<usize>) {
        let (Some(place), true) = (place, self.cursor_index().is_none()) else { return };
        let order = list::order(&self.list_data());
        self.cursor = order.get(place.min(order.len().saturating_sub(1))).map(|&i| self.todos[i].uid.clone());
    }

    /// Moves the cursor one row down or up, and stops at the first and last row. With no cursor
    /// yet, ↓ chooses the first row and ↑ the last. With a task open in the detail, the detail
    /// follows the cursor (Linear's j/k).
    fn step(&mut self, forward: bool) -> Task<Message> {
        let order = list::order(&self.list_data());
        let Some(last) = order.len().checked_sub(1) else { return Task::none() };
        let next = match (self.cursor_place(), forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(place), true) => (place + 1).min(last),
            (Some(place), false) => place.saturating_sub(1),
        };
        self.cursor = Some(self.todos[order[next]].uid.clone());
        let reveal = self.reveal(order[next]);
        let opened = self.detail.as_ref().is_some_and(|detail| detail.original.uid != self.todos[order[next]].uid);
        if opened {
            return Task::batch([self.select(order[next]), reveal]);
        }
        reveal
    }

    /// Scrolls the list the least that shows `todos[index]` completely.
    fn reveal(&mut self, index: usize) -> Task<Message> {
        let Some((offset, height)) = self.scroll else { return Task::none() };
        let Some((top, bottom)) = list::row_span(&self.list_data(), index) else { return Task::none() };
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
        let columns = self.columns();
        let header = self.header(class, columns, [&nav, &display, &property]);
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
        // The property editors and the "…" menu of the detail.
        let editor = || self.detail.as_ref()?.layer(&property).map(|layer| layer.map(Message::Detail));
        // The properties column is part of the body ([`App::body`]), not the shell's: the detail
        // brings its own, scrolling on its own.
        shell::shell(class, sidebar, header, self.body(class, columns, &property), None)
            .rail(rail)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(menu.or_else(editor))
            .into()
    }

    /// The window's header bar, split like the columns below it: the list's head over the list
    /// (with a line at the list's edge), the detail's head over detail and properties. Narrow
    /// windows show one of the two.
    /// The anchors are those of the navigation menu, the Display menu and the detail's editors.
    fn header<'a>(&'a self, class: SizeClass, columns: Columns, anchors: [&Anchor; 3]) -> Element<'a, Message> {
        let [nav, display, property] = anchors;
        let list_width = self.list_width();
        if columns == Columns::Detail {
            return self.detail_head(columns, property);
        }
        let list_head = self.list_head(class, list_width, nav, display);
        let content = content_width(class, self.sidebar, self.size.width);
        // A list that fills the panel needs no line at its edge: the panel's border is there.
        if !matches!(class, SizeClass::Wide | SizeClass::Medium) || list_width >= content - 2.0 * LINE {
            return list_head;
        }
        // The header bar pads 16 at the left; the list head keeps 12 at its right before the line.
        let mut split = row![
            container(list_head).width(list_width - space::L - space::M),
            Space::new().width(space::M),
            rule::vertical(LINE).style(style::rule),
        ]
        .align_y(Alignment::Center);
        if self.detail.is_some() {
            split = split.push(Space::new().width(space::L)).push(self.detail_head(columns, property));
        }
        split.into()
    }

    /// The head of the list: the filter's icon, name and "{n} open" – in narrow windows the
    /// navigation menu ("Today ▾") after the drawer button –, then Display (its label only where the
    /// list is at least [`HEAD_LABELS_FROM`] wide) and the one primary button "New task" ("+" alone
    /// in narrow windows).
    fn list_head<'a>(&'a self, class: SizeClass, width: f32, nav: &Anchor, display: &Anchor) -> Element<'a, Message> {
        let name = self.filter_name(&self.filter);
        let open = trf("{n} open", &[("n", &list::open_count(&self.todos, &self.filter, self.today))]);
        let icon = filter_icon(&self.filter);
        let new_task = Message::Switcher(Switcher::New);
        let show_display = if width >= HEAD_LABELS_FROM {
            widgets::text_button(
                Some(Icon::SlidersHorizontal),
                tr("Display"),
                style::button::ghost,
                Some(Message::ToggleDisplay),
            )
        } else {
            widgets::icon_button(Icon::SlidersHorizontal, tr("Display"), Some(Message::ToggleDisplay))
        };
        let narrow = !matches!(class, SizeClass::Wide | SizeClass::Medium);
        let new_button = if narrow {
            widgets::compact_button(Icon::Plus, tr("New task"), style::button::primary, Some(new_task))
        } else {
            widgets::text_button(Some(Icon::Plus), tr("New task"), style::button::primary, Some(new_task))
        };
        let trailing = [widgets::anchored(display, show_display), new_button];
        if !narrow {
            // Wide and medium have the sidebar or the rail for the navigation.
            return shell::header_bar([], shell::header_title(icon, name, Some(open)), trailing);
        }
        let title = row![
            widgets::menu_button(nav, icon, name, Message::ToggleNav),
            typo::text(typo::BODY, open).style(theme::text_3).wrapping(text::Wrapping::None),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);
        shell::header_bar([shell::sidebar_toggle(Message::ShowDrawer)], title, trailing)
    }

    /// How the detail lays itself out in `columns`: beside the properties column, with its
    /// properties as chips, or in place of the list.
    fn fit(columns: Columns) -> Fit {
        match columns {
            Columns::ListDetailProps => Fit::Columns,
            Columns::List | Columns::ListDetail => Fit::Chips,
            Columns::Detail => Fit::Narrow,
        }
    }

    /// The head of the detail (`detail.rs`, [`Detail::head`]) in the header bar, over the main
    /// column and the properties: the path, ↑ ↓, "…" and close; in narrow windows "‹ <filter>"
    /// back to the list. `anchor` is the one of the detail's editors and menu.
    fn detail_head<'a>(&'a self, columns: Columns, anchor: &Anchor) -> Element<'a, Message> {
        let Some(detail) = self.detail.as_ref() else { return Space::new().into() };
        detail.head(Self::fit(columns), self.filter_name(&self.filter), anchor).map(Message::Detail)
    }

    /// The detail's main column (`detail.rs`, [`Detail::main_column`]): title, notes, sub-tasks,
    /// for pull requests checks and reviews; the properties as chips under the title unless the
    /// properties column is there.
    fn detail_column<'a>(&'a self, columns: Columns, anchor: &Anchor) -> Element<'a, Message> {
        let Some(detail) = self.detail.as_ref() else { return Space::new().into() };
        detail.main_column(Self::fit(columns), palette(&self.theme()), anchor).map(Message::Detail)
    }

    /// The properties column of wide windows with [`Columns::ListDetailProps`] (`detail.rs`,
    /// [`Detail::properties_column`], [`size::PROPS`] wide), with the name of the account the
    /// task's list belongs to.
    fn properties_column<'a>(&'a self, anchor: &Anchor) -> Element<'a, Message> {
        let Some(detail) = self.detail.as_ref() else { return Space::new().into() };
        let account = account_of(&detail.original.collection, &self.accounts, |a| a.id.as_str());
        detail.properties_column(account.map(|a| a.label.clone()), anchor).map(Message::Detail)
    }

    /// The Display menu: how to group the list, "Show completed", and "Sync now" (`R`) under the
    /// time of the last sync.
    fn display_menu(&self) -> Element<'_, Message> {
        use list::GroupBy;
        let group = |mark: Icon, label: &'static str, by: GroupBy| {
            MenuItem::new(mark, label, Message::SetGroupBy(by)).current(self.group_by == by)
        };
        let check = Mark::Check(palette(&self.theme()).text_2, self.show_completed);
        let sync = vec![MenuItem::new(Icon::RefreshCw, tr("Sync now"), Message::Refresh).trail(Trail::Kbd("R"))];
        let synced = match self.last_sync() {
            Some(at) => MenuSection::titled(trf("Synced {when}", &[("when", &ago(at, Utc::now()))]), sync),
            None => MenuSection::new(sync),
        };
        widgets::menu(vec![
            MenuSection::titled(
                tr("Group by"),
                vec![
                    group(Icon::Calendar, tr("Due date"), GroupBy::Due),
                    group(Icon::List, tr("List"), GroupBy::List),
                    group(Icon::SignalHigh, tr("Priority"), GroupBy::Priority),
                    group(Icon::Minus, tr("None"), GroupBy::None),
                ],
            ),
            MenuSection::new(vec![MenuItem::new(
                check,
                tr("Show completed"),
                Message::ShowCompleted(!self.show_completed),
            )]),
            synced,
        ])
    }

    /// When the accounts last synced: the latest of them, as the core reports it.
    fn last_sync(&self) -> Option<DateTime<Utc>> {
        self.accounts
            .iter()
            .filter_map(|account| match &account.state {
                AccountState::Connected { last_sync, .. } => *last_sync,
                _ => None,
            })
            .max()
    }

    /// The content below the header bar in `columns`: the list alone at its width (a line at its
    /// right where it does not fill the panel), the detail instead of it, or list and detail side by
    /// side with a line between them, and the properties column after another line.
    fn body(&self, class: SizeClass, columns: Columns, anchor: &Anchor) -> Element<'_, Message> {
        let width = self.list_width();
        let line = || rule::vertical(LINE).style(style::rule);
        match columns {
            Columns::Detail => self.detail_column(columns, anchor),
            Columns::List => {
                let content = content_width(class, self.sidebar, self.size.width);
                if !matches!(class, SizeClass::Wide | SizeClass::Medium) || width >= content - 2.0 * LINE {
                    return self.list();
                }
                row![container(self.list()).width(width), line(), Space::new().width(Fill)].into()
            }
            Columns::ListDetail => {
                row![container(self.list()).width(width), line(), self.detail_column(columns, anchor)].into()
            }
            Columns::ListDetailProps => row![
                container(self.list()).width(width),
                line(),
                self.detail_column(columns, anchor),
                line(),
                self.properties_column(anchor),
            ]
            .into(),
        }
    }

    /// The quick add row, an error line if there is one, and the grouped list; loading and the
    /// error of a first load stand in for all three.
    fn list(&self) -> Element<'_, Message> {
        if let Some(unloaded) = self.unloaded() {
            return unloaded;
        }
        let mut content = column![self.quick_add_row()];
        if let Some(err) = &self.error {
            let line = typo::text(typo::META, err.clone())
                .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) });
            content = content.push(container(line).padding([space::S, space::L]));
        }
        content.push(list::view(self.list_data())).width(Fill).height(Fill).into()
    }

    /// The quick add row: a list row (40 with its line) that is a text input underneath – no box,
    /// "+" on the axis of the rows' leading symbols, the text on the axis of their titles, the key
    /// hint "N" at the right. The icon and the hint lie over the input, so it keeps its hover and
    /// focus states (focused: `selected` with the focus ring); the hint goes away as soon as
    /// something is typed.
    fn quick_add_row(&self) -> Element<'_, Message> {
        let height = size::ROW - LINE;
        let above = (height - typo::line_height(typo::BODY)) / 2.0;
        let input = text_input(tr("Add a task or paste a GitHub link…"), &self.quick_add)
            .id(QUICK_ADD)
            .on_input(Message::QuickAddChanged)
            .on_submit(Message::QuickAddSubmit)
            .padding(Padding { top: above, bottom: above, left: QUICK_ADD_START, right: QUICK_ADD_END })
            .style(quick_add_style);
        let plus = icon::icon(Icon::Plus, icon::SIZE).style(|t, _| svg::Style { color: Some(palette(t).text_3) });
        let mut over =
            row![plus, Space::new().width(Fill)].align_y(Alignment::Center).height(Fill).padding([0.0, space::L]);
        if self.quick_add.is_empty() {
            over = over.push(widgets::kbd("N"));
        }
        column![stack![input, over].height(height), rule::horizontal(LINE).style(style::rule)].into()
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

    /// Opens `todos[index]` in the detail, saving what the open detail still holds.
    fn select(&mut self, index: usize) -> Task<Message> {
        let Some(todo) = self.todos.get(index).cloned() else { return Task::none() };
        let flush = self.close_detail();
        self.selected = Some(index);
        self.cursor = Some(todo.uid.clone());
        self.detail = Some(Detail::new(&todo, &self.writable_lists(), &self.todos).on_day(self.today));
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
            // R.3: a new sub-task, "Copy link" and ↑ ↓ in the detail's head.
            detail::Action::Create(todo) => self.save(todo),
            detail::Action::CopyLink(url) => iced::clipboard::write(url),
            detail::Action::Previous => self.step(false),
            detail::Action::Next => self.step(true),
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
        let Some(open @ (Open::Detail | Open::Due | Open::PullRequest | Open::More)) = self.launch.open else {
            return Task::none();
        };
        let order = list::order(&self.list_data());
        let first = if open == Open::PullRequest {
            order.into_iter().find(|&index| is_open_pull_request(&self.todos[index]))
        } else {
            order.first().copied()
        };
        let Some(first) = first else { return Task::none() };
        let select = self.select(first);
        let editor = match open {
            Open::Due => detail::Editor::Due,
            Open::More => detail::Editor::More,
            _ => return select,
        };
        let card = self.update(Message::Detail(detail::Message::Open(editor)));
        Task::batch([select, card])
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
/// An open GitHub pull request (what `--open pr` looks for).
fn is_open_pull_request(todo: &Todo) -> bool {
    todo.link.as_ref().and_then(|link| link.github.as_ref()).is_some_and(|github| {
        github.kind == torro_pim::model::GitHubKind::PullRequest && github.state == torro_pim::model::GitHubState::Open
    })
}

fn filter_icon(filter: &Filter) -> Option<Icon> {
    match filter {
        Filter::Today => Some(Icon::Sun),
        Filter::Planned => Some(Icon::CalendarClock),
        Filter::All => Some(Icon::Layers),
        Filter::List(_) => None,
    }
}

/// The quick add field, built on `style::text_input`: no frame at rest and on hover (it is a list
/// row, not a box), and `selected` with the focus ring while focused.
fn quick_add_style(t: &Theme, status: text_input::Status) -> text_input::Style {
    let base = style::text_input(t, status);
    match status {
        text_input::Status::Focused { .. } => {
            text_input::Style { background: Background::Color(palette(t).selected), ..base }
        }
        _ => text_input::Style { border: base.border.width(0), ..base },
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
        assert_eq!(app.list_data().selected, None);
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(app.list_data().selected.map(|i| app.todos[i].uid.as_str()), Some("overdue"));
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
            (Key::Character("j".into()), Shortcut::Down),
            (Key::Character("k".into()), Shortcut::Up),
            (Key::Character("r".into()), Shortcut::Sync),
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
        // Heads are 32 high and flush with the rows, rows 40: overdue 32–72, child 72–112, then the
        // "Today" head 112–144 and its row.
        let _ = app.update(Message::Scrolled(0.0, 100.0));
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(app.scroll, Some((12.0, 100.0)), "child ends at 112, so 12 scrolls it in");
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(app.scroll, Some((84.0, 100.0)), "today ends at 184");
        let _ = press(&mut app, Shortcut::Up);
        assert_eq!(app.scroll, Some((72.0, 100.0)), "child starts at 72, above the view");
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
    fn the_columns_follow_the_window_and_the_open_task() {
        let mut app = app_with_rows();
        connected(&mut app);
        let _ = app.update(Message::Resized(WIDE));
        assert_eq!((app.columns(), app.list_width()), (Columns::List, size::CONTENT_MAX), "nothing open");
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Open);
        let _ = app.update(Message::Detail(detail::Message::Open(detail::Editor::Due)));
        for (size, sidebar, expected, list) in [
            (WIDE, SidebarMode::Expanded, Columns::ListDetailProps, size::LIST),
            (Size::new(1280.0, 840.0), SidebarMode::Expanded, Columns::ListDetail, size::LIST_MEDIUM),
            (Size::new(1400.0, 840.0), SidebarMode::Rail, Columns::ListDetailProps, size::LIST),
            (MEDIUM, SidebarMode::Rail, Columns::ListDetail, size::LIST_MEDIUM),
            (NARROW, SidebarMode::Expanded, Columns::Detail, NARROW.width),
        ] {
            let _ = app.update(Message::UiState(UiState { sidebar }));
            let _ = app.update(Message::Resized(size));
            assert_eq!((app.columns(), app.list_width()), (expected, list), "{size:?} {sidebar:?}");
            // Header, list, detail, properties and the open card are built from the one state.
            let _ = app.view();
        }
        assert!(app.detail.is_some(), "a resize does not close the detail");
    }

    #[test]
    fn three_columns_need_a_content_of_1320() {
        assert_eq!(content_width(SizeClass::Wide, SidebarMode::Expanded, 1600.0), 1360.0);
        assert_eq!(content_width(SizeClass::Wide, SidebarMode::Rail, 1600.0), 1540.0);
        assert_eq!(content_width(SizeClass::Medium, SidebarMode::Expanded, 1000.0), 940.0, "medium has the rail");
        assert_eq!(content_width(SizeClass::Narrow, SidebarMode::Expanded, 480.0), 480.0);
        assert_eq!(columns(SizeClass::Wide, 1320.0, true), Columns::ListDetailProps);
        assert_eq!(columns(SizeClass::Wide, 1319.0, true), Columns::ListDetail);
        assert_eq!(columns(SizeClass::Medium, 1320.0, true), Columns::ListDetail, "medium never has three");
        // Medium windows of 600-740: the detail beside the list would be a sliver, so it replaces it.
        assert_eq!(LIST_DETAIL_FROM, 683.0);
        assert_eq!(columns(SizeClass::Medium, LIST_DETAIL_FROM, true), Columns::ListDetail);
        assert_eq!(columns(SizeClass::Medium, LIST_DETAIL_FROM - 1.0, true), Columns::Detail);
        let medium_700 = content_width(SizeClass::Medium, SidebarMode::Rail, 700.0);
        assert_eq!(columns(SizeClass::Medium, medium_700, true), Columns::Detail, "700 wide");
        let medium_800 = content_width(SizeClass::Medium, SidebarMode::Rail, 800.0);
        assert_eq!(columns(SizeClass::Medium, medium_800, true), Columns::ListDetail, "800 wide");
        assert_eq!(columns(SizeClass::Narrow, 480.0, true), Columns::Detail);
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            assert_eq!(columns(class, 1400.0, false), Columns::List, "{class:?}");
        }
        assert_eq!(list_width(SizeClass::Medium, Columns::List, 540.0), 538.0, "inside the border of a small panel");
        assert_eq!(list_width(SizeClass::Narrow, Columns::List, 480.0), 480.0, "narrow: the whole width");
        assert_eq!(list_width(SizeClass::Medium, Columns::List, 740.0), 738.0, "no sliver of 18 beside the list");
        assert_eq!(list_width(SizeClass::Medium, Columns::List, 940.0), size::CONTENT_MAX);
    }

    #[test]
    fn quick_add_puts_the_task_where_the_plus_was() {
        use list::{Group, Prio};
        let today = NaiveDate::from_ymd_opt(2026, 10, 11).expect("valid date");
        let date = |d: u32| Some(EventTime::Date(NaiveDate::from_ymd_opt(2026, 10, d).expect("valid date")));
        let fields = |group: Option<Group>, filter: Filter| quick_add_fields(group.as_ref(), &filter, today);
        assert_eq!(fields(None, Filter::Today), (date(11), None, None), "under Today: due today, as before");
        assert_eq!(fields(None, Filter::All), (None, None, None));
        assert_eq!(fields(Some(Group::Overdue), Filter::All), (date(10), None, None));
        assert_eq!(fields(Some(Group::Today), Filter::All), (date(11), None, None));
        assert_eq!(fields(Some(Group::Upcoming), Filter::Today), (date(12), None, None));
        assert_eq!(fields(Some(Group::NoDate), Filter::Today), (None, None, None));
        assert_eq!(fields(Some(Group::List("home".into())), Filter::All), (None, None, Some("home".into())));
        assert_eq!(fields(Some(Group::Priority(Prio::Urgent)), Filter::Today), (date(11), Some(1), None));
        assert_eq!(fields(Some(Group::Priority(Prio::Low)), Filter::All), (None, Some(9), None));
    }

    #[test]
    fn group_heads_fold_add_and_regroup() {
        use list::{Group, GroupBy};
        let mut app = app_with_rows();
        let rows = list::order(&app.list_data()).len();
        let _ = app.update(Message::ToggleGroup(Group::Overdue));
        assert_eq!(list::order(&app.list_data()).len(), rows - 2, "overdue and its subtask fold away");
        let _ = app.update(Message::ToggleGroup(Group::Overdue));
        assert_eq!(list::order(&app.list_data()).len(), rows);
        assert!(app.update(Message::AddInGroup(Group::Upcoming)).units() > 0, "the quick add row gets the focus");
        assert_eq!(app.quick_group, Some(Group::Upcoming));
        let _ = app.update(Message::ToggleGroup(Group::Today));
        let _ = app.update(Message::ToggleDisplay);
        let _ = app.update(Message::SetGroupBy(GroupBy::Priority));
        assert_eq!(app.group_by, GroupBy::Priority);
        assert!(app.collapsed.is_empty() && app.quick_group.is_none() && app.menu.is_none(), "a fresh start");
        let _ = app.update(Message::AddInGroup(Group::Today));
        let _ = app.update(Message::SetFilter(Filter::Planned));
        assert_eq!(app.quick_group, None, "another filter forgets the group");
    }

    #[test]
    fn with_a_task_open_the_arrows_open_the_next_one() {
        let mut app = app_with_rows();
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Open);
        let _ = press(&mut app, Shortcut::Down);
        assert_eq!(app.detail.as_ref().map(|d| d.original.uid.as_str()), Some("child"), "↓ opens the next row");
        let _ = app.update(Message::Detail(detail::Message::Next));
        assert_eq!(app.detail.as_ref().map(|d| d.original.uid.as_str()), Some("today"), "so does ↓ in its head");
        let _ = app.update(Message::Detail(detail::Message::Previous));
        assert_eq!(app.detail.as_ref().map(|d| d.original.uid.as_str()), Some("child"));
        assert_eq!(cursor(&app), Some("child"), "the highlighted row follows");
        let _ = app.update(Message::Escape);
        let _ = press(&mut app, Shortcut::Down);
        assert!(app.detail.is_none(), "without an open task the arrows only move the cursor");
    }

    #[test]
    fn sync_times_read_like_a_clock() {
        let now = Utc::now();
        assert_eq!(ago(now, now), tr("just now"));
        assert_eq!(ago(now - chrono::Duration::minutes(2), now), trf("{n} min ago", &[("n", &2)]));
        assert_eq!(ago(now - chrono::Duration::hours(3), now), trf("{n} h ago", &[("n", &3)]));
        assert_eq!(ago(now - chrono::Duration::days(2), now), trf("{n} d ago", &[("n", &2)]));
    }

    #[test]
    fn the_demo_builds_in_every_layout_grouping_and_theme() {
        use list::GroupBy;
        let mut app = launched(&["--demo", "--open", "detail"]);
        let _ = app.update(Message::ShowCompleted(true));
        for appearance in [Appearance::Light, Appearance::Dark] {
            let _ = app.update(Message::Appearance(Ok(appearance)));
            for group_by in [GroupBy::Due, GroupBy::List, GroupBy::Priority, GroupBy::None] {
                let _ = app.update(Message::SetGroupBy(group_by));
                for size in [WIDE, Size::new(1280.0, 840.0), MEDIUM, NARROW, MINI, Size::new(320.0, 480.0), STRIP] {
                    let _ = app.update(Message::Resized(size));
                    let _ = app.update(Message::ToggleDisplay);
                    let _ = app.view();
                    let _ = app.update(Message::CloseMenu);
                }
            }
        }
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
        let first = list::order(&detail.list_data())[0];
        assert_eq!(open.original.uid, detail.todos[first].uid);
        assert!(open.layer(&Anchor::new()).is_none(), "no property editor");

        let pr = launched(&["--demo", "--open", "pr"]);
        let open = &pr.detail.as_ref().expect("the detail is open").original;
        assert!(is_open_pull_request(open), "{} is an open pull request", open.summary);

        let due = launched(&["--demo", "--open", "due"]);
        assert!(due.detail.as_ref().expect("the detail is open").layer(&Anchor::new()).is_some(), "the due card");
        let more = launched(&["--demo", "--open", "more"]);
        assert!(more.detail.as_ref().expect("the detail is open").layer(&Anchor::new()).is_some(), "the … menu");

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
