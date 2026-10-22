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
//! the grid. A ring's track is faintly tinted in its calendar's colour, and the ring names its
//! calendar in the last hour before the edge, where the fade has left it empty: a dot and the name
//! (task K.29). Overlapping events share their ring in lanes. Events are arcs filled strongly in
//! their calendar's colour (much stronger than the week grid's tint; [`mini::fill`] for the large
//! and the mini dials alike) with a colour bar where they start, past ones faded, the one open in
//! the side panel outlined with the focus ring. Titles run along the arc letter by letter (upright
//! in the bottom half) in light or dark text, whichever reads on the fill: in `MICRO` in narrow
//! lanes, in `CAPTION` in wider ones, with the time on a second line where the lane is wide enough.
//!
//! The night (the suite's setting, 22:00–06:00 by default, as in the timeline; task K.27) lies
//! behind the rings as a sector in the night colour, a moon on a small round plaque on the
//! numerals' circle at its first half hour outside the fading quarter. A double-click on the moon
//! opens the night's card (`EditNight`). The night fades with the last quarter, piece by piece
//! like the rest.
//!
//! The working hours (the suite's setting, 08:00–17:00 by default; task K.33) are a thin, muted
//! arc just inside the innermost ring, from their start to their end, where neither the night nor
//! the moon lie; they fade with the last quarter like the tracks.
//!
//! Resting on something for a moment shows a tooltip in the canvas beside the pointer: an event's
//! title, time and place; the calendar of an empty track; how to set the night on the moon. The
//! pointer is the hand over events and the moon, the open hand over the rest of the dial.
//!
//! The middle says only the date and the time. Away from now (another day, or the dial turned) it
//! shows a clock button, "Back to now". Free time is not named: a stretch without an event is not
//! necessarily free time.
//!
//! Beside the dial (in the properties column when wide, at the right when medium, below when
//! narrow) "Coming days" shows a mini radar for each of seven days, how full it is at a glance
//! (task K.30, `radar/mini.rs`): a page that starts the day after the shown one and stays put when
//! a tile is clicked; its ‹ › browse it a week at a time (task K.35).
//!
//! Clicks: an arc → `OpenEvent`, free room in the rings → `NewEvent` (rounded down to the quarter
//! hour), a mini radar → `GoTo` its day. The dial turns (`Turn`): the mouse wheel over it in quarter
//! hours (not in the narrow layout, where the wheel scrolls the page), and dragging it like a
//! knob – clockwise is later, a turn is twelve hours, in five-minute steps. A press only becomes
//! a drag after a few pixels, so a click stays a click.
//!
//! The geometry is in `radar/layout.rs`.

mod layout;
mod mini;

pub use layout::{HOUR_STEP, WHEEL_STEP, clamp_offset};
/// How many days a page of mini radars holds: load the events through the last of them.
pub use mini::DAYS as COMING_DAYS;
/// The first day of the mini radars' page once the radar shows a day.
pub use mini::page as coming_page;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use iced::alignment::Vertical;
use iced::border::Radius;
use iced::time::Instant;
use iced::widget::canvas::{self, Action, Frame, Path, Stroke};
use iced::widget::scrollable::Direction;
use iced::widget::text::Alignment;
use iced::widget::{column, container, responsive, row, rule, scrollable, stack};
use iced::{Color, Element, Event, Length, Padding, Point, Rectangle, Size, Theme, mouse};
use torro_core::i18n::tr;
use torro_core::settings::{Night, WorkingHours};
use torro_gui::datetime::format_time;
use torro_gui::icon::{self, Icon};
use torro_gui::style;
use torro_gui::theme::{self, Palette, mix, palette, radius, size, space};
use torro_gui::typo::{self, ellipsize, line_height, text_width};
use torro_gui::widgets::{self, Anchor};

use layout::{Arc, Dial, END, Hit, Span, Window, opacity, pieces, split};

use super::glance;
use super::timeline::{self, is_double_click};
use super::week;
use super::{ViewData, short_date};
use crate::app::Message;

/// Where the radar shows its mini radars of the coming days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListAt {
    /// In the properties column of the wide layout ([`list`]); the view itself leaves it out.
    Props,
    /// At the right of the dial, [`LIST_MEDIUM`] wide (medium layout), in two columns.
    Beside,
    /// Under the dial, as many columns as fit, the whole page scrolling (narrow layout).
    Below,
}

/// Width of the mini radars' column beside the dial in the medium layout (`radar.html`: 260); the
/// wide layout uses the properties column.
const LIST_MEDIUM: f32 = 260.0;
/// The dial keeps this much room to the edges of its stage (`radar.html`: 48 in all).
fn stage_margin() -> f32 {
    space::xl()
}
/// The narrow dial is never wider than this (`radar.html`: 440).
const NARROW_DIAL: f32 = 440.0;
/// Height of the week strip above the narrow radar (`week_strip`: weekday letter, the 28 circle,
/// dots, padding and its line), for [`strip_fits`].
fn week_strip() -> f32 {
    2.0 * size::control_sm() + 2.0 * space::xs() + space::s()
}
/// The middle keeps this far from the inner ring on each side (`radar.html`: 16).
fn middle_inset() -> f32 {
    space::l()
}
/// The neutral track of the mini dials' single ring: `text` at 7 % over the panel.
const TRACK: f32 = 0.07;
/// The track of a ring of the large dial: its calendar's colour at 10 % over the panel or the
/// night, so the rings tell their calendars apart while events stand clearly in front and the night
/// still shows (task K.29, set by screenshot).
const TRACK_TINT: f32 = 0.10;
/// The time under an event's title: the title's colour at this share over the fill, a step
/// quieter than the title and still 4.5:1 on the demo's colours.
const TIME_SHARE: f32 = 0.9;
/// Ticks: `text` at 35 % over the panel.
const TICK: f32 = 0.35;
/// Length of a full-hour tick and of a half-hour one.
fn tick_long() -> f32 {
    space::s()
}
fn tick_short() -> f32 {
    space::xs()
}
/// The start edge reaches this far inside the rings and this far out over the ticks.
fn edge_inside() -> f32 {
    space::s()
}
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
/// Titles in lanes at least this wide, in `MICRO`; from [`ONE_LINE`] on in `CAPTION`; the time as
/// a second line from [`TWO_LINES`] on.
const MICRO_LANE: f32 = 16.0;
const ONE_LINE: f32 = 22.0;
const TWO_LINES: f32 = 40.0;
/// Text along an arc starts this far after its start (the bar plus 4) and keeps this far from its
/// end. A little tighter than the week grid's 6, so a 45-minute event on the inner ring of a medium
/// dial still carries its title (task K.29).
fn text_start() -> f32 {
    BAR + space::xs()
}
fn text_end() -> f32 {
    space::xs()
}
/// Shorter than this a title is not worth starting: three letters and "…" in `MICRO`.
const TEXT_MIN: f32 = 20.0;
/// A title cut shorter than this many letters is left out: "Lun…" still hints at the lunch, "Lu…"
/// says nothing (the tooltip has the whole title).
const MIN_TITLE_LETTERS: usize = 3;
/// Past events are faded like in the week grid.
const PAST_OPACITY: f32 = 0.55;
/// Steps of the arcs drawn as polygons: one corner per degree at most.
const DEGREES_PER_CORNER: f32 = 1.0;
/// Pixels of a smooth (touchpad) scroll that make one wheel step.
const PIXELS_PER_STEP: f32 = 40.0;
/// The time chip at the start edge: the week grid's now label (500, 11 on 16).
fn chip_level() -> typo::Level {
    typo::Level { font: theme::font_tabular_medium(), line_height: typo::caption().line_height, ..typo::micro() }
}
fn chip_width() -> f32 {
    size::axis_narrow() - 2.0 * space::xs()
}
/// The large numerals at 3, 6, 9 and 12: 13/500.
fn major() -> typo::Level {
    typo::body_strong().tabular()
}
/// While dragging, the pointer this close to the centre does not turn the dial: there the
/// direction swings at the smallest move.
fn drag_dead() -> f32 {
    space::xl()
}
/// The round plaque under the moon (panel with a line around it), so the moon reads on any ground:
/// 22 across, 3 around the 16 icon.
const MOON_PLAQUE: f32 = icon::SIZE + 6.0;
/// Width of the arc of the working hours, [`layout::RING_GAP`] inside the innermost ring: as thin
/// as the line of the working hours in the week and the timeline.
const WORK_ARC: f32 = 2.0;
/// A tooltip is at most this wide; longer titles and places are cut with "…".
fn tip_max() -> f32 {
    size::props() - 2.0 * space::xl()
}
/// The dot before the title in a tooltip (radius), as the dots of the calendars in the sidebar.
fn tip_dot() -> f32 {
    space::xs()
}

