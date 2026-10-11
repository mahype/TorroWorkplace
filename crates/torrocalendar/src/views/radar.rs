//! Radar: twelve hours of a day on a clock face (tasks K.22 and K.23 in
//! `docs/tasks/kalender-aufgaben.md`, prototype `docs/prototypes/calendar/radar.html`, mode
//! "fade out", 3/4 visible).
//!
//! A 12-hour dial, 12 at the top, numbered with the real hours of the day (`15` where the 3
//! stands). The window starts at the start of the shown day – now for today, else 08:00 or the
//! hour of an earlier event ([`layout::day_start`]) – plus the turn of the dial. Its start is a
//! clear edge with the time in a chip outside, red at now and `text_2` anywhere else; the window
//! runs one turn: the first nine hours in full, the last three fading out to nothing just before
//! the edge. Iced has no masks, so the fading quarter is drawn in pieces of
//! two degrees, each with the opacity of its middle ([`layout::pieces`]).
//!
//! Every visible calendar gets a ring (task K.25), in the order of the sidebar, the first outside,
//! so the tracks show even on an empty day; fine hour lines run across them like the hour lines of
//! the grid. Overlapping events share their ring in lanes. Events are tinted arcs with a colour
//! bar where they start, past ones faded, the one open in the side panel outlined with the focus
//! ring. Titles run along the arc letter by letter (upright in the bottom half), with the time on
//! a second line where the lane is wide enough.
//!
//! The night (22:00–06:00, as in the timeline) lies behind the rings as a sector in the night
//! colour, a small moon on the numerals' circle at its first half hour. Free time of two hours or
//! more outside the night, from now on, says "3 h free" along the arc in the innermost ring. All of
//! it fades with the last quarter, piece by piece like the rest.
//!
//! The middle says the date and the time, what runs ("40 min left") or how long is free, what
//! comes next and what lasts all day; with nothing in the window, "Nothing planned" and when the
//! next event comes ("Free until tomorrow 09:30"); away from now, "Back to now". Beside the dial (in the
//! properties column when wide, at the right when medium, below when narrow) the events of the
//! next nine hours and of the fading quarter are listed.
//!
//! Clicks: an arc → `OpenEvent`, free room in the rings → `NewEvent` (rounded down to the quarter
//! hour), a list row → `OpenEvent`. The dial turns (`Turn`): the mouse wheel over it in quarter
//! hours (not in the narrow layout, where the wheel scrolls the page), and dragging it like a
//! knob – clockwise is later, a turn is twelve hours, in five-minute steps. A press only becomes
//! a drag after a few pixels, so a click stays a click.
//!
//! The geometry is in `radar/layout.rs`.

mod layout;

pub use layout::{HOUR_STEP, WHEEL_STEP, clamp_offset};

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use iced::alignment::Vertical;
use iced::border::Radius;
use iced::widget::canvas::{self, Action, Frame, Path, Stroke};
use iced::widget::scrollable::Direction;
use iced::widget::text::{Alignment, Wrapping};
use iced::widget::{button, column, container, responsive, row, rule, scrollable, stack};
use iced::{Color, Element, Event, Length, Padding, Point, Rectangle, Size, Theme, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::format_time;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, Palette, mix, over, palette, radius, size, space};
use torro_gui::typo::{self, ellipsize, line_height, text_width};
use torro_gui::{style, widgets};

use layout::{Arc, Dial, END, Hit, Span, VISIBLE, WINDOW, Window, opacity, pieces, split};

use super::glance::{self, span};
use super::{ViewData, short_date};
use crate::app::Message;

/// Where the radar lists its events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListAt {
    /// In the properties column of the wide layout ([`list`]); the view itself leaves it out.
    Props,
    /// At the right of the dial, [`LIST_MEDIUM`] wide (medium layout).
    Beside,
    /// Under the dial, the whole page scrolling (narrow layout).
    Below,
}

/// Width of the list beside the dial in the medium layout (`radar.html`: 260); the wide layout
/// uses the properties column.
const LIST_MEDIUM: f32 = 260.0;
/// The dial keeps this much room to the edges of its stage (`radar.html`: 48 in all).
const STAGE_MARGIN: f32 = space::XL;
/// The narrow dial is never wider than this (`radar.html`: 440).
const NARROW_DIAL: f32 = 440.0;
/// The middle keeps this far from the inner ring on each side (`radar.html`: 16).
const MIDDLE_INSET: f32 = space::L;
/// The track of a ring: `text` at 7 % over the panel.
const TRACK: f32 = 0.07;
/// Ticks: `text` at 35 % over the panel.
const TICK: f32 = 0.35;
/// Length of a full-hour tick and of a half-hour one.
const TICK_LONG: f32 = space::S;
const TICK_SHORT: f32 = space::XS;
/// The start edge reaches this far inside the rings and this far out over the ticks.
const EDGE_INSIDE: f32 = space::S;
const EDGE_OUTSIDE: f32 = 2.0;
/// Width of the start edge (`radar.html`: 1.5).
const EDGE_WIDTH: f32 = 1.5;
/// The colour bar at the start of an event, as in the week grid.
const BAR: f32 = 3.0;
/// The bar reaches 1 px over the arc on both sides.
const BAR_OVERHANG: f32 = 1.0;
/// The focus ring stands this far off the arc (radial; along the arc in degrees).
const RING_OFFSET: f32 = 3.0;
const RING_OFFSET_DEGREES: f32 = 0.8;
/// Titles only in lanes at least this wide; the time as a second line from [`TWO_LINES`] on.
const ONE_LINE: f32 = 22.0;
const TWO_LINES: f32 = 40.0;
/// Text along an arc starts this far after its start (the bar plus 6, as in the week grid) and
/// keeps this far from its end.
const TEXT_START: f32 = 9.0;
const TEXT_END: f32 = 6.0;
/// Shorter than this a title is not worth starting.
const TEXT_MIN: f32 = 24.0;
/// A title cut shorter than this many letters is left out, as in the week grid.
const MIN_TITLE_LETTERS: usize = 4;
/// Past events are faded like in the week grid.
const PAST_OPACITY: f32 = 0.55;
/// Steps of the arcs drawn as polygons: one corner per degree at most.
const DEGREES_PER_CORNER: f32 = 1.0;
/// Pixels of a smooth (touchpad) scroll that make one wheel step.
const PIXELS_PER_STEP: f32 = 40.0;
/// The time chip at the start edge: the week grid's now label (500, 11 on 16).
const CHIP: typo::Level =
    typo::Level { font: theme::FONT_TABULAR_MEDIUM, line_height: typo::CAPTION.line_height, ..typo::MICRO };
const CHIP_WIDTH: f32 = size::AXIS_NARROW - 2.0 * space::XS;
/// The large numerals at 3, 6, 9 and 12: 13/500.
const MAJOR: typo::Level = typo::BODY_STRONG.tabular();
/// Opacity of the dot of a fading event in the list.
const DIM_DOT: f32 = 0.55;
/// The middle shows at most this many all-day chips.
const CHIPS: usize = 2;
/// While dragging, the pointer this close to the centre does not turn the dial: there the
/// direction swings at the smallest move.
const DRAG_DEAD: f32 = space::XL;
/// Free time is named from two hours on, as in the week grid and the timeline.
const FREE_MINUTES: f32 = 120.0;

