//! The contact card (issue #789, `docs/inhaltskarten.md` "Kontakt"): a contact the assistant found, as the address
//! book holds it now.
//!
//! The card goes the way `contacts.get_contact` goes for the conversation's client: the area `contacts` must allow
//! reading, and the contact's book must be one of its books (`ContactsService::out_of_reach`) before the address book
//! is read; what it shows is the summary `contacts.list_contacts` lists.

use torro_ipc::api::{Access, Area};
use torro_ipc::assistant::Card;
use torro_ipc::{Request, Response};

use super::Lookup;

/// What the contact `id` shows now: [`Card::Contact`] with the contact as `contacts.list_contacts` lists it (name,
/// company, the main address and number, its photo's reference for `contacts.get_photo`).
///
/// Looks only in address books within [`Lookup::rights`] (`Rights::address_books`, a hidden one only when named in
/// `Rights::hidden_address_books`), and only with read access to `contacts`; answers [`Card::Unavailable`] for a
/// contact gone or out of reach, without saying which. Logs no name.
pub async fn look(lookup: &Lookup<'_>, id: &str) -> Card {
    if !lookup.rights.area(Area::Contacts).allows(Access::Read) {
        return Card::Unavailable;
    }
    let reach = lookup.reach();
    let request = Request::GetContact { id: id.to_owned() };
    if lookup.contacts.out_of_reach(reach, &request).is_some() {
        return Card::Unavailable;
    }
    match lookup.contacts.handle(request, reach).await {
        Response::Contact { contact } => Card::Contact { contact: Box::new(contact.summary()) },
        _ => Card::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use torro_ipc::api::{AreaRights, Rights, RightsTemplate, Selection};

    use super::*;
    use crate::assistant::cards::mail::tests::{Services, client};
    use crate::contacts::ContactsService;
    use crate::contacts::sync::fake::Server;
    use crate::mail::service::MailService;

    const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
    const NAME: &str = "Bernd Kraus";

    fn vcard(uid: &str, name: &str, email: &str) -> String {
        format!(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid}\r\nFN:{name}\r\nN:{name};;;;\r\nORG:Kraus GmbH\r\n\
             EMAIL;TYPE=WORK:{email}\r\nEND:VCARD\r\n"
        )
    }

    /// Anna's Nextcloud with the book `contacts` holding Bernd and the book `privat` holding a secret person, synced.
    async fn services(dir: &std::path::Path) -> Services {
        let server = Arc::new(Server::default());
        let work = server.add_book("contacts", true, false);
        let private = server.add_book("privat", true, false);
        server.put_card(&work, "bernd", &vcard("bernd", NAME, "bernd@example.com"));
        server.put_card(&private, "secret", &vcard("secret", "Secret Person", "secret@example.org"));
        let contacts = ContactsService::in_dir(&dir.join("contacts"), vec![(ACCOUNT.into(), server)]);
        contacts.sync_all().await;
        Services::with(dir, MailService::without_home(), contacts)
    }

    fn id(book: &str, uid: &str) -> String {
        format!("{ACCOUNT}/{book}/{uid}")
    }

    /// Read access everywhere, the book `contacts` only.
    fn work_book() -> Rights {
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.address_books = Selection::Only(vec![format!("{ACCOUNT}/contacts")]);
        rights
    }

    #[tokio::test]
    async fn a_contact_in_a_book_within_the_rights_shows() {
        let dir = tempfile::tempdir().expect("temp dir");
        let services = services(dir.path()).await;
        let rights = work_book();
        let client = client(&rights);

        let card = look(&services.lookup(&client, &rights), &id("contacts", "bernd")).await;
        let Card::Contact { contact } = card else { panic!("a contact card, got {card:?}") };
        assert_eq!(contact.display_name, NAME);
        assert_eq!(contact.email.as_deref(), Some("bernd@example.com"));
        assert_eq!(contact.id, id("contacts", "bernd"));
    }

    #[tokio::test]
    async fn a_contact_out_of_reach_is_unavailable_without_its_name() {
        let dir = tempfile::tempdir().expect("temp dir");
        let services = services(dir.path()).await;
        let mut no_contacts = work_book();
        no_contacts.areas.insert(Area::Contacts, AreaRights::NONE);
        let within = work_book();
        let no_books = RightsTemplate::FullAccess.rights();
        for (rights, id, why) in [
            (&no_contacts, id("contacts", "bernd"), "no right to read contacts"),
            (&no_books, id("contacts", "bernd"), "no book opened (as no template opens one)"),
            (&within, id("privat", "secret"), "a book outside the rights"),
            (&within, id("contacts", "gone"), "a contact that is not there (any more)"),
        ] {
            let client = client(rights);
            let card = look(&services.lookup(&client, rights), &id).await;
            assert_eq!(card, Card::Unavailable, "{why}");
            let shown = serde_json::to_string(&card).expect("serialised");
            assert!(!shown.contains("Bernd") && !shown.contains("Secret"), "{why}: {shown}");
        }
    }
}
