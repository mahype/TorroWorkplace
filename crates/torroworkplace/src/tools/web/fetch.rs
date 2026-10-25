//! Reading one web page for the tool web (task WEB.3, `docs/assistent.md` "Seite lesen", decision 23): the address
//! is checked, the page fetched only from the public internet and its main content given as Markdown
//! ([`WebPage`]). The command `tools.fetch_page` (task WEB.4) calls [`Fetcher::fetch`].
//!
//! The model picks addresses, so reading a page must not turn into a probe of the person's network. The guards are
//! those of the service discovery (`torro_pim_caldav::discover`):
//!
//! - **Address** ([`checked_url`]): `http`/`https` only, a host name ([`checked_domain`]: no IP literals, no
//!   `localhost`, `.local` or single labels), no `user:pass@`, ports 80 and 443 only, at most [`MAX_URL_BYTES`].
//! - **Connection:** every name is resolved and refused unless all its addresses are public ([`is_public`]); the
//!   HTTP client resolves through [`GuardedDns`] with the same rule and no proxy, so a name cannot resolve one way
//!   for the check and another for the connection.
//! - **Redirects** by hand, at most [`MAX_REDIRECTS`], each address checked anew, and against `robots.txt` too.
//! - **Limits:** [`CONNECT_TIMEOUT`], [`DEADLINE`] for everything, [`MAX_BODY_BYTES`] streamed, only
//!   [`TYPES`], one read per host and [`HOST_GAP`].
//! - **`robots.txt`** ([`super::robots`]) holds for addresses the model picked (`by_person: false`); an address
//!   the person wrote is read like a browser would.
//!
//! Nothing here logs, and no error carries the address or the page: [`FetchError`] has fixed texts only, and a
//! `reqwest::Error` loses its URL before anything looks at it.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dom_smoothie::{Config, Readability, TextMode};
use reqwest::Url;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, LOCATION};
use torro_ipc::web::{MAX_PAGE_CHARS, MAX_URL_BYTES, WebPage};
use torro_pim_caldav::discover::{GuardedDns, Resolver, SystemResolver, checked_domain, is_public};

use super::robots::{self, Robots, RobotsCache};

/// How many redirects one read follows (the same as the service discovery).
pub const MAX_REDIRECTS: usize = 5;

/// How long connecting to a server may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one read may take altogether: `robots.txt`, the turn of the host, redirects and the body.
pub const DEADLINE: Duration = Duration::from_secs(20);

/// The most bytes of a page read; a longer one is refused ([`FetchError::TooLarge`]).
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// The least time between two reads of pages on one host. Redirects within one read and `robots.txt` are part of
/// that read.
pub const HOST_GAP: Duration = Duration::from_secs(1);

/// The media types read; anything else is refused ([`FetchError::WrongType`]).
pub const TYPES: [&str; 3] = ["text/html", "application/xhtml+xml", "text/plain"];

/// The `Accept` header of a page.
const ACCEPTED: &str = "text/html, application/xhtml+xml, text/plain;q=0.9";

/// Why a page was not read. Nothing of the address or the page is in it; [`FetchError::code`] and
/// [`FetchError::reason`] give the answer of `tools.fetch_page`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    /// The address is longer than [`MAX_URL_BYTES`].
    TooLong,
    /// Not an absolute `http` or `https` address.
    NotWeb,
    /// The address holds a user name or a password.
    Credentials,
    /// The host is an IP address (in any spelling) instead of a name.
    IpAddress,
    /// The host is no name on the internet (`localhost`, `nas.local`, a single label).
    NoDomain,
    /// A port other than 80 and 443.
    Port,
    /// The name leads to an address that is not on the public internet (the person's network, loopback).
    NotPublic,
    /// The site's `robots.txt` does not allow [`robots::AGENT`] this page (only for addresses of the model).
    Robots,
    /// More than [`MAX_REDIRECTS`] redirects.
    TooManyRedirects,
    /// The page is none of [`TYPES`].
    WrongType,
    /// The page is longer than [`MAX_BODY_BYTES`].
    TooLarge,
    /// The site answered with this HTTP status instead of the page.
    Status(u16),
    /// The site cannot be reached: no such name, no connection, or [`DEADLINE`] passed.
    Offline,
    /// Anything else (a broken answer, no HTTP client).
    Failed,
}

