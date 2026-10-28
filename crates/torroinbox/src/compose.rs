//! The compose window (task E4.3 in `docs/tasks/mail.md`, `docs/mail.md` "Schreiben in einem
//! eigenen Fenster"): `torroinbox --compose [--reply <id> | --reply-all <id> | --forward <id>]
//! [--account <id>] [--attach <path>]`, a window of its own with the class `torro-compose`, so a
//! tiling manager puts it beside the list and one can read while writing.
//!
//! - **A file along (task WZ.8):** `--attach <path>` attaches that file as the window opens, as if
//!   it had been chosen; the core starts `torroinbox --compose --attach=<path>` for "Send by mail"
//!   on a file card of the assistant (`apps.open_app` with `file:<id>`), with the path its register
//!   keeps. The person sees the mail with the file and sends it, or not.
//!
//! - **Fields:** From (the account; a choice with two or more), To, Cc and Bcc (unfolded on
//!   demand, or at once when an answer fills them), Subject, the text and the attachments. The
//!   account's signature is shown under the text but not put into it: the core adds it when the
//!   message is built, so it would stand twice.
//! - **Answers:** a reply goes to `Reply-To:` (else the sender), reply all adds everyone else on
//!   the message except the accounts of this computer, a forward fills no recipient and takes the
//!   attachments along as `DraftAttachment::MailPart`. The text quotes the message ("On … wrote:"
//!   and `> ` before every line); `in_reply_to` lets the core set the reply headers. Opened from
//!   the panel of the inbox ([`crate::mail::compose_command`]) with the message id; the message
//!   is fetched here (`mail.get_message`).
//! - **Nothing gets lost:** every change is saved as a draft a few seconds after the last one
//!   ([`AUTOSAVE`], `mail.save_draft`; the id it answers replaces the draft next time). Closing
//!   with unsaved changes asks first. "Send" saves what is not saved and then sends the draft
//!   (`mail.send_draft`); an error stays in the window with the text, the draft stays where it
//!   is, and one can try again. Only after a send that the server took the window closes.
//! - **Formatted mail** (issue #125, for accounts that write HTML, `MailSettings::format`): a
//!   toolbar over the text and the keys `Cmd+B`/`I`/`U`/`K` (`Ctrl` elsewhere) write the marks of
//!   [`torro_mail::markup`] (`**bold**`, `*italic*`, `__underline__`, `[text](address)`, `- ` and
//!   `1. ` lists, `> ` quotes); Enter in a list item starts the next one. The editor shows the marks
//!   quietly and quotes in the secondary colour ([`markup::looks`]). Saved, the text becomes
//!   the draft's HTML ([`markup::to_html`]); the quoted history of an answer stays someone else's
//!   text (only its quote marks count), and the core puts the signature above it.
//! - **Keys:** the shortcut "send" of the system's preset in `torro_core::shortcuts` (tasks E10.1,
//!   E10.1b) sends, also from a field: `Ctrl+Enter`, on macOS Apple Mail's `Shift+Cmd+D` and
//!   `Cmd+Enter` or `Ctrl+Enter`; `Esc` closes (asking when
//!   something is unsaved), `Tab` moves between the fields.
//! - **Sizes** ([`torro_gui::layout`]): wide and medium show the form with a header bar, narrow
//!   the same with a compact "Send" button; mini and strip are displays, not editors: subject,
//!   recipient and the state of the draft, no keys.
//!
//! The window talks to the core through [`ComposeSource`], one request per method (the core's
//! [`InboxClient`] and the demo implement it here). Mail is foreign text: what a quoted message
//! brings is cleaned of control and direction characters before it enters the text.

use std::future::Future;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use iced::advanced::text::highlighter::{self, Highlighter};
use iced::font::Style as FontStyle;
use iced::keyboard::{self, Key, key::Named};
use iced::theme::Mode;
use iced::widget::operation;
use iced::widget::text::Wrapping;
use iced::widget::text_editor::{self, Binding, Content, Cursor, Edit, Motion, Position};
use iced::widget::{Space, column, container, row, text_input};
use iced::{Alignment, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::shortcuts::{self, Preset, System};
use torro_gui::design::{self, Follow};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, Cover, Surface, TitleButtons};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{datetime, style, typo};
use torro_inbox::InboxError;
use torro_inbox::demo::DemoSource;
use torro_ipc::inbox::InboxClient;
use torro_ipc::mail::{Delivery, MAX_ATTACHMENTS, MAX_FILE_BYTES};
use torro_ipc::{AccountInfo, IpcError, Request, Response};
use torro_mail::{AttachmentInfo, Draft, DraftAttachment, MailAddress, MailFormat, Message as Mail, html, markup};

use crate::app::{Launch, LaunchError, flag_value};
use crate::{keys, list, mail};

/// The compose window's identity: window class `torro-compose` (rules of a tiling manager match
/// on it), the binary and state directory of the inbox.
pub const COMPOSE: AppId = AppId {
    window_class: "torro-compose",
    binary: AppId::INBOX.binary,
    dir: AppId::INBOX.dir,
    display_name: AppId::INBOX.display_name,
    // Not an entry of the switcher: the panel starts it with its own arguments (`mail::compose_args`).
    args: &[],
};

/// How long after the last change the draft is saved. Short enough to lose little, long enough
/// not to save at every key.
pub const AUTOSAVE: Duration = Duration::from_secs(3);

/// How often the window checks whether [`AUTOSAVE`] has passed. Only while something is unsaved.
const CLOCK: Duration = Duration::from_secs(1);

/// The size a compose window opens at: a letter, not the inbox's three columns.
const WINDOW: Size = Size::new(760.0, 700.0);

/// The width of the label column of the fields: "Subject" fits at the text size.
fn label_width() -> f32 {
    size::row() + space::l()
}

/// The id of the To field, which takes the focus when a new message opens.
const TO: &str = "compose-to";
/// The id of the Cc field, which takes the focus when it is unfolded.
const CC: &str = "compose-cc";
/// The id of the Bcc field, likewise.
const BCC: &str = "compose-bcc";
/// The id of the Subject field.
const SUBJECT: &str = "compose-subject";
/// The id of the text, which takes the focus when an answer opens (the quote is below the cursor).
const BODY: &str = "compose-body";

/// The window settings: [`torro_gui::window`] for [`COMPOSE`] at the size of a letter, and a
/// close request that the window answers itself, so a click on its close button asks before
/// unsaved text goes ([`Message::Close`]).
pub fn window_settings() -> iced::window::Settings {
    iced::window::Settings { size: WINDOW, exit_on_close_request: false, ..torro_gui::window(COMPOSE) }
}

// ---------------------------------------------------------------------------------------------
// Start parameters
// ---------------------------------------------------------------------------------------------

/// What the window writes: a new message, or an answer to the message with this id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    New,
    /// To the sender (`--reply <id>`).
    Reply(String),
    /// To everyone on the message (`--reply-all <id>`).
    ReplyAll(String),
    /// To pass it on (`--forward <id>`).
    Forward(String),
}

impl Kind {
    /// The message answered or passed on.
    pub fn message(&self) -> Option<&str> {
        match self {
            Kind::New => None,
            Kind::Reply(id) | Kind::ReplyAll(id) | Kind::Forward(id) => Some(id),
        }
    }

    /// The name of the window in its header and title.
    pub fn title(&self) -> &'static str {
        match self {
            Kind::New => tr("New message"),
            Kind::Reply(_) => tr("Reply"),
            Kind::ReplyAll(_) => tr("Reply all"),
            Kind::Forward(_) => tr("Forward"),
        }
    }

    /// What did not work when the accounts or the message could not be fetched.
    fn load_failed(&self) -> &'static str {
        match self {
            Kind::New => tr("Couldn't open the compose window."),
            Kind::Reply(_) | Kind::ReplyAll(_) | Kind::Forward(_) => tr("Couldn't load the message."),
        }
    }

    fn icon(&self) -> Icon {
        match self {
            Kind::New => Icon::SquarePen,
            Kind::Reply(_) => Icon::Reply,
            Kind::ReplyAll(_) => Icon::ReplyAll,
            Kind::Forward(_) => Icon::Forward,
        }
    }
}

/// The states `--demo --open <state>` starts the window in, for pictures that can be made again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// Cc and Bcc unfolded and filled.
    Cc,
    /// Two attachments.
    Attach,
    /// A send that failed.
    Error,
    /// A send whose outcome is unknown.
    Unsure,
    /// The question on closing with unsaved changes.
    Discard,
    /// The menu of the From field.
    Menu,
    /// The message to answer is still loading.
    Loading,
    /// The message to answer could not be loaded.
    Failed,
    /// Formatted text: bold, italic, a link, lists and a quote (issue #125).
    Format,
}

impl Open {
    const ALL: [(&'static str, Open); 9] = [
        ("cc", Open::Cc),
        ("attach", Open::Attach),
        ("error", Open::Error),
        ("unsure", Open::Unsure),
        ("discard", Open::Discard),
        ("menu", Open::Menu),
        ("loading", Open::Loading),
        ("failed", Open::Failed),
        ("format", Open::Format),
    ];
}

/// The start parameters of a compose window (`--compose` and what goes with it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    pub kind: Kind,
    /// `--account <address or mail:address>`: the account to write from, if the window has it.
    pub account: Option<String>,
    /// `--open <state>` with `--demo`.
    pub open: Option<Open>,
    /// `--attach <path>`: a file that goes along from the start (task WZ.8).
    pub attach: Option<PathBuf>,
}

impl Start {
    /// Reads `--compose`, `--reply <id>`, `--reply-all <id>`, `--forward <id>`, `--account <id>`,
    /// `--attach <path>` and `--open <state>` from `args`. `None` without `--compose`; then the
    /// flags that only make sense with it are refused, so a mistyped command never opens the inbox
    /// instead. An empty path is a missing value.
    pub fn parse(args: &[String], demo: bool) -> Result<Option<Start>, LaunchError> {
        let answers: [(&'static str, Answer); 3] =
            [("--reply", Kind::Reply), ("--reply-all", Kind::ReplyAll), ("--forward", Kind::Forward)];
        let mut found = Vec::new();
        for (flag, make) in answers {
            if let Some(id) = flag_value(args, flag)? {
                if id.is_empty() {
                    return Err(LaunchError::MissingValue(flag));
                }
                found.push((flag, make(id.to_owned())));
            }
        }
        let account = flag_value(args, "--account")?;
        let attach = flag_value(args, "--attach")?;
        if attach == Some("") {
            return Err(LaunchError::MissingValue("--attach"));
        }
        if !args.iter().any(|arg| arg == "--compose") {
            let stray =
                found.first().map(|(flag, _)| *flag).or(account.map(|_| "--account")).or(attach.map(|_| "--attach"));
            return stray.map_or(Ok(None), |flag| Err(LaunchError::NeedsCompose(flag)));
        }
        let kind = match found.len() {
            0 => Kind::New,
            1 => found.remove(0).1,
            _ => return Err(LaunchError::Conflict(found[0].0, found[1].0)),
        };
        let open = match flag_value(args, "--open")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--open")),
            Some(word) => match Open::ALL.iter().find(|(name, _)| *name == word) {
                Some(&(_, open)) => Some(open),
                None => {
                    return Err(LaunchError::UnknownValue {
                        flag: "--open",
                        value: word.to_owned(),
                        valid: Open::ALL.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", "),
                    });
                }
            },
        };
        let account = account.map(|id| if id.starts_with("mail:") { id.to_owned() } else { format!("mail:{id}") });
        Ok(Some(Start { kind, account, open, attach: attach.map(PathBuf::from) }))
    }
}

/// The command that opens a new, empty compose window: this app's binary next to the running one
/// with `--compose` (and `--demo` when this window runs on the demo). Answers and forwards are
/// started by [`crate::mail::compose_command`].
pub fn new_args(demo: bool) -> Vec<String> {
    let mut args = vec!["--compose".to_owned()];
    if demo {
        args.push("--demo".to_owned());
    }
    args
}

/// [`new_args`] as a command.
pub fn new_command(demo: bool) -> std::process::Command {
    let mut command = std::process::Command::new(torro_gui::source::sibling(COMPOSE.binary));
    command.args(new_args(demo));
    command
}

// ---------------------------------------------------------------------------------------------
// The core, as the window uses it
// ---------------------------------------------------------------------------------------------

/// An account a message can be written from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    /// `mail:<address>`, the draft's `account`.
    pub account: String,
    /// The address.
    pub address: String,
    /// The name in `From:`; empty for none.
    pub name: String,
    /// What the core puts under the message.
    pub signature: Option<String>,
    /// The account writes formatted mail (`MailFormat::Html`, issue #125): the window shows the
    /// toolbar and saves the text as HTML too.
    pub html: bool,
}

impl Sender {
    /// `Name <address>`, or the address alone.
    pub fn label(&self) -> String {
        let name = list::one_line(&self.name);
        if name.is_empty() { self.address.clone() } else { format!("{name} <{}>", self.address) }
    }
}

/// The mail accounts of `accounts` (`accounts.list_accounts`) with mail switched on, in the
/// core's order.
pub fn senders_of(accounts: &[AccountInfo]) -> Vec<Sender> {
    accounts
        .iter()
        .filter(|account| account.kind == "mail")
        .filter_map(|account| {
            let view = account.mail.as_ref().filter(|view| view.settings.services.mail)?;
            Some(Sender {
                account: account.id.clone(),
                address: view.address.clone(),
                name: view.sender_name.clone(),
                signature: view
                    .settings
                    .signature
                    .as_ref()
                    .map(|signature| {
                        if view.settings.signature_format {
                            torro_mail::html::to_text(&torro_mail::markup::to_html(signature, None))
                        } else {
                            signature.clone()
                        }
                    })
                    .filter(|signature| !signature.trim().is_empty()),
                html: view.settings.format == MailFormat::Html,
            })
        })
        .collect()
}

/// Why a call of the window failed, as people read it ([`std::fmt::Display`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    /// The core does not answer.
    Offline,
    /// The message, draft or account is not there (`not-found`).
    NotFound,
    /// The rights do not cover it (`forbidden`).
    Forbidden,
    /// Anything else, with the core's own words.
    Other(String),
}

impl std::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComposeError::Offline => f.write_str(tr("Torro Workplace isn't responding. It may still be starting.")),
            ComposeError::NotFound => f.write_str(tr("The message or draft doesn't exist any more.")),
            ComposeError::Forbidden => f.write_str(tr("Torro Workplace doesn't allow this.")),
            ComposeError::Other(message) => f.write_str(message),
        }
    }
}

impl From<InboxError> for ComposeError {
    fn from(err: InboxError) -> ComposeError {
        match err {
            InboxError::NotFound => ComposeError::NotFound,
            InboxError::Forbidden => ComposeError::Forbidden,
            InboxError::Offline => ComposeError::Offline,
            other => ComposeError::Other(other.to_string()),
        }
    }
}

/// What the compose window can ask of the core. Every method is one request of the app key; the
/// core checks rights, the files a draft may attach and the send rule, the window never decides
/// them. (Not part of [`crate::mail::MailSource`]: that is the list's and the panel's.)
pub trait ComposeSource: Send + Sync {
    /// `accounts.list_accounts`, reduced to the mail accounts a message can be written from
    /// ([`senders_of`]). Empty without a mail account.
    fn senders(&self) -> impl Future<Output = Result<Vec<Sender>, ComposeError>> + Send;

    /// Preferred account for a new message. Sources without suite settings use the first sender.
    fn default_sender(&self) -> impl Future<Output = Result<Option<String>, ComposeError>> + Send {
        async { Ok(None) }
    }

    /// `mail.get_message` with its text: the message to answer or pass on.
    fn get_mail(&self, id: String) -> impl Future<Output = Result<Mail, ComposeError>> + Send;

    /// `mail.save_draft`: files `draft` in the account's drafts folder and answers it with its id.
    /// With the id of a saved draft it replaces that draft; `NotFound` when that is gone.
    fn save_draft(&self, draft: Draft) -> impl Future<Output = Result<Draft, ComposeError>> + Send;

    /// `mail.send_draft`: sends the saved draft `id` and files the copy. A server that refuses is
    /// an error whose text ends "Nothing was sent."; [`Delivery::Unknown`] is not: the connection
    /// broke after the message was handed over, and it is never sent again by itself.
    fn send_draft(&self, id: String) -> impl Future<Output = Result<Delivery, ComposeError>> + Send;

    /// `mail.delete_messages` for a draft that is not wanted any more (into the trash).
    fn discard_draft(&self, id: String) -> impl Future<Output = Result<(), ComposeError>> + Send;
}

