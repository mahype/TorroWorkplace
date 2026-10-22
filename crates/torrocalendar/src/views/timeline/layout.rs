//! Geometry of the timeline (task K.21), free of Iced widgets so that it can be tested: the track
//! of days and nights along x (time ↔ x with folded and unfolded nights), the rows across y, the
//! event columns with their lanes, the dots of events in a folded night, the all-day chips, free
//! gaps, the line of the working hours (task K.33) and hit-testing. `timeline.rs` draws the result.
//!
//! The night is the suite's setting (`Settings::night`, task K.27), 22:00–06:00 by default. x runs
//! along the track from the end of the night on its first day (06:00) to the end of the night
//! after its last day, relative to the left edge of the canvas; y from the top of the canvas. Each
//! day is a part from the end of one night to the start of the next at [`HOUR_WIDTH`] an hour;
//! each night a part [`NIGHT_WIDTH`] wide while folded, and as wide as its hours of the day while
//! unfolded. Without a night (`from == to`) the days follow each other, 24 hours each.
//!
//! [`is_double_click`] is here too: a double-click on the moon opens the night's card, here and in
//! the radar.
//!
//! Lanes, all-day days and free gaps come from the week's geometry (`week/layout.rs`).

use std::collections::BTreeSet;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use iced::time::Instant;
use iced::{Point, Rectangle, Size};
use torro_core::settings::{Night, WorkingHours};
use torro_gui::icon;
use torro_gui::theme::{size, space};

use crate::views::week::layout::{TimedSegment, all_day_days, assign_lanes, free_gaps as gaps_in_day};

/// Width of one hour of the day (and of an unfolded night).
pub const HOUR_WIDTH: f32 = 56.0;
/// Width of a folded night.
pub const NIGHT_WIDTH: f32 = 48.0;
/// Height of the row with the day labels ("Tue 13").
pub fn day_head() -> f32 {
    size::control_sm()
}
/// Height of the row with the hour labels.
pub fn axis_row() -> f32 {
    space::xl()
}
/// Pitch of the rows of all-day chips; a chip is [`CHIP_HEIGHT`] high.
pub fn chip_pitch() -> f32 {
    space::xl()
}
/// Height of an all-day chip and of the "Collapse night" button (`calendar.html`, `.chip`).
pub const CHIP_HEIGHT: f32 = 22.0;
/// Space above the first and below the last chip in the all-day row (28 for one row).
const STRIP_PAD: f32 = 3.0;
/// Height of the row at the bottom that says "3 h free".
pub fn free_row() -> f32 {
    size::control_sm()
}
/// Space between the all-day row and the event columns, and between the columns and the free row.
fn row_gap() -> f32 {
    space::xs()
}
/// Narrowest event column, so that a short entry stays visible and clickable.
pub const MIN_EVENT_WIDTH: f32 = 16.0;
/// The event columns are at most this high (eight hours of the week grid): a rotated title and
/// its time fit, and in a tall window the columns do not stretch into empty tinted bars.
pub fn max_lanes_height() -> f32 {
    8.0 * size::hour()
}
/// Gap at the right end of an event column, so that back-to-back events stay apart.
const GAP: f32 = 2.0;
/// Gap between the lanes of overlapping events (they share the height).
const LANE_GAP: f32 = 3.0;
/// Clicks in free time round the time down to a multiple of this many minutes.
pub const SLOT_MINUTES: u32 = 15;
/// "Today" and the start scroll so that now stands at this fraction of the visible width.
pub const FOCUS_AT: f32 = 0.3;
/// The teeth of a folded night's edges: this wide …
const TOOTH_WIDTH: f32 = 8.0;
/// … and this deep.
pub const TOOTH_DEPTH: f32 = 5.0;
/// The dots of the events in a folded night run from this fraction of the band's height …
const DOTS_FROM: f32 = 0.12;
/// … over this fraction of it, by their start.
const DOTS_SPAN: f32 = 0.7;
/// The dots stand this far left of the band's right edge, beside its label.
const DOT_INSET: f32 = 10.0;

fn minutes_between(from: NaiveDateTime, to: NaiveDateTime) -> f32 {
    (to - from).num_seconds() as f32 / 60.0
}

/// The moon is a click target this large (24, the smallest target), around its icon.
pub fn moon_target_size() -> f32 {
    space::xl()
}
/// Two presses at most this far apart in time …
pub const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);
/// … and this close (pixels) are a double-click.
const DOUBLE_CLICK_SLOP: f32 = 6.0;

/// Whether a press at `second` after one at `first` makes a double-click: soon enough and in
/// almost the same place.
pub fn is_double_click(first: (Instant, Point), second: (Instant, Point)) -> bool {
    let soon = second.0.checked_duration_since(first.0).is_some_and(|gap| gap <= DOUBLE_CLICK);
    soon && first.1.distance(second.1) <= DOUBLE_CLICK_SLOP
}

/// `day` at `minutes` after its midnight.
fn at_minutes(day: NaiveDate, minutes: u32) -> NaiveDateTime {
    day.and_time(NaiveTime::MIN) + Duration::minutes(i64::from(minutes))
}

/// The end of the night on the morning of `day`: where the day's part of the track starts.
fn morning(day: NaiveDate, night: Night) -> NaiveDateTime {
    at_minutes(day, night.to.minutes())
}

/// What a stretch of the track is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// This day from the end of the night to the start of the next (06:00–22:00).
    Day(NaiveDate),
    /// The night after `evening` (22:00 to 06:00 of the next day); folded into a narrow band
    /// unless `open`.
    Night { evening: NaiveDate, open: bool },
}

/// A stretch of the track: a day or a night, with its times and where it lies along x.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub part: Part,
    pub start: NaiveDateTime,
    /// Exclusive.
    pub end: NaiveDateTime,
    pub x: f32,
    pub width: f32,
}

impl Segment {
    /// A night shown as the narrow band.
    pub fn folded(&self) -> bool {
        matches!(self.part, Part::Night { open: false, .. })
    }

