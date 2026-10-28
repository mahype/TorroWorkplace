//! The audit log (`docs/api.md`, "Protokoll", task A.5): `audit.jsonl` in the core's state
//! directory, mode 0600, one JSON line ([`ActivityEntry`]) per request of a client key and per run
//! of a command a person allowed. It says who did what to which ids and how it ended, so a person
//! can follow what an agent did (`clients.activity`).
//!
//! **No contents, ever.** A line holds the time, the client id, the command's catalogue name, the
//! ids the command named ([`ids`]) and the result word ([`outcome`]). Never titles, names, search
//! terms, bodies, thread keys, approval summaries, requests or answers: those hold what the
//! person wrote (an answer of `calendar.list_events` is the calendar; an approval's summary and
//! its stored answer carry titles; inbox rule 7 keeps titles, bodies and thread keys out of every
//! log). Requests of the app key are not logged: that would be every move in the control centre.
//!
//! The file is rotated at 5 MB ([`ROTATE_AT`]): it becomes `audit.jsonl.1`, replacing an older
//! one, so the log never takes more than about twice that.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_ipc::assistant::{ChatWindow, TEST_ENGINE};
use torro_ipc::extensions::{self, InstallFrom};
use torro_ipc::mail::MoveTarget;
use torro_ipc::{ActivityEntry, Request, Response};

/// The log's name in the state directory.
pub const FILE: &str = "audit.jsonl";

/// The log is rotated before a line would make it larger than this.
pub const ROTATE_AT: u64 = 5 * 1024 * 1024;

/// At most this many entries answer `clients.activity`.
pub const ACTIVITY_MAX: usize = 50;

/// An id longer than this is cut: ids come from the client, and a line must stay a line.
const ID_MAX: usize = 100;

/// Why the log could not be read. Says no path: the answer goes to a client; the path is only
/// printed to the core's own log.
#[derive(Debug)]
pub enum AuditError {
    Io(io::Error),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditError::Io(err) => write!(f, "cannot read the audit log: {err}"),
        }
    }
}

impl std::error::Error for AuditError {}

impl AuditError {
    /// The answer to the app key: code `error`.
    pub fn response(&self) -> Response {
        Response::Error { code: "error".into(), message: self.to_string() }
    }
}

/// The audit log of the core.
pub struct Audit {
    /// `audit.jsonl`; `None` without a home directory and in tests that pass none: then nothing
    /// is written and the activity is empty.
    file: Option<PathBuf>,
    /// [`ROTATE_AT`]; tests pass a small one.
    rotate_at: u64,
    /// One write (and rotation) at a time, so lines never interleave and none is lost to a
    /// rename in between.
    writing: Mutex<()>,
}

impl Audit {
    /// The user's log: `audit.jsonl` in the core's state directory.
    ///
    /// In tests there is none, so a test that forgets to pass its own
    /// ([`crate::core::Core::with_audit`]) never writes into the user's state directory.
    pub fn system() -> Audit {
        if cfg!(test) {
            return Audit::new(None, ROTATE_AT);
        }
        Audit::new(AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join(FILE)), ROTATE_AT)
    }

    /// A log in `file`, rotated before it would grow past `rotate_at` bytes.
    pub fn new(file: Option<PathBuf>, rotate_at: u64) -> Audit {
        Audit { file, rotate_at, writing: Mutex::new(()) }
    }

    /// Appends `entry` as one line. Never fails the request it describes: a log that cannot be
    /// written is reported on stderr, and the request's answer goes out all the same.
    pub fn record(&self, entry: &ActivityEntry) {
        let Some(file) = &self.file else { return };
        let mut line = match serde_json::to_vec(entry) {
            Ok(line) => line,
            Err(err) => return eprintln!("torroworkplace: cannot write the audit log: {err}"),
        };
        line.push(b'\n');
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(err) = append(file, &line, self.rotate_at) {
            eprintln!("torroworkplace: cannot write the audit log {}: {err}", file.display());
        }
    }

    /// The latest entries of `client`, newest first, at most [`ACTIVITY_MAX`]: from the log and,
    /// when that holds too few, from the rotated one. A log that is not there yet is empty; a
    /// line that does not parse is skipped.
    pub fn recent(&self, client: &str) -> Result<Vec<ActivityEntry>, AuditError> {
        let Some(file) = &self.file else { return Ok(Vec::new()) };
        let mut found = Vec::new();
        for path in [file.clone(), rotated(file)] {
            let mut older = read_client(&path, client).map_err(|err| {
                eprintln!("torroworkplace: cannot read the audit log {}: {err}", path.display());
                AuditError::Io(err)
            })?;
            older.reverse();
            found.extend(older);
            if found.len() >= ACTIVITY_MAX {
                break;
            }
        }
        found.truncate(ACTIVITY_MAX);
        Ok(found)
    }
}

/// The line about a request of `client` (`None`: an unknown key) that got `answer`, for `ids`
/// taken before the request ran ([`ids`]).
pub fn entry(
    client: Option<String>,
    command: &str,
    ids: Vec<String>,
    answer: &Response,
    time: DateTime<Utc>,
) -> ActivityEntry {
    let (result, approval) = outcome(answer);
    ActivityEntry { time, client, command: command.to_string(), ids, result, approval }
}

/// The result word of `answer` and the approval it names: `pending` with the approval it waits
/// for, the error code of an error, `untrusted-certificate` for a sign-in that stopped at a
/// certificate (issue #284: nothing ran, so it is no `ok`), otherwise `ok`. Only the code of an
/// error, never its message, which can quote what the client sent; never the host or fingerprint
/// of a certificate either.
pub fn outcome(answer: &Response) -> (String, Option<String>) {
    match answer {
        Response::Pending { approval } => ("pending".into(), Some(approval.id.clone())),
        Response::Error { code, .. } => (code.clone(), None),
        Response::UntrustedCertificate { .. } => ("untrusted-certificate".into(), None),
        _ => ("ok".into(), None),
    }
}

