//! The commands of the area `contacts` (`docs/kontakte.md`, task KO1.4): read from the store,
//! written into it and then to the server.
//!
//! - **Reach:** a client sees only the books of its rights ([`Rights::reaches_address_book`]); a
//!   hidden book only when named. Everything else is filtered before it is read, in lists,
//!   searches, counts, groups and birthdays alike, and an id outside is `not-found`
//!   ([`ContactsService::out_of_reach`], checked before an approval is asked for).
//! - **Versions:** a change names the `etag` the client showed; another one is `conflict` at once,
//!   without asking the server. A contact in conflict is changed only by `resolve_conflict`.
//! - **Writing:** the vCard changes only where a field changed ([`vcard::patch`]), the change is
//!   queued and sent at once; offline it waits (`state: pending`). A newer version on the server is
//!   merged once ([`super::sync`]), else the contact is in conflict.
//! - **Read-only books** refuse every change with `unsupported`, but a favourite, which stays on
//!   this device.
//! - **Groups** are `CATEGORIES`; a book that keeps groups as cards of their own (iCloud) refuses
//!   changes of groups for now.
//! - **Import and export** (issue #725) keep every card as written: an import stores the file's
//!   card (a `UID` added where it has none) and leaves out people already there
//!   ([`torro_contacts::duplicates`]); an export gives the cards as stored.
//! - **Duplicates** (issue #726) are found in the books in reach that are not hidden; merging keeps
//!   the chosen contact's fields, takes the others' lists and deletes them; pairs said to be
//!   different people stay apart ([`Store::not_duplicates`]).

use base64::Engine;
use chrono::{Local, Utc};
use std::collections::HashSet;
use torro_contacts::suggest::GroupCandidate;

use torro_contacts::duplicates;
use torro_contacts::{
    AddressBook, Birthday, Contact, ContactFields, ContactPage, ContactState, ContactSummary, ContactsError,
    ContactsProvider, DuplicateReason, FieldKind, FieldsError, FromItem, Group, Imported, Keep, Labeled, Merged, Moved,
    PhotoSize, Stored, contact_id, vcard,
};
use torro_core::i18n::{Lang, lang, tr, trf};
use torro_ipc::api::Rights;
use torro_ipc::contacts::{BIRTHDAY_DAYS, MAX_BIRTHDAY_DAYS, MAX_IMPORT_BYTES, MAX_IMPORT_CARDS, MERGE_MAX, MERGE_MIN};
use torro_ipc::{Request, Response};

use super::store::{BookRow, CardRow, Op, Query, Queued, Store};
use super::sync::{self, Sent, with};
use super::{ContactsService, photo};
use crate::pim::Reach;

/// The most contacts, groups and members of one group `contacts.suggest_recipients` looks at (issue
/// #460): a search of one or two letters matches many, and the answer holds at most 20 anyway.
pub const SUGGEST_CANDIDATES: usize = 200;

/// The name of Nextcloud's system address book (every user of the instance), hidden by default.
const SYSTEM_BOOK: &str = "z-server-generated--system";

/// The largest vCard attached to a mail that `inbox.create_contact_from_item` takes (issue #727).
pub const MAX_ATTACHED_CARD_BYTES: usize = 1024 * 1024;

/// The calling code numbers with the trunk prefix 0 belong to, by the suite's language
/// ([`duplicates::country_code`]): the suite speaks German to people in Germany, Austria and
/// Switzerland, whose numbers are nearly all written with `+` when they cross a border.
fn country() -> Option<&'static str> {
    match lang() {
        Lang::De => duplicates::country_code("de"),
        Lang::En => None,
    }
}

/// A card an import or a mail brings: what it is to the address books.
enum Card {
    /// Does not read, names nobody, or is a group card.
    Skipped,
    /// The person is there already: the contact, when one of the books holds it (an earlier card of
    /// the same file has none).
    Known(Option<String>),
    /// New: the card as it will be stored, with its `UID`.
    New { uid: String, raw: String },
}

/// What a card is compared with: the `UID`s of the target book, and the mail addresses, numbers and
/// names with company of the books in reach that are not hidden, of the target book and of the
/// cards taken before ([`duplicates::keys`]), each with the contact that holds it.
#[derive(Default)]
struct Known {
    uids: HashSet<String>,
    keys: std::collections::HashMap<(DuplicateReason, String), Option<String>>,
}

impl Known {
    fn add(&mut self, contact: &Contact, id: Option<&str>) {
        for key in duplicates::keys(contact, country()) {
            self.keys.entry(key).or_insert_with(|| id.map(str::to_string));
        }
    }

    /// What `raw` is ([`Card`]); a new card is added, so the next one with the same person is known.
    fn sort(&mut self, raw: &str) -> Result<Card, ContactsError> {
        let contact = match vcard::read(raw) {
            Ok(contact) if vcard::group_card(raw).is_none() && !contact.shown_name().trim().is_empty() => contact,
            _ => return Ok(Card::Skipped),
        };
        if !contact.uid.is_empty() && self.uids.contains(&contact.uid) {
            return Ok(Card::Known(None));
        }
        if let Some(held) = duplicates::keys(&contact, country()).iter().find_map(|key| self.keys.get(key)) {
            return Ok(Card::Known(held.clone()));
        }
        // A card without a UID gets a fresh one; every other line stays as the file wrote it.
        let (uid, raw) = if contact.uid.trim().is_empty() {
            let uid = vcard::new_uid().map_err(ContactsError::Other)?;
            (uid.clone(), vcard::set_uid(raw, &uid))
        } else {
            (contact.uid.clone(), raw.to_string())
        };
        self.uids.insert(uid.clone());
        self.add(&contact, None);
        Ok(Card::New { uid, raw })
    }
}

/// What a command needs of the store before it runs: its contact and the contact's book.
struct Found {
    card: CardRow,
    book: BookRow,
}

/// The answer for `err`, with the fixed codes of the area.
pub fn error_response(err: &ContactsError) -> Response {
    let (code, message) = match err {
        ContactsError::Auth => ("auth", tr("The server refused the sign-in.").to_string()),
        ContactsError::Offline => ("offline", tr("The server cannot be reached.").to_string()),
        ContactsError::Conflict { .. } => {
            ("conflict", tr("The contact changed meanwhile. Load it again and repeat the change.").to_string())
        }
        ContactsError::ReadOnly => ("unsupported", tr("This address book is read-only.").to_string()),
        ContactsError::TooLarge => ("error", tr("The server refuses a contact this large.").to_string()),
        ContactsError::NotFound => ("not-found", tr("The contact or address book does not exist.").to_string()),
        ContactsError::Unsupported(message) => ("unsupported", message.clone()),
        ContactsError::Other(message) => ("error", message.clone()),
    };
    Response::Error { code: code.into(), message }
}

fn fields_error(err: FieldsError) -> ContactsError {
    match err {
        FieldsError::BadDate(field) => {
            ContactsError::Other(trf("“{field}” is not a date like 1990-04-15 or --04-15.", &[("field", &field)]))
        }
        FieldsError::WouldReplace(field) => ContactsError::Unsupported(trf(
            "“{field}” holds a value already; contacts.update_contact replaces it.",
            &[("field", &field)],
        )),
        FieldsError::Empty => ContactsError::Other(tr("Nothing to change was sent.").to_string()),
    }
}

/// The contact of a [`Response::Contact`].
fn contact_in(response: Response) -> Result<Contact, ContactsError> {
    match response {
        Response::Contact { contact } => Ok(*contact),
        _ => Err(ContactsError::NotFound),
    }
}

