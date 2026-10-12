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
//! - [`list`] draws a folder's messages, [`body`] the whole mail for the panel.
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

use chrono::{DateTime, Local, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, responsive, rich_text, row, scrollable, span, svg, text_input};
use iced::{Alignment, Color, Element, Fill, Padding, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::panel;
use torro_gui::source;
use torro_gui::theme::{self, palette, space};
use torro_gui::widgets::{self, Anchor, Mark};
use torro_gui::{datetime, style, typo};
use torro_inbox::InboxError;
use torro_inbox::demo::DemoSource;
use torro_ipc::inbox::InboxClient;
use torro_ipc::mail::{MoveRole, MoveTarget};
use torro_mail::{AttachmentInfo, Flag, Folder, MailAddress, Message as Mail, MessageSummary, Role};

use crate::app::{INBOX, Message};
use crate::list;

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

    /// `mail.search_messages` in the folder `folder` (a [`Folder::id`]): at most `limit` messages,
    /// newest first, holding every word of `query` when there is one, from `cursor` on (the
    /// [`MailPage::next`] of the page before).
    fn search_mail(
        &self,
        folder: String,
        query: Option<String>,
        limit: u32,
        cursor: Option<String>,
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
    ) -> Result<MailPage, InboxError> {
        let (messages, next) = InboxClient::search_mail(self, folder, query, limit, cursor).await?;
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
    ) -> Result<MailPage, InboxError> {
        let (messages, next) = DemoSource::search_mail(self, &folder, query.as_deref(), limit, cursor.as_deref())?;
        Ok(MailPage { messages, next })
    }

    async fn get_mail(&self, id: String) -> Result<Mail, InboxError> {
        DemoSource::get_mail(self, &id)
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
    /// The folder shown; `None` while a window of mail waits for its folders.
    pub folder: Option<&'a FolderView>,
    /// The mail accounts have no folders at all (no account connected).
    pub no_accounts: bool,
    /// Id of the message under the cursor.
    pub selected: Option<&'a str>,
    pub now: DateTime<Utc>,
    /// Registered by the selected row.
    pub anchor: Anchor,
    /// Two lines per row (narrow windows).
    pub two_line: bool,
    /// More messages wait on the server: "Load more" ends the list.
    pub more: bool,
    /// The attention colour of the theme, for the flag.
    pub attention: Color,
}

/// A folder's messages, scrolling, with "Load more" at the end when there are more; the empty
/// state of the folder or the search when there are none.
pub fn list<'a>(data: MailList<'a>) -> Element<'a, Message> {
    if data.messages.is_empty() {
        let (which, title, hint) = match data.folder {
            None if data.no_accounts => {
                (Icon::Mail, tr("No mail account yet."), tr("Connect one in Settings, under Accounts."))
            }
            None => return widgets::loading(),
            Some(folder) if folder.query.is_some() => (Icon::Search, tr("No messages found."), tr("Try other words.")),
            Some(folder) => {
                (folder_icon(folder.role), tr("No messages here."), tr("Mail in this folder shows up here."))
            }
        };
        return list::empty_state(which, title, hint);
    }
    responsive(move |bounds| rows(&data, bounds.width)).into()
}

