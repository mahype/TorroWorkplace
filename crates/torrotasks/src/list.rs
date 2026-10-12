//! The task list (task K.14 in `docs/tasks/kalender-aufgaben.md`, Linear look from T.1 and the
//! redesign R.2 in `docs/tasks/design.md`, mock-up `docs/prototypes/tasks/redesign.html`).
//!
//! [`arrange`] picks and orders the tasks for a filter, [`grouped`] splits them into the groups
//! the Display menu asks for ([`GroupBy`]: due date – Overdue, Today, Upcoming, No date,
//! Completed –, list, priority, GitHub repository – what a filter list starts with, task G.3 –,
//! or none); [`view`] renders them:
//!
//! - **Group heads** (32, `raised`, flush, no gap and no double line): a chevron to fold the group
//!   (`Message::ToggleGroup`), its icon on the axis of the rows' leading symbol, the name in
//!   `CAPTION`/`text_2`, the number of open tasks in `META`/`text_3`, and "+" at the right, which
//!   adds a task to this group (`Message::AddInGroup`). Heads only touch with a line between them
//!   when a group is folded.
//! - **Rows** (40, [`widgets::list_row`]): the leading symbol – a task's round check in its list's
//!   colour (→ `ToggleDone(index)`), for an issue of a GitHub filter list its check in GitHub's
//!   colours ([`issue_check`], task G.5: ticking closes the issue; locked with a tooltip when the
//!   account's token may not write issues), for a pull request of a filter list its type symbol
//!   in the colour of its state (no check), for a link in a task list (issue or pull request, task
//!   G.6) the same check as an issue of a filter list, always tickable –, the priority column (only filled when set; urgent is an
//!   orange square with "!"), `repo#123` for GitHub items, the title on one line with "…", hints
//!   (sub-tasks "2/4", notes), and at the right the GitHub labels (two and "+n"), checks, a review
//!   asked for, where the item comes from (list or repository as a pill) and the due date in
//!   `META` tabular (overdue in the attention colour – never Torro red). Done: `text_3`, struck
//!   through. A click on the row → `Select(index)`. Subtasks sit indented under their parent.
//! - **Narrower lists** drop parts in a fixed order instead of wrapping ([`RowParts`]): labels
//!   first, then the hints, the pill becomes a dot, `repo#123` becomes `#123`. A row never takes
//!   two lines.
//!
//! Empty → an empty state ("Nothing to do"; in a filter list "No matches"). A filter list
//! ([`ListData::read_only`]) offers no "+" in its group heads.
//!
//! For the keyboard (T.3) [`order`] gives the visible order of the rows and [`row_span`] where a
//! row lies in the scrolled content, so `app.rs` can step through the rows and keep the chosen
//! one in view; the scrollable reports its viewport as `Message::Scrolled`.
//!
//! **Manual order** (task P.3b, [`ListData::manual`]): one run of rows without group heads, sorted
//! as the user put them ([`manual_order`]; open before done, subtasks under their parent). A row
//! shows a grip in its left inner margin on hover; pressing it starts a drag
//! (`Message::DragStart`), the list reports the pointer's height (`Message::DragMove`) and shows a
//! 2-px marker in `text_2` where the task would go. [`drop_places`] says where a task may go – only
//! among the tasks it is ordered with ([`same_level`]) – and [`step_target`] where Alt+↑/↓ takes it.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::{
    Column, Space, button, center, checkbox, column, container, hover, mouse_area, responsive, rich_text, row, rule,
    scrollable, span, stack, svg, text,
};
use iced::{Alignment, Background, Color, Element, Fill, Length, Padding, Theme, border, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, Palette, palette, radius, size, space};
use torro_gui::typo::{self, ellipsize, text_width};
use torro_gui::{datetime, style, widgets};
use torro_pim::model::{ChecksState, GitHubDetails, GitHubKind, GitHubState, Link, ReviewState};
use torro_pim::source::{manual_order, same_level};
use torro_pim::{EventTime, Todo};

use crate::app::{Filter, Message};

/// What the list gets to draw.
pub struct ListData<'a> {
    pub todos: &'a [Todo],
    /// Collection id → colour.
    pub colors: HashMap<String, Color>,
    /// Collection id → name, for the list pill and the heads of [`GroupBy::List`].
    pub names: HashMap<String, String>,
    pub filter: &'a Filter,
    pub show_completed: bool,
    pub today: NaiveDate,
    /// Index into `todos` of the highlighted row (the keyboard cursor, the task in the detail).
    pub selected: Option<usize>,
    /// Whether rows say where they come from (list pill or dot): off in a list filter, where every
    /// row has the same list.
    pub show_list: bool,
    /// How the rows are grouped.
    pub group_by: GroupBy,
    /// Groups folded away: their head shows, their rows do not.
    pub collapsed: &'a HashSet<Group>,
    /// The width of the list column in logical pixels; decides which parts a row keeps
    /// ([`RowParts::for_width`]).
    pub width: f32,
    /// The colours of the current mode, for marks that take a fixed colour.
    pub palette: &'static Palette,
    /// The list takes no new tasks: a GitHub filter list (task G.3). Its empty state says that
    /// nothing matches, and no group head offers "+".
    pub read_only: bool,
    /// The GitHub filter lists by id, each with whether ticking one of its issues may close it on
    /// GitHub (task G.5, `PimAccount::may_close_issues` of its account). Their issues get the
    /// check of [`issue_check`], locked where this is `false`; their pull requests none. A GitHub
    /// item in any other list is a link (task G.6) and gets that check, never locked.
    pub issue_lists: HashMap<String, bool>,
    /// The list is in "Manual order" (Display › Order, task P.3b): one run without group heads,
    /// sorted by [`manual_order`] (open before done, subtasks under their parent), each row with a
    /// grip in its left inner margin on hover to drag it. Only a single writable list is.
    pub manual: bool,
    /// While a row is dragged by its grip: the task (index into `todos`) and the place in [`order`]
    /// the marker stands above (`order.len()`: under the last row; `None` until the pointer moved).
    pub drag: Option<(usize, Option<usize>)>,
}

/// The tasks to show as `(index into todos, depth)`: `depth` 0 for top-level tasks, 1 for
/// their subtasks and so on. Filter: `Today` = due today or earlier; `Planned` = with a due
/// date; `All`; `List(id)` = that collection. Completed ones only with `show_completed`.
/// Order: open before done, then by due date (none last), then priority (1 first, none
/// last), then title – in [`ListData::manual`] by [`manual_order`] instead; subtasks follow their
/// parent in the same order. A subtask whose parent is filtered out is shown at depth 0.
pub fn arrange(data: &ListData<'_>) -> Vec<(usize, usize)> {
    let todos = data.todos;
    let mut shown: Vec<usize> =
        (0..todos.len()).filter(|&i| is_shown(&todos[i], data.filter, data.show_completed, data.today)).collect();
    if data.manual {
        // Done tasks are a level of their own (`same_level`), so they stay below the open ones.
        shown.sort_by(|&a, &b| {
            todos[a].completed.cmp(&todos[b].completed).then_with(|| manual_order(&todos[a], &todos[b]))
        });
    } else {
        shown.sort_by(|&a, &b| compare(&todos[a], &todos[b]));
    }

    // Sorting first means the roots and every children list come out in display order.
    let uids: HashSet<&str> = shown.iter().map(|&i| todos[i].uid.as_str()).collect();
    let mut tree = Tree { todos, children: HashMap::new(), visited: vec![false; todos.len()], out: Vec::new() };
    let mut roots = Vec::new();
    for &i in &shown {
        match todos[i].parent.as_deref() {
            Some(parent) if uids.contains(parent) => tree.children.entry(parent).or_default().push(i),
            _ => roots.push(i),
        }
    }
    for root in roots {
        tree.push(root, 0);
    }
    // Subtasks whose parent chain loops back on itself have no root; list them at depth 0
    // so that no task silently disappears. Already placed tasks are skipped.
    for &i in &shown {
        tree.push(i, 0);
    }
    tree.out
}

/// Walks the parent/child structure depth first.
struct Tree<'a> {
    todos: &'a [Todo],
    /// Parent uid → indices of its (shown) subtasks, in display order.
    children: HashMap<&'a str, Vec<usize>>,
    visited: Vec<bool>,
    out: Vec<(usize, usize)>,
}

impl Tree<'_> {
    fn push(&mut self, index: usize, depth: usize) {
        if self.visited[index] {
            return;
        }
        self.visited[index] = true;
        self.out.push((index, depth));
        let todos = self.todos;
        let kids = self.children.get(todos[index].uid.as_str()).cloned().unwrap_or_default();
        for kid in kids {
            self.push(kid, depth + 1);
        }
    }
}

/// Whether the filter and the completed switch let the task through.
fn is_shown(todo: &Todo, filter: &Filter, show_completed: bool, today: NaiveDate) -> bool {
    if todo.completed && !show_completed {
        return false;
    }
    match filter {
        Filter::Today => todo.due.is_some_and(|due| due_date(due) <= today),
        Filter::Planned => todo.due.is_some(),
        Filter::All => true,
        Filter::List(id) => &todo.collection == id,
    }
}

/// Open before done, then due (none last), priority (none last), title.
fn compare(a: &Todo, b: &Todo) -> Ordering {
    a.completed
        .cmp(&b.completed)
        .then_with(|| cmp_none_last(a.due.map(due_local), b.due.map(due_local)))
        .then_with(|| cmp_none_last(priority_rank(a.priority), priority_rank(b.priority)))
        .then_with(|| a.summary.to_lowercase().cmp(&b.summary.to_lowercase()))
}

