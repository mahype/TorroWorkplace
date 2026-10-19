//! Who may talk to the core: keys and sign-in (`docs/api.md`, "Anmeldung").
//!
//! Two kinds of keys arrive with the requests on the socket:
//!
//! - The **app key** of the suite's own programs (control centre, apps, tray, `ctl`). The core
//!   creates it on its first start as `app.key` next to the socket (mode 0600); `torro_ipc::client`
//!   reads and sends it, so the programs never handle it. It may do everything.
//! - **Client keys** for scripts and agents, `torro_<id>_<64 hex>`. The plain key exists only in
//!   the keychain (service `torroworkplace`, account `client-<id>`) and at the client;
//!   `clients.json` (config directory, 0600) holds its SHA-256 with name, kind, rights and last
//!   use.
//!
//! Every request is checked against what the core holds in memory, so removing a client or
//! changing its rights takes effect with its very next request. Keys are compared as SHA-256
//! hashes in constant time and never logged. **Fail closed:** a damaged `clients.json` lets in
//! only the app key, and a missing one means "no clients", never "no checks".
//!
//! [`Clients::admit`] checks the first step of the rights (`docs/api.md`, "Prüfung im Dienst"):
//! level and admin switch of the command's area and the commands switched off one by one
//! ([`Rights::allows`]). Before it the core checks the way in ([`closed_door`], #231): whether the
//! client's kind may come the way the request came, with its switch on. Collections and approval
//! follow in the core ([`crate::core::Core::handle_from`]).
//!
//! The file is written on blocking threads (`spawn_blocking`) and the keychain on a thread of its
//! own ([`crate::account::keychain`], the Secret Service cannot run inside the core's runtime), so
//! neither holds up the requests of other clients. Changes are made in memory first and written
//! afterwards. A write that fails never leaves more allowed than before or after: a new client
//! is taken back, changed rights fall to what old and new both allow, and a client whose new key
//! could not be stored is locked out.
//!
//! **Lent keys** ([`Clients::lend`], task C.7): the core runs a coding agent as an engine of the
//! assistant and hands it a key of the assistant's client for that one run, in the agent's
//! environment only. Such a key lives in memory only (never in `clients.json`, never in the
//! keychain), stands for its client with the client's rights of now narrowed by the areas it was
//! lent without, and stops working when the run drops it ([`LentKey`]) or the client is removed.
//!
//! **Older files:** a client in a `clients.json` from before rights existed (A.1) gets the
//! template *Read only* ([`legacy_rights`]): its person never chose rights, and reading is the
//! least that keeps it useful.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use torro_inbox::{AgentKind, SourceRef};
use torro_ipc::api::{
    ASSISTANT_CLIENT, Area, BOT_CLIENT, HOOK_CLIENT, Lack, MCP_CLIENT, Operation, Rights, RightsTemplate,
    SCRIPT_CLIENT, Selection, is_mcp_kind,
};
use torro_ipc::assistant::Mode;
use torro_ipc::{ClientInfo, Door, Key, Request, Response};

/// Keychain service all suite secrets are filed under (as for the accounts).
const KEYCHAIN_SERVICE: &str = "torroworkplace";

/// Every client key's keychain account starts with this. Account ids must never start with it,
/// or removing an account could delete a client's key and reading one could send a client key to
/// a server ([`is_client_keychain_account`]).
const CLIENT_KEYCHAIN_PREFIX: &str = "client-";

/// `last_used` is written to `clients.json` at most once per this many seconds per client, so
/// a busy agent does not rewrite the file on every request.
const LAST_USED_RESOLUTION_SECS: i64 = 60;

/// Every app key starts with this; 64 hex digits follow.
const APP_KEY_PREFIX: &str = "torro_app_";

/// How often an `app.key` that holds no valid key is read before it is replaced, and the pause
/// in between: someone may still be writing it.
const APP_KEY_INVALID_READS: u32 = 5;
const APP_KEY_PAUSE: Duration = Duration::from_millis(20);

/// Rounds of reading and publishing before giving up on an `app.key` that keeps changing.
const APP_KEY_ATTEMPTS: u32 = 20;

/// A SHA-256 hash of a key.
type KeyHash = [u8; 32];

/// Where the plain client keys are kept: the system keychain in the core
/// ([`SystemKeychain`]), a map in tests, so tests never touch the user's keychain.
pub trait KeyStore: Send + Sync {
    /// Stores `key` under `account` (`client-<id>`), replacing what was there.
    fn set(&self, account: &str, key: &str) -> Result<(), String>;
    /// Removes the entry of `account`; a missing entry is fine. Errors never contain the key.
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// The keychain of the system: Secret Service on Linux, Keychain on macOS, Credential Manager on
/// Windows.
pub struct SystemKeychain;

impl SystemKeychain {
    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(|err| err.to_string())
    }
}

impl KeyStore for SystemKeychain {
    fn set(&self, account: &str, key: &str) -> Result<(), String> {
        Self::entry(account)?.set_password(key).map_err(|err| err.to_string())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// Who sent a request, as far as its key tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// The suite's own programs (app key), and the tray menu inside the core.
    App,
    /// A script or agent with its own key; `id` is its entry in `clients.json`, `rights` what
    /// they were when the request came in.
    Client { id: String, rights: Rights },
    /// No key or an unknown one; only `system.ping` is answered.
    Anonymous,
}

/// Why a request was not let in; `command` is its catalogue name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// No key, or one the core does not know (any more): error code `unauthorized`.
    Unauthorized { command: &'static str },
    /// A known client whose rights do not cover the command, or a `clients` command from a
    /// client key: error code `forbidden`.
    Forbidden { command: &'static str },
    /// The rights cover the command and it must wait for a person's approval
    /// ([`Rights::needs_approval`]), but the core could not record the approval
    /// (`approvals.json` cannot be written): error code `forbidden`, nothing runs. Normally the
    /// core answers `pending` instead ([`crate::approvals`]).
    NeedsApproval { command: &'static str },
    /// A known client whose rights, conversation or engine do not cover the command, with the
    /// reason (issue #133): error code `forbidden`, its message says what is missing.
    Lacks { command: &'static str, lack: Lack },
    /// A known client whose kind may not come the way the request came, or whose way a person
    /// turned off (#231, [`closed_door`]): error code `unauthorized`, `why` says which. Checked
    /// before the rights, so it answers the same for every command, `system.ping` too.
    Closed { command: &'static str, why: &'static str },
}

impl Refused {
    /// The command that was refused, by its catalogue name; for tests that compare refusals whatever
    /// their reason.
    #[cfg(test)]
    pub fn command(self) -> &'static str {
        match self {
            Refused::Unauthorized { command }
            | Refused::Forbidden { command }
            | Refused::NeedsApproval { command }
            | Refused::Lacks { command, .. }
            | Refused::Closed { command, .. } => command,
        }
    }

    /// The answer to the client. It names the command, never the key.
    pub fn response(self) -> Response {
        let (code, message) = match self {
            Refused::Unauthorized { command } => ("unauthorized", format!("{command} {NO_KEY}")),
            Refused::Forbidden { command } => ("forbidden", format!("this client may not use {command}")),
            Refused::NeedsApproval { command } => {
                ("forbidden", format!("{command} needs a person's approval, which the core cannot ask for now"))
            }
            Refused::Lacks { command, lack } => {
                ("forbidden", format!("this client may not use {command}: {}", lack.message()))
            }
            Refused::Closed { command, why } => ("unauthorized", format!("{command} was refused: {why}")),
        };
        Response::Error { code: code.into(), message }
    }
}

/// One client in `clients.json`. Never holds the key itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRecord {
    /// Short random id, part of the key and of the keychain account: `3fa9c1d2`.
    pub id: String,
    /// What the person called it: `Claude Code`.
    pub name: String,
    /// Which program: `script`, `claude-code`, `codex`, `gemini`, `claude-desktop`, `cursor`,
    /// `mcp`, `hook`, `assistant`, `bot` ([`torro_ipc::api::CLIENT_KINDS`]); an older file may hold
    /// another, which [`closed_door`] checks like `script`.
    pub kind: String,
    /// SHA-256 of the key, 64 lowercase hex digits.
    pub key_sha256: String,
    pub created: DateTime<Utc>,
    /// The last request with this key, to the minute.
    #[serde(default)]
    pub last_used: Option<DateTime<Utc>>,
    /// What the client may do. Missing in files from before A.2: then [`legacy_rights`].
    #[serde(default = "legacy_rights")]
    pub rights: Rights,
    /// The coding agent a hook client reports for, fixed when it was added
    /// ([`ClientInfo::agent`]); `None` for every other kind, and in files from before E1.2b.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentKind>,
}

impl ClientRecord {
    /// The record as the API shows it: without the hash of its key.
    fn info(&self) -> ClientInfo {
        ClientInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind.clone(),
            created: self.created,
            last_used: self.last_used,
            rights: self.rights.clone(),
            agent: self.agent,
        }
    }
}

/// The rights of a client listed in a `clients.json` from before rights existed: *Read only*,
/// the safest template that still lets it work.
fn legacy_rights() -> Rights {
    RightsTemplate::ReadOnly.rights()
}

/// How the answer to a missing or unknown key ends ([`Refused::Unauthorized`]): `torroworkplace mcp`
/// adds how to get a key only to these, not to a known key refused for its way ([`Refused::Closed`]).
pub const NO_KEY: &str = "needs a valid key";

/// "Allow API keys" is off (#231).
pub const API_OFF: &str = "The API is turned off in Torro Workplace, Settings › API.";
/// "Allow MCP" is off (#231).
pub const MCP_OFF: &str = "MCP is turned off in Torro Workplace, Settings › MCP.";
/// The key of an MCP client came through the API (#231).
pub const MCP_KEY: &str = "This key belongs to an MCP client.";
/// A script's key came through MCP (#231).
pub const SCRIPT_KEY: &str =
    "This key belongs to a script and works only through the API; an agent needs its own key from Settings › MCP.";

/// Why a client of `kind` may not come in `door` (`None`: whichever way, for an approved command
/// that runs now) with the switches `allow_api_keys` and `allow_mcp` as they are (#231,
/// `docs/api.md` "Transport"); `None` when it may. The core answers `unauthorized` with the reason
/// ([`Refused::Closed`]), before it looks at any right.
///
/// - `script` only through the API, and only with `allow_api_keys` ([`API_OFF`], [`SCRIPT_KEY`]),
/// - an MCP client ([`is_mcp_kind`]) only through MCP, and only with `allow_mcp` ([`MCP_OFF`],
///   [`MCP_KEY`]),
/// - `hook` either way, and only with `allow_mcp`: it belongs to an agent,
/// - `assistant` and `bot` are bound to neither: only they get lent keys, and the core runs them,
/// - any other kind (from an older `clients.json`) like `script`.
///
/// The switch is checked before the way, so a person who turned a way off always reads that.
pub fn closed_door(kind: &str, door: Option<Door>, allow_api_keys: bool, allow_mcp: bool) -> Option<&'static str> {
    match kind {
        ASSISTANT_CLIENT | BOT_CLIENT => None,
        HOOK_CLIENT => (!allow_mcp).then_some(MCP_OFF),
        kind if is_mcp_kind(kind) => match door {
            _ if !allow_mcp => Some(MCP_OFF),
            Some(Door::Api) => Some(MCP_KEY),
            Some(Door::Mcp) | None => None,
        },
        _ => match door {
            _ if !allow_api_keys => Some(API_OFF),
            Some(Door::Mcp) => Some(SCRIPT_KEY),
            Some(Door::Api) | None => None,
        },
    }
}

/// A selection of calendars or task lists names a collection by a bare URL, as before several
/// accounts existed ([`torro_pim::account::is_legacy_id`]).
/// Refuses `rights` whose selections a client may not be given ([`ClientsError::Invalid`]): a
/// collection named by a bare URL (task M1.4), a GitHub selection with anything but `owner/name`
/// and `owner` (task G.2), a mail selection with anything but mail account ids (task M2.1). For
/// [`Clients::update`] and [`Clients::add_client`] alike (task C.11).
fn check_selections(rights: &Rights) -> Result<(), ClientsError> {
    if has_legacy_selection(rights) {
        return Err(ClientsError::Invalid(
            "a selection names a collection by its URL; use the collection ids from accounts.list_accounts",
        ));
    }
    if has_bad_repo_selection(rights) {
        return Err(ClientsError::Invalid(
            "a GitHub selection names repositories as owner/name and owners as owner, nothing else",
        ));
    }
    if has_bad_mail_selection(rights) {
        return Err(ClientsError::Invalid(
            "a mail selection names accounts by their ids from accounts.list_accounts (mail:<address>)",
        ));
    }
    Ok(())
}

fn has_legacy_selection(rights: &Rights) -> bool {
    [&rights.calendars, &rights.task_lists].into_iter().any(|selection| match selection {
        Selection::All => false,
        Selection::Only(ids) => ids.iter().any(|id| torro_pim::account::is_legacy_id(id)),
    })
}

/// The mail selection names something that is no mail account id as the core makes it
/// (`mail:<address>`, [`torro_mail::is_account_id`], task M2.1): [`Rights::reaches_mail_account`]
/// would never match it, so the person would believe they granted something they did not.
fn has_bad_mail_selection(rights: &Rights) -> bool {
    match &rights.mail_accounts {
        Selection::All => false,
        Selection::Only(ids) => ids.iter().any(|id| !torro_mail::is_account_id(id)),
    }
}

