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
//!   window (task E8.3, [`message_card`]) – and the smart view "With attachments" ([`attachments`]:
//!   the messages with attachments of every account's inbox). [`body`] is the whole mail for the
//!   panel, [`reader`] the reader beside the list in the wide mail window (task E8.3), the text in
//!   the reading level (`typo::read()`); [`empty_reader`] keeps its column while no mail is open
//!   (task E8.11). The panel splits the buttons (task E9.4): the mail standards at the top – Reply ·
//!   Reply all · Forward | Archive · Delete, Flag, read –, what Torro adds in its fixed footer. The
//!   reader has none of the red (task E11.3): a command bar at the top ([`commands`]: Reply ▾ ·
//!   Forward · Archive · Delete · Later · "…", the rest in [`reply_menu`] and [`more_menu`]), Torro's
//!   functions as pills under the sender ([`pills`]: Task · AI task · Event · AI event · Mute…), the
//!   attachments as chips ([`attachment_line`]) and the view's switches in a foot ([`foot`]). Which
//!   bars a mail in the panel has, [`Bars`] says (task E11.4): the hub and the App Feed keep the
//!   split of E9.4, the mail window's panel in medium and narrow takes the reader's parts – the
//!   command bar, the head without the subject (the panel's title says it), the pills, and the
//!   chips and switches in its fixed footer ([`panel_foot`]); [`entry_panel`] is that panel for a
//!   mail entry.
//! - The task made of a mail (task E8.7b) stands over its text as a card ([`task_card`], "Open
//!   task"); "Task" on a folder's message makes one ([`MailSource::create_todo_from_mail`]).
//!
//! **Mail is foreign text** (`untrusted`). Its HTML comes cleaned by the core (no scripts, no
//! forms, legacy attributes as CSS) and is drawn without JavaScript; a click on one of its links
//! only reports the address, the app opens `http`, `https` and `mailto` in the browser. The text is drawn as it is;
//! the text can be selected and copied (issue #119, `torro_gui::selectable`, "Copy text"), and the web
//! addresses in it are listed under it as "Links" that open only on a click. An attachment is fetched by the core into its attachment folder and opened with the
//! system, except files that could run programs ([`runs_programs`]): their folder opens instead.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use chrono::{DateTime, Local, NaiveDate, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, responsive, row, rule, scrollable, svg};
use iced::{Alignment, Color, Element, Fill, Padding, Theme};
use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::{MailAppearance, MailFormat};
use torro_gui::icon::{self, Icon};
use torro_gui::layout;
use torro_gui::panel;
use torro_gui::selectable::{self, Selectable};
use torro_gui::source;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Mark, MenuItem, MenuSection, RowAction, Trail};
use torro_gui::{datetime, style, typo};
use torro_htmlview::{CidSource, Content, HtmlView, MAX_PICTURE_BYTES, Scheme};
use torro_inbox::InboxError;
use torro_inbox::demo::DemoSource;
use torro_inbox::item::{ItemTask, TaskDue};
use torro_inbox::source::TaskList;
use torro_inbox::{Item, Lane, Link, State};
use torro_ipc::inbox::CategoryInfo;
use torro_ipc::inbox::{CalendarEvent, InboxClient, MailFind, MailFound};
#[cfg(test)]
use torro_ipc::mail::MailSort;
use torro_ipc::mail::{Delivery, MailHighlight, MailIndexProgress, MoveRole, MoveTarget};
use torro_ipc::{Request, Response};
use torro_mail::{
    AttachmentInfo, CategoryGroup, Flag, Folder, InvitationAnswer, Invite, MailAddress, MailCategory, Message as Mail,
    MessageSummary, Order, Role,
};

