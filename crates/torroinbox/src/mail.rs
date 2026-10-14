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
//! - [`list`] draws a folder's messages – rows, or cards in the comfortable density of the mail
//!   window (task E8.3, [`message_card`]) – and the smart view "With attachments" ([`attachments`]:
//!   the messages with attachments of every account's inbox). [`body`] is the whole mail for the
//!   panel, [`reader`] the reader beside the list in the wide mail window (task E8.3): Reply ·
//!   Reply all · Forward | Done · Later · Task, the text in the reading level (`typo::read()`);
//!   [`empty_reader`] keeps its column while no mail is open (task E8.11).
//! - The task made of a mail (task E8.7b) stands over its text as a card ([`task_card`], "Open
//!   task"); "Task" on a folder's message makes one ([`MailSource::create_todo_from_mail`]).
//!
//! **Mail is foreign text** (`untrusted`). The core turns HTML into text and fetches nothing it
//! names, so no picture or other content from the web is ever loaded. The text is drawn as it is;
//! web addresses in it ([`pieces`]) become links that show the whole address and open only on a
//! click. An attachment is fetched by the core into its attachment folder and opened with the
//! system, except files that could run programs ([`runs_programs`]): their folder opens instead.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use chrono::{DateTime, Local, NaiveDate, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, responsive, rich_text, row, rule, scrollable, span, svg, text_input};
use iced::{Alignment, Color, Element, Fill, Padding, Theme};
use torro_core::i18n::{tr, trc, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout;
use torro_gui::panel;
use torro_gui::source;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Anchor, Mark, RowAction};
use torro_gui::{datetime, style, typo};
use torro_inbox::InboxError;
use torro_inbox::demo::DemoSource;
use torro_inbox::item::{ItemTask, TaskDue};
use torro_inbox::source::TaskList;
use torro_inbox::{Item, Lane, Link, State};
use torro_ipc::inbox::{CalendarEvent, InboxClient};
use torro_ipc::mail::{Delivery, MoveRole, MoveTarget};
use torro_mail::{
    AttachmentInfo, Flag, Folder, InvitationAnswer, Invite, MailAddress, Message as Mail, MessageSummary, Order, Role,
};

use crate::app::{INBOX, Message, View};
use crate::detail;
use crate::keys::Command as Key;
use crate::list::{self, Look};

/// How many messages one page of a folder holds.
pub const PAGE: u32 = 50;

/// The id of the search field above a folder's messages.
pub const SEARCH: &str = "mail-search";

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

    /// `mail.get_message` with its text. Reading does not mark it read.
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
}

/// One page of a folder ([`MailSource::search_mail`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MailPage {
    /// Newest first.
    pub messages: Vec<MessageSummary>,
    /// The cursor of the next page; `None` on the last one.
    pub next: Option<String>,
}

impl MailSource for InboxClient {
    async fn mail_folders(&self) -> Result<Vec<Folder>, InboxError> {
        InboxClient::mail_folders(self).await
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
        Ok(MailPage { messages, next })
    }

    async fn get_mail(&self, id: String) -> Result<Mail, InboxError> {
        InboxClient::get_mail(self, id).await
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
        Ok(MailPage { messages, next })
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
}

/// The view of one folder: which one, and the search typed into it. Two views of the same folder
/// with another search are different views, so an answer for the old search is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderView {
    /// The folder's id from the core (`f:…`).
    pub id: String,
    /// Its account, `mail:<address>`.
    pub account: String,
    /// The name to show ([`folder_name`]).
    pub name: String,
    pub role: Option<Role>,
    /// The words searched for; `None` lists the whole folder.
    pub query: Option<String>,
}

impl FolderView {
    /// The whole folder `folder`, without a search.
    pub fn of(folder: &Folder) -> FolderView {
        FolderView {
            id: folder.id.clone(),
            account: folder.account.clone(),
            name: folder_name(folder),
            role: folder.role,
            query: None,
        }
    }

    /// The same folder searched for `query`; a blank query lists the whole folder.
    pub fn searching(&self, query: &str) -> FolderView {
        let query = Some(query.trim().to_owned()).filter(|query| !query.is_empty());
        FolderView { query, ..self.clone() }
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
            MailAction::Archive | MailAction::Delete | MailAction::Flag(_) | MailAction::Read(_) => None,
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
}

impl MailEdit {
    /// The messages it changes.
    pub fn ids(&self) -> &[String] {
        match self {
            MailEdit::Flags { ids, .. } | MailEdit::Move { ids, .. } | MailEdit::Delete { ids } => ids,
        }
    }

