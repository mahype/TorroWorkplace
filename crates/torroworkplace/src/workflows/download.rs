//! Guarded document GETs and the closed Stripe resolver. Ephemeral link secrets are never logged or serialized.

use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};
use reqwest::Url;
use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use torro_ipc::workflows::RunReason;
use torro_pim_caldav::discover::{GuardedDns, Resolver, SystemResolver, checked_domain, is_public};

const STRIPE_PAGE: &str = "invoice.stripe.com";
const STRIPE_DATA: &str = "invoicedata.stripe.com";
const STRIPE_FILE: &str = "stripe-upload-api.s3.us-west-1.amazonaws.com";
const MAX_HTML: usize = 2 * 1024 * 1024;
const MAX_PDF: usize = 50 * 1024 * 1024;
const MAX_STRIPE_ERROR: usize = 16 * 1024;
const MAX_RESOURCE: usize = 512 * 1024;
const DEADLINE: Duration = Duration::from_secs(60);

/// Intentionally neither Debug nor Serialize. Only the live job holds a link or signed download target.
pub struct Link(Url);

impl Link {
    pub fn host(&self) -> &str {
        self.0.host_str().unwrap_or("")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failure {
    pub reason: RunReason,
    pub retryable: bool,
}

impl Failure {
    fn fixed(reason: RunReason) -> Self {
        Self { reason, retryable: false }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("workflow document retrieval failed")
    }
}

impl std::error::Error for Failure {}

fn unsupported() -> Failure {
    Failure::fixed(RunReason::Unsupported)
}

fn after_http_400(received: bool, error: Failure) -> Failure {
    if received && error.retryable { unsupported() } else { error }
}

fn decoded(segment: &str) -> Result<String, Failure> {
    let mut bytes = Vec::with_capacity(segment.len());
    let mut input = segment.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = input.next().and_then(|b| char::from(b).to_digit(16)).ok_or_else(unsupported)?;
            let low = input.next().and_then(|b| char::from(b).to_digit(16)).ok_or_else(unsupported)?;
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).map_err(|_| unsupported())
}

/// Exact hosts only: no suffix matching, ports, fragments, credentials, IP spellings or local names.
fn checked(input: &str, hosts: &[String]) -> Result<Link, Failure> {
    if input.len() > 16 * 1024 || input.chars().any(char::is_control) || input.trim() != input {
        return Err(unsupported());
    }
    let url = Url::parse(input).map_err(|_| unsupported())?;
    let domain = url.domain().filter(|host| checked_domain(host).is_some()).ok_or_else(unsupported)?;
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !hosts.iter().any(|host| host == domain)
    {
        return Err(unsupported());
    }
    let forbidden = ["pay", "payment", "payments", "login", "signin", "dashboard"];
    if url.path_segments().is_none_or(|segments| {
        segments.into_iter().any(|segment| {
            !decoded(segment).is_ok_and(|decoded| {
                !(forbidden.iter().any(|bad| decoded.eq_ignore_ascii_case(bad))
                    || domain == STRIPE_PAGE
                        && ["receipt", "receipts"].iter().any(|bad| decoded.eq_ignore_ascii_case(bad)))
            })
        })
    }) {
        return Err(unsupported());
    }
    Ok(Link(url))
}

/// Provider presentation resources a rendering profile may load as data. Nothing loaded here is ever executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Stylesheet,
    Script,
}

/// What a single guarded GET may return.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Want {
    Document,
    Json,
    Resource(ResourceKind),
}

/// Resolves a head reference of an already fetched page. Only absolute, scheme-relative and root-relative
/// references are accepted; document-relative paths would need the page's private path and are refused.
pub fn resource_link(page_host: &str, href: &str, hosts: &[String]) -> Result<Link, Failure> {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://{page_host}{href}")
    } else {
        href.to_owned()
    };
    checked(&absolute, hosts)
}

impl Link {
    /// The pinned form of a resource: path and query, never scheme, host or fragment.
    pub fn path_and_query(&self) -> String {
        match self.0.query() {
            Some(query) => format!("{}?{query}", self.0.path()),
            None => self.0.path().to_owned(),
        }
    }
}

/// One job's network budget, retained across redirects and resolver/download stages.
pub struct Budget {
    start: Instant,
    gets: usize,
    redirects: usize,
}

impl Budget {
    pub fn new() -> Self {
        Self { start: Instant::now(), gets: 0, redirects: 0 }
    }
    fn remaining(&self) -> Result<Duration, Failure> {
        DEADLINE
            .checked_sub(self.start.elapsed())
            .filter(|time| !time.is_zero())
            .ok_or(Failure { reason: RunReason::Offline, retryable: true })
    }
}

