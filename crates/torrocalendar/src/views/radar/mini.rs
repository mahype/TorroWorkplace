//! Mini radars of the coming days (task K.30 in `docs/tasks/kalender-aufgaben.md`): beside the
//! radar (in the properties column when wide, at the right when medium, under the dial when
//! narrow) a small dial for each of [`DAYS`] days shows at a glance how full that day is.
//!
//! The tiles are a page of their own (task K.35): it starts the day after the shown one, but a
//! click on a tile leaves it where it is and only marks the tile – a list that rebuilt itself
//! around every click lost the user. It moves on when the shown day leaves it ([`page`]), and its
//! own ‹ › browse it a week at a time (`Message::ComingPage`) without moving the radar.
//!
//! Each tile is a button: "Wed 14" above (or "Tomorrow"), under it a simplified dial of the same
//! twelve hours the large radar opens that day with (`radar::start`, unturned), so a click –
//! `Message::GoTo(day)` – shows exactly that. One ring holds all calendars, overlaps in at most
//! [`LANES`] lanes; events are arcs in their calendar's colour ([`arc_colour`]), the night a sector
//! behind the ring, ticks only at 12, 3, 6 and 9. Nothing fades, no numerals, no text on the arcs.
//! In the middle the number of timed events that day (or "–"), under it a dot per all-day event.

use chrono::{Datelike, Duration, NaiveDate};
use iced::widget::canvas::{self, Frame};
use iced::widget::{Space, button, column, container, row, space as blank, stack};
use iced::{Color, Element, Length, Point, Rectangle, Theme, mouse};
use torro_core::i18n::{tr, trf};
use torro_core::settings::Night;
use torro_gui::datetime::{format_day_month, month_name, weekday_name};
use torro_gui::icon::Icon;
use torro_gui::style;
use torro_gui::theme::{self, Palette, mix, palette, size, space};
use torro_gui::typo;
use torro_gui::widgets;

use super::layout::{self, Dial, END, Window, split};
use super::{ListAt, TICK, TRACK, line, sector};
use crate::app::Message;
use crate::views::ViewData;
use crate::views::glance::{self, Entry};

/// How many days a page of mini radars holds; ‹ › browse by as many.
pub const DAYS: i64 = 7;
/// Columns of tiles beside the dial (wide: properties column; medium: beside the dial).
pub const BESIDE_COLUMNS: usize = 2;
/// Under the narrow dial: as many columns as fit, each tile at least this wide.
pub const MIN_TILE: f32 = 104.0;
/// Overlapping events share the ring in at most this many lanes; a third lies over the second.
pub const LANES: usize = 2;
/// At most this many all-day dots under the count.
const ALL_DAY_DOTS: usize = 3;
/// Share of the calendar colour over the ring's track in an event's arc in light mode, for the
/// large and the mini dials alike (task K.29, set by screenshot): busy time stands out clearly from
/// the track, and dark text keeps 4.5:1 on it for every calendar colour.
pub const FILL_LIGHT: f32 = 0.45;
/// The same in dark mode. More would leave mid-light colours (the default `#7FB2FF`, orange) too
/// light for light text and too dark for dark text.
pub const FILL_DARK: f32 = 0.50;
/// The width of the ring (`docs/tasks`: about 12).
fn ring() -> f32 {
    space::m()
}
/// The ticks at 12, 3, 6 and 9 hang in from the edge of the dial this far…
fn tick_length() -> f32 {
    space::xs()
}
/// …and the ring keeps this far from them.
const TICK_GAP: f32 = 2.0;
/// Between two lanes of the ring.
const LANE_GAP: f32 = 1.0;
/// An arc starts and ends this far inside its times (degrees), so neighbours stay apart…
const ARC_INSET: f32 = 0.8;
/// …and is at least this long (degrees), so a short event still shows.
const ARC_MIN: f32 = 3.0;
/// The start edge reaches this far over the ring on both sides.
const EDGE_OVERHANG: f32 = 2.0;

/// [`FILL_DARK`] or [`FILL_LIGHT`], as `p` is the dark or the light palette.
pub fn fill(p: &Palette) -> f32 {
    if p.panel.relative_luminance() < 0.5 { FILL_DARK } else { FILL_LIGHT }
}