/// The radar for the day `data.anchor`, its dial turned `offset` minutes from the start of that
/// day ([`start`]), a ring for each of `calendars` (the visible ones' collection ids, in the order
/// of the sidebar) whether or not it has events, its list `at`.
pub fn view<'a>(data: ViewData<'a>, calendars: &[String], offset: i64, at: ListAt) -> Element<'a, Message> {
    let plan = Plan::new(&data, calendars, offset);
    match at {
        ListAt::Props => stage(plan),
        ListAt::Beside => {
            let width = LIST_MEDIUM - size::HAIRLINE;
            let list = scrollable(container(list_view(&plan, width - 2.0 * space::S)).padding(Padding {
                top: space::L,
                right: space::S,
                bottom: space::XL,
                left: space::S,
            }))
            .direction(Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(width)
            .height(Length::Fill);
            row![stage(plan), rule::vertical(1).style(style::rule), list].height(Length::Fill).into()
        }
        ListAt::Below => responsive(move |bounds| narrow(plan.clone(), bounds.width)).into(),
    }
}

/// Where the window starts: the start of the day `data.anchor` ([`layout::day_start`] over the
/// timed events of `data`: now for today, else 08:00 or the full hour of an earlier event, never
/// before 06:00) plus `offset` minutes of turning (kept to [`clamp_offset`]). The header names the
/// day of this time.
pub fn start(data: &ViewData<'_>, offset: i64) -> NaiveDateTime {
    let starts = data.items.iter().map(glance::Entry::new).filter(|entry| !entry.all_day).map(|entry| entry.start);
    layout::day_start(data.anchor, data.now, starts) + Duration::minutes(clamp_offset(offset))
}

/// The list for the properties column of the wide layout: the same as beside the dial.
pub fn list(data: &ViewData<'_>, calendars: &[String], offset: i64) -> Element<'static, Message> {
    let plan = Plan::new(data, calendars, offset);
    // The column pads its content by 8 on each side and has a line on the left.
    list_view(&plan, size::PROPS - size::HAIRLINE - 2.0 * space::S)
}

/// The dial filling its stage, the middle laid over it.
fn stage(plan: Plan) -> Element<'static, Message> {
    responsive(move |bounds| {
        let dial = Dial::fit(bounds, STAGE_MARGIN, plan.rings);
        let canvas = iced::widget::canvas(Radar { plan: plan.clone(), margin: STAGE_MARGIN, wheel: true })
            .width(Length::Fill)
            .height(Length::Fill);
        let middle = container(middle(&plan, &dial)).center(Length::Fill);
        stack![canvas, middle].into()
    })
    .into()
}

/// Narrow: the dial as wide as the window allows (at most [`NARROW_DIAL`]), the list under it, the
/// whole page scrolling. The wheel scrolls the page, not the time.
fn narrow(plan: Plan, width: f32) -> Element<'static, Message> {
    let edge = (width - 2.0 * space::S).clamp(0.0, NARROW_DIAL) + 2.0 * space::S;
    let dial = Dial::fit(Size::new(width, edge), space::S, plan.rings);
    let canvas = iced::widget::canvas(Radar { plan: plan.clone(), margin: space::S, wheel: false })
        .width(Length::Fill)
        .height(Length::Fixed(edge));
    let middle = container(middle(&plan, &dial)).center_x(Length::Fill).center_y(Length::Fixed(edge));
    let list = container(list_view(&plan, width - 2.0 * space::L)).padding(space::L);
    scrollable(column![stack![canvas, middle], rule::horizontal(1).style(style::rule), list])
        .direction(Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// An event as the radar sees it.
#[derive(Debug, Clone)]
struct Entry {
    /// Index into the app's event list, for `OpenEvent`.
    index: usize,
    /// The collection id, for its ring.
    calendar: String,
    /// Local times, title, colour, all-day (as the glance sees it).
    event: glance::Entry,
}

impl Entry {
    fn timed(&self) -> bool {
        !self.event.all_day
    }
}

/// Everything the radar draws, built once per view. Entry ids are positions in `entries`.
#[derive(Debug, Clone)]
struct Plan {
    window: Window,
    /// The real now: what has ended by then is faded.
    now: NaiveDateTime,
    /// Away from now (another day, or the dial turned): the edge is grey instead of red, "Back to
    /// now" shows.
    looking: bool,
    entries: Vec<Entry>,
    /// The timed entries in the window, in their rings.
    spans: Vec<Span>,
    rings: usize,
    /// The nights in the window ([`layout::nights`]).
    nights: Vec<(f32, f32)>,
    /// The free time of two hours or more from now on ([`layout::free`]).
    free: Vec<(f32, f32)>,
    /// The app's index of the event open in the side panel.
    selected: Option<usize>,
}

impl Plan {
    fn new(data: &ViewData<'_>, calendars: &[String], offset: i64) -> Plan {
        let window = Window::new(start(data, offset));
        let mut entries: Vec<Entry> = data
            .items
            .iter()
            .map(|item| Entry {
                index: item.index,
                calendar: item.event.collection.clone(),
                event: glance::Entry::new(item),
            })
            .collect();
        entries.sort_by(|a, b| {
            (a.event.start, a.event.end, &a.event.title).cmp(&(b.event.start, b.event.end, &b.event.title))
        });
        let inside =
            |entry: &Entry| entry.timed() && window.rel(entry.event.end) > 0.0 && window.rel(entry.event.start) < END;
        // Every visible calendar has its ring, and a calendar the list does not know (an event
        // still loaded from a calendar just removed) gets one while it has events in the window.
        let used = entries.iter().filter(|e| inside(e)).map(|e| e.calendar.as_str());
        let rings = ring_order(calendars.iter().map(String::as_str).chain(used), calendars);
        let spans = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| inside(entry))
            .filter_map(|(id, entry)| {
                let ring = rings.iter().position(|calendar| *calendar == entry.calendar)?;
                let (from, to) = (window.rel(entry.event.start), window.rel(entry.event.end));
                Some(Span { id, ring, from: from.max(0.0), to: to.min(END), starts: from >= 0.0 })
            })
            .collect::<Vec<Span>>();
        let nights = layout::nights(&window);
        let free = layout::free(&spans, &nights, window.rel(data.now), FREE_MINUTES);
        Plan {
            window,
            now: data.now,
            looking: data.anchor != data.now.date() || offset != 0,
            entries,
            spans,
            rings: rings.len(),
            nights,
            free,
            selected: data.selected,
        }
    }

    fn is_selected(&self, id: usize) -> bool {
        self.selected.is_some_and(|index| self.entries.get(id).is_some_and(|entry| entry.index == index))
    }

    /// The timed event running at the start of the window: of several the one begun last.
    fn running(&self) -> Option<&Entry> {
        let at = self.window.start;
        self.entries
            .iter()
            .filter(|e| e.timed() && e.event.start <= at && at < e.event.end)
            .max_by(|a, b| a.event.start.cmp(&b.event.start).then(b.event.end.cmp(&a.event.end)))
    }

    /// The first timed event starting after the start of the window.
    fn next(&self) -> Option<&Entry> {
        self.entries.iter().find(|e| e.timed() && e.event.start > self.window.start)
    }

    /// [`Plan::next`] if it starts in the window.
    fn next_inside(&self) -> Option<&Entry> {
        self.next().filter(|e| self.window.rel(e.event.start) < END)
    }

    /// What lasts all day on the day of the window's start.
    fn all_day(&self) -> Vec<&Entry> {
        let day = self.window.start.date();
        self.entries.iter().filter(|e| e.event.all_day && e.event.on(day)).collect()
    }

    /// The timed events of the window for the list: those starting in the first nine hours (and
    /// those running at its start), then those starting in the fading quarter.
    fn listed(&self) -> (Vec<usize>, Vec<usize>) {
        let mut sooner = Vec::new();
        let mut later = Vec::new();
        for span in &self.spans {
            let start = self.window.rel(self.entries[span.id].event.start);
            if start < VISIBLE { sooner.push(span.id) } else { later.push(span.id) }
        }
        (sooner, later)
    }
}

