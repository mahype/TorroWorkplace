//! The group card (issue #786, `assistant.show_items`, "Grenzen" in `docs/inhaltskarten.md`): several items a
//! command of the conversation found, each shown as it would be alone.
//!
//! Three steps, all of them here:
//!
//! 1. **What a command found** ([`seen`]): after `mail.search_messages`, `mail.get_message`, `calendar.list_events`,
//!    `tasks.list_todos`, `contacts.list_contacts` and `contacts.get_contact` the loop keeps the ids of the items
//!    the answer named with the tool line ([`Line::Seen`], `Assistant::see` in `store`), and how
//!    many the command found in all. Engine A reads the core's own answer, engine B the answer the agent hands back
//!    as text ([`seen_in`]); an id made up there does no harm, as every card is looked up with the rights of the
//!    conversation's client.
//! 2. **What the model shows** ([`Assistant::show_items`]): only items found so in the same conversation, at most
//!    [`MAX_SHOWN_ITEMS`], picked by the model (decision 5 of the plan of issue #784). One item becomes its own
//!    card, more a [`CardRef::Group`] with `total` from what the search counted, never from the model's input.
//! 3. **What a group shows now** ([`look`]): each item looked up alone.

use std::collections::BTreeMap;

use chrono::Utc;
use torro_ipc::Response;
use torro_ipc::api::Area;
use torro_ipc::assistant::{Card, CardRef, EntryContent, ItemRef, MAX_SHOWN_ITEMS, items_refusal};

use super::Lookup;
use crate::assistant::Assistant;
use crate::assistant::store::{Conversation, Line, Seen};

/// The commands whose answer is kept as what the conversation found ([`seen`], [`Line::Seen`]), by their catalogue
/// names. The plan names `mail.list_messages` too; the catalogue has no such command (a folder is listed with
/// `mail.search_messages` and `folder`), so it is not here.
pub const FINDS: [&str; 6] = [
    "mail.search_messages",
    "mail.get_message",
    "calendar.list_events",
    "tasks.list_todos",
    "contacts.list_contacts",
    "contacts.get_contact",
];

/// What the command `name` (its catalogue name) found by its answer `response`: the ids of the mails, events, tasks
/// or contacts it named, in its order, and how many it found in all (for `mail.search_messages` its
/// `total_estimate` when that is more than the page holds). `None` for every other command, an answer of another
/// kind (an error), and an answer that names nothing.
pub fn seen(name: &str, response: &Response) -> Option<Seen> {
    let (refs, counted): (Vec<ItemRef>, Option<u64>) = match (name, response) {
        ("mail.search_messages", Response::MailMessages { messages, total_estimate, .. }) => (
            messages
                .iter()
                .map(|message| ItemRef::Mail { account: message.account.clone(), id: message.id.clone() })
                .collect(),
            *total_estimate,
        ),
        ("mail.get_message", Response::MailMessage { message }) => {
            let summary = &message.summary;
            (vec![ItemRef::Mail { account: summary.account.clone(), id: summary.id.clone() }], None)
        }
        ("calendar.list_events", Response::Events { events }) => (
            events
                .iter()
                .map(|event| ItemRef::Event { uid: event.uid.clone(), recurrence_id: event.recurrence_id })
                .collect(),
            None,
        ),
        ("tasks.list_todos", Response::Todos { todos }) => {
            (todos.iter().map(|todo| ItemRef::Todo { uid: todo.uid.clone() }).collect(), None)
        }
        ("contacts.list_contacts", Response::Contacts(page)) => {
            (page.contacts.iter().map(|contact| ItemRef::Contact { id: contact.id.clone() }).collect(), None)
        }
        ("contacts.get_contact", Response::Contact { contact }) => {
            (vec![ItemRef::Contact { id: contact.id.clone() }], None)
        }
        _ => return None,
    };
    if refs.is_empty() {
        return None;
    }
    let total = counted.unwrap_or(0).max(refs.len() as u64);
    Some(Seen { refs, total })
}

/// [`seen`] for engine B, whose agent hands over the core's answer as the tool result `text` (the answer's JSON, as
/// the MCP server writes it); `None` for a text that is no answer.
pub fn seen_in(name: &str, text: &str) -> Option<Seen> {
    if !FINDS.contains(&name) {
        return None;
    }
    serde_json::from_str::<Response>(text).ok().and_then(|response| seen(name, &response))
}

