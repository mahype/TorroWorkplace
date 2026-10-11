//! Calendars and tasks in the core: the account, the CalDAV connection, an in-memory cache
//! and the sync loop. The apps reach it through the socket (`torro_ipc::pim::PimClient`).
//!
//! - One Nextcloud account (`crate::account`, secret in the keychain).
//! - The cache holds all collections, all todos and the events from 3 months back to 12
//!   months ahead, fetched on start, every 5 minutes, after a `Sync` request and after
//!   sign-in. Reads are answered from the cache, so the apps never wait for the network;
//!   ranges outside the window are fetched from the server directly.
//! - Writes go to the server first (ETag-guarded) and update the cache with the answer.
//!   Recurring occurrences are refused until series editing exists, because writing one
//!   would overwrite the whole series.
//! - When the server refuses the app password (401/403), the core asks once whether this device
//!   was marked for a remote wipe and, if so, removes the account, its secret and the cache
//!   (task S.12, `account::remote_wipe`).
//! - An SQLite cache that survives restarts comes later.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Duration as Span, Local, Utc};
use tokio::sync::{Mutex, RwLock};
use torro_ipc::{Request, Response};
use torro_pim::{AccountState, Collection, Event, EventTime, ProviderError, Todo};
use torro_pim_caldav::CalDavClient;
use torro_pim_caldav::nextcloud;

use torro_core::i18n::tr;

use crate::account::{self, Connected};
use crate::notify;

/// How often the cache is refreshed in the background.
const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);
/// The cached event window around now.
const WINDOW_BACK_DAYS: i64 = 90;
const WINDOW_AHEAD_DAYS: i64 = 365;

#[derive(Debug, Default)]
struct State {
    account: Option<Connected>,
    client: Option<CalDavClient>,
    /// Set while Login Flow v2 runs.
    signing_in: Option<(String, String)>,
    last_sync: Option<DateTime<Utc>>,
    problem: Option<String>,
    collections: Vec<Collection>,
    events: Vec<Event>,
    /// The range `events` covers; `None` before the first successful sync.
    window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    todos: Vec<Todo>,
    /// The remote wipe check already ran since the server started refusing the app password.
    /// Nextcloud rate-limits the check, and asking again on every failed request would not
    /// change the answer; a successful sync or a new sign-in clears it.
    wipe_checked: bool,
}

/// See the module docs.
#[derive(Debug)]
pub struct PimService {
    http: Option<reqwest::Client>,
    state: RwLock<State>,
    /// Only one sync at a time.
    syncing: Mutex<()>,
}

impl PimService {
    /// Loads the stored account, if any. Does not touch the network.
    pub fn new() -> PimService {
        let http = match torro_pim_caldav::http_client() {
            Ok(http) => Some(http),
            Err(err) => {
                eprintln!("torroworkplace: no HTTP client: {err}");
                None
            }
        };
        PimService { http, state: RwLock::new(State::default()), syncing: Mutex::new(()) }
    }

    /// Loads the stored account, then syncs now and every few minutes, for as long as the
    /// daemon runs.
    pub fn start(self: &Arc<Self>) {
        let service = Arc::clone(self);
        tokio::spawn(async move {
            match account::keychain(account::stored).await {
                Ok(Some(connected)) => {
                    let mut state = service.state.write().await;
                    state.client = service.http.as_ref().and_then(|http| account::caldav(http, &connected).ok());
                    state.account = Some(connected);
                }
                Ok(None) => {}
                Err(err) => eprintln!("torroworkplace: cannot read the account: {err}"),
            }
            loop {
                let connected = service.state.read().await.client.is_some();
                if connected && let Err(err) = service.sync().await {
                    eprintln!("torroworkplace: sync failed: {err}");
                }
                tokio::time::sleep(SYNC_EVERY).await;
            }
        });
    }