impl FetchError {
    /// The error code of `tools.fetch_page`: `unsupported` for an address or page the tool does not read (the
    /// address checks, `robots.txt`, redirects, type, size), `not-found` for 404 and 410, `offline` for
    /// [`FetchError::Offline`], else `error`.
    pub fn code(self) -> &'static str {
        match self {
            FetchError::Status(404 | 410) => "not-found",
            FetchError::Status(_) | FetchError::Failed => "error",
            FetchError::Offline => "offline",
            _ => "unsupported",
        }
    }

    /// Why, in English for people, one sentence without the address; the core translates it
    /// (`torro_core::i18n::tr`).
    pub fn reason(self) -> &'static str {
        match self {
            FetchError::TooLong => "The address is too long.",
            FetchError::NotWeb => "Only web addresses starting with http or https can be read.",
            FetchError::Credentials => "Addresses with a user name or password are not read.",
            FetchError::IpAddress => "Addresses with an IP address are not read, only those with a name.",
            FetchError::NoDomain => "The address names no host on the internet.",
            FetchError::Port => "Only addresses on the standard ports 80 and 443 are read.",
            FetchError::NotPublic => "The address leads into a local network, which is not read.",
            FetchError::Robots => "The site's robots.txt does not allow Torro to read this page.",
            FetchError::TooManyRedirects => "The page redirects more than five times.",
            FetchError::WrongType => "The page is neither HTML nor text.",
            FetchError::TooLarge => "The page is larger than 2 MB.",
            FetchError::Status(404 | 410) => "The site has no such page.",
            FetchError::Status(_) => "The site answered with an error.",
            FetchError::Offline => "The site cannot be reached.",
            FetchError::Failed => "The page could not be read.",
        }
    }
}

/// Which addresses a [`Fetcher`] may connect to and which port it accepts besides 80 and 443. Always
/// [`Guard::PUBLIC`] in the service; tests allow loopback and the port of their mock server.
#[derive(Debug, Clone, Copy)]
pub struct Guard {
    /// Whether an address of a resolved name may be connected to.
    pub allowed: fn(IpAddr) -> bool,
    /// One more port an address may name; `None` in the service.
    pub port: Option<u16>,
}

impl Guard {
    /// The public internet on ports 80 and 443.
    pub const PUBLIC: Guard = Guard { allowed: is_public, port: None };
}

/// `input` as an address the tool may read, after the module docs, without its fragment; else why not.
pub fn checked_url(input: &str, guard: Guard) -> Result<Url, FetchError> {
    let input = input.trim();
    if input.len() > MAX_URL_BYTES {
        return Err(FetchError::TooLong);
    }
    let mut url = Url::parse(input).map_err(|_| FetchError::NotWeb)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FetchError::NotWeb);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FetchError::Credentials);
    }
    // The URL parser reads every spelling of an IPv4 address (`0x7f.1`, `2130706433`) as one: no domain then.
    match (url.host_str(), url.domain()) {
        (None, _) => return Err(FetchError::NoDomain),
        (Some(_), None) => return Err(FetchError::IpAddress),
        (Some(_), Some(domain)) if checked_domain(domain).is_none() => return Err(FetchError::NoDomain),
        _ => {}
    }
    let port = url.port_or_known_default().ok_or(FetchError::Port)?;
    if !(port == 80 || port == 443 || Some(port) == guard.port) {
        return Err(FetchError::Port);
    }
    url.set_fragment(None);
    if url.as_str().len() > MAX_URL_BYTES {
        return Err(FetchError::TooLong);
    }
    Ok(url)
}

/// What one request found.
enum Step {
    /// The answer, not a redirect.
    Answer(reqwest::Response),
    /// A redirect to this address, already checked.
    Redirect(Url),
}

/// Reads web pages for the whole service: one, so that the turns of the hosts and the kept `robots.txt` hold for
/// the assistant and every client alike.
pub struct Fetcher {
    resolver: Arc<dyn Resolver>,
    guard: Guard,
    http: reqwest::Client,
    robots: Mutex<RobotsCache>,
    /// When each host may be read next.
    turns: Mutex<HashMap<String, Instant>>,
}

impl Fetcher {
    /// The guard of this fetcher: where it connects, and which port beside 80 and 443 it reads (tests).
    pub fn guard(&self) -> Guard {
        self.guard
    }

    /// The fetcher of the service: the system's DNS ([`SystemResolver`]) and [`Guard::PUBLIC`]. `None` when no HTTP
    /// client can be built.
    pub fn system() -> Option<Fetcher> {
        Fetcher::new(Arc::new(SystemResolver::new()), Guard::PUBLIC)
    }

    /// A fetcher resolving every name through `resolver` and connecting only where `guard` allows (tests: a fake
    /// resolver pointing names at a mock server on loopback). `None` when no HTTP client can be built.
    pub fn new(resolver: Arc<dyn Resolver>, guard: Guard) -> Option<Fetcher> {
        let guarded = GuardedDns::new(Arc::clone(&resolver), guard.allowed);
        let http = reqwest::Client::builder()
            .user_agent(crate::places::USER_AGENT)
            // A proxy would resolve the names itself, past the guard.
            .no_proxy()
            // By hand, so every address is checked (and asked about robots.txt) before it is asked.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DEADLINE)
            .dns_resolver(Arc::new(guarded))
            .build()
            .ok()?;
        Some(Fetcher {
            resolver,
            guard,
            http,
            robots: Mutex::new(RobotsCache::default()),
            turns: Mutex::new(HashMap::new()),
        })
    }

