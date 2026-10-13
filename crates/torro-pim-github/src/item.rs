//! An issue or pull request as GitHub reports it ([`Item`]), and as a task of a filter list
//! ([`Item::to_todo`]).

use chrono::NaiveDate;
use torro_pim::Todo;
use torro_pim::model::{
    ChecksState, EventTime, GitHubDetails, GitHubKind, GitHubLabel, GitHubState, Link, ReviewState,
};

use crate::address::IssueRef;

/// One issue or pull request, read from GitHub's answer ([`crate::graphql`]). Only what a task
/// shows is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Repository (`owner/name`, as GitHub spells it) and number. The core decides by the
    /// repository whether a client may see the item at all, before anything else is read.
    pub id: IssueRef,
    pub kind: GitHubKind,
    pub state: GitHubState,
    pub title: String,
    /// The text in Markdown; empty when there is none.
    pub body: String,
    /// The item's page on github.com.
    pub url: String,
    /// When it last changed on GitHub (RFC 3339), the version of the item.
    pub updated_at: String,
    pub labels: Vec<GitHubLabel>,
    pub assignees: Vec<String>,
    pub milestone: Option<String>,
    pub milestone_due: Option<NaiveDate>,
    /// The issue this one is a sub-issue of.
    pub parent: Option<IssueRef>,
    pub draft: bool,
    pub review: Option<ReviewState>,
    pub checks: Option<ChecksState>,
    pub conflict: bool,
}

impl Item {
    /// `owner/name` of the item's repository.
    pub fn repo(&self) -> &str {
        &self.id.repo
    }

    /// The item as a task of the filter list `collection` (an id), at `position` in the list
    /// (`sort_order`, so "Manual order" shows GitHub's order):
    ///
    /// - uid: the link id (`github:<owner>/<repo>#<n>`), so the same item has the same uid in
    ///   every list; `href` and `url`: its page; `etag`: when it last changed;
    /// - title, text (empty: none), closed or merged = done, labels = `categories`;
    /// - a sub-issue's `parent` is the uid of its parent issue;
    /// - `due` is the due date of its milestone (`docs/github.md`: a milestone's date counts
    ///   while the item has none of its own, and filter lists have none);
    /// - `link` with everything GitHub reports ([`GitHubDetails`]).
    pub fn to_todo(&self, collection: &str, position: usize) -> Todo {
        let id = self.id.link_id();
        Todo {
            uid: id.clone(),
            collection: collection.to_string(),
            href: Some(self.url.clone()),
            etag: Some(self.updated_at.clone()),
            summary: self.title.clone(),
            due: self.milestone_due.map(EventTime::Date),
            priority: None,
            completed: self.state != GitHubState::Open,
            parent: self.parent.as_ref().map(IssueRef::link_id),
            description: Some(self.body.clone()).filter(|body| !body.trim().is_empty()),
            url: Some(self.url.clone()),
            categories: self.labels.iter().map(|label| label.name.clone()).collect(),
            sort_order: i64::try_from(position).ok(),
            link: Some(Link {
                id,
                github: Some(GitHubDetails {
                    kind: self.kind,
                    state: self.state,
                    labels: self.labels.clone(),
                    assignees: self.assignees.clone(),
                    milestone: self.milestone.clone(),
                    milestone_due: self.milestone_due,
                    draft: self.draft,
                    review: self.review,
                    checks: self.checks,
                    conflict: self.conflict,
                }),
                mail: None,
            }),
        }
    }
}

/// The matches of several searches and the pinned items as one list: every item once (the same
/// repository, whatever its case, and number), in the order of the first search, then the new
/// ones of the next, then the pinned ones that no search found.
pub fn union(parts: impl IntoIterator<Item = Vec<Item>>) -> Vec<Item> {
    let mut all: Vec<Item> = Vec::new();
    for part in parts {
        for item in part {
            if !all.iter().any(|known| known.id.same(&item.id)) {
                all.push(item);
            }
        }
    }
    all
}

/// The items as tasks of the list `collection`, in their order ([`Item::to_todo`]).
pub fn todos(items: &[Item], collection: &str) -> Vec<Todo> {
    items.iter().enumerate().map(|(position, item)| item.to_todo(collection, position)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::read_search;
    use crate::graphql::tests::{day, fixture};

    fn items(page: &str) -> Vec<Item> {
        read_search(&fixture(page)).expect("a page").items
    }

    #[test]
    fn an_issue_becomes_a_task_with_its_link() {
        let todo = items("search_page1")[0].to_todo("github:mahype/00ff", 0);
        assert_eq!(todo.uid, "github:mahype/TorroWorkplace#123");
        assert_eq!(todo.collection, "github:mahype/00ff");
        assert_eq!(todo.summary, "Calendar crashes on DST change");
        assert_eq!(todo.href.as_deref(), Some("https://github.com/mahype/TorroWorkplace/issues/123"));
        assert_eq!(todo.url, todo.href);
        assert_eq!(todo.etag.as_deref(), Some("2026-10-10T08:15:00Z"));
        assert!(!todo.completed);
        assert_eq!(todo.due, day("2026-11-01").map(EventTime::Date), "the milestone's date");
        assert_eq!(todo.parent.as_deref(), Some("github:mahype/TorroWorkplace#100"), "a sub-issue");
        assert_eq!(todo.categories, ["bug"]);
        assert!(todo.description.as_deref().is_some_and(|text| text.starts_with("Steps:")));
        assert_eq!(todo.sort_order, Some(0));
        let link = todo.link.expect("a link");
        assert_eq!(link.github_ref(), Some(("mahype/TorroWorkplace", 123)));
        let details = link.github.expect("details");
        assert_eq!((details.kind, details.state), (GitHubKind::Issue, GitHubState::Open));
        assert_eq!(details.labels[0].color.as_deref(), Some("#d73a4a"));
        assert_eq!((details.assignees, details.milestone.as_deref()), (vec!["mahype".to_string()], Some("1.0")));
    }

    #[test]
    fn a_pull_request_carries_draft_review_checks_and_conflict_and_is_done_when_merged() {
        let todo = items("search_page1")[1].to_todo("l", 1);
        assert_eq!((todo.description.clone(), todo.due, todo.sort_order), (None, None, Some(1)));
        let details = todo.link.and_then(|link| link.github).expect("details");
        assert_eq!(details.kind, GitHubKind::PullRequest);
        assert!(details.draft && details.conflict);
        assert_eq!((details.review, details.checks), (Some(ReviewState::ChangesRequested), Some(ChecksState::Failure)));

        let merged = items("search_page2")[0].to_todo("l", 0);
        assert!(merged.completed);
        assert_eq!(merged.link.and_then(|link| link.github).map(|d| d.state), Some(GitHubState::Merged));
    }

    #[test]
    fn several_searches_and_pinned_items_are_one_list_without_doubles() {
        let first = items("search_page1");
        let mut shouting = first[0].clone();
        shouting.id.repo = "MAHYPE/torroworkplace".into();
        let all = union([first.clone(), vec![shouting, items("search_page2")[0].clone()], vec![first[1].clone()]]);
        let ids: Vec<String> = all.iter().map(|item| item.id.link_id()).collect();
        assert_eq!(
            ids,
            ["github:mahype/TorroWorkplace#123", "github:mahype/TorroWorkplace#45", "github:mahype/other#44"]
        );
        let positions: Vec<Option<i64>> = todos(&all, "l").iter().map(|todo| todo.sort_order).collect();
        assert_eq!(positions, [Some(0), Some(1), Some(2)]);
    }
}
