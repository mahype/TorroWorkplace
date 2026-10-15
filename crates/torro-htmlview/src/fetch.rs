//! Answers what Blitz asks for while drawing a mail: `http`/`https` over the workspace's `reqwest` (rustls),
//! `data:` decoded here, `cid:` from the app ([`CidSource`]); every other scheme is refused. Only pictures go
//! back to Blitz (PNG, JPEG, GIF, WebP by their first bytes).
//!
//! Blitz asks for more than the pictures of `<img>`: style sheets (`<link>`, `@import`), fonts (`@font-face`)
//! and pictures in CSS `url()`. Its [`Request`] carries only the URL, not what kind of resource it wants, so
//! such a request cannot be told from a picture's and goes out like one; only its answer is refused. The
//! core's cleaning already drops `<link>`, a `<style>` of the mail may still name the others.
//!
//! Web requests reach only public hosts (review of M9.6): an IP literal or `localhost` that is not public is
//! refused before the request, every redirect is checked the same way ([`Guard`]), and host names resolve
//! through [`PublicResolver`], which drops every address that is not public – so neither a redirect nor DNS
//! rebinding brings a mail to the loopback, the local network or a cloud's metadata service.
//!
//! Why not `blitz-net`: it brings native-tls (OpenSSL on Linux), and it hands a picture behind a redirect to
//! Blitz under the final URL, while Blitz waits under the URL in `src` – such a picture never shows (trial
//! M9.4). Here every answer goes back under the URL Blitz asked for.
//!
//! Every request Blitz makes gets an answer, also a refusal (empty bytes): Blitz holds back painting while a
//! style sheet it counts as critical (`@import`) is pending, and an answer that never comes would leave the
//! mail blank. Empty bytes are an empty style sheet, a font or picture that fails to decode.
//!
//! The fetches of all mails run on one small tokio runtime of this library, never on the UI thread; the
//! picture is decoded there too (Blitz decodes in the handler).

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request, Url};

/// Bytes of one picture at most, from the web, a `data:` URL or a part of the mail (task M9.6). An app that
/// reads a part for [`CidSource`] checks the size against it before reading.
pub const MAX_PICTURE_BYTES: u64 = 10 * 1024 * 1024;

/// Redirects one web request follows at most.
const MAX_REDIRECTS: usize = 5;

/// Supplies the pictures a mail carries with it (`cid:` URLs, RFC 2392).
///
/// The app implements it over `mail.get_attachment`: find the part whose `content_id` matches, let the core
/// place it, read the file. The library never asks the core itself.
pub trait CidSource: Send + Sync + 'static {
    /// The bytes of the part whose Content-ID is `content_id` (without `<>`, percent-decoded), or `None`
    /// when the mail has no such part or it cannot be read.
    ///
    /// Runs on a worker thread, never on the UI thread, so it may block (a request to the core, a file
    /// read). The library checks type and size itself: SVG, HTML and anything over [`MAX_PICTURE_BYTES`]
    /// are refused even when returned here; check the size before reading a file, though, so a huge part is
    /// never read only to be refused.
    fn picture(&self, content_id: &str) -> Option<Vec<u8>>;
}

/// How much a mail may fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    /// Bytes of one picture.
    pub picture: u64,
    /// Bytes of all pictures of one mail together.
    pub mail: u64,
    /// Time for one request, from connecting to the last byte.
    pub timeout: Duration,
}

impl Default for Limits {
    /// 10 MiB per picture, 50 MiB per mail, 20 s per request (task M9.6).
    fn default() -> Self {
        Self { picture: MAX_PICTURE_BYTES, mail: 50 * 1024 * 1024, timeout: Duration::from_secs(20) }
    }
}

