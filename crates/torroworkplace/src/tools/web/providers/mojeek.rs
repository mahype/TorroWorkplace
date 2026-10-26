//! Mojeek search API (`GET https://api.mojeek.com/search?q=…&api_key=…&fmt=json&t=…&lb=…&lbb=100`), as documented
//! at <https://www.mojeek.com/support/api/search/request_parameters.html> and the JSON format at
//! `…/json_response.html` (checked 2026-10-24). The key travels in the address: no error and no log may keep the
//! address (`super::send` drops it). Mojeek answers errors inside the JSON (`response.status` "ERROR: …", e.g.
//! "ERROR: Daily Limit Reached"), besides HTTP 401, 403 and 429.

use serde::Deserialize;
use torro_core::ai::BoxFuture;
use torro_ipc::web::{SearchHit, SearchProviderId};

use super::{address, checked, hit, send};
use crate::tools::web::{BROKEN, Capabilities, Query, SearchError, SearchProvider};

/// Mojeek's API.
pub const BASE: &str = "https://api.mojeek.com";

/// The most results the router ever wants of one request (`t`).
const MAX_HITS: usize = 20;

/// The Mojeek search API with the person's key. No `Debug`: it holds the key.
pub struct Mojeek {
    http: reqwest::Client,
    base: String,
    key: Option<String>,
}

impl Mojeek {
    /// Mojeek at its own address with `key` (`None`: none stored, the search answers [`SearchError::NeedsKey`]).
    pub fn new(http: reqwest::Client, key: Option<String>) -> Mojeek {
        Mojeek::at(http, BASE, key)
    }

    /// Mojeek at `base` (a fake server in tests) with `key`.
    pub fn at(http: reqwest::Client, base: &str, key: Option<String>) -> Mojeek {
        Mojeek { http, base: base.trim_end_matches('/').to_string(), key }
    }
}

impl SearchProvider for Mojeek {
    fn id(&self) -> SearchProviderId {
        SearchProviderId::Mojeek
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { max_hits: MAX_HITS, languages: true, cacheable: true }
    }

    fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>> {
        Box::pin(async move {
            let key = self.key.as_deref().map(str::trim).filter(|key| !key.is_empty()).ok_or(SearchError::NeedsKey)?;
            let count = query.count.min(MAX_HITS).to_string();
            // `lb` prefers pages in the language, `lbb=100` as strongly as Mojeek's documentation recommends.
            let language = query.language.to_ascii_uppercase();
            let params = [
                ("q", query.words.as_str()),
                ("api_key", key),
                ("fmt", "json"),
                ("t", &count),
                ("lb", &language),
                ("lbb", "100"),
            ];
            let request = self.http.get(address(&format!("{}/search", self.base), &params)?);
            let body = checked(send(request).await?, SearchError::NeedsKey)?;
            Ok(parse(&body)?.into_iter().take(query.count).collect())
        })
    }
}

#[derive(Deserialize)]
struct Answer {
    response: Response,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    status: String,
    #[serde(default)]
    results: Vec<Found>,
}

#[derive(Deserialize)]
struct Found {
    #[serde(default)]
    title: String,
    url: String,
    #[serde(default)]
    desc: String,
}

/// The hits of a Mojeek answer (`response.results[]`: `title`, `url`, `desc`), in its order; results without a web
/// address are left out. A `response.status` other than "OK" is an error: one naming a limit
/// [`SearchError::RateLimited`], one naming the key [`SearchError::NeedsKey`], else [`BROKEN`]. `Err` too when
/// `body` is no Mojeek answer.
pub fn parse(body: &[u8]) -> Result<Vec<SearchHit>, SearchError> {
    let answer: Answer = serde_json::from_slice(body).map_err(|_| SearchError::Failed(BROKEN))?;
    let status = answer.response.status.trim().to_ascii_lowercase();
    if status != "ok" {
        return Err(if status.contains("limit") {
            SearchError::RateLimited { retry_after: None }
        } else if status.contains("key") {
            SearchError::NeedsKey
        } else {
            SearchError::Failed(BROKEN)
        });
    }
    Ok(answer
        .response
        .results
        .into_iter()
        .filter_map(|r| hit(SearchProviderId::Mojeek, &r.title, &r.url, &r.desc))
        .collect())
}

#[cfg(test)]
mod tests {
    use httpmock::Method::GET;
    use httpmock::MockServer;

    use super::*;
    use crate::tools::web::providers::testing::{fixture, http};

    #[test]
    fn reads_a_mojeek_answer_and_its_errors() {
        let hits = parse(&fixture("mojeek.json")).expect("hits");
        let shown: Vec<(&str, &str)> = hits.iter().map(|h| (h.title.as_str(), h.url.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("Lisbon - Wikipedia", "https://en.wikipedia.org/wiki/Lisbon"),
                ("Lisbon travel - Lonely Planet", "https://www.lonelyplanet.com/portugal/lisbon"),
            ]
        );
        assert_eq!(hits[1].snippet, "Lisbon's appeal is as much about its past as its present.");
        assert_eq!(parse(&fixture("mojeek-limit.json")), Err(SearchError::RateLimited { retry_after: None }));
        assert_eq!(parse(br#"{"response":{"status":"ERROR: Invalid API key"}}"#), Err(SearchError::NeedsKey));
        assert_eq!(parse(br#"{"response":{"status":"ERROR: Something"}}"#), Err(SearchError::Failed(BROKEN)));
    }

    #[tokio::test]
    async fn sends_the_key_in_the_address_and_keeps_it_out_of_errors() {
        let server = MockServer::start_async().await;
        let found = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/search")
                    .query_param("q", "Lisbon")
                    .query_param("api_key", "secret-key")
                    .query_param("fmt", "json")
                    .query_param("t", "5")
                    .query_param("lb", "DE");
                then.status(200).body(fixture("mojeek.json"));
            })
            .await;
        let query = Query::new("Lisbon", None, Some("de"));
        let mojeek = Mojeek::at(http(), &server.base_url(), Some("secret-key".into()));
        assert_eq!(mojeek.search(&query).await.expect("hits").len(), 2);
        found.assert_async().await;
        let offline = Mojeek::at(http(), "http://127.0.0.1:9", Some("secret-key".into())).search(&query).await;
        assert!(!format!("{offline:?}").contains("secret-key"), "the error keeps no address: {offline:?}");
        assert!(matches!(offline, Err(SearchError::Failed(_))));
    }
}
