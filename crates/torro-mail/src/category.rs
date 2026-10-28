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
//!
//! **The catalogue can be edited** (task E15.1, issue #255): the 23 categories the suite brings
//! ([`Category::BUILT_IN`]) are its default, the person adds their own ([`Category::Custom`]) and
//! changes name, group, description and whether a model may choose one ([`CategoryDef`]). A
//! category's **key never changes** – renaming one keeps every mail and rule that names it valid.
//! So what a category is called and which group it is in comes from the catalogue the core keeps,
//! never from the key alone ([`Category::default_group`] is only the suite's default).

use std::borrow::Cow;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The four built-in main categories and any the person creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CategoryGroup {
    /// Something waits for the person: a question, an assignment, a sign-in code.
    WaitingForMe,
    /// Nothing to do now, but needed later – tax, a complaint, bookkeeping.
    PaperTrail,
    /// To read when the person wants: newsletters, offers, service notices.
    Info,
    /// Technical mail and rubbish: bounces, out-of-office, spam, phishing.
    Junk,
    /// A main category created by the person. The number is never reused.
    Custom(u32),
}

/// The monochrome icon shown for a mail category. Its colour comes from the category's group.
///
/// These are stable catalogue values, separate from the GUI's SVG implementation, so category
/// choices can travel over IPC and remain stored if the icon set is rearranged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CategoryIcon {
    MessageCircle,
    ListTodo,
    CircleCheck,
    UserCheck,
    ShieldAlert,
    User,
    CalendarPlus,
    TextQuote,
    Paperclip,
    SquareCheck,
    Send,
    CalendarDays,
    ChartColumn,
    AlignLeft,
    Mail,
    Flag,
    Bell,
    Users,
    Video,
    MailOpen,
    Reply,
    Trash2,
    Eye,
    Ellipsis,
    #[default]
    Tag,
}

impl CategoryIcon {
    /// Every icon a category can use, in the order shown by the icon picker.
    pub const ALL: [CategoryIcon; 25] = [
        Self::MessageCircle,
        Self::ListTodo,
        Self::CircleCheck,
        Self::UserCheck,
        Self::ShieldAlert,
        Self::User,
        Self::CalendarPlus,
        Self::TextQuote,
        Self::Paperclip,
        Self::SquareCheck,
        Self::Send,
        Self::CalendarDays,
        Self::ChartColumn,
        Self::AlignLeft,
        Self::Mail,
        Self::Flag,
        Self::Bell,
        Self::Users,
        Self::Video,
        Self::MailOpen,
        Self::Reply,
        Self::Trash2,
        Self::Eye,
        Self::Ellipsis,
        Self::Tag,
    ];

    /// Stable snake-case key used by the catalogue database and APIs.
    pub fn key(self) -> &'static str {
        match self {
            Self::MessageCircle => "message_circle",
            Self::ListTodo => "list_todo",
            Self::CircleCheck => "circle_check",
            Self::UserCheck => "user_check",
            Self::ShieldAlert => "shield_alert",
            Self::User => "user",
            Self::CalendarPlus => "calendar_plus",
            Self::TextQuote => "text_quote",
            Self::Paperclip => "paperclip",
            Self::SquareCheck => "square_check",
            Self::Send => "send",
            Self::CalendarDays => "calendar_days",
            Self::ChartColumn => "chart_column",
            Self::AlignLeft => "align_left",
            Self::Mail => "mail",
            Self::Flag => "flag",
            Self::Bell => "bell",
            Self::Users => "users",
            Self::Video => "video",
            Self::MailOpen => "mail_open",
            Self::Reply => "reply",
            Self::Trash2 => "trash2",
            Self::Eye => "eye",
            Self::Ellipsis => "ellipsis",
            Self::Tag => "tag",
        }
    }

    /// The icon named by `key`; unknown stored values fall back to the neutral tag.
    pub fn from_key(key: &str) -> CategoryIcon {
        Self::ALL.into_iter().find(|icon| icon.key() == key).unwrap_or_default()
    }
}

impl CategoryGroup {
    /// The built-in groups, in the order of the catalogue. User groups follow them in the store.
    pub const ALL: [CategoryGroup; 4] =
        [CategoryGroup::WaitingForMe, CategoryGroup::PaperTrail, CategoryGroup::Info, CategoryGroup::Junk];