/// Why a picture was not handed to Blitz. Only for tests and counting: nothing of it is logged with a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FetchError {
    /// Not `http`, `https`, `data` or `cid`, or a web URL while web pictures are off (tests).
    Scheme,
    /// The host, a redirect's host or every address a host name resolves to is not public (loopback, the
    /// local network, link-local …).
    Private,
    /// The bytes are no PNG, JPEG, GIF or WebP (SVG, HTML, a style sheet, a font …).
    NotAPicture,
    /// Over the limit of one picture.
    TooLarge,
    /// The mail used up its budget.
    Budget,
    /// The server answered with this status.
    Status(u16),
    /// Connecting, TLS, a timeout or a broken body.
    Network,
    /// `cid:` without a source, or the source has no such part.
    Missing,
    /// A `data:` URL that is not base64 or does not decode.
    BadData,
    /// The mail was closed while fetching.
    Closed,
    /// The library's runtime could not be started.
    NoRuntime,
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scheme => write!(f, "scheme not allowed"),
            Self::Private => write!(f, "host not public"),
            Self::NotAPicture => write!(f, "not a picture"),
            Self::TooLarge => write!(f, "picture too large"),
            Self::Budget => write!(f, "mail fetched too much"),
            Self::Status(code) => write!(f, "server answered {code}"),
            Self::Network => write!(f, "network error"),
            Self::Missing => write!(f, "embedded picture missing"),
            Self::BadData => write!(f, "data URL not readable"),
            Self::Closed => write!(f, "mail closed"),
            Self::NoRuntime => write!(f, "fetch runtime not available"),
        }
    }
}

impl std::error::Error for FetchError {}

/// The [`NetProvider`] of one mail. Cheap to clone (one `Arc`).
#[derive(Clone)]
pub(crate) struct Fetcher {
    shared: Arc<Shared>,
}

struct Shared {
    /// `false` only in tests: web URLs are refused without touching the network.
    web: bool,
    /// The client web requests go through; `None` while web pictures are off or when it cannot be built.
    client: Option<reqwest::Client>,
    /// Which hosts web requests may reach; the client's redirect policy holds the same one.
    guard: Guard,
    cid: Option<Arc<dyn CidSource>>,
    limits: Limits,
    /// Bytes fetched for this mail so far (the budget).
    used: AtomicU64,
    /// Requests not answered yet.
    pending: AtomicUsize,
    /// Requests answered so far; the document polls when this moves.
    answered: AtomicU64,
    /// Set when the mail is dropped: running fetches stop at the next chunk.
    closed: AtomicBool,
}

impl Fetcher {
    pub(crate) fn new(web: bool, cid: Option<Arc<dyn CidSource>>, limits: Limits) -> Self {
        // Built only for a mail that may fetch from the web: the demo and the tests never need it.
        let client = if web { client().cloned() } else { None };
        Self::build(web, client, Guard::default(), cid, limits)
    }

    /// A fetcher that may also reach `local` – the tests' mock servers on 127.0.0.1 – with a client of its
    /// own whose redirect policy knows the same exception. Only tests can build one, so the app's guard is
    /// never weakened.
    #[cfg(test)]
    pub(crate) fn reaching(local: Vec<SocketAddr>, limits: Limits) -> Self {
        let guard = Guard { local };
        Self::build(true, build_client(guard.clone()), guard, None, limits)
    }

    fn build(
        web: bool,
        client: Option<reqwest::Client>,
        guard: Guard,
        cid: Option<Arc<dyn CidSource>>,
        limits: Limits,
    ) -> Self {
        let shared = Shared {
            web,
            client,
            guard,
            cid,
            limits,
            used: AtomicU64::new(0),
            pending: AtomicUsize::new(0),
            answered: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        };
        Self { shared: Arc::new(shared) }
    }

    /// Requests that have not been answered yet.
    pub(crate) fn pending(&self) -> usize {
        self.shared.pending.load(Ordering::Acquire)
    }

    /// Requests answered so far (grows by one per answer, also per refusal).
    pub(crate) fn answered(&self) -> u64 {
        self.shared.answered.load(Ordering::Acquire)
    }

    /// Stops running fetches; later answers are dropped.
    pub(crate) fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
    }

    /// Fetches `url` and checks it, without Blitz: what [`NetProvider::fetch`] hands over. For the work on
    /// the runtime and for tests.
    pub(crate) async fn picture(&self, url: &Url) -> Result<Vec<u8>, FetchError> {
        let shared = &self.shared;
        let bytes = match url.scheme() {
            "http" | "https" if shared.web => web(shared, url).await?,
            "data" => data(url, shared.limits.picture)?,
            "cid" => cid(shared, url).await?,
            _ => return Err(FetchError::Scheme),
        };
        if shared.closed.load(Ordering::Acquire) {
            return Err(FetchError::Closed);
        }
        if bytes.len() as u64 > shared.limits.picture {
            return Err(FetchError::TooLarge);
        }
        if !is_picture(&bytes) {
            return Err(FetchError::NotAPicture);
        }
        // Web bytes were counted while streaming; the others count here.
        if url.scheme() != "http" && url.scheme() != "https" {
            reserve(shared, bytes.len() as u64)?;
        }
        Ok(bytes)
    }
}

