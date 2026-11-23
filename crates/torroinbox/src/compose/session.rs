//! The state of one open compose window, without the window (issue #969): what the page reports, what is
//! saved, sent or discarded, and what the page is told back.
//!
//! The page (`assets/compose/`, Squire in the system webview) owns what is on the screen; this session owns what
//! matters: the draft. The page sends [`FromPage`] messages (JSON lines over the bridge of [`super::web`]); every
//! one is typed and bounded, anything else is dropped. The session answers with events ([`Emit`]), also JSON.
//! Nothing the page says is trusted: recipients are read again ([`Recipients`]), the editor's HTML is rewritten
//! to the one form a draft may hold ([`rewrite_images`]: `cid:` pictures the session itself prepared, nothing
//! else) and cleaned once more by the core when the draft is saved.
//!
//! The session works one message at a time (the host runs it as one task): a save or a send is awaited inside the
//! message that asked for it, so the revision cannot change while it runs and a send can never overtake the
//! save it needs. Changes that arrive meanwhile wait in the queue and count afterwards.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Value, json};
use torro_contacts::{RecipientSuggestion, SuggestionKind};
use torro_core::i18n::{tr, trf};
use torro_core::settings::ComposePaper;
use torro_ipc::mail::{Delivery, MAX_ATTACHMENTS, MAX_BODY_BYTES, MAX_FILE_BYTES, PreparedImage};
use torro_mail::{Draft, DraftAttachment, ImageSize, ImageSource, MailAddress, html, markup};

use super::{
    AUTOSAVE, Attached, ComposeError, ComposeSource, Field, Kept, Kind, Notice, Open, Prepared, Recipients, Sender,
    opening, pick_sender_with_default, prepare, reopening, save_draft_kept,
};
use crate::list;

/// How the session tells the page (and the window) something: one JSON object with the key `e`.
pub type Emit = Arc<dyn Fn(Value) + Send + Sync>;

/// How long a prepared picture is used before its lease is renewed: the core drops one after an hour without
/// use ([`PreparedImage`]).
const LEASE_RENEWAL: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// What the protocol handler of the window serves besides the page's own files: the previews of prepared pictures
/// (`/img/<cid>`) and the portraits of suggested contacts (`/photo/<key>`). Filled by the session, read by the
/// webview's thread.
#[derive(Default)]
pub struct Store {
    images: Mutex<HashMap<String, Vec<u8>>>,
    photos: Mutex<HashMap<String, Vec<u8>>>,
}

impl Store {
    /// The PNG preview of the picture with this `cid`.
    pub fn image(&self, cid: &str) -> Option<Vec<u8>> {
        self.images.lock().unwrap_or_else(PoisonError::into_inner).get(cid).cloned()
    }

    /// The PNG of the contact portrait with this key.
    pub fn photo(&self, key: &str) -> Option<Vec<u8>> {
        self.photos.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned()
    }

    pub(super) fn put_image(&self, cid: &str, png: Vec<u8>) {
        self.images.lock().unwrap_or_else(PoisonError::into_inner).insert(cid.to_owned(), png);
    }

    pub(super) fn put_photo(&self, key: &str, png: Vec<u8>) {
        self.photos.lock().unwrap_or_else(PoisonError::into_inner).insert(key.to_owned(), png);
    }
}

// ---------------------------------------------------------------------------------------------
// What the page says
// ---------------------------------------------------------------------------------------------

/// One recipient chip as the page holds it: a person, or a word that is no address (`broken`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ChipIn {
    pub address: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub broken: bool,
}

/// What a recipient field holds in the page: its chips and what is typed after them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct RecipientsIn {
    #[serde(default)]
    pub chips: Vec<ChipIn>,
    #[serde(default)]
    pub typed: String,
}

impl RecipientsIn {
    /// The same as a [`Recipients`]; an address is read again (the page's word is not enough).
    fn recipients(&self) -> Recipients {
        let mut recipients = Recipients::default();
        for chip in self.chips.iter().take(torro_ipc::mail::MAX_RECIPIENTS) {
            if chip.broken {
                recipients.chips.push(super::Chip::Broken(chip.address.clone()));
            } else {
                let name = chip.name.as_deref().map(list::one_line).filter(|name| !name.is_empty());
                match super::parse_addresses(&chip.address) {
                    (found, _) if found.len() == 1 => {
                        let address = found.into_iter().next().map(|who| who.address).unwrap_or_default();
                        recipients.add(MailAddress { name, address });
                    }
                    _ => recipients.chips.push(super::Chip::Broken(chip.address.clone())),
                }
            }
        }
        recipients.typed = self.typed.clone();
        recipients
    }
}

/// The fields of the window as the page holds them: sent after every change (a moment after the last key), and
/// once as the baseline when the page has drawn the text it was given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Form {
    #[serde(default)]
    pub to: RecipientsIn,
    #[serde(default)]
    pub cc: RecipientsIn,
    #[serde(default)]
    pub bcc: RecipientsIn,
    #[serde(default)]
    pub subject: String,
    /// The editor's HTML, pictures as `/img/<cid>`.
    #[serde(default)]
    pub html: String,
    /// The page drew what it was given: the fields are what they were, not a change.
    #[serde(default)]
    pub baseline: bool,
}

/// What the page sends to the session. Unknown messages do not parse and are dropped.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FromPage {
    /// The page is loaded: tell it what to show.
    Ready,
    Form(Form),
    /// Turn `text` of a recipient field into chips.
    Commit {
        field: String,
        text: String,
    },
    /// Suggestions for the letters `query` typed into a recipient field; `seq` names the keystroke.
    Suggest {
        field: String,
        seq: u64,
        query: String,
    },
    AddContact {
        name: String,
        address: String,
    },
    /// "Attach files": the system's chooser.
    Attach,
    /// Files that came along (dropped): attached, or embedded when they are pictures and `embed` is set.
    Files {
        paths: Vec<String>,
        #[serde(default)]
        embed: bool,
        #[serde(default)]
        token: u64,
    },
    /// "Insert image": the system's chooser.
    ImagePick {
        token: u64,
    },
    /// A picture pasted as pixels: the bytes as Base64 and a name for the alt text.
    ImageBytes {
        token: u64,
        name: String,
        base64: String,
    },
    ImageSize {
        cid: String,
        size: ImageSize,
    },
    /// An embedded picture becomes an attachment of its own (the page took it out of the text).
    ImageAttach {
        cid: String,
    },
    RemoveAttachment {
        index: usize,
    },
    /// A paste: what the clipboard offered. The answer is cleaned HTML to insert.
    Paste {
        seq: u64,
        #[serde(default)]
        html: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        plain: bool,
    },
    OpenLink {
        url: String,
    },
    PickFrom {
        index: usize,
    },
    Send,
    /// Esc or the close button.
    Close,
    KeepWriting,
    DiscardChanges,
    SaveAndClose,
    DiscardDraft,
    ConfirmDiscard,
    Retry,
    DismissNotice,
    TogglePaper,
    FormatBar {
        on: bool,
    },
    /// A script error of the page, for the log.
    Error {
        message: String,
    },
    /// The answers of the page's own checks in the real web view (`TORRO_COMPOSE_SELFTEST=1`, see
    /// `docs/mail.md`): printed one per line, then the program ends with 0 when all are fine, else 1.
    SelfTest {
        results: Vec<Value>,
    },
}

// ---------------------------------------------------------------------------------------------
// The session
// ---------------------------------------------------------------------------------------------

/// The question over the window: on closing with unsaved changes, or on "Discard draft".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Close,
    Discard,
}

impl Ask {
    fn name(self) -> &'static str {
        match self {
            Ask::Close => "close",
            Ask::Discard => "discard",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Loading,
    Failed,
    Ready,
}

/// A picture of the text, prepared by the core.
#[derive(Debug, Clone)]
struct Picture {
    cid: String,
    /// What the draft names it by; the original source while the picture is not prepared (a reopened draft whose
    /// picture could not be prepared again keeps what it was).
    source: Option<ImageSource>,
    prepared: Option<PreparedImage>,
    size: ImageSize,
    /// In the text (`true`) or an attachment of its own.
    inline: bool,
    /// It could not be prepared: sending is refused until the page takes it out.
    failed: bool,
    name: String,
    leased: Instant,
}

impl Picture {
    fn natural(&self) -> (u32, u32) {
        self.prepared.as_ref().map_or((640, 480), |image| (image.width, image.height))
    }

    /// The width the picture is shown and sent at, as the size says.
    fn width_at(&self, size: ImageSize) -> u32 {
        let natural = match (size, &self.prepared) {
            (ImageSize::Original, Some(image)) => image.original_width,
            _ => self.natural().0,
        };
        size.width(natural)
    }

    fn height_at(&self, width: u32) -> u32 {
        let (natural_width, natural_height) = match (self.size, &self.prepared) {
            (ImageSize::Original, Some(image)) => (image.original_width, image.original_height),
            _ => self.natural(),
        };
        (u64::from(natural_height) * u64::from(width) / u64::from(natural_width.max(1))).max(1) as u32
    }

    fn draft(&self) -> Option<DraftAttachment> {
        Some(DraftAttachment::Image {
            id: self.cid.clone(),
            source: self.source.clone()?,
            inline: self.inline,
            size: self.size,
        })
    }

    fn json(&self) -> Value {
        let widths: Value = [ImageSize::Small, ImageSize::Medium, ImageSize::Large, ImageSize::Original]
            .into_iter()
            .map(|size| {
                (serde_json::to_value(size).ok().and_then(|v| v.as_str().map(str::to_owned)), self.width_at(size))
            })
            .filter_map(|(name, width)| Some((name?, json!(width))))
            .collect::<serde_json::Map<_, _>>()
            .into();
        json!({
            "cid": self.cid,
            "name": self.name,
            "inline": self.inline,
            "failed": self.failed,
            "size": self.size,
            "widths": widths,
            "width": self.width_at(self.size),
        })
    }
}

/// An id for a new picture: unique in the window, valid as a Content-ID ([`html::valid_cid`]).
fn new_cid(counter: u64) -> String {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("image-{counter}-{stamp:x}@torro")
}

/// Rewrites every `<img>` of `html` with `tag` (given the picture it names and its alt text as the attribute held
/// it), and drops each one that names no picture of the window: the page's own form (`/img/<cid>`) and a draft's
/// (`cid:<cid>`) are both read. Returns the HTML and the Content-IDs it references. The text between the tags is
/// copied as it is.
fn map_images(html: &str, pictures: &[Picture], tag: impl Fn(&Picture, &str) -> String) -> (String, Vec<String>) {
    let mut out = String::with_capacity(html.len());
    let mut used = Vec::new();
    let mut rest = html;
    while let Some(at) = find_img(rest) {
        out.push_str(&rest[..at]);
        let tag_and_after = &rest[at..];
        let end = tag_and_after.find('>').map_or(tag_and_after.len(), |end| end + 1);
        let old = &tag_and_after[..end];
        rest = &tag_and_after[end..];
        let cid = attribute(old, "src").and_then(|src| {
            let id = src.strip_prefix("/img/").or_else(|| src.strip_prefix("cid:"))?;
            html::valid_cid(id).then(|| id.to_owned())
        });
        let Some(picture) = cid.as_ref().and_then(|cid| pictures.iter().find(|p| p.inline && p.cid == *cid)) else {
            continue;
        };
        // The attribute came out of the page's DOM, so quotes in it are already escaped; this one stays that way.
        let alt =
            attribute(old, "alt").unwrap_or_default().replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;");
        out.push_str(&tag(picture, &alt));
        used.push(picture.cid.clone());
    }
    out.push_str(rest);
    (out, used)
}

/// The pictures of the page's HTML as the one form a draft may hold: `<img src="cid:<cid>" alt width height>` for a
/// picture the session knows, the size saying the width, and nothing for any other `img`. Returns the HTML and the
/// Content-IDs it references.
fn rewrite_images(html: &str, pictures: &[Picture]) -> (String, Vec<String>) {
    map_images(html, pictures, |picture, alt| {
        let width = picture.width_at(picture.size);
        let height = picture.height_at(width);
        format!("<img src=\"cid:{}\" alt=\"{alt}\" width=\"{width}\" height=\"{height}\">", picture.cid)
    })
}

/// A draft's HTML as the page shows it: the pictures at the address the page loads them from, with their id (the page
/// finds the picture's size and actions by it) and the width of their size.
fn page_images(html: &str, pictures: &[Picture]) -> String {
    map_images(html, pictures, |picture, alt| {
        let width = picture.width_at(picture.size);
        format!("<img src=\"/img/{cid}\" alt=\"{alt}\" width=\"{width}\" data-cid=\"{cid}\">", cid = picture.cid)
    })
    .0
}

/// The byte index of the next `<img` tag in `html` (any case), if there is one.
fn find_img(html: &str) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut from = 0;
    while let Some(found) = html[from..].find('<') {
        let at = from + found;
        let name = bytes.get(at + 1..at + 4)?;
        let ends = bytes.get(at + 4).is_none_or(|c| c.is_ascii_whitespace() || matches!(c, b'>' | b'/'));
        if name.eq_ignore_ascii_case(b"img") && ends {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// The value of the attribute `name` in the tag `tag`, quoted with `"` or `'`.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before_ok = at > 0 && lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = tag[at + name.len()..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let value = rest[1..].trim_start();
        let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let inner = &value[1..];
        return inner.find(quote).map(|end| inner[..end].to_owned());
    }
    None
}

/// The text and pictures of the page as a draft's HTML: nothing at all when neither holds anything.
fn is_blank(html: &str) -> bool {
    !html.contains("<img") && html::to_text(html).trim().is_empty()
}

/// The compose window's state.
pub struct Session<S: ComposeSource> {
    source: Arc<S>,
    store: Arc<Store>,
    emit: Emit,
    start: super::Start,
    phase: Phase,
    senders: Vec<Sender>,
    from: usize,
    to: Recipients,
    cc: Recipients,
    bcc: Recipients,
    subject: String,
    /// The editor's HTML as the page last reported it.
    html: String,
    attachments: Vec<Attached>,
    pictures: Vec<Picture>,
    /// Pictures of an account left: released with the next save that no longer needs them.
    retired: Vec<(String, String)>,
    in_reply_to: Option<String>,
    /// The draft as the core answered the last save (or as it was opened): its id, and its account.
    draft: Option<(String, String)>,
    /// A draft of another account that goes to the trash once the new one is filed.
    discard_after: Option<String>,
    unsure_parts: Option<Vec<DraftAttachment>>,
    revision: u64,
    saved: u64,
    /// When the last change was made; `None` when everything is saved.
    changed: Option<Instant>,
    saving: bool,
    sending: bool,
    /// The outcome of a send is unknown ([`Delivery::Unknown`]); it is not sent again.
    unsure: bool,
    discarding: bool,
    asking: Option<Ask>,
    notice: Option<Notice>,
    paper: ComposePaper,
    writable_book: Option<String>,
    counter: u64,
    photo_keys: HashMap<String, String>,
    #[cfg(test)]
    pub(crate) calls: Vec<Call>,
}

/// What the session has done with the core, for the tests.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Save(Box<Draft>),
    Send(String),
    Discard(String),
    Exit,
}

