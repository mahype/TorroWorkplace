//! Saving attachments (task M12.3, issue #290): the save targets in `<data>/mail/save-targets.json`
//! ([`SaveTargets`]) and the handlers of `mail.list_save_targets`, `mail.create_save_target`,
//! `mail.update_save_target`, `mail.remove_save_target` and `mail.save_attachments` ([`answer`]).
//! Issue #501 adds `mail.save_message_as`: the mail itself drawn as PDF or picture
//! ([`super::export`]) and written to the same places by the same rules ([`export_plan`]).
//!
//! **Targets** are the same for every mail account and live in their own file, not in the
//! settings, so `settings.update_settings` never writes them. Only a person sets a folder (point 9
//! of "Was nie nach außen geht"): the core refuses a client's other folder before anyone is asked
//! and again when the approval runs (`Core::switches_on_for_people`, so an approval whose folder a
//! person changed meanwhile ends `forbidden`), and [`SaveTargets::update_by_client`] keeps the
//! stored one under the same lock as the write, for a change between that last check and the write.
//!
//! **Paths** come from the target's schemes ([`torro_mail::save_target::resolve`]): every value
//! from the mail is one cleaned name, folders come only from a `/` in the scheme. The folder of a
//! target must exist; the subfolders below it are made one at a time, and each is checked to be
//! below the target's folder once the system resolved it ([`target_dir`]), so a symbolic link
//! planted below never makes a folder elsewhere. A file is never overwritten except with
//! `to: file`, where the system's save dialog asked: new files are made with `create_new`, a name
//! that is taken gets a number (`Invoice (2).pdf`, up to `(99)`).
//!
//! **Reach.** The message is looked up in the stores the caller reaches only
//! ([`MailService::stores`]), as `mail.get_attachment` does: an account out of reach is
//! `not-found` before anything is read.
//!
//! **Log.** Nothing here writes a file name, a target's name, a scheme or a path anywhere; error
//! messages for the caller hold no path either (they may reach an agent).

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use torro_core::i18n::{tr, trf};
use torro_ipc::Request;
use torro_ipc::Response;
use torro_ipc::mail::{
    AttachmentPlan, ExportFormat, ExportPlan, FailedFile, PlannedPlace, SaveTo, SavedFile, limit_refusal,
};
use torro_mail::save_target::{resolve, suggestion};
use torro_mail::{
    AttachmentInfo, Category, FileFacts, FileKind, MessageSummary, OnConflict, SaveTarget, SaveTargetSpec, SchemaError,
};
use torro_mail_imap::mime::{self, MimeError};

use super::read::{self, ATTACHMENT_MAX_BYTES};
use super::service::MailService;
use crate::pim::Reach;

/// The file of the save targets in the mail data directory (`<data>/mail/`).
pub const FILE_NAME: &str = "save-targets.json";

/// The most save targets a person keeps.
pub const MAX_TARGETS: usize = 50;

/// The longest name of a target, in characters.
const MAX_NAME_CHARS: usize = 60;

/// The longest scheme (subfolders or file name), in characters.
const MAX_SCHEME_CHARS: usize = 200;

/// The most senders of "Suggest when", and the longest one in characters.
const MAX_SENDERS: usize = 20;
const MAX_SENDER_CHARS: usize = 200;

/// The longest path from outside, in bytes: the target's folder, `to: file`, `to: folder`. As
/// `torro_core::settings` takes paths (`MAX_PATH_BYTES` there).
pub const MAX_PATH_BYTES: usize = 4096;

/// "Keep both" numbers a taken name up to `Name (99).ext`.
const MAX_COPY: u32 = 99;

/// The twelve month names in the language of the app, January first, for `{month_name}`
/// (`torro-mail` knows no `tr`).
pub fn month_names() -> [String; 12] {
    [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
    .map(|month| tr(month).to_string())
}

/// The folder "Save to Downloads" uses: the setting `mail_downloads_folder` when it is an absolute
/// path (a relative one, which only a hand-edited file or an old `settings.update_settings` of the
/// app key could leave, counts as not set), else the system's Downloads folder
/// (`dirs::download_dir`), else `<home>/Downloads`. `None` without a home directory.
pub fn downloads_folder(settings: &torro_core::settings::MailAttachments) -> Option<PathBuf> {
    settings
        .downloads
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(dirs::download_dir)
        .or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
}

/// What the handlers need from the core besides the mail service: the Downloads folder
/// ([`downloads_folder`]) and the categories of the catalogue a target's condition may name.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub downloads: Option<PathBuf>,
    pub categories: Vec<Category>,
}

// ----- the targets -----

/// Why a target could not be stored or found; [`TargetError::response`] is the answer.
#[derive(Debug)]
pub enum TargetError {
    /// A value breaks a rule (`error`), with the message for people.
    Invalid(String),
    /// No target with this id (`not-found`).
    NotFound,
    /// The file could not be read or written (`error`); the details go to the log.
    Io(io::Error),
    /// The file holds something this version does not read (`error`); it is left as it is.
    Damaged,
    /// There is no data directory (no home directory), so nothing can be stored.
    NoPlace,
}

impl TargetError {
    /// The answer for the caller: `not-found` or `error`, never with a path.
    pub fn response(&self) -> Response {
        let (code, message) = match self {
            TargetError::Invalid(message) => ("error", message.clone()),
            TargetError::NotFound => ("not-found", no_target().to_string()),
            TargetError::Io(err) => {
                eprintln!("torroworkplace: mail: cannot use the save targets: {}", err.kind());
                ("error", tr("The save targets cannot be read or written.").to_string())
            }
            TargetError::Damaged => {
                eprintln!("torroworkplace: mail: the file of the save targets cannot be read; it is left as it is");
                ("error", tr("The save targets cannot be read or written.").to_string())
            }
            TargetError::NoPlace => ("error", tr("The save targets cannot be read or written.").to_string()),
        };
        Response::Error { code: code.into(), message }
    }
}

fn no_target() -> &'static str {
    tr("No such save target.")
}

/// The file as written: `{"targets": [...]}`, so later fields fit beside the list.
#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredTargets {
    #[serde(default)]
    targets: Vec<SaveTarget>,
}

/// The save targets of the suite in their file (`<data>/mail/save-targets.json`, 0600, written
/// over a temporary file and renamed). Every change reads, checks and writes under one lock.
#[derive(Debug)]
pub struct SaveTargets {
    file: Option<PathBuf>,
    lock: Mutex<()>,
}

impl SaveTargets {
    /// The targets in `file`; `None` without a data directory: then there are none and none can be
    /// added. Nothing is read or made before the first call.
    pub fn at(file: Option<PathBuf>) -> SaveTargets {
        SaveTargets { file, lock: Mutex::new(()) }
    }

    /// Every target, in the order the person keeps them. A missing file holds none.
    pub fn list(&self) -> Result<Vec<SaveTarget>, TargetError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.read()
    }

    /// The target `id`, `None` when there is none.
    pub fn get(&self, id: &str) -> Result<Option<SaveTarget>, TargetError> {
        Ok(self.list()?.into_iter().find(|target| target.id == id))
    }

    /// Adds `spec` as a new target with a random id ([`check`]; the name trimmed). `Invalid` for a
    /// spec that breaks a rule, a name another target has (regardless of case) or a 51st target.
    pub fn create(&self, spec: SaveTargetSpec, categories: &[Category]) -> Result<SaveTarget, TargetError> {
        let spec = check(spec, categories)?;
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut targets = self.read()?;
        if targets.len() >= MAX_TARGETS {
            return Err(TargetError::Invalid(tr("There can be at most 50 save targets.").into()));
        }
        unique(&targets, &spec.name, None)?;
        let id = loop {
            let id = super::random_hex(8).map_err(|_| TargetError::NoPlace)?;
            if targets.iter().all(|target| target.id != id) {
                break id;
            }
        };
        let target = SaveTarget { id, spec };
        targets.push(target.clone());
        self.write(&targets)?;
        Ok(target)
    }

    /// Replaces the target `id` with `spec` (the app key: any folder). `NotFound` for an unknown
    /// id, `Invalid` as for [`SaveTargets::create`].
    pub fn update(&self, id: &str, spec: SaveTargetSpec, categories: &[Category]) -> Result<SaveTarget, TargetError> {
        self.replace(id, spec, categories, false)
    }

    /// [`SaveTargets::update`] for a client: the folder is the stored one, read under the same lock
    /// as the write, whatever `spec` says. The core refused another folder before
    /// (`Core::switches_on_for_people`, before asking and again when the approval runs, so a folder a
    /// person changed while the approval waited ends it `forbidden`); this only covers a folder a
    /// person changes between that last check and the write.
    pub fn update_by_client(
        &self,
        id: &str,
        spec: SaveTargetSpec,
        categories: &[Category],
    ) -> Result<SaveTarget, TargetError> {
        self.replace(id, spec, categories, true)
    }

    fn replace(
        &self,
        id: &str,
        spec: SaveTargetSpec,
        categories: &[Category],
        keep_folder: bool,
    ) -> Result<SaveTarget, TargetError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut targets = self.read()?;
        let at = targets.iter().position(|target| target.id == id).ok_or(TargetError::NotFound)?;
        let mut spec = spec;
        if keep_folder {
            spec.folder = targets[at].spec.folder.clone();
        }
        let spec = check(spec, categories)?;
        unique(&targets, &spec.name, Some(id))?;
        targets[at].spec = spec;
        let target = targets[at].clone();
        self.write(&targets)?;
        Ok(target)
    }

    /// Removes the target `id` and returns the rest; files saved with it stay where they are.
    /// `NotFound` for an unknown id.
    pub fn remove(&self, id: &str) -> Result<Vec<SaveTarget>, TargetError> {
        let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut targets = self.read()?;
        let before = targets.len();
        targets.retain(|target| target.id != id);
        if targets.len() == before {
            return Err(TargetError::NotFound);
        }
        self.write(&targets)?;
        Ok(targets)
    }

    fn read(&self) -> Result<Vec<SaveTarget>, TargetError> {
        let Some(file) = &self.file else { return Ok(Vec::new()) };
        match fs::read(file) {
            Ok(bytes) => serde_json::from_slice::<StoredTargets>(&bytes)
                .map(|stored| stored.targets)
                .map_err(|_| TargetError::Damaged),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(TargetError::Io(err)),
        }
    }

    fn write(&self, targets: &[SaveTarget]) -> Result<(), TargetError> {
        let Some(file) = &self.file else { return Err(TargetError::NoPlace) };
        if let Some(dir) = file.parent() {
            crate::clients::create_private_dir(dir).map_err(TargetError::Io)?;
        }
        let bytes = serde_json::to_vec_pretty(&StoredTargets { targets: targets.to_vec() })
            .map_err(|err| TargetError::Io(io::Error::other(err)))?;
        crate::clients::write_private(file, &bytes).map_err(|err| match err {
            crate::clients::ClientsError::Io { err, .. } => TargetError::Io(err),
            _ => TargetError::NoPlace,
        })
    }
}

/// `Invalid` when another target than `own` is called `name`, regardless of case.
fn unique(targets: &[SaveTarget], name: &str, own: Option<&str>) -> Result<(), TargetError> {
    let name = name.to_lowercase();
    let taken = targets.iter().any(|target| Some(target.id.as_str()) != own && target.spec.name.to_lowercase() == name);
    match taken {
        true => Err(TargetError::Invalid(tr("Another save target has this name.").into())),
        false => Ok(()),
    }
}

/// `spec` as it may be stored, its name trimmed; `Invalid` with the reason when it breaks a rule: a
/// name of 1 to 60 characters on one line; the folder an absolute path of at most
/// [`MAX_PATH_BYTES`] on one line; each scheme at most 200 characters on one line and filled
/// without [`SchemaError`] for an example attachment; at most 20 senders of at most 200
/// characters; only categories of `categories` (the catalogue).
pub fn check(mut spec: SaveTargetSpec, categories: &[Category]) -> Result<SaveTargetSpec, TargetError> {
    let invalid = |message: &str| Err(TargetError::Invalid(message.to_string()));
    spec.name = spec.name.trim().to_string();
    let name_chars = spec.name.chars().count();
    if name_chars == 0 || name_chars > MAX_NAME_CHARS || spec.name.chars().any(char::is_control) {
        return invalid(tr("A save target needs a name of 1 to 60 characters on one line."));
    }
    if !path_fits(&spec.folder) {
        return invalid(tr("The folder of a save target must be a full path of at most 4096 bytes."));
    }
    for scheme in [&spec.subfolders, &spec.file_name] {
        if scheme.chars().count() > MAX_SCHEME_CHARS || scheme.chars().any(char::is_control) {
            return invalid(tr("A scheme may have at most 200 characters on one line."));
        }
    }
    if let Err(SchemaError::Unknown(placeholder)) = resolve(&spec, &example()) {
        let shown = format!("{{{placeholder}}}");
        return Err(TargetError::Invalid(trf(
            "The scheme has an unknown placeholder: {placeholder}",
            &[("placeholder", &shown)],
        )));
    }
    let senders = &spec.suggest.senders;
    if senders.len() > MAX_SENDERS
        || senders
            .iter()
            .any(|sender| sender.chars().count() > MAX_SENDER_CHARS || sender.chars().any(char::is_control))
    {
        return invalid(tr("A save target may name at most 20 senders of at most 200 characters each."));
    }
    if spec.suggest.categories.iter().any(|category| !categories.contains(category)) {
        return invalid(tr("A category of the save target is not in the catalogue."));
    }
    Ok(spec)
}

/// Whether `path` can be a path from outside: absolute, one line, at most [`MAX_PATH_BYTES`].
fn path_fits(path: &str) -> bool {
    path.len() <= MAX_PATH_BYTES && !path.chars().any(char::is_control) && Path::new(path).is_absolute()
}

/// The attachment a scheme is tried with before it is stored: the issue's example.
fn example() -> FileFacts {
    FileFacts {
        mail_date: NaiveDate::from_ymd_opt(2026, 3, 28).unwrap_or_default(),
        file_name: Some("Rechnung_2026-03-28_4711.pdf".into()),
        content_type: "application/pdf".into(),
        sender_name: Some("Stadtwerke Bonn".into()),
        sender_address: "billing@stadtwerke-bonn.de".into(),
        account: "Privat".into(),
        subject: "Ihre Rechnung März 2026".into(),
        month_names: month_names(),
    }
}

