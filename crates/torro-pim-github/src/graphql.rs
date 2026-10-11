//! The GraphQL documents of the filter lists and how GitHub's answers are read.
//!
//! One document per kind of question: [`search_body`] (`search(type: ISSUE)`, one page) and
//! [`items_body`] (pinned items by repository and number). Both ask for the same fields of an
//! issue or pull request ([`FIELDS`]), so pull request state, checks and sub-issue parent come in
//! the same call. Values travel as GraphQL variables, never inside the document. Answers are read
//! by [`read_search`] and [`read_items`]; GitHub's GraphQL documentation describes their shape,
//! and `tests/fixtures/` holds answers of that shape.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use torro_pim::model::{ChecksState, GitHubKind, GitHubLabel, GitHubState, ReviewState};

use crate::Error;
use crate::address::IssueRef;
use crate::item::Item;

/// How many labels and assignees of an item are read; more do not fit a row anyway.
const LABELS: usize = 20;
const ASSIGNEES: usize = 10;

/// The fields read of every issue and pull request, as two fragments.
const FIELDS: &str = "fragment IssueFields on Issue { number title body url state updatedAt \
     repository { nameWithOwner } labels(first: 20) { nodes { name color } } \
     assignees(first: 10) { nodes { login } } milestone { title dueOn } \
     parent { number repository { nameWithOwner } } } \
     fragment PullFields on PullRequest { number title body url state isDraft updatedAt \
     repository { nameWithOwner } labels(first: 20) { nodes { name color } } \
     assignees(first: 10) { nodes { login } } milestone { title dueOn } reviewDecision mergeable \
     commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } }";

/// One page of `query`'s matches: at most `first` (GitHub allows 100), after the cursor `after`.
pub fn search_body(query: &str, first: usize, after: Option<&str>) -> Value {
    let document = format!(
        "query($q: String!, $first: Int!, $after: String) {{ search(type: ISSUE, query: $q, first: $first, \
         after: $after) {{ issueCount pageInfo {{ hasNextPage endCursor }} nodes {{ __typename ...IssueFields \
         ...PullFields }} }} }} {FIELDS}"
    );
    json!({ "query": document, "variables": { "q": query, "first": first, "after": after } })
}

/// The items `refs` in one call: alias `i<k>` asks repository `o<k>`/`r<k>` for number `n<k>`.
pub fn items_body(refs: &[IssueRef]) -> Value {
    let mut parameters = Vec::new();
    let mut fields = Vec::new();
    let mut variables = serde_json::Map::new();
    for (k, item) in refs.iter().enumerate() {
        let (owner, name) = item.owner_and_name();
        parameters.push(format!("$o{k}: String!, $r{k}: String!, $n{k}: Int!"));
        fields.push(format!(
            "i{k}: repository(owner: $o{k}, name: $r{k}) {{ issueOrPullRequest(number: $n{k}) {{ __typename \
             ...IssueFields ...PullFields }} }}"
        ));
        variables.insert(format!("o{k}"), json!(owner));
        variables.insert(format!("r{k}"), json!(name));
        variables.insert(format!("n{k}"), json!(item.number));
    }
    let document = format!("query({}) {{ {} }} {FIELDS}", parameters.join(", "), fields.join(" "));
    json!({ "query": document, "variables": variables })
}

/// One page of a search, as [`read_search`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// How many items match in all, as GitHub counts them (more than it hands out beyond 1,000).
    pub total: u64,
    pub items: Vec<Item>,
    /// The cursor of the next page; `None` on the last one.
    pub next: Option<String>,
}

/// GitHub's answer to [`search_body`]. Any error in it fails the page: a search GitHub cannot
/// run (an unknown qualifier, a repository the token does not reach) says so in `errors`.
pub fn read_search(answer: &Value) -> Result<Page, Error> {
    if let Some(message) = first_error(answer, |_| true) {
        return Err(Error::Github(format!("GitHub could not run the search: {message}")));
    }
    let search: Search = answer
        .pointer("/data/search")
        .cloned()
        .and_then(|search| serde_json::from_value(search).ok())
        .ok_or_else(|| Error::Github("GitHub sent an answer without search results.".into()))?;
    let items = search.nodes.into_iter().flatten().filter_map(Node::into_item).collect();
    let next = search.page_info.end_cursor.filter(|_| search.page_info.has_next_page);
    Ok(Page { total: search.issue_count, items, next })
}

