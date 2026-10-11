//! Agenda: the coming events as a list (task K.11 in `docs/tasks/kalender-aufgaben.md`).
//!
//! Grouped by day from `start` to `end`, only days with events, with headings `Today`,
//! `Tomorrow`, otherwise `Mo, 12.10.`. Per event: the time range (`09:30–10:15`) or `all day`
//! (`all day · day 2 of 3` for a multi-day event), a colour dot, title and location. Multi-day
//! events appear on every day they cover; a timed event crossing midnight shows `22:30–24:00`
//! on its first day and `00:00–00:30` on the next. All-day entries come first, then by start.
//! Past events of today are muted. Click on a row → `OpenEvent(index)`. Fits a narrow tile:
//! no fixed widths, text wraps. Empty → a friendly empty state.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::{column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Color, Element, Length, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_time, to_local, weekday_name};
use torro_gui::theme;
use torro_gui::widgets::{color_dot, empty_state, section_title};
use torro_pim::{Event, EventTime};

use super::{Item, ViewData};
use crate::app::Message;

/// The days of the agenda: its headings and rows, in display order.
#[derive(Debug, Clone)]
struct Day<'a> {
    date: NaiveDate,
    entries: Vec<Entry<'a>>,
}

/// One row: an event as it shows on one day.
#[derive(Debug, Clone)]
struct Entry<'a> {
    /// Index into the app's event list, for `OpenEvent`.
    index: usize,
    event: &'a Event,
    color: Color,
    /// `09:30–10:15`, `09:30`, `all day` or `all day · day 2 of 3`.
    time: String,
    /// Timed event of today that has already ended.
    past: bool,
    all_day: bool,
    /// Where the row sorts within its day: the start as shown on that day.
    start: NaiveDateTime,
}

pub fn view(data: ViewData<'_>) -> Element<'_, Message> {
    let days = group_by_day(&data.items, data.start, data.end, data.now);
    if days.is_empty() {
        return empty_state(tr("No events"), tr("Nothing planned in the next 30 days"));
    }
    let today = data.now.date();
    let mut list = column![].spacing(18).width(Length::Fill);
    for day in &days {
        let mut block = column![section_title(&day_heading(day.date, today))].spacing(6).width(Length::Fill);
        for entry in &day.entries {
            block = block.push(row_view(entry));
        }
        list = list.push(block);
    }
    scrollable(container(list).padding([8, 12]).width(Length::Fill)).width(Length::Fill).height(Length::Fill).into()
}

/// One event row: time, title (muted when past) and location below it, the colour dot beside
/// them. The whole row opens the event.
fn row_view<'a>(entry: &Entry<'a>) -> Element<'a, Message> {
    let event = entry.event;
    let title_color = if entry.past { theme::MUTED } else { theme::TEXT };
    let mut body = column![
        text(entry.time.clone()).size(13).color(theme::MUTED).width(Length::Fill),
        text(event.summary.as_str()).size(15).color(title_color).width(Length::Fill),
    ]
    .spacing(2)
    .width(Length::Fill);
    if let Some(location) = event.location.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        body = body.push(text(location).size(13).color(theme::MUTED).width(Length::Fill));
    }
    let line = row![container(color_dot(entry.color, 10.0)).padding([4, 0]), body]
        .spacing(10)
        .align_y(Alignment::Start)
        .width(Length::Fill);
    mouse_area(container(line).padding([6, 8]).width(Length::Fill))
        .on_press(Message::OpenEvent(entry.index))
        .interaction(mouse::Interaction::Pointer)
        .into()
}

/// `Today`, `Tomorrow`, otherwise `Mo, 12.10.` (short weekday name from `weekday_name`).
fn day_heading(day: NaiveDate, today: NaiveDate) -> String {
    match (day - today).num_days() {
        0 => tr("Today").to_string(),
        1 => tr("Tomorrow").to_string(),
        _ => format!("{}, {}", weekday_name(day, true), day.format("%d.%m.")),
    }
}

