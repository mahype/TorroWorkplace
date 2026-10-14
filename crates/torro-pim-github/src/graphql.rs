//! The GraphQL documents of the filter lists and how GitHub's answers are read.
//!
//! One document per kind of question: [`search_body`] (`search(type: ISSUE)`, one page) and
//! [`items_body`] (pinned items by repository and number). Both ask for the same fields of an
//! issue or pull request ([`ISSUE_FIELDS`], [`PULL_FIELDS`]), so pull request state, checks and
//! sub-issue parent come in the same call. Closing and reopening an issue (task G.5) takes two:
//! [`issue_id_body`] (the issue's node id, which the mutations need) and [`set_done_body`]
//! (`closeIssue` with `stateReason: COMPLETED`, or `reopenIssue`), which answers with the issue
//! as it is afterwards. The dialog of a filter list (task G.9) reads the repositories the account
//! reaches ([`repos_body`], page by page) and, in one call, a repository's labels, open milestones
//! and assignable people ([`repo_body`]). The inbox checks the issues and pull requests of its
//! GitHub entries (task E5.5) with [`standing_body`]: the pinned items' document, plus when each
//! was closed and whether a pull request still asks the signed-in account for a review. Values
//! travel as GraphQL variables, never inside the document. Answers are read by [`read_search`],
//! [`read_items`], [`read_standing`], [`read_issue_id`], [`read_set_done`], [`read_repos`] and
//! [`read_repo`]; GitHub's GraphQL documentation describes
//! their shape, and `tests/fixtures/` holds answers of that shape.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use torro_pim::model::{ChecksState, GitHubKind, GitHubLabel, GitHubState, ReviewState};
use torro_pim::source::{GitHubRepo, GitHubRepoDetails};

use crate::Error;
use crate::address::IssueRef;
use crate::item::Item;

/// How many labels and assignees of an item are read; more do not fit a row anyway.
const LABELS: usize = 20;
const ASSIGNEES: usize = 10;

/// The fields read of every issue. A fragment of its own, since a document must use every
/// fragment it defines: the mutations ([`set_done_body`]) answer with an issue only.
const ISSUE_FIELDS: &str = "fragment IssueFields on Issue { number title body url state updatedAt \
     repository { nameWithOwner } labels(first: 20) { nodes { name color } } \
     assignees(first: 10) { nodes { login } } milestone { title dueOn } \
     parent { number repository { nameWithOwner } } }";

/// The fields read of every pull request.
const PULL_FIELDS: &str = "fragment PullFields on PullRequest { number title body url state isDraft updatedAt \
     repository { nameWithOwner } labels(first: 20) { nodes { name color } } \
     assignees(first: 10) { nodes { login } } milestone { title dueOn } reviewDecision mergeable \
     commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } }";

/// One page of `query`'s matches: at most `first` (GitHub allows 100), after the cursor `after`.
pub fn search_body(query: &str, first: usize, after: Option<&str>) -> Value {
    let document = format!(
        "query($q: String!, $first: Int!, $after: String) {{ search(type: ISSUE, query: $q, first: $first, \
         after: $after) {{ issueCount pageInfo {{ hasNextPage endCursor }} nodes {{ __typename ...IssueFields \
         ...PullFields }} }} }} {ISSUE_FIELDS} {PULL_FIELDS}"
    );
    json!({ "query": document, "variables": { "q": query, "first": first, "after": after } })
}

/// The items `refs` in one call: alias `i<k>` asks repository `o<k>`/`r<k>` for number `n<k>`.
pub fn items_body(refs: &[IssueRef]) -> Value {
    lookup_body(refs, "", "")
}

/// What [`standing_body`] asks of an item besides [`ISSUE_FIELDS`] and [`PULL_FIELDS`] (task
/// E5.5): when it was closed (a merged pull request too), and whom a pull request still asks for
/// a review. GitHub drops a person's request once they review, and a team's once one of it does.
const STANDING_FIELDS: &str = " fragment IssueStanding on Issue { closedAt } \
     fragment PullStanding on PullRequest { closedAt reviewRequests(first: 100) { nodes { \
     requestedReviewer { __typename ... on User { isViewer } } } } }";

