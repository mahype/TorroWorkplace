//! The account's GitHub notifications over REST (task E5.3, concept `docs/eingang.md`): what the
//! core files into the inbox as the source `github:<login>` (task E5.4).
//!
//! [`Notifications::list`] reads the unread threads (`GET /notifications?all=false`), page by page
//! along `Link: rel="next"`, and polls cheaply: with `If-Modified-Since` GitHub answers `304` when
//! nothing changed, and such an answer does not count against the rate limit. How often it may ask
//! says `X-Poll-Interval` ([`Listing::poll`]). [`Notifications::done`] marks a thread done
//! (`DELETE /notifications/threads/{id}`), [`Notifications::read`] marks it read (`PATCH`).
//!
//! Everything in an answer comes from outside. A [`Thread`] keeps only what was checked: an id of
//! digits ([`ThreadId`]), a repository named as GitHub names them, a known [`Reason`] and
//! [`SubjectKind`], and the number of an issue or pull request only from an API address on the
//! host the token was sent to. The link a person opens ([`Thread::web_url`]) is built from these
//! parts on `https://github.com`, never taken from the answer (rule 15 of the E5 plan).
//!
//! The token goes into the `Authorization` header only, and only to the REST address it was made
//! with: a `next` page on any other address is not followed. `Debug` never shows it, and no error
//! holds it or GitHub's own words ([`NotificationsError::public_message`]).
//!
//! These endpoints work only with classic personal access tokens (`notifications` or `repo`
//! scope); a fine-grained token gets `403`, which is [`NotificationsError::NeedsScope`].

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use reqwest::header::{self, HeaderMap};
use reqwest::{Method, RequestBuilder};
use serde::Deserialize;
use serde_json::Value;
use torro_core::account::Secret;

/// Where the links of [`Thread::web_url`] point; never the host of an answer.
pub const WEB: &str = "https://github.com";

/// The media type GitHub's REST API asks for.
const ACCEPT: &str = "application/vnd.github+json";

/// The REST API version these requests are written for.
const API_VERSION: &str = "2022-11-28";

/// How many threads one page asks for; GitHub allows 50.
const PER_PAGE: usize = 50;

/// How many pages [`Notifications::list`] reads at most (500 threads). A list with more is not
/// [`Threads::complete`].
pub const MAX_PAGES: usize = 10;

/// The shortest wait between two polls, whatever `X-Poll-Interval` says (GitHub sends 60).
pub const MIN_POLL: Duration = Duration::from_secs(60);

/// How long to wait after a refusal for too many requests that names no time.
const DEFAULT_WAIT: i64 = 60;

/// The longest wait a refusal can ask for; a larger `Retry-After` is cut to this.
const MAX_WAIT: i64 = 24 * 60 * 60;

/// What a client reads instead of GitHub's own message ([`NotificationsError::Github`]).
pub const NOTIFICATIONS_FAILED: &str = "GitHub notifications could not be read. Try again later.";

/// Asking GitHub for one account's notifications: an HTTP client, the REST address
/// (`https://api.github.com`, the GraphQL address without `/graphql`; a fake server in tests) and
/// the account's token.
///
/// The client must send a `User-Agent`, GitHub refuses requests without one. `Debug` shows no
/// token ([`Secret`]).
#[derive(Debug, Clone)]
pub struct Notifications {
    http: reqwest::Client,
    base: String,
    token: Secret,
}

/// What one [`Notifications::list`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing {
    /// GitHub answered `304 Not Modified` to `If-Modified-Since`: nothing changed since the
    /// `last_modified` that was sent, which stays the one to send next time.
    Unchanged {
        /// How long to wait before the next poll ([`Listing::poll`]).
        poll: Duration,
    },
    /// The unread threads as GitHub listed them.
    Read(Threads),
}

/// The unread threads of one [`Notifications::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Threads {
    /// The threads that passed every check, most recently updated first, each id once.
    /// Threads that did not ([`read_threads`]) are left out.
    pub threads: Vec<Thread>,
    /// Every page was read: a thread missing here is no longer unread on GitHub. `false` when
    /// there were more than [`MAX_PAGES`] pages or the next page lay on another address; then a
    /// missing thread says nothing.
    pub complete: bool,
    /// GitHub's `Last-Modified` of the first page, to send as `last_modified` next time; `None`
    /// when GitHub sent none.
    pub last_modified: Option<String>,
    /// How long to wait before the next poll ([`Listing::poll`]).
    pub poll: Duration,
}

impl Listing {
    /// How long to wait before the next poll: GitHub's `X-Poll-Interval` of the first answer, at
    /// least [`MIN_POLL`]; [`MIN_POLL`] without that header.
    pub fn poll(&self) -> Duration {
        match self {
            Listing::Unchanged { poll } | Listing::Read(Threads { poll, .. }) => *poll,
        }
    }
}

/// The id of a notification thread: digits only, at most 20 of them (GitHub's ids fit `u64`).
/// The only way to name a thread to [`Notifications::done`] and [`Notifications::read`], so no
/// other text ever ends up in their address.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ThreadId(String);

