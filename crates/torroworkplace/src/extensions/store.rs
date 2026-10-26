//! Where the core keeps its MCP servers (task MA.3): `<data>/extensions/extensions.json` and their secrets
//! in the keychain.
//!
//! The file holds per server its id, name, source, command or address with its variables or headers, whether
//! it is on, the tools a person allowed (with fingerprint and access) and the tools it last reported. It is
//! written atomically, the folder 0700 and the file 0600 on Unix. A secret never reaches it: on disk every
//! secret is [`Value::Stored`] ([`stored_form`]), and its value lives in the keychain under
//! `ext:<id>:<NAME>` ([`secret_key`]) through a [`SecretStore`]; the core's is [`SystemSecrets`] (the vault of
//! `torro_core::account`), tests use a map. Agents that may not see a server's settings get them through
//! [`redacted`].
//!
//! Keychain calls can block (macOS may ask the person), so the async core runs the functions that take a
//! [`SecretStore`] off its runtime (`crate::account::keychain`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use torro_core::account::{Account, AccountKind};
use torro_ipc::Secret;
use torro_ipc::api::Access;
use torro_ipc::extensions::{SecretInfo, ServerSpec, Source, Value, valid_id};

use crate::clients::{create_private_dir, write_private};

/// The folder below the core's data folder that holds everything of the MCP servers.
pub const DIR: &str = "extensions";

/// The file in [`DIR`] that lists them.
pub const FILE: &str = "extensions.json";

/// The version of the file this code writes and reads.
const VERSION: u32 = 1;

/// What [`redacted`] puts in place of a value or an argument.
pub const REDACTED: &str = "…";

/// What the keychain entries of all servers start with: `ext:<id>:<NAME>` ([`secret_key`]).
pub const SECRET_PREFIX: &str = "ext:";

/// One installed server as the file keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredExtension {
    /// Its id ([`valid_id`]), unique in the file.
    pub id: String,
    /// What people call it.
    pub name: String,
    pub source: Source,
    /// How the core reaches it, in [`stored_form`]: no secret value, only [`Value::Stored`].
    pub server: ServerSpec,
    /// A person switched it on.
    pub enabled: bool,
    /// The tools a person allowed, as the server described them then. These are offered (to whom may use
    /// them) while the server is not running, and a call starts it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<AllowedTool>,
    /// Every tool the server reported when it last ran, allowed or not, so the page of a server in
    /// `review` still shows what to allow after the core restarted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reported: Vec<ReportedTool>,
    /// The allowed folder a person chose for a catalog server that works in one (`extensions.set_folder`,
    /// issue #303), as its path; `None` while none is chosen. Files written before have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// A tool a person allowed (`extensions.confirm_tools`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowedTool {
    /// Its name as the server calls it.
    pub name: String,
    /// Its description as allowed: the server's text, data, never instructions.
    pub description: String,
    /// Its input schema as allowed: the server's text too.
    pub input_schema: serde_json::Value,
    /// The access a call needs, from the catalog entry (`delete` without one); never `admin`.
    pub access: Access,
    /// The SHA-256 over name, description and input schema the person allowed (task MA.4 computes it). A
    /// tool the server reports with another fingerprint is in `review` again.
    pub fingerprint: String,
}

/// A tool as the server last reported it. All of it is the server's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// The file as written.
#[derive(Debug, Serialize, Deserialize)]
struct Contents {
    version: u32,
    #[serde(default)]
    extensions: Vec<StoredExtension>,
}

/// Why the file, a folder or the keychain failed. `Display` says it in English for the log and the `message`
/// of `error`; it never holds a secret.
#[derive(Debug)]
pub enum StoreError {
    /// Reading or writing `path` failed.
    Io { path: PathBuf, err: io::Error },
    /// The file is not what this code writes: damaged, a newer version, an invalid or doubled id. It is left
    /// as it is: [`Store::save`] would replace it, so the core must not save over a file it could not read.
    Damaged { path: PathBuf, reason: String },
    /// The keychain refused; its message.
    Keychain(String),
    /// Not a valid server id ([`valid_id`]); nothing was read, written or removed for it.
    InvalidId(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io { path, err } => write!(f, "{}: {err}", path.display()),
            StoreError::Damaged { path, reason } => write!(f, "{} is damaged: {reason}", path.display()),
            StoreError::Keychain(err) => write!(f, "keychain: {err}"),
            StoreError::InvalidId(id) => write!(f, "{id:?} is no valid id of an MCP server"),
        }
    }
}

impl std::error::Error for StoreError {}

