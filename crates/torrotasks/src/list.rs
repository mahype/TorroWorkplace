//! The task list (task K.14 in `docs/tasks/kalender-aufgaben.md`).
//!
//! [`arrange`] picks and orders the tasks for a filter; [`view`] renders them: checkbox
//! (→ `ToggleDone(index)`), title (struck through and dimmed when done; click →
//! `Select(index)`), a due badge (`Heute`, `Morgen`, `Mo, 12.10.`; overdue in the attention
//! colour – never Torro red), a priority mark for high (1–4) and medium (5), the list's
//! colour dot outside list filters. Subtasks (`parent` = another task's uid) sit indented
//! under their parent. The selected task is highlighted. Empty → a friendly empty state
//! ("Nichts zu tun").

use std::collections::HashMap;

use chrono::NaiveDate;
use iced::widget::text;
use iced::{Color, Element};
use torro_pim::Todo;

use crate::app::{Filter, Message};

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
    let _ = data;
    Vec::new()
}

pub fn view<'a>(data: ListData<'a>) -> Element<'a, Message> {
    text(format!("{} Aufgaben", arrange(&data).len())).into()
}
