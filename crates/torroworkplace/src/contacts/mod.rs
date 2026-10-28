//! Contacts in the core (`docs/kontakte.md`, issue #351): the address books of every Nextcloud and
//! CalDAV account, held on this device ([`store`]), brought in step with the servers ([`sync`]),
//! with previews of their pictures ([`photo`]).
//!
//! - The accounts and their CardDAV connections come from the calendar service
//!   (`PimService::dav_clients`): the same sign-in, the same guard that the password reaches only
//!   its own server. Mail accounts' address books come later.
//! - A round runs at the start, every [`SYNC_EVERY`], with `accounts.sync` and after every write.
//!   One round at a time.
//! - A server that refuses the password starts the remote wipe check of the calendar service
//!   (`PimService::on_refused`). An account removed or wiped takes its contacts, previews and
//!   queue with it ([`ContactsService::forget_account`], told by `PimService::on_forget`).
//! - Books a server no longer lists go with their contacts; [`ContactsService::sync_all`] names
//!   them, and the core takes them out of every client's rights.

pub mod photo;
pub mod store;
pub mod sync;

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use torro_contacts::{Changes, ContactsError, ContactsProvider, RemoteBook, RemoteCard, Stored, SyncToken};
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_pim_caldav::CalDavClient;

use crate::pim::PimService;
use photo::Previews;
use store::Store;

/// How often the contacts are fetched without being asked.
pub const SYNC_EVERY: Duration = Duration::from_secs(15 * 60);

/// One account's contacts server.
#[derive(Debug, Clone)]
pub enum Remote {
    Dav(CalDavClient),
    #[cfg(test)]
    Fake(Arc<sync::fake::Server>),
}

impl ContactsProvider for Remote {
    async fn address_books(&self) -> Result<Vec<RemoteBook>, ContactsError> {
        match self {
            Remote::Dav(client) => client.address_books().await,
            #[cfg(test)]
            Remote::Fake(server) => server.address_books().await,
        }
    }

    async fn changes(&self, book: &str, since: Option<&SyncToken>) -> Result<Changes, ContactsError> {
        match self {
            Remote::Dav(client) => client.changes(book, since).await,
            #[cfg(test)]
            Remote::Fake(server) => server.changes(book, since).await,
        }
    }

    async fn fetch(&self, book: &str, hrefs: &[String]) -> Result<Vec<RemoteCard>, ContactsError> {
        match self {
            Remote::Dav(client) => client.fetch(book, hrefs).await,
            #[cfg(test)]
            Remote::Fake(server) => server.fetch(book, hrefs).await,
        }
    }

    async fn put(
        &self,
        book: &str,
        href: Option<&str>,
        uid: &str,
        vcard: &str,
        etag: Option<&str>,
    ) -> Result<Stored, ContactsError> {
        match self {
            Remote::Dav(client) => client.put(book, href, uid, vcard, etag).await,
            #[cfg(test)]
            Remote::Fake(server) => server.put(book, href, uid, vcard, etag).await,
        }
    }

    async fn delete(&self, book: &str, href: &str, etag: Option<&str>) -> Result<(), ContactsError> {
        match self {
            Remote::Dav(client) => client.delete(book, href, etag).await,
            #[cfg(test)]
            Remote::Fake(server) => server.delete(book, href, etag).await,
        }
    }
}

/// Where the connections come from.
#[derive(Debug)]
enum Remotes {
    /// The calendar service's (in the core).
    Pim(Arc<PimService>),
    /// Servers in memory, by account id (tests).
    #[cfg(test)]
    Fixed(Vec<(String, Arc<sync::fake::Server>)>),
}

/// See the module docs.
#[derive(Debug)]
pub struct ContactsService {
    /// `None` when the store could not be opened (logged): every command then answers `error`.
    store: Option<Mutex<Store>>,
    previews: Previews,
    remotes: Remotes,
    /// One round at a time.
    rounds: tokio::sync::Mutex<()>,
}

impl ContactsService {
    /// The service on the user's data folder (`<data>/contacts/`), with the connections of `pim`.
    /// Tests never touch the user's contacts: there it holds none ([`ContactsService::in_dir`]).
    pub fn system(pim: Arc<PimService>) -> ContactsService {
        let dir = if cfg!(test) { None } else { AppPaths::for_app(AppId::WORKPLACE).map(|p| p.data.join("contacts")) };
        ContactsService::open(dir, Remotes::Pim(pim))
    }

    fn open(dir: Option<PathBuf>, remotes: Remotes) -> ContactsService {
        let store = dir.as_deref().and_then(|dir| match Store::open(dir) {
            Ok(store) => Some(Mutex::new(store)),
            Err(err) => {
                eprintln!("torroworkplace: contacts: {err}");
                None
            }
        });
        let previews = Previews::new(dir.map(|dir| dir.join("photos")));
        ContactsService { store, previews, remotes, rounds: tokio::sync::Mutex::new(()) }
    }

