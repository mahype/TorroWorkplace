//! Accounts the suite signs in to.
//!
//! An account is set up once and serves every app. Where and as whom lives in
//! `accounts.json` in the core's config directory; the secret (for Nextcloud the app
//! password from Login Flow v2, for GitHub the token) lives in the system keychain: Secret
//! Service on Linux, Keychain on macOS, Credential Manager on Windows.
//!
//! A secret on its way there travels as a [`Secret`], which never shows its value in `Debug`.

use std::borrow::Cow;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

use crate::AppId;
use crate::paths::AppPaths;

/// Keychain service name all suite secrets are filed under.
const KEYCHAIN_SERVICE: &str = "torroworkplace";

/// The file name of the account list in the core's config directory.
pub const ACCOUNTS_FILE: &str = "accounts.json";

/// The keychain entries of client keys start with this (`client-<id>`, kept by the core next to
/// the account secrets). No account id may start with it, or an account could overwrite, read out
/// or delete a client's key.
pub const RESERVED_ID_PREFIX: &str = "client-";

/// `id` is reserved for client keys ([`RESERVED_ID_PREFIX`]) and never names an account.
pub fn is_reserved_id(id: &str) -> bool {
    id.starts_with(RESERVED_ID_PREFIX)
}

/// What kind of server an account talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    /// Nextcloud: CalDAV for calendars and tasks, more to come.
    Nextcloud,
    /// GitHub: issues and pull requests (task G.1). Written `github`.
    #[serde(rename = "github")]
    GitHub,
}

/// Where GitHub accounts live: the `server` of every [`Account::github`].
pub const GITHUB_SERVER: &str = "https://github.com";

/// A secret on its way to the keychain or to its server: a password, an app password, a token.
///
/// `Debug` shows `Secret(…)`, never the value, so a secret cannot slip into a log, an error
/// message or an answer by accident. It serializes as the plain string (a request has to carry
/// it), and its JSON Schema is a `string` marked `writeOnly`. Comparing looks at every byte,
/// whatever differs first. Meant for every secret a request carries (GitHub tokens since G.1;
/// the CalDAV password of task M7 can take it over).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(secret: impl Into<String>) -> Secret {
        Secret(secret.into())
    }

    /// The secret itself, to store it or to send it to its server. Never print it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Nothing but white space, so nothing to store.
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(…)")
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Secret) -> bool {
        let (a, b) = (self.0.as_bytes(), other.0.as_bytes());
        let difference = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
        a.len() == b.len() && difference == 0
    }
}

impl Eq for Secret {}

impl JsonSchema for Secret {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Secret".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        let mut schema = Schema::default();
        schema.insert("type".into(), "string".into());
        // Clients send it; the core never sends it back.
        schema.insert("writeOnly".into(), true.into());
        schema
    }
}

/// One account, without its secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Stable id, also the keychain entry name, e.g. `nextcloud:anna@cloud.example.org`.
    pub id: String,
    pub kind: AccountKind,
    /// Base URL of the server, e.g. `https://cloud.example.org`.
    pub server: String,
    /// The name the server knows the user by (Nextcloud: `loginName`).
    pub login_name: String,
}

impl Account {
    pub fn nextcloud(server: &str, login_name: &str) -> Account {
        let host = server.split("://").nth(1).unwrap_or(server).trim_end_matches('/');
        Account {
            id: format!("nextcloud:{login_name}@{host}"),
            kind: AccountKind::Nextcloud,
            server: server.trim_end_matches('/').to_string(),
            login_name: login_name.to_string(),
        }
    }

    /// The GitHub account of `login` (task G.1): id `github:<login>`, server [`GITHUB_SERVER`].
    /// The caller checks `login` (GitHub's own answer to the token); it never holds a `/`.
    pub fn github(login: &str) -> Account {
        Account {
            id: format!("github:{login}"),
            kind: AccountKind::GitHub,
            server: GITHUB_SERVER.to_string(),
            login_name: login.to_string(),
        }
    }

