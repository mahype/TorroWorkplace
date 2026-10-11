//! Mini and strip: the calendar as a glance at a small tile (task K.19 in
//! `docs/tasks/kalender-aufgaben.md`; design skill `.claude/skills/torro-design`, section 6).
//!
//! - **Mini** ([`mini`]): the date at the top, then what matters now – the running event with
//!   "ends in 19 min", or the next one with its start time in `DISPLAY` and "in 1 h 20 min" – and
//!   as many of today's other events as fit. While nothing runs the top line says "Free until
//!   14:00"; when the day is done it says "Nothing else today" and the next event after today
//!   leads instead.
//! - **Strip** ([`strip`]): today as a horizontal timeline on a canvas – timed events as tinted
//!   blocks with their colour bar, all-day ones as chips on the left, the now line in red. It
//!   spans today's events and now, at least 08:00–18:00.
//!
//! Both are displays, not small apps: no buttons, forms or dialogs (a larger tile is the tiling
//! manager's job). They are rebuilt only with the app's minute tick and on new data.
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
use torro_gui::theme::{self, Palette, palette, radius, size, space};
use torro_gui::{style, typo, widgets};
use torro_pim::{Event as CalendarEvent, EventTime};

use super::{Item, ViewData, short_date};
use crate::app::Message;

const UNTITLED: &str = "(untitled)";
/// What [`ellipsize`] puts where it cut.
const ELLIPSIS: &str = "…";
/// Between the parts of a line: "in 20 min · Room 3".
const SEPARATOR: &str = " · ";
/// The strip shows at least this working day (hours), whatever the events.
const DAY_START: u32 = 8;
const DAY_END: u32 = 18;
/// The colour dot in front of a title in the mini tile.
const DOT: f32 = space::S;
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
const LABEL_ROOM: f32 = size::AXIS_NARROW;
/// Past blocks are drawn at this opacity over the panel (as in the week view).
const PAST_OPACITY: f32 = 0.55;
/// The time label of the now line: the axis figures at 500 on a 16-high pill, as in the week view.
const NOW_LABEL: typo::Level =
    typo::Level { font: theme::FONT_TABULAR_MEDIUM, line_height: typo::CAPTION.line_height, ..typo::MICRO };
/// Width of the now label: "23:59" with 3 on each side (as in the week view).
const NOW_LABEL_WIDTH: f32 = size::AXIS_NARROW - 2.0 * space::XS;
/// Radius of the dot at the top of the now line (7 across).
const NOW_DOT: f32 = 3.5;

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