/// A visible calendar as the radar shows its ring.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    /// The collection id (`Event::collection` of its events).
    pub id: String,
    /// Its name as the sidebar shows it ("Personal").
    pub name: String,
    /// The host of its account (`PimAccount::host`, "cloud.imt24.de"), if it has one. The radar
    /// names it behind the name when two visible calendars share that name.
    pub host: Option<String>,
    /// Its colour (`widgets::collection_color`): the dot before the name, the tint of the track.
    pub colour: Color,
}

/// The radar for the day `data.anchor`, its dial turned `offset` minutes from the start of that
/// day ([`start`]), a ring for each of `calendars` (the visible ones, in the order of the sidebar)
/// whether or not it has events, each named in its last hour, the mini radars of the page starting
/// on `coming` `at` their place (over the events of `data`: load them through the page's last
/// day), `night` behind the rings and the arc of the `working` hours inside them. The dial's canvas
/// is anchored at `anchor`, so the night's card can float beside the moon.
#[allow(clippy::too_many_arguments, reason = "what the app hands the view, every part named at the call")]
pub fn view<'a>(
    data: ViewData<'a>,
    calendars: &[Ring],
    offset: i64,
    coming: NaiveDate,
    at: ListAt,
    night: Night,
    working: WorkingHours,
    anchor: &Anchor,
) -> Element<'a, Message> {
    let plan = Plan::new(&data, calendars, offset, night, working);
    match at {
        ListAt::Props => stage(plan, anchor.clone()),
        ListAt::Beside => {
            let width = LIST_MEDIUM - size::HAIRLINE;
            let content = width - 2.0 * space::s();
            let coming = mini::Coming::new(&data, coming, night).view(content, mini::columns(at, content));
            let list = scrollable(container(coming).padding(Padding {
                top: space::l(),
                right: space::s(),
                bottom: space::xl(),
                left: space::s(),
            }))
            .direction(Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(width)
            .height(Length::Fill);
            row![stage(plan, anchor.clone()), rule::vertical(1).style(style::rule), list].height(Length::Fill).into()
        }
        ListAt::Below => {
            let anchor = anchor.clone();
            let coming = mini::Coming::new(&data, coming, night);
            responsive(move |bounds| narrow(plan.clone(), &coming, bounds, &anchor)).into()
        }
    }
}

/// Whether the week strip fits above the narrow radar in `bounds` (the room under the header)
/// without making the dial smaller than the width allows: the dial is what the view is for, so in a
/// low tile the strip gives way (‹ › and the small month still page the days).
pub fn strip_fits(bounds: Size) -> bool {
    let dial = (bounds.width - 2.0 * space::s()).clamp(0.0, NARROW_DIAL) + 2.0 * space::s();
    bounds.height - week_strip() >= dial
}

/// Where the window starts: the start of the day `data.anchor` ([`layout::day_start`] over the
/// timed events of `data`: now for today, else 08:00 or the full hour of an earlier event, never
/// before `night` ends) plus `offset` minutes of turning (kept to [`clamp_offset`]). The header
/// names the day of this time.
pub fn start(data: &ViewData<'_>, offset: i64, night: Night) -> NaiveDateTime {
    let starts = data.items.iter().map(glance::Entry::new).filter(|entry| !entry.all_day).map(|entry| entry.start);
    layout::day_start(data.anchor, data.now, starts, night) + Duration::minutes(clamp_offset(offset))
}

/// The mini radars of the page starting on `coming` for the properties column of the wide layout,
/// in two columns as beside the dial; each opens its day where the radar would with `night`. Load
/// the events through the page's last day.
pub fn list(data: &ViewData<'_>, coming: NaiveDate, night: Night) -> Element<'static, Message> {
    // The column pads its content by 8 on each side and has a line on the left.
    let width = size::props() - size::HAIRLINE - 2.0 * space::s();
    mini::Coming::new(data, coming, night).view(width, mini::columns(ListAt::Props, width))
}

/// The two canvases of the dial ([`Layer`]) for `plan`, `margin` from the canvas edges, each
/// `height` high.
fn canvases(plan: &Plan, margin: f32, wheel: bool, height: Length) -> [Element<'static, Message>; 2] {
    [Layer::Dial, Layer::Input].map(|layer| {
        iced::widget::canvas(Radar { plan: plan.clone(), margin, wheel, layer })
            .width(Length::Fill)
            .height(height)
            .into()
    })
}

/// The dial filling its stage, the middle laid over it, the input layer over both.
fn stage(plan: Plan, anchor: Anchor) -> Element<'static, Message> {
    responsive(move |bounds| {
        let dial = Dial::fit(bounds, stage_margin(), plan.rings.len());
        let [canvas, input] = canvases(&plan, stage_margin(), true, Length::Fill);
        let middle = container(middle(&plan, &dial)).center(Length::Fill);
        stack![widgets::anchored(&anchor, canvas), middle, input].into()
    })
    .into()
}

/// Narrow: the dial as large as the window allows – never wider or taller than what is on screen
/// (at most [`NARROW_DIAL`]), so a low tile shows the whole circle –, the mini radars of the
/// `coming` days under it in as many columns as fit, the whole page scrolling. The wheel scrolls
/// the page, not the time.
fn narrow(plan: Plan, coming: &mini::Coming, bounds: Size, anchor: &Anchor) -> Element<'static, Message> {
    let width = bounds.width;
    let edge = (width.min(bounds.height) - 2.0 * space::s()).clamp(0.0, NARROW_DIAL) + 2.0 * space::s();
    let dial = Dial::fit(Size::new(width, edge), space::s(), plan.rings.len());
    let [canvas, input] = canvases(&plan, space::s(), false, Length::Fixed(edge));
    let middle = container(middle(&plan, &dial)).center_x(Length::Fill).center_y(Length::Fixed(edge));
    let content = width - 2.0 * space::l();
    let list = container(coming.view(content, mini::columns(ListAt::Below, content))).padding(space::l());
    let canvas = widgets::anchored(anchor, canvas);
    scrollable(column![stack![canvas, middle, input], rule::horizontal(1).style(style::rule), list])
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
    /// The rings, the first outside.
    rings: Vec<RingLabel>,
    /// The nights in the window ([`layout::nights`]).
    nights: Vec<(f32, f32)>,
    /// The working hours in the window ([`layout::working`]).
    working: Vec<(f32, f32)>,
    /// The app's index of the event open in the side panel.
    selected: Option<usize>,
}

/// A ring as the radar draws it: what it names its calendar and in which colour.
#[derive(Debug, Clone, PartialEq)]
struct RingLabel {
    /// The calendar's name, "Persönlich · cloud.imt24.de" when another visible calendar has the
    /// same name ([`ring_label`]); empty for a calendar the sidebar does not know (only the dot).
    name: String,
    colour: Color,
}

/// What ring `calendar` says of itself among the visible `calendars`: its name, followed by the
/// host of its account when another of them has the same name (two "Persönlich" in two accounts).
fn ring_label(calendar: &Ring, calendars: &[Ring]) -> String {
    let twin = calendars.iter().any(|other| other.id != calendar.id && other.name == calendar.name);
    match calendar.host.as_deref().filter(|host| twin && !host.is_empty()) {
        Some(host) => format!("{} · {host}", calendar.name),
        None => calendar.name.clone(),
    }
}

impl Plan {
    fn new(data: &ViewData<'_>, calendars: &[Ring], offset: i64, night: Night, working: WorkingHours) -> Plan {
        let window = Window::new(start(data, offset, night));
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
        let ids: Vec<String> = calendars.iter().map(|c| c.id.clone()).collect();
        let rings = ring_order(ids.iter().map(String::as_str).chain(used), &ids);
        let labels = rings
            .iter()
            .map(|id| match calendars.iter().find(|c| &c.id == id) {
                Some(calendar) => RingLabel { name: ring_label(calendar, calendars), colour: calendar.colour },
                // Only rings with events in the window are unknown: they take their events' colour.
                None => RingLabel {
                    name: String::new(),
                    colour: entries.iter().find(|e| &e.calendar == id).map_or(Color::BLACK, |e| e.event.color),
                },
            })
            .collect();
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
        let nights = layout::nights(&window, night);
        let working = layout::working(&window, working);
        Plan {
            window,
            now: data.now,
            looking: data.anchor != data.now.date() || offset != 0,
            entries,
            spans,
            rings: labels,
            nights,
            working,
            selected: data.selected,
        }
    }

