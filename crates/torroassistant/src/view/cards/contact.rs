//! The contact card in the assistant's chat (issue #789): name, company, role, one address or number and the place,
//! with "Open in contacts" (`apps.open_app` with `contact:<id>`). The photo (`contacts.get_photo`) is not fetched
//! yet: the head's icon stands for the person.

use iced::Element;
use torro_contacts::ContactSummary;
use torro_core::AppId;
use torro_core::i18n::tr;
use torro_gui::content_card::{Action, ContentCard, Head};
use torro_gui::icon::Icon;
use torro_ipc::Request;

use super::{card_request, with_actions};
use crate::app::Message;

/// What "Open in contacts" sends: `apps.open_app` of the contacts app with `contact:<id>`.
pub(super) fn open(contact: &ContactSummary) -> Message {
    let target = Some(format!("contact:{}", contact.id));
    card_request(Request::OpenApp { app: AppId::CONTACTS.binary.to_owned(), target }, tr("Couldn't open the app."))
}

/// The buttons of the card of `contact`, the main one first.
pub(super) fn actions(contact: &ContactSummary) -> Vec<Action<Message>> {
    vec![Action::new(Icon::ArrowUpRight, tr("Open in contacts"), open(contact))]
}

/// The card of `contact`: the company in the head (else "Contacts"), the role, the mail address or else the number,
/// and the place.
pub(super) fn content(contact: &ContactSummary) -> ContentCard<Message> {
    let filled =
        |value: &Option<String>| value.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned);
    let source = filled(&contact.org).unwrap_or_else(|| tr("Contacts").to_owned());
    let reach = filled(&contact.email)
        .map(|mail| (Icon::Mail, mail))
        .or_else(|| filled(&contact.phone).map(|phone| (Icon::Phone, phone)));
    let mut card = ContentCard::new(Head::new(Icon::User, source), contact.display_name.clone())
        .line(Icon::Building2, filled(&contact.title).unwrap_or_default());
    if let Some((mark, value)) = reach {
        card = card.line(mark, value);
    }
    card = card.line(Icon::MapPin, filled(&contact.locality).unwrap_or_default());
    with_actions(card, actions(contact))
}

/// The card, at most `room` wide.
pub(super) fn view<'a>(contact: &ContactSummary, room: f32) -> Element<'a, Message> {
    content(contact).view(room)
}

/// The card in one line.
pub(super) fn line<'a>(contact: &ContactSummary) -> Element<'a, Message> {
    content(contact).short()
}
