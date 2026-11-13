//! What the history of a chat window shows, worked out apart from the widgets so it can be tested:
//! the messages with day dividers, the divider "New" after the last read message, the runs of one
//! author and the pictures sent at once as one grid ([`items`], [`Item::Album`]); mentions while typing ([`mention_query`], [`with_mention`]) and in a message
//! ([`pieces`]), and the text that goes out ([`outgoing`]); stage 7: which messages can be changed
//! and reacted to ([`is_changeable`], [`is_reactable`]), a reaction given or taken back
//! ([`apply_reaction`]) and a deletion as the history shows it ([`apply_deleted`]).
//!
//! Mentions: the composer shows `@Ben Koch`, as people read it; the provider wants `@"ben"`
//! (`docs/chat.md`, "Befehle": mentions are written as `@"<mentionId>"`). [`outgoing`] turns the one
//! into the other for the mentions the person picked from the suggestions, nothing else.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use torro_chat::{FileRef, Mention, Message, MessageKind};
use torro_core::i18n::tr;
use torro_gui::datetime;

/// Messages of one author closer than this stand together, with one name above and one time below.
pub const RUN_GAP: Duration = Duration::minutes(5);

/// Pictures of one author closer than this stand together as one grid (issue #674): Talk makes a
/// message of each file sent at once and stamps each when it is shared, so they come seconds apart.
pub const ALBUM_GAP: Duration = Duration::seconds(60);

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
    /// Pictures sent at once, as one grid (issue #674): each picture with its message, the oldest
    /// first; only the last may carry a caption. Replying, reactions and "…" go to the last message.
    /// `first` and `last` as for [`Item::Message`], the grid counting as one entry of its run.
    Album { tiles: Vec<(&'a Message, &'a FileRef)>, first: bool, last: bool },
}

/// The local day of `at`.
pub fn day_of(at: DateTime<Utc>) -> NaiveDate {
    datetime::to_local(at).date()
}

/// The history of `messages` (the oldest first): a day divider before the first message of each
/// day, "New" before the first message of someone else after the message number `new_after`, runs
/// of one author ([`RUN_GAP`], no divider between them) and pictures sent at once as one grid
/// ([`albums`]).
pub fn items(messages: &[Message], new_after: Option<u64>) -> Vec<Item<'_>> {
    albums(runs(messages, new_after))
}

/// The history without grids: every message on its own.
fn runs(messages: &[Message], new_after: Option<u64>) -> Vec<Item<'_>> {
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

