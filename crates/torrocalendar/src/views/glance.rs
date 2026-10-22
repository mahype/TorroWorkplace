//! Mini and strip: the calendar as a glance at a small tile (task K.19 in
//! `docs/tasks/kalender-aufgaben.md`; design skill `.claude/skills/torro-design`, section 6).
//!
//! - **Mini** ([`mini`]): the date at the top, then what matters now – the running event with
//!   "ends in 19 min", or the next one with its start time in `DISPLAY` and "in 1 h 20 min" – and
//!   as many of today's other events as fit. While nothing runs the top line says "Free until
//!   14:00"; when the day is done it says "Nothing else today" and the next event after today
//!   leads instead, its day ("Tomorrow") above its time. A tall tile fills the room left below
//!   with the days after, each under its name ([`later_days`]).
//! - **Strip** ([`strip`]): today as a horizontal timeline on a canvas – timed events as tinted
//!   blocks with their colour bar (a block too narrow for its title has it beside, on the free
//!   track: [`title_spots`]), all-day ones as chips on the left, the now line in red. It spans
//!   today's events and now, at least 08:00–18:00.
//!
//! Both are displays, not small apps: no buttons, forms or dialogs (a larger tile is the tiling
//! manager's job). They are rebuilt only with the app's minute tick and on new data.
//!
//! A glance shows what happens: cancelled events are left out ([`entries`], task TE.9), so they
//! never run, never lead and never end "Free until". The same goes for the day dots of the week
//! strip and the mini radars, which take their entries from here.
//!
//! The decisions – what runs, what comes next, the texts, the strip's hours and lanes, how much
//! fits – are plain functions with tests; the views only lay out their results.

use std::cmp::Reverse;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use iced::alignment::Vertical;
use iced::border::{self, Radius};
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::text::{Alignment as TextAlign, Wrapping};
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Background, Color, Element, Length, Padding, Point, Rectangle, Size, Theme, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_time, to_local};
use torro_gui::layout::shown_rows;
use torro_gui::theme::{self, Palette, mix, over, palette, radius, size, space};
use torro_gui::typo::{self, ellipsize, line_height, text_width};
use torro_gui::{style, widgets};
use torro_pim::{Event as CalendarEvent, EventTime};

use super::{Item, ViewData, short_date, takes_time};
use crate::app::Message;

const UNTITLED: &str = "(untitled)";
/// Between the parts of a line: "in 20 min · Room 3".
const SEPARATOR: &str = " · ";
/// The strip shows at least this working day (hours), whatever the events.
const DAY_START: u32 = 8;
const DAY_END: u32 = 18;
/// The colour dot in front of a title in the mini tile.
fn dot() -> f32 {
    space::s()
}
/// Width of the colour bar on the left of event blocks and chips (as in the week view).
const BAR: f32 = 3.0;
/// Text inside blocks starts this far from their left edge: the bar plus 6 (as in the week view).
const TEXT_LEFT: f32 = 9.0;
/// Space kept free at the right of the text in blocks.
const TEXT_RIGHT: f32 = 6.0;
/// Space above the first line of text in a block.
const TEXT_TOP: f32 = 4.0;
/// Gap between blocks in the strip, so that back-to-back events stay apart.
const GAP: f32 = 2.0;
/// A block is never narrower than this, so a short event stays visible.
const MIN_BLOCK: f32 = 4.0;
/// Room one hour label of the strip needs ("08:00" in `MICRO` and air): as wide as the narrow
/// time axis of the week view.
fn label_room() -> f32 {
    size::axis_narrow()
}
/// Past blocks are drawn at this opacity over the panel (as in the week view).
const PAST_OPACITY: f32 = 0.55;
/// The time label of the now line: the axis figures at 500 on a 16-high pill, as in the week view.
fn now_label() -> typo::Level {
    typo::Level { font: theme::font_tabular_medium(), line_height: typo::caption().line_height, ..typo::micro() }
}
/// Width of the now label: "23:59" with 3 on each side (as in the week view).
fn now_label_width() -> f32 {
    size::axis_narrow() - 2.0 * space::xs()
}
/// Radius of the dot at the top of the now line (7 across).
const NOW_DOT: f32 = 3.5;
/// A title is readable with at least this many of its letters before the "…" (all of a shorter
/// one), as in the week view and the radar: "Da…" says nothing.
const MIN_TITLE_LETTERS: usize = 4;
/// Gap between a block and its title beside it on the free track.
fn beside_gap() -> f32 {
    space::xs()
}
/// A hundredth of a point against rounding in measured widths ([`readable_width`]).
const WIDTH_SLACK: f32 = 0.01;

/// An event as the glance sees it: local times, title, place and calendar colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub title: String,
    pub location: Option<String>,
    pub color: Color,
    /// Local start.
    pub start: NaiveDateTime,
    /// Local (exclusive) end, never before `start`.
    pub end: NaiveDateTime,
    /// A date-only event, or one lasting a day or more: a chip, never "running".
    pub all_day: bool,
}

impl Entry {
    /// The entry for one item of the view data. An empty title becomes "(untitled)".
    pub fn new(item: &Item<'_>) -> Entry {
        let event: &CalendarEvent = item.event;
        let start = local(event.start);
        let end = event.end.map_or(start, local).max(start);
        let all_day = event.all_day() || end - start >= Duration::hours(24);
        let summary = event.summary.trim();
        let title = if summary.is_empty() { tr(UNTITLED).to_string() } else { summary.to_string() };
        let location = event.location.as_deref().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string);
        Entry { title, location, color: item.color, start, end, all_day }
    }

    /// Whether the entry touches `day`: it overlaps its 24 hours, or it has no length and starts
    /// on it. An all-day entry ending at midnight does not touch the day after.
    pub fn on(&self, day: NaiveDate) -> bool {
        let from = day.and_time(NaiveTime::MIN);
        let to = from + Duration::days(1);
        if self.end == self.start { from <= self.start && self.start < to } else { self.start < to && self.end > from }
    }

    /// A timed entry that has begun and not ended at `now`.
    fn running(&self, now: NaiveDateTime) -> bool {
        !self.all_day && self.start <= now && now < self.end
    }
}

fn local(time: EventTime) -> NaiveDateTime {
    match time {
        EventTime::Date(day) => day.and_time(NaiveTime::MIN),
        EventTime::DateTime(at) => to_local(at),
    }
}

