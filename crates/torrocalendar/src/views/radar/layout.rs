//! Geometry of the radar, free of Iced widgets so that it can be tested: where the window of a
//! day starts, time to angle and back within the window, how the last quarter fades out, the
//! ticks and numerals of the dial, the radii of the rings, the arcs of the events with their
//! lanes, the nights and the working hours in the window, hit-testing, turning the dial by dragging, where the letters
//! of a title go along an arc, where each ring names its calendar and where a tooltip goes.
//! `radar.rs` draws the result.
//!
//! Angles are in degrees, 0 at the top (where the 12 stands), growing clockwise, as on a clock.
//! Times inside the window are minutes after its start ("rel"); the window starts at the start of
//! the shown day ([`day_start`]: now for today) plus the turn of the dial, and covers twelve
//! hours, one turn of the 12-hour dial.

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use iced::{Point, Rectangle, Size};
use torro_core::settings::{Night, WorkingHours};
use torro_gui::theme::space;
use torro_gui::typo::{self, ellipsize, text_width};

use crate::views::glance;
use crate::views::timeline::moon_target_size;

/// The window: twelve hours, one turn of the dial (minutes).
pub const WINDOW: f32 = 720.0;
/// The first nine hours are drawn in full; the last quarter fades out (minutes).
pub const VISIBLE: f32 = 540.0;
/// The window ends this much before a full turn, so its faded end does not touch the clear edge
/// at its start (minutes).
pub const SEAM: f32 = 6.0;
/// Where the drawn window ends (minutes after its start).
pub const END: f32 = WINDOW - SEAM;
/// The fading quarter is drawn in pieces this long, each with the opacity of its middle: Iced has
/// no masks. Two minutes are one degree; at two degrees the steps showed as stripes in light mode.
pub const FADE_PIECE: f32 = 2.0;
/// Degrees per minute: twelve hours are a turn.
const DEGREES_PER_MINUTE: f32 = 360.0 / WINDOW;
/// One notch of the mouse wheel over the dial turns it this far (minutes).
pub const WHEEL_STEP: i64 = 15;
/// ↑ and ↓ turn the dial this far (minutes).
pub const HOUR_STEP: i64 = 60;
/// How far back the dial turns from the start of the day (minutes).
pub const LOOK_BACK: i64 = -12 * 60;
/// How far ahead the dial turns from the start of the day (minutes).
pub const LOOK_AHEAD: i64 = 24 * 60;
/// A day other than today opens at this hour…
const DAY_OPENS: u32 = 8;
/// A press on the dial turns it only once the pointer has moved this far (pixels); less is a click.
pub const DRAG_THRESHOLD: f32 = 4.0;
/// Dragging turns the dial in steps of this many minutes.
pub const DRAG_SNAP: f32 = 5.0;
/// No numeral this close to the start of the window, where the time chip sits (minutes, on both
/// sides of it).
const NUMERAL_CLEARANCE: f32 = 20.0;
/// Ticks every half hour.
const TICK_EVERY: f32 = 30.0;

/// From the outer edge of the dial inwards: the numerals sit on this radius (`R - 14`).
const NUMERAL_INSET: f32 = 14.0;
/// The ticks hang down from this radius (`R - 30`).
const TICK_INSET: f32 = 30.0;
/// The outermost ring starts here (`R - 40`).
const RING_INSET: f32 = 40.0;
/// A ring is at most this wide.
pub const RING_MAX: f32 = 64.0;
/// The dial is at most this wide: in a large window a bigger dial only grows the empty middle.
pub const MAX_DIAMETER: f32 = 720.0;
/// Between two rings.
pub const RING_GAP: f32 = 4.0;
/// The rings leave at least half the radius to the middle.
const INNER_SHARE: f32 = 0.5;
/// Lanes of a shared ring keep this much air between them.
const LANE_GAP: f32 = 2.0;
/// An event arc starts and ends this far inside its times, so neighbours stay apart (degrees).
const ARC_INSET: f32 = 0.4;
/// The shortest arc, so an event without length stays visible and clickable (degrees).
const ARC_MIN: f32 = 1.2;
/// A ring's name ends this far before the start edge (pixels along its middle)…
pub fn name_gap() -> f32 {
    space::s()
}
/// …and takes at most the last hour of the window, where the fade has left the ring nearly empty
/// (minutes).
pub const NAME_SPAN: f32 = 60.0;
/// Rings narrower than this show only the dot, no name.
pub const NAME_MIN_BAND: f32 = 14.0;
/// The radius of the dot before a ring's name, and the room between dot and name.
pub const NAME_DOT: f32 = 3.0;
fn name_dot_gap() -> f32 {
    space::xs()
}
/// A name cut shorter than this many letters is left out ("Pe…" says nothing; the tooltip has it).
const NAME_MIN_LETTERS: usize = 3;
/// A tooltip stands this far right of and below the pointer (or left of and above it at the edges).
pub fn tip_offset() -> f32 {
    space::m()
}

/// The turn of the dial, `minutes` from the start of the day, kept to [`LOOK_BACK`] …
/// [`LOOK_AHEAD`].
pub fn clamp_offset(minutes: i64) -> i64 {
    minutes.clamp(LOOK_BACK, LOOK_AHEAD)
}

/// Where the window of `day` starts before the dial is turned. Today: at `now`. Any other day: at
/// [`DAY_OPENS`] (08:00), or at the full hour of an earlier event that day, but never before
/// `night` ends that morning (06:00; without a night any hour will do). `starts` are the starts of
/// the timed events the caller knows (all-day ones left out); only those starting on `day` count,
/// not one begun the evening before.
pub fn day_start(
    day: NaiveDate,
    now: NaiveDateTime,
    starts: impl IntoIterator<Item = NaiveDateTime>,
    night: Night,
) -> NaiveDateTime {
    if day == now.date() {
        return now;
    }
    let first = starts.into_iter().filter(|start| start.date() == day).map(|start| start.hour()).min();
    let hour = first.map_or(DAY_OPENS, |hour| hour.min(DAY_OPENS));
    let midnight = day.and_time(NaiveTime::MIN);
    let opens = midnight + Duration::hours(i64::from(hour));
    if night.is_none() {
        return opens;
    }
    opens.max(midnight + Duration::minutes(i64::from(night.to.minutes())))
}

/// The signed turn from direction `from` to direction `to` (degrees, −180 … 180): the short way
/// round, so a drag over the 12 (from 359° to 1°) is two degrees, not −358.
pub fn turn_between(from: f32, to: f32) -> f32 {
    (to - from + 180.0).rem_euclid(360.0) - 180.0
}

/// The minutes a turn of `degrees` moves the window: 360° are twelve hours, clockwise is later.
pub fn minutes_of_turn(degrees: f32) -> f32 {
    degrees / DEGREES_PER_MINUTE
}