/// The GitHub selection names something that is no repository (`owner/name`) or owner
/// (`owner`): a URL, a path, an empty name (task G.2). [`Rights::reaches_repo`] would never match
/// it, so the person would believe they granted something they did not.
fn has_bad_repo_selection(rights: &Rights) -> bool {
    let part = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    match &rights.github_repos {
        Selection::All => false,
        Selection::Only(names) => names.iter().any(|name| match name.split_once('/') {
            Some((owner, repo)) => !(part(owner) && part(repo)),
            None => !part(name),
        }),
    }
}

/// Puts `owner` in front of every bare URL in the selections of `clients` (task M1.1): before
/// several accounts existed there was one, `owner`, so the rewrite is unambiguous and grants no
/// client anything it did not have. `true` when something changed.
fn qualify_legacy_selections(clients: &mut [Entry], owner: &str) -> bool {
    let mut changed = false;
    for entry in clients {
        let rights = &mut entry.record.rights;
        for selection in [&mut rights.calendars, &mut rights.task_lists] {
            if let Selection::Only(ids) = selection {
                for id in ids {
                    if let Some(qualified) = torro_pim::account::qualify_legacy_id(id, owner) {
                        *id = qualified;
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

/// `name` is the keychain account of a client key (`client-<id>`). Account ids of the accounts
/// (`nextcloud:anna@cloud.example.org`) must never be one.
pub fn is_client_keychain_account(name: &str) -> bool {
    name.starts_with(CLIENT_KEYCHAIN_PREFIX)
}

/// What `clients.json` holds.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ClientsFile {
    #[serde(default)]
    clients: Vec<ClientRecord>,
}

/// Why setting up or changing the clients failed. Never carries a key.
#[derive(Debug)]
pub enum ClientsError {
    Io {
        path: PathBuf,
        err: io::Error,
    },
    /// The system has no randomness to make a key from.
    Random(String),
    Keychain(String),
    /// `clients.json` could not be read when the core started; it is left as it is.
    Damaged(PathBuf),
    /// No client with this id.
    NotFound(String),
    /// The name is taken: a script client's name is its inbox source (`script:<name>`), so it
    /// must differ from every other script client's and from `cli`, the app key's.
    Conflict(&'static str),
    /// A client needs a name and a kind, and its rights must name collections by their ids;
    /// the message says what is wrong.
    Invalid(&'static str),
}

impl ClientsError {
    /// The error code of the API answer: `not-found` for an unknown client, else `error`.
    pub fn code(&self) -> &'static str {
        match self {
            ClientsError::NotFound(_) => "not-found",
            ClientsError::Conflict(_) => "conflict",
            _ => "error",
        }
    }
}

impl fmt::Display for ClientsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientsError::Io { path, err } => write!(f, "{}: {err}", path.display()),
            ClientsError::Random(err) => write!(f, "cannot make a key: {err}"),
            ClientsError::Keychain(err) => write!(f, "keychain: {err}"),
            ClientsError::Damaged(path) => {
                write!(f, "{} is damaged; repair or remove it before adding clients", path.display())
            }
            ClientsError::NotFound(id) => write!(f, "no client {id:?}"),
            ClientsError::Invalid(what) | ClientsError::Conflict(what) => f.write_str(what),
        }
    }
}

impl std::error::Error for ClientsError {}

/// A client in memory: its record and the hash to compare against.
struct Entry {
    record: ClientRecord,
    hash: KeyHash,
    /// No key gets this client in: its new key could not be stored ([`Clients::new_key`]). Only
    /// in memory; a new key that is stored lifts it.
    locked: bool,
}

struct State {
    clients: Vec<Entry>,
    /// The keys lent out now ([`Clients::lend`]); only in memory.
    leases: Vec<Lease>,
    /// `clients.json` could not be read at start. Nothing is written to it then, so the person
    /// can still repair it, and only the app key gets in.
    damaged: bool,
}

/// A key lent to a client ([`Clients::lend`]), as the gate keeps it: never the key itself.
struct Lease {
    client: String,
    /// What it was lent for: the engine of the assistant that runs with it.
    engine: String,
    hash: KeyHash,
    /// The areas taken out of the client's rights for requests with this key.
    without: Vec<Area>,
    /// The permission mode the client's rights are narrowed by for requests with this key (issue
    /// #132).
    mode: Mode,
    /// The approvals asked for with this key: the only ones it sees ([`Clients::lent_approvals`]).
    approvals: Vec<String>,
}

/// A key lent to a client for as long as this value lives ([`Clients::lend`]). Dropping it takes
/// the key back: the next request with it is `unauthorized`. Its `Debug` never shows the key.
pub struct LentKey {
    key: Key,
    hash: KeyHash,
    shared: Weak<Shared>,
}

impl LentKey {
    /// The key itself, for the environment of the program it is lent to. Never print it, never
    /// put it on a command line or into a file.
    pub fn key(&self) -> &Key {
        &self.key
    }
}

impl Drop for LentKey {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.lock().leases.retain(|lease| !same(&lease.hash, &self.hash));
        }
    }
}

impl fmt::Debug for LentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LentKey(…)")
    }
}

/// The gate of the core: knows the app key and the clients and decides who gets in.
pub struct Clients {
    shared: Arc<Shared>,
}

/// What [`Clients`] shares with the threads that write the file and the keychain.
struct Shared {
    app_key: KeyHash,
    file: PathBuf,
    keychain: Box<dyn KeyStore>,
    state: Mutex<State>,
    /// Held while `clients.json` is written; each write takes the state inside it, so the last
    /// write always holds the newest state, whatever order the threads run in.
    writing: Mutex<()>,
    /// One change (add, rights, new key, remove) at a time, so taking one back cannot undo
    /// another.
    changing: tokio::sync::Mutex<()>,
}

impl Clients {
    /// Reads the app key from `app_key` or creates it there (mode 0600), and the clients from
    /// `file` (`clients.json`). Blocks on the file system; the daemon calls it on a blocking
    /// thread.
    ///
    /// Fails only when the app key can neither be read nor created. A missing `clients.json`
    /// means no clients; a damaged one is reported on stderr and leaves only the app key valid
    /// (fail closed).
    ///
    /// Selections from before several accounts existed name collections by bare URL; they get
    /// the id of the account that owned them in front ([`qualify_legacy_selections`]), taken
    /// from `accounts.json` next to `file`, and the file is written once (task M1.1). When that
    /// write fails, the rewritten ids still apply and the next start tries again.
    pub fn open(app_key: &Path, file: &Path, keychain: Box<dyn KeyStore>) -> Result<Clients, ClientsError> {
        let app_key = hash(&load_or_create_app_key(app_key)?);
        let mut qualified = false;
        let state = match read_clients(file) {
            Ok(mut clients) => {
                let owner = || {
                    let accounts = file.with_file_name(torro_core::account::ACCOUNTS_FILE);
                    crate::account::legacy_owner(&accounts)
                };
                if clients.iter().any(|entry| has_legacy_selection(&entry.record.rights))
                    && let Some(owner) = owner()
                {
                    qualified = qualify_legacy_selections(&mut clients, &owner);
                }
                State { clients, leases: Vec::new(), damaged: false }
            }
            Err(err) => {
                eprintln!("torroworkplace: {err}; only the suite's own programs are let in");
                State { clients: Vec::new(), leases: Vec::new(), damaged: true }
            }
        };
        let shared = Shared {
            app_key,
            file: file.to_path_buf(),
            keychain,
            state: Mutex::new(state),
            writing: Mutex::new(()),
            changing: tokio::sync::Mutex::new(()),
        };
        if qualified && let Err(err) = shared.write(false) {
            eprintln!("torroworkplace: cannot store the new collection ids of the clients: {err}");
        }
        Ok(Clients { shared: Arc::new(shared) })
    }

    /// Decides whether `request` with `key` may run: the caller, or why not.
    ///
    /// - The app key may run everything.
    /// - A client key may run what its rights allow ([`Rights::allows`]): every `system`
    ///   command except `system.quit`, no `clients` command, the rest by level and admin switch of
    ///   the area.
    ///   Otherwise [`Refused::Forbidden`]. Collections and approval are the core's next steps.
    /// - Without a valid key (none, unknown or removed) only `system.ping` runs, as
    ///   [`Caller::Anonymous`]; everything else is [`Refused::Unauthorized`].
    ///
    /// Rights are looked up anew for every request, so a change ([`Clients::update`],
    /// [`Clients::new_key`], [`Clients::remove`]) applies from the very next one. The returned
    /// [`Caller`] carries the rights as they were at this moment.
    ///
    /// Only memory is read. When a client's last use changes, the file is written in the
    /// background.
    pub fn admit(&self, key: Option<&Key>, request: &Request) -> Result<Caller, Refused> {
        self.admit_operation(key, request.operation())
    }

    /// [`Clients::admit`] for the operation of a request; the table tests go through here.
    fn admit_operation(&self, key: Option<&Key>, operation: Operation) -> Result<Caller, Refused> {
        let command = operation.name();
        match key.map_or(Caller::Anonymous, |key| self.identify(key)) {
            Caller::App => Ok(Caller::App),
            Caller::Client { id, rights } if rights.allows(operation) => Ok(Caller::Client { id, rights }),
            Caller::Client { id, .. } => Err(match key.and_then(|key| self.lack(key, &id, operation)) {
                Some(lack) => Refused::Lacks { command, lack },
                None => Refused::Forbidden { command },
            }),
            Caller::Anonymous if std::ptr::eq(operation.command, Request::Ping.operation().command) => {
                Ok(Caller::Anonymous)
            }
            Caller::Anonymous => Err(Refused::Unauthorized { command }),
        }
    }

    /// Why the client `id` with `key` may not send `operation` (issue #133): its stored rights, and
    /// for a lent key the areas it was lent without, told apart into those its engine never gets
    /// ([`crate::assistant::never_given`]) and those the person switched off in the conversation.
    /// `None` when none of them refuses it (a refusal of the core's own).
    fn lack(&self, key: &Key, id: &str, operation: Operation) -> Option<Lack> {
        let hash = hash(key.expose());
        let state = self.lock();
        let record = &state.clients.iter().find(|entry| entry.record.id == id)?.record;
        let (kind, rights) = (record.kind.clone(), record.rights.clone());
        let lease = state.leases.iter().find(|lease| same(&lease.hash, &hash));
        let (never, without, mode) = match lease {
            Some(lease) => (crate::assistant::never_given(&lease.engine), lease.without.clone(), lease.mode),
            None => (&[][..], Vec::new(), Mode::Default),
        };
        drop(state);
        // Only what the key was lent without: with mail let out (task C.17) an agent's key keeps mail.
        let never: Vec<Area> = never.iter().filter(|area| without.contains(area)).copied().collect();
        let switched_off: Vec<Area> = without.into_iter().filter(|area| !never.contains(area)).collect();
        Lack::of_in(&kind, &rights, operation, &switched_off, &never, mode)
    }

    /// Who `key` belongs to. A known client's last use is noted.
    fn identify(&self, key: &Key) -> Caller {
        let hash = hash(key.expose());
        if same(&hash, &self.shared.app_key) {
            return Caller::App;
        }
        let mut state = self.lock();
        // Compare with every client, not stopping at the first match, so the time taken does not
        // depend on where a key is in the list.
        let mut found = None;
        for (index, entry) in state.clients.iter().enumerate() {
            if same(&entry.hash, &hash) && !entry.locked {
                found = Some(index);
            }
        }
        // A lent key stands for its client, with the areas it was lent without taken out of the
        // client's rights of now and narrowed by its permission mode ([`Clients::lend_in_mode`]).
        let mut without: &[Area] = &[];
        let mut mode = Mode::Default;
        for lease in &state.leases {
            if same(&lease.hash, &hash) {
                found = state.clients.iter().position(|entry| entry.record.id == lease.client && !entry.locked);
                without = &lease.without;
                mode = lease.mode;
            }
        }
        let Some(index) = found else {
            return Caller::Anonymous;
        };
        let without = without.to_vec();
        let record = &mut state.clients[index].record;
        let mut rights = mode.narrow(&record.rights);
        rights.areas.retain(|area, _| !without.contains(area));
        let caller = Caller::Client { id: record.id.clone(), rights };
        if note_use(record, Utc::now()) {
            drop(state);
            self.note_in_background();
        }
        caller
    }

    /// Lends client `id` a key of its own for one run of a program the core starts itself: an
    /// agent that drives the assistant (task C.7). Requests with the key run as the client, with
    /// its rights of the moment each request comes, minus the areas `without`; so the client's
    /// rights, collections, approvals and fixed refusals (the assistant never drives itself) all
    /// apply. The key is made here, kept only as a hash in memory, never written to
    /// `clients.json` or the keychain, and works until the [`LentKey`] is dropped or the client
    /// is removed or locked. `engine` names what it was lent for ([`Clients::lent_for`]), for the
    /// audit log.
    ///
    /// [`ClientsError::NotFound`] for no such client (or a locked one), [`ClientsError::Damaged`]
    /// while `clients.json` is.
    pub fn lend(&self, id: &str, engine: &str, without: &[Area]) -> Result<LentKey, ClientsError> {
        self.lend_in_mode(id, engine, without, Mode::Default)
    }