pub struct Body {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub source_host: String,
}

pub struct Downloader {
    http: reqwest::Client,
    #[cfg(test)]
    test_endpoint: Option<Url>,
}

async fn bounded_bytes(
    response: &mut reqwest::Response,
    limit: usize,
    allow: &(dyn Fn() -> Result<(), Failure> + Sync),
) -> Result<Vec<u8>, Failure> {
    if response.content_length().is_some_and(|length| length > limit as u64) {
        return Err(unsupported());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) =
        response.chunk().await.map_err(|_| Failure { reason: RunReason::Offline, retryable: true })?
    {
        allow()?;
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(unsupported());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Recognize explicit error semantics only, never arbitrary occurrences of "expired" in a page.
/// These supported forms are synthetic contracts, not a claim about a private pilot response.
fn stripe_expired(bytes: &[u8], mime: &str) -> bool {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: ErrorDetail,
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ErrorDetail {
        Text(String),
        Fields { code: Option<String>, message: Option<String> },
    }
    let message = |value: &str| {
        matches!(
            value.trim().trim_end_matches(['.', '!']).to_ascii_lowercase().as_str(),
            "this invoice link has expired"
                | "the invoice link has expired"
                | "invoice link has expired"
                | "invoice link expired"
                | "this invoice has expired"
                | "invoice has expired"
        )
    };
    match mime {
        "text/plain" => std::str::from_utf8(bytes).is_ok_and(message),
        "application/json" => {
            // Struct deserializers also accept positional JSON arrays. Reject those at both
            // levels; the typed second pass still rejects duplicate recognized fields.
            let Ok(serde_json::Value::Object(fields)) = serde_json::from_slice(bytes) else { return false };
            if !matches!(fields.get("error"), Some(serde_json::Value::String(_) | serde_json::Value::Object(_))) {
                return false;
            }
            match serde_json::from_slice::<ErrorBody>(bytes) {
                Ok(ErrorBody { error: ErrorDetail::Text(value) }) => message(&value),
                Ok(ErrorBody { error: ErrorDetail::Fields { code: Some(code), .. } }) => {
                    matches!(code.as_str(), "invoice_link_expired" | "invoice_expired" | "link_expired")
                }
                Ok(ErrorBody { error: ErrorDetail::Fields { code: None, message: Some(value) } }) => message(&value),
                _ => false,
            }
        }
        _ => false,
    }
}

impl Downloader {
    pub fn system() -> Result<Self, Failure> {
        Self::new(Arc::new(SystemResolver::new()))
    }

    fn new(resolver: Arc<dyn Resolver>) -> Result<Self, Failure> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .connect_timeout(Duration::from_secs(5))
            .timeout(DEADLINE)
            .dns_resolver(Arc::new(GuardedDns::new(resolver, is_public)))
            .user_agent(crate::places::USER_AGENT)
            .build()
            .map_err(|_| unsupported())?;
        Ok(Self {
            http,
            #[cfg(test)]
            test_endpoint: None,
        })
    }

    /// Offline tests replace only the socket destination; host/path/redirect guards and response parsing stay real.
    #[cfg(test)]
    pub(super) fn for_test(endpoint: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .referer(false)
                .build()
                .expect("test client"),
            test_endpoint: Some(Url::parse(endpoint).expect("test endpoint")),
        }
    }

    fn request_url(&self, link: &Link) -> Url {
        #[cfg(test)]
        if let Some(endpoint) = &self.test_endpoint {
            let mut local = endpoint.clone();
            local.set_path(&format!("/{}{}", link.host(), link.0.path()));
            local.set_query(link.0.query());
            return local;
        }
        link.0.clone()
    }

    /// `allow` rechecks current carrier/origin/grants immediately before every GET, including redirects.
    pub async fn fetch(
        &self,
        link: Link,
        hosts: &[String],
        budget: &mut Budget,
        allow: &(dyn Fn() -> Result<(), Failure> + Sync),
    ) -> Result<Body, Failure> {
        self.fetch_bounded(link, hosts, budget, allow, Want::Document).await
    }

    /// Loads one stylesheet or script as bounded data for a rendering profile, under the same host, redirect,
    /// permission and GET budget as the page itself.
    pub async fn fetch_resource(
        &self,
        link: Link,
        hosts: &[String],
        budget: &mut Budget,
        allow: &(dyn Fn() -> Result<(), Failure> + Sync),
        kind: ResourceKind,
    ) -> Result<Body, Failure> {
        self.fetch_bounded(link, hosts, budget, allow, Want::Resource(kind)).await
    }

    async fn fetch_bounded(
        &self,
        mut link: Link,
        hosts: &[String],
        budget: &mut Budget,
        allow: &(dyn Fn() -> Result<(), Failure> + Sync),
        want: Want,
    ) -> Result<Body, Failure> {
        let json = want == Want::Json;
        loop {
            link = checked(link.0.as_str(), hosts)?;
            let remaining = budget.remaining()?;
            if budget.gets >= 8 {
                return Err(unsupported());
            }
            allow()?;
            budget.gets += 1;
            let mut received_http_400 = false;
            let read = async {
                let mut response = self
                    .http
                    .get(self.request_url(&link))
                    .header(
                        ACCEPT,
                        match want {
                            Want::Json => "application/json",
                            Want::Document => "application/pdf, text/html;q=0.9",
                            Want::Resource(ResourceKind::Stylesheet) => "text/css",
                            Want::Resource(ResourceKind::Script) => "application/javascript, text/javascript",
                        },
                    )
                    .send()
                    .await
                    .map_err(|_| Failure { reason: RunReason::Offline, retryable: true })?;
                let status = response.status().as_u16();
                received_http_400 = status == 400;
                if matches!(status, 301 | 302 | 303 | 307 | 308) {
                    if budget.redirects >= 5 {
                        return Err(unsupported());
                    }
                    let location = response
                        .headers()
                        .get(LOCATION)
                        .and_then(|header| header.to_str().ok())
                        .ok_or_else(unsupported)?;
                    let next = link.0.join(location).map_err(|_| unsupported())?;
                    let next = checked(next.as_str(), hosts)?;
                    budget.redirects += 1;
                    return Ok(Err(next));
                }
                if status == 400
                    && json
                    && link.host() == STRIPE_DATA
                    && link.0.path().starts_with("/invoice_pdf_file_url/")
                {
                    // A permanent HTTP400 alone proves no expiry. Inspect only a bounded explicit
                    // error on the closed resolver, within this request's existing time/GET budget.
                    allow()?;
                    let mime = response
                        .headers()
                        .get(CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .filter(|value| value.len() <= 256)
                        .map(|value| value.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
                        .ok_or_else(unsupported)?;
                    let bytes = bounded_bytes(&mut response, MAX_STRIPE_ERROR, allow).await?;
                    return Err(if stripe_expired(&bytes, &mime) {
                        Failure::fixed(RunReason::Expired)
                    } else {
                        unsupported()
                    });
                }
                match status {
                    200 => {}
                    401 | 403 => return Err(Failure::fixed(RunReason::AuthRequired)),
                    404 => return Err(Failure::fixed(RunReason::NotFound)),
                    // HTTP 410 is an explicit expiry/deletion signal; a generic missing page is not.
                    410 => return Err(Failure::fixed(RunReason::Expired)),
                    429 | 500..=599 => return Err(Failure { reason: RunReason::Offline, retryable: true }),
                    _ => return Err(unsupported()),
                }
                let content_type = response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .filter(|kind| kind.len() <= 256)
                    .ok_or_else(|| Failure::fixed(RunReason::UnexpectedType))?
                    .to_owned();
                let mime = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
                let limit = match (want, mime.as_str()) {
                    (Want::Json, "application/json") => 256 * 1024,
                    (Want::Document, "application/pdf") => MAX_PDF,
                    (Want::Document, "text/html") => MAX_HTML,
                    (Want::Resource(ResourceKind::Stylesheet), "text/css")
                    | (Want::Resource(ResourceKind::Script), "application/javascript" | "text/javascript") => {
                        MAX_RESOURCE
                    }
                    (Want::Resource(_), _) => return Err(unsupported()),
                    _ => return Err(Failure::fixed(RunReason::UnexpectedType)),
                };
                let bytes = bounded_bytes(&mut response, limit, allow).await?;
                Ok(Ok(Body { bytes, content_type, source_host: link.host().into() }))
            };
            let result = tokio::time::timeout(remaining, read)
                .await
                .unwrap_or(Err(Failure { reason: RunReason::Offline, retryable: true }));
            match result.map_err(|error| after_http_400(received_http_400, error))? {
                Ok(body) => return Ok(body),
                Err(next) => link = next,
            }
        }
    }

    pub async fn resolve_stripe_invoice_pdf(
        &self,
        mail_link: Link,
        hosts: &[String],
        budget: &mut Budget,
        allow: &(dyn Fn() -> Result<(), Failure> + Sync),
    ) -> Result<Body, Failure> {
        if ![STRIPE_PAGE, STRIPE_DATA, STRIPE_FILE].iter().all(|host| hosts.iter().any(|given| host == given)) {
            return Err(unsupported());
        }
        let endpoint = stripe_endpoint(&mail_link)?;
        let json = self.fetch_bounded(endpoint, &[STRIPE_DATA.into()], budget, allow, Want::Json).await?;
        let FileUrl(url) = serde_json::from_slice::<FileUrl>(&json.bytes).map_err(|_| unsupported())?;
        let target = checked(&url, &[STRIPE_FILE.into()])?;
        let body = self.fetch_bounded(target, &[STRIPE_FILE.into()], budget, allow, Want::Document).await?;
        if !body.content_type.split(';').next().is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/pdf"))
        {
            return Err(Failure::fixed(RunReason::UnexpectedType));
        }
        Ok(body)
    }
}

fn stripe_endpoint(link: &Link) -> Result<Link, Failure> {
    let link = checked(link.0.as_str(), &[STRIPE_PAGE.into()])?;
    let segments: Vec<_> = link.0.path_segments().ok_or_else(unsupported)?.collect();
    let tail = segments.get(segments.len().checked_sub(2).ok_or_else(unsupported)?..).ok_or_else(unsupported)?;
    let tokens = tail
        .iter()
        .map(|segment| {
            let decoded = decoded(segment)?;
            if decoded.is_empty()
                || decoded.len() > 4096
                || !decoded.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            {
                return Err(unsupported());
            }
            Ok(decoded)
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    let mut endpoint = Url::parse("https://invoicedata.stripe.com/invoice_pdf_file_url/").map_err(|_| unsupported())?;
    {
        let mut path = endpoint.path_segments_mut().map_err(|_| unsupported())?;
        path.pop_if_empty();
        for token in &tokens {
            path.push(token);
        }
    }
    Ok(Link(endpoint))
}

/// Reject duplicate and unknown keys before forming a map. Only the supplied string is used as the file URL.
struct FileUrl(String);

impl<'de> Deserialize<'de> for FileUrl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FileVisitor;
        impl<'de> Visitor<'de> for FileVisitor {
            type Value = FileUrl;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("one file_url string")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<FileUrl, A::Error> {
                let mut value = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key != "file_url" || value.is_some() {
                        return Err(serde::de::Error::custom("unsupported resolver shape"));
                    }
                    let next = map.next_value::<String>()?;
                    if next.is_empty() || next.trim() != next {
                        return Err(serde::de::Error::custom("unsupported resolver value"));
                    }
                    value = Some(next);
                }
                value.map(FileUrl).ok_or_else(|| serde::de::Error::custom("missing resolver value"))
            }
        }
        deserializer.deserialize_map(FileVisitor)
    }
}

#[derive(Default)]
struct Links {
    current: Option<(String, String)>,
    anchors: Vec<(String, String)>,
    failed: bool,
    count: usize,
}
struct LinkSink(RefCell<Links>);

impl TokenSink for LinkSink {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut links = self.0.borrow_mut();
        links.count += 1;
        if links.count > 50_000 {
            links.failed = true;
            return TokenSinkResult::Continue;
        }
        match token {
            Token::TagToken(tag)
                if matches!(tag.name.as_ref(), "script" | "style") && tag.kind == TagKind::StartTag =>
            {
                if links.current.is_some() {
                    links.failed = true;
                }
                return TokenSinkResult::RawData(if tag.name.as_ref() == "script" {
                    html5ever::tokenizer::states::RawKind::ScriptData
                } else {
                    html5ever::tokenizer::states::RawKind::Rawtext
                });
            }
            Token::TagToken(tag) if tag.name.as_ref() == "a" && tag.kind == TagKind::StartTag => {
                if links.current.is_some() || tag.attrs.len() > 64 {
                    links.failed = true;
                }
                let href =
                    tag.attrs.iter().find(|attr| attr.name.local.as_ref() == "href").map(|attr| attr.value.to_string());
                if let Some(href) = href {
                    links.current = Some((href, String::new()));
                }
            }
            Token::TagToken(tag) if tag.name.as_ref() == "a" && tag.kind == TagKind::EndTag => {
                if let Some(link) = links.current.take() {
                    links.anchors.push(link);
                }
            }
            Token::CharacterTokens(text) => {
                if let Some((_, label)) = &mut links.current {
                    label.push_str(&text);
                }
            }
            Token::NullCharacterToken => links.failed = true,
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

/// Selects only actual mail hrefs, with no guessed suffixes or script-discovered URLs. Duplicates are harmless;
/// two distinct matching targets are ambiguous. Only hosts/labels, never links, belong in the package.
pub fn select(html: &str, host: &str, label: Option<&str>, pdf_only: bool) -> Result<Link, Failure> {
    let links = anchors(html)?;
    let mut selected: Option<Link> = None;
    for (href, text) in links {
        if label.is_some_and(|label| !text.split_whitespace().collect::<Vec<_>>().join(" ").eq_ignore_ascii_case(label))
        {
            continue;
        }
        let Ok(link) = checked(&href, &[host.into()]) else { continue };
        if pdf_only && !link.0.path().to_ascii_lowercase().ends_with(".pdf") {
            continue;
        }
        if selected.as_ref().is_some_and(|old| old.0 != link.0) {
            return Err(unsupported());
        }
        selected = Some(link);
    }
    selected.ok_or_else(unsupported)
}

pub fn direct_host(html: &str) -> Result<String, Failure> {
    let mut selected: Option<Link> = None;
    for (href, _) in anchors(html)? {
        let Ok(url) = Url::parse(&href) else { continue };
        if !url.path().to_ascii_lowercase().ends_with(".pdf") {
            continue;
        }
        let Some(host) = url.domain() else { continue };
        let Ok(link) = checked(&href, &[host.into()]) else { continue };
        if selected.as_ref().is_some_and(|old| old.0 != link.0) {
            return Err(unsupported());
        }
        selected = Some(link);
    }
    selected.map(|link| link.host().into()).ok_or_else(unsupported)
}

fn anchors(html: &str) -> Result<Vec<(String, String)>, Failure> {
    if html.len() > MAX_HTML {
        return Err(unsupported());
    }
    let tokenizer = Tokenizer::new(LinkSink(RefCell::new(Links::default())), TokenizerOpts::default());
    let queue = BufferQueue::default();
    queue.push_back(StrTendril::from_slice(html));
    let _ = tokenizer.feed(&queue);
    tokenizer.end();
    let links = tokenizer.sink.0.into_inner();
    if links.failed || links.current.is_some() {
        return Err(unsupported());
    }
    Ok(links.anchors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_accepts_only_one_file_url_string() {
        let fixture = include_str!("../../tests/fixtures/workflows/stripe_file_url.json");
        let FileUrl(url) = serde_json::from_str(fixture).expect("synthetic resolver fixture");
        assert!(checked(&url, &[STRIPE_FILE.into()]).is_ok());
        for json in [
            r#"{}"#,
            r#"{"file_url":null}"#,
            r#"{"file_url":""}"#,
            r#"{"file_url":"x","file_url":"y"}"#,
            r#"{"file_url":"x","other":1}"#,
            r#"{"file_url":" x"}"#,
        ] {
            assert!(serde_json::from_str::<FileUrl>(json).is_err());
        }
        assert!(
            serde_json::from_str::<FileUrl>(
                r#"{"file_url":"https://stripe-upload-api.s3.us-west-1.amazonaws.com/synthetic.pdf"}"#
            )
            .is_ok()
        );
    }

    #[test]
    fn exact_https_hosts_and_actual_invoice_links_only() {
        let hosts = vec![STRIPE_PAGE.into()];
        for input in [
            "http://invoice.stripe.com/i/a/b",
            "https://invoice.stripe.com.evil.example/i/a/b",
            "https://invoice.stripe.com:8443/i/a/b",
            "https://x@invoice.stripe.com/i/a/b",
            "https://invoice.stripe.com/receipt/a/b",
            "https://127.0.0.1/i/a/b",
            "https://invoice.stripe.com/i/a/b#x",
        ] {
            assert!(checked(input, &hosts).is_err());
        }
        let mail = torro_mail_imap::mime::read(include_bytes!("../../tests/fixtures/workflows/stripe_invoice.eml"))
            .expect("synthetic invoice mail");
        let html = mail.html.expect("actual HTML href, no guessed suffix");
        let link = select(&html, STRIPE_PAGE, Some("Invoice"), false).expect("supplied invoice link");
        let endpoint = stripe_endpoint(&link).expect("bounded endpoint");
        assert_eq!(endpoint.0.path(), "/invoice_pdf_file_url/acct_synthetic/token_synthetic");
        assert!(select("<a href='https://invoice.stripe.com/i/a/b'>Invoice</a><a href='https://invoice.stripe.com/i/a/c'>Invoice</a>", STRIPE_PAGE, None, false).is_err());
    }

    #[tokio::test]
    async fn redirects_types_expiry_and_network_budgets_stay_on_the_guarded_product_path() {
        let server = httpmock::MockServer::start_async().await;
        let foreign = server
            .mock_async(|when, then| {
                when.path("/files.example.org/redirect.pdf");
                then.status(302).header("location", "https://evil.example/private.pdf");
            })
            .await;
        let expired = server
            .mock_async(|when, then| {
                when.path("/files.example.org/expired.pdf");
                then.status(410);
            })
            .await;
        let html = server
            .mock_async(|when, then| {
                when.path("/files.example.org/oversized.pdf");
                then.status(200).header("content-type", "text/html").body("x".repeat(2 * 1024 * 1024 + 1));
            })
            .await;
        let downloader = Downloader::for_test(&server.base_url());
        let hosts = vec!["files.example.org".into()];
        let link = |name| checked(&format!("https://files.example.org/{name}.pdf"), &hosts).expect("synthetic link");
        assert_eq!(
            downloader
                .fetch(link("redirect"), &hosts, &mut Budget::new(), &|| Ok(()))
                .await
                .err()
                .expect("foreign redirect")
                .reason,
            RunReason::Unsupported
        );
        assert_eq!(
            downloader
                .fetch(link("expired"), &hosts, &mut Budget::new(), &|| Ok(()))
                .await
                .err()
                .expect("explicit expiry")
                .reason,
            RunReason::Expired
        );
        assert!(downloader.fetch(link("oversized"), &hosts, &mut Budget::new(), &|| Ok(())).await.is_err());
        let mut budget = Budget::new();
        budget.gets = 8;
        assert!(downloader.fetch(link("expired"), &hosts, &mut budget, &|| Ok(())).await.is_err());
        foreign.assert_calls_async(1).await;
        expired.assert_calls_async(1).await;
        html.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn presentation_resources_are_bounded_typed_data_from_the_package_hosts_only() {
        let hosts: Vec<String> = vec!["my.paddle.com".into(), "static.paddle.com".into()];
        for href in ["https://static.paddle.com/a.css", "//static.paddle.com/a.css", "/assets/a.css?v=1"] {
            assert!(resource_link("my.paddle.com", href, &hosts).is_ok(), "{href}");
        }
        for href in ["a.css", "../a.css", "http://static.paddle.com/a.css", "https://cdn.example.org/a.css", "/a.css#x"]
        {
            assert!(resource_link("my.paddle.com", href, &hosts).is_err(), "{href}");
        }
        let pinned = resource_link("my.paddle.com", "/assets/a.js?v=2", &hosts).expect("root-relative");
        assert_eq!((pinned.host(), pinned.path_and_query().as_str()), ("my.paddle.com", "/assets/a.js?v=2"));

        let server = httpmock::MockServer::start_async().await;
        let css = server
            .mock_async(|when, then| {
                when.path("/static.paddle.com/ok.css");
                then.status(200).header("content-type", "text/css; charset=utf-8").body(".a{color:red}");
            })
            .await;
        let js = server
            .mock_async(|when, then| {
                when.path("/static.paddle.com/ok.js");
                then.status(200).header("content-type", "text/javascript").body("void 0;");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.path("/static.paddle.com/html.css");
                then.status(200).header("content-type", "text/html").body("<p>no</p>");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.path("/static.paddle.com/big.css");
                then.status(200).header("content-type", "text/css").body("a".repeat(MAX_RESOURCE + 1));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.path("/static.paddle.com/away.css");
                then.status(302).header("location", "https://cdn.example.org/a.css");
            })
            .await;
        let downloader = Downloader::for_test(&server.base_url());
        let link = |path: &str| resource_link("my.paddle.com", &format!("https://static.paddle.com/{path}"), &hosts);
        let mut budget = Budget::new();
        let body = downloader
            .fetch_resource(link("ok.css").expect("css"), &hosts, &mut budget, &|| Ok(()), ResourceKind::Stylesheet)
            .await
            .expect("stylesheet as data");
        assert_eq!(body.bytes, b".a{color:red}");
        assert!(
            downloader
                .fetch_resource(link("ok.js").expect("js"), &hosts, &mut budget, &|| Ok(()), ResourceKind::Script)
                .await
                .is_ok()
        );
        assert_eq!(budget.gets, 2, "resources share the page's GET budget");
        for (path, kind) in [
            ("ok.css", ResourceKind::Script),
            ("html.css", ResourceKind::Stylesheet),
            ("big.css", ResourceKind::Stylesheet),
            ("away.css", ResourceKind::Stylesheet),
        ] {
            let failure = downloader
                .fetch_resource(link(path).expect("link"), &hosts, &mut Budget::new(), &|| Ok(()), kind)
                .await
                .err()
                .expect("refused");
            assert_eq!(failure.reason, RunReason::Unsupported, "{path}");
        }
        let revoked = downloader
            .fetch_resource(
                link("ok.css").expect("css"),
                &hosts,
                &mut Budget::new(),
                &|| Err(Failure::fixed(RunReason::PermissionRevoked)),
                ResourceKind::Stylesheet,
            )
            .await
            .err()
            .expect("rechecked before the GET");
        assert_eq!(revoked.reason, RunReason::PermissionRevoked);
        css.assert_calls_async(2).await;
        js.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn stripe_http_400_requires_an_explicit_bounded_expiry_indicator() {
        // Synthetic error variants, not a recording or claimed acceptance of a private invoice.
        let cases = [
            ("application/json", r#"{"error":"This invoice link has expired"}"#.to_string(), RunReason::Expired),
            ("application/json", r#"{"error":{"code":"invoice_link_expired"}}"#.into(), RunReason::Expired),
            ("application/json", r#"{"error":{"message":"This invoice link has expired"}}"#.into(), RunReason::Expired),
            ("text/plain; charset=utf-8", "This invoice link has expired.\n".into(), RunReason::Expired),
            ("application/json", r#"{"error":"Invalid invoice token"}"#.into(), RunReason::Unsupported),
            ("application/json", r#"{"error":"This invoice link has not expired"}"#.into(), RunReason::Unsupported),
            ("application/json", r#"{"other":"This invoice link has expired"}"#.into(), RunReason::Unsupported),
            ("application/json", r#"["This invoice link has expired"]"#.into(), RunReason::Unsupported),
            ("application/json", r#"{"error":["invoice_link_expired"]}"#.into(), RunReason::Unsupported),
            (
                "application/json",
                r#"{"error":{"code":"invalid_request","message":"This invoice link has expired"}}"#.into(),
                RunReason::Unsupported,
            ),
            (
                "application/json",
                r#"{"error":{"code":"invalid_request","code":"invoice_link_expired"}}"#.into(),
                RunReason::Unsupported,
            ),
            ("text/plain", "expired".into(), RunReason::Unsupported),
            (
                "application/json",
                r#"{"error":"This invoice link has expired","error":"Other failure"}"#.into(),
                RunReason::Unsupported,
            ),
            ("application/json", "{invalid}".into(), RunReason::Unsupported),
            ("application/json", "x".repeat(16 * 1024 + 1), RunReason::Unsupported),
            ("text/html", "<script>This invoice link has expired</script>".into(), RunReason::Unsupported),
        ];
        let hosts = vec![STRIPE_PAGE.into(), STRIPE_DATA.into(), STRIPE_FILE.into()];
        let server = httpmock::MockServer::start_async().await;
        let downloader = Downloader::for_test(&server.base_url());
        for (index, (mime, body, reason)) in cases.into_iter().enumerate() {
            let path = format!("/{STRIPE_DATA}/invoice_pdf_file_url/synthetic/case_{index}");
            let mock = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::GET).path(path);
                    then.status(400).header("content-type", mime).body(body);
                })
                .await;
            let link =
                checked(&format!("https://{STRIPE_PAGE}/i/synthetic/case_{index}"), &hosts).expect("synthetic link");
            let mut budget = Budget::new();
            let error = downloader
                .resolve_stripe_invoice_pdf(link, &hosts, &mut budget, &|| Ok(()))
                .await
                .err()
                .expect("no file from HTTP400");
            assert_eq!(error, Failure { reason, retryable: false }, "synthetic case {index}");
            assert_eq!(budget.gets, 1, "never follow an error payload to a PDF or retry a permanent error");
            mock.assert_calls_async(1).await;
        }
        let foreign = server
            .mock_async(|when, then| {
                when.path("/files.example.org/invoice.pdf");
                then.status(400)
                    .header("content-type", "application/json")
                    .body(r#"{"error":"This invoice link has expired"}"#);
            })
            .await;
        let direct_hosts = vec!["files.example.org".into()];
        let link = checked("https://files.example.org/invoice.pdf", &direct_hosts).expect("direct link");
        assert_eq!(
            downloader
                .fetch(link, &direct_hosts, &mut Budget::new(), &|| Ok(()))
                .await
                .err()
                .expect("generic400")
                .reason,
            RunReason::Unsupported
        );
        foreign.assert_calls_async(1).await;
        let revoked = std::sync::atomic::AtomicBool::new(false);
        let allow = || {
            if revoked.swap(true, std::sync::atomic::Ordering::SeqCst) {
                Err(Failure::fixed(RunReason::PermissionRevoked))
            } else {
                Ok(())
            }
        };
        let link = checked(&format!("https://{STRIPE_PAGE}/i/synthetic/case_0"), &hosts).expect("synthetic link");
        assert_eq!(
            downloader
                .resolve_stripe_invoice_pdf(link, &hosts, &mut Budget::new(), &allow)
                .await
                .err()
                .expect("revoked while response awaited")
                .reason,
            RunReason::PermissionRevoked
        );
    }

    #[tokio::test]
    async fn stripe_error_without_content_length_cannot_bypass_the_error_byte_limit() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("local server");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("GET");
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.expect("bounded request"));
                assert!(request.len() < 4096);
            }
            let body = serde_json::to_vec(
                &serde_json::json!({"error":"This invoice link has expired", "padding":"x".repeat(MAX_STRIPE_ERROR)}),
            )
            .expect("synthetic JSON");
            let head = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).await.expect("head");
            socket.write_all(&body).await.expect("body");
            // The reader may close as soon as it sees the bound exceeded.
            let _ = socket.write_all(b"\r\n0\r\n\r\n").await;
        });
        let downloader = Downloader::for_test(&endpoint);
        let hosts = vec![STRIPE_PAGE.into(), STRIPE_DATA.into(), STRIPE_FILE.into()];
        let link = checked(&format!("https://{STRIPE_PAGE}/i/synthetic/chunked"), &hosts).expect("synthetic link");
        let error = downloader
            .resolve_stripe_invoice_pdf(link, &hosts, &mut Budget::new(), &|| Ok(()))
            .await
            .err()
            .expect("bounded error");
        assert_eq!(error, Failure::fixed(RunReason::Unsupported));
        server.await.expect("server finished");
    }

    #[tokio::test]
    async fn received_stripe_http_400_is_permanent_even_when_its_body_is_interrupted() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("local server");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("GET");
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.expect("request"));
                assert!(request.len() < 4096);
            }
            socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n{\"error\":").await.expect("interrupted error body");
        });
        let downloader = Downloader::for_test(&endpoint);
        let hosts = vec![STRIPE_PAGE.into(), STRIPE_DATA.into(), STRIPE_FILE.into()];
        let link = checked(&format!("https://{STRIPE_PAGE}/i/synthetic/interrupted"), &hosts).expect("synthetic link");
        let error = downloader
            .resolve_stripe_invoice_pdf(link, &hosts, &mut Budget::new(), &|| Ok(()))
            .await
            .err()
            .expect("incomplete error");
        assert_eq!(
            error,
            Failure::fixed(RunReason::Unsupported),
            "an observed HTTP400 must not trigger network retries"
        );
        server.await.expect("server finished");
    }

    #[tokio::test]
    async fn a_timeout_after_http_400_is_permanent_but_permission_failures_are_preserved() {
        let timed_out = tokio::time::timeout(std::time::Duration::ZERO, std::future::pending::<()>()).await;
        assert!(timed_out.is_err());
        let timeout_error = Failure { reason: RunReason::Offline, retryable: true };
        assert_eq!(after_http_400(true, timeout_error), Failure::fixed(RunReason::Unsupported));
        assert_eq!(
            after_http_400(false, timeout_error),
            timeout_error,
            "no HTTP response still permits bounded network retries"
        );
        let revoked = Failure::fixed(RunReason::PermissionRevoked);
        assert_eq!(after_http_400(true, revoked), revoked);
    }

    #[tokio::test]
    async fn private_and_mixed_dns_answers_are_rejected_before_a_socket_is_opened() {
        use torro_pim_caldav::discover::{Lookup, MxRecord, SrvRecord};
        struct Dns(Vec<std::net::IpAddr>);
        impl Resolver for Dns {
            fn srv<'a>(&'a self, _: &'a str) -> Lookup<'a, SrvRecord> {
                Box::pin(async { Ok(Vec::new()) })
            }
            fn txt<'a>(&'a self, _: &'a str) -> Lookup<'a, String> {
                Box::pin(async { Ok(Vec::new()) })
            }
            fn mx<'a>(&'a self, _: &'a str) -> Lookup<'a, MxRecord> {
                Box::pin(async { Ok(Vec::new()) })
            }
            fn ips<'a>(&'a self, _: &'a str) -> Lookup<'a, std::net::IpAddr> {
                Box::pin(async { Ok(self.0.clone()) })
            }
        }
        for answers in [
            vec!["127.0.0.1"],
            vec!["192.168.1.1"],
            vec!["169.254.169.254"],
            vec!["::1"],
            vec!["fc00::1"],
            vec!["1.1.1.1", "127.0.0.1"],
        ] {
            let dns = Dns(answers.into_iter().map(|address| address.parse().expect("test IP")).collect());
            let downloader = Downloader::new(Arc::new(dns)).expect("guarded client");
            let hosts = vec!["files.example.org".into()];
            let link =
                checked("https://files.example.org/invoice.pdf", &hosts).expect("valid domain alone is insufficient");
            assert!(downloader.fetch(link, &hosts, &mut Budget::new(), &|| Ok(())).await.is_err());
        }
    }
}