/// The folder of the MCP servers: [`FILE`] and one folder per server with its working folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// The store in `dir`, which is `<data>/extensions` in the core ([`Store::in_data`]); nothing is read or
    /// created yet.
    pub fn new(dir: PathBuf) -> Store {
        Store { dir }
    }

    /// The store below the core's data folder `data`: `<data>/extensions`.
    pub fn in_data(data: &Path) -> Store {
        Store::new(data.join(DIR))
    }

    /// Its folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file that lists the servers.
    pub fn file(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    /// The servers in the file, in its order; none when there is no file yet. Every server comes in
    /// [`stored_form`], even from a file someone edited by hand. A file this code cannot read is
    /// [`StoreError::Damaged`] and stays untouched.
    pub fn load(&self) -> Result<Vec<StoredExtension>, StoreError> {
        let path = self.file();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(StoreError::Io { path, err }),
        };
        let damaged = |reason: String| StoreError::Damaged { path: path.clone(), reason };
        let contents: Contents = serde_json::from_str(&text).map_err(|err| damaged(err.to_string()))?;
        if contents.version != VERSION {
            return Err(damaged(format!("version {}; this version reads {VERSION}", contents.version)));
        }
        check_ids(&contents.extensions).map_err(damaged)?;
        Ok(contents
            .extensions
            .into_iter()
            .map(|extension| StoredExtension { server: stored_form(&extension.server), ..extension })
            .collect())
    }

    /// Replaces the file with `extensions`, atomically, creating the folder (0700) and the file (0600 on Unix).
    /// Each server is written in [`stored_form`], so a [`Value::Secret`] left in a spec never reaches the
    /// disk. Refuses an invalid or doubled id ([`StoreError::InvalidId`]) and writes nothing then.
    pub fn save(&self, extensions: &[StoredExtension]) -> Result<(), StoreError> {
        if let Err(reason) = check_ids(extensions) {
            return Err(StoreError::InvalidId(reason));
        }
        let extensions = extensions
            .iter()
            .map(|extension| StoredExtension { server: stored_form(&extension.server), ..extension.clone() })
            .collect();
        let contents = Contents { version: VERSION, extensions };
        let path = self.file();
        let mut bytes = serde_json::to_vec_pretty(&contents)
            .map_err(|err| StoreError::Io { path: path.clone(), err: io::Error::other(err) })?;
        bytes.push(b'\n');
        create_private_dir(&self.dir).map_err(|err| StoreError::Io { path: self.dir.clone(), err })?;
        write_private(&path, &bytes).map_err(|err| StoreError::Io { path, err: io::Error::other(err.to_string()) })
    }

    /// The working folder of the server `id`, `<dir>/<id>/work`, created for the owner only (0700 on Unix):
    /// where its process starts (task MA.4).
    pub fn work_dir(&self, id: &str) -> Result<PathBuf, StoreError> {
        let work = self.work_path(id)?;
        create_private_dir(&work).map_err(|err| StoreError::Io { path: work.clone(), err })?;
        Ok(work)
    }

    /// Where [`Store::work_dir`] is, without making it: for a call, whose server may be removed meanwhile (the
    /// supervisor makes the folder once it knows the server is not stopped, `super::supervisor::Launch`).
    pub fn work_path(&self, id: &str) -> Result<PathBuf, StoreError> {
        Ok(self.server_dir(id)?.join("work"))
    }

    /// Removes the folder of the server `id` with everything in it (`extensions.remove_extension`); one that
    /// is not there is fine. Its line in the file and its secrets are the caller's ([`Store::save`],
    /// [`forget_secrets`]).
    pub fn remove_files(&self, id: &str) -> Result<(), StoreError> {
        let dir = self.server_dir(id)?;
        match std::fs::remove_dir_all(&dir) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(StoreError::Io { path: dir, err }),
            _ => Ok(()),
        }
    }

    /// `<dir>/<id>`; only for a valid id, so no id reaches outside the folder.
    fn server_dir(&self, id: &str) -> Result<PathBuf, StoreError> {
        match valid_id(id) {
            true => Ok(self.dir.join(id)),
            false => Err(StoreError::InvalidId(id.to_string())),
        }
    }
}

/// `Err` with the reason when an id is invalid or appears twice.
fn check_ids(extensions: &[StoredExtension]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for extension in extensions {
        if !valid_id(&extension.id) {
            return Err(format!("{:?} is no valid id", extension.id));
        }
        if !seen.insert(extension.id.as_str()) {
            return Err(format!("the id {:?} appears twice", extension.id));
        }
    }
    Ok(())
}

/// Where the secrets of the servers are kept, by keychain account (`ext:<id>:<NAME>`, [`secret_key`]): the
/// keychain in the core ([`SystemSecrets`]), a map in tests. Errors never contain a secret.
pub trait SecretStore: Send + Sync {
    /// The secret under `key`; `None` when there is none.
    fn get(&self, key: &str) -> Result<Option<Secret>, String>;
    /// Stores `secret` under `key`, replacing what was there.
    fn set(&self, key: &str, secret: &Secret) -> Result<(), String>;
    /// Removes the secret under `key`; a missing one is fine.
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// The keychain of the system through the vault of `torro_core::account`, where the account secrets are.
///
/// The vault of a process is reachable only through an [`Account`], so each call goes through one whose id
/// is the key. A second vault of its own would not do: on macOS all secrets share one keychain entry, and
/// two vaults in one process would write over each other's secrets.
pub struct SystemSecrets;

impl SystemSecrets {
    /// The account that stands for `key` in the vault: only its id counts.
    fn carrier(key: &str) -> Account {
        Account {
            id: key.to_string(),
            kind: AccountKind::Nextcloud,
            server: String::new(),
            login_name: String::new(),
            display_name: None,
            mail: None,
            mail_settings: None,
            ai: None,
            ntfy: None,
        }
    }
}

impl SecretStore for SystemSecrets {
    fn get(&self, key: &str) -> Result<Option<Secret>, String> {
        Self::carrier(key).secret().map(|secret| secret.map(Secret::new)).map_err(|err| err.to_string())
    }

