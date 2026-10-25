//! The seven search providers of the tool `web` (task WEB.2) and what they share: one HTTP client with the suite's
//! user agent, one way to send a request without leaking its address, and the tidying of their hits.
//!
//! Each provider file has a pure `parse` of the provider's answer (tested against files in
//! `tests/fixtures/web/`) and a thin HTTP part with a constructor that takes the base address, so tests ask a
//! local fake server instead of the provider. [`for_settings`] builds the switched-on ones for a search.
//!
//! The rules of `docs/assistent.md` hold here: one request per search, no retry, no other user agent, no proxy;
//! a block or a limit is reported, never worked around. A `reqwest::Error` loses its URL before anything looks at
//! it (Mojeek carries the key in the address, every provider the words).

pub mod brave;
pub mod duckduckgo;
pub mod exa;
pub mod mojeek;
pub mod searxng;
pub mod tavily;
pub mod wikipedia;

use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use torro_core::settings::WebSearch;
use torro_ipc::web::{SearchHit, SearchProviderId};

use super::{BROKEN, OFFLINE, SearchError, SearchProvider};

/// The most bytes of one answer read; a longer one is no answer of a search provider.
pub const MAX_ANSWER_BYTES: usize = 2 * 1024 * 1024;

/// The most characters of a snippet; a longer one is cut with "…".
pub const MAX_SNIPPET_CHARS: usize = 400;

/// The HTTP client of every provider: the suite's user agent (`places::USER_AGENT`, never a browser's), short time
/// limits, no redirects (an API that redirects gives no answer, and the words would go to another host). `None`
/// when none can be built.
pub fn http_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(crate::places::USER_AGENT)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()
}

/// The providers of `web` a search can ask ([`WebSearch::askable`]: switched on, in [`SearchProviderId::ORDER`],
/// SearXNG only with an address), each with the key `key` gives for it (from the keychain, `websearch-<id>`; only
/// asked for providers that need one) and asking `http`. A provider whose key is missing is built too: it answers [`SearchError::NeedsKey`]
/// without sending anything, so the router can tell "only keys missing" apart.
pub fn for_settings(
    http: &reqwest::Client,
    web: &WebSearch,
    key: impl Fn(SearchProviderId) -> Option<String>,
) -> Vec<Arc<dyn SearchProvider>> {
    let built = |id: SearchProviderId| -> Option<Arc<dyn SearchProvider>> {
        let http = http.clone();
        Some(match id {
            SearchProviderId::Searxng => Arc::new(searxng::Searxng::new(http, web.searxng.as_deref()?)),
            SearchProviderId::Brave => Arc::new(brave::Brave::new(http, key(id))),
            SearchProviderId::Tavily => Arc::new(tavily::Tavily::new(http, key(id))),
            SearchProviderId::Exa => Arc::new(exa::Exa::new(http, key(id))),
            SearchProviderId::Mojeek => Arc::new(mojeek::Mojeek::new(http, key(id))),
            SearchProviderId::Duckduckgo => Arc::new(duckduckgo::Duckduckgo::new(http)),
            SearchProviderId::Wikipedia => Arc::new(wikipedia::Wikipedia::new(http)),
        })
    };
    web.askable().into_iter().filter_map(built).collect()
}

/// What came back from a provider: the status, the `Retry-After` it named and the body.
pub(crate) struct Answer {
    pub status: StatusCode,
    pub retry_after: Option<Duration>,
    pub body: Vec<u8>,
}

/// `base` with the query `params`, only `http` and `https`; [`BROKEN`] for anything else (a SearXNG address that
/// is none). The address holds the words (and Mojeek's key): never log it.
pub(crate) fn address(base: &str, params: &[(&str, &str)]) -> Result<reqwest::Url, SearchError> {
    reqwest::Url::parse_with_params(base, params)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .ok_or(SearchError::Failed(BROKEN))
}

/// Sends `request` once and reads its answer, at most [`MAX_ANSWER_BYTES`]. No error keeps the URL.
pub(crate) async fn send(request: reqwest::RequestBuilder) -> Result<Answer, SearchError> {
    let mut response = request.send().await.map_err(|err| net_failure(&err.without_url()))?;
    let status = response.status();
    let retry_after = retry_after(response.headers());
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| net_failure(&err.without_url()))? {
        if body.len() + chunk.len() > MAX_ANSWER_BYTES {
            return Err(SearchError::Failed(BROKEN));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Answer { status, retry_after, body })
}

/// The body of `answer` when it is a success; else why not: 429 a limit, 401 and 403 `refused` (a key provider's
/// [`SearchError::NeedsKey`], an open one's [`SearchError::Blocked`]), anything else [`BROKEN`].
pub(crate) fn checked(answer: Answer, refused: SearchError) -> Result<Vec<u8>, SearchError> {
    match answer.status {
        status if status.is_success() => Ok(answer.body),
        StatusCode::TOO_MANY_REQUESTS => Err(SearchError::RateLimited { retry_after: answer.retry_after }),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(refused),
        _ => Err(SearchError::Failed(BROKEN)),
    }
}