    /// How the group is written on the wire and in the API: `waiting_for_me`, `paper_trail`,
    /// `info`, `junk` (the serde name).
    pub fn key(self) -> Cow<'static, str> {
        match self {
            CategoryGroup::WaitingForMe => Cow::Borrowed("waiting_for_me"),
            CategoryGroup::PaperTrail => Cow::Borrowed("paper_trail"),
            CategoryGroup::Info => Cow::Borrowed("info"),
            CategoryGroup::Junk => Cow::Borrowed("junk"),
            CategoryGroup::Custom(id) => Cow::Owned(format!("custom_group_{id}")),
        }
    }

    /// The group named by `key` ([`CategoryGroup::key`]); `None` for a word no group has.
    pub fn from_key(key: &str) -> Option<CategoryGroup> {
        CategoryGroup::ALL.into_iter().find(|group| group.key() == key).or_else(|| {
            key.strip_prefix("custom_group_").and_then(|number| {
                let id = number.parse::<u32>().ok()?;
                (id >= 5 && id.to_string() == number).then_some(CategoryGroup::Custom(id))
            })
        })
    }

    /// The categories the suite puts in this group by default, in the order of its catalogue.
    /// What a group holds now is the core's catalogue's word.
    pub fn default_categories(self) -> Vec<Category> {
        Category::BUILT_IN.into_iter().filter(|category| category.default_group() == Some(self)).collect()
    }

    /// The row number of a built-in main category. Custom rows start after these four.
    pub fn database_id(self) -> u32 {
        match self {
            Self::WaitingForMe => 1,
            Self::PaperTrail => 2,
            Self::Info => 3,
            Self::Junk => 4,
            Self::Custom(id) => id,
        }
    }

    pub fn from_database_id(id: u32) -> Option<Self> {
        match id {
            1 => Some(Self::WaitingForMe),
            2 => Some(Self::PaperTrail),
            3 => Some(Self::Info),
            4 => Some(Self::Junk),
            5.. => Some(Self::Custom(id)),
            _ => None,
        }
    }
}

/// One editable main category. `group` is stable when its name, icon or colour changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryGroupDef {
    pub group: CategoryGroup,
    /// `None` keeps the translated suite name of a built-in group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub icon: CategoryIcon,
    /// Index into the four colours of the active design, so it follows theme changes.
    pub color: u8,
}

impl CategoryGroupDef {
    pub fn built_in(group: CategoryGroup) -> Option<Self> {
        let (icon, color) = match group {
            CategoryGroup::WaitingForMe => (CategoryIcon::UserCheck, 0),
            CategoryGroup::PaperTrail => (CategoryIcon::TextQuote, 1),
            CategoryGroup::Info => (CategoryIcon::Bell, 2),
            CategoryGroup::Junk => (CategoryIcon::ShieldAlert, 3),
            CategoryGroup::Custom(_) => return None,
        };
        Some(Self { group, name: None, icon, color })
    }

    pub fn defaults() -> Vec<Self> {
        CategoryGroup::ALL.into_iter().filter_map(Self::built_in).collect()
    }
}

impl Serialize for CategoryGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for CategoryGroup {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Self::from_key(&key).ok_or_else(|| serde::de::Error::custom("unknown category group"))
    }
}

impl JsonSchema for CategoryGroup {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "CategoryGroup".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A main mail category: a built-in key or `custom_group_<n>` for one created by the person.",
            "anyOf": [
                { "type": "string", "enum": ["waiting_for_me", "paper_trail", "info", "junk"] },
                { "type": "string", "pattern": "^custom_group_[1-9][0-9]{0,9}$" }
            ]
        })
    }
}

/// What a piece of mail is. The English key is what travels ([`Category::key`]); the words a
/// person reads come from the catalogue (`torro_core::category::name_of`).
///
/// The suite brings 23 ([`Category::BUILT_IN`]); three of them follow from the headers alone and
/// are by default never asked of a model ([`Category::by_rule_only`]). Categories the person made
/// are [`Category::Custom`], written `custom_<n>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    /// A category the person made (task E15.1): what it is called, its group and what it means
    /// are in the catalogue.
    Custom(CustomCategory),
}

/// The number of a category the person made ([`Category::Custom`]), written `custom_<n>`.
///
/// Given out once: the core never hands a number out again, also not after the category was
/// removed, so an old reference can never come to mean another category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CustomCategory(u32);

