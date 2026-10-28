//! Mail in the inbox (task E4.2 in `docs/tasks/mail.md`, `docs/eingang.md` "Die App"): the
//! folders of the mail accounts in the sidebar, a folder's messages in the same list, the whole
//! mail in the panel, and what can be done with it.
//!
//! - [`MailSource`] is what the window can do with mail, one `mail.*` request per method. The
//!   core's client ([`InboxClient`]) and the demo ([`DemoSource`]) implement it here, so the app
//!   stays generic over one source as before.
//! - [`FolderView`] is the view of one folder (with the search typed into it); [`by_account`]
//!   groups the folders for the sidebar, [`folder_name`] and [`folder_icon`] name and mark them.
//! - [`MailEdit`] is one change (flags, move, delete), [`MailAction`] what a button or key asks
//!   for. Archive and delete move the message and offer an undo that moves it back: a message keeps
//!   its id when it is moved.
//! - Reply, reply all and forward open the compose window (task E4.3) as its own process
//!   ([`compose_command`]): `torroinbox --compose --reply <id>`, `--reply-all <id>` or
//!   `--forward <id>`, started like the suite starts its programs ([`source::sibling`]).
//! - A mail with HTML (task M9.7, decision 9 in `docs/mail.md`) shows as it was written right in
//!   the panel and the reader: [`HtmlMail`] holds its HTML parsed by `torro_htmlview` while it is
//!   open, its `cid:` pictures come from the core ([`MailPictures`], `mail.get_attachment`). The
//!   switches over it ([`HtmlReading`]: "HTML | Text", "Light | Dark") start from the settings
//!   `mail_view` and `mail_appearance` ([`MailLook`], [`stored_look`]). The panel learns whether a
//!   mail has HTML from the same `mail.get_message` that brings its text ([`MailSource::get_mail`]
//!   asks with `include_html`).
//! - [`list`] draws a folder's messages – rows, or cards in the comfortable density of the mail
//!   window (task E8.3, [`message_card`]), in the mail window without buttons (task E14.2,
//!   [`MailList::bare`]) – and the smart view "With attachments" ([`attachments`]:
//!   the messages with attachments of every account's inbox). [`body`] is the whole mail for the
//!   panel, [`reader`] the reader beside the list in the wide mail window (task E8.3), the text in
//!   the reading level (`typo::read()`); [`empty_reader`] keeps its column while no mail is open
//!   (task E8.11). The panel of the hub and the App Feed splits the buttons (task E9.4): the mail
//!   standards at the top – Reply · Reply all · Forward | Archive · Delete, Flag, read –, what Torro
//!   adds in its fixed footer. The mail window has none of them (task E14.3): every action on the
//!   mail stands in its action rail at the right edge ([`action_groups`]: Reply · File · Torro ·
//!   Mark, at the foot Mute · More, drawn by `widgets::action_rail`), its head has no buttons and
//!   the attachments stand in their block at the reader's bottom ([`attachment_line`]). Which bars a
//!   mail in the panel has, [`Bars`] says (task E11.4): the hub and the App Feed keep the split of
//!   E9.4, the mail window's panel in medium and narrow takes the reader's head without the subject
//!   (the panel's title says it), the attachments as rows in its fixed footer ([`panel_foot`]) and the
//!   collapsed rail beside it; [`entry_panel`] is that panel for a mail entry.
//! - The task made of a mail (task E8.7b) stands over its text as a card ([`task_card`], "Open
//!   task"); "Task" on a folder's message makes one ([`MailSource::create_todo_from_mail`]).
//! - Saving attachments (task M12.6, issue #290): each attachment is a split chip – rows in the panel
//!   – whose name does what the setting `mail_attachments.click` says ([`click_act`], [`FileAct`]) and
//!   whose triangle opens its menu ([`file_menu`]: Open, Save to Downloads, Save as…, the save targets
//!   with where the file would go, "Suggested", "Click", "Manage save targets…"); "Save all ▾" from two
//!   on, and "Save attachments (n)" in the rail's File ([`all_act`]). Where a file would go comes from
//!   the core (`mail.list_save_targets`, [`MailSource::save_targets`]), saving is
//!   `mail.save_attachments` ([`MailSource::save_attachments`]); the window adds only the system's
//!   dialogs and "Show in folder" ([`saved_text`], [`saved_folder`]).
//! - The mail window's views of every account (task E18.3): "All", "Hidden" and "Archive" list the
//!   mail of one folder role ([`MailSource::mail_by_role`]), the number on "All" comes from
//!   [`MailSource::unread_in_all`], and "Show again" is [`MailSource::set_mail_shown`].
//! - The piles (task E19.3): [`MailSource::set_mail_pile`] puts mail on "Reply later" or "Set aside"
//!   or takes it off, [`MailSource::mail_in_pile`] lists a pile and [`MailSource::count_in_pile`] counts
//!   it; "All" asks [`MailSource::mail_by_role`] with `pile: without`. "Focus & reply" answers with
//!   [`MailSource::send_reply`], built as the compose window builds a reply ([`send_reply_through`]).
//! - "Focus & reply" (task E19.5, decision 10 of E19): [`FocusReply`] is the round through "Reply
//!   later" – the pile's mail as listed when it started, the place, the reply typed –, [`focus_page`]
//!   draws it in place of list and reader: the mail's head and text in the reading column, under a line
//!   the field and Send · Skip · Remove from Reply later · Open in compose window.
//!
//! **Mail is foreign text** (`untrusted`). Its HTML comes cleaned by the core (no scripts, no
//! forms, legacy attributes as CSS) and is drawn without JavaScript; a click on one of its links
//! only reports the address, the app opens `http`, `https` and `mailto` in the browser. The text is drawn as it is;
//! the text can be selected and copied (issue #119, `torro_gui::selectable`, "Copy text"), and the web
//! addresses in it are listed under it as "Links" that open only on a click. An attachment is fetched by the core into its attachment folder and opened with the
//! system, except files that could run programs ([`runs_programs`]): their folder opens instead.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use chrono::{DateTime, Local, NaiveDate, Utc};
use iced::keyboard;
use iced::widget::text::Wrapping;
use iced::widget::{center, column, container, mouse_area, responsive, row, rule, scrollable, svg, text_editor};
use iced::{Alignment, Color, Element, Fill, Padding, Theme};
use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::{AttachmentClick, MailAppearance, MailFormat};
use torro_core::shortcuts::System;
use torro_gui::icon::{self, Icon};
use torro_gui::layout;
use torro_gui::panel;
use torro_gui::selectable::{self, Selectable};
use torro_gui::source;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Mark, MenuItem, MenuSection, RailGroup, RailItem, RowAction};
use torro_gui::{datetime, style, typo};
use torro_htmlview::{CidSource, Content, HtmlView, MAX_PICTURE_BYTES, Scheme};
use torro_inbox::InboxError;
use torro_inbox::demo::DemoSource;
use torro_inbox::item::{ItemTask, TaskDue};
use torro_inbox::source::TaskList;
use torro_inbox::{Item, Lane, Link, State};
use torro_ipc::inbox::CategoryInfo;
use torro_ipc::inbox::{
    CalendarEvent, InboxClient, MailByRole, MailFind, MailFound, MailInPile, SaveTargets, SavedAttachments,
};
#[cfg(test)]
use torro_ipc::mail::MailSort;
use torro_ipc::mail::{
    AttachmentPlan, Delivery, MailHighlight, MailIndexProgress, MoveRole, MoveTarget, PileFilter, PlannedPlace, SaveTo,
    SavedFile,
};
use torro_ipc::{Request, Response};
use torro_mail::{
    AttachmentInfo, CategoryGroup, Draft, Flag, Folder, InvitationAnswer, Invite, MailAddress, MailCategory, MailPile,
    Message as Mail, MessageSummary, Order, Role,
};

use crate::app::{INBOX, Message, View};
use crate::compose::{self, ComposeError, ComposeSource};
use crate::detail;
use crate::keys::Command as Key;
use crate::list::{self, Look};

/// How many messages one page of a folder holds.
pub const PAGE: u32 = 50;

/// What the window can do with mail. Every method is one `mail.*` request of the app key; the core
/// checks rights and keeps the security rules, a source never decides them itself. Errors as the
/// inbox maps them ([`InboxError`]: `not-found`, `forbidden`, `unsupported`, offline, other).
pub trait MailSource: Send + Sync {
    /// `mail.list_folders`: the folders of every mail account, in the core's order, with their
    /// role and numbers. Empty without a mail account.
    fn mail_folders(&self) -> impl Future<Output = Result<Vec<Folder>, InboxError>> + Send;

    /// `mail.search_messages` in the folder `folder` (a [`Folder::id`]): at most `limit` messages
    /// in `order` (task E8.15), holding every word of `query` when there is one, from `cursor` on
    /// (the [`MailPage::next`] of the page before, asked in the same `order`).
    fn search_mail(
        &self,
        folder: String,
        query: Option<String>,
        limit: u32,
        cursor: Option<String>,
        order: Order,
    ) -> impl Future<Output = Result<MailPage, InboxError>> + Send;

    /// `inbox.list_categories`: the catalogue of mail categories with how much unread mail each
    /// one holds (task E12.8), for one account or all when `account` is `None`.
    fn list_categories(
        &self,
        account: Option<String>,
    ) -> impl Future<Output = Result<Vec<CategoryInfo>, InboxError>> + Send;

    /// `inbox.get_categorization`: whether a decision model is set up for sorting mail (task
    /// E12.8). Only then does the reader offer "Re-categorize".
    fn categorization_ready(&self) -> impl Future<Output = Result<bool, InboxError>> + Send;

    /// `mail.search_messages` filtered by `categories` (task E12.8): the mail of every account
    /// that is of one of them, newest first, at most `limit` from `cursor` on.
    fn search_by_category(
        &self,
        categories: Vec<torro_mail::Category>,
        limit: u32,
        cursor: Option<String>,
        order: Order,
    ) -> impl Future<Output = Result<MailPage, InboxError>> + Send;

    /// `inbox.set_item_category`: files the mail `id` under `category` by hand; with `remember`
    /// later mail of its sender too (task E12.8). Returns the entry as it stands now.
    fn set_mail_category(
        &self,
        id: String,
        category: torro_mail::Category,
        remember: bool,
    ) -> impl Future<Output = Result<Item, InboxError>> + Send;

    /// `inbox.classify_item`: asks the decision model what the mail `id` is, again (task E12.8).
    fn classify_mail(&self, id: String) -> impl Future<Output = Result<Item, InboxError>> + Send;

    /// `mail.get_message` with its text and – asked with `include_html` (task M9.3) – its cleaned
    /// HTML, which the panel and the reader draw (task M9.7, [`shows_html`]). Its text only when
    /// the HTML cannot be had (the core must fetch it for an old cached mail and the server is out
    /// of reach): the mail then shows as text. Reading does not mark it read.
    fn get_mail(&self, id: String) -> impl Future<Output = Result<Mail, InboxError>> + Send;

    /// `mail.get_attachment`: the attachment `part` of the message `id` as a file in the core's
    /// attachment folder; answers its path.
    fn mail_attachment(&self, id: String, part: String) -> impl Future<Output = Result<PathBuf, InboxError>> + Send;

    /// `mail.list_save_targets` (task M12.6, issue #290): the save targets, the Downloads folder and –
    /// with the message `id` – where each of its attachments would go ([`SaveTargets::files`], one
    /// [`torro_ipc::mail::AttachmentPlan`] per attachment that is not `inline`). The attachment menus
    /// show the targets from it, the paths and "Suggested" from the plans; a click on a name reads its
    /// plan's `sure`. [`InboxError::NotFound`] for a message the window may not see.
    fn save_targets(&self, id: Option<String>) -> impl Future<Output = Result<SaveTargets, InboxError>> + Send;

    /// `mail.save_attachments` (task M12.6): saves the attachments `parts` of the message `id` – none:
    /// every one that is not `inline` ("Save all") – where `to` says. `to: file` and `folder` carry the
    /// path a person chose in the system's dialog (the window holds the app key). Answers what was
    /// saved, skipped as the same file and failed ([`SavedAttachments`]); a file is never overwritten
    /// but with `to: file`. [`InboxError::NotFound`] for an unknown message, part or target, or a
    /// target whose folder is missing.
    fn save_attachments(
        &self,
        id: String,
        parts: Vec<String>,
        to: SaveTo,
    ) -> impl Future<Output = Result<SavedAttachments, InboxError>> + Send;

    /// `mail.set_flags`: sets `add` and clears `remove` on the messages `ids`.
    fn set_mail_flags(
        &self,
        ids: Vec<String>,
        add: Vec<Flag>,
        remove: Vec<Flag>,
    ) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// `mail.move_messages`: moves the messages `ids` (one account) to `to`; they keep their ids.
    fn move_mail(&self, ids: Vec<String>, to: MoveTarget) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// `mail.delete_messages` into the account's trash, never for good.
    fn delete_mail(&self, ids: Vec<String>) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// `mail.delete_messages` with `permanent` (task E10.1, "Delete permanently", only after the
    /// window asked): removes the messages `ids` for good – through the trash, as the core removes
    /// only what lies there ([`InboxClient::delete_mail_for_good`]). There is no undo.
    fn delete_mail_for_good(&self, ids: Vec<String>) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// `accounts.sync` (task E10.1, the shortcut "fetch"): the core fetches from the servers of every
    /// account now. The one method that is no `mail.*` request: fetching mail is the accounts'
    /// sync. The window lists anew afterwards.
    fn sync_mail(&self) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// `tasks.create_todo_from_mail` (task E8.7a; "Task" on a folder's message, task E8.7b): makes a
    /// task of the message `id` of the mail account `account` (`mail:<address>`) in the task list
    /// `list` (one of [`torro_inbox::InboxSource::list_task_lists`]), titled `title` (`None`: the
    /// core takes the subject), due on the local day `due` (`None`: no due date; task E8.1b, as the
    /// dialog of "Make task" asks it for entries). The core links it back with
    /// `mail:<address>:<Message-ID>`; the mail itself is not changed.
    ///
    /// Answers the new task as an entry would carry it ([`ItemTask`]: its uid, list – named
    /// `list.name` –, title, due day and link; `completed` false, `mail_entry` `None`), so the
    /// reader shows its card at once. Errors as the core maps them: [`InboxError::NotFound`] for an
    /// account, mail or list the window may not see, [`InboxError::Unsupported`] for a mail without
    /// a `Message-ID`.
    fn create_todo_from_mail(
        &self,
        account: String,
        id: String,
        list: TaskList,
        title: Option<String>,
        due: Option<NaiveDate>,
    ) -> impl Future<Output = Result<ItemTask, InboxError>> + Send;

    /// `mail.respond_to_invitation` (task E8.3b, Accept · Maybe · Decline): answers the invitation in
    /// the message `id` of the mail account `account` with `answer`. The core sends the organizer
    /// the reply from the account and puts the event in a calendar – `collection` (task E8.3c, an id
    /// of [`crate::hub::HubSource::list_calendars`]; `None`: the core's choice) unless a copy made
    /// before stays in its own – or, for a decline, removes that copy. The entry of the mail is not
    /// touched: the window marks it done itself (`inbox.set_item_done`). Answers how the reply went
    /// ([`Delivery::Unknown`]: it may not have gone out). Errors as the core maps them:
    /// [`InboxError::NotFound`] for a message, account or calendar the window may not see,
    /// [`InboxError::Forbidden`], [`InboxError::Other`] with the core's words for a mail without an
    /// invitation to answer, an event that is over, or a send that failed.
    fn respond_to_invitation(
        &self,
        account: String,
        id: String,
        answer: InvitationAnswer,
        collection: Option<String>,
    ) -> impl Future<Output = Result<Delivery, InboxError>> + Send;

    /// `calendar.list_events` (task E8.3c): the events of the shown calendars that overlap
    /// `[start, end)`, in instants ([`CalendarEvent`]), for the line on what an invitation overlaps
    /// ([`overlaps`]). Nothing is written.
    fn list_events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<CalendarEvent>, InboxError>> + Send;

    /// `mail.search_messages` across folders and accounts (issue #136, the search field): where
    /// `find` says – every folder of every account, one account or one folder –, the words and
    /// operators as typed, by relevance or by date in `find.order`, `find.limit` from `find.cursor`
    /// on. Answers the page with whether matches may be missing, where the words matched and about
    /// how many match in all. Errors as the core maps them; `InboxError::Other` with the core's
    /// words for an operator it cannot read (`before:soon`).
    fn find_mail(&self, find: MailFind) -> impl Future<Output = Result<MailFound, InboxError>> + Send;

    /// `mail.index_status`: how far the search index of each mail account is, for the honest line
    /// under the search field while matches may be missing.
    fn mail_index_status(&self) -> impl Future<Output = Result<Vec<MailIndexProgress>, InboxError>> + Send;

    /// `mail.search_messages` without a folder and with the query `in:<role>` (task E18.3,
    /// [`InboxClient::mail_by_role`]): the mail in the folders of `find.role` of every account the
    /// window reaches (or of `find.account`), leaving out or giving only snoozed and hidden mail as
    /// `find.snoozed` and `find.hidden` say and, by `find.pile` (task E19.3), leaving out or giving
    /// only mail on a pile, at most `find.limit` in `find.order` from `find.cursor` on (the
    /// [`MailPage::next`] of the page before, asked with the same fields). "All" is [`Role::Inbox`]
    /// with `snoozed` and `hidden` `Some(false)` and `pile` `Some(PileFilter::Without)`, "Hidden"
    /// [`Role::Inbox`] with `hidden` `Some(true)`, "Archive" [`Role::Archive`]. The page has no
    /// highlights.
    fn mail_by_role(&self, find: MailByRole) -> impl Future<Output = Result<MailPage, InboxError>> + Send;

    /// The number on "All" (task E18.3, decision 4 of E18): `mail.search_messages` as "All" asks it
    /// – [`Role::Inbox`], `snoozed` and `hidden` `Some(false)`, no mail on a pile (task E19.3) – with
    /// `unread: Some(true)` and
    /// `limit: 1`, for `account` (`mail:<address>`) or every account the window reaches
    /// ([`InboxClient::count_unread_in_all`]). Answers the core's `total_estimate`, 0 without one.
    fn unread_in_all(&self, account: Option<String>) -> impl Future<Output = Result<u64, InboxError>> + Send;

    /// `inbox.set_mail_shown` (task E18.2, "Show again"; [`InboxClient::set_mail_shown`]): `shown:
    /// true` takes the mail `ids` (`m:…`, 1 to 100) out of "Hidden" into "All" – the rule that hid
    /// them stays, their entry stays as it is –, `shown: false` takes that back (the undo).
    /// [`InboxError::NotFound`] when one of them is not mail the window may see; nothing is
    /// remembered then. `Ok` also when there was nothing to remember.
    fn set_mail_shown(&self, ids: Vec<String>, shown: bool) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// `inbox.set_mail_pile` (task E19.3, [`InboxClient::set_mail_pile`]): puts the mail `ids` (`m:…`,
    /// 1 to 100) on `pile` – "Reply later" or "Set aside"; a message lies on at most one, so this moves
    /// it from the other –, or takes them off with `None` (also the undo, with the pile each lay on
    /// before, [`MessageSummary::pile`]). `since` puts them on with the time they lay there before
    /// ([`MessageSummary::pile_since`], the undo of taking them off or moving them, decision of #315);
    /// `None` counts as now. Kept only in the core, nothing goes to the server.
    /// [`InboxError::NotFound`] when one of them is not mail the window may see; nothing changes then.
    fn set_mail_pile(
        &self,
        ids: Vec<String>,
        pile: Option<MailPile>,
        since: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// One page of a pile (task E19.3, [`InboxClient::mail_in_pile`]): the mail on `find.pile` in
    /// every folder but the trash and spam, of every account the window reaches (or of
    /// `find.account`), whether snoozed or hidden or not, at most `find.limit` in `find.order` from
    /// `find.cursor` on (the [`MailPage::next`] of the page before, asked with the same fields). Every
    /// message carries its pile. The page has no highlights.
    fn mail_in_pile(&self, find: MailInPile) -> impl Future<Output = Result<MailPage, InboxError>> + Send;

    /// How much mail lies on `pile` (task E19.3, [`InboxClient::count_in_pile`]), read and unread, for
    /// `account` (`mail:<address>`) or every account the window reaches: the number beside the pile
    /// in the sidebar. The core's `total_estimate`, 0 without one.
    fn count_in_pile(
        &self,
        pile: MailPile,
        account: Option<String>,
    ) -> impl Future<Output = Result<u64, InboxError>> + Send;

    /// Answers the message `id` with `text` ("Focus & reply", tasks E19.3 and E19.5), as the compose
    /// window answers it ([`send_reply_through`]): to the sender (`Reply-To:` first), from the account
    /// the mail came to, `Re:` before the subject, `text` above the quoted mail, formatted for an
    /// account that writes HTML; `mail.save_draft`, then `mail.send_draft`. The pile is not touched:
    /// the window takes the mail off "Reply later" afterwards (the core does too once the sent copy
    /// turns up). Errors as the core maps them; [`InboxError::Other`] with the reason when there is
    /// no account to write from, no one to answer, or the server refused (the saved draft stays in
    /// the drafts folder then).
    fn send_reply(&self, id: String, text: String) -> impl Future<Output = Result<(), InboxError>> + Send;
}

/// What "Focus & reply" sends for the message `id` (task E19.3, [`MailSource::send_reply`]): the
/// reply the compose window opens with ([`compose::opening`] with [`compose::Kind::Reply`], the account
/// of [`compose::pick_sender`]), `text` typed above the quote, saved and then sent through `source`.
/// A send whose outcome is unknown ([`Delivery::Unknown`]) counts as sent, as in the compose window:
/// it is never sent again by itself.
pub async fn send_reply_through<S: ComposeSource>(source: &S, id: String, text: &str) -> Result<(), InboxError> {
    let senders = source.senders().await.map_err(inbox_error)?;
    let message = source.get_mail(id.clone()).await.map_err(inbox_error)?;
    let own: Vec<String> = senders.iter().map(|sender| sender.address.to_lowercase()).collect();
    let opened = compose::opening(&compose::Kind::Reply(id), Some(&message), &own);
    let sender = compose::pick_sender(&senders, None, Some(&message))
        .and_then(|at| senders.get(at))
        .ok_or_else(|| InboxError::Other(tr("No mail account yet.").to_owned()))?;
    // Checked before saving, so a reply that cannot go leaves no draft behind.
    if opened.to.is_empty() && opened.cc.is_empty() {
        return Err(InboxError::Other(tr("Add at least one recipient.").to_owned()));
    }
    let saved = source.save_draft(reply_draft(sender, opened, text)).await.map_err(inbox_error)?;
    let draft = saved.id.ok_or_else(|| InboxError::Other("the draft was saved without an id".into()))?;
    source.send_draft(draft).await.map_err(inbox_error)?;
    Ok(())
}

/// The draft of an answer as the compose window would save it: `text` above `opened`'s quoted mail,
/// from `sender`; with the marks as HTML when the account writes formatted mail, the quote staying
/// someone else's text ([`torro_mail::markup::to_html`]).
fn reply_draft(sender: &compose::Sender, opened: compose::Opening, text: &str) -> Draft {
    let body = format!("{}{}", text.trim().replace("\r\n", "\n"), opened.body);
    let html = sender
        .html
        .then(|| torro_mail::markup::to_html(&body, opened.history.as_deref()))
        .filter(|markup| !markup.is_empty());
    Draft {
        id: None,
        account: sender.account.clone(),
        to: opened.to,
        cc: opened.cc,
        bcc: Vec::new(),
        subject: opened.subject,
        body: html.as_deref().map_or(body, torro_mail::html::to_text),
        html,
        in_reply_to: opened.in_reply_to,
        attachments: Vec::new(),
    }
}

/// A compose call's error as the mail window's calls report it.
fn inbox_error(err: ComposeError) -> InboxError {
    match err {
        ComposeError::Offline => InboxError::Offline,
        ComposeError::NotFound => InboxError::NotFound,
        ComposeError::Forbidden => InboxError::Forbidden,
        ComposeError::Other(message) => InboxError::Other(message),
    }
}

/// Whether a message on `pile` is one that `filter` asks for, as the core reads `mail.search_messages`'
/// `pile` (task E19.2): `None` every message, `Without` none on a pile, a pile only its mail.
fn pile_matches(filter: Option<PileFilter>, pile: Option<MailPile>) -> bool {
    match filter {
        None => true,
        Some(PileFilter::Without) => pile.is_none(),
        Some(PileFilter::ReplyLater) => pile == Some(MailPile::ReplyLater),
        Some(PileFilter::SetAside) => pile == Some(MailPile::SetAside),
    }
}

/// One page of a folder ([`MailSource::search_mail`]) or of a search ([`MailSource::find_mail`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MailPage {
    /// Newest first.
    pub messages: Vec<MessageSummary>,
    /// The cursor of the next page; `None` on the last one.
    pub next: Option<String>,
    /// Where the words of a search matched (issue #136); empty for a folder.
    pub highlights: Vec<MailHighlight>,
}

impl MailPage {
    /// A page of `messages` without highlights, `next` the cursor of the page after it.
    pub fn of(messages: Vec<MessageSummary>, next: Option<String>) -> MailPage {
        MailPage { messages, next, highlights: Vec::new() }
    }
}

impl MailSource for InboxClient {
    async fn mail_folders(&self) -> Result<Vec<Folder>, InboxError> {
        InboxClient::mail_folders(self).await
    }

    async fn list_categories(&self, account: Option<String>) -> Result<Vec<CategoryInfo>, InboxError> {
        InboxClient::list_categories_for(self, account).await
    }

    async fn categorization_ready(&self) -> Result<bool, InboxError> {
        Ok(InboxClient::categorization(self).await?.enabled)
    }

    async fn search_by_category(
        &self,
        categories: Vec<torro_mail::Category>,
        limit: u32,
        cursor: Option<String>,
        order: Order,
    ) -> Result<MailPage, InboxError> {
        let (messages, next) = InboxClient::search_by_category(self, categories, limit, cursor, order).await?;
        Ok(MailPage::of(messages, next))
    }

    async fn set_mail_category(
        &self,
        id: String,
        category: torro_mail::Category,
        remember: bool,
    ) -> Result<Item, InboxError> {
        InboxClient::set_mail_category(self, id, category, remember).await
    }

    async fn classify_mail(&self, id: String) -> Result<Item, InboxError> {
        InboxClient::classify_mail(self, id).await
    }

    async fn search_mail(
        &self,
        folder: String,
        query: Option<String>,
        limit: u32,
        cursor: Option<String>,
        order: Order,
    ) -> Result<MailPage, InboxError> {
        let (messages, next) = InboxClient::search_mail(self, folder, query, limit, cursor, order).await?;
        Ok(MailPage::of(messages, next))
    }

    async fn find_mail(&self, find: MailFind) -> Result<MailFound, InboxError> {
        InboxClient::find_mail(self, find).await
    }

    async fn mail_index_status(&self) -> Result<Vec<MailIndexProgress>, InboxError> {
        InboxClient::mail_index_status(self).await
    }

    async fn mail_by_role(&self, find: MailByRole) -> Result<MailPage, InboxError> {
        let (messages, next) = InboxClient::mail_by_role(self, find).await?;
        Ok(MailPage::of(messages, next))
    }

    async fn unread_in_all(&self, account: Option<String>) -> Result<u64, InboxError> {
        InboxClient::count_unread_in_all(self, account).await
    }

    async fn set_mail_shown(&self, ids: Vec<String>, shown: bool) -> Result<(), InboxError> {
        InboxClient::set_mail_shown(self, ids, shown).await
    }

    async fn set_mail_pile(
        &self,
        ids: Vec<String>,
        pile: Option<MailPile>,
        since: Option<DateTime<Utc>>,
    ) -> Result<(), InboxError> {
        InboxClient::set_mail_pile(self, ids, pile, since).await
    }

    async fn mail_in_pile(&self, find: MailInPile) -> Result<MailPage, InboxError> {
        let (messages, next) = InboxClient::mail_in_pile(self, find).await?;
        Ok(MailPage::of(messages, next))
    }

    async fn count_in_pile(&self, pile: MailPile, account: Option<String>) -> Result<u64, InboxError> {
        InboxClient::count_in_pile(self, pile, account).await
    }

    async fn send_reply(&self, id: String, text: String) -> Result<(), InboxError> {
        send_reply_through(self, id, &text).await
    }

    async fn get_mail(&self, id: String) -> Result<Mail, InboxError> {
        // With the HTML the core may have to fetch the mail once more (cached before it kept HTML);
        // offline that fails, and the text it holds must still show.
        match InboxClient::get_mail_html(self, id.clone()).await {
            Ok(mail) => Ok(mail),
            Err(_) => InboxClient::get_mail(self, id).await,
        }
    }

    async fn mail_attachment(&self, id: String, part: String) -> Result<PathBuf, InboxError> {
        InboxClient::mail_attachment(self, id, part).await
    }

    async fn save_targets(&self, id: Option<String>) -> Result<SaveTargets, InboxError> {
        InboxClient::save_targets(self, id).await
    }

    async fn save_attachments(
        &self,
        id: String,
        parts: Vec<String>,
        to: SaveTo,
    ) -> Result<SavedAttachments, InboxError> {
        InboxClient::save_attachments(self, id, parts, to).await
    }

    async fn set_mail_flags(&self, ids: Vec<String>, add: Vec<Flag>, remove: Vec<Flag>) -> Result<(), InboxError> {
        InboxClient::set_mail_flags(self, ids, add, remove).await
    }

    async fn move_mail(&self, ids: Vec<String>, to: MoveTarget) -> Result<(), InboxError> {
        InboxClient::move_mail(self, ids, to).await
    }

    async fn delete_mail(&self, ids: Vec<String>) -> Result<(), InboxError> {
        InboxClient::delete_mail(self, ids).await
    }

    async fn delete_mail_for_good(&self, ids: Vec<String>) -> Result<(), InboxError> {
        InboxClient::delete_mail_for_good(self, ids).await
    }

    async fn sync_mail(&self) -> Result<(), InboxError> {
        InboxClient::sync(self).await
    }

    async fn create_todo_from_mail(
        &self,
        account: String,
        id: String,
        list: TaskList,
        title: Option<String>,
        due: Option<NaiveDate>,
    ) -> Result<ItemTask, InboxError> {
        InboxClient::create_todo_from_mail(self, account, id, list, title, due).await
    }

    async fn respond_to_invitation(
        &self,
        account: String,
        id: String,
        answer: InvitationAnswer,
        collection: Option<String>,
    ) -> Result<Delivery, InboxError> {
        InboxClient::respond_to_invitation(self, account, id, answer, collection).await
    }

    async fn list_events(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<CalendarEvent>, InboxError> {
        InboxClient::list_events(self, start, end).await
    }
}

impl MailSource for DemoSource {
    async fn mail_folders(&self) -> Result<Vec<Folder>, InboxError> {
        DemoSource::mail_folders(self)
    }

    async fn list_categories(&self, account: Option<String>) -> Result<Vec<CategoryInfo>, InboxError> {
        Ok(DemoSource::category_counts_for(self, account.as_deref())
            .into_iter()
            .filter_map(|(category, unread, messages)| {
                torro_mail::CategoryDef::built_in(category).map(|def| CategoryInfo::of(&def, unread, messages))
            })
            .collect())
    }

    /// The demo has no AI account, so it offers no "Re-categorize".
    async fn categorization_ready(&self) -> Result<bool, InboxError> {
        Ok(false)
    }

    async fn search_by_category(
        &self,
        categories: Vec<torro_mail::Category>,
        limit: u32,
        _cursor: Option<String>,
        order: Order,
    ) -> Result<MailPage, InboxError> {
        Ok(MailPage::of(DemoSource::by_category(self, &categories, limit, order), None))
    }

    async fn set_mail_category(
        &self,
        id: String,
        category: torro_mail::Category,
        _remember: bool,
    ) -> Result<Item, InboxError> {
        DemoSource::set_mail_category(self, &id, category)
    }

    async fn classify_mail(&self, id: String) -> Result<Item, InboxError> {
        // The demo has no model: it answers what the mail already says.
        DemoSource::classified(self, &id)
    }

    async fn search_mail(
        &self,
        folder: String,
        query: Option<String>,
        limit: u32,
        cursor: Option<String>,
        order: Order,
    ) -> Result<MailPage, InboxError> {
        let (mut messages, next) =
            DemoSource::search_mail(self, &folder, query.as_deref(), limit, cursor.as_deref(), order)?;
        for message in &mut messages {
            message.invite = demo_invite(&message.id, message.date);
        }
        Ok(MailPage::of(messages, next))
    }

    async fn find_mail(&self, find: MailFind) -> Result<MailFound, InboxError> {
        let mut found = crate::search::demo_find(self, find)?;
        for message in &mut found.messages {
            message.invite = demo_invite(&message.id, message.date);
        }
        Ok(found)
    }

    async fn mail_index_status(&self) -> Result<Vec<MailIndexProgress>, InboxError> {
        crate::search::demo_index(self)
    }

    /// The demo's mail of the role ([`DemoSource::by_role`]) with the piles `find.pile` asks for, a
    /// page at a time; the cursor is the place of the next page, as in the demo's folders.
    async fn mail_by_role(&self, find: MailByRole) -> Result<MailPage, InboxError> {
        let mut found =
            DemoSource::by_role(self, find.role, find.account.as_deref(), find.snoozed, find.hidden, find.order);
        found.retain(|message| pile_matches(find.pile, message.pile));
        let mut page = demo_page(found, find.limit, find.cursor.as_deref())?;
        for message in &mut page.messages {
            message.invite = demo_invite(&message.id, message.date);
        }
        Ok(page)
    }

    /// The unread mail of what "All" lists, counted exactly: none on a pile.
    async fn unread_in_all(&self, account: Option<String>) -> Result<u64, InboxError> {
        let all =
            DemoSource::by_role(self, Role::Inbox, account.as_deref(), Some(false), Some(false), Order::NewestFirst);
        let unread = |message: &&MessageSummary| message.pile.is_none() && !message.flags.contains(&Flag::Seen);
        Ok(all.iter().filter(unread).count() as u64)
    }

    async fn set_mail_shown(&self, ids: Vec<String>, shown: bool) -> Result<(), InboxError> {
        DemoSource::set_mail_shown(self, &ids, shown)
    }

    async fn set_mail_pile(
        &self,
        ids: Vec<String>,
        pile: Option<MailPile>,
        since: Option<DateTime<Utc>>,
    ) -> Result<(), InboxError> {
        DemoSource::set_mail_pile(self, &ids, pile, since)
    }

    /// The demo's pile ([`DemoSource::in_pile`]) a page at a time, as its roles.
    async fn mail_in_pile(&self, find: MailInPile) -> Result<MailPage, InboxError> {
        let found = DemoSource::in_pile(self, find.pile, find.account.as_deref(), find.order);
        let mut page = demo_page(found, find.limit, find.cursor.as_deref())?;
        for message in &mut page.messages {
            message.invite = demo_invite(&message.id, message.date);
        }
        Ok(page)
    }

    /// The demo's pile counted exactly.
    async fn count_in_pile(&self, pile: MailPile, account: Option<String>) -> Result<u64, InboxError> {
        Ok(DemoSource::in_pile(self, pile, account.as_deref(), Order::NewestFirst).len() as u64)
    }

    /// Built and sent as for the core; the demo keeps the answer as sent and takes the conversation
    /// off "Reply later", as the core does ([`DemoSource::send_draft`]).
    async fn send_reply(&self, id: String, text: String) -> Result<(), InboxError> {
        send_reply_through(self, id, &text).await
    }

    async fn get_mail(&self, id: String) -> Result<Mail, InboxError> {
        let mut mail = DemoSource::get_mail(self, &id)?;
        mail.summary.invite = demo_invite(&mail.summary.id, mail.summary.date);
        mail.invitation = mail.summary.invite.is_some();
        Ok(mail)
    }

    async fn mail_attachment(&self, id: String, part: String) -> Result<PathBuf, InboxError> {
        DemoSource::mail_attachment(self, &id, &part)
    }

    /// The demo's two save targets ([`torro_inbox::demo::save_targets`]) and, for a message, where
    /// each of its attachments would go – planned as the core plans it ([`demo_plan`]).
    async fn save_targets(&self, id: Option<String>) -> Result<SaveTargets, InboxError> {
        let targets = torro_inbox::demo::save_targets();
        let files = match id {
            Some(id) => demo_plans(&DemoSource::get_mail(self, &id)?, &targets),
            None => Vec::new(),
        };
        Ok(SaveTargets { targets, downloads: torro_inbox::demo::DEMO_DOWNLOADS.to_owned(), files })
    }

    /// The demo writes no file (issue #290): it answers where the core would have put each one
    /// ([`demo_saved`]), so the window's hint and "Show in folder" have a path.
    async fn save_attachments(
        &self,
        id: String,
        parts: Vec<String>,
        to: SaveTo,
    ) -> Result<SavedAttachments, InboxError> {
        let mail = DemoSource::get_mail(self, &id)?;
        demo_saved(&mail, &parts, &to)
    }

    async fn set_mail_flags(&self, ids: Vec<String>, add: Vec<Flag>, remove: Vec<Flag>) -> Result<(), InboxError> {
        DemoSource::set_mail_flags(self, &ids, &add, &remove)
    }

    async fn move_mail(&self, ids: Vec<String>, to: MoveTarget) -> Result<(), InboxError> {
        match to {
            MoveTarget::Folder(folder) => DemoSource::move_mail(self, &ids, &folder),
            MoveTarget::Role(role) => DemoSource::move_mail_to(self, &ids, role.role()),
        }
    }

    async fn delete_mail(&self, ids: Vec<String>) -> Result<(), InboxError> {
        DemoSource::move_mail_to(self, &ids, Role::Trash)
    }

