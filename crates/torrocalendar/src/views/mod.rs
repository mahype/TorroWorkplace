//! The calendar views. Each one only renders what [`ViewData`] holds and reports clicks
//! as [`crate::app::Message`]s: `NewEvent` on an empty slot or day, `OpenEvent(index)` on
//! an event, `ShowDay(date)` on a day number in the month view.
//!
//! [`glance`] holds the two layouts of a small tile (mini, strip), [`week_strip`] the row of the
//! week's days above the day view in the narrow layout.

pub mod agenda;
pub mod glance;
pub mod mini_month;
pub mod month;
pub mod radar;
pub mod timeline;
pub mod week;
pub mod week_strip;

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use iced::Color;
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::weekday_name;
use torro_pim::Event;

/// Short month names for [`short_date`], January first (translated with `tr`).
const MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `Sun, 11 Oct` (German `So, 11. Okt.`): the date where a long one does not fit – the narrow
/// header, the mini tile, the strip.
pub fn short_date(date: NaiveDate) -> String {
    let month = MONTHS_SHORT.get(date.month0() as usize).map(|name| tr(name)).unwrap_or_default();
    trf("{weekday}, {day} {month}", &[("weekday", &weekday_name(date, true)), ("day", &date.day()), ("month", &month)])
}

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
    /// Index into the app's event list of the event open in the side panel (U.4), `None` when
    /// none is. The grids outline it with the focus ring (`.ev.sel` in the mock-up), the agenda
    /// selects its row; views without room for that (the glance) ignore it.
    pub selected: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_date_names_weekday_day_and_month() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 11).expect("date");
        let expected = trf("{weekday}, {day} {month}", &[("weekday", &tr("Sun")), ("day", &11), ("month", &tr("Oct"))]);
        assert_eq!(short_date(date), expected);
        assert_eq!(MONTHS_SHORT[9], "Oct");
    }
}