use crate::app::{INBOX, Message, View};
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
    /// one holds (task E12.8). The sidebar builds its group "Categories" from it.
    fn list_categories(&self) -> impl Future<Output = Result<Vec<CategoryInfo>, InboxError>> + Send;

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

    async fn list_categories(&self) -> Result<Vec<CategoryInfo>, InboxError> {
        InboxClient::list_categories(self).await
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

    async fn list_categories(&self) -> Result<Vec<CategoryInfo>, InboxError> {
        Ok(DemoSource::category_counts(self)
            .into_iter()
            .map(|(category, unread)| CategoryInfo {
                category,
                group: category.group(),
                label: torro_core::category::label(category).to_string(),
                group_label: torro_core::category::group_label(category.group()).to_string(),
                unread,
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

    async fn get_mail(&self, id: String) -> Result<Mail, InboxError> {
        let mut mail = DemoSource::get_mail(self, &id)?;
        mail.summary.invite = demo_invite(&mail.summary.id, mail.summary.date);
        mail.invitation = mail.summary.invite.is_some();
        Ok(mail)
    }

    async fn mail_attachment(&self, id: String, part: String) -> Result<PathBuf, InboxError> {
        DemoSource::mail_attachment(self, &id, &part)
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

    async fn list_categories(&self) -> Result<Vec<CategoryInfo>, InboxError> {
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
}

/// What the sidebar's group "Categories" shows (task E12.8): a whole group of mail categories, or
/// one category of it. Both list the mail of every account's inbox that is of one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryView {
    /// The group, which gives the entry its icon and its colour.
    pub group: CategoryGroup,
    /// One category of the group, or `None` for the whole group.
    pub only: Option<torro_mail::Category>,
}

impl CategoryView {
    /// The categories this view lists: the one chosen, else every one of the group.
    pub fn categories(&self) -> Vec<torro_mail::Category> {
        match self.only {
            Some(category) => vec![category],
            None => self.group.categories(),
        }
    }

    /// What a person reads for it.
    pub fn label(&self) -> String {
        match self.only {
            Some(category) => torro_core::category::label(category).to_string(),
            None => torro_core::category::group_label(self.group).to_string(),
        }
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

/// How the window shows mail, as the settings `mail_view` and `mail_appearance` say (task M9.5).
/// The switches over a mail start from it and go back to it whenever the setting changes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailLook {
    /// HTML or text first (`mail_view`); a mail without HTML always shows its text.
    pub format: MailFormat,
    /// The colours of the mail's content (`mail_appearance`); [`scheme_for`] resolves `app`.
    pub colors: MailAppearance,
    /// The categories the sidebar leaves out (task E12.10, `hidden_categories`): what the person
    /// switched off under Settings › Categorization.
    pub hidden_categories: Vec<torro_mail::Category>,
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
/// tooltip: `"E"` in a folder, where `e` archives, `""` at an entry, where `e` is Done.
pub fn filing_actions<'a>(
    folders: &[Folder],
    account: &str,
    folder: Option<&str>,
    id: &str,
    archive_key: &'static str,
) -> Vec<RowAction<'a, Message>> {
    let role_of = |folder: &str| folders.iter().find(|f| f.id == folder).and_then(|f| f.role);
    let lies_in = folder.map_or(Some(Role::Inbox), role_of);
    let has = |role: Role| folders.iter().any(|f| f.account == account && f.role == Some(role));
    let mut actions = Vec::new();
    if has(Role::Archive) && lies_in != Some(Role::Archive) {
        let archive = Message::Mail(MailAction::Archive, id.to_owned());
        actions.push(RowAction::new(Icon::Archive, trc("action", "Archive"), archive_key, archive));
    }
    if has(Role::Trash) && lies_in != Some(Role::Trash) {
        actions.push(RowAction::new(Icon::Trash2, tr("Delete"), "#", Message::Mail(MailAction::Delete, id.to_owned())));
    }
    actions
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
    /// The view shown: a folder, the smart view of attachments, or a window of mail waiting for
    /// its folders ([`View::Mail`]).
    pub view: &'a View,
    /// The mail accounts have no folders at all (no account connected).
    pub no_accounts: bool,
    /// Id of the message under the cursor.
    pub selected: Option<&'a str>,
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
    /// The strip of the selected row (task E11.2, as `list::Rows::edge_strip`): `true` in the mail
    /// window – flat cells from edge to edge below the lines (`list::with_row_strip`), Archive ·
    /// Delete · Task · Mute… (`list::edge_actions`) –, else the strip of buttons where the text starts.
    pub edge_strip: bool,
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
            View::Attachments => {
                (Icon::Paperclip, tr("No attachments."), tr("New mail with attachments shows up here."))
            }
            View::Invitations => {
                (Icon::Calendar, tr("No invitations."), tr("Invitations to events you can answer show up here."))
            }
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
/// (issue #118) hangs below its lines, as an entry's row – in the mail window (task E11.2,
/// [`MailList::edge_strip`]) as the strip from edge to edge (`list::with_row_strip`, in the order of
/// `list::edge_actions`) – and holds what its card offers ([`message_buttons`]), the bell last.
fn message_row<'a>(data: &MailList<'a>, message: &'a MessageSummary, width: f32) -> Element<'a, Message> {
    let mut marks: Vec<(Element<'a, Message>, f32)> = Vec::new();
    // What the mail is (task E12.8), before the other marks: it says most about the row.
    marks.extend(message.category.map(category_mark));
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
    let selected = data.selected == Some(message.id.as_str());
    let on_press = Message::SelectMail(message.id.clone());
    let mut height = list::message_height();
    let mut actions = Vec::new();
    if selected {
        let (buttons, more) = message_buttons(message, data.entries, data.folders, data.now);
        actions = data.anchors.mark(true, buttons);
        actions.extend(data.anchors.mark(true, more.into_iter().collect()));
    }
    // Below the lines, as the rows of entries (`list::entry_row`): nothing covers the row.
    if !actions.is_empty() {
        height += list::strip_height_in(data.edge_strip);
    }
    let content = list::mail_content(lines, width);
    let row = if data.edge_strip {
        let strip = list::with_row_strip(list::edge_actions(actions), width);
        list::strip_row(content, strip, selected, on_press, height)
    } else {
        list::sized_row(list::with_strip(content, actions, width), selected, on_press, height)
    };
    if selected { widgets::anchored(&data.anchors.row, row) } else { row }
}

/// The card of a message `width` wide (task E8.3, `list::mail_card`): the lines of its row, as chips
/// "Attachments" and "Flagged", and its buttons ([`message_buttons`]). An invitation (task E8.3b)
/// shows when it is and the answer given as chips. In the smart views, which hold every account's
/// mail, the head names the account.
fn message_card<'a>(data: &MailList<'a>, message: &'a MessageSummary, width: f32) -> Element<'a, Message> {
    let mut chips = Vec::new();
    // What the mail is (task E12.8): the whole pill, where the card has room for it.
    chips.extend(message.category.map(category_pill));
    if let Some(invite) = &message.invite {
        chips.extend(invite_chips(invite, data.now));
    }
    if message.has_attachments {
        chips.push(widgets::state_pill(Icon::Paperclip, tr("Attachments")));
    }
    if message.flags.contains(&Flag::Flagged) {
        chips.push(widgets::state_pill(Mark::Tinted(Icon::Flag, data.attention), tr("Flagged")));
    }
    let (buttons, more) = message_buttons(message, data.entries, data.folders, data.now);
    let (name, domain) = sender_parts(message.from.as_ref());
    let account = data.accounts.iter().position(|account| *account == address(&message.account));
    let smart = matches!(data.view, View::Attachments | View::Invitations | View::Search(_));
    let several = smart && data.accounts.len() > 1;
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
    let selected = data.selected == Some(message.id.as_str());
    let buttons = data.anchors.mark(selected, buttons);
    let more = data.anchors.mark(selected, more.into_iter().collect()).pop();
    let card =
        list::MailCard { lines, chips, buttons, more, selected, on_press: Message::SelectMail(message.id.clone()) };
    let card = list::mail_card(card, width);
    if selected { widgets::anchored(&data.anchors.row, card) } else { card }
}

/// The buttons of the card (and the action bar of the row) of `message` at `now`, with the bell
/// "Mute…" (`m`, issue #123) as the second part:
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

/// The smart view "With attachments" (task E8.3): of the messages of every account's inbox, those
/// with attachments, in `order` by date (task E8.15). `pages` are the first pages of the inboxes
/// as `mail.search_messages` answered them in that order; nothing new is asked of the core.
pub fn attachments(pages: Vec<Vec<MessageSummary>>, order: Order) -> Vec<MessageSummary> {
    let mut found: Vec<MessageSummary> =
        pages.into_iter().flatten().filter(|message| message.has_attachments).collect();
    found.sort_by(|a, b| order.apply(b.date.cmp(&a.date)));
    found
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

/// The smart view "Invitations" (task E8.3b): of the messages of every account's inbox, those that
/// invite to an event that is not over at `now`, the soonest first. `pages` are the first pages of
/// the inboxes as `mail.search_messages` answered them; nothing new is asked of the core.
pub fn invitations(pages: Vec<Vec<MessageSummary>>, now: DateTime<Utc>) -> Vec<MessageSummary> {
    let mut found: Vec<MessageSummary> = pages
        .into_iter()
        .flatten()
        .filter(|message| message.invite.as_ref().is_some_and(|invite| !invite_over(invite, now)))
        .collect();
    found.sort_by_key(|message| message.invite.as_ref().map(|invite| invite.start));
    found
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

/// The icon and the place in `Palette::category` of a category's group (task E12.8): one icon
/// per group, never per category – 23 would not be told apart (`docs/kategorien.md`). The icons
/// are deliberately not the folder icons (`folder_icon`): "Inbox" the folder and "Waiting for
/// me" the group stand in the same sidebar.
pub(crate) fn group_mark(group: CategoryGroup) -> (Icon, usize) {
    match group {
        CategoryGroup::WaitingForMe => (Icon::UserCheck, 0),
        CategoryGroup::PaperTrail => (Icon::TextQuote, 1),
        CategoryGroup::Info => (Icon::Info, 2),
        CategoryGroup::Junk => (Icon::ShieldAlert, 3),
    }
}

/// What a person reads for `held` beside the pill: the category, and that a model guessed it.
pub(crate) fn category_words(held: MailCategory) -> String {
    let label = torro_core::category::label(held.category);
    if held.is_suggestion() { trf("{category} · suggested", &[("category", &label)]) } else { label.to_string() }
}

/// The small mark of a mail's category for a row (task E12.8): the group's icon in its colour,
/// the words in the tooltip. The whole pill goes where there is room – the reader and the card.
pub(crate) fn category_mark<'a>(held: MailCategory) -> (Element<'a, Message>, f32) {
    let (mark, group) = group_mark(held.category.group());
    let pill = widgets::category_mark(mark, group, icon::SIZE_SM);
    (widgets::with_tooltip(pill, category_words(held)), icon::SIZE_SM)
}

/// The whole pill of a mail's category, for the reader and the card (task E12.8).
pub(crate) fn category_pill<'a>(held: MailCategory) -> Element<'a, Message> {
    let (mark, group) = group_mark(held.category.group());
    widgets::category_pill(mark, group, torro_core::category::label(held.category), held.is_suggestion())
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

/// A small icon (14) in `text_3`, or in `colour`.
fn small_icon<'a>(which: Icon, colour: Option<Color>) -> Element<'a, Message> {
    icon::icon(which, icon::SIZE_SM)
        .style(move |t: &Theme, _| svg::Style { color: Some(colour.unwrap_or(palette(t).text_3)) })
        .into()
}

/// What the panel shows of a mail.
#[derive(Debug, Clone, Copy)]
pub struct Reading<'a> {
    /// The message's id.
    pub id: &'a str,
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

    /// What the panel's footer (task E9.4) and the reader's foot (task E11.3, [`foot`]) show: for a loaded
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
    /// Task E11, for the mail window in medium and narrow: the reader's parts in the panel – the
    /// command bar ([`commands`], without Close: the ✕ stands in the panel's head), the sender and
    /// the pills ([`head`] without the subject, which the panel's title says), a line, the mail; the
    /// attachments as chips and the view's switches in the panel's footer ([`panel_foot`]).
    Command {
        /// The mail entry the mail belongs to (Later, the entry's pills, Done and "Move to …" in
        /// "…"); `None` for a folder's message.
        entry: Option<ReaderEntry<'a>>,
        /// How wide the panel is: [`size::panel()`] over the medium window, the whole window in a
        /// narrow one. The command bar's words ([`widgets::command_words`]) and what ends in "…"
        /// reckon with it.
        width: f32,
    },
}

/// The panel of a folder's message: the account and folder as path, the subject as title, the
/// mail as body ([`body`]) with the `bars` it says. With [`Bars::Split`] the mail standards stand
/// over the mail and the Torro functions in the panel's fixed footer (task E9.4, [`folder_footer`]):
/// Task, then the bell "Mute…" and the view's switches and "Copy text"; with [`Bars::Command`] the
/// reader's bars (task E11.4, [`panel_foot`] the footer). `anchors` are where the menus of its
/// buttons open (the app's window, issue #122).
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
        Bars::Command { width, .. } => panel_foot(reading, width),
    };
    let body = body(reading, bars, anchors);
    let panel = panel::panel(panel::path(mark, parts), panel::title(reading.title()), body, Message::CloseDetail);
    match bars {
        Bars::Split => panel.footer(footer),
        Bars::Command { entry, width } => panel.bar(panel_bar(reading, entry, anchors, width)).footer(footer),
    }
}

/// The panel of a mail entry in the mail window's medium and narrow layouts (task E11.4): the
/// entry's head as `detail::panel` draws it – source and `view_name` as path, the entry's title, ✕
/// – and its whole mail with the reader's bars ([`Bars::Command`] with `entry`, the panel `width`
/// wide) – the command bar fixed under the head ([`panel_bar`]) –, the entry's properties under the
/// sender ([`command_body`]), the attachments and the view's switches in the fixed footer
/// ([`panel_foot`]). The hub and the App Feed keep `detail::panel` with [`Bars::Split`].
pub fn entry_panel<'a>(
    reading: &Reading<'a>,
    entry: ReaderEntry<'a>,
    view_name: String,
    anchors: &detail::Anchors,
    width: f32,
) -> panel::Panel<'a, Message> {
    let body = body(reading, Bars::Command { entry: Some(entry), width }, anchors);
    let (path, title) = (detail::path(entry.item, view_name), detail::heading(entry.item));
    panel::panel(path, title, body, Message::CloseDetail)
        .bar(panel_bar(reading, Some(entry), anchors, width))
        .footer(panel_foot(reading, width))
}

/// The command bar of the mail window's panel (task E11.4, [`commands`], whose "…" has no Close in
/// the panel), fixed under the panel's head from edge to edge (`Panel::bar`): it does not scroll
/// away with the mail. A line above it parts it from the head; the bar draws the one below.
fn panel_bar<'a>(
    reading: &Reading<'a>,
    entry: Option<ReaderEntry<'a>>,
    anchors: &detail::Anchors,
    width: f32,
) -> Element<'a, Message> {
    let line = rule::horizontal(size::HAIRLINE).style(style::rule);
    column![line, commands(reading, entry.map(|entry| entry.item), anchors, width)].into()
}

/// The fixed footer of a mail's panel with the reader's bars (task E11.4, [`Bars::Command`]): the
/// mail's attachments as chips (as [`attachment_line`] has them over the reader's foot), under them
/// the view's switches and "Copy text" (as the reader's [`foot`] has them), each part only where the
/// mail has it, in the panel's padding – the panel draws the line above it. Empty – no footer –
/// while the mail loads and after an error. `width` is the panel's.
pub fn panel_foot<'a>(reading: &Reading<'_>, width: f32) -> Vec<Element<'a, Message>> {
    // The panel's footer pads its content by `space::l()` at each side.
    let room = (width - 2.0 * space::l()).max(0.0);
    let mut parts: Vec<Element<'a, Message>> = Vec::new();
    parts.extend(attachment_chips(reading, room));
    let controls = foot_controls(reading.view_tools());
    if !controls.is_empty() {
        parts.push(row(controls).spacing(space::s()).align_y(Alignment::Center).into());
    }
    if parts.is_empty() {
        return Vec::new();
    }
    vec![column(parts).spacing(space::s()).width(Fill).into()]
}

