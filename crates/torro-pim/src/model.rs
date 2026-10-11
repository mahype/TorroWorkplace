//! The shared model, shaped after iCalendar (RFC 5545) so CalDAV maps one to one and
//! other providers map onto it.

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A calendar or task list on an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    /// `<account-id>/<provider id>`: the account it belongs to, then the provider's own id (for
    /// CalDAV the collection URL). Unique across all accounts; see [`crate::account`].
    pub id: String,
    pub name: String,
    pub kind: CollectionKind,
    /// `#RRGGBB` as the server reports it, if any.
    pub color: Option<String>,
    /// The user may add, change and delete items. Shared read-only calendars and
    /// Nextcloud's contact birthdays are not writable.
    #[serde(default = "writable_by_default")]
    pub writable: bool,
}

fn writable_by_default() -> bool {
    true
}

/// What a collection holds. A CalDAV calendar may hold both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollectionKind {
    Events,
    Todos,
    Both,
}

impl CollectionKind {
    pub fn has_events(self) -> bool {
        matches!(self, CollectionKind::Events | CollectionKind::Both)
    }

    pub fn has_todos(self) -> bool {
        matches!(self, CollectionKind::Todos | CollectionKind::Both)
    }
}

/// When an event starts or ends, or a task is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EventTime {
    /// A calendar day (all-day entries, VALUE=DATE).
    Date(NaiveDate),
    /// An instant.
    DateTime(DateTime<Utc>),
}

/// A calendar entry (VEVENT). A recurring event appears once per occurrence, each with
/// the start of that occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Event {
    /// iCalendar UID; shared by all occurrences of a recurring event.
    pub uid: String,
    /// Id of the calendar it belongs to, as `calendar.list_calendars` reports it. Must name a
    /// calendar that can be written to when saving.
    pub collection: String,
    /// Where the item lives on the server (CalDAV URL). `None` only for an event not saved yet;
    /// pass the event back with its `href` and `etag` to change or delete it.
    pub href: Option<String>,
    /// Server version of the item, sent back on changes so the server can refuse stale writes.
    /// `None` only for an event not saved yet.
    pub etag: Option<String>,
    /// Title of the event.
    pub summary: String,
    /// Start: a calendar day for all-day events, otherwise an instant in UTC.
    pub start: EventTime,
    /// Exclusive end (DTEND, or DTSTART + DURATION); `None` when the entry has neither.
    pub end: Option<EventTime>,
    pub location: Option<String>,
    pub description: Option<String>,
    /// One occurrence of a recurring series. Editing series is not supported yet, so the
    /// apps show these read-only.
    pub recurring: bool,
}

impl Event {
    /// All-day entries carry a date without a time.
    pub fn all_day(&self) -> bool {
        matches!(self.start, EventTime::Date(_))
    }
}

/// A task (VTODO).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Todo {
    /// iCalendar UID.
    pub uid: String,
    /// Id of the task list it belongs to, as `tasks.list_lists` reports it. Must name a list
    /// that can be written to when saving.
    pub collection: String,
    /// Where the item lives on the server (CalDAV URL). `None` only for a task not saved yet;
    /// pass the task back with its `href` and `etag` to change or delete it.
    pub href: Option<String>,
    /// Server version of the item, sent back on changes so the server can refuse stale writes.
    /// `None` only for a task not saved yet.
    pub etag: Option<String>,
    /// Title of the task.
    pub summary: String,
    /// When the task is due: a day or an instant. `None` when it has no due date.
    pub due: Option<EventTime>,
    /// iCalendar priority: 1 highest … 9 lowest, `None` undefined.
    pub priority: Option<u8>,
    pub completed: bool,
    /// UID of the parent task (RELATED-TO;RELTYPE=PARENT).
    pub parent: Option<String>,
    pub description: Option<String>,
    /// A web address that belongs to the task (iCalendar `URL`), e.g. the issue it is about.
    /// The apps open only `http` and `https` addresses. `None` when there is none.
    #[serde(default)]
    pub url: Option<String>,
    /// Free tags (iCalendar `CATEGORIES`), in the stored order. Empty when there are none;
    /// saving an empty list removes them. Empty tags are dropped, surrounding spaces trimmed.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Position in a list ordered by hand (`X-APPLE-SORT-ORDER`, as Nextcloud Tasks and Apple
    /// Reminders write it): smaller comes first. `None` when the task was never placed by hand.
    #[serde(default)]
    pub sort_order: Option<i64>,
    /// The item of another service the task stands for, e.g. a GitHub issue (`X-TORRO-LINK`).
    /// Only [`Link::id`] is stored with the task; the core fills in the rest when it reads.
    #[serde(default)]
    pub link: Option<Link>,
}

/// A task's link to an item of another service; today GitHub issues and pull requests.
///
/// The task stores only [`Link::id`]. What the service reports about the item is in
/// [`Link::github`]: fetched by the core on every sync and never written into the stored task.
/// Clients may send a link back as they got it; saving uses only the id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    /// Stable id of the linked item: `github:<owner>/<repo>#<number>` for a GitHub issue or pull
    /// request, e.g. `github:mahype/TorroWorkplace#123`. Ids of other forms are kept as they are.
    pub id: String,
    /// What GitHub reports for the item. `None` until the core fetched it, without a GitHub
    /// account, or when the id is not a GitHub one.
    #[serde(default)]
    pub github: Option<GitHubDetails>,
}

/// The prefix of a GitHub [`Link::id`].
const GITHUB_PREFIX: &str = "github:";

impl Link {
    /// The link to issue or pull request `number` in `repo` (`owner/name`), without details.
    pub fn github(repo: &str, number: u64) -> Link {
        Link { id: format!("{GITHUB_PREFIX}{repo}#{number}"), github: None }
    }