/// The days from `start` to `end` (exclusive) that have events, each with its rows in
/// display order. An event appears on every day of its span that falls into the window.
/// `now` marks the events of today that have already ended.
fn group_by_day<'a>(items: &[Item<'a>], start: NaiveDate, end: NaiveDate, now: NaiveDateTime) -> Vec<Day<'a>> {
    let window_last = end - Duration::days(1);
    let mut by_day: BTreeMap<NaiveDate, Vec<Entry<'a>>> = BTreeMap::new();
    for item in items {
        let (first, last) = covered_days(item.event);
        let until = last.min(window_last);
        let mut day = first.max(start);
        while day <= until {
            by_day.entry(day).or_default().push(entry(item, day, now));
            day += Duration::days(1);
        }
    }
    by_day
        .into_iter()
        .map(|(date, mut entries)| {
            entries.sort_by(compare);
            Day { date, entries }
        })
        .collect()
}

/// The row of `item` on `day`, which must be one of its covered days.
fn entry<'a>(item: &Item<'a>, day: NaiveDate, now: NaiveDateTime) -> Entry<'a> {
    let event = item.event;
    let all_day = event.all_day();
    let (first, _) = covered_days(event);
    let start = if all_day {
        first.and_time(NaiveTime::MIN)
    } else if day == first {
        local_at(event.start)
    } else {
        day.and_time(NaiveTime::MIN)
    };
    let ended_at = event.end.map_or_else(|| local_at(event.start), local_at);
    let past = !all_day && day == now.date() && ended_at <= now;
    Entry { index: item.index, event, color: item.color, time: time_label(event, day), past, all_day, start }
}

/// All-day entries first, then by start, then by title and index.
fn compare(a: &Entry<'_>, b: &Entry<'_>) -> Ordering {
    b.all_day
        .cmp(&a.all_day)
        .then_with(|| a.start.cmp(&b.start))
        .then_with(|| a.event.summary.to_lowercase().cmp(&b.event.summary.to_lowercase()))
        .then_with(|| a.index.cmp(&b.index))
}

/// The label of `event` on `day`: the time range within that day, or `all day` with the
/// position in the span for a multi-day all-day event.
fn time_label(event: &Event, day: NaiveDate) -> String {
    let (first, last) = covered_days(event);
    if event.all_day() {
        let total = (last - first).num_days() + 1;
        if total <= 1 {
            return tr("all day").to_string();
        }
        let number = (day - first).num_days() + 1;
        return trf("all day · day {n} of {total}", &[("n", &number), ("total", &total)]);
    }

    let start = local_at(event.start);
    let from = if day == first { start.time() } else { NaiveTime::MIN };
    // An end at exactly midnight belongs to the day before; anything later runs to the end of `day`.
    match event.end.map(local_at).filter(|end| *end > start) {
        None => format_time(from),
        Some(end) => {
            let to = if end.date() > day { "24:00".to_string() } else { format_time(end.time()) };
            format!("{}–{to}", format_time(from))
        }
    }
}

/// The first and the last local day (inclusive) an event covers. An all-day event's end is
/// exclusive. A timed event that ends at midnight does not reach into the day it ends on.
fn covered_days(event: &Event) -> (NaiveDate, NaiveDate) {
    let start = local_at(event.start);
    let first = start.date();
    let Some(end) = event.end.map(local_at) else {
        return (first, first);
    };
    let last = if event.all_day() {
        end.date() - Duration::days(1)
    } else if end <= start {
        first
    } else if end.time() == NaiveTime::MIN {
        end.date() - Duration::days(1)
    } else {
        end.date()
    };
    (first, last.max(first))
}