/// The items `refs` in one call as the inbox checks them (task E5.5): what [`items_body`] asks,
/// and [`STANDING_FIELDS`]. Read by [`read_standing`].
pub fn standing_body(refs: &[IssueRef]) -> Value {
    lookup_body(refs, " ...IssueStanding ...PullStanding", STANDING_FIELDS)
}

/// The items `refs` in one call, alias `i<k>` for `refs[k]`, each with [`ISSUE_FIELDS`],
/// [`PULL_FIELDS`] and the fragments `spread`, whose definitions are `fragments`.
fn lookup_body(refs: &[IssueRef], spread: &str, fragments: &str) -> Value {
    let mut parameters = Vec::new();
    let mut fields = Vec::new();
    let mut variables = serde_json::Map::new();
    for (k, item) in refs.iter().enumerate() {
        let (owner, name) = item.owner_and_name();
        parameters.push(format!("$o{k}: String!, $r{k}: String!, $n{k}: Int!"));
        fields.push(format!(
            "i{k}: repository(owner: $o{k}, name: $r{k}) {{ issueOrPullRequest(number: $n{k}) {{ __typename \
             ...IssueFields ...PullFields{spread} }} }}"
        ));
        variables.insert(format!("o{k}"), json!(owner));
        variables.insert(format!("r{k}"), json!(name));
        variables.insert(format!("n{k}"), json!(item.number));
    }
    let document =
        format!("query({}) {{ {} }} {ISSUE_FIELDS} {PULL_FIELDS}{fragments}", parameters.join(", "), fields.join(" "));
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

/// An issue or pull request as the inbox checks it (task E5.5, [`read_standing`]): is its
/// matter gone?
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// The item as it was asked for (`refs[k]` of [`standing_body`]), whatever GitHub calls its
    /// repository now: the caller finds its entry by it.
    pub asked: IssueRef,
    /// The item as GitHub reports it: [`Item::state`] says open, closed or merged.
    pub item: Item,
    /// When it was closed or merged; `None` while it is open.
    pub closed_at: Option<DateTime<Utc>>,
    /// A pull request still asks the signed-in account for a review: the account itself, or a
    /// team (GitHub does not say whether the account is in it, so any team asked counts: an
    /// entry rather stays than goes by mistake). Always `false` for an issue.
    pub asks_viewer: bool,
}

/// GitHub's answer to [`standing_body`] for `refs`: the items that exist and the token reaches,
/// in the order of `refs`. An item that is gone, or whose repository the token does not reach
/// (`NOT_FOUND`; `FORBIDDEN`: an organisation that wants single sign-on, a fine-grained token
/// without it), is left out, so its entry stays as it is; every other error fails the call.
pub fn read_standing(answer: &Value, refs: &[IssueRef]) -> Result<Vec<Standing>, Error> {
    let out_of_reach = |kind: Option<&str>| matches!(kind, Some("NOT_FOUND" | "FORBIDDEN"));
    if let Some(message) = first_error(answer, |kind| !out_of_reach(kind)) {
        return Err(Error::Github(format!("GitHub could not read the issues and pull requests: {message}")));
    }
    let Some(data) = answer.get("data").filter(|data| !data.is_null()) else {
        // Nothing the token reaches: `errors` says so. An answer with neither is no answer.
        return match answer.get("errors") {
            Some(_) => Ok(Vec::new()),
            None => Err(Error::Github("GitHub sent an answer without data.".into())),
        };
    };
    Ok(refs
        .iter()
        .enumerate()
        .filter_map(|(k, asked)| {
            let node = data.pointer(&format!("/i{k}/issueOrPullRequest")).filter(|node| !node.is_null())?;
            let extra: StandingNode = serde_json::from_value(node.clone()).ok()?;
            let item = serde_json::from_value::<Node>(node.clone()).ok()?.into_item()?;
            let asks_viewer =
                extra.review_requests.into_iter().flat_map(Connection::into_nodes).any(|request| {
                    request.requested_reviewer.is_some_and(|who| who.typename == "Team" || who.is_viewer)
                });
            Some(Standing { asked: asked.clone(), item, closed_at: extra.closed_at, asks_viewer })
        })
        .collect())
}