    /// Answers the calendar, task and account requests; other requests are not ours.
    pub async fn handle(self: &Arc<Self>, request: Request) -> Response {
        match request {
            Request::ConnectNextcloud { server } => self.connect(server).await,
            // `sync` runs the remote wipe check itself.
            Request::Sync => match self.sync().await {
                Ok(()) => Response::Ok,
                Err(err) => provider_error(&err),
            },
            Request::CalendarCollections => self.shown_collections(|c| c.kind.has_events()).await,
            Request::TaskCollections => self.shown_collections(|c| c.kind.has_todos()).await,
            Request::Events { start, end } => match self.events(start, end).await {
                Ok(events) => {
                    Response::Events { events: without_hidden(events, &hidden_collections(), |e| &e.collection) }
                }
                Err(err) => self.failed(err).await,
            },
            Request::Todos => {
                let todos = self.state.read().await.todos.clone();
                Response::Todos { todos: without_hidden(todos, &hidden_collections(), |t| &t.collection) }
            }
            Request::SaveEvent { event } => match self.save_event(event).await {
                Ok(event) => Response::Event { event },
                Err(err) => self.failed(err).await,
            },
            Request::DeleteEvent { event } => match self.delete_event(event).await {
                Ok(()) => Response::Ok,
                Err(err) => self.failed(err).await,
            },
            Request::SaveTodo { todo } => match self.save_todo(todo).await {
                Ok(todo) => Response::Todo { todo },
                Err(err) => self.failed(err).await,
            },
            Request::DeleteTodo { todo } => match self.delete_todo(todo).await {
                Ok(()) => Response::Ok,
                Err(err) => self.failed(err).await,
            },
            _ => error("error", "not a calendar or task request".into()),
        }
    }

    /// The cached collections that `fit` (calendars or task lists) and are not hidden.
    async fn shown_collections(&self, fit: impl Fn(&Collection) -> bool) -> Response {
        let collections = self.state.read().await.collections.clone();
        Response::Collections { collections: shown(collections, &hidden_collections(), fit) }
    }

    /// `id` is a collection of the connected account, hidden or not.
    pub async fn knows_collection(&self, id: &str) -> bool {
        self.state.read().await.collections.iter().any(|c| c.id == id)
    }

    /// All accounts with state and all collections, for the control centre (task S.2).
    pub async fn accounts(&self) -> Vec<torro_ipc::AccountInfo> {
        account_infos(&*self.state.read().await)
    }

    /// Removes the account `id`: revokes its app password on the server (best effort),
    /// forgets account and secret, empties the cache (task S.2).
    pub async fn remove_account(&self, id: &str) -> Result<(), String> {
        let Some(connected) = self.state.read().await.account.clone().filter(|c| c.account.id == id) else {
            return Err(format!("no account {id:?}"));
        };
        // Revoke first: afterwards the secret is gone and the device would stay valid in
        // the user's "Devices & sessions" for good. Offline or refused only costs a log line.
        match &self.http {
            Some(http) => {
                if let Err(err) = account::revoke(http, &connected).await {
                    eprintln!("torroworkplace: app password not revoked on the server: {err}");
                }
            }
            None => eprintln!("torroworkplace: app password not revoked on the server: no HTTP client"),
        }
        self.forget_locally(|| account::keychain(account::forget)).await
    }