/// The calendars of `used` (each once) in the order of `calendars` (the sidebar's); ones it does
/// not know come last, by id.
fn ring_order<'a>(used: impl Iterator<Item = &'a str>, calendars: &[String]) -> Vec<String> {
    let mut rings: Vec<String> = Vec::new();
    for calendar in used {
        if !rings.iter().any(|ring| ring == calendar) {
            rings.push(calendar.to_string());
        }
    }
    let rank = |calendar: &String| calendars.iter().position(|c| c == calendar).unwrap_or(calendars.len());
    rings.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.cmp(b)));
    rings
}

/// The middle of the dial: date, time, what runs or how long is free, what comes next, the
/// all-day chips and – while looking ahead – "Back to now". Cut to the room inside the rings.
fn middle(plan: &Plan, dial: &Dial) -> Element<'static, Message> {
    let width = (2.0 * dial.inner - 2.0 * MIDDLE_INSET).max(0.0);
    let at = plan.window.start;
    let mut parts = column![
        typo::text(typo::META, short_date(at.date())).style(theme::text_3),
        typo::text(typo::DISPLAY.tabular(), format_time(at.time())),
    ]
    .spacing(space::XS)
    .align_x(iced::Alignment::Center);
    let mut detail = column![].spacing(space::XS).align_x(iced::Alignment::Center);
    if let Some(running) = plan.running() {
        let left = trf("{span} left", &[("span", &span(layout::minutes_until(at, running.event.end)))]);
        let title = ellipsize(&running.event.title, typo::BODY_STRONG, width - space::S - space::S);
        detail = detail
            .push(
                row![widgets::color_dot(running.event.color, space::S), one_line(typo::BODY_STRONG, title)]
                    .spacing(space::S)
                    .align_y(iced::Alignment::Center),
            )
            .push(one_line(typo::META, left).style(theme::text_2));
    } else if let Some(next) = plan.next_inside() {
        let free = trf("Free for {span}", &[("span", &span(layout::minutes_until(at, next.event.start)))]);
        detail =
            detail.push(one_line(typo::BODY_STRONG, ellipsize(&free, typo::BODY_STRONG, width)).style(theme::text_2));
    } else {
        // Nothing in the window: say so, and when the next event comes, wherever it lies.
        let nothing = ellipsize(tr("Nothing planned"), typo::BODY_STRONG, width);
        detail = detail.push(one_line(typo::BODY_STRONG, nothing).style(theme::text_2));
        if let Some(next) = plan.next() {
            let until = free_until(next.event.start, at.date(), plan.now.date());
            detail = detail.push(one_line(typo::META, ellipsize(&until, typo::META, width)).style(theme::text_3));
        }
    }
    if let Some(next) = plan.next_inside() {
        let line = trf(
            "Next {time} · {title}",
            &[("time", &format_time(next.event.start.time())), ("title", &next.event.title)],
        );
        detail = detail.push(one_line(typo::META, ellipsize(&line, typo::META, width)).style(theme::text_3));
    }
    parts = parts.push(detail);
    let chips: Vec<Element<'static, Message>> = plan
        .all_day()
        .into_iter()
        .take(CHIPS)
        .map(|entry| widgets::pill(Some(entry.event.color), ellipsize(&entry.event.title, typo::META, width / 2.0)))
        .collect();
    if !chips.is_empty() {
        parts = parts.push(row(chips).spacing(space::XS));
    }
    if plan.looking {
        parts =
            parts.push(widgets::text_button(None, tr("Back to now"), style::button::secondary, Some(Message::Today)));
    }
    container(parts.spacing(space::S)).width(width).center_x(width).into()
}

/// When the free time from the window's start on ends, at the start of the next event `next`:
/// "Free until 09:30" on `day` (the day of the window), "Free until tomorrow 09:30" on the day after
/// `today`, else "Free until Thu, 15 Oct at 09:30".
fn free_until(next: NaiveDateTime, day: NaiveDate, today: NaiveDate) -> String {
    let time = format_time(next.time());
    if next.date() == day {
        trf("Free until {time}", &[("time", &time)])
    } else if next.date() == today + Duration::days(1) {
        trf("Free until tomorrow {time}", &[("time", &time)])
    } else {
        trf("Free until {date} at {time}", &[("date", &short_date(next.date())), ("time", &time)])
    }
}

/// One line of text, never wrapped (the caller cuts it).
fn one_line(level: typo::Level, content: String) -> iced::widget::Text<'static> {
    typo::text(level, content).wrapping(Wrapping::None)
}

/// The list beside or under the dial, `width` wide: "Next 9 hours" and "Fading out, until 02:20",
/// a row per event with its time, colour dot and title; a click opens it.
fn list_view(plan: &Plan, width: f32) -> Element<'static, Message> {
    let (sooner, later) = plan.listed();
    let hours = (VISIBLE / 60.0) as u32;
    let until = format_time(plan.window.at(WINDOW).time());
    let section = |title: String, ids: Vec<usize>, dim: bool| {
        let rows: Vec<Element<'static, Message>> = if ids.is_empty() {
            vec![widgets::prop_row(None, typo::text(typo::BODY, tr("Nothing planned")).style(theme::text_3), None)]
        } else {
            ids.into_iter()
                .filter_map(|id| plan.entries.get(id))
                .map(|entry| list_row(plan, entry, width, dim))
                .collect()
        };
        widgets::prop_section(title, rows)
    };
    column![
        section(trf("Next {n} hours", &[("n", &hours)]), sooner, false),
        section(trf("Fading out, until {time}", &[("time", &until)]), later, true),
    ]
    .spacing(space::L)
    .into()
}

/// The time column of the list: as wide as the widest range.
fn time_width() -> f32 {
    text_width("00:00–00:00", typo::META)
}

/// A row of the list: "14:00–15:00", the colour dot, the title cut to the rest. A fading one
/// (`dim`) in `text_2` with a paler dot; the event open in the side panel is selected.
fn list_row(plan: &Plan, entry: &Entry, width: f32, dim: bool) -> Element<'static, Message> {
    let range = format!("{}–{}", format_time(entry.event.start.time()), format_time(entry.event.end.time()));
    let room = width - 2.0 * style::ITEM_PADDING.left - time_width() - 2.0 * space::S - space::S;
    let title = ellipsize(&entry.event.title, typo::BODY, room);
    let selected = plan.selected == Some(entry.index);
    let colour = entry.event.color;
    let content = row![
        container(typo::text(typo::META.tabular(), range).style(theme::text_3)).width(time_width()),
        widgets::color_dot(if dim { colour.scale_alpha(DIM_DOT) } else { colour }, space::S),
        one_line(typo::BODY, title).style(if dim { theme::text_2 } else { theme::text }),
    ]
    .spacing(space::S)
    .align_y(iced::Alignment::Center);
    button(container(content).center_y(Length::Fill))
        .width(Length::Fill)
        .height(size::CONTROL)
        .padding(style::ITEM_PADDING)
        .style(style::button::sidebar_item(selected))
        .on_press(Message::OpenEvent(entry.index))
        .into()
}

/// The dial on a canvas.
struct Radar {
    plan: Plan,
    /// Room to the edges of the canvas.
    margin: f32,
    /// The wheel turns the dial (not in the narrow layout, where it scrolls the page).
    wheel: bool,
}

/// What the canvas keeps between events.
#[derive(Debug, Default)]
struct Grip {
    /// Smooth scrolling (touchpads) comes in pixels; they add up to wheel steps.
    pixels: f32,
    /// The left button is down on the dial, outside an event.
    press: Option<Press>,
}

/// A press on the dial outside an event: a click (a new event in free room) until the pointer
/// moves [`layout::DRAG_THRESHOLD`], then a drag that turns the dial.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Press {
    /// Where it was pressed, in the canvas.
    origin: Point,
    /// What a click there means: free room starts an event; elsewhere on the dial nothing.
    hit: Option<Hit>,
    /// The direction of the pointer at the last move (degrees).
    angle: f32,
    dragging: bool,
    /// Minutes turned but not yet sent: less than a [`layout::DRAG_SNAP`] step.
    rest: f32,
}

