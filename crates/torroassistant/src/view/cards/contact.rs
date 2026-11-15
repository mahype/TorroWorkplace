//! The contact card in the assistant's chat (issue #789): photo, name, company, one address or number, with "Open in
//! contacts" (`apps.open_app` with `contact:<id>`).
//!
//! Strand 2g of issue #784 gives it its final parts (the photo through `contacts.get_photo`); this draws the head and
//! the name.

use iced::Element;
use torro_contacts::ContactSummary;
use torro_gui::content_card::{ContentCard, Head};
use torro_gui::icon::Icon;

use crate::app::Message;

/// The card of `contact`.
pub(super) fn content(contact: &ContactSummary) -> ContentCard<Message> {
    let source = contact.org.clone().unwrap_or_default();
    ContentCard::new(Head::new(Icon::User, source), contact.display_name.clone())
        .line(Icon::Mail, contact.email.clone().unwrap_or_default())
}

/// The card, at most `room` wide.
pub(super) fn view<'a>(contact: &ContactSummary, room: f32) -> Element<'a, Message> {
    content(contact).view(room)
}

/// The card in one line.
pub(super) fn line<'a>(contact: &ContactSummary) -> Element<'a, Message> {
    content(contact).short()
}
