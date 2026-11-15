//! The event card in the assistant's chat (issue #797): the calendar sheet, time, place, what overlaps, with "Open
//! in calendar" (`apps.open_app` with `event:<uid>`) and "Edit".
//!
//! Strand 2g of issue #784 gives it its final parts (the sheet, the overlaps, the buttons); this draws the head and
//! the title.

use iced::Element;
use torro_gui::content_card::{ContentCard, Head};
use torro_gui::icon::Icon;
use torro_ipc::assistant::Overlap;
use torro_pim::Event;

use crate::app::Message;

/// The card of `event` in the calendar `calendar` (its colour `color`), with `overlaps` and `more` more overlapping.
pub(super) fn content(
    event: &Event,
    calendar: &str,
    color: Option<&str>,
    overlaps: &[Overlap],
    more: u32,
) -> ContentCard<Message> {
    let _ = (color, overlaps, more);
    ContentCard::new(Head::new(Icon::Calendar, calendar), event.summary.clone())
        .line(Icon::MapPin, event.location.clone().unwrap_or_default())
}

/// The card, at most `room` wide.
pub(super) fn view<'a>(
    event: &Event,
    calendar: &str,
    color: Option<&str>,
    overlaps: &[Overlap],
    more: u32,
    room: f32,
) -> Element<'a, Message> {
    content(event, calendar, color, overlaps, more).view(room)
}

/// The card in one line.
pub(super) fn line<'a>(event: &Event, calendar: &str) -> Element<'a, Message> {
    content(event, calendar, None, &[], 0).short()
}
