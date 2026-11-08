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
        self.fetch_bounded(link, hosts, budget, allow, false).await
    }

    async fn fetch_bounded(
        &self,
        mut link: Link,
        hosts: &[String],
        budget: &mut Budget,
        allow: &(dyn Fn() -> Result<(), Failure> + Sync),
        json: bool,
    ) -> Result<Body, Failure> {
        loop {
            link = checked(link.0.as_str(), hosts)?;
            let remaining = budget.remaining()?;
            if budget.gets >= 8 {
                return Err(unsupported());
            }
            allow()?;
            budget.gets += 1;
            let read = async {
                let mut response = self
                    .http
                    .get(self.request_url(&link))
                    .header(ACCEPT, if json { "application/json" } else { "application/pdf, text/html;q=0.9" })
                    .send()
                    .await
                    .map_err(|_| Failure { reason: RunReason::Offline, retryable: true })?;
                let status = response.status().as_u16();
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
                let limit = match (json, mime.as_str()) {
                    (true, "application/json") => 256 * 1024,
                    (false, "application/pdf") => MAX_PDF,
                    (false, "text/html") => MAX_HTML,
                    _ => return Err(Failure::fixed(RunReason::UnexpectedType)),
                };
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
                Ok(Ok(Body { bytes, content_type, source_host: link.host().into() }))
            };
            match tokio::time::timeout(remaining, read)
                .await
                .map_err(|_| Failure { reason: RunReason::Offline, retryable: true })??
            {
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
        let json = self.fetch_bounded(endpoint, &[STRIPE_DATA.into()], budget, allow, true).await?;
        let FileUrl(url) = serde_json::from_slice::<FileUrl>(&json.bytes).map_err(|_| unsupported())?;
        let target = checked(&url, &[STRIPE_FILE.into()])?;
        let body = self.fetch_bounded(target, &[STRIPE_FILE.into()], budget, allow, false).await?;
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
