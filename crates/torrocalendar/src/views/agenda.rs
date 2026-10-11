//! Agenda: the coming events as a list (K.11, redesigned in K.24 after
//! `docs/prototypes/calendar/agenda.html`).
//!
//! The list is planned first ([`plan`], plain data, tested) and drawn afterwards:
//!
//! - **Days** with a date column at the left (weekday over the day number; today in a red circle,
//!   weekends quieter) and a quiet head ("Today", "Tomorrow" or a new month at the left, "4 events"
//!   at the right). Days are separated by lines. Iced's `scrollable` cannot pin the date column
//!   while its day scrolls past, so it stands at the top of its day.
//! - **All-day events** as tinted chips at the top of the day, multi-day ones with "day 2 of 3".
//! - **Timed events** as rows: start over duration, a bar in the calendar colour, title over the
//!   place (map pin, or a camera for a link or a video call). Hover `hover`, the event open in the
//!   side panel `selected`, ended ones at 55 %. The running event is tinted and says "40 min left"
//!   (not in the narrow layout, where the row has no room for it); after it the red now line.
//! - **Free time** of two hours or more between two events of a day: "3 h free" with a dotted line;
//!   on today only what is still ahead.
//! - **Empty days** between days with events as one line per week, "3 d · Nothing planned ·
//!   Tue 20 – Thu 22"; today always shows, even without events.
//! - **Calendar weeks** as quiet separators ("This week · Week 42", "Week 43").
//!
//! A click on a row or chip sends `OpenEvent(index)`. No events at all → a friendly empty state.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use iced::border::{self, Radius};
use iced::mouse;
use iced::widget::canvas::{self, Frame, LineDash, Path, Stroke};
use iced::widget::text::{Style as TextStyle, Wrapping};
use iced::widget::{Space, button, canvas as canvas_widget, column, container, mouse_area, responsive, row, rule, svg};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_time, month_name, to_local, week_start, weekday_name};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::theme::{self, Palette, over, palette, radius, size, space};
use torro_gui::typo::{ellipsize, text_width};
use torro_gui::widgets::{self, empty_state};
use torro_gui::{style, typo};
use torro_pim::{Event, EventTime};

use super::glance::span;
use super::week::{BAR, FREE_MINUTES, NOW_LABEL, NOW_LABEL_WIDTH};
use super::{Item, ViewData};
use crate::app::Message;

/// Width of the date column (`.day{grid-template-columns:56px 1fr}`): the time axis and a little.
const DATE_COLUMN: f32 = size::AXIS + space::XS;
/// The date column of the narrow layout (`html[data-size="s"] .day`, 44).
const DATE_COLUMN_NARROW: f32 = size::AXIS_NARROW;
/// The day number (`.date b{font:600 22px/32px "Inter Tabular"}`).
const DAY_NUMBER: typo::Level = typo::HEADLINE.tabular();
/// Today's red circle around the day number (`.date b`, 32).
const TODAY_CIRCLE: f32 = size::CONTROL;
/// The start of an event (`.ev .tm b`, 13/500 tabular).
const START: typo::Level = typo::BODY_STRONG.tabular();
/// The quiet head of a day ("Today … 4 events", `.dayhead{height:24px}`) and an all-day chip.
const DAY_HEAD: f32 = space::XL;
/// An event row: start and duration (36) with 4 above and below (`.ev{min-height:44px}`).
const EVENT_ROW: f32 = size::ROW + space::XS;
/// The "3 h free" line (`.free{height:28px}`).
const FREE_ROW: f32 = size::CONTROL_SM;
/// The line of empty days (`.gap{padding:10px 0}` around a 16-high line).
const GAP_ROW: f32 = size::CONTROL_LG;
/// Ended events and chips are drawn at this opacity (`.ev.past`).
const PAST_ALPHA: f32 = 0.55;
/// Dash and gap of the dotted line after "3 h free" (`repeating-linear-gradient(… 0 3px,
/// transparent 3px 6px)`).
const DASH: [f32; 2] = [3.0, 3.0];
/// Joins the parts of one line: "This week · Week 42", "Room Elbe · day 2 of 3".
const SEPARATOR: &str = " · ";
/// Between the first and the last of a run of empty days: "Tue 20 – Thu 22".
const RANGE: &str = " – ";

/// One block of the list, in display order.
#[derive(Debug, Clone, PartialEq)]
enum Block {
    /// A calendar week begins: "This week · Week 42", "Week 43".
    Week(String),
    Day(Day),
    /// Empty days in a row, never across a week boundary.
    Gap {
        first: NaiveDate,
        last: NaiveDate,
    },
}

/// A day with events (or today, which always shows).
#[derive(Debug, Clone, PartialEq)]
struct Day {
    date: NaiveDate,
    /// "Today", "Tomorrow", or the month's name on the first day shown of a new month.
    label: Option<String>,
    /// The all-day events, earliest first day first.
    chips: Vec<Chip>,
    /// The timed events with the free time between them and, on today, the now line.
    lines: Vec<Line>,
}

impl Day {
    /// The events of the day, all-day ones included: "4 events".
    fn count(&self) -> usize {
        self.chips.len() + self.lines.iter().filter(|line| matches!(line, Line::Event(_))).count()
    }
}

/// An all-day event on one day.
#[derive(Debug, Clone, PartialEq)]
struct Chip {
    /// Index into the app's event list, for `OpenEvent`.
    index: usize,
    color: Color,
    title: String,
    /// "day 2 of 3" for an event over several days.
    note: Option<String>,
    /// Its last day is before today.
    past: bool,
}

