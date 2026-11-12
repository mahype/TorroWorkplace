//! Sharing into a chat (stage 5 of `docs/chat.md`, TL2 of `docs/teilen.md`): `chat.send_message`
//! with `items`. The core makes one card of the items from its own store ([`card_for`]), keeps its
//! file as a prepared file of the caller and sends it as stage 4 sends any file
//! ([`ChatService::share`]): upload into the person's Talk folder, sharing into the conversation, the
//! card's text under it when the message has none.
//!
//! - **Reach:** the caller needs read access to every item besides `chat`; for a contact the area
//!   `contacts` and its address book. One item out of reach makes the whole command `not-found`,
//!   checked before the approval and again when it runs (`docs/teilen.md`, decision 1).
//! - **Lean:** a contact goes as a vCard 3.0 newly built of its name and the chosen fields only
//!   (decision 3), never the stored card with its notes, photo, `X-` fields and private addresses.
//! - **From the core:** text and file come from the store, never from the client (decision 2).
//! - **Nothing in logs:** `Debug` of a card names no contact and holds no byte.

use std::sync::Arc;

use torro_contacts::{Contact, ContactFields, FieldKind, vcard};
use torro_core::i18n::{tr, trf};
use torro_ipc::api::{Access, Area};
use torro_ipc::chat::MAX_FILES;
use torro_ipc::share::{MAX_CARD_BYTES, MAX_SHARE_ITEMS, ShareField, ShareItem, ShareKind};
use torro_ipc::{Request, Response};

use super::ChatService;
use super::media::refused;
use super::service::{Caller, conversation_of, error_response};
use crate::contacts::ContactsService;
use crate::pim::Reach;

/// The card of a share: the name of its file, a short text for the message, and the file.
#[derive(Clone, PartialEq, Eq)]
pub struct Card {
    /// The file's name: the contact's name with `.vcf`, `contacts.vcf` for several.
    pub name: String,
    /// One block per item: name, company, numbers, mail addresses, addresses, web pages.
    pub text: String,
    pub bytes: Vec<u8>,
    pub facts: CardFacts,
}

impl std::fmt::Debug for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Card").field("bytes", &self.bytes.len()).field("items", &self.facts.contacts.len()).finish()
    }
}

/// What an approval shows of a card, as the core made it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CardFacts {
    /// The file's name as it goes to the person's Talk folder.
    pub file: String,
    pub size: u64,
    pub contacts: Vec<SharedContact>,
    /// The card's text, which goes under the file when the message has none; from the address book.
    pub text: String,
}

/// One contact on a card, for its approval: "Contact card: Anna Muster (6 fields)".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedContact {
    /// The name as the address book holds it; foreign text.
    pub name: String,
    /// How many values go along: the name, and each number, address, … that goes.
    pub values: usize,
    /// The chosen fields beyond [`ShareField::DEFAULT`] this contact has values in ("+ private
    /// address"), in the order of [`ShareField::ALL`].
    pub extra: Vec<ShareField>,
}

/// What a share would be refused with by its shape alone, before anything is read: `fields`
/// without `items`, more than [`MAX_SHARE_ITEMS`] items, or with the card more than
/// [`MAX_FILES`] files. `files` is how many prepared files the message sends besides.
pub fn shape_refusal(items: &[ShareItem], fields: Option<&[ShareField]>, files: usize) -> Option<Response> {
    if items.is_empty() {
        return fields.is_some().then(|| refused("error", tr("Choose fields only together with items to share.")));
    }
    if items.len() > MAX_SHARE_ITEMS {
        return Some(refused("error", &trf("A message shares at most {n} items.", &[("n", &MAX_SHARE_ITEMS)])));
    }
    (files + 1 > MAX_FILES).then(|| refused("error", tr("A message takes at most 10 files.")))
}