/// GitHub's answer to [`items_body`] for `count` items: the ones that exist, in order. A pinned
/// item that is gone, or a repository the token no longer reaches, is left out (GitHub reports
/// `NOT_FOUND` for it); every other error fails the call.
pub fn read_items(answer: &Value, count: usize) -> Result<Vec<Item>, Error> {
    if let Some(message) = first_error(answer, |kind| kind != Some("NOT_FOUND")) {
        return Err(Error::Github(format!("GitHub could not read the pinned items: {message}")));
    }
    let data = answer.get("data").ok_or_else(|| Error::Github("GitHub sent an answer without data.".into()))?;
    Ok((0..count)
        .filter_map(|k| data.pointer(&format!("/i{k}/issueOrPullRequest")).cloned())
        .filter_map(|node| serde_json::from_value::<Node>(node).ok())
        .filter_map(Node::into_item)
        .collect())
}

/// The message of the first error in `answer` whose `type` `counts`.
fn first_error(answer: &Value, counts: impl Fn(Option<&str>) -> bool) -> Option<String> {
    let errors = answer.get("errors")?.as_array()?;
    errors
        .iter()
        .find(|error| counts(error.get("type").and_then(Value::as_str)))
        .map(|error| error.get("message").and_then(Value::as_str).unwrap_or("unknown error").to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Search {
    issue_count: u64,
    page_info: PageInfo,
    #[serde(default)]
    nodes: Vec<Option<Node>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct Connection<T> {
    #[serde(default = "Vec::new")]
    nodes: Vec<Option<T>>,
}

impl<T> Connection<T> {
    fn into_nodes(self) -> impl Iterator<Item = T> {
        self.nodes.into_iter().flatten()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    name_with_owner: String,
}

#[derive(Deserialize)]
struct Label {
    name: String,
    color: Option<String>,
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Milestone {
    title: String,
    due_on: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct Parent {
    number: u64,
    repository: Repository,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitNode {
    commit: Commit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    status_check_rollup: Option<Rollup>,
}

#[derive(Deserialize)]
struct Rollup {
    state: String,
}

/// An issue or pull request as GitHub sends it; every field optional, so an answer missing one
/// costs that item, not the page.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    #[serde(rename = "__typename")]
    typename: String,
    number: Option<u64>,
    title: Option<String>,
    body: Option<String>,
    url: Option<String>,
    state: Option<String>,
    updated_at: Option<String>,
    repository: Option<Repository>,
    labels: Option<Connection<Label>>,
    assignees: Option<Connection<User>>,
    milestone: Option<Milestone>,
    parent: Option<Parent>,
    #[serde(default)]
    is_draft: bool,
    review_decision: Option<String>,
    mergeable: Option<String>,
    commits: Option<Connection<CommitNode>>,
}

impl Node {
    /// The item, or `None` for a node that is no issue or pull request or lacks what makes one.
    fn into_item(self) -> Option<Item> {
        let kind = match self.typename.as_str() {
            "Issue" => GitHubKind::Issue,
            "PullRequest" => GitHubKind::PullRequest,
            _ => return None,
        };
        let state = match self.state.as_deref()? {
            "OPEN" => GitHubState::Open,
            "MERGED" => GitHubState::Merged,
            _ => GitHubState::Closed,
        };
        let labels = self.labels.map(Connection::into_nodes).into_iter().flatten().take(LABELS);
        let assignees = self.assignees.map(Connection::into_nodes).into_iter().flatten().take(ASSIGNEES);
        let checks = self
            .commits
            .and_then(|commits| commits.into_nodes().last())
            .and_then(|node| node.commit.status_check_rollup)
            .and_then(|rollup| checks_of(&rollup.state));
        Some(Item {
            id: IssueRef { repo: self.repository?.name_with_owner, number: self.number? },
            kind,
            state,
            title: self.title?,
            body: self.body.unwrap_or_default(),
            url: self.url?,
            updated_at: self.updated_at.unwrap_or_default(),
            labels: labels
                .map(|label| GitHubLabel { name: label.name, color: label.color.map(|c| format!("#{c}")) })
                .collect(),
            assignees: assignees.map(|user| user.login).collect(),
            milestone: self.milestone.as_ref().map(|m| m.title.clone()),
            milestone_due: self.milestone.and_then(|m| m.due_on).map(|due| due.date_naive()),
            parent: self.parent.map(|p| IssueRef { repo: p.repository.name_with_owner, number: p.number }),
            draft: self.is_draft,
            review: match self.review_decision.as_deref() {
                Some("APPROVED") => Some(ReviewState::Approved),
                Some("CHANGES_REQUESTED") => Some(ReviewState::ChangesRequested),
                Some("REVIEW_REQUIRED") => Some(ReviewState::Required),
                _ => None,
            },
            checks,
            conflict: self.mergeable.as_deref() == Some("CONFLICTING"),
        })
    }
}

/// GitHub's `StatusState` of the checks on a pull request's head commit.
fn checks_of(state: &str) -> Option<ChecksState> {
    match state {
        "SUCCESS" => Some(ChecksState::Success),
        "FAILURE" | "ERROR" => Some(ChecksState::Failure),
        "PENDING" | "EXPECTED" => Some(ChecksState::Pending),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use chrono::NaiveDate;

    use super::*;

    pub(crate) fn day(text: &str) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
    }

    pub(crate) fn fixture(name: &str) -> Value {
        let text = match name {
            "search_page1" => include_str!("../tests/fixtures/search_page1.json"),
            "search_page2" => include_str!("../tests/fixtures/search_page2.json"),
            "search_error" => include_str!("../tests/fixtures/search_error.json"),
            "pinned" => include_str!("../tests/fixtures/pinned.json"),
            other => panic!("no fixture {other}"),
        };
        serde_json::from_str(text).expect("valid fixture")
    }

    #[test]
    fn a_search_page_reads_issues_and_pull_requests_and_its_cursor() {
        let page = read_search(&fixture("search_page1")).expect("a page");
        assert_eq!(page.total, 3);
        assert_eq!(page.next.as_deref(), Some("Y3Vyc29yOjI="));
        assert_eq!(page.items.len(), 2, "the node that is no issue is left out");

        let issue = &page.items[0];
        assert_eq!((issue.repo(), issue.id.number, issue.kind), ("mahype/TorroWorkplace", 123, GitHubKind::Issue));
        assert_eq!(issue.state, GitHubState::Open);
        assert_eq!(issue.title, "Calendar crashes on DST change");
        assert_eq!(issue.labels, vec![GitHubLabel { name: "bug".into(), color: Some("#d73a4a".into()) }]);
        assert_eq!(issue.assignees, ["mahype"]);
        assert_eq!(issue.milestone.as_deref(), Some("1.0"));
        assert_eq!(issue.milestone_due, day("2026-11-01"));
        assert_eq!(issue.parent, Some(IssueRef { repo: "mahype/TorroWorkplace".into(), number: 100 }));

        let pull = &page.items[1];
        assert_eq!((pull.kind, pull.state, pull.draft), (GitHubKind::PullRequest, GitHubState::Open, true));
        assert_eq!(
            (pull.review, pull.checks, pull.conflict),
            (Some(ReviewState::ChangesRequested), Some(ChecksState::Failure), true)
        );
        assert_eq!(pull.parent, None);
    }

    #[test]
    fn the_last_page_has_no_cursor_and_a_merged_pull_request_is_merged() {
        let page = read_search(&fixture("search_page2")).expect("a page");
        assert_eq!(page.next, None);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].state, GitHubState::Merged);
        assert_eq!(page.items[0].review, Some(ReviewState::Approved));
        assert_eq!(page.items[0].checks, Some(ChecksState::Success));
    }

    #[test]
    fn errors_in_a_search_fail_it() {
        let err = read_search(&fixture("search_error")).expect_err("an error");
        assert_eq!(err.code(), "error");
        assert!(err.to_string().contains("GitHub could not run the search"), "{err}");
        assert!(read_search(&json!({"data": null})).is_err());
    }

    #[test]
    fn pinned_items_that_are_gone_are_left_out() {
        let items = read_items(&fixture("pinned"), 3).expect("items");
        let ids: Vec<String> = items.iter().map(|item| item.id.link_id()).collect();
        assert_eq!(ids, ["github:mahype/TorroWorkplace#7", "github:mahype/other#9"]);
        assert_eq!(items[0].state, GitHubState::Closed);
        let other = json!({"errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}], "data": null});
        assert!(read_items(&other, 1).is_err());
    }

    #[test]
    fn values_travel_as_variables() {
        let body = search_body("repo:a/b \"quoted\" }", 100, Some("abc"));
        assert_eq!(body["variables"], json!({"q": "repo:a/b \"quoted\" }", "first": 100, "after": "abc"}));
        assert!(!body["query"].as_str().expect("document").contains("quoted"));
        let refs =
            [IssueRef { repo: "mahype/TorroWorkplace".into(), number: 7 }, IssueRef { repo: "a/b".into(), number: 9 }];
        let body = items_body(&refs);
        assert_eq!(body["variables"]["o0"], "mahype");
        assert_eq!(body["variables"]["r1"], "b");
        assert_eq!(body["variables"]["n1"], 9);
        let document = body["query"].as_str().expect("document");
        assert!(document.contains("i1: repository(owner: $o1, name: $r1)") && !document.contains("mahype"));
    }
}
