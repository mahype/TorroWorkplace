//! Sample data in memory, for building and trying the apps without a server.
//!
//! `torrocalendar --demo` and `torrotasks --demo` use it. Changes live until the app
//! closes. The sample content (task K.2 in `docs/tasks/kalender-aufgaben.md`) is built
//! around "today", so the week view is never empty. It spreads over two accounts (task M1.1),
//! a work and a home Nextcloud, so the apps can show calendars and lists grouped by account.
//!
//! The list "Tasks" also holds GitHub issues and pull requests of `mahype/TorroWorkplace` as
//! linked tasks with what GitHub reports about them (task R.2 in `docs/tasks/design.md`): open
//! and closed issues, an open pull request with a review asked for and its checks running, a
//! draft and a merged one – so every state a row can show is there with `--demo`.
//!
//! A third account, the GitHub account `mahype`, holds two filter lists (task G.3), read-only
//! like the core's: "Needs me" (two searches, one pinned issue of another repository, more than
//! 1,000 matches) and "Torro bugs" (its last refresh from GitHub failed), so the sidebar's GitHub
//! group, the grouping by repository, the hints of a list and "Unpin" all show with `--demo`.
//!
//! Filter lists can be created, changed, deleted and tried out (task G.4). The demo cannot ask
//! GitHub; it searches the GitHub items of the sample instead and understands only a few
//! qualifiers ([`demo_matches`]), enough for the dialog's preview to show something real.

use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, Utc};

use crate::account::{GITHUB_KIND, account_of, local_id};
use crate::model::{ChecksState, GitHubDetails, GitHubFilter, GitHubKind, GitHubLabel, GitHubState, Link, ReviewState};
use crate::source::{FilterDefinition, FilterPreview, PimSource, move_plan};
use crate::{AccountState, Collection, CollectionKind, Event, EventTime, PimAccount, ProviderError, Todo};

#[derive(Debug, Default)]
struct Data {
    collections: Vec<Collection>,
    events: Vec<Event>,
    todos: Vec<Todo>,
    next_id: u64,
}

impl Data {
    /// `collection` is a known one that takes no changes: a filter list.
    fn read_only(&self, collection: &str) -> bool {
        self.collections.iter().any(|c| c.id == collection && !c.writable)
    }

    /// `tasks.save_todo` on the filter list `todo.collection` as the core answers it (task G.5),
    /// in memory: a changed `completed` of an issue "closes" or "reopens" it – every filter list
    /// holding it shows the new state, with a new version. A new task, a pull request, a stale
    /// version or any other change is refused as the core refuses it.
    fn tick_issue(&mut self, todo: Todo) -> Result<Todo, ProviderError> {
        let held = self.todos.iter().find(|t| t.collection == todo.collection && t.uid == todo.uid).cloned();
        let Some(held) = held.filter(|_| todo.href.is_some()) else {
            return Err(ProviderError::Other(READ_ONLY.into()));
        };
        let github = held.link.as_ref().and_then(|link| link.github.as_ref());
        if !github.is_some_and(|github| github.kind == GitHubKind::Issue) {
            return Err(ProviderError::Other(PULL_REQUESTS.into()));
        }
        if held.etag != todo.etag {
            return Err(ProviderError::Conflict { uid: todo.uid });
        }
        if (Todo { completed: held.completed, ..todo.clone() }) != held {
            return Err(ProviderError::Other(READ_ONLY.into()));
        }
        self.next_id += 1;
        let version = format!("\"demo-{}\"", self.next_id);
        let state = if todo.completed { GitHubState::Closed } else { GitHubState::Open };
        let lists: Vec<String> = self.collections.iter().filter(|c| c.source.is_some()).map(|c| c.id.clone()).collect();
        let mut saved = None;
        for copy in self.todos.iter_mut().filter(|t| t.uid == held.uid && lists.contains(&t.collection)) {
            copy.completed = todo.completed;
            copy.etag = Some(version.clone());
            if let Some(github) = copy.link.as_mut().and_then(|link| link.github.as_mut()) {
                github.state = state;
            }
            if copy.collection == todo.collection {
                saved = Some(copy.clone());
            }
        }
        saved.ok_or_else(|| ProviderError::Other(READ_ONLY.into()))
    }
}

/// What a change of a filter list's item other than ticking an issue is answered with; the core
/// answers `unsupported`.
const READ_ONLY: &str = "Of a GitHub issue in a filter list only whether it is done can be changed";
/// What ticking a pull request in a filter list is answered with (core: `unsupported`).
const PULL_REQUESTS: &str = "Pull requests are never closed or reopened here";
/// What deleting a filter list's item is answered with (core: `unsupported`).
const NOT_DELETED: &str = "The items of a GitHub filter list cannot be deleted; they live on GitHub";

/// What adding something that is no GitHub link is answered with (core: `error`).
const NO_LINK: &str = "Paste the address of a GitHub issue or pull request, or owner/repo#12";
/// What adding a GitHub item the sample does not have is answered with (core: `not-found`).
const UNKNOWN_ITEM: &str = "The demo knows only the GitHub items of its sample";

/// At most this many searches per filter list, as the core allows.
const MAX_QUERIES: usize = 10;
/// At most this many matches come with a preview, as from the core.
const PREVIEW_ITEMS: usize = 20;

/// Refuses a definition the core would refuse, with the core's messages.
fn check_definition(definition: &FilterDefinition) -> Result<(), ProviderError> {
    let refuse = |message: &str| Err(ProviderError::Other(message.into()));
    if definition.name.trim().is_empty() {
        return refuse("A filter list needs a name.");
    }
    let hex = |c: &str| c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|d| d.is_ascii_hexdigit());
    if definition.color.as_deref().is_some_and(|c| !hex(c)) {
        return refuse("A colour is written as #RRGGBB.");
    }
    match definition.queries.iter().filter(|q| !q.trim().is_empty()).count() {
        0 => refuse("A filter list needs at least one search."),
        n if n > MAX_QUERIES => refuse("A filter list may run at most 10 searches."),
        _ => Ok(()),
    }
}

/// Whether the GitHub item `todo` matches the search `query`, as far as the demo understands it:
/// `is:open`, `is:closed`, `is:merged`, `is:issue`, `is:pr` (and `type:`), `repo:`, `label:` (a
/// comma for any of several), `-label:`, `assignee:`, `review-requested:` and `milestone:`.
/// Everything else (`involves:@me`, `author:`, words) is let through: the demo knows no authors.
fn demo_match(todo: &Todo, query: &str) -> bool {
    let Some(link) = todo.link.as_ref() else { return false };
    let (Some(details), Some((repo, _))) = (link.github.as_ref(), link.github_ref()) else { return false };
    let me = |who: &str| if who == "@me" { GITHUB_LOGIN.to_string() } else { who.to_lowercase() };
    let labelled = |names: &str| {
        names.split(',').any(|name| details.labels.iter().any(|l| l.name.eq_ignore_ascii_case(name.trim_matches('"'))))
    };
    query.split_whitespace().all(|word| {
        let word = word.to_lowercase();
        match word.split_once(':') {
            Some(("is" | "state", "open")) => details.state == GitHubState::Open,
            Some(("is" | "state", "closed")) => details.state != GitHubState::Open,
            Some(("is", "merged")) => details.state == GitHubState::Merged,
            Some(("is" | "type", "issue")) => details.kind == GitHubKind::Issue,
            Some(("is" | "type", "pr")) => details.kind == GitHubKind::PullRequest,
            Some(("repo", name)) => repo.eq_ignore_ascii_case(name),
            Some(("label", names)) => labelled(names),
            Some(("-label", names)) => !labelled(names),
            Some(("assignee", who)) => details.assignees.iter().any(|a| a.to_lowercase() == me(who)),
            Some(("review-requested", _)) => details.review == Some(ReviewState::Required),
            Some(("milestone", name)) => {
                details.milestone.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(name.trim_matches('"')))
            }
            _ => true,
        }
    })
}

/// The GitHub items of the sample that `query` finds ([`demo_match`]), each once, as tasks of no
/// list. An address cannot be read without the core.
fn demo_matches(todos: &[Todo], query: &str) -> Result<Vec<Todo>, ProviderError> {
    let lower = query.trim().to_lowercase();
    if lower.starts_with("http") || lower.starts_with("github.com/") || lower.starts_with("www.github.com/") {
        return Err(ProviderError::Other("The demo cannot read addresses; paste a search instead.".into()));
    }
    let mut found: Vec<Todo> = Vec::new();
    for todo in todos.iter().filter(|t| demo_match(t, query)) {
        let Some(link) = todo.link.as_ref() else { continue };
        if !found.iter().any(|f| f.uid == link.id) {
            found.push(Todo {
                uid: link.id.clone(),
                collection: String::new(),
                href: todo.url.clone(),
                ..todo.clone()
            });
        }
    }
    Ok(found)
}

