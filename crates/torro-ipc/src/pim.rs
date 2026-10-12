//! [`PimSource`] over the socket: how the apps reach calendars and tasks in the core.
//!
//! Task K.7 in `docs/tasks/kalender-aufgaben.md`. Each method sends one [`Request`] with
//! [`crate::client::call`] and maps the [`Response`]: the expected variant → `Ok`,
//! `Response::Error { code, message }` → [`ProviderError`] (`auth` → `Auth`, `offline` →
//! `Offline`, `conflict` → `Conflict`, everything else → `Other(message)`), a core that is
//! not running → `ProviderError::Other` with the `IpcError` message, an unexpected
//! variant → `Other("unexpected answer …")`.
//!
//! [`PimSource::accounts`] reads `Accounts` and leaves out the collections, which come with
//! [`PimSource::collections`]; that one joins `CalendarCollections` and `TaskCollections`, so
//! the apps keep one call each.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use torro_pim::{Collection, Event, PimAccount, PimSource, ProviderError, Todo};

use crate::Request;
use crate::Response;

/// Talks to the core at its socket.
#[derive(Debug, Clone)]
pub struct PimClient {
    pub socket: PathBuf,
}

impl Default for PimClient {
    /// The core of this user session ([`crate::socket_path`]).
    fn default() -> PimClient {
        PimClient { socket: crate::socket_path().unwrap_or_else(|| PathBuf::from("core.sock")) }
    }
}

impl PimClient {
    /// Sends `request` and returns the answer, or the error the core reported as a
    /// [`ProviderError`]. Callers match the variant they expect.
    async fn ask(&self, request: Request) -> Result<Response, ProviderError> {
        match crate::client::call(&self.socket, &request).await {
            Ok(Response::Error { code, message }) => Err(match code.as_str() {
                "auth" => ProviderError::Auth,
                "offline" => ProviderError::Offline,
                // The wire answer names no item; the write methods fill in the uid they sent.
                "conflict" => ProviderError::Conflict { uid: String::new() },
                _ => ProviderError::Other(message),
            }),
            Ok(response) => Ok(response),
            Err(err) => Err(ProviderError::Other(err.to_string())),
        }
    }

    /// Sends a request answered with [`Response::Collections`].
    async fn ask_collections(&self, request: Request) -> Result<Vec<Collection>, ProviderError> {
        match self.ask(request).await? {
            Response::Collections { collections } => Ok(collections),
            other => Err(unexpected(other)),
        }
    }
}

/// An answer that is not the variant the request asks for.
fn unexpected(response: Response) -> ProviderError {
    ProviderError::Other(format!("unexpected answer: {response:?}"))
}

fn expect_ok(response: Response) -> Result<(), ProviderError> {
    match response {
        Response::Ok => Ok(()),
        other => Err(unexpected(other)),
    }
}

/// A conflict names the item that was sent; the wire answer does not.
fn with_uid(err: ProviderError, uid: &str) -> ProviderError {
    match err {
        ProviderError::Conflict { .. } => ProviderError::Conflict { uid: uid.to_string() },
        other => other,
    }
}

impl PimSource for PimClient {
    /// Every account the core lists, in its order; a sign-in that runs is listed too (empty id).
    async fn accounts(&self) -> Result<Vec<PimAccount>, ProviderError> {
        match self.ask(Request::Accounts).await? {
            Response::Accounts { accounts } => Ok(accounts
                .into_iter()
                .map(|account| PimAccount {
                    id: account.id,
                    label: account.label,
                    kind: account.kind,
                    state: account.state,
                })
                .collect()),
            other => Err(unexpected(other)),
        }
    }

    async fn connect_nextcloud(&self, server: String) -> Result<String, ProviderError> {
        match self.ask(Request::ConnectNextcloud { server }).await? {
            Response::LoginStarted { login_url } => Ok(login_url),
            other => Err(unexpected(other)),
        }
    }

    async fn sync(&self) -> Result<(), ProviderError> {
        expect_ok(self.ask(Request::Sync).await?)
    }

