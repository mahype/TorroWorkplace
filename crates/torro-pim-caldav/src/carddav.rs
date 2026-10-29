//! Talking CardDAV (RFC 6352) for `torro-contacts`: the [`ContactsProvider`] of [`CalDavClient`]
//! (issue #380).
//!
//! The same signed-in client as for calendars, so the same rules: every request goes through
//! [`CalDavClient::on_server`] (the password only reaches the account's own server), and so does
//! every address a server answer names. Address books must lie directly in the user's address book
//! home, cards directly in their book; anything else in an answer is dropped, not followed.
//!
//! Discovery asks the `current-user-principal` for its `addressbook-home-set`, and when that names
//! none, the server's `/.well-known/carddav` (RFC 6764) on the same origin. Changes come by
//! `sync-collection` (RFC 6578) when the core holds a token; without one, with an expired one or on
//! a server without the report, a PROPFIND lists every card with its ETag.
//!
//! Writes are ETag-guarded: a new card with `If-None-Match: *`, a changed or deleted one with
//! `If-Match`. A `412` brings the server's version along for a three-way merge. Error texts are
//! our own: neither a server's message nor a vCard ever ends up in one (they reach logs).

use reqwest::header::{CONTENT_TYPE, ETAG};
use reqwest::{Method, StatusCode, Url};
use torro_contacts::{
    Changes, ContactsError, ContactsProvider, Entry, Listing, RemoteBook, RemoteCard, Stored, SyncToken,
};

use crate::CalDavClient;
use crate::client::{NS_DAV, child, is, ok_prop, percent_decoded, responses, writable, xml_escaped};
use crate::write::item_url;

const NS_CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";

/// How many cards one `addressbook-multiget` asks for: large books go in several requests, so no
/// single answer grows without bound.
const MULTIGET_CHUNK: usize = 100;

/// How many pages of a truncated `sync-collection` answer (`507` on the book) are followed before
/// giving up: a server that never ends its list must not keep the core busy.
const MAX_SYNC_PAGES: usize = 50;

/// Nextcloud's own books: the system address book (every account of the instance) and the
/// contacts app's "Recently contacted". Shown on request only; writable as the server reports.
const HIDDEN_BOOKS: [&str; 2] = ["z-server-generated--system", "z-app-generated--contactsinteraction--recent"];

/// The well-known address of CardDAV (RFC 6764), on the origin of the account URL.
const WELL_KNOWN: &str = "/.well-known/carddav";

/// What discovery asks at every step: who the user is and where their address books are.
const DISCOVERY_PROPFIND: &str = r#"<d:propfind xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:prop><d:current-user-principal/><card:addressbook-home-set/></d:prop>
</d:propfind>"#;

/// What the address book home is asked of every collection in it. `getctag` is the fallback
/// change marker of servers without `sync-token`.
const BOOKS_PROPFIND: &str = r#"<d:propfind xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/">
  <d:prop><d:resourcetype/><d:displayname/><cs:getctag/><d:sync-token/><d:current-user-privilege-set/></d:prop>
</d:propfind>"#;

/// What a full listing asks of a book and its members: the book's token, every card's version.
const ENTRIES_PROPFIND: &str = r#"<d:propfind xmlns:d="DAV:">
  <d:prop><d:resourcetype/><d:getetag/><d:sync-token/></d:prop>
</d:propfind>"#;

/// The body type of every PROPFIND and REPORT.
const XML: &str = "application/xml; charset=utf-8";

/// One answer of the server, read completely. `body` stays inside this module: it is parsed, never
/// passed on in an error.
struct Answer {
    status: StatusCode,
    etag: Option<String>,
    body: String,
    /// Where the answer came from after redirects (the well-known address leads elsewhere).
    landed: Url,
}

/// What one discovery PROPFIND found, both already checked to be on the account's server.
#[derive(Default)]
struct Found {
    principal: Option<Url>,
    home: Option<Url>,
}

/// One page of a `sync-collection` answer.
#[derive(Default)]
struct SyncPage {
    changed: Vec<Entry>,
    removed: Vec<String>,
    token: Option<String>,
    /// The server cut the list short (`507` on the book itself, RFC 6578 section 3.6): ask again
    /// with the new token for the rest.
    truncated: bool,
}

impl ContactsProvider for CalDavClient {
    async fn address_books(&self) -> Result<Vec<RemoteBook>, ContactsError> {
        let home = self.addressbook_home().await?;
        let answer = self.send(propfind()?, &home, &[("Depth", "1")], Some((XML, BOOKS_PROPFIND.to_string()))).await?;
        let body = read_ok(answer)?;
        self.books_in_answer(&body, &home)
    }

    async fn changes(&self, book: &str, since: Option<&SyncToken>) -> Result<Changes, ContactsError> {
        let book = self.book_url(book)?;
        if let Some(since) = since
            && let Some(changes) = self.sync_collection(&book, since).await?
        {
            return Ok(changes);
        }
        self.full_listing(&book).await
    }

    async fn fetch(&self, book: &str, hrefs: &[String]) -> Result<Vec<RemoteCard>, ContactsError> {
        let book = self.book_url(book)?;
        let paths: Vec<String> = hrefs
            .iter()
            .filter_map(|href| self.card_url(&book, href))
            .map(|url| format!("<d:href>{}</d:href>", xml_escaped(url.path())))
            .collect();
        let mut cards = Vec::new();
        for chunk in paths.chunks(MULTIGET_CHUNK) {
            let body = format!(
                r#"<card:addressbook-multiget xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:prop><d:getetag/><card:address-data/></d:prop>
  {}
</card:addressbook-multiget>"#,
                chunk.join("\n  ")
            );
            let answer = self.send(report()?, &book, &[("Depth", "0")], Some((XML, body))).await?;
            let xml = read_ok(answer)?;
            cards.extend(self.cards_in_answer(&xml, &book)?);
        }
        Ok(cards)
    }

