//! GitHub's notifications as a source of the inbox (task E5.4): one entry per notification thread
//! of each GitHub account, source `github:<login>`, the account's id.
//!
//! **Reading** ([`start`], [`Poller`]): one loop for all accounts looks every [`TICK`] which account
//! is due and asks GitHub for its unread threads ([`Notifications::list`]), at most as often as
//! GitHub's `X-Poll-Interval` says and never more often than once a minute
//! ([`torro_pim_github::notifications::MIN_POLL`]). `Last-Modified` is kept in the inbox's cursors
//! ([`LAST_MODIFIED`]), so an unchanged inbox costs a `304` that GitHub does not count. Every
//! listing reaches back only to [`FIRST_DAYS`] days before the account's first one (question 4 of
//! E5, kept in [`FIRST_POLL`]), so connecting an account does not flood the inbox with old
//! notifications, neither then nor once something changes; an old thread with new activity comes
//! in as new. A muted source asks GitHub nothing.
//!
//! **Filing** ([`report`]): matter and id are the thread's id; the title is the subject's (foreign
//! text), there is no text (the app builds its line from the traits); the traits are `repo`,
//! `reason`, `kind` and, for a workflow run whose title says it, `result`; kind `notice` in the
//! lane of `rules::github_place` (never urgent, question 3 of E5); the link is
//! [`Thread::web_url`], built from the checked parts on github.com, never taken from the answer.
//! A thread updated later than filed is new activity: the inbox counts it once more and opens a
//! done entry again (`Inbox::file_remote`).
//!
//! **Done by itself:** a thread missing from a complete listing was read or done elsewhere, its
//! entry is resolved (`Inbox::resolve_remote`). Every entry was filed from within the same reach,
//! and a thread's `updated_at` only grows, so the reach leaves out none of them. A listing that is
//! not complete (too many pages) or that is the account's first resolves nothing: a missing thread
//! says nothing there (the first may meet entries of a reach forgotten since).
//!
//! **Reaching back** ([`settle_done`]): done here marks the thread done on GitHub (`DELETE
//! /notifications/threads/{id}`, question 1 of E5), once per time the entry became done
//! ([`Settled`]); undo changes nothing on GitHub.
//!
//! **Health** (`inbox.list_sources`): `signed_out` while the account needs a new sign-in (GitHub
//! refused the token; the account problem itself comes from `GithubAccounts::check_all` as before,
//! which a refusal here starts), `missing_scope` while the token may not read notifications (a
//! fine-grained token, question 5 of E5: no entries then), `offline` while GitHub cannot be
//! reached; cleared by the next listing that works.
//!
//! An account that is no longer there ends its part of the loop: its open entries are done
//! (`Inbox::retire_remote`), its statistics and cursors go (`Inbox::forget_source`; the core does
//! the same when it removes the account).
//!
//! Logs name the account's id and an error code, never a title, a repository or a thread id.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use torro_inbox::item::{Facet, FacetKey, SourceHealth};
use torro_inbox::{Item, Kind, SourceKind, SourceRef, State, rules};
use torro_ipc::{Request, Response};
use torro_pim_github::notifications::{self, Listing, NotificationsError, RunResult, Thread, ThreadId, Threads};

use super::Sources;
use crate::account::github::GithubAccounts;
use crate::inbox::{Health, Inbox, Remote};

/// How often the loop looks whether an account is due. Short, so a new sign-in or an unmuted
/// source is read soon; it asks GitHub only when an account's own pace allows.
const TICK: Duration = Duration::from_secs(10);

/// How many days before an account's first listing every listing of it reaches back (question 4
/// of E5): GitHub's `since`.
const FIRST_DAYS: i64 = 3;

/// How long to wait before asking again after GitHub refused the token or its scope. A new
/// sign-in is read at once anyway ([`Poller::round`]); this only keeps a token GitHub keeps
/// refusing from being sent every minute.
const RETRY_REFUSED: Duration = Duration::from_secs(15 * 60);

/// The cursor that holds GitHub's `Last-Modified` of the last listing.
const LAST_MODIFIED: &str = "last-modified";

/// The cursor that holds when the account was first listed (RFC 3339): every listing reaches back
/// to [`FIRST_DAYS`] before it. Without it the next listing is the first one.
const FIRST_POLL: &str = "first-poll";

/// How long after an entry became done its notification may still be marked done on GitHub
/// ([`Settled`]): the core calls [`settle_done`] right after the command that did it.
const SETTLE_FRESH: Duration = Duration::from_secs(60);