/// The footer of a folder's message in the panel (task E9.4), in the panel's style and in the
/// order of the reader's former Torro bar: "Task" (task E8.7b), a line, then the bell
/// "Mute…" (issue #123: the message's own traits) and the view group ([`view_controls`]) – one row
/// that wraps in a narrow panel. "Later" joins "Task" with task E9.2; the AI buttons need an entry
/// (the core suggests from entries only), so a folder's message has none.
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

/// What the view group of the panel's footer (task E9.4) and the reader's foot (task E11.3) shows
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

/// The mail in the panel with the `bars` the window chose ([`Bars`]); `anchors` are where the menus
/// of its buttons open (with [`Bars::Command`]: Reply ▾ `reply`, Later `snooze`, "…" `more`, the
/// pills `task`, `event`, `hide`).
///
/// - [`Bars::Split`] (task E9.4): who wrote it to whom and when, the mail standards
///   ([`panel_standards`]: reply, reply all, forward, archive, delete, flag, read), the attachments,
///   the HTML or the text with its links, and the note that the mail comes from outside. The Torro
///   functions, the switches "HTML | Text" and "Light | Dark" (task M9.7) and "Copy text" stand in
///   the panel's footer: an entry's (`detail::Footer`) or a folder's message's ([`panel()`]).
/// - [`Bars::Command`] (task E11.4, [`command_body`]): the sender, the pills, invitation and task,
///   an entry's properties, a line, the mail. The command bar stands fixed above it (`Panel::bar`,
///   [`panel_bar`]); attachments and the view's switches in the footer ([`panel_foot`]).
pub fn body<'a>(reading: &Reading<'a>, bars: Bars<'a>, anchors: &detail::Anchors) -> Element<'a, Message> {
    match bars {
        Bars::Split => split_body(reading),
        Bars::Command { entry, width } => command_body(reading, entry, anchors, width),
    }
}

/// The body of the mail window's panel (task E11.4, [`Bars::Command`]), `width` wide, in the order
/// of the reader (task E11.3) under the command bar, which stands fixed above it ([`panel_bar`]):
/// the head without the subject – the panel's title says it – ([`head_with`]: sender, why a rule put
/// the entry in its lane, the pills, invitation and task) lined up with the title, an entry's
/// properties, a line from edge to edge and the mail ([`panel_letter`]).
fn command_body<'a>(
    reading: &Reading<'a>,
    entry: Option<ReaderEntry<'a>>,
    anchors: &detail::Anchors,
    width: f32,
) -> Element<'a, Message> {
    let line = || rule::horizontal(size::HAIRLINE).style(style::rule);
    // The head lines up with the title: 16 from the panel's edges (`panel::inset`).
    let room = (width - 2.0 * space::l()).max(0.0);
    let head = head_with(reading, entry, anchors, room, false);
    let mut body = column![panel::inset(head)];
    // An entry keeps its properties (status, lane, source, time), as `detail::panel` shows them.
    if let Some(entry) = entry {
        body = body.push(widgets::prop_section(tr("Properties"), detail::properties(entry.item, reading.now)));
    }
    body.push(panel::bleed(line())).extend(panel_letter(reading)).spacing(space::l()).into()
}

/// The body of the panel with the split of task E9.4 ([`Bars::Split`], see [`body`]).
fn split_body<'a>(reading: &Reading<'a>) -> Element<'a, Message> {
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
    // The attachments as rows over the mail; the reader's bars have them as chips in the footer.
    if let Some(Ok(mail)) = reading.loaded {
        let files: Vec<&AttachmentInfo> = mail.attachments.iter().filter(|a| !a.inline).collect();
        if !files.is_empty() {
            let rows = files.into_iter().map(|file| attachment_row(reading.id, file)).collect();
            body = body.push(widgets::prop_section(tr("Attachments"), rows));
        }
    }
    body.extend(panel_letter(reading)).into()
}

/// The mail itself in the panel's body, both [`Bars`] alike: while it loads or after an error a
/// quiet line; once in, the HTML from edge to edge of the panel with the note under it, or the text
/// lined up with the title with the note on foreign text and its "Links".
fn panel_letter<'a>(reading: &Reading<'a>) -> Vec<Element<'a, Message>> {
    match reading.loaded {
        None => vec![panel::inset(note(tr("Loading the message…")))],
        Some(Err(why)) => {
            let what = typo::text(typo::body(), tr("Couldn't load the message.")).style(theme::text_2);
            let why = typo::text(typo::meta(), why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
            vec![panel::inset(column![what, why].spacing(space::xs()))]
        }
        Some(Ok(_)) => {
            let mut parts = vec![match reading.html.filter(HtmlReading::shown) {
                // The HTML reaches the panel's edges (its background with it), the note keeps the inset.
                // A mail without a background of its own starts its text in line with the title, 16
                // from the panel's edge (`panel::inset`).
                Some(html) => column![panel::bleed(html_view(html).margin(space::l())), panel::inset(html_note())]
                    .spacing(space::m())
                    .into(),
                None => panel::inset(text_block(reading.text, text_level(reading.read))),
            }];
            parts.extend(text_links(reading.text).filter(|_| !html_shown(reading)));
            parts
        }
    }
}

/// The level of a mail's text: the reading level in the comfortable mail window, else `BODY`.
fn text_level(read: bool) -> typo::Level {
    if read { typo::read() } else { typo::body() }
}

/// The entry a mail in the reader belongs to (task E8.3): Later in the command bar, the pills Task,
/// AI task, Event, AI event and Mute…, and Done and "Move to …" in "…" act on it, as `s`, `t`, `c`,
/// `m`, `e` and `l` do (task E11.3).
#[derive(Debug, Clone, Copy)]
pub struct ReaderEntry<'a> {
    pub item: &'a Item,
    /// The entry can be muted (`App::hidable`): the reader shows its pill "Mute…" (issue #123).
    pub hide: bool,
    /// An AI is set up (issue #120): the pills "AI task" and "AI event" are on; else off, with a
    /// tooltip that says to set one up first.
    pub ai: bool,
}

/// The reader of the wide mail window (task E8.3) beside the list, in four parts from top to bottom
/// (task E11.3, design D of `docs/prototypes/inbox/mail-buttons.html`):
///
/// 1. fixed, the **command bar** ([`commands`]): Reply with its menu "Reply all", Forward, Archive,
///    Delete, for an entry Later, and "…" ([`more_menu`]);
/// 2. scrolling, the **head** ([`head`]: subject, sender, why a rule put the entry in its lane, the
///    pills, invitation and task), a line over the reader's whole width and the **mail**
///    ([`reader_mail`]): a text in the page's padding, HTML from edge to edge ([`reader_widths`]);
/// 3. fixed, the mail's **attachments** as chips ([`attachment_line`]);
/// 4. fixed, the **foot** with the view's switches and "Copy text" ([`foot`]).
///
/// The window has no red button any more: Reply is the bar's strong cell, Torro's own buttons are
/// pills. `entry` is the mail entry the mail belongs to, `None` for a folder's message; `anchors`
/// are where the menus of its buttons open (the app's window). Close stands in "…" (Escape does
/// the same).
pub fn reader<'a>(
    reading: Reading<'a>,
    entry: Option<ReaderEntry<'a>>,
    anchors: detail::Anchors,
) -> Element<'a, Message> {
    responsive(move |bounds| {
        let widths = reader_widths(bounds.width, html_edge(&reading).is_some());
        let side = layout::page_padding().left;
        let head = container(head(&reading, entry, &anchors, widths.head)).width(Fill).padding(head_padding());
        let page = column![head, rule::horizontal(size::HAIRLINE).style(style::rule), reader_mail(&reading, widths)];
        let page = scrollable(page)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill);
        column![commands(&reading, entry.map(|entry| entry.item), &anchors, bounds.width), page]
            .push(attachment_line(&reading, widths.head, side))
            .push(foot(reading.view_tools(), side))
            .into()
    })
    .into()
}

