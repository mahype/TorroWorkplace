//! Recipient suggestions (`contacts.suggest_recipients`, issue #460): what the list under a recipient
//! field of the compose window offers for the letters typed so far.
//!
//! [`rank`] is the whole rule, without input or output: the core hands it the contacts and groups of
//! the books the caller reaches and the people of the mail accounts it may read ([`Correspondent`]),
//! the compose window's demo hands it the sample. Two sections, in this order:
//!
//! 1. **Contacts:** the addresses written to lately and often first (by their [`Correspondent::score`],
//!    then the last mail), then favourites, then groups (a group stands for all its members with an
//!    address), then everyone else, each of the last three by name. Every address once.
//! 2. **Seen:** addresses known only from mail, in no address book, the latest first. While there are
//!    some, the contacts leave them room for two ([`SEEN_ROOM`]).
//!
//! A suggestion matches when every word typed is the start of a word of its name, of its address, or
//! of a part of its address between `@ . - _ +`, compared [`fold`]ed: "jo be" finds "Jonas Berger" and
//! "jonas@berger-arch.de", "arch" finds the latter, "onas" nothing. An empty search suggests nothing.

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};

use crate::model::{Contact, RecipientSuggestion, SuggestedAddress, SuggestionKind};

/// How many suggestions come when the caller does not say.
pub const DEFAULT_LIMIT: u32 = 8;
/// The most suggestions one answer holds.
pub const MAX_LIMIT: u32 = 20;
/// The longest search a caller may send; longer ones are refused (`error`).
pub const MAX_QUERY_CHARS: usize = 500;
/// How many of the people of the mail that match a search the caller looks up in the address books
/// ([`rank`] needs their contacts among the candidates): the ones written to most.
pub const KNOWN_ADDRESSES: usize = 50;
/// While addresses known only from mail match, the contacts leave room for this many of them.
pub const SEEN_ROOM: usize = 2;

/// A person of the mail of one or more accounts: an address written to (in the sent folder) or one a
/// personal mail came from, with how often and how lately mail went there.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Correspondent {
    /// As the mail had it; compared without case.
    pub address: String,
    /// The name the mail gave, if any.
    pub name: Option<String>,
    /// How much mail went there lately: every mail sent counts by its age ([`sent_weight`]).
    pub score: f64,
    pub last_sent: Option<DateTime<Utc>>,
    pub last_received: Option<DateTime<Utc>>,
}

/// A group of an address book whose name may match, with its members.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GroupCandidate {
    pub name: String,
    pub members: Vec<Contact>,
}

/// What one mail sent `age_days` ago adds to its recipients' score: 4 within a week, 2 within a
/// month, 1 within three months, ½ within a year, nothing beyond. Newer mail counts more, so someone
/// written to daily last year does not stand above someone written to twice this week.
pub fn sent_weight(age_days: i64) -> f64 {
    match age_days {
        ..=7 => 4.0,
        8..=30 => 2.0,
        31..=90 => 1.0,
        91..=365 => 0.5,
        _ => 0.0,
    }
}

/// The people of several accounts as one list: the same address (without case) once, the scores
/// added, the latest dates and the first name given kept.
pub fn merge(people: impl IntoIterator<Item = Correspondent>) -> Vec<Correspondent> {
    let mut merged: BTreeMap<String, Correspondent> = BTreeMap::new();
    for person in people {
        let key = person.address.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        match merged.get_mut(&key) {
            None => {
                merged.insert(key, person);
            }
            Some(known) => {
                known.score += person.score;
                known.last_sent = known.last_sent.max(person.last_sent);
                known.last_received = known.last_received.max(person.last_received);
                if known.name.as_deref().is_none_or(|name| name.trim().is_empty()) {
                    known.name = person.name;
                }
            }
        }
    }
    merged.into_values().collect()
}

