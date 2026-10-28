//! Bringing the store and a contacts server in step, one account at a time.
//!
//! Per book: first the queue goes out (changes made offline, oldest first), then what changed on
//! the server comes in (`sync-collection`, or a full listing compared by ETag). A card changed here
//! and on the server is merged field by field ([`torro_contacts::merge`]); when both changed the
//! same field it is a `conflict` with the server's version beside it, and nothing is overwritten
//! until a person decides. Writing changes only the properties that changed
//! ([`torro_contacts::vcard::patch`]).
//!
//! - `offline` leaves the queue as it is; the next round tries again.
//! - A book is gone only when the server listed the account's books and it was not among them;
//!   then its contacts and queue go, and the caller takes it out of every client's rights.
//! - An account without an address book home (`NotFound` when listing) has no contacts; nothing
//!   is deleted for it.
//! - A card the server deleted while it waited to be written becomes a `conflict` without a server
//!   version: the person decides whether it comes back.
//! - A write the server refuses for good (read-only, too large) is taken back to the server's
//!   version; a contact made here that it refuses is dropped.

use std::sync::Mutex;

use torro_contacts::{
    Changes, ContactState, ContactsError, ContactsProvider, Listing, Merge, RemoteBook, RemoteCard, SyncToken, book_id,
    contact_id, merge, vcard,
};

use super::store::{BookRow, CardRow, Op, Store, StoreError};

/// Runs `work` on the store under its lock; a store error becomes [`ContactsError::Other`].
pub fn with<T>(store: &Mutex<Store>, work: impl FnOnce(&Store) -> Result<T, StoreError>) -> Result<T, ContactsError> {
    let store = store.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    work(&store).map_err(|err| {
        eprintln!("torroworkplace: contacts: {err}");
        ContactsError::Other("The contacts on this device cannot be read or written.".into())
    })
}

/// What one round of an account did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Synced {
    /// Books that are gone from the server, with everything of them gone here too.
    pub removed_books: Vec<String>,
}

/// One round for `account` with `provider`; see the module docs.
pub async fn sync_account<P: ContactsProvider>(
    store: &Mutex<Store>,
    account: &str,
    provider: &P,
) -> Result<Synced, ContactsError> {
    let remote = match provider.address_books().await {
        Ok(books) => books,
        // No address book home: no contacts, and nothing to delete.
        Err(ContactsError::NotFound) => return Ok(Synced::default()),
        Err(err) => return Err(err),
    };
    let rows: Vec<BookRow> = remote.iter().map(|book| row_of(account, book)).collect();
    let removed_books = with(store, |s| {
        let mut removed = Vec::new();
        for known in s.books(Some(account))? {
            if !rows.iter().any(|row| row.id == known.id) {
                s.remove_book(&known.id)?;
                removed.push(known.id);
            }
        }
        for row in &rows {
            s.put_book(row)?;
        }
        Ok(removed)
    })?;
    let mut first_error = None;
    for row in &rows {
        let Some(book) = with(store, |s| s.book(&row.id))? else { continue };
        match sync_book(store, provider, &book).await {
            Ok(()) => {}
            // Signing in and reaching the server fail for every book alike.
            Err(err @ (ContactsError::Auth | ContactsError::Offline)) => return Err(err),
            Err(err) => {
                eprintln!("torroworkplace: contacts: a book of {account} did not sync: {err}");
                first_error.get_or_insert(err);
            }
        }
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(Synced { removed_books }),
    }
}

fn row_of(account: &str, book: &RemoteBook) -> BookRow {
    BookRow {
        id: book_id(account, &book.name),
        account: account.to_string(),
        href: book.href.clone(),
        name: book.name.clone(),
        display_name: if book.display_name.trim().is_empty() { book.name.clone() } else { book.display_name.clone() },
        writable: book.writable,
        hidden: book.hidden_by_default,
        token: None,
        group_cards: false,
    }
}

