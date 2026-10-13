//! Approvals: client commands that wait for a person (`docs/api.md`, "Freigaben: der Mensch
//! entscheidet").
//!
//! When a client's rights say a command needs a person's approval
//! ([`torro_ipc::api::Rights::needs_approval`]), the core does not run it but keeps it here as an
//! open approval and answers `pending` with its id and a summary in words ([`summary`]). The
//! approvals live in `approvals.json` in the core's state directory (mode 0600), so a restart loses
//! none; an open one expires ten minutes after it was asked for ([`LIFETIME`]). Decided and expired
//! ones are kept a day ([`KEPT`]) so the client can still read the outcome, then dropped.
//!
//! A coding agent's question "may I use this tool?" (`approvals.ask_permission`, task E2.5) is an
//! approval too, of its own kind ([`is_question`]): its summary is built here from the tool and
//! the full command ([`question_summary`]), it expires after half a minute
//! ([`QUESTION_LIFETIME`]), and allowing it runs nothing but answers the agent's hook.
//!
//! Only the app key decides ([`torro_ipc::Request::DecideApproval`]): the control centre,
//! `torroworkplace ctl allow|deny <id>`, or the desktop notification with "Allow" and "Deny" that
//! every new approval shows ([`Approvals::system`]), whose click goes to the core with the app key.
//! The notification closes once the approval is decided by any of these ways, and the core asks
//! again about the open ones when it starts ([`Approvals::announce_open`]). Every open approval is
//! also an entry of the inbox, with "Allow" and "Deny", done once it is decided or expired (task
//! E2.1, [`crate::inbox::file_approvals`] over [`Approvals::with_open`]). The agent never gets a
//! code it could confirm with. A command kept from agents ([`Mcp::Never`]: taking secrets,
//! managing clients, deciding approvals) never becomes an approval, so a secret never reaches
//! `approvals.json`.
//!
//! The summary says what really runs: titles, dates and names come from the core's own caches,
//! looked up by the keys the command runs with ([`Facts`]), never from the titles the client sent
//! along. An inbox entry can change under its id (a client reposts in the same thread), so an
//! approval of an `inbox.*` command keeps what the entry looked like when it was asked for
//! ([`EntryPin`]); the core refuses to run it when the entry changed meanwhile (task E1.2c).
//! What is stored is the request itself, with the client's text in it; the file is for the
//! owner only. Changes are made under one lock and written before they count: an approval that
//! cannot be written is not opened (the core refuses the command), a decision that cannot be
//! written is not taken.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::paths::AppPaths;
use torro_gui::datetime::{format_short_date, format_time, to_local};
use torro_inbox::item::{FacetKey, MuteEffect, MuteScope};
use torro_inbox::{HideScope, Item, Lane, Link, Rule, RuleEffect, SourceRef};
use torro_ipc::api::Mcp;
use torro_ipc::{ApprovalInfo, ApprovalState, Request, Response};
use torro_mail::{MailAddress, Role};
use torro_pim::{Event, EventTime, Todo};
use torro_setups::LoadMode;

/// The file in the core's state directory.
pub const FILE: &str = "approvals.json";

/// How long an approval waits for a person.
pub const LIFETIME: chrono::Duration = chrono::Duration::minutes(10);

// ----- Questions of agents (task E2.5) ---------------------------------------------------------
//
// `approvals.ask_permission` is an approval of its own kind: a coding agent's `PermissionRequest`
// hook asks "may I use this tool?", the person answers like any approval, and allowing runs
// nothing in the core ([`crate::core::Core`] answers `ok`). The hook polls the answer and hands it
// to the agent; without one in time it says nothing, and the agent asks in its own terminal.

/// The catalogue name of a question ([`torro_ipc::Request::AskPermission`]); approvals of this
/// command are questions ([`is_question`]).
pub const QUESTION_COMMAND: &str = "approvals.ask_permission";

/// How long a question waits for a person: the 30 seconds the hook waits, and a little slack for
/// starting the hook and its first look, so the question never expires while the hook still
/// waits for it. The agent asks in its own terminal after that, so waiting longer would only
/// leave a question on the desktop that nobody hears the answer of.
pub const QUESTION_LIFETIME: chrono::Duration = chrono::Duration::seconds(32);

/// How many questions one client may have open at a time, within [`OPEN_MAX`]. A question
/// lives only half a minute, so the ten approvals alone would let an agent ask some twenty times
/// a minute and wear the person down until they allow blindly.
pub const QUESTIONS_OPEN_MAX: usize = 3;

/// The longest tool name a question takes, in characters.
pub const QUESTION_TOOL_MAX: usize = 100;

/// The longest `details` a question takes, in characters. The person must see every character
/// of what would run, so longer details are refused rather than cut (the agent then asks in its
/// own terminal); 2,000 leave room for the rest of the summary in an inbox entry's body.
pub const QUESTION_DETAILS_MAX: usize = 2000;

/// How long an approval of `request` waits for a person: [`QUESTION_LIFETIME`] for a question,
/// [`LIFETIME`] for everything else.
pub fn lifetime(request: &Request) -> chrono::Duration {
    match request {
        Request::AskPermission { .. } => QUESTION_LIFETIME,
        _ => LIFETIME,
    }
}

/// Whether `approval` is a question of an agent ([`QUESTION_COMMAND`]), not a command waiting.
pub fn is_question(approval: &ApprovalInfo) -> bool {
    approval.command == QUESTION_COMMAND
}

/// Why a question with `tool` and `details` is not put to the person, as the `error` answer;
/// `None` when it may be. `tool` must be a tool's name as agents give it: 1 to
/// [`QUESTION_TOOL_MAX`] ASCII letters, digits and `_ - . : /` (`Bash`, `apply_patch`,
/// `mcp__fs__write`), so it cannot carry a sentence of its own into the summary. `details` may
/// hold anything up to [`QUESTION_DETAILS_MAX`] characters.
pub fn question_refusal(tool: &str, details: &str) -> Option<Response> {
    let name_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '/');
    let message = if tool.is_empty() || tool.len() > QUESTION_TOOL_MAX || !tool.chars().all(name_char) {
        format!(
            "`tool` must be a tool name of 1 to {QUESTION_TOOL_MAX} letters, digits and _ - . : / (like Bash or \
             mcp__fs__write)"
        )
    } else if details.chars().count() > QUESTION_DETAILS_MAX {
        format!(
            "`details` holds more than {QUESTION_DETAILS_MAX} characters; the person must see all of it, so ask in \
             your own interface"
        )
    } else {
        return None;
    };
    Some(Response::Error { code: "error".into(), message })
}

/// The summary of a question, in the core's language: `Claude Code wants to use Bash in
/// "my-project": cargo test`. The project stands in quotes, so a directory name cannot pass as
/// the command. `agent` is who asks as the core knows it (a hook client's agent, else
/// the client's name), `tool` a name [`question_refusal`] let pass, `project` the agent's project
/// if it sent one.
///
/// `session` is the name the agent gave its session (task E2.6), only from a hook client: it
/// stands after who asks, marked as a session and in quotes (`Claude Code (session "Login bug")
/// wants to use …`), made one line of at most 80 characters ([`torro_inbox::hook::session_title`])
/// and quoted like the project, so it can pass neither as the core's words nor as the command.
///
/// `details` is what would run and is shown **in full**: invisible format characters go, control
/// characters (line breaks too) and blanks become spaces, long runs of them `␣×n` ([`harmless`]);
/// nothing else changes, so the person reads the command as it is. It comes last, after the core's words and after every
/// other placeholder was filled, so nothing in it can fill a placeholder or pass as the core's
/// words. Who asks and the project go through [`quote`] like every other client text.
pub fn question_summary(
    agent: &str,
    session: Option<&str>,
    tool: &str,
    project: Option<&str>,
    details: &str,
) -> String {
    let (agent, project, details) = (quote(agent), project.map(quote), harmless(details));
    // Filled before the sentence, so the name (quoted: no braces left) fills no placeholder.
    let agent = match session.and_then(torro_inbox::hook::session_title) {
        Some(session) => trf(
            "{agent} (session \"{session}\")",
            &[("agent", &agent), ("session", &quote_up_to(&session, torro_inbox::hook::SESSION_TITLE_MAX))],
        ),
        None => agent,
    };
    let details = details.trim();
    // `details` is the last argument on purpose: `trf` fills in order.
    match (project.filter(|project| !project.is_empty()), details.is_empty()) {
        (Some(project), false) => trf(
            "{agent} wants to use {tool} in \"{project}\": {details}",
            &[("agent", &agent), ("tool", &tool), ("project", &project), ("details", &details)],
        ),
        (Some(project), true) => trf(
            "{agent} wants to use {tool} in \"{project}\"",
            &[("agent", &agent), ("tool", &tool), ("project", &project)],
        ),
        (None, false) => {
            trf("{agent} wants to use {tool}: {details}", &[("agent", &agent), ("tool", &tool), ("details", &details)])
        }
        (None, true) => trf("{agent} wants to use {tool}", &[("agent", &agent), ("tool", &tool)]),
    }
}

/// `text` made harmless without shortening it, for what a question would run: invisible format
/// characters ([`is_format`]) go; control characters, every kind of space and the blank letters
/// that look like one ([`is_blank`]) become plain spaces; and a run of [`BLANK_RUN`] or more of
/// them is written `␣×n`, so no part of the command can hide behind a wide gap (past the end of
/// a title or a notification). Unlike [`quote`] it keeps quotes, braces and every other
/// character, so the command reads as it is.
fn harmless(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blanks = 0usize;
    let flush = |out: &mut String, blanks: &mut usize| {
        match *blanks {
            n if n >= BLANK_RUN => out.push_str(&format!("␣×{n}")),
            n => out.extend(std::iter::repeat_n(' ', n)),
        }
        *blanks = 0;
    };
    for c in text.chars().filter(|c| !is_format(*c)) {
        if c.is_control() || c.is_whitespace() || is_blank(c) {
            blanks += 1;
        } else {
            flush(&mut out, &mut blanks);
            out.push(c);
        }
    }
    flush(&mut out, &mut blanks);
    out
}

/// From how many blanks in a row [`harmless`] writes the run as `␣×n`.
const BLANK_RUN: usize = 3;

/// A letter that shows as nothing but is no space or format character: Braille blank, Hangul
/// fillers, halfwidth Hangul filler.
fn is_blank(c: char) -> bool {
    matches!(u32::from(c), 0x2800 | 0x3164 | 0x115F | 0x1160 | 0xFFA0)
}

/// How long a decided or expired approval is kept, so its client can still read the outcome.
pub const KEPT: chrono::Duration = chrono::Duration::days(1);

/// The longest piece of a client's text in a summary, in characters.
const QUOTE_MAX: usize = 60;

/// How many approvals one client may have open at a time. Each one asks the person with a
/// notification, so an agent in a loop must not be able to flood the desktop.
pub const OPEN_MAX: usize = 10;

/// How the person hears of approvals: asked once for every new one after it was stored (and
/// again when the core starts, [`Approvals::announce_open`]), and told when one was decided, so
/// the question can leave the desktop.
#[derive(Clone)]
pub struct Notifier {
    ask: Arc<dyn Fn(&ApprovalInfo) + Send + Sync>,
    settled: Arc<dyn Fn(&str) + Send + Sync>,
}

impl Notifier {
    /// Calls `ask` for every approval to ask about and `settled` with the id of every approval a
    /// person decided (in the control centre, with `ctl`, or in the notification itself). Both
    /// must return at once: they run in the core's request handling.
    pub fn new(
        ask: impl Fn(&ApprovalInfo) + Send + Sync + 'static,
        settled: impl Fn(&str) + Send + Sync + 'static,
    ) -> Notifier {
        Notifier { ask: Arc::new(ask), settled: Arc::new(settled) }
    }

    /// Calls `ask` and has nothing to take back when an approval is decided; for tests.
    pub fn asking(ask: impl Fn(&ApprovalInfo) + Send + Sync + 'static) -> Notifier {
        Notifier::new(ask, |_: &str| {})
    }
}

/// What the core itself holds about the things a request names, for [`summary`]. The core looks
/// everything up by the keys the command will run with (the `href` of an event or task, resolved
/// the way the write resolves it, and the ids of collections, templates and accounts), never in
/// the titles and names the client sent along: a client must not be able to show "Lunch" and
/// delete the board meeting.
///
/// Only filled after the client's reach was checked (`Core::handle_from`, step 2), so it names
/// nothing the client could not read anyway.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// The event or task that is changed, moved or deleted, as the core caches it; for
    /// `tasks.move_todo` the task moved, for `tasks.unpin_issue` the pinned item. `None` when the
    /// request names none or the core does not hold it: the summary then says "unknown".
    pub item: Option<Known>,
    /// The display name of what the request works on: the calendar or task list it writes to
    /// (for a move, where the item goes), the filter list, the collection shown or hidden, the
    /// template or the account. `None` when the core does not know it: the summary then shows the
    /// id the command runs with, which then finds nothing either.
    pub name: Option<String>,
    /// For a save that moves an item to another collection: the display name of the collection
    /// that holds it now and loses it. `None` when nothing moves.
    pub moved_from: Option<String>,
    /// For `tasks.save_todo` on an issue of a GitHub filter list: the issue as `owner/repo#n`, as
    /// the cache holds it under the `uid` (task G.5b). Ticking it closes or reopens the issue on
    /// GitHub, which the summary says instead of "change task". `None` for everything else
    /// (also pull requests, which are never closed here).
    pub issue: Option<String>,
    /// With [`Facts::issue`]: the save changes nothing of the task but whether it is done, so the
    /// summary may say "Close GitHub issue …" alone (task G.6b). `false` (the default) makes it
    /// name the change of the task as well. Always `true` for a filter list, where nothing more
    /// can be changed.
    pub ticks_only: bool,
    /// For `tasks.add_link`: the list is a GitHub filter list, so the item is pinned there, not
    /// written as a task (task G.6b).
    pub pins: bool,
    /// For the `mail.*` commands: what the core's mail store holds about what they name ([`MailFacts`]).
    /// `None` for every other command, and until the store exists (tasks M3.2, M3.3). For
    /// `accounts.update_mail_account` the account's label is [`Facts::name`].
    pub mail: Option<MailFacts>,
}

/// What the core holds about the messages, drafts and folders a `mail.*` command names, for the
/// approval's summary (rules set by task M2.1; looked up and worded by M3.2 and M3.3). Looked up
/// in the core's mail store by the ids the command runs with, after the client's reach was
/// checked, never taken from the client's text. Every text goes through [`quote`]. The summaries:
///
/// - `mail.send_draft`, from [`MailFacts::draft`]: `Send "<subject>" from <account> to
///   <first recipient>`, then ` and <n> more` when there are more recipients (Cc and Bcc counted),
///   then `, <n> attachments` when there are any, then `, Bcc: <address>, …` naming every blind
///   copy (the first [`BCC_SHOWN`], then ` and <n> more`), then `: "<start of the text>"` (task
///   M3.3). A blind copy is how mail would leave unseen, so the summary never folds it into the
///   count alone. Every recipient, the attachments and more of the text are in the approval's
///   details ([`details`], task M3.5). Without the draft: "Send an unknown draft".
/// - `mail.move_messages`: `Move "<subject>" to "<folder>"` for one message, `Move <n> messages,
///   among them "<first subject>", to "<folder>"` for several; the folder by the name the store
///   has, a role without a folder by its word ("Archive"). Messages the store does not hold count
///   as unknown ("Move 2 unknown messages to …"). An empty subject is "(no subject)".
/// - `mail.delete_messages`: as moving, `Delete …` and, for `permanent`, `Delete … for good`.
/// - `mail.set_flags` (only with `confirm: on_every_change`): `Mark "<subject>" as read, not
///   flagged` and the like.
/// - `mail.save_draft`: a new draft `Save a draft "<subject>" in <account>` (the subject the client
///   sent, since that is what will be written); replacing one `Replace the draft "<subject as
///   stored>"`, or "Replace an unknown draft".
///
/// Task M3.2 fills [`DraftFacts::subject`] only (replacing a draft); M3.3 counts the recipients
/// with Bcc and the attachments of the stored draft for sending (`crate::mail::send`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MailFacts {
    /// The account's address as the core knows it (`anna@mailbox.org`); `None` when unknown: the
    /// summary then shows the account id the command runs with.
    pub account: Option<String>,
    /// How many messages the command names (`ids`), known or not.
    pub count: usize,
    /// The subjects of the named messages the store holds, in the order of `ids`.
    pub subjects: Vec<String>,
    /// For `mail.move_messages`: the display name of the target folder, the role's word for a role.
    pub folder: Option<String>,
    /// For `mail.send_draft` and for `mail.save_draft` with an id: the draft as the core stores it.
    pub draft: Option<DraftFacts>,
}