fn cmp_none_last<T: Ord>(a: Option<T>, b: Option<T>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// iCalendar priorities run 1 (highest) to 9 (lowest); 0 and anything else count as none.
fn priority_rank(priority: Option<u8>) -> Option<u8> {
    match priority {
        Some(rank @ 1..=9) => Some(rank),
        _ => None,
    }
}

/// A due date or instant as local time. All-day dues count as the start of their day.
fn due_local(due: EventTime) -> NaiveDateTime {
    match due {
        EventTime::Date(day) => day.and_time(NaiveTime::MIN),
        EventTime::DateTime(at) => datetime::to_local(at),
    }
}

/// The local calendar day a due falls on.
fn due_date(due: EventTime) -> NaiveDate {
    due_local(due).date()
}

/// An open task whose day lies before `today`.
fn is_overdue(todo: &Todo, today: NaiveDate) -> bool {
    !todo.completed && todo.due.is_some_and(|due| due_date(due) < today)
}

/// `Today`, `Tomorrow`, `Yesterday`, otherwise the short date (`Mon, 12 Oct`,
/// `datetime::format_short_date`); a time of day is appended, after a comma behind a word
/// (`Today, 14:00`, as in the mock-up) and a space behind a date (`Mon, 12 Oct 14:00`).
pub(crate) fn due_text(due: EventTime, today: NaiveDate) -> String {
    let day = due_date(due);
    let (name, word) = match (day - today).num_days() {
        0 => (tr("Today").to_string(), true),
        1 => (tr("Tomorrow").to_string(), true),
        -1 => (tr("Yesterday").to_string(), true),
        _ => (datetime::format_short_date(day, today), false),
    };
    match due {
        EventTime::Date(_) => name,
        EventTime::DateTime(at) => {
            let time = datetime::format_time(datetime::to_local(at).time());
            if word { format!("{name}, {time}") } else { format!("{name} {time}") }
        }
    }
}

/// The due date as a row shows it, short as in the mock-up: a day within a week of today is
/// "Fri 9", one further away "20 Oct" (with the year when it is another one). A task due today
/// shows its time ("16:00") – or nothing when it has none – under a "Today" head, which says the
/// day already (`under_today`); elsewhere "Today" (and the time).
pub fn row_due(due: EventTime, today: NaiveDate, under_today: bool) -> String {
    let day = due_date(due);
    let time = match due {
        EventTime::Date(_) => None,
        EventTime::DateTime(at) => Some(datetime::format_time(datetime::to_local(at).time())),
    };
    if day == today {
        return match (under_today, time) {
            (true, time) => time.unwrap_or_default(),
            (false, Some(time)) => format!("{} {time}", tr("Today")),
            (false, None) => tr("Today").to_string(),
        };
    }
    if (day - today).num_days().abs() <= DAYS_WITH_WEEKDAY {
        return trf("{weekday} {day}", &[("weekday", &datetime::weekday_name(day, true)), ("day", &day.format("%-d"))]);
    }
    if day.format("%Y").to_string() != today.format("%Y").to_string() {
        return datetime::format_short_date(day, today);
    }
    datetime::format_day_month(day)
}

/// How far from today a row's date names the weekday ("Fri 9") instead of the month.
const DAYS_WITH_WEEKDAY: i64 = 6;

/// The priority of a task in the app's five classes – the one mapping between iCalendar's nine
/// levels and what rows, group heads, the detail and its menu show. iCalendar 1 is urgent (the
/// orange square), 2–4 high, 5 medium, 6–9 low; 0, nothing and anything else is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Prio {
    Urgent,
    High,
    Medium,
    Low,
    None,
}

impl Prio {
    /// The classes in the order the priority menu offers them.
    pub const ALL: [Prio; 5] = [Prio::None, Prio::Urgent, Prio::High, Prio::Medium, Prio::Low];

    /// The class of an iCalendar priority.
    pub fn of(priority: Option<u8>) -> Prio {
        match priority {
            Some(1) => Prio::Urgent,
            Some(2..=4) => Prio::High,
            Some(5) => Prio::Medium,
            Some(6..=9) => Prio::Low,
            _ => Prio::None,
        }
    }

    /// The iCalendar priority saved for this class: urgent 1, high 3 (inside 1–4, which clients
    /// that only know "high" read as high), medium 5, low 9; `None` saves none.
    /// `Prio::of(prio.ical()) == prio` for every class.
    pub fn ical(self) -> Option<u8> {
        match self {
            Prio::Urgent => Some(1),
            Prio::High => Some(3),
            Prio::Medium => Some(5),
            Prio::Low => Some(9),
            Prio::None => None,
        }
    }

    /// How many of the three bars are lit: high 3, medium 2, low 1, none 0. Urgent has a glyph of
    /// its own ([`prio_glyph`]); counted as bars it is all three.
    pub fn level(self) -> usize {
        match self {
            Prio::Urgent | Prio::High => 3,
            Prio::Medium => 2,
            Prio::Low => 1,
            Prio::None => 0,
        }
    }

    /// The short name: in group heads, the detail's chips, properties and menu.
    pub fn label(self) -> &'static str {
        match self {
            Prio::Urgent => tr("Urgent"),
            Prio::High => tr("High"),
            Prio::Medium => tr("Medium"),
            Prio::Low => tr("Low"),
            Prio::None => tr("No priority"),
        }
    }
}

/// How the Display menu groups the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupBy {
    /// Overdue, Today, Upcoming, No date, Completed.
    #[default]
    Due,
    /// One group per task list, by name.
    List,
    /// Urgent, High, Medium, Low, No priority.
    Priority,
    /// One group per GitHub repository (`owner/name`), by name; tasks without a GitHub link last.
    /// What a filter list starts with (task G.3).
    Repo,
    /// One run of rows without heads.
    None,
}

/// A group of the list. Open tasks fall into the due groups by their due day, done ones (shown
/// with "Show completed") into "Completed"; the other ways of grouping keep done tasks in their
/// list or priority, after the open ones.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Group {
    /// Open, due before today.
    Overdue,
    /// Open, due today.
    Today,
    /// Open, due after today.
    Upcoming,
    /// Open, without a due date.
    NoDate,
    /// Done.
    Completed,
    /// The tasks of one list ([`Collection::id`](torro_pim::Collection)).
    List(String),
    /// The tasks of one priority.
    Priority(Prio),
    /// The GitHub items of one repository (`owner/name`); empty for tasks without a GitHub link.
    Repo(String),
    /// Every row, without a head ([`GroupBy::None`]).
    All,
}

impl Group {
    /// The group of a top-level `todo` when grouping `by`.
    fn of(todo: &Todo, by: GroupBy, today: NaiveDate) -> Group {
        match by {
            GroupBy::Due => Group::by_due(todo, today),
            GroupBy::List => Group::List(todo.collection.clone()),
            GroupBy::Priority => Group::Priority(Prio::of(todo.priority)),
            GroupBy::Repo => {
                let repo = todo.link.as_ref().and_then(Link::github_ref).map(|(repo, _)| repo.to_string());
                Group::Repo(repo.unwrap_or_default())
            }
            GroupBy::None => Group::All,
        }
    }

    fn by_due(todo: &Todo, today: NaiveDate) -> Group {
        if todo.completed {
            return Group::Completed;
        }
        match todo.due.map(|due| due_date(due).cmp(&today)) {
            Some(Ordering::Less) => Group::Overdue,
            Some(Ordering::Equal) => Group::Today,
            Some(Ordering::Greater) => Group::Upcoming,
            None => Group::NoDate,
        }
    }

    /// Where the group stands: due groups in time order, lists by name, priorities from urgent.
    fn sort_key(&self, names: &HashMap<String, String>) -> (u8, String) {
        match self {
            Group::Overdue => (0, String::new()),
            Group::Today => (1, String::new()),
            Group::Upcoming => (2, String::new()),
            Group::NoDate => (3, String::new()),
            Group::Completed => (4, String::new()),
            Group::List(id) => (5, names.get(id).map_or_else(|| id.clone(), |name| name.to_lowercase())),
            Group::Priority(prio) => (6 + *prio as u8, String::new()),
            // After the five priorities; tasks without a repository last.
            Group::Repo(repo) if repo.is_empty() => (12, String::new()),
            Group::Repo(repo) => (11, repo.to_lowercase()),
            Group::All => (u8::MAX, String::new()),
        }
    }

    /// The name in the head.
    fn label(&self, names: &HashMap<String, String>) -> String {
        match self {
            Group::Overdue => tr("Overdue").into(),
            Group::Today => tr("Today").into(),
            Group::Upcoming => tr("Upcoming").into(),
            Group::NoDate => tr("No date").into(),
            Group::Completed => tr("Completed").into(),
            Group::List(id) => names.get(id).cloned().unwrap_or_else(|| tr("List").into()),
            Group::Priority(prio) => prio.label().into(),
            Group::Repo(repo) if repo.is_empty() => tr("No repository").into(),
            Group::Repo(repo) => repo.clone(),
            Group::All => String::new(),
        }
    }

    /// Whether the head offers "+": not for done tasks or a repository (a task cannot be added to
    /// GitHub here), and the headless run has no head.
    fn can_add(&self) -> bool {
        !matches!(self, Group::Completed | Group::Repo(_) | Group::All)
    }
}

/// [`arrange`]'s tasks split into their [`Group`]s for [`ListData::group_by`], in display order
/// and without empty groups. A top-level task decides the group; its subtasks stay with it, so a
/// tree is never split. Within a group the rows keep [`arrange`]'s order. A list in manual order
/// has one group without a head ([`Group::All`]).
pub fn grouped(data: &ListData<'_>) -> Vec<(Group, Vec<(usize, usize)>)> {
    let by = if data.manual { GroupBy::None } else { data.group_by };
    let mut groups: Vec<(Group, Vec<(usize, usize)>)> = Vec::new();
    let mut current = 0;
    for (index, depth) in arrange(data) {
        if depth == 0 {
            let group = Group::of(&data.todos[index], by, data.today);
            current = match groups.iter().position(|(known, _)| *known == group) {
                Some(position) => position,
                None => {
                    groups.push((group, Vec::new()));
                    groups.len() - 1
                }
            };
        }
        if let Some((_, items)) = groups.get_mut(current) {
            items.push((index, depth));
        }
    }
    // A stable sort: the rows of each group keep their order.
    groups.sort_by_cached_key(|(group, _)| group.sort_key(&data.names));
    groups
}

/// How many open tasks `filter` shows: the counter of a sidebar row.
pub fn open_count(todos: &[Todo], filter: &Filter, today: NaiveDate) -> usize {
    todos.iter().filter(|todo| is_shown(todo, filter, false, today)).count()
}

