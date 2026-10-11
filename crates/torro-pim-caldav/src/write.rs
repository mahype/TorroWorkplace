//! Writing events and tasks (task K.4 in `docs/tasks/kalender-aufgaben.md`).
//!
//! [`to_ics_event`] / [`to_ics_todo`] produce a complete `VCALENDAR` (RFC 5545: CRLF line
//! ends, lines folded at 75 octets, text escaped, `DTSTAMP`, `PRODID:-//Torro//Workplace//DE`,
//! times in UTC with `Z`, dates as `VALUE=DATE`). The `CalDavClient` methods PUT them to
//! `href` (new items: `<collection>/<uid>.ics`, with `If-None-Match: *`; updates with
//! `If-Match: <etag>`) and DELETE with `If-Match`. A 412 answer becomes
//! `ProviderError::Conflict`. The new ETag comes from the PUT answer's `ETag` header; when
//! the server sends none, it is fetched with a PROPFIND `getetag` on the href.
//!
//! Every PUT, DELETE and GET carries the account's password, so its URL must lie on the
//! client's own server (`CalDavClient::on_server`, task M1.4); a collection or href on another
//! host is refused before anything is sent.
//!
//! An existing item (with `href`) is never written or deleted without its ETag (task A.2b): a
//! PUT or DELETE without `If-Match` would blindly replace whatever the server holds now, a
//! whole series included. [`CalDavClient::stored_item`] reads what is there, so the core can
//! check for a series before it writes.
//!
//! Other programs keep more in an item than the model knows: Nextcloud Tasks its sort order,
//! categories and reminders, Thunderbird time zones, attendees and `X-MOZ-*` (task P.1 in
//! `docs/tasks/github.md`). So an existing item is not written from the model alone:
//! [`merge_ics_event`] / [`merge_ics_todo`] start from the object the server holds and replace
//! only the properties of model fields that changed. Everything else, other components
//! (`VTIMEZONE`, `VALARM`) and the parameters of unchanged properties (`TZID`) included, stays
//! as it was. Lines are folded anew, so the result is valid even when the server sent long
//! unfolded lines.

use chrono::{DateTime, NaiveDate, Utc};
use reqwest::{StatusCode, Url};
use roxmltree::Node;
use torro_pim::{Event, EventTime, ProviderError, Todo};

use crate::CalDavClient;
use crate::ical::{self, Property};

const NS_DAV: &str = "DAV:";
const PRODID: &str = "-//Torro//Workplace//DE";
/// RFC 5545 section 3.1: a content line is at most 75 octets, longer ones are folded.
/// The leading space of a continuation line counts towards the 75.
const MAX_LINE_OCTETS: usize = 75;

/// The event as a new `VCALENDAR` text.
pub fn to_ics_event(event: &Event) -> String {
    event_ics(event, Utc::now())
}

/// The task as a new `VCALENDAR` text.
pub fn to_ics_todo(todo: &Todo) -> String {
    todo_ics(todo, Utc::now())
}

/// The event written onto `stored`, the iCalendar object the server holds for it.
///
/// The VEVENT with the event's `uid` (else the first VEVENT) gets the model's values for the
/// fields that differ from what `stored` says, plus a new `DTSTAMP` and `LAST-MODIFIED`; the
/// properties of unchanged fields, every property the model does not know, nested and other
/// components stay as they are. A field the model cleared (no location) loses its property.
/// Without a VEVENT in `stored` the result is [`to_ics_event`].
pub fn merge_ics_event(stored: &str, event: &Event) -> String {
    event_onto(stored, event, Utc::now())
}

/// The task written onto `stored`, the iCalendar object the server holds for it.
///
/// Like [`merge_ics_event`] for the VTODO with the task's `uid` (else the first VTODO):
/// `VALARM`, `ATTACH`, `RELATED-TO` of other kinds and the like stay; `URL`, `CATEGORIES`,
/// `X-APPLE-SORT-ORDER` and `X-TORRO-LINK` (only the link's id) are model fields since P.2 and
/// change only when they differ. Without a VTODO in `stored` the result is [`to_ics_todo`].
pub fn merge_ics_todo(stored: &str, todo: &Todo) -> String {
    todo_onto(stored, todo, Utc::now())
}

impl CalDavClient {
    /// Creates (`href: None`; a new uid when empty) or updates the event; returns it with
    /// `href` and `etag` set. An update needs the `etag` it was read with; without one nothing
    /// is sent and the answer is an error. An update reads the stored object first and writes
    /// the event onto it ([`merge_ics_event`]), so what other programs stored there stays.
    pub async fn save_event(&self, mut event: Event) -> Result<Event, ProviderError> {
        if event.uid.is_empty() {
            event.uid = new_uid();
        }
        // Refused before anything is read: an update without its ETag is never sent.
        precondition(event.href.as_deref(), event.etag.as_deref())?;
        let ics = match self.stored_ics(&event.collection, event.href.as_deref()).await? {
            Some(stored) => merge_ics_event(&stored, &event),
            None => to_ics_event(&event),
        };
        let (href, etag) =
            self.put_item(&event.collection, &event.uid, event.href.as_deref(), event.etag.as_deref(), ics).await?;
        event.href = Some(href);
        event.etag = Some(etag);
        Ok(event)
    }

    /// Creates `event` as a new item in its collection (its `href` and `etag` are ignored),
    /// written onto the object the server holds for `from` (its `collection` and `href`), so
    /// that moving an event to another calendar keeps what the model does not know. `from`
    /// itself is left alone; the caller deletes it afterwards. Without `from.href`, or when
    /// nothing is stored there any more, the event is written from the model alone.
    pub async fn copy_event(&self, from: &Event, event: Event) -> Result<Event, ProviderError> {
        let stored = self.stored_ics(&from.collection, from.href.as_deref()).await?;
        let mut event = Event { href: None, etag: None, ..event };
        if event.uid.is_empty() {
            event.uid = new_uid();
        }
        let ics = match stored {
            Some(stored) => merge_ics_event(&stored, &event),
            None => to_ics_event(&event),
        };
        let (href, etag) = self.put_item(&event.collection, &event.uid, None, None, ics).await?;
        event.href = Some(href);
        event.etag = Some(etag);
        Ok(event)
    }

    /// Deletes the event if its ETag still matches. An event that was never saved has
    /// nothing on the server to delete and counts as done; one with `href` but without `etag`
    /// is refused before anything is sent.
    pub async fn delete_event(&self, event: &Event) -> Result<(), ProviderError> {
        self.delete_item(&event.collection, &event.uid, event.href.as_deref(), event.etag.as_deref()).await
    }

    /// Creates (`href: None`; a new uid when empty) or updates the task; returns it with
    /// `href` and `etag` set. An update needs the `etag` it was read with; without one nothing
    /// is sent and the answer is an error. An update reads the stored object first and writes
    /// the task onto it ([`merge_ics_todo`]), so what other programs stored there stays.
    pub async fn save_todo(&self, mut todo: Todo) -> Result<Todo, ProviderError> {
        if todo.uid.is_empty() {
            todo.uid = new_uid();
        }
        precondition(todo.href.as_deref(), todo.etag.as_deref())?;
        let ics = match self.stored_ics(&todo.collection, todo.href.as_deref()).await? {
            Some(stored) => merge_ics_todo(&stored, &todo),
            None => to_ics_todo(&todo),
        };
        let (href, etag) =
            self.put_item(&todo.collection, &todo.uid, todo.href.as_deref(), todo.etag.as_deref(), ics).await?;
        todo.href = Some(href);
        todo.etag = Some(etag);
        Ok(todo)
    }

    /// Creates `todo` as a new item in its collection (its `href` and `etag` are ignored),
    /// written onto the object the server holds for `from`, like [`CalDavClient::copy_event`]:
    /// moving a task to another list keeps its reminders, categories and sort order.
    pub async fn copy_todo(&self, from: &Todo, todo: Todo) -> Result<Todo, ProviderError> {
        let stored = self.stored_ics(&from.collection, from.href.as_deref()).await?;
        let mut todo = Todo { href: None, etag: None, ..todo };
        if todo.uid.is_empty() {
            todo.uid = new_uid();
        }
        let ics = match stored {
            Some(stored) => merge_ics_todo(&stored, &todo),
            None => to_ics_todo(&todo),
        };
        let (href, etag) = self.put_item(&todo.collection, &todo.uid, None, None, ics).await?;
        todo.href = Some(href);
        todo.etag = Some(etag);
        Ok(todo)
    }

