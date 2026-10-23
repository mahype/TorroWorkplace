//! Nextcloud's notifications as a source of the inbox (task E5.8): one entry per matter of each
//! Nextcloud account, `nextcloud:<login>@<host>` (accounts of the kind `nextcloud` only, neither
//! CalDAV nor mail accounts; `PimService::nextcloud_accounts`).
//!
//! - **Per account a loop** ([`follow`]): it asks while the account is there with an app password
//!   (`PimService::nextcloud_access`) and ends when it is gone; [`start`] looks for new accounts
//!   every [`POLL`]. Removing an account retires its entries in the core (`Core::retire_sources`).
//! - **Each round** ([`Poller::tick`], every [`POLL`]): a muted source asks nothing. The
//!   capabilities at the first round and every [`CAPABILITIES_EVERY`] (no `notifications` app:
//!   health `missing_app`, no listing); the listing with the ETag of the last one (cursor
//!   [`ETAG`]), so an unchanged list costs a `304`. Notifications older than the cursor [`SINCE`]
//!   are never filed: it is set at the first round to three days back ([`FIRST_DAYS`]) and moved
//!   to the present while the source is muted, so muting drops what came meanwhile.
//! - **Refusals:** 401 and 403 mean signed out (health `signed_out`): the loop asks nothing until
//!   the person signs in again, i.e. the app password changes ([`NextcloudAccess::sign_in`]),
//!   because Nextcloud's brute-force protection would hit the person. The remote wipe is checked by
//!   the calendar sync, which meets the same refusal. 503 (maintenance) waits for the next round,
//!   429 as long as the server asks; unreachable is health `offline`.
//! - **Entry** ([`report`]): matter `<app>:<object_type>:<object_id>` ([`matter`]), the
//!   notification ids the server's ids, so two notifications of one matter are one entry counted
//!   twice; title and text as plain text, from the person named in it; the link only on the
//!   account's own origin (rule 15 of E5); traits `app` and `person`; lane from
//!   `rules::nextcloud_place`; always a notice.
//! - **Resolved by itself** ([`gone`]): an id the server no longer lists is taken off its entry,
//!   which is done once it has none left. The server lists only its newest [`LISTING_MAX`]
//!   notifications, so a full listing resolves only the ids newer than its oldest one.
//! - **Reaching back** ([`settle_done`]): done deletes every notification of the entry on the
//!   server.
//!
//! - **Talk** (task E5.9, `super::talk`) runs in the same loop: on a server with Talk each round
//!   also lists the conversations for the source `talk:<login>@<host>`, unless that one is muted
//!   (a round asks nothing only when both are). Sign-in, capabilities and refusals are shared, so
//!   a refusal or an unreachable server shows on both sources. While the server has Talk, its
//!   chat notifications (`spreed`, object `chat`) are left out here ([`wanted`]): the conversation
//!   is the entry; calls and invitations stay.
//!
//! - **Reminders of events** (task ER3.1): Nextcloud's own reminder (`dav`, object `dav`, its id the
//!   SHA-256 of the event's UID) is no entry of its own while the suite reminds of that event itself
//!   (reminders on, the event in a calendar the suite shows and does not keep quiet,
//!   [`reminded_here`]): the suite's reminder entry stands for it. It stays on the server, so a
//!   phone still gets it.
//!
//! Logs name the account's id and a code ([`code`]), never a title, a text, a person or an app.

use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use sha2::{Digest, Sha256};
use torro_inbox::item::{Facet, FacetKey, SourceHealth};
use torro_inbox::{Item, Kind, Sender, SourceRef, rules};
use torro_ipc::{Request, Response};
use torro_nextcloud::{Capabilities, Listing, Notification, NotificationId, OcsError};

use super::Sources;
use super::talk::{self, Talk};
use crate::inbox::{Remote, RemoteEntry, THREAD_KEY_MAX};
use crate::pim::NextcloudAccess;

/// How often the notifications of an account are listed, and new accounts looked for.
const POLL: Duration = Duration::from_secs(60);

/// How long the capabilities of a server count before they are read again.
const CAPABILITIES_EVERY: TimeDelta = TimeDelta::hours(1);

/// How many days back the first listing of an account files.
const FIRST_DAYS: i64 = 3;

/// The most notifications the server lists at once: `Handler::get` of the notifications app limits
/// the listing to the newest 25 (by id), without pages
/// (<https://github.com/nextcloud/notifications/blob/master/lib/Handler.php>).
const LISTING_MAX: usize = 25;

/// The cursor with the ETag of the last listing.
const ETAG: &str = "etag";

/// The cursor with the time before which no notification is filed (RFC 3339).
const SINCE: &str = "since";

/// Starts the notifications of every Nextcloud account on the current runtime and returns at once:
/// a loop per account ([`follow`]), started as the account shows up (also after the stored
/// accounts were loaded). Without a runtime nothing starts.
pub(super) fn start(sources: Sources) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    runtime.spawn(async move {
        let running: Arc<Mutex<HashSet<String>>> = Arc::default();
        loop {
            for id in sources.pim.nextcloud_accounts().await {
                if !lock(&running).insert(id.clone()) {
                    continue;
                }
                let (sources, running) = (sources.clone(), Arc::clone(&running));
                tokio::spawn(async move {
                    follow(&sources, &id).await;
                    lock(&running).remove(&id);
                });
            }
            tokio::time::sleep(POLL).await;
        }
    });
}

fn lock(running: &Mutex<HashSet<String>>) -> std::sync::MutexGuard<'_, HashSet<String>> {
    running.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The loop of the account `id`: a round ([`Poller::tick`]) and the wait it asks for, as long as