/// The colour of an event's arc on a mini dial in calendar colour `colour` over `ground` (the
/// panel, or the night): [`fill`] of the colour over the ring's neutral track (one ring holds every
/// calendar here), so busy time stands out clearly from the empty track.
pub fn arc_colour(p: &Palette, colour: Color, ground: Color) -> Color {
    mix(mix(ground, p.text, TRACK), colour, fill(p))
}

/// The days of the page starting on `first`, in order (over the end of a month or year like any
/// other day).
pub fn days(first: NaiveDate) -> Vec<NaiveDate> {
    (0..DAYS).map(|n| first + Duration::days(n)).collect()
}

/// The first day of the page once the radar shows `shown`, the page starting on `first` before:
/// it stays while `shown` is one of its days or the day before them (where it starts on its own),
/// so a click on a tile does not move the tiles; else it starts the day after `shown` again.
pub fn page(first: NaiveDate, shown: NaiveDate) -> NaiveDate {
    if first - Duration::days(1) <= shown && shown < first + Duration::days(DAYS) {
        first
    } else {
        shown + Duration::days(1)
    }
}

/// How many columns of tiles `width` takes `at` its place: [`BESIDE_COLUMNS`] beside the dial,
/// under it as many tiles of at least [`MIN_TILE`] (with [`space::s()`] between) as fit, at least one.
pub fn columns(at: ListAt, width: f32) -> usize {
    match at {
        ListAt::Props | ListAt::Beside => BESIDE_COLUMNS,
        ListAt::Below => (((width + space::s()) / (MIN_TILE + space::s())).floor() as usize).max(1),
    }
}

/// The section "Coming days": a mini radar for each of the [`DAYS`] days of a page, built once per
/// view and drawn with [`Coming::view`].
#[derive(Debug, Clone)]
pub struct Coming {
    days: Vec<Day>,
    /// The page starts the day after today: the title says "Coming days", else the dates.
    from_tomorrow: bool,
}

impl Coming {
    /// The mini radars of the page starting on `first` over the events of `data` (load them
    /// through the end of its last day), each window where the large radar opens that day with
    /// `night`; the tile of `data.anchor`, if on the page, is marked as shown.
    pub fn new(data: &ViewData<'_>, first: NaiveDate, night: Night) -> Coming {
        let entries = glance::entries(&data.items);
        let today = data.now.date();
        let days = days(first).into_iter().map(|date| Day::new(data, &entries, date, today, night)).collect();
        Coming { days, from_tomorrow: first == today + Duration::days(1) }
    }

    /// The title: "Coming days" for the page after today, else its first and last day
    /// ("20 Oct – 26 Oct"), so a browsed page says where it is.
    fn title(&self) -> String {
        match (self.from_tomorrow, self.days.first(), self.days.last()) {
            (false, Some(first), Some(last)) => {
                format!("{} – {}", format_day_month(first.date), format_day_month(last.date))
            }
            _ => tr("Coming days").to_string(),
        }
    }

    /// The section `width` wide: its title with ‹ › to browse the pages, then the tiles in
    /// `columns` columns, [`space::s()`] apart; the last row keeps the width of the others.
    pub fn view(&self, width: f32, columns: usize) -> Element<'static, Message> {
        let columns = columns.max(1);
        let tile_width = ((width - space::s() * (columns as f32 - 1.0)) / columns as f32).floor().max(0.0);
        let rows: Vec<Element<'static, Message>> = self
            .days
            .chunks(columns)
            .map(|chunk| {
                let mut tiles: Vec<Element<'static, Message>> =
                    chunk.iter().map(|day| tile(day.clone(), tile_width)).collect();
                tiles.resize_with(columns, || Space::new().width(tile_width).into());
                row(tiles).spacing(space::s()).into()
            })
            .collect();
        let head = row![
            typo::text(typo::caption(), self.title()).style(theme::text_3),
            blank::horizontal(),
            widgets::icon_button(Icon::ChevronLeft, tr("Previous week"), Some(Message::ComingPage(-1))),
            widgets::icon_button(Icon::ChevronRight, tr("Next week"), Some(Message::ComingPage(1))),
        ]
        .height(size::control_sm())
        .align_y(iced::Alignment::Center);
        // The title keeps the text edge of `prop_section`'s, the arrows end at the tiles' edge.
        let head = container(head).padding(iced::Padding { left: style::ITEM_PADDING.left, ..iced::Padding::ZERO });
        column![head, column(rows).spacing(space::s())].spacing(space::xs()).into()
    }
}