/// `minutes` of dragging cut to whole [`DRAG_SNAP`] steps: the minutes to turn now and the rest to
/// carry into the next move (with the same sign), so slow drags add up instead of getting lost.
pub fn snap(minutes: f32) -> (i64, f32) {
    let steps = (minutes / DRAG_SNAP).trunc();
    ((steps * DRAG_SNAP) as i64, minutes - steps * DRAG_SNAP)
}

/// A press at `origin` has become a drag at `point`: the pointer moved [`DRAG_THRESHOLD`] or more.
pub fn is_drag(origin: Point, point: Point) -> bool {
    (point.x - origin.x).hypot(point.y - origin.y) >= DRAG_THRESHOLD
}

/// The angle of the hand of a 12-hour clock at `at` (15:00 is at 90°, like 03:00).
pub fn clock_angle(at: NaiveDateTime) -> f32 {
    let minutes = at.time().num_seconds_from_midnight() as f32 / 60.0;
    minutes.rem_euclid(WINDOW) * DEGREES_PER_MINUTE
}

/// `1 − smoothstep(f)^0.8`: the opacity at `rel` minutes into the window. 1 for the first nine
/// hours, then falling over the last quarter to 0 at [`END`].
pub fn opacity(rel: f32) -> f32 {
    if rel <= VISIBLE {
        return 1.0;
    }
    let f = ((rel - VISIBLE) / (END - VISIBLE)).clamp(0.0, 1.0);
    let smooth = f * f * (3.0 - 2.0 * f);
    (1.0 - smooth.powf(0.8)).clamp(0.0, 1.0)
}

/// A stretch of the window drawn in one go, with one opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    /// Minutes after the start of the window.
    pub from: f32,
    pub to: f32,
    pub opacity: f32,
}

/// `[from, to)` cut into what can be drawn with one opacity each: the part before the fade as one
/// piece, the part in it in [`FADE_PIECE`]s on a fixed grid (so rings and events fade alike), each
/// with the opacity of its middle. Pieces that would be invisible are left out.
pub fn pieces(from: f32, to: f32) -> Vec<Piece> {
    let (from, to) = (from.max(0.0), to.min(END));
    let mut out = Vec::new();
    if to <= from {
        return out;
    }
    if from < VISIBLE {
        out.push(Piece { from, to: to.min(VISIBLE), opacity: 1.0 });
    }
    let mut start = VISIBLE + ((from.max(VISIBLE) - VISIBLE) / FADE_PIECE).floor() * FADE_PIECE;
    while start < to {
        let end = start + FADE_PIECE;
        let (a, b) = (start.max(from), end.min(to));
        let alpha = opacity((start + end) / 2.0);
        if b > a && alpha > 0.002 {
            out.push(Piece { from: a, to: b, opacity: alpha });
        }
        start = end;
    }
    out
}

/// The twelve hours from `start` on the dial.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    /// Where the window starts: the start of the day plus the turn of the dial.
    pub start: NaiveDateTime,
    /// The angle of `start` on the dial.
    pub angle0: f32,
}

impl Window {
    pub fn new(start: NaiveDateTime) -> Window {
        Window { start, angle0: clock_angle(start) }
    }

    /// Minutes from the start of the window to `at` (negative before it).
    pub fn rel(&self, at: NaiveDateTime) -> f32 {
        (at - self.start).num_seconds() as f32 / 60.0
    }

    /// The time `rel` minutes into the window.
    pub fn at(&self, rel: f32) -> NaiveDateTime {
        self.start + Duration::seconds((rel * 60.0).round() as i64)
    }

    /// The angle of `rel` minutes into the window. Not wrapped: it grows past 360 so that arcs
    /// over the 12 stay in one piece.
    pub fn angle(&self, rel: f32) -> f32 {
        self.angle0 + rel * DEGREES_PER_MINUTE
    }

    /// The inverse of [`Window::angle`] for unwrapped angles.
    pub fn rel_of(&self, angle: f32) -> f32 {
        (angle - self.angle0) / DEGREES_PER_MINUTE
    }

    /// The minutes into the window of a direction on the dial (any angle, wrapped to one turn).
    pub fn rel_at(&self, angle: f32) -> f32 {
        (angle - self.angle0).rem_euclid(360.0) / DEGREES_PER_MINUTE
    }

    /// Where the ticks go, every [`TICK_EVERY`] minutes of the clock: minutes into the window and
    /// whether it is a full hour (a long tick).
    pub fn ticks(&self) -> Vec<(f32, bool)> {
        let clock = self.start.time().num_seconds_from_midnight() as f32 / 60.0;
        let mut minute = (clock / TICK_EVERY).ceil() * TICK_EVERY;
        let mut out = Vec::new();
        while minute - clock <= END {
            out.push((minute - clock, (minute.round() as i64).rem_euclid(60) == 0));
            minute += TICK_EVERY;
        }
        out
    }

    /// The numerals: the real hour of the day at each full hour (`15` where the 3 stands), and
    /// whether it is one of 3, 6, 9 or 12 (drawn larger). None next to the start of the window,
    /// where the time chip stands.
    pub fn numerals(&self) -> Vec<Numeral> {
        self.ticks()
            .into_iter()
            .filter(|(rel, full)| *full && *rel >= NUMERAL_CLEARANCE && *rel <= WINDOW - NUMERAL_CLEARANCE)
            .map(|(rel, _)| {
                let hour = self.at(rel).hour();
                Numeral { rel, hour, major: hour.is_multiple_of(3) }
            })
            .collect()
    }
}

/// A numeral of the dial.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Numeral {
    /// Minutes into the window.
    pub rel: f32,
    /// The hour of the day, 0–23.
    pub hour: u32,
    /// 3, 6, 9 or 12 (and 15, 18 …): 13/500 in `text`; the others 11/400 in `text_2`.
    pub major: bool,
}

/// The radii of the dial in a square canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dial {
    pub center: Point,
    /// The outer radius.
    pub radius: f32,
    /// Where the numerals sit.
    pub numerals: f32,
    /// Where the ticks start (they hang inwards).
    pub ticks: f32,
    /// The outer edge of the outermost ring.
    pub outer: f32,
    /// The inner edge of the innermost ring: the middle with the time lies inside it.
    pub inner: f32,
    /// The width of each ring.
    pub band: f32,
    /// How many rings.
    pub rings: usize,
}