    /// Sends the edit to `source`; the error as people read it.
    pub async fn apply<S: MailSource>(self, source: Arc<S>) -> Result<(), String> {
        let result = match self {
            MailEdit::Flags { ids, add, remove } => source.set_mail_flags(ids, add, remove).await,
            MailEdit::Move { ids, to } => source.move_mail(ids, to).await,
            MailEdit::Delete { ids } => source.delete_mail(ids).await,
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

/// A piece of a mail's text: words, or a web address that may be opened by a click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece<'a> {
    Text(&'a str),
    /// An `http://` or `https://` address, whole: the link shows exactly where it goes.
    Link(&'a str),
}

/// `text` cut into words and web addresses. An address starts with `http://` or `https://` (any
/// case) and runs to the next white space or `<`, `>`, `"`; a full stop, comma, closing bracket
/// and the like at its end belong to the sentence. Nothing else is a link: no `mailto:`, no
/// `file:`, no address hidden behind other words.
pub fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while let Some(start) = link_start(rest) {
        let tail = &rest[start..];
        let end = tail.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).unwrap_or(tail.len());
        let link = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'']);
        let scheme = if tail.to_ascii_lowercase().starts_with("https://") { "https://".len() } else { "http://".len() };
        if link.len() <= scheme {
            // Only the scheme: words, not a link.
            pieces.push(Piece::Text(&rest[..start + end]));
            rest = &rest[start + end..];
            continue;
        }
        if start > 0 {
            pieces.push(Piece::Text(&rest[..start]));
        }
        pieces.push(Piece::Link(link));
        rest = &rest[start + link.len()..];
    }
    if !rest.is_empty() {
        pieces.push(Piece::Text(rest));
    }
    pieces
}

/// Where the first web address in `text` starts, at the start of a word.
fn link_start(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find("http") {
        let at = from + found;
        let word_start = at == 0 || !lower[..at].ends_with(|c: char| c.is_alphanumeric());
        if word_start && (lower[at..].starts_with("http://") || lower[at..].starts_with("https://")) {
            return Some(at);
        }
        from = at + 4;
    }
    None
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
    /// Ids of those entries whose menu "Hide from inbox" has entries (`App::hide_choices`).
    pub hidable: Vec<&'a str>,
    /// Every folder of every account: Archive and Delete show only where the message's account has
    /// such a folder ([`filing_actions`], issue #118).
    pub folders: &'a [Folder],
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
            View::Folder(folder) if folder.query.is_some() => {
                (Icon::Search, tr("No messages found."), tr("Try other words."))
            }
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
/// avatar), the start of the text –, [`list::message_height`] high. Its action bar (issue #118,
/// `widgets::row_actions_at` on the first line, as an entry's row) holds what its card offers
/// ([`message_buttons`]), "…" last.
fn message_row<'a>(data: &MailList<'a>, message: &'a MessageSummary, width: f32) -> Element<'a, Message> {
    let mut marks: Vec<(Element<'a, Message>, f32)> = Vec::new();
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
    };
    let selected = data.selected == Some(message.id.as_str());
    let on_press = Message::SelectMail(message.id.clone());
    let height = list::message_height();
    let row = list::sized_row(list::mail_content(lines, width), selected, on_press, height);
    let (mut actions, more) = message_buttons(message, data.entries, &data.hidable, data.folders, data.now);
    actions.extend(more);
    let actions = data.anchors.mark(selected, actions);
    let words = widgets::row_actions_words(&actions, width);
    // On the first line, over the time, as the rows of entries: subject and preview stay readable.
    let row = widgets::row_actions_at(row, actions, selected, words, list::first_line_middle(height, 3));
    if selected { widgets::anchored(&data.anchors.row, row) } else { row }
}

/// The card of a message `width` wide (task E8.3, `list::mail_card`): the lines of its row, as chips
/// "Attachments" and "Flagged", and its buttons ([`message_buttons`]). An invitation (task E8.3b)
/// shows when it is and the answer given as chips. In the smart views, which hold every account's
/// mail, the head names the account.
fn message_card<'a>(data: &MailList<'a>, message: &'a MessageSummary, width: f32) -> Element<'a, Message> {
    let mut chips = Vec::new();
    if let Some(invite) = &message.invite {
        chips.extend(invite_chips(invite, data.now));
    }
    if message.has_attachments {
        chips.push(widgets::state_pill(Icon::Paperclip, tr("Attachments")));
    }
    if message.flags.contains(&Flag::Flagged) {
        chips.push(widgets::state_pill(Mark::Tinted(Icon::Flag, data.attention), tr("Flagged")));
    }
    let (buttons, more) = message_buttons(message, data.entries, &data.hidable, data.folders, data.now);
    let (name, domain) = sender_parts(message.from.as_ref());
    let account = data.accounts.iter().position(|account| *account == address(&message.account));
    let smart = matches!(data.view, View::Attachments | View::Invitations);
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
    };
    let selected = data.selected == Some(message.id.as_str());
    let buttons = data.anchors.mark(selected, buttons);
    let more = data.anchors.mark(selected, more.into_iter().collect()).pop();
    let card =
        list::MailCard { lines, chips, buttons, more, selected, on_press: Message::SelectMail(message.id.clone()) };
    let card = list::mail_card(card, width);
    if selected { widgets::anchored(&data.anchors.row, card) } else { card }
}

/// The buttons of the card (and the action bar of the row) of `message` at `now`, with "…" as the
/// second part:
///
/// - a message: Archive (`e`, what "done" is in a folder) and Delete (`#`, into the trash) as far as
///   its account has those folders ([`filing_actions`], issue #118), then Task (`t`,
///   `tasks.create_todo_from_mail`, task E8.7b); a message has no "Later" of its own, and no "…".
///   No Reply: from a list the mail is not read yet – Reply stands in the reader;
/// - an invitation whose event is not over (task E8.3b): Accept · Maybe · Decline
///   ([`Message::AnswerInvitation`]) · Later · Task, as the same mail under "For me" (task E8.11):
///   "Later" and "…" ("Hide from inbox", while `hidable` lists the entry) act on its entry in
///   `entries` ([`entry_of`], `Message::Act` with its id); without an open entry, only the answers
///   and Task.
pub fn message_buttons<'a>(
    message: &MessageSummary,
    entries: &[Item],
    hidable: &[&str],
    folders: &[Folder],
    now: DateTime<Utc>,
) -> (Vec<RowAction<'a, Message>>, Option<RowAction<'a, Message>>) {
    let id = || message.id.clone();
    // `Act`, as `t` at the row: the dialog then opens at this button, not at the reader's (issue #122).
    let task = RowAction::new(Icon::ListTodo, tr("Task"), "T", Message::Act(id(), Key::MakeTask));
    if message.invite.as_ref().is_none_or(|invite| invite_over(invite, now)) {
        let mut buttons = filing_actions(folders, &message.account, Some(&message.folder), &message.id, "E");
        buttons.push(task);
        return (buttons, None);
    }
    let answer = |(icon, word, answer): (Icon, &'static str, InvitationAnswer)| {
        RowAction::new(icon, word, "", Message::AnswerInvitation(message.account.clone(), id(), answer))
    };
    let mut buttons: Vec<_> = answers().into_iter().map(answer).collect();
    let entry = entry_of(entries, &message.id);
    let act = |item: &Item, command| Message::Act(item.id.clone(), command);
    buttons.extend(entry.map(|item| RowAction::new(Icon::Clock, tr("Later"), "S", act(item, Key::Snooze))));
    buttons.push(task);
    let more = entry
        .filter(|item| hidable.contains(&item.id.as_str()))
        .map(|item| RowAction::new(Icon::Ellipsis, tr("Hide from inbox"), "M", act(item, Key::Hide)));
    (buttons, more)
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

/// The mark of an invitation in a row's first line, with its width: the calendar's icon, "Invitation ·
/// Tue 20 Oct · 12:00–13:00" as tooltip. Rows of folder messages and of entries (task E8.3c) alike.
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

/// The search above a folder's messages: a field (`Enter` searches, `/` and the sidebar's search
/// button focus it) and, while a search is shown, "Clear search".
pub fn search_bar<'a>(input: &str, folder: &FolderView) -> Element<'a, Message> {
    let field = text_input(&trf("Search {folder}", &[("folder", &folder.name)]), input)
        .id(SEARCH)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        .on_input(Message::SearchInput)
        .on_submit(Message::Search);
    let mut bar = row![field].spacing(space::s()).align_y(Alignment::Center);
    if folder.query.is_some() || !input.is_empty() {
        bar = bar.push(widgets::icon_button(Icon::X, tr("Clear search"), Some(Message::ClearSearch)));
    }
    container(bar).width(Fill).padding(Padding::new(space::s()).left(space::l()).right(space::l())).into()
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
    /// The colour of links (the text colour, underlined: never Torro red).
    pub link: Color,
    /// The attention colour, for the flag.
    pub attention: Color,
    /// The message lies in the trash: it is not deleted again from here.
    pub in_trash: bool,
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
}

/// The panel of a folder's message: the account and folder as path, the subject as title, the
/// mail as body ([`body`]). No footer: the mail's actions stand above its text.
pub fn panel<'a>(reading: &Reading<'a>, folder: Option<&'a FolderView>) -> panel::Panel<'a, Message> {
    let mark = Some(Mark::Icon(folder_icon(folder.and_then(|folder| folder.role))));
    let parts: Vec<String> = folder.map(|f| vec![address(&f.account).to_owned(), f.name.clone()]).unwrap_or_default();
    panel::panel(panel::path(mark, parts), panel::title(reading.title()), body(reading), Message::CloseDetail)
}

/// The mail in the panel: who wrote it to whom and when, the actions (reply, reply all, forward,
/// archive, delete, flag, read), the attachments, the text with its links, and the notes that the
/// text comes from outside and – for HTML – that nothing from the web was loaded.
pub fn body<'a>(reading: &Reading<'a>) -> Element<'a, Message> {
    let mut body = column![].spacing(space::l());
    if let Some(summary) = reading.summary() {
        body = body.push(panel::inset(header(reading, summary)));
        body = body.push(panel::inset(actions(reading, summary)));
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
    match reading.loaded {
        None => body = body.push(panel::inset(note(tr("Loading the message…")))),
        Some(Err(why)) => {
            let what = typo::text(typo::body(), tr("Couldn't load the message.")).style(theme::text_2);
            let why = typo::text(typo::meta(), why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
            body = body.push(panel::inset(column![what, why].spacing(space::xs())));
        }
        Some(Ok(mail)) => {
            let files: Vec<&AttachmentInfo> = mail.attachments.iter().filter(|a| !a.inline).collect();
            if !files.is_empty() {
                let rows = files.into_iter().map(|file| attachment_row(reading.id, file)).collect();
                body = body.push(widgets::prop_section(tr("Attachments"), rows));
            }
            body = body.push(panel::inset(text_block(mail, reading.link, text_level(reading.read))));
        }
    }
    body.into()
}

/// The level of a mail's text: the reading level in the comfortable mail window, else `BODY`.
fn text_level(read: bool) -> typo::Level {
    if read { typo::read() } else { typo::body() }
}

/// The entry a mail in the reader belongs to (task E8.3): its buttons Done, Later and Task act on
/// it, as `e`, `s` and `t` do.
#[derive(Debug, Clone, Copy)]
pub struct ReaderEntry<'a> {
    pub item: &'a Item,
    /// "Hide from inbox" has entries for it (`App::hide_choices`): the reader shows its "…".
    pub hide: bool,
}