impl Data {
    /// The items of a filter list with `queries` and `pinned`, as the core keeps them in list
    /// `id`: what any search finds, plus the pinned items, each once.
    fn filter_items(&self, id: &str, queries: &[String], pinned: &[String]) -> Result<Vec<Todo>, ProviderError> {
        let mut items: Vec<Todo> = Vec::new();
        for query in queries {
            for item in demo_matches(&self.todos, query)? {
                if !items.iter().any(|i| i.uid == item.uid) {
                    items.push(item);
                }
            }
        }
        let kept = self.todos.iter().filter(|t| t.collection == id && pinned.contains(&t.uid));
        for item in kept {
            if !items.iter().any(|i| i.uid == item.uid) {
                items.push(item.clone());
            }
        }
        Ok(items
            .into_iter()
            .enumerate()
            .map(|(n, item)| Todo {
                collection: id.into(),
                etag: Some(format!("demo-{n}")),
                sort_order: i64::try_from(n).ok(),
                ..item
            })
            .collect())
    }
}

/// See the module docs.
#[derive(Debug, Default)]
pub struct DemoSource {
    data: Mutex<Data>,
}

impl DemoSource {
    /// Sample calendars, events and tasks around `today`.
    pub fn new(today: chrono::NaiveDate) -> DemoSource {
        let (collections, events, todos) = sample(today);
        DemoSource { data: Mutex::new(Data { collections, events, todos, next_id: 0 }) }
    }