/// The queue of `book`, then what changed on the server.
pub async fn sync_book<P: ContactsProvider>(
    store: &Mutex<Store>,
    provider: &P,
    book: &BookRow,
) -> Result<(), ContactsError> {
    flush(store, provider, book).await?;
    let since = book.token.clone().map(SyncToken);
    let Changes { listing, token } = provider.changes(&book.href, since.as_ref()).await?;
    let local = with(store, |s| s.cards(&book.id))?;
    let etag_of = |href: &str| local.iter().find(|c| c.href.as_deref() == Some(href)).and_then(|c| c.etag.clone());
    let (wanted, gone): (Vec<String>, Vec<String>) = match listing {
        Listing::Delta { changed, removed } => (
            changed.into_iter().filter(|e| etag_of(&e.href).as_deref() != Some(&e.etag)).map(|e| e.href).collect(),
            removed,
        ),
        Listing::Full(all) => {
            let gone = local
                .iter()
                .filter_map(|c| c.href.clone())
                .filter(|href| !all.iter().any(|e| e.href == *href))
                .collect();
            (all.into_iter().filter(|e| etag_of(&e.href).as_deref() != Some(&e.etag)).map(|e| e.href).collect(), gone)
        }
    };
    let fetched = if wanted.is_empty() { Vec::new() } else { provider.fetch(&book.href, &wanted).await? };
    let mut merged_any = false;
    for card in &fetched {
        merged_any |= with(store, |s| take_in(s, book, card))?;
    }
    with(store, |s| {
        for href in &gone {
            gone_on_server(s, book, href)?;
        }
        s.set_token(&book.id, token.as_ref().map(|t| t.0.as_str()))
    })?;
    // A merge of a change made here queued it again with the server's new version.
    if merged_any {
        flush(store, provider, book).await?;
    }
    Ok(())
}

/// A card from the server into the store; `true` when it was merged with a change waiting here.
fn take_in(s: &Store, book: &BookRow, card: &RemoteCard) -> Result<bool, StoreError> {
    if vcard::group_card(&card.vcard).is_some() {
        s.set_group_cards(&book.id, true)?;
        return Ok(false);
    }
    let Ok(theirs) = vcard::read(&card.vcard) else {
        eprintln!("torroworkplace: contacts: a card of a book is no vCard; left out");
        return Ok(false);
    };
    let uid = if theirs.uid.trim().is_empty() { uid_of_href(&card.href) } else { theirs.uid.clone() };
    let id = contact_id(&book.id, &uid);
    let local = match s.card_at(&book.id, &card.href)? {
        Some(local) => Some(local),
        None => s.card(&id)?,
    };
    let fresh = CardRow {
        id: id.clone(),
        book: book.id.clone(),
        uid: uid.clone(),
        href: Some(card.href.clone()),
        etag: card.etag.clone(),
        vcard: card.vcard.clone(),
        base: Some(card.vcard.clone()),
        state: ContactState::Synced,
        server: None,
        server_etag: None,
    };
    let Some(local) = local else {
        s.put_card(&fresh)?;
        return Ok(false);
    };
    if local.id != id {
        s.remove_card(&local.id)?;
    }
    match local.state {
        ContactState::Synced => {
            s.put_card(&fresh)?;
            Ok(false)
        }
        ContactState::Conflict => {
            s.put_card(&CardRow { server: Some(card.vcard.clone()), server_etag: card.etag.clone(), ..local })?;
            Ok(false)
        }
        ContactState::Pending => {
            let outcome = merged(&local, &card.vcard);
            match outcome {
                Some(vcard) => {
                    s.put_card(&CardRow {
                        vcard,
                        base: Some(card.vcard.clone()),
                        etag: card.etag.clone(),
                        href: Some(card.href.clone()),
                        ..local.clone()
                    })?;
                    s.enqueue(&super::store::Queued {
                        card: local.id.clone(),
                        book: book.id.clone(),
                        op: Op::Put,
                        uid: local.uid.clone(),
                        href: Some(card.href.clone()),
                        etag: card.etag.clone(),
                    })?;
                    Ok(true)
                }
                None => {
                    conflict(s, local, Some(card.vcard.clone()), card.etag.clone())?;
                    Ok(false)
                }
            }
        }
    }
}