    async fn put(
        &self,
        book: &str,
        href: Option<&str>,
        uid: &str,
        vcard: &str,
        etag: Option<&str>,
    ) -> Result<Stored, ContactsError> {
        let book = self.book_url(book)?;
        let (target, precondition) = match href {
            None => (new_card_url(&book, uid)?, ("If-None-Match", "*")),
            Some(href) => {
                let target = self.card_url(&book, href).ok_or_else(not_a_card)?;
                // Without `If-Match` the PUT would replace whatever the server holds now.
                let etag = etag.map(str::trim).filter(|etag| !etag.is_empty()).ok_or_else(|| {
                    ContactsError::Other("a changed contact needs the version it was read in (etag)".into())
                })?;
                (target, ("If-Match", etag))
            }
        };
        let body = Some(("text/vcard; charset=utf-8", vcard.to_string()));
        let answer = self.send(Method::PUT, &target, &[precondition], body).await?;
        if answer.status.is_success() {
            return Ok(Stored { href: target.to_string(), etag: answer.etag });
        }
        Err(self.write_error(&target, &answer).await)
    }

    async fn delete(&self, book: &str, href: &str, etag: Option<&str>) -> Result<(), ContactsError> {
        let book = self.book_url(book)?;
        let target = self.card_url(&book, href).ok_or_else(not_a_card)?;
        let headers: Vec<(&str, &str)> =
            etag.map(str::trim).filter(|etag| !etag.is_empty()).map(|etag| ("If-Match", etag)).into_iter().collect();
        let answer = self.send(Method::DELETE, &target, &headers, None).await?;
        // 404: gone already, which is what the caller wanted.
        if answer.status.is_success() || answer.status == StatusCode::NOT_FOUND {
            return Ok(());
        }
        Err(self.write_error(&target, &answer).await)
    }
}

impl CalDavClient {
    /// The user's address book home: from the principal the account URL names, else from the
    /// well-known address. [`ContactsError::NotFound`] when neither names one: an account without
    /// contacts.
    async fn addressbook_home(&self) -> Result<Url, ContactsError> {
        if let Some(home) = self.home_from(&self.base).await? {
            return Ok(home);
        }
        let well_known = self.base.join(WELL_KNOWN).map_err(other)?;
        self.home_from(&well_known).await?.ok_or(ContactsError::NotFound)
    }

    /// The address book home reachable from `start`: named there, or by the principal named
    /// there. A redirect is followed to where it led, since reqwest turns a PROPFIND that meets a
    /// `301` into a GET (Nextcloud's well-known addresses answer `301`).
    async fn home_from(&self, start: &Url) -> Result<Option<Url>, ContactsError> {
        let (found, landed) = self.discovery(start).await?;
        let found = match landed {
            Some(landed) if found.home.is_none() && found.principal.is_none() && landed != *start => {
                self.discovery(&landed).await?.0
            }
            _ => found,
        };
        if let Some(home) = found.home {
            return Ok(Some(home));
        }
        let Some(principal) = found.principal else { return Ok(None) };
        Ok(self.discovery(&principal).await?.0.home)
    }

    /// One discovery PROPFIND at `url` and where it landed (on this server only). An answer that is
    /// no multistatus finds nothing; only a refused sign-in or an unreachable server is an error.
    async fn discovery(&self, url: &Url) -> Result<(Found, Option<Url>), ContactsError> {
        let answer =
            self.send(propfind()?, url, &[("Depth", "0")], Some((XML, DISCOVERY_PROPFIND.to_string()))).await?;
        match answer.status {
            StatusCode::UNAUTHORIZED => return Err(ContactsError::Auth),
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => return Err(ContactsError::Offline),
            _ => {}
        }
        let landed = self.on_server(answer.landed.clone()).ok();
        if !answer.status.is_success() {
            return Ok((Found::default(), landed));
        }
        let Ok(doc) = roxmltree::Document::parse(&answer.body) else { return Ok((Found::default(), landed)) };
        let base = landed.clone().unwrap_or_else(|| url.clone());
        let href_of = |ns: &str, name: &str| {
            responses(&doc)
                .filter_map(ok_prop)
                .filter_map(|prop| child(prop, ns, name))
                .find_map(|node| child(node, NS_DAV, "href").and_then(|href| href.text()))
                .and_then(|href| base.join(href.trim()).ok())
                .and_then(|url| self.on_server(url).ok())
        };
        let found = Found {
            principal: href_of(NS_DAV, "current-user-principal"),
            home: href_of(NS_CARDDAV, "addressbook-home-set"),
        };
        Ok((found, landed))
    }

