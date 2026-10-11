//! Radar: the next twelve hours on a clock face (task K.22 in `docs/tasks/kalender-aufgaben.md`,
//! prototype `docs/prototypes/calendar/radar.html`, mode "fade out", 3/4 visible).
//!
//! A 12-hour dial, 12 at the top, numbered with the real hours of the day (`15` where the 3
//! stands). The window starts at now plus the look-ahead – a clear red edge with the time in a
//! chip outside – and runs one turn: the first nine hours in full, the last three fading out to
//! nothing just before the edge. Iced has no masks, so the fading quarter is drawn in pieces of
//! two degrees, each with the opacity of its middle ([`layout::pieces`]).
//!
//! Every calendar with timed events in the window gets a ring, in the order of the sidebar, the
//! first outside; overlapping events share their ring in lanes. Events are tinted arcs with a
//! colour bar where they start, past ones faded, the one open in the side panel outlined with the
//! focus ring. Titles run along the arc letter by letter (upright in the bottom half), with the
//! time on a second line where the lane is wide enough.
//!
//! The middle says the date and the time, what runs ("40 min left") or how long is free, what
//! comes next and what lasts all day; while looking ahead, "Back to now". Beside the dial (in the
//! properties column when wide, at the right when medium, below when narrow) the events of the
//! next nine hours and of the fading quarter are listed.
//!
//! Clicks: an arc → `OpenEvent`, free room in the rings → `NewEvent` (rounded down to the quarter
//! hour), a list row → `OpenEvent`. The mouse wheel over the dial looks ahead or back in quarter
//! hours (`LookAhead`; not in the narrow layout, where the wheel scrolls the page).
//!
//! The geometry is in `radar/layout.rs`.

mod layout;

pub use layout::{HOUR_STEP, WHEEL_STEP, clamp_offset};

use chrono::{Duration, NaiveDateTime, NaiveTime, Timelike};
use iced::alignment::Vertical;
use iced::border::Radius;
use iced::widget::canvas::{self, Action, Frame, Path, Stroke};
use iced::widget::scrollable::Direction;
use iced::widget::text::{Alignment, Wrapping};
use iced::widget::{button, column, container, responsive, row, rule, scrollable, stack};
use iced::{Color, Element, Event, Length, Padding, Point, Rectangle, Size, Theme, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::format_time;
use torro_gui::theme::{self, Palette, mix, over, palette, radius, size, space};
use torro_gui::typo::{self, ellipsize, line_height, text_width};
use torro_gui::{style, widgets};

use layout::{Arc, Dial, END, Hit, Span, VISIBLE, WINDOW, Window, opacity, pieces};

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

/// The radar for `data`, looking `offset` minutes ahead of `data.now`, the rings in the order of
/// `calendars` (collection ids, as in the sidebar), its list `at`.
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
    /// Looking ahead (or back): the edge is grey instead of red, "Back to now" shows.
    looking: bool,
    entries: Vec<Entry>,
    /// The timed entries in the window, in their rings.
    spans: Vec<Span>,
    rings: usize,
    /// The app's index of the event open in the side panel.
    selected: Option<usize>,
}

impl Plan {
    fn new(data: &ViewData<'_>, calendars: &[String], offset: i64) -> Plan {
        let window = Window::new(data.now + Duration::minutes(offset));
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
        let rings = ring_order(entries.iter().filter(|e| inside(e)).map(|e| e.calendar.as_str()), calendars);
        let spans = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| inside(entry))
            .filter_map(|(id, entry)| {
                let ring = rings.iter().position(|calendar| *calendar == entry.calendar)?;
                let (from, to) = (window.rel(entry.event.start), window.rel(entry.event.end));
                Some(Span { id, ring, from: from.max(0.0), to: to.min(END), starts: from >= 0.0 })
            })
            .collect();
        Plan {
            window,
            now: data.now,
            looking: offset != 0,
            entries,
            spans,
            rings: rings.len(),
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

/// The calendars of `used` in the order of `calendars` (the sidebar's); ones it does not know
/// come last, by id.
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
    } else if let Some(next) = plan.next() {
        let free = trf("Free for {span}", &[("span", &span(layout::minutes_until(at, next.event.start)))]);
        detail =
            detail.push(one_line(typo::BODY_STRONG, ellipsize(&free, typo::BODY_STRONG, width)).style(theme::text_2));
    }
    if let Some(next) = plan.next() {
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
    /// The wheel looks ahead (not in the narrow layout, where it scrolls the page).
    wheel: bool,
}

/// Smooth scrolling (touchpads) comes in pixels; they add up to wheel steps.
#[derive(Debug, Default)]
struct Scroll {
    pixels: f32,
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