impl Dial {
    /// The dial centred in `bounds`, `margin` from its shorter side, with `rings` rings of equal
    /// width (at most [`RING_MAX`], [`RING_GAP`] apart, the first outside). The rings leave at
    /// least half the radius to the middle.
    pub fn fit(bounds: Size, margin: f32, rings: usize) -> Dial {
        let radius = ((bounds.width.min(bounds.height) - 2.0 * margin) / 2.0).clamp(0.0, MAX_DIAMETER / 2.0);
        let outer = (radius - RING_INSET).max(0.0);
        let inner = (radius * INNER_SHARE).max(outer - rings as f32 * RING_MAX).min(outer);
        let band =
            if rings == 0 { 0.0 } else { ((outer - inner - RING_GAP * (rings as f32 - 1.0)) / rings as f32).max(0.0) };
        Dial {
            center: Point::new(bounds.width / 2.0, bounds.height / 2.0),
            radius,
            numerals: radius - NUMERAL_INSET,
            ticks: radius - TICK_INSET,
            outer,
            inner,
            band,
            rings,
        }
    }

    /// Inner and outer radius of ring `ring` (0 is the outermost).
    pub fn ring(&self, ring: usize) -> (f32, f32) {
        let outer = self.outer - ring as f32 * (self.band + RING_GAP);
        (outer - self.band, outer)
    }

    /// The point at `radius` in the direction `angle`.
    pub fn point(&self, radius: f32, angle: f32) -> Point {
        let a = angle.to_radians();
        Point::new(self.center.x + radius * a.sin(), self.center.y - radius * a.cos())
    }

    /// Radius and direction (0–360) of `point`.
    pub fn polar(&self, point: Point) -> (f32, f32) {
        let (dx, dy) = (point.x - self.center.x, point.y - self.center.y);
        (dx.hypot(dy), dx.atan2(-dy).to_degrees().rem_euclid(360.0))
    }

    /// A press at `point` can turn the dial: it lies on the dial, from the outer half of the
    /// middle out to the numerals. Near the centre a small move swings the direction wildly.
    pub fn grabbable(&self, point: Point) -> bool {
        let (radius, _) = self.polar(point);
        (self.radius * INNER_SHARE..=self.radius).contains(&radius)
    }
}

/// A timed event in the window, in its calendar's ring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    /// The caller's id of the entry.
    pub id: usize,
    /// The ring of its calendar.
    pub ring: usize,
    /// Minutes into the window, cut to `[0, END]`.
    pub from: f32,
    pub to: f32,
    /// It starts inside the window, so its colour bar is drawn (one begun earlier has none).
    pub starts: bool,
}

/// An event as drawn: a piece of its ring between two radii and two (unwrapped) angles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    pub id: usize,
    pub inner: f32,
    pub outer: f32,
    pub from: f32,
    pub to: f32,
    pub starts: bool,
}

impl Arc {
    /// The width of its lane.
    pub fn width(&self) -> f32 {
        self.outer - self.inner
    }

    /// The radius in the middle of its lane.
    pub fn middle(&self) -> f32 {
        (self.inner + self.outer) / 2.0
    }

    /// Its length along the middle of its lane, in pixels.
    pub fn length(&self) -> f32 {
        (self.to - self.from).to_radians() * self.middle()
    }

    /// The bottom half of the dial reads upside down along the arc: there the text runs the other
    /// way round and its letters turn over, so it stands upright.
    pub fn flipped(&self) -> bool {
        let middle = ((self.from + self.to) / 2.0).rem_euclid(360.0);
        middle > 90.0 && middle < 270.0
    }
}

/// Places the spans: each ring is split across its width into lanes for its overlapping events
/// (the first lane outside), lanes of a shared ring keep [`LANE_GAP`] apart; arcs start and end
/// [`ARC_INSET`] inside their times and are at least [`ARC_MIN`] long.
pub fn arcs(dial: &Dial, window: &Window, spans: &[Span]) -> Vec<Arc> {
    let mut out = Vec::with_capacity(spans.len());
    for ring in 0..dial.rings {
        let members: Vec<&Span> = spans.iter().filter(|s| s.ring == ring).collect();
        let intervals: Vec<(f32, f32)> = members.iter().map(|s| (s.from, s.to)).collect();
        let (_, ring_outer) = dial.ring(ring);
        for (span, (lane, lanes)) in members.iter().zip(glance::lanes(&intervals)) {
            let lane_width = dial.band / lanes as f32;
            let outer = ring_outer - lane as f32 * lane_width;
            let inner = outer - lane_width + if lanes > 1 { LANE_GAP } else { 0.0 };
            let from = window.angle(span.from) + ARC_INSET;
            let to = (window.angle(span.to) - ARC_INSET).max(from + ARC_MIN);
            out.push(Arc { id: span.id, inner, outer, from, to, starts: span.starts });
        }
    }
    out
}

/// What a click on the dial means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// The arc of the span with this id.
    Event(usize),
    /// Free room in the rings, this many minutes into the window.
    Free(f32),
}

/// Hit-tests `point`: an arc wins (the last drawn on top), else free room between the inner and
/// the outer edge of the rings inside the window. Nothing in the middle, on the numerals or in
/// the seam at the end of the window.
pub fn hit(dial: &Dial, window: &Window, arcs: &[Arc], point: Point) -> Option<Hit> {
    let (radius, angle) = dial.polar(point);
    let rel = window.rel_at(angle);
    if rel > END {
        return None;
    }
    let over = |arc: &&Arc| {
        let (from, to) = (window.rel_of(arc.from), window.rel_of(arc.to));
        (arc.inner..=arc.outer).contains(&radius) && (from..=to).contains(&rel)
    };
    if let Some(arc) = arcs.iter().rev().find(over) {
        return Some(Hit::Event(arc.id));
    }
    (dial.rings > 0 && (dial.inner..=dial.outer).contains(&radius)).then_some(Hit::Free(rel))
}

/// The ring (0 the outermost) whose track lies at `radius` from the centre; `None` in the gap
/// between two rings, inside the innermost and outside the outermost.
pub fn ring_at(dial: &Dial, radius: f32) -> Option<usize> {
    (0..dial.rings).find(|&ring| {
        let (inner, outer) = dial.ring(ring);
        (inner..=outer).contains(&radius)
    })
}

/// Where a ring names its calendar: a dot in the calendar's colour and, where the ring is wide
/// enough, the name along the arc. Both lie in the last hour of the window ([`NAME_SPAN`]), where
/// the fade has left the ring nearly empty, ending [`name_gap()`] before the start edge, so the
/// name turns with the dial.
#[derive(Debug, Clone, PartialEq)]
pub struct RingName {
    /// The radius of the middle of the ring: dot and letters are centred on it.
    pub radius: f32,
    /// The direction of the dot's centre (degrees, unwrapped like the window's angles).
    pub dot: f32,
    /// The name, `None` when it does not fit: the ring is narrower than [`NAME_MIN_BAND`], or the
    /// arc has room for fewer than [`NAME_MIN_LETTERS`] of it.
    pub text: Option<NameText>,
}

