//! Where the window's requests go: the core over its socket ([`ContactsClient`]), or the sample in
//! memory (`--demo`, [`DemoSource`]).
//!
//! The window reads and changes nothing itself (`CLAUDE.md`, "Jede Funktion dreimal"): every action is
//! a command of the API area `contacts` through [`ContactsSource`], which both sides speak. Besides
//! them the window asks the core what it reads of the settings ([`Backend::settings`],
//! `settings.get_settings`: the hidden address books, maps and the lookup of contact addresses) and,
//! for the map under an address (issue #355), `contacts.locate_contact`, `calendar.map_preview` and
//! `contacts.open_in_maps` ([`Backend::locate`], [`Backend::map`], [`Backend::open_in_maps`]), and
//! `contacts.set_location` for a pin set by hand ([`Backend::set_location`], issue #854).
//!
//! [`load`] is what the window shows: the books, every contact of them (all pages), the birthdays of
//! the coming year and the contacts used last.

use std::sync::Arc;

use torro_contacts::demo::DemoSource;
use torro_contacts::{
    AddressBook, Birthday, Contact, ContactFields, ContactPage, ContactQuery, ContactSummary, ContactsError,
    ContactsSource, DuplicateGroup, Group, Imported, Keep, Merged, Moved, PhotoSize,
};
use torro_gui::tile_map::TileKey;
use torro_ipc::Request;
use torro_ipc::Response;
use torro_ipc::contacts::{
    AddressRef, ContactLocation, ContactPin, ContactsClient, Geo, LocationSource, LocationsQuery, LocationsStatus,
    MAX_BIRTHDAY_DAYS,
};

/// The demo contacts the map of all contacts finds without a point, to be looked up (issue #357).
pub const DEMO_PENDING: [&str; 2] = ["demo-08", "demo-12"];

