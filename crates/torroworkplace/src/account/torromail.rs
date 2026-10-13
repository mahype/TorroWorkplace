//! The accounts of the separate TorroMail app (task M7.4 in `docs/tasks/mail.md`), read only.
//!
//! TorroMail keeps running beside the suite (`docs/mail.md`, decision 3); taking one of its
//! accounts over is a shortcut when setting up, never a move. So this module only reads: the
//! accounts from TorroMail's `state.json` ([`state_file`], [`parse`]) and, for the one account a
//! person chose to take over, its password from TorroMail's keychain entry (service `TorroMail`,
//! account = TorroMail's id, [`ForeignSecrets`]). Nothing of TorroMail is written or deleted.
//!
//! Listing never touches the keychain; only `accounts.import_torromail_account` does, which only
//! the app key may send. The id it reads comes from `state.json`, never from the request alone
//! ([`TorroMail::password`] takes a listed account), so no request can name another keychain
//! entry. Accounts that sign in with OAuth or use JMAP are listed with the reason they cannot be
//! taken over ([`Stored::unsupported`]).
//!
//! Tests bring their own `state.json` and a keychain in memory ([`TorroMail::new`]); the core of
//! a test reads no TorroMail at all ([`TorroMail::none`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use torro_core::account::Secret;
use torro_core::i18n::tr;
use torro_ipc::TorroMailAccount;
use torro_mail::{Endpoint, Security};

/// The service TorroMail files every secret under, in every keychain it uses.
pub const SERVICE: &str = "TorroMail";

/// TorroMail's file with its accounts, in its data directory.
pub const STATE_FILE: &str = "state.json";

/// The systems TorroMail knows a data directory for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    /// Linux and every other Unix: XDG.
    Linux,
}

impl Os {
    /// The system this program runs on.
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Where TorroMail keeps `state.json` on `os`, as TorroMail itself decides it
/// (`torromail-control/src/paths.rs`): macOS `~/Library/Application Support/TorroMail`, Windows
/// `AppData/Local/TorroMail` in the profile, else `$XDG_STATE_HOME/torromail` (only an absolute
/// one) or `~/.local/state/torromail`. `None` without a home directory.
pub fn state_file(os: Os, home: Option<&Path>, xdg_state_home: Option<&Path>) -> Option<PathBuf> {
    let dir = match os {
        Os::Mac => home?.join("Library/Application Support/TorroMail"),
        Os::Windows => home?.join("AppData/Local/TorroMail"),
        Os::Linux => match xdg_state_home.filter(|path| path.is_absolute()) {
            Some(base) => base.join("torromail"),
            None => home?.join(".local/state/torromail"),
        },
    };
    Some(dir.join(STATE_FILE))
}

/// [`state_file`] for this process: the home from `HOME` (Windows: `USERPROFILE`, as TorroMail
/// reads it) and `XDG_STATE_HOME`.
fn system_state_file() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = if cfg!(windows) { var("USERPROFILE").or_else(|| var("HOME")) } else { var("HOME") };
    state_file(Os::current(), home.as_deref(), var("XDG_STATE_HOME").as_deref())
}

/// One account as TorroMail stored it: what the suite needs to connect it, nothing of
/// TorroMail's own rights, folders or cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// TorroMail's id: its keychain entry is filed under it.
    pub id: String,
    /// The name next to the address (TorroMail's `name`).
    pub name: String,
    pub email: String,
    /// TorroMail's provider label (`IMAP/SMTP`, `Gmail`, `mailbox.org`).
    pub provider: String,
    /// TorroMail signs in to it with OAuth (Google, Microsoft), not with a password.
    pub oauth: bool,
    /// `None` when TorroMail has no host for it.
    pub imap: Option<Endpoint>,
    pub smtp: Option<Endpoint>,
    pub username: String,
}

