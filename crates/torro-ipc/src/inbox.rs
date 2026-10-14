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
//!
//! Tasks of folder mail (task E8.1b): [`InboxClient::create_todo_from_mail`] sends
//! `tasks.create_todo_from_mail` and answers the task in the inbox's model ([`ItemTask`]), so the
//! window needs no calendar model of its own for the due day.
//!
//! The hub (task E8.5b): [`InboxClient::get_quiet`] asks whether it is quiet now,
//! [`InboxClient::list_calendars`] and [`InboxClient::create_event_from_item`] make an event of an
//! entry, its times as instants, so the window needs no calendar model either.
//!
//! Invitations (task E8.3c): [`InboxClient::respond_to_invitation`] sends
//! `mail.respond_to_invitation` with the calendar the window chose, and
//! [`InboxClient::list_events`] reads the events of the shown calendars around an invitation as
//! [`CalendarEvent`]s – title, instants and whether they make their time busy –, so the window can
//! say what the invitation overlaps without a calendar model.

use std::path::PathBuf;

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeDelta, Utc};
use torro_inbox::item::{Hidden, HideScope, ItemTask, MuteEffect, MuteOption, MuteScope, Muted, TaskDue};
use torro_inbox::source::TaskList;
use torro_inbox::{InboxError, InboxSource, Item, ItemFilter, Lane, SourceInfo, SourceRef};
use torro_mail::{Flag, Folder, InvitationAnswer, Message, MessageSummary, Order};
use torro_pim::EventTime;

use crate::mail::{Delivery, MoveTarget};
use crate::{InboxStats, IpcError, Quiet, Request, Response, StatsRange};

/// An event of a shown calendar as the inbox needs it to tell what an invitation overlaps
/// ([`InboxClient::list_events`], task E8.3c), in instants, so the window needs no calendar model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    /// The event's title (`SUMMARY`) as the calendar holds it; may be empty. Text of the person or
    /// of whoever invited them: shown as it is, never interpreted.
    pub title: String,
    /// When it starts; an all-day event at local midnight of its first day.
    pub start: DateTime<Utc>,
    /// When it ends (exclusive): an all-day event at local midnight after its last day (a day after
    /// its start without an end), a timed event without an end where it starts.
    pub end: DateTime<Utc>,
    /// It makes its time busy (`torro_pim::Event::busy`): opaque and not all-day. Free time,
    /// holidays and birthdays are `false`.
    pub busy: bool,
}

impl CalendarEvent {
    /// `event` in instants, all-day times at local midnight ([`CalendarEvent`]).
    fn of(event: torro_pim::Event) -> CalendarEvent {
        let busy = event.busy();
        let instant = |time: EventTime| match time {
            EventTime::Date(day) => local_midnight(day),
            EventTime::DateTime(at) => at,
        };
        let start = instant(event.start);
        let end = match (event.start, event.end) {
            (_, Some(end)) => instant(end),
            (EventTime::Date(_), None) => start + TimeDelta::days(1),
            (EventTime::DateTime(_), None) => start,
        };
        CalendarEvent { title: event.summary, start, end: end.max(start), busy }
    }
}

