//! What a piece of mail *is* – invoice, shipping, promotion, sign-in code, phishing (task E12.1,
//! concept `docs/kategorien.md`).
//!
//! The lane of the inbox says **whether** something waits for the person; the category says
//! **what** it is. They are two dimensions: a category changes no lane, no counter and no
//! notification.
//!
//! This module is data only. The words a person reads live in `torro_core::category`, because
//! `torro-core` depends on this crate and not the other way round – a `tr()` here would be a
//! dependency cycle.
//!
//! Only mail carries a category. Entries of GitHub, Talk and ntfy leave it empty: their facets
//! (`reason`, `result`, `kind`) tell more, and a model call would be wasted there.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The four groups of [`Category`], in the order the sidebar lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CategoryGroup {
    /// Something waits for the person: a question, an assignment, a sign-in code.
    WaitingForMe,
    /// Nothing to do now, but needed later – tax, a complaint, bookkeeping.
    PaperTrail,
    /// To read when the person wants: newsletters, offers, service notices.
    Info,
    /// Technical mail and rubbish: bounces, out-of-office, spam, phishing.
    Junk,
}

impl CategoryGroup {
    /// Every group, in the order of the catalogue.
    pub const ALL: [CategoryGroup; 4] =
        [CategoryGroup::WaitingForMe, CategoryGroup::PaperTrail, CategoryGroup::Info, CategoryGroup::Junk];

    /// How the group is written on the wire and in the API: `waiting_for_me`, `paper_trail`,
    /// `info`, `junk` (the serde name).
    pub fn key(self) -> &'static str {
        match self {
            CategoryGroup::WaitingForMe => "waiting_for_me",
            CategoryGroup::PaperTrail => "paper_trail",
            CategoryGroup::Info => "info",
            CategoryGroup::Junk => "junk",
        }
    }

    /// The categories of this group, in the order of the catalogue.
    pub fn categories(self) -> Vec<Category> {
        Category::ALL.into_iter().filter(|category| category.group() == self).collect()
    }
}

/// What a piece of mail is. The English key is what travels; the words a person reads come from
/// `torro_core::category::label`.
///
/// Three of them follow from the headers alone and are never asked of a model
/// ([`Category::by_rule_only`]); the other 20 are the options of the one `choice` question
/// ([`Category::BY_MODEL`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// A question or request addressed to the person; an answer is expected.
    Request,
    /// An assignment or commitment with a deadline.
    Task,
    /// A signature, confirmation or authorisation is wanted.
    Approval,
    /// A sign-in code, two-factor prompt or password reset: short-lived and pressing.
    Security,
    /// A message written by a person, with nothing to do.
    Personal,
    /// An invitation to a calendar event. From the headers (iMIP `text/calendar`).
    Invitation,
    /// An invoice, reminder to pay or payment demand.
    Invoice,
    /// A payment confirmed: a receipt or proof of purchase.
    Receipt,
    /// An order or purchase confirmation.
    Order,
    /// A shipping confirmation, tracking update or delivery attempt.
    Shipping,
    /// A booking: travel, a hotel, a ticket, an appointment.
    Booking,
    /// A bank statement, payslip or annual statement.
    Statement,
    /// A contract, policy, notice of termination or letter from an authority.
    Document,
    /// Editorial content, a subscription, a blog post.
    Newsletter,
    /// An offer, a discount, product advertising.
    Promotion,
    /// Maintenance, changed terms, a status report of a service.
    ServiceNotice,
    /// A social network, forum or community.
    Social,
    /// An invitation to a webinar, conference or other event – not a calendar invitation.
    EventInvite,
    /// A delivery status notification: the mail could not be delivered. From the headers
    /// (task E9.3a brings the detection).
    Bounce,
    /// An automatic reply, such as an out-of-office notice. From the headers (`Auto-Submitted`).
    AutoReply,
    /// Bulk mail with no connection to the person.
    Spam,
    /// An attempt to deceive: a forged sender, a harmful link. Warned about on its own, not only
    /// as spam.
    Phishing,
    /// Fits nothing else, or the model was too unsure.
    Other,
}

impl Category {
    /// Every category, in the order of the catalogue (`docs/kategorien.md`), grouped as
    /// [`Category::group`] says.
    pub const ALL: [Category; 23] = [
        // Waiting for me
        Category::Request,
        Category::Task,
        Category::Approval,
        Category::Security,
        Category::Personal,
        Category::Invitation,
        // Paper trail
        Category::Invoice,
        Category::Receipt,
        Category::Order,
        Category::Shipping,
        Category::Booking,
        Category::Statement,
        Category::Document,
        // Info
        Category::Newsletter,
        Category::Promotion,
        Category::ServiceNotice,
        Category::Social,
        Category::EventInvite,
        // Junk
        Category::Bounce,
        Category::AutoReply,
        Category::Spam,
        Category::Phishing,
        Category::Other,
    ];

