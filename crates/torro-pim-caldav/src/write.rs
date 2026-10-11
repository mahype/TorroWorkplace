//! Writing events and tasks (task K.4 in `docs/tasks/kalender-aufgaben.md`).
//!
//! [`to_ics_event`] / [`to_ics_todo`] produce a complete `VCALENDAR` (RFC 5545: CRLF line
//! ends, lines folded at 75 octets, text escaped, `DTSTAMP`, `PRODID:-//Torro//Workplace//DE`,
//! times in UTC with `Z`, dates as `VALUE=DATE`). The `CalDavClient` methods PUT them to
//! `href` (new items: `<collection>/<uid>.ics`, with `If-None-Match: *`; updates with
//! `If-Match: <etag>`) and DELETE with `If-Match`. A 412 answer becomes
//! `ProviderError::Conflict`. The new ETag comes from the PUT answer's `ETag` header; when
//! the server sends none, it is fetched with a PROPFIND `getetag` on the href.

use chrono::{DateTime, NaiveDate, Utc};
use reqwest::{StatusCode, Url};
use roxmltree::Node;
use torro_pim::{Event, EventTime, ProviderError, Todo};

use crate::CalDavClient;

const NS_DAV: &str = "DAV:";
const PRODID: &str = "-//Torro//Workplace//DE";
/// RFC 5545 section 3.1: a content line is at most 75 octets, longer ones are folded.
/// The leading space of a continuation line counts towards the 75.
const MAX_LINE_OCTETS: usize = 75;

/// The event as a `VCALENDAR` text.
pub fn to_ics_event(event: &Event) -> String {
    event_ics(event, Utc::now())
}

/// The task as a `VCALENDAR` text.
pub fn to_ics_todo(todo: &Todo) -> String {
    todo_ics(todo, Utc::now())
}

impl CalDavClient {
    /// Creates (`href: None`; a new uid when empty) or updates the event; returns it with
    /// `href` and `etag` set.
    pub async fn save_event(&self, mut event: Event) -> Result<Event, ProviderError> {
        if event.uid.is_empty() {
            event.uid = new_uid();
        }
        let ics = to_ics_event(&event);
        let (href, etag) =
            self.put_item(&event.collection, &event.uid, event.href.as_deref(), event.etag.as_deref(), ics).await?;
        event.href = Some(href);
        event.etag = Some(etag);
        Ok(event)
    }

    /// Deletes the event if its ETag still matches. An event that was never saved has
    /// nothing on the server to delete and counts as done.
    pub async fn delete_event(&self, event: &Event) -> Result<(), ProviderError> {
        self.delete_item(&event.collection, &event.uid, event.href.as_deref(), event.etag.as_deref()).await
    }

    /// Creates (`href: None`; a new uid when empty) or updates the task; returns it with
    /// `href` and `etag` set.
    pub async fn save_todo(&self, mut todo: Todo) -> Result<Todo, ProviderError> {
        if todo.uid.is_empty() {
            todo.uid = new_uid();
        }
        let ics = to_ics_todo(&todo);
        let (href, etag) =
            self.put_item(&todo.collection, &todo.uid, todo.href.as_deref(), todo.etag.as_deref(), ics).await?;
        todo.href = Some(href);
        todo.etag = Some(etag);
        Ok(todo)
    }

    /// Deletes the task if its ETag still matches. A task that was never saved has nothing
    /// on the server to delete and counts as done.
    pub async fn delete_todo(&self, todo: &Todo) -> Result<(), ProviderError> {
        self.delete_item(&todo.collection, &todo.uid, todo.href.as_deref(), todo.etag.as_deref()).await
    }

