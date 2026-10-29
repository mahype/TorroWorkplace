//! The contract a contacts server fulfils for the core (CardDAV first: `torro-pim-caldav`).
//!
//! Asynchronous like [`crate::ContactsSource`]: the core runs it on its runtime. A provider knows
//! nothing of accounts or ids of the suite; it speaks in the server's own addresses (`href`) of
//! books and cards, and the core keeps the mapping.

use std::fmt;
use std::future::Future;

/// Why a call failed, in terms the core answers with.
///
/// `Debug` and `Display` never show a vCard or text of the server: errors end up in logs.
#[derive(Clone, PartialEq, Eq)]
pub enum ContactsError {
    /// Credentials missing or refused.
    Auth,
    /// Server not reachable; changes wait.
    Offline,
    /// The card changed on the server since it was read (ETag mismatch). Carries the server's
    /// version when it could be fetched, for a three-way merge.
    Conflict { server_vcard: Option<String>, etag: Option<String> },
    /// The address book may not be written.
    ReadOnly,
    /// The server refuses the size of the card (a picture too big).
    TooLarge,
    /// No such book or card (or the account has no address books).
    NotFound,
    /// The operation is not possible here; the text says why, in English, for people.
    Unsupported(String),
    /// Anything else; the text is ours, never the server's.
    Other(String),
}

impl fmt::Debug for ContactsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContactsError::Auth => f.write_str("Auth"),
            ContactsError::Offline => f.write_str("Offline"),
            ContactsError::Conflict { etag, server_vcard } => f
                .debug_struct("Conflict")
                .field("server_vcard", &server_vcard.as_ref().map(|_| ".."))
                .field("etag", etag)
                .finish(),
            ContactsError::ReadOnly => f.write_str("ReadOnly"),
            ContactsError::TooLarge => f.write_str("TooLarge"),
            ContactsError::NotFound => f.write_str("NotFound"),
            ContactsError::Unsupported(text) => f.debug_tuple("Unsupported").field(text).finish(),
            ContactsError::Other(text) => f.debug_tuple("Other").field(text).finish(),
        }
    }
}

impl fmt::Display for ContactsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContactsError::Auth => f.write_str("sign-in refused"),
            ContactsError::Offline => f.write_str("server not reachable"),
            ContactsError::Conflict { .. } => f.write_str("the contact changed on the server"),
            ContactsError::ReadOnly => f.write_str("This address book is read-only."),
            ContactsError::TooLarge => f.write_str("the server refuses a contact this large"),
            ContactsError::NotFound => f.write_str("not found"),
            ContactsError::Unsupported(text) | ContactsError::Other(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for ContactsError {}

/// An address book as the server lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBook {
    /// The book's address on the server (CardDAV: the collection URL).
    pub href: String,
    /// The last part of the path, the book's name in its id (`<account>/<name>`).
    pub name: String,
    pub display_name: String,
    pub writable: bool,
    /// Nextcloud's system address book and "Recently contacted".
    pub hidden_by_default: bool,
}

/// Opaque position in a book's change history (CardDAV: the token of `sync-collection`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncToken(pub String);

/// A card's address and version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub href: String,
    pub etag: String,
}

/// What changed in a book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing {
    /// Only what changed since the token: new or changed cards, and removed ones.
    Delta { changed: Vec<Entry>, removed: Vec<String> },
    /// Every card in the book (no token, an expired one, or a server without `sync-collection`):
    /// whatever the core holds and this lacks is gone.
    Full(Vec<Entry>),
}

/// The answer of [`ContactsProvider::changes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    pub listing: Listing,
    /// The token to ask with next time; `None` when the server has none.
    pub token: Option<SyncToken>,
}

/// One card fetched from the server.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteCard {
    pub href: String,
    pub etag: Option<String>,
    pub vcard: String,
}

impl fmt::Debug for RemoteCard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteCard").field("href", &self.href).field("etag", &self.etag).finish_non_exhaustive()
    }
}

/// Where a written card is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub href: String,
    /// The new version; `None` when the server did not say (fetch it on the next sync).
    pub etag: Option<String>,
}

/// A contacts server.
pub trait ContactsProvider: Send + Sync {
    /// The account's address books. [`ContactsError::NotFound`] when it has none at all (no
    /// address book home): that is "no contacts", not an error to show.
    fn address_books(&self) -> impl Future<Output = Result<Vec<RemoteBook>, ContactsError>> + Send;

    /// What changed in the book at `book` since `since` (`None`: list everything).
    fn changes(
        &self,
        book: &str,
        since: Option<&SyncToken>,
    ) -> impl Future<Output = Result<Changes, ContactsError>> + Send;

    /// The cards at `hrefs` in the book at `book`. Cards gone meanwhile are left out.
    fn fetch(
        &self,
        book: &str,
        hrefs: &[String],
    ) -> impl Future<Output = Result<Vec<RemoteCard>, ContactsError>> + Send;

    /// Writes a card. `href: None` creates it as `<book>/<uid>.vcf` and fails with
    /// [`ContactsError::Conflict`] when one is there already; otherwise `etag` must match the
    /// server's version.
    fn put(
        &self,
        book: &str,
        href: Option<&str>,
        uid: &str,
        vcard: &str,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<Stored, ContactsError>> + Send;

    /// Deletes a card; one gone already counts as deleted.
    fn delete(
        &self,
        book: &str,
        href: &str,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<(), ContactsError>> + Send;
}
