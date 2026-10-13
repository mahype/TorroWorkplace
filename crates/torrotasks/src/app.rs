//! The tasks window: state, loading, quick add, saving, and the Linear window around it (task
//! T.1): `torro_gui::shell` with the sidebar (filters, lists, "Open Calendar", "Settings") in the
//! wide layout and a navigation menu ("Today ▾") plus drawer in the narrow one. In the wide layout
//! the sidebar collapses to a rail of icons (U.2: panel button or `[`, remembered in `ui.json`);
//! the medium layout always shows that rail. The list (`list.rs`) and the detail (`detail.rs`)
//! only render and report clicks as [`Message`]s.
//!
//! The columns (redesign R.2 in `docs/tasks/design.md`, mock-up
//! `docs/prototypes/tasks/redesign.html`) – see [`Columns`]: without an open task the list stands
//! alone at the left, at most [`size::content_max()`] wide. With one open, wide windows whose content
//! is at least [`THREE_COLUMNS_FROM`] wide show three columns – the list ([`size::list()`]), the
//! detail and the properties ([`size::props()`]) –, narrower wide and medium windows the list
//! ([`size::list_medium()`]) and the detail, narrow windows the detail instead of the list. A
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
//! Issues of a GitHub filter list have a check (task G.5): ticking one closes it on GitHub through
//! the core's `save_todo` ([`Message::IssueChanged`]), and a quiet card at the bottom says "Issue
//! closed" with "Undo" (reopens it) for [`NOTICE_FOR`]. Pull requests have none; where the
//! account's token may not write issues ([`PimAccount::may_close_issues`]) the check is locked.
//!
//! The keyboard (T.3): ↑/↓ or j/k move a cursor through the rows as the list shows them (groups
//! and subtasks included) – with a task open they open the next one –, Enter opens the task under
//! it, Space ticks it, N focuses the quick add row, R syncs, Escape closes menu, drawer or detail
//! (see [`Message::Escape`]). The cursor is the highlighted row; the list scrolls to keep it in
//! view. Plain keys are ignored while a text field has the focus ([`shortcut`], [`typing`]) or a
//! menu is open.
//!
//! GitHub links (task G.6): a GitHub address or `owner/repo#12` in quick add ([`Link::pasted`])
//! goes to the core's `add_link` instead of becoming a task titled with it: in a task list a link
//! task with GitHub's title (and the due date quick add gives there), in a filter list – whose quick
//! add row says "Paste a GitHub link to pin…" – a pinned item. "Add to list…" in the detail of a
//! filter list's item does the same for the list picked; a card then says "Added to {list}" when
//! that is not the list shown (task G.6b). A link has the issue's check in the list
//! and the detail; ticking one goes the way of an issue of a filter list, so "Issue closed · Undo"
//! shows when the core closed the issue too – only when its answer reports the issue closed and
//! it was not before. In a list shared with others the core closes nothing (task G.6c), so no card.
//!
//! New lists (task G.8): one "New list…" under all lists of the sidebar and in the navigation menu
//! opens the dialog of `new_list.rs`, which asks what to make. A task list it makes itself
//! (`tasks.create_list`); the app then shows the new list and says "List “{name}” created". For a
//! GitHub filter list it hands over to the filter list dialog, with all GitHub accounts to choose
//! from and a back arrow to the choice.
//!
//! GitHub filter lists (tasks G.4, G.11): the "…" menu in the head of a filter list has "Edit
//! filter…" and "Delete list…". The dialog itself is `filter.rs`; the app only opens it, runs its
//! tasks and acts on its [`filter::Outcome`] (show the saved list, leave a deleted one, reload after
//! a conflict, back to the choice). The repository chosen last for a filter list is the app's: read
//! from `ui.json` at start, preselected in the next new list, written when a list is created or saved
//! (not in the demo, which stores nothing).
//!
//! Manual order (task P.3b): a single writable list can be ordered by hand – Display › Order "Due
//! date" / "Manual", stored per list in the suite settings (`settings.set_setting` `list_order`,
//! read with the settings on every tick; `--demo` keeps it in the process). In "Manual" the list
//! has no groups (`list.rs`); Alt+↑/↓ moves the task under the cursor one step among the tasks it is
//! ordered with, and a row can be dragged by its grip. A move shows at once and goes to the core's
//! `tasks.move_todo` ([`Moving`], one at a time); an error puts the list back, says so and reloads.
//!
//! Dragging to the sidebar (task G.6c): in a GitHub filter list, and in a writable list with
//! another list of its account, every row has the grip. Dragged onto a list of the sidebar that
//! takes it ([`App::drop_targets`]; that row shows a ring, `shell::drop_target`), an item of a
//! filter list becomes a link there (`tasks.add_link`, "Added to {list}"), a task moves there
//! (`tasks.save_todo` with the new list, "Moved to {list}"). Lists of another account are no
//! targets: the core refuses such a move (`unsupported`).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
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
use torro_core::settings::ListOrder;
use torro_gui::connect::{self, Connect};
use torro_gui::design::{self, Follow};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, Cover, SidebarMode, Surface, Switcher};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail, collection_color};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{style, typo};
use torro_ipc::{Request, Response, SettingKey};
use torro_pim::account::{GITHUB_KIND, account_of, combined_state_for};
use torro_pim::demo::DemoSource;
use torro_pim::model::{GitHubFilter, GitHubKind, GitHubState, Link};
use torro_pim::source::move_plan;
use torro_pim::{AccountState, Collection, CollectionKind, EventTime, PimAccount, PimSource, ProviderError, Todo};

use crate::detail::{self, Detail, Fit};
use crate::filter;
use crate::glance::{self, GlanceData};
use crate::list::{self, ListData};
use crate::new_list;

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
    /// The "…" menu in the head of a GitHub filter list: "Edit filter…", "Delete list…".
    List,
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
    /// The first GitHub filter list whose last refresh failed (else the first one): its symbol in
    /// the sidebar, the hint over the list, the rows grouped by repository.
    FilterList,
    /// The first item pinned in a filter list, in the detail with its "…" menu open (Unpin).
    Pinned,
    /// The first writable task list in manual order (task P.3b): one run of rows, no groups.
    Manual,
    /// The same with the Display menu open on its "Order" section.
    Order,
    /// The same with its first row being dragged and the marker two places further down.
    Drag,
    /// The first filter list with its first row dragged onto the first writable list of the
    /// sidebar, which shows the ring of a drop target (task G.6c).
    Drop,
    /// The dialog "New GitHub filter list" of the first GitHub account, as after the choice.
    FilterNew,
    /// The same with the list of repositories open.
    FilterRepos,
    /// The dialog "Edit filter list" of the first filter list, shown behind it.
    FilterEdit,
    /// "Edit filter list" of the first filter list scrolled to its end: name, colour, "GitHub
    /// search" with the searches.
    FilterSearch,
    /// The dialog "Edit filter list" of the first filter list with one search in one repository
    /// ("Torro bugs" in the demo): its labels chosen as chips.
    FilterRepo,
    /// The question before deleting the first filter list.
    FilterDelete,
    /// "New list…": the choice between a task list and a GitHub filter list (task G.8).
    NewList,
    /// "New task list", the form after the choice, on the first Nextcloud account.
    NewTaskList,
    /// A task list "Reading" just made through the form: the new list shown, the card "List
    /// “Reading” created" at the bottom.
    ListCreated,
    /// The error state with its "Retry".
    Error,
    /// No tasks at all.
    Empty,
    /// The loading state, kept for good.
    Loading,
}

impl Open {
    const ALL: [(&'static str, Open); 26] = [
        ("detail", Open::Detail),
        ("menu", Open::Menu),
        ("display", Open::Display),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("due", Open::Due),
        ("pr", Open::PullRequest),
        ("more", Open::More),
        ("filter", Open::FilterList),
        ("pinned", Open::Pinned),
        ("manual", Open::Manual),
        ("order", Open::Order),
        ("drag", Open::Drag),
        ("drop", Open::Drop),
        ("filter-new", Open::FilterNew),
        ("filter-repos", Open::FilterRepos),
        ("filter-edit", Open::FilterEdit),
        ("filter-repo", Open::FilterRepo),
        ("filter-search", Open::FilterSearch),
        ("filter-delete", Open::FilterDelete),
        ("new-list", Open::NewList),
        ("new-task-list", Open::NewTaskList),
        ("list-created", Open::ListCreated),
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
/// - `--design <id>`: draw with this design pack instead of the standard (task L.3); needs `--demo`.
///   An id that is not a valid pack is refused with the valid ones.
/// - `--now <YYYY-MM-DDTHH:MM>`: a fixed "now" (only its day counts here) and demo data around
///   it, for pictures that can be made again; needs `--demo`. The detail counts from that day as
///   well ([`Detail::on_day`]).
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
const DEMO_ERROR: &str = "The server did not answer (demo)";

/// The id of the quick add field, so "New" can focus it.
const QUICK_ADD: &str = "quick-add";
/// Where the quick add text starts: on the axis of the rows' titles – past the leading symbol
/// (16 in, 16 wide, 12 after it) and the priority column (14, 8 after it).
fn quick_add_start() -> f32 {
    space::l() + icon::SIZE + space::m() + icon::SIZE_SM + space::s()
}
/// Where the quick add text ends: before the key hint "N" at the right.
fn quick_add_end() -> f32 {
    space::l() + space::xl() + space::s()
}
/// Wide windows show the properties column beside the detail from this content width on (the
/// panel next to the sidebar or rail): list 460 + detail + properties 300 (mock-up).
pub const THREE_COLUMNS_FROM: f32 = 1320.0;
/// List ([`size::list_medium()`]) and detail side by side from this content width on: the detail
/// then gets at least the width of the properties column ([`size::props()`]), the narrowest column
/// the design gives a task's facts. Below it (medium windows under about 740) the detail beside
/// the list would be a sliver of wrapped words – there it replaces the list as in narrow windows.
/// The content's two border lines and the line between the columns count as well.
pub fn list_detail_from() -> f32 {
    size::list_medium() + size::props() + 3.0 * LINE
}
/// The list head keeps the label of "Display" from this list width on; narrower it is an icon
/// with a tooltip (mock-up: `@container list (max-width:439px)`).
const HEAD_LABELS_FROM: f32 = 440.0;
/// The one-pixel lines between the columns.
const LINE: f32 = size::HAIRLINE;
/// How long after the last keystroke typed title or notes are saved.
const AUTOSAVE: Duration = Duration::from_secs(1);
/// How long "Issue closed · Undo" stays (task G.5): long enough to reach "Undo", short enough not
/// to linger in a tile that runs all day.
pub const NOTICE_FOR: Duration = Duration::from_secs(8);

/// The card after an issue was closed (task G.5): "Issue closed" with "Undo".
#[derive(Debug, Clone)]
struct Notice {
    /// The closed issue as the core answered, for "Undo" when the list does not hold it any more.
    todo: Todo,
    /// When it was closed; the card goes after [`NOTICE_FOR`] ([`Message::NoticeTick`]).
    since: iced::time::Instant,
    /// The name of the list a GitHub link went to when it is not the one shown (task G.6b): the
    /// card then says "Added to {list}", without "Undo".
    added_to: Option<String>,
    /// With `added_to`: the task was moved there by dragging it onto the list (task G.6c), so the
    /// card says "Moved to {list}".
    moved: bool,
}

/// The card after a task list was made (task G.8): "List “{name}” created".
#[derive(Debug, Clone)]
struct Created {
    name: String,
    /// When it was made; the card goes after [`NOTICE_FOR`] ([`Message::NoticeTick`]).
    since: iced::time::Instant,
}

/// Moves of a list in manual order on their way to the core (task P.3b), sent one at a time: a
/// move names its tasks by `href` and needs their current etags, which the answer to the one
/// before brings. Each move shows at once ([`App::move_task`]).
#[derive(Debug, Clone)]
struct Moving {
    /// The tasks as they were before the first move not answered yet: what an error goes back to.
    undo: Vec<Todo>,
    /// Moves behind the one in flight, oldest first: the task and the one it goes in front of
    /// (`None`: the end of its level), by `href`.
    queued: VecDeque<(String, Option<String>)>,
}

/// Which task: its list and uid (task G.3b). The uid alone is not enough, since the same issue is
/// in every filter list that finds it, with the same uid; the cursor, the open detail and a drag
/// must stay on the copy the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    collection: String,
    uid: String,
}

impl Key {
    fn of(todo: &Todo) -> Key {
        Key { collection: todo.collection.clone(), uid: todo.uid.clone() }
    }

    /// `todo` is the task this names.
    fn is(&self, todo: &Todo) -> bool {
        todo.uid == self.uid && todo.collection == self.collection
    }
}

/// A row being dragged by its grip (task P.3b).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Drag {
    /// The dragged task, so a reload during the drag does not mix up rows.
    key: Key,
    /// Where the marker stands: a place of [`list::drop_places`]; `None` until the pointer moved.
    place: Option<usize>,
    /// The list of the sidebar the pointer is on, when it takes the task ([`App::drop_targets`],
    /// task G.6c): the drop goes there, and the list's marker hides meanwhile.
    over: Option<String>,
}

/// The columns below the header bar (mock-up `redesign.html`), chosen by [`columns`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Columns {
    /// No task open: the list alone at the left, at most [`size::content_max()`] wide (narrow
    /// windows: the full width).
    List,
    /// Narrow windows with a task open: the detail instead of the list.
    Detail,
    /// The list ([`size::list_medium()`]) and the detail; the properties are chips in the detail.
    ListDetail,
    /// The list ([`size::list()`]), the detail and the properties column ([`size::props()`]).
    ListDetailProps,
}

/// The columns for a window of `class` whose content (the panel beside sidebar or rail; the whole
/// window in narrow ones) is `content` wide, with a task open in the detail or not. Mini and strip
/// show a glance and no columns; they count as narrow here. A medium window too small for list
/// and detail side by side ([`list_detail_from()`]) shows the detail instead of the list, as a
/// narrow one does.
pub fn columns(class: SizeClass, content: f32, detail_open: bool) -> Columns {
    match (class, detail_open) {
        (_, false) => Columns::List,
        (SizeClass::Wide, true) if content >= THREE_COLUMNS_FROM => Columns::ListDetailProps,
        (SizeClass::Wide | SizeClass::Medium, true) if content >= list_detail_from() => Columns::ListDetail,
        (_, true) => Columns::Detail,
    }
}

/// The width of the content for a window `width` wide in `class`: the window less the sidebar
/// (`sidebar`: expanded or rail) and the inset margin in the wide and medium layouts, the whole
/// window otherwise.
pub fn content_width(class: SizeClass, sidebar: SidebarMode, width: f32) -> f32 {
    let side = match (class, sidebar) {
        (SizeClass::Wide, SidebarMode::Expanded) => size::sidebar(),
        (SizeClass::Wide, SidebarMode::Rail) | (SizeClass::Medium, _) => shell::rail(),
        (SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip, _) => return width,
    };
    (width - side - layout::inset()).max(0.0)
}