/// The card of `items` with `fields` (the default without), read with the rights of `reach`:
/// `not-found` for the whole share when the caller may not read one item (for a client: read access
/// to `contacts` and the contact's address book), `error` for a card over [`MAX_CARD_BYTES`].
pub async fn card_for(
    contacts: &ContactsService,
    reach: Reach<'_>,
    items: &[ShareItem],
    fields: Option<&[ShareField]>,
) -> Result<Card, Response> {
    if let Some(refusal) = shape_refusal(items, fields, 0) {
        return Err(refusal);
    }
    let fields = ShareField::chosen(fields);
    let mut lean = Vec::new();
    for item in items {
        match item.kind {
            ShareKind::Contact => lean.push(lean_contact(&read_contact(contacts, reach, &item.id).await?, &fields)?),
        }
    }
    let bytes: Vec<u8> = lean.iter().flat_map(|(raw, _)| raw.as_bytes().iter().copied()).collect();
    if bytes.len() as u64 > MAX_CARD_BYTES {
        let limit = torro_gui::units::format_bytes(MAX_CARD_BYTES);
        return Err(refused("error", &trf("The card is larger than {limit}.", &[("limit", &limit)])));
    }
    let name = match lean.as_slice() {
        [(_, contact)] => format!("{}.vcf", contact.shown_name()),
        _ => "contacts.vcf".to_string(),
    };
    let text = lean.iter().map(|(_, contact)| text_of(contact)).collect::<Vec<_>>().join("\n\n");
    let shared = lean.iter().map(|(_, contact)| shared(contact, &fields)).collect();
    let file = super::media::clean_name(&name);
    let facts = CardFacts { file: file.clone(), size: bytes.len() as u64, contacts: shared, text: text.clone() };
    Ok(Card { name: file, text, bytes, facts })
}

/// The contact `id` as `reach` may read it; `not-found` when it may not or there is none.
async fn read_contact(contacts: &ContactsService, reach: Reach<'_>, id: &str) -> Result<Contact, Response> {
    let not_found = || refused("not-found", tr("The contact or address book does not exist."));
    if let Reach::Client(rights) = reach
        && !rights.area(Area::Contacts).allows(Access::Read)
    {
        return Err(not_found());
    }
    let request = Request::GetContact { id: id.to_string() };
    if contacts.out_of_reach(reach, &request).is_some() {
        return Err(not_found());
    }
    match contacts.handle(request, reach).await {
        Response::Contact { contact } => Ok(*contact),
        _ => Err(not_found()),
    }
}

/// The lean vCard 3.0 of `contact` with its name and `fields`, and the contact as the card holds it.
fn lean_contact(contact: &Contact, fields: &[ShareField]) -> Result<(String, Contact), Response> {
    let has = |field| fields.contains(&field);
    let some =
        |values: &[torro_contacts::Labeled<String>], field| (has(field) && !values.is_empty()).then(|| values.to_vec());
    let addresses: Vec<_> = contact
        .addresses
        .iter()
        .filter(|address| match address.kind == FieldKind::Home {
            true => has(ShareField::PrivateAddresses),
            false => has(ShareField::WorkAddresses),
        })
        .cloned()
        .collect();
    let org = has(ShareField::Organization);
    let chosen = ContactFields {
        name: (!contact.name.is_empty()).then(|| contact.name.clone()),
        display_name: Some(contact.shown_name()),
        // Not the nickname: it is no field one chooses, and the approval would not name it.
        nickname: None,
        org: contact.org.clone().filter(|_| org),
        title: contact.title.clone().filter(|_| org),
        emails: some(&contact.emails, ShareField::Emails),
        phones: some(&contact.phones, ShareField::Phones),
        addresses: (!addresses.is_empty()).then_some(addresses),
        urls: some(&contact.urls, ShareField::Web),
        impps: some(&contact.impps, ShareField::Messengers),
        birthday: contact.birthday.as_ref().filter(|_| has(ShareField::Birthday)).map(ToString::to_string),
        anniversary: None,
        note: contact.note.clone().filter(|_| has(ShareField::Note)),
        groups: (has(ShareField::Groups) && !contact.groups.is_empty()).then(|| contact.groups.clone()),
    };
    // A new UID: the stored one may be an id of the provider, and vCard 3.0 needs none of ours.
    let uid = vcard::new_uid().map_err(|_| refused("error", tr("The card of this contact cannot be made.")))?;
    vcard::new_vcard(&uid, &chosen, vcard::Version::V3)
        .map_err(|_| refused("error", tr("The card of this contact cannot be made.")))
}

