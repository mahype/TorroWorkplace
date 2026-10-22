//! The calendar views. Each one only renders what [`ViewData`] holds and reports clicks
//! as [`crate::app::Message`]s: `NewEvent` on an empty slot or day, `OpenEvent(index)` on
//! an event, `ShowDay(date)` on a day number in the month view.
//!
//! [`glance`] holds the two layouts of a small tile (mini, strip), [`week_strip`] the row of the
//! week's days above the day view in the narrow layout.
//!
//! How the state of an event shows is decided once, here, for every view ([`Look`], task TE.9):
//! cancelled ones are struck through and faded like ended ones and leave their time free
//! ([`takes_time`]); tentative ones are a dashed outline without a fill (rule "Termine" in the
//! design skill). The drawing helpers for that ([`strike`], [`dashed_outline`], [`dashes`],
//! [`dashed`], [`dot`], [`title_text`]) are shared too, so the views cannot drift apart.

pub mod agenda;
pub mod glance;
pub mod mini_month;
pub mod month;
pub mod radar;
pub mod timeline;
pub mod week;
pub mod week_strip;

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use iced::border::{self, Radius};
use iced::widget::canvas::{self, Frame, LineDash, Path, Stroke};
use iced::widget::text::{self, Span, Wrapping};
use iced::widget::{Space, container, rich_text, span};
use iced::{Background, Color, Element, Length, Point, Rectangle, Size, Theme, mouse};
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::weekday_name;
use torro_gui::theme::{mix, palette, radius, size};
use torro_gui::typo;
use torro_pim::Event;
use torro_pim::model::EventStatus;

/// Short month names for [`short_date`], January first (translated with `tr`).
const MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `Sun, 11 Oct` (German `So, 11. Okt.`): the date where a long one does not fit – the narrow
/// header, the mini tile, the strip.
pub fn short_date(date: NaiveDate) -> String {
    let month = MONTHS_SHORT.get(date.month0() as usize).map(|name| tr(name)).unwrap_or_default();
    trf("{weekday}, {day} {month}", &[("weekday", &weekday_name(date, true)), ("day", &date.day()), ("month", &month)])
}

/// One visible event.
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    /// Index into the app's event list; send it back with `Message::OpenEvent`.
    pub index: usize,
    pub event: &'a Event,
    /// The colour of its calendar.
    pub color: Color,
}

/// What a view gets to draw.
#[derive(Debug, Clone)]
pub struct ViewData<'a> {
    /// The events of the shown calendars overlapping `[start, end)`, unsorted.
    pub items: Vec<Item<'a>>,
    /// First day shown (local).
    pub start: NaiveDate,
    /// Day after the last one shown (local).
    pub end: NaiveDate,
    /// The date the user navigated to (the day view's day; inside the month for month).
    pub anchor: NaiveDate,
    /// Local now, for the "now" line and highlighting today.
    pub now: NaiveDateTime,
    /// Index into the app's event list of the event open in the side panel (U.4), `None` when
    /// none is. The grids outline it with the focus ring (`.ev.sel` in the mock-up), the agenda
    /// selects its row; views without room for that (the glance) ignore it.
    pub selected: Option<usize>,
}

/// What is faded – events that have ended, and cancelled ones – is drawn at this opacity over its
/// ground, like CSS `opacity` (`.ev.past` in the mock-ups).
pub const FADED: f32 = 0.55;
/// Dash and gap of a tentative event's outline (`stroke-dasharray="3 3"` in the radar mock-up).
pub const DASH: [f32; 2] = [3.0, 3.0];

/// How an event shows by its state, the same in every view (task TE.9; rule "Termine" in the design
/// skill). Built with [`Look::of`]; a view that dims for reasons of its own (the neighbouring months
/// of the month view) may set `faded` on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    /// Drawn at [`FADED`] over its ground: the event has ended, or it is cancelled.
    pub faded: bool,
    /// The title struck through: the event is cancelled (`STATUS:CANCELLED`).
    pub struck: bool,
    /// Not settled yet (`STATUS:TENTATIVE`): a dashed outline in the calendar's colour instead of
    /// the tinted surface and its bar; a ring instead of a dot.
    pub dashed: bool,
}