/// The text of the `.vcf` file at `path` for `contacts.import_vcards` (issue #725): an absolute path
/// to a regular file reached without a symbolic link at its end, at most [`MAX_IMPORT_BYTES`];
/// bytes that are no UTF-8 become replacement characters. Blocks: call it on a blocking thread.
fn read_vcf(path: &str) -> Result<String, ContactsError> {
    let path = std::path::Path::new(path);
    let unreadable = || ContactsError::Other(tr("The file cannot be read.").to_string());
    if !path.is_absolute() {
        return Err(ContactsError::Other(tr("The path must be absolute.").to_string()));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| unreadable())?;
    if !meta.is_file() {
        return Err(unreadable());
    }
    if meta.len() > MAX_IMPORT_BYTES {
        return Err(ContactsError::Other(trf(
            "The file is larger than {mb} MB.",
            &[("mb", &(MAX_IMPORT_BYTES / 1024 / 1024))],
        )));
    }
    let bytes = std::fs::read(path).map_err(|_| unreadable())?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err(ContactsError::Other(trf(
            "The file is larger than {mb} MB.",
            &[("mb", &(MAX_IMPORT_BYTES / 1024 / 1024))],
        )));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Whether `reach` sees `book`.
pub fn reaches(reach: Reach<'_>, book: &BookRow) -> bool {
    match reach {
        Reach::Suite => true,
        Reach::Client(rights) => reaches_with(rights, book),
    }
}

fn reaches_with(rights: &Rights, book: &BookRow) -> bool {
    rights.reaches_address_book(&book.id, book.hidden)
}

/// The contact of `card` in `book` as the API answers it.
pub fn contact_of(s: &Store, card: &CardRow, book: &BookRow) -> Contact {
    let mut contact = vcard::read(&card.vcard).unwrap_or_default();
    contact.id = card.id.clone();
    contact.book = card.book.clone();
    contact.uid = card.uid.clone();
    contact.etag = shown_etag(card).map(str::to_string);
    contact.state = card.state;
    contact.readonly = !book.writable;
    contact.favorite = s.is_favorite(&card.id).unwrap_or(false);
    contact.untrusted = true;
    if card.state == ContactState::Conflict {
        contact.server = card.server.as_deref().and_then(|raw| vcard::read(raw).ok()).map(|mut server| {
            server.id = card.id.clone();
            server.book = card.book.clone();
            server.uid = card.uid.clone();
            server.etag = card.server_etag.clone();
            server.readonly = !book.writable;
            server.untrusted = true;
            Box::new(server)
        });
    }
    contact
}

/// The version a client is shown of `card` and sends back to change it. In a conflict that is the
/// server's version it is shown beside: settling the conflict must fail when the server changed
/// once more since, instead of writing over a version nobody saw (this device's own `etag` stays
/// put during a conflict).
fn shown_etag(card: &CardRow) -> Option<&str> {
    if card.state == ContactState::Conflict { card.server_etag.as_deref() } else { card.etag.as_deref() }
}

/// Whether `sent` is the version of `card` the client showed ([`shown_etag`]); a contact not on the
/// server (yet or any more) has none, and the client sends it empty.
fn same_version(card: &CardRow, sent: &str) -> bool {
    shown_etag(card).unwrap_or("") == sent
}

/// The vCard version most cards of `book` are written in; 3.0 when none (every server reads it).
fn version_of(cards: &[CardRow]) -> vcard::Version {
    let four = cards.iter().filter(|c| vcard::version(&c.vcard) == vcard::Version::V4).count();
    if four * 2 > cards.len() { vcard::Version::V4 } else { vcard::Version::V3 }
}

impl ContactsService {
    /// The books `reach` sees, of `account` or all.
    fn books_within(&self, reach: Reach<'_>, account: Option<&str>) -> Result<Vec<BookRow>, ContactsError> {
        let books = with(self.store()?, |s| s.books(account))?;
        Ok(books.into_iter().filter(|book| reaches(reach, book)).collect())
    }

    /// The users of `account`'s server as its system address book names them: user id and name,
    /// nothing else of a card. For the presence, which shows who is there (`docs/chat.md`,
    /// decision 10); empty when the server shares no such book or it is not synced yet.
    pub fn server_people(&self, account: &str) -> Vec<crate::chat::remote::RemotePerson> {
        let Ok(store) = self.store() else { return Vec::new() };
        with(store, |s| {
            let Some(book) = s.books(Some(account))?.into_iter().find(|book| book.name == SYSTEM_BOOK) else {
                return Ok(Vec::new());
            };
            Ok(s.cards(&book.id)?
                .into_iter()
                .filter_map(|card| {
                    let name = vcard::read(&card.vcard).ok()?.shown_name();
                    (!name.trim().is_empty()).then_some(crate::chat::remote::RemotePerson { id: card.uid, name })
                })
                .collect())
        })
        .unwrap_or_default()
    }

    /// Whether `reach` sees at least one book of `account` (`accounts.list_accounts` names
    /// `contacts` among its services then).
    pub fn reaches_any_book(&self, reach: Reach<'_>, account: &str) -> bool {
        self.books_within(reach, Some(account)).is_ok_and(|books| !books.is_empty())
    }

    fn find(&self, id: &str) -> Result<Found, ContactsError> {
        with(self.store()?, |s| {
            let Some(card) = s.card(id)? else { return Ok(None) };
            Ok(s.book(&card.book)?.map(|book| Found { card, book }))
        })?
        .ok_or(ContactsError::NotFound)
    }

    fn book(&self, id: &str) -> Result<BookRow, ContactsError> {
        with(self.store()?, |s| s.book(id))?.ok_or(ContactsError::NotFound)
    }

    /// For a client: the id in `request` it may not reach (a book outside its rights, a contact in
    /// one, or one that does not exist), answered `not-found` before anything else; `None` when
    /// everything is in reach and for requests of other areas. Moving needs both books.
    pub fn out_of_reach(&self, reach: Reach<'_>, request: &Request) -> Option<String> {
        let Reach::Client(rights) = reach else { return None };
        let book_outside = |id: &str| match self.book(id) {
            Ok(book) if reaches_with(rights, &book) => None,
            _ => Some(id.to_string()),
        };
        let contact_outside = |id: &str| match self.find(id) {
            Ok(found) if reaches_with(rights, &found.book) => None,
            _ => Some(id.to_string()),
        };
        match request {
            Request::ListContacts { book: Some(book), .. } | Request::ListContactGroups { book: Some(book) } => {
                book_outside(book)
            }
            Request::CreateContact { book, .. } => book_outside(book),
            Request::GetContact { id }
            | Request::GetContactPhoto { id, .. }
            | Request::AddToContact { id, .. }
            | Request::UpdateContact { id, .. }
            | Request::SetContactFavorite { id, .. }
            | Request::AddContactToGroup { id, .. }
            | Request::RemoveContactFromGroup { id, .. }
            | Request::MarkContactUsed { id }
            | Request::SetContactPhoto { id, .. }
            | Request::DeleteContact { id, .. }
            | Request::ResolveContactConflict { id, .. } => contact_outside(id),
            Request::MoveContact { id, to_book, .. } => contact_outside(id).or_else(|| book_outside(to_book)),
            // Issues #725 and #726: the book named and every contact named.
            Request::ImportVCards { book, .. } | Request::FindDuplicates { book: Some(book) } => book_outside(book),
            Request::ExportVCards { book, ids, .. } => {
                book.as_deref().and_then(book_outside).or_else(|| ids.iter().find_map(|id| contact_outside(id)))
            }
            Request::MergeContacts { ids, .. } | Request::DismissDuplicates { ids } => {
                ids.iter().find_map(|id| contact_outside(id))
            }
            _ => None,
        }
    }

    /// The contact `id` with the name it shows, when `reach` sees its book (`apps.open_app` with
    /// `contact:<id>`, issue #727); the id as the store holds it.
    pub fn named(&self, reach: Reach<'_>, id: &str) -> Option<(String, String)> {
        let found = self.find(id).ok().filter(|found| reaches(reach, &found.book))?;
        let name = vcard::read(&found.card.vcard).map(|contact| contact.shown_name()).unwrap_or_default();
        Some((found.card.id, name))
    }

    /// For `reach`: whether it sees the book `id` (`inbox.create_contact_from_item`, issue #727).
    pub fn sees_book(&self, reach: Reach<'_>, id: &str) -> bool {
        self.book(id).is_ok_and(|book| reaches(reach, &book))
    }

    /// The answer for a request of the area that cannot run whatever the store holds (issues #725,
    /// #726): `forbidden` for a file to import from a client (only the suite's apps name files),
    /// `error` for a request outside the limits. Checked before an approval is asked for, and again
    /// when the command runs. `None` for everything else.
    pub fn refusal(&self, reach: Reach<'_>, request: &Request) -> Option<Response> {
        let error = |message: String| Some(Response::Error { code: "error".into(), message });
        let named = |ids: &[String]| {
            let distinct: HashSet<&String> = ids.iter().collect();
            if !(MERGE_MIN..=MERGE_MAX).contains(&ids.len()) || distinct.len() != ids.len() {
                return error(trf(
                    "Name {min} to {max} different contacts.",
                    &[("min", &MERGE_MIN), ("max", &MERGE_MAX)],
                ));
            }
            None
        };
        match request {
            Request::ImportVCards { path: Some(_), .. } if !matches!(reach, Reach::Suite) => Some(Response::Error {
                code: "forbidden".into(),
                message: tr("Only the suite's apps import from a file; send the cards as `data`.").to_string(),
            }),
            Request::ImportVCards { data, path, .. } if data.is_some() == path.is_some() => {
                error(tr("Send the cards either as `data` or as `path`.").to_string())
            }
            Request::ExportVCards { group: Some(_), book: None, .. } => {
                error(tr("A group needs its address book (`book`).").to_string())
            }
            Request::ExportVCards { ids, .. } if ids.len() > MAX_IMPORT_CARDS => {
                error(trf("Name at most {n} contacts.", &[("n", &MAX_IMPORT_CARDS)]))
            }
            Request::DismissDuplicates { ids } => named(ids),
            Request::MergeContacts { ids, etags, keep } => named(ids).or_else(|| {
                if etags.len() != ids.len() || !ids.contains(keep) {
                    return error(
                        tr("Send an `etag` for each contact, in the same order, and `keep` among them.").to_string(),
                    );
                }
                None
            }),
            _ => None,
        }
    }

    /// Answers a `contacts.*` request for `reach` (its rights and reach checked before). A command
    /// that writes runs whole under the lock of the rounds, from its checks to the answer, so no
    /// round sends or merges the same contact in between.
    pub async fn handle(&self, request: Request, reach: Reach<'_>) -> Response {
        if let Some(refused) = self.refusal(reach, &request) {
            return refused;
        }
        let _round = match request.operation().access {
            torro_ipc::api::Access::Read => None,
            _ => Some(self.rounds.lock().await),
        };
        let answer = match request {
            Request::ListAddressBooks { account } => self.list_books(reach, account.as_deref()),
            Request::ListContacts { book, group, favorites, recent, query, limit, cursor } => {
                let query = torro_contacts::ContactQuery { book, group, favorites, recent, query, limit, cursor };
                self.list_contacts(reach, query).map(Response::Contacts)
            }
            Request::GetContact { id } => self.get_contact(&id),
            Request::GetContactPhoto { id, size } => self.get_photo(&id, size),
            Request::ListContactGroups { book } => self.list_groups(reach, book.as_deref()),
            Request::UpcomingBirthdays { days } => self.birthdays(reach, days),
            Request::CreateContact { book, fields } => Box::pin(self.create(&book, &fields)).await,
            Request::AddToContact { id, fields } => Box::pin(self.add_to(&id, &fields)).await,
            Request::UpdateContact { id, etag, fields } => {
                Box::pin(self.change(&id, &etag, |book, contact| {
                    // The groups as they are, sent along with the other fields, change nothing.
                    let regroups = fields
                        .groups
                        .as_ref()
                        .is_some_and(|groups| torro_contacts::model::clean_groups(groups) != contact.groups);
                    if regroups {
                        Self::no_group_cards(book, Some(&fields))?;
                    }
                    fields.apply(contact).map_err(fields_error)
                }))
                .await
            }
            Request::SetContactFavorite { id, favorite } => self.set_favorite(&id, favorite),
            Request::AddContactToGroup { id, etag, group } => Box::pin(self.group(&id, &etag, &group, true)).await,
            Request::RemoveContactFromGroup { id, etag, group } => {
                Box::pin(self.group(&id, &etag, &group, false)).await
            }
            Request::MarkContactUsed { id } => self.mark_used(&id),
            Request::SetContactPhoto { id, etag, data } => Box::pin(self.set_photo(&id, &etag, data.as_deref())).await,
            Request::MoveContact { id, etag, to_book } => Box::pin(self.move_contact(&id, &etag, &to_book)).await,
            Request::DeleteContact { id, etag } => Box::pin(self.delete(&id, &etag)).await,
            Request::ResolveContactConflict { id, keep, etag } => Box::pin(self.resolve(&id, keep, &etag)).await,
            Request::ImportVCards { book, data, path, dry_run } => {
                Box::pin(self.import(reach, &book, data, path, dry_run)).await
            }
            Request::ExportVCards { book, group, ids } => self.export(reach, book.as_deref(), group.as_deref(), &ids),
            Request::FindDuplicates { book } => self.find_duplicates(reach, book.as_deref()),
            Request::MergeContacts { ids, etags, keep } => Box::pin(self.merge(&ids, &etags, &keep)).await,
            Request::DismissDuplicates { ids } => self.dismiss(&ids),
            other => Err(ContactsError::Other(format!("{} is no contacts command", other.operation().name()))),
        };
        answer.unwrap_or_else(|err| error_response(&err))
    }

    fn list_books(&self, reach: Reach<'_>, account: Option<&str>) -> Result<Response, ContactsError> {
        let books = self.books_within(reach, account)?;
        let books = with(self.store()?, |s| {
            books
                .into_iter()
                .map(|book| {
                    Ok(AddressBook {
                        count: s.count(&book.id)?,
                        id: book.id,
                        account: book.account,
                        name: book.name,
                        display_name: book.display_name,
                        writable: book.writable,
                        hidden_by_default: book.hidden,
                        untrusted: true,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })?;
        Ok(Response::AddressBooks { books })
    }

    /// One page of contacts; filtered to the books of `reach` in the query itself, before the
    /// search and the paging.
    pub fn list_contacts(
        &self,
        reach: Reach<'_>,
        query: torro_contacts::ContactQuery,
    ) -> Result<ContactPage, ContactsError> {
        let books: Vec<BookRow> = self
            .books_within(reach, None)?
            .into_iter()
            .filter(|book| query.book.as_ref().is_none_or(|wanted| *wanted == book.id))
            .collect();
        if query.book.is_some() && books.is_empty() {
            return Err(ContactsError::NotFound);
        }
        let offset = match query.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor
                .parse::<usize>()
                .map_err(|_| ContactsError::Other(tr("The cursor is not one of an earlier answer.").to_string()))?,
        };
        let limit = query.page_size();
        let store_query = Query {
            books: books.iter().map(|b| b.id.clone()).collect(),
            group: query.group.clone(),
            favorites: query.favorites,
            recent: query.recent,
            text: query.query.clone(),
            offset,
            limit,
        };
        with(self.store()?, |s| {
            let (rows, more) = s.query(&store_query)?;
            let contacts: Vec<ContactSummary> = rows
                .iter()
                .filter_map(|row| {
                    books.iter().find(|b| b.id == row.book).map(|book| contact_of(s, row, book).summary())
                })
                .collect();
            Ok(ContactPage { contacts, next_cursor: more.then(|| (offset + limit).to_string()) })
        })
    }

    fn get_contact(&self, id: &str) -> Result<Response, ContactsError> {
        let found = self.find(id)?;
        let contact = with(self.store()?, |s| Ok(contact_of(s, &found.card, &found.book)))?;
        Ok(Response::Contact { contact: Box::new(contact) })
    }

    fn get_photo(&self, id: &str, size: PhotoSize) -> Result<Response, ContactsError> {
        let found = self.find(id)?;
        let photo = vcard::read(&found.card.vcard).ok().and_then(|c| c.photo).ok_or(ContactsError::NotFound)?;
        let png = self
            .previews()
            .get(&found.book.account, id, &found.card.vcard, &photo, size)
            .ok_or(ContactsError::NotFound)?;
        Ok(Response::Image { png: base64::engine::general_purpose::STANDARD.encode(png) })
    }

    fn list_groups(&self, reach: Reach<'_>, book: Option<&str>) -> Result<Response, ContactsError> {
        let books: Vec<String> = self
            .books_within(reach, None)?
            .into_iter()
            .map(|b| b.id)
            .filter(|id| book.is_none_or(|wanted| wanted == id))
            .collect();
        if book.is_some() && books.is_empty() {
            return Err(ContactsError::NotFound);
        }
        let groups = with(self.store()?, |s| s.groups(&books))?
            .into_iter()
            .map(|(book, name, count)| Group { book, name, count, untrusted: true })
            .collect();
        Ok(Response::ContactGroups { groups })
    }

    /// What `contacts.suggest_recipients` ranks for `query` (issue #460,
    /// [`torro_contacts::suggest::rank`]): the contacts of the books `reach` sees whose search text
    /// holds every word, and the groups whose name may match, each with its members; at most
    /// [`SUGGEST_CANDIDATES`] contacts, and as many per group. A client sees its books as everywhere
    /// ([`Rights::reaches_address_book`]: a hidden one only when named); the suite every book with
    /// "Recently contacted", and Nextcloud's system book only while the person shows hidden books
    /// (`show_hidden`, setting `contacts.show_hidden_books`): it holds every user of the instance.
    /// Books that are not hidden come first, so a person in "Recently contacted" too is suggested
    /// as the contact of a real book.
    pub fn suggest_candidates(
        &self,
        reach: Reach<'_>,
        query: &str,
        show_hidden: bool,
    ) -> Result<(Vec<Contact>, Vec<GroupCandidate>), ContactsError> {
        let words = torro_contacts::suggest::search_words(query);
        let books = self.suggest_books(reach, show_hidden)?;
        if words.is_empty() || books.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let ids: Vec<String> = books.iter().map(|b| b.id.clone()).collect();
        let contacts_of = |s: &Store, rows: Vec<CardRow>| -> Vec<Contact> {
            let mut contacts: Vec<Contact> = rows
                .iter()
                .filter_map(|row| books.iter().find(|b| b.id == row.book).map(|book| contact_of(s, row, book)))
                .collect();
            // The store sorts by name; the books' order decides who stands first for one address.
            contacts.sort_by_key(|contact| books.iter().position(|b| b.id == contact.book));
            contacts
        };
        with(self.store()?, |s| {
            let query = Query {
                books: ids.clone(),
                text: Some(words.join(" ")),
                limit: SUGGEST_CANDIDATES,
                ..Query::default()
            };
            let mut contacts = contacts_of(s, s.query(&query)?.0);
            // Favourites come before every other contact: never cut off by the names before them.
            let favourites = Query { favorites: true, ..query.clone() };
            for contact in contacts_of(s, s.query(&favourites)?.0) {
                if !contacts.iter().any(|held| held.id == contact.id) {
                    contacts.push(contact);
                }
            }
            let mut groups = Vec::new();
            for (book, name, _) in s.groups(&ids)? {
                if groups.len() >= SUGGEST_CANDIDATES || !torro_contacts::suggest::matches(&words, &name, "") {
                    continue;
                }
                let members = Query {
                    books: vec![book],
                    group: Some(name.clone()),
                    limit: SUGGEST_CANDIDATES,
                    ..Query::default()
                };
                groups.push(GroupCandidate { name, members: contacts_of(s, s.query(&members)?.0) });
            }
            Ok((contacts, groups))
        })
    }

    /// The books of the suggestions ([`ContactsService::suggest_candidates`]): not hidden ones first.
    fn suggest_books(&self, reach: Reach<'_>, show_hidden: bool) -> Result<Vec<BookRow>, ContactsError> {
        let mut books: Vec<BookRow> = self
            .books_within(reach, None)?
            .into_iter()
            .filter(|book| match reach {
                Reach::Suite => show_hidden || !(book.hidden && book.name == SYSTEM_BOOK),
                Reach::Client(_) => true,
            })
            .collect();
        books.sort_by_key(|book| book.hidden);
        Ok(books)
    }

    /// The contacts of the suggestions' books (as [`ContactsService::suggest_candidates`] has them)
    /// that hold one of `addresses` as a mail address, whatever its case (issue #460): the people of
    /// the mail, so a contact written to often ranks even when the search found others first, and
    /// its address is never offered as one "known only from mail".
    pub fn contacts_with(
        &self,
        reach: Reach<'_>,
        addresses: &[String],
        show_hidden: bool,
    ) -> Result<Vec<Contact>, ContactsError> {
        let books = self.suggest_books(reach, show_hidden)?;
        if addresses.is_empty() || books.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = books.iter().map(|b| b.id.clone()).collect();
        with(self.store()?, |s| {
            let mut found: Vec<Contact> = Vec::new();
            for address in addresses {
                let wanted = address.trim().to_lowercase();
                let query = Query { books: ids.clone(), text: Some(wanted.clone()), limit: 50, ..Query::default() };
                for row in s.query(&query)?.0 {
                    let Some(book) = books.iter().find(|b| b.id == row.book) else { continue };
                    let contact = contact_of(s, &row, book);
                    let holds = contact.emails.iter().any(|email| email.value.trim().to_lowercase() == wanted);
                    if holds && !found.iter().any(|known| known.id == contact.id) {
                        found.push(contact);
                    }
                }
            }
            Ok(found)
        })
    }

    fn birthdays(&self, reach: Reach<'_>, days: Option<u32>) -> Result<Response, ContactsError> {
        let days = days.unwrap_or(BIRTHDAY_DAYS);
        if days > MAX_BIRTHDAY_DAYS {
            return Err(ContactsError::Other(
                trf("Look at most {n} days ahead.", &[("n", &MAX_BIRTHDAY_DAYS)]).to_string(),
            ));
        }
        let books = self.books_within(reach, None)?;
        let ids: Vec<String> = books.iter().map(|b| b.id.clone()).collect();
        let today = Local::now().date_naive();
        let mut birthdays: Vec<Birthday> = with(self.store()?, |s| {
            Ok(s.with_birthday(&ids)?
                .iter()
                .filter_map(|row| {
                    let book = books.iter().find(|b| b.id == row.book)?;
                    Birthday::upcoming(&contact_of(s, row, book), today, days)
                })
                .collect())
        })?;
        birthdays.sort_by(|a, b| (a.next, &a.contact.display_name).cmp(&(b.next, &b.contact.display_name)));
        Ok(Response::Birthdays { birthdays })
    }

    /// Sends the queued change of `card` now and then brings `book` in step (plan of issue #351:
    /// a round after every write); the caller holds the lock of the rounds ([`ContactsService::handle`]). `None` when the change could not go yet (offline, no connection, a passing
    /// failure): it waits in the queue.
    async fn send_now(&self, book: &BookRow, card: &str) -> Result<Option<Sent>, ContactsError> {
        let store = self.store()?;
        let Some(remote) = self.remote(&book.account).await else { return Ok(None) };
        let sent = match sync::send_card(store, &remote, book, card).await {
            Ok(Sent::Waits) => return Ok(None),
            Ok(sent) => sent,
            Err(ContactsError::Auth) => {
                self.refused(&book.account).await;
                return Ok(None);
            }
            Err(_) => return Ok(None),
        };
        if let Some(fresh) = with(store, |s| s.book(&book.id))?
            && let Err(err) = sync::sync_book(store, &remote, &fresh).await
            && err != ContactsError::Offline
        {
            eprintln!("torroworkplace: contacts: a book did not sync after a change: {err}");
        }
        Ok(Some(sent))
    }

    /// A round for `book` now; the caller holds the lock of the rounds.
    async fn push(&self, book: &BookRow) {
        let Ok(store) = self.store() else { return };
        let Some(remote) = self.remote(&book.account).await else { return };
        match sync::sync_book(store, &remote, book).await {
            Ok(()) | Err(ContactsError::Offline) => {}
            Err(ContactsError::Auth) => self.refused(&book.account).await,
            Err(err) => eprintln!("torroworkplace: contacts: a change waits: {err}"),
        }
    }

    /// Writes `vcard` as the new version of `card`, queues it and sends it. Answers the contact as
    /// it is then (written, or still `pending` when it could not go yet); a newer version the server
    /// keeps is `conflict`, a refusal of the server its error.
    async fn write(&self, card: CardRow, book: &BookRow, vcard: String) -> Result<Response, ContactsError> {
        let id = card.id.clone();
        with(self.store()?, |s| {
            s.put_card(&CardRow { vcard, state: ContactState::Pending, ..card.clone() })?;
            s.enqueue(&Queued {
                card: card.id.clone(),
                book: card.book.clone(),
                op: Op::Put,
                uid: card.uid.clone(),
                href: card.href.clone(),
                etag: card.etag.clone(),
            })
        })?;
        match self.send_now(book, &id).await? {
            // The round after it may have merged the change after all (a 412 whose GET failed).
            Some(Sent::Conflict) => {
                let state = with(self.store()?, |s| s.card(&id))?.map(|card| card.state);
                match state {
                    Some(ContactState::Conflict) | None => {
                        Err(ContactsError::Conflict { server_vcard: None, etag: None })
                    }
                    Some(_) => self.answer(&id, book),
                }
            }
            Some(Sent::Refused(err)) => Err(err),
            Some(Sent::Written | Sent::Waits) | None => self.answer(&id, book),
        }
    }

    fn answer(&self, id: &str, book: &BookRow) -> Result<Response, ContactsError> {
        let contact = with(self.store()?, |s| Ok(s.card(id)?.map(|card| contact_of(s, &card, book))))?;
        contact.map(|contact| Response::Contact { contact: Box::new(contact) }).ok_or(ContactsError::NotFound)
    }

    fn writable(book: &BookRow) -> Result<(), ContactsError> {
        if book.writable { Ok(()) } else { Err(ContactsError::ReadOnly) }
    }

    fn no_group_cards(book: &BookRow, fields: Option<&ContactFields>) -> Result<(), ContactsError> {
        if book.group_cards && fields.is_none_or(|f| f.groups.is_some()) {
            return Err(ContactsError::Unsupported(
                tr("Groups of this address book cannot be changed here yet.").to_string(),
            ));
        }
        Ok(())
    }

    async fn create(&self, book_id: &str, fields: &ContactFields) -> Result<Response, ContactsError> {
        let book = self.book(book_id)?;
        Self::writable(&book)?;
        Self::no_group_cards(&book, Some(fields))?;
        let mut probe = Contact::default();
        fields.apply(&mut probe).map_err(fields_error)?;
        if probe.shown_name().is_empty() {
            return Err(ContactsError::Other(
                tr("A contact needs a name, a company, a mail address or a number.").to_string(),
            ));
        }
        let version = version_of(&with(self.store()?, |s| s.cards(&book.id))?);
        let uid = vcard::new_uid().map_err(ContactsError::Other)?;
        let (raw, _) = vcard::new_vcard(&uid, fields, version).map_err(fields_error)?;
        let card = CardRow {
            id: contact_id(&book.id, &uid),
            book: book.id.clone(),
            uid,
            href: None,
            etag: None,
            vcard: raw.clone(),
            base: None,
            state: ContactState::Pending,
            server: None,
            server_etag: None,
        };
        self.write(card, &book, raw).await
    }

    /// The contact `id` ready to change: in reach was checked, here its version, its book and its
    /// state.
    fn changeable(&self, id: &str, etag: Option<&str>) -> Result<Found, ContactsError> {
        let found = self.find(id)?;
        if let Some(etag) = etag
            && !same_version(&found.card, etag)
        {
            return Err(ContactsError::Conflict {
                server_vcard: None,
                etag: shown_etag(&found.card).map(str::to_string),
            });
        }
        Self::writable(&found.book)?;
        if found.card.state == ContactState::Conflict {
            return Err(ContactsError::Unsupported(
                tr("The contact changed here and on the server; settle that first (contacts.resolve_conflict).")
                    .to_string(),
            ));
        }
        Ok(found)
    }

    async fn add_to(&self, id: &str, fields: &ContactFields) -> Result<Response, ContactsError> {
        let found = self.changeable(id, None)?;
        Self::no_group_cards(&found.book, Some(fields))?;
        let before = vcard::read(&found.card.vcard).map_err(|_| ContactsError::NotFound)?;
        let mut after = before.clone();
        fields.add_to(&mut after).map_err(fields_error)?;
        let raw = vcard::patch(&found.card.vcard, &before, &after);
        self.write(found.card, &found.book, raw).await
    }

    async fn change(
        &self,
        id: &str,
        etag: &str,
        change: impl FnOnce(&BookRow, &mut Contact) -> Result<(), ContactsError>,
    ) -> Result<Response, ContactsError> {
        let found = self.changeable(id, Some(etag))?;
        let before = vcard::read(&found.card.vcard).map_err(|_| ContactsError::NotFound)?;
        let mut after = before.clone();
        change(&found.book, &mut after)?;
        let raw = vcard::patch(&found.card.vcard, &before, &after);
        self.write(found.card, &found.book, raw).await
    }

    async fn group(&self, id: &str, etag: &str, group: &str, add: bool) -> Result<Response, ContactsError> {
        let group = group.trim().to_string();
        if group.is_empty() {
            return Err(ContactsError::Other(tr("A group needs a name.").to_string()));
        }
        self.change(id, etag, |book, contact| {
            Self::no_group_cards(book, None)?;
            if add {
                if !contact.groups.contains(&group) {
                    contact.groups.push(group);
                }
            } else {
                contact.groups.retain(|g| *g != group);
            }
            Ok(())
        })
        .await
    }

    fn set_favorite(&self, id: &str, favorite: bool) -> Result<Response, ContactsError> {
        let found = self.find(id)?;
        with(self.store()?, |s| s.set_favorite(id, &found.book.account, favorite))?;
        Ok(Response::Ok)
    }

    fn mark_used(&self, id: &str) -> Result<Response, ContactsError> {
        let found = self.find(id)?;
        with(self.store()?, |s| s.mark_used(id, &found.book.account, Utc::now().timestamp_millis()))?;
        Ok(Response::Ok)
    }

    async fn set_photo(&self, id: &str, etag: &str, data: Option<&str>) -> Result<Response, ContactsError> {
        let photo = match data {
            None => None,
            Some(text) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(text.trim())
                    .map_err(|_| ContactsError::Other(tr("The picture is not Base64.").to_string()))?;
                Some(photo::prepare(&bytes).map_err(|err| {
                    ContactsError::Other(match err {
                        photo::PhotoError::TooLarge => trf(
                            "The picture is larger than {kb} KB.",
                            &[("kb", &(torro_ipc::contacts::MAX_PHOTO_BYTES / 1024))],
                        ),
                        photo::PhotoError::NotAPicture => tr("The picture is not a PNG or JPEG file.").to_string(),
                    })
                })?)
            }
        };
        let found = self.changeable(id, Some(etag))?;
        let raw = vcard::set_photo(&found.card.vcard, photo.as_ref());
        self.write(found.card, &found.book, raw).await
    }

    async fn move_contact(&self, id: &str, etag: &str, to_book: &str) -> Result<Response, ContactsError> {
        let found = self.changeable(id, Some(etag))?;
        let target = self.book(to_book)?;
        Self::writable(&target)?;
        if target.id == found.book.id {
            return Err(ContactsError::Unsupported(tr("The contact is in this address book already.").to_string()));
        }
        let new_id = contact_id(&target.id, &found.card.uid);
        if with(self.store()?, |s| s.card(&new_id))?.is_some() {
            return Err(ContactsError::Conflict { server_vcard: None, etag: None });
        }
        let remote = self.remote(&target.account).await.ok_or(ContactsError::Offline)?;
        let Stored { href, etag: new_etag } =
            remote.put(&target.href, None, &found.card.uid, &found.card.vcard, None).await.inspect_err(|err| {
                if *err == ContactsError::Auth {
                    eprintln!("torroworkplace: contacts: the target of a move refused the sign-in");
                }
            })?;
        let moved = CardRow {
            id: new_id.clone(),
            book: target.id.clone(),
            uid: found.card.uid.clone(),
            href: Some(href),
            etag: new_etag,
            vcard: found.card.vcard.clone(),
            base: Some(found.card.vcard.clone()),
            state: ContactState::Synced,
            server: None,
            server_etag: None,
        };
        let favorite = with(self.store()?, |s| s.is_favorite(id))?;
        with(self.store()?, |s| {
            s.put_card(&moved)?;
            if favorite {
                s.set_favorite(&new_id, &target.account, true)?;
            }
            Ok(())
        })?;
        // Then out of the old book; when that is refused, the contact is in both.
        let left_in_source = match &found.card.href {
            None => false,
            Some(href) => match self.remote(&found.book.account).await {
                Some(source) => source.delete(&found.book.href, href, found.card.etag.as_deref()).await.is_err(),
                None => true,
            },
        };
        if !left_in_source {
            with(self.store()?, |s| s.remove_card(id))?;
        }
        let contact = with(self.store()?, |s| Ok(contact_of(s, &moved, &target)))?;
        Ok(Response::ContactMoved(Box::new(Moved { contact, left_in_source })))
    }

    async fn delete(&self, id: &str, etag: &str) -> Result<Response, ContactsError> {
        let found = self.find(id)?;
        if !same_version(&found.card, etag) {
            return Err(ContactsError::Conflict {
                server_vcard: None,
                etag: shown_etag(&found.card).map(str::to_string),
            });
        }
        Self::writable(&found.book)?;
        with(self.store()?, |s| {
            s.remove_card(id)?;
            if let Some(href) = &found.card.href {
                s.enqueue(&Queued {
                    card: id.to_string(),
                    book: found.book.id.clone(),
                    op: Op::Delete,
                    uid: found.card.uid.clone(),
                    href: Some(href.clone()),
                    etag: shown_etag(&found.card).map(str::to_string),
                })?;
            }
            Ok(())
        })?;
        match self.send_now(&found.book, id).await? {
            // Changed on the server meanwhile: the round after it brought it back. Gone there too
            // (a 412 for a card no longer there): deleted all the same.
            Some(Sent::Conflict) => match with(self.store()?, |s| s.card(id))? {
                Some(_) => Err(ContactsError::Conflict { server_vcard: None, etag: None }),
                None => Ok(Response::Ok),
            },
            // Refused: the contact stays, here as there.
            Some(Sent::Refused(err)) => {
                with(self.store()?, |s| s.put_card(&found.card))?;
                Err(err)
            }
            Some(Sent::Written | Sent::Waits) | None => Ok(Response::Ok),
        }
    }

    async fn resolve(&self, id: &str, keep: Keep, etag: &str) -> Result<Response, ContactsError> {
        let found = self.find(id)?;
        if !same_version(&found.card, etag) {
            return Err(ContactsError::Conflict {
                server_vcard: None,
                etag: shown_etag(&found.card).map(str::to_string),
            });
        }
        Self::writable(&found.book)?;
        if found.card.state != ContactState::Conflict {
            return Err(ContactsError::Unsupported(tr("The contact is not in conflict.").to_string()));
        }
        let card = found.card.clone();
        let book = found.book.clone();
        // The server's version as the store keeps it; `None` when the server deleted the contact.
        let theirs = |s: &Store| -> Result<(), super::store::StoreError> {
            s.dequeue(&card.id)?;
            match &card.server {
                Some(raw) => s.put_card(&CardRow {
                    vcard: raw.clone(),
                    base: Some(raw.clone()),
                    etag: card.server_etag.clone(),
                    state: ContactState::Synced,
                    server: None,
                    server_etag: None,
                    ..card.clone()
                }),
                None => s.remove_card(&card.id),
            }
        };
        let mut ids = vec![card.id.clone()];
        match keep {
            Keep::Theirs => with(self.store()?, theirs)?,
            Keep::Mine => {
                // This device's version written over the server's: based on the server's, so the
                // merge on the way finds nothing to keep of it.
                let (href, etag) = match &card.server {
                    Some(_) => (card.href.clone(), card.server_etag.clone()),
                    None => (None, None),
                };
                with(self.store()?, |s| {
                    s.put_card(&CardRow {
                        base: card.server.clone(),
                        href: href.clone(),
                        etag: etag.clone(),
                        state: ContactState::Pending,
                        server: None,
                        server_etag: None,
                        ..card.clone()
                    })?;
                    s.enqueue(&Queued {
                        card: card.id.clone(),
                        book: book.id.clone(),
                        op: Op::Put,
                        uid: card.uid.clone(),
                        href,
                        etag,
                    })
                })?;
            }
            Keep::Both => {
                let uid = vcard::new_uid().map_err(ContactsError::Other)?;
                let copy_id = contact_id(&book.id, &uid);
                let copy = CardRow {
                    id: copy_id.clone(),
                    uid: uid.clone(),
                    href: None,
                    etag: None,
                    vcard: vcard::set_uid(&card.vcard, &uid),
                    base: None,
                    state: ContactState::Pending,
                    server: None,
                    server_etag: None,
                    book: book.id.clone(),
                };
                with(self.store()?, |s| {
                    theirs(s)?;
                    s.put_card(&copy)?;
                    s.enqueue(&Queued {
                        card: copy_id.clone(),
                        book: book.id.clone(),
                        op: Op::Put,
                        uid,
                        href: None,
                        etag: None,
                    })
                })?;
                ids.push(copy_id);
            }
        }
        self.push(&book).await;
        let contacts = with(self.store()?, |s| {
            Ok(ids.iter().filter_map(|id| s.card(id).ok().flatten()).map(|card| contact_of(s, &card, &book)).collect())
        })?;
        Ok(Response::ContactsResolved { contacts })
    }

    /// The books in reach that are not hidden: where people "already in the address books" are
    /// looked for, and what an export without a book and a search for duplicates read.
    fn shown_books(&self, reach: Reach<'_>) -> Result<Vec<BookRow>, ContactsError> {
        Ok(self.books_within(reach, None)?.into_iter().filter(|book| !book.hidden).collect())
    }

    /// The `UID`s of `target` and the people of the books in reach that are not hidden and of
    /// `target` ([`Known`]).
    fn known(&self, reach: Reach<'_>, target: &BookRow) -> Result<Known, ContactsError> {
        let mut books = self.shown_books(reach)?;
        if !books.iter().any(|book| book.id == target.id) {
            books.push(target.clone());
        }
        with(self.store()?, |s| {
            let mut known = Known::default();
            for book in &books {
                for card in s.cards(&book.id)? {
                    if book.id == target.id {
                        known.uids.insert(card.uid.clone());
                    }
                    if vcard::group_card(&card.vcard).is_none()
                        && let Ok(contact) = vcard::read(&card.vcard)
                    {
                        known.add(&contact, Some(&card.id));
                    }
                }
            }
            Ok(known)
        })
    }

    /// Stores `raw` as the new contact `uid` of `book` and queues it, without sending yet.
    fn store_new(&self, book: &BookRow, uid: &str, raw: &str) -> Result<CardRow, ContactsError> {
        let card = CardRow {
            id: contact_id(&book.id, uid),
            book: book.id.clone(),
            uid: uid.to_string(),
            href: None,
            etag: None,
            vcard: raw.to_string(),
            base: None,
            state: ContactState::Pending,
            server: None,
            server_etag: None,
        };
        with(self.store()?, |s| {
            s.put_card(&card)?;
            s.enqueue(&Queued {
                card: card.id.clone(),
                book: book.id.clone(),
                op: Op::Put,
                uid: uid.to_string(),
                href: None,
                etag: None,
            })
        })?;
        Ok(card)
    }

    /// `contacts.import_vcards` (issue #725): the cards of `data` or of the file at `path` into
    /// `book_id`; with `dry_run` only counted. More than [`MAX_IMPORT_CARDS`] cards write nothing.
    async fn import(
        &self,
        reach: Reach<'_>,
        book_id: &str,
        data: Option<String>,
        path: Option<String>,
        dry_run: bool,
    ) -> Result<Response, ContactsError> {
        let book = self.book(book_id)?;
        Self::writable(&book)?;
        let text = match (data, path) {
            (Some(data), None) => data,
            (None, Some(path)) => tokio::task::spawn_blocking(move || read_vcf(&path))
                .await
                .map_err(|err| ContactsError::Other(err.to_string()))??,
            _ => return Err(ContactsError::Other(tr("Send the cards either as `data` or as `path`.").to_string())),
        };
        let cards = vcard::split(&text);
        if cards.len() > MAX_IMPORT_CARDS {
            return Err(ContactsError::Other(trf(
                "The file holds more than {n} contacts; split it and import the parts.",
                &[("n", &MAX_IMPORT_CARDS)],
            )));
        }
        let mut known = self.known(reach, &book)?;
        let mut imported = Imported::default();
        let mut new = Vec::new();
        for raw in &cards {
            match known.sort(raw)? {
                Card::Skipped => imported.skipped += 1,
                Card::Known(_) => imported.duplicates += 1,
                Card::New { uid, raw } => {
                    imported.created += 1;
                    new.push((uid, raw));
                }
            }
        }
        if dry_run || new.is_empty() {
            return Ok(Response::ContactsImported(imported));
        }
        for (uid, raw) in &new {
            self.store_new(&book, uid, raw)?;
        }
        // One round sends them all; offline they wait (`state: pending`).
        self.push(&book).await;
        Ok(Response::ContactsImported(imported))
    }

    /// `contacts.export_vcards` (issue #725): the cards as stored, each ending its last line.
    fn export(
        &self,
        reach: Reach<'_>,
        book: Option<&str>,
        group: Option<&str>,
        ids: &[String],
    ) -> Result<Response, ContactsError> {
        let cards: Vec<CardRow> = if !ids.is_empty() {
            ids.iter().map(|id| self.find(id).map(|found| found.card)).collect::<Result<_, _>>()?
        } else {
            let books: Vec<String> = match book {
                Some(book) => vec![self.book(book)?.id],
                None => self.shown_books(reach)?.into_iter().map(|b| b.id).collect(),
            };
            with(self.store()?, |s| {
                let mut cards = Vec::new();
                for book in &books {
                    cards.extend(s.cards(book)?);
                }
                Ok(cards)
            })?
            .into_iter()
            .filter(|card| {
                group.is_none_or(|group| {
                    // Without case, as the contacts app puts groups together.
                    vcard::read(&card.vcard).is_ok_and(|contact| {
                        contact.groups.iter().any(|g| g.trim().to_lowercase() == group.trim().to_lowercase())
                    })
                })
            })
            .collect()
        };
        let mut data = String::new();
        for card in &cards {
            data.push_str(&card.vcard);
            if !card.vcard.ends_with('\n') {
                data.push_str("\r\n");
            }
        }
        Ok(Response::VCards { data, count: u32::try_from(cards.len()).unwrap_or(u32::MAX) })
    }

    /// The contacts of `books` as the API answers them, without group cards.
    fn contacts_in(&self, books: &[BookRow]) -> Result<Vec<Contact>, ContactsError> {
        with(self.store()?, |s| {
            let mut contacts = Vec::new();
            for book in books {
                for card in s.cards(&book.id)? {
                    if vcard::group_card(&card.vcard).is_none() {
                        contacts.push(contact_of(s, &card, book));
                    }
                }
            }
            Ok(contacts)
        })
    }

    /// `contacts.find_duplicates` (issue #726): in `book` (also a hidden one), else in the books in
    /// reach that are not hidden.
    fn find_duplicates(&self, reach: Reach<'_>, book: Option<&str>) -> Result<Response, ContactsError> {
        let books = match book {
            Some(book) => vec![self.book(book)?],
            None => self.shown_books(reach)?,
        };
        let contacts = self.contacts_in(&books)?;
        let dismissed = with(self.store()?, |s| s.not_duplicates())?;
        Ok(Response::ContactDuplicates { groups: duplicates::find(&contacts, country(), &dismissed) })
    }

    /// `contacts.dismiss_duplicates` (issue #726).
    fn dismiss(&self, ids: &[String]) -> Result<Response, ContactsError> {
        for id in ids {
            self.find(id)?;
        }
        with(self.store()?, |s| s.dismiss_duplicates(ids))?;
        Ok(Response::Ok)
    }

    /// `contacts.merge_contacts` (issue #726): every contact checked first (its version, a writable
    /// book, no conflict), then `keep` written with what the others bring, then the others deleted
    /// as `contacts.delete_contact` deletes; one whose deletion fails is named in `left`. A favourite
    /// stays one, a picture comes from the first other when `keep` has none.
    async fn merge(&self, ids: &[String], etags: &[String], keep: &str) -> Result<Response, ContactsError> {
        let mut found = Vec::new();
        for (id, etag) in ids.iter().zip(etags) {
            let one = self.find(id)?;
            if !one.book.writable {
                return Err(ContactsError::Unsupported(trf(
                    "The address book “{book}” is read-only.",
                    &[("book", &one.book.display_name)],
                )));
            }
            found.push((self.changeable(id, Some(etag))?, etag.clone()));
        }
        let at = found.iter().position(|(one, _)| one.card.id == keep).ok_or(ContactsError::NotFound)?;
        let (kept, _) = found.remove(at);
        let before = vcard::read(&kept.card.vcard).map_err(|_| ContactsError::NotFound)?;
        let others: Vec<Contact> = found
            .iter()
            .map(|(one, _)| vcard::read(&one.card.vcard).map_err(|_| ContactsError::NotFound))
            .collect::<Result<_, _>>()?;
        let after = duplicates::merged(&before, &others, country());
        if after.groups != before.groups {
            Self::no_group_cards(&kept.book, None)?;
        }
        let mut raw = vcard::patch(&kept.card.vcard, &before, &after);
        if vcard::photo(&kept.card.vcard).is_none()
            && let Some(photo) = found.iter().find_map(|(one, _)| vcard::photo(&one.card.vcard))
        {
            raw = vcard::set_photo(&raw, Some(&photo));
        }
        let favorite = with(self.store()?, |s| {
            let mut any = false;
            for (one, _) in &found {
                any |= s.is_favorite(&one.card.id)?;
            }
            Ok(any)
        })?;
        let (keep_id, keep_book) = (kept.card.id.clone(), kept.book.clone());
        self.write(kept.card, &keep_book, raw).await?;
        if favorite {
            with(self.store()?, |s| s.set_favorite(&keep_id, &keep_book.account, true))?;
        }
        let mut left = Vec::new();
        for (one, etag) in found {
            if Box::pin(self.delete(&one.card.id, &etag)).await.is_err() {
                left.push(one.card.id);
            }
        }
        let contact = with(self.store()?, |s| Ok(s.card(&keep_id)?.map(|card| contact_of(s, &card, &keep_book))))?
            .ok_or(ContactsError::NotFound)?;
        Ok(Response::ContactsMerged(Box::new(Merged { contact, left })))
    }

    /// `inbox.create_contact_from_item` with a vCard attached (issue #727): the first card of `text`
    /// into `book_id` as `contacts.import_vcards` would take it; a person there already is answered
    /// with `created: false` and the contact that holds it. The caller holds no lock; this takes the
    /// lock of the rounds.
    pub async fn contact_from_card(
        &self,
        reach: Reach<'_>,
        book_id: &str,
        text: &str,
    ) -> Result<FromItem, ContactsError> {
        let _round = self.rounds.lock().await;
        let book = self.book(book_id)?;
        Self::writable(&book)?;
        let raw = vcard::split(text).into_iter().next().unwrap_or_default();
        let mut known = self.known(reach, &book)?;
        match known.sort(&raw)? {
            Card::Skipped => Err(ContactsError::Other(tr("The attached vCard holds no contact.").to_string())),
            // Known by its UID in `book`, or by a contact of the books holding the person.
            Card::Known(held) => self.held(held.or_else(|| {
                vcard::read(&raw).ok().filter(|card| !card.uid.is_empty()).map(|card| contact_id(&book.id, &card.uid))
            })),
            // Kept as the mail brought it, as an import keeps a file's card.
            Card::New { uid, raw } => {
                let card = self.store_new(&book, &uid, &raw)?;
                let contact = match self.send_now(&book, &card.id).await? {
                    Some(Sent::Refused(err)) => return Err(err),
                    _ => self.answer(&card.id, &book)?,
                };
                Ok(FromItem { contact: contact_in(contact)?, created: true })
            }
        }
    }

    /// The contact `held` a card or a sender matched, as [`FromItem`] with `created: false`.
    fn held(&self, held: Option<String>) -> Result<FromItem, ContactsError> {
        let found = self.find(&held.ok_or(ContactsError::NotFound)?)?;
        let contact = with(self.store()?, |s| Ok(contact_of(s, &found.card, &found.book)))?;
        Ok(FromItem { contact, created: false })
    }

    /// `inbox.create_contact_from_item` without a vCard (issue #727): `name` and `address` of a
    /// mail's sender into `book_id`, as "+ Contact" of the compose window makes one; a contact of a
    /// book in reach that is not hidden holding the address already is answered with
    /// `created: false`. Takes the lock of the rounds.
    pub async fn contact_from_sender(
        &self,
        reach: Reach<'_>,
        book_id: &str,
        name: &str,
        address: &str,
    ) -> Result<FromItem, ContactsError> {
        let _round = self.rounds.lock().await;
        let book = self.book(book_id)?;
        Self::writable(&book)?;
        let wanted = address.trim().to_lowercase();
        if wanted.is_empty() {
            return Err(ContactsError::Other(tr("The mail names no sender.").to_string()));
        }
        let books = self.shown_books(reach)?;
        let ids: Vec<String> = books.iter().map(|b| b.id.clone()).collect();
        let held = with(self.store()?, |s| {
            let query = Query { books: ids.clone(), text: Some(wanted.clone()), limit: 50, ..Query::default() };
            Ok(s.query(&query)?.0.into_iter().find(|row| {
                vcard::read(&row.vcard)
                    .is_ok_and(|contact| contact.emails.iter().any(|email| email.value.trim().to_lowercase() == wanted))
            }))
        })?;
        if let Some(row) = held {
            return self.held(Some(row.id));
        }
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        let fields = ContactFields {
            display_name: (!name.is_empty()).then_some(name),
            emails: Some(vec![Labeled::new(address.trim().to_string(), FieldKind::Other)]),
            ..ContactFields::default()
        };
        let contact = contact_in(Box::pin(self.create(&book.id, &fields)).await?)?;
        Ok(FromItem { contact, created: true })
    }

    /// The facts of a contacts request for its approval: the contact's name and book from the
    /// store, the target book, and what a change replaces. `None` for other requests.
    pub fn facts(&self, request: &Request) -> Option<crate::approvals::ContactFacts> {
        let store = self.store().ok()?;
        let named = |id: &str| -> (Option<Contact>, Option<BookRow>) {
            match self.find(id) {
                Ok(found) => {
                    let contact = with(store, |s| Ok(contact_of(s, &found.card, &found.book))).ok();
                    (contact, Some(found.book))
                }
                Err(_) => (None, None),
            }
        };
        let book_name = |id: &str| self.book(id).ok().map(|b| b.display_name);
        // Name and book of each contact named but `keep` (issue #726).
        let others = |ids: &[String], keep: Option<&str>| -> Vec<(String, String)> {
            ids.iter()
                .filter(|id| Some(id.as_str()) != keep)
                .filter_map(|id| match named(id) {
                    (Some(contact), Some(book)) => Some((contact.shown_name(), book.display_name)),
                    _ => None,
                })
                .collect()
        };
        let (contact, book, to_book) = match request {
            Request::CreateContact { book, .. } | Request::ImportVCards { book, .. } => (None, book_name(book), None),
            Request::MergeContacts { ids, keep, .. } => {
                let (contact, book) = named(keep);
                let others = others(ids, Some(keep));
                return Some(crate::approvals::ContactFacts {
                    contact,
                    book: book.map(|b| b.display_name),
                    others,
                    ..Default::default()
                });
            }
            Request::DismissDuplicates { ids } => {
                return Some(crate::approvals::ContactFacts { others: others(ids, None), ..Default::default() });
            }
            Request::MoveContact { id, to_book, .. } => {
                let (contact, book) = named(id);
                (contact, book.map(|b| b.display_name), book_name(to_book))
            }
            Request::AddToContact { id, .. }
            | Request::UpdateContact { id, .. }
            | Request::SetContactFavorite { id, .. }
            | Request::AddContactToGroup { id, .. }
            | Request::RemoveContactFromGroup { id, .. }
            | Request::MarkContactUsed { id }
            | Request::SetContactPhoto { id, .. }
            | Request::DeleteContact { id, .. }
            | Request::ResolveContactConflict { id, .. } => {
                let (contact, book) = named(id);
                (contact, book.map(|b| b.display_name), None)
            }
            _ => return None,
        };
        Some(crate::approvals::ContactFacts { contact, book, to_book, ..Default::default() })
    }

    /// What `contacts.import_vcards` with the cards of `data` into `book` would do for `reach` (issue
    /// #725): the trial run an approval shows. `None` for a file, which only the suite's apps name,
    /// and when it cannot be counted.
    pub fn import_facts(&self, reach: Reach<'_>, request: &Request) -> Option<Imported> {
        let Request::ImportVCards { book, data: Some(data), path: None, .. } = request else { return None };
        let book = self.book(book).ok()?;
        let mut known = self.known(reach, &book).ok()?;
        let mut imported = Imported::default();
        for raw in vcard::split(data) {
            match known.sort(&raw).ok()? {
                Card::Skipped => imported.skipped += 1,
                Card::Known(_) => imported.duplicates += 1,
                Card::New { .. } => imported.created += 1,
            }
        }
        Some(imported)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use torro_contacts::{FieldKind, Labeled};
    use torro_ipc::api::{RightsTemplate, Selection};

    use super::super::sync::fake::Server;
    use super::*;

    const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";

    fn card(uid: &str, name: &str, email: &str) -> String {
        format!(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid}\r\nFN:{name}\r\nN:{name};;;;\r\nEMAIL;TYPE=WORK:{email}\r\nBDAY:--{}\r\nX-KEEP:me\r\nEND:VCARD\r\n",
            Local::now().format("%m%d")
        )
    }

    struct Setup {
        _dir: tempfile::TempDir,
        server: Arc<Server>,
        service: ContactsService,
        work: String,
        private: String,
    }

    async fn setup() -> Setup {
        let dir = tempfile::tempdir().expect("dir");
        let server = Arc::new(Server::default());
        let work = server.add_book("contacts", true, false);
        let private = server.add_book("privat", true, false);
        let team = server.add_book("team", false, false);
        server.add_book("z-server-generated--system", false, true);
        server.put_card(&work, "anna", &card("anna", "Anna Berg", "anna@example.org"));
        server.put_card(&private, "secret", &card("secret", "Secret Person", "secret@example.org"));
        server.put_card(&team, "bob", &card("bob", "Bob Team", "bob@example.org"));
        let service = ContactsService::in_dir(dir.path(), vec![(ACCOUNT.into(), Arc::clone(&server))]);
        service.sync_all().await;
        Setup { _dir: dir, server, service, work, private }
    }

    fn id(book: &str, uid: &str) -> String {
        format!("{ACCOUNT}/{book}/{uid}")
    }

    fn contact(response: Response) -> Contact {
        match response {
            Response::Contact { contact } => *contact,
            other => panic!("not a contact: {other:?}"),
        }
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            _ => "ok",
        }
    }

    fn only_work() -> Rights {
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.address_books = Selection::Only(vec![format!("{ACCOUNT}/contacts"), format!("{ACCOUNT}/team")]);
        rights
    }

    /// Issue #460: the candidates of the recipient suggestions come from the books the caller sees –
    /// a client's by its rights, the suite's without the system book unless hidden books are shown –
    /// and a group brings its members.
    #[tokio::test]
    async fn suggestion_candidates_come_from_the_books_in_reach() {
        let dir = tempfile::tempdir().expect("dir");
        let server = Arc::new(Server::default());
        let work = server.add_book("contacts", true, false);
        let private = server.add_book("privat", true, false);
        let system = server.add_book("z-server-generated--system", false, true);
        let grouped = |uid: &str, name: &str, email: &str| {
            card(uid, name, email).replace("END:VCARD", "CATEGORIES:Jour fixe\r\nEND:VCARD")
        };
        server.put_card(&work, "jonas", &grouped("jonas", "Jonas Berger", "jonas@berger.de"));
        server.put_card(&work, "mira", &grouped("mira", "Mira Schulte", "mira@schulte.de"));
        server.put_card(&private, "joachim", &card("joachim", "Joachim Geheim", "joachim@example.org"));
        server.put_card(&system, "johann", &card("johann", "Johann Kollege", "johann@cloud.example.org"));
        let service = ContactsService::in_dir(dir.path(), vec![(ACCOUNT.into(), Arc::clone(&server))]);
        service.sync_all().await;
        let names = |found: &(Vec<Contact>, Vec<GroupCandidate>)| {
            let mut names: Vec<String> = found.0.iter().map(Contact::shown_name).collect();
            names.sort();
            names
        };

        let suite = service.suggest_candidates(Reach::Suite, "jo", false).expect("suite");
        assert_eq!(names(&suite), ["Joachim Geheim", "Jonas Berger"], "no system book unless shown");
        let shown = service.suggest_candidates(Reach::Suite, "jo", true).expect("shown");
        assert_eq!(names(&shown), ["Joachim Geheim", "Johann Kollege", "Jonas Berger"]);

        let rights = only_work();
        let client = service.suggest_candidates(Reach::Client(&rights), "jo", true).expect("client");
        assert_eq!(names(&client), ["Jonas Berger"], "only the books of its rights");

        let groups = service.suggest_candidates(Reach::Client(&rights), "jour", false).expect("groups").1;
        assert_eq!(groups.len(), 1);
        let mut members: Vec<String> = groups[0].members.iter().map(Contact::shown_name).collect();
        members.sort();
        assert_eq!(groups[0].name, "Jour fixe");
        assert_eq!(members, ["Jonas Berger", "Mira Schulte"], "a group brings its members");
        assert!(service.suggest_candidates(Reach::Suite, "  ", true).expect("blank").0.is_empty());
    }

    #[tokio::test]
    async fn a_client_without_a_book_finds_nothing_of_it() {
        let setup = setup().await;
        let rights = only_work();
        let reach = Reach::Client(&rights);
        let secret = id("privat", "secret");
        let list = setup.service.list_contacts(reach, torro_contacts::ContactQuery::default()).expect("list");
        assert!(list.contacts.iter().all(|c| c.id != secret));
        let found = setup
            .service
            .list_contacts(reach, torro_contacts::ContactQuery { query: Some("secret".into()), ..Default::default() })
            .expect("search");
        assert!(found.contacts.is_empty(), "a search filters before it reads");
        for request in [
            Request::GetContact { id: secret.clone() },
            Request::GetContactPhoto { id: secret.clone(), size: PhotoSize::Small },
            Request::AddToContact { id: secret.clone(), fields: ContactFields::default() },
            Request::UpdateContact { id: secret.clone(), etag: "x".into(), fields: ContactFields::default() },
            Request::SetContactFavorite { id: secret.clone(), favorite: true },
            Request::AddContactToGroup { id: secret.clone(), etag: "x".into(), group: "g".into() },
            Request::RemoveContactFromGroup { id: secret.clone(), etag: "x".into(), group: "g".into() },
            Request::MarkContactUsed { id: secret.clone() },
            Request::SetContactPhoto { id: secret.clone(), etag: "x".into(), data: None },
            Request::DeleteContact { id: secret.clone(), etag: "x".into() },
            Request::ResolveContactConflict { id: secret.clone(), keep: Keep::Both, etag: "x".into() },
            Request::MoveContact { id: id("contacts", "anna"), etag: "x".into(), to_book: format!("{ACCOUNT}/privat") },
            Request::MoveContact { id: secret.clone(), etag: "x".into(), to_book: format!("{ACCOUNT}/contacts") },
            Request::CreateContact { book: format!("{ACCOUNT}/privat"), fields: ContactFields::default() },
            Request::ListContactGroups { book: Some(format!("{ACCOUNT}/privat")) },
            Request::GetContact { id: id("contacts", "nobody") },
        ] {
            assert!(setup.service.out_of_reach(reach, &request).is_some(), "{request:?}");
        }
        assert_eq!(setup.service.out_of_reach(reach, &Request::GetContact { id: id("contacts", "anna") }), None);
        let groups = setup.service.handle(Request::UpcomingBirthdays { days: None }, reach).await;
        let Response::Birthdays { birthdays } = groups else { panic!("birthdays") };
        assert!(birthdays.iter().all(|b| b.contact.id != secret) && !birthdays.is_empty());
        let Response::AddressBooks { books } =
            setup.service.handle(Request::ListAddressBooks { account: None }, reach).await
        else {
            panic!("books")
        };
        assert_eq!(books.len(), 2, "neither the private nor the hidden book");
        let _ = &setup.private;
    }

    #[tokio::test]
    async fn adding_keeps_what_is_there_and_never_prefers() {
        let setup = setup().await;
        let anna = id("contacts", "anna");
        let fields = ContactFields {
            emails: Some(vec![Labeled { value: "new@example.org".into(), kind: FieldKind::Home, pref: true }]),
            ..ContactFields::default()
        };
        let added =
            contact(setup.service.handle(Request::AddToContact { id: anna.clone(), fields }, Reach::Suite).await);
        assert_eq!(added.emails[0], Labeled::new("anna@example.org".to_string(), FieldKind::Work));
        assert_eq!(added.emails[1], Labeled::new("new@example.org".to_string(), FieldKind::Home));
        assert_eq!(added.state, ContactState::Synced);
        let (_, written) = setup.server.card(&setup.work, &format!("{}anna.vcf", setup.work)).expect("card");
        assert!(written.contains("X-KEEP:me") && written.contains("new@example.org"), "{written}");
    }

    #[tokio::test]
    async fn changes_need_the_shown_version_and_a_writable_book() {
        let setup = setup().await;
        let anna = id("contacts", "anna");
        let fields = || ContactFields { org: Some("Acme".into()), ..ContactFields::default() };
        let stale = Request::UpdateContact { id: anna.clone(), etag: "\"old\"".into(), fields: fields() };
        assert_eq!(code(&setup.service.handle(stale, Reach::Suite).await), "conflict");
        let shown = contact(setup.service.handle(Request::GetContact { id: anna.clone() }, Reach::Suite).await);
        let update =
            Request::UpdateContact { id: anna.clone(), etag: shown.etag.clone().expect("etag"), fields: fields() };
        let changed = contact(setup.service.handle(update, Reach::Suite).await);
        assert_eq!(changed.org.as_deref(), Some("Acme"));
        let bob = id("team", "bob");
        let shown = contact(setup.service.handle(Request::GetContact { id: bob.clone() }, Reach::Suite).await);
        assert!(shown.readonly);
        let delete = Request::DeleteContact { id: bob.clone(), etag: shown.etag.clone().expect("etag") };
        assert_eq!(code(&setup.service.handle(delete, Reach::Suite).await), "unsupported");
        let favorite = Request::SetContactFavorite { id: bob.clone(), favorite: true };
        assert_eq!(setup.service.handle(favorite, Reach::Suite).await, Response::Ok, "a favourite stays here");
        assert!(contact(setup.service.handle(Request::GetContact { id: bob }, Reach::Suite).await).favorite);
    }

    /// Review of #351: keeping this device's version fails when the server changed once more since
    /// the conflict was shown, instead of writing over a version nobody saw.
    #[tokio::test]
    async fn settling_a_conflict_needs_the_server_version_shown() {
        let setup = setup().await;
        let anna = id("contacts", "anna");
        let shown = contact(setup.service.handle(Request::GetContact { id: anna.clone() }, Reach::Suite).await);
        setup.server.lock().offline = true;
        let note = |text: &str| ContactFields { note: Some(text.into()), ..ContactFields::default() };
        let update = Request::UpdateContact { id: anna.clone(), etag: shown.etag.expect("etag"), fields: note("mine") };
        contact(setup.service.handle(update, Reach::Suite).await);
        let theirs = |text: &str| {
            card("anna", "Anna Berg", "anna@example.org").replace("END:VCARD", &format!("NOTE:{text}\r\nEND:VCARD"))
        };
        setup.server.put_card(&setup.work, "anna", &theirs("theirs"));
        setup.server.lock().offline = false;
        setup.service.sync_all().await;
        let conflict = contact(setup.service.handle(Request::GetContact { id: anna.clone() }, Reach::Suite).await);
        assert_eq!(conflict.state, ContactState::Conflict);
        let seen = conflict.etag.clone().expect("etag");
        assert_eq!(Some(&seen), conflict.server.as_ref().and_then(|s| s.etag.as_ref()), "the version shown beside");
        setup.server.put_card(&setup.work, "anna", &theirs("theirs again"));
        setup.service.sync_all().await;
        let stale = Request::ResolveContactConflict { id: anna.clone(), keep: Keep::Mine, etag: seen };
        assert_eq!(code(&setup.service.handle(stale, Reach::Suite).await), "conflict", "a version nobody saw");
        let again = contact(setup.service.handle(Request::GetContact { id: anna.clone() }, Reach::Suite).await);
        let mine =
            Request::ResolveContactConflict { id: anna.clone(), keep: Keep::Mine, etag: again.etag.expect("etag") };
        assert!(matches!(setup.service.handle(mine, Reach::Suite).await, Response::ContactsResolved { .. }));
        let (_, written) = setup.server.card(&setup.work, &format!("{}anna.vcf", setup.work)).expect("card");
        assert!(written.contains("NOTE:mine"), "{written}");
    }

    /// Review of #351: groups kept as group cards (iCloud) are not written as `CATEGORIES`, by no
    /// command, `update_contact` included.
    #[tokio::test]
    async fn a_book_with_group_cards_refuses_group_changes_on_every_way() {
        let setup = setup().await;
        setup.server.put_card(
            &setup.work,
            "friends",
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:friends\r\nFN:Friends\r\nX-ADDRESSBOOKSERVER-KIND:group\r\nX-ADDRESSBOOKSERVER-MEMBER:urn:uuid:anna\r\nEND:VCARD\r\n",
        );
        setup.service.sync_all().await;
        let anna = id("contacts", "anna");
        let etag = || async {
            let shown = contact(setup.service.handle(Request::GetContact { id: anna.clone() }, Reach::Suite).await);
            shown.etag.expect("etag")
        };
        let groups = ContactFields { groups: Some(vec!["Friends".into()]), ..ContactFields::default() };
        let update = Request::UpdateContact { id: anna.clone(), etag: etag().await, fields: groups };
        assert_eq!(code(&setup.service.handle(update, Reach::Suite).await), "unsupported");
        let add = Request::AddContactToGroup { id: anna.clone(), etag: etag().await, group: "Friends".into() };
        assert_eq!(code(&setup.service.handle(add, Reach::Suite).await), "unsupported");
        let org = ContactFields { org: Some("Acme".into()), groups: Some(Vec::new()), ..ContactFields::default() };
        let update = Request::UpdateContact { id: anna.clone(), etag: etag().await, fields: org };
        assert_eq!(
            code(&setup.service.handle(update, Reach::Suite).await),
            "ok",
            "other fields still change, the groups sent as they are along with them"
        );
    }

    /// Review of #351: a contact made offline is changed with an empty `etag`; a refusal of the
    /// server comes back as its error, not as the old contact.
    #[tokio::test]
    async fn made_offline_changes_and_refusals_are_told() {
        let setup = setup().await;
        setup.server.lock().offline = true;
        let fields = ContactFields { display_name: Some("Max Neu".into()), ..ContactFields::default() };
        let created = contact(
            setup
                .service
                .handle(Request::CreateContact { book: format!("{ACCOUNT}/contacts"), fields }, Reach::Suite)
                .await,
        );
        assert_eq!(created.etag, None);
        let update = Request::UpdateContact {
            id: created.id.clone(),
            etag: String::new(),
            fields: ContactFields { org: Some("Acme".into()), ..ContactFields::default() },
        };
        let changed = contact(setup.service.handle(update, Reach::Suite).await);
        assert_eq!((changed.org.as_deref(), changed.state), (Some("Acme"), ContactState::Pending));
        setup.server.lock().offline = false;
        setup.server.lock().put_error = Some(ContactsError::TooLarge);
        let anna =
            contact(setup.service.handle(Request::GetContact { id: id("contacts", "anna") }, Reach::Suite).await);
        let note = Request::UpdateContact {
            id: anna.id.clone(),
            etag: anna.etag.clone().expect("etag"),
            fields: ContactFields { note: Some("long".into()), ..ContactFields::default() },
        };
        assert_eq!(code(&setup.service.handle(note, Reach::Suite).await), "error", "too large is an error");
        let kept = contact(setup.service.handle(Request::GetContact { id: anna.id }, Reach::Suite).await);
        assert_eq!((kept.note, kept.state), (None, ContactState::Synced), "taken back to the server's");
    }

    #[tokio::test]
    async fn create_offline_move_and_delete() {
        let setup = setup().await;
        setup.server.lock().offline = true;
        let fields = ContactFields { display_name: Some("Max Neu".into()), ..ContactFields::default() };
        let created = contact(
            setup
                .service
                .handle(Request::CreateContact { book: format!("{ACCOUNT}/contacts"), fields }, Reach::Suite)
                .await,
        );
        assert_eq!(created.state, ContactState::Pending, "kept until the server can be reached");
        setup.server.lock().offline = false;
        setup.service.sync_all().await;
        let created = contact(setup.service.handle(Request::GetContact { id: created.id }, Reach::Suite).await);
        assert_eq!(created.state, ContactState::Synced);
        let moved = setup
            .service
            .handle(
                Request::MoveContact {
                    id: created.id.clone(),
                    etag: created.etag.clone().expect("etag"),
                    to_book: format!("{ACCOUNT}/privat"),
                },
                Reach::Suite,
            )
            .await;
        let Response::ContactMoved(moved) = moved else { panic!("moved: {moved:?}") };
        assert!(!moved.left_in_source);
        assert_eq!(moved.contact.book, format!("{ACCOUNT}/privat"));
        assert_eq!(
            code(&setup.service.handle(Request::GetContact { id: created.id }, Reach::Suite).await),
            "not-found"
        );
        let gone =
            Request::DeleteContact { id: moved.contact.id.clone(), etag: moved.contact.etag.clone().expect("etag") };
        assert_eq!(setup.service.handle(gone, Reach::Suite).await, Response::Ok);
        assert!(setup.server.lock().books[&setup.private].3.keys().all(|href| !href.contains(&moved.contact.uid)));
    }

    fn imported(response: Response) -> Imported {
        match response {
            Response::ContactsImported(imported) => imported,
            other => panic!("not an import: {other:?}"),
        }
    }

    fn work_cards(setup: &Setup) -> Vec<String> {
        setup.server.lock().books[&setup.work].3.values().map(|(_, vcard)| vcard.clone()).collect()
    }

    /// Issue #725: an import counts new, known and unreadable cards alike in a trial run and for
    /// real, keeps each new card as written, never takes a person twice, and keeps to its limits.
    #[tokio::test]
    async fn an_import_leaves_out_people_there_and_keeps_cards_as_written() {
        let setup = setup().await;
        let book = format!("{ACCOUNT}/contacts");
        let file = [
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:kim1\r\nFN:Kim Neu\r\nEMAIL:kim@example.org\r\nX-OWN:kept\r\nEND:VCARD\r\n",
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:a2\r\nFN:Anna\r\nEMAIL:ANNA@example.org\r\nEND:VCARD\r\n",
            "BEGIN:VCARD\r\nVERSION:3.0\r\nNOTE:nobody\r\nEND:VCARD\r\n",
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:kim2\r\nFN:K. Neu\r\nEMAIL:Kim@Example.org\r\nEND:VCARD\r\n",
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:g\r\nFN:Friends\r\nX-ADDRESSBOOKSERVER-KIND:group\r\nEND:VCARD\r\n",
            "BEGIN:VCARD\nVERSION:2.1\nFN:Lars Ohne\nTEL;CELL:+49 170 1112223\nEND:VCARD\n",
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:anna\r\nFN:Someone Else\r\nEND:VCARD\r\n",
        ]
        .concat();
        let import =
            |dry_run: bool| Request::ImportVCards { book: book.clone(), data: Some(file.clone()), path: None, dry_run };
        let expected = Imported { created: 2, duplicates: 3, skipped: 2 };
        assert_eq!(imported(setup.service.handle(import(true), Reach::Suite).await), expected);
        assert_eq!(work_cards(&setup).len(), 1, "a trial run writes nothing");
        assert_eq!(imported(setup.service.handle(import(false), Reach::Suite).await), expected);
        let written = work_cards(&setup);
        assert_eq!(written.len(), 3);
        assert!(
            written.contains(&file.split_inclusive("END:VCARD\r\n").next().expect("first").to_string()),
            "byte for byte"
        );
        let lars = written.iter().find(|card| card.contains("Lars Ohne")).expect("lars");
        assert!(lars.contains("UID:") && lars.contains("TEL;CELL:+49 170 1112223"), "{lars}");
        let again = imported(setup.service.handle(import(false), Reach::Suite).await);
        assert_eq!(again, Imported { created: 0, duplicates: 5, skipped: 2 }, "nobody twice");

        let team = Request::ImportVCards {
            book: format!("{ACCOUNT}/team"),
            data: Some(file.clone()),
            path: None,
            dry_run: true,
        };
        assert_eq!(code(&setup.service.handle(team, Reach::Suite).await), "unsupported");
        let both = Request::ImportVCards {
            book: book.clone(),
            data: Some(file.clone()),
            path: Some("/x.vcf".into()),
            dry_run: true,
        };
        assert_eq!(code(&setup.service.handle(both, Reach::Suite).await), "error");
        let rights = only_work();
        let file_of_client =
            Request::ImportVCards { book: book.clone(), data: None, path: Some("/x.vcf".into()), dry_run: true };
        assert_eq!(code(&setup.service.handle(file_of_client, Reach::Client(&rights)).await), "forbidden");
        let many = "BEGIN:VCARD\r\nFN:x\r\nEND:VCARD\r\n".repeat(MAX_IMPORT_CARDS + 1);
        let many = Request::ImportVCards { book: book.clone(), data: Some(many), path: None, dry_run: false };
        assert_eq!(code(&setup.service.handle(many, Reach::Suite).await), "error");
        assert_eq!(work_cards(&setup).len(), 3, "too many cards write nothing");

        // A person in a book the client does not reach is new to it.
        let secret = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Secret\r\nEMAIL:secret@example.org\r\nEND:VCARD\r\n";
        let probe = |reach| {
            let request =
                Request::ImportVCards { book: book.clone(), data: Some(secret.into()), path: None, dry_run: true };
            setup.service.import_facts(reach, &request).expect("counted")
        };
        assert_eq!(probe(Reach::Client(&rights)).created, 1);
        assert_eq!(probe(Reach::Suite).duplicates, 1);
    }

    /// Issue #725: a file is read only as an absolute path to a file, no link.
    #[tokio::test]
    async fn an_import_reads_a_file_only_by_an_absolute_path() {
        let setup = setup().await;
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("kontakte.vcf");
        std::fs::write(&path, "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Kim Neu\r\nEND:VCARD\r\n").expect("file");
        let import = |path: String| Request::ImportVCards {
            book: format!("{ACCOUNT}/contacts"),
            data: None,
            path: Some(path),
            dry_run: true,
        };
        let read = setup.service.handle(import(path.to_string_lossy().into_owned()), Reach::Suite).await;
        assert_eq!(imported(read).created, 1);
        assert_eq!(code(&setup.service.handle(import("kontakte.vcf".into()), Reach::Suite).await), "error");
        #[cfg(unix)]
        {
            let link = dir.path().join("link.vcf");
            std::os::unix::fs::symlink(&path, &link).expect("link");
            assert_eq!(
                code(&setup.service.handle(import(link.to_string_lossy().into_owned()), Reach::Suite).await),
                "error"
            );
        }
    }

    /// Issue #725: an export gives the cards as stored, of the contacts, the group or the books in
    /// reach.
    #[tokio::test]
    async fn an_export_gives_the_cards_as_stored() {
        let setup = setup().await;
        let export = |book: Option<String>, group: Option<&str>, ids: Vec<String>| Request::ExportVCards {
            book,
            group: group.map(str::to_string),
            ids,
        };
        let vcards = |response: Response| match response {
            Response::VCards { data, count } => (data, count),
            other => panic!("not cards: {other:?}"),
        };
        let (data, count) =
            vcards(setup.service.handle(export(None, None, vec![id("contacts", "anna")]), Reach::Suite).await);
        let (_, stored) = setup.server.card(&setup.work, &format!("{}anna.vcf", setup.work)).expect("card");
        assert_eq!((data, count), (stored, 1), "byte for byte");
        let rights = only_work();
        let (data, count) = vcards(setup.service.handle(export(None, None, Vec::new()), Reach::Client(&rights)).await);
        assert_eq!(count, 2, "the books in reach: anna and bob");
        assert!(!data.contains("Secret Person"));
        let grouped = export(Some(format!("{ACCOUNT}/contacts")), Some("Friends"), Vec::new());
        assert_eq!(vcards(setup.service.handle(grouped, Reach::Suite).await).1, 0);
        let lost = export(None, Some("Friends"), Vec::new());
        assert_eq!(code(&setup.service.handle(lost, Reach::Suite).await), "error");
    }

    /// Issue #726: Anna kept twice is found, can be kept apart, or merged: the mail addresses and
    /// numbers join, the other is deleted, and a deletion refused leaves it in `left`.
    #[tokio::test]
    async fn duplicates_are_found_kept_apart_and_merged() {
        let setup = setup().await;
        let twin = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:anna2\r\nFN:Anna B.\r\nEMAIL:Anna@Example.org\r\n\
                    TEL;TYPE=CELL:+49 171 2345678\r\nCATEGORIES:Friends\r\nEND:VCARD\r\n";
        setup.server.put_card(&setup.work, "anna2", twin);
        setup.service.sync_all().await;
        let groups = |response: Response| match response {
            Response::ContactDuplicates { groups } => groups,
            other => panic!("not duplicates: {other:?}"),
        };
        let found = groups(setup.service.handle(Request::FindDuplicates { book: None }, Reach::Suite).await);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].reasons, [DuplicateReason::Mail]);
        let ids: Vec<String> = found[0].contacts.iter().map(|c| c.id.clone()).collect();
        assert_eq!(ids.len(), 2);

        let dismiss = Request::DismissDuplicates { ids: ids.clone() };
        assert_eq!(setup.service.handle(dismiss, Reach::Suite).await, Response::Ok);
        assert!(groups(setup.service.handle(Request::FindDuplicates { book: None }, Reach::Suite).await).is_empty());
        let one = Request::DismissDuplicates { ids: vec![ids[0].clone()] };
        assert_eq!(code(&setup.service.handle(one, Reach::Suite).await), "error", "two or more");

        let anna = id("contacts", "anna");
        let twin_id = id("contacts", "anna2");
        let etag = |id: String| async {
            contact(setup.service.handle(Request::GetContact { id }, Reach::Suite).await).etag.expect("etag")
        };
        let favorite = Request::SetContactFavorite { id: twin_id.clone(), favorite: true };
        setup.service.handle(favorite, Reach::Suite).await;
        let stale = Request::MergeContacts {
            ids: vec![anna.clone(), twin_id.clone()],
            etags: vec![etag(anna.clone()).await, "\"old\"".into()],
            keep: anna.clone(),
        };
        assert_eq!(code(&setup.service.handle(stale, Reach::Suite).await), "conflict");
        let merge = Request::MergeContacts {
            ids: vec![anna.clone(), twin_id.clone()],
            etags: vec![etag(anna.clone()).await, etag(twin_id.clone()).await],
            keep: anna.clone(),
        };
        let Response::ContactsMerged(merged) = setup.service.handle(merge, Reach::Suite).await else {
            panic!("merged")
        };
        assert!(merged.left.is_empty());
        assert_eq!(merged.contact.display_name, "Anna Berg");
        assert_eq!(merged.contact.emails.len(), 1, "one address whatever its case");
        assert_eq!(merged.contact.phones[0].value, "+49 171 2345678");
        assert_eq!(merged.contact.groups, ["Friends"]);
        assert!(merged.contact.favorite, "a favourite stays one");
        assert_eq!(code(&setup.service.handle(Request::GetContact { id: twin_id }, Reach::Suite).await), "not-found");
        let (_, written) = setup.server.card(&setup.work, &format!("{}anna.vcf", setup.work)).expect("card");
        assert!(written.contains("X-KEEP:me") && written.contains("+49 171 2345678"), "{written}");
        assert_eq!(work_cards(&setup).len(), 1, "the other is gone on the server");
    }

    /// Issue #726: a merge never deletes in a read-only book; a deletion the server refuses leaves
    /// the contact in `left`.
    #[tokio::test]
    async fn a_merge_names_what_it_could_not_delete() {
        let setup = setup().await;
        let anna = id("contacts", "anna");
        let bob = id("team", "bob");
        let etag = |id: String| async {
            contact(setup.service.handle(Request::GetContact { id }, Reach::Suite).await).etag.expect("etag")
        };
        let read_only = Request::MergeContacts {
            ids: vec![anna.clone(), bob.clone()],
            etags: vec![etag(anna.clone()).await, etag(bob).await],
            keep: anna.clone(),
        };
        assert_eq!(code(&setup.service.handle(read_only, Reach::Suite).await), "unsupported");
        setup.server.put_card(&setup.work, "anna3", &card("anna3", "Anna Drei", "anna@example.org"));
        setup.service.sync_all().await;
        let third = id("contacts", "anna3");
        let merge = Request::MergeContacts {
            ids: vec![anna.clone(), third.clone()],
            etags: vec![etag(anna.clone()).await, etag(third.clone()).await],
            keep: anna.clone(),
        };
        // Changed on the server meanwhile, unseen here: the server refuses the deletion.
        setup.server.put_card(&setup.work, "anna3", &card("anna3", "Anna Drei", "anna3@example.org"));
        let Response::ContactsMerged(merged) = setup.service.handle(merge, Reach::Suite).await else {
            panic!("merged")
        };
        assert_eq!(merged.left, [third]);
    }

    /// Issues #725, #726: every book and contact the new commands name must be in reach.
    #[tokio::test]
    async fn import_export_and_duplicates_keep_to_the_reach() {
        let setup = setup().await;
        let rights = only_work();
        let reach = Reach::Client(&rights);
        let secret = id("privat", "secret");
        let anna = id("contacts", "anna");
        for request in [
            Request::ImportVCards {
                book: format!("{ACCOUNT}/privat"),
                data: Some(String::new()),
                path: None,
                dry_run: true,
            },
            Request::ExportVCards { book: Some(format!("{ACCOUNT}/privat")), group: None, ids: Vec::new() },
            Request::ExportVCards { book: None, group: None, ids: vec![anna.clone(), secret.clone()] },
            Request::FindDuplicates { book: Some(format!("{ACCOUNT}/privat")) },
            Request::MergeContacts {
                ids: vec![anna.clone(), secret.clone()],
                etags: vec!["a".into(), "b".into()],
                keep: anna.clone(),
            },
            Request::DismissDuplicates { ids: vec![secret.clone(), anna.clone()] },
        ] {
            assert!(setup.service.out_of_reach(reach, &request).is_some(), "{request:?}");
        }
        let found = Request::FindDuplicates { book: None };
        assert_eq!(setup.service.out_of_reach(reach, &found), None);
    }

    /// Issue #727: a sender or a card of a person there already makes nothing; a new one makes a
    /// contact, the card as the mail brought it.
    #[tokio::test]
    async fn a_mail_brings_a_contact_only_once() {
        let setup = setup().await;
        let book = format!("{ACCOUNT}/contacts");
        let known =
            setup.service.contact_from_sender(Reach::Suite, &book, "Anna", "ANNA@example.org").await.expect("known");
        assert_eq!((known.created, known.contact.id.as_str()), (false, id("contacts", "anna").as_str()));
        let new =
            setup.service.contact_from_sender(Reach::Suite, &book, "Kim  Neu", "kim@example.org").await.expect("new");
        assert!(new.created);
        assert_eq!(
            (new.contact.display_name.as_str(), new.contact.emails[0].value.as_str()),
            ("Kim Neu", "kim@example.org")
        );
        let card = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:lars\r\nFN:Lars Karte\r\nX-FROM:mail\r\nEND:VCARD\r\n";
        let made = setup.service.contact_from_card(Reach::Suite, &book, card).await.expect("card");
        assert!(made.created);
        assert!(work_cards(&setup).iter().any(|stored| stored == card), "as the mail brought it");
        let again = setup.service.contact_from_card(Reach::Suite, &book, card).await.expect("again");
        assert_eq!((again.created, again.contact.id), (false, made.contact.id));
        let empty = setup.service.contact_from_card(Reach::Suite, &book, "no card").await;
        assert!(matches!(empty, Err(ContactsError::Other(_))));
        let team =
            setup.service.contact_from_sender(Reach::Suite, &format!("{ACCOUNT}/team"), "", "x@example.org").await;
        assert_eq!(team.err(), Some(ContactsError::ReadOnly));
    }
}