/// Whose conversations `assistant.show_items` may add a card to (issue #786): a client its own only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner<'a> {
    /// The person (the app key): any conversation it names; none answers for it, so it must name one.
    Person,
    /// The assistant's own client: the conversations without a bot.
    Assistant,
    /// A bot's client: the conversations with this bot, by the bot's id ([`torro_ipc::bots::Bot::id`]).
    Bot(&'a str),
    /// Any other client: none.
    Other,
}

impl Owner<'_> {
    /// Whether `conversation` is one of the owner's.
    pub fn owns(&self, conversation: &Conversation) -> bool {
        match self {
            Owner::Person => true,
            Owner::Assistant => conversation.bot.is_none(),
            Owner::Bot(bot) => conversation.bot.as_deref() == Some(*bot),
            Owner::Other => false,
        }
    }
}

/// The card `assistant.show_items` adds for `items` in a conversation that found `seen` ([`Conversation::seen`]):
/// each item as a command found it (an event without its occurrence matches any found occurrence of it, and the
/// card keeps the one found), each once, in the order given. One item becomes its own card ([`ItemRef::card`]),
/// more a [`CardRef::Group`] of the area of the first item, with `total` when one command found all of them (the
/// newest such) and counted more than [`MAX_SHOWN_ITEMS`]. `None` when an item is not among what was found, and for
/// no items.
pub fn shown(seen: &BTreeMap<u64, Seen>, items: &[ItemRef]) -> Option<CardRef> {
    let mut refs: Vec<ItemRef> = Vec::with_capacity(items.len());
    for item in items {
        let found = seen.values().rev().flat_map(|seen| seen.refs.iter()).find(|found| matches(item, found))?;
        if !refs.contains(found) {
            refs.push(found.clone());
        }
    }
    let area = area_of(refs.first()?)?;
    if let [one] = refs.as_slice() {
        return Some(one.card());
    }
    let total = seen
        .values()
        .rev()
        .find(|seen| refs.iter().all(|item| seen.refs.contains(item)))
        .map(|seen| seen.total)
        .filter(|total| *total > MAX_SHOWN_ITEMS as u64);
    Some(CardRef::Group { items: refs, area, total })
}

/// Whether the item `item` a model named is `found`, as a command answered it.
fn matches(item: &ItemRef, found: &ItemRef) -> bool {
    match (item, found) {
        (ItemRef::Mail { account, id }, ItemRef::Mail { account: in_account, id: in_id }) => {
            account == in_account && id == in_id
        }
        (ItemRef::Event { uid, recurrence_id }, ItemRef::Event { uid: in_uid, recurrence_id: in_recurrence }) => {
            uid == in_uid && (recurrence_id.is_none() || recurrence_id == in_recurrence)
        }
        (ItemRef::Todo { uid }, ItemRef::Todo { uid: in_uid }) => uid == in_uid,
        (ItemRef::Contact { id }, ItemRef::Contact { id: in_id }) => id == in_id,
        _ => false,
    }
}

/// The area of an item's kind: which app "Show all" opens.
fn area_of(item: &ItemRef) -> Option<Area> {
    match item {
        ItemRef::Mail { .. } => Some(Area::Mail),
        ItemRef::Event { .. } => Some(Area::Calendar),
        ItemRef::Todo { .. } => Some(Area::Tasks),
        ItemRef::Contact { .. } => Some(Area::Contacts),
        ItemRef::Unknown => None,
    }
}

/// An error answer with `code`; the message is for the model and the log of the client, so English.
fn refusal(code: &str, message: &str) -> Response {
    Response::Error { code: code.into(), message: message.into() }
}