/// The entries of `items`, by start (then end, then title). Cancelled events are left out: they
/// keep no time busy ([`takes_time`]), and a tile read from a distance has no room for what does
/// not happen.
pub fn entries(items: &[Item<'_>]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = items.iter().filter(|item| takes_time(item.event.status)).map(Entry::new).collect();
    entries.sort_by(|a, b| (a.start, a.end, &a.title).cmp(&(b.start, b.end, &b.title)));
    entries
}

/// What a glance leads with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus<'a> {
    /// A timed event running now. Of several, the one that began last (the meeting just joined),
    /// and of those the one ending first.
    Now(&'a Entry),
    /// Nothing runs; the next timed event that starts later today.
    Next(&'a Entry),
    /// Nothing else timed today; the first event of the next day that has any – its first timed
    /// event, or its first all-day one if it has none.
    Later(&'a Entry),
    /// Nothing to come in the loaded days.
    Nothing,
}

/// The [`Focus`] of `entries` at `now`.
pub fn focus(entries: &[Entry], now: NaiveDateTime) -> Focus<'_> {
    let today = now.date();
    if let Some(entry) = entries.iter().filter(|e| e.running(now)).max_by_key(|e| (e.start, Reverse(e.end))) {
        return Focus::Now(entry);
    }
    if let Some(entry) =
        entries.iter().filter(|e| !e.all_day && e.start > now && e.start.date() == today).min_by_key(|e| e.start)
    {
        return Focus::Next(entry);
    }
    let Some(day) = entries.iter().map(|e| e.start.date()).filter(|d| *d > today).min() else {
        return Focus::Nothing;
    };
    let on_day = || entries.iter().filter(move |e| e.start.date() == day);
    match on_day().filter(|e| !e.all_day).min_by_key(|e| e.start).or_else(|| on_day().next()) {
        Some(entry) => Focus::Later(entry),
        None => Focus::Nothing,
    }
}

/// The events under the focus: the other timed events of its day that have not ended, by start –
/// today for a running or next event, the later day for [`Focus::Later`]. All-day events are not
/// among them; today's stand under the date ([`all_day_on`]).
pub fn rest<'a>(entries: &'a [Entry], now: NaiveDateTime, focus: Focus<'a>) -> Vec<&'a Entry> {
    let (lead, day) = match focus {
        Focus::Now(entry) | Focus::Next(entry) => (entry, now.date()),
        Focus::Later(entry) => (entry, entry.start.date()),
        Focus::Nothing => return Vec::new(),
    };
    entries.iter().filter(|e| !e.all_day && e.on(day) && e.end > now && !std::ptr::eq(*e, lead)).collect()
}

/// The all-day events touching `day`, in the order of `entries`.
pub fn all_day_on(entries: &[Entry], day: NaiveDate) -> Vec<&Entry> {
    entries.iter().filter(|e| e.all_day && e.on(day)).collect()
}

/// The quiet line about the day: "Free until 14:00" while nothing runs and something comes
/// today; "Nothing else today" when nothing timed is left; none while an event runs.
pub fn headline(focus: Focus<'_>) -> Option<String> {
    match focus {
        Focus::Next(entry) => Some(trf("Free until {time}", &[("time", &format_time(entry.start.time()))])),
        Focus::Later(_) | Focus::Nothing => Some(tr("Nothing else today").to_string()),
        Focus::Now(_) => None,
    }
}

/// When the focus happens, seen from `now`: "ends in 19 min", "in 1 h 20 min", and for a later
/// day "Tomorrow" or its short date ("Tue, 13 Oct").
pub fn when(focus: Focus<'_>, now: NaiveDateTime) -> Option<String> {
    match focus {
        Focus::Now(entry) => Some(trf("ends in {span}", &[("span", &span(minutes_until(now, entry.end)))])),
        Focus::Next(entry) => Some(trf("in {span}", &[("span", &span(minutes_until(now, entry.start)))])),
        Focus::Later(entry) => Some(if entry.start.date() == now.date() + Duration::days(1) {
            tr("Tomorrow").to_string()
        } else {
            short_date(entry.start.date())
        }),
        Focus::Nothing => None,
    }
}

/// The line above the big figure: the day of a [`Focus::Later`] ("Tomorrow", "Tue, 13 Oct"),
/// which reads before its time; the other foci happen today and have none.
pub fn day_above(focus: Focus<'_>, now: NaiveDateTime) -> Option<String> {
    match focus {
        Focus::Later(_) => when(focus, now),
        _ => None,
    }
}

/// The detail under the title of the lead: when ("ends in 19 min", "in 1 h 20 min") for an
/// event today, then the place; for a later day only the place, its day stands above the time.
fn detail_below(focus: Focus<'_>, entry: &Entry, now: NaiveDateTime) -> String {
    let when = if day_above(focus, now).is_some() { None } else { when(focus, now) };
    [when, entry.location.clone()].into_iter().flatten().collect::<Vec<_>>().join(SEPARATOR)
}

/// Days after the lead's for a tall mini tile: per day its label and events (all-day first, then
/// by start), as many as `rows` rows hold, a day's label counting as a row. A day is only begun
/// when its label and one event fit; the last one may be cut short. Days come from `entries` as
/// loaded (today and the weeks after), after `after`.
pub fn later_days(entries: &[Entry], after: NaiveDate, rows: usize) -> Vec<(NaiveDate, Vec<&Entry>)> {
    let mut days: Vec<NaiveDate> = entries.iter().map(|e| e.start.date()).filter(|d| *d > after).collect();
    days.sort();
    days.dedup();
    let mut left = rows;
    let mut out = Vec::new();
    for day in days {
        if left < 2 {
            break;
        }
        let mut on_day: Vec<&Entry> = entries.iter().filter(|e| e.on(day)).collect();
        on_day.sort_by_key(|e| (!e.all_day, e.start));
        let take = on_day.len().min(left - 1);
        on_day.truncate(take);
        left -= take + 1;
        out.push((day, on_day));
    }
    out
}

/// The time at the right of a row on a later `day`: its start, "until 00:30" for an event that
/// began the day before, none for an all-day event.
fn later_time(entry: &Entry, day: NaiveDate) -> Option<String> {
    if entry.all_day {
        None
    } else if entry.start.date() < day {
        Some(trf("until {time}", &[("time", &format_time(entry.end.time()))]))
    } else {
        Some(format_time(entry.start.time()))
    }
}

/// The name of a later day: "Tomorrow" seen from `today`, else its short date.
fn day_name(day: NaiveDate, today: NaiveDate) -> String {
    if day == today + Duration::days(1) { tr("Tomorrow").to_string() } else { short_date(day) }
}

/// The big figure of the mini tile: "Now" for a running event, otherwise the start time of the
/// focus ("All day" for an all-day one).
pub fn display(focus: Focus<'_>) -> Option<String> {
    match focus {
        Focus::Now(_) => Some(tr("Now").to_string()),
        Focus::Next(entry) | Focus::Later(entry) if entry.all_day => Some(tr("All day").to_string()),
        Focus::Next(entry) | Focus::Later(entry) => Some(format_time(entry.start.time())),
        Focus::Nothing => None,
    }
}

/// Whole minutes from `now` to `at`, rounded up: 30 seconds left are "1 min", not "0 min".
fn minutes_until(now: NaiveDateTime, at: NaiveDateTime) -> i64 {
    let seconds = (at - now).num_seconds().max(0);
    (seconds + 59) / 60
}

/// A duration in minutes as "19 min", "2 h" or "1 h 20 min".
pub fn span(minutes: i64) -> String {
    let (hours, minutes) = (minutes / 60, minutes % 60);
    match (hours, minutes) {
        (0, minutes) => trf("{n} min", &[("n", &minutes)]),
        (hours, 0) => trf("{n} h", &[("n", &hours)]),
        (hours, minutes) => trf("{h} h {m} min", &[("h", &hours), ("m", &minutes)]),
    }
}

/// The hours the strip spans, `[first, last)`: today's timed events and now, at least
/// 08:00–18:00, never beyond the day.
pub fn hours(entries: &[Entry], now: NaiveDateTime) -> (u32, u32) {
    let today = now.date();
    let (mut first, mut last) = (DAY_START.min(now.hour()), DAY_END.max(now.hour() + 1));
    for entry in entries.iter().filter(|e| !e.all_day && e.on(today)) {
        let (from, to) = minutes_today(entry, today);
        first = first.min((from / 60.0).floor() as u32);
        last = last.max((to / 60.0).ceil() as u32);
    }
    (first, last.clamp(first + 1, 24))
}

/// Start and end of `entry` in minutes since the midnight of `today`, cut to that day.
fn minutes_today(entry: &Entry, today: NaiveDate) -> (f32, f32) {
    let midnight = today.and_time(NaiveTime::MIN);
    let minutes = |at: NaiveDateTime| (at - midnight).num_seconds().clamp(0, 24 * 3600) as f32 / 60.0;
    (minutes(entry.start), minutes(entry.end))
}

/// Lanes for overlapping spans `(start, end)`: per span its lane and the number of lanes of its
/// group of overlapping spans. Spans that only touch (one ends when the next starts) share a lane.
pub fn lanes(spans: &[(f32, f32)]) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by(|&a, &b| spans[a].0.total_cmp(&spans[b].0).then(spans[a].1.total_cmp(&spans[b].1)));
    let mut out = vec![(0, 1); spans.len()];
    let mut group: Vec<usize> = Vec::new();
    // Where each lane of the current group is free again.
    let mut lane_ends: Vec<f32> = Vec::new();
    let mut group_end = f32::NEG_INFINITY;
    let close = |group: &mut Vec<usize>, lanes: usize, out: &mut Vec<(usize, usize)>| {
        for &i in group.iter() {
            out[i].1 = lanes;
        }
        group.clear();
    };
    for i in order {
        let (start, end) = spans[i];
        if start >= group_end && !group.is_empty() {
            close(&mut group, lane_ends.len(), &mut out);
            lane_ends.clear();
        }
        let lane = match lane_ends.iter().position(|&free| free <= start) {
            Some(lane) => lane,
            None => {
                lane_ends.push(f32::NEG_INFINITY);
                lane_ends.len() - 1
            }
        };
        lane_ends[lane] = end;
        out[i].0 = lane;
        group.push(i);
        group_end = if group.len() == 1 { end } else { group_end.max(end) };
    }
    close(&mut group, lane_ends.len(), &mut out);
    out
}

/// Where the title of a block in the strip goes (task K.34).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TitleSpot {
    /// In the block, with the time range and the place below, each cut with "…" to the block.
    Inside,
    /// Right of a block too narrow for it, on the free track of its lane: the text starts at `x`
    /// and is cut with "…" to `width`.
    Beside { x: f32, width: f32 },
    /// Neither in nor beside the block is room for a readable title: the bare block.
    Bare,
}

