//! Month view: six weeks of day cells (task K.10 in `docs/tasks/kalender-aufgaben.md`), drawn
//! after Linear (task C.3 in `docs/tasks/design.md`, tokens from the C.2 week view).
//!
//! A weekday header (Mon–Sun, `CAPTION` in `text_3`) above six rows of seven cells, starting at
//! `data.start` (the Monday on or before the 1st of the month). Only inner lines separate the
//! cells: no frame per cell. Days outside the anchor's month are dimmed: a `text_3` number, faded
//! entries and a surface towards the window background, a little more than the weekends of the
//! month are set off (as in the week view). Today's number is white in a red circle.
//!
//! Each cell lists its events, all-day first, then by start, as rows 20 high: all-day entries
//! as tinted chips with a bar in the calendar colour, timed ones with a dot and `14:00 Title`;
//! entries that are over are faded. As many rows as the cell is high (at most four, the
//! `+n more` line included); with more entries, the last row sums the rest up. A timed event that
//! continues from an earlier day shows as `… Title` on the following days. Multi-day and all-day
//! events appear on every day they cover. Click on a day number → `ShowDay(date)`, on an event →
//! `OpenEvent(index)`, on empty space in a cell → `NewEvent { date, time: None }`.
//!
//! Every colour comes from the theme's [`Palette`] when the view is drawn, so light and dark
//! both work.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use iced::alignment::Vertical;
use iced::border::{self, Radius};
use iced::widget::button;
use iced::widget::text::{Style as TextStyle, Wrapping};
use iced::widget::{Space, column, container, mouse_area, responsive, row, rule};
use iced::{Background, Border, Color, Element, Length, Padding, Theme, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_time, to_local, weekday_name};
use torro_gui::theme::{self, Palette, mix, over, palette, radius, size, space};
use torro_gui::typo::{ellipsize, text_width};
use torro_gui::{style, typo};
use torro_pim::{Event, EventTime};

use super::{Item, ViewData};
use crate::app::Message;

/// Lines per cell at most, the `+n more` line included.
const MAX_LINES: usize = 4;
/// Height of one event row; longer titles are clipped, not wrapped.
const EVENT_HEIGHT: f32 = 20.0;
/// Space between two event rows.
const LINE_GAP: f32 = 2.0;
/// Edge of the box around a day number, and of today's red circle.
const DAY_CIRCLE: f32 = 22.0;
/// The day number: 12/500 with tabular figures.
const DAY_NUMBER: typo::Level = typo::CAPTION.tabular();
/// Diameter of the dot in front of a timed entry.
const DOT: f32 = 6.0;
/// Width of the colour bar on the left of an all-day chip.
const BAR: f32 = 3.0;
/// Entries that are over, and those of the neighbouring months, are drawn at this opacity over
/// the panel (as in the week view).
const FADE_OPACITY: f32 = 0.55;
/// Weekend cells get this much of the window background over the panel (as in the week view).
const WEEKEND_SHADE: f32 = 0.35;
/// Cells of the neighbouring months get this much of the window background over the panel: more
/// than a weekend, so the month itself stands out.
const OUTSIDE_SHADE: f32 = 0.7;
const _: () = assert!(OUTSIDE_SHADE > WEEKEND_SHADE, "neighbouring months are dimmed more than weekends");
/// Height of the weekday header.
const HEADER_HEIGHT: f32 = size::CONTROL_SM;
/// The lines between the header and the grid and between the six rows of cells.
const INNER_LINES: f32 = 6.0;

pub fn view(data: ViewData<'_>) -> Element<'_, Message> {
    // The number of entry rows depends on the height the cells get, the cut titles on their width.
    responsive(move |bounds| month(&data, bounds.width, bounds.height)).into()
}

/// The header and the six weeks for a view `width` × `height`.
fn month<'a>(data: &ViewData<'a>, width: f32, height: f32) -> Element<'a, Message> {
    let today = data.now;
    let cell_height = (height - HEADER_HEIGHT - INNER_LINES) / 6.0;
    let cell_width = ((width - 6.0 * size::HAIRLINE) / 7.0).max(0.0);
    let lines = lines_that_fit(cell_height);
    let horizontal = || rule::horizontal(1).style(style::rule);
    let mut weeks = column![header(data.start), horizontal()].height(Length::Fill);
    for week in 0..6i64 {
        let mut cells = row![].height(Length::FillPortion(1));
        for day in 0..7i64 {
            if day > 0 {
                cells = cells.push(rule::vertical(1).style(style::rule));
            }
            let date = data.start + Duration::days(week * 7 + day);
            cells = cells.push(cell(date, data.anchor, today, &data.items, lines, cell_width));
        }
        if week > 0 {
            weeks = weeks.push(horizontal());
        }
        weeks = weeks.push(cells);
    }
    weeks.into()
}

