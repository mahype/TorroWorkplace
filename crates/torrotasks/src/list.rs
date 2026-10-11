//! The task list (task K.14 in `docs/tasks/kalender-aufgaben.md`).
//!
//! [`arrange`] picks and orders the tasks for a filter; [`view`] renders them: checkbox
//! (→ `ToggleDone(index)`), title (struck through and dimmed when done; click →
//! `Select(index)`), a due badge (`Today`, `Tomorrow`, `Mo, 12.10.`; overdue in the attention
//! colour – never Torro red), a priority mark for high (1–4) and medium (5), the list's
//! colour dot outside list filters. Subtasks (`parent` = another task's uid) sit indented
//! under their parent. The selected task is highlighted. Empty → a friendly empty state
//! ("Nothing to do").

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::{Space, checkbox, column, container, mouse_area, rich_text, row, scrollable, span, text};
use iced::{Alignment, Background, Color, Element, Length, mouse};
use torro_core::i18n::tr;
use torro_core::theme::STATUS_ATTENTION;
use torro_gui::datetime;
use torro_gui::theme;
use torro_pim::{EventTime, Todo};

use crate::app::{Filter, Message};

/// Indent per level of subtask.
const INDENT: f32 = 20.0;

/// What the list gets to draw.
pub struct ListData<'a> {
    pub todos: &'a [Todo],
    /// Collection id → colour.
    pub colors: HashMap<String, Color>,
    pub filter: &'a Filter,
    pub show_completed: bool,
    pub today: NaiveDate,
    /// Index into `todos` of the task in the detail panel.
    pub selected: Option<usize>,
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
fn due_text(due: EventTime, today: NaiveDate) -> String {
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

/// `!!` for high (1–4), `!` for medium (5), nothing otherwise.
fn priority_mark(priority: Option<u8>) -> Option<&'static str> {
    match priority {
        Some(1..=4) => Some("!!"),
        Some(5) => Some("!"),
        _ => None,
    }
}

pub fn view<'a>(data: ListData<'a>) -> Element<'a, Message> {
    let items = arrange(&data);
    if items.is_empty() {
        return empty_state();
    }
    let mut list = column![].spacing(2).width(Length::Fill);
    for (index, depth) in items {
        list = list.push(row_view(&data, index, depth));
    }
    scrollable(list).width(Length::Fill).height(Length::Fill).into()
}

fn empty_state<'a>() -> Element<'a, Message> {
    let hint = column![
        text(tr("Nothing to do")).size(18).color(theme::MUTED),
        text(tr("Add a task above")).size(13).color(theme::MUTED),
    ]
    .spacing(6)
    .align_x(Alignment::Center);
    container(hint).center(Length::Fill).into()
}

/// One task: indent, checkbox, title, badges; the selected one gets the surface colour.
fn row_view<'a>(data: &ListData<'a>, index: usize, depth: usize) -> Element<'a, Message> {
    let todo = &data.todos[index];
    let selected = data.selected == Some(index);
    let line = row![
        Space::new().width(Length::Fixed(depth as f32 * INDENT)),
        checkbox(todo.completed).on_toggle(move |_| Message::ToggleDone(index)),
        title_view(todo, index),
        badges_view(data, todo),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    container(line)
        .padding([6, 8])
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: selected.then_some(Background::Color(theme::SURFACE)),
            ..Default::default()
        })
        .into()
}

fn title_view<'a>(todo: &Todo, index: usize) -> Element<'a, Message> {
    let color = if todo.completed { theme::MUTED } else { theme::TEXT };
    let fragment: text::Span<'static, ()> = span(todo.summary.clone()).color(color).strikethrough(todo.completed);
    let title = rich_text(vec![fragment]).size(15).width(Length::Fill);
    mouse_area(title).on_press(Message::Select(index)).interaction(mouse::Interaction::Pointer).into()
}

/// Due date, priority and (outside list filters) the list's colour, right aligned.
fn badges_view<'a>(data: &ListData<'a>, todo: &Todo) -> Element<'a, Message> {
    let mut badges = row![].spacing(8).align_y(Alignment::Center);
    if let Some(due) = todo.due {
        let color = if is_overdue(todo, data.today) { theme::color(STATUS_ATTENTION) } else { theme::MUTED };
        badges = badges.push(text(due_text(due, data.today)).size(13).color(color));
    }
    if let Some(mark) = priority_mark(todo.priority) {
        let color = if todo.completed { theme::MUTED } else { theme::TEXT };
        badges = badges.push(text(mark).size(13).color(color));
    }
    if !matches!(data.filter, Filter::List(_)) {
        let color = data.colors.get(&todo.collection).copied().unwrap_or(theme::MUTED);
        badges = badges.push(dot(color));
    }
    badges.into()
}

/// The list's colour, a small round dot.
fn dot<'a>(color: Color) -> Element<'a, Message> {
    container(Space::new().width(Length::Fixed(8.0)).height(Length::Fixed(8.0)))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: iced::border::rounded(4),
            ..Default::default()
        })
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

    /// Summaries of the arranged tasks with their depth.
    fn shown<'a>(todos: &'a [Todo], filter: &'a Filter, show_completed: bool) -> Vec<(&'a str, usize)> {
        let data = ListData { todos, colors: HashMap::new(), filter, show_completed, today: today(), selected: None };
        arrange(&data).into_iter().map(|(i, depth)| (todos[i].summary.as_str(), depth)).collect()
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
        assert_eq!(due_text(EventTime::Date(day(14)), today()), "Mi, 14.10.");
        assert_eq!(due_text(EventTime::Date(day(5)), today()), "Mo, 05.10.");
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
    fn priority_marks() {
        assert_eq!(priority_mark(Some(1)), Some("!!"));
        assert_eq!(priority_mark(Some(4)), Some("!!"));
        assert_eq!(priority_mark(Some(5)), Some("!"));
        assert_eq!(priority_mark(Some(6)), None);
        assert_eq!(priority_mark(Some(9)), None);
        assert_eq!(priority_mark(Some(0)), None);
        assert_eq!(priority_mark(None), None);
    }
}
