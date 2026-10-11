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
        let mut state = State::default();
        match account::stored() {
            Ok(Some(connected)) => {
                state.client = http.as_ref().and_then(|http| account::caldav(http, &connected).ok());
                state.account = Some(connected);
            }
            Ok(None) => {}
            Err(err) => eprintln!("torroworkplace: cannot read the account: {err}"),
        }
        PimService { http, state: RwLock::new(state), syncing: Mutex::new(()) }
    }

    /// Syncs now and then every few minutes, for as long as the daemon runs.
    pub fn start(self: &Arc<Self>) {
        let service = Arc::clone(self);
        tokio::spawn(async move {
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
            Request::DisconnectAccount => match account::forget() {
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
            Request::Collections => Response::Collections { collections: self.state.read().await.collections.clone() },
            Request::Events { start, end } => match self.events(start, end).await {
                Ok(events) => Response::Events { events },
                Err(err) => provider_error(&err),
            },
            Request::Todos => Response::Todos { todos: self.state.read().await.todos.clone() },
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

    async fn account_state(&self) -> AccountState {
        let state = self.state.read().await;
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
        match fetch_all(&client).await {
            Ok((collections, events, window, todos)) => {
                let mut state = self.state.write().await;
                state.collections = collections;
                state.events = events;
                state.window = Some(window);
                state.todos = todos;
                state.last_sync = Some(Utc::now());
                state.problem = None;
                Ok(())
            }
            Err(err) => {
                self.state.write().await.problem = Some(err.to_string());
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
        self.client().await?.delete_event(&event).await?;
        self.state.write().await.events.retain(|e| e.href != event.href);
        Ok(())
    }

    async fn save_todo(&self, todo: Todo) -> Result<Todo, ProviderError> {
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
        self.client().await?.delete_todo(&todo).await?;
        self.state.write().await.todos.retain(|t| t.href != todo.href);
        Ok(())
    }

    async fn client(&self) -> Result<CalDavClient, ProviderError> {
        self.state.read().await.client.clone().ok_or_else(not_connected)
    }
}

/// Everything the cache holds, fetched from the server.
async fn fetch_all(
    client: &CalDavClient,
) -> Result<(Vec<Collection>, Vec<Event>, (DateTime<Utc>, DateTime<Utc>), Vec<Todo>), ProviderError> {
    let today = Local::now().date_naive();
    let start = today - Span::days(WINDOW_BACK_DAYS);
    let end = today + Span::days(WINDOW_AHEAD_DAYS);
    let window = (start.and_time(chrono::NaiveTime::MIN).and_utc(), end.and_time(chrono::NaiveTime::MIN).and_utc());
    let collections = client.collections().await?;
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