    /// The address books in the answer to [`BOOKS_PROPFIND`] on `home`: collections of type
    /// `addressbook` directly in the home, on this server.
    fn books_in_answer(&self, xml: &str, home: &Url) -> Result<Vec<RemoteBook>, ContactsError> {
        let doc = parse(xml)?;
        let mut books = Vec::new();
        for response in responses(&doc) {
            let Some(prop) = ok_prop(response) else { continue };
            let is_book =
                child(prop, NS_DAV, "resourcetype").is_some_and(|rt| child(rt, NS_CARDDAV, "addressbook").is_some());
            if !is_book {
                continue;
            }
            let Some(href) = child(response, NS_DAV, "href").and_then(|n| n.text()) else { continue };
            let Some((url, name)) = self.book_in_home(home, href) else { continue };
            let display_name = child(prop, NS_DAV, "displayname")
                .and_then(|n| n.text())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .unwrap_or(&name)
                .to_string();
            let hidden_by_default = HIDDEN_BOOKS.contains(&name.as_str());
            books.push(RemoteBook {
                href: url.to_string(),
                name,
                display_name,
                writable: writable(prop),
                hidden_by_default,
            });
        }
        Ok(books)
    }

    /// The URL (with a trailing slash) and decoded name of the book at `href`, when it is a
    /// collection directly in `home` on this server.
    fn book_in_home(&self, home: &Url, href: &str) -> Option<(Url, String)> {
        // A book is one path segment below the home, as a card is one below its book.
        let url = item_url(home.as_str(), href.trim().trim_end_matches('/'))?;
        let url = self.on_server(url).ok()?;
        let name = url.path_segments()?.next_back().map(percent_decoded)?;
        let mut book = url;
        book.set_path(&format!("{}/", book.path()));
        Some((book, name))
    }

    /// `book` parsed, with a trailing slash, on this server.
    fn book_url(&self, book: &str) -> Result<Url, ContactsError> {
        let mut url = Url::parse(book.trim())
            .map_err(|_| ContactsError::Other("the address book's address does not parse".into()))?;
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        self.on_server(url).map_err(|_| off_server())
    }

    /// The card at `href` when it lies directly in `book` on this server.
    fn card_url(&self, book: &Url, href: &str) -> Option<Url> {
        item_url(book.as_str(), href).and_then(|url| self.on_server(url).ok())
    }

    /// The changes since `since` by `sync-collection`; `None` when the server cannot answer it (an
    /// expired token, no support for the report): then the caller lists everything.
    async fn sync_collection(&self, book: &Url, since: &SyncToken) -> Result<Option<Changes>, ContactsError> {
        let mut token = since.0.clone();
        let mut changed: Vec<Entry> = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        for _ in 0..MAX_SYNC_PAGES {
            let body = format!(
                r#"<d:sync-collection xmlns:d="DAV:">
  <d:sync-token>{}</d:sync-token>
  <d:sync-level>1</d:sync-level>
  <d:prop><d:getetag/></d:prop>
</d:sync-collection>"#,
                xml_escaped(&token)
            );
            // RFC 6578 section 3.2: the report is defined for Depth 0 only.
            let answer = self.send(report()?, book, &[("Depth", "0")], Some((XML, body))).await?;
            if falls_back(answer.status, &answer.body) {
                return Ok(None);
            }
            let page = self.sync_page(&read_ok(answer)?, book)?;
            for entry in page.changed {
                removed.retain(|href| *href != entry.href);
                changed.retain(|known| known.href != entry.href);
                changed.push(entry);
            }
            for href in page.removed {
                changed.retain(|known| known.href != href);
                if !removed.contains(&href) {
                    removed.push(href);
                }
            }
            match (page.truncated, page.token) {
                (false, next) => {
                    return Ok(Some(Changes {
                        listing: Listing::Delta { changed, removed },
                        token: next.map(SyncToken),
                    }));
                }
                (true, Some(next)) => token = next,
                // Cut short without a way to go on: only a full listing is sure to be complete.
                (true, None) => return Ok(None),
            }
        }
        Err(ContactsError::Other("the server's list of changes did not end".into()))
    }

    /// One page of a `sync-collection` answer for `book`: cards changed (with their ETag) and
    /// removed (`404`), only those directly in the book.
    fn sync_page(&self, xml: &str, book: &Url) -> Result<SyncPage, ContactsError> {
        let doc = parse(xml)?;
        let mut page = SyncPage::default();
        for response in responses(&doc) {
            let Some(href) = child(response, NS_DAV, "href").and_then(|n| n.text()) else { continue };
            let status = child(response, NS_DAV, "status").and_then(|n| n.text()).unwrap_or_default();
            if is_book(book, href) {
                page.truncated |= status.contains(" 507");
                continue;
            }
            let Some(url) = self.card_url(book, href) else { continue };
            if status.contains(" 404") {
                page.removed.push(url.to_string());
                continue;
            }
            let Some(prop) = ok_prop(response) else { continue };
            if !is_collection(prop) {
                page.changed.push(Entry { href: url.to_string(), etag: etag_of(prop).unwrap_or_default() });
            }
        }
        page.token = child(doc.root_element(), NS_DAV, "sync-token").and_then(text_of);
        Ok(page)
    }

    /// Every card of `book` with its version, and the book's token to ask with next time.
    async fn full_listing(&self, book: &Url) -> Result<Changes, ContactsError> {
        let answer = self.send(propfind()?, book, &[("Depth", "1")], Some((XML, ENTRIES_PROPFIND.to_string()))).await?;
        let xml = read_ok(answer)?;
        let doc = parse(&xml)?;
        let mut entries = Vec::new();
        let mut token = None;
        for response in responses(&doc) {
            let Some(href) = child(response, NS_DAV, "href").and_then(|n| n.text()) else { continue };
            let Some(prop) = ok_prop(response) else { continue };
            if is_book(book, href) {
                token = child(prop, NS_DAV, "sync-token").and_then(text_of);
                continue;
            }
            let Some(url) = self.card_url(book, href) else { continue };
            if !is_collection(prop) {
                entries.push(Entry { href: url.to_string(), etag: etag_of(prop).unwrap_or_default() });
            }
        }
        Ok(Changes { listing: Listing::Full(entries), token: token.map(SyncToken) })
    }