    /// The evening of a night, `None` for a day.
    pub fn evening(&self) -> Option<NaiveDate> {
        match self.part {
            Part::Night { evening, .. } => Some(evening),
            Part::Day(_) => None,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Minutes from start to end (480 for a night of 22:00–06:00).
    pub fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes()
    }

    fn contains(&self, at: NaiveDateTime) -> bool {
        self.start <= at && at < self.end
    }

    /// The x of `at`, which must lie in the segment (kept to its edges otherwise). Inside a folded
    /// night the time is squeezed evenly into the band.
    fn x_of(&self, at: NaiveDateTime) -> f32 {
        let fraction = (minutes_between(self.start, at) / minutes_between(self.start, self.end)).clamp(0.0, 1.0);
        self.x + fraction * self.width
    }

    /// The time at `x`, kept to the segment, to the second.
    fn time_at(&self, x: f32) -> NaiveDateTime {
        let fraction = ((x - self.x) / self.width).clamp(0.0, 1.0);
        let seconds = (fraction * minutes_between(self.start, self.end) * 60.0) as i64;
        self.start + Duration::seconds(seconds)
    }
}

/// The days `[first, after)` laid out along x, each followed by its night.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub segments: Vec<Segment>,
    /// The end of the night on the first day (06:00).
    pub start: NaiveDateTime,
    /// The end of the night after the last day (06:00).
    pub end: NaiveDateTime,
    /// The width of the whole track, the width of the canvas.
    pub width: f32,
}

impl Track {
    /// The track for the days `[first, after)` with `night` after each day; the nights whose
    /// evening is in `open` are unfolded. Without a night ([`Night::is_none`]) there are only days,
    /// each 24 hours from the time the night would end. An empty range gives a track without
    /// segments, zero wide.
    pub fn new(first: NaiveDate, after: NaiveDate, open: &BTreeSet<NaiveDate>, night: Night) -> Track {
        let mut segments = Vec::new();
        let mut x = 0.0;
        let mut day = first;
        let night_minutes = night.minutes();
        let day_width = (24 * 60 - night_minutes) as f32 / 60.0 * HOUR_WIDTH;
        let night_width = night_minutes as f32 / 60.0 * HOUR_WIDTH;
        while day < after {
            let start = morning(day, night);
            let evening = start + Duration::minutes(i64::from(24 * 60 - night_minutes));
            segments.push(Segment { part: Part::Day(day), start, end: evening, x, width: day_width });
            x += day_width;
            if night_minutes > 0 {
                let is_open = open.contains(&day);
                let width = if is_open { night_width } else { NIGHT_WIDTH };
                let part = Part::Night { evening: day, open: is_open };
                segments.push(Segment { part, start: evening, end: start + Duration::days(1), x, width });
                x += width;
            }
            day += Duration::days(1);
        }
        Track { segments, start: morning(first, night), end: morning(after.max(first), night), width: x }
    }

    /// The x of `at`: 0 before the track, its width after it.
    pub fn x_of(&self, at: NaiveDateTime) -> f32 {
        if at <= self.start {
            return 0.0;
        }
        self.segments.iter().find(|segment| at < segment.end).map_or(self.width, |segment| segment.x_of(at))
    }

    /// The segment at `x`, `None` beside the track.
    pub fn segment_at(&self, x: f32) -> Option<&Segment> {
        self.segments.iter().find(|segment| segment.x <= x && x < segment.right())
    }

    /// The folded night `at` falls into.
    pub fn folded_night_of(&self, at: NaiveDateTime) -> Option<&Segment> {
        self.segments.iter().find(|segment| segment.folded() && segment.contains(at))
    }

    /// Every full hour in the days and the unfolded nights, with its x: where the hour lines and
    /// their labels go. A folded night has none.
    pub fn hours(&self) -> Vec<(NaiveDateTime, f32)> {
        let mut hours = Vec::new();
        for segment in self.segments.iter().filter(|segment| !segment.folded()) {
            let mut hour = segment.start.with_minute(0).and_then(|t| t.with_second(0)).unwrap_or(segment.start);
            if hour < segment.start {
                hour += Duration::hours(1);
            }
            while hour < segment.end {
                hours.push((hour, segment.x_of(hour)));
                hour += Duration::hours(1);
            }
        }
        hours
    }

    /// Where each day of the track lies along x: from its midnight (or the track's start) to the
    /// next midnight (or the track's end). The day after the last one, which only lends its early
    /// hours to the last night, is left out.
    pub fn days(&self) -> Vec<(NaiveDate, f32, f32)> {
        self.segments
            .iter()
            .filter_map(|segment| match segment.part {
                Part::Day(day) => Some(day),
                Part::Night { .. } => None,
            })
            .map(|day| {
                let midnight = day.and_time(NaiveTime::MIN);
                let next = midnight + Duration::days(1);
                (day, self.x_of(midnight.max(self.start)), self.x_of(next.min(self.end)))
            })
            .collect()
    }

    /// The midnights inside the track with their x, where a strong line parts the days: the one
    /// after each day, in its night or (without one, or with a night after midnight) in the day.
    pub fn midnights(&self) -> Vec<f32> {
        self.segments
            .iter()
            .filter_map(|segment| match segment.part {
                Part::Day(day) => Some((day + Duration::days(1)).and_time(NaiveTime::MIN)),
                Part::Night { .. } => None,
            })
            .filter(|midnight| self.start < *midnight && *midnight < self.end)
            .map(|midnight| self.x_of(midnight))
            .collect()
    }

    /// Where the part of `day` starts: the end of the night before it (06:00). Days beside the
    /// track are counted on from its start.
    fn day_start(&self, day: NaiveDate) -> NaiveDateTime {
        self.start + Duration::days((day - self.start.date()).num_days())
    }

    /// Where the part of `day` ends: the start of the night after it (22:00), or the start of the
    /// next day without a night.
    fn day_end(&self, day: NaiveDate) -> NaiveDateTime {
        let length = self.segments.iter().find(|s| matches!(s.part, Part::Day(_))).map(Segment::minutes);
        self.day_start(day) + Duration::minutes(length.unwrap_or(24 * 60))
    }