    /// The demo can remove no mail: the message goes to the trash, where the demo's next list
    /// finds it.
    async fn delete_mail_for_good(&self, ids: Vec<String>) -> Result<(), InboxError> {
        DemoSource::move_mail_to(self, &ids, Role::Trash)
    }

    /// The demo has no server to fetch from.
    async fn sync_mail(&self) -> Result<(), InboxError> {
        Ok(())
    }

    /// The demo keeps no tasks of folder mail: it answers the task the core would make – the
    /// subject as title when none is given, the day as due date –, linked by the message's id (its
    /// mail has no Message-ID).
    async fn create_todo_from_mail(
        &self,
        account: String,
        id: String,
        list: TaskList,
        title: Option<String>,
        due: Option<NaiveDate>,
    ) -> Result<ItemTask, InboxError> {
        let mail = DemoSource::get_mail(self, &id)?;
        let title = title.filter(|title| !title.trim().is_empty()).unwrap_or_else(|| subject(&mail.summary));
        Ok(ItemTask {
            uid: format!("demo-task-{id}"),
            collection: list.id,
            list: list.name,
            title,
            due: due.map(TaskDue::Date),
            completed: false,
            link: format!("{account}:{}@demo", id.trim_start_matches("m:")),
            mail_entry: None,
        })
    }

    /// The demo sends nothing and keeps no calendar: an invitation of its own ([`demo_invite`]) is
    /// answered as sent, any other message is no invitation; a calendar it does not have
    /// ([`crate::hub::DEMO_CALENDARS`]) is not found.
    async fn respond_to_invitation(
        &self,
        _account: String,
        id: String,
        _answer: InvitationAnswer,
        collection: Option<String>,
    ) -> Result<Delivery, InboxError> {
        let mail = DemoSource::get_mail(self, &id)?;
        if collection.is_some_and(|wanted| !crate::hub::DEMO_CALENDARS.iter().any(|(id, _)| *id == wanted)) {
            return Err(InboxError::NotFound);
        }
        match demo_invite(&mail.summary.id, mail.summary.date) {
            Some(_) => Ok(Delivery::Accepted),
            None => Err(InboxError::Other(tr("This mail holds no invitation that can be answered.").to_owned())),
        }
    }

    /// The demo's calendar ([`demo_events`]), as far as it overlaps `[start, end)`.
    async fn list_events(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<CalendarEvent>, InboxError> {
        let lunch = DemoSource::get_mail(self, "m:w4").ok().and_then(|mail| demo_invite("m:w4", mail.summary.date));
        Ok(demo_events(lunch.as_ref()).into_iter().filter(|event| event.start < end && start < event.end).collect())
    }
}

/// The empty inbox of the tests has no mail account: no folders, every message unknown.
#[cfg(test)]
impl MailSource for crate::empty::EmptySource {
    async fn mail_folders(&self) -> Result<Vec<Folder>, InboxError> {
        Ok(Vec::new())
    }

    async fn list_categories(&self, _account: Option<String>) -> Result<Vec<CategoryInfo>, InboxError> {
        Ok(Vec::new())
    }

    async fn categorization_ready(&self) -> Result<bool, InboxError> {
        Ok(false)
    }

    async fn search_by_category(
        &self,
        _categories: Vec<torro_mail::Category>,
        _limit: u32,
        _cursor: Option<String>,
        _order: Order,
    ) -> Result<MailPage, InboxError> {
        Ok(MailPage::of(Vec::new(), None))
    }

    async fn set_mail_category(
        &self,
        _id: String,
        _category: torro_mail::Category,
        _remember: bool,
    ) -> Result<Item, InboxError> {
        Err(InboxError::Unsupported)
    }

    async fn classify_mail(&self, _id: String) -> Result<Item, InboxError> {
        Err(InboxError::Unsupported)
    }

    async fn search_mail(
        &self,
        _folder: String,
        _query: Option<String>,
        _limit: u32,
        _cursor: Option<String>,
        _order: Order,
    ) -> Result<MailPage, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn get_mail(&self, _id: String) -> Result<Mail, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn mail_attachment(&self, _id: String, _part: String) -> Result<PathBuf, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn save_targets(&self, id: Option<String>) -> Result<SaveTargets, InboxError> {
        match id {
            Some(_) => Err(InboxError::NotFound),
            None => Ok(SaveTargets::default()),
        }
    }

    async fn save_attachments(
        &self,
        _id: String,
        _parts: Vec<String>,
        _to: SaveTo,
    ) -> Result<SavedAttachments, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn set_mail_flags(&self, _ids: Vec<String>, _add: Vec<Flag>, _remove: Vec<Flag>) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }

    async fn move_mail(&self, _ids: Vec<String>, _to: MoveTarget) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }

    async fn delete_mail(&self, _ids: Vec<String>) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }

    async fn delete_mail_for_good(&self, _ids: Vec<String>) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }

    async fn sync_mail(&self) -> Result<(), InboxError> {
        Ok(())
    }

    async fn create_todo_from_mail(
        &self,
        _account: String,
        _id: String,
        _list: TaskList,
        _title: Option<String>,
        _due: Option<NaiveDate>,
    ) -> Result<ItemTask, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn respond_to_invitation(
        &self,
        _account: String,
        _id: String,
        _answer: InvitationAnswer,
        _collection: Option<String>,
    ) -> Result<Delivery, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn list_events(&self, _start: DateTime<Utc>, _end: DateTime<Utc>) -> Result<Vec<CalendarEvent>, InboxError> {
        Ok(Vec::new())
    }

    async fn find_mail(&self, _find: MailFind) -> Result<MailFound, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn mail_index_status(&self) -> Result<Vec<MailIndexProgress>, InboxError> {
        Ok(Vec::new())
    }

    async fn mail_by_role(&self, _find: MailByRole) -> Result<MailPage, InboxError> {
        Ok(MailPage::of(Vec::new(), None))
    }

    async fn unread_in_all(&self, _account: Option<String>) -> Result<u64, InboxError> {
        Ok(0)
    }

    /// No mail is hidden, so there is nothing to remember – as the core answers then.
    async fn set_mail_shown(&self, _ids: Vec<String>, _shown: bool) -> Result<(), InboxError> {
        Ok(())
    }

    /// There is no mail to put on a pile.
    async fn set_mail_pile(
        &self,
        _ids: Vec<String>,
        _pile: Option<MailPile>,
        _since: Option<DateTime<Utc>>,
    ) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }

    async fn mail_in_pile(&self, _find: MailInPile) -> Result<MailPage, InboxError> {
        Ok(MailPage::of(Vec::new(), None))
    }

    async fn count_in_pile(&self, _pile: MailPile, _account: Option<String>) -> Result<u64, InboxError> {
        Ok(0)
    }

    /// There is no mail to answer.
    async fn send_reply(&self, _id: String, _text: String) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }
}

/// The page of the demo's `found` that starts at `cursor` (the place the demo gave out, `None`: the
/// start), at most `limit` long, with the cursor of the page after it. [`InboxError::Other`] for a
/// cursor the demo did not give out, as its folders answer it.
fn demo_page(found: Vec<MessageSummary>, limit: u32, cursor: Option<&str>) -> Result<MailPage, InboxError> {
    let start = match cursor {
        None => 0,
        Some(cursor) => cursor.parse::<usize>().map_err(|_| InboxError::Other("not a cursor".into()))?,
    };
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let total = found.len();
    let messages: Vec<MessageSummary> = found.into_iter().skip(start).take(limit).collect();
    let next = (start + messages.len() < total).then(|| (start + messages.len()).to_string());
    Ok(MailPage::of(messages, next))
}

/// What the sidebar's group "Categories" shows (task E12.8): a whole group of mail categories, or
/// one category of it. Both list the mail of every account's inbox that is of one of them.
///
/// Made from the catalogue the window loaded (task E15.2): which categories a group holds and
/// what a category is called are the person's to change, so the view keeps both as they were
/// when it was opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryView {
    /// The group, which gives the entry its icon and its colour.
    pub group: CategoryGroup,
    /// One category of the group, or `None` for the whole group.
    pub only: Option<torro_mail::Category>,
    /// The categories it lists: the one chosen, else every one of the group.
    pub categories: Vec<torro_mail::Category>,
    /// What a person reads for it.
    pub label: String,
}

impl CategoryView {
    /// The whole `group`, with the categories the catalogue puts in it now.
    pub fn of_group(group: CategoryGroup, catalogue: &[CategoryInfo]) -> CategoryView {
        let categories = catalogue.iter().filter(|held| held.group == group).map(|held| held.category).collect();
        let label = torro_core::category::group_label(group).to_string();
        CategoryView { group, only: None, categories, label }
    }

    /// The one category of `held`.
    pub fn of_category(held: &CategoryInfo) -> CategoryView {
        CategoryView {
            group: held.group,
            only: Some(held.category),
            categories: vec![held.category],
            label: held.label.clone(),
        }
    }

    /// The categories this view lists: the one chosen, else every one of the group.
    pub fn categories(&self) -> Vec<torro_mail::Category> {
        self.categories.clone()
    }

    /// What a person reads for it.
    pub fn label(&self) -> String {
        self.label.clone()
    }

    /// Which listing its messages belong to, so one view's page is never shown as another's.
    pub fn listing(&self) -> String {
        match self.only {
            Some(category) => format!("category:{}", category.key()),
            None => format!("category-group:{}", self.group.key()),
        }
    }
}

/// The view of one folder: which one, with its name and role. A search in it is a view of its own
/// (`search::SearchView`, issue #136).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderView {
    /// The folder's id from the core (`f:…`).
    pub id: String,
    /// Its account, `mail:<address>`.
    pub account: String,
    /// The name to show ([`folder_name`]).
    pub name: String,
    pub role: Option<Role>,
}

impl FolderView {
    /// The folder `folder`.
    pub fn of(folder: &Folder) -> FolderView {
        FolderView {
            id: folder.id.clone(),
            account: folder.account.clone(),
            name: folder_name(folder),
            role: folder.role,
        }
    }
}

/// The folders of one account, for a group in the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountFolders<'a> {
    /// `mail:<address>`.
    pub account: &'a str,
    /// In the order of [`folder_order`].
    pub folders: Vec<&'a Folder>,
}

/// `folders` by account, the accounts in the order the core lists their first folder, each
/// account's folders in [`folder_order`].
pub fn by_account(folders: &[Folder]) -> Vec<AccountFolders<'_>> {
    let mut groups: Vec<AccountFolders<'_>> = Vec::new();
    for folder in folders {
        match groups.iter_mut().find(|group| group.account == folder.account) {
            Some(group) => group.folders.push(folder),
            None => groups.push(AccountFolders { account: &folder.account, folders: vec![folder] }),
        }
    }
    for group in &mut groups {
        group.folders.sort_by_key(|folder| folder_order(folder));
    }
    groups
}

/// Where a folder stands in its account: the folders with a role first, as mail apps list them
/// (Inbox, Drafts, Sent, Archive, Junk, Trash), then the others by name.
pub fn folder_order(folder: &Folder) -> (usize, String) {
    let place = match folder.role {
        Some(Role::Inbox) => 0,
        Some(Role::Drafts) => 1,
        Some(Role::Sent) => 2,
        Some(Role::Archive) => 3,
        Some(Role::Junk) => 4,
        Some(Role::Trash) => 5,
        None => 6,
    };
    (place, folder.name.to_lowercase())
}

/// The inbox of the first account, where a window of mail starts (`--source mail`).
pub fn first_inbox(folders: &[Folder]) -> Option<&Folder> {
    by_account(folders).into_iter().flat_map(|group| group.folders).find(|folder| folder.role == Some(Role::Inbox))
}

/// The address of an account id: `anna@example.org` for `mail:anna@example.org`.
pub fn address(account: &str) -> &str {
    account.strip_prefix("mail:").unwrap_or(account)
}

/// The name of a folder in the user's language: the role's name for the folders with a role
/// (a German server's "Entwürfe" and an English one's "Drafts" read the same), else the server's
/// name on one line.
pub fn folder_name(folder: &Folder) -> String {
    match folder.role {
        Some(Role::Inbox) => tr("Inbox").to_owned(),
        Some(Role::Drafts) => tr("Drafts").to_owned(),
        Some(Role::Sent) => tr("Sent").to_owned(),
        Some(Role::Archive) => tr("Archive").to_owned(),
        Some(Role::Junk) => tr("Junk").to_owned(),
        Some(Role::Trash) => tr("Trash").to_owned(),
        None => list::one_line(&folder.name),
    }
}

/// The icon of a folder with `role`, as in the sidebar and the panel's path.
pub fn folder_icon(role: Option<Role>) -> Icon {
    match role {
        Some(Role::Inbox) => Icon::Inbox,
        Some(Role::Drafts) => Icon::SquarePen,
        Some(Role::Sent) => Icon::Send,
        Some(Role::Archive) => Icon::Archive,
        Some(Role::Junk) => Icon::ShieldAlert,
        Some(Role::Trash) => Icon::Trash2,
        None => Icon::Folder,
    }
}

/// Whether the sidebar shows a folder's unread number: not for drafts, sent mail, junk and the
/// trash, where unread mail is no news.
pub fn counts_unread(role: Option<Role>) -> bool {
    !matches!(role, Some(Role::Drafts | Role::Sent | Role::Junk | Role::Trash))
}

/// What a button or key of the panel asks for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailAction {
    /// Write an answer to the sender (`r`).
    Reply,
    /// Write an answer to everyone on the message (`a`).
    ReplyAll,
    /// Pass it on (`f`).
    Forward,
    /// Into the account's archive (`e` in a folder).
    Archive,
    /// Into the account's trash (`#`).
    Delete,
    /// Into the account's junk folder (task E10.1, the shortcut "junk").
    Junk,
    /// Set (`true`) or clear the flag.
    Flag(bool),
    /// Mark read (`true`) or unread.
    Read(bool),
}

/// Which compose window [`compose_command`] opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compose {
    Reply,
    ReplyAll,
    Forward,
}

impl Compose {
    /// The compose window `action` opens, if it opens one.
    pub fn of(action: MailAction) -> Option<Compose> {
        match action {
            MailAction::Reply => Some(Compose::Reply),
            MailAction::ReplyAll => Some(Compose::ReplyAll),
            MailAction::Forward => Some(Compose::Forward),
            MailAction::Archive | MailAction::Delete | MailAction::Junk | MailAction::Flag(_) | MailAction::Read(_) => {
                None
            }
        }
    }

    /// The flag that names the message to answer or pass on.
    fn flag(self) -> &'static str {
        match self {
            Compose::Reply => "--reply",
            Compose::ReplyAll => "--reply-all",
            Compose::Forward => "--forward",
        }
    }
}

/// The arguments of the compose window for the message `id`: `--compose --reply <id>` (or
/// `--reply-all`, `--forward`), with `--demo` when this window runs on the demo, so the compose
/// window finds the same sample mail. The window itself is task E4.3; it reads exactly these.
pub fn compose_args(kind: Compose, id: &str, demo: bool) -> Vec<String> {
    let mut args = vec!["--compose".to_owned(), kind.flag().to_owned(), id.to_owned()];
    if demo {
        args.push("--demo".to_owned());
    }
    args
}

/// The command that opens the compose window: this app's binary next to the running one (as the
/// suite starts its programs, [`source::sibling`]) with [`compose_args`].
pub fn compose_command(kind: Compose, id: &str, demo: bool) -> Command {
    let mut command = Command::new(source::sibling(INBOX.binary));
    command.args(compose_args(kind, id, demo));
    command
}

/// Whether `mail` can show as HTML (task M9.7): it came with cleaned HTML. A mail with a text part
/// and an HTML part has it as well as one made from HTML (`from_html`).
pub fn shows_html(mail: &Mail) -> bool {
    mail.html.as_deref().is_some_and(|html| !html.trim().is_empty())
}

/// Whether a link clicked in a mail's HTML (task M9.7) opens in the browser: `http`, `https` and
/// `mailto` only (the widget reports no other; checked again here all the same).
pub fn opens_in_browser(url: &str) -> bool {
    let scheme = url.split_once(':').map(|(scheme, _)| scheme.to_ascii_lowercase());
    matches!(scheme.as_deref(), Some("http" | "https" | "mailto"))
}

/// How the window shows mail, as the settings `mail_view` and `mail_appearance` say (task M9.5),
/// and what a click on an attachment does (`mail_attachments`, task M12.6). The switches over a mail
/// start from it and go back to it whenever the setting changes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailLook {
    /// HTML or text first (`mail_view`); a mail without HTML always shows its text.
    pub format: MailFormat,
    /// The colours of the mail's content (`mail_appearance`); [`scheme_for`] resolves `app`.
    pub colors: MailAppearance,
    /// The categories the sidebar leaves out (task E12.10, `hidden_categories`): what the person
    /// switched off under Settings › Categorization.
    pub hidden_categories: Vec<torro_mail::Category>,
    /// What a click on an attachment's name does (task M12.6, `mail_attachments.click`); "Open"
    /// until the core said otherwise, and in the demo.
    pub click: AttachmentClick,
}

/// Reads [`MailLook`] from the settings of the core (`settings.get_settings`), on every tick like
/// the appearance, so a change in the control centre reaches the open window. The demo has no core
/// and keeps the defaults (HTML, like the app).
pub async fn stored_look(suite: &source::Source) -> Result<MailLook, String> {
    let source::Source::Core(client) = suite else {
        return Ok(MailLook::default());
    };
    match torro_ipc::client::call(&client.socket, &Request::Settings).await {
        Ok(Response::Settings { settings }) => Ok(MailLook {
            format: settings.mail_view,
            colors: settings.mail_appearance,
            hidden_categories: settings.hidden_categories,
            click: settings.mail_attachments.click,
        }),
        Ok(_) => Err("unexpected answer to settings.get_settings".to_owned()),
        Err(err) => Err(err.to_string()),
    }
}

/// The colours a mail's content is drawn in for `colors`: light and dark as they say, `app` like
/// the window (`dark`: the window's theme is dark – "System" already resolved by the theme).
pub fn scheme_for(colors: MailAppearance, dark: bool) -> Scheme {
    match colors {
        MailAppearance::Light => Scheme::Light,
        MailAppearance::Dark => Scheme::Dark,
        MailAppearance::App if dark => Scheme::Dark,
        MailAppearance::App => Scheme::Light,
    }
}

/// The HTML of the mail held by the window, parsed for drawing (task M9.7). Lives as long as the
/// mail is open: dropping it stops the pictures still loading. Not `Send`: it stays with the window.
#[derive(Debug)]
pub struct HtmlMail {
    /// The message it belongs to.
    pub id: String,
    pub content: Content,
}

impl HtmlMail {
    /// Parses the HTML of `mail` (`None` without HTML, [`shows_html`]) and starts loading its
    /// pictures: `cid:` ones from `inbox` ([`MailPictures`]) when the mail has parts with a
    /// Content-ID, web ones unless `offline`. The demo and the tests are offline: they never touch
    /// the network.
    pub fn new<S: MailSource + 'static>(mail: &Mail, inbox: &Arc<S>, offline: bool) -> Option<HtmlMail> {
        let html = mail.html.as_deref().filter(|_| shows_html(mail))?;
        let pictures = MailPictures::of(mail, inbox).map(|pictures| Arc::new(pictures) as Arc<dyn CidSource>);
        let content = match (offline, pictures) {
            (true, pictures) => Content::offline(html, pictures),
            (false, Some(pictures)) => Content::with_cid(html, pictures),
            (false, None) => Content::new(html),
        };
        Some(HtmlMail { id: mail.summary.id.clone(), content })
    }
}

/// The pictures a mail carries with it (`cid:`), from the core: `mail.get_attachment` places the
/// part whose Content-ID matches, then its file is read. Only the parts of this one mail.
pub struct MailPictures<S> {
    inbox: Arc<S>,
    /// The message.
    id: String,
    /// `(content id, part)` of every part of the mail that has a Content-ID.
    parts: Vec<(String, String)>,
}

impl<S: MailSource + 'static> MailPictures<S> {
    /// The pictures of `mail` from `inbox`; `None` when no part of it has a Content-ID.
    pub fn of(mail: &Mail, inbox: &Arc<S>) -> Option<MailPictures<S>> {
        let parts: Vec<(String, String)> =
            mail.attachments.iter().filter_map(|file| Some((file.content_id.clone()?, file.part.clone()))).collect();
        (!parts.is_empty()).then(|| MailPictures { inbox: Arc::clone(inbox), id: mail.summary.id.clone(), parts })
    }
}

impl<S: MailSource + 'static> CidSource for MailPictures<S> {
    /// Runs on the picture fetcher's worker thread: it may wait for the core and the disk.
    fn picture(&self, content_id: &str) -> Option<Vec<u8>> {
        let part = self.parts.iter().find(|(id, _)| id == content_id).map(|(_, part)| part.clone())?;
        let path = wait_for(self.inbox.mail_attachment(self.id.clone(), part))?.ok()?;
        read_picture(&path)
    }
}

/// The bytes of the file at `path`, or `None` when it cannot be read or is larger than a picture
/// may be ([`MAX_PICTURE_BYTES`], the library's own limit): a huge part is never read into memory
/// only to be refused afterwards. The read stops one byte past the limit, so a file that grows
/// between the check and the read is refused too.
fn read_picture(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_PICTURE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_PICTURE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_PICTURE_BYTES).then_some(bytes)
}

/// Waits for `future` on this (blocking) thread: on the runtime it was started from when there is
/// one – the fetcher's, whose reactor serves the core's socket –, else on a small one of its own.
/// `None` when no runtime can be had.
fn wait_for<F: Future>(future: F) -> Option<F::Output> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => Some(handle.block_on(future)),
        Err(_) => {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
            Some(runtime.block_on(future))
        }
    }
}

/// Starts `command` and lets it run on its own; a thread waits for it, so it leaves no zombie.
pub fn start(mut command: Command) -> Result<(), String> {
    let mut child = command.spawn().map_err(|err| err.to_string())?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// One change of messages, one [`MailSource`] call each. The panel's buttons, the keys and every
/// undo end here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailEdit {
    /// Set `add` and clear `remove` (`mail.set_flags`).
    Flags { ids: Vec<String>, add: Vec<Flag>, remove: Vec<Flag> },
    /// Move to a folder or the folder of a role (`mail.move_messages`).
    Move { ids: Vec<String>, to: MoveTarget },
    /// Into the trash (`mail.delete_messages`).
    Delete { ids: Vec<String> },
    /// Removed for good (task E10.1, `mail.delete_messages` with `permanent`), no undo.
    DeleteForGood { ids: Vec<String> },
}

impl MailEdit {
    /// The messages it changes.
    pub fn ids(&self) -> &[String] {
        match self {
            MailEdit::Flags { ids, .. }
            | MailEdit::Move { ids, .. }
            | MailEdit::Delete { ids }
            | MailEdit::DeleteForGood { ids } => ids,
        }
    }

    /// Sends the edit to `source`; the error as people read it.
    pub async fn apply<S: MailSource>(self, source: Arc<S>) -> Result<(), String> {
        let result = match self {
            MailEdit::Flags { ids, add, remove } => source.set_mail_flags(ids, add, remove).await,
            MailEdit::Move { ids, to } => source.move_mail(ids, to).await,
            MailEdit::Delete { ids } => source.delete_mail(ids).await,
            MailEdit::DeleteForGood { ids } => source.delete_mail_for_good(ids).await,
        };
        result.map_err(|err| err.to_string())
    }
}

/// The edit `action` makes of the message `id`; `None` for the actions that write (they open the
/// compose window instead).
pub fn edit_of(action: MailAction, id: &str) -> Option<MailEdit> {
    let ids = vec![id.to_owned()];
    let flag = |flag: Flag, set: bool| {
        let (add, remove) = if set { (vec![flag], Vec::new()) } else { (Vec::new(), vec![flag]) };
        MailEdit::Flags { ids: ids.clone(), add, remove }
    };
    match action {
        MailAction::Archive => Some(MailEdit::Move { ids: ids.clone(), to: MoveTarget::Role(MoveRole::Archive) }),
        MailAction::Delete => Some(MailEdit::Delete { ids: ids.clone() }),
        MailAction::Junk => Some(MailEdit::Move { ids: ids.clone(), to: MoveTarget::Role(MoveRole::Junk) }),
        MailAction::Flag(set) => Some(flag(Flag::Flagged, set)),
        MailAction::Read(read) => Some(flag(Flag::Seen, read)),
        MailAction::Reply | MailAction::ReplyAll | MailAction::Forward => None,
    }
}

/// Where a message of a row or card can be filed (issue #118): the buttons Archive and Delete for
/// the message `id` of `account` (`mail:<address>`) lying in the folder `folder` (`None`: its
/// account's inbox, where an entry's mail lies), in this order. Each sends
/// [`Message::Mail`] – what `e` in a folder and `#` do – so the move, its toast and its undo are
/// those of the keys (`App::mail_action`).
///
/// A button shows only where it can work, as every action bar does (skill `torro-design`, "Nur
/// Aktionen, die für den Eintrag gehen"): Archive needs a folder of the role Archive in the account
/// and a message outside it, Delete a Trash and a message outside the trash – otherwise the core
/// answers `NO_ROLE_FOLDER` or there is nothing to do. `archive_key` is the key shown in Archive's
/// tooltip: `"E"` wherever `e` archives – in a folder, and at a mail entry since task E18.5.
pub fn filing_actions<'a>(
    folders: &[Folder],
    account: &str,
    folder: Option<&str>,
    id: &str,
    archive_key: &'static str,
) -> Vec<RowAction<'a, Message>> {
    let filing = Filing::of(folders, account, folder);
    let mut actions = Vec::new();
    if filing.archive {
        let archive = Message::Mail(MailAction::Archive, id.to_owned());
        actions.push(RowAction::new(Icon::Archive, trc("action", "Archive"), archive_key, archive));
    }
    if filing.delete {
        actions.push(RowAction::new(Icon::Trash2, tr("Delete"), "#", Message::Mail(MailAction::Delete, id.to_owned())));
    }
    actions
}

/// Where a message can be filed, by the rule of [`filing_actions`]: its rows and cards and the mail's
/// action rail (task E14.3, [`action_groups`]) offer the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Filing {
    /// The account has a folder of the role Archive and the message lies outside it.
    archive: bool,
    /// The account has a Trash and the message lies outside it.
    delete: bool,
    /// The message can be followed up: it lies outside the archive, the trash and spam
    /// ([`follows_up`]).
    follow_up: bool,
    /// The message can go on a pile: it lies outside the trash and spam ([`takes_piles`]).
    piles: bool,
}

impl Filing {
    /// For the message of `account` (`mail:<address>`) in the folder `folder` (`None`: its account's
    /// inbox), among every account's `folders`.
    fn of(folders: &[Folder], account: &str, folder: Option<&str>) -> Filing {
        let role_of = |folder: &str| folders.iter().find(|f| f.id == folder).and_then(|f| f.role);
        let lies_in = folder.map_or(Some(Role::Inbox), role_of);
        let has = |role: Role| folders.iter().any(|f| f.account == account && f.role == Some(role));
        Filing {
            archive: has(Role::Archive) && lies_in != Some(Role::Archive),
            delete: has(Role::Trash) && lies_in != Some(Role::Trash),
            follow_up: !matches!(lies_in, Some(Role::Archive | Role::Trash | Role::Junk)),
            piles: !matches!(lies_in, Some(Role::Trash | Role::Junk)),
        }
    }
}

/// Whether a mail in the folder `folder` (a [`Folder::id`]) can go on "Reply later" or "Set aside"
/// (review of PR #327): not in the trash or spam, whose mail a pile leaves out (decision 3 of E19); the
/// archive can – setting archived mail aside is the main case. A folder not known, as one without a
/// role, can.
pub fn takes_piles(folders: &[Folder], folder: &str) -> bool {
    let role = folders.iter().find(|held| held.id == folder).and_then(|held| held.role);
    !matches!(role, Some(Role::Trash | Role::Junk))
}

/// Whether a mail in the folder `folder` (a [`Folder::id`]) can be followed up (decision of #315):
/// not in the archive, the trash or spam – archived counts as done (decision 6 of E19), so a
/// follow-up there would never come back in the mail window. A folder not known, as one without a
/// role, can.
pub fn follows_up(folders: &[Folder], folder: &str) -> bool {
    let role = folders.iter().find(|held| held.id == folder).and_then(|held| held.role);
    !matches!(role, Some(Role::Archive | Role::Trash | Role::Junk))
}

/// The hint after archiving (`delete: false`) or deleting.
pub fn moved_text(delete: bool) -> &'static str {
    if delete { tr("Moved to Trash") } else { tr("Archived") }
}

/// `flags` after `edit` (only [`MailEdit::Flags`] changes them), sorted as the core keeps them.
pub fn flags_after(flags: &[Flag], edit: &MailEdit) -> Vec<Flag> {
    let mut flags = flags.to_vec();
    if let MailEdit::Flags { add, remove, .. } = edit {
        flags.retain(|flag| !remove.contains(flag));
        flags.extend(add.iter().filter(|flag| !flags.contains(flag)).copied().collect::<Vec<_>>());
        flags.sort();
    }
    flags
}

/// Whether a file of this name could run a program when the system opens it (scripts, installers,
/// programs, shortcuts, desktop entries). Such attachments are never opened from mail; their
/// folder is shown instead, so the person decides.
pub fn runs_programs(name: &str) -> bool {
    const RISKY: [&str; 30] = [
        "app", "appimage", "apk", "bat", "bin", "cmd", "com", "command", "cpl", "deb", "desktop", "dmg", "exe", "hta",
        "jar", "js", "jse", "lnk", "msi", "pkg", "ps1", "py", "rpm", "run", "scr", "sh", "url", "vbs", "workflow",
        "wsf",
    ];
    let extension = Path::new(name).extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase);
    extension.is_none_or(|ext| RISKY.contains(&ext.as_str()))
}

/// The command that opens `path` with the system's program for it: `open` on macOS, `xdg-open`
/// on Linux, the Explorer on Windows.
pub fn opener(path: &Path) -> Command {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    let mut command = Command::new(program);
    command.arg(path);
    command
}

/// What became of an attachment ([`open_attachment`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// Opened with the system's program for it.
    File(PathBuf),
    /// A file that could run a program ([`runs_programs`]): its folder was opened instead.
    Folder(PathBuf),
}

/// Fetches the attachment `part` of the message `id` through `source` and opens it with the
/// system – or, for a file that could run a program, its folder.
pub async fn open_attachment<S: MailSource>(source: Arc<S>, id: String, part: String) -> Result<Opened, String> {
    let path = source.mail_attachment(id, part).await.map_err(|err| err.to_string())?;
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    if runs_programs(&name) {
        let folder = path.parent().map_or_else(|| path.clone(), Path::to_path_buf);
        start(opener(&folder))?;
        return Ok(Opened::Folder(path));
    }
    start(opener(&path))?;
    Ok(Opened::File(path))
}

/// `name <address>`, or the address alone, on one line.
pub fn person(who: &MailAddress) -> String {
    match who.name.as_deref().map(list::one_line).filter(|name| !name.is_empty()) {
        Some(name) => format!("{name} <{}>", list::one_line(&who.address)),
        None => list::one_line(&who.address),
    }
}

/// The name to show for a sender in a list: the display name, else the address.
pub fn sender_name(from: Option<&MailAddress>) -> String {
    from.map(|who| who.name.as_deref().map(list::one_line).filter(|name| !name.is_empty()))
        .and_then(|name| name.or_else(|| from.map(|who| list::one_line(&who.address))))
        .unwrap_or_else(|| tr("Unknown sender").to_owned())
}

/// The names of `people`, comma separated.
fn names(people: &[MailAddress]) -> String {
    people.iter().map(|who| sender_name(Some(who))).collect::<Vec<_>>().join(", ")
}

/// A size in bytes as people read it: "860 B", "3 KB", "12 MB".
pub fn size_text(bytes: u64) -> String {
    const KB: u64 = 1024;
    if bytes < KB {
        trf("{n} B", &[("n", &bytes)])
    } else if bytes < KB * KB {
        trf("{n} KB", &[("n", &bytes.div_ceil(KB))])
    } else {
        trf("{n} MB", &[("n", &bytes.div_ceil(KB * KB))])
    }
}

/// The subject on one line, or "(no subject)".
pub fn subject(summary: &MessageSummary) -> String {
    let subject = list::one_line(&summary.subject);
    if subject.is_empty() { tr("(no subject)").to_owned() } else { subject }
}

/// What the message list gets to draw.
pub struct MailList<'a> {
    pub messages: &'a [MessageSummary],
    /// The catalogue of categories (task E15.2): what a mail's category is called and where it
    /// stands now.
    pub categories: &'a [CategoryInfo],
    /// The view shown: a folder, the smart view of attachments, or a window of mail waiting for
    /// its folders ([`View::Mail`]).
    pub view: &'a View,
    /// The mail accounts have no folders at all (no account connected).
    pub no_accounts: bool,
    /// Id of the message under the cursor.
    pub selected: Option<&'a str>,
    /// Additional messages chosen with Ctrl/Cmd, Shift or Select All.
    pub selected_ids: &'a [String],
    pub now: DateTime<Utc>,
    /// Registered by the selected row (`row`) and its buttons that open a menu (issue #122).
    pub anchors: detail::Anchors,
    /// The addresses of the mail accounts in the sidebar's order: the avatars take the colour of
    /// the folder's account, as in the overview (`list::account_index`).
    pub accounts: Vec<&'a str>,
    /// More messages wait on the server: "Load more" ends the list.
    pub more: bool,
    /// The attention colour of the theme, for the flag.
    pub attention: Color,
    /// Rows or cards (task E8.3).
    pub look: Look,
    /// The open entries of "For me" (task E8.11): an invitation's card whose mail has its entry here
    /// ([`entry_of`]) offers that entry's "Later" and "…", as the same mail does under "For me".
    pub entries: &'a [Item],
    /// Every folder of every account: Archive and Delete show only where the message's account has
    /// such a folder ([`filing_actions`], issue #118).
    pub folders: &'a [Folder],
    /// Where the words of a search matched (issue #136): the rows and cards of a search show their
    /// subject and passage with the matches stressed (`list::Hits`); empty elsewhere.
    pub highlights: &'a [MailHighlight],
    /// The text colour of the theme, for the matches in a passage.
    pub text: Color,
    /// Rows and cards without buttons (task E14.2, as `list::Rows::bare`): `true` in the mail window –
    /// the selected row shows its preview and is as high as unchosen, its menus hang at the row –,
    /// else (a search of mail in the hub) the strip of buttons where the text starts ([`message_buttons`]).
    /// Cards show only in the mail window and never carry buttons.
    pub bare: bool,
}

impl<'a> MailList<'a> {
    /// The matches of the message `id`, if the search has some for it.
    fn hits(&self, id: &str) -> Option<list::Hits<'a>> {
        let highlight = self.highlights.iter().find(|highlight| highlight.id == id)?;
        Some(list::Hits { highlight, text: self.text })
    }
}

/// The open entry of `entries` that holds the message `id` (it links to it, [`Link::Mail`]).
pub fn entry_of<'a>(entries: &'a [Item], id: &str) -> Option<&'a Item> {
    entries
        .iter()
        .find(|item| matches!(&item.link, Some(Link::Mail { message }) if message == id) && item.state.is_open())
}

/// A folder's messages, scrolling, with "Load more" at the end when there are more; the empty
/// state of the folder or the search when there are none.
pub fn list<'a>(data: MailList<'a>) -> Element<'a, Message> {
    if data.messages.is_empty() {
        let (which, title, hint) = match data.view {
            _ if data.no_accounts => {
                (Icon::Mail, tr("No mail account yet."), tr("Connect one in Settings, under Accounts."))
            }
            View::Search(search) => crate::search::empty_texts(search),
            View::Folder(folder) => {
                (folder_icon(folder.role), tr("No messages here."), tr("Mail in this folder shows up here."))
            }
            // The views of every account's mail (task E18.4).
            View::All => (Icon::Mail, tr("No mail in your inboxes."), tr("New mail shows up here.")),
            View::Hidden => (Icon::EyeOff, tr("No hidden mail."), tr("Mail you hide from the inbox shows up here.")),
            View::Archive => (Icon::Archive, tr("No archived mail."), tr("Mail you archive shows up here.")),
            // The piles (task E19.4): the hint names the key that fills them (`w`, `b`).
            View::ReplyLater => (
                Icon::MessageSquareReply,
                tr("Nothing to reply to later."),
                tr("Press W on a mail to answer it later."),
            ),
            View::SetAside => (Icon::Bookmark, tr("Nothing set aside."), tr("Press B on a mail to set it aside.")),
            _ => return widgets::loading(),
        };
        return list::empty_state(which, title, hint);
    }
    responsive(move |bounds| rows(&data, bounds.width)).into()
}

/// The rows at `width`, or a column of cards.
fn rows<'a>(data: &MailList<'a>, width: f32) -> Element<'a, Message> {
    let mut rows = column![].width(Fill);
    if data.look.cards() {
        let inner = width - list::cards_side();
        let cards = data.messages.iter().map(|message| message_card(data, message, inner));
        rows = rows.push(container(column(cards).spacing(size::card_gap())).padding(list::cards_inset()));
    } else {
        rows = rows.extend(data.messages.iter().map(|message| message_row(data, message, width)));
    }
    if data.more {
        let more = widgets::text_button(None, tr("Load more"), style::button::ghost, Some(Message::MoreMail));
        rows = rows.push(container(more).width(Fill).center_x(Fill).padding(space::s()));
    }
    scrollable(rows)
        .id(list::SCROLL)
        .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y, viewport.bounds().height))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Fill)
        .height(Fill)
        .into()
}

