//! The draft card in the assistant's chat (issue #797): from, to, subject and the start of the text, with "Send" and
//! "Discard". With an open approval both decide it (`approvals.decide_approval`, Allow or Deny); without one "Send" is
//! `mail.send_draft` with the app key and "Discard" does nothing. Nothing is sent before the person's click.
//!
//! Strand 2g of issue #784 wires the buttons; this draws the head and the subject.

use iced::Element;
use torro_gui::content_card::{ContentCard, Head};
use torro_gui::icon::Icon;
use torro_mail::Draft;

use crate::app::Message;

/// The card of `draft`, `approval` the id of the open approval of sending it.
pub(super) fn content(draft: &Draft, approval: Option<&str>) -> ContentCard<Message> {
    let _ = approval;
    let to = draft.to.first().map(|to| to.address.clone()).unwrap_or_default();
    ContentCard::new(Head::new(Icon::FilePenLine, draft.account.clone()), draft.subject.clone()).line(Icon::Send, to)
}

/// The card, at most `room` wide.
pub(super) fn view<'a>(draft: &Draft, approval: Option<&str>, room: f32) -> Element<'a, Message> {
    content(draft, approval).view(room)
}

/// The card in one line.
pub(super) fn line<'a>(draft: &Draft) -> Element<'a, Message> {
    content(draft, None).short()
}
