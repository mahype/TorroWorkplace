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
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, rule, scrollable};
use iced::{Alignment, Background, Color, Element, Length, Padding};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_time, to_local, weekday_name};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, color_dot, empty_state};
use torro_gui::{style, typo};
use torro_pim::{Event, EventTime};

use super::{Item, ViewData};
use crate::app::Message;

/// Width of the time column (`docs/tasks/design.md`, C.4): `09:30` over `10:15` in `META`.
const TIME_WIDTH: f32 = 56.0;
/// One logical pixel: the lines of a day head, as in `widgets::group_head`.
const LINE: f32 = 1.0;
/// Opacity of the colour dot of an event that has already ended (`torro-design`, 55 %).
const PAST_ALPHA: f32 = 0.55;
/// Separates the parts of a time label that the view shows apart: the range `09:30–10:15` and
/// `all day · day 2 of 3`.
const RANGE_DASH: char = '–';
const LABEL_SEPARATOR: &str = " · ";

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
    let mut list = column![].width(Length::Fill);
    for day in &days {
        let (label, date) = day_heading(day.date, today);
        list = list.push(day_head(label, date));
        for entry in &day.entries {
            list = list.push(row_view(entry));
        }
    }
    // Room under the last row so it does not touch the edge of the panel.
    let list = container(list).padding(Padding::new(0.0).bottom(space::L));
    scrollable(widgets::content(size::CONTENT_MAX, list))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The head of a day, after `widgets::group_head` (which has no slot for a date at the right):
/// 32 high on `raised` with lines above and below, the day in `CAPTION` / `text_2`, its date in
/// `META` tabular / `text_3`.
fn day_head<'a>(label: String, date: String) -> Element<'a, Message> {
    let content = row![
        typo::text(typo::CAPTION, label).style(theme::text_2),
        Space::new().width(Length::Fill),
        typo::text(typo::META.tabular(), date).style(theme::text_3),
    ]
    .align_y(Alignment::Center);
    let head = container(content)
        .width(Length::Fill)
        .center_y(size::ROW_GROUP - 2.0 * LINE)
        .padding(Padding::new(0.0).left(space::L).right(space::L))
        .style(|t| container::Style {
            background: Some(Background::Color(palette(t).raised)),
            ..container::Style::default()
        });
    column![
        Space::new().height(space::XS),
        rule::horizontal(LINE).style(style::rule),
        head,
        rule::horizontal(LINE).style(style::rule)
    ]
    .into()
}

/// One event row (`widgets::list_row`, so hover and focus come from the shared style): the time
/// in a column of its own, then the colour dot, the title and under it the location. A click
/// opens the event. An event that has ended today is muted.
fn row_view<'a>(entry: &Entry<'a>) -> Element<'a, Message> {
    let event = entry.event;
    let (first, second) = split_label(&entry.time, entry.all_day);
    let text_colour: fn(&iced::Theme) -> iced::widget::text::Style =
        if entry.past { theme::text_3 } else { theme::text };
    let time_colour: fn(&iced::Theme) -> iced::widget::text::Style =
        if entry.past { theme::text_3 } else { theme::text_2 };

    // The first line of every column sits in a box as high as a `BODY` line, so time, dot and
    // title share one centre line whatever their font size.
    let line = typo::BODY.line_height.to_absolute(typo::BODY.size);
    let mut time = column![
        container(typo::text(typo::META.tabular(), first).style(time_colour).wrapping(Wrapping::None))
            .height(line)
            .align_y(Alignment::Center)
    ];
    // The end of a range under its start. Multi-day all-day entries show their position under
    // the title instead; it is too wide for this column.
    let mut note = None;
    match second {
        Some(second) if !entry.all_day => {
            time = time.push(typo::text(typo::META.tabular(), second).style(theme::text_3).wrapping(Wrapping::None));
        }
        other => note = other,
    }
    let time = container(time).width(TIME_WIDTH).clip(true);

    let dot_colour = if entry.past { Color { a: PAST_ALPHA, ..entry.color } } else { entry.color };
    let dot = container(color_dot(dot_colour, space::S)).height(line).align_y(Alignment::Center);

    let title = typo::text(typo::BODY_STRONG, event.summary.as_str()).style(text_colour).wrapping(Wrapping::None);
    let mut body = column![title].width(Length::Fill);
    let location = event.location.as_deref().map(str::trim).filter(|l| !l.is_empty());
    let detail = [note.as_deref(), location].into_iter().flatten().collect::<Vec<_>>().join(LABEL_SEPARATOR);
    if !detail.is_empty() {
        body = body.push(typo::text(typo::META, detail).style(theme::text_3).wrapping(Wrapping::None));
    }
    let main = row![dot, body].spacing(space::S).align_y(Alignment::Start).width(Length::Fill);

    widgets::list_row(
        vec![time.into(), container(main).width(Length::Fill).clip(true).into()],
        false,
        Some(Message::OpenEvent(entry.index)),
    )
}

