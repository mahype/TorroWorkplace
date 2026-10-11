//! Timeline view (task K.21 in `docs/tasks/kalender-aufgaben.md`, prototype
//! `docs/prototypes/calendar/zeitstrahl.html`): the days run as one film strip from left to right
//! on a canvas in a sideways scrollable, the day before the anchor and the seven days from it.
//!
//! Top to bottom: the day labels ("Tue 13", today's number in a red circle; each sticks to the
//! left edge while its day is in view), the hours, the all-day chips (several days as one chip),
//! the events as full-height columns with their title and time turned to read upwards, and
//! "3 h free" under gaps of two hours or more. Overlapping events share the height in lanes.
//!
//! Nights (22:00–06:00) are folded into a narrow band with zigzag edges, a moon and "Night · 8 h";
//! events in it are dots in their calendar colour. A click on the band unfolds that night (hours at
//! the day's pace on a darker ground, "Collapse night" at its top); "Nights" in the header unfolds
//! or folds them all. A strong line parts the days at midnight; a red line marks now.
//!
//! Clicks: an event or chip → `OpenEvent`; free time → `NewEvent` at that time (rounded down to a
//! quarter hour); the all-day row → an all-day `NewEvent`; a folded night only unfolds. The wheel
//! scrolls sideways. Every colour comes from the theme's [`Palette`] at draw time. The geometry
//! (time ↔ x, nights, lanes, chips, free gaps, hits) is in `timeline/layout.rs`.
//!
//! Which nights are open and where the scrollable stands is window state ([`State`]), kept by the
//! app like a scroll position; nothing goes to the core.

mod layout;

use std::collections::BTreeSet;
use std::f32::consts::FRAC_PI_2;

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use iced::alignment::Vertical;
use iced::border::Radius;
use iced::widget::canvas::{self, Action, Frame, Path, Stroke};
use iced::widget::scrollable::{AbsoluteOffset, Direction};
use iced::widget::text::Alignment;
use iced::widget::{Id, operation, scrollable};
use iced::{Color, Element, Event, Length, Point, Rectangle, Size, Task, Theme, Vector, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_time, weekday_name};
use torro_gui::icon::{self, Icon};
use torro_gui::style;
use torro_gui::theme::{Palette, mix, palette, radius, space};
use torro_gui::typo::{self, ellipsize, text_width};

use layout::{Block, Hit, Rows, Segment, Span, Timed, Track};

use super::ViewData;
use super::week::{self, BAR, DAY_CIRCLE, DAY_NUMBER, FREE_MINUTES, NOW_DOT, NOW_LABEL, NOW_LABEL_WIDTH, Tone};
use crate::app::Message;

/// Id of the timeline's scrollable, for the scroll to now ([`State::show`]) and the wheel.
pub const SCROLL_ID: Id = Id::new("torrocalendar.timeline");

/// An unfolded night lays this much of the night colour over the panel (`.nightbg`).
const OPEN_NIGHT: f32 = 0.6;
/// Text in a column starts this far right of its colour bar (`.ev .txt{left:3px;padding:3px}`).
const TEXT_SIDE: f32 = 3.0;
/// Text in a column keeps this far from its top and bottom end (`padding:8px 3px`).
const TEXT_END: f32 = space::S;
/// From this width on a column has room for its time beside the title.
const TWO_LINES: f32 = 40.0;
/// Inner margin of the day labels and of "Collapse night" (`.daylabel{padding:0 8px}`).
const LABEL_PAD: f32 = space::S;
/// Gap between the parts of a label: weekday, number, "Today"; moon and text.
const LABEL_GAP: f32 = 6.0;
/// Inner margin of "Collapse night" (`.fold{padding:0 6px}`).
const FOLD_PAD: f32 = 6.0;
/// Hour labels stand this far right of their line (`.hl{left:p+4}`).
const HOUR_LABEL_GAP: f32 = space::XS;
/// Radius of the dot of an event in a folded night, and of the ring of night colour around it.
const DOT: f32 = 3.0;
const DOT_RING: f32 = 5.0;
/// Pixels the wheel moves per line, as Iced's scrollables do.
const WHEEL_LINE: f32 = 60.0;
/// Beyond the visible stretch, this many viewport widths on either side are drawn as well, so a
/// fast scroll never shows an empty edge before the next frame.
const DRAW_MARGIN: f32 = 1.0;

/// The timeline's window state, kept by the app across reloads, navigation and changes of size:
/// which nights are unfolded and where the scrollable stands. Nothing of it is stored.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// The evenings whose night is unfolded.
    open: BTreeSet<NaiveDate>,
    /// Where the scrollable stood at its last report; `None` until the first one after it was
    /// (re)built.
    viewport: Option<Viewport>,
    /// The moment the last [`State::show`] asked for, until the scrollable has settled on it: its
    /// first report (the width decides where 30 % is) and a report with another width (a resize
    /// that came after the scroll) scroll to it again; a report that only moved the offset, or the
    /// wheel, ends it.
    focus: Option<NaiveDateTime>,
}