impl Radar {
    fn dial(&self, bounds: Size) -> Dial {
        Dial::fit(bounds, self.margin, self.plan.rings)
    }

    fn hit(&self, bounds: Size, point: Point) -> Option<Hit> {
        let dial = self.dial(bounds);
        let arcs = layout::arcs(&dial, &self.plan.window, &self.plan.spans);
        layout::hit(&dial, &self.plan.window, &arcs, point)
    }

    /// The click on `hit` as a message: open the event, or a new one at the quarter hour.
    fn message(&self, hit: Hit) -> Option<Message> {
        match hit {
            Hit::Event(id) => Some(Message::OpenEvent(self.plan.entries.get(id)?.index)),
            Hit::Free(rel) => {
                let at = self.plan.window.at(rel);
                let minutes = at.time().num_seconds_from_midnight() / 60;
                let quarter = minutes - minutes % 15;
                let time = NaiveTime::from_hms_opt(quarter / 60, quarter % 60, 0)?;
                Some(Message::NewEvent { date: at.date(), time: Some(time) })
            }
        }
    }

    /// The left button goes down at `point`: an event opens at once; elsewhere on the dial the
    /// press is kept until it turns out to be a click or a drag.
    fn press(&self, state: &mut Grip, bounds: Size, point: Point) -> Option<Action<Message>> {
        let hit = self.hit(bounds, point);
        if let Some(Hit::Event(id)) = hit {
            let message = Message::OpenEvent(self.plan.entries.get(id)?.index);
            return Some(Action::publish(message).and_capture());
        }
        let dial = self.dial(bounds);
        if dial.outer <= 0.0 || !dial.grabbable(point) {
            return None;
        }
        let (_, angle) = dial.polar(point);
        state.press = Some(Press { origin: point, hit, angle, dragging: false, rest: 0.0 });
        Some(Action::capture())
    }

    /// The pointer moved to `point` with the button down: past the threshold the press is a drag,
    /// and the angle turned since the last move turns the dial (in whole snap steps, the rest
    /// carried on).
    fn drag(&self, state: &mut Grip, bounds: Size, point: Point) -> Option<Action<Message>> {
        let press = state.press.as_mut()?;
        if !press.dragging && !layout::is_drag(press.origin, point) {
            return None;
        }
        press.dragging = true;
        let (radius, angle) = self.dial(bounds).polar(point);
        if radius < DRAG_DEAD {
            return Some(Action::capture());
        }
        let turned = layout::minutes_of_turn(layout::turn_between(press.angle, angle)) + press.rest;
        let (minutes, rest) = layout::snap(turned);
        press.angle = angle;
        press.rest = rest;
        if minutes == 0 {
            return Some(Action::capture());
        }
        Some(Action::publish(Message::Turn(minutes)).and_capture())
    }

    /// The left button comes up: a press that never moved is a click.
    fn release(&self, state: &mut Grip) -> Option<Action<Message>> {
        let press = state.press.take()?;
        let click = if press.dragging { None } else { press.hit.and_then(|hit| self.message(hit)) };
        Some(click.map_or_else(Action::capture, |message| Action::publish(message).and_capture()))
    }

    /// Wheel steps from a scroll; down turns later, like the page would move on.
    fn steps(state: &mut Grip, delta: mouse::ScrollDelta) -> i64 {
        match delta {
            mouse::ScrollDelta::Lines { y, .. } => {
                state.pixels = 0.0;
                if y == 0.0 { 0 } else { -(y.signum() * y.abs().round().max(1.0)) as i64 }
            }
            mouse::ScrollDelta::Pixels { y, .. } => {
                state.pixels -= y;
                let steps = (state.pixels / PIXELS_PER_STEP).trunc();
                state.pixels -= steps * PIXELS_PER_STEP;
                steps as i64
            }
        }
    }
}

impl canvas::Program<Message> for Radar {
    type State = Grip;

    fn update(
        &self,
        state: &mut Grip,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                self.press(state, bounds.size(), cursor.position_in(bounds)?)
            }
            // A drag goes on beside the canvas: the knob turns as long as the button is down.
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.press.is_some() => {
                self.drag(state, bounds.size(), cursor.position_from(bounds.position())?)
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => self.release(state),
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if self.wheel => {
                cursor.position_in(bounds)?;
                let steps = Radar::steps(state, *delta);
                if steps == 0 {
                    return Some(Action::capture());
                }
                Some(Action::publish(Message::Turn(steps * WHEEL_STEP)).and_capture())
            }
            _ => None,
        }
    }

    /// The hand over events, an open hand over the rest of the dial (it can be turned), a closed
    /// one while it is.
    fn mouse_interaction(&self, state: &Grip, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.press.is_some_and(|press| press.dragging) {
            return mouse::Interaction::Grabbing;
        }
        let Some(point) = cursor.position_in(bounds) else { return mouse::Interaction::Idle };
        match self.hit(bounds.size(), point) {
            Some(Hit::Event(_)) => mouse::Interaction::Pointer,
            _ if self.dial(bounds.size()).grabbable(point) => mouse::Interaction::Grab,
            _ => mouse::Interaction::Idle,
        }
    }

    fn draw(
        &self,
        _state: &Grip,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let p = palette(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        let dial = self.dial(bounds.size());
        // While a window is resized the canvas can pass through sizes with no room for the rings
        // (zero radii would turn angles into infinities); there is nothing to draw then.
        if dial.outer <= 0.0 {
            return vec![frame.into_geometry()];
        }
        let plan = &self.plan;
        let window = &plan.window;
        let faded = |colour: Color, alpha: f32| mix(p.panel, colour, alpha);

        // The night behind the rings, from their inner edge out to the ticks.
        for (from, to) in &plan.nights {
            for piece in pieces(*from, *to) {
                let (start, end) = (window.angle(piece.from), seam(window, piece.to, *to));
                sector(&mut frame, &dial, dial.inner, dial.ticks, start, end, faded(p.night, piece.opacity));
            }
        }

        // The tracks of the rings, composed over the panel or the night.
        let (track, night_track) = (mix(p.panel, p.text, TRACK), mix(p.night, p.text, TRACK));
        let stretches = split(0.0, END, &plan.nights);
        for ring in 0..dial.rings {
            let (inner, outer) = dial.ring(ring);
            for &(from, to, night) in &stretches {
                for piece in pieces(from, to) {
                    let colour = faded(if night { night_track } else { track }, piece.opacity);
                    let (start, end) = (window.angle(piece.from), seam(window, piece.to, END));
                    sector(&mut frame, &dial, inner, outer, start, end, colour);
                }
            }
        }

        // Hour lines across the rings, like the hour lines of the grid.
        if dial.rings > 0 {
            for (rel, _) in window.ticks().into_iter().filter(|(_, full)| *full) {
                let angle = window.angle(rel);
                let (from, to) = (dial.point(dial.inner, angle), dial.point(dial.outer, angle));
                line(&mut frame, from, to, p.line.scale_alpha(opacity(rel)), size::HAIRLINE);
            }
        }

        // Ticks and numerals.
        let tick = faded(p.text, TICK);
        for (rel, full) in window.ticks() {
            let angle = window.angle(rel);
            let length = if full { TICK_LONG } else { TICK_SHORT };
            let from = dial.point(dial.ticks, angle);
            let to = dial.point(dial.ticks - length, angle);
            line(&mut frame, from, to, faded(tick, opacity(rel)), 1.0);
        }
        for numeral in window.numerals() {
            let (level, colour) = if numeral.major { (MAJOR, p.text) } else { (typo::MICRO, p.text_2) };
            let at = dial.point(dial.numerals, window.angle(numeral.rel));
            label(&mut frame, &format!("{:02}", numeral.hour), at, level, faded(colour, opacity(numeral.rel)));
        }

        // The events, then their titles, then the ring of the open one on top.
        let arcs = layout::arcs(&dial, window, &plan.spans);
        let tones = |entry: &Entry| [tone(p, entry, plan.now, p.panel), tone(p, entry, plan.now, p.night)];
        for arc in &arcs {
            let Some(entry) = plan.entries.get(arc.id) else { continue };
            event_arc(&mut frame, &dial, window, &plan.nights, arc, &tones(entry), &faded);
        }
        for arc in &arcs {
            let Some(entry) = plan.entries.get(arc.id) else { continue };
            let [day, night] = tones(entry);
            let starts_at_night = in_night(&plan.nights, window.rel_of(arc.from));
            arc_text(&mut frame, &dial, window, arc, entry, if starts_at_night { &night } else { &day }, &faded);
        }
        for arc in arcs.iter().filter(|arc| plan.is_selected(arc.id)) {
            let path = sector_path(
                &dial,
                arc.inner - RING_OFFSET,
                arc.outer + RING_OFFSET,
                arc.from - RING_OFFSET_DEGREES,
                arc.to + RING_OFFSET_DEGREES,
            );
            frame.stroke(&path, Stroke::default().with_color(p.focus_ring).with_width(style::FOCUS_RING_WIDTH));
        }

        moon(&mut frame, &dial, plan, p, &faded);
        free_time(&mut frame, &dial, plan, p, &faded);
        start_edge(&mut frame, &dial, plan, p);
        vec![frame.into_geometry()]
    }
}