/// One message in a row `width` wide: the mail row of the overview (task E3.11,
/// `list::mail_content`) – avatar with the sender's initials in the account's colour, sender ·
/// domain · paperclip · flag · time, the subject (medium weight while unread, with the dot at the
/// avatar), the start of the text –, [`list::message_height`] high. Under the cursor its action bar
/// (issue #118, [`row_buttons`]) hangs below its lines, as an entry's row – not in the mail window,
/// whose rows are bare (task E14.2, [`MailList::bare`]): there the row stays as high as any.
fn message_row<'a>(data: &MailList<'a>, message: &'a MessageSummary, width: f32) -> Element<'a, Message> {
    let mut marks: Vec<(Element<'a, Message>, f32)> = Vec::new();
    // What the mail is (task E12.8), before the other marks: it says most about the row.
    marks.extend(message.category.map(|held| category_mark(held, data.categories)));
    // An invitation (task E8.3b): its time and the answers stand on its card and in the reader.
    marks.extend(message.invite.as_ref().map(|invite| invite_mark(invite, data.now)));
    if message.has_attachments {
        marks.push((widgets::with_tooltip(small_icon(Icon::Paperclip, None), tr("Has attachments")), icon::SIZE_SM));
    }
    if message.flags.contains(&Flag::Flagged) {
        let flag = widgets::with_tooltip(small_icon(Icon::Flag, Some(data.attention)), tr("Flagged"));
        marks.push((flag, icon::SIZE_SM));
    }
    let (name, domain) = sender_parts(message.from.as_ref());
    let account = data.accounts.iter().position(|account| *account == address(&message.account));
    let snippet = Some(list::one_line(&message.snippet)).filter(|snippet| !snippet.is_empty());
    let lines = list::MailLines {
        name,
        domain,
        time: list::stamp(message.date, data.now),
        marks,
        subject: subject(message),
        new: !message.flags.contains(&Flag::Seen),
        count: None,
        preview: snippet,
        account,
        account_name: None,
        hits: data.hits(&message.id),
    };
    let selected = (data.selected_ids.is_empty() && data.selected == Some(message.id.as_str()))
        || data.selected_ids.contains(&message.id);
    let on_press = Message::SelectMail(message.id.clone());
    let actions = data.anchors.mark(true, row_buttons(data, message));
    // Below the lines, as the rows of entries (`list::entry_row`): nothing covers the row.
    let strip = if actions.is_empty() { 0.0 } else { list::strip_height() };
    let content = list::with_strip(list::mail_content(lines, width, selected), actions, width, Some(selected));
    let row: Element<'a, Message> = mouse_area(list::sized_row(
        content,
        selected,
        Some(!message.flags.contains(&Flag::Seen)),
        on_press,
        list::message_height() + strip,
    ))
    .on_right_press(Message::ContextMail(message.id.clone()))
    .into();
    // The menus and dialogs of the keys hang at the selected row – in a bare list it has nothing else.
    if data.selected == Some(message.id.as_str()) { widgets::anchored(&data.anchors.row, row) } else { row }
}

/// What the strip of `message`'s row holds ([`message_row`]): under the cursor its buttons
/// with the bell last ([`message_buttons`]); nothing for the other rows and for every bare row
/// ([`MailList::bare`], the mail window, task E14.2).
fn row_buttons<'a>(data: &MailList<'_>, message: &MessageSummary) -> Vec<RowAction<'a, Message>> {
    if data.bare || data.selected != Some(message.id.as_str()) {
        return Vec::new();
    }
    let (mut buttons, more) = message_buttons(message, data.entries, data.folders, data.now);
    buttons.extend(more);
    buttons
}

/// The card of a message `width` wide (task E8.3, `list::mail_card`): the lines of its row and, as
/// chips, "Attachments" and "Flagged"; no buttons (task E14.2). An invitation (task E8.3b)
/// shows when it is and the answer given as chips. In the smart views, which hold every account's
/// mail, the head names the account.
fn message_card<'a>(data: &MailList<'a>, message: &'a MessageSummary, width: f32) -> Element<'a, Message> {
    let mut chips = Vec::new();
    // What the mail is (task E12.8): the whole pill, where the card has room for it.
    chips.extend(message.category.map(|held| category_pill(held, data.categories)));
    if let Some(invite) = &message.invite {
        chips.extend(invite_chips(invite, data.now));
    }
    if message.has_attachments {
        chips.push(widgets::state_pill(Icon::Paperclip, tr("Attachments")));
    }
    if message.flags.contains(&Flag::Flagged) {
        chips.push(widgets::state_pill(Mark::Tinted(Icon::Flag, data.attention), tr("Flagged")));
    }
    let (name, domain) = sender_parts(message.from.as_ref());
    let account = data.accounts.iter().position(|account| *account == address(&message.account));
    let several =
        matches!(data.view, View::Search(_) | View::All | View::Hidden | View::Archive) && data.accounts.len() > 1;
    let lines = list::MailLines {
        name,
        domain,
        time: list::stamp(message.date, data.now),
        marks: Vec::new(),
        subject: subject(message),
        new: !message.flags.contains(&Flag::Seen),
        count: None,
        preview: Some(list::one_line(&message.snippet)).filter(|snippet| !snippet.is_empty()),
        account,
        account_name: several.then(|| account_name(&message.account)),
        hits: data.hits(&message.id),
    };
    let selected = (data.selected_ids.is_empty() && data.selected == Some(message.id.as_str()))
        || data.selected_ids.contains(&message.id);
    let card = list::MailCard { lines, chips, selected, on_press: Message::SelectMail(message.id.clone()) };
    let card: Element<'a, Message> =
        mouse_area(list::mail_card(card, width)).on_right_press(Message::ContextMail(message.id.clone())).into();
    if data.selected == Some(message.id.as_str()) { widgets::anchored(&data.anchors.row, card) } else { card }
}

/// The action bar of the row of `message` at `now` outside the mail window (a search of mail in the
/// hub, [`MailList::bare`] `false`), with the bell "Mute…" (`m`, issue #123) as the second part:
///
/// - a message: Archive (`e`, what "done" is in a folder) and Delete (`#`, into the trash) as far as
///   its account has those folders ([`filing_actions`], issue #118), then Task (`t`,
///   `tasks.create_todo_from_mail`, task E8.7b); a message has no "Later" of its own. No Reply:
///   from a list the mail is not read yet – Reply stands in the reader. The bell acts on the
///   message: its entry's menu when it has one, else the message's own traits;
/// - an invitation whose event is not over (task E8.3b): Accept · Maybe · Decline
///   ([`Message::AnswerInvitation`]) · Later · Task, as the same mail under "For me" (task E8.11):
///   "Later" and the bell act on its entry in `entries` ([`entry_of`], `Message::Act` with its id);
///   without an open entry, only the answers and Task, and the bell on the message.
pub fn message_buttons<'a>(
    message: &MessageSummary,
    entries: &[Item],
    folders: &[Folder],
    now: DateTime<Utc>,
) -> (Vec<RowAction<'a, Message>>, Option<RowAction<'a, Message>>) {
    let id = || message.id.clone();
    // `Act`, as `t` at the row: the dialog then opens at this button, not at the reader's (issue #122).
    let task = RowAction::new(Icon::ListTodo, tr("Task"), "T", Message::Act(id(), Key::MakeTask));
    let bell = |id: String| RowAction::new(Icon::BellOff, tr("Mute…"), "M", Message::Act(id, Key::Hide));
    if message.invite.as_ref().is_none_or(|invite| invite_over(invite, now)) {
        let mut buttons = filing_actions(folders, &message.account, Some(&message.folder), &message.id, "E");
        buttons.push(task);
        return (buttons, Some(bell(id())));
    }
    let answer = |(icon, word, answer): (Icon, &'static str, InvitationAnswer)| {
        RowAction::new(icon, word, "", Message::AnswerInvitation(message.account.clone(), id(), answer))
    };
    let mut buttons: Vec<_> = answers().into_iter().map(answer).collect();
    let entry = entry_of(entries, &message.id);
    let act = |item: &Item, command| Message::Act(item.id.clone(), command);
    buttons.extend(entry.map(|item| RowAction::new(Icon::Clock, tr("Later"), "S", act(item, Key::Snooze))));
    buttons.push(task);
    (buttons, Some(bell(entry.map_or_else(id, |item| item.id.clone()))))
}

/// The height of the card of `message`, `width` wide ([`message_card`]): its preview takes the lines
/// it needs at that width (task E8.11, `list::preview_lines`).
pub fn message_card_height(message: &MessageSummary, width: f32) -> f32 {
    let chips = message.has_attachments || message.flags.contains(&Flag::Flagged) || message.invite.is_some();
    let snippet = Some(list::one_line(&message.snippet)).filter(|snippet| !snippet.is_empty());
    list::mail_card_height(list::preview_lines(snippet.as_deref(), width), chips)
}

/// The reader of the wide mail window while no mail is open (task E8.11): the column beside the
/// list says what shows up there, so the list keeps its width and its edge.
pub fn empty_reader<'a>() -> Element<'a, Message> {
    list::empty_state(Icon::Mail, tr("Select a mail to read it"), tr("Its text and attachments show up here."))
}

/// The name of the account `mail:<address>` on a card: the domain of its address, as a row names
/// an entry's account (`list::account_label`).
fn account_name(account: &str) -> String {
    let address = address(account);
    torro_inbox::rules::domain_of(address).unwrap_or_else(|| list::one_line(address))
}

/// The demo's invitations (task E8.3b): Maria's "Lunch next week?" (`m:w4`) invites to lunch on the
/// Tuesday of next week at 12:00 for an hour, Paul's "Weekend plans" (`m:p1`) to a hike all
/// Saturday. Counted from the demo's "now" (the mail's `date` plus how long before "now" the demo
/// dated it), so `--now` gives the same pictures. `None` for every other message `id`. The entries
/// of these mails carry them too (task E8.3c, [`demo_entry_invite`]).
fn demo_invite(id: &str, date: DateTime<Utc>) -> Option<Invite> {
    let (title, before, all_day) = match id {
        "m:w4" => (tr("Lunch with Maria"), chrono::TimeDelta::hours(26), false),
        "m:p1" => (tr("Hike"), chrono::TimeDelta::minutes(50), true),
        _ => return None,
    };
    let today = (date + before).with_timezone(&Local).date_naive();
    let (start, end) = if all_day {
        // The coming Saturday, a week on when today is one.
        let ahead = match (12 - chrono::Datelike::weekday(&today).num_days_from_monday()) % 7 {
            0 => 7,
            days => days,
        };
        let saturday = today + chrono::Days::new(u64::from(ahead));
        let midnight = |day: NaiveDate| day.and_time(chrono::NaiveTime::MIN).and_utc();
        (midnight(saturday), midnight(saturday + chrono::Days::new(1)))
    } else {
        // The Tuesday of next week.
        let ahead = 8 - chrono::Datelike::weekday(&today).num_days_from_monday();
        let tuesday = today + chrono::Days::new(u64::from(ahead));
        let noon = datetime::to_utc(tuesday.and_hms_opt(12, 0, 0).unwrap_or_default());
        (noon, noon + chrono::TimeDelta::hours(1))
    };
    Some(Invite { title: title.to_owned(), start, end: Some(end), all_day, answered: None })
}

/// The invitation the core would put on the demo's entry `item` (task E8.3c, [`Item::invite`]): the
/// one of the message it links to ([`demo_invite`]; the demo's entries are dated as their mail).
pub fn demo_entry_invite(item: &Item) -> Option<Invite> {
    match &item.link {
        Some(Link::Mail { message }) => demo_invite(message, item.updated),
        _ => None,
    }
}

/// The demo's calendar around its invitations (task E8.3c): the busy "Team sync" from half past
/// twelve on the day of Maria's lunch (`lunch`) for an hour, so lunch overlaps it, and a free "Focus
/// time" before it, which overlaps nothing. The hike on Saturday has the day to itself.
fn demo_events(lunch: Option<&Invite>) -> Vec<CalendarEvent> {
    let Some(lunch) = lunch else { return Vec::new() };
    let sync = lunch.start + chrono::TimeDelta::minutes(30);
    let focus = lunch.start - chrono::TimeDelta::hours(2);
    vec![
        CalendarEvent { title: tr("Focus time").to_owned(), start: focus, end: lunch.start, busy: false },
        CalendarEvent {
            title: tr("Team sync").to_owned(),
            start: sync,
            end: sync + chrono::TimeDelta::hours(1),
            busy: true,
        },
    ]
}

/// The event of `invite` is over at `now`: its end (its start without one) has passed. Such an
/// invitation is not answered any more (the core says `error`).
pub fn invite_over(invite: &Invite, now: DateTime<Utc>) -> bool {
    invite.end.unwrap_or(invite.start) <= now
}

/// When the event of `invite` is, in local time: "Tue 20 Oct · 12:00–13:00", for a whole day "Sat
/// 17 Oct", for several days "Sat 17 Oct – Sun 18 Oct" (the end of an all-day event is the day after
/// its last).
pub fn invite_when(invite: &Invite, now: DateTime<Utc>) -> String {
    let today = now.with_timezone(&Local).date_naive();
    if invite.all_day {
        let first = invite.start.date_naive();
        let last = invite.end.map_or(first, |end| (end - chrono::TimeDelta::days(1)).date_naive()).max(first);
        let day = |day: NaiveDate| datetime::format_short_date(day, today);
        return if last == first { day(first) } else { format!("{} – {}", day(first), day(last)) };
    }
    let start = datetime::to_local(invite.start);
    let day = datetime::format_short_date(start.date(), today);
    match invite.end.map(datetime::to_local) {
        Some(end) if end.date() == start.date() => {
            format!("{day} · {}–{}", datetime::format_time(start.time()), datetime::format_time(end.time()))
        }
        Some(end) => format!(
            "{day} · {} – {} {}",
            datetime::format_time(start.time()),
            datetime::format_short_date(end.date(), today),
            datetime::format_time(end.time())
        ),
        None => format!("{day} · {}", datetime::format_time(start.time())),
    }
}

/// The words of an answer given: "Accepted", "Maybe", "Declined".
pub fn answer_word(answer: InvitationAnswer) -> &'static str {
    match answer {
        InvitationAnswer::Accept => tr("Accepted"),
        InvitationAnswer::Tentative => tr("Maybe"),
        InvitationAnswer::Decline => tr("Declined"),
    }
}

/// The mark of an answer given: a check, a question, a cross – in `text_3` like every state pill;
/// the word says which.
fn answer_icon(answer: InvitationAnswer) -> Icon {
    match answer {
        InvitationAnswer::Accept => Icon::CircleCheck,
        InvitationAnswer::Tentative => Icon::CircleDashed,
        InvitationAnswer::Decline => Icon::CircleX,
    }
}

/// The buttons that answer an invitation, in their order: Accept, Maybe, Decline – each its icon,
/// its word and the answer. Mail cards, entry cards and rows (task E8.3c) and the invitation's card
/// use the same three.
pub(crate) fn answers() -> [(Icon, &'static str, InvitationAnswer); 3] {
    [
        (Icon::Check, tr("Accept"), InvitationAnswer::Accept),
        (Icon::CircleDashed, tr("Maybe"), InvitationAnswer::Tentative),
        (Icon::X, tr("Decline"), InvitationAnswer::Decline),
    ]
}

/// The chips of an invitation on a card: when it is ([`invite_when`]) with the calendar's icon, and
/// the answer given, if any. Mail cards and entry cards (task E8.3c) alike.
pub(crate) fn invite_chips<'a>(invite: &Invite, now: DateTime<Utc>) -> Vec<Element<'a, Message>> {
    let mut chips = vec![widgets::state_pill(Icon::Calendar, invite_when(invite, now))];
    if let Some(answer) = invite.answered {
        chips.push(widgets::state_pill(answer_icon(answer), answer_word(answer)));
    }
    chips
}

/// The group-header icon and its place in `Palette::category` (task E12.8). Category rows use
/// [`category_symbol`] in this group's colour; the group icon is deliberately not a folder icon
/// ([`folder_icon`]): "Inbox" the folder and "Waiting for me" the group stand in one sidebar.
pub(crate) fn group_mark(group: CategoryGroup) -> (Icon, usize) {
    match group {
        CategoryGroup::WaitingForMe => (Icon::UserCheck, 0),
        CategoryGroup::PaperTrail => (Icon::TextQuote, 1),
        CategoryGroup::Info => (Icon::Info, 2),
        CategoryGroup::Junk => (Icon::ShieldAlert, 3),
    }
}

/// The monochrome mark of one category; its group supplies the colour.
pub(crate) fn category_symbol(icon: torro_mail::CategoryIcon) -> Icon {
    match icon {
        torro_mail::CategoryIcon::MessageCircle => Icon::MessageCircle,
        torro_mail::CategoryIcon::ListTodo => Icon::ListTodo,
        torro_mail::CategoryIcon::CircleCheck => Icon::CircleCheck,
        torro_mail::CategoryIcon::UserCheck => Icon::UserCheck,
        torro_mail::CategoryIcon::ShieldAlert => Icon::ShieldAlert,
        torro_mail::CategoryIcon::User => Icon::User,
        torro_mail::CategoryIcon::CalendarPlus => Icon::CalendarPlus,
        torro_mail::CategoryIcon::TextQuote => Icon::TextQuote,
        torro_mail::CategoryIcon::Paperclip => Icon::Paperclip,
        torro_mail::CategoryIcon::SquareCheck => Icon::SquareCheck,
        torro_mail::CategoryIcon::Send => Icon::Send,
        torro_mail::CategoryIcon::CalendarDays => Icon::CalendarDays,
        torro_mail::CategoryIcon::ChartColumn => Icon::ChartColumn,
        torro_mail::CategoryIcon::AlignLeft => Icon::AlignLeft,
        torro_mail::CategoryIcon::Mail => Icon::Mail,
        torro_mail::CategoryIcon::Flag => Icon::Flag,
        torro_mail::CategoryIcon::Bell => Icon::Bell,
        torro_mail::CategoryIcon::Users => Icon::Users,
        torro_mail::CategoryIcon::Video => Icon::Video,
        torro_mail::CategoryIcon::MailOpen => Icon::MailOpen,
        torro_mail::CategoryIcon::Reply => Icon::Reply,
        torro_mail::CategoryIcon::Trash2 => Icon::Trash2,
        torro_mail::CategoryIcon::Eye => Icon::Eye,
        torro_mail::CategoryIcon::Ellipsis => Icon::Ellipsis,
        torro_mail::CategoryIcon::Tag => Icon::Tag,
    }
}

/// What `category` is called and which group it stands in now, from the catalogue the window
/// loaded (`inbox.list_categories`, task E15.2): the person may rename any category and move it.
/// Without an entry – a load still on its way – the suite's word and group, and for a category
/// the person made "Category 7" in "Info".
pub(crate) fn category_look(categories: &[CategoryInfo], category: torro_mail::Category) -> (String, CategoryGroup) {
    match categories.iter().find(|held| held.category == category) {
        Some(held) => (held.label.clone(), held.group),
        None => {
            (torro_core::category::fallback_name(category), category.default_group().unwrap_or(CategoryGroup::Info))
        }
    }
}

/// What a person reads for `held` beside the pill: the category, and that a model guessed it.
pub(crate) fn category_words(held: MailCategory, categories: &[CategoryInfo]) -> String {
    let (label, _) = category_look(categories, held.category);
    if held.is_suggestion() { trf("{category} · suggested", &[("category", &label)]) } else { label }
}

/// The small mark of a mail's category for a row (task E12.8): the category icon in its group colour,
/// the words in the tooltip. The whole pill goes where there is room – the reader and the card.
pub(crate) fn category_mark<'a>(held: MailCategory, categories: &[CategoryInfo]) -> (Element<'a, Message>, f32) {
    let (_, group) = category_look(categories, held.category);
    let (_, group) = group_mark(group);
    let category_icon = categories
        .iter()
        .find(|info| info.category == held.category)
        .map(|info| info.icon)
        .unwrap_or_else(|| held.category.default_icon());
    // `widgets::category_mark`, but `on_accent` inside a chosen row drawn in the accent (issue #291,
    // `list::mark_colour`): the group's colour would not read there.
    let mark = icon::icon(category_symbol(category_icon), icon::SIZE_SM).style(move |t: &Theme, _| {
        let p = palette(t);
        svg::Style { color: Some(list::mark_colour(t, p.category[group.min(p.category.len() - 1)])) }
    });
    (widgets::with_tooltip(mark, category_words(held, categories)), icon::SIZE_SM)
}

/// The whole pill of a mail's category, for the reader and the card (task E12.8).
pub(crate) fn category_pill<'a>(held: MailCategory, categories: &[CategoryInfo]) -> Element<'a, Message> {
    let (label, group) = category_look(categories, held.category);
    let (_, group_at) = group_mark(group);
    let category_icon = categories
        .iter()
        .find(|info| info.category == held.category)
        .map(|info| info.icon)
        .unwrap_or_else(|| held.category.default_icon());
    widgets::category_pill(category_symbol(category_icon), group_at, label, held.is_suggestion())
}

/// The mark of an invitation in a row's first line, with its width: the calendar's icon,
/// "Invitation · Tue 20 Oct · 12:00–13:00" as tooltip. Rows of folder messages and of entries
/// (task E8.3c) alike.
pub(crate) fn invite_mark<'a>(invite: &Invite, now: DateTime<Utc>) -> (Element<'a, Message>, f32) {
    let tip = trf("Invitation · {when}", &[("when", &invite_when(invite, now))]);
    (widgets::with_tooltip(small_icon(Icon::Calendar, None), tip), icon::SIZE_SM)
}

/// The time the event of `invite` takes, to ask what it overlaps (task E8.3c,
/// [`MailSource::list_events`]): a whole day from local midnight of its first day to local midnight
/// after its last (the invitation names its days, [`Invite::start`] is midnight UTC of the first);
/// a timed one from its start to its end, at least a minute (an invitation without an end overlaps
/// what runs at its start).
pub fn invite_span(invite: &Invite) -> (DateTime<Utc>, DateTime<Utc>) {
    if invite.all_day {
        let first = invite.start.date_naive();
        let after = invite.end.map_or(first, |end| end.date_naive()).max(first + chrono::Days::new(1));
        let midnight = |day: NaiveDate| datetime::to_utc(day.and_time(chrono::NaiveTime::MIN));
        return (midnight(first), midnight(after));
    }
    let end = invite.end.unwrap_or(invite.start).max(invite.start + chrono::TimeDelta::minutes(1));
    (invite.start, end)
}

/// The events of `events` the event of `invite` overlaps (task E8.3c): those that make their time
/// busy ([`CalendarEvent::busy`]) and share time with [`invite_span`], the earliest first. Left out
/// is the event the suite made of this invitation when it was answered before – same title, same
/// start and end –: it is the invitation itself, no conflict.
pub fn overlaps<'e>(invite: &Invite, events: &'e [CalendarEvent]) -> Vec<&'e CalendarEvent> {
    let (start, end) = invite_span(invite);
    let copy =
        |event: &CalendarEvent| event.title.trim() == invite.title.trim() && (event.start, event.end) == (start, end);
    let mut found: Vec<&CalendarEvent> =
        events.iter().filter(|event| event.busy && event.start < end && start < event.end && !copy(event)).collect();
    found.sort_by_key(|event| event.start);
    found
}

/// The line under an invitation on what it overlaps (task E8.3c, [`overlaps`]): "Overlaps with Team
/// sync at 12:30", with "and 2 more" for more, or "No conflicts". The time alone when the event
/// starts on the invitation's first day, else with its day ("Sat 17 Oct 09:00"). Titles are text of
/// the calendars, on one line.
pub fn overlap_text(invite: &Invite, events: &[CalendarEvent], now: DateTime<Utc>) -> String {
    let found = overlaps(invite, events);
    let Some(first) = found.first() else { return tr("No conflicts").to_owned() };
    let today = now.with_timezone(&Local).date_naive();
    let day = datetime::to_local(invite_span(invite).0).date();
    let start = datetime::to_local(first.start);
    let time = if start.date() == day {
        datetime::format_time(start.time())
    } else {
        format!("{} {}", datetime::format_short_date(start.date(), today), datetime::format_time(start.time()))
    };
    let title = match list::one_line(&first.title) {
        title if title.is_empty() => tr("(no title)").to_owned(),
        title => title,
    };
    match found.len() - 1 {
        0 => trf("Overlaps with {title} at {time}", &[("title", &title), ("time", &time)]),
        more => {
            trf("Overlaps with {title} at {time} and {n} more", &[("title", &title), ("time", &time), ("n", &more)])
        }
    }
}

/// What the card of a mail's invitation shows besides the invitation (task E8.3c), as the window
/// holds it: the calendars to choose for the event and the events it may overlap. Both only matter
/// while the invitation waits for its answer; once answered, its event is in a calendar already
/// (the core keeps it there) and the card shows neither.
#[derive(Debug, Clone, Copy, Default)]
pub struct InviteSide<'a> {
    /// The calendars the event can go to ([`crate::hub::HubSource::list_calendars`]); `None` while
    /// they load or when they could not be read – the core then chooses.
    pub calendars: Option<&'a [TaskList]>,
    /// The calendar chosen for the answer, one of `calendars` (`App::invite_calendar`).
    pub calendar: Option<&'a TaskList>,
    /// The events of the shown calendars around this invitation ([`MailSource::list_events`]);
    /// `None` while they load or when they could not be read: then nothing is said about overlaps.
    pub events: Option<&'a [CalendarEvent]>,
}

/// The invitation of a mail as a card over its text in the reader and the panel (task E8.3b): the
/// calendar's tile, the event's title (the organizer's text, as it is) and when it is, then Accept ·
/// Maybe · Decline as chips to pick – the answer given picked –, which send
/// `mail.respond_to_invitation` ([`Message::AnswerInvitation`]). While an answer is on its way the
/// chips take no click; an event that is over says so instead.
///
/// Before it is answered (task E8.3c, `side`), a line says what it overlaps ([`overlap_text`]: the
/// `attention` colour on the icon, the words say it) once the events are in, and – with calendars to
/// choose – the calendar the event goes to, picked with [`Message::InviteCalendar`].
pub fn invite_card<'a>(
    invite: &Invite,
    summary: &MessageSummary,
    now: DateTime<Utc>,
    answering: bool,
    side: InviteSide<'a>,
    attention: Color,
) -> Element<'a, Message> {
    let tile = widgets::source_tile(widgets::TileFace::Icon(Icon::Calendar), widgets::TileSize::Large, None);
    let title = match list::one_line(&invite.title) {
        title if title.is_empty() => tr("(no title)").to_owned(),
        title => title,
    };
    let words = column![
        container(typo::text(typo::body_strong(), title).style(theme::text).wrapping(Wrapping::None)).clip(true),
        container(
            typo::text(typo::meta().tabular(), invite_when(invite, now)).style(theme::text_2).wrapping(Wrapping::None)
        )
        .clip(true),
    ]
    .width(Fill);
    let head = row![tile, words].spacing(space::m()).align_y(Alignment::Center);
    let mut parts: Vec<Element<'a, Message>> = vec![head.into()];
    if invite_over(invite, now) {
        parts.push(typo::text(typo::meta(), tr("This event is over.")).style(theme::text_3).into());
        return widgets::card(parts, false, None);
    }
    let open = invite.answered.is_none();
    if let Some(events) = side.events.filter(|_| open) {
        let mark = match overlaps(invite, events).is_empty() {
            true => small_icon(Icon::CircleCheck, None),
            false => small_icon(Icon::CircleAlert, Some(attention)),
        };
        let text = typo::text(typo::meta(), overlap_text(invite, events, now)).style(theme::text_2);
        let line = row![mark, container(text.wrapping(Wrapping::None)).clip(true)].spacing(space::s());
        parts.push(line.align_y(Alignment::Center).into());
    }
    if let Some(calendars) = side.calendars.filter(|calendars| open && !calendars.is_empty()) {
        let pick = crate::app::choice(calendars.to_vec(), side.calendar.cloned(), Message::InviteCalendar);
        // As wide as a property column: a pick list across a wide reader would read as a field.
        let line = widgets::prop_line(tr("Calendar"), Some(Mark::Icon(Icon::Calendar)), pick, None);
        parts.push(container(line).max_width(size::props()).into());
    }
    let chips = answers().into_iter().map(|(icon, word, answer)| {
        let on = invite.answered == Some(answer);
        let send = Message::AnswerInvitation(summary.account.clone(), summary.id.clone(), answer);
        widgets::choice_chip(word, Some(Mark::Icon(icon)), on, (!answering).then_some(send))
    });
    parts.push(widgets::chip_row(chips.collect()));
    widgets::card(parts, false, None)
}

/// The sender of a message for its row: the name (else the address, else "Unknown sender") and the
/// domain of the address when the name is not the address itself – as `list::sender` has it for
/// mail entries.
pub fn sender_parts(from: Option<&MailAddress>) -> (String, Option<String>) {
    let name = sender_name(from);
    let domain = from
        .map(|who| list::one_line(&who.address))
        .filter(|address| !address.is_empty() && *address != name)
        .and_then(|address| torro_inbox::rules::domain_of(&address));
    (name, domain)
}

/// A small icon (14) in `text_3`, or in `colour` – `on_accent` inside a chosen row drawn in the accent
/// (issue #291, `list::mark_colour`).
fn small_icon<'a>(which: Icon, colour: Option<Color>) -> Element<'a, Message> {
    icon::icon(which, icon::SIZE_SM)
        .style(move |t: &Theme, _| svg::Style {
            color: Some(colour.map_or(palette(t).text_3, |colour| list::mark_colour(t, colour))),
        })
        .into()
}

/// What the panel shows of a mail.
#[derive(Debug, Clone, Copy)]
pub struct Reading<'a> {
    /// The message's id.
    pub id: &'a str,
    /// The catalogue of categories (task E15.2), for the name and group of the mail's pill.
    pub categories: &'a [CategoryInfo],
    /// The message from the list, for title and header while the whole of it loads.
    pub summary: Option<&'a MessageSummary>,
    /// The whole message: `None` while it loads, then the answer.
    pub loaded: Option<&'a Result<Mail, String>>,
    pub now: DateTime<Utc>,
    /// The mail's text as the window keeps it to select and copy (issue #119): made from the loaded
    /// mail's text ([`text_of`]); empty while it loads.
    pub text: &'a Selectable,
    /// The attention colour, for the flag.
    pub attention: Color,
    /// The message lies in the trash: it is not deleted again from here.
    pub in_trash: bool,
    /// A decision model is set up for sorting mail (task E12.8, `inbox.get_categorization`):
    /// only then does the reader offer "Re-categorize". Without an AI the function is simply not
    /// there (`docs/ki.md`, principle 1).
    pub deciding: bool,
    /// Show the date under the sender; an entry's panel says when already ("Properties").
    pub dated: bool,
    /// The text in the reading level (`typo::read()`, task E8.3): the comfortable density of the
    /// mail window; `BODY` otherwise.
    pub read: bool,
    /// The account's place among the mail accounts, for the colour of the reader's avatar
    /// (`list::account_index`).
    pub account: Option<usize>,
    /// The task made of this mail (task E8.7b): the one its entry carries ([`Item::task`]), or the
    /// one "Task" just made of a folder's message. Shown as a card over the text ([`task_card`]).
    pub task: Option<&'a ItemTask>,
    /// An answer to the mail's invitation is on its way (task E8.3b): its card takes no click.
    pub answering: bool,
    /// The calendars and events beside the mail's invitation (task E8.3c, [`InviteSide`]).
    pub invite: InviteSide<'a>,
    /// The mail's HTML and how it shows (task M9.7); `None` for a mail without HTML, which shows
    /// its text without switches.
    pub html: Option<HtmlReading<'a>>,
    /// The save targets and where each of the mail's attachments would go (task M12.6,
    /// `mail.list_save_targets`); `None` while they load or when they could not be read – a click
    /// then opens, the menus offer Open, Downloads and Save as….
    pub saving: Option<&'a SaveTargets>,
    /// What a click on an attachment's name does (`mail_attachments.click`, [`click_act`]).
    pub click: &'a AttachmentClick,
    /// The attachment whose menu is open (its place among the mail's attachments that are not
    /// `inline`, [`files_of`]): its triangle carries the menu's anchor.
    pub file_menu: Option<usize>,
}

/// How the mail's HTML shows in the panel or the reader (task M9.7): as HTML or its text, in light
/// or dark colours, sharp at the window's scale. The switches over the mail show what is chosen.
#[derive(Debug, Clone, Copy)]
pub struct HtmlReading<'a> {
    /// The mail's HTML, parsed ([`HtmlMail`]).
    pub content: &'a Content,
    /// HTML or text, as the switch "HTML | Text" (or `v`) stands.
    pub format: MailFormat,
    /// The colours of the mail's content, as the switch "Light | Dark" stands.
    pub scheme: Scheme,
    /// Device pixels per logical pixel: the window's scale factor times the zoom.
    pub scale: f32,
}

impl HtmlReading<'_> {
    /// Whether the HTML shows (else the mail's text).
    pub fn shown(&self) -> bool {
        self.format == MailFormat::Html
    }
}

impl<'a> Reading<'a> {
    /// The headers: of the whole message once loaded, else of the list's summary.
    pub fn summary(&self) -> Option<&'a MessageSummary> {
        match self.loaded {
            Some(Ok(mail)) => Some(&mail.summary),
            _ => self.summary,
        }
    }

    /// The title of the panel: the subject.
    pub fn title(&self) -> String {
        self.summary().map_or_else(String::new, subject)
    }

    /// What the panel's footer (task E9.4) and the rail's group "More" (task E14.3) show: for a loaded
    /// mail its switches – "HTML | Text" for a mail with HTML, "Light | Dark" while the HTML shows –
    /// and "Copy text" when it has a text; nothing while it loads or after an error.
    pub fn view_tools(&self) -> ViewTools {
        if !matches!(self.loaded, Some(Ok(_))) {
            return ViewTools::default();
        }
        ViewTools {
            format: self.html.map(|html| html.format),
            scheme: self.html.filter(HtmlReading::shown).map(|html| html.scheme),
            copy: !self.text.text().trim().is_empty(),
        }
    }
}

/// Which bars a mail in the panel has (task E11.4), chosen by the window: the same mail, the same
/// messages and keys, laid out as the window's other parts are.
#[derive(Debug, Clone, Copy)]
pub enum Bars<'a> {
    /// Task E9.4, for the hub and the App Feed: the mail standards at the top of the body
    /// ([`panel_standards`]), Torro's functions and the view's switches in the panel's footer
    /// ([`folder_footer`], `detail::Footer`).
    Split,
    /// Task E14.3, for the mail window in medium and narrow: the reader's head in the panel – the
    /// sender without the subject, which the panel's title says ([`head`]) –, a line, the mail; the
    /// attachments as chips in the panel's footer ([`panel_foot`]). Every action stands in the mail's
    /// action rail, which the window puts at the panel's right (`Panel::side`, collapsed and without
    /// its button, [`action_groups`]).
    Command {
        /// The mail entry the mail belongs to (its properties under the sender, why a rule put it in
        /// its lane); `None` for a folder's message.
        entry: Option<ReaderEntry<'a>>,
        /// How wide the panel is: [`size::panel()`] over the medium window, the whole window in a
        /// narrow one. The body has this less the collapsed rail ([`widgets::RailMode::width`]); the
        /// footer the whole of it.
        width: f32,
    },
}

/// The panel of a folder's message: the account and folder as path, the subject as title, the
/// mail as body ([`body`]) with the `bars` it says. With [`Bars::Split`] the mail standards stand
/// over the mail and the Torro functions in the panel's fixed footer (task E9.4, [`folder_footer`]):
/// Task, then the bell "Mute…" and the view's switches and "Copy text"; with [`Bars::Command`] the
/// attachments in the footer ([`panel_foot`]) and the window adds the action rail (task E14.3,
/// `Panel::side`). `anchors` are where the menus of the footer's buttons open (the app's window,
/// issue #122).
pub fn panel<'a>(
    reading: &Reading<'a>,
    folder: Option<&'a FolderView>,
    anchors: &detail::Anchors,
    bars: Bars<'a>,
) -> panel::Panel<'a, Message> {
    let mark = Some(Mark::Icon(folder_icon(folder.and_then(|folder| folder.role))));
    let parts: Vec<String> = folder.map(|f| vec![address(&f.account).to_owned(), f.name.clone()]).unwrap_or_default();
    let footer = match bars {
        Bars::Split => folder_footer(reading, anchors),
        Bars::Command { .. } => panel_foot(reading, anchors),
    };
    let body = body(reading, bars, anchors);
    panel::panel(panel::path(mark, parts), panel::title(reading.title()), body, Message::CloseDetail).footer(footer)
}

/// The panel of a mail entry in the mail window's medium and narrow layouts (task E11.4): the
/// entry's head as `detail::panel` draws it – source and `view_name` as path, the entry's title, ✕
/// – and its whole mail ([`Bars::Command`] with `entry`, the panel `width` wide): the entry's
/// properties under the sender ([`command_body`]), the attachments in the fixed footer
/// ([`panel_foot`]). The window adds the action rail at its right (task E14.3, `Panel::side`). The
/// hub and the App Feed keep `detail::panel` with [`Bars::Split`].
pub fn entry_panel<'a>(
    reading: &Reading<'a>,
    entry: ReaderEntry<'a>,
    view_name: String,
    width: f32,
    anchors: &detail::Anchors,
) -> panel::Panel<'a, Message> {
    let body = body(reading, Bars::Command { entry: Some(entry), width }, anchors);
    let (path, title) = (detail::path(entry.item, view_name), detail::heading(entry.item));
    panel::panel(path, title, body, Message::CloseDetail).footer(panel_foot(reading, anchors))
}

