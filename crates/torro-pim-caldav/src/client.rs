//! Talking CalDAV (RFC 4791): discovery, calendars, events in a time range.

use chrono::{DateTime, Utc};
use reqwest::{Method, StatusCode, Url};
use torro_pim::{Collection, CollectionKind, Event, ProviderError, Todo};

use crate::{CalDavAccount, http_error, ical};

const NS_DAV: &str = "DAV:";
const NS_CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const NS_APPLE: &str = "http://apple.com/ns/ical/";

/// A signed-in connection to one CalDAV account.
#[derive(Debug, Clone)]
pub struct CalDavClient {
    pub(crate) http: reqwest::Client,
    pub(crate) base: Url,
    pub(crate) username: String,
    pub(crate) password: String,
}

impl CalDavClient {
    pub fn new(http: reqwest::Client, account: &CalDavAccount, password: String) -> Result<Self, ProviderError> {
        let base = Url::parse(&account.url).map_err(|err| ProviderError::Other(format!("{}: {err}", account.url)))?;
        Ok(CalDavClient { http, base, username: account.username.clone(), password })
    }

    /// The calendars and task lists of the account.
    pub async fn collections(&self) -> Result<Vec<Collection>, ProviderError> {
        let principal =
            self.find_href(&self.base, "<d:current-user-principal/>", NS_DAV, "current-user-principal").await?;
        let home = self.find_href(&principal, "<c:calendar-home-set/>", NS_CALDAV, "calendar-home-set").await?;
        let body = r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/">
  <d:prop><d:resourcetype/><d:displayname/><a:calendar-color/><c:supported-calendar-component-set/><d:current-user-privilege-set/></d:prop>
</d:propfind>"#;
        let xml = self.request("PROPFIND", &home, "1", body).await?;
        let doc = parse_xml(&xml)?;
        let mut collections = Vec::new();
        for response in responses(&doc) {
            let Some(prop) = ok_prop(response) else { continue };
            let is_calendar =
                child(prop, NS_DAV, "resourcetype").is_some_and(|rt| child(rt, NS_CALDAV, "calendar").is_some());
            if !is_calendar {
                continue;
            }
            let Some(href) = child(response, NS_DAV, "href").and_then(|n| n.text()) else { continue };
            let url = home.join(href.trim()).map_err(|err| ProviderError::Other(err.to_string()))?;
            let components: Vec<&str> = child(prop, NS_CALDAV, "supported-calendar-component-set")
                .map(|set| set.children().filter_map(|c| c.attribute("name")).collect())
                .unwrap_or_default();
            let kind = match (components.contains(&"VEVENT"), components.contains(&"VTODO")) {
                (true, false) => CollectionKind::Events,
                (false, true) => CollectionKind::Todos,
                _ => CollectionKind::Both,
            };
            let name = child(prop, NS_DAV, "displayname")
                .and_then(|n| n.text())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    url.path_segments().and_then(|mut s| s.rfind(|p| !p.is_empty())).unwrap_or("Calendar")
                })
                .to_string();
            let color = child(prop, NS_APPLE, "calendar-color")
                .and_then(|n| n.text())
                .map(|c| c.trim().chars().take(7).collect::<String>())
                .filter(|c| c.len() == 7 && c.starts_with('#'));
            let writable = writable(prop);
            collections.push(Collection { id: url.to_string(), name, kind, color, writable });
        }
        Ok(collections)
    }

    /// The events of `collection` overlapping `[start, end)`, recurring events expanded
    /// into their occurrences by the server.
    pub async fn events(
        &self,
        collection: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Event>, ProviderError> {
        let url = Url::parse(collection).map_err(|err| ProviderError::Other(err.to_string()))?;
        let (s, e) = (ical::format_utc(start), ical::format_utc(end));
        let body = format!(
            r#"<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><d:getetag/><c:calendar-data><c:expand start="{s}" end="{e}"/></c:calendar-data></d:prop>
  <c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{s}" end="{e}"/></c:comp-filter></c:comp-filter></c:filter>
</c:calendar-query>"#
        );
        let xml = self.request("REPORT", &url, "1", &body).await?;
        events_in_answer(&xml, collection)
    }

    /// All tasks of `collection` (task K.3), with `href` and `etag` set.
    pub async fn todos(&self, collection: &str) -> Result<Vec<Todo>, ProviderError> {
        let url = Url::parse(collection).map_err(|err| ProviderError::Other(err.to_string()))?;
        let body = r#"<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><d:getetag/><c:calendar-data/></d:prop>
  <c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VTODO"/></c:comp-filter></c:filter>
</c:calendar-query>"#;
        let xml = self.request("REPORT", &url, "1", body).await?;
        todos_in_answer(&xml, collection)
    }

    /// PROPFIND Depth 0 for one property holding an `<d:href>`.
    async fn find_href(&self, url: &Url, prop_xml: &str, ns: &str, name: &str) -> Result<Url, ProviderError> {
        let body = format!(
            r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop>{prop_xml}</d:prop></d:propfind>"#
        );
        let xml = self.request("PROPFIND", url, "0", &body).await?;
        let doc = parse_xml(&xml)?;
        let href = responses(&doc)
            .filter_map(ok_prop)
            .filter_map(|prop| child(prop, ns, name))
            .find_map(|node| child(node, NS_DAV, "href").and_then(|h| h.text()))
            .ok_or_else(|| ProviderError::Other(format!("server reports no {name}")))?;
        url.join(href.trim()).map_err(|err| ProviderError::Other(err.to_string()))
    }

    pub(crate) async fn request(
        &self,
        method: &str,
        url: &Url,
        depth: &str,
        body: &str,
    ) -> Result<String, ProviderError> {
        let method = Method::from_bytes(method.as_bytes()).map_err(|err| ProviderError::Other(err.to_string()))?;
        let response = self
            .http
            .request(method, url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .header("Depth", depth)
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body.to_string())
            .send()
            .await
            .map_err(http_error)?;
        match response.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(ProviderError::Auth),
            status if status.is_success() => response.text().await.map_err(http_error),
            status => Err(ProviderError::Other(format!("{url}: HTTP {}", status.as_u16()))),
        }
    }
}