/// the account is there with its app password. When the loop ends, the health is `signed_out`
/// for an account left without its app password (it needs a new sign-in), else cleared; for its
/// Talk too when the server has Talk.
async fn follow(sources: &Sources, id: &str) {
    let Ok(source) = id.parse::<SourceRef>() else { return };
    let mut poller = Poller::default();
    while let Some(access) = sources.pim.nextcloud_access(id).await {
        // While the inbox is off (task W.2) the server is not asked.
        if !sources.inbox_on() {
            tokio::time::sleep(super::RECHECK).await;
            continue;
        }
        let wait = poller.tick(sources, &source, &access, Utc::now()).await;
        tokio::time::sleep(wait).await;
    }
    let listed = sources.pim.nextcloud_accounts().await.iter().any(|known| known == id);
    poller.set_health(sources, &source, listed.then_some(SourceHealth::SignedOut));
}

/// What the loop of one account keeps between its rounds, in memory. No `Debug`: `refused` is
/// derived from the app password.
#[derive(Default)]
pub(super) struct Poller {
    /// The server's capabilities and when they were read.
    capabilities: Option<(Capabilities, DateTime<Utc>)>,
    /// The sign-in the server refused ([`NextcloudAccess::sign_in`]): nothing is asked with it.
    refused: Option<[u8; 32]>,
    /// Talk's conversations of the account (task E5.9).
    talk: Talk,
}

impl Poller {
    /// One round for `source` (and its Talk) with `access` at `now` (see the module docs);
    /// returns how long to wait before the next one. Sets the sources' health: `None` when the
    /// round worked.
    pub(super) async fn tick(
        &mut self,
        sources: &Sources,
        source: &SourceRef,
        access: &NextcloudAccess,
        now: DateTime<Utc>,
    ) -> Duration {
        if self.refused == Some(access.sign_in) {
            return POLL;
        }
        self.refused = None;
        let muted = is_muted(sources, source, now);
        if muted {
            // What comes while muted stays out, also once the source is heard again.
            keep_cursor(sources, source, SINCE, Some(&now.to_rfc3339()));
        }
        let talk_muted = is_muted(sources, &talk::source_of(source), now);
        if talk_muted {
            self.talk.mute();
        }
        if muted && talk_muted {
            return POLL;
        }
        match self.read(sources, source, access, now, muted, talk_muted).await {
            Ok(()) => POLL,
            Err(error) => self.failed(sources, source, access, &error),
        }
    }

    /// Reads the capabilities when due, then what of the notifications and of Talk is not muted,
    /// and sets the health of each that was read (`missing_app` or none). `Err` for what concerns
    /// both: a refusal, an unreachable server, maintenance, a throttle.
    async fn read(
        &mut self,
        sources: &Sources,
        source: &SourceRef,
        access: &NextcloudAccess,
        now: DateTime<Utc>,
        muted: bool,
        talk_muted: bool,
    ) -> Result<(), OcsError> {
        let due = self.capabilities.as_ref().is_none_or(|(_, at)| now - *at >= CAPABILITIES_EVERY);
        if due {
            self.capabilities = Some((access.client.capabilities().await?, now));
        }
        let has_talk = self.has_talk();
        if !muted {
            let health = self.notifications(sources, source, access, now, has_talk).await?;
            sources.inbox.set_source_health(source, health);
        }
        let talk = talk::source_of(source);
        if !has_talk {
            // No Talk on the server: no source of its own, so no health either.
            sources.inbox.set_source_health(&talk, None);
        } else if !talk_muted {
            let health = match self.talk.round(sources, &talk, &access.client, now).await {
                Ok(()) => None,
                Err(OcsError::MissingApp) => {
                    // Read the capabilities again next round: Talk may be gone, or come back.
                    self.capabilities = None;
                    Some(SourceHealth::MissingApp)
                }
                Err(error) => return Err(error),
            };
            sources.inbox.set_source_health(&talk, health);
        }
        Ok(())
    }

    /// Whether the server has Talk, as its capabilities said last.
    fn has_talk(&self) -> bool {
        self.capabilities.as_ref().is_some_and(|(capabilities, _)| capabilities.talk.is_some())
    }

    /// Sets `health` for `source` and, while the server has Talk, for its Talk source.
    fn set_health(&self, sources: &Sources, source: &SourceRef, health: Option<SourceHealth>) {
        sources.inbox.set_source_health(source, health);
        let talk = health.filter(|_| self.has_talk());
        sources.inbox.set_source_health(&talk::source_of(source), talk);
    }

    /// Lists the notifications, files and resolves; `has_talk` leaves Talk's chat notifications
    /// out ([`wanted`]). `Ok` with the health to show (`missing_app` or none).
    async fn notifications(
        &mut self,
        sources: &Sources,
        source: &SourceRef,
        access: &NextcloudAccess,
        now: DateTime<Utc>,
        has_talk: bool,
    ) -> Result<Option<SourceHealth>, OcsError> {
        if self.capabilities.as_ref().is_some_and(|(capabilities, _)| capabilities.notifications.is_none()) {
            return Ok(Some(SourceHealth::MissingApp));
        }
        let since = since(sources, source, now);
        let etag = read_cursor(sources, source, ETAG);
        let (notifications, etag) = match access.client.notifications(etag.as_deref()).await {
            Ok(Listing::NotModified) => return Ok(None),
            Ok(Listing::List { notifications, etag }) => (notifications, etag),
            Err(OcsError::MissingApp) => {
                // Read the capabilities again next round: the app may come back.
                self.capabilities = None;
                return Ok(Some(SourceHealth::MissingApp));
            }
            Err(error) => return Err(error),
        };
        // Removed while asking: its entries were retired, none is filed again.
        if !sources.pim.nextcloud_accounts().await.contains(&source.key()) {
            return Ok(None);
        }
        let reminded = reminded_here(sources, now).await;
        let filed = file(sources, source, access.client.server(), &notifications, since, has_talk, &reminded, now);
        resolve(sources, source, &notifications, now);
        // Only a listing filed in full is not asked for again (a 304 would hide what failed).
        keep_cursor(sources, source, ETAG, etag.as_deref().filter(|_| filed));
        Ok(None)
    }