    fn lock(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The work account: the calendar "Work" and the list "Tasks".
pub const WORK_ACCOUNT: &str = "nextcloud:anna@work.example.org";
/// The home account: "Personal", "Groceries" and "Family".
pub const HOME_ACCOUNT: &str = "nextcloud:anna@home.example.org";
/// Both, in the order [`DemoSource::accounts`] lists them.
const ACCOUNTS: [&str; 2] = [WORK_ACCOUNT, HOME_ACCOUNT];
/// The GitHub account with the filter lists "Needs me" and "Torro bugs", listed after the two
/// Nextcloud accounts.
pub const GITHUB_ACCOUNT: &str = "github:mahype";
/// Its login, the account's name.
const GITHUB_LOGIN: &str = "mahype";

// Filter list ids as the core writes them: `github:<login>/<list-id>`.
/// "Needs me": more than 1,000 matches, one pinned issue.
pub const NEEDS_ME: &str = "github:mahype/needs-me";
/// "Torro bugs": the last refresh from GitHub failed.
pub const TORRO_BUGS: &str = "github:mahype/torro-bugs";

// Collection ids as the core writes them: `<account-id>/<collection URL>` (`crate::account`).
const WORK: &str = "nextcloud:anna@work.example.org/https://work.example.org/remote.php/dav/calendars/anna/work/";
const TASKS: &str = "nextcloud:anna@work.example.org/https://work.example.org/remote.php/dav/calendars/anna/tasks/";
const PERSONAL: &str =
    "nextcloud:anna@home.example.org/https://home.example.org/remote.php/dav/calendars/anna/personal/";
const GROCERIES: &str =
    "nextcloud:anna@home.example.org/https://home.example.org/remote.php/dav/calendars/anna/groceries/";
const FAMILY: &str = "nextcloud:anna@home.example.org/https://home.example.org/remote.php/dav/calendars/anna/family/";

/// The collection URL in a demo collection id, which item hrefs start with; an id of no demo
/// account (tests add their own) is its own URL.
fn url_of(collection: &str) -> &str {
    account_of(collection, &ACCOUNTS, |id| id).and_then(|account| local_id(collection, account)).unwrap_or(collection)
}

/// The sample content: five collections in two Nextcloud accounts and two filter lists of the
/// GitHub account, 27 events for this week and the two after it, and 30 tasks (six GitHub items
/// in "Tasks", six in the filter lists). Dates are relative to `today`; a week runs Monday to
/// Sunday.
pub fn sample(today: NaiveDate) -> (Vec<Collection>, Vec<Event>, Vec<Todo>) {
    let monday = today - Days::new(u64::from(today.weekday().num_days_from_monday()));
    let collections = vec![
        collection(WORK, "Work", CollectionKind::Events, "#3474E6"),
        collection(PERSONAL, "Personal", CollectionKind::Events, "#119C8E"),
        collection(TASKS, "Tasks", CollectionKind::Todos, "#4F9426"),
        collection(GROCERIES, "Groceries", CollectionKind::Todos, "#7A57E3"),
        collection(FAMILY, "Family", CollectionKind::Both, "#E0485D"),
        filter_list(
            NEEDS_ME,
            "Needs me",
            "#D96CA8",
            GitHubFilter {
                queries: vec!["is:open involves:@me".into(), "is:open is:pr review-requested:@me".into()],
                pinned: vec![Link::github(DESIGN_REPO, 18).id],
                revision: 1,
                truncated: true,
                problem: None,
            },
        ),
        filter_list(
            TORRO_BUGS,
            "Torro bugs",
            "#E8A33D",
            GitHubFilter {
                queries: vec![format!("is:issue repo:{REPO} label:bug")],
                pinned: Vec::new(),
                revision: 3,
                truncated: false,
                problem: Some("GitHub did not answer in time.".into()),
            },
        ),
    ];
    let mut events = sample_events(monday);
    for (n, item) in events.iter_mut().enumerate() {
        item.href = Some(format!("{}evt-{}.ics", url_of(&item.collection), n + 1));
        item.etag = Some(format!("\"sample-evt-{}\"", n + 1));
    }
    let mut todos = sample_todos(today);
    for (n, item) in todos.iter_mut().enumerate() {
        item.href = Some(format!("{}todo-{}.ics", url_of(&item.collection), n + 1));
        item.etag = Some(format!("\"sample-todo-{}\"", n + 1));
    }
    // The core keeps a match where GitHub has it: its page is the href, its last update the etag.
    todos.extend(filter_todos(today).into_iter().enumerate().map(|(n, item)| Todo {
        href: item.url.clone(),
        etag: Some(format!("2026-10-0{}T12:00:00Z", n + 1)),
        sort_order: i64::try_from(n).ok(),
        ..item
    }));
    (collections, events, todos)
}

fn collection(id: &str, name: &str, kind: CollectionKind, color: &str) -> Collection {
    Collection { id: id.into(), name: name.into(), kind, color: Some(color.into()), writable: true, source: None }
}

/// A filter list of the GitHub account, as the core lists it: task items only, never writable.
fn filter_list(id: &str, name: &str, color: &str, source: GitHubFilter) -> Collection {
    Collection {
        id: id.into(),
        name: name.into(),
        kind: CollectionKind::Todos,
        color: Some(color.into()),
        writable: false,
        source: Some(source),
    }
}

/// Local wall-clock time on `day` as an instant. The sample avoids the hour that a DST
/// change skips.
fn at(day: NaiveDate, hour: u32, minute: u32) -> EventTime {
    let local = day.and_hms_opt(hour, minute, 0).and_then(|naive| naive.and_local_timezone(Local).earliest());
    EventTime::DateTime(local.map(|time| time.with_timezone(&Utc)).unwrap_or_default())
}

/// An event without href, etag and details. Set the rest with struct update syntax.
fn event(collection: &str, uid: &str, summary: &str, start: EventTime, end: EventTime) -> Event {
    Event {
        uid: uid.into(),
        collection: collection.into(),
        href: None,
        etag: None,
        summary: summary.into(),
        start,
        end: Some(end),
        location: None,
        description: None,
        recurring: false,
        transparency: crate::Transparency::Opaque,
    }
}

/// A task without href, etag and details. Set the rest with struct update syntax.
fn todo(collection: &str, uid: &str, summary: &str, due: Option<EventTime>) -> Todo {
    Todo {
        uid: uid.into(),
        collection: collection.into(),
        href: None,
        etag: None,
        summary: summary.into(),
        due,
        priority: None,
        completed: false,
        parent: None,
        description: None,
        url: None,
        categories: Vec::new(),
        sort_order: None,
        link: None,
    }
}

/// The repository the GitHub items of the sample belong to.
const REPO: &str = "mahype/TorroWorkplace";

/// A task in "Tasks" that stands for issue or pull request `number` of [`REPO`], with what GitHub
/// reports about it: `kind`, `state` and the `labels` as `(name, colour)`. The labels are the
/// task's tags as well, as the core keeps them. Set the rest with struct update syntax, the
/// details with [`with_details`].
fn github(
    uid: &str,
    number: u64,
    (kind, state): (GitHubKind, GitHubState),
    labels: &[(&str, &str)],
    summary: &str,
    due: Option<EventTime>,
) -> Todo {
    let details = GitHubDetails {
        kind,
        state,
        labels: labels
            .iter()
            .map(|(name, color)| GitHubLabel { name: (*name).into(), color: Some((*color).into()) })
            .collect(),
        assignees: Vec::new(),
        milestone: None,
        milestone_due: None,
        draft: false,
        review: None,
        checks: None,
        conflict: false,
    };
    let link = Link { github: Some(details), ..Link::github(REPO, number) };
    Todo {
        completed: state != GitHubState::Open,
        url: link.web_url(),
        categories: labels.iter().map(|(name, _)| (*name).to_string()).collect(),
        link: Some(link),
        ..todo(TASKS, uid, summary, due)
    }
}

/// `todo` with its GitHub details changed by `change`.
fn with_details(mut todo: Todo, change: impl FnOnce(&mut GitHubDetails)) -> Todo {
    if let Some(details) = todo.link.as_mut().and_then(|link| link.github.as_mut()) {
        change(details);
    }
    todo
}

/// The design repository, whose one issue is pinned in "Needs me".
const DESIGN_REPO: &str = "mahype/torro-design";

/// A match of the filter list `list` as the core keeps it ([`github`] otherwise): issue or pull
/// request `number` of `repo`, its link id as uid, without a due date. Href and etag come in
/// [`sample`].
fn found(
    list: &str,
    repo: &str,
    number: u64,
    kind_state: (GitHubKind, GitHubState),
    labels: &[(&str, &str)],
    summary: &str,
) -> Todo {
    let item = github("", number, kind_state, labels, summary, None);
    let details = item.link.clone().and_then(|link| link.github);
    let link = Link { github: details, ..Link::github(repo, number) };
    Todo { uid: link.id.clone(), collection: list.into(), url: link.web_url(), link: Some(link), ..item }
}

/// `todo` in the milestone `name` due on `day`: the core takes its date as the item's due date.
fn in_milestone(todo: Todo, name: &str, day: NaiveDate) -> Todo {
    let todo = Todo { due: Some(EventTime::Date(day)), ..todo };
    with_details(todo, |details| {
        details.milestone = Some(name.into());
        details.milestone_due = Some(day);
    })
}

/// The text of the pinned design issue, in Markdown as GitHub delivers it (with the comment of an
/// issue template): the panel shows it formatted (task G.7, `--demo --open pinned`).
const ISSUE_18_BODY: &str = "<!-- What should change, and why? -->
## Summary

The Iced apps copy the colour tokens by hand. Export them from `tokens.json` so **one file** stays the source.

## Tasks

- [x] List the tokens the apps use
- [ ] Generate `theme.rs` from `tokens.json`
- [ ] Check contrast in light and dark

```sh
cargo run -p torro-design -- export --format rust
```

> Light and dark must stay in step. See the [token reference](https://example.org/tokens).";

/// The matches of the two filter lists. "Needs me": a pull request waiting for a review, an
/// issue assigned to me in a milestone, and the pinned issue of the design repository. "Torro
/// bugs": two open bugs (one in a milestone) and a closed one.
fn filter_todos(today: NaiveDate) -> Vec<Todo> {
    let (issue, pull) = (GitHubKind::Issue, GitHubKind::PullRequest);
    let (open, closed) = (GitHubState::Open, GitHubState::Closed);
    let (bug, ui, linux, enhancement) =
        (("bug", "#D73A4A"), ("ui", "#3B82F6"), ("linux", "#D4A72C"), ("enhancement", "#2DA44E"));
    let milestone = today + Days::new(4);
    let mine = |details: &mut GitHubDetails| details.assignees = vec![GITHUB_LOGIN.into()];
    vec![
        with_details(
            found(NEEDS_ME, REPO, 138, (pull, open), &[ui], "Show filter lists in the tasks sidebar"),
            |details| {
                details.review = Some(ReviewState::Required);
                details.checks = Some(ChecksState::Pending);
            },
        ),
        with_details(
            in_milestone(
                found(NEEDS_ME, REPO, 142, (issue, open), &[bug], "Calendar treats a GitHub-only setup as connected"),
                "v0.4",
                milestone,
            ),
            mine,
        ),
        Todo {
            description: Some(ISSUE_18_BODY.into()),
            ..found(NEEDS_ME, DESIGN_REPO, 18, (issue, open), &[enhancement], "Export the colour tokens for Iced")
        },
        with_details(
            in_milestone(
                found(
                    TORRO_BUGS,
                    REPO,
                    123,
                    (issue, open),
                    &[bug, ui, linux],
                    "Tray icon disappears after resuming from sleep",
                ),
                "v0.4",
                milestone,
            ),
            mine,
        ),
        found(TORRO_BUGS, REPO, 145, (issue, open), &[bug, ui], "Week view: all-day events cover the day names"),
        found(TORRO_BUGS, REPO, 119, (issue, closed), &[bug], "Week view jumps when the clock changes"),
    ]
}

/// The GitHub items of the sample, in "Tasks": an overdue open issue, an open pull request due
/// today with a review asked for and its checks running, a merged one, a draft, an issue for
/// later and a closed one.
fn github_todos(today: NaiveDate) -> Vec<Todo> {
    let before = |offset: u64| today - Days::new(offset);
    let after = |offset: u64| today + Days::new(offset);
    let (issue, pull) = (GitHubKind::Issue, GitHubKind::PullRequest);
    let (bug, ui, linux, enhancement) =
        (("bug", "#D73A4A"), ("ui", "#3B82F6"), ("linux", "#D4A72C"), ("enhancement", "#2DA44E"));
    vec![
        with_details(
            Todo {
                priority: Some(3),
                description: Some(
                    "After suspend and resume the tray icon is gone until torroworkplace restarts.".into(),
                ),
                ..github(
                    "github-issue@torro-demo",
                    123,
                    (issue, GitHubState::Open),
                    &[bug, ui, linux],
                    "Tray icon disappears after resuming from sleep",
                    Some(EventTime::Date(before(1))),
                )
            },
            |details| {
                details.assignees = vec!["mahype".into()];
                details.milestone = Some("v0.4".into());
            },
        ),
        with_details(
            Todo {
                description: Some("Shows the saved workspace templates directly in the tray menu.".into()),
                ..github(
                    "github-pr-131@torro-demo",
                    131,
                    (pull, GitHubState::Open),
                    &[ui],
                    "Add workspace templates to the tray menu",
                    Some(at(today, 17, 0)),
                )
            },
            |details| {
                details.review = Some(ReviewState::Required);
                details.checks = Some(ChecksState::Pending);
                details.assignees = vec!["anna".into()];
                details.milestone = Some("v0.4".into());
            },
        ),
        with_details(
            github(
                "github-pr-127@torro-demo",
                127,
                (pull, GitHubState::Merged),
                &[linux],
                "Parse Hyprland clients with the new Lua API",
                Some(EventTime::Date(today)),
            ),
            |details| details.checks = Some(ChecksState::Success),
        ),
        with_details(
            github(
                "github-pr-134@torro-demo",
                134,
                (pull, GitHubState::Open),
                &[ui],
                "Sidebar rail: show list names as tooltips",
                Some(EventTime::Date(after(3))),
            ),
            |details| {
                details.draft = true;
                details.checks = Some(ChecksState::Success);
            },
        ),
        github(
            "github-issue-125@torro-demo",
            125,
            (issue, GitHubState::Open),
            &[ui, enhancement],
            "Display menu: add “Sync now” and the last sync time",
            Some(EventTime::Date(after(9))),
        ),
        github(
            "github-issue-119@torro-demo",
            119,
            (issue, GitHubState::Closed),
            &[bug],
            "Week view jumps when the clock changes",
            Some(EventTime::Date(before(5))),
        ),
    ]
}

/// Events from Monday to Sunday of this week and of the two weeks after it.
fn sample_events(monday: NaiveDate) -> Vec<Event> {
    let day = |offset: u64| monday + Days::new(offset);
    let mut events = Vec::new();

    // The daily standup is one series: the same uid, one occurrence per weekday, read-only.
    for offset in [0, 1, 2, 3, 4, 7, 8, 9, 10, 11] {
        events.push(Event {
            recurring: true,
            ..event(WORK, "daily-standup@torro-demo", "Daily Standup", at(day(offset), 9, 30), at(day(offset), 9, 45))
        });
    }

    events.extend([
        Event {
            location: Some("Meeting room Elbe".into()),
            description: Some("Agree on goals and scope for the next sprint.".into()),
            ..event(WORK, "sprint-planning@torro-demo", "Sprint planning", at(day(0), 10, 0), at(day(0), 11, 30))
        },
        Event {
            location: Some("Room Rhine".into()),
            description: Some("Go through the booking API endpoints with the team.".into()),
            ..event(WORK, "api-workshop@torro-demo", "API design workshop", at(day(1), 13, 0), at(day(1), 15, 0))
        },
        // Overlaps the workshop above.
        Event {
            location: Some("Office 2.14".into()),
            ..event(WORK, "one-on-one-lena@torro-demo", "1:1 with Lena", at(day(1), 14, 30), at(day(1), 15, 0))
        },
        Event {
            location: Some("Canteen North".into()),
            ..event(WORK, "lunch-jana@torro-demo", "Lunch with Jana", at(day(2), 12, 30), at(day(2), 13, 30))
        },
        Event {
            location: Some("Video call".into()),
            description: Some("Interim status of the pilot project with the city council.".into()),
            ..event(WORK, "client-presentation@torro-demo", "Client presentation", at(day(3), 15, 0), at(day(3), 16, 0))
        },
        Event {
            location: Some("Room Elbe".into()),
            ..event(WORK, "retro@torro-demo", "Retrospective", at(day(4), 15, 0), at(day(4), 16, 0))
        },
        // Crosses midnight.
        Event {
            location: Some("Video call".into()),
            description: Some("Deploy version 2.4, then on call.".into()),
            ..event(WORK, "release-call@torro-demo", "Release call", at(day(4), 22, 30), at(day(5), 0, 30))
        },
        Event {
            location: Some("Room Rhine".into()),
            description: Some("Status of the pilot and dates for the autumn.".into()),
            ..event(WORK, "project-review@torro-demo", "Project review", at(day(9), 9, 0), at(day(9), 10, 0))
        },
        Event {
            location: Some("Room Elbe".into()),
            description: Some("Figures for the third quarter and outlook.".into()),
            ..event(WORK, "quarterly-review@torro-demo", "Quarterly review", at(day(16), 10, 0), at(day(16), 12, 0))
        },
        Event {
            location: Some("City park".into()),
            description: Some("Easy eight kilometres.".into()),
            ..event(PERSONAL, "run@torro-demo", "Run", at(day(1), 18, 30), at(day(1), 19, 30))
        },
        Event {
            location: Some("Indoor pool North".into()),
            ..event(PERSONAL, "swimming@torro-demo", "Swimming", at(day(3), 19, 0), at(day(3), 20, 0))
        },
        Event {
            location: Some("Forest sports field".into()),
            description: Some("Team training.".into()),
            ..event(PERSONAL, "football@torro-demo", "Football", at(day(5), 9, 30), at(day(5), 11, 0))
        },
        Event {
            location: Some("Dental practice Berger".into()),
            ..event(PERSONAL, "dentist@torro-demo", "Dentist", at(day(8), 8, 0), at(day(8), 8, 45))
        },
        event(FAMILY, "birthday-mum@torro-demo", "Mum's birthday", EventTime::Date(day(6)), EventTime::Date(day(7))),
        // All-day with an exclusive end: Tuesday to Thursday of next week.
        Event {
            location: Some("Baltic Sea".into()),
            description: Some("Holiday house in Kühlungsborn, arriving Tuesday morning.".into()),
            ..event(FAMILY, "vacation-baltic@torro-demo", "Vacation", EventTime::Date(day(8)), EventTime::Date(day(11)))
        },
        Event {
            location: Some("Grandma's".into()),
            description: Some("Grandma cooks, we bring dessert.".into()),
            ..event(FAMILY, "family-dinner@torro-demo", "Family dinner", at(day(13), 12, 0), at(day(13), 15, 0))
        },
        Event {
            location: Some("Kindergarten Sunshine".into()),
            description: Some("Topic: settling in for the new kindergarten year.".into()),
            ..event(
                FAMILY,
                "kindergarten-evening@torro-demo",
                "Kindergarten parents' evening",
                at(day(15), 19, 30),
                at(day(15), 21, 0),
            )
        },
    ]);
    events
}

/// Tasks due before, on and after `today` and some without a date, three parents with three
/// and two subtasks, three completed tasks, one with tags, and the GitHub items of
/// [`github_todos`].
fn sample_todos(today: NaiveDate) -> Vec<Todo> {
    let before = |offset: u64| today - Days::new(offset);
    let after = |offset: u64| today + Days::new(offset);
    const QUARTERLY: &str = "quarterly-report@torro-demo";
    const WEEKLY_SHOP: &str = "weekly-shop@torro-demo";
    const VACATION: &str = "prepare-vacation@torro-demo";

    let mut todos = vec![
        Todo {
            priority: Some(1),
            description: Some("Sort receipts for 2025 by category, keep trade invoices separate.".into()),
            categories: vec!["Finance".into(), "Paperwork".into()],
            ..todo(TASKS, "tax-documents@torro-demo", "Sort tax documents", Some(at(before(4), 18, 0)))
        },
        Todo {
            priority: Some(5),
            ..todo(TASKS, "invoices@torro-demo", "File invoices", Some(EventTime::Date(before(2))))
        },
        Todo {
            priority: Some(9),
            ..todo(TASKS, "bike@torro-demo", "Call the bike workshop", Some(EventTime::Date(after(3))))
        },
        Todo {
            priority: Some(5),
            description: Some("Outline is done, figures still missing.".into()),
            ..todo(TASKS, QUARTERLY, "Write quarterly report", Some(EventTime::Date(after(9))))
        },
        Todo {
            parent: Some(QUARTERLY.into()),
            completed: true,
            ..todo(TASKS, "quarterly-figures@torro-demo", "Pull figures from the CRM", Some(EventTime::Date(before(3))))
        },
        Todo {
            parent: Some(QUARTERLY.into()),
            ..todo(TASKS, "quarterly-outlook@torro-demo", "Write outlook", Some(EventTime::Date(after(7))))
        },
        Todo {
            parent: Some(QUARTERLY.into()),
            ..todo(TASKS, "quarterly-charts@torro-demo", "Insert charts", Some(EventTime::Date(after(8))))
        },
        Todo { completed: true, ..todo(TASKS, "passport@torro-demo", "Apply for passport", None) },
        todo(GROCERIES, "oats@torro-demo", "Oats", None),
        Todo { priority: Some(5), ..todo(GROCERIES, WEEKLY_SHOP, "Weekly shop", Some(EventTime::Date(after(1)))) },
        Todo {
            parent: Some(WEEKLY_SHOP.into()),
            ..todo(GROCERIES, "groceries-produce@torro-demo", "Vegetables and fruit", None)
        },
        Todo {
            parent: Some(WEEKLY_SHOP.into()),
            ..todo(GROCERIES, "groceries-bread@torro-demo", "Wholegrain bread", None)
        },
        Todo { parent: Some(WEEKLY_SHOP.into()), ..todo(GROCERIES, "groceries-yogurt@torro-demo", "Yogurt", None) },
        Todo { priority: Some(2), ..todo(FAMILY, "gift-mum@torro-demo", "Buy Mum's present", Some(at(today, 18, 0))) },
        Todo {
            priority: Some(5),
            description: Some("Sort everything out before the holiday house.".into()),
            ..todo(FAMILY, VACATION, "Prepare vacation", Some(EventTime::Date(after(1))))
        },
        Todo {
            parent: Some(VACATION.into()),
            completed: true,
            ..todo(FAMILY, "vacation-house@torro-demo", "Confirm holiday house", None)
        },
        Todo {
            parent: Some(VACATION.into()),
            ..todo(FAMILY, "vacation-tickets@torro-demo", "Book train tickets", Some(EventTime::Date(today)))
        },
        todo(FAMILY, "pediatrician@torro-demo", "Book pediatrician appointment", Some(EventTime::Date(after(10)))),
    ];
    todos.extend(github_todos(today));
    todos
}

/// Start and end of an event as UTC instants; all-day dates count from local midnight.
fn span(event: &Event) -> (DateTime<Utc>, DateTime<Utc>) {
    let instant = |time: EventTime| match time {
        EventTime::DateTime(at) => at,
        EventTime::Date(date) => date
            .and_hms_opt(0, 0, 0)
            .and_then(|naive| naive.and_local_timezone(Local).earliest())
            .map(|local| local.with_timezone(&Utc))
            .unwrap_or_default(),
    };
    let start = instant(event.start);
    let end = event.end.map(instant).unwrap_or(start);
    (start, end)
}

impl PimSource for DemoSource {
    /// The work and the home account and the GitHub account, all connected and just synced.
    async fn accounts(&self) -> Result<Vec<PimAccount>, ProviderError> {
        let github = PimAccount {
            id: GITHUB_ACCOUNT.into(),
            label: GITHUB_LOGIN.into(),
            kind: GITHUB_KIND.into(),
            state: AccountState::Connected {
                server: "https://github.com".into(),
                login_name: GITHUB_LOGIN.into(),
                last_sync: Some(Utc::now()),
                problem: None,
            },
            // A token that may write issues, so ticking one closes it (task G.5).
            scopes: Some(vec!["repo".into(), "read:org".into()]),
        };
        Ok(ACCOUNTS
            .iter()
            .map(|id| {
                let host = id.split_once('@').map_or(*id, |(_, host)| host);
                PimAccount {
                    id: (*id).into(),
                    label: format!("anna@{host}"),
                    kind: "nextcloud".into(),
                    state: AccountState::Connected {
                        server: format!("https://{host}"),
                        login_name: "anna".into(),
                        last_sync: Some(Utc::now()),
                        problem: None,
                    },
                    scopes: None,
                }
            })
            .chain([github])
            .collect())
    }

    async fn connect_nextcloud(&self, server: String) -> Result<String, ProviderError> {
        Err(ProviderError::Other(format!("Demo mode cannot connect to {server}")))
    }

    async fn sync(&self) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn collections(&self) -> Result<Vec<Collection>, ProviderError> {
        Ok(self.lock().collections.clone())
    }

    async fn events(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Event>, ProviderError> {
        Ok(self
            .lock()
            .events
            .iter()
            .filter(|e| {
                let (s, e_end) = span(e);
                s < end && (e_end > start || (e_end == s && s >= start))
            })
            .cloned()
            .collect())
    }

    async fn todos(&self) -> Result<Vec<Todo>, ProviderError> {
        Ok(self.lock().todos.clone())
    }

    async fn save_event(&self, mut event: Event) -> Result<Event, ProviderError> {
        let mut data = self.lock();
        data.next_id += 1;
        let version = format!("\"demo-{}\"", data.next_id);
        match event.href.clone() {
            None => {
                if event.uid.is_empty() {
                    event.uid = format!("demo-event-{}", data.next_id);
                }
                event.href = Some(format!("{}{}.ics", url_of(&event.collection), event.uid));
                event.etag = Some(version);
                data.events.push(event.clone());
            }
            Some(href) => {
                let Some(slot) = data.events.iter_mut().find(|e| e.href.as_deref() == Some(href.as_str())) else {
                    return Err(ProviderError::Other(format!("No event at {href}")));
                };
                if slot.etag != event.etag {
                    return Err(ProviderError::Conflict { uid: event.uid });
                }
                event.etag = Some(version);
                *slot = event.clone();
            }
        }
        Ok(event)
    }

    async fn delete_event(&self, event: Event) -> Result<(), ProviderError> {
        let mut data = self.lock();
        let before = data.events.len();
        data.events.retain(|e| e.href != event.href || e.uid != event.uid);
        if data.events.len() == before { Err(ProviderError::Other("No such event".into())) } else { Ok(()) }
    }

    /// Like the core, a filter list takes no changes but the tick of an issue, which closes or
    /// reopens it ([`Data::tick_issue`]).
    async fn save_todo(&self, mut todo: Todo) -> Result<Todo, ProviderError> {
        let mut data = self.lock();
        if data.read_only(&todo.collection) {
            return data.tick_issue(todo);
        }
        data.next_id += 1;
        let version = format!("\"demo-{}\"", data.next_id);
        match todo.href.clone() {
            None => {
                if todo.uid.is_empty() {
                    todo.uid = format!("demo-todo-{}", data.next_id);
                }
                todo.href = Some(format!("{}{}.ics", url_of(&todo.collection), todo.uid));
                todo.etag = Some(version);
                data.todos.push(todo.clone());
            }
            Some(href) => {
                let Some(slot) = data.todos.iter_mut().find(|t| t.href.as_deref() == Some(href.as_str())) else {
                    return Err(ProviderError::Other(format!("No task at {href}")));
                };
                if slot.etag != todo.etag {
                    return Err(ProviderError::Conflict { uid: todo.uid });
                }
                // Ticking a link to an issue closes the issue too, as the core does (task G.6).
                let issue = slot.link.as_ref().and_then(|link| link.github.as_ref()).map(|github| github.kind);
                if issue == Some(GitHubKind::Issue)
                    && slot.completed != todo.completed
                    && let Some(github) = todo.link.as_mut().and_then(|link| link.github.as_mut())
                {
                    github.state = if todo.completed { GitHubState::Closed } else { GitHubState::Open };
                }
                todo.etag = Some(version);
                *slot = todo.clone();
            }
        }
        Ok(todo)
    }

    async fn delete_todo(&self, todo: Todo) -> Result<(), ProviderError> {
        let mut data = self.lock();
        if data.read_only(&todo.collection) {
            return Err(ProviderError::Other(NOT_DELETED.into()));
        }
        let before = data.todos.len();
        data.todos.retain(|t| t.href != todo.href || t.uid != todo.uid);
        if data.todos.len() == before { Err(ProviderError::Other("No such task".into())) } else { Ok(()) }
    }

    /// Renumbers like the core ([`move_plan`]) and stores every changed task with a new etag. In
    /// memory nothing fails halfway, so the move happens whole or not at all.
    async fn move_todo(&self, todo: Todo, before: Option<Todo>) -> Result<Vec<Todo>, ProviderError> {
        let mut data = self.lock();
        let stored = |href: &Option<String>| data.todos.iter().find(|t| href.is_some() && t.href == *href).cloned();
        let Some(current) = stored(&todo.href) else {
            return Err(ProviderError::Other("No such task".into()));
        };
        if current.etag != todo.etag {
            return Err(ProviderError::Conflict { uid: todo.uid });
        }
        let before = match before {
            Some(before) => match stored(&before.href) {
                Some(found) if found.collection == current.collection => Some(found),
                _ => return Err(ProviderError::Other("No such task in this list".into())),
            },
            None => None,
        };
        let plan = move_plan(&data.todos, &current, before.as_ref())
            .ok_or_else(|| ProviderError::Other("The tasks are not on the same level".into()))?;
        let mut written = Vec::with_capacity(plan.len());
        for mut task in plan {
            data.next_id += 1;
            task.etag = Some(format!("\"demo-{}\"", data.next_id));
            if let Some(slot) = data.todos.iter_mut().find(|t| t.href == task.href) {
                *slot = task.clone();
            }
            written.push(task);
        }
        Ok(written)
    }

    /// Takes the pin out of the list's definition. The sample's searches find none of the pinned
    /// items, so the item leaves the list at once, as it would with the core's next refresh.
    async fn unpin_issue(&self, collection: String, item: String) -> Result<(), ProviderError> {
        let mut data = self.lock();
        let Some(filter) = data.collections.iter_mut().find(|c| c.id == collection).and_then(|c| c.source.as_mut())
        else {
            return Err(ProviderError::Other(format!("No filter list {collection}")));
        };
        let names_item = |id: &String| {
            *id == item || Link { id: id.clone(), github: None }.web_url().is_some_and(|page| page == item)
        };
        let Some(at) = filter.pinned.iter().position(names_item) else {
            return Err(ProviderError::Other(format!("The list does not pin {item}")));
        };
        let id = filter.pinned.remove(at);
        data.todos.retain(|t| t.collection != collection || t.link.as_ref().is_none_or(|link| link.id != id));
        Ok(())
    }

    /// Like the core (task G.6), for the GitHub items of the sample only – the demo cannot ask
    /// GitHub: pinned in a filter list, a new link task in a writable list; an item the list holds
    /// already is the answer.
    async fn add_link(&self, collection: String, url: String) -> Result<Todo, ProviderError> {
        let link = Link::pasted(&url).ok_or_else(|| ProviderError::Other(NO_LINK.into()))?;
        let mut data = self.lock();
        let same = |t: &Todo| t.link.as_ref().is_some_and(|l| l.id.eq_ignore_ascii_case(&link.id));
        if let Some(held) = data.todos.iter().find(|t| t.collection == collection && same(t)) {
            return Ok(held.clone());
        }
        let known = data.todos.iter().find(|t| same(t) && t.link.as_ref().is_some_and(|l| l.github.is_some()));
        let Some(known) = known.cloned() else {
            return Err(ProviderError::Other(UNKNOWN_ITEM.into()));
        };
        data.next_id += 1;
        let next = data.next_id;
        let version = format!("\"demo-{next}\"");
        let position = data.todos.iter().filter(|t| t.collection == collection).count();
        let Some(list) = data.collections.iter_mut().find(|c| c.id == collection && c.kind.has_todos()) else {
            return Err(ProviderError::Other(format!("No task list {collection}")));
        };
        let todo = match (&mut list.source, list.writable) {
            (Some(filter), _) => {
                filter.pinned.push(link.id.clone());
                filter.revision += 1;
                Todo {
                    uid: link.id.clone(),
                    collection: collection.clone(),
                    href: known.url.clone(),
                    etag: Some(version),
                    due: None,
                    priority: None,
                    parent: None,
                    sort_order: i64::try_from(position).ok(),
                    ..known
                }
            }
            (None, true) => {
                let uid = format!("demo-todo-{next}");
                // The core stores the address, the link id and GitHub's title; done when closed.
                Todo {
                    href: Some(format!("{}{uid}.ics", url_of(&collection))),
                    etag: Some(version),
                    completed: known.completed,
                    url: known.url.clone(),
                    link: known.link.clone(),
                    ..todo(&collection, &uid, &known.summary, None)
                }
            }
            (None, false) => return Err(ProviderError::Other(format!("{} is read-only", list.name))),
        };
        data.todos.push(todo.clone());
        Ok(todo)
    }

    /// A new list of the GitHub account with a fresh id (`github:mahype/list-<n>`, never handed
    /// out twice) and the sample's items its searches find.
    async fn create_filter_list(
        &self,
        account: String,
        definition: FilterDefinition,
    ) -> Result<Collection, ProviderError> {
        check_definition(&definition)?;
        if account != GITHUB_ACCOUNT {
            return Err(ProviderError::Other(format!("{account} is no connected GitHub account")));
        }
        let mut data = self.lock();
        data.next_id += 1;
        let id = format!("{GITHUB_ACCOUNT}/list-{}", data.next_id);
        let queries: Vec<String> = definition.queries.into_iter().filter(|q| !q.trim().is_empty()).collect();
        let items = data.filter_items(&id, &queries, &[])?;
        let source = GitHubFilter { queries, pinned: Vec::new(), revision: 1, truncated: false, problem: None };
        let list = Collection {
            id: id.clone(),
            name: definition.name.trim().into(),
            kind: CollectionKind::Todos,
            color: definition.color,
            writable: false,
            source: Some(source),
        };
        data.collections.push(list.clone());
        data.todos.extend(items);
        Ok(list)
    }

    /// Like the core: an outdated `revision` is a conflict; pinned items stay.
    async fn update_filter_list(
        &self,
        collection: String,
        revision: u64,
        definition: FilterDefinition,
    ) -> Result<Collection, ProviderError> {
        check_definition(&definition)?;
        let mut data = self.lock();
        let Some(source) = data.collections.iter().find(|c| c.id == collection).and_then(|c| c.source.clone()) else {
            return Err(ProviderError::Other(format!("No filter list {collection}")));
        };
        if source.revision != revision {
            return Err(ProviderError::Conflict { uid: collection });
        }
        let queries: Vec<String> = definition.queries.into_iter().filter(|q| !q.trim().is_empty()).collect();
        let items = data.filter_items(&collection, &queries, &source.pinned)?;
        data.todos.retain(|t| t.collection != collection);
        data.todos.extend(items);
        let Some(list) = data.collections.iter_mut().find(|c| c.id == collection) else {
            return Err(ProviderError::Other(format!("No filter list {collection}")));
        };
        list.name = definition.name.trim().into();
        list.color = definition.color;
        list.source = Some(GitHubFilter { queries, revision: revision + 1, problem: None, truncated: false, ..source });
        Ok(list.clone())
    }

    async fn delete_filter_list(&self, collection: String) -> Result<(), ProviderError> {
        let mut data = self.lock();
        let before = data.collections.len();
        data.collections.retain(|c| c.id != collection || c.source.is_none());
        if data.collections.len() == before {
            return Err(ProviderError::Other(format!("No filter list {collection}")));
        }
        data.todos.retain(|t| t.collection != collection);
        Ok(())
    }

    /// Searches the sample's GitHub items ([`demo_matches`]); the count adds up over the searches.
    async fn preview_filter(&self, account: String, queries: Vec<String>) -> Result<FilterPreview, ProviderError> {
        if account != GITHUB_ACCOUNT {
            return Err(ProviderError::Other(format!("{account} is no connected GitHub account")));
        }
        let data = self.lock();
        let mut preview = FilterPreview::default();
        for query in queries.iter().filter(|q| !q.trim().is_empty()) {
            let found = demo_matches(&data.todos, query)?;
            preview.matches += found.len() as u64;
            for item in found {
                if preview.todos.len() < PREVIEW_ITEMS && !preview.todos.iter().any(|t| t.uid == item.uid) {
                    preview.todos.push(item);
                }
            }
        }
        Ok(preview)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::Timelike;

    use super::*;

    /// A Sunday, the day the sample is written for.
    fn sunday() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 11).expect("valid date")
    }

    fn local_date(time: EventTime) -> NaiveDate {
        match time {
            EventTime::Date(date) => date,
            EventTime::DateTime(instant) => instant.with_timezone(&Local).date_naive(),
        }
    }

    /// Start and end of a timed event; `None` for all-day events and events without end.
    fn timed_bounds(event: &Event) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        match (event.start, event.end) {
            (EventTime::DateTime(start), Some(EventTime::DateTime(end))) => Some((start, end)),
            _ => None,
        }
    }

    #[test]
    fn collections_and_counts_per_collection() {
        let (collections, events, todos) = sample(sunday());
        let shape: Vec<(&str, CollectionKind)> = collections.iter().map(|c| (c.name.as_str(), c.kind)).collect();
        assert_eq!(
            shape,
            vec![
                ("Work", CollectionKind::Events),
                ("Personal", CollectionKind::Events),
                ("Tasks", CollectionKind::Todos),
                ("Groceries", CollectionKind::Todos),
                ("Family", CollectionKind::Both),
                ("Needs me", CollectionKind::Todos),
                ("Torro bugs", CollectionKind::Todos),
            ]
        );
        let events_in = |id: &str| events.iter().filter(|e| e.collection == id).count();
        assert_eq!(events_in(WORK), 19);
        assert_eq!(events_in(PERSONAL), 4);
        assert_eq!(events_in(FAMILY), 4);
        assert_eq!(events.len(), 27);
        let todos_in = |id: &str| todos.iter().filter(|t| t.collection == id).count();
        assert_eq!(todos_in(TASKS), 14);
        assert_eq!(todos_in(GROCERIES), 5);
        assert_eq!(todos_in(FAMILY), 5);
        assert_eq!((todos_in(NEEDS_ME), todos_in(TORRO_BUGS)), (3, 3));
        assert_eq!(todos.len(), 30);

        for e in &events {
            assert!(collections.iter().any(|c| c.id == e.collection && c.kind.has_events()), "{}", e.summary);
        }
        for t in &todos {
            assert!(collections.iter().any(|c| c.id == t.collection && c.kind.has_todos()), "{}", t.summary);
        }
    }

    #[tokio::test]
    async fn the_sample_spreads_over_two_nextcloud_accounts_and_a_github_one() {
        let accounts = DemoSource::new(sunday()).accounts().await.expect("accounts");
        let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, [WORK_ACCOUNT, HOME_ACCOUNT, GITHUB_ACCOUNT]);
        assert!(accounts.iter().all(|a| matches!(a.state, AccountState::Connected { .. })));
        assert_eq!(accounts[0].label, "anna@work.example.org");
        assert_eq!((accounts[2].kind.as_str(), accounts[2].label.as_str()), (GITHUB_KIND, "mahype"));

        let (collections, _, _) = sample(sunday());
        let owner = |c: &Collection| account_of(&c.id, &accounts, |a| a.id.as_str()).map(|a| a.id.clone());
        let per_account = |id: &str| collections.iter().filter(|c| owner(c).as_deref() == Some(id)).count();
        assert_eq!((per_account(WORK_ACCOUNT), per_account(HOME_ACCOUNT), per_account(GITHUB_ACCOUNT)), (2, 3, 2));
    }

    #[test]
    fn every_item_has_unique_href_and_etag() {
        let (collections, events, todos) = sample(sunday());
        let filter_list = |id: &str| collections.iter().any(|c| c.id == id && c.source.is_some());
        let mut hrefs = HashSet::new();
        for e in &events {
            let href = e.href.as_deref().expect("sample event has an href");
            assert!(href.starts_with(url_of(&e.collection)) && href.ends_with(".ics"), "{href}");
            assert!(e.etag.is_some(), "{href}");
            assert!(hrefs.insert(href.to_owned()), "duplicate href {href}");
        }
        for t in &todos {
            let href = t.href.as_deref().expect("sample task has an href");
            if filter_list(&t.collection) {
                assert_eq!(t.href, t.url, "a match lives on GitHub");
            } else {
                assert!(href.starts_with(url_of(&t.collection)) && href.ends_with(".ics"), "{href}");
            }
            assert!(t.etag.is_some(), "{href}");
            assert!(hrefs.insert(href.to_owned()), "duplicate href {href}");
        }
    }

    #[test]
    fn the_filter_lists_are_read_only_with_their_hints_and_a_pin() {
        let (collections, _, todos) = sample(sunday());
        let lists: Vec<&Collection> = collections.iter().filter(|c| c.source.is_some()).collect();
        assert_eq!(lists.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), [NEEDS_ME, TORRO_BUGS]);
        for list in &lists {
            assert!(!list.writable && list.kind == CollectionKind::Todos && list.color.is_some(), "{}", list.name);
            for item in todos.iter().filter(|t| t.collection == list.id) {
                let link = item.link.as_ref().expect("a match is a GitHub item");
                assert_eq!(item.uid, link.id, "the core's uid of a match is its link id");
                assert!(link.github.is_some(), "{}", item.summary);
            }
        }
        let source = |id: &str| collections.iter().find(|c| c.id == id).and_then(|c| c.source.clone());
        let needs_me = source(NEEDS_ME).expect("a filter list");
        assert!(needs_me.truncated && needs_me.problem.is_none() && needs_me.queries.len() == 2);
        let pinned = &needs_me.pinned[0];
        assert!(todos.iter().any(|t| t.collection == NEEDS_ME && t.uid == *pinned), "the pinned item is in the list");
        let bugs = source(TORRO_BUGS).expect("a filter list");
        assert!(bugs.problem.is_some() && !bugs.truncated);
        let repos: HashSet<&str> = todos
            .iter()
            .filter(|t| t.collection == NEEDS_ME)
            .filter_map(|t| t.link.as_ref()?.github_ref().map(|(repo, _)| repo))
            .collect();
        assert_eq!(repos.len(), 2, "grouped by repository, the list has two groups");
    }