/// Midnight at the start of the local day `day`, as an instant; on a day that starts in a gap of
/// the clock (a change of summer time at midnight) the first instant after it, else UTC midnight.
fn local_midnight(day: NaiveDate) -> DateTime<Utc> {
    let midnight = day.and_time(NaiveTime::MIN);
    midnight
        .and_local_timezone(Local)
        .earliest()
        .or_else(|| (midnight + TimeDelta::hours(1)).and_local_timezone(Local).earliest())
        .map_or_else(|| midnight.and_utc(), |at| at.with_timezone(&Utc))
}

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
    /// at most `limit` (1 to 100) messages in `order` (task E8.15), matching `query` when there
    /// is one, from `cursor` on (the second value of the previous page, asked in the same
    /// `order`). Answers the page and the cursor of the next one (`None` on the last page).
    /// Nothing filters by flags or time: the window shows the folder as it is.
    pub async fn search_mail(
        &self,
        folder: String,
        query: Option<String>,
        limit: u32,
        cursor: Option<String>,
        order: Order,
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
            order: asked(order),
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

    /// `tasks.create_todo_from_mail` (task E8.7a; the inbox's "Task" on a folder's message, task
    /// E8.1b): makes a task of the message `id` of the mail account `account` (`mail:<address>`) in
    /// the task list `list` (one of [`InboxSource::list_task_lists`]), titled `title` (`None` or
    /// blank: the core takes the subject), due on the local day `due` as an all-day date (`None`: no
    /// due date) – as [`InboxSource::create_task_from_item`] sends its day. The mail itself is not
    /// changed; the core links the task back with `mail:<address>:<Message-ID>`.
    ///
    /// Answers the task as an entry carries it ([`ItemTask`]): uid, list (`list.name`), title, due
    /// and done as the core stored them, its link id (empty when the core sent none),
    /// `mail_entry` `None`. Errors as mapped above: [`InboxError::NotFound`] for an account, mail or
    /// list the app may not see, [`InboxError::Unsupported`] for a mail without a `Message-ID` or a
    /// list that takes no tasks, [`InboxError::Forbidden`] without the right to write tasks.
    pub async fn create_todo_from_mail(
        &self,
        account: String,
        id: String,
        list: TaskList,
        title: Option<String>,
        due: Option<NaiveDate>,
    ) -> Result<ItemTask, InboxError> {
        let request =
            Request::CreateTodoFromMail { account, id, collection: list.id, title, due: due.map(EventTime::Date) };
        match self.ask(request).await? {
            Response::Todo { todo } => Ok(ItemTask {
                uid: todo.uid,
                collection: todo.collection,
                list: list.name,
                title: todo.summary,
                due: todo.due.map(|due| match due {
                    EventTime::Date(day) => TaskDue::Date(day),
                    EventTime::DateTime(at) => TaskDue::DateTime(at),
                }),
                completed: todo.completed,
                link: todo.link.map(|link| link.id).unwrap_or_default(),
                mail_entry: None,
            }),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.get_stats` (task E8.8a): what the core counted over `range` up to the local day
    /// `until` (`None`: today), for "Insights". The app shows it as it comes; suggestions carry
    /// the entry and the [`MuteOption`] to pass to [`InboxSource::mute_item`]. Their values are
    /// foreign text where `untrusted`. Errors as for the other calls.
    pub async fn get_stats(&self, range: StatsRange, until: Option<NaiveDate>) -> Result<InboxStats, InboxError> {
        match self.ask(Request::GetStats { range, until }).await? {
            Response::Stats(stats) => Ok(stats),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.get_quiet` (task E8.5b): whether the inbox is quiet now, why and until when, for the
    /// hub's "Quiet". `None` when it is not quiet. The app key always gets the event's title.
    /// Errors as for the other calls.
    pub async fn get_quiet(&self) -> Result<Option<Quiet>, InboxError> {
        match self.ask(Request::GetQuiet).await? {
            Response::Quiet { quiet } => Ok(quiet),
            other => Err(unexpected(other)),
        }
    }

    /// `calendar.list_calendars`, without the calendars nothing can be written to (read-only
    /// shares) and without collections that hold no events: the calendars an event of an entry can
    /// go to (task E8.5b), each with its id and name in a [`TaskList`], the inbox's pair of both.
    pub async fn list_calendars(&self) -> Result<Vec<TaskList>, InboxError> {
        match self.ask(Request::CalendarCollections).await? {
            Response::Collections { collections } => Ok(collections
                .into_iter()
                .filter(|calendar| calendar.writable && calendar.kind.has_events() && calendar.source.is_none())
                .map(|calendar| TaskList { id: calendar.id, name: calendar.name })
                .collect()),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.respond_to_invitation` (task E8.3b; the window's Accept · Maybe · Decline, task E8.3c):
    /// answers the invitation in the message `id` (`m:…`) of the mail account `account`
    /// (`mail:<address>`) with `answer`. The core replies to the organizer from the account and puts
    /// the event in `collection` (an id of [`InboxClient::list_calendars`]; `None`: the core's choice,
    /// the first writable calendar) – unless it made a copy before, which stays in its calendar –,
    /// or removes that copy for a decline. Nothing about the inbox entry changes: the window marks
    /// it done itself (`inbox.set_item_done`).
    ///
    /// Answers how the reply went ([`Delivery::Unknown`]: it may not have gone out). Errors as
    /// mapped above: [`InboxError::NotFound`] for an account, mail or calendar the app may not see,
    /// [`InboxError::Forbidden`], [`InboxError::Unsupported`] for an invitation to a series,
    /// [`InboxError::Busy`], [`InboxError::Other`] with the core's words for a mail without an
    /// invitation to answer, an event that is over or a send that failed.
    pub async fn respond_to_invitation(
        &self,
        account: String,
        id: String,
        answer: InvitationAnswer,
        collection: Option<String>,
    ) -> Result<Delivery, InboxError> {
        match self.ask(Request::RespondToInvitation { account, id, answer, collection }).await? {
            Response::InvitationAnswered { delivery, .. } => Ok(delivery),
            other => Err(unexpected(other)),
        }
    }

    /// `calendar.list_events` (task E8.3c): the events of the calendars the suite shows that overlap
    /// `[start, end)`, as [`CalendarEvent`]s in the core's order – for the window's line on what an
    /// invitation overlaps. Hidden calendars are not asked (the core lists only shown ones). Errors
    /// as for the other calls.
    pub async fn list_events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>, InboxError> {
        match self.ask(Request::Events { start, end }).await? {
            Response::Events { events } => Ok(events.into_iter().map(CalendarEvent::of).collect()),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.create_event_from_item` (task E8.5b, "Event" in the hub and the panel): makes an
    /// event of the entry `id` in the calendar `calendar` (an id of [`InboxClient::list_calendars`])
    /// from `start` to `end`, titled `title` (`None` or blank: the core takes the entry's), and
    /// marks the entry done. The core answers with the new event, which the app does not need.
    /// Errors as mapped above: [`InboxError::NotFound`] for an entry or calendar the app may not
    /// see, [`InboxError::Forbidden`] for an approval or an entry of the suite.
    pub async fn create_event_from_item(
        &self,
        id: String,
        calendar: String,
        title: Option<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<(), InboxError> {
        let (start, end) = (EventTime::DateTime(start), EventTime::DateTime(end));
        match self.ask(Request::CreateEventFromItem { id, collection: calendar, title, start, end }).await? {
            Response::Event { .. } => Ok(()),
            other => Err(unexpected(other)),
        }
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

/// `order` as a request carries it: the default (newest first) is left out, so a request reads as
/// it did before the field existed (task E8.15).
fn asked(order: Order) -> Option<Order> {
    (order != Order::default()).then_some(order)
}

/// An answer that is not the variant the request asks for.
fn unexpected(response: Response) -> InboxError {
    InboxError::Other(format!("unexpected answer: {response:?}"))
}

impl InboxSource for InboxClient {
    /// `inbox.list_items` with the fields of `filter`; the core sorts, wakes and cuts.
    async fn list_items(&self, filter: ItemFilter) -> Result<Vec<Item>, InboxError> {
        let ItemFilter { lane, source, state, since, limit, order } = filter;
        match self.ask(Request::ListItems { lane, source, state, since, limit, order: asked(order) }).await? {
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
        self.ask_item(Request::SetItemDone { id, done, dismissed: false }).await
    }

    /// `inbox.set_item_done` with `dismissed: true` (task E8.6a).
    async fn dismiss_item(&self, id: String) -> Result<Item, InboxError> {
        self.ask_item(Request::SetItemDone { id, done: true, dismissed: true }).await
    }

    /// `inbox.list_mute_options` (task E8.6a).
    async fn list_mute_options(&self, id: String) -> Result<Vec<MuteOption>, InboxError> {
        match self.ask(Request::ListMuteOptions { id }).await? {
            Response::MuteOptions { options } => Ok(options),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.mute_item` (task E8.6a): the core keeps the rule or changes the entry and answers what
    /// changed.
    async fn mute_item(
        &self,
        id: String,
        scope: MuteScope,
        until: Option<DateTime<Utc>>,
        effect: MuteEffect,
    ) -> Result<Muted, InboxError> {
        match self.ask(Request::MuteItem { id, scope, until, effect }).await? {
            Response::Muted(muted) => Ok(muted),
            other => Err(unexpected(other)),
        }
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
            facets: Vec::new(),
            task: None,
            invite: None,
        }
    }

    /// Canned answers that echo what arrived, so the tests see the arguments.
    fn answer(request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "test".into(), api: crate::API_VERSION },
            // The id names the filter that arrived.
            Request::ListItems { lane, source, state, since, limit, order } => Response::Items {
                items: vec![item(&format!(
                    "{lane:?}|{}|{state:?}|{}|{limit:?}{}",
                    source.map(|s| s.key()).unwrap_or_default(),
                    since.map(|s| s.to_rfc3339()).unwrap_or_default(),
                    order.map(|order| format!("|{}", order.word())).unwrap_or_default()
                ))],
            },
            Request::ListSources => Response::Sources {
                sources: vec![SourceInfo { source: SourceRef::script("ci"), open: 2, muted: false, health: None }],
            },
            // A dismissed entry is titled so, to see the flag arrived.
            Request::SetItemDone { id, done, dismissed } => {
                let state = if done { State::Done { at: at(10) } } else { State::Seen };
                Response::Item { item: Item { state, title: format!("dismissed {dismissed}"), ..item(&id) } }
            }
            Request::ListMuteOptions { id } => Response::MuteOptions {
                options: vec![MuteOption {
                    scope: MuteScope::Conversation,
                    value: None,
                    label: id,
                    untrusted: false,
                    admin: false,
                }],
            },
            // The moved id names what arrived.
            Request::MuteItem { id, scope, until, effect } => Response::Muted(Muted {
                moved: vec![format!("{id}|{scope:?}|{}|{effect:?}", until.is_some())],
                ..Muted::default()
            }),
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
                    facet: None,
                    until: None,
                }),
                new_rule: true,
                done: vec![id],
            }),
            Request::RemoveRule { id } if id == "r1" => Response::Ok,
            // The span names what arrived.
            Request::GetStats { range, until } => Response::Stats(crate::InboxStats {
                range,
                from: until.unwrap_or_default(),
                until: until.unwrap_or_default(),
                days: Vec::new(),
                hours: vec![0; 24],
                totals: crate::StatsCounts { new: range.days(), ..crate::StatsCounts::default() },
                median_done_minutes: None,
                suggestions: Vec::new(),
            }),
            Request::GetQuiet => Response::Quiet {
                quiet: Some(Quiet { reason: crate::QuietReason::Busy, until: at(15), title: Some("Call".into()) }),
            },
            Request::CalendarCollections => Response::Collections { collections: lists() },
            // Only the times the client should send make an event.
            Request::CreateEventFromItem { id, collection, title, start, end }
                if id == "i1" && start == EventTime::DateTime(at(14)) && end == EventTime::DateTime(at(15)) =>
            {
                Response::Event {
                    event: torro_pim::Event {
                        uid: "new".into(),
                        collection,
                        href: Some("https://cloud.example.org/c/new.ics".into()),
                        etag: Some("\"1\"".into()),
                        summary: title.unwrap_or_default(),
                        start,
                        end: Some(end),
                        location: None,
                        description: None,
                        recurring: false,
                        transparency: torro_pim::Transparency::Opaque,
                    },
                }
            }
            Request::CreateEventFromItem { .. } => {
                Response::Error { code: "forbidden".into(), message: "an approval is the person's".into() }
            }
            // Task E8.3c: the calendar chosen comes back as the event's, an unknown one is none.
            Request::RespondToInvitation { id, answer, collection, .. } if id == "m:invite" => {
                Response::InvitationAnswered {
                    delivery: match answer {
                        torro_mail::InvitationAnswer::Tentative => Delivery::Unknown,
                        _ => Delivery::Accepted,
                    },
                    event: collection.map(|collection| event("Lunch", EventTime::DateTime(at(12)), collection)),
                }
            }
            Request::RespondToInvitation { .. } => {
                Response::Error { code: "error".into(), message: "This mail holds no invitation.".into() }
            }
            Request::Events { start, end } if start < end => Response::Events {
                events: vec![
                    event("Standup", EventTime::DateTime(start), "work".into()),
                    torro_pim::Event {
                        end: Some(EventTime::Date(day(13))),
                        ..event("Holiday", EventTime::Date(day(12)), "work".into())
                    },
                    torro_pim::Event {
                        transparency: torro_pim::Transparency::Transparent,
                        end: Some(EventTime::DateTime(end)),
                        ..event("Home office", EventTime::DateTime(start), "work".into())
                    },
                ],
            },
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
            // A mail of the account with a Message-ID; `m:bare` has none.
            Request::CreateTodoFromMail { account, id, collection, title, due } if id == "m:1" => Response::Todo {
                todo: torro_pim::Todo {
                    uid: "from-mail".into(),
                    collection,
                    href: Some("https://cloud.example.org/t/from-mail.ics".into()),
                    etag: Some("\"1\"".into()),
                    summary: title.unwrap_or_else(|| "Budget Q4".into()),
                    due,
                    priority: None,
                    completed: false,
                    parent: None,
                    description: None,
                    url: None,
                    categories: Vec::new(),
                    sort_order: None,
                    link: Some(torro_pim::model::Link::mail(&account, "<budget@example.org>")),
                },
            },
            Request::CreateTodoFromMail { id, .. } if id == "m:bare" => {
                Response::Error { code: "unsupported".into(), message: "the mail has no Message-ID".into() }
            }
            Request::CreateTodoFromMail { .. } => {
                Response::Error { code: "not-found".into(), message: "no such mail".into() }
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
            order: Order::OldestFirst,
        };
        let items = core.client().list_items(filter).await.expect("listed");
        assert_eq!(items.len(), 1);
        let said = format!("Some(ForMe)|agent:codex|Some(Open)|{}|Some(50)|oldest-first", at(7).to_rfc3339());
        assert_eq!(items[0].id, said);
        let all = core.client().list_items(ItemFilter::default()).await.expect("listed");
        assert_eq!(all[0].id, "None||None||None", "newest first is left out");
        assert!(all[0].untrusted, "the flag comes along");
    }

    #[tokio::test]
    async fn sources_are_listed() {
        let core = start(|request| async move { answer(request) }).await;
        let sources = core.client().list_sources().await.expect("listed");
        assert_eq!(sources, vec![SourceInfo { source: SourceRef::script("ci"), open: 2, muted: false, health: None }]);
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

    /// Task E8.6a: dismissing, the menu and muting each send their one request.
    #[tokio::test]
    async fn dismissing_and_muting_send_their_requests() {
        let core = start(|request| async move { answer(request) }).await;
        let client = core.client();
        assert_eq!(client.dismiss_item("a".into()).await.expect("dismissed").title, "dismissed true");
        assert_eq!(client.set_item_done("a".into(), true).await.expect("done").title, "dismissed false");
        let options = client.list_mute_options("m1".into()).await.expect("options");
        assert_eq!((options.len(), options[0].label.as_str()), (1, "m1"));
        let sender = MuteScope::Facet { key: torro_inbox::item::FacetKey::Sender, nth: 0 };
        let muted = client.mute_item("m1".into(), sender, Some(at(12)), MuteEffect::Info).await.expect("muted");
        assert_eq!(muted.moved, ["m1|Facet { key: Sender, nth: 0 }|true|Info"]);
    }

    /// Task E8.8a: the statistics are one request with the span.
    #[tokio::test]
    async fn stats_send_their_span() {
        let core = start(|request| async move { answer(request) }).await;
        let day = NaiveDate::from_ymd_opt(2026, 10, 13).expect("valid date");
        let stats = core.client().get_stats(StatsRange::Month, Some(day)).await.expect("stats");
        assert_eq!((stats.range, stats.until, stats.totals.new), (StatsRange::Month, day, 30));
        let today = core.client().get_stats(StatsRange::Week, None).await.expect("stats");
        assert_eq!((today.until, today.totals.new), (NaiveDate::default(), 7));
    }

    /// Task E8.5b: quiet is one request, an event of an entry sends its times as instants, and
    /// only calendars an event can be written to are offered.
    #[tokio::test]
    async fn quiet_and_events_send_their_requests() {
        let core = start(|request| async move { answer(request) }).await;
        let client = core.client();
        let quiet = client.get_quiet().await.expect("quiet").expect("it is quiet");
        assert_eq!((quiet.until, quiet.title.as_deref()), (at(15), Some("Call")));
        let calendars = client.list_calendars().await.expect("calendars");
        let names: Vec<&str> = calendars.iter().map(|calendar| calendar.name.as_str()).collect();
        assert_eq!(names, ["WORK", "EVENTS"], "no read-only share, no filter list, no list of tasks only");
        let made = client.create_event_from_item("i1".into(), "work".into(), Some("Call".into()), at(14), at(15));
        assert_eq!(made.await, Ok(()));
        let approval = client.create_event_from_item("a1".into(), "work".into(), None, at(14), at(15));
        assert_eq!(approval.await, Err(InboxError::Forbidden));
    }

    fn day(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("valid date")
    }

    /// An event of `collection` titled `title` from `start` for an hour (a timed one) or without end.
    fn event(title: &str, start: EventTime, collection: String) -> torro_pim::Event {
        torro_pim::Event {
            uid: format!("{title}@test"),
            collection,
            href: None,
            etag: None,
            summary: title.into(),
            start,
            end: match start {
                EventTime::DateTime(at) => Some(EventTime::DateTime(at + TimeDelta::hours(1))),
                EventTime::Date(_) => None,
            },
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
        }
    }

    /// Task E8.3c: an answer sends the calendar chosen and comes back as how the reply went; the
    /// events around an invitation come in instants, all-day ones from local midnight, with busy.
    #[tokio::test]
    async fn invitations_send_the_calendar_and_events_come_in_instants() {
        let core = start(|request| async move { answer(request) }).await;
        let client = core.client();
        let account = || "mail:anna@example.org".to_string();
        let accepted = client
            .respond_to_invitation(account(), "m:invite".into(), InvitationAnswer::Accept, Some("work".into()))
            .await;
        assert_eq!(accepted, Ok(Delivery::Accepted));
        let maybe = client.respond_to_invitation(account(), "m:invite".into(), InvitationAnswer::Tentative, None);
        assert_eq!(maybe.await, Ok(Delivery::Unknown));
        let none = client.respond_to_invitation(account(), "m:1".into(), InvitationAnswer::Decline, None).await;
        assert_eq!(none, Err(InboxError::Other("This mail holds no invitation.".into())));

        let events = client.list_events(at(9), at(11)).await.expect("events");
        let standup = &events[0];
        assert_eq!(
            (standup.title.as_str(), standup.start, standup.end, standup.busy),
            ("Standup", at(9), at(10), true)
        );
        let holiday = &events[1];
        assert_eq!((holiday.start, holiday.end), (local_midnight(day(12)), local_midnight(day(13))));
        assert!(!holiday.busy, "all day leaves the time free");
        assert!(!events[2].busy && events[2].end == at(11), "transparent is free");
        assert!(matches!(client.list_events(at(11), at(9)).await, Err(InboxError::Other(_))), "unexpected answer");
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

    /// Task E8.1b: a task of a folder's message carries its day as an all-day due date, and comes
    /// back in the inbox's model with the way back to the mail.
    #[tokio::test]
    async fn a_task_of_a_mail_sends_its_day_and_comes_back_as_the_entry_has_it() {
        let core = start(|request| async move { answer(request) }).await;
        let client = core.client();
        let work = TaskList { id: "work".into(), name: "Work".into() };
        let day = NaiveDate::from_ymd_opt(2026, 10, 16).expect("valid date");
        let account = "mail:anna@example.org".to_string();
        let task = client
            .create_todo_from_mail(account.clone(), "m:1".into(), work.clone(), None, Some(day))
            .await
            .expect("made");
        assert_eq!(
            task,
            ItemTask {
                uid: "from-mail".into(),
                collection: "work".into(),
                list: "Work".into(),
                title: "Budget Q4".into(),
                due: Some(TaskDue::Date(day)),
                completed: false,
                link: "mail:anna@example.org:budget@example.org".into(),
                mail_entry: None,
            }
        );
        let titled = client
            .create_todo_from_mail(account.clone(), "m:1".into(), work.clone(), Some("Answer Anna".into()), None)
            .await
            .expect("made");
        assert_eq!((titled.title.as_str(), titled.due), ("Answer Anna", None), "no day: no due date");
        let bare = client.create_todo_from_mail(account.clone(), "m:bare".into(), work.clone(), None, None).await;
        assert_eq!(bare, Err(InboxError::Unsupported), "no Message-ID");
        let gone = client.create_todo_from_mail(account, "m:gone".into(), work, None, None).await;
        assert_eq!(gone, Err(InboxError::NotFound));
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
            invite: None,
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
                order,
            } => Response::MailMessages {
                messages: vec![summary(
                    &format!(
                        "{}|{}{}",
                        query.unwrap_or_default(),
                        cursor.unwrap_or_default(),
                        order.map(|order| format!("|{}", order.word())).unwrap_or_default()
                    ),
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
        let search = |order| client.search_mail("f:inbox".into(), Some("offer".into()), 50, Some("c1".into()), order);
        let (page, next) = search(Order::NewestFirst).await.expect("a page");
        assert_eq!(
            (page[0].id.as_str(), page[0].folder.as_str(), next.as_deref()),
            ("offer|c1", "f:inbox", Some("next"))
        );
        let (oldest, _) = search(Order::OldestFirst).await.expect("a page");
        assert_eq!(oldest[0].id, "offer|c1|oldest-first", "the order goes along");
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
