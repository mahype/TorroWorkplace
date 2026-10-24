//! The tool `web` in the core (issue #305, `docs/assistent.md` "Werkzeug Web"): searching the web with the
//! providers a person switched on, and (task WEB.3) reading one page.
//!
//! - [`SearchProvider`]: one search service, with a pure `parse` of its answer per provider
//!   ([`providers`]) and a thin HTTP part.
//! - [`router::Router`]: asks the switched-on providers in [`SearchProviderId::ORDER`], falls back on a block, a
//!   limit or a missing key, adds Wikipedia's hits, throttles, pauses and keeps answers an hour in memory.
//! - [`catalogue`]: what each provider is, where the words go and on what terms, for `tools.list_search_providers`.
//! - [`fetch::Fetcher`]: reads one page (task WEB.3), only from the public internet, as Markdown; with
//!   [`robots`], a reader of `robots.txt` after RFC 9309, for the addresses the model picks.
//!
//! The commands (`tools.web_search`, `tools.fetch_page`, `tools.list_search_providers`) come with task WEB.4.
//! Nothing here logs or keeps search words, addresses or a key in an error: errors carry fixed texts only
//! ([`SearchError`], [`fetch::FetchError`]).

pub mod catalogue;
pub mod fetch;
pub mod providers;
pub mod robots;
pub mod router;

use std::time::Duration;

use torro_core::ai::BoxFuture;
use torro_ipc::web::{MAX_HITS, SearchHit, SearchProviderId};

/// One search as the providers get it. Build it with [`Query::new`], which checks the count and the
/// language; the core has checked the words (`tools.web_search`: not blank, at most `MAX_QUERY_CHARS`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// The search words, trimmed. They go to a provider outside the suite: never log them.
    pub words: String,
    /// How many hits at most, 1 to [`MAX_HITS`].
    pub count: usize,
    /// The language of the hits, an ISO 639-1 code in lower case (`de`, `en`); always two letters, so a
    /// provider may put it in a host name (Wikipedia: `de.wikipedia.org`).
    pub language: String,
}

impl Query {
    /// The search for `words` (trimmed) with at most `count` hits (`None`: 5; clamped to 1 to [`MAX_HITS`]) in
    /// `language`: the given ISO 639-1 code when it is two ASCII letters (in lower case), else the suite's language
    /// (`en` or `de`).
    pub fn new(words: &str, count: Option<u8>, language: Option<&str>) -> Query {
        let count = count.map_or(DEFAULT_HITS, usize::from).clamp(1, MAX_HITS);
        let language = language
            .map(|code| code.trim().to_ascii_lowercase())
            .filter(|code| code.len() == 2 && code.bytes().all(|b| b.is_ascii_lowercase()))
            .unwrap_or_else(|| suite_language().to_string());
        Query { words: words.trim().to_string(), count, language }
    }
}

/// How many hits a search gives when it names no count (`tools.web_search`).
pub const DEFAULT_HITS: usize = 5;

/// The suite's language as an ISO 639-1 code: `en` or `de`.
pub fn suite_language() -> &'static str {
    match torro_core::i18n::lang() {
        torro_core::i18n::Lang::De => "de",
        torro_core::i18n::Lang::En => "en",
    }
}

/// What a provider can do, for the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// The most hits one request may ask for; the provider asks for `min(query.count, max_hits)`.
    pub max_hits: usize,
    /// It takes [`Query::language`] into account (a parameter or a host of that language); a provider without
    /// one finds hits in any language.
    pub languages: bool,
    /// Its answers may be kept in the router's memory for an hour. `false` for Brave, whose terms allow results
    /// only for the answer; the router never keeps Brave's, whatever this says.
    pub cacheable: bool,
}

/// Why a provider gave no hits. Nothing of the request is in it: no words, no address, no key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchError {
    /// The provider says: too many requests (HTTP 429, a daily limit, no credits left). The router rests it for
    /// `retry_after` when the provider named a time, else an hour, and asks the next one.
    RateLimited {
        /// How long the provider asked to wait (`Retry-After` in seconds), when it said.
        retry_after: Option<Duration>,
    },
    /// The provider refused the core (a captcha, an anomaly page, 403, DuckDuckGo's 202). Never worked around:
    /// the router rests it for an hour and asks the next one.
    Blocked,
    /// The provider needs a key and none is stored, or it refused the key (401, 403). The router asks the next.
    NeedsKey,
    /// Anything else, with a fixed text for people and logs ([`OFFLINE`], [`BROKEN`]); the router stops there.
    Failed(&'static str),
}

/// [`SearchError::Failed`]: the provider cannot be reached (no connection, a time limit).
pub const OFFLINE: &str = "the search provider cannot be reached";

/// [`SearchError::Failed`]: the provider answered with an error or something that is not its answer.
pub const BROKEN: &str = "the search provider gave no usable answer";

/// One search service. Implementations hold their HTTP client, address and key; they never log the query, the
/// address or the key, and they never retry, change their user agent or work around a block: one request per
/// [`SearchProvider::search`].
pub trait SearchProvider: Send + Sync {
    /// Which provider this is.
    fn id(&self) -> SearchProviderId;

    /// What it can do.
    fn capabilities(&self) -> Capabilities;

    /// Asks the provider for `query` once: its hits, best first, at most `query.count` (and
    /// [`Capabilities::max_hits`]), each with an absolute `http`/`https` address, a plain-text snippet and
    /// `source` set to [`SearchProvider::id`]. A provider without its key answers [`SearchError::NeedsKey`]
    /// without sending anything.
    fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_keeps_count_and_language_in_bounds() {
        let query = Query::new("  Lisbon trams ", None, Some("PT"));
        assert_eq!(query, Query { words: "Lisbon trams".into(), count: DEFAULT_HITS, language: "pt".into() });
        assert_eq!(Query::new("x", Some(0), None).count, 1);
        assert_eq!(Query::new("x", Some(99), None).count, MAX_HITS);
        for odd in ["evil.example.org/", "d", "deu", "d1", ""] {
            assert_eq!(Query::new("x", None, Some(odd)).language, suite_language(), "{odd:?} is no language");
        }
    }
}
