//! The compose window (task E4.3 in `docs/tasks/mail.md`, `docs/mail.md`, issue #969): `torroinbox
//! --compose [--reply <id> | --reply-all <id> | --forward <id> | --draft <id>] [--account <id>] [--attach
//! <path>]`, a window of its own with the class `torro-compose`, so a tiling manager puts it beside the list
//! and one can read while writing.
//!
//! Since issue #969 the window is a real WYSIWYG editor: a tao/wry window with the system's webview (WebKit on
//! macOS and Linux, WebView2 on Windows) and the editor Squire, bundled with the program, no network
//! ([`web`], the page in `assets/compose/`). The attempt of issue #917 (`spikes/editor-webview/`) showed that
//! this closes what the old editor of stacked Iced text fields could not do (undo, a selection across
//! paragraphs and pictures, files dropped on Wayland). What the person sees is one continuous formatted
//! document; no marks (`**bold**`) are shown any more.
//!
//! This file is the part that knows nothing of windows: what the window is asked to do ([`Start`], [`Kind`]),
//! what it asks of the core ([`ComposeSource`], one request per method), what an answer starts with
//! ([`opening`], [`reopening`]), the recipients ([`Recipients`]) and how a draft is saved without losing
//! parts ([`save_draft_kept`]). [`session`] holds the state of one open window and builds the draft from what
//! the page reports; [`web`] is the window and the bridge between the page and the session.
//!
//! - **Answers** (issue #627): the quoted history is the element `div.torro-quote` of the document, folded under
//!   "Show history"; it is sent all the same. A forward keeps its history open.
//! - **Recipients as chips:** the page turns what was typed into chips at `,` `;` Enter and when the field is
//!   left; what it reads as an address, and what not, the session decides ([`Recipients`]). Words that are no
//!   address stay as marked chips, and Send names them.
//! - **Suggestions** (issue #460, `contacts.suggest_recipients`): the page asks 150 ms after the last letter
//!   (150 ms); an answer to an older search is dropped.
//! - **Writing on a draft** (issue #502): `--draft <id>` opens a saved draft (`mail.get_draft`) with its HTML,
//!   its pictures and its attachments, which are parts of the draft itself. Saving replaces it.
//! - **Nothing gets lost:** every change is saved as a draft a few seconds after the last one ([`AUTOSAVE`],
//!   `mail.save_draft`). Closing with unsaved changes asks first. "Send" saves what is not saved and then sends
//!   the draft (`mail.send_draft`); an error stays in the window with the text.
//! - **Pictures** (issue #461): a picture dropped on the text, pasted or chosen is prepared by the core
//!   (`mail.prepare_image`); the page shows its preview through the program's own address `torro://img/<cid>`,
//!   and the draft names it as `cid:<cid>` ([`session::Session::draft`]).
//! - **Keys:** the shortcut "send" of the system's preset in `torro_core::shortcuts` sends; `Esc` closes (asking
//!   when something is unsaved); `Cmd`/`Ctrl` + `B` `I` `U` `K` format, `Cmd`/`Ctrl` + `Z` undoes.
//!
//! Mail is foreign text: what a quoted message brings is cleaned of control and direction characters before it
//! enters the text, and everything the page hands back is cleaned again by `torro_mail::html::sanitize` here,
//! and by the core when the draft is saved.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use iced::keyboard::{self, Key};
use torro_contacts::RecipientSuggestion;
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::settings::ComposePaper;
use torro_core::shortcuts::{self, Preset, System};
use torro_gui::datetime;
use torro_inbox::InboxError;
use torro_inbox::demo::DemoSource;
use torro_ipc::inbox::InboxClient;
use torro_ipc::mail::{Delivery, MAX_FILE_BYTES, PreparedImage};
use torro_ipc::{AccountInfo, IpcError, Request, Response};
use torro_mail::{AttachmentInfo, Draft, DraftAttachment, ImageSource, MailAddress, MailFormat, Message as Mail, html};

use crate::app::{LaunchError, flag_value};
use crate::{keys, list, mail};

mod images;
pub mod session;
pub mod web;

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
    /// To write on a saved draft (`--draft <id>`, issue #502).
    Draft(String),
}

impl Kind {
    /// The message answered or passed on.
    pub fn message(&self) -> Option<&str> {
        match self {
            Kind::New | Kind::Draft(_) => None,
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
            // Not "Draft": the state beside it says "Draft saved" (design review of #502).
            Kind::Draft(_) => tr("Continue writing"),
        }
    }

