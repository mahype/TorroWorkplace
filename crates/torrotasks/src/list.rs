//! The task list (task K.14 in `docs/tasks/kalender-aufgaben.md`, Linear look from T.1 in
//! `docs/tasks/design.md`).
//!
//! [`arrange`] picks and orders the tasks for a filter, [`grouped`] splits them into the groups
//! of the list (Overdue, Today, Upcoming, No date, Completed); [`view`] renders them as
//! `widgets::list_row`s under `widgets::group_head`s, after `docs/prototypes/linear/tasks.html`:
//! round check in the list's colour (→ `ToggleDone(index)`), priority as three rising bars, the
//! title on one line (struck through and dimmed when done), the list as a pill (only where the
//! filter mixes lists and the row is wide enough), the due date in `META` tabular (overdue in the
//! attention colour – never Torro red). A click on the row → `Select(index)`. Subtasks
//! (`parent` = another task's uid) sit indented under their parent. Empty → an empty state
//! ("Nothing to do").
//!
//! For the keyboard (T.3) [`order`] gives the visible order of the rows and [`row_span`] where a
//! row lies in the scrolled content, so `app.rs` can step through the rows and keep the chosen
//! one in view; the scrollable reports its viewport as `Message::Scrolled`.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::{Space, checkbox, column, container, rich_text, row, scrollable, span, text};
use iced::{Alignment, Background, Color, Element, Fill, Padding, Theme, border};
use torro_core::i18n::tr;
use torro_gui::theme::{palette, size, space};
use torro_gui::widgets::{self, Mark};
use torro_gui::{datetime, icon, style, typo};
use torro_pim::{EventTime, Todo};

use crate::app::{Filter, Message};
use torro_gui::icon::Icon;

/// What the list gets to draw.
pub struct ListData<'a> {
    pub todos: &'a [Todo],
    /// Collection id → colour.
    pub colors: HashMap<String, Color>,
    /// Collection id → name, for the list pill.
    pub names: HashMap<String, String>,
    pub filter: &'a Filter,
    pub show_completed: bool,
    pub today: NaiveDate,
    /// Index into `todos` of the task in the detail panel.
    pub selected: Option<usize>,
    /// Whether rows carry the list pill: off in a list filter (every row has the same list) and
    /// below the medium layout (the title needs the room).
    pub show_list: bool,
    /// The attention colour for text of the current mode (`Palette::attention_text`), for the
    /// "Overdue" group icon (a mark takes a fixed colour).
    pub attention: Color,
}

