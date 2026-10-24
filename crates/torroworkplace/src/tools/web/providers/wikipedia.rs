//! Wikipedia's REST search (`GET https://<language>.wikipedia.org/w/rest.php/v1/search/page?q=…&limit=…`), as
//! documented at <https://www.mediawiki.org/wiki/API:REST_API/Reference> ("Search pages", checked 2026-10-24):
//! `limit` 1 to 100, `pages[]` with `key`, `title` and an `excerpt` in HTML (`<span class="searchmatch">`). Its text
//! stands under CC BY-SA 4.0, named at every hit. Wikimedia asks for a user agent with a contact: the suite's
//! (`places::USER_AGENT`). The router asks it besides the others, never instead of them.

use serde::Deserialize;
use torro_core::ai::BoxFuture;
use torro_ipc::web::{SearchHit, SearchProviderId};

use super::{address, checked, hit, plain_text, send};
use crate::tools::web::{BROKEN, Capabilities, Query, SearchError, SearchProvider};

/// The licence of Wikipedia's text.
pub const LICENSE: &str = "CC BY-SA 4.0";

/// The most pages one search asks for (the REST search takes up to 100).
const MAX_HITS: usize = 100;

/// Wikipedia in the language of each query.
#[derive(Debug)]
pub struct Wikipedia {
    http: reqwest::Client,
    /// `None`: `https://<language>.wikipedia.org`; tests give a fake server.
    base: Option<String>,
}

impl Wikipedia {
    /// Wikipedia in the language of each [`Query`] (`de.wikipedia.org`, `en.wikipedia.org`).
    pub fn new(http: reqwest::Client) -> Wikipedia {
        Wikipedia { http, base: None }
    }

    /// Wikipedia's search at `base` for every language (a fake server in tests); the hits still point at
    /// `<language>.wikipedia.org`.
    pub fn at(http: reqwest::Client, base: &str) -> Wikipedia {
        Wikipedia { http, base: Some(base.trim_end_matches('/').to_string()) }
    }
}

/// The site of Wikipedia in `language` (two lower-case letters, [`Query::language`]).
fn site(language: &str) -> String {
    format!("https://{language}.wikipedia.org")
}

impl SearchProvider for Wikipedia {
    fn id(&self) -> SearchProviderId {
        SearchProviderId::Wikipedia
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { max_hits: MAX_HITS, languages: true, cacheable: true }
    }

    fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>> {
        Box::pin(async move {
            let base = self.base.clone().unwrap_or_else(|| site(&query.language));
            let limit = query.count.min(MAX_HITS).to_string();
            let url = address(&format!("{base}/w/rest.php/v1/search/page"), &[("q", &query.words), ("limit", &limit)])?;
            let request = self.http.get(url);
            let body = checked(send(request).await?, SearchError::Blocked)?;
            Ok(parse(&body, &query.language)?.into_iter().take(query.count).collect())
        })
    }
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    pages: Vec<Page>,
}

#[derive(Deserialize)]
struct Page {
    key: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    excerpt: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// The hits of a search of Wikipedia in `language` (`pages[]`): the title, the article
/// `https://<language>.wikipedia.org/wiki/<key>` and the excerpt without its HTML (else the short description), each
/// with the licence [`LICENSE`]. Unlike the other providers' `parse` it takes the language: the answer names only
/// the article's key. `Err` when `body` is no answer of the REST search.
pub fn parse(body: &[u8], language: &str) -> Result<Vec<SearchHit>, SearchError> {
    let answer: Answer = serde_json::from_slice(body).map_err(|_| SearchError::Failed(BROKEN))?;
    let base = reqwest::Url::parse(&site(language)).map_err(|_| SearchError::Failed(BROKEN))?;
    let found = |page: Page| {
        let mut url = base.clone();
        url.path_segments_mut().ok()?.push("wiki").push(&page.key);
        let snippet = match page.excerpt.as_deref().map(plain_text).filter(|text| !text.trim().is_empty()) {
            Some(text) => text,
            None => page.description.unwrap_or_default(),
        };
        let mut found = hit(SearchProviderId::Wikipedia, &page.title, url.as_str(), &snippet)?;
        found.license = Some(LICENSE.to_string());
        Some(found)
    };
    Ok(answer.pages.into_iter().filter_map(found).collect())
}

#[cfg(test)]
mod tests {
    use httpmock::Method::GET;
    use httpmock::MockServer;

    use super::*;
    use crate::tools::web::providers::testing::{fixture, http};

    #[test]
    fn reads_a_wikipedia_answer_with_its_licence() {
        let hits = parse(&fixture("wikipedia.json"), "en").expect("hits");
        let shown: Vec<(&str, &str)> = hits.iter().map(|h| (h.title.as_str(), h.url.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("Lisbon", "https://en.wikipedia.org/wiki/Lisbon"),
                ("Teresa Lisbon", "https://en.wikipedia.org/wiki/Teresa_Lisbon"),
                ("Lisbon (disambiguation)", "https://en.wikipedia.org/wiki/Lisbon_(disambiguation)"),
            ]
        );
        assert!(hits[0].snippet.starts_with("Lisbon (/ˈlɪzbən/ LIZ-bən; Portuguese: Lisboa"), "{}", hits[0].snippet);
        assert!(hits.iter().all(|h| !h.snippet.contains('<')), "no HTML left");
        assert!(hits.iter().all(|h| h.license.as_deref() == Some(LICENSE) && h.source == SearchProviderId::Wikipedia));
        let german = r#"{"pages":[{"id":1,"key":"Köln","title":"Köln","excerpt":null,"description":"Stadt"}]}"#;
        let german = parse(german.as_bytes(), "de").expect("hits");
        assert_eq!(
            (german[0].url.as_str(), german[0].snippet.as_str()),
            ("https://de.wikipedia.org/wiki/K%C3%B6ln", "Stadt")
        );
        assert_eq!(parse(b"{\"pages\": 3}", "en"), Err(SearchError::Failed(BROKEN)));
    }

    #[tokio::test]
    async fn asks_the_rest_search_with_a_limit() {
        let server = MockServer::start_async().await;
        let found = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/w/rest.php/v1/search/page")
                    .query_param("q", "Lisbon")
                    .query_param("limit", "3");
                then.status(200).body(fixture("wikipedia.json"));
            })
            .await;
        let wikipedia = Wikipedia::at(http(), &server.base_url());
        let hits = wikipedia.search(&Query::new("Lisbon", Some(3), Some("en"))).await.expect("hits");
        assert_eq!(hits.len(), 3);
        found.assert_async().await;
    }
}