impl Assistant {
    /// `assistant.show_items` (issue #786) of `owner`: adds the card [`shown`] makes of `items` to the conversation
    /// `conversation` and answers [`Response::Ok`]. Without `conversation`, the owner's conversation whose answer
    /// runs now (of several, one whose commands found the items, the newest first). The core maps the caller to
    /// `owner` and checked its rights and kind before.
    ///
    /// Codes: as [`items_refusal`] (`error` for none, `forbidden` for more than [`MAX_SHOWN_ITEMS`] or another kind);
    /// `not-found` for a conversation that is not there or not the owner's, and without `conversation` when none
    /// of the owner's answers now (the person's never does); `forbidden` for an item no command of the conversation
    /// found; `error` when the card cannot be stored.
    pub(crate) async fn show_items(&self, owner: Owner<'_>, conversation: Option<&str>, items: &[ItemRef]) -> Response {
        if let Some(refused) = items_refusal(items) {
            return refused;
        }
        let mut state = self.state().await;
        let id = match conversation {
            Some(id) => state.conversations.get(id).filter(|conversation| owner.owns(conversation)).map(|_| id),
            None if owner == Owner::Person => None,
            None => {
                let mut answering: Vec<&Conversation> = state
                    .conversations
                    .values()
                    .filter(|conversation| state.running.contains_key(&conversation.id) && owner.owns(conversation))
                    .collect();
                // The newest first, those that found the items before the others.
                answering.sort_by_key(|conversation| std::cmp::Reverse(conversation.updated()));
                answering.sort_by_key(|conversation| shown(&conversation.seen, items).is_none());
                answering.first().map(|conversation| conversation.id.as_str())
            }
        };
        let Some(id) = id.map(str::to_string) else {
            return refusal("not-found", "no such conversation of yours");
        };
        let Some(entry) = state.conversations.get(&id).and_then(|conversation| {
            let card = shown(&conversation.seen, items)?;
            Some(conversation.next(EntryContent::Card { card }, Utc::now()))
        }) else {
            return refusal(
                "forbidden",
                "only mails, events, tasks and contacts a command of this conversation found can be shown; use the ids \
                 its answers gave",
            );
        };
        if self.write(&id, vec![Line::Entry { entry: entry.clone() }]).await.is_err() {
            return refusal("error", "the card cannot be stored");
        }
        if let Some(conversation) = state.conversations.get_mut(&id) {
            conversation.apply(Line::Entry { entry });
        }
        Response::Ok
    }
}

