//! The calendar window: state, navigation, loading and saving, and the Linear-style frame around
//! the views (sidebar with the small month, header bar, view menu). The views (`views/`) and the
//! editor (`editor.rs`) only render and report clicks as [`Message`]s.
//!
//! The window size picks the layout ([`layout::of`], design skill section 6): mini and strip are
//! glances without any frame (`views::glance`); narrow shows a week as one day under the week
//! strip, the radar under it too, and a month as the agenda; medium and wide show the chosen view. State (view, date,
//! selection, open editor) survives every change of size.
//!
//! An event (a click on it, "New event", a click on an empty slot) opens in the side panel
//! (`torro_gui::panel`, U.4): beside the view in the wide layout, where it takes the place of the
//! properties column ("Up next"), and over the view otherwise; the open event is outlined in the
//! views. The editor has no Save button: it hands out saves as it changes, and while typed text
//! waits to be saved a one-second timer sends `Flush` ([`AUTOSAVE`]). Escape closes an open menu
//! or the drawer first, then whatever is open in the panel, then the panel.
//!
//! In the wide layout the sidebar collapses to a rail of icons (U.2: panel button at its top or
//! `[`, remembered in `ui.json`); the rail leaves out the small month. The medium layout always
//! shows the rail, whose panel button opens the full sidebar as a drawer.
//!
//! The night of the timeline and the radar is the suite's setting (`Settings::night`, task K.27):
//! fetched at start and on every tick like the appearance, so a change elsewhere arrives. A
//! double-click on a moon (or "Nights" in the header while there is no night) opens the floating
//! card "Night" with "From" and "Until" on the quarter hour, "Save" and "No night"; saving sends
//! `Request::SetSetting` with the key `night` (the demo keeps it in the process).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, Duration as Days, Local, NaiveDate, NaiveDateTime, NaiveTime};
use iced::theme::Mode;
use iced::widget::space as blank;
use iced::widget::{column, container, pick_list, responsive, row, svg};
use iced::{Alignment, Color, Element, Rectangle, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Night, Quarter};
use torro_gui::connect::{self, Connect};
use torro_gui::datetime::{format_long_date, month_name, to_utc, week_start};
use torro_gui::design::{self, Follow};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::links;
use torro_gui::panel::{self, PanelMode};
use torro_gui::shell::{self, Cover, SidebarMode, Surface, Switcher, TitleButtons};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail, collection_color};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{style, typo};
use torro_pim::account::account_of;
use torro_pim::demo::DemoSource;
use torro_pim::{AccountState, Collection, Event, PimAccount, PimSource};

use crate::editor::{self, Editor};
use crate::views::{self, Item, ViewData, glance, mini_month, short_date, timeline, week_strip};

/// The views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalView {
    Day,
    Week,
    Month,
    Agenda,
    Timeline,
    /// The next twelve hours on a clock face (`views::radar`).
    Radar,
}

impl CalView {
    const ALL: [CalView; 6] =
        [CalView::Day, CalView::Week, CalView::Month, CalView::Agenda, CalView::Timeline, CalView::Radar];

    fn label(self) -> &'static str {
        match self {
            CalView::Day => tr("Day"),
            CalView::Week => tr("Week"),
            CalView::Month => tr("Month"),
            CalView::Agenda => tr("Agenda"),
            CalView::Timeline => tr("Timeline"),
            CalView::Radar => tr("Radar"),
        }
    }

    /// The icon in the sidebar and the view menu (`calendar.html`).
    fn icon(self) -> Icon {
        match self {
            CalView::Day => Icon::CalendarDays,
            CalView::Week => Icon::CalendarRange,
            CalView::Month => Icon::Calendar,
            CalView::Agenda => Icon::List,
            CalView::Timeline => Icon::ChartGantt,
            CalView::Radar => Icon::Radar,
        }
    }

    /// The key that switches to the view ([`shortcut`]), shown as a key hint.
    fn key(self) -> &'static str {
        match self {
            CalView::Day => "D",
            CalView::Week => "W",
            CalView::Month => "M",
            CalView::Agenda => "A",
            // T alone goes to today.
            CalView::Timeline => "⇧T",
            CalView::Radar => "R",
        }
    }
}

/// The view the narrow layout shows for the chosen `view`: a week or the timeline as one day (under
/// the week strip), a month as the agenda; seven columns, a film strip of days or a month grid do
/// not fit 360–600 px. Every other layout shows the chosen view.
fn shown_in(view: CalView, class: SizeClass) -> CalView {
    match (class, view) {
        (SizeClass::Narrow, CalView::Week) => CalView::Day,
        (SizeClass::Narrow, CalView::Timeline) => CalView::Day,
        (SizeClass::Narrow, CalView::Month) => CalView::Agenda,
        _ => view,
    }
}

/// Mini and strip: displays without header, sidebar, menus or dialogs.
fn is_glance(class: SizeClass) -> bool {
    matches!(class, SizeClass::Mini | SizeClass::Strip)
}

/// The menus that open over the window; at most one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// The app switcher at the top of the sidebar.
    Switcher,
    /// "Week ▾" in the medium and narrow header: the views and the calendars.
    View,
}

/// A state the window can start in (`--demo --open <state>`), so that every state can be
/// photographed from a script without clicking: the four views, the editor, the menus and the
/// forced loading, empty and error states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    Day,
    Week,
    Month,
    Agenda,
    Timeline,
    Radar,
    /// The side panel with the first event that is not part of a series.
    Editor,
    /// The side panel with a new event.
    EditorNew,
    /// The side panel with an event that cannot be edited (the first occurrence of a series).
    EditorReadOnly,
    /// The timeline with the night's card open under "Nights" in the header.
    Night,
    /// The view menu ("Week ▾"); only the medium and narrow header have it.
    Menu,
    /// The sidebar drawer of the medium and narrow layouts.
    Drawer,
    /// The app switcher at the top of the sidebar (in the drawer where there is no sidebar).
    Switcher,
    /// The error state with its "Retry".
    Error,
    /// No events at all.
    Empty,
    /// The loading state, kept for good.
    Loading,
}

impl Open {
    const ALL: [(&'static str, Open); 16] = [
        ("day", Open::Day),
        ("week", Open::Week),
        ("month", Open::Month),
        ("agenda", Open::Agenda),
        ("timeline", Open::Timeline),
        ("radar", Open::Radar),
        ("editor", Open::Editor),
        ("editor-new", Open::EditorNew),
        ("editor-read-only", Open::EditorReadOnly),
        ("night", Open::Night),
        ("menu", Open::Menu),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("error", Open::Error),
        ("empty", Open::Empty),
        ("loading", Open::Loading),
    ];

    /// The view this state shows, for the view states.
    fn view(self) -> Option<CalView> {
        match self {
            Open::Day => Some(CalView::Day),
            Open::Week => Some(CalView::Week),
            Open::Month => Some(CalView::Month),
            Open::Agenda => Some(CalView::Agenda),
            Open::Timeline | Open::Night => Some(CalView::Timeline),
            Open::Radar => Some(CalView::Radar),
            _ => None,
        }
    }
}

/// The start parameters of the window besides `--demo` (which [`Source`] reads):
///
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
/// - `--design <id>`: draw with this design pack instead of the standard (task L.3); needs `--demo`.
///   An id that is not a valid pack is refused with the valid ones.
/// - `--now <YYYY-MM-DDTHH:MM>`: a fixed "now" (and demo data around its day), for pictures that
///   can be made again; needs `--demo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
    pub now: Option<NaiveDateTime>,
    pub design: Option<String>,
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
        let design = match flag_value(args, "--design")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--design")),
            Some(id) if design::is_choice(id) => Some(id.to_owned()),
            Some(id) => {
                let valid = design::choices();
                return Err(unknown("--design", id, valid.iter().map(String::as_str).collect()));
            }
        };
        Ok(Launch { open, appearance, now, design })
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
const DEMO_ERROR: &str = "the server did not answer (demo)";

/// Days the agenda shows from its anchor on.
const AGENDA_DAYS: i64 = 30;

/// Days the timeline shows from its anchor on (after the day before it), and its step for ‹ ›.
const TIMELINE_DAYS: i64 = 7;

/// How long typing rests before the editor saves it (as in the task detail).
const AUTOSAVE: Duration = Duration::from_secs(1);

/// The open card "Night": where it floats and the times picked so far.
#[derive(Debug, Clone, Copy, PartialEq)]
struct NightCard {
    /// The moon it came from, on the canvas of the timeline or the radar; `None`: under "Nights"
    /// in the header.
    at: Option<Rectangle>,
    from: Quarter,
    to: Quarter,
}

pub struct App {
    source: Source,
    /// The start parameters; `open` also keeps the forced states (error, empty, loading) alive.
    launch: Launch,
    /// The one-time part of `launch.open` (the editor) has been done.
    opened: bool,
    now: NaiveDateTime,
    /// The chosen view. The narrow layout may show another one ([`App::shown`]); this one comes
    /// back when the window is wider again.
    view: CalView,
    /// The date the view is built around.
    anchor: NaiveDate,
    /// How far the radar's dial is turned from the start of the anchor's day, in minutes
    /// (negative: back; `views::radar::start`). Window state like a scroll position: kept while
    /// other views are shown, back to 0 whenever the day changes.
    turn: i64,
    /// The first day of the radar's mini radars (`views::radar::coming_page`): the day after the
    /// anchor at first, it stays while the anchor moves within it, and its ‹ › browse it.
    coming: NaiveDate,
    account: Option<AccountState>,
    /// Every account behind the data, for grouping the calendars in the sidebar (task M1.3).
    accounts: Vec<PimAccount>,
    /// Accounts whose calendars the sidebar has folded away. Window state, nothing is stored.
    folded: HashSet<String>,
    connect: Connect,
    collections: Vec<Collection>,
    hidden: HashSet<String>,
    events: Vec<Event>,
    loading: bool,
    /// Events have been loaded once; until then the body shows the loading state.
    loaded: bool,
    error: Option<String>,
    /// The open menu, if any.
    menu: Option<Menu>,
    /// The sidebar drawer of the medium and narrow layouts is open.
    drawer: bool,
    /// The sidebar the user chose for the wide layout, kept in `ui.json`.
    sidebar: SidebarMode,
    /// The first of the month the small month in the sidebar shows. Follows the anchor, but its
    /// own arrows browse it without moving the main view.
    mini_month: NaiveDate,
    /// The window size, for the size class ([`layout::of`]). Kept from the window events rather
    /// than read in `iced::widget::responsive`, so `update` sees a change of layout and can
    /// scroll the time grid back to the morning (the new widget tree loses its scroll position).
    size: Size,
    /// The event open in the side panel.
    editor: Option<Editor>,
    /// Counts the editors opened. Every save carries the count of its editor, so an answer that
    /// arrives after the panel moved on to another event does not reach the new editor.
    edits: u64,
    /// The time grid has been scrolled to the morning once; it only exists after the first load.
    scrolled: bool,
    appearance: Appearance,
    system_mode: Mode,
    /// The suite-wide zoom (Ctrl + Plus / Minus / 0), the factor of the whole window.
    zoom: Zoom,
    /// Follows the design pack the suite settings name (task L.3).
    design: Follow,
    /// The timeline's unfolded nights and scroll position (window state, nothing is stored).
    timeline: timeline::State,
    /// The suite's night, for the timeline and the radar (`Settings::night`).
    night: Night,
    /// A save of the night is on its way; fetched values from before it are not applied.
    night_saving: bool,
    /// The card "Night", while it is open.
    night_card: Option<NightCard>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The band above the content was pressed: drag the window (see [`shell::titlebar`]).
    DragWindow,
    AccountLoaded(Result<Vec<PimAccount>, String>),
    /// An account's title in the sidebar: fold or unfold its calendars.
    ToggleAccount(String),
    Connect(connect::Message),
    Loaded(Result<(Vec<Collection>, Vec<Event>), String>),
    SetView(CalView),
    /// ‹ or ←: one view-length back (the radar: a day).
    Prev,
    /// › or →: one view-length ahead (the radar: a day).
    Next,
    /// "Today", T, the radar's "Back to now": today, and the radar back at now.
    Today,
    /// The radar's dial (wheel, drag, ↑ ↓): turn it this many minutes later (negative: earlier).
    Turn(i64),
    Refresh,
    /// The error state's "Retry": ask for the account again, or reload the events.
    Retry,
    ToggleCollection(String),
    /// The app switcher (top of the sidebar).
    Switcher(Switcher),
    /// Open `Menu`, or close it when it is open.
    ToggleMenu(Menu),
    /// Close the open menu and the drawer (click beside them).
    CloseMenu,
    /// Escape: closes the open menu or drawer, else what is open in the side panel, else the panel.
    Escape,
    /// Open or close the sidebar drawer (medium and narrow layouts).
    ToggleDrawer,
    /// The panel button at the top of the sidebar, or `[`: collapses or expands the sidebar (wide),
    /// opens or closes the drawer (medium, narrow).
    ToggleSidebar,
    /// The view state stored in `ui.json`, loaded at start.
    UiState(UiState),
    /// From the small month: show `date` in the current view.
    GoTo(NaiveDate),
    /// The small month's arrows: browse it by this many months.
    MiniMonth(i32),
    /// The ‹ › of the radar's "Coming days": browse its mini radars by this many weeks, the radar
    /// staying on its day.
    ComingPage(i64),
    /// "Settings" in the band or the sidebar: open the control centre.
    OpenSettings,
    /// The core answered `Request::OpenApp`.
    AppOpened(Result<(), String>),
    /// The window has this size (at start and after every resize).
    Resized(Size),
    /// From a view: create an event on `date`, at `time` (`None`: all-day or no slot).
    NewEvent {
        date: NaiveDate,
        time: Option<NaiveTime>,
    },
    /// From a view: open `App::events[index]`.
    OpenEvent(usize),
    /// From the month view: switch to the day view of `date`.
    ShowDay(NaiveDate),
    Editor(editor::Message),
    /// Keyboard: a new event on the shown day at 09:00.
    NewEventHere,
    /// The answer to a save of the editor with this count ([`App::edits`]).
    Saved(u64, Result<Event, String>),
    Deleted(Result<(), String>),
    /// A link of the editor was handed to the browser (issue #126).
    LinkOpened(Result<(), String>),
    Tick,
    /// The desktop's light/dark preference (once at start, then whenever it changes).
    SystemMode(Mode),
    /// The appearance stored in the suite settings; an error keeps the one we have.
    Appearance(Result<Appearance, String>),
    /// The zoom keys, the stored zoom and its badge.
    Zoom(zoom::Message),
    /// The design pack stored in the suite settings, and its fonts loaded.
    Design(design::Message),
    /// The timeline's scroll position, wheel and nights.
    Timeline(timeline::Change),
    /// A double-click on a moon (`at`: its place on the canvas) or "Nights" in the header without
    /// a night (`at: None`): open the card "Night". `fold`: the night the first click unfolded.
    EditNight {
        fold: Option<NaiveDate>,
        at: Option<Rectangle>,
    },
    /// "From" in the card.
    NightFrom(Quarter),
    /// "Until" in the card.
    NightTo(Quarter),
    /// "Save" in the card: the picked times become the night.
    SaveNight,
    /// "No night" in the card.
    NoNight,
    /// ✕, Escape or a click beside the card.
    CloseNight,
    /// The night stored in the suite settings; an error keeps the one we have.
    Night(Result<Night, String>),
    /// The core answered a save of the night.
    NightSaved(Result<(), String>),
}

