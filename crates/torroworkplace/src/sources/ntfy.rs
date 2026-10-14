//! ntfy's topics as a source of the inbox (task E5.13): one entry per matter of each ntfy account,
//! source `ntfy:<host>`, the account's id (`crate::account::ntfy`).
//!
//! - **One subscription per account** ([`follow`], [`Subscriber::session`]) for all its topics
//!   (`torro_ntfy::Ntfy::subscribe`). [`start`] starts one for every account as it shows up
//!   (`NtfyAccounts::changes`); it ends with the account. The first connection of an account
//!   fetches nothing old: at its `open` the cursor [`SINCE`] is set to the time then, later to the
//!   id of each message, so a new connection asks for what came after it (`since`). A broken
//!   stream, or one that stays silent for two minutes (`torro_ntfy::QUIET`, no `keepalive`
//!   either), is opened again after a growing pause ([`Subscriber::pause`], 1 s doubling to 5
//!   min; back to 1 s after a connection that held a minute). A changed topic opens it anew at
//!   once, from the time of the change, so a new topic brings nothing old either; changed labels
//!   alone keep it open.
//! - **Muted or removed:** a muted source is not subscribed, and muting an open subscription ends
//!   it; what came meanwhile stays out (the cursor is dropped, so the next connection is a first
//!   one). A removed account ends its subscription; its entries were retired by the core
//!   (`Core::retire_sources`).
//! - **Refusals:** 401 and 403 are health `signed_out`, as is a stored login that would travel
//!   without TLS: nothing is asked again until the account changes (a new login or other topics). An unreachable server is `offline`; 429 and other
//!   answers wait for the next try.
//! - **Entry** ([`Subscriber::file`]): kind and lane from `rules::ntfy_place` (priority 5 an alert,
//!   for me and urgent, so it pops up even in quiet times; only ever from topics the person
//!   subscribed to, as `torro_ntfy::Subscription` drops others); title the message's title or its
//!   first line, text the message (without that line); traits `topic` (the person's label, never
//!   the topic's name), `priority` (`1`–`5`), each `tag`; the link only an `http`/`https` `click`;
//!   the message id the server's id. The inbox makes it `untrusted`.
//! - **Matter** ([`matter`]): the topic and the title, so a repeated message ("Backup failed") is
//!   one entry counted again. Both only as hashes: the matter is the entry's `thread_key`, which
//!   clients and remote models see, and a topic's name is a password on a public server. The topic
//!   is hashed with a random salt of the source ([`SALT`], kept in the inbox's cursors), so nobody
//!   can test guessed names against it.
//! - **Throttle** ([`Throttle`]): more than [`THROTTLE_MAX`] new matters within an hour of an
//!   account go, until that hour is over, into one entry per topic, "More from {topic}", in
//!   "Info" and never urgent. Messages of a matter already counted in the hour are filed as usual.
//! - **Reaching back:** none ([`settle_done`]).
//!
//! Logs name the account's id and a code ([`log_line`]), never a topic, a title or a text.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio::time::Instant;
use torro_core::i18n::trf;
use torro_inbox::item::{Facet, FacetKey, SourceHealth};
use torro_inbox::{Item, Kind, Lane, SourceRef, rules};
use torro_ntfy::{Event, Message, MessageId, Ntfy, NtfyError};

use super::Sources;
use super::nextcloud::{is_muted, keep_cursor, read_cursor};
use crate::account::ntfy::NtfyAccess;
use crate::inbox::Remote;

/// The first pause before a connection is opened again.
const PAUSE_FIRST: Duration = Duration::from_secs(1);

/// The longest pause before a connection is opened again.
const PAUSE_MAX: Duration = Duration::from_secs(5 * 60);

/// A connection that held this long was fine: the next pause is [`PAUSE_FIRST`] again, so a
/// server that takes the stream and closes it at once is not asked every second.
const STEADY: Duration = Duration::from_secs(60);

/// How many new matters an account files within [`THROTTLE_WINDOW`]; more go into "More from
/// {topic}".
pub const THROTTLE_MAX: usize = 60;

/// The span [`THROTTLE_MAX`] counts in, from the first new matter on.
pub const THROTTLE_WINDOW: TimeDelta = TimeDelta::hours(1);

/// The cursor with where the stream stopped: the last message's id, or the Unix time of a first
/// connection, sent as `since` when the stream is opened again.
const SINCE: &str = "since";

/// The cursor with the source's random salt for the hash of a topic in a matter ([`matter`]).
const SALT: &str = "salt";

/// Starts the subscriptions of every ntfy account on the current runtime and returns at once: a
/// loop per account ([`follow`]), started as the account shows up (also once the stored accounts
/// were loaded). Without a runtime nothing starts.
pub(super) fn start(sources: Sources) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    runtime.spawn(async move {
        let running: Arc<Mutex<HashSet<String>>> = Arc::default();
        let mut accounts = sources.ntfy.changes();
        loop {
            for id in sources.ntfy.ids().await {
                if !lock(&running).insert(id.clone()) {
                    continue;
                }
                let (sources, running) = (sources.clone(), Arc::clone(&running));
                tokio::spawn(async move {
                    follow(&sources, &id).await;
                    lock(&running).remove(&id);
                });
            }
            if accounts.changed().await.is_err() {
                return;
            }
        }
    });
}

