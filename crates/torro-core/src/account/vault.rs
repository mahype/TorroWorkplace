//! Where the account secrets live in the system keychain, and that it is asked as little as
//! possible (task X.13 in `docs/tasks/plattformen.md`).
//!
//! macOS asks for the keychain password once per entry and reading program unless the person
//! chooses "Always Allow", and the core's services read the same secret each for themselves. So:
//!
//! - **Bundled** (macOS): every account secret sits in one entry, [`VAULT_ENTRY`], as JSON
//!   ([`Contents`]). It is read once per process and then kept in memory; a change is made in
//!   memory and written in one go. An entry from before (one per account, named after its id) is
//!   moved into the vault the first time its secret is asked for, and then deleted. A vault that
//!   cannot be read or understood is never overwritten: every call fails until it can be read.
//! - **One entry per account** (Linux, Windows): the Windows Credential Manager holds at most 2560
//!   bytes per entry, too little for many secrets. Each secret is still read only once per
//!   process.
//!
//! The keychain itself is a [`Keychain`], so the logic is tested with a map instead of the
//! person's keychain.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// The keychain account of the bundled vault, under the suite's keychain service. Reserved: no
/// account id may be this ([`super::is_reserved_id`]).
pub const VAULT_ENTRY: &str = "torro-vault";

/// The version of [`Contents`] this code writes and reads.
const VERSION: u32 = 1;

/// The entries of one keychain service, by keychain account. Errors never contain a secret.
pub trait Keychain: Send {
    /// The value under `account`; `Ok(None)` when there is no such entry.
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    /// Stores `value` under `account`, replacing what was there.
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    /// Removes the entry of `account`; a missing entry is fine.
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// What the bundled vault holds, as written to the keychain.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Contents {
    version: u32,
    secrets: BTreeMap<String, String>,
}

/// The account secrets on top of a [`Keychain`], bundled or one entry per account.
pub struct Vault<K: Keychain> {
    keychain: K,
    bundled: bool,
    /// Bundled: the vault's secrets once read; `None` before (or while it cannot be read).
    secrets: Option<BTreeMap<String, String>>,
    /// Bundled: ids whose entry from before the vault was looked for already in this process.
    looked_up: HashSet<String>,
    /// One entry per account: what this process has read or written, `None` for "no entry".
    cache: HashMap<String, Option<String>>,
}

impl<K: Keychain> Vault<K> {
    /// A vault over `keychain`; `bundled` keeps every secret in [`VAULT_ENTRY`].
    pub fn new(keychain: K, bundled: bool) -> Vault<K> {
        Vault { keychain, bundled, secrets: None, looked_up: HashSet::new(), cache: HashMap::new() }
    }

    /// The secret of account `id`, `None` when there is none.
    pub fn get(&mut self, id: &str) -> Result<Option<String>, String> {
        if !self.bundled {
            if let Some(known) = self.cache.get(id) {
                return Ok(known.clone());
            }
            let read = self.keychain.get(id)?;
            self.cache.insert(id.to_string(), read.clone());
            return Ok(read);
        }
        if let Some(secret) = self.loaded()?.get(id) {
            return Ok(Some(secret.clone()));
        }
        if !self.looked_up.insert(id.to_string()) {
            return Ok(None);
        }
        // An entry from before the vault: move it in, then remove it.
        let Some(old) = self.keychain.get(id)? else { return Ok(None) };
        self.store(id, Some(old.clone()))?;
        self.forget_old(id);
        Ok(Some(old))
    }

    /// Stores `secret` for account `id`.
    pub fn set(&mut self, id: &str, secret: &str) -> Result<(), String> {
        if !self.bundled {
            self.keychain.set(id, secret)?;
            self.cache.insert(id.to_string(), Some(secret.to_string()));
            return Ok(());
        }
        self.store(id, Some(secret.to_string()))?;
        // A stale entry from before must not come back through `get` later.
        self.looked_up.insert(id.to_string());
        self.forget_old(id);
        Ok(())
    }