/// `text` as suggestions compare it: lower case, accents and umlaut dots taken off ("ß" becomes "ss").
/// Also what the contacts store folds its search with, so both find the same.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => out.push('a'),
            'æ' => out.push_str("ae"),
            'ç' | 'ć' | 'č' => out.push('c'),
            'ď' | 'đ' => out.push('d'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => out.push('e'),
            'ğ' => out.push('g'),
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => out.push('i'),
            'ł' | 'ľ' | 'ĺ' => out.push('l'),
            'ñ' | 'ń' | 'ň' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => out.push('o'),
            'œ' => out.push_str("oe"),
            'ŕ' | 'ř' => out.push('r'),
            'ß' => out.push_str("ss"),
            'ś' | 'š' | 'ş' | 'ș' => out.push('s'),
            'ť' | 'ţ' | 'ț' => out.push('t'),
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'ź' | 'ż' | 'ž' => out.push('z'),
            // Combining marks, as in a decomposed "u\u{308}" or the lower case of "İ".
            '\u{300}'..='\u{36f}' => {}
            _ => out.push(c),
        }
    }
    out
}

/// The words of a search, folded; empty for a blank one.
pub fn search_words(query: &str) -> Vec<String> {
    fold(query).split_whitespace().map(str::to_string).collect()
}

/// Whether every word of `words` starts a word of `name`, the whole `address` or a part of it
/// between `@ . - _ +`.
pub fn matches(words: &[String], name: &str, address: &str) -> bool {
    if words.is_empty() {
        return false;
    }
    let name = fold(name);
    let address = fold(address.trim());
    let parts: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '"' || c == '(')
        .chain(std::iter::once(address.as_str()))
        .chain(address.split(['@', '.', '-', '_', '+']))
        .filter(|part| !part.is_empty())
        .collect();
    words.iter().all(|word| parts.iter().any(|part| part.starts_with(word.as_str())))
}