/// The width of the shortest readable form of `title` in `CAPTION`: its first
/// [`MIN_TITLE_LETTERS`] letters and "…", or the whole title when that is not wider. Rounded up
/// by [`WIDTH_SLACK`]: `ellipsize` adds the letters up one by one, which can come out a hair
/// wider than this sum, and at exactly this width it must still keep them.
pub fn readable_width(title: &str) -> f32 {
    let head: String = title.chars().take(MIN_TITLE_LETTERS).collect();
    let cut = text_width(head.trim_end(), typo::caption()) + text_width(typo::ELLIPSIS, typo::caption());
    text_width(title, typo::caption()).min(cut) + WIDTH_SLACK
}

/// Where the titles of the strip's blocks go: per rectangle of `blocks` (as drawn, any order)
/// with the `readable` width of its title ([`readable_width`], same order), on a track that
/// ends at `track_end`.
///
/// - [`TitleSpot::Inside`] while the block holds `readable` between its text insets
///   (`TEXT_LEFT`, `TEXT_RIGHT`);
/// - else [`TitleSpot::Beside`]: from `beside_gap()` right of the block up to `TEXT_RIGHT` before
///   the first block in its way – one that shares some of its height and reaches further right,
///   in practice the next block of its lane – or up to `track_end`, when that holds `readable`;
/// - else [`TitleSpot::Bare`].
///
/// Text beside a block keeps to the block's own height. The lanes begin below the label row
/// ([`lanes_top`]), so it may cross the now line but never reaches its time label.
pub fn title_spots(blocks: &[Rectangle], readable: &[f32], track_end: f32) -> Vec<TitleSpot> {
    let shares_height = |a: &Rectangle, b: &Rectangle| a.y < b.y + b.height && b.y < a.y + a.height;
    blocks
        .iter()
        .zip(readable)
        .enumerate()
        .map(|(i, (block, &needed))| {
            if block.width - TEXT_LEFT - TEXT_RIGHT >= needed {
                return TitleSpot::Inside;
            }
            let right = block.x + block.width;
            let free_until = blocks
                .iter()
                .enumerate()
                .filter(|&(j, other)| j != i && other.x + other.width > right && shares_height(block, other))
                .map(|(_, other)| other.x - TEXT_RIGHT)
                .fold(track_end, f32::min);
            let x = right + beside_gap();
            let width = free_until - x;
            if width >= needed { TitleSpot::Beside { x, width } } else { TitleSpot::Bare }
        })
        .collect()
}

/// Where the lanes of the strip's timeline begin: below the row of hour labels and the now
/// label, set off by `space::xs()`.
fn lanes_top() -> f32 {
    line_height(now_label()) + space::xs()
}

/// Every how many hours the strip labels its axis, so labels `px_per_hour` apart do not crowd.
pub fn label_step(px_per_hour: f32) -> u32 {
    [1, 2, 3, 4, 6].into_iter().find(|step| *step as f32 * px_per_hour >= label_room()).unwrap_or(12)
}

/// Height of the date line and the lead of the mini tile: the date, then the big figure, the
/// title and when.
fn mini_top_height() -> f32 {
    line_height(typo::body_strong())
        + space::s()
        + line_height(typo::display())
        + space::xs()
        + line_height(typo::body_strong())
        + line_height(typo::meta())
}

/// How many rows fit in a mini tile `height` high besides the date and the lead, when they come
/// in `blocks` groups (today's all-day events under the date, the rest under the lead), each set
/// off by `space::s()`.
pub fn rows_that_fit(height: f32, blocks: usize) -> usize {
    let room = height - 2.0 * space::m() - mini_top_height() - blocks as f32 * space::s();
    let row = line_height(typo::body()) + space::xs();
    ((room + space::xs()) / row).floor().max(0.0) as usize
}