impl Look {
    /// The look of an event with `status` that has ended by now when `past`. A cancelled event is
    /// struck through and faded whether it is over or not; a tentative one is dashed.
    pub fn of(status: Option<EventStatus>, past: bool) -> Look {
        let cancelled = status == Some(EventStatus::Cancelled);
        Look { faded: past || cancelled, struck: cancelled, dashed: status == Some(EventStatus::Tentative) }
    }

    /// The opacity to draw with: [`FADED`] when faded, else 1.
    pub fn alpha(self) -> f32 {
        if self.faded { FADED } else { 1.0 }
    }

    /// `color` as drawn over `ground`: mixed towards it when faded (a canvas cannot fade a group of
    /// shapes, so the views compose by hand).
    pub fn over(self, ground: Color, color: Color) -> Color {
        if self.faded { mix(ground, color, FADED) } else { color }
    }
}

/// Whether an event with `status` fills its time: everything but a cancelled one. Only what takes
/// time splits "2 h free", runs now ("40 min left") or keeps the glance from saying "Free until
/// 14:00"; a cancelled event still shows, struck through, where it would have been.
pub fn takes_time(status: Option<EventStatus>) -> bool {
    status != Some(EventStatus::Cancelled)
}

/// A strike through text on a canvas: a 1-px line in `color` from `from` (the left end, at the
/// middle of the text's line) `width` to the right. In a turned frame it follows the text.
pub fn strike(frame: &mut Frame, from: Point, width: f32, color: Color) {
    if width > 0.0 {
        let to = Point::new(from.x + width, from.y);
        frame.stroke(&Path::line(from, to), Stroke::default().with_color(color).with_width(size::HAIRLINE));
    }
}

/// The outline of a tentative event or chip on a canvas: a 1-px dashed line ([`DASH`]) in `color`
/// just inside `rect`, rounded `radius::xs()` like the surface it stands for. No fill.
pub fn dashed_outline(frame: &mut Frame, rect: Rectangle, color: Color) {
    let half = size::HAIRLINE / 2.0;
    let inner = Size::new((rect.width - size::HAIRLINE).max(0.0), (rect.height - size::HAIRLINE).max(0.0));
    let path = Path::rounded_rectangle(Point::new(rect.x + half, rect.y + half), inner, Radius::from(radius::xs()));
    let stroke = Stroke {
        line_dash: LineDash { segments: &DASH, offset: 0 },
        ..Stroke::default().with_color(color).with_width(size::HAIRLINE)
    };
    frame.stroke(&path, stroke);
}

/// The dashes of a dashed line `length` long as `(from, to)` along it, after [`DASH`]: for shapes
/// a canvas cannot dash itself in one go (the radar's arcs fade piece by piece). The last dash is
/// cut at the end; nothing for no length, or one that is not finite (a degenerate shape).
pub fn dashes(length: f32) -> Vec<(f32, f32)> {
    let step = DASH[0] + DASH[1];
    let count = if length > 0.0 && length.is_finite() { (length / step).ceil() as usize } else { 0 };
    (0..count).map(|n| n as f32 * step).map(|from| (from, (from + DASH[0]).min(length))).collect()
}

/// What [`dashed`] draws in its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashedShape {
    /// The colour bar of a tentative row, dashed down its height (`.ev.tent .bar`).
    Bar,
    /// The outline of a tentative chip ([`dashed_outline`]).
    Outline,
}

/// A dashed bar or outline in `color` (faded towards the panel when `faded`) filling its bounds,
/// for widget views; a canvas view draws [`dashed_outline`] itself. Give it the size of the bar or
/// lay it under the chip's content with `Stack::push_under`, which keeps the content's size (as the
/// base layer it would stretch the chip to all the room it is given).
pub fn dashed<'a, M: 'a>(color: Color, faded: bool, shape: DashedShape) -> Element<'a, M> {
    canvas::Canvas::new(Dashed { color, faded, shape }).width(Length::Fill).height(Length::Fill).into()
}

struct Dashed {
    color: Color,
    faded: bool,
    shape: DashedShape,
}

impl<M> canvas::Program<M> for Dashed {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let color = Look { faded: self.faded, ..Look::default() }.over(palette(theme).panel, self.color);
        match self.shape {
            DashedShape::Bar => {
                let x = bounds.width / 2.0;
                for (from, to) in dashes(bounds.height) {
                    let line = Path::line(Point::new(x, from), Point::new(x, to));
                    frame.stroke(&line, Stroke::default().with_color(color).with_width(bounds.width));
                }
            }
            DashedShape::Outline => dashed_outline(&mut frame, Rectangle::new(Point::ORIGIN, bounds.size()), color),
        }
        vec![frame.into_geometry()]
    }
}