impl<S: ComposeSource + 'static> Session<S> {
    pub fn new(source: Arc<S>, start: super::Start, store: Arc<Store>, emit: Emit) -> Session<S> {
        Session {
            source,
            store,
            emit,
            start,
            phase: Phase::Loading,
            senders: Vec::new(),
            from: 0,
            to: Recipients::default(),
            cc: Recipients::default(),
            bcc: Recipients::default(),
            subject: String::new(),
            html: String::new(),
            attachments: Vec::new(),
            pictures: Vec::new(),
            retired: Vec::new(),
            in_reply_to: None,
            draft: None,
            discard_after: None,
            unsure_parts: None,
            revision: 0,
            saved: 0,
            changed: None,
            saving: false,
            sending: false,
            unsure: false,
            discarding: false,
            asking: None,
            notice: None,
            paper: ComposePaper::default(),
            writable_book: None,
            counter: 0,
            photo_keys: HashMap::new(),
            #[cfg(test)]
            calls: Vec::new(),
        }
    }

    fn tell(&self, event: Value) {
        (self.emit)(event);
    }

    fn ready(&self) -> bool {
        self.phase == Phase::Ready
    }

    /// Something was changed that no save has taken yet.
    fn unsaved(&self) -> bool {
        self.revision != self.saved
    }

    fn editable(&self) -> bool {
        self.ready() && !self.sending && !self.discarding
    }

    fn sender(&self) -> Option<&Sender> {
        self.senders.get(self.from)
    }

    fn account(&self) -> String {
        self.sender().map(|sender| sender.account.clone()).unwrap_or_default()
    }

    fn touch(&mut self) {
        self.revision += 1;
        self.changed = Some(Instant::now());
    }

    /// The window answers a message (issue #627): a reply, or a draft of one.
    fn is_answer(&self) -> bool {
        match self.start.kind {
            Kind::Reply(_) | Kind::ReplyAll(_) => true,
            Kind::Draft(_) => self.in_reply_to.is_some(),
            Kind::New | Kind::Forward(_) => false,
        }
    }

    /// The title of the window: the subject, else what is being written.
    pub fn title(&self) -> String {
        let subject = list::one_line(&self.subject);
        if subject.is_empty() { self.start.kind.title().to_owned() } else { subject }
    }

    /// The title in the header (issue #627): "Reply to Lena Brandt" for an answer (the first person in To), else what
    /// is being written.
    fn heading(&self) -> String {
        let name = self.to.addresses().first().map(|who| crate::mail::sender_name(Some(who))).filter(|n| !n.is_empty());
        match name.filter(|_| self.is_answer() && self.ready()) {
            Some(name) => trf("Reply to {name}", &[("name", &name)]),
            None => self.start.kind.title().to_owned(),
        }
    }

    // ----- what the page is told -----

    fn notice_event(&self) -> Value {
        match &self.notice {
            Some(notice) => json!({ "e": "notice", "what": notice.what, "why": notice.why }),
            None => json!({ "e": "notice", "what": null }),
        }
    }

    fn set_notice(&mut self, notice: Option<Notice>) {
        self.notice = notice;
        self.tell(self.notice_event());
    }

    /// The state next to the title and what the page must lock.
    fn publish(&self) {
        let state = if self.sending {
            "sending"
        } else if self.saving {
            "saving"
        } else if self.unsaved() {
            "unsaved"
        } else if self.draft.is_some() {
            "saved"
        } else {
            "clean"
        };
        self.tell(json!({
            "e": "status",
            "state": state,
            "locked": self.sending || self.discarding,
            "asking": self.asking.map(Ask::name),
            "title": self.title(),
        }));
    }

    fn attachments_event(&self) -> Value {
        let mut cards: Vec<Value> = self
            .attachments
            .iter()
            .map(|attached| {
                json!({
                    "name": attached.name(),
                    "size": (attached.size() > 0).then(|| crate::mail::size_text(attached.size())),
                    "kind": "file",
                })
            })
            .collect();
        cards.extend(self.pictures.iter().filter(|picture| !picture.inline).map(|picture| {
            let size =
                picture.prepared.as_ref().map(|image| format!("{}×{}", image.original_width, image.original_height));
            json!({ "name": picture.name, "size": size, "kind": "image", "cid": picture.cid })
        }));
        json!({ "e": "attachments", "items": cards })
    }

    fn recipients_json(recipients: &Recipients) -> Value {
        let chips: Vec<Value> = recipients
            .chips
            .iter()
            .map(|chip| match chip {
                super::Chip::Person(who) => json!({ "address": who.address, "name": who.name, "broken": false }),
                super::Chip::Broken(word) => json!({ "address": word, "name": null, "broken": true }),
            })
            .collect();
        json!({ "chips": chips, "typed": recipients.typed })
    }

    fn senders_json(&self) -> Value {
        self.senders
            .iter()
            .map(|sender| {
                json!({
                    "account": sender.account,
                    "address": sender.address,
                    "name": sender.name,
                    "signature": sender.signature,
                    "small": trf("from {address}", &[("address", &sender.address)]),
                })
            })
            .collect()
    }

    // ----- opening -----

    /// Fetches what the window starts with and tells the page. Called once, when the page is ready.
    pub async fn load(&mut self) {
        match self.start.open {
            Some(Open::Loading) => {
                self.tell(json!({ "e": "loading" }));
                return;
            }
            Some(Open::Failed) => {
                self.phase = Phase::Failed;
                let notice = Notice::new(self.start.kind.load_failed(), ComposeError::NotFound);
                self.tell(json!({ "e": "failed", "what": notice.what, "why": notice.why }));
                return;
            }
            _ => {}
        }
        self.tell(json!({ "e": "loading" }));
        let prepared = prepare(self.source.clone(), self.start.kind.clone()).await;
        self.loaded(prepared).await;
    }

    fn fail(&mut self, notice: Notice) {
        self.phase = Phase::Failed;
        self.tell(json!({ "e": "failed", "what": notice.what, "why": notice.why }));
    }

    async fn loaded(&mut self, result: Result<Prepared, ComposeError>) {
        let prepared = match result {
            Ok(prepared) => prepared,
            Err(err) => return self.fail(Notice::new(self.start.kind.load_failed(), err)),
        };
        if prepared.senders.is_empty() {
            return self.fail(Notice::new(tr("No mail account yet."), tr("Connect one in Settings, under Accounts.")));
        }
        // A draft of an account it cannot be written from would be saved into another one and lost from its own.
        if let Some(draft) = prepared.draft.as_deref()
            && !prepared.senders.iter().any(|sender| sender.account == draft.account)
        {
            let why = tr("The draft's account can't send mail from here.");
            return self.fail(Notice::new(self.start.kind.load_failed(), why));
        }
        let own: Vec<String> = prepared.senders.iter().map(|sender| sender.address.to_lowercase()).collect();
        let opened = match prepared.draft.as_deref() {
            Some(draft) => reopening(draft, prepared.message.as_deref()),
            None => opening(&self.start.kind, prepared.message.as_deref(), &own),
        };
        let wanted = prepared.draft.as_ref().map(|draft| draft.account.clone()).or_else(|| self.start.account.clone());
        self.from = pick_sender_with_default(
            &prepared.senders,
            wanted.as_deref(),
            prepared.message.as_deref(),
            prepared.default_sender.as_deref(),
        )
        .unwrap_or(0);
        self.senders = prepared.senders;
        self.writable_book = prepared.writable_book;
        self.paper = prepared.paper;
        self.to = Recipients::of(&opened.to);
        self.cc = Recipients::of(&opened.cc);
        self.bcc = Recipients::of(&opened.bcc);
        self.subject = opened.subject;
        self.attachments = opened.attachments;
        self.in_reply_to = opened.in_reply_to;
        // The text: a draft's own HTML (its pictures come back below), a plain draft's text, or the answer's quote.
        let (html, history) = match prepared.draft.as_deref() {
            Some(draft) => match draft.html.as_deref() {
                Some(html) => (html.to_owned(), html.contains(html::QUOTE_CLASS)),
                None => (markup::to_html(&html::plain_to_markup(&draft.body), None), false),
            },
            None => (markup::to_html(&opened.body, opened.history.as_deref()), opened.history.is_some()),
        };
        self.html = html;
        if let Some(draft) = prepared.draft {
            let Draft { id, account, attachments, .. } = *draft;
            self.draft = id.map(|id| (id, account.clone()));
            self.load_pictures(&account, &attachments).await;
            self.html = page_images(&self.html, &self.pictures);
        }
        self.phase = Phase::Ready;
        // `--attach` (task WZ.8): the file goes along as if it had been chosen.
        if let Some(path) = self.start.attach.clone() {
            self.attach(&[path]);
        }
        self.apply_open();
        self.html = page_images(&self.html, &self.pictures);
        let html = self.html.clone();
        let focus = if self.to.is_empty() { "to" } else { "body" };
        let (answer, forward) = (self.is_answer(), matches!(self.start.kind, Kind::Forward(_)));
        let init = json!({
            "e": "init",
            "title": self.title(),
            "kind": match self.start.kind {
                Kind::New => "new",
                Kind::Reply(_) => "reply",
                Kind::ReplyAll(_) => "reply_all",
                Kind::Forward(_) => "forward",
                Kind::Draft(_) => "draft",
            },
            "heading": self.heading(),
            "answer": answer,
            "forward": forward,
            "senders": self.senders_json(),
            "from": self.from,
            "to": Self::recipients_json(&self.to),
            "cc": Self::recipients_json(&self.cc),
            "bcc": Self::recipients_json(&self.bcc),
            "subject": self.subject,
            "html": html,
            "history": history,
            "foldHistory": answer && history,
            "images": self.pictures.iter().map(Picture::json).collect::<Vec<_>>(),
            "paper": self.paper,
            "writableBook": self.writable_book.is_some(),
            "focus": focus,
            "open": self.start.open.map(Open::name),
            "sendKeys": super::send_keys(),
            "selfTest": std::env::var_os("TORRO_COMPOSE_SELFTEST").is_some(),
        });
        self.tell(init);
        self.tell(self.attachments_event());
        if self.notice.is_some() {
            self.tell(self.notice_event());
        }
        self.publish();
    }

    /// The pictures of a reopened draft, prepared again so the page can show them and the next save names them.
    async fn load_pictures(&mut self, account: &str, attachments: &[DraftAttachment]) {
        for attachment in attachments {
            let DraftAttachment::Image { id, source, inline, size } = attachment else { continue };
            let prepared = self.source.prepare_image(account.to_owned(), source.clone()).await;
            let mut picture = Picture {
                cid: id.clone(),
                source: Some(source.clone()),
                prepared: None,
                size: *size,
                inline: *inline,
                failed: false,
                name: tr("Image").to_owned(),
                leased: Instant::now(),
            };
            match prepared {
                Ok(image) => self.adopt(&mut picture, image),
                Err(_) => picture.failed = true,
            }
            self.pictures.push(picture);
        }
    }

    /// `image` is what `picture` is: its preview goes to the window's store, the draft names the prepared source.
    fn adopt(&self, picture: &mut Picture, image: PreparedImage) {
        if let Ok(png) = base64::engine::general_purpose::STANDARD.decode(&image.preview) {
            self.store.put_image(&picture.cid, png);
        }
        picture.source = Some(ImageSource::Prepared { id: image.id.clone() });
        picture.prepared = Some(image);
        picture.failed = false;
        picture.leased = Instant::now();
    }

    /// The forced states of `--demo --open`, once the window is ready.
    fn apply_open(&mut self) {
        let filled = |session: &mut Session<S>| {
            session.to = Recipients::parsed("Jonas Weber <jonas.weber@work.example.org>");
            session.subject = "Budget review for Q4".to_owned();
            session.html = concat!(
                "<p>Hi Jonas,</p><p>the numbers look <b>fine</b> to me, <i>except</i> the travel budget. ",
                "See <a href=\"https://intranet.work.example.org/budget/q4\">the sheet</a>.</p>",
                "<ul><li>Flights</li><li>Hotels</li></ul><ol><li>Check</li><li><u>Approve</u></li></ol>",
                "<blockquote>Could you look before Thursday?</blockquote><p>Anna</p>"
            )
            .to_owned();
        };
        match self.start.open {
            Some(Open::Cc) => {
                self.to = Recipients::parsed("Jonas Weber <jonas.weber@work.example.org>");
                self.cc = Recipients::parsed("lena.hoffmann@work.example.org, team@work.example.org");
                self.bcc = Recipients::parsed("anna@example.org");
                self.subject = "Budget review for Q4".to_owned();
                self.html =
                    "<p>Hi Jonas,</p><p>the numbers look fine to me. Lena and the team are in copy.</p><p>Anna</p>"
                        .into();
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
                self.notice =
                    Some(Notice::new(tr("The message may or may not have gone out."), super::unsure_reason()));
            }
            Some(Open::Discard) => {
                self.revision = 1;
                self.asking = Some(Ask::Close);
            }
            Some(Open::DiscardDraft) => {
                filled(self);
                self.asking = Some(Ask::Discard);
            }
            Some(Open::Format | Open::PaperDark | Open::FormatOff) => {
                filled(self);
                if self.start.open == Some(Open::PaperDark) {
                    self.paper = ComposePaper::Dark;
                }
            }
            Some(Open::Suggest) => {
                self.to = Recipients::parsed("Maria Lopez <maria.lopez@example.org>, jonas@berger");
                self.to.typed = "jo".to_owned();
                self.subject = "Floor plan for the new office".to_owned();
                self.html = "<p>Hi Maria, hi Jonas,</p><p>the floor plan is attached. <b>Please reply by Friday.</b></p><p>Anna</p>".into();
                self.writable_book.get_or_insert_with(|| torro_contacts::demo::CONTACTS.to_owned());
            }
            Some(Open::Image | Open::ImageDark | Open::ImageRemove | Open::Paste | Open::PasteDark) => {
                self.paper = if matches!(self.start.open, Some(Open::ImageDark | Open::PasteDark)) {
                    ComposePaper::Dark
                } else {
                    ComposePaper::Light
                };
                self.to = Recipients::parsed("Jonas Weber <jonas.weber@work.example.org>");
                self.subject = "Floor plan for the new office".into();
                let cid = new_cid(0);
                let mut picture = Picture {
                    cid: cid.clone(),
                    source: None,
                    prepared: None,
                    size: ImageSize::Medium,
                    inline: true,
                    failed: false,
                    name: tr("Image").to_owned(),
                    leased: Instant::now(),
                };
                self.adopt_demo(&mut picture);
                self.pictures.push(picture);
                self.html = format!(
                    "<p>Hi Jonas, here is the <b>floor plan</b>.</p><img src=\"cid:{cid}\" alt=\"Floor plan\"><p>Many thanks,<br>Anna</p>"
                );
            }
            Some(Open::Loading | Open::Failed | Open::History | Open::Subject | Open::Menu) | None => {}
        }
    }

    fn adopt_demo(&self, picture: &mut Picture) {
        self.adopt(picture, super::demo_image());
    }

    // ----- messages from the page -----

    /// One message of the page. Everything that needs the core is awaited here.
    pub async fn handle(&mut self, message: FromPage) {
        match message {
            FromPage::Ready => self.load().await,
            FromPage::Error { message } => eprintln!("torro-compose: page error: {message}"),
            FromPage::SelfTest { results } => {
                let bad = results.iter().filter(|result| result["ok"] != true).count();
                for result in &results {
                    println!("{result}");
                }
                println!(
                    "{}",
                    json!({ "selftest": if bad == 0 { "ok" } else { "deviations" }, "checks": results.len(), "deviations": bad })
                );
                std::process::exit(i32::from(bad != 0));
            }
            _ if !self.ready() => {
                if matches!(message, FromPage::Retry) {
                    self.phase = Phase::Loading;
                    self.load().await;
                } else if matches!(message, FromPage::Close | FromPage::DiscardChanges) {
                    self.exit().await;
                }
            }
            FromPage::Form(form) => self.form(form),
            FromPage::Commit { field, text } => self.commit(&field, &text),
            FromPage::Suggest { field, seq, query } => self.suggest(&field, seq, &query).await,
            FromPage::AddContact { name, address } => self.add_contact(name, address).await,
            FromPage::Attach => {
                if self.editable() {
                    let paths = super::choose_files().await;
                    self.attach(&paths);
                }
            }
            FromPage::Files { paths, embed, token } => self.files(paths, embed, token).await,
            FromPage::ImagePick { token } => {
                if self.editable()
                    && let Some(path) = choose_image().await
                {
                    self.add_image(token, Origin::Path(path)).await;
                }
            }
            FromPage::ImageBytes { token, name, base64: data } => {
                let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).unwrap_or_default();
                self.add_image(token, Origin::Bytes(name, bytes)).await;
            }
            FromPage::ImageSize { cid, size } => {
                if let Some(picture) = self.pictures.iter_mut().find(|p| p.cid == cid)
                    && picture.size != size
                {
                    picture.size = size;
                    self.touch();
                    self.publish();
                }
            }
            FromPage::ImageAttach { cid } => {
                if let Some(picture) = self.pictures.iter_mut().find(|p| p.cid == cid && p.inline) {
                    picture.inline = false;
                    self.touch();
                    self.tell(self.attachments_event());
                    self.publish();
                }
            }
            FromPage::RemoveAttachment { index } => self.remove_attachment(index),
            FromPage::Paste { seq, html, text, plain } => self.paste(seq, html.as_deref(), text.as_deref(), plain),
            FromPage::OpenLink { url } => {
                if let Some(url) = html::safe_url(&url) {
                    let _ = webbrowser::open(&url);
                }
            }
            FromPage::PickFrom { index } => self.pick_from(index).await,
            FromPage::Send => self.send().await,
            FromPage::Close => self.close().await,
            FromPage::KeepWriting => {
                self.asking = None;
                self.publish();
            }
            FromPage::DiscardChanges => self.exit().await,
            FromPage::SaveAndClose => {
                self.asking = None;
                if self.flush().await {
                    self.exit().await;
                }
            }
            FromPage::DiscardDraft => {
                if self.editable() && self.asking.is_none() {
                    self.asking = Some(Ask::Discard);
                    self.publish();
                }
            }
            FromPage::ConfirmDiscard => self.confirm_discard().await,
            FromPage::Retry => {}
            FromPage::DismissNotice => self.set_notice(None),
            FromPage::TogglePaper => self.toggle_paper().await,
            FromPage::FormatBar { on } => {
                if self.source.remembers()
                    && let Some(file) = torro_gui::ui_state::file(super::COMPOSE)
                {
                    // A view choice: when it cannot be kept the bar simply opens again next time.
                    let _ = torro_gui::ui_state::save_flag_to(&file, super::FORMAT_BAR, on);
                }
            }
        }
    }

    /// The page reports its fields. A baseline is no change.
    fn form(&mut self, form: Form) {
        if !self.editable() {
            return;
        }
        // A text far over what a draft may hold is not taken: the last one stays, and the person is told.
        if form.html.len() > MAX_BODY_BYTES.saturating_mul(2) {
            self.set_notice(Some(Notice::plain(tr("The message is too large. Shorten it before saving or sending."))));
            return;
        }
        let (to, cc, bcc) = (form.to.recipients(), form.cc.recipients(), form.bcc.recipients());
        let changed = to != self.to
            || cc != self.cc
            || bcc != self.bcc
            || list::one_line(&form.subject) != list::one_line(&self.subject)
            || form.html != self.html;
        (self.to, self.cc, self.bcc, self.subject, self.html) = (to, cc, bcc, form.subject, form.html);
        if changed && !form.baseline {
            self.touch();
        }
        self.publish();
    }

    /// `text` of a field as chips (comma, Enter, leaving the field).
    fn commit(&mut self, field: &str, text: &str) {
        let Some(field) = Field::named(field) else { return };
        let (found, refused) = super::parse_addresses(text);
        let chips: Vec<Value> = found
            .iter()
            .map(|who| json!({ "address": who.address, "name": who.name, "broken": false }))
            .chain(refused.iter().map(|word| json!({ "address": word, "name": null, "broken": true })))
            .collect();
        self.tell(json!({ "e": "chips", "field": field.name(), "chips": chips }));
    }

    async fn suggest(&mut self, field: &str, seq: u64, query: &str) {
        let Some(field) = Field::named(field) else { return };
        let query: String = query.chars().take(torro_ipc::mail::MAX_QUERY_CHARS).collect();
        let items = if query.trim().is_empty() {
            Vec::new()
        } else {
            // An error or `forbidden` shows no list; the person types on.
            self.source.suggest(query).await.unwrap_or_default()
        };
        let writable = self.writable_book.is_some();
        let json: Vec<Value> = items.iter().map(|item| suggestion_json(item, writable)).collect();
        self.tell(json!({ "e": "suggestions", "field": field.name(), "seq": seq, "items": json }));
        // The portraits come after the list: it shows at once, the faces follow.
        for item in items.iter().take(8) {
            let (Some(contact), SuggestionKind::Contact) = (&item.contact, item.kind) else { continue };
            let key = match self.photo_keys.get(contact) {
                Some(key) => key.clone(),
                None => {
                    let key = format!("p{}", self.photo_keys.len());
                    if let Ok(Some(png)) = self.source.photo(contact.clone()).await {
                        self.store.put_photo(&key, png);
                        self.photo_keys.insert(contact.clone(), key.clone());
                    }
                    key
                }
            };
            if self.store.photo(&key).is_some() {
                self.tell(json!({ "e": "photo", "contact": contact, "url": format!("/photo/{key}") }));
            }
        }
    }

    async fn add_contact(&mut self, name: String, address: String) {
        let Some(book) = self.writable_book.clone() else { return };
        let result = self.source.create_contact(book, name, address.clone()).await;
        let error = result.err().map(|err| format!("{}: {err}", tr("Couldn't add the contact.")));
        self.tell(json!({ "e": "contact_added", "address": address, "error": error }));
    }

    // ----- files, pictures, attachments -----

    /// Takes the files `paths` along, up to the number the core accepts; the first one that cannot go is told.
    fn attach(&mut self, paths: &[PathBuf]) {
        let mut problem = None;
        let mut added = false;
        let count = |session: &Session<S>| session.attachments.len() + session.pictures.len();
        for path in paths {
            if self.attachments.iter().any(|known| matches!(known, Attached::File { path: other, .. } if other == path))
            {
                continue;
            }
            if count(self) >= MAX_ATTACHMENTS {
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
            self.tell(self.attachments_event());
            self.publish();
        }
        if let Some(why) = problem {
            self.set_notice(Some(Notice::new(tr("Couldn't attach the file."), why)));
        }
    }

    /// Dropped files: pictures on the text are embedded when `embed`, everything else is attached.
    async fn files(&mut self, paths: Vec<String>, embed: bool, token: u64) {
        if !self.editable() {
            return;
        }
        let (mut pictures, mut others) = (Vec::new(), Vec::new());
        for path in paths.into_iter().take(MAX_ATTACHMENTS) {
            let path = PathBuf::from(path);
            if embed && is_picture(&path) {
                pictures.push(path);
            } else {
                others.push(path);
            }
        }
        for path in pictures {
            self.add_image(token, Origin::Path(path)).await;
        }
        if !others.is_empty() {
            self.attach(&others);
        }
    }

    /// A picture for the text: prepared by the core, then handed to the page to put where it asked.
    async fn add_image(&mut self, token: u64, origin: Origin) {
        if !self.editable() {
            return;
        }
        if self.attachments.len() + self.pictures.len() >= MAX_ATTACHMENTS {
            let why = trf("At most {n} attachments fit in one message.", &[("n", &MAX_ATTACHMENTS)]);
            return self.set_notice(Some(Notice::new(tr("Couldn't insert the image."), why)));
        }
        let account = self.account();
        let (name, result) = match origin {
            Origin::Path(path) => {
                let name = path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                let result = if super::images::local_file(&path) {
                    let source = ImageSource::File { path: path.to_string_lossy().into_owned() };
                    self.source.prepare_image(account.clone(), source).await
                } else {
                    Err(ComposeError::Other(tr("Couldn't insert the image.").into()))
                };
                (name, result)
            }
            Origin::Bytes(name, bytes) => {
                let result = if bytes.is_empty() || bytes.len() as u64 > MAX_FILE_BYTES {
                    Err(ComposeError::Other(tr("Couldn't insert the image.").into()))
                } else {
                    super::upload_pixels(self.source.clone(), account.clone(), bytes).await
                };
                (name, result)
            }
        };
        match result {
            Ok(image) => {
                self.counter += 1;
                let cid = new_cid(self.counter);
                let mut picture = Picture {
                    cid: cid.clone(),
                    source: None,
                    prepared: None,
                    size: ImageSize::default(),
                    inline: true,
                    failed: false,
                    name: if name.is_empty() { tr("Image").to_owned() } else { list::one_line(&name) },
                    leased: Instant::now(),
                };
                self.adopt(&mut picture, image);
                let json = picture.json();
                self.pictures.push(picture);
                self.touch();
                self.tell(json!({ "e": "image", "token": token, "image": json }));
                self.publish();
            }
            Err(err) => self.set_notice(Some(Notice::new(tr("Couldn't insert the image."), err))),
        }
    }

    fn remove_attachment(&mut self, index: usize) {
        if !self.editable() {
            return;
        }
        if index < self.attachments.len() {
            self.attachments.remove(index);
        } else {
            let at = index - self.attachments.len();
            let Some(cid) = self.pictures.iter().filter(|p| !p.inline).nth(at).map(|p| p.cid.clone()) else { return };
            if let Some(position) = self.pictures.iter().position(|p| p.cid == cid)
                && let Some(gone) = self.pictures.remove(position).prepared
            {
                self.retired.push((self.account(), gone.id));
            }
        }
        self.touch();
        self.tell(self.attachments_event());
        self.publish();
    }

    /// A paste: the page sends what the clipboard offered, the answer is HTML that holds only what the mail
    /// allows. `plain` is the shortcut that keeps the text only.
    fn paste(&mut self, seq: u64, html: Option<&str>, text: Option<&str>, plain: bool) {
        if !self.editable() {
            return;
        }
        let html = html.filter(|html| !plain && !html.trim().is_empty());
        let result = html::prepare_paste(html, text).map(|paste| {
            let markup = if html.is_some() { &paste.markup } else { &paste.plain_markup };
            (markup::to_html(markup, None), paste.formatted)
        });
        match result {
            Ok((html, formatted)) => {
                self.tell(json!({ "e": "paste", "seq": seq, "html": html, "formatted": formatted }))
            }
            Err(_) => {
                self.tell(json!({ "e": "paste", "seq": seq, "html": null }));
                self.set_notice(Some(Notice::plain(tr("The message is too large to paste more text."))));
            }
        }
    }

    // ----- the account -----

    /// The account to write from changes: the pictures follow it (the core checks both accounts), or nothing
    /// changes and the page is told.
    async fn pick_from(&mut self, index: usize) {
        if !self.editable() || index >= self.senders.len() || index == self.from {
            return;
        }
        let account = self.senders[index].account.clone();
        let old = self.account();
        let mut moved = Vec::new();
        for (at, picture) in self.pictures.iter().enumerate() {
            let Some(prepared) = picture.prepared.as_ref() else { continue };
            let source = ImageSource::Prepared { id: prepared.id.clone() };
            match self.source.prepare_image(account.clone(), source).await {
                Ok(image) => moved.push((at, image)),
                Err(err) => {
                    for (_, copy) in moved {
                        let _ = self.source.release_images(account.clone(), vec![copy.id]).await;
                    }
                    self.tell(json!({ "e": "from", "from": self.from }));
                    return self
                        .set_notice(Some(Notice::new(tr("Couldn't copy the images to the other account."), err)));
                }
            }
        }
        for (at, image) in moved {
            if let Some(previous) = self.pictures[at].prepared.take() {
                self.retired.push((old.clone(), previous.id));
            }
            let mut picture = self.pictures[at].clone();
            self.adopt(&mut picture, image);
            self.pictures[at] = picture;
        }
        self.from = index;
        self.touch();
        self.tell(json!({ "e": "from", "from": index }));
        self.publish();
    }

    async fn toggle_paper(&mut self) {
        let before = self.paper;
        self.paper = match before {
            ComposePaper::Light => ComposePaper::Dark,
            ComposePaper::Dark => ComposePaper::Light,
        };
        self.tell(json!({ "e": "paper", "paper": self.paper }));
        if self.source.remembers()
            && let Err(err) = self.source.set_paper(self.paper).await
        {
            // The choice could not be kept: the paper turns back.
            self.paper = before;
            self.tell(json!({ "e": "paper", "paper": before }));
            self.set_notice(Some(Notice::new(tr("Couldn't keep the paper."), err)));
        }
    }

    // ----- the draft -----

    /// The draft as the fields hold it now. Addresses that are none are left out here ([`Session::problem`] stops
    /// a send with them), so an autosave never fails on a half-typed recipient.
    pub fn draft(&self) -> Draft {
        let account = self.account();
        // A draft that lives in another account cannot be replaced by one in this one: the core would not find it.
        let id = self.draft.as_ref().filter(|(_, in_account)| *in_account == account).map(|(id, _)| id.clone());
        let (html, used) = rewrite_images(&self.html, &self.pictures);
        let html = (!is_blank(&html)).then_some(html);
        let attachments = self
            .attachments
            .iter()
            .map(Attached::draft)
            .chain(
                self.pictures
                    .iter()
                    .filter(|picture| !picture.inline || used.contains(&picture.cid))
                    .filter_map(Picture::draft),
            )
            .collect();
        Draft {
            id,
            account,
            to: self.to.addresses(),
            cc: self.cc.addresses(),
            bcc: self.bcc.addresses(),
            subject: list::one_line(&self.subject),
            body: html.as_deref().map(html::to_text).unwrap_or_default(),
            html,
            in_reply_to: self.in_reply_to.clone(),
            attachments,
        }
    }

    /// What stops a send: no recipient, words in a field that are no address, a picture that is gone.
    pub fn problem(&self) -> Option<Notice> {
        let (_, used) = rewrite_images(&self.html, &self.pictures);
        if self.pictures.iter().any(|p| p.failed && (!p.inline || used.contains(&p.cid))) {
            return Some(Notice::plain(tr("The image source is no longer available. Insert it again.")));
        }
        let refused: Vec<String> =
            [&self.to, &self.cc, &self.bcc].into_iter().flat_map(|recipients| recipients.refused()).collect();
        if let Some(first) = refused.first() {
            return Some(Notice::plain(trf("{address} isn't a valid address.", &[("address", &first)])));
        }
        let draft = self.draft();
        if draft.to.len() + draft.cc.len() + draft.bcc.len() == 0 {
            return Some(Notice::plain(tr("Add at least one recipient.")));
        }
        None
    }

    /// The clock: saves a few seconds after the last change.
    pub async fn tick(&mut self, now: Instant) {
        if !self.editable() || self.asking == Some(Ask::Discard) {
            return;
        }
        // An open window keeps its pictures alive also while nobody writes.
        self.renew_leases().await;
        if self.changed.is_some_and(|at| now.duration_since(at) >= AUTOSAVE) {
            self.flush().await;
        }
    }

    /// Saves what is not saved. `true` when everything is saved afterwards.
    async fn flush(&mut self) -> bool {
        if self.discarding {
            return false;
        }
        if !self.unsaved() && self.draft.is_some() {
            return true;
        }
        let draft = self.draft();
        if draft.html.as_deref().is_some_and(|html| html.len() > MAX_BODY_BYTES) {
            self.sending = false;
            self.set_notice(Some(Notice::plain(tr("The message is too large. Shorten it before saving or sending."))));
            self.publish();
            return false;
        }
        self.renew_leases().await;
        let draft = Draft { attachments: self.draft().attachments, ..draft };
        // The old draft of another account goes once this one is filed: its parts may go along into the new one.
        self.discard_after = self.draft.clone().filter(|(_, account)| *account != draft.account).map(|(id, _)| id);
        let own: Vec<String> = self.draft.iter().map(|(id, _)| id.clone()).collect();
        let revision = self.revision;
        let sent_parts = draft.attachments.clone();
        #[cfg(test)]
        self.calls.push(Call::Save(Box::new(draft.clone())));
        self.saving = true;
        self.publish();
        let result = save_draft_kept(self.source.clone(), draft, own, self.unsure_parts.clone()).await;
        self.saving = false;
        match result {
            Ok(Kept { draft, unsure }) => {
                self.unsure_parts = unsure.then(|| draft.attachments.clone());
                self.filed(revision, draft, &sent_parts).await;
                true
            }
            Err(err) => {
                // Everything stays as it is; the clock tries again, and a send or a close that waited is given up.
                self.discard_after = None;
                self.sending = false;
                self.changed = Some(Instant::now());
                self.set_notice(Some(Notice::new(tr("Couldn't save the draft. Your text is still here."), err)));
                self.publish();
                false
            }
        }
    }

    /// The core filed the draft: its parts under their new names, the old draft of another account in the trash,
    /// the pictures of an account left released.
    async fn filed(&mut self, revision: u64, draft: Draft, sent: &[DraftAttachment]) {
        for saved in &draft.attachments {
            if let DraftAttachment::Image { id, source, .. } = saved
                && let Some(picture) = self.pictures.iter_mut().find(|p| p.cid == *id)
                && picture.prepared.is_none()
            {
                picture.source = Some(source.clone());
            }
        }
        // Parts of a draft that was removed meanwhile could not go along (`save_with_fallback`): they leave the
        // window too, which says so.
        let lost: Vec<&DraftAttachment> = sent.iter().filter(|saved| !draft.attachments.contains(saved)).collect();
        if draft.attachments.len() < sent.len() && !lost.is_empty() {
            self.attachments.retain(|attached| !lost.contains(&&attached.draft()));
            self.tell(self.attachments_event());
            self.set_notice(Some(Notice::plain(tr("The attachments of the removed draft could not be kept."))));
        } else if sent.len() == draft.attachments.len() {
            for attached in &mut self.attachments {
                let at = sent.iter().position(|saved| *saved == attached.draft());
                if let (Some(at), Attached::Part { message, part, .. }) = (at, attached)
                    && let DraftAttachment::MailPart { message: id, part: number } = &draft.attachments[at]
                {
                    *message = id.clone();
                    *part = number.clone();
                }
            }
        }
        if let Some(id) = draft.id {
            self.draft = Some((id, draft.account));
        }
        if let Some(old) = self.discard_after.take() {
            #[cfg(test)]
            self.calls.push(Call::Discard(old.clone()));
            let _ = self.source.discard_draft(old).await;
        }
        self.saved = self.saved.max(revision);
        if revision == self.revision {
            self.changed = None;
        }
        for (account, id) in std::mem::take(&mut self.retired) {
            let _ = self.source.release_images(account, vec![id]).await;
        }
        self.publish();
    }

    /// A picture not used for a while is prepared again: the core drops one after an hour without use.
    async fn renew_leases(&mut self) {
        let account = self.account();
        let due: Vec<usize> = self
            .pictures
            .iter()
            .enumerate()
            .filter(|(_, p)| p.prepared.is_some() && p.leased.elapsed() >= LEASE_RENEWAL)
            .map(|(at, _)| at)
            .collect();
        for at in due {
            let Some(prepared) = self.pictures[at].prepared.as_ref() else { continue };
            let source = ImageSource::Prepared { id: prepared.id.clone() };
            if let Ok(image) = self.source.prepare_image(account.clone(), source).await {
                let mut picture = self.pictures[at].clone();
                self.adopt(&mut picture, image);
                self.pictures[at] = picture;
            }
        }
    }

    // ----- sending and closing -----

    /// "Send": checks the recipients, saves what is not saved and sends.
    async fn send(&mut self) {
        if !self.editable() || self.unsure || self.asking.is_some() {
            return;
        }
        if let Some(problem) = self.problem() {
            self.set_notice(Some(problem));
            return;
        }
        self.set_notice(None);
        self.sending = true;
        self.publish();
        if (self.unsaved() || self.draft.is_none()) && !self.flush().await {
            self.sending = false;
            self.publish();
            return;
        }
        let Some((id, _)) = self.draft.clone() else {
            self.sending = false;
            self.publish();
            return;
        };
        #[cfg(test)]
        self.calls.push(Call::Send(id.clone()));
        let result = self.source.send_draft(id).await;
        self.sending = false;
        match result {
            Ok(Delivery::Accepted) => return self.exit().await,
            Ok(Delivery::Unknown) => {
                self.unsure = true;
                self.set_notice(Some(Notice::new(
                    tr("The message may or may not have gone out."),
                    super::unsure_reason(),
                )));
            }
            Err(err) => {
                // A draft that is gone is saved anew with the next send; the text is still here.
                if err == ComposeError::NotFound {
                    self.draft = None;
                    self.touch();
                }
                self.set_notice(Some(Notice::new(tr("Couldn't send the message."), err)));
            }
        }
        self.publish();
    }

    /// The window is asked to close: at once when everything is saved, else after a question.
    async fn close(&mut self) {
        if self.sending || self.discarding {
            return;
        }
        // Esc closes what is open first: the page takes care of its own menus; here only the question.
        if self.asking.is_some() {
            self.asking = None;
            return self.publish();
        }
        if self.unsaved() {
            self.asking = Some(Ask::Close);
            return self.publish();
        }
        self.exit().await;
    }

    /// "Discard draft" in its question: a draft never saved just closes; a saved one goes to the trash first, and
    /// the window closes once the core took it. A failure keeps the window with a notice.
    async fn confirm_discard(&mut self) {
        if self.asking != Some(Ask::Discard) {
            return;
        }
        self.asking = None;
        self.discarding = true;
        self.publish();
        let Some((id, _)) = self.draft.clone() else { return self.exit().await };
        #[cfg(test)]
        self.calls.push(Call::Discard(id.clone()));
        match self.source.discard_draft(id).await {
            Ok(()) => self.exit().await,
            Err(err) => {
                self.discarding = false;
                self.set_notice(Some(Notice::new(tr("The draft could not be discarded."), err)));
                self.publish();
            }
        }
    }

    /// Releases the pictures and closes the window.
    async fn exit(&mut self) {
        #[cfg(test)]
        self.calls.push(Call::Exit);
        let account = self.account();
        let ids: Vec<String> = self.pictures.iter().filter_map(|p| p.prepared.as_ref().map(|p| p.id.clone())).collect();
        if !ids.is_empty() {
            let _ = self.source.release_images(account, ids).await;
        }
        for (account, id) in std::mem::take(&mut self.retired) {
            let _ = self.source.release_images(account, vec![id]).await;
        }
        self.tell(json!({ "e": "close" }));
    }
}