/// The visible stretch of the track: its left edge and its width.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Viewport {
    offset: f32,
    width: f32,
}

/// What happened to the timeline's window state; the app hands it to [`State::update`] as
/// `Message::Timeline`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    /// The scrollable reports where it stands: after every scroll and every change of its size,
    /// and once when it appears.
    Scrolled { offset: f32, width: f32 },
    /// The wheel turned over the timeline: scroll sideways by this many pixels (right is positive).
    Wheel(f32),
    /// A click on a folded night or on "Collapse night": unfold or fold the night after this evening.
    ToggleNight(NaiveDate),
    /// "Nights" in the header: unfold every night of the shown days, or fold them all when one of
    /// them is open.
    ToggleNights,
}

impl State {
    /// Applies `change` for the timeline of the days `[first, after)` (the app's range). Returns
    /// the scroll a `Scrolled` may still owe ([`State::show`]) or the one a `Wheel` asks for.
    pub fn update<T>(&mut self, change: Change, first: NaiveDate, after: NaiveDate) -> Task<T> {
        match change {
            Change::Scrolled { offset, width } => {
                let before = self.viewport.replace(Viewport { offset, width });
                match (self.focus, before) {
                    (Some(focus), None) => self.scroll_to(focus, width, first, after),
                    (Some(focus), Some(before)) if before.width != width => self.scroll_to(focus, width, first, after),
                    (Some(_), Some(_)) => {
                        self.focus = None;
                        Task::none()
                    }
                    (None, _) => Task::none(),
                }
            }
            Change::Wheel(dx) => {
                self.focus = None;
                operation::scroll_by(SCROLL_ID, AbsoluteOffset { x: dx, y: 0.0 })
            }
            Change::ToggleNight(evening) => {
                if !self.open.remove(&evening) {
                    self.open.insert(evening);
                }
                Task::none()
            }
            Change::ToggleNights => {
                let evenings: Vec<NaiveDate> = first.iter_days().take_while(|day| *day < after).collect();
                if evenings.iter().any(|evening| self.open.contains(evening)) {
                    evenings.iter().for_each(|evening| {
                        self.open.remove(evening);
                    });
                } else {
                    self.open.extend(evenings);
                }
                Task::none()
            }
        }
    }

    /// Scrolls the timeline of the days `[first, after)` so that `focus` stands at 30 % of its
    /// width ("Today", the start, ‹ ›, a new layout). At once when the scrollable has reported its
    /// width, and again should its next report bring another width (the window was resized and the
    /// report of the new size comes after this call); otherwise on its first report
    /// (`Change::Scrolled`), so the app may call this before the timeline is on screen.
    pub fn show<T>(&mut self, focus: NaiveDateTime, first: NaiveDate, after: NaiveDate) -> Task<T> {
        self.focus = Some(focus);
        match self.viewport {
            Some(viewport) => self.scroll_to(focus, viewport.width, first, after),
            None => Task::none(),
        }
    }

    /// Forgets where the scrollable stands. The app calls it when the timeline is about to be
    /// built anew (another view before it), so that the next [`State::show`] waits for the new
    /// scrollable's width instead of trusting the old one.
    pub fn forget_viewport(&mut self) {
        self.viewport = None;
    }

    fn scroll_to<T>(&self, focus: NaiveDateTime, width: f32, first: NaiveDate, after: NaiveDate) -> Task<T> {
        let x = Track::new(first, after, &self.open).focus_offset(focus, width);
        operation::scroll_to(SCROLL_ID, AbsoluteOffset { x: Some(x), y: None })
    }
}