    /// Reads the page at `url`: checks the address ([`checked_url`]), asks the site's `robots.txt` unless
    /// `by_person` (the person wrote the address in the conversation; `tools.fetch_page` from any client is
    /// `false`), waits for the host's turn, follows at most [`MAX_REDIRECTS`] redirects (each checked, and asked
    /// about `robots.txt` unless `by_person`) and reads at most [`MAX_BODY_BYTES`] of [`TYPES`], all within
    /// [`DEADLINE`].
    ///
    /// The answer has the address after redirects, the page's title (empty when it has none) and its main content
    /// as Markdown with absolute links ([`extract`]; a text page as it is), cut at [`MAX_PAGE_CHARS`] characters
    /// with `truncated`. `untrusted` is always `true`. Nothing is logged.
    pub async fn fetch(&self, url: &str, by_person: bool) -> Result<WebPage, FetchError> {
        let url = checked_url(url, self.guard)?;
        tokio::time::timeout(DEADLINE, self.read(url, by_person)).await.unwrap_or(Err(FetchError::Offline))
    }

    async fn read(&self, url: Url, by_person: bool) -> Result<WebPage, FetchError> {
        let (url, response) = self.follow(url, by_person).await?;
        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        let (kind, charset) = content_type(response.headers()).ok_or(FetchError::WrongType)?;
        if !TYPES.contains(&kind.as_str()) {
            return Err(FetchError::WrongType);
        }
        let body = body(response, MAX_BODY_BYTES, false).await?;
        let html = kind != "text/plain";
        let text = decoded(&body, charset.as_deref(), html);
        let page_url = url.clone();
        // Taking a page apart is work for a while; not on the threads that serve requests.
        let (title, markdown) = tokio::task::spawn_blocking(move || match html {
            true => extract(&text, &page_url),
            false => (String::new(), text.trim().to_string()),
        })
        .await
        .map_err(|_| FetchError::Failed)?;
        let (markdown, truncated) = cut(markdown);
        Ok(WebPage::new(url.to_string(), title, markdown, truncated))
    }

    /// Asks the page at `url` and follows its redirects, at most [`MAX_REDIRECTS`]: every address checked
    /// ([`checked_url`], public addresses) and, unless `by_person`, asked about `robots.txt` first; the first request
    /// after the host's turn. The last address and its answer (not a redirect).
    async fn follow(&self, mut url: Url, by_person: bool) -> Result<(Url, reqwest::Response), FetchError> {
        for redirects in 0..=MAX_REDIRECTS {
            if !by_person && !self.robots_allow(&url).await {
                return Err(FetchError::Robots);
            }
            if redirects == 0 {
                let wait = self.turn(url.host_str().unwrap_or_default(), Instant::now());
                tokio::time::sleep(wait).await;
            }
            match self.step(&url, ACCEPTED).await? {
                Step::Answer(response) => return Ok((url, response)),
                Step::Redirect(next) => url = next,
            }
        }
        Err(FetchError::TooManyRedirects)
    }

    /// One request to `url` (its host checked first): the answer, or the checked address it redirects to.
    async fn step(&self, url: &Url, accept: &str) -> Result<Step, FetchError> {
        self.check_host(url).await?;
        let response = self
            .http
            .get(url.clone())
            .header(ACCEPT, accept)
            .send()
            .await
            .map_err(|err| failure(&err.without_url()))?;
        if !response.status().is_redirection() {
            return Ok(Step::Answer(response));
        }
        let next = response
            .headers()
            .get(LOCATION)
            .and_then(|location| location.to_str().ok())
            .and_then(|location| url.join(location).ok())
            .ok_or(FetchError::Status(response.status().as_u16()))?;
        Ok(Step::Redirect(checked_url(next.as_str(), self.guard)?))
    }

    /// Resolves the host of `url` and refuses it unless every address passes the guard. The client resolves once
    /// more through [`GuardedDns`] with the same rule; this check is the one that can say why.
    async fn check_host(&self, url: &Url) -> Result<(), FetchError> {
        let host = url.host_str().ok_or(FetchError::NoDomain)?;
        let ips = self.resolver.ips(host).await.map_err(|_| FetchError::Offline)?;
        if ips.is_empty() {
            return Err(FetchError::Offline);
        }
        if !ips.iter().all(|ip| (self.guard.allowed)(*ip)) {
            return Err(FetchError::NotPublic);
        }
        Ok(())
    }

    /// How long a read of `host` at `now` waits for its turn (zero when it is free); the turn after it comes
    /// [`HOST_GAP`] later.
    fn turn(&self, host: &str, now: Instant) -> Duration {
        let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns.retain(|_, next| *next > now);
        let next = turns.get(host).copied().unwrap_or(now);
        turns.insert(host.to_string(), next + HOST_GAP);
        next.saturating_duration_since(now)
    }