    #[tokio::test]
    async fn unpinning_takes_the_item_out_and_matches_cannot_be_changed() {
        let source = DemoSource::new(sunday());
        let pinned = Link::github(DESIGN_REPO, 18);
        let in_list = |todos: &[Todo]| todos.iter().any(|t| t.collection == NEEDS_ME && t.uid == pinned.id);
        let match_ = source.todos().await.expect("todos").into_iter().find(|t| t.collection == TORRO_BUGS);
        let match_ = match_.expect("a match");
        let renamed = Todo { summary: "Something else".into(), ..match_.clone() };
        assert_eq!(source.save_todo(renamed).await, Err(ProviderError::Other(READ_ONLY.into())));
        assert_eq!(source.delete_todo(match_).await, Err(ProviderError::Other(NOT_DELETED.into())));

        assert!(source.unpin_issue(TORRO_BUGS.into(), pinned.id.clone()).await.is_err(), "not pinned there");
        assert!(in_list(&source.todos().await.expect("todos")));
        source.unpin_issue(NEEDS_ME.into(), pinned.id.clone()).await.expect("unpinned");
        assert!(!in_list(&source.todos().await.expect("todos")));
        let collections = source.collections().await.expect("collections");
        let needs_me = collections.iter().find(|c| c.id == NEEDS_ME).and_then(|c| c.source.as_ref());
        assert_eq!(needs_me.map(|s| s.pinned.len()), Some(0));
        assert!(source.unpin_issue(NEEDS_ME.into(), pinned.id.clone()).await.is_err(), "only once");
        assert!(source.unpin_issue(TASKS.into(), pinned.id).await.is_err(), "no filter list");
    }