    /// Removes the secret of account `id`; a missing one is fine.
    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        if !self.bundled {
            self.keychain.delete(id)?;
            self.cache.insert(id.to_string(), None);
            return Ok(());
        }
        self.store(id, None)?;
        self.looked_up.insert(id.to_string());
        self.forget_old(id);
        Ok(())
    }

    /// Removes the entry of `id` from before the vault, if there is one. The secret is already
    /// safe in the vault, so a failure is only reported.
    fn forget_old(&self, id: &str) {
        if let Err(err) = self.keychain.delete(id) {
            eprintln!("torro: the old keychain entry of {id} stays: {err}");
        }
    }

    /// The vault's secrets, read from the keychain the first time.
    fn loaded(&mut self) -> Result<&BTreeMap<String, String>, String> {
        let secrets = match self.secrets.take() {
            Some(secrets) => secrets,
            None => match self.keychain.get(VAULT_ENTRY)? {
                None => BTreeMap::new(),
                Some(text) => parse(&text)?,
            },
        };
        Ok(self.secrets.insert(secrets))
    }

    /// Sets (`Some`) or removes (`None`) the secret of `id` and writes the vault. When the write
    /// fails, the vault in memory stays as it was.
    fn store(&mut self, id: &str, secret: Option<String>) -> Result<(), String> {
        let mut secrets = self.loaded()?.clone();
        let changed = match secret {
            Some(secret) => secrets.insert(id.to_string(), secret.clone()).as_ref() != Some(&secret),
            None => secrets.remove(id).is_some(),
        };
        if changed {
            let text = serde_json::to_string(&Contents { version: VERSION, secrets: secrets.clone() })
                .map_err(|_| "the vault could not be written".to_string())?;
            self.keychain.set(VAULT_ENTRY, &text)?;
        }
        self.secrets = Some(secrets);
        Ok(())
    }
}

/// The secrets of a vault as stored; an error (never with the contents) when it is not ours.
fn parse(text: &str) -> Result<BTreeMap<String, String>, String> {
    let contents: Contents =
        serde_json::from_str(text).map_err(|_| "the keychain vault is damaged; it is left as it is".to_string())?;
    if contents.version != VERSION {
        return Err(format!("the keychain vault has version {}; this version reads {VERSION}", contents.version));
    }
    Ok(contents.secrets)
}

/// The system keychain under one service: Keychain on macOS, Secret Service on Linux, Credential
/// Manager on Windows.
pub struct SystemKeychain {
    pub service: &'static str,
}

impl SystemKeychain {
    fn entry(&self, account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(self.service, account).map_err(|err| err.to_string())
    }
}