/// The node id of the issue `issue` (`I_kw…`): what [`set_done_body`] needs. The `__typename`
/// tells an issue from a pull request, which has no `id` asked for here.
pub fn issue_id_body(issue: &IssueRef) -> Value {
    let (owner, name) = issue.owner_and_name();
    let document = "query($o: String!, $r: String!, $n: Int!) { repository(owner: $o, name: $r) { \
                    issueOrPullRequest(number: $n) { __typename ... on Issue { id } } } }";
    json!({ "query": document, "variables": { "o": owner, "r": name, "n": issue.number } })
}

/// GitHub's answer to [`issue_id_body`]: the issue's node id. [`Error::NotFound`] when there is
/// no such issue or repository (or the token does not reach it), [`Error::Input`] for a pull
/// request, which is never closed here (`docs/github.md`, "Abhaken"); other errors as
/// [`change_error`] reads them.
pub fn read_issue_id(answer: &Value) -> Result<String, Error> {
    if let Some(err) = change_error(answer) {
        return Err(err);
    }
    let node = answer.pointer("/data/repository/issueOrPullRequest").filter(|node| !node.is_null());
    let Some(node) = node else { return Err(Error::NotFound) };
    match (node.get("__typename").and_then(Value::as_str), node.get("id").and_then(Value::as_str)) {
        (Some("Issue"), Some(id)) => Ok(id.to_string()),
        (Some("PullRequest"), _) => Err(Error::Input("Pull requests are never closed or reopened here.".into())),
        _ => Err(Error::Github("GitHub sent an answer without the issue.".into())),
    }
}

/// Closes the issue with the node id `id` as completed (`done`: `closeIssue` with
/// `stateReason: COMPLETED`) or reopens it (`reopenIssue`), and asks for the issue as it is
/// afterwards ([`ISSUE_FIELDS`]).
pub fn set_done_body(id: &str, done: bool) -> Value {
    let change = match done {
        true => "closeIssue(input: { issueId: $id, stateReason: COMPLETED })",
        false => "reopenIssue(input: { issueId: $id })",
    };
    let document =
        format!("mutation($id: ID!) {{ {change} {{ issue {{ __typename ...IssueFields }} }} }} {ISSUE_FIELDS}");
    json!({ "query": document, "variables": { "id": id } })
}

/// GitHub's answer to [`set_done_body`] with the same `done`: the issue as it is now. Errors as
/// [`change_error`] reads them.
pub fn read_set_done(answer: &Value, done: bool) -> Result<Item, Error> {
    if let Some(err) = change_error(answer) {
        return Err(err);
    }
    let pointer = if done { "/data/closeIssue/issue" } else { "/data/reopenIssue/issue" };
    answer
        .pointer(pointer)
        .cloned()
        .and_then(|node| serde_json::from_value::<Node>(node).ok())
        .and_then(Node::into_item)
        .ok_or_else(|| Error::Github("GitHub sent an answer without the issue.".into()))
}

/// Whose repositories [`repos_body`] lists: the account's own, those it collaborates on and those
/// of its organisations, both as `affiliations` and `ownerAffiliations` (GitHub's default for the
/// latter leaves the organisations' repositories out).
const AFFILIATIONS: &str = "[OWNER, COLLABORATOR, ORGANIZATION_MEMBER]";

/// One page of the repositories the token's account reaches (`viewer.repositories`, task G.9): at
/// most `first` (GitHub allows 100) after the cursor `after`, the most recently changed first.
/// Archived ones are asked for too and left out by [`read_repos`], so the document works whatever
/// filters GitHub's `repositories` field takes.
pub fn repos_body(first: usize, after: Option<&str>) -> Value {
    let document = format!(
        "query($first: Int!, $after: String) {{ viewer {{ repositories(first: $first, after: $after, \
         affiliations: {AFFILIATIONS}, ownerAffiliations: {AFFILIATIONS}, \
         orderBy: {{ field: UPDATED_AT, direction: DESC }}) {{ pageInfo {{ hasNextPage endCursor }} \
         nodes {{ nameWithOwner isPrivate isArchived }} }} }} }}"
    );
    json!({ "query": document, "variables": { "first": first, "after": after } })
}

