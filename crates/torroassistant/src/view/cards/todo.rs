//! The task card in the assistant's chat (issue #797): due date, priority, list, with "Open in tasks" (`apps.open_app`
//! with `task:<uid>`) and "Done" (`tasks.save_todo` with `completed`).
//!
//! Strand 2g of issue #784 gives it its final parts and buttons; this draws the head and the title.

use iced::Element;
use torro_gui::content_card::{ContentCard, Head};
use torro_gui::icon::Icon;
use torro_pim::Todo;

use crate::app::Message;

/// The card of `todo` in the list `list` (its colour `color`).
pub(super) fn content(todo: &Todo, list: &str, color: Option<&str>) -> ContentCard<Message> {
    let _ = color;
    ContentCard::new(Head::new(Icon::ListTodo, list), todo.summary.clone())
}

/// The card, at most `room` wide.
pub(super) fn view<'a>(todo: &Todo, list: &str, color: Option<&str>, room: f32) -> Element<'a, Message> {
    content(todo, list, color).view(room)
}

/// The card in one line.
pub(super) fn line<'a>(todo: &Todo, list: &str) -> Element<'a, Message> {
    content(todo, list, None).short()
}