/// The padding around the reader's head: the page's, a little less below, where the line under the
/// head follows.
fn head_padding() -> Padding {
    Padding { bottom: space::l(), ..layout::page_padding() }
}

/// How wide the parts of the reader are ([`reader_widths`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ReaderWidths {
    /// The head – subject, sender, pills – and what stands under the mail (the note, the
    /// attachments): inside the page's padding.
    head: f32,
    /// The mail's content: a text as wide as the head; HTML from edge to edge of the reader.
    mail: f32,
}

/// The widths in a reader `width` wide, for a mail shown as HTML (`html`) or as text, like a mail
/// app: no reading width as an upper bound anywhere – a reader that just stopped short of its
/// right edge looks odd. The head, a text and what stands under the mail use the whole width
/// inside the page's padding; HTML lies over the whole reader without padding (a newsletter centres
/// itself, a mail without a background of its own gets the padding as its margin,
/// [`HtmlView::margin`]).
fn reader_widths(width: f32, html: bool) -> ReaderWidths {
    let pad = layout::page_padding();
    let inner = (width - pad.left - pad.right).max(0.0);
    ReaderWidths { head: inner, mail: if html { width.max(0.0) } else { inner } }
}

/// The mail's HTML when it is loaded and shows as HTML: then the reader draws it edge to edge.
fn html_edge<'a>(reading: &Reading<'a>) -> Option<HtmlReading<'a>> {
    reading.html.filter(|html| html.shown() && matches!(reading.loaded, Some(Ok(_))))
}

/// The mail under the reader's head and its line (task E11.3), its parts as wide as `widths` says:
/// a text in the page's padding with its links and the note on foreign text; HTML from edge to edge
/// – a mail without a background of its own gets the padding as its margin – with the note under
/// it. While it loads, or after an error, a quiet line says so.
fn reader_mail<'a>(reading: &Reading<'a>, widths: ReaderWidths) -> Element<'a, Message> {
    let pad = layout::page_padding();
    match reading.loaded {
        None => container(note(tr("Loading the message…"))).padding(pad).into(),
        Some(Err(why)) => {
            let what = typo::text(typo::body(), tr("Couldn't load the message.")).style(theme::text_2);
            let why = typo::text(typo::meta(), why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
            container(column![what, why].spacing(space::xs())).padding(pad).into()
        }
        Some(Ok(_)) => {
            if let Some(html) = html_edge(reading) {
                // Below the line as far as a text starts: the view keeps the browser's 8 above.
                let top = Padding { top: space::l(), ..Padding::ZERO };
                let mail = container(html_view(html).margin(pad.left)).width(widths.mail).padding(top);
                let note =
                    container(container(html_note()).width(widths.head)).padding(Padding { top: space::m(), ..pad });
                return column![mail, note].into();
            }
            let mut text =
                column![text_block(reading.text, text_level(reading.read))].spacing(space::l()).width(widths.head);
            if let Some(links) = text_links(reading.text) {
                text = text.push(links);
            }
            container(text).padding(pad).into()
        }
    }
}

/// The head of a mail in the reader (task E11.3), `room` wide: the subject (`typo::headline()`), the
/// sender ([`sender_block`]), for an entry why a rule put it in its lane (issue #128), the pills
/// ([`pill_row`] of [`pills`]) and under them the invitation's card (task E8.3b) and the task's
/// (task E8.7b). Everything stands left-aligned, one under the other; while only the mail's id is
/// known, just the pills. The panel of the mail window takes the same head without the subject
/// (task E11.4, [`head_with`]).
pub fn head<'a>(
    reading: &Reading<'a>,
    entry: Option<ReaderEntry<'a>>,
    anchors: &detail::Anchors,
    room: f32,
) -> Element<'a, Message> {
    head_with(reading, entry, anchors, room, true)
}

/// The [`head`], with the subject on top only with `subject`: the panel of the mail window (task
/// E11.4) has it as its title already and leaves it out here.
fn head_with<'a>(
    reading: &Reading<'a>,
    entry: Option<ReaderEntry<'a>>,
    anchors: &detail::Anchors,
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
            head = head.push(widgets::chip_row(vec![category_pill(held)]));
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
    head = head.push(pill_row(pills(reading, entry), anchors));
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

/// Who wrote the mail, in the reader's head `room` wide (task E11.3): the avatar ([`size::pill()`])
/// and beside it the name – with the flag of a flagged mail – and the address quietly after it, under
/// them "to … · account · when" and, once the mail is loaded, "cc …" in `typo::meta()`/`text_2` –
/// left-aligned, no date at the right. What does not fit ends in "…".
fn sender_block<'a>(reading: &Reading<'a>, summary: &'a MessageSummary, room: f32) -> Element<'a, Message> {
    let name = sender_name(summary.from.as_ref());
    let edge = size::pill();
    let face = list::avatar(&name, reading.account, false, edge);
    let words_room = (room - edge - space::m()).max(0.0);
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
    row![face, words].spacing(space::m()).align_y(Alignment::Center).into()
}

/// A pill of the reader's head ([`pills`], drawn by `widgets::pill_button` in [`pill_row`]).
#[derive(Debug, Clone)]
pub struct Pill {
    pub icon: Icon,
    pub label: String,
    /// What a click sends; `None` shows the pill off.
    pub message: Option<Message>,
    /// Why it is off – an AI pill without an AI (issue #120) –, as its tooltip; `None` while it is
    /// on.
    pub tip: Option<String>,
}

/// What Torro adds to the mail in `reading` (task E11.3), as pills in this order: for an `entry`
/// Task (`AskTask`), AI task, Event (`AskEvent`), AI event (`suggest::ai_buttons`: off without an
/// AI, the tooltip says why) and – where it can be muted ([`ReaderEntry::hide`]) – Mute…
/// (`AskHide`); for a folder's message Task (`AskMailTask`, task E8.7b) and Mute… (`AskMuteMail`,
/// issue #123) – the core suggests and makes events from entries only. Each sends what its button
/// sent before (task E9.4), so requests and keys stay the same.
pub fn pills(reading: &Reading<'_>, entry: Option<ReaderEntry<'_>>) -> Vec<Pill> {
    let deciding = reading.deciding;
    let pill = |icon, label: &str, message| Pill { icon, label: label.to_owned(), message: Some(message), tip: None };
    let Some(ReaderEntry { item, hide, ai }) = entry else {
        let id = || reading.id.to_owned();
        return vec![
            pill(Icon::ListTodo, tr("Task"), Message::AskMailTask(id())),
            pill(Icon::BellOff, tr("Mute…"), Message::AskMuteMail(id())),
        ];
    };
    let id = || item.id.clone();
    let [ai_task, ai_event] = crate::suggest::ai_buttons(&item.id, ai).map(|(wanted, message, tip)| Pill {
        icon: Icon::Sparkles,
        label: wanted.label().to_owned(),
        tip: message.is_none().then_some(tip),
        message,
    });
    let mut pills = vec![
        pill(Icon::ListTodo, tr("Task"), Message::AskTask(id())),
        ai_task,
        pill(Icon::CalendarPlus, tr("Event"), Message::AskEvent(id())),
        ai_event,
    ];
    // What the mail is (task E12.8): change it by hand, or let the model look again. Only mail
    // has a category, and only there do the two make sense.
    if item.source.kind == torro_inbox::SourceKind::Mail {
        pills.push(pill(Icon::Tag, tr("Category…"), Message::AskCategory(id())));
        // Only with a decision model set up: without one the button could do nothing but show an
        // error, and a function without an AI simply is not there (`docs/ki.md`, principle 1).
        if deciding {
            pills.push(Pill {
                icon: Icon::Sparkles,
                label: tr("Re-categorize").to_owned(),
                message: Some(Message::Reclassify(id())),
                tip: None,
            });
        }
    }
    if hide {
        pills.push(pill(Icon::BellOff, tr("Mute…"), Message::AskHide(id())));
    }
    pills
}

