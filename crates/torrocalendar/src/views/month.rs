//! Month view: six weeks of day cells (task K.10 in `docs/tasks/kalender-aufgaben.md`).
//!
//! A weekday header (Mo–Su) above six rows of seven cells, starting at `data.start` (the
//! Monday on or before the 1st of the month). Days outside the anchor's month are dimmed,
//! today's number is in Torro red. Each cell lists its events, all-day first, then by start
//! (`14:00 Title`), at most four lines; with more, three events and `+n more`. A timed event
//! that continues from an earlier day shows as `… Title` on the following days. Multi-day
//! and all-day events appear on every day they cover. Click on a day number → `ShowDay(date)`,
//! on an event → `OpenEvent(index)`, on empty space in a cell → `NewEvent { date, time: None }`.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, border, mouse};
use torro_core::i18n::{tr, trf};
use torro_core::theme::TORRO_RED;
use torro_gui::datetime::{format_time, to_local, weekday_name};
use torro_gui::theme;
use torro_gui::widgets::color_dot;
use torro_pim::{Event, EventTime};

use super::{Item, ViewData};
use crate::app::Message;

/// Lines per cell, the `+n more` line included.
const MAX_LINES: usize = 4;
/// Height of one event row; longer titles are clipped, not wrapped.
const EVENT_HEIGHT: f32 = 17.0;

pub fn view(data: ViewData<'_>) -> Element<'_, Message> {
    let today = data.now.date();
    let mut weeks = column![header(data.start)].height(Length::Fill);
    for week in 0..6i64 {
        let mut cells = row![].height(Length::FillPortion(1));
        for day in 0..7i64 {
            let date = data.start + Duration::days(week * 7 + day);
            cells = cells.push(cell(date, data.anchor, today, &data.items));
        }
        weeks = weeks.push(cells);
    }
    weeks.into()
}

/// Mo–Su, short names, from `start` on.
fn header<'a>(start: NaiveDate) -> Element<'a, Message> {
    let mut names = row![].width(Length::Fill);
    for offset in 0..7 {
        let name = weekday_name(start + Duration::days(offset), true);
        names = names.push(
            container(text(name).size(12).color(theme::MUTED))
                .padding([4, 6])
                .width(Length::FillPortion(1))
                .center_x(Length::FillPortion(1)),
        );
    }
    container(names).style(|_| container::Style { border: line_border(), ..Default::default() }).into()
}

/// One day: the day number, then its events as far as they fit. Empty space in the cell
/// creates an event on that day.
fn cell<'a>(date: NaiveDate, anchor: NaiveDate, today: NaiveDate, items: &[Item<'a>]) -> Element<'a, Message> {
    let in_month = date.year() == anchor.year() && date.month() == anchor.month();
    let number_color = if date == today {
        theme::color(TORRO_RED)
    } else if in_month {
        theme::TEXT
    } else {
        theme::FAINT
    };
    let text_color = if in_month { theme::TEXT } else { theme::MUTED };
    let background = if in_month { theme::SURFACE } else { theme::BACKGROUND };

    let day = day_items(items, date);
    let (shown, more) = visible_split(day.len());
    let mut lines = column![].spacing(2);
    for item in &day[..shown] {
        lines = lines.push(event_row(*item, date, text_color));
    }
    if more > 0 {
        let summary = trf("{n} more", &[("n", &more)]);
        lines = lines.push(text(format!("+{summary}")).size(11).color(theme::MUTED));
    }

    let number = button(text(date.day().to_string()).size(13))
        .padding([2, 6])
        .style(move |_, _| button::Style { text_color: number_color, ..Default::default() })
        .on_press(Message::ShowDay(date));

    let cell = container(column![number, lines].spacing(2))
        .padding(4)
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(background)),
            border: line_border(),
            ..Default::default()
        });
    mouse_area(cell).on_press(Message::NewEvent { date, time: None }).into()
}

