//! Day and week view: a time grid (task K.9 in `docs/tasks/kalender-aufgaben.md`), drawn after
//! Linear (task C.2 in `docs/tasks/design.md`, mock-up `docs/prototypes/linear/calendar.html`).
//!
//! Columns for the days in `[start, end)` (one for the day view, seven for the week) under a
//! header "Mon 5" (today: the number in a red circle), a strip of all-day chips, and the hours
//! 0–24 down the time axis (scrolled so 07:00 is on top). Timed events are tinted blocks with a
//! bar in their calendar colour (overlapping ones side by side); past ones are faded; gaps of two
//! hours or more between the events of today and the days to come say "2 h free"; a red line
//! with its time in the axis marks now. Weekends are set off a little; in the week, today's
//! column (header, all-day strip and hours) is washed with a trace of the accent, so the eye
//! finds it at once.
//!
//! Click on an empty slot → `NewEvent { date, time }` (time rounded down to the half hour), on
//! an all-day row cell → `NewEvent { date, time: None }`, on an event → `OpenEvent(index)`. The
//! event open in the side panel (`ViewData::selected`) is outlined with the focus ring.
//!
//! In the wide layout the day view's properties column lists what is left of the day
//! ([`day_props`], "Up next"); a click on an entry opens it like a click on its block.
//!
//! Every colour comes from the theme's [`Palette`] at draw time, so light and dark both work.
//! The geometry (time ↔ y, splitting at midnight, lanes, free gaps, hit-testing) is in
//! `week/layout.rs`.

pub(super) mod layout;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use iced::alignment::{Horizontal, Vertical};
use iced::border::{self, Radius};
use iced::widget::canvas::{self, Action, Frame, Path, Stroke};
use iced::widget::scrollable::{AbsoluteOffset, Direction};
use iced::widget::text::{Alignment, Style as TextStyle};
use iced::widget::{Id, Space, column, container, responsive, row, rule, scrollable};
use iced::{Background, Color, Element, Event, Length, Padding, Point, Rectangle, Size, Task, Theme, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_time, to_local, weekday_name};
use torro_gui::theme::{self, Palette, mix, over, palette, radius, size, space};
use torro_gui::typo::{ellipsize, text_width};
use torro_gui::widgets::{self, Mark};
use torro_gui::{icon, style, typo};
use torro_pim::{Event as CalendarEvent, EventTime};

use layout::{GRID_HEIGHT, GridHit, HOUR_HEIGHT, StripHit};

use super::{ViewData, short_date};
use crate::app::Message;

/// Id of the time grid's scrollable, used by [`scroll_to_morning`].
pub const SCROLL_ID: Id = Id::new("torrocalendar.week.grid");

const UNTITLED: &str = "(untitled)";

/// The day number in the header: 13/500 with tabular figures (`.dayhead b`).
pub(super) const DAY_NUMBER: typo::Level = typo::BODY_STRONG.tabular();
/// Edge of the box around a day number, and of today's red circle (`.dayhead b`, 24).
pub(super) const DAY_CIRCLE: f32 = space::XL;
/// Width of the colour bar on the left of events and all-day chips (`inset 3px 0 0`).
pub(super) const BAR: f32 = 3.0;
/// Text inside events and chips starts this far from their left edge: the bar plus 6
/// (`.ev{padding:3px 6px 3px 9px}`).
const TEXT_LEFT: f32 = 9.0;
/// Space kept free at the right of the text in events and chips.
const TEXT_RIGHT: f32 = 6.0;
/// Space above the first line of text in an event.
const TEXT_TOP: f32 = 3.0;
/// Width of "09:30" in `MICRO` (tabular figures, so the same for every time) plus the gap of 6
/// before the title in a one-line event (`.ev.short{gap:6px}`).
const SHORT_TIME_WIDTH: f32 = 36.0;
/// A one-line event keeps its start time in front of the title only while at least this many
/// letters of the title fit beside it; otherwise the title alone is worth more than the time.
const MIN_TITLE_LETTERS: usize = 4;
/// Past events and chips are drawn at this opacity over their background (`.ev.past`).
const PAST_OPACITY: f32 = 0.55;
/// Weekend columns get this much of the window background over the panel (`.daycols > .weekend`).
const WEEKEND_SHADE: f32 = 0.35;
/// Today's column in the week gets this much of the accent over its ground: enough to find it at
/// a glance, too little to compete with the events or the red of the day circle and now line.
/// Red shows far less on the near-black panel than on white, so dark mode takes more of it.
const TODAY_WASH_DARK: f32 = 0.10;
const TODAY_WASH_LIGHT: f32 = 0.05;
/// Gaps between events this long or longer show "2 h free" (minutes).
pub(super) const FREE_MINUTES: f32 = 120.0;
/// The time label of the now line: the axis figures at 500 on a 16-high pill, white on red
/// (`.nowlabel{font:500 11px/16px}`).
pub(super) const NOW_LABEL: typo::Level =
    typo::Level { font: theme::FONT_TABULAR_MEDIUM, line_height: typo::CAPTION.line_height, ..typo::MICRO };
/// Width of the now label: fits "23:59" with 3 on each side, and the narrow axis.
pub(super) const NOW_LABEL_WIDTH: f32 = size::AXIS_NARROW - 2.0 * space::XS;
/// Radius of the dot at the start of the now line (7 across, `.now::before`).
pub(super) const NOW_DOT: f32 = 3.5;
/// The ring around the event open in the side panel stands this far off its edge
/// (`.ev.sel{outline:2px solid var(--focus);outline-offset:1px}`).
const RING_OFFSET: f32 = size::HAIRLINE;

/// Where the grid opens: 07:00 at the top, with room above the line for its label (which is
/// centred on the line).
const MORNING: f32 = 7.0 * HOUR_HEIGHT - space::M;

/// Task that scrolls the time grid so that 07:00 is at the top, with room above the line for
/// its label (which is centred on the line). The app should return it when the day or week
/// view opens (switching to it, or navigating to a new anchor), e.g.
/// `views::week::scroll_to_morning()` from `App::update`.
///
/// In a grid too high to scroll that far the scrollable stops short of 07:00; [`morning_pad`]
/// makes sure it stops where no hour label is cut either.
pub fn scroll_to_morning<T>() -> Task<T> {
    iced::widget::operation::scroll_to(SCROLL_ID, AbsoluteOffset { x: 0.0, y: MORNING })
}