    /// [`Clients::lend`] for a run in the permission mode `mode` (issue #132): every request with the
    /// key runs with the client's rights narrowed by it ([`Mode::narrow`]) as well, and an approval
    /// asked for with the key keeps it ([`Clients::lent_mode`]).
    pub fn lend_in_mode(&self, id: &str, engine: &str, without: &[Area], mode: Mode) -> Result<LentKey, ClientsError> {
        let mut state = self.lock();
        if state.damaged {
            return Err(ClientsError::Damaged(self.shared.file.clone()));
        }
        if !state.clients.iter().any(|entry| entry.record.id == id && !entry.locked) {
            return Err(ClientsError::NotFound(id.to_string()));
        }
        let key = new_client_key(id)?;
        let hash = hash(&key);
        state.leases.push(Lease {
            client: id.to_string(),
            engine: engine.to_string(),
            hash,
            without: without.to_vec(),
            mode,
            approvals: Vec::new(),
        });
        Ok(LentKey { key: Key::new(key), hash, shared: Arc::downgrade(&self.shared) })
    }

    /// What `key` was lent for ([`Clients::lend`]) while it is lent; `None` for every other key.
    pub fn lent_for(&self, key: &Key) -> Option<String> {
        self.with_lease(key, |lease| lease.engine.clone())
    }

    /// The areas the lent `key` was lent without ([`Clients::lend`]) while it is lent; `None` for
    /// every other key. An approval asked for with the key keeps them (task C.11), so it runs
    /// without them too, also after the lease or the core is gone.
    pub fn lent_without(&self, key: &Key) -> Option<Vec<Area>> {
        self.with_lease(key, |lease| lease.without.clone())
    }

    /// The permission mode the lent `key` was lent in ([`Clients::lend_in_mode`]) while it is lent;
    /// `None` for every other key. An approval asked for with the key keeps it (issue #132).
    pub fn lent_mode(&self, key: &Key) -> Option<Mode> {
        self.with_lease(key, |lease| lease.mode)
    }

    /// The approvals asked for with the lent `key` ([`Clients::note_lent_approval`]); `None` for
    /// a key that is not lent. A lent key sees only these, never the other approvals of its
    /// client: they may hold what its areas keep from it (a mail to send, asked for by engine A).
    pub fn lent_approvals(&self, key: &Key) -> Option<Vec<String>> {
        self.with_lease(key, |lease| lease.approvals.clone())
    }

    /// Notes that the lent `key` asked for the approval `id`; nothing for a key that is not lent.
    pub fn note_lent_approval(&self, key: &Key, id: &str) {
        self.with_lease(key, |lease| lease.approvals.push(id.to_string()));
    }

    /// Runs `with` on the lease of `key`, compared with every lease as keys are; `None` for a key
    /// that is not lent.
    fn with_lease<T>(&self, key: &Key, with: impl FnOnce(&mut Lease) -> T) -> Option<T> {
        let hash = hash(key.expose());
        let mut state = self.lock();
        let mut found = None;
        for (index, lease) in state.leases.iter().enumerate() {
            if same(&lease.hash, &hash) {
                found = Some(index);
            }
        }
        found.map(|index| with(&mut state.leases[index]))
    }

    /// All clients, without their keys' hashes, in the order they were added. Empty while
    /// `clients.json` is damaged.
    pub fn list(&self) -> Vec<ClientInfo> {
        self.lock().clients.iter().map(|entry| entry.record.info()).collect()
    }

    /// Adds a client of any kind but `hook` with `rights` and returns it with its key; see
    /// [`Clients::add_client`]. For tests; the core passes the agent of `clients.create_client`.
    #[cfg(test)]
    pub async fn add(&self, name: &str, kind: &str, rights: Rights) -> Result<(ClientInfo, Key), ClientsError> {
        self.add_client(name, kind, None, rights).await
    }

    /// Adds a client with `rights` and returns it with its key. The key is shown to the person
    /// once; the core keeps only its hash and the keychain the key.
    ///
    /// `name` and `kind` are trimmed and must not be empty ([`ClientsError::Invalid`]). A hook
    /// client ([`HOOK_CLIENT`]) must name the `agent` it reports for, and no other kind may
    /// ([`ClientsError::Invalid`]). A `script` client named `cli` or like another script client,
    /// ignoring case, is [`ClientsError::Conflict`]: its name is its inbox source. So is a second
    /// client of the kind [`ASSISTANT_CLIENT`]: there is one assistant for every engine (decision 5
    /// in `docs/assistent.md`), and the core tells it apart by its kind. While
    /// `clients.json` is damaged nothing is added ([`ClientsError::Damaged`]). When the file
    /// cannot be written, the client is taken back and its key leaves the keychain again.
    ///
    /// `rights` are checked as [`Clients::update`] checks them (task C.11: `clients.create_client`
    /// may bring its own): a bad selection is [`ClientsError::Invalid`] and nothing is added.
    pub async fn add_client(
        &self,
        name: &str,
        kind: &str,
        agent: Option<AgentKind>,
        rights: Rights,
    ) -> Result<(ClientInfo, Key), ClientsError> {
        let (name, kind) = (name.trim(), kind.trim());
        if name.is_empty() {
            return Err(ClientsError::Invalid("a client needs a name"));
        }
        if kind.is_empty() {
            return Err(ClientsError::Invalid("a client needs a kind"));
        }
        match (kind == HOOK_CLIENT, agent.is_some()) {
            (true, false) => return Err(ClientsError::Invalid("a hook client needs the agent it reports for")),
            (false, true) => return Err(ClientsError::Invalid("only hook clients name an agent")),
            _ => {}
        }
        check_selections(&rights)?;
        let rights = rights.with_known_denied();
        let _changing = self.shared.changing.lock().await;
        if kind == SCRIPT_CLIENT && self.name_taken(kind, name) {
            return Err(ClientsError::Conflict("a script client of this name exists, or the name is cli"));
        }
        if kind == MCP_CLIENT && self.name_taken(kind, name) {
            return Err(ClientsError::Conflict("an MCP client of this name exists"));
        }
        if kind == ASSISTANT_CLIENT && self.lock().clients.iter().any(|entry| entry.record.kind == ASSISTANT_CLIENT) {
            return Err(ClientsError::Conflict("the assistant has its client already"));
        }
        let id = self.new_id()?;
        let key = new_client_key(&id)?;
        self.keychain_set(&id, &key).await?;

        let hash = hash(&key);
        let record = ClientRecord {
            id: id.clone(),
            name: name.to_string(),
            kind: kind.to_string(),
            key_sha256: hex(&hash),
            created: Utc::now(),
            last_used: None,
            // A hook client never holds more than `Rights::hook`, whatever template it was given.
            rights: rights.for_kind(kind),
            agent,
        };
        let info = record.info();
        self.lock().clients.push(Entry { record, hash, locked: false });
        if let Err(err) = self.persist().await {
            // Without the file the client would be gone after a restart; take it back now.
            self.lock().clients.retain(|entry| entry.record.id != id);
            let _ = self.keychain_delete(&id).await;
            return Err(err);
        }
        Ok((info, Key::new(key)))
    }

    /// Replaces the rights of client `id`; its next request is checked against them. When the
    /// file cannot be written, the error is returned and the client keeps only what the old and
    /// the new rights both allow ([`Rights::common`]).
    ///
    /// A selection that names a collection by a bare URL, as before several accounts existed, is
    /// refused ([`ClientsError::Invalid`]) and nothing changes (task M1.4): old selections were
    /// rewritten once when the core started, and a bare URL now would reach no collection. So is
    /// a GitHub selection with anything but `owner/name` and `owner` in it (task G.2), and a mail
    /// selection with anything but mail account ids (task M2.1). A hook client keeps at most
    /// [`Rights::hook`] ([`Rights::for_kind`], task E1.1). Commands switched off that the catalogue
    /// does not know, and `system.ping`, are dropped ([`Rights::with_known_denied`], #231).
    pub async fn update(&self, id: &str, rights: Rights) -> Result<(), ClientsError> {
        check_selections(&rights)?;
        let rights = rights.with_known_denied();
        let _changing = self.shared.changing.lock().await;
        // Narrowed to what the client's kind may hold (a hook client: `Rights::hook`).
        let (old, new) = self.with_client(id, |record| {
            let new = rights.for_kind(&record.kind);
            (std::mem::replace(&mut record.rights, new.clone()), new)
        })?;
        if let Err(err) = self.persist().await {
            // Fail safe: neither the old nor the new rights may be wider than what the person
            // wants now, so only what both allow stays in force until a write succeeds.
            let _ = self.with_client(id, |record| record.rights = old.common(&new));
            return Err(err);
        }
        Ok(())
    }