/// The width of the list column in `columns`, inside a content `content` wide: alone at most
/// [`size::content_max()`] (inside the panel's border; the whole panel when less than
/// [`space::xxl()`] would be left beside it) – the whole width in narrow windows –, beside
/// the detail [`size::list()`] or [`size::list_medium()`]. Rows shorten after it ([`list::RowParts`]).
pub fn list_width(class: SizeClass, columns: Columns, content: f32) -> f32 {
    match columns {
        Columns::List if matches!(class, SizeClass::Wide | SizeClass::Medium) => {
            let inner = content - 2.0 * LINE;
            // A sliver of empty panel beside the list would only look like a mistake: then the
            // list takes the whole panel.
            if inner - size::content_max() < space::xxl() { inner } else { size::content_max() }
        }
        Columns::List | Columns::Detail => content,
        Columns::ListDetail => size::list_medium(),
        Columns::ListDetailProps => size::list(),
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
        Some(Group::Completed | Group::Repo(_) | Group::All) | None => (by_filter, None, None),
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
    /// How a GitHub filter list groups its rows: by repository until the Display menu says
    /// otherwise (task G.3). Kept apart from `group_by`, which filter lists do not offer all of.
    filter_group_by: list::GroupBy,
    /// Groups folded away in the list. Window state.
    collapsed: HashSet<list::Group>,
    /// The group whose "+" was pressed: the next quick add task goes there ([`quick_add_fields`]).
    quick_group: Option<list::Group>,
    /// Whether tasks have been loaded once; until then the list shows loading (or the error).
    loaded: bool,
    quick_add: String,
    /// Index into `todos` of the task open in the detail panel.
    selected: Option<usize>,
    /// The task under the keyboard cursor: the highlighted row. A [`Key`], not an index, so it
    /// survives reloads; clicking a row or opening a task moves it there.
    cursor: Option<Key>,
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
    /// Follows the design pack the suite settings name (task L.3).
    design: Follow,
    /// "Issue closed · Undo" after ticking an issue of a filter list (task G.5).
    notice: Option<Notice>,
    /// The dialog of a GitHub filter list, while it is open (task G.4).
    filter_dialog: Option<filter::Dialog>,
    /// "New list…", while it is open (task G.8).
    new_list: Option<new_list::Dialog>,
    /// The repository chosen last for a GitHub filter list (task G.11), for every account: from
    /// `ui.json`, preselected in a new filter list.
    last_repo: Option<String>,
    /// "List “{name}” created" after a task list was made.
    created: Option<Created>,
    /// How the task lists are ordered (Display › Order, task P.3b), by list id; a list not named
    /// here is ordered by due date. From the suite settings; `--demo` keeps it in the process.
    list_orders: BTreeMap<String, ListOrder>,
    /// Changes of `list_orders` on their way to the core; until they are answered, the settings
    /// fetched meanwhile are older than what the window shows and are not taken.
    orders_saving: usize,
    /// Moves of a list in manual order the core has not answered yet.
    moving: Option<Moving>,
    /// The row being dragged by its grip, if any.
    drag: Option<Drag>,
}

// A message may carry a whole task; two kinds do (`Saved`, `IssueChanged`), so Clippy no longer
// counts one variant as oversized. Boxing them would only add an allocation.
#[derive(Debug, Clone)]
pub enum Message {
    /// The band above the content was pressed: drag the window (see [`shell::titlebar`]).
    DragWindow,
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
    /// The core's answer to "Unpin" ([`detail::Action::Unpin`]).
    Unpinned(Result<(), String>),
    /// The core's answer to closing or reopening an issue of a filter list (task G.5): the issue
    /// as GitHub reports it now.
    IssueChanged(Result<Todo, String>),
    /// "Undo" in the "Issue closed" card: reopen it.
    Undo,
    /// The core's answer to a GitHub link pasted into quick add or "Add to list…" (task G.6): the
    /// task the list holds, and the due date and priority quick add gives a new task there
    /// ([`quick_add_fields`]), which a link in a task list then gets.
    LinkAdded(Result<Todo, String>, Option<EventTime>, Option<u8>),
    /// The clock while the card shows; it goes after [`NOTICE_FOR`].
    NoticeTick(iced::time::Instant),
    Refresh,
    Tick,
    /// The desktop's light/dark preference (once at start, then whenever it changes).
    SystemMode(Mode),
    /// The appearance stored in the suite settings; an error keeps the one we have.
    Appearance(Result<Appearance, String>),
    /// The zoom keys, the stored zoom and its badge.
    Zoom(zoom::Message),
    /// The design pack stored in the suite settings, and its fonts loaded.
    Design(design::Message),
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
    /// The repository chosen last for a filter list, read from `ui.json` at start.
    LastRepo(Option<String>),
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
    /// Open the dialog of a GitHub filter list ("New filter list…", "Edit filter…", "Delete list…").
    OpenFilterDialog(filter::Open),
    /// From the open filter list dialog.
    Filter(filter::Message),
    /// "New list…" in the sidebar or the navigation menu: open the choice (task G.8).
    OpenNewList,
    /// From the open "New list…" dialog.
    NewList(new_list::Message),
    /// Open or close the "…" menu in the head of a filter list.
    ToggleListMenu,
    /// Display › Order: order the list shown like this (task P.3b). Only a single writable list
    /// has an order of its own.
    SetListOrder(ListOrder),
    /// The orders of the task lists as stored in the suite settings; an error keeps the ones we have.
    ListOrders(Result<BTreeMap<String, ListOrder>, String>),
    /// The core's answer to storing an order ([`Message::SetListOrder`]).
    ListOrderSaved(Result<(), String>),
    /// The grip of `todos[index]` was pressed: the drag begins.
    DragStart(usize),
    /// The pointer is at this height in the list's content while a row is dragged.
    DragMove(f32),
    /// The mouse button was released while a row is dragged: the task goes where the marker is, or
    /// onto the list of the sidebar the pointer is on.
    DragEnd,
    /// While a row is dragged, the pointer came onto this list of the sidebar, which takes it
    /// (task G.6c).
    DragOver(String),
    /// The pointer left this list of the sidebar again.
    DragLeave(String),
    /// The core's answer to moving a task to another list by dragging it there (task G.6c).
    TaskMoved(Result<Todo, String>),
    /// The core's answer to a move in manual order: the tasks it wrote, with their new etags.
    Moved(Result<Vec<Todo>, ProviderError>),
    /// A failed move, said again once the reload it started has come back (a reload clears the
    /// error line).
    MoveFailed(String),
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
    /// Alt+↑: move the task under the cursor one step up (a list in manual order).
    MoveUp,
    /// Alt+↓: move it one step down.
    MoveDown,
}

/// Keyboard shortcuts of the list, like the calendar's: keys a widget already used (typing in a
/// text field, Enter in quick add) are ignored, and so is anything with Ctrl, Alt or the logo key –
/// except Alt+↑/↓, which move a task in manual order.
fn shortcut(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event as Key, Key as Name, key::Named};
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(Key::KeyPressed { key, modifiers, .. }) = event else { return None };
    if shell::is_sidebar_key(&key, modifiers) {
        return Some(Message::Shortcut(Shortcut::Sidebar));
    }
    if modifiers.alt() && !modifiers.control() && !modifiers.logo() {
        return match key.as_ref() {
            Name::Named(Named::ArrowUp) => Some(Message::Shortcut(Shortcut::MoveUp)),
            Name::Named(Named::ArrowDown) => Some(Message::Shortcut(Shortcut::MoveDown)),
            _ => None,
        };
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

/// The end of a drag: the left mouse button released anywhere in the window, inside the list or
/// not (the list only hears what happens over it).
fn drag_release(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => Some(Message::DragEnd),
        _ => None,
    }
}

/// Whether one of [`TEXT_FIELDS`] has the focus. A field that is not on screen does not answer.
fn typing() -> Task<bool> {
    Task::batch(TEXT_FIELDS.map(operation::is_focused)).collect().map(|answers| answers.contains(&true))
}

impl App {
    pub fn new(source: Source, launch: Launch) -> (App, Task<Message>) {
        let open = launch.open;
        let zoom = Zoom::for_source(&source);
        let design = Follow::for_source(&source);
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
            filter_group_by: list::GroupBy::Repo,
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
                Some(Open::Display | Open::Order) => Some(Menu::Display),
                Some(Open::Switcher) => Some(Menu::Switcher),
                _ => None,
            },
            // Without a sidebar the app switcher lives in the drawer.
            drawer: matches!(open, Some(Open::Drawer | Open::Switcher)),
            sidebar: SidebarMode::default(),
            zoom,
            design,
            launch,
            opened: false,
            notice: None,
            filter_dialog: None,
            new_list: None,
            last_repo: None,
            created: None,
            list_orders: BTreeMap::new(),
            orders_saving: 0,
            moving: None,
            drag: None,
        };
        let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
        // The demo stores nothing and reads no repository of the user's: its pictures stay the same.
        let last_repo = if app.source.is_demo() {
            Task::none()
        } else {
            ui_state::load_last_repo_task(AppId::TASKS).map(Message::LastRepo)
        };
        let task = Task::batch([
            ui_state::load_task(AppId::TASKS).map(Message::UiState),
            last_repo,
            app.fetch_account(),
            app.fetch_appearance(),
            app.fetch_list_orders(),
            app.zoom.fetch().map(Message::Zoom),
            app.design.fetch().map(Message::Design),
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
            // Escape closes an open menu, drawer, dialog or the detail; nothing listens while none is open.
            if self.menu.is_some()
                || self.drawer
                || self.detail.is_some()
                || self.filter_dialog.is_some()
                || self.new_list.is_some()
                || self.drag.is_some()
            {
                subscriptions.push(widgets::escape().map(|()| Message::Escape));
            }
            // Only while a row is dragged: the release ends it wherever it happens.
            if self.drag.is_some() {
                subscriptions.push(iced::event::listen_with(drag_release));
            }
            // The keys of the list are not for a list behind a dialog.
            if self.connected() && self.filter_dialog.is_none() && self.new_list.is_none() {
                subscriptions.push(iced::event::listen_with(shortcut));
            }
            // Its keys 1 and 2, only where the dialog shows.
            if let Some(dialog) = &self.new_list {
                subscriptions.push(dialog.subscription().map(Message::NewList));
            }
        }
        if let Some(dialog) = &self.filter_dialog {
            subscriptions.push(dialog.subscription().map(Message::Filter));
        }
        if self.detail.as_ref().is_some_and(Detail::wants_autosave) {
            subscriptions.push(iced::time::every(AUTOSAVE).map(|_| Message::Detail(detail::Message::Flush)));
        }
        // Only while "Issue closed" or "List … created" shows: nothing ticks in a quiet window.
        if self.notice.is_some() || self.created.is_some() {
            subscriptions.push(iced::time::every(Duration::from_secs(1)).map(Message::NoticeTick));
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
            Message::DragWindow => return torro_gui::drag_window(),
            Message::Resized(size) => self.size = size,
            Message::AccountLoaded(Ok(accounts)) => {
                let was_connected = self.connected();
                // Every account that can hold task lists, GitHub (filter lists) included.
                self.account = Some(combined_state_for(&accounts, CollectionKind::Todos));
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
                    if let Some(dialog) = self.filter_dialog.as_mut() {
                        dialog.reloaded(&self.collections);
                    }
                    self.keep_cursor(place);
                    // Keep the detail panel on the same task after a reload.
                    let selected = self.detail.as_ref().map(|d| Key::of(&d.original));
                    self.selected = selected.and_then(|key| self.todos.iter().position(|t| key.is(t)));
                    let lists = self.writable_lists();
                    let pinned = self.detail.as_ref().is_some_and(|detail| self.is_pinned(&detail.original));
                    let issue_check = self.detail.as_ref().and_then(|detail| self.issue_check(&detail.original));
                    match (self.selected, self.detail.as_mut()) {
                        (Some(_), Some(detail)) => {
                            detail.refresh(&lists, &self.todos);
                            detail.pinned = pinned;
                            detail.issue_check = issue_check;
                        }
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
                // A filter list takes GitHub links only: they are pinned there (task G.6).
                if let Filter::List(id) = &self.filter
                    && self.filter_source(id).is_some()
                {
                    if summary.is_empty() {
                        return Task::none();
                    }
                    if Link::pasted(&summary).is_none() {
                        self.error =
                            Some(tr("Only links to GitHub issues and pull requests can be pinned here").into());
                        return Task::none();
                    }
                    let id = id.clone();
                    self.quick_add.clear();
                    return self.add_link(id, summary, None, None);
                }
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
                // A pasted GitHub link becomes a link task with GitHub's title (task G.6).
                if Link::pasted(&summary).is_some() {
                    return self.add_link(collection, summary, due, priority);
                }
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
                    // A link closes its issue too (task G.6): the answer may bring "Issue closed".
                    if self.filter_source(&todo.collection).is_some() || list::github_of(&todo).is_some() {
                        return self.change_issue(todo);
                    }
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
                if self.shown_filter_list().is_some() {
                    self.filter_group_by = group_by;
                } else {
                    self.group_by = group_by;
                }
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
            // The reload closes the detail when no search finds the item any more.
            Message::Unpinned(result) => match result {
                Ok(()) => return self.load(),
                Err(err) => self.error = Some(trf("Unpin failed: {error}", &[("error", &err)])),
            },
            Message::IssueChanged(result) => return self.issue_changed(result),
            Message::LinkAdded(result, due, priority) => return self.link_added(result, due, priority),
            Message::Undo => {
                if let Some(todo) = self.undo() {
                    return self.change_issue(todo);
                }
            }
            Message::NoticeTick(now) => {
                if self.notice.as_ref().is_some_and(|notice| now.saturating_duration_since(notice.since) >= NOTICE_FOR)
                {
                    self.notice = None;
                }
                if self.created.as_ref().is_some_and(|card| now.saturating_duration_since(card.since) >= NOTICE_FOR) {
                    self.created = None;
                }
            }
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
                let appearance = Task::batch([
                    self.fetch_appearance(),
                    self.fetch_list_orders(),
                    self.zoom.fetch().map(Message::Zoom),
                    self.design.fetch().map(Message::Design),
                ]);
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
            // No canvas keeps a cache: the next frame draws with the new design, the task loads its
            // fonts.
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
            Message::LastRepo(repo) => self.last_repo = repo,
            Message::CloseMenu => self.close_menus(),
            Message::Escape if self.filter_dialog.is_some() => {
                return self.update(Message::Filter(filter::Message::Escape));
            }
            Message::Escape if self.new_list.is_some() => {
                return self.update(Message::NewList(new_list::Message::Cancel));
            }
            // Escape drops the dragged row where it was.
            Message::Escape if self.drag.is_some() => self.drag = None,
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
            Message::OpenFilterDialog(open) => return self.open_filter_dialog(open),
            Message::Filter(message) => return self.filter_message(message),
            Message::OpenNewList => {
                self.close_menus();
                self.new_list = Some(new_list::Dialog::new(self.list_offer()));
            }
            Message::NewList(message) => return self.new_list_message(message),
            Message::ToggleListMenu => self.toggle_menu(Menu::List),
            Message::SetListOrder(order) => return self.set_list_order(order),
            Message::ListOrders(Ok(orders)) if self.orders_saving == 0 => self.list_orders = orders,
            // Older than a change on its way, or the core is not answering: keep what we have.
            Message::ListOrders(_) => {}
            Message::ListOrderSaved(result) => {
                self.orders_saving = self.orders_saving.saturating_sub(1);
                if let Err(err) = result {
                    self.error = Some(trf("Couldn't save the order: {error}", &[("error", &err)]));
                    // What is stored comes back with the next fetch.
                    return self.fetch_list_orders();
                }
            }
            Message::DragStart(index) => self.start_drag(index),
            Message::DragMove(y) => self.drag_to(y),
            Message::DragEnd => return self.drop_dragged(),
            Message::DragOver(list) => self.drag_over(list),
            Message::DragLeave(list) => {
                // Leaving one row and coming onto the next can arrive in either order.
                if let Some(drag) = self.drag.as_mut().filter(|drag| drag.over.as_ref() == Some(&list)) {
                    drag.over = None;
                }
            }
            Message::TaskMoved(result) => return self.task_moved(result),
            Message::Moved(result) => return self.moved(result),
            Message::MoveFailed(text) => self.error = Some(text),
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
            Shortcut::MoveUp => self.move_step(true),
            Shortcut::MoveDown => self.move_step(false),
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

    /// The room under the title band, which the glances and the filter dialog fill
    /// ([`shell::below_titlebar`]).
    fn content_size(&self) -> Size {
        shell::below_titlebar(self.size, self.zoom.factor())
    }

    /// Mini or strip: a display without header, sidebar, menus or dialogs.
    fn is_glance(&self) -> bool {
        matches!(self.class(), SizeClass::Mini | SizeClass::Strip)
    }

    /// `todos[index]` with its done state flipped, ready to be saved; none if there is no such
    /// task, or it is a match of a GitHub filter list that cannot be ticked: a pull request, or an
    /// issue whose account may not close issues ([`App::issue_check`], task G.5). The one thing a
    /// glance lets you do.
    fn toggled(&self, index: usize) -> Option<Todo> {
        let mut todo = self.todos.get(index)?.clone();
        if self.filter_source(&todo.collection).is_some() && self.issue_check(&todo) != Some(true) {
            return None;
        }
        todo.completed = !todo.completed;
        Some(todo)
    }

    /// The definition of the GitHub filter list `id` ([`Collection::source`]); `None` for every
    /// other list.
    fn filter_source(&self, id: &str) -> Option<&GitHubFilter> {
        self.collections.iter().find(|c| c.id == id)?.source.as_ref()
    }

    /// The filter list the list shows, if the filter is one.
    fn shown_filter_list(&self) -> Option<&GitHubFilter> {
        match &self.filter {
            Filter::List(id) => self.filter_source(id),
            _ => None,
        }
    }

    /// `todo` is pinned in its filter list: `source.pinned` holds its link id.
    fn is_pinned(&self, todo: &Todo) -> bool {
        let (Some(source), Some(link)) = (self.filter_source(&todo.collection), todo.link.as_ref()) else {
            return false;
        };
        source.pinned.contains(&link.id)
    }

    /// How the list groups now: a filter list by its own choice (repository at first), every other
    /// view by the one choice of the Display menu.
    fn grouping(&self) -> list::GroupBy {
        if self.shown_filter_list().is_some() { self.filter_group_by } else { self.group_by }
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
            group_by: self.grouping(),
            collapsed: &self.collapsed,
            width: self.list_width(),
            palette: palette(&self.theme()),
            read_only: self.shown_filter_list().is_some(),
            issue_lists: self.issue_lists(),
            manual: self.manual(),
            grips: self.grips(),
            // Over a list of the sidebar the drop goes there: no marker in the list meanwhile.
            drag: self.drag.as_ref().and_then(|drag| {
                let place = drag.place.filter(|_| drag.over.is_none());
                Some((self.key_index(&drag.key)?, place))
            }),
        }
    }

    /// Index into `todos` of the task under the cursor, if it is in the list.
    fn cursor_index(&self) -> Option<usize> {
        let key = self.cursor.as_ref()?;
        self.key_index(key)
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
        self.cursor = order.get(place.min(order.len().saturating_sub(1))).map(|&i| Key::of(&self.todos[i]));
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
        self.cursor = Some(Key::of(&self.todos[order[next]]));
        let reveal = self.reveal(order[next]);
        let opened = self.detail.as_ref().is_some_and(|detail| !Key::of(&self.todos[order[next]]).is(&detail.original));
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
        let content = self.zoom.view(self.content());
        let (surface, cover) = (self.surface(), self.cover());
        widgets::ctrl_guard(shell::titlebar(content, self.zoom.factor(), surface, cover, Message::DragWindow))
    }

    /// What lies over the top of the window, which the title band repeats: a filter list dialog,
    /// else the drawer. The narrow editor fills the window as a view of its own ([`App::window`]);
    /// signing in and the glance have neither.
    fn cover(&self) -> Cover {
        let class = self.class();
        if !self.connected() || self.is_glance() {
            return Cover::Nothing;
        }
        match &self.filter_dialog {
            Some(dialog) if dialog.is_full(class) => Cover::Filled(Surface::Panel),
            dialog => Cover::dialog(dialog.is_some()).or(Cover::drawer(class, self.drawer)),
        }
    }

    /// The surface at the top of [`App::content`], which the title band takes: the window background
    /// while signing in and for the glance before the first load ([`App::unloaded`]), else that of
    /// the layout.
    fn surface(&self) -> Surface {
        let connected = matches!(self.account, Some(AccountState::Connected { .. }));
        if !connected || (self.is_glance() && !self.loaded) { Surface::Window } else { Surface::of(self.class()) }
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
            SizeClass::Strip => glance::strip(&data, self.content_size()),
            _ => glance::mini(&data, self.content_size()),
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
        let list_menu = Anchor::new();
        // In a narrow window the filter list dialog and the form "New task list" are views of their own.
        if let Some(dialog) = self.filter_dialog.as_ref().filter(|dialog| dialog.is_full(class)) {
            return dialog.view(class, self.content_size()).map(Message::Filter);
        }
        if let Some(dialog) = self.new_list.as_ref().filter(|dialog| dialog.is_full(class)) {
            return dialog.view(class, self.content_size()).map(Message::NewList);
        }
        let sidebar = self.sidebar(SidebarMode::Expanded, &switcher);
        let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
            .then(|| self.sidebar(SidebarMode::Rail, &switcher));
        let columns = self.columns();
        let header = self.header(class, columns, [&nav, &display, &property, &list_menu]);
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
            Menu::List => {
                widgets::floating(&list_menu, Placement::Below(Align::End), self.list_menu(), Message::CloseMenu)
            }
        });
        let dialog =
            self.filter_dialog.as_ref().map(|dialog| dialog.view(class, self.content_size()).map(Message::Filter));
        let dialog = dialog.or_else(|| {
            self.new_list.as_ref().map(|dialog| dialog.view(class, self.content_size()).map(Message::NewList))
        });
        // The property editors and the "…" menu of the detail.
        let editor = || self.detail.as_ref()?.layer(&property).map(|layer| layer.map(Message::Detail));
        // The properties column is part of the body ([`App::body`]), not the shell's: the detail
        // brings its own, scrolling on its own.
        let window = shell::shell(class, sidebar, header, self.body(class, columns, &property), None)
            .rail(rail)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(dialog.or(menu).or_else(editor));
        self.with_notice(window.into())
    }

    /// The window's header bar, split like the columns below it: the list's head over the list
    /// (with a line at the list's edge), the detail's head over detail and properties. Narrow
    /// windows show one of the two.
    /// The anchors are those of the navigation menu, the Display menu, the detail's editors and the
    /// "…" menu of a filter list.
    fn header<'a>(&'a self, class: SizeClass, columns: Columns, anchors: [&Anchor; 4]) -> Element<'a, Message> {
        let [nav, display, property, list_menu] = anchors;
        let list_width = self.list_width();
        if columns == Columns::Detail {
            return self.detail_head(columns, property);
        }
        let list_head = self.list_head(class, list_width, [nav, display, list_menu]);
        let content = content_width(class, self.sidebar, self.size.width);
        // A list that fills the panel needs no line at its edge: the panel's border is there.
        if !matches!(class, SizeClass::Wide | SizeClass::Medium) || list_width >= content - 2.0 * LINE {
            return list_head;
        }
        // The header bar pads 16 at the left; the list head keeps 12 at its right before the line.
        let mut split = row![
            container(list_head).width(list_width - space::l() - space::m()),
            Space::new().width(space::m()),
            rule::vertical(LINE).style(style::rule),
        ]
        .align_y(Alignment::Center);
        if self.detail.is_some() {
            split = split.push(Space::new().width(space::l())).push(self.detail_head(columns, property));
        }
        split.into()
    }

    /// The head of the list: the filter's icon, name and "{n} open" – in narrow windows the
    /// navigation menu ("Today ▾") after the drawer button –, then Display (its label only where the
    /// list is at least [`HEAD_LABELS_FROM`] wide) and the one primary button "New task" ("+" alone
    /// in narrow windows). A GitHub filter list takes no new tasks, so it has no "New task"; it has
    /// its "…" menu instead ("Edit filter…", "Delete list…").
    fn list_head<'a>(&'a self, class: SizeClass, width: f32, anchors: [&Anchor; 3]) -> Element<'a, Message> {
        let [nav, display, list_menu] = anchors;
        let name = self.filter_name(&self.filter);
        let open = trf("{n} open", &[("n", &list::open_count(&self.todos, &self.filter, self.today))]);
        let icon = self.filter_mark(&self.filter);
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
        let mut trailing = vec![widgets::anchored(display, show_display)];
        if self.shown_filter_list().is_none() {
            trailing.push(new_button);
        } else {
            let more = widgets::icon_button(Icon::Ellipsis, tr("List actions"), Some(Message::ToggleListMenu));
            trailing.push(widgets::anchored(list_menu, more));
        }
        if !narrow {
            // Wide and medium have the sidebar or the rail for the navigation.
            return shell::header_bar([], shell::header_title(icon, name, Some(open)), trailing);
        }
        let title = row![
            widgets::menu_button(nav, icon, name, Message::ToggleNav),
            typo::text(typo::body(), open).style(theme::text_3).wrapping(text::Wrapping::None),
        ]
        .spacing(space::xs())
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
    /// [`Detail::properties_column`], [`size::props()`] wide), with the name of the account the
    /// task's list belongs to.
    fn properties_column<'a>(&'a self, anchor: &Anchor) -> Element<'a, Message> {
        let Some(detail) = self.detail.as_ref() else { return Space::new().into() };
        let account = account_of(&detail.original.collection, &self.accounts, |a| a.id.as_str());
        let shown = self.shown_accounts();
        detail.properties_column(account.map(|account| account_title(account, &shown)), anchor).map(Message::Detail)
    }

    /// The Display menu: how to group the list, how to order it (a single writable list: "Due
    /// date" or "Manual", task P.3b), "Show completed", and "Sync now" (`R`) under the time of the
    /// last sync. A GitHub filter list groups by repository or not at all; a list in manual order
    /// has no groups, so it offers none.
    fn display_menu(&self) -> Element<'_, Message> {
        use list::GroupBy;
        let grouping = self.grouping();
        let group = |mark: Icon, label: &'static str, by: GroupBy| {
            MenuItem::new(mark, label, Message::SetGroupBy(by)).current(grouping == by)
        };
        let groups = if self.shown_filter_list().is_some() {
            vec![
                group(Icon::FolderGit2, tr("Repository"), GroupBy::Repo),
                group(Icon::Minus, tr("None"), GroupBy::None),
            ]
        } else {
            vec![
                group(Icon::Calendar, tr("Due date"), GroupBy::Due),
                group(Icon::List, tr("List"), GroupBy::List),
                group(Icon::SignalHigh, tr("Priority"), GroupBy::Priority),
                group(Icon::Minus, tr("None"), GroupBy::None),
            ]
        };
        let check = Mark::Check(palette(&self.theme()).text_2, self.show_completed);
        let sync = vec![MenuItem::new(Icon::RefreshCw, tr("Sync now"), Message::Refresh).trail(Trail::Kbd("R"))];
        let synced = match self.last_sync() {
            Some(at) => MenuSection::titled(trf("Synced {when}", &[("when", &ago(at, Utc::now()))]), sync),
            None => MenuSection::new(sync),
        };
        let mut sections = Vec::new();
        if !self.manual() {
            sections.push(MenuSection::titled(tr("Group by"), groups));
        }
        if let Some(current) = self.list_order() {
            let order = |mark: Icon, label: &'static str, order: ListOrder| {
                MenuItem::new(mark, label, Message::SetListOrder(order)).current(current == order)
            };
            sections.push(MenuSection::titled(
                tr("Order"),
                vec![
                    order(Icon::CalendarClock, tr("Due date"), ListOrder::DueDate),
                    order(Icon::GripVertical, tr("Manual"), ListOrder::Manual),
                ],
            ));
        }
        sections.push(MenuSection::new(vec![MenuItem::new(
            check,
            tr("Show completed"),
            Message::ShowCompleted(!self.show_completed),
        )]));
        sections.push(synced);
        widgets::menu(sections)
    }

    /// The "…" menu of the filter list shown: "Edit filter…" and "Delete list…".
    fn list_menu(&self) -> Element<'_, Message> {
        let Filter::List(id) = &self.filter else { return Space::new().into() };
        let open = |open: filter::Open| Message::OpenFilterDialog(open);
        widgets::menu(vec![MenuSection::new(vec![
            MenuItem::new(Icon::SquarePen, tr("Edit filter…"), open(filter::Open::Edit(id.clone()))),
            MenuItem::new(Icon::Trash2, tr("Delete list…"), open(filter::Open::Delete(id.clone()))),
        ])])
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
    /// error of a first load stand in for all three. A GitHub filter list takes no new tasks: in
    /// place of the quick add row it shows its hints, if it has any ([`App::filter_hints`]).
    fn list(&self) -> Element<'_, Message> {
        if let Some(unloaded) = self.unloaded() {
            return unloaded;
        }
        // A filter list takes pasted GitHub links, which are pinned (task G.6); its hints follow.
        let mut content = column![self.quick_add_row()];
        if let Some(source) = self.shown_filter_list() {
            content = content.extend(self.filter_hints(source));
        }
        if let Some(err) = &self.error {
            let line = typo::text(typo::meta(), err.clone())
                .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) });
            content = content.push(container(line).padding([space::s(), space::l()]));
        }
        content.push(list::view(self.list_data())).width(Fill).height(Fill).into()
    }

    /// The hints of a filter list over its rows, quiet, one line each (32 with the line below):
    /// "Couldn't refresh from GitHub" with the reason in `text_3` when the last refresh failed
    /// (the rows are those from before), and "Showing the first 1,000 matches" when a search found
    /// more than GitHub hands out. The icon stands on the axis of the rows' leading symbols.
    fn filter_hints(&self, source: &GitHubFilter) -> Vec<Element<'_, Message>> {
        let width = self.list_width();
        let mut hints = Vec::new();
        if let Some(problem) = &source.problem {
            let title = tr("Couldn't refresh from GitHub");
            // What is left beside the icon and the title, for the reason on the same line.
            let used = 2.0 * space::l() + icon::SIZE + space::m() + typo::text_width(title, typo::meta()) + space::s();
            let reason = typo::ellipsize(problem, typo::meta(), (width - used).max(0.0));
            let line = row![
                typo::text(typo::meta(), title).style(theme::text_2).wrapping(text::Wrapping::None),
                typo::text(typo::meta(), reason).style(theme::text_3).wrapping(text::Wrapping::None),
            ]
            .spacing(space::s());
            hints.push(hint_row(list::toned(Icon::CircleAlert, icon::SIZE_SM, list::Tone::Attention), line));
        }
        if source.truncated {
            let line = typo::text(typo::meta(), tr("Showing the first 1,000 matches"))
                .style(theme::text_2)
                .wrapping(text::Wrapping::None);
            hints.push(hint_row(list::toned(Icon::ListFilter, icon::SIZE_SM, list::Tone::Text3), line));
        }
        hints
    }

    /// The quick add row: a list row (40 with its line) that is a text input underneath – no box,
    /// "+" on the axis of the rows' leading symbols, the text on the axis of their titles, the key
    /// hint "N" at the right. The icon and the hint lie over the input, so it keeps its hover and
    /// focus states (focused: `selected` with the focus ring); the hint goes away as soon as
    /// something is typed.
    fn quick_add_row(&self) -> Element<'_, Message> {
        let height = size::row() - LINE;
        let above = (height - typo::line_height(typo::body())) / 2.0;
        let placeholder = match self.shown_filter_list() {
            Some(_) => tr("Paste a GitHub link to pin…"),
            None => tr("Add a task or paste a GitHub link…"),
        };
        let input = text_input(placeholder, &self.quick_add)
            .id(QUICK_ADD)
            .on_input(Message::QuickAddChanged)
            .on_submit(Message::QuickAddSubmit)
            .padding(Padding { top: above, bottom: above, left: quick_add_start(), right: quick_add_end() })
            .style(quick_add_style);
        let plus = icon::icon(Icon::Plus, icon::SIZE).style(|t, _| svg::Style { color: Some(palette(t).text_3) });
        let mut over =
            row![plus, Space::new().width(Fill)].align_y(Alignment::Center).height(Fill).padding([0.0, space::l()]);
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
                // A filter list has a symbol of its own; a task list its ring.
                let mark = match &collection.source {
                    Some(source) => Mark::Icon(filter_list_icon(source)),
                    None => Mark::Ring(collection_color(collection.color.as_deref())),
                };
                NavEntry {
                    mark,
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
        // While a row is dragged, the lists that take it report the pointer and show a ring (G.6c).
        let targets = self.dragged_targets();
        let over = self.drag.as_ref().and_then(|drag| drag.over.as_deref());
        let row = |e: NavEntry| {
            let trail = e.trail();
            let target = match &e.filter {
                Filter::List(id) if targets.contains(id) => Some(id.clone()),
                _ => None,
            };
            let item = widgets::sidebar_item_in(mode, e.mark, e.label, trail, e.active, Message::SetFilter(e.filter));
            match target {
                Some(id) => {
                    let on = over == Some(id.as_str());
                    shell::drop_target(item, on, Message::DragOver(id.clone()), Message::DragLeave(id))
                }
                None => item,
            }
        };
        let mut items: Vec<Element<'a, Message>> = views.into_iter().map(row).collect();
        let shown = self.shown_accounts();
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
                    items.push(widgets::sidebar_group_in(mode, account_title(account, &shown), toggle));
                    // The rail has no title to unfold with, so it shows every list.
                    if unfolded || mode == SidebarMode::Rail {
                        items.extend(group.items.into_iter().map(row));
                    }
                }
            }
        }
        // One entry for every kind of new list, under all lists (task G.8).
        items.push(widgets::sidebar_item_in(
            mode,
            Icon::Plus,
            tr("New list…"),
            Trail::None,
            false,
            Message::OpenNewList,
        ));
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
        let shown = self.shown_accounts();
        match shell::account_groups(&self.accounts, lists, NavEntry::list_id) {
            Err(lists) => {
                if !lists.is_empty() {
                    sections.push(MenuSection::titled(tr("Lists"), lists.into_iter().map(item).collect()));
                }
            }
            // A menu has no folding: every account is a section named after it.
            Ok(groups) => sections.extend(groups.into_iter().map(|group| {
                let title =
                    group.account.map_or_else(|| tr("Lists").to_string(), |account| account_title(account, &shown));
                MenuSection::titled(title, group.items.into_iter().map(item).collect())
            })),
        }
        sections.push(MenuSection::new(vec![MenuItem::new(Icon::Plus, tr("New list…"), Message::OpenNewList)]));
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
        self.cursor = Some(Key::of(&todo));
        let mut detail = Detail::new(&todo, &self.writable_lists(), &self.todos).on_day(self.today);
        detail.pinned = self.is_pinned(&todo);
        detail.issue_check = self.issue_check(&todo);
        self.detail = Some(detail);
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
            detail::Action::ToggleDone(uid) => match self.detail_index(&uid) {
                Some(index) => self.update(Message::ToggleDone(index)),
                None => Task::none(),
            },
            detail::Action::Show(uid) => match self.detail_index(&uid) {
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
            detail::Action::Unpin { collection, item } => {
                let source = self.source.clone();
                Task::perform(
                    async move { source.unpin_issue(collection, item).await.map_err(|e| e.to_string()) },
                    Message::Unpinned,
                )
            }
            detail::Action::AddLink { collection, url } => self.add_link(collection, url, None, None),
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

    /// The icon of a filter in the header: the smart lists' ([`filter_icon`]), a filter list's
    /// symbol ([`filter_list_icon`]); a task list has none (its ring is in the sidebar).
    fn filter_mark(&self, filter: &Filter) -> Option<Icon> {
        match filter {
            Filter::List(id) => self.filter_source(id).map(filter_list_icon),
            other => filter_icon(other),
        }
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

    /// The states of `--open` that need the first tasks: a detail needs a task to open, a filter
    /// list the lists.
    fn open_launched(&mut self) -> Task<Message> {
        if std::mem::replace(&mut self.opened, true) {
            return Task::none();
        }
        let filter_lists = || self.collections.iter().filter(|c| c.source.is_some());
        let filter_list = match self.launch.open {
            Some(Open::FilterList) => filter_lists()
                .find(|c| c.source.as_ref().is_some_and(|s| s.problem.is_some()))
                .or_else(|| filter_lists().next()),
            Some(Open::Pinned) => filter_lists().find(|c| c.source.as_ref().is_some_and(|s| !s.pinned.is_empty())),
            Some(Open::FilterEdit | Open::FilterSearch | Open::FilterDelete) => filter_lists().next(),
            Some(Open::FilterRepo) => filter_lists().find(|c| {
                c.source.as_ref().is_some_and(|s| matches!(s.queries.as_slice(), [q] if filter::repos_of(q).len() == 1))
            }),
            _ => None,
        }
        .map(|list| list.id.clone());
        if let Some(id) = filter_list.clone() {
            self.filter = Filter::List(id);
        }
        let dialog = match (self.launch.open, filter_list) {
            (Some(Open::FilterEdit | Open::FilterRepo | Open::FilterSearch), Some(id)) => Some(filter::Open::Edit(id)),
            (Some(Open::FilterDelete), Some(id)) => Some(filter::Open::Delete(id)),
            (Some(Open::FilterNew | Open::FilterRepos), _) => {
                self.github_accounts().next().map(|a| filter::Open::New(a.id.clone()))
            }
            _ => None,
        };
        if let Some(open) = dialog {
            let opened = self.open_filter_dialog(open);
            return match self.launch.open {
                Some(Open::FilterRepos) => {
                    Task::batch([opened, self.update(Message::Filter(filter::Message::TogglePicker))])
                }
                Some(Open::FilterSearch) => Task::batch([opened, operation::snap_to_end(filter::FORM_SCROLL)]),
                _ => opened,
            };
        }
        match self.launch.open {
            Some(Open::NewList) => return self.update(Message::OpenNewList),
            Some(Open::NewTaskList) => return self.open_new_task_list(None),
            Some(Open::ListCreated) => return self.open_new_task_list(Some("Reading")),
            _ => {}
        }
        if matches!(self.launch.open, Some(Open::Manual | Open::Order | Open::Drag)) {
            self.open_manual();
            return Task::none();
        }
        if self.launch.open == Some(Open::Drop) {
            self.open_drop();
            return Task::none();
        }
        let Some(open @ (Open::Detail | Open::Due | Open::PullRequest | Open::More | Open::Pinned)) = self.launch.open
        else {
            return Task::none();
        };
        let order = list::order(&self.list_data());
        let first = match open {
            Open::PullRequest => order.into_iter().find(|&index| is_open_pull_request(&self.todos[index])),
            Open::Pinned => order.into_iter().find(|&index| self.is_pinned(&self.todos[index])),
            _ => order.first().copied(),
        };
        let Some(first) = first else { return Task::none() };
        let select = self.select(first);
        let editor = match open {
            Open::Due => detail::Editor::Due,
            Open::More | Open::Pinned => detail::Editor::More,
            _ => return select,
        };
        let card = self.update(Message::Detail(detail::Message::Open(editor)));
        Task::batch([select, card])
    }

    /// The connected GitHub accounts, which can have filter lists.
    fn github_accounts(&self) -> impl Iterator<Item = &PimAccount> {
        self.accounts.iter().filter(|a| a.kind == GITHUB_KIND && matches!(a.state, AccountState::Connected { .. }))
    }

    /// The accounts that the sidebar shows with a group of their own (task K.31): those that own
    /// a list, when two or more do. With one owner the list is flat and no account is shown.
    /// [`account_title`] compares hosts only among these.
    fn shown_accounts(&self) -> Vec<&PimAccount> {
        match shell::account_groups(&self.accounts, self.todo_lists(), |c| c.id.as_str()) {
            Ok(groups) => groups.iter().filter_map(|group| group.account).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Opens the dialog of a filter list (`filter.rs`) and asks for what it needs: a new list –
    /// first of the GitHub account `id`, the others as chips, the repository chosen last
    /// preselected, a back arrow to the choice of "New list…" –, or "Edit filter…" / "Delete
    /// list…" of a filter list. Unknown ids open nothing.
    fn open_filter_dialog(&mut self, open: filter::Open) -> Task<Message> {
        self.close_menus();
        let shown = self.shown_accounts();
        let owner = |id: &str| account_of(id, &self.accounts, |a| a.id.as_str());
        let list = |id: &str| self.collections.iter().find(|c| c.id == id);
        let dialog = match open {
            filter::Open::New(id) => {
                let accounts = self
                    .github_accounts()
                    .map(|a| new_list::AccountChoice { id: a.id.clone(), label: account_title(a, &shown) })
                    .collect();
                let start = filter::Start { accounts, account: id, last_repo: self.last_repo.clone(), back: true };
                filter::Dialog::new_list(start)
            }
            filter::Open::Edit(id) => {
                let edit = |(list, a): (&Collection, &PimAccount)| {
                    filter::Dialog::edit(list, a.id.clone(), account_title(a, &shown))
                };
                list(&id).zip(owner(&id)).and_then(edit)
            }
            filter::Open::Delete(id) => list(&id).and_then(filter::Dialog::delete),
        };
        self.filter_dialog = dialog;
        match self.filter_dialog.as_mut() {
            Some(dialog) => dialog.load(&self.source).map(Message::Filter),
            None => Task::none(),
        }
    }

    /// Hands `message` to the open filter list dialog and acts on its outcome: a saved list is
    /// shown, a deleted one left, and after a conflict the lists are loaded again.
    fn filter_message(&mut self, message: filter::Message) -> Task<Message> {
        let Some(dialog) = self.filter_dialog.as_mut() else { return Task::none() };
        let (task, outcome) = dialog.update(message, &self.source);
        let task = task.map(Message::Filter);
        match outcome {
            filter::Outcome::Stay => task,
            filter::Outcome::Close => {
                self.filter_dialog = None;
                task
            }
            filter::Outcome::Back => {
                self.filter_dialog = None;
                self.new_list = Some(new_list::Dialog::new(self.list_offer()));
                task
            }
            filter::Outcome::Saved(id) => {
                let remember = self.filter_dialog.take().and_then(|dialog| dialog.repo_to_remember());
                let show = self.update(Message::SetFilter(Filter::List(id)));
                Task::batch([task, show, self.load(), self.remember_repo(remember)])
            }
            filter::Outcome::Deleted(id) => {
                self.filter_dialog = None;
                let shown = self.filter == Filter::List(id);
                let leave = if shown { self.update(Message::SetFilter(Filter::Today)) } else { Task::none() };
                Task::batch([task, leave, self.load()])
            }
            filter::Outcome::Reload => Task::batch([task, self.load()]),
        }
    }

    /// Keeps `repo` as the repository chosen last for a filter list, for the next new one; stores it
    /// in `ui.json` (not in the demo, which stores nothing).
    fn remember_repo(&mut self, repo: Option<String>) -> Task<Message> {
        let Some(repo) = repo else { return Task::none() };
        self.last_repo = Some(repo.clone());
        if self.source.is_demo() {
            return Task::none();
        }
        ui_state::save_last_repo_task(AppId::TASKS, repo)
    }

    /// What "New list…" can offer: the connected accounts that hold task lists (every kind but
    /// GitHub), the one of the list shown first, and whether a GitHub account is there.
    fn list_offer(&self) -> new_list::Offer {
        let connected = |a: &&PimAccount| matches!(a.state, AccountState::Connected { .. });
        let shown = self.shown_accounts();
        let task_accounts = self
            .accounts
            .iter()
            .filter(connected)
            .filter(|a| a.kind != GITHUB_KIND)
            .map(|a| new_list::AccountChoice { id: a.id.clone(), label: account_title(a, &shown) })
            .collect();
        let preferred = match &self.filter {
            Filter::List(id) => account_of(id, &self.accounts, |a| a.id.as_str()).map(|a| a.id.clone()),
            _ => None,
        };
        new_list::Offer { task_accounts, preferred, github: self.github_accounts().next().is_some() }
    }

    /// `--open new-task-list` and `list-created`: the form "New task list" straight away; with
    /// `name` typed into it and sent, so the answer brings the new list and its card.
    fn open_new_task_list(&mut self, name: Option<&str>) -> Task<Message> {
        self.new_list = new_list::Dialog::task_list(self.list_offer());
        let Some(name) = name else { return operation::focus(new_list::NAME_INPUT) };
        let typed = self.update(Message::NewList(new_list::Message::Name(name.into())));
        Task::batch([typed, self.update(Message::NewList(new_list::Message::Create))])
    }

    /// Hands `message` to the open "New list…" dialog and acts on its outcome: the filter list
    /// dialog takes over (first for the first GitHub account), a new task list is shown and
    /// announced.
    fn new_list_message(&mut self, message: new_list::Message) -> Task<Message> {
        let Some(dialog) = self.new_list.as_mut() else { return Task::none() };
        let (task, outcome) = dialog.update(message, &self.source);
        let task = task.map(Message::NewList);
        match outcome {
            new_list::Outcome::Stay => task,
            new_list::Outcome::Close => {
                self.new_list = None;
                task
            }
            new_list::Outcome::GithubFilter => {
                self.new_list = None;
                let Some(account) = self.github_accounts().next().map(|a| a.id.clone()) else { return task };
                Task::batch([task, self.open_filter_dialog(filter::Open::New(account))])
            }
            new_list::Outcome::Created(list) => {
                self.new_list = None;
                self.created = Some(Created { name: list.name.clone(), since: iced::time::Instant::now() });
                // In the sidebar at once; the reload brings it as the core keeps it.
                let id = list.id.clone();
                if !self.collections.iter().any(|c| c.id == id) {
                    self.collections.push(list);
                }
                let show = self.update(Message::SetFilter(Filter::List(id)));
                Task::batch([task, show, self.load()])
            }
        }
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

/// Ticking issues of GitHub filter lists (task G.5).
impl App {
    /// Whether `todo` is an issue of a GitHub filter list, which has a check: `Some(true)` when
    /// ticking may close it, `Some(false)` when its account's token may not write issues (the check
    /// is locked); `None` for pull requests and every task outside a filter list.
    ///
    /// A link in a task list (task G.6), issue or pull request, is a task of its own: `Some(true)`,
    /// ticking it always works (it closes an issue too where the core may).
    fn issue_check(&self, todo: &Todo) -> Option<bool> {
        let github = list::github_of(todo)?;
        if self.filter_source(&todo.collection).is_none() {
            return Some(true);
        }
        (github.kind == GitHubKind::Issue).then(|| self.may_close(&todo.collection))
    }

    /// Whether the account of the filter list `id` may close issues
    /// ([`PimAccount::may_close_issues`]); an account not listed (yet) may: the core decides.
    fn may_close(&self, id: &str) -> bool {
        account_of(id, &self.accounts, |a| a.id.as_str()).is_none_or(PimAccount::may_close_issues)
    }

    /// The filter lists by id, each with [`App::may_close`], for the checks of the list.
    fn issue_lists(&self) -> HashMap<String, bool> {
        let lists = self.collections.iter().filter(|c| c.source.is_some());
        lists.map(|c| (c.id.clone(), self.may_close(&c.id))).collect()
    }

    /// Index into `todos` of the task `uid` the open detail asks for (a subtask or itself), in the
    /// detail's list first: the same issue can be in two filter lists.
    fn detail_index(&self, uid: &str) -> Option<usize> {
        let list = self.detail.as_ref().map(|detail| detail.original.collection.as_str());
        let in_list = self.todos.iter().position(|t| t.uid == uid && Some(t.collection.as_str()) == list);
        in_list.or_else(|| self.todos.iter().position(|t| t.uid == uid))
    }

    /// "Undo": the card goes, and the issue it names is to be reopened – in the version the list
    /// holds if it does (a reload may have brought a newer one), else as the core answered.
    fn undo(&mut self) -> Option<Todo> {
        let notice = self.notice.take_if(|notice| notice.added_to.is_none())?;
        let same = |t: &&Todo| t.uid == notice.todo.uid && t.collection == notice.todo.collection;
        let closed = self.todos.iter().find(same).cloned().unwrap_or(notice.todo);
        Some(Todo { completed: false, ..closed })
    }

    /// Closes (`todo.completed`) or reopens the issue `todo` of a filter list through the core's
    /// `save_todo`; the answer comes as [`Message::IssueChanged`].
    fn change_issue(&self, todo: Todo) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { source.save_todo(todo).await.map_err(|e| e.to_string()) }, Message::IssueChanged)
    }

    /// The core's answer to [`App::change_issue`]: the issue goes into the list at once (the reload
    /// follows) and into an open detail; after closing, "Issue closed · Undo" shows. A refusal (the
    /// token may not write issues, GitHub not reachable …) is the error line over the list.
    fn issue_changed(&mut self, result: Result<Todo, String>) -> Task<Message> {
        let saved = match result {
            Ok(saved) => saved,
            Err(err) => {
                self.notice = None;
                self.error = Some(trf("Couldn't change the issue: {error}", &[("error", &err)]));
                return Task::none();
            }
        };
        let same = |t: &Todo| t.uid == saved.uid && t.collection == saved.collection;
        let is_closed = |todo: &Todo| list::github_of(todo).is_some_and(|github| github.state != GitHubState::Open);
        // Closed already before this tick (as far as the list knew): then the tick closed nothing.
        let was_closed = self.todos.iter().find(|t| same(t)).is_some_and(is_closed);
        if let Some(shown) = self.todos.iter_mut().find(|t| same(t)) {
            *shown = saved.clone();
        }
        // Only when an issue was really closed now, as the core's answer reports it: a link to a
        // pull request, one whose issue the core could not close, and one in a list shared with
        // others (task G.6c: the core closes nothing there) are only done themselves.
        let issue = list::github_of(&saved).is_some_and(|github| github.kind == GitHubKind::Issue);
        self.notice = (saved.completed && issue && is_closed(&saved) && !was_closed).then(|| Notice {
            todo: saved.clone(),
            since: iced::time::Instant::now(),
            added_to: None,
            moved: false,
        });
        let follow_up = match self.detail.as_mut() {
            Some(detail) if same(&detail.original) => detail.stored(&saved),
            _ => detail::Action::None,
        };
        Task::batch([self.detail_action(follow_up), self.load()])
    }

    /// Adds the GitHub item `url` to the list `collection` through the core's `add_link` (task
    /// G.6): pinned in a filter list, a link task elsewhere. `due` and `priority` are what quick add
    /// gives a new task there; the answer comes as [`Message::LinkAdded`].
    fn add_link(&self, collection: String, url: String, due: Option<EventTime>, priority: Option<u8>) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { source.add_link(collection, url).await.map_err(|e| e.to_string()) }, move |r| {
            Message::LinkAdded(r, due, priority)
        })
    }

    /// The core's answer to [`App::add_link`]: a new link task in a task list gets the due date and
    /// priority quick add gives there (it has none of its own yet), then the list is loaded again.
    /// A refusal (no GitHub account, an item GitHub does not have …) is the error line. When the
    /// link went to another list than the one shown ("Add to list…", or quick add in a smart
    /// list), a card says where: "Added to {list}" (task G.6b).
    fn link_added(
        &mut self,
        result: Result<Todo, String>,
        due: Option<EventTime>,
        priority: Option<u8>,
    ) -> Task<Message> {
        let added = match result {
            Ok(added) => added,
            Err(err) => {
                self.error = Some(trf("Couldn't add the link: {error}", &[("error", &err)]));
                return Task::none();
            }
        };
        self.error = None;
        if !matches!(&self.filter, Filter::List(id) if *id == added.collection) {
            let name = self.collections.iter().find(|c| c.id == added.collection).map(|c| c.name.clone());
            let added_to = Some(name.unwrap_or_else(|| tr("List").into()));
            self.notice =
                Some(Notice { todo: added.clone(), since: iced::time::Instant::now(), added_to, moved: false });
        }
        let planned = Todo { due: added.due.or(due), priority: added.priority.or(priority), ..added.clone() };
        let writable = self.writable_lists().iter().any(|list| list.id == added.collection);
        if writable && planned != added {
            return self.save(planned);
        }
        self.load()
    }

    /// `window` with the "Issue closed" card at the bottom centre while it shows. Always the same
    /// two layers, so the window keeps its state (scroll, focus) when the card comes and goes.
    fn with_notice<'a>(&'a self, window: Element<'a, Message>) -> Element<'a, Message> {
        let card: Element<'a, Message> = match (&self.notice, &self.created) {
            (Some(Notice { added_to: Some(list), moved, .. }), _) => added_card(list, *moved),
            (Some(_), _) => notice_card(),
            (None, Some(created)) => created_card(&created.name),
            (None, None) => Space::new().into(),
        };
        let layer = container(card).width(Fill).height(Fill).center_x(Fill).align_bottom(Fill).padding(space::xl());
        stack![window, layer].width(Fill).height(Fill).into()
    }
}