/// The mini tile for a window of `size`: the date (with "Free until 14:00" when it fits), the
/// lead – a later day's name, big time, title with its colour dot, when and where – and as many
/// further events of today as fit, then the days after ([`later_days`]) in a tall tile. A calm
/// "Nothing else today" when nothing comes.
pub fn mini(data: &ViewData<'_>, size: Size) -> Element<'static, Message> {
    let entries = entries(&data.items);
    let now = data.now;
    let focus = focus(&entries, now);
    let width = (size.width - 2.0 * space::m()).max(0.0);

    let date = short_date(now.date());
    let date_width = text_width(&date, typo::body_strong());
    let mut top = row![typo::text(typo::body_strong(), date).style(theme::text_2).wrapping(Wrapping::None)]
        .align_y(Alignment::Center);
    let mut body = column![].width(Length::Fill);
    // `rows_that_fit` counts with one line of detail in the lead; the day above a later lead is a
    // line more when the detail line is there as well (a place).
    let mut height = size.height;
    match (focus, display(focus)) {
        (Focus::Now(entry) | Focus::Next(entry) | Focus::Later(entry), Some(big)) => {
            if let Some(line) = headline(focus) {
                let line = ellipsize(&line, typo::meta(), width - date_width - space::s());
                top = top.push(Space::new().width(Length::Fill)).push(quiet(line));
            }
            let (day, detail) = (day_above(focus, now), detail_below(focus, entry, now));
            if day.is_some() && !detail.is_empty() {
                height -= line_height(typo::meta());
            }
            body = body.push(lead(entry, day, big, &detail, width));
        }
        _ => {
            let calm = headline(focus).unwrap_or_default();
            body = body.push(typo::text(typo::title(), ellipsize(&calm, typo::title(), width)).style(theme::text_2));
        }
    }

    // Today's all-day events belong to the date, so they come first and stand above the lead;
    // the rows under the lead continue its day. What does not fit is counted in "+3 more"; when
    // all of it fits, the days after fill the rest of a tall tile.
    let all_day = all_day_on(&entries, now.date());
    let rest = rest(&entries, now, focus);
    let blocks = usize::from(!all_day.is_empty()) + usize::from(!rest.is_empty());
    let count = all_day.len() + rest.len();
    let fit = rows_that_fit(height, blocks);
    let (shown, more) = shown_rows(count, fit);
    let later = match lead_day(focus, now) {
        // Each later day is a block of its own, set off like the others.
        Some(day) if more.is_none() => {
            fill_later(&entries, day, count, |groups| rows_that_fit(height, blocks + groups))
        }
        _ => Vec::new(),
    };
    let shown_all_day = shown.min(all_day.len());
    let mut above = column![].spacing(space::xs());
    for entry in all_day.iter().take(shown_all_day) {
        above = above.push(row_view(entry, None, width));
    }
    let mut below = column![].spacing(space::xs());
    for entry in rest.iter().take(shown - shown_all_day) {
        below = below.push(row_view(entry, Some(row_time(entry, now)), width));
    }
    if let Some(more) = more {
        below = below.push(quiet(trf("+{n} more", &[("n", &more)])));
    }

    let mut tile = column![top.width(Length::Fill), Space::new().height(space::s())];
    if shown_all_day > 0 {
        tile = tile.push(above).push(Space::new().height(space::s()));
    }
    tile = tile.push(body);
    if shown > shown_all_day || more.is_some() {
        tile = tile.push(Space::new().height(space::s())).push(below);
    }
    for (day, day_entries) in later {
        let mut group = column![quiet_caption(day_name(day, now.date()))].spacing(space::xs());
        for entry in day_entries {
            group = group.push(row_view(entry, later_time(entry, day), width));
        }
        tile = tile.push(Space::new().height(space::s())).push(group);
    }
    container(tile)
        .padding(space::m())
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(style::container::panel_flush)
        .into()
}

/// The day the lead belongs to, after which [`later_days`] begin: today for a running or next
/// event, the later day for [`Focus::Later`]; none when nothing comes.
fn lead_day(focus: Focus<'_>, now: NaiveDateTime) -> Option<NaiveDate> {
    match focus {
        Focus::Now(_) | Focus::Next(_) => Some(now.date()),
        Focus::Later(entry) => Some(entry.start.date()),
        Focus::Nothing => None,
    }
}

/// The [`later_days`] after `day` that fit besides the `used` rows above, where `fit(groups)` is
/// how many rows the tile holds with that many later days as blocks of their own.
fn fill_later(
    entries: &[Entry],
    day: NaiveDate,
    used: usize,
    fit: impl Fn(usize) -> usize,
) -> Vec<(NaiveDate, Vec<&Entry>)> {
    // More days take more gaps between them: reserve the gaps of `n` days, and if more come out,
    // try again with as many. Fewer days than reserved always fit.
    let mut n = 1;
    loop {
        let later = later_days(entries, day, fit(n).saturating_sub(used));
        if later.len() <= n {
            return later;
        }
        n = later.len();
    }
}

/// A day's name over its events in a tall mini tile: `CAPTION` in `text_3`, never wrapped.
fn quiet_caption(line: String) -> iced::widget::Text<'static> {
    typo::text(typo::caption(), line).style(theme::text_3).wrapping(Wrapping::None)
}

/// A line in `META` and `text_3`, never wrapped.
fn quiet(line: String) -> iced::widget::Text<'static> {
    typo::text(typo::meta(), line).style(theme::text_3).wrapping(Wrapping::None)
}

/// The lead of the mini tile: the `day` of a later event ("Tomorrow"), `big` in `DISPLAY`
/// (tabular when it is a time), the title after the colour dot, and `detail` (when, where) below.
fn lead(entry: &Entry, day: Option<String>, big: String, detail: &str, width: f32) -> Element<'static, Message> {
    let level = if big.starts_with(|c: char| c.is_ascii_digit()) { typo::display().tabular() } else { typo::display() };
    let title = ellipsize(&entry.title, typo::body_strong(), width - dot() - space::s());
    let title =
        row![widgets::color_dot(entry.color, dot()), typo::text(typo::body_strong(), title).wrapping(Wrapping::None)]
            .spacing(space::s())
            .align_y(Alignment::Center);
    let mut lead = column![];
    if let Some(day) = day {
        lead = lead.push(typo::text(typo::meta(), ellipsize(&day, typo::meta(), width)).style(theme::text_2));
    }
    lead =
        lead.push(typo::text(level, big).wrapping(Wrapping::None)).push(Space::new().height(space::xs())).push(title);
    if !detail.is_empty() {
        let detail = ellipsize(detail, typo::meta(), width);
        lead = lead.push(typo::text(typo::meta(), detail).style(theme::text_2).wrapping(Wrapping::None));
    }
    lead.into()
}

/// The time at the right of a row: the start, or "until 15:00" for an event already running.
fn row_time(entry: &Entry, now: NaiveDateTime) -> String {
    if entry.start <= now {
        trf("until {time}", &[("time", &format_time(entry.end.time()))])
    } else {
        format_time(entry.start.time())
    }
}

/// One more event: colour dot and title, its start `time` at the right (none for an all-day
/// event under the date).
fn row_view(entry: &Entry, time: Option<String>, width: f32) -> Element<'static, Message> {
    let time_width = time.as_deref().map_or(0.0, |time| text_width(time, typo::meta()) + space::s());
    let title = ellipsize(&entry.title, typo::body(), width - dot() - space::s() - time_width);
    let mut line =
        row![widgets::color_dot(entry.color, dot()), typo::text(typo::body(), title).wrapping(Wrapping::None)]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .height(line_height(typo::body()));
    if let Some(time) = time {
        line = line
            .push(Space::new().width(Length::Fill))
            .push(typo::text(typo::meta().tabular(), time).style(theme::text_3).wrapping(Wrapping::None));
    }
    line.into()
}

/// How many all-day chips fit under the date and the status line of a strip `height` high.
fn chips_that_fit(height: f32) -> usize {
    let room = height - 2.0 * space::s() - line_height(typo::body_strong()) - line_height(typo::meta()) - space::s();
    ((room + space::xs()) / (style::CHIP_HEIGHT + space::xs())).floor().max(0.0) as usize
}

/// The strip for a window of `size`: at the left the date, a status line ("Free until 14:00",
/// the running event and when it ends, "Nothing else today") and today's all-day events as chips;
/// beside it today as a timeline ([`Timeline`]).
pub fn strip(data: &ViewData<'_>, size: Size) -> Element<'static, Message> {
    let entries = entries(&data.items);
    let now = data.now;
    let today = now.date();
    let focus = focus(&entries, now);
    // The info column takes a sidebar's width, but no more than a quarter of a short strip, so
    // the hours keep the room.
    let left = size::sidebar().min(size.width / 4.0);

    let status = match focus {
        Focus::Now(entry) => {
            [Some(entry.title.clone()), when(focus, now)].into_iter().flatten().collect::<Vec<_>>().join(SEPARATOR)
        }
        _ => headline(focus).unwrap_or_default(),
    };
    let mut info = column![
        typo::text(typo::body_strong(), short_date(today)).style(theme::text_2).wrapping(Wrapping::None),
        quiet(ellipsize(&status, typo::meta(), left)),
    ];
    let all_day = all_day_on(&entries, today);
    let (shown, more) = shown_rows(all_day.len(), chips_that_fit(size.height));
    if shown > 0 || more.is_some() {
        let mut chips = column![].spacing(space::xs());
        for entry in all_day.iter().take(shown) {
            chips = chips.push(chip(entry, left));
        }
        if let Some(more) = more {
            chips = chips.push(quiet(trf("+{n} more", &[("n", &more)])));
        }
        info = info.push(Space::new().height(space::s())).push(chips);
    }

    let timed: Vec<Entry> = entries.iter().filter(|e| !e.all_day && e.on(today)).cloned().collect();
    let (first, last) = hours(&entries, now);
    let timeline =
        canvas::Canvas::new(Timeline { entries: timed, now, first, last }).width(Length::Fill).height(Length::Fill);
    let content = row![container(info).width(left).clip(true), timeline].spacing(space::l());
    container(content)
        .padding(Padding { top: space::s(), bottom: space::s(), left: space::m(), right: space::m() })
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::container::panel_flush)
        .into()
}