    #[tokio::test]
    async fn a_link_is_pinned_in_a_filter_list_and_a_task_elsewhere_and_ticking_it_closes_the_issue() {
        let source = DemoSource::new(sunday());
        let address = format!("https://github.com/{REPO}/issues/145");
        let pinned = source.add_link(NEEDS_ME.into(), address.clone()).await.expect("pinned");
        assert_eq!((pinned.collection.as_str(), pinned.uid.as_str()), (NEEDS_ME, Link::github(REPO, 145).id.as_str()));
        let collections = source.collections().await.expect("collections");
        let needs_me = collections.iter().find(|c| c.id == NEEDS_ME).and_then(|c| c.source.clone()).expect("a list");
        assert!(needs_me.pinned.contains(&pinned.uid) && needs_me.revision == 2);
        assert_eq!(source.add_link(NEEDS_ME.into(), address.clone()).await, Ok(pinned), "pinned once");

        let link = source.add_link(GROCERIES.into(), address).await.expect("a link task");
        assert!(link.href.is_some() && link.collection == GROCERIES && !link.completed);
        assert_eq!(link.link.as_ref().map(|l| l.id.as_str()), Some(Link::github(REPO, 145).id.as_str()));
        let ticked = source.save_todo(Todo { completed: true, ..link }).await.expect("ticked");
        let state = ticked.link.as_ref().and_then(|l| l.github.as_ref()).map(|g| g.state);
        assert_eq!(state, Some(GitHubState::Closed), "the issue closes with it");

        assert_eq!(source.add_link(TASKS.into(), "Buy milk".into()).await, Err(ProviderError::Other(NO_LINK.into())));
        let unknown = source.add_link(TASKS.into(), "a/b#1".into()).await;
        assert_eq!(unknown, Err(ProviderError::Other(UNKNOWN_ITEM.into())));
    }