/// One page of repositories, as [`read_repos`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPage {
    /// The repositories of the page that are not archived, in GitHub's order.
    pub repos: Vec<GitHubRepo>,
    /// The cursor of the next page; `None` on the last one.
    pub next: Option<String>,
}

/// GitHub's answer to [`repos_body`]. Any error in it fails the page.
pub fn read_repos(answer: &Value) -> Result<RepoPage, Error> {
    if let Some(message) = first_error(answer, |_| true) {
        return Err(Error::Github(format!("GitHub could not list the repositories: {message}")));
    }
    let repos: Repositories = answer
        .pointer("/data/viewer/repositories")
        .cloned()
        .and_then(|repos| serde_json::from_value(repos).ok())
        .ok_or_else(|| Error::Github("GitHub sent an answer without repositories.".into()))?;
    let next = repos.page_info.end_cursor.filter(|_| repos.page_info.has_next_page);
    let repos = repos
        .nodes
        .into_iter()
        .flatten()
        .filter(|repo| !repo.is_archived)
        .map(|repo| GitHubRepo { repo: repo.name_with_owner, private: repo.is_private })
        .collect();
    Ok(RepoPage { repos, next })
}

/// How many labels, open milestones and assignable people [`repo_body`] asks for: one page each,
/// so it stays one call.
const REPO_VALUES: usize = 100;

/// Labels (by name), open milestones (by due date) and the people issues can be assigned to of
/// the repository `owner`/`name`, in one call (task G.9).
pub fn repo_body(owner: &str, name: &str) -> Value {
    let document = format!(
        "query($o: String!, $r: String!) {{ repository(owner: $o, name: $r) {{ nameWithOwner \
         labels(first: {REPO_VALUES}, orderBy: {{ field: NAME, direction: ASC }}) {{ nodes {{ name color }} }} \
         milestones(first: {REPO_VALUES}, states: [OPEN], orderBy: {{ field: DUE_DATE, direction: ASC }}) \
         {{ nodes {{ title }} }} assignableUsers(first: {REPO_VALUES}) {{ nodes {{ login }} }} }} }}"
    );
    json!({ "query": document, "variables": { "o": owner, "r": name } })
}

/// GitHub's answer to [`repo_body`]: [`Error::NotFound`] when GitHub has no such repository or
/// the token does not reach it (`NOT_FOUND`, or no repository in the answer); any other error is
/// [`Error::Github`] with GitHub's message. Label colours come as `#rrggbb`.
pub fn read_repo(answer: &Value) -> Result<GitHubRepoDetails, Error> {
    if first_error(answer, |kind| kind == Some("NOT_FOUND")).is_some() {
        return Err(Error::NotFound);
    }
    if let Some(message) = first_error(answer, |_| true) {
        return Err(Error::Github(format!("GitHub could not read the repository: {message}")));
    }
    let node = answer.pointer("/data/repository").filter(|node| !node.is_null()).ok_or(Error::NotFound)?;
    let repo: Described = serde_json::from_value(node.clone())
        .map_err(|_| Error::Github("GitHub sent an answer without the repository.".into()))?;
    Ok(GitHubRepoDetails {
        repo: repo.name_with_owner,
        labels: repo
            .labels
            .into_iter()
            .flat_map(Connection::into_nodes)
            .map(|label| GitHubLabel { name: label.name, color: label.color.map(|c| format!("#{c}")) })
            .collect(),
        milestones: repo.milestones.into_iter().flat_map(Connection::into_nodes).map(|m| m.title).collect(),
        assignees: repo.assignable_users.into_iter().flat_map(Connection::into_nodes).map(|u| u.login).collect(),
    })
}