/// A failed request; `err` has lost its URL already.
fn net_failure(err: &reqwest::Error) -> SearchError {
    if err.is_connect() || err.is_timeout() { SearchError::Failed(OFFLINE) } else { SearchError::Failed(BROKEN) }
}

/// `Retry-After` in seconds; an HTTP date (rare for APIs) counts as none, so the router rests for its hour.
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// The header value of a key, marked sensitive so it never shows in a debug print. `None` for a key that cannot be
/// a header (the provider then answers [`SearchError::NeedsKey`]).
pub(crate) fn key_header(key: &str) -> Option<reqwest::header::HeaderValue> {
    let mut value = reqwest::header::HeaderValue::from_str(key.trim()).ok()?;
    value.set_sensitive(true);
    Some(value)
}

/// The text of an HTML extract (`<strong>`, `<span class="searchmatch">`, entities) without its markup.
pub(crate) fn plain_text(html: &str) -> String {
    dom_query::Document::fragment(html).text().to_string()
}

/// A hit of `source`, or `None` when `url` is no absolute `http`/`https` address. Title and snippet get their
/// whitespace folded; the snippet is cut at [`MAX_SNIPPET_CHARS`]; a blank title becomes the address.
pub(crate) fn hit(source: SearchProviderId, title: &str, url: &str, snippet: &str) -> Option<SearchHit> {
    let url = reqwest::Url::parse(url.trim()).ok().filter(|url| matches!(url.scheme(), "http" | "https"))?;
    let title = folded(title);
    Some(SearchHit {
        title: if title.is_empty() { url.to_string() } else { title },
        url: url.to_string(),
        snippet: cut(&folded(snippet), MAX_SNIPPET_CHARS),
        source,
        license: None,
    })
}

/// `text` with every run of whitespace as one space, trimmed.
fn folded(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` cut to `max` characters, the last one "…" when cut.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut short: String = text.chars().take(max.saturating_sub(1)).collect();
    short.push('…');
    short
}

#[cfg(test)]
pub(crate) mod testing {
    //! What the provider tests share: the fixtures and a client without the time limits' surprises.

    use std::path::Path;

    /// The fixture `name` from `tests/fixtures/web/`.
    pub fn fixture(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/web").join(name);
        std::fs::read(path).expect("fixture")
    }

    /// The providers' client, as in the service.
    pub fn http() -> reqwest::Client {
        super::http_client().expect("a client")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn hits_are_tidied_and_need_a_web_address() {
        let found =
            hit(SearchProviderId::Exa, "  Lisbon \n trams ", "https://example.org/a", " one\n two ").expect("a hit");
        assert_eq!((found.title.as_str(), found.snippet.as_str()), ("Lisbon trams", "one two"));
        assert_eq!(
            hit(SearchProviderId::Exa, "", "https://example.org/", "").expect("a hit").title,
            "https://example.org/"
        );
        for bad in ["javascript:alert(1)", "/relative", "file:///etc/passwd", ""] {
            assert!(hit(SearchProviderId::Exa, "t", bad, "").is_none(), "{bad:?} is no web address");
        }
        let long = "x".repeat(MAX_SNIPPET_CHARS + 10);
        let snippet = hit(SearchProviderId::Exa, "t", "https://example.org/", &long).expect("a hit").snippet;
        assert_eq!(snippet.chars().count(), MAX_SNIPPET_CHARS);
        assert!(snippet.ends_with('…'));
    }

    #[test]
    fn extracts_lose_their_markup() {
        assert_eq!(
            plain_text("<strong>Lisbon</strong> &amp; Porto <span class=\"searchmatch\">x</span>"),
            "Lisbon & Porto x"
        );
    }

    #[test]
    fn only_switched_on_providers_are_built_in_order() {
        let http = testing::http();
        let mut web = WebSearch {
            providers: BTreeSet::from([
                SearchProviderId::Wikipedia,
                SearchProviderId::Duckduckgo,
                SearchProviderId::Brave,
                SearchProviderId::Searxng,
            ]),
            searxng: None,
        };
        let asked_for_keys = std::sync::Mutex::new(Vec::new());
        let key = |id| {
            asked_for_keys.lock().expect("lock").push(id);
            None
        };
        let ids: Vec<_> = for_settings(&http, &web, key).iter().map(|p| p.id()).collect();
        assert_eq!(
            ids,
            [SearchProviderId::Brave, SearchProviderId::Duckduckgo, SearchProviderId::Wikipedia],
            "SearXNG needs its address"
        );
        assert_eq!(*asked_for_keys.lock().expect("lock"), [SearchProviderId::Brave], "only keys of switched-on ones");
        web.searxng = Some("https://search.example.org".into());
        assert_eq!(for_settings(&http, &web, |_| None)[0].id(), SearchProviderId::Searxng);
        assert!(for_settings(&http, &WebSearch::default(), |_| None).is_empty());
    }
}