/// Whether `current-user-privilege-set` allows changing items. A server that does not report
/// the set is trusted to allow it; a write attempt would still fail cleanly.
fn writable(prop: roxmltree::Node<'_, '_>) -> bool {
    let Some(set) = child(prop, NS_DAV, "current-user-privilege-set") else { return true };
    set.children().filter(|n| is(*n, NS_DAV, "privilege")).flat_map(|privilege| privilege.children()).any(|p| {
        p.is_element()
            && p.tag_name().namespace() == Some(NS_DAV)
            && matches!(p.tag_name().name(), "all" | "write" | "write-content" | "bind")
    })
}

fn parse_xml(xml: &str) -> Result<roxmltree::Document<'_>, ProviderError> {
    roxmltree::Document::parse(xml).map_err(|err| ProviderError::Other(format!("unreadable server answer: {err}")))
}

fn is(node: roxmltree::Node<'_, '_>, ns: &str, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace() == Some(ns)
}

fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, ns: &str, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children().find(|c| is(*c, ns, name))
}

/// The `<d:response>` elements of a multistatus.
fn responses<'a, 'i>(doc: &'a roxmltree::Document<'i>) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    doc.root_element().children().filter(|n| is(*n, NS_DAV, "response"))
}

/// The `<d:prop>` of a response's propstat with status 200.
fn ok_prop<'a, 'i>(response: roxmltree::Node<'a, 'i>) -> Option<roxmltree::Node<'a, 'i>> {
    response
        .children()
        .filter(|n| is(*n, NS_DAV, "propstat"))
        .find(|ps| child(*ps, NS_DAV, "status").and_then(|s| s.text()).is_some_and(|s| s.contains(" 200")))
        .and_then(|ps| child(ps, NS_DAV, "prop"))
}

/// One resource of a calendar query answer.
struct CalendarItem {
    /// Absolute URL of the resource.
    href: Option<String>,
    /// Version of the resource, quotes as the server sends them.
    etag: Option<String>,
    /// The iCalendar object.
    data: String,
}

/// The resources of a multistatus that carry `calendar-data`. Hrefs are relative to the
/// collection URL, so they are resolved against `collection`.
fn calendar_items(xml: &str, collection: &Url) -> Result<Vec<CalendarItem>, ProviderError> {
    let doc = parse_xml(xml)?;
    let items = responses(&doc)
        .filter_map(|response| {
            let prop = ok_prop(response)?;
            let data = child(prop, NS_CALDAV, "calendar-data").and_then(|n| n.text())?;
            let href = child(response, NS_DAV, "href")
                .and_then(|n| n.text())
                .and_then(|h| collection.join(h.trim()).ok())
                .map(|url| url.to_string());
            let etag = child(prop, NS_DAV, "getetag")
                .and_then(|n| n.text())
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .map(str::to_string);
            Some(CalendarItem { href, etag, data: data.to_string() })
        })
        .collect();
    Ok(items)
}

/// The events of a calendar query answer for `collection`, each with the `href` and `etag`
/// of the resource it was parsed from.
fn events_in_answer(xml: &str, collection: &str) -> Result<Vec<Event>, ProviderError> {
    let base = Url::parse(collection).map_err(|err| ProviderError::Other(err.to_string()))?;
    let mut events = Vec::new();
    for item in calendar_items(xml, &base)? {
        for mut event in ical::parse_events(&item.data, collection) {
            event.href.clone_from(&item.href);
            event.etag.clone_from(&item.etag);
            events.push(event);
        }
    }
    Ok(events)
}

