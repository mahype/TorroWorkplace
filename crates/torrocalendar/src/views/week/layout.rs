//! Geometry of the day and week view, free of Iced widgets so that it can be tested: time to
//! y and back, splitting entries at midnight, lanes for overlapping entries, the all-day
//! rows, and hit-testing. `week.rs` draws the result.
//!
//! Coordinates are relative to the top left corner of the canvas they are drawn on. Columns
//! are `[axis][day 0][day 1]…`; the time axis is [`axis_width`] wide, the day columns share
//! the rest equally.

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use iced::{Point, Rectangle, Size};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{size, space};

/// Height of one hour in the time grid.
pub fn hour_height() -> f32 {
    size::hour()
}
/// Height of the whole time grid, 24 hours.
pub fn grid_height() -> f32 {
    24.0 * hour_height()
}
/// Shortest timed block, so that a zero-length entry stays visible and clickable: one line of
/// `MICRO` text with a little air.
pub const MIN_EVENT_HEIGHT: f32 = 18.0;
/// Pitch of the rows in the all-day strip; a chip is this less [`GAP`] high (22, as
/// `calendar.html`'s `.chip`).
pub fn all_day_row_height() -> f32 {
    space::xl()
}
/// Padding above and below the chips in the all-day strip (`.allday > div{padding:4px 2px}`).
fn strip_padding() -> f32 {
    space::xs()
}
/// Clicks in the grid round the time down to a multiple of this many minutes.
pub const SLOT_MINUTES: u32 = 30;

/// Gap between blocks and around them, in pixels (`.ev{left:2px;right:4px}`).
const GAP: f32 = 2.0;
const MINUTES_PER_DAY: f32 = 24.0 * 60.0;

/// Position of an interval among the intervals it overlaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
    /// Lane index, 0 is the leftmost.
    pub lane: usize,
    /// Lanes of the whole cluster of overlapping intervals. Every member of a cluster gets
    /// the same count, so the cluster uses the full width together.
    pub lanes: usize,
}

/// Greedy lane assignment for `(start, end)` intervals. Intervals are taken by start (longer
/// first on ties) and go into the first lane that is free at their start. A cluster is a run
/// of intervals that overlap each other. Intervals that only touch do not overlap.
pub fn assign_lanes(intervals: &[(f32, f32)]) -> Vec<Lane> {
    let mut order: Vec<usize> = (0..intervals.len()).collect();
    order.sort_by(|&a, &b| {
        intervals[a].0.total_cmp(&intervals[b].0).then(intervals[b].1.total_cmp(&intervals[a].1)).then(a.cmp(&b))
    });

    let mut lanes = vec![Lane { lane: 0, lanes: 1 }; intervals.len()];
    // End of the last interval in each lane of the current cluster.
    let mut lane_ends: Vec<f32> = Vec::new();
    let mut cluster: Vec<usize> = Vec::new();
    let mut cluster_end = f32::NEG_INFINITY;
    for i in order {
        let (start, end) = intervals[i];
        if start >= cluster_end {
            close_cluster(&mut lanes, &cluster, lane_ends.len());
            cluster.clear();
            lane_ends.clear();
        }
        let lane = match lane_ends.iter().position(|&free_from| free_from <= start) {
            Some(lane) => {
                lane_ends[lane] = end;
                lane
            }
            None => {
                lane_ends.push(end);
                lane_ends.len() - 1
            }
        };
        lanes[i].lane = lane;
        cluster.push(i);
        cluster_end = cluster_end.max(end);
    }
    close_cluster(&mut lanes, &cluster, lane_ends.len());
    lanes
}

fn close_cluster(lanes: &mut [Lane], cluster: &[usize], count: usize) {
    for &i in cluster {
        lanes[i].lanes = count;
    }
}

/// Minutes since local midnight.
pub fn minutes_of(time: NaiveTime) -> f32 {
    time.num_seconds_from_midnight() as f32 / 60.0
}

fn minutes_to_y(minutes: f32) -> f32 {
    minutes / 60.0 * hour_height()
}

/// The y coordinate of a time of day in the grid.
pub fn y_of(time: NaiveTime) -> f32 {
    minutes_to_y(minutes_of(time))
}

