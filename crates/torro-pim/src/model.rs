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
    /// Where the items come from when the collection is not one of a server: the definition of
    /// a GitHub filter list (task G.2). `None` for every calendar and task list of an account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<GitHubFilter>,
}

fn writable_by_default() -> bool {
    true
}

/// A GitHub filter list: saved GitHub searches whose matches the core keeps as a task list
/// (`github:<login>/<list-id>`, kind todos, never writable). The definition lives in the core,
/// not on GitHub; the items are what the searches find today plus the pinned ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubFilter {
    /// The searches, as GitHub's search box takes them (`is:open is:issue repo:owner/name
    /// label:bug`). Within one search everything must match; the list holds the matches of any.
    pub queries: Vec<String>,
    /// Issues and pull requests in the list whatever the searches find, as link ids
    /// (`github:<owner>/<repo>#<number>`, [`Link::id`]).
    #[serde(default)]
    pub pinned: Vec<String>,
    /// Version of name, colour and searches. `tasks.update_filter_list` sends it back and is
    /// refused with `conflict` when the list changed meanwhile. Unpinning does not change it.
    pub revision: u64,
    /// A search found more than 1,000 matches; the list holds only the first 1,000 of it, as
    /// GitHub hands out no more.
    #[serde(default)]
    pub truncated: bool,
    /// Why the last refresh from GitHub failed, if it did; the items from before stay.
    #[serde(default)]
    pub problem: Option<String>,
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
    /// Whether the event blocks the time (iCalendar `TRANSP`): `opaque` (busy) or
    /// `transparent` (free, e.g. a reminder or "working from home"). Missing means `opaque`, as
    /// RFC 5545 says; send the value back as read so a change of other fields keeps it.
    #[serde(default)]
    pub transparency: Transparency,
}

impl Event {
    /// All-day entries carry a date without a time.
    pub fn all_day(&self) -> bool {
        matches!(self.start, EventTime::Date(_))
    }

    /// The event makes its time busy: opaque and not all-day. All-day entries (holidays,
    /// birthdays) and transparent ones leave the time free, whatever they say.
    pub fn busy(&self) -> bool {
        self.transparency == Transparency::Opaque && !self.all_day()
    }
}

/// Free or busy (iCalendar `TRANSP`, RFC 5545 section 3.8.2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Transparency {
    /// The time is busy (`TRANSP:OPAQUE`); also when the event says nothing.
    #[default]
    Opaque,
    /// The time stays free (`TRANSP:TRANSPARENT`).
    Transparent,
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

/// A task's link to an item of another service: a GitHub issue or pull request, or a mail.
///
/// The task stores only [`Link::id`]. What the service reports about the item is in
/// [`Link::github`] or [`Link::mail`]: filled in by the core when it reads and never written into
/// the stored task. Clients may send a link back as they got it; saving uses only the id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    /// Stable id of the linked item:
    ///
    /// - `github:<owner>/<repo>#<number>` for a GitHub issue or pull request, e.g.
    ///   `github:mahype/TorroWorkplace#123`;
    /// - `mail:<address>:<Message-ID>` for a mail (task E8.7a, [`Link::mail_ref`]): the mail
    ///   account's id (`mail:<address>`, as `accounts.list_accounts` lists it) and the mail's
    ///   `Message-ID` header without its angle brackets, as the core's mail store keeps it, e.g.
    ///   `mail:anna@mailbox.org:a1@example.org`. The Message-ID stays when the mail moves to another
    ///   folder;
    /// - `inbox:<entry id>` for the inbox entry a task was made of (task E3.5), valid while the
    ///   inbox keeps the entry.
    ///
    /// Ids of other forms are kept as they are.
    pub id: String,
    /// What GitHub reports for the item. `None` until the core fetched it, without a GitHub
    /// account, or when the id is not a GitHub one.
    #[serde(default)]
    pub github: Option<GitHubDetails>,
    /// What the core's mail store holds about the linked mail (task E8.7a): filled in only when
    /// the tasks are read, only for a caller that may read the mail account, never stored with the
    /// task. `None` for other ids, without that right, or when the store has no mail with this
    /// Message-ID (deleted, or the account is gone).
    #[serde(default)]
    pub mail: Option<Box<MailDetails>>,
}