/// The timeline of the days `[data.start, data.end)` in a sideways scrollable, filling the space
/// it gets.
pub fn view<'a>(data: ViewData<'a>, state: &State) -> Element<'a, Message> {
    let plan = Plan::new(&data, state);
    let width = plan.track.width;
    let canvas = iced::widget::canvas(Timeline { plan }).width(Length::Fixed(width)).height(Length::Fill);
    scrollable(canvas)
        .id(SCROLL_ID)
        .direction(Direction::Horizontal(style::scrollbar()))
        .style(style::scrollable)
        .on_scroll(|viewport| {
            Message::Timeline(Change::Scrolled { offset: viewport.absolute_offset().x, width: viewport.bounds().width })
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// One item of the view data. Entry ids are positions in [`Plan::entries`].
#[derive(Debug, Clone)]
struct Entry {
    /// Index into the app's event list, for `OpenEvent`.
    index: usize,
    color: Color,
    start: NaiveDateTime,
    end: NaiveDateTime,
    title: String,
}

/// Everything the canvas draws, built once per view.
#[derive(Debug, Clone)]
struct Plan {
    track: Track,
    entries: Vec<Entry>,
    timed: Vec<Timed>,
    spans: Vec<Span>,
    now: NaiveDateTime,
    /// The app's index of the event open in the side panel.
    selected: Option<usize>,
    viewport: Option<Viewport>,
}

impl Plan {
    fn new(data: &ViewData<'_>, state: &State) -> Plan {
        let mut plan = Plan {
            track: Track::new(data.start, data.end, &state.open),
            entries: Vec::new(),
            timed: Vec::new(),
            spans: Vec::new(),
            now: data.now,
            selected: data.selected,
            viewport: state.viewport,
        };
        for item in &data.items {
            let (start, end, all_day) = week::local_span(item.event);
            let id = plan.entries.len();
            plan.entries.push(Entry {
                index: item.index,
                color: item.color,
                start,
                end,
                title: week::title(item.event),
            });
            if all_day {
                plan.spans.push(Span::of(id, start, end));
            } else {
                plan.timed.push(Timed { id, start, end });
            }
        }
        plan
    }

    fn rows(&self, height: f32) -> Rows {
        Rows { height, chip_rows: layout::chip_rows(&self.spans) }
    }

    fn is_selected(&self, id: usize) -> bool {
        self.selected.is_some_and(|index| self.entries.get(id).is_some_and(|entry| entry.index == index))
    }

    /// The "Collapse night" buttons of the unfolded nights.
    fn folds(&self, rows: &Rows) -> Vec<(NaiveDate, Rectangle)> {
        let width = fold_width();
        self.track
            .segments
            .iter()
            .filter(|segment| !segment.folded())
            .filter_map(|segment| segment.evening().map(|evening| (evening, layout::fold_button(segment, rows, width))))
            .collect()
    }

    /// What a click at `point` on a canvas of `size` hits.
    fn hit(&self, size: Size, point: Point) -> Option<Hit> {
        let rows = self.rows(size.height);
        let blocks = layout::blocks(&self.track, &rows, &self.timed);
        let chips = layout::chips(&self.track, &rows, &self.spans);
        layout::hit(&self.track, &rows, &blocks, &chips, &self.folds(&rows), point)
    }

    /// The message of a click on `hit`.
    fn message(&self, hit: Hit) -> Option<Message> {
        Some(match hit {
            Hit::Event(id) | Hit::Chip(id) => Message::OpenEvent(self.entries.get(id)?.index),
            Hit::Unfold(evening) | Hit::Fold(evening) => Message::Timeline(Change::ToggleNight(evening)),
            Hit::Slot(at) => Message::NewEvent { date: at.date(), time: Some(at.time()) },
            Hit::AllDay(date) => Message::NewEvent { date, time: None },
        })
    }

    /// The stretch of x worth drawing: the visible one and a margin, or all before the first
    /// report of the scrollable.
    fn drawn(&self) -> (f32, f32) {
        match self.viewport {
            Some(Viewport { offset, width }) => (offset - DRAW_MARGIN * width, offset + (1.0 + DRAW_MARGIN) * width),
            None => (f32::NEG_INFINITY, f32::INFINITY),
        }
    }
}

/// The width of "Collapse night": moon, gap, label, and the inner margin.
fn fold_width() -> f32 {
    2.0 * FOLD_PAD + icon::SIZE_SM + space::XS + text_width(tr("Collapse night"), typo::CAPTION)
}

/// How far the wheel scrolls sideways: its turn up or down as left or right (down is right), in
/// pixels. `None` for a mostly sideways turn (a touchpad), which the scrollable takes itself.
fn sideways(delta: mouse::ScrollDelta) -> Option<f32> {
    let (x, y) = match delta {
        mouse::ScrollDelta::Lines { x, y } => (x * WHEEL_LINE, y * WHEEL_LINE),
        mouse::ScrollDelta::Pixels { x, y } => (x, y),
    };
    (y.abs() > x.abs()).then_some(-y)
}

/// The track with everything on it.
struct Timeline {
    plan: Plan,
}

impl canvas::Program<Message> for Timeline {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
            cursor.position_in(bounds)?;
            let dx = sideways(*delta)?;
            return Some(Action::publish(Message::Timeline(Change::Wheel(dx))).and_capture());
        }
        let point = week::left_press(event, bounds, cursor)?;
        let message = self.plan.message(self.plan.hit(bounds.size(), point)?)?;
        Some(Action::publish(message).and_capture())
    }

    fn mouse_interaction(&self, _state: &(), bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        let over = cursor.position_in(bounds).and_then(|point| self.plan.hit(bounds.size(), point));
        // Free time stays calm although a click there starts an event, as in the week.
        week::pointer_if(matches!(over, Some(Hit::Event(_) | Hit::Chip(_) | Hit::Unfold(_) | Hit::Fold(_))))
    }

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
        let plan = &self.plan;
        let rows = plan.rows(bounds.height);
        let (from, to) = plan.drawn();
        let seen = |x: f32, width: f32| x + width >= from && x <= to;
        let height = bounds.height;
        let now_x = (plan.track.start <= plan.now && plan.now < plan.track.end).then(|| plan.track.x_of(plan.now));

        let left = from.max(0.0);
        week::fill(
            &mut frame,
            Rectangle::new(Point::new(left, 0.0), Size::new(to.min(bounds.width) - left, height)),
            p.panel,
        );
        let ground = mix(p.panel, p.night, OPEN_NIGHT);
        for night in plan.track.segments.iter().filter(|s| s.evening().is_some() && !s.folded()) {
            if seen(night.x, night.width) {
                let top = rows.axis().1;
                week::fill(
                    &mut frame,
                    Rectangle::new(Point::new(night.x, top), Size::new(night.width, height - top)),
                    ground,
                );
            }
        }
        hours(&mut frame, p, plan, &rows, now_x, &seen);
        for x in plan.track.midnights().into_iter().filter(|x| seen(*x, 0.0)) {
            week::line(&mut frame, Point::new(x, 0.0), Point::new(x, height), p.line_strong);
        }
        for night in plan.track.segments.iter().filter(|s| s.folded() && seen(s.x, s.width)) {
            folded_night(&mut frame, p, plan, &rows, night);
        }
        free_time(&mut frame, p, plan, &rows, &seen);
        chips(&mut frame, p, plan, &rows, &seen);
        columns(&mut frame, p, plan, &rows, &seen);
        for (_, rect) in plan.folds(&rows).into_iter().filter(|(_, rect)| seen(rect.x, rect.width)) {
            fold_button(&mut frame, p, rect);
        }
        if plan.entries.is_empty() {
            let center = match plan.viewport {
                Some(Viewport { offset, width }) => offset + width / 2.0,
                None => bounds.width / 2.0,
            };
            let (top, bottom) = rows.all_day();
            let at = Point::new(center, (top + bottom) / 2.0);
            week::label(&mut frame, tr("No events"), at, typo::META, p.text_3, Alignment::Center, Vertical::Center);
        }
        day_labels(&mut frame, p, plan, &seen);
        if let Some(x) = now_x {
            now_line(&mut frame, p, &rows, x, height, plan.now);
        }
        vec![frame.into_geometry()]
    }
}