    /// The categories a decision model chooses between: [`Category::ALL`] without the three that
    /// follow from the headers ([`Category::by_rule_only`]), in the order of the catalogue.
    ///
    /// Exactly 20, which is also the narrowest documented limit of a `choice` question (the Jev
    /// reference; Ollama allows 26), so the whole catalogue fits into **one** call.
    pub const BY_MODEL: [Category; 20] = [
        Category::Request,
        Category::Task,
        Category::Approval,
        Category::Security,
        Category::Personal,
        Category::Invoice,
        Category::Receipt,
        Category::Order,
        Category::Shipping,
        Category::Booking,
        Category::Statement,
        Category::Document,
        Category::Newsletter,
        Category::Promotion,
        Category::ServiceNotice,
        Category::Social,
        Category::EventInvite,
        Category::Spam,
        Category::Phishing,
        Category::Other,
    ];

    /// How the category is written on the wire and in the API (the serde name): `request`,
    /// `service_notice`, `auto_reply` …
    pub fn key(self) -> &'static str {
        match self {
            Category::Request => "request",
            Category::Task => "task",
            Category::Approval => "approval",
            Category::Security => "security",
            Category::Personal => "personal",
            Category::Invitation => "invitation",
            Category::Invoice => "invoice",
            Category::Receipt => "receipt",
            Category::Order => "order",
            Category::Shipping => "shipping",
            Category::Booking => "booking",
            Category::Statement => "statement",
            Category::Document => "document",
            Category::Newsletter => "newsletter",
            Category::Promotion => "promotion",
            Category::ServiceNotice => "service_notice",
            Category::Social => "social",
            Category::EventInvite => "event_invite",
            Category::Bounce => "bounce",
            Category::AutoReply => "auto_reply",
            Category::Spam => "spam",
            Category::Phishing => "phishing",
            Category::Other => "other",
        }
    }

    /// The category named by `key` ([`Category::key`]); `None` for a word no category has.
    pub fn from_key(key: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|category| category.key() == key)
    }

    /// Which group the category belongs to.
    pub fn group(self) -> CategoryGroup {
        match self {
            Category::Request
            | Category::Task
            | Category::Approval
            | Category::Security
            | Category::Personal
            | Category::Invitation => CategoryGroup::WaitingForMe,
            Category::Invoice
            | Category::Receipt
            | Category::Order
            | Category::Shipping
            | Category::Booking
            | Category::Statement
            | Category::Document => CategoryGroup::PaperTrail,
            Category::Newsletter
            | Category::Promotion
            | Category::ServiceNotice
            | Category::Social
            | Category::EventInvite => CategoryGroup::Info,
            Category::Bounce | Category::AutoReply | Category::Spam | Category::Phishing | Category::Other => {
                CategoryGroup::Junk
            }
        }
    }

    /// The category follows from the headers alone and is never asked of a model: an invitation
    /// (iMIP), a bounce (DSN) and an automatic reply (`Auto-Submitted`). These are not among
    /// [`Category::BY_MODEL`].
    pub fn by_rule_only(self) -> bool {
        matches!(self, Category::Invitation | Category::Bounce | Category::AutoReply)
    }
}

/// Where a mail's category came from ([`MailCategory::source`]).
///
/// The app shows a category of a model as a suggestion and every other one plainly
/// (`docs/ki.md`, principle 5: an AI result is marked as such).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CategorySource {
    /// From a header, the folder it lies in, or a rule the person kept for a sender or domain.
    Rule,
    /// From the decision model: a suggestion, with [`MailCategory::confidence`].
    Model,
    /// The person set it by hand. Filing new mail never overwrites it; only an explicit
    /// `inbox.classify_item` does.
    Person,
}

/// The category of one piece of mail, with where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct MailCategory {
    pub category: Category,
    pub source: CategorySource,
    /// How sure the model was, in **percent** (0–100); `None` for every other source.
    ///
    /// Percent, not the protocol's fraction, because [`MailCategory`] is part of types that
    /// derive `Eq` (`MessageSummary`, `torro_inbox::Item`) and `f32` is not `Eq`. The threshold
    /// is compared on the model's own fraction before it is rounded here, so nothing is decided
    /// on the rounded value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
}

impl MailCategory {
    /// A category that follows from a header, the folder or a kept rule.
    pub fn by_rule(category: Category) -> MailCategory {
        MailCategory { category, source: CategorySource::Rule, confidence: None }
    }

    /// A category the person set by hand.
    pub fn by_person(category: Category) -> MailCategory {
        MailCategory { category, source: CategorySource::Person, confidence: None }
    }

