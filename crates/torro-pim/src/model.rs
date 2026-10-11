//! The shared model, shaped after iCalendar (RFC 5545) so CalDAV maps one to one and
//! other providers map onto it.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// A calendar or task list on an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    /// Provider-scoped id, for CalDAV the collection URL.
    pub id: String,
    pub name: String,
    pub kind: CollectionKind,
    /// `#RRGGBB` as the server reports it, if any.
    pub color: Option<String>,
}

/// What a collection holds. A CalDAV calendar may hold both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollectionKind {
    Events,
    Todos,
    Both,
}

impl CollectionKind {
    pub fn has_events(self) -> bool {
        matches!(self, CollectionKind::Events | CollectionKind::Both)
    }

    pub fn has_todos(self) -> bool {
        matches!(self, CollectionKind::Todos | CollectionKind::Both)
    }
}

/// When an event starts or ends, or a task is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventTime {
    /// A calendar day (all-day entries, VALUE=DATE).
    Date(NaiveDate),
    /// An instant.
    DateTime(DateTime<Utc>),
}

/// A calendar entry (VEVENT). A recurring event appears once per occurrence, each with
/// the start of that occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// iCalendar UID; shared by all occurrences of a recurring event.
    pub uid: String,
    /// [`Collection::id`] it belongs to.
    pub collection: String,
    /// Where the item lives on the server; `None` for an event not saved yet.
    pub href: Option<String>,
    /// Version on the server, guards writes; `None` for an event not saved yet.
    pub etag: Option<String>,
    pub summary: String,
    pub start: EventTime,
    /// Exclusive end (DTEND, or DTSTART + DURATION); `None` when the entry has neither.
    pub end: Option<EventTime>,
    pub location: Option<String>,
    pub description: Option<String>,
    /// One occurrence of a recurring series. Editing series is not supported yet, so the
    /// apps show these read-only.
    pub recurring: bool,
}

impl Event {
    /// All-day entries carry a date without a time.
    pub fn all_day(&self) -> bool {
        matches!(self.start, EventTime::Date(_))
    }
}

/// A task (VTODO).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    /// iCalendar UID.
    pub uid: String,
    pub collection: String,
    pub href: Option<String>,
    pub etag: Option<String>,
    pub summary: String,
    pub due: Option<EventTime>,
    /// iCalendar priority: 1 highest … 9 lowest, `None` undefined.
    pub priority: Option<u8>,
    pub completed: bool,
    /// UID of the parent task (RELATED-TO;RELTYPE=PARENT).
    pub parent: Option<String>,
    pub description: Option<String>,
}