/// A line under the chips of a day.
#[derive(Debug, Clone, PartialEq)]
enum Line {
    Event(Row),
    /// Free time of at least [`FREE_MINUTES`] before the next event, in whole hours.
    Free(i64),
    /// The now line, at this time (only on today).
    Now(NaiveTime),
}

/// A timed event as it shows on one day.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    /// Index into the app's event list, for `OpenEvent`.
    index: usize,
    color: Color,
    title: String,
    /// The location, its first line.
    place: Option<String>,
    /// The place is a link or a video call: a camera instead of the map pin.
    video: bool,
    /// "day 2 of 3" on the later days of an event that runs over midnight.
    note: Option<String>,
    /// Start on this day: the event's own, or 00:00 on the days after its first.
    start: NaiveTime,
    /// Length on this day (up to midnight); `None` for an event without an end.
    minutes: Option<i64>,
    /// Over on this day.
    past: bool,
    /// Running now (today only): whole minutes until the event ends.
    left: Option<i64>,
}

/// The agenda in the layout of `class`: the narrow layout has the narrower date and time columns
/// and leaves out "40 min left".
pub fn view(data: ViewData<'_>, class: SizeClass) -> Element<'_, Message> {
    let blocks = plan(&data.items, data.start, data.end, data.now);
    if blocks.is_empty() {
        return empty_state(tr("No events"), tr("Nothing planned in the next 30 days"));
    }
    let today = data.now.date();
    let selected = data.selected;
    let narrow = class == SizeClass::Narrow;
    responsive(move |bounds| list(&blocks, Metrics::new(bounds.width, narrow), today, selected)).into()
}

// ---------------------------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------------------------

/// The blocks of the agenda from `start` to `end` (exclusive): a week head whenever a calendar
/// week begins, the days with events (and today when it is in the window), and runs of empty days
/// between them, split at week boundaries. Empty days before the first and after the last shown
/// day are left out. No events in the window → nothing at all (the empty state).
fn plan(items: &[Item<'_>], start: NaiveDate, end: NaiveDate, now: NaiveDateTime) -> Vec<Block> {
    let window_last = end - Duration::days(1);
    let mut by_day: BTreeMap<NaiveDate, Vec<&Item<'_>>> = BTreeMap::new();
    for item in items {
        let (first, last) = covered_days(item.event);
        let mut day = first.max(start);
        while day <= last.min(window_last) {
            by_day.entry(day).or_default().push(item);
            day += Duration::days(1);
        }
    }
    if by_day.is_empty() {
        return Vec::new();
    }
    let today = now.date();
    if (start..end).contains(&today) {
        by_day.entry(today).or_default();
    }

    let mut blocks = Vec::new();
    let mut week = None;
    let mut month = start.month();
    let mut previous: Option<NaiveDate> = None;
    for (date, items) in by_day {
        let gaps = previous.map(|before| gaps(before, date)).unwrap_or_default();
        for (first, last) in gaps {
            push_week(&mut blocks, &mut week, first, today);
            blocks.push(Block::Gap { first, last });
        }
        push_week(&mut blocks, &mut week, date, today);
        let label = day_label(date, today, date.month() != month);
        blocks.push(Block::Day(Day {
            date,
            label,
            chips: chips(&items, date, today),
            lines: lines(&items, date, now),
        }));
        month = date.month();
        previous = Some(date);
    }
    blocks
}

/// Adds the head of `date`'s calendar week unless it is the week shown last.
fn push_week(blocks: &mut Vec<Block>, week: &mut Option<NaiveDate>, date: NaiveDate, today: NaiveDate) {
    let monday = week_start(date);
    if *week != Some(monday) {
        *week = Some(monday);
        blocks.push(Block::Week(week_label(date, today)));
    }
}

/// "This week · Week 42" for the week of today, else "Week 43".
fn week_label(date: NaiveDate, today: NaiveDate) -> String {
    let number = trf("Week {week}", &[("week", &date.iso_week().week())]);
    if week_start(date) == week_start(today) { format!("{}{SEPARATOR}{number}", tr("This week")) } else { number }
}

/// The head label of a day: "Today", "Tomorrow", or the month's name when a new month begins.
fn day_label(date: NaiveDate, today: NaiveDate, new_month: bool) -> Option<String> {
    match (date - today).num_days() {
        0 => Some(tr("Today").to_string()),
        1 => Some(tr("Tomorrow").to_string()),
        _ if new_month => Some(month_name(date.month()).to_string()),
        _ => None,
    }
}

/// The empty days strictly between `before` and `after` as `(first, last)` runs, one per
/// calendar week.
fn gaps(before: NaiveDate, after: NaiveDate) -> Vec<(NaiveDate, NaiveDate)> {
    let mut runs = Vec::new();
    let mut first = before + Duration::days(1);
    let end = after - Duration::days(1);
    while first <= end {
        let last = end.min(week_start(first) + Duration::days(6));
        runs.push((first, last));
        first = last + Duration::days(1);
    }
    runs
}

/// The all-day events of `date`: earliest first day first, then by title.
fn chips(items: &[&Item<'_>], date: NaiveDate, today: NaiveDate) -> Vec<Chip> {
    let mut found: Vec<(NaiveDate, Chip)> = items
        .iter()
        .filter(|item| item.event.all_day())
        .map(|item| {
            let (first, last) = covered_days(item.event);
            let chip = Chip {
                index: item.index,
                color: item.color,
                title: title(item.event),
                note: position(first, last, date),
                past: last < today,
            };
            (first, chip)
        })
        .collect();
    found.sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| by_title(&x.title, &y.title)).then(x.index.cmp(&y.index)));
    found.into_iter().map(|(_, chip)| chip).collect()
}

/// The timed events of `date` in order of their start, with "3 h free" before an event that
/// follows two or more free hours, and on today the now line before the first event still ahead
/// (at the end when none is). On today free time counts from now: what is behind is not free any
/// more.
fn lines(items: &[&Item<'_>], date: NaiveDate, now: NaiveDateTime) -> Vec<Line> {
    let mut rows: Vec<(NaiveDateTime, NaiveDateTime, Row)> =
        items.iter().filter(|item| !item.event.all_day()).map(|item| segment(item, date, now)).collect();
    rows.sort_by(|(a, _, x), (b, _, y)| {
        a.cmp(b).then_with(|| by_title(&x.title, &y.title)).then(x.index.cmp(&y.index))
    });

    let today = date == now.date();
    let mut lines = Vec::new();
    let mut now_shown = !today;
    let mut busy_until: Option<NaiveDateTime> = None;
    for (start, end, row) in rows {
        if !now_shown && start > now {
            lines.push(Line::Now(now.time()));
            now_shown = true;
        }
        if let Some(until) = busy_until {
            let from = if today { until.max(now) } else { until };
            let free = (start - from).num_minutes();
            if free as f32 >= FREE_MINUTES {
                lines.push(Line::Free(free / 60));
            }
        }
        busy_until = Some(busy_until.map_or(end, |until| until.max(end)));
        lines.push(Line::Event(row));
    }
    if !now_shown {
        lines.push(Line::Now(now.time()));
    }
    lines
}

/// The part of a timed event on `date` as `(start, end, row)`: from its start (00:00 on the days
/// after its first) to its end or midnight, whichever comes first.
fn segment(item: &Item<'_>, date: NaiveDate, now: NaiveDateTime) -> (NaiveDateTime, NaiveDateTime, Row) {
    let event = item.event;
    let (first, last) = covered_days(event);
    let event_start = local_at(event.start);
    let event_end = event.end.map(local_at).filter(|end| *end > event_start);
    let start = if date == first { event_start } else { date.and_time(NaiveTime::MIN) };
    let midnight = (date + Duration::days(1)).and_time(NaiveTime::MIN);
    let end = event_end.map_or(start, |end| end.min(midnight));
    let running = date == now.date() && start <= now && now < end;
    let place = event.location.as_deref().and_then(|text| text.lines().next()).map(str::trim).filter(|p| !p.is_empty());
    let row = Row {
        index: item.index,
        color: item.color,
        title: title(event),
        place: place.map(str::to_string),
        video: place.is_some_and(is_video),
        note: if date > first { position(first, last, date) } else { None },
        start: start.time(),
        minutes: event_end.map(|_| (end - start).num_minutes()),
        past: end <= now,
        left: event_end.filter(|_| running).map(|end| minutes_until(now, end)),
    };
    (start, end, row)
}

/// The length of an event under its start: "15 min", "2 h", and "1 h 30" without the second
/// unit, which the start column (as wide as the time axis) has no room for (`dur()` in the
/// prototype).
fn duration(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (hours, rest) if hours > 0 && rest > 0 => trf("{h} h {m}", &[("h", &hours), ("m", &rest)]),
        _ => span(minutes),
    }
}

/// "day 2 of 3" for `date` in an event from `first` to `last`; `None` for a single day.
fn position(first: NaiveDate, last: NaiveDate, date: NaiveDate) -> Option<String> {
    let total = (last - first).num_days() + 1;
    (total > 1).then(|| trf("day {n} of {total}", &[("n", &((date - first).num_days() + 1)), ("total", &total)]))
}

/// A place that is a video call: a link (`https://…`, `meet.example.org/daily`) or a place
/// that says "video".
fn is_video(place: &str) -> bool {
    let link = place.contains("://")
        || place.starts_with("www.")
        || (!place.contains(char::is_whitespace) && place.contains('.') && place.contains('/'));
    link || place.to_lowercase().contains("video")
}

/// Whole minutes from `now` to `at`, rounded up: 30 seconds left are "1 min", not "0 min".
fn minutes_until(now: NaiveDateTime, at: NaiveDateTime) -> i64 {
    ((at - now).num_seconds().max(0) + 59) / 60
}

/// The title of an event, "(untitled)" when it has none.
fn title(event: &Event) -> String {
    let summary = event.summary.trim();
    if summary.is_empty() { tr("(untitled)").to_string() } else { summary.to_string() }
}

fn by_title(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
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

fn is_weekend(date: NaiveDate) -> bool {
    matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

/// "3 d · Nothing planned · Tue 20 – Thu 22" without the count, which stands in the date column.
fn gap_text(first: NaiveDate, last: NaiveDate) -> String {
    let day = |date: NaiveDate| trf("{weekday} {day}", &[("weekday", &weekday_name(date, true)), ("day", &date.day())]);
    let range = if first == last { day(first) } else { format!("{}{RANGE}{}", day(first), day(last)) };
    format!("{}{SEPARATOR}{range}", tr("Nothing planned"))
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

/// The widths of the list at its current size, for its columns and for cutting titles.
#[derive(Debug, Clone, Copy)]
struct Metrics {
    narrow: bool,
    /// Left and right of the list (`.inner{padding:… 24px}`, narrow 12).
    side: f32,
    date: f32,
    /// The start column of a row: as wide as the time axis of the day and week views.
    time: f32,
    /// Between the cells of a row.
    gap: f32,
    /// What a row's text has beside the start column and the bar.
    text: f32,
}

impl Metrics {
    fn new(width: f32, narrow: bool) -> Metrics {
        let side = if narrow { space::M } else { space::XL };
        let date = if narrow { DATE_COLUMN_NARROW } else { DATE_COLUMN };
        let time = if narrow { size::AXIS_NARROW } else { size::AXIS };
        let gap = if narrow { space::S } else { space::M };
        let events = width.min(size::CONTENT_MAX) - 2.0 * side - date - 2.0 * space::S;
        let text = (events - time - BAR - 2.0 * gap).max(0.0);
        Metrics { narrow, side, date, time, gap, text }
    }
}

/// The whole list in a scrollable, at most [`size::CONTENT_MAX`] wide.
fn list(blocks: &[Block], m: Metrics, today: NaiveDate, selected: Option<usize>) -> Element<'static, Message> {
    let mut list = column![].width(Length::Fill);
    for (n, block) in blocks.iter().enumerate() {
        // Days and gaps are separated by a line, but not from a week head, whose own line already
        // separates (two lines would double up), nor from the end of the list.
        let line_below =
            !matches!(block, Block::Week(_)) && matches!(blocks.get(n + 1), Some(Block::Day(_) | Block::Gap { .. }));
        list = match block {
            Block::Week(label) => list.push(week_head(label, n == 0)),
            Block::Day(day) => list.push(day_view(day, m, today, selected)),
            Block::Gap { first, last } => list.push(gap_view(*first, *last, m)),
        };
        if line_below {
            list = list.push(rule::horizontal(size::HAIRLINE).style(style::rule));
        }
    }
    let top = if m.narrow { space::XS } else { space::S };
    let list = container(list).padding(Padding { top, right: m.side, bottom: space::XXL, left: m.side });
    iced::widget::scrollable(widgets::content(size::CONTENT_MAX, list))
        .direction(iced::widget::scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// "Week 43" in `CAPTION` / `text_3` with a line to the right (`.week`); 24 above, 8 for the
/// first, which sits right under the header bar.
fn week_head(label: &str, first: bool) -> Element<'static, Message> {
    let content = row![
        typo::text(typo::CAPTION, label.to_string()).style(theme::text_3).wrapping(Wrapping::None),
        rule::horizontal(size::HAIRLINE).style(style::rule),
    ]
    .spacing(space::M)
    .align_y(Alignment::Center);
    let top = if first { space::S } else { space::XL };
    container(content).padding(Padding::new(0.0).top(top).bottom(space::XS)).into()
}

/// A day: the date column, and beside it the head, the chips and the lines.
fn day_view(day: &Day, m: Metrics, today: NaiveDate, selected: Option<usize>) -> Element<'static, Message> {
    let head = row![
        typo::text(typo::CAPTION, day.label.clone().unwrap_or_default()).style(theme::text_2),
        Space::new().width(Length::Fill),
        typo::text(typo::META.tabular(), count_text(day.count())).style(theme::text_3),
    ]
    .align_y(Alignment::Center)
    .height(DAY_HEAD);
    let mut events =
        column![container(head).padding(Padding::new(0.0).left(space::S).right(space::S).bottom(space::XS))]
            .width(Length::Fill);
    if !day.chips.is_empty() {
        let chips = day.chips.iter().map(|chip| chip_view(chip, m, selected == Some(chip.index))).collect();
        events = events
            .push(container(widgets::chip_row(chips)).padding(Padding::new(0.0).left(space::S).bottom(space::XS)));
    }
    for line in &day.lines {
        events = events.push(match line {
            Line::Event(event) => row_view(event, m, selected == Some(event.index)),
            Line::Free(hours) => free_view(*hours, m),
            Line::Now(at) => now_view(*at),
        });
    }
    if day.count() == 0 {
        let calm = typo::text(typo::META, tr("Nothing planned")).style(theme::text_3);
        events = events.push(container(calm).padding(Padding::new(0.0).left(space::S).top(space::XS)));
    }
    row![date_view(day.date, today, m.date), events].padding([space::M, 0.0]).into()
}

/// "1 event", "4 events"; nothing for none.
fn count_text(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => tr("1 event").to_string(),
        n => trf("{n} events", &[("n", &n)]),
    }
}

/// The date column: weekday (`CAPTION`, `text_3`; today `text`) over the day number
/// (`HEADLINE` tabular; weekends `text_2`; today white in the red circle).
fn date_view(date: NaiveDate, today: NaiveDate, width: f32) -> Element<'static, Message> {
    let is_today = date == today;
    let weekend = is_weekend(date);
    let weekday = typo::text(typo::CAPTION, weekday_name(date, true))
        .style(move |t: &Theme| TextStyle { color: Some(if is_today { palette(t).text } else { palette(t).text_3 }) });
    let number = typo::text(DAY_NUMBER, date.day().to_string()).style(move |t: &Theme| {
        let p = palette(t);
        TextStyle {
            color: Some(if is_today {
                p.on_accent
            } else if weekend {
                p.text_2
            } else {
                p.text
            }),
        }
    });
    let circle = container(number).center(TODAY_CIRCLE).style(move |t| container::Style {
        background: is_today.then(|| Background::Color(palette(t).accent)),
        border: border::rounded(TODAY_CIRCLE / 2.0),
        ..container::Style::default()
    });
    column![weekday, circle].align_x(Alignment::Center).width(width).into()
}

/// An all-day chip (`.chip`): tinted, the bar at the left, title in `CAPTION`, "day 2 of 3" in
/// `META` / `text_3`. Outlined in `focus_ring` while open in the side panel.
fn chip_view(chip: &Chip, m: Metrics, selected: bool) -> Element<'static, Message> {
    let (color, alpha) = (chip.color, if chip.past { PAST_ALPHA } else { 1.0 });
    let note_width = chip.note.as_deref().map_or(0.0, |note| text_width(note, typo::META) + space::XS);
    let room = m.text + m.time + 2.0 * m.gap - 2.0 * space::S - note_width;
    let mut content = row![
        typo::text(typo::CAPTION, ellipsize(&chip.title, typo::CAPTION, room))
            .style(move |t: &Theme| faded(palette(t).text, alpha))
            .wrapping(Wrapping::None)
    ]
    .spacing(space::XS)
    .align_y(Alignment::Center);
    if let Some(note) = &chip.note {
        content =
            content.push(typo::text(typo::META, note.clone()).style(move |t: &Theme| faded(palette(t).text_3, alpha)));
    }
    let bar = container(Space::new()).width(BAR).height(Length::Fill).style(move |_| container::Style {
        background: Some(Background::Color(color.scale_alpha(alpha))),
        border: Border { radius: Radius::new(0.0).left(radius::XS), ..Border::default() },
        ..container::Style::default()
    });
    let body =
        container(content).padding([0.0, space::S]).height(Length::Fill).center_y(Length::Fill).style(move |t| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(tint(p, color).scale_alpha(alpha))),
                border: Border { radius: Radius::new(0.0).right(radius::XS), ..Border::default() },
                ..container::Style::default()
            }
        });
    let chip_box = container(row![bar, body].height(DAY_HEAD)).style(move |t| container::Style {
        border: if selected {
            border::rounded(radius::XS).color(palette(t).focus_ring).width(style::FOCUS_RING_WIDTH)
        } else {
            Border::default()
        },
        ..container::Style::default()
    });
    mouse_area(chip_box).on_press(Message::OpenEvent(chip.index)).interaction(mouse::Interaction::Pointer).into()
}