    /// Whether the site's `robots.txt` lets [`robots::AGENT`] read `url`; kept per origin for
    /// [`robots::KEEP_FOR`].
    async fn robots_allow(&self, url: &Url) -> bool {
        let origin = url.origin().ascii_serialization();
        let known = self.robots.lock().unwrap_or_else(PoisonError::into_inner).get(&origin, Instant::now());
        let robots = match known {
            Some(robots) => robots,
            None => {
                let (robots, keep) = self.robots_of(url).await;
                if keep {
                    self.robots.lock().unwrap_or_else(PoisonError::into_inner).put(
                        &origin,
                        robots.clone(),
                        Instant::now(),
                    );
                }
                robots
            }
        };
        let path = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_string(),
        };
        robots.allows(&path)
    }

    /// The `robots.txt` of the origin of `url` (RFC 9309 §2.3), and whether to keep it: an answer is kept, a server
    /// error or no answer is not (the next read asks again). More than [`MAX_REDIRECTS`] redirects count as no
    /// file (§2.3.1.2), an address the guards refuse or a server that cannot be reached as one that allows nothing.
    async fn robots_of(&self, url: &Url) -> (Robots, bool) {
        let mut address = url.clone();
        address.set_path("/robots.txt");
        address.set_query(None);
        for _ in 0..=MAX_REDIRECTS {
            match self.step(&address, "text/plain").await {
                Ok(Step::Answer(response)) => return robots_answer(response).await,
                Ok(Step::Redirect(next)) => address = next,
                Err(_) => return (Robots::DisallowAll, false),
            }
        }
        (Robots::AllowAll, true)
    }
}

/// What a site's answer for `robots.txt` says, and whether to keep it (not after a server error).
async fn robots_answer(response: reqwest::Response) -> (Robots, bool) {
    let status = response.status().as_u16();
    let body = match response.status().is_success() {
        true => body(response, robots::MAX_BYTES, true).await,
        false => Ok(Vec::new()),
    };
    match body {
        Ok(body) => (Robots::for_status(status, &body), status < 500 && status != 429),
        Err(_) => (Robots::DisallowAll, false),
    }
}

/// The media type of `headers` in lower case and its `charset`, when it names one.
fn content_type(headers: &HeaderMap) -> Option<(String, Option<String>)> {
    let value = headers.get(CONTENT_TYPE)?.to_str().ok()?.to_ascii_lowercase();
    let mut parts = value.split(';');
    let kind = parts.next()?.trim().to_string();
    let charset = parts
        .filter_map(|part| part.trim().strip_prefix("charset="))
        .map(|charset| charset.trim_matches(|c| c == '"' || c == '\'').to_string())
        .next();
    Some((kind, charset))
}