/// Ordering a list by hand (task P.3b).
impl App {
    /// How the list shown is ordered: `None` where it has no order of its own (the smart lists
    /// Today, Planned and All, a GitHub filter list, a list that is read-only).
    fn list_order(&self) -> Option<ListOrder> {
        let Filter::List(id) = &self.filter else { return None };
        let writable = self.collections.iter().any(|c| &c.id == id && c.writable && c.kind.has_todos());
        (writable && self.filter_source(id).is_none()).then(|| self.list_orders.get(id).copied().unwrap_or_default())
    }

    /// The list shown is in manual order.
    fn manual(&self) -> bool {
        self.list_order() == Some(ListOrder::Manual)
    }

    /// Index into `todos` of the task `key` names, if it is loaded.
    fn key_index(&self, key: &Key) -> Option<usize> {
        self.todos.iter().position(|t| key.is(t))
    }

    /// Display › Order: orders the list shown by `order`, at once in this window, then in the suite
    /// settings (`--demo` stores nothing). Folds, the "+" group and the scroll belong to the old
    /// order. Nothing happens where the list has no order of its own.
    fn set_list_order(&mut self, order: ListOrder) -> Task<Message> {
        let (Some(_), Filter::List(id)) = (self.list_order(), &self.filter) else { return Task::none() };
        let id = id.clone();
        match order {
            ListOrder::DueDate => self.list_orders.remove(&id),
            ListOrder::Manual => self.list_orders.insert(id.clone(), order),
        };
        self.menu = None;
        self.drag = None;
        self.collapsed.clear();
        self.quick_group = None;
        self.scroll = None;
        let Source::Core(client) = &self.source else { return Task::none() };
        self.orders_saving += 1;
        let socket = client.socket.clone();
        Task::perform(save_list_order(socket, list_order_request(id, order)), Message::ListOrderSaved)
    }