/// The fixed footer of a mail's panel in the mail window (task E11.4, [`Bars::Command`]): the mail's
/// attachments as rows with their triangles and "Save all ▾" from two on (task M12.6, design B,
/// [`attachment_rows`]), in the panel's padding – the panel draws the line above it. The view's
/// switches and "Copy text" stand in the rail's group "More" (task E14.3). Empty – no footer – while
/// the mail loads, after an error and for a mail without attachments. `anchors` are the window's, for
/// the menus of the triangles.
pub fn panel_foot<'a>(reading: &Reading<'_>, anchors: &detail::Anchors) -> Vec<Element<'a, Message>> {
    attachment_rows(reading, anchors).into_iter().collect()
}

/// The footer of a folder's message in the panel (task E9.4), in the panel's style and in the
/// order of the reader's former Torro bar: "Task" (task E8.7b), a line, then the bell
/// "Mute…" (issue #123: the message's own traits) and the view group ([`view_controls`]) – one row
/// that wraps in a narrow panel. "Later" joins "Task" with task E9.2; the AI fills in only the dialogs
/// of entries (the core suggests from entries only).
fn folder_footer<'a>(reading: &Reading<'a>, anchors: &detail::Anchors) -> Vec<Element<'a, Message>> {
    let id = reading.id;
    let task = Some(Message::AskMailTask(id.to_owned()));
    let task = widgets::text_button(Some(Icon::ListTodo), tr("Task"), style::button::secondary, task);
    let bell = widgets::icon_button(Icon::BellOff, tr("Mute…"), Some(Message::AskMuteMail(id.to_owned())));
    let bar = row![widgets::anchored(&anchors.task, task), separator(), widgets::anchored(&anchors.hide, bell)]
        .extend(view_controls(reading.view_tools()))
        .spacing(space::s())
        .align_y(Alignment::Center);
    // One row while it fits; a panel filling a narrow window wraps it.
    vec![bar.wrap().vertical_spacing(space::s()).into()]
}

/// What the view group of the panel's footer (task E9.4) and the rail's group "More" (task E14.3) shows
/// for a mail: the switches of a mail with HTML and "Copy text". Got from [`Reading::view_tools`];
/// while the mail loads or could not be loaded there is nothing to switch or copy, and every field is
/// empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewTools {
    /// "HTML | Text" (`v`), as it stands; `None` for a mail without HTML (it shows its text).
    pub format: Option<MailFormat>,
    /// "Light | Dark" for the colours of the mail's content, as it stands; only while the HTML
    /// shows.
    pub scheme: Option<Scheme>,
    /// "Copy text" (issue #119): the mail has a text to copy.
    pub copy: bool,
}

/// The view group of [`ViewTools`] in the panel's footer (task E9.4), in the panel's own size:
/// "HTML | Text", "Light | Dark" ([`widgets::segmented`]) and "Copy text" as an icon, each only where
/// `tools` has it. The entry's footer (`detail::Footer::view`) and a folder's message's
/// ([`folder_footer`]) put them after "Move to …" and the bell.
pub fn view_controls<'a>(tools: ViewTools) -> Vec<Element<'a, Message>> {
    let mut controls = Vec::new();
    if let Some(format) = tools.format {
        controls.push(widgets::segmented(formats(), usize::from(format == MailFormat::Text)));
    }
    if let Some(scheme) = tools.scheme {
        controls.push(widgets::segmented(schemes(), usize::from(scheme == Scheme::Dark)));
    }
    if tools.copy {
        controls.push(copy_button());
    }
    controls
}

/// The segments of "HTML | Text" (task M9.7), HTML first.
fn formats() -> [(String, Message); 2] {
    [
        (tr("HTML").to_owned(), Message::ShowMailAs(MailFormat::Html)),
        (tr("Text").to_owned(), Message::ShowMailAs(MailFormat::Text)),
    ]
}

/// The segments of "Light | Dark" (task M9.7), light first.
fn schemes() -> [(String, Message); 2] {
    [
        (tr("Light").to_owned(), Message::MailColors(Scheme::Light)),
        (tr("Dark").to_owned(), Message::MailColors(Scheme::Dark)),
    ]
}

/// The mail in the panel with the `bars` the window chose ([`Bars`]).
///
/// - [`Bars::Split`] (task E9.4): who wrote it to whom and when, the mail standards
///   ([`panel_standards`]: reply, reply all, forward, archive, delete, flag, read), the attachments,
///   the HTML or the text with its links, and the note that the mail comes from outside. The Torro
///   functions, the switches "HTML | Text" and "Light | Dark" (task M9.7) and "Copy text" stand in
///   the panel's footer: an entry's (`detail::Footer`) or a folder's message's ([`panel()`]).
/// - [`Bars::Command`] (task E11.4, [`command_body`]): the sender, invitation and task, an entry's
///   properties, a line, the mail; the attachments in the footer ([`panel_foot`]), every action in
///   the rail beside it (task E14.3).
pub fn body<'a>(reading: &Reading<'a>, bars: Bars<'a>, anchors: &detail::Anchors) -> Element<'a, Message> {
    match bars {
        Bars::Split => split_body(reading, anchors),
        Bars::Command { entry, width } => command_body(reading, entry, width),
    }
}

/// The body of the mail window's panel (task E11.4, [`Bars::Command`]) in a panel `width` wide, in
/// the order of the reader (task E11.3): the head without the subject – the panel's title says it –
/// ([`head_with`]: sender, why a rule put the entry in its lane, invitation and task) lined up with
/// the title, an entry's properties, a line from edge to edge and the mail ([`panel_letter`]). The
/// collapsed rail beside it (task E14.3) takes its width from the body.
fn command_body<'a>(reading: &Reading<'a>, entry: Option<ReaderEntry<'a>>, width: f32) -> Element<'a, Message> {
    let line = || rule::horizontal(size::HAIRLINE).style(style::rule);
    // The head lines up with the title: 16 from the panel's edges (`panel::inset`), the rail beside.
    // The rail stands `space::xs()` apart from the body (`Panel::side`).
    let room = (width - widgets::RailMode::Collapsed.width() - space::xs() - 2.0 * space::l()).max(0.0);
    let head = head_with(reading, entry, room, false);
    let mut body = column![panel::inset(head)];
    // An entry keeps its properties (status, lane, source, time), as `detail::panel` shows them.
    if let Some(entry) = entry {
        // "Follow up" in the mail window's words (task E19.4).
        let properties = detail::properties(entry.item, reading.now, true);
        body = body.push(widgets::prop_section(tr("Properties"), properties));
    }
    body.push(panel::bleed(line())).extend(panel_letter(reading)).spacing(space::l()).into()
}

/// The body of the panel with the split of task E9.4 ([`Bars::Split`], see [`body`]).
fn split_body<'a>(reading: &Reading<'a>, anchors: &detail::Anchors) -> Element<'a, Message> {
    let mut body = column![].spacing(space::l());
    if let Some(summary) = reading.summary() {
        body = body.push(panel::inset(header(reading, summary)));
        body = body.push(panel::inset(panel_standards(reading, summary)));
    }
    if let Some(summary) = reading.summary()
        && let Some(invite) = &summary.invite
    {
        let card = invite_card(invite, summary, reading.now, reading.answering, reading.invite, reading.attention);
        body = body.push(panel::inset(card));
    }
    if let Some(task) = reading.task {
        body = body.push(panel::inset(task_card(task, reading.now)));
    }
    // The attachments as rows over the mail, each with its triangle (task M12.6).
    if let Some(rows) = attachment_rows(reading, anchors) {
        body = body.push(rows);
    }
    body.extend(panel_letter(reading)).into()
}

/// The mail itself in the panel's body, both [`Bars`] alike: while it loads or after an error a
/// quiet line; once in, the mail in the reading column (issue #291, [`reading_column`]): the HTML with
/// the note under it, or the text with the note on foreign text and its "Links".
fn panel_letter<'a>(reading: &Reading<'a>) -> Vec<Element<'a, Message>> {
    match reading.loaded {
        None => vec![panel::inset(note(tr("Loading the message…")))],
        Some(Err(why)) => {
            let what = typo::text(typo::body(), tr("Couldn't load the message.")).style(theme::text_2);
            let why = typo::text(typo::meta(), why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
            vec![panel::inset(column![what, why].spacing(space::xs()))]
        }
        Some(Ok(_)) => vec![reading_column(letter(reading).width(Fill))],
    }
}

/// The loaded mail as the reading column holds it ([`panel_letter`], [`reader_mail`]): the HTML
/// ([`html_view`], no margin of its own – the column's padding is its margin, so its text starts where
/// a plain text does) with the note under it, or the text with the note on foreign text and its
/// "Links".
fn letter<'a>(reading: &Reading<'a>) -> iced::widget::Column<'a, Message> {
    if let Some(html) = html_edge(reading) {
        return column![html_view(html).margin(0.0), html_note()].spacing(space::m());
    }
    let mut text = column![text_block(reading.text, text_level(reading.read))].spacing(space::l());
    if let Some(links) = text_links(reading.text) {
        text = text.push(links);
    }
    text
}

/// The widest the mail's text or HTML stands in a reader or panel `width` wide (issue #291, the reading
/// column): the width inside [`layout::reading_padding`], at most [`size::reading_max`] – 600 in the
/// standard, 1200 in Console, where the mail fills the reader. Never negative.
fn reading_width(width: f32) -> f32 {
    let pad = layout::reading_padding();
    reading_width_in(width, pad.left + pad.right, size::reading_max())
}

/// [`reading_width`] for a reading padding of `sides` together and a column of at most `max`: apart
/// from the active design, so the tests can check every design.
fn reading_width_in(width: f32, sides: f32, max: f32) -> f32 {
    (width - sides).max(0.0).min(max)
}

/// `content` in the reading column of the panel (issue #291), whose width is not known here: at most
/// [`size::reading_max`] wide, the reading padding above and below it – at the sides the panel's own
/// inset (`panel::inset`), so the mail's text stays in line with the panel's title and head, which the
/// panel draws there. The reading padding's sides would set the text off that line and leave a narrow
/// panel a strip of text (in the standard 64 of 400 at either side).
fn reading_column<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let read = layout::reading_padding();
    let column = container(content).width(Fill).max_width(size::reading_max());
    panel::inset(container(column).width(Fill).padding(Padding::ZERO.top(read.top).bottom(read.bottom)))
}

/// The level of a mail's text: the reading level in the comfortable mail window, else `BODY`.
fn text_level(read: bool) -> typo::Level {
    if read { typo::read() } else { typo::body() }
}

/// The entry a mail in the reader belongs to (task E8.3): the rail's Done or Reopen, Later…, "Move
/// to …", Task, Event, Category… and Mute… act on it (task E14.3, [`action_groups`]), as `e`, `s`,
/// `l`, `t`, `c` and `m` do.
#[derive(Debug, Clone, Copy)]
pub struct ReaderEntry<'a> {
    pub item: &'a Item,
    /// The entry can be muted (`App::hidable`): the rail shows its group "Mute" (issue #123).
    pub hide: bool,
}

/// The reader of the wide mail window (task E8.3) beside the list (task E14.3, design D of
/// `docs/prototypes/inbox/mail-aktionsleiste.html`): at its right the mail's action `rail` over its
/// whole height (`widgets::action_rail` of [`action_groups`], as the window draws it in the mode it
/// remembers), at its left from top to bottom:
///
/// 1. scrolling, the **head** ([`head`]: subject, sender, why a rule put the entry in its lane,
///    invitation and task), a line over the reader's whole width and the **mail**
///    ([`reader_mail`]): text or HTML in the reading column (issue #291, [`reader_widths`]);
/// 2. fixed, the mail's **attachments** in their block ([`attachment_line`], task M12.6), whose
///    menus hang at `anchors`.
///
/// The head and the attachments line up with the mail's text, at the side of the reading padding.
/// No bar over the mail and no buttons in its head: every action stands in the rail, with its key.
/// `entry` is the mail entry the mail belongs to, `None` for a folder's message.
pub fn reader<'a>(
    reading: Reading<'a>,
    entry: Option<ReaderEntry<'a>>,
    rail: Element<'a, Message>,
    anchors: &detail::Anchors,
) -> Element<'a, Message> {
    let anchors = anchors.clone();
    let page = responsive(move |bounds| {
        let widths = reader_widths(bounds.width);
        let side = layout::reading_padding().left;
        let head = container(head(&reading, entry, widths.head)).width(Fill).padding(head_padding());
        let page = column![head, rule::horizontal(size::HAIRLINE).style(style::rule), reader_mail(&reading, widths)];
        let page = scrollable(page)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill);
        column![page].push(attachment_line(&reading, &anchors, widths.head, side)).into()
    });
    // The reader's scrollbar keeps clear of the rail's line (decision B).
    row![page, rail].spacing(space::xs()).height(Fill).into()
}

/// The padding around the reader's head: the page's above, a little less below, where the line under
/// the head follows; at the sides the reading padding (issue #291), so the head lines up with the
/// mail's text.
fn head_padding() -> Padding {
    let read = layout::reading_padding();
    Padding { left: read.left, right: read.right, bottom: space::l(), ..layout::page_padding() }
}

/// How wide the parts of the reader are ([`reader_widths`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ReaderWidths {
    /// The head – subject, sender, invitation, task – and the attachments: inside the reading padding's
    /// sides.
    head: f32,
    /// The mail's text or HTML: the reading column ([`reading_width`]), at most
    /// [`size::reading_max`].
    mail: f32,
}

/// The widths in a reader `width` wide (issue #291): the head over the whole width inside the reading
/// padding, the mail – text and HTML alike – in the reading column, at most [`size::reading_max`]
/// wide, at the left under the head (a newsletter centres itself in it, a mail without a background
/// of its own starts its text where the head does).
fn reader_widths(width: f32) -> ReaderWidths {
    let pad = layout::reading_padding();
    ReaderWidths { head: (width - pad.left - pad.right).max(0.0), mail: reading_width(width) }
}

/// The mail's HTML when it is loaded and shows as HTML.
fn html_edge<'a>(reading: &Reading<'a>) -> Option<HtmlReading<'a>> {
    reading.html.filter(|html| html.shown() && matches!(reading.loaded, Some(Ok(_))))
}

/// The mail under the reader's head and its line (task E11.3, issue #291): in the reading column
/// `widths.mail` wide with the reading padding around it ([`letter`]: text with its links and the
/// note on foreign text, or HTML with the note under it). While it loads, or after an error, a quiet
/// line says so.
fn reader_mail<'a>(reading: &Reading<'a>, widths: ReaderWidths) -> Element<'a, Message> {
    let pad = layout::reading_padding();
    match reading.loaded {
        None => container(note(tr("Loading the message…"))).padding(pad).into(),
        Some(Err(why)) => {
            let what = typo::text(typo::body(), tr("Couldn't load the message.")).style(theme::text_2);
            let why = typo::text(typo::meta(), why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
            container(column![what, why].spacing(space::xs())).padding(pad).into()
        }
        Some(Ok(_)) => container(letter(reading).width(widths.mail)).padding(pad).into(),
    }
}

/// The head of a mail in the reader (task E11.3), `room` wide: the subject (`typo::headline()`), the
/// sender ([`sender_block`]), for an entry why a rule put it in its lane (issue #128), what the mail
/// is (task E12.8) and under it the invitation's card (task E8.3b) and the task's (task E8.7b).
/// Everything stands left-aligned, one under the other; no buttons – they stand in the rail (task
/// E14.3). The panel of the mail window takes the same head without the subject (task E11.4,
/// [`head_with`]).
pub fn head<'a>(reading: &Reading<'a>, entry: Option<ReaderEntry<'a>>, room: f32) -> Element<'a, Message> {
    head_with(reading, entry, room, true)
}

/// The [`head`], with the subject on top only with `subject`: the panel of the mail window (task
/// E11.4) has it as its title already and leaves it out here.
fn head_with<'a>(
    reading: &Reading<'a>,
    entry: Option<ReaderEntry<'a>>,
    room: f32,
    subject: bool,
) -> Element<'a, Message> {
    let mut head = column![].spacing(space::l()).width(Fill);
    let summary = reading.summary();
    if let Some(summary) = summary {
        if subject {
            let line = typo::text(typo::headline(), self::subject(summary));
            head = head.push(line.style(theme::text).wrapping(Wrapping::WordOrGlyph));
        }
        let sender = sender_block(reading, summary, room);
        // Why a rule put the mail's entry in its lane (issue #128), quietly right under who sent it.
        head = head.push(match entry.map(|entry| entry.item).filter(|item| item.reason.is_some()) {
            Some(item) => {
                let why = typo::text(typo::meta(), detail::lane_text(item)).style(theme::text_3);
                column![sender, why.wrapping(Wrapping::WordOrGlyph)].spacing(space::s()).into()
            }
            None => sender,
        });
        // What the mail is (task E12.8), right under who sent it: the whole pill, with the
        // suggestion mark when a model guessed it (`docs/ki.md`, principle 5). Also in the panel,
        // which draws this head without the subject.
        if let Some(held) = summary.category {
            head = head.push(widgets::chip_row(vec![category_pill(held, reading.categories)]));
            // Phishing (task E12.9): a band over the message, and its links ask before they open.
            // Nothing is hidden, moved or deleted – the one change is the extra click.
            if held.category == torro_mail::Category::Phishing {
                head = head.push(widgets::warning_band(
                    tr("This looks like an attempt to deceive."),
                    tr("Links here open only after you confirm, and you see where each one goes. \
                        The mail itself is untouched."),
                ));
            }
        }
    }
    // What the mail invites to, with its answers (task E8.3b), and the task made of it (task E8.7b).
    if let Some(summary) = summary
        && let Some(invite) = &summary.invite
    {
        head =
            head.push(invite_card(invite, summary, reading.now, reading.answering, reading.invite, reading.attention));
    }
    if let Some(task) = reading.task {
        head = head.push(task_card(task, reading.now));
    }
    head.into()
}

/// Who wrote the mail, in the reader's head `room` wide (task E11.3): the avatar ([`size::pill()`];
/// none in a design without avatars, `[list] avatar = 0`, issue #291 – its place goes to the words)
/// and beside it the name – with the flag of a flagged mail – and the address quietly after it, under
/// them "to … · account · when" and, once the mail is loaded, "cc …" in `typo::meta()`/`text_2` –
/// left-aligned, no date at the right. What does not fit ends in "…".
fn sender_block<'a>(reading: &Reading<'a>, summary: &'a MessageSummary, room: f32) -> Element<'a, Message> {
    let name = sender_name(summary.from.as_ref());
    let edge = head_avatar_in(layout::list());
    let face = (edge > 0.0).then(|| list::avatar(&name, reading.account, edge));
    let words_room = (room - face.as_ref().map_or(0.0, |_| edge + space::m())).max(0.0);
    // A flagged mail says so at its sender, as the panel's head of task E9.4 did: the name leaves
    // room for the flag, so a long one never pushes it out.
    let flagged = summary.flags.contains(&Flag::Flagged);
    let flag_room = if flagged { icon::SIZE_SM + space::s() } else { 0.0 };
    let name = typo::ellipsize(&name, typo::body_strong(), (words_room - flag_room).max(0.0));
    let left = words_room - flag_room - typo::text_width(&name, typo::body_strong()) - space::s();
    let mut who = row![typo::text(typo::body_strong(), name).style(theme::text).wrapping(Wrapping::None)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if flagged {
        who = who.push(widgets::with_tooltip(small_icon(Icon::Flag, Some(reading.attention)), tr("Flagged")));
    }
    if let Some(from) = &summary.from {
        let address = typo::ellipsize(&list::one_line(&from.address), typo::meta(), left);
        who = who.push(typo::text(typo::meta(), address).style(theme::text_3).wrapping(Wrapping::None));
    }
    let mut to = Vec::new();
    if !summary.to.is_empty() {
        to.push(trf("to {names}", &[("names", &names(&summary.to))]));
    }
    to.push(address(&summary.account).to_owned());
    to.push(when(summary.date, reading.now));
    let to = typo::ellipsize(&to.join(" · "), typo::meta(), words_room);
    let to = typo::text(typo::meta(), to).style(theme::text_2).wrapping(Wrapping::None);
    let mut words = column![container(who).clip(true), container(to).clip(true)].width(Fill);
    // Who else got it, once the mail is loaded (its summary has no cc).
    if let Some(Ok(mail)) = reading.loaded
        && !mail.cc.is_empty()
    {
        let cc = typo::ellipsize(&trf("cc {names}", &[("names", &names(&mail.cc))]), typo::meta(), words_room);
        words = words.push(typo::text(typo::meta(), cc).style(theme::text_2).wrapping(Wrapping::None));
    }
    row(face).push(words).spacing(space::m()).align_y(Alignment::Center).into()
}

/// The edge of the sender's avatar in the reader's head in the list look `look`: [`size::pill()`],
/// none (0) in a design whose mail rows have no avatar (`[list] avatar = 0`, issue #291), so rows and
/// reader agree.
fn head_avatar_in(look: torro_core::design::ListLook) -> f32 {
    if look.avatar > 0.0 { size::pill() } else { 0.0 }
}

/// The groups of the mail's action rail (task E14.3, decision 2 of E14 in `docs/tasks/eingang.md`,
/// design D of `docs/prototypes/inbox/mail-aktionsleiste.html`) for the mail in `reading`, in this
/// order – each item with its icon, word and key and the message its button or key sent before
/// (tasks E9.4, E11.3), so requests and keys stay the same:
///
/// 1. **Reply** (the main group: a red icon collapsed, Reply strong expanded – decision A): Reply
///    (`r`), Reply all, Forward (`f`) – [`Message::Mail`].
/// 2. **File** ([`file_items`], task E19.4): Archive (`e`, task E18.5: in the place of the entry's
///    former Done – mail is archived, never done) where the account has the folder and the mail does
///    not lie in it already, as in rows ([`filing_actions`]; an entry's mail lies in its account's
///    inbox while it loads); Reply later (`w`) and Set aside (`b`, [`Message::Pile`]) but for mail in
///    the trash or spam ([`takes_piles`]) – for a mail on one of them "Remove from Reply later" or
///    "Remove from Set aside" in its place, with the same key, wherever it lies; Follow up… (`s`, `AskSnooze`) for every mail but one in the archive, the trash or spam
///    ([`follows_up`]) – its entry's when it has an open one, else the mail's own id (decision 8 of
///    E19); for an `entry` "Move to Info" or "Move to For me" (`l`,
///    `AskLane`); Delete (`#`) on the same rule as Archive; last "Save attachments (n)" for a loaded
///    mail with attachments (task M12.6, `AskSaveAttachments`: the menu of "Save all").
/// 3. **Torro**: Task (`t`; `AskTask` for an entry, `AskMailTask` for a folder's message), for an
///    entry Event (`c`, `AskEvent`) and – for a mail entry – Category… (`AskCategory`) and, with a
///    decision model set up ([`Reading::deciding`]), Re-categorize (`Reclassify`, sparkles in the
///    accent).
/// 4. **Mark**, once the mail's flags are known: Flag or Unflag, Mark read or Mark unread.
/// 5. **Mute** (at the foot): with `hidden` (the mail is listed in "Hidden", task E18.4) first
///    Show again (no key, [`Message::ShowMail`]: back into "All", the rule stays), then Mute… (`m`;
///    `AskHide` for an entry that can be muted, `AskMuteMail` for a folder's message).
/// 6. **More** (at the foot), what [`Reading::view_tools`] offers: Copy text (`CopyText`) first,
///    then Show as text or Show as HTML (`v`, `ShowMailAs`), Light or Dark colours (`MailColors`,
///    while the HTML shows), and with `close` (the reader; the panel has its ✕) Close (Escape,
///    `CloseDetail`).
///
/// A group without items is left out (Mark and More while the mail loads, Mute for an entry that
/// cannot be muted). Every group has its fixed icon, as design D draws it (finding 8): Reply, the
/// check of File, Torro's task, Mark's flag – in the attention colour while the mail is flagged –,
/// the bell and "…". A click on it sends its first item; Delete is marked `danger` and never that
/// click – a File whose first item would be Delete opens its card instead. Each group registers its icon in `anchors.rail`, each item that opens a
/// menu or dialog its row in the matching anchor of `anchors` (Follow up… `snooze`, "Move to …" `lane`,
/// Task `task`, Event `event`, Category… `category`, Mute… `hide`); pass the window's anchors made
/// with [`detail::Anchors::with_rail`], so those menus hang at the group's icon while the row is not
/// on screen. `folders` are every account's folders, for Archive and Delete. The same groups are the
/// context menu of the mail's row (`widgets::rail_context_menu`), so Show again stands there too.
pub fn action_groups<'a>(
    reading: &Reading<'_>,
    entry: Option<ReaderEntry<'_>>,
    folders: &[Folder],
    close: bool,
    hidden: bool,
    anchors: &detail::Anchors,
) -> Vec<RailGroup<'a, Message>> {
    let rail = &anchors.rail;
    let mail = |action: MailAction| Some(Message::Mail(action, reading.id.to_owned()));
    let reply = vec![
        RailItem::new(Icon::Reply, tr("Reply"), "R", mail(MailAction::Reply)).lead(),
        RailItem::new(Icon::ReplyAll, tr("Reply all"), "", mail(MailAction::ReplyAll)),
        RailItem::new(Icon::Forward, tr("Forward"), "F", mail(MailAction::Forward)),
    ];
    // A flagged mail says so at the group's icon (finding 9), in the colour of the flag in its row.
    let flagged = reading.summary().is_some_and(|summary| summary.flags.contains(&Flag::Flagged));
    let mark = RailGroup::new(Icon::Flag, tr("Mark"), mark_items(reading)).at(&rail.mark);
    let groups = [
        RailGroup::new(Icon::Reply, tr("Reply"), reply).accent().at(&rail.reply),
        RailGroup::new(Icon::Check, tr("File"), file_items(reading, entry, folders, anchors)).at(&rail.file),
        RailGroup::new(Icon::ListTodo, tr("Torro"), torro_items(reading, entry, anchors)).at(&rail.torro),
        if flagged { mark.tinted(reading.attention) } else { mark },
        RailGroup::new(Icon::BellOff, tr("Mute"), mute_items(reading, entry, hidden, anchors)).foot().at(&rail.mute),
        RailGroup::new(Icon::Ellipsis, tr("More"), more_items(reading.view_tools(), close)).foot().at(&rail.more),
    ];
    groups.into_iter().filter(|group| !group.items.is_empty()).collect()
}

/// The items of the rail's group "File" ([`action_groups`]), in this order (task E19.4): Archive ·
/// Reply later · Set aside · Follow up… · "Move to …" · Delete · Save attachments (n). The piles
/// follow [`MessageSummary::pile`]: on one, its item is "Remove from …" with the same key, in every
/// view; they wait for the mail's headers, which say its pile. Follow up… stands once the mail's
/// account is known (its headers or its entry), as Archive and Delete.
fn file_items<'a>(
    reading: &Reading<'_>,
    entry: Option<ReaderEntry<'_>>,
    folders: &[Folder],
    anchors: &detail::Anchors,
) -> Vec<RailItem<'a, Message>> {
    let mut items = Vec::new();
    // An entry's mail lies in its account's inbox while it loads, as its row files it.
    let filing = match (reading.summary(), entry) {
        (Some(summary), _) => Some(Filing::of(folders, &summary.account, Some(&summary.folder))),
        (None, Some(ReaderEntry { item, .. })) => Some(Filing::of(folders, &item.source.key(), None)),
        (None, None) => None,
    };
    let mail = |action: MailAction| Some(Message::Mail(action, reading.id.to_owned()));
    // First, so a click on the group's icon archives, as it marked an entry done before.
    let archive = filing.is_some_and(|filing| filing.archive);
    if archive {
        items.push(RailItem::new(Icon::Archive, trc("action", "Archive"), "E", mail(MailAction::Archive)));
    }
    // Always in their place, so the rail does not jump once the mail's headers are in; off until then.
    // Without Archive a click on the group's icon opens its card rather than putting the mail on a
    // pile – the pattern of Delete, which is never that click either (`RailGroup::first_press`).
    // Not for mail in the trash or spam (review of PR #327) – but "Remove from …" for one that lies on
    // a pile, wherever it is.
    let takes = filing.is_none_or(|filing| filing.piles);
    let on = reading.summary().and_then(|summary| summary.pile);
    let piles = pile_items(reading).into_iter().zip([MailPile::ReplyLater, MailPile::SetAside]);
    let piles = piles.filter(|(_, pile)| takes || on == Some(*pile)).map(|(item, _)| item);
    items.extend(piles.map(|item| if archive { item } else { item.danger() }));
    // Every mail can be followed up (decision 8 of E19): an open entry by its own id, any other mail –
    // a folder's, a done entry's – by the mail's id, of which the core makes the entry. Not mail in the
    // archive, the trash or spam (decision of #315): the folder of the mail shown decides.
    if filing.is_some_and(|filing| filing.follow_up) {
        let follow = match entry {
            Some(ReaderEntry { item, .. }) if !matches!(item.state, State::Done { .. }) => item.id.clone(),
            _ => reading.id.to_owned(),
        };
        let follow = RailItem::new(Icon::CalendarClock, tr("Follow up…"), "S", Some(Message::AskSnooze(follow)));
        // Without Archive not what a click on the group's icon does either, also while the piles are
        // off as the headers load: the click always opens the card (second review of #315).
        let follow = if archive { follow } else { follow.danger() };
        items.push(follow.at(&anchors.snooze));
    }
    if let Some(ReaderEntry { item, .. }) = entry {
        let lane = crate::app::other_lane(item.lane);
        let move_to = Some(Message::AskLane(item.id.clone()));
        items.push(RailItem::new(crate::app::lane_icon(lane), lane_word(lane), "L", move_to).at(&anchors.lane));
    }
    if filing.is_some_and(|filing| filing.delete) {
        // Never what a click on the group's icon does (finding 8).
        items.push(RailItem::new(Icon::Trash2, tr("Delete"), "#", mail(MailAction::Delete)).danger());
    }
    // Task M12.6: the menu of "Save all" at this item – from the list's context menu at the row –,
    // once the mail shows attachments. Last, so a click on the group's icon still files the mail.
    if let Some(files) = files_of(reading) {
        let words = trf("Save attachments ({n})", &[("n", &files.len())]);
        let save = Some(Message::AskSaveAttachments(reading.id.to_owned()));
        items.push(RailItem::new(Icon::Download, words, "", save).at(&anchors.save_rail));
    }
    items
}

/// "Reply later" (`w`) and "Set aside" (`b`) for the mail in `reading` (task E19.4): each puts it on
/// its pile ([`Message::Pile`]); for the pile it lies on "Remove from Reply later" or "Remove from Set
/// aside" instead, which takes it off. Off, without their keys, while the mail's headers are not in –
/// which pile it lies on is not known yet.
fn pile_items<'a>(reading: &Reading<'_>) -> [RailItem<'a, Message>; 2] {
    let known = reading.summary();
    let on = known.and_then(|summary| summary.pile);
    let pile = |pile: MailPile| known.map(|_| Message::Pile(reading.id.to_owned(), (on != Some(pile)).then_some(pile)));
    let key = |key: &'static str| if known.is_some() { key } else { "" };
    let reply = match on {
        Some(MailPile::ReplyLater) => tr("Remove from Reply later"),
        _ => tr("Reply later"),
    };
    let aside = match on {
        Some(MailPile::SetAside) => tr("Remove from Set aside"),
        _ => tr("Set aside"),
    };
    [
        RailItem::new(Icon::MessageSquareReply, reply, key("W"), pile(MailPile::ReplyLater)),
        RailItem::new(Icon::Bookmark, aside, key("B"), pile(MailPile::SetAside)),
    ]
}

/// The items of the rail's group "Torro" ([`action_groups`]): what Torro adds to the mail, as the
/// reader's pills had it (task E11.3).
fn torro_items<'a>(
    reading: &Reading<'_>,
    entry: Option<ReaderEntry<'_>>,
    anchors: &detail::Anchors,
) -> Vec<RailItem<'a, Message>> {
    let Some(ReaderEntry { item, .. }) = entry else {
        let task = Some(Message::AskMailTask(reading.id.to_owned()));
        return vec![RailItem::new(Icon::ListTodo, tr("Task"), "T", task).at(&anchors.task)];
    };
    let id = || item.id.clone();
    let mut items = vec![
        RailItem::new(Icon::ListTodo, tr("Task"), "T", Some(Message::AskTask(id()))).at(&anchors.task),
        RailItem::new(Icon::CalendarPlus, tr("Event"), "C", Some(Message::AskEvent(id()))).at(&anchors.event),
    ];
    // What the mail is (task E12.8): only mail has a category. "Re-categorize" only with a decision
    // model set up – without an AI the function is simply not there (`docs/ki.md`, principle 1).
    if item.source.kind == torro_inbox::SourceKind::Mail {
        let category = RailItem::new(Icon::Tag, tr("Category…"), "", Some(Message::AskCategory(id())));
        items.push(category.at(&anchors.category));
        if reading.deciding {
            items
                .push(RailItem::new(Icon::Sparkles, tr("Re-categorize"), "", Some(Message::Reclassify(id()))).accent());
        }
    }
    items
}

/// The items of the rail's group "Mark" ([`action_groups`]): Flag or Unflag, Mark read or Mark
/// unread – nothing until the mail's flags are known.
fn mark_items<'a>(reading: &Reading<'_>) -> Vec<RailItem<'a, Message>> {
    let Some(summary) = reading.summary() else { return Vec::new() };
    let mail = |action: MailAction| Some(Message::Mail(action, reading.id.to_owned()));
    let flagged = summary.flags.contains(&Flag::Flagged);
    let flag = if flagged { tr("Unflag") } else { tr("Flag") };
    let seen = match summary.flags.contains(&Flag::Seen) {
        true => RailItem::new(Icon::Mail, tr("Mark unread"), "", mail(MailAction::Read(false))),
        false => RailItem::new(Icon::MailOpen, tr("Mark read"), "", mail(MailAction::Read(true))),
    };
    vec![RailItem::new(Icon::Flag, flag, "", mail(MailAction::Flag(!flagged))), seen]
}

/// The items of the rail's group "Mute" ([`action_groups`]): for a mail of "Hidden" (`hidden`, task
/// E18.4) first "Show again" – it takes this one mail back into "All", never the rule that hid it
/// (decision 10 of E18) –, then the bell of the entry where it can be muted, else nothing; the bell of
/// a folder's message on its own traits (issue #123).
fn mute_items<'a>(
    reading: &Reading<'_>,
    entry: Option<ReaderEntry<'_>>,
    hidden: bool,
    anchors: &detail::Anchors,
) -> Vec<RailItem<'a, Message>> {
    let mut items = Vec::new();
    if hidden {
        let show = Some(Message::ShowMail(reading.id.to_owned()));
        items.push(RailItem::new(Icon::Eye, tr("Show again"), "", show));
    }
    let bell = match entry {
        Some(ReaderEntry { item, hide: true }) => Message::AskHide(item.id.clone()),
        Some(ReaderEntry { hide: false, .. }) => return items,
        None => Message::AskMuteMail(reading.id.to_owned()),
    };
    items.push(RailItem::new(Icon::BellOff, tr("Mute…"), "M", Some(bell)).at(&anchors.hide));
    items
}

/// The items of the rail's group "More" ([`action_groups`]) for `tools`: the other view of a mail
/// with HTML, the other colours while its HTML shows, "Copy text", and with `close` Close.
fn more_items<'a>(tools: ViewTools, close: bool) -> Vec<RailItem<'a, Message>> {
    let mut items = Vec::new();
    // First, so a click on "…" copies the text (decision D): the safest of the group.
    if tools.copy {
        items.push(RailItem::new(Icon::Copy, tr("Copy text"), "", Some(Message::CopyText)));
    }
    match tools.format {
        Some(MailFormat::Html) => {
            items.push(RailItem::new(
                Icon::AlignLeft,
                tr("Show as text"),
                "V",
                Some(Message::ShowMailAs(MailFormat::Text)),
            ));
        }
        Some(MailFormat::Text) => {
            items.push(RailItem::new(Icon::Code, tr("Show as HTML"), "V", Some(Message::ShowMailAs(MailFormat::Html))));
        }
        None => {}
    }
    match tools.scheme {
        Some(Scheme::Light) => {
            items.push(RailItem::new(Icon::Moon, tr("Dark colours"), "", Some(Message::MailColors(Scheme::Dark))));
        }
        Some(Scheme::Dark) => {
            items.push(RailItem::new(Icon::Sun, tr("Light colours"), "", Some(Message::MailColors(Scheme::Light))));
        }
        None => {}
    }
    if close {
        items.push(RailItem::new(Icon::X, tr("Close"), "Esc", Some(Message::CloseDetail)));
    }
    items
}

/// The mail's attachments at the reader's bottom (task M12.6, design A of
/// `docs/prototypes/inbox/mail-anhaenge.html`): a framed block (`widgets::file_block`) whose head
/// says "Attachments" and "{n} files · {size}" ([`files_meta`]), with "Save all ▾" at its right
/// from two attachments on ([`save_all_control`]), and under it each attachment as a split chip
/// ([`attachment_chips`]), which scroll inside the block beyond two lines. [`space::s()`] above it,
/// [`space::l()`] below and `side` at the sides; the chips wrap on a line `room` wide. The
/// triangles' menus hang at `anchors`. `None` while the mail loads, after an error and for a mail
/// without attachments (inline pictures are none).
pub fn attachment_line<'a>(
    reading: &Reading<'_>,
    anchors: &detail::Anchors,
    room: f32,
    side: f32,
) -> Option<Element<'a, Message>> {
    let files = files_of(reading)?;
    let action = save_all_control(reading, &files, anchors, false);
    let head = widgets::file_block_head(tr("Attachments"), files_meta(&files), action);
    // The block's frame and its inner padding at the sides (`widgets::file_block`) take from the
    // chips' line.
    let inner = (room - 2.0 * (space::m() + size::HAIRLINE)).max(0.0);
    let block = widgets::file_block(head, attachment_chips(reading, &files, anchors, inner));
    Some(
        container(block)
            .width(Fill)
            .padding(Padding { top: space::s(), bottom: space::l(), left: side, right: side })
            .into(),
    )
}

