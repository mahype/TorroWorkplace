//! The task card in the assistant's chat (issue #797): due date, priority and list, with "Open in tasks"
//! (`apps.open_app` with `task:<uid>`) and "Done" (`tasks.save_todo` with `completed`, the task as the core resolved
//! it, so its `etag` guards against a change meanwhile). A task done already has no "Done".
//!
//! The list's colour is not drawn yet: `content_card` has no place for it; the head names the list instead.

use chrono::{DateTime, Utc};
use iced::Element;
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::content_card::{Action, ContentCard, Head};
use torro_gui::datetime;
use torro_gui::icon::Icon;
use torro_ipc::Request;
use torro_pim::{EventTime, Todo};

use super::{card_request, event, with_actions};
use crate::app::Message;

/// What "Open in tasks" sends: `apps.open_app` of the tasks app with `task:<uid>`.
pub(super) fn open(todo: &Todo) -> Message {
    let request = Request::OpenApp { app: AppId::TASKS.binary.to_owned(), target: Some(format!("task:{}", todo.uid)) };
    card_request(request, tr("Couldn't open the app."))
}

/// What "Done" sends: `tasks.save_todo` with the task as resolved and `completed` set.
pub(super) fn done(todo: &Todo) -> Message {
    let request = Request::SaveTodo { todo: Todo { completed: true, ..todo.clone() } };
    card_request(request, tr("Couldn't mark the task done."))
}

/// The buttons of the card of `todo`, the main one first: "Open in tasks", and "Done" while it is open – quiet
/// (`ghost`), as it only lets the task go.
pub(super) fn actions(todo: &Todo) -> Vec<Action<Message>> {
    let mut actions = vec![Action::new(Icon::ArrowUpRight, tr("Open in tasks"), open(todo))];
    if !todo.completed {
        actions.push(Action::new(Icon::Check, tr("Done"), done(todo)).quiet());
    }
    actions
}

/// The level (`widgets::priority_dot`: 1 low … 4 urgent) and word of an iCalendar priority as the tasks app shows
/// it (1 urgent, 2–4 high, 5 medium, 6–9 low); `None` without one.
fn priority(priority: Option<u8>) -> Option<(usize, &'static str)> {
    match priority? {
        1 => Some((4, tr("Urgent"))),
        2..=4 => Some((3, tr("High"))),
        5 => Some((2, tr("Medium"))),
        6..=9 => Some((1, tr("Low"))),
        _ => None,
    }
}

/// The card of `todo` in the list `list` (its colour `color`, not drawn yet), `now` for the due date: when it is
/// due, its priority (its dot in the priority's colour, with the word), and whether it is done.
pub(super) fn content(todo: &Todo, list: &str, color: Option<&str>, now: DateTime<Utc>) -> ContentCard<Message> {
    let _ = color;
    let today = datetime::to_local(now).date();
    let title = match todo.summary.trim() {
        "" => tr("(untitled)").to_owned(),
        title => title.to_owned(),
    };
    let mut card = ContentCard::new(Head::new(Icon::ListTodo, list), title);
    if todo.completed {
        card = card.line(Icon::CircleCheck, tr("Done"));
    }
    if let Some(due) = &todo.due {
        let date = match due {
            EventTime::Date(day) => datetime::format_short_date(*day, today),
            // A due time is an instant: its day and time, without an end.
            at => event::when(at, None, today),
        };
        card = card.line(Icon::CalendarClock, trf("Due {date}", &[("date", &date)]));
    }
    if let Some((level, word)) = priority(todo.priority) {
        card = card.line_priority(level, word);
    }
    with_actions(card, actions(todo))
}

/// The card, at most `room` wide.
pub(super) fn view<'a>(
    todo: &Todo,
    list: &str,
    color: Option<&str>,
    now: DateTime<Utc>,
    room: f32,
) -> Element<'a, Message> {
    content(todo, list, color, now).view(room)
}

/// The card in one line.
pub(super) fn line<'a>(todo: &Todo, list: &str, now: DateTime<Utc>) -> Element<'a, Message> {
    content(todo, list, None, now).short()
}
