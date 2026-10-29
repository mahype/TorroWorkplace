//! Three-way merge of a contact changed here and on the server at the same time.
//!
//! Field by field: a field only one side changed takes that side's value; both sides with the
//! same value agree. Fields with several values (mail addresses, numbers, addresses, links,
//! handles, groups) merge as sets: one side adds a number, the other removes another, and both
//! happen. Only a single field both sides set to different values is a conflict the person
//! decides (`contacts.resolve_conflict`).

use crate::model::{Contact, Labeled};

/// What [`merge`] found.
// Made once per write that met a newer version; the size does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merge {
    /// Both changes fit together: the server's contact with this device's changes.
    Merged(Contact),
    /// Both sides changed the same field differently.
    Conflict { mine: Contact, theirs: Contact },
}

/// A single value: the side that changed it wins; both changing it differently is `None`.
fn pick<T: PartialEq + Clone>(base: &T, mine: &T, theirs: &T) -> Option<T> {
    if mine == base || mine == theirs {
        Some(theirs.clone())
    } else if theirs == base {
        Some(mine.clone())
    } else {
        None
    }
}

/// Values as sets, kept in the server's order: what this device removed goes, what it added
/// comes after; a value whose kind or preference one side changed takes that change.
fn merge_set<T: PartialEq + Clone>(
    base: &[Labeled<T>],
    mine: &[Labeled<T>],
    theirs: &[Labeled<T>],
) -> Option<Vec<Labeled<T>>> {
    let find = |list: &[Labeled<T>], value: &T| list.iter().find(|l| l.value == *value).cloned();
    let mut merged = Vec::new();
    for item in theirs {
        let in_base = find(base, &item.value);
        let in_mine = find(mine, &item.value);
        match (in_base, in_mine) {
            // Removed here, untouched there: it goes.
            (Some(b), None) if b == *item => {}
            // Removed here, changed there: the server's change stays.
            (Some(_), None) => merged.push(item.clone()),
            (Some(b), Some(m)) => merged.push(pick(&b, &m, item)?),
            (None, _) => merged.push(item.clone()),
        }
    }
    for item in mine {
        let new_here = find(base, &item.value).is_none();
        if new_here && find(theirs, &item.value).is_none() {
            merged.push(item.clone());
        }
    }
    Some(merged)
}

fn merge_groups(base: &[String], mine: &[String], theirs: &[String]) -> Vec<String> {
    let mut merged: Vec<String> = theirs.iter().filter(|g| mine.contains(g) || !base.contains(g)).cloned().collect();
    for group in mine {
        if !base.contains(group) && !merged.contains(group) {
            merged.push(group.clone());
        }
    }
    merged
}

/// Merges `mine` (this device's version) and `theirs` (the server's), both changed from `base`.
/// The merged contact keeps the server's id, `etag` and local state of `theirs`.
pub fn merge(base: &Contact, mine: &Contact, theirs: &Contact) -> Merge {
    let conflict = || Merge::Conflict { mine: mine.clone(), theirs: theirs.clone() };
    let mut merged = theirs.clone();
    macro_rules! single {
        ($($field:ident),*) => {$(
            match pick(&base.$field, &mine.$field, &theirs.$field) {
                Some(value) => merged.$field = value,
                None => return conflict(),
            }
        )*};
    }
    single!(name, display_name, nickname, org, title, birthday, anniversary, note, photo, geo);
    macro_rules! set {
        ($($field:ident),*) => {$(
            match merge_set(&base.$field, &mine.$field, &theirs.$field) {
                Some(value) => merged.$field = value,
                None => return conflict(),
            }
        )*};
    }
    set!(emails, phones, addresses, urls, impps);
    merged.groups = merge_groups(&base.groups, &mine.groups, &theirs.groups);
    Merge::Merged(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FieldKind;

    fn phone(value: &str) -> Labeled<String> {
        Labeled::new(value.to_string(), FieldKind::Mobile)
    }

    fn base() -> Contact {
        Contact {
            display_name: "Anna Berg".into(),
            org: Some("Acme".into()),
            phones: vec![phone("1"), phone("2")],
            groups: vec!["Work".into()],
            ..Contact::default()
        }
    }

    #[test]
    fn only_one_side_changed() {
        let base = base();
        let mut mine = base.clone();
        mine.title = Some("Boss".into());
        assert_eq!(merge(&base, &mine, &base), Merge::Merged(mine.clone()), "only here");
        assert_eq!(merge(&base, &base, &mine), Merge::Merged(mine.clone()), "only on the server");
        assert_eq!(merge(&base, &mine, &mine), Merge::Merged(mine), "both the same");
    }

    #[test]
    fn different_fields_merge_and_the_same_field_conflicts() {
        let base = base();
        let mut mine = base.clone();
        mine.title = Some("Boss".into());
        let mut theirs = base.clone();
        theirs.note = Some("from the server".into());
        let Merge::Merged(merged) = merge(&base, &mine, &theirs) else { panic!("no conflict expected") };
        assert_eq!((merged.title.as_deref(), merged.note.as_deref()), (Some("Boss"), Some("from the server")));

        theirs.org = Some("Other".into());
        mine.org = Some("Mine".into());
        assert!(matches!(merge(&base, &mine, &theirs), Merge::Conflict { .. }));
    }

    #[test]
    fn several_values_merge_as_sets() {
        let base = base();
        let mut mine = base.clone();
        mine.phones.push(phone("3"));
        mine.groups.push("Friends".into());
        let mut theirs = base.clone();
        theirs.phones.retain(|p| p.value != "1");
        theirs.groups.clear();
        let Merge::Merged(merged) = merge(&base, &mine, &theirs) else { panic!("sets never conflict here") };
        let values: Vec<&str> = merged.phones.iter().map(|p| p.value.as_str()).collect();
        assert_eq!(values, ["2", "3"], "one side added, the other removed: both happen");
        assert_eq!(merged.groups, ["Friends"]);

        // The same value relabelled differently on both sides is a conflict.
        let mut mine = base.clone();
        mine.phones[0].kind = FieldKind::Home;
        let mut theirs = base.clone();
        theirs.phones[0].kind = FieldKind::Work;
        assert!(matches!(merge(&base, &mine, &theirs), Merge::Conflict { .. }));
    }
}
