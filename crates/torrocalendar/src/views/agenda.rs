//! Agenda: the coming events as a list (task K.11 in `docs/tasks/kalender-aufgaben.md`).
//!
//! Grouped by day from `start` to `end` (headings like `Heute`, `Morgen`, `Mo, 12.10.`),
//! only days with events; per event the time range or `ganztägig`, a colour dot, title and
//! location. Multi-day events appear on every day they cover. Click → `OpenEvent(index)`.
//! Fits a narrow tile: no fixed widths. Empty → a friendly empty state.

use iced::Element;
use iced::widget::text;

use super::ViewData;
use crate::app::Message;

pub fn view(data: ViewData<'_>) -> Element<'_, Message> {
    text(format!("agenda: {} Termine", data.items.len())).into()
}