/// The reader of the wide mail window (task E8.3), beside the list: the mail's actions at the top
/// ([`reader_actions`]), the subject (`HEADLINE`, in compact `TITLE`), the sender with the large
/// avatar, to whom and when – and for an entry a rule put in its lane why (issue #128) –, then the
/// text – in the reading level ([`Reading::read`]) – with its
/// notes and the attachments. At most [`size::content_max()`] wide, with the page's padding,
/// scrolling. `entry` is the mail entry the mail belongs to, `None` for a folder's message;
/// `anchors` are where the menus of its buttons open (the app's window).
pub fn reader<'a>(
    reading: Reading<'a>,
    entry: Option<ReaderEntry<'a>>,
    anchors: detail::Anchors,
    comfortable: bool,
) -> Element<'a, Message> {
    responsive(move |bounds| {
        let pad = layout::page_padding();
        let room = (bounds.width - pad.left - pad.right).clamp(0.0, size::content_max());
        let content = reader_content(&reading, entry, &anchors, comfortable, room);
        let page = container(content).padding(pad);
        scrollable(page)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill)
            .into()
    })
    .into()
}

/// The reader's content `room` wide ([`reader`]).
fn reader_content<'a>(
    reading: &Reading<'a>,
    entry: Option<ReaderEntry<'a>>,
    anchors: &detail::Anchors,
    comfortable: bool,
    room: f32,
) -> Element<'a, Message> {
    let gap = if comfortable { layout::section_gap() } else { space::l() };
    let mut body = column![].spacing(gap).width(room);
    let Some(summary) = reading.summary() else {
        return body.push(note(tr("Loading the message…"))).into();
    };
    body = body.push(reader_actions(reading, summary, entry, anchors, comfortable, room));
    let level = if comfortable { typo::headline() } else { typo::title() };
    body = body.push(typo::text(level, subject(summary)).style(theme::text).wrapping(Wrapping::WordOrGlyph));
    let sender = reader_sender(reading, summary, comfortable, room);
    // Why a rule put the mail's entry in its lane (issue #128), quietly right under who sent it.
    match entry.map(|entry| entry.item).filter(|item| item.reason.is_some()) {
        Some(item) => {
            let why = typo::text(typo::meta(), detail::lane_text(item)).style(theme::text_3);
            body = body.push(column![sender, why.wrapping(Wrapping::WordOrGlyph)].spacing(space::s()));
        }
        None => body = body.push(sender),
    }
    // What the mail invites to, with its answers (task E8.3b): over the task and the text.
    if let Some(invite) = &summary.invite {
        body =
            body.push(invite_card(invite, summary, reading.now, reading.answering, reading.invite, reading.attention));
    }
    // The mail knows its task (task E8.7b): between who wrote it and what it says, as the design.
    if let Some(task) = reading.task {
        body = body.push(task_card(task, reading.now));
    }
    if !comfortable {
        body = body.push(rule::horizontal(size::HAIRLINE).style(style::rule));
    }
    match reading.loaded {
        None => body = body.push(note(tr("Loading the message…"))),
        Some(Err(why)) => {
            let what = typo::text(typo::body(), tr("Couldn't load the message.")).style(theme::text_2);
            let why = typo::text(typo::meta(), why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
            body = body.push(column![what, why].spacing(space::xs()));
        }
        Some(Ok(mail)) => {
            body = body.push(text_block(mail, reading.link, text_level(reading.read)));
            let files: Vec<&AttachmentInfo> = mail.attachments.iter().filter(|a| !a.inline).collect();
            if !files.is_empty() {
                let cards = files.into_iter().map(|file| attachment_card(reading.id, file));
                body = body.push(column(cards).spacing(space::s()));
            }
        }
    }
    body.into()
}