/// Keyboard shortcuts while nothing lies over the window (the side panel beside the view does
/// not count): N new event, T today, ←/→ back/forward, D/W/M/A/R and Shift + T the views, `[` the
/// sidebar. Keys a text input already used are ignored. The radar adds ↑ ↓ ([`radar_shortcut`]).
fn shortcut(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event as Key, Key as Name, key::Named};
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(Key::KeyPressed { key, modifiers, .. }) = event else { return None };
    if shell::is_sidebar_key(&key, modifiers) {
        return Some(Message::ToggleSidebar);
    }
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key.as_ref() {
        Name::Named(Named::ArrowLeft) => Some(Message::Prev),
        Name::Named(Named::ArrowRight) => Some(Message::Next),
        Name::Character(c) => match c {
            "n" => Some(Message::NewEventHere),
            // Iced reports the key without Shift, so Shift + T arrives as "t".
            "t" if modifiers.shift() => Some(Message::SetView(CalView::Timeline)),
            "t" => Some(Message::Today),
            "d" => Some(Message::SetView(CalView::Day)),
            "w" => Some(Message::SetView(CalView::Week)),
            "m" => Some(Message::SetView(CalView::Month)),
            "a" => Some(Message::SetView(CalView::Agenda)),
            "r" => Some(Message::SetView(CalView::Radar)),
            _ => None,
        },
        _ => None,
    }
}

/// The radar's own keys, listened to only while it is shown: ↑ turns the dial an hour earlier,
/// ↓ an hour later (as the wheel: down is later). The other views leave the arrows to scrolling.
fn radar_shortcut(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event as Key, Key as Name, key::Named};
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(Key::KeyPressed { key, modifiers, .. }) = event else { return None };
    if modifiers.control() || modifiers.alt() || modifiers.logo() || modifiers.shift() {
        return None;
    }
    match key.as_ref() {
        Name::Named(Named::ArrowUp) => Some(Message::Turn(-views::radar::HOUR_STEP)),
        Name::Named(Named::ArrowDown) => Some(Message::Turn(views::radar::HOUR_STEP)),
        _ => None,
    }
}

impl App {
    pub fn new(source: Source, launch: Launch) -> (App, Task<Message>) {
        let now = launch.now.unwrap_or_else(|| Local::now().naive_local());
        let open = launch.open;
        let zoom = Zoom::for_source(&source);
        let design = Follow::for_source(&source);
        let app = App {
            source,
            now,
            view: open.and_then(Open::view).unwrap_or(CalView::Week),
            anchor: now.date(),
            turn: 0,
            coming: now.date() + Days::days(1),
            account: None,
            accounts: Vec::new(),
            folded: HashSet::new(),
            connect: Connect,
            collections: Vec::new(),
            hidden: HashSet::new(),
            events: Vec::new(),
            loading: true,
            loaded: false,
            error: None,
            menu: match open {
                Some(Open::Menu) => Some(Menu::View),
                Some(Open::Switcher) => Some(Menu::Switcher),
                _ => None,
            },
            // Without a sidebar the app switcher lives in the drawer.
            drawer: matches!(open, Some(Open::Drawer | Open::Switcher)),
            sidebar: SidebarMode::default(),
            mini_month: mini_month::shift(now.date(), 0),
            size: torro_gui::window(AppId::CALENDAR).size,
            editor: None,
            edits: 0,
            scrolled: false,
            appearance: Appearance::System,
            system_mode: Mode::None,
            zoom,
            design,
            launch,
            opened: false,
            timeline: timeline::State::default(),
            night: Night::DEFAULT,
            night_saving: false,
            night_card: None,
        };
        // The size, and the traffic lights onto the title band's line (U.11).
        let size = Task::batch([
            iced::window::latest().and_then(iced::window::size).map(Message::Resized),
            torro_gui::place_traffic_lights(),
        ]);
        let task = Task::batch([
            ui_state::load_task(AppId::CALENDAR).map(Message::UiState),
            app.fetch_account(),
            app.fetch_appearance(),
            app.fetch_night(),
            app.zoom.fetch().map(Message::Zoom),
            app.design.fetch().map(Message::Design),
            iced::system::theme().map(Message::SystemMode),
            size,
        ]);
        (app, task)
    }