fn lock(running: &Mutex<HashSet<String>>) -> std::sync::MutexGuard<'_, HashSet<String>> {
    running.lock().unwrap_or_else(PoisonError::into_inner)
}

/// ntfy is never told of a done entry: a topic has nothing to mark. Kept so every remote source
/// answers [`super::settle_done`] the same way.
pub(super) fn settle_done(_sources: &Sources, _item: &Item, _ids: Vec<String>) {}

/// The loop of the account `id`: sessions ([`Subscriber::session`]) and the waits they ask for,
/// as long as the account is there. Its health is cleared when it ends, its cursor [`SINCE`] too
/// when the account was removed.
async fn follow(sources: &Sources, id: &str) {
    let Ok(source) = id.parse::<SourceRef>() else { return };
    let mut subscriber = Subscriber::new(source.clone());
    let mut accounts = sources.ntfy.changes();
    let mut inbox = sources.inbox.changes();
    loop {
        // Read after marking, so a change while the session starts is not missed.
        accounts.mark_unchanged();
        let Some(access) = sources.ntfy.access(id).await else { break };
        let gone = match subscriber.session(sources, &access, &mut accounts, &mut inbox).await {
            Ended::Gone => true,
            Ended::Renew => false,
            Ended::Retry => {
                let pause = subscriber.pause();
                tokio::select! {
                    () = tokio::time::sleep(pause) => false,
                    changed = accounts.changed() => changed.is_err(),
                }
            }
            Ended::Paused => accounts.changed().await.is_err(),
            Ended::Muted => tokio::select! {
                changed = inbox.changed() => changed.is_err(),
                changed = accounts.changed() => changed.is_err(),
            },
        };
        if gone {
            break;
        }
    }
    sources.inbox.set_source_health(&source, None);
    // An `open` read just before the removal may have kept a cursor after the core forgot the
    // source's: an account added again starts as a first connection.
    if !sources.ntfy.knows(id).await {
        keep_cursor(sources, &source, SINCE, None);
    }
}

/// Why a session ([`Subscriber::session`]) ended, and so what its loop does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ended {
    /// The account is gone: the loop ends.
    Gone,
    /// The topics, the login or the server changed: open the stream anew at once.
    Renew,
    /// The stream closed or broke, or the server could not be asked: open it again after
    /// [`Subscriber::pause`].
    Retry,
    /// The server refused the login or the topics, or there is nothing to subscribe to: wait for a
    /// change of the account.
    Paused,
    /// The person muted the source: wait until the inbox or the account changes.
    Muted,
}

/// What the loop of one account keeps between its sessions, in memory. `Debug` shows no salt.
pub(super) struct Subscriber {
    source: SourceRef,
    /// The pause before the next try ([`Subscriber::pause`]).
    pause: Duration,
    throttle: Throttle,
    /// The source's salt ([`SALT`]), once read.
    salt: Option<String>,
    /// The clock; tests pass a fixed one.
    clock: fn() -> DateTime<Utc>,
}

impl std::fmt::Debug for Subscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscriber").field("source", &self.source).field("pause", &self.pause).finish()
    }
}

impl Subscriber {
    /// The subscriber of `source` (`ntfy:<host>`), on the system clock.
    pub(super) fn new(source: SourceRef) -> Subscriber {
        Subscriber { source, pause: PAUSE_FIRST, throttle: Throttle::default(), salt: None, clock: Utc::now }
    }

    /// The pause before the stream is opened again after [`Ended::Retry`]: [`PAUSE_FIRST`],
    /// doubled on each call up to [`PAUSE_MAX`], until a connection holds for [`STEADY`].
    pub(super) fn pause(&mut self) -> Duration {
        let pause = self.pause;
        self.pause = (self.pause * 2).min(PAUSE_MAX);
        pause
    }

    /// One session of `access` (see the module docs): nothing while the source is muted, else opens
    /// the stream of its topics with `since` from the cursor and files each message, until the
    /// stream ends or breaks, the person mutes the source (`inbox`, the inbox's changes) or the
    /// account changes (`accounts`, `NtfyAccounts::changes`) so that the stream is another one or
    /// none. Sets the source's health: none once the stream is open, `signed_out` for a refusal,
    /// `offline` when the server cannot be reached.
    pub(super) async fn session(
        &mut self,
        sources: &Sources,
        access: &NtfyAccess,
        accounts: &mut watch::Receiver<u64>,
        inbox: &mut watch::Receiver<()>,
    ) -> Ended {
        if self.muted(sources) {
            return Ended::Muted;
        }
        let names = access.names();
        if names.is_empty() {
            sources.inbox.set_source_health(&self.source, None);
            return Ended::Paused;
        }
        let ntfy = match Ntfy::new(access.server.clone(), access.login.clone()) {
            Ok(ntfy) => ntfy,
            Err(error) => return self.failed(sources, &error),
        };
        let since = read_cursor(sources, &self.source, SINCE).and_then(|since| MessageId::parse(&since));
        let mut subscription = match ntfy.subscribe(&names, since.as_ref()).await {
            Ok(subscription) => subscription,
            Err(error) => return self.failed(sources, &error),
        };
        sources.inbox.set_source_health(&self.source, None);
        let opened = Instant::now();
        let mut access = access.clone();
        let ended = loop {
            tokio::select! {
                event = subscription.next() => match event {
                    Ok(Some(Event::Open)) => self.opened(sources),
                    Ok(Some(Event::Keepalive)) => {}
                    Ok(Some(Event::Message(message))) => self.file(sources, &access, &message).await,
                    // A stream that ends or breaks is opened again; whether the server is there,
                    // the next try finds out.
                    Ok(None) | Err(_) => break Ended::Retry,
                },
                changed = accounts.changed() => {
                    if changed.is_err() {
                        break Ended::Gone;
                    }
                    match sources.ntfy.access(&self.source.key()).await {
                        None => break Ended::Gone,
                        Some(fresh) if fresh.same_stream(&access) => access = fresh,
                        Some(fresh) => {
                            if fresh.names() != access.names() {
                                // A new topic brings nothing old: on from the change.
                                self.mark_now(sources);
                            }
                            break Ended::Renew;
                        }
                    }
                },
                changed = inbox.changed() => {
                    if changed.is_err() {
                        break Ended::Gone;
                    }
                    if self.muted(sources) {
                        break Ended::Muted;
                    }
                },
            }
        };
        if opened.elapsed() >= STEADY {
            self.pause = PAUSE_FIRST;
        }
        ended
    }

