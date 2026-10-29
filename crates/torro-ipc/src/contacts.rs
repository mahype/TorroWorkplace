//! The API area `contacts` (`docs/kontakte.md`): limits of its commands, and [`ContactsClient`],
//! the contacts app's way to the core. The answers carry the types of `torro_contacts` as they
//! are; every contact, book and group says `untrusted: true`.

use std::path::PathBuf;

use base64::Engine;
use torro_contacts::{
    AddressBook, Birthday, Contact, ContactFields, ContactPage, ContactQuery, ContactsError, ContactsSource, Group,
    Keep, Moved, PhotoSize,
};

use crate::{Request, Response};

/// The most bytes of a picture `contacts.set_photo` takes, before Base64: 700 KB. The core takes no
/// request over 1 MB, and Base64 makes a third more of it; an app scales larger pictures down
/// before it sends them (decision 3 in the plan of issue #351).
pub const MAX_PHOTO_BYTES: usize = 700 * 1024;

/// How many days `contacts.upcoming_birthdays` looks ahead when it is not told.
pub const BIRTHDAY_DAYS: u32 = 14;

/// The most days `contacts.upcoming_birthdays` looks ahead.
pub const MAX_BIRTHDAY_DAYS: u32 = 366;

/// Talks to the core at its socket: [`ContactsSource`] for the contacts app, one request per call.
#[derive(Debug, Clone)]
pub struct ContactsClient {
    pub socket: PathBuf,
}

impl Default for ContactsClient {
    /// The core of this user session ([`crate::socket_path`]).
    fn default() -> ContactsClient {
        ContactsClient { socket: crate::socket_path().unwrap_or_else(|| PathBuf::from("core.sock")) }
    }
}

/// An answer of another kind than asked; named by its kind only, never with its contents (contacts
/// end up in logs otherwise).
fn unexpected(response: Response) -> ContactsError {
    let kind = serde_json::to_value(&response)
        .ok()
        .and_then(|value| value.get("result").and_then(|r| r.as_str()).map(str::to_string))
        .unwrap_or_default();
    ContactsError::Other(format!("unexpected answer: {kind}"))
}

impl ContactsClient {
    /// Sends `request`; an error of the core comes back as the [`ContactsError`] of its code.
    async fn ask(&self, request: Request) -> Result<Response, ContactsError> {
        match crate::client::call(&self.socket, &request).await {
            Ok(Response::Error { code, message }) => Err(match code.as_str() {
                "auth" => ContactsError::Auth,
                "offline" => ContactsError::Offline,
                "conflict" => ContactsError::Conflict { server_vcard: None, etag: None },
                "not-found" => ContactsError::NotFound,
                "unsupported" => ContactsError::Unsupported(message),
                _ => ContactsError::Other(message),
            }),
            Ok(response) => Ok(response),
            Err(err) => Err(ContactsError::Other(err.to_string())),
        }
    }

    async fn contact(&self, request: Request) -> Result<Contact, ContactsError> {
        match self.ask(request).await? {
            Response::Contact { contact } => Ok(*contact),
            other => Err(unexpected(other)),
        }
    }

    async fn ok(&self, request: Request) -> Result<(), ContactsError> {
        match self.ask(request).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }
}

impl ContactsSource for ContactsClient {
    async fn address_books(&self, account: Option<String>) -> Result<Vec<AddressBook>, ContactsError> {
        match self.ask(Request::ListAddressBooks { account }).await? {
            Response::AddressBooks { books } => Ok(books),
            other => Err(unexpected(other)),
        }
    }

    async fn list_contacts(&self, query: ContactQuery) -> Result<ContactPage, ContactsError> {
        let ContactQuery { book, group, favorites, recent, query, limit, cursor } = query;
        match self.ask(Request::ListContacts { book, group, favorites, recent, query, limit, cursor }).await? {
            Response::Contacts(page) => Ok(page),
            other => Err(unexpected(other)),
        }
    }

    async fn get_contact(&self, id: String) -> Result<Contact, ContactsError> {
        self.contact(Request::GetContact { id }).await
    }

    async fn get_photo(&self, id: String, size: PhotoSize) -> Result<Vec<u8>, ContactsError> {
        match self.ask(Request::GetContactPhoto { id, size }).await? {
            Response::Image { png } => base64::engine::general_purpose::STANDARD
                .decode(png)
                .map_err(|_| ContactsError::Other("the picture is not Base64".into())),
            other => Err(unexpected(other)),
        }
    }

