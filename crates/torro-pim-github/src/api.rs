//! Asking GitHub: one GraphQL endpoint, the account's token in the `Authorization` header only.
//!
//! A filter list is fetched whole every time ([`Api::fetch`]): each search page by page up to
//! the 1,000 matches GitHub hands out, then the pinned items. Fetching only what changed would
//! miss an item that stopped matching (`docs/github.md`, "Technik").
//!
//! For the dialog of a filter list (task G.9) it also reads the repositories the account reaches
//! ([`Api::repos`]) and what can be picked in one of them ([`Api::describe_repo`]). For the inbox
//! (task E5.5) it reads how the issues and pull requests of its entries stand ([`Api::standing`]).

use std::future::Future;

use serde_json::Value;
use torro_core::account::Secret;
use torro_pim::source::{GitHubRepo, GitHubRepoDetails};

use crate::Error;
use crate::address::IssueRef;
use crate::graphql::{self, Page};
use crate::item::{self, Item};

/// The most matches GitHub hands out for one search.
pub const MAX_MATCHES: usize = 1000;

/// How many items one search page asks for; GitHub allows 100.
const PAGE_SIZE: usize = 100;

/// How many pinned items one call asks for, so a document stays well inside GitHub's limits; the
/// inbox's check of its entries ([`Api::standing`]) as well.
const PINNED_PER_CALL: usize = 50;