/// The attachments of the loaded mail that are not `inline`, in its order – what the block, the rows
/// and the menus list, and what [`Reading::file_menu`] counts in. `None` while the mail loads, after
/// an error and when it has none.
pub fn files_of<'r>(reading: &Reading<'r>) -> Option<Vec<&'r AttachmentInfo>> {
    let Some(Ok(mail)) = reading.loaded else { return None };
    let files: Vec<&AttachmentInfo> = mail.attachments.iter().filter(|file| !file.inline).collect();
    (!files.is_empty()).then_some(files)
}

/// "1 file · 3 KB", "3 files · 1 MB": how many attachments and how large they are together.
fn files_meta(files: &[&AttachmentInfo]) -> String {
    let count = match files.len() {
        1 => tr("1 file").to_owned(),
        n => trf("{n} files", &[("n", &n)]),
    };
    format!("{count} · {}", size_text(files.iter().map(|file| file.size).sum()))
}

/// "Save all ▾" from two attachments on, `None` for one: the click does what the setting says for all
/// of them ([`Message::SaveAll`], [`all_act`]), the triangle ("More ways to save all") opens their menu
/// ([`Message::AskSaveAll`]) at `anchors.save_all`. In the reader's block a ghost
/// (`widgets::split_action`), whose head frames it; `framed` (`widgets::split_action_framed`) over the
/// panel's rows, where nothing else does.
fn save_all_control<'a>(
    reading: &Reading<'_>,
    files: &[&AttachmentInfo],
    anchors: &detail::Anchors,
    framed: bool,
) -> Option<Element<'a, Message>> {
    (files.len() >= 2).then(|| {
        let id = reading.id.to_owned();
        let control = if framed { widgets::split_action_framed } else { widgets::split_action };
        // No tooltip on the action: its triangle has one, and two would stand over each other.
        control(
            Icon::Download,
            tr("Save all"),
            tr("More ways to save all"),
            Some(Message::SaveAll(id.clone())),
            Some(Message::AskSaveAll(id)),
            Some(&anchors.save_all),
        )
    })
}

/// The chips of the attachments `files`, wrapping on a line `room` wide, [`space::s()`] apart
/// ([`attachment_chip`]). At most [`FILE_LINES`] lines show; more scroll inside the block (scroll bar
/// beside the chips only then), so a mail with many attachments keeps its text in a low window.
fn attachment_chips<'a>(
    reading: &Reading<'_>,
    files: &[&AttachmentInfo],
    anchors: &detail::Anchors,
    room: f32,
) -> Element<'a, Message> {
    let chips = files.iter().enumerate().map(|(at, file)| attachment_chip(reading, at, file, anchors, room));
    let chips = row(chips).spacing(space::s()).wrap().vertical_spacing(space::s());
    container(bounded_scroll(chips)).max_height(chips_max_height()).into()
}

/// How many lines of chips the reader's attachment block shows before it scrolls.
const FILE_LINES: f32 = 2.0;

/// How many attachment rows the panel shows before they scroll.
const FILE_ROWS: f32 = 4.0;

/// The most the chips of the reader's block take: [`FILE_LINES`] chips and the gaps between them.
fn chips_max_height() -> f32 {
    FILE_LINES * size::control() + (FILE_LINES - 1.0) * space::s()
}

/// The most the panel's attachment rows take: [`FILE_ROWS`] rows of `size::control()`.
fn rows_max_height() -> f32 {
    FILE_ROWS * size::control()
}

/// `content` in a vertical scrollable as high as it is, up to what its parent allows: the suite's
/// scroll bar, which takes its room beside the content only while there is something to scroll, so
/// it never covers a chip's triangle or a row's.
fn bounded_scroll<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    scrollable(content)
        .direction(scrollable::Direction::Vertical(style::scrollbar().spacing(space::xs())))
        .style(style::scrollable)
        .width(Fill)
        .into()
}

/// The attachment `file`, the `at`-th of the mail's, as a split chip (`widgets::split_file_chip`) on
/// a line `room` wide: its name – cut with "…" where it would take more than half the line
/// ([`chip_name_room`]) – and its size. A click on it does what the setting says
/// ([`Message::OpenAttachment`], [`click_act`]), the tooltip says what and the whole name ("Open:
/// invoice.pdf", [`click_tooltip`]); the triangle opens its menu ([`Message::AskAttachment`]) and,
/// while that menu is open, carries its anchor (`anchors.attachment`).
fn attachment_chip<'a>(
    reading: &Reading<'_>,
    at: usize,
    file: &AttachmentInfo,
    anchors: &detail::Anchors,
    room: f32,
) -> Element<'a, Message> {
    let name = attachment_name(file);
    let weight = size_text(file.size);
    let shown = typo::ellipsize(&name, typo::body(), chip_name_room(&weight, room));
    let open = Some(Message::OpenAttachment(reading.id.to_owned(), file.part.clone()));
    let menu = Some(Message::AskAttachment(reading.id.to_owned(), at));
    let anchor = (reading.file_menu == Some(at)).then_some(&anchors.attachment);
    widgets::split_file_chip(shown, weight, click_tooltip(reading, file), open, menu, anchor)
}

/// The name of an attachment on one line, else "Attachment".
fn attachment_name(file: &AttachmentInfo) -> String {
    let name = file.filename.as_deref().map(list::one_line).filter(|name| !name.is_empty());
    name.unwrap_or_else(|| tr("Attachment").to_owned())
}

/// "Open: invoice.pdf", "Save to Invoices: invoice.pdf": what a click on the attachment's name does
/// ([`click_act`]) and its whole name, the tooltip of its chip and its row.
fn click_tooltip(reading: &Reading<'_>, file: &AttachmentInfo) -> String {
    let act = click_act(reading.click, reading.saving, &file.part);
    trf("{action}: {name}", &[("action", &act_words(&act, reading.saving, false)), ("name", &attachment_name(file))])
}

/// How wide the name on an attachment's chip may be ([`attachment_chip`]): half the line's `room`,
/// less what the chip needs beside the name – its padding, the paperclip, the gaps, its size `weight`
/// and the triangle after its line – so one long name never takes a line of its own.
fn chip_name_room(weight: &str, room: f32) -> f32 {
    let triangle = icon::SIZE_SM + 2.0 * space::s() + size::HAIRLINE;
    let frame = 2.0 * space::m() + icon::SIZE_SM + 2.0 * space::s() + typo::text_width(weight, typo::meta()) + triangle;
    (room / 2.0 - frame).max(0.0)
}

/// One button of the panel's mail standards ([`panel_standards`]).
#[derive(Clone)]
struct ReaderButton {
    icon: Icon,
    label: String,
    message: Option<Message>,
}

impl ReaderButton {
    fn new(icon: Icon, label: impl Into<String>, message: Option<Message>) -> Self {
        ReaderButton { icon, label: label.into(), message }
    }

    /// The button, [`size::control()`] high (`widgets::bar_button`): with its word in `style`, or
    /// its icon alone with the word as tooltip.
    fn view<'a>(self, words: bool, style: widgets::ButtonStyle) -> Element<'a, Message> {
        widgets::bar_button(self.icon, self.label, words, style, self.message)
    }
}

/// The mail standards of a mail in the panel (task E9.4, [`panel_standards`]): what every mail app
/// has, and nothing of Torro's. The mail window has its action rail instead (task E14.3,
/// [`action_groups`]).
struct Standards {
    /// Reply, Reply all, Forward (the first [`WRITE`]), then Archive and Delete – Delete off in the
    /// trash.
    lead: Vec<ReaderButton>,
    /// Flag or Unflag, Mark read or Mark unread: quiet icons after a line.
    tools: Vec<ReaderButton>,
}

/// How many of [`Standards::lead`] write (Reply, Reply all, Forward): a line follows them.
const WRITE: usize = 3;

/// The [`Standards`] of the mail in `reading` with its flags in `summary`; the panel has its ✕ in its
/// head. Each sends what its key sends ([`MailAction`]).
fn standards(reading: &Reading<'_>, summary: &MessageSummary) -> Standards {
    let id = reading.id;
    let mail = |action: MailAction| Some(Message::Mail(action, id.to_owned()));
    let flagged = summary.flags.contains(&Flag::Flagged);
    let read = summary.flags.contains(&Flag::Seen);
    let delete = (!reading.in_trash).then(|| Message::Mail(MailAction::Delete, id.to_owned()));
    let lead = vec![
        ReaderButton::new(Icon::Reply, tr("Reply"), mail(MailAction::Reply)),
        ReaderButton::new(Icon::ReplyAll, tr("Reply all"), mail(MailAction::ReplyAll)),
        ReaderButton::new(Icon::Forward, tr("Forward"), mail(MailAction::Forward)),
        ReaderButton::new(Icon::Archive, trc("action", "Archive"), mail(MailAction::Archive)),
        ReaderButton::new(Icon::Trash2, tr("Delete"), delete),
    ];
    let tools = vec![
        ReaderButton::new(
            Icon::Flag,
            if flagged { tr("Unflag") } else { tr("Flag") },
            mail(MailAction::Flag(!flagged)),
        ),
        if read {
            ReaderButton::new(Icon::Mail, tr("Mark unread"), mail(MailAction::Read(false)))
        } else {
            ReaderButton::new(Icon::MailOpen, tr("Mark read"), mail(MailAction::Read(true)))
        },
    ];
    Standards { lead, tools }
}

/// The mail standards at the top of the panel (task E9.4), in the reader's form – buttons
/// [`size::control()`] high, Reply primary with its word, the others `secondary` as icons, Flag and
/// read as quiet icons – in one row that wraps where the panel is narrower than its 400.
fn panel_standards<'a>(reading: &Reading<'a>, summary: &MessageSummary) -> Element<'a, Message> {
    let Standards { lead, tools } = standards(reading, summary);
    let mut bar = row![].spacing(space::xs()).align_y(Alignment::Center);
    for (i, button) in lead.into_iter().enumerate() {
        if i == WRITE {
            bar = bar.push(separator());
        }
        let style = if i == 0 { style::button::primary } else { style::button::secondary };
        bar = bar.push(button.view(i == 0, style));
    }
    bar = bar.push(separator());
    for button in tools {
        bar = bar.push(button.view(false, style::button::icon));
    }
    bar.wrap().vertical_spacing(space::xs()).into()
}

/// The line between groups of buttons in a bar, as high as a line of text, `space::xs()` beside it.
fn separator<'a>() -> Element<'a, Message> {
    let line = container(rule::vertical(size::HAIRLINE).style(style::rule)).height(typo::line_height(typo::body()));
    container(line).padding(Padding::new(0.0).left(space::xs()).right(space::xs())).into()
}

/// "Move to Info" or "Move to For me": the reader's button that moves the entry to `lane`.
fn lane_word(lane: Lane) -> String {
    let name = match lane {
        Lane::ForMe => tr("For me"),
        Lane::Info => tr("Info"),
    };
    trf("Move to {lane}", &[("lane", &name)])
}

/// Sender, recipients and date, as mail apps show them: the sender's name and address, "to …",
/// "cc …", and the moment with how long ago it was; a flagged mail says so.
fn header<'a>(reading: &Reading<'a>, summary: &'a MessageSummary) -> Element<'a, Message> {
    let mut lines = column![].spacing(space::xs());
    let from = summary.from.as_ref().map_or_else(|| tr("Unknown sender").to_owned(), person);
    lines = lines.push(typo::text(typo::body_strong(), from).style(theme::text).wrapping(Wrapping::WordOrGlyph));
    if !summary.to.is_empty() {
        let to = trf("to {names}", &[("names", &names(&summary.to))]);
        lines = lines.push(typo::text(typo::meta(), to).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    if let Some(Ok(mail)) = reading.loaded
        && !mail.cc.is_empty()
    {
        let cc = trf("cc {names}", &[("names", &names(&mail.cc))]);
        lines = lines.push(typo::text(typo::meta(), cc).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    let mut meta = row![].spacing(space::s()).align_y(Alignment::Center);
    if reading.dated {
        meta = meta.push(typo::text(typo::meta().tabular(), when(summary.date, reading.now)).style(theme::text_3));
    }
    if summary.flags.contains(&Flag::Flagged) {
        let flagged = row![small_icon(Icon::Flag, Some(reading.attention)), typo::text(typo::meta(), tr("Flagged"))]
            .spacing(space::xs())
            .align_y(Alignment::Center);
        meta = meta.push(flagged);
    }
    lines.push(meta).into()
}

/// "Sat, 3 Oct · 10:26 · 4 min ago": the moment in local time and how long ago it was.
pub fn when(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (local, today) = (at.with_timezone(&Local), now.with_timezone(&Local).date_naive());
    format!(
        "{} · {} · {}",
        datetime::format_short_date(local.date_naive(), today),
        datetime::format_time(local.time()),
        list::ago_long(at, now)
    )
}

/// The mail's attachments as rows (task M12.6, design B of `docs/prototypes/inbox/mail-anhaenge.html`):
/// "Attachments" and "{n} files · {size}" ([`files_meta`]) over them with "Save all ▾" at its right
/// from two on ([`save_all_control`], framed), then one row each ([`attachment_row`]); beyond
/// [`FILE_ROWS`] rows they scroll, so the footer never pushes the mail out of a low panel. For the
/// panel – its footer in the mail window ([`panel_foot`]), over the mail in the hub and the App Feed
/// ([`Bars::Split`]). `None` where [`files_of`] has none.
fn attachment_rows<'a>(reading: &Reading<'_>, anchors: &detail::Anchors) -> Option<Element<'a, Message>> {
    let files = files_of(reading)?;
    let title = typo::text(typo::caption(), tr("Attachments")).style(theme::text_3).wrapping(Wrapping::None);
    let meta = typo::text(typo::meta().tabular(), files_meta(&files)).style(theme::text_3).wrapping(Wrapping::None);
    let mut head = row![container(title).width(Fill).clip(true), meta].spacing(space::s()).align_y(Alignment::Center);
    if let Some(all) = save_all_control(reading, &files, anchors, true) {
        head = head.push(all);
    }
    let head = container(head).padding(Padding { bottom: space::xs(), ..style::ITEM_PADDING });
    let rows = files.iter().enumerate().map(|(at, file)| attachment_row(reading, at, file, anchors));
    let rows = container(bounded_scroll(column(rows))).max_height(rows_max_height());
    Some(column![head, rows].into())
}

/// The attachment `file`, the `at`-th of the mail's, as a row: the paperclip, its name and size – a
/// click does what the setting says ([`Message::OpenAttachment`], tooltip [`click_tooltip`]) – and at
/// the right its triangle, which opens its menu ([`Message::AskAttachment`]) and carries the menu's
/// anchor while it is open (`anchors.attachment`).
fn attachment_row<'a>(
    reading: &Reading<'_>,
    at: usize,
    file: &AttachmentInfo,
    anchors: &detail::Anchors,
) -> Element<'a, Message> {
    let line = row![
        container(widgets::prop_value(attachment_name(file))).width(Fill),
        typo::text(typo::meta().tabular(), size_text(file.size)).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let open = Message::OpenAttachment(reading.id.to_owned(), file.part.clone());
    let name = widgets::with_tooltip(
        widgets::prop_row(Some(Mark::Icon(Icon::Paperclip)), line, Some(open)),
        click_tooltip(reading, file),
    );
    let menu = Some(Message::AskAttachment(reading.id.to_owned(), at));
    let triangle = widgets::icon_button(Icon::ChevronDown, tr("More ways to open or save"), menu);
    let triangle = widgets::anchored_if((reading.file_menu == Some(at)).then_some(&anchors.attachment), triangle);
    row![container(name).width(Fill), triangle].spacing(space::xs()).align_y(Alignment::Center).into()
}

/// The text of `mail` as the window shows, selects and copies it (issue #119): plain, without
/// control characters ([`list::plain`]); empty for a mail without text.
pub fn text_of(mail: &Mail) -> String {
    mail.text.as_deref().map(list::plain).unwrap_or_default()
}

/// The mail's HTML ([`HtmlReading`]) as wide as its parent gives, links reported as
/// [`Message::HtmlLink`]. The mail paints its background up to the view's edges, so the caller
/// places it without padding and sets the margin of a mail without a background of its own.
fn html_view<'a>(html: HtmlReading<'a>) -> HtmlView<'a, Message> {
    HtmlView::new(html.content).scheme(html.scheme).scale(html.scale).on_link(Message::HtmlLink)
}

/// The note under a mail shown as HTML: it comes from outside, no scripts run. "Copy text" stands in
/// the rail's "More" (task E14.3) and the panel's footer (task E9.4), not beside it.
fn html_note<'a>() -> Element<'a, Message> {
    note(tr("Mail from outside the suite: no scripts run, pictures from the web are loaded."))
}

/// The text in `level`, to select with the mouse and copy (issue #119, [`Selectable`]), then the
/// note on foreign text. "Copy text" stands in the rail's "More" (task E14.3) and the panel's footer (task E9.4). The
/// web addresses in the text follow as "Links" ([`text_links`]), which the caller places.
fn text_block<'a>(text: &'a Selectable, level: typo::Level) -> Element<'a, Message> {
    let mut block = column![].spacing(space::s());
    if !text.text().trim().is_empty() {
        block = block.push(text.view(level, theme::text_2, Message::Text));
    }
    // No title over the text: the headers above already say whose mail it is (and "Message" reads
    // as a notice in German, the entries' word).
    block.push(note(detail::foreign_note())).into()
}

/// "Links": the web addresses in the mail's text (`torro_gui::selectable::web_links`), each a row
/// that opens it ([`Message::OpenMailLink`]); `None` without any. The text itself is selectable and
/// cannot hold clickable links, so they stand here.
fn text_links<'a>(text: &Selectable) -> Option<Element<'a, Message>> {
    selectable::links_section(selectable::web_links(text.text()), Message::OpenMailLink)
}

/// "Copy text": puts the whole text shown on the clipboard ([`Message::CopyText`]), as an icon
/// with its words as tooltip.
pub fn copy_button<'a>() -> Element<'a, Message> {
    widgets::icon_button(Icon::Copy, tr("Copy text"), Some(Message::CopyText))
}

/// A quiet sentence (`META`, `text_3`).
fn note<'a>(text: &'a str) -> Element<'a, Message> {
    typo::text(typo::meta(), text).style(theme::text_3).wrapping(Wrapping::WordOrGlyph).into()
}

/// The day a task is due ([`ItemTask::due`]), in local time.
pub fn due_day(due: TaskDue) -> NaiveDate {
    match due {
        TaskDue::Date(day) => day,
        TaskDue::DateTime(at) => at.with_timezone(&Local).date_naive(),
    }
}

/// The line under a task's title on its card as a whole, as [`task_card`] shows it where there is
/// room.
#[cfg(test)]
fn task_meta(task: &ItemTask, now: DateTime<Utc>) -> String {
    task_meta_parts(task, now).join(" · ")
}

/// The parts of the line under a task's title on its card (task E8.7b): "Task · Work · due Fri, 17
/// Oct · open" – the list when the core knows its name, the due day when there is one, and whether
/// it is open or done (in words, never by colour alone). A narrow card leaves some out
/// ([`task_meta_drop`]).
fn task_meta_parts(task: &ItemTask, now: DateTime<Utc>) -> Vec<String> {
    let today = now.with_timezone(&Local).date_naive();
    let mut parts = vec![tr("Task").to_owned()];
    let list = list::one_line(&task.list);
    if !list.is_empty() {
        parts.push(list);
    }
    if let Some(due) = task.due {
        let day = datetime::format_short_date(due_day(due), today);
        parts.push(trf("due {day}", &[("day", &day)]));
    }
    parts.push(if task.completed { tr("done") } else { tr("open") }.to_owned());
    parts
}

/// Which parts of [`task_meta_parts`] (`count` of them) a narrow task card leaves out, in this order
/// (task E8.12, finding 2): first "Task" – the ring and "Open task" say it –, then "open" or "done",
/// which the ring or the check shows by its form; the list and the due day stay longest.
fn task_meta_drop(count: usize) -> Vec<usize> {
    if count < 2 { Vec::new() } else { vec![0, count - 1] }
}

/// The task made of a mail, as a card over the mail's text (task E8.7b, `docs/eingang.md`, "Aufgabe
/// mit Rückbezug"): a ring for an open task, a check in the colour for "OK" for a done one, its title
/// and its meta line ([`task_meta_parts`]) on one line each, and "Open task", which opens it in the
/// tasks app through the core (`apps.open_app` with `task:<uid>`). The title is foreign text like
/// the mail's and shows as it is.
///
/// Laid out at its width (task E8.12, finding 2, `widgets::card_line`): where the meta line does
/// not fit it leaves out "Task", then the state ([`task_meta_drop`]), and cuts the rest with "…"
/// and a tooltip; "Open task" becomes its icon before it pushes the title away.
pub fn task_card<'a>(task: &ItemTask, now: DateTime<Utc>) -> Element<'a, Message> {
    let completed = task.completed;
    let mark = move || -> Element<'a, Message> {
        if completed {
            icon::icon(Icon::CircleCheck, icon::SIZE)
                .style(|t: &Theme, _| svg::Style { color: Some(palette(t).ok) })
                .into()
        } else {
            task_ring()
        }
    };
    let meta = task_meta_parts(task, now);
    let drop = task_meta_drop(meta.len());
    let open = widgets::RowAction::new(Icon::ArrowUpRight, tr("Open task"), "", Message::OpenTask(task.uid.clone()));
    let line = widgets::card_line(mark, icon::SIZE, list::one_line(&task.title), meta, drop, open);
    widgets::card(vec![line], false, None)
}

/// The open task's ring: the tasks app's check before it is ticked, as large as an icon.
fn task_ring<'a>() -> Element<'a, Message> {
    let edge = icon::SIZE;
    container(iced::widget::Space::new())
        .width(edge)
        .height(edge)
        .style(move |t: &Theme| container::Style {
            border: iced::border::rounded(edge / 2.0).color(palette(t).text_3).width(style::CHECK_BORDER),
            ..container::Style::default()
        })
        .into()
}

// ----- Saving attachments (task M12.6, issue #290) --------------------------------------------

/// One way to open or save attachments (task M12.6): what a click on an attachment's name or on
/// "Save all" does by the setting `mail_attachments.click` ([`click_act`], [`all_act`]), and every
/// entry of their menus ([`file_menu`]). The window carries it out (`Message::FileAct`); every save is
/// one `mail.save_attachments`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAct {
    /// Fetch the attachment and open it with the system ([`open_attachment`]); one attachment only.
    Open,
    /// Save to the Downloads folder (`to: downloads`).
    Downloads,
    /// Ask where first: the system's save dialog for one attachment (`to: file`), its folder dialog
    /// for all of them (`to: folder`). Cancelled, nothing is sent.
    Choose,
    /// Save to the save target with this id (`to: {"target": id}`).
    Target(String),
}

/// What a click on the name of the attachment `part` does (task M12.6) with the setting `click` and
/// the mail's plans `saving` (`None` while they load or when they could not be loaded): Open, Save to
/// Downloads and Save as… as set; a target while `saving` lists it – one that is gone opens –, and
/// without plans the target all the same: the core knows its targets and answers for one that is
/// gone, where opening instead would do something else unasked. "Suggested target, else open" the
/// plan's `sure` target – one whose conditions hold without a category the model only guessed
/// (decision 4 of issue #290) –, else it opens.
pub fn click_act(click: &AttachmentClick, saving: Option<&SaveTargets>, part: &str) -> FileAct {
    match click {
        AttachmentClick::Open => FileAct::Open,
        AttachmentClick::Downloads => FileAct::Downloads,
        AttachmentClick::SaveAs => FileAct::Choose,
        AttachmentClick::Target(id) => set_target(saving, id, FileAct::Open),
        AttachmentClick::Suggested => saving
            .and_then(|saving| saving.plan(part)?.sure.as_deref())
            .and_then(|id| listed(saving, id))
            .unwrap_or(FileAct::Open),
    }
}

/// What a click on "Save all" does (task M12.6): Open and "Suggested target, else open" save to
/// Downloads – there is nothing to open for several files, and each file has its own suggestion –,
/// Save as… asks for a folder, a target saves there while `saving` lists it or without plans (as in
/// [`click_act`]), one that is gone to Downloads.
pub fn all_act(click: &AttachmentClick, saving: Option<&SaveTargets>) -> FileAct {
    match click {
        AttachmentClick::Open | AttachmentClick::Suggested | AttachmentClick::Downloads => FileAct::Downloads,
        AttachmentClick::SaveAs => FileAct::Choose,
        AttachmentClick::Target(id) => set_target(saving, id, FileAct::Downloads),
    }
}

/// The target `id` of the setting: while `saving` lists it, `gone` when it lists it no longer, and
/// the target unchecked without plans – the core says when it is gone.
fn set_target(saving: Option<&SaveTargets>, id: &str, gone: FileAct) -> FileAct {
    match saving {
        Some(_) => listed(saving, id).unwrap_or(gone),
        None => FileAct::Target(id.to_owned()),
    }
}

/// [`FileAct::Target`] for the target `id` while `saving` lists it.
fn listed(saving: Option<&SaveTargets>, id: &str) -> Option<FileAct> {
    saving?.target(id).map(|target| FileAct::Target(target.id.clone()))
}

/// The words of `act` – for "Save all" (`all`) or one attachment: "Open", "Save to Downloads" / "Save
/// all to Downloads", "Save as…" / "Save all to folder…", "Save to Invoices" with the target's name
/// from `saving` ("Save to Unknown target" for one it does not list: the setting's target while the
/// plans are not in, [`click_act`]).
pub fn act_words(act: &FileAct, saving: Option<&SaveTargets>, all: bool) -> String {
    match (act, all) {
        (FileAct::Open, _) => tr("Open").to_owned(),
        (FileAct::Downloads, false) => tr("Save to Downloads").to_owned(),
        (FileAct::Downloads, true) => tr("Save all to Downloads").to_owned(),
        (FileAct::Choose, false) => tr("Save as…").to_owned(),
        (FileAct::Choose, true) => tr("Save all to folder…").to_owned(),
        (FileAct::Target(id), _) => {
            let name = saving.and_then(|saving| saving.target(id)).map(|target| list::one_line(&target.spec.name));
            trf("Save to {target}", &[("target", &name.unwrap_or_else(|| tr("Unknown target").to_owned()))])
        }
    }
}

/// "Invoices › 2026 › 03": the target and the subfolders the attachment `part` would go to in it, as
/// its plan in `saving` says – the menu's detail and the hint after saving. The target's name alone
/// without a plan (`part` `None`: "Save all", each file by its own date).
pub fn place_words(saving: &SaveTargets, target: &str, part: Option<&str>) -> String {
    let name =
        saving.target(target).map_or_else(|| tr("Unknown target").to_owned(), |found| list::one_line(&found.spec.name));
    let place =
        part.and_then(|part| saving.plan(part)).and_then(|plan| plan.places.iter().find(|p| p.target == target));
    match place {
        Some(place) => std::iter::once(name)
            .chain(place.folders.iter().map(|folder| list::one_line(folder)))
            .collect::<Vec<_>>()
            .join(" › "),
        None => name,
    }
}

/// Whether "Suggested" at the target carries the AI's sparkles (decision 7 of issue #290's plan): its
/// suggestion rests on a category the model guessed – the target asks for categories and the mail's
/// `category` is a guess. A suggestion by sender or file type is no AI and has none.
fn suggested_by_model(target: &torro_mail::SaveTarget, category: Option<MailCategory>) -> bool {
    !target.spec.suggest.categories.is_empty() && category.is_some_and(MailCategory::is_suggestion)
}

/// What the menu of an attachment's triangle or of "Save all ▾" shows (task M12.6, [`file_menu`]).
#[derive(Debug, Clone, Copy)]
pub struct FileMenu<'a> {
    /// The message's id.
    pub id: &'a str,
    /// The attachment's part; `None` for "Save all".
    pub part: Option<&'a str>,
    /// The save targets and plans of the mail; `None` while they load or when they failed.
    pub saving: Option<&'a SaveTargets>,
    /// What a click on the attachment's name does: its entry carries "Click".
    pub click: &'a AttachmentClick,
    /// The mail's category, for the sparkles of "Suggested".
    pub category: Option<MailCategory>,
}

/// The sections of the menu of an attachment or of "Save all" (task M12.6, design A):
///
/// 1. Open (`ArrowUpRight`, one attachment only), Save to Downloads (`Download`) and Save as…
///    (`Save`) – for all of them "Save all to Downloads" and "Save all to folder…";
/// 2. "Save to": the save targets (`Folder`), those suggested for the attachment first in the plan's
///    order, the others in the person's; for one attachment with where it would go as detail
///    ([`place_words`]) and "Suggested" – with the sparkles where it rests on a guessed category –,
///    for all of them "by each file's date";
/// 3. "Manage save targets…" ([`Message::ManageSaveTargets`]).
///
/// The entry a click on the attachment's name does ([`click_act`]) carries "Click"; "Save all" has
/// none. Every entry sends [`Message::FileAct`] for the attachment, or for all with `part` `None`.
/// Without targets – none set up, or still loading – section 2 is left out.
pub fn file_menu<'a>(menu: FileMenu<'_>) -> Vec<MenuSection<'a, Message>> {
    let all = menu.part.is_none();
    let click = menu.part.map(|part| click_act(menu.click, menu.saving, part));
    let entry = |icon: Icon, act: FileAct| {
        let words = act_words(&act, menu.saving, all);
        let clicked = click.as_ref() == Some(&act);
        let item = MenuItem::new(icon, words, Message::FileAct(menu.id.to_owned(), menu.part.map(str::to_owned), act));
        if clicked { item.tag(tr("Click"), false) } else { item }
    };
    let mut ways = Vec::new();
    if !all {
        ways.push(entry(Icon::ArrowUpRight, FileAct::Open));
    }
    ways.push(entry(Icon::Download, FileAct::Downloads));
    ways.push(entry(Icon::Save, FileAct::Choose));
    let mut sections = vec![MenuSection::new(ways)];
    if let Some(saving) = menu.saving.filter(|saving| !saving.targets.is_empty()) {
        let plan = menu.part.and_then(|part| saving.plan(part));
        let suggested: Vec<&str> =
            plan.map(|plan| plan.suggested.iter().map(String::as_str).collect()).unwrap_or_default();
        let mut targets: Vec<&torro_mail::SaveTarget> = suggested.iter().filter_map(|id| saving.target(id)).collect();
        targets.extend(saving.targets.iter().filter(|target| !suggested.contains(&target.id.as_str())));
        let items = targets
            .into_iter()
            .map(|target| {
                let act = FileAct::Target(target.id.clone());
                let detail = match menu.part {
                    Some(part) => place_words(saving, &target.id, Some(part)),
                    None => tr("by each file's date").to_owned(),
                };
                let clicked = click.as_ref() == Some(&act);
                let message = Message::FileAct(menu.id.to_owned(), menu.part.map(str::to_owned), act);
                let mut item = MenuItem::new(Icon::Folder, list::one_line(&target.spec.name), message).detail(detail);
                if suggested.contains(&target.id.as_str()) {
                    item = item.tag(tr("Suggested"), suggested_by_model(target, menu.category));
                }
                if clicked { item.tag(tr("Click"), false) } else { item }
            })
            .collect();
        sections.push(MenuSection::titled(tr("Save to"), items));
    }
    let manage = MenuItem::new(Icon::Settings, tr("Manage save targets…"), Message::ManageSaveTargets);
    sections.push(MenuSection::new(vec![manage]));
    sections
}

/// The menu card of [`file_menu`]'s `sections`, `width` wide and at most `tallest` high – it scrolls
/// beyond: the card of `widgets::menu`, wider, so the path and the two words of an entry stay
/// readable (`widgets::menu_width` cut "Invoices › 2026 › 03" beside "Suggested" and "Click").
pub fn file_menu_card<'a>(sections: Vec<MenuSection<'a, Message>>, width: f32, tallest: f32) -> Element<'a, Message> {
    let mut list = column![];
    for (at, section) in sections.into_iter().enumerate() {
        if at > 0 {
            list = list.push(crate::app::menu_line());
        }
        if let Some(title) = section.title {
            list = list.push(crate::app::menu_caption(&title));
        }
        list = list.extend(section.items.into_iter().map(widgets::menu_item));
    }
    let body = scrollable(list).height(iced::Length::Shrink).width(Fill);
    container(container(body).max_height(tallest))
        .width(width)
        .padding(style::MENU_INSET)
        .style(style::container::menu)
        .into()
}

/// What the hint says after `mail.save_attachments` answered `saved` (task M12.6) for a save to
/// `place` – the words of where ("Downloads", "Invoices › 2026 › 03", a folder's name): one file
/// "Saved to {where}", several "{n} attachments saved to {where}"; when some failed "{n} saved,
/// {name} failed" with the names of those. A file found there already with the same content counts as
/// saved: it is there.
pub fn saved_text(saved: &SavedAttachments, place: &str) -> String {
    let there = saved.saved.len() + saved.skipped.len();
    if !saved.failed.is_empty() {
        let names: Vec<String> = saved.failed.iter().map(|failed| list::one_line(&failed.name)).collect();
        return trf("{n} saved, {name} failed", &[("n", &there), ("name", &names.join(", "))]);
    }
    match there {
        1 => trf("Saved to {where}", &[("where", &place)]),
        n => trf("{n} attachments saved to {where}", &[("n", &n), ("where", &place)]),
    }
}

/// The folder "Show in folder" opens after a save (task M12.6): the one of the first file saved –
/// or found there with the same content –, as `open_attachment` opens the folder of a file that could
/// run programs. `None` when nothing lies anywhere.
pub fn saved_folder(saved: &SavedAttachments) -> Option<PathBuf> {
    let first = saved.saved.first().or_else(|| saved.skipped.first())?;
    Path::new(&first.path).parent().map(Path::to_path_buf)
}

/// Opens `folder` in the system's file manager ("Show in folder", task M12.6).
pub async fn show_folder(folder: PathBuf) -> Result<(), String> {
    start(opener(&folder))
}

/// The words of where a save to `to` goes, for its hint ([`saved_text`]), taken when it is sent – the
/// person may open another mail before the answer: "Downloads", the target with the subfolders of the
/// attachment `part` ([`place_words`]; for all of them the target alone), the name of the folder
/// chosen, or of the file's folder for "Save as…". `None` for a target `saving` does not list (no
/// plans yet): the hint then names the folder the file went to ([`saved_place`]).
pub fn save_place(to: &SaveTo, saving: Option<&SaveTargets>, part: Option<&str>) -> Option<String> {
    let named = |path: &Path| path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    match to {
        SaveTo::Downloads => Some(tr("Downloads").to_owned()),
        SaveTo::Target(id) => {
            saving.filter(|saving| saving.target(id).is_some()).map(|saving| place_words(saving, id, part))
        }
        SaveTo::File(file) => Some(Path::new(file).parent().map(named).unwrap_or_default()),
        SaveTo::Folder(folder) => Some(named(Path::new(folder))),
    }
}

