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
//! [`suite_description`] is the odd one out: it is **not** translated. It travels to a decision
//! model as the `criteria` of the `choice` question (task E12.3), and the model is prompted in
//! English.
//!
//! **The catalogue can be edited** (task E15.1): the person renames a category or writes their own
//! description, and makes categories of their own. So what a category is called is
//! [`name_of`] its entry in the catalogue ([`CategoryDef`]); the suite's words here are only what
//! an entry nobody changed falls back to.

use torro_mail::category::{Category, CategoryDef, CategoryGroup};

/// The context every category word is translated under ([`crate::i18n::trc`]).
pub(crate) const CONTEXT: &str = "category";

/// The English word for `category`, untranslated: what [`label`] looks up and what the entry in
/// `i18n::de` is keyed by (`category|<word>`). A test over the table checks that each one is
/// there, so a new category cannot ship without its German word. `None` for a category the person
/// made, which has only the name they gave it.
pub(crate) fn english_label(category: Category) -> Option<&'static str> {
    Some(match category {
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
        Category::Custom(_) => return None,
    })
}

/// The suite's word for `category`, translated: "Invoice", "Shipping", "Phishing" … `None` for a
/// category the person made. What a person reads is [`name_of`] the catalogue's entry, which may
/// carry their own name instead.
pub fn suite_label(category: Category) -> Option<&'static str> {
    english_label(category).map(|english| crate::i18n::trc(CONTEXT, english))
}

/// What a person reads for the category of `def`: the name they gave it, else the suite's word.
pub fn name_of(def: &CategoryDef) -> String {
    match def.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => name.to_string(),
        None => fallback_name(def.category),
    }
}

/// What a person reads for `category` when no catalogue is at hand: the suite's word, and for a
/// category the person made a plain "Category 7" – never the bare key.
pub fn fallback_name(category: Category) -> String {
    match (suite_label(category), category) {
        (Some(label), _) => label.to_string(),
        (None, Category::Custom(custom)) => crate::i18n::trf("Category {n}", &[("n", &custom.number().to_string())]),
        // Every category of the suite has a word ([`english_label`]).
        (None, _) => category.key().into_owned(),
    }
}

/// What a decision model reads about the category of `def`: the description the person wrote,
/// else the suite's own. Not translated: the person's words go as they are, the suite's are
/// English.
pub fn description_of(def: &CategoryDef) -> String {
    match def.description.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => text.to_string(),
        None => suite_description(def.category).unwrap_or_default().to_string(),
    }
}

/// The English word for `group`, untranslated; see [`english_label`].
pub(crate) fn english_group_label(group: CategoryGroup) -> &'static str {
    match group {
        CategoryGroup::WaitingForMe => "Waiting for me",
        CategoryGroup::PaperTrail => "Paper trail",
        CategoryGroup::Info => "Info",
        CategoryGroup::Junk => "Junk",
        CategoryGroup::Custom(_) => "Category",
    }
}

/// What a person reads for `group`: the four entries under "Categories" in the sidebar.
pub fn group_label(group: CategoryGroup) -> &'static str {
    crate::i18n::trc(CONTEXT, english_group_label(group))
}

/// What the category of the suite means, for a **decision model** – the `criteria` of the
/// `choice` question (task E12.3). English in every language: the model is prompted in English.
/// `None` for a category the person made; its description is theirs ([`description_of`]).
pub fn suite_description(category: Category) -> Option<&'static str> {
    Some(match category {
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
        Category::Custom(_) => return None,
    })
}

/// What a group means, for a decision model that is asked for the group first (task E15.4):
/// English, like [`suite_description`].
pub fn group_description(group: CategoryGroup) -> &'static str {
    match group {
        CategoryGroup::WaitingForMe => {
            "Something waits for me: a question, an assignment, a confirmation, a sign-in \
                                        code, or a message written to me by a person."
        }
        CategoryGroup::PaperTrail => {
            "A record to keep: an invoice, a receipt, an order, a shipment, a booking, a \
                                      statement or a document."
        }
        CategoryGroup::Info => {
            "To read when I like: a newsletter, an offer, a notice of a service, a social \
                                network or an event."
        }
        CategoryGroup::Junk => {
            "Technical mail and rubbish: an undeliverable mail, an automatic reply, spam, \
                                phishing, or nothing that fits."
        }
        CategoryGroup::Custom(_) => "Mail in this main category.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, trc_in};

    #[test]
    fn every_category_and_group_has_words_and_a_description() {
        for category in Category::BUILT_IN {
            assert!(!suite_label(category).unwrap_or_default().trim().is_empty(), "{category:?} has no label");
            let description = suite_description(category).unwrap_or_default();
            assert!(!description.trim().is_empty(), "{category:?} has no description");
            assert!(description.ends_with('.'), "{category:?}: a criterion is a sentence");
        }
        for group in CategoryGroup::ALL {
            assert!(!group_label(group).trim().is_empty(), "{group:?} has no label");
            assert!(group_description(group).ends_with('.'), "{group:?}: a criterion is a sentence");
        }
    }

    /// Two categories must never read the same, or the menu and the sidebar would be ambiguous.
    #[test]
    fn no_two_categories_read_alike_in_either_language() {
        for lang in [Lang::En, Lang::De] {
            let mut seen = std::collections::HashSet::new();
            for category in Category::BUILT_IN {
                let words = words_in(lang, category);
                assert!(seen.insert(words), "{category:?} reads like another category in {lang:?}: {words}");
            }
        }
    }

    /// The label of `category` in `lang`, independent of the process's own language.
    fn words_in(lang: Lang, category: Category) -> &'static str {
        trc_in(lang, CONTEXT, english_label(category).unwrap_or_default())
    }

    /// Task E15.1: the person's name and description win; left empty, the suite's words stand.
    /// A category the person made is never shown by its bare key.
    #[test]
    fn the_catalogue_entry_decides_what_a_category_is_called() {
        let mut def = CategoryDef::built_in(Category::Invoice).expect("built in");
        assert_eq!(name_of(&def), suite_label(Category::Invoice).unwrap_or_default());
        assert_eq!(description_of(&def), suite_description(Category::Invoice).unwrap_or_default());
        def.name = Some("  Bills  ".into());
        def.description = Some("Bills from the landlord, not from shops.".into());
        assert_eq!(name_of(&def), "Bills");
        assert_eq!(description_of(&def), "Bills from the landlord, not from shops.");
        def.name = Some("   ".into());
        assert_eq!(name_of(&def), suite_label(Category::Invoice).unwrap_or_default(), "blank is no name");
        let custom = Category::Custom(torro_mail::CustomCategory::new(7).expect("a number"));
        assert_eq!(suite_label(custom), None);
        assert_eq!(suite_description(custom), None);
        assert_eq!(fallback_name(custom), crate::i18n::trf("Category {n}", &[("n", &"7")]));
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
