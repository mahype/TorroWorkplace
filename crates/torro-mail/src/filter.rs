//! Mail filters (issue #339): a saved search of the mail search language with what to do with the
//! mail it finds – mark it as read, file it under a category, move it to the trash. Mail works with
//! categories only, so a filter no longer moves mail to the archive or a folder (issue #454).
//!
//! The types are the API's contract: the core keeps the filters in `<data>/mail/filters.json`
//! (`mail.list_filters`, `mail.create_filter` …), the settings page edits them, and the core runs the
//! enabled ones on new mail in an account's inbox. Which actions win when several filters find the
//! same mail is a pure rule here ([`FilterActions::of`]), so the core and any preview decide the same.
//!
//! **Order counts.** Filters stand in the order the person keeps them. Every matching filter runs;
//! for the trash and the category the first one that sets it wins, reading needs only one. Which
//! category new mail gets when filters or sorting rules of several categories match is the order of
//! the categories (`torro_inbox::rules::ranked_category`, issue #481): the one higher up wins.
//!
//! **Ids** have the fixed form `f<n>` (`f1`, `f12`), given once and never again, so the audit log
//! may write them as they are (they are hex digits) and an old reference never names another filter.
//!
//! On the wire like the other mail types: fields in snake_case (`mark_read`), the move as
//! `{"to": "trash"}`. The moves of before issue #454 (`{"to": "archive"}`, `{"to": "folder", …}`)
//! are refused with [`FILE_ONLY_TRASH`]; the core drops them from the filters it stored.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::Category;

/// The most filters the core keeps.
pub const FILTERS_MAX: usize = 200;

/// The longest search of a filter, in characters: as long as a search of `mail.search_messages`.
pub const FILTER_QUERY_MAX: usize = 500;

/// How the id of a filter starts: `f<n>`.
pub const FILTER_ID_PREFIX: &str = "f";

/// Why a move other than the trash is refused: mail works with categories, a filter moves to the
/// trash only (issue #454).
pub const FILE_ONLY_TRASH: &str = "file only takes {\"to\": \"trash\"}";

/// Where a filter moves the mail it finds: the trash only. A move into the trash can be taken back
/// from there until it is emptied; nothing is ever deleted for good.
///
/// Read by hand, so the archive and a folder of before issue #454 are refused with
/// [`FILE_ONLY_TRASH`] instead of serde's word for an unknown variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "to")]
pub enum FilterFile {
    /// The trash folder of the mail's account.
    Trash,
}

impl<'de> Deserialize<'de> for FilterFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            to: String,
        }
        match Wire::deserialize(deserializer)?.to.as_str() {
            "trash" => Ok(FilterFile::Trash),
            _ => Err(D::Error::custom(FILE_ONLY_TRASH)),
        }
    }
}

/// A mail filter as the core keeps it: a search, the account it runs in and what it does.
///
/// It finds mail in the inbox folder only: new mail as it arrives (while Torro Workplace runs), and
/// mail already there only through `mail.apply_filter`. At least one action is set: `file` (the
/// trash), `mark_read` or `category`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MailFilter {
    /// The id the core gave it, `f<n>`; stays when the filter is changed.
    pub id: String,
    /// The search, in the language of `mail.search_messages` (`from:shop.example has:attachment`);
    /// at most [`FILTER_QUERY_MAX`] characters, not empty. No `in:` (a filter looks at the inbox
    /// only) and no `filename:`. The person's own words: never shown in a log.
    pub query: String,
    /// The mail account it runs in (`mail:<address>`); left out: every mail account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Moves the mail to the trash; left out: it stays in the inbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<FilterFile>,
    /// Marks the mail as read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mark_read: bool,
    /// Files the mail under this category of `inbox.list_categories`, without asking a model, where
    /// the filter stands in the order of the sorting steps; a category the person set by hand stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    /// Switched off, it finds nothing on new mail and keeps its place.
    pub enabled: bool,
    /// When it was made.
    pub created: DateTime<Utc>,
}

impl MailFilter {
    /// It does something: it moves to the trash, marks as read or sets a category.
    pub fn has_action(&self) -> bool {
        self.file.is_some() || self.mark_read || self.category.is_some()
    }

    /// It runs on new mail of `account` (`mail:<address>`): it is enabled and made for that account
    /// or for every account.
    pub fn runs_in(&self, account: &str) -> bool {
        self.enabled && self.account.as_deref().is_none_or(|own| own == account)
    }
}