    /// Forgets the account locally: account list and secret (`forget_store`, in production
    /// [`account::forget`] on the keychain thread) first, then the cache. Shared by removing the
    /// account and by the remote wipe.
    async fn forget_locally<Forget, Fut>(&self, forget_store: Forget) -> Result<(), String>
    where
        Forget: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        forget_store().await?;
        let mut state = self.state.write().await;
        // A sign-in that runs meanwhile goes on; everything of the removed account goes.
        *state = State { signing_in: state.signing_in.take(), ..State::default() };
        Ok(())
    }

    /// The response for a failed request; a refused app password first runs the remote wipe
    /// check.
    async fn failed(&self, err: ProviderError) -> Response {
        if err == ProviderError::Auth {
            self.on_refused().await;
        }
        provider_error(&err)
    }

    /// The server refused the app password: asks once whether this device is to be wiped and
    /// does so (task S.12). Otherwise the account stays and shows "sign-in refused" until the
    /// user signs in again.
    async fn on_refused(&self) {
        let Some(http) = self.http.clone() else {
            return;
        };
        let Some(connected) = self.state.read().await.account.clone() else {
            return;
        };
        let (server, token) = (connected.account.server.as_str(), connected.app_password.as_str());
        let wiped = self
            .wipe_if_requested(
                &connected,
                || nextcloud::check_wipe(&http, server, token),
                || account::keychain(account::forget),
                || nextcloud::confirm_wipe(&http, server, token),
            )
            .await;
        match wiped {
            Ok(Some(true)) => {
                eprintln!("torroworkplace: the server asked to remove the account from this device; done");
                notify::show(
                    tr("Account removed").into(),
                    tr("Your Nextcloud account was removed from this device at the server's request.").into(),
                );
            }
            Ok(_) => {}
            Err(err) => eprintln!("torroworkplace: {err}"),
        }
    }

    /// The remote wipe for `connected` with the network and keychain steps passed in, so it can
    /// be tested without either. `Ok(None)` = skipped, because the check already ran for this
    /// refusal or the account changed meanwhile; otherwise the result of
    /// [`account::remote_wipe`]. A failed check may run again on the next refusal.
    async fn wipe_if_requested<Check, CheckFut, Forget, ForgetFut, Report, ReportFut>(
        &self,
        connected: &Connected,
        check: Check,
        forget_store: Forget,
        report: Report,
    ) -> Result<Option<bool>, String>
    where
        Check: FnOnce() -> CheckFut,
        CheckFut: Future<Output = Result<bool, ProviderError>>,
        Forget: FnOnce() -> ForgetFut,
        ForgetFut: Future<Output = Result<(), String>>,
        Report: FnOnce() -> ReportFut,
        ReportFut: Future<Output = Result<(), ProviderError>>,
    {
        let id = connected.account.id.as_str();
        {
            // Test and set under one lock, so concurrent failures ask only once.
            let mut state = self.state.write().await;
            if state.wipe_checked || !is_account(&state, id) {
                return Ok(None);
            }
            state.wipe_checked = true;
        }
        let wipe = || async {
            // The user may have removed the account or signed in anew while we asked.
            if !is_account(&*self.state.read().await, id) {
                return Err("the account changed meanwhile".to_string());
            }
            self.forget_locally(forget_store).await
        };
        let result = account::remote_wipe(check, wipe, report).await;
        if result.is_err() {
            let mut state = self.state.write().await;
            if is_account(&state, id) {
                state.wipe_checked = false;
            }
        }
        result.map(Some)
    }

    pub async fn account_state(&self) -> AccountState {
        state_of(&*self.state.read().await)
    }

    /// Starts Login Flow v2 and finishes it in the background.
    async fn connect(self: &Arc<Self>, server: String) -> Response {
        let Some(http) = self.http.clone() else {
            return error("error", "no HTTP client".into());
        };
        let flow = match account::start(&http, &server).await {
            Ok(flow) => flow,
            Err(err) => return error("error", err),
        };
        let login_url = flow.login_url.clone();
        self.state.write().await.signing_in = Some((server, login_url.clone()));
        let service = Arc::clone(self);
        tokio::spawn(async move {
            let result = account::finish(&http, &flow).await;
            {
                let mut state = service.state.write().await;
                state.signing_in = None;
                match result {
                    Ok(connected) => {
                        state.client = account::caldav(&http, &connected).ok();
                        state.account = Some(connected);
                        state.problem = None;
                        state.wipe_checked = false;
                    }
                    Err(err) => {
                        // Back to the connect screen; the apps show the state.
                        eprintln!("torroworkplace: sign-in failed: {err}");
                        return;
                    }
                }
            }
            if let Err(err) = service.sync().await {
                eprintln!("torroworkplace: first sync failed: {err}");
            }
        });
        Response::LoginStarted { login_url }
    }

    /// Fetches collections, todos and the event window from the server into the cache.
    /// On failure the cache stays as it was and the problem shows in the account state.
    pub async fn sync(&self) -> Result<(), ProviderError> {
        let _running = self.syncing.lock().await;
        let client = self.client().await?;
        let result = fetch_all(&client).await;
        let mut state = self.state.write().await;
        if state.client.is_none() {
            // The account was removed while the sync ran; keep the cache empty.
            return Err(not_connected());
        }
        match result {
            Ok((collections, events, window, todos)) => {
                state.collections = collections;
                state.events = events;
                state.window = Some(window);
                state.todos = todos;
                state.last_sync = Some(Utc::now());
                state.problem = None;
                state.wipe_checked = false;
                Ok(())
            }
            Err(err) => {
                state.problem = Some(err.to_string());
                drop(state);
                if err == ProviderError::Auth {
                    self.on_refused().await;
                }
                Err(err)
            }
        }
    }

    async fn events(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Event>, ProviderError> {
        {
            let state = self.state.read().await;
            if state.client.is_none() {
                return Ok(Vec::new());
            }
            if let Some(window) = state.window
                && covers(window, start, end)
            {
                return Ok(state.events.iter().filter(|e| overlaps(e, start, end)).cloned().collect());
            }
        }
        // Outside the cached window: ask the server.
        let client = self.client().await?;
        let collections = self.state.read().await.collections.clone();
        let mut events = Vec::new();
        for collection in collections.iter().filter(|c| c.kind.has_events()) {
            events.extend(client.events(&collection.id, start, end).await?);
        }
        Ok(events)
    }

    async fn save_event(&self, event: Event) -> Result<Event, ProviderError> {
        if event.recurring {
            return Err(recurring());
        }
        self.check_writable(&event.collection).await?;
        let client = self.client().await?;
        let saved = match moved(event.href.as_deref(), &event.collection) {
            // Moving to another calendar: create there, then remove the old one.
            Some(old_href) => {
                let old = Event { href: Some(old_href), ..event.clone() };
                let new = client.save_event(Event { href: None, etag: None, ..event }).await?;
                client.delete_event(&old).await?;
                self.state.write().await.events.retain(|e| e.href != old.href);
                new
            }
            None => client.save_event(event).await?,
        };
        let mut state = self.state.write().await;
        state.events.retain(|e| e.href != saved.href);
        state.events.push(saved.clone());
        Ok(saved)
    }

    async fn delete_event(&self, event: Event) -> Result<(), ProviderError> {
        if event.recurring {
            return Err(recurring());
        }
        self.check_writable(&event.collection).await?;
        self.client().await?.delete_event(&event).await?;
        self.state.write().await.events.retain(|e| e.href != event.href);
        Ok(())
    }

    async fn save_todo(&self, todo: Todo) -> Result<Todo, ProviderError> {
        self.check_writable(&todo.collection).await?;
        let client = self.client().await?;
        let saved = match moved(todo.href.as_deref(), &todo.collection) {
            Some(old_href) => {
                let old = Todo { href: Some(old_href), ..todo.clone() };
                let new = client.save_todo(Todo { href: None, etag: None, ..todo }).await?;
                client.delete_todo(&old).await?;
                self.state.write().await.todos.retain(|t| t.href != old.href);
                new
            }
            None => client.save_todo(todo).await?,
        };
        let mut state = self.state.write().await;
        state.todos.retain(|t| t.href != saved.href);
        state.todos.push(saved.clone());
        Ok(saved)
    }

    async fn delete_todo(&self, todo: Todo) -> Result<(), ProviderError> {
        self.check_writable(&todo.collection).await?;
        self.client().await?.delete_todo(&todo).await?;
        self.state.write().await.todos.retain(|t| t.href != todo.href);
        Ok(())
    }

    /// Refuses writes to collections the server marks read-only (contact birthdays, shared
    /// calendars without write access).
    async fn check_writable(&self, collection: &str) -> Result<(), ProviderError> {
        let state = self.state.read().await;
        match state.collections.iter().find(|c| c.id == collection) {
            Some(c) if !c.writable => Err(ProviderError::Other(format!("{} is read-only", c.name))),
            _ => Ok(()),
        }
    }

    async fn client(&self) -> Result<CalDavClient, ProviderError> {
        self.state.read().await.client.clone().ok_or_else(not_connected)
    }
}