impl ThreadId {
    /// `id` as a thread id, or `None` when it is empty, longer than 20 characters or holds
    /// anything but the digits `0`–`9`.
    pub fn parse(id: &str) -> Option<ThreadId> {
        let fits = !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit());
        fits.then(|| ThreadId(id.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One notification thread as checked from GitHub's answer ([`read_threads`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: ThreadId,
    /// The repository as `owner/name`, made of GitHub's name characters only ([`checked_repo`]).
    pub repo: String,
    pub reason: Reason,
    pub subject: Subject,
    /// When the thread last changed; a later value than the one filed means new activity.
    pub updated_at: DateTime<Utc>,
    /// As GitHub sends it; [`Notifications::list`] asks for unread threads only (`all=false`).
    pub unread: bool,
}

/// What a thread is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    /// The title as GitHub sends it: foreign text, shown but never followed.
    pub title: String,
    pub kind: SubjectKind,
    /// The number of the issue or pull request, only from an API address
    /// `<rest>/repos/<repo>/(issues|pulls)/<n>` on the host the token was sent to, of the
    /// thread's own repository and fitting [`Subject::kind`]; `None` otherwise.
    pub number: Option<u64>,
}

/// Why GitHub notified the person: every reason in GitHub's documentation (2026-10), and
/// [`Reason::Other`] for one added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// A Copilot cloud agent session finished.
    AgentSessionFinished,
    /// The person is asked to review and approve a deployment.
    ApprovalRequested,
    /// The person was assigned to the issue.
    Assign,
    /// The person created the thread.
    Author,
    /// A GitHub Actions workflow run the person triggered completed.
    CiActivity,
    /// The person commented on the thread.
    Comment,
    /// The person accepted an invitation to contribute to the repository.
    Invitation,
    /// The person subscribed to the thread.
    Manual,
    /// Organisation members asked to enable a feature such as Copilot.
    MemberFeatureRequested,
    /// The person was @mentioned.
    Mention,
    /// The person, or one of their teams, was asked to review a pull request.
    ReviewRequested,
    /// The person was credited for a security advisory.
    SecurityAdvisoryCredit,
    /// GitHub found a vulnerability in the repository.
    SecurityAlert,
    /// The person changed the thread's state (closed an issue, merged a pull request).
    StateChange,
    /// The person watches the repository.
    Subscribed,
    /// A team of the person was mentioned.
    TeamMention,
    /// A reason this code does not know.
    Other,
}

impl Reason {
    /// Every known reason with GitHub's name for it, in the order of GitHub's documentation.
    pub const ALL: [(Reason, &'static str); 16] = [
        (Reason::AgentSessionFinished, "agent_session_finished"),
        (Reason::ApprovalRequested, "approval_requested"),
        (Reason::Assign, "assign"),
        (Reason::Author, "author"),
        (Reason::CiActivity, "ci_activity"),
        (Reason::Comment, "comment"),
        (Reason::Invitation, "invitation"),
        (Reason::Manual, "manual"),
        (Reason::MemberFeatureRequested, "member_feature_requested"),
        (Reason::Mention, "mention"),
        (Reason::ReviewRequested, "review_requested"),
        (Reason::SecurityAdvisoryCredit, "security_advisory_credit"),
        (Reason::SecurityAlert, "security_alert"),
        (Reason::StateChange, "state_change"),
        (Reason::Subscribed, "subscribed"),
        (Reason::TeamMention, "team_mention"),
    ];

    /// The reason GitHub names `name`; [`Reason::Other`] for a name not in [`Reason::ALL`].
    pub fn from_github(name: &str) -> Reason {
        Reason::ALL.iter().find(|(_, known)| *known == name).map_or(Reason::Other, |(reason, _)| *reason)
    }

    /// GitHub's name of the reason (`review_requested`), `other` for [`Reason::Other`]: the value
    /// of the inbox facet `reason`.
    pub fn as_str(self) -> &'static str {
        Reason::ALL.iter().find(|(reason, _)| *reason == self).map_or("other", |(_, name)| name)
    }
}

/// What kind of thing a thread is about (`subject.type`), [`SubjectKind::Other`] for anything
/// else GitHub sends (Dependabot alert threads, for example).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubjectKind {
    PullRequest,
    Issue,
    /// A workflow run (`ci_activity`); [`Thread::result`] reads how it ended.
    CheckSuite,
    Release,
    Discussion,
    Commit,
    RepositoryVulnerabilityAlert,
    Other,
}

impl SubjectKind {
    /// Every known kind: GitHub's `subject.type` and the snake-case name of [`SubjectKind::as_str`].
    const ALL: [(SubjectKind, &'static str, &'static str); 7] = [
        (SubjectKind::PullRequest, "PullRequest", "pull_request"),
        (SubjectKind::Issue, "Issue", "issue"),
        (SubjectKind::CheckSuite, "CheckSuite", "check_suite"),
        (SubjectKind::Release, "Release", "release"),
        (SubjectKind::Discussion, "Discussion", "discussion"),
        (SubjectKind::Commit, "Commit", "commit"),
        (SubjectKind::RepositoryVulnerabilityAlert, "RepositoryVulnerabilityAlert", "repository_vulnerability_alert"),
    ];

    /// The kind GitHub's `subject.type` names, [`SubjectKind::Other`] for any other.
    pub fn from_github(name: &str) -> SubjectKind {
        SubjectKind::ALL.iter().find(|(_, github, _)| *github == name).map_or(SubjectKind::Other, |(kind, ..)| *kind)
    }

    /// The kind in snake case (`pull_request`, `check_suite`, `other`): the value of the inbox
    /// facet `kind`.
    pub fn as_str(self) -> &'static str {
        SubjectKind::ALL.iter().find(|(kind, ..)| *kind == self).map_or("other", |(.., name)| name)
    }
}

/// How a workflow run ended, read from its title ([`Thread::result`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunResult {
    Failed,
    Succeeded,
    Cancelled,
}