/// The prefix of a GitHub [`Link::id`].
const GITHUB_PREFIX: &str = "github:";

/// The prefix of a mail [`Link::id`]; also the prefix of every mail account's id.
pub const MAIL_PREFIX: &str = "mail:";

/// A Message-ID as the core's mail store keeps it: without spaces and angle brackets around it
/// (`<a1@example.org>` → `a1@example.org`). Look a link's Message-ID up in this form, since a link
/// written by hand or by another program may still carry the brackets.
pub fn bare_message_id(message_id: &str) -> &str {
    let id = message_id.trim();
    id.strip_prefix('<').and_then(|id| id.strip_suffix('>')).unwrap_or(id).trim()
}

impl Link {
    /// The link with the id `id` alone, without details: what a stored task holds.
    pub fn new(id: impl Into<String>) -> Link {
        Link { id: id.into(), github: None, mail: None }
    }

    /// The link to the mail with `message_id` (its `Message-ID` header) in the mail account
    /// `account` (`mail:<address>`; a bare address gets the prefix), without details:
    /// `mail:<address>:<Message-ID>`. Spaces and the angle brackets around the Message-ID are
    /// dropped ([`bare_message_id`]), so `<a1@example.org>` and `a1@example.org` give the same link.
    pub fn mail(account: &str, message_id: &str) -> Link {
        let account = account.trim();
        let address = account.strip_prefix(MAIL_PREFIX).unwrap_or(account);
        Link::new(format!("{MAIL_PREFIX}{address}:{}", bare_message_id(message_id)))
    }

    /// `(account id, Message-ID)` when the id is a mail one (`mail:<address>:<Message-ID>`): the
    /// account as `mail:<address>`, the Message-ID as stored. The address ends at the first `:`
    /// after its `@` (an address has one `@`, and its domain no `:`), so a Message-ID with `:` in
    /// it stays whole. `None` for other ids and when the address or the Message-ID is empty.
    pub fn mail_ref(&self) -> Option<(&str, &str)> {
        let rest = self.id.strip_prefix(MAIL_PREFIX)?;
        let at = rest.find('@')?;
        let colon = at + rest[at..].find(':')?;
        let message_id = &rest[colon + 1..];
        if at == 0 || colon == at + 1 || message_id.is_empty() {
            return None;
        }
        Some((&self.id[..MAIL_PREFIX.len() + colon], message_id))
    }