/// Where a picture for the text comes from.
enum Origin {
    Path(PathBuf),
    Bytes(String, Vec<u8>),
}

/// Whether `path` names a picture the text takes (by its ending; the core looks at the bytes).
fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ["png", "jpg", "jpeg", "gif", "webp"].contains(&ext.to_ascii_lowercase().as_str()))
}

/// Asks for a picture with the system's chooser; none when it is cancelled.
async fn choose_image() -> Option<PathBuf> {
    super::images::choose().await
}

/// A suggestion as the page shows it: the name and the address to show, what stands at the end, and the
/// addresses taking it adds.
fn suggestion_json(item: &RecipientSuggestion, writable: bool) -> Value {
    let day = |at: chrono::DateTime<chrono::Utc>| {
        torro_gui::datetime::format_day_month(torro_gui::datetime::to_local(at).date())
    };
    let (name, detail) = match item.kind {
        SuggestionKind::Group if item.members.len() == 1 => (item.name.clone(), tr("Group · 1 person").to_owned()),
        SuggestionKind::Group => (item.name.clone(), trf("Group · {n} people", &[("n", &item.members.len())])),
        _ if item.name.trim().is_empty() => (item.address.clone(), String::new()),
        _ => (item.name.clone(), item.address.clone()),
    };
    let trail = match item.kind {
        SuggestionKind::Group => tr("Add all").to_owned(),
        SuggestionKind::Seen => item
            .last_seen
            .max(item.last_sent)
            .map(|at| trf("Last mail {date}", &[("date", &day(at))]))
            .unwrap_or_default(),
        SuggestionKind::Contact => match (item.last_sent, item.favorite) {
            (Some(at), _) => trf("Last {date}", &[("date", &day(at))]),
            (None, true) => tr("Favorite").to_owned(),
            (None, false) => String::new(),
        },
    };
    let addresses: Vec<Value> = match item.kind {
        SuggestionKind::Group => item.members.iter().map(|m| json!({ "address": m.address, "name": m.name })).collect(),
        _ => vec![json!({ "address": item.address, "name": item.name })],
    };
    json!({
        "kind": match item.kind {
            SuggestionKind::Contact => "contact",
            SuggestionKind::Group => "group",
            SuggestionKind::Seen => "seen",
        },
        "name": list::one_line(&name),
        "detail": list::one_line(&detail),
        "trail": trail,
        "contact": item.contact,
        "addresses": addresses,
        "addable": writable && item.kind == SuggestionKind::Seen,
    })
}