    #[tokio::test]
    async fn ticking_an_issue_of_a_filter_list_closes_and_reopens_it_but_never_a_pull_request() {
        let source = DemoSource::new(sunday());
        let todos = source.todos().await.expect("todos");
        let in_list = |list: &str, number: u64| {
            let id = Link::github(REPO, number).id;
            todos.iter().find(|t| t.collection == list && t.uid == id).cloned().expect("in the list")
        };
        let state = |todo: &Todo| todo.link.as_ref().and_then(|link| link.github.as_ref()).map(|g| g.state);
        let issue = in_list(TORRO_BUGS, 145);
        let closed = source.save_todo(Todo { completed: true, ..issue.clone() }).await.expect("closed");
        assert!(closed.completed && closed.etag != issue.etag);
        assert_eq!(state(&closed), Some(GitHubState::Closed));
        let stale = source.save_todo(Todo { completed: false, ..issue }).await;
        assert!(matches!(stale, Err(ProviderError::Conflict { .. })), "{stale:?}");
        let reopened = source.save_todo(Todo { completed: false, ..closed }).await.expect("reopened");
        assert_eq!((reopened.completed, state(&reopened)), (false, Some(GitHubState::Open)));

        let pull = in_list(NEEDS_ME, 138);
        let refused = source.save_todo(Todo { completed: true, ..pull }).await;
        assert_eq!(refused, Err(ProviderError::Other(PULL_REQUESTS.into())));
        let new = Todo { href: None, uid: String::new(), ..in_list(NEEDS_ME, 142) };
        assert_eq!(source.save_todo(new).await, Err(ProviderError::Other(READ_ONLY.into())));
        let accounts = source.accounts().await.expect("accounts");
        assert!(accounts.iter().all(PimAccount::may_close_issues), "the demo's token may write issues");
    }