/// An all-day event as a chip `width` wide: tinted, with the colour bar, its title in `CAPTION`.
fn chip(entry: &Entry, width: f32) -> Element<'static, Message> {
    let color = entry.color;
    let title = ellipsize(&entry.title, typo::caption(), width - TEXT_LEFT - TEXT_RIGHT);
    let bar = container(Space::new().width(BAR).height(Length::Fill)).style(move |_: &Theme| container::Style {
        background: Some(Background::Color(color)),
        border: border::rounded(Radius::new(0.0).left(radius::xs())),
        ..container::Style::default()
    });
    let label = container(typo::text(typo::caption(), title).wrapping(Wrapping::None))
        .padding(Padding::new(0.0).left(TEXT_LEFT - BAR))
        .center_y(Length::Fill);
    container(row![bar, label])
        .width(width)
        .height(style::CHIP_HEIGHT)
        .clip(true)
        .style(move |t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(over(p.tinted(color), p.panel))),
                border: border::rounded(radius::xs()),
                ..container::Style::default()
            }
        })
        .into()
}

/// Today from `first` to `last` o'clock as a horizontal timeline: hour labels and lines, the
/// timed events as blocks in lanes, the now line.
struct Timeline {
    /// Today's timed entries.
    entries: Vec<Entry>,
    now: NaiveDateTime,
    first: u32,
    last: u32,
}

impl Timeline {
    /// x of `minutes` since midnight in a timeline `width` wide; the ends are inset by
    /// `space::l()`, so the first and the last label are not cut.
    fn x_of(&self, minutes: f32, width: f32) -> f32 {
        let from = self.first as f32 * 60.0;
        let total = (self.last - self.first).max(1) as f32 * 60.0;
        space::l() + (minutes - from) / total * (width - 2.0 * space::l()).max(0.0)
    }

    /// The block of each entry, in the order of `entries`, on a canvas `width` × `height`:
    /// from its start to its end (at least `MIN_BLOCK`, `GAP` short of the next), in its lane
    /// ([`lanes`]) below [`lanes_top`].
    fn blocks(&self, width: f32, height: f32) -> Vec<Rectangle> {
        let today = self.now.date();
        let top = lanes_top();
        let spans: Vec<(f32, f32)> = self.entries.iter().map(|e| minutes_today(e, today)).collect();
        spans
            .iter()
            .zip(lanes(&spans))
            .map(|(&(start, end), (lane, count))| {
                let x0 = self.x_of(start, width);
                let x1 = self.x_of(end, width).max(x0 + MIN_BLOCK);
                let lane_height = ((height - top) - (count - 1) as f32 * GAP) / count as f32;
                Rectangle::new(
                    Point::new(x0, top + lane as f32 * (lane_height + GAP)),
                    Size::new((x1 - x0 - GAP).max(MIN_BLOCK), lane_height),
                )
            })
            .collect()
    }
}

impl canvas::Program<Message> for Timeline {
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
        let (width, height) = (bounds.width, bounds.height);
        let label_row = line_height(now_label());
        let top = lanes_top();
        let now_minutes = self.now.num_seconds_from_midnight() as f32 / 60.0;
        let now_x = self.x_of(now_minutes, width);

        let per_hour = (width - 2.0 * space::l()) / (self.last - self.first).max(1) as f32;
        let step = label_step(per_hour);
        for hour in self.first..=self.last {
            let x = self.x_of(hour as f32 * 60.0, width);
            line(&mut frame, Point::new(x, top), Point::new(x, height), p.line);
            let covered = (x - now_x).abs() < now_label_width();
            if (hour - self.first).is_multiple_of(step) && !covered {
                let at = Point::new(x, label_row / 2.0);
                let hour_label = format!("{hour:02}:00");
                label(&mut frame, &hour_label, at, typo::micro(), p.text_3, TextAlign::Center, Vertical::Center);
            }
        }

        let blocks = self.blocks(width, height);
        let readable: Vec<f32> = self.entries.iter().map(|e| readable_width(&e.title)).collect();
        let spots = title_spots(&blocks, &readable, self.x_of(self.last as f32 * 60.0, width));
        for ((entry, &rect), spot) in self.entries.iter().zip(&blocks).zip(spots) {
            let tone = Tone::of(&p, entry.color, entry.end <= self.now);
            block(&mut frame, rect, entry, &tone, spot);
        }

        now_line(&mut frame, &p, now_x, top, height, width, self.now.time());
        vec![frame.into_geometry()]
    }
}

/// The colours of one block, composed over the panel: tinted fill, full-colour bar, title in
/// `text`, time in `text_2`, place in `text_3`; a past block faded towards the panel (55 %).
struct Tone {
    fill: Color,
    bar: Color,
    title: Color,
    time: Color,
    place: Color,
}

impl Tone {
    fn of(p: &Palette, color: Color, past: bool) -> Tone {
        let fade = |c: Color| if past { mix(p.panel, c, PAST_OPACITY) } else { c };
        Tone {
            fill: fade(over(p.tinted(color), p.panel)),
            bar: fade(color),
            title: fade(p.text),
            time: fade(p.text_2),
            place: fade(p.text_3),
        }
    }
}

/// An event block: rounded `radius::xs()`, the bar on the left, and its text where `spot` says
/// ([`title_spots`]): inside title, time range and place from the top, each cut with "…" to the
/// block's width; beside the title and, when a line more fits, the time range; or none.
fn block(frame: &mut Frame, rect: Rectangle, entry: &Entry, tone: &Tone, spot: TitleSpot) {
    frame.fill(&Path::rounded_rectangle(rect.position(), rect.size(), Radius::from(radius::xs())), tone.bar);
    let body = Size::new((rect.width - BAR).max(0.0), rect.height);
    frame.fill(
        &Path::rounded_rectangle(Point::new(rect.x + BAR, rect.y), body, Radius::new(0.0).right(radius::xs())),
        tone.fill,
    );
    let range = format!("{} – {}", format_time(entry.start.time()), format_time(entry.end.time()));
    match spot {
        TitleSpot::Inside => {
            let lines = [
                (entry.title.as_str(), typo::caption(), tone.title),
                (range.as_str(), typo::micro(), tone.time),
                (entry.location.as_deref().unwrap_or_default(), typo::meta(), tone.place),
            ];
            let clip = Rectangle { width: (rect.width - TEXT_RIGHT).max(0.0), ..rect };
            text_lines(frame, clip, rect.x + TEXT_LEFT, rect.width - TEXT_LEFT - TEXT_RIGHT, &lines);
        }
        TitleSpot::Beside { x, width } => {
            // Without a fill behind it a cut time ("09:30 – 1…") would read as a wrong one: the
            // range comes whole or not at all. The place stays in the block's larger views.
            let range = if text_width(&range, typo::micro()) <= width { range.as_str() } else { "" };
            let lines = [(entry.title.as_str(), typo::caption(), tone.title), (range, typo::micro(), tone.time)];
            text_lines(frame, Rectangle { x, width, ..rect }, x, width, &lines);
        }
        TitleSpot::Bare => {}
    }
}