    /// Deletes the task if its ETag still matches. A task that was never saved has nothing
    /// on the server to delete and counts as done; one with `href` but without `etag` is
    /// refused before anything is sent.
    pub async fn delete_todo(&self, todo: &Todo) -> Result<(), ProviderError> {
        self.delete_item(&todo.collection, &todo.uid, todo.href.as_deref(), todo.etag.as_deref()).await
    }

    /// PUTs one iCalendar object and returns the `(href, etag)` the item now has. `href`
    /// `None` means a new item, which must not overwrite anything (`If-None-Match: *`); an
    /// existing one is only replaced in the version `etag` names ([`precondition`]).
    async fn put_item(
        &self,
        collection: &str,
        uid: &str,
        href: Option<&str>,
        etag: Option<&str>,
        ics: String,
    ) -> Result<(String, String), ProviderError> {
        let (header, value) = precondition(href, etag)?;
        let (url, href) = match href {
            Some(href) => (resolve(collection, href)?, href.to_string()),
            None => {
                let url = new_item_url(collection, uid)?;
                (url.clone(), url.to_string())
            }
        };
        // The password goes with the PUT: never to another server than the account's.
        let url = self.on_server(url)?;
        let request = self
            .http
            .put(url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .header("Content-Type", "text/calendar; charset=utf-8")
            .body(ics)
            .header(header, value);
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status, uid, &url));
        }
        let header_etag = etag_header(&response);
        let etag = match header_etag {
            Some(etag) => etag,
            None => self.fetch_etag(&url).await?,
        };
        Ok((href, etag))
    }

    /// DELETEs one item, only in the version `etag` names. `href` `None` (never saved) is done
    /// without a request.
    async fn delete_item(
        &self,
        collection: &str,
        uid: &str,
        href: Option<&str>,
        etag: Option<&str>,
    ) -> Result<(), ProviderError> {
        let Some(href) = href else { return Ok(()) };
        let (header, value) = precondition(Some(href), etag)?;
        let url = self.on_server(resolve(collection, href)?)?;
        let request =
            self.http.delete(url.clone()).basic_auth(&self.username, Some(&self.password)).header(header, value);
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        // 404: already gone, which is what the caller wanted.
        if status.is_success() || status == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(status_error(status, uid, &url))
        }
    }

    /// What the server holds at `href`, an item directly in `collection` ([`item_url`]): its
    /// ETag and whether it is a series. `Ok(None)` when there is no such item (404).
    ///
    /// The core asks before it changes, moves or deletes an item it has not cached in that
    /// version: only the object itself tells whether writing it back would drop a rule.
    pub async fn stored_item(&self, collection: &str, href: &str) -> Result<Option<StoredItem>, ProviderError> {
        let Some(fetched) = self.fetch_item(collection, href).await? else { return Ok(None) };
        let etag = match fetched.etag {
            Some(etag) => etag,
            None => self.fetch_etag(&fetched.url).await?,
        };
        Ok(Some(StoredItem { etag, recurring: crate::ical::is_series(&fetched.ics) }))
    }

    /// The iCalendar object stored at `href` in `collection`, to write a change onto; `None`
    /// without `href` (a new item) or when nothing is stored there (404). Which version it is
    /// does not matter here: the PUT that follows carries the caller's `If-Match`, so a write
    /// built on a newer version than the caller read fails with a conflict.
    async fn stored_ics(&self, collection: &str, href: Option<&str>) -> Result<Option<String>, ProviderError> {
        let Some(href) = href else { return Ok(None) };
        Ok(self.fetch_item(collection, href).await?.map(|fetched| fetched.ics))
    }

    /// GETs one item directly in `collection` ([`item_url`]); `Ok(None)` on 404.
    async fn fetch_item(&self, collection: &str, href: &str) -> Result<Option<Fetched>, ProviderError> {
        let url = self.on_server(resolve(collection, href)?)?;
        let response = self
            .http
            .get(url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(status_error(status, href, &url));
        }
        let etag = etag_header(&response);
        let ics = response.text().await.map_err(transport_error)?;
        Ok(Some(Fetched { url, etag, ics }))
    }

    /// The ETag of one item, for servers that do not send it with the PUT answer.
    async fn fetch_etag(&self, url: &Url) -> Result<String, ProviderError> {
        let body = r#"<d:propfind xmlns:d="DAV:"><d:prop><d:getetag/></d:prop></d:propfind>"#;
        let xml = self.request("PROPFIND", url, "0", body).await?;
        etag_from_multistatus(&xml).ok_or_else(|| ProviderError::Other(format!("{url}: server sent no ETag")))
    }
}

/// What an item on the server is, as [`CalDavClient::stored_item`] read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredItem {
    /// The ETag of the version read, quotes as the server sends them.
    pub etag: String,
    /// The object is a series (`RRULE`, `RDATE` or `RECURRENCE-ID`): writing it back from one
    /// event or task would drop the rule and every exception.
    pub recurring: bool,
}

/// One item as a GET answered it.
struct Fetched {
    url: Url,
    /// The `ETag` header, when the server sent one.
    etag: Option<String>,
    ics: String,
}

/// The precondition header of a PUT or DELETE: `If-None-Match: *` for a new item (no `href`),
/// so nothing is overwritten, and `If-Match: <etag>` for an existing one. An existing item
/// without `etag` is refused: without `If-Match` the write would blindly replace whatever the
/// server holds now.
fn precondition<'a>(href: Option<&str>, etag: Option<&'a str>) -> Result<(&'static str, &'a str), ProviderError> {
    match (href, etag) {
        (None, _) => Ok(("If-None-Match", "*")),
        (Some(_), Some(etag)) if !etag.trim().is_empty() => Ok(("If-Match", etag)),
        (Some(href), _) => Err(ProviderError::Other(format!("{href} has no etag: reload the item and pass its etag"))),
    }
}

/// The `ETag` header of an answer, when there is a non-empty one.
fn etag_header(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn event_ics(event: &Event, now: DateTime<Utc>) -> String {
    calendar("VEVENT", &new_lines(event_fields(event, None, now)))
}

fn todo_ics(todo: &Todo, now: DateTime<Utc>) -> String {
    calendar("VTODO", &new_lines(todo_fields(todo, None, now)))
}

fn event_onto(stored: &str, event: &Event, now: DateTime<Utc>) -> String {
    let merged = merge_component(stored, "VEVENT", &event.uid, |props| {
        let before = ical::to_event(props, &event.collection);
        let mut fields = event_fields(event, before.as_ref(), now);
        fields.push(last_modified(now));
        fields
    });
    merged.unwrap_or_else(|| event_ics(event, now))
}

fn todo_onto(stored: &str, todo: &Todo, now: DateTime<Utc>) -> String {
    let merged = merge_component(stored, "VTODO", &todo.uid, |props| {
        let before = ical::to_todo(props, &todo.collection);
        let mut fields = todo_fields(todo, before.as_ref(), now);
        fields.push(last_modified(now));
        fields
    });
    merged.unwrap_or_else(|| todo_ics(todo, now))
}

/// Which stored properties a [`Field`] stands for.
#[derive(Debug, Clone, Copy)]
enum Covers {
    /// Every property with one of these names (any case).
    Names(&'static [&'static str]),
    /// `RELATED-TO` to the parent ([`ical::is_parent_link`]); links of other kinds are kept.
    ParentLink,
}

impl Covers {
    fn matches(self, prop: &Property<'_>) -> bool {
        match self {
            Covers::Names(names) => names.iter().any(|name| prop.name.eq_ignore_ascii_case(name)),
            Covers::ParentLink => ical::is_parent_link(prop),
        }
    }
}

/// One field of the model as content lines. When `changed`, the stored properties it covers
/// are dropped and `lines` (unfolded, possibly none) take their place; otherwise the stored
/// properties stay as they are, with their parameters and their exact text.
struct Field {
    covers: Covers,
    changed: bool,
    lines: Vec<String>,
}