/// How many pages of 100 repositories [`Api::repos`] reads at most: enough for a person's own and
/// their organisations' repositories, without asking GitHub dozens of times for a picker.
pub const REPO_PAGES: usize = 10;

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

    /// How the items `refs` stand, for the inbox's GitHub entries (task E5.5): each item as
    /// [`Api::items`] reads it, with when it was closed and whether a pull request still asks the
    /// signed-in account for a review ([`graphql::Standing`]). One call per 50 refs, in order;
    /// items that are gone or out of the token's reach are left out ([`graphql::read_standing`]).
    /// Only reads: nothing changes on GitHub. The first failure fails it all.
    pub async fn standing(&self, refs: &[IssueRef]) -> Result<Vec<graphql::Standing>, Error> {
        let mut standing = Vec::new();
        for chunk in refs.chunks(PINNED_PER_CALL) {
            let answer = self.post(graphql::standing_body(chunk)).await?;
            standing.extend(graphql::read_standing(&answer, chunk)?);
        }
        Ok(standing)
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

    /// Closes the issue `issue` as completed (`done`) or reopens it, and returns it as GitHub
    /// reports it afterwards (new state, new `updated_at`), task G.5. Pull requests are never
    /// closed here ([`Error::Input`], before anything changes). [`Error::Forbidden`] when the token
    /// may not write issues there, [`Error::NotFound`] when GitHub has no such issue or the token
    /// does not reach it. See [`set_done`].
    pub async fn set_issue_done(&self, issue: &IssueRef, done: bool) -> Result<Item, Error> {
        set_done(|body| self.post(body), issue, done).await
    }

    /// The repositories the account reaches, archived ones left out, the most recently changed
    /// first: at most [`REPO_PAGES`] pages of 100 (task G.9). See [`list_repos`].
    pub async fn repos(&self) -> Result<Vec<GitHubRepo>, Error> {
        list_repos(|body| self.post(body)).await
    }

    /// Labels, open milestones and assignable people of the repository `repo` (`owner/name`) in
    /// one call (task G.9). [`Error::Input`] for a `repo` that is not `owner/name`, before GitHub
    /// is asked; [`Error::NotFound`] when GitHub has no such repository or the token does not
    /// reach it. See [`describe`].
    pub async fn describe_repo(&self, repo: &str) -> Result<GitHubRepoDetails, Error> {
        describe(|body| self.post(body), repo).await
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

/// Closes (`done`) or reopens the issue `issue` through `post`: first its node id
/// ([`graphql::issue_id_body`]), then the mutation ([`graphql::set_done_body`]); the answer is the
/// issue as it is afterwards. Nothing changes when the first call fails. Apart from
/// [`Api::set_issue_done`] so it can be tested with recorded answers.
pub async fn set_done<Post, Fut>(mut post: Post, issue: &IssueRef, done: bool) -> Result<Item, Error>
where
    Post: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<Value, Error>>,
{
    let id = graphql::read_issue_id(&post(graphql::issue_id_body(issue)).await?)?;
    graphql::read_set_done(&post(graphql::set_done_body(&id, done)).await?, done)
}

/// The repositories of the account, asking `post` for one page of 100 after the other until
/// GitHub has no more or [`REPO_PAGES`] pages were read. Archived ones are left out
/// ([`graphql::read_repos`]); the first failure fails it all. Apart from [`Api::repos`] so it can
/// be tested with recorded pages.
pub async fn list_repos<Post, Fut>(mut post: Post) -> Result<Vec<GitHubRepo>, Error>
where
    Post: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<Value, Error>>,
{
    let mut repos = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..REPO_PAGES {
        let page = graphql::read_repos(&post(graphql::repos_body(PAGE_SIZE, after.as_deref())).await?)?;
        repos.extend(page.repos);
        match page.next {
            Some(cursor) => after = Some(cursor),
            None => break,
        }
    }
    Ok(repos)
}

/// Labels, open milestones and assignable people of `repo` through `post`, in one call
/// ([`graphql::repo_body`]). A `repo` that is not `owner/name` ([`repo_parts`]) asks nothing.
/// Apart from [`Api::describe_repo`] so it can be tested with recorded answers.
pub async fn describe<Post, Fut>(mut post: Post, repo: &str) -> Result<GitHubRepoDetails, Error>
where
    Post: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<Value, Error>>,
{
    let (owner, name) = repo_parts(repo)?;
    graphql::read_repo(&post(graphql::repo_body(owner, name)).await?)
}

/// Owner and name of `repo` written as `owner/name` (spaces around it ignored): both parts made
/// of letters, digits, `-`, `_` and `.`, as GitHub names them. [`Error::Input`] otherwise; the
/// message does not quote what was sent.
pub fn repo_parts(repo: &str) -> Result<(&str, &str), Error> {
    let fits = |part: &str| {
        !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    match repo.trim().split_once('/') {
        Some((owner, name)) if fits(owner) && fits(name) => Ok((owner, name)),
        _ => Err(Error::Input("Write the repository as owner/name, for example mahype/TorroWorkplace.".into())),
    }
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

    /// `set_done` against recorded answers: the node id, then `answer` to the mutation. Returns
    /// the result and the documents sent.
    async fn closing(done: bool, answer: &'static str) -> (Result<Item, Error>, Vec<Value>) {
        let asked = Arc::new(Mutex::new(Vec::<Value>::new()));
        let record = Arc::clone(&asked);
        let issue = IssueRef { repo: "mahype/TorroWorkplace".into(), number: 123 };
        let result = set_done(
            move |body: Value| {
                let first = record.lock().expect("record").is_empty();
                record.lock().expect("record").push(body);
                async move { Ok(fixture(if first { "issue_id" } else { answer })) }
            },
            &issue,
            done,
        )
        .await;
        let asked = asked.lock().expect("record").clone();
        (result, asked)
    }

    #[tokio::test]
    async fn an_issue_is_closed_as_completed_and_reopened_by_its_node_id() {
        let (closed, asked) = closing(true, "close_issue").await;
        assert_eq!(closed.map(|item| item.state), Ok(torro_pim::model::GitHubState::Closed));
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0]["variables"]["n"], 123);
        assert_eq!(asked[1]["variables"]["id"], "I_kwDOMahype85Abc123");
        assert!(asked[1]["query"].as_str().is_some_and(|q| q.contains("stateReason: COMPLETED")));
        let (reopened, asked) = closing(false, "reopen_issue").await;
        assert_eq!(reopened.map(|item| item.state), Ok(torro_pim::model::GitHubState::Open));
        assert!(asked[1]["query"].as_str().is_some_and(|q| q.contains("reopenIssue")));
    }

    #[tokio::test]
    async fn a_token_without_write_access_is_forbidden_and_a_failed_lookup_changes_nothing() {
        let (refused, _) = closing(true, "close_insufficient_scopes").await;
        assert_eq!(refused, Err(Error::Forbidden));
        let issue = IssueRef { repo: "mahype/TorroWorkplace".into(), number: 7 };
        let calls = Arc::new(Mutex::new(0));
        let count = Arc::clone(&calls);
        let offline = set_done(
            move |_| {
                *count.lock().expect("count") += 1;
                async { Err(Error::Offline) }
            },
            &issue,
            true,
        )
        .await;
        assert_eq!((offline, *calls.lock().expect("count")), (Err(Error::Offline), 1), "no mutation after it");
    }

    #[tokio::test]
    async fn the_repositories_follow_the_cursor_and_leave_archived_ones_out() {
        let asked = Arc::new(Mutex::new(Vec::<Value>::new()));
        let record = Arc::clone(&asked);
        let repos = list_repos(move |body: Value| {
            let page = if body["variables"]["after"].is_null() { "repos_page1" } else { "repos_page2" };
            record.lock().expect("record").push(body);
            async move { Ok(fixture(page)) }
        })
        .await
        .expect("both pages");
        let names: Vec<(&str, bool)> = repos.iter().map(|repo| (repo.repo.as_str(), repo.private)).collect();
        assert_eq!(
            names,
            [("mahype/TorroWorkplace", false), ("awesome-org/website", true), ("mahype/torro-design", true)]
        );
        let asked = asked.lock().expect("record");
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0]["variables"]["first"], 100);
        assert_eq!(asked[1]["variables"]["after"], "Y3Vyc29yOnYyOpK5MjAyNi0xMC0xMlQwOTowMDowMCswMjowMM4AAAAB");
    }

    #[tokio::test]
    async fn the_repositories_stop_after_the_last_page_they_read() {
        let calls = Arc::new(Mutex::new(0));
        let count = Arc::clone(&calls);
        let repos = list_repos(move |_| {
            *count.lock().expect("count") += 1;
            async { Ok(fixture("repos_page1")) }
        })
        .await
        .expect("pages");
        assert_eq!(*calls.lock().expect("count"), REPO_PAGES, "a page that always has a next one stops");
        assert_eq!(repos.len(), 2 * REPO_PAGES);
        let failed = list_repos(|_| async { Err(Error::Offline) }).await;
        assert_eq!(failed, Err(Error::Offline));
    }

    #[tokio::test]
    async fn a_repository_is_described_by_owner_and_name_and_a_wrong_name_asks_nothing() {
        let asked = Arc::new(Mutex::new(Vec::<Value>::new()));
        let record = Arc::clone(&asked);
        let post = move |body: Value| {
            record.lock().expect("record").push(body);
            async { Ok(fixture("repo")) }
        };
        let described = describe(post.clone(), " mahype/TorroWorkplace ").await.expect("described");
        assert_eq!((described.labels.len(), described.milestones.len()), (3, 2));
        assert_eq!(
            asked.lock().expect("record")[0]["variables"],
            serde_json::json!({"o": "mahype", "r": "TorroWorkplace"})
        );
        for wrong in ["TorroWorkplace", "mahype/", "/x", "a/b/c", "a b/c", "mahype/Torro?x=1"] {
            let refused = describe(post.clone(), wrong).await;
            assert!(matches!(refused, Err(Error::Input(_))), "{wrong}: {refused:?}");
        }
        assert_eq!(asked.lock().expect("record").len(), 1, "GitHub is asked only for a name that fits");
        assert_eq!(repo_parts("my.org/my_repo-2"), Ok(("my.org", "my_repo-2")));
        let gone = describe(|_| async { Ok(fixture("repo_not_found")) }, "mahype/nope").await;
        assert_eq!(gone, Err(Error::NotFound));
    }

    #[test]
    fn the_token_does_not_show_in_debug_output() {
        let api = Api::new(reqwest::Client::new(), "http://127.0.0.1:9/graphql", Secret::new("ghp_SECRET"));
        assert!(!format!("{api:?}").contains("SECRET"));
    }
}