/// Sends `request` over the socket of `client` and maps the answer: errors by their code, a core
/// that is not running as [`ComposeError::Offline`].
async fn ask(client: &InboxClient, request: Request) -> Result<Response, ComposeError> {
    match torro_ipc::client::call(&client.socket, &request).await {
        Ok(Response::Error { code, message }) => Err(match code.as_str() {
            "not-found" => ComposeError::NotFound,
            "forbidden" => ComposeError::Forbidden,
            _ => ComposeError::Other(message),
        }),
        Ok(response) => Ok(response),
        Err(IpcError::NotRunning) => Err(ComposeError::Offline),
        Err(err) => Err(ComposeError::Other(err.to_string())),
    }
}

fn unexpected(response: Response) -> ComposeError {
    ComposeError::Other(format!("unexpected answer: {response:?}"))
}

impl ComposeSource for InboxClient {
    async fn senders(&self) -> Result<Vec<Sender>, ComposeError> {
        match ask(self, Request::Accounts).await? {
            Response::Accounts { accounts } => Ok(senders_of(&accounts)),
            other => Err(unexpected(other)),
        }
    }

    async fn default_sender(&self) -> Result<Option<String>, ComposeError> {
        match ask(self, Request::Settings).await? {
            Response::Settings { settings } => Ok(settings.mail_sender),
            other => Err(unexpected(other)),
        }
    }

    async fn get_mail(&self, id: String) -> Result<Mail, ComposeError> {
        match ask(self, Request::GetMail { id, include_body: true, include_html: false }).await? {
            Response::MailMessage { message } => Ok(message),
            other => Err(unexpected(other)),
        }
    }

    async fn save_draft(&self, draft: Draft) -> Result<Draft, ComposeError> {
        match ask(self, Request::SaveDraft { draft }).await? {
            Response::MailDraft { draft } => Ok(draft),
            other => Err(unexpected(other)),
        }
    }

    async fn send_draft(&self, id: String) -> Result<Delivery, ComposeError> {
        match ask(self, Request::SendDraft { id }).await? {
            Response::MailSent { delivery } => Ok(delivery),
            // The app key is never asked to wait for a person; an answer like that is a bug of the
            // core, and nothing was sent.
            Response::Pending { .. } => {
                Err(ComposeError::Other(tr("The message waits for approval. Nothing was sent.").to_owned()))
            }
            other => Err(unexpected(other)),
        }
    }

    async fn discard_draft(&self, id: String) -> Result<(), ComposeError> {
        match ask(self, Request::DeleteMail { ids: vec![id], permanent: false }).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }
}

impl ComposeSource for DemoSource {
    async fn senders(&self) -> Result<Vec<Sender>, ComposeError> {
        Ok(self
            .mail_senders()
            .into_iter()
            .map(|sender| Sender {
                address: mail::address(&sender.account).to_owned(),
                account: sender.account,
                name: sender.name,
                signature: sender.signature,
                // The demo writes formatted mail, as accounts do unless the person chose plain text.
                html: true,
            })
            .collect())
    }

    async fn get_mail(&self, id: String) -> Result<Mail, ComposeError> {
        Ok(DemoSource::get_mail(self, &id)?)
    }

    async fn save_draft(&self, draft: Draft) -> Result<Draft, ComposeError> {
        Ok(DemoSource::save_draft(self, draft)?)
    }

    async fn send_draft(&self, id: String) -> Result<Delivery, ComposeError> {
        DemoSource::send_draft(self, &id)?;
        Ok(Delivery::Accepted)
    }

    async fn discard_draft(&self, id: String) -> Result<(), ComposeError> {
        Ok(DemoSource::discard_draft(self, &id)?)
    }
}

// ---------------------------------------------------------------------------------------------
// What an answer starts with
// ---------------------------------------------------------------------------------------------

/// A file or a part of another message that goes along.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attached {
    /// A file of this computer (`DraftAttachment::File`).
    File { path: PathBuf, name: String, size: u64 },
    /// An attachment of the message that is forwarded (`DraftAttachment::MailPart`).
    Part { message: String, part: String, name: String, size: u64 },
}

impl Attached {
    /// The file name to show.
    pub fn name(&self) -> &str {
        match self {
            Attached::File { name, .. } | Attached::Part { name, .. } => name,
        }
    }

    /// The size in bytes.
    pub fn size(&self) -> u64 {
        match self {
            Attached::File { size, .. } | Attached::Part { size, .. } => *size,
        }
    }

    /// The draft's form of it.
    pub fn draft(&self) -> DraftAttachment {
        match self {
            Attached::File { path, .. } => DraftAttachment::File { path: path.to_string_lossy().into_owned() },
            Attached::Part { message, part, .. } => {
                DraftAttachment::MailPart { message: message.clone(), part: part.clone() }
            }
        }
    }

    /// The file at `path` as an attachment. An error says in words why it cannot go: not a file,
    /// unreadable, or over the 25 MB the core accepts.
    pub fn file(path: &Path) -> Result<Attached, String> {
        let name = path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let meta = std::fs::metadata(path).map_err(|err| err.to_string())?;
        if !meta.is_file() {
            return Err(trf("{file} is not a file.", &[("file", &name)]));
        }
        if meta.len() > MAX_FILE_BYTES {
            let limit = mail::size_text(MAX_FILE_BYTES);
            return Err(trf("{file} is larger than {limit}.", &[("file", &name), ("limit", &limit)]));
        }
        Ok(Attached::File { path: path.to_path_buf(), name, size: meta.len() })
    }

    /// The attachment `info` of the message `message`, to forward it.
    fn part(message: &str, info: &AttachmentInfo) -> Attached {
        let name = info.filename.as_deref().map(list::one_line).filter(|name| !name.is_empty());
        Attached::Part {
            message: message.to_owned(),
            part: info.part.clone(),
            name: name.unwrap_or_else(|| tr("Attachment").to_owned()),
            size: info.size,
        }
    }
}

/// What the fields start with: empty for a new message, filled from the message for an answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Opening {
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub subject: String,
    /// Starts with two blank lines for the answer, then the quote.
    pub body: String,
    pub attachments: Vec<Attached>,
    /// The message answered (`Draft::in_reply_to`); a forward is no reply.
    pub in_reply_to: Option<String>,
    /// The line of `body` where the quoted history starts ("On … wrote:", or the first line of
    /// the forwarded header): from there on the text is someone else's ([`markup::to_html`]).
    pub history: Option<String>,
}

/// `Re: ` in front of `subject` (not twice, `AW:` counts as `Re:`).
pub fn reply_subject(subject: &str) -> String {
    prefixed(subject, "Re", &["re", "aw", "antw"])
}

/// `Fwd: ` in front of `subject` (not twice, `Fw:` and `WG:` count).
pub fn forward_subject(subject: &str) -> String {
    prefixed(subject, "Fwd", &["fwd", "fw", "wg"])
}

fn prefixed(subject: &str, prefix: &str, same: &[&str]) -> String {
    let subject = list::one_line(subject);
    let lower = subject.to_lowercase();
    if same.iter().any(|word| lower.strip_prefix(word).is_some_and(|rest| rest.starts_with(':'))) {
        subject
    } else {
        format!("{prefix}: {subject}").trim_end().to_owned()
    }
}

/// `text` as a quote: every line behind `> `, empty ones as a bare `>`, so a quote of a quote reads
/// `> > …`. Control and direction characters are dropped (the text is foreign), the ends trimmed.
pub fn quote(text: &str) -> String {
    list::plain(text)
        .replace("\r\n", "\n")
        .trim()
        .lines()
        .map(|line| if line.trim().is_empty() { ">".to_owned() } else { format!("> {}", line.trim_end()) })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A moment as the quote and the forwarded header name it: "Sunday, 11 October 2026 14:30" in the
/// user's time and language.
fn when(at: DateTime<Utc>) -> String {
    let local = datetime::to_local(at);
    format!("{} {}", datetime::format_long_date(local.date()), datetime::format_time(local.time()))
}

fn sender_of(message: &Mail) -> String {
    message.summary.from.as_ref().map_or_else(|| tr("Unknown sender").to_owned(), mail::person)
}

/// "On Sunday, 11 October 2026 14:30, Jonas Weber <jonas@…> wrote:".
pub fn attribution(message: &Mail) -> String {
    trf("On {date}, {who} wrote:", &[("date", &when(message.summary.date)), ("who", &sender_of(message))])
}

/// The lines in front of a forwarded text: who wrote it, when, about what, to whom.
pub fn forward_header(message: &Mail) -> String {
    let people = |people: &[MailAddress]| people.iter().map(mail::person).collect::<Vec<_>>().join(", ");
    let mut lines = vec![
        forward_line(),
        format!("{}: {}", tr("From"), sender_of(message)),
        format!("{}: {}", tr("Date"), when(message.summary.date)),
        format!("{}: {}", tr("Subject"), list::one_line(&message.summary.subject)),
    ];
    if !message.summary.to.is_empty() {
        lines.push(format!("{}: {}", tr("To"), people(&message.summary.to)));
    }
    if !message.cc.is_empty() {
        lines.push(format!("{}: {}", tr("Cc"), people(&message.cc)));
    }
    lines.join("\n")
}

/// The first line of [`forward_header`]: "---------- Forwarded message ----------".
fn forward_line() -> String {
    let dashes = "----------";
    format!("{dashes} {} {dashes}", tr("Forwarded message"))
}

/// Whether `address` is one of `own` (lower case), ignoring case.
fn is_own(address: &MailAddress, own: &[String]) -> bool {
    own.contains(&address.address.to_lowercase())
}

/// Adds `people` to `into` unless the address is there, theirs or one of `own`.
fn add_others(into: &mut Vec<MailAddress>, people: &[MailAddress], own: &[String], also_not: &[MailAddress]) {
    for who in people {
        let known = |list: &[MailAddress]| list.iter().any(|other| other.address.eq_ignore_ascii_case(&who.address));
        if !is_own(who, own) && !known(into) && !known(also_not) {
            into.push(who.clone());
        }
    }
}

/// What the window starts with for `kind`, given the message it answers (`None` for a new one)
/// and the addresses of this computer's accounts (`own`, lower case).
pub fn opening(kind: &Kind, message: Option<&Mail>, own: &[String]) -> Opening {
    let Some(message) = message.filter(|_| *kind != Kind::New) else { return Opening::default() };
    let text = message.text.as_deref().unwrap_or_default();
    let summary = &message.summary;
    if matches!(kind, Kind::Forward(_)) {
        let attachments =
            message.attachments.iter().filter(|info| !info.inline).map(|info| Attached::part(&summary.id, info));
        return Opening {
            subject: forward_subject(&summary.subject),
            body: format!("\n\n{}\n\n{}", forward_header(message), list::plain(text).trim()),
            attachments: attachments.collect(),
            history: Some(forward_line()),
            ..Opening::default()
        };
    }
    // Replies go where the sender wants them; to an answer to one's own mail they go to the same
    // people as the mail did.
    let target: Vec<MailAddress> =
        if message.reply_to.is_empty() { summary.from.iter().cloned().collect() } else { message.reply_to.clone() };
    let mut to = Vec::new();
    if target.iter().all(|who| is_own(who, own)) {
        add_others(&mut to, &summary.to, own, &[]);
    } else {
        add_others(&mut to, &target, own, &[]);
    }
    let mut cc = Vec::new();
    if matches!(kind, Kind::ReplyAll(_)) {
        add_others(&mut to, &summary.to, own, &[]);
        let to_so_far = to.clone();
        add_others(&mut cc, &message.cc, own, &to_so_far);
    }
    Opening {
        to,
        cc,
        subject: reply_subject(&summary.subject),
        body: format!("\n\n{}\n{}", attribution(message), quote(text)),
        attachments: Vec::new(),
        in_reply_to: Some(summary.id.clone()),
        history: Some(attribution(message)),
    }
}

/// The account to write from: the one asked for with `--account`, else the one the message came
/// to (its account, else the one named among its recipients), else the first. `None` without any.
pub fn pick_sender(senders: &[Sender], wanted: Option<&str>, message: Option<&Mail>) -> Option<usize> {
    pick_sender_with_default(senders, wanted, message, None)
}

/// Explicit account, then the account of a reply/forward, then the saved default for new mail.
pub fn pick_sender_with_default(
    senders: &[Sender],
    wanted: Option<&str>,
    message: Option<&Mail>,
    default: Option<&str>,
) -> Option<usize> {
    let find = |account: &str| senders.iter().position(|sender| sender.account == account);
    let by_recipient = |message: &Mail| {
        let named = message.summary.to.iter().chain(&message.cc);
        senders.iter().position(|sender| named.clone().any(|who| who.address.eq_ignore_ascii_case(&sender.address)))
    };
    wanted
        .and_then(find)
        .or_else(|| message.and_then(|message| find(&message.summary.account).or_else(|| by_recipient(message))))
        .or_else(|| default.and_then(find))
        .or((!senders.is_empty()).then_some(0))
}

// ---------------------------------------------------------------------------------------------
// Addresses typed into a field
// ---------------------------------------------------------------------------------------------

/// Whether `address` can be a recipient: one `@` with something before it and a domain with a dot
/// behind it, no white space, control or list characters. (The core checks it again.)
pub fn is_address(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else { return false };
    let plain = |c: char| !c.is_whitespace() && !c.is_control() && !"<>,;\"@()[]\\".contains(c);
    address.len() <= 254
        && !local.is_empty()
        && local.chars().all(plain)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
        && domain.chars().all(plain)
}

/// The words of a recipient field: addresses separated by commas or semicolons, each `address` or
/// `Name <address>` (the name may be quoted and hold commas). Answers the addresses it
/// understood and the words it did not.
pub fn parse_addresses(input: &str) -> (Vec<MailAddress>, Vec<String>) {
    let (mut found, mut refused) = (Vec::new(), Vec::new());
    let (mut current, mut quoted, mut angle) = (String::new(), false, false);
    let mut take = |word: &mut String| {
        let word = std::mem::take(word);
        let word = word.trim();
        if word.is_empty() {
            return;
        }
        let (name, address) = match (word.rfind('<'), word.ends_with('>')) {
            (Some(open), true) => (word[..open].trim().trim_matches('"').trim(), word[open + 1..word.len() - 1].trim()),
            _ => ("", word),
        };
        if is_address(address) {
            let name = list::one_line(name);
            found.push(MailAddress { name: (!name.is_empty()).then_some(name), address: address.to_owned() });
        } else {
            refused.push(word.to_owned());
        }
    };
    for c in input.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            '<' if !quoted => {
                angle = true;
                current.push(c);
            }
            '>' if !quoted => {
                angle = false;
                current.push(c);
            }
            ',' | ';' if !quoted && !angle => take(&mut current),
            _ => current.push(c),
        }
    }
    take(&mut current);
    (found, refused)
}

/// One recipient as a field shows it: `address`, or `Name <address>` with the name quoted when it
/// holds a character the reader would take for a separator.
pub fn format_address(who: &MailAddress) -> String {
    match who.name.as_deref().map(list::one_line).filter(|name| !name.is_empty()) {
        None => who.address.clone(),
        Some(name) if name.contains([',', ';', '<', '>', '"']) => {
            format!("\"{}\" <{}>", name.replace('"', "'"), who.address)
        }
        Some(name) => format!("{name} <{}>", who.address),
    }
}

/// [`format_address`] for a list, separated by commas.
pub fn format_addresses(people: &[MailAddress]) -> String {
    people.iter().map(format_address).collect::<Vec<_>>().join(", ")
}

// ---------------------------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------------------------

/// What the window has done with the core since it opened, for the tests (as `App::sent` for the
/// inbox): the calls it made and whether it closed.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Save(Box<Draft>),
    Send(String),
    Discard(String),
    Exit,
}

/// The kind of an answer by its start flag: `Kind::Reply` and the like.
type Answer = fn(String) -> Kind;

/// What the first call brings: the accounts, and the message to answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub senders: Vec<Sender>,
    pub message: Option<Box<Mail>>,
    pub default_sender: Option<String>,
}

/// Where the window stands before it can be written in.
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    /// The accounts, and the message to answer, are being fetched.
    Loading,
    /// They could not be fetched.
    Failed(Notice),
    /// Ready to write.
    Ready,
}

/// A sentence that something did not work, with the reason quietly after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub what: String,
    pub why: Option<String>,
}

impl Notice {
    fn new(what: impl Into<String>, why: impl std::fmt::Display) -> Notice {
        Notice { what: what.into(), why: Some(why.to_string()).filter(|why| !why.is_empty()) }
    }

