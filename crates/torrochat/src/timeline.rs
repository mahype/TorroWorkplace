//! What the history of a chat window shows, worked out apart from the widgets so it can be tested:
//! the messages with day dividers, the divider "New" after the last read message and the runs of one
//! author ([`items`]); mentions while typing ([`mention_query`], [`with_mention`]) and in a message
//! ([`pieces`]), and the text that goes out ([`outgoing`]).
//!
//! Mentions: the composer shows `@Ben Koch`, as people read it; the provider wants `@"ben"`
//! (`docs/chat.md`, "Befehle": mentions are written as `@"<mentionId>"`). [`outgoing`] turns the one
//! into the other for the mentions the person picked from the suggestions, nothing else.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use torro_chat::{Mention, Message, MessageKind};
use torro_core::i18n::tr;
use torro_gui::datetime;

/// Messages of one author closer than this stand together, with one name above and one time below.
pub const RUN_GAP: Duration = Duration::minutes(5);

/// The longest word after `@` that still asks for suggestions; a longer one is no mention.
const MENTION_MAX: usize = 40;

/// One line of the history.
#[derive(Debug, Clone, PartialEq)]
pub enum Item<'a> {
    /// The day of the messages below it.
    Day(NaiveDate),
    /// The divider "New": the messages below it came after the person last read.
    New,
    /// A message of the provider ("Dana created the conversation"), or a missed call: small, grey,
    /// centred.
    System(&'a Message),
    /// A message of a person. `first`: the first of a run (the author's name goes above it in a
    /// group); `last`: the last of a run (the time goes below it).
    Message { message: &'a Message, first: bool, last: bool },
}

/// The local day of `at`.
pub fn day_of(at: DateTime<Utc>) -> NaiveDate {
    datetime::to_local(at).date()
}

/// The history of `messages` (the oldest first): a day divider before the first message of each
/// day, "New" before the first message of someone else after the message number `new_after`, and
/// runs of one author ([`RUN_GAP`], no divider between them).
pub fn items(messages: &[Message], new_after: Option<u64>) -> Vec<Item<'_>> {
    let mut items = Vec::with_capacity(messages.len() + 4);
    let mut day = None;
    let mut new_shown = false;
    let mut previous: Option<&Message> = None;
    for message in messages {
        let today = day_of(message.at);
        let mut broken = false;
        if day != Some(today) {
            items.push(Item::Day(today));
            day = Some(today);
            broken = true;
        }
        let unread = new_after.is_some_and(|after| message.number() > after) && !message.author.own;
        if unread && !new_shown {
            items.push(Item::New);
            new_shown = true;
            broken = true;
        }
        if is_system(message) {
            items.push(Item::System(message));
            previous = None;
            continue;
        }
        let first = broken || previous.is_none_or(|before| !same_run(before, message));
        if first
            && let Some(Item::Message { last, .. }) = items.iter_mut().rev().find(|i| matches!(i, Item::Message { .. }))
        {
            *last = true;
        }
        items.push(Item::Message { message, first, last: false });
        previous = Some(message);
    }
    if let Some(Item::Message { last, .. }) = items.iter_mut().rev().find(|i| matches!(i, Item::Message { .. })) {
        *last = true;
    }
    items
}

/// A message of the provider, shown small: a system message or a missed call.
pub fn is_system(message: &Message) -> bool {
    matches!(message.kind, MessageKind::System | MessageKind::MissedCall)
}

/// Whether `next` continues the run of `before`: the same author, within [`RUN_GAP`].
fn same_run(before: &Message, next: &Message) -> bool {
    before.author.name == next.author.name
        && before.author.id == next.author.id
        && before.author.own == next.author.own
        && next.at - before.at <= RUN_GAP
}

/// The words of a day divider: "Today", "Yesterday", else the short date ("Sat, 3 Oct").
pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        tr("Today").to_owned()
    } else if today.pred_opt() == Some(day) {
        tr("Yesterday").to_owned()
    } else {
        datetime::format_short_date(day, today)
    }
}

