//! Talking CalDAV (RFC 4791): discovery, calendars, events in a time range.

use chrono::{DateTime, Utc};
use reqwest::{Method, StatusCode, Url};
use torro_pim::{Collection, CollectionKind, Event, ProviderError};

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
  <d:prop><d:resourcetype/><d:displayname/><a:calendar-color/><c:supported-calendar-component-set/></d:prop>
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
                    url.path_segments().and_then(|mut s| s.rfind(|p| !p.is_empty())).unwrap_or("Kalender")
                })
                .to_string();
            let color = child(prop, NS_APPLE, "calendar-color")
                .and_then(|n| n.text())
                .map(|c| c.trim().chars().take(7).collect::<String>())
                .filter(|c| c.len() == 7 && c.starts_with('#'));
            collections.push(Collection { id: url.to_string(), name, kind, color });
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
        let doc = parse_xml(&xml)?;
        let mut events = Vec::new();
        for response in responses(&doc) {
            if let Some(data) =
                ok_prop(response).and_then(|p| child(p, NS_CALDAV, "calendar-data")).and_then(|n| n.text())
            {
                events.extend(ical::parse_events(data, collection));
            }
        }
        Ok(events)
    }

    /// All tasks of `collection` (task K.3), with `href` and `etag` set.
    pub async fn todos(&self, collection: &str) -> Result<Vec<torro_pim::Todo>, ProviderError> {
        let _ = collection;
        Err(ProviderError::Other("todos not implemented yet (task K.3)".into()))
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