/// The id of the list's scrollable, so the keyboard can scroll the chosen row into view.
pub const SCROLL: &str = "task-list";

/// The one-pixel line between two group heads that touch (a folded group above).
const LINE: f32 = size::HAIRLINE;

/// Indices into `todos` of the rows as the list shows them, top to bottom; rows of folded
/// groups are left out.
pub fn order(data: &ListData<'_>) -> Vec<usize> {
    grouped(data)
        .into_iter()
        .filter(|(group, _)| !data.collapsed.contains(group))
        .flat_map(|(_, items)| items.into_iter().map(|(index, _)| index))
        .collect()
}

/// Where `todos[index]` lies in the list's content as `(top, bottom)`. The first row of a group
/// starts at its head, so scrolling up to it shows the head too. `None` if the row is not shown.
pub fn row_span(data: &ListData<'_>, index: usize) -> Option<(f32, f32)> {
    let mut y = 0.0;
    let mut after_head = false;
    for (group, items) in grouped(data) {
        let head = y;
        if group != Group::All {
            y += if after_head { LINE } else { 0.0 } + size::ROW_GROUP;
            after_head = true;
        }
        if data.collapsed.contains(&group) {
            continue;
        }
        for (n, (row, _)) in items.into_iter().enumerate() {
            if row == index {
                return Some((if n == 0 { head } else { y }, y + size::ROW));
            }
            y += size::ROW;
            after_head = false;
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Manual order
// ---------------------------------------------------------------------------------------------

/// Where `todos[index]` may go in a list in manual order, top to bottom, as `(place, before)`: the
/// marker stands above the row at `place` in [`order`] (`order.len()`: under the last row), and the
/// task goes in front of `todos[before]`, or to the end of its level with `None`. Only the tasks it
/// is ordered with are places ([`same_level`]: same list, same parent, both open or both done), so a
/// subtask stays under its parent and a task takes its subtasks along. The task's own place is
/// among them (dropping it there changes nothing). Empty when the task is not shown.
pub fn drop_places(data: &ListData<'_>, index: usize) -> Vec<(usize, Option<usize>)> {
    let rows = arrange(data);
    let Some(todo) = data.todos.get(index).filter(|_| rows.iter().any(|&(i, _)| i == index)) else {
        return Vec::new();
    };
    let mut places = Vec::new();
    let mut end = None;
    for (place, &(i, depth)) in rows.iter().enumerate() {
        if same_level(&data.todos[i], todo) {
            places.push((place, Some(i)));
            // The end of the level lies behind the subtasks of its last task.
            let subtasks = rows[place + 1..].iter().take_while(|&&(_, d)| d > depth).count();
            end = Some(place + 1 + subtasks);
        }
    }
    places.extend(end.map(|end| (end, None)));
    places
}

/// The place of `places` ([`drop_places`]) whose marker is nearest to `y`, the pointer's height in
/// the list's content (rows are [`size::ROW`] high; a list in manual order has no heads).
pub fn nearest_place(places: &[(usize, Option<usize>)], y: f32) -> Option<(usize, Option<usize>)> {
    let distance = |place: usize| (place as f32 * size::ROW - y).abs();
    places.iter().copied().min_by(|a, b| distance(a.0).total_cmp(&distance(b.0)))
}

/// Where Alt+↑ (`up`) or Alt+↓ takes `todos[index]` in a list in manual order: one step among the
/// tasks it is ordered with ([`drop_places`]). `Some(Some(i))` in front of `todos[i]`, `Some(None)`
/// to the end of its level, `None` when it is the first (↑) or last (↓) there already or not shown.
pub fn step_target(data: &ListData<'_>, index: usize, up: bool) -> Option<Option<usize>> {
    let places = drop_places(data, index);
    let at = places.iter().position(|&(_, before)| before == Some(index))?;
    if up {
        return at.checked_sub(1).map(|previous| places[previous].1);
    }
    // The place after the next task: in front of the one behind it, or the end. The last task
    // has only the end behind it.
    places.get(at + 1)?.1?;
    places.get(at + 2).map(|&(_, before)| before)
}

// ---------------------------------------------------------------------------------------------
// Shortening by width
// ---------------------------------------------------------------------------------------------

/// Below this list width the GitHub labels go (the mock-up's container queries).
const LABELS_FROM: f32 = 700.0;
/// Below this the hints (sub-tasks, notes) go.
const HINTS_FROM: f32 = 600.0;
/// Below this the pill of the list or repository becomes a dot.
const PILL_FROM: f32 = 560.0;
/// Below this `TorroWorkplace#123` becomes `#123`.
const FULL_REF_FROM: f32 = 520.0;

/// Which parts a row keeps in a list of a given width. They go in a fixed order as the list gets
/// narrower – labels, hints, the pill (it becomes a dot), the repository in front of `#123` –, so
/// the title keeps its room and a row never wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowParts {
    /// The GitHub labels (two and "+n").
    pub labels: bool,
    /// Sub-tasks "2/4" and the notes icon after the title.
    pub hints: bool,
    /// The list or repository as a pill; else only its dot.
    pub pill: bool,
    /// `TorroWorkplace#123`; else `#123`.
    pub full_ref: bool,
}

impl RowParts {
    /// The parts that fit a list `width` logical pixels wide.
    pub fn for_width(width: f32) -> RowParts {
        RowParts {
            labels: width >= LABELS_FROM,
            hints: width >= HINTS_FROM,
            pill: width >= PILL_FROM,
            full_ref: width >= FULL_REF_FROM,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------------------------

/// The list: group heads with their rows, scrolling; the empty state when nothing is shown.
pub fn view<'a>(data: ListData<'a>) -> Element<'a, Message> {
    let groups = grouped(&data);
    if groups.is_empty() && data.read_only {
        return widgets::empty_state(tr("No matches"), tr("Nothing on GitHub matches this list right now"));
    }
    if groups.is_empty() {
        return widgets::empty_state(tr("Nothing to do"), tr("Add a task above"));
    }
    let parts = RowParts::for_width(data.width);
    let subtasks = subtask_counts(data.todos);
    let held = data.drag.map(|(index, _)| index);
    let mut list = column![].width(Fill);
    let mut after_head = false;
    // Rows and the depth of the dragged one, for the marker of a list in manual order.
    let (mut rows, mut held_depth) = (0, 0);
    for (group, items) in groups {
        let folded = data.collapsed.contains(&group);
        if group != Group::All {
            if after_head {
                list = list.push(rule::horizontal(LINE).style(style::rule));
            }
            let open = items.iter().filter(|(i, _)| !data.todos[*i].completed).count();
            let count = if group == Group::Completed { items.len() } else { open };
            list = list.push(group_head(&group, &data, count, folded));
            after_head = true;
        }
        if folded {
            continue;
        }
        let under_today = group == Group::Today;
        for (index, depth) in items {
            let line = row_view(&data, index, depth, parts, &subtasks, under_today);
            list = list.push(if data.manual { with_grip(line, index, held == Some(index)) } else { line });
            if held == Some(index) {
                held_depth = depth;
            }
            rows += 1;
            after_head = false;
        }
    }
    let content = if data.manual { manual_content(list, &data, rows, held_depth) } else { list.into() };
    scrollable(content)
        .id(SCROLL)
        .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y, viewport.bounds().height))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Fill)
        .height(Fill)
        .into()
}

/// A row of a list in manual order with its grip: in the 16 left of the leading symbol, shown while
/// the pointer is over the row (and always on the row being dragged, `held`). Pressing it starts a
/// drag ([`Message::DragStart`]); the press stays with the grip, so the row does not open.
fn with_grip<'a>(line: Element<'a, Message>, index: usize, held: bool) -> Element<'a, Message> {
    let mark =
        container(toned(Icon::GripVertical, icon::SIZE_SM, Tone::Text3)).center_x(space::L).center_y(size::ROW - LINE);
    let grip = mouse_area(widgets::with_tooltip(mark, tr("Drag to reorder")))
        .on_press(Message::DragStart(index))
        .interaction(mouse::Interaction::Grab);
    if held { stack![line, grip].into() } else { hover(line, grip) }
}

/// The rows of a list in manual order, with the marker over them where a dragged task would go.
/// The pointer's height is reported ([`Message::DragMove`]) only while a row is dragged, so merely
/// moving the pointer over the list costs nothing. The same widgets with and without a drag, so
/// starting one keeps the rows' state.
fn manual_content<'a>(
    rows: Column<'a, Message>,
    data: &ListData<'a>,
    count: usize,
    depth: usize,
) -> Element<'a, Message> {
    let marker = match data.drag {
        Some((_, Some(place))) => drop_marker(place, count, depth),
        _ => Space::new().into(),
    };
    let area = mouse_area(stack![rows, marker].width(Fill));
    match data.drag {
        Some(_) => area.on_move(|point| Message::DragMove(point.y)).interaction(mouse::Interaction::Grabbing).into(),
        None => area.into(),
    }
}

/// How thick the drop marker is.
const MARKER: f32 = 2.0 * size::HAIRLINE;

/// The drop marker: a 2-px line in `text_2` on the line above the row at `place` (under the last
/// of `rows` at the end), from the indent of the dragged task's level (`depth`) to the right edge.
fn drop_marker<'a>(place: usize, rows: usize, depth: usize) -> Element<'a, Message> {
    let bottom = rows as f32 * size::ROW;
    let top = (place as f32 * size::ROW - MARKER / 2.0).clamp(0.0, (bottom - MARKER).max(0.0));
    let line = container(Space::new().width(Fill).height(MARKER)).style(|t: &Theme| container::Style {
        background: Some(Background::Color(palette(t).text_2)),
        border: border::rounded(MARKER / 2.0),
        ..container::Style::default()
    });
    let indent = space::L + depth as f32 * space::XL;
    column![Space::new().height(top), container(line).padding(Padding::new(0.0).left(indent).right(space::L))]
        .width(Fill)
        .into()
}

/// Edge of the chevron that folds a group (`.ghead .chev` 12).
const CHEVRON: f32 = space::M;

