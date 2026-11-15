//! The event card in the assistant's chat (issue #797): the calendar, title, time, place and what overlaps, with
//! "Open in calendar" (`apps.open_app` with `event:<uid>`: the calendar's week with the event chosen).
//!
//! Beside the title the calendar sheet of its start (`content_card::Sheet`): the calendar's colour as its top edge and
//! the tint of the month band, nowhere as a surface; the head names the calendar.

use chrono::{DateTime, NaiveDate, Utc};
use iced::Element;
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::content_card::{Action, ContentCard, Head, Sheet};
use torro_gui::icon::Icon;
use torro_gui::{datetime, widgets};
use torro_ipc::Request;
use torro_ipc::assistant::Overlap;
use torro_pim::{Event, EventTime};

use super::{card_request, with_actions};
use crate::app::Message;

/// What "Open in calendar" sends: `apps.open_app` of the calendar with `event:<uid>`.
pub(super) fn open(event: &Event) -> Message {
    let request =
        Request::OpenApp { app: AppId::CALENDAR.binary.to_owned(), target: Some(format!("event:{}", event.uid)) };
    card_request(request, tr("Couldn't open the app."))
}

/// The buttons of the card of `event`, the main one first.
pub(super) fn actions(event: &Event) -> Vec<Action<Message>> {
    vec![Action::new(Icon::ArrowUpRight, tr("Open in calendar"), open(event))]
}

/// When `start` (to `end`) is, in local time, `today` for the year: "Mon, 15 Oct, 14:00–15:00", "Mon, 15 Oct · All
/// day", across days "Mon, 15 Oct 22:00 – Tue, 16 Oct 01:00".
pub(super) fn when(start: &EventTime, end: Option<&EventTime>, today: NaiveDate) -> String {
    let day = |date: NaiveDate| datetime::format_short_date(date, today);
    match (start, end) {
        (EventTime::Date(first), Some(EventTime::Date(after)))
            if after.pred_opt().is_some_and(|last| last > *first) =>
        {
            // A day's end is exclusive in iCalendar: the last day is the one before it.
            let last = after.pred_opt().unwrap_or(*after);
            format!("{} – {}", day(*first), day(last))
        }
        (EventTime::Date(first), _) => format!("{} · {}", day(*first), tr("All day")),
        (EventTime::DateTime(at), end) => {
            let from = datetime::to_local(*at);
            let to = match end {
                Some(EventTime::DateTime(until)) => Some(datetime::to_local(*until)),
                _ => None,
            };
            let start_time = datetime::format_time(from.time());
            match to {
                Some(to) if to.date() == from.date() => {
                    format!("{}, {}–{}", day(from.date()), start_time, datetime::format_time(to.time()))
                }
                Some(to) => format!(
                    "{} {} – {} {}",
                    day(from.date()),
                    start_time,
                    day(to.date()),
                    datetime::format_time(to.time())
                ),
                None => format!("{}, {}", day(from.date()), start_time),
            }
        }
    }
}

/// The local day an event starts on.
fn first_day(start: &EventTime) -> NaiveDate {
    match start {
        EventTime::Date(day) => *day,
        EventTime::DateTime(at) => datetime::to_local(*at).date(),
    }
}

/// The line about what overlaps: the first other event with its time, and how many more ("and 2 more"); `None`
/// when nothing does.
fn overlapping(overlaps: &[Overlap], more: u32, today: NaiveDate) -> Option<String> {
    let first = overlaps.first()?;
    let time = when(&first.start, first.end.as_ref(), today);
    let others = (overlaps.len() - 1) as u64 + u64::from(more);
    let title = first.summary.trim();
    Some(match others {
        0 => trf("Overlaps with {title} ({time})", &[("title", &title), ("time", &time)]),
        n => {
            trf("Overlaps with {title} ({time}) and {count} more", &[("title", &title), ("time", &time), ("count", &n)])
        }
    })
}

/// The card of `event` in the calendar `calendar` (its colour `color`, `#RRGGBB`, on the sheet), with `overlaps` and
/// `more` more overlapping, `now` for the dates: the sheet of its first day, then time, place and what overlaps, the
/// most important first.
pub(super) fn content(
    event: &Event,
    calendar: &str,
    color: Option<&str>,
    overlaps: &[Overlap],
    more: u32,
    now: DateTime<Utc>,
) -> ContentCard<Message> {
    let today = datetime::to_local(now).date();
    let title = match event.summary.trim() {
        "" => tr("(untitled)").to_owned(),
        title => title.to_owned(),
    };
    let color = color.and_then(widgets::color_from_hex);
    let mut card = ContentCard::new(Head::new(Icon::Calendar, calendar), title)
        .sheet(Sheet::of(first_day(&event.start), color, today))
        .line(Icon::Clock, when(&event.start, event.end.as_ref(), today))
        .line(Icon::MapPin, event.location.clone().unwrap_or_default());
    if let Some(overlap) = overlapping(overlaps, more, today) {
        // The words say it; the icon in `attention` only marks it (torro-design: status never by colour alone).
        card = card.line_toned(Icon::TriangleAlert, overlap, Some(|p| p.attention));
    }
    with_actions(card, actions(event))
}

/// The card, at most `room` wide.
pub(super) fn view<'a>(
    event: &Event,
    calendar: &str,
    color: Option<&str>,
    overlaps: &[Overlap],
    more: u32,
    now: DateTime<Utc>,
    room: f32,
) -> Element<'a, Message> {
    content(event, calendar, color, overlaps, more, now).view(room)
}

/// The card in one line: the title, its time and "Open in calendar".
pub(super) fn line<'a>(event: &Event, calendar: &str, now: DateTime<Utc>) -> Element<'a, Message> {
    content(event, calendar, None, &[], 0, now).short()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_days_end_the_day_before_their_end() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).expect("a date");
        let day = |d: u32| EventTime::Date(NaiveDate::from_ymd_opt(2026, 11, d).expect("a date"));
        let one = when(&day(16), Some(&day(17)), today);
        assert!(one.ends_with(tr("All day")), "{one}");
        let three = when(&day(16), Some(&day(19)), today);
        assert_eq!(
            three,
            format!(
                "{} – {}",
                datetime::format_short_date(NaiveDate::from_ymd_opt(2026, 11, 16).expect("a date"), today),
                datetime::format_short_date(NaiveDate::from_ymd_opt(2026, 11, 18).expect("a date"), today)
            )
        );
    }
}