/// Empty space below 24:00 for a grid `viewport` high, so that the scroll to [`MORNING`] never
/// leaves an hour label cut at the top edge.
///
/// A viewport higher than `GRID_HEIGHT - MORNING` cannot scroll down to 07:00: Iced stops at the
/// end of the content, at an offset that falls anywhere between two hours and may halve the
/// label "06:00". The pad (less than an hour) lengthens the content just enough that this stop
/// lies as far above an hour line as [`MORNING`] lies above 07:00. Nothing is added when the
/// grid scrolls to 07:00 anyway, or when it does not scroll at all.
fn morning_pad(viewport: f32) -> f32 {
    let furthest = GRID_HEIGHT - viewport;
    if furthest >= MORNING || furthest <= 0.0 { 0.0 } else { (MORNING - furthest).rem_euclid(HOUR_HEIGHT) }
}

pub fn view(data: ViewData<'_>) -> Element<'_, Message> {
    let plan = Plan::new(&data);
    // The header has to leave room for the time axis, whose width follows the grid's width; the
    // pad under the grid follows its height.
    responsive(move |bounds| week(plan.clone(), bounds, true)).into()
}

/// The day view under the week strip of the narrow layout (`super::week_strip`, task K.19):
/// [`view`] for one day without the day header, which the strip above already shows.
pub fn day_under_strip(data: ViewData<'_>) -> Element<'_, Message> {
    let plan = Plan::new(&data);
    responsive(move |bounds| week(plan.clone(), bounds, false)).into()
}

/// The header (with `with_header`), the all-day strip and the scrolling grid for a view of
/// `bounds`.
fn week(plan: Plan, bounds: Size, with_header: bool) -> Element<'static, Message> {
    let axis = layout::axis_width(bounds.width);
    let header = with_header.then(|| header(&plan.days, plan.now.date(), axis));
    let strip_height = layout::strip_height(layout::strip_rows(&plan.bars));
    // Without a single event the strip says so, quietly, above an empty grid (design skill,
    // section 5: an empty view says what would be here). With events but none all day, an empty
    // strip would only be a band between the days and the hours: it is left out.
    let strip: Option<Element<'static, Message>> = match plan.empty_note() {
        Some(note) => Some(
            container(typo::text(typo::META, note).style(theme::text_3))
                .padding(Padding::ZERO.left(axis))
                .center_x(Length::Fill)
                .center_y(Length::Fixed(strip_height))
                .into(),
        ),
        None if plan.bars.is_empty() => None,
        None => Some(
            iced::widget::canvas(Strip { plan: plan.clone() })
                .width(Length::Fill)
                .height(Length::Fixed(strip_height))
                .into(),
        ),
    };
    // What the grid's scrollable gets: the view less the header, the strip and their lines.
    let header_height = if with_header { size::HEADER + size::HAIRLINE } else { 0.0 };
    let strip_height = if strip.is_some() { strip_height + size::HAIRLINE } else { 0.0 };
    let pad_above = header_height + strip_height;
    let pad = morning_pad(bounds.height - pad_above);
    let grid = iced::widget::canvas(Grid { plan }).width(Length::Fill).height(Length::Fixed(GRID_HEIGHT));
    let grid = scrollable(column![grid, Space::new().height(Length::Fixed(pad))])
        .id(SCROLL_ID)
        .direction(Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Length::Fill)
        .height(Length::Fill);
    let line = || rule::horizontal(1).style(style::rule);
    let mut parts = column![].height(Length::Fill);
    if let Some(header) = header {
        parts = parts.push(header).push(line());
    }
    if let Some(strip) = strip {
        parts = parts.push(strip).push(line());
    }
    parts.push(grid).into()
}

/// Everything the grid draws, built once per view. Entry ids are positions in `entries`.
#[derive(Debug, Clone)]
struct Plan {
    /// The day columns, first to last.
    days: Vec<NaiveDate>,
    /// One per item of the view data, in the same order.
    entries: Vec<Entry>,
    /// The timed pieces of the entries, per day column.
    timed: Vec<layout::TimedSegment>,
    /// The all-day bars.
    bars: Vec<layout::DaySpan>,
    now: NaiveDateTime,
    /// The app's index of the event open in the side panel.
    selected: Option<usize>,
}

#[derive(Debug, Clone)]
struct Entry {
    /// Index into the app's event list, for `OpenEvent`.
    index: usize,
    color: Color,
    /// Local start and (exclusive) end.
    start: NaiveDateTime,
    end: NaiveDateTime,
    title: String,
    location: Option<String>,
}

impl Entry {
    /// Over by `now`: drawn faded.
    fn past(&self, now: NaiveDateTime) -> bool {
        self.end <= now
    }

    /// "10:00 – 11:30", or just "10:00" for an entry without length.
    fn time_range(&self) -> String {
        if self.end > self.start {
            format!("{} – {}", format_time(self.start.time()), format_time(self.end.time()))
        } else {
            format_time(self.start.time())
        }
    }
}

impl Plan {
    fn new(data: &ViewData<'_>) -> Plan {
        let count = (data.end - data.start).num_days().max(1);
        let days: Vec<NaiveDate> = (0..count).map(|i| data.start + Duration::days(i)).collect();
        let columns = days.len();
        let mut plan = Plan {
            days,
            entries: Vec::new(),
            timed: Vec::new(),
            bars: Vec::new(),
            now: data.now,
            selected: data.selected,
        };
        for item in &data.items {
            let (start, end, all_day) = local_span(item.event);
            let id = plan.entries.len();
            let location = item.event.location.as_deref().map(str::trim).filter(|l| !l.is_empty());
            plan.entries.push(Entry {
                index: item.index,
                color: item.color,
                start,
                end,
                title: title(item.event),
                location: location.map(str::to_string),
            });
            if all_day {
                plan.bars.extend(layout::day_span(id, data.start, columns, start, end));
            } else {
                plan.timed.extend(layout::split_by_day(id, data.start, columns, start, end));
            }
        }
        plan
    }