/// One day's mini radar.
#[derive(Debug, Clone, PartialEq)]
struct Day {
    date: NaiveDate,
    /// The day is today or tomorrow: the tile says so instead of the weekday.
    today: bool,
    tomorrow: bool,
    /// The radar shows this day: its tile is marked.
    shown: bool,
    /// The twelve hours the large radar opens this day with.
    window: Window,
    /// The timed events in the window, in their lanes.
    arcs: Vec<MiniArc>,
    /// The nights in the window ([`layout::nights`]).
    nights: Vec<(f32, f32)>,
    /// How many timed events start this day; all-day ones are not counted.
    count: usize,
    /// The all-day events of the day: colour and title.
    all_day: Vec<(Color, String)>,
}

/// A timed event on a mini dial: minutes into the window (cut to `[0, END]`), its lane of how
/// many, its calendar's colour.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MiniArc {
    from: f32,
    to: f32,
    lane: usize,
    lanes: usize,
    colour: Color,
}

impl Day {
    fn new(data: &ViewData<'_>, entries: &[Entry], date: NaiveDate, today: NaiveDate, night: Night) -> Day {
        // The large radar's own start for this day, so the click opens exactly this window.
        let window = Window::new(super::start(&ViewData { anchor: date, ..data.clone() }, 0, night));
        let timed: Vec<&Entry> = entries.iter().filter(|e| !e.all_day).collect();
        let spans: Vec<(f32, f32, Color)> = timed
            .iter()
            .map(|e| (window.rel(e.start), window.rel(e.end), e.color))
            .filter(|(from, to, _)| *to > 0.0 && *from < END)
            .map(|(from, to, colour)| (from.max(0.0), to.min(END), colour))
            .collect();
        let intervals: Vec<(f32, f32)> = spans.iter().map(|(from, to, _)| (*from, *to)).collect();
        let arcs = spans
            .iter()
            .zip(lanes(&intervals))
            .map(|(&(from, to, colour), (lane, lanes))| MiniArc { from, to, lane, lanes, colour })
            .collect();
        let all_day = entries
            .iter()
            .filter(|e| e.all_day && e.on(date))
            .take(ALL_DAY_DOTS)
            .map(|e| (e.color, e.title.clone()))
            .collect();
        Day {
            date,
            today: date == today,
            tomorrow: date == today + Duration::days(1),
            shown: date == data.anchor,
            window,
            arcs,
            nights: layout::nights(&window, night),
            count: count(entries, date),
            all_day,
        }
    }

    /// What a click on the tile does: show this day in the radar, its dial unturned.
    fn message(&self) -> Message {
        Message::GoTo(self.date)
    }

    /// "Wed 14", or "Today" / "Tomorrow".
    fn label(&self) -> String {
        if self.today {
            tr("Today").to_string()
        } else if self.tomorrow {
            tr("Tomorrow").to_string()
        } else {
            trf("{weekday} {day}", &[("weekday", &weekday_name(self.date, true)), ("day", &self.date.day())])
        }
    }

    /// The tooltip: "Wednesday, 14 October · 3 events", then the all-day titles.
    fn tooltip(&self) -> String {
        let date = trf(
            "{weekday}, {day} {month}",
            &[
                ("weekday", &weekday_name(self.date, false)),
                ("day", &self.date.day()),
                ("month", &month_name(self.date.month())),
            ],
        );
        let events = match self.count {
            0 => tr("No events").to_string(),
            1 => tr("1 event").to_string(),
            n => trf("{n} events", &[("n", &n)]),
        };
        let mut parts = vec![date, events];
        parts.extend(self.all_day.iter().map(|(_, title)| title.clone()));
        parts.join(" · ")
    }
}

/// The timed events of `entries` starting on `date`: the number in the middle of its dial.
fn count(entries: &[Entry], date: NaiveDate) -> usize {
    entries.iter().filter(|e| !e.all_day && e.start.date() == date).count()
}