impl RunResult {
    /// `failed`, `succeeded` or `cancelled`: the value of the inbox facet `result`.
    pub fn as_str(self) -> &'static str {
        match self {
            RunResult::Failed => "failed",
            RunResult::Succeeded => "succeeded",
            RunResult::Cancelled => "cancelled",
        }
    }
}

impl Thread {
    /// The page a person opens for this thread, always on [`WEB`] and built only from the checked
    /// repository and number: `/pull/{n}` and `/issues/{n}` for pull requests and issues with a
    /// number, `/actions` for workflow runs, `/releases`, `/discussions`, the repository itself
    /// for anything else (and for an issue or pull request without a number).
    pub fn web_url(&self) -> String {
        let repo = format!("{WEB}/{}", self.repo);
        match (self.subject.kind, self.subject.number) {
            (SubjectKind::PullRequest, Some(n)) => format!("{repo}/pull/{n}"),
            (SubjectKind::Issue, Some(n)) => format!("{repo}/issues/{n}"),
            (SubjectKind::CheckSuite, _) => format!("{repo}/actions"),
            (SubjectKind::Release, _) => format!("{repo}/releases"),
            (SubjectKind::Discussion, _) => format!("{repo}/discussions"),
            _ => repo,
        }
    }

    /// How a workflow run ended, only for [`SubjectKind::CheckSuite`] and only when its title
    /// says it in one word: `failed`, `succeeded` or `cancelled` (also `canceled`), and no other
    /// of them (GitHub titles them "CI workflow run failed for main branch"). `None` for any other
    /// title, so a workflow named "fix failed builds" is never taken for a failure it does not
    /// report alone.
    pub fn result(&self) -> Option<RunResult> {
        if self.subject.kind != SubjectKind::CheckSuite {
            return None;
        }
        let mut found = None;
        for word in self.subject.title.split(|c: char| !c.is_ascii_alphabetic()) {
            let result = match word.to_ascii_lowercase().as_str() {
                "failed" => RunResult::Failed,
                "succeeded" => RunResult::Succeeded,
                "cancelled" | "canceled" => RunResult::Cancelled,
                _ => continue,
            };
            match found {
                Some(seen) if seen != result => return None,
                _ => found = Some(result),
            }
        }
        found
    }
}

/// Why the notifications could not be read or changed. No message holds the token or GitHub's
/// words; logs carry [`NotificationsError::code`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationsError {
    /// GitHub refused the token (HTTP 401): the person has to sign in again.
    Refused,
    /// GitHub answered 403 without naming a rate limit: the token may not read notifications. A
    /// fine-grained token never may (GitHub serves these endpoints to classic tokens only), a
    /// classic one needs the `notifications` or `repo` scope.
    NeedsScope,
    /// Too many requests (403 or 429 with a rate limit): ask again at `until` at the earliest,
    /// from `Retry-After`, else `X-RateLimit-Reset` when no request is left, else a minute.
    Throttled { until: DateTime<Utc> },
    /// GitHub could not be reached.
    Offline,
    /// GitHub answered, but not with what was asked for; the text is this code's own, never
    /// GitHub's, and still only for logs ([`NotificationsError::public_message`]).
    Github(String),
}

impl NotificationsError {
    /// The error code of the API answer (`docs/api.md`).
    pub fn code(&self) -> &'static str {
        match self {
            NotificationsError::Refused => "auth",
            NotificationsError::NeedsScope => "forbidden",
            NotificationsError::Throttled { .. } => "busy",
            NotificationsError::Offline => "offline",
            NotificationsError::Github(_) => "error",
        }
    }

    /// The message for a client key: fixed per code; [`NotificationsError::Github`] becomes
    /// [`NOTIFICATIONS_FAILED`].
    pub fn public_message(&self) -> String {
        match self {
            NotificationsError::Github(_) => NOTIFICATIONS_FAILED.to_string(),
            other => other.to_string(),
        }
    }
}