/// A draft as the core stores it, for [`MailFacts::draft`]: what the person must see before it
/// leaves, counted from the stored draft, never from the request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DraftFacts {
    pub subject: String,
    /// The first recipient's address (`To`, else `Cc`, else `Bcc`); `None` without recipients.
    pub first_recipient: Option<String>,
    /// All recipients: `To`, `Cc` and `Bcc` together, each address once.
    pub recipients: usize,
    /// The attachments, files and forwarded parts together.
    pub attachments: usize,
    /// For `mail.send_draft` (task M3.3): the start of the text that goes out, on one line; empty
    /// for a draft without text. The summary shows its first 60 characters ([`quote`]).
    pub text_start: String,
    /// For `mail.send_draft` (task M3.3): the Bcc addresses, which the summary names one by one.
    pub bcc: Vec<String>,
    /// For the details of `mail.send_draft` (task M3.5, [`details`]): the `To` recipients as the
    /// draft has them, with their names.
    pub to: Vec<MailAddress>,
    /// For the details of `mail.send_draft`: the `Cc` recipients, with their names.
    pub cc: Vec<MailAddress>,
    /// For the details of `mail.send_draft`: every attachment that goes out, in the draft's order.
    pub files: Vec<FileFact>,
}

/// One attachment of a draft that is sent, for the details of its approval ([`details`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileFact {
    /// The file name as it goes out (made safe by the core).
    pub name: String,
    /// Its size in bytes.
    pub size: u64,
}

/// How many Bcc addresses a summary of `mail.send_draft` names before it says "and <n> more".
pub const BCC_SHOWN: usize = 5;

/// An event or task as the core caches it, for [`Facts::item`].
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub title: String,
    /// When an event starts (its first cached occurrence); `None` for a task.
    pub start: Option<EventTime>,
}

/// What the inbox entry an `inbox.*` command names looked like when the client asked for
/// approval (task E1.2c): the person allowed what the summary showed of it, not what the client
/// may have reposted into the same thread since.
///
/// A repost replaces title, text and link ([`torro_inbox::rules::merge`]), sets `updated` to the
/// time of the report and counts one more; nothing else changes these, and `count` only grows. So
/// an entry that still has both was not reposted into, and whatever the command reads from it is
/// what the person saw. Waking a snoozed entry moves `updated` as well; such an approval is
/// refused too, which only means asking again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryPin {
    /// [`Item::updated`] when the approval was asked for.
    pub updated: DateTime<Utc>,
    /// [`Item::count`] when the approval was asked for.
    pub count: u32,
}

impl EntryPin {
    /// The pin of `item` as it is now.
    pub fn of(item: &Item) -> EntryPin {
        EntryPin { updated: item.updated, count: item.count }
    }

    /// `item` is still as it was pinned: neither reposted into nor woken.
    pub fn holds(&self, item: &Item) -> bool {
        *self == EntryPin::of(item)
    }
}

/// One approval in `approvals.json`: what the API shows, the request that runs when a person
/// allows it, and for an `inbox.*` command what its entry looked like then ([`EntryPin`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    info: ApprovalInfo,
    request: Request,
    /// Missing in approvals stored before task E1.2c, and for every other command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pin: Option<EntryPin>,
    /// For `mail.send_draft` (task M3.3): the fingerprint of the message the person was shown
    /// ([`Approvals::open_with_fingerprint`]). Missing for every other command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fingerprint: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ApprovalsFile {
    #[serde(default)]
    approvals: Vec<Entry>,
}

/// Why an approval could not be opened, read or decided.
#[derive(Debug)]
pub enum ApprovalsError {
    /// No home directory, so nowhere to keep approvals.
    NoHome,
    Io {
        path: PathBuf,
        err: io::Error,
    },
    /// The system has no randomness to make an id from.
    Random(String),
    /// No approval with this id for this caller (also one of another client, or one dropped
    /// after a day).
    NotFound(String),
    /// The approval was decided already or expired.
    NotOpen(String),
    /// The command is kept from agents ([`Mcp::Never`]) and never waits for approval.
    KeptBack(&'static str),
    /// The client has [`OPEN_MAX`] approvals open already.
    TooMany,
}

impl ApprovalsError {
    /// The error code of the API answer.
    pub fn code(&self) -> &'static str {
        match self {
            ApprovalsError::NotFound(_) => "not-found",
            ApprovalsError::NotOpen(_) => "conflict",
            ApprovalsError::KeptBack(_) => "forbidden",
            ApprovalsError::TooMany => "busy",
            _ => "error",
        }
    }

    /// The error as an API answer. A failure to read or write the file names no path towards the
    /// client (it would tell an agent where the user's home is); the whole of it goes to the log.
    pub fn response(&self) -> Response {
        let message = match self {
            ApprovalsError::NoHome | ApprovalsError::Io { .. } | ApprovalsError::Random(_) => {
                eprintln!("torroworkplace: {self}");
                "the approvals cannot be read or stored on this device; the core's log says why".to_string()
            }
            _ => self.to_string(),
        };
        Response::Error { code: self.code().into(), message }
    }
}

impl std::fmt::Display for ApprovalsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalsError::NoHome => f.write_str("no home directory to keep approvals in"),
            ApprovalsError::Io { path, err } => write!(f, "{}: {err}", path.display()),
            ApprovalsError::Random(err) => write!(f, "cannot make an approval id: {err}"),
            ApprovalsError::NotFound(id) => write!(f, "no approval {id:?}"),
            ApprovalsError::NotOpen(id) => write!(f, "approval {id:?} was decided already or expired"),
            ApprovalsError::KeptBack(command) => write!(f, "{command} never waits for approval"),
            ApprovalsError::TooMany => write!(
                f,
                "{OPEN_MAX} commands of this client wait for a person's approval already; wait for their decision"
            ),
        }
    }
}

impl std::error::Error for ApprovalsError {}

/// What a person's decision leaves to do.
#[derive(Debug)]
pub enum Decision {
    /// Denied; nothing runs. `request` is what was asked for, for the audit log's ids.
    Denied { approval: ApprovalInfo, request: Box<Request> },
    /// Allowed: the approval is [`ApprovalState::Running`]. Run `request` for the client `client`
    /// (rights and collections checked again) and store its answer with [`Approvals::finish`].
    /// With a `pin`, the inbox entry the request names must still hold it ([`EntryPin::holds`]),
    /// else the core answers `conflict` and runs nothing. With a `fingerprint`
    /// ([`Approvals::open_with_fingerprint`], `mail.send_draft`), the message built now must have
    /// the same one, else the core answers `conflict` and sends nothing.
    Run { client: String, request: Box<Request>, pin: Option<EntryPin>, fingerprint: Option<String> },
}

/// The open and recent approvals of the core.
pub struct Approvals {
    /// `approvals.json`; `None` without a home directory, then no approval can be opened.
    file: Option<PathBuf>,
    notify: Notifier,
    /// Also held while the file is written, so the file always holds the newest state.
    state: tokio::sync::Mutex<State>,
    /// Marked on every opened or decided approval ([`Approvals::changes`]).
    changed: tokio::sync::watch::Sender<()>,
}

#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    /// The file was read; it is read on first use, so building the core reads nothing.
    loaded: bool,
}

impl Approvals {
    /// The user's approvals: `approvals.json` in the core's state directory, and a desktop
    /// notification with "Allow" and "Deny" for each new one. A click there sends
    /// `approvals.decide_approval` to the core's socket with the app key, as the control centre
    /// would. Reads nothing until first used.
    ///
    /// In tests there is no such file and no notification: a test that forgets to pass its own
    /// approvals ([`crate::core::Core::with_approvals`]) gets `forbidden` for every approval
    /// instead of touching the user's state directory or desktop.
    pub fn system() -> Approvals {
        if cfg!(test) {
            return Approvals::new(None, Notifier::asking(|_: &ApprovalInfo| {}));
        }
        let file = AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join(FILE));
        Approvals::new(file, desktop_notifier(torro_ipc::socket_path()))
    }

    /// Approvals kept in `file`, announced with `notify`. Tests pass a temporary file and a
    /// notifier that shows nothing.
    pub fn new(file: Option<PathBuf>, notify: Notifier) -> Approvals {
        Approvals {
            file,
            notify,
            state: tokio::sync::Mutex::new(State::default()),
            changed: tokio::sync::watch::channel(()).0,
        }
    }

    /// Marked as changed each time an approval is opened or decided, after it was stored, and each
    /// time one is found expired; for the tray's count (task A.8b). Nobody looks at the approvals
    /// when one expires by the clock, so whoever counts still waits for the next `expires` of the
    /// open ones itself; the mark covers an expiry that falls between counting and that wait.
    pub fn changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.changed.subscribe()
    }

    /// Opens an approval for `request` of the client `client` (named `client_name`, or shown by
    /// its id when the name is empty), stores it and announces it. `summary` says in words what
    /// would happen ([`summary`]). For an `inbox.*` command, `pin` is what its entry looked like
    /// when the summary was written (task E1.2c); it is stored with the approval and handed back
    /// with the decision ([`Decision::Run`]). Every other command passes `None`.
    ///
    /// Refuses a command kept from agents ([`ApprovalsError::KeptBack`]) before anything is
    /// stored: such a request may carry a secret. A client with [`OPEN_MAX`] open approvals, or
    /// asking with [`QUESTIONS_OPEN_MAX`] questions open, gets [`ApprovalsError::TooMany`]. When the file cannot be written nothing is opened, and the
    /// caller must refuse the command.
    pub async fn open(
        &self,
        client: &str,
        client_name: &str,
        request: Request,
        pin: Option<EntryPin>,
        summary: String,
        now: DateTime<Utc>,
    ) -> Result<ApprovalInfo, ApprovalsError> {
        self.open_entry(client, client_name, request, (pin, None), (summary, Vec::new()), now).await
    }

    /// [`Approvals::open`] for a command that must run on exactly what the person was shown
    /// (task M3.3, `mail.send_draft`): `fingerprint` names it (for a mail, the SHA-256 of the
    /// built message and its envelope). It is stored with the approval, handed back with the
    /// decision ([`Decision::Run`]), and the caller compares it with what it would run then. It is
    /// never shown to the client: it is no code to confirm with.
    ///
    /// `(summary, details)` is what the person is shown: the summary ([`summary`]) and the lines
    /// below it ([`details`], task M3.5), both stored in [`ApprovalInfo`].
    pub async fn open_with_fingerprint(
        &self,
        client: &str,
        client_name: &str,
        request: Request,
        fingerprint: String,
        (summary, details): (String, Vec<String>),
        now: DateTime<Utc>,
    ) -> Result<ApprovalInfo, ApprovalsError> {
        self.open_entry(client, client_name, request, (None, Some(fingerprint)), (summary, details), now).await
    }

    /// [`Approvals::open`] with the entry's pin and fingerprint, and the details it shows.
    async fn open_entry(
        &self,
        client: &str,
        client_name: &str,
        request: Request,
        (pin, fingerprint): (Option<EntryPin>, Option<String>),
        (summary, details): (String, Vec<String>),
        now: DateTime<Utc>,
    ) -> Result<ApprovalInfo, ApprovalsError> {
        let command = request.operation().command;
        if command.mcp == Mcp::Never {
            return Err(ApprovalsError::KeptBack(command.name));
        }
        let mut state = self.state(now).await?;
        let open = state.entries.iter().filter(|e| e.info.client == client && e.info.state.is_pending()).count();
        if open >= OPEN_MAX {
            return Err(ApprovalsError::TooMany);
        }
        let asking = matches!(request, Request::AskPermission { .. });
        let questions = state
            .entries
            .iter()
            .filter(|e| e.info.client == client && e.info.state.is_pending() && is_question(&e.info))
            .count();
        if asking && questions >= QUESTIONS_OPEN_MAX {
            return Err(ApprovalsError::TooMany);
        }
        let info = ApprovalInfo {
            id: new_id()?,
            client: client.to_string(),
            // "… asks for approval" needs someone who asks; the id is what the person can look up.
            client_name: if client_name.trim().is_empty() { client } else { client_name }.to_string(),
            command: command.name.to_string(),
            summary,
            details,
            created: now,
            expires: now + lifetime(&request),
            decided: None,
            state: ApprovalState::Pending,
        };
        state.entries.push(Entry { info: info.clone(), request, pin, fingerprint });
        if let Err(err) = self.write(&state).await {
            state.entries.pop();
            return Err(err);
        }
        drop(state);
        self.changed.send_replace(());
        (self.notify.ask)(&info);
        Ok(info)
    }

    /// Asks the person again about every approval that is still open, as when it was opened, and
    /// returns how many. For the core's start: the questions of the core before went with its
    /// process. Approvals that expired meanwhile are not asked about. Without a file (no home
    /// directory, tests without their own approvals) there is nothing to ask about.
    pub async fn announce_open(&self, now: DateTime<Utc>) -> Result<usize, ApprovalsError> {
        if self.file.is_none() {
            return Ok(0);
        }
        let open: Vec<ApprovalInfo> = {
            let state = self.state(now).await?;
            state.entries.iter().filter(|entry| entry.info.state.is_pending()).map(|entry| entry.info.clone()).collect()
        };
        for info in &open {
            (self.notify.ask)(info);
        }
        Ok(open.len())
    }

    /// Runs `work` with the approvals still open as of `now` (expired ones are marked first) while
    /// no approval can be opened or decided, and returns what it returns. So whatever `work` passes
    /// the open approvals on to (the inbox, task E2.1) is never overtaken by an older view of them.
    /// `work` must return at once: it holds up every other approval command. Without a file (no
    /// home directory, tests without their own approvals) no approval can be open, and `work` gets
    /// none.
    pub async fn with_open<T>(
        &self,
        now: DateTime<Utc>,
        work: impl FnOnce(&[ApprovalInfo]) -> T,
    ) -> Result<T, ApprovalsError> {
        if self.file.is_none() {
            return Ok(work(&[]));
        }
        let state = self.state(now).await?;
        let open: Vec<ApprovalInfo> = state
            .entries
            .iter()
            .filter(|entry| entry.info.state.is_pending())
            .map(|entry| entry.info.clone())
            .collect();
        // `state` is held until `work` returns.
        Ok(work(&open))
    }

    /// The approvals `client` may see, newest first: its own, or all for `None` (the app key).
    pub async fn list(&self, client: Option<&str>, now: DateTime<Utc>) -> Result<Vec<ApprovalInfo>, ApprovalsError> {
        let state = self.state(now).await?;
        let mut list: Vec<ApprovalInfo> =
            state.entries.iter().filter(|entry| visible(entry, client)).map(|entry| entry.info.clone()).collect();
        list.sort_by_key(|info| std::cmp::Reverse(info.created));
        Ok(list)
    }

    /// The approval `id`, if `client` may see it ([`Approvals::list`]).
    pub async fn status(
        &self,
        client: Option<&str>,
        id: &str,
        now: DateTime<Utc>,
    ) -> Result<ApprovalInfo, ApprovalsError> {
        let state = self.state(now).await?;
        state
            .entries
            .iter()
            .find(|entry| entry.info.id == id && visible(entry, client))
            .map(|entry| entry.info.clone())
            .ok_or_else(|| ApprovalsError::NotFound(id.to_string()))
    }

    /// A person's decision on the open approval `id`, stored before it counts. Allowed, the
    /// approval becomes [`ApprovalState::Running`] and the caller runs the request
    /// ([`Decision::Run`]); denied, it is done. An approval that is no longer open is
    /// [`ApprovalsError::NotOpen`].
    pub async fn decide(&self, id: &str, allow: bool, now: DateTime<Utc>) -> Result<Decision, ApprovalsError> {
        let mut state = self.state(now).await?;
        let index = state
            .entries
            .iter()
            .position(|entry| entry.info.id == id)
            .ok_or_else(|| ApprovalsError::NotFound(id.to_string()))?;
        if !state.entries[index].info.state.is_pending() {
            return Err(ApprovalsError::NotOpen(id.to_string()));
        }
        let before = state.entries[index].info.clone();
        let entry = &mut state.entries[index];
        entry.info.decided = Some(now);
        entry.info.state = if allow { ApprovalState::Running } else { ApprovalState::Denied };
        let request = Box::new(entry.request.clone());
        let decision = match allow {
            true => Decision::Run {
                client: entry.info.client.clone(),
                request,
                pin: entry.pin,
                fingerprint: entry.fingerprint.clone(),
            },
            false => Decision::Denied { approval: entry.info.clone(), request },
        };
        if let Err(err) = self.write(&state).await {
            state.entries[index].info = before;
            return Err(err);
        }
        drop(state);
        self.changed.send_replace(());
        (self.notify.settled)(id);
        Ok(decision)
    }

    /// Stores the answer of an allowed approval that ran ([`ApprovalState::Done`]). Kept in memory
    /// even when the file cannot be written, so the client still reads it until the core stops.
    pub async fn finish(
        &self,
        id: &str,
        response: Response,
        now: DateTime<Utc>,
    ) -> Result<ApprovalInfo, ApprovalsError> {
        let mut state = self.state(now).await?;
        let entry = state
            .entries
            .iter_mut()
            .find(|entry| entry.info.id == id)
            .ok_or_else(|| ApprovalsError::NotFound(id.to_string()))?;
        entry.info.state = ApprovalState::Done { response: Box::new(response) };
        let info = entry.info.clone();
        if let Err(err) = self.write(&state).await {
            eprintln!("torroworkplace: cannot store the outcome of approval {id}: {err}");
        }
        Ok(info)
    }

    /// The state, read from the file on first use, with expired approvals marked and old ones
    /// dropped as of `now`. Both follow from the time alone, so they need no write of their own;
    /// an approval found expired is marked as changed ([`Approvals::changes`]).
    async fn state(&self, now: DateTime<Utc>) -> Result<tokio::sync::MutexGuard<'_, State>, ApprovalsError> {
        let mut state = self.state.lock().await;
        if !state.loaded {
            let file = self.file.clone().ok_or(ApprovalsError::NoHome)?;
            state.entries = blocking(move || read(&file)).await?;
            state.loaded = true;
        }
        if tidy(&mut state.entries, now) {
            self.changed.send_replace(());
        }
        Ok(state)
    }

    /// Writes `state` to the file (0600), on a blocking thread.
    async fn write(&self, state: &State) -> Result<(), ApprovalsError> {
        let file = self.file.clone().ok_or(ApprovalsError::NoHome)?;
        let content = ApprovalsFile { approvals: state.entries.clone() };
        blocking(move || {
            let bytes = serde_json::to_vec_pretty(&content)
                .map_err(|err| ApprovalsError::Io { path: file.clone(), err: io::Error::other(err) })?;
            if let Some(dir) = file.parent() {
                crate::clients::create_private_dir(dir)
                    .map_err(|err| ApprovalsError::Io { path: file.clone(), err })?;
            }
            crate::clients::write_private(&file, &bytes)
                .map_err(|err| ApprovalsError::Io { path: file.clone(), err: io::Error::other(err.to_string()) })
        })
        .await
    }
}