/// Lanes for overlapping `spans` as [`glance::lanes`] gives them, but at most [`LANES`]: a mini
/// dial has no room for more, and what does not fit lies over the last lane.
fn lanes(spans: &[(f32, f32)]) -> Vec<(usize, usize)> {
    glance::lanes(spans).into_iter().map(|(lane, lanes)| (lane.min(LANES - 1), lanes.min(LANES))).collect()
}

/// One tile, `width` wide: the label, the dial with the count and the all-day dots over its
/// middle; hover lays `hover` under it and shows the hand, a click goes to the day. The tile of
/// the shown day has the `selected` surface.
fn tile(day: Day, width: f32) -> Element<'static, Message> {
    let edge = (width - 2.0 * space::s()).max(0.0);
    let label = typo::text(typo::caption(), day.label()).style(theme::text_2);
    let count = if day.count == 0 {
        typo::text(typo::body_strong(), "–").style(theme::text_3)
    } else {
        typo::text(typo::body_strong().tabular(), day.count.to_string()).style(theme::text)
    };
    let dots = row(day.all_day.iter().map(|(colour, _)| widgets::color_dot(*colour, space::xs()))).spacing(space::xs());
    let middle =
        column![count, container(dots).height(space::xs())].spacing(space::xs()).align_x(iced::Alignment::Center);
    let (tooltip, message, shown) = (day.tooltip(), day.message(), day.shown);
    let dial = stack![
        iced::widget::canvas(MiniDial { day }).width(edge).height(edge),
        container(middle).center(Length::Fixed(edge)),
    ];
    let content = column![label, dial].spacing(space::xs()).align_x(iced::Alignment::Center);
    let tile =
        button(content).width(width).padding(space::s()).style(style::button::sidebar_item(shown)).on_press(message);
    widgets::with_tooltip(tile, tooltip)
}

/// The radii of a mini dial filling a square canvas of `edge`: the outer edge (where the ticks
/// start), the ring inside the ticks.
fn dial(edge: f32) -> Dial {
    let radius = (edge / 2.0).max(0.0);
    let outer = (radius - tick_length() - TICK_GAP).max(0.0);
    let inner = (outer - ring()).max(0.0);
    Dial {
        center: Point::new(radius, radius),
        radius,
        numerals: radius,
        ticks: radius,
        outer,
        inner,
        band: outer - inner,
        rings: 1,
    }
}

/// The inner and outer radius of `lane` of `lanes` in the ring of `dial`, the first lane outside.
fn lane_radii(dial: &Dial, lane: usize, lanes: usize) -> (f32, f32) {
    let lanes = lanes.max(1) as f32;
    let width = (dial.band - LANE_GAP * (lanes - 1.0)) / lanes;
    let outer = dial.outer - lane as f32 * (width + LANE_GAP);
    (outer - width, outer)
}

/// A mini dial on a canvas.
struct MiniDial {
    day: Day,
}

