//! Tavily search API (`POST https://api.tavily.com/search`, `Authorization: Bearer <key>`, JSON body), as
//! documented at <https://docs.tavily.com/documentation/api-reference/endpoint/search> (checked 2026-10-24):
//! `max_results` 0 to 20, `search_depth` `basic` (one credit). 401 is a refused key, 429 a limit (with
//! `Retry-After`); 432 and 433 say a plan's or a pay-as-you-go limit is used up and count as a limit too.

use serde::Deserialize;
use torro_core::ai::BoxFuture;
use torro_ipc::web::{SearchHit, SearchProviderId};

use super::{checked, hit, key_header, send};
use crate::tools::web::{BROKEN, Capabilities, Query, SearchError, SearchProvider};

/// Tavily's API.
pub const BASE: &str = "https://api.tavily.com";

/// Tavily's most results per request (`max_results`).
const MAX_HITS: usize = 20;

/// The Tavily search API with the person's key. No `Debug`: it holds the key.
pub struct Tavily {
    http: reqwest::Client,
    base: String,
    key: Option<String>,
}

impl Tavily {
    /// Tavily at its own address with `key` (`None`: none stored, the search answers [`SearchError::NeedsKey`]).
    pub fn new(http: reqwest::Client, key: Option<String>) -> Tavily {
        Tavily::at(http, BASE, key)
    }

    /// Tavily at `base` (a fake server in tests) with `key`.
    pub fn at(http: reqwest::Client, base: &str, key: Option<String>) -> Tavily {
        Tavily { http, base: base.trim_end_matches('/').to_string(), key }
    }
}

impl SearchProvider for Tavily {
    fn id(&self) -> SearchProviderId {
        SearchProviderId::Tavily
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { max_hits: MAX_HITS, languages: false, cacheable: true }
    }

    fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>> {
        Box::pin(async move {
            let key = self.key.as_deref().ok_or(SearchError::NeedsKey)?;
            let auth = key_header(&format!("Bearer {}", key.trim())).ok_or(SearchError::NeedsKey)?;
            let body = serde_json::json!({
                "query": query.words,
                "max_results": query.count.min(MAX_HITS),
                "search_depth": "basic",
            });
            let request = self
                .http
                .post(format!("{}/search", self.base))
                .header(reqwest::header::AUTHORIZATION, auth)
                .json(&body);
            let answer = send(request).await?;
            if matches!(answer.status.as_u16(), 432 | 433) {
                return Err(SearchError::RateLimited { retry_after: None });
            }
            let body = checked(answer, SearchError::NeedsKey)?;
            Ok(parse(&body)?.into_iter().take(query.count).collect())
        })
    }
}

#[derive(Deserialize)]
struct Results {
    results: Vec<Found>,
}

#[derive(Deserialize)]
struct Found {
    #[serde(default)]
    title: String,
    url: String,
    /// Plain text, the part of the page that fits the query.
    #[serde(default)]
    content: String,
}

/// The hits of a Tavily answer (`results[]`: `title`, `url`, `content`), in its order; results without a web
/// address are left out. `Err` when `body` is no Tavily answer.
pub fn parse(body: &[u8]) -> Result<Vec<SearchHit>, SearchError> {
    let answer: Results = serde_json::from_slice(body).map_err(|_| SearchError::Failed(BROKEN))?;
    Ok(answer.results.into_iter().filter_map(|r| hit(SearchProviderId::Tavily, &r.title, &r.url, &r.content)).collect())
}

#[cfg(test)]
mod tests {
    use httpmock::Method::POST;
    use httpmock::MockServer;

    use super::*;
    use crate::tools::web::providers::testing::{fixture, http};

    #[test]
    fn reads_a_tavily_answer() {
        let hits = parse(&fixture("tavily.json")).expect("hits");
        let shown: Vec<(&str, &str)> = hits.iter().map(|h| (h.title.as_str(), h.url.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("Lisbon | History, Culture, Geography, & Map | Britannica", "https://www.britannica.com/place/Lisbon"),
                ("Lisbon - Wikipedia", "https://en.wikipedia.org/wiki/Lisbon"),
            ]
        );
        assert!(hits[0].snippet.starts_with("Lisbon, city, port, capital of Portugal"));
        assert!(hits.iter().all(|h| h.source == SearchProviderId::Tavily));
        assert_eq!(parse(b"[]"), Err(SearchError::Failed(BROKEN)));
    }

    #[tokio::test]
    async fn sends_a_bearer_key_and_tells_limits() {
        let server = MockServer::start_async().await;
        let found = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/search")
                    .header("authorization", "Bearer tvly-good")
                    .json_body(serde_json::json!({"query": "Lisbon", "max_results": 2, "search_depth": "basic"}));
                then.status(200).body(fixture("tavily.json"));
            })
            .await;
        let query = Query::new("Lisbon", Some(2), None);
        assert_eq!(
            Tavily::at(http(), &server.base_url(), Some("tvly-good".into())).search(&query).await.expect("hits").len(),
            2
        );
        found.assert_async().await;

        server
            .mock_async(|when, then| {
                when.header("authorization", "Bearer tvly-used-up");
                then.status(432);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.header("authorization", "Bearer tvly-busy");
                then.status(429).header("retry-after", "30");
            })
            .await;
        let used_up = Tavily::at(http(), &server.base_url(), Some("tvly-used-up".into()));
        assert_eq!(used_up.search(&query).await, Err(SearchError::RateLimited { retry_after: None }));
        let busy = Tavily::at(http(), &server.base_url(), Some("tvly-busy".into()));
        let retry_after = Some(std::time::Duration::from_secs(30));
        assert_eq!(busy.search(&query).await, Err(SearchError::RateLimited { retry_after }));
        assert_eq!(Tavily::at(http(), "http://127.0.0.1:9", None).search(&query).await, Err(SearchError::NeedsKey));
    }
}