/// `pills` in a row that wraps, [`space::s()`] apart (`widgets::pill_button`), each that opens a
/// menu or dialog registering its anchor in `anchors` (`detail::Anchors::of`: Task `task`, Event
/// `event`, Mute… `hide`); an AI pill's dialog opens at Task, as before.
pub fn pill_row<'a>(pills: Vec<Pill>, anchors: &detail::Anchors) -> Element<'a, Message> {
    let pills = pills.into_iter().map(|Pill { icon, label, message, tip }| {
        let anchor = message.as_ref().and_then(|message| anchors.of(message)).cloned();
        let pill = widgets::pill_button(icon, label, message);
        let pill = match tip {
            Some(tip) => widgets::with_tooltip(pill, tip),
            None => pill,
        };
        widgets::anchored_if(anchor.as_ref(), pill)
    });
    row(pills).spacing(space::s()).wrap().vertical_spacing(space::s()).into()
}

/// One cell of the reader's command bar ([`command_cells`]).
#[derive(Debug, Clone)]
pub struct CommandCell {
    pub icon: Icon,
    pub label: String,
    /// What a click sends; `None` shows the cell off (Delete in the trash).
    pub message: Option<Message>,
}

/// The cells of the command bar for the mail in `reading` (task E11.3), from the left: Reply (drawn
/// as the split with its menu, [`reply_menu`]), Forward, Archive, Delete – off in the trash – and,
/// for an `entry` that is not done, Later (`AskSnooze`). "…" ([`more_menu`]) closes the bar and is
/// no cell of this list. Each sends what its key sends ([`MailAction`]).
pub fn command_cells(reading: &Reading<'_>, entry: Option<&Item>) -> Vec<CommandCell> {
    let id = reading.id;
    let mail = |action: MailAction| Some(Message::Mail(action, id.to_owned()));
    let cell = |icon, label: &str, message| CommandCell { icon, label: label.to_owned(), message };
    let delete = (!reading.in_trash).then(|| Message::Mail(MailAction::Delete, id.to_owned()));
    let mut cells = vec![
        cell(Icon::Reply, tr("Reply"), mail(MailAction::Reply)),
        cell(Icon::Forward, tr("Forward"), mail(MailAction::Forward)),
        cell(Icon::Archive, trc("action", "Archive"), mail(MailAction::Archive)),
        cell(Icon::Trash2, tr("Delete"), delete),
    ];
    if let Some(item) = entry.filter(|item| !matches!(item.state, State::Done { .. })) {
        cells.push(cell(Icon::Clock, tr("Later"), Some(Message::AskSnooze(item.id.clone()))));
    }
    cells
}

/// The command bar of the mail in `reading` (task E11.3, `widgets::command_bar`), `room` wide – the
/// reader's whole width: the [`command_cells`] – Reply as the strong split whose chevron opens
/// [`reply_menu`] (`AskReplyMenu`) – and "…" at the end (`AskMore`, [`more_menu`]). All cells show
/// their words or none ([`widgets::command_words`]). Each cell that opens a menu registers its
/// anchor in `anchors`: Reply `reply`, Later `snooze`, "…" `more`. The panel of the mail window
/// takes the same bar, as wide as the panel (task E11.4, [`Bars::Command`]).
pub fn commands<'a>(
    reading: &Reading<'_>,
    entry: Option<&Item>,
    anchors: &detail::Anchors,
    room: f32,
) -> Element<'a, Message> {
    let id = reading.id.to_owned();
    let cells = command_cells(reading, entry);
    let labels: Vec<&str> = cells.iter().map(|cell| cell.label.as_str()).collect();
    let words = widgets::command_words(&labels, room);
    let mut bar = Vec::with_capacity(cells.len() + 1);
    for (i, CommandCell { icon, label, message }) in cells.into_iter().enumerate() {
        bar.push(if i == 0 {
            let menu = Some(Message::AskReplyMenu(id.clone()));
            widgets::anchored(&anchors.reply, widgets::command_split(icon, label, words, message, menu))
        } else {
            let anchor = message.as_ref().and_then(|message| anchors.of(message)).cloned();
            widgets::anchored_if(anchor.as_ref(), widgets::command_cell(icon, label, words, false, message))
        });
    }
    bar.push(widgets::anchored(&anchors.more, widgets::command_more(Some(Message::AskMore(id)))));
    widgets::command_bar(bar)
}

/// The menu of Reply's chevron in the command bar (task E11.3): "Reply all", with its key `a`; it
/// sends what `a` sends for the message `id`.
pub fn reply_menu<'a>(id: &str) -> Vec<MenuSection<'a, Message>> {
    let all = Message::Mail(MailAction::ReplyAll, id.to_owned());
    vec![MenuSection::new(vec![MenuItem::new(Icon::ReplyAll, tr("Reply all"), all).trail(Trail::Kbd("A"))])]
}

/// The menu of "…" at the end of the command bar (task E11.3): what a mail app keeps out of its bar,
/// in up to three sections – Flag or Unflag and Mark read or Mark unread for the mail in `reading`
/// (once its flags are known); for an `entry` Done or Reopen (`SetDone`, `e`) and Move to Info or
/// Move to For me (`AskLane`, `l`: its question opens at "…", which has its anchor); with `close`
/// (the reader) Close (`CloseDetail`, Escape) – the panel of task E11.4 has its ✕ in its head. Each
/// entry shows its key where it has one and sends what it sent before.
pub fn more_menu<'a>(reading: &Reading<'_>, entry: Option<&Item>, close: bool) -> Vec<MenuSection<'a, Message>> {
    let id = reading.id;
    let mail = |action: MailAction| Message::Mail(action, id.to_owned());
    let mut sections = Vec::new();
    if let Some(summary) = reading.summary() {
        let flagged = summary.flags.contains(&Flag::Flagged);
        let flag = if flagged { tr("Unflag") } else { tr("Flag") };
        let flag = MenuItem::new(Icon::Flag, flag, mail(MailAction::Flag(!flagged)));
        let seen = if summary.flags.contains(&Flag::Seen) {
            MenuItem::new(Icon::Mail, tr("Mark unread"), mail(MailAction::Read(false)))
        } else {
            MenuItem::new(Icon::MailOpen, tr("Mark read"), mail(MailAction::Read(true)))
        };
        sections.push(MenuSection::new(vec![flag, seen]));
    }
    if let Some(item) = entry {
        let finish = if matches!(item.state, State::Done { .. }) {
            MenuItem::new(Icon::Inbox, tr("Reopen"), Message::SetDone(item.id.clone(), false))
        } else {
            MenuItem::new(Icon::Check, tr("Done"), Message::SetDone(item.id.clone(), true))
        };
        let lane = crate::app::other_lane(item.lane);
        let move_to = MenuItem::new(crate::app::lane_icon(lane), lane_word(lane), Message::AskLane(item.id.clone()));
        let mut entries = vec![finish.trail(Trail::Kbd("E")), move_to.trail(Trail::Kbd("L"))];
        // What the mail is, also reachable without the reader (task E12.8): in Compact and in the
        // narrow window the pills are not there.
        if item.source.kind == torro_inbox::SourceKind::Mail {
            entries.push(MenuItem::new(Icon::Tag, tr("Category…"), Message::AskCategory(item.id.clone())));
        }
        sections.push(MenuSection::new(entries));
    }
    if close {
        let close = MenuItem::new(Icon::X, tr("Close"), Message::CloseDetail).trail(Trail::Kbd("Esc"));
        sections.push(MenuSection::new(vec![close]));
    }
    sections
}

/// The mail's attachments as one wrapping line of chips (task E11.3, `widgets::file_chip`), fixed
/// over the reader's foot: a `line` above, [`space::s()`] above and below the chips and `side` at
/// the sides, the chips `room` wide in all ([`attachment_chips`]; the panel of task E11.4 has them
/// in its footer, [`panel_foot`]). A click opens one as before ([`Message::OpenAttachment`]).
/// `None` while the mail loads, after an error and for a mail without attachments (inline pictures
/// are none).
pub fn attachment_line<'a>(reading: &Reading<'_>, room: f32, side: f32) -> Option<Element<'a, Message>> {
    let chips = attachment_chips(reading, room)?;
    let line = container(chips).width(Fill).padding([space::s(), side]);
    Some(column![rule::horizontal(size::HAIRLINE).style(style::rule), line].into())
}

/// The chips of the mail's attachments, wrapping on a line `room` wide, [`space::s()`] apart
/// ([`attachment_chip`]); `None` where [`attachment_line`] has none.
fn attachment_chips<'a>(reading: &Reading<'_>, room: f32) -> Option<Element<'a, Message>> {
    let Some(Ok(mail)) = reading.loaded else { return None };
    let files: Vec<&AttachmentInfo> = mail.attachments.iter().filter(|file| !file.inline).collect();
    if files.is_empty() {
        return None;
    }
    let chips = files.into_iter().map(|file| attachment_chip(reading.id, file, room));
    Some(row(chips).spacing(space::s()).wrap().vertical_spacing(space::s()).into())
}

