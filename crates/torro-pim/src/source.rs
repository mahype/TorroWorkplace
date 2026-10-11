//! Where the apps get calendars and tasks from.
//!
//! The apps only know [`PimSource`]. In production that is the core over the socket
//! (`torro_ipc::pim::PimClient`); for building and trying the UIs it is
//! [`crate::demo::DemoSource`] with sample data in memory.

use std::future::Future;

use chrono::{DateTime, Utc};

use crate::{Collection, Event, PimAccount, ProviderError, Todo};

pub trait PimSource: Send + Sync {
    /// Every account behind the data, in the order they were added; empty when none is set up.
    /// A sign-in that runs is listed too, with an empty `id` and [`crate::AccountState::SigningIn`].
    /// Apps that show a single state use [`crate::combined_state`]; collections name their
    /// account in their id ([`crate::account`]).
    fn accounts(&self) -> impl Future<Output = Result<Vec<PimAccount>, ProviderError>> + Send;

    /// Starts Login Flow v2 for the Nextcloud at `server` (as the user typed it) and opens
    /// the browser; returns the login URL to show as a fallback link. The account is added to
    /// the others; signing in to one that exists again replaces its sign-in.
    fn connect_nextcloud(&self, server: String) -> impl Future<Output = Result<String, ProviderError>> + Send;

    /// Fetch everything from the servers of all accounts now. One account failing does not keep
    /// the others from syncing; the error is the first failure.
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