/// What the group of `items` of the area `area` shows now: [`Card::Group`] with each item looked up alone
/// ([`super::look_item`]; one out of reach is [`Card::Unavailable`], a kind that may not stand in a group as well), at
/// most [`MAX_SHOWN_ITEMS`] in their order, and `total` as it was stored (the core set it from the search, never a
/// model).
pub async fn look(lookup: &Lookup<'_>, items: &[ItemRef], area: Area, total: Option<u64>) -> Card {
    let mut shown = Vec::with_capacity(items.len().min(MAX_SHOWN_ITEMS));
    for item in items.iter().take(MAX_SHOWN_ITEMS) {
        shown.push(super::look_item(lookup, item).await);
    }
    Card::Group { items: shown, area, total }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};
    use torro_ipc::assistant::Entry;

    use super::*;
    use crate::assistant::store;
    use crate::assistant::tests::{assistant_in, growing, message, tool, written};

    const ANNA: &str = "mail:anna@example.org";
    const OWN: &str = "0123456789abcdef";
    const BOTS: &str = "fedcba9876543210";

    fn mail(n: usize) -> ItemRef {
        ItemRef::Mail { account: ANNA.into(), id: format!("m:{n}") }
    }

    /// The answer of `mail.search_messages` with the mails 0 to `count` − 1 on its page, counting `total` in all.
    fn searched(count: usize, total: u64) -> Response {
        let messages: Vec<Value> = (0..count)
            .map(|n| {
                json!({"id": format!("m:{n}"), "account": ANNA, "folder": "f:1", "thread_id": format!("t:{n}"),
                       "date": "2026-11-13T09:00:00Z"})
            })
            .collect();
        let answer =
            json!({"result": "mail-messages", "messages": messages, "source": "cache", "total_estimate": total});
        serde_json::from_value(answer).expect("a search")
    }

    /// Issue #786: the ids of what a search or a list found, with how many in all; nothing for another command,
    /// an error or an empty list. Engine B reads the same from the agent's text.
    #[test]
    fn a_search_or_a_list_tells_what_it_found() {
        let found = seen("mail.search_messages", &searched(3, 14)).expect("found");
        assert_eq!(found, Seen { refs: vec![mail(0), mail(1), mail(2)], total: 14 });
        assert_eq!(
            seen("mail.search_messages", &searched(3, 0)).map(|found| found.total),
            Some(3),
            "at least the page"
        );
        let events: Response = serde_json::from_value(json!({"result": "events", "events": [{
            "uid": "e1", "collection": "/cal/work/", "summary": "Team sync",
            "start": {"datetime": "2026-11-16T13:00:00Z"}, "recurring": false
        }]}))
        .expect("events");
        assert_eq!(
            seen("calendar.list_events", &events).map(|found| found.refs),
            Some(vec![ItemRef::Event { uid: "e1".into(), recurrence_id: None }])
        );
        let todos: Response = serde_json::from_value(json!({"result": "todos", "todos": [{
            "uid": "t1", "collection": "/tasks/work/", "summary": "Send the offer", "completed": false
        }]}))
        .expect("todos");
        assert_eq!(
            seen("tasks.list_todos", &todos).map(|found| found.refs),
            Some(vec![ItemRef::Todo { uid: "t1".into() }])
        );
        assert_eq!(seen("calendar.list_events", &searched(3, 3)), None, "another command's answer");
        assert_eq!(seen("mail.delete_messages", &searched(3, 3)), None, "another command");
        assert_eq!(seen("mail.search_messages", &searched(0, 0)), None, "nothing found");
        let failed = Response::Error { code: "forbidden".into(), message: "off".into() };
        assert_eq!(seen("mail.search_messages", &failed), None);

        let text = serde_json::to_string(&searched(2, 2)).expect("json");
        assert_eq!(seen_in("mail.search_messages", &text).map(|found| found.refs), Some(vec![mail(0), mail(1)]));
        assert_eq!(seen_in("mail.search_messages", "Done"), None);
        assert_eq!(seen_in("tools.create_pdf", &text), None);
    }

    /// What a conversation found: the search's tool line is entry 2.
    fn found(refs: Vec<ItemRef>, total: u64) -> BTreeMap<u64, Seen> {
        BTreeMap::from([(2, Seen { refs, total })])
    }

    /// Issue #786: one item is its own card, more a group of their area; an item no command found is refused, and
    /// the card keeps each item as it was found.
    #[test]
    fn only_found_items_are_shown_one_alone() {
        let seen = found((0..3).map(mail).collect(), 3);
        assert_eq!(shown(&seen, &[mail(1)]), Some(mail(1).card()), "one item is its own card");
        assert_eq!(
            shown(&seen, &[mail(2), mail(0), mail(2)]),
            Some(CardRef::Group { items: vec![mail(2), mail(0)], area: Area::Mail, total: None })
        );
        assert_eq!(shown(&seen, &[mail(0), mail(7)]), None, "a mail no command found");
        let other_account = ItemRef::Mail { account: "mail:ben@example.org".into(), id: "m:0".into() };
        assert_eq!(shown(&seen, &[other_account]), None, "the same id in another account");
        assert_eq!(shown(&seen, &[ItemRef::Todo { uid: "m:0".into() }]), None, "another kind");

        let at = torro_pim::EventTime::Date(chrono::NaiveDate::from_ymd_opt(2026, 11, 16).expect("a date"));
        let occurrence = ItemRef::Event { uid: "e1".into(), recurrence_id: Some(at) };
        let events = found(vec![occurrence.clone()], 1);
        let series = ItemRef::Event { uid: "e1".into(), recurrence_id: None };
        assert_eq!(shown(&events, &[series]), Some(occurrence.card()), "the occurrence found");
    }

    /// Issue #786: eleven hits set the total, ten do not; items of two searches have none.
    #[test]
    fn eleven_hits_set_the_total() {
        let eleven = found((0..11).map(mail).collect(), 11);
        let ten: Vec<ItemRef> = (0..10).map(mail).collect();
        let group = |items: Vec<ItemRef>, total| CardRef::Group { items, area: Area::Mail, total };
        assert_eq!(shown(&eleven, &ten), Some(group(ten.clone(), Some(11))));
        assert_eq!(shown(&eleven, &ten[..3]), Some(group(ten[..3].to_vec(), Some(11))), "three of eleven");
        let only_ten = found(ten.clone(), 10);
        assert_eq!(shown(&only_ten, &ten), Some(group(ten.clone(), None)));
        let mut two = found(vec![mail(0)], 30);
        two.insert(5, Seen { refs: vec![mail(1)], total: 30 });
        assert_eq!(shown(&two, &[mail(0), mail(1)]), Some(group(vec![mail(0), mail(1)], None)), "two searches");
    }

    fn search_tool() -> torro_ipc::assistant::EntryContent {
        tool("mail.search_messages")
    }

    /// The assistant's conversation [`OWN`] and a bot's [`BOTS`], each with a search that found the mails 0 to 10
    /// at entry 2.
    fn conversations(dir: &std::path::Path) -> Assistant {
        let folder = dir.join("assistant");
        written(&folder, OWN, "Mails", vec![message("Mails from Anna?"), search_tool()], &[], |_| {});
        written(&folder, BOTS, "Bot", vec![message("Mails?"), search_tool()], &[], |conversation| {
            conversation.bot = Some("b0b0b0b0".into());
        });
        for id in [OWN, BOTS] {
            let line = Line::Seen { seq: 2, refs: (0..11).map(mail).collect(), total: 11 };
            store::append(&folder, id, &[line]).expect("written");
        }
        assistant_in(dir, Duration::ZERO)
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            _ => "ok",
        }
    }

    async fn last(assistant: &Assistant, id: &str) -> Option<Entry> {
        assistant.state().await.conversations.get(id).and_then(|conversation| conversation.entries.last().cloned())
    }

    /// Issue #786, end to end in the store: a found item becomes a card in its own conversation and stays when the
    /// conversation is read again; an id no command found is `forbidden`, another's conversation `not-found`, and
    /// nothing is added then.
    #[tokio::test]
    async fn show_items_adds_a_card_only_for_found_items_in_own_conversations() {
        let dir = tempfile::tempdir().expect("temp dir");
        let assistant = conversations(dir.path());

        let shown = assistant.show_items(Owner::Assistant, Some(OWN), &[mail(4)]).await;
        assert_eq!(shown, Response::Ok);
        let card = last(&assistant, OWN).await.expect("an entry");
        assert_eq!(card.content, EntryContent::Card { card: mail(4).card() });
        let again = store::load_all(&dir.path().join("assistant")).expect("read");
        assert!(again.iter().any(|conversation| conversation.entries.last() == Some(&card)), "stored");

        let foreign = assistant.show_items(Owner::Assistant, Some(OWN), &[mail(4), mail(99)]).await;
        assert_eq!(code(&foreign), "forbidden", "a mail no command found");
        let group = [mail(0), ItemRef::Unknown];
        assert_eq!(code(&assistant.show_items(Owner::Assistant, Some(OWN), &group).await), "forbidden", "a group");
        let too_many: Vec<ItemRef> = (0..11).map(mail).collect();
        assert_eq!(code(&assistant.show_items(Owner::Assistant, Some(OWN), &too_many).await), "forbidden");

        let theirs = assistant.show_items(Owner::Assistant, Some(BOTS), &[mail(1)]).await;
        assert_eq!(code(&theirs), "not-found", "the bot's conversation");
        let mine = assistant.show_items(Owner::Bot("b0b0b0b0"), Some(OWN), &[mail(1)]).await;
        assert_eq!(code(&mine), "not-found", "the assistant's conversation for the bot");
        let other = assistant.show_items(Owner::Bot("a1a1a1a1"), Some(BOTS), &[mail(1)]).await;
        assert_eq!(code(&other), "not-found", "another bot's");
        assert_eq!(code(&assistant.show_items(Owner::Other, Some(OWN), &[mail(1)]).await), "not-found");
        assert_eq!(
            code(&assistant.show_items(Owner::Assistant, Some("0000000000000000"), &[mail(1)]).await),
            "not-found"
        );
        assert_eq!(last(&assistant, OWN).await, Some(card), "nothing added");
        assert_eq!(last(&assistant, BOTS).await.map(|entry| entry.seq), Some(2));

        let bots =
            assistant.show_items(Owner::Bot("b0b0b0b0"), Some(BOTS), &(0..10).map(mail).collect::<Vec<_>>()).await;
        assert_eq!(bots, Response::Ok);
        let group = CardRef::Group { items: (0..10).map(mail).collect(), area: Area::Mail, total: Some(11) };
        assert_eq!(last(&assistant, BOTS).await.map(|entry| entry.content), Some(EntryContent::Card { card: group }));
        assert_eq!(assistant.show_items(Owner::Person, Some(BOTS), &[mail(3)]).await, Response::Ok, "the person's key");
    }

    /// Issue #786: without a conversation, the owner's whose answer runs now; none running is `not-found`, and the
    /// person never has one.
    #[tokio::test]
    async fn show_items_without_a_conversation_takes_the_running_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let assistant = conversations(dir.path());
        assert_eq!(code(&assistant.show_items(Owner::Assistant, None, &[mail(1)]).await), "not-found", "none runs");
        let _running = growing(&assistant, OWN).await;
        let _theirs = growing(&assistant, BOTS).await;
        assert_eq!(assistant.show_items(Owner::Assistant, None, &[mail(1)]).await, Response::Ok);
        assert_eq!(
            last(&assistant, OWN).await.map(|entry| entry.content),
            Some(EntryContent::Card { card: mail(1).card() })
        );
        assert_eq!(last(&assistant, BOTS).await.map(|entry| entry.seq), Some(2), "not the bot's");
        assert_eq!(code(&assistant.show_items(Owner::Assistant, None, &[mail(42)]).await), "forbidden");
        assert_eq!(code(&assistant.show_items(Owner::Person, None, &[mail(1)]).await), "not-found");
    }
}