    /// `("owner/name", number)` when the id is a GitHub one (`github:<owner>/<repo>#<number>`,
    /// owner and repo made of ASCII letters, digits, `-`, `_` and `.`); `None` otherwise.
    pub fn github_ref(&self) -> Option<(&str, u64)> {
        let (repo, number) = self.id.strip_prefix(GITHUB_PREFIX)?.rsplit_once('#')?;
        let (owner, name) = repo.split_once('/')?;
        let part =
            |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !(part(owner) && part(name)) {
            return None;
        }
        Some((repo, number.parse().ok()?))
    }

    /// The item's page: `https://github.com/<owner>/<repo>/issues/<number>` for a GitHub id
    /// (GitHub forwards it to the pull request when it is one); `None` for other ids.
    pub fn web_url(&self) -> Option<String> {
        let (repo, number) = self.github_ref()?;
        Some(format!("https://github.com/{repo}/issues/{number}"))
    }
}

/// What GitHub reports about a linked issue or pull request. The core fills it in; clients
/// only read it. Repository and number are part of [`Link::id`] ([`Link::github_ref`]); the
/// title is the task's `summary`, labels are also the task's `categories`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitHubDetails {
    /// Issue or pull request.
    pub kind: GitHubKind,
    /// Open, closed, or (pull requests only) merged.
    pub state: GitHubState,
    /// The item's labels, in GitHub's order.
    #[serde(default)]
    pub labels: Vec<GitHubLabel>,
    /// Logins of the people assigned.
    #[serde(default)]
    pub assignees: Vec<String>,
    /// Title of the milestone the item is in, if any.
    #[serde(default)]
    pub milestone: Option<String>,
    /// Due date of that milestone, if it has one.
    #[serde(default)]
    pub milestone_due: Option<NaiveDate>,
    /// Pull requests only: still a draft.
    #[serde(default)]
    pub draft: bool,
    /// Pull requests only: GitHub's review decision; `None` when no review is asked for.
    #[serde(default)]
    pub review: Option<ReviewState>,
    /// Pull requests only: combined result of the checks (CI) on the head commit; `None`
    /// without checks.
    #[serde(default)]
    pub checks: Option<ChecksState>,
    /// Pull requests only: cannot be merged because of conflicts.
    #[serde(default)]
    pub conflict: bool,
}

/// What a GitHub item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitHubKind {
    Issue,
    PullRequest,
}

/// Where a GitHub item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitHubState {
    Open,
    Closed,
    /// A merged pull request (closed, too).
    Merged,
}

/// One label of a GitHub item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitHubLabel {
    pub name: String,
    /// `#RRGGBB`, the label's colour on GitHub.
    #[serde(default)]
    pub color: Option<String>,
}

/// GitHub's review decision on a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    /// A review is required and not given yet.
    Required,
    Approved,
    ChangesRequested,
}

/// Combined result of a pull request's checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChecksState {
    /// Still running or waiting to run.
    Pending,
    Success,
    /// At least one check failed.
    Failure,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_from_an_older_client_reads_without_the_new_fields() {
        let json = r#"{"uid":"t","collection":"a/https://x/tasks/","href":null,"etag":null,"summary":"Milk",
            "due":null,"priority":null,"completed":false,"parent":null,"description":null}"#;
        let todo: Todo = serde_json::from_str(json).expect("old task");
        assert_eq!((todo.url, todo.categories, todo.sort_order, todo.link), (None, Vec::new(), None, None));
    }

    #[test]
    fn a_link_reads_without_details_and_round_trips_with_them() {
        let link: Link = serde_json::from_str(r#"{"id":"github:mahype/TorroWorkplace#123"}"#).expect("link");
        assert_eq!(link, Link::github("mahype/TorroWorkplace", 123));
        let details = GitHubDetails {
            kind: GitHubKind::PullRequest,
            state: GitHubState::Open,
            labels: vec![GitHubLabel { name: "bug".into(), color: Some("#D73A4A".into()) }],
            assignees: vec!["mahype".into()],
            milestone: Some("1.0".into()),
            milestone_due: NaiveDate::from_ymd_opt(2026, 11, 1),
            draft: true,
            review: Some(ReviewState::ChangesRequested),
            checks: Some(ChecksState::Failure),
            conflict: false,
        };
        let full = Link { github: Some(details), ..link };
        let json = serde_json::to_string(&full).expect("json");
        assert!(json.contains(r#""kind":"pull_request""#), "{json}");
        assert!(json.contains(r#""review":"changes_requested""#), "{json}");
        assert_eq!(serde_json::from_str::<Link>(&json).expect("back"), full);
        // Details from an older core, without the pull request fields.
        let short: GitHubDetails = serde_json::from_str(r#"{"kind":"issue","state":"closed"}"#).expect("short");
        assert_eq!((short.labels.len(), short.draft, short.review), (0, false, None));
    }

    #[test]
    fn github_ids_name_repository_number_and_page() {
        let link = Link::github("mahype/TorroWorkplace", 123);
        assert_eq!(link.id, "github:mahype/TorroWorkplace#123");
        assert_eq!(link.github_ref(), Some(("mahype/TorroWorkplace", 123)));
        assert_eq!(link.web_url().as_deref(), Some("https://github.com/mahype/TorroWorkplace/issues/123"));
        for other in [
            "jira:ABC-1",
            "github:mahype#1",
            "github:mahype/TorroWorkplace#x",
            "github:mahype/Torro Workplace#1",
            "github:/x#1",
            "github:a/b/c#1",
            "github:a/b?x#1",
            "github:a/b#-1",
        ] {
            let link = Link { id: other.into(), github: None };
            assert_eq!((link.github_ref(), link.web_url()), (None, None), "{other}");
        }
    }
}