/// One event line in a cell: all-day entries as a tinted bar, timed ones with a colour dot.
fn event_row<'a>(item: Item<'a>, date: NaiveDate, text_color: Color) -> Element<'a, Message> {
    let label = entry_label(item.event, date);
    let body: Element<'a, Message> = if item.event.all_day() {
        let tint = Color { a: 0.35, ..item.color };
        container(text(label).size(12).color(text_color).wrapping(Wrapping::None))
            .padding([1, 4])
            .width(Length::Fill)
            .clip(true)
            .style(move |_| container::Style {
                background: Some(Background::Color(tint)),
                border: border::rounded(3.0),
                ..Default::default()
            })
            .into()
    } else {
        row![color_dot(item.color, 6.0), text(label).size(12).color(text_color).wrapping(Wrapping::None)]
            .spacing(4)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
    };
    mouse_area(container(body).width(Length::Fill).height(Length::Fixed(EVENT_HEIGHT)).clip(true))
        .on_press(Message::OpenEvent(item.index))
        .interaction(mouse::Interaction::Pointer)
        .into()
}

fn line_border() -> Border {
    Border { color: theme::LINE, width: 1.0, ..Default::default() }
}

/// Local start and exclusive end of an event. All-day entries without an end cover one day;
/// timed entries without an end are instants.
fn local_span(event: &Event) -> (NaiveDateTime, NaiveDateTime) {
    let at = |time: EventTime| match time {
        EventTime::Date(day) => day.and_time(NaiveTime::MIN),
        EventTime::DateTime(instant) => to_local(instant),
    };
    let start = at(event.start);
    let end = match event.end {
        Some(end) => at(end),
        None if event.all_day() => start + Duration::days(1),
        None => start,
    };
    (start, end)
}

/// The local days `[first, last)` that the span `[start, end)` touches. A span ending exactly
/// at midnight does not touch the day that starts there; an empty span touches its start day.
fn covered_days(start: NaiveDateTime, end: NaiveDateTime) -> (NaiveDate, NaiveDate) {
    let first = start.date();
    if end <= start {
        return (first, first + Duration::days(1));
    }
    let last = if end.time() == NaiveTime::MIN { end.date() } else { end.date() + Duration::days(1) };
    (first, last.max(first + Duration::days(1)))
}

/// Whether `event` is shown on the local day `date`.
fn on_day(event: &Event, date: NaiveDate) -> bool {
    let (start, end) = local_span(event);
    let (first, last) = covered_days(start, end);
    first <= date && date < last
}

/// Sort key within a day: all-day entries first (rank 0), then timed ones by the time they
/// start on that day (midnight for entries that began earlier).
fn sort_key(event: &Event, date: NaiveDate) -> (u8, NaiveTime) {
    if event.all_day() {
        return (0, NaiveTime::MIN);
    }
    let (start, _) = local_span(event);
    (1, if start.date() == date { start.time() } else { NaiveTime::MIN })
}

/// The items on `date` in display order: all-day first, then by start, then by title.
fn day_items<'a>(items: &[Item<'a>], date: NaiveDate) -> Vec<Item<'a>> {
    let mut day: Vec<Item<'a>> = items.iter().copied().filter(|item| on_day(item.event, date)).collect();
    day.sort_by(|a, b| {
        sort_key(a.event, date)
            .cmp(&sort_key(b.event, date))
            .then_with(|| a.event.summary.cmp(&b.event.summary))
            .then_with(|| a.index.cmp(&b.index))
    });
    day
}

/// The text of an event row on `date`: the title for all-day entries, `14:00 Title` where the
/// entry starts, `… Title` where it continues from an earlier day.
fn entry_label(event: &Event, date: NaiveDate) -> String {
    let title = if event.summary.trim().is_empty() { tr("(no title)").to_string() } else { event.summary.clone() };
    if event.all_day() {
        return title;
    }
    let (start, _) = local_span(event);
    if start.date() == date { format!("{} {title}", format_time(start.time())) } else { format!("… {title}") }
}