/// The tasks to show as `(index into todos, depth)`: `depth` 0 for top-level tasks, 1 for
/// their subtasks and so on. Filter: `Today` = due today or earlier; `Planned` = with a due
/// date; `All`; `List(id)` = that collection. Completed ones only with `show_completed`.
/// Order: open before done, then by due date (none last), then priority (1 first, none
/// last), then title; subtasks follow their parent in the same order. A subtask whose
/// parent is filtered out is shown at depth 0.
pub fn arrange(data: &ListData<'_>) -> Vec<(usize, usize)> {
    let todos = data.todos;
    let mut shown: Vec<usize> = (0..todos.len()).filter(|&i| is_shown(&todos[i], data)).collect();
    shown.sort_by(|&a, &b| compare(&todos[a], &todos[b]));

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
fn is_shown(todo: &Todo, data: &ListData<'_>) -> bool {
    if todo.completed && !data.show_completed {
        return false;
    }
    match data.filter {
        Filter::Today => todo.due.is_some_and(|due| due_date(due) <= data.today),
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

/// `Today`, `Tomorrow`, `Yesterday`, otherwise `Mo, 12.10.` (short weekday name from
/// `datetime::weekday_name`); a time of day is appended (`Today 14:00`).
pub(crate) fn due_text(due: EventTime, today: NaiveDate) -> String {
    let day = due_date(due);
    let name = match (day - today).num_days() {
        0 => tr("Today").to_string(),
        1 => tr("Tomorrow").to_string(),
        -1 => tr("Yesterday").to_string(),
        _ => format!("{}, {}", datetime::weekday_name(day, true), day.format("%d.%m.")),
    };
    match due {
        EventTime::Date(_) => name,
        EventTime::DateTime(at) => format!("{name} {}", datetime::format_time(datetime::to_local(at).time())),
    }
}

/// Linear's priority levels: how many of the three bars are lit. iCalendar 1–4 is high (3),
/// 5 medium (2), 6–9 low (1); 0 and anything else is none (0, all bars dim).
pub(crate) fn priority_level(priority: Option<u8>) -> usize {
    match priority {
        Some(1..=4) => 3,
        Some(5) => 2,
        Some(6..=9) => 1,
        _ => 0,
    }
}

/// The groups of the list, top to bottom. Open tasks fall into the first four by their due day,
/// done ones (shown with "Completed" on) into the last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Group {
    fn of(todo: &Todo, today: NaiveDate) -> Group {
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

    fn label(self) -> &'static str {
        match self {
            Group::Overdue => tr("Overdue"),
            Group::Today => tr("Today"),
            Group::Upcoming => tr("Upcoming"),
            Group::NoDate => tr("No date"),
            Group::Completed => tr("Completed"),
        }
    }

    /// The icon of the group head; "Overdue" carries the attention colour as in the mock-up.
    fn mark(self, attention: Color) -> Mark {
        match self {
            Group::Overdue => Mark::Tinted(Icon::CircleAlert, attention),
            Group::Today => Mark::Icon(Icon::Sun),
            Group::Upcoming => Mark::Icon(Icon::CalendarClock),
            Group::NoDate => Mark::Icon(Icon::Layers),
            Group::Completed => Mark::Icon(Icon::CircleCheck),
        }
    }
}

/// [`arrange`]'s tasks split into their [`Group`]s, in display order and without empty groups.
/// A top-level task decides the group; its subtasks stay with it, so a tree is never split.
/// [`arrange`] sorts open before done and then by due date, so every group is one run.
pub fn grouped(data: &ListData<'_>) -> Vec<(Group, Vec<(usize, usize)>)> {
    let mut groups: Vec<(Group, Vec<(usize, usize)>)> = Vec::new();
    for (index, depth) in arrange(data) {
        if depth == 0 {
            let group = Group::of(&data.todos[index], data.today);
            if groups.last().map(|(last, _)| *last) != Some(group) {
                groups.push((group, Vec::new()));
            }
        }
        if let Some((_, items)) = groups.last_mut() {
            items.push((index, depth));
        }
    }
    groups
}

/// How many open tasks `filter` shows: the counter of a sidebar row.
pub fn open_count(todos: &[Todo], filter: &Filter, today: NaiveDate) -> usize {
    let data = ListData {
        todos,
        colors: HashMap::new(),
        names: HashMap::new(),
        filter,
        show_completed: false,
        today,
        selected: None,
        show_list: false,
        attention: Color::TRANSPARENT,
    };
    todos.iter().filter(|todo| is_shown(todo, &data)).count()
}

/// The id of the list's scrollable, so the keyboard can scroll the chosen row into view.
pub const SCROLL: &str = "task-list";

/// Height of a group head: the 4 above it plus the head itself (`widgets::group_head`).
const GROUP_HEAD: f32 = space::XS + size::ROW_GROUP;
/// The one-pixel line of a group head.
const LINE: f32 = 1.0;
/// What the first head leaves out: the gap above it and its line above, because the quick add box
/// right above already ends the header area and a second line 8 below it would read as a double.
const FIRST_TRIM: f32 = space::XS + LINE;

/// Indices into `todos` of the rows as the list shows them, top to bottom.
pub fn order(data: &ListData<'_>) -> Vec<usize> {
    arrange(data).into_iter().map(|(index, _)| index).collect()
}

/// Where `todos[index]` lies in the list's content as `(top, bottom)`. The first row of a group
/// starts at its head, so scrolling up to it shows the head too. `None` if the row is not shown.
pub fn row_span(data: &ListData<'_>, index: usize) -> Option<(f32, f32)> {
    // The first head is shorter by [`FIRST_TRIM`]; it still starts at the top of the content.
    let mut y = -FIRST_TRIM;
    for (_, items) in grouped(data) {
        let head = y.max(0.0);
        y += GROUP_HEAD;
        for (n, (row, _)) in items.into_iter().enumerate() {
            if row == index {
                return Some((if n == 0 { head } else { y }, y + size::ROW));
            }
            y += size::ROW;
        }
    }
    None
}

/// The list: group heads with their rows, scrolling; the empty state when nothing is shown.
pub fn view<'a>(data: ListData<'a>) -> Element<'a, Message> {
    let groups = grouped(&data);
    if groups.is_empty() {
        return widgets::empty_state(tr("Nothing to do"), tr("Add a task above"));
    }
    let mut list = column![].width(Fill);
    for (n, (group, items)) in groups.into_iter().enumerate() {
        let head = widgets::group_head(group.mark(data.attention), group.label(), Some(items.len()), None);
        list = list.push(if n == 0 { without_top(head) } else { head });
        for (index, depth) in items {
            list = list.push(row_view(&data, index, depth));
        }
    }
    scrollable(list)
        .id(SCROLL)
        .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y, viewport.bounds().height))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Fill)
        .height(Fill)
        .into()
}

/// The group head without its gap and line above: moved up by [`FIRST_TRIM`] inside a clipping
/// container that is that much shorter, so only the part above the head's top line is cut off.
fn without_top<'a>(head: Element<'a, Message>) -> Element<'a, Message> {
    container(container(head).padding(Padding { top: -FIRST_TRIM, ..Padding::ZERO })).clip(true).into()
}