/// One attachment as a chip of [`attachment_line`] on a line `room` wide: its name – cut with "…"
/// where it would take more than half the line ([`chip_name_room`]), then the whole name as
/// tooltip – and its size; a click fetches and opens it, as the panel's row does.
fn attachment_chip<'a>(id: &str, file: &AttachmentInfo, room: f32) -> Element<'a, Message> {
    let name = file.filename.as_deref().map(list::one_line).filter(|name| !name.is_empty());
    let name = name.unwrap_or_else(|| tr("Attachment").to_owned());
    let weight = size_text(file.size);
    let shown = typo::ellipsize(&name, typo::body(), chip_name_room(&weight, room));
    let cut = shown != name;
    let chip = widgets::file_chip(shown, weight, Some(Message::OpenAttachment(id.to_owned(), file.part.clone())));
    if cut { widgets::with_tooltip(chip, name) } else { chip }
}

/// How wide the name on an attachment's chip may be ([`attachment_chip`]): half the line's `room`,
/// less what the chip needs beside the name – its padding, the paperclip, the gaps and its size
/// `weight` – so one long name never takes a line of its own.
fn chip_name_room(weight: &str, room: f32) -> f32 {
    let frame = 2.0 * space::m() + icon::SIZE_SM + 2.0 * space::s() + typo::text_width(weight, typo::meta());
    (room / 2.0 - frame).max(0.0)
}

/// The reader's foot (task E11.3), fixed at its bottom: a `line` above, the window's surface `bg`
/// under it, [`space::s()`] above and below and `side` at the sides; left-aligned "HTML | Text" and
/// "Light | Dark" as sunken switches (`widgets::view_switch`) and "Copy text" as a quiet icon
/// ([`foot_controls`]). `None` with nothing to show: while the mail loads, after an error, and for a
/// mail without text or HTML. The panel of the mail window has the same controls in its footer
/// (task E11.4, [`panel_foot`]), where the panel draws the line and the padding.
pub fn foot<'a>(tools: ViewTools, side: f32) -> Option<Element<'a, Message>> {
    let controls = foot_controls(tools);
    if controls.is_empty() {
        return None;
    }
    let controls = row(controls).spacing(space::s()).align_y(Alignment::Center);
    let line = container(controls).width(Fill).padding([space::s(), side]).style(style::container::window);
    Some(column![rule::horizontal(size::HAIRLINE).style(style::rule), line].into())
}

/// What the foot ([`foot`]) holds of [`ViewTools`], each only where `tools` has it: "HTML | Text"
/// ([`Icon::Code`], [`Icon::AlignLeft`]), "Light | Dark" ([`Icon::Sun`], [`Icon::Moon`], only while
/// the HTML shows) – icons with their word as tooltip – and "Copy text" ([`copy_button`]).
fn foot_controls<'a>(tools: ViewTools) -> Vec<Element<'a, Message>> {
    let mut controls = Vec::new();
    if let Some(format) = tools.format {
        let options = [
            (Icon::Code, tr("HTML"), Message::ShowMailAs(MailFormat::Html)),
            (Icon::AlignLeft, tr("Text"), Message::ShowMailAs(MailFormat::Text)),
        ];
        controls.push(widgets::view_switch(options, usize::from(format == MailFormat::Text)));
    }
    if let Some(scheme) = tools.scheme {
        let options = [
            (Icon::Sun, tr("Light"), Message::MailColors(Scheme::Light)),
            (Icon::Moon, tr("Dark"), Message::MailColors(Scheme::Dark)),
        ];
        controls.push(widgets::view_switch(options, usize::from(scheme == Scheme::Dark)));
    }
    if tools.copy {
        controls.push(copy_button());
    }
    controls
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
/// has, and nothing of Torro's. The reader has its command bar instead (task E11.3, [`commands`]).
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

/// An attachment as a row: the paperclip, its name and size; a click fetches and opens it.
fn attachment_row<'a>(id: &str, file: &AttachmentInfo) -> Element<'a, Message> {
    let name = file.filename.as_deref().map(list::one_line).filter(|name| !name.is_empty());
    let name = name.unwrap_or_else(|| tr("Attachment").to_owned());
    let line = row![
        container(widgets::prop_value(name)).width(Fill),
        typo::text(typo::meta().tabular(), size_text(file.size)).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let open = Message::OpenAttachment(id.to_owned(), file.part.clone());
    widgets::prop_row(Some(Mark::Icon(Icon::Paperclip)), line, Some(open))
}

/// The text of `mail` as the window shows, selects and copies it (issue #119): plain, without
/// control characters ([`list::plain`]); empty for a mail without text.
pub fn text_of(mail: &Mail) -> String {
    mail.text.as_deref().map(list::plain).unwrap_or_default()
}

/// Whether the mail in `reading` shows as HTML (task M9.7), not as text.
fn html_shown(reading: &Reading<'_>) -> bool {
    reading.html.is_some_and(|html| html.shown())
}

/// The mail's HTML ([`HtmlReading`]) as wide as its parent gives, links reported as
/// [`Message::HtmlLink`]. The mail paints its background up to the view's edges, so the caller
/// places it without padding and sets the margin of a mail without a background of its own.
fn html_view<'a>(html: HtmlReading<'a>) -> HtmlView<'a, Message> {
    HtmlView::new(html.content).scheme(html.scheme).scale(html.scale).on_link(Message::HtmlLink)
}

/// The note under a mail shown as HTML: it comes from outside, no scripts run. "Copy text" stands in
/// the reader's foot (task E11.3) and the panel's footer (task E9.4), not beside it.
fn html_note<'a>() -> Element<'a, Message> {
    note(tr("Mail from outside the suite: no scripts run, pictures from the web are loaded."))
}