/// An event row (`.ev`): start over duration, the bar, title over place, and on the running event
/// "40 min left" at the right. Hover `hover`, open in the side panel `selected`, running tinted,
/// ended at 55 %.
fn row_view(event: &Row, m: Metrics, selected: bool) -> Element<'static, Message> {
    let alpha = if event.past { PAST_ALPHA } else { 1.0 };
    let color = event.color;

    let mut time =
        column![typo::text(START, format_time(event.start)).style(move |t: &Theme| faded(palette(t).text, alpha))];
    if let Some(minutes) = event.minutes {
        time = time.push(
            typo::text(typo::META.tabular(), duration(minutes)).style(move |t: &Theme| faded(palette(t).text_3, alpha)),
        );
    }
    let time = container(time).width(m.time).height(Length::Fill).center_y(Length::Fill).clip(true);

    let bar = container(Space::new()).width(BAR).height(Length::Fill).style(move |_| container::Style {
        background: Some(Background::Color(color.scale_alpha(alpha))),
        border: border::rounded(BAR / 2.0),
        ..container::Style::default()
    });

    let left = event.left.filter(|_| !m.narrow).map(|minutes| trf("{span} left", &[("span", &span(minutes))]));
    let side_width = left.as_deref().map_or(0.0, |label| text_width(label, typo::META) + 2.0 * space::S + m.gap);
    let room = m.text - side_width;
    let mut what = column![
        typo::text(typo::BODY_STRONG, ellipsize(&event.title, typo::BODY_STRONG, room))
            .style(move |t: &Theme| faded(palette(t).text, alpha))
            .wrapping(Wrapping::None)
    ];
    if let Some(detail) = detail_view(event, room, alpha) {
        what = what.push(detail);
    }
    let what = container(what).width(Length::Fill).height(Length::Fill).center_y(Length::Fill).clip(true);

    let mut cells = row![time, bar, what].spacing(m.gap).height(Length::Fill);
    if let Some(label) = left {
        cells = cells.push(container(widgets::pill(None, label)).height(Length::Fill).center_y(Length::Fill));
    }
    let running = event.left.is_some();
    button(cells)
        .width(Length::Fill)
        .height(EVENT_ROW)
        .padding([space::XS, space::S])
        .style(move |t, status| row_style(palette(t), status, selected, running.then_some(color)))
        .on_press(Message::OpenEvent(event.index))
        .into()
}