    /// Asks the core for the orders of the lists. Done with the appearance on every tick, so a
    /// change in another window reaches this one within a minute. `--demo` has none to ask.
    fn fetch_list_orders(&self) -> Task<Message> {
        let Source::Core(client) = &self.source else { return Task::none() };
        Task::perform(stored_list_orders(client.socket.clone()), Message::ListOrders)
    }

    /// Alt+↑ (`up`) or Alt+↓: moves the task under the cursor one step among the tasks it is
    /// ordered with ([`list::step_target`]); nothing at the edge or outside manual order.
    fn move_step(&mut self, up: bool) -> Task<Message> {
        if !self.manual() {
            return Task::none();
        }
        let Some(index) = self.cursor_index() else { return Task::none() };
        match list::step_target(&self.list_data(), index, up) {
            Some(before) => self.move_task(index, before),
            None => Task::none(),
        }
    }

    /// The grip of `todos[index]` was pressed: it becomes the cursor row and is dragged.
    fn start_drag(&mut self, index: usize) {
        let Some(key) = self.todos.get(index).map(Key::of).filter(|_| self.manual() || self.grips()) else { return };
        self.cursor = Some(key.clone());
        self.drag = Some(Drag { key, place: None, over: None });
    }

    /// The pointer is at `y` in the list's content: the marker goes to the nearest place the
    /// dragged task may go ([`list::drop_places`]).
    fn drag_to(&mut self, y: f32) {
        let Some(index) = self.drag.as_ref().and_then(|drag| self.key_index(&drag.key)) else { return };
        let place = list::nearest_place(&list::drop_places(&self.list_data(), index), y).map(|(place, _)| place);
        if let Some(drag) = self.drag.as_mut() {
            drag.place = place;
        }
    }