/// Hour lines from the axis down, and their labels "09:00" in the axis, in the days and the
/// unfolded nights. No label right under the now label.
fn hours(
    frame: &mut Frame,
    p: &Palette,
    plan: &Plan,
    rows: &Rows,
    now_x: Option<f32>,
    seen: &dyn Fn(f32, f32) -> bool,
) {
    let (top, bottom) = rows.axis();
    let label_width = text_width("00:00", typo::MICRO);
    for (hour, x) in plan.track.hours().into_iter().filter(|(_, x)| seen(*x, label_width)) {
        week::line(frame, Point::new(x, bottom), Point::new(x, rows.height), p.line);
        let start = x + HOUR_LABEL_GAP;
        let covered = now_x.is_some_and(|now| {
            let half = NOW_LABEL_WIDTH / 2.0;
            start < now + half && now - half < start + label_width
        });
        if !covered {
            let at = Point::new(start, (top + bottom) / 2.0);
            let text = format!("{:02}:00", hour.hour());
            week::label(frame, &text, at, typo::MICRO, p.text_3, Alignment::Left, Vertical::Center);
        }
    }
}

/// A folded night: the band with zigzag edges in the night colour, a dot for each event in it,
/// and the moon with "Night · 8 h · 1 event" turned to read upwards in its middle.
fn folded_night(frame: &mut Frame, p: &Palette, plan: &Plan, rows: &Rows, night: &Segment) {
    let band =
        Rectangle::new(Point::new(night.x, layout::DAY_HEAD), Size::new(night.width, rows.height - layout::DAY_HEAD));
    let outline = Path::new(|path| {
        let mut points = layout::zigzag(band).into_iter();
        if let Some(first) = points.next() {
            path.move_to(first);
            points.for_each(|point| path.line_to(point));
            path.close();
        }
    });
    frame.fill(&outline, p.night);

    let evening = night.evening();
    let dots: Vec<layout::Dot> =
        layout::dots(&plan.track, rows, &plan.timed).into_iter().filter(|dot| Some(dot.evening) == evening).collect();
    for dot in &dots {
        let Some(entry) = plan.entries.get(dot.id) else { continue };
        frame.fill(&Path::circle(dot.center, DOT_RING), p.night);
        frame.fill(&Path::circle(dot.center, DOT), entry.color);
    }

    let mut text = trf("Night · {n} h", &[("n", &night.hours())]);
    match dots.len() {
        0 => {}
        1 => text = format!("{text} · {}", tr("1 event")),
        n => text = format!("{text} · {}", trf("{n} events", &[("n", &n)])),
    }
    let length = icon::SIZE_SM + LABEL_GAP + text_width(&text, typo::META);
    let center = band.center();
    let bottom = (center.y + length / 2.0).round();
    let moon = Rectangle::new(
        Point::new(center.x - icon::SIZE_SM / 2.0, bottom - icon::SIZE_SM),
        Size::new(icon::SIZE_SM, icon::SIZE_SM),
    );
    frame.draw_svg(moon, &Icon::Moon.tinted(p.text_3));
    let line = week::line_height(typo::META);
    let at = Point::new(center.x - line / 2.0, bottom - icon::SIZE_SM - LABEL_GAP);
    upright(frame, &text, at, typo::META, p.text_3);
}