/// The entries of `items`, by start (then end, then title).
pub fn entries(items: &[Item<'_>]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = items.iter().map(Entry::new).collect();
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

/// Every how many hours the strip labels its axis, so labels `px_per_hour` apart do not crowd.
pub fn label_step(px_per_hour: f32) -> u32 {
    [1, 2, 3, 4, 6].into_iter().find(|step| *step as f32 * px_per_hour >= LABEL_ROOM).unwrap_or(12)
}

/// `text` cut to fit `max_width` in `level`, with "…" where it was cut; empty when not even the
/// first letter fits.
///
/// Iced cannot measure text in a canvas, nor cut it with an ellipsis, so this estimates Inter's
/// letter widths a little generously: a cut comes a letter early rather than running over (the
/// views clip as well).
pub fn ellipsize(text: &str, level: typo::Level, max_width: f32) -> String {
    let size = level.size.0;
    if text_width(text, level) <= max_width {
        return text.to_string();
    }
    let room = max_width - text_width(ELLIPSIS, level);
    let mut kept = String::new();
    let mut width = 0.0;
    for c in text.chars() {
        width += char_em(c) * size;
        if width > room {
            break;
        }
        kept.push(c);
    }
    let kept = kept.trim_end();
    if kept.is_empty() { String::new() } else { format!("{kept}{ELLIPSIS}") }
}

/// The estimated width of `text` in `level` (see [`ellipsize`]).
pub fn text_width(text: &str, level: typo::Level) -> f32 {
    text.chars().map(char_em).sum::<f32>() * level.size.0
}

/// Estimated advance of `c` in Inter, in em, rounded up a little (Medium is wider than Regular).
fn char_em(c: char) -> f32 {
    match c {
        ' ' => 0.28,
        'i' | 'j' | 'l' | 'I' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' => 0.30,
        'f' | 't' | 'r' | '(' | ')' | '-' | '/' => 0.40,
        'm' | 'w' => 0.88,
        'M' | 'W' => 0.95,
        '0'..='9' => 0.64,
        c if c.len_utf8() >= 3 => 1.0,
        c if c.is_uppercase() => 0.72,
        c if c.is_lowercase() => 0.58,
        _ => 0.75,
    }
}

fn line_height(level: typo::Level) -> f32 {
    level.line_height.to_absolute(level.size).0
}

/// Height of the date line and the lead of the mini tile: the date, then the big figure, the
/// title and when.
fn mini_top_height() -> f32 {
    line_height(typo::BODY_STRONG)
        + space::S
        + line_height(typo::DISPLAY)
        + space::XS
        + line_height(typo::BODY_STRONG)
        + line_height(typo::META)
}

/// How many rows fit in a mini tile `height` high besides the date and the lead, when they come
/// in `blocks` groups (today's all-day events under the date, the rest under the lead), each set
/// off by `space::S`.
pub fn rows_that_fit(height: f32, blocks: usize) -> usize {
    let room = height - 2.0 * space::M - mini_top_height() - blocks as f32 * space::S;
    let row = line_height(typo::BODY) + space::XS;
    ((room + space::XS) / row).floor().max(0.0) as usize
}

/// How many of `count` rows to show when `fit` rows fit: all of them, or one row less and the
/// number left out ("+3 more"). With room for a single row that row is worth more than the count.
fn shown_rows(count: usize, fit: usize) -> (usize, Option<usize>) {
    if count <= fit || fit <= 1 {
        (count.min(fit), None)
    } else {
        let shown = fit - 1;
        (shown, Some(count - shown))
    }
}

/// The mini tile for a window of `size`: the date (with "Free until 14:00" when it fits), the
/// lead – big time, title with its colour dot, when and where – and as many further events of
/// today as fit. A calm "Nothing else today" when nothing comes.
pub fn mini(data: &ViewData<'_>, size: Size) -> Element<'static, Message> {
    let entries = entries(&data.items);
    let now = data.now;
    let focus = focus(&entries, now);
    let width = (size.width - 2.0 * space::M).max(0.0);

    let date = short_date(now.date());
    let date_width = text_width(&date, typo::BODY_STRONG);
    let mut top = row![typo::text(typo::BODY_STRONG, date).style(theme::text_2).wrapping(Wrapping::None)]
        .align_y(Alignment::Center);
    let mut body = column![].width(Length::Fill);
    match (focus, display(focus)) {
        (Focus::Now(entry) | Focus::Next(entry) | Focus::Later(entry), Some(big)) => {
            if let Some(line) = headline(focus) {
                let line = ellipsize(&line, typo::META, width - date_width - space::S);
                top = top.push(Space::new().width(Length::Fill)).push(quiet(line));
            }
            let where_ = [when(focus, now), entry.location.clone()].into_iter().flatten().collect::<Vec<_>>();
            body = body.push(lead(entry, big, &where_.join(SEPARATOR), width));
        }
        _ => {
            let calm = headline(focus).unwrap_or_default();
            body = body.push(typo::text(typo::TITLE, ellipsize(&calm, typo::TITLE, width)).style(theme::text_2));
        }
    }

    // Today's all-day events belong to the date, so they come first and stand above the lead;
    // the rows under the lead continue its day. What does not fit is counted in "+3 more".
    let all_day = all_day_on(&entries, now.date());
    let rest = rest(&entries, now, focus);
    let blocks = usize::from(!all_day.is_empty()) + usize::from(!rest.is_empty());
    let (shown, more) = shown_rows(all_day.len() + rest.len(), rows_that_fit(size.height, blocks));
    let shown_all_day = shown.min(all_day.len());
    let mut above = column![].spacing(space::XS);
    for entry in all_day.iter().take(shown_all_day) {
        above = above.push(row_view(entry, None, width));
    }
    let mut below = column![].spacing(space::XS);
    for entry in rest.iter().take(shown - shown_all_day) {
        below = below.push(row_view(entry, Some(row_time(entry, now)), width));
    }
    if let Some(more) = more {
        below = below.push(quiet(trf("+{n} more", &[("n", &more)])));
    }

    let mut tile = column![top.width(Length::Fill), Space::new().height(space::S)];
    if shown_all_day > 0 {
        tile = tile.push(above).push(Space::new().height(space::S));
    }
    tile = tile.push(body);
    if shown > shown_all_day || more.is_some() {
        tile = tile.push(Space::new().height(space::S)).push(below);
    }
    container(tile)
        .padding(space::M)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(style::container::panel_flush)
        .into()
}

/// A line in `META` and `text_3`, never wrapped.
fn quiet(line: String) -> iced::widget::Text<'static> {
    typo::text(typo::META, line).style(theme::text_3).wrapping(Wrapping::None)
}

/// The lead of the mini tile: `big` in `DISPLAY` (tabular when it is a time), the title after
/// the colour dot, and `detail` (when, where) below.
fn lead(entry: &Entry, big: String, detail: &str, width: f32) -> Element<'static, Message> {
    let level = if big.starts_with(|c: char| c.is_ascii_digit()) { typo::DISPLAY.tabular() } else { typo::DISPLAY };
    let title = ellipsize(&entry.title, typo::BODY_STRONG, width - DOT - space::S);
    let title =
        row![widgets::color_dot(entry.color, DOT), typo::text(typo::BODY_STRONG, title).wrapping(Wrapping::None)]
            .spacing(space::S)
            .align_y(Alignment::Center);
    let detail =
        typo::text(typo::META, ellipsize(detail, typo::META, width)).style(theme::text_2).wrapping(Wrapping::None);
    column![typo::text(level, big).wrapping(Wrapping::None), Space::new().height(space::XS), title, detail].into()
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
    let time_width = time.as_deref().map_or(0.0, |time| text_width(time, typo::META) + space::S);
    let title = ellipsize(&entry.title, typo::BODY, width - DOT - space::S - time_width);
    let mut line = row![widgets::color_dot(entry.color, DOT), typo::text(typo::BODY, title).wrapping(Wrapping::None)]
        .spacing(space::S)
        .align_y(Alignment::Center)
        .height(line_height(typo::BODY));
    if let Some(time) = time {
        line = line
            .push(Space::new().width(Length::Fill))
            .push(typo::text(typo::META.tabular(), time).style(theme::text_3).wrapping(Wrapping::None));
    }
    line.into()
}

/// How many all-day chips fit under the date and the status line of a strip `height` high.
fn chips_that_fit(height: f32) -> usize {
    let room = height - 2.0 * space::S - line_height(typo::BODY_STRONG) - line_height(typo::META) - space::S;
    ((room + space::XS) / (style::CHIP_HEIGHT + space::XS)).floor().max(0.0) as usize
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
    let left = size::SIDEBAR.min(size.width / 4.0);

    let status = match focus {
        Focus::Now(entry) => {
            [Some(entry.title.clone()), when(focus, now)].into_iter().flatten().collect::<Vec<_>>().join(SEPARATOR)
        }
        _ => headline(focus).unwrap_or_default(),
    };
    let mut info = column![
        typo::text(typo::BODY_STRONG, short_date(today)).style(theme::text_2).wrapping(Wrapping::None),
        quiet(ellipsize(&status, typo::META, left)),
    ];
    let all_day = all_day_on(&entries, today);
    let (shown, more) = shown_rows(all_day.len(), chips_that_fit(size.height));
    if shown > 0 || more.is_some() {
        let mut chips = column![].spacing(space::XS);
        for entry in all_day.iter().take(shown) {
            chips = chips.push(chip(entry, left));
        }
        if let Some(more) = more {
            chips = chips.push(quiet(trf("+{n} more", &[("n", &more)])));
        }
        info = info.push(Space::new().height(space::S)).push(chips);
    }

    let timed: Vec<Entry> = entries.iter().filter(|e| !e.all_day && e.on(today)).cloned().collect();
    let (first, last) = hours(&entries, now);
    let timeline =
        canvas::Canvas::new(Timeline { entries: timed, now, first, last }).width(Length::Fill).height(Length::Fill);
    let content = row![container(info).width(left).clip(true), timeline].spacing(space::L);
    container(content)
        .padding(Padding { top: space::S, bottom: space::S, left: space::M, right: space::M })
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::container::panel_flush)
        .into()
}