    fn plain(what: impl Into<String>) -> Notice {
        Notice { what: what.into(), why: None }
    }
}

/// What is done once the draft is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum After {
    /// Nothing: an autosave.
    Nothing,
    /// Send it.
    Send,
    /// Close the window.
    Close,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The top band of the window was pressed (macOS): drag the window.
    DragWindow,
    Zoom(zoom::Message),
    Resized(Size),
    SystemMode(Mode),
    Appearance(Result<Appearance, String>),
    /// The design pack stored in the suite settings, and its fonts loaded.
    Design(design::Message),
    /// The clock of the autosave.
    Tick(Instant),
    /// The accounts and the message to answer, or why not.
    Loaded(Result<Prepared, ComposeError>),
    /// The core answered at the start, or did not within `CORE_START_WAIT` (issue #206): the first
    /// load follows either way.
    CoreReady,
    /// "Retry" after a failed start.
    Retry,
    To(String),
    Cc(String),
    Bcc(String),
    Subject(String),
    Body(text_editor::Action),
    /// A button of the toolbar or its key (formatted mail only).
    Format(Format),
    ShowCc,
    ShowBcc,
    /// The From menu opens or closes.
    FromMenu,
    PickFrom(usize),
    /// "Attach files": opens the file chooser.
    Attach,
    /// The files chosen.
    Picked(Vec<PathBuf>),
    RemoveAttachment(usize),
    /// "Send" (`Ctrl+Enter`).
    Send,
    /// A save finished: the revision it saved and the draft the core answered.
    Saved(u64, Box<Result<Draft, ComposeError>>),
    /// A send finished.
    Sent(Result<Delivery, ComposeError>),
    /// A draft that is replaced by one in another account was moved to the trash.
    Cleaned,
    /// Esc.
    Escape,
    /// The window's close button.
    Close,
    /// "Keep writing" in the question on closing.
    KeepWriting,
    /// "Discard changes".
    DiscardChanges,
    /// "Save draft" in the question: saves, then closes.
    SaveAndClose,
    DismissNotice,
    /// Tab (`true`) or Shift+Tab: the next or previous field.
    Focus(bool),
}

/// The compose window: the fields, the draft they are saved as and what is going on with it.
pub struct Window<S> {
    source: Arc<S>,
    suite: Source,
    start: Start,
    appearance: Appearance,
    forced_appearance: Option<Appearance>,
    system_mode: Mode,
    zoom: Zoom,
    /// Follows the design pack the suite settings name (task L.3).
    design: Follow,
    size: Size,
    phase: Phase,
    senders: Vec<Sender>,
    /// Index into `senders`.
    from: usize,
    to: String,
    cc: String,
    bcc: String,
    show_cc: bool,
    show_bcc: bool,
    subject: String,
    body: Content,
    attachments: Vec<Attached>,
    in_reply_to: Option<String>,
    /// Where the quoted history starts in the text ([`Opening::history`]).
    history: Option<String>,
    /// The draft as the core answered the last save: its id, and the account it lives in.
    draft: Option<(String, String)>,
    /// Counts every change; a save remembers the number it saved.
    revision: u64,
    saved: u64,
    /// When the last change was made; `None` when everything is saved.
    changed: Option<Instant>,
    /// The revision of the save that is running.
    saving: Option<u64>,
    /// What waits for the running save.
    after: After,
    sending: bool,
    /// The outcome of a send is unknown ([`Delivery::Unknown`]); it is not sent again.
    unsure: bool,
    notice: Option<Notice>,
    /// The From menu is open.
    menu: bool,
    /// The question on closing with unsaved changes is open.
    asking: bool,
    #[cfg(test)]
    calls: Vec<Call>,
}

impl<S: ComposeSource + 'static> Window<S> {
    pub fn new(source: Arc<S>, suite: Source, launch: Launch) -> (Window<S>, Task<Message>) {
        let start =
            launch.compose.clone().unwrap_or(Start { kind: Kind::New, account: None, open: None, attach: None });
        let phase = if start.open == Some(Open::Failed) {
            Phase::Failed(Notice::new(start.kind.load_failed(), ComposeError::NotFound))
        } else {
            Phase::Loading
        };
        let window = Window {
            source,
            zoom: Zoom::for_source(&suite),
            design: Follow::for_source(&suite),
            suite,
            appearance: Appearance::System,
            forced_appearance: launch.appearance,
            system_mode: Mode::None,
            size: WINDOW,
            phase,
            senders: Vec::new(),
            from: 0,
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            show_cc: false,
            show_bcc: false,
            subject: String::new(),
            body: Content::new(),
            attachments: Vec::new(),
            in_reply_to: None,
            history: None,
            draft: None,
            revision: 0,
            saved: 0,
            changed: None,
            saving: None,
            after: After::Nothing,
            sending: false,
            unsure: false,
            notice: None,
            menu: false,
            asking: false,
            start,
            #[cfg(test)]
            calls: Vec::new(),
        };
        // The size, and the traffic lights onto the title band's line (U.11).
        let size = Task::batch([
            iced::window::latest().and_then(iced::window::size).map(Message::Resized),
            torro_gui::place_traffic_lights(),
        ]);
        let mut tasks = vec![
            window.fetch_appearance(),
            window.zoom.fetch().map(Message::Zoom),
            window.design.fetch().map(Message::Design),
            iced::system::theme().map(Message::SystemMode),
            size,
        ];
        // The states "loading" and "failed" stay as they are, for pictures.
        if !matches!(window.start.open, Some(Open::Loading | Open::Failed)) {
            // The first load waits for a core that is still starting (issue #206).
            let suite = window.suite.clone();
            tasks.push(Task::perform(async move { suite.ready().await }, |_| Message::CoreReady));
        }
        (window, Task::batch(tasks))
    }

    // ----- state -----

    fn class(&self) -> SizeClass {
        layout::of(self.size)
    }

    fn is_glance(&self) -> bool {
        matches!(self.class(), SizeClass::Mini | SizeClass::Strip)
    }

    fn ready(&self) -> bool {
        self.phase == Phase::Ready
    }

    /// Something was changed that no save has taken yet.
    fn unsaved(&self) -> bool {
        self.revision != self.saved
    }

    fn editable(&self) -> bool {
        self.ready() && !self.sending
    }

    fn sender(&self) -> Option<&Sender> {
        self.senders.get(self.from)
    }

    /// The account writes formatted mail: toolbar, keys and HTML.
    fn formatted(&self) -> bool {
        self.sender().is_some_and(|sender| sender.html)
    }

    /// The title of the window: the subject, else what is being written.
    pub fn title(&self) -> String {
        let subject = list::one_line(&self.subject);
        if subject.is_empty() { self.start.kind.title().to_owned() } else { subject }
    }

    pub fn theme(&self) -> Theme {
        theme::for_appearance(self.forced_appearance.unwrap_or(self.appearance), self.system_mode)
    }

    pub fn scale_factor(&self) -> f32 {
        self.zoom.factor()
    }

    fn fetch_appearance(&self) -> Task<Message> {
        let suite = self.suite.clone();
        Task::perform(async move { theme::stored_appearance(&suite).await }, Message::Appearance)
    }

    /// Fetches the accounts and the message to answer.
    fn prepare(&self) -> Task<Message> {
        let (source, kind) = (self.source.clone(), self.start.kind.clone());
        Task::perform(async move { prepare(source, kind).await }, Message::Loaded)
    }

    /// Something changed: a new revision, to be saved after [`AUTOSAVE`].
    fn touch(&mut self) {
        self.revision += 1;
        self.changed = Some(Instant::now());
    }

    // ----- updates -----

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::DragWindow => return torro_gui::drag_window(),
            Message::Zoom(message) => {
                let before = self.zoom.factor();
                let task = self.zoom.update(message).map(Message::Zoom);
                if self.zoom.factor() == before {
                    return task;
                }
                self.size = zoom::rescale(self.size, before, self.zoom.factor());
                let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
                return Task::batch([task, size]);
            }
            Message::Resized(size) => self.size = size,
            Message::SystemMode(mode) => self.system_mode = mode,
            Message::Appearance(Ok(appearance)) => self.appearance = appearance,
            // The core may be starting up; keep the colours we have.
            Message::Appearance(Err(_)) => {}
            Message::Design(message) => {
                if let Some(task) = self.design.update(message) {
                    return task.map(Message::Design);
                }
            }
            Message::Tick(now) => return self.tick(now),
            Message::Loaded(result) => return self.loaded(result),
            Message::CoreReady => return self.prepare(),
            Message::Retry => {
                self.phase = Phase::Loading;
                return self.prepare();
            }
            Message::To(text) => self.edited(|window| window.to = text),
            Message::Cc(text) => self.edited(|window| window.cc = text),
            Message::Bcc(text) => self.edited(|window| window.bcc = text),
            Message::Subject(text) => self.edited(|window| window.subject = text),
            Message::Body(action) => {
                // Enter in a list item starts the next one (an empty one ends the list).
                let enter = matches!(action, text_editor::Action::Edit(Edit::Enter));
                if enter && self.formatted() && continue_list(&mut self.body) {
                    self.touch();
                    return Task::none();
                }
                let edits = action.is_edit();
                self.body.perform(action);
                if edits {
                    self.touch();
                }
            }
            Message::Format(format) => {
                if self.editable() && self.formatted() {
                    apply_format(&mut self.body, format);
                    self.touch();
                    // A click on the toolbar took the focus from the text.
                    return operation::focus(BODY);
                }
            }
            Message::ShowCc => {
                self.show_cc = true;
                return operation::focus(CC);
            }
            Message::ShowBcc => {
                self.show_bcc = true;
                return operation::focus(BCC);
            }
            Message::FromMenu => self.menu = !self.menu,
            Message::PickFrom(index) => {
                self.menu = false;
                if index < self.senders.len() && index != self.from {
                    self.from = index;
                    self.touch();
                }
            }
            Message::Attach => return self.choose_files(),
            Message::Picked(paths) => self.attach(&paths),
            Message::RemoveAttachment(index) => {
                if index < self.attachments.len() && self.editable() {
                    self.attachments.remove(index);
                    self.touch();
                }
            }
            Message::Send => return self.send(),
            Message::Saved(revision, result) => return self.saved(revision, *result),
            Message::Sent(result) => return self.sent(result),
            Message::Cleaned => {}
            Message::Escape => return self.escape(),
            Message::Close => return self.close(),
            Message::KeepWriting => self.asking = false,
            Message::DiscardChanges => return self.exit(),
            Message::SaveAndClose => {
                self.asking = false;
                return self.save(After::Close);
            }
            Message::DismissNotice => self.notice = None,
            Message::Focus(forward) => {
                if self.ready() && !self.asking && !self.is_glance() {
                    return if forward { operation::focus_next() } else { operation::focus_previous() };
                }
            }
        }
        Task::none()
    }

    /// Applies a change of a field, if the fields can be changed now.
    fn edited(&mut self, change: impl FnOnce(&mut Self)) {
        if self.editable() {
            change(self);
            self.touch();
        }
    }

    /// The clock: saves when [`AUTOSAVE`] has passed since the last change and nothing else is
    /// going on.
    fn tick(&mut self, now: Instant) -> Task<Message> {
        let idle = self.changed.is_some_and(|changed| now.duration_since(changed) >= AUTOSAVE);
        if idle && self.unsaved() && self.saving.is_none() && !self.sending {
            return self.save(After::Nothing);
        }
        Task::none()
    }

    /// The accounts and the message are in (or are not).
    fn loaded(&mut self, result: Result<Prepared, ComposeError>) -> Task<Message> {
        let prepared = match result {
            Ok(prepared) => prepared,
            Err(err) => {
                self.phase = Phase::Failed(Notice::new(self.start.kind.load_failed(), err));
                return Task::none();
            }
        };
        if prepared.senders.is_empty() {
            self.phase =
                Phase::Failed(Notice::new(tr("No mail account yet."), tr("Connect one in Settings, under Accounts.")));
            return Task::none();
        }
        let own: Vec<String> = prepared.senders.iter().map(|sender| sender.address.to_lowercase()).collect();
        let opened = opening(&self.start.kind, prepared.message.as_deref(), &own);
        self.from = pick_sender_with_default(
            &prepared.senders,
            self.start.account.as_deref(),
            prepared.message.as_deref(),
            prepared.default_sender.as_deref(),
        )
        .unwrap_or(0);
        self.senders = prepared.senders;
        self.to = format_addresses(&opened.to);
        self.cc = format_addresses(&opened.cc);
        self.show_cc = !opened.cc.is_empty();
        self.subject = opened.subject;
        self.body = Content::with_text(&opened.body);
        self.attachments = opened.attachments;
        self.in_reply_to = opened.in_reply_to;
        self.history = opened.history;
        self.phase = Phase::Ready;
        // `--attach` (task WZ.8): the file goes along as if it had been chosen; one that cannot go
        // says why in the notice, the window opens anyway.
        if let Some(path) = self.start.attach.clone() {
            self.attach(&[path]);
        }
        self.apply_open();
        let focus = if self.to.is_empty() { TO } else { BODY };
        operation::focus(focus)
    }

    /// The forced states of `--demo --open`, once the window is ready.
    fn apply_open(&mut self) {
        match self.start.open {
            Some(Open::Cc) => {
                self.show_cc = true;
                self.show_bcc = true;
                self.to = "Jonas Weber <jonas.weber@work.example.org>".to_owned();
                self.cc = "lena.hoffmann@work.example.org, team@work.example.org".to_owned();
                self.bcc = "anna@example.org".to_owned();
                self.subject = "Budget review for Q4".to_owned();
                self.body = Content::with_text(
                    "Hi Jonas,\n\nthe numbers look fine to me. Lena and the team are in copy.\n\nAnna",
                );
            }
            Some(Open::Attach) => {
                self.attachments = vec![
                    Attached::File {
                        path: PathBuf::from("/tmp/budget-q4.csv"),
                        name: "budget-q4.csv".into(),
                        size: 2_340,
                    },
                    Attached::File { path: PathBuf::from("/tmp/notes.pdf"), name: "notes.pdf".into(), size: 860_000 },
                ];
            }
            Some(Open::Error) => {
                self.notice = Some(Notice::new(
                    tr("Couldn't send the message."),
                    tr("The outgoing mail server cannot be reached. Nothing was sent; try again later."),
                ));
            }
            Some(Open::Unsure) => {
                self.unsure = true;
                self.notice = Some(Notice::new(tr("The message may or may not have gone out."), unsure_reason()));
            }
            Some(Open::Discard) => {
                self.revision = 1;
                self.asking = true;
            }
            Some(Open::Menu) => self.menu = self.senders.len() > 1,
            Some(Open::Format) => {
                self.to = "Jonas Weber <jonas.weber@work.example.org>".to_owned();
                self.subject = "Budget review for Q4".to_owned();
                self.body = Content::with_text(concat!(
                    "Hi Jonas,\n\nthe numbers look **fine** to me, *except* the travel budget. ",
                    "See [the sheet](https://intranet.work.example.org/budget/q4).\n\n",
                    "- Flights\n- Hotels\n\n1. Check\n2. __Approve__\n\n> Could you look before Thursday?\n\nAnna",
                ));
            }
            Some(Open::Loading | Open::Failed) | None => {}
        }
    }

    /// Opens the file chooser.
    fn choose_files(&self) -> Task<Message> {
        if !self.editable() {
            return Task::none();
        }
        Task::perform(choose_files(), Message::Picked)
    }

    /// Takes the files `paths` along, up to the number of attachments the core accepts; the first
    /// one that cannot go is told in a notice.
    fn attach(&mut self, paths: &[PathBuf]) {
        let mut problem = None;
        let mut added = false;
        for path in paths {
            if self.attachments.iter().any(|known| matches!(known, Attached::File { path: other, .. } if other == path))
            {
                continue;
            }
            if self.attachments.len() >= MAX_ATTACHMENTS {
                problem.get_or_insert_with(|| {
                    trf("At most {n} attachments fit in one message.", &[("n", &MAX_ATTACHMENTS)])
                });
                break;
            }
            match Attached::file(path) {
                Ok(attached) => {
                    self.attachments.push(attached);
                    added = true;
                }
                Err(why) => {
                    problem.get_or_insert(why);
                }
            }
        }
        if added {
            self.touch();
        }
        if let Some(why) = problem {
            self.notice = Some(Notice::new(tr("Couldn't attach the file."), why));
        }
    }

    // ----- the draft -----

    /// The draft as the fields hold it now. Addresses that are none are left out here ([`Problem`]
    /// stops a send with them), so an autosave never fails on a half-typed recipient.
    pub fn draft(&self) -> Draft {
        let account = self.sender().map(|sender| sender.account.clone()).unwrap_or_default();
        // A draft that lives in another account cannot be replaced by one in this one: the core
        // would not find it. A new one is saved and the old one goes to the trash afterwards.
        let id = self.draft.as_ref().filter(|(_, in_account)| *in_account == account).map(|(id, _)| id.clone());
        let text = self.body.text().replace("\r\n", "\n");
        // Formatted mail: the marks as HTML, and the plain text the core would make of it.
        let html = self
            .formatted()
            .then(|| markup::to_html(&text, self.history.as_deref()))
            .filter(|markup| !markup.is_empty());
        Draft {
            id,
            account,
            to: parse_addresses(&self.to).0,
            cc: parse_addresses(&self.cc).0,
            bcc: parse_addresses(&self.bcc).0,
            subject: list::one_line(&self.subject),
            body: html.as_deref().map_or(text, html::to_text),
            html,
            in_reply_to: self.in_reply_to.clone(),
            attachments: self.attachments.iter().map(Attached::draft).collect(),
        }
    }

    /// What stops a send: no recipient, or words in a field that are no address.
    pub fn problem(&self) -> Option<Notice> {
        let refused: Vec<String> =
            [&self.to, &self.cc, &self.bcc].into_iter().flat_map(|field| parse_addresses(field).1).collect();
        if let Some(first) = refused.first() {
            return Some(Notice::plain(trf("{address} isn't a valid address.", &[("address", &first)])));
        }
        let draft = self.draft();
        if draft.to.len() + draft.cc.len() + draft.bcc.len() == 0 {
            return Some(Notice::plain(tr("Add at least one recipient.")));
        }
        None
    }

    /// Saves the draft, then does `after`. A save that is running takes `after` along when it is
    /// done; the fields are saved once more if they changed meanwhile.
    fn save(&mut self, after: After) -> Task<Message> {
        self.after = self.after.max(after);
        if self.saving.is_some() || !self.ready() {
            return Task::none();
        }
        let draft = self.draft();
        let old = self.draft.clone().filter(|(_, account)| *account != draft.account).map(|(id, _)| id);
        self.saving = Some(self.revision);
        let revision = self.revision;
        #[cfg(test)]
        self.calls.push(Call::Save(Box::new(draft.clone())));
        let source = self.source.clone();
        let saved = Task::perform(async move { save_with_fallback(source, draft).await }, move |result| {
            Message::Saved(revision, Box::new(result))
        });
        match old {
            Some(old) => Task::batch([saved, self.discard(old)]),
            None => saved,
        }
    }

    /// Moves a draft that was replaced by one in another account to the trash.
    fn discard(&mut self, id: String) -> Task<Message> {
        #[cfg(test)]
        self.calls.push(Call::Discard(id.clone()));
        let source = self.source.clone();
        Task::perform(async move { source.discard_draft(id).await }, |_| Message::Cleaned)
    }

    /// A save finished.
    fn saved(&mut self, revision: u64, result: Result<Draft, ComposeError>) -> Task<Message> {
        self.saving = None;
        let draft = match result {
            Ok(draft) => draft,
            Err(err) => {
                // Everything stays as it is; the clock tries again, and a send or a close that
                // waited for this save is given up.
                self.after = After::Nothing;
                self.sending = false;
                self.changed = Some(Instant::now());
                self.notice = Some(Notice::new(tr("Couldn't save the draft. Your text is still here."), err));
                return Task::none();
            }
        };
        if let Some(id) = draft.id {
            self.draft = Some((id, draft.account));
        }
        self.saved = self.saved.max(revision);
        let settled = revision == self.revision;
        if settled {
            self.changed = None;
        }
        if !settled && self.after != After::Nothing {
            // Changed while it was saved, and something waits for it: save what is new first.
            return self.save(After::Nothing);
        }
        match std::mem::replace(&mut self.after, After::Nothing) {
            After::Nothing => Task::none(),
            After::Send => self.dispatch(),
            After::Close => self.exit(),
        }
    }

    /// "Send" (or `Ctrl+Enter`): checks the recipients, saves what is not saved and sends.
    fn send(&mut self) -> Task<Message> {
        if !self.editable() || self.unsure || self.asking {
            return Task::none();
        }
        if let Some(problem) = self.problem() {
            self.notice = Some(problem);
            return Task::none();
        }
        self.notice = None;
        self.sending = true;
        if self.unsaved() || self.draft.is_none() || self.saving.is_some() {
            return self.save(After::Send);
        }
        self.dispatch()
    }

    /// Sends the saved draft.
    fn dispatch(&mut self) -> Task<Message> {
        let Some((id, _)) = self.draft.clone() else {
            self.sending = false;
            return Task::none();
        };
        #[cfg(test)]
        self.calls.push(Call::Send(id.clone()));
        let source = self.source.clone();
        Task::perform(async move { source.send_draft(id).await }, Message::Sent)
    }

    /// A send finished.
    fn sent(&mut self, result: Result<Delivery, ComposeError>) -> Task<Message> {
        self.sending = false;
        match result {
            Ok(Delivery::Accepted) => return self.exit(),
            Ok(Delivery::Unknown) => {
                self.unsure = true;
                self.notice = Some(Notice::new(tr("The message may or may not have gone out."), unsure_reason()));
            }
            Err(err) => {
                // A draft that is gone is saved anew with the next send; the text is still here.
                if err == ComposeError::NotFound {
                    self.draft = None;
                    self.touch();
                }
                self.notice = Some(Notice::new(tr("Couldn't send the message."), err));
            }
        }
        Task::none()
    }

    // ----- closing -----

    /// Esc: closes what is open, else asks to close the window.
    fn escape(&mut self) -> Task<Message> {
        if self.menu {
            self.menu = false;
        } else if self.asking {
            self.asking = false;
        } else {
            return self.close();
        }
        Task::none()
    }

    /// The window is asked to close: at once when everything is saved, else after a question.
    fn close(&mut self) -> Task<Message> {
        if self.sending || self.asking {
            return Task::none();
        }
        if self.unsaved() {
            self.menu = false;
            self.asking = true;
            return Task::none();
        }
        self.exit()
    }

    fn exit(&mut self) -> Task<Message> {
        #[cfg(test)]
        self.calls.push(Call::Exit);
        iced::exit()
    }

    // ----- events -----

    pub fn subscription(&self) -> Subscription<Message> {
        let mut all = vec![
            iced::system::theme_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            // The zoom keys work in every state, the glance included.
            self.zoom.subscription().map(Message::Zoom),
            iced::window::close_requests().map(|_| Message::Close),
        ];
        // Mini and strip are displays: no keys.
        if !self.is_glance() {
            all.push(iced::event::listen_with(key));
        }
        if self.changed.is_some() {
            all.push(iced::time::every(CLOCK).map(Message::Tick));
        }
        Subscription::batch(all)
    }
}