    /// Where the line of the working `hours` runs along x (task K.33): `(from, to)` for each
    /// stretch of them on the track, also of the day before the first, whose hours may reach into
    /// it. Parts in a folded night are left out, so the line ends at the edge of the band; parts in
    /// an unfolded night run on with the day. Empty without working hours.
    pub fn working(&self, hours: WorkingHours) -> Vec<(f32, f32)> {
        if hours.is_none() {
            return Vec::new();
        }
        let mut lines: Vec<(f32, f32)> = Vec::new();
        let mut day = self.start.date() - Duration::days(1);
        while day <= self.end.date() {
            let (from, to) = (at_minutes(day, hours.from.minutes()), at_minutes(day, hours.to.minutes()));
            for segment in self.segments.iter().filter(|segment| !segment.folded()) {
                let (start, end) = (from.max(segment.start), to.min(segment.end));
                if start >= end {
                    continue;
                }
                let (left, right) = (segment.x_of(start), segment.x_of(end));
                // A day and the unfolded night beside it meet without a gap: one line, not two.
                match lines.last_mut() {
                    Some(last) if (last.1 - left).abs() < 0.01 => last.1 = right,
                    _ => lines.push((left, right)),
                }
            }
            day += Duration::days(1);
        }
        lines
    }

    /// The scroll offset that puts `focus` at [`FOCUS_AT`] of a viewport `viewport` wide, kept to
    /// what the scrollable can reach.
    pub fn focus_offset(&self, focus: NaiveDateTime, viewport: f32) -> f32 {
        (self.x_of(focus) - FOCUS_AT * viewport).clamp(0.0, (self.width - viewport).max(0.0))
    }
}

/// The rows across the canvas, top to bottom: day labels, hour labels, all-day chips, the event
/// columns, and "3 h free" at the bottom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rows {
    /// The height of the canvas.
    pub height: f32,
    /// Rows of all-day chips ([`chip_rows`]); one is kept even without chips.
    pub chip_rows: usize,
}

impl Rows {
    /// The hour labels: `(top, bottom)`.
    pub fn axis(&self) -> (f32, f32) {
        (day_head(), day_head() + axis_row())
    }

    /// The line of the working hours (task K.33): `(top, bottom)`, the lowest [`GAP`] of the hour
    /// row, so it runs along the hours without touching their labels or the chips below.
    pub fn working(&self) -> (f32, f32) {
        let bottom = self.axis().1;
        (bottom - GAP, bottom)
    }

    /// The all-day chips: `(top, bottom)`.
    pub fn all_day(&self) -> (f32, f32) {
        let top = self.axis().1;
        (top, top + self.chip_rows.max(1) as f32 * chip_pitch() - (chip_pitch() - CHIP_HEIGHT) + 2.0 * STRIP_PAD)
    }

    /// The event columns: `(top, bottom)`, at most [`max_lanes_height()`] high, never upside down in
    /// a very low window.
    pub fn lanes(&self) -> (f32, f32) {
        let top = self.all_day().1 + row_gap();
        let bottom = (self.height - free_row() - row_gap()).min(top + max_lanes_height());
        (top, bottom.max(top + MIN_EVENT_WIDTH))
    }

    /// The row of the free-time notes, right under the event columns: `(top, bottom)`.
    pub fn free(&self) -> (f32, f32) {
        let bottom = self.lanes().1;
        (bottom + row_gap(), bottom + free_row())
    }

    /// The top of the chip in row `row` of the all-day strip.
    fn chip_top(&self, row: usize) -> f32 {
        self.all_day().0 + STRIP_PAD + row as f32 * chip_pitch()
    }
}

/// A timed entry on the track; `id` is the caller's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timed {
    pub id: usize,
    pub start: NaiveDateTime,
    /// Exclusive; equal to `start` for an entry without length.
    pub end: NaiveDateTime,
}

/// The part of `timed` shown as a column: kept to the track, and without what lies in a folded
/// night at either end (that part is a dot in the band). `None` when nothing is left: an entry
/// wholly inside a folded night, or beside the track.
pub fn visible_span(track: &Track, timed: &Timed) -> Option<(NaiveDateTime, NaiveDateTime)> {
    let start = timed.start.max(track.start);
    let end = timed.end.min(track.end);
    if timed.start >= track.end || (timed.end <= track.start && timed.end > timed.start) || end < start {
        return None;
    }
    let start = track.folded_night_of(start).map_or(start, |night| night.end);
    let ends_in_night = track.segments.iter().find(|s| s.folded() && s.start < end && end <= s.end);
    let end = ends_in_night.map_or(end, |night| night.start);
    let cut_away = start == end && timed.end > timed.start;
    (start <= end && !cut_away).then_some((start, end))
}

/// An event column or an all-day chip as drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub id: usize,
    pub rect: Rectangle,
}

/// Places the timed entries as columns over their time. Overlapping entries share the height of
/// the column area in lanes (the first lane on top); a column ends [`GAP`] short of its end time
/// and is at least [`MIN_EVENT_WIDTH`] wide.
pub fn blocks(track: &Track, rows: &Rows, timed: &[Timed]) -> Vec<Block> {
    let shown: Vec<(usize, NaiveDateTime, NaiveDateTime)> =
        timed.iter().filter_map(|t| visible_span(track, t).map(|(start, end)| (t.id, start, end))).collect();
    let intervals: Vec<(f32, f32)> = shown
        .iter()
        .map(|&(_, start, end)| (minutes_between(track.start, start), minutes_between(track.start, end)))
        .collect();
    let (top, bottom) = rows.lanes();
    shown
        .iter()
        .zip(assign_lanes(&intervals))
        .map(|(&(id, start, end), lane)| {
            let x = track.x_of(start);
            let right = track.x_of(end).max(x + MIN_EVENT_WIDTH) - GAP;
            let pitch = (bottom - top) / lane.lanes as f32;
            let y = top + lane.lane as f32 * pitch;
            Block { id, rect: Rectangle::new(Point::new(x, y), Size::new(right - x, pitch - LANE_GAP)) }
        })
        .collect()
}

