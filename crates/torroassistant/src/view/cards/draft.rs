//! The draft card in the assistant's chat (issue #797): from, to, subject and the start of the text, with "Send" and
//! "Discard". Nothing is sent before the person's click, and the window never sends past an approval:
//!
//! - **With an open approval** of sending exactly this state of the draft (`approval`), both decide it
//!   (`approvals.decide_approval`): "Send" allows, "Discard" denies. Sending then runs in the core as approved.
//! - **Without one**, "Send" is `mail.send_draft` with the app key (the person's own click) and "Discard" is
//!   switched off: deleting a draft is the mail window's, and the card asks the core for nothing else.

use iced::Element;
use torro_core::i18n::{tr, trf};
use torro_gui::content_card::{Action, ContentCard, Head};
use torro_gui::icon::Icon;
use torro_ipc::Request;
use torro_mail::Draft;

use super::{card_request, with_actions};
use crate::app::Message;

/// What "Send" sends: with `approval` its allowing, else `mail.send_draft` of the draft; `None` for a draft that
/// was never saved (no id to send).
pub(super) fn send(draft: &Draft, approval: Option<&str>) -> Option<Message> {
    match approval {
        Some(id) => Some(decide(id, true)),
        None => {
            let id = draft.id.clone()?;
            Some(card_request(Request::SendDraft { id }, tr("Couldn't send the draft.")))
        }
    }
}

/// What "Discard" sends: with `approval` its denial; `None` without one, so the button does nothing.
pub(super) fn discard(approval: Option<&str>) -> Option<Message> {
    approval.map(|id| decide(id, false))
}

/// `approvals.decide_approval` of `id`, as the person's click with the app key.
fn decide(id: &str, allow: bool) -> Message {
    card_request(Request::DecideApproval { id: id.to_owned(), allow, scope: None }, tr("Couldn't decide the approval."))
}

/// The buttons of the card, the main one first: "Send", then "Discard" (switched off without an approval).
pub(super) fn actions(draft: &Draft, approval: Option<&str>) -> Vec<Action<Message>> {
    vec![
        Action { icon: Icon::Send, label: tr("Send").to_owned(), on_press: send(draft, approval), quiet: false },
        Action { icon: Icon::Trash2, label: tr("Discard").to_owned(), on_press: discard(approval), quiet: false },
    ]
}

/// Whom the draft goes to: the names (else the addresses) of `To`, one line.
fn recipients(draft: &Draft) -> String {
    let names: Vec<&str> = draft
        .to
        .iter()
        .map(|to| to.name.as_deref().map(str::trim).filter(|name| !name.is_empty()).unwrap_or(&to.address))
        .collect();
    names.join(", ")
}

/// The card of `draft`, `approval` the id of the open approval of sending it: the account it goes from, the subject,
/// to whom, the start of the text, and that it waits for the person.
pub(super) fn content(draft: &Draft, approval: Option<&str>) -> ContentCard<Message> {
    let subject = match draft.subject.trim() {
        "" => tr("(no subject)").to_owned(),
        subject => subject.to_owned(),
    };
    let from = draft.account.trim_start_matches("mail:");
    let to = recipients(draft);
    let mut card = ContentCard::new(Head::new(Icon::FilePenLine, from), subject);
    if !to.is_empty() {
        card = card.line(Icon::Send, trf("To {names}", &[("names", &to)]));
    }
    let start = draft.body.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    card = card.line(Icon::AlignLeft, start);
    if approval.is_some() {
        card = card.line(Icon::Shield, tr("Waiting for your approval"));
    }
    with_actions(card, actions(draft, approval))
}

/// The card, at most `room` wide.
pub(super) fn view<'a>(draft: &Draft, approval: Option<&str>, room: f32) -> Element<'a, Message> {
    content(draft, approval).view(room)
}

/// The card in one line.
pub(super) fn line<'a>(draft: &Draft, approval: Option<&str>) -> Element<'a, Message> {
    content(draft, approval).short()
}