/// The reason added to the notice of a send whose outcome is unknown.
fn unsure_reason() -> &'static str {
    tr("The connection broke after the message was handed over. Look into the Sent folder before sending it again.")
}

/// Keys, also while a field has the focus: Esc, the shortcut "send" ([`is_send`]) and Tab. (Only
/// plain Enter and the letters are the fields'.)
fn key(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }) = event else {
        return None;
    };
    if is_send(System::current(), &key, physical_key, modifiers) {
        return Some(Message::Send);
    }
    match key {
        Key::Named(Named::Escape) => Some(Message::Escape),
        Key::Named(Named::Tab) if !modifiers.command() && !modifiers.alt() => Some(Message::Focus(!modifiers.shift())),
        _ => None,
    }
}

/// The accounts, and the message to answer when there is one.
pub async fn prepare<S: ComposeSource>(source: Arc<S>, kind: Kind) -> Result<Prepared, ComposeError> {
    let senders = source.senders().await?;
    let default_sender = source.default_sender().await?;
    let message = match kind.message() {
        Some(id) => Some(Box::new(source.get_mail(id.to_owned()).await?)),
        None => None,
    };
    Ok(Prepared { senders, message, default_sender })
}

/// Saves `draft`; a draft that is gone (`NotFound` for the id it carries) is saved as a new one,
/// so what was written is never lost to a draft someone removed.
pub async fn save_with_fallback<S: ComposeSource>(source: Arc<S>, draft: Draft) -> Result<Draft, ComposeError> {
    match source.save_draft(draft.clone()).await {
        Err(ComposeError::NotFound) if draft.id.is_some() => source.save_draft(Draft { id: None, ..draft }).await,
        other => other,
    }
}

/// Asks for files with the system's chooser; none when it is cancelled.
async fn choose_files() -> Vec<PathBuf> {
    let chosen = rfd::AsyncFileDialog::new().set_title(tr("Attach files")).pick_files().await;
    chosen.unwrap_or_default().iter().map(|file| file.path().to_path_buf()).collect()
}

// ---------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------

impl<S: ComposeSource + 'static> Window<S> {
    /// The status next to the title: what is going on with the draft.
    pub fn status(&self) -> Option<&'static str> {
        if self.sending {
            Some(tr("Sending…"))
        } else if self.saving.is_some() {
            Some(tr("Saving…"))
        } else if self.unsaved() {
            Some(tr("Unsaved changes"))
        } else if self.draft.is_some() {
            Some(tr("Draft saved"))
        } else {
            None
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let class = self.class();
        let cover = Cover::dialog(self.asking && !self.is_glance());
        let content: Element<'_, Message> = if self.is_glance() { self.glance(class) } else { self.window(class) };
        widgets::ctrl_guard(shell::titlebar(
            self.zoom.view(content),
            self.zoom.factor(),
            Surface::Panel,
            cover,
            TitleButtons::none(),
            Message::DragWindow,
        ))
    }

    /// Mini and strip: a display. What is being written, to whom, and how the draft stands; no
    /// field, no key.
    fn glance(&self, class: SizeClass) -> Element<'_, Message> {
        let padding = match class {
            SizeClass::Strip => Padding::new(space::s()).left(space::m()).right(space::m()),
            _ => Padding::new(space::m()),
        };
        let body: Element<'_, Message> = match &self.phase {
            Phase::Loading => widgets::loading(),
            Phase::Failed(notice) => notice_line(notice, None),
            Phase::Ready => {
                let recipients = {
                    let (to, _) = parse_addresses(&self.to);
                    let names: Vec<String> = to.iter().map(|who| mail::sender_name(Some(who))).collect();
                    if names.is_empty() { tr("No recipient yet").to_owned() } else { names.join(", ") }
                };
                let subject = list::one_line(&self.subject);
                let subject = if subject.is_empty() { tr("(no subject)").to_owned() } else { subject };
                // The text stands beside the icon, inside the tile's padding; what does not fit ends in "…".
                let room = shell::below_titlebar(self.size, self.zoom.factor()).width
                    - 2.0 * padding.left
                    - icon::SIZE
                    - space::s();
                let mut lines = column![
                    typo::text(typo::body_strong(), typo::ellipsize(&subject, typo::body_strong(), room))
                        .wrapping(Wrapping::None),
                    typo::text(typo::meta(), typo::ellipsize(&recipients, typo::meta(), room))
                        .style(theme::text_2)
                        .wrapping(Wrapping::None),
                ]
                .spacing(space::xs());
                if let Some(status) = self.status() {
                    lines = lines.push(typo::text(typo::meta(), status).style(theme::text_3).wrapping(Wrapping::None));
                }
                let mark = widgets::with_tooltip(
                    container(
                        icon::icon(self.start.kind.icon(), icon::SIZE)
                            .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_3) }),
                    ),
                    self.start.kind.title(),
                );
                row![mark, container(lines).width(Fill).clip(true)]
                    .spacing(space::s())
                    .align_y(Alignment::Center)
                    .into()
            }
        };
        container(body)
            .width(Fill)
            .height(Fill)
            .center_y(Fill)
            .padding(padding)
            .clip(true)
            .style(style::container::panel_flush)
            .into()
    }

    /// The window in the layout of `class` (not mini or strip).
    fn window(&self, class: SizeClass) -> Element<'_, Message> {
        let narrow = class == SizeClass::Narrow;
        let anchor = Anchor::new();
        let body: Element<'_, Message> = match &self.phase {
            Phase::Loading => widgets::loading(),
            Phase::Failed(notice) => failure(notice),
            Phase::Ready => self.form(narrow, &anchor),
        };
        // The compose window has no sidebar: header and form sit flush in the window, with the
        // padding of the medium header when there is room and the narrow one when there is not.
        let frame = if narrow { SizeClass::Narrow } else { SizeClass::Medium };
        let overlay = (self.menu && self.ready()).then(|| self.sender_menu(&anchor));
        let window: Element<'_, Message> =
            shell::shell(frame, Space::new(), self.header(narrow), body, None).overlay(overlay).into();
        if self.asking { widgets::modal(window, self.question(), size::panel(), Message::KeepWriting) } else { window }
    }

    /// The header bar: what is written and how the draft stands, then attach, close and the one
    /// primary button, "Send". Narrow windows keep the icons and drop the status.
    fn header(&self, narrow: bool) -> Element<'_, Message> {
        let kind = &self.start.kind;
        let note = if narrow { None } else { self.status().map(str::to_owned) };
        let title = shell::header_title(Some(kind.icon()), kind.title(), note);
        let editable = self.editable();
        let attach = widgets::icon_button(Icon::Paperclip, tr("Attach files"), editable.then_some(Message::Attach));
        let close = widgets::icon_button(Icon::X, tr("Close"), (!self.sending).then_some(Message::Close));
        let press = (editable && !self.unsure).then_some(Message::Send);
        let send: Element<'_, Message> = if narrow {
            widgets::compact_button(Icon::Send, tr("Send"), style::button::primary, press)
        } else {
            let label = if self.sending { tr("Sending…") } else { tr("Send") };
            let button = widgets::text_button(Some(Icon::Send), label, style::button::primary, press);
            widgets::with_tooltip(button, trf("Send ({keys})", &[("keys", &send_keys())]))
        };
        shell::header_bar([], title, [attach, close, send])
    }

    /// The form: a notice if there is one, the fields, the text, the signature.
    fn form(&self, narrow: bool, anchor: &Anchor) -> Element<'_, Message> {
        let editable = self.editable();
        let mut fields = column![].spacing(space::s());
        if let Some(notice) = &self.notice {
            fields = fields.push(notice_line(notice, Some(Message::DismissNotice)));
        }
        fields = fields.push(field(tr("From"), self.sender_control(anchor)));
        let mut folds = row![].spacing(space::xs());
        if !self.show_cc {
            let press = editable.then_some(Message::ShowCc);
            folds = folds.push(widgets::text_button(None, tr("Cc"), style::button::ghost, press));
        }
        if !self.show_bcc {
            let press = editable.then_some(Message::ShowBcc);
            folds = folds.push(widgets::text_button(None, tr("Bcc"), style::button::ghost, press));
        }
        let to = row![self.input(TO, &self.to, Message::To), folds].spacing(space::s()).align_y(Alignment::Center);
        fields = fields.push(field(tr("To"), to));
        if self.show_cc {
            fields = fields.push(field(tr("Cc"), self.input(CC, &self.cc, Message::Cc)));
        }
        if self.show_bcc {
            fields = fields.push(field(tr("Bcc"), self.input(BCC, &self.bcc, Message::Bcc)));
        }
        fields = fields.push(field(tr("Subject"), self.input(SUBJECT, &self.subject, Message::Subject)));
        if !self.attachments.is_empty() {
            let chips = self.attachments.iter().enumerate().map(|(i, attached)| self.attachment_chip(i, attached));
            fields = fields.push(field("", widgets::chip_row(chips.collect())));
        }
        let mut editor = iced::widget::text_editor(&self.body)
            .id(BODY)
            .placeholder(tr("Write your message…"))
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().font)
            .padding(style::INPUT_PADDING)
            .height(Fill)
            .wrapping(Wrapping::WordOrGlyph)
            .style(style::text_editor)
            .key_binding(editor_binding);
        if editable {
            editor = editor.on_action(Message::Body);
        }
        let mut form = column![fields].spacing(space::s()).height(Fill);
        if self.formatted() {
            form = form.push(self.toolbar());
            form = form.push(editor.highlight_with::<MarkupHighlighter>((), look_format));
        } else {
            form = form.push(editor);
        }
        if let Some(signature) = self.sender().and_then(|sender| sender.signature.as_deref()) {
            form = form.push(signature_block(signature));
        }
        // The form takes the whole width of its window (issue #124): the compose window is a tile
        // of its own, sized by the tiling manager, so a reading width would only leave a gap.
        let padding = if narrow { Padding::new(space::m()) } else { Padding::new(space::l()) };
        container(form).width(Fill).height(Fill).padding(padding).into()
    }

    /// The toolbar of formatted mail (issue #125): bold, italic, underline, link | lists, quote.
    /// Each button does what its key does; the tooltip names the key.
    fn toolbar(&self) -> Element<'_, Message> {
        let editable = self.editable();
        let button = |format: Format| -> Element<'_, Message> {
            let tip = match format.key() {
                Some(key) => trf("{name} ({keys})", &[("name", &format.name()), ("keys", &shortcut(key))]),
                None => format.name().to_owned(),
            };
            widgets::icon_button(format.icon(), tip, editable.then_some(Message::Format(format)))
        };
        let divider = container(iced::widget::rule::vertical(size::HAIRLINE).style(style::rule)).height(space::l());
        let inline = [Format::Bold, Format::Italic, Format::Underline, Format::Link].map(button);
        let blocks = [Format::Bullets, Format::Numbers, Format::Quote].map(button);
        row(inline).push(divider).extend(blocks).spacing(space::xs()).align_y(Alignment::Center).into()
    }

    /// A single-line field of the form.
    fn input<'a>(&self, id: &'static str, value: &'a str, on_input: fn(String) -> Message) -> Element<'a, Message> {
        text_input("", value)
            .id(id)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().font)
            .padding(style::INPUT_PADDING)
            .width(Fill)
            .style(style::text_input)
            .on_input_maybe(self.editable().then_some(on_input))
            .into()
    }

    /// The account to write from: its name when there is only one, else a button that opens the
    /// menu of them.
    fn sender_control(&self, anchor: &Anchor) -> Element<'_, Message> {
        match self.senders.as_slice() {
            [only] => container(typo::text(typo::body(), only.label()).style(theme::text_2).wrapping(Wrapping::None))
                .center_y(size::control())
                .into(),
            _ => {
                let label = self.sender().map(Sender::label).unwrap_or_default();
                row![widgets::select_button(anchor, label, Message::FromMenu)]
                    .height(size::control())
                    .align_y(Alignment::Center)
                    .into()
            }
        }
    }

    /// The menu of the accounts, under the From button.
    fn sender_menu(&self, anchor: &Anchor) -> Element<'_, Message> {
        let items = self
            .senders
            .iter()
            .enumerate()
            .map(|(i, sender)| {
                MenuItem::new(Icon::Mail, sender.address.clone(), Message::PickFrom(i)).current(i == self.from)
            })
            .collect();
        widgets::floating(
            anchor,
            Placement::Below(Align::Start),
            widgets::menu(vec![MenuSection::titled(tr("Send from"), items)]),
            Message::FromMenu,
        )
    }

    /// An attachment as a chip: its name and size, and a button that takes it out again.
    fn attachment_chip<'a>(&self, index: usize, attached: &'a Attached) -> Element<'a, Message> {
        let name = typo::ellipsize(&list::one_line(attached.name()), typo::meta(), size::props());
        let label = format!("{name} · {}", mail::size_text(attached.size()));
        let press = self.editable().then_some(Message::RemoveAttachment(index));
        let remove = widgets::icon_button(Icon::X, tr("Remove"), press);
        row![widgets::chip(Some(Mark::Icon(Icon::Paperclip)), label, None), remove].align_y(Alignment::Center).into()
    }

    /// The question on closing with unsaved changes, in a dialog over the window.
    fn question(&self) -> Element<'_, Message> {
        let actions = row![
            widgets::text_button(None, tr("Keep writing"), style::button::ghost, Some(Message::KeepWriting)),
            widgets::text_button(None, tr("Discard changes"), style::button::danger, Some(Message::DiscardChanges)),
            widgets::text_button(
                Some(Icon::Check),
                tr("Save draft"),
                style::button::primary,
                Some(Message::SaveAndClose)
            ),
        ]
        .spacing(space::s())
        .wrap()
        .vertical_spacing(space::s());
        let text = column![
            typo::text(typo::title(), tr("Save this draft?")),
            typo::text(typo::body(), tr("Your latest changes aren't saved yet.")).style(theme::text_2),
        ]
        .spacing(space::xs());
        container(column![text, actions].spacing(space::l()))
            .padding(space::l())
            .style(style::container::popover)
            .into()
    }
}

