//! Chat in the core (`docs/chat.md`, issue #643): the conversations and messages of every
//! Nextcloud account with Talk, read and written for the chat windows, the Notification Hub, the
//! API and MCP.
//!
//! - The accounts and their connections come from the calendar service
//!   (`PimService::nextcloud_access`): the same sign-in, the same guard that the app password
//!   reaches only its own server.
//! - Nothing is kept on disk ([`cache`]): the history lives on the server.
//! - Live updates run only for conversations an app is looking at ([`live`], decision 9): one
//!   long-polling request per conversation, at most [`live::MAX_LIVE`] at once.
//! - What other people see waits for approval unless the person allowed sending freely; a client
//!   that may send freely is still throttled ([`throttle`]).
//! - An account removed or wiped takes its conversations and messages with it
//!   ([`ChatService::forget_account`], told by `PimService::on_forget`), and its prepared files,
//!   previews, cards and fetched files too.
//! - Files and media (stage 4): sending in [`media`], reading, previews and link cards in
//!   [`attachments`].
//! - Sharing items of the suite (stage 5): contacts as a card in [`share`].
//! - Calls (stage 6b): ringing, declining and leaving in [`calls`]; the call runs in the browser.
//! - Content from other providers (issue #672): link cards of other servers follow Settings › Privacy,
//!   read afresh for every page ([`ChatService::set_privacy`]).
//! - The chat's windows float under AeroSpace and GlazeWM too ([`float`], issue #667).
//!
//! Logs name an account's id and a code, never a name, a text or a token.

pub mod attachments;
pub mod cache;
pub mod calls;
#[cfg(test)]
pub mod fake;
pub mod float;
pub mod live;
pub mod media;
pub mod remote;
pub mod service;
pub mod share;
pub mod talk;
pub mod throttle;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use torro_chat::ChatError;

use crate::pim::PimService;
use cache::Cache;
use remote::Remote;
use throttle::Throttle;

/// A person of `account`'s server whose status is not known: found by a search, or added to the
/// person's own list.
pub(crate) fn person(account: &str, person: remote::RemotePerson) -> torro_chat::Person {
    torro_chat::Person {
        id: torro_chat::person_id(account, &person.id),
        account: account.to_string(),
        name: person.name,
        status: torro_chat::Status::Offline,
        unknown: true,
        message: None,
        icon: None,
        clear_at: None,
        added: false,
        untrusted: true,
    }
}

/// Where the connections come from.
#[derive(Debug)]
enum Remotes {
    /// The calendar service's Nextcloud accounts (in the core).
    Pim(Arc<PimService>),
    /// Servers in memory, by account id (tests).
    #[cfg(test)]
    Fixed(Vec<(String, Arc<fake::Server>)>),
}

/// The reader the service asks; `ask` with nobody on the list until [`ChatService::set_privacy`].
struct Privacy(RwLock<torro_core::privacy::Reader>);

impl Default for Privacy {
    fn default() -> Privacy {
        Privacy(RwLock::new(Arc::new(torro_core::privacy::Privacy::default)))
    }
}

impl std::fmt::Debug for Privacy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Privacy")
    }
}

/// See the module docs.
#[derive(Debug)]
pub struct ChatService {
    remotes: Remotes,
    cache: Mutex<Cache>,
    /// Shared with the presence service: one budget for everything a client sends in both areas.
    throttle: Arc<Throttle>,
    /// One read of the server at a time per client key, so no agent keeps the server busy over
    /// many conversations, and none holds up another (decision 9): a lock per client id.
    client_fetch: Mutex<std::collections::BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Prepared files, uploads and sendings of files (stage 4).
    media: Mutex<media::Store>,
    /// Previews and link cards (stage 4).
    held: Mutex<attachments::Held>,
    /// The chat's own folder for fetched files (`<cache>/chat-attachments`); `None` without a home.
    attachments: Option<PathBuf>,
    /// Whether link cards of other servers load (issue #672).
    privacy: Privacy,
}

impl ChatService {
    /// The service of the core: the Nextcloud accounts of `pim`.
    pub fn system(pim: Arc<PimService>) -> ChatService {
        let folder = torro_core::paths::AppPaths::for_app(torro_core::AppId::WORKPLACE)
            .map(|paths| paths.state.join("cache").join("chat-attachments"));
        ChatService { attachments: folder, ..ChatService::with(Remotes::Pim(pim)) }
    }

    /// A service over servers in memory, by account id.
    #[cfg(test)]
    pub fn fixed(servers: Vec<(String, Arc<fake::Server>)>) -> ChatService {
        ChatService::with(Remotes::Fixed(servers))
    }