/// The line under the title: map pin or camera, the place and "day 2 of 3"; `None` with neither.
fn detail_view(event: &Row, room: f32, alpha: f32) -> Option<Element<'static, Message>> {
    let text = [event.place.as_deref(), event.note.as_deref()].into_iter().flatten().collect::<Vec<_>>();
    if text.is_empty() {
        return None;
    }
    let mut line = row![].spacing(space::XS).align_y(Alignment::Center);
    let mut room = room;
    if event.place.is_some() {
        let which = if event.video { Icon::Video } else { Icon::MapPin };
        line = line.push(
            icon::icon(which, icon::SIZE_SM)
                .style(move |t: &Theme, _| svg::Style { color: Some(palette(t).text_3.scale_alpha(alpha)) }),
        );
        room -= icon::SIZE_SM + space::XS;
    }
    let text = ellipsize(&text.join(SEPARATOR), typo::META, room);
    line = line.push(
        typo::text(typo::META, text).style(move |t: &Theme| faded(palette(t).text_2, alpha)).wrapping(Wrapping::None),
    );
    Some(line.into())
}

/// The surface of an event row: `selected` while open in the side panel; else `hover` under the
/// pointer, `selected` while pressed; else the running event's tint, or nothing.
fn row_style(p: &Palette, status: button::Status, selected: bool, running: Option<Color>) -> button::Style {
    let background = match status {
        _ if selected => Some(p.selected),
        button::Status::Hovered => Some(p.hover),
        button::Status::Pressed => Some(p.selected),
        _ => running.map(|color| tint(p, color)),
    };
    button::Style {
        background: background.map(Background::Color),
        text_color: p.text,
        border: border::rounded(radius::SM),
        ..button::Style::default()
    }
}