/// The account state as the apps see it: a running sign-in wins over the stored account.
fn state_of(state: &State) -> AccountState {
    if let Some((server, login_url)) = &state.signing_in {
        return AccountState::SigningIn { server: server.clone(), login_url: login_url.clone() };
    }
    match &state.account {
        Some(connected) => AccountState::Connected {
            server: connected.account.server.clone(),
            login_name: connected.account.login_name.clone(),
            last_sync: state.last_sync,
            problem: state.problem.clone(),
        },
        None => AccountState::NotConnected,
    }
}

/// `id` is still the account in `state`.
fn is_account(state: &State, id: &str) -> bool {
    state.account.as_ref().is_some_and(|connected| connected.account.id == id)
}

/// One entry per stored account (today at most one Nextcloud) with all cached collections,
/// hidden ones included: the control centre shows a switch for each. While the first sign-in
/// runs there is no account yet; an entry with an empty id stands in for it so the
/// add-account dialog can follow the sign-in.
fn account_infos(state: &State) -> Vec<torro_ipc::AccountInfo> {
    if let Some(connected) = &state.account {
        let account = &connected.account;
        return vec![torro_ipc::AccountInfo {
            id: account.id.clone(),
            kind: "nextcloud".into(),
            label: format!("{}@{}", account.login_name, host(&account.server)),
            state: state_of(state),
            collections: state.collections.clone(),
        }];
    }
    match &state.signing_in {
        Some((server, _)) => vec![torro_ipc::AccountInfo {
            id: String::new(),
            kind: "nextcloud".into(),
            label: server.clone(),
            state: state_of(state),
            collections: Vec::new(),
        }],
        None => Vec::new(),
    }
}