/// The demo contact the map of all contacts counts as without an address (issue #357).
pub const DEMO_WITHOUT_ADDRESS: &str = "demo-20";

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

    /// What the window reads of the settings ([`Shown`]); `None` from a core that does not answer.
    /// The demo shows no hidden books and has maps on.
    pub async fn settings(&self) -> Option<Shown> {
        let Backend::Core(client) = self else {
            return Some(Shown { hidden_books: false, maps: true, contact_lookup: true });
        };
        match torro_ipc::client::call(&client.socket, &Request::Settings).await {
            Ok(Response::Settings { settings }) => Some(Shown {
                hidden_books: settings.contacts_show_hidden_books,
                maps: settings.maps.enabled,
                contact_lookup: settings.maps.contact_lookup,
            }),
            _ => None,
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

    /// Every contact of a selection on the map (`contacts.list_locations`, issue #357); the error is the
    /// core's code. The demo puts each contact at its `GEO`, but leaves [`DEMO_PENDING`] to be looked
    /// up and gives [`DEMO_WITHOUT_ADDRESS`] no address.
    pub async fn locations(&self, query: LocationsQuery) -> Result<(Vec<ContactPin>, LocationsStatus), String> {
        let demo = match self {
            Backend::Core(client) => return client.locations(query).await,
            Backend::Demo(demo) => demo,
        };
        let LocationsQuery { book, group, favorites, query } = query;
        let selection = ContactQuery { book, group, favorites, query, ..ContactQuery::default() };
        let found = all_pages(self, selection).await.map_err(|_| "error".to_string())?;
        let mut pins = Vec::new();
        let mut status = LocationsStatus::default();
        for summary in found {
            let uid = summary.id.rsplit('/').next().unwrap_or_default();
            if uid == DEMO_WITHOUT_ADDRESS {
                status.without_address += 1;
                continue;
            }
            if DEMO_PENDING.contains(&uid) {
                status.pending.push(AddressRef { id: summary.id.clone(), address: 0 });
                continue;
            }
            let Ok(contact) = demo.get_contact(summary.id.clone()).await else { continue };
            if let Some(geo) = contact.geo {
                pins.push(ContactPin {
                    id: contact.id.clone(),
                    name: contact.shown_name(),
                    address: 0,
                    geo: Geo { lat: geo.lat, lon: geo.lon },
                    source: LocationSource::Vcard,
                    untrusted: true,
                });
            }
        }
        status.located = pins.len() as u32;
        Ok((pins, status))
    }

    /// One map tile as PNG (`calendar.map_tile`, issue #357); `None` when it cannot be had, and always
    /// in the demo, which draws its sketch instead ([`torro_gui::tile_map::demo_tile`]).
    pub async fn tile(&self, key: TileKey, dark: bool) -> Option<Vec<u8>> {
        let Backend::Core(client) = self else { return None };
        client.tile(key.z, key.x, key.y, dark).await
    }

    /// Opens the address number `address` of the contact `id` in the maps app of the settings
    /// (`contacts.open_in_maps`); the error is the core's message. The demo opens nothing.
    pub async fn open_in_maps(&self, id: String, address: usize) -> Result<(), String> {
        match self {
            Backend::Core(client) => client.open_in_maps(id, address).await,
            Backend::Demo(_) => Ok(()),
        }
    }

    /// `contacts.set_location` (issue #854): the pin of the first address, written as the vCard's `GEO`.
    /// The demo answers with the contact at the new point and keeps nothing.
    pub async fn set_location(
        &self,
        id: String,
        etag: String,
        geo: Geo,
        replace: bool,
    ) -> Result<Contact, ContactsError> {
        match self {
            Backend::Core(client) => client.set_location(id, etag, geo, replace).await,
            Backend::Demo(demo) => {
                let contact = demo.get_contact(id).await?;
                let point = torro_contacts::model::Geo::new(geo.lat, geo.lon);
                Ok(Contact { geo: point, ..contact })
            }
        }
    }
}

/// What the window reads of the settings (`settings.get_settings`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shown {
    /// The person wants the hidden address books (Nextcloud's system address book, "Recently
    /// contacted") in the app: `contacts_show_hidden_books`.
    pub hidden_books: bool,
    /// Maps and place search are on (`maps.enabled`). Whether the lookup of contact addresses is on
    /// the core answers itself (`no_consent`, issue #355).
    pub maps: bool,
    /// The person allowed looking up contact addresses (`maps.contactLookup`): only then the window
    /// offers the map of all contacts (issue #357).
    pub contact_lookup: bool,
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

    /// The demo reads no files: for it the window reads the one chosen here and sends its text.
    async fn import_vcards(
        &self,
        book: String,
        data: Option<String>,
        path: Option<String>,
        dry_run: bool,
    ) -> Result<Imported, ContactsError> {
        match self {
            Backend::Core(client) => client.import_vcards(book, data, path, dry_run).await,
            Backend::Demo(demo) => {
                let data = match (data, path) {
                    (None, Some(path)) => Some(
                        String::from_utf8_lossy(
                            &std::fs::read(&path).map_err(|err| ContactsError::Other(err.to_string()))?,
                        )
                        .into_owned(),
                    ),
                    (data, _) => data,
                };
                demo.import_vcards(book, data, None, dry_run).await
            }
        }
    }

    async fn export_vcards(
        &self,
        book: Option<String>,
        group: Option<String>,
        ids: Vec<String>,
    ) -> Result<(String, u32), ContactsError> {
        match self {
            Backend::Core(client) => client.export_vcards(book, group, ids).await,
            Backend::Demo(demo) => demo.export_vcards(book, group, ids).await,
        }
    }

    async fn find_duplicates(&self, book: Option<String>) -> Result<Vec<DuplicateGroup>, ContactsError> {
        match self {
            Backend::Core(client) => client.find_duplicates(book).await,
            Backend::Demo(demo) => demo.find_duplicates(book).await,
        }
    }

    async fn merge_contacts(
        &self,
        ids: Vec<String>,
        etags: Vec<String>,
        keep: String,
    ) -> Result<Merged, ContactsError> {
        match self {
            Backend::Core(client) => client.merge_contacts(ids, etags, keep).await,
            Backend::Demo(demo) => demo.merge_contacts(ids, etags, keep).await,
        }
    }

    async fn dismiss_duplicates(&self, ids: Vec<String>) -> Result<(), ContactsError> {
        match self {
            Backend::Core(client) => client.dismiss_duplicates(ids).await,
            Backend::Demo(demo) => demo.dismiss_duplicates(ids).await,
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
    /// The contacts that look like one person (`contacts.find_duplicates`, issue #726), of the books
    /// shown only; groups with fewer than two of them left out.
    pub duplicates: Vec<DuplicateGroup>,
    /// What the settings said (`None` from a core that did not answer): whether the map of all
    /// contacts is offered (issue #357).
    pub shown: Option<Shown>,
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
/// Birthdays, the recent list and the duplicates are extras: when they fail, the list still shows.
pub async fn load(backend: Backend) -> Result<Loaded, ContactsError> {
    let settings = backend.settings().await;
    let show_hidden = settings.is_some_and(|shown| shown.hidden_books);
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
    let mut duplicates = backend.find_duplicates(None).await.unwrap_or_default();
    for group in &mut duplicates {
        group.contacts.retain(|contact| shown(&contact.book));
    }
    duplicates.retain(|group| group.contacts.len() >= 2);
    Ok(Loaded { books, contacts, birthdays, recent, duplicates, shown: settings })
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
        assert_eq!(loaded.contacts.len(), 41, "both books, every contact");
        assert!(!loaded.birthdays.is_empty());
        assert!(loaded.recent.is_empty(), "nothing used yet");
        assert!(loaded.contacts.iter().any(|c| !c.groups.is_empty()), "groups come with the list");
        assert_eq!(loaded.duplicates.len(), 1, "Anna Berg twice (issue #726)");
    }

    #[tokio::test]
    async fn pages_are_joined() {
        let backend = demo_backend();
        let all = all_pages(&backend, ContactQuery::default()).await.expect("all");
        assert_eq!(all.len(), 41);
        let found = search(backend.clone(), "Berg".into()).await.expect("search");
        assert!(found.iter().any(|id| id.ends_with("demo-01")), "{found:?}");
        backend.mark_used(found[0].clone()).await.expect("used");
        let loaded = load(backend).await.expect("load");
        assert_eq!(loaded.recent.first(), found.first());
        assert!(loaded.books.iter().any(|b| b.id == demo::TEAM));
    }

    #[tokio::test]
    async fn the_demo_shows_no_hidden_books_and_has_maps() {
        let shown = Shown { hidden_books: false, maps: true, contact_lookup: true };
        assert_eq!(demo_backend().settings().await, Some(shown));
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

    /// Issue #357: the demo's map of all contacts has pins, two addresses to look up and one contact
    /// without an address; no tile of its own.
    #[tokio::test]
    async fn the_demo_map_has_pins_pending_and_one_without_address() {
        let backend = demo_backend();
        let (pins, status) = backend.locations(LocationsQuery::default()).await.expect("locations");
        assert_eq!(pins.len(), 38);
        assert_eq!(status.located, 38);
        assert_eq!(status.pending.len(), 2);
        assert_eq!(status.without_address, 1);
        let favorites = LocationsQuery { favorites: true, ..LocationsQuery::default() };
        assert!(backend.locations(favorites).await.expect("favourites").0.len() <= 5);
        assert_eq!(backend.tile(TileKey { z: 2, x: 1, y: 1 }, false).await, None);
    }
}