/// The sender of the mail in the reader `room` wide: the avatar (large in comfortable, as a row's
/// in compact), the name with the address quietly after it, "to …" with the account below, and
/// when at the right. Names and addresses that do not fit end in "…".
fn reader_sender<'a>(
    reading: &Reading<'a>,
    summary: &'a MessageSummary,
    comfortable: bool,
    room: f32,
) -> Element<'a, Message> {
    let name = sender_name(summary.from.as_ref());
    let edge = if comfortable { size::avatar_lg() } else { list::mark_width() };
    let face = list::avatar(&name, reading.account, false, edge);
    let when = when(summary.date, reading.now);
    let words_room = (room - edge - typo::text_width(&when, typo::meta()) - 2.0 * space::m()).max(0.0);
    let name = typo::ellipsize(&name, typo::body_strong(), words_room);
    let left = words_room - typo::text_width(&name, typo::body_strong()) - space::s();
    let mut who = row![typo::text(typo::body_strong(), name).style(theme::text).wrapping(Wrapping::None)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if let Some(from) = &summary.from {
        let address = typo::ellipsize(&list::one_line(&from.address), typo::meta(), left);
        who = who.push(typo::text(typo::meta(), address).style(theme::text_3).wrapping(Wrapping::None));
    }
    let mut to = vec![address(&summary.account).to_owned()];
    if !summary.to.is_empty() {
        to.insert(0, trf("to {names}", &[("names", &names(&summary.to))]));
    }
    let to = typo::ellipsize(&to.join(" · "), typo::meta(), words_room);
    let to = typo::text(typo::meta(), to).style(theme::text_2).wrapping(Wrapping::None);
    let words = column![container(who).clip(true), container(to).clip(true)].width(Fill);
    let when = typo::text(typo::meta().tabular(), when).style(theme::text_3).wrapping(Wrapping::None);
    row![face, words, when].spacing(space::m()).align_y(Alignment::Center).into()
}