/// How many of `total` entries a cell lists, and how many it sums up as `+n more`. The summary
/// line counts towards [`MAX_LINES`].
fn visible_split(total: usize) -> (usize, usize) {
    if total <= MAX_LINES { (total, 0) } else { (MAX_LINES - 1, total - (MAX_LINES - 1)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::ViewData;
    use chrono::NaiveDateTime;
    use torro_gui::datetime::to_utc;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    /// A local instant on October `day` at `hour`, as the stored UTC value.
    fn at(day: u32, hour: u32) -> EventTime {
        EventTime::DateTime(to_utc(d(day).and_hms_opt(hour, 0, 0).unwrap()))
    }

    fn event(summary: &str, start: EventTime, end: Option<EventTime>) -> Event {
        Event {
            uid: summary.to_string(),
            collection: "work".to_string(),
            href: None,
            etag: None,
            summary: summary.to_string(),
            start,
            end,
            location: None,
            description: None,
            recurring: false,
        }
    }

    fn all_day(summary: &str, first: u32, end: Option<u32>) -> Event {
        event(summary, EventTime::Date(d(first)), end.map(|day| EventTime::Date(d(day))))
    }

    fn items(events: &[Event]) -> Vec<Item<'_>> {
        events.iter().enumerate().map(|(index, event)| Item { index, event, color: Color::WHITE }).collect()
    }

    fn titles<'a>(day: &[Item<'a>]) -> Vec<&'a str> {
        day.iter().map(|item| item.event.summary.as_str()).collect()
    }

    #[test]
    fn all_day_span_is_end_exclusive() {
        let holiday = all_day("Holiday", 12, Some(15));
        let days: Vec<u32> = (10..=16).filter(|&day| on_day(&holiday, d(day))).collect();
        assert_eq!(days, vec![12, 13, 14]);
    }

    #[test]
    fn single_all_day_without_end_covers_its_day() {
        let birthday = all_day("Birthday", 12, None);
        assert!(on_day(&birthday, d(12)));
        assert!(!on_day(&birthday, d(11)) && !on_day(&birthday, d(13)));
    }

    #[test]
    fn timed_span_crossing_midnight_is_on_both_days() {
        let night = event("Night shift", at(11, 22), Some(at(12, 1)));
        assert!(on_day(&night, d(11)));
        assert!(on_day(&night, d(12)));
        assert!(!on_day(&night, d(13)));
    }

    #[test]
    fn timed_span_ending_at_midnight_stays_on_its_day() {
        let late = event("Late", at(11, 22), Some(at(12, 0)));
        assert!(on_day(&late, d(11)));
        assert!(!on_day(&late, d(12)));
    }

    #[test]
    fn timed_event_without_end_is_on_its_start_day_only() {
        let instant = event("Call", at(11, 9), None);
        assert!(on_day(&instant, d(11)));
        assert!(!on_day(&instant, d(12)));
    }

    #[test]
    fn day_lists_all_day_first_then_by_start_then_title() {
        let events = [
            event("Standup", at(11, 9), Some(at(11, 9))),
            all_day("Holiday", 11, Some(12)),
            event("Review", at(11, 14), None),
            event("Alpha", at(11, 14), None),
            event("Earlier", at(10, 23), Some(at(11, 1))),
            event("Tomorrow", at(12, 8), None),
        ];
        let list = items(&events);
        let day = day_items(&list, d(11));
        assert_eq!(titles(&day), vec!["Holiday", "Earlier", "Standup", "Alpha", "Review"]);
    }

    #[test]
    fn labels_show_start_time_or_continuation() {
        let review = event("Review", at(11, 14), None);
        assert_eq!(entry_label(&review, d(11)), "14:00 Review");
        assert_eq!(entry_label(&review, d(12)), "… Review");
        let holiday = all_day("Holiday", 11, None);
        assert_eq!(entry_label(&holiday, d(11)), "Holiday");
    }

    #[test]
    fn overflow_keeps_four_lines_including_the_summary() {
        assert_eq!(visible_split(0), (0, 0));
        assert_eq!(visible_split(4), (4, 0));
        assert_eq!(visible_split(5), (3, 2));
        assert_eq!(visible_split(9), (3, 6));
    }

    #[test]
    fn view_builds_for_an_empty_and_a_busy_month() {
        let busy = [all_day("Holiday", 12, Some(15)), event("Review", at(13, 14), None)];
        let list = items(&busy);
        let now = NaiveDateTime::new(d(11), NaiveTime::from_hms_opt(10, 0, 0).unwrap());
        for items in [Vec::new(), list] {
            let data = ViewData { items, start: d(5), end: d(5) + Duration::days(42), anchor: d(11), now };
            let _: Element<'_, Message> = view(data);
        }
    }
}
