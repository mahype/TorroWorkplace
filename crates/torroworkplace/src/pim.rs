//! Calendars and tasks in the core: the account, the CalDAV connection, an in-memory cache
//! and the sync loop. The apps reach it through the socket (`torro_ipc::pim::PimClient`).
//!
//! Tasks K.5 (cache, sync, reading and writing) and K.6 (account and Login Flow v2) in
//! `docs/tasks/kalender-aufgaben.md`.
//!
//! Design:
//! - One Nextcloud account (`torro_core::account`, secret in the keychain).
//! - The cache holds all collections, all todos and the events from 3 months back to 12
//!   months ahead, fetched on start, every 5 minutes, after a `Sync` request and after
//!   sign-in. Reads are answered from the cache, so the apps never wait for the network;
//!   ranges outside the window are fetched from the server directly.
//! - Writes go to the server first (ETag-guarded) and update the cache with the answer.
//! - An SQLite cache that survives restarts comes later.

use torro_ipc::{Request, Response};
use torro_pim::AccountState;

/// See the module docs.
#[derive(Debug, Default)]
pub struct PimService {}

impl PimService {
    /// Loads the stored account, if any. Does not touch the network.
    pub fn new() -> PimService {
        PimService::default()
    }

    /// Answers the calendar, task and account requests; other requests are not ours.
    pub async fn handle(&self, request: Request) -> Response {
        match request {
            Request::Account => Response::Account { account: AccountState::NotConnected },
            _ => Response::Error {
                code: "error".into(),
                message: "calendars and tasks are not implemented in the core yet (tasks K.5, K.6)".into(),
            },
        }
    }
}