/// "3 h free" under the gaps of two hours or more between the events of a day, from now on.
fn free_time(frame: &mut Frame, p: &Palette, plan: &Plan, rows: &Rows, seen: &dyn Fn(f32, f32) -> bool) {
    let (top, bottom) = rows.free();
    for (start, end) in layout::free_gaps(&plan.track, &plan.timed, plan.now, FREE_MINUTES) {
        let (x0, x1) = (plan.track.x_of(start), plan.track.x_of(end));
        if !seen(x0, x1 - x0) {
            continue;
        }
        let hours = (end - start).num_hours();
        let at = Point::new((x0 + x1) / 2.0, (top + bottom) / 2.0);
        let note = trf("{n} h free", &[("n", &hours)]);
        week::label(frame, &note, at, typo::META, p.text_3, Alignment::Center, Vertical::Center);
    }
}

/// The all-day chips: tinted with the bar, the title in one line.
fn chips(frame: &mut Frame, p: &Palette, plan: &Plan, rows: &Rows, seen: &dyn Fn(f32, f32) -> bool) {
    let chips = layout::chips(&plan.track, rows, &plan.spans);
    for chip in chips.iter().filter(|chip| seen(chip.rect.x, chip.rect.width)) {
        let Some(entry) = plan.entries.get(chip.id) else { continue };
        let tone = Tone::of(p, entry.color, p.panel, entry.end <= plan.now);
        week::surface(frame, chip.rect, &tone);
        let room = chip.rect.width - 2.0 * LABEL_PAD - BAR;
        let title = ellipsize(&entry.title, typo::CAPTION, room);
        let at = Point::new(chip.rect.x + BAR + LABEL_PAD, chip.rect.center_y());
        frame.with_clip(chip.rect, |frame| {
            week::label(frame, &title, at, typo::CAPTION, tone.title, Alignment::Left, Vertical::Center);
        });
    }
    selection(frame, p, plan, &chips);
}

/// The events as columns: tinted with the bar, title and time turned to read upwards.
fn columns(frame: &mut Frame, p: &Palette, plan: &Plan, rows: &Rows, seen: &dyn Fn(f32, f32) -> bool) {
    let blocks: Vec<Block> = layout::blocks(&plan.track, rows, &plan.timed)
        .into_iter()
        .filter(|block| seen(block.rect.x, block.rect.width))
        .collect();
    for block in &blocks {
        let Some(entry) = plan.entries.get(block.id) else { continue };
        let tone = Tone::of(p, entry.color, p.panel, entry.end <= plan.now);
        week::surface(frame, block.rect, &tone);
        column_text(frame, block.rect, entry, &tone);
    }
    selection(frame, p, plan, &blocks);
}

/// The focus ring around the event open in the side panel, over its neighbours.
fn selection(frame: &mut Frame, p: &Palette, plan: &Plan, blocks: &[Block]) {
    for block in blocks.iter().filter(|block| plan.is_selected(block.id)) {
        week::selection_ring(frame, block.rect, p);
    }
}

/// Title (`CAPTION`) and, from [`TWO_LINES`] on, the time (`MICRO`) of a column, turned −90° to
/// read from the bottom up, starting [`TEXT_END`] above its bottom and cut with "…" to its height.
/// A column too narrow for both gets the title alone, in its middle.
///
/// Not clipped: Iced 0.14 draws shapes inside `Frame::with_clip` wrongly once the canvas is
/// scrolled sideways (stray triangles, missing shapes), and turned text is shapes. The cut to the
/// height and the choice of lines by width keep the text inside the column instead.
fn column_text(frame: &mut Frame, rect: Rectangle, entry: &Entry, tone: &Tone) {
    let room = rect.height - 2.0 * TEXT_END;
    if room <= 0.0 {
        return;
    }
    let line = week::line_height(typo::CAPTION);
    let two = rect.width >= TWO_LINES;
    let x = if two { rect.x + BAR + TEXT_SIDE } else { rect.x + BAR + ((rect.width - BAR - line) / 2.0).max(0.0) };
    let y = rect.y + rect.height - TEXT_END;
    let title = ellipsize(&entry.title, typo::CAPTION, room);
    upright(frame, &title, Point::new(x, y), typo::CAPTION, tone.title);
    if two {
        let time = time_text(entry, room);
        upright(frame, &time, Point::new(x + line, y), typo::MICRO, tone.time);
    }
}

