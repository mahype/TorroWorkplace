//! Brave Search API (`GET https://api.search.brave.com/res/v1/web/search`, key in `X-Subscription-Token`), as
//! documented at <https://api-dashboard.search.brave.com/app/documentation/web-search/get-started> (checked
//! 2026-10-24). Its terms allow results only for the answer, not to store them: [`Capabilities::cacheable`] is
//! `false` and the router never keeps them.

use serde::Deserialize;
use torro_core::ai::BoxFuture;
use torro_ipc::web::{SearchHit, SearchProviderId};

use super::{address, checked, hit, key_header, plain_text, send};
use crate::tools::web::{BROKEN, Capabilities, Query, SearchError, SearchProvider};

/// Brave's API.
pub const BASE: &str = "https://api.search.brave.com";

/// Brave's most results per request (`count`).
const MAX_HITS: usize = 20;

/// The Brave Search API with the person's key. No `Debug`: it holds the key.
pub struct Brave {
    http: reqwest::Client,
    base: String,
    key: Option<String>,
}

impl Brave {
    /// Brave at its own address with `key` (`None`: none stored, the search answers [`SearchError::NeedsKey`]).
    pub fn new(http: reqwest::Client, key: Option<String>) -> Brave {
        Brave::at(http, BASE, key)
    }

    /// Brave at `base` (a fake server in tests) with `key`.
    pub fn at(http: reqwest::Client, base: &str, key: Option<String>) -> Brave {
        Brave { http, base: base.trim_end_matches('/').to_string(), key }
    }
}

impl SearchProvider for Brave {
    fn id(&self) -> SearchProviderId {
        SearchProviderId::Brave
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { max_hits: MAX_HITS, languages: true, cacheable: false }
    }

    fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>> {
        Box::pin(async move {
            let key = self.key.as_deref().and_then(key_header).ok_or(SearchError::NeedsKey)?;
            let count = query.count.min(MAX_HITS).to_string();
            let params = [("q", query.words.as_str()), ("count", &count), ("search_lang", &query.language)];
            let request = self
                .http
                .get(address(&format!("{}/res/v1/web/search", self.base), &params)?)
                .header(reqwest::header::ACCEPT, "application/json")
                .header("X-Subscription-Token", key);
            let body = checked(send(request).await?, SearchError::NeedsKey)?;
            Ok(parse(&body)?.into_iter().take(query.count).collect())
        })
    }
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    web: Option<Web>,
}

#[derive(Deserialize)]
struct Web {
    #[serde(default)]
    results: Vec<Found>,
}

#[derive(Deserialize)]
struct Found {
    #[serde(default)]
    title: String,
    url: String,
    /// With `<strong>` around the words found.
    #[serde(default)]
    description: String,
}

/// The hits of a Brave answer (`web.results[]`: `title`, `url`, `description` with HTML), in its order; results
/// without a web address are left out, an answer without `web` has none. `Err` when `body` is no Brave answer.
pub fn parse(body: &[u8]) -> Result<Vec<SearchHit>, SearchError> {
    let answer: Answer = serde_json::from_slice(body).map_err(|_| SearchError::Failed(BROKEN))?;
    let results = answer.web.map(|web| web.results).unwrap_or_default();
    Ok(results
        .into_iter()
        .filter_map(|r| hit(SearchProviderId::Brave, &r.title, &r.url, &plain_text(&r.description)))
        .collect())
}

#[cfg(test)]
mod tests {
    use httpmock::Method::GET;
    use httpmock::MockServer;

    use super::*;
    use crate::tools::web::providers::testing::{fixture, http};

    #[test]
    fn reads_a_brave_answer() {
        let hits = parse(&fixture("brave.json")).expect("hits");
        let shown: Vec<(&str, &str)> = hits.iter().map(|h| (h.title.as_str(), h.url.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("Lisbon - Wikipedia", "https://en.wikipedia.org/wiki/Lisbon"),
                ("Visit Lisboa | Official Tourism Website", "https://www.visitlisboa.com/en"),
            ],
            "a result without a web address is left out"
        );
        assert_eq!(hits[0].snippet, "Lisbon is the capital & largest city of Portugal.");
        assert!(hits.iter().all(|h| h.source == SearchProviderId::Brave && h.license.is_none()));
        assert_eq!(parse(br#"{"type":"search"}"#), Ok(Vec::new()), "no web results");
        assert_eq!(parse(b"<html>"), Err(SearchError::Failed(BROKEN)));
    }

    #[tokio::test]
    async fn sends_the_key_as_header_and_tells_a_refused_key() {
        let server = MockServer::start_async().await;
        let found = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/res/v1/web/search")
                    .query_param("q", "Lisbon")
                    .query_param("count", "3")
                    .query_param("search_lang", "en")
                    .header("x-subscription-token", "good");
                then.status(200).body(fixture("brave.json"));
            })
            .await;
        let query = Query::new("Lisbon", Some(3), Some("en"));
        let brave = Brave::at(http(), &server.base_url(), Some("good".into()));
        assert_eq!(brave.search(&query).await.expect("hits").len(), 2);
        found.assert_async().await;

        let refused = Brave::at(http(), &server.base_url(), Some("bad".into()));
        server
            .mock_async(|when, then| {
                when.header("x-subscription-token", "bad");
                then.status(401);
            })
            .await;
        assert_eq!(refused.search(&query).await, Err(SearchError::NeedsKey));
        // Port 9 (discard) answers nobody: a request would fail as offline.
        let keyless = Brave::at(http(), "http://127.0.0.1:9", None);
        assert_eq!(keyless.search(&query).await, Err(SearchError::NeedsKey), "nothing sent without a key");
    }
}
