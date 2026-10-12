//! Talking CalDAV (RFC 4791): discovery, calendars, events in a time range.
//!
//! The client sends the account's password with every request, so it only talks to its own
//! server (task M1.4): every URL it is handed or finds in an answer must have the origin
//! (scheme, host, port) of the account URL it was made with ([`CalDavClient::on_server`]).
//! Anything else is refused before a request is built.
//!
//! The collections come with whether they are private to the user (task G.6b,
//! [`CalDavClient::collections_with_privacy`]): theirs and writable by nobody else, as Nextcloud
//! reports owner and sharees. The core resolves GitHub links only there.
//!
//! The password comes in as a [`Secret`] and never shows in `Debug` (task M7.1): a client sits in
//! the core's account state, which may be printed while debugging.

use std::fmt;

use chrono::{DateTime, Utc};
use reqwest::{Method, StatusCode, Url};
use torro_core::account::Secret;
use torro_pim::{Collection, CollectionKind, Event, ProviderError, Todo};

use crate::{CalDavAccount, http_error, ical};

const NS_DAV: &str = "DAV:";
const NS_CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const NS_APPLE: &str = "http://apple.com/ns/ical/";
/// Nextcloud's (ownCloud's) sharing properties: `owner-principal`, `invite`.
const NS_OWNCLOUD: &str = "http://owncloud.org/ns";

/// What [`CalDavClient::collections_with_privacy`] asks of every collection in the calendar home:
/// kind, name, colour and rights, and who owns it and whom it is shared with (`{DAV:}owner` from
/// RFC 3744, Nextcloud's `oc:owner-principal` and `oc:invite`).
const COLLECTIONS_PROPFIND: &str = r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/" xmlns:oc="http://owncloud.org/ns">
  <d:prop><d:resourcetype/><d:displayname/><a:calendar-color/><c:supported-calendar-component-set/><d:current-user-privilege-set/><d:owner/><oc:owner-principal/><oc:invite/></d:prop>
</d:propfind>"#;

/// A signed-in connection to one CalDAV account.
///
/// `Debug` shows server and user, never the password.
#[derive(Clone)]
pub struct CalDavClient {
    pub(crate) http: reqwest::Client,
    pub(crate) base: Url,
    pub(crate) username: String,
    /// Plain text only for the `Authorization` header of each request; it arrives as a
    /// [`Secret`] and is left out of `Debug`.
    pub(crate) password: String,
}

impl fmt::Debug for CalDavClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CalDavClient")
            .field("base", &self.base.as_str())
            .field("username", &self.username)
            .field("password", &"…")
            .finish()
    }
}

/// What a CalDAV account offers, as its collections show it (task M7.1): calendars (events,
/// VEVENT) and task lists (VTODO). A collection that names neither or both counts for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Services {
    /// At least one collection holds events.
    pub calendars: bool,
    /// At least one collection holds tasks.
    pub tasks: bool,
}

impl Services {
    /// The services `collections` offer.
    pub fn of(collections: &[Collection]) -> Services {
        Services {
            calendars: collections.iter().any(|c| c.kind.has_events()),
            tasks: collections.iter().any(|c| c.kind.has_todos()),
        }
    }
}

impl CalDavClient {
    /// A client for `account`, signing every request in with `password`. Fails only for an
    /// account URL that does not parse.
    pub fn new(http: reqwest::Client, account: &CalDavAccount, password: Secret) -> Result<Self, ProviderError> {
        let base = Url::parse(&account.url).map_err(|err| ProviderError::Other(format!("{}: {err}", account.url)))?;
        Ok(CalDavClient { http, base, username: account.username.clone(), password: password.expose().to_string() })
    }

    /// Signs in and reports what the account offers (task M7.1): asks for the
    /// `current-user-principal` and the calendar home, then reads its collections ([`Services`]).
    /// An account without any collection offers nothing. Errors: `Auth` when the server refuses
    /// the password, `Offline` when it cannot be reached, `Other` when no CalDAV answers there.
    pub async fn services(&self) -> Result<Services, ProviderError> {
        Ok(Services::of(&self.collections().await?))
    }

