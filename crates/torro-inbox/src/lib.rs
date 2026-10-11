//! The inbox: one place for everything that wants the user's attention (`docs/eingang.md`).
//!
//! This crate holds what the core, the apps and the API share, without any I/O: the entries
//! ([`Item`] and what it points to, the wire format of the `inbox.*` requests), the rules for
//! merging, lanes, waking, clean-up and order ([`rules`]), the trait the app reads through
//! ([`InboxSource`]) and sample data for `--demo` ([`demo::DemoSource`]). Storage, rights and
//! enforcing the security rules live in the core (task E1.2).

pub mod demo;
pub mod item;
pub mod rules;
pub mod source;

pub use item::{
    Action, AgentInfo, AgentKind, Item, Kind, Lane, Link, ParseSourceError, Sender, SourceInfo, SourceKind, SourceRef,
    State,
};
pub use rules::Poster;
pub use source::{InboxError, InboxSource, ItemFilter, StateFilter};
