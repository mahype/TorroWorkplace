//! Geometry of the radar, free of Iced widgets so that it can be tested: time to angle and back
//! within the window, how the last quarter fades out, the ticks and numerals of the dial, the
//! radii of the rings, the arcs of the events with their lanes, hit-testing, and where the
//! letters of a title go along an arc. `radar.rs` draws the result.
//!
//! Angles are in degrees, 0 at the top (where the 12 stands), growing clockwise, as on a clock.
//! Times inside the window are minutes after its start ("rel"); the window starts at now plus the
//! look-ahead and covers twelve hours, one turn of the 12-hour dial.

use chrono::{Duration, NaiveDateTime, Timelike};
use iced::{Point, Size};
use torro_gui::typo::{self, text_width};

use crate::views::glance;

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
/// One notch of the mouse wheel over the dial looks this far ahead or back (minutes).
pub const WHEEL_STEP: i64 = 15;
/// ‹ › in the header look this far ahead or back (minutes).
pub const HOUR_STEP: i64 = 60;
/// How far back the look-ahead goes (minutes).
pub const LOOK_BACK: i64 = -12 * 60;
/// How far ahead the look-ahead goes (minutes).
pub const LOOK_AHEAD: i64 = 24 * 60;
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

/// The look-ahead `minutes` kept to [`LOOK_BACK`] … [`LOOK_AHEAD`].
pub fn clamp_offset(minutes: i64) -> i64 {
    minutes.clamp(LOOK_BACK, LOOK_AHEAD)
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
    /// Where the window starts: now plus the look-ahead.
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
        let radius = ((bounds.width.min(bounds.height) - 2.0 * margin) / 2.0).max(0.0);
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

/// Whole minutes from `from` to `to`, rounded up ("40 min left" while 39½ are), never below 0.
pub fn minutes_until(from: NaiveDateTime, to: NaiveDateTime) -> i64 {
    let seconds = (to - from).num_seconds().max(0);
    (seconds + 59) / 60
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
    fn the_window_starts_at_now_plus_the_look_ahead() {
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
    fn the_look_ahead_stays_between_twelve_hours_back_and_a_day_ahead() {
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
        let dial = Dial::fit(Size::new(1000.0, 900.0), 24.0, 3);
        assert_eq!(dial.center, Point::new(500.0, 450.0));
        assert!(close(dial.radius, 426.0));
        assert!(close(dial.outer, 386.0));
        assert!(close(dial.inner, 213.0), "half the radius, wider rings would be over 64");
        assert!(close(dial.band, (386.0 - 213.0 - 8.0) / 3.0));
        let (inner, outer) = dial.ring(2);
        assert!(close(outer, 386.0 - 2.0 * (dial.band + RING_GAP)));
        assert!(close(inner, dial.inner));
        // One ring in a large dial: at most 64 wide.
        let one = Dial::fit(Size::new(1000.0, 900.0), 24.0, 1);
        assert!(close(one.band, RING_MAX));
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
        let level = typo::CAPTION;
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
    fn minutes_left_round_up() {
        assert_eq!(minutes_until(at(13, 14, 20), at(13, 15, 0)), 40);
        let late = at(13, 14, 20) + Duration::seconds(30);
        assert_eq!(minutes_until(late, at(13, 15, 0)), 40);
        assert_eq!(minutes_until(at(13, 15, 0), at(13, 14, 0)), 0);
    }
}