/// One button of the reader's actions ([`reader_actions`]).
struct ReaderButton {
    icon: Icon,
    label: String,
    message: Option<Message>,
    /// Where its menu opens (Later, Task, Move to …, "…").
    anchor: Option<Anchor>,
}

impl ReaderButton {
    fn new(icon: Icon, label: impl Into<String>, message: Option<Message>) -> Self {
        ReaderButton { icon, label: label.into(), message, anchor: None }
    }

    fn at(self, anchor: &Anchor) -> Self {
        ReaderButton { anchor: Some(anchor.clone()), ..self }
    }

    /// The button with its word in `style`, or its icon alone.
    fn view<'a>(self, words: bool, style: widgets::ButtonStyle) -> Element<'a, Message> {
        let button = if words {
            widgets::text_button(Some(self.icon), self.label, style, self.message)
        } else {
            widgets::icon_button(self.icon, self.label, self.message)
        };
        match self.anchor {
            Some(anchor) => widgets::anchored(&anchor, button),
            None => button,
        }
    }

    /// Its width with its word: the button's padding, the icon, the gap and the word.
    fn width(&self) -> f32 {
        let pad = style::button_padding();
        pad.left + pad.right + icon::SIZE_SM + space::xs() + typo::text_width(&self.label, typo::caption())
    }
}