/// `https://cloud.example.org/` → `cloud.example.org`; a sub-path stays, like in the
/// account id (`torro_core::Account::nextcloud`).
fn host(server: &str) -> &str {
    server.split_once("://").map_or(server, |(_, rest)| rest).trim_end_matches('/')
}

/// The collections the user hid in the control centre. Read on every request: the file is
/// tiny and the control centre writes it from another process.
fn hidden_collections() -> Vec<String> {
    torro_core::settings::load().hidden_collections
}

/// Leaves out the items whose collection (`collection_of`) is hidden.
fn without_hidden<T>(items: Vec<T>, hidden: &[String], collection_of: impl Fn(&T) -> &String) -> Vec<T> {
    if hidden.is_empty() {
        return items;
    }
    items.into_iter().filter(|item| !hidden.contains(collection_of(item))).collect()
}

/// The `collections` that `fit` and are not `hidden`.
fn shown(collections: Vec<Collection>, hidden: &[String], fit: impl Fn(&Collection) -> bool) -> Vec<Collection> {
    let fitting = collections.into_iter().filter(|c| fit(c)).collect();
    without_hidden(fitting, hidden, |c| &c.id)
}

/// Everything the cache holds, fetched from the server.
async fn fetch_all(
    client: &CalDavClient,
) -> Result<(Vec<Collection>, Vec<Event>, (DateTime<Utc>, DateTime<Utc>), Vec<Todo>), ProviderError> {
    let today = Local::now().date_naive();
    let start = today - Span::days(WINDOW_BACK_DAYS);
    let end = today + Span::days(WINDOW_AHEAD_DAYS);
    let window = (start.and_time(chrono::NaiveTime::MIN).and_utc(), end.and_time(chrono::NaiveTime::MIN).and_utc());
    let mut collections = client.collections().await?;
    for collection in &mut collections {
        if let Some(name) = placeholder_name(&collection.name) {
            collection.name = name.to_string();
        }
    }
    let mut events = Vec::new();
    let mut todos = Vec::new();
    for collection in &collections {
        if collection.kind.has_events() {
            events.extend(client.events(&collection.id, window.0, window.1).await?);
        }
        if collection.kind.has_todos() {
            todos.extend(client.todos(&collection.id).await?);
        }
    }
    Ok((collections, events, window, todos))
}

/// Nextcloud stores translation keys instead of names for the collections it creates on its
/// own and leaves translating them to the clients.
fn placeholder_name(name: &str) -> Option<&'static str> {
    match name {
        "DEFAULT_TASK_CALENDAR_NAME" => Some(tr("Tasks")),
        "DEFAULT_CALENDAR_NAME" => Some(tr("Personal")),
        _ => None,
    }
}