    fn is_selected(&self, id: usize) -> bool {
        self.selected.is_some_and(|index| self.entries.get(id).is_some_and(|entry| entry.index == index))
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

/// The middle of the dial: the date and the time. Away from now (another day, or the dial turned)
/// a clock button brings the dial back to now. Cut to the room inside the rings.
fn middle(plan: &Plan, dial: &Dial) -> Element<'static, Message> {
    let width = (2.0 * dial.inner - 2.0 * middle_inset()).max(0.0);
    let at = plan.window.start;
    let clock = column![
        typo::text(typo::meta(), short_date(at.date())).style(theme::text_3),
        typo::text(typo::display().tabular(), format_time(at.time())),
    ]
    .spacing(space::xs())
    .align_x(iced::Alignment::Center);
    let mut parts = column![clock].spacing(space::s()).align_x(iced::Alignment::Center);
    if plan.looking {
        // The icon button carries its own tooltip, which is also its label for screen readers.
        parts = parts.push(widgets::icon_button(Icon::Clock, tr("Back to now"), Some(Message::Today)));
    }
    container(parts).width(width).center_x(width).into()
}

/// The dial on a canvas, in two layers ([`Layer`]).
struct Radar {
    plan: Plan,
    /// Room to the edges of the canvas.
    margin: f32,
    /// The wheel turns the dial (not in the narrow layout, where it scrolls the page).
    wheel: bool,
    layer: Layer,
}

/// The radar is two canvases of the same size, one over the other with the middle between them.
/// Iced draws the text of a canvas above all of its shapes, so a tooltip drawn on the dial's own
/// canvas let the numerals and titles under it shine through its card; a canvas of its own in a
/// layer above the dial covers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// Everything of the dial but the tooltips; takes no input.
    Dial,
    /// On top: all input (clicks, turning, the pointer, resting) and the tooltips. Where it has
    /// nothing to do – the middle – it lets the pointer through to the "Back to now" button.
    Input,
}

/// What the canvas keeps between events.
#[derive(Debug, Default)]
struct Grip {
    /// Smooth scrolling (touchpads) comes in pixels; they add up to wheel steps.
    pixels: f32,
    /// The left button is down on the dial, outside an event.
    press: Option<Press>,
    /// When and where the moon was last pressed, for a double-click.
    moon_press: Option<(Instant, Point)>,
    /// What the pointer rests on and since when, for its tooltip.
    rest: Option<Rest>,
}

/// What a tooltip on the dial is about.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tip {
    /// The moon, with its target: "Double-click to set the night" beside it.
    Moon(Rectangle),
    /// The event with this id in the plan: its title, time and place.
    Event(usize),
    /// The empty track of this ring: its calendar's name.
    Ring(usize),
}

/// The pointer resting on something that has a tooltip.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rest {
    tip: Tip,
    /// Since when it rests there; the tooltip shows from `since + TOOLTIP_DELAY` on.
    since: Instant,
    /// Where the pointer is, in the canvas: the tooltip stands beside it.
    at: Point,
}

impl Rest {
    /// The tooltip shows at `now`: the pointer has rested [`timeline::TOOLTIP_DELAY`].
    fn shown(&self, now: Instant) -> bool {
        now.checked_duration_since(self.since).is_some_and(|rested| rested >= timeline::TOOLTIP_DELAY)
    }
}