/// The keys of the shortcut "send" shown in the tooltip of "Send", in the system's preset
/// (`torro_core::shortcuts`, tasks E10.1, E10.1b): "Ctrl+Enter", "Shift+Cmd+D" on macOS. Also the
/// tooltip of "Send" in the mail window's "Focus & reply" (task E19.5).
pub(crate) fn send_keys() -> String {
    Preset::default().hint(shortcuts::Command::Send, System::current()).unwrap_or_default()
}

/// Whether `key` held with `modifiers` is the shortcut "send" on `system` (tasks E10.1, E10.1b):
/// Ctrl + Enter, on macOS also ⇧⌘D (Apple Mail), ⌘ + Enter or Ctrl + Enter. The reply field of the
/// mail window's "Focus & reply" sends with it too (task E19.5).
pub(crate) fn is_send(
    system: System,
    key: &Key,
    physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> bool {
    keys::shortcut_on(system, key, physical_key, modifiers) == Some(shortcuts::Command::Send)
}

/// The text's key bindings: the editor's own, except that the shortcut "send" ([`is_send`]) sends
/// instead of starting a new line.
fn editor_binding(press: text_editor::KeyPress) -> Option<Binding<Message>> {
    if is_send(System::current(), &press.key, press.physical_key, press.modifiers) {
        return Some(Binding::Custom(Message::Send));
    }
    if let Some(format) = format_key(&press.key, press.modifiers) {
        return Some(Binding::Custom(Message::Format(format)));
    }
    Binding::from_key_press(press)
}

/// The formatting of `Cmd`/`Ctrl` and B, I, U or K (no Shift, no Alt); `None` for other keys.
fn format_key(key: &Key, modifiers: keyboard::Modifiers) -> Option<Format> {
    let Key::Character(letter) = key.as_ref() else { return None };
    if !modifiers.command() || modifiers.shift() || modifiers.alt() {
        return None;
    }
    Format::ALL.into_iter().find(|format| format.key().is_some_and(|key| key.eq_ignore_ascii_case(letter)))
}

/// `Cmd+B` on macOS, `Ctrl+B` elsewhere, for tooltips.
fn shortcut(key: &str) -> String {
    let command = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
    format!("{command}+{key}")
}

// ---------------------------------------------------------------------------------------------
// Formatting the text (issue #125)
// ---------------------------------------------------------------------------------------------

/// What a button of the toolbar or its key does to the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `**…**` around the selection, or an empty pair to type into.
    Bold,
    /// `*…*`, likewise.
    Italic,
    /// `__…__`, likewise.
    Underline,
    /// `[selection](address)`: the selection's own address when it is one, else the cursor waits
    /// between the parentheses.
    Link,
    /// `- ` before the lines of the selection (or the cursor's line); again: taken away.
    Bullets,
    /// `1. `, `2. ` … likewise.
    Numbers,
    /// `> ` likewise.
    Quote,
}

impl Format {
    /// In the order of the toolbar.
    pub const ALL: [Format; 7] = [
        Format::Bold,
        Format::Italic,
        Format::Underline,
        Format::Link,
        Format::Bullets,
        Format::Numbers,
        Format::Quote,
    ];

    /// The mark around text, for bold, italic and underline.
    fn mark(self) -> Option<&'static str> {
        match self {
            Format::Bold => Some("**"),
            Format::Italic => Some("*"),
            Format::Underline => Some("__"),
            Format::Link | Format::Bullets | Format::Numbers | Format::Quote => None,
        }
    }

    /// The letter that does it with `Cmd`/`Ctrl`.
    fn key(self) -> Option<&'static str> {
        match self {
            Format::Bold => Some("B"),
            Format::Italic => Some("I"),
            Format::Underline => Some("U"),
            Format::Link => Some("K"),
            Format::Bullets | Format::Numbers | Format::Quote => None,
        }
    }

    fn icon(self) -> Icon {
        match self {
            Format::Bold => Icon::Bold,
            Format::Italic => Icon::Italic,
            Format::Underline => Icon::Underline,
            Format::Link => Icon::Link,
            Format::Bullets => Icon::List,
            Format::Numbers => Icon::ListOrdered,
            Format::Quote => Icon::TextQuote,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Format::Bold => tr("Bold"),
            Format::Italic => tr("Italic"),
            Format::Underline => tr("Underline"),
            Format::Link => tr("Link"),
            Format::Bullets => tr("Bulleted list"),
            Format::Numbers => tr("Numbered list"),
            Format::Quote => tr("Quote"),
        }
    }
}

/// Does `format` to `content` where its cursor or selection is ([`Format`]).
pub fn apply_format(content: &mut Content, format: Format) {
    match format.mark() {
        Some(mark) => wrap(content, mark),
        None if format == Format::Link => link(content),
        None => prefix_lines(content, format),
    }
}

fn paste(content: &mut Content, text: &str) {
    content.perform(text_editor::Action::Edit(Edit::Paste(Arc::new(text.to_owned()))));
}

fn step(content: &mut Content, motion: Motion, times: usize) {
    for _ in 0..times {
        content.perform(text_editor::Action::Move(motion));
    }
}

/// `mark` around each line of the selection (taken away where it is there already), or an empty
/// pair with the cursor between.
fn wrap(content: &mut Content, mark: &str) {
    match content.selection().filter(|selected| !selected.trim().is_empty()) {
        Some(selected) => {
            let lines: Vec<String> = selected.split('\n').map(|line| toggle_mark(line, mark)).collect();
            paste(content, &lines.join("\n"));
        }
        None => {
            paste(content, &format!("{mark}{mark}"));
            step(content, Motion::Left, mark.len());
        }
    }
}

/// `line` with `mark` around its text, or without it when it has it; blank lines and the white
/// space at the ends stay.
fn toggle_mark(line: &str, mark: &str) -> String {
    let text = line.trim();
    if text.is_empty() {
        return line.to_owned();
    }
    let start = line.len() - line.trim_start().len();
    let (lead, trail) = (&line[..start], &line[start + text.len()..]);
    let inner = text.strip_prefix(mark).and_then(|rest| rest.strip_suffix(mark)).filter(|inner| !inner.is_empty());
    match inner {
        Some(inner) => format!("{lead}{inner}{trail}"),
        None => format!("{lead}{mark}{text}{mark}{trail}"),
    }
}

/// `[selection](address)` ([`Format::Link`]); without a selection `[]()` with the cursor in the
/// brackets.
fn link(content: &mut Content) {
    match content.selection().filter(|selected| !selected.trim().is_empty() && !selected.contains('\n')) {
        Some(selected) => match html::safe_url(selected.trim()) {
            Some(address) => paste(content, &format!("[{selected}]({address})")),
            None => {
                paste(content, &format!("[{selected}]()"));
                step(content, Motion::Left, 1);
            }
        },
        None => {
            paste(content, "[]()");
            step(content, Motion::Left, 3);
        }
    }
}

/// The length of the mark of `format` at the start of `line` (with the white space before it):
/// `- ` or `* ` of an item, `12. ` of a numbered one, `>` (and one space) of a quote. `None` when
/// the line has none.
fn line_mark(line: &str, format: Format) -> Option<usize> {
    let rest = line.trim_start();
    let lead = line.len() - rest.len();
    let len = match format {
        Format::Bullets => (rest.starts_with("- ") || rest.starts_with("* ")).then_some(2)?,
        Format::Numbers => {
            let digits = rest.find(|c: char| !c.is_ascii_digit()).filter(|end| (1..=6).contains(end))?;
            rest[digits..].starts_with(". ").then_some(digits + 2)?
        }
        Format::Quote => {
            let after = rest.strip_prefix('>')?;
            1 + usize::from(after.starts_with(' '))
        }
        Format::Bold | Format::Italic | Format::Underline | Format::Link => return None,
    };
    Some(lead + len)
}

/// The mark of a list item of either kind at the start of `line`.
fn list_mark(line: &str) -> Option<usize> {
    line_mark(line, Format::Bullets).or_else(|| line_mark(line, Format::Numbers))
}

/// Takes away the first `len` characters of line `at` (white space and ASCII marks only).
fn cut_start(content: &mut Content, at: usize, len: usize) {
    content.move_to(Cursor { position: Position { line: at, column: 0 }, selection: None });
    if len == 0 {
        return;
    }
    for _ in 0..len {
        content.perform(text_editor::Action::Select(Motion::Right));
    }
    content.perform(text_editor::Action::Edit(Edit::Backspace));
}

/// The mark of `format` before every line from the cursor's to the selection's end (blank lines
/// of a selection left as they are); when all of them have it, it is taken away instead. A list
/// mark of the other kind is replaced. The cursor ends at the end of the last line.
fn prefix_lines(content: &mut Content, format: Format) {
    let cursor = content.cursor();
    let other = cursor.selection.unwrap_or(cursor.position);
    let (first, last) = (cursor.position.line.min(other.line), cursor.position.line.max(other.line));
    let lines: Vec<String> =
        (first..=last).filter_map(|at| content.line(at).map(|line| line.text.into_owned())).collect();
    let filled: Vec<&String> = lines.iter().filter(|line| !line.trim().is_empty()).collect();
    let remove = !filled.is_empty() && filled.iter().all(|line| line_mark(line, format).is_some());
    let mut number = 0;
    for (offset, line) in lines.iter().enumerate() {
        if line.trim().is_empty() && first != last {
            continue;
        }
        let at = first + offset;
        let existing = if remove {
            line_mark(line, format)
        } else if format == Format::Quote {
            None
        } else {
            list_mark(line)
        };
        cut_start(content, at, existing.unwrap_or(0));
        if !remove {
            number += 1;
            let mark = match format {
                Format::Numbers => format!("{number}. "),
                Format::Quote => "> ".to_owned(),
                _ => "- ".to_owned(),
            };
            paste(content, &mark);
        }
    }
    content.move_to(Cursor { position: Position { line: last, column: 0 }, selection: None });
    content.perform(text_editor::Action::Move(Motion::End));
}

/// Enter in a list item: a new item below (`- `, or the next number), or, in an empty item, the
/// end of the list (its mark goes). `false` when the cursor is not in the text of a list item, so
/// Enter does what it always does.
fn continue_list(content: &mut Content) -> bool {
    let cursor = content.cursor();
    if cursor.selection.is_some() {
        return false;
    }
    let at = cursor.position.line;
    let Some(line) = content.line(at).map(|line| line.text.into_owned()) else { return false };
    let Some(len) = list_mark(&line) else { return false };
    if cursor.position.column < len {
        return false;
    }
    if line[len..].trim().is_empty() {
        cut_start(content, at, line.chars().count());
        return true;
    }
    let mark = &line[..len];
    let next = match line_mark(&line, Format::Numbers) {
        Some(_) => {
            let lead = &mark[..mark.len() - mark.trim_start().len()];
            let number: u32 = mark.trim().trim_end_matches('.').parse().unwrap_or(0);
            format!("{lead}{}. ", number.saturating_add(1))
        }
        None => mark.to_owned(),
    };
    content.perform(text_editor::Action::Edit(Edit::Enter));
    paste(content, &next);
    true
}

/// The highlighter of the text of formatted mail: [`markup::looks`] line by line, drawn by
/// [`look_format`].
struct MarkupHighlighter {
    line: usize,
}

impl Highlighter for MarkupHighlighter {
    type Settings = ();
    type Highlight = markup::Look;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, markup::Look)>;

    fn new(_settings: &()) -> MarkupHighlighter {
        MarkupHighlighter { line: 0 }
    }

    fn update(&mut self, _settings: &()) {
        self.line = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.line = self.line.min(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.line += 1;
        markup::looks(line).into_iter()
    }

    fn current_line(&self) -> usize {
        self.line
    }
}

/// How a [`markup::Look`] is drawn: marks in `TEXT_3`, quotes in `TEXT_2`; bold asks for the
/// strong weight and italic for the italic style of the text font. The marks stay visible, so the
/// text reads right also where the editor draws one weight only (Iced 0.14 on macOS keeps the
/// line's weight) and for underline, which has no form in the editor.
fn look_format(look: &markup::Look, theme: &Theme) -> highlighter::Format<iced::Font> {
    let colors = palette(theme);
    let color = if look.mark {
        Some(colors.text_3)
    } else if look.quote {
        Some(colors.text_2)
    } else {
        None
    };
    let base = typo::body().font;
    // The design names the strong weight as a font of its own (its family may differ per weight).
    let weight = if look.strong { theme::font_semibold() } else { base };
    let font = iced::Font { style: if look.emphasis { FontStyle::Italic } else { weight.style }, ..weight };
    highlighter::Format { color, font: (font != base).then_some(font) }
}