/// The words of where a save went for its hint: `place` as [`save_place`] took it, else the name of
/// the folder the first file went to ([`saved_folder`]).
pub fn saved_place(place: Option<String>, saved: &SavedAttachments) -> String {
    place
        .or_else(|| saved_folder(saved)?.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// The name the save dialog of "Save as…" starts with (task M12.6): the plan's cleaned name, else the
/// attachment's own, else "attachment".
pub fn dialog_name(saving: Option<&SaveTargets>, file: &AttachmentInfo) -> String {
    match saving.and_then(|saving| saving.plan(&file.part)) {
        Some(plan) => plan.file_name.clone(),
        None => file
            .filename
            .as_deref()
            .map(torro_mail::save_target::clean_segment)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "attachment".to_owned()),
    }
}

/// The demo's plans of `mail`'s attachments in `targets` (issue #290), made as the core makes them
/// (`plan` in the core's `mail/save.rs`): the facts of the mail – its local day, sender, subject, the
/// account's address –, a place in each target whose schemes resolve, the targets suggested in their
/// order and the first sure one.
fn demo_plans(mail: &Mail, targets: &[torro_mail::SaveTarget]) -> Vec<AttachmentPlan> {
    let summary = &mail.summary;
    let months: [String; 12] = std::array::from_fn(|at| datetime::month_name(at as u32 + 1).to_owned());
    let sender = summary.from.as_ref();
    mail.attachments
        .iter()
        .filter(|file| !file.inline)
        .map(|file| {
            let facts = torro_mail::FileFacts {
                mail_date: summary.date.with_timezone(&Local).date_naive(),
                file_name: file.filename.clone(),
                content_type: file.content_type.clone(),
                sender_name: sender.and_then(|from| from.name.clone()),
                sender_address: sender.map(|from| from.address.clone()).unwrap_or_default(),
                account: address(&summary.account).to_owned(),
                subject: summary.subject.clone(),
                month_names: months.clone(),
            };
            let kind = torro_mail::FileKind::of(&file.content_type, file.filename.as_deref());
            let mut plan = AttachmentPlan {
                part: file.part.clone(),
                file_name: dialog_name(None, file),
                places: Vec::new(),
                suggested: Vec::new(),
                sure: None,
            };
            for target in targets {
                if let Ok(resolved) = torro_mail::save_target::resolve(&target.spec, &facts) {
                    let (folders, file) = (resolved.folders, resolved.file);
                    plan.places.push(PlannedPlace { target: target.id.clone(), folders, file });
                }
                let found = torro_mail::save_target::suggestion(
                    &target.spec.suggest,
                    summary.category,
                    kind,
                    &facts.sender_address,
                );
                if let Some(found) = found {
                    plan.suggested.push(target.id.clone());
                    if found.sure && plan.sure.is_none() {
                        plan.sure = Some(target.id.clone());
                    }
                }
            }
            plan
        })
        .collect()
}

/// What the demo answers to `mail.save_attachments` (issue #290): where the core would have put each
/// of `parts` of `mail` (none: every attachment that is not `inline`) for `to`, without writing a
/// file. [`InboxError::NotFound`] for an unknown part or target, [`InboxError::Other`] for "Save
/// as…" with more than one file, as the core answers.
fn demo_saved(mail: &Mail, parts: &[String], to: &SaveTo) -> Result<SavedAttachments, InboxError> {
    let targets = torro_inbox::demo::save_targets();
    let plans = demo_plans(mail, &targets);
    let wanted: Vec<&AttachmentPlan> = if parts.is_empty() {
        plans.iter().collect()
    } else {
        parts
            .iter()
            .map(|part| plans.iter().find(|plan| plan.part == *part).ok_or(InboxError::NotFound))
            .collect::<Result<_, _>>()?
    };
    if matches!(to, SaveTo::File(_)) && wanted.len() != 1 {
        return Err(InboxError::Other("one file at a time".to_owned()));
    }
    let mut saved = Vec::new();
    for plan in wanted {
        let path = match to {
            SaveTo::Downloads => Path::new(torro_inbox::demo::DEMO_DOWNLOADS).join(&plan.file_name),
            SaveTo::File(file) => PathBuf::from(file),
            SaveTo::Folder(folder) => Path::new(folder).join(&plan.file_name),
            SaveTo::Target(id) => {
                let target = targets.iter().find(|target| target.id == *id).ok_or(InboxError::NotFound)?;
                let place = plan.places.iter().find(|place| place.target == *id).ok_or(InboxError::NotFound)?;
                place
                    .folders
                    .iter()
                    .fold(PathBuf::from(&target.spec.folder), |path, folder| path.join(folder))
                    .join(&place.file)
            }
        };
        saved.push(SavedFile { part: plan.part.clone(), path: path.to_string_lossy().into_owned() });
    }
    Ok(SavedAttachments { saved, ..SavedAttachments::default() })
}

/// The hint after opening an attachment.
pub fn opened_text(opened: &Opened) -> String {
    match opened {
        Opened::File(path) => trf("Opened {file}", &[("file", &file_name(path))]),
        Opened::Folder(path) => {
            trf("{file} can run programs, so only its folder was opened.", &[("file", &file_name(path))])
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

// ----- "Focus & reply" (task E19.5) -------------------------------------------------------------

/// The id of the reply field of "Focus & reply" (task E19.5): the window puts the keyboard there
/// when a round starts, so the first reply can be typed at once.
pub const FOCUS_FIELD: &str = "focus-reply";

/// The id of the scrolling mail of "Focus & reply": the window scrolls it back to the top when
/// another mail shows.
pub const FOCUS_MAIL: &str = "focus-mail";

/// How many lines the reply field of "Focus & reply" shows empty, and how many it grows to before
/// it scrolls – never more than [`FIELD_SHARE`] of the page, so the mail above keeps its room.
const FIELD_LINES: f32 = 4.0;
const FIELD_MAX_LINES: f32 = 10.0;
const FIELD_SHARE: f32 = 0.4;

/// "Focus & reply" (task E19.5, decision 10 of E19): one round through the pile "Reply later", a
/// mail at a time with a field to answer it under the mail.
///
/// [`FocusReply::ids`] are the pile's mail as the view listed it when the round started, in its
/// order. A mail answered ([`FocusReply::answered`]) or taken off the pile
/// ([`FocusReply::take_current`]) leaves the round, so [`FocusReply::at`] then already points at the
/// next one; a skipped one stays on the pile and in the round ([`FocusReply::step`]), the arrows go
/// back to it. `at == ids.len()` is the end of the round: nothing left to reply to in it.
///
/// What is typed belongs to its mail: [`FocusReply::reply`] is the field of the mail shown, the
/// replies typed to the others wait until their mail shows again – skipping and ↑/↓ lose nothing.
/// While a reply is on its way ([`FocusReply::sending`]) nothing moves, so the answer always
/// belongs to the mail shown. [`FocusReply::keep`] and [`FocusReply::restore`] carry a round over
/// the hint "Reply discarded." and its undo.
#[derive(Debug, Clone, Default)]
pub struct FocusReply {
    /// The mail of the round (`m:…`), in the order of the pile when it started.
    pub ids: Vec<String>,
    /// The place of the mail shown in [`FocusReply::ids`]; `ids.len()` at the end.
    pub at: usize,
    /// What is typed into the field for the mail shown.
    pub reply: text_editor::Content,
    /// What was typed for the other mails of the round, by id; only replies with text.
    kept: HashMap<String, text_editor::Content>,
    /// The reply is on its way (`MailSource::send_reply`): "Sending…", the field and the buttons
    /// take nothing until the answer.
    pub sending: bool,
    /// Why the last send failed (the core's words); the text stays in the field to send again.
    pub error: Option<String>,
    /// When the reply now on its way went out, for the window's time limit (`App` gives up after
    /// `SEND_TIMEOUT`, fifth review of #315); `None` while nothing is on its way.
    pub sending_since: Option<std::time::Instant>,
    /// The number of the send on its way (the window counts its sends, review of PR #327): only its
    /// answer counts for the round; `None` while nothing is on its way.
    pub send: Option<u64>,
    /// The last send had no answer in time ([`FocusReply::give_up`]): under the field "No answer yet
    /// – check Sent before sending again.", the text stays.
    pub timed_out: bool,
    /// Every mail that was ever in the round – answered, taken off or skipped –, so the next page
    /// of the pile adds only mail the round has not had ([`FocusReply::extend`], decision of #315).
    seen: HashSet<String>,
    /// The pile may hold more than the round has: at its end the window loads the next page
    /// ([`FocusReply::wants_more`]); `false` once the pile had nothing new.
    pub more: bool,
    /// The next page is on its way; the end of the round shows loading until it is in.
    pub loading: bool,
    /// Why the next page could not be loaded; the end of the round says so with "Retry".
    pub load_error: Option<String>,
    /// How much mail lay on the pile when the round started (`count_in_pile`, the sidebar's number),
    /// less what left it since: the `{total}` of "{n} of {total}"; `None` when not known.
    pub total: Option<usize>,
    /// Which round this is, set by the window for every round it starts or brings back: the answer
    /// of a next page carries it and counts only for its own round (third review of #315).
    pub round: u64,
    /// The pile and since when of the mail the round loaded itself ([`FocusReply::extend`]), which the
    /// window lists nowhere: "Remove" and its undo find them here (third review of #315).
    piles: HashMap<String, (Option<MailPile>, Option<DateTime<Utc>>)>,
}

/// A round of "Focus & reply" as the undo of "Reply discarded." keeps it ([`FocusReply::keep`]):
/// its mail, the place and the text typed for each mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusKept {
    pub ids: Vec<String>,
    pub at: usize,
    /// The replies with text, by the id of their mail.
    pub texts: Vec<(String, String)>,
    /// Why the reply to the mail at `at` could not be sent, when the round comes back for it
    /// (a send that failed after its round was closed, second review of #315).
    pub error: Option<String>,
    /// Every mail the round had – the one on its way too –, so the round that comes back never takes
    /// one of them in again (third review of #315: no second send).
    pub seen: Vec<String>,
    /// The pile's number as the round counted it ([`FocusReply::total`]).
    pub total: Option<usize>,
    /// The piles of the mail the round loaded itself ([`FocusReply::pile_of`]).
    pub piles: Vec<(String, Option<MailPile>, Option<DateTime<Utc>>)>,
}

impl FocusReply {
    /// A round through `ids`, at the first, with an empty field.
    pub fn new(ids: Vec<String>) -> FocusReply {
        let seen = ids.iter().cloned().collect();
        FocusReply { ids, seen, ..FocusReply::default() }
    }

    /// A round through the pile (decision of #315): `ids` as loaded, `more` when the pile has
    /// further pages, `total` the pile's number when it is known.
    pub fn of_pile(ids: Vec<String>, more: bool, total: Option<usize>) -> FocusReply {
        FocusReply { more, total, ..FocusReply::new(ids) }
    }

    /// Whether the window should load the next page now: at the end of the round, with more on the
    /// pile, nothing on its way and no error waiting for "Retry".
    pub fn wants_more(&self) -> bool {
        self.current().is_none() && self.more && !self.loading && self.load_error.is_none()
    }

    /// The mail the round has had, for the next page ([`FocusReply::extend`]).
    pub fn seen(&self) -> &HashSet<String> {
        &self.seen
    }

    /// The next page is in: `found` – the pile's mail the round has not had yet, in the pile's order
    /// – go at the end of the round with their piles ([`FocusReply::pile_of`]), and the first of them
    /// shows when the round stood at its end; `more` says whether the pile has still further pages.
    pub fn extend(&mut self, found: Vec<MessageSummary>, more: bool) {
        let at_end = self.current().is_none();
        for message in found {
            if self.seen.insert(message.id.clone()) {
                self.piles.insert(message.id.clone(), (message.pile, message.pile_since));
                self.ids.push(message.id);
            }
        }
        self.more = more;
        self.loading = false;
        self.load_error = None;
        if at_end {
            self.arrive();
        }
    }

    /// The pile and since when of `id`, when the round loaded it itself ([`FocusReply::extend`]);
    /// `None` for mail the window lists.
    pub fn pile_of(&self, id: &str) -> Option<(Option<MailPile>, Option<DateTime<Utc>>)> {
        self.piles.get(id).copied()
    }

    /// `id` now lies on `pile` since `since`, where the round keeps its pile.
    pub fn set_pile(&mut self, id: &str, pile: Option<MailPile>, since: Option<DateTime<Utc>>) {
        if let Some(held) = self.piles.get_mut(id) {
            *held = (pile, since);
        }
    }

    /// The mail shown; `None` at the end of the round.
    pub fn current(&self) -> Option<&str> {
        self.ids.get(self.at).map(String::as_str)
    }

    /// "{n} of {total}" of the mail shown, from 1; `None` at the end. The total is the pile's, as far
    /// as it is known – never less than the round holds.
    pub fn position(&self) -> Option<(usize, usize)> {
        let total = self.total.unwrap_or(0).max(self.ids.len());
        self.current().map(|_| (self.at + 1, total))
    }

    /// One mail less on the pile, for the total.
    fn left_pile(&mut self) {
        self.total = self.total.map(|total| total.saturating_sub(1));
    }

    /// Whether [`FocusReply::step`] can go that way: forward from every mail – after the last one to
    /// the end –, back from any place after the first, also from the end. Never while sending.
    pub fn can_step(&self, forward: bool) -> bool {
        match (self.sending, forward) {
            (true, _) => false,
            (false, true) => self.at < self.ids.len(),
            (false, false) => self.at > 0 && !self.ids.is_empty(),
        }
    }

    /// The next (`forward`: "Skip", ↓, `j`) or the previous mail; `false` when it could not go there
    /// ([`FocusReply::can_step`]). The mail left stays on the pile with what was typed for it; the
    /// field shows what was typed for the next one, empty at first.
    pub fn step(&mut self, forward: bool) -> bool {
        if !self.can_step(forward) {
            return false;
        }
        self.leave();
        self.at = if forward { self.at + 1 } else { (self.at - 1).min(self.ids.len() - 1) };
        self.arrive();
        true
    }

    /// "Remove from Reply later": the mail shown leaves the round with what was typed for it, the
    /// next one shows; its id for the window to take it off the pile. `None` at the end and while
    /// sending.
    ///
    /// Returns the mail's id and what was typed for it, so its undo can put both back
    /// ([`FocusReply::put_back`], second review of #315).
    pub fn take_current(&mut self) -> Option<(String, String)> {
        if self.sending || self.current().is_none() {
            return None;
        }
        let text = self.reply.text();
        let id = self.ids.remove(self.at);
        self.left_pile();
        self.arrive();
        Some((id, text))
    }

    /// The undo of "Remove from Reply later": `id` back into the round at `place` (as far as the
    /// round still reaches), shown, with `text` in its field, back on "Reply later" since `since`
    /// ([`FocusReply::pile_of`], fourth review of #315). Nothing while sending.
    pub fn put_back(&mut self, id: String, place: usize, text: &str, since: Option<DateTime<Utc>>) {
        if self.sending || self.ids.contains(&id) {
            return;
        }
        self.leave();
        let place = place.min(self.ids.len());
        self.seen.insert(id.clone());
        self.piles.insert(id.clone(), (Some(MailPile::ReplyLater), since));
        self.total = self.total.map(|total| total + 1);
        self.ids.insert(place, id);
        self.at = place;
        self.reply = text_editor::Content::with_text(text);
        self.error = None;
    }

    /// The reply to `id` was sent: it leaves the round, and the mail after it shows with its own
    /// field. Only for the mail shown, the one whose reply is on its way (fourth review of #315: the
    /// round stays while it is, `App::close_focus`); any other id changes nothing.
    pub fn answered(&mut self, id: &str) {
        if self.current() != Some(id) {
            return;
        }
        self.sending = false;
        self.sending_since = None;
        self.send = None;
        self.ids.remove(self.at);
        self.kept.remove(id);
        self.left_pile();
        self.arrive();
    }

    /// The reply to `id` could not be sent, for `why`: it stays in the field, "Send" tries again.
    /// Only for the mail shown, as [`FocusReply::answered`].
    pub fn failed(&mut self, id: &str, why: String) {
        if self.current() != Some(id) {
            return;
        }
        self.sending = false;
        self.sending_since = None;
        self.send = None;
        self.error = Some(why);
    }

    /// The reply on its way had no answer in time (fifth review of #315): the round lets go – nothing
    /// on its way, the text stays in the field, a note says to look in Sent before sending again.
    /// Nothing while nothing is on its way.
    pub fn give_up(&mut self) {
        if !self.sending {
            return;
        }
        self.sending = false;
        self.sending_since = None;
        self.send = None;
        self.timed_out = true;
        self.error = None;
    }

    /// Whether "Send" goes: a mail shown, something typed, nothing on its way.
    pub fn can_send(&self) -> bool {
        !self.sending && self.current().is_some() && !self.reply.text().trim().is_empty()
    }

    /// Whether anything typed would be lost with the round: the field or a reply kept for another mail.
    pub fn has_text(&self) -> bool {
        !self.keep().texts.is_empty()
    }

    /// The round as the undo of "Reply discarded." keeps it: mail, place, every reply with text, what
    /// it had, its count and its piles. A round is never left while a reply is on its way (fourth
    /// review of #315), so nothing kept is on its way.
    pub fn keep(&self) -> FocusKept {
        let shown = self.current().map(|id| (id.to_owned(), self.reply.text()));
        let kept = self.kept.iter().map(|(id, reply)| (id.clone(), reply.text()));
        let mut texts: Vec<(String, String)> =
            shown.into_iter().chain(kept).filter(|(_, text)| !text.trim().is_empty()).collect();
        texts.sort();
        let ids = self.ids.clone();
        let mut seen: Vec<String> = self.seen.iter().cloned().collect();
        seen.sort();
        let mut piles: Vec<_> = self.piles.iter().map(|(id, (pile, since))| (id.clone(), *pile, *since)).collect();
        piles.sort_by(|a, b| a.0.cmp(&b.0));
        FocusKept { ids, at: self.at, texts, error: None, seen, total: self.total, piles }
    }

    /// The round `kept` back, at its place with its replies, its count, the piles it loaded and the
    /// error it came back for; nothing on its way. What lies on the pile beyond it shows up again at
    /// its end (`more`) – never mail it had before (third review of #315).
    pub fn restore(kept: FocusKept) -> FocusReply {
        let kept_texts = kept.texts.into_iter().map(|(id, text)| (id, text_editor::Content::with_text(&text)));
        let piles = kept.piles.into_iter().map(|(id, pile, since)| (id, (pile, since))).collect();
        let mut focus = FocusReply {
            at: kept.at,
            kept: kept_texts.collect(),
            more: true,
            total: kept.total,
            piles,
            ..FocusReply::new(kept.ids)
        };
        focus.seen.extend(kept.seen);
        focus.at = focus.at.min(focus.ids.len());
        focus.arrive();
        focus.error = kept.error;
        focus
    }

    /// Keeps what was typed for the mail shown before another one shows.
    fn leave(&mut self) {
        let reply = std::mem::take(&mut self.reply);
        if let Some(id) = self.current().map(str::to_owned)
            && !reply.text().trim().is_empty()
        {
            self.kept.insert(id, reply);
        }
    }

    /// The field of the mail now shown: what was typed for it, else empty; no error of another mail.
    fn arrive(&mut self) {
        let id = self.current().map(str::to_owned);
        self.reply = id.and_then(|id| self.kept.remove(&id)).unwrap_or_default();
        self.error = None;
        self.timed_out = false;
    }
}

/// What [`focus_page`] draws.
#[derive(Debug, Clone, Copy)]
pub struct FocusPage<'a> {
    /// The round.
    pub focus: &'a FocusReply,
    /// The mail shown, as the reader reads it (`App::reading`): its head while the whole of it loads,
    /// then its text or HTML. `None` while the window holds none yet.
    pub reading: Option<Reading<'a>>,
    /// A narrow window: the page's padding at the sides instead of the reading padding, which would
    /// leave the mail a strip there.
    pub narrow: bool,
}

/// "Focus & reply" (task E19.5) in place of list and reader, in every layout from narrow to wide
/// alike: in the reading column ([`size::reading_max`], at the left under the header as the reader's
/// mail stands)
///
/// 1. scrolling, the mail as the reader shows it: its head ([`head`]: subject, sender, invitation,
///    task), a line and its text or HTML ([`letter`]);
/// 2. fixed under a line from edge to edge, so it is at hand however long the mail is: the field
///    ("Write a reply…", the shortcut "send" of the compose window sends, Escape leaves the field),
///    why the last send failed, and the row Send (primary; "Sending…" while on its way) · Skip
///    (secondary) · Remove from Reply later (ghost; only its icon when the words do not fit) · at
///    the right the compose window for this reply ("Open in compose window": Cc, attachments,
///    format).
///
/// No action rail: the round does one thing. At the end of what is loaded the page shows loading
/// while the next page of the pile comes (or its error with "Retry", decision of #315); at the end
/// of the round it says "Nothing left
/// to reply to." with the way back ([`focus_done`]).
pub fn focus_page<'a>(page: FocusPage<'a>) -> Element<'a, Message> {
    let Some(id) = page.focus.current() else {
        // The end of what is loaded: the next page while the pile has more (decision of #315), its
        // error with "Retry", and only then the end of the round.
        return match &page.focus.load_error {
            Some(_) => widgets::error_state(tr("Couldn't load more messages."), Message::FocusLoad),
            None if page.focus.more => widgets::loading(),
            None => focus_done(),
        };
    };
    responsive(move |bounds| {
        let pad = focus_padding(page.narrow);
        let width = (bounds.width - pad.left - pad.right).max(0.0).min(size::reading_max());
        let line = || rule::horizontal(size::HAIRLINE).style(style::rule);
        let mut mail = column![].spacing(layout::section_gap()).width(width);
        if let Some(reading) = &page.reading {
            mail = mail.push(head(reading, None, width)).push(line());
        }
        mail = mail.push(focus_letter(page.reading, width));
        let mail = scrollable(container(mail).padding(Padding { bottom: layout::section_gap(), ..pad }))
            .id(FOCUS_MAIL)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill);
        let reply = container(focus_reply(page.focus, id, width, bounds.height)).padding(Padding {
            top: space::l(),
            bottom: space::l(),
            ..pad
        });
        column![mail, line(), reply].height(Fill).into()
    })
    .into()
}

/// The padding around the page of "Focus & reply": the page's above and below, at the sides the
/// reading padding as in the reader – the page's in a narrow window.
fn focus_padding(narrow: bool) -> Padding {
    let sides = if narrow { layout::page_padding().left } else { layout::reading_padding().left };
    Padding { left: sides, right: sides, ..layout::page_padding() }
}

/// The mail under its head in "Focus & reply", `width` wide: its text or HTML ([`letter`]); while
/// it loads, or after an error, a quiet line says so, as in the reader.
fn focus_letter<'a>(reading: Option<Reading<'a>>, width: f32) -> Element<'a, Message> {
    let Some(reading) = reading else { return note(tr("Loading the message…")) };
    match reading.loaded {
        None => note(tr("Loading the message…")),
        Some(Err(why)) => {
            let what = typo::text(typo::body(), tr("Couldn't load the message.")).style(theme::text_2);
            let why = typo::text(typo::meta(), why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
            column![what, why].spacing(space::xs()).into()
        }
        Some(Ok(_)) => letter(&reading).width(width).into(),
    }
}

/// The field, why the last send failed and the buttons of "Focus & reply" ([`focus_page`]) for the
/// mail `id`, `width` wide on a page `height` high: the field grows with the reply up to
/// [`FIELD_MAX_LINES`], never over [`FIELD_SHARE`] of the page. While sending the field takes no
/// typing, so what goes is what shows.
fn focus_reply<'a>(focus: &'a FocusReply, id: &'a str, width: f32, height: f32) -> Element<'a, Message> {
    let most = field_height(FIELD_MAX_LINES).min(height * FIELD_SHARE);
    let mut field = text_editor(&focus.reply)
        .id(FOCUS_FIELD)
        .placeholder(tr("Write a reply…"))
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
        .padding(style::INPUT_PADDING)
        .min_height(field_height(FIELD_LINES).min(most))
        .max_height(most)
        .wrapping(Wrapping::WordOrGlyph)
        .style(style::text_editor)
        .key_binding(focus_binding);
    if !focus.sending {
        field = field.on_action(Message::FocusEdit);
    }
    let mut block = column![field].spacing(space::m()).width(width);
    if let Some(why) = &focus.error {
        block = block.push(send_failure(Some(tr("Couldn't send the message.")), why));
    } else if focus.timed_out {
        block = block.push(send_failure(None, tr("No answer yet – check Sent before sending again.")));
    }
    block.push(focus_buttons(focus, id, width)).into()
}

/// The height of the reply field for `lines` lines of `typo::body()` inside its padding.
fn field_height(lines: f32) -> f32 {
    let body = typo::body();
    lines * body.line_height.to_absolute(body.size).0 + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// The keys of the reply field: the shortcut "send" of the compose window sends
/// (`compose::is_send`: Ctrl + Enter, on macOS also ⌘ + Enter and ⇧⌘D) instead of starting a new
/// line. Escape leaves the field – captured, as a sequence is (Iced passes a lone `Unfocus` on), so
/// the window's Escape does not also end the round with the text typed: a second Escape does, and
/// `j`/`k` walk the round meanwhile. Everything else as in any field.
fn focus_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return text_editor::Binding::from_key_press(press);
    }
    if compose::is_send(System::current(), &press.key, press.physical_key, press.modifiers) {
        return Some(text_editor::Binding::Custom(Message::FocusSend));
    }
    if matches!(press.key.as_ref(), keyboard::Key::Named(keyboard::key::Named::Escape)) {
        return Some(text_editor::Binding::Sequence(vec![text_editor::Binding::Unfocus]));
    }
    text_editor::Binding::from_key_press(press)
}

/// Why the reply could not be sent, under the field: the alert in the error colour, what happened
/// (`what`, when there is a headline) and the core's words or the note, as the window's error line
/// says it.
fn send_failure<'a>(what: Option<&'a str>, why: &'a str) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    let what = what.map(|what| typo::text(typo::meta(), what).style(theme::text_2));
    let why = typo::text(typo::meta(), why).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
    row![alert, column(what.map(Element::from)).push(why).spacing(space::xs()).width(Fill)]
        .spacing(space::s())
        .align_y(Alignment::Start)
        .into()
}

/// The row under the reply field: Send (primary, the shortcut in its tooltip; off while nothing is
/// typed, "Sending…" while on its way), Skip (secondary, `J`), Remove from Reply later (ghost, `W`)
/// and at the right "Open in compose window" (`SquarePen`) for the mail `id`. In a row narrower than
/// all words – the wider of "Send" and "Sending…", four gaps – Remove shows its icon with the words
/// as tooltip. Nothing takes a click while sending.
///
/// The compose window starts from the mail alone (`--reply <id>`, no text of its own): the reply typed
/// here stays here, and the tooltip says so, rather than putting mail text on a command line.
fn focus_buttons<'a>(focus: &'a FocusReply, id: &'a str, width: f32) -> Element<'a, Message> {
    let idle = !focus.sending;
    let send = if focus.sending {
        widgets::busy_button(tr("Sending…"), style::button::primary)
    } else {
        let send = focus.can_send().then_some(Message::FocusSend);
        let button = widgets::text_button(Some(Icon::Send), tr("Send"), style::button::primary, send);
        key_tip(button, tr("Send"), compose::send_keys())
    };
    let skip = widgets::text_button(None, tr("Skip"), style::button::secondary, idle.then_some(Message::FocusSkip));
    let skip = key_tip(skip, tr("Skip"), "J");
    let words = tr("Remove from Reply later");
    let remove = idle.then_some(Message::FocusRemove);
    let send_width = widgets::text_button_width(true, tr("Send")).max(widgets::text_button_width(true, tr("Sending…")));
    let all = send_width
        + widgets::text_button_width(false, tr("Skip"))
        + widgets::text_button_width(true, words)
        + size::control_sm()
        + 4.0 * space::s();
    let remove = if all <= width {
        widgets::text_button(Some(Icon::X), words, style::button::ghost, remove)
    } else {
        widgets::compact_button(Icon::X, words, style::button::ghost, remove)
    };
    let remove = key_tip(remove, words, "W");
    let compose = idle.then(|| Message::Mail(MailAction::Reply, id.to_owned()));
    let compose = widgets::icon_button(Icon::SquarePen, tr("Open in compose window (your text stays here)"), compose);
    row![send, skip, remove, iced::widget::space::horizontal(), compose]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// `content` with a tooltip below it: `label` and its key as [`widgets::kbd`], on the card the
/// suite's tooltips have (menu surface, after a short delay) – as the buttons of a row's action bar
/// name their key. `widgets::with_tooltip` takes words only.
fn key_tip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    key: impl iced::widget::text::IntoFragment<'a>,
) -> Element<'a, Message> {
    let words =
        row![typo::text(typo::caption(), label).style(theme::text_2).wrapping(Wrapping::None), widgets::kbd(key)]
            .spacing(space::s())
            .align_y(Alignment::Center);
    let card = container(words)
        .padding([style::KBD_PADDING.top + style::KBD_PADDING.bottom, space::s()])
        .style(style::container::menu);
    iced::widget::tooltip(content, card, iced::widget::tooltip::Position::Bottom)
        .gap(space::xs())
        .delay(std::time::Duration::from_millis(TIP_DELAY_MS))
        .into()
}

/// The delay of the suite's tooltips (`widgets::with_tooltip`), for [`key_tip`].
const TIP_DELAY_MS: u64 = 500;