    /// Whether the person muted the source. Then what comes meanwhile stays out: the cursor goes,
    /// so the next connection is a first one.
    fn muted(&self, sources: &Sources) -> bool {
        let muted = is_muted(sources, &self.source, (self.clock)());
        if muted {
            keep_cursor(sources, &self.source, SINCE, None);
            sources.inbox.set_source_health(&self.source, None);
        }
        muted
    }

    /// The stream is open: a first connection (no cursor) goes on from now next time.
    fn opened(&self, sources: &Sources) {
        if read_cursor(sources, &self.source, SINCE).is_none() {
            self.mark_now(sources);
        }
    }

    /// Keeps the time now as the cursor ([`SINCE`]): ntfy takes a Unix time for `since` too.
    fn mark_now(&self, sources: &Sources) {
        keep_cursor(sources, &self.source, SINCE, Some(&(self.clock)().timestamp().to_string()));
    }

    /// After the server could not be asked with `error`: the health it means and how the loop
    /// goes on. Logged with a code only.
    fn failed(&self, sources: &Sources, error: &NtfyError) -> Ended {
        eprintln!("{}", log_line(&self.source, error));
        let (health, ended) = match error {
            NtfyError::Auth => (Some(SourceHealth::SignedOut), Ended::Paused),
            NtfyError::Offline => (Some(SourceHealth::Offline), Ended::Retry),
            NtfyError::Busy | NtfyError::Server(_) => (None, Ended::Retry),
            // A login that would travel without TLS needs a new one from the person.
            NtfyError::InsecureLogin => (Some(SourceHealth::SignedOut), Ended::Paused),
            // A stored address or topic is checked when stored; one that fails waits for a change.
            NtfyError::InvalidServer | NtfyError::InvalidTopic => (None, Ended::Paused),
        };
        sources.inbox.set_source_health(&self.source, health);
        ended
    }

    /// Files `message` of one of the topics of `access` (see the module docs), then keeps its id
    /// as the cursor; failures to the log. Nothing once the account was removed, also when the
    /// stream still delivered a message before the session noticed: the core retired its entries
    /// and cursors then (`Core::retire_sources`), so none may come back.
    async fn file(&mut self, sources: &Sources, access: &NtfyAccess, message: &Message) {
        if !sources.ntfy.knows(&self.source.key()).await {
            return;
        }
        let now = (self.clock)();
        if let Some(label) = access.label(&message.topic)
            && let Some(salt) = self.salt(sources)
        {
            let topic = hash(&format!("{salt}\n{}", message.topic.as_str()), 16);
            let (title, body) = texts(message);
            let matter = matter(&topic, &title);
            let remote = match self.throttle.admit(&matter, now) {
                true => report(&self.source, matter, label, title, body, message),
                false => more(&self.source, &topic, label, &title, message),
            };
            if let Err(err) = sources.inbox.file_remote(&remote, now) {
                eprintln!("torroworkplace: inbox: cannot file a message of {}: {err}", self.source);
            }
        }
        keep_cursor(sources, &self.source, SINCE, Some(message.id.as_str()));
    }

    /// The source's salt ([`SALT`]): read from the cursors, or made and kept there the first time.
    /// `None` when it can be neither read nor made (logged); then nothing is filed.
    fn salt(&mut self, sources: &Sources) -> Option<String> {
        if self.salt.is_none() {
            self.salt = match sources.inbox.cursor(&self.source, SALT) {
                Ok(Some(salt)) => Some(salt),
                Ok(None) => new_salt(sources, &self.source),
                Err(err) => {
                    eprintln!("torroworkplace: inbox: cannot read the salt of {}: {err}", self.source);
                    None
                }
            };
        }
        self.salt.clone()
    }
}

/// A new random salt for `source`, kept in its cursors.
fn new_salt(sources: &Sources, source: &SourceRef) -> Option<String> {
    let mut bytes = [0u8; 16];
    if let Err(err) = getrandom::fill(&mut bytes) {
        eprintln!("torroworkplace: inbox: no salt for {source}: {err}");
        return None;
    }
    let salt = hex(&bytes);
    sources
        .inbox
        .set_cursor(source, SALT, Some(&salt))
        .map_err(|err| eprintln!("torroworkplace: inbox: {err}"))
        .ok()?;
    Some(salt)
}