impl NetProvider for Fetcher {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        self.shared.pending.fetch_add(1, Ordering::AcqRel);
        let job = Job { fetcher: self.clone(), url: request.url, handler };
        match runtime() {
            Some(runtime) => {
                runtime.spawn(job.run());
            }
            None => job.answer(Err(FetchError::NoRuntime)),
        }
    }
}

/// One request of Blitz on its way: fetch, check, answer.
struct Job {
    fetcher: Fetcher,
    url: Url,
    handler: Box<dyn NetHandler>,
}

impl Job {
    async fn run(self) {
        let result = self.fetcher.picture(&self.url).await;
        self.answer(result);
    }

    /// Answers under the URL Blitz asked for (not the one after redirects), refusals with empty bytes.
    fn answer(self, result: Result<Vec<u8>, FetchError>) {
        let bytes = result.map(Bytes::from).unwrap_or_default();
        let shared = &self.fetcher.shared;
        if !shared.closed.load(Ordering::Acquire) {
            self.handler.bytes(self.url.to_string(), bytes);
        }
        shared.answered.fetch_add(1, Ordering::AcqRel);
        shared.pending.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Counts `bytes` against the mail's budget.
fn reserve(shared: &Shared, bytes: u64) -> Result<(), FetchError> {
    let before = shared.used.fetch_add(bytes, Ordering::AcqRel);
    if before.saturating_add(bytes) > shared.limits.mail { Err(FetchError::Budget) } else { Ok(()) }
}

/// A picture from the web: only from a public host, redirects followed and checked, the size limit kept while
/// streaming.
async fn web(shared: &Shared, url: &Url) -> Result<Vec<u8>, FetchError> {
    shared.guard.check(url).map_err(|Blocked| FetchError::Private)?;
    let client = shared.client.as_ref().ok_or(FetchError::NoRuntime)?;
    let mut response = client.get(url.as_str()).timeout(shared.limits.timeout).send().await.map_err(failed)?;
    let status = response.status();
    if !status.is_success() {
        return Err(FetchError::Status(status.as_u16()));
    }
    if response.content_length().is_some_and(|length| length > shared.limits.picture) {
        return Err(FetchError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(failed)? {
        if shared.closed.load(Ordering::Acquire) {
            return Err(FetchError::Closed);
        }
        if (body.len() + chunk.len()) as u64 > shared.limits.picture {
            return Err(FetchError::TooLarge);
        }
        reserve(shared, chunk.len() as u64)?;
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// What a failed request was: [`FetchError::Private`] when the redirect policy or the resolver refused a host
/// ([`Blocked`] somewhere in the chain of sources), else [`FetchError::Network`].
fn failed(err: reqwest::Error) -> FetchError {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&err);
    while let Some(err) = source {
        if err.is::<Blocked>() {
            return FetchError::Private;
        }
        source = err.source();
    }
    FetchError::Network
}

/// A host a mail may not reach. Travels inside reqwest's errors from the redirect policy and the resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Blocked;

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "host not public")
    }
}

impl std::error::Error for Blocked {}

/// Decides which hosts web requests may reach: public ones only. Checks the URL before the request and every
/// redirect; host names are left to [`PublicResolver`], which sees the addresses they resolve to.
#[derive(Debug, Clone, Default)]
struct Guard {
    /// Addresses reachable although they are not public: the tests' mock servers. Only
    /// [`Fetcher::reaching`] fills it, and that exists only in tests; in the app it is always empty.
    local: Vec<SocketAddr>,
}

impl Guard {
    /// `Ok` for a host name other than `localhost`/`*.localhost` and for a public IP literal (IPv4-mapped
    /// IPv6 counts as its IPv4 address); `Blocked` for the rest and for a URL without host.
    fn check(&self, url: &Url) -> Result<(), Blocked> {
        let host = url.host_str().ok_or(Blocked)?;
        let literal = host.strip_prefix('[').and_then(|host| host.strip_suffix(']')).unwrap_or(host);
        let Ok(ip) = literal.parse::<IpAddr>() else {
            // The URL parser has turned every IPv4 spelling (`0x7f.1`, `2130706433`) into dotted form
            // already, so what does not parse here is a name.
            let name = host.trim_end_matches('.');
            return if name == "localhost" || name.ends_with(".localhost") { Err(Blocked) } else { Ok(()) };
        };
        let local = url.port_or_known_default().is_some_and(|port| self.local.contains(&SocketAddr::new(ip, port)));
        if is_public(ip) || local { Ok(()) } else { Err(Blocked) }
    }
}

/// Resolves host names for the web client and keeps only public addresses, so a name that points to the
/// loopback or the local network – also one that changes its answer after a first look (DNS rebinding) – is
/// never connected to. Every address filtered out is an error ([`Blocked`]).
struct PublicResolver;

impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            // Port 0: reqwest puts in the port of the URL.
            let found = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let public = public_only(found);
            if public.is_empty() {
                return Err(Box::new(Blocked) as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(public.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// The public ones of `addresses`, in their order.
fn public_only(addresses: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    addresses.into_iter().filter(|address| is_public(address.ip())).collect()
}

/// Whether `ip` is an address of the public internet. Not public: unspecified, loopback, private (RFC 1918,
/// ULA `fc00::/7`), shared (CGNAT `100.64.0.0/10`), link-local (the cloud metadata service at
/// 169.254.169.254, too), multicast, broadcast, documentation, benchmarking and reserved ranges. IPv6 that
/// carries an IPv4 address (mapped, compatible, NAT64) counts as that address.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => is_public_v6(ip),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let this_network = a == 0;
    let shared = a == 100 && (64..128).contains(&b);
    let protocol = a == 192 && b == 0 && c == 0;
    let benchmarking = a == 198 && (b == 18 || b == 19);
    // 240.0.0.0/4 is reserved, 255.255.255.255 (broadcast) lies in it.
    let reserved = a >= 240;
    !(this_network
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || shared
        || protocol
        || benchmarking
        || ip.is_documentation()
        || ip.is_multicast()
        || reserved)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let segments = ip.segments();
    let [.., a, b, c, d] = ip.octets();
    let embedded = Ipv4Addr::new(a, b, c, d);
    // NAT64 (64:ff9b::/96) reaches the IPv4 address in the last 32 bits.
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return is_public_v4(embedded);
    }
    // `::/96`: unspecified, loopback and the old IPv4-compatible addresses.
    let compatible = segments[..6] == [0; 6];
    let unique_local = segments[0] & 0xfe00 == 0xfc00;
    let link_local = segments[0] & 0xffc0 == 0xfe80;
    let site_local = segments[0] & 0xffc0 == 0xfec0;
    let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    !(compatible || ip.is_multicast() || unique_local || link_local || site_local || documentation)
}

/// A `data:` picture. Only base64: pictures in mail are never percent-encoded text.
fn data(url: &Url, limit: u64) -> Result<Vec<u8>, FetchError> {
    let rest = url.as_str().strip_prefix("data:").ok_or(FetchError::BadData)?;
    let (meta, payload) = rest.split_once(',').ok_or(FetchError::BadData)?;
    if !meta.to_ascii_lowercase().ends_with(";base64") {
        return Err(FetchError::BadData);
    }
    // Four characters carry three bytes: refuse before decoding something huge.
    if (payload.len() as u64 / 4).saturating_mul(3) > limit {
        return Err(FetchError::TooLarge);
    }
    let payload: String = payload.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let payload = percent_decode(&payload).ok_or(FetchError::BadData)?;
    base64::engine::general_purpose::STANDARD.decode(payload).map_err(|_| FetchError::BadData)
}

/// A `cid:` picture from the app, asked on a blocking thread.
async fn cid(shared: &Shared, url: &Url) -> Result<Vec<u8>, FetchError> {
    let source = shared.cid.clone().ok_or(FetchError::Missing)?;
    let id = percent_decode(url.path()).ok_or(FetchError::Missing)?;
    let id = String::from_utf8(id).map_err(|_| FetchError::Missing)?;
    tokio::task::spawn_blocking(move || source.picture(&id))
        .await
        .map_err(|_| FetchError::Missing)?
        .ok_or(FetchError::Missing)
}

/// `%XX` decoded; `None` for a broken escape.
fn percent_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'%' {
            let hex = bytes.get(at + 1..at + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(byte);
            at += 1;
        }
    }
    Some(out)
}

/// PNG, JPEG, GIF or WebP by their first bytes – the four types Blitz can decode here. Everything else (SVG,
/// HTML, CSS, fonts, scripts) is refused whatever the server claims it is.
pub(crate) fn is_picture(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()))
}