/// The rows at `width`.
fn rows<'a>(data: &MailList<'a>, width: f32) -> Element<'a, Message> {
    let mut rows = column![].width(Fill);
    rows = rows.extend(data.messages.iter().map(|message| message_row(data, message, width)));
    if data.more {
        let more = widgets::text_button(None, tr("Load more"), style::button::ghost, Some(Message::MoreMail));
        rows = rows.push(container(more).width(Fill).center_x(Fill).padding(space::S));
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

/// One message in a row `width` wide. One line: the envelope (closed while unread), the sender in
/// `text_2`, the subject (medium weight while unread) and the start of the text in `text_3`, then
/// a paperclip, the flag and the time. Two lines: the sender with paperclip, flag and time, the
/// subject below it.
fn message_row<'a>(data: &MailList<'a>, message: &'a MessageSummary, width: f32) -> Element<'a, Message> {
    let unread = !message.flags.contains(&Flag::Seen);
    let mark = icon::icon(if unread { Icon::Mail } else { Icon::MailOpen }, icon::SIZE).style(move |t: &Theme, _| {
        let p = palette(t);
        svg::Style { color: Some(if unread { p.text } else { p.text_3 }) }
    });
    let level = if unread { typo::BODY_STRONG } else { typo::BODY };
    let time = list::ago(message.date, data.now);
    let mut ends: Vec<Element<'a, Message>> = Vec::new();
    let mut ends_width = 0.0;
    if message.has_attachments {
        ends.push(widgets::with_tooltip(small_icon(Icon::Paperclip, None), tr("Has attachments")));
        ends_width += icon::SIZE_SM + list::CELL_GAP;
    }
    if message.flags.contains(&Flag::Flagged) {
        ends.push(widgets::with_tooltip(small_icon(Icon::Flag, Some(data.attention)), tr("Flagged")));
        ends_width += icon::SIZE_SM + list::CELL_GAP;
    }
    let room = (list::text_room(width, None, &time) - ends_width).max(0.0);
    ends.push(typo::text(typo::META.tabular(), time).style(theme::text_3).wrapping(Wrapping::None).into());
    let sender = sender_name(message.from.as_ref());
    let selected = data.selected == Some(message.id.as_str());
    let on_press = Message::SelectMail(message.id.clone());
    let row = if data.two_line {
        let name = typo::text(level, typo::ellipsize(&sender, level, room)).style(theme::text).wrapping(Wrapping::None);
        let first = row![container(name).width(Fill).clip(true)].extend(ends).spacing(list::CELL_GAP);
        let line = typo::ellipsize(&subject(message), typo::META, list::line_room(width));
        let second = typo::text(typo::META, line).style(theme::text_2).wrapping(Wrapping::None);
        let words = column![first.align_y(Alignment::Center), container(second).width(Fill).clip(true)].width(Fill);
        let mark = container(mark).center_y(typo::line_height(level));
        list::tall_row(row![mark, words].spacing(list::CELL_GAP).into(), selected, on_press)
    } else {
        // The sender takes at most a third of the room, so the subject keeps the most of it.
        let name = typo::ellipsize(&sender, typo::BODY, room / 3.0);
        let left = (room - typo::text_width(&name, typo::BODY) - space::S).max(0.0);
        let snippet = Some(list::one_line(&message.snippet)).filter(|snippet| !snippet.is_empty());
        let (title, preview) = list::fit(&subject(message), snippet.as_deref(), level, left, None);
        let mut words = row![
            typo::text(typo::BODY, name).style(theme::text_2).wrapping(Wrapping::None),
            typo::text(level, title).style(theme::text).wrapping(Wrapping::None),
        ]
        .spacing(space::S);
        if let Some(preview) = preview {
            words = words.push(typo::text(typo::BODY, preview).style(theme::text_3).wrapping(Wrapping::None));
        }
        let mut cells: Vec<Element<'a, Message>> = vec![mark.into(), container(words).width(Fill).clip(true).into()];
        cells.extend(ends);
        widgets::list_row(cells, selected, Some(on_press))
    };
    if selected { widgets::anchored(&data.anchor, row) } else { row }
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
        .size(typo::BODY.size)
        .line_height(typo::BODY.line_height)
        .font(typo::BODY.font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        .on_input(Message::SearchInput)
        .on_submit(Message::Search);
    let mut bar = row![field].spacing(space::S).align_y(Alignment::Center);
    if folder.query.is_some() || !input.is_empty() {
        bar = bar.push(widgets::icon_button(Icon::X, tr("Clear search"), Some(Message::ClearSearch)));
    }
    container(bar).width(Fill).padding(Padding::new(space::S).left(space::L).right(space::L)).into()
}

/// What the panel shows of a mail.
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
    let mut body = column![].spacing(space::L);
    if let Some(summary) = reading.summary() {
        body = body.push(panel::inset(header(reading, summary)));
        body = body.push(panel::inset(actions(reading, summary)));
    }
    match reading.loaded {
        None => body = body.push(panel::inset(note(tr("Loading the message…")))),
        Some(Err(why)) => {
            let what = typo::text(typo::BODY, tr("Couldn't load the message.")).style(theme::text_2);
            let why = typo::text(typo::META, why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
            body = body.push(panel::inset(column![what, why].spacing(space::XS)));
        }
        Some(Ok(mail)) => {
            let files: Vec<&AttachmentInfo> = mail.attachments.iter().filter(|a| !a.inline).collect();
            if !files.is_empty() {
                let rows = files.into_iter().map(|file| attachment_row(reading.id, file)).collect();
                body = body.push(widgets::prop_section(tr("Attachments"), rows));
            }
            body = body.push(panel::inset(text_block(mail, reading.link)));
        }
    }
    body.into()
}

/// Sender, recipients and date, as mail apps show them: the sender's name and address, "to …",
/// "cc …", and the moment with how long ago it was; a flagged mail says so.
fn header<'a>(reading: &Reading<'a>, summary: &'a MessageSummary) -> Element<'a, Message> {
    let mut lines = column![].spacing(space::XS);
    let from = summary.from.as_ref().map_or_else(|| tr("Unknown sender").to_owned(), person);
    lines = lines.push(typo::text(typo::BODY_STRONG, from).style(theme::text).wrapping(Wrapping::WordOrGlyph));
    if !summary.to.is_empty() {
        let to = trf("to {names}", &[("names", &names(&summary.to))]);
        lines = lines.push(typo::text(typo::META, to).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    if let Some(Ok(mail)) = reading.loaded
        && !mail.cc.is_empty()
    {
        let cc = trf("cc {names}", &[("names", &names(&mail.cc))]);
        lines = lines.push(typo::text(typo::META, cc).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    let mut meta = row![].spacing(space::S).align_y(Alignment::Center);
    if reading.dated {
        meta = meta.push(typo::text(typo::META.tabular(), when(summary.date, reading.now)).style(theme::text_3));
    }
    if summary.flags.contains(&Flag::Flagged) {
        let flagged = row![small_icon(Icon::Flag, Some(reading.attention)), typo::text(typo::META, tr("Flagged"))]
            .spacing(space::XS)
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
    row![
        widgets::text_button(Some(Icon::Reply), tr("Reply"), style::button::secondary, act(MailAction::Reply)),
        widgets::icon_button(Icon::ReplyAll, tr("Reply all"), act(MailAction::ReplyAll)),
        widgets::icon_button(Icon::Forward, tr("Forward"), act(MailAction::Forward)),
        iced::widget::space::horizontal(),
        widgets::icon_button(Icon::Archive, tr("Archive"), act(MailAction::Archive)),
        widgets::icon_button(Icon::Trash2, tr("Delete"), delete),
        widgets::icon_button(
            Icon::Flag,
            if flagged { tr("Unflag") } else { tr("Flag") },
            act(MailAction::Flag(!flagged))
        ),
        widgets::icon_button(read_icon, read_label, act(MailAction::Read(!read))),
    ]
    .spacing(space::XS)
    .align_y(Alignment::Center)
    .into()
}

/// An attachment as a row: the paperclip, its name and size; a click fetches and opens it.
fn attachment_row<'a>(id: &str, file: &AttachmentInfo) -> Element<'a, Message> {
    let name = file.filename.as_deref().map(list::one_line).filter(|name| !name.is_empty());
    let name = name.unwrap_or_else(|| tr("Attachment").to_owned());
    let line = row![
        container(widgets::prop_value(name)).width(Fill),
        typo::text(typo::META.tabular(), size_text(file.size)).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);
    let open = Message::OpenAttachment(id.to_owned(), file.part.clone());
    widgets::prop_row(Some(Mark::Icon(Icon::Paperclip)), line, Some(open))
}

/// The text with its web addresses as links ([`pieces`]: the whole address shows, underlined, and
/// opens on a click), then the notes on foreign text.
fn text_block<'a>(mail: &'a Mail, link: Color) -> Element<'a, Message> {
    let text = mail.text.as_deref().map(list::plain).unwrap_or_default();
    let mut block = column![].spacing(space::S);
    if !text.trim().is_empty() {
        let spans: Vec<iced::widget::text::Span<'a, String>> = pieces(&text)
            .into_iter()
            .map(|piece| match piece {
                Piece::Text(words) => span(words.to_owned()),
                Piece::Link(url) => span(url.to_owned()).color(link).underline(true).link(url.to_owned()),
            })
            .collect();
        let message = rich_text(spans)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .font(typo::BODY.font)
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph)
            .on_link_click(Message::OpenMailLink);
        block = block.push(message);
    }
    block = block.push(note(tr("This text comes from outside the suite and is shown as plain text.")));
    if mail.from_html {
        block = block.push(note(tr("Made from HTML. Pictures and other content from the web are not loaded.")));
    }
    // No title over the text: the headers above already say whose mail it is (and "Message" reads
    // as a notice in German, the entries' word).
    block.into()
}

/// A quiet sentence (`META`, `text_3`).
fn note<'a>(text: &'a str) -> Element<'a, Message> {
    typo::text(typo::META, text).style(theme::text_3).wrapping(Wrapping::WordOrGlyph).into()
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
    use torro_inbox::demo;

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
        for two_line in [false, true] {
            let data = MailList {
                messages: &summaries,
                folder: Some(&folder),
                no_accounts: false,
                selected: Some("m:w1"),
                now: now(),
                anchor: Anchor::new(),
                two_line,
                more: true,
                attention: Color::BLACK,
            };
            let _ = list(data);
        }
        for message in &mail {
            let loaded = Ok(message.clone());
            for loaded in [None, Some(&loaded)] {
                let reading = Reading {
                    id: &message.summary.id,
                    summary: Some(&message.summary),
                    loaded,
                    now: now(),
                    link: Color::BLACK,
                    attention: Color::BLACK,
                    in_trash: false,
                    dated: true,
                };
                let _ = panel(&reading, Some(&folder));
            }
        }
    }
}