/// A labelled row of the form: the label in `META` on the left, the control beside it.
fn field<'a>(label: &'static str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let label = container(typo::text(typo::meta(), label).style(theme::text_3).wrapping(Wrapping::None))
        .width(label_width())
        .center_y(size::control());
    row![label, control.into()].spacing(space::s()).align_y(Alignment::Center).into()
}

/// The account's signature under the text: read-only, because the core adds it to the message.
fn signature_block(signature: &str) -> Element<'_, Message> {
    let label = container(typo::text(typo::meta(), tr("Signature")).style(theme::text_3).wrapping(Wrapping::None));
    column![
        iced::widget::rule::horizontal(size::HAIRLINE).style(style::rule),
        row![label.width(label_width()), typo::text(typo::meta(), list::plain(signature)).style(theme::text_2)]
            .spacing(space::s()),
    ]
    .spacing(space::s())
    .into()
}

/// A sentence that something did not work, in a line of the form: the alert icon (the colour only
/// there), the sentence, the reason quietly below it, and the button that dismisses it.
fn notice_line(notice: &Notice, dismiss: Option<Message>) -> Element<'_, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).error_text) });
    let mut words = column![typo::text(typo::body(), notice.what.as_str())].spacing(space::xs());
    if let Some(why) = &notice.why {
        words = words.push(typo::text(typo::meta(), why.as_str()).style(theme::text_3));
    }
    let mut line = row![alert, words.width(Fill)].spacing(space::s());
    if let Some(dismiss) = dismiss {
        line = line.push(widgets::icon_button(Icon::X, tr("Dismiss"), Some(dismiss)));
    }
    line.into()
}