/// A group head: 32 high on `raised`, flush with the rows. The chevron sits in the 16 left of the
/// leading axis, the group's icon on that axis (16 in from the edge, like the rows' checks), the
/// name 12 after it. A click anywhere folds or unfolds the group; "+" adds a task to it.
fn group_head<'a>(group: &Group, data: &ListData<'a>, count: usize, folded: bool) -> Element<'a, Message> {
    let chevron = if folded { Icon::ChevronRight } else { Icon::ChevronDown };
    let mut line = row![
        container(toned(chevron, CHEVRON, Tone::Text3)).center_x(space::L),
        container(group_mark(group, data)).center_x(icon::SIZE),
        Space::new().width(space::M),
        typo::text(typo::CAPTION, group.label(&data.names)).style(theme::text_2).wrapping(text::Wrapping::None),
        Space::new().width(space::S),
        typo::text(typo::META.tabular(), count.to_string()).style(theme::text_3),
        Space::new().width(Fill),
    ]
    .align_y(Alignment::Center);
    if group.can_add() && !data.read_only {
        let add = button(center(toned(Icon::Plus, icon::SIZE_SM, Tone::Text3)))
            .width(size::CONTROL_SM)
            .height(size::CONTROL_SM)
            .padding(0)
            .style(style::button::icon)
            .on_press(Message::AddInGroup(group.clone()));
        line = line.push(add);
    }
    // The "+" button is wider than its icon; pull it in so the icon ends 16 from the edge, above
    // the due dates.
    let right = space::L - (size::CONTROL_SM - icon::SIZE_SM) / 2.0;
    let head = container(line).width(Fill).center_y(size::ROW_GROUP).padding(Padding::new(0.0).right(right)).style(
        |t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).raised)),
            ..container::Style::default()
        },
    );
    mouse_area(head).on_press(Message::ToggleGroup(group.clone())).interaction(mouse::Interaction::Pointer).into()
}

/// The icon of a group head (14): the attention colour for "Overdue", `text_3` for the other due
/// groups, the ring of a list, the priority glyph.
fn group_mark<'a>(group: &Group, data: &ListData<'a>) -> Element<'a, Message> {
    match group {
        Group::Overdue => toned(Icon::CircleAlert, icon::SIZE_SM, Tone::Attention),
        Group::Today => toned(Icon::Sun, icon::SIZE_SM, Tone::Text3),
        Group::Upcoming => toned(Icon::CalendarClock, icon::SIZE_SM, Tone::Text3),
        Group::NoDate => toned(Icon::Layers, icon::SIZE_SM, Tone::Text3),
        Group::Completed => toned(Icon::CircleCheck, icon::SIZE_SM, Tone::Text3),
        Group::List(id) => ring(colour_of(data, id)),
        Group::Priority(Prio::None) => toned(Icon::SignalHigh, icon::SIZE_SM, Tone::Text3),
        Group::Priority(prio) => prio_glyph(*prio),
        Group::Repo(_) => toned(Icon::FolderGit2, icon::SIZE_SM, Tone::Text3),
        Group::All => Space::new().into(),
    }
}

/// Edge of a list ring in a group head (as in chips, 12) and its line.
const RING: f32 = icon::SIZE_SM - 2.0 * RING_LINE;
// design-check: allow the ring's line is the sidebar ring's 2 px
const RING_LINE: f32 = 2.0;

/// A ring in a list's colour.
fn ring<'a>(colour: Color) -> Element<'a, Message> {
    container(Space::new().width(RING).height(RING))
        .style(move |_| container::Style {
            border: border::rounded(RING / 2.0).color(colour).width(RING_LINE),
            ..container::Style::default()
        })
        .into()
}

/// The colour of a collection, or the standard one.
fn colour_of(data: &ListData<'_>, collection: &str) -> Color {
    data.colors.get(collection).copied().unwrap_or_else(|| widgets::collection_color(None))
}

/// `done` of `total` subtasks per parent uid, counted over every loaded task (filtered out or not).
fn subtask_counts(todos: &[Todo]) -> HashMap<&str, (usize, usize)> {
    let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
    for todo in todos {
        if let Some(parent) = todo.parent.as_deref() {
            let entry = counts.entry(parent).or_default();
            entry.0 += usize::from(todo.completed);
            entry.1 += 1;
        }
    }
    counts
}

/// The small signs after a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hint {
    /// `done` of `total` sub-tasks.
    Subtasks(usize, usize),
    /// The task has notes.
    Notes,
}

impl Hint {
    fn text(self) -> Option<String> {
        match self {
            Hint::Subtasks(done, total) => Some(format!("{done}/{total}")),
            Hint::Notes => None,
        }
    }

    /// The width the hint takes: its icon and, after 4, its count.
    fn width(self) -> f32 {
        icon::SIZE_SM + self.text().map_or(0.0, |text| space::XS + text_width(&text, typo::META.tabular()))
    }

    fn view<'a>(self) -> Element<'a, Message> {
        let which = match self {
            Hint::Subtasks(..) => Icon::ListChecks,
            Hint::Notes => Icon::AlignLeft,
        };
        let mut hint = row![toned(which, icon::SIZE_SM, Tone::Text3)].spacing(space::XS).align_y(Alignment::Center);
        if let Some(text) = self.text() {
            hint =
                hint.push(typo::text(typo::META.tabular(), text).style(theme::text_3).wrapping(text::Wrapping::None));
        }
        hint.into()
    }
}

/// The hints of `todo`: its sub-tasks, then whether it has notes.
fn hints_of(todo: &Todo, subtasks: &HashMap<&str, (usize, usize)>) -> Vec<Hint> {
    let mut hints = Vec::new();
    if let Some(&(done, total)) = subtasks.get(todo.uid.as_str()) {
        hints.push(Hint::Subtasks(done, total));
    }
    if todo.description.as_deref().is_some_and(|notes| !notes.trim().is_empty()) {
        hints.push(Hint::Notes);
    }
    hints
}

/// One task: indent, leading symbol, priority, `repo#123`, title with hints, and at the right
/// labels, checks, review, where it comes from and the due date.
fn row_view<'a>(
    data: &ListData<'a>,
    index: usize,
    depth: usize,
    parts: RowParts,
    subtasks: &HashMap<&str, (usize, usize)>,
    under_today: bool,
) -> Element<'a, Message> {
    let todo = &data.todos[index];
    let github = github_of(todo);
    let mut line = row![].align_y(Alignment::Center);
    if depth > 0 {
        line = line.push(Space::new().width(depth as f32 * space::XL));
    }
    // An issue of a filter list has its check (task G.5); a link in a task list, issue or pull
    // request, is a task of its own and always has one (task G.6).
    let issue = match data.issue_lists.get(&todo.collection) {
        Some(may_close) => github.filter(|details| details.kind == GitHubKind::Issue).map(|_| *may_close),
        None => github.map(|_| true),
    };
    line = line
        .push(lead(todo, index, colour_of(data, &todo.collection), issue))
        .push(Space::new().width(space::M))
        .push(container(prio_cell(todo.priority)).width(icon::SIZE_SM))
        .push(Space::new().width(space::S));
    if let Some((repo, number)) = todo.link.as_ref().and_then(Link::github_ref) {
        let name = repo.rsplit('/').next().unwrap_or(repo);
        let reference = if parts.full_ref { format!("{name}#{number}") } else { format!("#{number}") };
        line = line
            .push(typo::text(typo::META.tabular(), reference).style(theme::text_3).wrapping(text::Wrapping::None))
            .push(Space::new().width(space::S));
    }
    let hints = if parts.hints { hints_of(todo, subtasks) } else { Vec::new() };
    line = line.push(title_cell(todo.summary.clone(), todo.completed, hints)).push(Space::new().width(space::M));
    line = line.push(right_part(data, todo, github, parts, under_today));
    widgets::list_row(vec![line.width(Fill).into()], data.selected == Some(index), Some(Message::Select(index)))
}

/// The title on one line, cut with "…" to the room the row leaves, and the hints right after it.
/// The cell takes the free width; it measures itself, so the cut fits whatever the right side
/// holds.
fn title_cell<'a>(summary: String, done: bool, hints: Vec<Hint>) -> Element<'a, Message> {
    responsive(move |room| {
        let hints_width = if hints.is_empty() {
            0.0
        } else {
            space::M + hints.iter().map(|hint| hint.width()).sum::<f32>() + space::S * (hints.len() - 1) as f32
        };
        let title_room = (room.width - hints_width).max(0.0);
        let title = row_title(ellipsize(&summary, typo::BODY_STRONG, title_room), done);
        let mut cell = row![container(title).max_width(title_room).clip(true)].align_y(Alignment::Center);
        if !hints.is_empty() {
            let signs = row(hints.iter().map(|hint| hint.view())).spacing(space::S).align_y(Alignment::Center);
            cell = cell.push(Space::new().width(space::M)).push(signs);
        }
        cell.into()
    })
    .height(Length::Shrink)
    .into()
}

/// A row's title in `BODY_STRONG`; a done one struck through in `text_3`.
fn row_title<'a>(title: String, done: bool) -> Element<'a, Message> {
    let fragment: text::Span<'static, ()> = span(title).strikethrough(done);
    rich_text(vec![fragment])
        .size(typo::BODY_STRONG.size)
        .line_height(typo::BODY_STRONG.line_height)
        .font(typo::BODY_STRONG.font)
        .wrapping(text::Wrapping::None)
        .style(move |t: &Theme| {
            let p = palette(t);
            text::Style { color: Some(if done { p.text_3 } else { p.text }) }
        })
        .into()
}

