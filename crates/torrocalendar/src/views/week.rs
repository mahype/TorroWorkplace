//! Day and week view: a time grid (task K.9 in `docs/tasks/kalender-aufgaben.md`).
//!
//! Columns for the days in `[start, end)` (one for the day view, seven for the week),
//! hours 0–24 down the side (scrolled so 07:00 is on top), a row for all-day events above
//! the grid, timed events as coloured blocks (overlapping ones side by side), a line at
//! `now` in today's column. Click on an empty slot → `NewEvent { date, time }` (time
//! rounded down to the half hour), on an all-day row cell → `NewEvent { date, time: None }`,
//! on an event → `OpenEvent(index)`.
//!
//! The geometry (time ↔ y, splitting at midnight, lanes, hit-testing) is in `week/layout.rs`.

mod layout;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use iced::alignment::{Horizontal, Vertical};
use iced::border::Radius;
use iced::widget::canvas::{self, Action, Frame, Path, Stroke};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::text::Alignment;
use iced::widget::{Id, Space, column, container, row, scrollable, text};
use iced::{Color, Element, Event, Length, Pixels, Point, Rectangle, Size, Task, mouse};
use torro_core::theme::TORRO_RED;
use torro_gui::datetime::{format_time, to_local, weekday_name};
use torro_gui::theme::{self, BACKGROUND, LINE, MUTED, SURFACE, TEXT};
use torro_pim::{Event as CalendarEvent, EventTime};

use layout::{GRID_HEIGHT, GridHit, HOUR_HEIGHT, StripHit};

use super::ViewData;
use crate::app::Message;

/// Id of the time grid's scrollable, used by [`scroll_to_morning`].
pub const SCROLL_ID: Id = Id::new("torrocalendar.week.grid");

/// Fill opacity of event blocks; the colour shows through a little.
const EVENT_ALPHA: f32 = 0.85;
const HEADER_TEXT_SIZE: f32 = 13.0;
const LABEL_TEXT_SIZE: f32 = 11.0;
const EVENT_TEXT_SIZE: f32 = 12.0;
const UNTITLED: &str = "(untitled)";

/// Task that scrolls the time grid so that 07:00 is at the top. The app should return it
/// when the day or week view opens (switching to it, or navigating to a new anchor), e.g.
/// `views::week::scroll_to_morning()` from `App::update`; `app.rs` is not changed here.
pub fn scroll_to_morning<T>() -> Task<T> {
    iced::widget::operation::scroll_to(SCROLL_ID, AbsoluteOffset { x: 0.0, y: 7.0 * HOUR_HEIGHT })
}

pub fn view(data: ViewData<'_>) -> Element<'_, Message> {
    let plan = Plan::new(&data);
    let header = header(&plan.days, data.now.date());
    let strip_height = layout::strip_height(layout::strip_rows(&plan.bars));
    let strip =
        iced::widget::canvas(Strip { plan: plan.clone() }).width(Length::Fill).height(Length::Fixed(strip_height));
    let grid = iced::widget::canvas(Grid { plan }).width(Length::Fill).height(Length::Fixed(GRID_HEIGHT));
    let grid = scrollable(grid).id(SCROLL_ID).width(Length::Fill).height(Length::Fill);
    column![header, strip, grid].height(Length::Fill).into()
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
}

#[derive(Debug, Clone)]
struct Entry {
    /// Index into the app's event list, for `OpenEvent`.
    index: usize,
    color: Color,
    /// Start time of a timed entry; `None` for all-day entries.
    time: Option<NaiveTime>,
    title: String,
}

impl Plan {
    fn new(data: &ViewData<'_>) -> Plan {
        let count = (data.end - data.start).num_days().max(1);
        let days: Vec<NaiveDate> = (0..count).map(|i| data.start + Duration::days(i)).collect();
        let columns = days.len();
        let mut plan = Plan { days, entries: Vec::new(), timed: Vec::new(), bars: Vec::new(), now: data.now };
        for item in &data.items {
            let (start, end, all_day) = local_span(item.event);
            let id = plan.entries.len();
            plan.entries.push(Entry {
                index: item.index,
                color: item.color,
                time: if all_day { None } else { Some(start.time()) },
                title: title(item.event),
            });
            if all_day {
                plan.bars.extend(layout::day_span(id, data.start, columns, start, end));
            } else {
                plan.timed.extend(layout::split_by_day(id, data.start, columns, start, end));
            }
        }
        plan
    }

    fn today_column(&self) -> Option<usize> {
        self.days.iter().position(|day| *day == self.now.date())
    }
}

/// Start and end in local time, and whether the entry goes into the all-day strip: a
/// date-only entry, or one lasting a day or more.
fn local_span(event: &CalendarEvent) -> (NaiveDateTime, NaiveDateTime, bool) {
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

fn title(event: &CalendarEvent) -> String {
    let summary = event.summary.trim();
    if summary.is_empty() { UNTITLED.to_string() } else { summary.to_string() }
}

/// The left-button press in `bounds`, relative to its top left corner.
fn left_press(event: &Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Point> {
    match event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor.position_in(bounds),
        _ => None,
    }
}

/// Weekday header: a label per day column, today in the primary colour.
fn header(days: &[NaiveDate], today: NaiveDate) -> Element<'static, Message> {
    let mut header = row![Space::new().width(Length::Fixed(layout::GUTTER_WIDTH))];
    for &day in days {
        let color = if day == today { theme::color(TORRO_RED) } else { MUTED };
        let label = format!("{} {}.", weekday_name(day, true), day.day());
        header = header.push(
            container(text(label).size(HEADER_TEXT_SIZE).color(color))
                .width(Length::FillPortion(1))
                .padding([6, 0])
                .align_x(Horizontal::Center),
        );
    }
    header.into()
}

/// The all-day strip above the grid.
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

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let plan = &self.plan;
        let count = plan.days.len();
        let (x0, col_w) = layout::columns(bounds.width, count);
        let height = bounds.height;

        fill(&mut frame, Rectangle { x: 0.0, y: 0.0, width: bounds.width, height }, BACKGROUND);
        if let Some(column) = plan.today_column() {
            let x = x0 + column as f32 * col_w;
            fill(&mut frame, Rectangle { x, y: 0.0, width: col_w, height }, SURFACE);
        }
        label(&mut frame, "all day", Point::new(8.0, height / 2.0), MUTED, Alignment::Left, Vertical::Center);
        for column in 0..=count {
            let x = x0 + column as f32 * col_w;
            line(&mut frame, Point::new(x, 0.0), Point::new(x, height), LINE, 1.0);
        }
        line(&mut frame, Point::new(0.0, height - 0.5), Point::new(bounds.width, height - 0.5), LINE, 1.0);

        let (bars, _) = layout::bar_blocks(&plan.bars, count, bounds.width);
        for bar in &bars {
            if let Some(entry) = plan.entries.get(bar.id) {
                draw_block(&mut frame, bar.rect, entry, true);
            }
        }
        vec![frame.into_geometry()]
    }
}