    /// `url` when it lies on this client's server (same origin as the account URL), else an
    /// error. Every request the client sends goes through here first, so the password never
    /// reaches another host, whatever a caller or a server answer names.
    pub(crate) fn on_server(&self, url: Url) -> Result<Url, ProviderError> {
        if url.origin() == self.base.origin() {
            Ok(url)
        } else {
            Err(ProviderError::Other(format!("{url} is not on the server of this account")))
        }
    }

    /// The calendars and task lists of the account. A calendar the server lists on another
    /// host is left out: the client would have to send the password there to read it.
    pub async fn collections(&self) -> Result<Vec<Collection>, ProviderError> {
        Ok(self.collections_with_privacy().await?.into_iter().map(|(collection, _)| collection).collect())
    }

    /// The collections of [`CalDavClient::collections`], each with whether it is *private*: the
    /// signed-in user owns it and nobody else may write to it (task G.6b, [`private`]). The core
    /// resolves GitHub links with the user's token only in private lists; in any other list
    /// someone else could place a link and read what GitHub answers. `false` whenever the server
    /// does not say (fail closed).
    pub async fn collections_with_privacy(&self) -> Result<Vec<(Collection, bool)>, ProviderError> {
        let principal =
            self.find_href(&self.base, "<d:current-user-principal/>", NS_DAV, "current-user-principal").await?;
        let home = self.find_href(&principal, "<c:calendar-home-set/>", NS_CALDAV, "calendar-home-set").await?;
        let xml = self.request("PROPFIND", &home, "1", COLLECTIONS_PROPFIND).await?;
        self.collections_in_answer(&xml, &home, &principal)
    }