/// One task: indent, check, priority, title, list pill, due date.
fn row_view<'a>(data: &ListData<'a>, index: usize, depth: usize) -> Element<'a, Message> {
    let todo = &data.todos[index];
    let colour = data.colors.get(&todo.collection).copied().unwrap_or_else(|| widgets::collection_color(None));
    let mut cells: Vec<Element<'a, Message>> = Vec::new();
    if depth > 0 {
        cells.push(Space::new().width(depth as f32 * space::XL).into());
    }
    cells.push(
        checkbox(todo.completed)
            .size(style::CHECK_SIZE)
            .style(style::task_check(colour))
            .on_toggle(move |_| Message::ToggleDone(index))
            .into(),
    );
    cells.push(priority_view(priority_level(todo.priority)));
    // Iced 0.14 has no ellipsis: the title takes the free width and is clipped there, so it
    // never runs under the pill and the date, which keep their own width.
    cells.push(container(title_view(todo)).width(Fill).clip(true).into());
    if data.show_list {
        let name = data.names.get(&todo.collection).cloned().unwrap_or_default();
        cells.push(widgets::pill(Some(colour), name));
    }
    if let Some(due) = todo.due {
        let overdue = is_overdue(todo, data.today);
        let date = typo::text(typo::META.tabular(), due_text(due, data.today)).wrapping(text::Wrapping::None).style(
            move |t: &Theme| {
                let p = palette(t);
                text::Style { color: Some(if overdue { p.attention_text } else { p.text_3 }) }
            },
        );
        cells.push(date.into());
    }
    widgets::list_row(cells, data.selected == Some(index), Some(Message::Select(index)))
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
        }
    }

    fn due(d: u32) -> Option<EventTime> {
        Some(EventTime::Date(day(d)))
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
            attention: Color::BLACK,
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
        assert_eq!(due_text(EventTime::Date(day(14)), today()), format!("{}, 14.10.", tr("Wed")));
        assert_eq!(due_text(EventTime::Date(day(5)), today()), format!("{}, 05.10.", tr("Mon")));
    }

    #[test]
    fn due_text_shows_time_of_day() {
        let at = EventTime::DateTime(datetime::to_utc(day(11).and_hms_opt(14, 0, 0).expect("time")));
        assert_eq!(due_text(at, today()), format!("{} 14:00", tr("Today")));
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
    fn priority_levels_follow_icalendar() {
        assert_eq!(priority_level(Some(1)), 3);
        assert_eq!(priority_level(Some(4)), 3);
        assert_eq!(priority_level(Some(5)), 2);
        assert_eq!(priority_level(Some(6)), 1);
        assert_eq!(priority_level(Some(9)), 1);
        assert_eq!(priority_level(Some(0)), 0);
        assert_eq!(priority_level(None), 0);
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
        let groups: Vec<(Group, Vec<&str>)> = grouped(&data(&todos, &Filter::All, true))
            .into_iter()
            .map(|(group, items)| (group, items.into_iter().map(|(i, _)| todos[i].summary.as_str()).collect()))
            .collect();
        assert_eq!(
            groups,
            [
                (Group::Overdue, vec!["overdue", "child"]),
                (Group::Today, vec!["today"]),
                (Group::Upcoming, vec!["later"]),
                (Group::NoDate, vec!["undated"]),
                (Group::Completed, vec!["done"]),
            ]
        );
    }

    #[test]
    fn order_is_the_row_order_and_spans_stack_up() {
        let todos = vec![
            Todo { due: due(12), ..todo("u", "later") },
            Todo { due: due(9), ..todo("o", "overdue") },
            Todo { parent: Some("o".into()), ..todo("c", "child") },
            Todo { due: due(11), ..todo("t", "today") },
        ];
        let d = data(&todos, &Filter::All, false);
        let names: Vec<&str> = order(&d).into_iter().map(|i| todos[i].summary.as_str()).collect();
        assert_eq!(names, ["overdue", "child", "today", "later"]);

        let span = |uid: &str| row_span(&d, todos.iter().position(|t| t.uid == uid).expect("task"));
        // The first head is shorter by FIRST_TRIM.
        let (head, row) = (GROUP_HEAD, size::ROW);
        let first = head - FIRST_TRIM;
        assert_eq!(span("o"), Some((0.0, first + row)), "the first row of a group starts at its head");
        assert_eq!(span("c"), Some((first + row, first + 2.0 * row)));
        assert_eq!(span("t"), Some((first + 2.0 * row, first + head + 3.0 * row)));
        assert_eq!(span("u").map(|(_, bottom)| bottom), Some(first + 2.0 * head + 4.0 * row));
        assert_eq!(row_span(&d, 99), None);
    }

    #[test]
    fn nothing_shown_means_no_groups() {
        assert!(grouped(&data(&[], &Filter::Today, false)).is_empty());
    }

    #[test]
    fn group_labels_are_translated() {
        assert_eq!(Group::Overdue.label(), tr("Overdue"));
        assert_eq!(Group::NoDate.label(), tr("No date"));
        assert_eq!(Group::Completed.label(), tr("Completed"));
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
}