    pub fn title(&self) -> String {
        trf("Calendar · {heading}", &[("heading", &self.heading())])
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // Faster while waiting for a sign-in in the browser.
        let every = if self.connected() { Duration::from_secs(60) } else { Duration::from_secs(2) };
        let tick = iced::time::every(every).map(|_| Message::Tick);
        let system = iced::system::theme_changes().map(Message::SystemMode);
        let resize = iced::window::resize_events().map(|(_, size)| Message::Resized(size));
        // Zoom keys work everywhere, glance views and open panels included.
        // The design pack polls the core every 2 s itself and wakes the window only for a change.
        let design = self.design.subscription().map(Message::Design);
        let mut all = vec![tick, system, resize, self.zoom.subscription().map(Message::Zoom), design];
        // Mini and strip are displays: no keys, nothing open to close.
        if !is_glance(self.class()) {
            // Over the view the panel is all there is to work with; beside it the view still is.
            let over = self.editor.is_some() && panel::mode_for(self.class()) == PanelMode::Over;
            if !over {
                all.push(iced::event::listen_with(shortcut));
                if self.shown() == CalView::Radar {
                    all.push(iced::event::listen_with(radar_shortcut));
                }
            }
            if self.menu.is_some() || self.drawer || self.editor.is_some() || self.night_card.is_some() {
                all.push(widgets::escape().map(|()| Message::Escape));
            }
        }
        // Only while typed text waits to be saved, so an idle panel costs nothing.
        if self.editor.as_ref().is_some_and(Editor::wants_autosave) {
            all.push(iced::time::every(AUTOSAVE).map(|_| Message::Editor(editor::Message::Flush)));
        }
        Subscription::batch(all)
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

    /// Asks the core for the stored night, like [`App::fetch_appearance`]. The demo has no core and
    /// keeps the night it has.
    fn fetch_night(&self) -> Task<Message> {
        let Source::Core(client) = &self.source else { return Task::none() };
        let socket = client.socket.clone();
        Task::perform(
            async move {
                match torro_ipc::client::call(&socket, &torro_ipc::Request::Settings).await {
                    Ok(torro_ipc::Response::Settings { settings }) => Ok(settings.night),
                    Ok(other) => Err(format!("unexpected answer: {other:?}")),
                    Err(err) => Err(err.to_string()),
                }
            },
            Message::Night,
        )
    }

    /// Makes `night` the night of this window and sends it to the core (`settings.set_setting`,
    /// key `night`); the demo keeps it in the process.
    fn set_night(&mut self, night: Night) -> Task<Message> {
        self.night = night;
        let Source::Core(client) = &self.source else { return Task::none() };
        self.night_saving = true;
        let socket = client.socket.clone();
        let request = torro_ipc::Request::SetSetting { key: torro_ipc::SettingKey::Night, value: night.to_value() };
        Task::perform(
            async move {
                match torro_ipc::client::call(&socket, &request).await {
                    Ok(torro_ipc::Response::Ok) => Ok(()),
                    Ok(torro_ipc::Response::Error { message, .. }) => Err(message),
                    Ok(other) => Err(format!("unexpected answer: {other:?}")),
                    Err(err) => Err(err.to_string()),
                }
            },
            Message::NightSaved,
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::DragWindow => return torro_gui::drag_window(),
            Message::AccountLoaded(Ok(accounts)) => {
                let was_connected = self.connected();
                // Only accounts that can hold calendars: GitHub alone is "No calendar account yet".
                self.account =
                    Some(torro_pim::account::combined_state_for(&accounts, torro_pim::CollectionKind::Events));
                self.accounts = accounts;
                if self.connected() && !was_connected {
                    return self.load();
                }
            }
            Message::AccountLoaded(Err(err)) => {
                self.loading = false;
                self.error = Some(load_failed(&err));
            }
            Message::Connect(message) => match self.connect.update(message) {
                // The account is set up in the control centre – in the first-run setup at this app's
                // step while that is not done (task W.9); the polling in `Tick` picks it up.
                connect::Action::OpenSettings => connect::open_setup_or_settings("calendar"),
            },
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok((collections, events)) => {
                        self.collections = collections;
                        self.events = events;
                        self.loaded = true;
                        self.error = None;
                        self.open_launched();
                        if !self.scrolled {
                            self.scrolled = true;
                            return self.scroll_to_morning();
                        }
                    }
                    Err(err) => self.error = Some(load_failed(&err)),
                }
            }
            Message::SetView(view) => {
                if view != self.view {
                    // Another view goes away: a timeline coming up is a new scrollable.
                    self.timeline.forget_viewport();
                }
                self.view = view;
                self.close_menus();
                return self.load().chain(self.scroll_to_morning());
            }
            Message::Prev => return self.go_to(self.step(-1)),
            Message::Next => return self.go_to(self.step(1)),
            Message::Today => return self.go_to(self.now.date()),
            Message::Turn(minutes) => self.turn = views::radar::clamp_offset(self.turn + minutes),
            Message::GoTo(date) => {
                self.close_menus();
                return self.go_to(date);
            }
            Message::MiniMonth(months) => self.mini_month = mini_month::shift(self.mini_month, months),
            Message::ComingPage(weeks) => {
                self.coming += Days::days(views::radar::COMING_DAYS * weeks);
                return self.load();
            }
            Message::Retry => {
                self.error = None;
                if !self.connected() {
                    self.account = None;
                    return self.fetch_account();
                }
                return self.load();
            }
            Message::Switcher(Switcher::Toggle) => return self.update(Message::ToggleMenu(Menu::Switcher)),
            Message::Switcher(Switcher::OpenApp(binary)) => {
                self.close_menus();
                let source = self.source.clone();
                return Task::perform(async move { source.open_app(binary).await }, Message::AppOpened);
            }
            // The calendar has no search yet; the button only closes what is open.
            Message::Switcher(Switcher::Search) => self.close_menus(),
            Message::Switcher(Switcher::New) => {
                self.close_menus();
                return self.update(Message::NewEventHere);
            }
            Message::ToggleMenu(menu) => {
                self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
            }
            Message::CloseMenu => self.close_menus(),
            Message::Escape => {
                if self.menu.is_some() || self.drawer || self.night_card.is_some() {
                    self.close_menus();
                } else if self.editor.is_some() {
                    return self.update(Message::Editor(editor::Message::Escape));
                }
            }
            Message::ToggleDrawer => {
                self.menu = None;
                self.drawer = !self.drawer;
            }
            Message::ToggleSidebar => {
                if !shell::has_sidebar(self.class()) {
                    return self.update(Message::ToggleDrawer);
                }
                self.close_menus();
                self.sidebar = self.sidebar.toggled();
                // The demo stores nothing.
                if !self.source.is_demo() {
                    return ui_state::save_task(AppId::CALENDAR, UiState { sidebar: self.sidebar });
                }
            }
            Message::UiState(state) => self.sidebar = state.sidebar,
            Message::OpenSettings => {
                self.close_menus();
                connect::open_settings();
            }
            // Nothing to show in the window: the other app either comes up or it does not.
            Message::AppOpened(Err(err)) => eprintln!("torrocalendar: cannot open the app: {err}"),
            Message::AppOpened(Ok(())) => {}
            Message::Resized(size) => {
                let before = self.class();
                self.size = size;
                // Another layout may need other days (the narrow day loads its week for the strip,
                // mini and strip load from today on), and it builds a different widget tree, in
                // which Iced drops the time grid's scroll position: load again and open the grid at
                // the morning instead of midnight.
                if self.class() != before && self.loaded {
                    return self.load().chain(self.scroll_to_morning());
                }
            }
            Message::Refresh => {
                let source = self.source.clone();
                let sync =
                    Task::perform(async move { source.sync().await.map_err(|e| e.to_string()) }, |_| Message::Tick);
                return sync.chain(self.load());
            }
            Message::ToggleAccount(id) => {
                if !self.folded.remove(&id) {
                    self.folded.insert(id);
                }
            }
            Message::ToggleCollection(id) => {
                if !self.hidden.remove(&id) {
                    self.hidden.insert(id);
                }
            }
            Message::NewEventHere => {
                let time = Some(NaiveTime::MIN + Days::hours(9));
                return self.update(Message::NewEvent { date: self.anchor, time });
            }
            Message::NewEvent { date, time } => {
                let draft = Editor::new_event(&self.writable_calendars(), date, time);
                // The title is what a new event needs first.
                return Task::batch([self.open_editor(draft), iced::widget::operation::focus(editor::TITLE_ID)]);
            }
            Message::OpenEvent(index) => {
                let Some(event) = self.events.get(index) else { return Task::none() };
                // A second click on the open event keeps what is typed.
                if self.editor.as_ref().and_then(Editor::event).is_some_and(|open| same_event(open, event)) {
                    return Task::none();
                }
                let editor = Editor::edit(event, &self.writable_calendars());
                return self.open_editor(editor);
            }
            Message::ShowDay(date) => {
                self.view = CalView::Day;
                return self.go_to(date);
            }
            Message::Editor(message) => {
                let Some(editor) = self.editor.as_mut() else { return Task::none() };
                let action = editor.update(message);
                return self.editor_action(action);
            }
            Message::Saved(edit, result) => {
                let current = edit == self.edits;
                match result {
                    Ok(saved) => {
                        // The editor keeps editing on top of the stored version, with its new etag,
                        // and sends what was typed meanwhile.
                        let follow_up = match self.editor.as_mut() {
                            Some(editor) if current && editor.awaits(&saved) => editor.stored(&saved),
                            _ => editor::Action::None,
                        };
                        return Task::batch([self.editor_action(follow_up), self.load()]);
                    }
                    Err(err) => {
                        if let Some(editor) = self.editor.as_mut().filter(|_| current) {
                            editor.save_failed();
                        }
                        self.error = Some(trf("Save failed: {error}", &[("error", &err)]));
                    }
                }
            }
            Message::Deleted(result) => match result {
                Ok(()) => return self.load(),
                Err(err) => self.error = Some(trf("Delete failed: {error}", &[("error", &err)])),
            },
            // Like the other apps: a browser that cannot be started is only logged.
            Message::LinkOpened(Ok(())) => {}
            Message::LinkOpened(Err(err)) => eprintln!("torrocalendar: cannot open the link in a browser: {err}"),
            Message::Tick => {
                self.now = self.launch.now.unwrap_or_else(|| Local::now().naive_local());
                let appearance =
                    Task::batch([self.fetch_appearance(), self.fetch_night(), self.zoom.fetch().map(Message::Zoom)]);
                // The accounts come with every tick: one may have been added or removed.
                let accounts = self.fetch_account();
                if !self.connected() {
                    return Task::batch([appearance, accounts]);
                }
                if self.editor.is_none() {
                    return Task::batch([appearance, accounts, self.load()]);
                }
                return Task::batch([appearance, accounts]);
            }
            Message::SystemMode(mode) => self.system_mode = mode,
            Message::Appearance(Ok(appearance)) => self.appearance = appearance,
            // The core may be starting up; keep the colours we have.
            Message::Appearance(Err(_)) => {}
            // The canvases (week, timeline, radar) keep no cache: the next frame draws with the new
            // design, the task loads its fonts.
            Message::Design(message) => {
                if let Some(task) = self.design.update(message) {
                    return task.map(Message::Design);
                }
            }
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
            Message::Timeline(change) => {
                let (first, after) = self.range();
                return self.timeline.update(change, first, after, self.night);
            }
            Message::EditNight { fold, at } => {
                if let Some(evening) = fold {
                    let (first, after) = self.range();
                    let _ =
                        self.timeline.update::<Message>(timeline::Change::FoldNight(evening), first, after, self.night);
                }
                self.close_menus();
                self.night_card = Some(NightCard { at, from: self.night.from, to: self.night.to });
            }
            Message::NightFrom(from) => {
                if let Some(card) = self.night_card.as_mut() {
                    card.from = from;
                }
            }
            Message::NightTo(to) => {
                if let Some(card) = self.night_card.as_mut() {
                    card.to = to;
                }
            }
            Message::SaveNight => {
                let Some(card) = self.night_card.take() else { return Task::none() };
                return self.set_night(Night { from: card.from, to: card.to });
            }
            Message::NoNight => {
                let Some(card) = self.night_card.take() else { return Task::none() };
                return self.set_night(Night::none_at(card.from));
            }
            Message::CloseNight => self.night_card = None,
            // While this window's own change is on its way, the stored night is the old one.
            Message::Night(Ok(night)) if !self.night_saving => self.night = night,
            // Stale, or the core is starting up: keep the night we have.
            Message::Night(_) => {}
            Message::NightSaved(result) => {
                self.night_saving = false;
                if let Err(err) = result {
                    self.error = Some(trf("Save failed: {error}", &[("error", &err)]));
                    return self.fetch_night();
                }
            }
        }
        Task::none()
    }

    /// The window with the zoom badge over it.
    pub fn view(&self) -> Element<'_, Message> {
        let content = self.zoom.view(self.content());
        let (surface, cover, buttons) = (self.surface(), self.cover(), self.title_buttons());
        widgets::ctrl_guard(shell::titlebar(content, self.zoom.factor(), surface, cover, buttons, Message::DragWindow))
    }

    /// The panel button and "Settings" in the band above the window (U.10). Signing in and the
    /// glance have no sidebar to toggle, nor room for the buttons.
    fn title_buttons(&self) -> TitleButtons<Message> {
        let class = self.class();
        if !self.connected() || is_glance(class) {
            return TitleButtons::none();
        }
        let label = shell::sidebar_button_label(class, self.sidebar, self.drawer);
        TitleButtons::none().sidebar(label, Message::ToggleSidebar).settings(Message::OpenSettings)
    }

    /// What lies over the top of the window, which the title band repeats: the editor's panel over
    /// the view, else the drawer. Nothing while signing in or in a glance, which have neither.
    fn cover(&self) -> Cover {
        let class = self.class();
        if !self.connected() || is_glance(class) {
            return Cover::Nothing;
        }
        Cover::panel(class, self.editor.is_some()).or(Cover::drawer(class, self.drawer))
    }

    /// The surface at the top of [`App::content`], which the title band takes: the window background
    /// while signing in and for the glance's loading and error state, else that of the layout.
    fn surface(&self) -> Surface {
        let class = self.class();
        let connected = matches!(self.account, Some(AccountState::Connected { .. }));
        let bare_glance = is_glance(class) && (self.error.is_some() || !self.loaded);
        if !connected || bare_glance { Surface::Window } else { Surface::of(class) }
    }

    fn content(&self) -> Element<'_, Message> {
        match &self.account {
            None => {
                return match &self.error {
                    Some(err) => widgets::error_state(err.as_str(), Message::Retry),
                    None => widgets::loading(),
                };
            }
            Some(account @ (AccountState::NotConnected | AccountState::SigningIn { .. })) => {
                return self.connect.view(account, tr("Calendar")).map(Message::Connect);
            }
            Some(AccountState::Connected { .. }) => {}
        }
        let class = self.class();
        if is_glance(class) {
            // An open editor stays in the state and comes back with a larger window.
            return self.glance(class);
        }
        self.window(class)
    }

    /// Mini or strip: the error and the loading state as in every layout, then the glance at
    /// today and the days after it (`views::glance`).
    fn glance(&self, class: SizeClass) -> Element<'_, Message> {
        if let Some(err) = &self.error {
            return widgets::error_state(err.as_str(), Message::Retry);
        }
        if !self.loaded {
            return widgets::loading();
        }
        let (start, end) = self.load_range();
        let data = ViewData { items: self.items(), start, end, anchor: self.now.date(), now: self.now, selected: None };
        match class {
            SizeClass::Strip => glance::strip(&data, self.content_size()),
            _ => glance::mini(&data, self.content_size()),
        }
    }

    /// The whole window in the layout for `class`: sidebar, header and the current view in the
    /// shell, the side panel beside or over the view, the drawer and the open menu above it.
    fn window(&self, class: SizeClass) -> Element<'_, Message> {
        let (switcher, views, field) = (Anchor::new(), Anchor::new(), Anchor::new());
        // The night's card floats beside a moon on the view's canvas, or under "Nights".
        let (night_view, night_button) = (Anchor::new(), Anchor::new());
        let menu = self.menu.and_then(|menu| match menu {
            Menu::Switcher => Some(widgets::floating(
                &switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(AppId::CALENDAR, Message::Switcher),
                Message::CloseMenu,
            )),
            // Only the medium and narrow header have the "Week ▾" button the menu hangs from.
            Menu::View => (!shell::has_sidebar(class))
                .then(|| widgets::floating(&views, Placement::Below(Align::End), self.view_menu(), Message::CloseMenu)),
        });
        let night = self.night_card.map(|card| {
            let (anchor, placement) = match card.at {
                Some(moon) => (night_view.area(moon), Placement::Beside),
                None => (night_button.clone(), Placement::Below(Align::End)),
            };
            widgets::floating(&anchor, placement, night_card(card), Message::CloseNight)
        });
        // One layer at a time: the window's menus, the night's card, else the calendar menu inside
        // the panel.
        let layer =
            menu.or(night).or_else(|| self.editor.as_ref().and_then(|editor| editor.layer(&field, Message::Editor)));
        let detail = self.editor.as_ref().map(|editor| editor.panel(&field, Message::Editor));
        // Beside the view the panel takes the place of the properties column ("Up next").
        let props = if self.panel_beside(class) { None } else { self.props(class) };
        let placed = panel::place(class, self.body(&night_view), detail, layer);
        let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
            .then(|| self.sidebar(SidebarMode::Rail, class, &switcher));
        let sidebar = self.sidebar(SidebarMode::Expanded, class, &switcher);
        shell::shell(class, sidebar, self.header(class, &views, &night_button), placed.body, props)
            .rail(rail)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(placed.overlay)
            .into()
    }

    /// The side panel is open and stands beside the view (the wide layout).
    fn panel_beside(&self, class: SizeClass) -> bool {
        self.editor.is_some() && panel::mode_for(class) == PanelMode::Beside
    }

    /// The header bar (`calendar.html`, `.header`):
    ///
    /// - **Wide:** "October 2026 Week 41", "Today", ‹ ›; at the right the views as a segmented
    ///   control, refresh and "New event". The timeline adds "Nights" before the views; without a
    ///   night it opens the card "Night" (anchored at `night_button`) instead.
    /// - **Medium:** the views (and calendars) in "Week ▾"; the rail beside it has the panel button.
    ///   The timeline's "Nights" is the moon alone.
    /// - **Narrow:** without the note and "Today" (the small month in the drawer and the T key go
    ///   to today), the day as "Wed, 7 Oct", and "New event" reduced to "+". "Day ▾" names the
    ///   view shown ([`App::shown`]).
    fn header<'a>(&'a self, class: SizeClass, views: &Anchor, night_button: &Anchor) -> Element<'a, Message> {
        let (title, note) = self.title_parts();
        let prev = widgets::icon_button(Icon::ChevronLeft, tr("Previous"), Some(Message::Prev));
        let next = widgets::icon_button(Icon::ChevronRight, tr("Next"), Some(Message::Next));
        let refresh = widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::Refresh));
        let new_event = Message::NewEvent { date: self.anchor, time: Some(NaiveTime::MIN + Days::hours(9)) };
        let select = || widgets::select_button(views, self.shown().label(), Message::ToggleMenu(Menu::View));
        if class == SizeClass::Wide || class == SizeClass::Medium {
            let today = widgets::text_button(None, tr("Today"), style::button::ghost, Some(Message::Today));
            let title = row![
                shell::header_title(None, title, note),
                row![today, prev, next].spacing(space::xs()).align_y(Alignment::Center)
            ]
            .spacing(space::l())
            .align_y(Alignment::Center);
            let primary =
                widgets::text_button(Some(Icon::Plus), tr("New event"), style::button::primary, Some(new_event));
            let mut trailing = Vec::new();
            if self.shown() == CalView::Timeline {
                // Without a night there is nothing to unfold, and no moon to double-click.
                let nights = Some(if self.night.is_none() {
                    Message::EditNight { fold: None, at: None }
                } else {
                    Message::Timeline(timeline::Change::ToggleNights)
                });
                let button = if class == SizeClass::Wide {
                    widgets::text_button(Some(Icon::Moon), tr("Nights"), style::button::ghost, nights)
                } else {
                    widgets::icon_button(Icon::Moon, tr("Nights"), nights)
                };
                trailing.push(widgets::anchored(night_button, button));
            }
            if class == SizeClass::Wide {
                let segments = CalView::ALL.map(|view| (view.label(), Message::SetView(view)));
                let active = CalView::ALL.iter().position(|view| *view == self.view).unwrap_or_default();
                trailing.push(widgets::segmented(segments, active));
            } else {
                trailing.push(select());
            }
            trailing.extend([refresh, primary]);
            shell::header_bar([], title, trailing)
        } else {
            // The short date: the long one ("Monday, 12 October 2026") squeezes the buttons.
            let title = match self.shown() {
                CalView::Day => short_date(self.anchor),
                CalView::Radar => short_date(self.radar_start().date()),
                _ => title,
            };
            let title = row![shell::header_title(None, title, None), row![prev, next].spacing(space::xs())]
                .spacing(space::s())
                .align_y(Alignment::Center);
            let primary = widgets::compact_button(Icon::Plus, tr("New event"), style::button::primary, Some(new_event));
            // Where the band has the panel button (macOS), the header leaves it out.
            let toggle = (!shell::has_band()).then(|| shell::sidebar_toggle(Message::ToggleDrawer));
            shell::header_bar(toggle, title, [select(), refresh, primary])
        }
    }

    /// The sidebar (`calendar.html`, `.sidebar`): app switcher, small month, the views with their
    /// keys, the calendars with their colour, visibility and number of loaded events, and at the
    /// bottom "Open Tasks ↗" and "Settings" (the latter in the band on macOS).
    ///
    /// As a drawer (medium, narrow) it leaves out the views: they are in the header's "Week ▾"
    /// menu already. As the rail (`mode`, U.2) it leaves out the small month, which has no icon
    /// form, and keeps the views as icons. What does not fit the window's height scrolls between
    /// the switcher and the footer ([`shell::sidebar`], U.12).
    fn sidebar<'a>(&'a self, mode: SidebarMode, class: SizeClass, switcher: &Anchor) -> Element<'a, Message> {
        let mut items = Vec::new();
        if mode == SidebarMode::Expanded {
            let shown = match self.shown() {
                CalView::Day | CalView::Week => Some(self.range()),
                // The radar's day, though its window reaches into the days around it.
                CalView::Radar => Some((self.anchor, self.anchor + Days::days(1))),
                _ => None,
            };
            items.push(mini_month::view(self.mini_month, shown, self.now.date()));
        }
        if shell::has_sidebar(class) || mode == SidebarMode::Rail {
            items.push(widgets::sidebar_group_in(mode, tr("Views"), None));
            items.extend(CalView::ALL.map(|view| {
                widgets::sidebar_item_in(
                    mode,
                    view.icon(),
                    view.label(),
                    Trail::Kbd(view.key()),
                    view == self.view,
                    Message::SetView(view),
                )
            }));
        }
        let row = |(calendar, colour, count): (&'a Collection, Color, usize)| {
            widgets::sidebar_item_in(
                mode,
                Mark::Check(colour, !self.hidden.contains(&calendar.id)),
                calendar.name.as_str(),
                Trail::Count(count),
                false,
                Message::ToggleCollection(calendar.id.clone()),
            )
        };
        match shell::account_groups(&self.accounts, self.calendars(), |c| c.0.id.as_str()) {
            // One account: as it always was, a flat list under "Calendars".
            Err(calendars) => {
                if !calendars.is_empty() {
                    items.push(widgets::sidebar_group_in(mode, tr("Calendars"), None));
                    items.extend(calendars.into_iter().map(row));
                }
            }
            Ok(groups) => {
                let shown: Vec<&PimAccount> = groups.iter().filter_map(|group| group.account).collect();
                for group in groups {
                    let Some(account) = group.account else {
                        items.push(widgets::sidebar_group_in(mode, tr("Calendars"), None));
                        items.extend(group.items.into_iter().map(row));
                        continue;
                    };
                    let unfolded = !self.folded.contains(&account.id);
                    let toggle = Some((unfolded, Message::ToggleAccount(account.id.clone())));
                    items.push(widgets::sidebar_group_in(mode, group_title(account, &shown), toggle));
                    // The rail has no title to unfold with, so it shows every calendar.
                    if unfolded || mode == SidebarMode::Rail {
                        items.extend(group.items.into_iter().map(row));
                    }
                }
            }
        }
        let mut footer = vec![widgets::sidebar_item_in(
            mode,
            Icon::SquareCheck,
            tr("Open Tasks"),
            Trail::Icon(Icon::ArrowUpRight),
            false,
            Message::Switcher(Switcher::OpenApp(AppId::TASKS.binary)),
        )];
        // Where the band has it (macOS, U.10), "Settings" sits in its top right corner.
        if !shell::has_band() {
            footer.push(widgets::sidebar_item_in(
                mode,
                Icon::Settings,
                tr("Settings"),
                Trail::None,
                false,
                Message::OpenSettings,
            ));
        }
        let head = shell::sidebar_head(mode, AppId::CALENDAR, switcher, Message::Switcher, Message::ToggleSidebar);
        shell::sidebar(head, items, footer)
    }

    /// The menu under "Week ▾": the views with their keys, then the calendars with a check box
    /// that shows and hides them (the menu stays open for that).
    fn view_menu(&self) -> Element<'_, Message> {
        let views = CalView::ALL
            .map(|view| {
                MenuItem::new(view.icon(), view.label(), Message::SetView(view))
                    .trail(Trail::Kbd(view.key()))
                    .current(view == self.shown())
            })
            .to_vec();
        let mut sections = vec![MenuSection::titled(tr("View"), views)];
        let item = |entry| self.calendar_item(entry);
        match shell::account_groups(&self.accounts, self.calendars(), |c| c.0.id.as_str()) {
            Err(calendars) => {
                let calendars: Vec<_> = calendars.into_iter().map(item).collect();
                if !calendars.is_empty() {
                    sections.push(MenuSection::titled(tr("Calendars"), calendars));
                }
            }
            // A menu has no folding: every account is a section named after it.
            Ok(groups) => {
                let shown: Vec<&PimAccount> = groups.iter().filter_map(|group| group.account).collect();
                sections.extend(groups.into_iter().map(|group| {
                    let title = group.account.map_or_else(|| tr("Calendars"), |account| group_title(account, &shown));
                    MenuSection::titled(title, group.items.into_iter().map(item).collect())
                }))
            }
        }
        widgets::menu(sections)
    }

    /// A calendar as a menu entry: check box in its colour, name, number of loaded events.
    fn calendar_item<'a>(&self, (calendar, colour, count): (&'a Collection, Color, usize)) -> MenuItem<'a, Message> {
        MenuItem::new(
            Mark::Check(colour, !self.hidden.contains(&calendar.id)),
            calendar.name.as_str(),
            Message::ToggleCollection(calendar.id.clone()),
        )
        .trail(Trail::Count(count))
    }

    /// The event calendars with their colour and how many of the loaded events they hold.
    fn calendars(&self) -> Vec<(&Collection, Color, usize)> {
        self.collections
            .iter()
            .filter(|c| c.kind.has_events())
            .map(|c| {
                let count = self.events.iter().filter(|e| e.collection == c.id).count();
                (c, collection_color(c.color.as_deref()), count)
            })
            .collect()
    }

    /// The content under the header: the error with "Retry", the calm loading state before the
    /// first events arrive, then the current view; the timeline and the radar anchor their canvas
    /// at `night` for the night's card.
    fn body(&self, night: &Anchor) -> Element<'_, Message> {
        if let Some(err) = &self.error {
            return widgets::error_state(err.as_str(), Message::Retry);
        }
        if !self.loaded {
            return widgets::loading();
        }
        self.current_view(night)
    }

    /// The properties column of the wide layout: what is left of the day beside the day view
    /// (`views::week::day_props`), the radar's mini radars of the coming days beside the radar. The
    /// other views fill the width and have none.
    fn props(&self, class: SizeClass) -> Option<Element<'_, Message>> {
        if !shell::has_props(class) || !self.loaded || self.error.is_some() {
            return None;
        }
        match self.shown() {
            CalView::Day => Some(views::week::day_props(&self.view_data())),
            CalView::Radar => Some(views::radar::list(&self.view_data(), self.coming, self.night)),
            _ => None,
        }
    }

    /// What the current view gets to draw.
    fn view_data(&self) -> ViewData<'_> {
        let (start, end) = self.range();
        ViewData { items: self.items(), start, end, anchor: self.anchor, now: self.now, selected: self.selected() }
    }

    /// The index in [`App::events`] of the event open in the side panel; `None` for none or a
    /// draft. Found again after every reload, which renumbers the events.
    fn selected(&self) -> Option<usize> {
        let open = self.editor.as_ref()?.event()?;
        self.events.iter().position(|event| same_event(event, open))
    }

    /// Opens `editor` in the side panel. A change still waiting in the editor open before is saved.
    fn open_editor(&mut self, editor: Editor) -> Task<Message> {
        let leave = self.close_editor();
        self.edits += 1;
        self.editor = Some(editor);
        leave
    }

    /// Closes the side panel; a change that still waits to be saved is saved.
    fn close_editor(&mut self) -> Task<Message> {
        match self.editor.take().and_then(|editor| editor.leave()) {
            Some(event) => self.save(event),
            None => Task::none(),
        }
    }

    /// Carries out what the editor asked for.
    fn editor_action(&mut self, action: editor::Action) -> Task<Message> {
        match action {
            editor::Action::None => Task::none(),
            editor::Action::Close => self.close_editor(),
            // The first field with a problem, after Enter.
            editor::Action::Focus(id) => iced::widget::operation::focus(id),
            editor::Action::Save(event) => self.save(event),
            // Checked again here: only web and mail addresses ever reach the browser.
            editor::Action::OpenLink(url) if links::openable(&url) => {
                Task::perform(async move { webbrowser::open(&url).map_err(|err| err.to_string()) }, Message::LinkOpened)
            }
            editor::Action::OpenLink(_) => Task::none(),
            editor::Action::Copy(text) => iced::clipboard::write(text),
            editor::Action::Delete(event) => {
                self.editor = None;
                let source = self.source.clone();
                Task::perform(
                    async move { source.delete_event(event).await.map_err(|e| e.to_string()) },
                    Message::Deleted,
                )
            }
        }
    }

    /// Saves `event` (the core's `save_event`) for the editor open now.
    fn save(&self, event: Event) -> Task<Message> {
        let source = self.source.clone();
        let edit = self.edits;
        Task::perform(async move { source.save_event(event).await.map_err(|e| e.to_string()) }, move |result| {
            Message::Saved(edit, result)
        })
    }

    fn current_view(&self, night: &Anchor) -> Element<'_, Message> {
        let data = self.view_data();
        match self.shown() {
            CalView::Day if self.class() == SizeClass::Narrow => {
                column![week_strip::view(&data), views::week::day_under_strip(data)].into()
            }
            // One day is not stretched over a wide window: it keeps a list's reading width at the
            // sidebar (design skill, section 6), and the properties column says what comes next.
            CalView::Day if shell::has_props(self.class()) => {
                container(views::week::view(data)).max_width(size::content_max()).into()
            }
            CalView::Day | CalView::Week => views::week::view(data),
            CalView::Month => views::month::view(data),
            CalView::Agenda => views::agenda::view(data, self.class()),
            CalView::Timeline => views::timeline::view(data, &self.timeline, self.night, night),
            // Narrow: under the week strip, whose days page the radar like the small month does –
            // unless the tile is so low that the strip would cut into the dial.
            CalView::Radar if self.class() == SizeClass::Narrow => {
                let (order, turn, coming, anchor) = (self.calendar_order(), self.turn, self.coming, night.clone());
                responsive(move |bounds| {
                    let below = views::radar::ListAt::Below;
                    let radar = views::radar::view(data.clone(), &order, turn, coming, below, self.night, &anchor);
                    if views::radar::strip_fits(bounds) {
                        column![week_strip::view(&data), radar].into()
                    } else {
                        radar
                    }
                })
                .into()
            }
            CalView::Radar => {
                let list = match self.class() {
                    SizeClass::Wide => views::radar::ListAt::Props,
                    _ => views::radar::ListAt::Beside,
                };
                views::radar::view(data, &self.calendar_order(), self.turn, self.coming, list, self.night, night)
            }
        }
    }

    /// The visible event calendars in the order of the sidebar, with their names, colours and the
    /// hosts of their accounts: the radar's rings.
    fn calendar_order(&self) -> Vec<views::radar::Ring> {
        let visible = self.calendars().into_iter().filter(|(calendar, _, _)| !self.hidden.contains(&calendar.id));
        visible
            .map(|(calendar, colour, _)| views::radar::Ring {
                id: calendar.id.clone(),
                name: calendar.name.clone(),
                host: account_of(&calendar.id, &self.accounts, |a| a.id.as_str()).map(|a| a.host().to_string()),
                colour,
            })
            .collect()
    }

    /// Where the radar's window starts: the start of the anchor's day turned by [`App::turn`].
    fn radar_start(&self) -> NaiveDateTime {
        views::radar::start(&self.view_data(), self.turn, self.night)
    }

    /// The loaded events of the shown calendars, with their calendar's colour.
    fn items(&self) -> Vec<Item<'_>> {
        let colors: HashMap<&str, iced::Color> =
            self.collections.iter().map(|c| (c.id.as_str(), collection_color(c.color.as_deref()))).collect();
        self.events
            .iter()
            .enumerate()
            .filter(|(_, e)| !self.hidden.contains(&e.collection))
            .map(|(index, event)| Item {
                index,
                event,
                color: colors.get(event.collection.as_str()).copied().unwrap_or(collection_color(None)),
            })
            .collect()
    }

    /// The layout class of the window.
    fn class(&self) -> SizeClass {
        layout::of(self.size)
    }

    /// The room under the title band, which the glances fill ([`shell::below_titlebar`]).
    fn content_size(&self) -> Size {
        shell::below_titlebar(self.size, self.zoom.factor())
    }

    /// The view on screen: the chosen one, or what the narrow layout shows for it ([`shown_in`]).
    fn shown(&self) -> CalView {
        shown_in(self.view, self.class())
    }

    /// The days [`App::load`] fetches: those the view shows; for the narrow day its whole week,
    /// for the dots of the week strip; for mini and strip today and the [`AGENDA_DAYS`] after it,
    /// so the glance finds the next event when today is done.
    fn load_range(&self) -> (NaiveDate, NaiveDate) {
        match self.class() {
            SizeClass::Mini | SizeClass::Strip => {
                let today = self.now.date();
                (today, today + Days::days(AGENDA_DAYS))
            }
            SizeClass::Narrow if self.shown() == CalView::Day => {
                let start = week_start(self.anchor);
                (start, start + Days::days(7))
            }
            // The radar's days and the week of its strip.
            SizeClass::Narrow if self.shown() == CalView::Radar => {
                let (first, after) = self.range();
                let start = week_start(self.anchor);
                (first.min(start), after.max(start + Days::days(7)))
            }
            // The last night of the timeline runs into the morning after its last day.
            _ if self.shown() == CalView::Timeline => {
                let (start, end) = self.range();
                (start, end + Days::days(1))
            }
            _ => self.range(),
        }
    }

    /// First and last+1 day the current view shows.
    fn range(&self) -> (NaiveDate, NaiveDate) {
        match self.shown() {
            CalView::Day => (self.anchor, self.anchor + Days::days(1)),
            CalView::Week => {
                let start = week_start(self.anchor);
                (start, start + Days::days(7))
            }
            CalView::Month => {
                let first = self.anchor.with_day(1).unwrap_or(self.anchor);
                let start = week_start(first);
                (start, start + Days::days(42))
            }
            CalView::Agenda => (self.anchor, self.anchor + Days::days(AGENDA_DAYS)),
            CalView::Timeline => (self.anchor - Days::days(1), self.anchor + Days::days(TIMELINE_DAYS)),
            // The day and every window its dial can turn to – from the evening before (twelve hours
            // back from 06:00) to the noon two days on (a day ahead from late tonight, twelve
            // hours long) – and the page of its mini radars, so they are full without loading on
            // their own (a browsed page may lie apart from the anchor: the range spans both). The
            // anchor's own events are always in, so its start does not jump.
            CalView::Radar => {
                let after = self.coming + Days::days(views::radar::COMING_DAYS);
                ((self.anchor - Days::days(1)).min(self.coming), (self.anchor + Days::days(3)).max(after))
            }
        }
    }

    /// The anchor one view-length back (`-1`) or ahead (`1`).
    fn step(&self, direction: i64) -> NaiveDate {
        match self.shown() {
            CalView::Day => self.anchor + Days::days(direction),
            CalView::Week => self.anchor + Days::days(7 * direction),
            CalView::Month => mini_month::shift(self.anchor, direction as i32),
            CalView::Agenda => self.anchor + Days::days(AGENDA_DAYS * direction),
            CalView::Timeline => self.anchor + Days::days(TIMELINE_DAYS * direction),
            CalView::Radar => self.anchor + Days::days(direction),
        }
    }

    fn heading(&self) -> String {
        match self.shown() {
            CalView::Day => format_long_date(self.anchor),
            CalView::Radar => format_long_date(self.radar_start().date()),
            CalView::Week => {
                let start = week_start(self.anchor);
                trf(
                    "Week {week} · {month} {year}",
                    &[
                        ("week", &start.iso_week().week()),
                        ("month", &month_name(start.month())),
                        ("year", &start.year()),
                    ],
                )
            }
            CalView::Month => format!("{} {}", month_name(self.anchor.month()), self.anchor.year()),
            CalView::Agenda => trf("From {date}", &[("date", &format_long_date(self.anchor))]),
            CalView::Timeline => trf(
                "Week {week} · {month} {year}",
                &[
                    ("week", &self.anchor.iso_week().week()),
                    ("month", &month_name(self.anchor.month())),
                    ("year", &self.anchor.year()),
                ],
            ),
        }
    }

    /// The header title and its quiet note: "October 2026" and "Week 41" for the week (the month
    /// of its Thursday, as ISO weeks count) and for the timeline (the anchor's), the long date for
    /// a day, the month otherwise.
    fn title_parts(&self) -> (String, Option<String>) {
        let month_year = |date: NaiveDate| format!("{} {}", month_name(date.month()), date.year());
        match self.shown() {
            CalView::Day => (format_long_date(self.anchor), None),
            CalView::Radar => (format_long_date(self.radar_start().date()), None),
            CalView::Week => {
                let thursday = week_start(self.anchor) + Days::days(3);
                (month_year(thursday), Some(trf("Week {week}", &[("week", &thursday.iso_week().week())])))
            }
            CalView::Month | CalView::Agenda => (month_year(self.anchor), None),
            CalView::Timeline => {
                (month_year(self.anchor), Some(trf("Week {week}", &[("week", &self.anchor.iso_week().week())])))
            }
        }
    }

    /// Moves the view to `date` (the small month follows) and loads it. The radar opens the new
    /// day at its start, its dial unturned; its mini radars keep their page while `date` is on it.
    fn go_to(&mut self, date: NaiveDate) -> Task<Message> {
        self.anchor = date;
        self.turn = 0;
        self.coming = views::radar::coming_page(self.coming, date);
        self.mini_month = mini_month::shift(date, 0);
        self.load().chain(self.scroll_to_morning())
    }

    fn close_menus(&mut self) {
        self.menu = None;
        self.drawer = false;
        self.night_card = None;
    }

    /// Day and week open at 07:00 instead of midnight; the timeline with now (or the time of now
    /// on the anchor's day) at 30 % of its width.
    fn scroll_to_morning(&mut self) -> Task<Message> {
        match self.shown() {
            CalView::Day | CalView::Week => views::week::scroll_to_morning(),
            CalView::Month | CalView::Agenda | CalView::Radar => Task::none(),
            CalView::Timeline => {
                let (first, after) = self.range();
                self.timeline.show(self.anchor.and_time(self.now.time()), first, after, self.night)
            }
        }
    }

    fn connected(&self) -> bool {
        matches!(self.account, Some(AccountState::Connected { .. }))
    }

    /// All event calendars; the editor offers the writable ones and opens events of the
    /// others read-only.
    fn writable_calendars(&self) -> Vec<Collection> {
        self.collections.iter().filter(|c| c.kind.has_events()).cloned().collect()
    }

    fn fetch_account(&self) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { source.accounts().await.map_err(|e| e.to_string()) }, Message::AccountLoaded)
    }

    /// The editor states of `--open`, once the first events are in: an event needs data to open.
    fn open_launched(&mut self) {
        if std::mem::replace(&mut self.opened, true) {
            return;
        }
        let event = match self.launch.open {
            Some(Open::Editor) => self.events.iter().find(|event| !event.recurring).or(self.events.first()),
            Some(Open::EditorReadOnly) => self.events.iter().find(|event| event.recurring).or(self.events.first()),
            Some(Open::EditorNew) => {
                let _ = self.update(Message::NewEventHere);
                None
            }
            Some(Open::Night) => {
                let _ = self.update(Message::EditNight { fold: None, at: None });
                None
            }
            _ => None,
        };
        if let Some(editor) = event.map(|event| Editor::edit(event, &self.writable_calendars())) {
            let _ = self.open_editor(editor);
        }
    }

    fn load(&mut self) -> Task<Message> {
        self.loading = true;
        // The forced states: loading never ends, the error comes at once, empty has no events.
        match self.launch.open {
            Some(Open::Loading) => return Task::none(),
            Some(Open::Error) => return Task::done(Message::Loaded(Err(DEMO_ERROR.into()))),
            _ => {}
        }
        let empty = self.launch.open == Some(Open::Empty);
        let (start, end) = self.load_range();
        let (start, end) = (to_utc(start.and_time(NaiveTime::MIN)), to_utc(end.and_time(NaiveTime::MIN)));
        let source = self.source.clone();
        Task::perform(
            async move {
                let collections = source.collections().await.map_err(|e| e.to_string())?;
                let events =
                    if empty { Vec::new() } else { source.events(start, end).await.map_err(|e| e.to_string())? };
                Ok((collections, events))
            },
            Message::Loaded,
        )
    }
}