    async fn list_groups(&self, book: Option<String>) -> Result<Vec<Group>, ContactsError> {
        match self.ask(Request::ListContactGroups { book }).await? {
            Response::ContactGroups { groups } => Ok(groups),
            other => Err(unexpected(other)),
        }
    }

    async fn upcoming_birthdays(&self, days: Option<u32>) -> Result<Vec<Birthday>, ContactsError> {
        match self.ask(Request::UpcomingBirthdays { days }).await? {
            Response::Birthdays { birthdays } => Ok(birthdays),
            other => Err(unexpected(other)),
        }
    }

    async fn create_contact(&self, book: String, fields: ContactFields) -> Result<Contact, ContactsError> {
        self.contact(Request::CreateContact { book, fields }).await
    }

    async fn add_to_contact(&self, id: String, fields: ContactFields) -> Result<Contact, ContactsError> {
        self.contact(Request::AddToContact { id, fields }).await
    }

    async fn update_contact(&self, id: String, etag: String, fields: ContactFields) -> Result<Contact, ContactsError> {
        self.contact(Request::UpdateContact { id, etag, fields }).await
    }

    async fn set_favorite(&self, id: String, favorite: bool) -> Result<(), ContactsError> {
        self.ok(Request::SetContactFavorite { id, favorite }).await
    }

    async fn add_to_group(&self, id: String, etag: String, group: String) -> Result<Contact, ContactsError> {
        self.contact(Request::AddContactToGroup { id, etag, group }).await
    }

    async fn remove_from_group(&self, id: String, etag: String, group: String) -> Result<Contact, ContactsError> {
        self.contact(Request::RemoveContactFromGroup { id, etag, group }).await
    }

    async fn mark_used(&self, id: String) -> Result<(), ContactsError> {
        self.ok(Request::MarkContactUsed { id }).await
    }