/// The suggestions for `query` (see the module documentation), at most `limit` ([`DEFAULT_LIMIT`]
/// without one, held within 1 and [`MAX_LIMIT`]). `contacts` and `groups` are those the caller
/// reaches, `history` the people of the mail it may read ([`merge`]d or not).
pub fn rank(
    query: &str,
    contacts: &[Contact],
    groups: &[GroupCandidate],
    history: &[Correspondent],
    limit: Option<u32>,
) -> Vec<RecipientSuggestion> {
    let words = search_words(query);
    if words.is_empty() {
        return Vec::new();
    }
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;
    let history: Vec<Correspondent> = merge(history.iter().cloned());
    let known = |address: &str| {
        let key = address.trim().to_lowercase();
        history.iter().find(|person| person.address.trim().to_lowercase() == key)
    };
    // Every address an address book holds, matching or not: those never count as "seen".
    let mut in_books: HashSet<String> = HashSet::new();
    for contact in contacts.iter().chain(groups.iter().flat_map(|group| &group.members)) {
        in_books.extend(contact.emails.iter().map(|email| email.value.trim().to_lowercase()));
    }

    let mut taken: HashSet<String> = HashSet::new();
    let (mut written, mut favourites, mut others) = (Vec::new(), Vec::new(), Vec::new());
    for contact in contacts {
        let name = contact.shown_name();
        for email in &contact.emails {
            let address = email.value.trim();
            if address.is_empty() || !matches(&words, &name, address) || !taken.insert(address.to_lowercase()) {
                continue;
            }
            let person = known(address);
            let suggestion = RecipientSuggestion {
                kind: SuggestionKind::Contact,
                name: name.clone(),
                address: address.to_string(),
                contact: Some(contact.id.clone()),
                photo: contact.photo.clone(),
                favorite: contact.favorite,
                members: Vec::new(),
                last_sent: person.and_then(|p| p.last_sent),
                last_seen: person.and_then(|p| p.last_received),
                untrusted: true,
            };
            match person.map(|p| p.score).filter(|score| *score > 0.0) {
                Some(score) => written.push((score, suggestion)),
                None if contact.favorite => favourites.push(suggestion),
                None => others.push(suggestion),
            }
        }
    }
    written.sort_by(|(a, x), (b, y)| b.total_cmp(a).then(y.last_sent.cmp(&x.last_sent)).then(by_name(x, y)));
    favourites.sort_by(by_name);
    others.sort_by(by_name);

    let mut group_lines: Vec<RecipientSuggestion> = Vec::new();
    let mut group_names: HashSet<String> = HashSet::new();
    for group in groups {
        if !matches(&words, &group.name, "") || !group_names.insert(fold(group.name.trim())) {
            continue;
        }
        let mut members: Vec<SuggestedAddress> = Vec::new();
        for member in &group.members {
            let Some(email) = member.emails.iter().find(|e| e.pref).or_else(|| member.emails.first()) else {
                continue;
            };
            let address = email.value.trim();
            if address.is_empty() || members.iter().any(|known| known.address.eq_ignore_ascii_case(address)) {
                continue;
            }
            members.push(SuggestedAddress { name: member.shown_name(), address: address.to_string() });
        }
        if members.is_empty() {
            continue;
        }
        group_lines.push(RecipientSuggestion {
            kind: SuggestionKind::Group,
            name: group.name.trim().to_string(),
            members,
            untrusted: true,
            ..RecipientSuggestion::default()
        });
    }
    group_lines.sort_by(by_name);

    let mut seen: Vec<RecipientSuggestion> = history
        .iter()
        .filter(|person| {
            let key = person.address.trim().to_lowercase();
            !in_books.contains(&key)
                && !taken.contains(&key)
                && matches(&words, person.name.as_deref().unwrap_or_default(), &person.address)
        })
        .map(|person| RecipientSuggestion {
            kind: SuggestionKind::Seen,
            name: person.name.clone().unwrap_or_default().trim().to_string(),
            address: person.address.trim().to_string(),
            last_sent: person.last_sent,
            last_seen: person.last_received,
            untrusted: true,
            ..RecipientSuggestion::default()
        })
        .collect();
    seen.sort_by(|a, b| latest(b).cmp(&latest(a)).then(a.address.cmp(&b.address)));

    let room = if seen.is_empty() { limit } else { limit.saturating_sub(SEEN_ROOM.min(seen.len())) };
    let mut out: Vec<RecipientSuggestion> = written
        .into_iter()
        .map(|(_, suggestion)| suggestion)
        .chain(favourites)
        .chain(group_lines)
        .chain(others)
        .take(room)
        .collect();
    let left = limit - out.len();
    out.extend(seen.into_iter().take(left));
    out
}

/// `suggestions` without what they know from mail (issue #460), for a reader that may not read it
/// (a model elsewhere under the data rule, a conversation with mail switched off): no address known
/// only from mail, no dates of mail, and the contacts in the order they have without mail –
/// favourites, groups, the rest, each by name.
pub fn without_mail(suggestions: &[RecipientSuggestion]) -> Vec<RecipientSuggestion> {
    let mut kept: Vec<RecipientSuggestion> = suggestions
        .iter()
        .filter(|s| s.kind != SuggestionKind::Seen)
        .map(|s| RecipientSuggestion { last_sent: None, last_seen: None, ..s.clone() })
        .collect();
    let rank = |s: &RecipientSuggestion| match (s.kind, s.favorite) {
        (SuggestionKind::Contact, true) => 0,
        (SuggestionKind::Group, _) => 1,
        _ => 2,
    };
    kept.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| by_name(a, b)));
    kept
}

/// By name as lists sort (folded), then by address.
fn by_name(a: &RecipientSuggestion, b: &RecipientSuggestion) -> std::cmp::Ordering {
    fold(&a.name).cmp(&fold(&b.name)).then_with(|| a.address.cmp(&b.address))
}