/// A client sees its own approvals, the app key (`None`) all.
fn visible(entry: &Entry, client: Option<&str>) -> bool {
    client.is_none_or(|client| entry.info.client == client)
}

/// Marks open approvals past their time as expired (decided at that time) and drops the ones
/// decided longer than [`KEPT`] ago. `true` when one expired.
fn tidy(entries: &mut Vec<Entry>, now: DateTime<Utc>) -> bool {
    let mut expired = false;
    for entry in entries.iter_mut() {
        if entry.info.state.is_pending() && entry.info.expires <= now {
            entry.info.state = ApprovalState::Expired;
            entry.info.decided = Some(entry.info.expires);
            expired = true;
        }
    }
    entries.retain(|entry| entry.info.decided.is_none_or(|decided| now - decided < KEPT));
    expired
}

/// The approvals in `file`; none when it does not exist. A file that is not ours is moved aside
/// (`approvals.json.damaged`) and reported, so approvals keep working.
///
/// An approval that was running when the core stopped is done with an error: nobody knows whether
/// its command took effect, and it must never run twice.
fn read(file: &Path) -> Result<Vec<Entry>, ApprovalsError> {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(ApprovalsError::Io { path: file.to_path_buf(), err }),
    };
    let mut entries = match serde_json::from_slice::<ApprovalsFile>(&bytes) {
        Ok(content) => content.approvals,
        Err(err) => {
            let aside = file.with_extension("json.damaged");
            eprintln!("torroworkplace: {} is damaged ({err}); moved to {}", file.display(), aside.display());
            fs::rename(file, &aside).map_err(|err| ApprovalsError::Io { path: file.to_path_buf(), err })?;
            Vec::new()
        }
    };
    for entry in &mut entries {
        if matches!(entry.info.state, ApprovalState::Running) {
            let message = "the core stopped while the command ran; check whether it took effect".to_string();
            entry.info.state =
                ApprovalState::Done { response: Box::new(Response::Error { code: "error".into(), message }) };
        }
    }
    Ok(entries)
}

/// Runs file work on a blocking thread, so other requests are not held up.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApprovalsError> + Send + 'static,
) -> Result<T, ApprovalsError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| ApprovalsError::Io { path: PathBuf::from(FILE), err: io::Error::other(err.to_string()) })?
}

/// 16 random hex digits.
fn new_id() -> Result<String, ApprovalsError> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|err| ApprovalsError::Random(err.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The notifier of [`Approvals::system`]: a desktop notification whose "Allow" or "Deny" goes to
/// the core at `socket` as `approvals.decide_approval` with the app key, which
/// `torro_ipc::client::call` reads from next to the socket. It runs in the core's own process, so
/// the decision has the app's authority and the agent never sees a code.
///
/// Each question closes when its approval is decided, whichever way ([`Notifier::new`]'s
/// `settled`), and when it expires ([`crate::notify::ask`]).
fn desktop_notifier(socket: Option<PathBuf>) -> Notifier {
    // The questions on the desktop by approval id; sending closes one.
    let open: Arc<std::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>> = Arc::default();
    let asking = Arc::clone(&open);
    let ask = move |approval: &ApprovalInfo| {
        let (close, settled) = tokio::sync::oneshot::channel();
        {
            let mut open = asking.lock().unwrap_or_else(PoisonError::into_inner);
            // Questions that went away on their own (expired, closed by hand) need no closing.
            open.retain(|_, close| !close.is_closed());
            open.insert(approval.id.clone(), close);
        }
        let runtime = tokio::runtime::Handle::try_current().ok();
        let (socket, id) = (socket.clone(), approval.id.clone());
        crate::notify::ask(approval, settled, move |allow| {
            let (Some(socket), Some(runtime)) = (socket, runtime) else { return };
            runtime.spawn(async move {
                let request = Request::DecideApproval { id, allow };
                if let Err(err) = torro_ipc::client::call(&socket, &request).await {
                    eprintln!("torroworkplace: cannot pass on the decision: {err}");
                }
            });
        });
    };
    let settled = move |id: &str| {
        let close = open.lock().unwrap_or_else(PoisonError::into_inner).remove(id);
        if let Some(close) = close {
            // Gone already when the person answered in the notification itself.
            let _ = close.send(());
        }
    };
    Notifier::new(ask, settled)
}

/// What `request` would do, in words, for the person who decides: `Delete event "Team meeting" on
/// Tue, 14 Oct 09:00 from "Work"`. In the core's language.
///
/// What is changed, moved or deleted is named from `facts`, which the core looked up by the keys
/// the command runs with ([`Facts`]); a changed title or start shows as `"old" → "new"`. The
/// client's own text appears only where it is what will be written: the title and start of a new
/// or changed item, a new name. An item the core does not hold is "an unknown event" or "an
/// unknown task", never the title the client claims.
///
/// All text, the client's and the cached one, is made harmless first ([`quote`]): one line, no
/// invisible format characters, no quotes that could end the quoted part, no placeholders, at
/// most 60 characters. Front ends still show the summary as plain text only.
pub fn summary(request: &Request, facts: &Facts) -> String {
    let today = Local::now().date_naive();
    // The display name the core knows, else the id the command runs with.
    let named = |id: &str| quote(facts.name.as_deref().unwrap_or(id));
    match request {
        Request::DeleteEvent { event } => match &facts.item {
            Some(item) => trf(
                "Delete event \"{title}\" on {when} from \"{calendar}\"",
                &[
                    ("title", &quote(&item.title)),
                    ("when", &item.start.map(|start| when(&start, today)).unwrap_or_default()),
                    ("calendar", &named(&event.collection)),
                ],
            ),
            None => trf("Delete an unknown event from \"{calendar}\"", &[("calendar", &named(&event.collection))]),
        },
        Request::SaveEvent { event } => event_saved(event, facts, today),
        Request::DeleteTodo { todo } => match &facts.item {
            Some(item) => trf(
                "Delete task \"{title}\" from \"{list}\"",
                &[("title", &quote(&item.title)), ("list", &named(&todo.collection))],
            ),
            None => trf("Delete an unknown task from \"{list}\"", &[("list", &named(&todo.collection))]),
        },
        Request::SaveTodo { todo } => todo_saved(todo, facts),
        Request::MoveTodo { .. } => match (&facts.item, &facts.name) {
            (Some(item), Some(list)) => {
                trf("Reorder task \"{title}\" in \"{list}\"", &[("title", &quote(&item.title)), ("list", &quote(list))])
            }
            _ => tr("Reorder an unknown task").to_string(),
        },
        Request::DeleteTemplate { id } => trf("Delete template \"{name}\"", &[("name", &named(id))]),
        Request::UpdateTemplate { id } => {
            trf("Overwrite template \"{name}\" with the active workspace", &[("name", &named(id))])
        }
        Request::LoadTemplate { id, mode: LoadMode::Replace } => {
            trf("Load template \"{name}\" and close the windows on the workspace", &[("name", &named(id))])
        }
        Request::LoadTemplate { id, .. } => trf("Load template \"{name}\"", &[("name", &named(id))]),
        Request::SaveTemplate { name: Some(name) } => {
            trf("Save the active workspace as template \"{name}\"", &[("name", &quote(name))])
        }
        Request::SaveTemplate { name: None } => tr("Save the active workspace as a template").to_string(),
        Request::RenameTemplate { id, name } => {
            trf("Rename template \"{name}\" to \"{new}\"", &[("name", &named(id)), ("new", &quote(name))])
        }
        Request::ConnectNextcloud { server } => trf("Connect the Nextcloud at {server}", &[("server", &quote(server))]),
        Request::ConnectGithub => tr("Connect a GitHub account").to_string(),
        Request::RemoveAccount { id } => trf("Remove the account {account}", &[("account", &named(id))]),
        Request::UpdateMailAccount { id, sender_name, signature, notify, services, folder_roles } => trf(
            "Change the mail account {account}: {changes}",
            &[
                ("account", &named(id)),
                (
                    "changes",
                    &mail_account_changes(
                        sender_name.as_deref(),
                        signature.as_deref(),
                        notify.is_some(),
                        services.as_ref(),
                        folder_roles.as_ref(),
                    ),
                ),
            ],
        ),
        Request::Sync => tr("Sync all accounts").to_string(),
        Request::SetSetting { key, value } => trf(
            "Change the setting {key} to {value}",
            &[("key", &quote(&format!("{key:?}"))), ("value", &quote(&value.to_string()))],
        ),
        Request::SetCollectionShown { collection, shown: true } => {
            trf("Show \"{collection}\" in the suite", &[("collection", &named(collection))])
        }
        Request::SetCollectionShown { collection, shown: false } => {
            trf("Hide \"{collection}\" in the suite", &[("collection", &named(collection))])
        }
        Request::UpdateSettings { .. } => tr("Replace all settings").to_string(),
        // The suite's name of the app (a window class like `torro-inbox-mail` says little); a name
        // of no app stays as sent – the core refuses it with `not-found`.
        Request::OpenApp { app } => {
            let name = AppId::find(app).map_or(app.as_str(), |known| known.display_name);
            trf("Open {app}", &[("app", &quote(name))])
        }
        // The account by the label the core knows; the name is what the list will be called.
        Request::CreateTaskList { account, name, .. } => {
            trf("Create task list \"{name}\" in {account}", &[("name", &quote(name)), ("account", &named(account))])
        }
        Request::CreateFilterList { name, .. } => trf("Create filter list \"{name}\"", &[("name", &quote(name))]),
        Request::UpdateFilterList { collection, name, .. } => {
            let (old, new) = (named(collection), quote(name));
            match old == new {
                true => trf("Change filter list \"{name}\"", &[("name", &old)]),
                false => trf("Change filter list \"{name}\" → \"{new}\"", &[("name", &old), ("new", &new)]),
            }
        }
        Request::DeleteFilterList { collection } => {
            trf("Delete filter list \"{list}\"", &[("list", &named(collection))])
        }
        Request::UnpinIssue { collection, url } => {
            // The item as the core will match it: repository and number from the address.
            let issue = torro_pim_github::address::issue_ref(url)
                .map_or_else(|| quote(url), |issue| quote(&format!("{}#{}", issue.repo, issue.number)));
            let list = named(collection);
            match &facts.item {
                Some(item) => trf(
                    "Unpin \"{title}\" ({item}) from filter list \"{list}\"",
                    &[("title", &quote(&item.title)), ("item", &issue), ("list", &list)],
                ),
                None => trf("Unpin {item} from filter list \"{list}\"", &[("item", &issue), ("list", &list)]),
            }
        }
        // The item as the core will ask GitHub for it; the core refuses an address that names
        // none before anyone is asked, so the raw text shows only for an approval stored before.
        Request::AddLink { collection, url } => {
            let issue = torro_pim_github::address::issue_ref(url)
                .map_or_else(|| quote(url), |issue| quote(&format!("{}#{}", issue.repo, issue.number)));
            let args: [(&str, &dyn std::fmt::Display); 2] = [("item", &issue), ("list", &named(collection))];
            match facts.pins {
                true => trf("Pin {item} to filter list \"{list}\"", &args),
                false => trf("Add a link to {item} to \"{list}\"", &args),
            }
        }
        Request::SetMailFlags { ids, add, remove } => trf(
            "Mark {messages} as {flags}",
            &[("messages", &mail_messages(facts.mail.as_ref(), ids.len())), ("flags", &flag_words(add, remove))],
        ),
        Request::MoveMail { ids, to } => {
            // The folder as the store names it; for a role the core found no folder for, its word.
            let folder = match (facts.mail.as_ref().and_then(|mail| mail.folder.as_deref()), to) {
                (Some(folder), _) => quote(folder),
                (None, torro_ipc::mail::MoveTarget::Role(role)) => role_word(role.role()).to_string(),
                (None, torro_ipc::mail::MoveTarget::Folder(id)) => quote(id),
            };
            trf(
                "Move {messages} to \"{folder}\"",
                &[("messages", &mail_messages(facts.mail.as_ref(), ids.len())), ("folder", &folder)],
            )
        }
        Request::DeleteMail { ids, permanent } => {
            let messages = mail_messages(facts.mail.as_ref(), ids.len());
            match permanent {
                true => trf("Delete {messages} for good", &[("messages", &messages)]),
                false => trf("Delete {messages}", &[("messages", &messages)]),
            }
        }
        Request::SaveDraft { draft } => match (&draft.id, facts.mail.as_ref().and_then(|mail| mail.draft.as_ref())) {
            (Some(_), Some(stored)) => trf("Replace the draft {subject}", &[("subject", &subject(&stored.subject))]),
            (Some(_), None) => tr("Replace an unknown draft").to_string(),
            // A new draft: what will be written is the client's text, in the account the core knows.
            (None, _) => {
                let account = facts.mail.as_ref().and_then(|mail| mail.account.as_deref()).unwrap_or(&draft.account);
                trf(
                    "Save a draft {subject} in {account}",
                    &[("subject", &subject(&draft.subject)), ("account", &quote(account))],
                )
            }
        },
        Request::SendDraft { .. } => match facts.mail.as_ref().and_then(|mail| mail.draft.as_ref()) {
            Some(draft) => {
                let account = facts.mail.as_ref().and_then(|mail| mail.account.as_deref()).unwrap_or_default();
                draft_sent(draft, account)
            }
            None => tr("Send an unknown draft").to_string(),
        },
        // Reads and the commands only the app key may send never wait for approval; say at least
        // which command it is.
        other => trf("Run {command}", &[("command", &other.operation().name())]),
    }
}

/// What the inbox holds about what an `inbox.*` request names, for [`inbox_summary`]. Looked up
/// in the inbox by the id the command runs with ([`crate::inbox::Inbox::facts`]), never taken
/// from the client's text, and only after the inbox's own checks passed
/// ([`crate::inbox::Inbox::refusal`]), so it names nothing the client could not see.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InboxFacts {
    /// The entry that `inbox.set_item_done`, `inbox.snooze_item` or `inbox.set_item_lane` would
    /// change, as stored. `None` when the request names none or there is none with its id: the
    /// summary then says "an unknown inbox entry".
    pub item: Option<Item>,
    /// The rule `inbox.remove_rule` would remove, as stored; `None` when unknown.
    pub rule: Option<Rule>,
    /// The display name of the task list `inbox.create_task_from_item` writes to, as the core
    /// caches it (task E3.5); `None` when unknown: the summary then shows the list's id.
    pub list: Option<String>,
}