/// The colour dot in front of an event `edge` across: filled in `color`, a ring of the same colour
/// for a tentative event (`look.dashed`), faded towards the panel when `look.faded`.
pub fn dot<'a, M: 'a>(color: Color, edge: f32, look: Look) -> Element<'a, M> {
    container(Space::new().width(edge).height(edge))
        .style(move |theme: &Theme| {
            let color = look.over(palette(theme).panel, color);
            container::Style {
                background: (!look.dashed).then_some(Background::Color(color)),
                border: border::rounded(edge / 2.0).color(color).width(if look.dashed { size::HAIRLINE } else { 0.0 }),
                ..container::Style::default()
            }
        })
        .into()
}

/// An event's title in `level` on one line, struck through when `struck` (cancelled), in the colour
/// `color` gives for the theme. The caller cuts it to its room first ([`typo::ellipsize`]).
pub fn title_text<'a, M: 'a>(
    level: typo::Level,
    content: String,
    struck: bool,
    color: impl Fn(&Theme) -> Color + 'a,
) -> Element<'a, M> {
    let fragment: Span<'static, ()> = span(content).strikethrough(struck);
    rich_text(vec![fragment])
        .size(level.size)
        .line_height(level.line_height)
        .font(level.font)
        .wrapping(Wrapping::None)
        .style(move |theme: &Theme| text::Style { color: Some(color(theme)) })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_date_names_weekday_day_and_month() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 11).expect("date");
        let expected = trf("{weekday}, {day} {month}", &[("weekday", &tr("Sun")), ("day", &11), ("month", &tr("Oct"))]);
        assert_eq!(short_date(date), expected);
        assert_eq!(MONTHS_SHORT[9], "Oct");
    }

    #[test]
    fn confirmed_and_unknown_events_look_plain_until_they_end() {
        for status in [None, Some(EventStatus::Confirmed)] {
            assert_eq!(Look::of(status, false), Look::default());
            assert_eq!(Look::of(status, true), Look { faded: true, ..Look::default() });
        }
    }

    #[test]
    fn cancelled_events_are_struck_through_and_faded_even_before_they_end() {
        let cancelled = Look { faded: true, struck: true, dashed: false };
        assert_eq!(Look::of(Some(EventStatus::Cancelled), false), cancelled);
        assert_eq!(Look::of(Some(EventStatus::Cancelled), true), cancelled);
    }

    #[test]
    fn tentative_events_are_dashed_and_fade_only_when_over() {
        assert_eq!(Look::of(Some(EventStatus::Tentative), false), Look { dashed: true, ..Look::default() });
        assert_eq!(Look::of(Some(EventStatus::Tentative), true), Look { faded: true, struck: false, dashed: true });
    }

    #[test]
    fn faded_looks_mix_towards_their_ground() {
        let (ground, color) = (Color::BLACK, Color::WHITE);
        let faded = Look::of(Some(EventStatus::Cancelled), false);
        assert!((faded.alpha() - FADED).abs() < f32::EPSILON);
        assert_eq!(faded.over(ground, color), mix(ground, color, FADED));
        assert!((Look::default().alpha() - 1.0).abs() < f32::EPSILON);
        assert_eq!(Look::default().over(ground, color), color);
    }

    #[test]
    fn only_cancelled_events_leave_their_time_free() {
        assert!(takes_time(None));
        assert!(takes_time(Some(EventStatus::Confirmed)));
        assert!(takes_time(Some(EventStatus::Tentative)));
        assert!(!takes_time(Some(EventStatus::Cancelled)));
    }

    #[test]
    fn dashes_follow_the_pattern_and_stop_at_the_end() {
        assert!(dashes(0.0).is_empty());
        assert!(dashes(-4.0).is_empty());
        assert!(dashes(f32::INFINITY).is_empty() && dashes(f32::NAN).is_empty());
        assert_eq!(dashes(2.0), vec![(0.0, 2.0)]);
        assert_eq!(dashes(10.0), vec![(0.0, 3.0), (6.0, 9.0)]);
        assert_eq!(dashes(13.0), vec![(0.0, 3.0), (6.0, 9.0), (12.0, 13.0)]);
        assert_eq!(dashes(12.0), vec![(0.0, 3.0), (6.0, 9.0)]);
    }
}
