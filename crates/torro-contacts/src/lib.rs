//! Contacts, independent of where they come from.
//!
//! The model ([`Contact`], [`AddressBook`], [`Group`]) is shared by the core, the contacts app and
//! every provider, and it is the wire format of the API area `contacts`. [`vcard`] reads vCards
//! and changes them without losing fields it does not know; [`merge`] brings two changes of the
//! same contact together. The core talks to servers through [`ContactsProvider`]
//! (`torro-pim-caldav` speaks CardDAV), the app reads and writes through [`ContactsSource`]: the
//! core over the socket, or [`demo::DemoSource`] for trying the app. [`suggest`] ranks the recipients
//! the compose window offers (issue #460), [`duplicates`] finds people kept twice (issue #726). No
//! input or output here.

pub mod demo;
pub mod duplicates;
pub mod merge;
pub mod model;
pub mod provider;
pub mod source;
pub mod suggest;
pub mod vcard;

pub use merge::{Merge, merge};
pub use model::{
    AddressBook, Birthday, Contact, ContactFields, ContactPage, ContactQuery, ContactState, ContactSummary,
    DuplicateGroup, DuplicateReason, FieldKind, FieldsError, FromItem, Geo, Group, Imported, Keep, Labeled, Merged,
    Moved, Name, PartialDate, PhotoRef, PhotoSize, PostalAddress, RecipientSuggestion, SuggestedAddress,
    SuggestionKind, account_of, book_id, book_of, contact_id,
};
pub use provider::{
    Changes, ContactsError, ContactsProvider, Entry, Listing, RemoteBook, RemoteCard, Stored, SyncToken,
};
pub use source::ContactsSource;
