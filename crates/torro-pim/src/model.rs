//! The shared model, shaped after iCalendar (RFC 5545) so CalDAV maps one to one and
//! other providers map onto it.

/// A calendar or task list on an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    /// Provider-scoped id, for CalDAV the collection URL.
    pub id: String,
    pub name: String,
    pub kind: CollectionKind,
    /// `#RRGGBB` as the server reports it, if any.
    pub color: Option<String>,
}

/// What a collection holds. A CalDAV calendar may hold both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionKind {
    Events,
    Todos,
    Both,
}

/// A calendar entry (VEVENT).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// iCalendar UID.
    pub uid: String,
    pub collection: String,
    pub summary: String,
    /// Start and end as RFC 3339; all-day entries carry a date only.
    pub start: String,
    pub end: Option<String>,
    pub all_day: bool,
    pub location: Option<String>,
    pub description: Option<String>,
}

/// A task (VTODO).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    /// iCalendar UID.
    pub uid: String,
    pub collection: String,
    pub summary: String,
    /// RFC 3339 date or date-time.
    pub due: Option<String>,
    /// iCalendar priority: 1 highest … 9 lowest, `None` undefined.
    pub priority: Option<u8>,
    pub completed: bool,
    /// UID of the parent task (RELATED-TO;RELTYPE=PARENT).
    pub parent: Option<String>,
    pub description: Option<String>,
}
