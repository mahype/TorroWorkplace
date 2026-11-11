//! Sample conversations and people in memory, for building and trying the chat windows without a
//! server (`torrochat --demo`, the Notification Hub's demo). Changes live until the app closes.
//!
//! One Nextcloud account with six people in every status, two groups and three direct chats: one
//! with a mention, a file, a system message and a missed call (`docs/chat.md`, "Demo").
//!
//! The sample content is data, not text of the suite, so it is not translated (as in
//! `torro_pim::demo`).

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Duration, Utc};

use crate::model::{
    Author, Conversation, ConversationKind, FileRef, Mention, Message, MessageKind, MessagePage, NotifyLevel,
    OwnStatus, Person, Preview, Quote, Status, conversation_id, message_id, person_id, split_message, split_person,
};
use crate::source::{ChatError, ChatSource, Outgoing, StatusChange};

/// The account of the sample.
pub const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
/// The user id of the sample's own person.
pub const ME: &str = "anna";
/// The direct chat with Ben: mention, file, system message, missed call.
pub const BEN: &str = "nextcloud:anna@cloud.example.org/ben4chat";
/// The group "Team Alpha".
pub const TEAM: &str = "nextcloud:anna@cloud.example.org/team4alpha";
/// The direct chat with Lena Hoffmann of [`DemoSource::for_hub`]: the conversation of the Talk entry
/// of the inbox's demo (`torro_inbox::demo`, thread `k7pq2xd9`). Not part of [`DemoSource::new`].
pub const LENA: &str = "nextcloud:anna@cloud.example.org/k7pq2xd9";

/// The most messages one page holds.
const PAGE_MAX: usize = 100;

#[derive(Debug)]
struct Data {
    conversations: Vec<Conversation>,
    messages: BTreeMap<String, Vec<Message>>,
    people: Vec<Person>,
    own: OwnStatus,
    next: u64,
}

/// The sample; cheap to share behind an `Arc`.
#[derive(Debug)]
pub struct DemoSource {
    data: Mutex<Data>,
}

impl Default for DemoSource {
    fn default() -> Self {
        DemoSource::new(Utc::now())
    }
}

impl DemoSource {
    /// The sample as it looks at `now`: messages lie in the hours before it.
    pub fn new(now: DateTime<Utc>) -> DemoSource {
        DemoSource { data: Mutex::new(sample(now)) }
    }

    /// The sample of [`DemoSource::new`] and the direct chat [`LENA`], for the Notification Hub's demo
    /// (issue #643): 24 messages – more than the panel's first page of 20, so "Show earlier" has
    /// something to load –, a system message, an answer with its quote, the person's own messages,
    /// and the last two unread, so the divider "New" stands before them. Lena is not on the list of
    /// people, as someone met only in a chat.
    pub fn for_hub(now: DateTime<Utc>) -> DemoSource {
        let mut data = sample(now);
        let messages = lena(now);
        let mut chat = conversation(LENA, "Lena Hoffmann", ConversationKind::Direct, &messages, 2);
        chat.person = Some(person_id(ACCOUNT, "lena"));
        data.conversations.push(chat);
        data.messages.insert(LENA.to_string(), messages);
        DemoSource { data: Mutex::new(data) }
    }