/// The end of a round of "Focus & reply", built as an empty state (`list::empty_state`): the check in
/// `text_3`, "Nothing left to reply to." and – `space::l()` under it – the way back to the pile
/// ("Back to Reply later", as Escape and the header's "‹ Reply later").
fn focus_done<'a>() -> Element<'a, Message> {
    let mark =
        icon::icon(Icon::CircleCheck, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let back = widgets::text_button(
        Some(Icon::ChevronLeft),
        tr("Back to Reply later"),
        style::button::secondary,
        Some(Message::FocusClose),
    );
    let title = typo::text(typo::body_strong(), tr("Nothing left to reply to.")).align_x(Alignment::Center);
    let words = column![title, back].spacing(space::l()).align_x(Alignment::Center);
    let content = column![mark, words].spacing(space::m()).align_x(Alignment::Center).max_width(size::form_max() / 2.0);
    center(content).padding(space::xl()).into()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use torro_inbox::{SourceKind, demo};

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time")
    }

    /// The click of a reading in the tests: open, the default.
    static OPEN: AttachmentClick = AttachmentClick::Open;

    /// Task E19.5 and its review: a round of "Focus & reply" – skipping keeps a mail in the round and
    /// what was typed for it, taking it off or answering it drops it and shows the next; an answer to a
    /// mail before the one shown keeps the one shown; nothing moves while sending; ↑ comes back from
    /// the end; `keep` and `restore` carry the round with every reply.
    #[test]
    fn a_round_of_focus_and_reply() {
        let ids = |focus: &FocusReply| focus.ids.join(" ");
        let mut focus = FocusReply::new(["a", "b", "c", "d"].map(String::from).to_vec());
        assert_eq!((focus.current(), focus.position()), (Some("a"), Some((1, 4))));
        assert!(!focus.can_send() && !focus.has_text(), "nothing typed");
        focus.reply = text_editor::Content::with_text("Thanks!");
        assert!(focus.can_send() && focus.has_text());
        assert!(focus.step(true));
        assert_eq!((focus.current(), focus.reply.text().trim()), (Some("b"), ""), "b has its own field");
        assert!(focus.has_text(), "a's reply is kept");
        assert!(focus.step(false));
        assert_eq!(focus.reply.text().trim(), "Thanks!", "back at a, its reply is there");
        assert!(focus.step(true));
        focus.reply = text_editor::Content::with_text("Maybe");
        assert_eq!(focus.take_current(), Some(("b".to_owned(), "Maybe".to_owned())), "with its text");
        assert_eq!((focus.current(), ids(&focus)), (Some("c"), "a c d".to_owned()));
        // Its undo puts it back at its place with its text (second review of #315).
        focus.put_back("b".into(), 1, "Maybe", None);
        assert_eq!(
            (focus.current(), focus.reply.text().trim(), ids(&focus)),
            (Some("b"), "Maybe", "a b c d".to_owned())
        );
        // Back on "Reply later" for the round (fourth review of #315).
        assert_eq!(focus.pile_of("b"), Some((Some(MailPile::ReplyLater), None)));
        assert!(focus.take_current().is_some());
        // An answer or an error counts only for the mail shown (fourth review of #315).
        focus.answered("a");
        assert_eq!((focus.current(), ids(&focus)), (Some("c"), "a c d".to_owned()), "a is not shown");
        focus.reply = text_editor::Content::with_text("On it");
        let kept = focus.keep();
        assert_eq!(
            kept,
            FocusKept {
                ids: vec!["a".into(), "c".into(), "d".into()],
                at: 1,
                texts: vec![("a".into(), "Thanks!".into()), ("c".into(), "On it".into())],
                error: None,
                seen: ["a", "b", "c", "d"].map(String::from).to_vec(),
                total: None,
                piles: vec![("b".into(), Some(MailPile::ReplyLater), None)],
            }
        );
        focus.sending = true;
        assert!(!focus.step(true) && !focus.step(false) && focus.take_current().is_none(), "nothing moves");
        focus.failed("a", "elsewhere".into());
        assert!(focus.sending && focus.error.is_none(), "another mail's error changes nothing");
        focus.answered("c");
        assert_eq!((focus.current(), focus.sending, ids(&focus)), (Some("d"), false, "a d".to_owned()));
        focus.failed("d", "offline".into());
        assert_eq!(focus.error.as_deref(), Some("offline"));
        assert!(focus.step(true));
        assert_eq!((focus.current(), focus.position(), focus.error.as_deref()), (None, None, None), "the end");
        assert!(!focus.can_step(true) && focus.step(false));
        assert_eq!(focus.current(), Some("d"), "back from the end");
        let back = FocusReply::restore(kept);
        assert_eq!((back.current(), back.reply.text().trim(), back.sending), (Some("c"), "On it", false));
    }

    fn folders() -> Vec<Folder> {
        DemoSource::new(now()).mail_folders().expect("folders")
    }

    /// Issue #291 (task L.35): the mail stands in the reading column – text and HTML alike at most
    /// `size::reading_max()` wide inside the reading padding, at the left –, the head over the whole
    /// width inside the padding's sides; in every built-in design.
    #[test]
    fn the_mail_stands_in_the_reading_column() {
        let pad = layout::reading_padding();
        let sides = pad.left + pad.right;
        assert_eq!((size::reading_max(), sides), (600.0, 128.0), "the standard's column and margins");
        for width in [3000.0, 1400.0, 900.0, 728.0] {
            assert_eq!(reader_widths(width), ReaderWidths { head: width - sides, mail: 600.0 }, "at {width}");
        }
        // Narrower than the column with its margins: the room inside them.
        assert_eq!(reader_widths(500.0), ReaderWidths { head: 372.0, mail: 372.0 });
        assert_eq!(reader_widths(10.0), ReaderWidths { head: 0.0, mail: 0.0 }, "never below nothing");
        // Compact and Console from their files: Console's column is as wide as the reader allows.
        for (id, column, margins) in [("compact", 640.0, 80.0), ("console", 1200.0, 36.0)] {
            let design = torro_core::design::load(std::path::Path::new(""), id).expect("built in");
            let sides = 2.0 * design.layout.reading_padding.horizontal;
            assert_eq!((design.size.reading_max, sides), (column, margins), "{id}");
            assert_eq!(reading_width_in(2000.0, sides, column), column, "{id}: the column at most");
            assert_eq!(reading_width_in(900.0, sides, column), (900.0 - sides).min(column), "{id}: narrower");
        }
    }

    /// Issue #291 (task L.35): the reader's head has the sender's avatar only where the mail rows
    /// have one (`[list] avatar`), so a design without avatars has none in the head either.
    #[test]
    fn the_readers_avatar_follows_the_rows() {
        assert_eq!(head_avatar_in(layout::list()), size::pill(), "the standard shows it");
        let console = torro_core::design::load(std::path::Path::new(""), "console").expect("built in");
        assert_eq!(head_avatar_in(console.list), 0.0, "Console: none");
    }

    #[test]
    fn folders_group_by_account_with_the_roles_first() {
        let folders = folders();
        let groups = by_account(&folders);
        assert_eq!(groups.iter().map(|g| g.account).collect::<Vec<_>>(), demo::MAIL_ACCOUNTS);
        let names: Vec<String> = groups[0].folders.iter().map(|f| folder_name(f)).collect();
        let roles = [tr("Inbox"), tr("Drafts"), tr("Sent"), tr("Archive"), tr("Junk"), tr("Trash"), "Projects"];
        assert_eq!(names, roles);
        assert_eq!(first_inbox(&folders).map(|f| f.id.as_str()), Some("f:work-inbox"));
        assert_eq!(address("mail:anna@example.org"), "anna@example.org");
    }

    #[test]
    fn folders_have_icons_and_quiet_counts() {
        assert_eq!(folder_icon(Some(Role::Inbox)), Icon::Inbox);
        assert_eq!(folder_icon(Some(Role::Trash)), Icon::Trash2);
        assert_eq!(folder_icon(None), Icon::Folder);
        assert!(counts_unread(Some(Role::Inbox)) && counts_unread(None) && counts_unread(Some(Role::Archive)));
        assert!(!counts_unread(Some(Role::Trash)) && !counts_unread(Some(Role::Junk)));
    }

    #[test]
    fn the_compose_window_gets_the_message_and_the_kind() {
        assert_eq!(compose_args(Compose::Reply, "m:1", false), ["--compose", "--reply", "m:1"]);
        assert_eq!(compose_args(Compose::ReplyAll, "m:1", false), ["--compose", "--reply-all", "m:1"]);
        assert_eq!(compose_args(Compose::Forward, "m:1", true), ["--compose", "--forward", "m:1", "--demo"]);
        let command = compose_command(Compose::Reply, "m:7", false);
        let program = Path::new(command.get_program());
        assert_eq!(program.file_name().and_then(|name| name.to_str()), Some(INBOX.binary), "this app again");
        let args: Vec<&std::ffi::OsStr> = command.get_args().collect();
        assert_eq!(args, ["--compose", "--reply", "m:7"]);
        assert_eq!(Compose::of(MailAction::Forward), Some(Compose::Forward));
        assert_eq!(Compose::of(MailAction::Archive), None);
    }

    #[test]
    fn actions_become_edits() {
        let ids = vec!["m:1".to_owned()];
        let archive = MailEdit::Move { ids: ids.clone(), to: MoveTarget::Role(MoveRole::Archive) };
        assert_eq!(edit_of(MailAction::Archive, "m:1"), Some(archive));
        assert_eq!(edit_of(MailAction::Delete, "m:1"), Some(MailEdit::Delete { ids: ids.clone() }));
        let flag = MailEdit::Flags { ids: ids.clone(), add: vec![Flag::Flagged], remove: Vec::new() };
        assert_eq!(edit_of(MailAction::Flag(true), "m:1"), Some(flag.clone()));
        let unread = MailEdit::Flags { ids, add: Vec::new(), remove: vec![Flag::Seen] };
        assert_eq!(edit_of(MailAction::Read(false), "m:1"), Some(unread.clone()));
        assert_eq!(edit_of(MailAction::Reply, "m:1"), None);
        assert_eq!(flags_after(&[Flag::Seen], &flag), [Flag::Seen, Flag::Flagged]);
        assert_eq!(flags_after(&[Flag::Seen, Flag::Flagged], &unread), [Flag::Flagged]);
    }

    /// Issue #119: the text is selectable, so its web addresses stand under it as "Links" – the same
    /// cut as the calendar's (`torro_gui::links`); nothing but `http` and `https` opens.
    #[test]
    fn web_addresses_are_links_and_nothing_else_is() {
        let text = "See https://intranet.example.org/budget/q4. Or (http://a.example/x), mailto:me@x.example, \
                    xhttps://no.example and https:// alone.";
        let links = selectable::web_links(text);
        assert_eq!(links, ["https://intranet.example.org/budget/q4", "http://a.example/x"]);
        assert!(text_links(&Selectable::new(text)).is_some());
        assert!(text_links(&Selectable::new("no address")).is_none(), "no section without links");
    }

    /// The demo's mail `id` loaded, its text and – for a mail with HTML – its HTML shown as `format`,
    /// handed to `check` as the reader would read it (`loaded` false: still loading).
    fn reading_of<R>(id: &str, format: MailFormat, loaded: bool, check: impl FnOnce(&Reading<'_>) -> R) -> R {
        let demo = Arc::new(DemoSource::new(now()));
        let mail = Ok(DemoSource::get_mail(&demo, id).expect("a demo mail"));
        let text = Selectable::new(&mail.as_ref().map(text_of).unwrap_or_default());
        let html = mail.as_ref().ok().and_then(|mail| HtmlMail::new(mail, &demo, true));
        let summary = mail.as_ref().ok().map(|mail| mail.summary.clone());
        let reading = Reading {
            categories: &[],
            id,
            summary: summary.as_ref(),
            loaded: loaded.then_some(&mail),
            now: now(),
            text: &text,
            attention: Color::BLACK,
            in_trash: false,
            deciding: false,
            dated: true,
            read: true,
            account: Some(0),
            task: None,
            answering: false,
            invite: InviteSide::default(),
            html: html.as_ref().map(|html| HtmlReading {
                content: &html.content,
                format,
                scheme: Scheme::Light,
                scale: 1.0,
            }),
            saving: None,
            click: &OPEN,
            file_menu: None,
        };
        check(&reading)
    }

    /// A mail entry of the demo, new and for the user, for the rail's groups and the panels.
    fn mail_entry() -> Item {
        let all = iced::futures::executor::block_on(torro_inbox::InboxSource::list_items(
            &DemoSource::new(now()),
            Default::default(),
        ))
        .expect("the demo answers");
        let mut item = all.into_iter().find(|item| item.source.kind == SourceKind::Mail).expect("a mail entry");
        item.state = State::New;
        item.lane = Lane::ForMe;
        item
    }

    /// The groups of the rail as words: each group's title with its items' words.
    fn rail_words(groups: &[RailGroup<'_, Message>]) -> Vec<(String, Vec<String>)> {
        let words = |group: &RailGroup<'_, Message>| group.items.iter().map(|item| item.label.to_string()).collect();
        groups.iter().map(|group| (group.label.to_string(), words(group))).collect()
    }

    /// The group titled `label` (in the user's language).
    fn group<'g, 'a>(groups: &'g [RailGroup<'a, Message>], label: &str) -> &'g RailGroup<'a, Message> {
        groups.iter().find(|group| group.label == label).expect("the group")
    }

    /// What the item `word` of the group `label` sends, `None` when it is off.
    fn sends(groups: &[RailGroup<'_, Message>], label: &str, word: &str) -> Option<Message> {
        let item = group(groups, label).items.iter().find(|item| item.label == word).expect("the item");
        item.press()
    }

    /// The flags' words of the mail in `reading`: Flag or Unflag, Mark read or Mark unread.
    fn mark_words(reading: &Reading<'_>) -> Vec<String> {
        let flags = &reading.summary().expect("headers").flags;
        let flag = if flags.contains(&Flag::Flagged) { tr("Unflag") } else { tr("Flag") };
        let read = if flags.contains(&Flag::Seen) { tr("Mark unread") } else { tr("Mark read") };
        vec![flag.to_owned(), read.to_owned()]
    }

    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    /// Task E14.3: for a mail entry the rail holds Reply, File, Torro, Mark, Mute and More in this
    /// order, Mute and More at the foot, Reply in the accent; every item sends what its button or key
    /// sent before and shows its key; a click on a group's icon sends its first item. Task E18.5: File
    /// has no Done – Archive (`e`) stands in its place. Task E19.4: Reply later (`w`), Set aside (`b`)
    /// and Follow up… (`s`) after Archive, in the place of Later….
    #[test]
    fn an_entrys_rail_holds_every_group_with_the_messages_of_before() {
        let item = mail_entry();
        let folders = folders();
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            let anchors = detail::Anchors::default().with_rail();
            let groups =
                action_groups(reading, Some(ReaderEntry { item: &item, hide: true }), &folders, true, false, &anchors);
            let move_to = lane_word(Lane::Info);
            assert_eq!(
                rail_words(&groups),
                [
                    (tr("Reply").to_owned(), strings(&[tr("Reply"), tr("Reply all"), tr("Forward")])),
                    (
                        tr("File").to_owned(),
                        vec![
                            trc("action", "Archive").to_owned(),
                            tr("Reply later").to_owned(),
                            tr("Set aside").to_owned(),
                            tr("Follow up…").to_owned(),
                            move_to.clone(),
                            tr("Delete").to_owned(),
                            // The budget's sheet (task M12.6).
                            trf("Save attachments ({n})", &[("n", &1)]),
                        ]
                    ),
                    (tr("Torro").to_owned(), strings(&[tr("Task"), tr("Event"), tr("Category…")])),
                    (tr("Mark").to_owned(), mark_words(reading)),
                    (tr("Mute").to_owned(), strings(&[tr("Mute…")])),
                    (tr("More").to_owned(), strings(&[tr("Copy text"), tr("Close")])),
                ]
            );
            let feet: Vec<bool> = groups.iter().map(|group| group.foot).collect();
            assert_eq!(feet, [false, false, false, false, true, true], "Mute and More at the foot");
            assert!(groups[0].accent && groups[0].items[0].lead, "Reply the main group, its row strong");
            assert!(groups[1..].iter().all(|group| !group.accent));
            let mail = |sent: Option<Message>, action: MailAction| matches!(sent, Some(Message::Mail(sent, id)) if sent == action && id == "m:w1");
            let (reply, file, torro) = (tr("Reply"), tr("File"), tr("Torro"));
            assert!(mail(sends(&groups, reply, tr("Reply")), MailAction::Reply));
            assert!(mail(sends(&groups, reply, tr("Reply all")), MailAction::ReplyAll));
            assert!(mail(sends(&groups, reply, tr("Forward")), MailAction::Forward));
            let entry = |sent: Option<Message>| match sent {
                Some(Message::AskSnooze(id) | Message::AskLane(id)) => id == item.id,
                Some(Message::AskTask(id) | Message::AskEvent(id) | Message::AskCategory(id)) => id == item.id,
                Some(Message::AskHide(id)) => id == item.id,
                _ => false,
            };
            assert!(matches!(sends(&groups, file, tr("Follow up…")), Some(Message::AskSnooze(_))));
            assert!(entry(sends(&groups, file, tr("Follow up…"))), "the entry's follow-up");
            let pile = |sent: Option<Message>, wanted| matches!(sent, Some(Message::Pile(id, pile)) if id == "m:w1" && pile == wanted);
            assert!(pile(sends(&groups, file, tr("Reply later")), Some(MailPile::ReplyLater)));
            assert!(pile(sends(&groups, file, tr("Set aside")), Some(MailPile::SetAside)));
            assert!(
                matches!(sends(&groups, file, &move_to), Some(Message::AskLane(_)))
                    && entry(sends(&groups, file, &move_to))
            );
            assert!(mail(sends(&groups, file, trc("action", "Archive")), MailAction::Archive));
            assert!(mail(sends(&groups, file, tr("Delete")), MailAction::Delete));
            assert!(matches!(sends(&groups, torro, tr("Task")), Some(Message::AskTask(_))));
            assert!(matches!(sends(&groups, torro, tr("Event")), Some(Message::AskEvent(_))));
            assert!(matches!(sends(&groups, torro, tr("Category…")), Some(Message::AskCategory(_))));
            for word in [tr("Task"), tr("Event"), tr("Category…")] {
                assert!(entry(sends(&groups, torro, word)), "{word}");
            }
            assert!(matches!(sends(&groups, tr("Mute"), tr("Mute…")), Some(Message::AskHide(id)) if id == item.id));
            assert!(matches!(sends(&groups, tr("More"), tr("Copy text")), Some(Message::CopyText)));
            assert!(matches!(sends(&groups, tr("More"), tr("Close")), Some(Message::CloseDetail)));
            let marks = &group(&groups, tr("Mark")).items;
            let flagged = reading.summary().expect("headers").flags.contains(&Flag::Flagged);
            assert!(mail(marks[0].press(), MailAction::Flag(!flagged)));
            assert!(matches!(marks[1].press(), Some(Message::Mail(MailAction::Read(_), id)) if id == "m:w1"));
            // `e` archives (task E18.5); Reply all has no single-key shortcut here.
            let keys: Vec<Vec<&str>> = groups.iter().map(|g| g.items.iter().map(|i| i.key).collect()).collect();
            assert_eq!(
                keys,
                [
                    vec!["R", "", "F"],
                    vec!["E", "W", "B", "S", "L", "#", ""],
                    vec!["T", "C", ""],
                    vec!["", ""],
                    vec!["M"],
                    vec!["", "Esc"]
                ]
            );
            // A click on a group's icon does its first item; its icon is that item's.
            for group in &groups {
                assert_eq!(format!("{:?}", group.first_press()), format!("{:?}", group.items[0].press()));
                assert!(group.anchor.is_some(), "{}: the open group hangs at its icon", group.label);
            }
            // Fixed icons, whatever the first item (finding 8).
            let icons: Vec<Icon> = groups.iter().map(|group| group.icon).collect();
            assert_eq!(icons, [Icon::Reply, Icon::Check, Icon::ListTodo, Icon::Flag, Icon::BellOff, Icon::Ellipsis]);
            assert!(group(&groups, tr("File")).items.iter().filter(|i| i.danger).all(|i| i.label == tr("Delete")));
            let flagged = reading.summary().expect("headers").flags.contains(&Flag::Flagged);
            assert_eq!(group(&groups, tr("Mark")).tint.is_some(), flagged, "a flagged mail shows at Mark");
            // The items that open a menu or dialog hang it at their row; the others need no anchor.
            let anchored: Vec<String> = groups
                .iter()
                .flat_map(|group| {
                    group.items.iter().filter(|item| item.anchor.is_some()).map(|item| item.label.to_string())
                })
                .collect();
            assert_eq!(
                anchored,
                [
                    tr("Follow up…").to_owned(),
                    move_to,
                    trf("Save attachments ({n})", &[("n", &1)]),
                    tr("Task").to_owned(),
                    tr("Event").to_owned(),
                    tr("Category…").to_owned(),
                    tr("Mute…").to_owned()
                ]
            );
            // Expanded and collapsed, with and without its button, a group open or none.
            for mode in [widgets::RailMode::Collapsed, widgets::RailMode::Expanded] {
                for open in [None, Some(1), Some(5)] {
                    let _ =
                        widgets::action_rail(groups.clone(), mode, open, Message::Rail, Some(Message::ToggleActions));
                }
            }
        });
    }

    /// Task E14.3: a done entry is not put off – and, mail being archived (task E18.5), neither done nor
    /// opened again from the rail; "Re-categorize" only with a decision model; an entry that cannot be
    /// muted has no group "Mute". Task E19.4: its mail is followed up by the mail's own id.
    #[test]
    fn a_done_entry_reopens_and_the_model_adds_recategorize() {
        let item = Item { state: State::Done { at: now() }, ..mail_entry() };
        let folders = folders();
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            let anchors = detail::Anchors::default();
            let entry = Some(ReaderEntry { item: &item, hide: false });
            let groups = action_groups(reading, entry, &folders, false, false, &anchors);
            let file = &group(&groups, tr("File")).items;
            let words: Vec<&str> = file.iter().map(|i| i.label.as_ref()).collect();
            let save = trf("Save attachments ({n})", &[("n", &1)]);
            let lane = lane_word(Lane::Info);
            assert_eq!(
                words,
                [
                    trc("action", "Archive"),
                    tr("Reply later"),
                    tr("Set aside"),
                    tr("Follow up…"),
                    lane.as_str(),
                    tr("Delete"),
                    save.as_str()
                ]
            );
            assert!(file.iter().all(|i| i.label != tr("Reopen") && i.label != tr("Done")), "no Done, no Reopen");
            let follow = sends(&groups, tr("File"), tr("Follow up…"));
            assert!(matches!(follow, Some(Message::AskSnooze(id)) if id == "m:w1"), "the mail, not the done entry");
            assert!(groups.iter().all(|g| g.label != tr("Mute")), "nothing to mute: no group");
            let more = &group(&groups, tr("More")).items;
            assert!(more.iter().all(|i| i.label != tr("Close")), "the panel has its ✕");
            assert!(group(&groups, tr("Torro")).items.iter().all(|i| i.label != tr("Re-categorize")), "no model");
            let deciding = Reading { deciding: true, ..*reading };
            let groups = action_groups(&deciding, entry, &folders, false, false, &anchors);
            let torro = &group(&groups, tr("Torro")).items;
            let last = torro.last().expect("an item");
            assert_eq!(last.label, tr("Re-categorize"));
            assert!(last.accent && matches!(last.press(), Some(Message::Reclassify(id)) if id == item.id));
        });
    }

    /// Task E14.3: a folder's message has no Done, "Move to …", Event or Category – Task makes a task
    /// of the message, the bell mutes its own traits, `e` archives it; in the trash it has no Delete, in
    /// an account without an archive folder no Archive. Task E19.4: it goes on the piles and is followed
    /// up by its own id.
    #[test]
    fn a_folders_message_files_as_its_account_allows() {
        let folders = folders();
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            let groups = action_groups(reading, None, &folders, true, false, &detail::Anchors::default());
            let titles: Vec<String> = rail_words(&groups).into_iter().map(|(title, _)| title).collect();
            assert_eq!(titles, strings(&[tr("Reply"), tr("File"), tr("Torro"), tr("Mark"), tr("Mute"), tr("More")]));
            let file = &group(&groups, tr("File")).items;
            let save = trf("Save attachments ({n})", &[("n", &1)]);
            assert_eq!(
                file.iter().map(|i| i.label.to_string()).collect::<Vec<_>>(),
                strings(&[
                    trc("action", "Archive"),
                    tr("Reply later"),
                    tr("Set aside"),
                    tr("Follow up…"),
                    tr("Delete"),
                    &save
                ])
            );
            let keys: Vec<&str> = file.iter().map(|i| i.key).collect();
            assert_eq!(keys, ["E", "W", "B", "S", "#", ""], "`e` archives in a folder (task E18.5)");
            assert!(matches!(file[3].press(), Some(Message::AskSnooze(id)) if id == "m:w1"), "by the mail's id");
            assert!(matches!(file[5].press(), Some(Message::AskSaveAttachments(id)) if id == "m:w1"));
            assert_eq!(group(&groups, tr("File")).icon, Icon::Check, "the group's fixed icon");
            assert!(group(&groups, tr("File")).first_press().is_some(), "a click archives");
            let torro = &group(&groups, tr("Torro")).items;
            assert_eq!(torro.len(), 1);
            assert!(matches!(torro[0].press(), Some(Message::AskMailTask(id)) if id == "m:w1"));
            let mute = sends(&groups, tr("Mute"), tr("Mute…"));
            assert!(matches!(mute, Some(Message::AskMuteMail(id)) if id == "m:w1"), "the message's own traits");
            // An account without an archive folder: no Archive.
            let no_archive: Vec<Folder> = folders.iter().filter(|f| f.role != Some(Role::Archive)).cloned().collect();
            let groups = action_groups(reading, None, &no_archive, true, false, &detail::Anchors::default());
            let file: Vec<String> = group(&groups, tr("File")).items.iter().map(|i| i.label.to_string()).collect();
            assert_eq!(file, strings(&[tr("Reply later"), tr("Set aside"), tr("Follow up…"), tr("Delete"), &save]));
            // Review of #315: the click opens the card rather than putting the mail on a pile.
            assert!(group(&groups, tr("File")).first_press().is_none(), "no pile and never Delete by a click");
        });
        // Without the archive and the piles' items the click never deletes (finding 8).
        reading_of("m:w1", MailFormat::Html, false, |reading| {
            let no_archive: Vec<Folder> = folders.iter().filter(|f| f.role != Some(Role::Archive)).cloned().collect();
            let loading = Reading { summary: None, ..*reading };
            let item = mail_entry();
            let entry = Some(ReaderEntry { item: &item, hide: true });
            let groups = action_groups(&loading, entry, &no_archive, true, false, &detail::Anchors::default());
            let file = group(&groups, tr("File"));
            // Review of #315: in their place but off, without keys, until the headers say the pile.
            let piles: Vec<_> =
                file.items.iter().filter(|i| [tr("Reply later"), tr("Set aside")].contains(&&*i.label)).collect();
            assert_eq!(piles.len(), 2, "the piles keep their place");
            assert!(piles.iter().all(|i| i.press().is_none() && i.key.is_empty()), "off while the headers load");
            // Second review of #315: nor does the click open "Follow up" – it opens the card.
            assert!(file.items.iter().any(|i| i.label == tr("Follow up…") && i.press().is_some()));
            assert!(file.first_press().is_none(), "the click opens the card");
        });
        // The trash's message (`m:w12`) is not deleted again.
        reading_of("m:w12", MailFormat::Html, true, |reading| {
            let groups = action_groups(reading, None, &folders, true, false, &detail::Anchors::default());
            let file: Vec<String> = group(&groups, tr("File")).items.iter().map(|i| i.label.to_string()).collect();
            // No follow-up in the trash (decision of #315), and no piles (review of PR #327).
            assert_eq!(file, strings(&[trc("action", "Archive")]));
        });
    }

    /// Decision of #315: no "Follow up…" for mail in the archive, the trash or spam – archived counts as
    /// done (decision 6 of E19), a follow-up there would never come back; in the inbox and in Sent it
    /// stands. The folder of the mail shown decides.
    #[test]
    fn no_follow_up_on_archived_deleted_or_spam_mail() {
        let folders = folders();
        for (id, offered) in [("m:w9", false), ("m:w12", false), ("m:w11", false), ("m:w1", true), ("m:w7", true)] {
            reading_of(id, MailFormat::Html, true, |reading| {
                let groups = action_groups(reading, None, &folders, true, false, &detail::Anchors::default());
                let file = group(&groups, tr("File"));
                let follow = file.items.iter().any(|item| item.label == tr("Follow up…"));
                assert_eq!(follow, offered, "{id}");
                assert_eq!(follows_up(&folders, &reading.summary().expect("headers").folder), offered, "{id}");
            });
        }
    }

    /// Review of PR #327: no "Reply later" or "Set aside" for mail in the trash or spam; in the archive,
    /// the inbox and Sent they stand ("Set aside" for archived mail is the main case). A mail in the
    /// trash that lies on a pile still offers "Remove from …".
    #[test]
    fn no_piles_for_deleted_or_spam_mail() {
        let folders = folders();
        let piles = |groups: &[RailGroup<'_, Message>]| -> Vec<String> {
            group(groups, tr("File"))
                .items
                .iter()
                .map(|item| item.label.to_string())
                .filter(|label| {
                    label.contains(tr("Reply later"))
                        || label.contains(trc("pile", "Set aside"))
                        || label == tr("Set aside")
                })
                .collect()
        };
        for (id, offered) in [("m:w12", false), ("m:w11", false), ("m:w9", true), ("m:w1", true), ("m:w7", true)] {
            reading_of(id, MailFormat::Html, true, |reading| {
                let groups = action_groups(reading, None, &folders, true, false, &detail::Anchors::default());
                let expected = if offered { strings(&[tr("Reply later"), tr("Set aside")]) } else { Vec::new() };
                assert_eq!(piles(&groups), expected, "{id}");
                assert_eq!(takes_piles(&folders, &reading.summary().expect("headers").folder), offered, "{id}");
            });
        }
        // In the trash, but on "Set aside": only its "Remove from Set aside".
        reading_of("m:w12", MailFormat::Html, true, |reading| {
            let Some(Ok(mail)) = reading.loaded else { panic!("loaded") };
            let mut mail = mail.clone();
            mail.summary.pile = Some(MailPile::SetAside);
            let loaded = Ok(mail.clone());
            let piled = Reading { summary: Some(&mail.summary), loaded: Some(&loaded), ..*reading };
            let groups = action_groups(&piled, None, &folders, true, false, &detail::Anchors::default());
            assert_eq!(piles(&groups), strings(&[tr("Remove from Set aside")]));
            assert!(matches!(sends(&groups, tr("File"), tr("Remove from Set aside")), Some(Message::Pile(_, None))));
        });
    }

    /// Task E19.4 (decision 4 of E19): a mail on a pile says so in File – "Remove from Reply later"
    /// (the demo's `m:p7`) or "Remove from Set aside" (`m:w5`) with the same key takes it off; the
    /// other pile's item moves it there.
    #[test]
    fn a_mail_on_a_pile_is_taken_off_with_its_key() {
        let folders = folders();
        for (id, on, off, other, moved) in [
            ("m:p7", tr("Remove from Reply later"), "W", tr("Set aside"), MailPile::SetAside),
            ("m:w5", tr("Remove from Set aside"), "B", tr("Reply later"), MailPile::ReplyLater),
        ] {
            reading_of(id, MailFormat::Text, true, |reading| {
                let groups = action_groups(reading, None, &folders, true, false, &detail::Anchors::default());
                let file = &group(&groups, tr("File")).items;
                let item = file.iter().find(|i| i.label == on).expect("Remove from …");
                assert_eq!(item.key, off, "{id}: the pile's own key");
                assert!(matches!(item.press(), Some(Message::Pile(sent, None)) if sent == id), "{id}: off the pile");
                let move_to = sends(&groups, tr("File"), other);
                assert!(
                    matches!(move_to, Some(Message::Pile(sent, Some(pile))) if sent == id && pile == moved),
                    "{id}"
                );
            });
        }
    }

    /// Task E14.3: "More" offers the other view of a mail with HTML (`v`), the other colours while the
    /// HTML shows, "Copy text" and – in the reader – Close; a mail without HTML only the last two.
    /// While the mail loads Mark and More wait for it.
    #[test]
    fn more_offers_the_view_of_the_mail_as_it_stands() {
        let folders = folders();
        let more = |reading: &Reading<'_>, close| {
            let groups = action_groups(reading, None, &folders, close, false, &detail::Anchors::default());
            groups.iter().find(|group| group.label == tr("More")).map(|group| {
                group
                    .items
                    .iter()
                    .map(|item| (item.label.to_string(), item.key, format!("{:?}", item.press())))
                    .collect::<Vec<_>>()
            })
        };
        let item = |word: &str, key, message: Message| (word.to_owned(), key, format!("{:?}", Some(message)));
        reading_of("m:p2", MailFormat::Html, true, |reading| {
            assert_eq!(
                more(reading, true),
                Some(vec![
                    item(tr("Copy text"), "", Message::CopyText),
                    item(tr("Show as text"), "V", Message::ShowMailAs(MailFormat::Text)),
                    item(tr("Dark colours"), "", Message::MailColors(Scheme::Dark)),
                    item(tr("Close"), "Esc", Message::CloseDetail),
                ])
            );
            let dark = HtmlReading { scheme: Scheme::Dark, ..reading.html.expect("its HTML") };
            let dark = Reading { html: Some(dark), ..*reading };
            let light = more(&dark, false).expect("More");
            assert_eq!(light[2], item(tr("Light colours"), "", Message::MailColors(Scheme::Light)));
        });
        reading_of("m:p2", MailFormat::Text, true, |reading| {
            assert_eq!(
                more(reading, false),
                Some(vec![
                    item(tr("Copy text"), "", Message::CopyText),
                    item(tr("Show as HTML"), "V", Message::ShowMailAs(MailFormat::Html)),
                ]),
                "no colours for the text"
            );
        });
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            assert_eq!(more(reading, false), Some(vec![item(tr("Copy text"), "", Message::CopyText)]), "a text mail");
        });
        reading_of("m:w1", MailFormat::Html, false, |reading| {
            assert_eq!(more(reading, false), None, "nothing to show while it loads");
            assert_eq!(more(reading, true).map(|items| items.len()), Some(1), "but Close");
            let loading = Reading { summary: None, ..*reading };
            let groups = action_groups(&loading, None, &folders, false, false, &detail::Anchors::default());
            let titles: Vec<String> = rail_words(&groups).into_iter().map(|(title, _)| title).collect();
            // File keeps its piles' places, off (review of #315); the rest needs the headers.
            assert_eq!(
                titles,
                strings(&[tr("Reply"), tr("File"), tr("Torro"), tr("Mute")]),
                "only what needs no headers"
            );
            assert!(group(&groups, tr("File")).items.iter().all(|i| i.press().is_none()), "File is off");
        });
    }

    /// Task E11.3: the attachments stand as chips at the reader's bottom once the mail is in – none
    /// while it loads, none for inline pictures –, and a long name ends in "…" before it takes half
    /// the line.
    #[test]
    fn attachments_are_chips_at_the_bottom() {
        let anchors = detail::Anchors::default();
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            assert!(attachment_line(reading, &anchors, 800.0, 24.0).is_some(), "the budget's sheet");
        });
        reading_of("m:w1", MailFormat::Html, false, |reading| {
            assert!(attachment_line(reading, &anchors, 800.0, 24.0).is_none(), "nothing while it loads");
        });
        reading_of("m:p2", MailFormat::Html, true, |reading| {
            assert!(attachment_line(reading, &anchors, 800.0, 24.0).is_none(), "the newsletter's pictures are inline");
        });
        let room = chip_name_room("3 KB", 800.0);
        assert!(room > 0.0 && room < 400.0, "{room}");
        let long = "quarterly-budget-review-with-all-departments-and-travel-costs-final-v7.xlsx";
        let shown = typo::ellipsize(long, typo::body(), room);
        assert!(shown != long && shown.ends_with('…'), "{shown}");
        assert_eq!(typo::ellipsize("budget-q4.csv", typo::body(), room), "budget-q4.csv", "a short name whole");
        assert_eq!(chip_name_room("3 KB", 0.0), 0.0, "never below nothing");
    }

    /// Design review of #290 (state "full"): a mail with many attachments (the demo's minutes,
    /// `m:w6`) keeps its block to two lines of chips and the panel's footer to four rows; the rest
    /// scrolls inside them, so the mail keeps its room in a low window.
    #[test]
    fn many_attachments_scroll_inside_their_block_and_rows() {
        let anchors = detail::Anchors::default();
        assert_eq!(chips_max_height(), 2.0 * size::control() + space::s());
        assert_eq!(rows_max_height(), 4.0 * size::control());
        reading_of("m:w6", MailFormat::Text, true, |reading| {
            let files = files_of(reading).expect("the minutes and the slides");
            assert!(files.len() > 2 * FILE_ROWS as usize, "more than fit: {}", files.len());
            assert!(files.len() as f32 * size::control() > rows_max_height(), "the rows would not fit");
            assert!(attachment_line(reading, &anchors, 800.0, 24.0).is_some());
            assert!(attachment_rows(reading, &anchors).is_some());
        });
    }

    /// The demo's plans for the mail `id` (task M12.6), as `mail.list_save_targets` answers them.
    fn saving_of(id: &str) -> SaveTargets {
        let demo = DemoSource::new(now());
        iced::futures::executor::block_on(MailSource::save_targets(&demo, Some(id.to_owned()))).expect("planned")
    }

    /// Task M12.6: "Save all ▾" stands over two attachments and more, not over one; the rail's File
    /// has "Save attachments (n)" only once the mail shows attachments; the panel lists them as rows.
    #[test]
    fn save_all_shows_from_two_attachments_on() {
        let anchors = detail::Anchors::default();
        let file_words = |reading: &Reading<'_>| -> Vec<String> {
            let groups = action_groups(reading, None, &folders(), true, false, &anchors);
            let file = groups.into_iter().find(|group| group.label == tr("File")).expect("File");
            file.items.iter().map(|item| item.label.to_string()).collect()
        };
        let two = trf("Save attachments ({n})", &[("n", &2)]);
        reading_of("m:p3", MailFormat::Text, true, |reading| {
            let files = files_of(reading).expect("two files");
            assert_eq!(files.len(), 2);
            assert!(save_all_control(reading, &files, &anchors, true).is_some(), "two: Save all");
            assert!(file_words(reading).contains(&two));
            assert_eq!(files_meta(&files), format!("{} · {}", trf("{n} files", &[("n", &2)]), size_text(144_690)));
            assert!(attachment_rows(reading, &anchors).is_some() && !panel_foot(reading, &anchors).is_empty());
        });
        reading_of("m:w1", MailFormat::Text, true, |reading| {
            let files = files_of(reading).expect("one file");
            assert!(save_all_control(reading, &files, &anchors, false).is_none(), "one: no Save all");
            let one = trf("Save attachments ({n})", &[("n", &1)]);
            assert!(file_words(reading).contains(&one));
        });
        reading_of("m:p3", MailFormat::Text, false, |reading| {
            assert!(files_of(reading).is_none(), "nothing while it loads");
            assert!(!file_words(reading).iter().any(|words| words.contains("(")), "no item while it loads");
        });
    }

    /// Task M12.6: the click by every setting – a target that is gone and a suggestion on a guessed
    /// category open (decision 4 of issue #290) –, and "Save all" never opens.
    #[test]
    fn the_click_follows_the_setting() {
        let mut saving = saving_of("m:p3");
        let invoices = || FileAct::Target("demo-invoices".into());
        assert_eq!(click_act(&AttachmentClick::Open, Some(&saving), "2"), FileAct::Open);
        assert_eq!(click_act(&AttachmentClick::Downloads, None, "2"), FileAct::Downloads);
        assert_eq!(click_act(&AttachmentClick::SaveAs, None, "2"), FileAct::Choose);
        let target = AttachmentClick::Target("demo-invoices".into());
        assert_eq!(click_act(&target, Some(&saving), "2"), invoices());
        assert_eq!(click_act(&target, None, "2"), invoices(), "no plans: the core decides");
        assert_eq!(click_act(&AttachmentClick::Target("gone".into()), Some(&saving), "2"), FileAct::Open);
        assert_eq!(click_act(&AttachmentClick::Suggested, Some(&saving), "2"), FileAct::Open, "a guessed category");
        saving.files[0].sure = Some("demo-invoices".into());
        assert_eq!(click_act(&AttachmentClick::Suggested, Some(&saving), "2"), invoices(), "a sure one saves");
        assert_eq!(click_act(&AttachmentClick::Suggested, Some(&saving), "3"), FileAct::Open, "the other file's plan");
        for (click, act) in [
            (AttachmentClick::Open, FileAct::Downloads),
            (AttachmentClick::Suggested, FileAct::Downloads),
            (AttachmentClick::Downloads, FileAct::Downloads),
            (AttachmentClick::SaveAs, FileAct::Choose),
            (target, invoices()),
            (AttachmentClick::Target("gone".into()), FileAct::Downloads),
        ] {
            assert_eq!(all_act(&click, Some(&saving)), act, "{click:?}");
        }
        let target = AttachmentClick::Target("demo-invoices".into());
        assert_eq!(all_act(&target, None), invoices(), "no plans: not silently to Downloads");
        assert_eq!(act_words(&invoices(), Some(&saving), false), trf("Save to {target}", &[("target", &"Invoices")]));
        assert_eq!(act_words(&FileAct::Choose, None, true), tr("Save all to folder…"));
    }

    /// The words of the tags of `item`, with whether each has the sparkles.
    fn tags_of(item: &MenuItem<'_, Message>) -> Vec<(String, bool)> {
        item.tags.iter().map(|tag| (tag.label.to_string(), tag.sparkles)).collect()
    }

    /// Task M12.6: the menu of an attachment – Open, Downloads, Save as…, the targets suggested first
    /// with where the file would go, "Suggested" with the sparkles only for a guessed category
    /// (decision 7), "Click" at what a click does – and the menu of "Save all" without Open and "Click".
    #[test]
    fn the_menu_marks_the_click_and_the_suggestion() {
        let saving = saving_of("m:p3");
        let guessed = Some(MailCategory::by_model(torro_mail::Category::Invoice, 0.9));
        let click = AttachmentClick::Target("demo-invoices".into());
        let menu = FileMenu { id: "m:p3", part: Some("2"), saving: Some(&saving), click: &click, category: guessed };
        let sections = file_menu(menu);
        assert_eq!(sections.len(), 3, "ways, targets, manage");
        let ways: Vec<String> = sections[0].items.iter().map(|item| item.label.to_string()).collect();
        assert_eq!(ways, [tr("Open"), tr("Save to Downloads"), tr("Save as…")]);
        assert!(sections[0].items.iter().all(|item| item.tags.is_empty()), "the click is the target");
        assert_eq!(sections[1].title.as_deref(), Some(tr("Save to")));
        let invoices = &sections[1].items[0];
        assert_eq!(invoices.label, "Invoices");
        assert_eq!(invoices.detail.as_deref(), Some("Invoices › 2026 › 09"), "by the date in its name");
        assert_eq!(tags_of(invoices), [(tr("Suggested").to_owned(), true), (tr("Click").to_owned(), false)]);
        let contracts = &sections[1].items[1];
        assert!(tags_of(contracts).is_empty(), "not suggested for an invoice");
        assert!(
            matches!(&invoices.on_press, Message::FileAct(id, Some(part), FileAct::Target(t)) if id == "m:p3" && part == "2" && t == "demo-invoices")
        );
        assert_eq!(sections[2].items[0].label, tr("Manage save targets…"));

        // Filed by a person: still suggested, without the sparkles; the click on Open marks Open.
        let by_hand = Some(MailCategory::by_person(torro_mail::Category::Invoice));
        let menu = FileMenu { click: &OPEN, category: by_hand, ..menu };
        let sections = file_menu(menu);
        assert_eq!(tags_of(&sections[0].items[0]), [(tr("Click").to_owned(), false)]);
        assert_eq!(tags_of(&sections[1].items[0]), [(tr("Suggested").to_owned(), false)]);

        // "Save all": no Open, no "Click", each file by its own date.
        let sections = file_menu(FileMenu { part: None, ..menu });
        let ways: Vec<String> = sections[0].items.iter().map(|item| item.label.to_string()).collect();
        assert_eq!(ways, [tr("Save all to Downloads"), tr("Save all to folder…")]);
        assert!(sections.iter().flat_map(|section| &section.items).all(|item| item.tags.is_empty()));
        assert_eq!(sections[1].items[0].detail.as_deref(), Some(tr("by each file's date")));
        // Without targets – still loading – only the ways and "Manage save targets…".
        assert_eq!(file_menu(FileMenu { saving: None, ..menu }).len(), 2);
        let _ = file_menu_card(file_menu(menu), size::panel(), 600.0);
    }

    /// Task M12.6: what the hint says after saving and which folder "Show in folder" opens; the demo
    /// answers where the core would put the files, without writing one.
    #[test]
    fn the_hint_says_where_the_files_went() {
        let saving = saving_of("m:p3");
        let mail = DemoSource::new(now()).get_mail("m:p3").expect("the invoice");
        let one = demo_saved(&mail, &["2".into()], &SaveTo::Target("demo-invoices".into())).expect("planned");
        let path = "/home/anna/Documents/Invoices/2026/09/invoice-2026-09-30.pdf";
        assert_eq!(one.saved.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), [path]);
        assert!(!Path::new(path).exists(), "nothing written");
        let place = saved_place(save_place(&SaveTo::Target("demo-invoices".into()), Some(&saving), Some("2")), &one);
        assert_eq!(place, "Invoices › 2026 › 09");
        assert_eq!(saved_text(&one, &place), trf("Saved to {where}", &[("where", &place)]));
        assert_eq!(saved_folder(&one), Some(PathBuf::from("/home/anna/Documents/Invoices/2026/09")));

        let all = demo_saved(&mail, &[], &SaveTo::Downloads).expect("planned");
        assert_eq!(all.saved.len(), 2);
        let place = saved_place(save_place(&SaveTo::Downloads, Some(&saving), None), &all);
        assert_eq!(
            saved_text(&all, &place),
            trf("{n} attachments saved to {where}", &[("n", &2), ("where", &tr("Downloads"))])
        );
        assert_eq!(
            save_place(&SaveTo::Target("demo-invoices".into()), Some(&saving), None).as_deref(),
            Some("Invoices"),
            "all: the target"
        );
        assert_eq!(save_place(&SaveTo::Folder("/home/anna/Bills".into()), None, None).as_deref(), Some("Bills"));
        assert_eq!(
            save_place(&SaveTo::File("/home/anna/Bills/a.pdf".into()), None, Some("2")).as_deref(),
            Some("Bills")
        );
        // No plans when it was sent (the mail's targets could not be loaded): the folder it went to.
        let unplanned = save_place(&SaveTo::Target("demo-invoices".into()), None, Some("2"));
        assert_eq!((unplanned.as_deref(), saved_place(unplanned.clone(), &one).as_str()), (None, "09"));
        assert_eq!(demo_saved(&mail, &["9".into()], &SaveTo::Downloads), Err(InboxError::NotFound));
        assert!(demo_saved(&mail, &[], &SaveTo::File("/tmp/a.pdf".into())).is_err(), "one file at a time");
        assert_eq!(saved_folder(&SavedAttachments::default()), None);
    }

    /// The source of the function `name` in this file, up to its closing brace.
    fn function_source<'s>(source: &'s str, name: &str) -> &'s str {
        let start = [format!("fn {name}<"), format!("fn {name}(")]
            .iter()
            .find_map(|head| source.find(head.as_str()))
            .expect("the function");
        let rest = &source[start..];
        &rest[..rest.find("\n}\n").expect("its end")]
    }

    /// Task E11.3: the reader has no red button – no part of it draws `style::button::primary`; Reply
    /// stands in the rail in the accent (task E14.3).
    #[test]
    fn the_reader_has_no_red_button() {
        let source = include_str!("mail.rs");
        let parts = [
            "reader",
            "reader_mail",
            "head",
            "sender_block",
            "attachment_line",
            "attachment_chip",
            "action_groups",
            // The mail window's panel (task E11.4).
            "entry_panel",
            "command_body",
            "head_with",
            "panel_letter",
            "panel_foot",
            "attachment_chips",
        ];
        for name in parts {
            assert!(!function_source(source, name).contains(concat!("button::", "primary")), "{name}");
        }
        // The check sees the panel's Reply, which stays primary in the hub (task E9.4).
        assert!(function_source(source, "panel_standards").contains(concat!("button::", "primary")));
    }

    /// Task E14.3: the reader and the mail window's panel have no bar over the mail and no buttons in
    /// the head – the rail beside the reader, the window's `Panel::side` beside the panel –, and the
    /// panel's footer holds only the attachments; the split of task E9.4 stays for the hub and the
    /// App Feed.
    #[test]
    fn the_mail_windows_reader_and_panel_leave_the_actions_to_the_rail() {
        let source = include_str!("mail.rs");
        let reader = function_source(source, "reader");
        assert!(reader.contains("row![page, rail]"), "the rail at the reader's right, over its whole height");
        for name in ["reader", "head_with", "command_body", "panel", "entry_panel", "panel_foot"] {
            let body = function_source(source, name);
            for part in [".bar(", "_button(", "segmented(", "view_controls("] {
                assert!(!body.contains(part), "{name}: {part}");
            }
        }
        let command = function_source(source, "command_body");
        for part in ["head_with(", "detail::properties(", "panel_letter(", "RailMode::Collapsed.width()"] {
            assert!(command.contains(part), "{part}");
        }
        assert!(command.contains("room, false)"), "the head without the subject");
        assert!(!command.contains("panel_standards") && !command.contains("header("), "none of the split");
        let split = function_source(source, "split_body");
        assert!(split.contains("panel_standards(") && split.contains("header("), "the hub's panel as in E9.4");
        let width = size::panel();
        let anchors = detail::Anchors::default();
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            assert_eq!(panel_foot(reading, &anchors).len(), 1, "the sheet's row");
        });
        reading_of("m:p2", MailFormat::Html, true, |reading| {
            assert!(panel_foot(reading, &anchors).is_empty(), "no attachments, no footer: the switches are in More");
        });
        reading_of("m:w1", MailFormat::Html, false, |reading| {
            assert!(panel_foot(reading, &anchors).is_empty(), "no footer while the mail loads");
        });
        let item = mail_entry();
        let entry = ReaderEntry { item: &item, hide: true };
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            let anchors = detail::Anchors::default();
            for width in [width, 480.0, 0.0] {
                let _ = entry_panel(reading, entry, tr("For me").to_owned(), width, &anchors);
                let _ = panel(reading, None, &anchors, Bars::Command { entry: None, width });
            }
        });
    }

    #[test]
    fn files_that_run_programs_are_not_opened() {
        for risky in ["setup.exe", "install.SH", "x.desktop", "Report.pdf.js", "tool.app", "no-extension"] {
            assert!(runs_programs(risky), "{risky}");
        }
        for safe in ["budget-q4.csv", "Invoice.PDF", "photo.jpg", "minutes.txt"] {
            assert!(!runs_programs(safe), "{safe}");
        }
    }

    #[test]
    fn people_and_sizes_read_well() {
        let jonas = MailAddress { name: Some("Jonas\nWeber".into()), address: "jonas@x.example".into() };
        assert_eq!(person(&jonas), "Jonas Weber <jonas@x.example>");
        let bare = MailAddress { name: Some("  ".into()), address: "it@x.example".into() };
        assert_eq!((person(&bare), sender_name(Some(&bare))), ("it@x.example".into(), "it@x.example".into()));
        assert_eq!(sender_name(None), tr("Unknown sender"));
        assert_eq!(size_text(860), trf("{n} B", &[("n", &860)]));
        assert_eq!(size_text(2_340), trf("{n} KB", &[("n", &3)]));
        assert_eq!(size_text(5 * 1024 * 1024), trf("{n} MB", &[("n", &5)]));
    }

    #[test]
    fn a_message_row_names_its_sender_and_domain_like_an_entry() {
        let who =
            |name: Option<&str>, address: &str| MailAddress { name: name.map(String::from), address: address.into() };
        let jonas = who(Some("Jonas Weber"), "jonas.weber@Work.Example.org");
        assert_eq!(sender_parts(Some(&jonas)), ("Jonas Weber".into(), Some("work.example.org".into())));
        let bare = who(None, "paul@example.org");
        assert_eq!(sender_parts(Some(&bare)), ("paul@example.org".into(), None), "no domain beside the address");
        assert_eq!(sender_parts(None), (tr("Unknown sender").to_owned(), None));
    }

    #[test]
    fn the_hint_names_what_was_opened() {
        let file = opened_text(&Opened::File(PathBuf::from("/tmp/a/budget.csv")));
        assert_eq!(file, trf("Opened {file}", &[("file", &"budget.csv")]));
        let folder = opened_text(&Opened::Folder(PathBuf::from("/tmp/a/setup.exe")));
        assert!(folder.contains("setup.exe"), "{folder}");
    }

    #[test]
    fn every_demo_mail_builds_a_list_and_a_panel() {
        let mail = demo::mail_sample(now());
        let summaries: Vec<MessageSummary> = mail.iter().map(|m| m.summary.clone()).collect();
        let folders = folders();
        let folder = FolderView::of(first_inbox(&folders).expect("an inbox"));
        let search = crate::search::SearchView::new("budget", crate::search::Scope::All, MailSort::Relevance);
        let views = [View::Folder(folder.clone()), View::Mail, View::Search(search)];
        // A search's matches in the subject and the passage of the first message (issue #136).
        let hit = |text: &str, hit| torro_ipc::mail::HighlightSpan { text: text.into(), hit };
        let highlights = [MailHighlight {
            id: summaries[0].id.clone(),
            subject: vec![hit("Budget", true), hit(" review", false)],
            snippet: vec![hit("the travel ", false), hit("budget", true), hit(" before Thursday", false)],
        }];
        let looks = [Look::Rows, Look::Cards];
        let all = iced::futures::executor::block_on(torro_inbox::InboxSource::list_items(
            &DemoSource::new(now()),
            Default::default(),
        ))
        .expect("the demo answers");
        for accounts in [Vec::new(), vec!["anna@work.example.org", "anna@example.org"]] {
            for (view, look) in views.iter().flat_map(|view| looks.map(|look| (view, look))) {
                for (messages, bare) in [(&summaries[..], false), (&summaries[..], true), (&[], true)] {
                    let data = MailList {
                        categories: &[],
                        messages,
                        view,
                        no_accounts: false,
                        selected: Some("m:w1"),
                        selected_ids: &[],
                        now: now(),
                        anchors: detail::Anchors::default(),
                        accounts: accounts.clone(),
                        more: true,
                        attention: Color::BLACK,
                        look,
                        entries: &all,
                        folders: &folders,
                        highlights: &highlights,
                        text: Color::BLACK,
                        bare,
                    };
                    let _ = list(data);
                }
            }
        }
        let _ = empty_reader();
        let entry = all.iter().find(|item| item.source.kind == SourceKind::Mail).cloned().expect("mail");
        let open = task(false);
        let done = ItemTask { due: Some(TaskDue::Date(now().date_naive())), ..task(true) };
        for (message, task) in mail.iter().zip([None, Some(&open), Some(&done)].into_iter().cycle()) {
            let loaded = Ok(message.clone());
            let text = Selectable::new(&text_of(message));
            // The HTML of a mail that has it (task M9.7), as HTML once and as text once.
            let html = HtmlMail::new(message, &Arc::new(DemoSource::new(now())), true);
            for loaded in [None, Some(&loaded)] {
                for read in [false, true] {
                    let format = if read { MailFormat::Text } else { MailFormat::Html };
                    let html = html.as_ref().map(|html| HtmlReading {
                        content: &html.content,
                        format,
                        scheme: Scheme::Dark,
                        scale: 1.0,
                    });
                    let saving = iced::futures::executor::block_on(MailSource::save_targets(
                        &DemoSource::new(now()),
                        Some(message.summary.id.clone()),
                    ))
                    .expect("the demo plans");
                    let reading = Reading {
                        categories: &[],
                        task,
                        id: &message.summary.id,
                        summary: Some(&message.summary),
                        loaded,
                        now: now(),
                        text: &text,
                        attention: Color::BLACK,
                        in_trash: false,
                        deciding: false,
                        dated: true,
                        read,
                        account: Some(0),
                        answering: false,
                        invite: InviteSide::default(),
                        html,
                        saving: Some(&saving),
                        click: &OPEN,
                        file_menu: Some(0),
                    };
                    let anchors = detail::Anchors::default();
                    let _ = panel(&reading, Some(&folder), &anchors, Bars::Split);
                    for entry in [None, Some(ReaderEntry { item: &entry, hide: true })] {
                        // The mail window's panel (task E11.4), over the medium window and filling a
                        // narrow one.
                        for width in [size::panel(), 480.0] {
                            let _ = panel(&reading, Some(&folder), &anchors, Bars::Command { entry, width });
                            let _ = body(&reading, Bars::Command { entry, width }, &anchors);
                            if let Some(entry) = entry {
                                let _ = entry_panel(&reading, entry, tr("For me").to_owned(), width, &anchors);
                            }
                        }
                        let _ = body(&reading, Bars::Split, &anchors);
                        let groups = action_groups(&reading, entry, &folders, true, false, &anchors);
                        let rail =
                            widgets::action_rail(groups, widgets::RailMode::Collapsed, None, Message::Rail, None);
                        let _ = reader(reading, entry, rail, &anchors);
                        // The reader's head, which it lays out only once it knows its width.
                        let _ = head(&reading, entry, 900.0);
                    }
                    let _ = reader_mail(&reading, reader_widths(900.0));
                    let _ = attachment_line(&reading, &anchors, 900.0, layout::page_padding().left);
                }
            }
        }
    }

    /// Task E8.11 (finding 3): an invitation's row in a search of the hub offers Later and the bell
    /// for its entry, as the same mail does under "For me"; without an open entry only the answers and
    /// Task; a message as any has Archive · Delete · Task (issue #118).
    #[test]
    fn an_invitation_row_offers_later_and_more_for_its_entry() {
        let mut message = demo::mail_sample(now()).remove(0).summary;
        let start = now() + chrono::TimeDelta::days(1);
        message.invite = Some(Invite { title: "Planning".into(), start, end: None, all_day: false, answered: None });
        let all = iced::futures::executor::block_on(torro_inbox::InboxSource::list_items(
            &DemoSource::new(now()),
            Default::default(),
        ))
        .expect("the demo answers");
        let mail = all.iter().find(|item| item.source.kind == SourceKind::Mail).expect("a mail entry");
        let entry = Item { link: Some(Link::Mail { message: message.id.clone() }), state: State::New, ..mail.clone() };
        let entries = [entry.clone()];
        let words =
            |buttons: &[RowAction<'_, Message>]| buttons.iter().map(|b| b.label.to_string()).collect::<Vec<_>>();
        let folders = folders();
        let (buttons, more) = message_buttons(&message, &entries, &folders, now());
        assert_eq!(words(&buttons), [tr("Accept"), tr("Maybe"), tr("Decline"), tr("Later"), tr("Task")]);
        assert!(matches!(&buttons[3].on_press, Message::Act(id, Key::Snooze) if *id == entry.id));
        assert!(matches!(more.map(|more| more.on_press), Some(Message::Act(id, Key::Hide)) if id == entry.id));
        let (alone, more) = message_buttons(&message, &[], &folders, now());
        assert_eq!(words(&alone), [tr("Accept"), tr("Maybe"), tr("Decline"), tr("Task")], "no entry: no Later");
        let more = more.expect("the bell (issue #123)");
        assert_eq!((more.icon, more.label.to_string()), (Icon::BellOff, tr("Mute…").to_owned()));
        assert!(matches!(more.on_press, Message::Act(id, Key::Hide) if id == message.id), "the message's own");
        let done = Item { state: State::Done { at: now() }, ..entry.clone() };
        assert_eq!(message_buttons(&message, &[done], &folders, now()).0.len(), 4, "a done entry is put away");
        let plain = MessageSummary { invite: None, ..message.clone() };
        let words_of = words(&message_buttons(&plain, &entries, &folders, now()).0);
        assert_eq!(words_of, [trc("action", "Archive"), tr("Delete"), tr("Task")]);
    }

    /// Task E14.2: in the mail window (bare rows) the selected message's row holds no buttons – a
    /// plain message and an invitation alike – and stays as high as any row; outside it (a search of
    /// mail in the hub) the selected row keeps its strip: the buttons and the bell last.
    #[test]
    fn a_messages_row_in_the_mail_window_has_no_buttons() {
        let folders = folders();
        let view = View::Mail;
        let mut invitation = demo::mail_sample(now()).remove(0).summary;
        let start = now() + chrono::TimeDelta::days(1);
        invitation.invite = Some(Invite { title: "Planning".into(), start, end: None, all_day: false, answered: None });
        let plain = MessageSummary { invite: None, ..invitation.clone() };
        let messages = [plain.clone(), invitation.clone()];
        let data = |bare, selected: &'static str| MailList {
            categories: &[],
            messages: &messages,
            view: &view,
            no_accounts: false,
            selected: Some(selected),
            selected_ids: &[],
            now: now(),
            anchors: detail::Anchors::default(),
            accounts: Vec::new(),
            more: false,
            attention: Color::BLACK,
            look: Look::Rows,
            entries: &[],
            folders: &folders,
            highlights: &[],
            text: Color::BLACK,
            bare,
        };
        for message in &messages {
            let bare = MailList { selected: Some(message.id.as_str()), ..data(true, "") };
            assert!(row_buttons(&bare, message).is_empty(), "{}: no buttons in the mail window", message.id);
            let hub = MailList { selected: Some(message.id.as_str()), ..data(false, "") };
            let (mut buttons, more) = message_buttons(message, &[], &folders, now());
            buttons.extend(more);
            let words =
                |actions: &[RowAction<'_, Message>]| actions.iter().map(|a| a.label.to_string()).collect::<Vec<_>>();
            assert_eq!(words(&row_buttons(&hub, message)), words(&buttons), "the hub's strip, the bell last");
            assert!(row_buttons(&data(false, "someone-else"), message).is_empty(), "only the cursor's row");
            for width in [0.0, 240.0, 480.0] {
                let _ = message_row(&bare, message, width);
            }
        }
    }

    /// Issue #118: a message's row and card file it – Archive (`e`) and Delete (`#`) send what the
    /// keys send – and never offer Reply, which stands in the reader only. A button the account
    /// cannot carry out is not there: no archive folder, no Archive; in the trash, no Delete.
    #[test]
    fn a_message_is_archived_and_deleted_from_its_row_never_replied_to() {
        let message = MessageSummary { invite: None, ..demo::mail_sample(now()).remove(0).summary };
        let folders = folders();
        let (buttons, more) = message_buttons(&message, &[], &folders, now());
        let sent: Vec<&Message> = buttons.iter().map(|button| &button.on_press).collect();
        assert!(matches!(sent[0], Message::Mail(MailAction::Archive, id) if *id == message.id), "{sent:?}");
        assert!(matches!(sent[1], Message::Mail(MailAction::Delete, id) if *id == message.id), "{sent:?}");
        assert!(matches!(sent[2], Message::Act(id, Key::MakeTask) if *id == message.id), "{sent:?}");
        assert_eq!(buttons.iter().map(|button| button.key).collect::<Vec<_>>(), ["E", "#", "T"]);
        let bell = more.map(|more| more.on_press);
        assert!(matches!(bell, Some(Message::Act(id, Key::Hide)) if id == message.id), "the bell (issue #123)");
        assert!(!sent.iter().any(|message| matches!(message, Message::Mail(MailAction::Reply, _))));

        let words =
            |actions: Vec<RowAction<'_, Message>>| actions.iter().map(|a| a.label.to_string()).collect::<Vec<_>>();
        let without = |role| folders.iter().filter(|f| f.role != Some(role)).cloned().collect::<Vec<_>>();
        let filed = |folders: &[Folder], folder: Option<&str>| {
            words(filing_actions(folders, &message.account, folder, &message.id, "E"))
        };
        assert_eq!(filed(&without(Role::Archive), None), [tr("Delete")], "no archive folder");
        assert_eq!(filed(&without(Role::Trash), None), [trc("action", "Archive")], "no trash");
        let of =
            |role| folders.iter().find(|f| f.account == message.account && f.role == Some(role)).map(|f| f.id.as_str());
        assert_eq!(filed(&folders, of(Role::Trash)), [trc("action", "Archive")], "in the trash already");
        assert_eq!(filed(&folders, of(Role::Archive)), [tr("Delete")], "archived already");
        assert!(filed(&[], None).is_empty(), "folders not known yet");
    }

    /// Task E8.11 (finding 5): a message's card is as high as the lines its preview takes.
    #[test]
    fn a_message_card_is_as_high_as_its_preview() {
        let message = demo::mail_sample(now()).remove(0).summary;
        let short = MessageSummary { snippet: "Short.".into(), ..message.clone() };
        let long = MessageSummary { snippet: "word ".repeat(80), ..message.clone() };
        let line = typo::line_height(typo::body());
        assert_eq!(message_card_height(&long, 480.0) - message_card_height(&short, 480.0), line);
    }

    /// A task made of a mail, open or `completed`, in the list "Work", without a due day.
    fn task(completed: bool) -> ItemTask {
        ItemTask {
            uid: "budget@demo".into(),
            collection: "demo/work".into(),
            list: "Work".into(),
            title: "Check the travel budget".into(),
            due: None,
            completed,
            link: "mail:anna@work.example.org:budget@work.example.org".into(),
            mail_entry: None,
        }
    }

    /// Task E8.7b: the card says what the task is, where it is, when it is due and whether it is open
    /// – all in words.
    #[test]
    fn the_task_card_names_list_due_day_and_state() {
        let today = now().with_timezone(&Local).date_naive();
        assert_eq!(task_meta(&task(false), now()), format!("{} · Work · {}", tr("Task"), tr("open")));
        let due = ItemTask { due: Some(TaskDue::Date(today)), list: String::new(), ..task(true) };
        let day = datetime::format_short_date(today, today);
        assert_eq!(
            task_meta(&due, now()),
            format!("{} · {} · {}", tr("Task"), trf("due {day}", &[("day", &day)]), tr("done"))
        );
        let _ = task_card(&task(false), now());
    }

    /// Task E8.12 (finding 2): a narrow task card leaves out "Task", then the state, before it cuts
    /// the line – never "… · ope".
    #[test]
    fn a_narrow_task_card_leaves_out_task_then_the_state() {
        let parts = task_meta_parts(&task(false), now());
        assert_eq!(parts, [tr("Task"), "Work", tr("open")]);
        let drop = task_meta_drop(parts.len());
        let level = typo::meta();
        let fit = |room| widgets::meta_fit(&parts, &drop, level, room);
        assert_eq!(fit(1000.0), task_meta(&task(false), now()));
        let shorter = format!("Work · {}", tr("open"));
        assert_eq!(fit(typo::text_width(&shorter, level)), shorter, "\"Task\" goes first");
        assert_eq!(fit(typo::text_width(&shorter, level) - 1.0), "Work", "then the state");
        assert!(task_meta_drop(1).is_empty());
    }

    /// Task E8.7b: "Task" on a folder's message makes a task of it in the demo, titled by its subject;
    /// task E8.1b: with the day asked for as due date.
    #[test]
    fn a_folder_message_becomes_a_task() {
        let demo = DemoSource::new(now());
        let list = TaskList { id: "demo/work".into(), name: "Work".into() };
        let day = NaiveDate::from_ymd_opt(2026, 10, 16).expect("valid date");
        let made = iced::futures::executor::block_on(MailSource::create_todo_from_mail(
            &demo,
            demo::MAIL_ACCOUNTS[0].into(),
            "m:w1".into(),
            list.clone(),
            None,
            Some(day),
        ))
        .expect("the demo makes it");
        assert_eq!((made.title.as_str(), made.list.as_str(), made.completed), ("Budget review for Q4", "Work", false));
        assert_eq!(made.due, Some(TaskDue::Date(day)));
        assert!(made.link.starts_with(demo::MAIL_ACCOUNTS[0]), "{}", made.link);
        let unknown = iced::futures::executor::block_on(MailSource::create_todo_from_mail(
            &demo,
            demo::MAIL_ACCOUNTS[0].into(),
            "m:nope".into(),
            list,
            Some("x".into()),
            None,
        ));
        assert!(unknown.is_err());
    }

    /// Task E18.3: the demo lists the mail of a role a page at a time, counts the unread mail of
    /// "All" without the hidden newsletter, and shows it again in "All" until the undo.
    #[test]
    fn the_demo_lists_mail_by_role_counts_all_and_shows_mail_again() {
        let demo = DemoSource::new(now());
        let by_role = |role, hidden, limit, cursor: Option<String>| {
            let find = MailByRole {
                role,
                account: None,
                snoozed: (role == Role::Inbox).then_some(false),
                hidden,
                pile: (role == Role::Inbox).then_some(PileFilter::Without),
                order: Order::NewestFirst,
                limit,
                cursor,
            };
            iced::futures::executor::block_on(MailSource::mail_by_role(&demo, find)).expect("a page")
        };
        let first = by_role(Role::Archive, None, 1, None);
        assert_eq!((first.messages[0].id.as_str(), first.next.as_deref()), ("m:w9", Some("1")));
        let last = by_role(Role::Archive, None, 1, first.next);
        assert_eq!((last.messages[0].id.as_str(), last.next), ("m:p6", None), "the archives of both accounts");
        let all = by_role(Role::Inbox, Some(false), 50, None).messages;
        assert!(!all.iter().any(|message| message.id == demo::HIDDEN_MAIL), "hidden from All");
        let hidden = by_role(Role::Inbox, Some(true), 50, None).messages;
        assert_eq!(hidden.iter().map(|message| message.id.as_str()).collect::<Vec<_>>(), [demo::HIDDEN_MAIL]);
        let bad = MailByRole {
            role: Role::Inbox,
            account: None,
            snoozed: None,
            hidden: None,
            pile: None,
            order: Order::NewestFirst,
            limit: 1,
            cursor: Some("x".into()),
        };
        assert!(matches!(
            iced::futures::executor::block_on(MailSource::mail_by_role(&demo, bad)),
            Err(InboxError::Other(_))
        ));

        let unread = |account: Option<&str>| {
            iced::futures::executor::block_on(MailSource::unread_in_all(&demo, account.map(str::to_owned)))
                .expect("a number")
        };
        let unread_shown = all.iter().filter(|message| !message.flags.contains(&Flag::Seen)).count() as u64;
        assert_eq!(unread(None), unread_shown, "the unread mail of All, the newsletter left out");
        assert_eq!(unread(Some(demo::MAIL_ACCOUNTS[0])) + unread(Some(demo::MAIL_ACCOUNTS[1])), unread(None));

        let show = |shown| {
            iced::futures::executor::block_on(MailSource::set_mail_shown(&demo, vec![demo::HIDDEN_MAIL.into()], shown))
        };
        show(true).expect("shown again");
        assert!(by_role(Role::Inbox, Some(false), 50, None).messages.iter().any(|m| m.id == demo::HIDDEN_MAIL));
        assert!(by_role(Role::Inbox, Some(true), 50, None).messages.is_empty());
        assert_eq!(unread(None), unread_shown + 1, "the unread newsletter counts again");
        show(false).expect("undone");
        assert_eq!(by_role(Role::Inbox, Some(false), 50, None).messages, all);
    }

    /// Task E19.3: the demo's piles are listed a page at a time and counted, "All" and its number
    /// leave them out, putting mail on the other pile moves it and taking it off brings it back.
    #[test]
    fn the_demo_lists_and_counts_its_piles_and_all_leaves_them_out() {
        use iced::futures::executor::block_on;
        let demo = DemoSource::new(now());
        let pile = |pile, account: Option<&str>, limit, cursor: Option<String>| {
            let find =
                MailInPile { pile, account: account.map(str::to_owned), order: Order::NewestFirst, limit, cursor };
            block_on(MailSource::mail_in_pile(&demo, find)).expect("a page")
        };
        let ids = |page: MailPage| page.messages.into_iter().map(|message| message.id).collect::<Vec<_>>();
        let count = |which, account: Option<&str>| {
            block_on(MailSource::count_in_pile(&demo, which, account.map(str::to_owned))).expect("a number")
        };
        let all = || {
            let find = MailByRole {
                role: Role::Inbox,
                account: None,
                snoozed: Some(false),
                hidden: Some(false),
                pile: Some(PileFilter::Without),
                order: Order::NewestFirst,
                limit: 50,
                cursor: None,
            };
            ids(block_on(MailSource::mail_by_role(&demo, find)).expect("a page"))
        };
        let unread = || block_on(MailSource::unread_in_all(&demo, None)).expect("a number");

        let first = pile(MailPile::ReplyLater, None, 1, None);
        assert_eq!(first.next.as_deref(), Some("1"), "a page at a time");
        assert!(first.messages.iter().all(|message| message.pile == Some(MailPile::ReplyLater)), "named");
        assert_eq!(ids(first), ["m:w13"]);
        assert_eq!(ids(pile(MailPile::ReplyLater, None, 1, Some("1".into()))), ["m:p7"]);
        assert_eq!(ids(pile(MailPile::SetAside, Some(demo::MAIL_ACCOUNTS[0]), 50, None)), [demo::SET_ASIDE_MAIL]);
        assert!(ids(pile(MailPile::SetAside, Some(demo::MAIL_ACCOUNTS[1]), 50, None)).is_empty(), "one account");
        assert_eq!((count(MailPile::ReplyLater, None), count(MailPile::SetAside, None)), (2, 1));
        assert_eq!(count(MailPile::ReplyLater, Some(demo::MAIL_ACCOUNTS[1])), 1);
        let on_piles = [demo::REPLY_LATER_MAIL[0], demo::REPLY_LATER_MAIL[1], demo::SET_ASIDE_MAIL];
        assert!(!all().iter().any(|id| on_piles.contains(&id.as_str())), "not in All");
        let unread_before = unread();

        // Moved to the other pile, taken off: back in "All", its unread mail counted again.
        let put = |ids: &[&str], to| {
            block_on(MailSource::set_mail_pile(&demo, ids.iter().map(|id| (*id).to_owned()).collect(), to, None))
        };
        put(&["m:p7"], Some(MailPile::SetAside)).expect("moved");
        assert_eq!((count(MailPile::ReplyLater, None), count(MailPile::SetAside, None)), (1, 2));
        put(&["m:p7"], None).expect("taken off");
        assert!(all().iter().any(|id| id == "m:p7"), "in All again");
        assert_eq!(unread(), unread_before + 1, "Clara's unread mail counts again");
        put(&["m:w1"], Some(MailPile::ReplyLater)).expect("put on");
        assert!(!all().iter().any(|id| id == "m:w1"));
        assert_eq!(put(&["m:nope"], None), Err(InboxError::NotFound));
    }

    /// Task E19.3: "Focus & reply" sends what the compose window would – to the sender, from the
    /// account the mail came to, `Re:`, the text above the quote, formatted for the demo's accounts –
    /// and the demo takes the conversation off "Reply later" as the core does.
    #[test]
    fn the_demo_sends_a_reply_as_the_compose_window_builds_it() {
        use iced::futures::executor::block_on;
        let demo = DemoSource::new(now());
        block_on(MailSource::send_reply(&demo, "m:p7".into(), "  Yes, book it!\n".into())).expect("sent");
        let sent = demo.sent();
        assert_eq!(sent.len(), 1, "saved and sent, no draft left: {:?}", demo.drafts());
        let reply = &sent[0];
        assert_eq!(reply.account, demo::MAIL_ACCOUNTS[1], "from the account it came to");
        assert_eq!(reply.to.iter().map(|to| to.address.as_str()).collect::<Vec<_>>(), ["clara@example.org"]);
        assert_eq!(
            (reply.subject.as_str(), reply.in_reply_to.as_deref()),
            ("Re: Birthday dinner on the 24th", Some("m:p7"))
        );
        assert!(reply.body.starts_with("Yes, book it!\n"), "the text first: {}", reply.body);
        assert!(reply.body.contains("> Dinner for Mum’s birthday"), "then the quote: {}", reply.body);
        assert!(reply.html.is_some(), "the demo writes formatted mail");
        let left = block_on(MailSource::count_in_pile(&demo, MailPile::ReplyLater, None)).expect("a number");
        assert_eq!(left, 1, "answered, off the pile");
        assert_eq!(block_on(MailSource::send_reply(&demo, "m:nope".into(), "Hi".into())), Err(InboxError::NotFound));
    }

    /// Task E8.3b: the demo's two invitations say when their events are and are answered as sent,
    /// any other mail is none. (The smart view "Invitations" that listed them is gone with issue #337.)
    #[test]
    fn the_demos_invitations_say_when_and_are_answered() {
        let demo = DemoSource::new(now());
        let page = |folder: &str| {
            iced::futures::executor::block_on(MailSource::search_mail(
                &demo,
                folder.into(),
                None,
                50,
                None,
                Order::NewestFirst,
            ))
            .expect("a page")
            .messages
        };
        let folders = iced::futures::executor::block_on(MailSource::mail_folders(&demo)).expect("folders");
        let inboxes: Vec<_> =
            folders.iter().filter(|folder| folder.role == Some(Role::Inbox)).map(|f| page(&f.id)).collect();
        let mut found: Vec<MessageSummary> =
            inboxes.into_iter().flatten().filter(|message| message.invite.is_some()).collect();
        found.sort_by_key(|message| message.invite.as_ref().map(|invite| invite.start));
        let ids: Vec<&str> = found.iter().map(|message| message.id.as_str()).collect();
        assert_eq!(ids, ["m:p1", "m:w4"], "the hike this Saturday before lunch next week");
        let hike = found[0].invite.clone().expect("an invite");
        assert!(hike.all_day && !invite_when(&hike, now()).contains(':'), "a whole day has no time");
        let lunch = found[1].invite.clone().expect("an invite");
        assert!(invite_when(&lunch, now()).contains("12:00"), "{}", invite_when(&lunch, now()));
        assert!(invite_over(&lunch, lunch.end.expect("an end")) && !invite_over(&lunch, now()));
        assert!(
            found
                .iter()
                .filter_map(|message| message.invite.as_ref())
                .all(|invite| invite_over(invite, lunch.end.expect("an end")))
        );

        let answer = |id: &str| {
            iced::futures::executor::block_on(MailSource::respond_to_invitation(
                &demo,
                "mail:anna@work.example.org".into(),
                id.into(),
                InvitationAnswer::Accept,
                None,
            ))
        };
        assert_eq!(answer("m:w4"), Ok(Delivery::Accepted));
        assert!(matches!(answer("m:w1"), Err(InboxError::Other(_))), "no invitation");
        assert_eq!(answer("m:nope"), Err(InboxError::NotFound));
        let height = |invite| message_card_height(&MessageSummary { invite, ..found[1].clone() }, 480.0);
        assert!(height(Some(lunch.clone())) >= height(None), "the chips' row");
        let full = iced::futures::executor::block_on(MailSource::get_mail(&demo, "m:w4".into())).expect("the mail");
        assert!(full.invitation && full.summary.invite.is_some());
    }

    /// Task E8.3c: an invitation overlaps the busy events that share its time – not free ones, not
    /// what only touches it, not its own copy –; the line names the first with its time and counts
    /// the rest, or says "No conflicts". A whole day asks from local midnight to local midnight.
    #[test]
    fn an_invitation_names_what_it_overlaps() {
        let demo = DemoSource::new(now());
        let invite = |id: &str| {
            let mail = iced::futures::executor::block_on(MailSource::get_mail(&demo, id.into())).expect("the mail");
            mail.summary.invite.expect("an invitation")
        };
        let events = |(start, end)| {
            iced::futures::executor::block_on(MailSource::list_events(&demo, start, end)).expect("the events")
        };
        let lunch = invite("m:w4");
        let span = invite_span(&lunch);
        assert_eq!(span, (lunch.start, lunch.end.expect("an end")));
        let around = events(span);
        let titles: Vec<&str> = overlaps(&lunch, &around).iter().map(|event| event.title.as_str()).collect();
        assert_eq!(titles, [tr("Team sync")], "the free focus time before it is none");
        let sync = datetime::format_time(datetime::to_local(lunch.start + chrono::TimeDelta::minutes(30)).time());
        let one = trf("Overlaps with {title} at {time}", &[("title", &tr("Team sync")), ("time", &sync)]);
        assert_eq!(overlap_text(&lunch, &around, now()), one);

        let (start, end) = span;
        let copy = CalendarEvent { title: lunch.title.clone(), start, end, busy: true };
        let touching = CalendarEvent {
            title: "Before".into(),
            start: start - chrono::TimeDelta::hours(1),
            end: start,
            busy: true,
        };
        let late = CalendarEvent { title: "Call".into(), start: end - chrono::TimeDelta::minutes(10), end, busy: true };
        let more = [around.clone(), vec![copy, touching, late]].concat();
        let two = trf(
            "Overlaps with {title} at {time} and {n} more",
            &[("title", &tr("Team sync")), ("time", &sync), ("n", &1)],
        );
        assert_eq!(overlap_text(&lunch, &more, now()), two, "the copy and what only touches it are none");

        let hike = invite("m:p1");
        let (first, after) = invite_span(&hike);
        let day = datetime::to_local(first);
        assert_eq!(
            (day.time(), datetime::to_local(after).date()),
            (chrono::NaiveTime::MIN, day.date() + chrono::Days::new(1))
        );
        assert_eq!(overlap_text(&hike, &events((first, after)), now()), tr("No conflicts"));
        let morning = CalendarEvent {
            title: "Climbing".into(),
            start: first + chrono::TimeDelta::hours(9),
            end: after,
            busy: true,
        };
        let at = datetime::format_time(datetime::to_local(morning.start).time());
        let named = trf("Overlaps with {title} at {time}", &[("title", &"Climbing".to_owned()), ("time", &at)]);
        assert_eq!(overlap_text(&hike, &[morning], now()), named, "the hike's day has the time alone");
    }

    /// Task E8.3: a message's card answers and archives (`r`, `e`), and is as high as its parts.
    #[test]
    fn a_message_card_is_as_high_as_what_it_shows() {
        let mut message = demo::mail_sample(now()).remove(0).summary;
        message.has_attachments = false;
        message.flags.clear();
        message.snippet = "Short".into();
        let plain = message_card_height(&message, 480.0);
        message.has_attachments = true;
        let row = size::card_gap() + size::control_sm();
        assert_eq!(message_card_height(&message, 480.0), plain + row, "the chips get a row of their own");
        message.has_attachments = false;
        message.snippet.clear();
        assert!(message_card_height(&message, 480.0) < plain, "no preview, no preview lines");
    }

    /// A mail's `cid:` pictures come from its own parts with that Content-ID, through
    /// `mail.get_attachment` (task M9.7); other ids and other mails give nothing.
    #[test]
    fn pictures_come_from_the_parts_of_this_mail() {
        let demo = Arc::new(DemoSource::new(now()));
        let sale = DemoSource::get_mail(&demo, "m:p2").expect("the newsletter");
        let pictures = MailPictures::of(&sale, &demo).expect("parts with a Content-ID");
        let logo = pictures.picture("logo@shop.example").expect("the logo");
        assert!(logo.starts_with(b"\x89PNG"), "the part's bytes");
        assert_eq!(pictures.picture("logo@other.example"), None, "not a part of this mail");
        let plain = DemoSource::get_mail(&demo, "m:w1").expect("a text mail");
        assert!(MailPictures::of(&plain, &demo).is_none(), "no part with a Content-ID");
        assert!(HtmlMail::new(&plain, &demo, true).is_none(), "no HTML, nothing to parse");
        let parsed = HtmlMail::new(&sale, &demo, true).expect("parsed");
        assert_eq!(parsed.id, "m:p2");
        assert!(parsed.content.height(480.0, 1.0, Scheme::Light) > 0.0, "it lays out");
    }

    /// Review of M9.7: a part larger than a picture may be is not read at all.
    #[test]
    fn a_picture_file_over_the_limit_is_not_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let small = dir.path().join("small.png");
        std::fs::write(&small, b"\x89PNG").expect("write");
        assert_eq!(read_picture(&small).as_deref(), Some(b"\x89PNG".as_slice()));
        let edge = dir.path().join("edge.png");
        std::fs::File::create(&edge).and_then(|file| file.set_len(MAX_PICTURE_BYTES)).expect("sparse file");
        assert_eq!(read_picture(&edge).map(|bytes| bytes.len() as u64), Some(MAX_PICTURE_BYTES), "the limit itself");
        let big = dir.path().join("big.png");
        std::fs::File::create(&big).and_then(|file| file.set_len(MAX_PICTURE_BYTES + 1)).expect("sparse file");
        assert_eq!(read_picture(&big), None);
        assert_eq!(read_picture(&dir.path().join("missing.png")), None);
    }

    #[test]
    fn only_web_and_mail_links_open_and_like_the_app_follows_the_window() {
        for url in ["https://shop.example", "http://shop.example", "HTTPS://shop.example", "mailto:a@b.example"] {
            assert!(opens_in_browser(url), "{url}");
        }
        for url in ["javascript:alert(1)", "file:///etc/passwd", "cid:logo@shop.example", "shop.example", ""] {
            assert!(!opens_in_browser(url), "{url}");
        }
        assert_eq!(scheme_for(MailAppearance::App, true), Scheme::Dark);
        assert_eq!(scheme_for(MailAppearance::App, false), Scheme::Light);
        assert_eq!(scheme_for(MailAppearance::Light, true), Scheme::Light);
        assert_eq!(scheme_for(MailAppearance::Dark, false), Scheme::Dark);
        assert_eq!(
            MailLook::default(),
            MailLook { format: MailFormat::Html, colors: MailAppearance::App, ..MailLook::default() }
        );
    }
}