// ----- the requests -----

/// A failed request, already as the answer to send.
type Answer<T> = Result<T, Box<Response>>;

/// Answers a save target or saving request for `reach` (the five requests of issue #290); any
/// other request is `unsupported`. The limits are checked first, also for the app key. Rights and
/// what only people may (`to: file | folder`, another folder of a target) were checked before
/// (`Core::handle_client`); `mail.update_save_target` from a client keeps the stored folder
/// ([`SaveTargets::update_by_client`]). Setting the click back after `mail.remove_save_target` is
/// the core's (it holds the settings).
pub async fn answer(mail: &MailService, reach: Reach<'_>, context: &Context, request: Request) -> Response {
    if let Some(refused) = limit_refusal(&request) {
        return refused;
    }
    let targets = mail.save_targets();
    let downloads = || context.downloads.as_deref().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default();
    let answered = match request {
        Request::ListSaveTargets { id, export_only } => list(mail, reach, id.as_deref(), export_only)
            .await
            .map(|(targets, files, exports)| Response::SaveTargets { targets, downloads: downloads(), files, exports }),
        Request::CreateSaveTarget { target } => {
            targets.create(target, &context.categories).map(|target| Response::SaveTarget { target }).map_err(boxed)
        }
        Request::UpdateSaveTarget { id, target } => match reach {
            Reach::Suite => targets.update(&id, target, &context.categories),
            Reach::Client(_) => targets.update_by_client(&id, target, &context.categories),
        }
        .map(|target| Response::SaveTarget { target })
        .map_err(boxed),
        Request::RemoveSaveTarget { id } => targets
            .remove(&id)
            .map(|targets| Response::SaveTargets {
                targets,
                downloads: downloads(),
                files: Vec::new(),
                exports: Vec::new(),
            })
            .map_err(boxed),
        Request::SaveMailAttachments { id, parts, to } => save(mail, reach, context, &id, &parts, &to).await,
        Request::SaveMailAs { id, format, to } => save_message(mail, reach, context, &id, format, &to, None).await,
        other => {
            let message = format!("{} is not a command for saving attachments", other.operation().name());
            Err(Box::new(Response::Error { code: "unsupported".into(), message }))
        }
    };
    answered.unwrap_or_else(|refused| *refused)
}

fn boxed(err: TargetError) -> Box<Response> {
    Box::new(err.response())
}

/// What the core checks of a client's save target request before anyone is asked to approve it
/// (`Core::handle_client`): an unknown target is `not-found` (`mail.update_save_target`,
/// `mail.remove_save_target`, `mail.save_attachments` and `mail.save_message_as` to a target), and a target that could not
/// be stored is `error` ([`check`], with the stored folder). `None` when it may go on, and for
/// every other request.
pub fn client_refusal(mail: &MailService, request: &Request, categories: &[Category]) -> Option<Response> {
    let id = match request {
        Request::UpdateSaveTarget { id, .. }
        | Request::RemoveSaveTarget { id }
        | Request::SaveMailAttachments { to: SaveTo::Target(id), .. }
        | Request::SaveMailAs { to: SaveTo::Target(id), .. } => id,
        _ => return None,
    };
    let stored = match mail.save_targets().get(id) {
        Ok(Some(stored)) => stored,
        Ok(None) => return Some(TargetError::NotFound.response()),
        Err(err) => return Some(err.response()),
    };
    if let Request::UpdateSaveTarget { target, .. } = request {
        let spec = SaveTargetSpec { folder: stored.spec.folder, ..target.clone() };
        return check(spec, categories).err().map(|err| err.response());
    }
    None
}

/// What the approval of `mail.save_attachments` shows ([`to_approve`]), looked up in the core's
/// store: never from the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToApprove {
    /// The message's account (`mail:<address>`).
    pub account: String,
    /// Its subject as the store holds it.
    pub subject: String,
    /// How many files the call saves: the parts named, or with none named the attachments that are
    /// not `inline` ([`known_attachments`]); 0 when they could not be read (the message could not
    /// be fetched).
    pub count: usize,
}

/// For the approval of `mail.save_attachments` of the message `id` with `parts`: the message as the
/// stores `reach` sees hold it ([`ToApprove`]), or the answer when it is out of reach or gone
/// (`not-found`), so nobody is asked to approve what would be refused anyway.
pub async fn to_approve(mail: &MailService, reach: Reach<'_>, id: &str, parts: &[String]) -> Answer<ToApprove> {
    let stores = mail.stores(reach).await;
    let located = read::locate(&stores, id).await?;
    let message = read::stored_message(&located).await?;
    let known = known_attachments(mail, &located, &message).await;
    // A part named twice is one file; once the attachments are known, a part the mail does not have
    // is refused here, so nobody approves a call that would end in `not-found`.
    let mut named: Vec<&String> = parts.iter().collect();
    named.sort();
    named.dedup();
    if let Some(known) = &known
        && !named.iter().all(|part| known.iter().any(|attachment| &attachment.part == *part))
    {
        return Err(read::no_attachment());
    }
    let count = match (named.len(), &known) {
        (0, Some(known)) => known.iter().filter(|attachment| !attachment.inline).count(),
        (named, Some(_)) => named,
        // Not known: no number from the request in front of a person, the summary says "the
        // attachments".
        (_, None) => 0,
    };
    Ok(ToApprove { account: located.account, subject: message.summary.subject, count })
}

/// For the approval of `mail.save_message_as` of the message `id` (issue #501): its account and
/// subject as the stores `reach` sees hold them, without fetching anything, `count` 1; the answer when
/// it is out of reach or gone (`not-found`).
pub async fn message_to_approve(mail: &MailService, reach: Reach<'_>, id: &str) -> Answer<ToApprove> {
    let stores = mail.stores(reach).await;
    let located = read::locate(&stores, id).await?;
    let summary = summary_of(&located).await?;
    Ok(ToApprove { account: located.account, subject: summary.subject, count: 1 })
}

/// Export needs only headers; a damaged disposable reading body must not hide the original.
async fn summary_of(located: &read::Located) -> Answer<MessageSummary> {
    let id = located.place.id.clone();
    located
        .store
        .run(move |store| store.summary(&id))
        .await
        .map_err(|err| Box::new(super::service::MailError::Store(err).response()))?
        .ok_or_else(|| failed("not-found", tr("No such message.")))
}

/// The attachments of the stored `message`, `inline` ones included: the store's once it has read the
/// message's text, else those of the message fetched once ([`read::fetch_raw`]). Not only a mail
/// whose text is on its way lacks it: the store drops the text again (`prune_bodies_for` after
/// reading, at once with headers only and else for mail older than `BODY_RETENTION_DAYS`), so an
/// older mail never has it. `None` when the fetch fails (offline, gone from the server,
/// unreadable): the caller goes on without them.
async fn known_attachments(
    mail: &MailService,
    located: &read::Located,
    message: &torro_mail::Message,
) -> Option<Vec<AttachmentInfo>> {
    if message.text.is_some() {
        return Some(message.attachments.clone());
    }
    // The server's BODYSTRUCTURE said there are none: nothing worth fetching the whole mail for.
    if !message.summary.has_attachments {
        return Some(Vec::new());
    }
    let raw = read::fetch_raw(mail, located).await.ok()?;
    Some(mime::read(&raw).ok()?.attachments)
}

// ----- mail.list_save_targets -----

/// The targets and, for `id`, the plan of each attachment of the message that is not `inline`
/// ([`plan`], the attachments from [`known_attachments`]) and of the mail itself as PDF and picture
/// ([`export_plan`], issue #501); no attachment plans when they cannot be read, the targets and the
/// mail's plans all the same. `export_only` instead plans every export format from the summary,
/// with no attachment plans or body access (issue #553).
async fn list(
    mail: &MailService,
    reach: Reach<'_>,
    id: Option<&str>,
    export_only: bool,
) -> Answer<(Vec<SaveTarget>, Vec<AttachmentPlan>, Vec<ExportPlan>)> {
    let targets = mail.save_targets().list().map_err(boxed)?;
    let Some(id) = id else { return Ok((targets, Vec::new(), Vec::new())) };
    let stores = mail.stores(reach).await;
    let located = read::locate(&stores, id).await?;
    let summary = summary_of(&located).await?;
    let account = account_name(mail, &located.account).await;
    let months = month_names();
    let exports = ExportFormat::ALL
        .iter()
        .filter(|&&format| export_only || format != ExportFormat::Eml)
        .map(|&format| export_plan(&targets, &summary, format, &account, &months))
        .collect();
    if export_only {
        return Ok((targets, Vec::new(), exports));
    }
    let message = read::stored_message(&located).await?;
    let Some(attachments) = known_attachments(mail, &located, &message).await else {
        return Ok((targets, Vec::new(), exports));
    };
    let files = attachments
        .iter()
        .filter(|attachment| !attachment.inline)
        .map(|attachment| plan(&targets, &message.summary, attachment, &account, &months))
        .collect();
    Ok((targets, files, exports))
}

/// The account's name for `{account}`: its display name, else its address.
async fn account_name(mail: &MailService, account: &str) -> String {
    match mail.account_name(account).await {
        Some(name) => name,
        None => account.strip_prefix(torro_mail::ACCOUNT_PREFIX).unwrap_or(account).to_string(),
    }
}

/// Where `attachment` of the mail `summary` would go in each of `targets`, and which of them are
/// suggested for it (`suggested`, in the order of the targets) and which one a click on "Suggested
/// target, else open" saves to (`sure`: the first suggested one whose conditions hold without a
/// category the model only guessed). A target whose scheme cannot be filled has no place.
pub fn plan(
    targets: &[SaveTarget],
    summary: &MessageSummary,
    attachment: &AttachmentInfo,
    account: &str,
    months: &[String; 12],
) -> AttachmentPlan {
    let facts = facts_of(summary, attachment.filename.as_deref(), &attachment.content_type, account, months);
    let kind = FileKind::of(&attachment.content_type, attachment.filename.as_deref());
    let mut plan = AttachmentPlan {
        part: attachment.part.clone(),
        file_name: downloads_name(attachment.filename.as_deref()),
        places: Vec::new(),
        suggested: Vec::new(),
        sure: None,
    };
    for target in targets {
        if let Ok(resolved) = resolve(&target.spec, &facts) {
            plan.places.push(PlannedPlace {
                target: target.id.clone(),
                folders: resolved.folders,
                file: resolved.file,
            });
        }
        if let Some(found) = suggestion(&target.spec.suggest, summary.category, kind, &facts.sender_address) {
            plan.suggested.push(target.id.clone());
            if found.sure && plan.sure.is_none() {
                plan.sure = Some(target.id.clone());
            }
        }
    }
    plan
}

/// The facts a scheme is filled with for one attachment of the mail `summary`: the mail's day on
/// this computer's clock (not UTC), its sender, subject and account, the attachment's name and type.
fn facts_of(
    summary: &MessageSummary,
    file_name: Option<&str>,
    content_type: &str,
    account: &str,
    months: &[String; 12],
) -> FileFacts {
    FileFacts {
        mail_date: summary.date.with_timezone(&Local).date_naive(),
        file_name: file_name.map(str::to_string),
        content_type: content_type.to_string(),
        sender_name: summary.from.as_ref().and_then(|from| from.name.clone()),
        sender_address: summary.from.as_ref().map(|from| from.address.clone()).unwrap_or_default(),
        account: account.to_string(),
        subject: summary.subject.clone(),
        month_names: months.clone(),
    }
}

/// The name an attachment gets in Downloads or a chosen folder: its last part after `/` or `\`,
/// cleaned as a target's scheme cleans it ([`torro_mail::save_target::clean_file_name`]: the names
/// Windows reserves, at most 120 bytes with the extension kept); `attachment` when nothing is left.
fn downloads_name(name: Option<&str>) -> String {
    let last = name.unwrap_or_default().rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned = torro_mail::save_target::clean_file_name(last);
    if cleaned.is_empty() { "attachment".to_string() } else { cleaned }
}

// ----- mail.save_attachments -----

/// Where one call puts its files, checked before the message is fetched.
#[derive(Debug, Clone)]
enum Place {
    /// Downloads or a chosen folder: the cleaned name in `dir`, a taken name kept ("Keep both").
    Folder(PathBuf),
    /// A save target: below its folder after its schemes, each file by its own date.
    Target(SaveTargetSpec),
    /// Exactly this file, replaced when it is there (the system's dialog asked).
    File(PathBuf),
}

/// The facts of the mail every file of one call shares, for the target's schemes.
struct Mail<'a> {
    summary: &'a MessageSummary,
    account: String,
    months: [String; 12],
}

/// What one call did, in the shape of [`Response::SavedAttachments`].
#[derive(Debug, Default, PartialEq)]
struct Saved {
    saved: Vec<SavedFile>,
    skipped: Vec<SavedFile>,
    failed: Vec<FailedFile>,
}

impl Saved {
    fn response(self) -> Response {
        Response::SavedAttachments { saved: self.saved, skipped: self.skipped, failed: self.failed }
    }

    fn record(&mut self, part: &str, name: &str, written: Result<Written, Problem>) {
        match written {
            Ok(Written::Saved(path)) => self.saved.push(SavedFile { part: part.into(), path: shown(&path) }),
            Ok(Written::Skipped(path)) => self.skipped.push(SavedFile { part: part.into(), path: shown(&path) }),
            Err(problem) => self.failed.push(FailedFile {
                part: part.into(),
                name: name.into(),
                code: problem.code().into(),
                message: problem.message(),
            }),
        }
    }
}