    /// Gives client `id` the name `name` (trimmed, not empty: [`ClientsError::Invalid`]), as a bot's
    /// client follows its bot's name (task B.1), so the person's approvals name it as they know it.
    /// Rights and key stay. When the file cannot be written, the old name comes back and the error
    /// is returned.
    pub async fn rename(&self, id: &str, name: &str) -> Result<(), ClientsError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ClientsError::Invalid("a client needs a name"));
        }
        let _changing = self.shared.changing.lock().await;
        let old = self.with_client(id, |record| std::mem::replace(&mut record.name, name.to_string()))?;
        if let Err(err) = self.persist().await {
            let _ = self.with_client(id, |record| record.name = old);
            return Err(err);
        }
        Ok(())
    }

    /// Gives client `id` a new key and returns it with the client; the old key stops working at
    /// once.
    ///
    /// When the file cannot be written, the error is returned and the client is locked: neither
    /// key gets it in until a new key is stored. The old key must not work again (the person
    /// wanted it gone), and the new one would not survive a restart.
    pub async fn new_key(&self, id: &str) -> Result<(ClientInfo, Key), ClientsError> {
        let _changing = self.shared.changing.lock().await;
        self.with_client(id, |_| ())?;
        let key = new_client_key(id)?;
        self.keychain_set(id, &key).await?;
        let new_hash = hash(&key);
        let (old_hash, info) = {
            let mut state = self.lock();
            let entry = state
                .clients
                .iter_mut()
                .find(|entry| entry.record.id == id)
                .ok_or_else(|| ClientsError::NotFound(id.to_string()))?;
            let old = std::mem::replace(&mut entry.hash, new_hash);
            entry.record.key_sha256 = hex(&new_hash);
            entry.locked = false;
            (old, entry.record.info())
        };
        if let Err(err) = self.persist().await {
            // Memory names the old key again, like the file, so a later write does not store a
            // key nobody holds; but the client stays locked. The keychain keeps the new key,
            // which only the person sees; trying again replaces it.
            if let Some(entry) = self.lock().clients.iter_mut().find(|entry| entry.record.id == id) {
                entry.hash = old_hash;
                entry.record.key_sha256 = hex(&old_hash);
                entry.locked = true;
            }
            return Err(err);
        }
        Ok((info, Key::new(key)))
    }

    /// Gives every collection of the account `from` in the clients' calendar and task list
    /// selections the id it has under the account `to` (`<from>/<URL>` → `<to>/<URL>`), when a
    /// mail account takes over the CalDAV account of the same sign-in (task M2.7): the clients
    /// keep reaching the same calendars and lists, nothing more. Mail accounts are never added to
    /// `mail_accounts` here. Writes `clients.json` (atomically) only when something changed. When
    /// that fails, memory goes back to the old ids like the file: they name no collection any
    /// more, so the clients reach none of them (fail closed) until the person grants them anew.
    pub async fn rename_collections(&self, from: &str, to: &str) -> Result<(), ClientsError> {
        let _changing = self.shared.changing.lock().await;
        let renamed =
            |id: &str| torro_pim::account::local_id(id, from).map(|url| torro_pim::account::collection_id(to, url));
        let before: Vec<(String, Rights)> = {
            let mut state = self.lock();
            let mut before = Vec::new();
            for entry in &mut state.clients {
                let rights = &mut entry.record.rights;
                let old = rights.clone();
                for selection in [&mut rights.calendars, &mut rights.task_lists] {
                    if let Selection::Only(ids) = selection {
                        for id in ids.iter_mut() {
                            if let Some(new) = renamed(id) {
                                *id = new;
                            }
                        }
                    }
                }
                if *rights != old {
                    before.push((entry.record.id.clone(), old));
                }
            }
            before
        };
        if before.is_empty() {
            return Ok(());
        }
        if let Err(err) = self.persist().await {
            for (id, old) in before {
                let _ = self.with_client(&id, |record| record.rights = old);
            }
            return Err(err);
        }
        Ok(())
    }

    /// Takes the account `id` out of every client's rights before the account is removed
    /// (`accounts.remove_account`, task M2.7b): its id from `mail_accounts` and `inbox_sources`
    /// (a mail, GitHub, Nextcloud or ntfy account's id is also its inbox source, task E5.2; a
    /// Nextcloud account's Talk, `talk:<login>@<host>`, goes with it), and its collections
    /// (`<id>/<url>`) from `calendars` and `task_lists`. Otherwise an agent would get its reach
    /// back without asking when the same address is connected again. Works for every kind of
    /// account (Nextcloud, CalDAV, GitHub's filter lists, mail); `All` stays `All`.
    ///
    /// `accounts` are the ids of all accounts the core knows (with `id` or without): a collection
    /// belongs to the account with the longest id it starts with (`torro_pim::account::account_of`),
    /// so removing `nextcloud:anna@example.org` keeps the collections of
    /// `nextcloud:anna@example.org/nc`. Writes `clients.json` (atomically, all clients in one
    /// write) only when something changed; when that fails, memory goes back as well, nothing
    /// is forgotten and the error says so.
    pub async fn forget_account(&self, id: &str, accounts: &[String]) -> Result<(), ClientsError> {
        let _changing = self.shared.changing.lock().await;
        let mut owners: Vec<&str> = accounts.iter().map(String::as_str).filter(|known| *known != id).collect();
        owners.push(id);
        let belongs = |collection: &str| {
            torro_pim::account::account_of(collection, &owners, |owner: &&str| owner).is_some_and(|owner| *owner == id)
        };
        // Talk is a source of its own, but the Nextcloud account's (task E5.2).
        let talk = id.strip_prefix("nextcloud:").map(|account| SourceRef::talk(account).key());
        let before: Vec<(String, Rights)> = {
            let mut state = self.lock();
            let mut before = Vec::new();
            for entry in &mut state.clients {
                let rights = &mut entry.record.rights;
                let old = rights.clone();
                for selection in [&mut rights.calendars, &mut rights.task_lists] {
                    if let Selection::Only(ids) = selection {
                        ids.retain(|collection| !belongs(collection));
                    }
                }
                for selection in [&mut rights.mail_accounts, &mut rights.inbox_sources] {
                    if let Selection::Only(ids) = selection {
                        ids.retain(|selected| selected != id && Some(selected.as_str()) != talk.as_deref());
                    }
                }
                if *rights != old {
                    before.push((entry.record.id.clone(), old));
                }
            }
            before
        };
        if before.is_empty() {
            return Ok(());
        }
        if let Err(err) = self.persist().await {
            for (client, old) in before {
                let _ = self.with_client(&client, |record| record.rights = old);
            }
            return Err(err);
        }
        Ok(())
    }

    /// Removes a client. Its key stops working at once, even when the file or the keychain
    /// cannot be updated; that is then reported as an error.
    pub async fn remove(&self, id: &str) -> Result<(), ClientsError> {
        let _changing = self.shared.changing.lock().await;
        {
            let mut state = self.lock();
            let before = state.clients.len();
            state.clients.retain(|entry| entry.record.id != id);
            if state.clients.len() == before {
                return Err(ClientsError::NotFound(id.to_string()));
            }
        }
        self.persist().await?;
        self.keychain_delete(id).await
    }

    /// `name` is the name of a client of `kind` ignoring case, or for a script `cli` (the app
    /// key's inbox source): the name of a script and of an "Other MCP client" is its inbox source
    /// (`script:<name>`, `agent:mcp:<name>`), so two posters would share one source or look alike.
    fn name_taken(&self, kind: &str, name: &str) -> bool {
        (kind == SCRIPT_CLIENT && name.eq_ignore_ascii_case("cli"))
            || self
                .lock()
                .clients
                .iter()
                .any(|entry| entry.record.kind == kind && entry.record.name.eq_ignore_ascii_case(name))
    }

    /// The clients as `clients.json` lists them.
    #[cfg(test)]
    fn records(&self) -> Vec<ClientRecord> {
        self.lock().clients.iter().map(|entry| entry.record.clone()).collect()
    }

    /// Runs `change` on the record of client `id`; [`ClientsError::NotFound`] without one.
    fn with_client<T>(&self, id: &str, change: impl FnOnce(&mut ClientRecord) -> T) -> Result<T, ClientsError> {
        let mut state = self.lock();
        let entry = state.clients.iter_mut().find(|entry| entry.record.id == id);
        entry.map(|entry| change(&mut entry.record)).ok_or_else(|| ClientsError::NotFound(id.to_string()))
    }

    /// A random id no client has. [`ClientsError::Damaged`] while the file is.
    fn new_id(&self) -> Result<String, ClientsError> {
        let state = self.lock();
        if state.damaged {
            return Err(ClientsError::Damaged(self.shared.file.clone()));
        }
        loop {
            let id = hex(&random::<4>()?);
            if !state.clients.iter().any(|entry| entry.record.id == id) {
                return Ok(id);
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared.lock()
    }

    /// Writes `clients.json` with the current state on a blocking thread.
    async fn persist(&self) -> Result<(), ClientsError> {
        let shared = Arc::clone(&self.shared);
        let file = self.shared.file.clone();
        tokio::task::spawn_blocking(move || shared.write(true))
            .await
            .map_err(|err| ClientsError::Io { path: file, err: io::Error::other(err.to_string()) })?
    }

    /// Writes the new `last_used` without making the request wait: on a blocking thread inside
    /// the core's runtime, right here outside of one (the synchronous tests).
    fn note_in_background(&self) {
        let shared = Arc::clone(&self.shared);
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn_blocking(move || shared.note_last_use());
            }
            Err(_) => shared.note_last_use(),
        }
    }

    async fn keychain_set(&self, id: &str, key: &str) -> Result<(), ClientsError> {
        let (shared, account, key) = (Arc::clone(&self.shared), keychain_account(id), key.to_string());
        crate::account::keychain(move || shared.keychain.set(&account, &key)).await.map_err(ClientsError::Keychain)
    }

    async fn keychain_delete(&self, id: &str) -> Result<(), ClientsError> {
        let (shared, account) = (Arc::clone(&self.shared), keychain_account(id));
        crate::account::keychain(move || shared.keychain.delete(&account)).await.map_err(ClientsError::Keychain)
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // Every change leaves the state whole before anything that could panic, so a poisoned
        // lock is still good to use.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Writes `clients.json` (0600) with the state as it is now. Never while it is damaged.
    /// `create_dir`: create the config directory first; noting a last use never does, so a
    /// late write cannot bring back a directory that was removed meanwhile.
    fn write(&self, create_dir: bool) -> Result<(), ClientsError> {
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let mut json = {
            let state = self.lock();
            if state.damaged {
                return Err(ClientsError::Damaged(self.file.clone()));
            }
            let file = ClientsFile { clients: state.clients.iter().map(|entry| entry.record.clone()).collect() };
            serde_json::to_vec_pretty(&file)
                .map_err(|err| ClientsError::Io { path: self.file.clone(), err: io::Error::other(err) })?
        };
        if create_dir && let Some(dir) = self.file.parent() {
            create_private_dir(dir).map_err(|err| ClientsError::Io { path: self.file.clone(), err })?;
        }
        json.push(b'\n');
        write_private(&self.file, &json)
    }

    fn note_last_use(&self) {
        match self.write(false) {
            Ok(()) => {}
            // The config directory is gone; there is nothing left to note it in.
            Err(ClientsError::Io { err, .. }) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => eprintln!("torroworkplace: cannot note when a client was last used: {err}"),
        }
    }
}

/// Sets `last_used` to `now` when the stored time is missing or older than the resolution;
/// `true` when it changed and should be written.
fn note_use(record: &mut ClientRecord, now: DateTime<Utc>) -> bool {
    let stale = record.last_used.is_none_or(|last| (now - last).num_seconds() >= LAST_USED_RESOLUTION_SECS);
    if stale {
        record.last_used = Some(now);
    }
    stale
}

fn keychain_account(id: &str) -> String {
    format!("{CLIENT_KEYCHAIN_PREFIX}{id}")
}

/// A new key for client `id`: `torro_<id>_<64 hex>`.
fn new_client_key(id: &str) -> Result<String, ClientsError> {
    Ok(format!("torro_{id}_{}", hex(&random::<32>()?)))
}

/// The app key in `path`, created there first when there is none.
///
/// Several cores may start at once on the first login (the tray and an app that starts the core
/// itself), so creating the file must not race: each core writes its key to a temporary file of
/// its own and publishes it with a hard link, which fails when the file exists already; the
/// slower core then reads the faster one's key. A published key is never replaced.
///
/// A file that is not a valid key (`torro_app_` and 64 hex digits) is read a few more times, in
/// case someone is still writing it, and then replaced the same way.
fn load_or_create_app_key(path: &Path) -> Result<String, ClientsError> {
    let io_error = |err| ClientsError::Io { path: path.to_path_buf(), err };
    let mut invalid_reads = 0;
    for _ in 0..APP_KEY_ATTEMPTS {
        match fs::read_to_string(path) {
            Ok(text) => {
                if let Some(key) = valid_app_key(&text) {
                    // An older or hand-made file may be readable for others; the key is only for us.
                    restrict_to_owner(path).map_err(io_error)?;
                    return Ok(key.to_string());
                }
                invalid_reads += 1;
                if invalid_reads < APP_KEY_INVALID_READS {
                    std::thread::sleep(APP_KEY_PAUSE);
                    continue;
                }
                // Still no key. Remove it only if nobody replaced it meanwhile, then publish ours.
                invalid_reads = 0;
                if fs::read_to_string(path).is_ok_and(|now| now == text) {
                    match fs::remove_file(path) {
                        Ok(()) => {}
                        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                        Err(err) => return Err(io_error(err)),
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if let Some(key) = publish_app_key(path)? {
                    return Ok(key);
                }
                // Another core was faster; the next round reads its key.
            }
            Err(err) => return Err(io_error(err)),
        }
    }
    Err(io_error(io::Error::other("the app key keeps changing; is another core starting?")))
}

/// Makes a new app key and publishes it at `path` unless a file is there already (`None`).
fn publish_app_key(path: &Path) -> Result<Option<String>, ClientsError> {
    let key = format!("{APP_KEY_PREFIX}{}", hex(&random::<32>()?));
    if let Some(dir) = path.parent() {
        create_private_dir(dir).map_err(|err| ClientsError::Io { path: path.to_path_buf(), err })?;
    }
    let tmp = write_temporary(path, format!("{key}\n").as_bytes())?;
    // `hard_link` fails when `path` exists, unlike `rename`, so two cores cannot both win.
    let published = fs::hard_link(&tmp, path);
    let _ = fs::remove_file(&tmp);
    match published {
        Ok(()) => Ok(Some(key)),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(None),
        Err(err) => Err(ClientsError::Io { path: path.to_path_buf(), err }),
    }
}

/// The key in the text of an `app.key`, if it is one.
fn valid_app_key(text: &str) -> Option<&str> {
    let key = text.trim();
    let secret = key.strip_prefix(APP_KEY_PREFIX)?;
    let hex_digits = secret.len() == 64 && secret.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    hex_digits.then_some(key)
}

/// The clients in `file`; none when it does not exist. Every hash must be 64 hex digits, and no
/// id or hash may appear twice, or the whole file counts as damaged. Commands switched off that
/// the catalogue no longer knows are dropped from the rights ([`Rights::with_known_denied`], #231);
/// the file keeps them until it is written next.
fn read_clients(file: &Path) -> Result<Vec<Entry>, ClientsError> {
    let damaged = || ClientsError::Damaged(file.to_path_buf());
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(ClientsError::Io { path: file.to_path_buf(), err }),
    };
    // It names who may use the suite; an older or hand-made file may be readable for others.
    restrict_to_owner(file).map_err(|err| ClientsError::Io { path: file.to_path_buf(), err })?;
    let parsed: ClientsFile = serde_json::from_slice(&bytes).map_err(|_| damaged())?;
    let entries = parsed
        .clients
        .into_iter()
        .map(|mut record| {
            record.rights = record.rights.with_known_denied();
            Ok(Entry { hash: from_hex(&record.key_sha256).ok_or_else(damaged)?, record, locked: false })
        })
        .collect::<Result<Vec<_>, ClientsError>>()?;
    let ids: HashSet<_> = entries.iter().map(|entry| entry.record.id.as_str()).collect();
    let hashes: HashSet<_> = entries.iter().map(|entry| entry.hash).collect();
    if ids.len() != entries.len() || hashes.len() != entries.len() {
        return Err(damaged());
    }
    Ok(entries)
}

/// Writes `bytes` to `path` atomically, readable and writable only by the owner on Unix.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), ClientsError> {
    let tmp = write_temporary(path, bytes)?;
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        ClientsError::Io { path: path.to_path_buf(), err }
    })
}

/// Writes `bytes` to a new temporary file next to `path` (mode 0600 on Unix) and returns its
/// path. The name holds the process id and a random part, so cores starting at the same time
/// never share one. The directory must exist.
fn write_temporary(path: &Path, bytes: &[u8]) -> Result<PathBuf, ClientsError> {
    let io_error = |err| ClientsError::Io { path: path.to_path_buf(), err };
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".{}.{}.tmp", std::process::id(), hex(&random::<8>()?)));
    let tmp = path.with_file_name(name);

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options.open(&tmp).and_then(|mut out| out.write_all(bytes).and_then(|()| out.sync_all()));
    match written {
        Ok(()) => Ok(tmp),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(io_error(err))
        }
    }
}