/// The body of `response`, at most `max` bytes: with `cut` the first `max` (a `robots.txt`), else
/// [`FetchError::TooLarge`] when it is longer.
async fn body(mut response: reqwest::Response, max: usize, cut: bool) -> Result<Vec<u8>, FetchError> {
    if !cut && response.content_length().is_some_and(|length| length > max as u64) {
        return Err(FetchError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| failure(&err.without_url()))? {
        if body.len() + chunk.len() > max {
            if !cut {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&chunk[..max - body.len()]);
            break;
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// A failed request; `err` has lost its URL already.
fn failure(err: &reqwest::Error) -> FetchError {
    if err.is_connect() || err.is_timeout() { FetchError::Offline } else { FetchError::Failed }
}

/// `body` as text in the character set of `charset` (from `Content-Type`), else, for HTML, of a `<meta>` near the
/// start, else UTF-8; a byte-order mark wins over both. Broken sequences become U+FFFD.
fn decoded(body: &[u8], charset: Option<&str>, html: bool) -> String {
    let named = charset.map(str::to_string).or_else(|| html.then(|| meta_charset(body)).flatten());
    let encoding =
        named.and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes())).unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(body);
    text.into_owned()
}

/// The `charset=` of a `<meta charset>` or `<meta http-equiv>` in the first 1024 bytes of an HTML page.
fn meta_charset(body: &[u8]) -> Option<String> {
    let start = String::from_utf8_lossy(&body[..body.len().min(1024)]).to_ascii_lowercase();
    let after = &start[start.find("charset=")? + "charset=".len()..];
    let label: String = after
        .trim_start_matches(['"', '\''])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
        .collect();
    (!label.is_empty()).then_some(label)
}

/// The title and the main content of the HTML page at `url` as Markdown, links made absolute against `url`
/// (Mozilla's Readability through `dom_smoothie`). When it finds no content: the `<title>` and the text of
/// `<body>` without scripts and styles.
pub fn extract(html: &str, url: &Url) -> (String, String) {
    let config = Config { text_mode: TextMode::Markdown, ..Config::default() };
    if let Ok(mut readability) = Readability::new(html, Some(url.as_str()), Some(config))
        && let Ok(article) = readability.parse()
    {
        let markdown = article.text_content.trim();
        if !markdown.is_empty() {
            return (folded(&article.title), markdown.to_string());
        }
    }
    body_text(html)
}

/// The `<title>` and the text of `<body>` of `html`, without scripts and styles: what [`extract`] gives when
/// Readability finds no content.
fn body_text(html: &str) -> (String, String) {
    let document = dom_query::Document::from(html);
    document.select("script, style, noscript, template").remove();
    let title = folded(&document.select("title").text());
    let text = document.select("body").formatted_text();
    (title, text.trim().to_string())
}

/// `text` with every run of whitespace as one space, trimmed.
fn folded(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `markdown` cut at [`MAX_PAGE_CHARS`] characters, and whether it was.
fn cut(markdown: String) -> (String, bool) {
    match markdown.char_indices().nth(MAX_PAGE_CHARS) {
        Some((at, _)) => (markdown[..at].to_string(), true),
        None => (markdown, false),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use httpmock::Method::GET;
    use httpmock::MockServer;
    use torro_pim_caldav::discover::{Lookup, MxRecord, SrvRecord};

    use super::*;
    use crate::tools::web::providers::testing::fixture;

    /// A DNS that knows a few hosts.
    struct FakeDns(HashMap<String, Vec<IpAddr>>);

    impl Resolver for FakeDns {
        fn srv<'a>(&'a self, _name: &'a str) -> Lookup<'a, SrvRecord> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn txt<'a>(&'a self, _name: &'a str) -> Lookup<'a, String> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn mx<'a>(&'a self, _name: &'a str) -> Lookup<'a, MxRecord> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn ips<'a>(&'a self, host: &'a str) -> Lookup<'a, IpAddr> {
            Box::pin(async move { Ok(self.0.get(host).cloned().unwrap_or_default()) })
        }
    }

    /// The tests' rule: the public internet and, since the mock server lives there, loopback.
    fn public_or_loopback(ip: IpAddr) -> bool {
        is_public(ip) || ip.is_loopback()
    }

    /// A fetcher whose DNS points `hosts` (name, address) at their addresses and that accepts the mock server's
    /// `port`.
    fn on_mock(server: &MockServer, hosts: &[(&str, &str)]) -> Fetcher {
        let table = hosts.iter().map(|(host, ip)| (host.to_string(), vec![ip.parse().expect("an IP")])).collect();
        let guard = Guard { allowed: public_or_loopback, port: Some(server.port()) };
        Fetcher::new(Arc::new(FakeDns(table)), guard).expect("a fetcher")
    }

    /// `path` on `host` at the mock server.
    fn at(server: &MockServer, host: &str, path: &str) -> String {
        format!("http://{host}:{}{path}", server.port())
    }

    #[test]
    fn addresses_are_checked_before_anything_is_asked() {
        let ok: &[(&str, &str)] = &[
            ("https://example.org/a?b=c#top", "https://example.org/a?b=c"),
            (" http://Example.ORG:80/ ", "http://example.org/"),
            ("https://example.org:443/x", "https://example.org/x"),
            ("http://example.org:443/", "http://example.org:443/"),
            ("https://bücher.de/", "https://xn--bcher-kva.de/"),
        ];
        for (input, url) in ok {
            assert_eq!(checked_url(input, Guard::PUBLIC).map(String::from), Ok(url.to_string()), "{input}");
        }
        let long = format!("https://example.org/{}", "a".repeat(MAX_URL_BYTES));
        let refused: &[(&str, FetchError)] = &[
            (&long, FetchError::TooLong),
            ("example.org/page", FetchError::NotWeb),
            ("ftp://example.org/", FetchError::NotWeb),
            ("file:///etc/passwd", FetchError::NotWeb),
            ("javascript:alert(1)", FetchError::NotWeb),
            ("data:text/html,<p>x</p>", FetchError::NotWeb),
            ("https://anna:secret@example.org/", FetchError::Credentials),
            ("https://anna@example.org/", FetchError::Credentials),
            ("http://127.0.0.1/", FetchError::IpAddress),
            ("http://192.168.178.1/admin", FetchError::IpAddress),
            ("http://10.0.0.1/", FetchError::IpAddress),
            ("http://[::1]/", FetchError::IpAddress),
            ("http://[fd00::1]/", FetchError::IpAddress),
            ("http://0x7f.1/", FetchError::IpAddress),
            ("http://2130706433/", FetchError::IpAddress),
            ("http://93.184.215.14/", FetchError::IpAddress),
            ("http://localhost/", FetchError::NoDomain),
            ("http://nas.local/", FetchError::NoDomain),
            ("http://router.lan/", FetchError::NoDomain),
            ("http://intranet/", FetchError::NoDomain),
            ("https://example.org:8080/", FetchError::Port),
            ("http://example.org:22/", FetchError::Port),
        ];
        for (input, why) in refused {
            assert_eq!(checked_url(input, Guard::PUBLIC), Err(*why), "{input}");
        }
        let test = Guard { allowed: public_or_loopback, port: Some(8080) };
        assert!(checked_url("https://example.org:8080/", test).is_ok(), "the tests' port");
    }

    #[test]
    fn every_error_has_a_code_and_a_sentence_without_the_address() {
        let all = [
            FetchError::TooLong,
            FetchError::NotWeb,
            FetchError::Credentials,
            FetchError::IpAddress,
            FetchError::NoDomain,
            FetchError::Port,
            FetchError::NotPublic,
            FetchError::Robots,
            FetchError::TooManyRedirects,
            FetchError::WrongType,
            FetchError::TooLarge,
            FetchError::Status(404),
            FetchError::Status(500),
            FetchError::Offline,
            FetchError::Failed,
        ];
        for error in all {
            assert!(error.reason().ends_with('.') && !error.reason().contains("://"), "{error:?}");
            assert!(["unsupported", "not-found", "offline", "error"].contains(&error.code()), "{error:?}");
            let german = torro_core::i18n::tr_in(torro_core::i18n::Lang::De, error.reason());
            assert_ne!(german, error.reason(), "{error:?} is translated");
        }
        assert_eq!(FetchError::NotPublic.code(), "unsupported");
        assert_eq!(FetchError::Robots.code(), "unsupported");
        assert_eq!(FetchError::Status(404).code(), "not-found");
        assert_eq!(FetchError::Offline.code(), "offline");
    }

    #[test]
    fn the_main_content_of_an_article_is_markdown_without_navigation_and_scripts() {
        let html = String::from_utf8(fixture("article.html")).expect("utf-8");
        let url = Url::parse("https://news.example.org/trams/lisbon").expect("url");
        let (title, markdown) = extract(&html, &url);
        assert_eq!(title, "The trams of Lisbon");
        assert!(markdown.contains("line 28 winds from Martim Moniz"), "{markdown}");
        assert!(markdown.contains("## Riding line 28"), "{markdown}");
        assert!(markdown.contains("](https://news.example.org/trams/history)"), "links absolute: {markdown}");
        assert!(markdown.contains("https://news.example.org/trams/images/tram28.jpg"), "{markdown}");
        for left_out in
            ["menu entry", "Sign in", "cookies", "TRACKER-SECRET", "PAGE-SCRIPT-TEXT", "font-family", "Copyright"]
        {
            assert!(!markdown.contains(left_out), "{left_out:?} in {markdown}");
        }
    }

    #[test]
    fn a_short_page_gives_its_text_without_scripts() {
        let html = "<html><head><title> Status </title><script>var hidden = 1;</script></head>\
                    <body><p>All systems go.</p><style>p { color: red }</style></body></html>";
        let url = Url::parse("https://status.example.org/").expect("url");
        let (title, markdown) = extract(html, &url);
        assert_eq!(title, "Status");
        assert!(markdown.contains("All systems go"), "{markdown}");
        assert!(!markdown.contains("hidden") && !markdown.contains("color"), "{markdown}");
        // What is left when Readability finds nothing.
        let (title, text) = body_text(html);
        assert_eq!((title.as_str(), text.as_str()), ("Status", "All systems go."));
    }

    #[test]
    fn long_pages_are_cut_and_marked() {
        let (short, cut_short) = cut("ä".repeat(10));
        assert_eq!((short.chars().count(), cut_short), (10, false));
        let (long, cut_long) = cut("ä".repeat(MAX_PAGE_CHARS + 5));
        assert_eq!((long.chars().count(), cut_long), (MAX_PAGE_CHARS, true));
    }

    #[test]
    fn character_sets_are_read_from_the_header_and_the_page() {
        let latin1 = b"Gr\xfc\xdfe aus Lissabon";
        assert_eq!(decoded(latin1, Some("iso-8859-1"), false), "Grüße aus Lissabon");
        let page = b"<html><head><meta charset=\"windows-1252\"></head><body>Gr\xfc\xdfe</body></html>";
        assert!(decoded(page, None, true).contains("Grüße"));
        assert_eq!(decoded("Grüße".as_bytes(), None, true), "Grüße");
        assert_eq!(decoded(b"\xef\xbb\xbfplain", Some("iso-8859-1"), false), "plain", "a BOM wins");
    }

    #[test]
    fn a_read_can_run_on_any_thread_of_the_service() {
        fn on_any_thread<T: Send>(_: T) {}
        let fetcher = Fetcher::new(Arc::new(FakeDns(HashMap::new())), Guard::PUBLIC).expect("a fetcher");
        on_any_thread(fetcher.fetch("https://example.org/", false));
        on_any_thread(Arc::new(fetcher));
    }

    #[test]
    fn each_host_is_read_once_a_second() {
        let fetcher = Fetcher::new(Arc::new(FakeDns(HashMap::new())), Guard::PUBLIC).expect("a fetcher");
        let now = Instant::now();
        assert_eq!(fetcher.turn("example.org", now), Duration::ZERO);
        assert_eq!(fetcher.turn("example.org", now), HOST_GAP);
        assert_eq!(fetcher.turn("example.org", now), HOST_GAP * 2, "one after the other");
        assert_eq!(fetcher.turn("other.example.org", now), Duration::ZERO, "another host");
        assert_eq!(fetcher.turn("example.org", now + HOST_GAP * 4), Duration::ZERO, "later");
    }

    #[tokio::test]
    async fn a_page_is_read_with_the_suite_user_agent() {
        let server = MockServer::start_async().await;
        let page = server
            .mock_async(|when, then| {
                when.method(GET).path("/trams/lisbon").header("user-agent", crate::places::USER_AGENT);
                then.status(200).header("content-type", "text/html; charset=utf-8").body(fixture("article.html"));
            })
            .await;
        let fetcher = on_mock(&server, &[("news.example.org", "127.0.0.1")]);
        let url = at(&server, "news.example.org", "/trams/lisbon#top");
        let read = fetcher.fetch(&url, true).await.expect("the page");
        assert_eq!(read.url, at(&server, "news.example.org", "/trams/lisbon"));
        assert_eq!(read.title, "The trams of Lisbon");
        assert!(read.markdown.contains(&format!("]({})", at(&server, "news.example.org", "/trams/history"))));
        assert!(read.untrusted && !read.truncated);
        assert_eq!(page.calls_async().await, 1);
    }

    #[tokio::test]
    async fn a_name_or_a_redirect_into_the_local_network_is_refused() {
        let server = MockServer::start_async().await;
        let port = server.port();
        let admin = server
            .mock_async(|when, then| {
                when.method(GET).path("/admin");
                then.status(200).header("content-type", "text/html").body("<p>router admin</p>");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/to-router");
                then.status(302).header("Location", format!("http://router.example.org:{port}/admin"));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/to-loopback");
                then.status(302).header("Location", format!("http://127.0.0.1:{port}/admin"));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/to-file");
                then.status(301).header("Location", "file:///etc/passwd");
            })
            .await;
        // The router's name leads into the person's network; the mock server answers on loopback for every name,
        // so only the guard keeps `/admin` from being asked.
        let hosts = [("news.example.org", "127.0.0.1"), ("router.example.org", "192.168.178.1")];
        let fetcher = on_mock(&server, &hosts);
        let refused: &[(&str, FetchError)] = &[
            ("/to-router", FetchError::NotPublic),
            ("/to-loopback", FetchError::IpAddress),
            ("/to-file", FetchError::NotWeb),
        ];
        for (path, why) in refused {
            assert_eq!(fetcher.fetch(&at(&server, "news.example.org", path), true).await, Err(*why), "{path}");
        }
        let direct = fetcher.fetch(&at(&server, "router.example.org", "/admin"), true).await;
        assert_eq!(direct, Err(FetchError::NotPublic));
        let unknown = fetcher.fetch(&at(&server, "nowhere.example.org", "/admin"), true).await;
        assert_eq!(unknown, Err(FetchError::Offline), "no such name");
        assert_eq!(admin.calls_async().await, 0);
    }

    #[tokio::test]
    async fn at_most_five_redirects_are_followed() {
        let server = MockServer::start_async().await;
        for n in 0..6 {
            server
                .mock_async(|when, then| {
                    when.method(GET).path(format!("/hop/{n}"));
                    then.status(302).header("Location", format!("/hop/{}", n + 1));
                })
                .await;
        }
        server
            .mock_async(|when, then| {
                when.method(GET).path("/hop/6");
                then.status(200).header("content-type", "text/plain").body("arrived");
            })
            .await;
        let fetcher = on_mock(&server, &[("a.example.org", "127.0.0.1"), ("b.example.org", "127.0.0.1")]);
        let five = fetcher.fetch(&at(&server, "a.example.org", "/hop/1"), true).await.expect("five redirects");
        assert_eq!((five.markdown.as_str(), five.url), ("arrived", at(&server, "a.example.org", "/hop/6")));
        let six = fetcher.fetch(&at(&server, "b.example.org", "/hop/0"), true).await;
        assert_eq!(six, Err(FetchError::TooManyRedirects));
    }

    #[tokio::test]
    async fn robots_txt_holds_for_the_model_and_not_for_the_person() {
        let server = MockServer::start_async().await;
        let robots = server
            .mock_async(|when, then| {
                when.method(GET).path("/robots.txt");
                then.status(200)
                    .header("content-type", "text/plain")
                    .body("User-agent: *\nDisallow: /private\n\nUser-agent: TorroWorkplace\nDisallow: /no-torro\n");
            })
            .await;
        for path in ["/private/page", "/no-torro/page", "/open/page"] {
            server
                .mock_async(|when, then| {
                    when.method(GET).path(path);
                    then.status(200).header("content-type", "text/plain").body(format!("page {path}"));
                })
                .await;
        }
        server
            .mock_async(|when, then| {
                when.method(GET).path("/old");
                then.status(301).header("Location", "/no-torro/page");
            })
            .await;
        let fetcher = on_mock(&server, &[("site.example.org", "127.0.0.1")]);
        let url = |path| at(&server, "site.example.org", path);
        // Our group holds, not the one for all.
        assert_eq!(fetcher.fetch(&url("/no-torro/page"), false).await, Err(FetchError::Robots));
        assert_eq!(fetcher.fetch(&url("/old"), false).await, Err(FetchError::Robots), "a redirect is asked too");
        assert!(fetcher.fetch(&url("/no-torro/page"), true).await.is_ok(), "the person wrote it");
        assert!(fetcher.fetch(&url("/private/page"), false).await.is_ok());
        assert_eq!(robots.calls_async().await, 1, "kept for the origin");
    }

    #[tokio::test]
    async fn a_missing_robots_txt_allows_and_a_broken_server_does_not() {
        let missing = MockServer::start_async().await;
        missing
            .mock_async(|when, then| {
                when.method(GET).path("/robots.txt");
                then.status(404);
            })
            .await;
        missing
            .mock_async(|when, then| {
                when.method(GET).path("/page");
                then.status(200).header("content-type", "text/plain").body("hello");
            })
            .await;
        let fetcher = on_mock(&missing, &[("missing.example.org", "127.0.0.1")]);
        assert!(fetcher.fetch(&at(&missing, "missing.example.org", "/page"), false).await.is_ok());

        let broken = MockServer::start_async().await;
        let robots = broken
            .mock_async(|when, then| {
                when.method(GET).path("/robots.txt");
                then.status(503);
            })
            .await;
        let fetcher = on_mock(&broken, &[("broken.example.org", "127.0.0.1")]);
        let url = at(&broken, "broken.example.org", "/page");
        assert_eq!(fetcher.fetch(&url, false).await, Err(FetchError::Robots));
        assert_eq!(fetcher.fetch(&url, false).await, Err(FetchError::Robots));
        assert_eq!(robots.calls_async().await, 2, "a server error is not kept");
    }

    #[tokio::test]
    async fn only_html_and_text_of_at_most_2_mb_are_read() {
        let server = MockServer::start_async().await;
        let answers: [(&str, Option<&str>, Vec<u8>); 5] = [
            ("/report.pdf", Some("application/pdf"), b"%PDF-1.7".to_vec()),
            ("/untyped", None, b"<p>x</p>".to_vec()),
            ("/huge", Some("text/plain"), vec![b'a'; MAX_BODY_BYTES + 1]),
            ("/long", Some("text/plain"), "ä".repeat(MAX_PAGE_CHARS + 10).into_bytes()),
            ("/latin1", Some("text/plain; charset=ISO-8859-1"), b"Gr\xfc\xdfe".to_vec()),
        ];
        for (path, kind, body) in answers {
            server
                .mock_async(|when, then| {
                    when.method(GET).path(path);
                    match kind {
                        Some(kind) => then.status(200).header("content-type", kind).body(body),
                        None => then.status(200).body(body),
                    };
                })
                .await;
        }
        server
            .mock_async(|when, then| {
                when.method(GET).path("/gone");
                then.status(404).header("content-type", "text/html").body("<p>not here</p>");
            })
            .await;
        let hosts: Vec<(String, &str)> = (0..6).map(|n| (format!("h{n}.example.org"), "127.0.0.1")).collect();
        let hosts: Vec<(&str, &str)> = hosts.iter().map(|(host, ip)| (host.as_str(), *ip)).collect();
        let fetcher = on_mock(&server, &hosts);
        let read = |n: usize, path: &'static str| {
            let url = at(&server, &format!("h{n}.example.org"), path);
            let fetcher = &fetcher;
            async move { fetcher.fetch(&url, true).await }
        };
        assert_eq!(read(0, "/report.pdf").await, Err(FetchError::WrongType));
        assert_eq!(read(1, "/untyped").await, Err(FetchError::WrongType));
        assert_eq!(read(2, "/huge").await, Err(FetchError::TooLarge));
        let long = read(3, "/long").await.expect("a long page");
        assert_eq!((long.markdown.chars().count(), long.truncated), (MAX_PAGE_CHARS, true));
        assert_eq!(read(4, "/latin1").await.expect("a page").markdown, "Grüße");
        assert_eq!(read(5, "/gone").await, Err(FetchError::Status(404)));
    }
}
