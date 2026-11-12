//! What the contact list shows, worked out apart from the widgets so it can be tested: the people in
//! their groups by status ([`groups`]), the group chats ([`group_chats`]), what a search keeps
//! ([`matches`]) and the words of a row ([`status_word`], [`person_line`], [`chat_line`]).
//!
//! The order follows `docs/chat.md` ("Fenster", Kontaktliste): "Online", "Away", "Busy", "Do not
//! disturb", "Offline", each by name. Someone the server did not tell the status of stands in
//! "Offline" with "Status unknown"; "Invisible" is only ever the person's own status, others see
//! "Offline". Stage 7: people whose direct chat is a favourite stand above all in "Favorites" (and
//! only there), favourite group chats come first in "Groups".

use torro_chat::{Conversation, ConversationKind, Person, Status};
use torro_core::i18n::{tr, trf};

/// "Offline" starts folded once more than this many people stand in it.
pub const FOLD_OFFLINE_FROM: usize = 20;

/// A group of the people tab, in the order the list shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    /// People whose direct chat is a favourite, whatever their status.
    Favorites,
    Online,
    Away,
    Busy,
    Dnd,
    Offline,
}

impl Group {
    pub const ALL: [Group; 6] = [Group::Favorites, Group::Online, Group::Away, Group::Busy, Group::Dnd, Group::Offline];

    /// The group's title.
    pub fn title(self) -> &'static str {
        match self {
            Group::Favorites => tr("Favorites"),
            Group::Online => tr("Online"),
            Group::Away => tr("Away"),
            Group::Busy => tr("Busy"),
            Group::Dnd => tr("Do not disturb"),
            Group::Offline => tr("Offline"),
        }
    }

    /// The group of `person`'s status; never [`Group::Favorites`], which [`groups`] decides by the
    /// conversations.
    pub fn of(person: &Person) -> Group {
        if person.unknown {
            return Group::Offline;
        }
        match person.status {
            Status::Online => Group::Online,
            Status::Away => Group::Away,
            Status::Busy => Group::Busy,
            Status::Dnd => Group::Dnd,
            Status::Invisible | Status::Offline => Group::Offline,
        }
    }

    /// The status the group's mark shows. "Favorites" mixes statuses and has none of its own; it is
    /// never asked, because [`Group::of`] never gives it.
    pub fn status(self) -> Status {
        match self {
            Group::Favorites => Status::Offline,
            Group::Online => Status::Online,
            Group::Away => Status::Away,
            Group::Busy => Status::Busy,
            Group::Dnd => Status::Dnd,
            Group::Offline => Status::Offline,
        }
    }
}

/// The word of a status ("Online", "Do not disturb").
pub fn status_word(status: Status) -> &'static str {
    match status {
        Status::Online => tr("Online"),
        Status::Away => tr("Away"),
        Status::Busy => tr("Busy"),
        Status::Dnd => tr("Do not disturb"),
        Status::Invisible => tr("Invisible"),
        Status::Offline => tr("Offline"),
    }
}

/// The people in their groups, each group by name (ignoring case); groups without anyone are left
/// out. People whose direct chat in `conversations` is a favourite stand in "Favorites" and not in
/// their status's group, so each person is one row. `query` keeps only people whose name or status
/// message holds it ([`matches`]).
pub fn groups<'a>(people: &'a [Person], conversations: &[Conversation], query: &str) -> Vec<(Group, Vec<&'a Person>)> {
    let place = |person: &Person| {
        if is_favorite(conversations, person) { Group::Favorites } else { Group::of(person) }
    };
    Group::ALL
        .into_iter()
        .filter_map(|group| {
            let mut members: Vec<&Person> = people
                .iter()
                .filter(|person| place(person) == group)
                .filter(|person| matches(query, &person.name, person.message.as_deref()))
                .collect();
            members.sort_by_cached_key(|person| person.name.to_lowercase());
            (!members.is_empty()).then_some((group, members))
        })
        .collect()
}

/// Whether `query` (ignoring case and the spaces around it) is in `name` or `line`; an empty query
/// keeps everything.
pub fn matches(query: &str, name: &str, line: Option<&str>) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || name.to_lowercase().contains(&query)
        || line.is_some_and(|line| line.to_lowercase().contains(&query))
}