impl Stored {
    /// Why the suite cannot take the account over, in the person's language; `None` when it can.
    /// OAuth needs client ids the suite does not have yet, JMAP is not spoken (`docs/mail.md`,
    /// "später"), and without both servers and a user name there is nothing to sign in to.
    pub fn unsupported(&self) -> Option<&'static str> {
        if self.oauth {
            Some(tr("Signs in with Google or Microsoft, which Torro Workplace does not support yet."))
        } else if self.provider.to_ascii_lowercase().contains("jmap") {
            Some(tr("Uses JMAP, which Torro Workplace does not support yet."))
        } else if self.imap.is_none()
            || self.smtp.is_none()
            || self.username.trim().is_empty()
            || torro_mail::account_id(&self.email).is_err()
        {
            Some(tr("Not completely set up in TorroMail."))
        } else {
            None
        }
    }

    /// The account as `accounts.list_torromail_accounts` lists it; `connected` when the suite has
    /// a mail account with the address already. Holds nothing secret: there is nothing secret here.
    pub fn listed(&self, connected: bool) -> TorroMailAccount {
        TorroMailAccount {
            id: self.id.clone(),
            address: self.email.clone(),
            sender_name: self.name.clone(),
            provider: self.provider.clone(),
            imap: self.imap.clone(),
            smtp: self.smtp.clone(),
            login_name: self.username.clone(),
            unsupported: self.unsupported().map(str::to_string),
            connected,
        }
    }
}

/// The accounts in the text of a `state.json`, in its order. The file is TorroMail's, not ours:
/// an account without an id that could be a keychain account name, an address or a known login
/// method is left out (never with its content in a log), the others stay. Text that is no JSON
/// object with `accounts` is an error.
pub fn parse(text: &str) -> Result<Vec<Stored>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|_| "TorroMail's state.json is not valid JSON".to_string())?;
    let accounts = value
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or_else(|| "TorroMail's state.json lists no accounts".to_string())?;
    let stored: Vec<Stored> = accounts.iter().filter_map(account).collect();
    if stored.len() < accounts.len() {
        eprintln!("torroworkplace: {} account(s) of TorroMail could not be read", accounts.len() - stored.len());
    }
    Ok(stored)
}

/// One account object of `state.json`, read as TorroMail reads it (`account.rs`, `from_json`):
/// ports default to 993 and 587, a missing security follows from the port.
fn account(value: &Value) -> Option<Stored> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let id = text("id").filter(|id| is_plain_id(id))?;
    let oauth = match text("loginMethod")?.as_str() {
        "password" => false,
        "oauth" => true,
        _ => return None,
    };
    let port = |key: &str, default: u16| match value.get(key) {
        None => Some(default),
        Some(port) => port.as_u64().and_then(|port| u16::try_from(port).ok()).filter(|port| *port > 0),
    };
    let imap_port = port("imapPort", 993)?;
    let smtp_port = port("smtpPort", 587)?;
    let server = |host: &str, port: u16, security: &str, implied_tls: u16| {
        let host = text(host).map(|host| host.trim().to_string()).filter(|host| !host.is_empty())?;
        let security = match text(security).as_deref() {
            Some("tls") => Security::Tls,
            Some("starttls") => Security::StartTls,
            None if port == implied_tls => Security::Tls,
            None => Security::StartTls,
            Some(_) => return None,
        };
        Some(Endpoint { host, port, security })
    };
    Some(Stored {
        name: text("name").unwrap_or_default(),
        email: text("email")?.trim().to_string(),
        provider: text("provider").unwrap_or_default(),
        oauth,
        imap: server("imapHost", imap_port, "imapSecurity", 993),
        smtp: server("smtpHost", smtp_port, "smtpSecurity", 465),
        username: text("username").unwrap_or_default().trim().to_string(),
        id,
    })
}

