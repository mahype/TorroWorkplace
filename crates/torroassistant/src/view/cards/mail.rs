//! The mail card in the assistant's chat (issue #786): sender, subject, date and the start of the text, marked
//! `untrusted`, with "Open in Mail" (`apps.open_app` with `message:<account>:<id>`).
//!
//! Strand 2g of issue #784 gives it its final parts; this draws the head, the subject and "Open in Mail".

use iced::Element;
use torro_core::i18n::tr;
use torro_gui::content_card::{Action, ContentCard, Head};
use torro_gui::icon::Icon;
use torro_mail::MessageSummary;

use crate::app::Message;

/// The card of `message` as the core resolved it.
pub(super) fn content(message: &MessageSummary) -> ContentCard<Message> {
    let sender = message.from.as_ref().map(|from| from.address.clone()).unwrap_or_default();
    let open = Message::OpenMail { account: message.account.clone(), id: message.id.clone() };
    ContentCard::new(Head::new(Icon::Mail, sender).untrusted(), message.subject.clone())
        .line(Icon::AlignLeft, message.snippet.clone())
        .main(Action::new(Icon::ArrowUpRight, tr("Open in Mail"), open))
}

/// The card of `message`, at most `room` wide.
pub(super) fn view<'a>(message: &MessageSummary, room: f32) -> Element<'a, Message> {
    content(message).view(room)
}

/// The card of `message` in one line.
pub(super) fn line<'a>(message: &MessageSummary) -> Element<'a, Message> {
    content(message).short()
}