/// "3 h free" in `META` / `text_3` under the bar, followed by a dotted line (`.free`).
fn free_view(hours: i64, m: Metrics) -> Element<'static, Message> {
    let note =
        typo::text(typo::META, trf("{n} h free", &[("n", &hours)])).style(theme::text_3).wrapping(Wrapping::None);
    let dots = canvas_widget(Dots).width(Length::Fill).height(size::HAIRLINE);
    let line = row![note, dots].spacing(space::S).align_y(Alignment::Center);
    row![Space::new().width(m.time), line]
        .spacing(m.gap)
        .align_y(Alignment::Center)
        .height(FREE_ROW)
        .padding([0.0, space::S])
        .into()
}

/// The now line (`.nowline`): the time white on a red pill as in the day view's axis, then a
/// 1-px red line to the right edge.
fn now_view(at: NaiveTime) -> Element<'static, Message> {
    let pill = container(
        typo::text(NOW_LABEL, format_time(at)).style(|t: &Theme| TextStyle { color: Some(palette(t).on_accent) }),
    )
    .center_x(NOW_LABEL_WIDTH)
    .style(|t| container::Style {
        background: Some(Background::Color(palette(t).accent)),
        border: border::rounded(radius::XS),
        ..container::Style::default()
    });
    let line = container(Space::new()).width(Length::Fill).height(size::HAIRLINE).style(|t| container::Style {
        background: Some(Background::Color(palette(t).accent)),
        ..container::Style::default()
    });
    row![pill, line].spacing(space::XS).align_y(Alignment::Center).padding([0.0, space::S]).into()
}