/// A ring's name as [`glyphs`] lays it out.
#[derive(Debug, Clone, PartialEq)]
pub struct NameText {
    /// The name in `MICRO`, cut with "…" to the room there is.
    pub text: String,
    /// Where its first letter starts (degrees, unwrapped).
    pub start: f32,
    /// In the bottom half the name runs anticlockwise with its letters turned over, to stand
    /// upright; the dot then comes first on the side of the edge.
    pub flipped: bool,
}

/// The name `name` of ring `ring` on `dial` for `window` ([`RingName`]); `None` when the dial has
/// no such ring or no room. An empty name gets the dot alone.
pub fn ring_name(dial: &Dial, window: &Window, ring: usize, name: &str) -> Option<RingName> {
    if ring >= dial.rings {
        return None;
    }
    let (inner, outer) = dial.ring(ring);
    let radius = (inner + outer) / 2.0;
    if radius.is_nan() || radius <= 0.0 {
        return None;
    }
    let degrees = |pixels: f32| (pixels / radius).to_degrees();
    let end = window.angle(WINDOW) - degrees(name_gap());
    let begin = window.angle(WINDOW - NAME_SPAN);
    // The dot and the room after it, before the first letter.
    let lead = 2.0 * NAME_DOT + name_dot_gap();
    let room = (end - begin).to_radians() * radius - lead;
    let letters = name.chars().count();
    let text =
        (outer - inner >= NAME_MIN_BAND && room > 0.0).then(|| ellipsize(name, typo::micro(), room)).filter(|cut| {
            let kept = cut.trim_end_matches(typo::ELLIPSIS).chars().count();
            !cut.is_empty() && kept >= NAME_MIN_LETTERS.min(letters)
        });
    let Some(text) = text else {
        return Some(RingName { radius, dot: end - degrees(NAME_DOT), text: None });
    };
    let block = lead + text_width(&text, typo::micro());
    let middle = (end - degrees(block / 2.0)).rem_euclid(360.0);
    let flipped = middle > 90.0 && middle < 270.0;
    let place = if flipped {
        // Read from the edge side: the dot there, the letters running back from it.
        RingName {
            radius,
            dot: end - degrees(NAME_DOT),
            text: Some(NameText { text, start: end - degrees(lead), flipped }),
        }
    } else {
        // Read towards the edge: the block ends at `end`.
        let start = end - degrees(block);
        RingName {
            radius,
            dot: start + degrees(NAME_DOT),
            text: Some(NameText { text, start: start + degrees(lead), flipped }),
        }
    };
    Some(place)
}

/// The top-left corner of a tooltip of `size` for the pointer at `pointer`: [`tip_offset()`] right
/// of and below it; left of it where `area` (the canvas) ends on the right, above it where it ends
/// at the bottom; never over the edges of `area` when it fits at all.
pub fn tip_origin(pointer: Point, size: Size, area: Rectangle) -> Point {
    let right = pointer.x + tip_offset();
    let x = if right + size.width <= area.x + area.width { right } else { pointer.x - tip_offset() - size.width };
    let below = pointer.y + tip_offset();
    let y = if below + size.height <= area.y + area.height { below } else { pointer.y - tip_offset() - size.height };
    let x = x.clamp(area.x, (area.x + area.width - size.width).max(area.x));
    let y = y.clamp(area.y, (area.y + area.height - size.height).max(area.y));
    Point::new(x, y)
}

/// One letter of a text along an arc.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub letter: String,
    /// The direction of the letter's middle (degrees, unwrapped like the arc).
    pub angle: f32,
    /// How far to turn the letter (radians): along the arc, upright in the bottom half.
    pub turn: f32,
}

/// The letters of `text` in `level` on a circle of `radius`, starting `start` degrees and running
/// clockwise, or (with `flipped`) running anticlockwise with the letters turned over, so that
/// the text stands upright in the bottom half. Widths come from [`typo::text_width`], the same
/// estimate [`typo::ellipsize`] cuts with.
pub fn glyphs(text: &str, level: typo::Level, radius: f32, start: f32, flipped: bool) -> Vec<Glyph> {
    if radius <= 0.0 {
        return Vec::new();
    }
    let direction = if flipped { -1.0 } else { 1.0 };
    let mut along = 0.0;
    text.chars()
        .map(|c| {
            let letter = c.to_string();
            let width = text_width(&letter, level);
            let angle = start + direction * ((along + width / 2.0) / radius).to_degrees();
            along += width;
            let turn = angle.to_radians() + if flipped { std::f32::consts::PI } else { 0.0 };
            Glyph { letter, angle, turn }
        })
        .collect()
}

/// The nights of `night` (22:00–06:00 by default, as in the timeline) in the window: minutes after
/// its start, cut to `[0, END]`, in order; none without a night. A window is shorter than a day,
/// so there are at most two, and one that started the evening before the window counts.
pub fn nights(window: &Window, night: Night) -> Vec<(f32, f32)> {
    if night.is_none() {
        return Vec::new();
    }
    let evening = |days: i64| {
        let day = window.start.date() + Duration::days(days);
        day.and_time(NaiveTime::MIN) + Duration::minutes(i64::from(night.from.minutes()))
    };
    let length = Duration::minutes(i64::from(night.minutes()));
    (-1..=1)
        .map(evening)
        .map(|start| (window.rel(start).max(0.0), window.rel(start + length).min(END)))
        .filter(|(from, to)| to > from)
        .collect()
}

/// The working `hours` in the window (task K.33): minutes after its start, cut to `[0, END]`, in
/// order, as [`nights`] does; none without working hours. Those of the day before and after the
/// window's start count too, since the window may reach into them.
pub fn working(window: &Window, hours: WorkingHours) -> Vec<(f32, f32)> {
    if hours.is_none() {
        return Vec::new();
    }
    let at = |days: i64, minutes: u32| {
        let day = window.start.date() + Duration::days(days);
        day.and_time(NaiveTime::MIN) + Duration::minutes(i64::from(minutes))
    };
    (-1..=1)
        .map(|days| {
            (window.rel(at(days, hours.from.minutes())).max(0.0), window.rel(at(days, hours.to.minutes())).min(END))
        })
        .filter(|(from, to)| to > from)
        .collect()
}

/// `[from, to)` cut at the edges of `nights` (sorted, apart, as from [`nights`]): the stretches in
/// order, each with whether it lies in a night. Drawing them one by one puts the right ground under
/// every piece.
pub fn split(from: f32, to: f32, nights: &[(f32, f32)]) -> Vec<(f32, f32, bool)> {
    let mut out = Vec::new();
    let mut at = from;
    for &(start, end) in nights {
        if end <= at || start >= to {
            continue;
        }
        if start > at {
            out.push((at, start, false));
        }
        let end = end.min(to);
        out.push((at.max(start), end, true));
        at = end;
    }
    if to > at {
        out.push((at, to, false));
    }
    out
}