/// The ids `request` names that may go into the log, each as `kind:value`
/// ([`ActivityEntry::ids`]).
///
/// What counts as safe:
///
/// - **As sent, if they look the part:** ids that carry no content by themselves. Inbox entries,
///   rules, approvals and clients are random hex digits the suite made ([`is_random_id`]);
///   accounts are `<kind>:<name>` ([`is_account_id`]). All of them come from the client, which can
///   put any text there, so one that does not have its form is hashed instead (task E1.2c). Also
///   a source, a setting's key (never its value), and an app only when it is one of the suite's.
/// - **Partly hashed:** a collection id keeps its account and hashes the rest
///   (`collection:<account>/<hash>`, [`collection`]): the rest is the calendar's URL on the
///   server, with server, user and the calendar's name in it.
/// - **Hashed** ([`hashed`]): what can be made from a name. CalDAV hrefs are often a UUID, but
///   other programs name them after the title (`Dinner-with-Anna.ics`); issue URLs name the
///   repository; template ids are made from the template's name (`dev-morning`).
/// - **Never:** titles, names, search terms, bodies, thread keys, server addresses (a URL can
///   hold a user name), tokens, settings' values, time ranges of a query.
///
/// A hashed id that could also have been sent as it is says so (`inbox:hash:<hash>`), so it is
/// never taken for an id of that form. The match has no catch-all on purpose: a new request must
/// get its rule here.
pub fn ids(request: &Request) -> Vec<String> {
    let id = |kind: &str, value: &str| format!("{kind}:{}", cut(value));
    // An id from the client as it is when it has its form, else hashed.
    let checked = |kind: &str, value: &str, form: fn(&str) -> bool| match form(value) {
        true => id(kind, value),
        false => format!("{kind}:hash:{}", hashed(value)),
    };
    let random = |kind: &str, value: &str| checked(kind, value, is_random_id);
    let account = |value: &str| checked("account", value, is_account_id);
    let item = |href: &Option<String>| href.as_deref().map(|href| format!("item:{}", hashed(href)));
    match request {
        Request::Ping
        | Request::Status
        | Request::Quit
        | Request::ListTemplates
        | Request::ListScreens
        | Request::SaveTemplate { .. }
        | Request::ConnectNextcloud { .. }
        | Request::ConnectGithub
        | Request::SetGithubToken { .. }
        // Never the address searched or signed in with (task M7.1): it names a person.
        | Request::DiscoverServices { .. }
        // Never the place searched for or the point of a map (task TE.1): where a person goes.
        | Request::SearchPlaces { .. }
        | Request::MapPreview { .. }
        | Request::ConnectCalDav { .. }
        | Request::Sync
        | Request::CalendarCollections
        | Request::TaskCollections
        | Request::Events { .. }
        | Request::Todos
        | Request::Settings
        | Request::UpdateSettings { .. }
        | Request::Accounts
        | Request::ListApps
        | Request::ListDesigns
        | Request::ListFonts
        | Request::ListColorSchemes
        | Request::ListClients
        | Request::CreateClient { .. }
        | Request::ListApprovals
        | Request::ListItems { .. }
        | Request::ListSources
        | Request::PostItem { .. }
        | Request::ResolveItems { .. }
        | Request::ListRules
        // The span only, never what was counted (task E8.8a).
        | Request::GetStats { .. }
        // Never the event's title or times (task E8.5b).
        | Request::GetQuiet
        // Nor those of a reminder; a pause only by its command (task ER1.2).
        | Request::ListReminders { .. }
        | Request::PauseReminders { .. } => Vec::new(),
        Request::SnoozeReminder { id: reminder, .. } | Request::DismissReminder { id: reminder } => {
            vec![random("reminder", reminder)]
        }
        Request::PreviewReminder { href } => item(&Some(href.clone())).into_iter().collect(),
        Request::UpdateTemplate { id: template }
        | Request::DeleteTemplate { id: template }
        | Request::LoadTemplate { id: template, .. }
        | Request::RenameTemplate { id: template, .. }
        // The template only, never the screen it loads on (issue #243); the one that loses a key
        // is added by the core, which looks it up ([`taken_from_id`]).
        | Request::SetTemplateShortcut { id: template, .. }
        | Request::SetTemplateStartup { id: template, .. } => vec![format!("template:{}", hashed(template))],
        Request::SaveEvent { event, .. } | Request::DeleteEvent { event, .. } => {
            [Some(collection(&event.collection)), item(&event.href)].into_iter().flatten().collect()
        }
        Request::SaveTodo { todo } | Request::DeleteTodo { todo } => {
            [Some(collection(&todo.collection)), item(&todo.href)].into_iter().flatten().collect()
        }
        Request::MoveTodo { href, before, .. } => {
            [item(&Some(href.clone())), item(before)].into_iter().flatten().collect()
        }
        // The list only (task E8.7a): never the mail account, the mail's id or its Message-ID,
        // not even hashed – together they would tell which mail a person turned into a task.
        Request::CreateTodoFromMail { collection: list, .. } => vec![collection(list)],
        // The account only, never the name of the new list (task G.7).
        Request::CreateTaskList { account: name, .. }
        | Request::CreateFilterList { account: name, .. }
        | Request::PreviewFilter { account: name, .. } => vec![account(name)],
        // The account, and the repository hashed: its name may be a private one (task G.9).
        Request::ListGithubRepos { account: name } => vec![account(name)],
        Request::DescribeGithubRepo { account: name, repo } => {
            vec![account(name), format!("repo:{}", hashed(&repo.trim().to_ascii_lowercase()))]
        }
        Request::UpdateFilterList { collection: list, .. }
        | Request::DeleteFilterList { collection: list }
        | Request::SetCollectionShown { collection: list, .. } => vec![collection(list)],
        Request::UnpinIssue { collection: list, url } | Request::AddLink { collection: list, url } => {
            vec![collection(list), format!("item:{}", hashed(url))]
        }
        Request::SetSetting { key, .. } => {
            let key = serde_json::to_value(key).ok().and_then(|key| key.as_str().map(str::to_string));
            key.map(|key| vec![id("setting", &key)]).unwrap_or_default()
        }
        Request::RemoveAccount { id: name } => vec![account(name)],
        // Only names of the suite (a binary or a window class) are logged, never a free text, and
        // never the target: it names a mail or a task (task E8.7a).
        Request::OpenApp { app, .. } => match AppId::find(app).is_some() {
            true => vec![id("app", app)],
            false => Vec::new(),
        },
        Request::UpdateClient { id: client, .. }
        | Request::NewClientKey { id: client }
        | Request::RemoveClient { id: client }
        | Request::ClientActivity { id: client } => vec![random("client", client)],
        Request::ApprovalStatus { id: approval } | Request::DecideApproval { id: approval, .. } => {
            vec![random("approval", approval)]
        }
        // A question names no id: its tool, project and command are what the agent would run, and
        // the approval it opens is in the line's `approval` already (task E2.5).
        Request::AskPermission { .. } => Vec::new(),
        Request::GetItem { id: entry }
        | Request::OpenItem { id: entry }
        // The entry only, never what the AI suggested (issue #120).
        | Request::SuggestTaskFromItem { id: entry, .. }
        | Request::SuggestEventFromItem { id: entry }
        | Request::SetItemDone { id: entry, .. }
        | Request::SnoozeItem { id: entry, .. }
        | Request::SetItemLane { id: entry, .. }
        | Request::HideSender { id: entry, .. }
        // The entry only, never the trait muted or its value (task E8.6a).
        | Request::ListMuteOptions { id: entry }
        // The entry only, never the category it was filed under (task E12.4).
        | Request::SetItemCategory { id: entry, .. }
        | Request::ClassifyItem { id: entry }
        | Request::MuteItem { id: entry, .. } => vec![random("inbox", entry)],
        // Mail shown again (task E18.2) or put on a pile (task E19.2) by its messages, hashed as in
        // the mail commands.
        Request::SetMailShown { ids, .. } | Request::SetMailPile { ids, .. } => {
            ids.iter().map(|message| format!("message:{}", hashed(message))).collect()
        }
        // Nothing of their own to name: a catalogue, a status, a background run.
        Request::ListCategories { .. }
        | Request::GetCategorization
        | Request::CatchUpCategories { .. }
        | Request::PreviewCategoryJob { .. }
        | Request::StartCategoryJob { .. }
        | Request::GetCategoryJob { .. }
        | Request::ControlCategoryJob { .. } => {
            Vec::new()
        }
        Request::PreviewClearCategories | Request::ClearCategories { .. } | Request::ControlClearCategories { .. } => Vec::new(),
        // The catalogue and the sorting rules (task E15.1): never a name, a description, a
        // condition's value or a category – a person's own words about their mail. A sorting
        // rule is named by its id only.
        Request::CreateCategory { .. }
        | Request::UpdateCategory { .. }
        | Request::RemoveCategory { .. }
        | Request::ListSortRules
        | Request::CreateSortRule { .. } => Vec::new(),
        Request::UpdateSortRule { id: rule, .. }
        | Request::RemoveSortRule { id: rule }
        | Request::MoveSortRule { id: rule, .. } => vec![random("rule", rule)],
        // The function and the account are ids, not content; the model's name is the server's.
        Request::AiAssignments => Vec::new(),
        Request::SetAiAssignment { engine, .. } => engine.iter().map(|id| account(id)).collect(),
        // The slots (issue #325) like the assignments; a download by its account and its id, never
        // the model's name; the recommendation names nothing.
        Request::SetAiSlot { slot, engine, .. } => {
            [Some(id("slot", slot.key())), engine.as_deref().map(account)].into_iter().flatten().collect()
        }
        Request::AiRecommend | Request::AiPulls => Vec::new(),
        Request::PullAiModel { account: name, .. } => vec![account(name)],
        Request::CancelAiPull { id: pull } => vec![random("pull", pull)],
        Request::CreateTaskFromItem { id: entry, collection: list, .. } => {
            vec![random("inbox", entry), collection(list)]
        }
        // The entry and the calendar, never the title or the times (task E8.5b).
        Request::CreateEventFromItem { id: entry, collection: calendar, .. } => {
            vec![random("inbox", entry), collection(calendar)]
        }
        // Answering an invitation (task E8.3b): the account and the mail hashed as for sending, the
        // calendar when one is named; never the answer, the title, the times or the organizer.
        Request::RespondToInvitation { account: name, id: message, collection: calendar, .. } => {
            [Some(account(name)), Some(format!("message:{}", hashed(message))), calendar.as_deref().map(collection)]
                .into_iter()
                .flatten()
                .collect()
        }
        Request::MuteSource { source, .. } => vec![id("source", &source.to_string())],
        Request::RemoveRule { id: rule } => vec![random("rule", rule)],
        // Mail (security rule 3 of task M2.1): never the address signed in with, nor a search, a
        // subject, a recipient, a file or a folder name. The account as for collections; message,
        // draft and folder ids only hashed, so lines can be matched but not with a mailbox.
        Request::ConnectMail { .. } => Vec::new(),
        // TorroMail's accounts (task M7.4): no address, and no id of another app's keychain.
        Request::TorroMailAccounts | Request::ImportTorroMailAccount { .. } => Vec::new(),
        // AI accounts (task KI.1a): never the address typed nor the key; finding names nothing.
        Request::ConnectAi { .. } | Request::FindAiServers => Vec::new(),
        // ntfy (task E5.12): never the address, a topic or the login; a topic's account only.
        Request::ConnectNtfy { .. } | Request::SetNtfyLogin { .. } => Vec::new(),
        Request::SetNtfyTopic { account: name, .. } => vec![account(name)],
        Request::AiModels { account: name, .. } => vec![account(name)],
        // The account and the switch, never the model's name (task S.24).
        Request::SetModelEnabled { account: name, enabled, .. } => {
            vec![account(name), id("enabled", if *enabled { "true" } else { "false" })]
        }
        Request::UpdateMailAccount { id: name, .. } => vec![account(name)],
        // The assistant (task C.1): the conversation as the core made it, and for a message the
        // engine (decision 5: the log says which engine the assistant used), never a text, a title
        // or a model's name.
        Request::ListEngines | Request::ListConversations => Vec::new(),
        // Voice (task C.9): no ids, never what was said.
        Request::VoiceStatus | Request::TestVoice => Vec::new(),
        Request::SendMessage { conversation, engine, bot, .. } => conversation
            .iter()
            .map(|id| random("conversation", id))
            .chain(bot.iter().map(|id| random("bot", id)))
            .chain([engine_id(engine)])
            .collect(),
        Request::OpenChatWindow { window: ChatWindow::Bot(bot) } => vec![format!("bot:{}", hashed(bot))],
        Request::OpenChatWindow { window: ChatWindow::New } => Vec::new(),
        Request::GetConversation { id: conversation, .. }
        | Request::GetCards { conversation }
        | Request::CancelAnswer { conversation }
        | Request::RenameConversation { id: conversation, .. }
        | Request::DeleteConversation { id: conversation }
        | Request::PinConversation { id: conversation, .. }
        | Request::OpenChatWindow { window: ChatWindow::Conversation(conversation) } => {
            vec![random("conversation", conversation)]
        }
        // The assistant's profile and memory (task C.10): a note by the id the core made, never
        // its text, a name or the personality.
        Request::GetProfile | Request::UpdateProfile { .. } | Request::ListMemories | Request::MyAccess => Vec::new(),
        Request::Remember { replaces, .. } => replaces.iter().map(|id| random("note", id)).collect(),
        Request::Forget { id: note } => vec![random("note", note)],
        // Bots (task B.1): a bot by the id the core made, never its name or texts; a message to one
        // with its conversation, never the text.
        Request::ListBots | Request::SaveBot { id: None, .. } => Vec::new(),
        Request::GetBot { id: bot } | Request::DeleteBot { id: bot } | Request::SaveBot { id: Some(bot), .. } => {
            vec![random("bot", bot)]
        }
        // The person's yes to the assistant asking a bot (task B.2): the assistant's conversation
        // and the bot.
        Request::ConfirmBot { conversation, bot } => vec![random("conversation", conversation), random("bot", bot)],
        Request::AskBot { bot, conversation, .. } => {
            [Some(random("bot", bot)), conversation.as_deref().map(|id| random("conversation", id))]
                .into_iter()
                .flatten()
                .collect()
        }
        // The assistant's own tools (issue #225): a file by the id the core made, never a title, the
        // Markdown, the template or a path.
        Request::ListTools | Request::CreatePdf { .. } => Vec::new(),
        Request::OpenFile { file } | Request::SaveFileAs { file, .. } => vec![random("file", file)],
        // Save targets of attachments (issue #290): a target by the id the core made, never its name,
        // folder or schemes; adding one names nothing.
        Request::CreateSaveTarget { .. } => Vec::new(),
        Request::UpdateSaveTarget { id: target, .. } | Request::RemoveSaveTarget { id: target } => {
            vec![random("save-target", target)]
        }
        // Saving attachments: the message hashed and the target by its id, never a file name, a part's
        // name or a path (`to: file | folder`, where the files went).
        Request::SaveMailAttachments { id: message, to, .. } => {
            let target = match to {
                torro_ipc::mail::SaveTo::Target(target) => Some(random("save-target", target)),
                _ => None,
            };
            [Some(format!("message:{}", hashed(message))), target].into_iter().flatten().collect()
        }
        Request::ListSaveTargets { id } => id.iter().map(|message| format!("message:{}", hashed(message))).collect(),
        // Mail filters (issue #339): a filter by the id the core made (`f<n>`, hex digits) and the
        // account it names, never its search, folder or category – the person's words about their mail.
        Request::ListMailFilters => Vec::new(),
        Request::CreateMailFilter { account: named, .. } | Request::PreviewMailFilter { account: named, .. } => {
            named.iter().map(|name| account(name)).collect()
        }
        Request::UpdateMailFilter { id: filter, account: named, .. } => {
            std::iter::once(random("filter", filter)).chain(named.iter().map(|name| account(name))).collect()
        }
        Request::RemoveMailFilter { id: filter } | Request::ApplyMailFilter { id: filter } => {
            vec![random("filter", filter)]
        }
        // MCP servers (issue #302): a server by its id and a tool by its name when they have their
        // form, never the arguments, a command, an address or a secret.
        Request::ListExtensions | Request::ListCatalog | Request::ListExtensionTools => Vec::new(),
        Request::GetExtension { id }
        | Request::SetExtensionEnabled { id, .. }
        | Request::ConfirmExtensionTools { id, .. }
        | Request::UpdateExtension { id, .. }
        | Request::SetExtensionSecret { id, .. }
        | Request::SetExtensionFolder { id, .. }
        | Request::ReadExtensionLogs { id }
        | Request::RemoveExtension { id }
        | Request::InstallExtension { from: InstallFrom::Custom { id, .. } }
        | Request::InstallExtension { from: InstallFrom::Catalog { entry: id, .. } } => {
            vec![checked("extension", id, extensions::valid_id)]
        }
        Request::CallExtensionTool { extension, tool, .. } => vec![
            checked("extension", extension, extensions::valid_id),
            checked("tool", tool, |tool| extensions::tool_name("x", tool).is_some()),
        ],
        // The tool `files` (issue #304): nothing at all, every field is a path or a name searched for.
        Request::ListFolder { .. }
        | Request::FindFiles { .. }
        | Request::CreateFolder { .. }
        | Request::CopyFile { .. }
        | Request::MoveFile { .. }
        | Request::TrashFile { .. } => Vec::new(),
        // The web (issue #305): never the search words, the language asked for or the address of a page;
        // a key by its provider only, never the key.
        Request::WebSearch { .. } | Request::FetchPage { .. } | Request::ListSearchProviders => Vec::new(),
        Request::SetSearchKey { provider, .. } => vec![id("provider", provider.name())],
        mail @ (Request::MailFolders { .. }
        | Request::MailAccountStats { .. }
        | Request::GetMailCachePolicy { .. }
        | Request::SetMailCachePolicy { .. }
        | Request::PreviewMailCache { .. }
        | Request::StartMailCacheJob { .. }
        | Request::MailCacheJobStatus { .. }
        | Request::ControlMailCacheJob { .. }
        | Request::MailBackupCreate { .. }
        | Request::MailBackupStatus { .. }
        | Request::MailBackupCancel { .. }
        | Request::MailBackupVerify { .. }
        | Request::MailRestorePlan { .. }
        | Request::MailRestoreApply { .. }
        | Request::SearchMail { .. }
        | Request::MailIndexStatus { .. }
        | Request::GetMail { .. }
        | Request::GetMailThread { .. }
        | Request::GetMailAttachment { .. }
        | Request::SetMailFlags { .. }
        | Request::MoveMail { .. }
        | Request::DeleteMail { .. }
        | Request::SaveDraft { .. }
        | Request::SendDraft { .. }) => {
            let folder = match mail {
                Request::SearchMail { folder: Some(folder), .. }
                | Request::MoveMail { to: MoveTarget::Folder(folder), .. } => Some(folder),
                _ => None,
            };
            mail.mail_account()
                .map(account)
                .into_iter()
                .chain(folder.map(|folder| format!("folder:{}", hashed(folder))))
                .chain(mail.message_ids().into_iter().map(|message| format!("message:{}", hashed(message))))
                .collect()
        }
    }
}

