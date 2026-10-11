//! Day and week view: a time grid (task K.9 in `docs/tasks/kalender-aufgaben.md`).
//!
//! Columns for the days in `[start, end)` (one for the day view, seven for the week),
//! hours 0–24 down the side (scrolled so 07:00 is on top), a row for all-day events above
//! the grid, timed events as coloured blocks (overlapping ones side by side), a line at
//! `now` in today's column. Click on an empty slot → `NewEvent { date, time }` (time
//! rounded down to the half hour), on an all-day row cell → `NewEvent { date, time: None }`,
//! on an event → `OpenEvent(index)`.

use iced::Element;
use iced::widget::text;

use super::ViewData;
use crate::app::Message;

pub fn view(data: ViewData<'_>) -> Element<'_, Message> {
    text(format!("week: {} Termine", data.items.len())).into()
}