    /// After the round failed with `error`: the health it means for the source and its Talk
    /// (kept as it was after an unexpected answer; cleared in maintenance and when throttled, as
    /// the server is there and takes the sign-in), and how long to wait.
    fn failed(
        &mut self,
        sources: &Sources,
        source: &SourceRef,
        access: &NextcloudAccess,
        error: &OcsError,
    ) -> Duration {
        let health = match error {
            OcsError::Refused | OcsError::Forbidden => {
                self.refused = Some(access.sign_in);
                eprintln!("{}", log_line(source, error));
                Some(SourceHealth::SignedOut)
            }
            OcsError::MissingApp => Some(SourceHealth::MissingApp),
            OcsError::Offline => Some(SourceHealth::Offline),
            OcsError::Maintenance | OcsError::Throttled { .. } => None,
            OcsError::Other(_) => {
                eprintln!("{}", log_line(source, error));
                return POLL;
            }
        };
        self.set_health(sources, source, health);
        match error {
            OcsError::Throttled { retry_after } => (*retry_after).max(POLL),
            _ => POLL,
        }
    }
}

/// Whether the person muted `source` (`inbox.mute_source`), as `inbox.list_sources` says.
pub(super) fn is_muted(sources: &Sources, source: &SourceRef, now: DateTime<Utc>) -> bool {
    listed(sources, source, now).is_some_and(|info| info.muted)
}

/// Whether the loop of `source` found the server refusing the sign-in (health `signed_out`).
pub(super) fn is_signed_out(sources: &Sources, source: &SourceRef, now: DateTime<Utc>) -> bool {
    listed(sources, source, now).is_some_and(|info| info.health == Some(SourceHealth::SignedOut))
}

/// `source` as `inbox.list_sources` lists it for the person.
fn listed(sources: &Sources, source: &SourceRef, now: DateTime<Utc>) -> Option<torro_inbox::SourceInfo> {
    match sources.inbox.handle(Request::ListSources, None, now) {
        Response::Sources { sources } => sources.into_iter().find(|info| info.source == *source),
        _ => None,
    }
}

/// The time before which nothing of `source` is filed: the cursor [`SINCE`], set to
/// [`FIRST_DAYS`] before `now` at the first round.
fn since(sources: &Sources, source: &SourceRef, now: DateTime<Utc>) -> DateTime<Utc> {
    let kept = read_cursor(sources, source, SINCE)
        .and_then(|since| DateTime::parse_from_rfc3339(&since).ok())
        .map(|since| since.with_timezone(&Utc));
    kept.unwrap_or_else(|| {
        let since = now - TimeDelta::days(FIRST_DAYS);
        keep_cursor(sources, source, SINCE, Some(&since.to_rfc3339()));
        since
    })
}

/// The cursor `name` of `source`; `None` also when it cannot be read (logged).
pub(super) fn read_cursor(sources: &Sources, source: &SourceRef, name: &str) -> Option<String> {
    sources.inbox.cursor(source, name).unwrap_or_else(|err| {
        eprintln!("torroworkplace: inbox: cannot read where {source} stopped: {err}");
        None
    })
}

/// Keeps `value` as the cursor `name` of `source` (`None` removes it); failures to the log.
pub(super) fn keep_cursor(sources: &Sources, source: &SourceRef, name: &str, value: Option<&str>) {
    if let Err(err) = sources.inbox.set_cursor(source, name, value) {
        eprintln!("torroworkplace: inbox: cannot keep where {source} stopped: {err}");
    }
}

/// Files the notifications of `listed` that are [`wanted`] (with `has_talk`) and not older than
/// `since`, oldest first, so the newest gives its matter's entry the texts. Whether every one was
/// filed.
#[allow(clippy::too_many_arguments)]
fn file(
    sources: &Sources,
    source: &SourceRef,
    server: &str,
    listed: &[Notification],
    since: DateTime<Utc>,
    has_talk: bool,
    reminded: &HashSet<String>,
    now: DateTime<Utc>,
) -> bool {
    let mut filed = true;
    let fresh = |n: &&Notification| n.time >= since && wanted(n, has_talk) && !is_reminded(n, reminded);
    for notification in listed.iter().rev().filter(fresh) {
        if let Err(err) = sources.inbox.file_remote(&report(source, server, notification), now) {
            eprintln!("torroworkplace: inbox: cannot file a notification of {source}: {err}");
            filed = false;
        }
    }
    filed
}

/// Whether `notification` becomes part of the inbox: all but Talk's chat notifications (`spreed`,
/// object `chat`: messages, mentions, replies, reactions) while the server has Talk (`has_talk`),
/// as the Talk source files the conversation itself (task E5.9). Calls and invitations stay.
fn wanted(notification: &Notification, has_talk: bool) -> bool {
    !(has_talk && notification.app == "spreed" && notification.object_type == "chat")
}

