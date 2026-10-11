//! Who may talk to the core: keys and sign-in (`docs/api.md`, "Anmeldung").
//!
//! Two kinds of keys arrive with the requests on the socket:
//!
//! - The **app key** of the suite's own programs (control centre, apps, tray, `ctl`). The core
//!   creates it on its first start as `app.key` next to the socket (mode 0600); `torro_ipc::client`
//!   reads and sends it, so the programs never handle it. It may do everything.
//! - **Client keys** for scripts and agents, `torro_<id>_<64 hex>`. The plain key exists only in
//!   the keychain (service `torroworkplace`, account `client-<id>`) and at the client;
//!   `clients.json` (config directory, 0600) holds its SHA-256 with name, kind and last use.
//!
//! Every request is checked against what the core holds in memory, so removing a client takes
//! effect with its very next request. Keys are compared as SHA-256 hashes in constant time and
//! never logged. **Fail closed:** a damaged `clients.json` lets in only the app key, and a
//! missing one means "no clients", never "no checks".
//!
//! Until the rights per area exist (A.2 in `docs/tasks/api-mcp.md`), a client key may only
//! `system.ping`; everything else is `forbidden`. So no agent gets in before its rights can be
//! checked.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use torro_ipc::{Key, Request, Response};

/// Keychain service all suite secrets are filed under (as for the accounts).
const KEYCHAIN_SERVICE: &str = "torroworkplace";

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
    /// A script or agent with its own key; `id` is its entry in `clients.json`.
    Client { id: String },
    /// No key or an unknown one; only `system.ping` is answered.
    Anonymous,
}