/// The latest mail either way.
fn latest(suggestion: &RecipientSuggestion) -> Option<DateTime<Utc>> {
    suggestion.last_sent.max(suggestion.last_seen)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::model::{FieldKind, Labeled};

    fn contact(id: &str, name: &str, addresses: &[&str], favorite: bool) -> Contact {
        Contact {
            id: id.into(),
            display_name: name.into(),
            emails: addresses.iter().map(|a| Labeled::new((*a).to_string(), FieldKind::Work)).collect(),
            favorite,
            ..Contact::default()
        }
    }

    fn day(n: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, n, 9, 0, 0).single().expect("date")
    }

    fn person(address: &str, score: f64, sent: Option<u32>, received: Option<u32>) -> Correspondent {
        Correspondent {
            address: address.into(),
            name: None,
            score,
            last_sent: sent.map(day),
            last_received: received.map(day),
        }
    }

    fn lines(suggestions: &[RecipientSuggestion]) -> Vec<String> {
        suggestions
            .iter()
            .map(|s| match s.kind {
                SuggestionKind::Group => format!("group {}", s.name),
                SuggestionKind::Seen => format!("seen {}", s.address),
                SuggestionKind::Contact => s.address.clone(),
            })
            .collect()
    }

    #[test]
    fn a_word_matches_only_from_the_start_of_a_name_or_address_part() {
        let words = search_words("Jo BE");
        assert!(matches(&words, "Jonas Berger", "jonas@berger-arch.de"));
        assert!(matches(&search_words("arch"), "Jonas Berger", "jonas@berger-arch.de"));
        assert!(matches(&search_words("müll"), "Lena Müller", "lena@example.org"));
        assert!(matches(&search_words("MULL"), "Lena Müller", "lena@example.org"), "folded both ways");
        assert!(!matches(&search_words("onas"), "Jonas Berger", "jonas@berger-arch.de"));
        assert!(!matches(&search_words(""), "Jonas", "jonas@x.de"), "an empty search matches nothing");
    }

    #[test]
    fn written_to_first_then_favourites_groups_and_the_rest() {
        let contacts = vec![
            contact("c1", "Jonas Berger", &["jonas@berger-arch.de"], false),
            contact("c2", "Johanna Kraft", &["j.kraft@stadtwerke.de"], true),
            contact("c3", "Jochen Abel", &["jochen@abel.de"], false),
            contact("c4", "Josef Zander", &["josef@zander.de"], false),
        ];
        let groups = vec![GroupCandidate {
            name: "Jour fixe".into(),
            members: vec![contacts[0].clone(), contacts[2].clone(), contact("c9", "Mira", &["mira@x.de"], false)],
        }];
        let history =
            vec![person("josef@zander.de", 2.0, Some(1), None), person("JONAS@berger-arch.de", 4.0, Some(3), None)];
        let found = rank("jo", &contacts, &groups, &history, None);
        assert_eq!(
            lines(&found),
            ["jonas@berger-arch.de", "josef@zander.de", "j.kraft@stadtwerke.de", "group Jour fixe", "jochen@abel.de"]
        );
        assert_eq!(found[0].last_sent, Some(day(3)), "the last mail sent there");
        assert_eq!(found[3].members.len(), 3, "a group stands for all its members");
        assert!(found.iter().all(|s| s.untrusted));
    }

    #[test]
    fn a_group_without_addresses_is_left_out_and_members_come_once() {
        let jonas = contact("c1", "Jonas", &["jonas@x.de"], false);
        let groups = vec![
            GroupCandidate { name: "Team".into(), members: vec![jonas.clone(), jonas.clone()] },
            GroupCandidate { name: "Team".into(), members: vec![jonas.clone()] },
            GroupCandidate { name: "Tennis".into(), members: vec![contact("c2", "Nobody", &[], false)] },
        ];
        let found = rank("te", &[], &groups, &[], None);
        assert_eq!(lines(&found), ["group Team"]);
        assert_eq!(found[0].members, [SuggestedAddress { name: "Jonas".into(), address: "jonas@x.de".into() }]);
    }

    #[test]
    fn addresses_only_from_mail_come_last_the_latest_first_with_room_kept() {
        let contacts: Vec<Contact> =
            (0..10).map(|i| contact(&format!("c{i}"), &format!("Jo {i}"), &[&format!("jo{i}@x.de")], false)).collect();
        let history = vec![
            person("jo.meyer@posteo.de", 0.0, None, Some(2)),
            person("jo.old@posteo.de", 0.0, None, Some(1)),
            person("jo.new@posteo.de", 1.0, Some(4), None),
            // In an address book: never "seen".
            person("jo3@x.de", 0.0, None, Some(4)),
        ];
        let found = rank("jo", &contacts, &[], &history, None);
        assert_eq!(found.len(), 8);
        assert_eq!(found.iter().filter(|s| s.kind == SuggestionKind::Contact).count(), 6, "room for two");
        assert_eq!(lines(&found[6..]), ["seen jo.new@posteo.de", "seen jo.meyer@posteo.de"]);
        // Without enough contacts the seen ones fill the rest.
        let few = rank("jo", &contacts[3..4], &[], &history, Some(5));
        assert_eq!(
            lines(&few),
            ["jo3@x.de", "seen jo.new@posteo.de", "seen jo.meyer@posteo.de", "seen jo.old@posteo.de"]
        );
    }

    #[test]
    fn the_limit_is_held_within_one_and_twenty_and_every_address_comes_once() {
        let contacts: Vec<Contact> =
            (0..30).map(|i| contact(&format!("c{i}"), "Anna", &[&format!("anna{i}@x.de")], false)).collect();
        assert_eq!(rank("an", &contacts, &[], &[], Some(0)).len(), 1);
        assert_eq!(rank("an", &contacts, &[], &[], Some(99)).len(), MAX_LIMIT as usize);
        let twice = vec![contact("a", "Anna", &["anna@x.de"], false), contact("b", "Anna B", &["ANNA@x.de"], true)];
        assert_eq!(rank("anna", &twice, &[], &[], None).len(), 1);
        assert!(rank("   ", &twice, &[], &[], None).is_empty());
    }

    #[test]
    fn without_mail_leaves_contacts_by_name_and_nothing_of_mail() {
        let contacts = vec![
            contact("c1", "Zora Abel", &["zora@x.de"], false),
            contact("c2", "Anna", &["anna@x.de"], false),
            contact("c3", "Mia Adler", &["mia@x.de"], true),
        ];
        let history = vec![person("zora@x.de", 4.0, Some(3), None), person("al@y.de", 0.0, None, Some(2))];
        let found = rank("a", &contacts, &[], &history, None);
        assert_eq!(lines(&found), ["zora@x.de", "mia@x.de", "anna@x.de", "seen al@y.de"]);
        let kept = without_mail(&found);
        assert_eq!(lines(&kept), ["mia@x.de", "anna@x.de", "zora@x.de"], "favourite, then by name");
        assert!(kept.iter().all(|s| s.last_sent.is_none() && s.last_seen.is_none()));
    }

    #[test]
    fn people_of_several_accounts_are_merged() {
        let merged = merge([
            person("a@x.de", 2.0, Some(1), None),
            Correspondent { name: Some("Anna".into()), ..person("A@X.de", 0.5, Some(3), Some(2)) },
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!((merged[0].score, merged[0].last_sent, merged[0].last_received), (2.5, Some(day(3)), Some(day(2))));
        assert_eq!(merged[0].name.as_deref(), Some("Anna"));
        assert_eq!(sent_weight(0), 4.0);
        assert_eq!(sent_weight(20), 2.0);
        assert_eq!(sent_weight(60), 1.0);
        assert_eq!(sent_weight(300), 0.5);
        assert_eq!(sent_weight(400), 0.0);
    }
}