/// What an `inbox.*` request would do, in words, like [`summary`]: `Unmute "Codex"
/// (agent:codex) in the inbox`. In the core's language.
///
/// Entries and rules are named from `facts`, looked up by the id the command runs with; a source
/// is named by its label and key, the key being what the command runs with. The client's own
/// text appears only where it is what will be written: the title of a report and a thread key.
/// All of it goes through [`quote`]. Requests that never wait for approval (reads) get the
/// general sentence of [`summary`].
pub fn inbox_summary(request: &Request, facts: &InboxFacts) -> String {
    let today = Local::now().date_naive();
    let entry = || match &facts.item {
        Some(item) => format!("\"{}\"", quote(&item.title)),
        None => tr("an unknown inbox entry").to_string(),
    };
    match request {
        // A hook's agent state is stored under a title the core writes, not the one sent along.
        Request::PostItem { kind: torro_inbox::Kind::AgentWaiting | torro_inbox::Kind::AgentDone, .. } => {
            tr("Report the state of a coding agent to the inbox").to_string()
        }
        Request::PostItem { title, .. } => trf("Report \"{title}\" to the inbox", &[("title", &quote(title))]),
        Request::ResolveItems { thread_key } => {
            trf("Mark its own inbox entries of thread {thread} as done", &[("thread", &quote(thread_key))])
        }
        // What opening does follows the stored link: the page's host, the app, the agent's window.
        Request::OpenItem { .. } => match facts.item.as_ref().and_then(|item| item.link.as_ref()) {
            Some(Link::Url { url }) => trf(
                "Open the web page on {host} from {entry}",
                &[("host", &quote(&web_host(url).unwrap_or_default())), ("entry", &entry())],
            ),
            Some(Link::App { app }) => {
                let name = AppId::find(app).map_or(app.as_str(), |a| a.display_name);
                trf("Open {app} from {entry}", &[("app", &quote(name)), ("entry", &entry())])
            }
            Some(Link::Window) => trf("Bring the agent window of {entry} to the front", &[("entry", &entry())]),
            // A mail opens in the Inbox app (task E4.1).
            Some(Link::Mail { .. }) => {
                trf("Open {app} from {entry}", &[("app", &quote(AppId::INBOX.display_name)), ("entry", &entry())])
            }
            None => trf("Show {entry} on the screen", &[("entry", &entry())]),
        },
        // The title that will be written: the client's, or – left out or blank – the entry's. The
        // core puts the entry's title into the request before it asks (task E1.2c).
        Request::CreateTaskFromItem { collection, title, due, .. } => {
            let title = title.as_deref().filter(|title| !title.trim().is_empty());
            let title = quote(title.or(facts.item.as_ref().map(|item| item.title.as_str())).unwrap_or_default());
            let list = quote(facts.list.as_deref().unwrap_or(collection));
            match due {
                Some(due) => trf(
                    "Make a task \"{title}\" in \"{list}\" due {when} from {entry}",
                    &[("title", &title), ("list", &list), ("when", &when(due, today)), ("entry", &entry())],
                ),
                None => trf(
                    "Make a task \"{title}\" in \"{list}\" from {entry}",
                    &[("title", &title), ("list", &list), ("entry", &entry())],
                ),
            }
        }
        // Done marks the mail of a mail entry read on its server (task E4.1).
        Request::SetItemDone { done: true, .. } if is_mail(facts.item.as_ref()) => {
            trf("Mark {entry} as done and its mail as read", &[("entry", &entry())])
        }
        Request::SetItemDone { done: true, .. } => trf("Mark {entry} as done", &[("entry", &entry())]),
        Request::SetItemDone { done: false, .. } => trf("Open {entry} again", &[("entry", &entry())]),
        Request::SnoozeItem { until: Some(until), .. } => trf(
            "Snooze {entry} until {when}",
            &[("entry", &entry()), ("when", &when(&EventTime::DateTime(*until), today))],
        ),
        Request::SnoozeItem { until: None, .. } => trf("Stop snoozing {entry}", &[("entry", &entry())]),
        Request::SetItemLane { lane, remember: false, .. } => {
            trf("Move {entry} to {lane}", &[("entry", &entry()), ("lane", &lane_name(*lane))])
        }
        // For mail the lane is kept for the sender, not the account (task E4.1).
        Request::SetItemLane { lane, remember: true, .. }
            if facts.item.as_ref().and_then(torro_inbox::rules::mail_sender).is_some() =>
        {
            let sender = facts.item.as_ref().and_then(torro_inbox::rules::mail_sender).unwrap_or_default();
            trf(
                "Move {entry} to {lane} and send later mail from {sender} there too",
                &[("entry", &entry()), ("lane", &lane_name(*lane)), ("sender", &format!("\"{}\"", quote(&sender)))],
            )
        }
        Request::SetItemLane { lane, remember: true, .. } => {
            let source =
                facts.item.as_ref().map_or_else(|| tr("its source").to_string(), |item| source_name(&item.source));
            trf(
                "Move {entry} to {lane} and send later notices of {source} there too",
                &[("entry", &entry()), ("lane", &lane_name(*lane)), ("source", &source)],
            )
        }
        Request::MuteSource { source, muted: true } => {
            trf("Mute {source} in the inbox", &[("source", &source_name(source))])
        }
        Request::MuteSource { source, muted: false } => {
            trf("Unmute {source} in the inbox", &[("source", &source_name(source))])
        }
        // The sender or domain as the inbox holds it for the entry, never as the client says (task
        // E3.10); a rule holds for every mail account (task E3.11).
        Request::HideSender { scope, .. } => {
            let target = facts.item.as_ref().and_then(|item| torro_inbox::rules::hide_target(item, *scope));
            match (scope, target) {
                (HideScope::Conversation, _) => {
                    trf("Hide {entry} from the inbox; the mail stays unread", &[("entry", &entry())])
                }
                (HideScope::Sender, Some(sender)) => trf(
                    "Hide mail from {sender} in every mail account; the mail stays unread",
                    &[("sender", &format!("\"{}\"", quote(&sender)))],
                ),
                (HideScope::Domain, Some(domain)) => trf(
                    "Hide mail from everyone at {domain} in every mail account; the mail stays unread",
                    &[("domain", &format!("\"{}\"", quote(&domain)))],
                ),
                (_, None) => trf("Hide the sender of {entry} in the inbox", &[("entry", &entry())]),
            }
        }
        // The trait and its value as the inbox holds them for the entry, never as the client says
        // (task E8.6a): a trait holds for every source of the kind.
        Request::MuteItem { scope, until, effect, .. } => {
            let what = match (scope, facts.item.as_ref().and_then(|item| torro_inbox::rules::mute_target(item, *scope)))
            {
                (MuteScope::Conversation, _) => trf("the conversation {entry}", &[("entry", &entry())]),
                (MuteScope::Facet { .. }, Some(facet)) => {
                    let named = format!("\"{}\"", quote(&torro_inbox::rules::facet_label(&facet)));
                    reach_of(facet.key, named)
                }
                (MuteScope::Facet { .. }, None) => trf("a trait of {entry}", &[("entry", &entry())]),
            };
            let effect = mute_effect_name(*effect);
            match until {
                Some(until) => trf(
                    "Mute {what} until {when} ({effect})",
                    &[("what", &what), ("when", &when(&EventTime::DateTime(*until), today)), ("effect", &effect)],
                ),
                None => trf("Mute {what} for good ({effect})", &[("what", &what), ("effect", &effect)]),
            }
        }
        Request::RemoveRule { .. } => match &facts.rule {
            // A muted trait (task E8.6a), named by its words.
            Some(Rule { facet: Some(facet), .. }) => trf(
                "Stop muting {what} in the inbox",
                &[("what", &format!("\"{}\"", quote(&torro_inbox::rules::facet_label(facet))))],
            ),
            // A hidden sender or domain is hidden in every mail account (task E3.11).
            Some(Rule { sender: Some(sender), effect: RuleEffect::Muted, .. }) => {
                trf("Show mail from {sender} in the inbox again", &[("sender", &format!("\"{}\"", quote(sender)))])
            }
            Some(Rule { domain: Some(domain), effect: RuleEffect::Muted, .. }) => trf(
                "Show mail from everyone at {domain} in the inbox again",
                &[("domain", &format!("\"{}\"", quote(domain)))],
            ),
            Some(Rule { source, effect: RuleEffect::Muted, .. }) => {
                trf("Unmute {source} in the inbox", &[("source", &source_name(source))])
            }
            Some(Rule { source, sender: Some(sender), effect: RuleEffect::Lane { lane }, .. }) => trf(
                "Stop sending mail from {sender} of {source} to {lane}",
                &[
                    ("sender", &format!("\"{}\"", quote(sender))),
                    ("source", &source_name(source)),
                    ("lane", &lane_name(*lane)),
                ],
            ),
            Some(Rule { source, effect: RuleEffect::Lane { lane }, .. }) => trf(
                "Stop sending notices of {source} to {lane}",
                &[("source", &source_name(source)), ("lane", &lane_name(*lane))],
            ),
            // Only rules with a trait keep no popups; one without is nothing the core makes.
            Some(Rule { effect: RuleEffect::NoPopup, .. }) | None => tr("Remove an unknown inbox rule").to_string(),
        },
        other => summary(other, &Facts::default()),
    }
}

/// The host a web link opens, as the browser reads it: what follows `://` up to the first `/`,
/// `?`, `#` or backslash (browsers take it for a slash), without a user part (`a@b` opens `b`),
/// in lower case. `None` for anything but `http`/`https` or without a host.
fn web_host(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#', '\\']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    (!host.is_empty()).then(|| host.to_string())
}

/// `what` with how far a muted trait of `key` reaches (task E8.6a): one that can name more than
/// one source holds for every source of the kind, also those the asking key may not see, which the
/// person must know before allowing it (as for a hidden sender, "in every mail account").
fn reach_of(key: FacetKey, what: String) -> String {
    match key {
        FacetKey::Sender | FacetKey::Domain | FacetKey::List | FacetKey::Bulk => {
            trf("{what} in every mail account", &[("what", &what)])
        }
        FacetKey::Project | FacetKey::Kind => trf("{what} for every agent", &[("what", &what)]),
        FacetKey::Account | FacetKey::Agent | FacetKey::Client | FacetKey::Conversation => what,
    }
}

/// What muting does, in a few words for a summary (task E8.6a): "no popup", "only Info", "hidden".
fn mute_effect_name(effect: MuteEffect) -> &'static str {
    match effect {
        MuteEffect::NoPopup => tr("no popup"),
        MuteEffect::Info => tr("only Info"),
        MuteEffect::Hide => tr("hidden"),
    }
}

/// `item` is an entry of a mail account (task E4.1).
fn is_mail(item: Option<&Item>) -> bool {
    item.is_some_and(|item| item.source.kind == torro_inbox::SourceKind::Mail)
}

/// A source for a summary: its label and its key, `"Codex" (agent:codex)`, both quoted.
fn source_name(source: &SourceRef) -> String {
    format!("\"{}\" ({})", quote(&source.label()), quote(&source.key()))
}

/// The name of a lane as the Inbox app shows it.
fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::ForMe => tr("For me"),
        Lane::Info => tr("Info"),
    }
}

/// `"old"` when the title stays, `"old" → "new"` when it changes; quoted, without the outer
/// quotes the sentence puts around it.
fn title_change(old: &str, new: &str) -> (String, Option<String>) {
    let (old, new) = (quote(old), quote(new));
    if old == new { (old, None) } else { (old, Some(new)) }
}

/// The start of an event for a summary, `old → new` when a change moves it.
fn when_change(old: Option<EventTime>, new: &EventTime, today: chrono::NaiveDate) -> String {
    match old {
        Some(old) if old != *new => format!("{} → {}", when(&old, today), when(new, today)),
        Some(old) => when(&old, today),
        None => when(new, today),
    }
}

/// The summary of `calendar.save_event`: create, change, or move to another calendar.
fn event_saved(event: &Event, facts: &Facts, today: chrono::NaiveDate) -> String {
    let calendar = quote(facts.name.as_deref().unwrap_or(&event.collection));
    let new = quote(&event.summary);
    if event.href.is_none() {
        return trf(
            "Create event \"{title}\" on {when} in \"{calendar}\"",
            &[("title", &new), ("when", &when(&event.start, today)), ("calendar", &calendar)],
        );
    }
    let Some(item) = &facts.item else {
        return match &facts.moved_from {
            Some(from) => trf(
                "Move an unknown event from \"{from}\" to \"{calendar}\"",
                &[("from", &quote(from)), ("calendar", &calendar)],
            ),
            None => trf(
                "Change an unknown event in \"{calendar}\" to \"{new}\" on {when}",
                &[("calendar", &calendar), ("new", &new), ("when", &when(&event.start, today))],
            ),
        };
    };
    let when = when_change(item.start, &event.start, today);
    let (title, renamed) = title_change(&item.title, &event.summary);
    let from = facts.moved_from.as_deref().map(quote).unwrap_or_default();
    let renamed_to = renamed.as_deref().unwrap_or_default();
    let args: [(&str, &dyn std::fmt::Display); 5] =
        [("title", &title), ("new", &renamed_to), ("when", &when), ("from", &from), ("calendar", &calendar)];
    let text = match (renamed.is_some(), facts.moved_from.is_some()) {
        (false, false) => "Change event \"{title}\" on {when} in \"{calendar}\"",
        (true, false) => "Change event \"{title}\" → \"{new}\" on {when} in \"{calendar}\"",
        (false, true) => "Move event \"{title}\" on {when} from \"{from}\" to \"{calendar}\"",
        (true, true) => "Move event \"{title}\" → \"{new}\" on {when} from \"{from}\" to \"{calendar}\"",
    };
    trf(text, &args)
}

/// The summary of `tasks.save_todo`: create, change, or move to another list.
fn todo_saved(todo: &Todo, facts: &Facts) -> String {
    let list = quote(facts.name.as_deref().unwrap_or(&todo.collection));
    let new = quote(&todo.summary);
    if todo.href.is_none() {
        return trf("Create task \"{title}\" in \"{list}\"", &[("title", &new), ("list", &list)]);
    }
    // Ticking an issue of a filter list is the only change that runs there: it closes or reopens
    // the issue on GitHub, whatever title the client sent (G.5b). Ticking a link does the same,
    // and when the save changes more of the task, the summary says that too (G.6b).
    let Some(issue) = &facts.issue else { return task_changed(todo, facts, &list, &new) };
    if let (Some(item), true) = (&facts.item, facts.ticks_only) {
        let args: [(&str, &dyn std::fmt::Display); 2] = [("title", &quote(&item.title)), ("issue", &quote(issue))];
        return match todo.completed {
            true => trf("Close GitHub issue \"{title}\" ({issue})", &args),
            false => trf("Reopen GitHub issue \"{title}\" ({issue})", &args),
        };
    }
    let args: [(&str, &dyn std::fmt::Display); 2] =
        [("change", &task_changed(todo, facts, &list, &new)), ("issue", &quote(issue))];
    match todo.completed {
        true => trf("{change} and close GitHub issue {issue}", &args),
        false => trf("{change} and reopen GitHub issue {issue}", &args),
    }
}

/// The summary of `tasks.save_todo` changing or moving a stored task (`facts.item`), in the list
/// `list` with the title `new`, both quoted.
fn task_changed(todo: &Todo, facts: &Facts, list: &str, new: &str) -> String {
    let Some(item) = &facts.item else {
        return match &facts.moved_from {
            Some(from) => {
                trf("Move an unknown task from \"{from}\" to \"{list}\"", &[("from", &quote(from)), ("list", &list)])
            }
            None => trf("Change an unknown task in \"{list}\" to \"{new}\"", &[("list", &list), ("new", &new)]),
        };
    };
    let (title, renamed) = title_change(&item.title, &todo.summary);
    let from = facts.moved_from.as_deref().map(quote).unwrap_or_default();
    let renamed_to = renamed.as_deref().unwrap_or_default();
    let args: [(&str, &dyn std::fmt::Display); 4] =
        [("title", &title), ("new", &renamed_to), ("from", &from), ("list", &list)];
    let text = match (renamed.is_some(), facts.moved_from.is_some()) {
        (false, false) => "Change task \"{title}\" in \"{list}\"",
        (true, false) => "Change task \"{title}\" → \"{new}\" in \"{list}\"",
        (false, true) => "Move task \"{title}\" from \"{from}\" to \"{list}\"",
        (true, true) => "Move task \"{title}\" → \"{new}\" from \"{from}\" to \"{list}\"",
    };
    trf(text, &args)
}

/// The start of an event for a summary: `Tue, 14 Oct`, with the local time for a timed one.
fn when(start: &EventTime, today: chrono::NaiveDate) -> String {
    match start {
        EventTime::Date(date) => format_short_date(*date, today),
        EventTime::DateTime(at) => {
            let local = to_local(*at);
            format!("{} {}", format_short_date(local.date(), today), format_time(local.time()))
        }
    }
}

/// What `accounts.update_mail_account` changes, in words, for its summary (task M2.1): each field
/// given, new names and the start of a new signature quoted, the services by what is switched on
/// (switching one on widens what agents see), each folder role with the path it gets
/// (`Sent → "INBOX.Sent"`, the path quoted: it decides where sent mail goes). "nothing" when no
/// field is given.
fn mail_account_changes(
    sender_name: Option<&str>,
    signature: Option<&str>,
    notify: bool,
    services: Option<&torro_mail::Services>,
    folder_roles: Option<&BTreeMap<Role, String>>,
) -> String {
    let mut changes = Vec::new();
    match sender_name {
        Some("") => changes.push(tr("no sender name").to_string()),
        Some(name) => changes.push(trf("sender name \"{name}\"", &[("name", &quote(name))])),
        None => {}
    }
    match signature {
        Some("") => changes.push(tr("no signature").to_string()),
        Some(text) => changes.push(trf("signature \"{start}\"", &[("start", &quote(text))])),
        None => {}
    }
    if notify {
        changes.push(tr("inbox notifications").to_string());
    }
    if let Some(services) = services {
        let on: Vec<&str> =
            [(services.mail, tr("mail")), (services.calendars, tr("calendars")), (services.tasks, tr("tasks"))]
                .into_iter()
                .filter_map(|(on, name)| on.then_some(name))
                .collect();
        let list = if on.is_empty() { tr("none").to_string() } else { on.join(", ") };
        changes.push(trf("services: {list}", &[("list", &list)]));
    }
    for (role, path) in folder_roles.into_iter().flatten() {
        changes.push(trf("{role} → \"{path}\"", &[("role", &role_word(*role)), ("path", &quote(path))]));
    }
    if changes.is_empty() {
        return tr("nothing").to_string();
    }
    changes.join(", ")
}