    /// PUTs one iCalendar object and returns the `(href, etag)` the item now has. `href`
    /// `None` means a new item, which must not overwrite anything (`If-None-Match: *`).
    async fn put_item(
        &self,
        collection: &str,
        uid: &str,
        href: Option<&str>,
        etag: Option<&str>,
        ics: String,
    ) -> Result<(String, String), ProviderError> {
        let existing = href.is_some();
        let (url, href) = match href {
            Some(href) => (resolve(collection, href)?, href.to_string()),
            None => {
                let url = new_item_url(collection, uid)?;
                (url.clone(), url.to_string())
            }
        };
        let mut request = self
            .http
            .put(url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .header("Content-Type", "text/calendar; charset=utf-8")
            .body(ics);
        request = match (existing, etag) {
            (false, _) => request.header("If-None-Match", "*"),
            (true, Some(etag)) => request.header("If-Match", etag),
            (true, None) => request,
        };
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status, uid, &url));
        }
        let header_etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let etag = match header_etag {
            Some(etag) => etag,
            None => self.fetch_etag(&url).await?,
        };
        Ok((href, etag))
    }

    /// DELETEs one item. `href` `None` (never saved) is done without a request.
    async fn delete_item(
        &self,
        collection: &str,
        uid: &str,
        href: Option<&str>,
        etag: Option<&str>,
    ) -> Result<(), ProviderError> {
        let Some(href) = href else { return Ok(()) };
        let url = resolve(collection, href)?;
        let mut request = self.http.delete(url.clone()).basic_auth(&self.username, Some(&self.password));
        if let Some(etag) = etag {
            request = request.header("If-Match", etag);
        }
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        // 404: already gone, which is what the caller wanted.
        if status.is_success() || status == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(status_error(status, uid, &url))
        }
    }

    /// The ETag of one item, for servers that do not send it with the PUT answer.
    async fn fetch_etag(&self, url: &Url) -> Result<String, ProviderError> {
        let body = r#"<d:propfind xmlns:d="DAV:"><d:prop><d:getetag/></d:prop></d:propfind>"#;
        let xml = self.request("PROPFIND", url, "0", body).await?;
        etag_from_multistatus(&xml).ok_or_else(|| ProviderError::Other(format!("{url}: server sent no ETag")))
    }
}

fn event_ics(event: &Event, now: DateTime<Utc>) -> String {
    let mut body =
        vec![text_line("UID", &event.uid), format!("DTSTAMP:{}", utc(now)), time_line("DTSTART", event.start)];
    if let Some(end) = event.end {
        body.push(time_line("DTEND", end));
    }
    body.push(text_line("SUMMARY", &event.summary));
    push_optional_text(&mut body, "LOCATION", event.location.as_deref());
    push_optional_text(&mut body, "DESCRIPTION", event.description.as_deref());
    calendar("VEVENT", &body)
}

fn todo_ics(todo: &Todo, now: DateTime<Utc>) -> String {
    let mut body =
        vec![text_line("UID", &todo.uid), format!("DTSTAMP:{}", utc(now)), text_line("SUMMARY", &todo.summary)];
    if let Some(due) = todo.due {
        body.push(time_line("DUE", due));
    }
    // RFC 5545: 1 is the highest priority, 9 the lowest, 0 means undefined.
    if let Some(priority) = todo.priority.filter(|p| (1..=9).contains(p)) {
        body.push(format!("PRIORITY:{priority}"));
    }
    if todo.completed {
        body.push("STATUS:COMPLETED".to_string());
        body.push(format!("COMPLETED:{}", utc(now)));
    } else {
        body.push("STATUS:NEEDS-ACTION".to_string());
    }
    if let Some(parent) = &todo.parent {
        body.push(format!("RELATED-TO;RELTYPE=PARENT:{}", escape(parent)));
    }
    push_optional_text(&mut body, "DESCRIPTION", todo.description.as_deref());
    calendar("VTODO", &body)
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

fn push_optional_text(body: &mut Vec<String>, name: &str, value: Option<&str>) {
    // An empty value is treated as absent: the PUT replaces the whole object, so leaving the
    // line out is how a cleared field is stored.
    if let Some(value) = value.filter(|v| !v.is_empty()) {
        body.push(text_line(name, value));
    }
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