impl Field {
    fn new(names: &'static [&'static str], changed: bool, lines: Vec<String>) -> Field {
        Field { covers: Covers::Names(names), changed, lines }
    }
}

/// The lines of all changed fields, in order: the body of a new component.
fn new_lines(fields: Vec<Field>) -> Vec<String> {
    fields.into_iter().filter(|field| field.changed).flat_map(|field| field.lines).collect()
}

/// `DTSTAMP` says when the object was last written, so every write sets it.
fn stamp(now: DateTime<Utc>) -> Field {
    Field::new(&["DTSTAMP"], true, vec![format!("DTSTAMP:{}", utc(now))])
}

/// `LAST-MODIFIED`, set on every change of a stored object; new objects leave it out.
fn last_modified(now: DateTime<Utc>) -> Field {
    Field::new(&["LAST-MODIFIED"], true, vec![format!("LAST-MODIFIED:{}", utc(now))])
}

/// The fields of an event. `before` is the event as the stored object describes it; `None`
/// (a new object, or one the model cannot read) marks every field as changed.
fn event_fields(event: &Event, before: Option<&Event>, now: DateTime<Utc>) -> Vec<Field> {
    let differs = |same: fn(&Event, &Event) -> bool| before.is_none_or(|before| !same(before, event));
    let start_moved = differs(|a, b| a.start == b.start);
    vec![
        Field::new(&["UID"], differs(|a, b| a.uid == b.uid), vec![text_line("UID", &event.uid)]),
        stamp(now),
        Field::new(&["DTSTART"], start_moved, vec![time_line("DTSTART", event.start)]),
        // A stored DURATION counts from DTSTART: when the start moves, the end is written as
        // DTEND, or the event would grow or shrink.
        Field::new(
            &["DTEND", "DURATION"],
            start_moved || differs(|a, b| a.end == b.end),
            event.end.map(|end| time_line("DTEND", end)).into_iter().collect(),
        ),
        Field::new(&["SUMMARY"], differs(|a, b| a.summary == b.summary), vec![text_line("SUMMARY", &event.summary)]),
        Field::new(
            &["LOCATION"],
            differs(|a, b| non_empty(&a.location) == non_empty(&b.location)),
            optional_text("LOCATION", &event.location),
        ),
        Field::new(
            &["DESCRIPTION"],
            differs(|a, b| non_empty(&a.description) == non_empty(&b.description)),
            optional_text("DESCRIPTION", &event.description),
        ),
    ]
}

/// The fields of a task; `before` as in [`event_fields`].
fn todo_fields(todo: &Todo, before: Option<&Todo>, now: DateTime<Utc>) -> Vec<Field> {
    let differs = |same: fn(&Todo, &Todo) -> bool| before.is_none_or(|before| !same(before, todo));
    let parent = Field {
        covers: Covers::ParentLink,
        changed: differs(|a, b| a.parent == b.parent),
        lines: todo.parent.iter().map(|parent| format!("RELATED-TO;RELTYPE=PARENT:{}", escape(parent))).collect(),
    };
    vec![
        Field::new(&["UID"], differs(|a, b| a.uid == b.uid), vec![text_line("UID", &todo.uid)]),
        stamp(now),
        Field::new(&["SUMMARY"], differs(|a, b| a.summary == b.summary), vec![text_line("SUMMARY", &todo.summary)]),
        Field::new(
            &["DUE"],
            differs(|a, b| a.due == b.due),
            todo.due.map(|due| time_line("DUE", due)).into_iter().collect(),
        ),
        Field::new(
            &["PRIORITY"],
            differs(|a, b| priority(a) == priority(b)),
            priority(todo).map(|p| format!("PRIORITY:{p}")).into_iter().collect(),
        ),
        // Status, completion time and percentage move together. While the state stays, all
        // three stay as stored: `IN-PROCESS` at 40 %, or the time the task was really done.
        Field::new(
            &["STATUS", "COMPLETED", "PERCENT-COMPLETE"],
            differs(|a, b| a.completed == b.completed),
            completion_lines(todo.completed, now),
        ),
        parent,
        Field::new(
            &["DESCRIPTION"],
            differs(|a, b| non_empty(&a.description) == non_empty(&b.description)),
            optional_text("DESCRIPTION", &todo.description),
        ),
        Field::new(
            &["URL"],
            differs(|a, b| uri(&a.url) == uri(&b.url)),
            uri(&todo.url).map(|url| format!("URL:{url}")).into_iter().collect(),
        ),
        // All CATEGORIES lines are one field: while the tags stay, so do the stored lines.
        Field::new(
            &["CATEGORIES"],
            differs(|a, b| tags(a) == tags(b)),
            categories_line(&tags(todo)).into_iter().collect(),
        ),
        Field::new(
            &["X-APPLE-SORT-ORDER"],
            differs(|a, b| a.sort_order == b.sort_order),
            todo.sort_order.map(|n| format!("X-APPLE-SORT-ORDER:{n}")).into_iter().collect(),
        ),
        // Only the link's id is stored; what the core fetched for it is not the task's.
        Field::new(
            &["X-TORRO-LINK"],
            differs(|a, b| link_id(a) == link_id(b)),
            link_id(todo).map(|id| text_line("X-TORRO-LINK", id)).into_iter().collect(),
        ),
    ]
}

/// A URI value as it is written: without control characters (a line break would start a new
/// property, and URIs are not escaped like text) and surrounding spaces; empty counts as absent.
fn uri(value: &Option<String>) -> Option<String> {
    let clean: String = value.as_deref()?.chars().filter(|c| !c.is_control()).collect();
    Some(clean.trim().to_string()).filter(|clean| !clean.is_empty())
}

/// The tags as they are stored: trimmed, empty ones dropped (as [`ical`] reads them).
fn tags(todo: &Todo) -> Vec<&str> {
    todo.categories.iter().map(|tag| tag.trim()).filter(|tag| !tag.is_empty()).collect()
}

/// `CATEGORIES:a,b\, c`: one line, tags escaped so a comma inside one stays part of it.
fn categories_line(tags: &[&str]) -> Option<String> {
    (!tags.is_empty()).then(|| {
        let escaped: Vec<String> = tags.iter().map(|tag| escape(tag)).collect();
        format!("CATEGORIES:{}", escaped.join(","))
    })
}

/// The id of the task's link, trimmed; empty counts as no link.
fn link_id(todo: &Todo) -> Option<&str> {
    todo.link.as_ref().map(|link| link.id.trim()).filter(|id| !id.is_empty())
}

/// RFC 5545: 1 is the highest priority, 9 the lowest, 0 means undefined.
fn priority(todo: &Todo) -> Option<u8> {
    todo.priority.filter(|p| (1..=9).contains(p))
}

fn completion_lines(completed: bool, now: DateTime<Utc>) -> Vec<String> {
    if completed {
        vec!["STATUS:COMPLETED".to_string(), format!("COMPLETED:{}", utc(now)), "PERCENT-COMPLETE:100".to_string()]
    } else {
        vec!["STATUS:NEEDS-ACTION".to_string()]
    }
}

/// An empty text counts as absent, in the model and as read.
fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|v| !v.is_empty())
}

/// The line of an optional text field. An empty value is treated as absent: leaving the line
/// out is how a cleared field is stored.
fn optional_text(name: &str, value: &Option<String>) -> Vec<String> {
    non_empty(value).map(|value| text_line(name, value)).into_iter().collect()
}