/// The ids of the agents that can drive the assistant (task C.7); fixed names, nothing a client chose.
const AGENT_ENGINES: [&str; 1] = ["claude-code"];

/// The assistant's engine for the log (decision 5 in `docs/assistent.md`: the log says which engine
/// the assistant used): `engine:<id>` for the test engine, the agents the core starts itself
/// (task C.7) and an AI account's id, any other text hashed (`engine:hash:<hash>`). Next to the ids
/// of `assistant.send_message`, and of every tool the assistant runs while it answers (task C.2).
pub fn engine_id(engine: &str) -> String {
    match engine == TEST_ENGINE || AGENT_ENGINES.contains(&engine) || is_account_id(engine) {
        true => format!("engine:{}", cut(engine)),
        false => format!("engine:hash:{}", hashed(engine)),
    }
}

/// The ids that say a bot ran for the assistant (task B.2), next to the line of the bot's client:
/// `bot:<the bot's id>` and `for:<the assistant's client id>`, so the log names both. Ids that do
/// not have the form the core makes are hashed.
pub fn delegation_ids(bot: &str, assistant: &str) -> Vec<String> {
    vec![made_id("bot", bot), behalf_id(assistant)]
}

/// `taken-from:<hash>`: the template that lost its key to `workspaces.set_template_shortcut` (issue
/// #243), hashed like the template the command names: ids of templates are made from their names.
pub fn taken_from_id(template: &str) -> String {
    format!("taken-from:{}", hashed(template))
}