/// Why a request was not let in ([`Clients::admit`]); `command` is its catalogue name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// No key, or one the core does not know (any more): error code `unauthorized`.
    Unauthorized { command: &'static str },
    /// A known client whose rights do not cover the command: error code `forbidden`.
    Forbidden { command: &'static str },
}

impl Refused {
    /// The answer to the client. It names the command, never the key.
    pub fn response(self) -> Response {
        let (code, message) = match self {
            Refused::Unauthorized { command } => ("unauthorized", format!("{command} needs a valid key")),
            Refused::Forbidden { command } => ("forbidden", format!("this client may not use {command}")),
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
    /// Which program: `claude-code`, `codex`, `claude-desktop`, `cursor`, `script`.
    pub kind: String,
    /// SHA-256 of the key, 64 lowercase hex digits.
    pub key_sha256: String,
    pub created: DateTime<Utc>,
    /// The last request with this key, to the minute.
    #[serde(default)]
    pub last_used: Option<DateTime<Utc>>,
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
        }
    }
}

impl std::error::Error for ClientsError {}

/// A client in memory: its record and the hash to compare against.
struct Entry {
    record: ClientRecord,
    hash: KeyHash,
}

struct State {
    clients: Vec<Entry>,
    /// `clients.json` could not be read at start. Nothing is written to it then, so the person
    /// can still repair it, and only the app key gets in.
    damaged: bool,
}

/// The gate of the core: knows the app key and the clients and decides who gets in.
pub struct Clients {
    app_key: KeyHash,
    file: PathBuf,
    keychain: Box<dyn KeyStore>,
    state: Mutex<State>,
}

impl Clients {
    /// Reads the app key from `app_key` or creates it there (mode 0600), and the clients from
    /// `file` (`clients.json`).
    ///
    /// Fails only when the app key can neither be read nor created. A missing `clients.json`
    /// means no clients; a damaged one is reported on stderr and leaves only the app key valid
    /// (fail closed).
    pub fn open(app_key: &Path, file: &Path, keychain: Box<dyn KeyStore>) -> Result<Clients, ClientsError> {
        let app_key = hash(&load_or_create_app_key(app_key)?);
        let state = match read_clients(file) {
            Ok(clients) => State { clients, damaged: false },
            Err(err) => {
                eprintln!("torroworkplace: {err}; only the suite's own programs are let in");
                State { clients: Vec::new(), damaged: true }
            }
        };
        Ok(Clients { app_key, file: file.to_path_buf(), keychain, state: Mutex::new(state) })
    }

    /// Decides whether `request` with `key` may run: the caller, or why not.
    ///
    /// - The app key may run everything.
    /// - `system.ping` runs for anyone, with or without a valid key.
    /// - A client key may run nothing else yet: [`Refused::Forbidden`] until A.2 checks rights
    ///   per area.
    /// - No key or an unknown or removed one: [`Refused::Unauthorized`].
    pub fn admit(&self, key: Option<&Key>, request: &Request) -> Result<Caller, Refused> {
        let caller = key.map_or(Caller::Anonymous, |key| self.identify(key));
        let command = request.operation().name();
        match caller {
            Caller::App => Ok(Caller::App),
            caller if matches!(request, Request::Ping) => Ok(caller),
            Caller::Client { .. } => Err(Refused::Forbidden { command }),
            Caller::Anonymous => Err(Refused::Unauthorized { command }),
        }
    }

    /// Who `key` belongs to. A known client's last use is noted.
    fn identify(&self, key: &Key) -> Caller {
        let hash = hash(key.expose());
        if same(&hash, &self.app_key) {
            return Caller::App;
        }
        let mut state = self.lock();
        // Compare with every client, not stopping at the first match, so the time taken does not
        // depend on where a key is in the list.
        let mut found = None;
        for (index, entry) in state.clients.iter().enumerate() {
            if same(&entry.hash, &hash) {
                found = Some(index);
            }
        }
        let Some(index) = found else {
            return Caller::Anonymous;
        };
        let id = state.clients[index].record.id.clone();
        if note_use(&mut state.clients[index].record, Utc::now()) {
            self.save_or_warn(&state);
        }
        Caller::Client { id }
    }

    /// Adds a client and returns its id and key. The key is shown to the person once; the core
    /// keeps only its hash and the keychain the key.
    #[cfg_attr(not(test), expect(dead_code, reason = "A.2 offers it as clients.create"))]
    pub(crate) fn add(&self, name: &str, kind: &str) -> Result<(String, Key), ClientsError> {
        let mut state = self.lock();
        if state.damaged {
            return Err(ClientsError::Damaged(self.file.clone()));
        }
        let id = loop {
            let id = hex(&random::<4>()?);
            if !state.clients.iter().any(|entry| entry.record.id == id) && id != "app" {
                break id;
            }
        };
        let key = format!("torro_{id}_{}", hex(&random::<32>()?));
        self.keychain.set(&keychain_account(&id), &key).map_err(ClientsError::Keychain)?;

        let hash = hash(&key);
        let record = ClientRecord {
            id: id.clone(),
            name: name.to_string(),
            kind: kind.to_string(),
            key_sha256: hex(&hash),
            created: Utc::now(),
            last_used: None,
        };
        state.clients.push(Entry { record, hash });
        if let Err(err) = self.save(&state) {
            // Without the file the client would be gone after a restart; take it back now.
            state.clients.pop();
            let _ = self.keychain.delete(&keychain_account(&id));
            return Err(err);
        }
        Ok((id, Key::new(key)))
    }

    /// Removes a client. Its key stops working at once, even when the file or the keychain
    /// cannot be updated; that is then reported as an error.
    #[cfg_attr(not(test), expect(dead_code, reason = "A.2 offers it as clients.remove"))]
    pub(crate) fn remove(&self, id: &str) -> Result<(), ClientsError> {
        let mut state = self.lock();
        let before = state.clients.len();
        state.clients.retain(|entry| entry.record.id != id);
        if state.clients.len() == before {
            return Err(ClientsError::NotFound(id.to_string()));
        }
        self.save(&state)?;
        self.keychain.delete(&keychain_account(id)).map_err(ClientsError::Keychain)
    }

    /// The clients as `clients.json` lists them.
    #[cfg(test)]
    fn records(&self) -> Vec<ClientRecord> {
        self.lock().clients.iter().map(|entry| entry.record.clone()).collect()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Every change leaves the state whole before anything that could panic, so a poisoned
        // lock is still good to use.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Writes `clients.json` (0600). Never while it is damaged.
    fn save(&self, state: &State) -> Result<(), ClientsError> {
        if state.damaged {
            return Err(ClientsError::Damaged(self.file.clone()));
        }
        let file = ClientsFile { clients: state.clients.iter().map(|entry| entry.record.clone()).collect() };
        let mut json = serde_json::to_vec_pretty(&file)
            .map_err(|err| ClientsError::Io { path: self.file.clone(), err: io::Error::other(err) })?;
        json.push(b'\n');
        write_private(&self.file, &json)
    }

    fn save_or_warn(&self, state: &State) {
        if let Err(err) = self.save(state) {
            eprintln!("torroworkplace: cannot note when a client was last used: {err}");
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
    format!("client-{id}")
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
/// id or hash may appear twice, or the whole file counts as damaged.
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
        .map(|record| Ok(Entry { hash: from_hex(&record.key_sha256).ok_or_else(damaged)?, record }))
        .collect::<Result<Vec<_>, ClientsError>>()?;
    let ids: HashSet<_> = entries.iter().map(|entry| entry.record.id.as_str()).collect();
    let hashes: HashSet<_> = entries.iter().map(|entry| entry.hash).collect();
    if ids.len() != entries.len() || hashes.len() != entries.len() {
        return Err(damaged());
    }
    Ok(entries)
}

/// Writes `bytes` to `path` atomically, readable and writable only by the owner on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), ClientsError> {
    let tmp = write_temporary(path, bytes)?;
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        ClientsError::Io { path: path.to_path_buf(), err }
    })
}

/// Writes `bytes` to a new temporary file next to `path` (mode 0600 on Unix) and returns its
/// path. The name holds the process id and a random part, so cores starting at the same time
/// never share one.
fn write_temporary(path: &Path, bytes: &[u8]) -> Result<PathBuf, ClientsError> {
    let io_error = |err| ClientsError::Io { path: path.to_path_buf(), err };
    if let Some(dir) = path.parent() {
        create_private_dir(dir).map_err(io_error)?;
    }
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

/// Creates `dir` and its missing parents; on Unix the ones it creates are for the owner only.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
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
    use std::sync::Arc;

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
            assert_eq!(clients.app_key, hash(key.expose()));
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
                            open_in(dir.path()).app_key
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

    #[test]
    fn errors_never_contain_the_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (_, key) = clients.add("Claude Code", "claude-code").expect("added");
        for key in [key, Key::new("torro_x_wrongsecret")] {
            let Err(refused) = clients.admit(Some(&key), &Request::Accounts) else {
                panic!("refused");
            };
            let Response::Error { message, .. } = refused.response() else { panic!("an error") };
            assert!(!message.contains(key.expose()), "{message}");
            assert!(message.contains("accounts.list_accounts"), "names the command: {message}");
        }
    }

    #[test]
    fn a_client_key_may_only_ping_until_rights_exist() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keychain = MemoryKeychain::default();
        let clients = Clients::open(&app_key_file(dir.path()), &clients_file(dir.path()), Box::new(keychain.clone()))
            .expect("opens");
        let (id, key) = clients.add("Claude Code", "claude-code").expect("added");

        assert!(key.expose().starts_with(&format!("torro_{id}_")));
        assert_eq!(keychain.get(&format!("client-{id}")).as_deref(), Some(key.expose()), "plain key in the keychain");
        assert_eq!(clients.admit(Some(&key), &Request::Ping), Ok(Caller::Client { id: id.clone() }));
        assert_eq!(code(clients.admit(Some(&key), &Request::CalendarCollections)), "forbidden");
        assert_eq!(code(clients.admit(Some(&key), &Request::Status)), "forbidden");
    }

    #[test]
    fn clients_json_holds_the_hash_not_the_key_and_is_private() {
        let dir = tempfile::tempdir().expect("temp dir");
        let clients = open_in(dir.path());
        let (id, key) = clients.add("Script", "script").expect("added");

        let text = fs::read_to_string(clients_file(dir.path())).expect("written");
        assert!(!text.contains(key.expose()));
        assert!(!text.contains(key.expose().rsplit('_').next().expect("secret part")));
        assert!(text.contains(&hex(&hash(key.expose()))));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(clients_file(dir.path())).expect("exists").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // A restart reads the client back and still knows its key.
        let again = open_in(dir.path());
        assert_eq!(again.admit(Some(&key), &Request::Ping), Ok(Caller::Client { id }));
    }

    #[test]
    fn a_removed_client_is_refused_with_its_next_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keychain = MemoryKeychain::default();
        let clients = Clients::open(&app_key_file(dir.path()), &clients_file(dir.path()), Box::new(keychain.clone()))
            .expect("opens");
        let (id, key) = clients.add("Codex", "codex").expect("added");
        assert!(matches!(clients.admit(Some(&key), &Request::Ping), Ok(Caller::Client { .. })));

        clients.remove(&id).expect("removed");
        assert_eq!(clients.admit(Some(&key), &Request::Ping), Ok(Caller::Anonymous));
        assert_eq!(code(clients.admit(Some(&key), &Request::Status)), "unauthorized");
        assert_eq!(keychain.get(&format!("client-{id}")), None, "key gone from the keychain");
        assert!(open_in(dir.path()).records().is_empty(), "and from clients.json");
        assert!(matches!(clients.remove(&id), Err(ClientsError::NotFound(_))));
    }

    #[test]
    fn a_damaged_clients_json_lets_in_only_the_app_key_and_is_left_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (_, key) = open_in(dir.path()).add("Script", "script").expect("added");
        let file = clients_file(dir.path());
        let damaged = fs::read_to_string(&file).expect("written").replace("\"clients\"", "\"clients\" oops");
        fs::write(&file, &damaged).expect("damage it");

        let clients = open_in(dir.path());
        assert_eq!(code(clients.admit(Some(&key), &Request::Status)), "unauthorized");
        assert_eq!(clients.admit(Some(&key), &Request::Ping), Ok(Caller::Anonymous));
        assert_eq!(clients.admit(Some(&app_key(dir.path())), &Request::Status), Ok(Caller::App));

        assert!(matches!(clients.add("Other", "script"), Err(ClientsError::Damaged(_))));
        assert_eq!(fs::read_to_string(&file).expect("still there"), damaged, "not overwritten");
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
}