/// `stored` with the `component` (`VEVENT`, `VTODO`) that has `uid` (else the first one)
/// rewritten by `fields`, which gets that component's own properties (not those of nested
/// components such as `VALARM`). Properties covered by a changed field are dropped, the
/// changed fields' lines follow right after `BEGIN` (RFC 5545 puts properties before nested
/// components), and every other line stays. `None` when `stored` holds no such component.
fn merge_component(
    stored: &str,
    component: &str,
    uid: &str,
    fields: impl FnOnce(&[Property<'_>]) -> Vec<Field>,
) -> Option<String> {
    let lines = ical::unfold(stored);
    let parsed: Vec<Option<Property<'_>>> = lines.iter().map(|line| ical::parse_line(line)).collect();
    let candidates = components(&parsed, component);
    let has_uid = |candidate: &&Candidate| {
        candidate
            .own
            .iter()
            .filter_map(|&i| parsed.get(i)?.as_ref())
            .any(|p| p.name.eq_ignore_ascii_case("UID") && p.value.trim() == uid)
    };
    let chosen = candidates.iter().find(has_uid).or_else(|| candidates.first())?;

    let own: Vec<Property<'_>> = chosen.own.iter().filter_map(|&i| ical::parse_line(lines.get(i)?)).collect();
    let fields = fields(&own);
    let dropped = |prop: &Property<'_>| fields.iter().any(|field| field.changed && field.covers.matches(prop));

    let mut out = String::new();
    for (i, (line, prop)) in lines.iter().zip(&parsed).enumerate() {
        if chosen.own.contains(&i) && prop.as_ref().is_some_and(dropped) {
            continue;
        }
        push_line(&mut out, line);
        if i == chosen.begin {
            for new in fields.iter().filter(|field| field.changed).flat_map(|field| &field.lines) {
                push_line(&mut out, new);
            }
        }
    }
    Some(out)
}

/// One top-level component of a stored object: the index of its `BEGIN` line and of its own
/// property lines.
struct Candidate {
    begin: usize,
    own: Vec<usize>,
}

/// The `component`s directly inside the `VCALENDAR`, in order. Lines of nested components
/// (`VALARM`) and of other components (`VTIMEZONE` with its `STANDARD`/`DAYLIGHT`) belong to
/// none of them.
fn components(parsed: &[Option<Property<'_>>], component: &str) -> Vec<Candidate> {
    let mut found = Vec::new();
    let mut current: Option<Candidate> = None;
    // 1 inside the VCALENDAR, 2 inside a component, 3 and more in nested ones.
    let mut depth = 0usize;
    for (i, prop) in parsed.iter().enumerate() {
        let Some(prop) = prop else { continue };
        if prop.name.eq_ignore_ascii_case("BEGIN") {
            if depth == 1 && prop.value.trim().eq_ignore_ascii_case(component) {
                current = Some(Candidate { begin: i, own: Vec::new() });
            }
            depth += 1;
        } else if prop.name.eq_ignore_ascii_case("END") {
            depth = depth.saturating_sub(1);
            if depth == 1
                && let Some(done) = current.take()
            {
                found.push(done);
            }
        } else if depth == 2
            && let Some(candidate) = current.as_mut()
        {
            candidate.own.push(i);
        }
    }
    found
}

/// Wraps one component in a `VCALENDAR`. `body` holds unfolded content lines.
fn calendar(component: &str, body: &[String]) -> String {
    let mut out = String::new();
    push_line(&mut out, "BEGIN:VCALENDAR");
    push_line(&mut out, "VERSION:2.0");
    push_line(&mut out, &format!("PRODID:{PRODID}"));
    push_line(&mut out, &format!("BEGIN:{component}"));
    for line in body {
        push_line(&mut out, line);
    }
    push_line(&mut out, &format!("END:{component}"));
    push_line(&mut out, "END:VCALENDAR");
    out
}

/// Appends one content line, folded at 75 octets and ended with CRLF. Folds fall between
/// characters, never inside a UTF-8 sequence.
fn push_line(out: &mut String, line: &str) {
    let mut octets = 0;
    for ch in line.chars() {
        let len = ch.len_utf8();
        if octets + len > MAX_LINE_OCTETS {
            out.push_str("\r\n ");
            octets = 1;
        }
        out.push(ch);
        octets += len;
    }
    out.push_str("\r\n");
}

fn text_line(name: &str, value: &str) -> String {
    format!("{name}:{}", escape(value))
}

/// Escapes a TEXT value (RFC 5545 section 3.3.11). A CR is dropped; a line break becomes `\n`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// `DTSTART:20261011T080000Z` for a time, `DTSTART;VALUE=DATE:20261011` for a day.
fn time_line(name: &str, time: EventTime) -> String {
    match time {
        EventTime::Date(date) => format!("{name};VALUE=DATE:{}", date_value(date)),
        EventTime::DateTime(at) => format!("{name}:{}", utc(at)),
    }
}

fn utc(at: DateTime<Utc>) -> String {
    at.format("%Y%m%dT%H%M%SZ").to_string()
}

fn date_value(date: NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

/// A UID for a new item: `<nanoseconds since 1970>-<process id>@torro`. It needs no random
/// number generator and no new dependency; the process id keeps two instances started in
/// the same nanosecond apart.
fn new_uid() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{nanos}-{}@torro", std::process::id())
}

fn parse_url(text: &str) -> Result<Url, ProviderError> {
    Url::parse(text).map_err(|err| ProviderError::Other(format!("{text}: {err}")))
}

/// The absolute URL of the item `href` (a path or a full URL, as the server lists it), but only
/// when it is an item **directly in** `collection`; `None` otherwise.
///
/// `href` is resolved against the collection the way the request will be sent (WHATWG URL
/// rules: `..`, `%2e%2e` and `\` are applied, `//` starts a new host), and only then compared:
/// same scheme, host and port, no user, query or fragment, and a path that is the collection's
/// path plus one non-empty name without `/` and without an encoded `/` or `\`. A text prefix
/// would not do: `…/work/../home/x.ics` starts with `…/work/` but names an item in `home`.
///
/// Every write and delete goes through here, so no href can reach outside its collection, and
/// the core checks a client's hrefs with the same function before anything is sent.
pub fn item_url(collection: &str, href: &str) -> Option<Url> {
    let mut base = Url::parse(collection).ok()?;
    if !base.path().ends_with('/') {
        let folder = format!("{}/", base.path());
        base.set_path(&folder);
    }
    let url = base.join(href.trim()).ok()?;
    let same_server = url.scheme() == base.scheme()
        && url.host_str() == base.host_str()
        && url.port_or_known_default() == base.port_or_known_default();
    let nothing_else =
        url.username().is_empty() && url.password().is_none() && url.query().is_none() && url.fragment().is_none();
    let name = url.path().strip_prefix(base.path())?;
    let lower = name.to_ascii_lowercase();
    let one_name = !name.is_empty()
        && !name.contains('/')
        && !lower.contains("%2f")
        && !lower.contains("%5c")
        && !lower.replace("%2e", ".").chars().all(|c| c == '.');
    (same_server && nothing_else && one_name).then_some(url)
}

/// [`item_url`], as an error when `href` is not an item directly in `collection`.
fn resolve(collection: &str, href: &str) -> Result<Url, ProviderError> {
    item_url(collection, href).ok_or_else(|| ProviderError::Other(format!("{href} is not an item in {collection}")))
}

/// `<collection with trailing slash><uid>.ics`; the uid is one path segment, so a `/` in it
/// is escaped rather than read as a folder.
fn new_item_url(collection: &str, uid: &str) -> Result<Url, ProviderError> {
    let mut url = parse_url(collection)?;
    url.path_segments_mut()
        .map_err(|()| ProviderError::Other(format!("{collection} cannot hold items")))?
        .pop_if_empty()
        .push(&format!("{uid}.ics"));
    Ok(url)
}

/// The error for a failed PUT or DELETE answer.
fn status_error(status: StatusCode, uid: &str, url: &Url) -> ProviderError {
    match status {
        StatusCode::PRECONDITION_FAILED => ProviderError::Conflict { uid: uid.to_string() },
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Auth,
        other => ProviderError::Other(format!("{url}: HTTP {}", other.as_u16())),
    }
}

/// Connection problems and timeouts mean the server is out of reach.
fn transport_error(err: reqwest::Error) -> ProviderError {
    if err.is_connect() || err.is_timeout() { ProviderError::Offline } else { ProviderError::Other(err.to_string()) }
}

/// The `getetag` of the item in a PROPFIND multistatus, from the answer with status 200.
fn etag_from_multistatus(xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    doc.root_element()
        .children()
        .filter(|n| is_dav(*n, "response"))
        .flat_map(|response| response.children().filter(|n| is_dav(*n, "propstat")))
        .filter(|propstat| dav_child(*propstat, "status").and_then(|s| s.text()).is_some_and(|s| s.contains(" 200")))
        .find_map(|propstat| dav_child(propstat, "prop"))
        .and_then(|prop| dav_child(prop, "getetag"))
        .and_then(|etag| etag.text())
        .map(str::trim)
        .filter(|etag| !etag.is_empty())
        .map(str::to_string)
}

fn is_dav(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace() == Some(NS_DAV)
}

fn dav_child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| is_dav(*n, name))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use torro_pim::model::{GitHubDetails, GitHubKind, GitHubLabel, GitHubState, Link};

    use super::*;

    const COLLECTION: &str = "https://cloud.example.org/remote.php/dav/calendars/anna/personal/";

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 11, 12, 0, 0).unwrap()
    }

    fn event() -> Event {
        Event {
            uid: "evt-1@torro".to_string(),
            collection: COLLECTION.to_string(),
            href: None,
            etag: None,
            summary: "Planung".to_string(),
            start: EventTime::DateTime(at(11, 8)),
            end: Some(EventTime::DateTime(at(11, 9))),
            location: None,
            description: None,
            recurring: false,
        }
    }

    fn todo() -> Todo {
        Todo {
            uid: "todo-2@torro".to_string(),
            collection: COLLECTION.to_string(),
            href: None,
            etag: None,
            summary: "Angebot schreiben".to_string(),
            due: Some(EventTime::DateTime(at(12, 17))),
            priority: Some(1),
            completed: true,
            parent: Some("todo-1@torro".to_string()),
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
        }
    }

    /// The content lines of an iCalendar text, after unfolding.
    fn unfolded_lines(ics: &str) -> Vec<String> {
        ics.replace("\r\n ", "").split("\r\n").filter(|l| !l.is_empty()).map(str::to_string).collect()
    }

    /// Inverse of [`escape`], for checking round trips.
    fn unescape(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    }

    #[test]
    fn timed_event_has_frame_and_utc_times() {
        let ics = event_ics(&event(), now());
        assert!(ics.ends_with("\r\n"));
        assert!(!ics.replace("\r\n", "").contains('\n'), "bare LF in output");
        let lines = unfolded_lines(&ics);
        assert_eq!(
            lines,
            [
                "BEGIN:VCALENDAR",
                "VERSION:2.0",
                "PRODID:-//Torro//Workplace//DE",
                "BEGIN:VEVENT",
                "UID:evt-1@torro",
                "DTSTAMP:20261011T120000Z",
                "DTSTART:20261011T080000Z",
                "DTEND:20261011T090000Z",
                "SUMMARY:Planung",
                "END:VEVENT",
                "END:VCALENDAR",
            ]
        );
    }

    #[test]
    fn all_day_event_uses_value_date_and_optional_fields() {
        let mut event = event();
        event.start = EventTime::Date(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap());
        event.end = Some(EventTime::Date(NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()));
        event.location = Some("Büro, Raum 3".to_string());
        event.description = Some(String::new());
        let lines = unfolded_lines(&event_ics(&event, now()));
        assert!(lines.contains(&"DTSTART;VALUE=DATE:20261011".to_string()));
        assert!(lines.contains(&"DTEND;VALUE=DATE:20261012".to_string()));
        assert!(lines.contains(&"LOCATION:Büro\\, Raum 3".to_string()));
        assert!(!lines.iter().any(|l| l.starts_with("DESCRIPTION")), "empty description is left out");
    }

    #[test]
    fn completed_todo_with_parent_priority_and_due() {
        let mut todo = todo();
        todo.description = Some("Entwurf\nfertig".to_string());
        let lines = unfolded_lines(&todo_ics(&todo, now()));
        assert_eq!(lines[0], "BEGIN:VCALENDAR");
        assert!(lines.contains(&"BEGIN:VTODO".to_string()));
        assert!(lines.contains(&"END:VTODO".to_string()));
        assert!(lines.contains(&"UID:todo-2@torro".to_string()));
        assert!(lines.contains(&"DTSTAMP:20261011T120000Z".to_string()));
        assert!(lines.contains(&"DUE:20261012T170000Z".to_string()));
        assert!(lines.contains(&"PRIORITY:1".to_string()));
        assert!(lines.contains(&"STATUS:COMPLETED".to_string()));
        assert!(lines.contains(&"COMPLETED:20261011T120000Z".to_string()));
        assert!(lines.contains(&"RELATED-TO;RELTYPE=PARENT:todo-1@torro".to_string()));
        assert!(lines.contains(&"DESCRIPTION:Entwurf\\nfertig".to_string()));
        assert!(!lines.iter().any(|l| l.starts_with("STATUS:NEEDS")));
    }

    #[test]
    fn open_todo_needs_action_and_date_due() {
        let mut todo = todo();
        todo.completed = false;
        todo.priority = None;
        todo.parent = None;
        todo.due = Some(EventTime::Date(NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()));
        let lines = unfolded_lines(&todo_ics(&todo, now()));
        assert!(lines.contains(&"DUE;VALUE=DATE:20261012".to_string()));
        assert!(lines.contains(&"STATUS:NEEDS-ACTION".to_string()));
        assert!(!lines.iter().any(|l| l.starts_with("COMPLETED") || l.starts_with("PRIORITY")));
        assert!(!lines.iter().any(|l| l.starts_with("RELATED-TO")));
    }

    #[test]
    fn text_values_are_escaped() {
        let mut event = event();
        event.summary = "Essen, Trinken; Pause\\ und\nZeile".to_string();
        let lines = unfolded_lines(&event_ics(&event, now()));
        assert!(lines.contains(&"SUMMARY:Essen\\, Trinken\\; Pause\\\\ und\\nZeile".to_string()));
    }

    #[test]
    fn long_description_folds_at_75_octets_and_unfolds_to_the_text() {
        let text = "Größe und Übung für Äpfel, Bäume; Wälder\nZweite Zeile mit Umlauten ÖÜ ".repeat(6);
        let mut event = event();
        event.description = Some(text.clone());
        let ics = event_ics(&event, now());

        for line in ics.split("\r\n") {
            assert!(line.len() <= MAX_LINE_OCTETS, "line has {} octets: {line}", line.len());
        }
        assert!(ics.contains("\r\n "), "long line must be folded");
        assert!(ics.split("\r\n").any(|l| l.len() == MAX_LINE_OCTETS), "folds fill the 75 octets");

        let description = unfolded_lines(&ics)
            .into_iter()
            .find_map(|l| l.strip_prefix("DESCRIPTION:").map(str::to_string))
            .expect("description line");
        assert_eq!(unescape(&description), text);
    }

    #[test]
    fn multibyte_summary_is_folded_between_characters() {
        let mut event = event();
        event.summary = "ÄÖÜ".repeat(50);
        let ics = event_ics(&event, now());
        for line in ics.split("\r\n") {
            assert!(line.len() <= MAX_LINE_OCTETS, "line has {} octets", line.len());
        }
        // "SUMMARY:" takes 8 octets, then 33 umlauts (66 octets) make 74; the next one would
        // reach 76, so the line folds there.
        let first = ics.split("\r\n").find(|l| l.starts_with("SUMMARY:")).expect("summary line");
        assert_eq!(first.len(), 74);
        let summary = unfolded_lines(&ics)
            .into_iter()
            .find_map(|l| l.strip_prefix("SUMMARY:").map(str::to_string))
            .expect("summary line");
        assert_eq!(summary, "ÄÖÜ".repeat(50));
    }

    // Writing onto a stored object (task P.1): what the model does not know stays.

    /// A task as Nextcloud Tasks writes it: sort order, categories, URL, a sibling link, an
    /// attachment, a reminder, a folded description and an over-long unfolded line.
    const NEXTCLOUD_TODO: &str = include_str!("../tests/fixtures/nextcloud-tasks-todo.ics");
    /// An event as Thunderbird writes it: time zone, attendees, `X-MOZ-*` and a reminder.
    const THUNDERBIRD_EVENT: &str = include_str!("../tests/fixtures/thunderbird-event.ics");

    /// The moment of the write in these tests, and its form in the output.
    fn later() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 12, 9, 30, 0).unwrap()
    }
    const LATER: &str = "20261012T093000Z";

    /// The task in `ics`, as the core reads it.
    fn read_todo(ics: &str) -> Todo {
        ical::parse_todos(ics, COLLECTION).into_iter().next().expect("a task")
    }

    /// The event in `ics`, as the core reads it.
    fn read_event(ics: &str) -> Event {
        ical::parse_events(ics, COLLECTION).into_iter().next().expect("an event")
    }

    /// `written` is `stored` with exactly the lines `replaced` taken out and `new` added: every
    /// other line is still there, unchanged and in its order.
    fn assert_rewritten(stored: &str, written: &str, replaced: &[&str], new: &[String]) {
        let stored = unfolded_lines(&stored.replace("\r\n", "\n").replace('\n', "\r\n"));
        let written = unfolded_lines(written);
        for line in replaced {
            assert!(stored.iter().any(|l| l == line), "the fixture has no line {line:?}");
        }
        for line in new {
            assert!(written.contains(line), "{line:?} is missing in:\n{}", written.join("\n"));
        }
        let kept: Vec<&String> = stored.iter().filter(|l| !replaced.contains(&l.as_str())).collect();
        let rest: Vec<&String> = written.iter().filter(|l| !new.contains(l)).collect();
        assert_eq!(rest, kept, "everything else stays as it was");
    }

    fn stamps() -> Vec<String> {
        vec![format!("DTSTAMP:{LATER}"), format!("LAST-MODIFIED:{LATER}")]
    }

    fn with_stamps(lines: &[&str]) -> Vec<String> {
        stamps().into_iter().chain(lines.iter().map(|l| l.to_string())).collect()
    }

    const TODO_STAMPS: [&str; 2] = ["LAST-MODIFIED:20261005T091500Z", "DTSTAMP:20261005T091500Z"];
    const EVENT_STAMPS: [&str; 2] = ["LAST-MODIFIED:20261002T101500Z", "DTSTAMP:20261002T101500Z"];

    #[test]
    fn an_unchanged_task_keeps_every_line_but_its_time_stamps() {
        let todo = read_todo(NEXTCLOUD_TODO);
        let written = todo_onto(NEXTCLOUD_TODO, &todo, later());
        assert_rewritten(NEXTCLOUD_TODO, &written, &TODO_STAMPS, &stamps());
    }

    #[test]
    fn a_nextcloud_task_keeps_sort_order_categories_and_reminder_when_done() {
        let mut todo = read_todo(NEXTCLOUD_TODO);
        todo.summary = "Steuererklärung abschicken".into();
        todo.completed = true;
        let written = todo_onto(NEXTCLOUD_TODO, &todo, later());

        let replaced = [
            TODO_STAMPS[0],
            TODO_STAMPS[1],
            "SUMMARY:Steuererklärung abgeben",
            "STATUS:IN-PROCESS",
            "PERCENT-COMPLETE:40",
        ];
        let completed = format!("COMPLETED:{LATER}");
        let new = with_stamps(&[
            "SUMMARY:Steuererklärung abschicken",
            "STATUS:COMPLETED",
            &completed,
            "PERCENT-COMPLETE:100",
        ]);
        assert_rewritten(NEXTCLOUD_TODO, &written, &replaced, &new);
        // Spot checks of what the helper already covers, so a reader sees it.
        let lines = unfolded_lines(&written);
        for kept in ["X-APPLE-SORT-ORDER:2", "CATEGORIES:Finanzen,Privat", "BEGIN:VALARM", "TRIGGER;RELATED=END:-P1D"] {
            assert!(lines.contains(&kept.to_string()), "{kept}");
        }
        assert_eq!(read_todo(&written), todo, "the written task reads back as saved");
    }

    #[test]
    fn cleared_fields_are_gone_and_only_the_parent_link_is_dropped() {
        let mut todo = read_todo(NEXTCLOUD_TODO);
        todo.description = Some(String::new());
        todo.priority = None;
        todo.due = None;
        todo.parent = None;
        let written = todo_onto(NEXTCLOUD_TODO, &todo, later());
        let replaced = [
            TODO_STAMPS[0],
            TODO_STAMPS[1],
            "DESCRIPTION:Belege aus dem Ordner „Steuer 2025“ zusammensuchen\\, Anlage N ausfüllen und alles bis Ende Oktober abschicken.",
            "PRIORITY:5",
            "DUE;VALUE=DATE:20261031",
            "RELATED-TO;RELTYPE=PARENT:b7e1d2c3-0000-4a5b-8c9d-aaaaaaaaaaaa",
        ];
        // The reminder's own DESCRIPTION and the sibling link stay (checked by the helper).
        assert_rewritten(NEXTCLOUD_TODO, &written, &replaced, &stamps());
        let back = read_todo(&written);
        assert_eq!((back.description, back.priority, back.due, back.parent), (None, None, None, None));
    }

    /// A link with details the core fetched, which never go into the stored task.
    fn issue_link() -> Link {
        let details = GitHubDetails {
            kind: GitHubKind::Issue,
            state: GitHubState::Open,
            labels: vec![GitHubLabel { name: "bug".into(), color: Some("#D73A4A".into()) }],
            assignees: vec!["mahype".into()],
            milestone: Some("1.0".into()),
            milestone_due: None,
            draft: false,
            review: None,
            checks: None,
            conflict: false,
        };
        Link { github: Some(details), ..Link::github("mahype/TorroWorkplace", 123) }
    }

    #[test]
    fn a_new_task_writes_url_categories_sort_order_and_only_the_link_id() {
        let todo = Todo {
            completed: false,
            url: Some("https://github.com/mahype/TorroWorkplace/issues/123".into()),
            categories: vec!["Work".into(), " Steuer, 2025 ".into(), String::new()],
            sort_order: Some(7),
            link: Some(issue_link()),
            ..todo()
        };
        let ics = todo_ics(&todo, now());
        let lines = unfolded_lines(&ics);
        for line in [
            "URL:https://github.com/mahype/TorroWorkplace/issues/123",
            "CATEGORIES:Work,Steuer\\, 2025",
            "X-APPLE-SORT-ORDER:7",
            "X-TORRO-LINK:github:mahype/TorroWorkplace#123",
        ] {
            assert!(lines.contains(&line.to_string()), "{line} missing in {lines:?}");
        }
        assert!(!ics.contains("bug") && !ics.contains("1.0"), "fetched details are not stored: {ics}");
        let back = read_todo(&ics);
        assert_eq!(back.categories, ["Work", "Steuer, 2025"]);
        assert_eq!((back.url, back.sort_order), (todo.url.clone(), Some(7)));
        assert_eq!(back.link, Some(Link::github("mahype/TorroWorkplace", 123)));

        let bare = unfolded_lines(&todo_ics(&todo_without_extras(), now()));
        let extras = ["URL", "CATEGORIES", "X-APPLE-SORT-ORDER", "X-TORRO-LINK"];
        assert!(!bare.iter().any(|l| extras.iter().any(|e| l.starts_with(e))), "{bare:?}");
    }

    fn todo_without_extras() -> Todo {
        Todo { url: Some(" ".into()), link: Some(Link { id: " ".into(), github: None }), ..todo() }
    }

    #[test]
    fn changed_url_categories_and_sort_order_replace_only_their_lines() {
        let mut todo = read_todo(NEXTCLOUD_TODO);
        assert_eq!(todo.categories, ["Finanzen", "Privat"]);
        assert_eq!(todo.sort_order, Some(2));
        todo.url = Some("https://www.elster.de/eportal/formulare".into());
        todo.categories = vec!["Finanzen".into(), "Dringend".into()];
        todo.sort_order = Some(-1);
        todo.link = Some(issue_link());
        let written = todo_onto(NEXTCLOUD_TODO, &todo, later());
        let replaced = [
            TODO_STAMPS[0],
            TODO_STAMPS[1],
            "URL:https://www.elster.de/eportal/start",
            "CATEGORIES:Finanzen,Privat",
            "X-APPLE-SORT-ORDER:2",
        ];
        let new = with_stamps(&[
            "URL:https://www.elster.de/eportal/formulare",
            "CATEGORIES:Finanzen,Dringend",
            "X-APPLE-SORT-ORDER:-1",
            "X-TORRO-LINK:github:mahype/TorroWorkplace#123",
        ]);
        assert_rewritten(NEXTCLOUD_TODO, &written, &replaced, &new);
        let back = read_todo(&written);
        assert_eq!(back, Todo { link: Some(Link::github("mahype/TorroWorkplace", 123)), ..todo });
    }

    #[test]
    fn cleared_url_categories_sort_order_and_link_are_gone() {
        let stored = NEXTCLOUD_TODO.replace("CLASS:PRIVATE", "X-TORRO-LINK:github:mahype/TorroWorkplace#123");
        let mut todo = read_todo(&stored);
        assert!(todo.link.is_some());
        todo.url = None;
        todo.categories = Vec::new();
        todo.sort_order = None;
        todo.link = None;
        let written = todo_onto(&stored, &todo, later());
        let replaced = [
            TODO_STAMPS[0],
            TODO_STAMPS[1],
            "URL:https://www.elster.de/eportal/start",
            "CATEGORIES:Finanzen,Privat",
            "X-APPLE-SORT-ORDER:2",
            "X-TORRO-LINK:github:mahype/TorroWorkplace#123",
        ];
        assert_rewritten(&stored, &written, &replaced, &stamps());
        assert_eq!(read_todo(&written), todo);
    }

    #[test]
    fn unchanged_tags_keep_their_lines_and_fetched_details_change_nothing() {
        // Two CATEGORIES lines, as some clients write them; the link as stored.
        let stored = NEXTCLOUD_TODO
            .replace("CATEGORIES:Finanzen,Privat", "CATEGORIES:Finanzen\nCATEGORIES:Privat")
            .replace("CLASS:PRIVATE", "X-TORRO-LINK:github:mahype/TorroWorkplace#123");
        let mut todo = read_todo(&stored);
        assert_eq!(todo.categories, ["Finanzen", "Privat"]);
        // What the core fetched rides along; the stored task does not change for it.
        todo.link = Some(issue_link());
        todo.categories = vec![" Finanzen".into(), "Privat".into()];
        let written = todo_onto(&stored, &todo, later());
        assert_rewritten(&stored, &written, &TODO_STAMPS, &stamps());
    }

    #[test]
    fn a_url_cannot_add_properties() {
        let todo = Todo { url: Some("https://example.org/a\r\nX-EVIL:1\nb".into()), ..todo() };
        let lines = unfolded_lines(&todo_ics(&todo, now()));
        assert!(lines.contains(&"URL:https://example.org/aX-EVIL:1b".to_string()), "{lines:?}");
        assert!(!lines.iter().any(|l| l.starts_with("X-EVIL")));
    }

    #[test]
    fn a_done_task_keeps_its_completion_time_and_reopening_clears_it() {
        let done = NEXTCLOUD_TODO.replace(
            "STATUS:IN-PROCESS\nPERCENT-COMPLETE:40",
            "STATUS:COMPLETED\nCOMPLETED:20261006T120000Z\nPERCENT-COMPLETE:100",
        );
        let mut todo = read_todo(&done);
        assert!(todo.completed);

        todo.summary = "Erledigt".into();
        let renamed = todo_onto(&done, &todo, later());
        let replaced = [TODO_STAMPS[0], TODO_STAMPS[1], "SUMMARY:Steuererklärung abgeben"];
        assert_rewritten(&done, &renamed, &replaced, &with_stamps(&["SUMMARY:Erledigt"]));

        todo.summary = "Steuererklärung abgeben".into();
        todo.completed = false;
        let reopened = todo_onto(&done, &todo, later());
        let replaced =
            [TODO_STAMPS[0], TODO_STAMPS[1], "STATUS:COMPLETED", "COMPLETED:20261006T120000Z", "PERCENT-COMPLETE:100"];
        assert_rewritten(&done, &reopened, &replaced, &with_stamps(&["STATUS:NEEDS-ACTION"]));
        assert!(!read_todo(&reopened).completed);
    }

    #[test]
    fn a_thunderbird_event_keeps_time_zone_attendees_and_reminder() {
        let mut event = read_event(THUNDERBIRD_EVENT);
        event.location = Some("Raum 4".into());
        event.description = Some("Neue Agenda".into());
        let written = event_onto(THUNDERBIRD_EVENT, &event, later());
        let replaced = [
            EVENT_STAMPS[0],
            EVENT_STAMPS[1],
            "LOCATION:Büro\\, Raum 3",
            "DESCRIPTION:Agenda: Stand der Kacheln\\, Kalender und Aufgaben\\; danach Fragen.",
        ];
        let new = with_stamps(&["LOCATION:Raum 4", "DESCRIPTION:Neue Agenda"]);
        assert_rewritten(THUNDERBIRD_EVENT, &written, &replaced, &new);
        let lines = unfolded_lines(&written);
        for kept in
            ["DTSTART;TZID=Europe/Berlin:20261013T100000", "BEGIN:VTIMEZONE", "X-MOZ-GENERATION:3", "SEQUENCE:2"]
        {
            assert!(lines.contains(&kept.to_string()), "{kept}");
        }
        assert_eq!(read_event(&written), event);
    }

    #[test]
    fn a_moved_event_gets_utc_times_and_keeps_the_rest() {
        let mut event = read_event(THUNDERBIRD_EVENT);
        event.start = EventTime::DateTime(at(14, 8));
        event.end = Some(EventTime::DateTime(at(14, 9)));
        let written = event_onto(THUNDERBIRD_EVENT, &event, later());
        let replaced = [
            EVENT_STAMPS[0],
            EVENT_STAMPS[1],
            "DTSTART;TZID=Europe/Berlin:20261013T100000",
            "DTEND;TZID=Europe/Berlin:20261013T113000",
        ];
        let new = with_stamps(&["DTSTART:20261014T080000Z", "DTEND:20261014T090000Z"]);
        assert_rewritten(THUNDERBIRD_EVENT, &written, &replaced, &new);
    }

    #[test]
    fn a_duration_becomes_dtend_when_start_or_end_moves() {
        let stored = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:d\r\nDTSTART:20261011T080000Z\r\nDURATION:PT1H\r\n\
SUMMARY:Kurz\r\nX-FOO:bar\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        // The end moves: DTSTART stays as stored, the duration is replaced.
        let mut event = read_event(stored);
        event.end = Some(EventTime::DateTime(at(11, 10)));
        let written = event_onto(stored, &event, later());
        let new = with_stamps(&["DTEND:20261011T100000Z"]);
        assert_rewritten(stored, &written, &["DURATION:PT1H"], &new);

        // Only the start moves: a kept DURATION would move the end too, so DTEND is written.
        let mut event = read_event(stored);
        event.start = EventTime::DateTime(at(11, 7));
        let written = event_onto(stored, &event, later());
        let new = with_stamps(&["DTSTART:20261011T070000Z", "DTEND:20261011T090000Z"]);
        assert_rewritten(stored, &written, &["DTSTART:20261011T080000Z", "DURATION:PT1H"], &new);
        assert_eq!(read_event(&written), event);
    }

    #[test]
    fn written_objects_are_folded_at_75_octets_with_crlf() {
        let mut todo = read_todo(NEXTCLOUD_TODO);
        todo.description = Some("Größe und Übung für Äpfel, Bäume; Wälder ".repeat(5));
        let written = todo_onto(NEXTCLOUD_TODO, &todo, later());
        assert!(written.ends_with("\r\n"));
        assert!(!written.replace("\r\n", "").contains('\n'), "bare LF in output");
        for line in written.split("\r\n") {
            assert!(line.len() <= MAX_LINE_OCTETS, "line has {} octets: {line}", line.len());
        }
        // The server sent the attachment unfolded and too long; it is folded now and the same.
        let attach = "ATTACH;FMTTYPE=application/pdf:https://cloud.example.org/remote.php/dav/files/anna/Steuer/Bescheid-2025.pdf";
        assert!(attach.len() > MAX_LINE_OCTETS);
        assert!(written.split("\r\n").any(|l| l.starts_with("ATTACH;") && l.len() <= MAX_LINE_OCTETS));
        assert!(unfolded_lines(&written).contains(&attach.to_string()));
        assert_eq!(read_todo(&written).description, todo.description);
        // CRLF and bare LF from the server give the same result.
        let crlf = NEXTCLOUD_TODO.replace('\n', "\r\n");
        assert_eq!(todo_onto(&crlf, &todo, later()), written);
    }

    #[test]
    fn the_component_with_the_uid_is_written_and_others_stay() {
        let stored = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:a\r\nSUMMARY:Erste\r\nEND:VTODO\r\n\
BEGIN:VTODO\r\nUID:b\r\nSUMMARY:Zweite\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let mut todo = ical::parse_todos(stored, COLLECTION).into_iter().nth(1).expect("second task");
        todo.summary = "Zweite, neu".into();
        let written = todo_onto(stored, &todo, later());
        // `b` gets its stamps and the new summary; `a` stays as it was (checked by the helper).
        assert_rewritten(stored, &written, &["SUMMARY:Zweite"], &with_stamps(&["SUMMARY:Zweite\\, neu"]));
        let lines = unfolded_lines(&written);
        let first = lines.iter().position(|l| l == "UID:a").expect("a");
        let second = lines.iter().position(|l| l == "UID:b").expect("b");
        let summary = lines.iter().position(|l| l == "SUMMARY:Zweite\\, neu").expect("new summary");
        assert!(first < second && second.abs_diff(summary) < 6, "the new summary sits in `b`: {lines:?}");
    }

    #[test]
    fn without_a_matching_component_the_object_is_written_anew() {
        let todo = todo();
        assert_eq!(todo_onto(THUNDERBIRD_EVENT, &todo, later()), todo_ics(&todo, later()));
        let event = event();
        assert_eq!(event_onto(NEXTCLOUD_TODO, &event, later()), event_ics(&event, later()));
    }

    #[test]
    fn an_unreadable_stored_task_is_written_in_full_but_keeps_foreign_lines() {
        // A cancelled task does not read as a task, so every field is written.
        let stored = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:x\r\nSUMMARY:Alt\r\nSTATUS:CANCELLED\r\nX-FOO:bar\r\n\
END:VTODO\r\nEND:VCALENDAR\r\n";
        let todo = Todo { uid: "x".into(), summary: "Neu".into(), ..todo() };
        let written = todo_onto(stored, &todo, later());
        let lines = unfolded_lines(&written);
        assert!(lines.contains(&"X-FOO:bar".to_string()));
        assert!(lines.contains(&"SUMMARY:Neu".to_string()));
        assert!(!lines.iter().any(|l| l == "SUMMARY:Alt" || l == "STATUS:CANCELLED"), "{lines:?}");
        assert_eq!(lines.iter().filter(|l| l.starts_with("UID:")).count(), 1);
    }

    #[test]
    fn new_uid_has_torro_suffix() {
        let uid = new_uid();
        assert!(uid.ends_with("@torro"), "{uid}");
        assert!(uid.len() > "@torro".len());
    }

    #[test]
    fn new_item_url_ends_with_slash_and_uid() {
        let expected = "https://cloud.example.org/remote.php/dav/calendars/anna/personal/evt-1@torro.ics";
        let with_slash = new_item_url(COLLECTION, "evt-1@torro").expect("url");
        let without_slash =
            new_item_url("https://cloud.example.org/remote.php/dav/calendars/anna/personal", "evt-1@torro")
                .expect("url");
        assert_eq!(with_slash.as_str(), expected);
        assert_eq!(without_slash.as_str(), expected);
    }

    #[test]
    fn an_item_directly_in_the_collection_resolves() {
        let expected = "https://cloud.example.org/remote.php/dav/calendars/anna/personal/evt-1.ics";
        for href in [
            "/remote.php/dav/calendars/anna/personal/evt-1.ics",
            "evt-1.ics",
            " https://cloud.example.org/remote.php/dav/calendars/anna/personal/evt-1.ics ",
            "https://cloud.example.org:443/remote.php/dav/calendars/anna/personal/evt-1.ics",
        ] {
            assert_eq!(item_url(COLLECTION, href).map(String::from).as_deref(), Some(expected), "{href}");
        }
        let without_slash = "https://cloud.example.org/remote.php/dav/calendars/anna/personal";
        assert_eq!(item_url(without_slash, "evt-1.ics").map(String::from).as_deref(), Some(expected));
        assert!(resolve(COLLECTION, "evt-1.ics").is_ok());
    }

    #[test]
    fn hrefs_outside_the_collection_are_refused() {
        for href in [
            // Up and into a sibling calendar, plain and percent-encoded.
            "/remote.php/dav/calendars/anna/personal/../work/evt-1.ics",
            "/remote.php/dav/calendars/anna/personal/%2e%2e/work/evt-1.ics",
            "/remote.php/dav/calendars/anna/personal/%2E%2E/work/evt-1.ics",
            "/remote.php/dav/calendars/anna/personal/.%2e/work/evt-1.ics",
            "../work/evt-1.ics",
            "../../../files/anna/notes.txt",
            // Backslashes count as slashes in http URLs; encoded ones must not slip through.
            "/remote.php/dav/calendars/anna/personal/..\\work\\evt-1.ics",
            "\\remote.php\\dav\\files\\anna\\notes.txt",
            "evt%2F1.ics",
            "evt%5c1.ics",
            // Doubled slashes and children of children.
            "/remote.php/dav/calendars/anna/personal//evt-1.ics",
            "/remote.php/dav/calendars/anna/personal/sub/evt-1.ics",
            // Other hosts, ports, schemes, users.
            "//evil.example.org/remote.php/dav/calendars/anna/personal/evt-1.ics",
            "https://evil.example.org/remote.php/dav/calendars/anna/personal/evt-1.ics",
            "https://cloud.example.org:8443/remote.php/dav/calendars/anna/personal/evt-1.ics",
            "http://cloud.example.org/remote.php/dav/calendars/anna/personal/evt-1.ics",
            "https://bob@cloud.example.org/remote.php/dav/calendars/anna/personal/evt-1.ics",
            // Absolute paths elsewhere, the collection itself, queries and fragments.
            "/remote.php/dav/files/anna/notes.txt",
            "/remote.php/dav/calendars/anna/personal/",
            "/remote.php/dav/calendars/anna/personal",
            "/remote.php/dav/calendars/anna/personal/.",
            "/remote.php/dav/calendars/anna/personal/%2e",
            "evt-1.ics?x=1",
            "evt-1.ics#x",
            "",
        ] {
            assert_eq!(item_url(COLLECTION, href), None, "{href:?}");
            assert!(resolve(COLLECTION, href).is_err(), "{href:?}");
        }
        assert_eq!(item_url("not a url", "evt-1.ics"), None);
    }

    #[test]
    fn etag_is_read_from_the_200_propstat() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response><d:href>/remote.php/dav/calendars/anna/personal/evt-1@torro.ics</d:href>
    <d:propstat><d:prop><d:getetag>"abc123"</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