/// The runtime all mails fetch on: two worker threads, started on the first picture. `None` when it cannot
/// be started (the mail then shows without pictures).
fn runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("torro-htmlview")
                .enable_all()
                .build()
                .inspect_err(|err| eprintln!("htmlview: cannot start the picture fetcher: {err}"))
                .ok()
        })
        .as_ref()
}

/// The HTTP client of all mails, built on the first mail that may fetch from the web ([`build_client`]).
fn client() -> Option<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT.get_or_init(|| build_client(Guard::default())).as_ref()
}

/// A client with no cookies (no cookie store), no `Referer`, a generic user agent that tells the sender nothing
/// about the app, at most [`MAX_REDIRECTS`] redirects each checked by `guard`, and names resolved through
/// [`PublicResolver`].
fn build_client(guard: Guard) -> Option<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("image/webp,image/png,image/jpeg,image/gif;q=0.9,*/*;q=0.5"),
    );
    let limit = reqwest::redirect::Policy::limited(MAX_REDIRECTS);
    let redirect = reqwest::redirect::Policy::custom(move |attempt| match guard.check(attempt.url()) {
        Ok(()) => limit.redirect(attempt),
        Err(blocked) => attempt.error(blocked),
    });
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .default_headers(headers)
        .referer(false)
        .redirect(redirect)
        .dns_resolver(PublicResolver)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .inspect_err(|err| eprintln!("htmlview: cannot build the picture client: {err}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::mpsc;

    use httpmock::Method::GET;
    use httpmock::MockServer;

    use super::*;

    /// The smallest PNG: one transparent pixel.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
        0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D,
        0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// The fetcher of the app: only public hosts.
    fn fetcher(limits: Limits) -> Fetcher {
        Fetcher::new(true, None, limits)
    }

    /// A fetcher that may reach `server` (on 127.0.0.1) and nothing else that is not public.
    fn local(server: &MockServer, limits: Limits) -> Fetcher {
        Fetcher::reaching(vec![*server.address()], limits)
    }

    fn url(text: &str) -> Url {
        Url::parse(text).expect("url")
    }

    fn block<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime").block_on(future)
    }

    /// Catches what Blitz would get.
    struct Catch(Mutex<Option<mpsc::Sender<(String, Bytes)>>>);

    impl NetHandler for Catch {
        fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
            if let Ok(mut sender) = self.0.lock()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send((resolved_url, bytes));
            }
        }
    }

    #[test]
    fn recognises_only_the_four_picture_types() {
        assert!(is_picture(PNG));
        assert!(is_picture(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]));
        assert!(is_picture(b"GIF89a\x01\x00"));
        assert!(is_picture(b"RIFF\x10\x00\x00\x00WEBPVP8 "));
        assert!(!is_picture(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"));
        assert!(!is_picture(b"<?xml version=\"1.0\"?><svg/>"));
        assert!(!is_picture(b"<!DOCTYPE html><html></html>"));
        assert!(!is_picture(b"body { color: red }"));
        assert!(!is_picture(b"RIFF\x10\x00\x00\x00WAVEfmt "));
        assert!(!is_picture(b""));
    }

    #[test]
    fn default_limits_are_ten_and_fifty_mib() {
        let limits = Limits::default();
        assert_eq!(limits.picture, 10 * 1024 * 1024);
        assert_eq!(limits.mail, 50 * 1024 * 1024);
    }

    #[test]
    fn follows_a_redirect_and_answers_under_the_asked_url() {
        let server = MockServer::start();
        let moved = server.mock(|when, then| {
            when.method(GET).path("/track/1");
            then.status(302).header("Location", "/cdn/picture.png");
        });
        let picture = server.mock(|when, then| {
            when.method(GET).path("/cdn/picture.png");
            then.status(200).header("Content-Type", "image/png").body(PNG);
        });
        let asked = url(&server.url("/track/1"));
        let fetcher = local(&server, Limits::default());
        let (sender, receiver) = mpsc::channel();
        fetcher.fetch(0, Request::get(asked.clone()), Box::new(Catch(Mutex::new(Some(sender)))));
        let (answered_url, bytes) = receiver.recv_timeout(Duration::from_secs(10)).expect("answer");
        assert_eq!(answered_url, asked.to_string(), "Blitz waits under the URL in src, not the redirect target");
        assert_eq!(bytes.as_ref(), PNG);
        moved.assert();
        picture.assert();
        // The counters move right after the answer.
        for _ in 0..100 {
            if fetcher.pending() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(fetcher.pending(), 0);
        assert_eq!(fetcher.answered(), 1);
    }

    #[test]
    fn sends_no_referer_and_no_cookie_across_a_redirect() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/a");
            then.status(302).header("Location", "/b").header("Set-Cookie", "id=secret");
        });
        let target = server.mock(|when, then| {
            when.method(GET).path("/b").header_missing("referer").header_missing("cookie");
            then.status(200).body(PNG);
        });
        let result = block(local(&server, Limits::default()).picture(&url(&server.url("/a"))));
        assert_eq!(result.as_deref(), Ok(PNG));
        target.assert();
    }

    #[test]
    fn refuses_svg_and_html_whatever_the_server_claims() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/logo.svg");
            then.status(200).header("Content-Type", "image/png").body("<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
        });
        server.mock(|when, then| {
            when.method(GET).path("/page");
            then.status(200).header("Content-Type", "image/gif").body("<!DOCTYPE html><p>hi</p>");
        });
        let fetcher = local(&server, Limits::default());
        assert_eq!(block(fetcher.picture(&url(&server.url("/logo.svg")))), Err(FetchError::NotAPicture));
        assert_eq!(block(fetcher.picture(&url(&server.url("/page")))), Err(FetchError::NotAPicture));
    }

    #[test]
    fn refusals_still_answer_blitz_with_empty_bytes() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/style.css");
            then.status(200).body("body { color: red }");
        });
        let fetcher = local(&server, Limits::default());
        let (sender, receiver) = mpsc::channel();
        fetcher.fetch(0, Request::get(url(&server.url("/style.css"))), Box::new(Catch(Mutex::new(Some(sender)))));
        let (_, bytes) = receiver.recv_timeout(Duration::from_secs(10)).expect("answer");
        assert!(bytes.is_empty());
    }

    #[test]
    fn keeps_the_size_of_one_picture_with_and_without_a_length() {
        let limits = Limits { picture: 1024, ..Limits::default() };
        let mut big = PNG.to_vec();
        big.resize(2048, 0);
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/big.png");
            then.status(200).body(big.clone());
        });
        server.mock(|when, then| {
            when.method(GET).path("/ok.png");
            then.status(200).body(PNG);
        });
        let fetcher = local(&server, limits);
        assert_eq!(block(fetcher.picture(&url(&server.url("/big.png")))), Err(FetchError::TooLarge));
        assert_eq!(block(fetcher.picture(&url(&server.url("/ok.png")))).as_deref(), Ok(PNG));
    }

    #[test]
    fn keeps_the_budget_of_one_mail() {
        let limits = Limits { picture: 1024, mail: 150, ..Limits::default() };
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/p.png");
            then.status(200).body(PNG);
        });
        let fetcher = local(&server, limits);
        let picture = url(&server.url("/p.png"));
        assert!(block(fetcher.picture(&picture)).is_ok());
        assert!(block(fetcher.picture(&picture)).is_ok());
        assert_eq!(block(fetcher.picture(&picture)), Err(FetchError::Budget), "{} bytes each", PNG.len());
    }

    #[test]
    fn reports_a_failing_server() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/gone.png");
            then.status(404);
        });
        let result = block(local(&server, Limits::default()).picture(&url(&server.url("/gone.png"))));
        assert_eq!(result, Err(FetchError::Status(404)));
    }

    #[test]
    fn web_pictures_can_be_off_for_tests() {
        let fetcher = Fetcher::new(false, None, Limits::default());
        assert_eq!(block(fetcher.picture(&url("https://example.com/a.png"))), Err(FetchError::Scheme));
    }

    #[test]
    fn refuses_other_schemes() {
        let fetcher = fetcher(Limits::default());
        for other in ["file:///etc/passwd", "ftp://example.com/a.png", "javascript:alert(1)"] {
            assert_eq!(block(fetcher.picture(&url(other))), Err(FetchError::Scheme), "{other}");
        }
    }

    #[test]
    fn decodes_data_pictures_and_refuses_svg_in_them() {
        let fetcher = fetcher(Limits::default());
        let png = base64::engine::general_purpose::STANDARD.encode(PNG);
        let ok = url(&format!("data:image/png;base64,{png}"));
        assert_eq!(block(fetcher.picture(&ok)).as_deref(), Ok(PNG));
        let svg = base64::engine::general_purpose::STANDARD.encode("<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
        let svg = url(&format!("data:image/png;base64,{svg}"));
        assert_eq!(block(fetcher.picture(&svg)), Err(FetchError::NotAPicture));
        assert_eq!(block(fetcher.picture(&url("data:image/png,plain"))), Err(FetchError::BadData));
    }

    struct Parts;

    impl CidSource for Parts {
        fn picture(&self, content_id: &str) -> Option<Vec<u8>> {
            match content_id {
                "logo@torro.example" => Some(PNG.to_vec()),
                "page@torro.example" => Some(b"<html></html>".to_vec()),
                _ => None,
            }
        }
    }

    #[test]
    fn asks_the_app_for_cid_pictures() {
        let fetcher = Fetcher::new(false, Some(Arc::new(Parts)), Limits::default());
        assert_eq!(block(fetcher.picture(&url("cid:logo@torro.example"))).as_deref(), Ok(PNG));
        assert_eq!(block(fetcher.picture(&url("cid:logo%40torro.example"))).as_deref(), Ok(PNG));
        assert_eq!(block(fetcher.picture(&url("cid:page@torro.example"))), Err(FetchError::NotAPicture));
        assert_eq!(block(fetcher.picture(&url("cid:other@torro.example"))), Err(FetchError::Missing));
        let without = fetcher_without_cid();
        assert_eq!(block(without.picture(&url("cid:logo@torro.example"))), Err(FetchError::Missing));
    }

    fn fetcher_without_cid() -> Fetcher {
        Fetcher::new(false, None, Limits::default())
    }

    #[test]
    fn a_closed_mail_gets_no_more_answers() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/p.png");
            then.status(200).delay(Duration::from_millis(200)).body(PNG);
        });
        let fetcher = local(&server, Limits::default());
        let (sender, receiver) = mpsc::channel();
        fetcher.fetch(0, Request::get(url(&server.url("/p.png"))), Box::new(Catch(Mutex::new(Some(sender)))));
        fetcher.close();
        assert!(receiver.recv_timeout(Duration::from_secs(2)).is_err());
        for _ in 0..100 {
            if fetcher.pending() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fetcher.pending(), 0, "the request still counts as answered");
    }

    /// Review of M9.6 (SSRF): a mail reaches no host that is not public, written as an IP literal in any
    /// spelling or as `localhost`; refused before anything goes out.
    #[test]
    fn refuses_hosts_that_are_not_public() {
        let fetcher = fetcher(Limits::default());
        for private in [
            "http://127.0.0.1/a.png",
            "http://127.1.2.3:8080/a.png",
            "http://localhost/a.png",
            "http://LOCALHOST./a.png",
            "http://mail.localhost/a.png",
            "http://[::1]/a.png",
            "http://10.0.0.1/a.png",
            "http://172.16.5.4/a.png",
            "http://192.168.178.1/a.png",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::ffff:127.0.0.1]/a.png",
            "http://[::ffff:169.254.169.254]/a.png",
            "http://[64:ff9b::a00:1]/a.png",
            "http://0x7f.1/a.png",
            "http://2130706433/a.png",
            "http://0.0.0.0/a.png",
            "http://100.64.0.1/a.png",
            "http://[fd00::1]/a.png",
            "http://[fe80::1]/a.png",
            "http://[::]/a.png",
            "http://224.0.0.1/a.png",
            "http://255.255.255.255/a.png",
        ] {
            assert_eq!(block(fetcher.picture(&url(private))), Err(FetchError::Private), "{private}");
        }
    }

    /// The app's fetcher does not reach the tests' mock server either: the exception exists only in
    /// [`Fetcher::reaching`].
    #[test]
    fn the_apps_fetcher_does_not_reach_a_local_server() {
        let server = MockServer::start();
        let picture = server.mock(|when, then| {
            when.method(GET).path("/p.png");
            then.status(200).body(PNG);
        });
        let result = block(fetcher(Limits::default()).picture(&url(&server.url("/p.png"))));
        assert_eq!(result, Err(FetchError::Private));
        picture.assert_calls(0);
    }

    /// The guard as a pure function: names and public addresses pass, the test exception holds only for its
    /// own address and port.
    #[test]
    fn the_guard_lets_names_and_public_addresses_pass() {
        let open = Guard::default();
        for public in ["https://example.com/a.png", "http://93.184.216.34/a.png", "https://[2606:4700::1111]/a.png"] {
            assert_eq!(open.check(&url(public)), Ok(()), "{public}");
        }
        let local = Guard { local: vec!["127.0.0.1:8080".parse().expect("address")] };
        assert_eq!(local.check(&url("http://127.0.0.1:8080/a.png")), Ok(()));
        assert_eq!(local.check(&url("http://127.0.0.1:8081/a.png")), Err(Blocked), "another port");
        assert_eq!(local.check(&url("http://localhost:8080/a.png")), Err(Blocked), "a name is no exception");
    }

    /// A redirect from a host the test lets through to one that is not public is refused, and the private
    /// host is never asked.
    #[test]
    fn refuses_a_redirect_to_a_host_that_is_not_public() {
        let public = MockServer::start();
        let private = MockServer::start();
        let target = private.mock(|when, then| {
            when.method(GET).path("/p.png");
            then.status(200).body(PNG);
        });
        let targets = [
            private.url("/p.png"),
            "http://169.254.169.254/latest/meta-data/".to_owned(),
            format!("http://localhost:{}/p.png", public.port()),
            "http://[::ffff:10.0.0.1]/p.png".to_owned(),
        ];
        for (n, location) in targets.iter().enumerate() {
            public.mock(|when, then| {
                when.method(GET).path(format!("/moved/{n}"));
                then.status(302).header("Location", location);
            });
        }
        // Only the first hop is let through.
        let fetcher = local(&public, Limits::default());
        for (n, location) in targets.iter().enumerate() {
            let result = block(fetcher.picture(&url(&public.url(format!("/moved/{n}")))));
            assert_eq!(result, Err(FetchError::Private), "{location}");
        }
        target.assert_calls(0);
    }

    /// At most five redirects, like before the custom policy.
    #[test]
    fn follows_five_redirects_and_no_more() {
        let server = MockServer::start();
        for n in 0..6 {
            server.mock(|when, then| {
                when.method(GET).path(format!("/r{n}"));
                then.status(302).header("Location", format!("/r{}", n + 1));
            });
        }
        server.mock(|when, then| {
            when.method(GET).path("/r6");
            then.status(200).body(PNG);
        });
        let fetcher = local(&server, Limits::default());
        assert_eq!(block(fetcher.picture(&url(&server.url("/r1")))).as_deref(), Ok(PNG), "five redirects");
        assert_eq!(block(fetcher.picture(&url(&server.url("/r0")))), Err(FetchError::Network), "six redirects");
    }

    /// The resolver's filter as a pure function: only public addresses stay, in their order.
    #[test]
    fn the_resolver_keeps_only_public_addresses() {
        let address = |text: &str| text.parse::<SocketAddr>().expect("address");
        let found = [
            "127.0.0.1:0",
            "93.184.216.34:0",
            "10.1.2.3:0",
            "192.168.0.10:0",
            "169.254.169.254:0",
            "100.100.100.100:0",
            "[::1]:0",
            "[::ffff:192.168.0.1]:0",
            "[fc00::1]:0",
            "[2606:4700::1111]:0",
        ]
        .map(address);
        assert_eq!(public_only(found), vec![address("93.184.216.34:0"), address("[2606:4700::1111]:0")]);
        assert!(public_only([address("127.0.0.1:0"), address("[::1]:0")]).is_empty());
    }

    /// A name that resolves only to the loopback is an error, not an empty list (the hosts file answers
    /// `localhost`, no network is asked).
    #[test]
    fn the_resolver_refuses_a_name_with_only_private_addresses() {
        use reqwest::dns::Resolve as _;
        let name: reqwest::dns::Name = "localhost".parse().expect("name");
        let result = block(PublicResolver.resolve(name));
        let err = result.err().expect("refused");
        assert!(err.is::<Blocked>(), "{err}");
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("a%40b").as_deref(), Some(b"a@b".as_slice()));
        assert_eq!(percent_decode("a%4"), None);
        assert_eq!(percent_decode("a%zz"), None);
    }
}