    /// The quiet line of a view without any event: "No events this week", "No events today", or
    /// "No events" for another single day. `None` as soon as there is one.
    fn empty_note(&self) -> Option<&'static str> {
        if !self.entries.is_empty() {
            None
        } else if self.days.len() > 1 {
            Some(tr("No events this week"))
        } else if self.days.first() == Some(&self.now.date()) {
            Some(tr("No events today"))
        } else {
            Some(tr("No events"))
        }
    }

    /// Whether the entry `id` is the event open in the side panel.
    fn is_selected(&self, id: usize) -> bool {
        self.selected.is_some_and(|index| self.entries.get(id).is_some_and(|entry| entry.index == index))
    }

    fn today_column(&self) -> Option<usize> {
        self.days.iter().position(|day| *day == self.now.date())
    }

    /// Whether `column` is today in a view of several days: the column that is set off. The day
    /// view has nothing to set it off from.
    fn is_marked_today(&self, column: usize) -> bool {
        self.days.len() > 1 && self.today_column() == Some(column)
    }

    /// The ground of the day column `column` in the grid: the panel, a little darker (lighter)
    /// at the weekend, with the accent wash ([`today_wash`]) on today in the week.
    fn ground(&self, theme: &Theme, column: usize) -> Color {
        let p = palette(theme);
        let base = match self.days.get(column) {
            Some(day) if is_weekend(*day) => mix(p.panel, p.bg, WEEKEND_SHADE),
            _ => p.panel,
        };
        if self.is_marked_today(column) { today_wash(theme, base) } else { base }
    }

    /// The free gaps of `column` worth a note: today from now on, days to come in full, none
    /// on days gone by.
    fn free_gaps(&self, column: usize) -> Vec<(f32, f32)> {
        let Some(&day) = self.days.get(column) else {
            return Vec::new();
        };
        let today = self.now.date();
        let from = if day < today {
            return Vec::new();
        } else if day == today {
            layout::minutes_of(self.now.time())
        } else {
            0.0
        };
        layout::free_gaps(&self.timed, column, from, FREE_MINUTES)
    }
}

/// Start and end in local time, and whether the entry goes into the all-day strip: a
/// date-only entry, or one lasting a day or more.
pub(super) fn local_span(event: &CalendarEvent) -> (NaiveDateTime, NaiveDateTime, bool) {
    let start = match event.start {
        EventTime::Date(day) => day.and_time(NaiveTime::MIN),
        EventTime::DateTime(at) => to_local(at),
    };
    let raw_end = match event.end {
        Some(EventTime::Date(day)) => day.and_time(NaiveTime::MIN),
        Some(EventTime::DateTime(at)) => to_local(at),
        None => start,
    };
    let end = raw_end.max(start);
    let all_day = event.all_day() || end - start >= Duration::hours(24);
    (start, end, all_day)
}

pub(super) fn title(event: &CalendarEvent) -> String {
    let summary = event.summary.trim();
    if summary.is_empty() { tr(UNTITLED).to_string() } else { summary.to_string() }
}

/// `ground` with the trace of the accent that sets today's column off in the week.
fn today_wash(theme: &Theme, ground: Color) -> Color {
    let share = if theme.extended_palette().is_dark { TODAY_WASH_DARK } else { TODAY_WASH_LIGHT };
    mix(ground, palette(theme).accent, share)
}

fn is_weekend(day: NaiveDate) -> bool {
    matches!(day.weekday(), Weekday::Sat | Weekday::Sun)
}

/// The hand over everything a click does something on (an event, a bar, an empty slot or day
/// cell), the plain arrow elsewhere (the time axis). The same hit-test as the click, so the
/// cursor never promises more than a press does.
pub(super) fn pointer_if(clickable: bool) -> mouse::Interaction {
    if clickable { mouse::Interaction::Pointer } else { mouse::Interaction::Idle }
}

/// The left-button press in `bounds`, relative to its top left corner.
pub(super) fn left_press(event: &Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Point> {
    match event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor.position_in(bounds),
        _ => None,
    }
}

/// Day header (`.dayhead`): "Mon 5" per column, the weekday in `text_3`, the number in `text_2`
/// (weekends `text_3`); today's weekday in `text` and its number white in a red circle. With
/// several days, today's cell also gets the column's accent wash, so the band runs from the header
/// down through the hours.
///
/// With the narrow axis the columns are too slim for both side by side, so the weekday goes
/// above its number, as in the mock-up's week strip (`.strip`).
fn header(days: &[NaiveDate], today: NaiveDate, axis: f32) -> Element<'static, Message> {
    let stacked = axis < size::AXIS;
    let mut header = row![Space::new().width(Length::Fixed(axis))];
    let several = days.len() > 1;
    for &day in days {
        let is_today = day == today;
        let name =
            typo::text(typo::META, weekday_name(day, true)).style(if is_today { theme::text } else { theme::text_3 });
        let number = typo::text(DAY_NUMBER, day.day().to_string());
        let number = if is_today {
            container(number.style(|t: &Theme| TextStyle { color: Some(palette(t).on_accent) }))
                .center(DAY_CIRCLE)
                .style(|t| container::Style {
                    background: Some(Background::Color(palette(t).accent)),
                    border: border::rounded(DAY_CIRCLE / 2.0),
                    ..container::Style::default()
                })
        } else {
            container(number.style(if is_weekend(day) { theme::text_3 } else { theme::text_2 })).center(DAY_CIRCLE)
        };
        let label: Element<'static, Message> = if stacked {
            column![name, number].align_x(Horizontal::Center).into()
        } else {
            row![name, number].spacing(space::XS).align_y(Vertical::Center).into()
        };
        let marked = several && is_today;
        header = header.push(
            container(label)
                .width(Length::FillPortion(1))
                .height(size::HEADER)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .style(move |t| container::Style {
                    background: marked.then(|| Background::Color(today_wash(t, palette(t).panel))),
                    ..container::Style::default()
                }),
        );
    }
    header.into()
}

/// What a property row in [`day_props`] has room for: the column less its padding and the row's,
/// the dot and the gaps around the title.
const PROPS_ROOM: f32 = size::PROPS - 4.0 * space::S - icon::SIZE - 2.0 * space::S;

/// One entry of "Up next": an event of the shown day as the properties column lists it.
#[derive(Debug, Clone, PartialEq)]
struct Next {
    /// Index into the app's event list, for `OpenEvent`.
    index: usize,
    color: Color,
    title: String,
    /// "All day", "Now" for one running, or its start "15:00".
    when: String,
}

/// The events of the day view's day (`data.start`) for its properties column, all-day ones first,
/// then by start. On today only what has not ended yet: that is what comes next. On other days
/// the whole day.
fn up_next(data: &ViewData<'_>) -> Vec<Next> {
    let today = data.start == data.now.date();
    let mut found: Vec<(bool, NaiveDateTime, Next)> = data
        .items
        .iter()
        .filter_map(|item| {
            let (start, end, all_day) = local_span(item.event);
            // All-day events last the whole day, even those stored without an end.
            if today && !all_day && end <= data.now {
                return None;
            }
            let when = if all_day {
                tr("All day").to_string()
            } else if today && start <= data.now {
                tr("Now").to_string()
            } else {
                format_time(start.time())
            };
            let next = Next { index: item.index, color: item.color, title: title(item.event), when };
            Some((!all_day, start, next))
        })
        .collect();
    found.sort_by(|a, b| (a.0, a.1, &a.2.title).cmp(&(b.0, b.1, &b.2.title)));
    found.into_iter().map(|(_, _, next)| next).collect()
}