impl fmt::Display for NotificationsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotificationsError::Refused => f.write_str("GitHub no longer accepts the token. Sign in to GitHub again."),
            NotificationsError::NeedsScope => f.write_str(
                "GitHub does not let this token read notifications. Use a classic token with the notifications scope.",
            ),
            NotificationsError::Throttled { .. } => {
                f.write_str("GitHub asks to wait before reading notifications again. They are read again later.")
            }
            NotificationsError::Offline => f.write_str("GitHub could not be reached. Check the internet connection."),
            NotificationsError::Github(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for NotificationsError {}

impl Notifications {
    /// Notifications of the account whose `token` it is, asked at `rest_base`
    /// (`https://api.github.com`; a trailing `/` is ignored).
    pub fn new(http: reqwest::Client, rest_base: impl Into<String>, token: Secret) -> Notifications {
        let base = rest_base.into().trim_end_matches('/').to_string();
        Notifications { http, base, token }
    }

    /// The unread threads (`GET /notifications?all=false&per_page=50`), up to [`MAX_PAGES`] pages
    /// along `Link: rel="next"`, each thread once.
    ///
    /// - `last_modified`: the [`Threads::last_modified`] of the previous call; sent as
    ///   `If-Modified-Since` with the first page, so GitHub answers [`Listing::Unchanged`] when
    ///   nothing changed (a header value that is not one is not sent).
    /// - `since`: only threads updated after this time (the first poll of an account, E5.4).
    ///
    /// The first failing page fails it all: [`NotificationsError::Refused`] (401),
    /// [`NotificationsError::NeedsScope`] or [`NotificationsError::Throttled`] (403, 429),
    /// [`NotificationsError::Offline`], [`NotificationsError::Github`] for anything else.
    pub async fn list(
        &self,
        last_modified: Option<&str>,
        since: Option<DateTime<Utc>>,
    ) -> Result<Listing, NotificationsError> {
        let mut url = self.first_page(since);
        let mut threads: Vec<Thread> = Vec::new();
        let mut poll = MIN_POLL;
        let mut stamp = None;
        for page in 0..MAX_PAGES {
            let mut request = self.request(Method::GET, &url);
            if page == 0
                && let Some(value) = last_modified.and_then(|v| header::HeaderValue::from_str(v).ok())
            {
                request = request.header(header::IF_MODIFIED_SINCE, value);
            }
            let response = request.send().await.map_err(unreachable)?;
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            if page == 0 {
                poll = poll_interval(&headers);
                if status == 304 && last_modified.is_some() {
                    return Ok(Listing::Unchanged { poll });
                }
                stamp = headers.get(header::LAST_MODIFIED).and_then(|v| v.to_str().ok()).map(str::to_string);
            }
            if status != 200 {
                let body = response.text().await.unwrap_or_default();
                return Err(refusal(status, &headers, &body, Utc::now()));
            }
            let answer: Value = response
                .json()
                .await
                .map_err(|_| NotificationsError::Github("GitHub sent notifications that are not JSON.".into()))?;
            for thread in read_threads(&answer, &self.base)? {
                if !threads.iter().any(|seen| seen.id == thread.id) {
                    threads.push(thread);
                }
            }
            match self.next_page(&headers) {
                Some(next) => url = next,
                None => {
                    let complete = !has_next(&headers);
                    return Ok(Listing::Read(Threads { threads, complete, last_modified: stamp, poll }));
                }
            }
        }
        Ok(Listing::Read(Threads { threads, complete: false, last_modified: stamp, poll }))
    }

    /// Marks the thread `id` done (`DELETE /notifications/threads/{id}`, GitHub answers 204): it
    /// leaves the person's GitHub inbox until new activity brings it back. Errors as in
    /// [`Notifications::list`].
    pub async fn done(&self, id: &ThreadId) -> Result<(), NotificationsError> {
        self.change(Method::DELETE, id).await
    }

    /// Marks the thread `id` read (`PATCH /notifications/threads/{id}`, GitHub answers 205, or
    /// 304 when it already was). Errors as in [`Notifications::list`].
    pub async fn read(&self, id: &ThreadId) -> Result<(), NotificationsError> {
        self.change(Method::PATCH, id).await
    }

    async fn change(&self, method: Method, id: &ThreadId) -> Result<(), NotificationsError> {
        let url = format!("{}/notifications/threads/{id}", self.base);
        let response = self.request(method, &url).send().await.map_err(unreachable)?;
        let status = response.status().as_u16();
        if matches!(status, 200..=299 | 304) {
            return Ok(());
        }
        let headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();
        Err(refusal(status, &headers, &body, Utc::now()))
    }

    /// A request to `url` with the token and the headers GitHub's REST API asks for.
    fn request(&self, method: Method, url: &str) -> RequestBuilder {
        self.http
            .request(method, url)
            .bearer_auth(self.token.expose())
            .header(header::ACCEPT, ACCEPT)
            .header("X-GitHub-Api-Version", API_VERSION)
    }

    fn first_page(&self, since: Option<DateTime<Utc>>) -> String {
        let mut url = format!("{}/notifications?all=false&per_page={PER_PAGE}", self.base);
        if let Some(since) = since {
            url.push_str("&since=");
            url.push_str(&since.to_rfc3339_opts(SecondsFormat::Secs, true));
        }
        url
    }

    /// The `next` page, only when it is another page of this list on the address the token was
    /// made for: the token never goes anywhere else.
    fn next_page(&self, headers: &HeaderMap) -> Option<String> {
        let next = headers.get(header::LINK).and_then(|v| v.to_str().ok()).and_then(next_link)?;
        let own = format!("{}/notifications?", self.base);
        next.starts_with(&own).then(|| next.to_string())
    }
}

/// Whether the answer names a `next` page at all, followed or not.
fn has_next(headers: &HeaderMap) -> bool {
    headers.get(header::LINK).and_then(|v| v.to_str().ok()).and_then(next_link).is_some()
}

/// The address of `rel="next"` in a `Link` header (`<https://…?page=2>; rel="next", <…>;
/// rel="last"`), `None` without one.
pub fn next_link(link: &str) -> Option<&str> {
    link.split(',').find_map(|part| {
        let (target, params) = part.split_once(';')?;
        let next = params.split(';').any(|param| {
            let param = param.trim();
            param == "rel=\"next\"" || param == "rel=next"
        });
        let target = target.trim().strip_prefix('<')?.strip_suffix('>')?;
        next.then_some(target)
    })
}

/// `X-Poll-Interval` in seconds, at least [`MIN_POLL`]; [`MIN_POLL`] without a readable one.
fn poll_interval(headers: &HeaderMap) -> Duration {
    let seconds = headers.get("x-poll-interval").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok());
    seconds.map(Duration::from_secs).unwrap_or(MIN_POLL).max(MIN_POLL)
}

/// The error for a refused request with `status`, read from the headers and, only to tell a
/// secondary rate limit from a missing right, the body; `now` is when it arrived.
fn refusal(status: u16, headers: &HeaderMap, body: &str, now: DateTime<Utc>) -> NotificationsError {
    match status {
        401 => NotificationsError::Refused,
        403 | 429 => match throttled_until(status, headers, body, now) {
            Some(until) => NotificationsError::Throttled { until },
            None => NotificationsError::NeedsScope,
        },
        status => NotificationsError::Github(format!("GitHub answered the notifications with HTTP {status}.")),
    }
}

