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

use base64::Engine;
use chrono::{Local, Utc};
use torro_contacts::{
    AddressBook, Birthday, Contact, ContactFields, ContactPage, ContactState, ContactSummary, ContactsError,
    ContactsProvider, FieldsError, Group, Keep, Moved, PhotoSize, Stored, contact_id, vcard,
};
use torro_core::i18n::{tr, trf};
use torro_ipc::api::Rights;
use torro_ipc::contacts::{BIRTHDAY_DAYS, MAX_BIRTHDAY_DAYS};
use torro_ipc::{Request, Response};

use super::store::{BookRow, CardRow, Op, Query, Queued, Store};
use super::sync::{self, Sent, with};
use super::{ContactsService, photo};
use crate::pim::Reach;

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
    contact.etag = card.etag.clone();
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

/// Whether `sent` is the version of `card` the client showed; a contact not on the server yet has
/// none, and the client sends it empty.
fn same_version(card: &CardRow, sent: &str) -> bool {
    card.etag.as_deref().unwrap_or("") == sent
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
            _ => None,
        }
    }

    /// Answers a `contacts.*` request for `reach` (its rights and reach checked before).
    pub async fn handle(&self, request: Request, reach: Reach<'_>) -> Response {
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
                Box::pin(self.change(&id, &etag, |_, contact| fields.apply(contact).map_err(fields_error))).await
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
    /// a round after every write), under the lock of the rounds, so a round never sends the same
    /// change at once. `None` when the change could not go yet (offline, no connection, a passing
    /// failure): it waits in the queue.
    async fn send_now(&self, book: &BookRow, card: &str) -> Result<Option<Sent>, ContactsError> {
        let store = self.store()?;
        let Some(remote) = self.remote(&book.account).await else { return Ok(None) };
        let _round = self.rounds.lock().await;
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

    /// A round for `book` now, under the lock of the rounds.
    async fn push(&self, book: &BookRow) {
        let Ok(store) = self.store() else { return };
        let Some(remote) = self.remote(&book.account).await else { return };
        let _round = self.rounds.lock().await;
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
            Some(Sent::Conflict) => Err(ContactsError::Conflict { server_vcard: None, etag: None }),
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
            return Err(ContactsError::Conflict { server_vcard: None, etag: found.card.etag.clone() });
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
        // No round sends this contact meanwhile.
        let _round = self.rounds.lock().await;
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
            return Err(ContactsError::Conflict { server_vcard: None, etag: found.card.etag.clone() });
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
                    etag: found.card.etag.clone(),
                })?;
            }
            Ok(())
        })?;
        // Changed on the server meanwhile or refused: the round after it brought the contact back.
        match self.send_now(&found.book, id).await? {
            Some(Sent::Conflict) => Err(ContactsError::Conflict { server_vcard: None, etag: None }),
            Some(Sent::Refused(err)) => Err(err),
            Some(Sent::Written | Sent::Waits) | None => Ok(Response::Ok),
        }
    }

    async fn resolve(&self, id: &str, keep: Keep, etag: &str) -> Result<Response, ContactsError> {
        let found = self.find(id)?;
        if !same_version(&found.card, etag) {
            return Err(ContactsError::Conflict { server_vcard: None, etag: found.card.etag.clone() });
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
        let (contact, book, to_book) = match request {
            Request::CreateContact { book, .. } => (None, book_name(book), None),
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
        Some(crate::approvals::ContactFacts { contact, book, to_book })
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
}