    fn set(&self, key: &str, secret: &Secret) -> Result<(), String> {
        Self::carrier(key).set_secret(secret.expose()).map_err(|err| err.to_string())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        Self::carrier(key).delete_secret().map_err(|err| err.to_string())
    }
}

/// The keychain account of the secret `name` (a variable or header) of the server `id`: `ext:<id>:<NAME>`.
/// The id has no `:` ([`valid_id`]), so no two servers share a key.
pub fn secret_key(id: &str, name: &str) -> String {
    format!("{SECRET_PREFIX}{id}:{name}")
}

/// `spec` as the file keeps it: every [`Value::Secret`] (and every [`Value::Stored`]) becomes
/// `Value::Stored { secret: true }`, which on disk only marks the name as a secret; plain values, the
/// command, its arguments and the address stay.
pub fn stored_form(spec: &ServerSpec) -> ServerSpec {
    map_values(spec, |_, value| match value {
        Value::Plain(text) => Value::Plain(text.clone()),
        Value::Secret(_) | Value::Stored { .. } => Value::Stored { secret: true },
    })
}

/// `spec` for whom may see that a server is there but not how it is set up (a key that is not the app key):
/// the command stays, but of a path only its last part ([`redacted_command`]), each argument becomes [`REDACTED`], the address keeps only its origin
/// (`https://host:port`, followed by `/…` when it had more), and the value of every variable or header
/// becomes [`REDACTED`] (secrets stay [`Value::Stored`]). The names of variables and headers stay.
pub fn redacted(spec: &ServerSpec) -> ServerSpec {
    let hidden = map_values(spec, |_, value| match value {
        Value::Plain(_) => Value::Plain(REDACTED.to_string()),
        Value::Secret(_) => Value::Stored { secret: true },
        Value::Stored { secret } => Value::Stored { secret: *secret },
    });
    match hidden {
        ServerSpec::Stdio { command, args, env } => {
            let args = args.iter().map(|_| REDACTED.to_string()).collect();
            ServerSpec::Stdio { command: redacted_command(&command), args, env }
        }
        ServerSpec::Http { url, headers } => ServerSpec::Http { url: redacted_url(&url), headers },
    }
}

/// A command as [`redacted`] shows it: a name (`npx`) as it is, a path only by its last part after
/// [`REDACTED`] (`…/server`), since the folders before it may name the person's home and user name.
pub fn redacted_command(command: &str) -> String {
    match command.rsplit_once(['/', '\\']) {
        Some((_, "")) => REDACTED.to_string(),
        Some((_, name)) => format!("{REDACTED}/{name}"),
        None => command.to_string(),
    }
}

/// Whether the secrets stored for `old` may stay when a server's spec is replaced by `new`
/// (`extensions.update_extension`): only when the secrets would still go to the same place. For a program
/// the command and its arguments are the same (`npx` with another package in its arguments is another
/// program); for an address scheme, host and port are (the path may change). From one kind to the other they
/// never stay. When this is `false`, [`save_secrets`] removes them, so a changed command
/// or address never gets a secret meant for the one before.
pub fn keeps_secrets(old: &ServerSpec, new: &ServerSpec) -> bool {
    match (old, new) {
        (ServerSpec::Stdio { command: old, args: old_args, .. }, ServerSpec::Stdio { command: new, args, .. }) => {
            old == new && old_args == args
        }
        (ServerSpec::Http { url: old, .. }, ServerSpec::Http { url: new, .. }) => {
            matches!((origin(old), origin(new)), (Some(old), Some(new)) if old == new)
        }
        _ => false,
    }
}

/// The names of the variables or headers of `spec` that are secrets ([`Value::Secret`] or [`Value::Stored`]),
/// in order.
pub fn secret_names(spec: &ServerSpec) -> Vec<String> {
    spec.values().iter().filter(|(_, value)| !matches!(value, Value::Plain(_))).map(|(name, _)| name.clone()).collect()
}

/// Puts the secrets of `new` for the server `id` in the keychain and returns `new` in [`stored_form`], for the
/// file. `old` is the spec it replaces (`None` when installing):
///
/// - each [`Value::Secret`] of `new` is stored under `ext:<id>:<NAME>`, replacing what was there;
/// - a [`Value::Stored`] of `new` keeps the secret stored under its name, but only when [`keeps_secrets`]
///   says so; otherwise it is removed and the name is "Missing" until a person gives it again;
/// - a secret name of `old` that is no secret in `new` (gone, or plain now) loses its secret.
///
/// The old secrets go before the new ones are stored; on a keychain error the rest is not done.
pub fn save_secrets(
    secrets: &dyn SecretStore,
    id: &str,
    old: Option<&ServerSpec>,
    new: &ServerSpec,
) -> Result<ServerSpec, StoreError> {
    check_id(id)?;
    if let Some(old) = old {
        let keep = keeps_secrets(old, new);
        for name in secret_names(old) {
            let name = name.as_str();
            let kept = keep && matches!(new.values().get(name), Some(Value::Stored { .. }));
            let replaced = matches!(new.values().get(name), Some(Value::Secret(_)));
            if !kept && !replaced {
                secrets.delete(&secret_key(id, name)).map_err(StoreError::Keychain)?;
            }
        }
    }
    for (name, value) in new.values() {
        if let Value::Secret(secret) = value {
            secrets.set(&secret_key(id, name), secret).map_err(StoreError::Keychain)?;
        }
    }
    Ok(stored_form(new))
}

/// The keychain entries [`save_secrets`] may change when `new` replaces `old` for the server `id`: each secret
/// name of either, with what is stored under it now (`None` for nothing). [`restore_secrets`] puts them back
/// when the change fails after the secrets were saved. Never log or answer with the result.
pub fn snapshot_secrets(
    secrets: &dyn SecretStore,
    id: &str,
    old: &ServerSpec,
    new: &ServerSpec,
) -> Result<BTreeMap<String, Option<Secret>>, StoreError> {
    check_id(id)?;
    let names: BTreeSet<String> = secret_names(old).into_iter().chain(secret_names(new)).collect();
    let mut snapshot = BTreeMap::new();
    for name in names {
        let secret = secrets.get(&secret_key(id, &name)).map_err(StoreError::Keychain)?;
        snapshot.insert(name, secret);
    }
    Ok(snapshot)
}

/// Puts the entries of a [`snapshot_secrets`] back for the server `id`: a secret where there was one, none
/// where there was none. Every entry is tried, also after one fails; the first error is the answer.
pub fn restore_secrets(
    secrets: &dyn SecretStore,
    id: &str,
    snapshot: &BTreeMap<String, Option<Secret>>,
) -> Result<(), StoreError> {
    check_id(id)?;
    let mut first = Ok(());
    for (name, secret) in snapshot {
        let key = secret_key(id, name);
        let done = match secret {
            Some(secret) => secrets.set(&key, secret),
            None => secrets.delete(&key),
        };
        if let Err(err) = done {
            first = first.and(Err(StoreError::Keychain(err)));
        }
    }
    first
}

/// The stored secrets of the server `id` for the secret names of `spec` ([`secret_names`]), by name: what
/// `super::env::child_env` and `super::env::headers` take. A name without a stored secret is left out (those
/// functions then fail with a missing secret). Never log or answer with the result.
pub fn read_secrets(
    secrets: &dyn SecretStore,
    id: &str,
    spec: &ServerSpec,
) -> Result<BTreeMap<String, Secret>, StoreError> {
    check_id(id)?;
    let mut found = BTreeMap::new();
    for name in secret_names(spec) {
        if let Some(secret) = secrets.get(&secret_key(id, &name)).map_err(StoreError::Keychain)? {
            found.insert(name, secret);
        }
    }
    Ok(found)
}

/// `spec` as people see it ([`torro_ipc::extensions::ExtensionInfo::server`]) with whether each secret is in
/// the keychain now (`Value::Stored { secret }`), and the same as the list of
/// [`torro_ipc::extensions::ExtensionInfo::secrets`]. Never a value.
pub fn secret_state(
    secrets: &dyn SecretStore,
    id: &str,
    spec: &ServerSpec,
) -> Result<(ServerSpec, Vec<SecretInfo>), StoreError> {
    check_id(id)?;
    let mut infos = Vec::new();
    for name in secret_names(spec) {
        let stored = secrets.get(&secret_key(id, &name)).map_err(StoreError::Keychain)?.is_some();
        infos.push(SecretInfo { name, stored });
    }
    let stored: BTreeSet<&str> = infos.iter().filter(|info| info.stored).map(|info| info.name.as_str()).collect();
    let shown = map_values(spec, |name, value| match value {
        Value::Plain(text) => Value::Plain(text.clone()),
        Value::Secret(_) | Value::Stored { .. } => Value::Stored { secret: stored.contains(name) },
    });
    Ok((shown, infos))
}

/// Removes every keychain entry of the server `id` that a name of `spec` can have, secret or not (a name may
/// have been one before): when the server is removed (`extensions.remove_extension`). A missing entry is fine.
pub fn forget_secrets(secrets: &dyn SecretStore, id: &str, spec: &ServerSpec) -> Result<(), StoreError> {
    check_id(id)?;
    for name in spec.values().keys() {
        secrets.delete(&secret_key(id, name)).map_err(StoreError::Keychain)?;
    }
    Ok(())
}

fn check_id(id: &str) -> Result<(), StoreError> {
    match valid_id(id) {
        true => Ok(()),
        false => Err(StoreError::InvalidId(id.to_string())),
    }
}

/// `spec` with each variable or header value replaced by `f` of its name and value.
fn map_values(spec: &ServerSpec, f: impl Fn(&str, &Value) -> Value) -> ServerSpec {
    let map =
        |values: &BTreeMap<String, Value>| values.iter().map(|(name, value)| (name.clone(), f(name, value))).collect();
    match spec {
        ServerSpec::Stdio { command, args, env } => {
            ServerSpec::Stdio { command: command.clone(), args: args.clone(), env: map(env) }
        }
        ServerSpec::Http { url, headers } => ServerSpec::Http { url: url.clone(), headers: map(headers) },
    }
}

/// Scheme, host and port of `url` (`https://host:port`, the scheme's own port left out), lowercased; `None`
/// when it is no URL with a host.
pub fn origin(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    parsed.host_str()?;
    Some(parsed.origin().ascii_serialization())
}

/// The origin of `url`, then `/…` when it had a path, query or fragment; [`REDACTED`] for no URL.
fn redacted_url(url: &str) -> String {
    let Some(origin) = origin(url) else { return REDACTED.to_string() };
    let more = reqwest::Url::parse(url)
        .map(|parsed| parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some())
        .unwrap_or(true);
    match more {
        true => format!("{origin}/{REDACTED}"),
        false => origin,
    }
}

/// A [`SecretStore`] in memory, for the tests of the core.
#[cfg(test)]
pub(crate) mod testing {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex, PoisonError};