/// The properties column of the day view in the wide layout: "Up next" on today (the short date
/// on other days), a row per event with its colour dot, title and when; a click opens it. With
/// nothing (left) to show, a quiet line says so.
pub fn day_props(data: &ViewData<'_>) -> Element<'static, Message> {
    let today = data.start == data.now.date();
    let heading = if today { tr("Up next").to_string() } else { short_date(data.start) };
    let entries = up_next(data);
    let rows: Vec<Element<'static, Message>> = if entries.is_empty() {
        let calm = if today { tr("Nothing else today") } else { tr("No events") };
        vec![widgets::prop_row(None, typo::text(typo::BODY, calm).style(theme::text_3), None)]
    } else {
        entries.into_iter().map(next_row).collect()
    };
    widgets::prop_section(heading, rows)
}

/// A row of [`day_props`]: dot, title cut to the room left beside the time, the time at the right.
fn next_row(next: Next) -> Element<'static, Message> {
    let when_width = text_width(&next.when, typo::META);
    let title = ellipsize(&next.title, typo::BODY, PROPS_ROOM - when_width - space::S);
    let value = row![
        typo::text(typo::BODY, title).wrapping(iced::widget::text::Wrapping::None),
        Space::new().width(Length::Fill),
        typo::text(typo::META.tabular(), next.when).style(theme::text_3),
    ]
    .align_y(Vertical::Center);
    widgets::prop_row(Some(Mark::Dot(next.color)), value, Some(Message::OpenEvent(next.index)))
}

/// The all-day strip above the grid (`.allday`): a chip per entry, an empty cell in the axis.
struct Strip {
    plan: Plan,
}

impl canvas::Program<Message> for Strip {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let point = left_press(event, bounds, cursor)?;
        let count = self.plan.days.len();
        let (bars, _) = layout::bar_blocks(&self.plan.bars, count, bounds.width);
        let message = match layout::strip_hit(&bars, count, bounds.width, point)? {
            StripHit::Bar(id) => Message::OpenEvent(self.plan.entries.get(id)?.index),
            StripHit::Day(column) => Message::NewEvent { date: *self.plan.days.get(column)?, time: None },
        };
        Some(Action::publish(message).and_capture())
    }

    fn mouse_interaction(&self, _state: &(), bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        let count = self.plan.days.len();
        let (bars, _) = layout::bar_blocks(&self.plan.bars, count, bounds.width);
        let over = cursor.position_in(bounds).and_then(|point| layout::strip_hit(&bars, count, bounds.width, point));
        pointer_if(matches!(over, Some(layout::StripHit::Bar(_))))
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
        let count = plan.days.len();
        let (x0, col_w) = layout::columns(bounds.width, count);
        let height = bounds.height;

        // No "All day" in the axis: the mock-up has it, but 52 px do not hold the German
        // "Ganztägig", and a strip of chips between the day header and the hours explains itself
        // (the narrow mock-up leaves it out as well).
        fill(&mut frame, Rectangle::new(Point::ORIGIN, bounds.size()), p.panel);
        if let Some(column) = plan.today_column().filter(|&column| plan.is_marked_today(column)) {
            let x = x0 + column as f32 * col_w;
            fill(&mut frame, Rectangle::new(Point::new(x, 0.0), Size::new(col_w, height)), today_wash(theme, p.panel));
        }
        for column in 0..count {
            let x = x0 + column as f32 * col_w;
            line(&mut frame, Point::new(x, 0.0), Point::new(x, height), p.line);
        }

        let (bars, _) = layout::bar_blocks(&plan.bars, count, bounds.width);
        for bar in &bars {
            if let Some(entry) = plan.entries.get(bar.id) {
                let tone = Tone::of(p, entry.color, p.panel, entry.past(plan.now));
                surface(&mut frame, bar.rect, &tone);
                frame.with_clip(text_clip(bar.rect), |frame| {
                    let at = Point::new(bar.rect.x + TEXT_LEFT, bar.rect.center_y());
                    let title = ellipsize(&entry.title, typo::CAPTION, text_room(bar.rect));
                    label(frame, &title, at, typo::CAPTION, tone.title, Alignment::Left, Vertical::Center);
                });
            }
        }
        for bar in bars.iter().filter(|bar| plan.is_selected(bar.id)) {
            selection_ring(&mut frame, bar.rect, p);
        }
        vec![frame.into_geometry()]
    }
}

/// The hour grid with the timed blocks, the free gaps and the now line.
struct Grid {
    plan: Plan,
}

