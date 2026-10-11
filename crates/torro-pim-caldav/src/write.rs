//! Writing events and tasks (task K.4 in `docs/tasks/kalender-aufgaben.md`).
//!
//! [`to_ics_event`] / [`to_ics_todo`] produce a complete `VCALENDAR` (RFC 5545: CRLF line
//! ends, lines folded at 75 octets, text escaped, `DTSTAMP`, `PRODID:-//Torro//Workplace//DE`,
//! times in UTC with `Z`, dates as `VALUE=DATE`). The `CalDavClient` methods PUT them to
//! `href` (new items: `<collection>/<uid>.ics`, with `If-None-Match: *`; updates with
//! `If-Match: <etag>`) and DELETE with `If-Match`. A 412 answer becomes
//! `ProviderError::Conflict`. The new ETag comes from the PUT answer's `ETag` header; when
//! the server sends none, it is fetched with a PROPFIND `getetag` on the href.

use torro_pim::{Event, ProviderError, Todo};

use crate::CalDavClient;

/// The event as a `VCALENDAR` text.
pub fn to_ics_event(event: &Event) -> String {
    let _ = event;
    String::new()
}

/// The task as a `VCALENDAR` text.
pub fn to_ics_todo(todo: &Todo) -> String {
    let _ = todo;
    String::new()
}

impl CalDavClient {
    /// Creates (`href: None`; a new uid when empty) or updates the event; returns it with
    /// `href` and `etag` set.
    pub async fn save_event(&self, event: Event) -> Result<Event, ProviderError> {
        let _ = event;
        Err(ProviderError::Other("save_event not implemented yet (task K.4)".into()))
    }

    pub async fn delete_event(&self, event: &Event) -> Result<(), ProviderError> {
        let _ = event;
        Err(ProviderError::Other("delete_event not implemented yet (task K.4)".into()))
    }

    pub async fn save_todo(&self, todo: Todo) -> Result<Todo, ProviderError> {
        let _ = todo;
        Err(ProviderError::Other("save_todo not implemented yet (task K.4)".into()))
    }

    pub async fn delete_todo(&self, todo: &Todo) -> Result<(), ProviderError> {
        let _ = todo;
        Err(ProviderError::Other("delete_todo not implemented yet (task K.4)".into()))
    }
}