/// This device's version of `local` merged onto the server's `theirs_raw`: the server's card with
/// this device's changes written in. `None` when both changed the same field.
pub fn merged(local: &CardRow, theirs_raw: &str) -> Option<String> {
    let theirs = vcard::read(theirs_raw).ok()?;
    let mine = vcard::read(&local.vcard).ok()?;
    let base = local.base.as_deref().and_then(|b| vcard::read(b).ok()).unwrap_or_else(|| theirs.clone());
    match merge(&base, &mine, &theirs) {
        Merge::Merged(contact) => Some(vcard::patch(theirs_raw, &theirs, &contact)),
        Merge::Conflict { .. } => None,
    }
}

fn conflict(s: &Store, local: CardRow, server: Option<String>, server_etag: Option<String>) -> Result<(), StoreError> {
    s.dequeue(&local.id)?;
    s.put_card(&CardRow { state: ContactState::Conflict, server, server_etag, ..local })
}

/// The server no longer has the card at `href`.
fn gone_on_server(s: &Store, book: &BookRow, href: &str) -> Result<(), StoreError> {
    let Some(local) = s.card_at(&book.id, href)? else { return Ok(()) };
    match local.state {
        ContactState::Synced => s.remove_card(&local.id),
        // Changed here, deleted there: the person decides.
        ContactState::Pending | ContactState::Conflict => conflict(s, local, None, None),
    }
}

/// The uid of a card without one: its file name without `.vcf`.
fn uid_of_href(href: &str) -> String {
    let name = href.trim_end_matches('/').rsplit('/').next().unwrap_or(href);
    name.strip_suffix(".vcf").unwrap_or(name).to_string()
}

/// Sends the queue of `book`, oldest first. Stops at `offline` and `auth`; the rest stays queued.
pub async fn flush<P: ContactsProvider>(
    store: &Mutex<Store>,
    provider: &P,
    book: &BookRow,
) -> Result<(), ContactsError> {
    for queued in with(store, |s| s.queued(&book.id))? {
        match queued.op {
            Op::Delete => {
                let Some(href) = queued.href.as_deref() else {
                    with(store, |s| s.dequeue(&queued.card))?;
                    continue;
                };
                match provider.delete(&book.href, href, queued.etag.as_deref()).await {
                    // Changed on the server meanwhile, or not to be deleted: it comes back with
                    // the next listing, the person sees it again.
                    Ok(())
                    | Err(ContactsError::NotFound | ContactsError::Conflict { .. } | ContactsError::ReadOnly) => {
                        with(store, |s| s.dequeue(&queued.card))?
                    }
                    Err(err @ (ContactsError::Offline | ContactsError::Auth)) => return Err(err),
                    Err(err) => {
                        eprintln!("torroworkplace: contacts: a deletion waits: {err}");
                        return Err(err);
                    }
                }
            }
            Op::Put => {
                let Some(card) = with(store, |s| s.card(&queued.card))? else {
                    with(store, |s| s.dequeue(&queued.card))?;
                    continue;
                };
                if card.state == ContactState::Conflict {
                    with(store, |s| s.dequeue(&card.id))?;
                    continue;
                }
                put(store, provider, book, card).await?;
            }
        }
    }
    Ok(())
}