/// The time of day at a y coordinate, rounded down to [`SLOT_MINUTES`] and kept inside the
/// day (`23:30` at most).
pub fn slot_at(y: f32) -> NaiveTime {
    let minutes = (y / hour_height() * 60.0).clamp(0.0, MINUTES_PER_DAY);
    let step = SLOT_MINUTES as f32;
    let slot = ((minutes / step).floor() * step).min(MINUTES_PER_DAY - step) as u32;
    NaiveTime::from_hms_opt(slot / 60, slot % 60, 0).unwrap_or(NaiveTime::MIN)
}

/// A piece of a timed entry on one day column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimedSegment {
    /// The caller's id of the entry.
    pub id: usize,
    pub column: usize,
    /// Minutes since midnight of that column's day.
    pub start: f32,
    pub end: f32,
}

/// Splits a timed entry `[start, end)` at midnight into one segment per day column. Days
/// outside the `columns` days from `first` are left out. An entry with `end == start` gets a
/// zero-length segment on its start day.
pub fn split_by_day(
    id: usize,
    first: NaiveDate,
    columns: usize,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Vec<TimedSegment> {
    let end = end.max(start);
    let mut out = Vec::new();
    let mut day = start.date();
    loop {
        let day_start = day.and_time(NaiveTime::MIN);
        let day_end = day_start + Duration::days(1);
        let from = start.max(day_start);
        let to = end.min(day_end);
        let column = (day - first).num_days();
        let on_start_day = day == start.date();
        if column >= 0 && (column as usize) < columns && (to > from || on_start_day) {
            out.push(TimedSegment {
                id,
                column: column as usize,
                start: minutes_between(day_start, from),
                end: minutes_between(day_start, to),
            });
        }
        if end <= day_end {
            break;
        }
        day += Duration::days(1);
    }
    out
}

fn minutes_between(from: NaiveDateTime, to: NaiveDateTime) -> f32 {
    (to - from).num_seconds() as f32 / 60.0
}

/// The days `[first, after)` an entry from `start` to `end` covers, for entries in the
/// all-day strip. An end at exactly midnight does not count its own day, and an entry always
/// covers at least one day.
pub fn all_day_days(start: NaiveDateTime, end: NaiveDateTime) -> (NaiveDate, NaiveDate) {
    let first = start.date();
    let mut after = if end.time() == NaiveTime::MIN { end.date() } else { end.date() + Duration::days(1) };
    if after <= first {
        after = first + Duration::days(1);
    }
    (first, after)
}

/// A bar in the all-day strip, as column indices `[first, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaySpan {
    pub id: usize,
    pub first: usize,
    pub end: usize,
}

/// The bar of an all-day or multi-day entry, clipped to the `columns` days from `first`.
/// `None` when it does not touch them.
pub fn day_span(
    id: usize,
    first: NaiveDate,
    columns: usize,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Option<DaySpan> {
    let (from, after) = all_day_days(start, end);
    let limit = columns as i64;
    let first_column = (from - first).num_days().clamp(0, limit) as usize;
    let end_column = (after - first).num_days().clamp(0, limit) as usize;
    (end_column > first_column).then_some(DaySpan { id, first: first_column, end: end_column })
}

/// Width of the time axis for a grid `total_width` wide: [`size::axis_narrow()`] in the narrow
/// (and mini) layout, otherwise [`size::axis()`].
///
/// The grid only knows its own width, not the window's. In the narrow and mini layout the content
/// is flush with the window, so its width decides the class just as the window's would; in the
/// medium and wide layout it is always wider than [`layout::NARROW_MAX_WIDTH`] less the sidebar.
/// The height plays no part here, hence the unbounded one.
pub fn axis_width(total_width: f32) -> f32 {
    match layout::of(Size::new(total_width, f32::INFINITY)) {
        SizeClass::Mini | SizeClass::Narrow => size::axis_narrow(),
        SizeClass::Strip | SizeClass::Medium | SizeClass::Wide => size::axis(),
    }
}

/// Horizontal split: `(x of the first day column, width of a day column)`.
pub fn columns(total_width: f32, count: usize) -> (f32, f32) {
    let count = count.max(1) as f32;
    let axis = axis_width(total_width);
    (axis, (total_width - axis).max(0.0) / count)
}

/// A timed block as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedBlock {
    pub id: usize,
    pub rect: Rectangle,
}