/// The window that could not start: what went wrong and "Retry".
fn failure(notice: &Notice) -> Element<'_, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).error_text) });
    let mut words = column![typo::text(typo::body_strong(), notice.what.as_str()).align_x(Alignment::Center)]
        .spacing(space::xs())
        .align_x(Alignment::Center);
    if let Some(why) = &notice.why {
        words = words.push(typo::text(typo::meta(), why.as_str()).style(theme::text_3).align_x(Alignment::Center));
    }
    let content = column![
        alert,
        words,
        widgets::text_button(Some(Icon::RefreshCw), tr("Retry"), style::button::secondary, Some(Message::Retry)),
    ]
    .spacing(space::m())
    .align_x(Alignment::Center)
    .max_width(size::form_max() / 2.0);
    iced::widget::center(content).padding(space::xl()).into()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use iced::futures::executor::block_on;
    use torro_ipc::AccountInfo;

    use super::*;

    const WIDE: Size = Size::new(1600.0, 1000.0);
    const MEDIUM: Size = Size::new(800.0, 600.0);
    const NARROW: Size = Size::new(480.0, 900.0);
    const SIZES: [Size; 6] =
        [Size::new(240.0, 160.0), Size::new(320.0, 480.0), Size::new(1200.0, 200.0), NARROW, MEDIUM, WIDE];
    const OWN: [&str; 2] = ["anna@work.example.org", "anna@example.org"];

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    fn who(name: &str, address: &str) -> MailAddress {
        MailAddress { name: (!name.is_empty()).then(|| name.to_owned()), address: address.to_owned() }
    }

    fn own() -> Vec<String> {
        OWN.iter().map(|address| (*address).to_owned()).collect()
    }

    fn demo_mail(id: &str) -> Mail {
        DemoSource::new(Utc::now()).get_mail(id).expect("a message of the demo")
    }

    /// A window on the demo, started with `words`, with the accounts and the message in.
    fn ready(words: &[&str]) -> Window<DemoSource> {
        let source = Arc::new(DemoSource::new(Utc::now()));
        ready_on(source, words)
    }

    fn ready_on<S: ComposeSource + 'static>(source: Arc<S>, words: &[&str]) -> Window<S> {
        let mut all = vec!["--compose", "--demo"];
        all.extend_from_slice(words);
        let launch = Launch::parse(&args(&all)).expect("valid parameters");
        let kind = launch.compose.as_ref().map(|start| start.kind.clone()).expect("a compose window");
        let mut window = Window::new(source.clone(), Source::from_args(args(&["--demo"])), launch).0;
        let prepared = block_on(prepare(source, kind));
        let _ = window.update(Message::Loaded(prepared));
        assert!(window.ready(), "the window is ready to write");
        window
    }

    /// What the core answers to the last save the window started, handed to `update` as the task
    /// would.
    fn deliver_save<S: ComposeSource + 'static>(window: &mut Window<S>) -> Task<Message> {
        let Some(Call::Save(draft)) = window.calls.iter().rev().find(|call| matches!(call, Call::Save(_))).cloned()
        else {
            panic!("a save was started")
        };
        let revision = window.saving.expect("a save is running");
        let result = block_on(save_with_fallback(window.source.clone(), *draft));
        window.update(Message::Saved(revision, Box::new(result)))
    }

    fn type_in<S: ComposeSource + 'static>(window: &mut Window<S>, to: &str, subject: &str, text: &str) {
        let _ = window.update(Message::To(to.into()));
        let _ = window.update(Message::Subject(subject.into()));
        let _ =
            window.update(Message::Body(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text.into())))));
    }

    fn later() -> Instant {
        Instant::now() + AUTOSAVE + Duration::from_secs(1)
    }

    fn exited<S>(window: &Window<S>) -> bool {
        window.calls.contains(&Call::Exit)
    }

    fn sends<S>(window: &Window<S>) -> usize {
        window.calls.iter().filter(|call| matches!(call, Call::Send(_))).count()
    }

    // ----- the pieces -----

    #[test]
    fn recipients_are_read_as_people_type_them() {
        let (found, refused) =
            parse_addresses("Anna <anna@example.org>, bob@example.org; \"Doe, Jane\" <jane@example.org>, nobody");
        assert_eq!(
            found,
            [who("Anna", "anna@example.org"), who("", "bob@example.org"), who("Doe, Jane", "jane@example.org")]
        );
        assert_eq!(refused, ["nobody"]);
        assert_eq!(parse_addresses("  ,; "), (Vec::new(), Vec::new()), "separators alone are nothing");
        assert_eq!(parse_addresses("a@b.org,").0, [who("", "a@b.org")], "a comma at the end is fine");
        // What a field shows reads back as the same people.
        let people = [who("Doe, Jane", "jane@example.org"), who("Bob", "bob@example.org"), who("", "x@example.org")];
        assert_eq!(parse_addresses(&format_addresses(&people)).0, people);
    }

    #[test]
    fn addresses_need_an_at_and_a_domain() {
        for good in ["a@b.org", "first.last+tag@sub.example.co.uk", "ä@müller.de"] {
            assert!(is_address(good), "{good}");
        }
        for bad in
            ["", "a", "a@b", "@b.org", "a@", "a@@b.org", "a b@c.org", "a@b..org", "a@.org", "a@b.org.", "a<b>@c.org"]
        {
            assert!(!is_address(bad), "{bad:?}");
        }
    }

    #[test]
    fn subjects_get_their_prefix_once() {
        assert_eq!(reply_subject("Budget"), "Re: Budget");
        assert_eq!(reply_subject("Re: Budget"), "Re: Budget");
        assert_eq!(reply_subject("AW: Budget"), "AW: Budget", "a German reply is a reply");
        assert_eq!(reply_subject(" re:  Budget "), "re: Budget");
        assert_eq!(reply_subject("Research"), "Re: Research", "a word that starts with re");
        assert_eq!(reply_subject(""), "Re:");
        assert_eq!(forward_subject("Budget"), "Fwd: Budget");
        assert_eq!(forward_subject("Fwd: Budget"), "Fwd: Budget");
        assert_eq!(forward_subject("WG: Budget"), "WG: Budget");
    }

    #[test]
    fn a_quote_puts_a_mark_before_every_line_and_drops_what_cannot_be_trusted() {
        assert_eq!(quote("Hi\n\nthere\n"), "> Hi\n>\n> there");
        assert_eq!(quote("> old\nnew"), "> > old\n> new", "a quote of a quote");
        assert_eq!(quote("a\r\nb"), "> a\n> b");
        assert_eq!(quote("ab\u{202E}cd\u{1b}[31m"), "> abcd[31m", "direction and control characters go");
        assert_eq!(quote("  \n"), "");
    }

    #[test]
    fn a_reply_goes_to_the_sender_and_quotes_the_message() {
        let message = demo_mail("m:w1");
        let kind = Kind::Reply("m:w1".into());
        let opened = opening(&kind, Some(&message), &own());
        assert_eq!(opened.to, [who("Jonas Weber", "jonas.weber@work.example.org")]);
        assert!(opened.cc.is_empty() && opened.attachments.is_empty());
        assert_eq!(opened.subject, "Re: Budget review for Q4");
        assert_eq!(opened.in_reply_to.as_deref(), Some("m:w1"), "the core sets the reply headers from it");
        assert!(opened.body.starts_with("\n\n"), "the cursor has room above the quote");
        assert!(opened.body.contains(&attribution(&message)), "{}", opened.body);
        assert!(opened.body.contains("\n> Hi Anna,\n>\n> the numbers for Q4 are in."), "{}", opened.body);
        let start = tr("On {date}, {who} wrote:").split('{').next().unwrap_or_default();
        assert!(attribution(&message).starts_with(start), "the sentence is the translation's");
    }

    #[test]
    fn a_reply_follows_reply_to_and_an_answer_to_ones_own_mail_goes_to_the_same_people() {
        let mut message = demo_mail("m:w1");
        message.reply_to = vec![who("List", "list@work.example.org")];
        let kind = Kind::Reply("m:w1".into());
        assert_eq!(opening(&kind, Some(&message), &own()).to, [who("List", "list@work.example.org")]);
        // A mail Anna wrote herself: the answer goes to whom she wrote to.
        message.reply_to.clear();
        message.summary.from = Some(who("Anna Berg", "anna@work.example.org"));
        message.summary.to = vec![who("Jonas", "jonas.weber@work.example.org")];
        assert_eq!(opening(&kind, Some(&message), &own()).to, [who("Jonas", "jonas.weber@work.example.org")]);
    }

    #[test]
    fn reply_all_adds_the_others_but_never_the_own_accounts_or_anyone_twice() {
        let mut message = demo_mail("m:w1");
        message.summary.to = vec![who("Anna", "ANNA@work.example.org"), who("Team", "team@work.example.org")];
        message.cc = vec![
            who("Lena", "lena.hoffmann@work.example.org"),
            who("Anna again", "anna@example.org"),
            who("Team twice", "Team@work.example.org"),
            who("Jonas twice", "jonas.weber@work.example.org"),
        ];
        let opened = opening(&Kind::ReplyAll("m:w1".into()), Some(&message), &own());
        assert_eq!(
            opened.to,
            [who("Jonas Weber", "jonas.weber@work.example.org"), who("Team", "team@work.example.org")]
        );
        assert_eq!(opened.cc, [who("Lena", "lena.hoffmann@work.example.org")]);
        assert_eq!(opened.in_reply_to.as_deref(), Some("m:w1"));
    }

    #[test]
    fn a_forward_has_no_recipient_takes_the_attachments_and_is_no_reply() {
        let message = demo_mail("m:w1");
        let opened = opening(&Kind::Forward("m:w1".into()), Some(&message), &own());
        assert!(opened.to.is_empty() && opened.cc.is_empty());
        assert_eq!(opened.subject, "Fwd: Budget review for Q4");
        assert_eq!(opened.in_reply_to, None);
        assert_eq!(
            opened.attachments,
            [Attached::Part { message: "m:w1".into(), part: "2".into(), name: "budget-q4.csv".into(), size: 2_340 }]
        );
        assert_eq!(
            opened.attachments[0].draft(),
            DraftAttachment::MailPart { message: "m:w1".into(), part: "2".into() }
        );
        let header = forward_header(&message);
        assert!(opened.body.contains(&header), "{}", opened.body);
        assert!(header.contains("Jonas Weber <jonas.weber@work.example.org>") && header.contains("Budget review"));
        assert!(opened.body.contains("the numbers for Q4 are in."));
        assert!(!opened.body.contains("> Hi"), "a forward is not quoted");
        // Pictures inside the text are not attachments.
        let mut inline = message.clone();
        inline.attachments[0].inline = true;
        assert!(opening(&Kind::Forward("m:w1".into()), Some(&inline), &own()).attachments.is_empty());
    }

    #[test]
    fn a_new_message_starts_empty() {
        assert_eq!(opening(&Kind::New, None, &own()), Opening::default());
        assert_eq!(opening(&Kind::New, Some(&demo_mail("m:w1")), &own()), Opening::default());
    }

    #[test]
    fn the_account_to_write_from_is_the_one_asked_for_or_the_one_the_mail_came_to() {
        let sender = |address: &str| Sender {
            account: format!("mail:{address}"),
            address: address.into(),
            name: String::new(),
            signature: None,
            html: true,
        };
        let senders = [sender("anna@work.example.org"), sender("anna@example.org")];
        let message = demo_mail("m:p1");
        assert_eq!(message.summary.account, "mail:anna@example.org");
        assert_eq!(pick_sender(&senders, None, Some(&message)), Some(1), "the account of the message");
        assert_eq!(
            pick_sender(&senders, Some("mail:anna@work.example.org"), Some(&message)),
            Some(0),
            "--account wins"
        );
        assert_eq!(pick_sender(&senders, Some("mail:nobody@example.org"), None), Some(0), "unknown: the first");
        assert_eq!(
            pick_sender_with_default(&senders, None, None, Some("mail:anna@example.org")),
            Some(1),
            "a new message uses the saved default"
        );
        assert_eq!(
            pick_sender_with_default(&senders, None, Some(&message), Some("mail:anna@work.example.org")),
            Some(1),
            "a reply keeps the received account"
        );
        let mut moved = message;
        moved.summary.account = "mail:old@example.org".into();
        moved.summary.to = vec![who("", "ANNA@example.org")];
        assert_eq!(pick_sender(&senders, None, Some(&moved)), Some(1), "the one named among the recipients");
        assert_eq!(pick_sender(&[], None, None), None);
    }

    #[test]
    fn the_senders_are_the_mail_accounts_with_mail_on() {
        let account = |json: serde_json::Value| -> AccountInfo { serde_json::from_value(json).expect("an account") };
        let mail = |address: &str, name: &str, signature: Option<&str>, mail_on: bool| {
            serde_json::json!({
                "address": address, "sender_name": name,
                "imap": {"host": "imap.example.org", "port": 993, "security": "tls"},
                "smtp": {"host": "smtp.example.org", "port": 587, "security": "starttls"},
                "settings": {"signature": signature, "services": {"mail": mail_on, "calendars": false, "tasks": false},
                    "format": if address.starts_with("blank") { "plain" } else { "html" }},
                "mail": {}
            })
        };
        let info = |kind: &str, id: &str, mail: Option<serde_json::Value>| {
            let mut json = serde_json::json!({
                "id": id, "kind": kind, "label": id, "state": {"state": "not-connected"}, "collections": []
            });
            if let Some(mail) = mail {
                json["mail"] = mail;
            }
            account(json)
        };
        let accounts = [
            info("nextcloud", "nextcloud:anna@cloud.example.org", None),
            info("mail", "mail:anna@example.org", Some(mail("anna@example.org", "Anna", Some("Anna\nBerg"), true))),
            info("mail", "mail:off@example.org", Some(mail("off@example.org", "", None, false))),
            info("mail", "mail:blank@example.org", Some(mail("blank@example.org", "", Some("  "), true))),
        ];
        let senders = senders_of(&accounts);
        assert_eq!(
            senders,
            [
                Sender {
                    account: "mail:anna@example.org".into(),
                    address: "anna@example.org".into(),
                    name: "Anna".into(),
                    signature: Some("Anna\nBerg".into()),
                    html: true,
                },
                Sender {
                    account: "mail:blank@example.org".into(),
                    address: "blank@example.org".into(),
                    name: String::new(),
                    signature: None,
                    html: false,
                },
            ],
            "no other kind, no account with mail off, a blank signature is none, the format as set"
        );
        assert_eq!(senders[0].label(), "Anna <anna@example.org>");
        assert_eq!(senders[1].label(), "blank@example.org");
    }

    #[test]
    fn files_are_checked_before_they_are_attached() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let small = dir.path().join("notes.txt");
        std::fs::write(&small, "hello").expect("written");
        assert_eq!(
            Attached::file(&small),
            Ok(Attached::File { path: small.clone(), name: "notes.txt".into(), size: 5 })
        );
        let big = dir.path().join("big.bin");
        std::fs::File::create(&big).and_then(|file| file.set_len(MAX_FILE_BYTES + 1)).expect("a sparse file");
        let refusal = Attached::file(&big).expect_err("over the limit");
        assert!(refusal.contains("big.bin") && refusal.contains(&mail::size_text(MAX_FILE_BYTES)), "{refusal}");
        let folder = dir.path().file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        assert_eq!(Attached::file(dir.path()), Err(trf("{file} is not a file.", &[("file", &folder)])));
        assert!(Attached::file(&dir.path().join("gone.txt")).is_err());
    }

    // ----- the start parameters -----

    #[test]
    fn the_start_parameters_name_the_kind_the_account_and_the_state() {
        let parse = |words: &[&str]| Start::parse(&args(words), true);
        assert_eq!(parse(&["--demo"]), Ok(None), "no --compose, no compose window");
        let kind = |words: &[&str]| parse(words).expect("valid").expect("a window").kind;
        assert_eq!(kind(&["--compose"]), Kind::New);
        assert_eq!(kind(&["--compose", "--reply", "m:1"]), Kind::Reply("m:1".into()));
        assert_eq!(kind(&["--compose", "--reply-all=m:2"]), Kind::ReplyAll("m:2".into()));
        assert_eq!(kind(&["--compose", "--forward", "m:3"]), Kind::Forward("m:3".into()));
        let start = parse(&["--compose", "--account", "mail:anna@example.org", "--open", "menu"]).expect("valid");
        assert_eq!(
            start,
            Some(Start {
                kind: Kind::New,
                account: Some("mail:anna@example.org".into()),
                open: Some(Open::Menu),
                attach: None
            })
        );
        for (name, state) in Open::ALL {
            let start = parse(&["--compose", "--open", name]).expect("valid").expect("a window");
            assert_eq!(start.open, Some(state));
        }
    }

    /// Task WZ.8: `--attach <path>` (or `--attach=<path>`) names a file that goes along; it needs
    /// `--compose` and a path.
    #[test]
    fn the_start_parameters_take_a_file_to_attach() {
        let parse = |words: &[&str]| Start::parse(&args(words), false);
        let attach = |words: &[&str]| parse(words).expect("valid").expect("a window").attach;
        assert_eq!(
            attach(&["--compose", "--attach", "/home/anna/Offer.pdf"]),
            Some(PathBuf::from("/home/anna/Offer.pdf"))
        );
        assert_eq!(
            attach(&["--compose", "--attach=/home/anna/Offer.pdf"]),
            Some(PathBuf::from("/home/anna/Offer.pdf"))
        );
        assert_eq!(attach(&["--compose"]), None);
        assert_eq!(parse(&["--attach", "/home/anna/Offer.pdf"]), Err(LaunchError::NeedsCompose("--attach")));
        assert_eq!(parse(&["--compose", "--attach"]), Err(LaunchError::MissingValue("--attach")));
        assert_eq!(parse(&["--compose", "--attach="]), Err(LaunchError::MissingValue("--attach")));
    }

    /// Task WZ.8: the file of `--attach` is attached once the window is ready, and saved with the
    /// draft; a file that cannot go says why and leaves the window open.
    #[test]
    fn a_file_given_at_the_start_goes_along() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("Offer.pdf");
        std::fs::write(&file, b"%PDF-1.7 offer").expect("written");
        let path = file.to_string_lossy().into_owned();
        let window = ready(&["--attach", &path]);
        assert_eq!(window.attachments, [Attached::File { path: file.clone(), name: "Offer.pdf".into(), size: 14 }]);
        assert!(window.unsaved(), "saved as a draft soon");
        assert_eq!(window.draft().attachments, [DraftAttachment::File { path }]);
        let gone = dir.path().join("Gone.pdf").to_string_lossy().into_owned();
        let window = ready(&["--attach", &gone]);
        assert!(window.attachments.is_empty());
        assert_eq!(window.notice.as_ref().map(|notice| notice.what.as_str()), Some(tr("Couldn't attach the file.")));
    }

    // ----- the window -----

    #[test]
    fn a_new_message_starts_in_the_first_account_with_its_signature_and_asks_for_a_recipient() {
        let mut window = ready(&[]);
        assert_eq!(window.from, 0);
        let draft = window.draft();
        assert_eq!((draft.account.as_str(), draft.id), ("mail:anna@work.example.org", None));
        assert_eq!(
            window.sender().and_then(|sender| sender.signature.as_deref()),
            Some("Anna Berg\nProduct, Torro Workplace")
        );
        assert_eq!(window.problem(), Some(Notice::plain(tr("Add at least one recipient."))));
        assert!(!window.unsaved() && window.status().is_none());
        let _ = window.update(Message::To("nobody".into()));
        let problem = window.problem().expect("not an address");
        assert_eq!(problem.what, trf("{address} isn't a valid address.", &[("address", &"nobody")]));
        // Pressing "Send" with that says so and sends nothing.
        let _ = window.update(Message::Send);
        assert_eq!(window.notice.as_ref(), Some(&problem));
        assert!(window.calls.is_empty() && !window.sending);
    }

    #[test]
    fn an_answer_opens_in_the_account_the_mail_came_to_with_its_fields_filled() {
        let window = ready(&["--reply-all", "m:p1"]);
        assert_eq!(window.sender().map(|sender| sender.account.as_str()), Some("mail:anna@example.org"));
        let draft = window.draft();
        assert_eq!(draft.in_reply_to.as_deref(), Some("m:p1"));
        assert!(!draft.to.is_empty() && draft.subject.starts_with("Re: "));
        assert!(draft.body.contains("\n> "), "the quote is in the text: {}", draft.body);
        let chosen = ready(&["--reply", "m:p1", "--account", "anna@work.example.org"]);
        assert_eq!(chosen.sender().map(|sender| sender.account.as_str()), Some("mail:anna@work.example.org"));
    }

    #[test]
    fn changes_are_saved_a_few_seconds_after_the_last_one_and_the_id_replaces_the_draft() {
        let mut window = ready(&[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        assert!(window.unsaved() && window.changed.is_some());
        assert_eq!(window.status(), Some(tr("Unsaved changes")));
        let _ = window.update(Message::Tick(Instant::now()));
        assert!(window.calls.is_empty(), "too soon");
        let _ = window.update(Message::Tick(later()));
        assert_eq!(window.calls.len(), 1, "saved once the time was up");
        assert_eq!(window.status(), Some(tr("Saving…")));
        let _ = window.update(Message::Tick(later()));
        assert_eq!(window.calls.len(), 1, "not again while a save runs");
        let _ = deliver_save(&mut window);
        assert!(!window.unsaved() && window.changed.is_none());
        assert_eq!(window.status(), Some(tr("Draft saved")));
        let saved = window.source.drafts();
        assert_eq!(saved.len(), 1);
        assert_eq!((saved[0].to.clone(), saved[0].subject.as_str()), (vec![who("", "bob@example.org")], "Lunch"));
        assert_eq!(saved[0].body, "Shall we?");

        // The next change replaces that draft instead of adding another.
        let _ = window.update(Message::Subject("Lunch tomorrow".into()));
        let _ = window.update(Message::Tick(later()));
        let Some(Call::Save(draft)) = window.calls.last().cloned() else { panic!("a second save") };
        assert_eq!(draft.id, saved[0].id, "the id the core gave replaces the draft");
        let _ = deliver_save(&mut window);
        let after = window.source.drafts();
        assert_eq!((after.len(), after[0].subject.as_str()), (1, "Lunch tomorrow"));
    }

    #[test]
    fn nothing_is_saved_that_was_not_touched() {
        let mut window = ready(&["--reply", "m:w1"]);
        let _ = window.update(Message::Tick(later()));
        assert!(window.calls.is_empty() && !window.unsaved(), "an answer nobody changed is no draft");
        assert!(window.changed.is_none(), "and no clock runs");
    }

    #[test]
    fn half_typed_recipients_do_not_stop_the_autosave() {
        let mut window = ready(&[]);
        let _ = window.update(Message::To("bob@example.org, ali".into()));
        let _ = window.update(Message::Tick(later()));
        let Some(Call::Save(draft)) = window.calls.last().cloned() else { panic!("a save") };
        assert_eq!(draft.to, [who("", "bob@example.org")], "the part that is an address is saved");
        let _ = deliver_save(&mut window);
        assert_eq!(window.to, "bob@example.org, ali", "the field keeps what was typed");
    }

    #[test]
    fn edits_made_while_a_save_runs_are_saved_in_the_next_round() {
        let mut window = ready(&[]);
        let _ = window.update(Message::Subject("One".into()));
        let _ = window.update(Message::Tick(later()));
        let _ = window.update(Message::Subject("Two".into()));
        let _ = deliver_save(&mut window);
        assert!(window.unsaved(), "the second edit is not in the draft yet");
        assert!(window.changed.is_some(), "and the clock still runs");
        let _ = window.update(Message::Tick(later()));
        let _ = deliver_save(&mut window);
        assert!(!window.unsaved());
        assert_eq!(window.source.drafts()[0].subject, "Two");
    }

    #[test]
    fn a_draft_that_is_gone_is_saved_again_instead_of_lost() {
        let source = Arc::new(DemoSource::new(Utc::now()));
        let draft = |id: Option<&str>| Draft {
            html: None,
            id: id.map(str::to_owned),
            account: "mail:anna@example.org".into(),
            to: vec![who("", "bob@example.org")],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Hello".into(),
            body: "Text".into(),
            in_reply_to: None,
            attachments: Vec::new(),
        };
        let saved = block_on(save_with_fallback(source.clone(), draft(Some("m:draft-77")))).expect("saved anew");
        assert_eq!(saved.id.as_deref(), Some("m:draft-1"));
        assert_eq!(source.drafts().len(), 1);
        let unknown = Draft { account: "mail:nobody@example.org".into(), ..draft(None) };
        assert_eq!(
            block_on(save_with_fallback(source, unknown)),
            Err(ComposeError::NotFound),
            "an account is no draft"
        );
    }

    #[test]
    fn a_failed_save_keeps_the_text_says_so_and_tries_again() {
        let source = Arc::new(Flaky::new());
        let mut window = ready_on(source.clone(), &[]);
        source.fail_save.lock().expect("lock").replace(ComposeError::Offline);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        let _ = window.update(Message::Tick(later()));
        let _ = deliver_save(&mut window);
        let notice = window.notice.clone().expect("a notice");
        assert_eq!(notice.what, tr("Couldn't save the draft. Your text is still here."));
        assert_eq!(notice.why.as_deref(), Some(ComposeError::Offline.to_string().as_str()));
        assert!(window.unsaved() && window.editable(), "nothing is lost, and the fields still take text");
        assert_eq!((window.to.as_str(), window.subject.as_str()), ("bob@example.org", "Lunch"));
        source.fail_save.lock().expect("lock").take();
        let _ = window.update(Message::Tick(later()));
        assert_eq!(window.calls.iter().filter(|call| matches!(call, Call::Save(_))).count(), 2, "it tries again");
    }

    #[test]
    fn sending_saves_what_is_unsaved_and_then_sends_that_draft_and_closes() {
        let mut window = ready(&[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        let _ = window.update(Message::Send);
        assert!(window.sending && !window.editable(), "the fields are locked while it goes");
        assert!(matches!(window.calls.as_slice(), [Call::Save(_)]), "saved first");
        assert_eq!(sends(&window), 0);
        let _ = deliver_save(&mut window);
        let id = window.draft.as_ref().map(|(id, _)| id.clone()).expect("a saved draft");
        assert_eq!(window.calls.last(), Some(&Call::Send(id)), "the draft the core named is the one sent");
        assert!(!exited(&window), "not closed before the server took it");
        let _ = window.update(Message::Sent(Ok(Delivery::Accepted)));
        assert!(exited(&window));
    }

    #[test]
    fn a_send_that_fails_leaves_the_window_the_text_and_the_draft_where_they_are() {
        let source = Arc::new(Flaky::new());
        let mut window = ready_on(source.clone(), &[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        let _ = window.update(Message::Send);
        let _ = deliver_save(&mut window);
        // The core answers with its own sentence, which says nothing was sent.
        let reason = "The outgoing mail server cannot be reached. Nothing was sent; try again later.";
        let _ = window.update(Message::Sent(Err(ComposeError::Other(reason.into()))));

        assert!(!exited(&window), "the window stays");
        assert!(!window.sending && window.editable(), "and can be written in again");
        let notice = window.notice.clone().expect("the error stays in the window");
        assert_eq!((notice.what.as_str(), notice.why.as_deref()), (tr("Couldn't send the message."), Some(reason)));
        assert_eq!((window.to.as_str(), window.subject.as_str()), ("bob@example.org", "Lunch"));
        assert_eq!(window.body.text(), "Shall we?");
        let kept = source.inner.drafts();
        assert_eq!(kept.len(), 1, "the draft is still in the drafts folder");
        assert_eq!((kept[0].subject.as_str(), kept[0].body.as_str()), ("Lunch", "Shall we?"));
        assert!(source.inner.sent().is_empty(), "nothing went out");
        assert!(window.draft.is_some(), "and the window still knows it");

        // Trying again sends the same draft, without saving it a second time.
        let saves = window.calls.iter().filter(|call| matches!(call, Call::Save(_))).count();
        let _ = window.update(Message::Send);
        assert_eq!(window.calls.iter().filter(|call| matches!(call, Call::Save(_))).count(), saves);
        assert_eq!(sends(&window), 2);
        let _ = window.update(Message::Sent(Ok(Delivery::Accepted)));
        assert!(exited(&window));
    }

    #[test]
    fn the_demo_refuses_a_draft_without_recipient_as_the_core_does() {
        let demo = DemoSource::new(Utc::now());
        let draft = Draft {
            html: None,
            id: None,
            account: "mail:anna@example.org".into(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: String::new(),
            body: String::new(),
            in_reply_to: None,
            attachments: Vec::new(),
        };
        let id = DemoSource::save_draft(&demo, draft).expect("saved").id.expect("an id");
        let err = block_on(ComposeSource::send_draft(&demo, id.clone())).expect_err("no recipient");
        assert_eq!(err, ComposeError::Other(tr("The draft has no recipient.").into()));
        assert_eq!(demo.drafts().len(), 1, "the draft stays");
        assert_eq!(block_on(ComposeSource::send_draft(&demo, "m:nope".into())), Err(ComposeError::NotFound));
    }

    #[test]
    fn a_send_of_unknown_outcome_stays_and_is_not_sent_again() {
        let mut window = ready(&[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        let _ = window.update(Message::Send);
        let _ = deliver_save(&mut window);
        let _ = window.update(Message::Sent(Ok(Delivery::Unknown)));
        assert!(!exited(&window) && window.unsure);
        assert_eq!(
            window.notice.as_ref().map(|notice| notice.what.as_str()),
            Some(tr("The message may or may not have gone out."))
        );
        let _ = window.update(Message::Send);
        assert_eq!(sends(&window), 1, "never twice by itself");
        // The draft is in the drafts folder, so closing loses nothing.
        let _ = window.update(Message::Close);
        assert!(exited(&window));
    }

    #[test]
    fn a_draft_the_core_lost_is_saved_anew_by_the_next_send() {
        let mut window = ready(&[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        let _ = window.update(Message::Send);
        let _ = deliver_save(&mut window);
        let _ = window.update(Message::Sent(Err(ComposeError::NotFound)));
        assert!(window.draft.is_none() && window.unsaved(), "the next send saves again");
        let _ = window.update(Message::Send);
        assert!(matches!(window.calls.last(), Some(Call::Save(_))));
    }

    #[test]
    fn closing_asks_only_when_something_is_unsaved() {
        let mut window = ready(&[]);
        let _ = window.update(Message::Escape);
        assert!(exited(&window), "nothing written, nothing to ask");

        let mut window = ready(&[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        let _ = window.update(Message::Escape);
        assert!(window.asking && !exited(&window), "unsaved: asked first");
        let _ = window.update(Message::Escape);
        assert!(!window.asking, "Esc in the question keeps writing");
        let _ = window.update(Message::Close);
        assert!(window.asking, "the close button asks too");
        let _ = window.update(Message::KeepWriting);
        assert!(!window.asking && !exited(&window));

        let _ = window.update(Message::Close);
        let _ = window.update(Message::DiscardChanges);
        assert!(exited(&window) && window.source.drafts().is_empty(), "discarded: nothing was saved");
    }

    #[test]
    fn saving_from_the_question_closes_after_the_save() {
        let mut window = ready(&[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        let _ = window.update(Message::Escape);
        let _ = window.update(Message::SaveAndClose);
        assert!(!window.asking && !exited(&window), "closes after the save, not before");
        let _ = deliver_save(&mut window);
        assert!(exited(&window));
        assert_eq!(window.source.drafts().len(), 1);

        // Once saved, closing asks nothing.
        let mut window = ready(&[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        let _ = window.update(Message::Tick(later()));
        let _ = deliver_save(&mut window);
        let _ = window.update(Message::Escape);
        assert!(exited(&window) && !window.asking);
    }

    #[test]
    fn a_failed_save_on_close_keeps_the_window_open() {
        let source = Arc::new(Flaky::new());
        let mut window = ready_on(source.clone(), &[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        source.fail_save.lock().expect("lock").replace(ComposeError::Offline);
        let _ = window.update(Message::Escape);
        let _ = window.update(Message::SaveAndClose);
        let _ = deliver_save(&mut window);
        assert!(!exited(&window) && window.notice.is_some(), "the text is not thrown away");
    }

    #[test]
    fn another_account_gets_a_new_draft_and_the_old_one_goes_to_the_trash() {
        let mut window = ready(&[]);
        type_in(&mut window, "bob@example.org", "Lunch", "Shall we?");
        let _ = window.update(Message::Tick(later()));
        let _ = deliver_save(&mut window);
        let old = window.draft.as_ref().map(|(id, _)| id.clone()).expect("saved");
        let _ = window.update(Message::PickFrom(1));
        assert_eq!(window.draft().account, "mail:anna@example.org");
        assert_eq!(window.draft().id, None, "the old draft lives in the other account");
        let _ = window.update(Message::Tick(later()));
        assert!(window.calls.contains(&Call::Discard(old)));
        let _ = deliver_save(&mut window);
        assert_eq!(window.draft.as_ref().map(|(_, account)| account.as_str()), Some("mail:anna@example.org"));
    }

    #[test]
    fn attachments_are_added_removed_and_end_up_in_the_draft() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("plan.pdf");
        std::fs::write(&path, vec![0u8; 3000]).expect("written");
        let mut window = ready(&["--forward", "m:w1"]);
        assert_eq!(window.attachments.len(), 1, "the forwarded one");
        let _ = window.update(Message::Picked(vec![path.clone(), path.clone()]));
        assert_eq!(window.attachments.len(), 2, "the same file twice is one");
        assert!(window.unsaved());
        let draft = window.draft();
        assert_eq!(draft.attachments[1], DraftAttachment::File { path: path.to_string_lossy().into_owned() });
        assert_eq!(draft.in_reply_to, None);
        let _ = window.update(Message::RemoveAttachment(0));
        assert_eq!(window.draft().attachments.len(), 1);

        let _ = window.update(Message::Picked(vec![dir.path().join("gone.pdf")]));
        let notice = window.notice.clone().expect("it could not be attached");
        assert_eq!(notice.what, tr("Couldn't attach the file."));
        assert_eq!(window.attachments.len(), 1);
    }

    #[test]
    fn no_more_attachments_than_the_core_takes() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let paths: Vec<PathBuf> = (0..=MAX_ATTACHMENTS)
            .map(|n| {
                let path = dir.path().join(format!("f{n}.txt"));
                std::fs::write(&path, "x").expect("written");
                path
            })
            .collect();
        let mut window = ready(&[]);
        let _ = window.update(Message::Picked(paths));
        assert_eq!(window.attachments.len(), MAX_ATTACHMENTS);
        let notice = window.notice.clone().expect("told");
        assert_eq!(notice.why, Some(trf("At most {n} attachments fit in one message.", &[("n", &MAX_ATTACHMENTS)])));
    }

    #[test]
    fn the_keys_are_escape_ctrl_enter_and_tab() {
        let press = |key: Key, modifiers: keyboard::Modifiers| {
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
                location: keyboard::Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        let map = |event| key(event, iced::event::Status::Ignored, iced::window::Id::unique());
        let none = keyboard::Modifiers::empty();
        assert!(matches!(map(press(Key::Named(Named::Escape), none)), Some(Message::Escape)));
        assert!(matches!(map(press(Key::Named(Named::Enter), keyboard::Modifiers::COMMAND)), Some(Message::Send)));
        assert!(map(press(Key::Named(Named::Enter), none)).is_none(), "plain Enter is the text's");
        assert!(matches!(map(press(Key::Named(Named::Tab), none)), Some(Message::Focus(true))));
        assert!(matches!(map(press(Key::Named(Named::Tab), keyboard::Modifiers::SHIFT)), Some(Message::Focus(false))));
        assert!(map(press(Key::Character("a".into()), none)).is_none());
    }

    /// Tasks E10.1, E10.1b: on macOS Ctrl + Enter sends as ⌘ + Enter does, and Apple Mail's ⇧⌘D
    /// too; elsewhere only Ctrl + Enter.
    #[test]
    fn send_is_ctrl_enter_and_on_a_mac_also_cmd_enter() {
        let enter = Key::Named(Named::Enter);
        let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
        let (ctrl, logo, shift) = (keyboard::Modifiers::CTRL, keyboard::Modifiers::LOGO, keyboard::Modifiers::SHIFT);
        assert!(is_send(System::Mac, &enter, physical, logo) && is_send(System::Mac, &enter, physical, ctrl));
        assert!(!is_send(System::Mac, &enter, physical, ctrl | shift));
        let d = Key::Character("D".into());
        assert!(is_send(System::Mac, &d, physical, logo | shift), "⇧⌘D, as in Apple Mail");
        for other in [System::Windows, System::Linux] {
            assert!(is_send(other, &enter, physical, ctrl), "{other:?}");
            assert!(!is_send(other, &enter, physical, logo), "the logo key is no Ctrl: {other:?}");
            assert!(!is_send(other, &enter, physical, keyboard::Modifiers::empty()), "Enter is the text's: {other:?}");
            assert!(!is_send(other, &d, physical, ctrl | shift), "Thunderbird and Outlook have no ⇧D: {other:?}");
        }
        let wanted = if cfg!(target_os = "macos") { "Shift+Cmd+D" } else { "Ctrl+Enter" };
        assert_eq!(send_keys(), wanted);
    }

    // ----- formatted mail (issue #125) -----

    fn text_of(content: &Content) -> String {
        content.text().trim_end_matches('\n').to_owned()
    }

    /// `text` in an editor with the cursor at its end.
    fn editor(text: &str) -> Content {
        let mut content = Content::with_text(text);
        content.perform(text_editor::Action::Move(Motion::DocumentEnd));
        content
    }

    fn select_all(content: &mut Content) {
        content.perform(text_editor::Action::SelectAll);
    }

    #[test]
    fn bold_italic_and_underline_wrap_the_selection_or_wait_for_text() {
        let mut content = editor("Hello world");
        select_all(&mut content);
        apply_format(&mut content, Format::Bold);
        assert_eq!(text_of(&content), "**Hello world**");
        select_all(&mut content);
        apply_format(&mut content, Format::Bold);
        assert_eq!(text_of(&content), "Hello world", "again takes it away");

        let mut content = editor("Hi ");
        apply_format(&mut content, Format::Italic);
        paste(&mut content, "there");
        assert_eq!(text_of(&content), "Hi *there*", "the cursor waits between the marks");

        let mut content = editor("one\n\n  two ");
        select_all(&mut content);
        apply_format(&mut content, Format::Underline);
        assert_eq!(text_of(&content), "__one__\n\n  __two__ ", "each line, blank ones and edges left");
    }

    #[test]
    fn links_take_the_selection_and_an_address_of_its_own() {
        let mut content = editor("https://example.org/q4");
        select_all(&mut content);
        apply_format(&mut content, Format::Link);
        assert_eq!(text_of(&content), "[https://example.org/q4](https://example.org/q4)");

        let mut content = editor("the sheet");
        select_all(&mut content);
        apply_format(&mut content, Format::Link);
        paste(&mut content, "https://example.org");
        assert_eq!(text_of(&content), "[the sheet](https://example.org)", "the cursor waits for the address");

        let mut content = editor("See ");
        apply_format(&mut content, Format::Link);
        paste(&mut content, "this");
        assert_eq!(text_of(&content), "See [this]()");
    }

    #[test]
    fn lists_and_quotes_mark_the_lines_and_take_the_mark_away_again() {
        let mut content = editor("one\ntwo\n\nthree");
        select_all(&mut content);
        apply_format(&mut content, Format::Numbers);
        assert_eq!(text_of(&content), "1. one\n2. two\n\n3. three");
        select_all(&mut content);
        apply_format(&mut content, Format::Bullets);
        assert_eq!(text_of(&content), "- one\n- two\n\n- three", "the other kind of list is replaced");
        select_all(&mut content);
        apply_format(&mut content, Format::Bullets);
        assert_eq!(text_of(&content), "one\ntwo\n\nthree", "again takes it away");

        let mut content = editor("first\nsecond");
        apply_format(&mut content, Format::Quote);
        assert_eq!(text_of(&content), "first\n> second", "without a selection the cursor's line");
        apply_format(&mut content, Format::Quote);
        assert_eq!(text_of(&content), "first\nsecond");
    }

    #[test]
    fn enter_in_a_list_item_starts_the_next_and_an_empty_one_ends_the_list() {
        let mut content = editor("- milk");
        assert!(continue_list(&mut content));
        paste(&mut content, "bread");
        assert_eq!(text_of(&content), "- milk\n- bread");
        let mut content = editor("  9. nine");
        assert!(continue_list(&mut content));
        assert_eq!(text_of(&content), "  9. nine\n  10. ");
        assert!(continue_list(&mut content), "an empty item");
        assert_eq!(content.line_count(), 2, "the line stays");
        assert_eq!(text_of(&content), "  9. nine", "without its mark: the list ends");
        let mut plain = editor("no list");
        assert!(!continue_list(&mut plain), "Enter as always");
    }

    #[test]
    fn the_keys_of_formatting_are_command_and_a_letter() {
        let command = keyboard::Modifiers::COMMAND;
        let key = |letter: &str| Key::Character(letter.into());
        assert_eq!(format_key(&key("b"), command), Some(Format::Bold));
        assert_eq!(format_key(&key("I"), command), Some(Format::Italic));
        assert_eq!(format_key(&key("u"), command), Some(Format::Underline));
        assert_eq!(format_key(&key("k"), command), Some(Format::Link));
        assert_eq!(format_key(&key("b"), keyboard::Modifiers::empty()), None, "typing a b");
        assert_eq!(format_key(&key("b"), command | keyboard::Modifiers::SHIFT), None);
        assert_eq!(format_key(&key("x"), command), None);
        let tip = trf("{name} ({keys})", &[("name", &tr("Bold")), ("keys", &shortcut("B"))]);
        assert!(tip.contains("+B"), "{tip}");
    }

    #[test]
    fn formatted_mail_is_saved_as_html_with_the_history_marked_and_plain_mail_as_text() {
        let mut window = ready(&["--reply", "m:w1"]);
        assert!(window.formatted(), "the demo writes formatted mail");
        let _ = window.update(Message::Body(text_editor::Action::Move(Motion::DocumentStart)));
        let _ = window.update(Message::Body(text_editor::Action::Edit(Edit::Paste(Arc::new("Thanks, ".into())))));
        let _ = window.update(Message::Format(Format::Bold));
        let _ = window.update(Message::Body(text_editor::Action::Edit(Edit::Paste(Arc::new("Jonas".into())))));
        let draft = window.draft();
        let html = draft.html.expect("formatted");
        assert!(html.starts_with("<p>Thanks, <strong>Jonas</strong></p><div class=\"torro-quote\">"), "{html}");
        assert!(html.contains("<blockquote type=\"cite\">"), "the history is quoted: {html}");
        assert_eq!(html::sanitize(&html), html, "nothing the core would take away");
        assert!(draft.body.starts_with("Thanks, Jonas\n\n"), "the plain text beside it: {}", draft.body);
        assert!(window.unsaved(), "formatting is a change");

        // An account that writes plain text: no toolbar, no HTML, the marks stay as typed.
        window.senders.iter_mut().for_each(|sender| sender.html = false);
        let _ = window.update(Message::Format(Format::Italic));
        let plain = window.draft();
        assert_eq!(plain.html, None);
        assert!(plain.body.starts_with("Thanks, **Jonas**"), "{}", plain.body);
        for size in [WIDE, NARROW] {
            let _ = window.update(Message::Resized(size));
            let _ = window.view();
        }
    }

    #[test]
    fn answers_and_forwards_know_where_the_history_starts() {
        let message = demo_mail("m:w1");
        let reply = opening(&Kind::Reply("m:w1".into()), Some(&message), &own());
        assert_eq!(reply.history.as_deref(), Some(attribution(&message).as_str()));
        let forward = opening(&Kind::Forward("m:w1".into()), Some(&message), &own());
        let first = forward_header(&message).lines().next().map(str::to_owned);
        assert_eq!(forward.history, first);
        let html = markup::to_html(&forward.body, forward.history.as_deref());
        assert!(html.starts_with("<div class=\"torro-quote\">"), "the forwarded text is not own text: {html}");
    }

    #[test]
    fn nothing_is_typed_or_sent_while_the_window_is_not_ready() {
        let mut window = ready(&[]);
        window.phase = Phase::Loading;
        let _ = window.update(Message::Subject("x".into()));
        let _ = window.update(Message::Send);
        assert!(window.subject.is_empty() && window.calls.is_empty());
    }

    #[test]
    fn every_state_draws_in_every_size() {
        let states: Vec<&[&str]> = vec![
            &[],
            &["--reply", "m:w1"],
            &["--forward", "m:w1", "--open", "attach"],
            &["--open", "cc"],
            &["--open", "error"],
            &["--open", "unsure"],
            &["--open", "discard"],
            &["--open", "menu"],
        ];
        for words in states {
            let mut window = ready(words);
            for size in SIZES {
                window.size = size;
                let _ = window.view();
            }
        }
        for open in ["loading", "failed"] {
            let source = Arc::new(DemoSource::new(Utc::now()));
            let launch = Launch::parse(&args(&["--compose", "--demo", "--open", open])).expect("valid");
            let mut window = Window::new(source, Source::from_args(args(&["--demo"])), launch).0;
            for size in SIZES {
                window.size = size;
                let _ = window.view();
            }
        }
    }

    #[test]
    fn mini_and_strip_show_the_draft_and_nothing_to_edit() {
        let mut window = ready(&[]);
        type_in(&mut window, "Bob <bob@example.org>", "Lunch", "Shall we?");
        for size in [Size::new(240.0, 160.0), Size::new(1200.0, 200.0)] {
            window.size = size;
            assert!(window.is_glance());
            let _ = window.view();
        }
        let _ = window.subscription();
        window.size = MEDIUM;
        assert!(!window.is_glance());
    }

    #[test]
    fn the_window_is_titled_by_its_subject() {
        let mut window = ready(&["--reply", "m:w1"]);
        assert_eq!(window.title(), "Re: Budget review for Q4");
        let _ = window.update(Message::Subject(String::new()));
        assert_eq!(window.title(), tr("Reply"));
        assert_eq!(ready(&[]).title(), tr("New message"));
        assert_eq!(COMPOSE.window_class, "torro-compose");
        assert_eq!(COMPOSE.binary, "torroinbox");
    }

    /// A demo source whose saves can be made to fail.
    struct Flaky {
        inner: DemoSource,
        fail_save: Mutex<Option<ComposeError>>,
    }

    impl Flaky {
        fn new() -> Flaky {
            Flaky { inner: DemoSource::new(Utc::now()), fail_save: Mutex::new(None) }
        }
    }

    impl ComposeSource for Flaky {
        async fn senders(&self) -> Result<Vec<Sender>, ComposeError> {
            ComposeSource::senders(&self.inner).await
        }

        async fn get_mail(&self, id: String) -> Result<Mail, ComposeError> {
            ComposeSource::get_mail(&self.inner, id).await
        }

        async fn save_draft(&self, draft: Draft) -> Result<Draft, ComposeError> {
            if let Some(err) = self.fail_save.lock().expect("lock").clone() {
                return Err(err);
            }
            ComposeSource::save_draft(&self.inner, draft).await
        }

        async fn send_draft(&self, id: String) -> Result<Delivery, ComposeError> {
            ComposeSource::send_draft(&self.inner, id).await
        }

        async fn discard_draft(&self, id: String) -> Result<(), ComposeError> {
            ComposeSource::discard_draft(&self.inner, id).await
        }
    }
}