impl canvas::Program<Message> for Grid {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let point = left_press(event, bounds, cursor)?;
        let count = self.plan.days.len();
        let blocks = layout::timed_blocks(&self.plan.timed, count, bounds.width);
        let message = match layout::grid_hit(&blocks, count, bounds.width, point)? {
            GridHit::Event(id) => Message::OpenEvent(self.plan.entries.get(id)?.index),
            GridHit::Slot { column, time } => {
                Message::NewEvent { date: *self.plan.days.get(column)?, time: Some(time) }
            }
        };
        Some(Action::publish(message).and_capture())
    }

    fn mouse_interaction(&self, _state: &(), bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        let count = self.plan.days.len();
        let blocks = layout::timed_blocks(&self.plan.timed, count, bounds.width);
        let over = cursor.position_in(bounds).and_then(|point| layout::grid_hit(&blocks, count, bounds.width, point));
        // The hand only over events; empty slots stay calm although a click there starts a new one.
        pointer_if(matches!(over, Some(layout::GridHit::Event(_))))
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
        let count = plan.days.len();
        let width = bounds.width;
        let (x0, col_w) = layout::columns(width, count);
        let column_x = |column: usize| x0 + column as f32 * col_w;
        let under = |column: usize| plan.ground(theme, column);
        let now_y = plan.today_column().map(|_| layout::y_of(plan.now.time()));

        fill(&mut frame, Rectangle::new(Point::ORIGIN, Size::new(width, GRID_HEIGHT)), p.panel);
        for column in 0..count {
            let ground = under(column);
            if ground != p.panel {
                fill(
                    &mut frame,
                    Rectangle::new(Point::new(column_x(column), 0.0), Size::new(col_w, GRID_HEIGHT)),
                    ground,
                );
            }
        }

        // Inner lines only: no line at 00:00 and 24:00 (the edges), none right of the last day.
        for hour in 1..24u32 {
            let y = hour as f32 * HOUR_HEIGHT;
            line(&mut frame, Point::new(x0, y), Point::new(width, y), p.line);
            // The now label sits in the axis; an hour label right under it would be unreadable.
            let covered = now_y.is_some_and(|now| (now - y).abs() < line_height(NOW_LABEL));
            if !covered {
                let at = Point::new(x0 - space::S, y);
                label(
                    &mut frame,
                    &format!("{hour:02}:00"),
                    at,
                    typo::MICRO,
                    p.text_3,
                    Alignment::Right,
                    Vertical::Center,
                );
            }
        }
        for column in 0..count {
            let x = column_x(column);
            line(&mut frame, Point::new(x, 0.0), Point::new(x, GRID_HEIGHT), p.line);
        }

        for column in 0..count {
            for (start, end) in plan.free_gaps(column) {
                let (top, bottom) = layout::y_range(start, end);
                let hours = ((end - start) / 60.0).floor() as u32;
                let at = Point::new(column_x(column) + col_w / 2.0, (top + bottom) / 2.0);
                let note = trf("{n} h free", &[("n", &hours)]);
                label(&mut frame, &note, at, typo::META, p.text_3, Alignment::Center, Vertical::Center);
            }
        }

        let blocks = layout::timed_blocks(&plan.timed, count, width);
        for block in &blocks {
            let Some(entry) = plan.entries.get(block.id) else { continue };
            let column = layout::column_at(block.rect.x, count, width).unwrap_or_default();
            let tone = Tone::of(p, entry.color, under(column), entry.past(plan.now));
            surface(&mut frame, block.rect, &tone);
            // The day view has the width for the place; seven columns do not.
            event_text(&mut frame, block.rect, entry, &tone, count == 1);
        }
        // Over the blocks, so a neighbour in the next lane does not cover it.
        for block in blocks.iter().filter(|block| plan.is_selected(block.id)) {
            selection_ring(&mut frame, block.rect, p);
        }

        if let Some(y) = now_y {
            now_line(&mut frame, p, x0, width, y, plan.now.time());
        }
        vec![frame.into_geometry()]
    }
}

/// The colours of one event or chip, already composed over what lies under it: tinted fill,
/// full-colour bar, title in `text`, time in `text_2`, place in `text_3`. A past entry is faded
/// towards its background as a whole (55 %), like CSS `opacity` – composed by hand, because a
/// canvas cannot fade a group of shapes.
pub(super) struct Tone {
    pub(super) fill: Color,
    pub(super) bar: Color,
    pub(super) title: Color,
    pub(super) time: Color,
    place: Color,
}

impl Tone {
    pub(super) fn of(p: &Palette, color: Color, under: Color, past: bool) -> Tone {
        // The tint is laid over the panel, not the weekend shade, so an event looks the same on
        // every day (`color-mix(var(--c) var(--tint), var(--panel))`).
        let fill = over(p.tinted(color), p.panel);
        let fade = |c: Color| if past { mix(under, c, PAST_OPACITY) } else { c };
        Tone { fill: fade(fill), bar: fade(color), title: fade(p.text), time: fade(p.text_2), place: fade(p.text_3) }
    }
}

/// The surface of an event or chip: rounded `radius::XS`, no border, the bar on the left. The
/// bar is the whole shape in the calendar colour with the tinted body laid over all but its
/// first 3 px, which clips the bar to the rounded corners as CSS's inset shadow does.
pub(super) fn surface(frame: &mut Frame, rect: Rectangle, tone: &Tone) {
    frame.fill(&Path::rounded_rectangle(rect.position(), rect.size(), Radius::from(radius::XS)), tone.bar);
    let body = Size::new((rect.width - BAR).max(0.0), rect.height);
    let body_radius = Radius::new(0.0).right(radius::XS);
    frame.fill(&Path::rounded_rectangle(Point::new(rect.x + BAR, rect.y), body, body_radius), tone.fill);
}

/// The outline of the event open in the side panel: a ring in `focus_ring`, as wide as the focus
/// ring of the controls, [`RING_OFFSET`] off the block's edge and rounded along with it. Neutral
/// like every selection; the calendar colour stays the block's own.
pub(super) fn selection_ring(frame: &mut Frame, rect: Rectangle, p: &Palette) {
    let grow = RING_OFFSET + style::FOCUS_RING_WIDTH / 2.0;
    let ring = Rectangle {
        x: rect.x - grow,
        y: rect.y - grow,
        width: rect.width + 2.0 * grow,
        height: rect.height + 2.0 * grow,
    };
    let path = Path::rounded_rectangle(ring.position(), ring.size(), Radius::from(radius::XS + grow));
    frame.stroke(&path, Stroke::default().with_color(p.focus_ring).with_width(style::FOCUS_RING_WIDTH));
}

/// Where the text of an event or chip may go: inside it, short of its right edge.
fn text_clip(rect: Rectangle) -> Rectangle {
    Rectangle { width: (rect.width - TEXT_RIGHT).max(0.0), ..rect }
}

/// The width the text of an event or chip has: from its text edge to short of its right edge.
fn text_room(rect: Rectangle) -> f32 {
    (rect.width - TEXT_LEFT - TEXT_RIGHT).max(0.0)
}

/// Smallest event height that takes the title and the time on two lines.
fn two_lines_fit(height: f32) -> bool {
    height >= TEXT_TOP + line_height(typo::CAPTION) + line_height(typo::MICRO)
}

/// The time line under the title in `room`: the range "10:00 – 11:30", or the start alone when
/// the range does not fit (a narrow lane), cut with "…" as a last resort.
fn time_line(entry: &Entry, room: f32) -> String {
    let range = entry.time_range();
    if text_width(&range, typo::MICRO) <= room {
        range
    } else {
        ellipsize(&format_time(entry.start.time()), typo::MICRO, room)
    }
}