    /// The cards of an `addressbook-multiget` answer for `book`; a card the server no longer has
    /// (no `address-data` with status 200) is left out.
    fn cards_in_answer(&self, xml: &str, book: &Url) -> Result<Vec<RemoteCard>, ContactsError> {
        let doc = parse(xml)?;
        let cards = responses(&doc)
            .filter_map(|response| {
                let prop = ok_prop(response)?;
                let vcard = child(prop, NS_CARDDAV, "address-data").and_then(|n| n.text())?;
                let href = child(response, NS_DAV, "href").and_then(|n| n.text())?;
                let url = self.card_url(book, href)?;
                Some(RemoteCard { href: url.to_string(), etag: etag_of(prop), vcard: vcard.to_string() })
            })
            .collect();
        Ok(cards)
    }

    /// The error for a refused PUT or DELETE of `target`. A `412` fetches the server's version for
    /// the merge; when that fails too, the conflict comes without it.
    async fn write_error(&self, target: &Url, answer: &Answer) -> ContactsError {
        match answer.status {
            StatusCode::PRECONDITION_FAILED => {
                let (server_vcard, etag) = match self.send(Method::GET, target, &[], None).await {
                    Ok(current) if current.status.is_success() => (Some(current.body), current.etag),
                    _ => (None, None),
                };
                ContactsError::Conflict { server_vcard, etag }
            }
            StatusCode::UNAUTHORIZED => ContactsError::Auth,
            StatusCode::FORBIDDEN => ContactsError::ReadOnly,
            _ if needs_privileges(&answer.body) => ContactsError::ReadOnly,
            StatusCode::PAYLOAD_TOO_LARGE => ContactsError::TooLarge,
            StatusCode::NOT_FOUND => ContactsError::NotFound,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => ContactsError::Offline,
            status => ContactsError::Other(format!("the server refused the change (HTTP {})", status.as_u16())),
        }
    }

    /// Sends one request to `url`, signed in, and reads the whole answer. The URL goes through
    /// [`CalDavClient::on_server`] first: the password never reaches another host.
    async fn send(
        &self,
        method: Method,
        url: &Url,
        headers: &[(&str, &str)],
        body: Option<(&str, String)>,
    ) -> Result<Answer, ContactsError> {
        let url = self.on_server(url.clone()).map_err(|_| off_server())?;
        let mut request = self.http.request(method, url).basic_auth(&self.username, Some(&self.password));
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if let Some((content_type, body)) = body {
            request = request.header(CONTENT_TYPE, content_type).body(body);
        }
        let response = request.send().await.map_err(transport)?;
        let status = response.status();
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let landed = response.url().clone();
        let body = response.text().await.map_err(transport)?;
        Ok(Answer { status, etag, body, landed })
    }
}

/// The body of a successful answer, else the error its status means.
fn read_ok(answer: Answer) -> Result<String, ContactsError> {
    if answer.status.is_success() {
        return Ok(answer.body);
    }
    Err(match answer.status {
        // As for calendars: a refused read means the app password no longer works.
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ContactsError::Auth,
        StatusCode::NOT_FOUND => ContactsError::NotFound,
        // Throttled or in maintenance: try again later, nothing to show.
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => ContactsError::Offline,
        status => ContactsError::Other(format!("the server answered HTTP {}", status.as_u16())),
    })
}

/// Whether a `sync-collection` answer asks for a full listing instead: the token is unknown or
/// expired (`DAV:valid-sync-token`), or the server does not do the report.
fn falls_back(status: StatusCode, body: &str) -> bool {
    match status.as_u16() {
        405 | 415 | 501 => true,
        400 | 403 | 409 => has_dav_element(body, "valid-sync-token") || has_dav_element(body, "supported-report"),
        _ => false,
    }
}

/// Whether an error body names the privileges the user lacks (RFC 3744 `DAV:need-privileges`).
fn needs_privileges(body: &str) -> bool {
    has_dav_element(body, "need-privileges")
}

/// Whether `body` is XML with a `DAV:` element `name` anywhere.
fn has_dav_element(body: &str, name: &str) -> bool {
    roxmltree::Document::parse(body).is_ok_and(|doc| doc.descendants().any(|node| is(node, NS_DAV, name)))
}

/// Whether `href` names `book` itself.
fn is_book(book: &Url, href: &str) -> bool {
    book.join(href.trim()).is_ok_and(|url| {
        url.origin() == book.origin() && url.path().trim_end_matches('/') == book.path().trim_end_matches('/')
    })
}

/// Whether the resource of `prop` is a collection (a folder in a book is no card).
fn is_collection(prop: roxmltree::Node<'_, '_>) -> bool {
    child(prop, NS_DAV, "resourcetype").is_some_and(|rt| child(rt, NS_DAV, "collection").is_some())
}

/// The `getetag` of `prop`, quotes as the server sends them.
fn etag_of(prop: roxmltree::Node<'_, '_>) -> Option<String> {
    child(prop, NS_DAV, "getetag").and_then(text_of)
}

/// The trimmed, non-empty text of `node`.
fn text_of(node: roxmltree::Node<'_, '_>) -> Option<String> {
    node.text().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string)
}

