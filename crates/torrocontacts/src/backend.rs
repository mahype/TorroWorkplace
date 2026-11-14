//! Where the window's requests go: the core over its socket ([`ContactsClient`]), or the sample in
//! memory (`--demo`, [`DemoSource`]).
//!
//! The window reads and changes nothing itself (`CLAUDE.md`, "Jede Funktion dreimal"): every action is
//! a command of the API area `contacts` through [`ContactsSource`], which both sides speak. Besides
//! them the window asks the core what it reads of the settings ([`Backend::settings`],
//! `settings.get_settings`: the hidden address books, maps and the lookup of contact addresses) and,
//! for the map under an address (issue #355), `contacts.locate_contact`, `calendar.map_preview` and
//! `contacts.open_in_maps` ([`Backend::locate`], [`Backend::map`], [`Backend::open_in_maps`]).
//!
//! [`load`] is what the window shows: the books, every contact of them (all pages), the birthdays of
//! the coming year and the contacts used last.

use std::sync::Arc;

use torro_contacts::demo::DemoSource;
use torro_contacts::{
    AddressBook, Birthday, Contact, ContactFields, ContactPage, ContactQuery, ContactSummary, ContactsError,
    ContactsSource, Group, Keep, Moved, PhotoSize,
};
use torro_ipc::Request;
use torro_ipc::Response;
use torro_ipc::contacts::{ContactLocation, ContactsClient, Geo, LocationSource, MAX_BIRTHDAY_DAYS};

/// How many contacts one page of `contacts.list_contacts` asks for: the most the core gives.
pub const PAGE: u32 = 500;

/// The core over its socket, or the demo. Cheap to clone, so every task can own one.
#[derive(Debug, Clone)]
pub enum Backend {
    Core(ContactsClient),
    Demo(Arc<DemoSource>),
}

impl Backend {
    /// The core of this user session.
    pub fn core() -> Backend {
        Backend::Core(ContactsClient::default())
    }

    /// The sample of `torro_contacts::demo` around today.
    pub fn demo() -> Backend {
        Backend::Demo(Arc::new(DemoSource::default()))
    }

    pub fn is_demo(&self) -> bool {
        matches!(self, Backend::Demo(_))
    }

    /// What the window reads of the settings ([`Shown`]). A core that does not answer says no to
    /// everything; the demo shows no hidden books and has maps and the lookup on.
    pub async fn settings(&self) -> Shown {
        let Backend::Core(client) = self else { return Shown { hidden_books: false, maps: true, lookup: true } };
        match torro_ipc::client::call(&client.socket, &Request::Settings).await {
            Ok(Response::Settings { settings }) => Shown {
                hidden_books: settings.contacts_show_hidden_books,
                maps: settings.maps.enabled,
                lookup: settings.maps.contact_lookup,
            },
            _ => Shown::default(),
        }
    }

    /// Where the address number `address` of the contact `id` is (`contacts.locate_contact`, issue
    /// #355); the error is the core's code (`busy`, `offline`, `not-found` …). The demo looks nothing
    /// up: the first address is at the contact's `GEO`, the others are not found.
    pub async fn locate(&self, id: String, address: usize) -> Result<ContactLocation, String> {
        match self {
            Backend::Core(client) => client.locate(id, address).await,
            Backend::Demo(demo) => {
                let contact = demo.get_contact(id).await.map_err(|_| "not-found".to_string())?;
                Ok(match contact.geo.filter(|_| address == 0) {
                    Some(geo) => ContactLocation::Found {
                        geo: Geo { lat: geo.lat, lon: geo.lon },
                        source: LocationSource::Vcard,
                    },
                    None => ContactLocation::NotFound,
                })
            }
        }
    }

    /// The map around `geo` as PNG (`calendar.map_preview`, [`torro_gui::map::MAP_WIDTH`] ×
    /// [`torro_gui::map::MAP_HEIGHT`], `dark` as the window is drawn); `None` when it cannot be had,
    /// and always in the demo, which draws its sketch instead.
    pub async fn map(&self, geo: Geo, dark: bool) -> Option<Vec<u8>> {
        let Backend::Core(client) = self else { return None };
        client.map(geo, torro_gui::map::MAP_WIDTH, torro_gui::map::MAP_HEIGHT, dark).await
    }

    /// Opens the address number `address` of the contact `id` in the maps app of the settings
    /// (`contacts.open_in_maps`); the error is the core's message. The demo opens nothing.
    pub async fn open_in_maps(&self, id: String, address: usize) -> Result<(), String> {
        match self {
            Backend::Core(client) => client.open_in_maps(id, address).await,
            Backend::Demo(_) => Ok(()),
        }
    }
}

/// What the window reads of the settings (`settings.get_settings`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shown {
    /// The person wants the hidden address books (Nextcloud's system address book, "Recently
    /// contacted") in the app: `contacts_show_hidden_books`.
    pub hidden_books: bool,
    /// Maps and place search are on (`maps.enabled`).
    pub maps: bool,
    /// The lookup of contact addresses is on (`maps.contactLookup`, issue #355).
    pub lookup: bool,
}