impl CustomCategory {
    /// The category numbered `number`; `None` for 0, which no category has.
    pub fn new(number: u32) -> Option<CustomCategory> {
        (number > 0).then_some(CustomCategory(number))
    }

    /// Its number, as in `custom_<n>`.
    pub fn number(self) -> u32 {
        self.0
    }
}

/// How a category the person made starts on the wire: `custom_<n>`. No category of the suite
/// starts like this (a test checks), so the two can never be mixed up.
pub const CUSTOM_PREFIX: &str = "custom_";

impl Category {
    /// The suite's stable icon for a built-in category; custom categories start with a tag.
    pub fn default_icon(self) -> CategoryIcon {
        match self {
            Self::Request => CategoryIcon::MessageCircle,
            Self::Task => CategoryIcon::ListTodo,
            Self::Approval => CategoryIcon::CircleCheck,
            Self::Security => CategoryIcon::ShieldAlert,
            Self::Personal => CategoryIcon::User,
            Self::Invitation => CategoryIcon::CalendarPlus,
            Self::Invoice => CategoryIcon::TextQuote,
            Self::Receipt => CategoryIcon::Paperclip,
            Self::Order => CategoryIcon::SquareCheck,
            Self::Shipping => CategoryIcon::Send,
            Self::Booking => CategoryIcon::CalendarDays,
            Self::Statement => CategoryIcon::ChartColumn,
            Self::Document => CategoryIcon::AlignLeft,
            Self::Newsletter => CategoryIcon::Mail,
            Self::Promotion => CategoryIcon::Flag,
            Self::ServiceNotice => CategoryIcon::Bell,
            Self::Social => CategoryIcon::Users,
            Self::EventInvite => CategoryIcon::Video,
            Self::Bounce => CategoryIcon::MailOpen,
            Self::AutoReply => CategoryIcon::Reply,
            Self::Spam => CategoryIcon::Trash2,
            Self::Phishing => CategoryIcon::Eye,
            Self::Other => CategoryIcon::Ellipsis,
            Self::Custom(_) => CategoryIcon::Tag,
        }
    }

    /// The categories the suite brings, in the order of its catalogue (`docs/kategorien.md`),
    /// grouped as [`Category::default_group`] says. The person may add their own and move any of
    /// them to another group; the core's catalogue says what holds now.
    pub const BUILT_IN: [Category; 23] = [
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

    /// The categories of the suite a decision model chooses between by default:
    /// [`Category::BUILT_IN`] without the three that follow from the headers
    /// ([`Category::by_rule_only`]), in the order of the catalogue.
    ///
    /// Exactly 20, which is also the narrowest documented limit of a `choice` question (the Jev
    /// reference; Ollama allows 26), so the default catalogue fits into **one** call. The person
    /// can change which ones a model may choose ([`CategoryDef::model`]).
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
    /// `service_notice`, `auto_reply` … and `custom_<n>` for one the person made.
    pub fn key(self) -> Cow<'static, str> {
        match self {
            Category::Custom(custom) => Cow::Owned(format!("{CUSTOM_PREFIX}{}", custom.number())),
            built_in => Cow::Borrowed(built_in.built_in_key().unwrap_or_default()),
        }
    }

    /// The key of a category of the suite; `None` for one the person made.
    fn built_in_key(self) -> Option<&'static str> {
        Some(match self {
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
            Category::Custom(_) => return None,
        })
    }

    /// The category named by `key` ([`Category::key`]); `None` for a word no category has.
    ///
    /// `custom_<n>` is read for any `n` above 0, written without leading zeros, so every
    /// category has exactly one way to be written. Whether the catalogue holds it is the core's
    /// to say.
    pub fn from_key(key: &str) -> Option<Category> {
        if let Some(number) = key.strip_prefix(CUSTOM_PREFIX) {
            let canonical =
                !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) && !number.starts_with('0');
            return canonical
                .then(|| number.parse().ok().and_then(CustomCategory::new))
                .flatten()
                .map(Category::Custom);
        }
        Category::BUILT_IN.into_iter().find(|category| category.built_in_key() == Some(key))
    }

    /// The category is one the suite brings, not one the person made.
    pub fn is_built_in(self) -> bool {
        !matches!(self, Category::Custom(_))
    }

    /// Which group the suite puts the category in; `None` for one the person made, which has
    /// only the group the catalogue gives it. The person can move any category, so what holds
    /// now is the catalogue's word ([`CategoryDef::group`]), not this.
    pub fn default_group(self) -> Option<CategoryGroup> {
        Some(match self {
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
            Category::Custom(_) => return None,
        })
    }

    /// The category follows from the headers alone and is never asked of a model: an invitation
    /// (iMIP), a bounce (DSN) and an automatic reply (`Auto-Submitted`). These are not among
    /// [`Category::BY_MODEL`].
    pub fn by_rule_only(self) -> bool {
        matches!(self, Category::Invitation | Category::Bounce | Category::AutoReply)
    }
}

