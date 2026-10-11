//! The calendar views. Each one only renders what [`ViewData`] holds and reports clicks
//! as [`crate::app::Message`]s: `NewEvent` on an empty slot or day, `OpenEvent(index)` on
//! an event, `ShowDay(date)` on a day number in the month view.

pub mod agenda;
pub mod month;
pub mod week;

use chrono::{NaiveDate, NaiveDateTime};
use iced::Color;
use torro_pim::Event;

/// One visible event.
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    /// Index into the app's event list; send it back with `Message::OpenEvent`.
    pub index: usize,
    pub event: &'a Event,
    /// The colour of its calendar.
    pub color: Color,
}

/// What a view gets to draw.
#[derive(Debug, Clone)]
pub struct ViewData<'a> {
    /// The events of the shown calendars overlapping `[start, end)`, unsorted.
    pub items: Vec<Item<'a>>,
    /// First day shown (local).
    pub start: NaiveDate,
    /// Day after the last one shown (local).
    pub end: NaiveDate,
    /// The date the user navigated to (the day view's day; inside the month for month).
    pub anchor: NaiveDate,
    /// Local now, for the "now" line and highlighting today.
    pub now: NaiveDateTime,
}
