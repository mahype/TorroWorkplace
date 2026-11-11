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
//!   ([`ChatService::forget_account`], told by `PimService::on_forget`).
//!
//! Logs name an account's id and a code, never a name, a text or a token.

pub mod cache;
#[cfg(test)]
pub mod fake;
pub mod live;
pub mod remote;
pub mod service;
pub mod talk;
pub mod throttle;

use std::sync::{Arc, Mutex, MutexGuard};

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

/// See the module docs.
#[derive(Debug)]
pub struct ChatService {
    remotes: Remotes,
    cache: Mutex<Cache>,
    /// Shared with the presence service: one budget for everything a client sends in both areas.
    throttle: Arc<Throttle>,
    /// One fetch at a time for client keys, so no agent keeps the server busy over many
    /// conversations (decision 9).
    client_fetch: tokio::sync::Mutex<()>,
}

impl ChatService {
    /// The service of the core: the Nextcloud accounts of `pim`.
    pub fn system(pim: Arc<PimService>) -> ChatService {
        ChatService::with(Remotes::Pim(pim))
    }

    /// A service over servers in memory, by account id.
    #[cfg(test)]
    pub fn fixed(servers: Vec<(String, Arc<fake::Server>)>) -> ChatService {
        ChatService::with(Remotes::Fixed(servers))
    }

    fn with(remotes: Remotes) -> ChatService {
        ChatService {
            remotes,
            cache: Mutex::new(Cache::default()),
            throttle: Arc::new(Throttle::default()),
            client_fetch: tokio::sync::Mutex::new(()),
        }
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

    /// Drops everything held of `account` (account removed or wiped); live requests for it end at
    /// their next turn.
    pub fn forget_account(&self, account: &str) {
        self.cache().forget_account(account);
    }

    /// Drops everything of accounts that are no longer there (a missed message of
    /// `PimService::on_forget`).
    pub async fn forget_gone(&self) {
        let known = self.accounts().await;
        self.cache().keep_accounts(&known);
    }
}