impl Keychain for SystemKeychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match self.entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err.to_string()),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.entry(account)?.set_password(value).map_err(|err| err.to_string())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A keychain in memory that counts reads and can be told to refuse writes.
    #[derive(Clone, Default)]
    struct Memory {
        entries: Arc<Mutex<BTreeMap<String, String>>>,
        reads: Arc<Mutex<Vec<String>>>,
        refuse_writes: Arc<Mutex<bool>>,
    }

    impl Memory {
        fn with(entries: &[(&str, &str)]) -> Memory {
            let memory = Memory::default();
            for (account, value) in entries {
                memory.lock().insert(account.to_string(), value.to_string());
            }
            memory
        }
        fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
            self.entries.lock().expect("entries")
        }
        fn reads(&self) -> Vec<String> {
            self.reads.lock().expect("reads").clone()
        }
        fn vault(&self) -> BTreeMap<String, String> {
            parse(self.lock().get(VAULT_ENTRY).expect("a vault")).expect("readable")
        }
    }

    impl Keychain for Memory {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            self.reads.lock().expect("reads").push(account.to_string());
            Ok(self.lock().get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> Result<(), String> {
            if *self.refuse_writes.lock().expect("flag") {
                return Err("refused".into());
            }
            self.lock().insert(account.to_string(), value.to_string());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            self.lock().remove(account);
            Ok(())
        }
    }

    #[test]
    fn many_accounts_cost_one_read() {
        let keychain = Memory::default();
        let mut vault = Vault::new(keychain.clone(), true);
        vault.set("nextcloud:anna@cloud", "app-password").expect("set");
        vault.set("github:anna", "token").expect("set");
        let mut fresh = Vault::new(keychain.clone(), true);
        keychain.reads.lock().expect("reads").clear();
        for _ in 0..5 {
            assert_eq!(fresh.get("nextcloud:anna@cloud").expect("get").as_deref(), Some("app-password"));
            assert_eq!(fresh.get("github:anna").expect("get").as_deref(), Some("token"));
        }
        assert_eq!(keychain.reads(), [VAULT_ENTRY], "one read of the vault, nothing else");
        assert_eq!(keychain.lock().len(), 1, "only the vault entry");
    }

    #[test]
    fn entries_from_before_move_into_the_vault() {
        let keychain = Memory::with(&[("nextcloud:anna@cloud", "old-password")]);
        let mut vault = Vault::new(keychain.clone(), true);
        assert_eq!(vault.get("nextcloud:anna@cloud").expect("get").as_deref(), Some("old-password"));
        assert_eq!(keychain.vault().get("nextcloud:anna@cloud").map(String::as_str), Some("old-password"));
        assert!(!keychain.lock().contains_key("nextcloud:anna@cloud"), "the old entry is gone");
        // A missing secret is looked for in the old place once per process.
        assert_eq!(vault.get("github:nobody").expect("get"), None);
        assert_eq!(vault.get("github:nobody").expect("get"), None);
        assert_eq!(keychain.reads().iter().filter(|r| *r == "github:nobody").count(), 1);
    }

    #[test]
    fn set_and_delete_write_the_vault_and_clear_old_entries() {
        let keychain = Memory::with(&[("caldav:anna@dav", "stale")]);
        let mut vault = Vault::new(keychain.clone(), true);
        vault.set("caldav:anna@dav", "new").expect("set");
        assert!(!keychain.lock().contains_key("caldav:anna@dav"), "the stale entry cannot come back");
        assert_eq!(vault.get("caldav:anna@dav").expect("get").as_deref(), Some("new"));
        vault.delete("caldav:anna@dav").expect("delete");
        assert_eq!(vault.get("caldav:anna@dav").expect("get"), None);
        assert!(keychain.vault().is_empty());
        vault.delete("never:there").expect("deleting nothing is fine");
    }

    #[test]
    fn a_failed_write_leaves_the_vault_as_it_was() {
        let keychain = Memory::default();
        let mut vault = Vault::new(keychain.clone(), true);
        vault.set("github:anna", "token").expect("set");
        *keychain.refuse_writes.lock().expect("flag") = true;
        assert!(vault.set("github:anna", "other").is_err());
        assert!(vault.delete("github:anna").is_err());
        assert_eq!(vault.get("github:anna").expect("get").as_deref(), Some("token"));
        assert_eq!(keychain.vault().get("github:anna").map(String::as_str), Some("token"));
    }

    #[test]
    fn a_vault_it_cannot_read_is_never_overwritten() {
        for text in ["not json", r#"{"version": 2, "secrets": {}}"#] {
            let keychain = Memory::with(&[(VAULT_ENTRY, text)]);
            let mut vault = Vault::new(keychain.clone(), true);
            let err = vault.get("github:anna").expect_err("unreadable");
            assert!(!err.contains("not json"), "errors never show the contents: {err}");
            assert!(vault.set("github:anna", "token").is_err());
            assert!(vault.delete("github:anna").is_err());
            assert_eq!(keychain.lock().get(VAULT_ENTRY).map(String::as_str), Some(text));
        }
    }

    #[test]
    fn one_entry_per_account_is_read_once_per_process() {
        let keychain = Memory::with(&[("github:anna", "token")]);
        let mut vault = Vault::new(keychain.clone(), false);
        for _ in 0..3 {
            assert_eq!(vault.get("github:anna").expect("get").as_deref(), Some("token"));
            assert_eq!(vault.get("github:nobody").expect("get"), None);
        }
        assert_eq!(keychain.reads(), ["github:anna", "github:nobody"]);
        vault.set("github:anna", "new").expect("set");
        assert_eq!(keychain.lock().get("github:anna").map(String::as_str), Some("new"));
        assert_eq!(vault.get("github:anna").expect("get").as_deref(), Some("new"));
        vault.delete("github:anna").expect("delete");
        assert_eq!(vault.get("github:anna").expect("get"), None);
        assert!(!keychain.lock().contains_key(VAULT_ENTRY), "no vault entry in this mode");
    }
}