/// The conversations of the tab "Groups": every one that is no direct chat, the favourites first,
/// each part the most recent first.
pub fn group_chats<'a>(conversations: &'a [Conversation], query: &str) -> Vec<&'a Conversation> {
    let mut chats: Vec<&Conversation> = conversations
        .iter()
        .filter(|c| matches!(c.kind, ConversationKind::Group | ConversationKind::Public | ConversationKind::NoteToSelf))
        .filter(|c| matches(query, &c.name, None))
        .collect();
    chats.sort_by_key(|c| (!c.favorite, std::cmp::Reverse(c.last_activity)));
    chats
}

/// Whether `person`'s direct chat is a favourite.
pub fn is_favorite(conversations: &[Conversation], person: &Person) -> bool {
    direct_chat(conversations, person).is_some_and(|c| c.favorite)
}

/// How many group chats have unread messages: the number on the tab "Groups".
pub fn unread_groups(conversations: &[Conversation]) -> usize {
    group_chats(conversations, "").into_iter().filter(|c| c.unread > 0).count()
}

/// The direct chat with `person`, when there is one.
pub fn direct_chat<'a>(conversations: &'a [Conversation], person: &Person) -> Option<&'a Conversation> {
    conversations.iter().find(|c| c.kind == ConversationKind::Direct && c.person.as_deref() == Some(&person.id))
}

/// The line under a person's name: the status message, "Status unknown" when the server did not
/// tell, else the status's word.
pub fn person_line(person: &Person) -> String {
    if person.unknown {
        return tr("Status unknown").to_owned();
    }
    match person.message.as_deref().map(str::trim).filter(|message| !message.is_empty()) {
        Some(message) => match person.icon.as_deref() {
            Some(icon) if !icon.trim().is_empty() => format!("{icon} {message}"),
            _ => message.to_owned(),
        },
        None => status_word(Group::of(person).status()).to_owned(),
    }
}

/// The line under a group chat's name: its last message as "Author: text", a message of the
/// provider as it is, "No messages yet" without one.
pub fn chat_line(conversation: &Conversation) -> String {
    match &conversation.last_message {
        Some(last) if last.system || last.author.is_empty() => last.text.clone(),
        Some(last) => trf("{author}: {text}", &[("author", &first_name(&last.author)), ("text", &last.text)]),
        None => tr("No messages yet").to_owned(),
    }
}

/// The first word of a name ("Ben" of "Ben Koch"), the whole name when it is one word.
pub fn first_name(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_owned()
}

/// The initials of a name for its avatar: the first letters of the first and the last word, in
/// capitals ("BK" of "Ben Koch", "T" of "Team"); "?" for a name without letters.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().filter(|word| word.chars().any(char::is_alphanumeric)).collect();
    let first = |word: &&str| word.chars().find(|c| c.is_alphanumeric());
    let letters: String = match words.as_slice() {
        [] => return "?".to_owned(),
        [one] => first(one).into_iter().collect(),
        [one, .., last] => first(one).into_iter().chain(first(last)).collect(),
    };
    letters.to_uppercase()
}

/// How many people are there now (online, away, busy or do not disturb); people whose status is
/// unknown never count. The figure of the mini tile.
pub fn there(people: &[Person]) -> usize {
    people.iter().filter(|person| !person.unknown && person.status.is_there()).count()
}

/// How many conversations have unread messages.
pub fn unread_conversations(conversations: &[Conversation]) -> usize {
    conversations.iter().filter(|c| c.unread > 0).count()
}

/// The accounts the people and conversations belong to, each once, in the order first seen: the own
/// status is asked of the first.
pub fn accounts(people: &[Person], conversations: &[Conversation]) -> Vec<String> {
    let mut accounts: Vec<String> = Vec::new();
    let seen = people.iter().map(|p| &p.account).chain(conversations.iter().map(|c| &c.account));
    for account in seen {
        if !accounts.contains(account) {
            accounts.push(account.clone());
        }
    }
    accounts
}

#[cfg(test)]
mod tests {
    use iced::futures::executor::block_on;
    use torro_chat::ChatSource;
    use torro_chat::demo::DemoSource;

    use super::*;

    fn sample() -> (Vec<Person>, Vec<Conversation>) {
        let demo = DemoSource::new(chrono::DateTime::from_timestamp(1_800_000_000, 0).expect("a time"));
        (block_on(demo.people(None)).expect("people"), block_on(demo.conversations(None)).expect("conversations"))
    }