/// The colours of an event, composed over its ground (the panel, or the night): tinted fill, full
/// bar, title in `text`, time in `text_2`; one that has ended faded as in the week grid.
struct Tone {
    fill: Color,
    bar: Color,
    title: Color,
    time: Color,
}

fn tone(p: &Palette, entry: &Entry, now: NaiveDateTime, ground: Color) -> Tone {
    let colour = entry.event.color;
    let past = entry.event.end <= now;
    let fade = |c: Color| if past { mix(ground, c, PAST_OPACITY) } else { c };
    // The tint lies over the ring's track, not the bare ground: over the panel a dark calendar
    // colour came out darker than an empty track, so busy time looked weaker than free time.
    let track = mix(ground, p.text, TRACK);
    Tone { fill: fade(over(p.tinted(colour), track)), bar: fade(colour), title: fade(p.text), time: fade(p.text_2) }
}

/// The end angle of a fill piece ending `to` minutes into the window, a quarter degree further so
/// neighbouring pieces overlap and no hairline of the panel shows between them; never past
/// `until`, where what it belongs to ends.
fn seam(window: &Window, to: f32, until: f32) -> f32 {
    window.angle(to).min(window.angle(until)) + if to < until { 0.25 } else { 0.0 }
}

/// `rel` minutes into the window lie in one of `nights`.
fn in_night(nights: &[(f32, f32)], rel: f32) -> bool {
    nights.iter().any(|(from, to)| (*from..*to).contains(&rel))
}

/// An event's arc, its fading part in pieces, each composed over its ground (`tones`: over the
/// panel, over the night), and its colour bar where it starts.
fn event_arc(
    frame: &mut Frame,
    dial: &Dial,
    window: &Window,
    nights: &[(f32, f32)],
    arc: &Arc,
    tones: &[Tone; 2],
    faded: &dyn Fn(Color, f32) -> Color,
) {
    let (from, to) = (window.rel_of(arc.from), window.rel_of(arc.to));
    for (start, end, night) in split(from, to, nights) {
        let tone = &tones[usize::from(night)];
        for piece in pieces(start, end) {
            let (a, b) = (window.angle(piece.from), seam(window, piece.to, to).min(arc.to));
            sector(frame, dial, arc.inner, arc.outer, a, b, faded(tone.fill, piece.opacity));
        }
    }
    if arc.starts {
        let width = (BAR / arc.middle()).to_degrees();
        let tone = &tones[usize::from(in_night(nights, from))];
        let colour = faded(tone.bar, opacity(from));
        sector(frame, dial, arc.inner - BAR_OVERHANG, arc.outer + BAR_OVERHANG, arc.from, arc.from + width, colour);
    }
}

/// The title along the arc, cut with "…", and below it the time where the lane takes two lines.
/// Each letter takes the opacity of the place it stands on.
fn arc_text(
    frame: &mut Frame,
    dial: &Dial,
    window: &Window,
    arc: &Arc,
    entry: &Entry,
    tone: &Tone,
    faded: &dyn Fn(Color, f32) -> Color,
) {
    let width = arc.width();
    if width < ONE_LINE {
        return;
    }
    let room = arc.length() - TEXT_START - TEXT_END;
    if room < TEXT_MIN {
        return;
    }
    let flipped = arc.flipped();
    // "Up" for the letters: outwards in the top half, inwards in the bottom half.
    let up = if flipped { -1.0 } else { 1.0 };
    let middle = arc.middle();
    let Some(title) = arc_title(&entry.event.title, room) else { return };
    let mut lines = vec![(title, typo::CAPTION, tone.title, 0.0)];
    if let Some(time) = arc_time(entry, room).filter(|_| width >= TWO_LINES) {
        // Title over time, the pair centred on the middle of the lane.
        let (title_h, time_h) = (line_height(typo::CAPTION), line_height(typo::MICRO));
        let half = (title_h + time_h) / 2.0;
        lines[0].3 = half - title_h / 2.0;
        lines.push((time, typo::MICRO, tone.time, time_h / 2.0 - half));
    }
    for (text, level, colour, lift) in lines {
        if text.is_empty() {
            continue;
        }
        let radius = middle + up * lift;
        let start = if flipped {
            arc.to - (TEXT_END / radius).to_degrees()
        } else {
            arc.from + (TEXT_START / radius).to_degrees()
        };
        for glyph in layout::glyphs(&text, level, radius, start, flipped) {
            let alpha = opacity(window.rel_of(glyph.angle));
            if alpha <= 0.002 {
                continue;
            }
            let at = dial.point(radius, glyph.angle);
            frame.with_save(|frame| {
                frame.translate(iced::Vector::new(at.x, at.y));
                frame.rotate(glyph.turn);
                label(frame, &glyph.letter, Point::ORIGIN, level, faded(colour, alpha));
            });
        }
    }
}

/// The title for an arc with `room` along it, cut with "…"; `None` when fewer than
/// [`MIN_TITLE_LETTERS`] of it would be left ("Da…" says nothing; the list has the title).
fn arc_title(title: &str, room: f32) -> Option<String> {
    let cut = ellipsize(title, typo::CAPTION, room);
    let letters = cut.trim_end_matches(typo::ELLIPSIS).chars().count();
    (!cut.is_empty() && letters >= MIN_TITLE_LETTERS.min(title.chars().count())).then_some(cut)
}

/// The time line for an arc with `room` along it: "18:30–19:30", or the start alone when the
/// range does not fit, or nothing (a cut "09:…" says nothing).
fn arc_time(entry: &Entry, room: f32) -> Option<String> {
    let start = format_time(entry.event.start.time());
    let range = format!("{start}–{}", format_time(entry.event.end.time()));
    [range, start].into_iter().find(|time| text_width(time, typo::MICRO) <= room)
}