/// The old href when an item moves to another collection (its href is not inside the
/// collection any more).
fn moved(href: Option<&str>, collection: &str) -> Option<String> {
    href.filter(|href| !href.starts_with(collection)).map(str::to_string)
}

/// The cached window covers `[start, end)`, with a day of slack for all-day entries
/// whose dates are compared in local time.
fn covers(window: (DateTime<Utc>, DateTime<Utc>), start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    window.0 + Span::days(1) <= start && end + Span::days(1) <= window.1
}

/// The event overlaps `[start, end)`. All-day dates count from local midnight.
fn overlaps(event: &Event, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    let instant = |time: EventTime| match time {
        EventTime::DateTime(at) => at,
        EventTime::Date(date) => torro_local_midnight(date),
    };
    let from = instant(event.start);
    let to = event.end.map(instant).unwrap_or(from);
    from < end && (to > start || (to == from && from >= start))
}

fn torro_local_midnight(date: chrono::NaiveDate) -> DateTime<Utc> {
    date.and_time(chrono::NaiveTime::MIN)
        .and_local_timezone(Local)
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| date.and_time(chrono::NaiveTime::MIN).and_utc())
}

fn not_connected() -> ProviderError {
    ProviderError::Other("not connected to a Nextcloud".into())
}

fn recurring() -> ProviderError {
    ProviderError::Other("recurring events cannot be edited yet".into())
}

fn error(code: &str, message: String) -> Response {
    Response::Error { code: code.into(), message }
}

