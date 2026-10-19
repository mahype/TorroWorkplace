//! The words for a mail's category (task E12.1, concept `docs/kategorien.md`): what a person
//! reads on the pill, in the sidebar and in the "Category" menu, and what a decision model is
//! told each option means.
//!
//! The category itself is [`torro_mail::category`], which holds no words: `torro-core` depends on
//! `torro-mail`, so a `tr()` there would be a dependency cycle. This module closes that gap and is
//! the **one** place a category's words come from – the API answers with them too
//! (`inbox.list_categories`), so the app and an agent read the same thing.
//!
//! Every label goes through [`crate::i18n::trc`] with the context `category`, not plain `tr`:
//! several of these English words already mean something else in the suite ("Task" is a to-do,
//! "Order" is a sort order, "Event" is a calendar event, "Junk" is the spam folder). The context
//! keeps those apart and makes a later collision impossible.
//!
//! [`description`] is the odd one out: it is **not** translated. It travels to a decision model as
//! the `criteria` of the `choice` question (task E12.3), and the model is prompted in English.

use torro_mail::category::{Category, CategoryGroup};

/// The context every category word is translated under ([`crate::i18n::trc`]).
pub(crate) const CONTEXT: &str = "category";

/// The English word for `category`, untranslated: what [`label`] looks up and what the entry in
/// `i18n::de` is keyed by (`category|<word>`). A test over the table checks that each one is
/// there, so a new category cannot ship without its German word.
pub(crate) fn english_label(category: Category) -> &'static str {
    match category {
        Category::Request => "Request",
        Category::Task => "Task",
        Category::Approval => "Approval",
        Category::Security => "Security",
        Category::Personal => "Personal",
        Category::Invitation => "Invitation",
        Category::Invoice => "Invoice",
        Category::Receipt => "Receipt",
        Category::Order => "Order",
        Category::Shipping => "Shipping",
        Category::Booking => "Booking",
        Category::Statement => "Statement",
        Category::Document => "Document",
        Category::Newsletter => "Newsletter",
        Category::Promotion => "Promotion",
        Category::ServiceNotice => "Service notice",
        Category::Social => "Social",
        Category::EventInvite => "Event",
        Category::Bounce => "Not delivered",
        Category::AutoReply => "Auto-reply",
        Category::Spam => "Spam",
        Category::Phishing => "Phishing",
        Category::Other => "Other",
    }
}

/// What a person reads for `category`: "Invoice", "Shipping", "Phishing" …
pub fn label(category: Category) -> &'static str {
    crate::i18n::trc(CONTEXT, english_label(category))
}

/// The English word for `group`, untranslated; see [`english_label`].
pub(crate) fn english_group_label(group: CategoryGroup) -> &'static str {
    match group {
        CategoryGroup::WaitingForMe => "Waiting for me",
        CategoryGroup::PaperTrail => "Paper trail",
        CategoryGroup::Info => "Info",
        CategoryGroup::Junk => "Junk",
    }
}

/// What a person reads for `group`: the four entries under "Categories" in the sidebar.
pub fn group_label(group: CategoryGroup) -> &'static str {
    crate::i18n::trc(CONTEXT, english_group_label(group))
}