/// `items` with pictures sent at once joined into [`Item::Album`]: two or more pictures in a row
/// ([`album_picture`]) of one run, each within [`ALBUM_GAP`] of the one before and no divider
/// between them; the first with a caption of its own ends the grid. A message with several files,
/// all pictures with a preview, is a grid of its own.
fn albums(items: Vec<Item<'_>>) -> Vec<Item<'_>> {
    let mut out = Vec::with_capacity(items.len());
    let mut pending: Vec<(&Message, bool, bool)> = Vec::new();
    for item in items {
        let Item::Message { message, first, last } = item else {
            flush_album(&mut out, &mut pending);
            out.push(item);
            continue;
        };
        if let Some(tiles) = own_album(message) {
            flush_album(&mut out, &mut pending);
            out.push(Item::Album { tiles, first, last });
            continue;
        }
        let Some(file) = album_picture(message) else {
            flush_album(&mut out, &mut pending);
            out.push(Item::Message { message, first, last });
            continue;
        };
        let joins = pending
            .last()
            .is_some_and(|&(before, _, _)| same_run(before, message) && message.at - before.at <= ALBUM_GAP);
        if !joins {
            flush_album(&mut out, &mut pending);
        }
        pending.push((message, first, last));
        if has_caption(message, file) {
            flush_album(&mut out, &mut pending);
        }
    }
    flush_album(&mut out, &mut pending);
    out
}

/// What `pending` holds goes out: two or more pictures as a grid, a single one as its message.
fn flush_album<'a>(out: &mut Vec<Item<'a>>, pending: &mut Vec<(&'a Message, bool, bool)>) {
    match pending.as_slice() {
        [] => {}
        [(message, first, last)] => out.push(Item::Message { message, first: *first, last: *last }),
        [(_, first, _), .., (_, _, last)] => {
            let (first, last) = (*first, *last);
            let tiles = pending.iter().map(|(message, _, _)| (*message, &message.files[0])).collect();
            out.push(Item::Album { tiles, first, last });
        }
    }
    pending.clear();
}

/// The picture of a message that can stand in a grid: a message of a person, no reply, exactly one
/// file, a picture the server draws a preview of.
fn album_picture(message: &Message) -> Option<&FileRef> {
    match message.files.as_slice() {
        [file] if message.kind == MessageKind::Message && message.reply_to.is_none() && is_album_file(file) => {
            Some(file)
        }
        _ => None,
    }
}

/// A message with several files, all pictures with a preview: its pictures as a grid of their own.
fn own_album(message: &Message) -> Option<Vec<(&Message, &FileRef)>> {
    let grid = message.kind == MessageKind::Message
        && message.reply_to.is_none()
        && message.files.len() >= 2
        && message.files.iter().all(is_album_file);
    grid.then(|| message.files.iter().map(|file| (message, file)).collect())
}

/// A picture the server draws a preview of; videos, SVGs and pictures without one stand alone.
fn is_album_file(file: &FileRef) -> bool {
    crate::view::has_preview(file) && file.content_type.starts_with("image/")
}

/// Whether `message` says more than the name of its picture: such a caption ends a grid.
fn has_caption(message: &Message, file: &FileRef) -> bool {
    let text = message.text.trim();
    !text.is_empty() && text != file.name
}

/// The first web link of `text` (`https://` or `http://` up to the next space): the one a link card
/// stands for, as in Talk.
pub fn first_link(text: &str) -> Option<&str> {
    let start = text.find("https://").or_else(|| text.find("http://"))?;
    let rest = &text[start..];
    rest.split_whitespace().next()
}

/// A message of the provider, shown small: a system message or a missed call.
pub fn is_system(message: &Message) -> bool {
    matches!(message.kind, MessageKind::System | MessageKind::MissedCall)
}

/// The reactions the message menu offers in one click, the most common first.
pub const QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🎉"];

/// A message people can react to: one a person wrote (or recorded), not deleted, not of the provider.
pub fn is_reactable(message: &Message) -> bool {
    matches!(message.kind, MessageKind::Message | MessageKind::Voice)
}

/// A message the person may try to edit or delete: their own and [`is_reactable`]. Whether it is
/// still within the provider's time is for the core to say (`unsupported`).
pub fn is_changeable(message: &Message) -> bool {
    message.author.own && is_reactable(message)
}

/// `message` with the person's reaction `emoji` given, or taken back with `remove`, as the core
/// answers it: the count goes up or down by one, a reaction nobody gives any more goes; giving one
/// twice or taking back one not given changes nothing. The window shows it at once and takes it
/// back when the core refuses (`chat.react`).
pub fn apply_reaction(message: &mut Message, emoji: &str, remove: bool) {
    let given = message.own_reactions.iter().any(|own| own == emoji);
    if remove && given {
        message.own_reactions.retain(|own| own != emoji);
        if let Some(count) = message.reactions.get_mut(emoji) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                message.reactions.remove(emoji);
            }
        }
    } else if !remove && !given {
        message.own_reactions.push(emoji.to_owned());
        *message.reactions.entry(emoji.to_owned()).or_insert(0) += 1;
    }
}

