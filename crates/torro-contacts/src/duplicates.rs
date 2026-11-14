//! Contacts that look like one person (issue #726, `contacts.find_duplicates` and
//! `contacts.merge_contacts`), and the rule an import keeps out the people there already (issue
//! #725). No input or output here.
//!
//! - **Alike** are two contacts with the same mail address (whatever its case), the same number
//!   ([`normalize_phone`]: "0171 2345678" and "+49 171 2345678" are one number where the trunk
//!   prefix 0 means Germany), or the same name with the same company, both set ([`same_person`]).
//! - **Shared** addresses and numbers do not count when more than [`SHARED_MAX`] contacts hold them:
//!   a company's switchboard or `info@` is no sign that the people behind it are one.
//! - **Groups** are the contacts tied by such pairs, directly or through others ([`find`]); pairs the
//!   person said are different people ("Not duplicates") are left out.
//! - **Merging** ([`merged`]) keeps the single fields of the contact chosen, fills its empty ones
//!   from the others in their order, and joins the lists (mail addresses, numbers, addresses, links,
//!   handles, groups) without making any value of the others the preferred one.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::model::{Contact, DuplicateGroup, DuplicateReason, Labeled};
use crate::suggest::fold;

/// More contacts than this with one mail address or number do not make duplicates of each other.
pub const SHARED_MAX: usize = 3;

/// Numbers with fewer digits are no sign of a person (extensions, short codes).
const MIN_DIGITS: usize = 6;

/// The country calling code a number with the trunk prefix 0 belongs to, by the language of the
/// suite: `de` is Germany (`49`); English names no country, so "0171…" stays as written.
pub fn country_code(language: &str) -> Option<&'static str> {
    match language.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase().as_str() {
        "de" => Some("49"),
        _ => None,
    }
}

/// `text` as a number to compare: `+<digits>` for a number written internationally (`+` or `00`,
/// a `(0)` after the country code left out), `+<country><digits>` for one with the trunk prefix 0
/// when `country` is known, else its digits. `None` below six digits.
pub fn normalize_phone(text: &str, country: Option<&str>) -> Option<String> {
    let text = text.trim();
    let digits = |text: &str| text.chars().filter(char::is_ascii_digit).collect::<String>();
    let international = |rest: &str| {
        let rest = digits(&rest.replacen("(0)", "", 1));
        (rest.len() >= MIN_DIGITS).then(|| format!("+{rest}"))
    };
    if let Some(rest) = text.strip_prefix('+') {
        return international(rest);
    }
    let all = digits(text);
    if let Some(rest) = all.strip_prefix("00") {
        return international(rest);
    }
    if all.len() < MIN_DIGITS {
        return None;
    }
    match (all.strip_prefix('0'), country) {
        (Some(rest), Some(country)) => Some(format!("+{country}{rest}")),
        _ => Some(all),
    }
}

/// A mail address to compare: trimmed, lower case; `None` when empty.
fn mail_key(address: &str) -> Option<String> {
    let key = address.trim().to_lowercase();
    (!key.is_empty()).then_some(key)
}

/// The person's name and company to compare, folded; `None` unless both are set. The name is the
/// one shown or the parts of the name, never the company or an address standing in for it.
fn name_key(contact: &Contact) -> Option<String> {
    let name = if contact.display_name.trim().is_empty() { contact.name.full() } else { contact.display_name.clone() };
    let name = fold(&name).split_whitespace().collect::<Vec<_>>().join(" ");
    let company = fold(contact.org.as_deref().unwrap_or_default()).split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty() && !company.is_empty()).then(|| format!("{name}\u{0}{company}"))
}

/// The keys of `contact` for each reason.
pub fn keys(contact: &Contact, country: Option<&str>) -> Vec<(DuplicateReason, String)> {
    let mut keys: Vec<(DuplicateReason, String)> = Vec::new();
    for key in contact.emails.iter().filter_map(|e| mail_key(&e.value)) {
        keys.push((DuplicateReason::Mail, key));
    }
    for key in contact.phones.iter().filter_map(|p| normalize_phone(&p.value, country)) {
        keys.push((DuplicateReason::Phone, key));
    }
    keys.extend(name_key(contact).map(|key| (DuplicateReason::NameAndCompany, key)));
    keys.sort();
    keys.dedup();
    keys
}