/// What the category means, for a **decision model** – the `criteria` of the one `choice`
/// question (task E12.3). English in every language: the model is prompted in English, and these
/// words are not shown to a person.
pub fn description(category: Category) -> &'static str {
    match category {
        Category::Request => "A question or request addressed to me; an answer is expected.",
        Category::Task => "An assignment or a commitment with a deadline I have to meet.",
        Category::Approval => "A signature, confirmation or authorisation is wanted from me.",
        Category::Security => "A sign-in code, a two-factor prompt or a password reset: short-lived and pressing.",
        Category::Personal => "A message written by a person, with nothing for me to do.",
        Category::Invitation => "An invitation to a calendar event.",
        Category::Invoice => "An invoice, a reminder to pay or a demand for payment.",
        Category::Receipt => "A payment confirmed: a receipt or proof of purchase.",
        Category::Order => "An order or purchase confirmation.",
        Category::Shipping => "A shipping confirmation, a tracking update or a delivery attempt.",
        Category::Booking => "A booking: travel, a hotel, a ticket or an appointment.",
        Category::Statement => "A bank statement, a payslip or an annual statement.",
        Category::Document => "A contract, an insurance policy, a notice of termination or a letter from an authority.",
        Category::Newsletter => "Editorial content, a subscription or a blog post.",
        Category::Promotion => "An offer, a discount or product advertising.",
        Category::ServiceNotice => "Maintenance, changed terms or a status report of a service.",
        Category::Social => "A social network, a forum or a community.",
        Category::EventInvite => "An invitation to a webinar, a conference or another event.",
        Category::Bounce => "A delivery status notification: the mail could not be delivered.",
        Category::AutoReply => "An automatic reply, such as an out-of-office notice.",
        Category::Spam => "Bulk mail with no connection to me.",
        Category::Phishing => "An attempt to deceive: a forged sender or a harmful link.",
        Category::Other => "Fits none of the other categories.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, trc_in};

    #[test]
    fn every_category_and_group_has_words_and_a_description() {
        for category in Category::ALL {
            assert!(!label(category).trim().is_empty(), "{category:?} has no label");
            let description = description(category);
            assert!(!description.trim().is_empty(), "{category:?} has no description");
            assert!(description.ends_with('.'), "{category:?}: a criterion is a sentence");
        }
        for group in CategoryGroup::ALL {
            assert!(!group_label(group).trim().is_empty(), "{group:?} has no label");
        }
    }

    /// Two categories must never read the same, or the menu and the sidebar would be ambiguous.
    #[test]
    fn no_two_categories_read_alike_in_either_language() {
        for lang in [Lang::En, Lang::De] {
            let mut seen = std::collections::HashSet::new();
            for category in Category::ALL {
                let words = words_in(lang, category);
                assert!(seen.insert(words), "{category:?} reads like another category in {lang:?}: {words}");
            }
        }
    }

    /// The label of `category` in `lang`, independent of the process's own language.
    fn words_in(lang: Lang, category: Category) -> &'static str {
        trc_in(lang, CONTEXT, english_label(category))
    }

    /// Task E12.1: the five English words the suite already uses for something else keep their
    /// own German word under the context. That the table is *complete* is checked in `i18n`,
    /// which can see it (`every_mail_category_is_translated`).
    #[test]
    fn german_keeps_the_clashing_words_apart() {
        assert_eq!(trc_in(Lang::De, CONTEXT, "Task"), "Auftrag", "the to-do stays \"Aufgabe\"");
        assert_eq!(trc_in(Lang::De, CONTEXT, "Order"), "Bestellung", "the sort order stays \"Reihenfolge\"");
        assert_eq!(trc_in(Lang::De, CONTEXT, "Event"), "Veranstaltung", "the calendar event stays \"Termin\"");
        assert_eq!(trc_in(Lang::De, CONTEXT, "Other"), "Sonstiges", "\"Weitere\" is the other one");
        assert_eq!(trc_in(Lang::De, CONTEXT, "Junk"), "Technik und Müll", "the folder stays \"Spam\"");
        // Without the context those words still mean what they meant before.
        assert_eq!(crate::i18n::tr_in(Lang::De, "Task"), "Aufgabe");
        assert_eq!(crate::i18n::tr_in(Lang::De, "Order"), "Reihenfolge");
        assert_eq!(crate::i18n::tr_in(Lang::De, "Junk"), "Spam");
    }

    /// English shows the English word, whatever the table holds.
    #[test]
    fn english_reads_the_english_words() {
        assert_eq!(trc_in(Lang::En, CONTEXT, "Task"), "Task");
        assert_eq!(trc_in(Lang::En, CONTEXT, "Not delivered"), "Not delivered");
    }
}