/// The right side of a row, 8 apart: labels, checks, review, origin, due date.
fn right_part<'a>(
    data: &ListData<'a>,
    todo: &Todo,
    github: Option<&GitHubDetails>,
    parts: RowParts,
    under_today: bool,
) -> Element<'a, Message> {
    let mut right = row![].spacing(space::S).align_y(Alignment::Center);
    if let Some(details) = github {
        if parts.labels && !details.labels.is_empty() {
            right = right.push(labels(details));
        }
        if let Some(checks) = details.checks {
            let (which, tone) = checks_mark(checks);
            right = right.push(toned(which, icon::SIZE_SM, tone));
        }
        if details.review == Some(ReviewState::Required) {
            right = right.push(toned(Icon::Eye, icon::SIZE_SM, Tone::Attention));
        }
    }
    if data.show_list {
        right = right.push(origin(data, todo, github.is_some(), parts.pill));
    }
    if let Some(due) = todo.due {
        let label = row_due(due, data.today, under_today);
        if !label.is_empty() {
            let overdue = is_overdue(todo, data.today);
            let width = text_width(&label, typo::META.tabular()).max(DUE_MIN);
            let date =
                typo::text(typo::META.tabular(), label).wrapping(text::Wrapping::None).style(move |t: &Theme| {
                    let p = palette(t);
                    text::Style { color: Some(if overdue { p.attention_text } else { p.text_3 }) }
                });
            right = right.push(container(date).width(width).align_right(width));
        }
    }
    right.into()
}

/// The least width of the due date, so dates line up at the right (`.due{min-width:44px}`).
const DUE_MIN: f32 = size::HEADER;
/// Edge of the dot that stands for a list or repository in narrow lists (`.pdot` 6).
// design-check: allow the dot of a pill is 6, as in widgets::pill
const ORIGIN_DOT: f32 = 6.0;
/// How many labels a row shows before "+n".
const LABELS_SHOWN: usize = 2;

/// The GitHub labels of a row: two chips, then "+n" for the rest.
fn labels<'a>(details: &GitHubDetails) -> Element<'a, Message> {
    let mut chips = row![].spacing(space::XS).align_y(Alignment::Center);
    for label in details.labels.iter().take(LABELS_SHOWN) {
        chips = chips.push(widgets::label_chip(label.color.as_deref().unwrap_or_default(), label.name.clone()));
    }
    let more = details.labels.len().saturating_sub(LABELS_SHOWN);
    if more > 0 {
        chips = chips.push(typo::text(typo::CAPTION.tabular(), format!("+{more}")).style(theme::text_3));
    }
    chips.into()
}

/// Where a row comes from: its list (dot in the list's colour and name) or, for a GitHub item,
/// its repository after the GitHub mark; a dot alone when the list is narrow.
fn origin<'a>(data: &ListData<'a>, todo: &Todo, github: bool, pill: bool) -> Element<'a, Message> {
    if let Some((repo, _)) = todo.link.as_ref().and_then(Link::github_ref).filter(|_| github) {
        let name = repo.rsplit('/').next().unwrap_or(repo).to_string();
        let colour = data.palette.text_3;
        return if pill { widgets::icon_pill(Icon::Github, name) } else { widgets::color_dot(colour, ORIGIN_DOT) };
    }
    let colour = colour_of(data, &todo.collection);
    let name = data.names.get(&todo.collection).cloned().unwrap_or_default();
    if pill { widgets::pill(Some(colour), name) } else { widgets::color_dot(colour, ORIGIN_DOT) }
}

/// What GitHub reports about a task's link, if anything.
pub(crate) fn github_of(todo: &Todo) -> Option<&GitHubDetails> {
    todo.link.as_ref()?.github.as_ref()
}

/// The leading symbol of a row: for an issue of a filter list (`issue`: whether ticking may close
/// it) its [`issue_check`], for another GitHub item its type symbol, else the round check in the
/// list's colour.
fn lead<'a>(todo: &Todo, index: usize, colour: Color, issue: Option<bool>) -> Element<'a, Message> {
    match (github_of(todo), issue) {
        (Some(_), Some(may_close)) => {
            issue_check(todo.completed, style::CHECK_SIZE, may_close.then_some(Message::ToggleDone(index)))
        }
        (Some(details), None) => {
            let (which, tone) = github_mark(details);
            container(toned(which, icon::SIZE, tone)).center(icon::SIZE).into()
        }
        (None, _) => checkbox(todo.completed)
            .size(style::CHECK_SIZE)
            .style(style::task_check(colour))
            .on_toggle(move |_| Message::ToggleDone(index))
            .into(),
    }
}

/// The check of a GitHub issue in a filter list (task G.5), `edge` wide: open a ring in the colour
/// of an open issue (`ok`), closed filled in `merged` with the check mark – the colours of
/// GitHub's own issue symbols ([`github_mark`]). `on_toggle` closes or reopens it; `None` locks
/// it (40 %, the account's token may not write issues) with a tooltip that says so.
pub(crate) fn issue_check<'a, M: Clone + 'a>(done: bool, edge: f32, on_toggle: Option<M>) -> Element<'a, M> {
    let check = checkbox(done).size(edge).style(issue_check_style);
    match on_toggle {
        Some(message) => check.on_toggle(move |_| message.clone()).into(),
        None => widgets::with_tooltip(check, tr("This GitHub token can't close issues")),
    }
}

/// [`style::task_check`] in the issue's colours; an open issue's ring in `ok` instead of the
/// neutral ring of a task, so it reads as GitHub's open-issue circle.
fn issue_check_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let p = palette(theme);
    let (done, locked) = match status {
        checkbox::Status::Active { is_checked } | checkbox::Status::Hovered { is_checked } => (is_checked, false),
        checkbox::Status::Disabled { is_checked } => (is_checked, true),
    };
    let colour = if done { p.merged } else { p.ok };
    let style = style::task_check(colour)(theme, status);
    if done || locked {
        return style;
    }
    checkbox::Style { border: style.border.color(colour), ..style }
}

/// The symbol and colour of a GitHub item's type and state: open issue `CircleDot` (ok), closed
/// issue `CircleCheck` (merged), open pull request `GitPullRequest` (ok), draft
/// `GitPullRequestDraft` (`text_3`), merged `GitMerge` (merged), closed `GitPullRequestClosed`
/// (error) – the design skill's table.
pub(crate) fn github_mark(details: &GitHubDetails) -> (Icon, Tone) {
    match (details.kind, details.state) {
        (GitHubKind::Issue, GitHubState::Open) => (Icon::CircleDot, Tone::Ok),
        (GitHubKind::Issue, _) => (Icon::CircleCheck, Tone::Merged),
        (GitHubKind::PullRequest, GitHubState::Open) if details.draft => (Icon::GitPullRequestDraft, Tone::Text3),
        (GitHubKind::PullRequest, GitHubState::Open) => (Icon::GitPullRequest, Tone::Ok),
        (GitHubKind::PullRequest, GitHubState::Merged) => (Icon::GitMerge, Tone::Merged),
        (GitHubKind::PullRequest, GitHubState::Closed) => (Icon::GitPullRequestClosed, Tone::Error),
    }
}

/// The symbol of a pull request's checks: running `CircleDotDashed` (attention), passed
/// `CircleCheck` (ok), failed `CircleX` (error).
fn checks_mark(checks: ChecksState) -> (Icon, Tone) {
    match checks {
        ChecksState::Pending => (Icon::CircleDotDashed, Tone::Attention),
        ChecksState::Success => (Icon::CircleCheck, Tone::Ok),
        ChecksState::Failure => (Icon::CircleX, Tone::Error),
    }
}

/// The colour of a symbol, picked from the palette of the current mode when it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Text3,
    Ok,
    Attention,
    Error,
    Merged,
}

impl Tone {
    fn of(self, p: &Palette) -> Color {
        match self {
            Tone::Text3 => p.text_3,
            Tone::Ok => p.ok,
            // Small icons on light surfaces need the darker attention colour.
            Tone::Attention => p.attention_text,
            Tone::Error => p.error,
            Tone::Merged => p.merged,
        }
    }
}

/// `which` at `edge` in the colour of `tone`.
pub(crate) fn toned<'a, M: 'a>(which: Icon, edge: f32, tone: Tone) -> Element<'a, M> {
    icon::icon(which, edge).style(move |t, _| svg::Style { color: Some(tone.of(palette(t))) }).into()
}

/// The priority column of a row: empty without a priority, the urgent square, or the bars.
fn prio_cell<'a>(priority: Option<u8>) -> Element<'a, Message> {
    match Prio::of(priority) {
        Prio::None => Space::new().into(),
        prio => prio_glyph(prio),
    }
}

/// The glyph of a priority, 14 wide: urgent as an orange square with "!", the others as three
/// bars (none: all dim).
pub(crate) fn prio_glyph<'a, M: 'a>(prio: Prio) -> Element<'a, M> {
    match prio {
        Prio::Urgent => container(typo::text(typo::CAPTION, "!").style(|t: &Theme| text::Style {
            // The panel colour is dark on the bright orange of dark mode and white on the darker
            // orange of light mode: readable in both.
            color: Some(palette(t).panel),
        }))
        .center(icon::SIZE_SM)
        .style(|t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).attention_text)),
            border: border::rounded(radius::XS),
            ..container::Style::default()
        })
        .into(),
        other => priority_view(other.level()),
    }
}

/// The title on one line; a done task struck through in `text_3`.
pub(crate) fn title_view<'a, M: 'a>(todo: &Todo) -> Element<'a, M> {
    let fragment: text::Span<'static, ()> = span(todo.summary.clone()).strikethrough(todo.completed);
    let completed = todo.completed;
    rich_text(vec![fragment])
        .size(typo::BODY.size)
        .line_height(typo::BODY.line_height)
        .font(typo::BODY.font)
        .wrapping(text::Wrapping::None)
        .style(move |t: &Theme| {
            let p = palette(t);
            text::Style { color: Some(if completed { p.text_3 } else { p.text }) }
        })
        .into()
}

/// Width of one priority bar and the gap between the bars (the mock-up's `.prio`).
// design-check: allow the priority glyph is drawn from boxes, its sizes are the mock-up's
const BAR_WIDTH: f32 = 3.0;
const BAR_GAP: f32 = 2.0;
/// Heights of the three bars, rising; the tallest is the height of the glyph.
const BAR_HEIGHTS: [f32; 3] = [5.0, 8.0, 12.0];
const BAR_RADIUS: f32 = 1.0;