/// Lines of text from the top of `clip` at `x`, each cut with "…" to `room`; stops at the first
/// empty line or the first that has no height left in `clip`.
fn text_lines(frame: &mut Frame, clip: Rectangle, x: f32, room: f32, lines: &[(&str, typo::Level, Color)]) {
    frame.with_clip(clip, |frame| {
        let mut y = clip.y + TEXT_TOP;
        for &(content, level, color) in lines {
            if content.is_empty() || y + line_height(level) > clip.y + clip.height {
                break;
            }
            let text = ellipsize(content, level, room);
            label(frame, &text, Point::new(x, y), level, color, TextAlign::Left, Vertical::Top);
            y += line_height(level);
        }
    });
}

/// Now: a 1-px red line down the track with a dot at its top, and the time as a red pill with
/// white figures in the label row (as in the week view, turned on its side).
fn now_line(frame: &mut Frame, p: &Palette, x: f32, top: f32, height: f32, width: f32, now: NaiveTime) {
    line(frame, Point::new(x, top), Point::new(x, height), p.accent);
    frame.fill(&Path::circle(Point::new(x, top), NOW_DOT), p.accent);
    let pill_height = line_height(now_label());
    let left = (x - now_label_width() / 2.0).clamp(0.0, (width - now_label_width()).max(0.0));
    let pill = Rectangle::new(Point::new(left, 0.0), Size::new(now_label_width(), pill_height));
    frame.fill(&Path::rounded_rectangle(pill.position(), pill.size(), Radius::from(radius::xs())), p.accent);
    let at = Point::new(pill.center_x(), pill.center_y());
    label(frame, &format_time(now), at, now_label(), p.on_accent, TextAlign::Center, Vertical::Center);
}

/// A 1-px line.
fn line(frame: &mut Frame, from: Point, to: Point, color: Color) {
    frame.stroke(&Path::line(from, to), Stroke::default().with_color(color).with_width(1.0));
}

