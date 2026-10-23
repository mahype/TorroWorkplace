//! The small month at the top of the calendar's sidebar (`docs/prototypes/linear/calendar.html`,
//! `.mini`): a month to browse with its own ‹ ›, the days the main view shows on a neutral surface,
//! today as a red circle. A click on a day moves the main view there ([`Message::GoTo`]); the
//! arrows only browse the small month ([`Message::MiniMonth`]), so looking ahead does not move
//! the week the user is working in.

use chrono::{Datelike, Duration, NaiveDate};
use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row};
use iced::{Alignment, Background, Element, Fill, Padding, border};
use torro_core::i18n::tr;
use torro_gui::datetime::{month_grid, month_name, weekday_name};
use torro_gui::icon::Icon;
use torro_gui::theme::{self, palette, radius, size, space};
use torro_gui::{style, typo, widgets};

use crate::app::Message;

/// Height of a week row and of the weekday line in the active design ([`cell_in`]).
fn cell() -> f32 {
    cell_in(space::xl(), typo::line_height(typo::micro()), space::xs())
}

/// Height of a week row and of the weekday line for a design whose `xl` gap, `micro` line and `xs` gap
/// are these: `xl` (`.mini td{height:24px}` in the standard), but at least the line of a day's number
/// with `xs` around it – a dense design's `xl` (Console 12) is smaller than the number (14), which
/// would run over its row (issue #291).
fn cell_in(xl: f32, micro_line: f32, xs: f32) -> f32 {
    xl.max(micro_line + xs)
}

/// Edge of the circle behind a day number (`.mini td span{width:22px}`): [`circle_in`] of the cell.
fn circle() -> f32 {
    circle_in(cell())
}

/// The circle in a cell `cell` high: the cell less a line above and below, so today's circle does not
/// touch the highlighted row's edges.
fn circle_in(cell: f32) -> f32 {
    cell - 2.0
}
/// Today's number: the tabular figures of the axis, one weight up, white on red.
fn today_level() -> typo::Level {
    typo::Level { font: theme::font_tabular_medium(), ..typo::micro() }
}

/// The first of the month `months` months after (negative: before) the month of `date`.
/// Used by the small month's arrows and by the month view's ‹ ›.
pub fn shift(date: NaiveDate, months: i32) -> NaiveDate {
    let index = date.year() * 12 + date.month0() as i32 + months;
    NaiveDate::from_ymd_opt(index.div_euclid(12), index.rem_euclid(12) as u32 + 1, 1).unwrap_or(date)
}

/// The weeks (Monday first) that contain a day of `month`'s month: four to six rows, without the
/// trailing week of the six-week grid that lies wholly in the next month.
pub fn weeks(month: NaiveDate) -> Vec<[NaiveDate; 7]> {
    let grid = month_grid(month);
    let (weeks, _) = grid.as_chunks::<7>();
    weeks
        .iter()
        .filter(|week| week.iter().any(|day| day.month() == month.month() && day.year() == month.year()))
        .copied()
        .collect()
}

/// The small month for the month of `month`. `shown` is the range `[start, end)` the main view
/// shows (only for the day and week view; `None` highlights nothing): those days get the
/// `selected` surface, rounded at the ends of each row. `today` is drawn as a red circle with a
/// white number. Days of the neighbouring months are in `text_3`.
pub fn view<'a>(month: NaiveDate, shown: Option<(NaiveDate, NaiveDate)>, today: NaiveDate) -> Element<'a, Message> {
    let head = row![
        typo::text(typo::body_strong(), format!("{} {}", month_name(month.month()), month.year()))
            .wrapping(Wrapping::None),
        blank::horizontal(),
        widgets::icon_button(Icon::ChevronLeft, tr("Previous month"), Some(Message::MiniMonth(-1))),
        widgets::icon_button(Icon::ChevronRight, tr("Next month"), Some(Message::MiniMonth(1))),
    ]
    .height(size::control_sm())
    .align_y(Alignment::Center);

    let weeks = weeks(month);
    let letters = weeks.first().map(|week| {
        row(week.iter().map(|day| {
            let letter: String = weekday_name(*day, true).chars().take(1).collect();
            container(typo::text(typo::micro(), letter).style(theme::text_3)).center_x(Fill).center_y(cell()).into()
        }))
    });
    let lit = |day: NaiveDate| shown.is_some_and(|(start, end)| start <= day && day < end);
    let rows = weeks.into_iter().map(|week| {
        row(week.iter().map(|&day| {
            let ends =
                (!lit(day - Duration::days(1)) || day == week[0], !lit(day + Duration::days(1)) || day == week[6]);
            day_cell(day, month, today, lit(day), ends)
        }))
        .into()
    });
    let grid = column(letters.map(Element::from)).extend(rows);
    column![head, grid]
        .padding(Padding { top: space::s(), right: space::s(), bottom: space::xs(), left: space::s() })
        .into()
}

