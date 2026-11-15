//! The mail card in the assistant's chat (issue #786): sender, subject, date and the start of the text, marked
//! `untrusted`, with "Open in Mail" (`apps.open_app` with `message:<account>:<id>`, [`Message::OpenMail`]). Never a
//! picture from the net: the card shows only what the mail cache holds.

use chrono::{DateTime, Utc};
use iced::Element;
use torro_core::i18n::{tr, trf};
use torro_gui::content_card::{Action, ContentCard, Head};
use torro_gui::datetime;
use torro_gui::icon::Icon;
use torro_mail::MessageSummary;

use super::with_actions;
use crate::app::Message;

/// The buttons of the card of `message`, the main one first: "Open in Mail", the mail window on that mail.
pub(super) fn actions(message: &MessageSummary) -> Vec<Action<Message>> {
    let open = Message::OpenMail { account: message.account.clone(), id: message.id.clone() };
    vec![Action::new(Icon::ArrowUpRight, tr("Open in Mail"), open)]
}

/// Who sent `message`, as its head names it: the sender's name, else the address, else the account it came to.
fn sender(message: &MessageSummary) -> String {
    let named = message.from.as_ref().and_then(|from| {
        let name = from.name.as_deref().map(str::trim).filter(|name| !name.is_empty());
        name.map(str::to_owned).or_else(|| Some(from.address.clone()).filter(|address| !address.is_empty()))
    });
    named.unwrap_or_else(|| message.account.trim_start_matches("mail:").to_owned())
}

/// The names of the attachments of `message`, in their order.
///
/// `torro_mail::MessageSummary` carries only `has_attachments`, so this is empty until `Card::Mail` brings the names
/// (Coder K / the coordination: read them here). The line then falls back to "Attachments".
fn attachment_names(message: &MessageSummary) -> Vec<String> {
    let _ = message;
    Vec::new()
}

/// The line of the attachments: the first name with how many more ("offer.pdf and 2 more"), the one name alone,
/// "Attachments" when the names are not known; `None` without attachments.
fn attachment_words(names: &[String], has_attachments: bool) -> Option<String> {
    match names {
        [] => has_attachments.then(|| tr("Attachments").to_owned()),
        [one] => Some(one.clone()),
        [first, rest @ ..] => Some(trf("{name} and {count} more", &[("name", first), ("count", &rest.len())])),
    }
}

/// The card of `message` as the core resolved it, `now` for its date: the sender with `untrusted`, the subject,
/// when it came, the start of its text and whether it has attachments.
pub(super) fn content(message: &MessageSummary, now: DateTime<Utc>) -> ContentCard<Message> {
    let subject = match message.subject.trim() {
        "" => tr("(no subject)").to_owned(),
        subject => subject.to_owned(),
    };
    let mut card = ContentCard::new(Head::new(Icon::Mail, sender(message)).untrusted(), subject)
        .line(Icon::Clock, datetime::format_moment(message.date, now))
        .line(Icon::AlignLeft, message.snippet.clone());
    if let Some(words) = attachment_words(&attachment_names(message), message.has_attachments) {
        card = card.line(Icon::Paperclip, words);
    }
    with_actions(card, actions(message))
}

/// The card of `message`, at most `room` wide.
pub(super) fn view<'a>(message: &MessageSummary, now: DateTime<Utc>, room: f32) -> Element<'a, Message> {
    content(message, now).view(room)
}

/// The card of `message` in one line.
pub(super) fn line<'a>(message: &MessageSummary, now: DateTime<Utc>) -> Element<'a, Message> {
    content(message, now).short()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_attachment_line_names_the_first_and_counts_the_rest() {
        let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        assert_eq!(attachment_words(&[], false), None);
        assert_eq!(attachment_words(&[], true).as_deref(), Some(tr("Attachments")));
        assert_eq!(attachment_words(&names(&["offer.pdf"]), true).as_deref(), Some("offer.pdf"));
        let three = trf("{name} and {count} more", &[("name", &"offer.pdf"), ("count", &2)]);
        assert_eq!(attachment_words(&names(&["offer.pdf", "a.png", "b.png"]), true), Some(three));
    }
}