#[cfg(test)]
mod tests {
    use iced::futures::executor::block_on;
    use torro_gui::source::Source;
    use torro_inbox::demo::DemoSource;

    use super::*;
    use crate::app::Launch;

    type Log = Arc<Mutex<Vec<Value>>>;

    fn blank_draft() -> Draft {
        Draft {
            id: None,
            account: String::new(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: String::new(),
            body: String::new(),
            html: None,
            in_reply_to: None,
            attachments: Vec::new(),
        }
    }

    fn who(name: &str, address: &str) -> MailAddress {
        MailAddress { name: (!name.is_empty()).then(|| name.to_owned()), address: address.to_owned() }
    }

    fn later() -> Instant {
        Instant::now() + AUTOSAVE + std::time::Duration::from_secs(1)
    }

    /// A session on `source` started with `words`, after the page said it is ready.
    fn open_on<S: ComposeSource + 'static>(source: Arc<S>, words: &[&str]) -> (Session<S>, Log) {
        let mut all = vec!["--compose".to_owned(), "--demo".to_owned()];
        all.extend(words.iter().map(|word| (*word).to_owned()));
        let launch = Launch::parse(&all).expect("valid parameters");
        let start = launch.compose.clone().expect("a compose window");
        let log = Log::default();
        let sink = log.clone();
        let emit: Emit = Arc::new(move |event| sink.lock().expect("lock").push(event));
        let mut session = Session::new(source, start, Arc::new(Store::default()), emit);
        let _ = Source::from_args(["--demo".to_owned()]);
        block_on(session.handle(FromPage::Ready));
        (session, log)
    }