/// The time of a message, "10:02", in local time.
pub fn time_label(at: DateTime<Utc>) -> String {
    datetime::format_time(datetime::to_local(at).time())
}

/// The word after an `@` the person is typing at the end of `before` (the text before the cursor):
/// `Some("be")` for `"Hi @be"`, `Some("")` right after the `@`. `None` when the cursor is in no such
/// word: no `@`, a space after it, an `@` inside a word (a mail address) or a word too long.
pub fn mention_query(before: &str) -> Option<&str> {
    let at = before.rfind('@')?;
    let query = &before[at + 1..];
    let starts_word = before[..at].chars().next_back().is_none_or(char::is_whitespace);
    let one_word = !query.chars().any(char::is_whitespace);
    (starts_word && one_word && query.chars().count() <= MENTION_MAX).then_some(query)
}

/// `before` with the word after its last `@` replaced by the mention's name and a space: what the
/// composer holds once a suggestion is picked. `before` unchanged when it ends in no mention.
pub fn with_mention(before: &str, mention: &Mention) -> String {
    match mention_query(before) {
        Some(query) => format!("{}@{} ", &before[..before.len() - query.len() - 1], mention.name),
        None => before.to_owned(),
    }
}

/// The text to send: every `@Name` of a mention the person picked becomes `@"<id>"`, the way the
/// provider takes mentions; all else stays as typed. Longer names first, so "@Ben Koch" is not taken
/// for "@Ben".
pub fn outgoing(text: &str, picked: &[Mention]) -> String {
    let mut picked: Vec<&Mention> = picked.iter().collect();
    picked.sort_by_key(|mention| std::cmp::Reverse(mention.name.len()));
    let mut text = text.trim().to_owned();
    for mention in picked {
        let id = mention.id.replace('"', "");
        text = text.replace(&format!("@{}", mention.name), &format!("@\"{id}\""));
    }
    text
}

/// `text` cut into pieces, each with whether it is a mention (`@Name` of one of `mentions`): the
/// history shows mentions stronger.
pub fn pieces(text: &str, mentions: &[Mention]) -> Vec<(String, bool)> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        // The earliest mention in what is left; of two at the same place the longer.
        let next = mentions
            .iter()
            .filter(|mention| !mention.name.is_empty())
            .filter_map(|mention| {
                let word = format!("@{}", mention.name);
                rest.find(&word).map(|at| (at, word.len()))
            })
            .min_by_key(|&(at, len)| (at, std::cmp::Reverse(len)));
        match next {
            Some((at, len)) => {
                if at > 0 {
                    pieces.push((rest[..at].to_owned(), false));
                }
                pieces.push((rest[at..at + len].to_owned(), true));
                rest = &rest[at + len..];
            }
            None => {
                pieces.push((rest.to_owned(), false));
                rest = "";
            }
        }
    }
    pieces
}

#[cfg(test)]
mod tests {
    use iced::futures::executor::block_on;
    use torro_chat::ChatSource;
    use torro_chat::demo::{BEN, DemoSource, TEAM};

    use super::*;

    fn page(conversation: &str) -> torro_chat::MessagePage {
        let demo = DemoSource::new(DateTime::from_timestamp(1_800_000_000, 0).expect("a time"));
        block_on(demo.messages(conversation.into(), None, None, None)).expect("messages")
    }

