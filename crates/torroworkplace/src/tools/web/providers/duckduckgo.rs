//! DuckDuckGo's results page without scripts (`POST https://html.duckduckgo.com/html/`, form `q` and `kl`). There is
//! no official interface: the page is read like a browser would show it, with the suite's own user agent, and
//! DuckDuckGo may block this at any time (the notice in [`crate::tools::web::catalogue`]). A block is never worked
//! around: a captcha or anomaly page, 202 and 403 are [`SearchError::Blocked`] and the router rests DuckDuckGo for an
//! hour; it asks at most once in ten seconds anyway.
//!
//! The page (recorded 2026-10-24, `tests/fixtures/web/duckduckgo.html`): every hit a `div.result` with the link
//! `a.result__a` and the extract `a.result__snippet`; ads are `div.result--ad`. Links point at the page itself or,
//! on older pages, at DuckDuckGo's redirect `//duckduckgo.com/l/?uddg=<address>`, which is resolved here.

use dom_query::Document;
use torro_core::ai::BoxFuture;
use torro_ipc::web::{SearchHit, SearchProviderId};

use super::{checked, hit, send};
use crate::tools::web::{BROKEN, Capabilities, Query, SearchError, SearchProvider};

/// DuckDuckGo's page without scripts.
pub const BASE: &str = "https://html.duckduckgo.com";

/// One page shows about this many hits; DuckDuckGo is asked for one page.
const MAX_HITS: usize = 10;

/// DuckDuckGo's results page.
#[derive(Debug)]
pub struct Duckduckgo {
    http: reqwest::Client,
    base: String,
}

impl Duckduckgo {
    /// DuckDuckGo at its own address.
    pub fn new(http: reqwest::Client) -> Duckduckgo {
        Duckduckgo::at(http, BASE)
    }

    /// DuckDuckGo at `base` (a fake server in tests).
    pub fn at(http: reqwest::Client, base: &str) -> Duckduckgo {
        Duckduckgo { http, base: base.trim_end_matches('/').to_string() }
    }
}

/// DuckDuckGo's region (`kl`) for a language: English US, German Germany, any other no region (`wt-wt`), so a
/// guessed region never narrows the hits.
fn region(language: &str) -> &'static str {
    match language {
        "en" => "us-en",
        "de" => "de-de",
        _ => "wt-wt",
    }
}

impl SearchProvider for Duckduckgo {
    fn id(&self) -> SearchProviderId {
        SearchProviderId::Duckduckgo
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { max_hits: MAX_HITS, languages: true, cacheable: true }
    }

    fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>> {
        Box::pin(async move {
            let form = [("q", query.words.as_str()), ("kl", region(&query.language))];
            let request = self.http.post(format!("{}/html/", self.base)).form(&form);
            let answer = send(request).await?;
            // DuckDuckGo answers a suspected bot with 202 and a challenge instead of hits.
            if answer.status == reqwest::StatusCode::ACCEPTED {
                return Err(SearchError::Blocked);
            }
            let body = checked(answer, SearchError::Blocked)?;
            Ok(parse(&body)?.into_iter().take(query.count).collect())
        })
    }
}

/// The hits of DuckDuckGo's results page, in its order, without ads and with redirects resolved. A captcha or
/// anomaly page is [`SearchError::Blocked`]; a page without its results list (`#links`) is [`BROKEN`] (the page
/// changed), one with an empty list has no hits.
pub fn parse(body: &[u8]) -> Result<Vec<SearchHit>, SearchError> {
    let html = String::from_utf8_lossy(body);
    let page = Document::from(html.as_ref());
    if page.select("[class*='anomaly-modal'], #challenge-form, form[action*='anomaly']").exists() {
        return Err(SearchError::Blocked);
    }
    if !page.select("#links").exists() {
        return Err(SearchError::Failed(BROKEN));
    }
    let hits = page
        .select("div.result")
        .iter()
        .filter(|result| !result.has_class("result--ad"))
        .filter_map(|result| {
            let link = result.select("a.result__a").first();
            let url = target(&link.attr("href")?)?;
            let snippet = result.select(".result__snippet").first().text();
            hit(SearchProviderId::Duckduckgo, &link.text(), &url, &snippet)
        })
        .collect();
    Ok(hits)
}