/// Places the timed segments: each day column is split into lanes for its overlapping
/// segments. A block ends [`GAP`] above its end time, so back-to-back entries stay apart;
/// blocks shorter than [`MIN_EVENT_HEIGHT`] are drawn that tall.
pub fn timed_blocks(segments: &[TimedSegment], count: usize, total_width: f32) -> Vec<TimedBlock> {
    let (x0, col_w) = columns(total_width, count);
    let mut blocks = Vec::with_capacity(segments.len());
    for column in 0..count {
        let members: Vec<&TimedSegment> = segments.iter().filter(|s| s.column == column).collect();
        let intervals: Vec<(f32, f32)> = members.iter().map(|s| (s.start, s.end)).collect();
        for (segment, lane) in members.iter().zip(assign_lanes(&intervals)) {
            let lane_width = (col_w - 2.0 * GAP) / lane.lanes as f32;
            let x = x0 + column as f32 * col_w + GAP + lane.lane as f32 * lane_width;
            let y = minutes_to_y(segment.start);
            let height = (minutes_to_y(segment.end) - y - GAP).max(MIN_EVENT_HEIGHT);
            blocks.push(TimedBlock {
                id: segment.id,
                rect: Rectangle::new(Point::new(x, y), Size::new(lane_width - GAP, height)),
            });
        }
    }
    blocks
}

/// A bar as drawn in the all-day strip.
#[derive(Debug, Clone, PartialEq)]
pub struct BarBlock {
    pub id: usize,
    pub rect: Rectangle,
}

/// Number of rows the all-day bars need.
pub fn strip_rows(spans: &[DaySpan]) -> usize {
    assign_lanes(&span_intervals(spans)).iter().map(|lane| lane.lanes).max().unwrap_or(0)
}

fn span_intervals(spans: &[DaySpan]) -> Vec<(f32, f32)> {
    spans.iter().map(|s| (s.first as f32, s.end as f32)).collect()
}

/// Places the all-day bars in rows, like the timed lanes. Returns the bars and the row count.
pub fn bar_blocks(spans: &[DaySpan], count: usize, total_width: f32) -> (Vec<BarBlock>, usize) {
    let (x0, col_w) = columns(total_width, count);
    let lanes = assign_lanes(&span_intervals(spans));
    let rows = lanes.iter().map(|lane| lane.lanes).max().unwrap_or(0);
    let blocks = spans
        .iter()
        .zip(lanes)
        .map(|(span, lane)| {
            let x = x0 + span.first as f32 * col_w + GAP;
            let width = (span.end - span.first) as f32 * col_w - 2.0 * GAP;
            let y = strip_padding() + lane.lane as f32 * all_day_row_height();
            let height = all_day_row_height() - GAP;
            BarBlock { id: span.id, rect: Rectangle::new(Point::new(x, y), Size::new(width, height)) }
        })
        .collect();
    (blocks, rows)
}

/// Height of the all-day strip with `rows` rows. At least one row, so empty days stay
/// clickable.
pub fn strip_height(rows: usize) -> f32 {
    rows.max(1) as f32 * all_day_row_height() - GAP + 2.0 * strip_padding()
}

/// Free time in a day column: the gaps between the timed segments of `column` that last at
/// least `min_minutes`, as `(start, end)` in minutes since midnight.
///
/// Only gaps *between* entries count: the time before the first and after the last entry of a
/// day is not "free", it is the morning and the evening. A gap is cut to start no earlier than
/// `from` (now, for today; `0.0` for days to come), so time that has passed is never offered.
/// Overlapping and touching segments form one busy stretch.
pub fn free_gaps(segments: &[TimedSegment], column: usize, from: f32, min_minutes: f32) -> Vec<(f32, f32)> {
    let mut busy: Vec<(f32, f32)> =
        segments.iter().filter(|s| s.column == column).map(|s| (s.start, s.end.max(s.start))).collect();
    busy.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut gaps = Vec::new();
    let mut busy_until: Option<f32> = None;
    for (start, end) in busy {
        if let Some(until) = busy_until {
            let gap_start = until.max(from);
            if start - gap_start >= min_minutes {
                gaps.push((gap_start, start));
            }
        }
        busy_until = Some(busy_until.map_or(end, |until| until.max(end)));
    }
    gaps
}

/// The y range of a span in minutes, for drawing something in a gap.
pub fn y_range(start: f32, end: f32) -> (f32, f32) {
    (minutes_to_y(start), minutes_to_y(end))
}