impl Serialize for Category {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for Category {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Category, D::Error> {
        let key = Cow::<'de, str>::deserialize(deserializer)?;
        Category::from_key(&key).ok_or_else(|| serde::de::Error::custom(format!("unknown category `{key}`")))
    }
}

impl JsonSchema for Category {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Category".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let built_in: Vec<&'static str> = Category::BUILT_IN.into_iter().filter_map(Category::built_in_key).collect();
        schemars::json_schema!({
            "description": "A mail category: one of the suite's keys, or `custom_<n>` for one the person made \
                            (`inbox.list_categories` lists the catalogue).",
            "anyOf": [
                { "type": "string", "enum": built_in },
                { "type": "string", "pattern": "^custom_[1-9][0-9]{0,9}$" }
            ]
        })
    }
}

/// The most characters of a category's name ([`CategoryDef::name`]).
pub const NAME_MAX: usize = 60;

/// The most characters of a category's description ([`CategoryDef::description`]): enough for
/// what it is, a few examples and exceptions, and short enough that 20 of them fit into one call
/// to a decision model.
pub const DESCRIPTION_MAX: usize = 500;

/// The most categories of one group a decision model may choose between (task E15.4): the
/// narrowest documented limit of a `choice` question. With more than this over all groups the
/// model is asked for the group first, so one group must never hold more.
pub const MODEL_PER_GROUP_MAX: usize = 20;

/// One category of the catalogue as the core keeps it (task E15.1, issue #255): its key, which
/// never changes, and what the person may change about it.
///
/// For a category of the suite, `name` and `description` left `None` mean the suite's own words
/// (translated name, English description); a category the person made always has both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryDef {
    pub category: Category,
    /// The group it stands in now.
    pub group: CategoryGroup,
    /// What the person calls it; `None`: the suite's word for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What it means, as a decision model reads it – with examples and exceptions; `None`: the
    /// suite's description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A decision model may choose it. Off, only a rule or the person files mail under it.
    pub model: bool,
    /// The monochrome icon shown for this category; its group supplies the colour.
    #[serde(default = "CategoryIcon::default")]
    pub icon: CategoryIcon,
}

impl CategoryDef {
    /// The suite's own entry for `category`, as nobody changed it; `None` for a category the
    /// person made, which has no default.
    pub fn built_in(category: Category) -> Option<CategoryDef> {
        Some(CategoryDef {
            category,
            group: category.default_group()?,
            name: None,
            description: None,
            model: Category::BY_MODEL.contains(&category),
            icon: category.default_icon(),
        })
    }