/// An event starting in a folded night, as a dot in its band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dot {
    pub id: usize,
    pub evening: NaiveDate,
    pub center: Point,
}

/// The dots of the entries that start in a folded night: beside the band's label, from top to
/// bottom by their start.
pub fn dots(track: &Track, rows: &Rows, timed: &[Timed]) -> Vec<Dot> {
    let band = rows.height - day_head();
    timed
        .iter()
        .filter_map(|t| {
            let night = track.folded_night_of(t.start)?;
            let evening = night.evening()?;
            let fraction = minutes_between(night.start, t.start) / minutes_between(night.start, night.end);
            let y = day_head() + band * (DOTS_FROM + fraction * DOTS_SPAN);
            Some(Dot { id: t.id, evening, center: Point::new(night.right() - DOT_INSET, y) })
        })
        .collect()
}

/// An all-day or multi-day entry over the days `[first, after)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub id: usize,
    pub first: NaiveDate,
    pub after: NaiveDate,
}

impl Span {
    /// The days an entry from `start` to `end` covers (an end at midnight does not count its day).
    pub fn of(id: usize, start: NaiveDateTime, end: NaiveDateTime) -> Span {
        let (first, after) = all_day_days(start, end);
        Span { id, first, after }
    }
}

fn span_intervals(spans: &[Span], origin: NaiveDate) -> Vec<(f32, f32)> {
    spans.iter().map(|s| ((s.first - origin).num_days() as f32, (s.after - origin).num_days() as f32)).collect()
}

/// Rows the all-day chips need: overlapping spans go into rows of their own.
pub fn chip_rows(spans: &[Span]) -> usize {
    let origin = spans.iter().map(|s| s.first).min().unwrap_or_default();
    assign_lanes(&span_intervals(spans, origin)).iter().map(|lane| lane.lanes).max().unwrap_or(0)
}

/// Places the all-day chips: from the start of their first day's part to the end of their last
/// day's part (06:00 to 22:00), so a chip over several days runs on through the nights between
/// them and ends where its last night begins. Chips beside the track are left out.
pub fn chips(track: &Track, rows: &Rows, spans: &[Span]) -> Vec<Block> {
    let origin = spans.iter().map(|s| s.first).min().unwrap_or_default();
    spans
        .iter()
        .zip(assign_lanes(&span_intervals(spans, origin)))
        .filter_map(|(span, lane)| {
            let x = track.x_of(track.day_start(span.first)) + GAP;
            let right = track.x_of(track.day_end(span.after - Duration::days(1))) - 2.0 * GAP;
            let rect = Rectangle::new(Point::new(x, rows.chip_top(lane.lane)), Size::new(right - x, CHIP_HEIGHT));
            (rect.width >= 1.0).then_some(Block { id: span.id, rect })
        })
        .collect()
}

/// Free time worth a note in the days of the track (never in a night): the gaps between the
/// entries of a day that last at least `min_minutes`, from `now` on; days gone by have none.
/// As in the week, the morning before the first entry and the evening after the last are not
/// "free".
pub fn free_gaps(
    track: &Track,
    timed: &[Timed],
    now: NaiveDateTime,
    min_minutes: f32,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let mut found = Vec::new();
    for day in track.segments.iter().filter(|s| matches!(s.part, Part::Day(_)) && s.end > now) {
        let midnight = day.start.date().and_time(NaiveTime::MIN);
        let busy: Vec<TimedSegment> = timed
            .iter()
            .filter(|t| t.end > day.start && t.start < day.end)
            .map(|t| TimedSegment {
                id: t.id,
                column: 0,
                start: minutes_between(midnight, t.start.max(day.start)),
                end: minutes_between(midnight, t.end.min(day.end)),
            })
            .collect();
        let from = minutes_between(midnight, now.max(day.start));
        for (start, end) in gaps_in_day(&busy, 0, from, min_minutes) {
            let at = |minutes: f32| midnight + Duration::seconds((minutes * 60.0).round() as i64);
            found.push((at(start), at(end)));
        }
    }
    found
}

/// The moon of a folded night's band, as a click target [`moon_target_size()`] square around its icon:
/// the icon stands at the bottom of the band's label, which is `label` long (moon, gap and text,
/// turned to read upwards) and centred in the band below the day labels.
pub fn moon(night: &Segment, rows: &Rows, label: f32) -> Rectangle {
    let center = Point::new(night.x + night.width / 2.0, day_head() + (rows.height - day_head()) / 2.0);
    let bottom = (center.y + label / 2.0).round();
    let icon = Point::new(center.x, bottom - icon::SIZE_SM / 2.0);
    Rectangle::new(
        Point::new(icon.x - moon_target_size() / 2.0, icon.y - moon_target_size() / 2.0),
        Size::new(moon_target_size(), moon_target_size()),
    )
}

/// The "Collapse night" button of an unfolded night, `width` wide (its label decides), at the
/// top left of the night in the all-day row.
pub fn fold_button(night: &Segment, rows: &Rows, width: f32) -> Rectangle {
    Rectangle::new(Point::new(night.x + space::xs(), rows.chip_top(0)), Size::new(width, CHIP_HEIGHT))
}

/// The outline of a folded night's band `rect`: straight sides, top and bottom edges in teeth
/// about [`TOOTH_WIDTH`] wide and [`TOOTH_DEPTH`] deep, mirrored, clockwise from the top left.
pub fn zigzag(rect: Rectangle) -> Vec<Point> {
    let teeth = ((rect.width / TOOTH_WIDTH).round() as usize).max(2);
    let x = |k: usize| rect.x + k as f32 / teeth as f32 * rect.width;
    let depth = |k: usize| if k.is_multiple_of(2) { TOOTH_DEPTH } else { 0.0 };
    let top = (0..=teeth).map(|k| Point::new(x(k), rect.y + depth(k)));
    let bottom = (0..=teeth).rev().map(|k| Point::new(x(k), rect.y + rect.height - depth(k)));
    top.chain(bottom).collect()
}