impl canvas::Program<Message> for MiniDial {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let p = palette(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        let dial = dial(bounds.width.min(bounds.height));
        if dial.inner <= 0.0 {
            return vec![frame.into_geometry()];
        }
        let day = &self.day;
        let window = &day.window;

        // The night behind the ring, from its inner edge out to the ticks.
        for (from, to) in &day.nights {
            sector(&mut frame, &dial, dial.inner, dial.radius, window.angle(*from), window.angle(*to), p.night);
        }
        // The track, over the panel or the night.
        let (track, night_track) = (mix(p.panel, p.text, TRACK), mix(p.night, p.text, TRACK));
        for (from, to, night) in split(0.0, END, &day.nights) {
            let colour = if night { night_track } else { track };
            sector(&mut frame, &dial, dial.inner, dial.outer, window.angle(from), window.angle(to), colour);
        }
        // Ticks at 12, 3, 6 and 9 only.
        let tick = mix(p.panel, p.text, TICK);
        for angle in [0.0, 90.0, 180.0, 270.0] {
            line(&mut frame, dial.point(dial.radius, angle), dial.point(dial.radius - tick_length(), angle), tick, 1.0);
        }
        // The events.
        let in_night = |rel: f32| day.nights.iter().any(|(from, to)| (*from..*to).contains(&rel));
        for arc in &day.arcs {
            let (inner, outer) = lane_radii(&dial, arc.lane, arc.lanes);
            let from = window.angle(arc.from) + ARC_INSET;
            let to = (window.angle(arc.to) - ARC_INSET).max(from + ARC_MIN);
            let ground = if in_night(arc.from) { p.night } else { p.panel };
            sector(&mut frame, &dial, inner, outer, from, to, arc_colour(&p, arc.colour, ground));
        }
        // Where the window starts, in the quiet grey of the large radar away from now.
        let angle = window.angle0;
        let (from, to) = (dial.point(dial.inner - EDGE_OVERHANG, angle), dial.point(dial.outer + EDGE_OVERHANG, angle));
        line(&mut frame, from, to, p.text_3, 1.0);
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDateTime;
    use torro_pim::{Event as CalendarEvent, EventTime};

    use super::*;
    use crate::views::Item;

    /// Where the tile of `date` starts its window.
    fn window_start(coming: &Coming, date: NaiveDate) -> Option<NaiveDateTime> {
        coming.days.iter().find(|day| day.date == date).map(|day| day.window.start)
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("date")
    }

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        date(2026, 10, day).and_hms_opt(hour, minute, 0).expect("time")
    }

    fn local(day: u32, hour: u32, minute: u32) -> EventTime {
        EventTime::DateTime(at(day, hour, minute).and_local_timezone(chrono::Local).single().expect("local").to_utc())
    }

    fn event(calendar: &str, summary: &str, start: EventTime, end: EventTime) -> CalendarEvent {
        CalendarEvent {
            uid: summary.into(),
            collection: calendar.into(),
            href: None,
            etag: None,
            summary: summary.into(),
            start,
            end: Some(end),
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
            alarms: Vec::new(),
        }
    }

    /// Around Tuesday 13 October 2026, 14:20 (the demo's `--now`).
    fn events() -> Vec<CalendarEvent> {
        vec![
            event("work", "Standup", local(14, 9, 30), local(14, 9, 45)),
            event("work", "Review", local(14, 10, 0), local(14, 11, 0)),
            event("personal", "Dentist", local(14, 10, 30), local(14, 11, 30)),
            event("family", "Call", local(14, 10, 45), local(14, 11, 15)),
            event("personal", "Late call", local(14, 23, 0), local(15, 0, 30)),
            event("work", "Early train", local(16, 6, 30), local(16, 7, 30)),
            event("family", "Holiday", EventTime::Date(date(2026, 10, 14)), EventTime::Date(date(2026, 10, 15))),
        ]
    }

