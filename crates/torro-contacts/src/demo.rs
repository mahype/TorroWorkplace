//! Sample contacts in memory, for building and trying the contacts app without a server.
//!
//! `torrocontacts --demo` uses it; changes live until the app closes. One Nextcloud account holds
//! two address books, "Contacts" (writable) and "Team" (shared with us, read-only), with 40
//! contacts in three groups, five favourites, pictures and initials, birthdays in the next 14 days
//! around "today", one contact in conflict and one changed offline, and addresses in several
//! countries with coordinates (`GEO`) for the map of a later stage.
//!
//! The sample content is data, not text of the suite, so it is not translated (as in
//! `torro_pim::demo`).

use std::sync::{Mutex, MutexGuard};

use chrono::{Datelike, Days, Local, NaiveDate};

use crate::model::{
    AddressBook, Birthday, Contact, ContactFields, ContactPage, ContactQuery, ContactState, FieldKind, Geo, Group,
    Keep, Labeled, Moved, Name, PartialDate, PhotoRef, PhotoSize, PostalAddress, book_of, contact_id,
};
use crate::provider::ContactsError;
use crate::source::ContactsSource;

/// The account of the sample.
pub const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
/// The writable book.
pub const CONTACTS: &str = "nextcloud:anna@cloud.example.org/contacts";
/// The read-only book shared with the account.
pub const TEAM: &str = "nextcloud:anna@cloud.example.org/team";

#[derive(Debug, Default)]
struct Data {
    books: Vec<AddressBook>,
    contacts: Vec<Contact>,
    recent: Vec<String>,
    next: u64,
}

impl Data {
    fn find(&self, id: &str) -> Result<usize, ContactsError> {
        self.contacts.iter().position(|c| c.id == id).ok_or(ContactsError::NotFound)
    }

    fn book(&self, id: &str) -> Result<&AddressBook, ContactsError> {
        self.books.iter().find(|b| b.id == id).ok_or(ContactsError::NotFound)
    }

    /// The contact `id` when `etag` is its version and its book may be written.
    fn writable(&self, id: &str, etag: Option<&str>) -> Result<usize, ContactsError> {
        let at = self.find(id)?;
        let contact = &self.contacts[at];
        if contact.readonly {
            return Err(ContactsError::ReadOnly);
        }
        if let Some(etag) = etag
            && contact.etag.as_deref() != Some(etag)
        {
            return Err(ContactsError::Conflict { server_vcard: None, etag: contact.etag.clone() });
        }
        Ok(at)
    }

    fn version(&mut self) -> String {
        self.next += 1;
        format!("\"demo-{}\"", self.next)
    }

    fn save(&mut self, at: usize) -> Contact {
        let etag = self.version();
        let contact = &mut self.contacts[at];
        contact.etag = Some(etag);
        if contact.state == ContactState::Synced || contact.state == ContactState::Pending {
            contact.state = ContactState::Synced;
        }
        contact.clone()
    }
}

/// Contacts in memory; see the module docs.
#[derive(Debug)]
pub struct DemoSource {
    data: Mutex<Data>,
    today: NaiveDate,
}

impl Default for DemoSource {
    fn default() -> Self {
        DemoSource::new(Local::now().date_naive())
    }
}

impl DemoSource {
    /// The sample with birthdays around `today`.
    pub fn new(today: NaiveDate) -> DemoSource {
        let (books, contacts) = sample(today);
        DemoSource { data: Mutex::new(Data { books, contacts, recent: Vec::new(), next: 100 }), today }
    }