/// A provider failure with the code `torro_ipc::pim::PimClient` maps back.
fn provider_error(err: &ProviderError) -> Response {
    let code = match err {
        ProviderError::Auth => "auth",
        ProviderError::Offline => "offline",
        ProviderError::Conflict { .. } => "conflict",
        ProviderError::Other(message) if message.starts_with("not connected") => "not-connected",
        ProviderError::Other(_) => "error",
    };
    error(code, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> Arc<PimService> {
        Arc::new(PimService { http: None, state: RwLock::new(State::default()), syncing: Mutex::new(()) })
    }

    fn event(start: DateTime<Utc>, hours: i64) -> Event {
        Event {
            uid: "u".into(),
            collection: "https://cloud/dav/calendars/a/work/".into(),
            href: Some("https://cloud/dav/calendars/a/work/u.ics".into()),
            etag: Some("\"1\"".into()),
            summary: "Workshop".into(),
            start: EventTime::DateTime(start),
            end: Some(EventTime::DateTime(start + Span::hours(hours))),
            location: None,
            description: None,
            recurring: false,
        }
    }

    #[tokio::test]
    async fn without_account_reads_are_empty_and_writes_fail() {
        let service = service();
        assert_eq!(service.account_state().await, AccountState::NotConnected);
        assert_eq!(service.handle(Request::Todos).await, Response::Todos { todos: vec![] });
        let now = Utc::now();
        assert_eq!(
            service.handle(Request::Events { start: now, end: now + Span::days(1) }).await,
            Response::Events { events: vec![] }
        );
        let response = service.handle(Request::SaveEvent { event: event(now, 1) }).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "not-connected"), "{response:?}");
    }

    #[tokio::test]
    async fn recurring_occurrences_are_refused() {
        let service = service();
        let response =
            service.handle(Request::DeleteEvent { event: Event { recurring: true, ..event(Utc::now(), 1) } }).await;
        assert!(
            matches!(response, Response::Error { ref message, .. } if message.contains("recurring")),
            "{response:?}"
        );
    }

    #[test]
    fn window_and_overlap() {
        let now = Utc::now();
        let window = (now - Span::days(90), now + Span::days(365));
        assert!(covers(window, now, now + Span::days(7)));
        assert!(!covers(window, now - Span::days(100), now));
        let e = event(now, 2);
        assert!(overlaps(&e, now - Span::hours(1), now + Span::hours(1)));
        assert!(!overlaps(&e, now + Span::hours(2), now + Span::hours(3)));
    }

    #[test]
    fn nextcloud_placeholder_names_are_translated() {
        assert_eq!(placeholder_name("DEFAULT_TASK_CALENDAR_NAME"), Some(tr("Tasks")));
        assert_eq!(placeholder_name("Persönlich"), None);
    }

    fn collection(id: &str) -> Collection {
        Collection {
            id: id.into(),
            name: id.into(),
            kind: torro_pim::CollectionKind::Events,
            color: None,
            writable: true,
        }
    }

    fn connected() -> Connected {
        Connected {
            account: torro_core::Account::nextcloud("https://cloud.example.org/", "anna"),
            app_password: "secret".into(),
        }
    }

    #[test]
    fn no_account_and_no_sign_in_lists_nothing() {
        assert_eq!(account_infos(&State::default()), vec![]);
    }

    #[test]
    fn a_running_sign_in_is_listed_without_id() {
        let state = State {
            signing_in: Some(("cloud.example.org".into(), "https://cloud.example.org/login/v2/flow/x".into())),
            ..State::default()
        };
        let infos = account_infos(&state);
        assert_eq!(
            infos,
            vec![torro_ipc::AccountInfo {
                id: String::new(),
                kind: "nextcloud".into(),
                label: "cloud.example.org".into(),
                state: AccountState::SigningIn {
                    server: "cloud.example.org".into(),
                    login_url: "https://cloud.example.org/login/v2/flow/x".into(),
                },
                collections: vec![],
            }]
        );
    }

    #[test]
    fn a_connected_account_lists_all_collections() {
        let now = Utc::now();
        let state = State {
            account: Some(connected()),
            last_sync: Some(now),
            problem: Some("server not reachable".into()),
            collections: vec![collection("work"), collection("home")],
            ..State::default()
        };
        let infos = account_infos(&state);
        assert_eq!(
            infos,
            vec![torro_ipc::AccountInfo {
                id: "nextcloud:anna@cloud.example.org".into(),
                kind: "nextcloud".into(),
                label: "anna@cloud.example.org".into(),
                state: AccountState::Connected {
                    server: "https://cloud.example.org".into(),
                    login_name: "anna".into(),
                    last_sync: Some(now),
                    problem: Some("server not reachable".into()),
                },
                collections: vec![collection("work"), collection("home")],
            }]
        );
    }

    #[test]
    fn hidden_collections_and_their_items_are_left_out() {
        let hidden = vec!["home".to_string()];
        let collections = without_hidden(vec![collection("work"), collection("home")], &hidden, |c| &c.id);
        assert_eq!(collections, vec![collection("work")]);
        let mut private = event(Utc::now(), 1);
        private.collection = "home".into();
        let mut work = event(Utc::now(), 1);
        work.collection = "work".into();
        let events = without_hidden(vec![private, work.clone()], &hidden, |e| &e.collection);
        assert_eq!(events, vec![work]);
        let all = without_hidden(vec![collection("home")], &[], |c| &c.id);
        assert_eq!(all, vec![collection("home")]);
    }

    fn of_kind(id: &str, kind: torro_pim::CollectionKind) -> Collection {
        Collection { kind, ..collection(id) }
    }

    #[test]
    fn calendars_and_task_lists_are_told_apart() {
        use torro_pim::CollectionKind::{Both, Events, Todos};
        let all = vec![of_kind("work", Both), of_kind("birthdays", Events), of_kind("home", Todos)];
        let calendars = shown(all.clone(), &[], |c| c.kind.has_events());
        assert_eq!(calendars, vec![of_kind("work", Both), of_kind("birthdays", Events)]);
        let lists = shown(all.clone(), &[], |c| c.kind.has_todos());
        assert_eq!(lists, vec![of_kind("work", Both), of_kind("home", Todos)]);
        let hidden = vec!["work".to_string()];
        assert_eq!(shown(all, &hidden, |c| c.kind.has_todos()), vec![of_kind("home", Todos)]);
    }

    #[tokio::test]
    async fn only_cached_collections_are_known() {
        let service = service();
        assert!(!service.knows_collection("work").await);
        service.state.write().await.collections = vec![collection("work")];
        assert!(service.knows_collection("work").await);
        assert!(!service.knows_collection("home").await);
    }

    #[test]
    fn host_strips_scheme_and_slash() {
        assert_eq!(host("https://cloud.example.org/"), "cloud.example.org");
        assert_eq!(host("https://example.org/nextcloud"), "example.org/nextcloud");
        assert_eq!(host("cloud.example.org"), "cloud.example.org");
    }

    #[tokio::test]
    async fn removing_an_unknown_account_fails_and_keeps_the_state() {
        let service = service();
        assert!(service.remove_account("nextcloud:anna@cloud.example.org").await.is_err());
        service.state.write().await.account = Some(connected());
        assert!(service.remove_account("nextcloud:bob@cloud.example.org").await.is_err());
        assert!(service.remove_account("").await.is_err());
        assert_eq!(service.accounts().await.len(), 1);
    }

    /// A service whose server just refused the app password, with a filled cache.
    async fn refused_service() -> Arc<PimService> {
        let service = service();
        {
            let mut state = service.state.write().await;
            state.account = Some(connected());
            state.collections = vec![collection("work")];
            state.problem = Some(ProviderError::Auth.to_string());
        }
        service
    }

    /// Runs the remote wipe with a scripted check answer and returns its result plus the steps
    /// taken; keychain and network are stand-ins.
    async fn wipe(
        service: &PimService,
        check: Result<bool, ProviderError>,
    ) -> (Result<Option<bool>, String>, Vec<&'static str>) {
        let steps = std::cell::RefCell::new(Vec::new());
        let result = service
            .wipe_if_requested(
                &connected(),
                || {
                    steps.borrow_mut().push("check");
                    async { check }
                },
                || {
                    steps.borrow_mut().push("forget");
                    async { Ok(()) }
                },
                || {
                    steps.borrow_mut().push("report");
                    async { Ok(()) }
                },
            )
            .await;
        (result, steps.into_inner())
    }

    #[tokio::test]
    async fn a_remote_wipe_removes_account_secret_and_cache_and_reports_it() {
        let service = refused_service().await;
        let (result, steps) = wipe(&service, Ok(true)).await;
        assert_eq!(result, Ok(Some(true)));
        assert_eq!(steps, ["check", "forget", "report"]);
        assert_eq!(service.account_state().await, AccountState::NotConnected);
        assert!(service.accounts().await.is_empty());
        assert!(service.state.read().await.collections.is_empty());
    }

    #[tokio::test]
    async fn without_a_remote_wipe_the_account_stays_and_is_checked_only_once() {
        let service = refused_service().await;
        let (result, steps) = wipe(&service, Ok(false)).await;
        assert_eq!(result, Ok(Some(false)));
        assert_eq!(steps, ["check"]);
        assert_eq!(service.accounts().await.len(), 1);
        assert_eq!(service.state.read().await.collections, vec![collection("work")]);
        // The next refused request does not ask the server again.
        let (result, steps) = wipe(&service, Ok(true)).await;
        assert_eq!(result, Ok(None));
        assert!(steps.is_empty());
        assert_eq!(service.accounts().await.len(), 1);
    }

    #[tokio::test]
    async fn a_failed_wipe_check_keeps_the_account_and_may_be_retried() {
        let service = refused_service().await;
        let (result, steps) = wipe(&service, Err(ProviderError::Offline)).await;
        assert!(result.is_err());
        assert_eq!(steps, ["check"]);
        assert_eq!(service.accounts().await.len(), 1);
        assert_eq!(service.state.read().await.collections, vec![collection("work")]);
        let (_, steps) = wipe(&service, Ok(false)).await;
        assert_eq!(steps, ["check"], "a failed check runs again on the next refusal");
    }

    #[tokio::test]
    async fn no_wipe_check_for_an_account_that_is_gone() {
        let service = service();
        let (result, steps) = wipe(&service, Ok(true)).await;
        assert_eq!(result, Ok(None));
        assert!(steps.is_empty());
    }

    #[test]
    fn moving_is_detected_by_href() {
        let collection = "https://cloud/dav/calendars/a/work/";
        assert_eq!(moved(Some("https://cloud/dav/calendars/a/work/u.ics"), collection), None);
        assert_eq!(
            moved(Some("https://cloud/dav/calendars/a/home/u.ics"), collection),
            Some("https://cloud/dav/calendars/a/home/u.ics".into())
        );
        assert_eq!(moved(None, collection), None);
    }
}