/// Where a label `width` wide stands in a day from `from` to `to` when the viewport starts at
/// `offset`: at the day's start, or at the viewport's left edge while the day runs on beyond it,
/// but never past the day's end.
pub fn sticky_x(from: f32, to: f32, offset: f32, width: f32) -> f32 {
    offset.clamp(from, (to - width).max(from))
}

/// What a click on the timeline means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// The column of the timed entry with this id.
    Event(usize),
    /// The all-day chip of the entry with this id.
    Chip(usize),
    /// The band of the folded night after this evening: unfold it.
    Unfold(NaiveDate),
    /// The moon in the band of the folded night after this evening: a click unfolds it like the
    /// band, a double-click opens the night's card.
    Moon(NaiveDate),
    /// "Collapse night" of the unfolded night after this evening.
    Fold(NaiveDate),
    /// Free time at this moment (rounded down to [`SLOT_MINUTES`]): a new event.
    Slot(NaiveDateTime),
    /// The all-day row of this day: a new all-day event.
    AllDay(NaiveDate),
}

/// Hit-tests `point` on the canvas. "Collapse night" buttons come first, then the moons
/// ([`moon`]), then the columns and the chips (the later drawn win), then the folded nights, then
/// empty space: the all-day row and the columns below it. The day labels and the hour labels do
/// nothing.
pub fn hit(
    track: &Track,
    rows: &Rows,
    blocks: &[Block],
    chips: &[Block],
    folds: &[(NaiveDate, Rectangle)],
    moons: &[(NaiveDate, Rectangle)],
    point: Point,
) -> Option<Hit> {
    if let Some(&(evening, _)) = folds.iter().find(|(_, rect)| rect.contains(point)) {
        return Some(Hit::Fold(evening));
    }
    if let Some(&(evening, _)) = moons.iter().find(|(_, rect)| rect.contains(point)) {
        return Some(Hit::Moon(evening));
    }
    if let Some(block) = blocks.iter().rev().find(|b| b.rect.contains(point)) {
        return Some(Hit::Event(block.id));
    }
    if let Some(chip) = chips.iter().rev().find(|c| c.rect.contains(point)) {
        return Some(Hit::Chip(chip.id));
    }
    let segment = track.segment_at(point.x)?;
    if point.y < day_head() || point.y >= rows.height {
        return None;
    }
    if segment.folded() {
        return segment.evening().map(Hit::Unfold);
    }
    if point.y < rows.axis().1 {
        return None;
    }
    let at = segment.time_at(point.x);
    if point.y < rows.all_day().1 {
        return Some(Hit::AllDay(at.date()));
    }
    Some(Hit::Slot(slot_of(at)))
}

/// `at` rounded down to [`SLOT_MINUTES`].
fn slot_of(at: NaiveDateTime) -> NaiveDateTime {
    let minutes = at.time().num_seconds_from_midnight() / 60;
    let slot = minutes / SLOT_MINUTES * SLOT_MINUTES;
    at.date().and_time(NaiveTime::from_hms_opt(slot / 60, slot % 60, 0).unwrap_or(NaiveTime::MIN))
}

