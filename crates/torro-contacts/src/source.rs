//! Where the contacts app gets its contacts from.
//!
//! The app only knows [`ContactsSource`]. In production that is the core over the socket
//! (`torro_ipc::contacts::ContactsClient`); for building and trying the app it is
//! [`crate::demo::DemoSource`]. Each method is one command of the API area `contacts`.

use std::future::Future;

use crate::model::{
    AddressBook, Birthday, Contact, ContactFields, ContactPage, ContactQuery, Group, Keep, Moved, PhotoSize,
};
use crate::provider::ContactsError;

pub trait ContactsSource: Send + Sync {
    /// `contacts.list_address_books`: the books the caller reaches, of one account or all.
    fn address_books(
        &self,
        account: Option<String>,
    ) -> impl Future<Output = Result<Vec<AddressBook>, ContactsError>> + Send;

    /// `contacts.list_contacts`.
    fn list_contacts(&self, query: ContactQuery) -> impl Future<Output = Result<ContactPage, ContactsError>> + Send;

    /// `contacts.get_contact`: every field and the `etag` to change it with.
    fn get_contact(&self, id: String) -> impl Future<Output = Result<Contact, ContactsError>> + Send;

    /// `contacts.get_photo`: the picture as PNG.
    fn get_photo(&self, id: String, size: PhotoSize) -> impl Future<Output = Result<Vec<u8>, ContactsError>> + Send;

    /// `contacts.list_groups`.
    fn list_groups(&self, book: Option<String>) -> impl Future<Output = Result<Vec<Group>, ContactsError>> + Send;

    /// `contacts.upcoming_birthdays`: within `days` days (default 14), the soonest first.
    fn upcoming_birthdays(
        &self,
        days: Option<u32>,
    ) -> impl Future<Output = Result<Vec<Birthday>, ContactsError>> + Send;

    /// `contacts.create_contact`.
    fn create_contact(
        &self,
        book: String,
        fields: ContactFields,
    ) -> impl Future<Output = Result<Contact, ContactsError>> + Send;

    /// `contacts.add_to_contact`: adds without changing what is there.
    fn add_to_contact(
        &self,
        id: String,
        fields: ContactFields,
    ) -> impl Future<Output = Result<Contact, ContactsError>> + Send;

    /// `contacts.update_contact`: replaces the fields sent.
    fn update_contact(
        &self,
        id: String,
        etag: String,
        fields: ContactFields,
    ) -> impl Future<Output = Result<Contact, ContactsError>> + Send;

    /// `contacts.set_favorite`.
    fn set_favorite(&self, id: String, favorite: bool) -> impl Future<Output = Result<(), ContactsError>> + Send;

    /// `contacts.add_to_group`.
    fn add_to_group(
        &self,
        id: String,
        etag: String,
        group: String,
    ) -> impl Future<Output = Result<Contact, ContactsError>> + Send;

    /// `contacts.remove_from_group`.
    fn remove_from_group(
        &self,
        id: String,
        etag: String,
        group: String,
    ) -> impl Future<Output = Result<Contact, ContactsError>> + Send;

    /// `contacts.mark_used`.
    fn mark_used(&self, id: String) -> impl Future<Output = Result<(), ContactsError>> + Send;

    /// `contacts.set_photo`: PNG or JPEG, `None` removes the picture.
    fn set_photo(
        &self,
        id: String,
        etag: String,
        data: Option<Vec<u8>>,
    ) -> impl Future<Output = Result<Contact, ContactsError>> + Send;

    /// `contacts.move_contact`.
    fn move_contact(
        &self,
        id: String,
        etag: String,
        to_book: String,
    ) -> impl Future<Output = Result<Moved, ContactsError>> + Send;

    /// `contacts.delete_contact`.
    fn delete_contact(&self, id: String, etag: String) -> impl Future<Output = Result<(), ContactsError>> + Send;

    /// `contacts.resolve_conflict`: the contacts that stay (two with [`Keep::Both`]).
    fn resolve_conflict(
        &self,
        id: String,
        keep: Keep,
        etag: String,
    ) -> impl Future<Output = Result<Vec<Contact>, ContactsError>> + Send;
}