/// Every call goes to the side the backend stands for.
impl ContactsSource for Backend {
    async fn address_books(&self, account: Option<String>) -> Result<Vec<AddressBook>, ContactsError> {
        match self {
            Backend::Core(client) => client.address_books(account).await,
            Backend::Demo(demo) => demo.address_books(account).await,
        }
    }

    async fn list_contacts(&self, query: ContactQuery) -> Result<ContactPage, ContactsError> {
        match self {
            Backend::Core(client) => client.list_contacts(query).await,
            Backend::Demo(demo) => demo.list_contacts(query).await,
        }
    }

    async fn get_contact(&self, id: String) -> Result<Contact, ContactsError> {
        match self {
            Backend::Core(client) => client.get_contact(id).await,
            Backend::Demo(demo) => demo.get_contact(id).await,
        }
    }

    async fn get_photo(&self, id: String, size: PhotoSize) -> Result<Vec<u8>, ContactsError> {
        match self {
            Backend::Core(client) => client.get_photo(id, size).await,
            Backend::Demo(demo) => demo.get_photo(id, size).await,
        }
    }

    async fn list_groups(&self, book: Option<String>) -> Result<Vec<Group>, ContactsError> {
        match self {
            Backend::Core(client) => client.list_groups(book).await,
            Backend::Demo(demo) => demo.list_groups(book).await,
        }
    }

    async fn upcoming_birthdays(&self, days: Option<u32>) -> Result<Vec<Birthday>, ContactsError> {
        match self {
            Backend::Core(client) => client.upcoming_birthdays(days).await,
            Backend::Demo(demo) => demo.upcoming_birthdays(days).await,
        }
    }

    async fn create_contact(&self, book: String, fields: ContactFields) -> Result<Contact, ContactsError> {
        match self {
            Backend::Core(client) => client.create_contact(book, fields).await,
            Backend::Demo(demo) => demo.create_contact(book, fields).await,
        }
    }

    async fn add_to_contact(&self, id: String, fields: ContactFields) -> Result<Contact, ContactsError> {
        match self {
            Backend::Core(client) => client.add_to_contact(id, fields).await,
            Backend::Demo(demo) => demo.add_to_contact(id, fields).await,
        }
    }

    async fn update_contact(&self, id: String, etag: String, fields: ContactFields) -> Result<Contact, ContactsError> {
        match self {
            Backend::Core(client) => client.update_contact(id, etag, fields).await,
            Backend::Demo(demo) => demo.update_contact(id, etag, fields).await,
        }
    }

    async fn set_favorite(&self, id: String, favorite: bool) -> Result<(), ContactsError> {
        match self {
            Backend::Core(client) => client.set_favorite(id, favorite).await,
            Backend::Demo(demo) => demo.set_favorite(id, favorite).await,
        }
    }

    async fn add_to_group(&self, id: String, etag: String, group: String) -> Result<Contact, ContactsError> {
        match self {
            Backend::Core(client) => client.add_to_group(id, etag, group).await,
            Backend::Demo(demo) => demo.add_to_group(id, etag, group).await,
        }
    }

    async fn remove_from_group(&self, id: String, etag: String, group: String) -> Result<Contact, ContactsError> {
        match self {
            Backend::Core(client) => client.remove_from_group(id, etag, group).await,
            Backend::Demo(demo) => demo.remove_from_group(id, etag, group).await,
        }
    }

    async fn mark_used(&self, id: String) -> Result<(), ContactsError> {
        match self {
            Backend::Core(client) => client.mark_used(id).await,
            Backend::Demo(demo) => demo.mark_used(id).await,
        }
    }

    async fn set_photo(&self, id: String, etag: String, data: Option<Vec<u8>>) -> Result<Contact, ContactsError> {
        match self {
            Backend::Core(client) => client.set_photo(id, etag, data).await,
            Backend::Demo(demo) => demo.set_photo(id, etag, data).await,
        }
    }

    async fn move_contact(&self, id: String, etag: String, to_book: String) -> Result<Moved, ContactsError> {
        match self {
            Backend::Core(client) => client.move_contact(id, etag, to_book).await,
            Backend::Demo(demo) => demo.move_contact(id, etag, to_book).await,
        }
    }

    async fn delete_contact(&self, id: String, etag: String) -> Result<(), ContactsError> {
        match self {
            Backend::Core(client) => client.delete_contact(id, etag).await,
            Backend::Demo(demo) => demo.delete_contact(id, etag).await,
        }
    }

    async fn resolve_conflict(&self, id: String, keep: Keep, etag: String) -> Result<Vec<Contact>, ContactsError> {
        match self {
            Backend::Core(client) => client.resolve_conflict(id, keep, etag).await,
            Backend::Demo(demo) => demo.resolve_conflict(id, keep, etag).await,
        }
    }
}

/// What the window shows, as one load brought it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loaded {
    /// The books shown: the hidden ones only when the person wants them.
    pub books: Vec<AddressBook>,
    /// Every contact of those books, sorted by family name as the core sends them.
    pub contacts: Vec<ContactSummary>,
    /// The birthdays of the coming year, the soonest first.
    pub birthdays: Vec<Birthday>,
    /// The ids of the contacts used last, the latest first.
    pub recent: Vec<String>,
}