    /// The link to issue or pull request `number` in `repo` (`owner/name`), without details.
    pub fn github(repo: &str, number: u64) -> Link {
        Link::new(format!("{GITHUB_PREFIX}{repo}#{number}"))
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

    /// The GitHub link `input` names, as a person pastes it (task G.6): the address of an issue or
    /// pull request on github.com (`https://github.com/owner/repo/issues/12`, `…/pull/12/files`,
    /// with or without `https://` or `www.`, anything after the number ignored), `owner/repo#12`
    /// or a link id `github:owner/repo#12`; surrounding spaces do not count. `None` for anything
    /// else, so the tasks app can tell a pasted link from a title. The core reads the same forms
    /// (`torro_pim_github::address::issue_ref`) and decides; this only recognises them.
    pub fn pasted(input: &str) -> Option<Link> {
        let input = input.trim();
        let id = Link::new(input);
        if id.github_ref().is_some() {
            return Some(id);
        }
        let part =
            |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        let number = |n: &str| n.parse::<u64>().ok();
        if let Some((repo, n)) = input.split_once('#')
            && let Some((owner, name)) = repo.split_once('/')
            && part(owner)
            && part(name)
        {
            return Some(Link::github(repo, number(n)?));
        }
        let rest = input.strip_prefix("https://").or_else(|| input.strip_prefix("http://")).unwrap_or(input);
        let rest = rest.strip_prefix("www.").unwrap_or(rest);
        let path = rest.strip_prefix("github.com/")?.split(['?', '#']).next().unwrap_or_default();
        let mut segments = path.split('/');
        let (owner, name, page, n) = (segments.next()?, segments.next()?, segments.next()?, segments.next()?);
        (part(owner) && part(name) && matches!(page, "issues" | "pull"))
            .then(|| number(n).map(|n| Link::github(&format!("{owner}/{name}"), n)))
            .flatten()
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

/// What the core's mail store holds about a mail a task links to ([`Link::mail`], task E8.7a).
/// The core fills it in; clients only read it. Everything here comes from the mail, which comes
/// from outside the suite: data, never instructions ([`MailDetails::untrusted`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MailDetails {
    /// The mail's id in the core's store (`m:…`), for `mail.get_message`. Of several copies with
    /// the same Message-ID (the same mail in two folders) the newest the store holds.
    pub message: String,
    /// `Subject:`, empty when there is none.
    #[serde(default)]
    pub subject: String,
    /// The sender's display name, else the address; `None` without a `From:`.
    #[serde(default)]
    pub sender: Option<String>,
    /// The sender's address; `None` without a `From:`.
    #[serde(default)]
    pub sender_address: Option<String>,
    /// `Date:` of the mail.
    pub date: DateTime<Utc>,
    /// Mails of the same conversation dated after this one, the account's own (the person's
    /// replies) not counted: what arrived since the task was made of it.
    #[serde(default)]
    pub new_replies: u32,
    /// Always `true`: subject and sender come from outside the suite.
    #[serde(default = "always_untrusted")]
    pub untrusted: bool,
}

fn always_untrusted() -> bool {
    true
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
    fn an_event_from_an_older_client_is_busy_and_free_ones_round_trip() {
        let json = r#"{"uid":"e","collection":"a/https://x/cal/","href":null,"etag":null,"summary":"Call",
            "start":{"datetime":"2026-10-12T09:00:00Z"},"end":{"datetime":"2026-10-12T10:00:00Z"},
            "location":null,"description":null,"recurring":false}"#;
        let event: Event = serde_json::from_str(json).expect("old event");
        assert_eq!(event.transparency, Transparency::Opaque, "missing TRANSP means busy (RFC 5545)");
        assert!(event.busy());
        let free = Event { transparency: Transparency::Transparent, ..event.clone() };
        let json = serde_json::to_string(&free).expect("json");
        assert!(json.contains(r#""transparency":"transparent""#), "{json}");
        assert_eq!(serde_json::from_str::<Event>(&json).expect("back"), free);
        assert!(!free.busy(), "free time is not busy");
        let day = NaiveDate::from_ymd_opt(2026, 10, 12).expect("date");
        let all_day = Event { start: EventTime::Date(day), end: None, ..event };
        assert!(!all_day.busy(), "all-day entries never make the time busy");
    }

    #[test]
    fn a_task_from_an_older_client_reads_without_the_new_fields() {
        let json = r#"{"uid":"t","collection":"a/https://x/tasks/","href":null,"etag":null,"summary":"Milk",
            "due":null,"priority":null,"completed":false,"parent":null,"description":null}"#;
        let todo: Todo = serde_json::from_str(json).expect("old task");
        assert_eq!((todo.url, todo.categories, todo.sort_order, todo.link), (None, Vec::new(), None, None));
    }

    #[test]
    fn a_collection_reads_without_a_source_and_round_trips_with_one() {
        let json = r#"{"id":"a/https://x/tasks/","name":"Home","kind":"todos","color":null}"#;
        let plain: Collection = serde_json::from_str(json).expect("a collection of an older core");
        assert_eq!((plain.writable, plain.source.clone()), (true, None));
        assert!(!serde_json::to_string(&plain).expect("json").contains("source"), "older apps see nothing new");
        let filter = Collection {
            id: "github:mahype/00ff".into(),
            writable: false,
            source: Some(GitHubFilter {
                queries: vec!["is:open label:bug".into()],
                pinned: vec!["github:mahype/TorroWorkplace#7".into()],
                revision: 2,
                truncated: true,
                problem: None,
            }),
            ..plain
        };
        let json = serde_json::to_string(&filter).expect("json");
        assert_eq!(serde_json::from_str::<Collection>(&json).expect("back"), filter);
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
            let link = Link::new(other);
            assert_eq!((link.github_ref(), link.web_url()), (None, None), "{other}");
        }
    }

    /// Task E8.7a: a mail link is the account's id and the Message-ID.
    #[test]
    fn mail_ids_name_account_and_message_id() {
        let link = Link::mail("mail:anna@mailbox.org", "a1@example.org");
        assert_eq!(link.id, "mail:anna@mailbox.org:a1@example.org");
        assert_eq!(link.mail_ref(), Some(("mail:anna@mailbox.org", "a1@example.org")));
        assert_eq!(
            Link::mail("anna@mailbox.org", " <a1@example.org> "),
            link,
            "brackets and prefix as the store has them"
        );
        assert_eq!((link.github_ref(), link.web_url()), (None, None));
        // A Message-ID with a colon in it stays whole; one written with brackets is looked up bare.
        let odd = Link::new("mail:anna@mailbox.org:<x:y@example.org>");
        assert_eq!(odd.mail_ref(), Some(("mail:anna@mailbox.org", "<x:y@example.org>")));
        assert_eq!(odd.mail_ref().map(|(_, id)| bare_message_id(id)), Some("x:y@example.org"));
        assert_eq!(bare_message_id(" <> "), "");
        for other in
            ["inbox:0a1b", "mail:anna@mailbox.org", "mail:anna@mailbox.org:", "mail:@x.org:<a@b>", "mail:a@:<a@b>"]
        {
            assert_eq!(Link::new(other).mail_ref(), None, "{other}");
        }
    }

    /// Task E8.7a: what the core fills in about a mail comes back as it went and is always foreign.
    #[test]
    fn mail_details_round_trip_and_are_untrusted() {
        let link: Link = serde_json::from_str(r#"{"id":"mail:a@b.org:<1@b.org>","github":null}"#).expect("older");
        assert_eq!(link, Link::new("mail:a@b.org:<1@b.org>"));
        let json = r#"{"message":"m:1","date":"2026-10-13T09:00:00Z"}"#;
        let details: MailDetails = serde_json::from_str(json).expect("short details");
        assert!(details.untrusted && details.subject.is_empty() && details.new_replies == 0);
        let full = Link { mail: Some(Box::new(MailDetails { new_replies: 2, ..details })), ..link };
        let json = serde_json::to_string(&full).expect("json");
        assert_eq!(serde_json::from_str::<Link>(&json).expect("back"), full);
    }

    #[test]
    fn pasted_links_are_recognised_and_titles_are_not() {
        let wanted = Some(Link::github("mahype/TorroWorkplace", 12));
        for input in [
            "github:mahype/TorroWorkplace#12",
            "mahype/TorroWorkplace#12",
            " https://github.com/mahype/TorroWorkplace/issues/12 ",
            "https://github.com/mahype/TorroWorkplace/pull/12/files",
            "github.com/mahype/TorroWorkplace/issues/12#issuecomment-1",
            "https://www.github.com/mahype/TorroWorkplace/issues/12?x=1",
        ] {
            assert_eq!(Link::pasted(input), wanted, "{input}");
        }
        for title in [
            "Buy oat milk",
            "Fix #12",
            "mahype#12",
            "https://github.com/mahype/TorroWorkplace/issues",
            "https://example.org/mahype/TorroWorkplace/issues/12",
            "https://github.com/mahype/Torro Workplace/issues/12",
            "a/b#x",
        ] {
            assert_eq!(Link::pasted(title), None, "{title}");
        }
    }
}