    /// The radar on `anchor` while it is Tuesday the 13th, 14:20.
    fn data(events: &[CalendarEvent], anchor: NaiveDate) -> ViewData<'_> {
        ViewData {
            items: events.iter().enumerate().map(|(index, event)| Item { index, event, color: Color::BLACK }).collect(),
            start: anchor - Duration::days(1),
            end: anchor + Duration::days(DAYS + 1),
            anchor,
            now: at(13, 14, 20),
            selected: None,
        }
    }

    /// The tiles of the page that starts the day after `anchor`, as the radar opens it.
    fn after(events: &[CalendarEvent], anchor: NaiveDate) -> Coming {
        Coming::new(&data(events, anchor), anchor + Duration::days(1), Night::DEFAULT)
    }

    #[test]
    fn a_page_holds_seven_days_also_over_month_and_year_end() {
        let days = days(date(2026, 10, 14));
        assert_eq!(days.len(), 7);
        assert_eq!((days[0], days[6]), (date(2026, 10, 14), date(2026, 10, 20)));
        assert_eq!(super::days(date(2026, 10, 29))[3..5], [date(2026, 11, 1), date(2026, 11, 2)]);
        let new_year = super::days(date(2026, 12, 30));
        assert_eq!((new_year[2], new_year[3], new_year[6]), (date(2027, 1, 1), date(2027, 1, 2), date(2027, 1, 5)));
        // The view builds them all, over the year end too.
        let events = events();
        let coming = after(&events, date(2026, 12, 29));
        assert_eq!(coming.days.iter().map(|day| day.date).collect::<Vec<_>>(), new_year);
    }

    #[test]
    fn the_page_stays_while_the_shown_day_is_on_it() {
        let first = date(2026, 10, 14);
        // The day before the page (where it starts on its own) and each of its days keep it.
        for shown in (-1..DAYS).map(|n| first + Duration::days(n)) {
            assert_eq!(page(first, shown), first, "{shown}");
        }
        // Leaving it, it starts the day after the shown one again.
        assert_eq!(page(first, date(2026, 10, 21)), date(2026, 10, 22));
        assert_eq!(page(first, date(2026, 10, 12)), date(2026, 10, 13));
        // A browsed page far ahead comes back to the shown day when it moves on.
        assert_eq!(page(date(2026, 11, 4), date(2026, 10, 14)), date(2026, 10, 15));
    }

    #[test]
    fn the_shown_day_is_marked_and_the_title_says_where_the_page_is() {
        let events = events();
        // On today the page after it is "Coming days", none of its tiles shown.
        let today = after(&events, date(2026, 10, 13));
        assert!(today.days.iter().all(|day| !day.shown));
        assert_eq!(today.title(), tr("Coming days"));
        // Thursday clicked on that page: the same tiles, Thursday marked.
        let thursday = Coming::new(&data(&events, date(2026, 10, 15)), date(2026, 10, 14), Night::DEFAULT);
        assert_eq!(
            thursday.days.iter().map(|day| day.date).collect::<Vec<_>>(),
            today.days.iter().map(|day| day.date).collect::<Vec<_>>()
        );
        let marked: Vec<NaiveDate> = thursday.days.iter().filter(|day| day.shown).map(|day| day.date).collect();
        assert_eq!(marked, [date(2026, 10, 15)]);
        assert_eq!(thursday.title(), tr("Coming days"));
        // A page browsed a week on names its first and last day.
        let browsed = Coming::new(&data(&events, date(2026, 10, 13)), date(2026, 10, 21), Night::DEFAULT);
        assert_eq!(
            browsed.title(),
            format!("{} – {}", format_day_month(date(2026, 10, 21)), format_day_month(date(2026, 10, 27)))
        );
        let _ = browsed.view(283.0, 2);
    }

    #[test]
    fn each_tile_opens_its_day_where_the_large_radar_does() {
        let events = events();
        let coming = after(&events, date(2026, 10, 13));
        for day in days(date(2026, 10, 14)) {
            let large = super::super::start(&data(&events, day), 0, Night::DEFAULT);
            assert_eq!(window_start(&coming, day), Some(large), "{day}");
        }
        // Wednesday at 08:00 (the standup at 09:30 is later), Friday at 06:00 for the early train.
        assert_eq!(window_start(&coming, date(2026, 10, 14)), Some(at(14, 8, 0)));
        assert_eq!(window_start(&coming, date(2026, 10, 16)), Some(at(16, 6, 0)));
        // A shown day in the past brings today among the tiles: it opens at now, like the radar.
        let yesterday = after(&events, date(2026, 10, 12));
        assert_eq!(window_start(&yesterday, date(2026, 10, 13)), Some(at(13, 14, 20)));
        let today = yesterday.days.first().expect("a tile");
        assert_eq!((today.today, today.label()), (true, tr("Today").to_string()));
    }

    #[test]
    fn a_click_on_a_tile_goes_to_its_day() {
        let events = events();
        let coming = after(&events, date(2026, 10, 13));
        let wednesday = &coming.days[0];
        assert!(matches!(wednesday.message(), Message::GoTo(day) if day == date(2026, 10, 14)));
        assert_eq!(wednesday.label(), tr("Tomorrow"));
        let thursday = &coming.days[1];
        assert_eq!(
            thursday.label(),
            trf("{weekday} {day}", &[("weekday", &weekday_name(thursday.date, true)), ("day", &15)])
        );
    }

    #[test]
    fn overlapping_events_share_the_ring_in_two_lanes_at_most() {
        assert_eq!(lanes(&[(0.0, 60.0), (30.0, 90.0)]), [(0, 2), (1, 2)]);
        // A third overlap lies over the second lane.
        assert_eq!(lanes(&[(0.0, 60.0), (10.0, 60.0), (20.0, 60.0)]), [(0, 2), (1, 2), (1, 2)]);
        assert_eq!(lanes(&[(0.0, 30.0), (30.0, 60.0)]), [(0, 1), (0, 1)], "touching is no overlap");
        let events = events();
        let coming = after(&events, date(2026, 10, 13));
        let wednesday = &coming.days[0];
        assert!(wednesday.arcs.iter().all(|arc| arc.lane < LANES && arc.lanes <= LANES));
        // Review, dentist and call overlap: two lanes, the call over the second.
        let lanes_of: Vec<(usize, usize)> = wednesday.arcs.iter().map(|arc| (arc.lane, arc.lanes)).collect();
        assert_eq!(lanes_of, [(0, 1), (0, 2), (1, 2), (1, 2)]);
        // Lanes split the ring across its width, the first outside.
        let dial = dial(120.0);
        let (first, second) = (lane_radii(&dial, 0, 2), lane_radii(&dial, 1, 2));
        assert_eq!(first.1, dial.outer);
        assert!(second.1 <= first.0 && (second.0 - dial.inner).abs() < 0.01, "{first:?} {second:?}");
        assert_eq!(lane_radii(&dial, 0, 1), (dial.inner, dial.outer));
    }

    #[test]
    fn the_count_takes_the_timed_events_of_the_day_but_not_all_day_ones() {
        let events = events();
        let coming = after(&events, date(2026, 10, 13));
        let wednesday = &coming.days[0];
        // Standup, review, dentist, call and the late call; the holiday is a dot.
        assert_eq!(wednesday.count, 5);
        assert_eq!(wednesday.all_day, [(Color::BLACK, "Holiday".to_string())]);
        assert_eq!(wednesday.arcs.len(), 4, "the late call at 23:00 lies beyond the window");
        let thursday = &coming.days[1];
        assert_eq!((thursday.count, thursday.all_day.len()), (0, 0), "the late call began on Wednesday");
        assert_eq!(thursday.tooltip(), format!("{} · {}", long(thursday.date), tr("No events")));
        assert_eq!(
            wednesday.tooltip(),
            format!("{} · {} · Holiday", long(wednesday.date), trf("{n} events", &[("n", &5)]))
        );
        assert!(coming.days[2].tooltip().ends_with(tr("1 event")));
        // The night lies behind each dial as in the large radar.
        assert_eq!(wednesday.nights, layout::nights(&wednesday.window, Night::DEFAULT));
    }

    fn long(date: NaiveDate) -> String {
        trf(
            "{weekday}, {day} {month}",
            &[("weekday", &weekday_name(date, false)), ("day", &date.day()), ("month", &month_name(date.month()))],
        )
    }

    #[test]
    fn columns_follow_the_width_under_the_dial_and_are_two_beside_it() {
        assert_eq!(columns(ListAt::Props, 283.0), 2);
        assert_eq!(columns(ListAt::Beside, 243.0), 2);
        // 480 wide less 2 × 16: four of at least 104 with 8 between.
        assert_eq!(columns(ListAt::Below, 448.0), 4);
        assert_eq!(columns(ListAt::Below, 104.0 * 3.0 + 16.0), 3);
        assert_eq!(columns(ListAt::Below, 104.0 * 3.0 + 15.0), 2);
        assert_eq!(columns(ListAt::Below, 80.0), 1, "never none");
        let events = events();
        let coming = after(&events, date(2026, 10, 13));
        for width in [80.0, 283.0, 448.0] {
            let _ = coming.view(width, columns(ListAt::Below, width));
        }
    }

    #[test]
    fn arcs_stand_out_from_the_track_in_both_modes() {
        for theme in [theme::theme(), theme::light()] {
            let p = palette(&theme);
            let blue = p.calendar[0];
            let track = mix(p.panel, p.text, TRACK);
            let arc = arc_colour(&p, blue, p.panel);
            let distance = (arc.r - track.r).abs() + (arc.g - track.g).abs() + (arc.b - track.b).abs();
            assert!(distance > 0.5, "{arc:?} on {track:?}");
        }
    }
}