    #[tokio::test]
    async fn the_preview_searches_the_sample() {
        let source = DemoSource::new(sunday());
        let bugs = vec![format!("is:issue is:open repo:{REPO} label:bug")];
        let preview = source.preview_filter(GITHUB_ACCOUNT.into(), bugs).await.expect("preview");
        assert!(preview.matches >= 3, "the open bugs of the sample: {}", preview.matches);
        assert!(preview.todos.iter().all(|t| t.collection.is_empty() && t.link.is_some()));
        let both = vec!["is:pr".to_string(), "is:issue".to_string()];
        let all = source.preview_filter(GITHUB_ACCOUNT.into(), both).await.expect("preview");
        assert!(all.matches > preview.matches, "the counts add up");
        let address = vec!["https://github.com/mahype/TorroWorkplace/issues".to_string()];
        assert!(source.preview_filter(GITHUB_ACCOUNT.into(), address).await.is_err());
        assert!(source.preview_filter(WORK_ACCOUNT.into(), vec!["is:open".into()]).await.is_err());
    }

    #[tokio::test]
    async fn filter_lists_are_created_changed_and_deleted() {
        let source = DemoSource::new(sunday());
        let definition = |name: &str, query: &str| FilterDefinition {
            name: name.into(),
            color: Some("#5B8CFF".into()),
            queries: vec![query.into()],
        };
        let empty = source.create_filter_list(GITHUB_ACCOUNT.into(), definition(" ", "is:open")).await;
        assert_eq!(empty, Err(ProviderError::Other("A filter list needs a name.".into())));

        let list = source.create_filter_list(GITHUB_ACCOUNT.into(), definition("PRs", "is:pr")).await.expect("new");
        assert!(list.id.starts_with("github:mahype/") && !list.writable);
        let items = |todos: &[Todo], id: &str| todos.iter().filter(|t| t.collection == id).count();
        let pulls = items(&source.todos().await.expect("todos"), &list.id);
        assert!(pulls > 0, "the new list holds the sample's pull requests");

        let stale = source.update_filter_list(list.id.clone(), 7, definition("Bugs", "label:bug")).await;
        assert_eq!(stale, Err(ProviderError::Conflict { uid: list.id.clone() }));
        let changed =
            source.update_filter_list(list.id.clone(), 1, definition("Bugs", "label:bug")).await.expect("changed");
        assert_eq!((changed.name.as_str(), changed.source.map(|s| s.revision)), ("Bugs", Some(2)));
        assert_ne!(items(&source.todos().await.expect("todos"), &list.id), 0);

        source.delete_filter_list(list.id.clone()).await.expect("deleted");
        assert_eq!(items(&source.todos().await.expect("todos"), &list.id), 0);
        assert!(!source.collections().await.expect("collections").iter().any(|c| c.id == list.id));
        assert!(source.delete_filter_list(list.id).await.is_err(), "only once");
        assert!(source.delete_filter_list(TASKS.into()).await.is_err(), "no filter list");
    }

    #[test]
    fn daily_standup_is_one_recurring_series() {
        let (_, events, _) = sample(sunday());
        let standups: Vec<&Event> = events.iter().filter(|e| e.uid == "daily-standup@torro-demo").collect();
        assert_eq!(standups.len(), 10);
        assert_eq!(events.iter().filter(|e| e.recurring).count(), 10);
        let mut days: Vec<NaiveDate> = standups.iter().map(|e| local_date(e.start)).collect();
        days.sort();
        days.dedup();
        assert_eq!(days.len(), 10, "one occurrence per day");
        for e in standups {
            assert!(e.recurring);
            let (start, end) = timed_bounds(e).expect("standup is timed");
            let (start, end) = (start.with_timezone(&Local), end.with_timezone(&Local));
            assert_eq!((start.hour(), start.minute()), (9, 30));
            assert_eq!((end.hour(), end.minute()), (9, 45));
            assert!(start.weekday().number_from_monday() <= 5, "standup on a weekend");
        }
    }