/// The first error of an answer to [`issue_id_body`] or [`set_done_body`], by its `type`:
/// `FORBIDDEN` and `INSUFFICIENT_SCOPES` are [`Error::Forbidden`] (the token may not write
/// issues there), `NOT_FOUND` is [`Error::NotFound`], anything else [`Error::Github`] with
/// GitHub's message.
///
/// Only `errors[0]` is read: GitHub answers a change with one error, and the first one decides
/// what the caller can do about it. Further errors in the same answer are ignored on purpose.
fn change_error(answer: &Value) -> Option<Error> {
    let error = answer.get("errors")?.as_array()?.first()?;
    let message = error.get("message").and_then(Value::as_str).unwrap_or("unknown error");
    Some(match error.get("type").and_then(Value::as_str) {
        Some("FORBIDDEN" | "INSUFFICIENT_SCOPES") => Error::Forbidden,
        Some("NOT_FOUND") => Error::NotFound,
        _ => Error::Github(format!("GitHub could not change the issue: {message}")),
    })
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

/// `viewer.repositories` as [`repos_body`] asks for it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repositories {
    page_info: PageInfo,
    #[serde(default)]
    nodes: Vec<Option<RepoNode>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoNode {
    name_with_owner: String,
    #[serde(default)]
    is_private: bool,
    #[serde(default)]
    is_archived: bool,
}

/// A repository as [`repo_body`] asks for it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Described {
    name_with_owner: String,
    labels: Option<Connection<Label>>,
    milestones: Option<Connection<OpenMilestone>>,
    assignable_users: Option<Connection<User>>,
}

#[derive(Deserialize)]
struct OpenMilestone {
    title: String,
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

/// What [`STANDING_FIELDS`] adds to an issue or pull request.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StandingNode {
    closed_at: Option<DateTime<Utc>>,
    review_requests: Option<Connection<ReviewRequest>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewRequest {
    requested_reviewer: Option<Reviewer>,
}

/// Who is asked for a review: a `User` (with `isViewer`), a `Team`, a `Bot` or a `Mannequin`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reviewer {
    #[serde(rename = "__typename")]
    typename: String,
    #[serde(default)]
    is_viewer: bool,
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
            "issue_id" => include_str!("../tests/fixtures/issue_id.json"),
            "close_issue" => include_str!("../tests/fixtures/close_issue.json"),
            "reopen_issue" => include_str!("../tests/fixtures/reopen_issue.json"),
            "close_insufficient_scopes" => include_str!("../tests/fixtures/close_insufficient_scopes.json"),
            "close_forbidden" => include_str!("../tests/fixtures/close_forbidden.json"),
            "repos_page1" => include_str!("../tests/fixtures/repos_page1.json"),
            "repos_page2" => include_str!("../tests/fixtures/repos_page2.json"),
            "repo" => include_str!("../tests/fixtures/repo.json"),
            "repo_not_found" => include_str!("../tests/fixtures/repo_not_found.json"),
            "standing" => include_str!("../tests/fixtures/standing.json"),
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

    /// The eight items `tests/fixtures/standing.json` answers for, in its order.
    fn standing_refs() -> Vec<IssueRef> {
        let mine = |number| IssueRef { repo: "mahype/TorroWorkplace".into(), number };
        let mut refs: Vec<IssueRef> = [42, 43, 44, 45, 7, 8].into_iter().map(mine).collect();
        refs.push(IssueRef { repo: "mahype/secret".into(), number: 1 });
        refs.push(IssueRef { repo: "sso-org/app".into(), number: 2 });
        refs
    }

    #[test]
    fn the_inbox_asks_what_the_pinned_items_ask_and_when_closed_and_whom_a_review_asks() {
        let refs = standing_refs();
        let body = standing_body(&refs[..2]);
        assert_eq!(body["variables"], items_body(&refs[..2])["variables"], "the same variables");
        let document = body["query"].as_str().expect("document");
        for part in [
            "i1: repository(owner: $o1, name: $r1) { issueOrPullRequest(number: $n1) { __typename ...IssueFields \
             ...PullFields ...IssueStanding ...PullStanding } }",
            "fragment IssueStanding on Issue { closedAt }",
            "reviewRequests(first: 100) { nodes { requestedReviewer { __typename ... on User { isViewer } } } }",
        ] {
            assert!(document.contains(part), "{part} in {document}");
        }
        let pinned = items_body(&refs[..2]);
        assert!(!pinned["query"].as_str().expect("document").contains("Standing"), "the pinned items ask less");
    }