/// The one line of a short event in `room`: the start time and the title cut with "…" to the
/// rest ("09:30 Daily St…"), or – when fewer than [`MIN_TITLE_LETTERS`] of the title would be
/// left – the title alone, since a bare "14:30" does not say what happens.
fn short_line(title: &str, time: String, room: f32) -> (Option<String>, String) {
    if text_width(title, typo::CAPTION) <= room - SHORT_TIME_WIDTH {
        return (Some(time), title.to_string());
    }
    let cut = ellipsize(title, typo::CAPTION, room - SHORT_TIME_WIDTH);
    let letters = cut.trim_end_matches(typo::ELLIPSIS).chars().count();
    if letters >= MIN_TITLE_LETTERS.min(title.chars().count()) && !cut.is_empty() {
        (Some(time), cut)
    } else {
        (None, ellipsize(title, typo::CAPTION, room))
    }
}

/// The text of a timed event. Title (`CAPTION`) over the time range (`MICRO`), and the place
/// below when `with_place` and there is room; a block too low for two lines (under half an
/// hour) gets one: "09:30 Daily standup" ([`short_line`]). Never wrapped: in a narrow tile
/// wrapping would break words into single letters; every line is cut with "…" to the block.
fn event_text(frame: &mut Frame, rect: Rectangle, entry: &Entry, tone: &Tone, with_place: bool) {
    let x = rect.x + TEXT_LEFT;
    let room = text_room(rect);
    frame.with_clip(text_clip(rect), |frame| {
        if two_lines_fit(rect.height) {
            let mut y = rect.y + TEXT_TOP;
            let title = ellipsize(&entry.title, typo::CAPTION, room);
            label(frame, &title, Point::new(x, y), typo::CAPTION, tone.title, Alignment::Left, Vertical::Top);
            y += line_height(typo::CAPTION);
            let time = time_line(entry, room);
            label(frame, &time, Point::new(x, y), typo::MICRO, tone.time, Alignment::Left, Vertical::Top);
            y += line_height(typo::MICRO);
            let fits = y + line_height(typo::META) <= rect.y + rect.height;
            if let Some(place) = entry.location.as_deref().filter(|_| with_place && fits) {
                let place = ellipsize(place, typo::META, room);
                label(frame, &place, Point::new(x, y), typo::META, tone.place, Alignment::Left, Vertical::Top);
            }
        } else {
            let y = rect.center_y();
            let (time, title) = short_line(&entry.title, format_time(entry.start.time()), room);
            let mut title_x = x;
            if let Some(time) = time {
                label(frame, &time, Point::new(x, y), typo::MICRO, tone.time, Alignment::Left, Vertical::Center);
                title_x += SHORT_TIME_WIDTH;
            }
            label(frame, &title, Point::new(title_x, y), typo::CAPTION, tone.title, Alignment::Left, Vertical::Center);
        }
    });
}

/// Now: a 1-px red line across the days with a dot where it leaves the axis, and the time as
/// a red pill with white figures in the axis (`.now`, `.nowlabel`).
fn now_line(frame: &mut Frame, p: &Palette, x0: f32, width: f32, y: f32, now: NaiveTime) {
    line(frame, Point::new(x0, y), Point::new(width, y), p.accent);
    frame.fill(&Path::circle(Point::new(x0, y), NOW_DOT), p.accent);
    let pill_height = line_height(NOW_LABEL);
    let pill = Rectangle::new(
        Point::new(x0 - space::XS - NOW_LABEL_WIDTH, y - pill_height / 2.0),
        Size::new(NOW_LABEL_WIDTH, pill_height),
    );
    frame.fill(&Path::rounded_rectangle(pill.position(), pill.size(), Radius::from(radius::XS)), p.accent);
    let at = Point::new(pill.center_x(), y);
    label(frame, &format_time(now), at, NOW_LABEL, p.on_accent, Alignment::Center, Vertical::Center);
}

pub(super) fn line_height(level: typo::Level) -> f32 {
    level.line_height.to_absolute(level.size).0
}

pub(super) fn fill(frame: &mut Frame, rect: Rectangle, color: Color) {
    frame.fill_rectangle(rect.position(), rect.size(), color);
}

/// A 1-px line.
pub(super) fn line(frame: &mut Frame, from: Point, to: Point, color: Color) {
    frame.stroke(&Path::line(from, to), Stroke::default().with_color(color).with_width(1.0));
}