/// The number of a filter id (`f12` → 12); `None` for any other text.
pub fn filter_number(id: &str) -> Option<u32> {
    let digits = id.strip_prefix(FILTER_ID_PREFIX)?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The id of the filter numbered `number`: `f<number>`.
pub fn filter_id(number: u32) -> String {
    format!("{FILTER_ID_PREFIX}{number}")
}

/// What several filters that found the same mail do together (decision 3 of issue #339): read once
/// any of them marks it as read; the category and the trash of the **first** filter that sets one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterActions {
    /// Mark the mail as read.
    pub mark_read: bool,
    /// File it under this category.
    pub category: Option<Category>,
    /// Move it to the trash.
    pub file: Option<FilterFile>,
}

impl FilterActions {
    /// The actions of `filters`, in the order the person keeps them. The caller passes only the
    /// filters that found the mail; whether they are enabled is not looked at here.
    pub fn of<'a>(filters: impl IntoIterator<Item = &'a MailFilter>) -> FilterActions {
        let mut actions = FilterActions::default();
        for filter in filters {
            actions.mark_read |= filter.mark_read;
            if actions.category.is_none() {
                actions.category = filter.category;
            }
            if actions.file.is_none() {
                actions.file = filter.file.clone();
            }
        }
        actions
    }

    /// Nothing to do.
    pub fn is_empty(&self) -> bool {
        !self.mark_read && self.category.is_none() && self.file.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(id: u32) -> MailFilter {
        MailFilter {
            id: filter_id(id),
            query: "from:shop.example".into(),
            account: None,
            file: None,
            mark_read: false,
            category: None,
            enabled: true,
            created: DateTime::<Utc>::UNIX_EPOCH,
        }
    }

    #[test]
    fn a_filter_is_written_with_its_move_as_a_tagged_object_and_read_back() {
        let mut moved = filter(3);
        moved.account = Some("mail:anna@example.org".into());
        moved.file = Some(FilterFile::Trash);
        moved.mark_read = true;
        moved.category = Some(Category::Invoice);
        let json = serde_json::to_value(&moved).expect("writes");
        assert_eq!(json["id"], "f3");
        assert_eq!(json["file"], serde_json::json!({"to": "trash"}));
        assert_eq!(json["mark_read"], true);
        assert_eq!(json["category"], "invoice");
        assert_eq!(serde_json::from_value::<MailFilter>(json).expect("reads"), moved);

        let plain = serde_json::to_value(filter(1)).expect("writes");
        assert!(plain.get("account").is_none() && plain.get("file").is_none() && plain.get("mark_read").is_none());
    }

    /// Issue #454: the archive and a folder are refused with a message that says what is left.
    #[test]
    fn only_the_trash_reads_as_a_move() {
        assert_eq!(
            serde_json::from_value::<FilterFile>(serde_json::json!({"to": "trash"})).ok(),
            Some(FilterFile::Trash)
        );
        for old in [serde_json::json!({"to": "archive"}), serde_json::json!({"to": "folder", "id": "f:7"})] {
            let err = serde_json::from_value::<FilterFile>(old).expect_err("refused").to_string();
            assert!(err.contains(FILE_ONLY_TRASH), "{err}");
        }
    }

    #[test]
    fn ids_are_f_and_a_number_without_leading_zeros() {
        assert_eq!(filter_id(12), "f12");
        assert_eq!(filter_number("f12"), Some(12));
        for wrong in ["f", "f0", "f012", "F1", "f1a", "12", "f-1", "f:1"] {
            assert_eq!(filter_number(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn a_filter_runs_in_its_account_or_every_one_while_enabled() {
        let mut every = filter(1);
        assert!(every.runs_in("mail:anna@example.org"));
        every.enabled = false;
        assert!(!every.runs_in("mail:anna@example.org"));
        let mut one = filter(2);
        one.account = Some("mail:anna@example.org".into());
        assert!(one.runs_in("mail:anna@example.org") && !one.runs_in("mail:bernd@example.org"));
        assert!(!one.has_action());
        one.mark_read = true;
        assert!(one.has_action());
    }

    /// Decision 3: every filter runs, reading needs one, trash and category the first that sets them.
    #[test]
    fn the_first_filter_wins_the_move_and_the_category() {
        let mut first = filter(1);
        first.category = Some(Category::Invoice);
        let mut second = filter(2);
        second.mark_read = true;
        second.category = Some(Category::Newsletter);
        let mut third = filter(3);
        third.file = Some(FilterFile::Trash);
        let actions = FilterActions::of([&first, &second, &third]);
        assert_eq!(
            actions,
            FilterActions { mark_read: true, category: Some(Category::Invoice), file: Some(FilterFile::Trash) }
        );
        assert!(FilterActions::of([]).is_empty());
    }
}
