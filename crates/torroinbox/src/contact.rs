//! "Save as contact" and "Add contact" (issue #727): the action of a mail that keeps its sender – or
//! the vCard attached to it – in the address books, through the core's
//! `inbox.create_contact_from_item`.
//!
//! - **Where:** in the mail window's rail, group "Torro" (`mail::torro_items`, also for mail without an
//!   entry, which sends the mail's `m:…` id), and in the footer of a mail entry's panel in the hub and
//!   the App Feed (`detail::button_groups`), an icon button beside "Make task" and "Make event".
//! - **Its name** ([`label`]): "Add contact" when the core would make the contact of the mail's vCard
//!   ([`has_vcard`]: its first attachment of type `text/vcard` or `text/x-vcard`, or named `….vcf`, at
//!   most [`MAX_CARD_BYTES`]), else "Save as contact" – then the core saves the sender. The core finds
//!   the card itself; the window only names the action after it, by the same rule.
//! - **Only with a book to write to** ([`writable_book`]): the first address book that may be written
//!   and is not hidden, the rule of the compose window's "+ Contact". The window asks for it once at
//!   start (and again on its tick until there is one), never while it draws.
//! - **The answer** ([`saved_text`]): a toast "Saved to contacts" or – when the person was there
//!   already and nothing was made – "Already in your contacts", both with "Open contact", which opens
//!   the contacts app on the contact (`apps.open_app` with [`target`]).
//!
//! The demo ([`demo_from_item`]) answers without a core: the first save of a mail makes a contact of
//! its sender, every further one finds it there.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock, PoisonError};

use torro_contacts::{Contact, FieldKind, FromItem, Labeled};
use torro_core::i18n::tr;
use torro_inbox::demo::DemoSource;
use torro_inbox::{InboxError, InboxSource, Link};
use torro_ipc::inbox::InboxClient;
use torro_ipc::{IpcError, Request, Response};
use torro_mail::{AttachmentInfo, Message as Mail};

/// What a footer needs to show the action (`detail::Footer::contact`): whether the mail carries a
/// vCard (its name) and whether a save is on its way (then it takes no click).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContactOffer {
    pub vcard: bool,
    pub busy: bool,
}

/// Whether `file` is a vCard as the core looks for one: of type `text/vcard` or `text/x-vcard`
/// (parameters such as `charset` aside), or named `….vcf`, without regard to case.
pub fn is_vcard(file: &AttachmentInfo) -> bool {
    let kind = file.content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    let named = file.filename.as_deref().is_some_and(|name| name.trim().to_ascii_lowercase().ends_with(".vcf"));
    kind == "text/vcard" || kind == "text/x-vcard" || named
}

/// The largest vCard the core reads from a mail (`MAX_ATTACHED_CARD_BYTES` of the core, 1 MiB): a
/// larger one is not read, the core saves the sender instead.
pub const MAX_CARD_BYTES: u64 = 1024 * 1024;

/// Whether the core makes the contact of `mail`'s vCard, as it picks the card: the first attachment
/// that is one ([`is_vcard`]; also an inline part counts), and only while it is at most
/// [`MAX_CARD_BYTES`] large – a later card is never taken in its place.
pub fn has_vcard(mail: &Mail) -> bool {
    mail.attachments.iter().find(|file| is_vcard(file)).is_some_and(|card| card.size <= MAX_CARD_BYTES)
}

/// The action's name: "Add contact" for a mail with a vCard, else "Save as contact".
pub fn label(vcard: bool) -> &'static str {
    if vcard { tr("Add contact") } else { tr("Save as contact") }
}

/// The toast after the answer: "Saved to contacts" for a contact made now, "Already in your
/// contacts" when the core found the person there.
pub fn saved_text(created: bool) -> &'static str {
    if created { tr("Saved to contacts") } else { tr("Already in your contacts") }
}

/// The target `apps.open_app` opens the contacts app on: `contact:<id>`.
pub fn target(contact: &str) -> String {
    format!("contact:{contact}")
}

/// The book "Save as contact" writes to, of `books` as `contacts.list_address_books` answers them: the
/// first one that may be written and is not hidden (the compose window's rule for "+ Contact").
pub fn writable_book(books: Vec<torro_contacts::AddressBook>) -> Option<String> {
    books.into_iter().find(|book| book.writable && !book.hidden_by_default).map(|book| book.id)
}

/// `request` to the core, its error as the window's calls report it.
async fn ask(client: &InboxClient, request: Request) -> Result<Response, InboxError> {
    match torro_ipc::client::call(&client.socket, &request).await {
        Ok(Response::Error { code, message }) => Err(match code.as_str() {
            "not-found" => InboxError::NotFound,
            "forbidden" => InboxError::Forbidden,
            "unsupported" => InboxError::Unsupported,
            _ => InboxError::Other(message),
        }),
        Ok(response) => Ok(response),
        Err(IpcError::NotRunning) => Err(InboxError::Offline),
        Err(err) => Err(InboxError::Other(err.to_string())),
    }
}

/// `contacts.list_address_books` through the core, then [`writable_book`].
pub async fn core_writable_book(client: &InboxClient) -> Result<Option<String>, InboxError> {
    match ask(client, Request::ListAddressBooks { account: None }).await? {
        Response::AddressBooks { books } => Ok(writable_book(books)),
        other => Err(InboxError::Other(format!("unexpected answer: {other:?}"))),
    }
}

/// `inbox.create_contact_from_item` through the core for the entry or mail `id` into `book`.
pub async fn core_from_item(client: &InboxClient, id: String, book: String) -> Result<FromItem, InboxError> {
    match ask(client, Request::CreateContactFromItem { id, book }).await? {
        Response::ContactFromItem(from) => Ok(*from),
        // The app key is never asked to wait for a person; such an answer made nothing.
        Response::Pending { .. } => Err(InboxError::Other(tr("The contact waits for approval.").to_owned())),
        other => Err(InboxError::Other(format!("unexpected answer: {other:?}"))),
    }
}

