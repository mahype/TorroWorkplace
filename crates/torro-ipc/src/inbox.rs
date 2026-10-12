//! [`InboxSource`] over the socket: how the inbox app reaches the core (task E1.3).
//!
//! Each method sends one `inbox.*` [`Request`] with [`crate::client::call`] (the app key next to
//! the socket, like [`crate::pim::PimClient`]) and maps the [`Response`]:
//!
//! - the expected variant → `Ok`,
//! - `Response::Error { code, .. }`: `not-found` → [`InboxError::NotFound`], `forbidden` →
//!   [`InboxError::Forbidden`], `busy` → [`InboxError::Busy`], `unsupported` →
//!   [`InboxError::Unsupported`], every other code →
//!   [`InboxError::Other`] with the core's message,
//! - a core that is not running → [`InboxError::Offline`], so the app shows "isn't responding"
//!   and tries again on its next refresh; any other socket problem → [`InboxError::Other`] with
//!   the `IpcError` message,
//! - an unexpected variant → `InboxError::Other("unexpected answer …")`.
//!
//! The client checks nothing itself: rights and the security rules of the inbox are the core's.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use torro_inbox::{InboxError, InboxSource, Item, ItemFilter, Lane, SourceInfo, SourceRef};

use crate::{IpcError, Request, Response};

/// Talks to the core at its socket.
#[derive(Debug, Clone)]
pub struct InboxClient {
    pub socket: PathBuf,
}

impl Default for InboxClient {
    /// The core of this user session ([`crate::socket_path`]).
    fn default() -> InboxClient {
        InboxClient { socket: crate::socket_path().unwrap_or_else(|| PathBuf::from("core.sock")) }
    }
}

impl InboxClient {
    /// Sends `request` and returns the answer, or the error the core reported as an
    /// [`InboxError`]. Callers match the variant they expect.
    async fn ask(&self, request: Request) -> Result<Response, InboxError> {
        match crate::client::call(&self.socket, &request).await {
            Ok(Response::Error { code, message }) => Err(match code.as_str() {
                "not-found" => InboxError::NotFound,
                "forbidden" => InboxError::Forbidden,
                "busy" => InboxError::Busy,
                "unsupported" => InboxError::Unsupported,
                _ => InboxError::Other(message),
            }),
            Ok(response) => Ok(response),
            Err(IpcError::NotRunning) => Err(InboxError::Offline),
            Err(err) => Err(InboxError::Other(err.to_string())),
        }
    }

    /// Sends a request answered with [`Response::Item`].
    async fn ask_item(&self, request: Request) -> Result<Item, InboxError> {
        match self.ask(request).await? {
            Response::Item { item } => Ok(item),
            other => Err(unexpected(other)),
        }
    }
}

/// An answer that is not the variant the request asks for.
fn unexpected(response: Response) -> InboxError {
    InboxError::Other(format!("unexpected answer: {response:?}"))
}

impl InboxSource for InboxClient {
    /// `inbox.list_items` with the fields of `filter`; the core sorts, wakes and cuts.
    async fn list_items(&self, filter: ItemFilter) -> Result<Vec<Item>, InboxError> {
        let ItemFilter { lane, source, state, since, limit } = filter;
        match self.ask(Request::ListItems { lane, source, state, since, limit }).await? {
            Response::Items { items } => Ok(items),
            other => Err(unexpected(other)),
        }
    }

    async fn list_sources(&self) -> Result<Vec<SourceInfo>, InboxError> {
        match self.ask(Request::ListSources).await? {
            Response::Sources { sources } => Ok(sources),
            other => Err(unexpected(other)),
        }
    }

    async fn set_item_done(&self, id: String, done: bool) -> Result<Item, InboxError> {
        self.ask_item(Request::SetItemDone { id, done }).await
    }

    async fn snooze_item(&self, id: String, until: Option<DateTime<Utc>>) -> Result<Item, InboxError> {
        self.ask_item(Request::SnoozeItem { id, until }).await
    }

    async fn set_item_lane(&self, id: String, lane: Lane, remember: bool) -> Result<Item, InboxError> {
        self.ask_item(Request::SetItemLane { id, lane, remember }).await
    }