/// Creates `dir` and its missing parents; on Unix the ones it creates are for the owner only,
/// and so is `dir` itself if it was there already (an older version or the system may have made
/// it readable by others). Parents that were there keep their mode.
pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        builder.mode(0o700);
        builder.create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        builder.create(dir)
    }
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

/// Windows keeps files in the profile private to the user already.
#[cfg(not(unix))]
fn restrict_to_owner(path: &Path) -> io::Result<()> {
    let _ = path;
    Ok(())
}

fn random<const N: usize>() -> Result<[u8; N], ClientsError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|err| ClientsError::Random(err.to_string()))?;
    Ok(bytes)
}

fn hash(key: &str) -> KeyHash {
    Sha256::digest(key.as_bytes()).into()
}

/// Equal hashes, compared in constant time: every byte is looked at, whatever differs first.
fn same(a: &KeyHash, b: &KeyHash) -> bool {
    let difference = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(difference) == 0
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(text: &str) -> Option<KeyHash> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;

    use torro_ipc::api::{Access, Area, AreaRights, CATALOGUE, Lack, Level, Mcp, Selection};

    use super::*;

    /// A keychain in memory, shared with the test so it can look inside.
    #[derive(Clone, Default)]
    pub(crate) struct MemoryKeychain(Arc<Mutex<HashMap<String, String>>>);

    impl MemoryKeychain {
        fn get(&self, account: &str) -> Option<String> {
            self.0.lock().expect("lock").get(account).cloned()
        }
    }

    impl KeyStore for MemoryKeychain {
        fn set(&self, account: &str, key: &str) -> Result<(), String> {
            self.0.lock().expect("lock").insert(account.into(), key.into());
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<(), String> {
            self.0.lock().expect("lock").remove(account);
            Ok(())
        }
    }

    /// The gate over `dir/state/app.key` and `dir/config/clients.json`.
    pub(crate) fn open_in(dir: &Path) -> Clients {
        Clients::open(&app_key_file(dir), &clients_file(dir), Box::new(MemoryKeychain::default())).expect("opens")
    }

    pub(crate) fn app_key_file(dir: &Path) -> PathBuf {
        dir.join("state").join("app.key")
    }

    pub(crate) fn clients_file(dir: &Path) -> PathBuf {
        dir.join("config").join("clients.json")
    }

    pub(crate) fn app_key(dir: &Path) -> Key {
        Key::new(fs::read_to_string(app_key_file(dir)).expect("app key").trim())
    }

    fn code(result: Result<Caller, Refused>) -> String {
        match result {
            Err(refused) => match refused.response() {
                Response::Error { code, .. } => code,
                other => panic!("a refusal is an error, got {other:?}"),
            },
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn the_app_key_is_created_once_private_and_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let _ = open_in(dir.path());
        let first = app_key(dir.path());
        assert!(first.expose().starts_with("torro_app_"));
        assert_eq!(first.expose().len(), "torro_app_".len() + 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(app_key_file(dir.path())).expect("exists").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let _ = open_in(dir.path());
        assert_eq!(app_key(dir.path()).expose(), first.expose(), "a second start keeps the key");
    }

    #[cfg(unix)]
    #[test]
    fn an_app_key_readable_by_others_is_restricted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let path = app_key_file(dir.path());
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        let old = format!("torro_app_{}", "ab".repeat(32));
        fs::write(&path, format!("{old}\n")).expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");

        let clients = open_in(dir.path());
        assert_eq!(fs::metadata(&path).expect("exists").permissions().mode() & 0o777, 0o600);
        assert_eq!(clients.admit(Some(&Key::new(old)), &Request::Status), Ok(Caller::App));
    }

    #[test]
    fn an_app_key_that_is_no_key_is_replaced() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = app_key_file(dir.path());
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        for broken in ["", "torro_app_short\n", &format!("torro_app_{}\n", "AB".repeat(32))] {
            fs::write(&path, broken).expect("write");
            let clients = open_in(dir.path());
            let key = app_key(dir.path());
            assert!(valid_app_key(key.expose()).is_some(), "{broken:?} was replaced by a key");
            assert_eq!(clients.shared.app_key, hash(key.expose()));
        }
    }

    /// Two cores starting at once on the first login: both must end up with the key on disk,
    /// or the suite locks itself out.
    #[test]
    fn cores_starting_at_once_share_one_app_key() {
        for _ in 0..25 {
            let dir = tempfile::tempdir().expect("temp dir");
            let start = std::sync::Barrier::new(4);
            let held: Vec<KeyHash> = std::thread::scope(|scope| {
                let cores: Vec<_> = (0..4)
                    .map(|_| {
                        scope.spawn(|| {
                            start.wait();
                            open_in(dir.path()).shared.app_key
                        })
                    })
                    .collect();
                cores.into_iter().map(|core| core.join().expect("opens")).collect()
            });
            let on_disk = hash(app_key(dir.path()).expose());
            assert!(held.iter().all(|key| *key == on_disk), "every core holds the key on disk");
            let left_over: Vec<_> = fs::read_dir(app_key_file(dir.path()).parent().expect("parent"))
                .expect("state dir")
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                .collect();
            assert!(left_over.is_empty(), "no temporary files stay behind");
        }
    }

    #[test]
    fn the_app_key_may_do_everything() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let key = app_key(dir.path());
        for request in [Request::Ping, Request::Status, Request::DeleteTemplate { id: "dev".into() }] {
            assert_eq!(clients.admit(Some(&key), &request), Ok(Caller::App), "{request:?}");
        }
    }

    #[test]
    fn without_a_valid_key_only_ping_is_answered() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        assert_eq!(clients.admit(None, &Request::Ping), Ok(Caller::Anonymous));
        assert_eq!(code(clients.admit(None, &Request::Status)), "unauthorized");

        let wrong = Key::new(format!("torro_app_{}", "0".repeat(64)));
        assert_eq!(clients.admit(Some(&wrong), &Request::Ping), Ok(Caller::Anonymous));
        assert_eq!(code(clients.admit(Some(&wrong), &Request::ListTemplates)), "unauthorized");
    }

    /// A gate in `dir` with its keychain, so the test can look inside.
    fn open_with_keychain(dir: &Path) -> (Clients, MemoryKeychain) {
        let keychain = MemoryKeychain::default();
        let clients = Clients::open(&app_key_file(dir), &clients_file(dir), Box::new(keychain.clone())).expect("opens");
        (clients, keychain)
    }

    fn read_only() -> Rights {
        RightsTemplate::ReadOnly.rights()
    }

    /// The same level and admin switch in every area, the ones never granted included.
    fn everywhere(level: Level, admin: bool) -> Rights {
        let mut rights = read_only();
        for area in Area::ALL {
            rights.areas.insert(area, AreaRights { level, admin });
        }
        rights
    }

    fn caller(id: &str, rights: Rights) -> Result<Caller, Refused> {
        Ok(Caller::Client { id: id.to_string(), rights })
    }

    #[tokio::test]
    async fn errors_never_contain_the_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let nothing = everywhere(Level::NoAccess, false);
        let (_, key) = clients.add("Claude Code", "claude-code", nothing).await.expect("added");
        for key in [key, Key::new("torro_x_wrongsecret")] {
            let Err(refused) = clients.admit(Some(&key), &Request::Accounts) else {
                panic!("refused");
            };
            let Response::Error { message, .. } = refused.response() else { panic!("an error") };
            assert!(!message.contains(key.expose()), "{message}");
            assert!(message.contains("accounts.list_accounts"), "names the command: {message}");
        }
    }

    /// Task M7.1, rule 5 of "Was nie nach außen geht": a script key with the admin switch of every
    /// area still cannot hand the core a CalDAV password; the app key can.
    #[tokio::test]
    async fn no_client_key_signs_in_with_a_password() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let full = everywhere(Level::Full, true);
        let (_, key) = clients.add("Backup", "script", full.clone()).await.expect("added");
        let sign_in = Request::ConnectCalDav {
            address: "anna@mailbox.org".into(),
            server: None,
            login_name: "anna@mailbox.org".into(),
            password: torro_ipc::Secret::new("hunter2"),
            calendars: true,
            tasks: true,
        };
        let refused = clients.admit(Some(&key), &sign_in);
        assert_eq!(refused, Err(Refused::Lacks { command: "accounts.connect_caldav", lack: Lack::ForPeople }));
        assert_eq!(clients.admit(Some(&app_key(dir.path())), &sign_in), Ok(Caller::App));
        let search = Request::DiscoverServices { address: "anna@mailbox.org".into() };
        assert!(clients.admit(Some(&key), &search).is_ok());
    }

    #[tokio::test]
    async fn a_client_key_runs_what_its_rights_allow() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (clients, keychain) = open_with_keychain(dir.path());
        let (client, key) = clients.add("Claude Code", "claude-code", read_only()).await.expect("added");
        let id = client.id;

        assert!(key.expose().starts_with(&format!("torro_{id}_")));
        assert_eq!(keychain.get(&format!("client-{id}")).as_deref(), Some(key.expose()), "plain key in the keychain");
        for request in [Request::Ping, Request::Status, Request::CalendarCollections, Request::ListTemplates] {
            assert_eq!(clients.admit(Some(&key), &request), caller(&id, read_only()), "{request:?}");
        }
        for request in [Request::SaveTemplate { name: None }, Request::Sync, Request::ListClients] {
            assert_eq!(code(clients.admit(Some(&key), &request)), "forbidden", "{request:?}");
        }
    }

    /// The table: every command of the catalogue × every level × admin switch, through the gate
    /// with a real client key. Each round changes the rights and checks the very next request,
    /// so a change of rights applies at once. A new command is covered without touching this.
    #[tokio::test]
    async fn every_command_by_level_and_admin_switch() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (client, key) = clients.add("Script", "script", read_only()).await.expect("added");
        for level in [Level::NoAccess, Level::Read, Level::Write, Level::Full] {
            for admin in [false, true] {
                let rights = everywhere(level, admin);
                clients.update(&client.id, rights.clone()).await.expect("rights changed");
                for command in CATALOGUE {
                    let allowed = match (command.area, command.access) {
                        // Kept from agents means for the app key only (`Rights::allows`).
                        _ if command.mcp == Mcp::Never => false,
                        (Area::System | Area::Approvals, Access::Admin) => false,
                        (Area::System | Area::Approvals, _) => true,
                        (Area::Clients, _) => false,
                        (_, Access::Admin) => admin,
                        (_, Access::Read) => level != Level::NoAccess,
                        (_, Access::Write) => matches!(level, Level::Write | Level::Full),
                        (_, Access::Delete) => level == Level::Full,
                    };
                    let expected = match allowed {
                        true => caller(&client.id, rights.clone()).map_err(Refused::command),
                        false => Err(command.name),
                    };
                    let operation = Operation { command, access: command.access };
                    let got = clients.admit_operation(Some(&key), operation);
                    // Refused with the reason (issue #133), always as `forbidden`.
                    assert!(got.is_ok() || code(got.clone()) == "forbidden", "{}", command.name);
                    let got = got.map_err(Refused::command);
                    assert_eq!(got, expected, "{} with {level:?}, admin {admin}", command.name);
                }
            }
        }
    }

    #[tokio::test]
    async fn loading_with_replace_needs_full_access() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (_, key) = clients.add("Script", "script", everywhere(Level::Write, false)).await.expect("added");
        let load = |mode| Request::LoadTemplate { id: "dev".into(), mode };
        assert!(clients.admit(Some(&key), &load(torro_setups::LoadMode::Add)).is_ok());
        assert_eq!(code(clients.admit(Some(&key), &load(torro_setups::LoadMode::Replace))), "forbidden");
    }

    #[tokio::test]
    async fn clients_commands_are_forbidden_to_every_client_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        // Even rights that name the area `clients` with full access and admin do not count.
        let (client, key) = clients.add("Script", "script", everywhere(Level::Full, true)).await.expect("added");
        let id = client.id;
        for request in [
            Request::ListClients,
            Request::CreateClient {
                name: "Me".into(),
                kind: "script".into(),
                template: RightsTemplate::FullAccess,
                agent: None,
                rights: None,
            },
            Request::UpdateClient { id: id.clone(), rights: everywhere(Level::Full, true) },
            Request::NewClientKey { id: id.clone() },
            Request::RemoveClient { id: id.clone() },
        ] {
            let command = request.operation().name();
            assert_eq!(clients.admit(Some(&key), &request), Err(Refused::Lacks { command, lack: Lack::ForPeople }));
            assert_eq!(clients.admit(Some(&app_key(dir.path())), &request), Ok(Caller::App));
        }
    }

    #[tokio::test]
    async fn clients_json_holds_the_hash_and_rights_not_the_key_and_is_private() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let mut rights = RightsTemplate::ReadAndWrite.rights();
        rights.calendars = Selection::Only(vec!["/cal/work/".into()]);
        let (client, key) = clients.add("Script", "script", rights.clone()).await.expect("added");

        let text = fs::read_to_string(clients_file(dir.path())).expect("written");
        assert!(!text.contains(key.expose()));
        assert!(!text.contains(key.expose().rsplit('_').next().expect("secret part")));
        assert!(text.contains(&hex(&hash(key.expose()))));
        assert!(text.contains("/cal/work/"), "rights are stored");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(clients_file(dir.path())).expect("exists").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // A restart reads the client back and still knows its key and rights.
        let again = open_in(dir.path());
        assert_eq!(again.admit(Some(&key), &Request::Ping), caller(&client.id, rights));
    }

    #[tokio::test]
    async fn the_list_shows_clients_with_rights_and_without_hashes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (first, _) = clients.add(" Claude Code ", "claude-code", read_only()).await.expect("added");
        let (second, _) = clients.add("Script", "script", everywhere(Level::Full, false)).await.expect("added");
        assert_eq!(first.name, "Claude Code", "trimmed");
        assert_eq!(clients.list(), vec![first, second]);
    }

    #[tokio::test]
    async fn a_client_needs_a_name_and_a_kind() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        assert!(matches!(clients.add("  ", "script", read_only()).await, Err(ClientsError::Invalid(_))));
        assert!(matches!(clients.add("Script", "", read_only()).await, Err(ClientsError::Invalid(_))));
        assert!(clients.list().is_empty());
        assert!(!clients_file(dir.path()).exists());
    }

    #[tokio::test]
    async fn rights_with_a_bare_collection_url_are_refused_and_change_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (client, _) = clients.add("Script", "script", read_only()).await.expect("added");
        let mut rights = read_only();
        rights.task_lists =
            Selection::Only(vec!["https://cloud.example.org/remote.php/dav/calendars/anna/work/".into()]);
        let refused = clients.update(&client.id, rights.clone()).await;
        assert!(matches!(refused, Err(ClientsError::Invalid(_))), "{refused:?}");
        assert_eq!(refused.map_err(|err| err.code()).err(), Some("error"));
        assert_eq!(clients.list(), vec![client.clone()], "the rights stay as they were");
        rights.task_lists = Selection::Only(vec![
            "nextcloud:anna@cloud.example.org/https://cloud.example.org/remote.php/dav/calendars/anna/work/".into(),
        ]);
        clients.update(&client.id, rights).await.expect("ids are fine");
    }

    #[tokio::test]
    async fn a_github_selection_names_repositories_and_owners_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (client, _) = clients.add("Script", "script", read_only()).await.expect("added");
        for bad in ["https://github.com/mahype/TorroWorkplace", "mahype/TorroWorkplace/issues", "", "a/", "a b"] {
            let rights = Rights { github_repos: Selection::Only(vec![bad.into()]), ..read_only() };
            let refused = clients.update(&client.id, rights).await;
            assert!(matches!(refused, Err(ClientsError::Invalid(_))), "{bad:?}: {refused:?}");
        }
        assert_eq!(clients.list(), vec![client.clone()], "the rights stay as they were");
        let rights = Rights {
            github_repos: Selection::Only(vec!["mahype/TorroWorkplace".into(), "awesomemotive".into()]),
            ..read_only()
        };
        clients.update(&client.id, rights.clone()).await.expect("repositories and owners are fine");
        assert_eq!(clients.list()[0].rights, rights);
    }

    #[test]
    fn clients_from_before_github_reach_no_repository_and_no_github_command() {
        let json = r#"{"id":"c1","name":"Old","kind":"script","keySha256":"00","created":"2026-01-01T00:00:00Z",
            "rights":{"areas":{"tasks":{"level":"full","admin":false}},"calendars":"all","taskLists":"all",
            "confirm":"never"}}"#;
        let record: ClientRecord = serde_json::from_str(json).expect("an older record");
        assert_eq!(record.rights.github_repos, Selection::Only(vec![]));
        let preview = Request::PreviewFilter { account: "github:mahype".into(), queries: vec!["is:open".into()] };
        assert!(!record.rights.allows(preview.operation()));
    }

    #[tokio::test]
    async fn a_hook_client_never_holds_more_than_posting_to_the_inbox() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let full = RightsTemplate::FullAccess.rights();
        let (hook, _) = clients
            .add_client("Claude Code hooks", "hook", Some(AgentKind::ClaudeCode), full.clone())
            .await
            .expect("added");
        assert_eq!(hook.rights, full.for_kind(torro_ipc::api::HOOK_CLIENT));
        assert!(!hook.rights.allows(Request::CalendarCollections.operation()), "no calendar");
        assert_eq!(hook.rights.inbox_sources, Selection::Only(vec![]), "no source to see");
        clients.update(&hook.id, everywhere(Level::Full, true)).await.expect("updated");
        let rights = clients.list()[0].rights.clone();
        assert!(!rights.allows(Request::Todos.operation()));
        assert!(!rights.area(Area::Inbox).admin && rights.area(Area::Inbox).level == Level::Write);
        // Other kinds keep what they were given.
        let (script, _) = clients.add("Script", "script", full.clone()).await.expect("added");
        assert_eq!(script.rights, full);
    }

    #[test]
    fn a_clients_file_from_before_the_inbox_loads_without_inbox_rights() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = clients_file(dir.path());
        fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        let old = format!(
            r#"{{"clients":[{{"id":"a1b2c3d4","name":"Claude Code","kind":"claude-code","keySha256":"{}",
            "created":"2026-10-11T08:00:00Z","rights":{{"areas":{{"tasks":{{"level":"full","admin":false}}}},
            "calendars":"all","taskLists":"all","githubRepos":"all","confirm":"on_delete"}}}}]}}"#,
            "1".repeat(64)
        );
        fs::write(&file, old).expect("write");
        let clients = open_in(dir.path());
        let listed = clients.list();
        assert_eq!(listed.len(), 1, "an older file is not damaged");
        let rights = &listed[0].rights;
        assert_eq!(rights.inbox_sources, Selection::Only(vec![]));
        assert_eq!(rights.github_repos, Selection::All, "the rest stays as stored");
        assert!(!rights.allows(Request::ListSources.operation()), "no inbox area yet");
        assert!(!rights.inbox_sources.contains("agent:claude-code"));
    }

    /// #231: a `clients.json` from before the deny list loads with none; names the catalogue does
    /// not know and `system.ping` fall out of a stored list when it is read, the rest stays.
    #[test]
    fn a_deny_list_loads_without_unknown_names_and_older_files_load_unchanged() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = clients_file(dir.path());
        fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        let record = |id: &str, digit: &str, denied: &str| {
            format!(
                r#"{{"id":"{id}","name":"Script {id}","kind":"script","keySha256":"{}",
                "created":"2026-10-11T08:00:00Z","rights":{{"areas":{{"tasks":{{"level":"full","admin":false}}}},
                "calendars":"all","taskLists":"all","confirm":"on_delete"{denied}}}}}"#,
                digit.repeat(64)
            )
        };
        let denied = r#","denied":["tasks.delete_todo","tasks.gone","system.ping"]"#;
        let json = format!(r#"{{"clients":[{},{}]}}"#, record("a1b2c3d4", "1", ""), record("e5f6a7b8", "2", denied));
        fs::write(&file, json).expect("write");
        let listed = open_in(dir.path()).list();
        assert_eq!(listed.len(), 2, "not damaged");
        assert!(listed[0].rights.denied.is_empty(), "an older file switches nothing off");
        let kept: Vec<&str> = listed[1].rights.denied.iter().map(String::as_str).collect();
        assert_eq!(kept, ["tasks.delete_todo"]);
        let command =
            torro_ipc::api::CATALOGUE.iter().find(|c| c.name == "tasks.delete_todo").expect("in the catalogue");
        assert!(!listed[1].rights.allows(Operation { command, access: command.access }), "switched off");
    }

    /// #231: changed rights keep only commands the catalogue knows in their deny list, never
    /// `system.ping`; the stored file holds what applies.
    #[tokio::test]
    async fn changed_rights_drop_unknown_names_from_the_deny_list() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (client, key) = clients.add("Script", "script", read_only()).await.expect("added");
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.denied = ["system.ping", "calendar.gone", "workspaces.delete_template"].map(String::from).into();
        clients.update(&client.id, rights).await.expect("changed");
        let stored: Vec<String> = open_in(dir.path()).records()[0].rights.denied.iter().cloned().collect();
        assert_eq!(stored, ["workspaces.delete_template"]);
        assert!(matches!(clients.admit(Some(&key), &Request::Ping), Ok(Caller::Client { .. })), "ping stays open");
        let delete = Request::DeleteTemplate { id: "dev".into() };
        let refused = clients.admit(Some(&key), &delete);
        assert_eq!(
            refused,
            Err(Refused::Lacks {
                command: "workspaces.delete_template",
                lack: Lack::Denied { area: Area::Workspaces }
            })
        );
    }

    /// #231 (`docs/api.md` "Transport"): which kind comes which way with which switch.
    #[test]
    fn each_kind_has_its_way_and_its_switch() {
        let (api, mcp) = (Some(Door::Api), Some(Door::Mcp));
        // Kind, way, API keys on, MCP on: the reason it is refused, if any.
        let table = [
            ("script", api, true, true, None),
            ("script", mcp, true, true, Some(SCRIPT_KEY)),
            ("script", api, false, true, Some(API_OFF)),
            ("script", api, true, false, None),
            ("claude-code", mcp, true, true, None),
            ("claude-code", api, true, true, Some(MCP_KEY)),
            ("claude-code", mcp, false, true, None),
            ("mcp", mcp, true, false, Some(MCP_OFF)),
            ("cursor", api, true, false, Some(MCP_OFF)),
            ("hook", api, true, true, None),
            ("hook", mcp, true, true, None),
            ("hook", api, true, false, Some(MCP_OFF)),
            ("assistant", mcp, false, false, None),
            ("bot", api, false, false, None),
            ("zed-editor", api, true, true, None),
            ("zed-editor", mcp, true, true, Some(SCRIPT_KEY)),
            ("zed-editor", api, false, true, Some(API_OFF)),
            // An approved command runs whichever way it came, only with the switch on.
            ("script", None, true, true, None),
            ("codex", None, true, false, Some(MCP_OFF)),
        ];
        for (kind, door, api_keys, mcp_on, expected) in table {
            assert_eq!(closed_door(kind, door, api_keys, mcp_on), expected, "{kind} {door:?} {api_keys} {mcp_on}");
        }
    }

    /// #231: an "Other MCP client" has a name of its own like a script, among the clients of its kind.
    #[tokio::test]
    async fn other_mcp_clients_have_unique_names() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        clients.add("Zed", MCP_CLIENT, read_only()).await.expect("added");
        let again = clients.add(" zed ", MCP_CLIENT, read_only()).await;
        assert!(matches!(&again, Err(err) if err.code() == "conflict"), "{again:?}");
        clients.add("Zed", "script", read_only()).await.expect("a script may share it");
        clients.add("cli", MCP_CLIENT, read_only()).await.expect("cli is the app key's script source only");
    }

    /// Task M2.1: a `clients.json` from before mail opens no mail account, and sending would wait
    /// for a person.
    #[test]
    fn a_clients_file_from_before_mail_loads_without_mail_rights() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = clients_file(dir.path());
        fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        let old = format!(
            r#"{{"clients":[{{"id":"a1b2c3d4","name":"Claude Code","kind":"claude-code","keySha256":"{}",
            "created":"2026-10-11T08:00:00Z","rights":{{"areas":{{"tasks":{{"level":"full","admin":false}}}},
            "calendars":"all","taskLists":"all","githubRepos":"all","inboxSources":"all","confirm":"never"}}}}]}}"#,
            "1".repeat(64)
        );
        fs::write(&file, old).expect("write");
        let clients = open_in(dir.path());
        let listed = clients.list();
        assert_eq!(listed.len(), 1, "an older file is not damaged");
        let rights = &listed[0].rights;
        assert_eq!(rights.mail_accounts, Selection::Only(vec![]));
        assert_eq!(rights.outbound, torro_ipc::api::Outbound::WithApproval);
        assert_eq!(rights.area(Area::Mail), AreaRights::NONE);
        assert!(!rights.allows(Request::MailFolders { account: None }.operation()), "no mail area yet");
        assert!(!rights.reaches_mail_account("mail:anna@mailbox.org"));
        assert_eq!(rights.inbox_sources, Selection::All, "the rest stays as stored");
    }

    /// Task M2.1, rule 5 of "Was nie nach außen geht": a script key with the admin switch of every
    /// area still cannot hand the core a mail password; `outbound: never` keeps it from sending.
    #[tokio::test]
    async fn no_client_key_connects_mail_and_never_means_no_sending() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let full = everywhere(Level::Full, true);
        let (client, key) = clients.add("Backup", "script", full.clone()).await.expect("added");
        let server =
            |host: &str| torro_mail::Endpoint { host: host.into(), port: 993, security: torro_mail::Security::Tls };
        let connect = Request::ConnectMail {
            address: "anna@mailbox.org".into(),
            sender_name: String::new(),
            imap: server("imap.mailbox.org"),
            smtp: server("smtp.mailbox.org"),
            login_name: "anna@mailbox.org".into(),
            password: torro_ipc::Secret::new("hunter2"),
            calendars: false,
            tasks: false,
            caldav: None,
        };
        let refused = Refused::Lacks { command: "accounts.connect_mail", lack: Lack::ForPeople };
        assert_eq!(clients.admit(Some(&key), &connect), Err(refused));
        assert_eq!(clients.admit(Some(&app_key(dir.path())), &connect), Ok(Caller::App));

        let send = Request::SendDraft { id: "m:1".into() };
        assert!(clients.admit(Some(&key), &send).is_ok(), "with approval by default");
        let never = Rights { outbound: torro_ipc::api::Outbound::Never, ..full };
        clients.update(&client.id, never).await.expect("updated");
        assert_eq!(
            clients.admit(Some(&key), &send),
            Err(Refused::Lacks { command: "mail.send_draft", lack: Lack::Outbound })
        );
    }

    #[tokio::test]
    async fn a_mail_selection_names_mail_accounts_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (client, _) = clients.add("Script", "script", read_only()).await.expect("added");
        for bad in ["anna@mailbox.org", "caldav:anna@mailbox.org", "mail:anna@Mailbox.org", "mail:x/y@z.org"] {
            let rights = Rights { mail_accounts: Selection::Only(vec![bad.into()]), ..read_only() };
            assert!(matches!(clients.update(&client.id, rights).await, Err(ClientsError::Invalid(_))), "{bad}");
        }
        let good = Rights { mail_accounts: Selection::Only(vec!["mail:anna@mailbox.org".into()]), ..read_only() };
        clients.update(&client.id, good.clone()).await.expect("a mail account id");
        assert_eq!(clients.list()[0].rights, good);
    }

    #[tokio::test]
    async fn changed_rights_apply_to_the_next_request_and_survive_a_restart() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (client, key) = clients.add("Script", "script", read_only()).await.expect("added");
        let save = Request::SaveTemplate { name: None };
        assert_eq!(code(clients.admit(Some(&key), &save)), "forbidden");

        let full = RightsTemplate::FullAccess.rights();
        clients.update(&client.id, full.clone()).await.expect("changed");
        assert_eq!(clients.admit(Some(&key), &save), caller(&client.id, full.clone()));
        assert_eq!(open_in(dir.path()).records()[0].rights, full);
        assert!(matches!(clients.update("nope", read_only()).await, Err(ClientsError::NotFound(_))));
    }

    /// Makes every later write of `clients.json` fail: a directory that is not empty takes its
    /// place, and renaming a file onto it fails, for root too.
    fn block_writes(dir: &Path) {
        let file = clients_file(dir);
        fs::remove_file(&file).expect("written before");
        fs::create_dir_all(file.join("in-the-way")).expect("a directory in the way");
    }

    /// Task B.1: a bot's client follows its bot's name; rights and key stay, a blank name or an
    /// unknown client changes nothing.
    #[tokio::test]
    async fn a_client_is_renamed_and_keeps_its_rights_and_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (info, key) = clients.add("Travel planner", "bot", Rights::none()).await.expect("added");
        clients.rename(&info.id, "  Trip planner ").await.expect("renamed");
        let renamed = clients.list().into_iter().find(|client| client.id == info.id).expect("still there");
        assert_eq!((renamed.name.as_str(), &renamed.rights), ("Trip planner", &Rights::none()));
        assert!(matches!(clients.admit(Some(&key), &Request::Ping), Ok(Caller::Client { .. })), "the key works");
        assert!(matches!(clients.rename(&info.id, " ").await, Err(ClientsError::Invalid(_))));
        assert!(matches!(clients.rename("00000000", "X").await, Err(ClientsError::NotFound(_))));
        let reopened = open_in(dir.path());
        assert_eq!(reopened.list()[0].name, "Trip planner", "stored");
    }

    #[tokio::test]
    async fn rights_that_cannot_be_stored_fall_to_what_old_and_new_both_allow() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let mut old = RightsTemplate::FullAccess.rights();
        old.calendars = Selection::Only(vec!["/cal/work/".into(), "/cal/home/".into()]);
        let (client, key) = clients.add("Script", "script", old.clone()).await.expect("added");
        block_writes(dir.path());

        let mut new = RightsTemplate::ReadOnly.rights();
        new.calendars = Selection::All;
        assert!(matches!(clients.update(&client.id, new.clone()).await, Err(ClientsError::Io { .. })));
        let kept = clients.list()[0].rights.clone();
        assert_eq!(kept, old.common(&new));
        assert_eq!(kept.area(Area::Workspaces).level, Level::Read, "not the old full access");
        assert_eq!(kept.calendars, old.calendars, "not the new `all`");
        assert_eq!(code(clients.admit(Some(&key), &Request::SaveTemplate { name: None })), "forbidden");
    }

    #[tokio::test]
    async fn a_new_key_that_cannot_be_stored_locks_the_client() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (client, old) = clients.add("Codex", "codex", read_only()).await.expect("added");
        block_writes(dir.path());

        assert!(matches!(clients.new_key(&client.id).await, Err(ClientsError::Io { .. })));
        assert_eq!(code(clients.admit(Some(&old), &Request::Status)), "unauthorized", "the old key stays invalid");
        assert_eq!(clients.admit(Some(&old), &Request::Ping), Ok(Caller::Anonymous));

        // Once the file can be written again, a new key lifts the lock.
        fs::remove_dir_all(clients_file(dir.path())).expect("out of the way");
        let (_, new) = clients.new_key(&client.id).await.expect("stored now");
        assert!(clients.admit(Some(&new), &Request::Status).is_ok());
        assert_eq!(code(clients.admit(Some(&old), &Request::Status)), "unauthorized");
    }

    #[tokio::test]
    async fn a_new_key_replaces_the_old_one_at_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (clients, keychain) = open_with_keychain(dir.path());
        let (client, old) = clients.add("Codex", "codex", read_only()).await.expect("added");

        let (info, new) = clients.new_key(&client.id).await.expect("new key");
        assert_eq!(info.id, client.id);
        assert!(new.expose().starts_with(&format!("torro_{}_", client.id)));
        assert_eq!(code(clients.admit(Some(&old), &Request::Status)), "unauthorized");
        assert!(clients.admit(Some(&new), &Request::Status).is_ok());
        assert_eq!(keychain.get(&format!("client-{}", client.id)).as_deref(), Some(new.expose()));
        let again = open_in(dir.path());
        assert!(again.admit(Some(&new), &Request::Status).is_ok(), "the file names the new key");
        assert_eq!(code(again.admit(Some(&old), &Request::Status)), "unauthorized");
        assert!(matches!(clients.new_key("nope").await, Err(ClientsError::NotFound(_))));
    }

    #[tokio::test]
    async fn a_removed_client_is_refused_with_its_next_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (clients, keychain) = open_with_keychain(dir.path());
        let (client, key) = clients.add("Codex", "codex", read_only()).await.expect("added");
        let id = client.id;
        assert!(matches!(clients.admit(Some(&key), &Request::Ping), Ok(Caller::Client { .. })));

        clients.remove(&id).await.expect("removed");
        assert_eq!(clients.admit(Some(&key), &Request::Ping), Ok(Caller::Anonymous));
        assert_eq!(code(clients.admit(Some(&key), &Request::Status)), "unauthorized");
        assert_eq!(keychain.get(&format!("client-{id}")), None, "key gone from the keychain");
        assert!(open_in(dir.path()).records().is_empty(), "and from clients.json");
        assert!(matches!(clients.remove(&id).await, Err(ClientsError::NotFound(_))));
    }

    #[tokio::test]
    async fn a_damaged_clients_json_lets_in_only_the_app_key_and_is_left_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (_, key) = open_in(dir.path()).add("Script", "script", read_only()).await.expect("added");
        let file = clients_file(dir.path());
        let damaged = fs::read_to_string(&file).expect("written").replace("\"clients\"", "\"clients\" oops");
        fs::write(&file, &damaged).expect("damage it");

        let clients = open_in(dir.path());
        assert_eq!(code(clients.admit(Some(&key), &Request::Status)), "unauthorized");
        assert_eq!(clients.admit(Some(&key), &Request::Ping), Ok(Caller::Anonymous));
        assert_eq!(clients.admit(Some(&app_key(dir.path())), &Request::Status), Ok(Caller::App));

        assert!(matches!(clients.add("Other", "script", read_only()).await, Err(ClientsError::Damaged(_))));
        assert!(clients.list().is_empty());
        assert_eq!(fs::read_to_string(&file).expect("still there"), damaged, "not overwritten");
    }

    /// A `clients.json` written by A.1, before rights existed: still read, as *Read only*.
    #[test]
    fn a_client_from_before_rights_reads_as_read_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = clients_file(dir.path());
        fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        let old = format!(
            r#"{{"clients":[{{"id":"a1b2c3d4","name":"Script","kind":"script","keySha256":"{}","created":"2026-10-11T08:00:00Z","lastUsed":null}}]}}"#,
            "1".repeat(64)
        );
        fs::write(&file, old).expect("write");
        let entries = read_clients(&file).expect("an older file is not damaged");
        assert_eq!(entries[0].record.rights, read_only());
    }

    /// A `clients.json` with one client limited to the calendar `work` and the task lists
    /// `tasks` (both by bare URL, as before M1.1) and `new` (already with its account).
    fn write_legacy_selections(dir: &Path) -> PathBuf {
        let file = clients_file(dir);
        fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        let mut rights = read_only();
        rights.calendars = Selection::Only(vec!["https://cloud.example.org/dav/calendars/anna/work/".into()]);
        rights.task_lists = Selection::Only(vec![
            "https://cloud.example.org/dav/calendars/anna/tasks/".into(),
            "nextcloud:anna@cloud.example.org/https://cloud.example.org/dav/calendars/anna/new/".into(),
        ]);
        let record = ClientRecord {
            id: "a1b2c3d4".into(),
            name: "Script".into(),
            kind: "script".into(),
            key_sha256: "1".repeat(64),
            created: Utc::now(),
            last_used: None,
            rights,
            agent: None,
        };
        fs::write(&file, serde_json::to_vec(&ClientsFile { clients: vec![record] }).expect("json")).expect("write");
        file
    }

    fn selections(clients: &Clients) -> (Selection, Selection) {
        let rights = clients.records()[0].rights.clone();
        (rights.calendars, rights.task_lists)
    }

    #[test]
    fn selections_from_before_several_accounts_get_the_account_and_are_stored() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = write_legacy_selections(dir.path());
        let accounts = [
            torro_core::Account::nextcloud("https://cloud.example.org", "anna"),
            torro_core::Account::nextcloud("https://other.example.org", "bob"),
        ];
        let accounts_file = file.with_file_name(torro_core::account::ACCOUNTS_FILE);
        fs::write(&accounts_file, serde_json::to_vec(&accounts).expect("json")).expect("accounts");

        let anna = "nextcloud:anna@cloud.example.org";
        let expected = (
            Selection::Only(vec![format!("{anna}/https://cloud.example.org/dav/calendars/anna/work/")]),
            Selection::Only(vec![
                format!("{anna}/https://cloud.example.org/dav/calendars/anna/tasks/"),
                format!("{anna}/https://cloud.example.org/dav/calendars/anna/new/"),
            ]),
        );
        assert_eq!(selections(&open_in(dir.path())), expected, "rewritten in memory");
        let stored = read_clients(&file).expect("still a valid file");
        assert_eq!(
            (stored[0].record.rights.calendars.clone(), stored[0].record.rights.task_lists.clone()),
            expected,
            "and in clients.json"
        );
        assert_eq!(selections(&open_in(dir.path())), expected, "a second start changes nothing");
    }

    #[test]
    fn without_an_account_old_selections_stay_as_they_are() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = write_legacy_selections(dir.path());
        let before = fs::read(&file).expect("read");
        let clients = open_in(dir.path());
        assert!(matches!(selections(&clients).0, Selection::Only(ids) if ids[0].starts_with("https://")));
        assert_eq!(fs::read(&file).expect("read"), before, "nothing written");
    }

    #[test]
    fn client_keychain_accounts_are_told_from_account_ids() {
        assert!(is_client_keychain_account(&keychain_account("a1b2c3d4")));
        assert!(!is_client_keychain_account("nextcloud:anna@cloud.example.org"));
    }

    #[test]
    fn a_bad_hash_makes_the_file_count_as_damaged() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = clients_file(dir.path());
        fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        let record =
            r#"{"clients":[{"id":"a","name":"A","kind":"script","keySha256":"zz","created":"2026-10-11T08:00:00Z"}]}"#;
        fs::write(&file, record).expect("write");
        assert!(matches!(read_clients(&file), Err(ClientsError::Damaged(_))));
    }

    #[test]
    fn a_twice_listed_id_or_hash_makes_the_file_count_as_damaged() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = clients_file(dir.path());
        fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        let record = |id: &str, hash: &str| {
            format!(
                r#"{{"id":"{id}","name":"A","kind":"script","keySha256":"{hash}","created":"2026-10-11T08:00:00Z"}}"#
            )
        };
        let (one, two) = ("1".repeat(64), "2".repeat(64));
        for clients in [[record("a", &one), record("a", &two)], [record("a", &one), record("b", &one)]] {
            fs::write(&file, format!(r#"{{"clients":[{}]}}"#, clients.join(","))).expect("write");
            assert!(matches!(read_clients(&file), Err(ClientsError::Damaged(_))), "{clients:?}");
        }
        fs::write(&file, format!(r#"{{"clients":[{},{}]}}"#, record("a", &one), record("b", &two))).expect("write");
        assert_eq!(read_clients(&file).expect("two different clients").len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_clients_json_readable_by_others_is_restricted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let file = clients_file(dir.path());
        fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        fs::write(&file, r#"{"clients":[]}"#).expect("write");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).expect("chmod");
        let _ = open_in(dir.path());
        assert_eq!(fs::metadata(&file).expect("exists").permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn a_missing_clients_json_means_no_clients_not_no_checks() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        assert!(!clients_file(dir.path()).exists());
        assert_eq!(code(clients.admit(Some(&Key::new("torro_x_y")), &Request::Status)), "unauthorized");
    }

    #[test]
    fn last_use_is_noted_to_the_minute() {
        let start = Utc::now();
        let mut record = ClientRecord {
            id: "a".into(),
            name: "A".into(),
            kind: "script".into(),
            key_sha256: String::new(),
            created: start,
            last_used: None,
            rights: legacy_rights(),
            agent: None,
        };
        assert!(note_use(&mut record, start));
        assert!(!note_use(&mut record, start + chrono::TimeDelta::seconds(30)));
        assert_eq!(record.last_used, Some(start));
        assert!(note_use(&mut record, start + chrono::TimeDelta::seconds(61)));
    }

    #[test]
    fn hashes_compare_whole() {
        let a = hash("torro_a_1");
        let mut b = a;
        assert!(same(&a, &b));
        b[31] ^= 1;
        assert!(!same(&a, &b));
        assert_eq!(from_hex(&hex(&a)), Some(a));
        assert_eq!(from_hex("ab"), None);
    }

    #[tokio::test]
    async fn a_hook_client_belongs_to_one_agent_and_keeps_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let hook = Rights::hook();
        let without = clients.add_client("Hooks", HOOK_CLIENT, None, hook.clone()).await;
        assert!(matches!(without, Err(ClientsError::Invalid(_))), "a hook names its agent");
        let other = clients.add_client("Codex", "codex", Some(AgentKind::Codex), read_only()).await;
        assert!(matches!(other, Err(ClientsError::Invalid(_))), "only hooks name an agent");
        let (added, _) =
            clients.add_client("Codex hooks", HOOK_CLIENT, Some(AgentKind::Codex), hook).await.expect("added");
        assert_eq!(added.agent, Some(AgentKind::Codex));
        assert_eq!(clients.list().len(), 1, "nothing added by the refused ones");

        let reopened = open_in(dir.path());
        assert_eq!(reopened.list()[0].agent, Some(AgentKind::Codex), "kept in clients.json");
    }

    #[tokio::test]
    async fn script_names_are_inbox_sources_and_unique() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        clients.add("Backup", "script", read_only()).await.expect("added");
        for name in ["backup", " BACKUP ", "cli", "Cli"] {
            let refused = clients.add(name, "script", read_only()).await;
            assert!(matches!(&refused, Err(err) if err.code() == "conflict"), "{name}");
        }
        // Other kinds file under `agent:<kind>`, so their names may repeat.
        clients.add("Backup", "claude-code", read_only()).await.expect("added");
        clients.add("cli", "codex", read_only()).await.expect("added");
        assert_eq!(clients.list().len(), 3);
    }

    #[cfg(unix)]
    /// Decision 5 in `docs/assistent.md`: one client "Assistant" for every engine. A second one is
    /// `conflict`; its first-start rights read calendars, tasks and the inbox only.
    #[tokio::test]
    async fn there_is_one_assistant_client() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (assistant, _) = clients.add("Assistant", ASSISTANT_CLIENT, Rights::assistant()).await.expect("added");
        assert_eq!((assistant.kind.as_str(), &assistant.rights), (ASSISTANT_CLIENT, &Rights::assistant()));
        let again = clients.add("Assistant 2", ASSISTANT_CLIENT, RightsTemplate::FullAccess.rights()).await;
        assert!(matches!(again, Err(ClientsError::Conflict(_))), "{again:?}");
        assert_eq!(clients.list().len(), 1);
        clients.remove(&assistant.id).await.expect("removed");
        clients.add("Assistant", ASSISTANT_CLIENT, Rights::assistant()).await.expect("again after removing it");
    }

    #[test]
    fn an_existing_state_directory_is_made_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let parent = dir.path().join("local");
        let state = parent.join("state");
        fs::create_dir_all(&state).expect("dirs");
        for path in [&parent, &state] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("mode");
        }
        create_private_dir(&state).expect("made private");
        let mode = |path: &Path| fs::metadata(path).expect("exists").permissions().mode() & 0o777;
        assert_eq!(mode(&state), 0o700);
        assert_eq!(mode(&parent), 0o755, "a parent that was there keeps its mode");
    }

    /// Task M2.7b: a removed account leaves every selection, in memory and in `clients.json`;
    /// another account whose id starts with the same text keeps its collections, `All` stays.
    #[tokio::test]
    async fn forgetting_an_account_takes_it_out_of_every_selection() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (anna, nested) = ("mail:anna@example.org", "nextcloud:anna@cloud.example.org/nc");
        let (cloud, github) = ("nextcloud:anna@cloud.example.org", "github:anna");
        let at = |account: &str, url: &str| format!("{account}/{url}");
        let mut rights = read_only();
        rights.mail_accounts = Selection::Only(vec![anna.into(), "mail:bob@example.org".into()]);
        // Task E5.2: a Nextcloud account's Talk goes with it, the other account's stays.
        let (talk, nested_talk) = ("talk:anna@cloud.example.org", "talk:anna@cloud.example.org/nc");
        rights.inbox_sources =
            Selection::Only([anna, "agent:codex", cloud, talk, nested_talk, github].map(String::from).to_vec());
        rights.calendars = Selection::Only(vec![
            at(anna, "https://dav.example.org/work/"),
            at(cloud, "https://cloud.example.org/remote.php/dav/calendars/anna/home/"),
            at(nested, "https://cloud.example.org/nc/remote.php/dav/calendars/anna/home/"),
        ]);
        rights.task_lists = Selection::Only(vec![at(anna, "https://dav.example.org/todo/"), at(github, "list-1")]);
        let (agent, _) = clients.add("Agent", "claude-code", rights).await.expect("added");
        let mut everything = read_only();
        everything.mail_accounts = Selection::All;
        let (all, _) = clients.add("All", "codex", everything).await.expect("added");
        let known: Vec<String> = [anna, cloud, nested, github].map(String::from).to_vec();

        clients.forget_account(anna, &known).await.expect("forgotten");
        clients.forget_account(cloud, &known).await.expect("forgotten");
        clients.forget_account(github, &known).await.expect("forgotten");
        let expected = |rights: &Rights| {
            assert_eq!(rights.mail_accounts, Selection::Only(vec!["mail:bob@example.org".into()]));
            assert_eq!(rights.inbox_sources, Selection::Only(vec!["agent:codex".into(), nested_talk.into()]));
            assert_eq!(
                rights.calendars,
                Selection::Only(vec![at(nested, "https://cloud.example.org/nc/remote.php/dav/calendars/anna/home/")])
            );
            assert_eq!(rights.task_lists, Selection::Only(vec![]));
        };
        let listed = |id: &str| clients.list().into_iter().find(|client| client.id == id).expect("listed").rights;
        expected(&listed(&agent.id));
        assert_eq!(listed(&all.id).mail_accounts, Selection::All);
        let reopened = open_in(dir.path());
        let stored = reopened.records().into_iter().find(|record| record.id == agent.id).expect("stored").rights;
        expected(&stored);
    }

    /// Issue #133: a `forbidden` from the gate names its cause – the engine that never gets mail, an
    /// area switched off in the conversation, a level too low, an area not granted – and its message
    /// tells the agent so.
    #[tokio::test]
    async fn a_refusal_of_the_gate_names_its_cause() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients =
            Clients::open(&app_key_file(dir.path()), &clients_file(dir.path()), Box::new(MemoryKeychain::default()))
                .expect("opens");
        let mut rights = Rights::assistant();
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        rights.mail_accounts = Selection::All;
        let (assistant, own) = clients.add("Assistant", ASSISTANT_CLIENT, rights).await.expect("added");
        let refused = |key: &Key, request: &Request| clients.admit(Some(key), request).expect_err("refused");
        let message = |refused: Refused| match refused.response() {
            Response::Error { code, message } if code == "forbidden" => message,
            other => panic!("{other:?}"),
        };
        let folders = Request::MailFolders { account: None };

        // 1. Claude Code never gets mail, whatever the rights say.
        let agent = clients.lend(&assistant.id, "claude-code", &[Area::Mail, Area::Tasks]).expect("lent");
        let never = refused(agent.key(), &folders);
        assert_eq!(
            never,
            Refused::Lacks { command: "mail.list_folders", lack: Lack::NeverToAgents { area: Area::Mail } }
        );
        assert!(message(never).contains("not given to a coding agent"));
        // 2. Switched off for this conversation, with an agent and with an AI account alike.
        let tasks = refused(agent.key(), &Request::TaskCollections);
        assert_eq!(
            tasks,
            Refused::Lacks { command: "tasks.list_lists", lack: Lack::SwitchedOff { area: Area::Tasks } }
        );
        assert!(message(tasks).contains("switched off tasks for this conversation"));
        let model = clients.lend(&assistant.id, "ai:openrouter@openrouter.ai", &[Area::Mail]).expect("lent");
        assert_eq!(
            refused(model.key(), &folders),
            Refused::Lacks { command: "mail.list_folders", lack: Lack::SwitchedOff { area: Area::Mail } }
        );
        // 3. More than reading, with the client's own key.
        let draft = Request::DeleteMail { ids: vec!["m:1".into()], permanent: false };
        let level = refused(&own, &draft);
        assert_eq!(
            level,
            Refused::Lacks {
                command: "mail.delete_messages",
                lack: Lack::Level { area: Area::Mail, needs: Access::Delete }
            }
        );
        assert!(message(level).contains("needs full access to mail"));
        // 4. Mail accounts are listed in `accounts`.
        let accounts = refused(&own, &Request::Accounts);
        assert_eq!(
            accounts,
            Refused::Lacks { command: "accounts.list_accounts", lack: Lack::Area { area: Area::Accounts } }
        );
        assert!(message(accounts).contains("no access to accounts"));
    }

    #[tokio::test]
    async fn a_lent_key_stands_for_its_client_without_some_areas_until_it_is_dropped() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keychain = MemoryKeychain::default();
        let clients = Clients::open(&app_key_file(dir.path()), &clients_file(dir.path()), Box::new(keychain.clone()))
            .expect("opens");
        let mut rights = Rights::assistant();
        rights
            .areas
            .insert(Area::Mail, torro_ipc::api::AreaRights { level: torro_ipc::api::Level::Read, admin: false });
        let (assistant, _) = clients.add("Assistant", ASSISTANT_CLIENT, rights).await.expect("added");
        let stored_before = fs::read_to_string(clients_file(dir.path())).expect("stored");

        let lent = clients.lend(&assistant.id, "claude-code", &[Area::Mail, Area::Tasks]).expect("lent");
        assert_eq!(format!("{lent:?}"), "LentKey(…)", "Debug never shows the key");
        let key = lent.key().clone();
        assert!(key.expose().starts_with(&format!("torro_{}_", assistant.id)));
        assert_eq!(clients.lent_for(&key).as_deref(), Some("claude-code"));
        assert_eq!(clients.lent_for(&app_key(dir.path())), None);
        assert_eq!(clients.lent_approvals(&key), Some(Vec::new()));
        clients.note_lent_approval(&key, "0123456789abcdef");
        assert_eq!(clients.lent_approvals(&key), Some(vec!["0123456789abcdef".to_string()]));
        assert_eq!(clients.lent_approvals(&app_key(dir.path())), None, "only lent keys are narrowed");

        // The client with its rights of now, without the areas it was lent without.
        let Ok(Caller::Client { id, rights }) = clients.admit(Some(&key), &Request::CalendarCollections) else {
            panic!("a lent key gets its client in");
        };
        assert_eq!(id, assistant.id);
        assert!(rights.areas.contains_key(&Area::Calendar));
        assert!(!rights.areas.contains_key(&Area::Mail) && !rights.areas.contains_key(&Area::Tasks));
        let mail = Request::MailFolders { account: None };
        assert_eq!(code(clients.admit(Some(&key), &mail)), "forbidden", "mail was lent without");
        assert!(clients.admit(Some(&key), &Request::TaskCollections).is_err(), "tasks too");

        // Never on disk, never in the keychain.
        assert_eq!(fs::read_to_string(clients_file(dir.path())).expect("stored"), stored_before);
        assert!(!fs::read_to_string(clients_file(dir.path())).expect("stored").contains(key.expose()));
        assert!(keychain.0.lock().expect("lock").values().all(|stored| stored != key.expose()));

        // Dropped: taken back.
        drop(lent);
        assert_eq!(code(clients.admit(Some(&key), &Request::CalendarCollections)), "unauthorized");
        assert_eq!(clients.lent_for(&key), None);

        // Removing the client ends a key lent to it as well.
        let lent = clients.lend(&assistant.id, "claude-code", &[]).expect("lent");
        clients.remove(&assistant.id).await.expect("removed");
        assert_eq!(code(clients.admit(Some(lent.key()), &Request::CalendarCollections)), "unauthorized");
        assert!(matches!(clients.lend(&assistant.id, "claude-code", &[]), Err(ClientsError::NotFound(_))));
    }
}