/// `message` as it stands once the core deleted it (`chat.delete_message`): in its place, without
/// text, files or reactions, saying "This message was deleted."
pub fn apply_deleted(message: &mut Message) {
    message.kind = MessageKind::Deleted;
    message.text.clear();
    message.mentions.clear();
    message.files.clear();
    message.reactions.clear();
    message.own_reactions.clear();
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
    use torro_chat::demo::{BEN, DESIGN, DemoSource, TEAM};

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
                Item::Album { tiles, first, last } => {
                    let numbers: Vec<String> = tiles.iter().map(|(message, _)| message.number().to_string()).collect();
                    format!("[{}]{}{}", numbers.join(","), if *first { "<" } else { "" }, if *last { ">" } else { "" })
                }
            })
            .collect()
    }

    /// The demo's "Design sync": Gesa's four header pictures, sent at once, are 13 to 16.
    fn design() -> Vec<Message> {
        page(DESIGN).messages
    }

    fn numbered(messages: &mut [Message], number: u64) -> &mut Message {
        messages.iter_mut().find(|message| message.number() == number).expect("the message")
    }

    #[test]
    fn pictures_sent_at_once_are_one_grid() {
        let messages = design();
        let kinds = kinds(&items(&messages, None));
        assert_eq!(kinds.last().map(String::as_str), Some("[13,14,15,16]<>"), "one entry of its run: {kinds:?}");
        assert!(kinds.contains(&"3<>".to_string()), "a single picture stays a message: {kinds:?}");
    }

    #[test]
    fn a_caption_ends_the_grid() {
        let mut messages = design();
        numbered(&mut messages, 14).text = "This one?".into();
        let kinds = kinds(&items(&messages, None));
        assert_eq!(kinds[kinds.len() - 2..], ["[13,14]<", "[15,16]>"], "{kinds:?}");
    }

    #[test]
    fn a_minute_and_a_second_apart_is_no_grid() {
        let mut messages = design();
        for number in 14..=16 {
            numbered(&mut messages, number).at += ALBUM_GAP + Duration::seconds(1);
        }
        let kinds = kinds(&items(&messages, None));
        assert_eq!(kinds[kinds.len() - 2..], ["13<", "[14,15,16]>"], "{kinds:?}");
    }

    #[test]
    fn quotes_videos_and_svgs_never_stand_in_a_grid() {
        let mut messages = design();
        let reply = numbered(&mut messages, 3).id.clone();
        numbered(&mut messages, 14).reply_to =
            Some(torro_chat::Quote { id: reply, author: "Gesa Lange".into(), text: "First drafts".into() });
        let quoted = kinds(&items(&messages, None));
        assert_eq!(quoted[quoted.len() - 3..], ["13<", "14", "[15,16]>"], "the quote stands alone: {quoted:?}");
        let mut messages = design();
        for (number, kind) in [(14, "video/mp4"), (15, "image/svg+xml")] {
            numbered(&mut messages, number).files[0].content_type = kind.into();
        }
        let mixed = kinds(&items(&messages, None));
        assert!(!mixed.iter().any(|kind| kind.starts_with('[')), "{mixed:?}");
    }

    #[test]
    fn several_pictures_of_one_message_are_a_grid() {
        let mut messages = design();
        let extra = numbered(&mut messages, 14).files[0].clone();
        let three = numbered(&mut messages, 3);
        three.files.push(extra);
        let kinds = kinds(&items(&messages, None));
        assert!(kinds.contains(&"[3,3]<>".to_string()), "{kinds:?}");
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
    fn a_reaction_counts_once_per_person() {
        let mut cem = page(TEAM).messages.remove(2);
        assert_eq!(cem.own_reactions, ["👍"], "the demo's own reaction");
        apply_reaction(&mut cem, "👍", false);
        assert_eq!(cem.reactions.get("👍"), Some(&2), "given already: nothing changes");
        apply_reaction(&mut cem, "👍", true);
        assert_eq!((cem.reactions.get("👍"), cem.own_reactions.len()), (Some(&1), 0));
        apply_reaction(&mut cem, "😮", false);
        apply_reaction(&mut cem, "😮", true);
        assert!(!cem.reactions.contains_key("😮"), "nobody gives it any more");
    }

    #[test]
    fn only_own_messages_of_people_can_be_changed() {
        let team = page(TEAM).messages;
        assert!(!is_reactable(&team[0]) && !is_changeable(&team[0]), "the provider's message");
        assert!(is_reactable(&team[2]) && !is_changeable(&team[2]), "Cem's");
        assert!(is_changeable(&team[3]), "Anna's own");
        let mut gone = team[3].clone();
        apply_deleted(&mut gone);
        assert!(!is_reactable(&gone) && gone.text.is_empty() && gone.reactions.is_empty());
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