/// `for:<client id>`: the client a bot ran for (task B.2), also on the lines of a person's decision.
pub fn behalf_id(client: &str) -> String {
    made_id("for", client)
}

/// `kind:<value>` for an id the core made, hashed when it does not have that form.
fn made_id(kind: &str, value: &str) -> String {
    match is_random_id(value) {
        true => format!("{kind}:{}", cut(value)),
        false => format!("{kind}:hash:{}", hashed(value)),
    }
}

/// A collection id (`<account id>/<the provider's id>`) for the log: the account as it is and the
/// rest hashed, `collection:nextcloud:anna@cloud.example.org/<hash>`. The rest is the calendar's
/// URL with server, user and the calendar's name in it. An id whose front is no account id (a bare
/// URL from before several accounts, or any text a client sent) is hashed whole:
/// `collection:hash:<hash>`. The account ends at the first `/`; for the rare account id with a `/`
/// in it the rest of it is hashed with the URL, which still names the collection the same way
/// every time.
fn collection(value: &str) -> String {
    match value.split_once('/') {
        Some((account, rest)) if is_account_id(account) => format!("collection:{account}/{}", hashed(rest)),
        _ => format!("collection:hash:{}", hashed(value)),
    }
}

/// The form of the ids the suite makes at random: inbox entries, rules and approvals (16 hex
/// digits), clients (8). Up to 36 hex digits and dashes, so a UUID passes too.
fn is_random_id(value: &str) -> bool {
    (1..=36).contains(&value.len()) && value.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The form of an account id: `<kind>:<name>`, the kind in small letters (`nextcloud`, `github`),
/// the whole without spaces or other characters a text would bring, at most [`ID_MAX`] long.
fn is_account_id(value: &str) -> bool {
    let Some((kind, name)) = value.split_once(':') else { return false };
    let printable = value.chars().all(|c| c.is_ascii_graphic());
    let kind_ok = !kind.is_empty() && kind.chars().all(|c| c.is_ascii_lowercase());
    kind_ok && !name.is_empty() && printable && value.len() <= ID_MAX
}

/// The first 16 hex digits of the SHA-256 of `value`: the same thing gets the same hash, so lines
/// about it can be matched, but the name it was made from does not show. It hides the name from
/// whoever reads the file, not from someone who guesses it and hashes the guess.
pub fn hashed(value: &str) -> String {
    Sha256::digest(value.as_bytes()).iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

/// `value` cut to [`ID_MAX`] characters.
fn cut(value: &str) -> &str {
    value.char_indices().nth(ID_MAX).map_or(value, |(end, _)| &value[..end])
}

/// Where the log goes when it is rotated: `audit.jsonl.1`.
fn rotated(file: &Path) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    file.with_file_name(name)
}

/// Appends `line` to `file` (created with mode 0600, its directory for the owner only), first
/// moving a log that would grow past `rotate_at` to [`rotated`].
fn append(file: &Path, line: &[u8], rotate_at: u64) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        crate::clients::create_private_dir(dir)?;
    }
    let size = fs::metadata(file).map(|meta| meta.len()).unwrap_or(0);
    if size > 0 && size + line.len() as u64 > rotate_at {
        fs::rename(file, rotated(file))?;
    }
    let mut options = fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(file)?;
    restrict(file)?;
    out.write_all(line)
}