/// The reader's actions in one row (task E8.3): Reply (the window's one primary button), Reply
/// all, Forward | Done, Later, Task on the mail's entry – "Archive" and "Task" (task E8.7b,
/// `tasks.create_todo_from_mail`) in their place for a folder's message, where `e` archives –, then at the right as icons Delete, Flag, read or unread, for an
/// entry also Archive, "Move to …" and "…" (Hide from inbox), and Close. In comfortable the first
/// six carry their words while they fit into `room`, else every button is its icon with the word
/// as tooltip.
fn reader_actions<'a>(
    reading: &Reading<'a>,
    summary: &MessageSummary,
    entry: Option<ReaderEntry<'a>>,
    anchors: &detail::Anchors,
    comfortable: bool,
    room: f32,
) -> Element<'a, Message> {
    let id = reading.id;
    let mail = |action: MailAction| Some(Message::Mail(action, id.to_owned()));
    let write = [
        ReaderButton::new(Icon::Reply, tr("Reply"), mail(MailAction::Reply)),
        ReaderButton::new(Icon::ReplyAll, tr("Reply all"), mail(MailAction::ReplyAll)),
        ReaderButton::new(Icon::Forward, tr("Forward"), mail(MailAction::Forward)),
    ];
    let flagged = summary.flags.contains(&Flag::Flagged);
    let read = summary.flags.contains(&Flag::Seen);
    let delete = (!reading.in_trash).then(|| Message::Mail(MailAction::Delete, id.to_owned()));
    let flag = if flagged { tr("Unflag") } else { tr("Flag") };
    let mut triage = Vec::new();
    let mut tools = vec![
        ReaderButton::new(Icon::Trash2, tr("Delete"), delete),
        ReaderButton::new(Icon::Flag, flag, mail(MailAction::Flag(!flagged))),
        if read {
            ReaderButton::new(Icon::Mail, tr("Mark unread"), mail(MailAction::Read(false)))
        } else {
            ReaderButton::new(Icon::MailOpen, tr("Mark read"), mail(MailAction::Read(true)))
        },
    ];
    match entry {
        Some(ReaderEntry { item, hide }) => {
            let item_id = || item.id.clone();
            let done = matches!(item.state, State::Done { .. });
            triage.push(if done {
                ReaderButton::new(Icon::Inbox, tr("Reopen"), Some(Message::SetDone(item_id(), false)))
            } else {
                ReaderButton::new(Icon::Check, tr("Done"), Some(Message::SetDone(item_id(), true)))
            });
            if !done {
                let later = ReaderButton::new(Icon::Clock, tr("Later"), Some(Message::AskSnooze(item_id())));
                triage.push(later.at(&anchors.snooze));
            }
            let task = ReaderButton::new(Icon::ListTodo, tr("Task"), Some(Message::AskTask(item_id())));
            triage.push(task.at(&anchors.task));
            tools.insert(0, ReaderButton::new(Icon::Archive, trc("action", "Archive"), mail(MailAction::Archive)));
            let lane = crate::app::other_lane(item.lane);
            let move_to =
                ReaderButton::new(crate::app::lane_icon(lane), lane_word(lane), Some(Message::AskLane(item_id())));
            tools.push(move_to.at(&anchors.lane));
            if hide {
                let more = ReaderButton::new(Icon::Ellipsis, tr("Hide from inbox"), Some(Message::AskHide(item_id())));
                tools.push(more.at(&anchors.hide));
            }
        }
        None => {
            triage.push(ReaderButton::new(Icon::Archive, trc("action", "Archive"), mail(MailAction::Archive)));
            // A folder's message becomes a task of its own (task E8.7b, `tasks.create_todo_from_mail`).
            let task = ReaderButton::new(Icon::ListTodo, tr("Task"), Some(Message::AskMailTask(id.to_owned())));
            triage.push(task.at(&anchors.task));
        }
    }
    tools.push(ReaderButton::new(Icon::X, tr("Close"), Some(Message::CloseDetail)));

    let count = write.len() + triage.len() + tools.len();
    let gaps = count as f32 * space::xs() + space::l();
    let spoken: f32 = write.iter().chain(&triage).map(ReaderButton::width).sum();
    let words = comfortable && spoken + tools.len() as f32 * size::control_sm() + gaps <= room;
    let mut bar = row![].spacing(space::xs()).align_y(Alignment::Center);
    for (i, button) in write.into_iter().enumerate() {
        let style = if i == 0 { style::button::primary } else { style::button::secondary };
        bar = bar.push(button.view(words, style));
    }
    let line = container(rule::vertical(size::HAIRLINE).style(style::rule)).height(typo::line_height(typo::body()));
    bar = bar.push(container(line).padding(Padding::new(0.0).left(space::xs()).right(space::xs())));
    for button in triage {
        bar = bar.push(button.view(words, style::button::ghost));
    }
    bar = bar.push(iced::widget::space::horizontal());
    for button in tools {
        bar = bar.push(button.view(false, style::button::ghost));
    }
    bar.into()
}

/// "Move to Info" or "Move to For me": the reader's button that moves the entry to `lane`.
fn lane_word(lane: Lane) -> String {
    let name = match lane {
        Lane::ForMe => tr("For me"),
        Lane::Info => tr("Info"),
    };
    trf("Move to {lane}", &[("lane", &name)])
}