    /// This service with `folder` for fetched files (tests).
    #[cfg(test)]
    pub fn with_attachments(self, folder: PathBuf) -> ChatService {
        ChatService { attachments: Some(folder), ..self }
    }

    fn with(remotes: Remotes) -> ChatService {
        ChatService {
            remotes,
            cache: Mutex::new(Cache::default()),
            throttle: Arc::new(Throttle::default()),
            client_fetch: Mutex::new(std::collections::BTreeMap::new()),
            media: Mutex::new(media::Store::default()),
            held: Mutex::new(attachments::Held::default()),
            attachments: None,
            privacy: Privacy::default(),
        }
    }

    /// Sets what says whether content from other providers loads (`privacy` of the settings, issue
    /// #672); without it every provider is asked about.
    pub fn set_privacy(&self, reader: torro_core::privacy::Reader) {
        *self.privacy.0.write().unwrap_or_else(PoisonError::into_inner) = reader;
    }

    /// What Settings › Privacy says now.
    pub(crate) fn privacy(&self) -> torro_core::privacy::Privacy {
        let reader = Arc::clone(&self.privacy.0.read().unwrap_or_else(PoisonError::into_inner));
        reader()
    }

    /// Waits until `caller` may read the server: at once for the apps, one read at a time for each
    /// client (decision 9). Held while the guard lives.
    pub(crate) async fn one_read(&self, caller: service::Caller<'_>) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        if caller.is_app() {
            return None;
        }
        let lock = {
            let mut locks = self.client_fetch.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            Arc::clone(locks.entry(caller.client.unwrap_or_default().to_string()).or_default())
        };
        Some(lock.lock_owned().await)
    }

    /// The budget of what clients send, for the presence service to share.
    pub fn throttle(&self) -> Arc<Throttle> {
        Arc::clone(&self.throttle)
    }

    /// The cache; a lock a panicking thread left is taken over, the cache cannot break by that.
    fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The ids of the accounts that may have a chat: every Nextcloud account that is signed in.
    pub async fn accounts(&self) -> Vec<String> {
        match &self.remotes {
            Remotes::Pim(pim) => pim.nextcloud_accounts().await,
            #[cfg(test)]
            Remotes::Fixed(servers) => servers.iter().map(|(id, _)| id.clone()).collect(),
        }
    }

    /// The server of `account`; `not-found` for an account that is none of these or signed out.
    pub async fn remote(&self, account: &str) -> Result<Remote, ChatError> {
        match &self.remotes {
            Remotes::Pim(pim) => match pim.nextcloud_access(account).await {
                Some(access) => Ok(Remote::Talk(Box::new(access.client))),
                None => Err(ChatError::NotFound),
            },
            #[cfg(test)]
            Remotes::Fixed(servers) => servers
                .iter()
                .find(|(id, _)| id == account)
                .map(|(_, server)| Remote::Fake(Arc::clone(server)))
                .ok_or(ChatError::NotFound),
        }
    }

    /// Tells the calendar service that `account`'s server refused the app password, so it checks
    /// for a remote wipe as after a refused calendar call.
    async fn refused(&self, account: &str) {
        match &self.remotes {
            Remotes::Pim(pim) => pim.on_refused(account).await,
            #[cfg(test)]
            Remotes::Fixed(_) => {}
        }
    }

    /// The tokens of the conversations of `account` held in memory, for tests.
    #[cfg(test)]
    pub fn cached_rooms(&self, account: &str) -> Vec<String> {
        self.cache().account(account).rooms.iter().map(|room| room.token.clone()).collect()
    }

    /// Drops everything held of `account` (account removed or wiped); live requests for it end at
    /// their next turn.
    pub fn forget_account(&self, account: &str) {
        self.cache().forget_account(account);
        self.media().forget_account(account);
        self.held().forget_account(account);
        if let Some(folder) = &self.attachments {
            // The fetched files of the account go with it; failing leaves them for the daily sweep.
            let _ = std::fs::remove_dir_all(folder.join(crate::audit::hashed(account)));
        }
    }

    /// Drops everything of accounts that are no longer there (a missed message of
    /// `PimService::on_forget`).
    pub async fn forget_gone(&self) {
        let known = self.accounts().await;
        let gone: Vec<String> = self.cache().accounts.keys().filter(|id| !known.contains(id)).cloned().collect();
        for account in gone {
            self.forget_account(&account);
        }
        self.cache().keep_accounts(&known);
    }
}
