//! Accounts the suite signs in to.
//!
//! An account is set up once and serves every app. Where and as whom lives in
//! `accounts.json` in the core's config directory; the secret (for Nextcloud the app
//! password from Login Flow v2) lives in the system keychain: Secret Service on Linux,
//! Keychain on macOS, Credential Manager on Windows.

use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::AppId;
use crate::paths::AppPaths;

/// Keychain service name all suite secrets are filed under.
const KEYCHAIN_SERVICE: &str = "torroworkplace";

/// What kind of server an account talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    /// Nextcloud: CalDAV for calendars and tasks, more to come.
    Nextcloud,
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

    fn keychain_entry(&self) -> Result<keyring::Entry, AccountError> {
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
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountError::NoHome => f.write_str("no home directory"),
            AccountError::Io(err) => write!(f, "accounts file: {err}"),
            AccountError::Format(err) => write!(f, "accounts file is damaged: {err}"),
            AccountError::Keychain(err) => write!(f, "keychain: {err}"),
        }
    }
}

impl std::error::Error for AccountError {}

/// All accounts; an empty list when none are set up yet.
pub fn load() -> Result<Vec<Account>, AccountError> {
    match fs::read(accounts_file()?) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(AccountError::Format),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(AccountError::Io(err)),
    }
}

/// Replaces the stored accounts. Secrets are not touched.
pub fn save(accounts: &[Account]) -> Result<(), AccountError> {
    let file = accounts_file()?;
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(AccountError::Io)?;
    }
    let json = serde_json::to_vec_pretty(accounts).map_err(AccountError::Format)?;
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(AccountError::Io)?;
    fs::rename(&tmp, &file).map_err(AccountError::Io)
}

fn accounts_file() -> Result<PathBuf, AccountError> {
    AppPaths::for_app(AppId::WORKPLACE).map(|p| p.config.join("accounts.json")).ok_or(AccountError::NoHome)
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
}