    /// A category the decision model chose, with the confidence it reported as a fraction of 1.
    /// The fraction is kept as percent, clamped to 0–100; a value that is not a number becomes 0.
    pub fn by_model(category: Category, confidence: f32) -> MailCategory {
        let percent = if confidence.is_nan() { 0.0 } else { (confidence * 100.0).round().clamp(0.0, 100.0) };
        MailCategory { category, source: CategorySource::Model, confidence: Some(percent as u8) }
    }

    /// The category is a suggestion of a model, so the app marks it as one.
    pub fn is_suggestion(self) -> bool {
        self.source == CategorySource::Model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_is_written_as_its_key_and_read_back() {
        for category in Category::ALL {
            let json = serde_json::to_value(category).expect("json");
            assert_eq!(json, serde_json::json!(category.key()), "{category:?}");
            assert_eq!(serde_json::from_value::<Category>(json).expect("back"), category);
            assert_eq!(Category::from_key(category.key()), Some(category));
        }
        assert_eq!(Category::from_key("no such category"), None);
        for group in CategoryGroup::ALL {
            let json = serde_json::to_value(group).expect("json");
            assert_eq!(json, serde_json::json!(group.key()), "{group:?}");
        }
        for source in [CategorySource::Rule, CategorySource::Model, CategorySource::Person] {
            let json = serde_json::to_value(source).expect("json");
            assert!(json.is_string(), "{source:?}");
        }
    }

    #[test]
    fn the_catalogue_has_twenty_three_categories_split_over_four_groups_without_a_duplicate() {
        assert_eq!(Category::ALL.len(), 23);
        let mut seen = std::collections::HashSet::new();
        for category in Category::ALL {
            assert!(seen.insert(category.key()), "duplicate key {}", category.key());
        }
        let mut from_groups: Vec<Category> = Vec::new();
        for group in CategoryGroup::ALL {
            let of_group = group.categories();
            assert!(!of_group.is_empty(), "{group:?} is empty");
            assert!(of_group.iter().all(|category| category.group() == group), "{group:?}");
            from_groups.extend(of_group);
        }
        assert_eq!(from_groups, Category::ALL.to_vec(), "the groups together are the catalogue, in its order");
    }

    #[test]
    fn exactly_twenty_categories_are_left_to_the_model() {
        assert_eq!(Category::BY_MODEL.len(), 20);
        for category in Category::BY_MODEL {
            assert!(!category.by_rule_only(), "{category:?} follows from the headers");
        }
        for by_rule in [Category::Invitation, Category::Bounce, Category::AutoReply] {
            assert!(by_rule.by_rule_only(), "{by_rule:?}");
            assert!(!Category::BY_MODEL.contains(&by_rule), "{by_rule:?} is no option of the question");
        }
        let left_out: Vec<Category> = Category::ALL.into_iter().filter(|c| !Category::BY_MODEL.contains(c)).collect();
        assert_eq!(left_out, [Category::Invitation, Category::Bounce, Category::AutoReply]);
        let in_catalogue_order: Vec<Category> = Category::ALL.into_iter().filter(|c| !c.by_rule_only()).collect();
        assert_eq!(in_catalogue_order, Category::BY_MODEL.to_vec(), "BY_MODEL keeps the catalogue's order");
    }

    #[test]
    fn a_models_confidence_is_kept_as_percent_and_only_a_model_has_one() {
        assert_eq!(MailCategory::by_model(Category::Invoice, 0.93).confidence, Some(93));
        assert_eq!(MailCategory::by_model(Category::Invoice, 0.0).confidence, Some(0));
        assert_eq!(MailCategory::by_model(Category::Invoice, 1.0).confidence, Some(100));
        assert_eq!(MailCategory::by_model(Category::Invoice, 2.5).confidence, Some(100), "clamped");
        assert_eq!(MailCategory::by_model(Category::Invoice, -1.0).confidence, Some(0), "clamped");
        assert_eq!(MailCategory::by_model(Category::Invoice, f32::NAN).confidence, Some(0));
        assert_eq!(MailCategory::by_rule(Category::Invitation).confidence, None);
        assert_eq!(MailCategory::by_person(Category::Invoice).confidence, None);
        assert!(MailCategory::by_model(Category::Invoice, 0.9).is_suggestion());
        assert!(!MailCategory::by_rule(Category::Invitation).is_suggestion());
        assert!(!MailCategory::by_person(Category::Invoice).is_suggestion());
    }

    #[test]
    fn a_category_without_a_confidence_leaves_the_field_out() {
        let json = serde_json::to_value(MailCategory::by_rule(Category::Invitation)).expect("json");
        assert_eq!(json, serde_json::json!({"category": "invitation", "source": "rule"}));
        let json = serde_json::to_value(MailCategory::by_model(Category::Shipping, 0.97)).expect("json");
        assert_eq!(json, serde_json::json!({"category": "shipping", "source": "model", "confidence": 97}));
    }
}