    /// The button was released: the dragged task goes where the marker stands (nothing before the
    /// pointer moved).
    fn drop_dragged(&mut self) -> Task<Message> {
        let Some(drag) = self.drag.take() else { return Task::none() };
        if let (Some(index), Some(list)) = (self.key_index(&drag.key), drag.over) {
            return self.drop_on_list(index, list);
        }
        let (Some(index), Some(place)) = (self.key_index(&drag.key), drag.place) else { return Task::none() };
        let places = list::drop_places(&self.list_data(), index);
        match places.into_iter().find(|&(at, _)| at == place) {
            Some((_, before)) => self.move_task(index, before),
            None => Task::none(),
        }
    }

    /// Puts `todos[index]` in front of `todos[before]` (to the end of its level with `None`): at
    /// once here, renumbered like the core does it ([`move_plan`]), then through the core's
    /// `tasks.move_todo` – right away, or after the moves still on their way ([`Moving`]). The
    /// cursor stays on the task and the list scrolls to keep it in view. Nothing when the task is
    /// there already or has never been stored.
    fn move_task(&mut self, index: usize, before: Option<usize>) -> Task<Message> {
        let Some(todo) = self.todos.get(index).cloned() else { return Task::none() };
        let before = before.and_then(|i| self.todos.get(i)).cloned();
        let Some(href) = todo.href.clone() else { return Task::none() };
        let plan = move_plan(&self.todos, &todo, before.as_ref()).unwrap_or_default();
        if plan.is_empty() {
            return Task::none();
        }
        let undo = self.todos.clone();
        for planned in plan {
            if let Some(shown) = self.todos.iter_mut().find(|t| t.href == planned.href) {
                shown.sort_order = planned.sort_order;
            }
        }
        self.cursor = Some(Key::of(&todo));
        let step = (href, before.and_then(|b| b.href));
        let reveal = self.reveal(index);
        if let Some(moving) = self.moving.as_mut() {
            moving.queued.push_back(step);
            return reveal;
        }
        self.moving = Some(Moving { undo, queued: VecDeque::new() });
        Task::batch([self.send_move(step), reveal])
    }

    /// Sends one move to the core with the tasks as this window holds them now (their latest
    /// etags). A task that is gone meanwhile fails the move like the core would.
    fn send_move(&self, (href, before): (String, Option<String>)) -> Task<Message> {
        let find = |href: &str| self.todos.iter().find(|t| t.href.as_deref() == Some(href)).cloned();
        let before = match before {
            Some(before) => find(&before).map(Some),
            None => Some(None),
        };
        let (Some(todo), Some(before)) = (find(&href), before) else {
            return Task::done(Message::Moved(Err(ProviderError::Other(tr("The task is gone").into()))));
        };
        let source = self.source.clone();
        Task::perform(async move { source.move_todo(todo, before).await }, Message::Moved)
    }

    /// The core's answer to a move. Written tasks keep the place this window gave them and take
    /// their new etags; the next move waiting goes out, and after the last one the list is
    /// loaded again. An error puts the list back as it was before the moves still unanswered, says
    /// what happened (a `conflict`: changed elsewhere) and reloads.
    fn moved(&mut self, result: Result<Vec<Todo>, ProviderError>) -> Task<Message> {
        let written = match result {
            Ok(written) => written,
            Err(err) => {
                if let Some(moving) = self.moving.take() {
                    self.todos = moving.undo;
                }
                let text = move_failed(&err);
                self.error = Some(text.clone());
                return self.load().chain(Task::done(Message::MoveFailed(text)));
            }
        };
        for stored in written {
            if let Some(shown) = self.todos.iter_mut().find(|t| t.href.is_some() && t.href == stored.href) {
                shown.etag = stored.etag;
            }
        }
        match self.moving.as_mut().and_then(|moving| moving.queued.pop_front()) {
            Some(next) => self.send_move(next),
            None => {
                self.moving = None;
                self.load()
            }
        }
    }

    /// `--open manual | order | drag`: the first writable task list in manual order, for pictures
    /// of it; `drag` with its first row dragged and the marker two places further down.
    fn open_manual(&mut self) {
        let Some(id) = self.writable_lists().first().map(|list| list.id.clone()) else { return };
        self.filter = Filter::List(id.clone());
        self.list_orders.insert(id, ListOrder::Manual);
        if self.launch.open != Some(Open::Drag) {
            return;
        }
        let Some(&first) = list::order(&self.list_data()).first() else { return };
        let place = list::drop_places(&self.list_data(), first).get(2).map(|&(place, _)| place);
        self.cursor = Some(Key::of(&self.todos[first]));
        self.drag = Some(Drag { key: Key::of(&self.todos[first]), place, over: None });
    }
}

/// Dragging a row onto a list of the sidebar (task G.6c).
impl App {
    /// The lists of the sidebar a task of the list `collection` may be dropped on, by id:
    ///
    /// - from a GitHub filter list every writable task list, where the item becomes a link
    ///   (`tasks.add_link`; one there already is the answer, nothing doubles);
    /// - from a writable task list the other writable lists of the same account, where the task
    ///   moves (`tasks.save_todo` with the new list). The core refuses a move to another account
    ///   (`unsupported`), so those lists are no targets; neither is any list while the accounts are
    ///   not known.
    ///
    /// Empty for every other list (read-only, unknown).
    fn drop_targets(&self, collection: &str) -> Vec<String> {
        let writable = self.writable_lists();
        if self.filter_source(collection).is_some() {
            return writable.into_iter().map(|list| list.id).collect();
        }
        if !writable.iter().any(|list| list.id == collection) {
            return Vec::new();
        }
        let account = |id: &str| account_of(id, &self.accounts, |a| a.id.as_str()).map(|a| a.id.as_str());
        let Some(own) = account(collection) else { return Vec::new() };
        writable
            .into_iter()
            .filter(|list| list.id != collection && account(&list.id) == Some(own))
            .map(|list| list.id)
            .collect()
    }

    /// The rows of the list shown have a grip to drag them onto a list of the sidebar: one list is
    /// shown and its tasks have somewhere to go ([`App::drop_targets`]). The smart lists (Today,
    /// Planned, All) have none; manual order has its own grip ([`App::manual`]).
    fn grips(&self) -> bool {
        matches!(&self.filter, Filter::List(id) if !self.drop_targets(id).is_empty())
    }

    /// The lists of the sidebar the row being dragged may be dropped on; empty without a drag.
    fn dragged_targets(&self) -> Vec<String> {
        let dragged = self.drag.as_ref().and_then(|drag| self.key_index(&drag.key));
        dragged.map(|index| self.drop_targets(&self.todos[index].collection)).unwrap_or_default()
    }

    /// The pointer came onto the list `list` of the sidebar while a row is dragged: the drop goes
    /// there, if it takes the row.
    fn drag_over(&mut self, list: String) {
        if !self.dragged_targets().contains(&list) {
            return;
        }
        if let Some(drag) = self.drag.as_mut() {
            drag.over = Some(list);
        }
    }

    /// `todos[index]` was dropped on the list `list` of the sidebar: an item of a filter list
    /// becomes a link there ([`App::add_link`], "Added to {list}"), a task moves there – out of its
    /// parent, which stays behind, as "Move to list" in the detail does it. Nothing for a list that
    /// does not take it.
    fn drop_on_list(&mut self, index: usize, list: String) -> Task<Message> {
        let Some(todo) = self.todos.get(index).cloned() else { return Task::none() };
        if !self.drop_targets(&todo.collection).contains(&list) {
            return Task::none();
        }
        if self.filter_source(&todo.collection).is_some() {
            let Some(url) = todo.link.map(|link| link.id) else { return Task::none() };
            return self.add_link(list, url, None, None);
        }
        let moved = Todo { collection: list, parent: None, ..todo };
        let source = self.source.clone();
        Task::perform(async move { source.save_todo(moved).await.map_err(|e| e.to_string()) }, Message::TaskMoved)
    }

    /// The core's answer to a move by dragging: "Moved to {list}", then as any save (the detail
    /// follows, the list is loaded again). A refusal is the error line.
    fn task_moved(&mut self, result: Result<Todo, String>) -> Task<Message> {
        match result {
            Ok(saved) => {
                let name = self.collections.iter().find(|c| c.id == saved.collection).map(|c| c.name.clone());
                let added_to = Some(name.unwrap_or_else(|| tr("List").into()));
                let since = iced::time::Instant::now();
                self.notice = Some(Notice { todo: saved.clone(), since, added_to, moved: true });
                self.update(Message::Saved(Ok(saved)))
            }
            Err(err) => {
                self.error = Some(trf("Couldn't move the task: {error}", &[("error", &err)]));
                Task::none()
            }
        }
    }

    /// `--open drop`: the first filter list with its first row dragged onto the first writable
    /// list of the sidebar, for pictures of the drop target.
    fn open_drop(&mut self) {
        let Some(id) = self.collections.iter().find(|c| c.source.is_some()).map(|c| c.id.clone()) else { return };
        self.filter = Filter::List(id);
        let Some(&first) = list::order(&self.list_data()).first() else { return };
        let over = self.drop_targets(&self.todos[first].collection).into_iter().next();
        self.cursor = Some(Key::of(&self.todos[first]));
        self.drag = Some(Drag { key: Key::of(&self.todos[first]), place: None, over });
    }
}

/// The request that stores how the list `list` is ordered (`settings.set_setting` with
/// `list_order`, task P.3a).
fn list_order_request(list: String, order: ListOrder) -> Request {
    Request::SetSetting { key: SettingKey::ListOrder, value: serde_json::json!({ "list": list, "order": order }) }
}

