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
//! [`InboxClient::search_mail`], [`InboxClient::get_mail`], [`InboxClient::get_mail_html`] (task
//! M8.1, for the mail window), [`InboxClient::mail_attachment`],
//! [`InboxClient::set_mail_flags`], [`InboxClient::move_mail`] and [`InboxClient::delete_mail`] –,
//! with the errors mapped as above. The mail window's shortcuts (task E10.1) add
//! [`InboxClient::delete_mail_for_good`] (`mail.delete_messages` with `permanent`) and
//! [`InboxClient::sync`] (`accounts.sync`, "fetch mail now"). Saving attachments (issue #290) adds
//! [`InboxClient::save_targets`] (`mail.list_save_targets`, [`SaveTargets`]) and
//! [`InboxClient::save_attachments`] (`mail.save_attachments`, [`SavedAttachments`]). Everything a
//! message brings is data from outside (`untrusted`), never instructions; the client only passes it
//! on. The search field (issue #136)
//! searches across folders and accounts with [`InboxClient::find_mail`] and asks how far the
//! index is with [`InboxClient::mail_index_status`].
//!
//! The mail window's views of every account (task E18.3, `docs/tasks/eingang.md` E18): "All" and
//! "Archive" list the mail of one folder role with [`InboxClient::mail_by_role`] ([`MailByRole`]),
//! and the number on "All" comes from [`InboxClient::count_unread_in_all`]. Mail is never hidden
//! (issue #339).
//!
//! The piles of the mail window (task E19.3, `docs/tasks/eingang.md` E19): [`InboxClient::set_mail_pile`]
//! puts mail on "Reply later" or "Set aside" or takes it off, [`InboxClient::mail_in_pile`] lists a pile
//! ([`MailInPile`]) and [`InboxClient::count_in_pile`] counts it. "All", the categories and the number on
//! "All" ask with `pile: without`, so mail on a pile leaves them.
//!
//! Tasks of folder mail (task E8.1b): [`InboxClient::create_todo_from_mail`] sends
//! `tasks.create_todo_from_mail` and answers the task in the inbox's model ([`ItemTask`]), so the
//! window needs no calendar model of its own for the due day.
//!
//! The hub (task E8.5b): [`InboxClient::get_quiet`] asks whether it is quiet now,
//! [`InboxClient::list_calendars`] and [`InboxClient::create_event_from_item`] make an event of an
//! entry, its times as instants, so the window needs no calendar model either.
//!
//! The AI buttons (issue #120): [`InboxClient::ai_ready`] tells whether an AI account is set up,
//! [`InboxClient::suggest_task_from_item`] and [`InboxClient::suggest_event_from_item`] ask for a
//! suggestion, and [`InboxClient::create_task_with`] and [`InboxClient::create_event_with`] make the
//! task or event with the note and place the person kept of it.
//!
//! Invitations (task E8.3c): [`InboxClient::respond_to_invitation`] sends
//! `mail.respond_to_invitation` with the calendar the window chose, and
//! [`InboxClient::list_events`] reads the events of the shown calendars around an invitation as
//! [`CalendarEvent`]s – title, instants and whether they make their time busy –, so the window can
//! say what the invitation overlaps without a calendar model.

use std::path::PathBuf;

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeDelta, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use torro_inbox::item::{ItemTask, MuteEffect, MuteOption, MuteScope, Muted, TaskDue};
use torro_inbox::source::TaskList;
use torro_inbox::{InboxError, InboxSource, Item, ItemFilter, Lane, SourceInfo, SourceRef};
use torro_mail::{
    Category, CategoryGroup, Flag, Folder, InvitationAnswer, MailPile, Message, MessageSummary, Order, Role, SaveTarget,
};
use torro_pim::EventTime;

use crate::assistant::EngineKind;
use crate::mail::{
    AttachmentPlan, Delivery, FailedFile, MailHighlight, MailIndexProgress, MailSort, MoveTarget, PileFilter, SaveTo,
    SavedFile,
};
use crate::{EventSuggestion, InboxStats, IpcError, Quiet, Request, Response, StatsRange, TaskSuggestion};

/// A search across mail as the window asks it ([`InboxClient::find_mail`], issue #136).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailFind {
    /// Only this account (`mail:<address>`); `None` with `folder` `None`: every account.
    pub account: Option<String>,
    /// Only this folder (an id of [`InboxClient::mail_folders`]); `None`: every folder.
    pub folder: Option<String>,
    /// Words and operators as typed (`from:anna "budget review" -draft`), at most 500 characters.
    pub query: String,
    /// By relevance or by date.
    pub sort: MailSort,
    /// Newest or oldest first, when `sort` is by date.
    pub order: Order,
    /// At most this many messages, 1 to 100.
    pub limit: u32,
    /// The cursor of the previous page, asked with the same query, sort and order.
    pub cursor: Option<String>,
}

/// The mail of one folder role across accounts as the mail window lists it
/// ([`InboxClient::mail_by_role`], task E18.3): "All" is [`Role::Inbox`] with `snoozed`
/// `Some(false)`, "Archive" [`Role::Archive`]. Sent as one `mail.search_messages` without a folder and with the query
/// `in:<role>`, so every account's folder of that role counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailByRole {
    /// The folders of this role, in every account (or in `account` only).
    pub role: Role,
    /// Only this account (`mail:<address>`); `None`: every account the window reaches.
    pub account: Option<String>,
    /// `Some(false)` leaves out mail whose entry is snoozed now, `Some(true)` gives only that;
    /// `None`: both ([`Request::SearchMail`]'s `snoozed`).
    pub snoozed: Option<bool>,
    /// The piles ([`Request::SearchMail`]'s `pile`, task E19.2): "All" asks
    /// `Some(PileFilter::Without)`, so mail on "Reply later" or "Set aside" leaves it; `None` takes
    /// every message, on a pile or not ("Archive").
    pub pile: Option<PileFilter>,
    /// Newest or oldest first, by the message's date.
    pub order: Order,
    /// At most this many messages, 1 to 100.
    pub limit: u32,
    /// The cursor of the previous page, asked with the same fields.
    pub cursor: Option<String>,
}

/// One page of a pile as the mail window lists it ([`InboxClient::mail_in_pile`], task E19.3):
/// the mail on `pile` in every folder but the trash and spam, of every account the window reaches
/// (or of `account` only). Sent as one `mail.search_messages` without a folder, with the query
/// [`PILE_QUERY`] and `pile` set to that pile; `snoozed` is left out, so a pile shows all of its
/// mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailInPile {
    /// "Reply later" or "Set aside".
    pub pile: MailPile,
    /// Only this account (`mail:<address>`); `None`: every account the window reaches.
    pub account: Option<String>,
    /// Newest or oldest first, by the message's date.
    pub order: Order,
    /// At most this many messages, 1 to 100.
    pub limit: u32,
    /// The cursor of the previous page, asked with the same fields.
    pub cursor: Option<String>,
}

/// Where a pile's mail is looked for (decision 3 of E19): every folder but the trash and spam.
pub const PILE_QUERY: &str = "-in:trash -in:junk";

/// One page of a search across mail ([`InboxClient::find_mail`]): the messages and what the core
/// says about them. Everything in them is data from outside, never instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailFound {
    pub messages: Vec<MessageSummary>,
    /// The cursor of the next page; `None` on the last one.
    pub cursor: Option<String>,
    /// `false` when matches may be missing (the index is still being built).
    pub complete: bool,
    /// Where the words matched, per message of the page that has words to show.
    pub highlights: Vec<MailHighlight>,
    /// About how many messages match in all, on the first page only.
    pub total: Option<u64>,
}