    fn open(words: &[&str]) -> (Session<DemoSource>, Log) {
        open_on(Arc::new(DemoSource::new(chrono::Utc::now())), words)
    }

    fn events(log: &Log, name: &str) -> Vec<Value> {
        log.lock().expect("lock").iter().filter(|event| event["e"] == name).cloned().collect()
    }

    fn last(log: &Log, name: &str) -> Value {
        events(log, name).pop().unwrap_or_else(|| panic!("an event {name}"))
    }

    fn page(session: &mut Session<impl ComposeSource + 'static>, to: &[&str], subject: &str, html: &str) {
        let chips: Vec<ChipIn> =
            to.iter().map(|address| ChipIn { address: (*address).to_owned(), name: None, broken: false }).collect();
        let form = Form {
            to: RecipientsIn { chips, typed: String::new() },
            subject: subject.into(),
            html: html.into(),
            ..Form::default()
        };
        block_on(session.handle(FromPage::Form(form)));
    }

    fn baseline(session: &mut Session<impl ComposeSource + 'static>) {
        let form = Form {
            to: RecipientsIn {
                chips: session
                    .to
                    .addresses()
                    .into_iter()
                    .map(|who| ChipIn { address: who.address, name: who.name, broken: false })
                    .collect(),
                typed: String::new(),
            },
            subject: session.subject.clone(),
            html: session.html.clone(),
            baseline: true,
            ..Form::default()
        };
        block_on(session.handle(FromPage::Form(form)));
    }

    fn exited<S: ComposeSource>(session: &Session<S>) -> bool {
        session.calls.contains(&Call::Exit)
    }

    fn saves<S: ComposeSource>(session: &Session<S>) -> usize {
        session.calls.iter().filter(|call| matches!(call, Call::Save(_))).count()
    }

    fn sends<S: ComposeSource>(session: &Session<S>) -> usize {
        session.calls.iter().filter(|call| matches!(call, Call::Send(_))).count()
    }

    /// A demo source whose saves and sends can be made to fail.
    struct Flaky {
        inner: DemoSource,
        fail_save: Mutex<Option<ComposeError>>,
        fail_send: Mutex<Option<Result<Delivery, ComposeError>>>,
    }

    impl Flaky {
        fn new() -> Flaky {
            Flaky {
                inner: DemoSource::new(chrono::Utc::now()),
                fail_save: Mutex::new(None),
                fail_send: Mutex::new(None),
            }
        }
    }

    impl ComposeSource for Flaky {
        async fn senders(&self) -> Result<Vec<Sender>, ComposeError> {
            ComposeSource::senders(&self.inner).await
        }

        async fn get_mail(&self, id: String) -> Result<torro_mail::Message, ComposeError> {
            ComposeSource::get_mail(&self.inner, id).await
        }

        async fn get_draft(&self, id: String) -> Result<Draft, ComposeError> {
            ComposeSource::get_draft(&self.inner, id).await
        }

        async fn save_draft(&self, draft: Draft) -> Result<Draft, ComposeError> {
            if let Some(err) = self.fail_save.lock().expect("lock").clone() {
                return Err(err);
            }
            ComposeSource::save_draft(&self.inner, draft).await
        }

        async fn send_draft(&self, id: String) -> Result<Delivery, ComposeError> {
            if let Some(answer) = self.fail_send.lock().expect("lock").clone() {
                return answer;
            }
            ComposeSource::send_draft(&self.inner, id).await
        }

        async fn discard_draft(&self, id: String) -> Result<(), ComposeError> {
            ComposeSource::discard_draft(&self.inner, id).await
        }
    }

    // ----- opening -----