    fn kinds(items: &[Item<'_>]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Day(_) => "day".into(),
                Item::New => "new".into(),
                Item::System(_) => "system".into(),
                Item::Message { message, first, last } => {
                    format!("{}{}{}", message.number(), if *first { "<" } else { "" }, if *last { ">" } else { "" })
                }
            })
            .collect()
    }

    #[test]
    fn the_history_has_days_the_divider_new_and_runs() {
        let page = page(BEN);
        let read = page.last_read.as_deref().and_then(torro_chat::split_message).map(|(_, n)| n);
        assert_eq!(read, Some(3), "the demo has two unread");
        let items = items(&page.messages, read);
        let kinds = kinds(&items);
        // Ben, then Anna three minutes later on the day before; the missed call; Ben twice an hour apart.
        assert_eq!(kinds.iter().filter(|kind| *kind == "day").count(), 2, "{kinds:?}");
        assert!(kinds.contains(&"system".to_string()), "{kinds:?}");
        let new = kinds.iter().position(|kind| kind == "new").expect("the divider");
        assert_eq!(kinds[new + 1], "4<>", "{kinds:?}");
        assert_eq!(kinds.last().map(String::as_str), Some("5<>"), "an hour apart: runs of their own");
    }

    #[test]
    fn messages_of_one_author_close_together_are_one_run() {
        let mut messages = page(TEAM).messages;
        // Cem writes twice, a minute apart.
        let mut again = messages[2].clone();
        again.id = torro_chat::message_id(TEAM, 9);
        again.at += Duration::minutes(1);
        messages.insert(3, again);
        let kinds = kinds(&items(&messages, None));
        assert!(!kinds.contains(&"new".to_string()), "nothing new without a read mark");
        assert!(kinds.contains(&"3<".to_string()) && kinds.contains(&"9>".to_string()), "{kinds:?}");
    }

    #[test]
    fn own_messages_are_never_new() {
        let page = page(TEAM);
        assert!(!kinds(&items(&page.messages, Some(3))).contains(&"new".to_string()), "4 is Anna's own");
    }

    #[test]
    fn days_are_named_today_yesterday_or_by_date() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).expect("a day");
        assert_eq!(day_label(today, today), tr("Today"));
        assert_eq!(day_label(today.pred_opt().expect("a day"), today), tr("Yesterday"));
        let earlier = NaiveDate::from_ymd_opt(2026, 11, 3).expect("a day");
        assert_eq!(day_label(earlier, today), datetime::format_short_date(earlier, today));
    }

    #[test]
    fn a_mention_is_the_word_after_an_at_sign() {
        assert_eq!(mention_query("Hi @be"), Some("be"));
        assert_eq!(mention_query("@"), Some(""));
        assert_eq!(mention_query("Hi @ben "), None, "a space ends it");
        assert_eq!(mention_query("anna@example.org"), None, "a mail address is no mention");
        assert_eq!(mention_query("no at sign"), None);
        assert_eq!(mention_query(&format!("@{}", "x".repeat(MENTION_MAX + 1))), None);
    }

    #[test]
    fn picking_a_suggestion_writes_the_name_and_sending_writes_the_id() {
        let ben = Mention { id: "ben".into(), name: "Ben Koch".into() };
        let typed = with_mention("Can you check @be", &ben);
        assert_eq!(typed, "Can you check @Ben Koch ");
        assert_eq!(with_mention("nothing to replace", &ben), "nothing to replace");
        let sent = outgoing(&format!("{typed}page 3?"), std::slice::from_ref(&ben));
        assert_eq!(sent, "Can you check @\"ben\" page 3?");
        assert_eq!(outgoing("  @Ben Koch typed by hand  ", &[]), "@Ben Koch typed by hand", "only picked ones");
        let both = [Mention { id: "b".into(), name: "Ben".into() }, ben];
        assert_eq!(outgoing("@Ben Koch and @Ben", &both), "@\"ben\" and @\"b\"", "the longer name first");
    }

    #[test]
    fn mentions_in_a_message_are_pieces_of_their_own() {
        let mentions = [Mention { id: "anna".into(), name: "Anna Berg".into() }];
        assert_eq!(
            pieces("@Anna Berg can you check page 3?", &mentions),
            [("@Anna Berg".to_string(), true), (" can you check page 3?".to_string(), false)]
        );
        assert_eq!(pieces("plain", &mentions), [("plain".to_string(), false)]);
        assert!(pieces("", &mentions).is_empty());
    }
}