    /// The catalogue as the suite brings it, in its order.
    pub fn defaults() -> Vec<CategoryDef> {
        Category::BUILT_IN.into_iter().filter_map(CategoryDef::built_in).collect()
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
        for category in Category::BUILT_IN {
            let json = serde_json::to_value(category).expect("json");
            assert_eq!(json, serde_json::json!(category.key()), "{category:?}");
            assert_eq!(serde_json::from_value::<Category>(json).expect("back"), category);
            assert_eq!(Category::from_key(&category.key()), Some(category));
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
    fn custom_main_category_ids_round_trip_without_aliasing_built_ins() {
        for id in [5, 42, u32::MAX] {
            let group = CategoryGroup::Custom(id);
            let json = serde_json::to_value(group).expect("serialize main category");
            assert_eq!(serde_json::from_value::<CategoryGroup>(json).expect("read main category"), group);
            assert_eq!(CategoryGroup::from_database_id(id), Some(group));
        }
        for key in ["custom_group_0", "custom_group_1", "custom_group_04", "custom_group_5x", "custom_group_4294967296"]
        {
            assert_eq!(CategoryGroup::from_key(key), None, "{key}");
        }
    }

    #[test]
    fn the_catalogue_has_twenty_three_categories_split_over_four_groups_without_a_duplicate() {
        assert_eq!(Category::BUILT_IN.len(), 23);
        let mut seen = std::collections::HashSet::new();
        for category in Category::BUILT_IN {
            assert!(seen.insert(category.key()), "duplicate key {}", category.key());
            assert!(!category.key().starts_with(CUSTOM_PREFIX), "{category:?} reads like a custom category");
            assert!(category.is_built_in());
        }
        let mut from_groups: Vec<Category> = Vec::new();
        for group in CategoryGroup::ALL {
            let of_group = group.default_categories();
            assert!(!of_group.is_empty(), "{group:?} is empty");
            assert!(of_group.iter().all(|category| category.default_group() == Some(group)), "{group:?}");
            from_groups.extend(of_group);
        }
        assert_eq!(from_groups, Category::BUILT_IN.to_vec(), "the groups together are the catalogue, in its order");
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
        let left_out: Vec<Category> =
            Category::BUILT_IN.into_iter().filter(|c| !Category::BY_MODEL.contains(c)).collect();
        assert_eq!(left_out, [Category::Invitation, Category::Bounce, Category::AutoReply]);
        let in_catalogue_order: Vec<Category> = Category::BUILT_IN.into_iter().filter(|c| !c.by_rule_only()).collect();
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

    /// Task E15.1: a category the person made is written `custom_<n>`, read back the same, and
    /// has exactly one spelling.
    #[test]
    fn a_custom_category_is_written_with_its_number_and_read_back() {
        let custom = Category::Custom(CustomCategory::new(7).expect("a number"));
        assert_eq!(custom.key(), "custom_7");
        let json = serde_json::to_value(custom).expect("json");
        assert_eq!(json, serde_json::json!("custom_7"));
        assert_eq!(serde_json::from_value::<Category>(json).expect("back"), custom);
        assert_eq!(Category::from_key("custom_7"), Some(custom));
        assert!(!custom.is_built_in());
        assert_eq!(custom.default_group(), None, "only the catalogue knows its group");
        assert!(!custom.by_rule_only());
        assert_eq!(CategoryDef::built_in(custom), None);
        for wrong in ["custom_0", "custom_", "custom_x", "custom_07", "custom_-1", "custom_99999999999", "Custom_7"] {
            assert_eq!(Category::from_key(wrong), None, "{wrong}");
            assert!(serde_json::from_value::<Category>(serde_json::json!(wrong)).is_err(), "{wrong}");
        }
        assert_eq!(CustomCategory::new(0), None);
    }

    /// Task E15.1: the suite's default catalogue keeps every category in its group, and a model
    /// may choose exactly the 20 it chose before.
    #[test]
    fn the_default_catalogue_is_the_catalogue_of_the_suite() {
        let defaults = CategoryDef::defaults();
        assert_eq!(defaults.iter().map(|def| def.category).collect::<Vec<_>>(), Category::BUILT_IN.to_vec());
        let icons: std::collections::HashSet<CategoryIcon> = defaults.iter().map(|def| def.icon).collect();
        assert_eq!(icons.len(), defaults.len(), "every built-in category has its own icon");
        for def in &defaults {
            assert_eq!(Some(def.group), def.category.default_group());
            assert_eq!((def.name.as_deref(), def.description.as_deref()), (None, None));
            assert_eq!(def.model, Category::BY_MODEL.contains(&def.category), "{:?}", def.category);
            assert_eq!(CategoryIcon::from_key(def.icon.key()), def.icon, "{:?}", def.category);
        }
        let custom = Category::Custom(CustomCategory::new(1).expect("a number"));
        assert_eq!(custom.default_icon(), CategoryIcon::Tag);
        for group in CategoryGroup::ALL {
            let by_model = defaults.iter().filter(|def| def.group == group && def.model).count();
            assert!(by_model <= MODEL_PER_GROUP_MAX, "{group:?}");
        }
    }

    #[test]
    fn category_icon_keys_are_stable_in_json_and_the_catalogue_store() {
        for icon in CategoryIcon::ALL {
            assert_eq!(serde_json::to_value(icon).expect("json"), serde_json::json!(icon.key()));
            assert_eq!(CategoryIcon::from_key(icon.key()), icon);
        }
        assert_eq!(CategoryIcon::from_key("a_future_icon"), CategoryIcon::Tag);
    }
}