/// An attachment in the reader (task E8.3): a card with the file's tile ([`widgets::source_tile`],
/// large), its name and size, and "Open", which fetches and opens it as a click on the panel's row
/// does ([`open_attachment`]).
fn attachment_card<'a>(id: &str, file: &AttachmentInfo) -> Element<'a, Message> {
    let name = file.filename.as_deref().map(list::one_line).filter(|name| !name.is_empty());
    let name = name.unwrap_or_else(|| tr("Attachment").to_owned());
    let tile = widgets::source_tile(widgets::TileFace::Icon(Icon::Paperclip), widgets::TileSize::Large, None);
    let words = column![
        container(typo::text(typo::body_strong(), name).style(theme::text).wrapping(Wrapping::None)).clip(true),
        typo::text(typo::meta().tabular(), size_text(file.size)).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .width(Fill);
    let open = Message::OpenAttachment(id.to_owned(), file.part.clone());
    let open = widgets::text_button(None, tr("Open"), style::button::ghost, Some(open));
    let line = row![tile, words, open].spacing(space::m()).align_y(Alignment::Center);
    widgets::card(vec![line.into()], false, None)
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

/// The mail's actions in a row: "Reply" with its words, reply all and forward as icons, then at
/// the right archive, delete, flag and read as icons. Delete is off in the trash.
fn actions<'a>(reading: &Reading<'a>, summary: &MessageSummary) -> Element<'a, Message> {
    let id = reading.id;
    let act = |action: MailAction| Some(Message::Mail(action, id.to_owned()));
    let flagged = summary.flags.contains(&Flag::Flagged);
    let read = summary.flags.contains(&Flag::Seen);
    let (read_icon, read_label) =
        if read { (Icon::Mail, tr("Mark unread")) } else { (Icon::MailOpen, tr("Mark read")) };
    let delete = (!reading.in_trash).then(|| Message::Mail(MailAction::Delete, id.to_owned()));
    // "Task" for a folder's message (task E8.7b); an entry's panel has "Make task" in its footer. Only
    // a folder's panel shows the date (`dated`), so it tells the two apart.
    let task = reading.dated.then(|| Message::AskMailTask(id.to_owned()));
    let mut tools = row![].spacing(space::xs()).align_y(Alignment::Center);
    if task.is_some() {
        tools = tools.push(widgets::icon_button(Icon::ListTodo, tr("Task"), task));
    }
    row![
        widgets::text_button(Some(Icon::Reply), tr("Reply"), style::button::secondary, act(MailAction::Reply)),
        widgets::icon_button(Icon::ReplyAll, tr("Reply all"), act(MailAction::ReplyAll)),
        widgets::icon_button(Icon::Forward, tr("Forward"), act(MailAction::Forward)),
        iced::widget::space::horizontal(),
        tools,
        widgets::icon_button(Icon::Archive, trc("action", "Archive"), act(MailAction::Archive)),
        widgets::icon_button(Icon::Trash2, tr("Delete"), delete),
        widgets::icon_button(
            Icon::Flag,
            if flagged { tr("Unflag") } else { tr("Flag") },
            act(MailAction::Flag(!flagged))
        ),
        widgets::icon_button(read_icon, read_label, act(MailAction::Read(!read))),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center)
    .into()
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

/// The text in `level` with its web addresses as links ([`pieces`]: the whole address shows,
/// underlined, and opens on a click), then the notes on foreign text.
fn text_block<'a>(mail: &'a Mail, link: Color, level: typo::Level) -> Element<'a, Message> {
    let text = mail.text.as_deref().map(list::plain).unwrap_or_default();
    let mut block = column![].spacing(space::s());
    if !text.trim().is_empty() {
        let spans: Vec<iced::widget::text::Span<'a, String>> = pieces(&text)
            .into_iter()
            .map(|piece| match piece {
                Piece::Text(words) => span(words.to_owned()),
                Piece::Link(url) => span(url.to_owned()).color(link).underline(true).link(url.to_owned()),
            })
            .collect();
        let message = rich_text(spans)
            .size(level.size)
            .line_height(level.line_height)
            .font(level.font)
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph)
            .on_link_click(Message::OpenMailLink);
        block = block.push(message);
    }
    block = block.push(note(detail::foreign_note()));
    if mail.from_html {
        block = block.push(note(tr("Made from HTML. Pictures and other content from the web are not loaded.")));
    }
    // No title over the text: the headers above already say whose mail it is (and "Message" reads
    // as a notice in German, the entries' word).
    block.into()
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
    fn a_search_is_part_of_the_view() {
        let folders = folders();
        let inbox = FolderView::of(first_inbox(&folders).expect("an inbox"));
        assert_eq!(inbox.searching("  "), inbox, "blank: the whole folder");
        let searched = inbox.searching(" budget ");
        assert_eq!(searched.query.as_deref(), Some("budget"));
        assert_ne!(searched, inbox, "another search, another view");
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

    #[test]
    fn web_addresses_are_links_and_nothing_else_is() {
        let text = "See https://intranet.example.org/budget/q4. Or (http://a.example/x), mailto:me@x.example, \
                    xhttps://no.example and https:// alone.";
        let links: Vec<&str> = pieces(text)
            .into_iter()
            .filter_map(|piece| match piece {
                Piece::Link(url) => Some(url),
                Piece::Text(_) => None,
            })
            .collect();
        assert_eq!(links, ["https://intranet.example.org/budget/q4", "http://a.example/x"]);
        let whole: String = pieces(text)
            .into_iter()
            .map(|piece| match piece {
                Piece::Text(words) | Piece::Link(words) => words,
            })
            .collect();
        assert_eq!(whole, text, "nothing is lost or added");
        assert_eq!(pieces("HTTPS://Example.org"), [Piece::Link("HTTPS://Example.org")]);
        assert_eq!(pieces(""), []);
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
        let views = [View::Folder(folder.clone()), View::Attachments, View::Mail];
        let looks = [Look::Rows, Look::Cards];
        let all = iced::futures::executor::block_on(torro_inbox::InboxSource::list_items(
            &DemoSource::new(now()),
            Default::default(),
        ))
        .expect("the demo answers");
        for accounts in [Vec::new(), vec!["anna@work.example.org", "anna@example.org"]] {
            for (view, look) in views.iter().flat_map(|view| looks.map(|look| (view, look))) {
                for messages in [&summaries[..], &[]] {
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
                        hidable: all.iter().map(|item| item.id.as_str()).collect(),
                        folders: &folders,
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
            for loaded in [None, Some(&loaded)] {
                for read in [false, true] {
                    let reading = Reading {
                        task,
                        id: &message.summary.id,
                        summary: Some(&message.summary),
                        loaded,
                        now: now(),
                        link: Color::BLACK,
                        attention: Color::BLACK,
                        in_trash: false,
                        dated: true,
                        read,
                        account: Some(0),
                        answering: false,
                        invite: InviteSide::default(),
                    };
                    let _ = panel(&reading, Some(&folder));
                    for entry in [None, Some(ReaderEntry { item: &entry, hide: true })] {
                        let _ = reader(reading, entry, detail::Anchors::default(), read);
                    }
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
        let (buttons, more) = message_buttons(&message, &entries, &[entry.id.as_str()], &folders, now());
        assert_eq!(words(&buttons), [tr("Accept"), tr("Maybe"), tr("Decline"), tr("Later"), tr("Task")]);
        assert!(matches!(&buttons[3].on_press, Message::Act(id, Key::Snooze) if *id == entry.id));
        assert!(matches!(more.map(|more| more.on_press), Some(Message::Act(id, Key::Hide)) if id == entry.id));
        let (_, more) = message_buttons(&message, &entries, &[], &folders, now());
        assert!(more.is_none(), "nothing to hide: no \"…\"");
        let (alone, more) = message_buttons(&message, &[], &[], &folders, now());
        assert_eq!(words(&alone), [tr("Accept"), tr("Maybe"), tr("Decline"), tr("Task")], "no entry: no Later");
        assert!(more.is_none());
        let done = Item { state: State::Done { at: now() }, ..entry.clone() };
        assert_eq!(message_buttons(&message, &[done], &[], &folders, now()).0.len(), 4, "a done entry is put away");
        let plain = MessageSummary { invite: None, ..message.clone() };
        let words_of = words(&message_buttons(&plain, &entries, &[], &folders, now()).0);
        assert_eq!(words_of, [trc("action", "Archive"), tr("Delete"), tr("Task")]);
    }

    /// Issue #118: a message's row and card file it – Archive (`e`) and Delete (`#`) send what the
    /// keys send – and never offer Reply, which stands in the reader only. A button the account
    /// cannot carry out is not there: no archive folder, no Archive; in the trash, no Delete.
    #[test]
    fn a_message_is_archived_and_deleted_from_its_row_never_replied_to() {
        let message = MessageSummary { invite: None, ..demo::mail_sample(now()).remove(0).summary };
        let folders = folders();
        let (buttons, more) = message_buttons(&message, &[], &[], &folders, now());
        let sent: Vec<&Message> = buttons.iter().map(|button| &button.on_press).collect();
        assert!(matches!(sent[0], Message::Mail(MailAction::Archive, id) if *id == message.id), "{sent:?}");
        assert!(matches!(sent[1], Message::Mail(MailAction::Delete, id) if *id == message.id), "{sent:?}");
        assert!(matches!(sent[2], Message::Act(id, Key::MakeTask) if *id == message.id), "{sent:?}");
        assert_eq!(buttons.iter().map(|button| button.key).collect::<Vec<_>>(), ["E", "#", "T"]);
        assert!(more.is_none());
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
}