    /// Wheel steps from a scroll; down looks ahead, like the page would move on.
    fn steps(state: &mut Scroll, delta: mouse::ScrollDelta) -> i64 {
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
    type State = Scroll;

    fn update(
        &self,
        state: &mut Scroll,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let point = cursor.position_in(bounds)?;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let message = self.message(self.hit(bounds.size(), point)?)?;
                Some(Action::publish(message).and_capture())
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if self.wheel => {
                let steps = Radar::steps(state, *delta);
                if steps == 0 {
                    return Some(Action::capture());
                }
                Some(Action::publish(Message::LookAhead(steps * WHEEL_STEP)).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(&self, _state: &Scroll, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        let over = cursor.position_in(bounds).and_then(|point| self.hit(bounds.size(), point));
        // The hand only over events; free room stays calm although a click there starts one.
        if matches!(over, Some(Hit::Event(_))) { mouse::Interaction::Pointer } else { mouse::Interaction::Idle }
    }

    fn draw(
        &self,
        _state: &Scroll,
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

        // The tracks of the rings.
        let track = faded(p.text, TRACK);
        for ring in 0..dial.rings {
            let (inner, outer) = dial.ring(ring);
            for piece in pieces(0.0, END) {
                let colour = faded(track, piece.opacity);
                sector(&mut frame, &dial, inner, outer, window.angle(piece.from), seam(window, piece.to), colour);
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
        for arc in &arcs {
            let Some(entry) = plan.entries.get(arc.id) else { continue };
            event_arc(&mut frame, &dial, window, arc, &tone(p, entry, plan.now), &faded);
        }
        for arc in &arcs {
            let Some(entry) = plan.entries.get(arc.id) else { continue };
            arc_text(&mut frame, &dial, window, arc, entry, &tone(p, entry, plan.now), &faded);
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

        start_edge(&mut frame, &dial, plan, p);
        vec![frame.into_geometry()]
    }
}

/// The colours of an event, composed over the panel: tinted fill, full bar, title in `text`, time
/// in `text_2`; one that has ended faded as in the week grid.
struct Tone {
    fill: Color,
    bar: Color,
    title: Color,
    time: Color,
}

fn tone(p: &Palette, entry: &Entry, now: NaiveDateTime) -> Tone {
    let colour = entry.event.color;
    let past = entry.event.end <= now;
    let fade = |c: Color| if past { mix(p.panel, c, PAST_OPACITY) } else { c };
    Tone { fill: fade(over(p.tinted(colour), p.panel)), bar: fade(colour), title: fade(p.text), time: fade(p.text_2) }
}

/// The end angle of a fill piece, a quarter degree further so neighbouring pieces overlap and no
/// hairline of the panel shows between them; never past the end of the window.
fn seam(window: &Window, to: f32) -> f32 {
    window.angle(to).min(window.angle(END)) + if to < END { 0.25 } else { 0.0 }
}

/// An event's arc, its fading part in pieces, and its colour bar where it starts.
fn event_arc(
    frame: &mut Frame,
    dial: &Dial,
    window: &Window,
    arc: &Arc,
    tone: &Tone,
    faded: &dyn Fn(Color, f32) -> Color,
) {
    let (from, to) = (window.rel_of(arc.from), window.rel_of(arc.to));
    for piece in pieces(from, to) {
        let end = window.angle(piece.to) + if piece.to < to { 0.25 } else { 0.0 };
        sector(
            frame,
            dial,
            arc.inner,
            arc.outer,
            window.angle(piece.from),
            end.min(arc.to),
            faded(tone.fill, piece.opacity),
        );
    }
    if arc.starts {
        let width = (BAR / arc.middle()).to_degrees();
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

/// The clear edge at the start of the window: a line through the rings and the ticks, red at now
/// and `text_2` while looking ahead, with the time in a chip on the numerals' circle.
fn start_edge(frame: &mut Frame, dial: &Dial, plan: &Plan, p: &Palette) {
    let angle = plan.window.angle0;
    let (edge, chip, figures) =
        if plan.looking { (p.text_2, p.selected, p.text) } else { (p.accent, p.accent, p.on_accent) };
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
        let day = at(13, 0, 0).date();
        ViewData {
            items: events.iter().enumerate().map(|(index, event)| Item { index, event, color: Color::BLACK }).collect(),
            start: day,
            end: day + Duration::days(2),
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
    fn rings_only_for_calendars_with_timed_events_in_the_window_in_sidebar_order() {
        let events = events();
        let plan = Plan::new(&data(&events), &calendars(), 0);
        // Family has only breakfast (over) and an all-day holiday: no ring.
        assert_eq!(plan.rings, 2);
        let ring_of = |title: &str| {
            plan.spans.iter().find(|s| plan.entries[s.id].event.title == title).map(|s| s.ring).expect(title)
        };
        assert_eq!((ring_of("Run"), ring_of("API design workshop")), (0, 1), "personal before work, as listed");
        assert!(plan.spans.iter().all(|s| plan.entries[s.id].event.title != "Breakfast"));
        let workshop = plan.spans.iter().find(|s| s.ring == 1 && !s.starts).expect("the workshop runs");
        assert_eq!(workshop.from, 0.0, "cut to the start of the window");
        // Looking back two hours brings breakfast into the window and the family ring with it.
        let back = Plan::new(&data(&events), &calendars(), -7 * 60);
        assert_eq!(back.rings, 3);
        assert!(back.looking);
    }

    #[test]
    fn unknown_calendars_ring_last() {
        let order = ring_order(["zeta", "work", "alpha"].into_iter(), &calendars());
        assert_eq!(order, ["work", "alpha", "zeta"]);
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
    fn a_click_on_an_arc_opens_its_event_and_free_room_starts_one_at_the_quarter_hour() {
        let radar = radar();
        let bounds = Size::new(1000.0, 900.0);
        let dial = radar.dial(bounds);
        let arcs = layout::arcs(&dial, &radar.plan.window, &radar.plan.spans);
        let run = arcs.iter().find(|a| radar.plan.entries[a.id].event.title == "Run").expect("run is drawn");
        let on_run = dial.point(run.middle(), (run.from + run.to) / 2.0);
        assert!(matches!(radar.message(radar.hit(bounds, on_run).expect("hit")), Some(Message::OpenEvent(2))));
        // 21:10 in the outer ring is free: a new event at 21:00.
        let free = dial.point(dial.ring(0).0 + 1.0, radar.plan.window.angle(410.0));
        let message = radar.message(radar.hit(bounds, free).expect("free room"));
        let expected = NaiveTime::from_hms_opt(21, 0, 0);
        assert!(
            matches!(message, Some(Message::NewEvent { date, time }) if date == at(13, 0, 0).date() && time == expected),
            "{message:?}"
        );
        let cursor = |point: Point| mouse::Cursor::Available(point);
        let rect = Rectangle::new(Point::ORIGIN, bounds);
        assert_eq!(radar.mouse_interaction(&Scroll::default(), rect, cursor(on_run)), mouse::Interaction::Pointer);
        assert_eq!(radar.mouse_interaction(&Scroll::default(), rect, cursor(free)), mouse::Interaction::Idle);
    }

    #[test]
    fn the_wheel_looks_ahead_in_quarter_hours() {
        let mut scroll = Scroll::default();
        assert_eq!(Radar::steps(&mut scroll, mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 }), 1, "down looks ahead");
        assert_eq!(Radar::steps(&mut scroll, mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 }), -2);
        assert_eq!(Radar::steps(&mut scroll, mouse::ScrollDelta::Pixels { x: 0.0, y: -30.0 }), 0, "not yet a step");
        assert_eq!(Radar::steps(&mut scroll, mouse::ScrollDelta::Pixels { x: 0.0, y: -30.0 }), 1);
        let radar = radar();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 900.0));
        let wheel = Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 } });
        let mut state = Scroll::default();
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