/// The day column at `x`, `None` in the gutter or outside the columns.
pub fn column_at(x: f32, count: usize, total_width: f32) -> Option<usize> {
    let (x0, col_w) = columns(total_width, count);
    if x < x0 || col_w <= 0.0 {
        return None;
    }
    let column = ((x - x0) / col_w) as usize;
    (column < count.max(1)).then_some(column)
}

/// What a click in the time grid means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridHit {
    /// The block of the timed segment with this id.
    Event(usize),
    /// Empty space at this day column and time.
    Slot { column: usize, time: NaiveTime },
}

/// Hit-tests a point in the time grid. Blocks win over the empty space behind them; later
/// blocks are drawn on top and win over earlier ones.
pub fn grid_hit(blocks: &[TimedBlock], count: usize, total_width: f32, point: Point) -> Option<GridHit> {
    if let Some(block) = blocks.iter().rev().find(|b| b.rect.contains(point)) {
        return Some(GridHit::Event(block.id));
    }
    let column = column_at(point.x, count, total_width)?;
    if !(0.0..grid_height()).contains(&point.y) {
        return None;
    }
    Some(GridHit::Slot { column, time: slot_at(point.y) })
}

/// What a click in the all-day strip means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripHit {
    /// The bar of the span with this id.
    Bar(usize),
    /// Empty space in this day column.
    Day(usize),
}