/// One line of text in `level`, never wrapped.
fn label(
    frame: &mut Frame,
    content: &str,
    position: Point,
    level: typo::Level,
    color: Color,
    align_x: TextAlign,
    align_y: Vertical,
) {
    frame.fill_text(canvas::Text {
        content: content.to_string(),
        position,
        max_width: f32::INFINITY,
        color,
        size: level.size,
        line_height: level.line_height,
        font: level.font,
        align_x,
        align_y,
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sunday 11 October 2026 at `hour:minute`.
    fn at(hour: u32, minute: u32) -> NaiveDateTime {
        on(11, hour, minute)
    }

    fn on(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("date").and_hms_opt(hour, minute, 0).expect("time")
    }

    fn timed(title: &str, start: NaiveDateTime, end: NaiveDateTime) -> Entry {
        Entry { title: title.into(), location: None, color: Color::BLACK, start, end, all_day: false }
    }

    fn all_day(title: &str, day: u32) -> Entry {
        Entry { all_day: true, ..timed(title, on(day, 0, 0), on(day + 1, 0, 0)) }
    }

    fn sorted(mut entries: Vec<Entry>) -> Vec<Entry> {
        entries.sort_by(|a, b| (a.start, a.end, &a.title).cmp(&(b.start, b.end, &b.title)));
        entries
    }

    fn title(focus: Focus<'_>) -> Option<&str> {
        match focus {
            Focus::Now(e) | Focus::Next(e) | Focus::Later(e) => Some(e.title.as_str()),
            Focus::Nothing => None,
        }
    }

    #[test]
    fn a_running_event_leads_with_ends_in() {
        let entries = sorted(vec![
            timed("Standup", at(9, 30), at(10, 0)),
            timed("Review", at(14, 0), at(15, 0)),
            all_day("Home office", 11),
        ]);
        let now = at(9, 41);
        let focus = focus(&entries, now);
        assert!(matches!(focus, Focus::Now(_)));
        assert_eq!(title(focus), Some("Standup"));
        assert_eq!(when(focus, now), Some(trf("ends in {span}", &[("span", &trf("{n} min", &[("n", &19)]))])));
        assert_eq!(display(focus), Some(tr("Now").to_string()));
        assert_eq!(headline(focus), None, "nothing is free while an event runs");
    }

    #[test]
    fn cancelled_events_are_left_out_so_they_neither_run_nor_lead() {
        let event = |summary: &str, start: NaiveDateTime, end: NaiveDateTime, status| CalendarEvent {
            uid: String::new(),
            collection: String::new(),
            href: None,
            etag: None,
            summary: summary.into(),
            start: EventTime::DateTime(torro_gui::datetime::to_utc(start)),
            end: Some(EventTime::DateTime(torro_gui::datetime::to_utc(end))),
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
            alarms: Vec::new(),
            recurrence_id: None,
            repeat: None,
            time_zone: None,
            url: None,
            status,
            geo: None,
            organizer: None,
            attendees: Vec::new(),
            exceptions: Vec::new(),
        };
        let cancelled = Some(torro_pim::model::EventStatus::Cancelled);
        let events = [
            event("Standup", at(9, 30), at(10, 0), cancelled),
            event("Review", at(14, 0), at(15, 0), None),
            event("Dentist", at(16, 0), at(17, 0), Some(torro_pim::model::EventStatus::Tentative)),
        ];
        let items: Vec<Item<'_>> =
            events.iter().enumerate().map(|(index, event)| Item { index, event, color: Color::BLACK }).collect();
        let entries = entries(&items);
        let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Review", "Dentist"]);
        let lead = focus(&entries, at(9, 41));
        assert_eq!(title(lead), Some("Review"), "the cancelled standup does not run");
        assert_eq!(headline(lead), Some(trf("Free until {time}", &[("time", &"14:00")])));
    }

    #[test]
    fn of_two_running_events_the_one_begun_last_leads() {
        let entries = sorted(vec![timed("Workshop", at(9, 0), at(12, 0)), timed("Call", at(10, 0), at(10, 30))]);
        assert_eq!(title(focus(&entries, at(10, 10))), Some("Call"));
    }

    #[test]
    fn between_events_the_next_one_leads_with_in() {
        let entries = sorted(vec![
            timed("Standup", at(9, 30), at(10, 0)),
            timed("Review", at(14, 0), at(15, 0)),
            timed("Dentist", at(17, 30), at(18, 30)),
        ]);
        let now = at(12, 40);
        let focus = focus(&entries, now);
        assert!(matches!(focus, Focus::Next(_)));
        assert_eq!(title(focus), Some("Review"));
        let in_span = trf("{h} h {m} min", &[("h", &1), ("m", &20)]);
        assert_eq!(when(focus, now), Some(trf("in {span}", &[("span", &in_span)])));
        assert_eq!(display(focus), Some("14:00".to_string()));
        assert_eq!(headline(focus), Some(trf("Free until {time}", &[("time", &"14:00")])));
    }

    #[test]
    fn the_rest_of_today_leaves_out_the_lead_and_what_is_over() {
        let entries = sorted(vec![
            timed("Standup", at(9, 30), at(10, 0)),
            timed("Review", at(14, 0), at(15, 0)),
            timed("Dentist", at(17, 30), at(18, 30)),
            timed("Tomorrow's call", on(12, 9, 0), on(12, 10, 0)),
            all_day("Home office", 11),
        ]);
        let now = at(12, 40);
        let rest: Vec<&str> = rest(&entries, now, focus(&entries, now)).iter().map(|e| e.title.as_str()).collect();
        assert_eq!(rest, ["Dentist"], "all-day events stand under the date instead");
        let all_day: Vec<&str> = all_day_on(&entries, now.date()).iter().map(|e| e.title.as_str()).collect();
        assert_eq!(all_day, ["Home office"]);
    }

    #[test]
    fn when_nothing_is_left_today_the_next_day_leads() {
        let entries = sorted(vec![
            timed("Standup", at(9, 30), at(10, 0)),
            all_day("Birthday", 13),
            all_day("Home office", 12),
            timed("Planning", on(12, 9, 0), on(12, 10, 0)),
            timed("Early call", on(12, 7, 30), on(12, 8, 0)),
        ]);
        let now = at(20, 0);
        let focus = focus(&entries, now);
        assert!(matches!(focus, Focus::Later(_)));
        assert_eq!(title(focus), Some("Early call"), "the first timed event of the next day with events");
        assert_eq!(when(focus, now), Some(tr("Tomorrow").to_string()));
        assert_eq!(display(focus), Some("07:30".to_string()));
        assert_eq!(headline(focus), Some(tr("Nothing else today").to_string()));
        let rest: Vec<&str> = rest(&entries, now, focus).iter().map(|e| e.title.as_str()).collect();
        assert_eq!(rest, ["Planning"], "the rows under the lead continue its day");
    }

    #[test]
    fn a_later_lead_names_its_day_above_the_time() {
        let mut call = timed("Early call", on(12, 7, 30), on(12, 8, 0));
        call.location = Some("Room 3".into());
        let entries = sorted(vec![call]);
        let now = at(20, 0);
        let focus = focus(&entries, now);
        assert_eq!(day_above(focus, now), Some(tr("Tomorrow").to_string()));
        assert_eq!(detail_below(focus, &entries[0], now), "Room 3", "the day is not repeated below");
        let today = sorted(vec![timed("Review", at(21, 0), at(22, 0))]);
        let next = super::focus(&today, now);
        assert_eq!(day_above(next, now), None);
        assert_eq!(detail_below(next, &today[0], now), trf("in {span}", &[("span", &span(60))]));
    }

    #[test]
    fn a_tall_tile_fills_with_the_days_after() {
        let entries = sorted(vec![
            timed("Review", at(14, 0), at(15, 0)),
            timed("Planning", on(12, 9, 0), on(12, 10, 0)),
            all_day("Home office", 12),
            timed("Dentist", on(12, 17, 0), on(12, 18, 0)),
            timed("Gym", on(14, 7, 0), on(14, 8, 0)),
        ]);
        let titles = |days: Vec<(NaiveDate, Vec<&Entry>)>| {
            days.into_iter()
                .map(|(day, list)| (day, list.iter().map(|e| e.title.clone()).collect::<Vec<_>>()))
                .collect::<Vec<_>>()
        };
        let today = at(0, 0).date();
        let (mon, wed) = (on(12, 0, 0).date(), on(14, 0, 0).date());
        assert_eq!(
            titles(later_days(&entries, today, 10)),
            [(mon, vec!["Home office".to_string(), "Planning".into(), "Dentist".into()]), (wed, vec!["Gym".into()])],
            "all-day first, each day under its name"
        );
        assert_eq!(titles(later_days(&entries, today, 3)), [(mon, vec!["Home office".into(), "Planning".into()])]);
        assert!(later_days(&entries, today, 1).is_empty(), "a name alone is not worth a row");
        assert_eq!(later_time(&timed("Planning", on(12, 9, 0), on(12, 10, 0)), mon), Some("09:00".to_string()));
        assert_eq!(later_time(&all_day("Home office", 12), mon), None);
        let party = timed("Party", on(12, 22, 0), on(13, 1, 30));
        assert_eq!(later_time(&party, on(13, 0, 0).date()), Some(trf("until {time}", &[("time", &"01:30")])));
        assert_eq!(day_name(mon, today), tr("Tomorrow"));
        assert_eq!(day_name(wed, today), short_date(wed));
    }

    #[test]
    fn a_later_day_with_only_all_day_events_leads_with_one_of_them() {
        let entries = sorted(vec![all_day("Birthday", 13)]);
        let now = at(20, 0);
        let focus = focus(&entries, now);
        assert_eq!(title(focus), Some("Birthday"));
        assert_eq!(when(focus, now), Some(short_date(on(13, 0, 0).date())));
        assert_eq!(display(focus), Some(tr("All day").to_string()));
    }

    #[test]
    fn with_nothing_to_come_the_tile_is_calm() {
        let entries = sorted(vec![timed("Standup", at(9, 30), at(10, 0))]);
        let focus = focus(&entries, at(11, 0));
        assert_eq!(focus, Focus::Nothing);
        assert_eq!((when(focus, at(11, 0)), display(focus)), (None, None));
        assert_eq!(headline(focus), Some(tr("Nothing else today").to_string()));
    }

    #[test]
    fn rows_name_the_start_or_how_long_a_running_event_lasts() {
        let workshop = timed("Workshop", at(13, 0), at(15, 0));
        assert_eq!(row_time(&workshop, at(12, 0)), "13:00");
        assert_eq!(row_time(&workshop, at(14, 40)), trf("until {time}", &[("time", &"15:00")]));
    }

    #[test]
    fn spans_read_in_minutes_and_hours() {
        assert_eq!(span(19), trf("{n} min", &[("n", &19)]));
        assert_eq!(span(120), trf("{n} h", &[("n", &2)]));
        assert_eq!(span(80), trf("{h} h {m} min", &[("h", &1), ("m", &20)]));
        // Half a minute left is a minute, not none.
        let now = at(9, 59) + Duration::seconds(30);
        assert_eq!(minutes_until(now, at(10, 0)), 1);
        assert_eq!(minutes_until(at(9, 41), at(10, 0)), 19);
    }

    #[test]
    fn the_strip_spans_at_least_the_working_day() {
        let entries = sorted(vec![timed("Standup", at(9, 30), at(10, 0)), all_day("Home office", 11)]);
        assert_eq!(hours(&entries, at(12, 0)), (8, 18));
        assert_eq!(hours(&[], at(12, 0)), (8, 18));
    }

    #[test]
    fn the_strip_grows_to_early_late_and_now() {
        let entries = sorted(vec![timed("Gym", at(6, 45), at(7, 30)), timed("Dinner", at(19, 0), at(21, 10))]);
        assert_eq!(hours(&entries, at(12, 0)), (6, 22));
        assert_eq!(hours(&[], at(22, 15)), (8, 23), "now stays in view");
        assert_eq!(hours(&[], at(6, 5)), (6, 18));
    }

    #[test]
    fn the_strip_cuts_events_at_midnight() {
        let entries =
            sorted(vec![timed("Night shift", on(10, 22, 0), at(6, 0)), timed("Party", at(20, 0), on(12, 2, 0))]);
        assert_eq!(hours(&entries, at(12, 0)), (0, 24));
    }

    #[test]
    fn overlapping_events_share_the_height_touching_ones_do_not() {
        // 09–10 and 09:30–11 overlap; 11–12 only touches the second; 14–15 stands alone.
        let spans = [(540.0, 600.0), (570.0, 660.0), (660.0, 720.0), (840.0, 900.0)];
        assert_eq!(lanes(&spans), vec![(0, 2), (1, 2), (0, 1), (0, 1)]);
        assert_eq!(lanes(&[]), vec![]);
        // A third event during both takes a third lane; one after the first reuses lane 0.
        let spans = [(540.0, 600.0), (540.0, 660.0), (550.0, 560.0), (600.0, 630.0)];
        assert_eq!(lanes(&spans), vec![(0, 3), (1, 3), (2, 3), (0, 3)]);
    }

    /// A block in the lanes from `x` to `right`, `y` to `bottom`.
    fn rect(x: f32, right: f32, y: f32, bottom: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(right - x, bottom - y))
    }

    #[test]
    fn a_readable_title_needs_four_letters_or_all_of_a_short_one() {
        let cut = text_width("Dail", typo::caption()) + text_width(typo::ELLIPSIS, typo::caption());
        assert_eq!(readable_width("Daily Standup"), cut + WIDTH_SLACK);
        assert_eq!(readable_width("Gym"), text_width("Gym", typo::caption()) + WIDTH_SLACK);
        // Five letters are narrower whole than four and "…".
        assert_eq!(readable_width("Lunch"), text_width("Lunch", typo::caption()) + WIDTH_SLACK);
        // A cut after a space keeps no trailing space, as `ellipsize` does: "1:1…".
        let space = text_width("1:1", typo::caption()) + text_width(typo::ELLIPSIS, typo::caption());
        assert_eq!(readable_width("1:1 with Lena"), space + WIDTH_SLACK);
        // At exactly that width the title is cut where the rule says.
        assert_eq!(ellipsize("Daily Standup", typo::caption(), readable_width("Daily Standup")), "Dail…");
        assert_eq!(ellipsize("1:1 with Lena", typo::caption(), readable_width("1:1 with Lena")), "1:1…");
        assert_eq!(ellipsize("Lunch", typo::caption(), readable_width("Lunch")), "Lunch");
    }

    #[test]
    fn a_wide_block_keeps_its_title_inside() {
        let blocks = [rect(100.0, 300.0, 20.0, 170.0)];
        assert_eq!(title_spots(&blocks, &[40.0], 800.0), [TitleSpot::Inside]);
        // Exactly the readable width between the insets is enough.
        let snug = [rect(100.0, 100.0 + TEXT_LEFT + TEXT_RIGHT + 40.0, 20.0, 170.0)];
        assert_eq!(title_spots(&snug, &[40.0], 800.0), [TitleSpot::Inside]);
    }

    #[test]
    fn a_narrow_block_puts_its_title_beside_up_to_the_next_block_of_its_lane() {
        // 30 minutes in a 900 × 190 strip are about 25 points: no room inside.
        let blocks = [rect(100.0, 125.0, 20.0, 170.0), rect(400.0, 500.0, 20.0, 170.0)];
        let spots = title_spots(&blocks, &[40.0, 40.0], 800.0);
        let x = 125.0 + beside_gap();
        assert_eq!(spots[0], TitleSpot::Beside { x, width: 400.0 - TEXT_RIGHT - x });
        assert_eq!(spots[1], TitleSpot::Inside);
    }

    #[test]
    fn with_no_readable_room_beside_either_the_block_stays_bare() {
        let blocks = [rect(100.0, 125.0, 20.0, 170.0), rect(150.0, 400.0, 20.0, 170.0)];
        assert_eq!(title_spots(&blocks, &[40.0, 40.0], 800.0), [TitleSpot::Bare, TitleSpot::Inside]);
    }

    #[test]
    fn of_two_short_events_back_to_back_only_the_second_has_room_beside() {
        // 09:30–10:00 and 10:00–10:30 in one lane, `GAP` apart; then the track is free.
        let blocks = [rect(100.0, 123.0, 20.0, 170.0), rect(125.0, 148.0, 20.0, 170.0)];
        let spots = title_spots(&blocks, &[40.0, 40.0], 800.0);
        assert_eq!(spots[0], TitleSpot::Bare, "the second block follows right after the first");
        let x = 148.0 + beside_gap();
        assert_eq!(spots[1], TitleSpot::Beside { x, width: 800.0 - x }, "up to the end of the track");
    }

    #[test]
    fn a_short_event_at_the_end_of_the_track_has_only_the_rest_of_it() {
        let last = [rect(770.0, 795.0, 20.0, 170.0)];
        assert_eq!(title_spots(&last, &[40.0], 800.0), [TitleSpot::Bare]);
        // An hour earlier the rest of the track holds a cut title.
        let earlier = [rect(720.0, 745.0, 20.0, 170.0)];
        let x = 745.0 + beside_gap();
        assert_eq!(title_spots(&earlier, &[40.0], 800.0), [TitleSpot::Beside { x, width: 800.0 - x }]);
    }

    #[test]
    fn only_blocks_sharing_the_height_stand_in_the_way() {
        // Lane 0 of two above lane 1 with a long block beside it; then a block over the full
        // height, which ends the free track of both lanes.
        let blocks = [rect(100.0, 125.0, 20.0, 94.0), rect(110.0, 300.0, 96.0, 170.0), rect(500.0, 600.0, 20.0, 170.0)];
        let spots = title_spots(&blocks, &[40.0, 40.0, 40.0], 800.0);
        let x = 125.0 + beside_gap();
        assert_eq!(spots[0], TitleSpot::Beside { x, width: 500.0 - TEXT_RIGHT - x });
        assert_eq!(spots[1..], [TitleSpot::Inside, TitleSpot::Inside]);
    }

    #[test]
    fn titles_beside_never_reach_the_label_row_of_the_now_line() {
        // The demo strip at 900 × 190 (the timeline beside the info column): text keeps to the
        // height of its block, and every block lies below the row of the now label.
        let timeline = Timeline {
            entries: sorted(vec![
                timed("Daily Standup", at(9, 30), at(10, 0)),
                timed("Design review", at(11, 0), at(12, 30)),
                timed("1:1 with Lena", at(14, 30), at(15, 0)),
            ]),
            now: at(10, 30),
            first: 8,
            last: 18,
        };
        let (width, height) = (640.0, 174.0);
        let blocks = timeline.blocks(width, height);
        assert!(blocks.iter().all(|b| b.y >= line_height(now_label())));
        let readable: Vec<f32> = timeline.entries.iter().map(|e| readable_width(&e.title)).collect();
        let spots = title_spots(&blocks, &readable, timeline.x_of(18.0 * 60.0, width));
        assert!(matches!(spots[0], TitleSpot::Beside { .. }), "the standup is too narrow for its title");
        assert_eq!(spots[1], TitleSpot::Inside);
        let TitleSpot::Beside { width: room, .. } = spots[0] else { return };
        assert!(room < blocks[1].x - blocks[0].x, "it ends before the review");
        assert!(matches!(spots[2], TitleSpot::Beside { .. }));
    }

    #[test]
    fn labels_thin_out_when_hours_are_narrow() {
        assert_eq!(label_step(94.0), 1);
        assert_eq!(label_step(34.0), 2);
        assert_eq!(label_step(12.0), 4);
    }

    #[test]
    fn rows_fit_the_height_of_the_tile() {
        // 240×160: the lead and one more row.
        assert_eq!(rows_that_fit(160.0, 1), 1);
        assert_eq!(rows_that_fit(100.0, 1), 0);
        assert!(rows_that_fit(480.0, 2) >= 10);
    }

    #[test]
    fn all_day_entries_touch_their_days_only() {
        let entry = all_day("Home office", 11);
        assert!(entry.on(at(0, 0).date()));
        assert!(!entry.on(on(12, 0, 0).date()), "ending at midnight does not touch the next day");
        let instant = timed("Reminder", at(23, 59), at(23, 59));
        assert!(instant.on(at(0, 0).date()));
    }
}