/// The mails the demo saved as contacts while it runs, by message id.
fn demo_saved() -> &'static Mutex<HashSet<String>> {
    static SAVED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SAVED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The demo's `inbox.create_contact_from_item`: the mail of `id` (an entry's or an `m:…` id) found
/// as the core finds it, a contact of its sender in `book` – made the first time (`created: true`),
/// found there every time after.
pub async fn demo_from_item(source: &DemoSource, id: String, book: String) -> Result<FromItem, InboxError> {
    let message = if id.starts_with("m:") {
        id
    } else {
        let items = source.list_items(torro_inbox::ItemFilter::default()).await?;
        let item = items.into_iter().find(|item| item.id == id).ok_or(InboxError::NotFound)?;
        match item.link {
            Some(Link::Mail { message }) => message,
            _ => return Err(InboxError::Unsupported),
        }
    };
    let mail = DemoSource::get_mail(source, &message)?;
    let from = mail.summary.from.clone().ok_or(InboxError::Unsupported)?;
    let created = demo_saved().lock().unwrap_or_else(PoisonError::into_inner).insert(message.clone());
    let uid = format!("demo-mail-{}", message.trim_start_matches("m:"));
    let contact = Contact {
        id: format!("{book}/{uid}"),
        book,
        uid,
        display_name: from.name.clone().unwrap_or_else(|| from.address.clone()),
        emails: vec![Labeled::new(from.address, FieldKind::Work)],
        untrusted: true,
        ..Contact::default()
    };
    Ok(FromItem { contact, created })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: Option<&str>, kind: &str) -> AttachmentInfo {
        AttachmentInfo {
            part: "2".into(),
            filename: name.map(str::to_owned),
            content_type: kind.into(),
            size: 120,
            inline: false,
            content_id: None,
        }
    }

    #[test]
    fn vcards_are_found_by_type_or_name_without_case() {
        assert!(is_vcard(&file(None, "text/vcard")));
        assert!(is_vcard(&file(None, "TEXT/X-VCARD; charset=utf-8")));
        assert!(is_vcard(&file(Some("Lena.VCF"), "application/octet-stream")));
        assert!(!is_vcard(&file(Some("budget.csv"), "text/csv")));
        assert!(!is_vcard(&file(Some("vcf.txt"), "text/plain")), "only the end of the name counts");
    }

    /// Only the first card counts, and only up to 1 MiB, as the core reads it: a larger one leaves
    /// "Save as contact", the core saving the sender.
    #[test]
    fn only_the_first_card_up_to_a_mebibyte_counts() {
        let demo = DemoSource::new(chrono::Utc::now());
        let mut mail = DemoSource::get_mail(&demo, "m:w4").expect("a demo mail");
        mail.attachments = vec![file(Some("budget.csv"), "text/csv")];
        assert!(!has_vcard(&mail), "no card");
        let mut card = file(Some("lena.vcf"), "text/vcard");
        card.size = MAX_CARD_BYTES;
        mail.attachments.push(card.clone());
        assert!(has_vcard(&mail), "up to 1 MiB");
        assert_eq!(label(has_vcard(&mail)), tr("Add contact"));
        let mut large = card.clone();
        large.size = MAX_CARD_BYTES + 1;
        mail.attachments = vec![large, card];
        assert!(!has_vcard(&mail), "the first card is too large; the second is not taken in its place");
        assert_eq!(label(has_vcard(&mail)), tr("Save as contact"));
    }

    #[test]
    fn the_name_follows_the_vcard() {
        assert_eq!(label(true), tr("Add contact"));
        assert_eq!(label(false), tr("Save as contact"));
    }

    #[test]
    fn the_toast_says_whether_a_contact_was_made() {
        assert_eq!(saved_text(true), tr("Saved to contacts"));
        assert_eq!(saved_text(false), tr("Already in your contacts"));
    }

    #[test]
    fn the_contacts_app_opens_on_the_contact() {
        assert_eq!(target("book/uid-1"), "contact:book/uid-1");
    }

    #[test]
    fn the_book_is_the_first_writable_one_that_is_not_hidden() {
        let (books, _) = torro_contacts::demo::sample(chrono::NaiveDate::default());
        assert_eq!(writable_book(books.clone()), Some(torro_contacts::demo::CONTACTS.to_owned()));
        let hidden: Vec<_> = books
            .iter()
            .cloned()
            .map(|mut book| {
                book.hidden_by_default = book.writable;
                book
            })
            .collect();
        assert_eq!(writable_book(hidden), None, "a hidden book is not written to");
        let read_only: Vec<_> = books
            .into_iter()
            .map(|mut book| {
                book.writable = false;
                book
            })
            .collect();
        assert_eq!(writable_book(read_only), None, "without a writable book there is no action");
    }

    #[test]
    fn the_demo_makes_a_contact_once_and_finds_it_after() {
        let demo = DemoSource::new(chrono::Utc::now());
        let book = torro_contacts::demo::CONTACTS.to_owned();
        let first = iced::futures::executor::block_on(demo_from_item(&demo, "m:w7".into(), book.clone()));
        let first = first.expect("the demo answers");
        assert!(first.created);
        assert_eq!(first.contact.book, book);
        assert!(first.contact.id.starts_with(&format!("{book}/")));
        let again = iced::futures::executor::block_on(demo_from_item(&demo, "m:w7".into(), book));
        assert!(!again.expect("the demo answers").created, "no second contact of the same mail");
    }
}