    /// The calendars, then the task lists that are not calendars as well: a collection with
    /// events and todos is in both answers but listed once.
    async fn collections(&self) -> Result<Vec<Collection>, ProviderError> {
        let (calendars, lists) = tokio::join!(
            self.ask_collections(Request::CalendarCollections),
            self.ask_collections(Request::TaskCollections)
        );
        let mut collections = calendars?;
        for list in lists? {
            if !collections.iter().any(|c| c.id == list.id) {
                collections.push(list);
            }
        }
        Ok(collections)
    }

    async fn events(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Event>, ProviderError> {
        match self.ask(Request::Events { start, end }).await? {
            Response::Events { events } => Ok(events),
            other => Err(unexpected(other)),
        }
    }

    async fn todos(&self) -> Result<Vec<Todo>, ProviderError> {
        match self.ask(Request::Todos).await? {
            Response::Todos { todos } => Ok(todos),
            other => Err(unexpected(other)),
        }
    }

    async fn save_event(&self, event: Event) -> Result<Event, ProviderError> {
        let uid = event.uid.clone();
        match self.ask(Request::SaveEvent { event }).await.map_err(|err| with_uid(err, &uid))? {
            Response::Event { event } => Ok(event),
            other => Err(unexpected(other)),
        }
    }

    async fn delete_event(&self, event: Event) -> Result<(), ProviderError> {
        let uid = event.uid.clone();
        expect_ok(self.ask(Request::DeleteEvent { event }).await.map_err(|err| with_uid(err, &uid))?)
    }

    async fn save_todo(&self, todo: Todo) -> Result<Todo, ProviderError> {
        let uid = todo.uid.clone();
        match self.ask(Request::SaveTodo { todo }).await.map_err(|err| with_uid(err, &uid))? {
            Response::Todo { todo } => Ok(todo),
            other => Err(unexpected(other)),
        }
    }

    async fn delete_todo(&self, todo: Todo) -> Result<(), ProviderError> {
        let uid = todo.uid.clone();
        expect_ok(self.ask(Request::DeleteTodo { todo }).await.map_err(|err| with_uid(err, &uid))?)
    }

    /// `Request::MoveTodo` with the `href` and `etag` of `todo` and the `href` of `before`. A
    /// task that was never saved has no place in a list yet: `Other` without asking the core.
    async fn move_todo(&self, todo: Todo, before: Option<Todo>) -> Result<Vec<Todo>, ProviderError> {
        let Some(href) = todo.href else {
            return Err(ProviderError::Other("the task is not saved yet".into()));
        };
        let request =
            Request::MoveTodo { href, etag: todo.etag.unwrap_or_default(), before: before.and_then(|b| b.href) };
        match self.ask(request).await.map_err(|err| with_uid(err, &todo.uid))? {
            Response::Todos { todos } => Ok(todos),
            other => Err(unexpected(other)),
        }
    }

    /// `Request::UnpinIssue` with the list's id and the item's link id as `url`.
    async fn unpin_issue(&self, collection: String, item: String) -> Result<(), ProviderError> {
        expect_ok(self.ask(Request::UnpinIssue { collection, url: item }).await?)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::future::Future;
    use std::path::PathBuf;
    use std::time::Duration;

    use chrono::{NaiveDate, TimeZone};
    use torro_pim::{AccountState, CollectionKind, EventTime};

    use super::*;
    use crate::{AccountInfo, IpcError, client, server};

    fn utc(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
    }

    fn account() -> AccountState {
        AccountState::Connected {
            server: "https://cloud.example.test".into(),
            login_name: "anna".into(),
            last_sync: None,
            problem: None,
        }
    }

    fn collection() -> Collection {
        Collection {
            id: "/cal/work/".into(),
            name: "Work".into(),
            kind: CollectionKind::Both,
            color: Some("#3366ff".into()),
            writable: true,
            source: None,
        }
    }

    fn calendar() -> Collection {
        Collection {
            id: "/cal/birthdays/".into(),
            name: "Birthdays".into(),
            kind: CollectionKind::Events,
            ..collection()
        }
    }

    fn list() -> Collection {
        Collection { id: "/tasks/home/".into(), name: "Home".into(), kind: CollectionKind::Todos, ..collection() }
    }

    fn account_info() -> AccountInfo {
        AccountInfo {
            id: "nextcloud:anna@cloud.example.test".into(),
            kind: "nextcloud".into(),
            label: "anna@cloud.example.test".into(),
            state: account(),
            collections: vec![collection(), calendar(), list()],
            scopes: None,
        }
    }

    fn event() -> Event {
        Event {
            uid: "meeting-1".into(),
            collection: "/cal/work/".into(),
            href: None,
            etag: None,
            summary: "Standup".into(),
            start: EventTime::DateTime(utc(12, 9)),
            end: Some(EventTime::DateTime(utc(12, 10))),
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
        }
    }

    fn todo() -> Todo {
        Todo {
            uid: "todo-1".into(),
            collection: "/tasks/home/".into(),
            href: None,
            etag: None,
            summary: "Buy milk".into(),
            due: Some(EventTime::Date(NaiveDate::from_ymd_opt(2026, 10, 12).expect("valid date"))),
            priority: Some(3),
            completed: false,
            parent: None,
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
        }
    }

    /// What a core that stores the item answers to a write: the item with an href and an etag.
    fn stored_event(event: Event) -> Event {
        Event { href: Some(format!("/cal/work/{}.ics", event.uid)), etag: Some("\"2\"".into()), ..event }
    }

    fn stored_todo(todo: Todo) -> Todo {
        Todo { href: Some(format!("/tasks/home/{}.ics", todo.uid)), etag: Some("\"2\"".into()), ..todo }
    }

    /// Canned answers for every request the client sends.
    fn answer(request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "test".into(), api: crate::API_VERSION },
            Request::Accounts => Response::Accounts { accounts: vec![account_info()] },
            Request::ConnectNextcloud { server } => {
                Response::LoginStarted { login_url: format!("{server}/login/v2/flow") }
            }
            Request::Sync | Request::DeleteEvent { .. } | Request::DeleteTodo { .. } => Response::Ok,
            // Only the pinned item of the test list can be unpinned.
            Request::UnpinIssue { collection, url }
                if collection == "github:mahype/bugs" && url == "github:mahype/TorroWorkplace#7" =>
            {
                Response::Ok
            }
            Request::UnpinIssue { .. } => {
                Response::Error { code: "not-found".into(), message: "the list does not pin it".into() }
            }
            // The collection with both kinds is in both answers.
            Request::CalendarCollections => Response::Collections { collections: vec![collection(), calendar()] },
            Request::TaskCollections => Response::Collections { collections: vec![list(), collection()] },
            // The summary echoes the requested range, so the test sees the arguments arrive.
            Request::Events { start, end } => {
                Response::Events { events: vec![Event { summary: format!("{start} until {end}"), ..event() }] }
            }
            Request::Todos => Response::Todos { todos: vec![todo()] },
            Request::SaveEvent { event } => Response::Event { event: stored_event(event) },
            Request::SaveTodo { todo } => Response::Todo { todo: stored_todo(todo) },
            // The answer names what arrived: the moved task's href as uid, `before` as summary.
            Request::MoveTodo { href, etag, before } => Response::Todos {
                todos: vec![Todo {
                    uid: href,
                    etag: Some(etag),
                    summary: before.unwrap_or_default(),
                    sort_order: Some(0),
                    ..todo()
                }],
            },
            other => Response::Error { code: "error".into(), message: format!("test core does not know {other:?}") },
        }
    }