/// The tasks of a calendar query answer for `collection`, each with the `href` and `etag`
/// of the resource it was parsed from.
fn todos_in_answer(xml: &str, collection: &str) -> Result<Vec<Todo>, ProviderError> {
    let base = Url::parse(collection).map_err(|err| ProviderError::Other(err.to_string()))?;
    let mut todos = Vec::new();
    for item in calendar_items(xml, &base)? {
        for mut todo in ical::parse_todos(&item.data, collection) {
            todo.href.clone_from(&item.href);
            todo.etag.clone_from(&item.etag);
            todos.push(todo);
        }
    }
    Ok(todos)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLLECTION: &str = "https://cloud.example.org/remote.php/dav/calendars/anna/personal/";

    #[test]
    fn events_get_href_and_etag_from_their_response() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response><d:href>/remote.php/dav/calendars/anna/personal/workshop.ics</d:href>
    <d:propstat><d:prop><d:getetag>"abc123"</d:getetag><c:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:workshop
DTSTART:20261011T080000Z
SUMMARY:Workshop
END:VEVENT
END:VCALENDAR
</c:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
  <d:response><d:href>other.ics</d:href>
    <d:propstat><d:prop><d:getetag>"def456"</d:getetag><c:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:other
DTSTART:20261012T080000Z
SUMMARY:Other
END:VEVENT
END:VCALENDAR
</c:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
  <d:response><d:href>gone.ics</d:href>
    <d:propstat><d:prop><d:getetag/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
  </d:response>
</d:multistatus>"#;
        let events = events_in_answer(xml, COLLECTION).expect("answer parses");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].uid, "workshop");
        assert_eq!(events[0].collection, COLLECTION);
        assert_eq!(
            events[0].href.as_deref(),
            Some("https://cloud.example.org/remote.php/dav/calendars/anna/personal/workshop.ics")
        );
        assert_eq!(events[0].etag.as_deref(), Some("\"abc123\""));
        assert_eq!(
            events[1].href.as_deref(),
            Some("https://cloud.example.org/remote.php/dav/calendars/anna/personal/other.ics")
        );
        assert_eq!(events[1].etag.as_deref(), Some("\"def456\""));
    }

    #[test]
    fn todos_get_href_and_etag_from_their_response() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response><d:href>/remote.php/dav/calendars/anna/tasks/steuer.ics</d:href>
    <d:propstat><d:prop><d:getetag>"t-1"</d:getetag><c:calendar-data>BEGIN:VCALENDAR
BEGIN:VTODO
UID:steuer
SUMMARY:Steuer machen
PRIORITY:2
END:VTODO
END:VCALENDAR
</c:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
</d:multistatus>"#;
        let todos = todos_in_answer(xml, COLLECTION).expect("answer parses");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].uid, "steuer");
        assert_eq!(todos[0].summary, "Steuer machen");
        assert_eq!(todos[0].priority, Some(2));
        assert_eq!(
            todos[0].href.as_deref(),
            Some("https://cloud.example.org/remote.php/dav/calendars/anna/tasks/steuer.ics")
        );
        assert_eq!(todos[0].etag.as_deref(), Some("\"t-1\""));
    }

    #[test]
    fn reads_multistatus() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:response><d:href>/remote.php/dav/calendars/anna/personal/</d:href>
    <d:propstat><d:prop><d:displayname>Persönlich</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
    <d:propstat><d:prop><x:calendar-color xmlns:x="http://apple.com/ns/ical/"/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
  </d:response>
</d:multistatus>"#;
        let doc = parse_xml(xml).ok();
        let name = doc
            .as_ref()
            .and_then(|d| responses(d).next())
            .and_then(ok_prop)
            .and_then(|p| child(p, NS_DAV, "displayname"))
            .and_then(|n| n.text());
        assert_eq!(name, Some("Persönlich"));
    }

    #[test]
    fn privilege_set_decides_writability() {
        let prop = |privileges: &str| {
            format!(
                r#"<d:prop xmlns:d="DAV:"><d:current-user-privilege-set>{privileges}</d:current-user-privilege-set></d:prop>"#
            )
        };
        let check = |xml: String| {
            let doc = roxmltree::Document::parse(&xml).expect("xml");
            writable(doc.root_element())
        };
        assert!(check(prop("<d:privilege><d:read/></d:privilege><d:privilege><d:write/></d:privilege>")));
        assert!(check(prop("<d:privilege><d:all/></d:privilege>")));
        assert!(!check(prop(
            "<d:privilege><d:read/></d:privilege><d:privilege><d:read-current-user-privilege-set/></d:privilege>"
        )));
        let none = roxmltree::Document::parse(r#"<d:prop xmlns:d="DAV:"/>"#).expect("xml");
        assert!(writable(none.root_element()));
    }
}