/// The text in `level`, to select with the mouse and copy (issue #119, [`Selectable`]), then the
/// note on foreign text. "Copy text" stands in the reader's foot (task E11.3) and the panel's footer (task E9.4). The
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use torro_inbox::{SourceKind, demo};

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time")
    }

    fn folders() -> Vec<Folder> {
        DemoSource::new(now()).mail_folders().expect("folders")
    }

    #[test]
    fn html_lies_edge_to_edge_under_a_head_as_wide_as_the_reader() {
        let pad = layout::page_padding();
        let sides = pad.left + pad.right;
        for width in [2000.0, 1400.0, 700.0] {
            let html = reader_widths(width, true);
            assert_eq!(html.mail, width, "the mail fills the reader at {width}");
            assert_eq!(html.head, width - sides, "the head keeps only the padding at {width}");
        }
        // No upper bound: far wider than the reading width.
        assert!(reader_widths(3000.0, true).head > size::content_max());
    }

    #[test]
    fn text_uses_the_whole_width_inside_the_padding() {
        let pad = layout::page_padding();
        let sides = pad.left + pad.right;
        for width in [3000.0, 2000.0, 1400.0, 500.0] {
            let text = reader_widths(width, false);
            assert_eq!(text, ReaderWidths { head: width - sides, mail: width - sides }, "at {width}");
        }
        assert!(reader_widths(2000.0, false).mail > size::content_max(), "no reading width as a cap");
        // Never below nothing, however narrow.
        assert_eq!(reader_widths(10.0, false), ReaderWidths { head: 0.0, mail: 0.0 });
        assert_eq!(reader_widths(10.0, true), ReaderWidths { head: 0.0, mail: 10.0 });
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
        };
        check(&reading)
    }

    /// A mail entry of the demo, new and for the user, for the reader's command bar, pills and menus.
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

    /// The words of the cells of the reader's command bar.
    fn cell_words(cells: &[CommandCell]) -> Vec<String> {
        cells.iter().map(|cell| cell.label.clone()).collect()
    }

    /// The words of a menu's entries, section by section.
    fn menu_words(sections: &[MenuSection<'_, Message>]) -> Vec<Vec<String>> {
        sections.iter().map(|section| section.items.iter().map(|item| item.label.to_string()).collect()).collect()
    }

    /// Task E11.3: the command bar holds exactly Reply, Forward, Archive, Delete and – for an entry
    /// that is not done – Later, then "…"; each sends what its key sends, Delete is off in the trash.
    #[test]
    fn the_command_bar_holds_reply_forward_archive_delete_later_and_more() {
        let item = mail_entry();
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            let standard = [tr("Reply"), tr("Forward"), trc("action", "Archive"), tr("Delete")];
            assert_eq!(cell_words(&command_cells(reading, None)), standard, "a folder's message");
            let cells = command_cells(reading, Some(&item));
            assert_eq!(cell_words(&cells), [standard.as_slice(), &[tr("Later")]].concat(), "an entry");
            let mail = |n: usize, action: MailAction| {
                let sent = cells[n].message.as_ref();
                matches!(sent, Some(Message::Mail(sent, id)) if *sent == action && id == "m:w1")
            };
            assert!(mail(0, MailAction::Reply) && mail(1, MailAction::Forward));
            assert!(mail(2, MailAction::Archive) && mail(3, MailAction::Delete));
            assert!(matches!(&cells[4].message, Some(Message::AskSnooze(id)) if *id == item.id));
            let trash = Reading { in_trash: true, ..*reading };
            assert!(command_cells(&trash, None)[3].message.is_none(), "Delete off in the trash");
            let done = Item { state: State::Done { at: now() }, ..item.clone() };
            assert_eq!(cell_words(&command_cells(reading, Some(&done))), standard, "a done entry is not put off");
            // Words for all cells or none, across the reader's whole width.
            let labels = [standard.as_slice(), &[tr("Later")]].concat();
            assert!(widgets::command_words(&labels, 1200.0), "the wide reader speaks");
            assert!(!widgets::command_words(&labels, 400.0), "a narrow one shows icons");
            for room in [400.0, 900.0, 1400.0] {
                let _ = commands(reading, Some(&item), &detail::Anchors::default(), room);
            }
        });
    }

    /// Task E11.3: Reply's menu holds exactly "Reply all" (`a`); "…" holds Flag, read, for an entry
    /// Done or Reopen (`e`) and "Move to …" (`l`), and in the reader Close (Escape) – each sending
    /// what the buttons sent before.
    #[test]
    fn the_menus_hold_what_the_bar_leaves_out() {
        let reply = reply_menu("m:w1");
        assert_eq!(menu_words(&reply), [[tr("Reply all")]]);
        let all = &reply[0].items[0];
        assert!(matches!(&all.on_press, Message::Mail(MailAction::ReplyAll, id) if id == "m:w1"));
        assert_eq!(all.trail, Trail::Kbd("A"));
        let item = mail_entry();
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            let summary = reading.summary().expect("headers");
            let flagged = summary.flags.contains(&Flag::Flagged);
            let flag = if flagged { tr("Unflag") } else { tr("Flag") };
            let read = if summary.flags.contains(&Flag::Seen) { tr("Mark unread") } else { tr("Mark read") };
            let more = more_menu(reading, Some(&item), true);
            let move_to = lane_word(Lane::Info);
            assert_eq!(
                menu_words(&more),
                [
                    vec![flag.to_owned(), read.to_owned()],
                    vec![tr("Done").to_owned(), move_to.clone(), tr("Category…").to_owned()],
                    vec![tr("Close").to_owned()]
                ]
            );
            let sent: Vec<&Message> =
                more.iter().flat_map(|section| section.items.iter().map(|i| &i.on_press)).collect();
            assert!(matches!(sent[0], Message::Mail(MailAction::Flag(on), id) if *on != flagged && id == "m:w1"));
            assert!(matches!(sent[1], Message::Mail(MailAction::Read(_), id) if id == "m:w1"));
            assert!(matches!(sent[2], Message::SetDone(id, true) if *id == item.id));
            assert!(matches!(sent[3], Message::AskLane(id) if *id == item.id));
            assert!(matches!(sent[4], Message::AskCategory(id) if *id == item.id), "task E12.8");
            assert!(matches!(sent[5], Message::CloseDetail));
            let keys: Vec<Trail> = more.iter().flat_map(|section| section.items.iter().map(|i| i.trail)).collect();
            assert_eq!(
                keys,
                [Trail::None, Trail::None, Trail::Kbd("E"), Trail::Kbd("L"), Trail::None, Trail::Kbd("Esc")]
            );
            // A folder's message: the mail's own entries and Close; the panel (task E11.4) has its ✕.
            let folder = more_menu(reading, None, true);
            assert_eq!(menu_words(&folder), [vec![flag.to_owned(), read.to_owned()], vec![tr("Close").to_owned()]]);
            assert_eq!(more_menu(reading, None, false).len(), 1, "no Close in the panel");
            let done = Item { state: State::Done { at: now() }, ..item.clone() };
            let reopen = more_menu(reading, Some(&done), true);
            assert!(matches!(&reopen[1].items[0].on_press, Message::SetDone(_, false)), "a done entry reopens");
        });
    }

    /// Task E11.3: the pills are exactly Task, AI task, Event, AI event, Mute… for an entry – the AI
    /// off and saying why without an AI, no Mute… where it cannot be muted – and Task, Mute… for a
    /// folder's message; each sends what its button sent before.
    #[test]
    fn the_pills_are_what_torro_adds() {
        let item = mail_entry();
        let words = |pills: &[Pill]| pills.iter().map(|pill| pill.label.clone()).collect::<Vec<_>>();
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            let all = pills(reading, Some(ReaderEntry { item: &item, hide: true, ai: true }));
            // Task E12.8 adds "Category…" before "Mute…"; "Re-categorize" only with a model
            // set up (`docs/ki.md`, principle 1), and this reading has none.
            assert_eq!(
                words(&all),
                [tr("Task"), tr("AI task"), tr("Event"), tr("AI event"), tr("Category…"), tr("Mute…")]
            );
            let last = all.len() - 1;
            assert!(matches!(all[last - 1].message.as_ref(), Some(Message::AskCategory(id)) if *id == item.id));
            let sent = |n: usize| all[n].message.as_ref();
            assert!(matches!(sent(0), Some(Message::AskTask(id)) if *id == item.id));
            assert!(matches!(sent(1), Some(Message::SuggestTask(id)) if *id == item.id));
            assert!(matches!(sent(2), Some(Message::AskEvent(id)) if *id == item.id));
            assert!(matches!(sent(3), Some(Message::SuggestEvent(id)) if *id == item.id));
            assert!(matches!(sent(5), Some(Message::AskHide(id)) if *id == item.id));
            assert!(all.iter().all(|pill| pill.tip.is_none()), "on, no tooltip of their own");
            let anchors = detail::Anchors::default();
            let _ = pill_row(all.clone(), &anchors);
            let quiet = pills(reading, Some(ReaderEntry { item: &item, hide: false, ai: false }));
            assert_eq!(
                words(&quiet),
                [tr("Task"), tr("AI task"), tr("Event"), tr("AI event"), tr("Category…")],
                "no bell"
            );
            let tip = |wanted| Some(crate::suggest::tooltip(wanted, false));
            assert!(quiet[1].message.is_none() && quiet[1].tip == tip(crate::suggest::Wanted::Task));
            assert!(quiet[3].message.is_none() && quiet[3].tip == tip(crate::suggest::Wanted::Event));
            let folder = pills(reading, None);
            assert_eq!(words(&folder), [tr("Task"), tr("Mute…")]);
            assert!(matches!(folder[0].message.as_ref(), Some(Message::AskMailTask(id)) if id == "m:w1"));
            assert!(matches!(folder[1].message.as_ref(), Some(Message::AskMuteMail(id)) if id == "m:w1"));
            let _ = head(reading, None, &anchors, 800.0);
        });
    }

    /// Task E11.3: the foot holds "HTML | Text", "Light | Dark" (only while the HTML shows) and "Copy
    /// text", each where the mail has it, and nothing – no foot at all – while the mail loads.
    #[test]
    fn the_foot_holds_the_view_once_the_mail_is_in() {
        let side = layout::page_padding().left;
        reading_of("m:p2", MailFormat::Html, true, |reading| {
            assert_eq!(foot_controls(reading.view_tools()).len(), 3, "HTML | Text, Light | Dark, Copy text");
            assert!(foot(reading.view_tools(), side).is_some());
        });
        reading_of("m:p2", MailFormat::Text, true, |reading| {
            assert_eq!(foot_controls(reading.view_tools()).len(), 2, "no Light | Dark for the text");
        });
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            assert_eq!(foot_controls(reading.view_tools()).len(), 1, "a text mail: Copy text");
        });
        reading_of("m:p2", MailFormat::Html, false, |reading| {
            assert!(foot_controls(reading.view_tools()).is_empty(), "no switch while it loads");
            assert!(foot(reading.view_tools(), side).is_none(), "and no foot");
        });
    }

    /// Task E11.3: the attachments stand as chips over the foot once the mail is in – none while it
    /// loads, none for inline pictures –, and a long name ends in "…" before it takes half the line.
    #[test]
    fn attachments_are_chips_over_the_foot() {
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            assert!(attachment_line(reading, 800.0, 24.0).is_some(), "the budget's sheet");
        });
        reading_of("m:w1", MailFormat::Html, false, |reading| {
            assert!(attachment_line(reading, 800.0, 24.0).is_none(), "nothing while it loads");
        });
        reading_of("m:p2", MailFormat::Html, true, |reading| {
            assert!(attachment_line(reading, 800.0, 24.0).is_none(), "the newsletter's pictures are inline");
        });
        let room = chip_name_room("3 KB", 800.0);
        assert!(room > 0.0 && room < 400.0, "{room}");
        let long = "quarterly-budget-review-with-all-departments-and-travel-costs-final-v7.xlsx";
        let shown = typo::ellipsize(long, typo::body(), room);
        assert!(shown != long && shown.ends_with('…'), "{shown}");
        assert_eq!(typo::ellipsize("budget-q4.csv", typo::body(), room), "budget-q4.csv", "a short name whole");
        assert_eq!(chip_name_room("3 KB", 0.0), 0.0, "never below nothing");
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
    /// is the command bar's strong cell, Torro's buttons are pills.
    #[test]
    fn the_reader_has_no_red_button() {
        let source = include_str!("mail.rs");
        let parts = [
            "reader",
            "reader_mail",
            "head",
            "sender_block",
            "pill_row",
            "commands",
            "attachment_line",
            "attachment_chip",
            "foot",
            "foot_controls",
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

    /// Task E11.4: the mail window's panel is built from the reader's parts – command bar, the head
    /// without the subject (the panel's title has it), the pills, the mail – and none of the split of
    /// task E9.4, which the hub and the App Feed keep; its footer holds the attachments' chips and the
    /// foot's switches, nothing while the mail loads.
    #[test]
    fn the_mail_windows_panel_takes_the_readers_parts() {
        let source = include_str!("mail.rs");
        let command = function_source(source, "command_body");
        for part in ["head_with(", "detail::properties(", "panel_letter("] {
            assert!(command.contains(part), "{part}");
        }
        // The command bar stands fixed above the body (`Panel::bar`), not in what scrolls.
        assert!(!command.contains("commands("), "the bar does not scroll with the mail");
        assert!(function_source(source, "panel_bar").contains("commands("));
        assert!(command.contains("room, false)"), "the head without the subject");
        assert!(!command.contains("panel_standards") && !command.contains("header("), "none of the split");
        let split = function_source(source, "split_body");
        assert!(split.contains("panel_standards(") && split.contains("header("), "the hub's panel as in E9.4");
        let foot = function_source(source, "panel_foot");
        assert!(foot.contains("attachment_chips(") && foot.contains("foot_controls("), "the reader's foot");
        for name in ["panel", "entry_panel"] {
            assert!(function_source(source, name).contains("panel_foot("), "{name}: the reader's foot");
            assert!(function_source(source, name).contains(".bar(panel_bar("), "{name}: the fixed bar");
        }
        let width = size::panel();
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            assert_eq!(panel_foot(reading, width).len(), 1, "the sheet's chip over Copy text, in one column");
        });
        reading_of("m:p2", MailFormat::Html, true, |reading| {
            assert_eq!(panel_foot(reading, width).len(), 1, "the switches of an HTML mail");
        });
        reading_of("m:w1", MailFormat::Html, false, |reading| {
            assert!(panel_foot(reading, width).is_empty(), "no footer while the mail loads");
            assert!(foot_controls(reading.view_tools()).is_empty());
        });
        let item = mail_entry();
        let entry = ReaderEntry { item: &item, hide: true, ai: true };
        reading_of("m:w1", MailFormat::Html, true, |reading| {
            let anchors = detail::Anchors::default();
            for width in [width, 480.0, 0.0] {
                let _ = entry_panel(reading, entry, tr("For me").to_owned(), &anchors, width);
                let _ = panel(reading, None, &anchors, Bars::Command { entry: None, width });
            }
            // The panel's "…" leaves Close out: the ✕ stands in its head.
            let more = more_menu(reading, Some(&item), false);
            assert!(more.iter().all(|section| section.items.iter().all(|i| i.label != tr("Close"))));
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
        let views = [View::Folder(folder.clone()), View::Attachments, View::Mail, View::Search(search)];
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
                for (messages, edge_strip) in [(&summaries[..], false), (&summaries[..], true), (&[], true)] {
                    let data = MailList {
                        messages,
                        view,
                        no_accounts: false,
                        selected: Some("m:w1"),
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
                        edge_strip,
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
                    let reading = Reading {
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
                    };
                    let anchors = detail::Anchors::default();
                    let _ = panel(&reading, Some(&folder), &anchors, Bars::Split);
                    for entry in [None, Some(ReaderEntry { item: &entry, hide: true, ai: false })] {
                        // The mail window's panel (task E11.4), over the medium window and filling a
                        // narrow one.
                        for width in [size::panel(), 480.0] {
                            let _ = panel(&reading, Some(&folder), &anchors, Bars::Command { entry, width });
                            let _ = body(&reading, Bars::Command { entry, width }, &anchors);
                            if let Some(entry) = entry {
                                let _ = entry_panel(&reading, entry, tr("For me").to_owned(), &anchors, width);
                            }
                        }
                        let _ = body(&reading, Bars::Split, &anchors);
                        let _ = reader(reading, entry, anchors.clone());
                        // The reader's parts, which it lays out only once it knows its width.
                        let _ = commands(&reading, entry.map(|entry| entry.item), &anchors, 900.0);
                        let _ = head(&reading, entry, &anchors, 900.0);
                        let _ = more_menu(&reading, entry.map(|entry| entry.item), true);
                    }
                    let _ = reader_mail(&reading, reader_widths(900.0, html_edge(&reading).is_some()));
                    let _ = attachment_line(&reading, 900.0, layout::page_padding().left);
                    let _ = foot(reading.view_tools(), layout::page_padding().left);
                }
            }
        }
    }

    /// Task E8.11 (finding 3): an invitation's card in "Invitations" offers Later and "…" for its
    /// entry, as the same mail does under "For me"; without an open entry only the answers and Task;
    /// a message as any has Archive · Delete · Task (issue #118).
    #[test]
    fn an_invitation_card_offers_later_and_more_for_its_entry() {
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

    /// Task E11.2: a message's strip in the mail window – its buttons and then the bell, in the order
    /// of `list::edge_actions` – is Archive · Delete · Task (its icon red) · Mute…, an invitation's
    /// Accept · Maybe · Decline · Later · Task · Mute…: the bell stays last, nothing is lost.
    #[test]
    fn a_messages_strip_files_it_then_task_then_the_bell() {
        let strip = |message: &MessageSummary, entries: &[Item]| {
            let (mut buttons, more) = message_buttons(message, entries, &folders(), now());
            buttons.extend(more);
            let before: Vec<String> = buttons.iter().map(|b| format!("{:?}", b.on_press)).collect();
            let strip = list::edge_actions(buttons);
            let after: Vec<String> = strip.iter().map(|b| format!("{:?}", b.on_press)).collect();
            assert_eq!(after, before, "the order the buttons had already");
            let red: Vec<String> = strip.iter().filter(|b| b.accent).map(|b| b.label.to_string()).collect();
            assert_eq!(red, [tr("Task")]);
            strip.iter().map(|b| b.label.to_string()).collect::<Vec<_>>()
        };
        let mut message = demo::mail_sample(now()).remove(0).summary;
        let plain = MessageSummary { invite: None, ..message.clone() };
        assert_eq!(strip(&plain, &[]), [trc("action", "Archive"), tr("Delete"), tr("Task"), tr("Mute…")]);
        let start = now() + chrono::TimeDelta::days(1);
        message.invite = Some(Invite { title: "Planning".into(), start, end: None, all_day: false, answered: None });
        let all = iced::futures::executor::block_on(torro_inbox::InboxSource::list_items(
            &DemoSource::new(now()),
            Default::default(),
        ))
        .expect("the demo answers");
        let mail = all.iter().find(|item| item.source.kind == SourceKind::Mail).expect("a mail entry");
        let entry = Item { link: Some(Link::Mail { message: message.id.clone() }), state: State::New, ..mail.clone() };
        let answers = [tr("Accept"), tr("Maybe"), tr("Decline"), tr("Later"), tr("Task"), tr("Mute…")];
        assert_eq!(strip(&message, &[entry]), answers);
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

    /// Task E8.3b: the smart view keeps the invitations not over yet, the soonest first; the demo's
    /// two are answered as sent, any other mail is none.
    #[test]
    fn invitations_are_the_ones_not_over_yet_soonest_first() {
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
        let found = invitations(inboxes.clone(), now());
        let ids: Vec<&str> = found.iter().map(|message| message.id.as_str()).collect();
        assert_eq!(ids, ["m:p1", "m:w4"], "the hike this Saturday before lunch next week");
        let hike = found[0].invite.clone().expect("an invite");
        assert!(hike.all_day && !invite_when(&hike, now()).contains(':'), "a whole day has no time");
        let lunch = found[1].invite.clone().expect("an invite");
        assert!(invite_when(&lunch, now()).contains("12:00"), "{}", invite_when(&lunch, now()));
        assert!(invite_over(&lunch, lunch.end.expect("an end")) && !invite_over(&lunch, now()));
        assert!(invitations(inboxes, lunch.end.expect("an end")).is_empty(), "both over by then");

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

    /// Task E8.3: the smart view keeps the messages with attachments of every inbox, newest first.
    #[test]
    fn the_smart_view_holds_the_mail_with_attachments_newest_first() {
        let mail = demo::mail_sample(now());
        let pages: Vec<Vec<MessageSummary>> = demo::MAIL_ACCOUNTS
            .iter()
            .map(|account| mail.iter().filter(|m| m.summary.account == *account).map(|m| m.summary.clone()).collect())
            .collect();
        let found = attachments(pages.clone(), Order::NewestFirst);
        assert!(!found.is_empty(), "the demo has mail with attachments");
        assert!(found.iter().all(|message| message.has_attachments));
        assert!(found.windows(2).all(|pair| pair[0].date >= pair[1].date), "newest first");
        let oldest = attachments(pages, Order::OldestFirst);
        assert!(oldest.windows(2).all(|pair| pair[0].date <= pair[1].date), "oldest first (task E8.15)");
        let wanted = mail.iter().filter(|m| m.summary.has_attachments).count();
        assert_eq!(found.len(), wanted, "from every account");
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
            MailLook { format: MailFormat::Html, colors: MailAppearance::App, hidden_categories: Vec::new() }
        );
    }
}