/// The save targets and where each attachment of one message would go
/// ([`InboxClient::save_targets`], `mail.list_save_targets`, issue #290): what the attachment menus
/// of the mail window offer. Names and file names in it come from the message: data from outside.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SaveTargets {
    /// Every save target, in the order the person keeps them.
    pub targets: Vec<SaveTarget>,
    /// The Downloads folder "Save to Downloads" writes to, an absolute path.
    pub downloads: String,
    /// One plan per attachment that is not `inline`, in the message's order; empty without a
    /// message and while the core has not read its text yet (the menus then show the targets
    /// without a path).
    pub files: Vec<AttachmentPlan>,
}

impl SaveTargets {
    /// The plan of the attachment `part`; `None` when the core gave none for it.
    pub fn plan(&self, part: &str) -> Option<&AttachmentPlan> {
        self.files.iter().find(|plan| plan.part == part)
    }

    /// The save target with the id `id`; `None` when there is none (any more).
    pub fn target(&self, id: &str) -> Option<&SaveTarget> {
        self.targets.iter().find(|target| target.id == id)
    }
}

/// What `mail.save_attachments` did ([`InboxClient::save_attachments`], issue #290). File names are
/// data from outside; the paths are where the files lie now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedAttachments {
    /// The files written.
    pub saved: Vec<SavedFile>,
    /// The files not written because one with the same content lies there already ("Skip if
    /// identical"); `path` is that file.
    pub skipped: Vec<SavedFile>,
    /// The attachments that could not be saved, with why; the others were saved all the same.
    pub failed: Vec<FailedFile>,
}

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
            sort: None,
            snoozed: None,
            categories: Vec::new(),
            pile: None,
        };
        match self.ask(request).await? {
            Response::MailMessages { messages, cursor, .. } => Ok((messages, cursor)),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.search_messages` across folders and accounts (issue #136): `find` says where (every
    /// folder of every account the window reaches, one account, or one folder), what (words and
    /// operators, as the user typed them) and how (by relevance, or by date in `find.order`).
    /// Answers the page with what the core says about it: whether matches may be missing, where
    /// the words matched and about how many match in all.
    pub async fn find_mail(&self, find: MailFind) -> Result<MailFound, InboxError> {
        let MailFind { account, folder, query, sort, order, limit, cursor } = find;
        let request = Request::SearchMail {
            account,
            folder,
            query: Some(query),
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit,
            cursor,
            // The order only counts by date; by relevance the core ignores it.
            order: asked(order).filter(|_| sort == MailSort::Date),
            sort: (sort != MailSort::default()).then_some(sort),
            snoozed: None,
            categories: Vec::new(),
            pile: None,
        };
        match self.ask(request).await? {
            Response::MailMessages { messages, cursor, complete, highlights, total_estimate, .. } => {
                Ok(MailFound { messages, cursor, complete, highlights, total: total_estimate })
            }
            other => Err(unexpected(other)),
        }
    }

    /// `mail.search_messages` for the mail of one folder role (task E18.3, [`MailByRole`]): no
    /// `folder`, the query `in:<role>` (the role's serde word: `in:inbox`, `in:archive`), with
    /// `account`, `snoozed`, `pile` (task E19.3), `order`, `limit` and `cursor` as `find`
    /// says; nothing filters
    /// by flags, time or category. Answers the page, newest first unless `find.order` says
    /// otherwise, and the cursor of the next one (`None` on the last page).
    pub async fn mail_by_role(&self, find: MailByRole) -> Result<(Vec<MessageSummary>, Option<String>), InboxError> {
        let MailByRole { role, account, snoozed, pile, order, limit, cursor } = find;
        let request = Request::SearchMail {
            account,
            folder: None,
            query: Some(in_role(role)),
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit,
            cursor,
            order: asked(order),
            sort: None,
            snoozed,
            categories: Vec::new(),
            pile,
        };
        match self.ask(request).await? {
            Response::MailMessages { messages, cursor, .. } => Ok((messages, cursor)),
            other => Err(unexpected(other)),
        }
    }

    /// How much unread mail "All" holds (task E18.3, decision 4 of E18): the search of "All" –
    /// [`Role::Inbox`] with `snoozed` `Some(false)` and, since task E19.3, `pile:
    /// without` (mail on a pile is not in "All") – with `unread: Some(true)` and `limit: 1`, for
    /// `account` or every account the window reaches. Answers the core's `total_estimate`, 0 when it
    /// sends none.
    pub async fn count_unread_in_all(&self, account: Option<String>) -> Result<u64, InboxError> {
        let request = Request::SearchMail {
            account,
            folder: None,
            query: Some(in_role(Role::Inbox)),
            unread: Some(true),
            flagged: None,
            since: None,
            before: None,
            limit: 1,
            cursor: None,
            order: None,
            sort: None,
            snoozed: Some(false),
            categories: Vec::new(),
            pile: Some(PileFilter::Without),
        };
        match self.ask(request).await? {
            Response::MailMessages { total_estimate, .. } => Ok(total_estimate.unwrap_or(0)),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.set_mail_pile` (task E19.2): puts the mail messages `ids` (`m:…`, 1 to 100) on `pile`
    /// – "Reply later" or "Set aside", moving them from the other one –, or takes them off with
    /// `None` (also the undo of putting them there). `since` puts them back with the time they had
    /// on their pile ([`torro_mail::MessageSummary::pile_since`]): the undo of taking them off or
    /// moving them, so a reply of one's own sent meanwhile still takes them off "Reply later"; `None`
    /// counts as now. Nothing goes to the server. [`InboxError::NotFound`] when one of them lies in
    /// an account the window may not see; nothing changes then.
    pub async fn set_mail_pile(
        &self,
        ids: Vec<String>,
        pile: Option<MailPile>,
        since: Option<DateTime<Utc>>,
    ) -> Result<(), InboxError> {
        self.ask_ok(Request::SetMailPile { ids, pile, since }).await
    }

    /// One page of a pile (task E19.3, [`MailInPile`]): `mail.search_messages` without a folder,
    /// the query [`PILE_QUERY`] (every folder but the trash and spam), `pile` set to `find.pile`,
    /// with `account`, `order`, `limit` and `cursor` as `find` says; `snoozed`, flags,
    /// time and category filter nothing. Answers the page and the cursor of the next one (`None`
    /// on the last page).
    pub async fn mail_in_pile(&self, find: MailInPile) -> Result<(Vec<MessageSummary>, Option<String>), InboxError> {
        let MailInPile { pile, account, order, limit, cursor } = find;
        match self.ask(pile_search(pile, account, limit, cursor, asked(order))).await? {
            Response::MailMessages { messages, cursor, .. } => Ok((messages, cursor)),
            other => Err(unexpected(other)),
        }
    }

    /// How much mail lies on `pile` (task E19.3, the numbers in the mail window's sidebar): the
    /// search of [`InboxClient::mail_in_pile`] with `limit: 1`, for `account` or every account the
    /// window reaches – read and unread alike. Answers the core's `total_estimate`, 0 when it sends
    /// none.
    pub async fn count_in_pile(&self, pile: MailPile, account: Option<String>) -> Result<u64, InboxError> {
        match self.ask(pile_search(pile, account, 1, None, None)).await? {
            Response::MailMessages { total_estimate, .. } => Ok(total_estimate.unwrap_or(0)),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.index_status`: how far the core's search index of each mail account is (issue #136),
    /// in the order of the accounts.
    pub async fn mail_index_status(&self) -> Result<Vec<MailIndexProgress>, InboxError> {
        match self.ask(Request::MailIndexStatus { account: None }).await? {
            Response::MailIndexStatus { accounts } => Ok(accounts),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.get_message` with its text (`include_body`); reading it does not mark it read.
    pub async fn get_mail(&self, id: String) -> Result<Message, InboxError> {
        match self.ask(Request::GetMail { id, include_body: true, include_html: false }).await? {
            Response::MailMessage { message } => Ok(message),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.get_message` with its text and its cleaned HTML (`include_body`, `include_html`, task
    /// M8.1), for showing the mail as it was written. `Message::html` is `None` when the mail has no
    /// HTML or it is too large: then show `Message::text`. Images `cid:<id>` name the attachment
    /// whose `content_id` is `<id>` ([`InboxClient::mail_attachment`] fetches it). Reading it does
    /// not mark it read.
    pub async fn get_mail_html(&self, id: String) -> Result<Message, InboxError> {
        match self.ask(Request::GetMail { id, include_body: true, include_html: true }).await? {
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

    /// `mail.list_save_targets` (issue #290): the save targets, the Downloads folder and – with the
    /// message `id` – where each of its attachments would go ([`SaveTargets`]). `NotFound` for a
    /// message the window may not see.
    pub async fn save_targets(&self, id: Option<String>) -> Result<SaveTargets, InboxError> {
        match self.ask(Request::ListSaveTargets { id }).await? {
            Response::SaveTargets { targets, downloads, files } => Ok(SaveTargets { targets, downloads, files }),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.save_attachments` (issue #290): saves the attachments `parts` of the message `id` (none:
    /// every one that is not `inline`) where `to` says; the window, holding the app key, may send
    /// `to: file` and `folder` with the path a person chose in the system's dialog. Answers what was
    /// saved, skipped and what failed ([`SavedAttachments`]); `NotFound` for an unknown message,
    /// part or target, or a target whose folder is missing.
    pub async fn save_attachments(
        &self,
        id: String,
        parts: Vec<String>,
        to: SaveTo,
    ) -> Result<SavedAttachments, InboxError> {
        match self.ask(Request::SaveMailAttachments { id, parts, to }).await? {
            Response::SavedAttachments { saved, skipped, failed } => Ok(SavedAttachments { saved, skipped, failed }),
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

    /// `mail.delete_messages` for good (task E10.1, "Delete permanently" after the window asked): the
    /// core removes only messages that lie in the trash, so the messages `ids` go there first
    /// (`permanent: false`; those in it already stay) and are then removed (`permanent: true`). They
    /// keep their ids when they are moved. Nothing brings them back; when the first step fails,
    /// nothing is removed, and when the second fails they stay in the trash.
    pub async fn delete_mail_for_good(&self, ids: Vec<String>) -> Result<(), InboxError> {
        self.ask_ok(Request::DeleteMail { ids: ids.clone(), permanent: false }).await?;
        self.ask_ok(Request::DeleteMail { ids, permanent: true }).await
    }

    /// `accounts.sync` (task E10.1, the shortcut "fetch"): the core fetches mail, calendars and tasks
    /// from the servers of every account now. Answers the first account's failure, if any; the
    /// others are fetched all the same.
    pub async fn sync(&self) -> Result<(), InboxError> {
        self.ask_ok(Request::Sync).await
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
        self.create_event_with(id, calendar, EventFields { title, start, end, location: None, notes: None }).await
    }

    /// `inbox.create_event_from_item` with everything the dialog of an AI's suggestion holds (issue
    /// #120): as [`InboxClient::create_event_from_item`], with the `location` and the `notes` put
    /// before the entry's text (`None` or blank: left out). Errors as there.
    pub async fn create_event_with(&self, id: String, calendar: String, fields: EventFields) -> Result<(), InboxError> {
        let EventFields { title, start, end, location, notes } = fields;
        let (start, end) = (EventTime::DateTime(start), EventTime::DateTime(end));
        let request = Request::CreateEventFromItem { id, collection: calendar, title, start, end, location, notes };
        match self.ask(request).await? {
            Response::Event { .. } => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.create_task_from_item` with a note before the entry's text (issue #120, the dialog of
    /// an AI's suggestion; `None` or blank: left out) and the day as an all-day due date. The core
    /// answers with the new task, which the app does not need. Errors as for the other calls.
    pub async fn create_task_with(
        &self,
        id: String,
        list: String,
        title: Option<String>,
        due: Option<NaiveDate>,
        notes: Option<String>,
    ) -> Result<(), InboxError> {
        let due = due.map(EventTime::Date);
        match self.ask(Request::CreateTaskFromItem { id, collection: list, title, due, notes }).await? {
            Response::Todo { .. } => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    /// Whether an AI is set up for the AI buttons (issue #120): `assistant.list_engines` lists an
    /// AI account ([`EngineKind::Ai`]) without a known problem. The suggestions take only AI
    /// accounts, so a coding agent alone does not count. `false` when the core cannot be asked.
    pub async fn ai_ready(&self) -> bool {
        match self.ask(Request::ListEngines).await {
            Ok(Response::Engines { engines }) => {
                engines.iter().any(|engine| engine.kind == EngineKind::Ai && engine.problem.is_none())
            }
            _ => false,
        }
    }

    /// `inbox.suggest_task_from_item` (issue #120, "Fill in with AI" in the dialog of "Make task"):
    /// what the AI suggests as a task of the entry `id`, its list one of `lists` (the names the
    /// dialog offers); nothing is saved. Errors as mapped above: [`InboxError::NotFound`] for an entry
    /// the app may not see, [`InboxError::Offline`] while the core is not running, and
    /// [`InboxError::Other`] with the core's words for everything about the AI (none set up, its
    /// server offline, an answer that is no suggestion).
    pub async fn suggest_task_from_item(&self, id: String, lists: Vec<String>) -> Result<TaskSuggestion, InboxError> {
        match self.ask(Request::SuggestTaskFromItem { id, lists }).await? {
            Response::TaskSuggestion { suggestion } => Ok(suggestion),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.suggest_event_from_item` (issue #120, "Fill in with AI" in the dialog of "Make event"): what the AI suggests as an event of
    /// the entry `id`; nothing is saved. Errors as for [`InboxClient::suggest_task_from_item`].
    pub async fn suggest_event_from_item(&self, id: String) -> Result<EventSuggestion, InboxError> {
        match self.ask(Request::SuggestEventFromItem { id }).await? {
            Response::EventSuggestion { suggestion } => Ok(suggestion),
            other => Err(unexpected(other)),
        }
    }
}

/// What [`InboxClient::create_event_with`] makes of an entry besides its calendar (issue #120): the
/// title (`None` or blank: the entry's), the instants it starts and ends, where, and a note before the
/// entry's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventFields {
    pub title: Option<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub location: Option<String>,
    pub notes: Option<String>,
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

/// The query naming the folders of `role` in every account: `in:` with the role's serde word
/// (`in:inbox`, `in:archive`), which `mail.search_messages` reads as that role.
fn in_role(role: Role) -> String {
    let word = match role {
        Role::Inbox => "inbox",
        Role::Drafts => "drafts",
        Role::Sent => "sent",
        Role::Archive => "archive",
        Role::Junk => "junk",
        Role::Trash => "trash",
    };
    format!("in:{word}")
}

/// The `mail.search_messages` of a pile ([`InboxClient::mail_in_pile`], [`InboxClient::count_in_pile`]):
/// no folder, [`PILE_QUERY`], only the mail on `pile`; nothing else filters.
fn pile_search(
    pile: MailPile,
    account: Option<String>,
    limit: u32,
    cursor: Option<String>,
    order: Option<Order>,
) -> Request {
    Request::SearchMail {
        account,
        folder: None,
        query: Some(PILE_QUERY.to_owned()),
        unread: None,
        flagged: None,
        since: None,
        before: None,
        limit,
        cursor,
        order,
        sort: None,
        snoozed: None,
        categories: Vec::new(),
        pile: Some(only(pile)),
    }
}

/// The search filter that gives only the mail on `pile`.
fn only(pile: MailPile) -> PileFilter {
    match pile {
        MailPile::ReplyLater => PileFilter::ReplyLater,
        MailPile::SetAside => PileFilter::SetAside,
    }
}

/// An answer that is not the variant the request asks for.
fn unexpected(response: Response) -> InboxError {
    InboxError::Other(format!("unexpected answer: {response:?}"))
}

impl InboxClient {
    /// `inbox.list_categories` (task E12.8): the catalogue with how much unread mail each
    /// category holds, over every account this window may read.
    pub async fn list_categories(&self) -> Result<Vec<CategoryInfo>, InboxError> {
        self.list_categories_for(None).await
    }

    /// Category counts for one mail account, or all accounts when `account` is `None`.
    pub async fn list_categories_for(&self, account: Option<String>) -> Result<Vec<CategoryInfo>, InboxError> {
        match self.ask(Request::ListCategories { account }).await? {
            Response::Categories { categories, .. } => Ok(categories),
            other => Err(unexpected(other)),
        }
    }

    /// `mail.search_messages` filtered by `categories` (task E12.8), over every account, without the
    /// mail on a pile (`pile: without`, task E19.3).
    pub async fn search_by_category(
        &self,
        categories: Vec<torro_mail::Category>,
        limit: u32,
        cursor: Option<String>,
        order: Order,
    ) -> Result<(Vec<MessageSummary>, Option<String>), InboxError> {
        let request = Request::SearchMail {
            account: None,
            folder: None,
            query: None,
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit,
            cursor,
            order: asked(order),
            sort: None,
            snoozed: None,
            categories,
            // Task E19.3: mail on a pile leaves the categories as it leaves "All" (decision 3 of E19).
            pile: Some(PileFilter::Without),
        };
        match self.ask(request).await? {
            Response::MailMessages { messages, cursor, .. } => Ok((messages, cursor)),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.get_categorization` (task E12.8): how the sorting of mail by a model stands.
    pub async fn categorization(&self) -> Result<Categorization, InboxError> {
        match self.ask(Request::GetCategorization).await? {
            Response::Categorization { categorization } => Ok(categorization),
            other => Err(unexpected(other)),
        }
    }

    /// Count the mail selected for a persistent categorization job.
    pub async fn preview_category_job(&self, scope: CategoryJobScope) -> Result<CategoryJobPreview, InboxError> {
        match self.ask(Request::PreviewCategoryJob { scope }).await? {
            Response::CategoryJobPreview { preview } => Ok(preview),
            other => Err(unexpected(other)),
        }
    }

    /// Start a persistent categorization run over a chosen scope.
    pub async fn start_category_job(&self, scope: CategoryJobScope) -> Result<CategoryJob, InboxError> {
        match self.ask(Request::StartCategoryJob { scope }).await? {
            Response::CategoryJob { job: Some(job) } => Ok(job),
            other => Err(unexpected(other)),
        }
    }

    /// The latest categorization job, or one by ID.
    pub async fn category_job(&self, id: Option<String>) -> Result<Option<CategoryJob>, InboxError> {
        match self.ask(Request::GetCategoryJob { id }).await? {
            Response::CategoryJob { job } => Ok(job),
            other => Err(unexpected(other)),
        }
    }

    /// Pause, resume or cancel a persistent job.
    pub async fn control_category_job(
        &self,
        id: String,
        action: CategoryJobControl,
    ) -> Result<CategoryJob, InboxError> {
        match self.ask(Request::ControlCategoryJob { id, action }).await? {
            Response::CategoryJob { job: Some(job) } => Ok(job),
            other => Err(unexpected(other)),
        }
    }

    /// Count all category assignments before a global reset.
    pub async fn preview_clear_categories(&self) -> Result<CategoryClearPreview, InboxError> {
        match self.ask(Request::PreviewClearCategories).await? {
            Response::CategoryClearPreview { preview } => Ok(preview),
            other => Err(unexpected(other)),
        }
    }

    /// Remove every category assignment in one resumable job.
    pub async fn clear_categories(&self, disable_automatic: bool) -> Result<Categorization, InboxError> {
        match self.ask(Request::ClearCategories { all_accounts: true, disable_automatic }).await? {
            Response::Categorization { categorization } => Ok(categorization),
            other => Err(unexpected(other)),
        }
    }

    /// Pause, resume or cancel an all-account category reset.
    pub async fn control_clear_categories(
        &self,
        id: String,
        action: CategoryJobControl,
    ) -> Result<Categorization, InboxError> {
        match self.ask(Request::ControlClearCategories { id, action }).await? {
            Response::Categorization { categorization } => Ok(categorization),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.set_item_category` (task E12.8): files the mail by hand, and with `remember` keeps
    /// it for its sender.
    pub async fn set_mail_category(
        &self,
        id: String,
        category: torro_mail::Category,
        remember: bool,
    ) -> Result<Item, InboxError> {
        match self.ask(Request::SetItemCategory { id, category, remember }).await? {
            Response::ItemCategory { item, .. } => Ok(item),
            other => Err(unexpected(other)),
        }
    }

    /// `inbox.classify_item` (task E12.8): asks the decision model again.
    pub async fn classify_mail(&self, id: String) -> Result<Item, InboxError> {
        match self.ask(Request::ClassifyItem { id }).await? {
            Response::ItemCategory { item, .. } => Ok(item),
            other => Err(unexpected(other)),
        }
    }
}

impl InboxSource for InboxClient {
    /// `inbox.list_items` with the fields of `filter`; the core sorts, wakes and cuts.
    async fn list_items(&self, filter: ItemFilter) -> Result<Vec<Item>, InboxError> {
        let ItemFilter { lane, source, state, since, limit, order, dismissed, categories } = filter;
        let asking =
            Request::ListItems { lane, source, state, since, limit, order: asked(order), dismissed, categories };
        match self.ask(asking).await? {
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

    /// `inbox.snooze_item` with the follow-up's condition `unless_reply` (task E19.3) and
    /// `keep_open` (review of #315).
    async fn snooze_item(
        &self,
        id: String,
        until: Option<DateTime<Utc>>,
        unless_reply: bool,
        keep_open: bool,
    ) -> Result<Item, InboxError> {
        self.ask_item(Request::SnoozeItem { id, until, unless_reply, keep_open }).await
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

    /// `inbox.remove_rule` (also "Undo" after muting).
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
        InboxClient::create_task_with(self, id, list, title, due, None).await
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
    use torro_inbox::{Kind, State, StateFilter};

    use super::*;

    use crate::{client, server};

    /// Issue #290: the plans and targets of `mail.list_save_targets` are found by part and id.
    #[test]
    fn save_targets_find_their_plans_and_targets() {
        let spec = torro_mail::SaveTargetSpec::new("Invoices", "/home/anna/Invoices");
        let plan = AttachmentPlan {
            part: "2".into(),
            file_name: "invoice.pdf".into(),
            places: Vec::new(),
            suggested: Vec::new(),
            sure: None,
        };
        let saving = SaveTargets {
            targets: vec![SaveTarget { id: "t1".into(), spec }],
            downloads: "/home/anna/Downloads".into(),
            files: vec![plan],
        };
        assert_eq!(saving.plan("2").map(|plan| plan.file_name.as_str()), Some("invoice.pdf"));
        assert!(saving.plan("3").is_none());
        assert_eq!(saving.target("t1").map(|target| target.spec.name.as_str()), Some("Invoices"));
        assert!(saving.target("gone").is_none());
    }

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
            reason: None,
            category: None,
            follow_up: None,
        }
    }

    /// Canned answers that echo what arrived, so the tests see the arguments.
    fn answer(request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "test".into(), api: crate::API_VERSION },
            // The id names the filter that arrived.
            Request::ListItems { lane, source, state, since, limit, order, dismissed, .. } => Response::Items {
                items: vec![item(&format!(
                    "{lane:?}|{}|{state:?}|{}|{limit:?}{}{}",
                    source.map(|s| s.key()).unwrap_or_default(),
                    since.map(|s| s.to_rfc3339()).unwrap_or_default(),
                    order.map(|order| format!("|{}", order.word())).unwrap_or_default(),
                    dismissed.map(|dismissed| format!("|dismissed={dismissed}")).unwrap_or_default()
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
            Request::SnoozeItem { id, until, .. } => Response::Item {
                item: Item { state: until.map_or(State::Seen, |until| State::Snoozed { until }), ..item(&id) },
            },
            Request::SetItemLane { id, lane, remember } => {
                Response::Item { item: Item { lane, title: format!("remember {remember}"), ..item(&id) } }
            }
            Request::MuteSource { source, muted: true } if source == SourceRef::script("ci") => Response::Ok,
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
            // Issue #120: the place and the note come back in the event, so the tests see them.
            Request::CreateEventFromItem { id, collection, title, start, end, location, notes }
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
                        location,
                        description: notes,
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
                    },
                    reset: 0,
                }
            }
            // Issue #120: one whose note says which arrived.
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
            Request::CreateTaskFromItem { id, collection, title, due, notes } if id == "i1" => Response::Todo {
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
                    description: notes,
                    url: None,
                    categories: Vec::new(),
                    sort_order: None,
                    link: None,
                },
            },
            // Issue #120: the note must arrive; a task without one for `i2` is refused.
            Request::CreateTaskFromItem { id, notes: Some(notes), .. } if id == "i2" && notes == "Bring the slides" => {
                Response::Todo {
                    todo: torro_pim::Todo {
                        uid: "noted".into(),
                        collection: "work".into(),
                        href: None,
                        etag: None,
                        summary: "Prepare".into(),
                        due: None,
                        priority: None,
                        completed: false,
                        parent: None,
                        description: Some(notes),
                        url: None,
                        categories: Vec::new(),
                        sort_order: None,
                        link: None,
                    },
                }
            }
            Request::ListEngines => Response::Engines {
                engines: vec![
                    crate::assistant::Engine {
                        id: "claude-code".into(),
                        kind: EngineKind::Agent,
                        name: "Claude Code".into(),
                        problem: None,
                        models: Vec::new(),
                    },
                    crate::assistant::Engine {
                        id: "ai:openrouter@openrouter.ai".into(),
                        kind: EngineKind::Ai,
                        name: "OpenRouter".into(),
                        problem: Some("no key".into()),
                        models: Vec::new(),
                    },
                ],
            },
            Request::SuggestTaskFromItem { id, lists } if id == "i1" => Response::TaskSuggestion {
                suggestion: TaskSuggestion {
                    title: "Send the report".into(),
                    notes: None,
                    due: Some(EventTime::Date(day(16))),
                    list: lists.into_iter().next(),
                    engine: "ai:ollama@localhost:11434".into(),
                    model: "qwen3:8b".into(),
                },
            },
            Request::SuggestEventFromItem { id } if id == "i1" => Response::EventSuggestion {
                suggestion: EventSuggestion {
                    title: "Lunch with Kim".into(),
                    start: Some(EventTime::DateTime(at(12))),
                    end: None,
                    location: Some("Café".into()),
                    attendees: vec!["Kim".into()],
                    engine: "ai:ollama@localhost:11434".into(),
                    model: "qwen3:8b".into(),
                },
            },
            Request::SuggestTaskFromItem { .. } | Request::SuggestEventFromItem { .. } => {
                Response::Error { code: "not-connected".into(), message: "Set up an AI first.".into() }
            }
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
            dismissed: Some(false),
            categories: Vec::new(),
        };
        let items = core.client().list_items(filter).await.expect("listed");
        assert_eq!(items.len(), 1);
        let said =
            format!("Some(ForMe)|agent:codex|Some(Open)|{}|Some(50)|oldest-first|dismissed=false", at(7).to_rfc3339());
        assert_eq!(items[0].id, said);
        let all = core.client().list_items(ItemFilter::default()).await.expect("listed");
        assert_eq!(all[0].id, "None||None||None", "newest first is left out");
        assert!(all[0].untrusted, "the flag comes along");
    }

    /// Task E10.1: "Delete permanently" moves into the trash first, then removes for good; "fetch" is
    /// `accounts.sync`.
    #[tokio::test]
    async fn deleting_for_good_goes_through_the_trash_and_fetching_syncs() {
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let core = start({
            let asked = asked.clone();
            move |request| {
                let asked = asked.clone();
                async move {
                    if let Ok(mut asked) = asked.lock() {
                        asked.push(request.clone());
                    }
                    match request {
                        Request::Ping | Request::Sync | Request::DeleteMail { .. } => Response::Ok,
                        other => Response::Error { code: "error".into(), message: format!("{other:?}") },
                    }
                }
            }
        })
        .await;
        let client = core.client();
        client.delete_mail_for_good(vec!["m:1".into()]).await.expect("deleted");
        client.sync().await.expect("synced");
        let asked: Vec<Request> =
            asked.lock().expect("the list").iter().filter(|r| !matches!(r, Request::Ping)).cloned().collect();
        assert_eq!(
            asked,
            [
                Request::DeleteMail { ids: vec!["m:1".into()], permanent: false },
                Request::DeleteMail { ids: vec!["m:1".into()], permanent: true },
                Request::Sync,
            ]
        );
        let refused = start(|request| async move {
            match request {
                Request::Ping => Response::Ok,
                _ => Response::Error { code: "not-found".into(), message: "no such message".into() },
            }
        })
        .await;
        let gone = refused.client().delete_mail_for_good(vec!["m:gone".into()]).await;
        assert!(matches!(gone, Err(InboxError::NotFound)), "{gone:?}");
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
        let snoozed = client.snooze_item("b".into(), Some(at(15)), false, false).await.expect("snoozed");
        assert_eq!((snoozed.id.as_str(), snoozed.state), ("b", State::Snoozed { until: at(15) }));
        assert_eq!(client.snooze_item("b".into(), None, false, false).await.expect("back").state, State::Seen);
        let moved = client.set_item_lane("c".into(), Lane::Info, true).await.expect("moved");
        assert_eq!((moved.lane, moved.title.as_str()), (Lane::Info, "remember true"));
        client.mute_source(SourceRef::script("ci"), true).await.expect("muted");
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
        }
    }

    /// Task E8.3c: an answer sends the calendar chosen and comes back as how the reply went; the
    /// events around an invitation come in instants, all-day ones from local midnight, with busy.
    /// Issue #120: the AI buttons ask whether an AI account is set up (a coding agent or an account
    /// with a problem does not count), ask for suggestions, and save the note and the place.
    #[tokio::test]
    async fn suggestions_and_their_notes_go_through_the_core() {
        let core = start(|request| async move { answer(request) }).await;
        let client = core.client();
        assert!(!client.ai_ready().await, "only an agent and an account with a problem");
        let task = client.suggest_task_from_item("i1".into(), vec!["Work".into()]).await.expect("a suggestion");
        assert_eq!((task.title.as_str(), task.due), ("Send the report", Some(EventTime::Date(day(16)))));
        assert_eq!(task.list.as_deref(), Some("Work"), "the lists go along");
        let event = client.suggest_event_from_item("i1".into()).await.expect("a suggestion");
        assert_eq!((event.location.as_deref(), event.attendees.len()), (Some("Café"), 1));
        let none = client.suggest_task_from_item("i9".into(), Vec::new()).await;
        assert_eq!(none, Err(InboxError::Other("Set up an AI first.".into())));
        let noted = client.create_task_with("i2".into(), "work".into(), None, None, Some("Bring the slides".into()));
        assert_eq!(noted.await, Ok(()));
        let fields = EventFields {
            title: Some("Call".into()),
            start: at(14),
            end: at(15),
            location: Some("Room 2".into()),
            notes: Some("Agenda".into()),
        };
        assert_eq!(client.create_event_with("i1".into(), "work".into(), fields).await, Ok(()));
    }

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
            back: None,
            pile: None,
            category: None,
            untrusted: true,
            pile_since: None,
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
                sort: None,
                snoozed: None,
                ..
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
                complete: true,
                incomplete_reason: None,
                highlights: Vec::new(),
                total_estimate: None,
            },
            // A search across folders (issue #136): what arrived goes back in the id.
            Request::SearchMail {
                account, folder: None, query: Some(query), limit: 20, cursor, order, sort, ..
            } => {
                let id = format!(
                    "{}|{query}|{sort:?}|{order:?}|{}",
                    account.unwrap_or_default(),
                    cursor.unwrap_or_default()
                );
                let hit = crate::mail::HighlightSpan { text: "Offer".into(), hit: true };
                Response::MailMessages {
                    highlights: vec![MailHighlight { id: id.clone(), subject: vec![hit], snippet: Vec::new() }],
                    messages: vec![summary(&id, "f:inbox")],
                    cursor: None,
                    source: crate::mail::MailSource::Cache,
                    complete: false,
                    incomplete_reason: Some(crate::mail::IncompleteReason::Indexing),
                    total_estimate: Some(7),
                }
            }
            Request::MailIndexStatus { account: None } => Response::MailIndexStatus {
                accounts: vec![MailIndexProgress {
                    account: "mail:anna@example.org".into(),
                    messages: 18,
                    indexed: 12,
                    text_unavailable: 0,
                    state: crate::mail::IndexState::Indexing,
                }],
            },
            Request::GetMail { id, include_body: true, include_html } => Response::MailMessage {
                message: Message {
                    summary: summary(&id, "f:inbox"),
                    cc: Vec::new(),
                    reply_to: Vec::new(),
                    text: Some("Hello".into()),
                    from_html: false,
                    html: include_html.then(|| "<p>Hello</p>".into()),
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
        // Issue #136: across folders, with the sort; the order only by date.
        let find = |account: Option<&str>, sort, order| MailFind {
            account: account.map(str::to_owned),
            folder: None,
            query: "from:anna offer".into(),
            sort,
            order,
            limit: 20,
            cursor: Some("c2".into()),
        };
        let found = client.find_mail(find(None, MailSort::Relevance, Order::OldestFirst)).await.expect("found");
        assert_eq!(found.messages[0].id, "|from:anna offer|Some(Relevance)|None|c2", "no order by relevance");
        assert_eq!((found.complete, found.total, found.highlights.len()), (false, Some(7), 1));
        let found = client.find_mail(find(Some("mail:anna@example.org"), MailSort::Date, Order::OldestFirst));
        let found = found.await.expect("found");
        assert_eq!(found.messages[0].id, "mail:anna@example.org|from:anna offer|None|Some(OldestFirst)|c2");
        let index = client.mail_index_status().await.expect("the index");
        assert_eq!((index[0].indexed, index[0].messages), (12, 18));
        let message = client.get_mail("m:1".into()).await.expect("the message");
        assert_eq!((message.summary.id.as_str(), message.text.as_deref()), ("m:1", Some("Hello")));
        assert_eq!(message.html, None, "only asked for by the mail window");
        let message = client.get_mail_html("m:1".into()).await.expect("the message");
        assert_eq!((message.text.as_deref(), message.html.as_deref()), (Some("Hello"), Some("<p>Hello</p>")));
        let path = client.mail_attachment("m:1".into(), "2".into()).await.expect("fetched");
        assert_eq!(path, PathBuf::from("/attachments/m:1-2.pdf"));
        client.set_mail_flags(vec!["m:1".into()], vec![Flag::Flagged], vec![Flag::Seen]).await.expect("flagged");
        let archive = MoveTarget::Role(crate::mail::MoveRole::Archive);
        client.move_mail(vec!["m:1".into()], archive).await.expect("archived");
        client.delete_mail(vec!["m:1".into(), "m:2".into()]).await.expect("into the trash, not for good");
        assert_eq!(client.delete_mail(vec!["m:3".into()]).await, Err(InboxError::NotFound));
    }

    /// Answers the searches by role (task E18.3) with what arrived: a
    /// search echoes its fields in the id of its one message, its `total_estimate` is 4 for an
    /// account and missing without one.
    fn role_answer(request: Request) -> Response {
        match request {
            Request::SearchMail {
                account,
                folder: None,
                query,
                unread,
                flagged: None,
                since: None,
                before: None,
                limit,
                cursor,
                order,
                sort: None,
                snoozed,
                categories,
                pile,
            } if categories.is_empty() => {
                // The pile only when there is one (task E19.3), so the ids before it stay as they were.
                let id = format!(
                    "{}|{}|{unread:?}|{snoozed:?}|{limit}|{}|{order:?}{}",
                    account.as_deref().unwrap_or_default(),
                    query.unwrap_or_default(),
                    cursor.unwrap_or_default(),
                    pile.map(|pile| format!("|{pile:?}")).unwrap_or_default(),
                );
                Response::MailMessages {
                    total_estimate: account.map(|_| 4),
                    messages: vec![summary(&id, "f:inbox")],
                    cursor: Some("next".into()),
                    source: crate::mail::MailSource::Cache,
                    complete: true,
                    incomplete_reason: None,
                    highlights: Vec::new(),
                }
            }
            other => Response::Error { code: "error".into(), message: format!("unexpected: {other:?}") },
        }
    }

    #[tokio::test]
    async fn mail_by_role_and_counting_send_their_fields() {
        let core = start(|request| async move { role_answer(request) }).await;
        let client = core.client();
        let all = MailByRole {
            role: torro_mail::Role::Inbox,
            account: None,
            snoozed: Some(false),
            pile: None,
            order: Order::NewestFirst,
            limit: 50,
            cursor: None,
        };
        let (page, next) = client.mail_by_role(all.clone()).await.expect("a page");
        assert_eq!(page[0].id, "|in:inbox|None|Some(false)|50||None", "no folder, the role as query");
        assert_eq!(next.as_deref(), Some("next"));
        let archive = MailByRole {
            role: torro_mail::Role::Archive,
            account: Some("mail:anna@example.org".into()),
            snoozed: None,
            pile: None,
            order: Order::OldestFirst,
            limit: 20,
            cursor: Some("c1".into()),
        };
        let (page, _) = client.mail_by_role(archive).await.expect("a page");
        assert_eq!(page[0].id, "mail:anna@example.org|in:archive|None|None|20|c1|Some(OldestFirst)");
        // Task E19.3: "All" leaves out the mail on a pile.
        let without = MailByRole { pile: Some(PileFilter::Without), ..all };
        let (page, _) = client.mail_by_role(without).await.expect("a page");
        assert_eq!(page[0].id, "|in:inbox|None|Some(false)|50||None|Without", "the pile goes along");

        // The number on "All": the same search, unread only, one message, its estimate.
        assert_eq!(client.count_unread_in_all(Some("mail:anna@example.org".into())).await, Ok(4));
        assert_eq!(client.count_unread_in_all(None).await, Ok(0), "no estimate counts as none");
        let core_seen = start(|request| async move {
            match request {
                Request::SearchMail {
                    query: Some(query),
                    unread: Some(true),
                    limit: 1,
                    cursor: None,
                    snoozed: Some(false),
                    folder: None,
                    pile: Some(PileFilter::Without),
                    ..
                } if query == "in:inbox" => Response::MailMessages {
                    messages: Vec::new(),
                    cursor: None,
                    source: crate::mail::MailSource::Cache,
                    complete: true,
                    incomplete_reason: None,
                    highlights: Vec::new(),
                    total_estimate: Some(9),
                },
                _ => Response::Error { code: "error".into(), message: "not the count".into() },
            }
        })
        .await;
        assert_eq!(core_seen.client().count_unread_in_all(None).await, Ok(9), "asked as \"All\" with unread, no pile");
    }

    /// The query of a search by role is `in:` with the role's serde word.
    #[test]
    fn a_role_is_searched_by_its_serde_word() {
        use torro_mail::Role;
        for role in [Role::Inbox, Role::Drafts, Role::Sent, Role::Archive, Role::Junk, Role::Trash] {
            let word = serde_json::to_value(role).expect("json");
            assert_eq!(in_role(role), format!("in:{}", word.as_str().expect("a word")));
        }
    }

    /// Task E19.3: putting on a pile, a pile's page and its number, a follow-up "only if no reply"
    /// and the categories each send their one request with the fields of E19.1 and E19.2.
    #[tokio::test]
    async fn piles_and_follow_ups_send_their_fields() {
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let core = start({
            let asked = asked.clone();
            move |request| {
                let asked = asked.clone();
                async move {
                    if !matches!(request, Request::Ping)
                        && let Ok(mut asked) = asked.lock()
                    {
                        asked.push(request.clone());
                    }
                    match request {
                        Request::Ping | Request::SetMailPile { .. } => Response::Ok,
                        Request::SearchMail { account, .. } => Response::MailMessages {
                            messages: vec![MessageSummary {
                                pile: Some(MailPile::SetAside),
                                ..summary("m:1", "f:inbox")
                            }],
                            cursor: Some("next".into()),
                            source: crate::mail::MailSource::Cache,
                            complete: true,
                            incomplete_reason: None,
                            highlights: Vec::new(),
                            // An estimate for one account only, to see a missing one counts as none.
                            total_estimate: account.map(|_| 3),
                        },
                        Request::SnoozeItem { id, until, unless_reply, keep_open } => Response::Item {
                            item: Item {
                                state: until.map_or(State::Seen, |until| State::Snoozed { until }),
                                title: format!("unless_reply {unless_reply} keep_open {keep_open}"),
                                ..item(&id)
                            },
                        },
                        other => Response::Error { code: "error".into(), message: format!("{other:?}") },
                    }
                }
            }
        })
        .await;
        let client = core.client();
        let anna = || Some("mail:anna@example.org".to_owned());
        client.set_mail_pile(vec!["m:1".into(), "m:2".into()], Some(MailPile::ReplyLater), None).await.expect("put on");
        client.set_mail_pile(vec!["m:1".into()], None, None).await.expect("taken off");
        client.set_mail_pile(vec!["m:1".into()], Some(MailPile::SetAside), Some(at(9))).await.expect("undone");
        let find = MailInPile {
            pile: MailPile::SetAside,
            account: anna(),
            order: Order::OldestFirst,
            limit: 20,
            cursor: Some("c1".into()),
        };
        let (page, next) = client.mail_in_pile(find).await.expect("a page");
        assert_eq!((page[0].pile, next.as_deref()), (Some(MailPile::SetAside), Some("next")), "the pile comes back");
        assert_eq!(client.count_in_pile(MailPile::ReplyLater, anna()).await, Ok(3));
        assert_eq!(client.count_in_pile(MailPile::ReplyLater, None).await, Ok(0), "no estimate counts as none");
        let followed = client.snooze_item("m:7".into(), Some(at(15)), true, false).await.expect("followed up");
        assert_eq!(
            (followed.title.as_str(), followed.state),
            ("unless_reply true keep_open false", State::Snoozed { until: at(15) })
        );
        client.search_by_category(vec![Category::Invoice], 50, None, Order::NewestFirst).await.expect("a page");

        let search = |account: Option<String>, pile, limit, cursor: Option<&str>, order| Request::SearchMail {
            account,
            folder: None,
            query: Some(PILE_QUERY.into()),
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit,
            cursor: cursor.map(str::to_owned),
            order,
            sort: None,
            snoozed: None,
            categories: Vec::new(),
            pile: Some(pile),
        };
        let asked = asked.lock().expect("the list").clone();
        assert_eq!(
            asked,
            [
                Request::SetMailPile {
                    ids: vec!["m:1".into(), "m:2".into()],
                    pile: Some(MailPile::ReplyLater),
                    since: None
                },
                Request::SetMailPile { ids: vec!["m:1".into()], pile: None, since: None },
                Request::SetMailPile { ids: vec!["m:1".into()], pile: Some(MailPile::SetAside), since: Some(at(9)) },
                search(anna(), PileFilter::SetAside, 20, Some("c1"), Some(Order::OldestFirst)),
                search(anna(), PileFilter::ReplyLater, 1, None, None),
                search(None, PileFilter::ReplyLater, 1, None, None),
                Request::SnoozeItem { id: "m:7".into(), until: Some(at(15)), unless_reply: true, keep_open: false },
                Request::SearchMail {
                    account: None,
                    folder: None,
                    query: None,
                    unread: None,
                    flagged: None,
                    since: None,
                    before: None,
                    limit: 50,
                    cursor: None,
                    order: None,
                    sort: None,
                    snoozed: None,
                    categories: vec![Category::Invoice],
                    pile: Some(PileFilter::Without),
                },
            ]
        );
        assert_eq!(PILE_QUERY, "-in:trash -in:junk", "every folder but the trash and spam");
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

/// One main category as `inbox.list_categories` lists it under `groups`, and as
/// `inbox.create_category_group` and `inbox.update_category_group` answer (issue #343), also one
/// with no category in it yet. The words are the suite's or the person's own, never foreign text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryGroupInfo {
    /// Its stable key: `waiting_for_me`, `paper_trail`, `info`, `junk`, `unsorted` or
    /// `custom_group_<n>`.
    pub group: CategoryGroup,
    /// What a person reads for it: the person's name, else the suite's word.
    pub label: String,
    /// Its monochrome icon.
    pub icon: torro_mail::CategoryIcon,
    /// Its colour in the group palette of the active design, 0–4.
    pub color: u8,
    /// One of the five the suite brings (`false` for one the person made).
    pub built_in: bool,
}

impl CategoryGroupInfo {
    pub fn of(def: &torro_mail::CategoryGroupDef) -> Self {
        Self {
            group: def.group,
            label: def.name.as_deref().unwrap_or_else(|| torro_core::category::group_label(def.group)).to_string(),
            icon: def.icon,
            color: def.color,
            built_in: !matches!(def.group, CategoryGroup::Custom(_)),
        }
    }
}

/// One category of mail as `inbox.list_categories` lists it (task E12.4, `docs/kategorien.md`):
/// the catalogue the sidebar and the "Category" menu are built from, with how much unread mail
/// each one holds.
///
/// The words are the suite's own, so nothing here is foreign text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryInfo {
    /// The category on the wire: `invoice`, `service_notice`, `auto_reply` …
    pub category: Category,
    /// Which of the five groups it belongs to.
    pub group: CategoryGroup,
    /// What a person reads for the category, in the core's language.
    pub label: String,
    /// What a person reads for its group.
    pub group_label: String,
    /// The main category's chosen icon and theme colour.
    #[serde(default)]
    pub group_icon: torro_mail::CategoryIcon,
    #[serde(default)]
    pub group_color: u8,
    /// The category's monochrome icon; its colour comes from `group`.
    #[serde(default)]
    pub icon: torro_mail::CategoryIcon,
    /// Unread mail of this category, counted only in the accounts the caller may read; 0 for a
    /// caller without access to any mail. The inbox folder only: mail thrown away, marked as
    /// spam or sent by the person keeps its category and is counted nowhere.
    pub unread: u32,
    /// All mail of this category in the same place, read or not: whether the category **occurs**
    /// at all. The sidebar lists a category by this and shows [`CategoryInfo::unread`] as its
    /// number, so an entry never leads to an empty list.
    #[serde(default)]
    pub messages: u32,
    /// What it means, as a decision model reads it (task E15.1): the person's words, or the
    /// suite's English description while they wrote none. Words the person keeps, not foreign
    /// text.
    #[serde(default)]
    pub description: String,
    /// A decision model may choose it (task E15.1).
    #[serde(default)]
    pub model: bool,
    /// The suite brings it (task E15.1): it can be changed and put back, not removed. `false` for
    /// a category the person made (`custom_<n>`).
    #[serde(default)]
    pub built_in: bool,
    /// The person changed its name (task E15.1); `label` is then their name. `false` for a
    /// category the person made, whose name is always theirs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub renamed: bool,
}

impl CategoryInfo {
    /// The entry of `def` with its counts: the words a person reads come from the catalogue
    /// (task E15.1) – the name they gave it, else the suite's word.
    pub fn of(def: &torro_mail::CategoryDef, unread: u32, messages: u32) -> CategoryInfo {
        Self::of_with_group(def, unread, messages, None)
    }

    pub fn of_with_group(
        def: &torro_mail::CategoryDef,
        unread: u32,
        messages: u32,
        group: Option<&CategoryGroupInfo>,
    ) -> CategoryInfo {
        CategoryInfo {
            category: def.category,
            group: def.group,
            label: torro_core::category::name_of(def),
            group_label: group
                .map_or_else(|| torro_core::category::group_label(def.group).to_string(), |group| group.label.clone()),
            group_icon: group.map_or(
                torro_mail::CategoryGroupDef::built_in(def.group)
                    .map_or(torro_mail::CategoryIcon::Tag, |group| group.icon),
                |group| group.icon,
            ),
            group_color: group
                .map_or(torro_mail::CategoryGroupDef::built_in(def.group).map_or(0, |group| group.color), |group| {
                    group.color
                }),
            icon: def.icon,
            unread,
            messages,
            description: torro_core::category::description_of(def),
            model: def.model,
            built_in: def.category.is_built_in(),
            renamed: def.category.is_built_in() && def.name.as_deref().is_some_and(|name| !name.trim().is_empty()),
        }
    }
}

/// How the categorisation of mail stands (`inbox.get_categorization`, task E12.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Categorization {
    /// Mail is sorted by a model at all. `false` without a model assigned, or when the person
    /// switched it off.
    pub enabled: bool,
    /// The AI account the decision model belongs to (`ai:…`); `None` when none is assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// The model that decides; `None` when none is assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How far the backlog run has come, per mail account the caller may read.
    #[serde(default)]
    pub accounts: Vec<CategoryProgress>,
    /// The run sorts the window anew, also mail that had a category (task E15.4,
    /// `inbox.catch_up_categories` with `again`); `accounts` counts what it went through.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub again: bool,
    /// The most recent all-account category reset, if one has been started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_job: Option<CategoryClearJob>,
}

/// How far the backlog of one mail account has been sorted ([`Categorization::accounts`], task
/// E12.4); shaped like [`crate::mail::MailIndexProgress`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryProgress {
    /// The account: `mail:<address>`.
    pub account: String,
    /// How much mail the run looks at: the inbox of the last 30 days, at most 500 messages.
    pub messages: u64,
    /// Of them, how much has a category now.
    pub done: u64,
    /// What the core is doing about the rest.
    pub state: CatchUpState,
}

/// What the backlog run of one account is doing ([`CategoryProgress`], task E12.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatchUpState {
    /// Nobody has started it yet.
    NotStarted,
    /// It is running: the core asks the model, throttled, in the background.
    Running,
    /// Every mail the run looks at has a category.
    Complete,
    /// Someone stopped it; what was sorted stays sorted.
    Cancelled,
}

/// The mail selected for a deliberate categorization run (issue #263). Empty account or folder
/// lists mean every accessible account or every folder of the selected accounts, respectively.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct CategoryJobScope {
    #[serde(default)]
    pub accounts: Vec<String>,
    #[serde(default)]
    pub folders: Vec<String>,
    /// Inclusive lower date bound. `None` includes the oldest locally known mail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,
    /// Exclusive upper date bound. `None` includes the newest locally known mail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<DateTime<Utc>>,
    /// Also revisit categories supplied by a model. Person and rule categories stay protected.
    #[serde(default)]
    pub again: bool,
    /// Optional explicit decision model. Both fields must be supplied together; otherwise the
    /// current `inbox.categorize` assignment is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Counts in the selected scope before a job starts. These are all mail, not sidebar unread counts.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct CategoryJobPreview {
    pub total: u64,
    pub unclassified: u64,
    pub automatic: u64,
    pub protected: u64,
    pub eligible: u64,
    /// The exact request scope echoed to correlate overlapping previews from a quickly edited
    /// form. Older servers may leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_scope: Option<CategoryJobScope>,
}

/// Persistent state of one run. A run interrupted by a daemon restart is shown as paused and
/// can be resumed using the same ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CategoryJobState {
    Running,
    Paused,
    Cancelled,
    Complete,
    Failed,
}

/// Control of a previously started job. Resume keeps the original scope and cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CategoryJobControl {
    Pause,
    Resume,
    Cancel,
}

/// Per-account cursor and progress. The maximum sequence freezes the locally known scope at the
/// start; new mail is handled by the regular incoming-mail path, not silently added to this job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryJobAccount {
    pub account: String,
    pub max_sequence: i64,
    pub cursor: i64,
    pub eligible: u64,
    pub processed: u64,
    pub changed: u64,
    pub failed: u64,
}

/// The most recent user-directed categorization job, persisted by the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryJob {
    pub id: String,
    pub scope: CategoryJobScope,
    pub state: CategoryJobState,
    pub accounts: Vec<CategoryJobAccount>,
    #[serde(default)]
    pub errors: Vec<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Incremented when a paused job resumes so an older worker cannot write into it.
    pub generation: u64,
    /// Whether the chosen model sends mail content beyond this computer. `None` when no model is
    /// used (for a rule-only recheck).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_in_cloud: Option<bool>,
}

/// Per-account count before removing every category in one deliberate action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryClearAccount {
    pub account: String,
    pub categorized: u64,
}

/// Preview for the all-account category reset. Counts include manual, rule and model categories
/// in every folder, read and unread alike.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct CategoryClearPreview {
    pub accounts: Vec<CategoryClearAccount>,
    pub total: u64,
}

/// Durable progress in one account. `max_sequence` freezes what existed when the job began.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryClearProgress {
    pub account: String,
    pub max_sequence: i64,
    pub cursor: i64,
    pub categorized: u64,
    pub removed: u64,
}

/// One resumable all-account reset. Nothing in this record contains mail text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CategoryClearJob {
    pub id: String,
    pub state: CategoryJobState,
    pub accounts: Vec<CategoryClearProgress>,
    pub disable_automatic: bool,
    /// True after category-bearing inbox entries have also been cleared.
    #[serde(default)]
    pub entries_cleared: bool,
    #[serde(default)]
    pub errors: Vec<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub generation: u64,
}