/// The events the suite reminds of itself now (task ER3.1), as Nextcloud names them in its reminders:
/// the SHA-256 of each UID, in hex. Empty while reminders are off or the calendars cannot be read.
pub(super) async fn reminded_here(sources: &Sources, now: DateTime<Utc>) -> HashSet<String> {
    let settings = sources.pim.settings().current().reminders;
    if !settings.enabled {
        return HashSet::new();
    }
    let (start, end) = crate::reminders::window(now);
    let events = match sources.pim.handle_within(Request::Events { start, end }, crate::pim::Reach::Suite).await {
        Response::Events { events } => events,
        _ => return HashSet::new(),
    };
    events
        .iter()
        .filter(|event| settings.calendar(&event.collection) != torro_core::settings::CalendarReminders::Off)
        .map(|event| Sha256::digest(event.uid.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect())
        .collect()
}

/// `notification` is Nextcloud's reminder of an event the suite reminds of itself.
fn is_reminded(notification: &Notification, reminded: &HashSet<String>) -> bool {
    notification.app == "dav"
        && notification.object_type == "dav"
        && reminded.contains(&notification.object_id.to_ascii_lowercase())
}

/// The report of `notification` of `source` on `server` (see the module docs).
fn report(source: &SourceRef, server: &str, notification: &Notification) -> Remote {
    let person = notification.person.as_ref();
    let facets = [
        Some(Facet { key: FacetKey::App, value: notification.app.clone() }),
        person.map(|person| Facet { key: FacetKey::Person, value: person.name.clone() }),
    ];
    Remote {
        source: source.clone(),
        matter: matter(notification),
        ids: vec![notification.id.to_string()],
        kind: Kind::Notice,
        lane: rules::nextcloud_place(&notification.app, &notification.object_type),
        title: notification.subject.clone(),
        body: Some(notification.message.clone()).filter(|message| !message.trim().is_empty()),
        from: person.map(|person| Sender { name: person.name.clone(), address: None }),
        // The crate keeps only links of the server; checked again, as the rule is the core's.
        url: notification.link.clone().filter(|link| torro_nextcloud::same_origin(link, server)),
        facets: facets.into_iter().flatten().collect(),
        date: notification.time,
    }
}

/// The matter of `notification`: `<app>:<object_type>:<object_id>`, or `<app>:#<SHA-256 of it>`
/// when that is longer than [`THREAD_KEY_MAX`] bytes (the app id has at most 64).
fn matter(notification: &Notification) -> String {
    let matter = format!("{}:{}:{}", notification.app, notification.object_type, notification.object_id);
    if matter.len() <= THREAD_KEY_MAX {
        return matter;
    }
    let digest = Sha256::digest(matter.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}:#{hex}", notification.app)
}

/// Takes the ids [`gone`] from the server off the entries of `source`.
fn resolve(sources: &Sources, source: &SourceRef, listed: &[Notification], now: DateTime<Utc>) {
    let open = match sources.inbox.open_remote(source) {
        Ok(open) => open,
        Err(err) => {
            eprintln!("torroworkplace: inbox: cannot read the open entries of {source}: {err}");
            return;
        }
    };
    let gone = gone(&open, listed);
    if !gone.is_empty()
        && let Err(err) = sources.inbox.resolve_remote(source, &gone, now)
    {
        eprintln!("torroworkplace: inbox: cannot settle the entries of {source}: {err}");
    }
}

/// The ids of the `open` entries the server no longer holds: not in `listed`, and either the
/// listing is shorter than [`LISTING_MAX`] (so it is all there is) or the id is newer than the
/// oldest one listed (ids only grow; older ones may lie beyond the listing's end).
fn gone(open: &[RemoteEntry], listed: &[Notification]) -> Vec<String> {
    let ids: BTreeSet<NotificationId> = listed.iter().map(|notification| notification.id).collect();
    let complete = listed.len() < LISTING_MAX;
    let oldest = ids.first().copied();
    open.iter()
        .flat_map(|entry| &entry.ids)
        .filter(|id| {
            NotificationId::parse(id)
                .is_some_and(|id| !ids.contains(&id) && (complete || oldest.is_some_and(|oldest| id > oldest)))
        })
        .cloned()
        .collect()
}

/// Deletes the notifications `ids` of the done entry `item` on the account's server, in the
/// background: spawn the calls on the current runtime and return at once. Failures only to the
/// log, with the account and a code.
pub(super) fn settle_done(sources: &Sources, item: &Item, ids: Vec<String>) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    let (sources, source) = (sources.clone(), item.source.clone());
    runtime.spawn(async move {
        delete(&sources, &source, &ids).await;
    });
}

/// Deletes the notifications `ids` of `source` on its server, one call each (a notification
/// already gone counts as deleted); returns how many were deleted. Sends nothing while the loop
/// found the sign-in refused, and stops at a refusal: asking again with a refused app password
/// would only feed Nextcloud's brute-force protection.
async fn delete(sources: &Sources, source: &SourceRef, ids: &[String]) -> usize {
    if is_signed_out(sources, source, Utc::now()) {
        eprintln!("torroworkplace: inbox: cannot delete the notifications of {source}: auth");
        return 0;
    }
    let Some(access) = sources.pim.nextcloud_access(&source.key()).await else {
        eprintln!("torroworkplace: inbox: cannot delete the notifications of {source}: not-connected");
        return 0;
    };
    let mut deleted = 0;
    for id in ids.iter().filter_map(|id| NotificationId::parse(id)) {
        match access.client.delete_notification(id).await {
            Ok(()) => deleted += 1,
            Err(error) => {
                eprintln!("{}", log_line(source, &error));
                if matches!(error, OcsError::Refused | OcsError::Forbidden) {
                    break;
                }
            }
        }
    }
    deleted
}

/// The log line for `error` of `source`: the account's id and [`code`], nothing the server sent.
fn log_line(source: &SourceRef, error: &OcsError) -> String {
    format!("torroworkplace: inbox: Nextcloud notifications of {source}: {}", code(error))
}