/// Counts the new matters of an account in a window of [`THROTTLE_WINDOW`] that starts with the
/// first of them, in memory.
#[derive(Debug, Default)]
pub(super) struct Throttle {
    /// When the window began; `None` before the first matter.
    since: Option<DateTime<Utc>>,
    /// The matters counted in the window.
    matters: HashSet<String>,
}

impl Throttle {
    /// Whether a message of `matter` at `now` is filed as itself: a matter counted in the window
    /// is; a new one is counted while fewer than [`THROTTLE_MAX`] are, else it goes into "More
    /// from {topic}" until the window is over. A new window starts with the first matter after the
    /// last one ended.
    pub(super) fn admit(&mut self, matter: &str, now: DateTime<Utc>) -> bool {
        if self.since.is_none_or(|since| now - since >= THROTTLE_WINDOW) {
            self.since = Some(now);
            self.matters.clear();
        }
        if self.matters.contains(matter) {
            return true;
        }
        if self.matters.len() >= THROTTLE_MAX {
            return false;
        }
        self.matters.insert(matter.to_string());
        true
    }
}

/// Title and text of `message`: its title and its text, or without a title the first line of the
/// text and the rest of it.
fn texts(message: &Message) -> (String, Option<String>) {
    let text = message.text.trim();
    let (title, body) = match message.title.as_deref() {
        Some(title) => (title.trim(), text),
        None => text.split_once('\n').map_or((text, ""), |(first, rest)| (first.trim(), rest.trim())),
    };
    (title.to_string(), Some(body.to_string()).filter(|body| !body.is_empty()))
}

/// The matter of a message with `title` in the topic hashed as `topic` (see the module docs):
/// `<topic>:<hash of the title>`, 41 bytes.
fn matter(topic: &str, title: &str) -> String {
    format!("{topic}:{}", hash(title, 24))
}

/// The report of `message` in the topic labelled `label` (see the module docs).
fn report(
    source: &SourceRef,
    matter: String,
    label: &str,
    title: String,
    body: Option<String>,
    message: &Message,
) -> Remote {
    let (kind, lane) = rules::ntfy_place(message.priority);
    let mut facets = vec![
        Facet { key: FacetKey::Topic, value: label.to_string() },
        Facet { key: FacetKey::Priority, value: message.priority.to_string() },
    ];
    facets.extend(message.tags.iter().map(|tag| Facet { key: FacetKey::Tag, value: tag.clone() }));
    Remote {
        source: source.clone(),
        matter,
        ids: vec![message.id.to_string()],
        kind,
        lane,
        title,
        body,
        from: None,
        // Checked by `torro_ntfy` (http or https only) and again by the inbox.
        url: message.click.clone(),
        facets,
        date: message.time,
    }
}

/// The throttled report of `message` (title `title`) in the topic hashed as `topic` and labelled
/// `label`: into "More from {topic}", in "Info", never urgent, the message's title as its text.
fn more(source: &SourceRef, topic: &str, label: &str, title: &str, message: &Message) -> Remote {
    Remote {
        source: source.clone(),
        matter: format!("{topic}:more"),
        ids: vec![message.id.to_string()],
        kind: Kind::Notice,
        lane: Lane::Info,
        title: trf("More from {topic}", &[("topic", &label)]),
        body: Some(title.to_string()).filter(|title| !title.is_empty()),
        from: None,
        url: None,
        facets: vec![Facet { key: FacetKey::Topic, value: label.to_string() }],
        date: message.time,
    }
}