/// Why `a` and `b` look like one person, every reason once and in order; empty when they do not.
/// For the import, which compares one card with each contact: no [`SHARED_MAX`] here.
pub fn same_person(a: &Contact, b: &Contact, country: Option<&str>) -> Vec<DuplicateReason> {
    let theirs = keys(b, country);
    let mut reasons: Vec<DuplicateReason> =
        keys(a, country).into_iter().filter(|key| theirs.contains(key)).map(|(reason, _)| reason).collect();
    reasons.dedup();
    reasons
}

/// How many fields a contact fills: the one with most comes first in a group.
pub fn filled(contact: &Contact) -> usize {
    let text = |value: &Option<String>| usize::from(value.as_deref().is_some_and(|v| !v.trim().is_empty()));
    usize::from(!contact.display_name.trim().is_empty())
        + usize::from(!contact.name.is_empty())
        + text(&contact.nickname)
        + text(&contact.org)
        + text(&contact.title)
        + text(&contact.note)
        + usize::from(contact.photo.is_some())
        + usize::from(contact.birthday.is_some())
        + usize::from(contact.anniversary.is_some())
        + contact.emails.len()
        + contact.phones.len()
        + contact.addresses.len()
        + contact.urls.len()
        + contact.impps.len()
        + contact.groups.len()
}

/// The pair of ids as "Not duplicates" keeps it: the smaller first.
pub fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) }
}

/// The groups of `contacts` that look like one person each ([module docs](self)), without the pairs
/// in `dismissed` ([`pair`]). Each group holds its contacts with most fields first, the groups sorted
/// by the name of their first contact.
pub fn find(contacts: &[Contact], country: Option<&str>, dismissed: &HashSet<(String, String)>) -> Vec<DuplicateGroup> {
    let mut holders: BTreeMap<(DuplicateReason, String), Vec<usize>> = BTreeMap::new();
    for (index, contact) in contacts.iter().enumerate() {
        for key in keys(contact, country) {
            holders.entry(key).or_default().push(index);
        }
    }
    // Union-find over the contacts, with the reasons of every pair that joined them.
    let mut parent: Vec<usize> = (0..contacts.len()).collect();
    fn root(parent: &mut [usize], mut at: usize) -> usize {
        while parent[at] != at {
            parent[at] = parent[parent[at]];
            at = parent[at];
        }
        at
    }
    let mut edges: Vec<(usize, DuplicateReason)> = Vec::new();
    for ((reason, _), holding) in &holders {
        let shared = matches!(reason, DuplicateReason::Mail | DuplicateReason::Phone) && holding.len() > SHARED_MAX;
        if holding.len() < 2 || shared {
            continue;
        }
        for (n, &a) in holding.iter().enumerate() {
            for &b in &holding[n + 1..] {
                if dismissed.contains(&pair(&contacts[a].id, &contacts[b].id)) {
                    continue;
                }
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                if ra != rb {
                    parent[ra] = rb;
                }
                edges.push((a, *reason));
            }
        }
    }
    let mut groups: BTreeMap<usize, (BTreeSet<usize>, BTreeSet<DuplicateReason>)> = BTreeMap::new();
    for (a, reason) in edges {
        let group = groups.entry(root(&mut parent, a)).or_default();
        group.1.insert(reason);
    }
    for index in 0..contacts.len() {
        let at = root(&mut parent, index);
        if let Some(group) = groups.get_mut(&at) {
            group.0.insert(index);
        }
    }
    let mut found: Vec<DuplicateGroup> = groups
        .into_values()
        .filter(|(members, _)| members.len() >= 2)
        .map(|(members, reasons)| {
            let mut members: Vec<&Contact> = members.into_iter().map(|i| &contacts[i]).collect();
            members.sort_by(|a, b| filled(b).cmp(&filled(a)).then_with(|| a.id.cmp(&b.id)));
            DuplicateGroup { reasons: reasons.into_iter().collect(), contacts: members.into_iter().cloned().collect() }
        })
        .collect();
    found.sort_by_key(|group| group.contacts.first().map(|c| fold(&c.shown_name())).unwrap_or_default());
    found
}