/// Mon–Sun, short names, from `start` on, left-aligned with the day numbers below.
fn header<'a>(start: NaiveDate) -> Element<'a, Message> {
    let mut names = row![].width(Length::Fill).height(HEADER_HEIGHT);
    for offset in 0..7 {
        if offset > 0 {
            // As wide as the line between two cells, so the names stay above their columns.
            names = names.push(Space::new().width(size::HAIRLINE));
        }
        let name = typo::text(typo::CAPTION, weekday_name(start + Duration::days(offset), true)).style(theme::text_3);
        names = names.push(
            container(name)
                .padding(Padding::ZERO.left(space::S))
                .width(Length::FillPortion(1))
                .height(Length::Fill)
                .align_y(Vertical::Center),
        );
    }
    names.into()
}

/// One day `width` wide: the day number, then its events as far as `lines` rows go. Empty space
/// in the cell creates an event on that day.
fn cell<'a>(
    date: NaiveDate,
    anchor: NaiveDate,
    today: NaiveDateTime,
    items: &[Item<'a>],
    lines: usize,
    width: f32,
) -> Element<'a, Message> {
    let in_month = date.year() == anchor.year() && date.month() == anchor.month();
    let day = day_items(items, date);
    let (shown, more) = visible_split(day.len(), lines);
    let mut rows = column![].spacing(LINE_GAP);
    for item in &day[..shown] {
        let faded = !in_month || ends_before(item.event, today);
        rows = rows.push(event_row(*item, date, faded, width - 2.0 * space::XS));
    }
    if more > 0 {
        let summary = trf("{n} more", &[("n", &more)]);
        let more = typo::text(typo::META, format!("+{summary}")).style(theme::text_2).wrapping(Wrapping::None);
        rows = rows.push(container(more).padding(Padding::ZERO.left(space::XS)).height(EVENT_HEIGHT));
    }

    let number =
        container(day_number(date, today.date(), in_month)).padding(Padding::ZERO.top(space::S).left(space::S));
    let rows = container(rows).padding([0.0, space::XS]);
    let shade = cell_shade(in_month, is_weekend(date));
    let cell = container(column![number, rows].spacing(space::XS))
        .padding(Padding::ZERO.bottom(space::XS))
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .clip(true)
        .style(move |theme| container::Style {
            background: shade.map(|shade| Background::Color(mix(palette(theme).panel, palette(theme).bg, shade))),
            ..container::Style::default()
        });
    mouse_area(cell).on_press(Message::NewEvent { date, time: None }).interaction(mouse::Interaction::Pointer).into()
}

/// The day number: `text_2` (weekends and neighbouring months `text_3`); today white in a red
/// circle. Click → `ShowDay`.
fn day_number<'a>(date: NaiveDate, today: NaiveDate, in_month: bool) -> Element<'a, Message> {
    let is_today = date == today;
    let number = typo::text(DAY_NUMBER, date.day().to_string()).style(move |theme: &Theme| {
        let p = palette(theme);
        let color = if is_today {
            p.on_accent
        } else if in_month && !is_weekend(date) {
            p.text_2
        } else {
            p.text_3
        };
        TextStyle { color: Some(color) }
    });
    button(container(number).center(DAY_CIRCLE))
        .padding(0)
        .width(DAY_CIRCLE)
        .height(DAY_CIRCLE)
        .style(move |theme, status| {
            let p = palette(theme);
            let background = match (is_today, status) {
                (true, _) => Some(p.accent),
                (false, button::Status::Hovered) => Some(p.hover),
                (false, button::Status::Pressed) => Some(p.selected),
                (false, _) => None,
            };
            button::Style {
                background: background.map(Background::Color),
                border: border::rounded(DAY_CIRCLE / 2.0),
                ..button::Style::default()
            }
        })
        .on_press(Message::ShowDay(date))
        .into()
}