/// `mail.save_attachments`: in this order the paths of `to: file | folder` (`error` unless absolute,
/// one line, at most [`MAX_PATH_BYTES`]), the message in the stores `reach` sees (`not-found`), the
/// place (an unknown target, a missing folder: `not-found`), the parts the store knows (an unknown
/// one: `not-found`), then the message fetched once and each file saved; a file that fails is
/// listed in `failed` and the others are saved all the same.
async fn save(
    mail: &MailService,
    reach: Reach<'_>,
    context: &Context,
    id: &str,
    parts: &[String],
    to: &SaveTo,
) -> Answer<Response> {
    if let SaveTo::File(path) | SaveTo::Folder(path) = to
        && !path_fits(path)
    {
        return Err(failed("error", tr("The path must be a full path of at most 4096 bytes on one line.")));
    }
    if matches!(to, SaveTo::File(_)) && parts.len() > 1 {
        return Err(failed("error", tr("Save as takes exactly one attachment.")));
    }
    let stores = mail.stores(reach).await;
    let located = read::locate(&stores, id).await?;
    let message = read::stored_message(&located).await?;
    let place = place_of(mail, context, to)?;
    if message.text.is_some() && parts.iter().any(|part| !message.attachments.iter().any(|known| known.part == *part)) {
        return Err(read::no_attachment());
    }
    let raw = read::fetch_raw(mail, &located).await?;
    let wanted = wanted_parts(&raw, parts)?;
    if matches!(place, Place::File(_)) && wanted.len() != 1 {
        return Err(failed("error", tr("Save as takes exactly one attachment.")));
    }
    let shared =
        Mail { summary: &message.summary, account: account_name(mail, &located.account).await, months: month_names() };
    let mut saved = Saved::default();
    for part in wanted {
        save_one(&raw, &part, &place, &shared, ATTACHMENT_MAX_BYTES, &mut saved).await;
    }
    Ok(saved.response())
}

/// The place `to` names, checked: a known target whose folder exists, an existing Downloads or
/// chosen folder, a file whose folder exists. `not-found` otherwise, for the whole call.
fn place_of(mail: &MailService, context: &Context, to: &SaveTo) -> Answer<Place> {
    match to {
        SaveTo::Downloads => match &context.downloads {
            Some(dir) if dir.is_dir() => Ok(Place::Folder(dir.clone())),
            _ => Err(failed("not-found", tr("The Downloads folder is missing."))),
        },
        SaveTo::Target(id) => {
            let target = mail.save_targets().get(id).map_err(boxed)?.ok_or_else(|| boxed(TargetError::NotFound))?;
            target_place(target.spec)
        }
        SaveTo::Folder(dir) if Path::new(dir).is_dir() => Ok(Place::Folder(PathBuf::from(dir))),
        SaveTo::Folder(_) => Err(failed("not-found", tr("The folder is missing."))),
        SaveTo::File(file) if Path::new(file).parent().is_some_and(Path::is_dir) => {
            Ok(Place::File(PathBuf::from(file)))
        }
        SaveTo::File(_) => Err(failed("not-found", tr("The folder is missing."))),
    }
}

fn target_place(spec: SaveTargetSpec) -> Answer<Place> {
    if !Path::new(&spec.folder).is_dir() {
        let message = trf("The folder of {target} is missing.", &[("target", &spec.name)]);
        return Err(Box::new(Response::Error { code: "not-found".into(), message }));
    }
    Ok(Place::Target(spec))
}

/// The parts to save: `parts`, each once, or with none named every attachment of `raw` that is not
/// `inline`. `not-found` for a part `raw` has no attachment of; `error` when `raw` cannot be read.
fn wanted_parts(raw: &[u8], parts: &[String]) -> Answer<Vec<String>> {
    let read = mime::read(raw).map_err(|_| read::unreadable())?;
    if parts.is_empty() {
        return Ok(read.attachments.into_iter().filter(|info| !info.inline).map(|info| info.part).collect());
    }
    let mut wanted: Vec<String> = Vec::new();
    for part in parts {
        if !read.attachments.iter().any(|info| info.part == *part) {
            return Err(read::no_attachment());
        }
        if !wanted.contains(part) {
            wanted.push(part.clone());
        }
    }
    Ok(wanted)
}

/// Saves the attachment `part` of `raw` at `place`, at most `max_bytes`, and notes how it went in
/// `saved`. Writing runs off the runtime.
async fn save_one(raw: &[u8], part: &str, place: &Place, mail: &Mail<'_>, max_bytes: usize, saved: &mut Saved) {
    let attachment = match mime::attachment(raw, part) {
        Ok(attachment) => attachment,
        Err(err) => {
            let (code, message) = match err {
                MimeError::NoSuchPart => ("not-found", tr("The message has no such attachment.")),
                _ => ("error", tr("The message could not be read.")),
            };
            saved.failed.push(FailedFile {
                part: part.into(),
                name: String::new(),
                code: code.into(),
                message: message.into(),
            });
            return;
        }
    };
    let name = attachment.filename.clone().unwrap_or_default();
    let fail =
        |code: &str, message: String| FailedFile { part: part.into(), name: name.clone(), code: code.into(), message };
    if attachment.bytes.len() > max_bytes {
        saved.failed.push(fail("error", tr("The attachment is larger than 50 MiB.").into()));
        return;
    }
    let job = match destination(place, &attachment, mail) {
        Ok(job) => job,
        Err(problem) => {
            saved.failed.push(fail(problem.code(), problem.message()));
            return;
        }
    };
    let bytes = attachment.bytes;
    let written = tokio::task::spawn_blocking(move || job.run(&bytes)).await.unwrap_or(Err(Problem::Write));
    saved.record(part, &name, written);
}

fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// One file to write, the name settled.
#[derive(Debug, Clone)]
enum Job {
    /// A new file named `name` in `dir` (made below a target's folder when `below` is set),
    /// after `rule` for a taken name.
    New { dir: PathBuf, below: Option<(PathBuf, Vec<String>)>, name: String, rule: OnConflict },
    /// This file, replaced.
    Replace(PathBuf),
}

/// What became of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Written {
    Saved(PathBuf),
    /// The same content is there already ("Skip if identical").
    Skipped(PathBuf),
}

/// Why one file was not saved; [`Problem::code`] and [`Problem::message`] for [`FailedFile`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum Problem {
    /// The scheme of the target cannot be filled (`error`).
    Scheme,
    /// A subfolder resolves to a place outside the target's folder (`error`).
    Outside,
    /// Every name up to `(99)` is taken (`error`).
    Taken,
    /// The system refused writing there (`forbidden`; macOS asks before a program writes to
    /// Downloads, Documents and the Desktop).
    Denied,
    /// A folder or a link is where the file of `to: file` would be (`error`).
    NotReplaceable,
    /// Anything else failed (`error`).
    Write,
}

impl Problem {
    fn code(&self) -> &'static str {
        match self {
            Problem::Denied => "forbidden",
            _ => "error",
        }
    }

    fn message(&self) -> String {
        match self {
            Problem::Scheme => tr("The scheme of the save target has an unknown placeholder.").into(),
            Problem::Outside => tr("A folder of the save target leads outside of it.").into(),
            Problem::Taken => tr("Too many files of this name are in the folder.").into(),
            Problem::Denied => denied_message(),
            Problem::NotReplaceable => tr("A folder or a link cannot be replaced.").into(),
            Problem::Write => tr("The file could not be saved.").into(),
        }
    }

    fn of(err: &io::Error) -> Problem {
        match err.kind() {
            io::ErrorKind::PermissionDenied => Problem::Denied,
            _ => Problem::Write,
        }
    }
}

/// "Torro Workplace may not write to this folder.", on macOS with where a person allows it.
fn denied_message() -> String {
    let denied = tr("Torro Workplace may not write to this folder.");
    #[cfg(target_os = "macos")]
    {
        format!("{denied} {}", tr("Allow it in System Settings › Privacy & Security › Files and Folders."))
    }
    #[cfg(not(target_os = "macos"))]
    {
        denied.to_string()
    }
}

/// The file `attachment` becomes at `place`.
fn destination(place: &Place, attachment: &mime::Attachment, mail: &Mail<'_>) -> Result<Job, Problem> {
    match place {
        Place::Folder(dir) => Ok(Job::New {
            dir: dir.clone(),
            below: None,
            name: downloads_name(attachment.filename.as_deref()),
            rule: OnConflict::KeepBoth,
        }),
        Place::Target(spec) => {
            let facts = facts_of(
                mail.summary,
                attachment.filename.as_deref(),
                &attachment.content_type,
                &mail.account,
                &mail.months,
            );
            let resolved = resolve(spec, &facts).map_err(|_| Problem::Scheme)?;
            Ok(Job::New {
                dir: PathBuf::from(&spec.folder),
                below: Some((PathBuf::from(&spec.folder), resolved.folders)),
                name: resolved.file,
                rule: spec.on_conflict,
            })
        }
        Place::File(file) => Ok(Job::Replace(file.clone())),
    }
}

impl Job {
    /// EML publication: copy/verify to a private temporary in the destination directory, sync it,
    /// then atomically publish. Existing files remain intact on every pre-publication failure.
    fn run_file(self, mut source: super::eml::Prepared) -> Result<Written, Problem> {
        let (dir, name, rule, replace) = match self {
            Job::New { dir, below, name, rule } => {
                let dir = match below {
                    Some((base, folders)) => target_dir(&base, &folders)?,
                    None => dir,
                };
                (dir, name, rule, None)
            }
            Job::Replace(path) => {
                replaceable(&path)?;
                let dir = path.parent().ok_or(Problem::Write)?.to_path_buf();
                (dir, String::new(), OnConflict::KeepBoth, Some(path))
            }
        };
        let mut temporary = tempfile::NamedTempFile::new_in(&dir).map_err(|err| Problem::of(&err))?;
        super::eml::copy(&mut source.file, temporary.as_file_mut(), source.bytes, &source.sha256).map_err(|err| {
            match err {
                super::eml::Error::Write(io::ErrorKind::PermissionDenied) => Problem::Denied,
                _ => Problem::Write,
            }
        })?;
        temporary.as_file().sync_all().map_err(|err| Problem::of(&err))?;
        if let Some(path) = replace {
            replaceable(&path)?;
            temporary.persist(&path).map_err(|err| Problem::of(&err.error))?;
            return Ok(Written::Saved(path));
        }
        publish_new(temporary, &dir, &name, rule, &mut source.file, source.bytes)
    }

    /// Writes `bytes` (blocking).
    fn run(self, bytes: &[u8]) -> Result<Written, Problem> {
        self.run_verified(bytes, verify_file)
    }

    /// The verification seam exercises final-path failures after actual writes, without timing races.
    fn run_verified(
        self,
        bytes: &[u8],
        verify: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
    ) -> Result<Written, Problem> {
        let written = match self {
            Job::New { dir, below, name, rule } => {
                let dir = match below {
                    Some((base, folders)) => target_dir(&base, &folders)?,
                    None => dir,
                };
                write_new(&dir, &name, bytes, rule)
            }
            Job::Replace(file) => replace(&file, bytes),
        }?;
        let path = match &written {
            Written::Saved(path) | Written::Skipped(path) => path,
        };
        verify(path, bytes).map_err(|err| Problem::of(&err))?;
        Ok(written)
    }
}

/// Publish a verified destination temporary without ever overwriting a competing writer's file.
fn publish_new(
    mut temporary: tempfile::NamedTempFile,
    dir: &Path,
    name: &str,
    rule: OnConflict,
    source: &mut fs::File,
    bytes: u64,
) -> Result<Written, Problem> {
    for copy in 1..=MAX_COPY {
        let path = dir.join(numbered(name, copy));
        if rule == OnConflict::SkipIdentical && same_file(&path, source, bytes) {
            return Ok(Written::Skipped(path));
        }
        match temporary.persist_noclobber(&path) {
            Ok(_) => return Ok(Written::Saved(path)),
            Err(err) if err.error.kind() == io::ErrorKind::AlreadyExists => temporary = err.file,
            Err(err) => return Err(Problem::of(&err.error)),
        }
    }
    Err(Problem::Taken)
}

/// Never replace a link, directory or special file. A missing destination is allowed.
fn replaceable(path: &Path) -> Result<(), Problem> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(_) => Err(Problem::NotReplaceable),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Problem::of(&err)),
    }
}

/// Bounded equality check, never following a known symbolic link or buffering a whole original.
fn same_file(path: &Path, source: &mut fs::File, bytes: u64) -> bool {
    use std::io::{Read, Seek};
    if !fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == bytes) {
        return false;
    }
    let Ok(mut there) = fs::File::open(path) else { return false };
    if source.rewind().is_err() {
        return false;
    }
    let (mut a, mut b) = ([0u8; 64 * 1024], [0u8; 64 * 1024]);
    let mut left = bytes;
    while left > 0 {
        let count = left.min(a.len() as u64) as usize;
        if source.read_exact(&mut a[..count]).is_err()
            || there.read_exact(&mut b[..count]).is_err()
            || a[..count] != b[..count]
        {
            return false;
        }
        left -= count as u64;
    }
    there.read(&mut b[..1]).is_ok_and(|read| read == 0)
}

/// The folder `folders` below `base`, made one at a time: each one, once made (or found), is
/// resolved by the system and must still be below `base` resolved, so a symbolic link below `base`
/// that leads elsewhere stops it before anything is made there ([`Problem::Outside`]). The folder
/// it ends in is checked once more.
fn target_dir(base: &Path, folders: &[String]) -> Result<PathBuf, Problem> {
    let base = base.canonicalize().map_err(|err| Problem::of(&err))?;
    let inside = |path: &Path| -> Result<PathBuf, Problem> {
        let resolved = path.canonicalize().map_err(|err| Problem::of(&err))?;
        match resolved.starts_with(&base) && resolved.is_dir() {
            true => Ok(resolved),
            false => Err(Problem::Outside),
        }
    };
    let mut current = base.clone();
    for folder in folders {
        let next = current.join(folder);
        match fs::create_dir(&next) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(Problem::of(&err)),
        }
        current = inside(&next)?;
    }
    inside(&current)
}

/// Writes `bytes` as a new file `name` in `dir`, never over one that is there: a taken name gets a
/// number, `Name (2).ext` up to `(99)`; with [`OnConflict::SkipIdentical`] a file there (or a
/// numbered one) with the same content is the answer instead ([`Written::Skipped`]).
fn write_new(dir: &Path, name: &str, bytes: &[u8], rule: OnConflict) -> Result<Written, Problem> {
    for copy in 1..=MAX_COPY {
        let path = dir.join(numbered(name, copy));
        if rule == OnConflict::SkipIdentical && same_content(&path, bytes) {
            return Ok(Written::Skipped(path));
        }
        match create_new(&path, bytes) {
            Ok(()) => return Ok(Written::Saved(path)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(Problem::of(&err)),
        }
    }
    Err(Problem::Taken)
}

/// `name` as the `copy`th file of that name: itself for 1, else with ` (copy)` before its
/// extension (`Invoice (2).pdf`, `README (2)`).
fn numbered(name: &str, copy: u32) -> String {
    if copy <= 1 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({copy}){}", &name[..dot], &name[dot..]),
        _ => format!("{name} ({copy})"),
    }
}