/// Linear's priority glyph: three rising bars in a 14-wide cell, the first `level` in `text_2`,
/// the rest in `line_strong`. Shape, not colour, carries the level.
pub(crate) fn priority_view<'a, M: 'a>(level: usize) -> Element<'a, M> {
    let bars = BAR_HEIGHTS.into_iter().enumerate().map(|(i, height)| {
        let lit = i < level;
        container(Space::new().width(BAR_WIDTH).height(height))
            .style(move |t: &Theme| {
                let p = palette(t);
                container::Style {
                    background: Some(Background::Color(if lit { p.text_2 } else { p.line_strong })),
                    border: border::rounded(BAR_RADIUS),
                    ..container::Style::default()
                }
            })
            .into()
    });
    let glyph = BAR_HEIGHTS[2];
    container(row(bars).spacing(BAR_GAP).align_y(Alignment::End))
        .width(icon::SIZE_SM)
        .height(glyph)
        .align_bottom(glyph)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_core::i18n::trf;
    use torro_pim::model::GitHubLabel;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).expect("valid date")
    }

    fn today() -> NaiveDate {
        day(11)
    }

    fn todo(uid: &str, summary: &str) -> Todo {
        Todo {
            uid: uid.into(),
            collection: "work".into(),
            href: None,
            etag: None,
            summary: summary.into(),
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

    fn due(d: u32) -> Option<EventTime> {
        Some(EventTime::Date(day(d)))
    }

    fn no_folds() -> &'static HashSet<Group> {
        static EMPTY: std::sync::OnceLock<HashSet<Group>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(HashSet::new)
    }

    fn data<'a>(todos: &'a [Todo], filter: &'a Filter, show_completed: bool) -> ListData<'a> {
        ListData {
            todos,
            colors: HashMap::new(),
            names: HashMap::new(),
            filter,
            show_completed,
            today: today(),
            selected: None,
            show_list: true,
            group_by: GroupBy::Due,
            collapsed: no_folds(),
            width: size::CONTENT_MAX,
            palette: palette(&Theme::Dark),
            read_only: false,
            issue_lists: HashMap::new(),
            manual: false,
            drag: None,
        }
    }

    /// Summaries of the arranged tasks with their depth.
    fn shown<'a>(todos: &'a [Todo], filter: &'a Filter, show_completed: bool) -> Vec<(&'a str, usize)> {
        arrange(&data(todos, filter, show_completed))
            .into_iter()
            .map(|(i, depth)| (todos[i].summary.as_str(), depth))
            .collect()
    }

    fn names(todos: &[Todo], filter: &Filter, show_completed: bool) -> Vec<String> {
        shown(todos, filter, show_completed).into_iter().map(|(name, _)| name.to_string()).collect()
    }

    #[test]
    fn filters_pick_tasks() {
        let mut done = todo("4", "done today");
        done.due = due(11);
        done.completed = true;
        let todos = vec![
            Todo { due: due(9), ..todo("1", "overdue") },
            Todo { due: due(11), ..todo("2", "today") },
            Todo { due: due(12), ..todo("3", "later") },
            todo("5", "undated"),
            done,
        ];
        assert_eq!(names(&todos, &Filter::Today, false), ["overdue", "today"]);
        assert_eq!(names(&todos, &Filter::Today, true), ["overdue", "today", "done today"]);
        assert_eq!(names(&todos, &Filter::Planned, false), ["overdue", "today", "later"]);
        assert_eq!(names(&todos, &Filter::All, false), ["overdue", "today", "later", "undated"]);
        assert_eq!(names(&todos, &Filter::All, true), ["overdue", "today", "later", "undated", "done today"]);
    }

    #[test]
    fn list_filter_keeps_one_collection() {
        let todos = vec![Todo { collection: "home".into(), ..todo("1", "bread") }, todo("2", "report")];
        assert_eq!(names(&todos, &Filter::List("home".into()), false), ["bread"]);
        assert_eq!(names(&todos, &Filter::List("work".into()), false), ["report"]);
    }

    #[test]
    fn open_tasks_come_before_done_ones() {
        let mut done = todo("1", "done");
        done.completed = true;
        done.due = due(9);
        let todos = vec![done, Todo { due: due(20), ..todo("2", "open") }];
        assert_eq!(names(&todos, &Filter::All, true), ["open", "done"]);
    }

    #[test]
    fn orders_by_due_priority_then_title() {
        let todos = vec![
            Todo { due: due(13), ..todo("1", "late") },
            Todo { due: due(12), ..todo("2", "early") },
            todo("3", "no due"),
        ];
        assert_eq!(names(&todos, &Filter::All, false), ["early", "late", "no due"]);

        let prio = |uid: &str, priority: Option<u8>| Todo { due: due(12), priority, ..todo(uid, uid) };
        let todos = vec![prio("none", None), prio("low", Some(9)), prio("high", Some(1)), prio("mid", Some(5))];
        assert_eq!(names(&todos, &Filter::All, false), ["high", "mid", "low", "none"]);

        let todos = vec![todo("1", "Zeta"), todo("2", "alpha"), todo("3", "Beta")];
        assert_eq!(names(&todos, &Filter::All, false), ["alpha", "Beta", "Zeta"]);
    }

    #[test]
    fn subtasks_follow_their_parent_over_several_levels() {
        let todos = vec![
            Todo { due: due(12), ..todo("p", "parent") },
            Todo { due: due(14), ..todo("q", "other") },
            Todo { due: due(13), parent: Some("p".into()), ..todo("c2", "child late") },
            Todo { due: due(12), parent: Some("p".into()), ..todo("c1", "child early") },
            Todo { parent: Some("c1".into()), ..todo("g", "grandchild") },
        ];
        assert_eq!(
            shown(&todos, &Filter::All, false),
            [("parent", 0), ("child early", 1), ("grandchild", 2), ("child late", 1), ("other", 0)]
        );
    }

    #[test]
    fn subtask_of_filtered_out_parent_is_top_level() {
        let mut parent = todo("p", "finished parent");
        parent.completed = true;
        let todos = vec![parent, Todo { parent: Some("p".into()), ..todo("c", "open child") }];
        assert_eq!(shown(&todos, &Filter::All, false), [("open child", 0)]);
        assert_eq!(shown(&todos, &Filter::All, true), [("finished parent", 0), ("open child", 1)]);

        // Parent not due today, so the Today filter drops it; the child stays at depth 0.
        let todos = vec![
            Todo { due: due(12), ..todo("p", "future parent") },
            Todo { due: due(11), parent: Some("p".into()), ..todo("c", "today child") },
        ];
        assert_eq!(shown(&todos, &Filter::Today, false), [("today child", 0)]);
    }

    #[test]
    fn parent_cycle_loses_no_task() {
        let todos = vec![
            Todo { parent: Some("b".into()), ..todo("a", "first") },
            Todo { parent: Some("a".into()), ..todo("b", "second") },
            Todo { parent: Some("a".into()), ..todo("self", "self") },
        ];
        let mut result = shown(&todos, &Filter::All, false);
        assert_eq!(result.len(), 3);
        result.sort_unstable();
        let mut names: Vec<&str> = result.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        assert_eq!(names, ["first", "second", "self"]);
    }

    #[test]
    fn date_time_dues_count_on_their_local_day() {
        let at = |d: u32| Some(EventTime::DateTime(datetime::to_utc(day(d).and_hms_opt(14, 0, 0).expect("time"))));
        let todos = vec![
            Todo { due: at(11), ..todo("1", "this afternoon") },
            Todo { due: at(12), ..todo("2", "tomorrow afternoon") },
        ];
        assert_eq!(names(&todos, &Filter::Today, false), ["this afternoon"]);
        assert_eq!(names(&todos, &Filter::Planned, false), ["this afternoon", "tomorrow afternoon"]);
    }

    #[test]
    fn due_text_is_relative_to_today() {
        assert_eq!(due_text(EventTime::Date(day(11)), today()), tr("Today"));
        assert_eq!(due_text(EventTime::Date(day(12)), today()), tr("Tomorrow"));
        assert_eq!(due_text(EventTime::Date(day(10)), today()), tr("Yesterday"));
        assert_eq!(due_text(EventTime::Date(day(14)), today()), datetime::format_short_date(day(14), today()));
        assert_eq!(due_text(EventTime::Date(day(5)), today()), datetime::format_short_date(day(5), today()));
        let short = trf("{weekday}, {day} {month}", &[("weekday", &tr("Wed")), ("day", &14), ("month", &tr("Oct"))]);
        assert_eq!(due_text(EventTime::Date(day(14)), today()), short, "one short form, without leading zeros");
    }

    #[test]
    fn due_text_shows_time_of_day() {
        let at = EventTime::DateTime(datetime::to_utc(day(11).and_hms_opt(14, 0, 0).expect("time")));
        assert_eq!(due_text(at, today()), format!("{}, 14:00", tr("Today")));
        let later = EventTime::DateTime(datetime::to_utc(day(14).and_hms_opt(9, 30, 0).expect("time")));
        assert_eq!(due_text(later, today()), format!("{} 09:30", datetime::format_short_date(day(14), today())));
    }

    #[test]
    fn row_dates_are_short_and_leave_out_what_the_head_says() {
        let at = |d: u32, h: u32| EventTime::DateTime(datetime::to_utc(day(d).and_hms_opt(h, 0, 0).expect("time")));
        let date = |d: u32| EventTime::Date(day(d));
        assert_eq!(row_due(at(11, 16), today(), true), "16:00", "under Today the time is enough");
        assert_eq!(row_due(date(11), today(), true), "", "and a day without a time shows nothing");
        assert_eq!(row_due(date(11), today(), false), tr("Today"));
        assert_eq!(row_due(at(11, 16), today(), false), format!("{} 16:00", tr("Today")));
        let weekday = |d: u32, name: &'static str| trf("{weekday} {day}", &[("weekday", &tr(name)), ("day", &d)]);
        assert_eq!(row_due(date(9), today(), false), weekday(9, "Fri"), "within a week: the weekday");
        assert_eq!(row_due(at(12, 9), today(), false), weekday(12, "Mon"), "another day: no time");
        assert_eq!(row_due(date(17), today(), false), weekday(17, "Sat"));
        assert_eq!(row_due(date(20), today(), false), datetime::format_day_month(day(20)), "further: day and month");
        let next_year = NaiveDate::from_ymd_opt(2027, 1, 20).expect("valid date");
        assert_eq!(
            row_due(EventTime::Date(next_year), today(), false),
            datetime::format_short_date(next_year, today())
        );
    }

    #[test]
    fn overdue_means_open_and_before_today() {
        let overdue = Todo { due: due(10), ..todo("1", "a") };
        let done = Todo { completed: true, ..overdue.clone() };
        assert!(is_overdue(&overdue, today()));
        assert!(!is_overdue(&done, today()));
        assert!(!is_overdue(&Todo { due: due(11), ..todo("2", "b") }, today()));
        assert!(!is_overdue(&todo("3", "c"), today()));
    }

    #[test]
    fn priority_classes_follow_icalendar() {
        let classes: Vec<Prio> =
            [Some(1), Some(2), Some(4), Some(5), Some(6), Some(9), Some(0), None].into_iter().map(Prio::of).collect();
        let expected =
            [Prio::Urgent, Prio::High, Prio::High, Prio::Medium, Prio::Low, Prio::Low, Prio::None, Prio::None];
        assert_eq!(classes, expected, "1 is urgent, the square");
        let levels: Vec<usize> = expected.iter().map(|prio| prio.level()).collect();
        assert_eq!(levels, [3, 3, 3, 2, 1, 1, 0, 0]);
    }

    #[test]
    fn every_priority_class_saves_a_value_of_its_own_class() {
        for prio in Prio::ALL {
            assert_eq!(Prio::of(prio.ical()), prio, "{prio:?}");
        }
        let saved: Vec<Option<u8>> = Prio::ALL.iter().map(|prio| prio.ical()).collect();
        assert_eq!(saved, [None, Some(1), Some(3), Some(5), Some(9)]);
    }

    fn summaries(todos: &[Todo], d: &ListData<'_>) -> Vec<(Group, Vec<String>)> {
        grouped(d)
            .into_iter()
            .map(|(group, items)| (group, items.into_iter().map(|(i, _)| todos[i].summary.clone()).collect()))
            .collect()
    }

    #[test]
    fn groups_follow_the_due_day_and_keep_subtasks_with_their_parent() {
        let mut done = Todo { due: due(9), ..todo("d", "done") };
        done.completed = true;
        let todos = vec![
            Todo { due: due(12), ..todo("u", "later") },
            Todo { due: due(11), ..todo("t", "today") },
            Todo { due: due(9), ..todo("o", "overdue") },
            // An undated subtask stays under its overdue parent instead of moving to "No date".
            Todo { parent: Some("o".into()), ..todo("c", "child") },
            todo("n", "undated"),
            done,
        ];
        let strings = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(
            summaries(&todos, &data(&todos, &Filter::All, true)),
            [
                (Group::Overdue, strings(&["overdue", "child"])),
                (Group::Today, strings(&["today"])),
                (Group::Upcoming, strings(&["later"])),
                (Group::NoDate, strings(&["undated"])),
                (Group::Completed, strings(&["done"])),
            ]
        );
    }

    #[test]
    fn grouping_by_list_priority_or_none() {
        let todos = vec![
            Todo { due: due(9), priority: Some(5), collection: "home".into(), ..todo("a", "bread") },
            Todo { due: due(10), priority: Some(1), ..todo("b", "report") },
            Todo { due: due(12), collection: "home".into(), ..todo("c", "milk") },
            Todo { parent: Some("b".into()), collection: "home".into(), ..todo("d", "figures") },
        ];
        let filter = Filter::All;
        let mut d = data(&todos, &filter, false);
        d.names = [("home".to_string(), "Home".to_string()), ("work".to_string(), "Errands".to_string())].into();
        let strings = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();

        d.group_by = GroupBy::List;
        assert_eq!(
            summaries(&todos, &d),
            [
                (Group::List("work".into()), strings(&["report", "figures"])),
                (Group::List("home".into()), strings(&["bread", "milk"])),
            ],
            "lists by name (Errands before Home), a subtask stays with its parent whatever its list"
        );
        d.group_by = GroupBy::Priority;
        assert_eq!(
            summaries(&todos, &d),
            [
                (Group::Priority(Prio::Urgent), strings(&["report", "figures"])),
                (Group::Priority(Prio::Medium), strings(&["bread"])),
                (Group::Priority(Prio::None), strings(&["milk"])),
            ]
        );
        d.group_by = GroupBy::None;
        assert_eq!(summaries(&todos, &d), [(Group::All, strings(&["bread", "report", "figures", "milk"]))]);
        assert_eq!(Group::All.label(&d.names), "", "the headless run has no name");
        assert!(!Group::All.can_add() && !Group::Completed.can_add() && Group::Today.can_add());
    }

    #[test]
    fn order_is_the_row_order_and_spans_stack_up() {
        let todos = vec![
            Todo { due: due(12), ..todo("u", "later") },
            Todo { due: due(9), ..todo("o", "overdue") },
            Todo { parent: Some("o".into()), ..todo("c", "child") },
            Todo { due: due(11), ..todo("t", "today") },
        ];
        let filter = Filter::All;
        let d = data(&todos, &filter, false);
        let names: Vec<&str> = order(&d).into_iter().map(|i| todos[i].summary.as_str()).collect();
        assert_eq!(names, ["overdue", "child", "today", "later"]);

        let span = |d: &ListData<'_>, uid: &str| row_span(d, todos.iter().position(|t| t.uid == uid).expect("task"));
        let (head, row) = (size::ROW_GROUP, size::ROW);
        assert_eq!(span(&d, "o"), Some((0.0, head + row)), "the first row of a group starts at its head");
        assert_eq!(span(&d, "c"), Some((head + row, head + 2.0 * row)));
        assert_eq!(span(&d, "t"), Some((head + 2.0 * row, 2.0 * head + 3.0 * row)));
        assert_eq!(span(&d, "u").map(|(_, bottom)| bottom), Some(3.0 * head + 4.0 * row));
        assert_eq!(row_span(&d, 99), None);

        // Fold "Today": its row is gone, and the "Upcoming" head touches it with a line between.
        let folded: HashSet<Group> = [Group::Today].into();
        let d = ListData { collapsed: &folded, ..data(&todos, &filter, false) };
        let names: Vec<&str> = order(&d).into_iter().map(|i| todos[i].summary.as_str()).collect();
        assert_eq!(names, ["overdue", "child", "later"]);
        assert_eq!(span(&d, "t"), None);
        let upcoming = 2.0 * head + 2.0 * row;
        assert_eq!(span(&d, "u"), Some((upcoming, upcoming + LINE + head + row)));

        // Without heads the rows start at the top.
        let d = ListData { group_by: GroupBy::None, ..data(&todos, &filter, false) };
        assert_eq!(span(&d, "o"), Some((0.0, row)));
    }

    #[test]
    fn nothing_shown_means_no_groups() {
        assert!(grouped(&data(&[], &Filter::Today, false)).is_empty());
    }

    #[test]
    fn group_labels_are_translated() {
        let names = HashMap::from([("work".to_string(), "Work".to_string())]);
        assert_eq!(Group::Overdue.label(&names), tr("Overdue"));
        assert_eq!(Group::NoDate.label(&names), tr("No date"));
        assert_eq!(Group::Completed.label(&names), tr("Completed"));
        assert_eq!(Group::List("work".into()).label(&names), "Work");
        assert_eq!(Group::Priority(Prio::Urgent).label(&names), tr("Urgent"));
        assert_eq!(Group::Priority(Prio::None).label(&names), tr("No priority"));
    }

    #[test]
    fn open_count_ignores_done_tasks() {
        let mut done = Todo { due: due(11), ..todo("d", "done") };
        done.completed = true;
        let todos =
            vec![Todo { due: due(10), ..todo("o", "overdue") }, Todo { due: due(13), ..todo("l", "later") }, done];
        assert_eq!(open_count(&todos, &Filter::Today, today()), 1);
        assert_eq!(open_count(&todos, &Filter::Planned, today()), 2);
        assert_eq!(open_count(&todos, &Filter::All, today()), 2);
    }

    #[test]
    fn narrower_lists_drop_labels_then_hints_then_the_pill_then_the_repository() {
        let parts = |width: f32| {
            let p = RowParts::for_width(width);
            (p.labels, p.hints, p.pill, p.full_ref)
        };
        assert_eq!(parts(size::CONTENT_MAX), (true, true, true, true), "720: everything");
        assert_eq!(parts(700.0), (true, true, true, true));
        assert_eq!(parts(699.0), (false, true, true, true), "labels go first");
        assert_eq!(parts(600.0), (false, true, true, true));
        assert_eq!(parts(599.0), (false, false, true, true), "then the hints");
        assert_eq!(parts(559.0), (false, false, false, true), "then the pill becomes a dot");
        assert_eq!(parts(519.0), (false, false, false, false), "then repo#123 becomes #123");
        assert_eq!(parts(size::LIST), (false, false, false, false), "the list beside the detail (460)");
        assert_eq!(parts(size::LIST_MEDIUM), (false, false, false, false));
    }

    fn details(kind: GitHubKind, state: GitHubState) -> GitHubDetails {
        GitHubDetails {
            kind,
            state,
            labels: vec![GitHubLabel { name: "bug".into(), color: Some("#D73A4A".into()) }],
            assignees: Vec::new(),
            milestone: None,
            milestone_due: None,
            draft: false,
            review: None,
            checks: None,
            conflict: false,
        }
    }

    #[test]
    fn github_items_show_their_type_and_state() {
        use GitHubKind::{Issue, PullRequest};
        use GitHubState::{Closed, Merged, Open};
        assert_eq!(github_mark(&details(Issue, Open)), (Icon::CircleDot, Tone::Ok));
        assert_eq!(github_mark(&details(Issue, Closed)), (Icon::CircleCheck, Tone::Merged));
        assert_eq!(github_mark(&details(PullRequest, Open)), (Icon::GitPullRequest, Tone::Ok));
        let draft = GitHubDetails { draft: true, ..details(PullRequest, Open) };
        assert_eq!(github_mark(&draft), (Icon::GitPullRequestDraft, Tone::Text3));
        assert_eq!(github_mark(&details(PullRequest, Merged)), (Icon::GitMerge, Tone::Merged));
        assert_eq!(github_mark(&details(PullRequest, Closed)), (Icon::GitPullRequestClosed, Tone::Error));
        assert_eq!(checks_mark(ChecksState::Pending), (Icon::CircleDotDashed, Tone::Attention));
        assert_eq!(checks_mark(ChecksState::Success), (Icon::CircleCheck, Tone::Ok));
        assert_eq!(checks_mark(ChecksState::Failure), (Icon::CircleX, Tone::Error));
    }

    #[test]
    fn hints_count_subtasks_and_notes() {
        let todos = vec![
            Todo { description: Some("Read it first.".into()), ..todo("p", "parent") },
            Todo { parent: Some("p".into()), completed: true, ..todo("a", "a") },
            Todo { parent: Some("p".into()), ..todo("b", "b") },
            Todo { description: Some("  ".into()), ..todo("q", "blank notes") },
        ];
        let counts = subtask_counts(&todos);
        assert_eq!(hints_of(&todos[0], &counts), [Hint::Subtasks(1, 2), Hint::Notes]);
        assert!(hints_of(&todos[3], &counts).is_empty(), "blank notes are no notes");
        assert!(Hint::Subtasks(1, 2).width() > Hint::Notes.width());
    }

    #[test]
    fn every_row_and_head_builds_in_every_width_and_grouping() {
        let mut gh = Todo { due: due(9), ..todo("g", "Tray icon disappears after resuming from sleep") };
        let mut labelled = details(GitHubKind::PullRequest, GitHubState::Open);
        labelled.labels.extend(labelled.labels.clone());
        labelled.labels.push(GitHubLabel { name: "ui".into(), color: None });
        labelled.checks = Some(ChecksState::Pending);
        labelled.review = Some(ReviewState::Required);
        gh.link = Some(Link { github: Some(labelled), ..Link::github("mahype/TorroWorkplace", 131) });
        let todos = vec![
            gh,
            Todo { due: due(11), priority: Some(1), description: Some("x".into()), ..todo("t", "today") },
            Todo { parent: Some("t".into()), ..todo("c", "child") },
            Todo { completed: true, ..todo("d", "done") },
        ];
        let filter = Filter::All;
        let folded: HashSet<Group> = [Group::Today].into();
        for group_by in [GroupBy::Due, GroupBy::List, GroupBy::Priority, GroupBy::Repo, GroupBy::None] {
            for width in [size::CONTENT_MAX, size::LIST, size::LIST_MEDIUM, 300.0] {
                for read_only in [false, true] {
                    let d = ListData { group_by, width, read_only, collapsed: &folded, ..data(&todos, &filter, true) };
                    let _ = view(d);
                }
            }
        }
        let _ = view(ListData { read_only: true, ..data(&[], &filter, true) });
    }

    #[test]
    fn issues_of_a_filter_list_get_a_check_locked_without_write_access_and_pull_requests_none() {
        let item = |uid: &str, kind: GitHubKind, state: GitHubState| Todo {
            collection: "github:mahype/bugs".into(),
            completed: state != GitHubState::Open,
            link: Some(Link { github: Some(details(kind, state)), ..Link::github("mahype/TorroWorkplace", 7) }),
            ..todo(uid, uid)
        };
        let todos = vec![
            item("open", GitHubKind::Issue, GitHubState::Open),
            item("closed", GitHubKind::Issue, GitHubState::Closed),
            item("pull", GitHubKind::PullRequest, GitHubState::Open),
        ];
        let filter = Filter::List("github:mahype/bugs".into());
        for may_close in [true, false] {
            let lists = HashMap::from([("github:mahype/bugs".to_string(), may_close)]);
            let _ = view(ListData { read_only: true, issue_lists: lists, ..data(&todos, &filter, true) });
        }
        // The check builds in both states, open and locked, as the detail uses it too.
        let _: Element<'_, Message> = issue_check(false, style::CHECK_SIZE, Some(Message::ToggleDone(0)));
        let _: Element<'_, Message> = issue_check(true, style::CHECK_SIZE, None);
    }

    #[test]
    fn grouping_by_repository_puts_tasks_without_one_last() {
        let item = |uid: &str, repo: &str, number: u64| Todo {
            link: Some(Link {
                github: Some(details(GitHubKind::Issue, GitHubState::Open)),
                ..Link::github(repo, number)
            }),
            ..todo(uid, uid)
        };
        let todos = vec![
            item("w1", "mahype/TorroWorkplace", 1),
            todo("plain", "plain"),
            item("d1", "mahype/torro-design", 18),
            item("w2", "mahype/TorroWorkplace", 2),
        ];
        let filter = Filter::All;
        let d = ListData { group_by: GroupBy::Repo, ..data(&todos, &filter, false) };
        let strings = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(
            summaries(&todos, &d),
            [
                (Group::Repo("mahype/torro-design".into()), strings(&["d1"])),
                (Group::Repo("mahype/TorroWorkplace".into()), strings(&["w1", "w2"])),
                (Group::Repo(String::new()), strings(&["plain"])),
            ],
            "by name regardless of case, then the tasks without a repository"
        );
        assert_eq!(Group::Repo("mahype/torro-design".into()).label(&d.names), "mahype/torro-design");
        assert_eq!(Group::Repo(String::new()).label(&d.names), tr("No repository"));
        assert!(!Group::Repo("mahype/torro-design".into()).can_add(), "nothing is added to GitHub here");
    }

    /// A task of the list "work" stored at its own href, with a number for manual order.
    fn placed(uid: &str, sort_order: i64) -> Todo {
        Todo { href: Some(format!("work/{uid}.ics")), sort_order: Some(sort_order), ..todo(uid, uid) }
    }

    /// The list "work" in manual order: a (with subtasks a1, a2), b, c – and, done, d.
    fn manual_list() -> Vec<Todo> {
        vec![
            Todo { due: due(9), ..placed("c", 30) },
            placed("b", 20),
            Todo { parent: Some("a".into()), ..placed("a2", 2) },
            placed("a", 10),
            Todo { parent: Some("a".into()), ..placed("a1", 1) },
            Todo { completed: true, ..placed("d", 5) },
        ]
    }

    fn index_of(todos: &[Todo], uid: &str) -> usize {
        todos.iter().position(|t| t.uid == uid).expect("a task of the sample")
    }

    #[test]
    fn manual_order_is_one_run_by_hand_with_subtasks_under_their_parent() {
        let todos = manual_list();
        let filter = Filter::List("work".into());
        let d = ListData { manual: true, ..data(&todos, &filter, true) };
        assert_eq!(
            shown_manual(&todos, &d),
            [("a", 0), ("a1", 1), ("a2", 1), ("b", 0), ("c", 0), ("d", 0)],
            "by number, not by due date; done ones last"
        );
        let groups: Vec<Group> = grouped(&d).into_iter().map(|(group, _)| group).collect();
        assert_eq!(groups, [Group::All], "no heads, not even Overdue for c");
        assert_eq!(row_span(&d, index_of(&todos, "b")), Some((3.0 * size::ROW, 4.0 * size::ROW)));
        let _ = view(ListData { drag: Some((index_of(&todos, "b"), Some(1))), ..d });
    }

    fn shown_manual<'a>(todos: &'a [Todo], d: &ListData<'a>) -> Vec<(&'a str, usize)> {
        arrange(d).into_iter().map(|(i, depth)| (todos[i].uid.as_str(), depth)).collect()
    }

    #[test]
    fn a_task_may_only_go_among_the_tasks_it_is_ordered_with() {
        let todos = manual_list();
        let filter = Filter::List("work".into());
        let d = ListData { manual: true, ..data(&todos, &filter, true) };
        let at = |uid: &str| Some(index_of(&todos, uid));
        // Rows: a 0, a1 1, a2 2, b 3, c 4, d 5.
        assert_eq!(
            drop_places(&d, index_of(&todos, "b")),
            [(0, at("a")), (3, at("b")), (4, at("c")), (5, None)],
            "a top-level task goes above a, b or c, or below c – never between a and its subtasks"
        );
        assert_eq!(drop_places(&d, index_of(&todos, "a2")), [(1, at("a1")), (2, at("a2")), (3, None)]);
        assert_eq!(drop_places(&d, index_of(&todos, "d")), [(5, at("d")), (6, None)], "done ones among themselves");
        let hidden = ListData { show_completed: false, ..data(&todos, &filter, false) };
        assert!(drop_places(&ListData { manual: true, ..hidden }, index_of(&todos, "d")).is_empty());

        let places = drop_places(&d, index_of(&todos, "b"));
        assert_eq!(nearest_place(&places, 0.0), Some((0, at("a"))));
        assert_eq!(nearest_place(&places, 1.4 * size::ROW), Some((0, at("a"))), "nearer a than b");
        assert_eq!(nearest_place(&places, 4.6 * size::ROW), Some((5, None)));
        assert_eq!(nearest_place(&[], 10.0), None);
    }

    #[test]
    fn alt_arrows_step_among_the_tasks_of_the_level_and_stop_at_its_edges() {
        let todos = manual_list();
        let filter = Filter::List("work".into());
        let d = ListData { manual: true, ..data(&todos, &filter, true) };
        let step = |uid: &str, up: bool| step_target(&d, index_of(&todos, uid), up);
        let before = |uid: &str| Some(Some(index_of(&todos, uid)));
        assert_eq!(step("b", true), before("a"));
        assert_eq!(step("b", false), Some(None), "below c: the end");
        assert_eq!(step("a", false), before("c"), "past b, with its subtasks");
        assert_eq!(step("a", true), None, "the first stays");
        assert_eq!(step("c", false), None, "the last stays");
        assert_eq!(step("c", true), before("b"));
        assert_eq!(step("a1", false), Some(None), "a subtask stays under its parent");
        assert_eq!(step("a2", true), before("a1"));
        assert_eq!(step("a1", true), None);
        assert_eq!((step("d", true), step("d", false)), (None, None), "alone among the done ones");
    }
}