    #[test]
    fn standing_reads_merged_closed_reviewed_and_open_items_and_leaves_out_what_is_out_of_reach() {
        let refs = standing_refs();
        let standing = read_standing(&fixture("standing"), &refs).expect("standing");
        let at = |text: &str| text.parse::<DateTime<Utc>>().ok();
        let seen: Vec<(u64, GitHubState, Option<DateTime<Utc>>, bool)> =
            standing.iter().map(|s| (s.asked.number, s.item.state, s.closed_at, s.asks_viewer)).collect();
        assert_eq!(
            seen,
            [
                (42, GitHubState::Merged, at("2026-10-13T09:50:00Z"), false),
                (43, GitHubState::Open, None, false),
                (44, GitHubState::Open, None, true),
                (45, GitHubState::Open, None, true),
                (7, GitHubState::Closed, at("2026-10-13T09:45:00Z"), false),
                (8, GitHubState::Open, None, false),
            ],
            "merged, review given, still asked, a team asked, closed, open; no secret, no single sign-on"
        );
        assert_eq!(standing[0].asked, refs[0], "as asked for");
        assert_eq!(standing[0].item.kind, GitHubKind::PullRequest);
        assert_eq!(standing[4].item.kind, GitHubKind::Issue);

        let shut = json!({"data": null, "errors": [{"type": "FORBIDDEN", "message": "SAML"}]});
        assert_eq!(read_standing(&shut, &refs[..1]), Ok(Vec::new()), "nothing reached, nothing failed");
        let limited = json!({"data": null, "errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]});
        assert_eq!(read_standing(&limited, &refs).map_err(|err| err.code()), Err("error"));
        assert!(read_standing(&json!({}), &refs).is_err());
    }

    #[test]
    fn closing_asks_for_the_node_id_then_closes_as_completed_or_reopens() {
        let issue = IssueRef { repo: "mahype/TorroWorkplace".into(), number: 123 };
        let body = issue_id_body(&issue);
        assert_eq!(body["variables"], json!({"o": "mahype", "r": "TorroWorkplace", "n": 123}));
        assert!(!body["query"].as_str().expect("document").contains("mahype"), "values are variables");
        assert_eq!(read_issue_id(&fixture("issue_id")).as_deref(), Ok("I_kwDOMahype85Abc123"));

        let close = set_done_body("I_kwDOMahype85Abc123", true);
        let document = close["query"].as_str().expect("document");
        assert!(document.contains("closeIssue(input: { issueId: $id, stateReason: COMPLETED })"), "{document}");
        assert!(!document.contains("PullFields") && !document.contains("I_kw"), "{document}");
        assert_eq!(close["variables"], json!({"id": "I_kwDOMahype85Abc123"}));
        let reopen = set_done_body("I_kwDOMahype85Abc123", false);
        assert!(reopen["query"].as_str().expect("document").contains("reopenIssue(input: { issueId: $id })"));

        let closed = read_set_done(&fixture("close_issue"), true).expect("the closed issue");
        assert_eq!((closed.id.number, closed.kind, closed.state), (123, GitHubKind::Issue, GitHubState::Closed));
        assert_eq!(closed.updated_at, "2026-10-12T09:30:00Z", "the new version");
        let reopened = read_set_done(&fixture("reopen_issue"), false).expect("the open issue");
        assert_eq!(reopened.state, GitHubState::Open);
        assert!(read_set_done(&fixture("close_issue"), false).is_err(), "the answer to the other question");
    }

    #[test]
    fn a_token_that_may_not_write_a_missing_issue_and_a_pull_request_are_told_apart() {
        for name in ["close_insufficient_scopes", "close_forbidden"] {
            assert_eq!(read_set_done(&fixture(name), true), Err(Error::Forbidden), "{name}");
            assert_eq!(read_issue_id(&fixture(name)), Err(Error::Forbidden), "{name}");
        }
        let gone = json!({"data": {"repository": null},
            "errors": [{"type": "NOT_FOUND", "message": "Could not resolve to a Repository with the name 'a/b'."}]});
        assert_eq!(read_issue_id(&gone), Err(Error::NotFound));
        assert_eq!(read_issue_id(&json!({"data": {"repository": {"issueOrPullRequest": null}}})), Err(Error::NotFound));
        let pull = json!({"data": {"repository": {"issueOrPullRequest": {"__typename": "PullRequest"}}}});
        assert_eq!(read_issue_id(&pull).map_err(|err| err.code()), Err("error"));
        let limited = json!({"errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]});
        let err = read_set_done(&limited, true).expect_err("an error");
        assert_eq!(err.code(), "error");
        assert!(err.to_string().contains("rate limit"), "the suite reads GitHub's words: {err}");
    }

    #[test]
    fn repositories_are_read_without_archived_ones_with_their_cursor() {
        let page = read_repos(&fixture("repos_page1")).expect("a page");
        let repo = |name: &str, private| GitHubRepo { repo: name.into(), private };
        assert_eq!(page.repos, [repo("mahype/TorroWorkplace", false), repo("awesome-org/website", true)]);
        assert_eq!(page.next.as_deref(), Some("Y3Vyc29yOnYyOpK5MjAyNi0xMC0xMlQwOTowMDowMCswMjowMM4AAAAB"));
        let last = read_repos(&fixture("repos_page2")).expect("a page");
        assert_eq!((last.repos, last.next), (vec![repo("mahype/torro-design", true)], None));
        let limited = json!({"errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]});
        assert_eq!(read_repos(&limited).map_err(|err| err.code()), Err("error"));
        assert!(read_repos(&json!({"data": {"viewer": null}})).is_err());
    }

    #[test]
    fn the_repositories_document_asks_for_every_affiliation_newest_first() {
        let body = repos_body(100, Some("abc"));
        assert_eq!(body["variables"], json!({"first": 100, "after": "abc"}));
        let document = body["query"].as_str().expect("document");
        for part in [
            "viewer { repositories(",
            "affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]",
            "ownerAffiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]",
            "orderBy: { field: UPDATED_AT, direction: DESC }",
            "isArchived",
        ] {
            assert!(document.contains(part), "{part} in {document}");
        }
        assert_eq!(repos_body(100, None)["variables"]["after"], Value::Null);
    }

    #[test]
    fn a_repository_is_described_in_one_call() {
        let body = repo_body("mahype", "TorroWorkplace");
        assert_eq!(body["variables"], json!({"o": "mahype", "r": "TorroWorkplace"}));
        let document = body["query"].as_str().expect("document");
        assert!(!document.contains("mahype"), "values are variables: {document}");
        for part in ["labels(first: 100", "milestones(first: 100, states: [OPEN]", "assignableUsers(first: 100)"] {
            assert!(document.contains(part), "{part} in {document}");
        }

        let described = read_repo(&fixture("repo")).expect("the repository");
        assert_eq!(described.repo, "mahype/TorroWorkplace");
        let label = |name: &str, color: &str| GitHubLabel { name: name.into(), color: Some(color.into()) };
        assert_eq!(
            described.labels,
            [label("bug", "#d73a4a"), label("enhancement", "#a2eeef"), label("good first issue", "#7057ff")]
        );
        assert_eq!(described.milestones, ["v0.4", "v0.5"]);
        assert_eq!(described.assignees, ["mahype", "octocat"]);
    }

    #[test]
    fn a_repository_github_does_not_have_is_not_found_and_other_errors_fail() {
        assert_eq!(read_repo(&fixture("repo_not_found")), Err(Error::NotFound));
        assert_eq!(read_repo(&json!({"data": {"repository": null}})), Err(Error::NotFound));
        let limited = json!({"data": null, "errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]});
        let err = read_repo(&limited).expect_err("an error");
        assert_eq!(err.code(), "error");
        assert!(err.to_string().contains("rate limit"), "{err}");
        let bare = read_repo(&json!({"data": {"repository": {"nameWithOwner": "a/b"}}})).expect("no values");
        assert_eq!(bare, GitHubRepoDetails { repo: "a/b".into(), ..GitHubRepoDetails::default() });
    }
}