    use torro_ipc::Secret;

    use super::SecretStore;

    /// Keys and values in a shared map; clones see the same map.
    #[derive(Clone, Default)]
    pub(crate) struct MemorySecrets(pub(crate) Arc<Mutex<BTreeMap<String, String>>>);

    impl MemorySecrets {
        /// The keys stored now, in order.
        pub(crate) fn keys(&self) -> Vec<String> {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).keys().cloned().collect()
        }
    }

    impl SecretStore for MemorySecrets {
        fn get(&self, key: &str) -> Result<Option<Secret>, String> {
            Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned().map(Secret::new))
        }

        fn set(&self, key: &str, secret: &Secret) -> Result<(), String> {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(key.to_string(), secret.expose().to_string());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), String> {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::MemorySecrets;
    use super::*;

    fn plain(text: &str) -> Value {
        Value::Plain(text.into())
    }

    fn secret(text: &str) -> Value {
        Value::Secret(Secret::new(text))
    }

    fn github() -> ServerSpec {
        ServerSpec::Stdio {
            command: "docker".into(),
            args: vec!["run".into(), "-i".into(), "--rm".into(), "ghcr.io/github/github-mcp-server:v0.20.2".into()],
            env: BTreeMap::from([
                ("GITHUB_PERSONAL_ACCESS_TOKEN".into(), secret("ghp_topsecret")),
                ("GITHUB_TOOLSETS".into(), plain("repos,issues")),
            ]),
        }
    }

    fn remote(url: &str, token: Value) -> ServerSpec {
        ServerSpec::Http {
            url: url.into(),
            headers: BTreeMap::from([("Authorization".into(), token), ("X-Team".into(), plain("torro"))]),
        }
    }

    fn extension(id: &str, server: ServerSpec) -> StoredExtension {
        StoredExtension {
            id: id.into(),
            name: "GitHub".into(),
            source: Source::Catalog { entry: "github".into(), version: "0.20.2".into() },
            server,
            enabled: true,
            allowed: vec![AllowedTool {
                name: "get_me".into(),
                description: "Who you are".into(),
                input_schema: serde_json::json!({"type": "object"}),
                access: Access::Read,
                fingerprint: "ab".repeat(32),
            }],
            reported: vec![ReportedTool {
                name: "create_issue".into(),
                description: "Opens an issue".into(),
                input_schema: serde_json::json!({"type": "object", "properties": {"title": {"type": "string"}}}),
            }],
            folder: None,
        }
    }

    #[test]
    fn the_file_goes_there_and_back_without_a_secret_value() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::in_data(dir.path());
        assert_eq!(store.load().expect("no file yet"), Vec::new());

        let written = vec![
            StoredExtension { folder: Some("/Users/anna/Documents".into()), ..extension("github", github()) },
            StoredExtension {
                source: Source::Custom,
                allowed: Vec::new(),
                reported: Vec::new(),
                enabled: false,
                ..extension("team", remote("https://mcp.example.com/mcp", secret("Bearer tok_hidden")))
            },
        ];
        store.save(&written).expect("saved");

        let text = std::fs::read_to_string(store.file()).expect("file");
        for value in ["ghp_topsecret", "tok_hidden"] {
            assert!(!text.contains(value), "no secret value on disk: {text}");
        }
        assert!(text.contains("GITHUB_PERSONAL_ACCESS_TOKEN"), "the name stays");
        let expected: Vec<_> =
            written.iter().map(|e| StoredExtension { server: stored_form(&e.server), ..e.clone() }).collect();
        assert_eq!(store.load().expect("read"), expected);
        let ServerSpec::Stdio { env, .. } = &expected[0].server else { panic!("stdio") };
        assert_eq!(env["GITHUB_PERSONAL_ACCESS_TOKEN"], Value::Stored { secret: true });
        assert_eq!(env["GITHUB_TOOLSETS"], plain("repos,issues"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode(store.dir()), 0o700);
            assert_eq!(mode(&store.file()), 0o600);
        }
    }

    #[test]
    fn a_file_of_this_version_reads() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::new(dir.path().to_path_buf());
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extensions/extensions.json");
        std::fs::copy(fixture, store.file()).expect("copied");
        let read = store.load().expect("reads");
        assert_eq!(read.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["time", "team"]);
        assert_eq!(read[0].allowed[0].access, Access::Read);
        assert_eq!(read[1].server, remote("http://127.0.0.1:8808/mcp", Value::Stored { secret: true }));
        // Written before issue #303: no folder chosen.
        assert!(read.iter().all(|e| e.folder.is_none()));
    }

    #[test]
    fn a_secret_written_by_hand_is_never_taken_as_a_value() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::new(dir.path().to_path_buf());
        let line = r#"{"version":1,"extensions":[{"id":"x","name":"X","source":{"kind":"custom"},
            "server":{"transport":"stdio","command":"x","env":{"TOKEN":{"secret":"typed in"}}},"enabled":false}]}"#;
        std::fs::write(store.file(), line).expect("written");
        let read = store.load().expect("reads");
        assert_eq!(read[0].server.values()["TOKEN"], Value::Stored { secret: true });
    }

    #[test]
    fn a_damaged_file_or_bad_ids_are_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::new(dir.path().to_path_buf());
        for text in [
            "{not json",
            r#"{"version":2,"extensions":[]}"#,
            r#"{"version":1,"extensions":[{"id":"Bad_Id","name":"X","source":{"kind":"custom"},
                "server":{"transport":"stdio","command":"x"},"enabled":false}]}"#,
        ] {
            std::fs::write(store.file(), text).expect("written");
            assert!(matches!(store.load(), Err(StoreError::Damaged { .. })), "{text}");
            assert_eq!(std::fs::read_to_string(store.file()).expect("left alone"), text);
        }
        let twice = [extension("github", github()), extension("github", github())];
        assert!(matches!(store.save(&twice), Err(StoreError::InvalidId(_))));
        assert!(matches!(store.save(&[extension("../up", github())]), Err(StoreError::InvalidId(_))));
    }

    #[test]
    fn a_server_has_a_private_working_folder_that_goes_with_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::in_data(dir.path());
        let path = store.work_path("time").expect("a path");
        assert!(!path.exists(), "only the path, nothing made");
        let work = store.work_dir("time").expect("made");
        assert_eq!(work, dir.path().join("extensions/time/work"));
        assert_eq!(work, path);
        assert!(work.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&work).expect("metadata").permissions().mode() & 0o777, 0o700);
        }
        std::fs::write(work.join("cache"), "x").expect("written");
        store.remove_files("time").expect("removed");
        assert!(!dir.path().join("extensions/time").exists());
        store.remove_files("time").expect("removing again is fine");
        assert!(matches!(store.work_dir(".."), Err(StoreError::InvalidId(_))));
        assert!(matches!(store.work_path(".."), Err(StoreError::InvalidId(_))));
        assert!(matches!(store.remove_files(""), Err(StoreError::InvalidId(_))));
    }

    #[test]
    fn secrets_go_to_the_keychain_under_the_server_and_name() {
        let secrets = MemorySecrets::default();
        let stored = save_secrets(&secrets, "github", None, &github()).expect("stored");
        assert_eq!(stored, stored_form(&github()));
        assert_eq!(secrets.keys(), ["ext:github:GITHUB_PERSONAL_ACCESS_TOKEN"]);

        let read = read_secrets(&secrets, "github", &stored).expect("read");
        assert_eq!(read, BTreeMap::from([("GITHUB_PERSONAL_ACCESS_TOKEN".to_string(), Secret::new("ghp_topsecret"))]));

        let (shown, infos) = secret_state(&secrets, "github", &stored).expect("state");
        assert_eq!(infos, [SecretInfo { name: "GITHUB_PERSONAL_ACCESS_TOKEN".into(), stored: true }]);
        assert_eq!(shown.values()["GITHUB_PERSONAL_ACCESS_TOKEN"], Value::Stored { secret: true });
        assert!(!format!("{shown:?}{infos:?}").contains("ghp_topsecret"));

        forget_secrets(&secrets, "github", &stored).expect("forgotten");
        assert_eq!(secrets.keys(), Vec::<String>::new());
        let (shown, infos) = secret_state(&secrets, "github", &stored).expect("state");
        assert_eq!(infos, [SecretInfo { name: "GITHUB_PERSONAL_ACCESS_TOKEN".into(), stored: false }]);
        assert_eq!(shown.values()["GITHUB_PERSONAL_ACCESS_TOKEN"], Value::Stored { secret: false });
        assert_eq!(read_secrets(&secrets, "github", &stored).expect("read"), BTreeMap::new());
        assert!(matches!(save_secrets(&secrets, "Not:Valid", None, &github()), Err(StoreError::InvalidId(_))));
    }

    #[test]
    fn an_update_keeps_secrets_only_for_the_same_command_or_host() {
        let secrets = MemorySecrets::default();
        let old = save_secrets(&secrets, "github", None, &github()).expect("stored");

        // Same command and arguments, the secret given as stored: it stays.
        let same = old.clone();
        save_secrets(&secrets, "github", Some(&old), &same).expect("updated");
        assert_eq!(secrets.keys(), ["ext:github:GITHUB_PERSONAL_ACCESS_TOKEN"]);

        // H3 of the API review: the same command with other arguments (another package or image) is another
        // program, and the token is not handed on.
        let mut other_image = same.clone();
        if let ServerSpec::Stdio { args, .. } = &mut other_image {
            args[3] = "ghcr.io/attacker/server:latest".into();
        }
        assert!(!keeps_secrets(&same, &other_image));
        let npx = |package: &str| ServerSpec::Stdio {
            command: "npx".into(),
            args: vec!["-y".into(), package.into()],
            env: same.values().clone(),
        };
        assert!(!keeps_secrets(&npx("@scope/server@1"), &npx("@evil/server@1")));
        assert!(keeps_secrets(&npx("@scope/server@1"), &npx("@scope/server@1")));
        save_secrets(&secrets, "github", Some(&old), &old).expect("stored again");

        save_secrets(&secrets, "github", Some(&same), &other_image).expect("updated");
        assert_eq!(secrets.keys(), Vec::<String>::new(), "gone with other arguments");
        save_secrets(&secrets, "github", None, &github()).expect("stored again");

        // Another command: the secret is gone, the name waits for a new one.
        let other = ServerSpec::Stdio { command: "/tmp/evil".into(), args: Vec::new(), env: same.values().clone() };
        assert!(!keeps_secrets(&same, &other));
        save_secrets(&secrets, "github", Some(&same), &other).expect("updated");
        assert_eq!(secrets.keys(), Vec::<String>::new());

        // An address: the path may change, the host may not.
        let first = remote("https://mcp.example.com/v1", secret("Bearer one"));
        let stored = save_secrets(&secrets, "team", None, &first).expect("stored");
        let moved = remote("https://mcp.example.com/v2?x=1", Value::Stored { secret: true });
        assert!(keeps_secrets(&stored, &moved));
        save_secrets(&secrets, "team", Some(&stored), &moved).expect("updated");
        assert_eq!(secrets.keys(), ["ext:team:Authorization"]);
        let elsewhere = remote("https://attacker.example.net/v2", Value::Stored { secret: true });
        assert!(!keeps_secrets(&moved, &elsewhere));
        save_secrets(&secrets, "team", Some(&moved), &elsewhere).expect("updated");
        assert_eq!(secrets.keys(), Vec::<String>::new());

        // A secret that becomes plain is removed; a new value replaces the old one.
        save_secrets(&secrets, "team", None, &first).expect("stored");
        let now_plain = remote("https://mcp.example.com/v1", plain("Bearer public"));
        save_secrets(&secrets, "team", Some(&first), &now_plain).expect("updated");
        assert_eq!(secrets.keys(), Vec::<String>::new());
        save_secrets(&secrets, "team", Some(&now_plain), &first).expect("stored again");
        let replaced = remote("https://other.example.com/", secret("Bearer two"));
        save_secrets(&secrets, "team", Some(&first), &replaced).expect("replaced");
        assert_eq!(secrets.get("ext:team:Authorization").expect("get"), Some(Secret::new("Bearer two")));
    }

    /// Code review of #302: what an update saved goes back when it fails afterwards, a replaced secret as
    /// well as a removed one and one that was not there before.
    #[test]
    fn a_snapshot_puts_the_secrets_back_as_they_were() {
        let secrets = MemorySecrets::default();
        let old = save_secrets(&secrets, "github", None, &github()).expect("stored");
        let mut new = remote("https://mcp.example.com/", secret("Bearer new"));
        let snapshot = snapshot_secrets(&secrets, "github", &old, &new).expect("snapshot");
        assert_eq!(snapshot.keys().collect::<Vec<_>>(), ["Authorization", "GITHUB_PERSONAL_ACCESS_TOKEN"]);
        save_secrets(&secrets, "github", Some(&old), &new).expect("updated");
        assert_eq!(secrets.keys(), ["ext:github:Authorization"]);
        restore_secrets(&secrets, "github", &snapshot).expect("restored");
        assert_eq!(secrets.keys(), ["ext:github:GITHUB_PERSONAL_ACCESS_TOKEN"]);
        assert_eq!(
            secrets.get("ext:github:GITHUB_PERSONAL_ACCESS_TOKEN").expect("get"),
            Some(Secret::new("ghp_topsecret"))
        );

        // A secret given anew over a stored one gets its old value back.
        let first = save_secrets(&secrets, "team", None, &new).expect("stored");
        new = remote("https://mcp.example.com/", secret("Bearer newer"));
        let snapshot = snapshot_secrets(&secrets, "team", &first, &new).expect("snapshot");
        save_secrets(&secrets, "team", Some(&first), &new).expect("updated");
        restore_secrets(&secrets, "team", &snapshot).expect("restored");
        assert_eq!(secrets.get("ext:team:Authorization").expect("get"), Some(Secret::new("Bearer new")));
    }

    #[test]
    fn secrets_stay_only_within_one_kind_and_origin() {
        let stdio = ServerSpec::Stdio { command: "npx".into(), args: Vec::new(), env: BTreeMap::new() };
        let http = ServerSpec::Http { url: "https://example.com/mcp".into(), headers: BTreeMap::new() };
        assert!(keeps_secrets(&stdio, &stdio.clone()));
        assert!(!keeps_secrets(&stdio, &http));
        assert!(!keeps_secrets(&http, &stdio));
        let port = ServerSpec::Http { url: "https://example.com:8443/mcp".into(), headers: BTreeMap::new() };
        assert!(!keeps_secrets(&http, &port), "another port is another place");
        let upper = ServerSpec::Http { url: "https://EXAMPLE.com:443/other".into(), headers: BTreeMap::new() };
        assert!(keeps_secrets(&http, &upper), "case and the scheme's own port do not count");
        let broken = ServerSpec::Http { url: "not a url".into(), headers: BTreeMap::new() };
        assert!(!keeps_secrets(&broken, &broken));
    }

    #[test]
    fn the_stored_form_marks_every_secret_and_keeps_the_rest() {
        assert_eq!(
            stored_form(&github()),
            ServerSpec::Stdio {
                command: "docker".into(),
                args: vec!["run".into(), "-i".into(), "--rm".into(), "ghcr.io/github/github-mcp-server:v0.20.2".into()],
                env: BTreeMap::from([
                    ("GITHUB_PERSONAL_ACCESS_TOKEN".into(), Value::Stored { secret: true }),
                    ("GITHUB_TOOLSETS".into(), plain("repos,issues")),
                ]),
            }
        );
        let reported = remote("https://x.example/", Value::Stored { secret: false });
        assert_eq!(stored_form(&reported), remote("https://x.example/", Value::Stored { secret: true }));
        assert_eq!(secret_names(&github()), ["GITHUB_PERSONAL_ACCESS_TOKEN"]);
    }

    #[test]
    fn the_redacted_form_shows_names_but_no_values() {
        let shown = redacted(&github());
        let ServerSpec::Stdio { command, args, env } = &shown else { panic!("stdio") };
        assert_eq!(command, "docker");
        assert_eq!(args, &vec![REDACTED.to_string(); 4]);
        assert_eq!(env["GITHUB_PERSONAL_ACCESS_TOKEN"], Value::Stored { secret: true });
        assert_eq!(env["GITHUB_TOOLSETS"], plain(REDACTED));
        let text = format!("{shown:?}");
        for hidden in ["ghcr.io", "repos,issues", "ghp_topsecret"] {
            assert!(!text.contains(hidden), "{text}");
        }

        let ServerSpec::Http { url, headers } =
            redacted(&remote("https://user@mcp.example.com/mcp?api_key=k123", plain("Bearer visible")))
        else {
            panic!("http")
        };
        assert_eq!(url, format!("https://mcp.example.com/{REDACTED}"));
        assert_eq!(headers["Authorization"], plain(REDACTED));
        assert_eq!(headers["X-Team"], plain(REDACTED));
        let bare = redacted(&remote("http://localhost:8080", plain("x")));
        let program = |command: &str| ServerSpec::Stdio { command: command.into(), args: vec![], env: BTreeMap::new() };
        for (command, shown) in [
            ("/Users/anna/.local/bin/server", format!("{REDACTED}/server")),
            ("C:\\Users\\anna\\server.exe", format!("{REDACTED}/server.exe")),
            ("./server", format!("{REDACTED}/server")),
            ("/opt/servers/", REDACTED.to_string()),
            ("npx", "npx".to_string()),
        ] {
            let ServerSpec::Stdio { command, .. } = redacted(&program(command)) else { panic!("stdio") };
            assert_eq!(command, shown, "no folders of a path");
        }
        assert!(matches!(bare, ServerSpec::Http { url, .. } if url == "http://localhost:8080"));
        assert!(matches!(redacted(&remote("::", plain("x"))), ServerSpec::Http { url, .. } if url == REDACTED));
    }
}