    /// A core listening on a socket in a temporary directory. Dropping it stops the server.
    struct Core {
        _dir: tempfile::TempDir,
        socket: PathBuf,
        server: tokio::task::JoinHandle<Result<(), IpcError>>,
    }

    impl Core {
        fn client(&self) -> PimClient {
            PimClient { socket: self.socket.clone() }
        }
    }

    impl Drop for Core {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    /// Starts a core that answers with `handler`, and waits until it accepts connections.
    async fn start<H, F>(handler: H) -> Core
    where
        H: Fn(Request) -> F + Send + Sync + 'static,
        F: Future<Output = Response> + Send + 'static,
    {
        let dir = tempfile::tempdir().expect("temporary directory");
        let socket = dir.path().join("core.sock");
        let server = tokio::spawn({
            let socket = socket.clone();
            async move { server::serve(&socket, handler).await }
        });
        for _ in 0..200 {
            // Any answer counts: the handler under test may not know about Ping.
            if client::call(&socket, &Request::Ping).await.is_ok() {
                return Core { _dir: dir, socket, server };
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("core did not start listening on {}", socket.display());
    }

    /// Starts a core that answers every request with the given error and returns what the
    /// client makes of it.
    async fn error_from(code: &'static str, message: &'static str) -> ProviderError {
        let core = start(move |_| async move { Response::Error { code: code.into(), message: message.into() } }).await;
        core.client().collections().await.expect_err("the core answers with an error")
    }

    #[tokio::test]
    async fn accounts_are_read_without_their_collections() {
        let core = start(|request| async move {
            match request {
                Request::Accounts => {
                    let second = AccountInfo {
                        id: "nextcloud:bob@other.example.test".into(),
                        label: "bob@other.example.test".into(),
                        collections: vec![],
                        ..account_info()
                    };
                    Response::Accounts { accounts: vec![account_info(), second] }
                }
                other => answer(other),
            }
        })
        .await;
        let accounts = core.client().accounts().await.unwrap();
        assert_eq!(
            accounts,
            vec![
                PimAccount {
                    id: "nextcloud:anna@cloud.example.test".into(),
                    label: "anna@cloud.example.test".into(),
                    kind: "nextcloud".into(),
                    state: account(),
                },
                PimAccount {
                    id: "nextcloud:bob@other.example.test".into(),
                    label: "bob@other.example.test".into(),
                    kind: "nextcloud".into(),
                    state: account(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn connect_returns_login_url() {
        let core = start(|request| async move { answer(request) }).await;
        let url = core.client().connect_nextcloud("https://cloud.example.test".into()).await.unwrap();
        assert_eq!(url, "https://cloud.example.test/login/v2/flow");
    }

    #[tokio::test]
    async fn sync_is_ok() {
        let core = start(|request| async move { answer(request) }).await;
        core.client().sync().await.unwrap();
    }

    #[tokio::test]
    async fn without_accounts_the_list_is_empty_and_not_connected() {
        let core = start(|request| async move {
            match request {
                Request::Accounts => Response::Accounts { accounts: vec![] },
                other => answer(other),
            }
        })
        .await;
        let accounts = core.client().accounts().await.unwrap();
        assert!(accounts.is_empty());
        assert_eq!(torro_pim::combined_state(&accounts), AccountState::NotConnected);
    }

    #[tokio::test]
    async fn collections_join_calendars_and_lists_once_each() {
        let core = start(|request| async move { answer(request) }).await;
        assert_eq!(core.client().collections().await.unwrap(), vec![collection(), calendar(), list()]);
    }

    #[tokio::test]
    async fn collections_fail_when_one_half_fails() {
        let core = start(|request| async move {
            match request {
                Request::TaskCollections => Response::Error { code: "offline".into(), message: "down".into() },
                other => answer(other),
            }
        })
        .await;
        assert_eq!(core.client().collections().await, Err(ProviderError::Offline));
    }

    #[tokio::test]
    async fn events_send_the_range() {
        let core = start(|request| async move { answer(request) }).await;
        let (start, end) = (utc(12, 0), utc(13, 0));
        let events = core.client().events(start, end).await.unwrap();
        assert_eq!(events, vec![Event { summary: format!("{start} until {end}"), ..event() }]);
    }

    #[tokio::test]
    async fn todos_are_listed() {
        let core = start(|request| async move { answer(request) }).await;
        assert_eq!(core.client().todos().await.unwrap(), vec![todo()]);
    }

    #[tokio::test]
    async fn save_event_returns_the_stored_event() {
        let core = start(|request| async move { answer(request) }).await;
        let saved = core.client().save_event(event()).await.unwrap();
        assert_eq!(saved, stored_event(event()));
    }

    #[tokio::test]
    async fn delete_event_is_ok() {
        let core = start(|request| async move { answer(request) }).await;
        core.client().delete_event(event()).await.unwrap();
    }

    #[tokio::test]
    async fn save_todo_returns_the_stored_todo() {
        let core = start(|request| async move { answer(request) }).await;
        let saved = core.client().save_todo(todo()).await.unwrap();
        assert_eq!(saved, stored_todo(todo()));
    }

    #[tokio::test]
    async fn delete_todo_is_ok() {
        let core = start(|request| async move { answer(request) }).await;
        core.client().delete_todo(todo()).await.unwrap();
    }

    #[tokio::test]
    async fn move_todo_sends_href_etag_and_before() {
        let core = start(|request| async move { answer(request) }).await;
        let moved = stored_todo(todo());
        let before = Todo { uid: "todo-2".into(), ..stored_todo(todo()) };
        let before = Todo { href: Some("/tasks/home/todo-2.ics".into()), ..before };
        let written = core.client().move_todo(moved.clone(), Some(before)).await.unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(Some(written[0].uid.clone()), moved.href, "the href went out");
        assert_eq!(written[0].etag, moved.etag, "the etag went out");
        assert_eq!(written[0].summary, "/tasks/home/todo-2.ics", "before went out");
        let to_the_end = core.client().move_todo(moved, None).await.unwrap();
        assert_eq!(to_the_end[0].summary, "", "no before");
        assert!(matches!(core.client().move_todo(todo(), None).await, Err(ProviderError::Other(_))), "not saved");
    }

    #[tokio::test]
    async fn unpin_sends_list_and_item() {
        let core = start(|request| async move { answer(request) }).await;
        let client = core.client();
        client.unpin_issue("github:mahype/bugs".into(), "github:mahype/TorroWorkplace#7".into()).await.unwrap();
        let other = client.unpin_issue("github:mahype/bugs".into(), "github:mahype/TorroWorkplace#8".into()).await;
        assert_eq!(other, Err(ProviderError::Other("the list does not pin it".into())));
    }

    #[tokio::test]
    async fn error_codes_map_to_provider_errors() {
        assert_eq!(error_from("auth", "login refused").await, ProviderError::Auth);
        assert_eq!(error_from("offline", "no route to host").await, ProviderError::Offline);
        assert_eq!(error_from("conflict", "changed").await, ProviderError::Conflict { uid: String::new() });
        assert_eq!(error_from("error", "disk full").await, ProviderError::Other("disk full".into()));
        assert_eq!(error_from("busy", "sync running").await, ProviderError::Other("sync running".into()));
    }

    #[tokio::test]
    async fn conflict_on_write_names_the_item() {
        let core = start(|_| async { Response::Error { code: "conflict".into(), message: "changed".into() } }).await;
        assert_eq!(core.client().save_event(event()).await, Err(ProviderError::Conflict { uid: "meeting-1".into() }));
        assert_eq!(core.client().save_todo(todo()).await, Err(ProviderError::Conflict { uid: "todo-1".into() }));
        let stored = stored_todo(todo());
        assert_eq!(core.client().move_todo(stored, None).await, Err(ProviderError::Conflict { uid: "todo-1".into() }));
    }

    #[tokio::test]
    async fn unexpected_variant_is_other() {
        let core = start(|_| async { Response::Ok }).await;
        let err = core.client().collections().await.unwrap_err();
        assert!(matches!(&err, ProviderError::Other(msg) if msg.starts_with("unexpected answer")), "got {err:?}");
    }

    #[tokio::test]
    async fn missing_core_is_other_with_ipc_message() {
        let dir = tempfile::tempdir().unwrap();
        let client = PimClient { socket: dir.path().join("core.sock") };
        assert_eq!(client.accounts().await, Err(ProviderError::Other(IpcError::NotRunning.to_string())));
    }
}