/// Writes one card; a newer version on the server is merged and written once more.
async fn put<P: ContactsProvider>(
    store: &Mutex<Store>,
    provider: &P,
    book: &BookRow,
    card: CardRow,
) -> Result<(), ContactsError> {
    let mut card = card;
    for attempt in 0..2 {
        let answer = provider.put(&book.href, card.href.as_deref(), &card.uid, &card.vcard, card.etag.as_deref()).await;
        match answer {
            Ok(stored) => {
                return with(store, |s| {
                    s.dequeue(&card.id)?;
                    s.put_card(&CardRow {
                        href: Some(stored.href),
                        etag: stored.etag,
                        base: Some(card.vcard.clone()),
                        state: ContactState::Synced,
                        server: None,
                        server_etag: None,
                        ..card
                    })
                });
            }
            Err(ContactsError::Conflict { server_vcard: Some(server), etag }) if attempt == 0 => {
                match merged(&card, &server) {
                    Some(vcard) => {
                        card = CardRow { vcard, base: Some(server), etag, ..card };
                    }
                    None => return with(store, |s| conflict(s, card, Some(server), etag)),
                }
            }
            Err(ContactsError::Conflict { server_vcard, etag }) => {
                return with(store, |s| conflict(s, card, server_vcard, etag));
            }
            // Changed here, deleted there.
            Err(ContactsError::NotFound) => return with(store, |s| conflict(s, card, None, None)),
            Err(err @ (ContactsError::ReadOnly | ContactsError::TooLarge)) => {
                eprintln!("torroworkplace: contacts: the server refused a change, taken back: {err}");
                return with(store, |s| {
                    s.dequeue(&card.id)?;
                    match card.base.clone() {
                        Some(base) => s.put_card(&CardRow { vcard: base, state: ContactState::Synced, ..card }),
                        None => s.remove_card(&card.id),
                    }
                });
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

#[cfg(test)]
pub mod fake {
    //! A contacts server in memory, for the tests of the store, the sync and the commands.

    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use torro_contacts::{
        Changes, ContactsError, ContactsProvider, Entry, Listing, RemoteBook, RemoteCard, Stored, SyncToken,
    };

    #[derive(Debug, Default)]
    pub struct State {
        /// Book href → (name, writable, hidden, cards: href → (etag, vcard)).
        pub books: BTreeMap<String, (String, bool, bool, BTreeMap<String, (String, String)>)>,
        pub version: u64,
        pub offline: bool,
        pub refuse_auth: bool,
        /// No address book home.
        pub no_home: bool,
        /// Hrefs written, for the tests.
        pub puts: Vec<String>,
    }

    #[derive(Debug, Default)]
    pub struct Server {
        pub state: Mutex<State>,
    }

    pub const HOME: &str = "https://cloud.example.org/remote.php/dav/addressbooks/users/anna/";

    impl Server {
        pub fn lock(&self) -> std::sync::MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        pub fn add_book(&self, name: &str, writable: bool, hidden: bool) -> String {
            let href = format!("{HOME}{name}/");
            self.lock().books.insert(href.clone(), (name.into(), writable, hidden, BTreeMap::new()));
            href
        }

        /// Puts a card as another device would; returns its href.
        pub fn put_card(&self, book: &str, uid: &str, vcard: &str) -> String {
            let mut state = self.lock();
            state.version += 1;
            let etag = format!("\"{}\"", state.version);
            let href = format!("{book}{uid}.vcf");
            if let Some(entry) = state.books.get_mut(book) {
                entry.3.insert(href.clone(), (etag, vcard.into()));
            }
            href
        }

        pub fn remove_card(&self, book: &str, href: &str) {
            if let Some(entry) = self.lock().books.get_mut(book) {
                entry.3.remove(href);
            }
        }

        pub fn card(&self, book: &str, href: &str) -> Option<(String, String)> {
            self.lock().books.get(book).and_then(|entry| entry.3.get(href).cloned())
        }

        fn check(&self) -> Result<std::sync::MutexGuard<'_, State>, ContactsError> {
            let state = self.lock();
            if state.offline {
                return Err(ContactsError::Offline);
            }
            if state.refuse_auth {
                return Err(ContactsError::Auth);
            }
            Ok(state)
        }
    }

    impl ContactsProvider for Server {
        async fn address_books(&self) -> Result<Vec<RemoteBook>, ContactsError> {
            let state = self.check()?;
            if state.no_home {
                return Err(ContactsError::NotFound);
            }
            Ok(state
                .books
                .iter()
                .map(|(href, (name, writable, hidden, _))| RemoteBook {
                    href: href.clone(),
                    name: name.clone(),
                    display_name: name.clone(),
                    writable: *writable,
                    hidden_by_default: *hidden,
                })
                .collect())
        }

        async fn changes(&self, book: &str, _since: Option<&SyncToken>) -> Result<Changes, ContactsError> {
            let state = self.check()?;
            let cards = &state.books.get(book).ok_or(ContactsError::NotFound)?.3;
            let all = cards.iter().map(|(href, (etag, _))| Entry { href: href.clone(), etag: etag.clone() }).collect();
            Ok(Changes { listing: Listing::Full(all), token: Some(SyncToken(state.version.to_string())) })
        }

        async fn fetch(&self, book: &str, hrefs: &[String]) -> Result<Vec<RemoteCard>, ContactsError> {
            let state = self.check()?;
            let cards = &state.books.get(book).ok_or(ContactsError::NotFound)?.3;
            Ok(hrefs
                .iter()
                .filter_map(|href| {
                    let (etag, vcard) = cards.get(href)?;
                    Some(RemoteCard { href: href.clone(), etag: Some(etag.clone()), vcard: vcard.clone() })
                })
                .collect())
        }

        async fn put(
            &self,
            book: &str,
            href: Option<&str>,
            uid: &str,
            vcard: &str,
            etag: Option<&str>,
        ) -> Result<Stored, ContactsError> {
            let mut state = self.check()?;
            state.version += 1;
            let version = format!("\"{}\"", state.version);
            let (_, writable, _, cards) = state.books.get_mut(book).ok_or(ContactsError::NotFound)?;
            if !*writable {
                return Err(ContactsError::ReadOnly);
            }
            let href = href.map(str::to_string).unwrap_or_else(|| format!("{book}{uid}.vcf"));
            match (cards.get(&href), etag) {
                (Some((now, server)), Some(sent)) if now != sent => {
                    return Err(ContactsError::Conflict {
                        server_vcard: Some(server.clone()),
                        etag: Some(now.clone()),
                    });
                }
                (Some((now, server)), None) => {
                    return Err(ContactsError::Conflict {
                        server_vcard: Some(server.clone()),
                        etag: Some(now.clone()),
                    });
                }
                (None, Some(_)) => return Err(ContactsError::NotFound),
                _ => {}
            }
            cards.insert(href.clone(), (version.clone(), vcard.into()));
            state.puts.push(href.clone());
            Ok(Stored { href, etag: Some(version) })
        }

        async fn delete(&self, book: &str, href: &str, etag: Option<&str>) -> Result<(), ContactsError> {
            let mut state = self.check()?;
            let (_, writable, _, cards) = state.books.get_mut(book).ok_or(ContactsError::NotFound)?;
            if !*writable {
                return Err(ContactsError::ReadOnly);
            }
            match (cards.get(href), etag) {
                (Some((now, server)), Some(sent)) if now != sent => {
                    Err(ContactsError::Conflict { server_vcard: Some(server.clone()), etag: Some(now.clone()) })
                }
                _ => {
                    cards.remove(href);
                    Ok(())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Server;
    use super::*;
    use torro_contacts::FieldKind;

    const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";

    fn card(uid: &str, name: &str, tel: &str, note: &str) -> String {
        format!(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid}\r\nFN:{name}\r\nN:{name};;;;\r\nTEL;TYPE=CELL:{tel}\r\nNOTE:{note}\r\nEND:VCARD\r\n"
        )
    }

    fn open() -> (tempfile::TempDir, Mutex<Store>) {
        let dir = tempfile::tempdir().expect("dir");
        let store = Mutex::new(Store::open(dir.path()).expect("store"));
        (dir, store)
    }

    fn book_row(store: &Mutex<Store>) -> BookRow {
        with(store, |s| s.book(&format!("{ACCOUNT}/contacts"))).expect("read").expect("book")
    }

    /// Changes the stored card `id` here, as a command would: written in, queued.
    fn change_here(store: &Mutex<Store>, id: &str, change: impl FnOnce(&mut torro_contacts::Contact)) {
        with(store, |s| {
            let row = s.card(id)?.expect("card");
            let before = vcard::read(&row.vcard).expect("read");
            let mut after = before.clone();
            change(&mut after);
            let vcard = vcard::patch(&row.vcard, &before, &after);
            s.put_card(&CardRow { vcard, state: ContactState::Pending, ..row.clone() })?;
            s.enqueue(&super::super::store::Queued {
                card: row.id.clone(),
                book: row.book.clone(),
                op: Op::Put,
                uid: row.uid.clone(),
                href: row.href.clone(),
                etag: row.etag.clone(),
            })
        })
        .expect("changed");
    }

    #[tokio::test]
    async fn first_sync_change_and_deletion() {
        let (_dir, store) = open();
        let server = Server::default();
        let book = server.add_book("contacts", true, false);
        server.add_book("z-server-generated--system", false, true);
        let href = server.put_card(&book, "a", &card("a", "Berg", "+49 171 2345678", "one"));
        server.put_card(&book, "b", &card("b", "Keller", "+41 79 1", "two"));
        sync_account(&store, ACCOUNT, &server).await.expect("synced");
        let books = with(&store, |s| s.books(Some(ACCOUNT))).expect("books");
        assert_eq!(books.len(), 2);
        assert!(books.iter().any(|b| b.hidden && !b.writable));
        let id = format!("{ACCOUNT}/contacts/a");
        assert!(with(&store, |s| s.card(&id)).expect("read").is_some());

        server.put_card(&book, "a", &card("a", "Berg", "+49 171 2345678", "changed"));
        server.remove_card(&book, &format!("{book}b.vcf"));
        sync_account(&store, ACCOUNT, &server).await.expect("synced");
        let row = with(&store, |s| s.card(&id)).expect("read").expect("card");
        assert!(row.vcard.contains("NOTE:changed"));
        assert_eq!(row.href.as_deref(), Some(href.as_str()));
        assert!(with(&store, |s| s.card(&format!("{ACCOUNT}/contacts/b"))).expect("read").is_none(), "deleted there");
    }

    #[tokio::test]
    async fn made_offline_is_written_later_and_survives_a_restart() {
        let dir = tempfile::tempdir().expect("dir");
        let server = Server::default();
        let book = server.add_book("contacts", true, false);
        {
            let store = Mutex::new(Store::open(dir.path()).expect("store"));
            sync_account(&store, ACCOUNT, &server).await.expect("synced");
            let row = book_row(&store);
            let raw = card("new", "Neu", "1", "made offline");
            with(&store, |s| {
                s.put_card(&CardRow {
                    id: format!("{}/new", row.id),
                    book: row.id.clone(),
                    uid: "new".into(),
                    href: None,
                    etag: None,
                    vcard: raw.clone(),
                    base: None,
                    state: ContactState::Pending,
                    server: None,
                    server_etag: None,
                })?;
                s.enqueue(&super::super::store::Queued {
                    card: format!("{}/new", row.id),
                    book: row.id.clone(),
                    op: Op::Put,
                    uid: "new".into(),
                    href: None,
                    etag: None,
                })
            })
            .expect("made");
            server.lock().offline = true;
            assert_eq!(sync_account(&store, ACCOUNT, &server).await, Err(ContactsError::Offline));
        }
        // A new start: the queue is still there and goes out.
        server.lock().offline = false;
        let store = Mutex::new(Store::open(dir.path()).expect("store again"));
        sync_account(&store, ACCOUNT, &server).await.expect("synced");
        assert!(server.card(&book, &format!("{book}new.vcf")).is_some(), "written later");
        let row = with(&store, |s| s.card(&format!("{ACCOUNT}/contacts/new"))).expect("read").expect("card");
        assert_eq!(row.state, ContactState::Synced);
    }

    #[tokio::test]
    async fn changes_on_both_sides_merge_or_conflict() {
        let (_dir, store) = open();
        let server = Server::default();
        let book = server.add_book("contacts", true, false);
        server.put_card(&book, "a", &card("a", "Berg", "+49 171 1", "one"));
        sync_account(&store, ACCOUNT, &server).await.expect("synced");
        let id = format!("{ACCOUNT}/contacts/a");

        // Here a second number offline, there a new note: both happen.
        server.lock().offline = true;
        change_here(&store, &id, |c| {
            c.phones.push(torro_contacts::Labeled::new("+49 30 2".into(), FieldKind::Work));
        });
        let _ = sync_account(&store, ACCOUNT, &server).await;
        server.lock().offline = false;
        server.put_card(&book, "a", &card("a", "Berg", "+49 171 1", "from elsewhere"));
        sync_account(&store, ACCOUNT, &server).await.expect("synced");
        let (_, written) = server.card(&book, &format!("{book}a.vcf")).expect("card");
        assert!(written.contains("NOTE:from elsewhere") && written.contains("+49 30 2"), "{written}");
        let row = with(&store, |s| s.card(&id)).expect("read").expect("card");
        assert_eq!(row.state, ContactState::Synced);

        // Both change the note: a conflict with the server's version beside it.
        server.lock().offline = true;
        change_here(&store, &id, |c| c.note = Some("mine".into()));
        let _ = sync_account(&store, ACCOUNT, &server).await;
        server.lock().offline = false;
        let (_, now) = server.card(&book, &format!("{book}a.vcf")).expect("card");
        server.put_card(&book, "a", &now.replace("NOTE:from elsewhere", "NOTE:theirs"));
        sync_account(&store, ACCOUNT, &server).await.expect("synced");
        let row = with(&store, |s| s.card(&id)).expect("read").expect("card");
        assert_eq!(row.state, ContactState::Conflict);
        assert!(row.vcard.contains("NOTE:mine") && row.server.as_deref().is_some_and(|s| s.contains("NOTE:theirs")));
        let (_, kept) = server.card(&book, &format!("{book}a.vcf")).expect("card");
        assert!(kept.contains("NOTE:theirs"), "nothing overwritten");
    }

    #[tokio::test]
    async fn a_card_deleted_there_while_changed_here_is_a_conflict() {
        let (_dir, store) = open();
        let server = Server::default();
        let book = server.add_book("contacts", true, false);
        let href = server.put_card(&book, "a", &card("a", "Berg", "1", "one"));
        sync_account(&store, ACCOUNT, &server).await.expect("synced");
        let id = format!("{ACCOUNT}/contacts/a");
        server.lock().offline = true;
        change_here(&store, &id, |c| c.note = Some("mine".into()));
        let _ = sync_account(&store, ACCOUNT, &server).await;
        server.lock().offline = false;
        server.remove_card(&book, &href);
        sync_account(&store, ACCOUNT, &server).await.expect("synced");
        let row = with(&store, |s| s.card(&id)).expect("read").expect("kept");
        assert_eq!((row.state, row.server), (ContactState::Conflict, None));
    }

    #[tokio::test]
    async fn a_book_goes_only_when_the_listing_worked_and_no_home_deletes_nothing() {
        let (_dir, store) = open();
        let server = Server::default();
        let book = server.add_book("contacts", true, false);
        let other = server.add_book("old", true, false);
        server.put_card(&book, "a", &card("a", "Berg", "1", "one"));
        sync_account(&store, ACCOUNT, &server).await.expect("synced");
        server.lock().no_home = true;
        assert_eq!(sync_account(&store, ACCOUNT, &server).await, Ok(Synced::default()));
        assert_eq!(with(&store, |s| s.books(Some(ACCOUNT))).expect("books").len(), 2, "nothing deleted");
        server.lock().no_home = false;
        server.lock().books.remove(&other);
        let synced = sync_account(&store, ACCOUNT, &server).await.expect("synced");
        assert_eq!(synced.removed_books, [format!("{ACCOUNT}/old")]);
        server.lock().refuse_auth = true;
        assert_eq!(sync_account(&store, ACCOUNT, &server).await, Err(ContactsError::Auth));
    }
}