/// `keep` with what `others` bring ([module docs](self)): its single fields stay, empty ones are
/// filled from the others in their order; mail addresses (whatever their case), numbers (as
/// [`normalize_phone`] compares them), addresses, links, handles and groups are joined, the others'
/// never preferred. The picture is not touched here: it is a line of its own in the vCard.
pub fn merged(keep: &Contact, others: &[Contact], country: Option<&str>) -> Contact {
    fn fill(current: &mut Option<String>, other: &Option<String>) {
        if current.as_deref().is_none_or(|c| c.trim().is_empty())
            && let Some(value) = other.as_deref().filter(|v| !v.trim().is_empty())
        {
            *current = Some(value.to_string());
        }
    }
    fn join<T: Clone>(current: &mut Vec<Labeled<T>>, other: &[Labeled<T>], same: impl Fn(&T, &T) -> bool) {
        for value in other {
            if !current.iter().any(|held| same(&held.value, &value.value)) {
                current.push(Labeled { value: value.value.clone(), kind: value.kind.clone(), pref: false });
            }
        }
    }
    let mut merged = keep.clone();
    for other in others {
        if merged.display_name.trim().is_empty() {
            merged.display_name = other.display_name.clone();
        }
        if merged.name.is_empty() {
            merged.name = other.name.clone();
        }
        fill(&mut merged.nickname, &other.nickname);
        fill(&mut merged.org, &other.org);
        fill(&mut merged.title, &other.title);
        fill(&mut merged.note, &other.note);
        merged.birthday = merged.birthday.or(other.birthday);
        merged.anniversary = merged.anniversary.or(other.anniversary);
        join(&mut merged.emails, &other.emails, |a, b| mail_key(a) == mail_key(b));
        join(&mut merged.phones, &other.phones, |a, b| {
            a == b || normalize_phone(a, country).is_some_and(|n| Some(n) == normalize_phone(b, country))
        });
        join(&mut merged.addresses, &other.addresses, |a, b| a == b);
        join(&mut merged.urls, &other.urls, |a, b| a.trim() == b.trim());
        join(&mut merged.impps, &other.impps, |a, b| a.trim().eq_ignore_ascii_case(b.trim()));
        for group in &other.groups {
            if !merged.groups.contains(group) {
                merged.groups.push(group.clone());
            }
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FieldKind, Name, PartialDate};

    fn person(id: &str, name: &str, org: Option<&str>, mails: &[&str], phones: &[&str]) -> Contact {
        Contact {
            id: id.into(),
            display_name: name.into(),
            org: org.map(str::to_string),
            emails: mails.iter().map(|m| Labeled::new(m.to_string(), FieldKind::Work)).collect(),
            phones: phones.iter().map(|p| Labeled::new(p.to_string(), FieldKind::Mobile)).collect(),
            ..Contact::default()
        }
    }

    #[test]
    fn numbers_compare_however_they_are_written() {
        let de = country_code("de");
        assert_eq!(de, Some("49"));
        assert_eq!(country_code("de-AT"), Some("49"));
        assert_eq!(country_code("en"), None);
        let n = |text: &str| normalize_phone(text, de);
        assert_eq!(n("+49 171 2345678").as_deref(), Some("+491712345678"));
        assert_eq!(n("0171 / 234 56 78"), n("+49 171 2345678"));
        assert_eq!(n("0049 171 2345678"), n("+49 171 2345678"));
        assert_eq!(n("+49 (0) 171 2345678"), n("+49 171 2345678"));
        assert_eq!(n("12345"), None, "too short to say anything");
        assert_eq!(n("234 5678").as_deref(), Some("2345678"), "a local number stays as written");
        assert_eq!(normalize_phone("0171 2345678", None).as_deref(), Some("01712345678"), "no country known");
        assert_ne!(normalize_phone("0171 2345678", None), normalize_phone("+49 171 2345678", None));
    }

    #[test]
    fn people_are_alike_by_mail_number_or_name_with_company() {
        let de = country_code("de");
        let anna = person("a", "Anna Berg", Some("Acme"), &["Anna@Acme.de"], &["0171 2345678"]);
        let mail = person("b", "A. B.", None, &["anna@acme.de"], &[]);
        assert_eq!(same_person(&anna, &mail, de), [DuplicateReason::Mail]);
        let phone = person("c", "Anna", None, &[], &["+49 171 2345678"]);
        assert_eq!(same_person(&anna, &phone, de), [DuplicateReason::Phone]);
        let named = person("d", "anna  BERG", Some("ACME"), &[], &[]);
        assert_eq!(same_person(&anna, &named, de), [DuplicateReason::NameAndCompany]);
        let no_company = person("e", "Anna Berg", None, &[], &[]);
        assert!(same_person(&anna, &no_company, de).is_empty(), "a name alone names too many people");
        let all = person("f", "Anna Berg", Some("Acme"), &["anna@acme.de"], &["01712345678"]);
        assert_eq!(
            same_person(&anna, &all, de),
            [DuplicateReason::Mail, DuplicateReason::Phone, DuplicateReason::NameAndCompany]
        );
    }

    #[test]
    fn groups_join_pairs_leave_shared_numbers_and_dismissed_pairs_out() {
        let de = country_code("de");
        let contacts = vec![
            person("1", "Anna Berg", None, &["anna@acme.de"], &[]),
            person("2", "Anna B.", None, &["ANNA@acme.de"], &["0171 2345678"]),
            person("3", "Anna", None, &[], &["+49 171 2345678"]),
            person("4", "Ben", None, &["ben@acme.de"], &["0800 100100"]),
            // A switchboard four people share is no sign of one person.
            person("5", "Cem", None, &[], &["0800 100100"]),
            person("6", "Dora", None, &[], &["0800 100100"]),
            person("7", "Emil", None, &[], &["0800 100100"]),
            person("8", "Finn", None, &["finn@x.de"], &[]),
            person("9", "Finn X", None, &["finn@x.de"], &[]),
        ];
        let groups = find(&contacts, de, &HashSet::new());
        let ids = |group: &DuplicateGroup| group.contacts.iter().map(|c| c.id.clone()).collect::<Vec<String>>();
        assert_eq!(groups.len(), 2, "{groups:?}");
        assert_eq!(ids(&groups[0]), ["2", "1", "3"], "tied through each other, the fullest first");
        assert_eq!(groups[0].reasons, [DuplicateReason::Mail, DuplicateReason::Phone]);
        assert_eq!(ids(&groups[1]), ["8", "9"]);
        let dismissed: HashSet<(String, String)> = [pair("9", "8")].into_iter().collect();
        let groups = find(&contacts, de, &dismissed);
        assert_eq!(groups.len(), 1, "different people stay apart");
    }

    #[test]
    fn merging_keeps_the_chosen_fields_and_joins_the_lists() {
        let de = country_code("de");
        let mut keep = person("1", "Anna Berg", Some("Acme"), &["anna@acme.de"], &["0171 2345678"]);
        keep.emails[0].pref = true;
        let mut other = person("2", "Anna B.", Some("Other"), &["ANNA@acme.de", "anna@home.de"], &["+49 171 2345678"]);
        other.phones.push(Labeled { value: "030 1234567".into(), kind: FieldKind::Home, pref: true });
        other.note = Some("met at the fair".into());
        other.birthday = PartialDate::parse("1990-04-15");
        other.name = Name { family: "Berg".into(), given: "Anna".into(), ..Name::default() };
        other.groups = vec!["Friends".into()];
        let merged = merged(&keep, &[other], de);
        assert_eq!(merged.display_name, "Anna Berg");
        assert_eq!(merged.org.as_deref(), Some("Acme"), "the chosen contact's company stays");
        assert_eq!(merged.note.as_deref(), Some("met at the fair"), "an empty field is filled");
        assert_eq!(merged.name.family, "Berg");
        assert!(merged.birthday.is_some());
        let mails: Vec<&str> = merged.emails.iter().map(|e| e.value.as_str()).collect();
        assert_eq!(mails, ["anna@acme.de", "anna@home.de"]);
        let phones: Vec<&str> = merged.phones.iter().map(|p| p.value.as_str()).collect();
        assert_eq!(phones, ["0171 2345678", "030 1234567"]);
        assert!(merged.emails[0].pref && merged.phones.iter().skip(1).all(|p| !p.pref), "never the others' preferred");
        assert_eq!(merged.groups, ["Friends"]);
    }
}