/// The first `chars` hex digits of the SHA-256 of `text`.
fn hash(text: &str, chars: usize) -> String {
    let mut digest = hex(&Sha256::digest(text.as_bytes()));
    digest.truncate(chars);
    digest
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The log line for `error` of `source`: the account's id and the error's code, nothing the server
/// sent and no topic.
fn log_line(source: &SourceRef, error: &NtfyError) -> String {
    format!("torroworkplace: inbox: ntfy topics of {source}: {}", error.code())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use chrono::TimeZone;
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use torro_inbox::Link;
    use torro_ipc::{Request, Response};

    use super::*;
    use crate::account::github::tests::{MemoryTokens, NO_CLI, service};
    use crate::account::ntfy::tests::service as ntfy_service;
    use crate::account::ntfy::tests::topic as account_topic;
    use crate::inbox_notify::{Popup, Popups};
    use crate::pim::PimService;
    use crate::settings_service::SettingsStore;

    const TOPIC: &str = "backups_x7Qp2";
    const OTHER: &str = "alerts_9Kd";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 10, 0, 0).single().expect("valid time")
    }

    /// A message `id` of [`TOPIC`], sent an hour before `now()`.
    fn message(id: &str, title: Option<&str>, text: &str, priority: u8) -> serde_json::Value {
        let time = now().timestamp() - 3600;
        let mut line = json!({
            "id": id, "time": time, "event": "message", "topic": TOPIC, "message": text, "priority": priority,
        });
        if let Some(title) = title {
            line["title"] = json!(title);
        }
        line
    }

    fn body(lines: &[serde_json::Value]) -> String {
        lines.iter().map(|line| format!("{line}\n")).collect()
    }

    fn open() -> serde_json::Value {
        json!({ "id": "open000001", "time": now().timestamp(), "event": "open", "topic": TOPIC })
    }

    fn keepalive() -> serde_json::Value {
        json!({ "id": "keep000001", "time": now().timestamp(), "event": "keepalive", "topic": TOPIC })
    }

    /// An ntfy account with the topic [`TOPIC`] labelled "Backups" on `server`, its inbox in a
    /// temp directory.
    struct Fixture {
        _dir: tempfile::TempDir,
        sources: Sources,
        source: SourceRef,
        subscriber: Subscriber,
        accounts: watch::Receiver<u64>,
        inbox: watch::Receiver<()>,
    }

    impl Fixture {
        /// Connects the account on `base` (an ntfy server that answers `/v1/health` and polls).
        async fn new(base: &str) -> Fixture {
            let dir = tempfile::tempdir().expect("temp dir");
            let sources = sources_in(dir.path());
            let info = sources.ntfy.connect(base, &[account_topic(TOPIC, "Backups")]).await.expect("connected");
            let source: SourceRef = info.id.parse().expect("a source");
            let mut subscriber = Subscriber::new(source.clone());
            subscriber.clock = now;
            let (accounts, inbox) = (sources.ntfy.changes(), sources.inbox.changes());
            Fixture { _dir: dir, sources, source, subscriber, accounts, inbox }
        }

        async fn access(&self) -> NtfyAccess {
            self.sources.ntfy.access(&self.source.key()).await.expect("the account")
        }

        async fn session(&mut self) -> Ended {
            let access = self.access().await;
            self.subscriber.session(&self.sources, &access, &mut self.accounts, &mut self.inbox).await
        }

        /// The open entries of the source.
        fn open(&self) -> Vec<Item> {
            let open = self.sources.inbox.open_remote(&self.source).expect("open entries");
            open.iter().map(|entry| self.item(&entry.id)).collect()
        }

        fn item(&self, id: &str) -> Item {
            match self.sources.inbox.handle(Request::GetItem { id: id.into() }, None, now()) {
                Response::Item { item } => item,
                other => panic!("expected an item, got {other:?}"),
            }
        }

        fn health(&self) -> Option<SourceHealth> {
            match self.sources.inbox.handle(Request::ListSources, None, now()) {
                Response::Sources { sources } => {
                    sources.into_iter().find(|info| info.source == self.source).and_then(|info| info.health)
                }
                other => panic!("expected sources, got {other:?}"),
            }
        }

        fn since(&self) -> Option<String> {
            self.sources.inbox.cursor(&self.source, SINCE).expect("cursor")
        }

        fn mute(&self, muted: bool) {
            let request = Request::MuteSource { source: self.source.clone(), muted };
            assert_eq!(self.sources.inbox.handle(request, None, now()), Response::Ok);
        }
    }

    fn sources_in(dir: &Path) -> Sources {
        let settings = SettingsStore::at(dir.join("config").join("settings.json"));
        let github = service(dir, &MemoryTokens::default(), "http://127.0.0.1:9/graphql", NO_CLI);
        Sources {
            inbox: Arc::new(crate::inbox::tests::inbox_in(dir)),
            github: Arc::new(github),
            pim: Arc::new(PimService::with_settings(settings)),
            ntfy: Arc::new(ntfy_service(dir, &MemoryTokens::default())),
        }
    }

    /// A fake ntfy server: healthy, every topic may be polled.
    async fn fake_ntfy() -> MockServer {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/health");
                then.status(200).json_body(json!({"healthy": true}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).query_param("poll", "1");
                then.status(200).body("");
            })
            .await;
        server
    }

    /// The stream of [`TOPIC`] answers `lines` to a request whose `since` is `since` (`None`: none).
    async fn stream<'a>(
        server: &'a MockServer,
        since: Option<&str>,
        lines: &[serde_json::Value],
    ) -> httpmock::Mock<'a> {
        let body = body(lines);
        let since = since.map(str::to_string);
        server
            .mock_async(|when, then| {
                let when = when.method(GET).path(format!("/{TOPIC}/json")).query_param_missing("poll");
                match since {
                    Some(since) => when.query_param("since", since),
                    None => when.query_param_missing("since"),
                };
                then.status(200).body(body);
            })
            .await
    }

    fn facet(item: &Item, key: FacetKey) -> Vec<&str> {
        item.facets.iter().filter(|facet| facet.key == key).map(|facet| facet.value.as_str()).collect()
    }

    #[tokio::test]
    async fn messages_are_entries_with_their_lane_traits_and_link() {
        let server = fake_ntfy().await;
        let mut fixture = Fixture::new(&server.base_url()).await;
        let mut alert = message("h8t4Fgaw7w", Some("Backup failed"), "The backup of nas-01 stopped.\nDisk full.", 5);
        alert["tags"] = json!(["warning", "floppy_disk"]);
        alert["click"] = json!("https://nas.example.org/backups/42");
        let mut calm = message("hwQ2YpKdmg", None, "Disk almost full\nAt 91 %.", 2);
        calm["click"] = json!("javascript:alert(1)");
        let feed = stream(&server, None, &[open(), alert, keepalive(), calm]).await;

        assert_eq!(fixture.session().await, Ended::Retry, "the stream ended");
        assert_eq!(feed.calls_async().await, 1);
        assert_eq!(fixture.health(), None);
        let open = fixture.open();
        assert_eq!(open.len(), 2, "the keepalive is no entry");
        let alert = open.iter().find(|item| item.kind == Kind::Alert).expect("the alert");
        assert_eq!((alert.lane, alert.urgent, alert.untrusted), (Lane::ForMe, true, true));
        assert_eq!(alert.title, "Backup failed");
        assert_eq!(alert.body.as_deref(), Some("The backup of nas-01 stopped.\nDisk full."));
        assert_eq!(alert.link, Some(Link::Url { url: "https://nas.example.org/backups/42".into() }));
        assert_eq!(facet(alert, FacetKey::Topic), ["Backups"], "the label, never the topic's name");
        assert_eq!(facet(alert, FacetKey::Priority), ["5"]);
        assert_eq!(facet(alert, FacetKey::Tag), ["warning", "floppy_disk"]);
        let calm = open.iter().find(|item| item.kind == Kind::Notice).expect("the notice");
        assert_eq!((calm.lane, calm.urgent), (Lane::Info, false), "priority 2 is information");
        assert_eq!((calm.title.as_str(), calm.body.as_deref()), ("Disk almost full", Some("At 91 %.")));
        assert_eq!(calm.link, None, "only http and https");
        for item in &open {
            let shown = serde_json::to_string(item).expect("json");
            assert!(!shown.contains(TOPIC) && !item.thread_key.contains("Backup"), "{shown}");
        }
        assert_eq!(fixture.since().as_deref(), Some("hwQ2YpKdmg"), "the last id seen");
    }

    #[tokio::test]
    async fn the_same_title_counts_again_and_another_topic_is_another_matter() {
        let server = fake_ntfy().await;
        let mut fixture = Fixture::new(&server.base_url()).await;
        let mut other = message("Zq3LmN8pRt", Some("Backup failed"), "elsewhere", 4);
        other["topic"] = json!(OTHER);
        let lines = [
            message("h8t4Fgaw7w", Some("Backup failed"), "first", 4),
            message("Kp7Vw2XyZa", Some("Backup failed"), "second", 4),
            other,
        ];
        stream(&server, None, &lines).await;
        assert_eq!(fixture.session().await, Ended::Retry);
        let open = fixture.open();
        assert_eq!(open.len(), 1, "the unsubscribed topic is dropped by the stream");
        assert_eq!((open[0].count, open[0].body.as_deref(), open[0].lane), (2, Some("second"), Lane::ForMe));

        // The matter hashes the topic with the source's salt, so the same title in another topic
        // (or of another account) is another matter.
        let (title, salt) = ("Backup failed", fixture.subscriber.salt(&fixture.sources).expect("salt"));
        let here = matter(&hash(&format!("{salt}\n{TOPIC}"), 16), title);
        assert_eq!(open[0].thread_key, here);
        assert_ne!(here, matter(&hash(&format!("{salt}\n{OTHER}"), 16), title));
        assert_ne!(here, matter(&hash(TOPIC, 16), title), "unsalted, a guessed name could be tested");
    }

    #[tokio::test]
    async fn an_alert_pops_up_even_in_quiet_times_and_a_notice_for_me_waits() {
        let server = fake_ntfy().await;
        let mut fixture = Fixture::new(&server.base_url()).await;
        let mut popups = Popups::new();
        assert_eq!(popups.step(now(), &[], Some("Client call")), [], "the first look learns");
        let lines = [message("h8t4Fgaw7w", Some("Server down"), "", 5), message("Kp7Vw2XyZa", Some("Slow"), "", 4)];
        stream(&server, None, &lines).await;
        fixture.session().await;
        let open = fixture.sources.inbox.for_me(now(), 10).expect("for me").newest;
        assert_eq!(open.len(), 2);
        let shown = popups.step(now(), &open, Some("Client call"));
        let ids: Vec<&str> = shown.iter().filter_map(Popup::entry).collect();
        let alert = open.iter().find(|item| item.title == "Server down").expect("alert");
        assert_eq!(ids, [alert.id.as_str()], "only the alert during the busy event");
        assert!(shown[0].urgent());
    }

    #[tokio::test]
    async fn a_new_connection_asks_since_where_the_last_one_stopped() {
        let server = fake_ntfy().await;
        let mut fixture = Fixture::new(&server.base_url()).await;
        let first = stream(&server, None, &[open()]).await;
        assert_eq!(fixture.session().await, Ended::Retry);
        assert_eq!(first.calls_async().await, 1, "the first connection fetches nothing old");
        let stamp = now().timestamp().to_string();
        assert_eq!(fixture.since(), Some(stamp.clone()), "on from the time of the first connection");

        let second = stream(&server, Some(&stamp), &[open(), message("h8t4Fgaw7w", Some("Up"), "", 3)]).await;
        assert_eq!(fixture.session().await, Ended::Retry);
        assert_eq!(second.calls_async().await, 1);
        let third = stream(&server, Some("h8t4Fgaw7w"), &[open()]).await;
        assert_eq!(fixture.session().await, Ended::Retry);
        assert_eq!(third.calls_async().await, 1, "since the last message");
        assert_eq!(fixture.open().len(), 1, "nothing twice");
    }

    #[test]
    fn the_pause_grows_from_a_second_to_five_minutes() {
        let mut subscriber = Subscriber::new(SourceRef::ntfy("ntfy.sh"));
        let pauses: Vec<u64> = (0..11).map(|_| subscriber.pause().as_secs()).collect();
        assert_eq!(pauses, [1, 2, 4, 8, 16, 32, 64, 128, 256, 300, 300]);
    }

    #[test]
    fn the_throttle_counts_new_matters_for_an_hour() {
        let mut throttle = Throttle::default();
        for n in 0..THROTTLE_MAX {
            assert!(throttle.admit(&format!("m{n}"), now()), "{n}");
        }
        assert!(!throttle.admit("one more", now() + TimeDelta::minutes(30)));
        assert!(throttle.admit("m3", now() + TimeDelta::minutes(59)), "a matter counted goes on");
        assert!(!throttle.admit("one more", now() + TimeDelta::minutes(59)));
        assert!(throttle.admit("one more", now() + THROTTLE_WINDOW), "a new hour");
    }

    #[tokio::test]
    async fn a_flood_goes_into_more_from_its_topic() {
        let server = fake_ntfy().await;
        let mut fixture = Fixture::new(&server.base_url()).await;
        let mut lines: Vec<serde_json::Value> = (0..=THROTTLE_MAX)
            .map(|n| message(&format!("msg{n:04}"), Some(&format!("Job {n} failed")), "", 5))
            .collect();
        lines.push(message("again00001", Some("Job 0 failed"), "", 5));
        stream(&server, None, &lines).await;
        fixture.session().await;
        let open = fixture.open();
        assert_eq!(open.len(), THROTTLE_MAX + 1);
        let more = open.iter().find(|item| item.title == trf("More from {topic}", &[("topic", &"Backups")]));
        let more = more.expect("the collected entry");
        assert_eq!((more.kind, more.lane, more.urgent), (Kind::Notice, Lane::Info, false));
        assert_eq!(more.body.as_deref(), Some("Job 60 failed"));
        let job = open.iter().find(|item| item.title == "Job 0 failed").expect("first job");
        assert_eq!((job.count, job.kind), (2, Kind::Alert), "a matter counted in the hour goes on");
    }

    #[tokio::test]
    async fn a_muted_source_is_not_subscribed_and_drops_what_came_meanwhile() {
        let server = fake_ntfy().await;
        let mut fixture = Fixture::new(&server.base_url()).await;
        fixture.sources.inbox.set_cursor(&fixture.source, SINCE, Some("h8t4Fgaw7w")).expect("cursor");
        let feed = stream(&server, None, &[open()]).await;
        fixture.mute(true);
        assert_eq!(fixture.session().await, Ended::Muted);
        assert_eq!(feed.calls_async().await, 0, "nothing asked");
        assert_eq!(fixture.since(), None);
        fixture.mute(false);
        assert_eq!(fixture.session().await, Ended::Retry);
        assert_eq!(feed.calls_async().await, 1, "a first connection again");
    }

    #[tokio::test]
    async fn a_refusal_is_signed_out_and_unreachable_is_offline() {
        let server = fake_ntfy().await;
        let mut fixture = Fixture::new(&server.base_url()).await;
        let refused = server
            .mock_async(|when, then| {
                when.method(GET).path(format!("/{TOPIC}/json")).query_param_missing("poll");
                then.status(401)
                    .json_body(json!({"code": 40101, "http": 401, "error": format!("{TOPIC} unauthorized")}));
            })
            .await;
        assert_eq!(fixture.session().await, Ended::Paused, "waits for a new login");
        assert_eq!(fixture.health(), Some(SourceHealth::SignedOut));
        refused.delete_async().await;
        stream(&server, None, &[open()]).await;
        assert_eq!(fixture.session().await, Ended::Retry);
        assert_eq!(fixture.health(), None, "cleared once the stream opens");

        let gone = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = gone.local_addr().expect("address").port();
        drop(gone);
        let mut access = fixture.access().await;
        access.server = torro_ntfy::Server::parse(&format!("http://127.0.0.1:{port}")).expect("address");
        let ended = fixture.subscriber.session(&fixture.sources, &access, &mut fixture.accounts, &mut fixture.inbox);
        assert_eq!(ended.await, Ended::Retry);
        assert_eq!(fixture.health(), Some(SourceHealth::Offline));
    }

    #[tokio::test]
    async fn a_message_read_after_the_account_was_removed_is_not_filed() {
        let server = fake_ntfy().await;
        let mut fixture = Fixture::new(&server.base_url()).await;
        let access = fixture.access().await;
        fixture.sources.ntfy.remove(&fixture.source.key()).await.expect("removed");
        let line = message("h8t4Fgaw7w", Some("Backup failed"), "", 5);
        let Some(Event::Message(late)) = torro_ntfy::read_event(line.to_string().as_bytes()) else {
            panic!("a message")
        };
        fixture.subscriber.file(&fixture.sources, &access, &late).await;
        assert!(fixture.open().is_empty());
        assert_eq!(fixture.since(), None, "no cursor kept for a removed account");
    }

    #[test]
    fn logs_and_debug_name_no_topic() {
        let source = SourceRef::ntfy("ntfy.sh");
        for error in [NtfyError::Auth, NtfyError::Offline, NtfyError::Server(format!("{TOPIC} broke"))] {
            let line = log_line(&source, &error);
            assert!(line.contains("ntfy:ntfy.sh") && !line.contains(TOPIC), "{line}");
        }
        let access = NtfyAccess {
            server: torro_ntfy::Server::parse("").expect("server"),
            topics: vec![crate::account::ntfy::AccessTopic {
                name: torro_ntfy::Topic::parse(TOPIC).expect("topic"),
                label: "Backups".into(),
            }],
            login: Some(torro_ntfy::Login::new(None, torro_core::account::Secret::new("tk_SECRET"))),
        };
        let shown = format!("{access:?} {:?}", Subscriber::new(source));
        assert!(!shown.contains(TOPIC) && !shown.contains("SECRET") && shown.contains("Backups"), "{shown}");
    }

    /// An ntfy server on this computer that is healthy, lets every topic be polled and keeps every
    /// stream open after its `open` event, so a test can change things while it runs. Returns its
    /// address and how many streams were opened.
    async fn holding_ntfy() -> (String, Arc<Mutex<usize>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let streams = Arc::new(Mutex::new(0));
        let counted = Arc::clone(&streams);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let counted = Arc::clone(&counted);
                tokio::spawn(async move {
                    let mut head = vec![0u8; 8192];
                    let read = socket.read(&mut head).await.unwrap_or_default();
                    let line = String::from_utf8_lossy(&head[..read]).lines().next().unwrap_or_default().to_string();
                    let answer = |body: &str| {
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    };
                    let reply = if line.contains("/v1/health") {
                        answer(r#"{"healthy":true}"#)
                    } else if line.contains("poll=1") {
                        answer("")
                    } else {
                        *counted.lock().expect("lock") += 1;
                        format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}\n", open())
                    };
                    let _ = socket.write_all(reply.as_bytes()).await;
                    tokio::time::sleep(Duration::from_secs(600)).await;
                });
            }
        });
        (base, streams)
    }

    /// Runs a session of `fixture` in the background and waits until its stream is open.
    async fn running(fixture: Fixture, streams: &Arc<Mutex<usize>>) -> tokio::task::JoinHandle<(Fixture, Ended)> {
        let before = *streams.lock().expect("lock");
        let task = tokio::spawn(async move {
            let mut fixture = fixture;
            let ended = fixture.session().await;
            (fixture, ended)
        });
        for _ in 0..500 {
            if *streams.lock().expect("lock") > before {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        task
    }

    async fn finished(task: tokio::task::JoinHandle<(Fixture, Ended)>) -> (Fixture, Ended) {
        tokio::time::timeout(Duration::from_secs(10), task).await.expect("the session ended").expect("no panic")
    }

    #[tokio::test]
    async fn muting_ends_an_open_subscription() {
        let (base, streams) = holding_ntfy().await;
        let fixture = Fixture::new(&base).await;
        let (sources, source) = (fixture.sources.clone(), fixture.source.clone());
        let task = running(fixture, &streams).await;
        let request = Request::MuteSource { source, muted: true };
        assert_eq!(sources.inbox.handle(request, None, now()), Response::Ok);
        let (fixture, ended) = finished(task).await;
        assert_eq!(ended, Ended::Muted);
        assert_eq!(fixture.since(), None);
    }

    #[tokio::test]
    async fn a_changed_topic_renews_new_labels_do_not_and_removing_ends_it() {
        let (base, streams) = holding_ntfy().await;
        let fixture = Fixture::new(&base).await;
        let (sources, id) = (fixture.sources.clone(), fixture.source.key());

        let task = running(fixture, &streams).await;
        sources.ntfy.set_topic(&id, TOPIC, Some("Nightly backups"), true).await.expect("relabeled");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!task.is_finished(), "a new label keeps the stream");
        sources.ntfy.set_topic(&id, OTHER, Some("Alerts"), true).await.expect("added");
        let (fixture, ended) = finished(task).await;
        assert_eq!(ended, Ended::Renew);
        assert_eq!(fixture.since(), Some(now().timestamp().to_string()), "a new topic brings nothing old");
        let labels: BTreeSet<String> = fixture.access().await.topics.into_iter().map(|topic| topic.label).collect();
        assert_eq!(labels, BTreeSet::from(["Alerts".to_string(), "Nightly backups".to_string()]));

        let task = running(fixture, &streams).await;
        sources.ntfy.remove(&id).await.expect("removed");
        assert_eq!(finished(task).await.1, Ended::Gone);
    }

    #[tokio::test]
    async fn start_subscribes_each_account_and_its_loop_ends_with_it() {
        let (base, streams) = holding_ntfy().await;
        let fixture = Fixture::new(&base).await;
        start(fixture.sources.clone());
        for _ in 0..500 {
            if *streams.lock().expect("lock") > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(*streams.lock().expect("lock"), 1, "one stream for the account");
        fixture.sources.ntfy.remove(&fixture.source.key()).await.expect("removed");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(*streams.lock().expect("lock"), 1, "not opened again");
    }

    #[test]
    fn texts_take_the_first_line_without_a_title() {
        let message = |title: Option<&str>, text: &str| Message {
            id: MessageId::parse("a1").expect("id"),
            time: now(),
            topic: torro_ntfy::Topic::parse(TOPIC).expect("topic"),
            title: title.map(str::to_string),
            text: text.into(),
            priority: 3,
            tags: Vec::new(),
            click: None,
        };
        assert_eq!(texts(&message(Some(" Title "), " Text ")), ("Title".into(), Some("Text".into())));
        assert_eq!(texts(&message(None, "Line one\n\nLine two")), ("Line one".into(), Some("Line two".into())));
        assert_eq!(texts(&message(None, "Only")), ("Only".into(), None));
        assert_eq!(texts(&message(None, "")), (String::new(), None));
    }
}