    /// The secret from the keychain, `None` when there is none.
    pub fn secret(&self) -> Result<Option<String>, AccountError> {
        match self.keychain_entry()?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(AccountError::Keychain(err.to_string())),
        }
    }

    pub fn set_secret(&self, secret: &str) -> Result<(), AccountError> {
        self.keychain_entry()?.set_password(secret).map_err(|err| AccountError::Keychain(err.to_string()))
    }

    pub fn delete_secret(&self) -> Result<(), AccountError> {
        match self.keychain_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(AccountError::Keychain(err.to_string())),
        }
    }

    /// The keychain entry of this account; never one of a client key ([`is_reserved_id`]).
    fn keychain_entry(&self) -> Result<keyring::Entry, AccountError> {
        if is_reserved_id(&self.id) {
            return Err(AccountError::ReservedId(self.id.clone()));
        }
        keyring::Entry::new(KEYCHAIN_SERVICE, &self.id).map_err(|err| AccountError::Keychain(err.to_string()))
    }
}

/// Why reading or writing accounts failed.
#[derive(Debug)]
pub enum AccountError {
    /// No home directory, so no config directory.
    NoHome,
    Io(io::Error),
    /// `accounts.json` is not what we wrote.
    Format(serde_json::Error),
    /// The system keychain refused or is not running.
    Keychain(String),
    /// The account id starts with [`RESERVED_ID_PREFIX`], which belongs to client keys; such an
    /// account gets no keychain entry.
    ReservedId(String),
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountError::NoHome => f.write_str("no home directory"),
            AccountError::Io(err) => write!(f, "accounts file: {err}"),
            AccountError::Format(err) => write!(f, "accounts file is damaged: {err}"),
            AccountError::Keychain(err) => write!(f, "keychain: {err}"),
            AccountError::ReservedId(id) => write!(f, "account id {id:?} is reserved for client keys"),
        }
    }
}

impl std::error::Error for AccountError {}

/// All accounts; an empty list when none are set up yet.
pub fn load() -> Result<Vec<Account>, AccountError> {
    load_from(&accounts_file()?)
}

/// All accounts in the account list at `path` (an [`ACCOUNTS_FILE`]); an empty list when the
/// file does not exist. For callers that are handed their config directory, such as the core's
/// tests, which must never read the user's file.
///
/// An account whose id is reserved for client keys ([`is_reserved_id`]) is left out (and named
/// on stderr): the core never writes one, so it was put there by hand or by another program.
pub fn load_from(path: &Path) -> Result<Vec<Account>, AccountError> {
    let mut accounts: Vec<Account> = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(AccountError::Format)?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(AccountError::Io(err)),
    };
    accounts.retain(|account| {
        let reserved = is_reserved_id(&account.id);
        if reserved {
            eprintln!(
                "torro: ignoring account {:?} in {}: the id is reserved for client keys",
                account.id,
                path.display()
            );
        }
        !reserved
    });
    Ok(accounts)
}

/// Replaces the stored accounts. Secrets are not touched.
pub fn save(accounts: &[Account]) -> Result<(), AccountError> {
    save_to(&accounts_file()?, accounts)
}

/// Replaces the account list at `path` (an [`ACCOUNTS_FILE`]) atomically; secrets are not
/// touched. For callers that are handed their config directory, like [`load_from`].
pub fn save_to(path: &Path, accounts: &[Account]) -> Result<(), AccountError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(AccountError::Io)?;
    }
    let json = serde_json::to_vec_pretty(accounts).map_err(AccountError::Format)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(AccountError::Io)?;
    fs::rename(&tmp, path).map_err(AccountError::Io)
}

