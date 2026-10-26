//! The person's own SearXNG (`GET <address>/search?q=…&format=json&language=…`), as documented at
//! <https://docs.searxng.org/dev/search_api.html> (checked 2026-10-24). JSON must be switched on in the instance's
//! `settings.yml`; without it SearXNG answers 403, which counts as a refusal ([`SearchError::Blocked`]), as does a
//! 429 of its limiter as a limit. The address may lie in the local network: the person set it.

use serde::Deserialize;
use torro_core::ai::BoxFuture;
use torro_ipc::web::{SearchHit, SearchProviderId};

use super::{address, checked, hit, send};
use crate::tools::web::{BROKEN, Capabilities, Query, SearchError, SearchProvider};

/// One page of SearXNG holds about this many results; the router asks for one page.
const MAX_HITS: usize = 20;

/// The person's SearXNG at its address.
#[derive(Debug)]
pub struct Searxng {
    http: reqwest::Client,
    base: String,
}

impl Searxng {
    /// SearXNG at `address`, the base address from the settings (`torro_core::settings::WebSearch::searxng`,
    /// checked there: `http`/`https`, no query, no sign-in); tests give a fake server's.
    pub fn new(http: reqwest::Client, address: &str) -> Searxng {
        Searxng { http, base: address.trim().trim_end_matches('/').to_string() }
    }
}

impl SearchProvider for Searxng {
    fn id(&self) -> SearchProviderId {
        SearchProviderId::Searxng
    }

    /// The person may move to another instance; its answers are not this one's.
    fn instance(&self) -> &str {
        &self.base
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { max_hits: MAX_HITS, languages: true, cacheable: true }
    }

    fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>> {
        Box::pin(async move {
            let params =
                [("q", query.words.as_str()), ("format", "json"), ("language", &query.language), ("pageno", "1")];
            let request = self.http.get(address(&format!("{}/search", self.base), &params)?);
            let body = checked(send(request).await?, SearchError::Blocked)?;
            Ok(parse(&body)?.into_iter().take(query.count).collect())
        })
    }
}

#[derive(Deserialize)]
struct Answer {
    results: Vec<Found>,
}

#[derive(Deserialize)]
struct Found {
    #[serde(default)]
    title: String,
    url: String,
    /// Plain text; missing for some engines.
    #[serde(default)]
    content: Option<String>,
}

/// The hits of a SearXNG answer (`results[]`: `title`, `url`, `content`), in its order (SearXNG ranks across its
/// engines); results without a web address are left out. `Err` when `body` is no SearXNG answer (an HTML page:
/// JSON is off).
pub fn parse(body: &[u8]) -> Result<Vec<SearchHit>, SearchError> {
    let answer: Answer = serde_json::from_slice(body).map_err(|_| SearchError::Failed(BROKEN))?;
    let found = |r: Found| hit(SearchProviderId::Searxng, &r.title, &r.url, r.content.as_deref().unwrap_or_default());
    Ok(answer.results.into_iter().filter_map(found).collect())
}

#[cfg(test)]
mod tests {
    use httpmock::Method::GET;
    use httpmock::MockServer;

    use super::*;
    use crate::tools::web::providers::testing::{fixture, http};

    #[test]
    fn reads_a_searxng_answer() {
        let hits = parse(&fixture("searxng.json")).expect("hits");
        let shown: Vec<(&str, &str, &str)> =
            hits.iter().map(|h| (h.title.as_str(), h.url.as_str(), h.snippet.as_str())).collect();
        assert_eq!(
            shown,
            [
                (
                    "Lisbon - Wikipedia",
                    "https://en.wikipedia.org/wiki/Lisbon",
                    "Lisbon is the capital and largest city of Portugal."
                ),
                ("Lisbon travel guide", "https://www.example.org/lisbon", ""),
            ],
            "a result without content keeps an empty snippet, one without a web address is left out"
        );
        assert_eq!(parse(b"<!DOCTYPE html><html></html>"), Err(SearchError::Failed(BROKEN)));
    }

    #[tokio::test]
    async fn asks_for_json_under_the_address_and_tells_a_refusal() {
        let server = MockServer::start_async().await;
        let found = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/searx/search")
                    .query_param("q", "Lisbon")
                    .query_param("format", "json")
                    .query_param("language", "de");
                then.status(200).body(fixture("searxng.json"));
            })
            .await;
        let query = Query::new("Lisbon", Some(1), Some("de"));
        let own = Searxng::new(http(), &format!("{}/searx/", server.base_url()));
        assert_eq!(own.search(&query).await.expect("hits").len(), 1, "at most the count");
        found.assert_async().await;
        server
            .mock_async(|when, then| {
                when.path("/off/search");
                then.status(403).body("<html>Forbidden</html>");
            })
            .await;
        let off = Searxng::new(http(), &format!("{}/off", server.base_url()));
        assert_eq!(off.search(&query).await, Err(SearchError::Blocked), "JSON switched off");
    }
}