    /// What did not work when the accounts or the message could not be fetched.
    fn load_failed(&self) -> &'static str {
        match self {
            Kind::New => tr("Couldn't open the compose window."),
            Kind::Reply(_) | Kind::ReplyAll(_) | Kind::Forward(_) => tr("Couldn't load the message."),
            Kind::Draft(_) => tr("Couldn't load the draft."),
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
    /// Recipients as chips, one of them no address, and the suggestions for "jo" (issue #460).
    Suggest,
    /// The dark paper (issue #460).
    PaperDark,
    Image,
    ImageDark,
    ImageRemove,
    Paste,
    PasteDark,
    /// An answer with its quoted history unfolded (issue #627).
    History,
    /// The format bar folded with "Aa" (issue #627).
    FormatOff,
    /// An answer with its subject unfolded for editing (issue #627).
    Subject,
    /// The question of "Discard draft" (issue #627).
    DiscardDraft,
}

impl Open {
    /// Its name as `--open` spells it.
    pub fn name(self) -> &'static str {
        Open::ALL.iter().find(|(_, open)| *open == self).map_or("", |(name, _)| name)
    }

    const ALL: [(&'static str, Open); 20] = [
        ("paste", Open::Paste),
        ("paste-dark", Open::PasteDark),
        ("image", Open::Image),
        ("image-dark", Open::ImageDark),
        ("image-remove", Open::ImageRemove),
        ("cc", Open::Cc),
        ("attach", Open::Attach),
        ("error", Open::Error),
        ("unsure", Open::Unsure),
        ("discard", Open::Discard),
        ("menu", Open::Menu),
        ("loading", Open::Loading),
        ("failed", Open::Failed),
        ("format", Open::Format),
        ("suggest", Open::Suggest),
        ("paper-dark", Open::PaperDark),
        ("history", Open::History),
        ("format-off", Open::FormatOff),
        ("subject", Open::Subject),
        ("discard-draft", Open::DiscardDraft),
    ];
}

/// The kind of an answer by its start flag: `Kind::Reply` and the like.
type Answer = fn(String) -> Kind;

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
    /// Reads `--compose`, `--reply <id>`, `--reply-all <id>`, `--forward <id>`, `--draft <id>`,
    /// `--account <id>`,
    /// `--attach <path>` and `--open <state>` from `args`. `None` without `--compose`; then the
    /// flags that only make sense with it are refused, so a mistyped command never opens the inbox
    /// instead. An empty path is a missing value.
    pub fn parse(args: &[String], demo: bool) -> Result<Option<Start>, LaunchError> {
        let answers: [(&'static str, Answer); 4] = [
            ("--reply", Kind::Reply),
            ("--reply-all", Kind::ReplyAll),
            ("--forward", Kind::Forward),
            ("--draft", Kind::Draft),
        ];
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
    /// Whether the window keeps its own view choices (the folded format bar, issue #627) in
    /// `ui.json`: the core's client does, the demo and tests do not, so they never touch the
    /// person's file.
    fn remembers(&self) -> bool {
        false
    }

    /// One `mail.prepare_image` request. Originals and processing stay in the service.
    fn prepare_image(
        &self,
        account: String,
        source: ImageSource,
    ) -> impl Future<Output = Result<PreparedImage, ComposeError>> + Send {
        let _ = (account, source);
        async { Err(ComposeError::Forbidden) }
    }

    /// One bounded `mail.upload_image` request; final replies with the prepared image.
    fn upload_image(&self, request: Request) -> impl Future<Output = Result<Response, ComposeError>> + Send {
        let _ = request;
        async { Err(ComposeError::Forbidden) }
    }

    /// One `mail.release_images` request; no count or foreign information is returned.
    fn release_images(
        &self,
        account: String,
        ids: Vec<String>,
    ) -> impl Future<Output = Result<(), ComposeError>> + Send {
        let _ = (account, ids);
        async { Ok(()) }
    }
    /// `accounts.list_accounts`, reduced to the mail accounts a message can be written from
    /// ([`senders_of`]). Empty without a mail account.
    fn senders(&self) -> impl Future<Output = Result<Vec<Sender>, ComposeError>> + Send;

    /// Preferred account for a new message. Sources without suite settings use the first sender.
    fn default_sender(&self) -> impl Future<Output = Result<Option<String>, ComposeError>> + Send {
        async { Ok(None) }
    }

    /// `mail.get_message` with its text: the message to answer or pass on.
    fn get_mail(&self, id: String) -> impl Future<Output = Result<Mail, ComposeError>> + Send;

    /// `mail.get_draft` (issue #502): the saved draft `id` to write on, with its Bcc, the message
    /// it answers and its attachments as parts of itself; `NotFound` when it is gone.
    fn get_draft(&self, id: String) -> impl Future<Output = Result<Draft, ComposeError>> + Send;

    /// `mail.save_draft`: files `draft` in the account's drafts folder and answers it with its id.
    /// With the id of a saved draft it replaces that draft; `NotFound` when that is gone.
    fn save_draft(&self, draft: Draft) -> impl Future<Output = Result<Draft, ComposeError>> + Send;

    /// `mail.send_draft`: sends the saved draft `id` and files the copy. A server that refuses is
    /// an error whose text ends "Nothing was sent."; [`Delivery::Unknown`] is not: the connection
    /// broke after the message was handed over, and it is never sent again by itself.
    fn send_draft(&self, id: String) -> impl Future<Output = Result<Delivery, ComposeError>> + Send;

    /// `mail.delete_messages` for a draft that is not wanted any more (into the trash).
    fn discard_draft(&self, id: String) -> impl Future<Output = Result<(), ComposeError>> + Send;

    /// `contacts.suggest_recipients` (issue #460): who to write to for `query`, in the order to show.
    /// None where the source has no contacts.
    fn suggest(&self, query: String) -> impl Future<Output = Result<Vec<RecipientSuggestion>, ComposeError>> + Send {
        let _ = query;
        async { Ok(Vec::new()) }
    }

    /// `contacts.get_photo` in the small size: a contact's picture as PNG, `None` without one.
    fn photo(&self, contact: String) -> impl Future<Output = Result<Option<Vec<u8>>, ComposeError>> + Send {
        let _ = contact;
        async { Ok(None) }
    }

    /// The address book "+ Contact" writes to (`contacts.list_address_books`): the first one that
    /// may be written and is not hidden; `None` without one, and then the button is missing.
    fn writable_book(&self) -> impl Future<Output = Result<Option<String>, ComposeError>> + Send {
        async { Ok(None) }
    }

    /// `contacts.create_contact`: a contact with `name` (may be empty) and `address` in `book`.
    fn create_contact(
        &self,
        book: String,
        name: String,
        address: String,
    ) -> impl Future<Output = Result<(), ComposeError>> + Send {
        let _ = (book, name, address);
        async { Err(ComposeError::Forbidden) }
    }

    /// The paper the window starts with (setting `compose_paper`).
    fn paper(&self) -> impl Future<Output = Result<ComposePaper, ComposeError>> + Send {
        async { Ok(ComposePaper::default()) }
    }

    /// `settings.set_setting compose_paper`: keeps the paper for the next window.
    fn set_paper(&self, paper: ComposePaper) -> impl Future<Output = Result<(), ComposeError>> + Send {
        let _ = paper;
        async { Ok(()) }
    }
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
    fn remembers(&self) -> bool {
        true
    }

    async fn prepare_image(&self, account: String, source: ImageSource) -> Result<PreparedImage, ComposeError> {
        match ask(self, Request::PrepareMailImage { account, source }).await? {
            Response::MailImage { image } => Ok(image),
            other => Err(unexpected(other)),
        }
    }

    async fn upload_image(&self, request: Request) -> Result<Response, ComposeError> {
        ask(self, request).await
    }

    async fn release_images(&self, account: String, ids: Vec<String>) -> Result<(), ComposeError> {
        match ask(self, Request::ReleaseMailImages { account, ids }).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }
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

    async fn get_draft(&self, id: String) -> Result<Draft, ComposeError> {
        match ask(self, Request::GetDraft { id }).await? {
            Response::MailDraft { draft } => Ok(draft),
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

    async fn suggest(&self, query: String) -> Result<Vec<RecipientSuggestion>, ComposeError> {
        match ask(self, Request::SuggestRecipients { query, limit: None, contacts_only: false }).await? {
            Response::RecipientSuggestions { suggestions } => Ok(suggestions),
            other => Err(unexpected(other)),
        }
    }

    async fn photo(&self, contact: String) -> Result<Option<Vec<u8>>, ComposeError> {
        use base64::Engine as _;
        match ask(self, Request::GetContactPhoto { id: contact, size: torro_contacts::PhotoSize::Small }).await {
            Ok(Response::Image { png }) => Ok(base64::engine::general_purpose::STANDARD.decode(png).ok()),
            Ok(other) => Err(unexpected(other)),
            Err(ComposeError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn writable_book(&self) -> Result<Option<String>, ComposeError> {
        match ask(self, Request::ListAddressBooks { account: None }).await? {
            Response::AddressBooks { books } => {
                Ok(books.into_iter().find(|book| book.writable && !book.hidden_by_default).map(|book| book.id))
            }
            other => Err(unexpected(other)),
        }
    }

    async fn create_contact(&self, book: String, name: String, address: String) -> Result<(), ComposeError> {
        match ask(self, Request::CreateContact { book, fields: new_contact(name, address) }).await? {
            Response::Contact { .. } => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    async fn paper(&self) -> Result<ComposePaper, ComposeError> {
        match ask(self, Request::Settings).await? {
            Response::Settings { settings } => Ok(settings.compose_paper),
            other => Err(unexpected(other)),
        }
    }

    async fn set_paper(&self, paper: ComposePaper) -> Result<(), ComposeError> {
        let value = serde_json::to_value(paper).map_err(|err| ComposeError::Other(err.to_string()))?;
        match ask(self, Request::SetSetting { key: torro_ipc::SettingKey::ComposePaper, value }).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(other)),
        }
    }
}

/// The fields of a contact "+ Contact" makes: the name the mail gave, if any, and the address.
fn new_contact(name: String, address: String) -> torro_contacts::ContactFields {
    let name = list::one_line(&name);
    torro_contacts::ContactFields {
        display_name: (!name.is_empty()).then_some(name),
        emails: Some(vec![torro_contacts::Labeled::new(address, torro_contacts::FieldKind::Other)]),
        ..torro_contacts::ContactFields::default()
    }
}

impl ComposeSource for DemoSource {
    async fn prepare_image(&self, _account: String, _source: ImageSource) -> Result<PreparedImage, ComposeError> {
        Ok(demo_image())
    }

    /// The demo takes every upload as complete: a picture pasted as pixels becomes the demo's floor plan.
    async fn upload_image(&self, _request: Request) -> Result<Response, ComposeError> {
        Ok(Response::MailImage { image: demo_image() })
    }
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

    async fn get_draft(&self, id: String) -> Result<Draft, ComposeError> {
        Ok(DemoSource::get_draft(self, &id)?)
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

    async fn suggest(&self, query: String) -> Result<Vec<RecipientSuggestion>, ComposeError> {
        Ok(demo_suggestions(&query, Utc::now()))
    }

    async fn photo(&self, contact: String) -> Result<Option<Vec<u8>>, ComposeError> {
        Ok(Some(demo_photo(&contact)))
    }

    async fn writable_book(&self) -> Result<Option<String>, ComposeError> {
        Ok(Some(torro_contacts::demo::CONTACTS.to_owned()))
    }

    async fn create_contact(&self, _book: String, name: String, address: String) -> Result<(), ComposeError> {
        demo_added().lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((name, address));
        Ok(())
    }
}

/// The contacts "+ Contact" made in the demo, until the window closes.
fn demo_added() -> &'static std::sync::Mutex<Vec<(String, String)>> {
    static ADDED: std::sync::OnceLock<std::sync::Mutex<Vec<(String, String)>>> = std::sync::OnceLock::new();
    ADDED.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// The suggestions of the demo for `query` at `now`: the sample contacts of `torro_contacts::demo`,
/// their groups and a group "Jour fixe", and a fixed history (written to Jonas Keller lately and
/// often, Emma Johansson a while ago; mail from Jo Meyer and to Johannes Brandt, in no book).
pub fn demo_suggestions(query: &str, now: DateTime<Utc>) -> Vec<RecipientSuggestion> {
    use torro_contacts::suggest::{Correspondent, GroupCandidate, rank};
    let (_, mut contacts) = torro_contacts::demo::sample(now.date_naive());
    for (i, (name, address)) in
        demo_added().lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().enumerate()
    {
        contacts.push(torro_contacts::Contact {
            id: format!("{}/demo-added-{i}", torro_contacts::demo::CONTACTS),
            display_name: name.clone(),
            emails: vec![torro_contacts::Labeled::new(address.clone(), torro_contacts::FieldKind::Other)],
            ..torro_contacts::Contact::default()
        });
    }
    let mut groups: Vec<GroupCandidate> = ["Family", "Friends", "Work"]
        .into_iter()
        .map(|name| GroupCandidate {
            name: name.to_owned(),
            members: contacts.iter().filter(|c| c.groups.iter().any(|g| g == name)).cloned().collect(),
        })
        .collect();
    groups.push(GroupCandidate { name: "Jour fixe".into(), members: contacts.iter().take(5).cloned().collect() });
    let days = |n: i64| Some(now - chrono::Duration::days(n));
    let person = |address: &str, name: Option<&str>, score: f64, sent: Option<DateTime<Utc>>, seen| Correspondent {
        address: address.into(),
        name: name.map(str::to_owned),
        score,
        last_sent: sent,
        last_received: seen,
    };
    let history = [
        person("jonas.keller@example.org", None, 6.0, days(2), days(1)),
        person("emma.johansson@example.org", None, 1.0, days(40), None),
        person("jo.meyer@posteo.de", Some("Jo Meyer"), 0.0, None, days(8)),
        person("johannes.brandt@web.de", Some("Johannes Brandt"), 0.5, days(100), None),
    ];
    rank(query, &contacts, &groups, &history, None)
}

/// The picture of a sample contact: a square in one of four quiet colours, by its id.
fn demo_photo(contact: &str) -> Vec<u8> {
    const COLOURS: [[u8; 3]; 4] = [[0x5B, 0x8C, 0xFF], [0x3F, 0xB9, 0x84], [0xE8, 0xA3, 0x3D], [0xD9, 0x6C, 0xA8]];
    let pick = contact.bytes().fold(0usize, |sum, byte| sum.wrapping_add(usize::from(byte)));
    torro_contacts::demo::solid_png(64, COLOURS[pick % COLOURS.len()])
}

fn demo_image() -> PreparedImage {
    use base64::Engine as _;
    // A small generated floor-plan fixture, independent of files and the service.
    let mut pixels = ::image::RgbaImage::from_pixel(640, 360, ::image::Rgba([244, 245, 246, 255]));
    for (x, y, p) in pixels.enumerate_pixels_mut() {
        let wall = (32..608).contains(&x) && ((32..38).contains(&y) || (322..328).contains(&y))
            || (32..328).contains(&y) && ((32..38).contains(&x) || (602..608).contains(&x))
            || (38..322).contains(&y) && (310..316).contains(&x)
            || (316..602).contains(&x) && (170..176).contains(&y);
        if wall {
            *p = ::image::Rgba([79, 83, 90, 255]);
        }
    }
    let mut png = std::io::Cursor::new(Vec::new());
    let _ = ::image::DynamicImage::ImageRgba8(pixels).write_to(&mut png, ::image::ImageFormat::Png);
    PreparedImage {
        id: "demo-image".into(),
        width: 1600,
        height: 900,
        original_width: 2400,
        original_height: 1350,
        preview: base64::engine::general_purpose::STANDARD.encode(png.into_inner()).into(),
    }
}

async fn upload_pixels<S: ComposeSource>(
    source: Arc<S>,
    account: String,
    bytes: Vec<u8>,
) -> Result<PreparedImage, ComposeError> {
    use base64::Engine as _;
    let mut upload = None;
    let chunks = bytes.chunks(torro_ipc::mail::IMAGE_CHUNK_BYTES);
    let count = chunks.len();
    for (i, chunk) in chunks.enumerate() {
        let request = Request::UploadMailImage {
            account: account.clone(),
            upload: upload.clone(),
            offset: (i * torro_ipc::mail::IMAGE_CHUNK_BYTES) as u64,
            base64: base64::engine::general_purpose::STANDARD.encode(chunk).into(),
            finish: i + 1 == count,
        };
        match source.upload_image(request).await {
            Ok(Response::MailImage { image }) => return Ok(image),
            Ok(Response::MailImageUpload { id, .. }) => upload = Some(id),
            result => {
                if let Some(id) = upload {
                    let _ = source.release_images(account, vec![id]).await;
                }
                return Err(result.err().unwrap_or(ComposeError::Other(tr("Couldn't insert the image.").into())));
            }
        }
    }
    Err(ComposeError::Other(tr("Couldn't insert the image.").into()))
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
    /// Only a draft brings blind copies (issue #502).
    pub bcc: Vec<MailAddress>,
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
        bcc: Vec::new(),
        subject: reply_subject(&summary.subject),
        body: format!("\n\n{}\n{}", attribution(message), quote(text)),
        attachments: Vec::new(),
        in_reply_to: Some(summary.id.clone()),
        history: Some(attribution(message)),
    }
}

/// What the window starts with to write on the saved `draft` (issue #502): its recipients, subject,
/// the message it answers, its text (formatted text as the marks of [`markup`], from where its
/// quoted history starts; else its plain text) and its attachments. A part of a message the window
/// knows (`mail`, the draft as `mail.get_message` reads it) takes its name and size from there,
/// any other one is called "Attachment" with no size.
pub fn reopening(draft: &Draft, mail: Option<&Mail>) -> Opening {
    let (mut body, history) = match draft.html.as_deref() {
        Some(markup) => html::to_markup(markup),
        None => (draft.body.clone(), None),
    };
    // An answer without own text yet starts with room above the quote, as a new answer does: typing
    // at once must not change the line where the history starts.
    if history.as_deref().is_some_and(|history| body.trim_start().starts_with(history)) {
        body = format!("\n\n{}", body.trim_start());
    }
    let attachments = draft
        .attachments
        .iter()
        .filter_map(|attachment| {
            Some(match attachment {
                DraftAttachment::Image { .. } => return None,
                DraftAttachment::MailPart { message, part } => {
                    let known = mail
                        .filter(|mail| mail.summary.id == *message)
                        .and_then(|mail| mail.attachments.iter().find(|info| info.part == *part));
                    match known {
                        Some(info) => Attached::part(message, info),
                        None => Attached::Part {
                            message: message.clone(),
                            part: part.clone(),
                            name: tr("Attachment").to_owned(),
                            size: 0,
                        },
                    }
                }
                DraftAttachment::File { path } => {
                    let path = PathBuf::from(path);
                    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    Attached::File { path, name, size: 0 }
                }
            })
        })
        .collect();
    Opening {
        to: draft.to.clone(),
        cc: draft.cc.clone(),
        bcc: draft.bcc.clone(),
        subject: list::one_line(&draft.subject),
        body,
        attachments,
        in_reply_to: draft.in_reply_to.clone(),
        history,
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
#[cfg(test)]
pub fn format_address(who: &MailAddress) -> String {
    match who.name.as_deref().map(list::one_line).filter(|name| !name.is_empty()) {
        None => who.address.clone(),
        Some(name) if name.contains([',', ';', '<', '>', '"']) => {
            format!("\"{}\" <{}>", name.replace('"', "'"), who.address)
        }
        Some(name) => format!("{name} <{}>", who.address),
    }
}

/// [`format_address`] for a list, separated by commas: read back by [`parse_addresses`] as the same
/// people (the fields show chips since issue #460, so only the tests write lists).
#[cfg(test)]
pub fn format_addresses(people: &[MailAddress]) -> String {
    people.iter().map(format_address).collect::<Vec<_>>().join(", ")
}

/// One of the three recipient fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    To,
    Cc,
    Bcc,
}

impl Field {
    pub const ALL: [Field; 3] = [Field::To, Field::Cc, Field::Bcc];

    /// Its name in the bridge to the page.
    pub fn name(self) -> &'static str {
        match self {
            Field::To => "to",
            Field::Cc => "cc",
            Field::Bcc => "bcc",
        }
    }

    /// The field the bridge calls `name`.
    pub fn named(name: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|field| field.name() == name)
    }
}

/// A recipient taken from a field (issue #460): a person, or a word that is no address – marked in
/// the field and named when the mail is sent, as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chip {
    Person(MailAddress),
    Broken(String),
}

/// What a recipient field holds (issue #460): the chips taken so far and what is being typed after
/// them. The draft takes the people of both ([`Recipients::addresses`]), so an autosave never loses
/// a half-typed one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recipients {
    pub chips: Vec<Chip>,
    pub typed: String,
}

impl Recipients {
    /// `people` as chips.
    pub fn of(people: &[MailAddress]) -> Recipients {
        let mut recipients = Recipients::default();
        for who in people {
            recipients.add(who.clone());
        }
        recipients
    }

    /// `text` as chips, read as [`parse_addresses`] reads it.
    pub fn parsed(text: &str) -> Recipients {
        let mut recipients = Recipients { typed: text.to_owned(), ..Recipients::default() };
        recipients.commit();
        recipients
    }

    /// Turns what was typed into chips: each address a person (once), each other word a broken
    /// chip. `false` when nothing was typed.
    pub fn commit(&mut self) -> bool {
        let typed = std::mem::take(&mut self.typed);
        if typed.trim().is_empty() {
            return false;
        }
        let (found, refused) = parse_addresses(&typed);
        for who in found {
            self.add(who);
        }
        self.chips.extend(refused.into_iter().map(Chip::Broken));
        true
    }

    /// `who` as a chip, unless the field holds the address already; what was typed goes.
    pub fn add(&mut self, who: MailAddress) -> bool {
        self.typed.clear();
        if self.holds(&who.address) {
            return false;
        }
        self.chips.push(Chip::Person(who));
        true
    }

    /// Whether a chip of the field is `address`, whatever its case.
    pub fn holds(&self, address: &str) -> bool {
        self.chips.iter().any(|chip| matches!(chip, Chip::Person(who) if who.address.eq_ignore_ascii_case(address)))
    }

    /// The people of the chips and of what is typed: what the draft holds.
    pub fn addresses(&self) -> Vec<MailAddress> {
        let mut people: Vec<MailAddress> = self
            .chips
            .iter()
            .filter_map(|chip| match chip {
                Chip::Person(who) => Some(who.clone()),
                Chip::Broken(_) => None,
            })
            .collect();
        for who in parse_addresses(&self.typed).0 {
            if !people.iter().any(|known| known.address.eq_ignore_ascii_case(&who.address)) {
                people.push(who);
            }
        }
        people
    }

    /// The words of the chips and of what is typed that are no address.
    pub fn refused(&self) -> Vec<String> {
        let broken = self.chips.iter().filter_map(|chip| match chip {
            Chip::Broken(word) => Some(word.clone()),
            Chip::Person(_) => None,
        });
        broken.chain(parse_addresses(&self.typed).1).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.chips.is_empty() && self.typed.trim().is_empty()
    }
}

// ---------------------------------------------------------------------------------------------
// What the first call brings, and notices
// ---------------------------------------------------------------------------------------------

/// What the first call brings: the accounts, and the message to answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub senders: Vec<Sender>,
    pub message: Option<Box<Mail>>,
    /// The draft to write on (`--draft`, issue #502).
    pub draft: Option<Box<Draft>>,
    pub default_sender: Option<String>,
    /// Where "+ Contact" writes ([`ComposeSource::writable_book`]); `None` hides the button.
    pub writable_book: Option<String>,
    /// The paper the person chose last (setting `compose_paper`).
    pub paper: ComposePaper,
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

/// The reason added to the notice of a send whose outcome is unknown.
fn unsure_reason() -> &'static str {
    tr("The connection broke after the message was handed over. Look into the Sent folder before sending it again.")
}

/// The key in `ui.json` under which the format bar is folded or not (issue #627).
const FORMAT_BAR: &str = "compose.format_bar";

/// The accounts, and the message to answer when there is one.
pub async fn prepare<S: ComposeSource>(source: Arc<S>, kind: Kind) -> Result<Prepared, ComposeError> {
    let senders = source.senders().await?;
    let default_sender = source.default_sender().await?;
    let mut message = match kind.message() {
        Some(id) => Some(Box::new(source.get_mail(id.to_owned()).await?)),
        None => None,
    };
    let draft = match &kind {
        Kind::Draft(id) => {
            let draft = source.get_draft(id.clone()).await?;
            // Only the names and sizes of its attachments: without them they are "Attachment".
            message = source.get_mail(id.clone()).await.ok().map(Box::new);
            Some(Box::new(draft))
        }
        _ => None,
    };
    // Neither stops the window: without them "+ Contact" is missing and the paper is light.
    let writable_book = source.writable_book().await.ok().flatten();
    let paper = source.paper().await.unwrap_or_default();
    Ok(Prepared { senders, message, draft, default_sender, writable_book, paper })
}

/// Saves `draft`; a draft that is gone (`NotFound` for the id it carries) is saved as a new one,
/// so what was written is never lost to a draft someone removed. Its own attachments (parts of the
/// gone draft, issue #502) went with it: when the new one cannot take them it is saved without
/// them, and the session says so ([`session::Session::saved`]).
pub async fn save_with_fallback<S: ComposeSource>(source: Arc<S>, draft: Draft) -> Result<Draft, ComposeError> {
    let Some(gone) = draft.id.clone() else { return source.save_draft(draft).await };
    match source.save_draft(draft.clone()).await {
        Err(ComposeError::NotFound) => {
            let fresh = Draft { id: None, ..draft };
            let own = |attachment: &DraftAttachment| matches!(attachment, DraftAttachment::MailPart { message, .. } if *message == gone);
            match source.save_draft(fresh.clone()).await {
                Err(ComposeError::NotFound) if fresh.attachments.iter().any(own) => {
                    let attachments = fresh.attachments.iter().filter(|attachment| !own(attachment)).cloned().collect();
                    source.save_draft(Draft { attachments, ..fresh }).await
                }
                other => other,
            }
        }
        other => other,
    }
}

/// A save of [`save_draft_kept`]: the draft as the core keeps it, its own parts under their new
/// numbers; `unsure` when those numbers could not be read after the save (they are still the old
/// ones then, and the next save reads them first).
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub draft: Draft,
    pub unsure: bool,
}

/// Saves `draft` ([`save_with_fallback`]) and names the parts of the draft itself anew: they are
/// attachments the core took over from the draft it replaced (`own`: the ids the window knew it by,
/// and the id it has now), whose numbers change with the new message (issue #502). The saved draft
/// is read once (`mail.get_message`) and each such part takes the number of the attachment at its
/// place; when the number of attachments differs, the parts stay as they were. When the reading
/// fails, the answer says `unsure`.
///
/// `stale` are the attachments of the last save whose numbers stayed unsure: before this save the
/// stored draft is read, and each of them still in `draft` takes the number at its place. A save
/// that cannot read them is not made (the error is the answer; the window tries again), so no part
/// is taken by a number that now names another one.
pub async fn save_draft_kept<S: ComposeSource>(
    source: Arc<S>,
    draft: Draft,
    own: Vec<String>,
    stale: Option<Vec<DraftAttachment>>,
) -> Result<Kept, ComposeError> {
    let draft = match (stale, draft.id.clone()) {
        (Some(stale), Some(id)) => {
            let mail = source.get_mail(id.clone()).await?;
            let ordinary = stale.iter().filter(|a| !matches!(a, DraftAttachment::Image { .. })).collect::<Vec<_>>();
            let parts: Vec<&AttachmentInfo> = mail
                .attachments
                .iter()
                .filter(|info| {
                    !info.inline
                        && !stale.iter().any(
                            |a| matches!(a, DraftAttachment::Image { id, .. } if info.content_id.as_ref() == Some(id)),
                        )
                })
                .collect();
            let renamed = |attachment: &DraftAttachment| {
                let at = ordinary.iter().position(|old| **old == *attachment)?;
                let own = matches!(attachment, DraftAttachment::MailPart { message, .. } if *message == id);
                let info = parts.get(at).filter(|_| own && parts.len() == ordinary.len())?;
                Some(DraftAttachment::MailPart { message: id.clone(), part: info.part.clone() })
            };
            let attachments = draft.attachments.iter().map(|old| renamed(old).unwrap_or_else(|| old.clone())).collect();
            Draft { attachments, ..draft }
        }
        _ => draft,
    };
    let saved = save_with_fallback(source.clone(), draft).await?;
    let Some(id) = saved.id.clone() else { return Ok(Kept { draft: saved, unsure: false }) };
    let names_itself = |message: &str| message == id || own.iter().any(|own| own == message);
    let keeps = saved
        .attachments
        .iter()
        .any(|attachment| matches!(attachment, DraftAttachment::MailPart { message, .. } if names_itself(message)));
    if !keeps {
        return Ok(Kept { draft: saved, unsure: false });
    }
    let Ok(mail) = source.get_mail(id.clone()).await else { return Ok(Kept { draft: saved, unsure: true }) };
    let parts: Vec<&AttachmentInfo> = mail
        .attachments
        .iter()
        .filter(|info| {
            !info.inline
                && !saved
                    .attachments
                    .iter()
                    .any(|a| matches!(a, DraftAttachment::Image { id, .. } if info.content_id.as_ref() == Some(id)))
        })
        .collect();
    let ordinary = saved.attachments.iter().filter(|a| !matches!(a, DraftAttachment::Image { .. })).collect::<Vec<_>>();
    if parts.len() != ordinary.len() {
        return Ok(Kept { draft: saved, unsure: false });
    }
    let attachments = saved
        .attachments
        .iter()
        .map(|attachment| match attachment {
            DraftAttachment::MailPart { message, .. } if names_itself(message) => {
                let at = ordinary.iter().position(|a| **a == *attachment);
                match at.and_then(|at| parts.get(at)) {
                    Some(info) => DraftAttachment::MailPart { message: id.clone(), part: info.part.clone() },
                    None => attachment.clone(),
                }
            }
            other => other.clone(),
        })
        .collect();
    Ok(Kept { draft: Draft { attachments, ..saved }, unsure: false })
}

/// Asks for files with the system's chooser; none when it is cancelled.
async fn choose_files() -> Vec<PathBuf> {
    let chosen = rfd::AsyncFileDialog::new().set_title(tr("Attach files")).pick_files().await;
    chosen.unwrap_or_default().iter().map(|file| file.path().to_path_buf()).collect()
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

#[cfg(test)]
mod tests {
    use iced::futures::executor::block_on;
    use iced::keyboard::key::Named;

    use super::*;

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
        assert_eq!(kind(&["--compose", "--draft", "m:4"]), Kind::Draft("m:4".into()), "issue #502");
        assert_eq!(Kind::Draft("m:4".into()).title(), tr("Continue writing"));
        assert_eq!(Kind::Draft("m:4".into()).message(), None, "it answers nothing by itself");
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

    #[test]
    fn every_demo_state_has_a_name_that_reads_back() {
        for (name, state) in Open::ALL {
            assert_eq!(state.name(), name);
        }
    }

    #[test]
    fn a_fresh_start_prepares_the_accounts_and_the_message_to_answer() {
        let source = Arc::new(DemoSource::new(Utc::now()));
        let prepared = block_on(prepare(source.clone(), Kind::Reply("m:w1".into()))).expect("prepared");
        assert!(!prepared.senders.is_empty());
        assert_eq!(prepared.message.as_ref().map(|mail| mail.summary.id.as_str()), Some("m:w1"));
        assert!(prepared.draft.is_none());
        let missing = block_on(prepare(source, Kind::Reply("m:none".into())));
        assert_eq!(missing, Err(ComposeError::NotFound));
    }
}