/// Sends `request` (from [`list_order_request`]) to the core at `socket`.
async fn save_list_order(socket: std::path::PathBuf, request: Request) -> Result<(), String> {
    match torro_ipc::client::call(&socket, &request).await {
        Ok(Response::Ok) => Ok(()),
        Ok(Response::Error { message, .. }) => Err(message),
        Ok(other) => Err(format!("unexpected answer: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

/// How the task lists are ordered, as the suite settings at the core `socket` say.
async fn stored_list_orders(socket: std::path::PathBuf) -> Result<BTreeMap<String, ListOrder>, String> {
    match torro_ipc::client::call(&socket, &Request::Settings).await {
        Ok(Response::Settings { settings }) => Ok(settings.list_orders),
        Ok(other) => Err(format!("unexpected answer: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

/// What the error line says when a move failed. A conflict means the list changed elsewhere (in
/// Nextcloud, another app): it is loaded again, so moving once more works.
fn move_failed(err: &ProviderError) -> String {
    match err {
        ProviderError::Conflict { .. } => {
            tr("The list changed elsewhere and was reloaded. Move the task again.").to_string()
        }
        other => trf("Couldn't move the task: {error}", &[("error", other)]),
    }
}

/// "Issue closed" with "Undo" (task G.5): one line on the menu surface with its border and shadow
/// (like the zoom badge), 36 high, the violet check of a closed issue before the words.
fn notice_card<'a>() -> Element<'a, Message> {
    let line = row![
        list::toned(Icon::CircleCheck, icon::SIZE, list::Tone::Merged),
        typo::text(typo::body(), tr("Issue closed")).wrapping(text::Wrapping::None),
        widgets::text_button(None, tr("Undo"), style::button::ghost, Some(Message::Undo)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    container(line)
        .padding(Padding { top: space::xs(), right: space::xs(), bottom: space::xs(), left: space::m() })
        .style(style::container::menu)
        .into()
}

/// "Added to {list}" after a GitHub link went to a list not shown (task G.6b), or "Moved to {list}"
/// after a task was dragged onto another list (`moved`, task G.6c): the card of [`notice_card`]
/// with a check, no action. A long list name is cut with "…".
fn added_card<'a>(name: &str, moved: bool) -> Element<'a, Message> {
    let name = typo::ellipsize(name, typo::body(), size::props());
    let args: [(&str, &dyn std::fmt::Display); 1] = [("list", &name)];
    let text = if moved { trf("Moved to {list}", &args) } else { trf("Added to {list}", &args) };
    let line = row![
        list::toned(Icon::Check, icon::SIZE, list::Tone::Ok),
        typo::text(typo::body(), text).wrapping(text::Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    container(line).padding(Padding::from([space::s(), space::m()])).style(style::container::menu).into()
}

/// "List “{name}” created" after a task list was made (task G.8): the card of [`added_card`].
fn created_card<'a>(name: &str) -> Element<'a, Message> {
    let name = typo::ellipsize(name, typo::body(), size::props());
    let line = row![
        list::toned(Icon::CircleCheck, icon::SIZE, list::Tone::Ok),
        typo::text(typo::body(), new_list::created_text(&name)).wrapping(text::Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    container(line).padding(Padding::from([space::s(), space::m()])).style(style::container::menu).into()
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

/// The symbol of a GitHub filter list in the sidebar and the header. Its definition stores none,
/// so it is read from the searches: one for bugs (`label:bug`) is a bug, one about me (`@me`:
/// assigned, review requested, mentioned) a person with a check, anything else the filter symbol.
fn filter_list_icon(source: &GitHubFilter) -> Icon {
    let any = |needle: &str| source.queries.iter().any(|query| query.to_lowercase().contains(needle));
    if any("label:bug") {
        Icon::Bug
    } else if any("@me") {
        Icon::UserCheck
    } else {
        Icon::ListFilter
    }
}

/// The title of an account in the sidebar, the navigation menu, the detail and the filter dialog
/// (task K.31): its host, `cloud.example.org` for `anna@cloud.example.org`. Only when another
/// account of `shown` has the same host, the full address, so the two stay apart. A GitHub account
/// is "GitHub · login" (its label is a bare login, which has no host to show).
fn account_title(account: &PimAccount, shown: &[&PimAccount]) -> String {
    if account.kind == GITHUB_KIND {
        trf("GitHub · {login}", &[("login", &account.label)])
    } else {
        let twin = shown
            .iter()
            .any(|other| other.kind != GITHUB_KIND && other.id != account.id && other.host() == account.host());
        if twin { account.label.clone() } else { account.host().to_string() }
    }
}

/// A hint over the list ([`App::filter_hints`]): `mark` (14) centred on the axis of the rows'
/// leading symbols, `line` 12 after it, 32 high with the list's line below.
fn hint_row<'a>(mark: Element<'a, Message>, line: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let content =
        row![container(mark).center_x(icon::SIZE), line.into()].spacing(space::m()).align_y(Alignment::Center);
    column![
        container(content).width(Fill).center_y(size::row_group() - LINE).padding([0.0, space::l()]).clip(true),
        rule::horizontal(LINE).style(style::rule),
    ]
    .into()
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
        let _ = app.update(Message::AccountLoaded(Ok(demo_accounts())));
        let names = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(
            grouped(&app),
            (3, Some(vec![names(&["Tasks"]), names(&["Groceries", "Family"]), names(&["Needs me", "Torro bugs"])])),
            "Today, Planned and All stay above the groups, each account has its lists"
        );
        let _ = app.update(Message::AccountLoaded(Ok(vec![account(torro_pim::demo::WORK_ACCOUNT)])));
        assert_eq!(grouped(&app).1, None, "a single account is not grouped");
    }

    /// The GitHub account of the demo.
    fn github_account() -> PimAccount {
        PimAccount { kind: GITHUB_KIND.into(), label: "mahype".into(), ..account(torro_pim::demo::GITHUB_ACCOUNT) }
    }

    /// The accounts of `--demo`: two Nextcloud accounts and the GitHub one.
    fn demo_accounts() -> Vec<PimAccount> {
        let mut accounts = two_accounts();
        accounts.push(github_account());
        accounts
    }

    #[test]
    fn filter_lists_sit_in_the_github_group_with_their_own_symbol() {
        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::AccountLoaded(Ok(demo_accounts())));
        let (_, lists) = app.nav_entries();
        let marks: Vec<(String, Mark)> =
            lists.iter().filter(|e| e.list_id().starts_with("github:")).map(|e| (e.label.clone(), e.mark)).collect();
        assert_eq!(
            marks,
            [("Needs me".to_string(), Mark::Icon(Icon::UserCheck)), ("Torro bugs".to_string(), Mark::Icon(Icon::Bug))]
        );
        assert!(lists.iter().filter(|e| !e.list_id().starts_with("github:")).all(|e| matches!(e.mark, Mark::Ring(_))));
        assert_eq!(account_title(&github_account(), &[]), trf("GitHub · {login}", &[("login", &"mahype")]));
        assert_eq!(account_title(&account("nextcloud:anna@cloud"), &[]), "cloud", "others by their host");
        let _ = app.update(Message::SetFilter(Filter::List(torro_pim::demo::TORRO_BUGS.into())));
        assert_eq!(app.filter_mark(&app.filter), Some(Icon::Bug), "the header shows it too");
    }

    #[test]
    fn account_titles_are_hosts_unless_two_shown_accounts_share_one() {
        let anna = account("nextcloud:anna@cloud.example.org");
        let bob = account("nextcloud:bob@imt.example.org");
        let twin = account("nextcloud:bob@cloud.example.org");
        assert_eq!(account_title(&anna, &[&anna, &bob]), "cloud.example.org", "other hosts: the host");
        assert_eq!(account_title(&anna, &[&anna, &twin]), "nextcloud:anna@cloud.example.org", "same host: address");
        assert_eq!(account_title(&twin, &[&anna, &twin]), "nextcloud:bob@cloud.example.org", "both get the address");
        assert_eq!(account_title(&anna, &[&anna]), "cloud.example.org", "an account alone is not its own twin");
        assert_eq!(account_title(&anna, &[]), "cloud.example.org", "no group shown: the host");

        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::AccountLoaded(Ok(two_accounts())));
        let work = app.accounts.iter().find(|a| a.id == torro_pim::demo::WORK_ACCOUNT).expect("a demo account");
        assert_eq!(account_title(work, &app.shown_accounts()), "work.example.org", "the demo spreads over hosts");
    }

    /// The demo app on the filter list "Torro bugs" with the accounts `accounts`, and the index of
    /// its open issue #145.
    fn on_torro_bugs(accounts: Vec<PimAccount>) -> (App, usize) {
        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::AccountLoaded(Ok(accounts)));
        let _ = app.update(Message::SetFilter(Filter::List(torro_pim::demo::TORRO_BUGS.into())));
        let issue = app
            .todos
            .iter()
            .position(|t| t.collection == torro_pim::demo::TORRO_BUGS && t.uid == "github:mahype/TorroWorkplace#145");
        (app, issue.expect("the sample's open bug"))
    }

    /// `todo` saved in the demo source, as the core would answer it.
    fn stored(app: &App, todo: Todo) -> Todo {
        iced::futures::executor::block_on(app.source.save_todo(todo)).expect("the demo closes and reopens issues")
    }

    #[test]
    fn ticking_an_issue_closes_it_and_undo_reopens_it() {
        let (mut app, issue) = on_torro_bugs(demo_accounts());
        assert_eq!(app.issue_check(&app.todos[issue]), Some(true));
        assert_eq!(app.list_data().issue_lists.get(torro_pim::demo::TORRO_BUGS), Some(&true));
        let ticked = app.toggled(issue).expect("an issue can be ticked");
        assert!(ticked.completed);
        assert!(app.update(Message::ToggleDone(issue)).units() > 0, "the core is asked");

        let closed = stored(&app, ticked);
        let _ = app.update(Message::IssueChanged(Ok(closed.clone())));
        assert!(app.todos[issue].completed, "done at once, before the reload");
        let since = app.notice.as_ref().map(|notice| notice.since).expect("Issue closed · Undo");
        for size in [WIDE, MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }

        // The card goes by itself after a while, and not before.
        let _ = app.update(Message::NoticeTick(since + NOTICE_FOR - Duration::from_secs(1)));
        assert!(app.notice.is_some());
        let undo = app.undo().expect("Undo reopens the closed issue");
        assert!(!undo.completed && undo.uid == closed.uid && undo.etag == closed.etag);
        assert!(app.notice.is_none(), "the card goes with Undo");
        let reopened = stored(&app, undo);
        let _ = app.update(Message::IssueChanged(Ok(reopened)));
        assert!(!app.todos[issue].completed);
        assert!(app.notice.is_none(), "reopening says nothing");

        let _ = app.update(Message::IssueChanged(Ok(closed)));
        let since = app.notice.as_ref().map(|notice| notice.since).expect("closed again");
        let _ = app.update(Message::NoticeTick(since + NOTICE_FOR));
        assert!(app.notice.is_none(), "gone after {NOTICE_FOR:?}");
        assert_eq!(app.update(Message::Undo).units(), 0, "nothing left to undo");
    }

    /// `on_torro_bugs` with the open bug #145 found by "Needs me" as well, that copy first in the
    /// loaded tasks; the index of the copy in "Torro bugs".
    fn with_a_copy_in_another_list(app: &mut App, issue: usize) -> usize {
        let copy = Todo { collection: torro_pim::demo::NEEDS_ME.into(), ..app.todos[issue].clone() };
        let mut todos = app.todos.clone();
        todos.insert(0, copy);
        let _ = app.update(Message::Loaded(Ok((app.collections.clone(), todos))));
        app.todos
            .iter()
            .position(|t| t.collection == torro_pim::demo::TORRO_BUGS && t.uid == app.todos[issue + 1].uid)
            .expect("the bug in its own list")
    }

    #[test]
    fn the_same_issue_in_two_filter_lists_keeps_cursor_and_detail_on_its_own_copy() {
        let (mut app, issue) = on_torro_bugs(demo_accounts());
        let issue = with_a_copy_in_another_list(&mut app, issue);
        let own = |app: &App| app.todos[issue].collection.clone();
        assert!(issue > 0, "the other list's copy comes first");

        let _ = app.select(issue);
        assert_eq!(app.cursor_index(), Some(issue), "the cursor row is the chosen copy, not the first with its uid");
        assert_eq!(app.list_data().selected, Some(issue));
        assert_eq!(app.detail_index(&app.todos[issue].uid), Some(issue));

        // A reload keeps both on that copy.
        let todos = app.todos.clone();
        let _ = app.update(Message::Loaded(Ok((app.collections.clone(), todos))));
        assert_eq!(app.selected, Some(issue));
        assert_eq!(app.detail.as_ref().map(|d| d.original.collection.clone()), Some(own(&app)));
        assert_eq!(app.cursor_index(), Some(issue));

        // The keyboard goes up to the bug above it and back, never through the other list's copy.
        let before = app.cursor.clone();
        let _ = app.step(false);
        assert_ne!(app.cursor, before);
        assert_eq!(app.detail.as_ref().map(|d| d.original.collection.clone()), Some(own(&app)));
        let _ = app.step(true);
        assert_eq!(app.cursor, before);
        assert_eq!(app.selected, Some(issue));
    }

    #[test]
    fn a_filter_list_shows_its_items_in_the_order_of_the_search() {
        let mut app = launched(&["--demo"]);
        let id = torro_pim::demo::NEEDS_ME;
        let _ = app.update(Message::SetFilter(Filter::List(id.into())));
        // One run of rows: the groups of repositories sit apart from the order inside them.
        let _ = app.update(Message::SetGroupBy(list::GroupBy::None));
        let mut expected: Vec<&Todo> = app.todos.iter().filter(|t| t.collection == id).collect();
        expected.sort_by_key(|t| t.sort_order);
        let expected: Vec<String> = expected.into_iter().map(|t| t.uid.clone()).collect();
        assert!(expected.len() > 1, "the sample has several matches");
        assert_eq!(rows(&app), expected, "not by due date, priority and title");

        // A newer answer from the core with another order is shown as it comes.
        let mut todos = app.todos.clone();
        for todo in todos.iter_mut().filter(|t| t.collection == id) {
            todo.sort_order = todo.sort_order.map(|n| 100 - n);
        }
        let _ = app.update(Message::Loaded(Ok((app.collections.clone(), todos))));
        let reversed: Vec<String> = expected.into_iter().rev().collect();
        assert_eq!(rows(&app), reversed);
    }

    #[test]
    fn the_detail_closes_an_issue_like_the_list() {
        let (mut app, issue) = on_torro_bugs(demo_accounts());
        let _ = app.select(issue);
        assert_eq!(app.detail.as_ref().and_then(|detail| detail.issue_check), Some(true));
        let tick = app.update(Message::Detail(detail::Message::ToggleDone));
        assert!(tick.units() > 0, "the tick goes to the core");
        let closed = stored(&app, Todo { completed: true, ..app.todos[issue].clone() });
        let _ = app.update(Message::IssueChanged(Ok(closed.clone())));
        assert_eq!(app.detail.as_ref().map(|detail| detail.original.clone()), Some(closed), "the state follows");
    }

    /// Index of the task of the demo with the link id `github:mahype/TorroWorkplace#<number>` in
    /// the list "Tasks" (a link, task G.6).
    fn link_in_tasks(app: &App, number: u64) -> usize {
        let id = Link::github("mahype/TorroWorkplace", number).id;
        let link = |t: &Todo| t.link.as_ref().is_some_and(|l| l.id == id) && app.filter_source(&t.collection).is_none();
        app.todos.iter().position(link).expect("a link of the sample")
    }

    #[test]
    fn a_pasted_link_is_added_as_a_link_in_a_task_list_and_pinned_in_a_filter_list() {
        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::AccountLoaded(Ok(demo_accounts())));
        let address = "https://github.com/mahype/TorroWorkplace/issues/145";
        let _ = app.update(Message::QuickAddChanged(address.into()));
        assert!(app.update(Message::QuickAddSubmit).units() > 0, "the core is asked");
        assert!(app.quick_add.is_empty() && app.error.is_none());

        // Under "Today" the new link is due today, as any task added there.
        let target = app.target_list().expect("a writable list");
        let added = iced::futures::executor::block_on(app.source.add_link(target, address.into())).expect("added");
        assert_eq!((added.summary.as_str(), added.due), ("Week view: all-day events cover the day names", None));
        let today = Some(EventTime::Date(app.today));
        assert!(app.update(Message::LinkAdded(Ok(added.clone()), today, None)).units() > 0, "saved with its day");
        // Not the list shown: a card says where it went (task G.6b), with nothing to undo.
        let name = app.collections.iter().find(|c| c.id == added.collection).map(|c| c.name.clone());
        assert!(name.is_some());
        assert_eq!(app.notice.as_ref().and_then(|notice| notice.added_to.clone()), name, "Added to {name:?}");
        let _ = app.view();
        assert_eq!(app.update(Message::Undo).units(), 0, "nothing to undo");
        // In the list shown the new row says enough.
        app.notice = None;
        let _ = app.update(Message::SetFilter(Filter::List(added.collection.clone())));
        let _ = app.update(Message::LinkAdded(Ok(added.clone()), None, None));
        assert!(app.notice.is_none());
        let _ = app.update(Message::SetFilter(Filter::Today));
        let refused = Message::LinkAdded(Err("GitHub has no such issue".into()), None, None);
        let _ = app.update(refused);
        assert!(app.error.as_deref().is_some_and(|e| e.contains("GitHub has no such issue")));

        // A filter list takes links only, and pins them.
        let _ = app.update(Message::SetFilter(Filter::List(torro_pim::demo::NEEDS_ME.into())));
        let _ = app.update(Message::QuickAddChanged("Buy oat milk".into()));
        assert_eq!(app.update(Message::QuickAddSubmit).units(), 0, "nothing to pin");
        assert_eq!(app.quick_add, "Buy oat milk", "the text stays to be corrected");
        assert!(app.error.is_some());
        let _ = app.update(Message::QuickAddChanged("mahype/TorroWorkplace#145".into()));
        assert!(app.update(Message::QuickAddSubmit).units() > 0);
        for size in [WIDE, MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
    }

    #[test]
    fn a_link_has_a_check_and_says_issue_closed_only_when_its_issue_closed() {
        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::AccountLoaded(Ok(demo_accounts())));
        let issue = link_in_tasks(&app, 123);
        assert_eq!(app.issue_check(&app.todos[issue]), Some(true), "never locked");
        let ticked = app.toggled(issue).expect("a link can be ticked");
        assert!(app.update(Message::ToggleDone(issue)).units() > 0, "the core is asked");
        let _ = app.update(Message::IssueChanged(Ok(stored(&app, ticked))));
        assert!(app.notice.is_some(), "Issue closed · Undo");
        let undo = app.undo().expect("Undo");
        assert!(!undo.completed);

        let pull = link_in_tasks(&app, 131);
        assert_eq!(app.issue_check(&app.todos[pull]), Some(true), "a link to a pull request is ticked too");
        let ticked = app.toggled(pull).expect("a link can be ticked");
        let _ = app.update(Message::IssueChanged(Ok(stored(&app, ticked))));
        assert!(app.todos[pull].completed && app.notice.is_none(), "a pull request is not closed: no card");
    }

    #[test]
    fn without_write_access_the_check_is_locked_and_a_refusal_is_shown() {
        let mut reader = github_account();
        reader.scopes = Some(vec!["read:org".into()]);
        let mut accounts = two_accounts();
        accounts.push(reader);
        let (mut app, issue) = on_torro_bugs(accounts);
        assert_eq!(app.issue_check(&app.todos[issue]), Some(false));
        assert_eq!(app.list_data().issue_lists.get(torro_pim::demo::TORRO_BUGS), Some(&false));
        assert!(app.toggled(issue).is_none());
        assert_eq!(app.update(Message::ToggleDone(issue)).units(), 0, "nothing goes to GitHub");
        let _ = app.view();

        // A fine-grained token says nothing about its scopes: ticking is tried, GitHub decides.
        let (mut app, issue) = on_torro_bugs(demo_accounts());
        assert!(app.toggled(issue).is_some());
        let refused = "GitHub does not let this token change the issue.";
        let _ = app.update(Message::IssueChanged(Err(refused.into())));
        assert_eq!(app.error, Some(trf("Couldn't change the issue: {error}", &[("error", &refused)])));
        assert!(app.notice.is_none() && !app.todos[issue].completed);
    }

    #[test]
    fn a_filter_list_symbol_comes_from_its_searches() {
        let source = |queries: &[&str]| GitHubFilter {
            queries: queries.iter().map(|q| q.to_string()).collect(),
            pinned: Vec::new(),
            revision: 1,
            truncated: false,
            problem: None,
        };
        assert_eq!(filter_list_icon(&source(&["is:open Label:Bug repo:a/b"])), Icon::Bug);
        assert_eq!(filter_list_icon(&source(&["is:pr", "review-requested:@me"])), Icon::UserCheck);
        assert_eq!(filter_list_icon(&source(&["org:torro is:open"])), Icon::ListFilter);
    }

    #[test]
    fn a_filter_list_groups_by_repository_and_takes_no_new_tasks() {
        use list::GroupBy;
        let mut app = launched(&["--demo"]);
        assert_eq!(app.list_data().group_by, GroupBy::Due);
        let _ = app.update(Message::SetFilter(Filter::List(torro_pim::demo::NEEDS_ME.into())));
        let data = app.list_data();
        assert_eq!((data.group_by, data.read_only), (GroupBy::Repo, true), "by repository, read-only");
        let groups: Vec<list::Group> = list::grouped(&data).into_iter().map(|(group, _)| group).collect();
        assert_eq!(groups.len(), 2, "the two repositories of the sample");
        let _ = app.update(Message::SetGroupBy(GroupBy::None));
        assert_eq!((app.list_data().group_by, app.group_by), (GroupBy::None, GroupBy::Due), "its own choice");
        let _ = app.update(Message::SetFilter(Filter::All));
        assert_eq!(app.list_data().group_by, GroupBy::Due, "the other views keep theirs");
        let _ = app.update(Message::SetFilter(Filter::List(torro_pim::demo::NEEDS_ME.into())));
        assert_eq!(app.list_data().group_by, GroupBy::None, "and the filter list keeps its own");
        // A pull request is never ticked off (G.5): Space does nothing.
        let pull = list::order(&app.list_data()).into_iter().find(|&i| is_open_pull_request(&app.todos[i]));
        let pull = pull.expect("the sample's pull request");
        assert!(app.toggled(pull).is_none());
        assert_eq!(app.update(Message::ToggleDone(pull)).units(), 0);
        for size in [WIDE, MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            let _ = app.update(Message::ToggleDisplay);
            let _ = app.view();
            let _ = app.update(Message::CloseMenu);
        }
    }

    #[test]
    fn a_pinned_item_is_unpinned_through_the_source() {
        let mut app = launched(&["--demo", "--open", "pinned"]);
        assert_eq!(app.filter, Filter::List(torro_pim::demo::NEEDS_ME.into()));
        let detail = app.detail.as_ref().expect("the pinned item is open");
        assert!(detail.pinned && detail.layer(&Anchor::new()).is_some(), "with its \"…\" menu");
        assert!(app.update(Message::Detail(detail::Message::Unpin)).units() > 0, "the request goes out");
        assert_eq!(app.update(Message::Unpinned(Ok(()))).units(), 1, "then the tasks are reloaded");
        let _ = app.update(Message::Unpinned(Err("not pinned".into())));
        assert_eq!(app.error, Some(trf("Unpin failed: {error}", &[("error", &"not pinned")])));
        // An item that is not pinned has no "Unpin".
        let index = app.todos.iter().position(|t| t.collection == torro_pim::demo::TORRO_BUGS).expect("a match");
        let _ = app.update(Message::Select(index));
        assert!(!app.detail.as_ref().expect("open").pinned);
    }

    #[test]
    fn the_filter_list_state_opens_the_list_with_a_hint() {
        let app = launched(&["--demo", "--open", "filter"]);
        assert_eq!(app.filter, Filter::List(torro_pim::demo::TORRO_BUGS.into()), "the one whose refresh failed");
        let source = app.shown_filter_list().expect("a filter list");
        assert_eq!(app.filter_hints(source).len(), 1);
        let mut needs_me = launched(&["--demo"]);
        let _ = needs_me.update(Message::SetFilter(Filter::List(torro_pim::demo::NEEDS_ME.into())));
        let source = needs_me.shown_filter_list().expect("a filter list");
        assert_eq!(needs_me.filter_hints(source).len(), 1, "more than 1,000 matches");
        let _ = needs_me.list();
    }

    #[test]
    fn a_github_account_alone_is_enough_for_the_tasks_app() {
        let mut app = demo_app();
        let _ = app.update(Message::AccountLoaded(Ok(vec![github_account()])));
        assert!(app.connected(), "its filter lists are task lists");
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
        app.cursor.as_ref().map(|key| key.uid.as_str())
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
        PimAccount { id: id.into(), label: id.into(), kind: "nextcloud".into(), state, scopes: None }
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
            assert_eq!(app.cursor.as_ref().map(|key| key.uid.clone()), open);
            // Every size builds its view from this state.
            let _ = app.view();
        }
    }

    #[test]
    fn the_columns_follow_the_window_and_the_open_task() {
        let mut app = app_with_rows();
        connected(&mut app);
        let _ = app.update(Message::Resized(WIDE));
        assert_eq!((app.columns(), app.list_width()), (Columns::List, size::content_max()), "nothing open");
        let _ = press(&mut app, Shortcut::Down);
        let _ = press(&mut app, Shortcut::Open);
        let _ = app.update(Message::Detail(detail::Message::Open(detail::Editor::Due)));
        for (size, sidebar, expected, list) in [
            (WIDE, SidebarMode::Expanded, Columns::ListDetailProps, size::list()),
            (Size::new(1280.0, 840.0), SidebarMode::Expanded, Columns::ListDetail, size::list_medium()),
            (Size::new(1400.0, 840.0), SidebarMode::Rail, Columns::ListDetailProps, size::list()),
            (MEDIUM, SidebarMode::Rail, Columns::ListDetail, size::list_medium()),
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
        assert_eq!(list_detail_from(), 683.0);
        assert_eq!(columns(SizeClass::Medium, list_detail_from(), true), Columns::ListDetail);
        assert_eq!(columns(SizeClass::Medium, list_detail_from() - 1.0, true), Columns::Detail);
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
        assert_eq!(list_width(SizeClass::Medium, Columns::List, 940.0), size::content_max());
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
    fn the_title_band_repeats_what_lies_over_the_window() {
        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::Resized(Size::new(800.0, 600.0)));
        assert_eq!(app.cover(), Cover::Nothing);
        app.drawer = true;
        assert_eq!(app.cover(), Cover::Drawer, "the drawer over a medium window");
        let _ = app.update(Message::Resized(Size::new(240.0, 160.0)));
        assert_eq!(app.cover(), Cover::Nothing, "a glance shows no drawer");

        let mut dialog = launched_with_accounts(&["--demo", "--open", "filter-new"]);
        dialog.drawer = true;
        for (size, cover) in [
            (Size::new(1600.0, 1000.0), Cover::Dialog),
            (Size::new(800.0, 600.0), Cover::Dialog),
            (Size::new(420.0, 780.0), Cover::Filled(Surface::Panel)),
        ] {
            let _ = dialog.update(Message::Resized(size));
            assert_eq!(dialog.cover(), cover, "{size:?}: the dialog, but not the narrow editor's own view");
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
        assert_eq!(launch.now, NaiveDate::from_ymd_opt(2026, 10, 12).and_then(|day| day.and_hms_opt(10, 30, 0)));
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
    }

    /// The message "design changed" (task L.3): the stored setting arrives and the window draws
    /// with that pack from the next frame; a broken one gives the standard.
    #[test]
    fn the_stored_design_is_drawn_and_a_broken_one_gives_the_standard() {
        let dir = std::env::temp_dir().join(format!("torrotasks-designs-{}", std::process::id()));
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

    /// A demo app started with these words, with the demo's accounts (GitHub included) and data.
    fn launched_with_accounts(words: &[&str]) -> App {
        let launch = Launch::parse(&args(words)).expect("valid parameters");
        let (mut app, _) = App::new(launch.source(&args(words)), launch);
        let (collections, _, todos) = torro_pim::demo::sample(app.today);
        let _ = app.update(Message::AccountLoaded(Ok(demo_accounts())));
        let _ = app.update(Message::Loaded(Ok((collections, todos))));
        app
    }

    #[test]
    fn filter_list_dialogs_open_and_their_outcome_is_followed() {
        use torro_pim::demo::{NEEDS_ME, TORRO_BUGS};
        let app = launched_with_accounts(&["--demo", "--open", "filter-edit"]);
        assert!(matches!(app.filter_dialog, Some(filter::Dialog::Edit(_))));
        assert_eq!(app.filter, Filter::List(NEEDS_ME.into()), "the list is shown behind its dialog");

        let mut app = launched_with_accounts(&["--demo", "--open", "filter-delete"]);
        assert!(matches!(app.filter_dialog, Some(filter::Dialog::Delete(_))));
        let _ = app.update(Message::Escape);
        assert!(app.filter_dialog.is_none(), "Escape cancels");
        let _ = app.update(Message::OpenFilterDialog(filter::Open::Delete(NEEDS_ME.into())));
        let _ = app.update(Message::Filter(filter::Message::Deleted(Ok(()))));
        assert_eq!((app.filter_dialog.is_none(), &app.filter), (true, &Filter::Today), "the deleted list is left");

        let mut app = launched_with_accounts(&["--demo", "--open", "filter-new"]);
        assert!(matches!(app.filter_dialog, Some(filter::Dialog::Edit(_))));
        let _ = app.update(Message::Filter(filter::Message::Saved(Ok(TORRO_BUGS.into()))));
        assert_eq!((app.filter_dialog.is_none(), &app.filter), (true, &Filter::List(TORRO_BUGS.into())));

        // An id that is no filter list opens nothing.
        let _ = app.update(Message::OpenFilterDialog(filter::Open::Edit("nope".into())));
        assert!(app.filter_dialog.is_none());
        assert_eq!(app.github_accounts().count(), 1);
    }

    #[test]
    fn new_list_offers_what_the_accounts_allow() {
        use torro_pim::demo::{GITHUB_ACCOUNT, HOME_ACCOUNT, WORK_ACCOUNT};
        let mut app = launched_with_accounts(&["--demo"]);
        let offer = app.list_offer();
        let ids: Vec<&str> = offer.task_accounts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, [WORK_ACCOUNT, HOME_ACCOUNT], "every connected account but GitHub's");
        assert!(offer.github);
        assert_eq!(offer.preferred, None, "Today belongs to no account");
        let home_list = app.collections.iter().find(|c| c.id.starts_with(HOME_ACCOUNT)).map(|c| c.id.clone());
        let _ = app.update(Message::SetFilter(Filter::List(home_list.expect("a list at home"))));
        assert_eq!(app.list_offer().preferred.as_deref(), Some(HOME_ACCOUNT), "the account of the list shown");
        // Only GitHub: no task lists to make.
        let _ = app.update(Message::AccountLoaded(Ok(vec![github_account()])));
        let offer = app.list_offer();
        assert!(offer.task_accounts.is_empty() && offer.github);
        assert_eq!(app.github_accounts().next().map(|a| a.id.as_str()), Some(GITHUB_ACCOUNT));
    }

    #[test]
    fn new_list_hands_a_github_filter_list_to_its_dialog() {
        let mut app = launched_with_accounts(&["--demo", "--open", "new-list"]);
        assert!(app.new_list.is_some(), "the choice is open");
        assert!(app.filter_dialog.is_none());
        let _ = app.update(Message::NewList(new_list::Message::Pick(new_list::Kind::GithubFilter)));
        assert!(app.new_list.is_none());
        assert!(matches!(app.filter_dialog, Some(filter::Dialog::Edit(_))), "a new filter list of the GitHub account");
        // Escape closes the choice, and the keys of the list are off while it shows.
        let _ = app.update(Message::Filter(filter::Message::Cancel));
        let _ = app.update(Message::OpenNewList);
        assert!(app.new_list.is_some());
        let _ = app.update(Message::Escape);
        assert!(app.new_list.is_none(), "Escape closes it");
    }

    #[test]
    fn the_filter_dialog_goes_back_to_the_choice_and_remembers_its_repository() {
        use torro_pim::demo::{GITHUB_ACCOUNT, TORRO_BUGS};
        let design = "mahype/torro-design";
        let repos = || {
            let repos = iced::futures::executor::block_on(
                DemoSource::new(NaiveDate::default()).list_github_repos(GITHUB_ACCOUNT.into()),
            );
            Message::Filter(filter::Message::Repos(GITHUB_ACCOUNT.into(), repos.map_err(|e| e.to_string())))
        };
        let mut app = launched_with_accounts(&["--demo", "--open", "new-list"]);
        let _ = app.update(Message::NewList(new_list::Message::Pick(new_list::Kind::GithubFilter)));
        let _ = app.update(Message::Filter(filter::Message::Back));
        assert!(app.filter_dialog.is_none() && app.new_list.is_some(), "back to the choice");
        let _ = app.update(Message::NewList(new_list::Message::Pick(new_list::Kind::GithubFilter)));
        let _ = app.update(repos());
        let _ = app.update(Message::Filter(filter::Message::PickRepo(design.into())));
        let _ = app.update(Message::Filter(filter::Message::Saved(Ok(TORRO_BUGS.into()))));
        assert_eq!(app.last_repo.as_deref(), Some(design), "remembered on saving");
        // The next new list starts there.
        let _ = app.update(Message::OpenFilterDialog(filter::Open::New(GITHUB_ACCOUNT.into())));
        let _ = app.update(repos());
        assert_eq!(app.filter_dialog.as_ref().and_then(filter::Dialog::repo_to_remember).as_deref(), Some(design));
        // Escape closes the list of repositories before the dialog.
        let _ = app.update(Message::Filter(filter::Message::TogglePicker));
        let _ = app.update(Message::Escape);
        assert!(app.filter_dialog.is_some());
        let _ = app.update(Message::Escape);
        assert!(app.filter_dialog.is_none());
        // The states for pictures: the open list of repositories, a list of one repository.
        let app = launched_with_accounts(&["--demo", "--open", "filter-repos"]);
        assert!(matches!(app.filter_dialog, Some(filter::Dialog::Edit(_))));
        let app = launched_with_accounts(&["--demo", "--open", "filter-repo"]);
        assert_eq!(app.filter, Filter::List(TORRO_BUGS.into()));
    }

    #[test]
    fn a_new_task_list_is_shown_and_announced() {
        use torro_pim::demo::WORK_ACCOUNT;
        let mut app = launched_with_accounts(&["--demo", "--open", "new-task-list"]);
        assert!(app.new_list.as_ref().is_some_and(|dialog| dialog.is_full(SizeClass::Narrow)), "the form is open");
        for size in [WIDE, MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
        // The core's answer to "Create list", as the demo gives it.
        let made = iced::futures::executor::block_on(app.source.create_task_list(
            WORK_ACCOUNT.into(),
            "Reading".into(),
            Some("#3FB984".into()),
        ))
        .expect("the demo makes task lists");
        let _ = app.update(Message::NewList(new_list::Message::Name("Reading".into())));
        let _ = app.update(Message::NewList(new_list::Message::Create));
        let _ = app.update(Message::NewList(new_list::Message::Created(Ok(made.clone()))));
        assert!(app.new_list.is_none(), "the dialog closes");
        assert_eq!(app.filter, Filter::List(made.id.clone()), "the new list is shown");
        assert!(app.collections.iter().any(|c| c.id == made.id), "in the sidebar before the reload");
        assert_eq!(app.filter_name(&app.filter), "Reading");
        let since = app.created.as_ref().map(|card| card.since).expect("List “Reading” created");
        let _ = app.view();
        let _ = app.update(Message::NoticeTick(since + NOTICE_FOR));
        assert!(app.created.is_none(), "the card goes after a while");
    }

    /// The demo on the list "Tasks" in manual order (`--open manual`).
    fn manual_app() -> App {
        let app = launched(&["--demo", "--open", "manual"]);
        assert!(app.manual(), "the first writable list, by hand");
        app
    }

    /// The uids of the rows as the list shows them.
    fn rows(app: &App) -> Vec<String> {
        list::order(&app.list_data()).into_iter().map(|i| app.todos[i].uid.clone()).collect()
    }

    /// The uids of the top-level open rows, in the order shown.
    fn top_rows(app: &App) -> Vec<String> {
        let data = app.list_data();
        let top = |&(i, depth): &(usize, usize)| (depth == 0 && !app.todos[i].completed).then_some(i);
        list::arrange(&data).iter().filter_map(top).map(|i| app.todos[i].uid.clone()).collect()
    }

    fn put_cursor(app: &mut App, uid: &str) {
        app.cursor = app.todos.iter().find(|t| t.uid == uid).map(Key::of);
        assert!(app.cursor.is_some(), "{uid} is loaded");
    }

    #[test]
    fn the_order_is_offered_for_a_single_writable_list_only() {
        let mut app = launched_with_accounts(&["--demo"]);
        assert_eq!(app.list_order(), None, "Today has no order of its own");
        for filter in [Filter::Planned, Filter::All, Filter::List(torro_pim::demo::NEEDS_ME.into())] {
            let _ = app.update(Message::SetFilter(filter.clone()));
            assert_eq!(app.list_order(), None, "{filter:?}");
            assert_eq!(app.update(Message::SetListOrder(ListOrder::Manual)).units(), 0);
            assert!(app.list_orders.is_empty(), "nothing is set for {filter:?}");
        }
        let tasks = app.writable_lists().first().map(|list| list.id.clone()).expect("a writable list");
        let _ = app.update(Message::SetFilter(Filter::List(tasks.clone())));
        assert_eq!(app.list_order(), Some(ListOrder::DueDate), "by due date until set otherwise");
        let _ = app.update(Message::ToggleDisplay);
        assert_eq!(app.update(Message::SetListOrder(ListOrder::Manual)).units(), 0, "the demo keeps it in the process");
        assert_eq!(app.list_orders.get(&tasks), Some(&ListOrder::Manual));
        assert!(app.manual() && app.menu.is_none(), "chosen, the menu closes");
        assert_eq!(list::grouped(&app.list_data()).len(), 1, "no due groups");
        let _ = app.update(Message::SetListOrder(ListOrder::DueDate));
        assert!(app.list_orders.is_empty(), "the default is not kept");
        for size in [WIDE, MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            for order in [ListOrder::DueDate, ListOrder::Manual] {
                let _ = app.update(Message::SetListOrder(order));
                let _ = app.update(Message::ToggleDisplay);
                let _ = app.view();
                let _ = app.update(Message::CloseMenu);
            }
        }
    }

    #[test]
    fn the_menu_writes_the_setting_through_the_core() {
        assert_eq!(
            list_order_request("work".into(), ListOrder::Manual),
            Request::SetSetting {
                key: SettingKey::ListOrder,
                value: serde_json::json!({ "list": "work", "order": "manual" }),
            }
        );
        assert_eq!(
            list_order_request("work".into(), ListOrder::DueDate),
            Request::SetSetting {
                key: SettingKey::ListOrder,
                value: serde_json::json!({ "list": "work", "order": "due_date" }),
            }
        );
        // A core that is never asked here: the tasks are built, not run.
        let core = Source::Core(torro_ipc::pim::PimClient { socket: "no-core.sock".into() });
        let (mut app, _) = App::new(core, Launch::default());
        connected(&mut app);
        let (collections, _, todos) = torro_pim::demo::sample(app.today);
        let _ = app.update(Message::Loaded(Ok((collections, todos))));
        let tasks = app.writable_lists().first().map(|list| list.id.clone()).expect("a writable list");
        let _ = app.update(Message::SetFilter(Filter::List(tasks.clone())));
        assert_eq!(app.update(Message::SetListOrder(ListOrder::Manual)).units(), 1, "set_setting goes out");
        assert_eq!(app.orders_saving, 1);
        let _ = app.update(Message::ListOrders(Ok(BTreeMap::new())));
        assert!(app.manual(), "settings read before the change are not taken");
        let _ = app.update(Message::ListOrderSaved(Ok(())));
        let _ = app.update(Message::ListOrders(Ok(BTreeMap::from([(tasks.clone(), ListOrder::Manual)]))));
        assert!(app.manual() && app.error.is_none());
        let _ = app.update(Message::ListOrders(Ok(BTreeMap::new())));
        assert!(!app.manual(), "changed in another window: followed");
        let _ = app.update(Message::SetListOrder(ListOrder::Manual));
        assert!(app.update(Message::ListOrderSaved(Err("core is not running".into()))).units() > 0, "fetched again");
        let said = trf("Couldn't save the order: {error}", &[("error", &"core is not running")]);
        assert_eq!(app.error, Some(said));
    }

    #[test]
    fn alt_arrows_move_the_task_in_manual_order_and_stop_at_the_edges() {
        let mut app = manual_app();
        assert_eq!(press(&mut app, Shortcut::MoveDown).units(), 0, "nothing without a cursor");
        let before = top_rows(&app);
        assert!(before.len() >= 3, "the sample has tasks to order: {before:?}");
        put_cursor(&mut app, &before[0]);
        assert_eq!(press(&mut app, Shortcut::MoveUp).units(), 0, "the first stays first");
        assert_eq!(top_rows(&app), before);
        assert!(app.moving.is_none());

        assert!(press(&mut app, Shortcut::MoveDown).units() > 0, "the move goes to the core");
        let mut expected = before.clone();
        expected.swap(0, 1);
        assert_eq!(top_rows(&app), expected, "shown at once");
        assert_eq!(cursor(&app), Some(before[0].as_str()), "the cursor stays on the task");
        assert!(app.moving.as_ref().is_some_and(|moving| moving.queued.is_empty()));

        // A second move waits for the answer to the first, then goes out.
        let _ = press(&mut app, Shortcut::MoveDown);
        expected.swap(1, 2);
        assert_eq!(top_rows(&app), expected);
        assert_eq!(app.moving.as_ref().map(|moving| moving.queued.len()), Some(1));
        assert!(app.update(Message::Moved(Ok(Vec::new()))).units() > 0, "the next one is sent");
        assert!(app.moving.as_ref().is_some_and(|moving| moving.queued.is_empty()));
        assert!(app.update(Message::Moved(Ok(Vec::new()))).units() > 0, "after the last one: reload");
        assert!(app.moving.is_none());

        let last = expected.last().cloned().expect("a last row");
        put_cursor(&mut app, &last);
        assert_eq!(press(&mut app, Shortcut::MoveDown).units(), 0, "the last stays last");
        assert!(press(&mut app, Shortcut::MoveUp).units() > 0);
        let n = expected.len();
        expected.swap(n - 2, n - 1);
        assert_eq!(top_rows(&app), expected);
    }

    #[test]
    fn subtasks_move_under_their_parent_and_parents_take_them_along() {
        let mut app = manual_app();
        let shown = rows(&app);
        let parent_of = |uid: &str| app.todos.iter().find(|t| t.uid == uid).and_then(|t| t.parent.clone());
        let first_kid = shown.iter().find(|uid| parent_of(uid).is_some()).expect("the sample has subtasks");
        let parent = parent_of(first_kid).expect("a parent");
        let kids: Vec<String> = shown.iter().filter(|uid| parent_of(uid).as_ref() == Some(&parent)).cloned().collect();
        assert!(kids.len() >= 2, "a task with two subtasks or more");
        let block = |rows: &[String]| {
            let at = rows.iter().position(|uid| *uid == parent).expect("the parent is shown");
            rows[at..=at + kids.len()].to_vec()
        };
        let mut family = block(&shown);
        assert_eq!(&family[1..], &kids[..], "the subtasks follow their parent");

        put_cursor(&mut app, &kids[0]);
        assert_eq!(press(&mut app, Shortcut::MoveUp).units(), 0, "the first subtask stays under its parent");
        assert!(press(&mut app, Shortcut::MoveDown).units() > 0);
        family.swap(1, 2);
        assert_eq!(block(&rows(&app)), family, "moved among its siblings, still under the parent");
        put_cursor(&mut app, family.last().expect("a last subtask"));
        let _ = app.update(Message::Moved(Ok(Vec::new())));
        let last = rows(&app);
        assert_eq!(press(&mut app, Shortcut::MoveDown).units(), 0, "the last subtask does not leave its parent");
        assert_eq!(rows(&app), last);

        // The parent moves with its subtasks.
        put_cursor(&mut app, &parent);
        let top = top_rows(&app);
        let up = top.iter().position(|uid| *uid == parent).is_some_and(|at| at > 0);
        let _ = press(&mut app, if up { Shortcut::MoveUp } else { Shortcut::MoveDown });
        assert_eq!(block(&rows(&app)), family, "the subtasks come along");
        assert_ne!(top_rows(&app), top);
    }

    #[test]
    fn a_failed_move_goes_back_says_so_and_reloads() {
        let mut app = manual_app();
        let before = rows(&app);
        let orders: Vec<Option<i64>> = app.todos.iter().map(|t| t.sort_order).collect();
        let first = top_rows(&app)[0].clone();
        put_cursor(&mut app, &first);
        let _ = press(&mut app, Shortcut::MoveDown);
        let _ = press(&mut app, Shortcut::MoveDown);
        assert_ne!(rows(&app), before);
        let reload = app.update(Message::Moved(Err(ProviderError::Conflict { uid: String::new() })));
        assert!(reload.units() > 0, "loaded again");
        assert_eq!(rows(&app), before, "back as before the first unanswered move");
        assert_eq!(app.todos.iter().map(|t| t.sort_order).collect::<Vec<_>>(), orders);
        assert!(app.moving.is_none(), "the waiting move is dropped");
        let conflict = tr("The list changed elsewhere and was reloaded. Move the task again.");
        assert_eq!(app.error.as_deref(), Some(conflict));
        // The reload clears the error line; the message brings it back.
        let (collections, _, todos) = torro_pim::demo::sample(app.today);
        let _ = app.update(Message::Loaded(Ok((collections, todos))));
        let _ = app.update(Message::MoveFailed(conflict.into()));
        assert_eq!(app.error.as_deref(), Some(conflict));

        let _ = press(&mut app, Shortcut::MoveDown);
        let _ = app.update(Message::Moved(Err(ProviderError::Offline)));
        let said = trf("Couldn't move the task: {error}", &[("error", &ProviderError::Offline)]);
        assert_eq!(app.error, Some(said));
        assert_eq!(rows(&app), before);
    }

    #[test]
    fn a_move_reaches_the_demo_source_and_a_stale_one_is_a_conflict() {
        let mut app = manual_app();
        let top = top_rows(&app);
        let index = |app: &App, uid: &str| app.todos.iter().position(|t| t.uid == uid).expect("shown");
        let (first, second) = (app.todos[index(&app, &top[0])].clone(), app.todos[index(&app, &top[1])].clone());
        // What the core answers when the first task goes behind the second (the third is in front).
        let third = app.todos[index(&app, &top[2])].clone();
        let written = iced::futures::executor::block_on(app.source.move_todo(first.clone(), Some(third)));
        let written = written.expect("the demo moves it");
        assert!(!written.is_empty());
        let stale = iced::futures::executor::block_on(app.source.move_todo(first, Some(second)));
        assert!(matches!(stale, Err(ProviderError::Conflict { .. })), "{stale:?}");
        assert_eq!(move_failed(&ProviderError::Conflict { uid: String::new() }), app_conflict_text());
        let _ = app.update(Message::Moved(Ok(written)));
    }

    fn app_conflict_text() -> String {
        tr("The list changed elsewhere and was reloaded. Move the task again.").to_string()
    }

    #[test]
    fn a_row_is_dragged_by_its_grip_to_the_marker() {
        let mut app = manual_app();
        let top = top_rows(&app);
        let first = app.todos.iter().position(|t| t.uid == top[0]).expect("shown");
        let idle = app.subscription().units();
        let _ = app.update(Message::DragStart(first));
        assert_eq!(app.drag, Some(Drag { key: Key::of(&app.todos[first]), place: None, over: None }));
        assert_eq!(cursor(&app), Some(top[0].as_str()), "the dragged row is the cursor row");
        assert_eq!(app.subscription().units(), idle + 2, "Escape and the release listen while dragging");
        assert_eq!(app.update(Message::DragEnd).units(), 0, "released before it moved: nothing");
        assert!(app.drag.is_none());

        let _ = app.update(Message::DragStart(first));
        let places = list::drop_places(&app.list_data(), first);
        let (place, _) = places[2];
        let _ = app.update(Message::DragMove(place as f32 * size::row() + 3.0));
        assert_eq!(app.drag.as_ref().and_then(|drag| drag.place), Some(place), "the marker snaps to the place");
        for size in [WIDE, MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
        assert!(app.update(Message::DragEnd).units() > 0, "dropped: the move goes out");
        let mut expected = top.clone();
        expected.swap(0, 1);
        assert_eq!(top_rows(&app), expected, "in front of the third, behind the second");

        // Escape lets go without moving.
        let _ = app.update(Message::Moved(Ok(Vec::new())));
        let now = rows(&app);
        let _ = app.update(Message::DragStart(first));
        let _ = app.update(Message::DragMove(0.0));
        let _ = app.update(Message::Escape);
        assert!(app.drag.is_none());
        assert_eq!(app.update(Message::DragEnd).units(), 0);
        assert_eq!(rows(&app), now);
    }

    #[test]
    fn the_drag_state_starts_with_the_marker_shown() {
        let app = launched(&["--demo", "--open", "drag"]);
        assert!(app.manual());
        assert!(app.drag.as_ref().is_some_and(|drag| drag.place.is_some()));
        let order = launched(&["--demo", "--open", "order"]);
        assert_eq!(order.menu, Some(Menu::Display));
        assert!(order.manual());
    }

    #[test]
    fn alt_with_the_arrows_moves_and_nothing_else() {
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
        let up = shortcut(press(Key::Named(Named::ArrowUp), Modifiers::ALT), Status::Ignored, id);
        assert!(matches!(up, Some(Message::Shortcut(Shortcut::MoveUp))));
        let down = shortcut(press(Key::Named(Named::ArrowDown), Modifiers::ALT), Status::Ignored, id);
        assert!(matches!(down, Some(Message::Shortcut(Shortcut::MoveDown))));
        assert!(shortcut(press(Key::Character("j".into()), Modifiers::ALT), Status::Ignored, id).is_none());
        let both = Modifiers::ALT | Modifiers::CTRL;
        assert!(shortcut(press(Key::Named(Named::ArrowUp), both), Status::Ignored, id).is_none());
        assert!(shortcut(press(Key::Named(Named::ArrowUp), Modifiers::ALT), Status::Captured, id).is_none());
        // Outside manual order Alt+↓ does nothing.
        let mut app = app_with_rows();
        let _ = app.update(Message::ShortcutChecked(Shortcut::Down, false));
        assert_eq!(app.update(Message::ShortcutChecked(Shortcut::MoveDown, false)).units(), 0);
    }

    // ----- Dragging onto the sidebar (task G.6c) ------------------------------------------------

    /// The id of the demo's list named `name`.
    fn list_named(app: &App, name: &str) -> String {
        app.collections.iter().find(|c| c.name == name).map(|c| c.id.clone()).expect("a list of the demo")
    }

    /// The first row of the list shown, as an index into `todos`.
    fn first_row(app: &App) -> usize {
        list::order(&app.list_data()).first().copied().expect("a row")
    }

    #[test]
    fn an_item_of_a_filter_list_dragged_onto_a_list_becomes_a_link_there() {
        let mut app = launched_with_accounts(&["--demo"]);
        let _ = app.update(Message::SetFilter(Filter::List(torro_pim::demo::NEEDS_ME.into())));
        assert!(app.grips() && app.list_data().grips, "its rows have a grip");
        let tasks = list_named(&app, "Tasks");
        let first = first_row(&app);
        let _ = app.update(Message::DragStart(first));
        // Every writable list takes it, whatever the account; filter lists do not.
        let mut targets = app.dragged_targets();
        targets.sort();
        let mut writable: Vec<String> = app.writable_lists().into_iter().map(|c| c.id).collect();
        writable.sort();
        assert_eq!(targets, writable);
        assert!(!targets.iter().any(|id| id.starts_with("github:")));

        let _ = app.update(Message::DragOver(torro_pim::demo::TORRO_BUGS.into()));
        assert_eq!(app.drag.as_ref().and_then(|drag| drag.over.clone()), None, "a filter list takes nothing");
        let _ = app.update(Message::DragOver(tasks.clone()));
        assert_eq!(app.drag.as_ref().and_then(|drag| drag.over.clone()), Some(tasks.clone()), "the target");
        for size in [WIDE, MEDIUM] {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
        assert!(app.update(Message::DragEnd).units() > 0, "the core is asked to add the link");
        assert!(app.drag.is_none());

        // The core's answer: "Added to Tasks".
        let url = app.todos[first].link.as_ref().map(|link| link.id.clone()).expect("a GitHub item");
        let added = iced::futures::executor::block_on(app.source.add_link(tasks.clone(), url)).expect("added");
        assert_eq!(added.collection, tasks);
        let _ = app.update(Message::LinkAdded(Ok(added), None, None));
        assert_eq!(app.notice.as_ref().and_then(|n| n.added_to.as_deref()), Some("Tasks"));
        assert!(!app.notice.as_ref().is_some_and(|n| n.moved));
    }

    #[test]
    fn a_task_dragged_onto_another_list_of_its_account_moves_there() {
        let mut app = launched_with_accounts(&["--demo"]);
        let (groceries, family, tasks) =
            (list_named(&app, "Groceries"), list_named(&app, "Family"), list_named(&app, "Tasks"));
        let _ = app.update(Message::SetFilter(Filter::List(groceries.clone())));
        // Family is in the same account; Tasks is in the other one, the core would refuse the move.
        assert_eq!(app.drop_targets(&groceries), std::slice::from_ref(&family));
        assert!(app.grips());
        let first = first_row(&app);
        let _ = app.update(Message::DragStart(first));
        let _ = app.update(Message::DragOver(tasks.clone()));
        assert_eq!(app.drag.as_ref().and_then(|drag| drag.over.clone()), None, "no target across accounts");
        let _ = app.update(Message::DragOver(family.clone()));
        // Leaving another row afterwards (events in either order) keeps the target.
        let _ = app.update(Message::DragLeave(tasks.clone()));
        assert_eq!(app.drag.as_ref().and_then(|drag| drag.over.clone()), Some(family.clone()));
        let _ = app.update(Message::DragLeave(family.clone()));
        assert_eq!(app.drag.as_ref().and_then(|drag| drag.over.clone()), None);
        let _ = app.update(Message::DragOver(family.clone()));
        let _ = app.view();
        assert!(app.update(Message::DragEnd).units() > 0, "the core is asked to move it");

        // The core's answer: "Moved to Family", the row leaves the list with the reload.
        let moved = Todo { collection: family.clone(), parent: None, ..app.todos[first].clone() };
        let moved = stored(&app, moved);
        let _ = app.update(Message::TaskMoved(Ok(moved)));
        let notice = app.notice.as_ref().expect("a card");
        assert_eq!((notice.added_to.as_deref(), notice.moved), (Some("Family"), true));
        let _ = app.view();
        assert_eq!(app.update(Message::Undo).units(), 0, "nothing to undo");
        let _ = app.update(Message::TaskMoved(Err("The server did not answer".into())));
        assert!(app.error.as_deref().is_some_and(|e| e.contains("The server did not answer")));

        // Tasks has no other list in its account, the smart lists have no grip.
        assert!(app.drop_targets(&tasks).is_empty());
        let _ = app.update(Message::SetFilter(Filter::List(tasks)));
        assert!(!app.grips());
        let _ = app.update(Message::DragStart(first_row(&app)));
        assert!(app.drag.is_none(), "nothing to drag to");
        let _ = app.update(Message::SetFilter(Filter::Today));
        assert!(!app.grips());
    }

    #[test]
    fn without_the_accounts_a_task_has_nowhere_to_go() {
        let app = launched(&["--demo"]);
        let groceries = list_named(&app, "Groceries");
        assert!(app.drop_targets(&groceries).is_empty(), "the account of a list is not known yet");
        assert!(!app.drop_targets(torro_pim::demo::NEEDS_ME).is_empty(), "a link may go anywhere");
    }

    #[test]
    fn the_drop_state_shows_a_filter_list_row_over_a_list() {
        let app = launched(&["--demo", "--open", "drop"]);
        assert!(app.shown_filter_list().is_some());
        let over = app.drag.as_ref().and_then(|drag| drag.over.clone()).expect("over a list");
        assert!(app.writable_lists().iter().any(|list| list.id == over));
        assert_eq!(app.list_data().drag.and_then(|(_, place)| place), None, "no marker in the list");
    }

    #[test]
    fn ticking_a_link_whose_issue_stays_open_shows_no_issue_closed_card() {
        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::AccountLoaded(Ok(demo_accounts())));
        let issue = link_in_tasks(&app, 123);
        let ticked = app.toggled(issue).expect("a link can be ticked");
        // A list shared with others: the core marks the task done and leaves the issue open.
        let _ = app.update(Message::IssueChanged(Ok(Todo { etag: Some("\"2\"".into()), ..ticked.clone() })));
        assert!(app.todos[issue].completed, "the task is done");
        assert!(app.notice.is_none(), "no Issue closed · Undo");
        // Without GitHub's details in the answer, the same.
        let bare = Todo { link: ticked.link.clone().map(|link| Link { github: None, ..link }), ..ticked.clone() };
        let _ = app.update(Message::IssueChanged(Ok(bare)));
        assert!(app.notice.is_none());
    }

    #[test]
    fn an_issue_closed_before_the_tick_brings_no_card() {
        let mut app = launched(&["--demo"]);
        let _ = app.update(Message::AccountLoaded(Ok(demo_accounts())));
        let issue = link_in_tasks(&app, 123);
        let closed = stored(&app, Todo { completed: true, ..app.todos[issue].clone() });
        // The list already knew it closed (GitHub, or a tick elsewhere): this tick closed nothing.
        app.todos[issue] = Todo { completed: false, ..closed.clone() };
        let _ = app.update(Message::IssueChanged(Ok(closed)));
        assert!(app.notice.is_none());
    }
}