/// "10:00 – 11:30", or the start alone where the range does not fit `room`.
fn time_text(entry: &Entry, room: f32) -> String {
    let range = format!("{} – {}", format_time(entry.start.time()), format_time(entry.end.time()));
    if entry.end > entry.start && text_width(&range, typo::MICRO) <= room {
        range
    } else {
        ellipsize(&format_time(entry.start.time()), typo::MICRO, room)
    }
}

/// One line of text turned −90°, reading upwards: `at` is where its first letter's top left lies
/// on screen, the line runs up from there and its letters stand to the right of `at.x`. Iced draws
/// turned text as outlines; the position is kept to whole pixels so they stay as sharp as they can.
fn upright(frame: &mut Frame, content: &str, at: Point, level: typo::Level, color: Color) {
    frame.with_save(|frame| {
        frame.translate(Vector::new(at.x.round(), at.y.round()));
        frame.rotate(-FRAC_PI_2);
        week::label(frame, content, Point::ORIGIN, level, color, Alignment::Left, Vertical::Top);
    });
}

/// "Collapse night" at the top of an unfolded night: a small raised button with the moon.
fn fold_button(frame: &mut Frame, p: &Palette, rect: Rectangle) {
    let shape = Path::rounded_rectangle(rect.position(), rect.size(), Radius::from(radius::XS));
    frame.fill(&shape, p.raised);
    frame.stroke(&shape, Stroke::default().with_color(p.line_strong).with_width(1.0));
    let moon_top = rect.center_y() - icon::SIZE_SM / 2.0;
    let moon = Rectangle::new(Point::new(rect.x + FOLD_PAD, moon_top), Size::new(icon::SIZE_SM, icon::SIZE_SM));
    frame.draw_svg(moon, &Icon::Moon.tinted(p.text_3));
    let at = Point::new(moon.x + icon::SIZE_SM + space::XS, rect.center_y());
    week::label(frame, tr("Collapse night"), at, typo::CAPTION, p.text_2, Alignment::Left, Vertical::Center);
}

/// The day labels ("Tue 13", today "Wed (14) Today" with the number white in a red circle) on the
/// panel, each at its day's start or stuck to the left edge of the view while its day runs on.
fn day_labels(frame: &mut Frame, p: &Palette, plan: &Plan, seen: &dyn Fn(f32, f32) -> bool) {
    let offset = plan.viewport.map_or(0.0, |viewport| viewport.offset);
    let today = plan.now.date();
    let middle = layout::DAY_HEAD / 2.0;
    for (day, from, to) in plan.track.days().into_iter().filter(|(_, from, to)| seen(*from, to - from)) {
        let is_today = day == today;
        let weekday = weekday_name(day, true);
        let number = day.day().to_string();
        let number_box = DAY_CIRCLE.max(text_width(&number, DAY_NUMBER) + space::S);
        let mut width = 2.0 * LABEL_PAD + text_width(weekday, typo::META) + LABEL_GAP + number_box;
        if is_today {
            width += LABEL_GAP + text_width(tr("Today"), typo::META);
        }
        let x = layout::sticky_x(from, to, offset, width);
        week::fill(frame, Rectangle::new(Point::new(x, 0.0), Size::new(width.min(to - x), layout::DAY_HEAD)), p.panel);
        // Not clipped to the day (see `column_text`): `sticky_x` keeps the label inside it.
        let mut cursor = x + LABEL_PAD;
        let tone = if is_today { p.text } else { p.text_3 };
        week::label(frame, weekday, Point::new(cursor, middle), typo::META, tone, Alignment::Left, Vertical::Center);
        cursor += text_width(weekday, typo::META) + LABEL_GAP;
        let center = Point::new(cursor + number_box / 2.0, middle);
        let colour = if is_today {
            frame.fill(&Path::circle(center, DAY_CIRCLE / 2.0), p.accent);
            p.on_accent
        } else {
            p.text_2
        };
        week::label(frame, &number, center, DAY_NUMBER, colour, Alignment::Center, Vertical::Center);
        if is_today {
            let at = Point::new(cursor + number_box + LABEL_GAP, middle);
            week::label(frame, tr("Today"), at, typo::META, p.text_3, Alignment::Left, Vertical::Center);
        }
    }
}