/// An event time as local time; all-day dates count as the start of their day.
fn local_at(time: EventTime) -> NaiveDateTime {
    match time {
        EventTime::Date(day) => day.and_time(NaiveTime::MIN),
        EventTime::DateTime(at) => to_local(at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use torro_gui::datetime::to_utc;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).expect("valid date")
    }

    fn local(d: u32, h: u32, m: u32) -> NaiveDateTime {
        day(d).and_hms_opt(h, m, 0).expect("valid time")
    }

    /// A timed instant on October `d` at `h:m`, local time.
    fn at(d: u32, h: u32, m: u32) -> EventTime {
        EventTime::DateTime(to_utc(local(d, h, m)))
    }

    /// An all-day date in October.
    fn date(d: u32) -> EventTime {
        EventTime::Date(day(d))
    }

    fn event(summary: &str, start: EventTime, end: Option<EventTime>) -> Event {
        Event {
            uid: summary.into(),
            collection: "home".into(),
            href: None,
            etag: None,
            summary: summary.into(),
            start,
            end,
            location: None,
            description: None,
            recurring: false,
        }
    }

    /// Items for `events`, indexed by position, all in one colour.
    fn items(events: &[Event]) -> Vec<Item<'_>> {
        events.iter().enumerate().map(|(index, event)| Item { index, event, color: theme::TEXT }).collect()
    }

    /// The agenda's grouping as `(day of month, [(time label, summary)])`.
    fn grouped(events: &[Event], start: u32, end: u32, now: NaiveDateTime) -> Vec<(u32, Vec<(String, String)>)> {
        let items = items(events);
        group_by_day(&items, day(start), day(end), now)
            .into_iter()
            .map(|d| {
                let rows = d.entries.iter().map(|e| (e.time.clone(), e.event.summary.clone())).collect();
                (d.date.day(), rows)
            })
            .collect()
    }

    fn line(time: &str, summary: &str) -> (String, String) {
        (time.to_string(), summary.to_string())
    }

    #[test]
    fn timed_event_crossing_midnight_appears_on_both_days() {
        let events = [event("late", at(11, 22, 30), Some(at(12, 0, 30)))];
        let now = local(10, 8, 0);
        assert_eq!(
            grouped(&events, 11, 13, now),
            [(11, vec![line("22:30–24:00", "late")]), (12, vec![line("00:00–00:30", "late")])]
        );
    }

    #[test]
    fn timed_event_ending_at_midnight_stays_on_its_day() {
        let events = [event("evening", at(11, 22, 0), Some(at(12, 0, 0)))];
        assert_eq!(grouped(&events, 11, 13, local(10, 8, 0)), [(11, vec![line("22:00–24:00", "evening")])]);
    }

    #[test]
    fn timed_event_spanning_three_days_labels_the_middle_day_fully() {
        let events = [event("trip", at(11, 22, 0), Some(at(13, 10, 0)))];
        assert_eq!(
            grouped(&events, 11, 14, local(10, 8, 0)),
            [
                (11, vec![line("22:00–24:00", "trip")]),
                (12, vec![line("00:00–24:00", "trip")]),
                (13, vec![line("00:00–10:00", "trip")]),
            ]
        );
    }

    #[test]
    fn timed_event_without_end_shows_its_start() {
        let events = [event("call", at(11, 9, 30), None)];
        assert_eq!(grouped(&events, 11, 12, local(10, 8, 0)), [(11, vec![line("09:30", "call")])]);
    }

    #[test]
    fn all_day_event_uses_an_exclusive_end() {
        let events = [event("holiday", date(12), Some(date(13)))];
        assert_eq!(grouped(&events, 11, 14, local(10, 8, 0)), [(12, vec![line("all day", "holiday")])]);
    }

    #[test]
    fn multi_day_all_day_event_appears_on_every_covered_day_with_position() {
        let events = [event("course", date(11), Some(date(14)))];
        assert_eq!(
            grouped(&events, 11, 20, local(10, 8, 0)),
            [
                (11, vec![line("all day · day 1 of 3", "course")]),
                (12, vec![line("all day · day 2 of 3", "course")]),
                (13, vec![line("all day · day 3 of 3", "course")]),
            ]
        );
    }

    #[test]
    fn position_counts_the_whole_span_even_when_the_window_starts_later() {
        let events = [event("course", date(11), Some(date(14)))];
        assert_eq!(grouped(&events, 12, 13, local(10, 8, 0)), [(12, vec![line("all day · day 2 of 3", "course")])]);
    }

    #[test]
    fn window_clips_events_and_leaves_out_empty_days() {
        let events = [
            event("before", at(9, 10, 0), Some(at(9, 11, 0))),
            event("inside", at(12, 10, 0), Some(at(12, 11, 0))),
            event("after", at(20, 10, 0), Some(at(20, 11, 0))),
        ];
        assert_eq!(grouped(&events, 11, 13, local(10, 8, 0)), [(12, vec![line("10:00–11:00", "inside")])]);
        assert!(grouped(&events, 11, 11, local(10, 8, 0)).is_empty());
    }

    #[test]
    fn all_day_entries_come_first_then_by_start() {
        let events = [
            event("timed late", at(11, 15, 0), None),
            event("timed early", at(11, 9, 0), None),
            event("all day", date(11), Some(date(12))),
        ];
        assert_eq!(
            grouped(&events, 11, 12, local(10, 8, 0)),
            [(11, vec![line("all day", "all day"), line("09:00", "timed early"), line("15:00", "timed late")])]
        );
    }

    #[test]
    fn same_start_is_sorted_by_title_and_all_day_by_start_date() {
        let events = [
            event("Zeta", at(11, 9, 0), None),
            event("alpha", at(11, 9, 0), None),
            event("long", date(10), Some(date(13))),
            event("short", date(11), Some(date(12))),
        ];
        let result = grouped(&events, 11, 12, local(10, 8, 0));
        let titles: Vec<&str> = result[0].1.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(titles, ["long", "short", "alpha", "Zeta"]);
    }

    #[test]
    fn continuation_of_yesterdays_event_comes_before_todays_timed_events() {
        let events = [event("morning", at(12, 7, 0), None), event("night", at(11, 23, 0), Some(at(12, 1, 0)))];
        assert_eq!(
            grouped(&events, 12, 13, local(10, 8, 0)),
            [(12, vec![line("00:00–01:00", "night"), line("07:00", "morning")])]
        );
    }

    #[test]
    fn today_past_events_are_marked() {
        let events = [
            event("ended", at(11, 9, 0), Some(at(11, 9, 30))),
            event("running", at(11, 9, 30), Some(at(11, 11, 0))),
            event("tomorrow", at(12, 9, 0), Some(at(12, 9, 30))),
            event("all day", date(11), None),
        ];
        let items = items(&events);
        let now = local(11, 10, 0);
        let days = group_by_day(&items, day(11), day(13), now);
        let flags = |d: usize| -> Vec<(String, bool)> {
            days[d].entries.iter().map(|e| (e.event.summary.clone(), e.past)).collect()
        };
        assert_eq!(
            flags(0),
            [("all day".to_string(), false), ("ended".to_string(), true), ("running".to_string(), false),]
        );
        assert_eq!(flags(1), [("tomorrow".to_string(), false)]);
    }

    #[test]
    fn day_headings() {
        let today = day(11);
        assert_eq!(day_heading(day(11), today), "Today");
        assert_eq!(day_heading(day(12), today), "Tomorrow");
        assert_eq!(day_heading(day(14), today), "Mi, 14.10.");
        assert_eq!(day_heading(day(12), day(11)), "Tomorrow");
        assert_eq!(day_heading(day(20), day(11)), "Di, 20.10.");
    }

    #[test]
    fn view_builds_with_and_without_events() {
        let _ = view(ViewData {
            items: Vec::new(),
            start: day(11),
            end: day(11) + Duration::days(30),
            anchor: day(11),
            now: local(11, 8, 0),
        });
        let events = [event("meeting", at(11, 9, 0), Some(at(11, 10, 0)))];
        let _ = view(ViewData {
            items: items(&events),
            start: day(11),
            end: day(11) + Duration::days(30),
            anchor: day(11),
            now: local(11, 8, 0),
        });
    }
}