    async fn set_photo(&self, id: String, etag: String, data: Option<Vec<u8>>) -> Result<Contact, ContactsError> {
        let data = data.map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes));
        self.contact(Request::SetContactPhoto { id, etag, data }).await
    }

    async fn move_contact(&self, id: String, etag: String, to_book: String) -> Result<Moved, ContactsError> {
        match self.ask(Request::MoveContact { id, etag, to_book }).await? {
            Response::ContactMoved(moved) => Ok(*moved),
            other => Err(unexpected(other)),
        }
    }

    async fn delete_contact(&self, id: String, etag: String) -> Result<(), ContactsError> {
        self.ok(Request::DeleteContact { id, etag }).await
    }

    async fn resolve_conflict(&self, id: String, keep: Keep, etag: String) -> Result<Vec<Contact>, ContactsError> {
        match self.ask(Request::ResolveContactConflict { id, keep, etag }).await? {
            Response::ContactsResolved { contacts } => Ok(contacts),
            other => Err(unexpected(other)),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use torro_contacts::demo::{CONTACTS, DemoSource, TEAM};

    use super::*;
    use crate::{client, server};

    /// A test core that answers the contacts commands from the demo data, as the core would.
    async fn answer(demo: &DemoSource, request: Request) -> Response {
        let error = |err: ContactsError| {
            let code = match err {
                ContactsError::NotFound => "not-found",
                ContactsError::ReadOnly | ContactsError::Unsupported(_) => "unsupported",
                ContactsError::Conflict { .. } => "conflict",
                ContactsError::Offline => "offline",
                ContactsError::Auth => "auth",
                _ => "error",
            };
            Response::Error { code: code.into(), message: err.to_string() }
        };
        let contact = |c: Result<Contact, ContactsError>| c.map(|c| Response::Contact { contact: Box::new(c) });
        let answer = match request {
            Request::ListAddressBooks { account } => {
                demo.address_books(account).await.map(|books| Response::AddressBooks { books })
            }
            Request::ListContacts { book, group, favorites, recent, query, limit, cursor } => demo
                .list_contacts(ContactQuery { book, group, favorites, recent, query, limit, cursor })
                .await
                .map(Response::Contacts),
            Request::GetContact { id } => contact(demo.get_contact(id).await),
            Request::GetContactPhoto { id, size } => demo
                .get_photo(id, size)
                .await
                .map(|png| Response::Image { png: base64::engine::general_purpose::STANDARD.encode(png) }),
            Request::ListContactGroups { book } => {
                demo.list_groups(book).await.map(|groups| Response::ContactGroups { groups })
            }
            Request::UpcomingBirthdays { days } => {
                demo.upcoming_birthdays(days).await.map(|birthdays| Response::Birthdays { birthdays })
            }
            Request::CreateContact { book, fields } => contact(demo.create_contact(book, fields).await),
            Request::AddToContact { id, fields } => contact(demo.add_to_contact(id, fields).await),
            Request::UpdateContact { id, etag, fields } => contact(demo.update_contact(id, etag, fields).await),
            Request::SetContactFavorite { id, favorite } => {
                demo.set_favorite(id, favorite).await.map(|()| Response::Ok)
            }
            Request::AddContactToGroup { id, etag, group } => contact(demo.add_to_group(id, etag, group).await),
            Request::RemoveContactFromGroup { id, etag, group } => {
                contact(demo.remove_from_group(id, etag, group).await)
            }
            Request::MarkContactUsed { id } => demo.mark_used(id).await.map(|()| Response::Ok),
            Request::SetContactPhoto { id, etag, data } => {
                let data = data.map(|d| base64::engine::general_purpose::STANDARD.decode(d).unwrap_or_default());
                contact(demo.set_photo(id, etag, data).await)
            }
            Request::MoveContact { id, etag, to_book } => {
                demo.move_contact(id, etag, to_book).await.map(|moved| Response::ContactMoved(Box::new(moved)))
            }
            Request::DeleteContact { id, etag } => demo.delete_contact(id, etag).await.map(|()| Response::Ok),
            Request::ResolveContactConflict { id, keep, etag } => {
                demo.resolve_conflict(id, keep, etag).await.map(|contacts| Response::ContactsResolved { contacts })
            }
            _ => Ok(Response::Pong { version: "test".into(), api: 0 }),
        };
        answer.unwrap_or_else(error)
    }

    #[tokio::test]
    async fn the_client_round_trips_through_a_test_core() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let socket = dir.path().join("core.sock");
        let demo = Arc::new(DemoSource::default());
        let server = tokio::spawn({
            let (socket, demo) = (socket.clone(), Arc::clone(&demo));
            async move {
                server::serve(&socket, move |request| {
                    let demo = Arc::clone(&demo);
                    async move { answer(&demo, request).await }
                })
                .await
            }
        });
        for _ in 0..200 {
            if client::call(&socket, &Request::Ping).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let client = ContactsClient { socket };
        assert_eq!(client.address_books(None).await.expect("books").len(), 2);
        let page =
            client.list_contacts(ContactQuery { limit: Some(5), ..ContactQuery::default() }).await.expect("page");
        assert_eq!(page.contacts.len(), 5);
        assert!(page.contacts.iter().all(|c| c.untrusted));
        let first = client.get_contact(page.contacts[0].id.clone()).await.expect("contact");
        let created = client
            .create_contact(
                CONTACTS.into(),
                ContactFields { display_name: Some("Max Neu".into()), ..ContactFields::default() },
            )
            .await
            .expect("created");
        let changed = client
            .update_contact(
                created.id.clone(),
                created.etag.clone().expect("etag"),
                ContactFields { org: Some("Acme".into()), ..ContactFields::default() },
            )
            .await
            .expect("changed");
        assert_eq!(changed.org.as_deref(), Some("Acme"));
        assert_eq!(
            client.create_contact(TEAM.into(), ContactFields::default()).await,
            Err(ContactsError::Unsupported("This address book is read-only.".into()))
        );
        assert_eq!(client.get_contact("nowhere/x".into()).await, Err(ContactsError::NotFound));
        let with_photo = page.contacts.iter().find(|c| c.photo.is_some()).map(|c| c.id.clone());
        if let Some(id) = with_photo {
            assert_eq!(&client.get_photo(id, PhotoSize::Small).await.expect("png")[1..4], b"PNG");
        }
        client.set_favorite(first.id.clone(), true).await.expect("favourite");
        client.mark_used(first.id.clone()).await.expect("used");
        assert!(!client.list_groups(None).await.expect("groups").is_empty());
        client.upcoming_birthdays(Some(14)).await.expect("birthdays");
        client.delete_contact(changed.id.clone(), changed.etag.clone().expect("etag")).await.expect("deleted");
        server.abort();
    }
}