/// Whether `path` is a plain file (not a link) holding exactly `bytes`.
fn same_content(path: &Path, bytes: &[u8]) -> bool {
    verify_file(path, bytes).is_ok()
}

/// Checks the closed file at its final path, without allocating another attachment-sized buffer.
/// Failures leave it untouched: another process may already have changed the destination.
fn verify_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    verify_file_opened(path, bytes, |_| {})
}

fn verify_file_opened(path: &Path, bytes: &[u8], opened: impl FnOnce(&Path)) -> io::Result<()> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "saved file verification failed");
    let plain = |meta: &fs::Metadata| meta.is_file() && meta.len() == bytes.len() as u64;
    let before = fs::symlink_metadata(path)?;
    if !plain(&before) {
        return Err(invalid());
    }
    let mut file = fs::File::open(path)?;
    let held = file.metadata()?;
    if !plain(&held) || !same_file_metadata(&before, &held) {
        return Err(invalid());
    }
    opened(path);
    let mut buffer = [0; 64 * 1024];
    for expected in bytes.chunks(buffer.len()) {
        let block = &mut buffer[..expected.len()];
        file.read_exact(block)?;
        if block != expected {
            return Err(invalid());
        }
    }
    let after = fs::symlink_metadata(path)?;
    if file.read(&mut buffer[..1])? != 0 || !plain(&after) || !same_file_metadata(&held, &after) {
        return Err(invalid());
    }
    Ok(())
}

/// Bind the bytes read to the same final file, not a replacement of the same size. Unix exposes
/// device/inode in safe std; other platforms have the portable creation/modification metadata.
fn same_file_metadata(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
            return false;
        }
    }
    a.created().ok() == b.created().ok() && a.modified().ok() == b.modified().ok()
}

/// Makes `path` with `bytes`; `AlreadyExists` when something is there (also a link, which is never
/// followed). A file half written is removed again.
fn create_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(err) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(err);
    }
    Ok(())
}

/// `to: file`: replaces the file at `path` (the system's dialog asked), like `tools.save_file_as`:
/// not a folder and not a symbolic link.
fn replace(path: &Path, bytes: &[u8]) -> Result<Written, Problem> {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir() || meta.file_type().is_symlink()) {
        return Err(Problem::NotReplaceable);
    }
    let mut file =
        fs::OpenOptions::new().write(true).create(true).truncate(true).open(path).map_err(|err| Problem::of(&err))?;
    file.write_all(bytes).and_then(|()| file.sync_all()).map_err(|err| Problem::of(&err))?;
    drop(file);
    Ok(Written::Saved(path.to_path_buf()))
}

// ----- mail.save_message_as (issue #501) -----

/// The most characters of the subject in a file name.
const MAX_SUBJECT_CHARS: usize = 60;

/// The subject as a file name takes it: one line, at most [`MAX_SUBJECT_CHARS`], "Mail" for none.
fn name_subject(subject: &str) -> String {
    let one_line: String = subject.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = one_line.chars().take(MAX_SUBJECT_CHARS).collect();
    match short.trim() {
        "" => tr("Mail").to_string(),
        short => short.to_string(),
    }
}

/// The kind of file `format` makes, for a target's "Suggest when" (decision 5 of issue #501).
fn export_kind(format: ExportFormat) -> Option<FileKind> {
    match format {
        ExportFormat::Pdf => Some(FileKind::Pdf),
        ExportFormat::Png => Some(FileKind::Image),
        ExportFormat::Eml => None,
    }
}

/// The facts a target's schemes are filled with for the mail `summary` saved as `format`: the file is
/// "<subject>.<ext>" for `{name}` and `{ext}`.
fn export_facts(summary: &MessageSummary, format: ExportFormat, account: &str, months: &[String; 12]) -> FileFacts {
    let name = format!("{}.{}", name_subject(&summary.subject), format.extension());
    facts_of(summary, Some(&name), format.content_type(), account, months)
}

/// Where the mail `summary` saved as `format` would go (decision 5 of issue #501): in Downloads or a
/// chosen folder "<YYYY-MM-DD> <subject>.<ext>" by the mail's day on this computer's clock, in each of
/// `targets` after its schemes, and the targets whose "Suggest when" holds for a PDF or a picture.
pub fn export_plan(
    targets: &[SaveTarget],
    summary: &MessageSummary,
    format: ExportFormat,
    account: &str,
    months: &[String; 12],
) -> ExportPlan {
    let day = summary.date.with_timezone(&Local).format("%Y-%m-%d");
    let file_name = torro_mail::save_target::clean_file_name(&format!(
        "{day} {}.{}",
        name_subject(&summary.subject),
        format.extension()
    ));
    let facts = export_facts(summary, format, account, months);
    let mut plan = ExportPlan { format, file_name, places: Vec::new(), suggested: Vec::new() };
    for target in targets {
        if let Ok(resolved) = resolve(&target.spec, &facts) {
            plan.places.push(PlannedPlace {
                target: target.id.clone(),
                folders: resolved.folders,
                file: resolved.file,
            });
        }
        if suggestion(&target.spec.suggest, summary.category, export_kind(format), &facts.sender_address).is_some() {
            plan.suggested.push(target.id.clone());
        }
    }
    plan
}

/// `mail.save_message_as`: in this order the path of `to: file | folder` (`error` unless absolute, one
/// line, at most [`MAX_PATH_BYTES`]), the message in the stores `reach` sees (`not-found`), the place
/// (an unknown target, a missing folder: `not-found`), then the message fetched once, drawn
/// ([`super::export::draw`], `error` when it cannot be) and written by the rules of attachments.
async fn save_message(
    mail: &MailService,
    reach: Reach<'_>,
    context: &Context,
    id: &str,
    format: ExportFormat,
    to: &SaveTo,
    checked_target: Option<SaveTargetSpec>,
) -> Answer<Response> {
    if let SaveTo::File(path) | SaveTo::Folder(path) = to
        && !path_fits(path)
    {
        return Err(failed("error", tr("The path must be a full path of at most 4096 bytes on one line.")));
    }
    let stores = mail.stores(reach).await;
    let located = read::locate(&stores, id).await?;
    let place = match checked_target {
        Some(spec) => target_place(spec)?,
        None => place_of(mail, context, to)?,
    };
    if format == ExportFormat::Eml {
        let summary = summary_of(&located).await?;
        // Keep the chunked IMAP capture out of the shared save/dispatch future, even for non-EML calls.
        let original = Box::pin(super::eml::prepare(mail, &located)).await.map_err(|err| err.response())?;
        let (account, months) = (account_name(mail, &located.account).await, month_names());
        let job = export_destination(&place, &summary, format, &account, &months).map_err(|err| err.failed())?;
        let written = tokio::task::spawn_blocking(move || job.run_file(original)).await.unwrap_or(Err(Problem::Write));
        return match written {
            Ok(Written::Saved(path)) => {
                Ok(Response::SavedMail { path: shown(&path), skipped: false, shortened: false })
            }
            Ok(Written::Skipped(path)) => {
                Ok(Response::SavedMail { path: shown(&path), skipped: true, shortened: false })
            }
            Err(problem) => Err(problem.failed()),
        };
    }
    let message = read::stored_message(&located).await?;
    let summary = &message.summary;
    let raw = read::fetch_raw(mail, &located).await?;
    let read = mime::read(&raw).map_err(|_| read::unreadable())?;
    let head = super::export::Head {
        subject: summary.subject.clone(),
        from: summary.from.clone(),
        to: summary.to.clone(),
        cc: message.cc.clone(),
        date: summary.date,
    };
    let body = match read.html {
        Some(html) => super::export::Body::Html { html, raw: Arc::new(raw), parts: read.attachments },
        None => super::export::Body::Text(read.text),
    };
    let drawn = super::export::draw(head, body, format, true)
        .await
        .map_err(|()| failed("error", tr("The mail could not be drawn.")))?;
    let (account, months) = (account_name(mail, &located.account).await, month_names());
    let job = export_destination(&place, summary, format, &account, &months).map_err(|problem| problem.failed())?;
    let bytes = drawn.bytes;
    let written = tokio::task::spawn_blocking(move || job.run(&bytes)).await.unwrap_or(Err(Problem::Write));
    let shortened = drawn.shortened;
    match written {
        Ok(Written::Saved(path)) => Ok(Response::SavedMail { path: shown(&path), skipped: false, shortened }),
        Ok(Written::Skipped(path)) => Ok(Response::SavedMail { path: shown(&path), skipped: true, shortened }),
        Err(problem) => Err(problem.failed()),
    }
}

/// Runs an approved EML save with the exact target snapshot fingerprinted by the core. Never
/// re-reads that target after checking the approval, even if a person edits it concurrently.
pub(crate) async fn approved_eml(
    mail: &MailService,
    reach: Reach<'_>,
    context: &Context,
    id: &str,
    to: &SaveTo,
    target: Option<SaveTargetSpec>,
) -> Response {
    save_message(mail, reach, context, id, ExportFormat::Eml, to, target).await.unwrap_or_else(|err| *err)
}

/// The file the mail `summary` saved as `format` becomes at `place`.
fn export_destination(
    place: &Place,
    summary: &MessageSummary,
    format: ExportFormat,
    account: &str,
    months: &[String; 12],
) -> Result<Job, Problem> {
    match place {
        Place::Folder(dir) => Ok(Job::New {
            dir: dir.clone(),
            below: None,
            name: export_plan(&[], summary, format, account, months).file_name,
            rule: OnConflict::KeepBoth,
        }),
        Place::Target(spec) => {
            let resolved =
                resolve(spec, &export_facts(summary, format, account, months)).map_err(|_| Problem::Scheme)?;
            Ok(Job::New {
                dir: PathBuf::from(&spec.folder),
                below: Some((PathBuf::from(&spec.folder), resolved.folders)),
                name: resolved.file,
                rule: spec.on_conflict,
            })
        }
        Place::File(file) => Ok(Job::Replace(file.clone())),
    }
}

impl Problem {
    /// The problem as the answer of a call that saves one file.
    fn failed(&self) -> Box<Response> {
        Box::new(Response::Error { code: self.code().into(), message: self.message() })
    }
}