/// The floating card "Night" (`widgets::popover`): the moon and "Calendar" with ✕ at the top, the
/// title, "From" and "Until" as lists of the quarter hours, "Save" and "No night" at the bottom. The
/// same fields as the row "Night" in the control centre.
fn night_card<'a>(card: NightCard) -> Element<'a, Message> {
    let moon = icon::icon(Icon::Moon, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let head = row![
        moon,
        typo::text(typo::meta(), tr("Calendar")).style(theme::text_3),
        blank::horizontal(),
        widgets::icon_button(Icon::X, tr("Close"), Some(Message::CloseNight)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let time = |label: &'a str, value: Quarter, on_pick: fn(Quarter) -> Message| {
        let field = pick_list(Quarter::all().collect::<Vec<_>>(), Some(value), on_pick)
            .padding(style::pick_list_padding())
            .text_size(typo::caption().size)
            .text_line_height(typo::caption().line_height)
            .font(typo::caption().font)
            .style(style::pick_list)
            .menu_style(style::pick_list_menu);
        let row = row![typo::text(typo::body(), label).style(theme::text_2), blank::horizontal(), field]
            .align_y(Alignment::Center);
        widgets::prop_row(None, row, None)
    };
    let rows = vec![time(tr("From"), card.from, Message::NightFrom), time(tr("Until"), card.to, Message::NightTo)];
    let actions = vec![
        widgets::text_button(None, tr("Save"), style::button::secondary, Some(Message::SaveNight)),
        widgets::text_button(None, tr("No night"), style::button::ghost, Some(Message::NoNight)),
    ];
    widgets::popover(head, tr("Night"), rows, actions)
}

/// Whether `a` and `b` are the same event: the same uid in the same calendar, and for the
/// occurrences of a series (which share their uid) the same start.
fn same_event(a: &Event, b: &Event) -> bool {
    a.uid == b.uid && a.collection == b.collection && (!a.recurring || a.start == b.start)
}

/// The title of an account's group in the sidebar and the menu (task K.31): its host, which is
/// short and still tells accounts apart. Only when another shown account has the same host, the
/// full address, so the two groups stay apart. `shown` holds the accounts that have a group.
fn group_title<'a>(account: &'a PimAccount, shown: &[&PimAccount]) -> &'a str {
    let twin = shown.iter().any(|other| other.id != account.id && other.host() == account.host());
    if twin { account.label.as_str() } else { account.host() }
}

