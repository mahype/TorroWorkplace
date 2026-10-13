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
//! One exception: `approvals.decide_approval` answers `conflict` for an approval that was decided
//! already or expired, which [`InboxClient::decide_approval`] reports as [`InboxError::Settled`].
//!
//! The client checks nothing itself: rights and the security rules of the inbox are the core's.
//!
//! Mail (task E4.2): the inbox shows the folders and whole messages of the mail accounts, so the
//! client also sends the `mail.*` requests the window needs – [`InboxClient::mail_folders`],
//! [`InboxClient::search_mail`], [`InboxClient::get_mail`], [`InboxClient::mail_attachment`],
//! [`InboxClient::set_mail_flags`], [`InboxClient::move_mail`] and [`InboxClient::delete_mail`] –,
//! with the errors mapped as above. Everything a message brings is data from outside
//! (`untrusted`), never instructions; the client only passes it on.

use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use torro_inbox::item::{Hidden, HideScope};
use torro_inbox::source::TaskList;
use torro_inbox::{InboxError, InboxSource, Item, ItemFilter, Lane, SourceInfo, SourceRef};
use torro_mail::{Flag, Folder, Message, MessageSummary};
use torro_pim::EventTime;

use crate::mail::MoveTarget;
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
        inbox_answer(crate::client::call(&self.socket, &request).await)
    }

    /// Sends a request answered with [`Response::Item`].
    async fn ask_item(&self, request: Request) -> Result<Item, InboxError> {
        match self.ask(request).await? {
            Response::Item { item } => Ok(item),
            other => Err(unexpected(other)),
        }
    }

    /// Sends a request answered with [`Response::Ok`].
    async fn ask_ok(&self, request: Request) -> Result<(), InboxError> {
        match self.ask(request).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.list_folders` for every mail account the app reaches, in the core's order. Empty
    /// without a mail account.
    pub async fn mail_folders(&self) -> Result<Vec<Folder>, InboxError> {
        match self.ask(Request::MailFolders { account: None }).await? {
            Response::MailFolders { folders } => Ok(folders),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.search_messages` in the folder `folder` (an id of [`InboxClient::mail_folders`]):
    /// at most `limit` (1 to 100) messages, newest first, matching `query` when there is one,
    /// from `cursor` on (the second value of the previous page). Answers the page and the cursor
    /// of the next one (`None` on the last page). Nothing filters by flags or time: the window
    /// shows the folder as it is.
    pub async fn search_mail(
        &self,
        folder: String,
        query: Option<String>,
        limit: u32,
        cursor: Option<String>,
    ) -> Result<(Vec<MessageSummary>, Option<String>), InboxError> {
        let request = Request::SearchMail {
            account: None,
            folder: Some(folder),
            query,
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit,
            cursor,
        };
        match self.ask(request).await? {
            Response::MailMessages { messages, cursor, .. } => Ok((messages, cursor)),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.get_message` with its text (`include_body`); reading it does not mark it read.
    pub async fn get_mail(&self, id: String) -> Result<Message, InboxError> {
        match self.ask(Request::GetMail { id, include_body: true }).await? {
            Response::MailMessage { message } => Ok(message),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.get_attachment`: the core fetches the attachment `part` of the message `id` into its
    /// attachment folder and answers where it put the file.
    pub async fn mail_attachment(&self, id: String, part: String) -> Result<PathBuf, InboxError> {
        match self.ask(Request::GetMailAttachment { id, attachment: part }).await? {
            Response::MailAttachment { path, .. } => Ok(PathBuf::from(path)),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.set_flags`: sets `add` and clears `remove` on the messages `ids` (1 to 100).
    pub async fn set_mail_flags(&self, ids: Vec<String>, add: Vec<Flag>, remove: Vec<Flag>) -> Result<(), InboxError> {
        self.ask_ok(Request::SetMailFlags { ids, add, remove }).await
    }

    /// `mail.move_messages`: moves the messages `ids` (1 to 100, one account) to `to`; they keep
    /// their ids, so moving them back is the undo.
    pub async fn move_mail(&self, ids: Vec<String>, to: MoveTarget) -> Result<(), InboxError> {
        self.ask_ok(Request::MoveMail { ids, to }).await
    }

    /// `mail.delete_messages` into the trash, never for good: the window offers an undo instead
    /// (moving them back).
    pub async fn delete_mail(&self, ids: Vec<String>) -> Result<(), InboxError> {
        self.ask_ok(Request::DeleteMail { ids, permanent: false }).await
    }
}

/// The core's answer, or the error it reported, as the module documentation maps them.
fn inbox_answer(answer: Result<Response, IpcError>) -> Result<Response, InboxError> {
    match answer {
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

    /// `inbox.hide_sender` (tasks E3.10, E3.11): the core hides the entry or keeps the rule and
    /// answers what changed.
    async fn hide_sender(&self, id: String, scope: HideScope) -> Result<Hidden, InboxError> {
        match self.ask(Request::HideSender { id, scope }).await? {
            Response::Hidden(hidden) => Ok(hidden),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.remove_rule` (task E3.11: "Undo" after hiding a sender or domain).
    async fn remove_rule(&self, id: String) -> Result<(), InboxError> {
        match self.ask(Request::RemoveRule { id }).await? {
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

    /// `tasks.list_lists`, without the lists nothing can be written to (GitHub filter lists,
    /// read-only shares) and without calendars that hold no tasks.
    async fn list_task_lists(&self) -> Result<Vec<TaskList>, InboxError> {
        match self.ask(Request::TaskCollections).await? {
            Response::Collections { collections } => Ok(collections
                .into_iter()
                .filter(|list| list.writable && list.kind.has_todos() && list.source.is_none())
                .map(|list| TaskList { id: list.id, name: list.name })
                .collect()),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.create_task_from_item` with the day as an all-day due date; the core answers with
    /// the new task, which the app does not need.
    async fn create_task_from_item(
        &self,
        id: String,
        list: String,
        title: Option<String>,
        due: Option<NaiveDate>,
    ) -> Result<(), InboxError> {
        let due = due.map(EventTime::Date);
        match self.ask(Request::CreateTaskFromItem { id, collection: list, title, due }).await? {
            Response::Todo { .. } => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    /// `approvals.decide_approval` with the app key (task E3.8). The core answers with the
    /// approval as it stands now, which the app does not need: the entry is done either way, and
    /// the next list shows it. `conflict` (decided already or expired) is [`InboxError::Settled`].
    async fn decide_approval(&self, approval: String, allow: bool) -> Result<(), InboxError> {
        let answer = crate::client::call(&self.socket, &Request::DecideApproval { id: approval, allow }).await;
        if let Ok(Response::Error { code, .. }) = &answer
            && code == "conflict"
        {
            return Err(InboxError::Settled);
        }
        match inbox_answer(answer)? {
            Response::Approval { .. } => Ok(()),
            other => Err(unexpected(other)),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::future::Future;
    use std::time::Duration;

    use chrono::TimeZone;
    use torro_inbox::{Kind, Rule, State, StateFilter};

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
            // The rule names the entry and the scope that arrived.
            Request::HideSender { id, scope } => Response::Hidden(Hidden {
                rule: Some(Rule {
                    id: format!("{id}|{scope:?}"),
                    source: SourceRef::mail("anna@example.org"),
                    sender: None,
                    domain: Some("shop.example".into()),
                    effect: torro_inbox::RuleEffect::Muted,
                    created: at(10),
                }),
                new_rule: true,
                done: vec![id],
            }),
            Request::RemoveRule { id } if id == "r1" => Response::Ok,
            Request::OpenItem { id } if id == "window" => Response::Ok,
            Request::TaskCollections => Response::Collections { collections: lists() },
            Request::CreateTaskFromItem { id, collection, title, due } if id == "i1" => Response::Todo {
                todo: torro_pim::Todo {
                    uid: "new".into(),
                    collection,
                    href: Some("https://cloud.example.org/t/new.ics".into()),
                    etag: Some("\"1\"".into()),
                    summary: title.unwrap_or_default(),
                    due,
                    priority: None,
                    completed: false,
                    parent: None,
                    description: None,
                    url: None,
                    categories: Vec::new(),
                    sort_order: None,
                    link: None,
                },
            },
            Request::CreateTaskFromItem { .. } => {
                Response::Error { code: "not-found".into(), message: "no task list \"gone\"".into() }
            }
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
        let hidden = client.hide_sender("m1".into(), HideScope::Domain).await.expect("hidden");
        let rule = hidden.rule.expect("a rule");
        assert_eq!((rule.id.as_str(), rule.domain.as_deref()), ("m1|Domain", Some("shop.example")));
        assert_eq!((hidden.new_rule, hidden.done), (true, vec!["m1".to_string()]));
        client.remove_rule("r1".into()).await.expect("removed");
    }

    #[tokio::test]
    async fn opening_sends_open_item_and_maps_unsupported() {
        let core = start(|request| async move { answer(request) }).await;
        let client = core.client();
        client.open_item("window".into()).await.expect("opened");
        assert_eq!(client.open_item("gone".into()).await, Err(InboxError::Unsupported));
    }

    /// A writable task list, a read-only share, a GitHub filter list and a calendar without tasks.
    fn lists() -> Vec<torro_pim::Collection> {
        let list = |id: &str, kind, writable| torro_pim::Collection {
            id: id.into(),
            name: id.to_uppercase(),
            kind,
            color: None,
            writable,
            source: None,
        };
        let filter = torro_pim::Collection {
            source: Some(torro_pim::model::GitHubFilter {
                queries: vec!["is:open".into()],
                pinned: Vec::new(),
                revision: 1,
                truncated: false,
                problem: None,
            }),
            ..list("github", torro_pim::CollectionKind::Todos, false)
        };
        vec![
            list("work", torro_pim::CollectionKind::Both, true),
            list("shared", torro_pim::CollectionKind::Todos, false),
            filter,
            list("events", torro_pim::CollectionKind::Events, true),
        ]
    }

    #[tokio::test]
    async fn only_lists_a_task_can_be_written_to_are_offered() {
        let core = start(|request| async move { answer(request) }).await;
        let lists = core.client().list_task_lists().await.expect("lists");
        assert_eq!(lists, vec![TaskList { id: "work".into(), name: "WORK".into() }]);
    }

    #[tokio::test]
    async fn making_a_task_sends_the_day_as_due_date() {
        let core = start(|request| async move {
            // Only the request the client should build is answered with a task.
            let day = NaiveDate::from_ymd_opt(2026, 10, 14).map(EventTime::Date);
            match request {
                Request::CreateTaskFromItem { ref title, due, .. } if title.as_deref() == Some("Fix") && due == day => {
                    answer(request)
                }
                Request::CreateTaskFromItem { .. } => Response::Error { code: "error".into(), message: "wrong".into() },
                other => answer(other),
            }
        })
        .await;
        let client = core.client();
        let day = NaiveDate::from_ymd_opt(2026, 10, 14);
        client.create_task_from_item("i1".into(), "work".into(), Some("Fix".into()), day).await.expect("made");
        let gone = client.create_task_from_item("i2".into(), "gone".into(), Some("Fix".into()), day).await;
        assert_eq!(gone, Err(InboxError::NotFound));
    }

    #[tokio::test]
    async fn deciding_sends_the_decision_and_a_settled_approval_is_its_own_error() {
        let core = start(|request| async move {
            match request {
                Request::DecideApproval { id, allow: true } if id == "a1" => Response::Approval {
                    approval: crate::ApprovalInfo {
                        id,
                        client: "c1".into(),
                        client_name: "Claude Code".into(),
                        command: "approvals.ask_permission".into(),
                        summary: "Bash: cargo test".into(),
                        details: Vec::new(),
                        created: at(10),
                        expires: at(11),
                        decided: Some(at(10)),
                        state: crate::ApprovalState::Running,
                    },
                },
                Request::DecideApproval { .. } => {
                    Response::Error { code: "conflict".into(), message: "decided already or expired".into() }
                }
                other => answer(other),
            }
        })
        .await;
        let client = core.client();
        assert_eq!(client.decide_approval("a1".into(), true).await, Ok(()));
        assert_eq!(client.decide_approval("a1".into(), false).await, Err(InboxError::Settled));
    }

    fn summary(id: &str, folder: &str) -> MessageSummary {
        MessageSummary {
            id: id.into(),
            account: "mail:anna@example.org".into(),
            folder: folder.into(),
            thread_id: "t1".into(),
            from: None,
            to: Vec::new(),
            subject: "Offer".into(),
            date: at(9),
            snippet: String::new(),
            flags: Vec::new(),
            has_attachments: false,
            untrusted: true,
        }
    }

    /// Answers the mail requests the window sends, each only with the arguments the client should
    /// build; anything else is an error naming what arrived.
    fn mail_answer(request: Request) -> Response {
        let folder = |id: &str, role| torro_mail::Folder {
            id: id.into(),
            account: "mail:anna@example.org".into(),
            name: id.into(),
            path: id.into(),
            role,
            unread: 1,
            total: 2,
        };
        match request {
            Request::MailFolders { account: None } => {
                Response::MailFolders { folders: vec![folder("f:inbox", Some(torro_mail::Role::Inbox))] }
            }
            Request::SearchMail {
                account: None,
                folder: Some(folder),
                query,
                unread: None,
                flagged: None,
                since: None,
                before: None,
                limit: 50,
                cursor,
            } => Response::MailMessages {
                messages: vec![summary(
                    &format!("{}|{}", query.unwrap_or_default(), cursor.unwrap_or_default()),
                    &folder,
                )],
                cursor: Some("next".into()),
                source: crate::mail::MailSource::Cache,
            },
            Request::GetMail { id, include_body: true } => Response::MailMessage {
                message: Message {
                    summary: summary(&id, "f:inbox"),
                    cc: Vec::new(),
                    reply_to: Vec::new(),
                    text: Some("Hello".into()),
                    from_html: false,
                    attachments: Vec::new(),
                    traits: torro_mail::MessageTraits::default(),
                    invitation: false,
                },
            },
            Request::GetMailAttachment { id, attachment } => Response::MailAttachment {
                path: format!("/attachments/{id}-{attachment}.pdf"),
                size: 3,
                content_type: "application/pdf".into(),
            },
            Request::SetMailFlags { ids, add, remove }
                if ids == ["m:1"] && add == [Flag::Flagged] && remove == [Flag::Seen] =>
            {
                Response::Ok
            }
            Request::MoveMail { ids, to: MoveTarget::Role(crate::mail::MoveRole::Archive) } if ids == ["m:1"] => {
                Response::Ok
            }
            Request::DeleteMail { ids, permanent: false } if ids == ["m:1", "m:2"] => Response::Ok,
            Request::DeleteMail { .. } => Response::Error { code: "not-found".into(), message: "gone".into() },
            other => answer(other),
        }
    }

    #[tokio::test]
    async fn mail_calls_send_what_the_window_needs() {
        let core = start(|request| async move { mail_answer(request) }).await;
        let client = core.client();
        let folders = client.mail_folders().await.expect("folders");
        assert_eq!(folders.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["f:inbox"]);
        let (page, next) =
            client.search_mail("f:inbox".into(), Some("offer".into()), 50, Some("c1".into())).await.expect("a page");
        assert_eq!(
            (page[0].id.as_str(), page[0].folder.as_str(), next.as_deref()),
            ("offer|c1", "f:inbox", Some("next"))
        );
        assert!(page[0].untrusted, "foreign text stays marked");
        let message = client.get_mail("m:1".into()).await.expect("the message");
        assert_eq!((message.summary.id.as_str(), message.text.as_deref()), ("m:1", Some("Hello")));
        let path = client.mail_attachment("m:1".into(), "2".into()).await.expect("fetched");
        assert_eq!(path, PathBuf::from("/attachments/m:1-2.pdf"));
        client.set_mail_flags(vec!["m:1".into()], vec![Flag::Flagged], vec![Flag::Seen]).await.expect("flagged");
        let archive = MoveTarget::Role(crate::mail::MoveRole::Archive);
        client.move_mail(vec!["m:1".into()], archive).await.expect("archived");
        client.delete_mail(vec!["m:1".into(), "m:2".into()]).await.expect("into the trash, not for good");
        assert_eq!(client.delete_mail(vec!["m:3".into()]).await, Err(InboxError::NotFound));
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