/// The text of a card for one contact: one value per line, as people read a business card.
fn text_of(contact: &Contact) -> String {
    let mut lines = vec![contact.shown_name()];
    let work: Vec<&str> = [contact.title.as_deref(), contact.org.as_deref()].into_iter().flatten().collect();
    if !work.is_empty() {
        lines.push(work.join(" · "));
    }
    lines.extend(contact.phones.iter().map(|phone| phone.value.clone()));
    lines.extend(contact.emails.iter().map(|mail| mail.value.clone()));
    for address in &contact.addresses {
        let place = [address.value.postal_code.as_str(), address.value.locality.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let parts = [address.value.street.as_str(), place.as_str(), address.value.country.as_str()];
        let line = parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(", ");
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines.extend(contact.urls.iter().map(|url| url.value.clone()));
    lines.join("\n")
}

/// What the approval shows of `contact` as its card holds it.
fn shared(contact: &Contact, fields: &[ShareField]) -> SharedContact {
    let has_value = |field: ShareField| match field {
        ShareField::Organization => contact.org.is_some() || contact.title.is_some(),
        ShareField::Phones => !contact.phones.is_empty(),
        ShareField::Emails => !contact.emails.is_empty(),
        ShareField::WorkAddresses => contact.addresses.iter().any(|a| a.kind != FieldKind::Home),
        ShareField::PrivateAddresses => contact.addresses.iter().any(|a| a.kind == FieldKind::Home),
        ShareField::Web => !contact.urls.is_empty(),
        ShareField::Birthday => contact.birthday.is_some(),
        ShareField::Note => contact.note.is_some(),
        ShareField::Messengers => !contact.impps.is_empty(),
        ShareField::Groups => !contact.groups.is_empty(),
    };
    let values = 1
        + usize::from(contact.org.is_some())
        + usize::from(contact.title.is_some())
        + contact.phones.len()
        + contact.emails.len()
        + contact.addresses.len()
        + contact.urls.len()
        + contact.impps.len()
        + usize::from(contact.birthday.is_some())
        + usize::from(contact.note.is_some())
        + usize::from(!contact.groups.is_empty());
    let extra = ShareField::ALL
        .into_iter()
        .filter(|field| !field.is_default() && fields.contains(field) && has_value(*field))
        .collect();
    SharedContact { name: contact.shown_name(), values, extra }
}

impl ChatService {
    /// `chat.send_message` with `items` for `caller` with `reach`: the card made of them is kept as
    /// a prepared file of the caller and sent with the message's other `files`, its text under it
    /// when the message has none. The card's file is dropped again when the sending does not begin.
    pub async fn share(self: &Arc<Self>, contacts: &ContactsService, request: Request, caller: Caller<'_>) -> Response {
        let Request::ChatSendMessage { conversation, text, reply_to, silent, mut files, items, fields } = request
        else {
            return refused("error", "only chat.send_message shares items");
        };
        if let Some(refusal) = shape_refusal(&items, fields.as_deref(), files.len()) {
            return refusal;
        }
        let Ok((account, _)) = conversation_of(&conversation) else {
            return error_response(&torro_chat::ChatError::NotFound);
        };
        let account = account.to_string();
        let card = match card_for(contacts, caller.reach, &items, fields.as_deref()).await {
            Ok(card) => card,
            Err(refusal) => return refusal,
        };
        let id = match self.prepare_card(&account, &card.name, card.bytes, caller) {
            Ok(id) => id,
            Err(refusal) => return refusal,
        };
        files.push(id.clone());
        let text = if text.trim().is_empty() { card.text } else { text };
        let request =
            Request::ChatSendMessage { conversation, text, reply_to, silent, files, items: Vec::new(), fields: None };
        let answer = self.handle(request, caller).await;
        if !matches!(answer, Response::ChatSending { .. }) {
            self.release(&account, &[id], &[], caller);
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use torro_contacts::{Labeled, PostalAddress};

    use super::*;

    fn anna() -> Contact {
        let address =
            |street: &str, kind| Labeled::new(PostalAddress { street: street.into(), ..Default::default() }, kind);
        Contact {
            uid: "anna-uid".into(),
            display_name: "Anna Muster".into(),
            org: Some("Werke".into()),
            title: Some("Lead".into()),
            phones: vec![Labeled::new("+49 30 1234".into(), FieldKind::Work)],
            emails: vec![Labeled::new("anna@werke.de".into(), FieldKind::Work)],
            addresses: vec![address("Hauptstr. 1", FieldKind::Work), address("Gartenweg 2", FieldKind::Home)],
            birthday: torro_contacts::PartialDate::parse("1990-04-15"),
            note: Some("Knows the secret plan".into()),
            groups: vec!["Friends".into()],
            ..Contact::default()
        }
    }

    #[test]
    fn a_card_holds_the_name_and_the_chosen_fields_only() {
        let (raw, card) = lean_contact(&anna(), &ShareField::chosen(None)).expect("card");
        assert!(raw.starts_with("BEGIN:VCARD\r\nVERSION:3.0\r\n") && !raw.contains("anna-uid"), "{raw}");
        for kept in ["Anna Muster", "Werke", "Lead", "+49 30 1234", "anna@werke.de", "Hauptstr. 1"] {
            assert!(raw.contains(kept), "{kept} goes: {raw}");
        }
        for private in ["Gartenweg", "secret plan", "Friends", "1990", "BDAY", "NOTE"] {
            assert!(!raw.contains(private), "{private} stays: {raw}");
        }
        let text = text_of(&card);
        assert_eq!(text, "Anna Muster\nLead · Werke\n+49 30 1234\nanna@werke.de\nHauptstr. 1");
        let facts = shared(&card, &ShareField::chosen(None));
        assert_eq!((facts.values, facts.extra.as_slice()), (6, &[][..]));

        let more = ShareField::chosen(Some(&[ShareField::PrivateAddresses, ShareField::Birthday, ShareField::Phones]));
        let (raw, card) = lean_contact(&anna(), &more).expect("card");
        assert!(raw.contains("Gartenweg") && raw.contains("BDAY") && !raw.contains("anna@werke.de"), "{raw}");
        assert_eq!(shared(&card, &more).extra, [ShareField::PrivateAddresses, ShareField::Birthday]);
    }

    #[test]
    fn a_share_is_refused_by_its_shape_before_anything_is_read() {
        let code = |refusal: Option<Response>| match refusal {
            Some(Response::Error { code, .. }) => code,
            other => format!("{other:?}"),
        };
        let one = [ShareItem::contact("a/b/c")];
        assert_eq!(code(shape_refusal(&[], Some(&[ShareField::Note]), 0)), "error", "fields without items");
        assert!(shape_refusal(&[], None, 3).is_none(), "no items, nothing to say");
        let many = vec![ShareItem::contact("a/b/c"); MAX_SHARE_ITEMS + 1];
        assert_eq!(code(shape_refusal(&many, None, 0)), "error");
        assert_eq!(code(shape_refusal(&one, None, MAX_FILES)), "error", "the card is one more file");
        assert!(shape_refusal(&one, None, MAX_FILES - 1).is_none());
    }
}
