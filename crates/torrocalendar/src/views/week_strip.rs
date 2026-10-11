//! The week strip of the narrow layout (task K.19; mock-up `docs/prototypes/linear/cal-s.png`,
//! `.strip` in `calendar.html`): the seven days of the shown day's week above the day view. Per
//! day the weekday letter (`MICRO`), the day number in a 28 circle – today white on red, the shown
//! day on the neutral selection – and a dot per calendar with events that day. A click on a day
//! shows it (`Message::GoTo`).

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use iced::alignment::Horizontal;
use iced::border;
use iced::widget::text::Style as TextStyle;
use iced::widget::{button, column, container, row, rule};
use iced::{Background, Color, Element, Length, Theme};
use torro_gui::datetime::{week_start, weekday_name};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::{style, typo, widgets};

use super::ViewData;
use super::glance::{self, Entry};
use crate::app::Message;

/// At most this many dots under a day; more calendars on one day add nothing at a glance.
const MAX_DOTS: usize = 3;
/// The day number: 13/500 with tabular figures, as in the week view's header.
const DAY_NUMBER: typo::Level = typo::BODY_STRONG.tabular();
/// Edge of the circle around a day number (`.strip b`, 28).
const CIRCLE: f32 = size::CONTROL_SM;

/// The strip for the week of `data.anchor`, with dots from `data.items` (load the whole week for
/// them) and a line below.
pub fn view(data: &ViewData<'_>) -> Element<'static, Message> {
    let entries = glance::entries(&data.items);
    let today = data.now.date();
    let first = week_start(data.anchor);
    let days = (0..7).map(|offset| {
        let day = first + Duration::days(offset);
        cell(day, today, data.anchor, dots(&entries, day))
    });
    let strip = container(row(days)).padding(space::XS).width(Length::Fill);
    column![strip, rule::horizontal(1).style(style::rule)].into()
}

/// The colours of the calendars with events on `day`, each once, in the order of the events,
/// at most [`MAX_DOTS`].
pub fn dots(entries: &[Entry], day: NaiveDate) -> Vec<Color> {
    let mut colors: Vec<Color> = Vec::new();
    for entry in entries.iter().filter(|e| e.on(day)) {
        if !colors.contains(&entry.color) && colors.len() < MAX_DOTS {
            colors.push(entry.color);
        }
    }
    colors
}

/// The first letter of the short weekday name ("M", German "M" for "Mo").
fn letter(day: NaiveDate) -> String {
    weekday_name(day, true).chars().next().map(String::from).unwrap_or_default()
}

/// One day: letter, number, dots; a ghost button that shows the day.
fn cell(day: NaiveDate, today: NaiveDate, shown: NaiveDate, dots: Vec<Color>) -> Element<'static, Message> {
    let is_today = day == today;
    let weekend = matches!(day.weekday(), Weekday::Sat | Weekday::Sun);
    let letter = typo::text(typo::MICRO, letter(day)).style(if is_today { theme::text } else { theme::text_3 });
    let number = typo::text(DAY_NUMBER, day.day().to_string());
    let number = if is_today {
        number.style(|t: &Theme| TextStyle { color: Some(palette(t).on_accent) })
    } else if day == shown {
        number.style(theme::text)
    } else {
        number.style(if weekend { theme::text_3 } else { theme::text_2 })
    };
    let circle = container(number).center(CIRCLE).style(move |t: &Theme| {
        let p = palette(t);
        let fill = if is_today {
            Some(p.accent)
        } else if day == shown {
            Some(p.selected)
        } else {
            None
        };
        container::Style {
            background: fill.map(Background::Color),
            border: border::rounded(CIRCLE / 2.0),
            ..container::Style::default()
        }
    });
    let dots = row(dots.into_iter().map(|color| widgets::color_dot(color, space::XS))).spacing(space::XS);
    let content =
        column![letter, circle, container(dots).height(space::XS)].spacing(space::XS).align_x(Horizontal::Center);
    button(container(content).center_x(Length::Fill))
        .width(Length::FillPortion(1))
        .padding(space::XS)
        .style(style::button::ghost)
        .on_press(Message::GoTo(day))
        .into()
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDateTime, NaiveTime};

    use super::*;

    fn at(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("date").and_time(NaiveTime::MIN) + Duration::hours(hour.into())
    }

    fn entry(color: Color, start: NaiveDateTime, end: NaiveDateTime) -> Entry {
        Entry { title: "x".into(), location: None, color, start, end, all_day: false }
    }

    #[test]
    fn dots_show_each_calendar_once_and_at_most_three() {
        let [blue, green, orange, pink] =
            [Color::BLACK, Color::WHITE, Color::from_rgb(1.0, 0.5, 0.0), Color::from_rgb(1.0, 0.0, 1.0)];
        let entries = vec![
            entry(blue, at(7, 9), at(7, 10)),
            entry(blue, at(7, 11), at(7, 12)),
            entry(green, at(7, 13), at(7, 14)),
            entry(orange, at(7, 15), at(7, 16)),
            entry(pink, at(7, 17), at(7, 18)),
            entry(pink, at(8, 9), at(8, 10)),
        ];
        let day = |d| at(d, 0).date();
        assert_eq!(dots(&entries, day(7)), vec![blue, green, orange]);
        assert_eq!(dots(&entries, day(8)), vec![pink]);
        assert_eq!(dots(&entries, day(9)), vec![]);
    }

    #[test]
    fn the_letter_is_the_first_of_the_short_weekday() {
        // 5 October 2026 is a Monday.
        assert_eq!(letter(at(5, 0).date()), weekday_name(at(5, 0).date(), true).chars().take(1).collect::<String>());
    }
}
