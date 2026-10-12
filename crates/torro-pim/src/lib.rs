//! Calendars and tasks, independent of where they come from.
//!
//! The model ([`Event`], [`Todo`], [`Collection`]) is shared by the core, the apps and
//! every provider. The apps read and write through [`PimSource`]: the core over the socket,
//! or [`demo::DemoSource`] for trying the UIs. Providers (`torro-pim-caldav` first) sit
//! behind the core. Collection ids name their account ([`account`]).

pub mod account;
pub mod demo;
pub mod model;
pub mod provider;
pub mod source;

pub use account::{AccountState, PimAccount, combined_state};
pub use model::{Collection, CollectionKind, Event, EventTime, Todo, Transparency};
pub use provider::{Change, Provider, ProviderError, SyncToken};
pub use source::PimSource;