/// Where this user's account list lives: [`ACCOUNTS_FILE`] in the core's config directory.
pub fn accounts_file() -> Result<PathBuf, AccountError> {
    AppPaths::for_app(AppId::WORKPLACE).map(|p| p.config.join(ACCOUNTS_FILE)).ok_or(AccountError::NoHome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nextcloud_id_names_user_and_host() {
        let account = Account::nextcloud("https://cloud.example.org/", "anna");
        assert_eq!(account.id, "nextcloud:anna@cloud.example.org");
        assert_eq!(account.server, "https://cloud.example.org");
    }

    #[test]
    fn a_missing_list_is_empty_and_a_written_one_is_read() {
        // A file of our own in the system's temp directory; never the user's list.
        let path = std::env::temp_dir().join(format!("torro-core-{}-{ACCOUNTS_FILE}", std::process::id()));
        let _ = fs::remove_file(&path);
        assert!(load_from(&path).expect("no file is no accounts").is_empty());
        let accounts = vec![Account::nextcloud("https://cloud.example.org", "anna")];
        fs::write(&path, serde_json::to_vec(&accounts).expect("json")).expect("written");
        let read = load_from(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(read.expect("read"), accounts);
    }

    #[test]
    fn github_accounts_are_named_after_the_login_and_written_github() {
        let account = Account::github("mahype");
        assert_eq!((account.id.as_str(), account.server.as_str()), ("github:mahype", GITHUB_SERVER));
        assert_eq!(account.kind, AccountKind::GitHub);
        let json = serde_json::to_value(&account).expect("json");
        assert_eq!(json["kind"], "github");
        assert_eq!(serde_json::from_value::<Account>(json).expect("read back"), account);
    }

    #[test]
    fn a_list_is_saved_to_the_given_file_and_read_back() {
        // A directory of our own in the system's temp directory; never the user's list.
        let dir = std::env::temp_dir().join(format!("torro-core-save-{}", std::process::id()));
        let path = dir.join("config").join(ACCOUNTS_FILE);
        let accounts = vec![Account::nextcloud("https://cloud.example.org", "anna"), Account::github("anna")];
        let saved = save_to(&path, &accounts);
        let read = load_from(&path);
        let _ = fs::remove_dir_all(&dir);
        saved.expect("saved");
        assert_eq!(read.expect("read"), accounts);
    }

    #[test]
    fn account_ids_of_client_keys_are_refused_on_load_and_in_the_keychain() {
        // A file of our own in the system's temp directory; never the user's list.
        let path = std::env::temp_dir().join(format!("torro-core-reserved-{}-{ACCOUNTS_FILE}", std::process::id()));
        let anna = Account::nextcloud("https://cloud.example.org", "anna");
        let intruder = Account { id: "client-a1b2c3d4".into(), ..anna.clone() };
        fs::write(&path, serde_json::to_vec(&[intruder.clone(), anna.clone()]).expect("json")).expect("written");
        let read = load_from(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(read.expect("read"), [anna]);

        // Refused before the keychain is asked, so no test touches the user's keychain.
        assert!(matches!(intruder.secret(), Err(AccountError::ReservedId(_))));
        assert!(matches!(intruder.set_secret("x"), Err(AccountError::ReservedId(_))));
        assert!(matches!(intruder.delete_secret(), Err(AccountError::ReservedId(_))));
        assert!(is_reserved_id("client-") && !is_reserved_id("nextcloud:client-x@cloud"));
    }

    #[test]
    fn secrets_hide_in_debug_compare_whole_and_serialize_plain() {
        let secret = Secret::new("ghp_secret");
        assert_eq!(format!("{secret:?}"), "Secret(…)");
        assert_eq!(serde_json::to_value(&secret).expect("json"), "ghp_secret");
        assert_eq!(serde_json::from_str::<Secret>("\"ghp_secret\"").expect("parses"), secret);
        assert_ne!(secret, Secret::new("ghp_secreT"));
        assert_ne!(secret, Secret::new("ghp_secret2"));
        assert!(Secret::new(" \t").is_blank() && !secret.is_blank());
        let schema = schemars::schema_for!(Secret);
        assert_eq!(schema.get("type"), Some(&"string".into()));
        assert_eq!(schema.get("writeOnly"), Some(&true.into()));
    }
}
