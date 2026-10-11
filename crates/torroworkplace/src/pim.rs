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
//! - An SQLite cache that survives restarts comes later.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Duration as Span, Local, Utc};
use tokio::sync::{Mutex, RwLock};
use torro_ipc::{Request, Response};
use torro_pim::{AccountState, Collection, Event, EventTime, ProviderError, Todo};
use torro_pim_caldav::CalDavClient;

use torro_core::i18n::tr;

use crate::account::{self, Connected};

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
            Request::Account => Response::Account { account: self.account_state().await },
            Request::ConnectNextcloud { server } => self.connect(server).await,
            Request::DisconnectAccount => match account::keychain(account::forget).await {
                Ok(()) => {
                    *self.state.write().await = State::default();
                    Response::Ok
                }
                Err(err) => error("error", err),
            },
            Request::Sync => match self.sync().await {
                Ok(()) => Response::Ok,
                Err(err) => provider_error(&err),
            },
            Request::Collections => {
                let collections = self.state.read().await.collections.clone();
                Response::Collections { collections: without_hidden(collections, &hidden_collections(), |c| &c.id) }
            }
            Request::Events { start, end } => match self.events(start, end).await {
                Ok(events) => {
                    Response::Events { events: without_hidden(events, &hidden_collections(), |e| &e.collection) }
                }
                Err(err) => provider_error(&err),
            },
            Request::Todos => {
                let todos = self.state.read().await.todos.clone();
                Response::Todos { todos: without_hidden(todos, &hidden_collections(), |t| &t.collection) }
            }
            Request::SaveEvent { event } => match self.save_event(event).await {
                Ok(event) => Response::Event { event },
                Err(err) => provider_error(&err),
            },
            Request::DeleteEvent { event } => match self.delete_event(event).await {
                Ok(()) => Response::Ok,
                Err(err) => provider_error(&err),
            },
            Request::SaveTodo { todo } => match self.save_todo(todo).await {
                Ok(todo) => Response::Todo { todo },
                Err(err) => provider_error(&err),
            },
            Request::DeleteTodo { todo } => match self.delete_todo(todo).await {
                Ok(()) => Response::Ok,
                Err(err) => provider_error(&err),
            },
            _ => error("error", "not a calendar or task request".into()),
        }
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
        account::keychain(account::forget).await?;
        let mut state = self.state.write().await;
        // A sign-in that runs meanwhile goes on; everything of the removed account goes.
        *state = State { signing_in: state.signing_in.take(), ..State::default() };
        Ok(())
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
                Ok(())
            }
            Err(err) => {
                state.problem = Some(err.to_string());
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
        assert_eq!(service.handle(Request::Account).await, Response::Account { account: AccountState::NotConnected });
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