/// The error state's text when the account or the events could not be loaded.
fn load_failed(err: &str) -> String {
    trf("The calendar could not be loaded: {error}", &[("error", &err)])
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
        let _ = app.update(Message::Resized(Size::new(1440.0, 900.0)));
        let _ = app.update(Message::ToggleSidebar);
        assert_eq!((app.sidebar, app.drawer), (SidebarMode::Expanded, false), "wide: the choice flips");
        for size in [Size::new(800.0, 600.0), Size::new(420.0, 780.0)] {
            let _ = app.update(Message::Resized(size));
            let _ = app.update(Message::ToggleSidebar);
            assert!(app.drawer, "{size:?}: the panel button opens the drawer");
            let _ = app.update(Message::ToggleSidebar);
            assert!(!app.drawer, "{size:?}: and closes it");
            assert_eq!(app.sidebar, SidebarMode::Expanded, "{size:?}: the wide choice stays");
        }
    }

    /// The message of pressing `key` with `modifiers` while nothing has taken it.
    fn press(key: &str, modifiers: iced::keyboard::Modifiers) -> Option<Message> {
        use iced::keyboard::{Event, Key, Location, key::Physical};
        let press = iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Character(key.into()),
            modified_key: Key::Character(key.into()),
            physical_key: Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: Location::Standard,
            modifiers,
            text: Some(key.into()),
            repeat: false,
        });
        shortcut(press, iced::event::Status::Ignored, iced::window::Id::unique())
    }

    #[test]
    fn the_bracket_toggles_the_sidebar() {
        let message = press("[", iced::keyboard::Modifiers::empty());
        assert!(matches!(message, Some(Message::ToggleSidebar)));
    }

    #[test]
    fn t_goes_to_today_and_shift_t_to_the_timeline() {
        use iced::keyboard::Modifiers;
        assert!(matches!(press("t", Modifiers::empty()), Some(Message::Today)));
        assert!(matches!(press("t", Modifiers::SHIFT), Some(Message::SetView(CalView::Timeline))));
    }

    #[test]
    fn the_timeline_shows_the_day_before_and_seven_after_and_steps_a_week() {
        let mut app = launched(&["--demo", "--now", "2026-10-13T14:20"]);
        let _ = app.update(Message::Resized(WIDE));
        let _ = app.update(Message::SetView(CalView::Timeline));
        assert_eq!(app.range(), (date(2026, 10, 12), date(2026, 10, 20)));
        assert_eq!(app.load_range(), (date(2026, 10, 12), date(2026, 10, 21)), "the last night until 06:00");
        let _ = app.update(Message::Next);
        assert_eq!(app.anchor, date(2026, 10, 20));
        assert_eq!(app.title_parts(), (format!("{} 2026", month_name(10)), Some(trf("Week {week}", &[("week", &43)]))));
        let _ = app.update(Message::Resized(NARROW));
        assert_eq!((app.view, app.shown()), (CalView::Timeline, CalView::Day), "narrow shows the day");
    }

    #[test]
    fn the_timeline_scrolls_to_now_once_its_scrollable_reports_its_width() {
        let mut app = launched(&["--demo", "--now", "2026-10-13T14:20"]);
        let _ = app.update(Message::Resized(WIDE));
        let _ = app.update(Message::SetView(CalView::Timeline));
        let report = Message::Timeline(timeline::Change::Scrolled { offset: 0.0, width: 1100.0 });
        assert!(app.update(report.clone()).units() > 0, "the scroll waited for the width");
        assert_eq!(app.update(report).units(), 0);
        assert!(app.update(Message::Today).units() > 0);
        let _ = app.update(Message::Timeline(timeline::Change::ToggleNights));
        let _ = app.view();
    }

    #[test]
    fn the_window_builds_with_rail_and_drawer_in_every_layout() {
        let mut app = demo_app();
        let _ = app.update(Message::AccountLoaded(Ok(one_account())));
        for sidebar in [SidebarMode::Expanded, SidebarMode::Rail] {
            let _ = app.update(Message::UiState(UiState { sidebar }));
            for size in [Size::new(1440.0, 900.0), Size::new(800.0, 600.0), Size::new(420.0, 780.0)] {
                let _ = app.update(Message::Resized(size));
                let _ = app.update(Message::ToggleDrawer);
                let _ = app.view();
            }
        }
    }

    #[test]
    fn two_accounts_group_the_calendars_and_one_keeps_the_flat_list() {
        let mut app = launched(&["--demo"]);
        let names = |app: &App| {
            let groups = shell::account_groups(&app.accounts, app.calendars(), |c| c.0.id.as_str());
            groups.ok().map(|groups| {
                groups.iter().map(|g| g.items.iter().map(|c| c.0.name.clone()).collect::<Vec<_>>()).collect::<Vec<_>>()
            })
        };
        assert_eq!(
            names(&app),
            Some(vec![vec!["Work".to_string()], vec!["Personal".to_string(), "Family".to_string()]]),
            "the demo's two accounts, each with its calendars"
        );
        let _ = app.update(Message::AccountLoaded(Ok(vec![account(torro_pim::demo::WORK_ACCOUNT)])));
        assert_eq!(names(&app), None, "a single account is not grouped");
    }

    #[test]
    fn an_account_title_folds_and_unfolds_its_calendars() {
        let mut app = launched(&["--demo"]);
        let work = torro_pim::demo::WORK_ACCOUNT.to_string();
        let _ = app.update(Message::ToggleAccount(work.clone()));
        assert!(app.folded.contains(&work));
        // Folding hides rows in the sidebar only; the calendars keep showing their events.
        assert!(app.hidden.is_empty());
        let _ = app.update(Message::ToggleAccount(work.clone()));
        assert!(app.folded.is_empty());
    }

    #[test]
    fn the_grouped_sidebar_and_menu_build_in_every_layout() {
        for accounts in [one_account(), two_accounts()] {
            let mut app = launched(&["--demo"]);
            let _ = app.update(Message::AccountLoaded(Ok(accounts)));
            let _ = app.update(Message::ToggleAccount(torro_pim::demo::HOME_ACCOUNT.into()));
            for sidebar in [SidebarMode::Expanded, SidebarMode::Rail] {
                let _ = app.update(Message::UiState(UiState { sidebar }));
                for size in [Size::new(1440.0, 900.0), Size::new(800.0, 600.0), Size::new(420.0, 780.0)] {
                    let _ = app.update(Message::Resized(size));
                    let _ = app.update(Message::ToggleDrawer);
                    let _ = app.view();
                    let _ = app.view_menu();
                }
            }
        }
    }

    #[test]
    fn the_radar_gets_the_visible_calendars_with_name_colour_and_host() {
        let mut app = launched(&["--demo"]);
        let rings = app.calendar_order();
        let named: Vec<(&str, Option<&str>)> = rings.iter().map(|r| (r.name.as_str(), r.host.as_deref())).collect();
        assert_eq!(
            named,
            [
                ("Work", Some("work.example.org")),
                ("Personal", Some("home.example.org")),
                ("Family", Some("home.example.org"))
            ]
        );
        let work = app.collections.iter().find(|c| c.id == rings[0].id).expect("work");
        assert_eq!(rings[0].colour, collection_color(work.color.as_deref()));
        // A hidden calendar has no ring.
        let _ = app.update(Message::ToggleCollection(rings[1].id.clone()));
        let names: Vec<String> = app.calendar_order().into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["Work", "Family"]);
    }

    #[test]
    fn a_group_is_named_by_its_host_unless_two_shown_accounts_share_it() {
        let anna = account("nextcloud:anna@cloud.example.org");
        let bob = account("nextcloud:bob@imt.example.org");
        let twin = account("nextcloud:bob@cloud.example.org");
        assert_eq!(group_title(&anna, &[&anna, &bob]), "cloud.example.org", "other hosts: the host");
        assert_eq!(group_title(&anna, &[&anna, &twin]), "nextcloud:anna@cloud.example.org", "same host: address");
        assert_eq!(group_title(&twin, &[&anna, &twin]), "nextcloud:bob@cloud.example.org", "both get the address");
        assert_eq!(group_title(&anna, &[&anna]), "cloud.example.org", "an account alone is not its own twin");
        assert_eq!(group_title(&anna, &[]), "cloud.example.org", "no group shown: the host");
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

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    #[test]
    fn the_small_month_browses_without_moving_the_view() {
        let mut app = demo_app();
        let anchor = app.anchor;
        let _ = app.update(Message::MiniMonth(2));
        assert_eq!(app.anchor, anchor, "the main view stays");
        assert_eq!(app.mini_month, mini_month::shift(anchor, 2));
        let _ = app.update(Message::GoTo(date(2026, 3, 18)));
        assert_eq!((app.anchor, app.mini_month), (date(2026, 3, 18), date(2026, 3, 1)), "a day click moves both");
        let _ = app.update(Message::Today);
        assert_eq!(app.mini_month, mini_month::shift(app.now.date(), 0), "the small month follows the anchor");
    }

    #[test]
    fn menus_and_drawer_open_one_at_a_time_and_close_together() {
        let mut app = demo_app();
        let _ = app.update(Message::Switcher(Switcher::Toggle));
        assert_eq!(app.menu, Some(Menu::Switcher));
        let _ = app.update(Message::ToggleMenu(Menu::View));
        assert_eq!(app.menu, Some(Menu::View), "the view menu replaces the switcher");
        let _ = app.update(Message::ToggleDrawer);
        assert_eq!((app.menu, app.drawer), (None, true), "opening the drawer closes the menu");
        let _ = app.update(Message::CloseMenu);
        assert_eq!((app.menu, app.drawer), (None, false));
    }

    #[test]
    fn choosing_a_view_closes_the_menu_and_keeps_it() {
        let mut app = demo_app();
        let _ = app.update(Message::ToggleMenu(Menu::View));
        let _ = app.update(Message::SetView(CalView::Month));
        assert_eq!((app.view, app.menu), (CalView::Month, None));
    }

    #[test]
    fn toggling_a_calendar_keeps_the_menu_open() {
        let mut app = demo_app();
        let _ = app.update(Message::ToggleMenu(Menu::View));
        let _ = app.update(Message::ToggleCollection("work".into()));
        assert_eq!(app.menu, Some(Menu::View));
        assert!(app.hidden.contains("work"));
    }

    #[test]
    fn the_week_title_names_the_month_of_its_thursday_and_the_week() {
        let mut app = demo_app();
        // Week 40 of 2026 runs from Mon 28 Sep to Sun 4 Oct; its Thursday is 1 October.
        app.anchor = date(2026, 9, 29);
        let (title, note) = app.title_parts();
        assert_eq!(title, format!("{} 2026", month_name(10)));
        assert_eq!(note, Some(trf("Week {week}", &[("week", &40)])));
    }

    #[test]
    fn the_layout_follows_the_window_size() {
        let mut app = demo_app();
        let _ = app.update(Message::Resized(Size::new(420.0, 780.0)));
        assert_eq!(layout::of(app.size), SizeClass::Narrow);
        let _ = app.update(Message::Resized(Size::new(1440.0, 900.0)));
        assert_eq!(layout::of(app.size), SizeClass::Wide);
    }

    #[test]
    fn retry_clears_the_error_and_loads_again() {
        let mut app = demo_app();
        let _ = app.update(Message::AccountLoaded(Ok(one_account())));
        let _ = app.update(Message::Loaded(Err("offline".into())));
        assert_eq!(app.error, Some(load_failed("offline")));
        let _ = app.update(Message::Retry);
        assert_eq!((app.error.clone(), app.loading), (None, true));
    }

    const NARROW: Size = Size::new(420.0, 780.0);
    const WIDE: Size = Size::new(1440.0, 900.0);
    const MINI: Size = Size::new(240.0, 160.0);

    #[test]
    fn narrow_shows_a_week_as_a_day_and_a_month_as_the_agenda_and_keeps_the_choice() {
        let mut app = demo_app();
        let _ = app.update(Message::Resized(NARROW));
        assert_eq!((app.view, app.shown()), (CalView::Week, CalView::Day));
        let _ = app.update(Message::SetView(CalView::Month));
        assert_eq!((app.view, app.shown()), (CalView::Month, CalView::Agenda));
        let _ = app.update(Message::Resized(WIDE));
        assert_eq!((app.view, app.shown()), (CalView::Month, CalView::Month), "the month comes back");
    }

    #[test]
    fn the_narrow_day_steps_by_a_day_and_loads_its_week() {
        let mut app = demo_app();
        let _ = app.update(Message::Resized(NARROW));
        app.anchor = date(2026, 10, 7);
        assert_eq!(app.range(), (date(2026, 10, 7), date(2026, 10, 8)));
        assert_eq!(app.step(1), date(2026, 10, 8));
        assert_eq!(app.load_range(), (date(2026, 10, 5), date(2026, 10, 12)), "Monday to Sunday for the strip");
    }

    #[test]
    fn mini_and_strip_load_from_today_and_keep_the_view_and_the_date() {
        let mut app = demo_app();
        let _ = app.update(Message::SetView(CalView::Month));
        app.anchor = date(2026, 3, 18);
        let today = app.now.date();
        for size in [MINI, Size::new(1200.0, 200.0)] {
            let _ = app.update(Message::Resized(size));
            assert_eq!(app.load_range(), (today, today + Days::days(AGENDA_DAYS)));
            assert_eq!((app.view, app.anchor), (CalView::Month, date(2026, 3, 18)));
        }
    }

    #[test]
    fn an_open_editor_survives_a_trip_through_mini() {
        let mut app = demo_app();
        let _ = app.update(Message::NewEvent { date: date(2026, 10, 7), time: None });
        let _ = app.update(Message::Resized(MINI));
        assert!(app.editor.is_some());
        let _ = app.update(Message::Resized(NARROW));
        assert!(app.editor.is_some(), "and opens over the window in narrow");
    }

    /// A launched demo app with the window at `size` and the event `index` open in the panel.
    fn with_open_event(size: Size, index: usize) -> App {
        let mut app = launched(&["--demo", "--now", "2026-10-07T14:20"]);
        let _ = app.update(Message::Resized(size));
        let _ = app.update(Message::OpenEvent(index));
        app
    }

    /// The index of the first event of the demo that can be edited.
    fn editable(app: &App) -> usize {
        app.events.iter().position(|event| !event.recurring).expect("the demo has editable events")
    }

    #[test]
    fn the_panel_stands_beside_the_view_only_in_wide_windows() {
        let app = with_open_event(WIDE, 0);
        assert!(app.panel_beside(SizeClass::Wide), "it takes the place of the properties column");
        for size in [Size::new(800.0, 600.0), NARROW] {
            let mut app = with_open_event(size, 0);
            assert!(!app.panel_beside(app.class()), "{size:?}: over the view");
            let _ = app.update(Message::Editor(editor::Message::Close));
            assert!(!app.panel_beside(SizeClass::Wide), "closed, nothing stands beside");
        }
    }

    #[test]
    fn the_window_builds_with_the_panel_in_every_layout_and_view() {
        for size in [WIDE, Size::new(800.0, 600.0), NARROW, MINI] {
            let mut app = with_open_event(size, 0);
            for view in CalView::ALL {
                let _ = app.update(Message::SetView(view));
                let _ = app.view();
            }
            let _ = app.update(Message::NewEvent { date: date(2026, 10, 8), time: None });
            let _ = app.update(Message::Editor(editor::Message::ToggleCalendars));
            let _ = app.view();
        }
    }

    #[test]
    fn the_open_event_is_selected_in_the_views_and_found_again_after_a_reload() {
        let mut app = with_open_event(WIDE, 0);
        let index = editable(&app);
        let _ = app.update(Message::OpenEvent(index));
        assert_eq!(app.view_data().selected, Some(index));
        // A reload in another order renumbers the events; the selection follows the event.
        let (collections, mut events, _) = torro_pim::demo::sample(app.now.date());
        events.reverse();
        let _ = app.update(Message::Loaded(Ok((collections, events))));
        let moved = app.view_data().selected.expect("still selected");
        assert_eq!(app.events[moved].uid, app.editor.as_ref().and_then(Editor::event).expect("open").uid);
        // Occurrences of a series share their uid; only the one clicked is selected.
        let first = app.events.iter().position(|event| event.recurring).expect("the demo has a series");
        let _ = app.update(Message::OpenEvent(first));
        assert_eq!(app.view_data().selected, Some(first));
        let _ = app.update(Message::NewEventHere);
        assert_eq!(app.view_data().selected, None, "a draft is not in the calendar yet");
    }

    #[test]
    fn opening_another_event_saves_the_pending_change_first() {
        let mut app = with_open_event(WIDE, 0);
        let index = editable(&app);
        let _ = app.update(Message::OpenEvent(index));
        let edits = app.edits;
        let _ = app.update(Message::Editor(editor::Message::Location("Room 9".into())));
        assert_eq!(app.update(Message::OpenEvent(index)).units(), 0, "a second click keeps the panel as it is");
        assert_eq!(app.edits, edits);
        let other = app.events.iter().rposition(|event| !event.recurring).expect("another event");
        assert!(app.update(Message::OpenEvent(other)).units() > 0, "the pending location is saved");
        assert_eq!(app.edits, edits + 1);
        assert_eq!(app.view_data().selected, Some(other));
    }

    #[test]
    fn an_answer_for_an_earlier_editor_does_not_reach_the_next() {
        let mut app = with_open_event(WIDE, 0);
        let index = editable(&app);
        let _ = app.update(Message::OpenEvent(index));
        let earlier = app.edits;
        let event = app.events[index].clone();
        let _ = app.update(Message::NewEventHere);
        let _ = app.update(Message::Editor(editor::Message::Title(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Insert('X'),
        ))));
        let _ = app.update(Message::Editor(editor::Message::Flush));
        let _ = app.update(Message::Saved(earlier, Ok(event)));
        assert!(app.editor.as_ref().is_some_and(|editor| editor.event().is_none()), "the draft stays a draft");
    }

    #[test]
    fn escape_closes_the_menu_first_then_the_panel() {
        let mut app = with_open_event(Size::new(800.0, 600.0), 0);
        let _ = app.update(Message::ToggleMenu(Menu::View));
        let _ = app.update(Message::Escape);
        assert_eq!((app.menu, app.editor.is_some()), (None, true));
        let _ = app.update(Message::Escape);
        assert!(app.editor.is_none());
        let _ = app.update(Message::Escape);
        assert!(app.editor.is_none(), "nothing left to close");
    }

    #[test]
    fn deleting_closes_the_panel() {
        let mut app = with_open_event(WIDE, 0);
        let index = editable(&app);
        let _ = app.update(Message::OpenEvent(index));
        let _ = app.update(Message::Editor(editor::Message::Delete));
        assert!(app.editor.is_some(), "it asks first");
        assert!(app.update(Message::Editor(editor::Message::ConfirmDelete)).units() > 0);
        assert!(app.editor.is_none());
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    fn connected() -> AccountState {
        AccountState::Connected { server: "demo".into(), login_name: "demo".into(), last_sync: None, problem: None }
    }

    /// A connected account with this id, named like it.
    fn account(id: &str) -> PimAccount {
        PimAccount { id: id.into(), label: id.into(), kind: "nextcloud".into(), state: connected(), scopes: None }
    }

    fn one_account() -> Vec<PimAccount> {
        vec![account("work")]
    }

    /// The accounts of `--demo`: the sample spreads over two.
    fn two_accounts() -> Vec<PimAccount> {
        vec![account(torro_pim::demo::WORK_ACCOUNT), account(torro_pim::demo::HOME_ACCOUNT)]
    }

    /// A demo app started with these words, its account and its first events loaded.
    fn launched(words: &[&str]) -> App {
        let launch = Launch::parse(&args(words)).expect("valid parameters");
        let (mut app, _) = App::new(launch.source(&args(words)), launch);
        let (collections, events, _) = torro_pim::demo::sample(app.now.date());
        let _ = app.update(Message::AccountLoaded(Ok(two_accounts())));
        let _ = app.update(Message::Loaded(Ok((collections, events))));
        app
    }

    #[test]
    fn the_title_band_repeats_what_lies_over_the_window() {
        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::Resized(Size::new(800.0, 600.0)));
        assert_eq!(app.cover(), Cover::Nothing);
        app.drawer = true;
        assert_eq!(app.cover(), Cover::Drawer, "the drawer over a medium window");
        let _ = app.update(Message::Resized(Size::new(240.0, 160.0)));
        assert_eq!(app.cover(), Cover::Nothing, "a glance shows no drawer");

        let mut edit = launched(&["--demo", "--open", "editor"]);
        edit.drawer = true;
        for (size, cover) in [
            (Size::new(800.0, 600.0), Cover::Panel),
            (Size::new(420.0, 780.0), Cover::Filled(Surface::Panel)),
            (Size::new(1600.0, 1000.0), Cover::Nothing),
        ] {
            let _ = edit.update(Message::Resized(size));
            assert_eq!(edit.cover(), cover, "{size:?}: the editor's panel lies above the drawer");
        }
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
        assert_eq!(launch.now, Some(date(2026, 10, 12).and_hms_opt(10, 30, 0).expect("valid time")));
        for (word, appearance) in
            [("light", Appearance::Light), ("dark", Appearance::Dark), ("system", Appearance::System)]
        {
            let launch = Launch::parse(&args(&["--appearance", word])).expect("works without --demo");
            assert_eq!(launch.appearance, Some(appearance));
        }
    }

    #[test]
    fn a_design_is_chosen_only_with_the_demo() {
        let launch = Launch::parse(&args(&["--demo", "--design", "torro"])).expect("the standard is always there");
        assert_eq!(launch.design.as_deref(), Some("torro"));
        assert_eq!(Launch::parse(&args(&["--design", "torro"])), Err(LaunchError::NeedsDemo("--design")));
        assert_eq!(Launch::parse(&args(&["--demo", "--design"])), Err(LaunchError::MissingValue("--design")));
        // Which ids are valid is `torro_gui::design::is_choice`'s test; it reads the user's designs folder.
    }

    /// The message "design changed" (task L.3): the stored setting arrives and the window draws
    /// with that pack from the next frame; a broken one gives the standard.
    #[test]
    fn the_stored_design_is_drawn_and_a_broken_one_gives_the_standard() {
        let dir = std::env::temp_dir().join(format!("torrocalendar-designs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // The standard's values under another id, so the tests beside this one see the same tokens.
        for (id, toml) in [
            ("twin", "format = 1\n[about]\nname = \"Torro\"\nversion = \"1.0.0\"\n"),
            ("pale", "format = 1\n[about]\nname = \"Pale\"\nversion = \"1.0.0\"\n[colors.dark]\ntext = \"#202020\"\n"),
        ] {
            std::fs::create_dir_all(dir.join(id)).expect("pack folder");
            std::fs::write(dir.join(id).join("design.toml"), toml).expect("design.toml");
        }
        let mut app = launched(&["--demo"]);
        app.design = Follow::new(None).in_dir(dir.clone());
        let switched = app.update(Message::Design(design::Message::Stored(Ok("twin".into()))));
        assert_eq!(design::active().id, "twin");
        assert!(switched.units() > 0, "the fonts of the new design are loaded");
        let _: Element<'_, Message> = app.view();
        let _ = app.update(Message::Design(design::Message::Stored(Ok("pale".into()))));
        assert!(design::active().is_standard(), "a broken pack is never drawn");
        let _ = std::fs::remove_dir_all(dir);
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
        assert_eq!(Launch::parse(&args(&["--open", "week"])), Err(LaunchError::NeedsDemo("--open")));
        assert_eq!(Launch::parse(&args(&["--now", "2026-10-12T10:30"])), Err(LaunchError::NeedsDemo("--now")));
        for bad in ["tomorrow", "2026-10-12", "2026-10-12 10:30", "2026-13-01T10:30"] {
            assert_eq!(Launch::parse(&args(&["--demo", "--now", bad])), Err(LaunchError::BadNow(bad.into())), "{bad}");
        }
    }

    #[test]
    fn the_view_states_choose_the_view() {
        for (word, view) in [
            ("day", CalView::Day),
            ("week", CalView::Week),
            ("month", CalView::Month),
            ("agenda", CalView::Agenda),
            ("timeline", CalView::Timeline),
            ("radar", CalView::Radar),
        ] {
            assert_eq!(launched(&["--demo", "--open", word]).view, view);
        }
    }

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        date(2026, 10, day).and_hms_opt(hour, minute, 0).expect("valid time")
    }

    /// The radar of the demo on Tuesday 13 October 2026 at 14:20, wide.
    fn radar_app() -> App {
        let mut app = launched(&["--demo", "--open", "radar", "--now", "2026-10-13T14:20"]);
        let _ = app.update(Message::Resized(WIDE));
        app
    }

    #[test]
    fn the_radar_pages_days_and_turns_the_hour_on_the_dial() {
        let mut app = radar_app();
        assert_eq!((app.shown(), app.radar_start()), (CalView::Radar, at(13, 14, 20)), "today at now");
        assert_eq!(
            app.range(),
            (date(2026, 10, 12), date(2026, 10, 21)),
            "the evening before to the mini radars' last day"
        );
        let _ = app.update(Message::Turn(views::radar::HOUR_STEP));
        let _ = app.update(Message::Turn(-views::radar::WHEEL_STEP));
        assert_eq!((app.turn, app.radar_start()), (45, at(13, 15, 5)), "an hour on, a quarter back");
        // Turned past midnight the header names the next day; the day itself stays.
        let _ = app.update(Message::Turn(10 * 60));
        assert_eq!((app.anchor, app.radar_start().date()), (date(2026, 10, 13), date(2026, 10, 14)));
        assert_eq!(app.title_parts().0, format_long_date(date(2026, 10, 14)));
        for _ in 0..30 {
            let _ = app.update(Message::Turn(views::radar::HOUR_STEP));
        }
        assert_eq!(app.turn, 24 * 60, "a day ahead at most");
        let _ = app.update(Message::SetView(CalView::Week));
        let _ = app.update(Message::SetView(CalView::Radar));
        assert_eq!(app.turn, 24 * 60, "kept while another view is shown");
        // › and → page a day, the dial back at the start of that day: 08:00 or earlier.
        let _ = app.update(Message::Next);
        assert_eq!((app.anchor, app.turn), (date(2026, 10, 14), 0));
        let start = app.radar_start();
        assert_eq!(start.date(), date(2026, 10, 14));
        assert!(start.time() <= NaiveTime::from_hms_opt(8, 0, 0).expect("time"), "{start}");
        assert!(start.time() >= NaiveTime::from_hms_opt(6, 0, 0).expect("time"), "{start}");
        let _ = app.update(Message::Turn(30));
        let _ = app.update(Message::Prev);
        let _ = app.update(Message::Prev);
        assert_eq!((app.anchor, app.turn), (date(2026, 10, 12), 0), "‹ twice: Monday");
        let _ = app.update(Message::Today);
        assert_eq!((app.anchor, app.turn, app.radar_start()), (date(2026, 10, 13), 0, at(13, 14, 20)));
    }

    #[test]
    fn the_arrows_page_the_radar_by_days() {
        use iced::keyboard::{Event, Key, Location, Modifiers, key::Named, key::Physical};
        let arrow = |named: Named| {
            iced::Event::Keyboard(Event::KeyPressed {
                key: Key::Named(named),
                modified_key: Key::Named(named),
                physical_key: Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
                location: Location::Standard,
                modifiers: Modifiers::empty(),
                text: None,
                repeat: false,
            })
        };
        let id = iced::window::Id::unique();
        let ignored = iced::event::Status::Ignored;
        let mut app = radar_app();
        let right = shortcut(arrow(Named::ArrowRight), ignored, id).expect("→ is taken");
        let _ = app.update(right);
        assert_eq!((app.anchor, app.turn), (date(2026, 10, 14), 0));
        let left = shortcut(arrow(Named::ArrowLeft), ignored, id).expect("← is taken");
        let _ = app.update(left);
        assert_eq!(app.anchor, date(2026, 10, 13));
        // ↑ ↓ are the radar's alone: an hour earlier or later.
        assert!(shortcut(arrow(Named::ArrowDown), ignored, id).is_none(), "not for the other views");
        let down = radar_shortcut(arrow(Named::ArrowDown), ignored, id);
        assert!(matches!(down, Some(Message::Turn(60))), "{down:?}");
        let up = radar_shortcut(arrow(Named::ArrowUp), ignored, id);
        assert!(matches!(up, Some(Message::Turn(-60))), "{up:?}");
        assert!(radar_shortcut(arrow(Named::ArrowUp), iced::event::Status::Captured, id).is_none());
        assert!(radar_shortcut(arrow(Named::ArrowRight), ignored, id).is_none(), "← → stay with paging");
        let _ = app.update(down.expect("down"));
        assert_eq!(app.radar_start(), at(13, 15, 20));
    }

    #[test]
    fn a_day_in_the_small_month_or_the_week_strip_stays_in_the_radar() {
        let mut app = radar_app();
        let _ = app.update(Message::Turn(90));
        // The small month (wide) and the week strip (narrow) both send `GoTo`.
        let _ = app.update(Message::GoTo(date(2026, 11, 3)));
        assert_eq!(
            (app.view, app.shown(), app.anchor, app.turn),
            (CalView::Radar, CalView::Radar, date(2026, 11, 3), 0)
        );
        assert_eq!(app.mini_month, date(2026, 11, 1));
        let _ = app.update(Message::Resized(NARROW));
        let _ = app.update(Message::GoTo(date(2026, 11, 5)));
        assert_eq!((app.shown(), app.anchor), (CalView::Radar, date(2026, 11, 5)));
        // The 5th is on the page the 3rd opened (4th to 10th), so it stays.
        assert_eq!(app.load_range(), (date(2026, 11, 2), date(2026, 11, 11)), "the strip's week and the radar's days");
        let _ = app.view();
    }

    #[test]
    fn a_mini_radar_opens_its_day_in_the_radar_unturned() {
        let mut app = radar_app();
        // The range reaches through the seventh day after the shown one, so its mini radar is full.
        let (_, after) = app.range();
        assert_eq!(after, app.anchor + Days::days(views::radar::COMING_DAYS + 1));
        let _ = app.view();
        // A tile sends `GoTo` its day: the radar stays and opens it at its start, unturned.
        let _ = app.update(Message::Turn(90));
        let _ = app.update(Message::GoTo(date(2026, 10, 16)));
        assert_eq!(
            (app.view, app.shown(), app.anchor, app.turn),
            (CalView::Radar, CalView::Radar, date(2026, 10, 16), 0)
        );
        assert_eq!(app.radar_start().date(), date(2026, 10, 16));
        for class in [Size::new(800.0, 600.0), NARROW] {
            let _ = app.update(Message::Resized(class));
            let _ = app.view();
        }
    }

    #[test]
    fn a_mini_radar_clicked_leaves_the_tiles_where_they_are_and_arrows_browse_them() {
        let mut app = radar_app();
        let today = app.anchor;
        assert_eq!(app.coming, today + Days::days(1), "the page starts after the shown day");
        // A tile three days on: the radar shows it, the page stays.
        let _ = app.update(Message::GoTo(today + Days::days(3)));
        assert_eq!((app.anchor, app.coming), (today + Days::days(3), today + Days::days(1)));
        // ‹ › browse a week at a time; the radar stays on its day and the range loads both.
        let _ = app.update(Message::ComingPage(1));
        assert_eq!((app.anchor, app.coming), (today + Days::days(3), today + Days::days(8)));
        assert_eq!(app.range(), (today + Days::days(2), today + Days::days(15)));
        let _ = app.view();
        // A tile of the browsed page keeps it too.
        let _ = app.update(Message::GoTo(today + Days::days(10)));
        assert_eq!(app.coming, today + Days::days(8));
        let _ = app.update(Message::ComingPage(-2));
        assert_eq!(app.coming, today - Days::days(6));
        assert_eq!(app.range(), (today - Days::days(6), today + Days::days(13)));
        // "Today" on that page keeps it, today's tile marked…
        let _ = app.update(Message::Today);
        assert_eq!((app.anchor, app.coming), (today, today - Days::days(6)));
        // …and leaves a page without today behind: it starts after today again.
        let _ = app.update(Message::ComingPage(3));
        let _ = app.update(Message::Today);
        assert_eq!((app.anchor, app.coming), (today, today + Days::days(1)));
    }

    #[test]
    fn the_radar_key_is_r() {
        use iced::keyboard::{Event, Key, Location, Modifiers, key::Physical};
        let press = iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Character("r".into()),
            modified_key: Key::Character("r".into()),
            physical_key: Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: Some("r".into()),
            repeat: false,
        });
        let message = shortcut(press, iced::event::Status::Ignored, iced::window::Id::unique());
        assert!(matches!(message, Some(Message::SetView(CalView::Radar))));
        assert_eq!(CalView::Radar.key(), "R");
    }

    #[test]
    fn the_menu_states_start_with_the_menu_open() {
        let menu = launched(&["--demo", "--open", "menu"]);
        assert_eq!((menu.menu, menu.drawer), (Some(Menu::View), false));
        let drawer = launched(&["--demo", "--open", "drawer"]);
        assert_eq!((drawer.menu, drawer.drawer), (None, true));
        let switcher = launched(&["--demo", "--open", "switcher"]);
        assert_eq!(
            (switcher.menu, switcher.drawer),
            (Some(Menu::Switcher), true),
            "in the drawer where there is no sidebar"
        );
    }

    #[test]
    fn the_editor_states_open_once_the_events_are_in() {
        let (app, _) = App::new(Source::from_args(args(&["--demo"])), Launch::default());
        assert!(app.editor.is_none());
        let edit = launched(&["--demo", "--open", "editor"]);
        let mut editor = edit.editor.clone().expect("the editor is open");
        // Only an existing event can be deleted: this one is the first that is not a series.
        assert!(matches!(editor.update(editor::Message::Delete), editor::Action::None), "it asks first");
        assert!(
            matches!(editor.update(editor::Message::ConfirmDelete), editor::Action::Delete(event) if !event.recurring)
        );
        let new = launched(&["--demo", "--open", "editor-new"]);
        let editor = new.editor.as_ref().expect("the editor is open");
        assert!(editor.event().is_none(), "it is a draft");
        let series = launched(&["--demo", "--open", "editor-read-only"]);
        let editor = series.editor.as_ref().expect("the editor is open");
        assert!(editor.event().is_some_and(|event| event.recurring), "a series opens read-only");
        // Closing it does not bring it back with the next reload.
        let mut closed = launched(&["--demo", "--open", "editor"]);
        let _ = closed.update(Message::Editor(editor::Message::Close));
        let (collections, events, _) = torro_pim::demo::sample(closed.now.date());
        let _ = closed.update(Message::Loaded(Ok((collections, events))));
        assert!(closed.editor.is_none());
    }

    #[test]
    fn loading_never_ends_and_the_error_comes_at_once() {
        let mut loading = launched(&["--demo", "--open", "loading"]);
        assert_eq!(loading.load().units(), 0, "nothing is asked for");
        let mut error = launched(&["--demo", "--open", "error"]);
        assert_eq!(error.load().units(), 1, "the error is the answer");
        let _ = error.update(Message::Loaded(Err(DEMO_ERROR.into())));
        assert_eq!(error.error, Some(load_failed(DEMO_ERROR)));
    }

    #[test]
    fn now_fixes_the_time_and_the_demo_data_around_it() {
        let words = ["--demo", "--now", "2026-03-04T08:15"];
        let mut app = launched(&words);
        let pinned = date(2026, 3, 4).and_hms_opt(8, 15, 0).expect("valid time");
        assert_eq!((app.now, app.anchor), (pinned, pinned.date()));
        let _ = app.update(Message::Tick);
        assert_eq!(app.now, pinned, "the clock does not run on");
        let _ = app.update(Message::Today);
        assert_eq!(app.anchor, pinned.date());
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

    #[test]
    fn without_a_preference_the_system_appearance_is_dark() {
        assert_eq!(demo_app().theme(), theme::theme());
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

    fn quarter(hour: u32, minute: u32) -> Quarter {
        Quarter::new(hour, minute).expect("quarter")
    }

    #[test]
    fn the_stored_night_is_taken_over_unless_a_save_is_on_its_way() {
        let mut app = launched(&["--demo"]);
        assert_eq!(app.night, Night::DEFAULT);
        let late = Night { from: quarter(23, 30), to: quarter(7, 0) };
        let _ = app.update(Message::Night(Ok(late)));
        assert_eq!(app.night, late, "a change made elsewhere arrives");
        let _ = app.update(Message::Night(Err("starting".into())));
        assert_eq!(app.night, late, "a core that does not answer keeps the night");
        app.night_saving = true;
        let _ = app.update(Message::Night(Ok(Night::DEFAULT)));
        assert_eq!(app.night, late, "the stored night is the old one while ours is on its way");
        let _ = app.update(Message::NightSaved(Ok(())));
        assert!(!app.night_saving);
    }

    #[test]
    fn the_card_sets_the_night_or_none() {
        let mut app = launched(&["--demo", "--open", "timeline"]);
        let moon = Rectangle::new(iced::Point::new(900.0, 300.0), Size::new(24.0, 24.0));
        let _ = app.update(Message::EditNight { fold: None, at: Some(moon) });
        assert_eq!(
            app.night_card.map(|card| (card.at, card.from, card.to)),
            Some((Some(moon), quarter(22, 0), quarter(6, 0)))
        );
        let _ = app.update(Message::NightFrom(quarter(23, 30)));
        let _ = app.update(Message::NightTo(quarter(7, 0)));
        assert_eq!(app.night, Night::DEFAULT, "nothing changes before Save");
        let _ = app.update(Message::SaveNight);
        assert_eq!((app.night, app.night_card), (Night { from: quarter(23, 30), to: quarter(7, 0) }, None));
        let _ = app.update(Message::EditNight { fold: None, at: None });
        let _ = app.update(Message::NoNight);
        assert!(app.night.is_none() && app.night_card.is_none(), "{:?}", app.night);
        let _ = app.view();
    }

    #[test]
    fn escape_and_a_click_beside_close_the_card_without_saving() {
        let mut app = launched(&["--demo", "--open", "night"]);
        assert!(app.night_card.is_some(), "--open night opens it");
        let _ = app.update(Message::NightFrom(quarter(20, 0)));
        let _ = app.update(Message::Escape);
        assert_eq!((app.night_card, app.night), (None, Night::DEFAULT));
        let _ = app.update(Message::EditNight { fold: None, at: None });
        let _ = app.update(Message::CloseNight);
        assert_eq!(app.night_card, None);
    }

    #[test]
    fn a_double_click_folds_back_the_night_its_first_click_opened() {
        let mut app = launched(&["--demo", "--open", "timeline"]);
        let evening = app.anchor;
        let _ = app.update(Message::Timeline(timeline::Change::ToggleNight(evening)));
        assert!(format!("{:?}", app.timeline).contains(&format!("open: {{{evening:?}}}")), "the first click");
        let _ = app.update(Message::EditNight { fold: Some(evening), at: None });
        assert!(app.night_card.is_some());
        assert!(format!("{:?}", app.timeline).contains("open: {}"), "folded again: {:?}", app.timeline);
    }

    #[test]
    fn without_a_night_the_header_button_opens_the_card() {
        let mut app = launched(&["--demo", "--open", "timeline"]);
        let _ = app.update(Message::Resized(Size::new(1440.0, 900.0)));
        app.night = Night::none_at(quarter(22, 0));
        let _ = app.view();
        let _ = app.update(Message::EditNight { fold: None, at: None });
        assert!(app.night_card.is_some_and(|card| card.at.is_none()));
        let _ = app.view();
    }
}