    fn lock(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn matches(contact: &Contact, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let digits: String = query.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_start_matches('0');
    let text = [
        contact.shown_name(),
        contact.org.clone().unwrap_or_default(),
        contact.note.clone().unwrap_or_default(),
        contact.emails.iter().map(|e| e.value.as_str()).collect::<Vec<_>>().join(" "),
        contact.addresses.iter().map(|a| a.value.one_line()).collect::<Vec<_>>().join(" "),
    ]
    .join(" ")
    .to_lowercase();
    text.contains(&query)
        || (digits.len() >= 3
            && contact
                .phones
                .iter()
                .any(|p| p.value.chars().filter(char::is_ascii_digit).collect::<String>().contains(digits)))
}

impl ContactsSource for DemoSource {
    async fn address_books(&self, account: Option<String>) -> Result<Vec<AddressBook>, ContactsError> {
        let data = self.lock();
        Ok(data
            .books
            .iter()
            .filter(|b| account.as_ref().is_none_or(|a| *a == b.account))
            .map(|b| AddressBook {
                count: u32::try_from(data.contacts.iter().filter(|c| c.book == b.id).count()).unwrap_or(u32::MAX),
                ..b.clone()
            })
            .collect())
    }

    async fn list_contacts(&self, query: ContactQuery) -> Result<ContactPage, ContactsError> {
        let data = self.lock();
        if let Some(book) = &query.book {
            data.book(book)?;
        }
        let mut found: Vec<&Contact> = data
            .contacts
            .iter()
            .filter(|c| query.book.as_ref().is_none_or(|b| *b == c.book))
            .filter(|c| query.group.as_ref().is_none_or(|g| c.groups.contains(g)))
            .filter(|c| !query.favorites || c.favorite)
            .filter(|c| !query.recent || data.recent.contains(&c.id))
            .filter(|c| query.query.as_deref().is_none_or(|q| matches(c, q)))
            .collect();
        if query.recent {
            found.sort_by_key(|c| data.recent.iter().position(|r| *r == c.id));
        } else {
            found.sort_by_key(|c| (c.name.family.to_lowercase(), c.name.given.to_lowercase(), c.shown_name()));
        }
        let start: usize = query.cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0);
        let size = query.page_size();
        let contacts = found.iter().skip(start).take(size).map(|c| c.summary()).collect();
        let next_cursor = (start + size < found.len()).then(|| (start + size).to_string());
        Ok(ContactPage { contacts, next_cursor })
    }

    async fn get_contact(&self, id: String) -> Result<Contact, ContactsError> {
        let data = self.lock();
        Ok(data.contacts[data.find(&id)?].clone())
    }

    async fn get_photo(&self, id: String, size: PhotoSize) -> Result<Vec<u8>, ContactsError> {
        let data = self.lock();
        let contact = &data.contacts[data.find(&id)?];
        let photo = contact.photo.as_ref().ok_or(ContactsError::NotFound)?;
        let seed = photo.version.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(u32::from(b)));
        let colour = [(seed % 160 + 60) as u8, ((seed >> 8) % 160 + 60) as u8, ((seed >> 16) % 160 + 60) as u8];
        Ok(solid_png(size.pixels(), colour))
    }

    async fn list_groups(&self, book: Option<String>) -> Result<Vec<Group>, ContactsError> {
        let data = self.lock();
        let mut groups: Vec<Group> = Vec::new();
        for contact in data.contacts.iter().filter(|c| book.as_ref().is_none_or(|b| *b == c.book)) {
            for name in &contact.groups {
                match groups.iter_mut().find(|g| g.book == contact.book && g.name == *name) {
                    Some(group) => group.count += 1,
                    None => {
                        groups.push(Group { book: contact.book.clone(), name: name.clone(), count: 1, untrusted: true })
                    }
                }
            }
        }
        groups.sort_by(|a, b| (&a.book, &a.name).cmp(&(&b.book, &b.name)));
        Ok(groups)
    }

    async fn upcoming_birthdays(&self, days: Option<u32>) -> Result<Vec<Birthday>, ContactsError> {
        let data = self.lock();
        let mut birthdays: Vec<Birthday> =
            data.contacts.iter().filter_map(|c| Birthday::upcoming(c, self.today, days.unwrap_or(14))).collect();
        birthdays.sort_by(|a, b| (a.next, &a.contact.display_name).cmp(&(b.next, &b.contact.display_name)));
        Ok(birthdays)
    }

