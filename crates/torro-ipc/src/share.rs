//! Sharing items of the suite into another place (`docs/teilen.md`, issue #643): what a client names
//! to share ([`ShareItem`]) and what of it goes along ([`ShareField`]). The core makes the card from
//! its own store; a client never sends the text or the file of a card. The first place that takes
//! items is a chat (`chat.send_message` with `items`, stage 5 of `docs/chat.md`); the first kind is a
//! contact.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most items of one share.
pub const MAX_SHARE_ITEMS: usize = 20;

/// The most bytes of the file of one card (all its items together).
pub const MAX_CARD_BYTES: u64 = 5 * 1024 * 1024;

/// What kind of item is shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShareKind {
    /// A contact of an address book, by its id (`<book id>/<uid>`, as `contacts.list_contacts`
    /// names it). It goes as a short text and a lean vCard 3.0 with the chosen fields only.
    Contact,
}

/// One item to share: `{"kind": "contact", "id": "<contact id>"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ShareItem {
    pub kind: ShareKind,
    /// The item's id in its area.
    pub id: String,
}

impl ShareItem {
    /// The contact `id`.
    pub fn contact(id: &str) -> ShareItem {
        ShareItem { kind: ShareKind::Contact, id: id.to_string() }
    }
}

/// What of a contact goes along on its card. The name always goes. Without a choice the core takes
/// [`ShareField::DEFAULT`]: what people hand out on a business card, nothing private.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShareField {
    /// Company and position.
    Organization,
    /// Every phone number.
    Phones,
    /// Every mail address.
    Emails,
    /// Every address that is not marked as home.
    WorkAddresses,
    /// Addresses marked as home.
    PrivateAddresses,
    /// Web pages.
    Web,
    Birthday,
    Note,
    /// Messenger handles.
    Messengers,
    /// The contact's groups.
    Groups,
}

impl ShareField {
    /// Every field, in the order cards and approvals name them.
    pub const ALL: [ShareField; 10] = [
        ShareField::Organization,
        ShareField::Phones,
        ShareField::Emails,
        ShareField::WorkAddresses,
        ShareField::PrivateAddresses,
        ShareField::Web,
        ShareField::Birthday,
        ShareField::Note,
        ShareField::Messengers,
        ShareField::Groups,
    ];

    /// What goes when nobody chose (`docs/teilen.md`, decision 4).
    pub const DEFAULT: [ShareField; 5] =
        [ShareField::Organization, ShareField::Phones, ShareField::Emails, ShareField::WorkAddresses, ShareField::Web];

    /// Whether the field goes without being chosen.
    pub fn is_default(self) -> bool {
        ShareField::DEFAULT.contains(&self)
    }

    /// The fields of `chosen`, or [`ShareField::DEFAULT`] without a choice; sorted, each once.
    pub fn chosen(chosen: Option<&[ShareField]>) -> Vec<ShareField> {
        let mut fields = chosen.map_or_else(|| ShareField::DEFAULT.to_vec(), <[ShareField]>::to_vec);
        fields.sort();
        fields.dedup();
        fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_and_fields_read_as_the_concept_writes_them() {
        let item: ShareItem =
            serde_json::from_str(r#"{"kind":"contact","id":"nextcloud:anna@cloud.example/contacts/u1"}"#)
                .expect("item");
        assert_eq!(item, ShareItem::contact("nextcloud:anna@cloud.example/contacts/u1"));
        let fields: Vec<ShareField> = serde_json::from_str(r#"["private_addresses","phones"]"#).expect("fields");
        assert_eq!(fields, [ShareField::PrivateAddresses, ShareField::Phones]);
    }

    #[test]
    fn without_a_choice_only_what_a_business_card_holds_goes() {
        assert_eq!(ShareField::chosen(None), ShareField::DEFAULT.to_vec());
        assert!(!ShareField::chosen(None).contains(&ShareField::PrivateAddresses));
        let chosen = ShareField::chosen(Some(&[ShareField::Note, ShareField::Phones, ShareField::Note]));
        assert_eq!(chosen, [ShareField::Phones, ShareField::Note]);
        assert!(ShareField::ALL.iter().filter(|field| field.is_default()).count() == ShareField::DEFAULT.len());
    }
}