    #[test]
    fn a_new_message_starts_in_the_first_account_and_asks_for_a_recipient() {
        let (mut session, log) = open(&[]);
        let init = last(&log, "init");
        assert_eq!(
            (init["kind"].as_str(), init["focus"].as_str(), init["answer"].as_bool()),
            (Some("new"), Some("to"), Some(false))
        );
        assert_eq!(init["heading"], tr("New message"));
        assert_eq!(init["from"], 0);
        assert_eq!(init["senders"][0]["signature"], "Anna Berg\nProduct, Torro Workplace");
        let draft = session.draft();
        assert_eq!((draft.account.as_str(), draft.id.clone()), ("mail:anna@work.example.org", None));
        assert_eq!((draft.html.clone(), draft.body.as_str()), (None, ""), "an empty text is no HTML");
        assert_eq!(session.problem(), Some(Notice::plain(tr("Add at least one recipient."))));
        // Sending with a word that is no address says so and sends nothing.
        page(&mut session, &["nobody"], "", "");
        let problem = session.problem().expect("not an address");
        assert_eq!(problem.what, trf("{address} isn't a valid address.", &[("address", &"nobody")]));
        block_on(session.handle(FromPage::Send));
        assert_eq!(last(&log, "notice")["what"], problem.what);
        assert!(session.calls.is_empty() && !session.sending);
    }

    #[test]
    fn an_answer_opens_in_the_account_the_mail_came_to_with_its_fields_and_history_folded() {
        let (session, log) = open(&["--reply-all", "m:p1"]);
        let init = last(&log, "init");
        assert_eq!(
            (init["answer"].as_bool(), init["history"].as_bool(), init["foldHistory"].as_bool()),
            (Some(true), Some(true), Some(true))
        );
        assert_eq!(session.sender().map(|sender| sender.account.as_str()), Some("mail:anna@example.org"));
        let html = init["html"].as_str().expect("html");
        assert!(html.starts_with("<div class=\"torro-quote\">"), "{html}");
        assert!(html.contains("<blockquote type=\"cite\">"), "the quote is the history, not text to edit: {html}");
        let draft = session.draft();
        assert_eq!(draft.in_reply_to.as_deref(), Some("m:p1"));
        assert!(!draft.to.is_empty() && draft.subject.starts_with("Re: "));
        assert!(draft.body.contains("\n> "), "the quote is in the plain text: {}", draft.body);
        assert!(init["heading"].as_str().is_some_and(|heading| heading.starts_with(tr("Reply to {name}").split('{').next().unwrap_or_default())));
        let (chosen, _) = open(&["--reply", "m:p1", "--account", "anna@work.example.org"]);
        assert_eq!(chosen.sender().map(|sender| sender.account.as_str()), Some("mail:anna@work.example.org"));
        let (forward, log) = open(&["--forward", "m:w1"]);
        let init = last(&log, "init");
        assert_eq!(
            (init["answer"].as_bool(), init["foldHistory"].as_bool(), init["forward"].as_bool()),
            (Some(false), Some(false), Some(true))
        );
        assert!(forward.draft().in_reply_to.is_none(), "a forward is no reply");
    }

    #[test]
    fn the_history_is_sent_the_same_whether_it_is_folded_or_not() {
        let (mut session, _) = open(&["--reply", "m:w1"]);
        baseline(&mut session);
        let sent = session.draft();
        assert!(sent.html.as_deref().is_some_and(|html| html.contains("blockquote")), "{:?}", sent.html);
        assert!(!session.unsaved(), "the page drawing what it was given is no change");
    }

    #[test]
    fn a_failed_start_says_why_and_a_retry_opens_it() {
        let (mut session, log) = open(&["--reply", "m:w1", "--open", "failed"]);
        assert_eq!(last(&log, "failed")["what"], tr("Couldn't load the message."));
        assert!(!session.ready());
        let (loading, log) = open(&["--open", "loading"]);
        assert!(!loading.ready() && !events(&log, "loading").is_empty());
        block_on(session.handle(FromPage::Close));
        assert!(exited(&session), "a window that did not open can be closed");
    }

    // ----- saving -----

    #[test]
    fn changes_are_saved_a_few_seconds_after_the_last_one_and_the_id_replaces_the_draft() {
        let (mut session, log) = open(&[]);
        baseline(&mut session);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        assert!(session.unsaved() && session.changed.is_some());
        assert_eq!(last(&log, "status")["state"], "unsaved");
        block_on(session.tick(Instant::now()));
        assert!(session.calls.is_empty(), "too soon");
        block_on(session.tick(later()));
        assert_eq!(saves(&session), 1, "saved once the time was up");
        assert!(!session.unsaved() && session.changed.is_none());
        let states: Vec<Value> = events(&log, "status").iter().map(|event| event["state"].clone()).collect();
        assert!(states.windows(2).any(|pair| pair[0] == "saving" && pair[1] == "saved"), "{states:?}");
        let saved = session.source.drafts();
        assert_eq!(saved.len(), 1);
        assert_eq!((saved[0].to.clone(), saved[0].subject.as_str()), (vec![who("", "bob@example.org")], "Lunch"));
        assert_eq!((saved[0].html.as_deref(), saved[0].body.as_str()), (Some("<div>Shall we?</div>"), "Shall we?"));

        // The next change replaces that draft instead of adding another.
        page(&mut session, &["bob@example.org"], "Lunch tomorrow", "<div>Shall we?</div>");
        block_on(session.tick(later()));
        let Some(Call::Save(draft)) = session.calls.last().cloned() else { panic!("a second save") };
        assert_eq!(draft.id, saved[0].id, "the id the core gave replaces the draft");
        let after = session.source.drafts();
        assert_eq!((after.len(), after[0].subject.as_str()), (1, "Lunch tomorrow"));
    }

    #[test]
    fn nothing_is_saved_that_was_not_touched() {
        let (mut session, _) = open(&["--reply", "m:w1"]);
        baseline(&mut session);
        block_on(session.tick(later()));
        assert!(session.calls.is_empty() && !session.unsaved(), "an answer nobody changed is no draft");
        assert!(session.changed.is_none(), "and no clock runs");
        // The same fields again are no change either.
        baseline(&mut session);
        let again = Form {
            html: session.html.clone(),
            subject: session.subject.clone(),
            to: RecipientsIn {
                chips: session
                    .to
                    .addresses()
                    .into_iter()
                    .map(|w| ChipIn { address: w.address, name: w.name, broken: false })
                    .collect(),
                typed: String::new(),
            },
            ..Form::default()
        };
        block_on(session.handle(FromPage::Form(again)));
        assert!(!session.unsaved());
    }

    #[test]
    fn half_typed_recipients_do_not_stop_the_autosave() {
        let (mut session, _) = open(&[]);
        let form = Form {
            to: RecipientsIn {
                chips: vec![ChipIn { address: "bob@example.org".into(), ..ChipIn::default() }],
                typed: "ali".into(),
            },
            ..Form::default()
        };
        block_on(session.handle(FromPage::Form(form)));
        block_on(session.tick(later()));
        let Some(Call::Save(draft)) = session.calls.last().cloned() else { panic!("a save") };
        assert_eq!(draft.to, [who("", "bob@example.org")], "the part that is an address is saved");
        assert_eq!(session.to.typed, "ali", "what is typed stays as typed");
    }

    #[test]
    fn a_failed_save_keeps_the_text_says_so_and_tries_again() {
        let source = Arc::new(Flaky::new());
        let (mut session, log) = open_on(source.clone(), &[]);
        source.fail_save.lock().expect("lock").replace(ComposeError::Offline);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        block_on(session.tick(later()));
        let notice = last(&log, "notice");
        assert_eq!(notice["what"], tr("Couldn't save the draft. Your text is still here."));
        assert_eq!(notice["why"], ComposeError::Offline.to_string());
        assert!(session.unsaved() && session.editable(), "nothing is lost, and the fields still take text");
        assert_eq!((session.to.addresses(), session.subject.as_str()), (vec![who("", "bob@example.org")], "Lunch"));
        source.fail_save.lock().expect("lock").take();
        block_on(session.tick(later()));
        assert_eq!(saves(&session), 2, "it tries again");
        assert!(!session.unsaved());
    }

    // ----- sending -----

    #[test]
    fn sending_saves_what_is_unsaved_and_then_sends_that_draft_and_closes() {
        let (mut session, log) = open(&[]);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        block_on(session.handle(FromPage::Send));
        let id = session.draft.as_ref().map(|(id, _)| id.clone()).expect("a saved draft");
        assert!(
            matches!(session.calls.as_slice(), [Call::Save(_), Call::Send(sent), Call::Exit] if *sent == id),
            "{:?}",
            session.calls
        );
        assert_eq!(last(&log, "close")["e"], "close");
        assert_eq!(session.source.sent().len(), 1);
    }

    #[test]
    fn a_send_that_fails_leaves_the_window_the_text_and_the_draft_where_they_are() {
        let source = Arc::new(Flaky::new());
        let (mut session, log) = open_on(source.clone(), &[]);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        // The core answers with its own sentence, which says nothing was sent.
        let reason = "The outgoing mail server cannot be reached. Nothing was sent; try again later.";
        source.fail_send.lock().expect("lock").replace(Err(ComposeError::Other(reason.into())));
        block_on(session.handle(FromPage::Send));
        assert!(!exited(&session), "the window stays");
        assert!(!session.sending && session.editable(), "and can be written in again");
        let notice = last(&log, "notice");
        assert_eq!(
            (notice["what"].as_str(), notice["why"].as_str()),
            (Some(tr("Couldn't send the message.")), Some(reason))
        );
        assert_eq!(session.subject, "Lunch");
        let kept = source.inner.drafts();
        assert_eq!((kept.len(), kept[0].subject.as_str()), (1, "Lunch"), "the draft is still in the drafts folder");
        assert!(source.inner.sent().is_empty(), "nothing went out");
        // Trying again sends the same draft, without saving it a second time.
        source.fail_send.lock().expect("lock").take();
        let before = saves(&session);
        block_on(session.handle(FromPage::Send));
        assert_eq!(saves(&session), before);
        assert_eq!(sends(&session), 2);
        assert!(exited(&session));
    }

    #[test]
    fn a_send_of_unknown_outcome_stays_and_is_not_sent_again() {
        let source = Arc::new(Flaky::new());
        let (mut session, log) = open_on(source.clone(), &[]);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        source.fail_send.lock().expect("lock").replace(Ok(Delivery::Unknown));
        block_on(session.handle(FromPage::Send));
        assert!(!exited(&session) && session.unsure);
        assert_eq!(last(&log, "notice")["what"], tr("The message may or may not have gone out."));
        block_on(session.handle(FromPage::Send));
        assert_eq!(sends(&session), 1, "never twice by itself");
        // The draft is in the drafts folder, so closing loses nothing.
        block_on(session.handle(FromPage::Close));
        assert!(exited(&session));
    }

    #[test]
    fn a_draft_the_core_lost_is_saved_anew_by_the_next_send() {
        let source = Arc::new(Flaky::new());
        let (mut session, _) = open_on(source.clone(), &[]);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        source.fail_send.lock().expect("lock").replace(Err(ComposeError::NotFound));
        block_on(session.handle(FromPage::Send));
        assert!(session.draft.is_none() && session.unsaved(), "the next send saves again");
        source.fail_send.lock().expect("lock").take();
        block_on(session.handle(FromPage::Send));
        assert!(session.calls.iter().filter(|call| matches!(call, Call::Save(_))).count() >= 2);
        assert!(exited(&session));
    }

    // ----- closing -----

    #[test]
    fn closing_asks_only_when_something_is_unsaved() {
        let (mut session, _) = open(&[]);
        block_on(session.handle(FromPage::Close));
        assert!(exited(&session), "nothing written, nothing to ask");

        let (mut session, log) = open(&[]);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        block_on(session.handle(FromPage::Close));
        assert!(session.asking == Some(Ask::Close) && !exited(&session), "unsaved: asked first");
        assert_eq!(last(&log, "status")["asking"], "close");
        block_on(session.handle(FromPage::Close));
        assert!(session.asking.is_none(), "Esc in the question keeps writing");
        block_on(session.handle(FromPage::Close));
        block_on(session.handle(FromPage::KeepWriting));
        assert!(session.asking.is_none() && !exited(&session));
        block_on(session.handle(FromPage::Close));
        block_on(session.handle(FromPage::DiscardChanges));
        assert!(exited(&session) && session.source.drafts().is_empty(), "discarded: nothing was saved");
    }

