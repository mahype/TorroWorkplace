//! The shared model, shaped after iCalendar (RFC 5545) so CalDAV maps one to one and
//! other providers map onto it.

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
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
    /// The user may add, change and delete items. Shared read-only calendars and
    /// Nextcloud's contact birthdays are not writable.
    #[serde(default = "writable_by_default")]
    pub writable: bool,
}

fn writable_by_default() -> bool {
    true
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EventTime {
    /// A calendar day (all-day entries, VALUE=DATE).
    Date(NaiveDate),
    /// An instant.
    DateTime(DateTime<Utc>),
}

/// A calendar entry (VEVENT). A recurring event appears once per occurrence, each with
/// the start of that occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Event {
    /// iCalendar UID; shared by all occurrences of a recurring event.
    pub uid: String,
    /// Id of the calendar it belongs to, as `calendar.list_calendars` reports it. Must name a
    /// calendar that can be written to when saving.
    pub collection: String,
    /// Where the item lives on the server (CalDAV URL). `None` only for an event not saved yet;
    /// pass the event back with its `href` and `etag` to change or delete it.
    pub href: Option<String>,
    /// Server version of the item, sent back on changes so the server can refuse stale writes.
    /// `None` only for an event not saved yet.
    pub etag: Option<String>,
    /// Title of the event.
    pub summary: String,
    /// Start: a calendar day for all-day events, otherwise an instant in UTC.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Todo {
    /// iCalendar UID.
    pub uid: String,
    /// Id of the task list it belongs to, as `tasks.list_lists` reports it. Must name a list
    /// that can be written to when saving.
    pub collection: String,
    /// Where the item lives on the server (CalDAV URL). `None` only for a task not saved yet;
    /// pass the task back with its `href` and `etag` to change or delete it.
    pub href: Option<String>,
    /// Server version of the item, sent back on changes so the server can refuse stale writes.
    /// `None` only for a task not saved yet.
    pub etag: Option<String>,
    /// Title of the task.
    pub summary: String,
    /// When the task is due: a day or an instant. `None` when it has no due date.
    pub due: Option<EventTime>,
    /// iCalendar priority: 1 highest … 9 lowest, `None` undefined.
    pub priority: Option<u8>,
    pub completed: bool,
    /// UID of the parent task (RELATED-TO;RELTYPE=PARENT).
    pub parent: Option<String>,
    pub description: Option<String>,
}