#[cfg(test)]
mod tests {
    use torro_core::settings::Quarter;

    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("date")
    }

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        d(day).and_hms_opt(hour, minute, 0).expect("time")
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    const DAY: f32 = 16.0 * HOUR_WIDTH;

    /// Mon 12 to Tue 13 October, both nights folded, or the night after `open`.
    fn track(open: &[u32]) -> Track {
        track_with(open, Night::DEFAULT)
    }

    fn track_with(open: &[u32], night: Night) -> Track {
        let open: BTreeSet<NaiveDate> = open.iter().map(|&day| d(day)).collect();
        Track::new(d(12), d(14), &open, night)
    }

    fn night(from: (u32, u32), to: (u32, u32)) -> Night {
        let quarter = |(hour, minute)| Quarter::new(hour, minute).expect("quarter");
        Night { from: quarter(from), to: quarter(to) }
    }

    fn rows() -> Rows {
        Rows { height: 600.0, chip_rows: 1 }
    }

    #[test]
    fn days_and_folded_nights_alternate_along_the_track() {
        let track = track(&[]);
        assert_eq!(track.segments.len(), 4);
        assert_eq!(track.segments[0].part, Part::Day(d(12)));
        assert_eq!(track.segments[1].part, Part::Night { evening: d(12), open: false });
        assert_eq!((track.start, track.end), (at(12, 6, 0), at(14, 6, 0)));
        assert!(close(track.width, 2.0 * (DAY + NIGHT_WIDTH)));
        assert_eq!(track.segments[1].minutes(), 8 * 60);
    }

    #[test]
    fn time_maps_to_x_with_a_folded_night() {
        let track = track(&[]);
        assert!(close(track.x_of(at(12, 6, 0)), 0.0));
        assert!(close(track.x_of(at(12, 10, 30)), 4.5 * HOUR_WIDTH));
        assert!(close(track.x_of(at(12, 22, 0)), DAY), "the night starts where the day ends");
        assert!(close(track.x_of(at(13, 2, 0)), DAY + NIGHT_WIDTH / 2.0), "midnight + 2 h: half the band");
        assert!(close(track.x_of(at(13, 6, 0)), DAY + NIGHT_WIDTH));
        assert!(close(track.x_of(at(13, 14, 20)), DAY + NIGHT_WIDTH + (8.0 + 20.0 / 60.0) * HOUR_WIDTH));
        assert!(close(track.x_of(at(1, 0, 0)), 0.0), "before the track");
        assert!(close(track.x_of(at(20, 0, 0)), track.width), "after the track");
    }

    #[test]
    fn time_maps_to_x_with_an_unfolded_night() {
        let track = track(&[12]);
        assert_eq!(track.segments[1].part, Part::Night { evening: d(12), open: true });
        assert!(close(track.x_of(at(13, 2, 0)), DAY + 4.0 * HOUR_WIDTH), "the night runs at the day's pace");
        assert!(close(track.x_of(at(13, 6, 0)), DAY + 8.0 * HOUR_WIDTH));
        assert!(close(track.width, 2.0 * DAY + 8.0 * HOUR_WIDTH + NIGHT_WIDTH));
        let hours: Vec<NaiveDateTime> = track.hours().into_iter().map(|(hour, _)| hour).collect();
        assert!(hours.contains(&at(13, 2, 0)), "an unfolded night has hour lines");
        assert!(!hours.contains(&at(14, 2, 0)), "a folded one has none");
        assert_eq!(hours.len(), 16 + 8 + 16);
    }

    #[test]
    fn x_maps_back_to_time_in_open_parts() {
        let track = track(&[12]);
        let rows = rows();
        let y = rows.lanes().0 + 10.0;
        let slot = |x: f32| hit(&track, &rows, &[], &[], &[], &[], Point::new(x, y));
        assert_eq!(slot(4.5 * HOUR_WIDTH + 10.0), Some(Hit::Slot(at(12, 10, 30))), "10:40 → 10:30");
        assert_eq!(slot(4.5 * HOUR_WIDTH + 14.5), Some(Hit::Slot(at(12, 10, 45))));
        assert_eq!(slot(DAY + 3.0 * HOUR_WIDTH), Some(Hit::Slot(at(13, 1, 0))), "after midnight, next day");
    }

    #[test]
    fn days_and_midnights_lie_where_their_times_do() {
        let track = track(&[]);
        let days = track.days();
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].0, d(12));
        assert!(close(days[0].1, 0.0) && close(days[0].2, DAY + NIGHT_WIDTH / 4.0), "{days:?}");
        let midnights = track.midnights();
        assert_eq!(midnights.len(), 2, "after each day, the last inside its night");
        assert!(close(midnights[0], DAY + NIGHT_WIDTH / 4.0));
    }

    #[test]
    fn the_focus_lands_at_thirty_percent_and_stays_reachable() {
        let track = track(&[]);
        let now = at(13, 14, 20);
        let offset = track.focus_offset(now, 600.0);
        assert!(close(track.x_of(now) - offset, 180.0));
        assert_eq!(track.focus_offset(at(12, 6, 0), 600.0), 0.0, "not before the start");
        assert!(close(track.focus_offset(at(14, 5, 0), 600.0), track.width - 600.0), "not past the end");
    }

    fn hours(from: (u32, u32), to: (u32, u32)) -> WorkingHours {
        let quarter = |(hour, minute)| Quarter::new(hour, minute).expect("quarter");
        WorkingHours { from: quarter(from), to: quarter(to) }
    }

    /// Task K.33: one line per day from the start to the end of the working hours.
    #[test]
    fn the_working_line_runs_from_start_to_end_of_each_day() {
        let track = track(&[]);
        let lines = track.working(hours((7, 30), (16, 0)));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(close(lines[0].0, 1.5 * HOUR_WIDTH) && close(lines[0].1, 10.0 * HOUR_WIDTH), "{lines:?}");
        assert!(close(lines[1].0, track.x_of(at(13, 7, 30))) && close(lines[1].1, track.x_of(at(13, 16, 0))));
        let rows = rows();
        assert_eq!(rows.working(), (rows.axis().1 - GAP, rows.axis().1));
        assert!(track.working(hours((9, 0), (9, 0))).is_empty(), "no working hours, no line");
    }

    /// Working hours from 05:00 reach into the night 22:00–06:00: folded, the line starts at the
    /// right edge of the band (or at the start of the track on the first day); unfolded, at 05:00.
    #[test]
    fn the_working_line_ends_at_a_folded_night() {
        let early = hours((5, 0), (9, 0));
        let folded = track(&[]).working(early);
        assert_eq!(folded.len(), 2, "{folded:?}");
        assert!(close(folded[0].0, 0.0) && close(folded[0].1, 3.0 * HOUR_WIDTH), "{folded:?}");
        assert!(close(folded[1].0, DAY + NIGHT_WIDTH), "the band's right edge: {folded:?}");
        assert!(close(folded[1].1, DAY + NIGHT_WIDTH + 3.0 * HOUR_WIDTH));
        let open = track(&[12]);
        let unfolded = open.working(early);
        assert_eq!(unfolded.len(), 2, "{unfolded:?}");
        assert!(close(unfolded[1].0, open.x_of(at(13, 5, 0))), "{unfolded:?}");
        assert!(close(unfolded[1].0, DAY + 7.0 * HOUR_WIDTH), "inside the unfolded night");
        assert!(close(unfolded[1].1, open.x_of(at(13, 9, 0))), "one line across the morning");
    }

    fn timed(id: usize, start: NaiveDateTime, end: NaiveDateTime) -> Timed {
        Timed { id, start, end }
    }

    #[test]
    fn overlapping_events_share_the_height() {
        let track = track(&[]);
        let rows = rows();
        let blocks = blocks(
            &track,
            &rows,
            &[
                timed(0, at(13, 14, 0), at(13, 15, 0)),
                timed(1, at(13, 14, 30), at(13, 15, 0)),
                timed(2, at(13, 16, 0), at(13, 17, 0)),
            ],
        );
        let (top, bottom) = rows.lanes();
        let (a, b, c) = (blocks[0].rect, blocks[1].rect, blocks[2].rect);
        assert!(close(a.y, top) && close(b.y, top + (bottom - top) / 2.0), "two lanes, one above the other");
        assert!(close(a.height, b.height));
        assert!(close(c.y, top) && close(c.height, bottom - top - LANE_GAP), "alone: the full height");
        assert!(close(a.width, HOUR_WIDTH - GAP));
        assert!(close(a.x, track.x_of(at(13, 14, 0))));
    }

    #[test]
    fn short_events_keep_a_minimum_width() {
        let track = track(&[]);
        let blocks = blocks(&track, &rows(), &[timed(0, at(13, 9, 30), at(13, 9, 30))]);
        assert!(close(blocks[0].rect.width, MIN_EVENT_WIDTH - GAP));
    }

    #[test]
    fn events_in_a_folded_night_become_dots() {
        let track = track(&[]);
        let rows = rows();
        let release = timed(0, at(12, 22, 30), at(13, 0, 30));
        let late = timed(1, at(12, 21, 0), at(12, 23, 0));
        let early = timed(2, at(13, 5, 0), at(13, 7, 0));
        assert_eq!(visible_span(&track, &release), None, "wholly in the night");
        assert_eq!(visible_span(&track, &late), Some((at(12, 21, 0), at(12, 22, 0))), "cut at the band");
        assert_eq!(visible_span(&track, &early), Some((at(13, 6, 0), at(13, 7, 0))), "from the band on");
        let dots = dots(&track, &rows, &[release, late, early]);
        assert_eq!(dots.iter().map(|dot| (dot.id, dot.evening)).collect::<Vec<_>>(), [(0, d(12)), (2, d(12))]);
        assert!(dots[0].center.y < dots[1].center.y, "by their start, top to bottom");
        assert!(track.segments[1].x < dots[0].center.x && dots[0].center.x < track.segments[1].right());
        let unfolded = self::track(&[12]);
        assert!(visible_span(&unfolded, &release).is_some(), "an unfolded night shows it as a column");
        assert!(super::dots(&unfolded, &rows, &[release]).is_empty());
    }

    #[test]
    fn chips_run_through_the_nights_and_end_where_the_last_night_begins() {
        let track = track(&[]);
        let rows = rows();
        let spans = [Span { id: 0, first: d(12), after: d(14) }, Span { id: 1, first: d(13), after: d(14) }];
        assert_eq!(chip_rows(&spans), 2);
        let chips = chips(&track, &rows, &spans);
        assert!(close(chips[0].rect.x, GAP));
        assert!(close(chips[0].rect.x + chips[0].rect.width, track.x_of(at(13, 22, 0)) - 2.0 * GAP));
        assert!(chips[1].rect.y > chips[0].rect.y, "the overlapping one goes below");
        let beside = super::chips(&track, &rows, &[Span { id: 2, first: d(1), after: d(3) }]);
        assert!(beside.is_empty());
        assert_eq!(Span::of(3, at(12, 0, 0), at(13, 0, 0)), Span { id: 3, first: d(12), after: d(13) });
    }

    #[test]
    fn the_all_day_row_grows_with_its_chips() {
        // The standard's day head and free row are its buttons (36, issue #291), the hour row 24.
        let one = Rows { height: 600.0, chip_rows: 0 };
        assert_eq!(one.all_day(), (60.0, 88.0), "one row is kept");
        let two = Rows { chip_rows: 2, ..one };
        assert_eq!(two.all_day(), (60.0, 112.0));
        assert!(two.lanes().0 > one.lanes().0);
        assert_eq!(one.lanes(), (92.0, 92.0 + max_lanes_height()), "columns stop at their maximum height");
        assert_eq!(one.free(), (480.0, 512.0), "free time right under the columns");
        let low = Rows { height: 300.0, ..one };
        assert_eq!(low.lanes(), (92.0, 260.0));
        assert_eq!(low.free(), (264.0, 296.0));
    }

    #[test]
    fn free_time_is_between_events_in_the_days_from_now_on() {
        let track = track(&[]);
        let events = [
            timed(0, at(12, 9, 0), at(12, 10, 0)),
            timed(1, at(12, 15, 0), at(12, 16, 0)),
            timed(2, at(13, 9, 30), at(13, 9, 45)),
            timed(3, at(13, 13, 0), at(13, 15, 0)),
            timed(4, at(13, 18, 30), at(13, 19, 30)),
            timed(5, at(13, 23, 0), at(14, 1, 0)),
        ];
        let now = at(13, 14, 20);
        let gaps = free_gaps(&track, &events, now, 120.0);
        // Monday is over; Tuesday 09:45–13:00 is past, 15:00–18:30 is 3½ h ahead. The evening up to
        // the night event is not free time.
        assert_eq!(gaps, [(at(13, 15, 0), at(13, 18, 30))]);
        let earlier = free_gaps(&track, &events, at(12, 8, 0), 120.0);
        assert_eq!(earlier.first(), Some(&(at(12, 10, 0), at(12, 15, 0))));
    }

    #[test]
    fn hits_find_events_nights_buttons_and_free_time() {
        let track = track(&[13]);
        let rows = rows();
        let events = [timed(7, at(12, 10, 0), at(12, 11, 0))];
        let blocks = blocks(&track, &rows, &events);
        let chips = chips(&track, &rows, &[Span { id: 9, first: d(13), after: d(14) }]);
        let night = track.segments[3];
        let folds = [(d(13), fold_button(&night, &rows, 100.0))];
        let moons = [(d(12), moon(&track.segments[1], &rows, 100.0))];
        let find = |x: f32, y: f32| hit(&track, &rows, &blocks, &chips, &folds, &moons, Point::new(x, y));
        let block = blocks[0].rect;
        assert_eq!(find(block.center_x(), block.center_y()), Some(Hit::Event(7)));
        assert_eq!(find(chips[0].rect.center_x(), chips[0].rect.center_y()), Some(Hit::Chip(9)));
        assert_eq!(find(folds[0].1.center_x(), folds[0].1.center_y()), Some(Hit::Fold(d(13))));
        let band = track.segments[1];
        assert_eq!(find(band.x + 10.0, 200.0), Some(Hit::Unfold(d(12))), "anywhere in the band");
        let moon = moons[0].1;
        assert_eq!(find(moon.center_x(), moon.center_y()), Some(Hit::Moon(d(12))), "the moon in it");
        assert!(moon.width >= moon_target_size() && moon.x >= band.x - moon_target_size(), "a target of 24");
        assert_eq!(find(band.x + 10.0, day_head() + 2.0), Some(Hit::Unfold(d(12))), "up to the axis row");
        assert_eq!(find(band.x + 10.0, 10.0), None, "not in the day labels");
        assert_eq!(find(2.0 * HOUR_WIDTH + 1.0, 300.0), Some(Hit::Slot(at(12, 8, 0))));
        assert_eq!(find(2.0 * HOUR_WIDTH, rows.all_day().0 + 5.0), Some(Hit::AllDay(d(12))));
        assert_eq!(find(2.0 * HOUR_WIDTH, rows.axis().0 + 5.0), None, "the hour labels do nothing");
        assert_eq!(find(track.width + 5.0, 300.0), None);
        assert_eq!(find(night.x + 3.0 * HOUR_WIDTH, 400.0), Some(Hit::Slot(at(14, 1, 0))), "unfolded night");
    }

    #[test]
    fn the_teeth_of_a_folded_night_are_mirrored() {
        let points = zigzag(Rectangle::new(Point::new(100.0, 28.0), Size::new(NIGHT_WIDTH, 500.0)));
        assert_eq!(points.len(), 2 * 7, "six teeth across 48, both edges");
        assert_eq!(points[0], Point::new(100.0, 28.0 + TOOTH_DEPTH));
        assert_eq!(points[1], Point::new(108.0, 28.0));
        assert_eq!(points[13], Point::new(100.0, 528.0 - TOOTH_DEPTH));
        assert!(points.iter().all(|p| (100.0..=148.0).contains(&p.x) && (28.0..=528.0).contains(&p.y)));
    }

    #[test]
    fn day_labels_stick_to_the_left_edge_while_their_day_is_in_view() {
        assert_eq!(sticky_x(0.0, 1000.0, -5.0, 80.0), 0.0, "at the day's start");
        assert_eq!(sticky_x(0.0, 1000.0, 400.0, 80.0), 400.0, "at the viewport's edge");
        assert_eq!(sticky_x(0.0, 1000.0, 990.0, 80.0), 920.0, "pushed out by the next day");
        assert_eq!(sticky_x(0.0, 50.0, 20.0, 80.0), 0.0, "a day narrower than its label");
    }

    #[test]
    fn another_night_moves_the_segments() {
        let track = track_with(&[], night((23, 30), (7, 0)));
        assert_eq!((track.start, track.end), (at(12, 7, 0), at(14, 7, 0)));
        let (day, band) = (track.segments[0], track.segments[1]);
        assert_eq!((day.start, day.end), (at(12, 7, 0), at(12, 23, 30)));
        assert!(close(day.width, 16.5 * HOUR_WIDTH));
        assert_eq!((band.start, band.end), (at(12, 23, 30), at(13, 7, 0)));
        assert_eq!(band.minutes(), 7 * 60 + 30);
        assert!(close(track.x_of(at(12, 23, 0)), 16.0 * HOUR_WIDTH), "23:00 is still day");
        assert!(track.folded_night_of(at(13, 6, 30)).is_some(), "06:30 is still night");
        let open = track_with(&[12], night((23, 30), (7, 0)));
        assert!(close(open.segments[1].width, 7.5 * HOUR_WIDTH), "unfolded at the day's pace");
        let midnights = track.midnights();
        assert!(close(midnights[0], 16.5 * HOUR_WIDTH + NIGHT_WIDTH / 15.0), "half an hour into the band");
        let chip = chips(&track, &rows(), &[Span { id: 0, first: d(12), after: d(13) }]);
        assert!(close(chip[0].rect.x + chip[0].rect.width, track.x_of(at(12, 23, 30)) - 2.0 * GAP));
    }

    #[test]
    fn a_night_after_midnight_leaves_the_midnight_in_the_day() {
        let track = track_with(&[], night((1, 0), (9, 0)));
        assert_eq!((track.segments[0].start, track.segments[0].end), (at(12, 9, 0), at(13, 1, 0)));
        assert_eq!((track.segments[1].start, track.segments[1].end), (at(13, 1, 0), at(13, 9, 0)));
        assert!(close(track.midnights()[0], 15.0 * HOUR_WIDTH), "midnight inside the day");
    }

    #[test]
    fn without_a_night_the_days_are_whole_and_nothing_folds() {
        let track = track_with(&[12], Night::none_at(Quarter::new(6, 0).expect("quarter")));
        assert_eq!(track.segments.len(), 2, "two days, no nights");
        assert!(track.segments.iter().all(|s| matches!(s.part, Part::Day(_))));
        assert!(close(track.width, 2.0 * 24.0 * HOUR_WIDTH));
        assert_eq!(track.folded_night_of(at(13, 2, 0)), None);
        assert_eq!(track.midnights().len(), 2);
        let late = timed(0, at(12, 23, 0), at(13, 1, 0));
        assert_eq!(visible_span(&track, &late), Some((at(12, 23, 0), at(13, 1, 0))), "a column, no dot");
        assert!(dots(&track, &rows(), &[late]).is_empty());
        let chip = chips(&track, &rows(), &[Span { id: 0, first: d(12), after: d(13) }]);
        assert!(close(chip[0].rect.x + chip[0].rect.width, 24.0 * HOUR_WIDTH - 2.0 * GAP));
    }

    #[test]
    fn free_time_leaves_out_the_night_it_is_given() {
        let events = [timed(0, at(13, 9, 0), at(13, 10, 0)), timed(1, at(13, 18, 0), at(13, 19, 0))];
        let now = at(13, 8, 0);
        let late = track_with(&[], night((23, 30), (7, 0)));
        assert_eq!(free_gaps(&late, &events, now, 120.0), [(at(13, 10, 0), at(13, 18, 0))]);
        // Without a night the evening is still not "free": nothing comes after it in the day.
        let none = track_with(&[], Night::none_at(Quarter::new(6, 0).expect("quarter")));
        assert_eq!(free_gaps(&none, &events, now, 120.0), [(at(13, 10, 0), at(13, 18, 0))]);
    }

    #[test]
    fn two_quick_presses_in_one_place_are_a_double_click() {
        let start = Instant::now();
        let point = Point::new(100.0, 200.0);
        let later = |millis: u64| start + std::time::Duration::from_millis(millis);
        assert!(is_double_click((start, point), (later(250), Point::new(103.0, 198.0))));
        assert!(is_double_click((start, point), (later(400), point)), "up to the limit");
        assert!(!is_double_click((start, point), (later(450), point)), "too slow");
        assert!(!is_double_click((start, point), (later(100), Point::new(120.0, 200.0))), "elsewhere");
        assert!(!is_double_click((later(100), point), (start, point)), "not backwards in time");
    }
}