/// Every page of `query` (the core gives at most [`PAGE`] at once).
pub async fn all_pages(backend: &Backend, mut query: ContactQuery) -> Result<Vec<ContactSummary>, ContactsError> {
    query.limit = Some(PAGE);
    let mut contacts = Vec::new();
    loop {
        let page = backend.list_contacts(query.clone()).await?;
        contacts.extend(page.contacts);
        match page.next_cursor {
            Some(cursor) if query.cursor.as_deref() != Some(cursor.as_str()) => query.cursor = Some(cursor),
            _ => return Ok(contacts),
        }
    }
}

/// Loads what the window shows (step 7 of the plan): the books, every contact of the books shown, the
/// birthdays of the coming year and the contacts used last. Books the person hides by default
/// ([`AddressBook::hidden_by_default`]) come along, with their contacts, only when the settings say so.
/// Birthdays and the recent list are extras: when they fail, the list still shows.
pub async fn load(backend: Backend) -> Result<Loaded, ContactsError> {
    let show_hidden = backend.settings().await.hidden_books;
    let mut books = backend.address_books(None).await?;
    books.retain(|book| show_hidden || !book.hidden_by_default);
    let shown = |book: &str| books.iter().any(|known| known.id == book);
    let mut contacts = all_pages(&backend, ContactQuery::default()).await?;
    contacts.retain(|contact| shown(&contact.book));
    let mut birthdays = backend.upcoming_birthdays(Some(MAX_BIRTHDAY_DAYS)).await.unwrap_or_default();
    birthdays.retain(|birthday| shown(&birthday.contact.book));
    let recent = backend
        .list_contacts(ContactQuery { recent: true, limit: Some(PAGE), ..ContactQuery::default() })
        .await
        .map(|page| page.contacts.into_iter().filter(|c| shown(&c.book)).map(|c| c.id).collect())
        .unwrap_or_default();
    Ok(Loaded { books, contacts, birthdays, recent })
}

/// The ids of the contacts matching `query` (`contacts.list_contacts` with `query`): the core searches
/// names, company, mail, numbers (also by digits), town and note.
pub async fn search(backend: Backend, query: String) -> Result<Vec<String>, ContactsError> {
    let found = all_pages(&backend, ContactQuery { query: Some(query), ..ContactQuery::default() }).await?;
    Ok(found.into_iter().map(|contact| contact.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_contacts::demo;

    fn demo_backend() -> Backend {
        Backend::Demo(Arc::new(DemoSource::new(chrono::NaiveDate::from_ymd_opt(2026, 10, 28).expect("date"))))
    }

    #[tokio::test]
    async fn a_load_brings_every_page_and_the_extras() {
        let loaded = load(demo_backend()).await.expect("load");
        assert_eq!(loaded.books.len(), 2);
        assert_eq!(loaded.contacts.len(), 40, "both books, every contact");
        assert!(!loaded.birthdays.is_empty());
        assert!(loaded.recent.is_empty(), "nothing used yet");
        assert!(loaded.contacts.iter().any(|c| !c.groups.is_empty()), "groups come with the list");
    }

    #[tokio::test]
    async fn pages_are_joined() {
        let backend = demo_backend();
        let all = all_pages(&backend, ContactQuery::default()).await.expect("all");
        assert_eq!(all.len(), 40);
        let found = search(backend.clone(), "Berg".into()).await.expect("search");
        assert!(found.iter().any(|id| id.ends_with("demo-01")), "{found:?}");
        backend.mark_used(found[0].clone()).await.expect("used");
        let loaded = load(backend).await.expect("load");
        assert_eq!(loaded.recent.first(), found.first());
        assert!(loaded.books.iter().any(|b| b.id == demo::TEAM));
    }

    #[tokio::test]
    async fn the_demo_shows_no_hidden_books_and_has_maps() {
        assert_eq!(demo_backend().settings().await, Shown { hidden_books: false, maps: true, lookup: true });
    }

    /// Issue #355: the demo places the first address at the contact's `GEO`, opens nothing and has no
    /// picture of its own (the window draws its sketch).
    #[tokio::test]
    async fn the_demo_locates_the_first_address_by_its_geo() {
        let backend = demo_backend();
        let first = all_pages(&backend, ContactQuery::default()).await.expect("all")[0].id.clone();
        let located = backend.locate(first.clone(), 0).await.expect("located");
        assert!(matches!(located, ContactLocation::Found { source: LocationSource::Vcard, .. }), "{located:?}");
        assert_eq!(backend.locate(first.clone(), 1).await, Ok(ContactLocation::NotFound));
        assert_eq!(backend.locate("nowhere".into(), 0).await, Err("not-found".to_string()));
        assert_eq!(backend.map(Geo { lat: 1.0, lon: 2.0 }, false).await, None);
        assert_eq!(backend.open_in_maps(first, 0).await, Ok(()));
    }
}