/// When to ask again after a 403 or 429, or `None` when the 403 is no rate limit. In GitHub's
/// order: `Retry-After` seconds; `X-RateLimit-Reset` (epoch seconds) when `X-RateLimit-Remaining`
/// is 0; a minute for a 429 or a body that speaks of a rate limit. Waits are cut to a day.
fn throttled_until(status: u16, headers: &HeaderMap, body: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let number =
        |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<i64>().ok());
    let after = |seconds: i64| now + TimeDelta::seconds(seconds.clamp(0, MAX_WAIT));
    if let Some(seconds) = number("retry-after") {
        return Some(after(seconds));
    }
    if number("x-ratelimit-remaining") == Some(0)
        && let Some(reset) = number("x-ratelimit-reset")
    {
        // The reset time itself, not `now` plus a rounded difference; one in the past is now.
        let at = DateTime::from_timestamp(reset, 0).unwrap_or(now);
        return Some(at.clamp(now, after(MAX_WAIT)));
    }
    let limit = status == 429 || body.to_ascii_lowercase().contains("rate limit");
    limit.then(|| after(DEFAULT_WAIT))
}

fn unreachable(err: reqwest::Error) -> NotificationsError {
    if err.is_connect() || err.is_timeout() {
        NotificationsError::Offline
    } else {
        NotificationsError::Github(format!("GitHub could not be asked for notifications: {}", err.without_url()))
    }
}

/// A thread as GitHub sends it; only the fields that are read.
#[derive(Deserialize)]
struct RawThread {
    id: String,
    repository: RawRepository,
    subject: RawSubject,
    reason: String,
    unread: bool,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct RawRepository {
    full_name: String,
}

#[derive(Deserialize)]
struct RawSubject {
    title: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(rename = "type")]
    kind: String,
}

/// The threads of one page of `GET /notifications` (a JSON array). An entry is left out when it
/// lacks a field, its id is no [`ThreadId`] or its repository no `owner/name` ([`checked_repo`]).
/// `api` is the REST address the page came from: a subject's number is read only from an address
/// there ([`Subject::number`]). [`NotificationsError::Github`] when the page is no array.
pub fn read_threads(answer: &Value, api: &str) -> Result<Vec<Thread>, NotificationsError> {
    let entries = answer
        .as_array()
        .ok_or_else(|| NotificationsError::Github("GitHub sent notifications that are not a list.".into()))?;
    let api = api.trim_end_matches('/');
    Ok(entries.iter().filter_map(|entry| read_thread(entry, api)).collect())
}

fn read_thread(entry: &Value, api: &str) -> Option<Thread> {
    let raw = RawThread::deserialize(entry).ok()?;
    let id = ThreadId::parse(&raw.id)?;
    let repo = checked_repo(&raw.repository.full_name)?;
    let kind = SubjectKind::from_github(&raw.subject.kind);
    let number = raw.subject.url.as_deref().and_then(|url| subject_number(url, api, &repo, kind));
    Some(Thread {
        id,
        repo,
        reason: Reason::from_github(&raw.reason),
        subject: Subject { title: raw.subject.title, kind, number },
        updated_at: raw.updated_at,
        unread: raw.unread,
    })
}

/// `repo` when it is `owner/name` as GitHub names them: an owner of letters, digits and `-` (at
/// most 39), a name of letters, digits, `-`, `_` and `.` (at most 100, not `.` or `..`). `None`
/// otherwise, so a link built from it stays inside the repository's pages.
pub fn checked_repo(repo: &str) -> Option<String> {
    let (owner, name) = repo.split_once('/')?;
    let owner_fits =
        !owner.is_empty() && owner.len() <= 39 && owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let name_fits = !name.is_empty()
        && name.len() <= 100
        && name != "."
        && name != ".."
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    (owner_fits && name_fits).then(|| repo.to_string())
}