    #[test]
    fn saving_from_the_question_closes_after_the_save_and_a_failed_save_keeps_the_window() {
        let source = Arc::new(Flaky::new());
        let (mut session, _) = open_on(source.clone(), &[]);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        source.fail_save.lock().expect("lock").replace(ComposeError::Offline);
        block_on(session.handle(FromPage::Close));
        block_on(session.handle(FromPage::SaveAndClose));
        assert!(!exited(&session) && session.unsaved(), "a failed save keeps the window and the text");
        source.fail_save.lock().expect("lock").take();
        block_on(session.handle(FromPage::Close));
        block_on(session.handle(FromPage::SaveAndClose));
        assert!(exited(&session));
        assert_eq!(source.inner.drafts().len(), 1);
    }

    #[test]
    fn another_account_gets_a_new_draft_and_the_old_one_goes_to_the_trash() {
        let (mut session, log) = open(&[]);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        block_on(session.tick(later()));
        let (old, account) = session.draft.clone().expect("saved");
        block_on(session.handle(FromPage::PickFrom { index: 1 }));
        assert_eq!(last(&log, "from")["from"], 1);
        block_on(session.tick(later()));
        let Some(Call::Save(draft)) = session.calls.iter().rev().find(|call| matches!(call, Call::Save(_))).cloned()
        else {
            panic!("a save")
        };
        assert_eq!(draft.id, None, "a new one, the old id belongs to {account}");
        assert!(session.calls.contains(&Call::Discard(old)), "the old one goes to the trash");
        assert_eq!(session.source.drafts().len(), 1);
    }

    // ----- attachments -----