    #[test]
    fn people_stand_in_groups_by_status_in_the_concepts_order() {
        let (people, conversations) = sample();
        let groups = groups(&people, &conversations, "");
        let order: Vec<Group> = groups.iter().map(|(group, _)| *group).collect();
        assert_eq!(order, Group::ALL, "the demo has someone in every group");
        let names = |at: usize| groups[at].1.iter().map(|person| person.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names(0), ["Gesa Lange"], "her chat is a favourite");
        assert_eq!(names(1), ["Ben Koch"], "online, without Gesa: one row each");
        assert_eq!(groups[1].0.title(), tr("Online"));
        assert_eq!(Group::Favorites.title(), tr("Favorites"));
        assert_eq!(Group::Dnd.title(), tr("Do not disturb"));
        let plain = super::groups(&people, &[], "");
        assert_eq!(plain[0].0, Group::Online, "no favourites without conversations");
        let online: Vec<&str> = plain[0].1.iter().map(|person| person.name.as_str()).collect();
        assert_eq!(online, ["Ben Koch", "Gesa Lange"], "by name");
    }

    #[test]
    fn unknown_and_invisible_people_stand_in_offline() {
        let (mut people, _) = sample();
        people[0].unknown = true;
        people[1].status = Status::Invisible;
        assert_eq!(Group::of(&people[0]), Group::Offline);
        assert_eq!(Group::of(&people[1]), Group::Offline);
        assert_eq!(person_line(&people[0]), tr("Status unknown"));
        assert_eq!(there(&people), 3, "Dana, Elif and Gesa are there; Ben's status is unknown");
    }

    #[test]
    fn a_search_keeps_names_and_status_messages() {
        let (people, conversations) = sample();
        let found: Vec<&str> = groups(&people, &conversations, "MEETING")
            .iter()
            .flat_map(|(_, p)| p.iter().map(|p| p.name.as_str()))
            .collect();
        assert_eq!(found, ["Dana Erb"], "her status message says it");
        assert!(groups(&people, &conversations, "nobody here").is_empty());
        let chats: Vec<&str> = group_chats(&conversations, " team ").iter().map(|c| c.name.as_str()).collect();
        assert_eq!(chats, ["Team Alpha"]);
    }

    #[test]
    fn the_groups_tab_lists_conversations_that_are_no_direct_chat() {
        let (people, conversations) = sample();
        let chats: Vec<&str> = group_chats(&conversations, "").iter().map(|c| c.name.as_str()).collect();
        assert_eq!(chats, ["Team Alpha", "Design sync"], "the favourite first");
        let mut swapped = conversations.clone();
        for conversation in &mut swapped {
            conversation.favorite = conversation.name == "Design sync";
        }
        let chats: Vec<&str> = group_chats(&swapped, "").iter().map(|c| c.name.as_str()).collect();
        assert_eq!(chats, ["Design sync", "Team Alpha"], "a favourite goes before a more recent chat");
        assert_eq!(unread_groups(&conversations), 0);
        let ben = people.iter().find(|person| person.name == "Ben Koch").expect("Ben");
        assert_eq!(direct_chat(&conversations, ben).map(|c| c.unread), Some(2));
        let cem = people.iter().find(|person| person.name == "Cem Demir").expect("Cem");
        assert!(direct_chat(&conversations, cem).is_none(), "no chat with Cem yet");
        assert_eq!(unread_conversations(&conversations), 2);
    }

    #[test]
    fn rows_say_what_is_new_in_words() {
        let (people, conversations) = sample();
        let dana = people.iter().find(|person| person.name == "Dana Erb").expect("Dana");
        assert_eq!(person_line(dana), "In a meeting");
        let cem = people.iter().find(|person| person.name == "Cem Demir").expect("Cem");
        assert_eq!(person_line(cem), tr("Away"), "without a message the status");
        let team = conversations.iter().find(|c| c.name == "Team Alpha").expect("team");
        assert_eq!(
            chat_line(team),
            trf("{author}: {text}", &[("author", &"Anna"), ("text", &"I can review tomorrow morning.")])
        );
    }

    #[test]
    fn initials_take_the_first_and_the_last_word() {
        assert_eq!(initials("Ben Koch"), "BK");
        assert_eq!(initials("Anna Maria Berg"), "AB");
        assert_eq!(initials("team"), "T");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("élif aydın"), "ÉA");
    }

    #[test]
    fn the_accounts_come_from_people_and_conversations() {
        let (people, conversations) = sample();
        assert_eq!(accounts(&people, &conversations), [torro_chat::demo::ACCOUNT]);
        assert!(accounts(&[], &[]).is_empty());
    }
}
