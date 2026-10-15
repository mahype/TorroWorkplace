//! Fetches the pictures of a mail for Blitz: `http`/`https` over the workspace's `reqwest` (rustls), `data:`
//! decoded here, `cid:` from the app ([`CidSource`]). Nothing else, and only picture types.
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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request, Url};

/// Supplies the pictures a mail carries with it (`cid:` URLs, RFC 2392).
///
/// The app implements it over `mail.get_attachment`: find the part whose `content_id` matches, let the core
/// place it, read the file. The library never asks the core itself.
pub trait CidSource: Send + Sync + 'static {
    /// The bytes of the part whose Content-ID is `content_id` (without `<>`, percent-decoded), or `None`
    /// when the mail has no such part or it cannot be read.
    ///
    /// Runs on a worker thread, never on the UI thread, so it may block (a request to the core, a file
    /// read). The library checks type and size itself: SVG, HTML and anything over 10 MiB are refused even
    /// when returned here.
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
        Self { picture: 10 * 1024 * 1024, mail: 50 * 1024 * 1024, timeout: Duration::from_secs(20) }
    }
}

/// Why a picture was not handed to Blitz. Only for tests and counting: nothing of it is logged with a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FetchError {
    /// Not `http`, `https`, `data` or `cid`, or a web URL while web pictures are off (tests).
    Scheme,
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
        let shared = Shared {
            web,
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

/// A picture from the web: redirects followed, the size limit kept while streaming.
async fn web(shared: &Shared, url: &Url) -> Result<Vec<u8>, FetchError> {
    let client = client().ok_or(FetchError::NoRuntime)?;
    let mut response =
        client.get(url.as_str()).timeout(shared.limits.timeout).send().await.map_err(|_| FetchError::Network)?;
    let status = response.status();
    if !status.is_success() {
        return Err(FetchError::Status(status.as_u16()));
    }
    if response.content_length().is_some_and(|length| length > shared.limits.picture) {
        return Err(FetchError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| FetchError::Network)? {
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

/// The HTTP client of all mails: no cookies (no cookie store), no `Referer`, at most five redirects, a
/// generic user agent that tells the sender nothing about the app.
fn client() -> Option<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_static("image/webp,image/png,image/jpeg,image/gif;q=0.9,*/*;q=0.5"),
            );
            reqwest::Client::builder()
                .user_agent("Mozilla/5.0")
                .default_headers(headers)
                .referer(false)
                .redirect(reqwest::redirect::Policy::limited(5))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .inspect_err(|err| eprintln!("htmlview: cannot build the picture client: {err}"))
                .ok()
        })
        .as_ref()
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

    fn fetcher(limits: Limits) -> Fetcher {
        Fetcher::new(true, None, limits)
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
        let fetcher = fetcher(Limits::default());
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
        let result = block(fetcher(Limits::default()).picture(&url(&server.url("/a"))));
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
        let fetcher = fetcher(Limits::default());
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
        let fetcher = fetcher(Limits::default());
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
        let fetcher = fetcher(limits);
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
        let fetcher = fetcher(limits);
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
        let result = block(fetcher(Limits::default()).picture(&url(&server.url("/gone.png"))));
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
        let fetcher = fetcher(Limits::default());
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

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("a%40b").as_deref(), Some(b"a@b".as_slice()));
        assert_eq!(percent_decode("a%4"), None);
        assert_eq!(percent_decode("a%zz"), None);
    }
}