/// The hour grid with the timed blocks and the "now" line.
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

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let plan = &self.plan;
        let count = plan.days.len();
        let width = bounds.width;
        let (x0, col_w) = layout::columns(width, count);

        fill(&mut frame, Rectangle { x: 0.0, y: 0.0, width, height: GRID_HEIGHT }, BACKGROUND);
        if let Some(column) = plan.today_column() {
            let x = x0 + column as f32 * col_w;
            fill(&mut frame, Rectangle { x, y: 0.0, width: col_w, height: GRID_HEIGHT }, SURFACE);
        }

        for hour in 0..24u32 {
            let y = hour as f32 * HOUR_HEIGHT;
            line(&mut frame, Point::new(x0, y), Point::new(width, y), LINE, 1.0);
            let half = y + HOUR_HEIGHT / 2.0;
            line(&mut frame, Point::new(x0, half), Point::new(width, half), Color { a: 0.5, ..LINE }, 1.0);
            let hour_label = format!("{hour:02}:00");
            label(&mut frame, &hour_label, Point::new(x0 - 6.0, y + 2.0), MUTED, Alignment::Right, Vertical::Top);
        }
        line(&mut frame, Point::new(x0, GRID_HEIGHT), Point::new(width, GRID_HEIGHT), LINE, 1.0);
        for column in 0..=count {
            let x = x0 + column as f32 * col_w;
            line(&mut frame, Point::new(x, 0.0), Point::new(x, GRID_HEIGHT), LINE, 1.0);
        }

        let blocks = layout::timed_blocks(&plan.timed, count, width);
        for block in &blocks {
            if let Some(entry) = plan.entries.get(block.id) {
                draw_block(&mut frame, block.rect, entry, false);
            }
        }

        if let Some(column) = plan.today_column() {
            let y = layout::y_of(plan.now.time());
            let x = x0 + column as f32 * col_w;
            let red = theme::color(TORRO_RED);
            line(&mut frame, Point::new(x, y), Point::new(x + col_w, y), red, 2.0);
            frame.fill(&Path::circle(Point::new(x, y), 4.0), red);
        }
        vec![frame.into_geometry()]
    }
}

/// A timed or all-day block: rounded, filled with the calendar colour, clipped text.
/// `centered` puts the text in the middle (all-day bars), otherwise at the top.
fn draw_block(frame: &mut Frame, rect: Rectangle, entry: &Entry, centered: bool) {
    let shape =
        Path::rounded_rectangle(Point::new(rect.x, rect.y), Size::new(rect.width, rect.height), Radius::from(4.0));
    frame.fill(&shape, Color { a: EVENT_ALPHA, ..entry.color });
    frame.stroke(&shape, Stroke::default().with_color(entry.color).with_width(1.0));

    let content = match entry.time {
        Some(time) => format!("{} {}", format_time(time), entry.title),
        None => entry.title.clone(),
    };
    let (y, align_y) =
        if centered { (rect.y + rect.height / 2.0, Vertical::Center) } else { (rect.y + 3.0, Vertical::Top) };
    frame.with_clip(rect, |frame| {
        frame.fill_text(canvas::Text {
            content,
            position: Point::new(rect.x + 6.0, y),
            max_width: (rect.width - 10.0).max(0.0),
            color: TEXT,
            size: Pixels(EVENT_TEXT_SIZE),
            align_y,
            ..Default::default()
        });
    });
}

fn fill(frame: &mut Frame, rect: Rectangle, color: Color) {
    frame.fill_rectangle(Point::new(rect.x, rect.y), Size::new(rect.width, rect.height), color);
}

fn line(frame: &mut Frame, from: Point, to: Point, color: Color, width: f32) {
    frame.stroke(&Path::line(from, to), Stroke::default().with_color(color).with_width(width));
}

fn label(frame: &mut Frame, content: &str, position: Point, color: Color, align_x: Alignment, align_y: Vertical) {
    frame.fill_text(canvas::Text {
        content: content.to_string(),
        position,
        color,
        size: Pixels(LABEL_TEXT_SIZE),
        align_x,
        align_y,
        ..Default::default()
    });
}