/// The moon on the numerals' circle at the first half hour of the night ([`layout::moon`]), in
/// `text_3`, fading with the night.
fn moon(frame: &mut Frame, dial: &Dial, plan: &Plan, p: &Palette, faded: &dyn Fn(Color, f32) -> Color) {
    let Some(rel) = layout::moon(&plan.window, &plan.nights) else { return };
    let at = dial.point(dial.numerals, plan.window.angle(rel));
    let half = icon::SIZE_SM / 2.0;
    let bounds = Rectangle::new(Point::new(at.x - half, at.y - half), Size::new(icon::SIZE_SM, icon::SIZE_SM));
    frame.draw_svg(bounds, &Icon::Moon.tinted(faded(p.text_3, opacity(rel))));
}

/// "3 h free" in `text_3` along the arc of each free stretch ([`Plan::free`]), centred on it in
/// the innermost ring, upright in the bottom half like the titles; left out where it does not fit.
/// The ring is empty there – free means no event in any ring – while just inside the rings the
/// label ran into the lines of the middle in the medium and narrow layouts.
fn free_time(frame: &mut Frame, dial: &Dial, plan: &Plan, p: &Palette, faded: &dyn Fn(Color, f32) -> Color) {
    let level = typo::META;
    let Some(innermost) = dial.rings.checked_sub(1) else { return };
    let (inner, outer) = dial.ring(innermost);
    let radius = (inner + outer) / 2.0;
    if outer - inner < line_height(level) || radius <= 0.0 {
        return;
    }
    let window = &plan.window;
    for &(from, to) in &plan.free {
        let note = free_note(from, to);
        let width = text_width(&note, level);
        let room = (window.angle(to) - window.angle(from)).to_radians() * radius;
        if width + 2.0 * space::S > room {
            continue;
        }
        let middle = window.angle((from + to) / 2.0);
        let flipped = (90.0..270.0).contains(&middle.rem_euclid(360.0));
        let half = (width / 2.0 / radius).to_degrees();
        let start = if flipped { middle + half } else { middle - half };
        for glyph in layout::glyphs(&note, level, radius, start, flipped) {
            let alpha = opacity(window.rel_of(glyph.angle));
            if alpha <= 0.002 {
                continue;
            }
            let at = dial.point(radius, glyph.angle);
            frame.with_save(|frame| {
                frame.translate(iced::Vector::new(at.x, at.y));
                frame.rotate(glyph.turn);
                label(frame, &glyph.letter, Point::ORIGIN, level, faded(p.text_3, alpha));
            });
        }
    }
}

/// "3 h free" for the free stretch `from`–`to` (minutes into the window): whole hours, as in the
/// timeline.
fn free_note(from: f32, to: f32) -> String {
    let hours = ((to - from) / 60.0).floor() as i64;
    trf("{n} h free", &[("n", &hours)])
}

/// The colours of the start edge, of the chip with its time and of the time: red with white
/// figures at now, `text_2` on the neutral selection anywhere else (`looking`).
fn edge_colours(p: &Palette, looking: bool) -> (Color, Color, Color) {
    if looking { (p.text_2, p.selected, p.text) } else { (p.accent, p.accent, p.on_accent) }
}

/// The clear edge at the start of the window: a line through the rings and the ticks, with the
/// time in a chip on the numerals' circle ([`edge_colours`]).
fn start_edge(frame: &mut Frame, dial: &Dial, plan: &Plan, p: &Palette) {
    let angle = plan.window.angle0;
    let (edge, chip, figures) = edge_colours(p, plan.looking);
    let from = dial.point((dial.inner - EDGE_INSIDE).max(0.0), angle);
    let to = dial.point(dial.ticks + EDGE_OUTSIDE, angle);
    line(frame, from, to, edge, EDGE_WIDTH);
    let at = dial.point(dial.numerals, angle);
    let height = line_height(CHIP);
    let pill = Rectangle::new(Point::new(at.x - CHIP_WIDTH / 2.0, at.y - height / 2.0), Size::new(CHIP_WIDTH, height));
    frame.fill(&Path::rounded_rectangle(pill.position(), pill.size(), Radius::from(radius::XS)), chip);
    label(frame, &format_time(plan.window.start.time()), at, CHIP, figures);
}

/// The piece of a ring between two radii and two angles, as a polygon (corners at most a degree
/// apart, finer than a pixel at these radii).
fn sector_path(dial: &Dial, inner: f32, outer: f32, from: f32, to: f32) -> Path {
    let corners = (((to - from) / DEGREES_PER_CORNER).ceil() as usize).max(1);
    let step = (to - from) / corners as f32;
    Path::new(|b| {
        b.move_to(dial.point(outer, from));
        for i in 1..=corners {
            b.line_to(dial.point(outer, from + step * i as f32));
        }
        for i in (0..=corners).rev() {
            b.line_to(dial.point(inner.max(0.0), from + step * i as f32));
        }
        b.close();
    })
}

fn sector(frame: &mut Frame, dial: &Dial, inner: f32, outer: f32, from: f32, to: f32, colour: Color) {
    // A turn at most: anything else (or not finite) comes from a degenerate dial.
    if to > from && to - from <= 360.0 && outer > inner && inner.is_finite() && outer.is_finite() {
        frame.fill(&sector_path(dial, inner, outer, from, to), colour);
    }
}

fn line(frame: &mut Frame, from: Point, to: Point, colour: Color, width: f32) {
    frame.stroke(&Path::line(from, to), Stroke::default().with_color(colour).with_width(width));
}