/// A log made readable for others (by hand, by an older version) is made private again.
#[cfg(unix)]
fn restrict(file: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(file)?.permissions().mode();
    if mode & 0o077 != 0 {
        fs::set_permissions(file, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Windows keeps files in the profile private to the user already.
#[cfg(not(unix))]
fn restrict(file: &Path) -> io::Result<()> {
    let _ = file;
    Ok(())
}

/// The entries of `client` in `path`, oldest first; none when the file is not there.
fn read_client(path: &Path, client: &str) -> io::Result<Vec<ActivityEntry>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut entries = Vec::new();
    for line in io::BufReader::new(file).lines() {
        let Ok(entry) = serde_json::from_str::<ActivityEntry>(&line?) else { continue };
        if entry.client.as_deref() == Some(client) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A log in `dir/state/audit.jsonl`, rotated at the real size.
    pub(crate) fn audit_in(dir: &Path) -> Audit {
        Audit::new(Some(file_in(dir)), ROTATE_AT)
    }

    pub(crate) fn file_in(dir: &Path) -> PathBuf {
        dir.join("state").join(FILE)
    }

    fn line(client: &str, command: &str, n: i64) -> ActivityEntry {
        ActivityEntry {
            time: DateTime::from_timestamp(1_800_000_000 + n, 0).expect("time"),
            client: Some(client.into()),
            command: command.into(),
            ids: vec![format!("inbox:{n}")],
            result: "ok".into(),
            approval: None,
        }
    }

    #[test]
    fn the_log_is_private_and_one_line_per_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let audit = audit_in(dir.path());
        audit.record(&line("a1", "inbox.get_item", 1));
        audit.record(&line("a1", "inbox.get_item", 2));
        let text = fs::read_to_string(file_in(dir.path())).expect("written");
        assert_eq!(text.lines().count(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(file_in(dir.path())).expect("exists").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn the_activity_is_the_clients_own_newest_first() {
        let dir = tempfile::tempdir().expect("temp dir");
        let audit = audit_in(dir.path());
        audit.record(&line("a1", "system.ping", 1));
        audit.record(&line("b2", "system.status", 2));
        audit.record(&line("a1", "inbox.get_item", 3));
        let recent = audit.recent("a1").expect("read");
        assert_eq!(recent, vec![line("a1", "inbox.get_item", 3), line("a1", "system.ping", 1)]);
        assert_eq!(audit_in(&dir.path().join("none")).recent("a1").expect("no log yet"), vec![]);
    }

    #[test]
    fn the_log_is_rotated_before_it_grows_past_the_limit_and_both_are_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let one = serde_json::to_vec(&line("a1", "system.ping", 1)).expect("json").len() as u64 + 1;
        let audit = Audit::new(Some(file_in(dir.path())), one * 2);
        for n in 1..=3 {
            audit.record(&line("a1", "system.ping", n));
        }
        let current = fs::read_to_string(file_in(dir.path())).expect("log");
        let old = fs::read_to_string(rotated(&file_in(dir.path()))).expect("rotated");
        assert_eq!((old.lines().count(), current.lines().count()), (2, 1));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(rotated(&file_in(dir.path()))).expect("exists").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let commands: Vec<_> = audit.recent("a1").expect("read").into_iter().map(|e| e.time.timestamp()).collect();
        assert_eq!(commands, vec![1_800_000_003, 1_800_000_002, 1_800_000_001]);
    }

    #[test]
    fn at_most_fifty_entries_answer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let audit = audit_in(dir.path());
        for n in 0..60 {
            audit.record(&line("a1", "system.ping", n));
        }
        let recent = audit.recent("a1").expect("read");
        assert_eq!(recent.len(), ACTIVITY_MAX);
        assert_eq!(recent[0], line("a1", "system.ping", 59));
    }

    #[test]
    fn a_sign_in_stopped_at_a_certificate_is_no_ok() {
        let answer = Response::UntrustedCertificate {
            service: "IMAP".into(),
            host: "mail.home.lan".into(),
            sha256: format!("AB{}", ":CD".repeat(31)),
            public: false,
        };
        assert_eq!(outcome(&answer), ("untrusted-certificate".into(), None));
        assert_eq!(outcome(&Response::Ok), ("ok".into(), None));
        let line = entry(None, "accounts.connect_mail", Vec::new(), &answer, DateTime::UNIX_EPOCH);
        let text = serde_json::to_string(&line).expect("json");
        assert!(!text.contains("home.lan") && !text.contains("AB:CD"), "{text}");
    }

    #[test]
    fn without_a_file_nothing_is_written() {
        let audit = Audit::system();
        audit.record(&line("a1", "system.ping", 1));
        assert_eq!(audit.recent("a1").expect("empty"), vec![]);
    }

    /// Task C.1: the assistant's lines name the conversation and the engine, never the message,
    /// a title or a model; a conversation id without the core's form is hashed.
    #[test]
    fn assistant_lines_name_the_conversation_and_the_engine_only() {
        let send = Request::SendMessage {
            conversation: Some("0123456789abcdef".into()),
            engine: "ai:ollama@localhost:11434".into(),
            model: Some("secret-model".into()),
            effort: None,
            text: "Dinner with Anna".into(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
        };
        assert_eq!(
            super::ids(&send),
            vec!["conversation:0123456789abcdef".to_string(), "engine:ai:ollama@localhost:11434".to_string()]
        );
        let new = Request::SendMessage {
            conversation: None,
            engine: TEST_ENGINE.into(),
            model: None,
            effort: None,
            text: "Hi".into(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
        };
        assert_eq!(super::ids(&new), vec!["engine:test".to_string()]);
        let odd = Request::SendMessage {
            conversation: None,
            engine: "Anna's server".into(),
            model: None,
            effort: None,
            text: "Hi".into(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
        };
        assert_eq!(super::ids(&odd), vec![format!("engine:hash:{}", hashed("Anna's server"))]);
        assert_eq!(super::engine_id("claude-code"), "engine:claude-code", "an agent the core starts is named");

        let rename = Request::RenameConversation { id: "0123456789abcdef".into(), title: "Salary talks".into() };
        assert_eq!(super::ids(&rename), vec!["conversation:0123456789abcdef".to_string()]);
        let named = Request::DeleteConversation { id: "salary talks".into() };
        assert_eq!(super::ids(&named), vec![format!("conversation:hash:{}", hashed("salary talks"))]);
        assert_eq!(super::ids(&Request::OpenChatWindow { window: ChatWindow::New }), Vec::<String>::new());
        assert_eq!(super::ids(&Request::ListConversations), Vec::<String>::new());
    }

    #[test]
    fn reminder_commands_log_their_id_and_never_a_time_or_an_address() {
        let id = "0123456789abcdef0123456789abcdef";
        let snooze = Request::SnoozeReminder { id: id.into(), until: torro_ipc::Snooze::Minutes(5) };
        assert_eq!(ids(&snooze), vec![format!("reminder:{id}")], "32 hex characters are taken as they are");
        assert_eq!(ids(&Request::DismissReminder { id: id.into() }), vec![format!("reminder:{id}")]);
        let odd = ids(&Request::DismissReminder { id: "Dinner with Anna".into() });
        assert!(odd[0].starts_with("reminder:hash:"), "{odd:?}");
        assert!(ids(&Request::ListReminders { due_only: true }).is_empty());
        assert!(ids(&Request::PauseReminders { until: Some(Utc::now()) }).is_empty(), "not even the time");
        let preview = ids(&Request::PreviewReminder { href: "https://cloud.example.org/cal/work/Dinner.ics".into() });
        assert!(preview[0].starts_with("item:") && !preview[0].contains("Dinner"), "{preview:?}");
    }

    #[test]
    fn ids_never_carry_a_name_a_title_or_a_search() {
        let event = torro_pim::Event {
            uid: "u".into(),
            collection: "nextcloud:anna@cloud.example.org/cal/work/".into(),
            href: Some("https://cloud.example.org/cal/work/Dinner-with-Anna.ics".into()),
            etag: None,
            summary: "Dinner with Anna".into(),
            start: torro_pim::EventTime::DateTime(Utc::now()),
            end: None,
            location: Some("Secret place".into()),
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
        };
        let ids = ids(&Request::DeleteEvent { event, scope: None });
        assert_eq!(ids[0], format!("collection:nextcloud:anna@cloud.example.org/{}", hashed("cal/work/")));
        assert!(ids[1].starts_with("item:") && ids[1].len() == "item:".len() + 16, "{ids:?}");
        assert!(!ids.join(" ").contains("Dinner") && !ids.join(" ").contains("work"), "{ids:?}");

        let template = super::ids(&Request::RenameTemplate { id: "secret-plans".into(), name: "Secret plans".into() });
        assert!(!template.join(" ").contains("secret"), "{template:?}");
        // Issue #243: the template hashed, never the screen; the one that lost the key hashed too.
        let startup = torro_setups::Startup { screen: "DELL-U3423WE".into(), fallback: Default::default() };
        let startup = super::ids(&Request::SetTemplateStartup { id: "secret-plans".into(), startup: Some(startup) });
        assert_eq!(startup, template);
        assert!(!startup.join(" ").contains("DELL"), "{startup:?}");
        let key = super::ids(&Request::SetTemplateShortcut { id: "secret-plans".into(), key: Some(3) });
        assert_eq!(key, template);
        assert_eq!(taken_from_id("mail-day"), format!("taken-from:{}", hashed("mail-day")));
        let preview = Request::PreviewFilter { account: "github:anna".into(), queries: vec!["is:open salary".into()] };
        assert_eq!(super::ids(&preview), vec!["account:github:anna".to_string()]);
        // The account, and a repository's name only hashed (task G.9), the same in any case.
        let repos = Request::ListGithubRepos { account: "github:anna".into() };
        assert_eq!(super::ids(&repos), vec!["account:github:anna".to_string()]);
        let describe =
            |repo: &str| super::ids(&Request::DescribeGithubRepo { account: "github:anna".into(), repo: repo.into() });
        let described = describe("anna/salary-talks");
        assert_eq!(described, vec!["account:github:anna".to_string(), format!("repo:{}", hashed("anna/salary-talks"))]);
        assert!(!described.join(" ").contains("salary"), "{described:?}");
        assert_eq!(describe(" Anna/Salary-Talks"), described);
        let open = Request::OpenApp { app: "Dinner with Anna".into(), target: None };
        assert_eq!(super::ids(&open), Vec::<String>::new());
        let mail = Request::OpenApp { app: "torro-inbox-mail".into(), target: None };
        assert_eq!(super::ids(&mail), vec!["app:torro-inbox-mail".to_string()], "a window class of the suite");
        let set = Request::SetSetting { key: torro_ipc::SettingKey::Zoom, value: serde_json::json!(1.5) };
        assert_eq!(super::ids(&set), vec!["setting:zoom".to_string()]);
        // Muting (task E8.6a): the entry only, never the trait or a value of it.
        let sender = torro_inbox::item::MuteScope::Facet { key: torro_inbox::item::FacetKey::Sender, nth: 0 };
        let mute = Request::MuteItem {
            id: "3d29c13547a49e39".into(),
            scope: sender,
            until: None,
            effect: torro_inbox::item::MuteEffect::Hide,
        };
        assert_eq!(super::ids(&mute), vec!["inbox:3d29c13547a49e39".to_string()]);
        let options = Request::ListMuteOptions { id: "news@shop.example".into() };
        let logged = super::ids(&options).join(" ");
        assert!(logged.starts_with("inbox:hash:") && !logged.contains("shop"), "{logged}");
        // The statistics name nothing (task E8.8a): no source, no trait, no span.
        let stats = Request::GetStats { range: torro_ipc::StatsRange::Quarter, until: None };
        assert!(super::ids(&stats).is_empty());
        // The remote sources (task E5.2): an entry by its id, never its repository, reason or tag; a
        // source by its key, the account's id.
        let by_repo = torro_inbox::item::MuteScope::Facet { key: torro_inbox::item::FacetKey::Repo, nth: 0 };
        let mute_repo = Request::MuteItem {
            id: "3d29c13547a49e39".into(),
            scope: by_repo,
            until: None,
            effect: torro_inbox::item::MuteEffect::Info,
        };
        assert_eq!(super::ids(&mute_repo), vec!["inbox:3d29c13547a49e39".to_string()]);
        let done = Request::SetItemDone { id: "3d29c13547a49e39".into(), done: true, dismissed: true };
        assert_eq!(super::ids(&done), vec!["inbox:3d29c13547a49e39".to_string()]);
        let github = Request::MuteSource { source: torro_inbox::SourceRef::github("anna-berg"), muted: true };
        assert_eq!(super::ids(&github), vec!["source:github:anna-berg".to_string()]);
        // The assistant's own tools (issue #225): a file by its id, never a title, content or path.
        let pdf = Request::CreatePdf { title: "Salary talk".into(), markdown: "# Salary".into(), template: None };
        assert!(super::ids(&pdf).is_empty());
        let save = Request::SaveFileAs { file: "1a2b3c4d".into(), path: "/Users/anna/Salary.pdf".into() };
        assert_eq!(super::ids(&save), vec!["file:1a2b3c4d".to_string()]);
        let open = super::ids(&Request::OpenFile { file: "/Users/anna/Salary.pdf".into() }).join(" ");
        assert!(open.starts_with("file:hash:") && !open.contains("Salary"), "{open}");
        // MCP servers (issue #302): the server and the tool, never the arguments or a secret.
        let call = Request::CallExtensionTool {
            extension: "github".into(),
            tool: "search_issues".into(),
            arguments: serde_json::json!({"query": "salary"}).as_object().cloned().unwrap_or_default(),
        };
        assert_eq!(super::ids(&call), ["extension:github", "tool:search_issues"]);
        let odd = Request::CallExtensionTool {
            extension: "Salary Talk".into(),
            tool: "a b".into(),
            arguments: Default::default(),
        };
        let logged = super::ids(&odd).join(" ");
        assert!(logged.starts_with("extension:hash:") && !logged.contains("Salary"), "{logged}");
        let secret = Request::SetExtensionSecret {
            id: "github".into(),
            name: "GITHUB_PERSONAL_ACCESS_TOKEN".into(),
            value: torro_ipc::Secret::new("ghp_salary"),
        };
        assert_eq!(super::ids(&secret), ["extension:github"]);
        assert!(super::ids(&Request::ListExtensionTools).is_empty());
        // The tool `files` (issue #304): never a path or a name searched for.
        let path = || "/Users/anna/Salary/2026.pdf".to_string();
        for request in [
            Request::ListFolder { path: Some(path()) },
            Request::FindFiles { folder: path(), name: "salary".into() },
            Request::CreateFolder { path: path() },
            Request::CopyFile { from: path(), to: path() },
            Request::MoveFile { from: path(), to: path() },
            Request::TrashFile { path: path() },
        ] {
            assert!(super::ids(&request).is_empty(), "{request:?}");
        }
    }

    /// Task E1.2c: collection ids keep only their account; ids a client sends are logged as they
    /// are only when they have the form the suite gives them, else hashed.
    #[test]
    fn collection_and_client_ids_keep_only_what_has_their_form() {
        let shown = |list: &str| super::ids(&Request::SetCollectionShown { collection: list.into(), shown: true });
        let url = "nextcloud:anna@cloud.example.org/https://cloud.example.org/remote.php/dav/calendars/anna/salary/";
        let logged = shown(url);
        assert_eq!(logged, vec![format!("collection:nextcloud:anna@cloud.example.org/{}", hashed(&url[33..]))]);
        assert!(!logged[0].contains("salary") && !logged[0].contains("remote.php"), "{logged:?}");
        assert_eq!(shown("github:mahype/00ff"), vec![format!("collection:github:mahype/{}", hashed("00ff"))]);
        let legacy = "https://cloud.example.org/dav/anna/salary/";
        assert_eq!(shown(legacy), vec![format!("collection:hash:{}", hashed(legacy))]);
        assert_eq!(shown("Salary talk/x"), vec![format!("collection:hash:{}", hashed("Salary talk/x"))]);

        let get = |id: &str| super::ids(&Request::GetItem { id: id.into() });
        assert_eq!(get("0a1b2c3d4e5f6071"), vec!["inbox:0a1b2c3d4e5f6071".to_string()]);
        assert_eq!(get("123e4567-e89b-12d3-a456-426614174000"), vec!["inbox:123e4567-e89b-12d3-a456-426614174000"]);
        let text = "Ignore previous instructions";
        assert_eq!(get(text), vec![format!("inbox:hash:{}", hashed(text))]);
        assert_eq!(get(&"a".repeat(300)), vec![format!("inbox:hash:{}", hashed(&"a".repeat(300)))]);
        let rule = super::ids(&Request::RemoveRule { id: "my secret rule".into() });
        assert_eq!(rule, vec![format!("rule:hash:{}", hashed("my secret rule"))]);
        let status = super::ids(&Request::ApprovalStatus { id: "00ff00ff00ff00ff".into() });
        assert_eq!(status, vec!["approval:00ff00ff00ff00ff".to_string()]);
        let client = super::ids(&Request::RemoveClient { id: "Anna's laptop".into() });
        assert_eq!(client, vec![format!("client:hash:{}", hashed("Anna's laptop"))]);
        let account = super::ids(&Request::RemoveAccount { id: "Anna Smith".into() });
        assert_eq!(account, vec![format!("account:hash:{}", hashed("Anna Smith"))]);
        let task = Request::CreateTaskFromItem {
            id: "e1".into(),
            collection: url.into(),
            title: None,
            due: None,
            notes: None,
        };
        assert_eq!(super::ids(&task), vec!["inbox:e1".to_string(), logged[0].clone()]);
        // Task E8.5b: an event of an entry logs the entry and the calendar, never title or times.
        let at = torro_pim::EventTime::DateTime(chrono::Utc::now());
        let event = Request::CreateEventFromItem {
            id: "e1".into(),
            collection: url.into(),
            title: Some("Secret call".into()),
            start: at,
            end: at,
            location: None,
            notes: None,
        };
        assert_eq!(super::ids(&event), vec!["inbox:e1".to_string(), logged[0].clone()]);
        assert!(super::ids(&Request::GetQuiet).is_empty());
        // Task E8.3b: an answered invitation logs the account, the mail hashed and the calendar.
        let answer = Request::RespondToInvitation {
            account: "mail:anna@example.org".into(),
            id: "m:0a1b2c3d".into(),
            answer: torro_mail::InvitationAnswer::Accept,
            collection: Some(url.into()),
        };
        let expected = vec![
            "account:mail:anna@example.org".to_string(),
            format!("message:{}", hashed("m:0a1b2c3d")),
            logged[0].clone(),
        ];
        assert_eq!(super::ids(&answer), expected);
    }

    /// Task E8.7a: a task made of a mail logs its list only, opening an app with a target the app
    /// only: no account, mail id, Message-ID or target, not even hashed.
    #[test]
    fn mail_tasks_and_targets_log_no_mail() {
        let list = "nextcloud:anna@cloud.example.org/https://cloud.example.org/remote.php/dav/calendars/anna/tasks/";
        let task = Request::CreateTodoFromMail {
            account: "mail:anna@mailbox.org".into(),
            id: "m:0a1b2c3d".into(),
            collection: list.into(),
            title: Some("Answer the offer".into()),
            due: None,
        };
        let logged = super::ids(&task);
        assert_eq!(logged, vec![super::collection(list)]);
        let message_id = "<offer@keller.example>";
        let open = Request::OpenApp {
            app: "torro-inbox-mail".into(),
            target: Some(format!("mail:anna@mailbox.org:{message_id}")),
        };
        let opened = super::ids(&open);
        assert_eq!(opened, vec!["app:torro-inbox-mail".to_string()]);
        let all = [logged, opened].concat().join(" ");
        for secret in ["m:0a1b2c3d", "keller", "offer", &hashed("m:0a1b2c3d"), &hashed(message_id), "Answer"] {
            assert!(!all.contains(secret), "{secret} in {all}");
        }
    }

    /// Issue #339: a mail filter by its id `f<n>` as it is and its account, never its search, folder
    /// or category; an id of another form is hashed.
    #[test]
    fn mail_filters_log_their_id_and_account_only() {
        let create = Request::CreateMailFilter {
            query: "from:salary@boss.example".into(),
            account: Some("mail:anna@mailbox.org".into()),
            file: Some(torro_mail::FilterFile::Folder { id: "f:secret-folder".into() }),
            mark_read: true,
            category: Some(torro_mail::Category::Invoice),
            enabled: true,
        };
        assert_eq!(super::ids(&create), ["account:mail:anna@mailbox.org"]);
        let update = Request::UpdateMailFilter {
            id: "f12".into(),
            query: "from:salary@boss.example".into(),
            account: None,
            file: None,
            mark_read: true,
            category: None,
            enabled: None,
        };
        assert_eq!(super::ids(&update), ["filter:f12"]);
        let preview = Request::PreviewMailFilter { query: "salary".into(), account: None };
        assert!(super::ids(&preview).is_empty());
        assert!(super::ids(&Request::ListMailFilters).is_empty());
        assert_eq!(super::ids(&Request::ApplyMailFilter { id: "f3".into() }), ["filter:f3"]);
        let odd = Request::RemoveMailFilter { id: "salary filter".into() };
        assert_eq!(super::ids(&odd), [format!("filter:hash:{}", hashed("salary filter"))]);
        let all = [create, update, preview].iter().flat_map(super::ids).collect::<Vec<_>>().join(" ");
        for secret in ["salary", "boss", "secret-folder", "invoice"] {
            assert!(!all.contains(secret), "{secret} in {all}");
        }
    }

    /// Security rule 3 of task M2.1: the account as it is, messages and folders only hashed, never
    /// a search, a subject, a recipient, a file or a folder name.
    #[test]
    fn mail_requests_log_the_account_and_hashes_only() {
        let search = Request::SearchMail {
            account: Some("mail:anna@mailbox.org".into()),
            folder: Some("f:0a1b".into()),
            query: Some("salary negotiation".into()),
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit: 20,
            cursor: None,
            order: None,
            sort: None,
            snoozed: None,
            hidden: None,
            categories: Vec::new(),
            pile: None,
        };
        let logged = super::ids(&search);
        assert_eq!(logged, vec!["account:mail:anna@mailbox.org".to_string(), format!("folder:{}", hashed("f:0a1b"))]);
        let draft = torro_mail::Draft {
            id: None,
            account: "mail:anna@mailbox.org".into(),
            to: vec![torro_mail::MailAddress { name: Some("Bob".into()), address: "bob@example.org".into() }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Secret plans".into(),
            body: "…".into(),
            html: None,
            in_reply_to: Some("m:7".into()),
            attachments: vec![torro_mail::DraftAttachment::File { path: "/home/anna/contract.pdf".into() }],
        };
        let logged = super::ids(&Request::SaveDraft { draft });
        assert_eq!(logged, vec!["account:mail:anna@mailbox.org".to_string(), format!("message:{}", hashed("m:7"))]);
        let line = logged.join(" ");
        for secret in ["bob", "Secret", "contract", "salary"] {
            assert!(!line.contains(secret), "{line}");
        }
        let to = torro_ipc::mail::MoveTarget::Folder("f:9".into());
        let moved = super::ids(&Request::MoveMail { ids: vec!["m:1".into(), "m:2".into()], to });
        let expected = [format!("folder:{}", hashed("f:9")), format!("message:{}", hashed("m:1"))];
        assert_eq!(moved[..2], expected, "{moved:?}");
        assert_eq!(moved.len(), 3);
        // Task E18.2: "Show again" names its mail hashed, as the mail commands do.
        let shown = super::ids(&Request::SetMailShown { ids: vec!["m:1".into(), "m:2".into()], shown: true });
        assert_eq!(shown, [format!("message:{}", hashed("m:1")), format!("message:{}", hashed("m:2"))]);
        // Task E19.2: the piles alike, never the pile's word.
        let piled = super::ids(&Request::SetMailPile { ids: vec!["m:1".into()], pile: None, since: None });
        assert_eq!(piled, [format!("message:{}", hashed("m:1"))]);
        let update = Request::UpdateMailAccount {
            format: None,
            id: "mail:anna@mailbox.org".into(),
            sender_name: Some("Anna Example".into()),
            signature: None,
            notify: None,
            services: None,
            folder_roles: None,
        };
        assert_eq!(super::ids(&update), vec!["account:mail:anna@mailbox.org".to_string()]);
        let verify = Request::MailBackupVerify {
            file: "/Users/anna/Documents/Salary mail.zip".into(),
            account: "mail:anna@mailbox.org".into(),
        };
        let logged = super::ids(&verify);
        assert_eq!(logged, vec!["account:mail:anna@mailbox.org".to_string()]);
        assert!(!logged.join(" ").contains("Salary"));
    }
}