    /// The collections in the answer `xml` to [`COLLECTIONS_PROPFIND`] on the calendar home
    /// `home` of the user `principal`, each with whether it is private ([`private`]).
    fn collections_in_answer(
        &self,
        xml: &str,
        home: &Url,
        principal: &Url,
    ) -> Result<Vec<(Collection, bool)>, ProviderError> {
        let doc = parse_xml(xml)?;
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
            let Ok(url) = self.on_server(url) else { continue };
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
            let private = private(prop, home, principal);
            collections.push((Collection { id: url.to_string(), name, kind, color, writable, source: None }, private));
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
        let url = self.on_server(url.clone())?;
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

/// Whether the collection in `prop` (of the calendar home `home`) is private to the user
/// `principal`: it is theirs ([`owned`]) and nobody else may write to it ([`shared_for_writing`]).
/// Someone who may write to a list could place a GitHub link there and read the title the core
/// fetches with the user's token (task G.6b).
fn private(prop: roxmltree::Node<'_, '_>, home: &Url, principal: &Url) -> bool {
    owned(prop, home, principal) && !shared_for_writing(prop)
}

/// Whether the server reports `principal` as the owner of the collection in `prop`: Nextcloud's
/// `oc:owner-principal` (`principals/users/anna`, relative to its DAV root; a calendar shared with
/// the user names the one who shared it) and the standard `{DAV:}owner` (an href; Nextcloud reports
/// the same owner there). Every owner reported must be the user; none reported is not owned.
fn owned(prop: roxmltree::Node<'_, '_>, home: &Url, principal: &Url) -> bool {
    let me = principal_path(principal);
    let nextcloud = child(prop, NS_OWNCLOUD, "owner-principal")
        .and_then(|n| n.text())
        .map(|owner| owner.trim().trim_matches('/'))
        .filter(|owner| !owner.is_empty())
        .map(|owner| me.ends_with(&format!("/{}", percent_decoded(owner))));
    let acl = child(prop, NS_DAV, "owner")
        .and_then(|owner| child(owner, NS_DAV, "href"))
        .and_then(|href| href.text())
        .map(|href| {
            home.join(href.trim()).is_ok_and(|url| url.origin() == principal.origin() && principal_path(&url) == me)
        });
    match (nextcloud, acl) {
        (None, None) => false,
        (nextcloud, acl) => nextcloud.unwrap_or(true) && acl.unwrap_or(true),
    }
}

/// Whether others may write to the collection in `prop`, as far as the server says: Nextcloud
/// lists the sharees of the user's own collections in `oc:invite`, each with `oc:read` or
/// `oc:read-write` (users, groups and circles alike; public links are read-only). A server that
/// reports no `oc:invite` says nothing about it, which counts as shared.
fn shared_for_writing(prop: roxmltree::Node<'_, '_>) -> bool {
    let Some(invite) = child(prop, NS_OWNCLOUD, "invite") else { return true };
    invite.descendants().any(|node| is(node, NS_OWNCLOUD, "read-write"))
}

/// The path of a principal URL, percent-decoded and without a trailing slash, so
/// `/remote.php/dav/principals/users/anna%40example.org/` and the `principals/users/anna@example.org`
/// of `oc:owner-principal` compare.
fn principal_path(url: &Url) -> String {
    percent_decoded(url.path().trim_end_matches('/'))
}

/// `text` with every `%XX` decoded; anything else, and a `%` not followed by two hex digits, as it
/// is. Invalid UTF-8 becomes U+FFFD, which then matches nothing.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        let hex = bytes.get(i + 1..i + 3).and_then(|pair| std::str::from_utf8(pair).ok());
        match hex.and_then(|pair| u8::from_str_radix(pair, 16).ok()) {
            Some(decoded) if byte == b'%' => {
                out.push(decoded);
                i += 3;
            }
            _ => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
    fn only_urls_on_the_accounts_server_are_used() {
        let account =
            CalDavAccount { url: "https://cloud.example.org/remote.php/dav/".into(), username: "anna".into() };
        let client = CalDavClient::new(reqwest::Client::new(), &account, Secret::new("secret")).expect("client");
        let url = |text: &str| Url::parse(text).expect("url");
        for good in [COLLECTION, "https://cloud.example.org:443/other/path/", "https://CLOUD.example.org/x.ics"] {
            assert!(client.on_server(url(good)).is_ok(), "{good}");
        }
        for foreign in [
            "https://evil.example.org/remote.php/dav/calendars/anna/personal/",
            "http://cloud.example.org/remote.php/dav/",
            "https://cloud.example.org:8443/remote.php/dav/",
            "https://cloud.example.org.evil.example/remote.php/dav/",
            "https://anna:pw@evil.example.org/",
        ] {
            assert!(client.on_server(url(foreign)).is_err(), "{foreign}");
        }
    }

    /// One collection of a calendar home answer, at `path` under anna's home, with `props` added
    /// to its 200 propstat.
    fn home_entry(path: &str, props: &str) -> String {
        format!(
            r#"<d:response><d:href>/remote.php/dav/calendars/anna/{path}/</d:href><d:propstat><d:prop>
<d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>{path}</d:displayname>
<c:supported-calendar-component-set><c:comp name="VTODO"/></c:supported-calendar-component-set>{props}
</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#
        )
    }

    #[test]
    fn only_lists_of_the_user_that_nobody_else_may_write_to_are_private() {
        let shared = |access: &str| {
            format!(
                "<oc:invite><oc:user><d:href>principal:principals/users/bob</d:href><oc:common-name>Bob</oc:common-name>\
                 <oc:invite-accepted/><oc:access><oc:{access}/></oc:access></oc:user></oc:invite>"
            )
        };
        let anna = "<oc:owner-principal>principals/users/anna</oc:owner-principal>";
        let entries = [
            home_entry("own", &format!("{anna}<oc:invite/>")),
            home_entry("read-only-for-bob", &format!("{anna}{}", shared("read"))),
            home_entry("writable-for-bob", &format!("{anna}{}", shared("read-write"))),
            home_entry(
                "team_shared_by_bob",
                "<oc:owner-principal>principals/users/bob</oc:owner-principal><oc:invite/>\
                 <d:owner><d:href>/remote.php/dav/principals/users/bob</d:href></d:owner>",
            ),
            home_entry(
                "acl-owner",
                "<d:owner><d:href>/remote.php/dav/principals/users/anna</d:href></d:owner><oc:invite/>",
            ),
            home_entry(
                "owners-disagree",
                "<oc:owner-principal>principals/users/anna</oc:owner-principal><oc:invite/>\
                 <d:owner><d:href>/remote.php/dav/principals/users/bob</d:href></d:owner>",
            ),
            home_entry("nobody-says", "<oc:invite/>"),
            home_entry("sharing-unknown", anna),
        ];
        let xml = format!(
            r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:oc="http://owncloud.org/ns">{}</d:multistatus>"#,
            entries.concat()
        );
        let account =
            CalDavAccount { url: "https://cloud.example.org/remote.php/dav/".into(), username: "anna".into() };
        let client = CalDavClient::new(reqwest::Client::new(), &account, Secret::new("secret")).expect("client");
        let url = |text: &str| Url::parse(text).expect("url");
        let home = url("https://cloud.example.org/remote.php/dav/calendars/anna/");
        let principal = url("https://cloud.example.org/remote.php/dav/principals/users/anna/");

        let found = client.collections_in_answer(&xml, &home, &principal).expect("answer parses");
        let private: Vec<(&str, bool)> = found.iter().map(|(c, private)| (c.name.as_str(), *private)).collect();
        assert_eq!(
            private,
            [
                ("own", true),
                ("read-only-for-bob", true),
                ("writable-for-bob", false),
                ("team_shared_by_bob", false),
                ("acl-owner", true),
                ("owners-disagree", false),
                ("nobody-says", false),
                ("sharing-unknown", false),
            ]
        );
    }

    #[test]
    fn owners_compare_without_percent_encoding() {
        let xml = r#"<d:prop xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns"><oc:owner-principal>principals/users/anna@example.org</oc:owner-principal><oc:invite/></d:prop>"#;
        let doc = roxmltree::Document::parse(xml).expect("xml");
        let url = |text: &str| Url::parse(text).expect("url");
        let home = url("https://cloud.example.org/remote.php/dav/calendars/anna%40example.org/");
        let anna = url("https://cloud.example.org/remote.php/dav/principals/users/anna%40example.org/");
        let other = url("https://cloud.example.org/remote.php/dav/principals/users/xanna@example.org/");
        assert!(private(doc.root_element(), &home, &anna));
        assert!(!private(doc.root_element(), &home, &other), "a name that only ends the same is someone else");
        assert_eq!(percent_decoded("a%40b%2"), "a@b%2");
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

    #[test]
    fn the_password_stays_out_of_debug_output() {
        let account = CalDavAccount { url: "https://dav.example.org/".into(), username: "anna".into() };
        let client = CalDavClient::new(reqwest::Client::new(), &account, Secret::new("hunter2")).expect("client");
        let shown = format!("{client:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("anna") && shown.contains("dav.example.org"), "{shown}");
    }

    #[test]
    fn services_follow_the_kinds_of_the_collections() {
        let of = |kinds: &[CollectionKind]| {
            let collections: Vec<Collection> = kinds
                .iter()
                .map(|&kind| Collection {
                    id: "x".into(),
                    name: "x".into(),
                    kind,
                    color: None,
                    writable: true,
                    source: None,
                })
                .collect();
            Services::of(&collections)
        };
        assert_eq!(of(&[]), Services { calendars: false, tasks: false });
        assert_eq!(of(&[CollectionKind::Events]), Services { calendars: true, tasks: false });
        assert_eq!(of(&[CollectionKind::Todos]), Services { calendars: false, tasks: true });
        assert_eq!(of(&[CollectionKind::Events, CollectionKind::Todos]), Services { calendars: true, tasks: true });
        assert_eq!(of(&[CollectionKind::Both]), Services { calendars: true, tasks: true });
    }
}