/// Empty days (`.gap`): "3 d" in the date column, "Nothing planned · Tue 20 – Thu 22" beside it.
fn gap_view(first: NaiveDate, last: NaiveDate, m: Metrics) -> Element<'static, Message> {
    let days = (last - first).num_days() + 1;
    let count = typo::text(typo::CAPTION.tabular(), trf("{n} d", &[("n", &days)])).style(theme::text_3);
    let text = typo::text(typo::META, gap_text(first, last)).style(theme::text_3).wrapping(Wrapping::None);
    row![container(count).center_x(m.date), container(text).padding(Padding::new(0.0).left(space::S)).clip(true)]
        .align_y(Alignment::Center)
        .height(GAP_ROW)
        .into()
}

/// The tinted surface of an event in `color`, composed over the panel.
fn tint(p: &Palette, color: Color) -> Color {
    over(p.tinted(color), p.panel)
}

/// A text colour at `alpha` (ended events).
fn faded(color: Color, alpha: f32) -> TextStyle {
    TextStyle { color: Some(color.scale_alpha(alpha)) }
}

/// The dotted line after "3 h free", in `line_strong`.
struct Dots;

impl canvas::Program<Message> for Dots {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let y = bounds.height / 2.0;
        let stroke = Stroke {
            line_dash: LineDash { segments: &DASH, offset: 0 },
            ..Stroke::default().with_color(palette(theme).line_strong).with_width(size::HAIRLINE)
        };
        frame.stroke(&Path::line(Point::new(0.0, y), Point::new(bounds.width, y)), stroke);
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_gui::datetime::to_utc;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).expect("valid date")
    }

    fn local(d: u32, h: u32, m: u32) -> NaiveDateTime {
        day(d).and_hms_opt(h, m, 0).expect("valid time")
    }

    fn time(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).expect("valid time")
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
        events
            .iter()
            .enumerate()
            .map(|(index, event)| Item { index, event, color: theme::Palette::dark().text })
            .collect()
    }

    fn planned(events: &[Event], start: u32, end: u32, now: NaiveDateTime) -> Vec<Block> {
        plan(&items(events), day(start), day(end), now)
    }

    fn days(blocks: &[Block]) -> Vec<&Day> {
        blocks
            .iter()
            .filter_map(|block| match block {
                Block::Day(day) => Some(day),
                _ => None,
            })
            .collect()
    }

    /// The shape of the list: `W` week head, `D13` day 13, `G20-22` empty days.
    fn shape(blocks: &[Block]) -> Vec<String> {
        blocks
            .iter()
            .map(|block| match block {
                Block::Week(_) => "W".to_string(),
                Block::Day(d) => format!("D{}", d.date.day()),
                Block::Gap { first, last } => format!("G{}-{}", first.day(), last.day()),
            })
            .collect()
    }

    /// The lines of a day as short words: `09:30` an event by its start, `free 3`, `now`.
    fn words(day: &Day) -> Vec<String> {
        day.lines
            .iter()
            .map(|line| match line {
                Line::Event(row) => format_time(row.start),
                Line::Free(hours) => format!("free {hours}"),
                Line::Now(_) => "now".to_string(),
            })
            .collect()
    }

    fn rows(day: &Day) -> Vec<&Row> {
        day.lines
            .iter()
            .filter_map(|line| match line {
                Line::Event(row) => Some(row),
                _ => None,
            })
            .collect()
    }

    // 2026-10-13 is a Tuesday in ISO week 42.

    #[test]
    fn groups_by_day_with_all_day_chips_and_timed_rows_in_order() {
        let events = [
            event("late", at(14, 15, 0), Some(at(14, 16, 0))),
            event("early", at(14, 9, 0), Some(at(14, 9, 15))),
            event("holiday", date(14), Some(date(15))),
            event("other day", at(16, 10, 0), Some(at(16, 11, 0))),
        ];
        let blocks = planned(&events, 14, 20, local(10, 8, 0));
        assert_eq!(shape(&blocks), ["W", "D14", "G15-15", "D16"]);
        let found = days(&blocks);
        assert_eq!(found[0].chips.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["holiday"]);
        assert_eq!(words(found[0]), ["09:00", "free 5", "15:00"]);
        assert_eq!(found[0].count(), 3);
        assert_eq!(count_text(found[0].count()), trf("{n} events", &[("n", &3)]));
        assert_eq!(count_text(1), tr("1 event"));
    }

    #[test]
    fn no_events_in_the_window_is_the_empty_state_even_with_today_in_it() {
        let events = [event("later", at(25, 9, 0), Some(at(25, 10, 0)))];
        assert!(planned(&events, 13, 20, local(13, 8, 0)).is_empty());
        assert!(planned(&[], 13, 20, local(13, 8, 0)).is_empty());
    }

    #[test]
    fn durations_read_as_minutes_hours_or_both() {
        let events = [
            event("standup", at(14, 9, 30), Some(at(14, 9, 45))),
            event("workshop", at(14, 10, 0), Some(at(14, 12, 0))),
            event("talk", at(14, 13, 0), Some(at(14, 14, 30))),
            event("open end", at(14, 18, 0), None),
        ];
        let blocks = planned(&events, 14, 15, local(10, 8, 0));
        let minutes: Vec<_> = rows(days(&blocks)[0]).iter().map(|row| row.minutes).collect();
        assert_eq!(minutes, [Some(15), Some(120), Some(90), None]);
        assert_eq!(duration(15), trf("{n} min", &[("n", &15)]));
        assert_eq!(duration(120), trf("{n} h", &[("n", &2)]));
        assert_eq!(duration(90), trf("{h} h {m}", &[("h", &1), ("m", &30)]));
        // Each fits the start column (by the generous estimate of `text_width`, which runs a few
        // pixels wide; the narrow column of 44 is checked on screen).
        for minutes in [45, 90, 570, 600] {
            assert!(text_width(&duration(minutes), typo::META) <= size::AXIS, "{}", duration(minutes));
        }
    }

    #[test]
    fn free_time_of_two_hours_or_more_between_two_events() {
        let events = [
            event("a", at(14, 9, 0), Some(at(14, 10, 0))),
            event("b", at(14, 11, 0), Some(at(14, 12, 0))),
            event("c", at(14, 15, 30), Some(at(14, 16, 0))),
            // Inside "c": the busy time runs to the latest end so far.
            event("d", at(14, 15, 30), Some(at(14, 15, 45))),
            event("e", at(14, 18, 0), Some(at(14, 19, 0))),
        ];
        let blocks = planned(&events, 14, 15, local(10, 8, 0));
        assert_eq!(words(days(&blocks)[0]), ["09:00", "11:00", "free 3", "15:30", "15:30", "free 2", "18:00"]);
    }

    #[test]
    fn free_time_today_counts_from_now() {
        let events = [
            event("morning", at(13, 9, 0), Some(at(13, 10, 0))),
            event("afternoon", at(13, 15, 0), Some(at(13, 16, 0))),
            event("evening", at(13, 17, 0), Some(at(13, 20, 0))),
            event("night", at(13, 22, 0), Some(at(13, 23, 0))),
        ];
        // 12:30: two and a half hours are left before "afternoon"; the gap before "night" is 2 h.
        let blocks = planned(&events, 13, 14, local(13, 12, 30));
        assert_eq!(words(days(&blocks)[0]), ["09:00", "now", "free 2", "15:00", "17:00", "free 2", "22:00"]);
        // 14:00: only one hour is left; what is behind is not free any more.
        let blocks = planned(&events, 13, 14, local(13, 14, 0));
        assert_eq!(words(days(&blocks)[0]), ["09:00", "now", "15:00", "17:00", "free 2", "22:00"]);
    }

    #[test]
    fn running_event_is_marked_and_the_now_line_follows_it() {
        let events = [
            event("standup", at(13, 9, 30), Some(at(13, 9, 45))),
            event("workshop", at(13, 13, 0), Some(at(13, 15, 0))),
            event("one on one", at(13, 14, 30), Some(at(13, 15, 0))),
        ];
        let blocks = planned(&events, 13, 14, local(13, 14, 20));
        let today = days(&blocks)[0];
        assert_eq!(words(today), ["09:30", "13:00", "now", "14:30"]);
        let found = rows(today);
        assert_eq!(
            found.iter().map(|row| (row.past, row.left)).collect::<Vec<_>>(),
            [(true, None), (false, Some(40)), (false, None)]
        );
        assert_eq!(today.label.as_deref(), Some(tr("Today")));
    }

    #[test]
    fn now_line_ends_a_day_that_is_over_and_starts_one_still_ahead() {
        let events = [event("done", at(13, 9, 0), Some(at(13, 10, 0)))];
        assert_eq!(words(days(&planned(&events, 13, 14, local(13, 18, 0)))[0]), ["09:00", "now"]);
        assert_eq!(words(days(&planned(&events, 13, 14, local(13, 7, 0)))[0]), ["now", "09:00"]);
        // Other days have no now line.
        let events = [event("tomorrow", at(14, 9, 0), Some(at(14, 10, 0)))];
        let blocks = planned(&events, 13, 15, local(13, 18, 0));
        assert_eq!(shape(&blocks), ["W", "D13", "D14"]);
        assert_eq!(words(days(&blocks)[0]), ["now"], "today shows even without events");
        assert_eq!(words(days(&blocks)[1]), ["09:00"]);
        assert_eq!(days(&blocks)[1].label.as_deref(), Some(tr("Tomorrow")));
    }

    #[test]
    fn empty_days_between_shown_days_are_one_line_per_week() {
        let events = [
            event("mon", at(19, 9, 0), Some(at(19, 10, 0))),
            event("fri", at(23, 9, 0), Some(at(23, 10, 0))),
            event("next tue", at(27, 9, 0), Some(at(27, 10, 0))),
            event("much later", at(10, 9, 0), Some(at(10, 10, 0))),
        ];
        let blocks = planned(&events, 13, 31, local(13, 8, 0));
        // Today (13) shows; 14–18 are empty (rest of week 42); 24–25 end week 43, 26 starts 44.
        assert_eq!(shape(&blocks), ["W", "D13", "G14-18", "W", "D19", "G20-22", "D23", "G24-25", "W", "G26-26", "D27"]);
        assert_eq!(gap_text(day(20), day(22)), {
            let d = |n: u32| trf("{weekday} {day}", &[("weekday", &weekday_name(day(n), true)), ("day", &n)]);
            format!("{}{SEPARATOR}{}{RANGE}{}", tr("Nothing planned"), d(20), d(22))
        });
        assert!(!gap_text(day(26), day(26)).contains(RANGE));
    }

    #[test]
    fn week_heads_name_this_week_and_the_numbers_of_the_others() {
        let events = [event("a", at(14, 9, 0), None), event("b", at(20, 9, 0), None)];
        let blocks = planned(&events, 14, 21, local(13, 8, 0));
        let heads: Vec<&str> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Week(label) => Some(label.as_str()),
                _ => None,
            })
            .collect();
        let this = format!("{}{SEPARATOR}{}", tr("This week"), trf("Week {week}", &[("week", &42)]));
        assert_eq!(heads, [this, trf("Week {week}", &[("week", &43)])]);
        // A list that starts in a later week has no "This week".
        let blocks = planned(&events, 20, 21, local(13, 8, 0));
        assert_eq!(blocks.first(), Some(&Block::Week(trf("Week {week}", &[("week", &43)]))));
    }

    #[test]
    fn a_new_month_is_named_in_the_day_head() {
        let november = NaiveDate::from_ymd_opt(2026, 11, 2).expect("valid date");
        let events = [
            event("a", at(30, 9, 0), None),
            event("b", at(31, 9, 0), None),
            event("c", EventTime::DateTime(to_utc(november.and_time(time(9, 0)))), None),
        ];
        let start = day(30);
        let blocks = plan(&items(&events), start, start + Duration::days(10), local(13, 8, 0));
        let labels: Vec<_> = days(&blocks).iter().map(|d| d.label.clone()).collect();
        assert_eq!(labels, [None, None, Some(month_name(11).to_string())]);
    }

    #[test]
    fn multi_day_events_show_their_position() {
        let events = [event("course", date(14), Some(date(17))), event("release", at(16, 22, 30), Some(at(17, 0, 30)))];
        let blocks = planned(&events, 14, 20, local(10, 8, 0));
        assert_eq!(shape(&blocks), ["W", "D14", "D15", "D16", "D17"]);
        let found = days(&blocks);
        let note = |n: i64, total: i64| Some(trf("day {n} of {total}", &[("n", &n), ("total", &total)]));
        assert_eq!(
            found.iter().map(|d| d.chips.first().and_then(|c| c.note.clone())).collect::<Vec<_>>(),
            [note(1, 3), note(2, 3), note(3, 3), None]
        );
        // The timed one: 22:30 for 90 minutes, then 00:00 for 30 minutes with its position.
        let first = rows(found[2])[0];
        assert_eq!((first.start, first.minutes, first.note.clone()), (time(22, 30), Some(90), None));
        let second = rows(found[3])[0];
        assert_eq!((second.start, second.minutes, second.note.clone()), (time(0, 0), Some(30), note(2, 2)));
    }

    #[test]
    fn past_marks_what_has_ended() {
        let events = [
            event("over", date(11), Some(date(13))),
            event("still on", date(12), Some(date(14))),
            event("ended", at(13, 9, 0), Some(at(13, 9, 30))),
            event("night", at(12, 23, 0), Some(at(13, 1, 0))),
            event("later", at(13, 11, 0), Some(at(13, 12, 0))),
        ];
        let blocks = planned(&events, 12, 14, local(13, 10, 0));
        let found = days(&blocks);
        let chips = |d: &Day| d.chips.iter().map(|c| (c.title.clone(), c.past)).collect::<Vec<_>>();
        assert_eq!(chips(found[0]), [("over".to_string(), true), ("still on".to_string(), false)]);
        assert_eq!(chips(found[1]), [("still on".to_string(), false)], "an all-day event lasts all of today");
        let past = |d: &Day| rows(d).iter().map(|r| (r.title.clone(), r.past)).collect::<Vec<_>>();
        assert_eq!(past(found[0]), [("night".to_string(), true)]);
        assert_eq!(
            past(found[1]),
            [("night".to_string(), true), ("ended".to_string(), true), ("later".to_string(), false)]
        );
    }

    #[test]
    fn places_with_links_or_video_get_the_camera() {
        assert!(is_video("meet.example.org/daily"));
        assert!(is_video("https://example.org/call"));
        assert!(is_video("Video call"));
        assert!(!is_video("Room Elbe"));
        assert!(!is_video("Dr. Weber, Main St. 3/1"));
    }

    #[test]
    fn view_builds_in_every_layout() {
        let events = [
            event("meeting", at(13, 9, 0), Some(at(13, 15, 0))),
            event("holiday", date(13), Some(date(15))),
            event("later", at(20, 9, 0), None),
        ];
        for class in [SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            for list in [Vec::new(), items(&events)] {
                let data = ViewData {
                    items: list,
                    start: day(13),
                    end: day(13) + Duration::days(30),
                    anchor: day(13),
                    now: local(13, 10, 0),
                    selected: Some(0),
                };
                let _ = view(data, class);
            }
        }
        let wide = Metrics::new(size::CONTENT_MAX, false);
        let narrow = Metrics::new(size::CONTENT_MAX, true);
        assert!(narrow.text > wide.text, "narrow columns leave the text more room at the same width");
    }
}