/// Keeps `rest` up to date with what the pointer is `over` (and where) at `now`, and asks for the
/// redraws the tooltip needs. On something new the tooltip shows after the delay – at once when
/// one was showing already, so moving from one event to the next does not wait again; it follows
/// the pointer while it shows; it goes when the pointer leaves for a place without one.
fn track(rest: &mut Option<Rest>, over: Option<(Tip, Point)>, now: Instant) -> Option<Action<Message>> {
    let showing = rest.is_some_and(|rest| rest.shown(now));
    match (rest.as_mut(), over) {
        (Some(was), Some((tip, at))) if was.tip == tip => {
            let moved = was.at != at;
            was.at = at;
            // The moon's tooltip stands beside the moon, not the pointer.
            (showing && moved && !matches!(tip, Tip::Moon(_))).then(Action::request_redraw)
        }
        (_, Some((tip, at))) => {
            let since = if showing { now.checked_sub(timeline::TOOLTIP_DELAY).unwrap_or(now) } else { now };
            *rest = Some(Rest { tip, since, at });
            Some(if showing {
                Action::request_redraw()
            } else {
                Action::request_redraw_at(now + timeline::TOOLTIP_DELAY)
            })
        }
        (Some(_), None) => {
            *rest = None;
            showing.then(Action::request_redraw)
        }
        (None, None) => None,
    }
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
        Dial::fit(bounds, self.margin, self.plan.rings.len())
    }

    /// The moon's target on a canvas of `bounds`, if the window has a moon.
    fn moon(&self, bounds: Size) -> Option<Rectangle> {
        layout::moon_target(&self.dial(bounds), &self.plan.window, &self.plan.nights)
    }

    fn hit(&self, bounds: Size, point: Point) -> Option<Hit> {
        let dial = self.dial(bounds);
        let arcs = layout::arcs(&dial, &self.plan.window, &self.plan.spans);
        layout::hit(&dial, &self.plan.window, &arcs, point)
    }

    /// What a tooltip at `point` would be about: the moon, an event, or the empty track of a ring.
    fn tip_at(&self, bounds: Size, point: Point) -> Option<Tip> {
        if let Some(target) = self.moon(bounds).filter(|target| target.contains(point)) {
            return Some(Tip::Moon(target));
        }
        match self.hit(bounds, point)? {
            Hit::Event(id) => Some(Tip::Event(id)),
            Hit::Free(_) => {
                let dial = self.dial(bounds);
                layout::ring_at(&dial, dial.polar(point).0).map(Tip::Ring)
            }
        }
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

    /// The left button goes down at `point`: the second of two quick presses on the moon opens the
    /// night's card; an event opens at once; elsewhere on the dial the press is kept until it turns
    /// out to be a click or a drag. A tooltip goes away.
    fn press(&self, state: &mut Grip, bounds: Size, point: Point) -> Option<Action<Message>> {
        let now = Instant::now();
        let tooltip = state.rest.take().is_some_and(|rest| rest.shown(now));
        let moon = self.moon(bounds).filter(|target| target.contains(point));
        let first = state.moon_press.take();
        if let Some(target) = moon {
            if first.is_some_and(|first| is_double_click(first, (now, point))) {
                return Some(Action::publish(Message::EditNight { fold: None, at: Some(target) }).and_capture());
            }
            state.moon_press = Some((now, point));
        }
        let hit = self.hit(bounds, point);
        if let Some(Hit::Event(id)) = hit {
            let message = Message::OpenEvent(self.plan.entries.get(id)?.index);
            return Some(Action::publish(message).and_capture());
        }
        let dial = self.dial(bounds);
        if dial.outer <= 0.0 || !dial.grabbable(point) {
            return tooltip.then(Action::request_redraw);
        }
        let (_, angle) = dial.polar(point);
        state.press = Some(Press { origin: point, hit, angle, dragging: false, rest: 0.0 });
        // Nothing is published that would redraw the canvas: ask for it, so the tooltip goes.
        Some(if tooltip { Action::request_redraw().and_capture() } else { Action::capture() })
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
        if radius < drag_dead() {
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
        if self.layer == Layer::Dial {
            return None;
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                self.press(state, bounds.size(), cursor.position_in(bounds)?)
            }
            // A drag goes on beside the canvas: the knob turns as long as the button is down.
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.press.is_some() => {
                self.drag(state, bounds.size(), cursor.position_from(bounds.position())?)
            }
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                let over = cursor
                    .position_in(bounds)
                    .and_then(|point| self.tip_at(bounds.size(), point).map(|tip| (tip, point)));
                track(&mut state.rest, over, Instant::now())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => self.release(state),
            // Without focus the release may never come: a press kept now would turn the dial at
            // the next move of the pointer, and hide the hand over events behind the closed one.
            Event::Window(iced::window::Event::Unfocused) => {
                state.press = None;
                None
            }
            // A redraw asked for at once (the pointer changing from the open hand to the hand, say)
            // replaces the one asked for when the tooltip is due: ask again after every frame
            // until it shows.
            Event::Window(iced::window::Event::RedrawRequested(now)) => {
                let rest = state.rest.filter(|rest| state.press.is_none() && !rest.shown(*now))?;
                Some(Action::request_redraw_at(rest.since + timeline::TOOLTIP_DELAY))
            }
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

    /// The hand over events and the moon, an open hand over the rest of the dial (it can be
    /// turned), a closed one while it is. Elsewhere – the middle, beside the dial – `None`, so the
    /// layers below answer: the "Back to now" button gets its hand and its hover.
    fn mouse_interaction(&self, state: &Grip, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if self.layer == Layer::Dial {
            return mouse::Interaction::None;
        }
        if state.press.is_some_and(|press| press.dragging) {
            return mouse::Interaction::Grabbing;
        }
        let Some(point) = cursor.position_in(bounds) else { return mouse::Interaction::None };
        if self.moon(bounds.size()).is_some_and(|target| target.contains(point)) {
            return mouse::Interaction::Pointer;
        }
        match self.hit(bounds.size(), point) {
            Some(Hit::Event(_)) => mouse::Interaction::Pointer,
            _ if self.dial(bounds.size()).grabbable(point) => mouse::Interaction::Grab,
            _ => mouse::Interaction::None,
        }
    }

    fn draw(
        &self,
        state: &Grip,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let p = palette(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        if self.layer == Layer::Input {
            self.draw_tooltip(&mut frame, state, &p, bounds.size());
            return vec![frame.into_geometry()];
        }
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

        // The tracks of the rings, each faintly in its calendar's colour over the panel or the night.
        let stretches = split(0.0, END, &plan.nights);
        for (ring, named) in plan.rings.iter().enumerate().take(dial.rings) {
            let (inner, outer) = dial.ring(ring);
            let (track, night_track) = (ring_track(named.colour, p.panel), ring_track(named.colour, p.night));
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

        // The working hours: a thin arc just inside the innermost ring, fading like the tracks.
        // Faded with `scale_alpha` like the hour lines, since the colour is translucent too; without
        // rings there is nothing for it to run along.
        let (work_inner, work_outer) = (dial.inner - layout::RING_GAP - WORK_ARC, dial.inner - layout::RING_GAP);
        for (from, to) in plan.working.iter().filter(|_| dial.rings > 0) {
            for piece in pieces(*from, *to) {
                let (start, end) = (window.angle(piece.from), seam(window, piece.to, *to));
                let colour = p.working_hours.scale_alpha(piece.opacity);
                sector(&mut frame, &dial, work_inner, work_outer, start, end, colour);
            }
        }

        // Ticks and numerals.
        let tick = faded(p.text, TICK);
        for (rel, full) in window.ticks() {
            let angle = window.angle(rel);
            let length = if full { tick_long() } else { tick_short() };
            let from = dial.point(dial.ticks, angle);
            let to = dial.point(dial.ticks - length, angle);
            line(&mut frame, from, to, faded(tick, opacity(rel)), 1.0);
        }
        for numeral in window.numerals() {
            let (level, colour) = if numeral.major { (major(), p.text) } else { (typo::micro(), p.text_2) };
            let at = dial.point(dial.numerals, window.angle(numeral.rel));
            label(&mut frame, &format!("{:02}", numeral.hour), at, level, faded(colour, opacity(numeral.rel)));
        }

        // The events, then their titles, then the ring of the open one on top.
        let arcs = layout::arcs(&dial, window, &plan.spans);
        let tones = |entry: &Entry| [tone(&p, entry, plan.now, p.panel), tone(&p, entry, plan.now, p.night)];
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

        for (ring, named) in plan.rings.iter().enumerate() {
            ring_name(&mut frame, &dial, window, ring, named, &p);
        }
        moon(&mut frame, &dial, plan, &p, &faded);
        start_edge(&mut frame, &dial, plan, &p);
        vec![frame.into_geometry()]
    }
}

impl Radar {
    /// The tooltip of what the pointer rests on, on the input layer. None while the button is
    /// down: a press hides it, a drag turns the dial under it.
    fn draw_tooltip(&self, frame: &mut Frame, state: &Grip, p: &Palette, bounds: Size) {
        let Some(rest) = state.rest.filter(|rest| state.press.is_none() && rest.shown(Instant::now())) else {
            return;
        };
        let area = Rectangle::new(Point::ORIGIN, bounds);
        let card = match rest.tip {
            Tip::Moon(target) => return timeline::moon_tooltip(frame, p, target, area),
            Tip::Event(id) => self.plan.entries.get(id).map(event_tip),
            Tip::Ring(ring) => self.plan.rings.get(ring).filter(|named| !named.name.is_empty()).map(|named| Card {
                colour: named.colour,
                title: named.name.clone(),
                time: None,
                place: None,
            }),
        };
        if let Some(card) = card {
            tooltip(frame, p, &card, rest.at, area);
        }
    }
}

/// The colours of an event, composed over its ground (the panel, or the night): the fill in its
/// calendar's colour ([`event_fill`]), the bar in full colour, title and time in light or dark text,
/// whichever reads on the fill ([`on_fill`]); one that has ended faded as in the week grid.
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
    let fill = event_fill(p, colour, ground);
    let title = on_fill(fill);
    Tone { fill: fade(fill), bar: fade(colour), title: fade(title), time: fade(mix(fill, title, TIME_SHARE)) }
}

/// The track of a ring in calendar colour `colour` over `ground` (the panel, or the night).
fn ring_track(colour: Color, ground: Color) -> Color {
    mix(ground, colour, TRACK_TINT)
}

/// An event's fill: its calendar's colour `colour` at [`mini::fill`] over the ring's track, so busy
/// time stands out clearly from the track around it (and dark calendar colours never come out
/// weaker than the empty track).
fn event_fill(p: &Palette, colour: Color, ground: Color) -> Color {
    mix(ring_track(colour, ground), colour, mini::fill(p))
}

/// Light or dark text on `fill`: the dark palette's `text` or the light palette's, whichever has
/// the higher contrast. A fill in a light calendar colour takes dark letters in dark mode too.
fn on_fill(fill: Color) -> Color {
    let (light, dark) = (Palette::dark().text, Palette::light().text);
    if light.relative_contrast(fill) >= dark.relative_contrast(fill) { light } else { dark }
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

/// The level of an event's title in a lane `width` wide: `MICRO` from [`MICRO_LANE`] on, `CAPTION`
/// from [`ONE_LINE`] on, none in narrower lanes.
fn title_level(width: f32) -> Option<typo::Level> {
    if width >= ONE_LINE {
        Some(typo::caption())
    } else if width >= MICRO_LANE {
        Some(typo::micro())
    } else {
        None
    }
}

/// The title along the arc in [`title_level`], cut with "…", and below it the time where the lane
/// takes two lines. Each letter takes the opacity of the place it stands on.
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
    let Some(level) = title_level(width) else { return };
    let room = arc.length() - text_start() - text_end();
    if room < TEXT_MIN {
        return;
    }
    let flipped = arc.flipped();
    // "Up" for the letters: outwards in the top half, inwards in the bottom half.
    let up = if flipped { -1.0 } else { 1.0 };
    let middle = arc.middle();
    let Some(title) = arc_title(&entry.event.title, room, level) else { return };
    let mut lines = vec![(title, level, tone.title, 0.0)];
    if let Some(time) = arc_time(entry, room).filter(|_| width >= TWO_LINES) {
        // Title over time, the pair centred on the middle of the lane.
        let (title_h, time_h) = (line_height(level), line_height(typo::micro()));
        let half = (title_h + time_h) / 2.0;
        lines[0].3 = half - title_h / 2.0;
        lines.push((time, typo::micro(), tone.time, time_h / 2.0 - half));
    }
    for (text, level, colour, lift) in lines {
        if text.is_empty() {
            continue;
        }
        let radius = middle + up * lift;
        let start = if flipped {
            arc.to - (text_end() / radius).to_degrees()
        } else {
            arc.from + (text_start() / radius).to_degrees()
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

/// The title in `level` for an arc with `room` along it, cut with "…"; `None` when fewer than
/// [`MIN_TITLE_LETTERS`] of it would be left ("Da…" says nothing; the tooltip has the title).
fn arc_title(title: &str, room: f32, level: typo::Level) -> Option<String> {
    let cut = ellipsize(title, level, room);
    let letters = cut.trim_end_matches(typo::ELLIPSIS).chars().count();
    (!cut.is_empty() && letters >= MIN_TITLE_LETTERS.min(title.chars().count())).then_some(cut)
}

/// The time line for an arc with `room` along it: "18:30–19:30", or the start alone when the
/// range does not fit, or nothing (a cut "09:…" says nothing).
fn arc_time(entry: &Entry, room: f32) -> Option<String> {
    let start = format_time(entry.event.start.time());
    let range = format!("{start}–{}", format_time(entry.event.end.time()));
    [range, start].into_iter().find(|time| text_width(time, typo::micro()) <= room)
}

/// The colours of the moon: the icon (`text_2`), its plaque (`panel`) and the plaque's rim
/// (`line`). The icon keeps 3:1 against the plaque in both modes, whatever lies around it.
fn moon_colours(p: &Palette) -> (Color, Color, Color) {
    (p.text_2, p.panel, p.line)
}

/// The moon on the numerals' circle at the first half hour of the night outside the fading quarter
/// ([`layout::moon`]): the icon at full size on a small round plaque, so it reads on the night and
/// on the panel alike. Only a moon in the fading quarter fades.
fn moon(frame: &mut Frame, dial: &Dial, plan: &Plan, p: &Palette, faded: &dyn Fn(Color, f32) -> Color) {
    let Some(rel) = layout::moon(&plan.window, &plan.nights) else { return };
    let alpha = opacity(rel);
    let at = dial.point(dial.numerals, plan.window.angle(rel));
    let (icon_colour, plaque, rim) = moon_colours(p);
    let disc = Path::circle(at, MOON_PLAQUE / 2.0);
    frame.fill(&disc, faded(plaque, alpha));
    frame.stroke(&disc, Stroke::default().with_color(rim.scale_alpha(alpha)).with_width(size::HAIRLINE));
    let half = icon::SIZE / 2.0;
    let bounds = Rectangle::new(Point::new(at.x - half, at.y - half), Size::new(icon::SIZE, icon::SIZE));
    frame.draw_svg(bounds, &Icon::Moon.tinted(faded(icon_colour, alpha)));
}

/// The name of ring `ring` in the last hour before the edge ([`layout::ring_name`]): a dot in the
/// calendar's colour and the name in `MICRO`, `text_2`, at full opacity (it does not fade with the
/// quarter it stands in), upright like the titles.
fn ring_name(frame: &mut Frame, dial: &Dial, window: &Window, ring: usize, named: &RingLabel, p: &Palette) {
    let Some(place) = layout::ring_name(dial, window, ring, &named.name) else { return };
    frame.fill(&Path::circle(dial.point(place.radius, place.dot), layout::NAME_DOT), named.colour);
    let Some(text) = place.text else { return };
    for glyph in layout::glyphs(&text.text, typo::micro(), place.radius, text.start, text.flipped) {
        let at = dial.point(place.radius, glyph.angle);
        frame.with_save(|frame| {
            frame.translate(iced::Vector::new(at.x, at.y));
            frame.rotate(glyph.turn);
            label(frame, &glyph.letter, Point::ORIGIN, typo::micro(), p.text_2);
        });
    }
}

/// What a tooltip card says: a dot in `colour` and `title`, below it the `time` and the `place`.
#[derive(Debug, Clone, PartialEq)]
struct Card {
    colour: Color,
    title: String,
    time: Option<String>,
    place: Option<String>,
}

/// The card for an event: its title, "14:00–15:00" and its place, if it has one.
fn event_tip(entry: &Entry) -> Card {
    let (start, end) = (format_time(entry.event.start.time()), format_time(entry.event.end.time()));
    Card {
        colour: entry.event.color,
        title: entry.event.title.clone(),
        time: Some(format!("{start}–{end}")),
        place: entry.event.location.clone(),
    }
}

/// One line of a tooltip card as drawn.
#[derive(Debug, Clone, PartialEq)]
struct CardLine {
    /// The text, cut with "…" to [`tip_max()`].
    text: String,
    level: typo::Level,
    /// From the card's inner left edge to the text.
    indent: f32,
    /// What stands before the text: the calendar's dot (the title), the pin (the place), nothing.
    mark: CardMark,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CardMark {
    Dot,
    Pin,
    None,
}

/// The lines of `card` as drawn and the size of the card around them: the title in
/// `BODY_STRONG` behind the dot; under it, lined up with the title, the time in tabular `META` and
/// the place in `META` behind its pin.
fn card_layout(card: &Card) -> (Vec<CardLine>, Size) {
    let indent = 2.0 * tip_dot() + space::s();
    let line = |text: &str, level: typo::Level, indent: f32, mark: CardMark| CardLine {
        text: ellipsize(text, level, tip_max() - indent),
        level,
        indent,
        mark,
    };
    let mut lines = vec![line(&card.title, typo::body_strong(), indent, CardMark::Dot)];
    if let Some(time) = &card.time {
        lines.push(line(time, typo::meta().tabular(), indent, CardMark::None));
    }
    if let Some(place) = &card.place {
        lines.push(line(place, typo::meta(), indent + icon::SIZE_SM + space::xs(), CardMark::Pin));
    }
    let width = lines.iter().map(|line| line.indent + text_width(&line.text, line.level)).fold(0.0, f32::max);
    let height = lines.iter().map(|line| line_height(line.level)).sum::<f32>();
    (lines, Size::new(width + 2.0 * space::s(), height + 2.0 * space::s()))
}

/// The tooltip `card` beside the pointer at `pointer`, never over the edges of `area` (the
/// canvas): `menu` surface with the `line_strong` rim like the suite's tooltips, the title in
/// `text`, time and place in `text_2`.
fn tooltip(frame: &mut Frame, p: &Palette, card: &Card, pointer: Point, area: Rectangle) {
    let (lines, extent) = card_layout(card);
    let origin = layout::tip_origin(pointer, extent, area);
    let surface = Path::rounded_rectangle(origin, extent, Radius::from(radius::sm()));
    frame.fill(&surface, p.menu);
    frame.stroke(&surface, Stroke::default().with_color(p.line_strong).with_width(size::HAIRLINE));
    let left = origin.x + space::s();
    let mut top = origin.y + space::s();
    for line in &lines {
        let middle = top + line_height(line.level) / 2.0;
        let colour = match line.mark {
            CardMark::Dot => {
                frame.fill(&Path::circle(Point::new(left + tip_dot(), middle), tip_dot()), card.colour);
                p.text
            }
            CardMark::Pin => {
                let pin = icon::SIZE_SM;
                let at = Point::new(left + 2.0 * tip_dot() + space::s(), middle - pin / 2.0);
                frame.draw_svg(Rectangle::new(at, Size::new(pin, pin)), &Icon::MapPin.tinted(p.text_2));
                p.text_2
            }
            CardMark::None => p.text_2,
        };
        let at = Point::new(left + line.indent, middle);
        week::label(frame, &line.text, at, line.level, colour, Alignment::Left, Vertical::Center);
        top += line_height(line.level);
    }
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
    let from = dial.point((dial.inner - edge_inside()).max(0.0), angle);
    let to = dial.point(dial.ticks + EDGE_OUTSIDE, angle);
    line(frame, from, to, edge, EDGE_WIDTH);
    let at = dial.point(dial.numerals, angle);
    let height = line_height(chip_level());
    let pill =
        Rectangle::new(Point::new(at.x - chip_width() / 2.0, at.y - height / 2.0), Size::new(chip_width(), height));
    frame.fill(&Path::rounded_rectangle(pill.position(), pill.size(), Radius::from(radius::xs())), chip);
    label(frame, &format_time(plan.window.start.time()), at, chip_level(), figures);
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
    use iced::window::RedrawRequest;
    use torro_pim::{Event as CalendarEvent, EventTime};

    use super::super::Item;
    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("date").and_hms_opt(hour, minute, 0).expect("time")
    }

    #[test]
    fn the_week_strip_gives_way_in_a_low_tile() {
        // A tall narrow tile (480 × 856 under the header): strip and full-width dial both fit.
        assert!(strip_fits(Size::new(480.0, 856.0)));
        // A low tile (579 × 377): the strip would push the dial out of view.
        assert!(!strip_fits(Size::new(579.0, 377.0)));
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
            recurrence_id: None,
            repeat: None,
            time_zone: None,
            url: None,
            status: None,
            geo: None,
            organizer: None,
            attendees: Vec::new(),
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

    /// The demo's calendar colours.
    const BLUE: Color = Color::from_rgb(0x34 as f32 / 255.0, 0x74 as f32 / 255.0, 0xE6 as f32 / 255.0);
    const TEAL: Color = Color::from_rgb(0x11 as f32 / 255.0, 0x9C as f32 / 255.0, 0x8E as f32 / 255.0);
    const RED: Color = Color::from_rgb(0xE0 as f32 / 255.0, 0x48 as f32 / 255.0, 0x5D as f32 / 255.0);

    fn ring(id: &str, name: &str, host: &str, colour: Color) -> Ring {
        Ring { id: id.into(), name: name.into(), host: Some(host.into()), colour }
    }

    /// Family, Personal and Work, as the sidebar lists them.
    fn calendars() -> Vec<Ring> {
        vec![
            ring("family", "Family", "home.example.org", RED),
            ring("personal", "Personal", "home.example.org", TEAL),
            ring("work", "Work", "work.example.org", BLUE),
        ]
    }

    fn ids() -> Vec<String> {
        calendars().into_iter().map(|calendar| calendar.id).collect()
    }

    #[test]
    fn every_visible_calendar_has_a_ring_in_sidebar_order() {
        let events = events();
        let plan = Plan::new(&data(&events), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        // Family has only breakfast (over) and an all-day holiday, and still its ring, the first.
        assert_eq!(plan.rings.len(), 3);
        let ring_of = |title: &str| {
            plan.spans.iter().find(|s| plan.entries[s.id].event.title == title).map(|s| s.ring).expect(title)
        };
        assert_eq!((ring_of("Run"), ring_of("API design workshop")), (1, 2), "personal before work, as listed");
        assert!(plan.spans.iter().all(|s| plan.entries[s.id].event.title != "Breakfast"));
        let workshop = plan.spans.iter().find(|s| s.ring == 2 && !s.starts).expect("the workshop runs");
        assert_eq!(workshop.from, 0.0, "cut to the start of the window");
        // Looking back brings breakfast into the family ring; the rings stay as they are.
        let back = Plan::new(&data(&events), &calendars(), -7 * 60, Night::DEFAULT, WorkingHours::DEFAULT);
        assert_eq!(back.rings.len(), 3);
        assert!(back.looking);
        assert_eq!(back.spans.iter().find(|s| plan.entries[s.id].event.title == "Breakfast").map(|s| s.ring), Some(0));
    }

    #[test]
    fn rings_show_without_any_events() {
        let plan = Plan::new(&data(&[]), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        assert_eq!((plan.rings.len(), plan.spans.len()), (3, 0));
        // A hidden calendar is not in the list the app passes: no ring for it.
        let plan = Plan::new(&data(&[]), &calendars()[1..], 0, Night::DEFAULT, WorkingHours::DEFAULT);
        assert_eq!(plan.rings.len(), 2);
        // No calendars at all: a bare dial.
        assert_eq!(Plan::new(&data(&[]), &[], 0, Night::DEFAULT, WorkingHours::DEFAULT).rings.len(), 0);
    }

    #[test]
    fn unknown_calendars_ring_last() {
        let order = ring_order(["zeta", "work", "alpha"].into_iter(), &ids());
        assert_eq!(order, ["work", "alpha", "zeta"]);
        // The sidebar's calendars once each, then a used one it does not know.
        let order = ring_order(["family", "personal", "work", "work", "zeta"].into_iter(), &ids());
        assert_eq!(order, ["family", "personal", "work", "zeta"]);
        // A ring the sidebar does not know has no name, only the dot in its events' colour.
        let events = [event("zeta", "Stray", local(13, 15, 0), local(13, 16, 0))];
        let plan = Plan::new(&data_on(&events, 13), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        assert_eq!(plan.rings.last().map(|ring| ring.name.as_str()), Some(""));
    }

    #[test]
    fn a_ring_names_the_host_only_when_two_visible_calendars_share_the_name() {
        let personal = ring("a", "Persönlich", "cloud.imt24.de", TEAL);
        let other = ring("b", "Persönlich", "nextcloud.rheinschmiede.net", BLUE);
        let work = ring("c", "Work", "cloud.imt24.de", RED);
        let all = [personal.clone(), other.clone(), work.clone()];
        assert_eq!(ring_label(&personal, &all), "Persönlich · cloud.imt24.de");
        assert_eq!(ring_label(&other, &all), "Persönlich · nextcloud.rheinschmiede.net");
        assert_eq!(ring_label(&work, &all), "Work", "a name of its own needs no host");
        assert_eq!(ring_label(&personal, &[personal.clone(), work.clone()]), "Persönlich", "the twin is hidden");
        let without = Ring { host: None, ..other };
        assert_eq!(ring_label(&without, &[personal, without.clone()]), "Persönlich", "no account, no host");
        // The plan carries the names and colours in ring order.
        let plan = Plan::new(&data(&[]), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        let named: Vec<(&str, Color)> = plan.rings.iter().map(|ring| (ring.name.as_str(), ring.colour)).collect();
        assert_eq!(named, [("Family", RED), ("Personal", TEAL), ("Work", BLUE)]);
    }

    #[test]
    fn the_night_comes_with_the_window() {
        let events = events();
        // 14:20: the night from 22:00 holds the late call.
        let plan = Plan::new(&data(&events), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        assert_eq!(plan.nights, [(460.0, END)]);
        // An empty Sunday: no night between 08:00 and 19:54.
        let sunday = Plan::new(&data_on(&[], 18), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        assert!(sunday.nights.is_empty());
        // Looking back at last night: the night until 06:00 lies in the window.
        let back = Plan::new(&data(&events), &calendars(), -12 * 60, Night::DEFAULT, WorkingHours::DEFAULT);
        assert_eq!(back.window.start, at(13, 2, 20));
        assert_eq!(back.nights, [(0.0, 220.0)], "until 06:00");
    }

    #[test]
    fn the_middle_offers_back_to_now_only_away_from_now() {
        let events = events();
        let dial = |plan: &Plan| Dial::fit(Size::new(800.0, 600.0), stage_margin(), plan.rings.len());
        // Today at 14:20: date and time only, no button.
        let today = Plan::new(&data(&events), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        assert!(!today.looking);
        // Another day, and the dial turned on today: the button is there.
        let tomorrow = Plan::new(&data_on(&events, 14), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        let turned = Plan::new(&data(&events), &calendars(), 60, Night::DEFAULT, WorkingHours::DEFAULT);
        assert!(tomorrow.looking && turned.looking);
        // The middle builds in each case, with and without events in the window.
        let empty = Plan::new(&data(&[]), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        for plan in [&today, &tomorrow, &turned, &empty] {
            let _ = middle(plan, &dial(plan));
        }
    }

    /// The input layer of the dial of a 1000 × 900 canvas with the demo events.
    fn radar() -> Radar {
        let events = events();
        let plan = Plan::new(&data(&events), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        Radar { plan, margin: stage_margin(), wheel: true, layer: Layer::Input }
    }

    #[test]
    fn today_starts_at_now_with_a_red_edge_and_other_days_in_the_morning_with_a_grey_one() {
        let events = events();
        let today = Plan::new(&data(&events), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        assert_eq!((today.window.start, today.looking), (at(13, 14, 20), false));
        let turned = Plan::new(&data(&events), &calendars(), 60, Night::DEFAULT, WorkingHours::DEFAULT);
        assert_eq!((turned.window.start, turned.looking), (at(13, 15, 20), true));
        // Wednesday: the late call at midnight opens it as early as the night allows.
        let tomorrow = Plan::new(&data_on(&events, 14), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        assert_eq!((tomorrow.window.start, tomorrow.looking), (at(14, 6, 0), true));
        // An event begun the evening before does not count: 08:00.
        let overnight = [event("work", "Release", local(14, 22, 0), local(15, 1, 0))];
        assert_eq!(start(&data_on(&overnight, 15), 0, Night::DEFAULT), at(15, 8, 0));
        // An early event opens the day at its hour, an all-day one does not.
        let early = vec![
            event("work", "Early train", local(15, 6, 30), local(15, 7, 30)),
            event("family", "Trip", EventTime::Date(at(15, 0, 0).date()), EventTime::Date(at(16, 0, 0).date())),
        ];
        assert_eq!(start(&data_on(&early, 15), 0, Night::DEFAULT), at(15, 6, 0));
        assert_eq!(start(&data_on(&early[1..], 15), 0, Night::DEFAULT), at(15, 8, 0), "all-day events do not count");
        assert_eq!(start(&data_on(&early, 15), -90, Night::DEFAULT), at(15, 4, 30), "turned back before it");
        assert_eq!(start(&data_on(&early, 15), 99 * 60, Night::DEFAULT), at(16, 6, 0), "a day ahead at most");
    }

    #[test]
    fn the_edge_is_red_at_now_and_grey_elsewhere() {
        for theme in [theme::theme(), theme::light()] {
            let p = palette(&theme);
            assert_eq!(edge_colours(&p, false), (p.accent, p.accent, p.on_accent));
            assert_eq!(edge_colours(&p, true), (p.text_2, p.selected, p.text));
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
    }

    #[test]
    fn the_hand_shows_over_events_and_the_open_hand_over_the_rest_of_the_dial() {
        let radar = radar();
        let bounds = Size::new(1000.0, 900.0);
        let rect = Rectangle::new(Point::ORIGIN, bounds);
        let dial = radar.dial(bounds);
        let arcs = layout::arcs(&dial, &radar.plan.window, &radar.plan.spans);
        let cursor = |point: Point| mouse::Cursor::Available(point);
        let state = Grip::default();
        // Every arc, in its middle and near both of its ends.
        for arc in &arcs {
            for angle in [arc.from + 0.2, (arc.from + arc.to) / 2.0, arc.to - 0.2] {
                let at = dial.point(arc.middle(), angle);
                assert_eq!(radar.mouse_interaction(&state, rect, cursor(at)), mouse::Interaction::Pointer, "{arc:?}");
            }
        }
        // The empty track of a ring and the numerals can be turned.
        let free = dial.point(dial.ring(0).0 + 1.0, radar.plan.window.angle(410.0));
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(free)), mouse::Interaction::Grab);
        let numerals = dial.point(dial.numerals, radar.plan.window.angle(100.0));
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(numerals)), mouse::Interaction::Grab);
        // The middle and the corners leave the pointer to the layers below: the "Back to now"
        // button lies in the middle.
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(dial.center)), mouse::Interaction::None);
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(Point::new(2.0, 2.0))), mouse::Interaction::None);
        assert_eq!(radar.mouse_interaction(&state, rect, mouse::Cursor::Unavailable), mouse::Interaction::None);
        // The dial's own layer takes no input at all.
        let dial_layer = Radar { layer: Layer::Dial, ..radar };
        let on_arc = arcs.first().map(|arc| dial.point(arc.middle(), (arc.from + arc.to) / 2.0)).expect("an arc");
        assert_eq!(dial_layer.mouse_interaction(&state, rect, cursor(on_arc)), mouse::Interaction::None);
        let mut grip = Grip::default();
        assert!(dial_layer.update(&mut grip, &mouse(PRESS), rect, cursor(on_arc)).is_none());
    }

    #[test]
    fn a_lost_focus_ends_a_press_so_the_next_move_does_not_turn_the_dial() {
        let radar = radar();
        let rect = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 900.0));
        let dial = radar.dial(rect.size());
        let on_dial = |angle: f32| mouse::Cursor::Available(dial.point(dial.numerals, angle));
        let mut state = Grip::default();
        let _ = radar.update(&mut state, &mouse(PRESS), rect, on_dial(350.0));
        assert!(state.press.is_some());
        let _ = radar.update(&mut state, &Event::Window(iced::window::Event::Unfocused), rect, on_dial(350.0));
        assert_eq!(state.press, None);
        let moved_on = dial.point(dial.numerals, 20.0);
        assert!(published(radar.update(&mut state, &moved(moved_on), rect, on_dial(20.0))).is_none(), "no turn");
        assert_ne!(radar.mouse_interaction(&state, rect, on_dial(20.0)), mouse::Interaction::Grabbing);
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
    fn a_double_click_on_the_moon_opens_the_night() {
        let radar = radar();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 900.0));
        let moon = radar.moon(bounds.size()).expect("the demo window has a night");
        let mut state = Grip::default();
        let at = mouse::Cursor::Available(moon.center());
        assert_eq!(radar.mouse_interaction(&state, bounds, at), mouse::Interaction::Pointer);
        assert!(radar.update(&mut state, &moved(moon.center()), bounds, at).is_some(), "the tooltip is on its way");
        assert!(state.rest.is_some_and(|rest| rest.tip == Tip::Moon(moon)));
        assert!(published(radar.update(&mut state, &mouse(PRESS), bounds, at)).is_none(), "one click: nothing");
        let _ = radar.update(&mut state, &mouse(RELEASE), bounds, at);
        let second = published(radar.update(&mut state, &mouse(PRESS), bounds, at));
        assert!(
            matches!(second, Some(Message::EditNight { fold: None, at: Some(target) }) if target == moon),
            "{second:?}"
        );
        let none = Night::none_at(Night::DEFAULT.from);
        let events = events();
        let without = Radar { plan: Plan::new(&data(&events), &calendars(), 0, none, WorkingHours::DEFAULT), ..radar };
        assert_eq!(without.moon(bounds.size()), None, "no night, no moon");
    }

    #[test]
    fn arc_text_is_cut_but_never_to_nothing_worth_reading() {
        assert_eq!(arc_title("Run", 100.0, typo::caption()).as_deref(), Some("Run"));
        let cut = arc_title("API design workshop", 90.0, typo::caption()).expect("room for a part");
        assert!(cut.starts_with("API") && cut.ends_with(typo::ELLIPSIS), "{cut}");
        assert_eq!(arc_title("Daily Standup", 20.0, typo::caption()), None, "\"Da…\" says nothing");
        let micro = arc_title("Daily Standup", 30.0, typo::micro()).expect("smaller letters, more of them");
        assert!(micro.starts_with("Dai") && micro.ends_with(typo::ELLIPSIS), "{micro}");
        let events = events();
        let plan = Plan::new(&data(&events), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        let run = plan.entries.iter().find(|e| e.event.title == "Run").expect("run");
        assert_eq!(arc_time(run, 200.0).as_deref(), Some("18:30–19:30"));
        assert_eq!(arc_time(run, 40.0).as_deref(), Some("18:30"), "the start alone");
        assert_eq!(arc_time(run, 20.0), None, "no cut time");
    }

    #[test]
    fn the_view_builds_in_every_place_of_the_list() {
        let events = events();
        for place in [ListAt::Props, ListAt::Beside, ListAt::Below] {
            let _ = view(
                data(&events),
                &calendars(),
                90,
                at(14, 0, 0).date(),
                place,
                Night::DEFAULT,
                WorkingHours::DEFAULT,
                &Anchor::new(),
            );
        }
        let _ = list(&data(&events), at(14, 0, 0).date(), Night::DEFAULT);
        let _ = narrow(
            Plan::new(&data(&events), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT),
            &mini::Coming::new(&data(&events), at(14, 0, 0).date(), Night::DEFAULT),
            Size::new(480.0, 800.0),
            &Anchor::new(),
        );
        let plan = Plan::new(&data(&events), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        let _ = middle(&plan, &Dial::fit(Size::new(800.0, 600.0), stage_margin(), plan.rings.len()));
    }

    /// The demo's calendar colours, the suite's standard ones and the default for calendars
    /// without a colour (`#7FB2FF`).
    fn colours(p: &Palette) -> Vec<Color> {
        let mut all = vec![BLUE, TEAL, RED, torro_gui::widgets::collection_color(None)];
        all.extend(p.calendar);
        all
    }

    #[test]
    fn titles_read_on_the_strong_fill_of_every_calendar_colour_in_both_modes() {
        for (mode, p) in [("dark", Palette::dark()), ("light", Palette::light())] {
            for ground in [p.panel, p.night] {
                for colour in colours(&p) {
                    let fill = event_fill(&p, colour, ground);
                    let title = on_fill(fill);
                    let ratio = title.relative_contrast(fill);
                    assert!(ratio >= 4.5, "{mode}: title {ratio:.2} on {colour:?}");
                }
                // On the demo's own colours the time reads too, and busy time stands clearly apart
                // from the track around it.
                for colour in [BLUE, TEAL, RED] {
                    let fill = event_fill(&p, colour, ground);
                    let time = mix(fill, on_fill(fill), TIME_SHARE);
                    assert!(time.relative_contrast(fill) >= 4.5, "{mode}: time on {colour:?}");
                    let track = ring_track(colour, ground);
                    assert!(fill.relative_contrast(track) >= 1.5, "{mode}: fill on track {colour:?}");
                }
            }
        }
        // Much stronger than the week grid's tint, and the mini dials take the same share.
        assert!(mini::FILL_DARK > Palette::dark().tint * 2.0 && mini::FILL_LIGHT > Palette::light().tint * 2.0);
        assert_eq!((mini::fill(&Palette::dark()), mini::fill(&Palette::light())), (mini::FILL_DARK, mini::FILL_LIGHT));
    }

    #[test]
    fn a_past_event_stays_faded_and_its_track_stays_quiet() {
        let p = Palette::dark();
        let events = events();
        let plan = Plan::new(&data(&events), &calendars(), -7 * 60, Night::DEFAULT, WorkingHours::DEFAULT);
        let breakfast = plan.entries.iter().find(|e| e.event.title == "Breakfast").expect("breakfast");
        let past = tone(&p, breakfast, plan.now, p.panel);
        let fill = event_fill(&p, breakfast.event.color, p.panel);
        assert_eq!(past.fill, mix(p.panel, fill, PAST_OPACITY));
        // The track is the calendar's colour at a tenth: much closer to the panel than to the fill.
        let track = ring_track(BLUE, p.panel);
        assert!(track.relative_contrast(p.panel) < 1.3);
    }

    #[test]
    fn the_title_takes_micro_in_narrow_lanes_caption_in_wider_ones_and_none_below() {
        assert_eq!(title_level(15.9), None);
        assert_eq!(title_level(16.0), Some(typo::micro()));
        assert_eq!(title_level(21.9), Some(typo::micro()));
        assert_eq!(title_level(22.0), Some(typo::caption()));
        assert_eq!(title_level(64.0), Some(typo::caption()));
    }

    #[test]
    fn every_event_of_45_minutes_carries_its_title_in_the_wide_and_the_medium_dial() {
        // The dial's stage in a wide window (1600 × 1000) and in a medium one (800 × 600).
        for stage in [Size::new(1050.0, 900.0), Size::new(470.0, 510.0)] {
            let dial = Dial::fit(stage, stage_margin(), calendars().len());
            let window = Window::new(at(13, 14, 20));
            for ring in 0..dial.rings {
                for start in [30.0, 200.0, 330.0, 480.0] {
                    let span = Span { id: 0, ring, from: start, to: start + 45.0, starts: true };
                    let arc = layout::arcs(&dial, &window, &[span])[0];
                    let level = title_level(arc.width()).expect("a level for the lane");
                    let room = arc.length() - text_start() - text_end();
                    assert!(room >= TEXT_MIN, "{stage:?} ring {ring} at {start}: {room}");
                    for title in ["Lunch with Jana", "Dentist", "Run", "Client presentation"] {
                        assert!(arc_title(title, room, level).is_some(), "{stage:?} ring {ring} at {start}: {title}");
                    }
                }
            }
        }
    }

    fn instant(base: Instant, millis: u64) -> Instant {
        base + std::time::Duration::from_millis(millis)
    }

    #[test]
    fn a_tooltip_shows_after_a_rest_moves_on_at_once_and_goes() {
        let t0 = Instant::now();
        let delay = timeline::TOOLTIP_DELAY.as_millis() as u64;
        let (a, b) = (Point::new(10.0, 10.0), Point::new(14.0, 12.0));
        let mut rest = None;
        // Coming onto an event: a redraw is asked for when the delay is over.
        let action = track(&mut rest, Some((Tip::Event(1), a)), t0);
        assert!(action.is_some_and(|action| action.into_inner().1 == RedrawRequest::At(t0 + timeline::TOOLTIP_DELAY)));
        assert!(rest.is_some_and(|rest| !rest.shown(instant(t0, delay - 1)) && rest.shown(instant(t0, delay))));
        // Moving on the same event before it shows: nothing to redraw, the rest keeps its start.
        assert!(track(&mut rest, Some((Tip::Event(1), b)), instant(t0, 100)).is_none());
        assert!(rest.is_some_and(|rest| rest.since == t0 && rest.at == b));
        // Shown, it follows the pointer.
        let later = instant(t0, delay + 50);
        let follow = track(&mut rest, Some((Tip::Event(1), a)), later);
        assert!(follow.is_some_and(|action| action.into_inner().1 == RedrawRequest::NextFrame));
        // On to the next event while one shows: the next one shows at once.
        let _ = track(&mut rest, Some((Tip::Event(2), b)), later);
        assert!(rest.is_some_and(|rest| rest.tip == Tip::Event(2) && rest.shown(later)));
        // Off the dial: gone, with a redraw to take it away.
        let gone = track(&mut rest, None, later);
        assert!(gone.is_some_and(|action| action.into_inner().1 == RedrawRequest::NextFrame));
        assert_eq!(rest, None);
        // Onto another event after that: the delay again.
        let _ = track(&mut rest, Some((Tip::Event(3), a)), later);
        assert!(rest.is_some_and(|rest| !rest.shown(later)));
    }

    #[test]
    fn every_frame_before_the_tooltip_is_due_asks_for_the_frame_that_shows_it() {
        let radar = radar();
        let rect = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 900.0));
        let now = Instant::now();
        let at = Point::new(10.0, 10.0);
        let mut state = Grip { rest: Some(Rest { tip: Tip::Event(0), since: now, at }), ..Grip::default() };
        let frame = Event::Window(iced::window::Event::RedrawRequested(now));
        let cursor = mouse::Cursor::Available(at);
        let action = radar.update(&mut state, &frame, rect, cursor).map(|action| action.into_inner().1);
        assert_eq!(action, Some(RedrawRequest::At(now + timeline::TOOLTIP_DELAY)));
        // Once it shows, frames ask for nothing more.
        let due = Event::Window(iced::window::Event::RedrawRequested(now + timeline::TOOLTIP_DELAY));
        assert!(radar.update(&mut state, &due, rect, cursor).is_none());
    }

    #[test]
    fn a_press_takes_the_tooltip_away_and_the_empty_track_names_its_calendar() {
        let radar = radar();
        let rect = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 900.0));
        let dial = radar.dial(rect.size());
        let cursor = |point: Point| mouse::Cursor::Available(point);
        let mut state = Grip::default();
        // Resting on the empty track of the middle ring (Personal) at 16:00.
        let (inner, outer) = dial.ring(1);
        let track_at = dial.point((inner + outer) / 2.0, radar.plan.window.angle(100.0));
        let _ = radar.update(&mut state, &moved(track_at), rect, cursor(track_at));
        assert_eq!(state.rest.map(|rest| rest.tip), Some(Tip::Ring(1)));
        assert_eq!(radar.mouse_interaction(&state, rect, cursor(track_at)), mouse::Interaction::Grab, "still a knob");
        assert_eq!(radar.plan.rings[1].name, "Personal");
        // The gap between two rings has no tooltip.
        let gap = dial.point(inner - layout::RING_GAP / 2.0, radar.plan.window.angle(100.0));
        assert_eq!(radar.tip_at(rect.size(), gap), None);
        // On an event: the event.
        let arcs = layout::arcs(&dial, &radar.plan.window, &radar.plan.spans);
        let run = arcs.iter().find(|a| radar.plan.entries[a.id].event.title == "Run").expect("run");
        let on_run = dial.point(run.middle(), (run.from + run.to) / 2.0);
        assert_eq!(radar.tip_at(rect.size(), on_run), Some(Tip::Event(run.id)));
        // A press – on an event or on the track – takes the tooltip away.
        let _ = radar.update(&mut state, &moved(on_run), rect, cursor(on_run));
        assert!(state.rest.is_some());
        let _ = radar.update(&mut state, &mouse(PRESS), rect, cursor(on_run));
        assert_eq!(state.rest, None);
        let _ = radar.update(&mut state, &moved(track_at), rect, cursor(track_at));
        let _ = radar.update(&mut state, &mouse(PRESS), rect, cursor(track_at));
        assert_eq!(state.rest, None);
        // While the button is down nothing new comes to rest: the move is a drag.
        let _ = radar.update(&mut state, &moved(on_run), rect, cursor(on_run));
        assert_eq!(state.rest, None);
    }

    #[test]
    fn the_tooltip_card_says_title_time_and_place_and_is_cut_to_its_width() {
        let events = vec![CalendarEvent {
            location: Some("Room Rhine, second floor, at the far end of the corridor".into()),
            ..event("work", "API design workshop with the whole platform team", local(13, 14, 0), local(13, 15, 0))
        }];
        let plan = Plan::new(&data(&events), &calendars(), 0, Night::DEFAULT, WorkingHours::DEFAULT);
        let card = event_tip(&plan.entries[0]);
        assert_eq!(card.time.as_deref(), Some("14:00–15:00"));
        let (lines, extent) = card_layout(&card);
        let marks: Vec<CardMark> = lines.iter().map(|line| line.mark).collect();
        assert_eq!(marks, [CardMark::Dot, CardMark::None, CardMark::Pin]);
        assert!(lines[0].text.ends_with(typo::ELLIPSIS) && lines[2].text.ends_with(typo::ELLIPSIS));
        assert!(extent.width <= tip_max() + 2.0 * space::s() + 0.5, "{extent:?}");
        let height = line_height(typo::body_strong()) + 2.0 * line_height(typo::meta()) + 2.0 * space::s();
        assert!((extent.height - height).abs() < 0.01);
        // Without a place, two lines; the time lines up with the title.
        let short = Card { place: None, ..card };
        let (lines, _) = card_layout(&short);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].indent, lines[1].indent);
    }

    #[test]
    fn the_moon_reads_on_its_plaque_in_both_modes() {
        for p in [Palette::dark(), Palette::light()] {
            let (icon, plaque, _) = moon_colours(&p);
            assert!(icon.relative_contrast(plaque) >= 3.0);
            // The plaque stands on the panel or the night; either way the moon is the same.
            assert_eq!(plaque, p.panel);
        }
        assert!(MOON_PLAQUE <= timeline::moon_target_size(), "the target covers the plaque");
    }
}
