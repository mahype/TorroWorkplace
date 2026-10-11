//! The contract every provider fulfils.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Collection, Event, Todo};

/// Opaque position in a collection's change history (CalDAV: the sync-token of
/// `sync-collection`, others: a cursor or timestamp).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncToken(pub String);

/// One change a provider reports since a sync token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Event { item: Event, etag: String },
    Todo { item: Todo, etag: String },
    Removed { uid: String },
}

/// Why a provider call failed, in the terms the UI shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderError {
    /// Credentials missing or refused.
    Auth,
    /// Server not reachable; the cache keeps working.
    Offline,
    /// The item changed on the server since it was read (ETag mismatch).
    Conflict {
        uid: String,
    },
    Other(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::Auth => f.write_str("sign-in refused"),
            ProviderError::Offline => f.write_str("server not reachable"),
            ProviderError::Conflict { uid } => write!(f, "{uid} changed on the server"),
            ProviderError::Other(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ProviderError {}

/// A source of calendars and tasks.
///
/// Synchronous on purpose for now; the sync engine runs it off the UI thread. Whether it
/// becomes async is decided with the first real implementation.
pub trait Provider {
    /// Short, stable provider name, e.g. `caldav`.
    fn name(&self) -> &'static str;

    /// The collections on the account.
    fn collections(&self) -> Result<Vec<Collection>, ProviderError>;

    /// Changes in `collection` since `since` (`None`: everything), and the new token.
    fn changes(&self, collection: &str, since: Option<&SyncToken>) -> Result<(Vec<Change>, SyncToken), ProviderError>;

    /// Creates or replaces an event. `etag` guards against overwriting a newer version.
    fn put_event(&self, event: &Event, etag: Option<&str>) -> Result<String, ProviderError>;

    /// Creates or replaces a task. `etag` guards against overwriting a newer version.
    fn put_todo(&self, todo: &Todo, etag: Option<&str>) -> Result<String, ProviderError>;

    /// Deletes an item.
    fn delete(&self, collection: &str, uid: &str, etag: Option<&str>) -> Result<(), ProviderError>;
}