/// The address a result's `href` leads to: DuckDuckGo's redirect (`//duckduckgo.com/l/?uddg=…`) resolved to its
/// `uddg`, an ad's click address (`/y.js`) none, any other address itself.
fn target(href: &str) -> Option<String> {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://duckduckgo.com{href}")
    } else {
        href.to_string()
    };
    let url = reqwest::Url::parse(&absolute).ok()?;
    let ours = url.host_str().is_some_and(|host| host == "duckduckgo.com" || host.ends_with(".duckduckgo.com"));
    match (ours, url.path()) {
        (true, "/l/") => url.query_pairs().find(|(name, _)| name == "uddg").map(|(_, value)| value.into_owned()),
        (true, _) => None,
        (false, _) => Some(url.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use httpmock::Method::POST;
    use httpmock::MockServer;

    use super::*;
    use crate::tools::web::providers::testing::{fixture, http};

    #[test]
    fn reads_the_recorded_results_page() {
        let hits = parse(&fixture("duckduckgo.html")).expect("hits");
        assert_eq!(hits.len(), 10);
        assert_eq!(hits[0].title, "Lisbon - Wikipedia");
        assert_eq!(hits[0].url, "https://en.wikipedia.org/wiki/Lisbon");
        assert!(
            hits[0].snippet.starts_with("Lisbon is recognised as an alpha− level global city"),
            "{}",
            hits[0].snippet
        );
        assert!(hits.iter().all(|h| h.source == SearchProviderId::Duckduckgo && !h.snippet.contains('<')));
    }

    #[test]
    fn resolves_redirects_and_leaves_ads_out() {
        let hits = parse(&fixture("duckduckgo-redirect.html")).expect("hits");
        let urls: Vec<&str> = hits.iter().map(|h| h.url.as_str()).collect();
        assert_eq!(urls, ["https://www.visitlisboa.com/en/places/tram-28", "https://example.org/a?b=c&d=e"]);
        assert_eq!(hits[0].snippet, "The tram climbs through Alfama.");
    }

    #[test]
    fn a_captcha_page_is_a_block() {
        assert_eq!(parse(&fixture("duckduckgo-captcha.html")), Err(SearchError::Blocked));
        assert_eq!(parse(b"<html><body>Something else</body></html>"), Err(SearchError::Failed(BROKEN)));
        assert_eq!(
            parse(b"<html><body><div id=\"links\"><div class=\"no-results\"></div></div></body></html>"),
            Ok(Vec::new())
        );
    }

    #[tokio::test]
    async fn posts_the_form_and_tells_a_block() {
        let server = MockServer::start_async().await;
        let found = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/html/")
                    .form_urlencoded_tuple("q", "Lisbon")
                    .form_urlencoded_tuple("kl", "de-de");
                then.status(200).body(fixture("duckduckgo.html"));
            })
            .await;
        let query = Query::new("Lisbon", Some(3), Some("de"));
        let duckduckgo = Duckduckgo::at(http(), &server.base_url());
        assert_eq!(duckduckgo.search(&query).await.expect("hits").len(), 3, "at most the count");
        found.assert_async().await;

        let blocked = MockServer::start_async().await;
        blocked
            .mock_async(|when, then| {
                when.path("/html/");
                then.status(202).body(fixture("duckduckgo-captcha.html"));
            })
            .await;
        let duckduckgo = Duckduckgo::at(http(), &blocked.base_url());
        assert_eq!(duckduckgo.search(&query).await, Err(SearchError::Blocked));
    }
}