    async fn create_contact(&self, book: String, fields: ContactFields) -> Result<Contact, ContactsError> {
        let mut data = self.lock();
        let shelf = data.book(&book)?;
        if !shelf.writable {
            return Err(ContactsError::ReadOnly);
        }
        let uid = format!("demo-new-{}", data.next);
        let mut contact = Contact { id: contact_id(&book, &uid), book, uid, untrusted: true, ..Contact::default() };
        fields.apply(&mut contact).map_err(|err| ContactsError::Other(err.to_string()))?;
        data.contacts.push(contact);
        let at = data.contacts.len() - 1;
        Ok(data.save(at))
    }

    async fn add_to_contact(&self, id: String, fields: ContactFields) -> Result<Contact, ContactsError> {
        let mut data = self.lock();
        let at = data.writable(&id, None)?;
        fields.add_to(&mut data.contacts[at]).map_err(|err| ContactsError::Unsupported(err.to_string()))?;
        Ok(data.save(at))
    }

    async fn update_contact(&self, id: String, etag: String, fields: ContactFields) -> Result<Contact, ContactsError> {
        let mut data = self.lock();
        let at = data.writable(&id, Some(&etag))?;
        fields.apply(&mut data.contacts[at]).map_err(|err| ContactsError::Other(err.to_string()))?;
        Ok(data.save(at))
    }

    async fn set_favorite(&self, id: String, favorite: bool) -> Result<(), ContactsError> {
        let mut data = self.lock();
        let at = data.find(&id)?;
        data.contacts[at].favorite = favorite;
        Ok(())
    }

    async fn add_to_group(&self, id: String, etag: String, group: String) -> Result<Contact, ContactsError> {
        let mut data = self.lock();
        let at = data.writable(&id, Some(&etag))?;
        let group = group.trim().to_string();
        if !group.is_empty() && !data.contacts[at].groups.contains(&group) {
            data.contacts[at].groups.push(group);
        }
        Ok(data.save(at))
    }

    async fn remove_from_group(&self, id: String, etag: String, group: String) -> Result<Contact, ContactsError> {
        let mut data = self.lock();
        let at = data.writable(&id, Some(&etag))?;
        data.contacts[at].groups.retain(|g| *g != group);
        Ok(data.save(at))
    }

    async fn mark_used(&self, id: String) -> Result<(), ContactsError> {
        let mut data = self.lock();
        data.find(&id)?;
        data.recent.retain(|r| *r != id);
        data.recent.insert(0, id);
        data.recent.truncate(50);
        Ok(())
    }

    async fn set_photo(&self, id: String, etag: String, photo: Option<Vec<u8>>) -> Result<Contact, ContactsError> {
        let mut data = self.lock();
        let at = data.writable(&id, Some(&etag))?;
        let version = data.next.to_string();
        data.contacts[at].photo = photo.map(|_| PhotoRef { version });
        Ok(data.save(at))
    }

    async fn move_contact(&self, id: String, etag: String, to_book: String) -> Result<Moved, ContactsError> {
        let mut data = self.lock();
        let at = data.writable(&id, Some(&etag))?;
        let target = data.book(&to_book)?;
        if !target.writable {
            return Err(ContactsError::ReadOnly);
        }
        let contact = &mut data.contacts[at];
        contact.id = contact_id(&to_book, &contact.uid);
        contact.book = to_book;
        Ok(Moved { contact: data.save(at), left_in_source: false })
    }

    async fn delete_contact(&self, id: String, etag: String) -> Result<(), ContactsError> {
        let mut data = self.lock();
        let at = data.writable(&id, Some(&etag))?;
        data.contacts.remove(at);
        data.recent.retain(|r| *r != id);
        Ok(())
    }