/// One line of text in `level` (size, line height and Inter cut), never wrapped.
pub(super) fn label(
    frame: &mut Frame,
    content: &str,
    position: Point,
    level: typo::Level,
    color: Color,
    align_x: Alignment,
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
    use iced::widget::canvas::Program;

    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("date").and_hms_opt(hour, minute, 0).expect("time")
    }

    fn entry(start: NaiveDateTime, end: NaiveDateTime) -> Entry {
        Entry { index: 0, color: Color::BLACK, start, end, title: "Standup".into(), location: None }
    }

    #[test]
    fn entries_end_in_the_past_or_not() {
        let now = at(7, 14, 20);
        assert!(entry(at(7, 9, 30), at(7, 9, 45)).past(now));
        assert!(entry(at(7, 13, 0), at(7, 14, 20)).past(now), "ending right now is over");
        assert!(!entry(at(7, 14, 0), at(7, 15, 0)).past(now), "running");
        assert!(!entry(at(8, 9, 0), at(8, 10, 0)).past(now));
    }

    #[test]
    fn time_range_shows_start_and_end() {
        assert_eq!(entry(at(7, 10, 0), at(7, 11, 30)).time_range(), "10:00 – 11:30");
        assert_eq!(entry(at(7, 10, 0), at(7, 10, 0)).time_range(), "10:00");
    }

    #[test]
    fn weekend_is_saturday_and_sunday() {
        // 10 and 11 October 2026 are Saturday and Sunday.
        let days: Vec<bool> = (5..=11).map(|d| is_weekend(at(d, 0, 0).date())).collect();
        assert_eq!(days, [false, false, false, false, false, true, true]);
    }

    #[test]
    fn short_events_get_one_line() {
        // Half an hour less the gap is 22 px: too low for title and time.
        assert!(!two_lines_fit(HOUR_HEIGHT / 2.0 - 2.0));
        assert!(two_lines_fit(HOUR_HEIGHT * 0.75 - 2.0));
        assert!(two_lines_fit(HOUR_HEIGHT - 2.0));
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
            transparency: torro_pim::Transparency::Opaque,
        }
    }

    fn local(day: u32, hour: u32, minute: u32) -> EventTime {
        EventTime::DateTime(at(day, hour, minute).and_local_timezone(chrono::Local).single().expect("local").to_utc())
    }

    #[test]
    fn up_next_lists_what_is_left_of_today_and_all_of_another_day() {
        let events = [
            calendar_event("Review", local(7, 16, 0), Some(local(7, 17, 0))),
            calendar_event("Standup", local(7, 9, 30), Some(local(7, 9, 45))),
            calendar_event("Holiday", EventTime::Date(at(7, 0, 0).date()), None),
            calendar_event("Workshop", local(7, 14, 0), Some(local(7, 15, 0))),
        ];
        let items =
            events.iter().enumerate().map(|(index, event)| super::super::Item { index, event, color: Color::BLACK });
        let day = at(7, 0, 0).date();
        let mut data = ViewData {
            items: items.collect(),
            start: day,
            end: day + Duration::days(1),
            anchor: day,
            now: at(7, 14, 20),
            selected: None,
        };
        let listed =
            |data: &ViewData<'_>| up_next(data).into_iter().map(|next| (next.title, next.when)).collect::<Vec<_>>();
        let all_day = tr("All day").to_string();
        assert_eq!(
            listed(&data),
            [
                ("Holiday".into(), all_day.clone()),
                ("Workshop".into(), tr("Now").to_string()),
                ("Review".into(), "16:00".to_string()),
            ],
            "the standup is over"
        );
        data.now = at(6, 18, 0);
        assert_eq!(listed(&data).len(), 4, "seen from the day before, the whole day comes");
        assert_eq!(listed(&data)[1], ("Standup".into(), "09:30".to_string()));
        let _: Element<'_, Message> = day_props(&data);
    }

    #[test]
    fn an_empty_grid_says_so_for_its_week_or_day() {
        let monday = at(5, 0, 0).date();
        let empty = |start: NaiveDate, days: i64| {
            let data = ViewData {
                items: Vec::new(),
                start,
                end: start + Duration::days(days),
                anchor: start,
                now: at(7, 14, 20),
                selected: None,
            };
            Plan::new(&data).empty_note()
        };
        assert_eq!(empty(monday, 7), Some(tr("No events this week")));
        assert_eq!(empty(at(7, 0, 0).date(), 1), Some(tr("No events today")));
        assert_eq!(empty(at(8, 0, 0).date(), 1), Some(tr("No events")));
        let event = calendar_event("Standup", local(7, 9, 30), Some(local(7, 9, 45)));
        let items = vec![super::super::Item { index: 0, event: &event, color: Color::BLACK }];
        let data = ViewData {
            items,
            start: monday,
            end: monday + Duration::days(7),
            anchor: monday,
            now: at(7, 14, 20),
            selected: None,
        };
        assert_eq!(Plan::new(&data).empty_note(), None);
    }

    #[test]
    fn only_today_in_the_week_is_washed() {
        let plan = |start: NaiveDate, days: i64, now: NaiveDateTime| {
            let data = ViewData {
                items: Vec::new(),
                start,
                end: start + Duration::days(days),
                anchor: start,
                now,
                selected: None,
            };
            Plan::new(&data)
        };
        let dark = theme::theme();
        let (t, p) = (&dark, Palette::dark());
        let monday = at(5, 0, 0).date();
        // Wednesday 7 October: the third column, washed; the weekend keeps its shade.
        let week = plan(monday, 7, at(7, 14, 20));
        let washed: Vec<bool> = (0..7).map(|column| week.is_marked_today(column)).collect();
        assert_eq!(washed, [false, false, true, false, false, false, false]);
        assert_eq!(week.ground(t, 2), today_wash(t, p.panel));
        assert_ne!(week.ground(t, 2), p.panel);
        assert_eq!(week.ground(t, 1), p.panel);
        assert_eq!(week.ground(t, 5), mix(p.panel, p.bg, WEEKEND_SHADE));
        // Today on a Sunday: the wash over the weekend shade.
        let sunday = plan(monday, 7, at(11, 9, 0));
        assert_eq!(sunday.ground(t, 6), today_wash(t, mix(p.panel, p.bg, WEEKEND_SHADE)));
        // Another week, and the day view of today: nothing to set off.
        let next = plan(monday + Duration::days(7), 7, at(7, 14, 20));
        assert!((0..7).all(|column| !next.is_marked_today(column)));
        let day = plan(at(7, 0, 0).date(), 1, at(7, 14, 20));
        assert!(!day.is_marked_today(0));
        assert_eq!(day.ground(t, 0), p.panel);
        // Light mode washes too, more gently.
        let light = theme::light();
        let white = Palette::light().panel;
        assert_ne!(week.ground(&light, 2), white);
        assert_eq!(week.ground(&light, 1), white);
    }

    #[test]
    fn the_morning_scroll_never_halves_an_hour_label() {
        // Low grids scroll to 07:00 as asked, grids higher than the day do not scroll: no pad.
        assert_eq!(morning_pad(400.0), 0.0);
        assert_eq!(morning_pad(GRID_HEIGHT - MORNING), 0.0);
        assert_eq!(morning_pad(GRID_HEIGHT + 100.0), 0.0);
        // In between the scroll stops at the end of grid and pad, which must lie as far above an
        // hour line as the morning offset lies above 07:00 – whatever the height.
        for viewport in [830.0, 852.0, 880.0, 900.0, 1000.0, 1100.0, 1150.0] {
            let pad = morning_pad(viewport);
            assert!((0.0..HOUR_HEIGHT).contains(&pad), "{viewport}: {pad}");
            let stop = (GRID_HEIGHT + pad - viewport).min(MORNING);
            let below_line = stop.rem_euclid(HOUR_HEIGHT);
            assert!((below_line - MORNING.rem_euclid(HOUR_HEIGHT)).abs() < 1e-3, "{viewport}: stops at {stop}");
        }
    }

    #[test]
    fn short_lines_keep_the_time_while_the_title_has_room() {
        let time = || "09:30".to_string();
        let wide = short_line("Daily Standup", time(), 400.0);
        assert_eq!(wide, (Some(time()), "Daily Standup".to_string()));
        // A week column of an 800-wide window: the title is cut with "…", the time stays.
        let (lead, title) = short_line("Daily Standup", time(), 88.0);
        assert_eq!(lead, Some(time()));
        assert!(title.ends_with(typo::ELLIPSIS) && title.starts_with("Dai"), "{title}");
        // A half lane: no room for the title beside the time, so the title goes alone.
        let (lead, title) = short_line("1:1 with Lena", time(), 50.0);
        assert_eq!(lead, None);
        assert!(title.starts_with("1:1"), "{title}");
    }

    #[test]
    fn narrow_blocks_show_the_start_instead_of_a_cut_range() {
        let entry = entry(at(7, 13, 0), at(7, 15, 0));
        assert_eq!(time_line(&entry, 200.0), "13:00 – 15:00");
        assert_eq!(time_line(&entry, 50.0), "13:00");
    }

    #[test]
    fn tones_follow_the_theme() {
        for p in [Palette::dark(), Palette::light()] {
            let blue = p.calendar[0];
            let tone = Tone::of(p, blue, p.panel, false);
            assert_eq!((tone.bar, tone.title, tone.time), (blue, p.text, p.text_2));
            assert_eq!(tone.fill, over(p.tinted(blue), p.panel));
            assert_eq!(tone.fill.a, 1.0, "composed, so the bar does not shine through");
        }
    }

    #[test]
    fn past_tones_fade_towards_the_background() {
        let p = Palette::light();
        let (now, past) = (Tone::of(p, p.calendar[0], p.panel, false), Tone::of(p, p.calendar[0], p.panel, true));
        // Light mode: text is dark on a white panel, so faded text is lighter.
        assert!(past.title.r > now.title.r);
        assert!((past.title.r - mix(p.panel, p.text, PAST_OPACITY).r).abs() < 1e-6);
        assert_ne!(past.bar, now.bar);
    }

    #[test]
    fn free_gaps_only_from_today_on() {
        let blue = Color::BLACK;
        let event = |start, end| CalendarEvent {
            uid: String::new(),
            collection: String::new(),
            href: None,
            etag: None,
            summary: "x".into(),
            start: EventTime::DateTime(start),
            end: Some(EventTime::DateTime(end)),
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
        };
        let utc = |day, hour| at(day, hour, 0).and_local_timezone(chrono::Local).single().expect("local").to_utc();
        // Monday 5 (past) and Thursday 8 (to come) both have 10:00 and 16:00.
        let events: Vec<CalendarEvent> = [5, 8]
            .into_iter()
            .flat_map(|day| [event(utc(day, 10), utc(day, 11)), event(utc(day, 16), utc(day, 17))])
            .collect();
        let items =
            events.iter().enumerate().map(|(index, event)| super::super::Item { index, event, color: blue }).collect();
        let start = at(5, 0, 0).date();
        let data = ViewData {
            items,
            start,
            end: start + Duration::days(7),
            anchor: start,
            now: at(7, 14, 20),
            selected: None,
        };
        let plan = Plan::new(&data);
        assert_eq!(plan.free_gaps(0), vec![], "Monday is over");
        assert_eq!(plan.free_gaps(3), vec![(660.0, 960.0)], "Thursday 11:00–16:00");
    }

    /// A week (Mon 5 – Sun 11 Oct 2026) with a timed event on Wednesday and an all-day one.
    fn week_plan() -> Plan {
        let events = [
            calendar_event("Workshop", local(7, 10, 0), Some(local(7, 11, 0))),
            calendar_event("Holiday", EventTime::Date(at(7, 0, 0).date()), None),
        ];
        let items =
            events.iter().enumerate().map(|(index, event)| super::super::Item { index, event, color: Color::BLACK });
        let monday = at(5, 0, 0).date();
        let data = ViewData {
            items: items.collect(),
            start: monday,
            end: monday + Duration::days(7),
            anchor: monday,
            now: at(7, 14, 20),
            selected: None,
        };
        Plan::new(&data)
    }

    /// What the canvas shows with the cursor at `point` inside bounds that start at (30, 40).
    fn interaction(program: &impl canvas::Program<Message, State = ()>, point: Point) -> mouse::Interaction {
        let bounds = Rectangle::new(Point::new(30.0, 40.0), Size::new(700.0, GRID_HEIGHT + 100.0));
        let cursor = mouse::Cursor::Available(Point::new(bounds.x + point.x, bounds.y + point.y));
        program.mouse_interaction(&(), bounds, cursor)
    }

    #[test]
    fn the_grid_shows_the_hand_only_over_events() {
        let plan = week_plan();
        let grid = Grid { plan: plan.clone() };
        let blocks = layout::timed_blocks(&plan.timed, plan.days.len(), 700.0);
        let block = blocks.first().expect("the workshop is drawn").rect;
        assert_eq!(interaction(&grid, block.center()), mouse::Interaction::Pointer, "an event");
        let slot = Point::new(block.x + 1.0, 3.0 * HOUR_HEIGHT);
        assert_eq!(interaction(&grid, slot), mouse::Interaction::Idle, "an empty slot stays calm");
        let axis = Point::new(layout::columns(700.0, 7).0 / 2.0, block.center().y);
        assert_eq!(interaction(&grid, axis), mouse::Interaction::Idle, "the time axis does nothing");
        let below = Point::new(block.x, GRID_HEIGHT + 1.0);
        assert_eq!(interaction(&grid, below), mouse::Interaction::Idle, "beyond 24:00");
    }

    #[test]
    fn the_grid_shows_no_hand_with_the_cursor_elsewhere() {
        let grid = Grid { plan: week_plan() };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(700.0, 400.0));
        let outside = mouse::Cursor::Available(Point::new(900.0, 100.0));
        assert_eq!(grid.mouse_interaction(&(), bounds, outside), mouse::Interaction::Idle);
        assert_eq!(grid.mouse_interaction(&(), bounds, mouse::Cursor::Unavailable), mouse::Interaction::Idle);
    }

    #[test]
    fn only_the_event_open_in_the_panel_is_selected() {
        let mut plan = week_plan();
        assert!(!plan.is_selected(0) && !plan.is_selected(1), "nothing open");
        plan.selected = Some(1);
        assert!(!plan.is_selected(0));
        assert!(plan.is_selected(1), "the holiday, in the all-day strip");
        assert!(!plan.is_selected(7), "no such entry");
    }

    #[test]
    fn the_all_day_strip_shows_the_hand_only_over_bars() {
        let plan = week_plan();
        let strip = Strip { plan: plan.clone() };
        let (bars, _) = layout::bar_blocks(&plan.bars, plan.days.len(), 700.0);
        let bar = bars.first().expect("the holiday is drawn").rect;
        assert_eq!(interaction(&strip, bar.center()), mouse::Interaction::Pointer, "a bar");
        let day = Point::new(bar.x - 2.0 * layout::columns(700.0, 7).1, bar.y);
        assert_eq!(interaction(&strip, day), mouse::Interaction::Idle, "an empty day cell stays calm");
        let axis = Point::new(layout::columns(700.0, 7).0 / 2.0, bar.y);
        assert_eq!(interaction(&strip, axis), mouse::Interaction::Idle, "the axis cell does nothing");
    }
}