/// A fixed word for `error`, for the log.
pub(super) fn code(error: &OcsError) -> &'static str {
    match error {
        OcsError::Refused => "auth",
        OcsError::Forbidden => "forbidden",
        OcsError::Maintenance => "maintenance",
        OcsError::Throttled { .. } => "busy",
        OcsError::MissingApp => "unsupported",
        OcsError::Offline => "offline",
        OcsError::Other(_) => "error",
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::path::Path;

    use chrono::TimeZone;
    use httpmock::Method::{DELETE, GET};
    use httpmock::{Mock, MockServer};
    use serde_json::{Value, json};
    use torro_core::Account;
    use torro_inbox::{Lane, State};

    use super::*;
    use crate::account::github::tests::{MemoryTokens, NO_CLI, service};
    use crate::pim::PimService;
    use crate::settings_service::SettingsStore;

    pub(in crate::sources) const LIST_PATH: &str = "/ocs/v2.php/apps/notifications/api/v2/notifications";
    pub(in crate::sources) const CAPABILITIES_PATH: &str = "/ocs/v2.php/cloud/capabilities";
    const PASSWORD: &str = "app-password-SECRET";

    pub(in crate::sources) fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 10, 0, 0).single().expect("valid time")
    }

    /// A Nextcloud account `anna` on a mock server, with its inbox in a temp directory (also used
    /// by the tests of `super::talk`).
    pub(in crate::sources) struct Fixture {
        _dir: tempfile::TempDir,
        pub(in crate::sources) server: MockServer,
        pub(in crate::sources) sources: Sources,
        pub(in crate::sources) source: SourceRef,
    }

    impl Fixture {
        pub(in crate::sources) async fn new() -> Fixture {
            let dir = tempfile::tempdir().expect("temp dir");
            let server = MockServer::start_async().await;
            let sources = sources_in(dir.path());
            let account = Account::nextcloud(&server.base_url(), "anna");
            let source = account.id.parse().expect("a source");
            sources.pim.pretend_connected(account, PASSWORD).await;
            Fixture { _dir: dir, server, sources, source }
        }

        async fn access(&self) -> NextcloudAccess {
            self.sources.pim.nextcloud_access(&self.source.key()).await.expect("connected")
        }

        pub(in crate::sources) async fn tick(&self, poller: &mut Poller, now: DateTime<Utc>) -> Duration {
            let access = self.access().await;
            poller.tick(&self.sources, &self.source, &access, now).await
        }

        pub(in crate::sources) async fn capabilities(&self, notifications: bool) -> Mock<'_> {
            let capabilities = match notifications {
                true => json!({ "notifications": { "ocs-endpoints": ["list", "get", "delete"] } }),
                false => json!({ "theming": { "name": "Nextcloud" } }),
            };
            let body = envelope(json!({ "version": { "major": 31 }, "capabilities": capabilities }));
            self.server
                .mock_async(|when, then| {
                    when.method(GET).path(CAPABILITIES_PATH);
                    then.status(200).body(body.to_string());
                })
                .await
        }

        /// The listing `notifications` with the ETag `etag`, for a request without `If-None-Match`
        /// or with another one.
        pub(in crate::sources) async fn listing(&self, notifications: &[Value], etag: &str) -> Mock<'_> {
            let body = envelope(Value::Array(notifications.to_vec())).to_string();
            let etag = etag.to_string();
            self.server
                .mock_async(|when, then| {
                    when.method(GET).path(LIST_PATH);
                    then.status(200).header("ETag", etag).body(body);
                })
                .await
        }

        /// The open entries of the source, newest state as stored.
        pub(in crate::sources) fn open(&self) -> Vec<Item> {
            let open = self.sources.inbox.open_remote(&self.source).expect("open entries");
            open.iter().map(|entry| self.item(&entry.id)).collect()
        }

        pub(in crate::sources) fn item(&self, id: &str) -> Item {
            match self.sources.inbox.handle(Request::GetItem { id: id.into() }, None, now()) {
                Response::Item { item } => item,
                other => panic!("expected an item, got {other:?}"),
            }
        }

        pub(in crate::sources) fn health(&self) -> Option<SourceHealth> {
            match self.sources.inbox.handle(Request::ListSources, None, now()) {
                Response::Sources { sources } => {
                    sources.into_iter().find(|info| info.source == self.source).and_then(|info| info.health)
                }
                other => panic!("expected sources, got {other:?}"),
            }
        }
    }

    fn sources_in(dir: &Path) -> Sources {
        let settings = SettingsStore::at(dir.join("config").join("settings.json"));
        let github = service(dir, &MemoryTokens::default(), "http://127.0.0.1:9/graphql", NO_CLI);
        Sources {
            inbox: Arc::new(crate::inbox::tests::inbox_in(dir)),
            github: Arc::new(github),
            pim: Arc::new(PimService::with_settings(settings.clone())),
            settings,
            ntfy: Arc::new(crate::account::ntfy::NtfyAccounts::new(None, Arc::new(MemoryTokens::default()))),
        }
    }

    pub(in crate::sources) fn envelope(data: Value) -> Value {
        json!({ "ocs": { "meta": { "status": "ok", "statuscode": 200, "message": "OK" }, "data": data } })
    }

    /// A share of the file `object_id` by Bert, notification `id` at `datetime`, linking to `link`.
    fn shared(id: u64, object_id: &str, datetime: &str, link: &str) -> Value {
        json!({
            "notification_id": id,
            "app": "files_sharing",
            "user": "anna",
            "datetime": datetime,
            "object_type": "remote_share",
            "object_id": object_id,
            "subject": "Bert Example shared a file with you",
            "message": "Please have a look.",
            "link": link,
            "subjectRich": "{user} shared {file} with you",
            "subjectRichParameters": {
                "user": { "type": "user", "id": "bert", "name": "Bert Example" },
                "file": { "type": "file", "id": object_id, "name": "Report.pdf", "path": "Report.pdf" }
            },
            "messageRich": "",
            "messageRichParameters": [],
            "actions": []
        })
    }

    /// An update notice, notification `id` at `datetime`, without person and link.
    fn update(id: u64, datetime: &str) -> Value {
        json!({
            "notification_id": id,
            "app": "updatenotification",
            "user": "anna",
            "datetime": datetime,
            "object_type": "core",
            "object_id": format!("31.0.{id}"),
            "subject": "Update to Nextcloud 31 is available.",
            "message": "",
            "link": "",
            "actions": []
        })
    }

    #[tokio::test]
    async fn a_notification_is_an_entry_with_its_traits_lane_person_and_link() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        let link = format!("{}/apps/files/?fileid=77", fixture.server.base_url());
        fixture
            .listing(
                &[shared(1043, "77", "2026-10-13T08:15:00+00:00", &link), update(1041, "2026-10-13T07:00:00Z")],
                "\"a\"",
            )
            .await;
        let mut poller = Poller::default();
        assert_eq!(fixture.tick(&mut poller, now()).await, POLL);
        let open = fixture.open();
        assert_eq!(open.len(), 2);
        let share = open.iter().find(|item| item.thread_key == "files_sharing:remote_share:77").expect("the share");
        assert_eq!(share.title, "Bert Example shared Report.pdf with you");
        assert_eq!(share.body.as_deref(), Some("Please have a look."));
        assert_eq!(share.from.as_ref().map(|from| from.name.as_str()), Some("Bert Example"));
        assert_eq!(share.lane, Lane::ForMe);
        assert_eq!(share.kind, Kind::Notice);
        assert!(share.untrusted);
        assert_eq!(share.link, Some(torro_inbox::Link::Url { url: link }));
        let facets = vec![
            Facet { key: FacetKey::App, value: "files_sharing".into() },
            Facet { key: FacetKey::Person, value: "Bert Example".into() },
        ];
        assert_eq!(share.facets, facets);
        let notice = open.iter().find(|item| item.thread_key.starts_with("updatenotification:")).expect("the update");
        assert_eq!((notice.lane, notice.link.clone(), notice.body.clone()), (Lane::Info, None, None));
        assert_eq!(fixture.sources.inbox.remote_ids(&share.id).expect("ids"), ["1043"]);
        assert_eq!(fixture.health(), None);
        assert_eq!(fixture.sources.inbox.cursor(&fixture.source, ETAG).expect("cursor").as_deref(), Some("\"a\""));
    }

    #[tokio::test]
    async fn a_link_off_the_server_is_dropped() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        let elsewhere = shared(1043, "77", "2026-10-13T08:15:00Z", "https://evil.example.org/apps/files/?fileid=77");
        fixture.listing(&[elsewhere], "\"a\"").await;
        fixture.tick(&mut Poller::default(), now()).await;
        assert_eq!(fixture.open()[0].link, None);
        let report = report(
            &fixture.source,
            "https://cloud.example.org",
            &Notification { link: Some(format!("{}/x", fixture.server.base_url())), ..torro_nextcloud_notification() },
        );
        assert_eq!(report.url, None, "a link of another origin than the account's server");
    }

    /// Task ER3.1: Nextcloud's reminder of an event the suite reminds of itself is no entry of its
    /// own; with the suite's reminders off it is one as before.
    #[tokio::test]
    async fn a_reminder_the_suite_gives_itself_is_no_second_entry() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        let url = format!("{}/remote.php/dav/calendars/anna/work/", fixture.server.base_url());
        let calendar = torro_pim::Collection {
            id: torro_pim::account::collection_id(&fixture.source.key(), &url),
            name: "Work".into(),
            kind: torro_pim::CollectionKind::Events,
            color: None,
            writable: true,
            source: None,
        };
        let event = torro_pim::Event {
            uid: "sync-1".into(),
            collection: calendar.id.clone(),
            href: Some(format!("{url}sync-1.ics")),
            etag: Some("\"1\"".into()),
            summary: "Weekly sync".into(),
            start: torro_pim::EventTime::DateTime(now() + TimeDelta::minutes(10)),
            end: Some(torro_pim::EventTime::DateTime(now() + TimeDelta::minutes(55))),
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
            alarms: Vec::new(),
            recurrence_id: None,
            repeat: None,
            time_zone: None,
            url: None,
            status: None,
            geo: None,
            organizer: None,
            attendees: Vec::new(),
            exceptions: Vec::new(),
        };
        fixture.sources.pim.cache_collections(vec![calendar]).await;
        fixture.sources.pim.cache_events_as_synced(vec![event]).await;
        let hash: String = Sha256::digest(b"sync-1").iter().map(|byte| format!("{byte:02x}")).collect();
        let reminder = json!({
            "notification_id": 2001, "app": "dav", "user": "anna", "datetime": "2026-10-13T08:20:00Z",
            "object_type": "dav", "object_id": hash, "subject": "Weekly sync (in 10 minutes)", "message": "",
            "link": "", "subjectRich": "", "subjectRichParameters": [], "messageRich": "", "messageRichParameters": [],
            "actions": []
        });
        assert!(reminded_here(&fixture.sources, now()).await.contains(&hash), "the suite reminds of it");
        fixture.listing(std::slice::from_ref(&reminder), "\"a\"").await;
        fixture.tick(&mut Poller::default(), now()).await;
        assert!(fixture.open().iter().all(|item| !item.thread_key.starts_with("dav:")), "{:?}", fixture.open());

        let off = serde_json::json!({"enabled": false});
        fixture.sources.pim.settings().set(torro_ipc::SettingKey::Reminders, &off).expect("off");
        assert!(reminded_here(&fixture.sources, now()).await.is_empty());
        keep_cursor(&fixture.sources, &fixture.source, ETAG, None);
        fixture.tick(&mut Poller::default(), now()).await;
        assert!(fixture.open().iter().any(|item| item.thread_key.starts_with("dav:")), "filed as before");
    }

    /// A notification as the crate reads it, for [`report`] and [`matter`].
    fn torro_nextcloud_notification() -> Notification {
        Notification {
            id: NotificationId::parse("7").expect("an id"),
            app: "deck".into(),
            object_type: "card".into(),
            object_id: "12".into(),
            time: now(),
            subject: "A card was assigned to you".into(),
            message: String::new(),
            link: None,
            person: None,
        }
    }

    #[tokio::test]
    async fn two_notifications_of_one_matter_are_one_entry_counted_twice() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        let link = format!("{}/f/77", fixture.server.base_url());
        let notifications =
            [shared(1044, "77", "2026-10-13T09:00:00Z", &link), shared(1043, "77", "2026-10-13T08:15:00Z", &link)];
        fixture.listing(&notifications, "\"b\"").await;
        fixture.tick(&mut Poller::default(), now()).await;
        let open = fixture.open();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].count, 2);
        assert_eq!(fixture.sources.inbox.remote_ids(&open[0].id).expect("ids"), ["1043", "1044"]);
    }

    #[tokio::test]
    async fn listing_again_counts_nothing_twice_and_a_304_files_nothing() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        let first = fixture.listing(&[update(1041, "2026-10-13T07:00:00Z")], "\"c\"").await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        fixture.tick(&mut poller, now()).await;
        assert_eq!(fixture.open()[0].count, 1, "the same notification listed twice");
        first.delete_async().await;
        let not_modified = fixture
            .server
            .mock_async(|when, then| {
                when.method(GET).path(LIST_PATH).header("if-none-match", "\"c\"");
                then.status(304);
            })
            .await;
        fixture.tick(&mut poller, now()).await;
        assert_eq!(not_modified.calls_async().await, 1);
        assert_eq!(fixture.open().len(), 1);
        assert_eq!(fixture.open()[0].count, 1);
    }

    #[tokio::test]
    async fn deleted_on_the_server_the_entry_is_done() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        let listing = fixture.listing(&[update(1041, "2026-10-13T07:00:00Z")], "\"d\"").await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        let id = fixture.open()[0].id.clone();
        listing.delete_async().await;
        fixture.listing(&[], "\"e\"").await;
        fixture.tick(&mut poller, now()).await;
        assert!(fixture.open().is_empty());
        assert!(matches!(fixture.item(&id).state, State::Done { .. }));
    }

    #[test]
    fn a_full_listing_resolves_only_ids_newer_than_its_oldest() {
        let notification =
            |id: &str| Notification { id: NotificationId::parse(id).expect("an id"), ..torro_nextcloud_notification() };
        let full: Vec<Notification> = (100..100 + LISTING_MAX as u64).map(|id| notification(&id.to_string())).collect();
        let open = vec![
            RemoteEntry { id: "old".into(), ids: vec!["20".into()] },
            RemoteEntry { id: "new".into(), ids: vec!["110".into(), "200".into(), "105".into()] },
        ];
        assert_eq!(gone(&open, &full), ["200"], "20 may lie beyond the listing; 110 and 105 are listed");
        assert_eq!(gone(&open, &full[..3]), ["20", "110", "200", "105"], "a short listing is all there is");
        let broken = vec![RemoteEntry { id: "x".into(), ids: vec!["not-an-id".into()] }];
        assert!(gone(&broken, &[]).is_empty());
    }

    #[tokio::test]
    async fn done_deletes_every_notification_of_the_entry() {
        let fixture = Fixture::new().await;
        let mut deletes = Vec::new();
        for id in [1043, 1044] {
            let mock = fixture
                .server
                .mock_async(|when, then| {
                    when.method(DELETE).path(format!("{LIST_PATH}/{id}")).header("ocs-apirequest", "true");
                    then.status(200).body(envelope(json!([])).to_string());
                })
                .await;
            deletes.push(mock);
        }
        let ids = vec!["1043".to_string(), "1044".to_string()];
        assert_eq!(delete(&fixture.sources, &fixture.source, &ids).await, 2);
        for mock in deletes {
            assert_eq!(mock.calls_async().await, 1);
        }
    }

    #[tokio::test]
    async fn settle_done_reaches_the_server_in_the_background() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        fixture.listing(&[update(1041, "2026-10-13T07:00:00Z")], "\"f\"").await;
        fixture.tick(&mut Poller::default(), now()).await;
        let deleted = fixture
            .server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{LIST_PATH}/1041"));
                then.status(200).body(envelope(json!([])).to_string());
            })
            .await;
        let id = fixture.open()[0].id.clone();
        let done = Request::SetItemDone { id: id.clone(), done: true, dismissed: false };
        let Response::Item { item } = fixture.sources.inbox.handle(done, None, now()) else { panic!("done") };
        super::super::settle_done(&fixture.sources, &item);
        for _ in 0..100 {
            if deleted.calls_async().await == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(deleted.calls_async().await, 1);
    }

    #[tokio::test]
    async fn without_the_notifications_app_the_source_says_so_and_lists_nothing() {
        let fixture = Fixture::new().await;
        fixture.capabilities(false).await;
        let listing = fixture.listing(&[update(1041, "2026-10-13T07:00:00Z")], "\"g\"").await;
        fixture.tick(&mut Poller::default(), now()).await;
        assert_eq!(listing.calls_async().await, 0);
        assert_eq!(fixture.health(), Some(SourceHealth::MissingApp));
    }

    #[tokio::test]
    async fn a_404_of_the_listing_is_a_missing_app_too() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        fixture
            .server
            .mock_async(|when, then| {
                when.method(GET).path(LIST_PATH);
                then.status(404).body(envelope(json!([])).to_string());
            })
            .await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        assert_eq!(fixture.health(), Some(SourceHealth::MissingApp));
        assert!(poller.capabilities.is_none(), "read again next round");
    }

    #[tokio::test]
    async fn the_first_listing_files_three_days_only_also_later() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        let old = update(1001, "2026-10-10T09:00:00Z");
        let listing = fixture.listing(&[update(1041, "2026-10-13T07:00:00Z"), old.clone()], "\"h\"").await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        assert_eq!(fixture.open().len(), 1);
        listing.delete_async().await;
        let link = format!("{}/f/1", fixture.server.base_url());
        fixture.listing(&[shared(1050, "1", "2026-10-14T09:00:00Z", &link), old], "\"i\"").await;
        fixture.tick(&mut poller, now() + TimeDelta::days(2)).await;
        let keys: Vec<String> = fixture.open().into_iter().map(|item| item.thread_key).collect();
        assert_eq!(keys, ["files_sharing:remote_share:1"], "the old one stays out; 1041 was deleted");
    }

    #[tokio::test]
    async fn a_muted_source_asks_nothing_and_files_nothing_from_meanwhile() {
        let fixture = Fixture::new().await;
        let capabilities = fixture.capabilities(true).await;
        let listing = fixture.listing(&[], "\"j\"").await;
        let mute = Request::MuteSource { source: fixture.source.clone(), muted: true };
        assert!(matches!(fixture.sources.inbox.handle(mute, None, now()), Response::Ok | Response::Muted(_)));
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        assert_eq!(listing.calls_async().await, 0);
        assert_eq!(capabilities.calls_async().await, 1, "for Talk, which is not muted");
        let talk = Request::MuteSource { source: talk::source_of(&fixture.source), muted: true };
        fixture.sources.inbox.handle(talk, None, now());
        fixture.tick(&mut Poller::default(), now()).await;
        assert_eq!(capabilities.calls_async().await + listing.calls_async().await, 1, "both muted: nothing");
        let unmute = Request::MuteSource { source: fixture.source.clone(), muted: false };
        fixture.sources.inbox.handle(unmute, None, now());
        listing.delete_async().await;
        fixture.listing(&[update(1041, "2026-10-14T09:59:00Z")], "\"k\"").await;
        fixture.tick(&mut poller, now() + TimeDelta::minutes(1)).await;
        assert!(fixture.open().is_empty(), "came while muted");
    }

    #[tokio::test]
    async fn a_refused_sign_in_pauses_until_the_person_signs_in_again() {
        let fixture = Fixture::new().await;
        let capabilities = fixture
            .server
            .mock_async(|when, then| {
                when.method(GET).path(CAPABILITIES_PATH);
                then.status(401);
            })
            .await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        fixture.tick(&mut poller, now()).await;
        assert_eq!(capabilities.calls_async().await, 1, "never again with the refused password");
        assert_eq!(fixture.health(), Some(SourceHealth::SignedOut));
        let mut signed_in_again = fixture.access().await;
        signed_in_again.sign_in = [7; 32];
        poller.tick(&fixture.sources, &fixture.source, &signed_in_again, now()).await;
        assert_eq!(capabilities.calls_async().await, 2);
    }

    #[tokio::test]
    async fn done_sends_nothing_while_the_sign_in_is_refused() {
        let fixture = Fixture::new().await;
        fixture
            .server
            .mock_async(|when, then| {
                when.method(GET).path(CAPABILITIES_PATH);
                then.status(401);
            })
            .await;
        let deleted = fixture
            .server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{LIST_PATH}/1041"));
                then.status(401);
            })
            .await;
        fixture.tick(&mut Poller::default(), now()).await;
        assert_eq!(delete(&fixture.sources, &fixture.source, &["1041".to_string()]).await, 0);
        assert_eq!(deleted.calls_async().await, 0);
    }

    #[tokio::test]
    async fn an_account_without_its_app_password_is_signed_out() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sources = sources_in(dir.path());
        let account = Account::nextcloud("https://cloud.example.org", "anna");
        let source: SourceRef = account.id.parse().expect("a source");
        sources.pim.pretend_connected(account.clone(), "").await;
        follow(&sources, &account.id).await;
        let health = listed(&sources, &source, now()).and_then(|info| info.health);
        assert_eq!(health, Some(SourceHealth::SignedOut));
    }

    #[tokio::test]
    async fn maintenance_waits_and_a_throttle_waits_as_asked() {
        let fixture = Fixture::new().await;
        let maintenance = fixture
            .server
            .mock_async(|when, then| {
                when.method(GET).path(CAPABILITIES_PATH);
                then.status(503);
            })
            .await;
        fixture.sources.inbox.set_source_health(&fixture.source, Some(SourceHealth::Offline));
        assert_eq!(fixture.tick(&mut Poller::default(), now()).await, POLL);
        assert_eq!(fixture.health(), None, "the server is there");
        maintenance.delete_async().await;
        fixture
            .server
            .mock_async(|when, then| {
                when.method(GET).path(CAPABILITIES_PATH);
                then.status(429).header("Retry-After", "300");
            })
            .await;
        assert_eq!(fixture.tick(&mut Poller::default(), now()).await, Duration::from_secs(300));
    }

    #[test]
    fn a_long_matter_is_hashed_and_fits() {
        let short = torro_nextcloud_notification();
        assert_eq!(matter(&short), "deck:card:12");
        let long = Notification { object_id: "x".repeat(300), ..torro_nextcloud_notification() };
        let hashed = matter(&long);
        assert!(hashed.starts_with("deck:#") && hashed.len() <= THREAD_KEY_MAX, "{hashed}");
        assert_eq!(hashed, matter(&long), "the same matter each time");
    }

    #[test]
    fn the_log_names_the_account_and_a_code_only() {
        let source = SourceRef::nextcloud("anna@cloud.example.org");
        let error = OcsError::Other("Nextcloud sent an answer that is not JSON".into());
        assert_eq!(
            log_line(&source, &error),
            "torroworkplace: inbox: Nextcloud notifications of nextcloud:anna@cloud.example.org: error"
        );
        for error in [OcsError::Refused, OcsError::Forbidden, OcsError::Offline, OcsError::MissingApp] {
            assert!(!log_line(&source, &error).contains(PASSWORD));
        }
    }

    #[tokio::test]
    async fn the_access_holds_no_password_in_debug() {
        let fixture = Fixture::new().await;
        let access = fixture.access().await;
        assert!(!format!("{access:?}").contains(PASSWORD));
        assert_ne!(access.sign_in, [0; 32]);
    }
}