    #[test]
    fn a_weekday_has_overlapping_meetings() {
        let (_, events, _) = sample(sunday());
        let timed: Vec<(&Event, DateTime<Utc>, DateTime<Utc>)> =
            events.iter().filter_map(|e| timed_bounds(e).map(|(s, t)| (e, s, t))).collect();
        let overlap = timed.iter().enumerate().any(|(i, (a, a_start, a_end))| {
            timed[i + 1..].iter().any(|(b, b_start, b_end)| {
                let day = local_date(a.start);
                a_start < b_end
                    && b_start < a_end
                    && day == local_date(b.start)
                    && day.weekday().number_from_monday() <= 5
            })
        });
        assert!(overlap);
    }

    #[test]
    fn all_day_vacation_spans_three_days() {
        let (_, events, _) = sample(sunday());
        let vacation = events.iter().find(|e| e.summary == "Vacation").expect("Vacation exists");
        assert!(vacation.all_day());
        let (EventTime::Date(first), Some(EventTime::Date(end))) = (vacation.start, vacation.end) else {
            panic!("Vacation is all-day with a date end");
        };
        assert_eq!((end - first).num_days(), 3, "end is exclusive");
        // Tuesday of next week: Sunday is the 6th day of this week.
        assert_eq!(first, sunday() + Days::new(2));
    }

    #[test]
    fn one_meeting_crosses_midnight() {
        let (_, events, _) = sample(sunday());
        let crosses = events.iter().filter_map(timed_bounds).any(|(start, end)| {
            let (start, end) = (start.with_timezone(&Local), end.with_timezone(&Local));
            start.date_naive() != end.date_naive() && (end.hour(), end.minute()) == (0, 30)
        });
        assert!(crosses);
    }

    #[test]
    fn subtasks_reference_existing_parents() {
        let (_, _, todos) = sample(sunday());
        let uids: HashSet<&str> = todos.iter().map(|t| t.uid.as_str()).collect();
        for t in &todos {
            if let Some(parent) = &t.parent {
                assert!(uids.contains(parent.as_str()), "{} has no parent {parent}", t.summary);
            }
        }
        let parents: HashSet<&str> = todos.iter().filter_map(|t| t.parent.as_deref()).collect();
        assert_eq!(parents.len(), 3);
        for parent in parents {
            let children = todos.iter().filter(|t| t.parent.as_deref() == Some(parent)).count();
            assert!((2..=3).contains(&children), "{parent} has {children} subtasks");
        }
    }

    #[test]
    fn tasks_cover_overdue_today_later_and_undated() {
        let (_, _, todos) = sample(sunday());
        let open = || todos.iter().filter(|t| !t.completed);
        assert_eq!(todos.iter().filter(|t| t.completed).count(), 6, "three tasks, a merged PR, two closed issues");
        assert!(open().any(|t| t.due.is_some_and(|due| local_date(due) < sunday())), "overdue");
        assert!(open().any(|t| t.due.is_some_and(|due| local_date(due) == sunday())), "due today");
        assert!(open().any(|t| t.due.is_some_and(|due| local_date(due) > sunday())), "later");
        assert!(todos.iter().any(|t| t.due.is_none()), "without due date");
        for priority in [Some(1), Some(5), Some(9), None] {
            assert!(todos.iter().any(|t| t.priority == priority), "priority {priority:?}");
        }
    }

    #[test]
    fn one_task_has_tags_and_the_github_items_cover_every_state() {
        let (_, _, todos) = sample(sunday());
        assert!(todos.iter().any(|t| t.categories.len() > 1 && t.link.is_none()), "a task with tags");
        // The linked tasks of "Tasks"; the filter lists have their own test.
        let github: Vec<(&GitHubDetails, &Todo)> = todos
            .iter()
            .filter(|t| t.collection == TASKS)
            .filter_map(|t| Some((t.link.as_ref()?.github.as_ref()?, t)))
            .collect();
        assert_eq!(github.len(), 6);
        for (details, todo) in &github {
            let link = todo.link.as_ref().expect("linked");
            assert_eq!(link.github_ref().map(|(repo, _)| repo), Some(REPO), "{}", todo.summary);
            assert_eq!(todo.url, link.web_url(), "the url opens the item");
            assert_eq!(todo.completed, details.state != GitHubState::Open, "{}: done when closed", todo.summary);
            assert!(!details.labels.is_empty() && details.labels.iter().all(|l| l.color.is_some()));
        }
        let has = |kind, state, extra: fn(&GitHubDetails) -> bool| {
            github.iter().any(|(d, _)| d.kind == kind && d.state == state && extra(d))
        };
        assert!(has(GitHubKind::Issue, GitHubState::Open, |_| true), "an open issue");
        assert!(has(GitHubKind::Issue, GitHubState::Closed, |_| true), "a closed issue");
        assert!(
            has(GitHubKind::PullRequest, GitHubState::Open, |d| d.review == Some(ReviewState::Required)
                && d.checks == Some(ChecksState::Pending)
                && !d.draft),
            "an open pull request waiting for a review, checks running"
        );
        assert!(has(GitHubKind::PullRequest, GitHubState::Open, |d| d.draft), "a draft");
        assert!(has(GitHubKind::PullRequest, GitHubState::Merged, |_| true), "a merged pull request");
    }

    #[tokio::test]
    async fn week_query_returns_the_weeks_events() {
        // A Sunday and the Monday after it, so both edges of the week are covered.
        for today in [sunday(), sunday() + Days::new(1)] {
            let monday = today - Days::new(u64::from(today.weekday().num_days_from_monday()));
            let EventTime::DateTime(start) = at(monday, 0, 0) else { panic!("week starts at an instant") };
            let EventTime::DateTime(end) = at(monday + Days::new(7), 0, 0) else { panic!("week ends at an instant") };
            let got = DemoSource::new(today).events(start, end).await.expect("events");
            let (_, all, _) = sample(today);
            let week = monday..monday + Days::new(7);
            let expected: Vec<Event> = all.into_iter().filter(|e| week.contains(&local_date(e.start))).collect();
            assert!(!got.is_empty());
            assert_eq!(got, expected, "week of {monday}");
        }
    }

    #[tokio::test]
    async fn a_task_is_moved_by_hand_and_a_stale_one_is_refused() {
        use crate::source::manual_order;
        let source = DemoSource::new(sunday());
        let open_groceries = |todos: Vec<Todo>| -> Vec<Todo> {
            let mut list: Vec<Todo> =
                todos.into_iter().filter(|t| t.collection == GROCERIES && !t.completed && t.parent.is_none()).collect();
            list.sort_by(manual_order);
            list
        };
        let before = open_groceries(source.todos().await.expect("todos"));
        assert!(before.len() >= 2, "the sample has a list to order");
        let (first, last) = (before[0].clone(), before[before.len() - 1].clone());

        let written = source.move_todo(last.clone(), Some(first.clone())).await.expect("moved");
        assert!(written.iter().all(|t| t.etag != last.etag && t.etag != first.etag), "new versions");
        let after = open_groceries(source.todos().await.expect("todos"));
        assert_eq!(after[0].uid, last.uid, "now on top");
        assert_eq!(after.len(), before.len());

        // The version read before the move is out of date now.
        assert!(matches!(source.move_todo(last, None).await, Err(ProviderError::Conflict { .. })));
        let elsewhere = source.todos().await.expect("todos").into_iter().find(|t| t.collection != GROCERIES);
        let moved = after[0].clone();
        assert!(matches!(source.move_todo(moved, elsewhere).await, Err(ProviderError::Other(_))));
    }

    fn source_with_calendar() -> DemoSource {
        let source = DemoSource::default();
        source.lock().collections.push(Collection {
            id: "demo:/work/".into(),
            name: "Work".into(),
            kind: CollectionKind::Events,
            color: Some("#3474E6".into()),
            writable: true,
            source: None,
        });
        source
    }

    #[tokio::test]
    async fn save_update_delete_round_trip() {
        let source = source_with_calendar();
        let start = Utc::now();
        let new = Event {
            uid: String::new(),
            collection: "demo:/work/".into(),
            href: None,
            etag: None,
            summary: "Workshop".into(),
            start: EventTime::DateTime(start),
            end: Some(EventTime::DateTime(start + chrono::Duration::hours(1))),
            location: None,
            description: None,
            recurring: false,
            transparency: crate::Transparency::Opaque,
        };
        let mut saved = source.save_event(new).await.unwrap();
        assert!(saved.href.is_some() && saved.etag.is_some());
        let stale = saved.clone();
        saved.summary = "Workshop Teil 1".into();
        let updated = source.save_event(saved).await.unwrap();
        assert!(matches!(source.save_event(stale).await, Err(ProviderError::Conflict { .. })));
        let range = source.events(start - chrono::Duration::days(1), start + chrono::Duration::days(1)).await.unwrap();
        assert_eq!(range, vec![updated.clone()]);
        source.delete_event(updated).await.unwrap();
        assert!(
            source
                .events(start - chrono::Duration::days(1), start + chrono::Duration::days(1))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