/// Starts the notifications of every GitHub account on the current runtime and returns at once:
/// one loop ([`Poller`]) every [`TICK`].
pub(super) fn start(sources: Sources) {
    tokio::spawn(async move {
        let mut poller = Poller::default();
        loop {
            poller.round(&sources, Utc::now()).await;
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Marks the threads `ids` of the done entry `item` done on GitHub (`DELETE
/// /notifications/threads/{id}`, question 1 of E5), in the background: the calls are spawned on the
/// current runtime and it returns at once. Only when the entry has just become done
/// ([`Settled::first`]): marking an entry done that was done already, or making a task of it then,
/// sends nothing again. Ids that are no thread id are skipped. Failures only to the log, with the
/// account and a code.
pub(super) fn settle_done(sources: &Sources, item: &Item, ids: Vec<String>) {
    let State::Done { at } = item.state else { return };
    if !SETTLED.first(&item.id, at, Utc::now()) {
        return;
    }
    let threads: Vec<ThreadId> = ids.iter().filter_map(|id| ThreadId::parse(id)).collect();
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    let (github, account) = (Arc::clone(&sources.github), item.source.key());
    runtime.spawn(async move { mark_done(&github, &account, &threads).await });
}

/// Marks `threads` of `account` done on GitHub, one `DELETE` each; failures go to the log.
async fn mark_done(github: &GithubAccounts, account: &str, threads: &[ThreadId]) {
    let Some(api) = github.notifications(account).await else {
        eprintln!("torroworkplace: inbox: no GitHub token for {account}, its notification stays on GitHub");
        return;
    };
    for thread in threads {
        if let Err(err) = api.done(thread).await {
            eprintln!("torroworkplace: inbox: a GitHub notification of {account} was not marked done: {}", err.code());
        }
    }
}

/// The entries whose notifications were marked done lately, with the time each became done. An
/// entry marked done again keeps that time (`rules::set_done`), so the same time again means "done
/// already, told already"; done anew after undo is a new time, and GitHub is told again. Kept in
/// memory and only for [`SETTLE_FRESH`]; an entry done longer ago was told then. So only an entry
/// done again within that minute after a restart of the core is told twice, which GitHub takes as
/// done once more.
struct Settled(Mutex<Vec<(String, DateTime<Utc>)>>);

/// The entries of every account told lately ([`settle_done`]).
static SETTLED: Settled = Settled(Mutex::new(Vec::new()));

impl Settled {
    /// Whether the entry `id`, done at `at`, should reach GitHub now (`now`): it became done no
    /// longer than [`SETTLE_FRESH`] ago, and not at the same `at` as told before. Notes it then.
    fn first(&self, id: &str, at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        let fresh = |done: DateTime<Utc>| now.signed_duration_since(done) <= delta(SETTLE_FRESH);
        if !fresh(at) {
            return false;
        }
        let mut told = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        told.retain(|(_, done)| fresh(*done));
        if told.iter().any(|(entry, done)| entry == id && *done == at) {
            return false;
        }
        told.push((id.to_string(), at));
        true
    }
}

/// The loop's memory: when each account may be asked next, and which accounts it saw last.
#[derive(Debug, Default)]
struct Poller {
    /// Per account id.
    paces: BTreeMap<String, Pace>,
    /// The accounts of the last round; `None` before the first, so the first round retires the
    /// entries of accounts removed while the core was not running.
    known: Option<Vec<String>>,
}

/// How one account is asked.
#[derive(Debug, Default)]
struct Pace {
    /// Not before this; `None` asks in the next round.
    next: Option<DateTime<Utc>>,
    /// The account needed a new sign-in when last seen: once it no longer does, it is read at once.
    signed_out: bool,
}

impl Poller {
    /// One round at `now`: nothing until the GitHub accounts are loaded; the entries of accounts
    /// gone are retired ([`Poller::retire`]); then every account that is due is read
    /// ([`poll`]), except one that needs a new sign-in (health `signed_out`, nothing is sent).
    async fn round(&mut self, sources: &Sources, now: DateTime<Utc>) {
        let Some(checks) = sources.github.account_checks().await else { return };
        let active: Vec<String> = checks.iter().map(|check| check.account.clone()).collect();
        self.retire(sources, &active, now);
        for check in checks {
            let Ok(source) = check.account.parse::<SourceRef>() else { continue };
            let pace = self.paces.entry(check.account.clone()).or_default();
            if matches!(check.health, Health::Problem { .. }) {
                pace.signed_out = true;
                sources.inbox.set_source_health(&source, Some(SourceHealth::SignedOut));
                continue;
            }
            // Signed in again: read now, whatever the pace said.
            if std::mem::take(&mut pace.signed_out) {
                pace.next = None;
            }
            if pace.next.is_some_and(|next| next > now) {
                continue;
            }
            let wait = poll(sources, &source, now).await;
            pace.next = Some(now + delta(wait));
        }
    }

    /// Notes `active` as the accounts there are. On the first round, and whenever an account is
    /// gone, the open entries of every GitHub source not in `active` are done; a gone account's
    /// statistics, cursors and health go with it, and so does its pace.
    fn retire(&mut self, sources: &Sources, active: &[String], now: DateTime<Utc>) {
        let gone: Vec<String> = match &self.known {
            Some(known) => known.iter().filter(|id| !active.contains(id)).cloned().collect(),
            None => Vec::new(),
        };
        if (self.known.is_none() || !gone.is_empty())
            && let Err(err) = sources.inbox.retire_remote(SourceKind::Github, active, now)
        {
            eprintln!("torroworkplace: inbox: the entries of a removed GitHub account stay open: {err}");
        }
        for id in gone {
            if let Err(err) = sources.inbox.forget_source(&id) {
                eprintln!("torroworkplace: inbox: the statistics of the removed GitHub account {id} stay: {err}");
            }
            if let Ok(source) = id.parse::<SourceRef>() {
                sources.inbox.set_source_health(&source, None);
            }
            self.paces.remove(&id);
        }
        self.known = Some(active.to_vec());
    }
}

/// Reads the unread threads of the account of `source` at `now` and files them ([`file`]);
/// answers how long to wait before the next listing: GitHub's poll interval, the time a rate limit
/// names, or [`RETRY_REFUSED`] after a refusal. A muted source and an account without a token
/// are not asked (the next round looks again after a minute).
async fn poll(sources: &Sources, source: &SourceRef, now: DateTime<Utc>) -> Duration {
    let inbox = &sources.inbox;
    if muted(inbox, source, now) {
        return notifications::MIN_POLL;
    }
    let account = source.key();
    let Some(api) = sources.github.notifications(&account).await else { return notifications::MIN_POLL };
    let started = cursor(inbox, source, FIRST_POLL).and_then(|stamp| stamp.parse::<DateTime<Utc>>().ok());
    let reach = Reach { since: started.unwrap_or(now) - TimeDelta::days(FIRST_DAYS), first: started.is_none() };
    let last_modified = cursor(inbox, source, LAST_MODIFIED);
    match api.list(last_modified.as_deref(), Some(reach.since)).await {
        Ok(Listing::Unchanged { poll }) => {
            inbox.set_source_health(source, None);
            poll
        }
        Ok(Listing::Read(threads)) => {
            inbox.set_source_health(source, None);
            let poll = threads.poll;
            file(inbox, source, threads, reach, now);
            poll
        }
        Err(err) => refused(sources, source, err, now).await,
    }
}

/// What GitHub's refusal `err` means for the account of `source` at `now`: its health, and how long
/// to wait. A refused token has the accounts checked again (`GithubAccounts::check_all`), which
/// makes it the account's problem as before, and the account waits for a new sign-in.
async fn refused(sources: &Sources, source: &SourceRef, err: NotificationsError, now: DateTime<Utc>) -> Duration {
    let account = source.key();
    eprintln!("torroworkplace: inbox: the GitHub notifications of {account} were not read: {}", err.code());
    let health = |health| sources.inbox.set_source_health(source, Some(health));
    match err {
        NotificationsError::Refused => {
            health(SourceHealth::SignedOut);
            if let Err(err) = sources.github.check_all().await {
                eprintln!("torroworkplace: inbox: the GitHub accounts were checked again: {}", err.code());
            }
            RETRY_REFUSED
        }
        NotificationsError::NeedsScope => {
            health(SourceHealth::MissingScope);
            RETRY_REFUSED
        }
        NotificationsError::Throttled { until } => {
            until.signed_duration_since(now).to_std().unwrap_or_default().max(notifications::MIN_POLL)
        }
        NotificationsError::Offline => {
            health(SourceHealth::Offline);
            notifications::MIN_POLL
        }
        NotificationsError::Github(_) => notifications::MIN_POLL,
    }
}

/// How far back a listing of an account reaches ([`FIRST_DAYS`]).
#[derive(Debug, Clone, Copy)]
struct Reach {
    /// Only threads updated since then: GitHub's `since`, and checked again here.
    since: DateTime<Utc>,
    /// It is the account's first listing ([`FIRST_POLL`]).
    first: bool,
}

/// Files the listed `threads` of `source` at `now`, oldest first – those updated since
/// `reach.since` only, whatever GitHub sent –, and resolves the open entries whose threads are
/// missing when the listing is complete and not the first. The cursors move on only when every
/// thread was filed, so a thread the inbox could not take is listed again instead of hidden behind
/// a `304`.
fn file(inbox: &Inbox, source: &SourceRef, threads: Threads, reach: Reach, now: DateTime<Utc>) {
    let account = source.key();
    let mut filed = true;
    for thread in threads.threads.iter().rev().filter(|thread| thread.updated_at >= reach.since) {
        if let Err(err) = inbox.file_remote(&report(source, thread), now) {
            eprintln!("torroworkplace: inbox: a GitHub notification of {account} was not filed: {err}");
            filed = false;
        }
    }
    if threads.complete && !reach.first {
        resolve_missing(inbox, source, &threads.threads, now);
    }
    if !filed {
        return;
    }
    let mut moved = inbox.set_cursor(source, LAST_MODIFIED, threads.last_modified.as_deref());
    if reach.first {
        let stamp = now.to_rfc3339_opts(SecondsFormat::Secs, true);
        moved = moved.and_then(|()| inbox.set_cursor(source, FIRST_POLL, Some(&stamp)));
    }
    if let Err(err) = moved {
        eprintln!("torroworkplace: inbox: where the GitHub notifications of {account} stopped was not kept: {err}");
    }
}

/// Resolves the ids of the open entries of `source` that the complete listing `listed` no longer
/// holds: read or done elsewhere.
fn resolve_missing(inbox: &Inbox, source: &SourceRef, listed: &[Thread], now: DateTime<Utc>) {
    let listed: BTreeSet<&str> = listed.iter().map(|thread| thread.id.as_str()).collect();
    let gone: Vec<String> = match inbox.open_remote(source) {
        Ok(open) => open.into_iter().flat_map(|entry| entry.ids).filter(|id| !listed.contains(id.as_str())).collect(),
        Err(err) => {
            eprintln!("torroworkplace: inbox: the open GitHub entries of {} were not read: {err}", source.key());
            return;
        }
    };
    if gone.is_empty() {
        return;
    }
    if let Err(err) = inbox.resolve_remote(source, &gone, now) {
        eprintln!("torroworkplace: inbox: GitHub notifications of {} read elsewhere stay open: {err}", source.key());
    }
}

/// The report of `thread` for the inbox (see the module docs): everything but `source` comes from
/// GitHub, checked by `torro_pim_github::notifications`.
fn report(source: &SourceRef, thread: &Thread) -> Remote {
    let (reason, kind) = (thread.reason.as_str(), thread.subject.kind.as_str());
    let result = thread.result().map(RunResult::as_str);
    let facet = |key, value: &str| Facet { key, value: value.to_string() };
    let mut facets =
        vec![facet(FacetKey::Repo, &thread.repo), facet(FacetKey::Reason, reason), facet(FacetKey::Kind, kind)];
    facets.extend(result.map(|result| facet(FacetKey::Result, result)));
    Remote {
        source: source.clone(),
        matter: thread.id.to_string(),
        ids: vec![thread.id.to_string()],
        kind: Kind::Notice,
        lane: rules::github_place(reason, kind, result),
        title: thread.subject.title.clone(),
        body: None,
        from: None,
        url: Some(thread.web_url()),
        facets,
        date: thread.updated_at,
    }
}

/// The person muted `source` as a whole (`inbox.mute_source`), as `inbox.list_sources` says it.
fn muted(inbox: &Inbox, source: &SourceRef, now: DateTime<Utc>) -> bool {
    match inbox.handle(Request::ListSources, None, now) {
        Response::Sources { sources } => sources.iter().any(|info| info.source == *source && info.muted),
        _ => false,
    }
}

/// The cursor `name` of `source`; `None` when there is none or it cannot be read (logged).
fn cursor(inbox: &Inbox, source: &SourceRef, name: &str) -> Option<String> {
    inbox.cursor(source, name).unwrap_or_else(|err| {
        eprintln!(
            "torroworkplace: inbox: where the GitHub notifications of {} stopped is unknown: {err}",
            source.key()
        );
        None
    })
}

/// `duration` as chrono's, the longest one for one that does not fit.
fn delta(duration: Duration) -> TimeDelta {
    TimeDelta::from_std(duration).unwrap_or(TimeDelta::MAX)
}

#[cfg(test)]
mod tests {
    use httpmock::Method::{DELETE, GET, POST};
    use httpmock::{Mock, MockServer};
    use serde_json::{Value, json};
    use torro_core::account::Secret;
    use torro_inbox::{Lane, Link, StateFilter};
    use torro_ipc::StatsRange;

    use super::*;
    use crate::account::github::tests::{MemoryTokens, NO_CLI, service, viewer};
    use crate::pim::PimService;
    use crate::settings_service::SettingsStore;

    const LOGIN: &str = "anna-berg";
    const TOKEN: &str = "ghp_classicSECRET";

    /// A fixed clock: Tuesday 13 October 2026, 10:00 UTC, and minutes after it.
    fn at(minutes: i64) -> DateTime<Utc> {
        "2026-10-13T10:00:00Z".parse::<DateTime<Utc>>().expect("time") + TimeDelta::minutes(minutes)
    }

    fn source() -> SourceRef {
        SourceRef::github(LOGIN)
    }

    /// A core's view of GitHub and the inbox in `dir`, with the account `github:anna-berg` whose
    /// token GitHub (`server`) takes.
    struct World {
        _dir: tempfile::TempDir,
        server: MockServer,
        sources: Sources,
        poller: tokio::sync::Mutex<Poller>,
    }

    impl World {
        async fn new() -> World {
            let dir = tempfile::tempdir().expect("temp dir");
            let server = MockServer::start_async().await;
            signs_in(&server, 200).await;
            let tokens = MemoryTokens::default();
            let github = Arc::new(service(dir.path(), &tokens, &server.url("/graphql"), NO_CLI));
            github.connect_token(Secret::new(TOKEN)).await.expect("connected");
            github.start();
            for _ in 0..200 {
                if github.account_checks().await.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let settings = SettingsStore::at(dir.path().join("config").join("settings.json"));
            let pim = PimService::with_filter_file(settings, dir.path().join("config").join("filters.json"));
            let inbox = Arc::new(crate::inbox::tests::inbox_in(dir.path()));
            let sources = Sources { inbox, github, pim: Arc::new(pim) };
            World { _dir: dir, server, sources, poller: tokio::sync::Mutex::new(Poller::default()) }
        }

        /// GitHub lists `threads` with `X-Poll-Interval: poll` and `Last-Modified: stamp`, to
        /// requests that send `If-Modified-Since` (`Some`) or not (`None`).
        async fn lists(&self, modified: Option<&str>, threads: Value, poll: u64, stamp: &str) -> Mock<'_> {
            self.server
                .mock_async(|when, then| {
                    let when =
                        when.method(GET).path("/notifications").header("authorization", format!("Bearer {TOKEN}"));
                    if let Some(value) = modified {
                        when.header("if-modified-since", value);
                    } else {
                        when.header_missing("if-modified-since");
                    }
                    then.status(200)
                        .header("X-Poll-Interval", poll.to_string())
                        .header("Last-Modified", stamp)
                        .json_body(threads);
                })
                .await
        }

        /// GitHub answers every listing with `status`.
        async fn refuses(&self, status: u16) -> Mock<'_> {
            self.server
                .mock_async(|when, then| {
                    when.method(GET).path("/notifications");
                    then.status(status)
                        .json_body(json!({"message": "Resource not accessible by personal access token"}));
                })
                .await
        }

        async fn round(&self, minutes: i64) {
            self.poller.lock().await.round(&self.sources, at(minutes)).await;
        }

        fn inbox(&self) -> &Inbox {
            &self.sources.inbox
        }

        /// The entries of the source as the app key sees them: open, snoozed and done.
        fn items(&self) -> Vec<Item> {
            let mut all = Vec::new();
            for state in [StateFilter::Open, StateFilter::Snoozed, StateFilter::Done] {
                let list = Request::ListItems {
                    lane: None,
                    source: Some(source()),
                    state: Some(state),
                    since: None,
                    limit: None,
                };
                match self.inbox().handle(list, None, at(600)) {
                    Response::Items { items } => all.extend(items),
                    other => panic!("expected items, got {other:?}"),
                }
            }
            all
        }

        fn item(&self, thread: &str) -> Item {
            self.items().into_iter().find(|item| item.thread_key == thread).expect("an entry of the thread")
        }

        fn health(&self) -> Option<SourceHealth> {
            match self.inbox().handle(Request::ListSources, None, at(600)) {
                Response::Sources { sources } => {
                    sources.into_iter().find(|info| info.source == source()).and_then(|info| info.health)
                }
                other => panic!("expected sources, got {other:?}"),
            }
        }
    }

    /// GitHub's GraphQL endpoint answers the check of a token with `status`: 200 takes it.
    async fn signs_in(server: &MockServer, status: u16) {
        server
            .mock_async(|when, then| {
                when.method(POST).path("/graphql");
                then.status(status).header("X-OAuth-Scopes", "notifications, repo").body(viewer(LOGIN));
            })
            .await;
    }

    /// A thread as GitHub lists it: about `kind` (`PullRequest`, `Issue`, `CheckSuite`) number
    /// `number` in `repo`, for `reason`, last updated at `updated` (minutes after [`at`]).
    fn thread(id: &str, repo: &str, kind: &str, number: u64, reason: &str, title: &str, updated: i64) -> Value {
        let page = if kind == "Issue" { "issues" } else { "pulls" };
        let url = (kind != "CheckSuite").then(|| format!("API/repos/{repo}/{page}/{number}"));
        json!({
            "id": id,
            "repository": {"full_name": repo},
            "subject": {"title": title, "url": url, "type": kind},
            "reason": reason,
            "unread": true,
            "updated_at": at(updated).to_rfc3339_opts(SecondsFormat::Secs, true),
        })
    }

    /// `threads` with their API addresses on `server`, as GitHub would send them there.
    fn on(server: &MockServer, threads: Value) -> Value {
        let text = threads.to_string().replace("API/", &format!("{}/", server.base_url()));
        serde_json::from_str(&text).expect("JSON")
    }

    fn review(updated: i64) -> Value {
        thread("101", "torro/workplace", "PullRequest", 42, "review_requested", "Add the inbox", updated)
    }

    fn failed_run(updated: i64) -> Value {
        thread(
            "102",
            "torro/workplace",
            "CheckSuite",
            0,
            "ci_activity",
            "CI workflow run failed for main branch",
            updated,
        )
    }

    fn comment(updated: i64) -> Value {
        thread("103", "torro/site", "Issue", 7, "comment", "Typo on the start page", updated)
    }

    fn facet(item: &Item, key: FacetKey) -> Option<&str> {
        item.facets.iter().find(|facet| facet.key == key).map(|facet| facet.value.as_str())
    }

    const STAMP: &str = "Tue, 13 Oct 2026 10:00:00 GMT";

    #[tokio::test]
    async fn threads_become_entries_with_traits_lane_and_a_link_built_on_github() {
        let world = World::new().await;
        let threads = on(&world.server, json!([review(-5), failed_run(-10), comment(-20)]));
        world.lists(None, threads, 60, STAMP).await;
        world.round(0).await;

        let pr = world.item("101");
        assert_eq!((pr.kind, pr.lane, pr.urgent, pr.untrusted), (Kind::Notice, Lane::ForMe, false, true));
        assert_eq!((pr.title.as_str(), pr.body.as_deref(), pr.from.as_ref()), ("Add the inbox", None, None));
        assert_eq!(pr.link, Some(Link::Url { url: "https://github.com/torro/workplace/pull/42".into() }));
        assert_eq!(
            [facet(&pr, FacetKey::Repo), facet(&pr, FacetKey::Reason), facet(&pr, FacetKey::Kind)],
            [Some("torro/workplace"), Some("review_requested"), Some("pull_request")]
        );
        assert_eq!(facet(&pr, FacetKey::Result), None);
        assert_eq!(world.inbox().remote_ids(&pr.id).expect("ids"), ["101"]);

        let run = world.item("102");
        assert_eq!((run.lane, facet(&run, FacetKey::Result)), (Lane::ForMe, Some("failed")), "a failed run is for me");
        assert_eq!(run.link, Some(Link::Url { url: "https://github.com/torro/workplace/actions".into() }));
        let note = world.item("103");
        assert_eq!(note.lane, Lane::Info, "a comment is for information");
        assert_eq!(note.link, Some(Link::Url { url: "https://github.com/torro/site/issues/7".into() }));
        assert_eq!(world.health(), None);
    }

    #[tokio::test]
    async fn every_listing_reaches_three_days_before_the_first_and_sends_last_modified() {
        let world = World::new().await;
        let since = (at(0) - TimeDelta::days(3)).to_rfc3339_opts(SecondsFormat::Secs, true);
        let first = world
            .server
            .mock_async(|when, then| {
                when.method(GET).path("/notifications").query_param("since", since.as_str());
                then.status(200).header("Last-Modified", STAMP).json_body(json!([]));
            })
            .await;
        world.round(0).await;
        assert_eq!(first.calls_async().await, 1, "only the last three days");
        first.delete_async().await;

        let unchanged = world
            .server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/notifications")
                    .query_param("since", since.as_str())
                    .header("if-modified-since", STAMP);
                then.status(304).header("X-Poll-Interval", "60");
            })
            .await;
        world.round(1).await;
        assert_eq!(unchanged.calls_async().await, 1, "the same reach, `If-Modified-Since` from the cursor");
        assert!(world.items().is_empty(), "304: nothing");
        unchanged.delete_async().await;

        // Something changed a day later: an old unread thread GitHub lists anyway stays out.
        let old = thread("104", "torro/site", "Issue", 3, "subscribed", "Old idea", -5 * 24 * 60);
        let threads = on(&world.server, json!([review(24 * 60), old]));
        let changed = world
            .server
            .mock_async(|when, then| {
                when.method(GET).path("/notifications").query_param("since", since.as_str());
                then.status(200).header("Last-Modified", STAMP).json_body(threads);
            })
            .await;
        world.round(24 * 60).await;
        assert_eq!(changed.calls_async().await, 1, "still three days before the first listing");
        let threads: Vec<String> = world.items().into_iter().map(|item| item.thread_key).collect();
        assert_eq!(threads, ["101"], "nothing older");
    }

    #[tokio::test]
    async fn github_s_poll_interval_and_the_minute_are_kept() {
        let world = World::new().await;
        let listing = world.lists(None, json!([]), 120, STAMP).await;
        world.round(0).await;
        world.round(1).await;
        assert_eq!(listing.calls_async().await, 1, "X-Poll-Interval 120: not after one minute");
        listing.delete_async().await;
        let again = world.lists(Some(STAMP), json!([]), 5, STAMP).await;
        world.round(2).await;
        assert_eq!(again.calls_async().await, 1, "after two minutes");
        world.round(2).await;
        assert_eq!(again.calls_async().await, 1, "X-Poll-Interval 5 still waits a minute");
        world.round(3).await;
        assert_eq!(again.calls_async().await, 2);
    }

    #[tokio::test]
    async fn new_activity_counts_again_and_brings_a_done_entry_back() {
        let world = World::new().await;
        let first = world.lists(None, on(&world.server, json!([review(-5)])), 60, STAMP).await;
        world.round(0).await;
        first.delete_async().await;
        let id = world.item("101").id;
        let done = Request::SetItemDone { id: id.clone(), done: true, dismissed: false };
        assert!(matches!(world.inbox().handle(done, None, at(1)), Response::Item { .. }));

        // Listed again as it was: nothing new.
        let same = world.lists(Some(STAMP), on(&world.server, json!([review(-5)])), 60, STAMP).await;
        world.round(1).await;
        assert_eq!(same.calls_async().await, 1);
        let entry = world.item("101");
        assert!(matches!(entry.state, State::Done { .. }), "{entry:?}");
        assert_eq!(entry.count, 1);
        same.delete_async().await;

        let later = "Tue, 13 Oct 2026 10:02:00 GMT";
        world.lists(Some(STAMP), on(&world.server, json!([review(2)])), 60, later).await;
        world.round(2).await;
        let entry = world.item("101");
        assert_eq!((entry.id.as_str(), entry.count, entry.state), (id.as_str(), 2, State::New), "new activity");
    }

    #[tokio::test]
    async fn a_thread_missing_from_a_complete_listing_is_done_but_not_from_an_incomplete_one() {
        let world = World::new().await;
        let first = world.lists(None, on(&world.server, json!([review(-5), comment(-20)])), 60, STAMP).await;
        world.round(0).await;
        first.delete_async().await;

        // More pages than read: the list is not complete, so a missing thread says nothing.
        let next = format!("<{}/notifications?all=false&per_page=50&page=2>; rel=\"next\"", world.server.base_url());
        let page = world
            .server
            .mock_async(|when, then| {
                when.method(GET).path("/notifications");
                then.status(200).header("Link", next.as_str()).header("Last-Modified", STAMP).json_body(json!([]));
            })
            .await;
        world.round(1).await;
        assert!(page.calls_async().await >= 10, "every page was asked for");
        assert!(world.items().iter().all(|item| item.state == State::New), "nothing done");
        page.delete_async().await;

        // Complete: the comment was read elsewhere.
        world.lists(Some(STAMP), on(&world.server, json!([review(-5)])), 60, STAMP).await;
        world.round(2).await;
        assert!(matches!(world.item("103").state, State::Done { .. }), "read elsewhere");
        assert_eq!(world.item("101").state, State::New);
    }

    #[tokio::test]
    async fn done_marks_the_thread_done_once_and_undo_not_at_all() {
        let world = World::new().await;
        let first = world.lists(None, on(&world.server, json!([review(-5)])), 60, STAMP).await;
        world.round(0).await;
        first.delete_async().await;
        let deleted = world
            .server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/notifications/threads/101")
                    .header("authorization", format!("Bearer {TOKEN}"));
                then.status(204);
            })
            .await;
        let id = world.item("101").id;
        let set = |done: bool| Request::SetItemDone { id: id.clone(), done, dismissed: false };
        let now = Utc::now();

        let Response::Item { item } = world.inbox().handle(set(true), None, now) else { panic!("done") };
        super::super::settle_done(&world.sources, &item);
        for _ in 0..200 {
            if deleted.calls_async().await > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(deleted.calls_async().await, 1, "done reached GitHub");

        // Done again, and a task of the done entry: already told.
        let Response::Item { item } = world.inbox().handle(set(true), None, now) else { panic!("done") };
        super::super::settle_done(&world.sources, &item);
        // Undo never reaches GitHub (the core does not even call it; the module ignores it too).
        let Response::Item { item: undone } = world.inbox().handle(set(false), None, now) else { panic!("undone") };
        super::super::settle_done(&world.sources, &undone);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(deleted.calls_async().await, 1, "exactly one DELETE");
    }

    #[test]
    fn an_entry_reaches_github_once_per_time_it_became_done() {
        let settled = Settled(Mutex::new(Vec::new()));
        let done = at(0);
        assert!(settled.first("e1", done, at(0)));
        assert!(!settled.first("e1", done, at(0)), "done again: already told");
        assert!(settled.first("e2", done, at(0)), "another entry");
        assert!(settled.first("e1", at(1), at(1)), "done anew after undo");
        assert!(!settled.first("e3", done, at(5)), "done long ago: told then");
    }

    #[tokio::test]
    async fn a_muted_source_asks_github_nothing() {
        let world = World::new().await;
        let mute = Request::MuteSource { source: source(), muted: true };
        assert_eq!(world.inbox().handle(mute, None, at(0)), Response::Ok);
        let listing = world.lists(None, on(&world.server, json!([review(-5)])), 60, STAMP).await;
        world.round(0).await;
        assert_eq!(listing.calls_async().await, 0, "muted: not asked");
        let unmute = Request::MuteSource { source: source(), muted: false };
        assert_eq!(world.inbox().handle(unmute, None, at(1)), Response::Ok);
        world.round(1).await;
        assert_eq!(listing.calls_async().await, 1, "asked again once unmuted");
    }

    /// The statistics of the week up to the fixed clock's day have rows of the source.
    fn counted(world: &World) -> bool {
        let week = Request::GetStats { range: StatsRange::Week, until: Some(at(0).date_naive()) };
        let Response::Stats(stats) = world.inbox().handle(week, None, at(600)) else { panic!("statistics") };
        stats.days.iter().flat_map(|day| &day.sources).any(|row| row.source == source())
    }

    #[tokio::test]
    async fn a_removed_account_ends_its_loop_and_its_entries_and_statistics_go() {
        let world = World::new().await;
        let listing = world.lists(None, on(&world.server, json!([review(-5), comment(-20)])), 60, STAMP).await;
        world.round(0).await;
        assert_eq!(world.items().len(), 2);
        assert!(counted(&world), "counted while the account is there");
        world.sources.github.remove(&source().key()).await.expect("removed");
        world.round(1).await;
        world.round(5).await;
        assert_eq!(listing.calls_async().await, 1, "not asked any more");
        assert!(world.items().iter().all(|item| matches!(item.state, State::Done { .. })), "retired");
        assert!(!counted(&world), "its statistics went with it");
        assert_eq!(world.inbox().cursor(&source(), LAST_MODIFIED).expect("cursor"), None, "its cursors too");
    }

    #[tokio::test]
    async fn a_refused_token_waits_for_a_new_sign_in_and_says_signed_out() {
        let world = World::new().await;
        // GitHub no longer takes the token, for notifications and for the check alike.
        world.server.reset_async().await;
        signs_in(&world.server, 401).await;
        let refused = world.refuses(401).await;
        world.round(0).await;
        assert_eq!(world.health(), Some(SourceHealth::SignedOut));
        let checks = world.sources.github.account_checks().await.expect("loaded");
        assert!(matches!(checks[0].health, Health::Problem { .. }), "the account's problem, from the check");
        world.round(1).await;
        world.round(20).await;
        assert_eq!(refused.calls_async().await, 1, "never sent again until a new sign-in");

        world.server.reset_async().await;
        signs_in(&world.server, 200).await;
        let listing = world.lists(None, json!([]), 60, STAMP).await;
        world.sources.github.connect_token(Secret::new(TOKEN)).await.expect("signed in again");
        world.round(21).await;
        assert_eq!(listing.calls_async().await, 1, "read at once after the new sign-in");
        assert_eq!(world.health(), None);
    }

    #[tokio::test]
    async fn a_fine_grained_token_says_missing_scope_and_files_nothing() {
        let world = World::new().await;
        let refused = world.refuses(403).await;
        world.round(0).await;
        assert_eq!(world.health(), Some(SourceHealth::MissingScope));
        assert!(world.items().is_empty());
        world.round(1).await;
        assert_eq!(refused.calls_async().await, 1, "not every minute");
        refused.delete_async().await;
        world.lists(None, json!([]), 60, STAMP).await;
        world.round(16).await;
        assert_eq!(world.health(), None, "cleared once it works");
    }

    #[test]
    fn a_report_takes_the_thread_s_parts_and_no_number_from_another_host() {
        // The API address in `review` is not GitHub's: the number is not taken, the link is the repo's.
        let threads = notifications::read_threads(&json!([review(0)]), "https://api.github.com").expect("threads");
        let report = report(&source(), &threads[0]);
        assert_eq!((report.matter.as_str(), report.ids.as_slice()), ("101", ["101".to_string()].as_slice()));
        assert_eq!(report.url.as_deref(), Some("https://github.com/torro/workplace"));
        assert_eq!((report.kind, report.lane, report.date), (Kind::Notice, Lane::ForMe, at(0)));
    }
}