/// An id as TorroMail makes them for accounts: a UUID (`8-4-4-4-12` hex digits). Only such an id
/// becomes a keychain account name we ask for. TorroMail files other secrets under the same
/// service (`client-key-<id>`, its agents' keys); a `state.json` someone edited must not turn one
/// of those into an "account password" that the core then sends to a server named in the same file.
fn is_plain_id(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Where TorroMail's passwords are read from: its keychain entries in the core
/// ([`SystemForeignSecrets`]), a map in tests. Only reads; errors never hold a secret.
pub trait ForeignSecrets: Send + Sync {
    /// The password of TorroMail's account `id`; `Ok(None)` when TorroMail stored none.
    fn password(&self, id: &str) -> Result<Option<Secret>, String>;
}

/// TorroMail's own keychain entries: the macOS keychain and the Windows Credential Manager through
/// `keyring` (a generic password, service `TorroMail`, account `id`, as TorroMail files it), the
/// Secret Service through `secret-tool` elsewhere, with the attributes TorroMail uses
/// (`service`, `account`). On macOS TorroMail binds its entries to its own signature, so the
/// system may ask the person first or refuse; a refusal is an error the control centre answers
/// with the password field.
pub struct SystemForeignSecrets;

impl ForeignSecrets for SystemForeignSecrets {
    #[cfg(not(any(target_os = "macos", windows)))]
    fn password(&self, id: &str) -> Result<Option<Secret>, String> {
        let output = std::process::Command::new("secret-tool")
            .args(["lookup", "service", SERVICE, "account", id])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|err| format!("secret-tool could not be started: {err}"))?;
        if output.status.success() {
            let text = String::from_utf8(output.stdout).map_err(|_| "the stored password is not text".to_string())?;
            return Ok(Some(Secret::new(text)));
        }
        // `secret-tool` is silent when it merely found nothing.
        match output.stderr.is_empty() {
            true => Ok(None),
            false => Err("the Secret Service could not be asked".to_string()),
        }
    }

    #[cfg(any(target_os = "macos", windows))]
    fn password(&self, id: &str) -> Result<Option<Secret>, String> {
        let entry = keyring::Entry::new(SERVICE, id).map_err(|err| err.to_string())?;
        match entry.get_password() {
            Ok(password) => Ok(Some(Secret::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// TorroMail as the core sees it: where its `state.json` is and how its passwords are read.
pub struct TorroMail {
    /// `None` without a home directory, or in tests that bring none: then there is no TorroMail.
    state: Option<PathBuf>,
    secrets: Arc<dyn ForeignSecrets>,
}

impl TorroMail {
    /// The user's TorroMail: its `state.json` where TorroMail keeps it, its keychain entries.
    pub fn system() -> TorroMail {
        TorroMail { state: system_state_file(), secrets: Arc::new(SystemForeignSecrets) }
    }

    /// No TorroMail: no accounts, no keychain. What the core of a test has unless it brings one.
    pub fn none() -> TorroMail {
        TorroMail::new(None, Arc::new(NoSecrets))
    }

    /// A TorroMail with its `state.json` at `state` and its passwords in `secrets`, for tests.
    pub fn new(state: Option<PathBuf>, secrets: Arc<dyn ForeignSecrets>) -> TorroMail {
        TorroMail { state, secrets }
    }

    /// TorroMail's accounts; none when it has no `state.json` (not installed, never set up). An
    /// unreadable file is an error that names no content.
    pub async fn accounts(&self) -> Result<Vec<Stored>, String> {
        let Some(path) = self.state.clone() else { return Ok(Vec::new()) };
        let read = tokio::task::spawn_blocking(move || std::fs::read_to_string(path)).await;
        match read {
            Ok(Ok(text)) => parse(&text),
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Ok(Err(err)) => Err(format!("TorroMail's state.json cannot be read: {err}")),
            Err(_) => Err("TorroMail's state.json cannot be read".to_string()),
        }
    }

    /// The password TorroMail keeps for `account`, read on a thread of its own (a keychain may
    /// block while it asks the person). Takes a listed account, so only an id from `state.json`
    /// is ever asked for.
    pub async fn password(&self, account: &Stored) -> Result<Option<Secret>, String> {
        let (secrets, id) = (Arc::clone(&self.secrets), account.id.clone());
        super::keychain(move || secrets.password(&id)).await
    }
}

/// A keychain without entries, for [`TorroMail::none`].
struct NoSecrets;

impl ForeignSecrets for NoSecrets {
    fn password(&self, _id: &str) -> Result<Option<Secret>, String> {
        Ok(None)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// TorroMail's keychain in memory: passwords by id, and which ids were asked for.
    #[derive(Clone, Default)]
    pub(crate) struct MemorySecrets {
        pub(crate) passwords: Arc<Mutex<HashMap<String, String>>>,
        pub(crate) asked: Arc<Mutex<Vec<String>>>,
        /// Every read fails, as a keychain that refuses this program does.
        pub(crate) refuse: bool,
    }

    impl MemorySecrets {
        pub(crate) fn with(id: &str, password: &str) -> MemorySecrets {
            let secrets = MemorySecrets::default();
            secrets.passwords.lock().expect("lock").insert(id.into(), password.into());
            secrets
        }

        pub(crate) fn asked(&self) -> Vec<String> {
            self.asked.lock().expect("lock").clone()
        }
    }

    impl ForeignSecrets for MemorySecrets {
        fn password(&self, id: &str) -> Result<Option<Secret>, String> {
            self.asked.lock().expect("lock").push(id.into());
            if self.refuse {
                return Err("User interaction is not allowed.".into());
            }
            Ok(self.passwords.lock().expect("lock").get(id).cloned().map(Secret::new))
        }
    }

    /// A `state.json` as TorroMail writes it (`state.rs`, `to_json`), with the accounts given.
    pub(crate) fn state_json(accounts: &[Value]) -> String {
        serde_json::json!({ "accounts": accounts, "selectedAccountId": null }).to_string()
    }

    /// An account object as TorroMail writes it, with everything the suite ignores as well.
    pub(crate) fn torromail_account(id: &str, email: &str, imap: (&str, u16), smtp: (&str, u16)) -> Value {
        serde_json::json!({
            "id": id,
            "name": "Anna Example",
            "email": email,
            "provider": "IMAP/SMTP",
            "loginMethod": "password",
            "imapHost": imap.0,
            "imapPort": imap.1,
            "imapSecurity": "tls",
            "smtpHost": smtp.0,
            "smtpPort": smtp.1,
            "smtpSecurity": "starttls",
            "username": "anna",
            "isVerified": true,
            "knownMailboxes": ["INBOX", "Sent"],
            "permissions": { "read": "all", "send": false },
            "searchCache": { "level": "headers" }
        })
    }

    #[test]
    fn the_state_file_is_where_torromail_keeps_it() {
        let home = Path::new("/home/anna");
        let at = |os, xdg: Option<&str>| state_file(os, Some(home), xdg.map(Path::new));
        assert_eq!(
            at(Os::Mac, Some("/x")),
            Some(PathBuf::from("/home/anna/Library/Application Support/TorroMail/state.json"))
        );
        assert_eq!(at(Os::Windows, None), Some(PathBuf::from("/home/anna/AppData/Local/TorroMail/state.json")));
        assert_eq!(at(Os::Linux, None), Some(PathBuf::from("/home/anna/.local/state/torromail/state.json")));
        // An absolute path on the platform running the test: "/state" is not absolute on Windows.
        let state = std::env::temp_dir().join("state");
        let state_text = state.to_str().expect("a temp dir in Unicode");
        assert_eq!(at(Os::Linux, Some(state_text)), Some(state.join("torromail/state.json")));
        // A relative XDG_STATE_HOME is invalid and ignored, as TorroMail does.
        assert_eq!(at(Os::Linux, Some("rel")), Some(PathBuf::from("/home/anna/.local/state/torromail/state.json")));
        assert_eq!(state_file(Os::Mac, None, None), None, "no home, no guess");
        assert_eq!(state_file(Os::Linux, None, Some(&state)), Some(state.join("torromail/state.json")));
    }

    #[test]
    fn accounts_are_read_as_torromail_reads_them() {
        let mut old = torromail_account(
            "01d10000-0000-4000-8000-000000000001",
            "bob@example.net",
            ("mail.example.net", 143),
            ("mail.example.net", 465),
        );
        // Written before the security was stated: it follows from the port.
        for key in ["imapSecurity", "smtpSecurity"] {
            old.as_object_mut().expect("object").remove(key);
        }
        let mut defaults = torromail_account(
            "01d20000-0000-4000-8000-000000000002",
            "carl@example.net",
            ("imap.example.net", 1),
            ("smtp.example.net", 1),
        );
        for key in ["imapPort", "smtpPort", "imapSecurity", "smtpSecurity"] {
            defaults.as_object_mut().expect("object").remove(key);
        }
        let text = state_json(&[
            torromail_account(
                "4c1d5e2a-0b6f-4a8e-9c3d-1e2f3a4b5caa",
                "anna@mailbox.org",
                ("imap.mailbox.org", 993),
                ("smtp.mailbox.org", 587),
            ),
            old,
            defaults,
        ]);
        let stored = parse(&text).expect("parsed");
        let endpoint = |host: &str, port, security| Some(Endpoint { host: host.into(), port, security });
        assert_eq!(stored.len(), 3);
        assert_eq!(
            stored[0],
            Stored {
                id: "4c1d5e2a-0b6f-4a8e-9c3d-1e2f3a4b5caa".into(),
                name: "Anna Example".into(),
                email: "anna@mailbox.org".into(),
                provider: "IMAP/SMTP".into(),
                oauth: false,
                imap: endpoint("imap.mailbox.org", 993, Security::Tls),
                smtp: endpoint("smtp.mailbox.org", 587, Security::StartTls),
                username: "anna".into(),
            }
        );
        assert_eq!(stored[0].unsupported(), None);
        assert_eq!(
            (stored[1].imap.clone(), stored[1].smtp.clone()),
            (endpoint("mail.example.net", 143, Security::StartTls), endpoint("mail.example.net", 465, Security::Tls))
        );
        assert_eq!(
            (stored[2].imap.clone(), stored[2].smtp.clone()),
            (endpoint("imap.example.net", 993, Security::Tls), endpoint("smtp.example.net", 587, Security::StartTls))
        );
    }

    #[test]
    fn oauth_jmap_and_half_set_up_accounts_say_why_and_broken_ones_are_left_out() {
        let mut oauth = torromail_account(
            "00000000-0000-4000-8000-00000000000a",
            "anna@gmail.com",
            ("imap.gmail.com", 993),
            ("smtp.gmail.com", 465),
        );
        oauth["loginMethod"] = "oauth".into();
        oauth["oauthIssuer"] = "google".into();
        let mut jmap = torromail_account(
            "00000000-0000-4000-8000-00000000000b",
            "anna@fastmail.com",
            ("imap.fastmail.com", 993),
            ("smtp.fastmail.com", 465),
        );
        jmap["provider"] = "Fastmail (JMAP)".into();
        let mut half = torromail_account(
            "00000000-0000-4000-8000-00000000000c",
            "anna@example.org",
            ("imap.example.org", 993),
            ("", 587),
        );
        half["smtpHost"] = "".into();
        let mut path = torromail_account("../../etc", "x@example.org", ("a.example.org", 993), ("b.example.org", 587));
        path["id"] = "../../etc".into();
        let mut method = torromail_account(
            "00000000-0000-4000-8000-00000000000d",
            "y@example.org",
            ("a.example.org", 993),
            ("b.example.org", 587),
        );
        method["loginMethod"] = "magic".into();
        let mut port = torromail_account(
            "00000000-0000-4000-8000-00000000000e",
            "z@example.org",
            ("a.example.org", 993),
            ("b.example.org", 587),
        );
        port["imapPort"] = 70000.into();
        // TorroMail's agent keys share its keychain service: never an account's password.
        let key = torromail_account("client-key-0f3c", "k@example.org", ("a.example.org", 993), ("b.example.org", 587));
        let plain = torromail_account("anna", "n@example.org", ("a.example.org", 993), ("b.example.org", 587));
        let stored = parse(&state_json(&[oauth, jmap, half, path, method, port, key, plain])).expect("parsed");
        let ids: Vec<&str> = stored.iter().map(|account| account.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "00000000-0000-4000-8000-00000000000a",
                "00000000-0000-4000-8000-00000000000b",
                "00000000-0000-4000-8000-00000000000c"
            ],
            "an id that is no UUID, a login method or a port that is none: left out"
        );
        assert!(is_plain_id("4C1D5E2A-0B6F-4A8E-9C3D-1E2F3A4B5C6D"), "upper case is a UUID too");
        for not_one in ["client-key-4c1d5e2a-0b6f-4a8e-9c3d-1e2f3a4b5c6d", "4c1d5e2a-0b6f-4a8e-9c3d-1e2f3a4b5c6", ""] {
            assert!(!is_plain_id(not_one), "{not_one}");
        }
        let why: Vec<Option<&str>> = stored.iter().map(Stored::unsupported).collect();
        assert_eq!(
            why,
            [
                Some(tr("Signs in with Google or Microsoft, which Torro Workplace does not support yet.")),
                Some(tr("Uses JMAP, which Torro Workplace does not support yet.")),
                Some(tr("Not completely set up in TorroMail.")),
            ]
        );
        assert!(parse("[]").is_err() && parse("not json").is_err(), "no accounts list is an error");
        assert_eq!(parse(r#"{"accounts": []}"#), Ok(Vec::new()));
    }

    #[test]
    fn listing_names_no_secret_and_marks_what_is_connected() {
        let account = torromail_account(
            "4c1d5e2a-0b6f-4a8e-9c3d-1e2f3a4b5c6d",
            "anna@mailbox.org",
            ("imap.mailbox.org", 993),
            ("smtp.mailbox.org", 587),
        );
        let stored = parse(&state_json(&[account])).expect("parsed");
        let listed = stored[0].listed(true);
        assert_eq!(
            (listed.address.as_str(), listed.sender_name.as_str(), listed.login_name.as_str(), listed.connected),
            ("anna@mailbox.org", "Anna Example", "anna", true)
        );
        assert_eq!(listed.unsupported, None);
    }

    #[tokio::test]
    async fn no_file_is_no_account_and_the_keychain_is_asked_for_listed_ids_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemorySecrets::with("4c1d5e2a-0b6f-4a8e-9c3d-1e2f3a4b5c6d", "hunter2-TM");
        let torromail = TorroMail::new(Some(dir.path().join(STATE_FILE)), Arc::new(secrets.clone()));
        assert_eq!(torromail.accounts().await, Ok(Vec::new()), "TorroMail never set up");
        assert!(secrets.asked().is_empty(), "listing reads no keychain");

        let account = torromail_account(
            "4c1d5e2a-0b6f-4a8e-9c3d-1e2f3a4b5c6d",
            "anna@mailbox.org",
            ("imap.mailbox.org", 993),
            ("smtp.mailbox.org", 587),
        );
        std::fs::write(dir.path().join(STATE_FILE), state_json(&[account])).expect("written");
        let stored = torromail.accounts().await.expect("read");
        let password = torromail.password(&stored[0]).await.expect("read").expect("stored");
        assert_eq!(password.expose(), "hunter2-TM");
        assert_eq!(secrets.asked(), ["4c1d5e2a-0b6f-4a8e-9c3d-1e2f3a4b5c6d"]);

        std::fs::write(dir.path().join(STATE_FILE), "{").expect("written");
        let broken = torromail.accounts().await.expect_err("not JSON");
        assert!(!broken.contains('{'), "names no content: {broken}");
        assert_eq!(TorroMail::none().accounts().await, Ok(Vec::new()));
    }
}
