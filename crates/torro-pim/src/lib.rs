//! Calendars and tasks, independent of where they come from.
//!
//! The apps only ever talk to this crate. A provider (`torro-pim-caldav` first, Google
//! or Todoist later as their own crates) fills the offline cache; the UIs read and write
//! the cache, and the sync engine carries changes both ways. Adding a provider never
//! changes an app.
//!
//! Scaffold: the model and the provider contract. Cache and sync follow in the MVP.

pub mod model;
pub mod provider;

pub use model::{Collection, CollectionKind, Event, Todo};
pub use provider::{Change, Provider, ProviderError, SyncToken};