/// One event row `width` wide in a cell: all-day entries as a tinted chip with a bar, timed ones
/// with a dot. The title is cut with "…" to what is left of the row ([`title_room`]). `faded`
/// rows (over, or in a neighbouring month) are drawn at [`FADE_OPACITY`].
fn event_row<'a>(item: Item<'a>, date: NaiveDate, faded: bool, width: f32) -> Element<'a, Message> {
    let (prefix, title) = entry_parts(item.event, date);
    let title = ellipsize(&title, typo::CAPTION, title_room(item.event.all_day(), prefix.as_deref(), width));
    let color = item.color;
    let title = typo::text(typo::CAPTION, title)
        .style(move |theme: &Theme| TextStyle { color: Some(shade(palette(theme), palette(theme).text, faded)) })
        .wrapping(Wrapping::None);
    let body: Element<'a, Message> = if item.event.all_day() {
        let bar = container(Space::new()).width(BAR).height(Length::Fill).style(move |theme| container::Style {
            background: Some(Background::Color(shade(palette(theme), color, faded))),
            border: Border { radius: Radius::new(0.0).left(radius::XS), ..Border::default() },
            ..container::Style::default()
        });
        let fill = container(title)
            .padding([0.0, space::S])
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Vertical::Center)
            .clip(true)
            .style(move |theme| {
                let p = palette(theme);
                container::Style {
                    background: Some(Background::Color(shade(p, over(p.tinted(color), p.panel), faded))),
                    border: Border { radius: Radius::new(0.0).right(radius::XS), ..Border::default() },
                    ..container::Style::default()
                }
            });
        row![bar, fill].height(Length::Fill).into()
    } else {
        let dot = container(Space::new()).width(DOT).height(DOT).style(move |theme| container::Style {
            background: Some(Background::Color(shade(palette(theme), color, faded))),
            border: border::rounded(DOT / 2.0),
            ..container::Style::default()
        });
        let mut line = row![dot].spacing(space::XS).align_y(Vertical::Center).padding(Padding::ZERO.left(space::XS));
        if let Some(prefix) = prefix {
            let prefix = typo::text(typo::MICRO, prefix)
                .style(move |theme: &Theme| TextStyle {
                    color: Some(shade(palette(theme), palette(theme).text_2, faded)),
                })
                .wrapping(Wrapping::None);
            line = line.push(prefix);
        }
        line.push(title).into()
    };
    mouse_area(container(body).width(Length::Fill).height(EVENT_HEIGHT).clip(true))
        .on_press(Message::OpenEvent(item.index))
        .interaction(mouse::Interaction::Pointer)
        .into()
}

/// The width left for the title in an event row `width` wide: an all-day chip less its bar and
/// its padding; a timed row less its left padding, the dot, the time and the gaps between them.
fn title_room(all_day: bool, prefix: Option<&str>, width: f32) -> f32 {
    let room = if all_day {
        width - BAR - 2.0 * space::S
    } else {
        let prefix = prefix.map_or(0.0, |prefix| text_width(prefix, typo::MICRO) + space::XS);
        width - space::XS - DOT - space::XS - prefix
    };
    room.max(0.0)
}

/// `color` as it is drawn on the panel: faded towards it when `faded`, like CSS `opacity`.
fn shade(p: &Palette, color: Color, faded: bool) -> Color {
    if faded { mix(p.panel, color, FADE_OPACITY) } else { color }
}

/// How much of the window background lies over a cell's panel: [`OUTSIDE_SHADE`] for a day of a
/// neighbouring month, [`WEEKEND_SHADE`] for a weekend of the month, none for a weekday.
fn cell_shade(in_month: bool, weekend: bool) -> Option<f32> {
    match (in_month, weekend) {
        (false, _) => Some(OUTSIDE_SHADE),
        (true, true) => Some(WEEKEND_SHADE),
        (true, false) => None,
    }
}

fn is_weekend(day: NaiveDate) -> bool {
    matches!(day.weekday(), Weekday::Sat | Weekday::Sun)
}