/// Splits a time label for the two lines of the time column: `09:30–10:15` → `09:30`, `10:15`;
/// `all day · day 2 of 3` → `all day`, `day 2 of 3`; anything without a separator stays whole.
fn split_label(label: &str, all_day: bool) -> (String, Option<String>) {
    let split = if all_day { label.split_once(LABEL_SEPARATOR) } else { label.split_once(RANGE_DASH) };
    match split {
        Some((first, second)) => (first.to_string(), Some(second.to_string())),
        None => (label.to_string(), None),
    }
}

/// The head of a day as `(label, date)`: `Today` / `Tomorrow` with `Mon, 12.10.`, otherwise the
/// weekday with `12.10.` (weekday names from `weekday_name`).
fn day_heading(day: NaiveDate, today: NaiveDate) -> (String, String) {
    let date = day.format("%d.%m.");
    match (day - today).num_days() {
        0 => (tr("Today").to_string(), format!("{}, {date}", weekday_name(day, true))),
        1 => (tr("Tomorrow").to_string(), format!("{}, {date}", weekday_name(day, true))),
        _ => (weekday_name(day, false).to_string(), date.to_string()),
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
        assert_eq!(grouped(&events, 11, 14, local(10, 8, 0)), [(12, vec![line(tr("all day"), "holiday")])]);
    }

    #[test]
    fn multi_day_all_day_event_appears_on_every_covered_day_with_position() {
        let events = [event("course", date(11), Some(date(14)))];
        assert_eq!(
            grouped(&events, 11, 20, local(10, 8, 0)),
            [
                (11, vec![line(trf("all day · day {n} of {total}", &[("n", &1), ("total", &3)]).as_str(), "course")]),
                (12, vec![line(trf("all day · day {n} of {total}", &[("n", &2), ("total", &3)]).as_str(), "course")]),
                (13, vec![line(trf("all day · day {n} of {total}", &[("n", &3), ("total", &3)]).as_str(), "course")]),
            ]
        );
    }

    #[test]
    fn position_counts_the_whole_span_even_when_the_window_starts_later() {
        let events = [event("course", date(11), Some(date(14)))];
        assert_eq!(
            grouped(&events, 12, 13, local(10, 8, 0)),
            [(12, vec![line(trf("all day · day {n} of {total}", &[("n", &2), ("total", &3)]).as_str(), "course")])]
        );
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
            [(11, vec![line(tr("all day"), "all day"), line("09:00", "timed early"), line("15:00", "timed late")])]
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
        let heading = |d: u32| day_heading(day(d), today);
        assert_eq!(heading(11), (tr("Today").to_string(), format!("{}, 11.10.", tr("Sun"))));
        assert_eq!(heading(12), (tr("Tomorrow").to_string(), format!("{}, 12.10.", tr("Mon"))));
        assert_eq!(heading(14), (tr("Wednesday").to_string(), "14.10.".to_string()));
        assert_eq!(day_heading(day(20), day(11)), (tr("Tuesday").to_string(), "20.10.".to_string()));
    }

    #[test]
    fn time_labels_split_for_the_two_columns() {
        assert_eq!(split_label("09:30–10:15", false), ("09:30".to_string(), Some("10:15".to_string())));
        assert_eq!(split_label("09:30", false), ("09:30".to_string(), None));
        assert_eq!(split_label("all day", true), ("all day".to_string(), None));
        let multi = trf("all day · day {n} of {total}", &[("n", &2), ("total", &3)]);
        let (first, second) = split_label(&multi, true);
        assert_eq!(first, tr("all day"));
        // Whatever the language, the position is the part after the separator.
        assert!(second.is_some_and(|rest| !rest.is_empty() && !rest.contains(LABEL_SEPARATOR)));
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