    #[test]
    fn attachments_are_added_removed_and_end_up_in_the_draft() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let file = dir.path().join("Offer.txt");
        std::fs::write(&file, b"offer").expect("written");
        let (mut session, log) = open(&[]);
        let path = file.to_string_lossy().into_owned();
        block_on(session.handle(FromPage::Files { paths: vec![path.clone(), path.clone()], embed: false, token: 0 }));
        assert_eq!(session.attachments.len(), 1, "once");
        assert_eq!(last(&log, "attachments")["items"][0]["name"], "Offer.txt");
        assert!(session.unsaved());
        assert_eq!(session.draft().attachments, [DraftAttachment::File { path }]);
        block_on(session.handle(FromPage::RemoveAttachment { index: 0 }));
        assert!(session.draft().attachments.is_empty());
        let gone = dir.path().join("Gone.txt").to_string_lossy().into_owned();
        block_on(session.handle(FromPage::Files { paths: vec![gone], embed: false, token: 0 }));
        assert_eq!(last(&log, "notice")["what"], tr("Couldn't attach the file."));
    }

    #[test]
    fn a_file_given_at_the_start_goes_along_and_no_more_than_the_core_takes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("Offer.pdf");
        std::fs::write(&file, b"%PDF-1.7 offer").expect("written");
        let path = file.to_string_lossy().into_owned();
        let (mut session, _) = open(&["--attach", &path]);
        assert_eq!(session.attachments, [Attached::File { path: file.clone(), name: "Offer.pdf".into(), size: 14 }]);
        assert!(session.unsaved(), "saved as a draft soon");
        let paths: Vec<String> = (0..MAX_ATTACHMENTS + 3)
            .map(|n| {
                let file = dir.path().join(format!("f{n}.txt"));
                std::fs::write(&file, b"x").expect("written");
                file.to_string_lossy().into_owned()
            })
            .collect();
        block_on(session.handle(FromPage::Files { paths, embed: false, token: 0 }));
        assert_eq!(session.attachments.len(), MAX_ATTACHMENTS);
    }

    // ----- drafts -----

    #[test]
    fn a_draft_opens_with_every_field_and_saving_replaces_it() {
        let source = Arc::new(DemoSource::new(chrono::Utc::now()));
        let draft = Draft {
            id: None,
            account: "mail:anna@example.org".into(),
            to: vec![who("Bob", "bob@example.org")],
            cc: vec![who("", "cc@example.org")],
            bcc: vec![who("", "bcc@example.org")],
            subject: "Hello".into(),
            body: "Hi there".into(),
            html: Some("<p>Hi <b>there</b></p>".into()),
            in_reply_to: None,
            attachments: Vec::new(),
        };
        let saved = source.save_draft(draft).expect("saved");
        let id = saved.id.clone().expect("an id");
        let (mut session, log) = open_on(source.clone(), &["--draft", &id]);
        let init = last(&log, "init");
        assert_eq!(init["html"], "<p>Hi <b>there</b></p>", "a draft is written on as it was saved");
        assert_eq!(init["to"]["chips"][0]["name"], "Bob");
        assert_eq!(init["cc"]["chips"][0]["address"], "cc@example.org");
        assert_eq!(init["bcc"]["chips"][0]["address"], "bcc@example.org");
        assert_eq!(init["subject"], "Hello");
        assert_eq!(init["focus"], "body", "there is a recipient already");
        assert!(!session.unsaved());
        page(&mut session, &["bob@example.org"], "Hello again", "<p>Hi <b>there</b></p>");
        block_on(session.tick(later()));
        let Some(Call::Save(draft)) = session.calls.last().cloned() else { panic!("a save") };
        assert_eq!(draft.id.as_deref(), Some(id.as_str()), "saving replaces it");
        assert_eq!(source.drafts().len(), 1);
    }

    #[test]
    fn a_plain_draft_opens_as_paragraphs_without_marks() {
        let source = Arc::new(DemoSource::new(chrono::Utc::now()));
        let draft = Draft {
            id: None,
            account: "mail:anna@example.org".into(),
            to: vec![who("", "bob@example.org")],
            subject: "Plain".into(),
            body: "one *star*\n\ntwo".into(),
            ..blank_draft()
        };
        let id = source.save_draft(draft).expect("saved").id.expect("an id");
        let (_, log) = open_on(source, &["--draft", &id]);
        let html = last(&log, "init")["html"].as_str().expect("html").to_owned();
        assert!(html.contains("one *star*") && !html.contains("<em>"), "{html}");
        assert!(html.matches("<p>").count() == 2, "{html}");
    }

    // ----- pictures -----

    fn png_file(dir: &std::path::Path, name: &str) -> String {
        // Canonical: a path with a symbolic link on the way (macOS temporary folders have one) is refused.
        let file = std::fs::canonicalize(dir).expect("canonical").join(name);
        std::fs::write(&file, b"\x89PNG\r\n\x1a\nnot a real picture; the demo core does not look").expect("written");
        file.to_string_lossy().into_owned()
    }

    #[test]
    fn a_picture_is_prepared_by_the_core_shown_from_the_store_and_sent_as_cid() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (mut session, log) = open(&[]);
        block_on(session.handle(FromPage::Files {
            paths: vec![png_file(dir.path(), "plan.png")],
            embed: true,
            token: 7,
        }));
        let event = last(&log, "image");
        assert_eq!(event["token"], 7);
        let cid = event["image"]["cid"].as_str().expect("a cid").to_owned();
        assert!(html::valid_cid(&cid));
        assert_eq!(event["image"]["name"], "plan.png");
        assert!(
            session.store.image(&cid).is_some_and(|png| png.starts_with(b"\x89PNG")),
            "the preview is served to the page"
        );
        // Not in the text yet: no attachment.
        assert!(session.draft().attachments.is_empty());
        let text = format!(
            "<div>Hi</div><img src=\"/img/{cid}\" alt=\"Plan\" data-cid=\"{cid}\" onerror=\"x()\"><div>Bye</div>"
        );
        page(&mut session, &["bob@example.org"], "Plan", &text);
        let draft = session.draft();
        let html = draft.html.expect("html");
        assert!(html.contains(&format!("<img src=\"cid:{cid}\" alt=\"Plan\" width=\"640\" height=\"360\">")), "{html}");
        assert!(!html.contains("onerror") && !html.contains("/img/"), "{html}");
        assert_eq!(
            draft.attachments,
            [DraftAttachment::Image {
                id: cid.clone(),
                source: ImageSource::Prepared { id: "demo-image".into() },
                inline: true,
                size: ImageSize::Medium,
            }]
        );
        // A size changes the width the draft names.
        block_on(session.handle(FromPage::ImageSize { cid: cid.clone(), size: ImageSize::Small }));
        assert!(session.draft().html.expect("html").contains("width=\"320\" height=\"180\""));
        // Taken out of the text it is no attachment; undo in the page would bring it back with the same id.
        page(&mut session, &["bob@example.org"], "Plan", "<div>Hi</div>");
        assert!(session.draft().attachments.is_empty());
        page(&mut session, &["bob@example.org"], "Plan", &text);
        assert_eq!(session.draft().attachments.len(), 1, "the picture is still the window's");
    }

    #[test]
    fn a_picture_on_the_clipboard_comes_as_bytes_and_a_bad_one_says_so() {
        let (mut session, log) = open(&[]);
        let data = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nxx");
        block_on(session.handle(FromPage::ImageBytes { token: 1, name: "Screenshot".into(), base64: data }));
        assert_eq!(last(&log, "image")["image"]["name"], "Screenshot");
        let before = events(&log, "image").len();
        block_on(session.handle(FromPage::ImageBytes { token: 2, name: "x".into(), base64: String::new() }));
        assert_eq!(events(&log, "image").len(), before, "no bytes, no picture");
        assert_eq!(last(&log, "notice")["what"], tr("Couldn't insert the image."));
    }

    #[test]
    fn a_picture_taken_out_of_the_text_can_become_an_attachment() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (mut session, log) = open(&[]);
        block_on(session.handle(FromPage::Files {
            paths: vec![png_file(dir.path(), "plan.png")],
            embed: true,
            token: 1,
        }));
        let cid = last(&log, "image")["image"]["cid"].as_str().expect("a cid").to_owned();
        block_on(session.handle(FromPage::ImageAttach { cid: cid.clone() }));
        let items = last(&log, "attachments")["items"].clone();
        assert_eq!((items[0]["kind"].as_str(), items[0]["cid"].as_str()), (Some("image"), Some(cid.as_str())));
        assert!(matches!(session.draft().attachments.as_slice(), [DraftAttachment::Image { inline: false, .. }]));
        // Its card removes it.
        block_on(session.handle(FromPage::RemoveAttachment { index: 0 }));
        assert!(session.draft().attachments.is_empty());
        assert_eq!(session.retired.len(), 1, "released with the next save");
    }

    #[test]
    fn the_pictures_of_a_reopened_draft_are_prepared_again_and_a_lost_one_blocks_the_send() {
        let source = Arc::new(DemoSource::new(chrono::Utc::now()));
        let draft = Draft {
            id: None,
            account: "mail:anna@example.org".into(),
            to: vec![who("", "bob@example.org")],
            subject: "Plan".into(),
            html: Some("<p>See</p><img src=\"cid:old@torro\" alt=\"Plan\" width=\"640\" height=\"360\">".into()),
            body: "See".into(),
            attachments: vec![DraftAttachment::Image {
                id: "old@torro".into(),
                source: ImageSource::MailPart { message: "m:draft-1".into(), part: "2".into() },
                inline: true,
                size: ImageSize::Large,
            }],
            ..blank_draft()
        };
        let id = source.save_draft(draft).expect("saved").id.expect("an id");
        let (session, log) = open_on(source, &["--draft", &id]);
        let init = last(&log, "init");
        assert!(
            init["html"]
                .as_str()
                .is_some_and(|html| html.contains("src=\"/img/old@torro\"") && html.contains("data-cid=\"old@torro\"")),
            "{init}"
        );
        assert_eq!(init["images"][0]["size"], "large");
        assert!(session.store.image("old@torro").is_some());
        // Saved again it names the prepared source, the size kept.
        let attachments = session.draft().attachments;
        assert!(
            matches!(attachments.as_slice(), [DraftAttachment::Image { id, source: ImageSource::Prepared { .. }, size: ImageSize::Large, .. }] if id == "old@torro"),
            "{attachments:?}"
        );
        assert!(session.problem().is_none());
    }

    // ----- recipients and suggestions -----

    #[test]
    fn typed_words_become_chips_and_a_wrong_one_is_marked() {
        let (mut session, log) = open(&[]);
        block_on(session.handle(FromPage::Commit {
            field: "to".into(),
            text: "Anna <anna@example.org>, nobody; \"Doe, Jane\" <jane@example.org>".into(),
        }));
        let event = last(&log, "chips");
        assert_eq!(event["field"], "to");
        let chips = event["chips"].as_array().expect("chips");
        assert_eq!(chips.len(), 3);
        assert_eq!((chips[0]["name"].as_str(), chips[0]["address"].as_str()), (Some("Anna"), Some("anna@example.org")));
        assert_eq!(chips[1]["name"], "Doe, Jane");
        assert_eq!(
            (chips[2]["broken"].as_bool(), chips[2]["address"].as_str()),
            (Some(true), Some("nobody")),
            "words that are no address come last"
        );
        block_on(session.handle(FromPage::Commit { field: "everyone".into(), text: "x@y.org".into() }));
        assert_eq!(events(&log, "chips").len(), 1, "an unknown field is ignored");
    }

    #[test]
    fn suggestions_come_with_the_keystroke_they_are_for_and_their_addresses() {
        let (mut session, log) = open(&[]);
        block_on(session.handle(FromPage::Suggest { field: "to".into(), seq: 9, query: "jo".into() }));
        let event = last(&log, "suggestions");
        assert_eq!((event["field"].as_str(), event["seq"].as_u64()), (Some("to"), Some(9)));
        let items = event["items"].as_array().expect("items");
        assert!(!items.is_empty());
        for item in items {
            assert!(item["addresses"].as_array().is_some_and(|a| !a.is_empty()), "{item}");
            assert!(["contact", "group", "seen"].contains(&item["kind"].as_str().unwrap_or_default()));
        }
        assert!(items.iter().any(|item| item["kind"] == "group"), "groups are offered");
        block_on(session.handle(FromPage::Suggest { field: "to".into(), seq: 10, query: "  ".into() }));
        assert_eq!(last(&log, "suggestions")["items"], json!([]), "nothing typed, nothing suggested");
    }

    // ----- paste -----

    #[test]
    fn a_paste_is_cleaned_to_what_mail_allows() {
        let (mut session, log) = open(&[]);
        let rich = "<b>Bold</b> and <a href=\"https://example.org/x\">a link</a><script>steal()</script><img src=\"https://evil.example/x.png\"><ul><li>one</li></ul>";
        block_on(session.handle(FromPage::Paste {
            seq: 1,
            html: Some(rich.into()),
            text: Some("Bold and a link".into()),
            plain: false,
        }));
        let event = last(&log, "paste");
        let html = event["html"].as_str().expect("html").to_owned();
        assert!(
            html.contains("<strong>Bold</strong>") && html.contains("<a href=\"https://example.org/x\">"),
            "{html}"
        );
        assert!(!html.contains("script") && !html.contains("<img") && !html.contains("evil"), "{html}");
        assert_eq!((event["seq"].as_u64(), event["formatted"].as_bool()), (Some(1), Some(true)));
        // The shortcut that keeps the text only.
        block_on(session.handle(FromPage::Paste {
            seq: 2,
            html: Some(rich.into()),
            text: Some("*not* bold\n\nsecond".into()),
            plain: true,
        }));
        let html = last(&log, "paste")["html"].as_str().expect("html").to_owned();
        assert!(!html.contains("<strong>") && !html.contains("<em>"), "text only keeps no marks: {html}");
        assert!(html.contains("*not* bold") && html.contains("second"), "{html}");
        // Too much is refused and says so.
        let big = "x".repeat(1024 * 1024 + 1);
        block_on(session.handle(FromPage::Paste { seq: 3, html: None, text: Some(big), plain: false }));
        assert!(last(&log, "paste")["html"].is_null());
        assert_eq!(last(&log, "notice")["what"], tr("The message is too large to paste more text."));
    }

    // ----- discarding -----

    #[test]
    fn discarding_asks_and_a_draft_never_saved_just_closes() {
        let (mut session, log) = open(&[]);
        block_on(session.handle(FromPage::DiscardDraft));
        assert_eq!(last(&log, "status")["asking"], "discard");
        block_on(session.handle(FromPage::KeepWriting));
        assert!(session.asking.is_none() && !exited(&session));
        block_on(session.handle(FromPage::DiscardDraft));
        block_on(session.handle(FromPage::ConfirmDiscard));
        assert!(
            exited(&session) && !session.calls.iter().any(|call| matches!(call, Call::Discard(_))),
            "nothing to take to the trash"
        );
    }

    #[test]
    fn discarding_a_saved_draft_moves_it_to_the_trash_then_closes_and_a_failure_keeps_the_window() {
        let (mut session, _) = open(&[]);
        page(&mut session, &["bob@example.org"], "Lunch", "<div>Shall we?</div>");
        block_on(session.tick(later()));
        let (id, _) = session.draft.clone().expect("saved");
        block_on(session.handle(FromPage::DiscardDraft));
        block_on(session.handle(FromPage::ConfirmDiscard));
        assert_eq!(session.calls.last(), Some(&Call::Exit));
        assert!(session.calls.contains(&Call::Discard(id)));
        assert!(session.source.drafts().is_empty(), "the draft is gone from the drafts folder");
        // Nothing is saved while the discard runs or after it.
        block_on(session.tick(later()));
        assert_eq!(saves(&session), 1);
    }

    // ----- the rest of what the page says -----

    #[test]
    fn the_paper_turns_and_the_demo_keeps_nothing() {
        let (mut session, log) = open(&[]);
        assert!(!session.source.remembers(), "the demo never writes settings");
        block_on(session.handle(FromPage::TogglePaper));
        assert_eq!(last(&log, "paper")["paper"], "dark");
        block_on(session.handle(FromPage::TogglePaper));
        assert_eq!(last(&log, "paper")["paper"], "light");
        let (dark, log) = open(&["--open", "paper-dark"]);
        assert_eq!(last(&log, "init")["paper"], "dark");
        assert!(!dark.unsaved());
    }

    #[test]
    fn the_demo_states_fill_what_a_picture_of_them_needs() {
        let (_, log) = open(&["--open", "cc"]);
        let init = last(&log, "init");
        assert_eq!(
            (init["cc"]["chips"].as_array().map(Vec::len), init["bcc"]["chips"].as_array().map(Vec::len)),
            (Some(2), Some(1))
        );
        let (_, log) = open(&["--open", "image"]);
        let init = last(&log, "init");
        assert!(init["html"].as_str().is_some_and(|html| html.contains("<img src=\"/img/")), "{init}");
        assert_eq!(init["images"].as_array().map(Vec::len), Some(1));
        let (_, log) = open(&["--open", "discard"]);
        assert_eq!(last(&log, "status")["asking"], "close");
        let (_, log) = open(&["--open", "error"]);
        assert_eq!(last(&log, "notice")["what"], tr("Couldn't send the message."));
        let (_, log) = open(&["--open", "history", "--reply", "m:w1"]);
        assert_eq!(last(&log, "init")["open"], "history");
    }

    #[test]
    fn too_large_a_text_is_not_taken_and_not_saved() {
        let (mut session, log) = open(&[]);
        page(&mut session, &["bob@example.org"], "Big", "<div>fine</div>");
        let kept = session.html.clone();
        page(&mut session, &["bob@example.org"], "Big", &"x".repeat(2 * MAX_BODY_BYTES + 1));
        assert_eq!(session.html, kept, "the last text stays");
        assert_eq!(last(&log, "notice")["what"], tr("The message is too large. Shorten it before saving or sending."));
        page(&mut session, &["bob@example.org"], "Big", &"x".repeat(MAX_BODY_BYTES + 10));
        block_on(session.tick(later()));
        assert_eq!(saves(&session), 0, "a text over the limit is not sent to the core");
    }

    fn picture(cid: &str) -> Picture {
        Picture {
            cid: cid.into(),
            source: Some(ImageSource::Prepared { id: "p1".into() }),
            prepared: Some(PreparedImage {
                id: "p1".into(),
                width: 1600,
                height: 900,
                original_width: 2400,
                original_height: 1350,
                preview: String::new().into(),
            }),
            size: ImageSize::Medium,
            inline: true,
            failed: false,
            name: "plan.png".into(),
            leased: Instant::now(),
        }
    }

    #[test]
    fn the_pictures_of_the_page_become_the_cid_pictures_of_a_draft_and_only_those() {
        let pictures = [picture("one@torro")];
        let page = concat!(
            "<p>Hi</p><img src=\"/img/one@torro\" alt=\"Plan &amp; more\" width=\"1\" style=\"x\" onerror=\"evil()\">",
            "<img src=\"https://evil.example/x.png\"><img src=\"/img/unknown@torro\"><p>Bye</p>"
        );
        let (html, used) = rewrite_images(page, &pictures);
        assert_eq!(used, ["one@torro"]);
        assert_eq!(
            html, "<p>Hi</p><img src=\"cid:one@torro\" alt=\"Plan &amp; more\" width=\"640\" height=\"360\"><p>Bye</p>",
            "other pictures, attributes and handlers are gone; the size says the width"
        );
        let (again, _) = rewrite_images(&html, &pictures);
        assert_eq!(again, html, "a saved text reads back the same");
        let (none, used) = rewrite_images("<IMG SRC='/img/one@torro'>", &[]);
        assert_eq!((none.as_str(), used.len()), ("", 0), "without the picture it is no picture");
    }

    #[test]
    fn a_picture_is_shown_and_sent_at_the_width_its_size_says() {
        let mut p = picture("one@torro");
        assert_eq!(
            [p.width_at(ImageSize::Small), p.width_at(ImageSize::Medium), p.width_at(ImageSize::Large)],
            [320, 640, 960]
        );
        assert_eq!(p.width_at(ImageSize::Original), 2400);
        p.size = ImageSize::Original;
        assert_eq!(p.height_at(2400), 1350);
        let json = p.json();
        assert_eq!(json["widths"]["small"], 320);
        assert_eq!(json["width"], 2400);
    }

    #[test]
    fn attributes_are_read_quoted_and_whole() {
        let tag = "<img data-src=\"x\" src='/img/a@b' alt=\"A b\">";
        assert_eq!(attribute(tag, "src").as_deref(), Some("/img/a@b"), "not data-src");
        assert_eq!(attribute(tag, "alt").as_deref(), Some("A b"));
        assert_eq!(attribute(tag, "width"), None);
        assert_eq!(attribute("<img src=bare>", "src"), None, "unquoted values are not read");
    }

    #[test]
    fn blank_text_without_pictures_is_no_html() {
        assert!(is_blank("<div><br></div>"));
        assert!(is_blank(""));
        assert!(!is_blank("<p>Hi</p>"));
        assert!(!is_blank("<div><br></div><img src=\"cid:a\">"));
    }

    #[test]
    fn what_the_page_says_is_typed_and_unknown_things_do_not_parse() {
        let form: FromPage = serde_json::from_str(
            r#"{"t":"form","to":{"chips":[{"address":"a@b.org","name":"A"}],"typed":"x"},"subject":"S","html":"<p>x</p>"}"#,
        )
        .expect("a form");
        let FromPage::Form(form) = form else { panic!("a form") };
        assert_eq!((form.subject.as_str(), form.to.chips.len(), form.to.typed.as_str()), ("S", 1, "x"));
        assert!(!form.baseline);
        assert!(serde_json::from_str::<FromPage>(r#"{"t":"run","cmd":"rm -rf /"}"#).is_err());
        assert!(serde_json::from_str::<FromPage>(r#"{"t":"image_size","cid":"a","size":"huge"}"#).is_err());
        assert!(serde_json::from_str::<FromPage>(r#"{"nothing":1}"#).is_err());
    }

    #[test]
    fn a_chip_of_the_page_is_read_again_and_a_wrong_one_stays_broken() {
        let page = RecipientsIn {
            chips: vec![
                ChipIn { address: "anna@example.org".into(), name: Some("Anna".into()), broken: false },
                ChipIn { address: "not an address".into(), name: None, broken: false },
                ChipIn { address: "x@y".into(), name: None, broken: true },
                ChipIn { address: "ANNA@example.org".into(), name: None, broken: false },
            ],
            typed: "half".into(),
        };
        let read = page.recipients();
        assert_eq!(read.addresses().len(), 1, "once, whatever its case");
        assert_eq!(read.refused(), ["not an address", "x@y", "half"]);
    }

    #[test]
    fn the_cids_of_new_pictures_are_valid_content_ids() {
        for counter in [0, 1, 99] {
            assert!(html::valid_cid(&new_cid(counter)), "{}", new_cid(counter));
        }
        assert_ne!(new_cid(1), new_cid(2));
    }
}