    /// The service on the store in `dir` with the servers `servers` (tests).
    #[cfg(test)]
    pub fn in_dir(dir: &Path, servers: Vec<(String, Arc<sync::fake::Server>)>) -> ContactsService {
        ContactsService::open(Some(dir.to_path_buf()), Remotes::Fixed(servers))
    }

    /// The store, or `error` when there is none.
    pub(crate) fn store(&self) -> Result<&Mutex<Store>, ContactsError> {
        self.store.as_ref().ok_or_else(|| ContactsError::Other("The contacts on this device cannot be opened.".into()))
    }

    pub(crate) fn previews(&self) -> &Previews {
        &self.previews
    }

    /// The accounts with a contacts server.
    async fn remotes(&self) -> Vec<(String, Remote)> {
        match &self.remotes {
            Remotes::Pim(pim) => pim.dav_clients().await.into_iter().map(|(id, c)| (id, Remote::Dav(c))).collect(),
            #[cfg(test)]
            Remotes::Fixed(servers) => {
                servers.iter().map(|(id, server)| (id.clone(), Remote::Fake(Arc::clone(server)))).collect()
            }
        }
    }

    /// The contacts server of `account`.
    pub(crate) async fn remote(&self, account: &str) -> Option<Remote> {
        self.remotes().await.into_iter().find(|(id, _)| id == account).map(|(_, remote)| remote)
    }

    /// The ids of the accounts with a contacts server.
    pub(crate) async fn accounts(&self) -> Vec<String> {
        self.remotes().await.into_iter().map(|(id, _)| id).collect()
    }

    /// A refused password: the calendar service checks for a remote wipe.
    async fn refused(&self, account: &str) {
        match &self.remotes {
            Remotes::Pim(pim) => pim.on_refused(account).await,
            #[cfg(test)]
            Remotes::Fixed(_) => {}
        }
    }

    /// One round for every account; returns the books that are gone. One account failing does not
    /// keep the others; its error is logged (never with a name or a field).
    pub async fn sync_all(&self) -> Vec<String> {
        let mut removed = Vec::new();
        if self.store.is_none() {
            return removed;
        }
        for (account, _) in self.remotes().await {
            match self.sync_account(&account).await {
                Ok(synced) => removed.extend(synced.removed_books),
                Err(ContactsError::Offline) => {}
                Err(err) => eprintln!("torroworkplace: contacts of {account} did not sync: {err}"),
            }
        }
        removed
    }

    /// One round for `account`.
    pub async fn sync_account(&self, account: &str) -> Result<sync::Synced, ContactsError> {
        let store = self.store()?;
        let Some(remote) = self.remote(account).await else { return Ok(sync::Synced::default()) };
        let _round = self.rounds.lock().await;
        let result = sync::sync_account(store, account, &remote).await;
        if result == Err(ContactsError::Auth) {
            self.refused(account).await;
        }
        result
    }

    /// Everything of `account` on this device: contacts, queue, favourites, recents, previews.
    pub async fn forget_account(&self, account: &str) {
        let _round = self.rounds.lock().await;
        if let Some(store) = &self.store
            && let Err(err) = sync::with(store, |s| s.forget_account(account))
        {
            eprintln!("torroworkplace: contacts of a removed account could not be removed: {err}");
        }
        self.previews.forget_account(account);
    }

    /// Whether `account` has address books on this device.
    pub fn has_books(&self, account: &str) -> bool {
        self.store
            .as_ref()
            .is_some_and(|store| sync::with(store, |s| s.books(Some(account))).is_ok_and(|b| !b.is_empty()))
    }
}

/// Where the store of `dir` keeps its file (for tests and the hand check of issue #351).
#[cfg(test)]
pub fn file_in(dir: &Path) -> PathBuf {
    dir.join("contacts.sqlite")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_account_removed_takes_its_contacts_and_previews_along() {
        let dir = tempfile::tempdir().expect("dir");
        let server = Arc::new(sync::fake::Server::default());
        let book = server.add_book("contacts", true, false);
        server.put_card(&book, "a", "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:a\r\nFN:Anna\r\nEND:VCARD\r\n");
        let account = "nextcloud:anna@cloud.example.org";
        let service = ContactsService::in_dir(dir.path(), vec![(account.into(), Arc::clone(&server))]);
        assert!(service.sync_all().await.is_empty());
        assert!(service.has_books(account));
        std::fs::create_dir_all(dir.path().join("photos").join(crate::audit::hashed(account))).expect("previews");
        service.forget_account(account).await;
        assert!(!service.has_books(account));
        assert!(!dir.path().join("photos").join(crate::audit::hashed(account)).exists());
        assert!(file_in(dir.path()).exists(), "the store stays for the other accounts");
    }
}