/// Where the moon stands on the numerals' circle: at the first half hour of the clock inside a
/// night of the window that is not in the fading quarter (between two numerals, so it never covers
/// one), clear of the time chip at the start and of the seam at the end. Only when the whole night
/// lies in the fading quarter it stands at the first half hour there and fades with it. `None`
/// when no half hour of a night is in the window.
pub fn moon(window: &Window, nights: &[(f32, f32)]) -> Option<f32> {
    let clear = NUMERAL_CLEARANCE..=WINDOW - NUMERAL_CLEARANCE;
    let in_night = |rel: f32| nights.iter().any(|(from, to)| (*from..=*to).contains(&rel));
    // Ticks are every half hour; those that are not full hours are the half hours.
    let halves: Vec<f32> = window
        .ticks()
        .into_iter()
        .filter(|(rel, full)| !full && clear.contains(rel) && in_night(*rel))
        .map(|(rel, _)| rel)
        .collect();
    halves.iter().copied().find(|rel| *rel <= VISIBLE).or_else(|| halves.first().copied())
}

/// The moon ([`moon`]) as a click target, [`moon_target_size()`] square around its icon on the numerals'
/// circle of `dial`; `None` without a moon.
pub fn moon_target(dial: &Dial, window: &Window, nights: &[(f32, f32)]) -> Option<Rectangle> {
    let rel = moon(window, nights)?;
    let at = dial.point(dial.numerals, window.angle(rel));
    let half = moon_target_size() / 2.0;
    Some(Rectangle::new(Point::new(at.x - half, at.y - half), Size::new(moon_target_size(), moon_target_size())))
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("date").and_hms_opt(hour, minute, 0).expect("time")
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn time_maps_to_the_angle_of_a_twelve_hour_clock() {
        assert!(close(clock_angle(at(13, 0, 0)), 0.0));
        assert!(close(clock_angle(at(13, 12, 0)), 0.0));
        assert!(close(clock_angle(at(13, 15, 0)), 90.0), "15:00 stands where the 3 does");
        assert!(close(clock_angle(at(13, 14, 20)), 70.0));
        assert!(close(clock_angle(at(13, 20, 30)), 255.0));
    }

    #[test]
    fn a_turned_window_maps_time_to_angle_and_back() {
        let now = at(13, 14, 20);
        let window = Window::new(now + Duration::minutes(90));
        assert_eq!(window.start, at(13, 15, 50));
        assert!(close(window.angle0, 115.0));
        assert!(close(window.rel(at(13, 16, 50)), 60.0));
        assert!(close(window.rel(now), -90.0));
        assert!(close(window.angle(60.0), 145.0));
        assert!(close(window.angle(WINDOW), 115.0 + 360.0), "unwrapped: a full turn later");
        assert_eq!(window.at(30.0), at(13, 16, 20));
        // Back from a direction: 02:00 at night lies 10 h 10 min into the window.
        assert!(close(window.rel_at(clock_angle(at(14, 2, 0))), 610.0));
        assert!(close(window.rel_of(window.angle(123.0)), 123.0));
    }

    #[test]
    fn today_opens_at_now_and_other_days_in_the_morning() {
        let day = at(14, 0, 0).date();
        let now = at(13, 14, 20);
        assert_eq!(
            day_start(now.date(), now, [at(13, 7, 0)], Night::DEFAULT),
            now,
            "today: now, whatever came earlier"
        );
        assert_eq!(day_start(day, now, [], Night::DEFAULT), at(14, 8, 0), "no events: 08:00");
        assert_eq!(
            day_start(day, now, [at(14, 10, 0), at(14, 6, 30)], Night::DEFAULT),
            at(14, 6, 0),
            "the full hour before"
        );
        assert_eq!(day_start(day, now, [at(14, 5, 0)], Night::DEFAULT), at(14, 6, 0), "never before the night ends");
        assert_eq!(day_start(day, now, [at(14, 9, 0)], Night::DEFAULT), at(14, 8, 0), "a later event changes nothing");
        assert_eq!(
            day_start(day, now, [at(13, 23, 0), at(15, 6, 0)], Night::DEFAULT),
            at(14, 8, 0),
            "only that day's events"
        );
        // A day in the past opens the same way.
        assert_eq!(day_start(at(12, 0, 0).date(), now, [at(12, 7, 15)], Night::DEFAULT), at(12, 7, 0));
    }

    #[test]
    fn dragging_turns_by_the_angle_between_two_moves_on_a_five_minute_grid() {
        assert!(close(turn_between(90.0, 120.0), 30.0));
        assert!(close(turn_between(120.0, 90.0), -30.0));
        assert!(close(turn_between(350.0, 10.0), 20.0), "clockwise over the 12");
        assert!(close(turn_between(10.0, 350.0), -20.0), "anticlockwise over the 12");
        assert!(close(minutes_of_turn(30.0), 60.0), "an hour per numeral");
        assert!(close(minutes_of_turn(-360.0), -WINDOW), "a turn back is twelve hours back");
        assert_eq!(snap(7.0), (5, 2.0));
        assert_eq!(snap(-7.0), (-5, -2.0), "the rest keeps its sign");
        assert_eq!(snap(4.0), (0, 4.0), "less than a step waits");
        // Two slow moves of 1.5° each (3 minutes) make one step together.
        let (first, rest) = snap(minutes_of_turn(turn_between(0.0, 1.5)));
        let (second, _) = snap(minutes_of_turn(turn_between(1.5, 3.0)) + rest);
        assert_eq!((first, second), (0, 5));
    }

    #[test]
    fn a_press_becomes_a_drag_after_four_pixels_on_the_dial() {
        let origin = Point::new(100.0, 100.0);
        assert!(!is_drag(origin, Point::new(103.0, 100.0)), "a click that trembles stays a click");
        assert!(is_drag(origin, Point::new(103.0, 104.0)));
        let dial = Dial::fit(Size::new(800.0, 700.0), 24.0, 2);
        assert!(dial.grabbable(dial.point(dial.numerals, 45.0)), "on the numerals");
        assert!(dial.grabbable(dial.point(dial.inner + 1.0, 200.0)), "on the rings");
        assert!(!dial.grabbable(dial.center), "not in the centre");
        assert!(!dial.grabbable(dial.point(dial.radius + 4.0, 0.0)), "not beside the dial");
    }

    #[test]
    fn the_turn_stays_between_twelve_hours_back_and_a_day_ahead() {
        assert_eq!(clamp_offset(0), 0);
        assert_eq!(clamp_offset(-13 * 60), LOOK_BACK);
        assert_eq!(clamp_offset(25 * 60), LOOK_AHEAD);
        assert_eq!((WHEEL_STEP, HOUR_STEP), (15, 60));
    }

    #[test]
    fn the_last_quarter_fades_from_full_to_nothing() {
        assert_eq!(opacity(0.0), 1.0);
        assert_eq!(opacity(VISIBLE), 1.0, "nine hours in full");
        let middle = opacity((VISIBLE + END) / 2.0);
        // smoothstep(0.5) = 0.5, 0.5^0.8 ≈ 0.574.
        assert!((middle - (1.0 - 0.5f32.powf(0.8))).abs() < 1e-4, "{middle}");
        assert!(opacity(VISIBLE + 30.0) > 0.85, "a soft start");
        assert!(opacity(END - 1.0) < 0.01, "nearly gone just before the edge");
        assert_eq!(opacity(END), 0.0);
        let mut last = 1.0;
        for minute in (VISIBLE as i32..=END as i32).step_by(6) {
            let now = opacity(minute as f32);
            assert!(now <= last, "falls steadily: {minute}");
            last = now;
        }
    }

    #[test]
    fn pieces_cut_the_fade_on_a_fixed_grid() {
        let all = pieces(0.0, END);
        assert_eq!(all[0], Piece { from: 0.0, to: VISIBLE, opacity: 1.0 });
        assert!(all[1..].iter().all(|p| close(p.to - p.from, FADE_PIECE) || p.to == END));
        assert!(all.windows(2).all(|w| close(w[0].to, w[1].from)), "no gaps");
        // An event in the fade gets the same pieces as the ring under it.
        let event = pieces(600.0, 610.0);
        assert_eq!(event.len(), 5);
        assert_eq!((event[0].from, event[0].to), (600.0, 600.0 + FADE_PIECE));
        let ring = all.iter().find(|p| p.from == 600.0).expect("the same grid");
        assert_eq!(ring.opacity, event[0].opacity);
        assert!(pieces(-30.0, 0.0).is_empty(), "before the window");
        assert!(pieces(700.0, 800.0).iter().all(|p| p.to <= END));
    }

    #[test]
    fn numerals_show_the_real_hour_and_spare_the_start() {
        let window = Window::new(at(13, 14, 20));
        let numerals = window.numerals();
        let hours: Vec<u32> = numerals.iter().map(|n| n.hour).collect();
        assert_eq!(hours, [15, 16, 17, 18, 19, 20, 21, 22, 23, 0, 1, 2]);
        assert_eq!(numerals[0].rel, 40.0);
        assert!(close(window.angle(numerals[0].rel).rem_euclid(360.0), 90.0), "15 at the 3");
        let majors: Vec<u32> = numerals.iter().filter(|n| n.major).map(|n| n.hour).collect();
        assert_eq!(majors, [15, 18, 21, 0]);
        // At 14:50 the 15 would stand next to the time chip.
        let hours: Vec<u32> = Window::new(at(13, 14, 50)).numerals().iter().map(|n| n.hour).collect();
        assert_eq!(hours.first(), Some(&16));
        assert_eq!(hours.last(), Some(&2), "02:00 lies 11 h 10 min in, clear of the start");
    }

    #[test]
    fn ticks_every_half_hour_long_on_the_hour() {
        let ticks = Window::new(at(13, 14, 20)).ticks();
        assert_eq!(ticks[0], (10.0, false), "14:30");
        assert_eq!(ticks[1], (40.0, true), "15:00");
        assert_eq!(ticks.len(), 24, "14:30 to 02:00 within the window less its seam");
    }

    #[test]
    fn rings_share_the_room_and_leave_the_middle() {
        let dial = Dial::fit(Size::new(800.0, 700.0), 24.0, 3);
        assert_eq!(dial.center, Point::new(400.0, 350.0));
        assert!(close(dial.radius, 326.0));
        assert!(close(dial.outer, 286.0));
        assert!(close(dial.inner, 163.0), "half the radius, wider rings would be over 64");
        assert!(close(dial.band, (286.0 - 163.0 - 8.0) / 3.0));
        let (inner, outer) = dial.ring(2);
        assert!(close(outer, 286.0 - 2.0 * (dial.band + RING_GAP)));
        assert!(close(inner, dial.inner));
        // One ring in a large dial: at most 64 wide.
        let one = Dial::fit(Size::new(1000.0, 900.0), 24.0, 1);
        assert!(close(one.band, RING_MAX));
        // A large window does not grow the dial past its maximum.
        let large = Dial::fit(Size::new(1600.0, 1200.0), 24.0, 3);
        assert!(close(large.radius, MAX_DIAMETER / 2.0));
        assert_eq!(large.center, Point::new(800.0, 600.0));
        let none = Dial::fit(Size::new(400.0, 400.0), 8.0, 0);
        assert_eq!((none.band, none.inner), (0.0, none.outer));
    }

    #[test]
    fn overlapping_events_share_their_ring_in_lanes() {
        let dial = Dial::fit(Size::new(1000.0, 900.0), 24.0, 2);
        let window = Window::new(at(13, 14, 20));
        let spans = [
            Span { id: 0, ring: 0, from: 0.0, to: 40.0, starts: false },
            Span { id: 1, ring: 0, from: 10.0, to: 40.0, starts: true },
            Span { id: 2, ring: 1, from: 250.0, to: 310.0, starts: true },
        ];
        let arcs = arcs(&dial, &window, &spans);
        let (a, b, c) = (arcs[0], arcs[1], arcs[2]);
        assert!(close(a.outer, dial.outer), "the first lane outside");
        assert!(close(a.width(), dial.band / 2.0 - LANE_GAP));
        assert!(b.outer <= a.inner, "side by side across the ring");
        assert!(close(c.width(), dial.band), "alone in its ring");
        assert!(close(c.outer, dial.ring(1).1));
        assert!(close(c.from, window.angle(250.0) + ARC_INSET));
        assert!(close(c.to, window.angle(310.0) - ARC_INSET));
        let instant = super::arcs(&dial, &window, &[Span { id: 9, ring: 0, from: 5.0, to: 5.0, starts: true }]);
        assert!(close(instant[0].to - instant[0].from, ARC_MIN));
    }

    #[test]
    fn a_click_finds_the_event_under_it_or_free_room() {
        let dial = Dial::fit(Size::new(1000.0, 900.0), 24.0, 1);
        let window = Window::new(at(13, 14, 20));
        // 15:00–16:00 in the only ring.
        let arcs = arcs(&dial, &window, &[Span { id: 4, ring: 0, from: 40.0, to: 100.0, starts: true }]);
        let middle = (dial.inner + dial.outer) / 2.0;
        let on_event = dial.point(middle, window.angle(70.0));
        assert_eq!(hit(&dial, &window, &arcs, on_event), Some(Hit::Event(4)));
        let free = dial.point(middle, window.angle(200.0));
        match hit(&dial, &window, &arcs, free) {
            Some(Hit::Free(rel)) => assert!((rel - 200.0).abs() < 0.1, "{rel}"),
            other => panic!("expected free room, got {other:?}"),
        }
        assert_eq!(hit(&dial, &window, &arcs, dial.center), None, "the middle");
        assert_eq!(hit(&dial, &window, &arcs, dial.point(dial.numerals, window.angle(70.0))), None, "the numerals");
        assert_eq!(hit(&dial, &window, &arcs, dial.point(middle, window.angle(END + 2.0))), None, "the seam");
    }

    #[test]
    fn titles_run_along_the_arc_and_turn_over_below() {
        let level = typo::caption();
        let top = glyphs("Run", level, 200.0, 10.0, false);
        assert_eq!(top.len(), 3);
        assert!(top.windows(2).all(|w| w[1].angle > w[0].angle), "clockwise");
        let first = text_width("R", level) / 2.0 / 200.0;
        assert!(close(top[0].angle, 10.0 + first.to_degrees()));
        assert!(close(top[0].turn, top[0].angle.to_radians()));
        let below = glyphs("Run", level, 200.0, 200.0, true);
        assert!(below.windows(2).all(|w| w[1].angle < w[0].angle), "anticlockwise");
        assert!(close(below[0].turn, below[0].angle.to_radians() + std::f32::consts::PI));
        let arc = |from: f32, to: f32| Arc { id: 0, inner: 100.0, outer: 140.0, from, to, starts: true };
        assert!(!arc(20.0, 60.0).flipped());
        assert!(arc(150.0, 200.0).flipped());
        assert!(!arc(340.0, 380.0).flipped(), "over the 12");
        assert!(close(arc(0.0, 90.0).length(), std::f32::consts::FRAC_PI_2 * 120.0));
    }

    #[test]
    fn the_night_lies_in_the_window_also_over_midnight() {
        // 14:20: the night starts 7 h 40 min in and runs past the end of the window.
        assert_eq!(nights(&Window::new(at(13, 14, 20)), Night::DEFAULT), [(460.0, END)]);
        // 08:00: no night at all before 19:54.
        assert!(nights(&Window::new(at(18, 8, 0)), Night::DEFAULT).is_empty());
        // 23:10, past midnight: the night begun the evening before runs until 06:00.
        assert_eq!(nights(&Window::new(at(13, 23, 10)), Night::DEFAULT), [(0.0, 410.0)]);
        // 01:00: still the night of the evening before.
        assert_eq!(nights(&Window::new(at(14, 1, 0)), Night::DEFAULT), [(0.0, 300.0)]);
        // 20:00: the whole night of eight hours fits, from 22:00 to 06:00.
        assert_eq!(nights(&Window::new(at(13, 20, 0)), Night::DEFAULT), [(120.0, 600.0)]);
    }

    #[test]
    fn stretches_are_cut_at_the_edges_of_the_night() {
        let nights = [(120.0, 600.0)];
        assert_eq!(split(0.0, END, &nights), [(0.0, 120.0, false), (120.0, 600.0, true), (600.0, END, false)]);
        assert_eq!(split(100.0, 200.0, &nights), [(100.0, 120.0, false), (120.0, 200.0, true)]);
        assert_eq!(split(200.0, 300.0, &nights), [(200.0, 300.0, true)], "inside the night");
        assert_eq!(split(0.0, 60.0, &nights), [(0.0, 60.0, false)], "before it");
        assert_eq!(split(0.0, 60.0, &[]), [(0.0, 60.0, false)]);
    }

    #[test]
    fn the_moon_stands_at_the_first_half_hour_of_the_night() {
        let window = Window::new(at(13, 14, 20));
        assert_eq!(moon(&window, &nights(&window, Night::DEFAULT)), Some(490.0), "22:30");
        assert_eq!(window.at(490.0), at(13, 22, 30));
        // Starting at 23:10 inside the night: 23:30, just clear of the chip.
        let late = Window::new(at(13, 23, 10));
        assert_eq!(moon(&late, &nights(&late, Night::DEFAULT)), Some(20.0));
        // At 23:20 the 23:30 would touch the chip: 00:30.
        let later = Window::new(at(13, 23, 20));
        assert_eq!(later.at(moon(&later, &nights(&later, Night::DEFAULT)).expect("a moon")), at(14, 0, 30));
        let morning = Window::new(at(18, 8, 0));
        assert_eq!(moon(&morning, &nights(&morning, Night::DEFAULT)), None, "no night, no moon");
    }

    fn night(from: (u32, u32), to: (u32, u32)) -> Night {
        let quarter = |(hour, minute)| torro_core::settings::Quarter::new(hour, minute).expect("quarter");
        Night { from: quarter(from), to: quarter(to) }
    }

    fn hours(from: (u32, u32), to: (u32, u32)) -> WorkingHours {
        let quarter = |(hour, minute)| torro_core::settings::Quarter::new(hour, minute).expect("quarter");
        WorkingHours { from: quarter(from), to: quarter(to) }
    }

    /// Task K.33: the arc of the working hours runs from their start to their end in the window.
    #[test]
    fn the_working_arc_runs_from_start_to_end_in_the_window() {
        let day = hours((7, 30), (16, 0));
        assert_eq!(working(&Window::new(at(13, 6, 0)), day), [(90.0, 600.0)]);
        assert_eq!(working(&Window::new(at(13, 10, 30)), day), [(0.0, 330.0)], "begun before the window");
        assert!(working(&Window::new(at(13, 18, 0)), day).is_empty(), "the evening has none");
        assert_eq!(working(&Window::new(at(13, 20, 0)), day), [(690.0, END)], "the next morning");
        assert!(working(&Window::new(at(13, 6, 0)), hours((9, 0), (9, 0))).is_empty(), "no working hours");
    }

    #[test]
    fn the_night_sector_follows_the_night_it_is_given() {
        let late = night((23, 30), (7, 0));
        // 14:20: the night starts 9 h 10 min in and runs past the end of the window.
        assert_eq!(nights(&Window::new(at(13, 14, 20)), late), [(550.0, END)]);
        // 02:00: the night begun at 23:30 runs until 07:00.
        assert_eq!(nights(&Window::new(at(14, 2, 0)), late), [(0.0, 300.0)]);
        // A night after midnight only.
        assert_eq!(nights(&Window::new(at(13, 20, 0)), night((1, 0), (9, 0))), [(300.0, END)]);
        let none = Night::none_at(late.from);
        assert!(nights(&Window::new(at(13, 20, 0)), none).is_empty(), "no night, no sector");
        let window = Window::new(at(13, 20, 0));
        assert_eq!(moon(&window, &nights(&window, none)), None, "and no moon");
    }

    #[test]
    fn another_day_opens_when_its_night_ends() {
        let (day, now) = (at(14, 0, 0).date(), at(13, 14, 20));
        assert_eq!(day_start(day, now, [at(14, 6, 15)], night((23, 30), (7, 0))), at(14, 7, 0));
        assert_eq!(day_start(day, now, [at(14, 5, 0)], night((22, 0), (6, 30))), at(14, 6, 30));
        let none = Night::none_at(night((22, 0), (6, 0)).from);
        assert_eq!(day_start(day, now, [at(14, 5, 10)], none), at(14, 5, 0), "without a night any hour");
        assert_eq!(day_start(day, now, [], none), at(14, 8, 0));
    }

    #[test]
    fn the_moon_stays_out_of_the_fading_quarter_while_the_night_allows() {
        // 21:00: the night from 22:00 to 06:00 fills the window from 1 h on; 22:30 is in full view.
        let evening = Window::new(at(13, 21, 0));
        let rel = moon(&evening, &nights(&evening, Night::DEFAULT)).expect("a moon");
        assert!(rel <= VISIBLE && evening.at(rel) == at(13, 22, 30));
        // 11:00: the night begins 11 h in, in the fading quarter: the moon fades with it there.
        let morning = Window::new(at(13, 11, 0));
        let rel = moon(&morning, &nights(&morning, Night::DEFAULT)).expect("a moon");
        assert!(rel > VISIBLE && morning.at(rel) == at(13, 22, 30));
    }

    #[test]
    fn a_point_finds_its_ring_but_not_the_gaps_between_them() {
        let dial = Dial::fit(Size::new(1000.0, 900.0), 24.0, 3);
        for ring in 0..3 {
            let (inner, outer) = dial.ring(ring);
            assert_eq!(ring_at(&dial, (inner + outer) / 2.0), Some(ring));
        }
        let (inner, _) = dial.ring(0);
        assert_eq!(ring_at(&dial, inner - RING_GAP / 2.0), None, "the gap");
        assert_eq!(ring_at(&dial, dial.outer + 1.0), None, "outside");
        assert_eq!(ring_at(&dial, dial.inner - 1.0), None, "the middle");
    }

    #[test]
    fn a_ring_names_its_calendar_just_before_the_edge() {
        let dial = Dial::fit(Size::new(1000.0, 900.0), 24.0, 3);
        // 14:20: the last hour runs from 01:20 to 02:20, top right of the dial.
        let window = Window::new(at(13, 14, 20));
        let place = ring_name(&dial, &window, 0, "Work").expect("a ring");
        let (inner, outer) = dial.ring(0);
        assert!(close(place.radius, (inner + outer) / 2.0));
        let text = place.text.as_ref().expect("room for the name");
        assert_eq!(text.text, "Work");
        assert!(!text.flipped, "top half: read towards the edge");
        let edge = window.angle(WINDOW);
        let gap = (name_gap() / place.radius).to_degrees();
        let end = text.start + (text_width("Work", typo::micro()) / place.radius).to_degrees();
        assert!(close(end, edge - gap), "the name ends {} before the edge: {end} vs {}", name_gap(), edge - gap);
        assert!(place.dot < text.start, "the dot comes first");
        assert!(place.dot > window.angle(WINDOW - NAME_SPAN), "within the last hour");
        // It turns with the dial: half an hour later it stands 15° further.
        let later = ring_name(&dial, &Window::new(at(13, 14, 50)), 0, "Work").expect("a ring");
        assert!(close(later.dot, place.dot + 15.0));
        // A long name is cut; a ring with too little room keeps only the dot.
        let long = ring_name(&dial, &window, 2, "Family and friends and neighbours of the street").expect("a ring");
        let cut = long.text.expect("a part of it").text;
        assert!(cut.ends_with(typo::ELLIPSIS), "{cut}");
        let narrow = Dial::fit(Size::new(300.0, 300.0), 8.0, 4);
        assert!(narrow.band < NAME_MIN_BAND);
        let dot = ring_name(&narrow, &window, 0, "Work").expect("a ring");
        assert_eq!(dot.text, None);
        assert!(close(dot.dot, edge - (name_gap() / dot.radius).to_degrees() - (NAME_DOT / dot.radius).to_degrees()));
        // An empty name: the dot alone; no ring: nothing.
        assert_eq!(ring_name(&dial, &window, 1, "").expect("a ring").text, None);
        assert_eq!(ring_name(&dial, &window, 3, "Work"), None);
    }

    #[test]
    fn in_the_bottom_half_the_name_reads_from_the_edge_and_stands_upright() {
        let dial = Dial::fit(Size::new(1000.0, 900.0), 24.0, 3);
        // 08:00: the edge stands at the 8, the last hour from 19:00 to 20:00 at the bottom left.
        let window = Window::new(at(13, 8, 0));
        let place = ring_name(&dial, &window, 0, "Work").expect("a ring");
        let text = place.text.expect("room for the name");
        assert!(text.flipped);
        let edge = window.angle(WINDOW) - (name_gap() / place.radius).to_degrees();
        assert!(place.dot < edge && place.dot > text.start, "dot at the edge side, the letters behind it");
        // The letters run anticlockwise from there and stay in the last hour.
        let letters = glyphs(&text.text, typo::micro(), place.radius, text.start, true);
        assert!(letters.iter().all(|glyph| glyph.angle > window.angle(WINDOW - NAME_SPAN)));
    }

    #[test]
    fn a_tooltip_stands_beside_the_pointer_and_never_over_the_edge() {
        let area = Rectangle::new(Point::ORIGIN, Size::new(400.0, 300.0));
        let card = Size::new(120.0, 60.0);
        assert_eq!(tip_origin(Point::new(100.0, 100.0), card, area), Point::new(112.0, 112.0), "right, below");
        assert_eq!(tip_origin(Point::new(350.0, 100.0), card, area), Point::new(218.0, 112.0), "left at the edge");
        assert_eq!(tip_origin(Point::new(100.0, 280.0), card, area), Point::new(112.0, 208.0), "above at the bottom");
        assert_eq!(tip_origin(Point::new(395.0, 295.0), card, area), Point::new(263.0, 223.0), "in the corner");
        // A card wider than the canvas starts at its left edge.
        assert_eq!(tip_origin(Point::new(10.0, 10.0), Size::new(500.0, 60.0), area).x, 0.0);
    }

    #[test]
    fn the_moon_is_a_target_of_its_own() {
        let window = Window::new(at(13, 14, 20));
        let dial = Dial::fit(Size::new(600.0, 600.0), 16.0, 2);
        let target = moon_target(&dial, &window, &nights(&window, Night::DEFAULT)).expect("a moon");
        assert_eq!(target.width, moon_target_size());
        let center = dial.point(dial.numerals, window.angle(490.0));
        assert!(target.contains(center));
        let morning = Window::new(at(18, 8, 0));
        assert_eq!(moon_target(&dial, &morning, &nights(&morning, Night::DEFAULT)), None);
    }
}