    /// The data; a lock a panicking thread left is taken over, the sample cannot break by that.
    fn lock(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn person(user: &str, name: &str, status: Status, message: Option<&str>) -> Person {
    Person {
        id: person_id(ACCOUNT, user),
        account: ACCOUNT.into(),
        name: name.into(),
        status,
        unknown: false,
        message: message.map(str::to_string),
        icon: None,
        clear_at: None,
        added: false,
        untrusted: true,
    }
}

fn text(conversation: &str, number: u64, user: &str, name: &str, at: DateTime<Utc>, text: &str) -> Message {
    Message {
        id: message_id(conversation, number),
        author: Author { id: Some(person_id(ACCOUNT, user)), name: name.into(), own: user == ME },
        at,
        text: text.into(),
        kind: MessageKind::Message,
        mentions: Vec::new(),
        files: Vec::new(),
        reply_to: None,
        reactions: BTreeMap::new(),
        own_reactions: Vec::new(),
        edited: false,
        untrusted: true,
    }
}

fn system(conversation: &str, number: u64, at: DateTime<Utc>, kind: MessageKind, text: &str) -> Message {
    Message {
        id: message_id(conversation, number),
        author: Author::default(),
        at,
        text: text.into(),
        kind,
        mentions: Vec::new(),
        files: Vec::new(),
        reply_to: None,
        reactions: BTreeMap::new(),
        own_reactions: Vec::new(),
        edited: false,
        untrusted: true,
    }
}

fn conversation(id: &str, name: &str, kind: ConversationKind, messages: &[Message], unread: u64) -> Conversation {
    let last = messages.last();
    let read = messages.len().saturating_sub(unread as usize).checked_sub(1).and_then(|at| messages.get(at));
    Conversation {
        id: id.into(),
        account: ACCOUNT.into(),
        name: name.into(),
        kind,
        unread,
        mentioned: false,
        mentioned_directly: false,
        last_message: last.map(|message| Preview {
            author: message.author.name.clone(),
            text: message.text.clone(),
            system: message.kind != MessageKind::Message,
        }),
        last_activity: last.map(|message| message.at).unwrap_or_default(),
        last_read: read.map(|message| message.id.clone()),
        person: None,
        has_call: false,
        ringing: false,
        favorite: false,
        read_only: false,
        federated: false,
        notify: NotifyLevel::Default,
        untrusted: true,
    }
}

fn sample(now: DateTime<Utc>) -> Data {
    let ago = |minutes: i64| now - Duration::minutes(minutes);
    let people = vec![
        person("ben", "Ben Koch", Status::Online, Some("In the office")),
        person("cem", "Cem Demir", Status::Away, None),
        person("dana", "Dana Erb", Status::Busy, Some("In a meeting")),
        person("elif", "Elif Aydin", Status::Dnd, Some("Writing, back at 3")),
        person("finn", "Finn Maier", Status::Offline, None),
        person("gesa", "Gesa Lange", Status::Online, None),
    ];

    let mut ben = vec![
        text(BEN, 1, "ben", "Ben Koch", ago(26 * 60), "Morning! Did you see the new start page?"),
        text(BEN, 2, ME, "Anna Berg", ago(26 * 60 - 3), "Not yet, I look at it after lunch."),
        system(BEN, 3, ago(5 * 60), MessageKind::MissedCall, "You missed a call from Ben Koch"),
        text(BEN, 4, "ben", "Ben Koch", ago(62), "Here is the offer, second draft."),
        text(BEN, 5, "ben", "Ben Koch", ago(12), "@Anna Berg can you check page 3 before four?"),
    ];
    ben[3].files = vec![FileRef {
        id: "4711".into(),
        name: "Offer.pdf".into(),
        size: 1_258_291,
        content_type: "application/pdf".into(),
        preview: false,
        hide_download: false,
        width: None,
        height: None,
    }];
    ben[4].mentions = vec![Mention { id: ME.into(), name: "Anna Berg".into() }];

    let mut team = vec![
        system(TEAM, 1, ago(3 * 24 * 60), MessageKind::System, "Dana Erb created the conversation"),
        text(TEAM, 2, "dana", "Dana Erb", ago(190), "Release notes are in the wiki."),
        text(TEAM, 3, "cem", "Cem Demir", ago(185), "Thanks! I add the screenshots today."),
        text(TEAM, 4, ME, "Anna Berg", ago(180), "I can review tomorrow morning."),
    ];
    team[2].reply_to = Some(Quote {
        id: message_id(TEAM, 2),
        author: "Dana Erb".into(),
        text: "Release notes are in the wiki.".into(),
    });
    team[2].reactions = BTreeMap::from([("👍".to_string(), 2)]);

    let design = conversation_id(ACCOUNT, "design4sync");
    let design_messages = vec![
        text(&design, 1, "gesa", "Gesa Lange", ago(2 * 24 * 60), "Icons for the new app are done."),
        text(&design, 2, "elif", "Elif Aydin", ago(2 * 24 * 60 - 20), "They look great."),
    ];

    let dana = conversation_id(ACCOUNT, "dana4chat");
    let dana_messages = vec![
        text(&dana, 1, ME, "Anna Berg", ago(4 * 24 * 60), "Do we meet on Thursday?"),
        text(&dana, 2, "dana", "Dana Erb", ago(4 * 24 * 60 - 5), "Yes, ten o'clock."),
    ];

    let gesa = conversation_id(ACCOUNT, "gesa4chat");
    let gesa_messages = vec![text(&gesa, 1, "gesa", "Gesa Lange", ago(45), "Lunch at one?")];

    let mut conversations = vec![
        conversation(BEN, "Ben Koch", ConversationKind::Direct, &ben, 2),
        conversation(&gesa, "Gesa Lange", ConversationKind::Direct, &gesa_messages, 1),
        conversation(TEAM, "Team Alpha", ConversationKind::Group, &team, 0),
        conversation(&design, "Design sync", ConversationKind::Group, &design_messages, 0),
        conversation(&dana, "Dana Erb", ConversationKind::Direct, &dana_messages, 0),
    ];
    conversations[0].mentioned = true;
    conversations[0].mentioned_directly = true;
    conversations[0].person = Some(person_id(ACCOUNT, "ben"));
    conversations[1].person = Some(person_id(ACCOUNT, "gesa"));
    conversations[4].person = Some(person_id(ACCOUNT, "dana"));

    let messages = BTreeMap::from([
        (BEN.to_string(), ben),
        (TEAM.to_string(), team),
        (design, design_messages),
        (dana, dana_messages),
        (gesa, gesa_messages),
    ]);
    let own = OwnStatus {
        account: ACCOUNT.into(),
        name: "Anna Berg".into(),
        status: Status::Online,
        message: None,
        icon: None,
        clear_at: None,
        heartbeat: true,
    };
    Data { conversations, messages, people, own, next: 100 }
}

/// The messages of [`LENA`], the oldest first, the last at `now` less six minutes (the Talk entry of
/// the inbox's demo arrived then).
fn lena(now: DateTime<Utc>) -> Vec<Message> {
    let ago = |minutes: i64| now - Duration::minutes(minutes);
    let lines: [(&str, &str, i64); 24] = [
        ("lena", "Morning! Are the sidebar drafts still on for today?", 27 * 60),
        (ME, "Yes, I am finishing the second one.", 27 * 60 - 4),
        ("lena", "Great. Which widths did you try?", 27 * 60 - 6),
        (ME, "240 for the sidebar and 400 for the panel.", 27 * 60 - 9),
        ("lena", "And the rail when it is collapsed?", 27 * 60 - 11),
        (ME, "60, only icons with the names as tooltips.", 27 * 60 - 14),
        ("lena", "Sounds right. Do the groups keep their titles?", 27 * 60 - 20),
        (ME, "In the rail they become a line.", 27 * 60 - 22),
        ("lena", "Nice. I will look at it over lunch.", 27 * 60 - 25),
        ("lena", "Looked at it. The spacing between the groups feels a bit tight.", 24 * 60),
        (ME, "Agreed, I will try 24 instead of 16.", 24 * 60 - 7),
        ("lena", "And the counts could be lighter.", 24 * 60 - 9),
        (ME, "Text 3 then, like the rest of the metadata.", 24 * 60 - 12),
        ("lena", "Perfect.", 24 * 60 - 13),
        ("", "Lena Hoffmann started a call", 8 * 60),
        ("", "Call ended after 14 minutes", 8 * 60 - 14),
        ("lena", "Thanks for the call. I wrote the points into the ticket.", 8 * 60 - 16),
        (ME, "Got them. The new drafts are in the shared folder.", 3 * 60),
        ("lena", "Opening them now.", 3 * 60 - 5),
        ("lena", "The dark one works really well.", 2 * 60),
        (ME, "Good to hear! The light one still needs the hover colours.", 2 * 60 - 3),
        ("lena", "No hurry.", 2 * 60 - 4),
        ("lena", "One more thing about the rail: does the badge fit at 60?", 9),
        ("lena", "Can we go through the sidebar drafts before 3?", 6),
    ];
    let mut messages: Vec<Message> = lines
        .iter()
        .zip(1..)
        .map(|((user, line, minutes), number)| match *user {
            "" => system(LENA, number, ago(*minutes), MessageKind::System, line),
            user if user == ME => text(LENA, number, ME, "Anna Berg", ago(*minutes), line),
            user => text(LENA, number, user, "Lena Hoffmann", ago(*minutes), line),
        })
        .collect();
    if let Some(answer) = messages.get_mut(20) {
        answer.reply_to = Some(Quote {
            id: message_id(LENA, 20),
            author: "Lena Hoffmann".into(),
            text: "The dark one works really well.".into(),
        });
    }
    messages
}

impl ChatSource for DemoSource {
    async fn conversations(&self, account: Option<String>) -> Result<Vec<Conversation>, ChatError> {
        let data = self.lock();
        let mut list: Vec<Conversation> = data
            .conversations
            .iter()
            .filter(|conversation| account.as_deref().is_none_or(|account| conversation.account == account))
            .cloned()
            .collect();
        list.sort_by_key(|conversation| std::cmp::Reverse(conversation.last_activity));
        Ok(list)
    }

    async fn messages(
        &self,
        conversation: String,
        before: Option<String>,
        after: Option<String>,
        limit: Option<u32>,
    ) -> Result<MessagePage, ChatError> {
        let data = self.lock();
        let all = data.messages.get(&conversation).ok_or(ChatError::NotFound)?;
        let number = |id: &Option<String>| id.as_deref().and_then(split_message).map(|(_, number)| number);
        let limit = (limit.unwrap_or(50) as usize).clamp(1, PAGE_MAX);
        let (messages, more) = match (number(&before), number(&after)) {
            (_, Some(after)) => {
                (all.iter().filter(|message| message.number() > after).take(limit).cloned().collect::<Vec<_>>(), false)
            }
            (before, None) => {
                let older: Vec<&Message> =
                    all.iter().filter(|message| before.is_none_or(|before| message.number() < before)).collect();
                let from = older.len().saturating_sub(limit);
                (older[from..].iter().map(|message| (*message).clone()).collect(), from > 0)
            }
        };
        let last_read = data.conversations.iter().find(|c| c.id == conversation).and_then(|c| c.last_read.clone());
        Ok(MessagePage { messages, last_read, more, offline: false })
    }

    async fn send(&self, message: Outgoing) -> Result<String, ChatError> {
        let mut data = self.lock();
        data.next += 1;
        let id = message_id(&message.conversation, data.next);
        let name = data.own.name.clone();
        let list = data.messages.get_mut(&message.conversation).ok_or(ChatError::NotFound)?;
        let reply_to = message.reply_to.as_deref().and_then(|quoted| {
            list.iter().find(|other| other.id == quoted).map(|other| Quote {
                id: other.id.clone(),
                author: other.author.name.clone(),
                text: other.text.clone(),
            })
        });
        let mut sent = text(&message.conversation, 0, ME, &name, Utc::now(), &message.text);
        sent.id = id.clone();
        sent.reply_to = reply_to;
        let preview = Preview { author: name, text: sent.text.clone(), system: false };
        let at = sent.at;
        list.push(sent);
        if let Some(conversation) = data.conversations.iter_mut().find(|c| c.id == message.conversation) {
            conversation.last_message = Some(preview);
            conversation.last_activity = at;
            conversation.last_read = Some(id.clone());
            conversation.unread = 0;
        }
        Ok(id)
    }

    async fn mark_read(&self, conversation: String, up_to: Option<String>) -> Result<(), ChatError> {
        let mut data = self.lock();
        let newest = data.messages.get(&conversation).and_then(|list| list.last()).map(|message| message.id.clone());
        let entry = data.conversations.iter_mut().find(|c| c.id == conversation).ok_or(ChatError::NotFound)?;
        entry.last_read = up_to.or(newest);
        entry.unread = 0;
        entry.mentioned = false;
        entry.mentioned_directly = false;
        Ok(())
    }

    async fn mark_unread(&self, conversation: String) -> Result<(), ChatError> {
        let mut data = self.lock();
        let before_last = data
            .messages
            .get(&conversation)
            .and_then(|list| list.len().checked_sub(2).and_then(|at| list.get(at)))
            .map(|message| message.id.clone());
        let entry = data.conversations.iter_mut().find(|c| c.id == conversation).ok_or(ChatError::NotFound)?;
        entry.unread = 1;
        entry.last_read = before_last;
        Ok(())
    }

    async fn suggest_mentions(&self, conversation: String, query: String) -> Result<Vec<Mention>, ChatError> {
        let data = self.lock();
        if !data.messages.contains_key(&conversation) {
            return Err(ChatError::NotFound);
        }
        let query = query.to_lowercase();
        Ok(data
            .people
            .iter()
            .filter(|person| person.name.to_lowercase().contains(&query))
            .filter_map(|person| {
                let (_, user) = split_person(&person.id)?;
                Some(Mention { id: user.to_string(), name: person.name.clone() })
            })
            .collect())
    }

    async fn create_conversation(&self, account: String, with: String) -> Result<Conversation, ChatError> {
        let mut data = self.lock();
        if account != ACCOUNT {
            return Err(ChatError::NotFound);
        }
        if let Some(existing) = data.conversations.iter().find(|c| c.person.as_deref() == Some(with.as_str())) {
            return Ok(existing.clone());
        }
        let person = data.people.iter().find(|person| person.id == with).ok_or(ChatError::NotFound)?.clone();
        let (_, user) = split_person(&person.id).ok_or(ChatError::NotFound)?;
        let id = conversation_id(ACCOUNT, &format!("{user}4chat"));
        let mut created = conversation(&id, &person.name, ConversationKind::Direct, &[], 0);
        created.person = Some(person.id.clone());
        created.last_activity = Utc::now();
        data.messages.insert(id, Vec::new());
        data.conversations.push(created.clone());
        Ok(created)
    }

    async fn people(&self, account: Option<String>) -> Result<Vec<Person>, ChatError> {
        let data = self.lock();
        Ok(data
            .people
            .iter()
            .filter(|person| account.as_deref().is_none_or(|account| person.account == account))
            .cloned()
            .collect())
    }

    async fn find_people(&self, account: String, query: String) -> Result<Vec<Person>, ChatError> {
        if account != ACCOUNT {
            return Err(ChatError::NotFound);
        }
        if query.chars().count() < 2 {
            return Err(ChatError::Other("A search needs at least two characters.".into()));
        }
        let known = self.lock().people.clone();
        let query = query.to_lowercase();
        let mut found: Vec<Person> =
            known.into_iter().filter(|person| person.name.to_lowercase().contains(&query)).collect();
        // Someone the list does not show yet, so "Add to list" has something to add.
        if "hanna vogt".contains(&query) && !found.iter().any(|person| person.id.ends_with("/hanna")) {
            found.push(person("hanna", "Hanna Vogt", Status::Offline, None));
        }
        Ok(found)
    }

    async fn add_person(&self, account: String, person_id: String) -> Result<(), ChatError> {
        let mut data = self.lock();
        if account != ACCOUNT {
            return Err(ChatError::NotFound);
        }
        if data.people.iter().any(|person| person.id == person_id) {
            return Ok(());
        }
        let (_, user) = split_person(&person_id).ok_or(ChatError::NotFound)?;
        let name = if user == "hanna" { "Hanna Vogt".to_string() } else { user.to_string() };
        let mut added = person(user, &name, Status::Offline, None);
        added.added = true;
        data.people.push(added);
        Ok(())
    }

    async fn remove_person(&self, account: String, person_id: String) -> Result<(), ChatError> {
        let mut data = self.lock();
        if account != ACCOUNT {
            return Err(ChatError::NotFound);
        }
        data.people.retain(|person| !(person.id == person_id && person.added));
        Ok(())
    }

    async fn own_status(&self, account: String) -> Result<OwnStatus, ChatError> {
        if account != ACCOUNT {
            return Err(ChatError::NotFound);
        }
        Ok(self.lock().own.clone())
    }

    async fn set_own_status(&self, account: String, change: StatusChange) -> Result<(), ChatError> {
        let mut data = self.lock();
        if account != ACCOUNT {
            return Err(ChatError::NotFound);
        }
        if let Some(status) = change.status {
            data.own.status = status;
        }
        if let Some(message) = change.message {
            data.own.message = (!message.trim().is_empty()).then_some(message);
            data.own.icon = change.icon;
            data.own.clear_at = change.clear_at;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> DemoSource {
        DemoSource::new(DateTime::from_timestamp(1_800_000_000, 0).expect("a time"))
    }

    #[tokio::test]
    async fn the_sample_has_what_the_concept_asks_for() {
        let demo = demo();
        let people = demo.people(None).await.expect("people");
        assert_eq!(people.len(), 6);
        for status in [Status::Online, Status::Away, Status::Busy, Status::Dnd, Status::Offline] {
            assert!(people.iter().any(|person| person.status == status), "{status:?}");
        }
        let conversations = demo.conversations(None).await.expect("conversations");
        assert_eq!(conversations.iter().filter(|c| c.kind == ConversationKind::Group).count(), 2);
        assert_eq!(conversations.iter().filter(|c| c.kind == ConversationKind::Direct).count(), 3);
        assert_eq!(conversations[0].id, BEN, "the most recent first");
        let ben = demo.messages(BEN.into(), None, None, None).await.expect("messages").messages;
        assert!(ben.iter().any(|message| !message.mentions.is_empty()), "a mention");
        assert!(ben.iter().any(|message| !message.files.is_empty()), "a file");
        assert!(ben.iter().any(|message| message.kind == MessageKind::MissedCall), "a missed call");
        let team = demo.messages(TEAM.into(), None, None, None).await.expect("messages").messages;
        assert!(team.iter().any(|message| message.kind == MessageKind::System), "a system message");
    }

    #[tokio::test]
    async fn pages_go_back_and_forward() {
        let demo = demo();
        let last_two = demo.messages(BEN.into(), None, None, Some(2)).await.expect("a page");
        assert_eq!(last_two.messages.iter().map(Message::number).collect::<Vec<_>>(), [4, 5]);
        assert!(last_two.more);
        assert_eq!(last_two.last_read, Some(message_id(BEN, 3)), "two of five are unread");
        let older = demo.messages(BEN.into(), Some(message_id(BEN, 4)), None, Some(10)).await.expect("a page");
        assert_eq!(older.messages.iter().map(Message::number).collect::<Vec<_>>(), [1, 2, 3]);
        assert!(!older.more);
        let newer = demo.messages(BEN.into(), None, Some(message_id(BEN, 4)), None).await.expect("a page");
        assert_eq!(newer.messages.iter().map(Message::number).collect::<Vec<_>>(), [5]);
        assert_eq!(demo.messages("nope/x".into(), None, None, None).await, Err(ChatError::NotFound));
    }

    #[tokio::test]
    async fn sending_adds_the_message_and_reads_the_conversation() {
        let demo = demo();
        let outgoing = Outgoing {
            conversation: BEN.into(),
            text: "On it.".into(),
            reply_to: Some(message_id(BEN, 5)),
            silent: false,
        };
        let id = demo.send(outgoing).await.expect("sent");
        let page = demo.messages(BEN.into(), None, None, None).await.expect("messages");
        let sent = page.messages.last().expect("the sent message");
        assert_eq!(sent.id, id);
        assert!(sent.author.own);
        assert_eq!(sent.reply_to.as_ref().map(|quote| quote.author.as_str()), Some("Ben Koch"));
        let ben = demo.conversations(None).await.expect("conversations").into_iter().find(|c| c.id == BEN);
        assert_eq!(ben.map(|c| c.unread), Some(0));
    }

    #[tokio::test]
    async fn read_and_unread_move_the_marker() {
        let demo = demo();
        demo.mark_read(BEN.into(), None).await.expect("read");
        let find = |list: Vec<Conversation>| list.into_iter().find(|c| c.id == BEN).expect("Ben");
        let ben = find(demo.conversations(None).await.expect("conversations"));
        assert_eq!((ben.unread, ben.mentioned, ben.last_read), (0, false, Some(message_id(BEN, 5))));
        demo.mark_unread(BEN.into()).await.expect("unread");
        let ben = find(demo.conversations(None).await.expect("conversations"));
        assert_eq!((ben.unread, ben.last_read), (1, Some(message_id(BEN, 4))));
    }

    #[tokio::test]
    async fn people_are_found_added_and_removed() {
        let demo = demo();
        assert!(demo.find_people(ACCOUNT.into(), "h".into()).await.is_err(), "two characters at least");
        let found = demo.find_people(ACCOUNT.into(), "hanna".into()).await.expect("found");
        let hanna = found.first().expect("Hanna").id.clone();
        demo.add_person(ACCOUNT.into(), hanna.clone()).await.expect("added");
        assert_eq!(demo.people(None).await.expect("people").len(), 7);
        // Only people the person added themselves can be removed.
        demo.remove_person(ACCOUNT.into(), person_id(ACCOUNT, "ben")).await.expect("ok");
        demo.remove_person(ACCOUNT.into(), hanna).await.expect("removed");
        assert_eq!(demo.people(None).await.expect("people").len(), 6);
    }

    #[tokio::test]
    async fn a_direct_chat_is_created_once() {
        let demo = demo();
        let ben = demo.create_conversation(ACCOUNT.into(), person_id(ACCOUNT, "ben")).await.expect("Ben");
        assert_eq!(ben.id, BEN, "the existing one");
        let cem = demo.create_conversation(ACCOUNT.into(), person_id(ACCOUNT, "cem")).await.expect("Cem");
        assert_eq!(cem.kind, ConversationKind::Direct);
        let again = demo.create_conversation(ACCOUNT.into(), person_id(ACCOUNT, "cem")).await.expect("Cem");
        assert_eq!(again.id, cem.id);
    }

    #[tokio::test]
    async fn the_hub_sample_adds_lena_with_two_unread() {
        let plain = demo().conversations(None).await.expect("conversations");
        assert!(!plain.iter().any(|c| c.id == LENA), "not part of the plain sample");
        let hub = DemoSource::for_hub(DateTime::from_timestamp(1_800_000_000, 0).expect("a time"));
        let page = hub.messages(LENA.into(), None, None, Some(20)).await.expect("messages");
        assert_eq!(page.messages.len(), 20);
        assert!(page.more, "older messages wait behind the first page");
        assert_eq!(page.last_read, Some(message_id(LENA, 22)), "the last two are unread");
        let all = hub.messages(LENA.into(), None, None, Some(100)).await.expect("messages").messages;
        assert_eq!(all.len(), 24);
        assert!(all.iter().any(|message| message.kind == MessageKind::System));
        assert!(all.iter().any(|message| message.author.own));
        assert!(all.iter().any(|message| message.reply_to.is_some()));
        assert_eq!(hub.people(None).await.expect("people").len(), 6, "the people stay as they are");
    }

    #[tokio::test]
    async fn the_own_status_changes_part_by_part() {
        let demo = demo();
        let change = StatusChange { status: Some(Status::Dnd), ..StatusChange::default() };
        demo.set_own_status(ACCOUNT.into(), change).await.expect("set");
        let change = StatusChange { message: Some("In a meeting".into()), ..StatusChange::default() };
        demo.set_own_status(ACCOUNT.into(), change).await.expect("set");
        let own = demo.own_status(ACCOUNT.into()).await.expect("own");
        assert_eq!((own.status, own.message.as_deref()), (Status::Dnd, Some("In a meeting")));
        let change = StatusChange { message: Some(String::new()), ..StatusChange::default() };
        demo.set_own_status(ACCOUNT.into(), change).await.expect("set");
        assert_eq!(demo.own_status(ACCOUNT.into()).await.expect("own").message, None);
    }
}