/// The messages a mail command names, from what the store holds ([`MailFacts`]): one known
/// message by its subject, several by their number and the first subject, else "an unknown
/// message" or "<n> unknown messages". `count` is how many ids the request names.
fn mail_messages(facts: Option<&MailFacts>, count: usize) -> String {
    let first = facts.and_then(|facts| facts.subjects.first());
    match (count, first) {
        (1, Some(first)) => subject(first),
        (_, Some(first)) => trf("{n} messages, among them {subject}", &[("n", &count), ("subject", &subject(first))]),
        (1, None) => tr("an unknown message").to_string(),
        (_, None) => trf("{n} unknown messages", &[("n", &count)]),
    }
}

/// The summary of sending `draft` from the account with the address `account` (task M3.3, rules
/// at [`MailFacts`]): subject, account, first recipient and how many more, attachments, the start
/// of the text.
fn draft_sent(draft: &DraftFacts, account: &str) -> String {
    let first = quote(draft.first_recipient.as_deref().unwrap_or_default());
    let recipients = match draft.recipients.saturating_sub(1) {
        0 => first,
        more => trf("{first} and {n} more", &[("first", &first), ("n", &more)]),
    };
    let mut said = trf(
        "Send {subject} from {account} to {recipients}",
        &[("subject", &subject(&draft.subject)), ("account", &quote(account)), ("recipients", &recipients)],
    );
    match draft.attachments {
        0 => {}
        1 => said = trf("{summary}, 1 attachment", &[("summary", &said)]),
        n => said = trf("{summary}, {n} attachments", &[("summary", &said), ("n", &n)]),
    }
    if !draft.bcc.is_empty() {
        let shown: Vec<String> = draft.bcc.iter().take(BCC_SHOWN).map(|address| quote(address)).collect();
        let list = match draft.bcc.len().saturating_sub(BCC_SHOWN) {
            0 => shown.join(", "),
            more => trf("{first} and {n} more", &[("first", &shown.join(", ")), ("n", &more)]),
        };
        said = trf("{summary}, Bcc: {list}", &[("summary", &said), ("list", &list)]);
    }
    match quote(&draft.text_start) {
        empty if empty.is_empty() => said,
        text => format!("{said}: \"{text}\""),
    }
}

/// The longest piece of text from outside in a line of the details ([`details`]), in characters:
/// room for a long subject, an address and the start of the text (`mime` keeps 200 of it).
pub const DETAIL_MAX: usize = 200;

/// What the summary of `request` leaves out, one line each, in the core's language (task M3.5,
/// [`ApprovalInfo::details`]); empty for every command but `mail.send_draft` and for a draft the
/// core does not hold. For sending, from [`MailFacts::draft`], in this order:
///
/// - `Subject: "<subject>"` ("(no subject)" for none);
/// - `To: <address>` per recipient, `<address> ("<name>")` with a name, then `Cc: …`, then
///   `Bcc: <address>`: every recipient, the address first, so a name cannot hide where it goes;
/// - `Attachment: "<file name>", <size>` per attachment ("12 KB");
/// - `Text: "<start of the text>"` unless it is empty.
///
/// Every text from the draft is made harmless as in the summary ([`quote`]: one line, no format
/// characters, no double quotes, no braces), but cut at [`DETAIL_MAX`] characters instead of 60.
/// The person sees all of it, so nothing is left out but the end of a long text.
pub fn details(request: &Request, facts: &Facts) -> Vec<String> {
    let Request::SendDraft { .. } = request else { return Vec::new() };
    let Some(draft) = facts.mail.as_ref().and_then(|mail| mail.draft.as_ref()) else { return Vec::new() };
    let long = |text: &str| quote_up_to(text, DETAIL_MAX);
    let recipient = |who: &MailAddress| match who.name.as_deref().map(long).filter(|name| !name.is_empty()) {
        Some(name) => format!("{} (\"{name}\")", long(&who.address)),
        None => long(&who.address),
    };
    let subject = match long(&draft.subject) {
        empty if empty.is_empty() => tr("(no subject)").to_string(),
        quoted => format!("\"{quoted}\""),
    };
    let mut lines = vec![trf("Subject: {subject}", &[("subject", &subject)])];
    lines.extend(draft.to.iter().map(|who| trf("To: {recipient}", &[("recipient", &recipient(who))])));
    lines.extend(draft.cc.iter().map(|who| trf("Cc: {recipient}", &[("recipient", &recipient(who))])));
    lines.extend(draft.bcc.iter().map(|address| trf("Bcc: {recipient}", &[("recipient", &long(address))])));
    lines.extend(draft.files.iter().map(|file| {
        let name = format!("\"{}\"", long(&file.name));
        trf("Attachment: {name}, {size}", &[("name", &name), ("size", &size_words(file.size))])
    }));
    match long(&draft.text_start) {
        empty if empty.is_empty() => {}
        text => lines.push(trf("Text: {text}", &[("text", &format!("\"{text}\""))])),
    }
    lines
}

/// A size in bytes as people read it: "860 B", "3 KB", "12 MB" (rounded up, as the inbox app
/// writes it).
fn size_words(bytes: u64) -> String {
    const KB: u64 = 1024;
    if bytes < KB {
        trf("{n} B", &[("n", &bytes)])
    } else if bytes < KB * KB {
        trf("{n} KB", &[("n", &bytes.div_ceil(KB))])
    } else {
        trf("{n} MB", &[("n", &bytes.div_ceil(KB * KB))])
    }
}

/// A subject quoted for a summary ([`quote`]); "(no subject)" for an empty one.
fn subject(text: &str) -> String {
    match quote(text) {
        empty if empty.is_empty() => tr("(no subject)").to_string(),
        quoted => format!("\"{quoted}\""),
    }
}

/// What `mail.set_flags` does, in words: "read, not flagged". "unchanged" when nothing changes.
fn flag_words(add: &[torro_mail::Flag], remove: &[torro_mail::Flag]) -> String {
    use torro_mail::Flag;
    let set = add.iter().map(|flag| match flag {
        Flag::Seen => tr("read"),
        Flag::Flagged => tr("flagged"),
        Flag::Answered => tr("answered"),
    });
    let cleared = remove.iter().map(|flag| match flag {
        Flag::Seen => tr("unread"),
        Flag::Flagged => tr("not flagged"),
        Flag::Answered => tr("not answered"),
    });
    let words: Vec<&str> = set.chain(cleared).collect();
    if words.is_empty() { tr("unchanged").to_string() } else { words.join(", ") }
}

/// The word for a folder role, as the inbox app names the folders.
fn role_word(role: Role) -> &'static str {
    match role {
        Role::Inbox => tr("Inbox"),
        Role::Drafts => tr("Drafts"),
        Role::Sent => tr("Sent"),
        Role::Archive => tr("Archive"),
        Role::Junk => tr("Junk"),
        Role::Trash => tr("Trash"),
    }
}

/// A client's text made harmless for a summary: invisible format characters go ([`is_format`]),
/// control characters and line breaks become spaces, runs of spaces one, double quotes single ones
/// (so the text cannot end the quoted part and pretend to be the core's words), braces round ones
/// (so it cannot fill a placeholder), and at most [`QUOTE_MAX`] characters with `…`.
fn quote(text: &str) -> String {
    quote_up_to(text, QUOTE_MAX)
}

/// [`quote`] cut at `max` characters instead of [`QUOTE_MAX`].
fn quote_up_to(text: &str, max: usize) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !is_format(*c))
        .map(|c| match c {
            '"' | '“' | '”' | '„' | '‟' | '＂' => '\'',
            '{' => '(',
            '}' => ')',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let one_line = words.join(" ");
    match one_line.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &one_line[..cut]),
        None => one_line,
    }
}