/// The number in `url` when it is exactly `<api>/repos/<repo>/issues/<n>` for an issue or
/// `<api>/repos/<repo>/pulls/<n>` for a pull request (repository compared without case, as
/// GitHub does), `n` a positive number.
fn subject_number(url: &str, api: &str, repo: &str, kind: SubjectKind) -> Option<u64> {
    let segment = match kind {
        SubjectKind::Issue => "issues",
        SubjectKind::PullRequest => "pulls",
        _ => return None,
    };
    let rest = url.strip_prefix(api)?.strip_prefix("/repos/")?;
    let parts: Vec<&str> = rest.split('/').collect();
    let [owner, name, found, number] = parts.as_slice() else {
        return None;
    };
    let same_repo = format!("{owner}/{name}").eq_ignore_ascii_case(repo);
    if !same_repo || *found != segment || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok().filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use httpmock::Method::{DELETE, GET, PATCH};
    use httpmock::MockServer;

    use super::*;

    const API: &str = "https://api.github.com";

    fn fixture(name: &str) -> &'static str {
        match name {
            "reasons" => include_str!("../tests/fixtures/notifications/reasons.json"),
            "kinds" => include_str!("../tests/fixtures/notifications/kinds.json"),
            "invalid" => include_str!("../tests/fixtures/notifications/invalid.json"),
            "page1" => include_str!("../tests/fixtures/notifications/page1.json"),
            "page2" => include_str!("../tests/fixtures/notifications/page2.json"),
            other => panic!("no fixture {other}"),
        }
    }

    fn threads(name: &str) -> Vec<Thread> {
        let answer: Value = serde_json::from_str(fixture(name)).expect("fixture is JSON");
        read_threads(&answer, API).expect("a list")
    }

    /// A fixture as the fake server sends it: its API addresses on the fake server.
    fn served(server: &MockServer, name: &str) -> String {
        fixture(name).replace(API, &server.base_url())
    }

    fn client(server: &MockServer) -> Notifications {
        Notifications::new(reqwest::Client::new(), server.base_url(), Secret::new("ghp_SECRET"))
    }

    fn read(listing: Listing) -> Threads {
        match listing {
            Listing::Read(threads) => threads,
            Listing::Unchanged { .. } => panic!("expected threads, got 304"),
        }
    }

    #[test]
    fn every_documented_reason_is_read_and_an_unknown_one_is_other() {
        let threads = threads("reasons");
        let reasons: Vec<Reason> = threads.iter().map(|thread| thread.reason).collect();
        let mut expected: Vec<Reason> = Reason::ALL.iter().map(|(reason, _)| *reason).collect();
        expected.push(Reason::Other);
        assert_eq!(reasons, expected);
        for (thread, (_, name)) in threads.iter().zip(Reason::ALL) {
            assert_eq!(thread.reason.as_str(), name);
            assert_eq!(thread.subject.title, format!("Issue for {name}"));
        }
        assert_eq!(Reason::Other.as_str(), "other");
    }

    #[test]
    fn every_kind_is_read_with_its_number_and_its_page() {
        let threads = threads("kinds");
        let seen: Vec<(SubjectKind, Option<u64>, String)> =
            threads.iter().map(|t| (t.subject.kind, t.subject.number, t.web_url())).collect();
        let repo = "https://github.com/example-org/example-app";
        assert_eq!(
            seen,
            [
                (SubjectKind::PullRequest, Some(42), format!("{repo}/pull/42")),
                (SubjectKind::Issue, Some(7), format!("{repo}/issues/7")),
                (SubjectKind::CheckSuite, None, format!("{repo}/actions")),
                (SubjectKind::Release, None, format!("{repo}/releases")),
                (SubjectKind::Discussion, None, format!("{repo}/discussions")),
                (SubjectKind::Commit, None, repo.to_string()),
                (SubjectKind::RepositoryVulnerabilityAlert, None, repo.to_string()),
                (SubjectKind::Other, None, repo.to_string()),
            ]
        );
        let names: Vec<&str> = threads.iter().map(|t| t.subject.kind.as_str()).collect();
        assert_eq!(
            names,
            [
                "pull_request",
                "issue",
                "check_suite",
                "release",
                "discussion",
                "commit",
                "repository_vulnerability_alert",
                "other"
            ]
        );
        let first = &threads[0];
        assert_eq!((first.id.as_str(), first.repo.as_str()), ("2000", "example-org/example-app"));
        assert_eq!(first.updated_at, "2026-10-13T09:30:00Z".parse::<DateTime<Utc>>().expect("time"));
        assert_eq!(
            threads.iter().map(|t| t.unread).collect::<Vec<_>>(),
            [true, false, true, false, true, false, true, false]
        );
        assert_eq!(first.reason, Reason::ReviewRequested);
    }

    #[test]
    fn wrong_ids_and_repositories_are_dropped_and_no_foreign_address_gives_a_number() {
        let threads = threads("invalid");
        let ids: Vec<&str> = threads.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["3000", "3006", "3007", "3008", "3009"], "bad ids, bad repositories and broken entries go");
        assert_eq!(threads[0].subject.number, Some(5));
        for thread in &threads[1..] {
            assert_eq!(thread.subject.number, None, "{}", thread.subject.title);
            assert_eq!(thread.web_url(), "https://github.com/example-org/example-app", "{}", thread.subject.title);
            assert!(!thread.web_url().contains("evil"));
        }
    }

    #[test]
    fn ids_and_repositories_are_checked_strictly() {
        for good in ["1", "12345678901234567890"] {
            assert!(ThreadId::parse(good).is_some(), "{good}");
        }
        for bad in ["", "123456789012345678901", "12a", "-1", " 1", "1/2", "１"] {
            assert!(ThreadId::parse(bad).is_none(), "{bad}");
        }
        for good in ["mahype/TorroWorkplace", "my-org/my_repo.rs", "a/.github"] {
            assert_eq!(checked_repo(good).as_deref(), Some(good));
        }
        for bad in ["mahype", "mahype/", "/x", "a/b/c", "a b/c", "a/..", "a/.", "my_org/x", "a/b?c", "a/b#c"] {
            assert_eq!(checked_repo(bad), None, "{bad}");
        }
    }

    fn check_suite(title: &str, kind: SubjectKind) -> Thread {
        Thread {
            id: ThreadId("1".into()),
            repo: "example-org/example-app".into(),
            reason: Reason::CiActivity,
            subject: Subject { title: title.into(), kind, number: None },
            updated_at: DateTime::<Utc>::UNIX_EPOCH,
            unread: true,
        }
    }

    #[test]
    fn a_run_result_comes_only_from_a_title_that_says_it_alone() {
        let cases = [
            ("CI workflow run failed for main branch", Some(RunResult::Failed)),
            ("Deploy workflow run succeeded for main branch", Some(RunResult::Succeeded)),
            ("CI workflow run cancelled for feat/x branch", Some(RunResult::Cancelled)),
            ("CI workflow run canceled for main branch", Some(RunResult::Cancelled)),
            ("Retry failed jobs workflow run succeeded for main branch", None),
            ("CI workflow run completed for main branch", None),
            ("unfailedness", None),
        ];
        for (title, expected) in cases {
            assert_eq!(check_suite(title, SubjectKind::CheckSuite).result(), expected, "{title}");
        }
        assert_eq!(check_suite("Login failed on Windows", SubjectKind::Issue).result(), None);
        assert_eq!(RunResult::Failed.as_str(), "failed");
    }

    #[test]
    fn the_next_link_is_found_among_the_others() {
        let link = r#"<https://api.github.com/notifications?page=2>; rel="next", <https://api.github.com/notifications?page=5>; rel="last""#;
        assert_eq!(next_link(link), Some("https://api.github.com/notifications?page=2"));
        let last = r#"<https://api.github.com/notifications?page=1>; rel="prev", <https://api.github.com/notifications?page=1>; rel="first""#;
        assert_eq!(next_link(last), None);
        assert_eq!(next_link(""), None);
    }

    #[tokio::test]
    async fn pages_are_followed_with_the_headers_github_asks_for() {
        let server = MockServer::start_async().await;
        let next = format!("<{}/notifications?all=false&per_page=50&page=2>; rel=\"next\"", server.base_url());
        let first = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/notifications")
                    .query_param("all", "false")
                    .query_param("per_page", "50")
                    .query_param("since", "2026-10-10T08:00:00Z")
                    .query_param_missing("page")
                    .header("authorization", "Bearer ghp_SECRET")
                    .header("accept", "application/vnd.github+json")
                    .header("x-github-api-version", "2022-11-28");
                then.status(200)
                    .header("Last-Modified", "Tue, 13 Oct 2026 10:00:00 GMT")
                    .header("X-Poll-Interval", "120")
                    .header("Link", next.as_str())
                    .body(served(&server, "page1"));
            })
            .await;
        let second = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/notifications")
                    .query_param("page", "2")
                    .header("authorization", "Bearer ghp_SECRET");
                then.status(200).header("X-Poll-Interval", "30").body(served(&server, "page2"));
            })
            .await;
        let since = "2026-10-10T08:00:00Z".parse::<DateTime<Utc>>().expect("time");
        let threads = read(client(&server).list(None, Some(since)).await.expect("listed"));
        assert_eq!((first.calls_async().await, second.calls_async().await), (1, 1));
        let ids: Vec<&str> = threads.threads.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["4001", "4002", "4003"], "each thread once");
        assert!(threads.complete);
        assert_eq!(threads.last_modified.as_deref(), Some("Tue, 13 Oct 2026 10:00:00 GMT"));
        assert_eq!(threads.poll, Duration::from_secs(120), "the first answer's interval counts");
        assert_eq!(threads.threads[0].subject.number, Some(42), "numbers from the server the token went to");
        assert_eq!(threads.threads[2].result(), Some(RunResult::Succeeded));
    }

    #[tokio::test]
    async fn a_list_longer_than_ten_pages_is_not_complete() {
        let server = MockServer::start_async().await;
        let next = format!("<{}/notifications?all=false&per_page=50&page=2>; rel=\"next\"", server.base_url());
        let pages = server
            .mock_async(|when, then| {
                when.method(GET).path("/notifications");
                then.status(200).header("Link", next.as_str()).body(served(&server, "page1"));
            })
            .await;
        let threads = read(client(&server).list(None, None).await.expect("listed"));
        assert_eq!(pages.calls_async().await, MAX_PAGES);
        assert!(!threads.complete);
        assert_eq!(threads.threads.len(), 2);
        assert_eq!(threads.poll, MIN_POLL, "no interval sent");
    }

    #[tokio::test]
    async fn a_next_page_on_another_address_is_not_asked_with_the_token() {
        let server = MockServer::start_async().await;
        let elsewhere = MockServer::start_async().await;
        let away = elsewhere
            .mock_async(|when, then| {
                when.any_request();
                then.status(200).body("[]");
            })
            .await;
        let next = format!("<{}/notifications?page=2>; rel=\"next\"", elsewhere.base_url());
        server
            .mock_async(|when, then| {
                when.method(GET).path("/notifications");
                then.status(200).header("Link", next.as_str()).body(served(&server, "page1"));
            })
            .await;
        let threads = read(client(&server).list(None, None).await.expect("listed"));
        assert_eq!(away.calls_async().await, 0);
        assert!(!threads.complete, "a page left unread says nothing about missing threads");
        assert_eq!(threads.threads.len(), 2);
    }

    #[tokio::test]
    async fn if_modified_since_gets_a_304_and_its_poll_interval() {
        let server = MockServer::start_async().await;
        let unchanged = server
            .mock_async(|when, then| {
                when.method(GET).path("/notifications").header("if-modified-since", "Tue, 13 Oct 2026 10:00:00 GMT");
                then.status(304).header("X-Poll-Interval", "90");
            })
            .await;
        let listing = client(&server).list(Some("Tue, 13 Oct 2026 10:00:00 GMT"), None).await.expect("listed");
        assert_eq!(listing, Listing::Unchanged { poll: Duration::from_secs(90) });
        assert_eq!(listing.poll(), Duration::from_secs(90));
        assert_eq!(unchanged.calls_async().await, 1);
    }

    #[tokio::test]
    async fn a_short_poll_interval_is_raised_to_a_minute() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/notifications").header_missing("if-modified-since");
                then.status(200).header("X-Poll-Interval", "5").body("[]");
            })
            .await;
        let listing = client(&server).list(None, None).await.expect("listed");
        assert_eq!(listing.poll(), MIN_POLL);
        assert_eq!(read(listing), Threads { threads: vec![], complete: true, last_modified: None, poll: MIN_POLL });
    }

    async fn refused(status: u16, headers: &[(&str, &str)], body: &str) -> NotificationsError {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/notifications");
                let mut then = then.status(status).body(body);
                for (name, value) in headers {
                    then = then.header(*name, *value);
                }
            })
            .await;
        client(&server).list(None, None).await.expect_err("refused")
    }

    #[tokio::test]
    async fn a_refused_token_a_missing_right_and_rate_limits_are_told_apart() {
        assert_eq!(refused(401, &[], r#"{"message":"Bad credentials"}"#).await, NotificationsError::Refused);
        let fine_grained = r#"{"message":"Resource not accessible by personal access token","status":"403"}"#;
        let refused_scope = refused(403, &[("X-RateLimit-Remaining", "4999")], fine_grained).await;
        assert_eq!(refused_scope, NotificationsError::NeedsScope);
        assert!(!refused_scope.to_string().contains("Resource not accessible"), "GitHub's words stay out");

        let before = Utc::now();
        let NotificationsError::Throttled { until } = refused(429, &[("Retry-After", "120")], "").await else {
            panic!("429 is a rate limit");
        };
        assert!(until >= before + TimeDelta::seconds(120) && until <= Utc::now() + TimeDelta::seconds(120));

        let reset = (Utc::now() + TimeDelta::seconds(600)).timestamp();
        let reset_text = reset.to_string();
        let limited = refused(
            403,
            &[("X-RateLimit-Remaining", "0"), ("X-RateLimit-Reset", reset_text.as_str())],
            r#"{"message":"API rate limit exceeded"}"#,
        )
        .await;
        assert_eq!(limited, NotificationsError::Throttled { until: DateTime::from_timestamp(reset, 0).expect("time") });
        let other = refused(500, &[], "boom").await;
        assert_eq!((other.code(), other.public_message()), ("error", NOTIFICATIONS_FAILED.to_string()));
    }

    #[test]
    fn a_wait_is_read_in_githubs_order() {
        let now = "2026-10-13T10:00:00Z".parse::<DateTime<Utc>>().expect("time");
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut map = HeaderMap::new();
            for (name, value) in pairs {
                map.insert(*name, header::HeaderValue::from_static(value));
            }
            map
        };
        let wait = |status, pairs: &[(&'static str, &'static str)], body| refusal(status, &headers(pairs), body, now);
        let at = |seconds| NotificationsError::Throttled { until: now + TimeDelta::seconds(seconds) };
        assert_eq!(wait(429, &[("retry-after", "30")], ""), at(30));
        assert_eq!(wait(403, &[("retry-after", "30"), ("x-ratelimit-remaining", "0")], ""), at(30));
        assert_eq!(wait(429, &[], ""), at(60));
        assert_eq!(wait(403, &[], "You have exceeded a secondary rate limit"), at(60));
        assert_eq!(wait(429, &[("retry-after", "999999999")], ""), at(MAX_WAIT));
        assert_eq!(wait(403, &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1")], ""), at(0));
        assert_eq!(wait(403, &[("x-ratelimit-remaining", "12")], "Forbidden"), NotificationsError::NeedsScope);
    }

    #[tokio::test]
    async fn done_deletes_the_thread_and_read_patches_it() {
        let server = MockServer::start_async().await;
        let deleted = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/notifications/threads/4001")
                    .header("authorization", "Bearer ghp_SECRET")
                    .header("accept", "application/vnd.github+json")
                    .header("x-github-api-version", "2022-11-28");
                then.status(204);
            })
            .await;
        let patched = server
            .mock_async(|when, then| {
                when.method(PATCH).path("/notifications/threads/4001").header("authorization", "Bearer ghp_SECRET");
                then.status(205);
            })
            .await;
        let id = ThreadId::parse("4001").expect("id");
        let notifications = client(&server);
        assert_eq!(notifications.done(&id).await, Ok(()));
        assert_eq!(notifications.read(&id).await, Ok(()));
        assert_eq!((deleted.calls_async().await, patched.calls_async().await), (1, 1));
    }

    #[tokio::test]
    async fn a_refused_done_says_why() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(DELETE).path("/notifications/threads/1");
                then.status(401);
            })
            .await;
        let id = ThreadId::parse("1").expect("id");
        assert_eq!(client(&server).done(&id).await, Err(NotificationsError::Refused));
    }

    #[tokio::test]
    async fn an_unreachable_github_is_offline() {
        let notifications = Notifications::new(reqwest::Client::new(), "http://127.0.0.1:9", Secret::new("ghp_SECRET"));
        assert_eq!(notifications.list(None, None).await, Err(NotificationsError::Offline));
    }

    #[test]
    fn the_token_shows_nowhere() {
        let notifications =
            Notifications::new(reqwest::Client::new(), "https://api.github.com/", Secret::new("ghp_SECRET"));
        assert!(!format!("{notifications:?}").contains("SECRET"));
        assert!(format!("{notifications:?}").contains("https://api.github.com\""), "trailing slash dropped");
        let errors = [
            NotificationsError::Refused,
            NotificationsError::NeedsScope,
            NotificationsError::Throttled { until: DateTime::<Utc>::UNIX_EPOCH },
            NotificationsError::Offline,
            NotificationsError::Github("GitHub answered the notifications with HTTP 500.".into()),
        ];
        for error in errors {
            assert!(!format!("{error} {error:?} {}", error.public_message()).contains("SECRET"));
        }
        assert_eq!(NotificationsError::NeedsScope.code(), "forbidden");
        assert_eq!(NotificationsError::Throttled { until: DateTime::<Utc>::UNIX_EPOCH }.code(), "busy");
    }
}