/// An all-day event as a chip `width` wide: tinted, with the colour bar, its title in `CAPTION`.
fn chip(entry: &Entry, width: f32) -> Element<'static, Message> {
    let color = entry.color;
    let title = ellipsize(&entry.title, typo::CAPTION, width - TEXT_LEFT - TEXT_RIGHT);
    let bar = container(Space::new().width(BAR).height(Length::Fill)).style(move |_: &Theme| container::Style {
        background: Some(Background::Color(color)),
        border: border::rounded(Radius::new(0.0).left(radius::XS)),
        ..container::Style::default()
    });
    let label = container(typo::text(typo::CAPTION, title).wrapping(Wrapping::None))
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
                border: border::rounded(radius::XS),
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
    /// `space::L`, so the first and the last label are not cut.
    fn x_of(&self, minutes: f32, width: f32) -> f32 {
        let from = self.first as f32 * 60.0;
        let total = (self.last - self.first).max(1) as f32 * 60.0;
        space::L + (minutes - from) / total * (width - 2.0 * space::L).max(0.0)
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
        let today = self.now.date();
        let label_row = line_height(NOW_LABEL);
        let top = label_row + space::XS;
        let now_minutes = self.now.num_seconds_from_midnight() as f32 / 60.0;
        let now_x = self.x_of(now_minutes, width);

        let per_hour = (width - 2.0 * space::L) / (self.last - self.first).max(1) as f32;
        let step = label_step(per_hour);
        for hour in self.first..=self.last {
            let x = self.x_of(hour as f32 * 60.0, width);
            line(&mut frame, Point::new(x, top), Point::new(x, height), p.line);
            let covered = (x - now_x).abs() < NOW_LABEL_WIDTH;
            if (hour - self.first).is_multiple_of(step) && !covered {
                let at = Point::new(x, label_row / 2.0);
                let hour_label = format!("{hour:02}:00");
                label(&mut frame, &hour_label, at, typo::MICRO, p.text_3, TextAlign::Center, Vertical::Center);
            }
        }

        let spans: Vec<(f32, f32)> = self.entries.iter().map(|e| minutes_today(e, today)).collect();
        for ((entry, &(start, end)), (lane, count)) in self.entries.iter().zip(&spans).zip(lanes(&spans)) {
            let x0 = self.x_of(start, width);
            let x1 = self.x_of(end, width).max(x0 + MIN_BLOCK);
            let lane_height = ((height - top) - (count - 1) as f32 * GAP) / count as f32;
            let rect = Rectangle::new(
                Point::new(x0, top + lane as f32 * (lane_height + GAP)),
                Size::new((x1 - x0 - GAP).max(MIN_BLOCK), lane_height),
            );
            let tone = Tone::of(p, entry.color, entry.end <= self.now);
            block(&mut frame, rect, entry, &tone);
        }

        now_line(&mut frame, p, now_x, top, height, width, self.now.time());
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

/// An event block: rounded `radius::XS`, the bar on the left, title, time range and place from
/// the top, each cut with "…" to the block's width and left out when there is no height for it.
fn block(frame: &mut Frame, rect: Rectangle, entry: &Entry, tone: &Tone) {
    frame.fill(&Path::rounded_rectangle(rect.position(), rect.size(), Radius::from(radius::XS)), tone.bar);
    let body = Size::new((rect.width - BAR).max(0.0), rect.height);
    frame.fill(
        &Path::rounded_rectangle(Point::new(rect.x + BAR, rect.y), body, Radius::new(0.0).right(radius::XS)),
        tone.fill,
    );
    let room = rect.width - TEXT_LEFT - TEXT_RIGHT;
    let range = format!("{} – {}", format_time(entry.start.time()), format_time(entry.end.time()));
    let lines = [
        (entry.title.as_str(), typo::CAPTION, tone.title),
        (range.as_str(), typo::MICRO, tone.time),
        (entry.location.as_deref().unwrap_or_default(), typo::META, tone.place),
    ];
    let clip = Rectangle { width: (rect.width - TEXT_RIGHT).max(0.0), ..rect };
    frame.with_clip(clip, |frame| {
        let mut y = rect.y + TEXT_TOP;
        for (content, level, color) in lines {
            if content.is_empty() || y + line_height(level) > rect.y + rect.height {
                break;
            }
            let text = ellipsize(content, level, room);
            label(frame, &text, Point::new(rect.x + TEXT_LEFT, y), level, color, TextAlign::Left, Vertical::Top);
            y += line_height(level);
        }
    });
}

/// Now: a 1-px red line down the track with a dot at its top, and the time as a red pill with
/// white figures in the label row (as in the week view, turned on its side).
fn now_line(frame: &mut Frame, p: &Palette, x: f32, top: f32, height: f32, width: f32, now: NaiveTime) {
    line(frame, Point::new(x, top), Point::new(x, height), p.accent);
    frame.fill(&Path::circle(Point::new(x, top), NOW_DOT), p.accent);
    let pill_height = line_height(NOW_LABEL);
    let left = (x - NOW_LABEL_WIDTH / 2.0).clamp(0.0, (width - NOW_LABEL_WIDTH).max(0.0));
    let pill = Rectangle::new(Point::new(left, 0.0), Size::new(NOW_LABEL_WIDTH, pill_height));
    frame.fill(&Path::rounded_rectangle(pill.position(), pill.size(), Radius::from(radius::XS)), p.accent);
    let at = Point::new(pill.center_x(), pill.center_y());
    label(frame, &format_time(now), at, NOW_LABEL, p.on_accent, TextAlign::Center, Vertical::Center);
}

/// `b` laid over `a` at opacity `t` (0 = `a`, 1 = `b`), opaque.
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t)
}

