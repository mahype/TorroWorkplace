//! Where the apps get calendars and tasks from.
//!
//! The apps only know [`PimSource`]. In production that is the core over the socket
//! (`torro_ipc::pim::PimClient`); for building and trying the UIs it is
//! [`crate::demo::DemoSource`] with sample data in memory.

use std::future::Future;

use chrono::{DateTime, Utc};

use crate::{AccountState, Collection, Event, ProviderError, Todo};

pub trait PimSource: Send + Sync {
    /// The account behind the data.
    fn account(&self) -> impl Future<Output = Result<AccountState, ProviderError>> + Send;

    /// Starts Login Flow v2 for the Nextcloud at `server` (as the user typed it) and opens
    /// the browser; returns the login URL to show as a fallback link.
    fn connect_nextcloud(&self, server: String) -> impl Future<Output = Result<String, ProviderError>> + Send;

    /// Fetch everything from the server now.
    fn sync(&self) -> impl Future<Output = Result<(), ProviderError>> + Send;

    fn collections(&self) -> impl Future<Output = Result<Vec<Collection>, ProviderError>> + Send;

    /// Events overlapping `[start, end)`, recurring ones expanded into occurrences.
    fn events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<Event>, ProviderError>> + Send;

    /// All tasks, completed ones included.
    fn todos(&self) -> impl Future<Output = Result<Vec<Todo>, ProviderError>> + Send;

    /// Creates (`href: None`) or updates an event; returns it as stored, with `href` and
    /// `etag` set. An outdated `etag` fails with [`ProviderError::Conflict`].
    fn save_event(&self, event: Event) -> impl Future<Output = Result<Event, ProviderError>> + Send;

    fn delete_event(&self, event: Event) -> impl Future<Output = Result<(), ProviderError>> + Send;

    /// Creates (`href: None`) or updates a task; returns it as stored.
    fn save_todo(&self, todo: Todo) -> impl Future<Output = Result<Todo, ProviderError>> + Send;

    fn delete_todo(&self, todo: Todo) -> impl Future<Output = Result<(), ProviderError>> + Send;
}