</d:multistatus>"#;
        assert_eq!(etag_from_multistatus(xml).as_deref(), Some("\"abc123\""));

        let missing = r#"<d:multistatus xmlns:d="DAV:"><d:response>
    <d:propstat><d:prop/><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
  </d:response></d:multistatus>"#;
        assert_eq!(etag_from_multistatus(missing), None);
        assert_eq!(etag_from_multistatus("not xml"), None);
    }

    #[test]
    fn new_items_must_not_overwrite_and_existing_ones_need_their_etag() {
        assert_eq!(precondition(None, None), Ok(("If-None-Match", "*")));
        assert_eq!(precondition(None, Some("\"1\"")), Ok(("If-None-Match", "*")), "a new item never replaces one");
        assert_eq!(precondition(Some("evt-1.ics"), Some("\"1\"")), Ok(("If-Match", "\"1\"")));
        for etag in [None, Some(""), Some("  ")] {
            let refused = precondition(Some("evt-1.ics"), etag);
            assert!(
                matches!(&refused, Err(ProviderError::Other(message)) if message.contains("reload the item and pass its etag")),
                "{etag:?}: {refused:?}"
            );
        }
    }

    #[test]
    fn statuses_map_to_provider_errors() {
        let url = Url::parse(COLLECTION).expect("url");
        assert_eq!(
            status_error(StatusCode::PRECONDITION_FAILED, "evt-1@torro", &url),
            ProviderError::Conflict { uid: "evt-1@torro".to_string() }
        );
        assert_eq!(status_error(StatusCode::UNAUTHORIZED, "x", &url), ProviderError::Auth);
        assert_eq!(status_error(StatusCode::FORBIDDEN, "x", &url), ProviderError::Auth);
        assert!(matches!(status_error(StatusCode::INTERNAL_SERVER_ERROR, "x", &url), ProviderError::Other(_)));
    }
}
