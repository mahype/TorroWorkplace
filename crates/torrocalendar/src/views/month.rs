//! Month view: six weeks of day cells (task K.10 in `docs/tasks/kalender-aufgaben.md`).
//!
//! Weekday header Mo–So, 42 cells from `start`; days outside the anchor's month dimmed,
//! today highlighted. Each cell lists its events (all-day first, then by time, `14:00
//! Titel`) as far as they fit, then `+n weitere`. Click on the day number → `ShowDay(date)`,
//! on an event → `OpenEvent(index)`, on empty space in the cell → `NewEvent { date, time:
//! None }`.

use iced::Element;
use iced::widget::text;

use super::ViewData;
use crate::app::Message;

pub fn view(data: ViewData<'_>) -> Element<'_, Message> {
    text(format!("month: {} Termine", data.items.len())).into()
}