/// Now: a 1-px red line from the axis down with a dot where it leaves the axis, and the time as a
/// red pill with white figures in the axis, as in the week.
fn now_line(frame: &mut Frame, p: &Palette, rows: &Rows, x: f32, height: f32, now: NaiveDateTime) {
    let (top, bottom) = rows.axis();
    week::line(frame, Point::new(x, bottom), Point::new(x, height), p.accent);
    frame.fill(&Path::circle(Point::new(x, bottom), NOW_DOT), p.accent);
    let pill_height = week::line_height(NOW_LABEL);
    let middle = (top + bottom) / 2.0;
    let pill = Rectangle::new(
        Point::new(x - NOW_LABEL_WIDTH / 2.0, middle - pill_height / 2.0),
        Size::new(NOW_LABEL_WIDTH, pill_height),
    );
    frame.fill(&Path::rounded_rectangle(pill.position(), pill.size(), Radius::from(radius::XS)), p.accent);
    let time = format_time(now.time());
    week::label(frame, &time, Point::new(x, middle), NOW_LABEL, p.on_accent, Alignment::Center, Vertical::Center);
}

#[cfg(test)]
mod tests {
    use iced::widget::canvas::Program;
    use torro_pim::{Event as CalendarEvent, EventTime};

    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("date")
    }

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        d(day).and_hms_opt(hour, minute, 0).expect("time")
    }

    fn local(day: u32, hour: u32, minute: u32) -> EventTime {
        EventTime::DateTime(at(day, hour, minute).and_local_timezone(chrono::Local).single().expect("local").to_utc())
    }

    fn calendar_event(summary: &str, start: EventTime, end: Option<EventTime>) -> CalendarEvent {
        CalendarEvent {
            uid: summary.into(),
            collection: String::new(),
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

    /// Mon 12 to Wed 14 October 2026, now Tue 13 14:20: a workshop on Monday, a release in
    /// Tuesday's night and a holiday on Wednesday.
    fn events() -> Vec<CalendarEvent> {
        vec![
            calendar_event("Workshop", local(12, 10, 0), Some(local(12, 11, 0))),
            calendar_event("Release call", local(13, 22, 30), Some(local(14, 0, 30))),
            calendar_event("Holiday", EventTime::Date(d(14)), Some(EventTime::Date(d(15)))),
        ]
    }

    fn data(events: &[CalendarEvent]) -> ViewData<'_> {
        let items = events
            .iter()
            .enumerate()
            .map(|(index, event)| super::super::Item { index: index + 10, event, color: Color::BLACK })
            .collect();
        ViewData { items, start: d(12), end: d(15), anchor: d(13), now: at(13, 14, 20), selected: None }
    }

    const SIZE: Size = Size::new(4000.0, 600.0);

    #[test]
    fn timed_events_become_columns_and_all_day_ones_chips() {
        let events = events();
        let plan = Plan::new(&data(&events), &State::default());
        assert_eq!(plan.timed.iter().map(|t| t.id).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(plan.spans, [Span { id: 2, first: d(14), after: d(15) }]);
        assert_eq!(plan.entries[2].index, 12, "the app's index for OpenEvent");
    }

    #[test]
    fn clicks_open_events_unfold_nights_and_start_new_events() {
        let events = events();
        let plan = Plan::new(&data(&events), &State::default());
        let rows = plan.rows(SIZE.height);
        let workshop = layout::blocks(&plan.track, &rows, &plan.timed)[0].rect;
        let message = |point: Point| plan.hit(SIZE, point).and_then(|hit| plan.message(hit));
        assert!(matches!(message(workshop.center()), Some(Message::OpenEvent(10))));
        let night = plan.track.segments[3];
        assert!(
            matches!(message(Point::new(night.x + 5.0, 300.0)), Some(Message::Timeline(Change::ToggleNight(e))) if e == d(13))
        );
        let free = Point::new(plan.track.x_of(at(13, 16, 50)), 300.0);
        let expected = (d(13), at(13, 16, 45).time());
        assert!(
            matches!(message(free), Some(Message::NewEvent { date, time: Some(time) }) if (date, time) == expected)
        );
        let all_day = Point::new(plan.track.x_of(at(12, 15, 0)), rows.all_day().0 + 10.0);
        assert!(matches!(message(all_day), Some(Message::NewEvent { date, time: None }) if date == d(12)));
    }

    /// What the canvas shows with the cursor at `point`.
    fn interaction(timeline: &Timeline, point: Point) -> mouse::Interaction {
        let bounds = Rectangle::new(Point::new(30.0, 40.0), SIZE);
        let cursor = mouse::Cursor::Available(Point::new(bounds.x + point.x, bounds.y + point.y));
        timeline.mouse_interaction(&(), bounds, cursor)
    }

    #[test]
    fn the_hand_shows_over_what_a_click_opens_or_unfolds() {
        let events = events();
        let timeline = Timeline { plan: Plan::new(&data(&events), &State::default()) };
        let rows = timeline.plan.rows(SIZE.height);
        let workshop = layout::blocks(&timeline.plan.track, &rows, &timeline.plan.timed)[0].rect;
        assert_eq!(interaction(&timeline, workshop.center()), mouse::Interaction::Pointer);
        let night = timeline.plan.track.segments[1];
        assert_eq!(interaction(&timeline, Point::new(night.x + 5.0, 300.0)), mouse::Interaction::Pointer);
        let free = Point::new(timeline.plan.track.x_of(at(13, 16, 0)), 300.0);
        assert_eq!(interaction(&timeline, free), mouse::Interaction::Idle, "free time stays calm");
    }

    #[test]
    fn the_wheel_scrolls_sideways_and_leaves_sideways_turns_to_the_scrollable() {
        assert_eq!(sideways(mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 }), Some(WHEEL_LINE), "down is right");
        assert_eq!(sideways(mouse::ScrollDelta::Pixels { x: 0.0, y: 30.0 }), Some(-30.0));
        assert_eq!(sideways(mouse::ScrollDelta::Pixels { x: 40.0, y: 5.0 }), None);
        let events = events();
        let timeline = Timeline { plan: Plan::new(&data(&events), &State::default()) };
        let bounds = Rectangle::new(Point::ORIGIN, SIZE);
        let wheel = Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 } });
        let over = mouse::Cursor::Available(Point::new(100.0, 100.0));
        assert!(timeline.update(&mut (), &wheel, bounds, over).is_some(), "taken over the timeline");
        let outside = mouse::Cursor::Available(Point::new(100.0, 900.0));
        assert!(timeline.update(&mut (), &wheel, bounds, outside).is_none());
    }

    #[test]
    fn a_scroll_to_now_waits_for_the_width_of_the_scrollable() {
        let mut state = State::default();
        let (first, after) = (d(12), d(20));
        let report = |offset: f32, width: f32| Change::Scrolled { offset, width };
        assert_eq!(state.show::<()>(at(13, 14, 20), first, after).units(), 0, "nothing to scroll yet");
        assert!(state.update::<()>(report(0.0, 900.0), first, after).units() > 0, "the first report scrolls");
        assert_eq!(state.focus, Some(at(13, 14, 20)), "until the scrollable got there");
        assert_eq!(state.update::<()>(report(1140.0, 900.0), first, after).units(), 0, "there: settled");
        assert_eq!(state.focus, None);
        assert_eq!(
            state.update::<()>(report(1140.0, 600.0), first, after).units(),
            0,
            "a resize alone keeps the offset"
        );
        assert!(state.show::<()>(at(14, 9, 0), first, after).units() > 0, "with a known width at once");
        assert!(state.update::<()>(report(1500.0, 800.0), first, after).units() > 0, "a later new width scrolls again");
        assert!(state.update::<()>(Change::Wheel(60.0), first, after).units() > 0);
        assert_eq!(state.focus, None, "the wheel takes over");
        state.forget_viewport();
        assert_eq!(state.show::<()>(at(14, 9, 0), first, after).units(), 0, "a new scrollable is waited for");
    }

    #[test]
    fn nights_unfold_one_by_one_or_all_together() {
        let mut state = State::default();
        let (first, after) = (d(12), d(15));
        let _ = state.update::<()>(Change::ToggleNight(d(13)), first, after);
        assert_eq!(state.open, BTreeSet::from([d(13)]));
        let _ = state.update::<()>(Change::ToggleNights, first, after);
        assert_eq!(state.open, BTreeSet::new(), "one open: all fold");
        let _ = state.update::<()>(Change::ToggleNights, first, after);
        assert_eq!(state.open, BTreeSet::from([d(12), d(13), d(14)]), "none open: all unfold");
        let _ = state.update::<()>(Change::ToggleNight(d(13)), first, after);
        assert!(!state.open.contains(&d(13)));
        let plan = Plan::new(&data(&events()), &state);
        assert!(plan.track.width > Plan::new(&data(&events()), &State::default()).track.width);
        assert_eq!(plan.folds(&plan.rows(SIZE.height)).len(), 2, "a collapse button per open night");
    }

    #[test]
    fn time_text_shows_the_range_where_it_fits() {
        let entry =
            Entry { index: 0, color: Color::BLACK, start: at(13, 10, 0), end: at(13, 11, 30), title: "x".into() };
        assert_eq!(time_text(&entry, 200.0), "10:00 – 11:30");
        assert_eq!(time_text(&entry, 40.0), "10:00");
    }

    #[test]
    fn the_view_builds_with_and_without_events() {
        let events = events();
        let _ = view(data(&events), &State::default());
        let _ = view(data(&[]), &State::default());
    }
}
