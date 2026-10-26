//! Exa search API (`POST https://api.exa.ai/search`, key in `x-api-key`, JSON body), as documented at
//! <https://exa.ai/docs/reference/search> (checked 2026-10-24): `numResults` 1 to 100, `type` `auto`, short
//! highlights as the extract (`contents.highlights.maxCharacters`). 401 and 403 refuse the key, 402 (no credits
//! left) and 429 are limits.

use serde::Deserialize;
use torro_core::ai::BoxFuture;
use torro_ipc::web::{SearchHit, SearchProviderId};

use super::{MAX_SNIPPET_CHARS, checked, hit, key_header, send};
use crate::tools::web::{BROKEN, Capabilities, Query, SearchError, SearchProvider};

/// Exa's API.
pub const BASE: &str = "https://api.exa.ai";

/// The most results the router ever wants; Exa gives up to 100.
const MAX_HITS: usize = 100;

/// The Exa search API with the person's key. No `Debug`: it holds the key.
pub struct Exa {
    http: reqwest::Client,
    base: String,
    key: Option<String>,
}

impl Exa {
    /// Exa at its own address with `key` (`None`: none stored, the search answers [`SearchError::NeedsKey`]).
    pub fn new(http: reqwest::Client, key: Option<String>) -> Exa {
        Exa::at(http, BASE, key)
    }

    /// Exa at `base` (a fake server in tests) with `key`.
    pub fn at(http: reqwest::Client, base: &str, key: Option<String>) -> Exa {
        Exa { http, base: base.trim_end_matches('/').to_string(), key }
    }
}

impl SearchProvider for Exa {
    fn id(&self) -> SearchProviderId {
        SearchProviderId::Exa
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { max_hits: MAX_HITS, languages: false, cacheable: true }
    }

    fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>> {
        Box::pin(async move {
            let key = self.key.as_deref().and_then(key_header).ok_or(SearchError::NeedsKey)?;
            let body = serde_json::json!({
                "query": query.words,
                "numResults": query.count.min(MAX_HITS),
                "type": "auto",
                "contents": { "highlights": { "maxCharacters": MAX_SNIPPET_CHARS } },
            });
            let request = self.http.post(format!("{}/search", self.base)).header("x-api-key", key).json(&body);
            let answer = send(request).await?;
            if answer.status == reqwest::StatusCode::PAYMENT_REQUIRED {
                return Err(SearchError::RateLimited { retry_after: None });
            }
            let body = checked(answer, SearchError::NeedsKey)?;
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
    title: Option<String>,
    url: String,
    #[serde(default)]
    highlights: Vec<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

/// The hits of an Exa answer (`results[]`: `title`, `url` and as the extract the `highlights` joined with " … ",
/// else `summary`, else `text`), in its order; results without a web address are left out. `Err` when `body` is
/// no Exa answer.
pub fn parse(body: &[u8]) -> Result<Vec<SearchHit>, SearchError> {
    let answer: Answer = serde_json::from_slice(body).map_err(|_| SearchError::Failed(BROKEN))?;
    let found = |r: Found| {
        let snippet =
            if r.highlights.is_empty() { r.summary.or(r.text).unwrap_or_default() } else { r.highlights.join(" … ") };
        hit(SearchProviderId::Exa, r.title.as_deref().unwrap_or_default(), &r.url, &snippet)
    };
    Ok(answer.results.into_iter().filter_map(found).collect())
}

#[cfg(test)]
mod tests {
    use httpmock::Method::POST;
    use httpmock::MockServer;

    use super::*;
    use crate::tools::web::providers::testing::{fixture, http};

    #[test]
    fn reads_an_exa_answer() {
        let hits = parse(&fixture("exa.json")).expect("hits");
        let shown: Vec<(&str, &str)> = hits.iter().map(|h| (h.title.as_str(), h.snippet.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("Lisbon travel guide", "Trams climb the hills of Alfama. … Tram 28 is the best known line."),
                ("https://example.org/lisbon-notes", "Notes on Lisbon without a title."),
            ],
            "highlights first, else the summary; a blank title becomes the address"
        );
        assert_eq!(parse(b"{}"), Err(SearchError::Failed(BROKEN)), "no results at all");
        assert_eq!(parse(b"nope"), Err(SearchError::Failed(BROKEN)));
    }

    #[tokio::test]
    async fn sends_the_key_as_header_and_tells_spent_credits() {
        let server = MockServer::start_async().await;
        let found = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/search")
                    .header("x-api-key", "good")
                    .json_body_includes(r#"{"query": "Lisbon", "numResults": 4, "type": "auto"}"#);
                then.status(200).body(fixture("exa.json"));
            })
            .await;
        let query = Query::new("Lisbon", Some(4), None);
        assert_eq!(
            Exa::at(http(), &server.base_url(), Some("good".into())).search(&query).await.expect("hits").len(),
            2
        );
        found.assert_async().await;
        server
            .mock_async(|when, then| {
                when.header("x-api-key", "spent");
                then.status(402);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.header("x-api-key", "bad");
                then.status(403);
            })
            .await;
        let spent = Exa::at(http(), &server.base_url(), Some("spent".into()));
        assert_eq!(spent.search(&query).await, Err(SearchError::RateLimited { retry_after: None }));
        let bad = Exa::at(http(), &server.base_url(), Some("bad".into()));
        assert_eq!(bad.search(&query).await, Err(SearchError::NeedsKey));
    }
}