    async fn resolve_conflict(&self, id: String, keep: Keep, etag: String) -> Result<Vec<Contact>, ContactsError> {
        let mut data = self.lock();
        let at = data.writable(&id, Some(&etag))?;
        let mine = data.contacts[at].clone();
        let Some(theirs) = mine.server.clone().filter(|_| mine.state == ContactState::Conflict) else {
            return Err(ContactsError::Unsupported("The contact is not in conflict.".into()));
        };
        let settled = |mut contact: Contact| {
            contact.state = ContactState::Synced;
            contact.server = None;
            contact
        };
        match keep {
            Keep::Mine => {
                data.contacts[at] = settled(mine);
                Ok(vec![data.save(at)])
            }
            Keep::Theirs => {
                data.contacts[at] = settled(*theirs);
                Ok(vec![data.save(at)])
            }
            Keep::Both => {
                data.contacts[at] = settled(*theirs);
                let kept = data.save(at);
                let uid = format!("demo-new-{}", data.next);
                let copy = Contact { id: contact_id(&mine.book, &uid), uid, ..settled(mine) };
                data.contacts.push(copy);
                let last = data.contacts.len() - 1;
                Ok(vec![kept, data.save(last)])
            }
        }
    }
}

/// A square PNG of one colour, `edge` pixels wide: the demo's pictures.
pub fn solid_png(edge: u32, rgb: [u8; 3]) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    let row: Vec<u8> = std::iter::once(0).chain(rgb.iter().copied().cycle().take(edge as usize * 3)).collect();
    let raw: Vec<u8> = row.iter().copied().cycle().take(row.len() * edge as usize).collect();
    // zlib with stored (uncompressed) blocks: no compressor needed for a few kilobytes.
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(i + 1 == blocks.len()));
        let len = u16::try_from(block.len()).unwrap_or(u16::MAX);
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut header = Vec::new();
    header.extend_from_slice(&edge.to_be_bytes());
    header.extend_from_slice(&edge.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// The people of the sample: given, family, company, town, postal code, country, coordinates.
type Person = (&'static str, &'static str, &'static str, &'static str, &'static str, &'static str, f64, f64);

const PEOPLE: [Person; 40] = [
    ("Anna", "Berg", "Example GmbH", "Berlin", "10115", "Germany", 52.532, 13.384),
    ("Jonas", "Keller", "Keller & Söhne", "Zürich", "8002", "Switzerland", 47.364, 8.531),
    ("Maria", "Lopez", "Estudio Lopez", "Madrid", "28013", "Spain", 40.416, -3.703),
    ("Pierre", "Martin", "Atelier Martin", "Paris", "75004", "France", 48.856, 2.352),
    ("Lena", "Fischer", "Fotografie Fischer", "Leipzig", "04109", "Germany", 51.339, 12.373),
    ("Max", "Weber", "Weber Bau", "Hamburg", "20095", "Germany", 53.551, 9.993),
    ("Sofia", "Rossi", "Rossi Design", "Milan", "20121", "Italy", 45.464, 9.190),
    ("Lukas", "Hoffmann", "Example GmbH", "Munich", "80331", "Germany", 48.137, 11.575),
    ("Emma", "Johansson", "Nordic Labs", "Stockholm", "111 52", "Sweden", 59.329, 18.068),
    ("Noah", "Dubois", "", "Lyon", "69002", "France", 45.764, 4.835),
    ("Mia", "Schneider", "Schneider Law", "Cologne", "50667", "Germany", 50.937, 6.960),
    ("Liam", "O'Brien", "Harbour Ltd", "Dublin", "D02", "Ireland", 53.349, -6.260),
    ("Hannah", "Wagner", "", "Vienna", "1010", "Austria", 48.208, 16.373),
    ("Elias", "Becker", "Example GmbH", "Berlin", "10178", "Germany", 52.521, 13.410),
    ("Clara", "Schulz", "Kita Sonnenschein", "Potsdam", "14467", "Germany", 52.396, 13.058),
    ("Ben", "Koch", "Koch Catering", "Frankfurt", "60311", "Germany", 50.110, 8.682),
    ("Lea", "Richter", "", "Dresden", "01067", "Germany", 51.050, 13.737),
    ("Paul", "Klein", "Klein IT", "Stuttgart", "70173", "Germany", 48.775, 9.182),
    ("Ida", "Larsen", "Fjord AS", "Oslo", "0150", "Norway", 59.913, 10.752),
    ("Felix", "Wolf", "Wolf Media", "Düsseldorf", "40213", "Germany", 51.227, 6.773),
    ("Nora", "Schröder", "", "Bremen", "28195", "Germany", 53.079, 8.801),
    ("Jakob", "Neumann", "Example GmbH", "Berlin", "10117", "Germany", 52.516, 13.388),
    ("Amelie", "Schwarz", "Praxis Schwarz", "Hanover", "30159", "Germany", 52.375, 9.732),
    ("David", "Cohen", "Cohen & Partners", "London", "EC1A", "United Kingdom", 51.517, -0.097),
    ("Laura", "Zimmermann", "", "Freiburg", "79098", "Germany", 47.999, 7.842),
    ("Tim", "Braun", "Braun Bikes", "Nuremberg", "90402", "Germany", 49.452, 11.077),
    ("Sara", "Hartmann", "Example GmbH", "Berlin", "10119", "Germany", 52.530, 13.402),
    ("Leon", "Krüger", "Krüger Logistik", "Kiel", "24103", "Germany", 54.323, 10.122),
    ("Marie", "Lange", "", "Münster", "48143", "Germany", 51.962, 7.626),
    ("Finn", "Jansen", "Jansen BV", "Amsterdam", "1012", "Netherlands", 52.373, 4.893),
    ("Julia", "Peters", "Peters PR", "Bonn", "53111", "Germany", 50.737, 7.098),
    ("Tom", "Novak", "Novak s.r.o.", "Prague", "110 00", "Czechia", 50.088, 14.420),
    ("Eva", "Kowalski", "", "Warsaw", "00-001", "Poland", 52.230, 21.012),
    ("Moritz", "Schmitt", "Schmitt Weine", "Mainz", "55116", "Germany", 49.993, 8.247),
    ("Lina", "Haas", "", "Graz", "8010", "Austria", 47.071, 15.439),
    ("Oskar", "Vogel", "Vogel Verlag", "Heidelberg", "69117", "Germany", 49.398, 8.672),
    ("Greta", "Frank", "Example GmbH", "Berlin", "10243", "Germany", 52.512, 13.454),
    ("Anton", "Berger", "Berger Holz", "Innsbruck", "6020", "Austria", 47.269, 11.404),
    ("Yuki", "Tanaka", "Tanaka KK", "Tokyo", "100-0005", "Japan", 35.681, 139.767),
    ("Olivia", "Smith", "Smith & Co", "New York", "10007", "United States", 40.713, -74.006),
];

const GROUPS: [&str; 3] = ["Family", "Friends", "Work"];

fn book(id: &str, name: &str, display_name: &str, writable: bool) -> AddressBook {
    AddressBook {
        id: id.to_string(),
        account: ACCOUNT.to_string(),
        name: name.to_string(),
        display_name: display_name.to_string(),
        count: 0,
        writable,
        hidden_by_default: false,
        untrusted: true,
    }
}

/// The books and contacts of the sample, with birthdays around `today`.
pub fn sample(today: NaiveDate) -> (Vec<AddressBook>, Vec<Contact>) {
    let books = vec![book(CONTACTS, "contacts", "Contacts", true), book(TEAM, "team", "Team (shared)", false)];
    let contacts = PEOPLE
        .iter()
        .enumerate()
        .map(|(i, &(given, family, org, town, code, country, lat, lon))| {
            // Every fifth contact is the team's, read-only.
            let in_team = i % 5 == 4;
            let book = if in_team { TEAM } else { CONTACTS };
            let uid = format!("demo-{:02}", i + 1);
            let mail_name = format!("{}.{}", given.to_lowercase(), family.to_lowercase().replace(['\'', ' '], ""));
            // Birthdays: the first six within the next 14 days, the others spread over the year.
            let birthday = if i < 6 {
                today.checked_add_days(Days::new(i as u64 * 2)).map(|d| PartialDate {
                    year: (i % 2 == 0).then(|| d.year() - 25 - i as i32),
                    month: d.month(),
                    day: d.day(),
                })
            } else if i % 3 == 0 {
                Some(PartialDate { year: Some(1970 + i as i32), month: (i % 12) as u32 + 1, day: (i % 27) as u32 + 1 })
            } else {
                None
            };
            let mut contact = Contact {
                id: contact_id(book, &uid),
                book: book.to_string(),
                uid,
                name: Name { family: family.into(), given: given.into(), ..Name::default() },
                display_name: format!("{given} {family}"),
                org: (!org.is_empty()).then(|| org.to_string()),
                title: (i % 4 == 0).then(|| ["Engineer", "Designer", "Manager", "Photographer"][i / 4 % 4].to_string()),
                photo: (i % 3 == 1).then(|| PhotoRef { version: format!("demo-photo-{i}") }),
                emails: vec![Labeled {
                    value: format!("{mail_name}@example.org"),
                    kind: if org.is_empty() { FieldKind::Home } else { FieldKind::Work },
                    pref: true,
                }],
                phones: vec![Labeled::new(format!("+49 171 {:07}", 2_345_678 + i * 1_111), FieldKind::Mobile)],
                addresses: vec![Labeled::new(
                    PostalAddress {
                        street: format!("Example Street {}", i + 1),
                        locality: town.into(),
                        postal_code: code.into(),
                        country: country.into(),
                        ..PostalAddress::default()
                    },
                    if org.is_empty() { FieldKind::Home } else { FieldKind::Work },
                )],
                birthday,
                groups: vec![GROUPS[i % 3].to_string()],
                geo: Some(Geo { lat, lon }),
                favorite: [0, 1, 4, 6, 10].contains(&i),
                etag: Some(format!("\"demo-{i}\"")),
                readonly: in_team,
                untrusted: true,
                ..Contact::default()
            };
            if i % 7 == 2 {
                contact
                    .urls
                    .push(Labeled::new(format!("https://{}.example.org", family.to_lowercase()), FieldKind::Work));
                contact.note = Some("Met at the spring conference.".into());
            }
            contact
        })
        .collect::<Vec<_>>();
    let mut contacts = contacts;
    // One contact changed here and on the server: the conflict to resolve.
    if let Some(conflict) = contacts.iter_mut().find(|c| c.uid == "demo-04") {
        let mut server = conflict.clone();
        server.phones[0].value = "+33 6 12 34 56 78".into();
        server.etag = Some("\"demo-server\"".into());
        conflict.phones[0].value = "+33 6 98 76 54 32".into();
        conflict.state = ContactState::Conflict;
        conflict.server = Some(Box::new(server));
    }
    // One changed offline, not yet on the server.
    if let Some(pending) = contacts.iter_mut().find(|c| c.uid == "demo-08") {
        pending.title = Some("Head of Research".into());
        pending.state = ContactState::Pending;
    }
    (books, contacts)
}

/// The book of `id` in the sample, for tests and the app's preview.
pub fn sample_book_of(id: &str) -> Option<&'static str> {
    book_of(id, [CONTACTS, TEAM]).map(|(book, _)| book)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> DemoSource {
        DemoSource::new(NaiveDate::from_ymd_opt(2026, 10, 28).expect("date"))
    }

    #[tokio::test]
    async fn the_sample_has_what_the_app_shows() {
        let demo = demo();
        let books = demo.address_books(None).await.expect("books");
        assert_eq!(books.len(), 2);
        assert_eq!(books.iter().filter(|b| !b.writable).count(), 1);
        assert_eq!(books.iter().map(|b| b.count).sum::<u32>(), 40);
        assert_eq!(
            demo.list_groups(None)
                .await
                .expect("groups")
                .iter()
                .map(|g| &g.name)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3
        );
        let favorites =
            demo.list_contacts(ContactQuery { favorites: true, ..ContactQuery::default() }).await.expect("list");
        assert_eq!(favorites.contacts.len(), 5);
        let birthdays = demo.upcoming_birthdays(None).await.expect("birthdays");
        assert!(birthdays.len() >= 5, "{birthdays:?}");
        assert!(birthdays.windows(2).all(|w| w[0].next <= w[1].next));
        let all = demo.list_contacts(ContactQuery { limit: Some(500), ..ContactQuery::default() }).await.expect("all");
        assert!(all.contacts.iter().any(|c| c.state == ContactState::Conflict));
        assert!(all.contacts.iter().any(|c| c.state == ContactState::Pending));
        assert!(all.contacts.iter().any(|c| c.photo.is_some()) && all.contacts.iter().any(|c| c.photo.is_none()));
        let countries: std::collections::BTreeSet<String> =
            demo.lock().contacts.iter().map(|c| c.addresses[0].value.country.clone()).collect();
        assert!(countries.len() > 5);
        let png = demo
            .get_photo(all.contacts.iter().find(|c| c.photo.is_some()).expect("photo").id.clone(), PhotoSize::Small)
            .await
            .expect("png");
        assert_eq!(&png[1..4], b"PNG");
    }

    #[tokio::test]
    async fn search_finds_numbers_by_digits_and_pages() {
        let demo = demo();
        let found = demo
            .list_contacts(ContactQuery { query: Some("0171 234".into()), ..ContactQuery::default() })
            .await
            .expect("search");
        assert!(found.contacts.iter().any(|c| c.display_name == "Anna Berg"), "{found:?}");
        let first =
            demo.list_contacts(ContactQuery { limit: Some(15), ..ContactQuery::default() }).await.expect("page");
        assert_eq!(first.contacts.len(), 15);
        let second = demo
            .list_contacts(ContactQuery {
                limit: Some(30),
                cursor: first.next_cursor.clone(),
                ..ContactQuery::default()
            })
            .await
            .expect("page");
        assert_eq!(second.contacts.len(), 25);
        assert_eq!(second.next_cursor, None);
    }

    #[tokio::test]
    async fn changes_live_in_memory_and_respect_read_only_and_versions() {
        let demo = demo();
        let created = demo
            .create_contact(
                CONTACTS.into(),
                ContactFields { display_name: Some("New Person".into()), ..ContactFields::default() },
            )
            .await
            .expect("create");
        assert_eq!(sample_book_of(&created.id), Some(CONTACTS));
        assert_eq!(demo.create_contact(TEAM.into(), ContactFields::default()).await, Err(ContactsError::ReadOnly));
        let etag = created.etag.clone().expect("etag");
        let changed = demo
            .update_contact(
                created.id.clone(),
                etag.clone(),
                ContactFields { org: Some("Acme".into()), ..ContactFields::default() },
            )
            .await
            .expect("update");
        assert!(matches!(
            demo.update_contact(created.id.clone(), etag, ContactFields::default()).await,
            Err(ContactsError::Conflict { .. })
        ));
        demo.set_favorite(created.id.clone(), true).await.expect("favorite");
        demo.mark_used(created.id.clone()).await.expect("used");
        let recent =
            demo.list_contacts(ContactQuery { recent: true, ..ContactQuery::default() }).await.expect("recent");
        assert_eq!(recent.contacts[0].id, created.id);
        let conflict = demo.get_contact(contact_id(CONTACTS, "demo-04")).await.expect("conflict");
        let kept = demo
            .resolve_conflict(conflict.id.clone(), Keep::Both, conflict.etag.clone().expect("etag"))
            .await
            .expect("both");
        assert_eq!(kept.len(), 2);
        demo.delete_contact(changed.id.clone(), changed.etag.clone().expect("etag")).await.expect("delete");
        assert_eq!(demo.get_contact(changed.id).await, Err(ContactsError::NotFound));
    }

    #[test]
    fn the_model_survives_json() {
        let (books, contacts) = sample(NaiveDate::from_ymd_opt(2026, 1, 1).expect("date"));
        for contact in &contacts {
            let json = serde_json::to_string(contact).expect("json");
            let back: Contact = serde_json::from_str(&json).expect("back");
            assert_eq!(&back, contact);
        }
        let json = serde_json::to_string(&books).expect("json");
        assert_eq!(serde_json::from_str::<Vec<AddressBook>>(&json).expect("back"), books);
    }
}