/// A Unicode format character (general category Cf, Unicode 16): invisible, yet able to change
/// what a person reads. Bidi controls turn `Delete "gnihton"` around on screen, zero-width ones
/// join or split words unseen, tag characters carry hidden text. Rust's standard library has no
/// category lookup, so the ranges are listed here.
fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use chrono::TimeZone;

    use super::*;

    /// Approvals in `dir/state/approvals.json` that show no notification.
    pub(crate) fn quiet_in(dir: &Path) -> Approvals {
        Approvals::new(Some(file_in(dir)), Notifier::asking(|_: &ApprovalInfo| {}))
    }

    pub(crate) fn file_in(dir: &Path) -> PathBuf {
        dir.join("state").join(FILE)
    }

    /// What a fake notifier saw: the ids asked about and the ids settled, in order.
    #[derive(Default)]
    struct Seen {
        asked: Mutex<Vec<String>>,
        settled: Mutex<Vec<String>>,
    }

    /// Approvals in `dir` with a notifier that only records into the returned [`Seen`].
    fn recorded_in(dir: &Path) -> (Approvals, Arc<Seen>) {
        let seen = Arc::new(Seen::default());
        let (asked, settled) = (Arc::clone(&seen), Arc::clone(&seen));
        let notify = Notifier::new(
            move |info: &ApprovalInfo| asked.asked.lock().expect("lock").push(info.id.clone()),
            move |id: &str| settled.settled.lock().expect("lock").push(id.to_string()),
        );
        (Approvals::new(Some(file_in(dir)), notify), seen)
    }

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 9, minute, 0).single().expect("time")
    }

    fn delete(id: &str) -> Request {
        Request::DeleteTemplate { id: id.into() }
    }

    #[tokio::test]
    async fn an_approval_survives_a_restart_and_expires_after_ten_minutes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let opened =
            approvals.open("c1", "Claude Code", delete("dev"), None, "Delete".into(), at(0)).await.expect("opened");
        assert_eq!(opened.state, ApprovalState::Pending);
        assert_eq!(opened.expires, at(10));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(file_in(dir.path())).expect("written").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // A new core reads it back, still open a minute before it expires.
        let restarted = quiet_in(dir.path());
        assert_eq!(restarted.status(Some("c1"), &opened.id, at(9)).await.expect("kept"), opened);
        let expired = restarted.status(Some("c1"), &opened.id, at(10)).await.expect("kept");
        assert_eq!((expired.state, expired.decided), (ApprovalState::Expired, Some(at(10))));
        assert!(matches!(restarted.decide(&opened.id, true, at(11)).await, Err(ApprovalsError::NotOpen(_))));

        // A day after it expired it is gone.
        let later = at(10) + KEPT;
        assert!(matches!(restarted.status(None, &opened.id, later).await, Err(ApprovalsError::NotFound(_))));
    }

    #[tokio::test]
    async fn a_client_sees_only_its_own_approvals_and_the_app_key_all() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let mine = approvals.open("c1", "One", delete("a"), None, "a".into(), at(0)).await.expect("opened");
        let theirs = approvals.open("c2", "Two", delete("b"), None, "b".into(), at(1)).await.expect("opened");

        let ids = |list: Vec<ApprovalInfo>| list.into_iter().map(|info| info.id).collect::<Vec<_>>();
        assert_eq!(ids(approvals.list(Some("c1"), at(2)).await.expect("list")), std::slice::from_ref(&mine.id));
        assert_eq!(ids(approvals.list(None, at(2)).await.expect("list")), [theirs.id.clone(), mine.id], "newest first");
        assert!(matches!(approvals.status(Some("c1"), &theirs.id, at(2)).await, Err(ApprovalsError::NotFound(_))));
    }

    #[tokio::test]
    async fn a_decision_is_taken_once_and_the_outcome_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let first = approvals.open("c1", "One", delete("a"), None, "a".into(), at(0)).await.expect("opened");
        let second = approvals.open("c1", "One", delete("b"), None, "b".into(), at(0)).await.expect("opened");

        let Ok(Decision::Run { client, request, pin, fingerprint }) = approvals.decide(&first.id, true, at(1)).await
        else {
            panic!("allowed runs");
        };
        assert_eq!((client.as_str(), *request, pin, fingerprint), ("c1", delete("a"), None, None));
        assert_eq!(approvals.status(None, &first.id, at(1)).await.expect("kept").state, ApprovalState::Running);
        let done = approvals.finish(&first.id, Response::Ok, at(1)).await.expect("finished");
        assert_eq!(done.state, ApprovalState::Done { response: Box::new(Response::Ok) });
        assert!(matches!(approvals.decide(&first.id, false, at(2)).await, Err(ApprovalsError::NotOpen(_))));

        let Ok(Decision::Denied { approval: denied, request }) = approvals.decide(&second.id, false, at(2)).await
        else {
            panic!("denied")
        };
        assert_eq!((denied.state, denied.decided), (ApprovalState::Denied, Some(at(2))));
        assert_eq!(*request, delete("b"), "what was asked for, for the audit log");
        assert!(matches!(approvals.decide("nope", true, at(2)).await, Err(ApprovalsError::NotFound(_))));

        // Outcomes survive a restart too.
        let restarted = quiet_in(dir.path());
        assert_eq!(restarted.status(None, &first.id, at(3)).await.expect("kept"), done);
    }

    #[tokio::test]
    async fn a_command_running_when_the_core_stopped_never_runs_again() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let opened = approvals.open("c1", "One", delete("a"), None, "a".into(), at(0)).await.expect("opened");
        assert!(matches!(approvals.decide(&opened.id, true, at(1)).await, Ok(Decision::Run { .. })));

        let restarted = quiet_in(dir.path());
        let info = restarted.status(None, &opened.id, at(2)).await.expect("kept");
        let ApprovalState::Done { response } = info.state else { panic!("done, got {:?}", info.state) };
        assert!(matches!(*response, Response::Error { ref code, .. } if code == "error"));
    }

    #[tokio::test]
    async fn a_secret_taking_command_never_becomes_an_approval() {
        let dir = tempfile::tempdir().expect("temp dir");
        let shown = Arc::new(Mutex::new(0));
        let counter = Arc::clone(&shown);
        let notify = Notifier::asking(move |_: &ApprovalInfo| *counter.lock().expect("lock") += 1);
        let approvals = Approvals::new(Some(file_in(dir.path())), notify);

        let token = Request::SetGithubToken { token: torro_ipc::Secret::new("ghp_secret") };
        let refused = approvals.open("c1", "One", token, None, "x".into(), at(0)).await;
        assert!(matches!(refused, Err(ApprovalsError::KeptBack("accounts.set_github_token"))));
        let decide = Request::DecideApproval { id: "x".into(), allow: true };
        assert!(matches!(
            approvals.open("c1", "One", decide, None, "x".into(), at(0)).await,
            Err(ApprovalsError::KeptBack(_))
        ));
        assert!(!file_in(dir.path()).exists(), "nothing stored");
        assert_eq!(*shown.lock().expect("lock"), 0, "nothing announced");

        approvals.open("c1", "One", delete("a"), None, "a".into(), at(0)).await.expect("opened");
        assert_eq!(*shown.lock().expect("lock"), 1, "a new approval is announced once");
        let stored = fs::read_to_string(file_in(dir.path())).expect("stored");
        assert!(!stored.contains("ghp_secret"));
    }

    #[tokio::test]
    async fn a_client_cannot_flood_the_person_with_questions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        for n in 0..OPEN_MAX {
            approvals.open("c1", "One", delete(&n.to_string()), None, "x".into(), at(0)).await.expect("opened");
        }
        let refused = approvals.open("c1", "One", delete("more"), None, "x".into(), at(0)).await;
        assert!(matches!(refused, Err(ApprovalsError::TooMany)));
        assert_eq!(ApprovalsError::TooMany.code(), "busy");
        approvals.open("c2", "Two", delete("other"), None, "x".into(), at(0)).await.expect("other clients still ask");
        // Once they expired it may ask again.
        approvals.open("c1", "One", delete("later"), None, "x".into(), at(10)).await.expect("opened after expiry");
    }

    #[tokio::test]
    async fn an_approval_that_cannot_be_stored_is_not_opened() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A file where the state directory should be: nothing can be written below it.
        fs::write(dir.path().join("state"), "not a directory").expect("blocker");
        let approvals = quiet_in(dir.path());
        let refused = approvals.open("c1", "One", delete("a"), None, "a".into(), at(0)).await;
        assert!(matches!(refused, Err(ApprovalsError::Io { .. })), "{refused:?}");
    }

    #[tokio::test]
    async fn a_damaged_file_is_moved_aside_and_approvals_go_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::create_dir_all(dir.path().join("state")).expect("dir");
        fs::write(file_in(dir.path()), "{ not json").expect("damage");
        let approvals = quiet_in(dir.path());
        assert!(approvals.list(None, at(0)).await.expect("list").is_empty());
        assert!(dir.path().join("state").join("approvals.json.damaged").exists());
        approvals.open("c1", "One", delete("a"), None, "a".into(), at(0)).await.expect("opened");
    }

    #[tokio::test]
    async fn open_approvals_are_asked_about_again_when_the_core_starts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let early = approvals.open("c1", "One", delete("a"), None, "a".into(), at(0)).await.expect("opened");
        let open = approvals.open("c1", "One", delete("b"), None, "b".into(), at(5)).await.expect("opened");
        let decided = approvals.open("c1", "One", delete("c"), None, "c".into(), at(5)).await.expect("opened");
        approvals.decide(&decided.id, false, at(6)).await.expect("denied");

        // A new core asks again about the one still open; `early` expired at minute 10.
        let (restarted, seen) = recorded_in(dir.path());
        assert_eq!(restarted.announce_open(at(11)).await.expect("announced"), 1);
        assert_eq!(*seen.asked.lock().expect("lock"), std::slice::from_ref(&open.id));
        assert_ne!(early.id, open.id);
        assert_eq!(restarted.announce_open(at(15)).await.expect("announced"), 0, "expired too by now");

        // Without a file there is nothing to ask about, and nothing fails.
        assert_eq!(Approvals::system().announce_open(at(0)).await.expect("nothing"), 0);
    }

    #[tokio::test]
    async fn a_decision_takes_the_question_off_the_desktop() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (approvals, seen) = recorded_in(dir.path());
        let first = approvals.open("c1", "One", delete("a"), None, "a".into(), at(0)).await.expect("opened");
        let second = approvals.open("c1", "One", delete("b"), None, "b".into(), at(0)).await.expect("opened");
        assert_eq!(*seen.asked.lock().expect("lock"), [first.id.clone(), second.id.clone()]);
        assert!(seen.settled.lock().expect("lock").is_empty());

        approvals.decide(&second.id, true, at(1)).await.expect("allowed");
        approvals.decide(&first.id, false, at(1)).await.expect("denied");
        assert_eq!(*seen.settled.lock().expect("lock"), [second.id.clone(), first.id.clone()]);
        // One that is no longer open settles nothing again.
        assert!(approvals.decide(&first.id, true, at(2)).await.is_err());
        assert_eq!(seen.settled.lock().expect("lock").len(), 2);
    }

    #[tokio::test]
    async fn a_client_without_a_name_is_shown_by_its_id() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let nameless = approvals.open("c1", " ", delete("a"), None, "a".into(), at(0)).await.expect("opened");
        assert_eq!(nameless.client_name, "c1");
        let named = approvals.open("c1", "Claude Code", delete("b"), None, "b".into(), at(0)).await.expect("opened");
        assert_eq!(named.client_name, "Claude Code");
    }

    #[test]
    fn file_errors_reach_the_client_without_a_path() {
        let err = ApprovalsError::Io {
            path: PathBuf::from("/home/anna/.local/state/torroworkplace/approvals.json"),
            err: io::Error::other("disk full"),
        };
        let Response::Error { code, message } = err.response() else { panic!("an error") };
        assert_eq!(code, "error");
        assert!(!message.contains("/home/anna") && !message.contains("approvals.json"), "{message}");
        assert!(ApprovalsError::NotFound("x".into()).to_string().contains("\"x\""), "other errors stay as they are");
    }

    #[test]
    fn client_text_in_a_summary_stays_one_harmless_line() {
        assert_eq!(quote("Team\nmeeting \"now\"  {when}"), "Team meeting 'now' (when)");
        let long = "x".repeat(100);
        assert_eq!(quote(&long), format!("{}…", "x".repeat(QUOTE_MAX)));
        assert_eq!(quote("äöü"), "äöü");
        assert_eq!(quote("Ａ＂Ｂ"), "Ａ'Ｂ", "a fullwidth quote ends nothing either");
    }

    #[test]
    fn invisible_format_characters_are_removed() {
        // Bidi controls would show "Delete 'gnihton'" turned around; each one goes.
        for control in ['\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}']
        {
            assert_eq!(quote(&format!("Lunch{control}gnihton\u{2069}")), "Lunchgnihton", "{:04X}", u32::from(control));
        }
        // Zero-width characters, joiners and marks, the byte order mark, soft hyphen, tag text.
        for hidden in ['\u{200B}', '\u{200C}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{2060}', '\u{FEFF}', '\u{00AD}'] {
            assert_eq!(quote(&format!("Team{hidden}meeting")), "Teammeeting", "{:04X}", u32::from(hidden));
        }
        assert_eq!(quote("ok\u{E0001}\u{E0069}\u{E0067}\u{E007F}"), "ok");
        assert_eq!(quote("\u{200B}\u{200B}"), "", "nothing visible stays nothing");
        // Visible text that only looks unusual stays.
        assert_eq!(quote("Café ☕ 東京"), "Café ☕ 東京");
    }

    fn event(collection: &str, href: Option<&str>, title: &str, start: EventTime) -> Event {
        Event {
            uid: "u".into(),
            collection: collection.into(),
            href: href.map(str::to_string),
            etag: Some("1".into()),
            summary: title.into(),
            start,
            end: None,
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
        }
    }

    fn todo(collection: &str, href: Option<&str>, title: &str) -> Todo {
        Todo {
            uid: "t".into(),
            collection: collection.into(),
            href: href.map(str::to_string),
            etag: Some("1".into()),
            summary: title.into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
        }
    }

    fn day(day: u32) -> EventTime {
        EventTime::Date(chrono::NaiveDate::from_ymd_opt(2026, 10, day).expect("date"))
    }

    fn cached(title: &str, start: Option<EventTime>) -> Option<Known> {
        Some(Known { title: title.into(), start })
    }

    fn facts(item: Option<Known>, name: &str) -> Facts {
        Facts { item, name: Some(name.into()), ..Facts::default() }
    }

    #[test]
    fn a_summary_names_the_item_that_runs_not_the_one_the_client_claims() {
        let today = Local::now().date_naive();
        let claimed = event("c", Some("/cal/board.ics"), "Lunch", day(20));
        let real = facts(cached("Board meeting", Some(day(14))), "Work");
        assert_eq!(
            summary(&Request::DeleteEvent { event: claimed.clone() }, &real),
            trf(
                "Delete event \"{title}\" on {when} from \"{calendar}\"",
                &[("title", &"Board meeting"), ("when", &when(&day(14), today)), ("calendar", &"Work")]
            )
        );
        let unknown = summary(&Request::DeleteEvent { event: claimed }, &facts(None, "Work"));
        assert_eq!(unknown, trf("Delete an unknown event from \"{calendar}\"", &[("calendar", &"Work")]));
        assert!(!unknown.contains("Lunch"));

        let claimed = todo("l", Some("/tasks/taxes.ics"), "Water plants");
        let real = facts(cached("File taxes", None), "Home");
        assert_eq!(
            summary(&Request::DeleteTodo { todo: claimed.clone() }, &real),
            trf("Delete task \"{title}\" from \"{list}\"", &[("title", &"File taxes"), ("list", &"Home")])
        );
        assert_eq!(
            summary(&Request::DeleteTodo { todo: claimed }, &facts(None, "Home")),
            trf("Delete an unknown task from \"{list}\"", &[("list", &"Home")])
        );
        let reorder = Request::MoveTodo { href: "/tasks/taxes.ics".into(), etag: "1".into(), before: None };
        assert_eq!(
            summary(&reorder, &real),
            trf("Reorder task \"{title}\" in \"{list}\"", &[("title", &"File taxes"), ("list", &"Home")])
        );
        assert_eq!(summary(&reorder, &Facts::default()), tr("Reorder an unknown task"));
    }

    /// `apps.open_app` by window class (task E8.1): the summary names the window as the suite does.
    #[test]
    fn opening_an_app_names_it_as_the_suite_does() {
        let open = |app: &str| summary(&Request::OpenApp { app: app.into() }, &Facts::default());
        assert_eq!(open("torro-inbox-mail"), trf("Open {app}", &[("app", &quote("Torro Inbox Mail"))]));
        assert_eq!(open("torrocalendar"), trf("Open {app}", &[("app", &quote("Torro Calendar"))]));
    }

    #[test]
    fn a_change_shows_old_and_new() {
        let today = Local::now().date_naive();
        let changed = event("c", Some("/cal/board.ics"), "Board meeting (moved)", day(15));
        let real = facts(cached("Board meeting", Some(day(14))), "Work");
        let moved_day = format!("{} → {}", when(&day(14), today), when(&day(15), today));
        assert_eq!(
            summary(&Request::SaveEvent { event: changed.clone() }, &real),
            trf(
                "Change event \"{title}\" → \"{new}\" on {when} in \"{calendar}\"",
                &[
                    ("title", &"Board meeting"),
                    ("new", &"Board meeting (moved)"),
                    ("when", &moved_day),
                    ("calendar", &"Work")
                ]
            )
        );
        let same = event("c", Some("/cal/board.ics"), "Board meeting", day(14));
        assert_eq!(
            summary(&Request::SaveEvent { event: same.clone() }, &real),
            trf(
                "Change event \"{title}\" on {when} in \"{calendar}\"",
                &[("title", &"Board meeting"), ("when", &when(&day(14), today)), ("calendar", &"Work")]
            )
        );
        let moving = Facts { moved_from: Some("Home".into()), ..real.clone() };
        assert_eq!(
            summary(&Request::SaveEvent { event: same.clone() }, &moving),
            trf(
                "Move event \"{title}\" on {when} from \"{from}\" to \"{calendar}\"",
                &[
                    ("title", &"Board meeting"),
                    ("when", &when(&day(14), today)),
                    ("from", &"Home"),
                    ("calendar", &"Work")
                ]
            )
        );
        assert_eq!(
            summary(&Request::SaveEvent { event: same }, &facts(None, "Work")),
            trf(
                "Change an unknown event in \"{calendar}\" to \"{new}\" on {when}",
                &[("calendar", &"Work"), ("new", &"Board meeting"), ("when", &when(&day(14), today))]
            )
        );
        let new = event("c", None, "Lunch", day(14));
        assert_eq!(
            summary(&Request::SaveEvent { event: new }, &facts(None, "Work")),
            trf(
                "Create event \"{title}\" on {when} in \"{calendar}\"",
                &[("title", &"Lunch"), ("when", &when(&day(14), today)), ("calendar", &"Work")]
            )
        );

        let renamed = todo("l", Some("/tasks/taxes.ics"), "File taxes today");
        let real = facts(cached("File taxes", None), "Home");
        assert_eq!(
            summary(&Request::SaveTodo { todo: renamed.clone() }, &real),
            trf(
                "Change task \"{title}\" → \"{new}\" in \"{list}\"",
                &[("title", &"File taxes"), ("new", &"File taxes today"), ("list", &"Home")]
            )
        );
        let moving = Facts { moved_from: Some("Work".into()), ..real };
        assert_eq!(
            summary(&Request::SaveTodo { todo: renamed }, &moving),
            trf(
                "Move task \"{title}\" → \"{new}\" from \"{from}\" to \"{list}\"",
                &[("title", &"File taxes"), ("new", &"File taxes today"), ("from", &"Work"), ("list", &"Home")]
            )
        );
    }

    #[test]
    fn ticking_a_github_issue_says_close_or_reopen_with_the_cached_title() {
        let issue = |done: bool, summary: &str| Request::SaveTodo {
            todo: Todo { completed: done, ..todo("github:torro/filter", Some("github:torro/app#12"), summary) },
        };
        let known = Facts {
            item: cached("Crash on start", None),
            name: Some("Open bugs".into()),
            issue: Some("torro/app#12".into()),
            ticks_only: true,
            ..Facts::default()
        };
        let args: [(&str, &dyn std::fmt::Display); 2] = [("title", &"Crash on start"), ("issue", &"torro/app#12")];
        assert_eq!(
            summary(&issue(true, "What the client says"), &known),
            trf("Close GitHub issue \"{title}\" ({issue})", &args)
        );
        assert_eq!(
            summary(&issue(false, "What the client says"), &known),
            trf("Reopen GitHub issue \"{title}\" ({issue})", &args)
        );

        // A link ticked with other changes (task G.6b): the change of the task, then the issue.
        let more = Facts { ticks_only: false, ..known };
        let change: [(&str, &dyn std::fmt::Display); 3] =
            [("title", &"Crash on start"), ("new", &"Renamed"), ("list", &"Open bugs")];
        let change = trf("Change task \"{title}\" → \"{new}\" in \"{list}\"", &change);
        let both: [(&str, &dyn std::fmt::Display); 2] = [("change", &change), ("issue", &"torro/app#12")];
        assert_eq!(summary(&issue(true, "Renamed"), &more), trf("{change} and close GitHub issue {issue}", &both));
        assert_eq!(summary(&issue(false, "Renamed"), &more), trf("{change} and reopen GitHub issue {issue}", &both));
    }

    #[test]
    fn adding_a_link_names_the_item_by_its_address_and_the_list_by_its_name() {
        let add = |url: &str| Request::AddLink { collection: "nextcloud:a/https://x/work/".into(), url: url.into() };
        let work = Facts { name: Some("Work".into()), ..Facts::default() };
        let item: (&str, &dyn std::fmt::Display) = ("item", &"torro/app#12");
        assert_eq!(
            summary(&add("https://github.com/torro/app/issues/12"), &work),
            trf("Add a link to {item} to \"{list}\"", &[item, ("list", &"Work")])
        );
        let bugs = Facts { name: Some("Open bugs".into()), pins: true, ..Facts::default() };
        assert_eq!(
            summary(&add("github:torro/app#12"), &bugs),
            trf("Pin {item} to filter list \"{list}\"", &[item, ("list", &"Open bugs")])
        );
    }

    /// Task M2.1: the account by the name the core knows, the fields that change, the start of a
    /// new signature, and the services by what is switched on.
    #[test]
    fn changing_mail_is_summed_up_from_the_store() {
        use torro_ipc::mail::{MoveRole, MoveTarget};
        use torro_mail::Flag;
        let ids = |n: usize| (0..n).map(|i| format!("m:{i}")).collect::<Vec<_>>();
        let held = |subjects: &[&str], folder: Option<&str>| Facts {
            mail: Some(MailFacts {
                account: Some("anna@mailbox.org".into()),
                count: subjects.len(),
                subjects: subjects.iter().map(|s| s.to_string()).collect(),
                folder: folder.map(str::to_string),
                draft: None,
            }),
            ..Facts::default()
        };
        let hello = subject("Hello");
        assert_eq!(hello, "\"Hello\"");
        let delete = |n, permanent| Request::DeleteMail { ids: ids(n), permanent };
        assert_eq!(
            summary(&delete(1, false), &held(&["Hello"], None)),
            trf("Delete {messages}", &[("messages", &hello)])
        );
        let two = trf("{n} messages, among them {subject}", &[("n", &2), ("subject", &hello)]);
        assert_eq!(
            summary(&delete(2, true), &held(&["Hello", "Bye"], None)),
            trf("Delete {messages} for good", &[("messages", &two)])
        );
        let unknown = trf("{n} unknown messages", &[("n", &3)]);
        assert_eq!(summary(&delete(3, false), &Facts::default()), trf("Delete {messages}", &[("messages", &unknown)]));
        assert_eq!(subject(" "), tr("(no subject)"));

        let to_archive = Request::MoveMail { ids: ids(1), to: MoveTarget::Role(MoveRole::Archive) };
        let named = trf("Move {messages} to \"{folder}\"", &[("messages", &hello), ("folder", &"Archiv")]);
        assert_eq!(summary(&to_archive, &held(&["Hello"], Some("Archiv"))), named);
        let by_role = trf("Move {messages} to \"{folder}\"", &[("messages", &hello), ("folder", &tr("Archive"))]);
        assert_eq!(summary(&to_archive, &held(&["Hello"], None)), by_role);

        let flags = Request::SetMailFlags { ids: ids(1), add: vec![Flag::Seen], remove: vec![Flag::Flagged] };
        let words = format!("{}, {}", tr("read"), tr("not flagged"));
        let marked = trf("Mark {messages} as {flags}", &[("messages", &hello), ("flags", &words)]);
        assert_eq!(summary(&flags, &held(&["Hello"], None)), marked);

        // A new draft names the client's subject (what will be written), a replaced one the stored.
        let draft = |id: Option<&str>| torro_mail::Draft {
            id: id.map(str::to_string),
            account: "mail:anna@mailbox.org".into(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Claimed \"title\"".into(),
            body: String::new(),
            in_reply_to: None,
            attachments: Vec::new(),
        };
        let new = summary(&Request::SaveDraft { draft: draft(None) }, &held(&[], None));
        let said = trf(
            "Save a draft {subject} in {account}",
            &[("subject", &"\"Claimed 'title'\""), ("account", &"anna@mailbox.org")],
        );
        assert_eq!(new, said);
        let stored = Facts {
            mail: Some(MailFacts {
                draft: Some(DraftFacts { subject: "Stored".into(), ..Default::default() }),
                ..Default::default()
            }),
            ..Facts::default()
        };
        let replace = Request::SaveDraft { draft: draft(Some("m:1")) };
        assert_eq!(summary(&replace, &stored), trf("Replace the draft {subject}", &[("subject", &"\"Stored\"")]));
        assert_eq!(summary(&replace, &Facts::default()), tr("Replace an unknown draft"));
    }

    /// Task M3.3: sending names the stored draft: subject, account, first recipient and how many
    /// more (Bcc counted), attachments and the start of the text, all made harmless.
    #[test]
    fn sending_a_draft_is_summed_up_from_the_stored_draft() {
        let send = Request::SendDraft { id: "m:1".into() };
        let held = |draft: DraftFacts| Facts {
            mail: Some(MailFacts {
                account: Some("anna@mailbox.org".into()),
                draft: Some(draft),
                ..MailFacts::default()
            }),
            ..Facts::default()
        };
        let lunch = DraftFacts {
            subject: "Lunch".into(),
            first_recipient: Some("bernd@example.com".into()),
            recipients: 3,
            attachments: 2,
            text_start: "Dear Bernd, \"see\" you {at} noon.".into(),
            ..DraftFacts::default()
        };
        let recipients = trf("{first} and {n} more", &[("first", &"bernd@example.com"), ("n", &2)]);
        let base = trf(
            "Send {subject} from {account} to {recipients}",
            &[("subject", &"\"Lunch\""), ("account", &"anna@mailbox.org"), ("recipients", &recipients)],
        );
        let with_files = trf("{summary}, {n} attachments", &[("summary", &base), ("n", &2)]);
        let expected = format!("{with_files}: \"Dear Bernd, 'see' you (at) noon.\"");
        assert_eq!(summary(&send, &held(lunch.clone())), expected);

        let alone = DraftFacts { recipients: 1, attachments: 1, text_start: String::new(), ..lunch };
        let base = trf(
            "Send {subject} from {account} to {recipients}",
            &[("subject", &"\"Lunch\""), ("account", &"anna@mailbox.org"), ("recipients", &"bernd@example.com")],
        );
        assert_eq!(summary(&send, &held(alone.clone())), trf("{summary}, 1 attachment", &[("summary", &base)]));

        // Every blind copy is named, the first five, then how many more.
        let hidden = |n: usize| (0..n).map(|i| format!("eve{i}@evil.example")).collect::<Vec<_>>();
        let one = DraftFacts { attachments: 0, bcc: hidden(1), ..alone.clone() };
        let said = trf("{summary}, Bcc: {list}", &[("summary", &base), ("list", &"eve0@evil.example")]);
        assert_eq!(summary(&send, &held(one)), said);
        let many = DraftFacts { attachments: 0, bcc: hidden(7), ..alone };
        let five = hidden(5).join(", ");
        let list = trf("{first} and {n} more", &[("first", &five), ("n", &2)]);
        assert_eq!(summary(&send, &held(many)), trf("{summary}, Bcc: {list}", &[("summary", &base), ("list", &list)]));
        assert_eq!(summary(&send, &Facts::default()), tr("Send an unknown draft"));
    }

    /// Task M3.5: the details of sending list every recipient with the address first, every
    /// attachment with its size and the start of the text, made harmless like the summary but
    /// longer; every other command has none.
    #[test]
    fn sending_lists_every_recipient_attachment_and_the_text_in_the_details() {
        let send = Request::SendDraft { id: "m:1".into() };
        let who =
            |name: Option<&str>, address: &str| MailAddress { name: name.map(str::to_string), address: address.into() };
        let long_text = format!("Dear Bernd,\n\u{202E}see {{you}} \"soon\". {}", "x".repeat(300));
        let draft = DraftFacts {
            to: vec![who(Some("Bernd \"Boss\" Example"), "bernd@example.com"), who(None, "dora@example.com")],
            cc: vec![who(Some("  "), "carl@example.org")],
            bcc: vec!["eve@example.net".into()],
            files: vec![
                FileFact { name: "plan.pdf".into(), size: 2_340 },
                FileFact { name: "photo.jpg".into(), size: 3 * 1024 * 1024 },
                FileFact { name: "a.txt".into(), size: 860 },
            ],
            text_start: long_text,
            ..DraftFacts::default()
        };
        let facts = Facts { mail: Some(MailFacts { draft: Some(draft), ..MailFacts::default() }), ..Facts::default() };
        let lines = details(&send, &facts);
        let recipient = |label: &'static str, value: &str| trf(label, &[("recipient", &value)]);
        let file = |name: &str, size: String| trf("Attachment: {name}, {size}", &[("name", &name), ("size", &size)]);
        assert_eq!(
            lines[..8],
            [
                trf("Subject: {subject}", &[("subject", &tr("(no subject)"))]),
                recipient("To: {recipient}", "bernd@example.com (\"Bernd 'Boss' Example\")"),
                recipient("To: {recipient}", "dora@example.com"),
                recipient("Cc: {recipient}", "carl@example.org"),
                recipient("Bcc: {recipient}", "eve@example.net"),
                file("\"plan.pdf\"", trf("{n} KB", &[("n", &3)])),
                file("\"photo.jpg\"", trf("{n} MB", &[("n", &3)])),
                file("\"a.txt\"", trf("{n} B", &[("n", &860)])),
            ]
        );
        let text = &lines[8];
        assert!(text.contains("Dear Bernd, see (you) 'soon'."), "harmless, on one line: {text}");
        assert!(!text.contains('\u{202E}') && !text.contains('\n'));
        assert!(text.ends_with("…\"") && text.chars().count() < DETAIL_MAX + 20, "cut at {DETAIL_MAX}: {text}");
        assert_eq!(lines.len(), 9);

        assert!(details(&send, &Facts::default()).is_empty(), "an unknown draft has none");
        let delete = Request::DeleteTemplate { id: "dev".into() };
        assert!(details(&delete, &facts).is_empty(), "nor has any other command");
    }

    /// Task M3.3: the fingerprint of what the person was shown survives a restart and comes back
    /// with the decision; approvals without one read as before.
    #[tokio::test]
    async fn an_approval_keeps_the_fingerprint_of_what_it_shows() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let send = Request::SendDraft { id: "m:1".into() };
        let asked = approvals
            .open_with_fingerprint(
                "c1",
                "One",
                send.clone(),
                "f00d".into(),
                ("Send".into(), vec!["To: b".into()]),
                at(0),
            )
            .await
            .expect("opened");
        assert_eq!(asked.details, ["To: b"]);
        let restarted = quiet_in(dir.path());
        let kept = restarted.status(Some("c1"), &asked.id, at(1)).await.expect("kept");
        assert_eq!(kept.details, ["To: b"], "the details survive a restart");
        assert!(matches!(restarted.status(Some("c2"), &asked.id, at(1)).await, Err(ApprovalsError::NotFound(_))));
        let Ok(Decision::Run { fingerprint, request, .. }) = restarted.decide(&asked.id, true, at(1)).await else {
            panic!("allowed runs");
        };
        assert_eq!((fingerprint.as_deref(), *request), (Some("f00d"), send));
        let listed = serde_json::to_string(&restarted.list(Some("c1"), at(1)).await.expect("listed")).expect("json");
        assert!(!listed.contains("f00d"), "the client never sees it: {listed}");
    }

    #[test]
    fn changing_a_mail_account_names_it_and_what_changes() {
        let update = |signature: Option<&str>, services| Request::UpdateMailAccount {
            id: "mail:anna@mailbox.org".into(),
            sender_name: None,
            signature: signature.map(str::to_string),
            notify: Some(torro_mail::Notify::Off),
            services,
            folder_roles: None,
        };
        let anna = Facts { name: Some("anna@mailbox.org".into()), ..Facts::default() };
        let long = format!("Best regards\n{}", "x".repeat(100));
        let said = summary(&update(Some(&long), None), &anna);
        let start = quote(&long);
        let changes = format!("{}, {}", trf("signature \"{start}\"", &[("start", &start)]), tr("inbox notifications"));
        assert_eq!(
            said,
            trf(
                "Change the mail account {account}: {changes}",
                &[("account", &"anna@mailbox.org"), ("changes", &changes)]
            )
        );
        assert!(!said.contains('\n') && start.ends_with('…'), "one line, the start only: {said}");

        let services = torro_mail::Services { mail: true, calendars: true, tasks: false };
        let said = summary(&update(Some(""), Some(services)), &Facts::default());
        let list = format!("{}, {}", tr("mail"), tr("calendars"));
        let changes = [
            tr("no signature").to_string(),
            tr("inbox notifications").to_string(),
            trf("services: {list}", &[("list", &list)]),
        ]
        .join(", ");
        let account: (&str, &dyn std::fmt::Display) = ("account", &"mail:anna@mailbox.org");
        assert_eq!(said, trf("Change the mail account {account}: {changes}", &[account, ("changes", &changes)]));
        assert_eq!(mail_account_changes(None, None, false, None, None), tr("nothing"));

        // Each role with the path it gets, the path made harmless.
        let roles = BTreeMap::from([(Role::Sent, "INBOX.Sent \"x\"".to_string()), (Role::Drafts, "Drafts".into())]);
        let said = mail_account_changes(None, None, false, None, Some(&roles));
        let drafts = trf("{role} → \"{path}\"", &[("role", &tr("Drafts")), ("path", &"Drafts")]);
        let sent = trf("{role} → \"{path}\"", &[("role", &tr("Sent")), ("path", &"INBOX.Sent 'x'")]);
        assert_eq!(said, format!("{drafts}, {sent}"));
    }

    #[test]
    fn collections_templates_and_accounts_are_shown_by_name() {
        let named = |name: &str| Facts { name: Some(name.into()), ..Facts::default() };
        assert_eq!(
            summary(&delete("dev"), &named("Development")),
            trf("Delete template \"{name}\"", &[("name", &"Development")])
        );
        assert_eq!(summary(&delete("dev"), &Facts::default()), trf("Delete template \"{name}\"", &[("name", &"dev")]));
        let rename = Request::RenameTemplate { id: "dev".into(), name: "Review".into() };
        assert_eq!(
            summary(&rename, &named("Development")),
            trf("Rename template \"{name}\" to \"{new}\"", &[("name", &"Development"), ("new", &"Review")])
        );
        let hide = Request::SetCollectionShown { collection: "anna/https://x/cal/work/".into(), shown: false };
        assert_eq!(
            summary(&hide, &named("Work")),
            trf("Hide \"{collection}\" in the suite", &[("collection", &"Work")])
        );
        let remove = Request::RemoveAccount { id: "nextcloud:anna@cloud.example.org".into() };
        assert_eq!(
            summary(&remove, &named("anna@cloud.example.org")),
            trf("Remove the account {account}", &[("account", &"anna@cloud.example.org")])
        );
        // The account by the label the core knows, else by its id; never by text of the client.
        let create = Request::CreateTaskList {
            account: "nextcloud:anna@cloud.example.org".into(),
            name: "Groceries\u{202E}".into(),
            color: Some("#3FB984".into()),
        };
        let in_account = |account: &str| {
            trf("Create task list \"{name}\" in {account}", &[("name", &"Groceries"), ("account", &account)])
        };
        assert_eq!(summary(&create, &named("anna@cloud.example.org")), in_account("anna@cloud.example.org"));
        assert_eq!(summary(&create, &Facts::default()), in_account("nextcloud:anna@cloud.example.org"));
        let update = Request::UpdateFilterList {
            collection: "github:anna/1".into(),
            revision: 1,
            name: "Bugs".into(),
            color: None,
            queries: vec![],
        };
        assert_eq!(
            summary(&update, &named("Open bugs")),
            trf("Change filter list \"{name}\" → \"{new}\"", &[("name", &"Open bugs"), ("new", &"Bugs")])
        );
        let unpin = Request::UnpinIssue {
            collection: "github:anna/1".into(),
            url: "https://github.com/torro/app/issues/12".into(),
        };
        assert_eq!(
            summary(&unpin, &Facts { item: cached("Crash on start", None), ..named("Open bugs") }),
            trf(
                "Unpin \"{title}\" ({item}) from filter list \"{list}\"",
                &[("title", &"Crash on start"), ("item", &"torro/app#12"), ("list", &"Open bugs")]
            )
        );
        assert_eq!(summary(&Request::Sync, &Facts::default()), tr("Sync all accounts"));
        assert_eq!(
            summary(&Request::Todos, &Facts::default()),
            trf("Run {command}", &[("command", &"tasks.list_todos")])
        );
    }

    #[test]
    fn cached_text_is_made_harmless_too() {
        let real = facts(cached("Board\u{202E}gniteem \"x\"", Some(day(14))), "Wo{rk}");
        let text = summary(&Request::DeleteEvent { event: event("c", Some("/a.ics"), "Lunch", day(14)) }, &real);
        assert!(text.contains("Boardgniteem 'x'") && text.contains("Wo(rk)"), "{text}");
    }

    /// An inbox entry of the script `backup`, as the inbox stores it.
    fn inbox_entry(title: &str) -> Item {
        let at = Utc.with_ymd_and_hms(2026, 10, 12, 8, 0, 0).single().expect("time");
        Item {
            id: "e1".into(),
            source: SourceRef::script("backup"),
            thread_key: "t".into(),
            kind: torro_inbox::Kind::Notice,
            lane: Lane::Info,
            urgent: false,
            title: title.into(),
            body: None,
            from: None,
            agent: None,
            received: at,
            updated: at,
            count: 1,
            state: torro_inbox::State::New,
            link: None,
            actions: Vec::new(),
            untrusted: true,
            facets: Vec::new(),
        }
    }

    #[test]
    fn inbox_summaries_tell_each_command_apart_and_name_what_the_inbox_holds() {
        let source = SourceRef::agent("codex");
        let named = "\"Codex\" (agent:codex)";
        let mute =
            |muted| inbox_summary(&Request::MuteSource { source: source.clone(), muted }, &InboxFacts::default());
        assert_eq!(mute(true), trf("Mute {source} in the inbox", &[("source", &named)]));
        assert_eq!(mute(false), trf("Unmute {source} in the inbox", &[("source", &named)]));

        let known = InboxFacts { item: Some(inbox_entry("Backup \"done\"")), ..InboxFacts::default() };
        let entry = "\"Backup 'done'\"";
        let lane = |remember| Request::SetItemLane { id: "e1".into(), lane: Lane::ForMe, remember };
        assert_eq!(
            inbox_summary(&lane(true), &known),
            trf(
                "Move {entry} to {lane} and send later notices of {source} there too",
                &[("entry", &entry), ("lane", &tr("For me")), ("source", &"\"backup\" (script:backup)")]
            )
        );
        assert_eq!(
            inbox_summary(&lane(false), &known),
            trf("Move {entry} to {lane}", &[("entry", &entry), ("lane", &tr("For me"))])
        );
        let unknown = tr("an unknown inbox entry");
        assert_eq!(
            inbox_summary(&lane(true), &InboxFacts::default()),
            trf(
                "Move {entry} to {lane} and send later notices of {source} there too",
                &[("entry", &unknown), ("lane", &tr("For me")), ("source", &tr("its source"))]
            )
        );
        let done = |done| inbox_summary(&Request::SetItemDone { id: "e1".into(), done, dismissed: false }, &known);
        assert_eq!(done(true), trf("Mark {entry} as done", &[("entry", &entry)]));
        assert_eq!(done(false), trf("Open {entry} again", &[("entry", &entry)]));
        let snooze = inbox_summary(&Request::SnoozeItem { id: "e1".into(), until: None }, &InboxFacts::default());
        assert_eq!(snooze, trf("Stop snoozing {entry}", &[("entry", &unknown)]));

        let rule = |effect| InboxFacts {
            rule: Some(Rule {
                id: "r1".into(),
                source: source.clone(),
                sender: None,
                domain: None,
                effect,
                created: Utc::now(),
                facet: None,
                until: None,
            }),
            ..InboxFacts::default()
        };
        let remove = Request::RemoveRule { id: "r1".into() };
        assert_eq!(
            inbox_summary(&remove, &rule(RuleEffect::Muted)),
            trf("Unmute {source} in the inbox", &[("source", &named)])
        );
        assert_eq!(
            inbox_summary(&remove, &rule(RuleEffect::Lane { lane: Lane::Info })),
            trf("Stop sending notices of {source} to {lane}", &[("source", &named), ("lane", &tr("Info"))])
        );
        assert_eq!(inbox_summary(&remove, &InboxFacts::default()), tr("Remove an unknown inbox rule"));

        // Mail (task E4.1): done marks the mail read, a kept lane is the sender's, opening shows the
        // Inbox app.
        let mail = Item {
            source: SourceRef::mail("anna@example.org"),
            kind: torro_inbox::Kind::Message,
            from: Some(torro_inbox::Sender { name: "Shop".into(), address: Some("News@Shop.example".into()) }),
            link: Some(Link::Mail { message: "m:1".into() }),
            ..inbox_entry("Weekly news")
        };
        let mail = InboxFacts { item: Some(mail), ..InboxFacts::default() };
        let news = "\"Weekly news\"";
        let done = inbox_summary(&Request::SetItemDone { id: "e1".into(), done: true, dismissed: false }, &mail);
        assert_eq!(done, trf("Mark {entry} as done and its mail as read", &[("entry", &news)]));
        assert_eq!(
            inbox_summary(&lane(true), &mail),
            trf(
                "Move {entry} to {lane} and send later mail from {sender} there too",
                &[("entry", &news), ("lane", &tr("For me")), ("sender", &"\"news@shop.example\"")]
            )
        );
        assert_eq!(
            inbox_summary(&Request::OpenItem { id: "e1".into() }, &mail),
            trf("Open {app} from {entry}", &[("app", &"Torro Inbox"), ("entry", &news)])
        );
        let sender_rule = InboxFacts {
            rule: Some(Rule {
                id: "r2".into(),
                source: SourceRef::mail("anna@example.org"),
                sender: Some("news@shop.example".into()),
                domain: None,
                effect: RuleEffect::Lane { lane: Lane::ForMe },
                created: Utc::now(),
                facet: None,
                until: None,
            }),
            ..InboxFacts::default()
        };
        assert_eq!(
            inbox_summary(&Request::RemoveRule { id: "r2".into() }, &sender_rule),
            trf(
                "Stop sending mail from {sender} of {source} to {lane}",
                &[
                    ("sender", &"\"news@shop.example\""),
                    ("source", &"\"anna@example.org\" (mail:anna@example.org)"),
                    ("lane", &tr("For me")),
                ]
            )
        );

        // Hiding (tasks E3.10, E3.11): named from the entry, never from the request; a sender or
        // domain in every mail account, a conversation with its mail left unread.
        let hide = |scope| Request::HideSender { id: "e1".into(), scope };
        assert_eq!(
            inbox_summary(&hide(HideScope::Conversation), &mail),
            trf("Hide {entry} from the inbox; the mail stays unread", &[("entry", &news)])
        );
        assert_eq!(
            inbox_summary(&hide(HideScope::Sender), &mail),
            trf(
                "Hide mail from {sender} in every mail account; the mail stays unread",
                &[("sender", &"\"news@shop.example\"")]
            )
        );
        assert_eq!(
            inbox_summary(&hide(HideScope::Domain), &mail),
            trf(
                "Hide mail from everyone at {domain} in every mail account; the mail stays unread",
                &[("domain", &"\"shop.example\"")]
            )
        );
        assert_eq!(
            inbox_summary(&hide(HideScope::Domain), &InboxFacts::default()),
            trf("Hide the sender of {entry} in the inbox", &[("entry", &unknown)])
        );
        let hidden = |sender: Option<&str>, domain: Option<&str>| InboxFacts {
            rule: Some(Rule {
                id: "r3".into(),
                source: SourceRef::mail("anna@example.org"),
                sender: sender.map(String::from),
                domain: domain.map(String::from),
                effect: RuleEffect::Muted,
                created: Utc::now(),
                facet: None,
                until: None,
            }),
            ..InboxFacts::default()
        };
        let remove_hidden = Request::RemoveRule { id: "r3".into() };
        assert_eq!(
            inbox_summary(&remove_hidden, &hidden(Some("news@shop.example"), None)),
            trf("Show mail from {sender} in the inbox again", &[("sender", &"\"news@shop.example\"")])
        );
        assert_eq!(
            inbox_summary(&remove_hidden, &hidden(None, Some("shop.example"))),
            trf("Show mail from everyone at {domain} in the inbox again", &[("domain", &"\"shop.example\"")])
        );

        // Muting (task E8.6a): the trait and its value from the entry, the end and the effect in words.
        let mut muted_mail = mail.item.clone().expect("mail");
        muted_mail.facets = torro_inbox::rules::facets(&muted_mail, None, false);
        let muted_mail = InboxFacts { item: Some(muted_mail), ..InboxFacts::default() };
        let mute = |scope, until, effect| Request::MuteItem { id: "e1".into(), scope, until, effect };
        let sender = format!("\"{}\"", trf("Mail from {sender}", &[("sender", &"news@shop.example")]));
        let sender = trf("{what} in every mail account", &[("what", &sender)]);
        let by_sender = MuteScope::Facet { key: torro_inbox::item::FacetKey::Sender };
        assert_eq!(
            inbox_summary(&mute(by_sender, None, MuteEffect::NoPopup), &muted_mail),
            trf("Mute {what} for good ({effect})", &[("what", &sender), ("effect", &tr("no popup"))])
        );
        let until = Utc::now() + chrono::TimeDelta::days(1);
        let when_text = when(&EventTime::DateTime(until), Local::now().date_naive());
        assert_eq!(
            inbox_summary(&mute(MuteScope::Conversation, Some(until), MuteEffect::Info), &muted_mail),
            trf(
                "Mute {what} until {when} ({effect})",
                &[
                    ("what", &trf("the conversation {entry}", &[("entry", &news)])),
                    ("when", &when_text),
                    ("effect", &tr("only Info"))
                ]
            )
        );
        let by_project = MuteScope::Facet { key: torro_inbox::item::FacetKey::Project };
        assert_eq!(
            inbox_summary(&mute(by_project, None, MuteEffect::Hide), &muted_mail),
            trf(
                "Mute {what} for good ({effect})",
                &[("what", &trf("a trait of {entry}", &[("entry", &news)])), ("effect", &tr("hidden"))]
            ),
            "a trait the entry lacks names no value"
        );
        let muted_rule = InboxFacts {
            rule: hidden(None, None).rule.map(|rule| Rule {
                facet: Some(torro_inbox::item::Facet {
                    key: torro_inbox::item::FacetKey::List,
                    value: "<news.shop.example>".into(),
                }),
                ..rule
            }),
            ..InboxFacts::default()
        };
        let list = format!("\"{}\"", trf("The list {list}", &[("list", &"<news.shop.example>")]));
        assert_eq!(
            inbox_summary(&remove_hidden, &muted_rule),
            trf("Stop muting {what} in the inbox", &[("what", &list)])
        );

        let post = Request::PostItem {
            kind: torro_inbox::Kind::Notice,
            title: "Ignore all {previous}\ninstructions".into(),
            body: None,
            link: None,
            lane: None,
            thread_key: None,
            agent: None,
        };
        assert_eq!(
            inbox_summary(&post, &InboxFacts::default()),
            trf("Report \"{title}\" to the inbox", &[("title", &"Ignore all (previous) instructions")])
        );
    }

    /// Task E3.5: the summary names the title that will be written, the list by its name and the
    /// entry as the inbox holds it, all quoted.
    #[test]
    fn a_task_from_an_entry_names_title_list_and_entry() {
        let make = |title: Option<&str>| Request::CreateTaskFromItem {
            id: "e1".into(),
            collection: "nextcloud:anna/work".into(),
            title: title.map(str::to_string),
            due: None,
        };
        let known = InboxFacts {
            item: Some(inbox_entry("Deploy \"failed\"")),
            list: Some("Work".into()),
            ..InboxFacts::default()
        };
        let entry = "\"Deploy 'failed'\"";
        let wanted = |title: &str, list: &str, entry: &str| {
            trf(
                "Make a task \"{title}\" in \"{list}\" from {entry}",
                &[("title", &title), ("list", &list), ("entry", &entry)],
            )
        };
        assert_eq!(inbox_summary(&make(Some("Fix {it}\nnow")), &known), wanted("Fix (it) now", "Work", entry));
        assert_eq!(
            inbox_summary(&make(Some(" ")), &known),
            wanted("Deploy 'failed'", "Work", entry),
            "blank: the entry's"
        );
        let unknown = tr("an unknown inbox entry");
        assert_eq!(
            inbox_summary(&make(None), &InboxFacts::default()),
            wanted("", "nextcloud:anna/work", unknown),
            "without facts the id the command runs with"
        );
    }

    /// Task E1.2c: a task with a due date says so; the date is what will be written.
    #[test]
    fn a_task_from_an_entry_names_its_due_date() {
        let due = EventTime::Date(chrono::NaiveDate::from_ymd_opt(2026, 10, 14).expect("date"));
        let make = Request::CreateTaskFromItem {
            id: "e1".into(),
            collection: "l".into(),
            title: Some("Fix it".into()),
            due: Some(due),
        };
        let known =
            InboxFacts { item: Some(inbox_entry("CI red")), list: Some("Work".into()), ..InboxFacts::default() };
        let today = Local::now().date_naive();
        assert_eq!(
            inbox_summary(&make, &known),
            trf(
                "Make a task \"{title}\" in \"{list}\" due {when} from {entry}",
                &[("title", &"Fix it"), ("list", &"Work"), ("when", &when(&due, today)), ("entry", &"\"CI red\"")],
            )
        );
    }

    /// Task E1.2c: opening says what opens, from the link the inbox holds: the page's host (as the
    /// browser reads it), the app by name, or the agent's window.
    #[test]
    fn opening_an_entry_names_the_host_the_app_or_the_agent_window() {
        let entry = "\"CI red\"";
        let open = |link: Option<Link>| {
            let facts = InboxFacts { item: Some(Item { link, ..inbox_entry("CI red") }), ..InboxFacts::default() };
            inbox_summary(&Request::OpenItem { id: "e1".into() }, &facts)
        };
        let page = |host: &str| trf("Open the web page on {host} from {entry}", &[("host", &host), ("entry", &entry)]);
        let url = |url: &str| Some(Link::Url { url: url.into() });
        assert_eq!(open(url("https://CI.example.org/run/1?x=y")), page("ci.example.org"));
        assert_eq!(open(url("https://bank.example@evil.example/login")), page("evil.example"), "a user part");
        assert_eq!(open(url("http://evil.example\\@bank.example/")), page("evil.example"), "a backslash");
        assert_eq!(open(url("https://evil.example:8443#@bank.example")), page("evil.example:8443"));
        assert_eq!(
            open(Some(Link::App { app: "torrotasks".into() })),
            trf("Open {app} from {entry}", &[("app", &"Torro Tasks"), ("entry", &entry)])
        );
        assert_eq!(
            open(Some(Link::Window)),
            trf("Bring the agent window of {entry} to the front", &[("entry", &entry)])
        );
        assert_eq!(open(None), trf("Show {entry} on the screen", &[("entry", &entry)]));
    }

    /// Task E1.2c: an approval keeps what its entry looked like and hands it back when allowed;
    /// approvals stored before have none.
    #[tokio::test]
    async fn an_approval_keeps_the_pin_of_its_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let pin = EntryPin::of(&inbox_entry("CI red"));
        let open = Request::OpenItem { id: "e1".into() };
        let asked = approvals.open("c1", "One", open, Some(pin), "x".into(), at(0)).await.expect("opened");
        let restarted = quiet_in(dir.path());
        let Ok(Decision::Run { pin: kept, .. }) = restarted.decide(&asked.id, true, at(1)).await else {
            panic!("allowed runs");
        };
        assert_eq!(kept, Some(pin));
        assert!(pin.holds(&inbox_entry("CI red")));
        let reposted = Item { count: 2, ..inbox_entry("CI green") };
        assert!(!pin.holds(&reposted));
        let old: Entry = serde_json::from_value(serde_json::json!({
            "info": serde_json::to_value(&asked).expect("json"),
            "request": { "method": "inbox.open_item", "id": "e1" },
        }))
        .expect("an approval from before reads");
        assert_eq!(old.pin, None);
    }

    // ----- Questions of agents (task E2.5) -------------------------------------------------------

    #[test]
    fn the_question_command_is_the_catalogue_name() {
        let request =
            Request::AskPermission { tool: "Bash".into(), details: String::new(), project: None, title: None };
        assert_eq!(request.operation().name(), QUESTION_COMMAND);
        assert_eq!(lifetime(&request), QUESTION_LIFETIME);
        assert_eq!(lifetime(&delete("dev")), LIFETIME);
        assert!(QUESTION_LIFETIME >= chrono::Duration::seconds(30) && QUESTION_LIFETIME < LIFETIME);
    }

    #[tokio::test]
    async fn a_question_expires_after_half_a_minute() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let request = Request::AskPermission { tool: "Bash".into(), details: "ls".into(), project: None, title: None };
        let asked = approvals.open("h1", "Claude Code hooks", request, None, "q".into(), at(0)).await.expect("opened");
        assert!(is_question(&asked));
        let just_before = at(0) + QUESTION_LIFETIME - chrono::Duration::seconds(1);
        assert_eq!(
            approvals.status(Some("h1"), &asked.id, just_before).await.expect("kept").state,
            ApprovalState::Pending
        );
        let expired = approvals.status(Some("h1"), &asked.id, at(0) + QUESTION_LIFETIME).await.expect("kept");
        assert_eq!(expired.state, ApprovalState::Expired);
        assert!(matches!(approvals.decide(&asked.id, true, at(1)).await, Err(ApprovalsError::NotOpen(_))));
    }

    #[tokio::test]
    async fn a_client_asks_at_most_three_questions_at_a_time() {
        let dir = tempfile::tempdir().expect("temp dir");
        let approvals = quiet_in(dir.path());
        let ask = || Request::AskPermission { tool: "Bash".into(), details: "ls".into(), project: None, title: None };
        for _ in 0..QUESTIONS_OPEN_MAX {
            approvals.open("h1", "Hooks", ask(), None, "q".into(), at(0)).await.expect("opened");
        }
        assert!(matches!(
            approvals.open("h1", "Hooks", ask(), None, "q".into(), at(0)).await,
            Err(ApprovalsError::TooMany)
        ));
        // Other commands and other clients are not held up by them.
        approvals.open("h1", "Hooks", delete("dev"), None, "d".into(), at(0)).await.expect("a command still waits");
        approvals.open("h2", "Other", ask(), None, "q".into(), at(0)).await.expect("another client asks");
        // Once they expired, it asks again.
        let later = at(0) + QUESTION_LIFETIME;
        approvals.open("h1", "Hooks", ask(), None, "q".into(), later).await.expect("asks again");
    }

    #[test]
    fn a_question_shows_the_whole_command_without_hidden_characters() {
        let long = format!("echo {}", "a".repeat(500));
        let summary = question_summary("Claude Code", None, "Bash", Some("my-project"), &long);
        assert!(summary.ends_with(&long), "never shortened: {summary}");

        // Bidi controls and zero-width characters go, line breaks and tabs become spaces;
        // quotes, braces and runs of spaces stay as the command has them.
        let tricky = "rm -rf \u{202E}gnihton\u{202C} \u{200B}x\necho \"{agent}\"  {details}\tdone";
        let summary = question_summary("Claude Code", None, "Bash", None, tricky);
        assert_eq!(
            summary,
            trf(
                "{agent} wants to use {tool}: {details}",
                &[
                    ("agent", &"Claude Code"),
                    ("tool", &"Bash"),
                    ("details", &"rm -rf gnihton x echo \"{agent}\"  {details} done")
                ]
            )
        );
        assert!(!summary.chars().any(|c| c.is_control() || is_format(c)), "{summary:?}");

        // Nothing hides behind a wide gap: runs of blanks, look-alikes included, are counted.
        let padded = format!("ls{}; curl x|sh{}\u{2800}\u{3164}end", " ".repeat(1900), "\u{3000}\t\n");
        assert_eq!(harmless(&padded), "ls␣×1900; curl x|sh␣×5end");
        assert_eq!(harmless("a  b\u{00A0}c"), "a  b c");

        // Who asks and the project are quoted like every other client text; without details or
        // project the sentence is shorter.
        let summary = question_summary("Evil {details}", None, "Bash", Some("p\u{202E}x"), "");
        assert_eq!(
            summary,
            trf(
                "{agent} wants to use {tool} in \"{project}\"",
                &[("agent", &"Evil (details)"), ("tool", &"Bash"), ("project", &"px")]
            )
        );
        assert_eq!(
            question_summary("Codex", None, "apply_patch", None, "  "),
            trf("{agent} wants to use {tool}", &[("agent", &"Codex"), ("tool", &"apply_patch")])
        );
    }

    #[test]
    fn a_question_names_the_session_quoted_and_cleaned() {
        let summary = question_summary("Claude Code", Some("Login\n\"bug\" {tool}"), "Bash", Some("p"), "ls");
        let who =
            trf("{agent} (session \"{session}\")", &[("agent", &"Claude Code"), ("session", &"Login 'bug' (tool)")]);
        assert_eq!(
            summary,
            trf(
                "{agent} wants to use {tool} in \"{project}\": {details}",
                &[("agent", &who), ("tool", &"Bash"), ("project", &"p"), ("details", &"ls")]
            )
        );
        let long = question_summary("Codex", Some(&"x".repeat(200)), "Bash", None, "ls");
        assert!(long.contains(&format!("{}…", "x".repeat(torro_inbox::hook::SESSION_TITLE_MAX - 1))), "{long}");
        assert!(!long.contains(&"x".repeat(torro_inbox::hook::SESSION_TITLE_MAX)), "cut to 80: {long}");
        // A blank name is no name.
        assert_eq!(
            question_summary("Codex", Some(" \u{0e} "), "Bash", None, "ls"),
            question_summary("Codex", None, "Bash", None, "ls")
        );
    }

    #[test]
    fn only_tool_names_and_details_the_person_can_read_whole_are_asked() {
        for tool in ["Bash", "apply_patch", "mcp__fs__write", "WebFetch", "a.b:c/d-e"] {
            assert_eq!(question_refusal(tool, "ls"), None, "{tool}");
        }
        let refused = |tool: &str, details: &str| match question_refusal(tool, details) {
            Some(Response::Error { code, .. }) => code,
            other => panic!("refused, got {other:?}"),
        };
        for tool in ["", "Bash ls", "Bash\n", "Bäsh", &"x".repeat(QUESTION_TOOL_MAX + 1)] {
            assert_eq!(refused(tool, "ls"), "error", "{tool:?}");
        }
        assert_eq!(question_refusal("Bash", &"ä".repeat(QUESTION_DETAILS_MAX)), None, "characters, not bytes");
        assert_eq!(refused("Bash", &"x".repeat(QUESTION_DETAILS_MAX + 1)), "error");
    }
}