/// `<book>/<name>.vcf` for a new card: the uid itself when it is plain (letters, digits, `-`, `_`,
/// `.`, `@`), else a name made of it. A uid from elsewhere (`move_contact` keeps it) may hold `/`
/// or `\`; escaped as `%2F` they would make an address every later read and write refuses
/// (`write::item_url`). The name is made the same way each time, so sending again after a lost
/// answer finds the card instead of making a second one.
fn new_card_url(book: &Url, uid: &str) -> Result<Url, ContactsError> {
    if uid.trim().is_empty() {
        return Err(ContactsError::Other("a new contact needs a uid".into()));
    }
    let mut url = book.clone();
    url.path_segments_mut()
        .map_err(|()| ContactsError::Other("the address book cannot hold contacts".into()))?
        .pop_if_empty()
        .push(&format!("{}.vcf", card_name(uid)));
    Ok(url)
}

/// The file name of a new card with `uid`; see [`new_card_url`].
fn card_name(uid: &str) -> String {
    let plain = !uid.starts_with('.')
        && uid.len() <= 200
        && uid.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'));
    if plain {
        return uid.to_string();
    }
    // The uid's bytes in hex; a long one is cut and its whole told apart by a checksum (FNV-1a).
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let bytes = uid.as_bytes();
    if bytes.len() <= 64 {
        return format!("torro-{}", hex(bytes));
    }
    let sum = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
    format!("torro-{}-{sum:016x}", hex(&bytes[..64]))
}

fn parse(xml: &str) -> Result<roxmltree::Document<'_>, ContactsError> {
    roxmltree::Document::parse(xml).map_err(|_| ContactsError::Other("unreadable answer from the server".into()))
}

fn propfind() -> Result<Method, ContactsError> {
    Method::from_bytes(b"PROPFIND").map_err(other)
}

fn report() -> Result<Method, ContactsError> {
    Method::from_bytes(b"REPORT").map_err(other)
}

/// Connection problems and timeouts mean the server is out of reach; anything else is ours to name.
fn transport(err: reqwest::Error) -> ContactsError {
    if err.is_connect() || err.is_timeout() {
        ContactsError::Offline
    } else {
        ContactsError::Other("the request to the server failed".into())
    }
}

fn off_server() -> ContactsError {
    ContactsError::Other("the address is not on the server of this account".into())
}

fn not_a_card() -> ContactsError {
    ContactsError::Other("the address is not a contact in this address book".into())
}

/// For errors that cannot happen with the constant inputs they get (a method name, a fixed path).
fn other<E>(_: E) -> ContactsError {
    ContactsError::Other("internal error building a request".into())
}

#[cfg(test)]
mod tests {
    use httpmock::Method::{DELETE, GET, PUT};
    use httpmock::{Mock, MockServer};
    use torro_core::account::Secret;

    use super::*;
    use crate::CalDavAccount;

    const PRINCIPAL: &str = include_str!("../tests/fixtures/carddav/nextcloud-principal.xml");
    const PRINCIPAL_HOME: &str = include_str!("../tests/fixtures/carddav/nextcloud-principal-home.xml");
    const HOME: &str = include_str!("../tests/fixtures/carddav/nextcloud-home.xml");
    const BOOK_FULL: &str = include_str!("../tests/fixtures/carddav/nextcloud-book-full.xml");
    const SYNC_DELTA: &str = include_str!("../tests/fixtures/carddav/nextcloud-sync-delta.xml");
    const SYNC_EXPIRED: &str = include_str!("../tests/fixtures/carddav/nextcloud-sync-expired.xml");
    const MULTIGET: &str = include_str!("../tests/fixtures/carddav/nextcloud-multiget.xml");
    const SERVER_CARD: &str = include_str!("../tests/fixtures/carddav/nextcloud-card-server.vcf");
    const NEED_PRIVILEGES: &str = include_str!("../tests/fixtures/carddav/nextcloud-need-privileges.xml");

    const BOOK_PATH: &str = "/remote.php/dav/addressbooks/users/anna/contacts/";
    const VCARD: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ben\r\nFN:Ben Beispiel\r\nEND:VCARD\r\n";

    fn client(server: &MockServer) -> CalDavClient {
        let account = CalDavAccount { url: server.url("/remote.php/dav/"), username: "anna".into() };
        CalDavClient::new(reqwest::Client::new(), &account, Secret::new("secret")).expect("client")
    }