/// Hit-tests a point in the all-day strip.
pub fn strip_hit(bars: &[BarBlock], count: usize, total_width: f32, point: Point) -> Option<StripHit> {
    if let Some(bar) = bars.iter().rev().find(|b| b.rect.contains(point)) {
        return Some(StripHit::Bar(bar.id));
    }
    column_at(point.x, count, total_width).map(StripHit::Day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        d(day).and_hms_opt(hour, minute, 0).unwrap()
    }

    fn t(hour: u32, minute: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, 0).unwrap()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn touching_intervals_get_one_lane() {
        let lanes = assign_lanes(&[(0.0, 60.0), (60.0, 120.0)]);
        assert_eq!(lanes, vec![Lane { lane: 0, lanes: 1 }; 2]);
    }

    #[test]
    fn overlapping_intervals_split_into_lanes() {
        let lanes = assign_lanes(&[(0.0, 60.0), (30.0, 90.0)]);
        assert_eq!(lanes, vec![Lane { lane: 0, lanes: 2 }, Lane { lane: 1, lanes: 2 }]);
    }

    #[test]
    fn cluster_lane_count_is_shared_but_separate_clusters_are_not() {
        let lanes = assign_lanes(&[(0.0, 60.0), (30.0, 90.0), (120.0, 180.0)]);
        assert_eq!(lanes[0], Lane { lane: 0, lanes: 2 });
        assert_eq!(lanes[1], Lane { lane: 1, lanes: 2 });
        assert_eq!(lanes[2], Lane { lane: 0, lanes: 1 });
    }

    #[test]
    fn a_free_lane_is_reused_in_a_chain() {
        // A ends where C starts, so C takes A's lane and B keeps its own.
        let lanes = assign_lanes(&[(0.0, 60.0), (30.0, 90.0), (60.0, 120.0)]);
        assert_eq!(lanes.iter().map(|l| l.lane).collect::<Vec<_>>(), vec![0, 1, 0]);
        assert!(lanes.iter().all(|l| l.lanes == 2));
    }

    #[test]
    fn time_maps_to_y_and_back() {
        assert!(close(y_of(t(7, 0)), 336.0));
        assert!(close(y_of(t(12, 30)), 600.0));
        assert_eq!(slot_at(0.0), t(0, 0));
        assert_eq!(slot_at(23.9), t(0, 0));
        assert_eq!(slot_at(24.0), t(0, 30));
        assert_eq!(slot_at(336.0), t(7, 0));
        assert_eq!(slot_at(-10.0), t(0, 0));
        assert_eq!(slot_at(grid_height() * 2.0), t(23, 30));
    }

    #[test]
    fn entry_over_midnight_is_split_into_two_days() {
        let segments = split_by_day(3, d(12), 7, at(12, 22, 0), at(13, 2, 0));
        assert_eq!(
            segments,
            vec![
                TimedSegment { id: 3, column: 0, start: 1320.0, end: 1440.0 },
                TimedSegment { id: 3, column: 1, start: 0.0, end: 120.0 },
            ]
        );
    }

    #[test]
    fn entry_ending_at_midnight_stays_on_its_day() {
        let segments = split_by_day(0, d(12), 7, at(12, 10, 0), at(13, 0, 0));
        assert_eq!(segments, vec![TimedSegment { id: 0, column: 0, start: 600.0, end: 1440.0 }]);
    }

    #[test]
    fn part_before_the_shown_days_is_dropped() {
        let segments = split_by_day(0, d(13), 7, at(12, 23, 0), at(13, 1, 0));
        assert_eq!(segments, vec![TimedSegment { id: 0, column: 0, start: 0.0, end: 60.0 }]);
    }

    #[test]
    fn zero_length_entry_keeps_one_segment() {
        let segments = split_by_day(0, d(12), 1, at(12, 10, 0), at(12, 10, 0));
        assert_eq!(segments, vec![TimedSegment { id: 0, column: 0, start: 600.0, end: 600.0 }]);
    }

    #[test]
    fn all_day_days_are_exclusive_at_the_end() {
        assert_eq!(all_day_days(at(12, 0, 0), at(14, 0, 0)), (d(12), d(14)));
        assert_eq!(all_day_days(at(12, 10, 0), at(13, 10, 0)), (d(12), d(14)));
        assert_eq!(all_day_days(at(12, 0, 0), at(13, 0, 0)), (d(12), d(13)));
        assert_eq!(all_day_days(at(12, 0, 0), at(12, 0, 0)), (d(12), d(13)));
    }

    #[test]
    fn day_span_is_clipped_to_the_shown_days() {
        let span = day_span(1, d(12), 7, at(10, 0, 0), at(14, 0, 0));
        assert_eq!(span, Some(DaySpan { id: 1, first: 0, end: 2 }));
        assert_eq!(day_span(2, d(12), 7, at(1, 0, 0), at(5, 0, 0)), None);
    }

    #[test]
    fn timed_blocks_sit_side_by_side_with_equal_width() {
        let segments = [
            TimedSegment { id: 0, column: 0, start: 600.0, end: 660.0 },
            TimedSegment { id: 1, column: 0, start: 630.0, end: 690.0 },
        ];
        let blocks = timed_blocks(&segments, 7, 500.0);
        let (a, b) = (&blocks[0].rect, &blocks[1].rect);
        assert!(close(a.width, b.width));
        assert!(a.x < b.x);
        assert!(close(a.y, 480.0));
        assert!(close(a.height, 46.0), "an hour less the gap to the next block");
    }

    #[test]
    fn back_to_back_blocks_do_not_touch() {
        let segments = [
            TimedSegment { id: 0, column: 0, start: 600.0, end: 660.0 },
            TimedSegment { id: 1, column: 0, start: 660.0, end: 720.0 },
        ];
        let blocks = timed_blocks(&segments, 1, 500.0);
        assert!(blocks[0].rect.y + blocks[0].rect.height < blocks[1].rect.y);
    }

    #[test]
    fn short_timed_block_gets_minimum_height() {
        let segments = [TimedSegment { id: 0, column: 2, start: 600.0, end: 600.0 }];
        let blocks = timed_blocks(&segments, 7, 500.0);
        assert!(close(blocks[0].rect.height, MIN_EVENT_HEIGHT));
    }

    #[test]
    fn grid_hit_finds_events_and_rounded_slots() {
        let segments = [TimedSegment { id: 7, column: 0, start: 600.0, end: 660.0 }];
        let blocks = timed_blocks(&segments, 7, 500.0);
        let inside = Point::new(60.0, 500.0);
        assert_eq!(grid_hit(&blocks, 7, 500.0, inside), Some(GridHit::Event(7)));
        // y = 50 is 01:02, rounded down to 01:00, in column 1 (narrow axis 44, columns 65 wide).
        let empty = Point::new(44.0 + 65.0 + 10.0, 50.0);
        assert_eq!(grid_hit(&blocks, 7, 500.0, empty), Some(GridHit::Slot { column: 1, time: t(1, 0) }));
        assert_eq!(grid_hit(&blocks, 7, 500.0, Point::new(10.0, 50.0)), None);
        assert_eq!(grid_hit(&blocks, 7, 500.0, Point::new(100.0, grid_height() + 5.0)), None);
    }

    #[test]
    fn column_at_ignores_the_axis() {
        assert_eq!(column_at(51.9, 7, 1000.0), None);
        assert_eq!(column_at(52.0, 7, 1000.0), Some(0));
        assert_eq!(column_at(999.0, 7, 1000.0), Some(6));
        assert_eq!(column_at(1000.0, 7, 1000.0), None);
        assert_eq!(column_at(43.9, 1, 500.0), None);
        assert_eq!(column_at(44.0, 1, 500.0), Some(0));
    }

    #[test]
    fn the_axis_is_narrower_in_the_narrow_layout() {
        assert_eq!(axis_width(420.0), size::axis_narrow());
        assert_eq!(axis_width(300.0), size::axis_narrow(), "mini");
        assert_eq!(axis_width(600.0), size::axis());
        assert_eq!(axis_width(1200.0), size::axis());
        assert_eq!(columns(1000.0, 7), (size::axis(), (1000.0 - size::axis()) / 7.0));
    }

    #[test]
    fn strip_rows_are_chips_with_padding() {
        // One row: 4 padding, a 22-high chip, 4 padding.
        assert!(close(strip_height(0), 30.0));
        assert!(close(strip_height(1), 30.0));
        assert!(close(strip_height(2), 54.0));
        let (bars, _) = bar_blocks(&[DaySpan { id: 0, first: 0, end: 1 }], 7, 1000.0);
        assert!(close(bars[0].rect.y, 4.0));
        assert!(close(bars[0].rect.height, 22.0));
    }

    fn seg(column: usize, start: f32, end: f32) -> TimedSegment {
        TimedSegment { id: 0, column, start, end }
    }

    #[test]
    fn free_gaps_are_between_entries_only() {
        // 09:30–09:45, 15:00–16:00: one gap of 5¼ h; morning and evening are not "free".
        let segments = [seg(0, 570.0, 585.0), seg(0, 900.0, 960.0), seg(1, 600.0, 700.0)];
        assert_eq!(free_gaps(&segments, 0, 0.0, 120.0), vec![(585.0, 900.0)]);
        assert_eq!(free_gaps(&segments, 1, 0.0, 120.0), vec![], "a single entry has no gap");
        assert_eq!(free_gaps(&[], 0, 0.0, 120.0), vec![]);
    }

    #[test]
    fn free_gaps_skip_short_gaps_and_merge_overlaps() {
        let segments = [seg(0, 600.0, 720.0), seg(0, 660.0, 690.0), seg(0, 780.0, 840.0), seg(0, 1000.0, 1060.0)];
        // 12:00–13:00 is one hour; 14:00–16:40 is long enough. The inner entry does not end the
        // busy stretch early.
        assert_eq!(free_gaps(&segments, 0, 0.0, 120.0), vec![(840.0, 1000.0)]);
    }

    #[test]
    fn free_gaps_start_no_earlier_than_now() {
        let segments = [seg(0, 600.0, 660.0), seg(0, 1020.0, 1080.0)];
        // Now 14:20: 14:20–17:00 is still 2 h 40 min free.
        assert_eq!(free_gaps(&segments, 0, 860.0, 120.0), vec![(860.0, 1020.0)]);
        // Now 15:30: only 1½ h left, not worth a note.
        assert_eq!(free_gaps(&segments, 0, 930.0, 120.0), vec![]);
        // After the gap: nothing.
        assert_eq!(free_gaps(&segments, 0, 1050.0, 120.0), vec![]);
    }

    #[test]
    fn strip_hit_finds_bars_and_days() {
        let spans = [DaySpan { id: 4, first: 0, end: 1 }];
        let (bars, rows) = bar_blocks(&spans, 7, 500.0);
        assert_eq!(rows, 1);
        assert_eq!(strip_hit(&bars, 7, 500.0, Point::new(100.0, 10.0)), Some(StripHit::Bar(4)));
        assert_eq!(strip_hit(&bars, 7, 500.0, Point::new(100.0, 30.0)), Some(StripHit::Day(0)));
        assert_eq!(strip_hit(&bars, 7, 500.0, Point::new(300.0, 10.0)), Some(StripHit::Day(3)));
    }

    #[test]
    fn overlapping_bars_go_to_separate_rows() {
        let spans = [DaySpan { id: 0, first: 0, end: 3 }, DaySpan { id: 1, first: 1, end: 2 }];
        assert_eq!(strip_rows(&spans), 2);
        let (bars, _) = bar_blocks(&spans, 7, 500.0);
        assert!(bars[1].rect.y > bars[0].rect.y);
    }
}