/// A translucent `top` composed over the opaque `under`.
fn over(top: Color, under: Color) -> Color {
    mix(under, top, top.a)
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

    #[test]
    fn labels_thin_out_when_hours_are_narrow() {
        assert_eq!(label_step(94.0), 1);
        assert_eq!(label_step(34.0), 2);
        assert_eq!(label_step(12.0), 4);
    }

    #[test]
    fn ellipsize_cuts_long_titles_only() {
        assert_eq!(ellipsize("Standup", typo::CAPTION, 200.0), "Standup");
        let cut = ellipsize("Quarterly planning with the whole team", typo::CAPTION, 100.0);
        assert!(cut.ends_with(ELLIPSIS), "{cut}");
        assert!(text_width(&cut, typo::CAPTION) <= 100.0);
        assert!(!cut.contains(" …"), "no space before the ellipsis");
        assert_eq!(ellipsize("Standup", typo::CAPTION, 5.0), "");
    }

    #[test]
    fn rows_fit_the_height_of_the_tile() {
        // 240×160: the lead and one more row.
        assert_eq!(rows_that_fit(160.0, 1), 1);
        assert_eq!(rows_that_fit(100.0, 1), 0);
        assert!(rows_that_fit(480.0, 2) >= 10);
        assert_eq!(shown_rows(3, 5), (3, None));
        assert_eq!(shown_rows(6, 3), (2, Some(4)), "the last row says how many are left out");
        assert_eq!(shown_rows(2, 0), (0, None));
        assert_eq!(shown_rows(3, 1), (1, None), "one row rather than only a count");
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