    /// Answers a PROPFIND at `path` with Depth `depth` with the multistatus `body`.
    async fn propfind_at<'a>(server: &'a MockServer, path: &str, depth: &str, body: &str) -> Mock<'a> {
        server
            .mock_async(|when, then| {
                when.path(path).header("depth", depth).body_includes("<d:propfind");
                then.status(207).header("Content-Type", "application/xml; charset=utf-8").body(body);
            })
            .await
    }

    /// A multistatus with one response at `href` holding `props` (status 200).
    fn multistatus(href: &str, props: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav"><d:response><d:href>{href}</d:href><d:propstat><d:prop>{props}</d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#
        )
    }

    fn card(server: &MockServer, name: &str) -> String {
        server.url(format!("{BOOK_PATH}{name}"))
    }

    #[tokio::test]
    async fn address_books_come_from_the_home_the_principal_names() {
        let server = MockServer::start_async().await;
        let asked = propfind_at(&server, "/remote.php/dav/", "0", PRINCIPAL).await;
        propfind_at(&server, "/remote.php/dav/principals/users/anna/", "0", PRINCIPAL_HOME).await;
        propfind_at(&server, "/remote.php/dav/addressbooks/users/anna/", "1", HOME).await;

        let books = client(&server).address_books().await.expect("books");
        let home = server.url("/remote.php/dav/addressbooks/users/anna/");
        let found: Vec<(&str, &str, &str, bool, bool)> = books
            .iter()
            .map(|b| {
                (
                    b.href.trim_start_matches(&home),
                    b.name.as_str(),
                    b.display_name.as_str(),
                    b.writable,
                    b.hidden_by_default,
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("contacts/", "contacts", "Contacts", true, false),
                ("familie%20m%C3%BCller/", "familie müller", "familie müller", true, false),
                ("team_shared_by_bob/", "team_shared_by_bob", "Team (Bob)", false, false),
                ("z-server-generated--system/", "z-server-generated--system", "Accounts", false, true),
                (
                    "z-app-generated--contactsinteraction--recent/",
                    "z-app-generated--contactsinteraction--recent",
                    "Recently contacted",
                    true,
                    true
                ),
            ],
            "no plain folder, nothing on another host, outside the home or below another book"
        );
        assert!(books.iter().all(|b| b.href.starts_with(&home)));
        assert_eq!(asked.calls_async().await, 1, "no well-known detour when the principal has a home");
    }

    /// A principal without `addressbook-home-set`: the well-known address answers `301` (Nextcloud),
    /// which reqwest follows with a GET; discovery asks again where it led.
    #[tokio::test]
    async fn the_home_comes_from_the_well_known_address_when_the_principal_names_none() {
        let server = MockServer::start_async().await;
        propfind_at(&server, "/remote.php/dav/", "0", PRINCIPAL).await;
        propfind_at(&server, "/remote.php/dav/principals/users/anna/", "0", PRINCIPAL).await;
        server
            .mock_async(|when, then| {
                when.path("/.well-known/carddav");
                then.status(301).header("Location", "/dav/");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/dav/");
                then.status(200).body("This is the WebDAV interface. It can only be accessed by WebDAV clients.");
            })
            .await;
        let principal = "<d:current-user-principal><d:href>/dav/principals/anna/</d:href></d:current-user-principal>";
        propfind_at(&server, "/dav/", "0", &multistatus("/dav/", principal)).await;
        let home = "<card:addressbook-home-set><d:href>/dav/addressbooks/anna/</d:href></card:addressbook-home-set>";
        propfind_at(&server, "/dav/principals/anna/", "0", &multistatus("/dav/principals/anna/", home)).await;
        let book =
            "<d:resourcetype><d:collection/><card:addressbook/></d:resourcetype><d:displayname>Friends</d:displayname>";
        let books = propfind_at(&server, "/dav/addressbooks/anna/", "1", &multistatus("friends/", book)).await;

        let found = client(&server).address_books().await.expect("books");
        assert_eq!(books.calls_async().await, 1);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].href.as_str(), found[0].name.as_str()),
            (server.url("/dav/addressbooks/anna/friends/").as_str(), "friends")
        );
    }

    #[tokio::test]
    async fn without_an_address_book_home_there_are_no_contacts() {
        let server = MockServer::start_async().await;
        propfind_at(&server, "/remote.php/dav/", "0", PRINCIPAL).await;
        propfind_at(&server, "/remote.php/dav/principals/users/anna/", "0", PRINCIPAL).await;
        server
            .mock_async(|when, then| {
                when.path("/.well-known/carddav");
                then.status(404);
            })
            .await;
        assert_eq!(client(&server).address_books().await, Err(ContactsError::NotFound));
    }

    #[tokio::test]
    async fn the_first_sync_lists_every_card_with_the_books_token() {
        let server = MockServer::start_async().await;
        propfind_at(&server, BOOK_PATH, "1", BOOK_FULL).await;
        let changes = client(&server).changes(&server.url(BOOK_PATH), None).await.expect("changes");
        let entries = [
            ("anna-1.vcf", "\"0c3b5a4d3a7e9f1e2b6d8c4a1f0e9d8c\""),
            ("ben.vcf", "\"b1\""),
            ("carla%20b.vcf", "\"c1\""),
        ]
        .map(|(name, etag)| Entry { href: card(&server, name), etag: etag.into() });
        assert_eq!(changes.listing, Listing::Full(entries.to_vec()), "no folder, no foreign or nested card");
        assert_eq!(changes.token, Some(SyncToken("http://sabre.io/ns/sync/42".into())));
    }

    #[tokio::test]
    async fn a_sync_with_a_token_brings_only_the_changes() {
        let server = MockServer::start_async().await;
        let sync = server
            .mock_async(|when, then| {
                when.path(BOOK_PATH)
                    .header("depth", "0")
                    .body_includes("<d:sync-collection")
                    .body_includes("<d:sync-token>http://sabre.io/ns/sync/42</d:sync-token>");
                then.status(207).body(SYNC_DELTA);
            })
            .await;
        let book = server.url("/remote.php/dav/addressbooks/users/anna/contacts");
        let since = SyncToken("http://sabre.io/ns/sync/42".into());
        let changes = client(&server).changes(&book, Some(&since)).await.expect("changes");
        assert_eq!(sync.calls_async().await, 1);
        assert_eq!(
            changes.listing,
            Listing::Delta {
                changed: vec![
                    Entry { href: card(&server, "ben.vcf"), etag: "\"b2\"".into() },
                    Entry { href: card(&server, "dora.vcf"), etag: "\"d1\"".into() },
                ],
                removed: vec![card(&server, "carla%20b.vcf")],
            }
        );
        assert_eq!(changes.token, Some(SyncToken("http://sabre.io/ns/sync/45".into())));
    }

    #[tokio::test]
    async fn an_expired_token_falls_back_to_a_full_listing() {
        let server = MockServer::start_async().await;
        let sync = server
            .mock_async(|when, then| {
                when.path(BOOK_PATH).body_includes("<d:sync-collection");
                then.status(403).body(SYNC_EXPIRED);
            })
            .await;
        let full = propfind_at(&server, BOOK_PATH, "1", BOOK_FULL).await;
        let since = SyncToken("http://sabre.io/ns/sync/1".into());
        let changes = client(&server).changes(&server.url(BOOK_PATH), Some(&since)).await.expect("changes");
        assert_eq!((sync.calls_async().await, full.calls_async().await), (1, 1));
        assert!(matches!(&changes.listing, Listing::Full(entries) if entries.len() == 3));
        assert_eq!(changes.token, Some(SyncToken("http://sabre.io/ns/sync/42".into())));
    }

    #[tokio::test]
    async fn cards_are_fetched_in_parts_of_at_most_a_hundred() {
        let server = MockServer::start_async().await;
        let href = |n: usize| format!("<d:href>{BOOK_PATH}card-{n:03}.vcf</d:href>");
        let first = server
            .mock_async(|when, then| {
                when.path(BOOK_PATH)
                    .body_includes("<card:addressbook-multiget")
                    .body_includes("<card:address-data/>")
                    .body_includes(href(0))
                    .body_includes(href(99))
                    .body_excludes(href(100));
                then.status(207).body(MULTIGET);
            })
            .await;
        let second = server
            .mock_async(|when, then| {
                when.path(BOOK_PATH).body_includes(href(100)).body_includes(href(149)).body_excludes(href(99));
                then.status(207).body(r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"/>"#);
            })
            .await;
        let mut hrefs: Vec<String> = (0..150).map(|n| card(&server, &format!("card-{n:03}.vcf"))).collect();
        hrefs.push("https://evil.example.org/remote.php/dav/addressbooks/users/anna/contacts/x.vcf".into());

        let cards = client(&server).fetch(&server.url(BOOK_PATH), &hrefs).await.expect("cards");
        assert_eq!((first.calls_async().await, second.calls_async().await), (1, 1));
        let found: Vec<(String, Option<&str>)> = cards.iter().map(|c| (c.href.clone(), c.etag.as_deref())).collect();
        assert_eq!(
            found,
            [(card(&server, "card-000.vcf"), Some("\"e000\"")), (card(&server, "card-001.vcf"), Some("\"e001\""))],
            "the missing card and the one on another host are left out"
        );
        assert!(cards[0].vcard.starts_with("BEGIN:VCARD\r\n") && cards[0].vcard.contains("FN:Ben Beispiel"));
        assert!(client(&server).fetch(&server.url(BOOK_PATH), &[]).await.expect("nothing").is_empty());
        assert_eq!(first.calls_async().await, 1, "nothing asked for, nothing sent");
    }

    #[tokio::test]
    async fn a_new_card_goes_to_its_uid_and_never_overwrites() {
        let server = MockServer::start_async().await;
        let created = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path_includes(BOOK_PATH)
                    .header("If-None-Match", "*")
                    .header_missing("If-Match")
                    .header("Content-Type", "text/vcard; charset=utf-8")
                    .body(VCARD);
                then.status(201).header("ETag", "\"n1\"");
            })
            .await;
        let stored = client(&server).put(&server.url(BOOK_PATH), None, "a/b c", VCARD, None).await.expect("stored");
        assert_eq!(created.calls_async().await, 1);
        let name = "torro-612f622063.vcf";
        assert_eq!(stored, Stored { href: card(&server, name), etag: Some("\"n1\"".into()) });
        assert!(
            crate::write::item_url(&server.url(BOOK_PATH), &stored.href).is_some(),
            "a uid with a / gets an address later reads and writes take"
        );
        assert_eq!(card_name("a/b c"), card_name("a/b c"), "the same name every time");
        assert_eq!(card_name("4f1c-anna@example.org"), "4f1c-anna@example.org");
        assert_eq!(card_name(".."), "torro-2e2e");
        let long = card_name(&"\\".repeat(300));
        assert!(long.len() < 160 && long != card_name(&"\\".repeat(301)), "{long}");

        let changed = server
            .mock_async(|when, then| {
                when.method(PUT).path(format!("{BOOK_PATH}ben.vcf")).header("If-Match", "\"b1\"");
                then.status(204);
            })
            .await;
        let stored = client(&server)
            .put(&server.url(BOOK_PATH), Some("ben.vcf"), "ben", VCARD, Some("\"b1\""))
            .await
            .expect("stored");
        assert_eq!(changed.calls_async().await, 1);
        assert_eq!(stored, Stored { href: card(&server, "ben.vcf"), etag: None }, "no ETag: the next sync brings it");
    }

    #[tokio::test]
    async fn a_conflict_brings_the_servers_version() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(PUT).path(format!("{BOOK_PATH}ben.vcf"));
                then.status(412);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{BOOK_PATH}ben.vcf")).header("If-Match", "\"b1\"");
                then.status(412);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{BOOK_PATH}ben.vcf"));
                then.status(200).header("ETag", "\"b2\"").body(SERVER_CARD);
            })
            .await;
        let client = client(&server);
        let book = server.url(BOOK_PATH);
        let expected =
            ContactsError::Conflict { server_vcard: Some(SERVER_CARD.to_string()), etag: Some("\"b2\"".into()) };
        assert_eq!(client.put(&book, Some("ben.vcf"), "ben", VCARD, Some("\"b1\"")).await, Err(expected.clone()));
        assert_eq!(client.delete(&book, "ben.vcf", Some("\"b1\"")).await, Err(expected));
        assert!(!format!("{:?}", client.delete(&book, "ben.vcf", Some("\"b1\"")).await).contains("Beispiel"));

        // The card exists already: a new one with its uid never replaces it.
        server
            .mock_async(|when, then| {
                when.method(PUT).path(format!("{BOOK_PATH}carla.vcf")).header("If-None-Match", "*");
                then.status(412);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(format!("{BOOK_PATH}carla.vcf"));
                then.status(500).body("internal server text");
            })
            .await;
        assert_eq!(
            client.put(&book, None, "carla", VCARD, None).await,
            Err(ContactsError::Conflict { server_vcard: None, etag: None }),
            "the server's version could not be read"
        );
    }

    #[tokio::test]
    async fn refused_writes_name_their_reason_in_our_words() {
        let server = MockServer::start_async().await;
        let shared = "/remote.php/dav/addressbooks/users/anna/team_shared_by_bob/";
        server
            .mock_async(|when, then| {
                when.method(PUT).path_includes(shared);
                then.status(403).body(NEED_PRIVILEGES);
            })
            .await;
        let answers = [("big", 413, ""), ("auth", 401, ""), ("broken", 500, "Sabre exception: secret detail")];
        for (name, status, body) in answers {
            server
                .mock_async(|when, then| {
                    when.method(PUT).path(format!("{BOOK_PATH}{name}.vcf"));
                    then.status(status).body(body);
                })
                .await;
        }
        let client = client(&server);
        let book = server.url(BOOK_PATH);
        assert_eq!(client.put(&server.url(shared), None, "new", VCARD, None).await, Err(ContactsError::ReadOnly));
        let put = |name: &'static str| client.put(&book, None, name, VCARD, None);
        assert_eq!(put("big").await, Err(ContactsError::TooLarge));
        assert_eq!(put("auth").await, Err(ContactsError::Auth));
        let broken = put("broken").await.expect_err("refused");
        assert_eq!(broken, ContactsError::Other("the server refused the change (HTTP 500)".into()));
        assert!(!broken.to_string().contains("secret"));
    }

    #[tokio::test]
    async fn deleting_a_card_gone_already_succeeds() {
        let server = MockServer::start_async().await;
        let guarded = server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{BOOK_PATH}gone.vcf")).header("If-Match", "\"g1\"");
                then.status(404);
            })
            .await;
        let blind = server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{BOOK_PATH}other.vcf")).header_missing("If-Match");
                then.status(204);
            })
            .await;
        let client = client(&server);
        let book = server.url(BOOK_PATH);
        assert_eq!(client.delete(&book, &card(&server, "gone.vcf"), Some("\"g1\"")).await, Ok(()));
        assert_eq!(client.delete(&book, "other.vcf", None).await, Ok(()));
        assert_eq!((guarded.calls_async().await, blind.calls_async().await), (1, 1));
    }

    #[tokio::test]
    async fn addresses_off_the_book_are_refused_before_any_request() {
        let server = MockServer::start_async().await;
        let any = server
            .mock_async(|when, then| {
                when.path_includes("/");
                then.status(204);
            })
            .await;
        let client = client(&server);
        let book = server.url(BOOK_PATH);
        let foreign = "https://evil.example.org/remote.php/dav/addressbooks/users/anna/contacts/ben.vcf";
        let outside = "/remote.php/dav/addressbooks/users/anna/other/ben.vcf";
        for href in [foreign, outside, "sub/ben.vcf", "../ben.vcf", ""] {
            assert!(matches!(client.delete(&book, href, Some("\"e\"")).await, Err(ContactsError::Other(_))), "{href}");
            let put = client.put(&book, Some(href), "ben", VCARD, Some("\"e\"")).await;
            assert!(matches!(put, Err(ContactsError::Other(_))), "{href}");
        }
        let elsewhere = "https://evil.example.org/remote.php/dav/addressbooks/users/anna/contacts/";
        assert!(matches!(client.changes(elsewhere, None).await, Err(ContactsError::Other(_))));
        assert!(matches!(client.fetch(elsewhere, &["x.vcf".into()]).await, Err(ContactsError::Other(_))));
        let without_etag = client.put(&book, Some("ben.vcf"), "ben", VCARD, None).await;
        assert!(matches!(without_etag, Err(ContactsError::Other(_))), "never overwrite blindly");
        assert!(matches!(client.put(&book, None, " ", VCARD, None).await, Err(ContactsError::Other(_))));
        assert_eq!(any.calls_async().await, 0);
    }

    #[tokio::test]
    async fn an_unreachable_server_is_offline() {
        // A port that was free a moment ago: nothing listens there now (a dropped `MockServer`
        // goes back to httpmock's pool and keeps answering).
        let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).expect("a free port").port();
        let account = CalDavAccount { url: format!("http://127.0.0.1:{port}/dav/"), username: "anna".into() };
        let client = CalDavClient::new(reqwest::Client::new(), &account, Secret::new("secret")).expect("client");
        assert_eq!(client.address_books().await, Err(ContactsError::Offline));
    }
}