/// One day: its number (circled when it is today) as a button that moves the main view there,
/// on the `selected` surface when `lit`; `ends` rounds the surface on the left and right.
fn day_cell<'a>(
    day: NaiveDate,
    month: NaiveDate,
    today: NaiveDate,
    lit: bool,
    (first, last): (bool, bool),
) -> Element<'a, Message> {
    let number = day.day().to_string();
    let face: Element<'a, Message> = if day == today {
        container(
            typo::text(today_level(), number)
                .style(|t| iced::widget::text::Style { color: Some(palette(t).on_accent) }),
        )
        .center(circle())
        .style(|t| container::Style {
            background: Some(Background::Color(palette(t).accent)),
            border: border::rounded(circle() / 2.0),
            ..container::Style::default()
        })
        .into()
    } else {
        let tone = if day.month() == month.month() { theme::text_2 } else { theme::text_3 };
        container(typo::text(typo::micro(), number).style(tone)).center(circle()).into()
    };
    let pick = button(container(face).center_x(Fill).center_y(cell()))
        .width(Fill)
        .height(cell())
        .padding(0)
        .style(style::button::ghost)
        .on_press(Message::GoTo(day));
    let corners = border::Radius::new(0).left(if first { radius::sm() } else { 0.0 }).right(if last {
        radius::sm()
    } else {
        0.0
    });
    container(pick)
        .width(Fill)
        .style(move |t| container::Style {
            background: lit.then(|| Background::Color(palette(t).selected)),
            border: border::rounded(corners),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    #[test]
    fn shift_moves_to_the_first_across_years() {
        assert_eq!(shift(date(2026, 10, 7), 1), date(2026, 11, 1));
        assert_eq!(shift(date(2026, 1, 31), -1), date(2025, 12, 1));
        assert_eq!(shift(date(2026, 12, 15), 1), date(2027, 1, 1));
        assert_eq!(shift(date(2026, 10, 7), 0), date(2026, 10, 1));
    }

    #[test]
    fn october_2026_has_five_weeks_from_monday_28_september() {
        // As in the prototype: 28 Sep … 1 Nov.
        let weeks = weeks(date(2026, 10, 7));
        assert_eq!(weeks.len(), 5);
        assert_eq!(weeks[0][0], date(2026, 9, 28));
        assert_eq!(weeks[4][6], date(2026, 11, 1));
    }

    #[test]
    fn a_month_can_need_six_weeks() {
        // August 2026 starts on a Saturday and ends on a Monday.
        let weeks = weeks(date(2026, 8, 1));
        assert_eq!(weeks.len(), 6);
        assert_eq!(weeks[5][0], date(2026, 8, 31));
    }

    /// A built-in design (issue #291) from its embedded file, read without switching the process's.
    fn built_in(id: &str) -> torro_core::design::Design {
        torro_core::design::load(std::path::Path::new(""), id).expect("a built-in design")
    }

    /// Issue #291 (review of the three designs): a day's cell holds its number – the line of `micro`
    /// and `xs` around it – in every design, so Console's numbers (14 high, its `xl` is 12) no longer
    /// run over their rows; Standard and Compact keep their 24. The circle stays a line inside the cell.
    #[test]
    fn a_day_cell_holds_its_number_in_every_design() {
        let cell = |id: &str| {
            let d = built_in(id);
            cell_in(d.space.xl, d.typo.micro.line, d.space.xs)
        };
        assert_eq!([cell("torro"), cell("compact"), cell("console")], [24.0, 24.0, 16.0]);
        for id in ["torro", "compact", "console"] {
            let d = built_in(id);
            assert!(cell(id) >= d.typo.micro.line + d.space.xs, "{id}: the number fits its cell");
            assert!(circle_in(cell(id)) >= d.typo.micro.line, "{id}: and today's circle");
        }
        assert_eq!((super::cell(), circle()), (24.0, 22.0), "the process draws the standard");
    }

    #[test]
    fn view_builds_with_and_without_a_highlight() {
        let today = date(2026, 10, 7);
        let _ = view(today, Some((date(2026, 10, 5), date(2026, 10, 12))), today);
        let _ = view(shift(today, 1), None, today);
    }
}