/// Whether `event` is over by `now`; an entry without length is over when it has started.
fn ends_before(event: &Event, now: NaiveDateTime) -> bool {
    let (start, end) = local_span(event);
    end.max(start) <= now
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

/// The text of an event row on `date` as (lead-in, title): no lead-in for all-day entries,
/// `14:00` where the entry starts, `…` where it continues from an earlier day.
fn entry_parts(event: &Event, date: NaiveDate) -> (Option<String>, String) {
    let title = if event.summary.trim().is_empty() { tr("(no title)").to_string() } else { event.summary.clone() };
    if event.all_day() {
        return (None, title);
    }
    let (start, _) = local_span(event);
    let prefix = if start.date() == date { format_time(start.time()) } else { "…".to_string() };
    (Some(prefix), title)
}

/// How many rows a cell of `height` holds: below the day number, above the bottom edge, at most
/// [`MAX_LINES`].
fn lines_that_fit(height: f32) -> usize {
    let free = height - space::S - DAY_CIRCLE - space::XS - space::XS;
    if free < EVENT_HEIGHT { 0 } else { (((free + LINE_GAP) / (EVENT_HEIGHT + LINE_GAP)) as usize).min(MAX_LINES) }
}

/// How many of `total` entries a cell with `lines` rows lists, and how many it sums up as
/// `+n more`. The summary line takes a row of its own; a cell without rows shows nothing.
fn visible_split(total: usize, lines: usize) -> (usize, usize) {
    if lines == 0 {
        (0, 0)
    } else if total <= lines {
        (total, 0)
    } else {
        (lines - 1, total - (lines - 1))
    }
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
    fn rows_show_start_time_or_continuation() {
        let review = event("Review", at(11, 14), None);
        assert_eq!(entry_parts(&review, d(11)), (Some("14:00".into()), "Review".into()));
        assert_eq!(entry_parts(&review, d(12)), (Some("…".into()), "Review".into()));
        let holiday = all_day("Holiday", 11, None);
        assert_eq!(entry_parts(&holiday, d(11)), (None, "Holiday".into()));
    }

    #[test]
    fn titles_get_what_is_left_of_the_row() {
        assert_eq!(title_room(true, None, 100.0), 100.0 - BAR - 2.0 * space::S);
        let timed = title_room(false, Some("14:00"), 100.0);
        assert!(timed < 100.0 - DOT - 2.0 * space::XS - space::XS, "the time takes its share");
        assert_eq!(title_room(false, Some("14:00"), 10.0), 0.0, "never negative");
    }

    #[test]
    fn overflow_keeps_the_rows_including_the_summary() {
        assert_eq!(visible_split(0, 4), (0, 0));
        assert_eq!(visible_split(4, 4), (4, 0));
        assert_eq!(visible_split(5, 4), (3, 2));
        assert_eq!(visible_split(9, 4), (3, 6));
        assert_eq!(visible_split(3, 2), (1, 2));
        assert_eq!(visible_split(1, 1), (1, 0));
        assert_eq!(visible_split(3, 0), (0, 0), "no room, no rows");
    }

    #[test]
    fn rows_that_fit_follow_the_cell_height() {
        // Number box 8 + 22, gap 4, bottom 4 leave 38 px of 120: four rows of 20 with 2 between.
        assert_eq!(lines_that_fit(38.0 + 4.0 * 20.0 + 3.0 * 2.0), 4);
        assert_eq!(lines_that_fit(38.0 + 4.0 * 20.0 + 3.0 * 2.0 - 1.0), 3);
        assert_eq!(lines_that_fit(38.0 + 19.0), 0);
        assert_eq!(lines_that_fit(500.0), MAX_LINES);
        assert_eq!(lines_that_fit(0.0), 0);
    }

    #[test]
    fn events_are_over_when_they_end_before_now() {
        let now = d(11).and_hms_opt(10, 0, 0).unwrap();
        assert!(ends_before(&event("Early", at(11, 9), Some(at(11, 10))), now));
        assert!(ends_before(&event("Call", at(11, 9), None), now), "an instant is over once it has started");
        assert!(!ends_before(&event("Running", at(11, 9), Some(at(11, 11))), now));
        assert!(!ends_before(&all_day("Today", 11, None), now));
        assert!(ends_before(&all_day("Yesterday", 10, None), now));
    }

    #[test]
    fn neighbouring_months_are_dimmed_more_than_weekends() {
        assert_eq!(cell_shade(true, false), None);
        assert_eq!(cell_shade(true, true), Some(WEEKEND_SHADE));
        assert_eq!(cell_shade(false, false), Some(OUTSIDE_SHADE));
        assert_eq!(cell_shade(false, true), Some(OUTSIDE_SHADE));
    }

    #[test]
    fn weekend_is_saturday_and_sunday() {
        // 10 and 11 October 2026 are Saturday and Sunday.
        let days: Vec<bool> = (5..=11).map(|day| is_weekend(d(day))).collect();
        assert_eq!(days, [false, false, false, false, false, true, true]);
    }

    #[test]
    fn faded_colours_move_towards_the_panel_in_both_themes() {
        for p in [Palette::dark(), Palette::light()] {
            assert_eq!(shade(p, p.text, false), p.text);
            let faded = shade(p, p.text, true);
            assert!((faded.r - mix(p.panel, p.text, FADE_OPACITY).r).abs() < 1e-6);
            assert!((faded.r - p.panel.r).abs() < (p.text.r - p.panel.r).abs());
            let tint = over(p.tinted(p.calendar[0]), p.panel);
            assert_eq!(tint.a, 1.0, "composed, so the bar does not shine through");
        }
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