    async fn mute_source(&self, source: SourceRef, muted: bool) -> Result<(), InboxError> {
        match self.ask(Request::MuteSource { source, muted }).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.open_item`: the core opens the entry (window, page or app) on the person's screen.
    async fn open_item(&self, id: String) -> Result<(), InboxError> {
        match self.ask(Request::OpenItem { id }).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::future::Future;
    use std::time::Duration;

    use chrono::TimeZone;
    use torro_inbox::{Kind, State, StateFilter};

    use super::*;
    use crate::{client, server};

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 12, hour, 0, 0).single().expect("valid time")
    }

    fn item(id: &str) -> Item {
        Item {
            id: id.into(),
            source: SourceRef::script("ci"),
            thread_key: "build:main".into(),
            kind: Kind::Notice,
            lane: Lane::ForMe,
            urgent: false,
            title: "Build failed".into(),
            body: None,
            from: None,
            agent: None,
            received: at(8),
            updated: at(9),
            count: 1,
            state: State::New,
            link: None,
            actions: Vec::new(),
            untrusted: true,
        }
    }

    /// Canned answers that echo what arrived, so the tests see the arguments.
    fn answer(request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "test".into(), api: crate::API_VERSION },
            // The id names the filter that arrived.
            Request::ListItems { lane, source, state, since, limit } => Response::Items {
                items: vec![item(&format!(
                    "{lane:?}|{}|{state:?}|{}|{limit:?}",
                    source.map(|s| s.key()).unwrap_or_default(),
                    since.map(|s| s.to_rfc3339()).unwrap_or_default()
                ))],
            },
            Request::ListSources => Response::Sources {
                sources: vec![SourceInfo { source: SourceRef::script("ci"), open: 2, muted: false }],
            },
            Request::SetItemDone { id, done } => {
                let state = if done { State::Done { at: at(10) } } else { State::Seen };
                Response::Item { item: Item { state, ..item(&id) } }
            }
            Request::SnoozeItem { id, until } => Response::Item {
                item: Item { state: until.map_or(State::Seen, |until| State::Snoozed { until }), ..item(&id) },
            },
            Request::SetItemLane { id, lane, remember } => {
                Response::Item { item: Item { lane, title: format!("remember {remember}"), ..item(&id) } }
            }
            Request::MuteSource { source, muted: true } if source == SourceRef::script("ci") => Response::Ok,
            Request::OpenItem { id } if id == "window" => Response::Ok,
            Request::OpenItem { .. } => {
                Response::Error { code: "unsupported".into(), message: "no window of the agent found".into() }
            }
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
        fn client(&self) -> InboxClient {
            InboxClient { socket: self.socket.clone() }
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
            if client::call(&socket, &Request::Ping).await.is_ok() {
                return Core { _dir: dir, socket, server };
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("core did not start listening on {}", socket.display());
    }

    /// What the client makes of a core that answers everything with `code`.
    async fn error_from(code: &'static str) -> InboxError {
        let core =
            start(move |_| async move { Response::Error { code: code.into(), message: "said the core".into() } }).await;
        core.client().set_item_done("x".into(), true).await.expect_err("the core answers with an error")
    }

    #[tokio::test]
    async fn list_items_sends_every_field_of_the_filter() {
        let core = start(|request| async move { answer(request) }).await;
        let filter = ItemFilter {
            lane: Some(Lane::ForMe),
            source: Some(SourceRef::agent("codex")),
            state: Some(StateFilter::Open),
            since: Some(at(7)),
            limit: Some(50),
        };
        let items = core.client().list_items(filter).await.expect("listed");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, format!("Some(ForMe)|agent:codex|Some(Open)|{}|Some(50)", at(7).to_rfc3339()));
        let all = core.client().list_items(ItemFilter::default()).await.expect("listed");
        assert_eq!(all[0].id, "None||None||None");
        assert!(all[0].untrusted, "the flag comes along");
    }

    #[tokio::test]
    async fn sources_are_listed() {
        let core = start(|request| async move { answer(request) }).await;
        let sources = core.client().list_sources().await.expect("listed");
        assert_eq!(sources, vec![SourceInfo { source: SourceRef::script("ci"), open: 2, muted: false }]);
    }

    #[tokio::test]
    async fn changes_return_the_entry_as_stored() {
        let core = start(|request| async move { answer(request) }).await;
        let client = core.client();
        assert_eq!(client.set_item_done("a".into(), true).await.expect("done").state, State::Done { at: at(10) });
        assert_eq!(client.set_item_done("a".into(), false).await.expect("open").state, State::Seen);
        let snoozed = client.snooze_item("b".into(), Some(at(15))).await.expect("snoozed");
        assert_eq!((snoozed.id.as_str(), snoozed.state), ("b", State::Snoozed { until: at(15) }));
        assert_eq!(client.snooze_item("b".into(), None).await.expect("back").state, State::Seen);
        let moved = client.set_item_lane("c".into(), Lane::Info, true).await.expect("moved");
        assert_eq!((moved.lane, moved.title.as_str()), (Lane::Info, "remember true"));
        client.mute_source(SourceRef::script("ci"), true).await.expect("muted");
    }

    #[tokio::test]
    async fn opening_sends_open_item_and_maps_unsupported() {
        let core = start(|request| async move { answer(request) }).await;
        let client = core.client();
        client.open_item("window".into()).await.expect("opened");
        assert_eq!(client.open_item("gone".into()).await, Err(InboxError::Unsupported));
    }

    #[tokio::test]
    async fn error_codes_map_to_inbox_errors() {
        assert_eq!(error_from("not-found").await, InboxError::NotFound);
        assert_eq!(error_from("forbidden").await, InboxError::Forbidden);
        assert_eq!(error_from("busy").await, InboxError::Busy);
        assert_eq!(error_from("unsupported").await, InboxError::Unsupported);
        assert_eq!(error_from("unauthorized").await, InboxError::Other("said the core".into()));
        assert_eq!(error_from("error").await, InboxError::Other("said the core".into()));
    }

    #[tokio::test]
    async fn an_unexpected_answer_is_other() {
        let core = start(|_| async { Response::Ok }).await;
        let err = core.client().list_items(ItemFilter::default()).await.expect_err("not items");
        assert!(matches!(&err, InboxError::Other(message) if message.starts_with("unexpected answer")), "{err:?}");
        let err = core.client().set_item_done("a".into(), true).await.expect_err("not an item");
        assert!(matches!(&err, InboxError::Other(message) if message.starts_with("unexpected answer")), "{err:?}");
    }

    #[tokio::test]
    async fn a_core_that_is_not_running_is_offline() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let client = InboxClient { socket: dir.path().join("core.sock") };
        assert_eq!(client.list_items(ItemFilter::default()).await, Err(InboxError::Offline));
        assert_eq!(client.list_sources().await, Err(InboxError::Offline));
    }
}
