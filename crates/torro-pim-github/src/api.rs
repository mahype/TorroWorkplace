//! Asking GitHub: one GraphQL endpoint, the account's token in the `Authorization` header only.
//!
//! A filter list is fetched whole every time ([`Api::fetch`]): each search page by page up to
//! the 1,000 matches GitHub hands out, then the pinned items. Fetching only what changed would
//! miss an item that stopped matching (`docs/github.md`, "Technik").

use std::future::Future;

use serde_json::Value;
use torro_core::account::Secret;

use crate::Error;
use crate::address::IssueRef;
use crate::graphql::{self, Page};
use crate::item::{self, Item};

/// The most matches GitHub hands out for one search.
pub const MAX_MATCHES: usize = 1000;

/// How many items one search page asks for; GitHub allows 100.
const PAGE_SIZE: usize = 100;

/// How many pinned items one call asks for, so a document stays well inside GitHub's limits.
const PINNED_PER_CALL: usize = 50;

/// What it takes to ask GitHub as one account: an HTTP client, the GraphQL endpoint
/// (`https://api.github.com/graphql`, a fake server in tests) and the account's token.
///
/// `Debug` shows no token ([`Secret`]).
#[derive(Debug, Clone)]
pub struct Api {
    http: reqwest::Client,
    url: String,
    token: Secret,
}

/// The matches of one search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    /// How many items match in all, as GitHub counts them; may be more than `items` holds.
    pub total: u64,
    /// The first matches, in the search's order.
    pub items: Vec<Item>,
}

/// A filter list as fetched ([`Api::fetch`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// Every match of every search, then the pinned items no search found; each item once.
    pub items: Vec<Item>,
    /// A search had more than [`MAX_MATCHES`] matches; only those are in `items`.
    pub truncated: bool,
}

impl Api {
    pub fn new(http: reqwest::Client, url: impl Into<String>, token: Secret) -> Api {
        Api { http, url: url.into(), token }
    }

    /// The first `max` matches of `query` (at most [`MAX_MATCHES`]), page by page.
    pub async fn search(&self, query: &str, max: usize) -> Result<Search, Error> {
        paginate(|body| self.post(body), query, max).await
    }

    /// The items `refs`, those that still exist ([`graphql::read_items`]), in order.
    pub async fn items(&self, refs: &[IssueRef]) -> Result<Vec<Item>, Error> {
        let mut items = Vec::new();
        for chunk in refs.chunks(PINNED_PER_CALL) {
            let answer = self.post(graphql::items_body(chunk)).await?;
            items.extend(graphql::read_items(&answer, chunk.len())?);
        }
        Ok(items)
    }

    /// The whole list: every match of each of `queries` (up to [`MAX_MATCHES`] each) and the
    /// `pinned` items (link ids; ones that name no item are skipped), each item once
    /// ([`item::union`]). The first failure fails it all, so a list never loses items because one
    /// search failed.
    pub async fn fetch(&self, queries: &[String], pinned: &[String]) -> Result<Fetched, Error> {
        let mut parts = Vec::new();
        let mut truncated = false;
        for query in queries {
            let search = self.search(query, MAX_MATCHES).await?;
            truncated |= search.total > MAX_MATCHES as u64;
            parts.push(search.items);
        }
        let refs: Vec<IssueRef> = pinned.iter().filter_map(|id| crate::address::issue_ref(id)).collect();
        if !refs.is_empty() {
            parts.push(self.items(&refs).await?);
        }
        Ok(Fetched { items: item::union(parts), truncated })
    }

    /// Sends one GraphQL document and returns GitHub's answer as JSON.
    async fn post(&self, body: Value) -> Result<Value, Error> {
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(self.token.expose())
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(unreachable)?;
        match response.status().as_u16() {
            200..=299 => {}
            401 => return Err(Error::Refused),
            403 | 429 => {
                return Err(Error::Github(
                    "GitHub refused the request: the rate limit is reached, or the token needs single sign-on \
                     approval. The list is refreshed again later."
                        .into(),
                ));
            }
            status => return Err(Error::Github(format!("GitHub answered with HTTP {status}."))),
        }
        response.json().await.map_err(|_| Error::Github("GitHub sent an answer that is not JSON.".into()))
    }
}

fn unreachable(err: reqwest::Error) -> Error {
    if err.is_connect() || err.is_timeout() {
        Error::Offline
    } else {
        Error::Github(format!("GitHub could not be asked: {}", err.without_url()))
    }
}

/// The first `max` matches of `query` (never more than [`MAX_MATCHES`]), asking `post` for one
/// page after the other until GitHub has no more. Apart from [`Api::search`] so it can be tested
/// with recorded pages.
pub async fn paginate<Post, Fut>(mut post: Post, query: &str, max: usize) -> Result<Search, Error>
where
    Post: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<Value, Error>>,
{
    let max = max.min(MAX_MATCHES);
    let mut items = Vec::new();
    let mut total = 0;
    let mut after: Option<String> = None;
    loop {
        let first = (max - items.len()).min(PAGE_SIZE);
        if first == 0 {
            break;
        }
        let Page { total: count, items: page, next } =
            graphql::read_search(&post(graphql::search_body(query, first, after.as_deref())).await?)?;
        total = count;
        let empty = page.is_empty();
        items.extend(page);
        match next {
            Some(cursor) if !empty => after = Some(cursor),
            _ => break,
        }
    }
    items.truncate(max);
    Ok(Search { total, items })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::graphql::tests::fixture;

    #[tokio::test]
    async fn a_search_follows_the_cursor_until_the_last_page() {
        let asked = Arc::new(Mutex::new(Vec::<Value>::new()));
        let record = Arc::clone(&asked);
        let search = paginate(
            move |body: Value| {
                let page = if body["variables"]["after"].is_null() { "search_page1" } else { "search_page2" };
                record.lock().expect("record").push(body);
                async move { Ok(fixture(page)) }
            },
            "is:open",
            MAX_MATCHES,
        )
        .await
        .expect("both pages");
        assert_eq!(search.total, 3);
        let ids: Vec<String> = search.items.iter().map(|item| item.id.link_id()).collect();
        assert_eq!(
            ids,
            ["github:mahype/TorroWorkplace#123", "github:mahype/TorroWorkplace#45", "github:mahype/other#44"]
        );
        let asked = asked.lock().expect("record");
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0]["variables"]["first"], 100);
        assert_eq!(asked[1]["variables"]["after"], "Y3Vyc29yOjI=");
    }

    #[tokio::test]
    async fn a_search_asks_for_no_more_than_it_needs() {
        let search = paginate(
            |body: Value| async move {
                assert_eq!(body["variables"]["first"], 2, "a preview asks for its few items only");
                Ok(fixture("search_page1"))
            },
            "is:open",
            2,
        )
        .await
        .expect("one page");
        assert_eq!((search.total, search.items.len()), (3, 2));
    }

    #[tokio::test]
    async fn a_failed_page_fails_the_search() {
        let err = paginate(|_| async { Err(Error::Offline) }, "is:open", 10).await.expect_err("offline");
        assert_eq!(err, Error::Offline);
        let err = paginate(|_| async { Ok(fixture("search_error")) }, "is:open", 10).await.expect_err("error");
        assert_eq!(err.code(), "error");
    }

    #[test]
    fn the_token_does_not_show_in_debug_output() {
        let api = Api::new(reqwest::Client::new(), "http://127.0.0.1:9/graphql", Secret::new("ghp_SECRET"));
        assert!(!format!("{api:?}").contains("SECRET"));
    }
}