fn failed(code: &str, message: &str) -> Box<Response> {
    Box::new(Response::Error { code: code.into(), message: message.into() })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use chrono::{TimeZone, Utc};
    use torro_ipc::api::{RightsTemplate, Selection};
    use torro_mail::{DateSource, MailAddress, MailCategory, SuggestWhen};

    use super::*;
    use crate::account::github::tests::MemoryTokens;
    use crate::mail::service::tests::{PASSWORD, mail_account, service_in, store_accounts};
    use crate::mail::sync::fake::FakeImap;

    const ANNA: &str = "mail:anna@example.org";

    #[test]
    fn verification_failures_after_writing_keep_files_and_allow_partial_results() {
        for mode in ["new", "replace", "identical", "empty", "missing", "wrong", "truncated", "denied"] {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = dir.path().join("attachment.pdf");
            let job = match mode {
                "replace" => {
                    fs::write(&path, b"previous").expect("old file");
                    Job::Replace(path.clone())
                }
                _ => {
                    if mode == "identical" {
                        fs::write(&path, b"expected").expect("identical");
                    }
                    Job::New {
                        dir: dir.path().into(),
                        below: None,
                        name: "attachment.pdf".into(),
                        rule: OnConflict::SkipIdentical,
                    }
                }
            };
            let bytes = if mode == "empty" { &b""[..] } else { &b"expected"[..] };
            let written = job.run_verified(bytes, |path, bytes| {
                match mode {
                    "missing" => fs::remove_file(path)?,
                    "wrong" => fs::write(path, b"opposite")?,
                    "truncated" => fs::write(path, b"ex")?,
                    "denied" => return Err(io::Error::from(io::ErrorKind::PermissionDenied)),
                    _ => {}
                }
                verify_file(path, bytes)
            });
            let mut saved = Saved::default();
            saved.record("2", "attachment.pdf", written);
            if matches!(mode, "missing" | "wrong" | "truncated" | "denied") {
                assert_eq!(saved.failed.len(), 1, "{mode}");
                assert_eq!(saved.failed[0].code, if mode == "denied" { "forbidden" } else { "error" });
                assert!(saved.saved.is_empty() && saved.skipped.is_empty());
                if mode != "missing" {
                    assert!(path.is_file(), "verification must not delete: {mode}");
                }
            } else {
                assert!(saved.failed.is_empty(), "{mode}");
                assert_eq!(saved.saved.len() + saved.skipped.len(), 1, "{mode}");
                assert_eq!(saved.skipped.len(), usize::from(mode == "identical"));
                assert_eq!(fs::read(&path).expect("verified final file"), bytes);
            }
            let next =
                Job::New { dir: dir.path().into(), below: None, name: "next.pdf".into(), rule: OnConflict::KeepBoth };
            saved.record("3", "next.pdf", next.run(b"next"));
            assert!(saved.saved.iter().any(|file| file.part == "3"), "next attachment still saved: {mode}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn verification_rejects_a_same_sized_replacement_of_the_open_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("saved");
        fs::write(&path, b"expected").expect("file");
        let result = verify_file_opened(&path, b"expected", |path| {
            fs::rename(path, dir.path().join("previous")).expect("move opened inode");
            fs::write(path, b"opposite").expect("same-sized replacement");
        });
        assert!(result.is_err());
        assert_eq!(fs::read(path).expect("replacement left intact"), b"opposite");
    }

    #[test]
    fn final_file_verification_checks_type_length_and_every_byte() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("saved.pdf");
        let bytes = vec![42; 150_000];
        assert!(verify_file(&path, &bytes).is_err(), "missing");
        fs::write(&path, &bytes).expect("file");
        verify_file(&path, &bytes).expect("verified");
        let mut wrong = bytes.clone();
        wrong[100_000] = 43;
        assert!(verify_file(&path, &wrong).is_err(), "same length, wrong content in a later block");
        assert!(verify_file(&path, &bytes[..bytes.len() - 1]).is_err(), "too long");
        assert!(verify_file(&path, &[bytes.as_slice(), &[42]].concat()).is_err(), "truncated");
        assert_eq!(fs::read(&path).expect("left intact"), bytes);
        fs::write(&path, []).expect("empty");
        verify_file(&path, &[]).expect("zero-byte files are real files");
        assert!(verify_file(dir.path(), &[]).is_err(), "not a regular file");
    }

    #[cfg(unix)]
    #[test]
    fn final_file_verification_refuses_links_and_unreadable_files() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("saved");
        fs::write(&path, b"contents").expect("file");
        let link = dir.path().join("link");
        symlink(&path, &link).expect("link");
        assert!(verify_file(&link, b"contents").is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("permissions");
        // Privileged test runners may still read mode 000; ordinary runners must report forbidden.
        if fs::File::open(&path).is_err() {
            let problem = verify_file(&path, b"contents").expect_err("unreadable");
            assert_eq!(Problem::of(&problem).code(), "forbidden");
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("restore");
    }

    fn targets_in(dir: &Path) -> SaveTargets {
        SaveTargets::at(Some(dir.join("mail").join(FILE_NAME)))
    }

    fn spec(name: &str, folder: &Path) -> SaveTargetSpec {
        SaveTargetSpec::new(name, folder.to_string_lossy())
    }

    fn invalid(result: Result<SaveTarget, TargetError>) -> String {
        match result {
            Err(TargetError::Invalid(message)) => message,
            other => panic!("refused as invalid, got {other:?}"),
        }
    }

    /// A message from Stadtwerke Bonn of 2026-03-28 with `files` (name, content) attached as
    /// `application/pdf`.
    fn raw_with(files: &[(&str, &[u8])]) -> String {
        raw_on(Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).single().expect("a date"), "Ihre Rechnung", files)
    }

    /// [`raw_with`] sent at `date` with `subject`.
    pub(crate) fn raw_on(date: chrono::DateTime<Utc>, subject: &str, files: &[(&str, &[u8])]) -> String {
        use base64::Engine;
        let (date, id) = (date.to_rfc2822(), subject.replace(' ', "-"));
        let mut raw = format!(
            "From: Stadtwerke Bonn <billing@stadtwerke-bonn.de>\r\nTo: anna@example.org\r\n\
             Subject: {subject}\r\nDate: {date}\r\nMessage-ID: <{id}@example.org>\r\nMIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\nSee attached.\r\n"
        );
        for (name, content) in files {
            let encoded = base64::engine::general_purpose::STANDARD.encode(content);
            raw.push_str(&format!(
                "--b1\r\nContent-Type: application/pdf; name=\"{name}\"\r\n\
                 Content-Disposition: attachment; filename=\"{name}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n\
                 {encoded}\r\n"
            ));
        }
        raw.push_str("--b1--\r\n");
        raw
    }

    /// The parts of the attachments of `raw`, in order.
    fn parts_of(raw: &[u8]) -> Vec<String> {
        mime::read(raw).expect("a message").attachments.into_iter().filter(|a| !a.inline).map(|a| a.part).collect()
    }

    fn summary() -> MessageSummary {
        MessageSummary {
            caught: false,
            id: "m:1".into(),
            account: ANNA.into(),
            folder: "f:1".into(),
            thread_id: "t:1".into(),
            from: Some(MailAddress {
                name: Some("Stadtwerke Bonn".into()),
                address: "billing@stadtwerke-bonn.de".into(),
            }),
            to: Vec::new(),
            subject: "Ihre Rechnung".into(),
            date: Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).single().expect("a date"),
            snippet: String::new(),
            flags: Vec::new(),
            has_attachments: true,
            invite: None,
            back: None,
            pile: None,
            category: None,
            untrusted: true,
            pile_since: None,
            logo: None,
        }
    }

    /// Saves every part of `raw` at `place` with the limit `max`, as the handler does.
    async fn save_all(raw: &[u8], place: &Place, max: usize) -> Saved {
        let summary = summary();
        let mail = Mail { summary: &summary, account: "Privat".into(), months: month_names() };
        let mut saved = Saved::default();
        for part in parts_of(raw) {
            save_one(raw, &part, place, &mail, max, &mut saved).await;
        }
        saved
    }

    fn names(files: &[SavedFile]) -> Vec<String> {
        files
            .iter()
            .map(|file| Path::new(&file.path).file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string())
            .collect()
    }

    /// The targets are checked, kept with a random id in a private file, and found by it; a broken
    /// rule stores nothing.
    #[test]
    fn targets_are_checked_and_kept_in_their_own_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let targets = targets_in(dir.path());
        let folder = dir.path().join("Rechnungen");
        assert!(targets.list().expect("none yet").is_empty());

        let made = targets.create(spec("  Invoices ", &folder), &Category::BUILT_IN).expect("stored");
        assert_eq!((made.spec.name.as_str(), made.id.len()), ("Invoices", 16), "trimmed, random id");
        assert_eq!(targets.get(&made.id).expect("read"), Some(made.clone()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join("mail").join(FILE_NAME)).expect("a file").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let all = &Category::BUILT_IN;
        assert_eq!(invalid(targets.create(spec("INVOICES", &folder), all)), tr("Another save target has this name."));
        assert_eq!(
            invalid(targets.create(spec("", &folder), all)),
            tr("A save target needs a name of 1 to 60 characters on one line.")
        );
        assert!(matches!(targets.create(spec(&"x".repeat(61), &folder), all), Err(TargetError::Invalid(_))));
        assert!(matches!(targets.create(spec("Two\nlines", &folder), all), Err(TargetError::Invalid(_))));
        let relative = SaveTargetSpec::new("Relative", "Rechnungen");
        assert_eq!(
            invalid(targets.create(relative, all)),
            tr("The folder of a save target must be a full path of at most 4096 bytes.")
        );
        let long = SaveTargetSpec::new("Long", format!("/{}", "a".repeat(MAX_PATH_BYTES)));
        assert!(matches!(targets.create(long, all), Err(TargetError::Invalid(_))));
        let unknown = SaveTargetSpec { subfolders: "{yaer}/{month}".into(), ..spec("Typo", &folder) };
        assert_eq!(
            invalid(targets.create(unknown, all)),
            trf("The scheme has an unknown placeholder: {placeholder}", &[("placeholder", &"{yaer}")])
        );
        let wide = SaveTargetSpec { file_name: "x".repeat(201), ..spec("Wide", &folder) };
        assert!(matches!(targets.create(wide, all), Err(TargetError::Invalid(_))));
        let many = SuggestWhen { senders: vec!["a.de".into(); 21], ..SuggestWhen::default() };
        assert!(matches!(
            targets.create(SaveTargetSpec { suggest: many, ..spec("Many", &folder) }, all),
            Err(TargetError::Invalid(_))
        ));
        let custom = Category::Custom(torro_mail::CustomCategory::new(7).expect("a number"));
        let other = SuggestWhen { categories: vec![custom], ..SuggestWhen::default() };
        assert_eq!(
            invalid(targets.create(SaveTargetSpec { suggest: other, ..spec("Club", &folder) }, all)),
            tr("A category of the save target is not in the catalogue.")
        );
        assert_eq!(targets.list().expect("read").len(), 1, "nothing else was stored");

        for n in 1..MAX_TARGETS {
            targets.create(spec(&format!("Target {n}"), &folder), all).expect("within the limit");
        }
        assert_eq!(
            invalid(targets.create(spec("One more", &folder), all)),
            tr("There can be at most 50 save targets.")
        );

        let renamed = targets.update(&made.id, spec("Bills", &folder), all).expect("changed");
        assert_eq!((renamed.id.as_str(), renamed.spec.name.as_str()), (made.id.as_str(), "Bills"));
        assert!(matches!(targets.update("nope", spec("X", &folder), all), Err(TargetError::NotFound)));
        let rest = targets.remove(&made.id).expect("removed");
        assert!(rest.iter().all(|target| target.id != made.id) && rest.len() == MAX_TARGETS - 1);
        assert!(matches!(targets.remove(&made.id), Err(TargetError::NotFound)));
    }

    /// A client's change keeps the stored folder, read under the lock of the write: a person who
    /// changed it after the core's check keeps theirs.
    #[test]
    fn a_client_keeps_the_folder_a_person_set_meanwhile() {
        let dir = tempfile::tempdir().expect("temp dir");
        let targets = targets_in(dir.path());
        let (first, second) = (dir.path().join("Rechnungen"), dir.path().join("Belege"));
        let made = targets.create(spec("Invoices", &first), &Category::BUILT_IN).expect("stored");
        // The core checked the client's folder against the first one; then the person moved it.
        targets.update(&made.id, spec("Invoices", &second), &Category::BUILT_IN).expect("the person");
        let sent = SaveTargetSpec { subfolders: "{year}".into(), ..spec("Invoices", &first) };
        let kept = targets.update_by_client(&made.id, sent, &Category::BUILT_IN).expect("the client");
        assert_eq!(kept.spec.folder, second.to_string_lossy(), "the person's folder stays");
        assert_eq!(kept.spec.subfolders, "{year}", "the rest is the client's");
    }

    /// Names from the mail are cleaned and stay one piece in the folder; nothing is overwritten:
    /// a taken name is numbered, again and again.
    #[tokio::test]
    async fn files_are_cleaned_and_never_overwritten() {
        let dir = tempfile::tempdir().expect("temp dir");
        let place = Place::Folder(dir.path().to_path_buf());
        let raw = raw_with(&[("../../etc/passwd", b"one"), ("CON.pdf", b"two"), ("a:b?.pdf", b"three")]);
        let first = save_all(raw.as_bytes(), &place, ATTACHMENT_MAX_BYTES).await;
        assert!(first.failed.is_empty(), "{first:?}");
        assert_eq!(names(&first.saved), ["passwd", "_CON.pdf", "a_b_.pdf"]);
        assert!(first.saved.iter().all(|file| Path::new(&file.path).parent() == Some(dir.path())));

        let again = save_all(raw.as_bytes(), &place, ATTACHMENT_MAX_BYTES).await;
        assert_eq!(names(&again.saved), ["passwd (2)", "_CON (2).pdf", "a_b_ (2).pdf"]);
        let third = save_all(raw.as_bytes(), &place, ATTACHMENT_MAX_BYTES).await;
        assert_eq!(names(&third.saved)[1], "_CON (3).pdf");
        assert_eq!(fs::read(dir.path().join("_CON.pdf")).expect("kept"), b"two");
        assert_eq!(numbered("README", 2), "README (2)");
    }

    /// A target puts each file below its folder after its schemes, with the date of the file's
    /// name or else the mail's; "Skip if identical" saves nothing when the same content is there,
    /// and keeps both when it differs.
    #[tokio::test]
    async fn a_target_files_by_date_and_skips_identical_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().join("Rechnungen");
        fs::create_dir(&base).expect("the folder");
        let target = SaveTargetSpec {
            date_from: DateSource::FileName,
            on_conflict: OnConflict::SkipIdentical,
            ..spec("Invoices", &base)
        };
        let place = Place::Target(target);
        let raw = raw_with(&[("Rechnung_2025-12-01.pdf", b"old"), ("Rechnung.pdf", b"new")]);
        let first = save_all(raw.as_bytes(), &place, ATTACHMENT_MAX_BYTES).await;
        let paths: Vec<PathBuf> = first.saved.iter().map(|file| PathBuf::from(&file.path)).collect();
        let canonical = base.canonicalize().expect("the folder");
        assert_eq!(
            paths,
            [
                canonical.join("2025").join("12").join("Rechnung_2025-12-01.pdf"),
                canonical.join("2026").join("03").join("Rechnung.pdf")
            ]
        );

        let again = save_all(raw.as_bytes(), &place, ATTACHMENT_MAX_BYTES).await;
        assert!(again.saved.is_empty(), "{again:?}");
        assert_eq!(again.skipped.iter().map(|file| PathBuf::from(&file.path)).collect::<Vec<_>>(), paths);

        let changed = raw_with(&[("Rechnung.pdf", b"changed")]);
        let both = save_all(changed.as_bytes(), &place, ATTACHMENT_MAX_BYTES).await;
        assert_eq!(names(&both.saved), ["Rechnung (2).pdf"]);
        assert_eq!(fs::read(&paths[1]).expect("kept"), b"new");
    }

    /// A file over the limit fails on its own; the others of "Save all" are saved, and the answer
    /// names the one missing, without a path.
    #[tokio::test]
    async fn a_file_over_the_limit_fails_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let place = Place::Folder(dir.path().to_path_buf());
        let raw = raw_with(&[("small.pdf", b"tiny"), ("big.pdf", b"far too large")]);
        let saved = save_all(raw.as_bytes(), &place, 8).await;
        assert_eq!(names(&saved.saved), ["small.pdf"]);
        let [failed] = saved.failed.as_slice() else { panic!("one failed: {saved:?}") };
        assert_eq!((failed.name.as_str(), failed.code.as_str()), ("big.pdf", "error"));
        assert_eq!(failed.message, tr("The attachment is larger than 50 MiB."));
        assert!(!dir.path().join("big.pdf").exists());
    }

    /// A subfolder that is a symbolic link out of the target's folder is never followed to make or
    /// write anything there; a link inside the folder is fine.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_below_the_folder_never_leads_outside() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (base, outside) = (dir.path().join("Rechnungen"), dir.path().join("elsewhere"));
        fs::create_dir(&base).expect("the folder");
        fs::create_dir(&outside).expect("outside");
        std::os::unix::fs::symlink(&outside, base.join("2026")).expect("a link");
        let place = Place::Target(spec("Invoices", &base));
        let raw = raw_with(&[("Rechnung.pdf", b"pdf")]);
        let saved = save_all(raw.as_bytes(), &place, ATTACHMENT_MAX_BYTES).await;
        assert!(saved.saved.is_empty(), "{saved:?}");
        assert_eq!(saved.failed[0].message, tr("A folder of the save target leads outside of it."));
        assert_eq!(fs::read_dir(&outside).expect("readable").count(), 0, "nothing was made outside");

        fs::remove_file(base.join("2026")).expect("unlinked");
        fs::create_dir(base.join("real")).expect("a folder inside");
        std::os::unix::fs::symlink(base.join("real"), base.join("2026")).expect("a link inside");
        let saved = save_all(raw.as_bytes(), &place, ATTACHMENT_MAX_BYTES).await;
        assert_eq!(names(&saved.saved), ["Rechnung.pdf"]);
        assert!(base.join("real").join("03").join("Rechnung.pdf").exists());
    }

    /// The system refusing to write gives `forbidden` and a message without a path.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_folder_the_core_may_not_write_to_is_forbidden() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).expect("a folder");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).expect("read only");
        let saved = save_all(raw_with(&[("a.pdf", b"x")]).as_bytes(), &Place::Folder(locked.clone()), 100).await;
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).expect("writable again");
        let [failed] = saved.failed.as_slice() else { panic!("refused: {saved:?}") };
        assert_eq!(failed.code, "forbidden");
        assert!(failed.message.starts_with(tr("Torro Workplace may not write to this folder.")), "{failed:?}");
        assert!(!failed.message.contains(&*locked.to_string_lossy()));
    }

    /// `to: file` replaces the file the dialog chose, but never a folder or a link.
    #[tokio::test]
    async fn save_as_replaces_the_chosen_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("chosen.pdf");
        fs::write(&file, b"before").expect("there");
        let raw = raw_with(&[("Rechnung.pdf", b"after")]);
        let saved = save_all(raw.as_bytes(), &Place::File(file.clone()), 100).await;
        assert_eq!(names(&saved.saved), ["chosen.pdf"]);
        assert_eq!(fs::read(&file).expect("replaced"), b"after");
        let folder = save_all(raw.as_bytes(), &Place::File(dir.path().to_path_buf()), 100).await;
        assert_eq!(folder.failed[0].message, tr("A folder or a link cannot be replaced."));
    }

    /// A plan names each target's place, the targets suggested and the one a click saves to: a
    /// category the model only guessed suggests, but is not sure.
    #[test]
    fn a_plan_suggests_by_category_kind_and_sender() {
        let folder = Path::new("/Users/anna/Rechnungen");
        let by_category = |categories: Vec<Category>| SuggestWhen { categories, ..SuggestWhen::default() };
        let targets = vec![
            SaveTarget { id: "never".into(), spec: spec("Never", folder) },
            SaveTarget {
                id: "invoices".into(),
                spec: SaveTargetSpec { suggest: by_category(vec![Category::Invoice]), ..spec("Invoices", folder) },
            },
            SaveTarget {
                id: "bonn".into(),
                spec: SaveTargetSpec {
                    suggest: SuggestWhen {
                        kinds: vec![FileKind::Pdf],
                        senders: vec!["stadtwerke-bonn.de".into()],
                        ..SuggestWhen::default()
                    },
                    subfolders: "{sender}/{year}".into(),
                    ..spec("Bonn", folder)
                },
            },
        ];
        let attachment = AttachmentInfo {
            part: "2".into(),
            filename: Some("../Rechnung.pdf".into()),
            content_type: "application/pdf".into(),
            size: 3,
            inline: false,
            content_id: None,
        };
        let guessed = MailCategory::by_model(Category::Invoice, 0.8);
        let header = MailCategory::by_rule(Category::Invoice);
        let months = month_names();

        let plan_for =
            |category| plan(&targets, &MessageSummary { category, ..summary() }, &attachment, "Privat", &months);
        let guess = plan_for(Some(guessed));
        assert_eq!(guess.file_name, "Rechnung.pdf");
        assert_eq!(guess.places.len(), 3);
        assert_eq!(guess.places[2].folders, ["Stadtwerke Bonn", "2026"]);
        assert_eq!(guess.suggested, ["invoices", "bonn"]);
        assert_eq!(guess.sure.as_deref(), Some("bonn"), "the guessed category is not sure");
        let sure = plan_for(Some(header));
        assert_eq!(sure.sure.as_deref(), Some("invoices"));
        let none = plan_for(None);
        assert_eq!(none.suggested, ["bonn"], "no category fits no category condition");
        let png = AttachmentInfo { content_type: "image/png".into(), filename: Some("scan.png".into()), ..attachment };
        let picture = plan(&targets, &summary(), &png, "Privat", &months);
        assert!(picture.suggested.is_empty() && picture.sure.is_none());
    }

    /// A name longer than a file system takes is cut before its extension, for Downloads and a chosen
    /// folder as for a target's scheme: `Invoice….pdf` stays a PDF.
    #[test]
    fn a_long_name_keeps_its_extension() {
        let long = format!("Rechnung {}.pdf", "x".repeat(187));
        assert_eq!(long.len(), 200);
        let name = downloads_name(Some(&long));
        assert!(name.len() <= 120 && name.ends_with(".pdf"), "{name}");
        let attachment = AttachmentInfo {
            part: "2".into(),
            filename: Some(long),
            content_type: "application/pdf".into(),
            size: 3,
            inline: false,
            content_id: None,
        };
        let targets = vec![SaveTarget { id: "any".into(), spec: spec("Any", Path::new("/Users/anna/Belege")) }];
        let planned = plan(&targets, &summary(), &attachment, "Privat", &month_names());
        assert_eq!(planned.file_name, name);
        assert_eq!(planned.places[0].file, name, "the same name as the target's \"{{name}}.{{ext}}\"");
    }

    /// The service with Anna's account on `server`, synced once.
    async fn synced(dir: &Path, server: &FakeImap) -> MailService {
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir, &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir, &secrets);
        service.reload().await;
        service.sync_all(Reach::Suite).await.expect("synced");
        service
    }

    /// The id of the mail with `subject` in Anna's inbox.
    pub(crate) async fn message_id(service: &MailService, subject: &str) -> String {
        let (_, store) = service.stores(Reach::Suite).await.into_iter().next().expect("Anna's store");
        let found = store
            .run(|store| {
                let Some(inbox) = store.folder_by_role(torro_mail::Role::Inbox)? else { return Ok(Vec::new()) };
                store.list_messages(&inbox.id, 10, 0)
            })
            .await
            .expect("listed");
        found.into_iter().find(|found| found.subject == subject).expect("the mail").id
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    /// The whole way through the service: the plan from the stored message, the files saved to a
    /// target once fetched, by the mail's day on this computer's clock; a missing folder stops the
    /// call, and a client without the account finds neither the message to plan nor to save.
    #[tokio::test]
    async fn attachments_are_planned_and_saved_from_the_reachable_stores_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let now = Utc::now();
        let files: [(&str, &[u8]); 2] = [("Rechnung_2026-03-28_4711.pdf", b"pdf"), ("Liste.pdf", b"list")];
        server.add("INBOX", &raw_on(now, "Recent", &files), &[]);
        server.add("INBOX", &raw_with(&files), &[]);
        let service = synced(dir.path(), &server).await;
        let id = message_id(&service, "Recent").await;
        let day = now.with_timezone(&Local);
        let (year, month) = (day.format("%Y").to_string(), day.format("%m").to_string());
        let base = dir.path().join("Rechnungen");
        fs::create_dir(&base).expect("the folder");
        let downloads = dir.path().join("Downloads");
        fs::create_dir(&downloads).expect("downloads");
        let context = Context { downloads: Some(downloads.clone()), categories: Category::BUILT_IN.to_vec() };
        let made = service.save_targets().create(spec("Invoices", &base), &context.categories).expect("stored");

        let listed = answer(
            &service,
            Reach::Suite,
            &context,
            Request::ListSaveTargets { id: Some(id.clone()), export_only: false },
        )
        .await;
        let Response::SaveTargets { targets, downloads: shown, files, .. } = listed else {
            panic!("targets: {listed:?}")
        };
        assert_eq!((targets, shown), (vec![made.clone()], downloads.to_string_lossy().into_owned()));
        assert_eq!(
            files.iter().map(|plan| plan.file_name.as_str()).collect::<Vec<_>>(),
            ["Rechnung_2026-03-28_4711.pdf", "Liste.pdf"]
        );
        assert_eq!(files[0].places[0].folders, [year.clone(), month.clone()]);

        let all =
            Request::SaveMailAttachments { id: id.clone(), parts: Vec::new(), to: SaveTo::Target(made.id.clone()) };
        let saved = answer(&service, Reach::Suite, &context, all.clone()).await;
        let Response::SavedAttachments { saved, skipped, failed } = saved else { panic!("saved: {saved:?}") };
        assert_eq!((saved.len(), skipped.len(), failed.len()), (2, 0, 0));
        let folder = base.canonicalize().expect("the folder").join(&year).join(&month);
        assert_eq!(fs::read(folder.join("Liste.pdf")).expect("saved"), b"list");

        let one =
            Request::SaveMailAttachments { id: id.clone(), parts: vec![files[1].part.clone()], to: SaveTo::Downloads };
        let Response::SavedAttachments { saved, .. } = answer(&service, Reach::Suite, &context, one).await else {
            panic!("saved")
        };
        assert_eq!(saved[0].path, downloads.join("Liste.pdf").to_string_lossy());
        let unknown = Request::SaveMailAttachments { id: id.clone(), parts: vec!["9".into()], to: SaveTo::Downloads };
        assert_eq!(code(&answer(&service, Reach::Suite, &context, unknown).await), "not-found");

        fs::remove_dir_all(&base).expect("removed");
        let missing = answer(&service, Reach::Suite, &context, all).await;
        assert_eq!(
            missing,
            Response::Error {
                code: "not-found".into(),
                message: trf("The folder of {target} is missing.", &[("target", &"Invoices")]),
            }
        );

        let mut rights = RightsTemplate::FullAccess.rights();
        rights.mail_accounts = Selection::Only(Vec::new());
        let client = Reach::Client(&rights);
        let plan = Request::ListSaveTargets { id: Some(id.clone()), export_only: false };
        assert_eq!(code(&answer(&service, client, &context, plan).await), "not-found");
        let save = Request::SaveMailAttachments { id, parts: Vec::new(), to: SaveTo::Downloads };
        assert_eq!(code(&answer(&service, client, &context, save).await), "not-found");
        assert_eq!(fs::read_dir(&downloads).expect("readable").count(), 1, "nothing more was saved");
    }

    /// Issue #501: the mail itself is named "<date> <subject>" in Downloads, by its subject in a
    /// target's schemes, and suggested where a target asks for PDFs or pictures.
    #[test]
    fn a_mail_is_planned_by_its_date_and_subject() {
        let folder = Path::new("/Users/anna/Rechnungen");
        let pdfs = SuggestWhen { kinds: vec![FileKind::Pdf], ..SuggestWhen::default() };
        let targets = vec![
            SaveTarget {
                id: "pdf".into(),
                spec: SaveTargetSpec { suggest: pdfs, subfolders: "{sender}/{year}".into(), ..spec("PDFs", folder) },
            },
            SaveTarget {
                id: "named".into(),
                spec: SaveTargetSpec { file_name: "{date} {name}.{ext}".into(), ..spec("Named", folder) },
            },
        ];
        let months = month_names();
        let pdf = export_plan(&targets, &summary(), ExportFormat::Pdf, "Privat", &months);
        assert_eq!((pdf.format, pdf.file_name.as_str()), (ExportFormat::Pdf, "2026-03-28 Ihre Rechnung.pdf"));
        assert_eq!(pdf.places[0].folders, ["Stadtwerke Bonn", "2026"]);
        assert_eq!(pdf.places[0].file, "Ihre Rechnung.pdf", "the subject stands for the name");
        assert_eq!(pdf.places[1].file, "2026-03-28 Ihre Rechnung.pdf");
        assert_eq!(pdf.suggested, ["pdf"]);
        let png = export_plan(&targets, &summary(), ExportFormat::Png, "Privat", &months);
        assert_eq!(png.file_name, "2026-03-28 Ihre Rechnung.png");
        assert!(png.suggested.is_empty(), "a picture is no PDF");
        let mut targets = targets;
        targets.push(SaveTarget {
            id: "sender".into(),
            spec: SaveTargetSpec {
                suggest: SuggestWhen { senders: vec!["stadtwerke-bonn.de".into()], ..SuggestWhen::default() },
                ..spec("Sender", folder)
            },
        });
        let eml = export_plan(&targets, &summary(), ExportFormat::Eml, "Privat", &months);
        assert_eq!(eml.file_name, "2026-03-28 Ihre Rechnung.eml");
        assert_eq!(eml.places[0].file, "Ihre Rechnung.eml");
        assert_eq!(eml.suggested, ["sender"], "EML matches sender rules, never PDF/image type rules");

        let odd = MessageSummary { subject: format!("  Re: a/b\n{}", "x".repeat(80)), ..summary() };
        let named = export_plan(&[], &odd, ExportFormat::Pdf, "Privat", &months).file_name;
        assert!(named.starts_with("2026-03-28 Re_ a_b x") && named.ends_with(".pdf"), "{named}");
        assert!(named.chars().count() <= "2026-03-28 ".len() + MAX_SUBJECT_CHARS + ".pdf".len());
        let blank = MessageSummary { subject: " ".into(), ..summary() };
        let named = export_plan(&[], &blank, ExportFormat::Png, "Privat", &months).file_name;
        assert_eq!(named, format!("2026-03-28 {}.png", tr("Mail")));
    }

    /// Issue #501, the whole way through the service: the plans come with the targets, the mail is
    /// drawn and saved to a target after its schemes, a second save is numbered and with "Skip if
    /// identical" found there; a client without the account finds no mail.
    #[tokio::test]
    async fn a_mail_is_saved_as_pdf_and_picture() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &raw_with(&[]), &[]);
        let service = synced(dir.path(), &server).await;
        let id = message_id(&service, "Ihre Rechnung").await;
        let base = dir.path().join("Rechnungen");
        fs::create_dir(&base).expect("the folder");
        let downloads = dir.path().join("Downloads");
        fs::create_dir(&downloads).expect("downloads");
        let context = Context { downloads: Some(downloads.clone()), categories: Category::BUILT_IN.to_vec() };
        let target = SaveTargetSpec { subfolders: "{year}".into(), ..spec("Invoices", &base) };
        let made = service.save_targets().create(target, &context.categories).expect("stored");

        let listed = answer(
            &service,
            Reach::Suite,
            &context,
            Request::ListSaveTargets { id: Some(id.clone()), export_only: false },
        )
        .await;
        let Response::SaveTargets { exports, .. } = listed else { panic!("targets: {listed:?}") };
        assert_eq!(exports.iter().map(|plan| plan.format).collect::<Vec<_>>(), [ExportFormat::Pdf, ExportFormat::Png]);
        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(rename_all = "lowercase")]
        enum OldFormat {
            Pdf,
            Png,
        }
        #[derive(Deserialize)]
        struct OldPlan {
            format: OldFormat,
        }
        let old: Vec<OldPlan> = serde_json::from_value(serde_json::to_value(&exports).expect("wire"))
            .expect("pre-EML client reads legacy plans");
        assert_eq!(old.into_iter().map(|p| p.format).collect::<Vec<_>>(), [OldFormat::Pdf, OldFormat::Png]);
        assert_eq!(exports[0].places[0].file, "Ihre Rechnung.pdf");

        let to_target = Request::SaveMailAs { id: id.clone(), format: ExportFormat::Pdf, to: SaveTo::Target(made.id) };
        let saved = answer(&service, Reach::Suite, &context, to_target.clone()).await;
        let Response::SavedMail { path, skipped: false, shortened: false } = saved else { panic!("saved: {saved:?}") };
        let folder = base.canonicalize().expect("the folder").join("2026");
        assert_eq!(PathBuf::from(&path), folder.join("Ihre Rechnung.pdf"));
        assert!(fs::read(&path).expect("written").starts_with(b"%PDF"));
        let again = answer(&service, Reach::Suite, &context, to_target).await;
        let Response::SavedMail { path, .. } = again else { panic!("saved: {again:?}") };
        assert_eq!(PathBuf::from(&path), folder.join("Ihre Rechnung (2).pdf"), "never overwritten");

        let picture = Request::SaveMailAs { id: id.clone(), format: ExportFormat::Png, to: SaveTo::Downloads };
        let saved = answer(&service, Reach::Suite, &context, picture).await;
        let Response::SavedMail { path, .. } = saved else { panic!("saved: {saved:?}") };
        assert_eq!(PathBuf::from(&path), downloads.join("2026-03-28 Ihre Rechnung.png"));
        assert!(fs::read(&path).expect("written").starts_with(b"\x89PNG"));

        let gone = Request::SaveMailAs { id: id.clone(), format: ExportFormat::Pdf, to: SaveTo::Target("nope".into()) };
        assert_eq!(code(&answer(&service, Reach::Suite, &context, gone).await), "not-found");
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.mail_accounts = Selection::Only(Vec::new());
        let save = Request::SaveMailAs { id, format: ExportFormat::Pdf, to: SaveTo::Downloads };
        assert_eq!(code(&answer(&service, Reach::Client(&rights), &context, save).await), "not-found");
        assert_eq!(fs::read_dir(&downloads).expect("readable").count(), 1, "nothing more was saved");
    }

    /// Issue #501, Erledigt 3 and 4 for HTML: an HTML mail goes through the service drawn as HTML, with
    /// the picture it carries itself (`cid:`) taken from its own parts – a red square in the picture.
    #[tokio::test]
    async fn an_html_mail_is_drawn_with_its_own_pictures() {
        use base64::Engine;
        let mut logo = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut logo, 40, 40);
            encoder.set_color(png::ColorType::Rgb);
            let mut writer = encoder.write_header().expect("a header");
            writer.write_image_data(&[220u8, 20, 20].repeat(40 * 40)).expect("red");
        }
        let logo = base64::engine::general_purpose::STANDARD.encode(&logo);
        let date = Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).single().expect("a date").to_rfc2822();
        let raw = format!(
            "From: Stadtwerke Bonn <billing@stadtwerke-bonn.de>\r\nTo: anna@example.org\r\nSubject: Newsletter\r\n\
             Date: {date}\r\nMessage-ID: <html@example.org>\r\nMIME-Version: 1.0\r\n\
             Content-Type: multipart/related; boundary=\"r1\"\r\n\r\n--r1\r\n\
             Content-Type: text/html; charset=utf-8\r\n\r\n\
             <html><body><h1>Hallo</h1><img src=\"cid:logo@example.org\" width=\"40\" height=\"40\"></body></html>\r\n\
             --r1\r\nContent-Type: image/png\r\nContent-ID: <logo@example.org>\r\n\
             Content-Disposition: inline\r\nContent-Transfer-Encoding: base64\r\n\r\n{logo}\r\n--r1--\r\n"
        );
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &raw, &[]);
        let service = synced(dir.path(), &server).await;
        let id = message_id(&service, "Newsletter").await;
        let downloads = dir.path().join("Downloads");
        fs::create_dir(&downloads).expect("downloads");
        let context = Context { downloads: Some(downloads), categories: Category::BUILT_IN.to_vec() };
        let save = Request::SaveMailAs { id, format: ExportFormat::Png, to: SaveTo::Downloads };
        let saved = answer(&service, Reach::Suite, &context, save).await;
        let Response::SavedMail { path, .. } = saved else { panic!("saved: {saved:?}") };
        let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(&path).expect("written")));
        let mut reader = decoder.read_info().expect("a PNG");
        let mut pixels = vec![0; reader.output_buffer_size().expect("a size")];
        let frame = reader.next_frame(&mut pixels).expect("pixels");
        let red =
            pixels[..frame.buffer_size()].chunks_exact(3).filter(|p| p[0] > 180 && p[1] < 60 && p[2] < 60).count();
        assert!(red > 1000, "the picture of the mail is drawn: {red} red pixels");
    }

    /// Originals remain byte-identical, including unknown/folded headers, nested alternative/related
    /// parts and binary attachments. Export preview is strictly metadata-only and opt-in.
    #[tokio::test]
    async fn eml_exports_exact_mime_without_body_preview_or_cache_writes() {
        let _serial = super::super::eml::TESTS.lock().await;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let text = raw_with(&[]);
        let mixed = raw_with(&[("binary.dat", &[0, 255, 13, 10, 128])]);
        let related = concat!(
            "From: sender@example.org\r\nTo: anna@example.org\r\nSubject: Related\r\n",
            "Message-ID: <related@example.org>\r\nX-Extra: untouched\r\n\tfolded value\r\n",
            "MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=m\r\n\r\n",
            "--m\r\nContent-Type: multipart/alternative; boundary=a\r\n\r\n",
            "--a\r\nContent-Type: text/plain\r\n\r\nPlain text\r\n",
            "--a\r\nContent-Type: multipart/related; boundary=r\r\n\r\n",
            "--r\r\nContent-Type: text/html\r\n\r\n<p>HTML<img src=\"cid:picture\"></p>\r\n",
            "--r\r\nContent-Type: image/png\r\nContent-ID: <picture>\r\nContent-Disposition: inline\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\nAAECAw==\r\n--r--\r\n--a--\r\n",
            "--m\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=binary.dat\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\nAP8NCoA=\r\n--m--\r\n",
        );
        let bodies = [text.as_str(), mixed.as_str(), related];
        let uids: Vec<_> = bodies.iter().map(|raw| server.add("INBOX", raw, &[])).collect();
        let service = synced(dir.path(), &server).await;
        let store = service.stores(Reach::Suite).await.remove(0).1;
        let downloads = dir.path().join("exports");
        fs::create_dir(&downloads).expect("folder");
        let context = Context { downloads: Some(downloads.clone()), categories: Category::BUILT_IN.to_vec() };
        let places = store
            .run(move |store| {
                let folder = store.folder_by_role(torro_mail::Role::Inbox)?.expect("inbox");
                let mut places = Vec::new();
                for uid in uids {
                    let id = store
                        .list_messages(&folder.id, 20, 0)?
                        .into_iter()
                        .find(|m| store.resolve_message(&m.id).ok().flatten().is_some_and(|p| p.uid == uid))
                        .expect("message")
                        .id;
                    places.push(store.resolve_message(&id)?.expect("place"));
                }
                Ok(places)
            })
            .await
            .expect("places");
        for (place, raw) in places.iter().zip(bodies) {
            let lines = server.lines().len();
            let preview = answer(
                &service,
                Reach::Suite,
                &context,
                Request::ListSaveTargets { id: Some(place.id.clone()), export_only: true },
            )
            .await;
            let Response::SaveTargets { files, exports, .. } = preview else { panic!("{preview:?}") };
            assert!(files.is_empty());
            assert_eq!(exports.iter().map(|plan| plan.format).collect::<Vec<_>>(), ExportFormat::ALL);
            // Released command sessions may finish their IDLE/LOGOUT housekeeping concurrently.
            // Count body/export fetches, not unrelated connection lifecycle lines.
            let later = server.lines()[lines..].to_vec();
            assert!(
                !later
                    .iter()
                    .any(|line| line.contains("BODY.PEEK[]") || line.contains("(UID INTERNALDATE RFC822.SIZE)")),
                "preview performs no source fetches: {later:?}"
            );
            let before = read::stored_message(&read::Located {
                account: ANNA.into(),
                store: store.clone(),
                place: place.clone(),
            })
            .await
            .expect("before");
            let save = Request::SaveMailAs { id: place.id.clone(), format: ExportFormat::Eml, to: SaveTo::Downloads };
            let saved = answer(&service, Reach::Suite, &context, save.clone()).await;
            let Response::SavedMail { path, skipped: false, shortened: false } = saved else { panic!("{saved:?}") };
            let bytes = fs::read(&path).expect("saved original");
            assert_eq!(bytes, raw.as_bytes());
            assert!(Path::new(&path).extension().is_some_and(|ext| ext == "eml"));
            let parsed = mime::read(&bytes).expect("standard MIME readback");
            if raw == related {
                assert!(parsed.html.is_some());
                let attachment = parsed.attachments.iter().find(|a| !a.inline).expect("binary attachment");
                assert_eq!(mime::attachment(&bytes, &attachment.part).expect("decoded").bytes, [0, 255, 13, 10, 128]);
                let header = b"X-Extra: untouched\r\n\tfolded value\r\n";
                assert!(bytes.windows(header.len()).any(|s| s == header), "unknown folded header remains");
            }
            let after = read::stored_message(&read::Located {
                account: ANNA.into(),
                store: store.clone(),
                place: place.clone(),
            })
            .await
            .expect("after");
            assert_eq!(before, after, "export does not update reading cache");
            assert!(server.flags("INBOX", place.uid).is_empty(), "PEEK never marks read");
            let target = SaveTargetSpec {
                on_conflict: OnConflict::SkipIdentical,
                ..spec(&format!("Original {}", place.uid), &downloads)
            };
            let target = service.save_targets().create(target, &context.categories).expect("target");
            let to = SaveTo::Target(target.id);
            let request = Request::SaveMailAs { id: place.id.clone(), format: ExportFormat::Eml, to };
            let first = answer(&service, Reach::Suite, &context, request.clone()).await;
            let Response::SavedMail { path: target_path, skipped: false, shortened: false } = first else {
                panic!("{first:?}")
            };
            assert_eq!(fs::read(&target_path).expect("target bytes"), raw.as_bytes());
            let second = answer(&service, Reach::Suite, &context, request).await;
            assert_eq!(second, Response::SavedMail { path: target_path, skipped: true, shortened: false });
            let located = read::Located { account: ANNA.into(), store: store.clone(), place: place.clone() };
            // A locally retained original works even after it has disappeared from the server.
            let local_place = place.clone();
            let original = raw.as_bytes().to_vec();
            store
                .run(move |store| {
                    store.save_original(
                        &local_place.folder_path,
                        local_place.uidvalidity,
                        local_place.uid,
                        Utc::now().fixed_offset(),
                        &[],
                        &original,
                    )
                })
                .await
                .expect("local original");
            server.expunge("INBOX", place.uid);
            let commands = server.lines().len();
            let captured = super::super::eml::prepare(&service, &located).await.expect("offline source");
            let busy = answer(&service, Reach::Suite, &context, save.clone()).await;
            assert_eq!(code(&busy), "busy");
            let job = Job::New {
                dir: downloads.clone(),
                below: None,
                name: "Original.eml".into(),
                rule: OnConflict::KeepBoth,
            };
            let written = job.clone().run_file(captured).expect("published");
            assert!(matches!(written, Written::Saved(_)));
            let captured = super::super::eml::prepare(&service, &located).await.expect("local again");
            let written = Job::New {
                dir: downloads.clone(),
                below: None,
                name: "Original.eml".into(),
                rule: OnConflict::SkipIdentical,
            }
            .run_file(captured)
            .expect("skip identical");
            assert!(matches!(written, Written::Skipped(_)));
            let later = server.lines()[commands..].to_vec();
            assert!(
                !later
                    .iter()
                    .any(|line| line.contains("BODY.PEEK[]") || line.contains("(UID INTERNALDATE RFC822.SIZE)")),
                "local export never fetches: {later:?}"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(fs::metadata(path).expect("mode").permissions().mode() & 0o777, 0o600);
            }
            // Use another name for the next fixture's direct Job publication.
            fs::remove_file(downloads.join("Original.eml")).expect("test file");
        }
    }

    #[tokio::test]
    async fn eml_name_conflicts_preserve_old_files_and_clean_up_when_all_names_are_taken() {
        let _serial = super::super::eml::TESTS.lock().await;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let raw = raw_with(&[]);
        server.add("INBOX", &raw, &[]);
        let service = synced(dir.path(), &server).await;
        let id = message_id(&service, "Ihre Rechnung").await;
        let located = read::locate(&service.stores(Reach::Suite).await, &id).await.expect("located");
        for rule in [OnConflict::KeepBoth, OnConflict::SkipIdentical] {
            let exports = tempfile::tempdir().expect("exports");
            let job = Job::New { dir: exports.path().into(), below: None, name: "Original.eml".into(), rule };
            let captured = super::super::eml::prepare(&service, &located).await.expect("capture");
            // Names become occupied after destination planning; no existence check may clobber them.
            for copy in 1..=2 {
                fs::write(exports.path().join(numbered("Original.eml", copy)), b"old content").expect("old");
            }
            let path = exports.path().join("Original (3).eml");
            assert_eq!(job.clone().run_file(captured), Ok(Written::Saved(path.clone())));
            assert_eq!(fs::read(path).expect("new original"), raw.as_bytes());
            for copy in 1..=2 {
                assert_eq!(fs::read(exports.path().join(numbered("Original.eml", copy))).expect("old"), b"old content");
            }
            assert_eq!(fs::read_dir(exports.path()).expect("files").count(), 3);
            for copy in 3..=MAX_COPY {
                fs::write(exports.path().join(numbered("Original.eml", copy)), b"old content").expect("occupied");
            }
            let captured = super::super::eml::prepare(&service, &located).await.expect("capture");
            assert_eq!(job.run_file(captured), Err(Problem::Taken));
            assert_eq!(fs::read_dir(exports.path()).expect("files").count(), MAX_COPY as usize, "no temporary remains");
            for copy in 1..=MAX_COPY {
                assert_eq!(
                    fs::read(exports.path().join(numbered("Original.eml", copy))).expect("unchanged"),
                    b"old content"
                );
            }
        }
    }

    #[test]
    fn eml_atomic_publication_numbers_concurrent_writers_without_clobbering() {
        use std::io::Write;
        use std::sync::Barrier;

        let dir = tempfile::tempdir().expect("exports");
        let start = Barrier::new(2);
        std::thread::scope(|threads| {
            let writers: Vec<_> = [b"first original".as_slice(), b"second original".as_slice()]
                .into_iter()
                .map(|bytes| {
                    let (dir, start) = (dir.path(), &start);
                    threads.spawn(move || {
                        let mut temporary = tempfile::NamedTempFile::new_in(dir).expect("temporary");
                        temporary.write_all(bytes).expect("destination bytes");
                        let mut source = tempfile::tempfile().expect("source");
                        source.write_all(bytes).expect("source bytes");
                        start.wait();
                        let written = publish_new(
                            temporary,
                            dir,
                            "Original.eml",
                            OnConflict::KeepBoth,
                            &mut source,
                            bytes.len() as u64,
                        )
                        .expect("published");
                        let Written::Saved(path) = written else { panic!("not saved") };
                        assert_eq!(fs::read(&path).expect("own bytes"), bytes);
                        path
                    })
                })
                .collect();
            let mut paths: Vec<_> = writers.into_iter().map(|writer| writer.join().expect("writer")).collect();
            paths.sort();
            let mut expected = vec![dir.path().join("Original.eml"), dir.path().join("Original (2).eml")];
            expected.sort();
            assert_eq!(paths, expected);
        });
        assert_eq!(fs::read_dir(dir.path()).expect("files").count(), 2, "no temporary remains");
    }

    #[tokio::test]
    async fn eml_incomplete_sources_and_failed_publication_leave_no_files() {
        let _serial = super::super::eml::TESTS.lock().await;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let raw = raw_with(&[("a.dat", b"data")]);
        let uid = server.add("INBOX", &raw, &[]);
        let service = synced(dir.path(), &server).await;
        let id = message_id(&service, "Ihre Rechnung").await;
        let located = read::locate(&service.stores(Reach::Suite).await, &id).await.expect("located");
        let exports = dir.path().join("exports");
        fs::create_dir(&exports).expect("folder");
        let context = Context { downloads: Some(exports.clone()), categories: Category::BUILT_IN.to_vec() };
        let request = Request::SaveMailAs { id: id.clone(), format: ExportFormat::Eml, to: SaveTo::Downloads };
        for claimed in [0, raw.len() - 1, raw.len() + 1, super::super::store::ORIGINAL_MAX_BYTES as usize + 1] {
            server.misreport_size("INBOX", uid, claimed);
            assert_eq!(code(&answer(&service, Reach::Suite, &context, request.clone()).await), "error");
            assert_eq!(fs::read_dir(&exports).expect("files").count(), 0);
        }
        server.misreport_size("INBOX", uid, raw.len());
        // Corrupt or unprovable local originals are tried once on the server, never reconstructed.
        let db = located.store.run(|store| Ok(store.path().to_path_buf())).await.expect("db path");
        for damage in [
            "UPDATE originals SET sha256 = 'broken'",
            "UPDATE originals SET raw = x'61'",
            "UPDATE originals SET bytes = bytes - 1",
            "UPDATE messages SET size = NULL",
        ] {
            let place = located.place.clone();
            let original = raw.as_bytes().to_vec();
            located
                .store
                .run(move |store| {
                    store.set_size(&place.folder, place.uid, original.len() as u64)?;
                    store.save_original(
                        &place.folder_path,
                        place.uidvalidity,
                        place.uid,
                        Utc::now().fixed_offset(),
                        &[],
                        &original,
                    )
                })
                .await
                .expect("local original");
            rusqlite::Connection::open(&db).expect("db").execute(damage, []).expect("damage fixture");
            let lines = server.lines().len();
            let original = super::super::eml::prepare(&service, &located).await.expect("server fallback");
            assert!(server.lines()[lines..].iter().any(|line| line.contains("BODY.PEEK[]<")));
            drop(original);
        }
        server.withhold_size("INBOX", uid);
        assert_eq!(code(&answer(&service, Reach::Suite, &context, request.clone()).await), "error");
        // Restore a proven local original, then fail destination writes/replacement.
        let place = located.place.clone();
        let bytes = raw.as_bytes().to_vec();
        located
            .store
            .run(move |store| {
                store.set_size(&place.folder, place.uid, bytes.len() as u64)?;
                store.save_original(
                    &place.folder_path,
                    place.uidvalidity,
                    place.uid,
                    Utc::now().fixed_offset(),
                    &[],
                    &bytes,
                )
            })
            .await
            .expect("proven original");
        let old = exports.join("old.eml");
        fs::write(&old, b"old content").expect("old");
        let prepared = super::super::eml::prepare(&service, &located).await.expect("capture");
        prepared.file.set_len(1).expect("truncate spool");
        assert!(Job::Replace(old.clone()).run_file(prepared).is_err());
        assert_eq!(fs::read(&old).expect("old intact"), b"old content");
        let captured = super::super::eml::prepare(&service, &located).await.expect("capture");
        assert!(
            Job::New { dir: exports.join("missing"), below: None, name: "a.eml".into(), rule: OnConflict::KeepBoth }
                .run_file(captured)
                .is_err()
        );
        let captured = super::super::eml::prepare(&service, &located).await.expect("capture");
        Job::Replace(old.clone()).run_file(captured).expect("regular replacement");
        assert_eq!(fs::read(&old).expect("replaced"), raw.as_bytes());
        #[cfg(unix)]
        {
            let link = exports.join("link.eml");
            std::os::unix::fs::symlink(&old, &link).expect("link");
            let captured = super::super::eml::prepare(&service, &located).await.expect("capture");
            assert_eq!(Job::Replace(link.clone()).run_file(captured), Err(Problem::NotReplaceable));
            fs::remove_file(link).expect("link removed");
        }
        assert_eq!(fs::read_dir(&exports).expect("files").count(), 1, "no temporary/partial files remain");
        // A damaged disposable reader body does not prevent summary-only preview, approval or EML.
        let body = super::super::store::Body::of(mime::read(raw.as_bytes()).expect("mime"), None);
        let body_id = id.clone();
        located.store.run(move |store| store.set_body(&body_id, &body)).await.expect("reading body");
        rusqlite::Connection::open(&db)
            .expect("db")
            .execute("UPDATE bodies SET attachments_json = 'broken'", [])
            .expect("corrupt reading metadata");
        let preview = answer(
            &service,
            Reach::Suite,
            &context,
            Request::ListSaveTargets { id: Some(id.clone()), export_only: true },
        )
        .await;
        assert!(matches!(preview, Response::SaveTargets { .. }));
        assert!(message_to_approve(&service, Reach::Suite, &id).await.is_ok());
        assert!(matches!(answer(&service, Reach::Suite, &context, request).await, Response::SavedMail { .. }));
    }

    #[tokio::test]
    async fn eml_rechecks_server_facts_and_mapping_but_not_flags() {
        let _serial = super::super::eml::TESTS.lock().await;
        for change in ["flags", "size", "validity", "mapping", "gone"] {
            let dir = tempfile::tempdir().expect("temp dir");
            let server = FakeImap::start(PASSWORD).await;
            let raw = raw_with(&[]);
            let uid = server.add("INBOX", &raw, &[]);
            let service = synced(dir.path(), &server).await;
            let id = message_id(&service, "Ihre Rechnung").await;
            let context =
                Context { downloads: Some(dir.path().join("exports")), categories: Category::BUILT_IN.to_vec() };
            fs::create_dir(context.downloads.as_ref().expect("folder")).expect("exports");
            let stores = service.stores(Reach::Suite).await;
            let located = read::locate(&stores, &id).await.expect("located");
            if change == "gone" {
                server.withhold_text("INBOX", uid);
                let response = answer(
                    &service,
                    Reach::Suite,
                    &context,
                    Request::SaveMailAs { id, format: ExportFormat::Eml, to: SaveTo::Downloads },
                )
                .await;
                assert_eq!(code(&response), "not-found");
                continue;
            }
            let already = server.lines().iter().filter(|line| line.contains("BODY.PEEK[]<")).count();
            server.hold_backup_chunks(true);
            let save = answer(
                &service,
                Reach::Suite,
                &context,
                Request::SaveMailAs { id, format: ExportFormat::Eml, to: SaveTo::Downloads },
            );
            let mutate = async {
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while server.lines().iter().filter(|line| line.contains("BODY.PEEK[]<")).count() == already {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("export reaches gated body");
                match change {
                    "flags" => server.set_flags("INBOX", uid, &["\\Flagged"]),
                    "size" => server.misreport_size("INBOX", uid, raw.len() + 1),
                    "validity" => server.renumber("INBOX"),
                    "mapping" => {
                        let place = located.place.clone();
                        located
                            .store
                            .run(move |store| store.delete_uids(&place.folder, &[place.uid]))
                            .await
                            .expect("gone from cache");
                    }
                    _ => unreachable!(),
                }
                server.hold_backup_chunks(false);
            };
            let (response, ()) = tokio::join!(save, mutate);
            match change {
                "flags" => assert!(matches!(response, Response::SavedMail { .. }), "{response:?}"),
                "validity" => assert_eq!(code(&response), "not-found"),
                _ => assert_eq!(code(&response), "conflict"),
            }
            if change != "flags" {
                assert_eq!(fs::read_dir(context.downloads.as_ref().expect("folder")).expect("files").count(), 0);
            }
        }
    }

    /// Issue #501: "Skip if identical" finds the same drawing of the mail there and writes nothing, as
    /// PDF and as picture.
    #[tokio::test]
    async fn the_same_mail_is_skipped_in_a_target_that_skips_identical_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &raw_with(&[]), &[]);
        let service = synced(dir.path(), &server).await;
        let id = message_id(&service, "Ihre Rechnung").await;
        let context = Context { downloads: None, categories: Category::BUILT_IN.to_vec() };
        let target = SaveTargetSpec { on_conflict: OnConflict::SkipIdentical, ..spec("Skip", dir.path()) };
        let made = service.save_targets().create(target, &context.categories).expect("stored");
        for format in [ExportFormat::Pdf, ExportFormat::Png] {
            let save = Request::SaveMailAs { id: id.clone(), format, to: SaveTo::Target(made.id.clone()) };
            let first = answer(&service, Reach::Suite, &context, save.clone()).await;
            let Response::SavedMail { path, skipped: false, .. } = first else { panic!("saved: {first:?}") };
            let second = answer(&service, Reach::Suite, &context, save).await;
            assert_eq!(second, Response::SavedMail { path, skipped: true, shortened: false }, "{format:?}");
        }
    }

    /// The store drops the text of an older mail again (`prune_bodies_for`: at once with headers only,
    /// else after `BODY_RETENTION_DAYS`), so the attachments come from the message fetched once: the
    /// plans and the approval know them, and a part the mail does not have is refused before anyone is
    /// asked. When the message cannot be fetched there are no plans, and the targets come all the same.
    #[tokio::test]
    async fn a_mail_without_its_text_is_planned_from_the_server() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let uid = server.add("INBOX", &raw_with(&[("Rechnung.pdf", b"pdf"), ("Liste.pdf", b"list")]), &[]);
        let service = synced(dir.path(), &server).await;
        let id = message_id(&service, "Ihre Rechnung").await;
        let stores = service.stores(Reach::Suite).await;
        let stored = read::stored_message(&read::locate(&stores, &id).await.expect("found")).await.expect("stored");
        assert!(stored.text.is_none(), "the text of an old mail is not kept");
        let context = Context { downloads: None, categories: Category::BUILT_IN.to_vec() };
        let made = service.save_targets().create(spec("Invoices", dir.path()), &context.categories).expect("stored");
        let list = Request::ListSaveTargets { id: Some(id.clone()), export_only: false };

        let listed = answer(&service, Reach::Suite, &context, list.clone()).await;
        let Response::SaveTargets { files, .. } = listed else { panic!("targets: {listed:?}") };
        let names: Vec<&str> = files.iter().map(|plan| plan.file_name.as_str()).collect();
        assert_eq!(names, ["Rechnung.pdf", "Liste.pdf"]);
        assert_eq!(files[0].places[0].target, made.id);
        let all = to_approve(&service, Reach::Suite, &id, &[]).await.expect("in reach");
        assert_eq!((all.subject.as_str(), all.count), ("Ihre Rechnung", 2));
        let unknown = to_approve(&service, Reach::Suite, &id, &["9".into()]).await.expect_err("no such part");
        assert_eq!(code(&unknown), "not-found");

        server.expunge("INBOX", uid);
        let listed = answer(&service, Reach::Suite, &context, list).await;
        let Response::SaveTargets { targets, files, .. } = listed else { panic!("targets: {listed:?}") };
        assert_eq!((targets, files), (vec![made], Vec::new()));
        // Parts named while the mail cannot be read are no number in front of a person.
        let named: Vec<String> = (1..=30).map(|n| n.to_string()).collect();
        // The failed fetch may also tell the store the mail is gone (the server's EXPUNGE, depending
        // on timing); then it is `not-found`, which is right as well.
        match to_approve(&service, Reach::Suite, &id, &named).await {
            Ok(gone) => assert_eq!(gone.count, 0, "nothing known to count, no count from the request"),
            Err(gone) => assert_eq!(code(&gone), "not-found"),
        }
    }
}