/// One line of text in `level`, centred on `at`.
fn label(frame: &mut Frame, content: &str, at: Point, level: typo::Level, colour: Color) {
    frame.fill_text(canvas::Text {
        content: content.to_string(),
        position: at,
        max_width: f32::INFINITY,
        color: colour,
        size: level.size,
        line_height: level.line_height,
        font: level.font,
        align_x: Alignment::Center,
        align_y: Vertical::Center,
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use iced::widget::canvas::Program;
    use torro_pim::{Event as CalendarEvent, EventTime};

    use super::super::Item;
    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("date").and_hms_opt(hour, minute, 0).expect("time")
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
        }
    }

    /// Tuesday 13 October 2026 at 14:20, as the demo with `--now 2026-10-13T14:20`.
    fn events() -> Vec<CalendarEvent> {
        vec![
            event("work", "API design workshop", local(13, 13, 0), local(13, 15, 0)),
            event("work", "1:1 with Lena", local(13, 14, 30), local(13, 15, 0)),
            event("personal", "Run", local(13, 18, 30), local(13, 19, 30)),
            event("family", "Breakfast", local(13, 7, 0), local(13, 8, 0)),
            event("personal", "Late call", local(14, 0, 0), local(14, 0, 30)),
            event("family", "Holiday", EventTime::Date(at(13, 0, 0).date()), EventTime::Date(at(14, 0, 0).date())),
        ]
    }

    fn data(events: &[CalendarEvent]) -> ViewData<'_> {
        data_on(events, 13)
    }

    /// The radar on 2026-10-`day` while it is Tuesday the 13th, 14:20.
    fn data_on(events: &[CalendarEvent], day: u32) -> ViewData<'_> {
        let day = at(day, 0, 0).date();
        ViewData {
            items: events.iter().enumerate().map(|(index, event)| Item { index, event, color: Color::BLACK }).collect(),
            start: day - Duration::days(1),
            end: day + Duration::days(3),
            anchor: day,
            now: at(13, 14, 20),
            selected: None,
        }
    }

    fn calendars() -> Vec<String> {
        ["family", "personal", "work"].map(String::from).to_vec()
    }

    fn titles(plan: &Plan, ids: &[usize]) -> Vec<String> {
        ids.iter().map(|id| plan.entries[*id].event.title.clone()).collect()
    }

    #[test]
    fn every_visible_calendar_has_a_ring_in_sidebar_order() {
        let events = events();
        let plan = Plan::new(&data(&events), &calendars(), 0);
        // Family has only breakfast (over) and an all-day holiday, and still its ring, the first.
        assert_eq!(plan.rings, 3);
        let ring_of = |title: &str| {
            plan.spans.iter().find(|s| plan.entries[s.id].event.title == title).map(|s| s.ring).expect(title)
        };
        assert_eq!((ring_of("Run"), ring_of("API design workshop")), (1, 2), "personal before work, as listed");
        assert!(plan.spans.iter().all(|s| plan.entries[s.id].event.title != "Breakfast"));
        let workshop = plan.spans.iter().find(|s| s.ring == 2 && !s.starts).expect("the workshop runs");
        assert_eq!(workshop.from, 0.0, "cut to the start of the window");
        // Looking back brings breakfast into the family ring; the rings stay as they are.
        let back = Plan::new(&data(&events), &calendars(), -7 * 60);
        assert_eq!(back.rings, 3);
        assert!(back.looking);
        assert_eq!(back.spans.iter().find(|s| plan.entries[s.id].event.title == "Breakfast").map(|s| s.ring), Some(0));
    }

    #[test]
    fn rings_show_without_any_events() {
        let plan = Plan::new(&data(&[]), &calendars(), 0);
        assert_eq!((plan.rings, plan.spans.len()), (3, 0));
        // A hidden calendar is not in the list the app passes: no ring for it.
        let plan = Plan::new(&data(&[]), &calendars()[1..], 0);
        assert_eq!(plan.rings, 2);
        // No calendars at all: a bare dial.
        assert_eq!(Plan::new(&data(&[]), &[], 0).rings, 0);
    }

    #[test]
    fn unknown_calendars_ring_last() {
        let order = ring_order(["zeta", "work", "alpha"].into_iter(), &calendars());
        assert_eq!(order, ["work", "alpha", "zeta"]);
        // The sidebar's calendars once each, then a used one it does not know.
        let order = ring_order(["family", "personal", "work", "work", "zeta"].into_iter(), &calendars());
        assert_eq!(order, ["family", "personal", "work", "zeta"]);
    }

    #[test]
    fn the_night_and_free_time_come_with_the_window() {
        let events = events();
        // 14:20: free 15:00–18:30 and 19:30–22:00; the night from 22:00 holds the late call.
        let plan = Plan::new(&data(&events), &calendars(), 0);
        assert_eq!(plan.nights, [(460.0, END)]);
        assert_eq!(plan.free, [(40.0, 250.0), (310.0, 460.0)]);
        assert_eq!(
            (free_note(40.0, 250.0), free_note(310.0, 460.0)),
            (trf("{n} h free", &[("n", &3)]), trf("{n} h free", &[("n", &2)]))
        );
        // An empty Sunday: no night between 08:00 and 19:54, all of it free.
        let sunday = Plan::new(&data_on(&[], 18), &calendars(), 0);
        assert!(sunday.nights.is_empty());
        assert_eq!(sunday.free, [(0.0, END)]);
        // Looking back at last night: what is over is not free.
        let back = Plan::new(&data(&events), &calendars(), -12 * 60);
        assert_eq!(back.window.start, at(13, 2, 20));
        assert_eq!(back.nights, [(0.0, 220.0)], "until 06:00");
        assert!(back.free.is_empty(), "all of it lies before now");
    }

    #[test]
    fn with_nothing_in_the_window_the_middle_says_when_the_next_event_comes() {
        let events = events();
        let today = at(13, 0, 0).date();
        // Wednesday with only Tuesday's events loaded: nothing runs, nothing comes.
        let wednesday = Plan::new(&data_on(&events[..4], 14), &calendars(), 0);
        assert!(wednesday.next().is_none() && wednesday.running().is_none());
        let late = [event("work", "Late call", local(15, 9, 30), local(15, 10, 0))];
        let plan = Plan::new(&data_on(&late, 14), &calendars(), 0);
        assert!(plan.next().is_some() && plan.next_inside().is_none(), "the next event lies beyond the window");
        assert_eq!(free_until(at(14, 9, 30), today, today), trf("Free until tomorrow {time}", &[("time", &"09:30")]));
        assert_eq!(
            free_until(at(14, 17, 0), at(14, 0, 0).date(), today),
            trf("Free until {time}", &[("time", &"17:00")])
        );
        let far = free_until(at(15, 9, 30), at(14, 0, 0).date(), today);
        assert_eq!(
            far,
            trf("Free until {date} at {time}", &[("date", &short_date(at(15, 0, 0).date())), ("time", &"09:30")])
        );
        // The middle builds in all three cases.
        for plan in [&wednesday, &plan, &Plan::new(&data(&events), &calendars(), 0)] {
            let _ = middle(plan, &Dial::fit(Size::new(800.0, 600.0), STAGE_MARGIN, plan.rings));
        }
    }

    #[test]
    fn the_middle_knows_what_runs_and_what_comes_next() {
        let events = events();
        let plan = Plan::new(&data(&events), &calendars(), 0);
        assert_eq!(plan.running().map(|e| e.event.title.as_str()), Some("API design workshop"));
        assert_eq!(plan.next().map(|e| e.event.title.as_str()), Some("1:1 with Lena"));
        let ahead = Plan::new(&data(&events), &calendars(), 60);
        assert!(ahead.running().is_none(), "15:20: nothing runs");
        assert_eq!(ahead.next().map(|e| e.event.title.as_str()), Some("Run"));
        let all_day: Vec<&str> = plan.all_day().iter().map(|e| e.event.title.as_str()).collect();
        assert_eq!(all_day, ["Holiday"]);
    }

    #[test]
    fn the_list_splits_at_nine_hours() {
        let events = events();
        let plan = Plan::new(&data(&events), &calendars(), 0);
        let (sooner, later) = plan.listed();
        assert_eq!(titles(&plan, &sooner), ["API design workshop", "1:1 with Lena", "Run"]);
        assert_eq!(titles(&plan, &later), ["Late call"], "00:00 lies 9 h 40 min ahead");
        let _ = list_view(&plan, size::PROPS);
    }

    /// The dial of a 1000 × 900 canvas with the demo events.
    fn radar() -> Radar {
        let events = events();
        Radar { plan: Plan::new(&data(&events), &calendars(), 0), margin: STAGE_MARGIN, wheel: true }
    }

    #[test]
    fn today_starts_at_now_with_a_red_edge_and_other_days_in_the_morning_with_a_grey_one() {
        let events = events();
        let today = Plan::new(&data(&events), &calendars(), 0);
        assert_eq!((today.window.start, today.looking), (at(13, 14, 20), false));
        let turned = Plan::new(&data(&events), &calendars(), 60);
        assert_eq!((turned.window.start, turned.looking), (at(13, 15, 20), true));
        // Wednesday: the late call at midnight opens it as early as the night allows.
        let tomorrow = Plan::new(&data_on(&events, 14), &calendars(), 0);
        assert_eq!((tomorrow.window.start, tomorrow.looking), (at(14, 6, 0), true));
        // An event begun the evening before does not count: 08:00.
        let overnight = [event("work", "Release", local(14, 22, 0), local(15, 1, 0))];
        assert_eq!(start(&data_on(&overnight, 15), 0), at(15, 8, 0));
        // An early event opens the day at its hour, an all-day one does not.
        let early = vec![
            event("work", "Early train", local(15, 6, 30), local(15, 7, 30)),
            event("family", "Trip", EventTime::Date(at(15, 0, 0).date()), EventTime::Date(at(16, 0, 0).date())),
        ];
        assert_eq!(start(&data_on(&early, 15), 0), at(15, 6, 0));
        assert_eq!(start(&data_on(&early[1..], 15), 0), at(15, 8, 0), "all-day events do not count");
        assert_eq!(start(&data_on(&early, 15), -90), at(15, 4, 30), "turned back before it");
        assert_eq!(start(&data_on(&early, 15), 99 * 60), at(16, 6, 0), "a day ahead at most");
    }

    #[test]
    fn the_edge_is_red_at_now_and_grey_elsewhere() {
        for theme in [theme::theme(), theme::light()] {
            let p = palette(&theme);
            assert_eq!(edge_colours(p, false), (p.accent, p.accent, p.on_accent));
            assert_eq!(edge_colours(p, true), (p.text_2, p.selected, p.text));
        }
    }

    fn mouse(event: mouse::Event) -> Event {
        Event::Mouse(event)
    }

    fn moved(position: Point) -> Event {
        mouse(mouse::Event::CursorMoved { position })
    }

    const PRESS: mouse::Event = mouse::Event::ButtonPressed(mouse::Button::Left);
    const RELEASE: mouse::Event = mouse::Event::ButtonReleased(mouse::Button::Left);

    /// The message an update publishes, if any.
    fn published(action: Option<Action<Message>>) -> Option<Message> {
        action.and_then(|action| action.into_inner().0)
    }

    #[test]
    fn a_click_on_an_arc_opens_its_event_and_free_room_starts_one_at_the_quarter_hour() {
        let radar = radar();
        let bounds = Size::new(1000.0, 900.0);
        let rect = Rectangle::new(Point::ORIGIN, bounds);
        let dial = radar.dial(bounds);
        let arcs = layout::arcs(&dial, &radar.plan.window, &radar.plan.spans);
        let run = arcs.iter().find(|a| radar.plan.entries[a.id].event.title == "Run").expect("run is drawn");
        let on_run = dial.point(run.middle(), (run.from + run.to) / 2.0);
        let cursor = |point: Point| mouse::Cursor::Available(point);
        let mut state = Grip::default();
        let open = published(radar.update(&mut state, &mouse(PRESS), rect, cursor(on_run)));
        assert!(matches!(open, Some(Message::OpenEvent(2))), "{open:?}");
        assert_eq!(state.press, None, "an event opens at once and is never dragged");
        // 21:10 in the outer ring is free: a new event at 21:00 when the button comes up again.
        let free = dial.point(dial.ring(0).0 + 1.0, radar.plan.window.angle(410.0));
        assert!(published(radar.update(&mut state, &mouse(PRESS), rect, cursor(free))).is_none());
        let trembling = Point::new(free.x + 2.0, free.y + 1.0);
        assert!(published(radar.update(&mut state, &moved(trembling), rect, cursor(trembling))).is_none());
        let message = published(radar.update(&mut state, &mouse(RELEASE), rect, cursor(trembling)));
        let expected = NaiveTime::from_hms_opt(21, 0, 0);
        assert!(
            matches!(message, Some(Message::NewEvent { date, time }) if date == at(13, 0, 0).date() && time == expected),
            "{message:?}"
        );
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(on_run)), mouse::Interaction::Pointer);
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(free)), mouse::Interaction::Grab);
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(dial.center)), mouse::Interaction::Idle);
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(Point::new(2.0, 2.0))), mouse::Interaction::Idle);
    }

    #[test]
    fn dragging_the_dial_turns_it_clockwise_later_and_is_no_click() {
        let radar = radar();
        let bounds = Size::new(1000.0, 900.0);
        let rect = Rectangle::new(Point::ORIGIN, bounds);
        let dial = radar.dial(bounds);
        let on_dial = |angle: f32| dial.point(dial.numerals, angle);
        let cursor = |point: Point| mouse::Cursor::Available(point);
        let mut state = Grip::default();
        // Pressed on the numerals at 350°, just left of the 12.
        assert!(published(radar.update(&mut state, &mouse(PRESS), rect, cursor(on_dial(350.0)))).is_none());
        assert!(state.press.is_some_and(|press| !press.dragging));
        // Clockwise over the 12 to 5°: 15° are half an hour later.
        let turned = published(radar.update(&mut state, &moved(on_dial(5.0)), rect, cursor(on_dial(5.0))));
        assert!(matches!(turned, Some(Message::Turn(30))), "{turned:?}");
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(on_dial(5.0))), mouse::Interaction::Grabbing);
        // 1° more is two minutes: not yet a step; another 2° make it one, with a minute left.
        assert!(published(radar.update(&mut state, &moved(on_dial(6.0)), rect, cursor(on_dial(6.0)))).is_none());
        let step = published(radar.update(&mut state, &moved(on_dial(8.0)), rect, cursor(on_dial(8.0))));
        assert!(matches!(step, Some(Message::Turn(5))), "{step:?}");
        // Back anticlockwise, beyond the canvas too: earlier.
        let outside = Point::new(dial.center.x - 2.0 * dial.radius, dial.center.y);
        let back = published(radar.update(&mut state, &moved(outside), rect, mouse::Cursor::Available(outside)));
        assert!(matches!(back, Some(Message::Turn(minutes)) if minutes < 0), "{back:?}");
        // Letting go after a drag is no click.
        assert!(published(radar.update(&mut state, &mouse(RELEASE), rect, cursor(outside))).is_none());
        assert_eq!(state.press, None);
        // Nothing turns without a press, and a press beside the dial is ignored.
        assert!(radar.update(&mut state, &moved(on_dial(90.0)), rect, cursor(on_dial(90.0))).is_none());
        assert!(radar.update(&mut state, &mouse(PRESS), rect, cursor(Point::new(2.0, 2.0))).is_none());
        assert_eq!(state.press, None);
    }

    #[test]
    fn the_wheel_turns_in_quarter_hours() {
        let mut scroll = Grip::default();
        assert_eq!(Radar::steps(&mut scroll, mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 }), 1, "down looks ahead");
        assert_eq!(Radar::steps(&mut scroll, mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 }), -2);
        assert_eq!(Radar::steps(&mut scroll, mouse::ScrollDelta::Pixels { x: 0.0, y: -30.0 }), 0, "not yet a step");
        assert_eq!(Radar::steps(&mut scroll, mouse::ScrollDelta::Pixels { x: 0.0, y: -30.0 }), 1);
        let radar = radar();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 900.0));
        let wheel = Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 } });
        let mut state = Grip::default();
        let cursor = mouse::Cursor::Available(Point::new(500.0, 450.0));
        assert!(radar.update(&mut state, &wheel, bounds, cursor).is_some());
        let narrow = Radar { wheel: false, ..radar };
        assert!(narrow.update(&mut state, &wheel, bounds, cursor).is_none(), "narrow: the page scrolls");
    }

    #[test]
    fn arc_text_is_cut_but_never_to_nothing_worth_reading() {
        assert_eq!(arc_title("Run", 100.0).as_deref(), Some("Run"));
        let cut = arc_title("API design workshop", 90.0).expect("room for a part");
        assert!(cut.starts_with("API") && cut.ends_with(typo::ELLIPSIS), "{cut}");
        assert_eq!(arc_title("Daily Standup", 30.0), None, "\"Da…\" says nothing");
        let events = events();
        let plan = Plan::new(&data(&events), &calendars(), 0);
        let run = plan.entries.iter().find(|e| e.event.title == "Run").expect("run");
        assert_eq!(arc_time(run, 200.0).as_deref(), Some("18:30–19:30"));
        assert_eq!(arc_time(run, 40.0).as_deref(), Some("18:30"), "the start alone");
        assert_eq!(arc_time(run, 20.0), None, "no cut time");
    }

    #[test]
    fn the_view_builds_in_every_place_of_the_list() {
        let events = events();
        for place in [ListAt::Props, ListAt::Beside, ListAt::Below] {
            let _ = view(data(&events), &calendars(), 90, place);
        }
        let _ = list(&data(&events), &calendars(), 0);
        let plan = Plan::new(&data(&events), &calendars(), 0);
        let _ = middle(&plan, &Dial::fit(Size::new(800.0, 600.0), STAGE_MARGIN, plan.rings));
    }
}
