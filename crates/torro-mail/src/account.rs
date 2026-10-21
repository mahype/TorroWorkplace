//! A mail account: where its servers are ([`MailAccountConfig`]), what the person set for it
//! ([`MailSettings`]) and its id ([`account_id`]).
//!
//! Both structs are stored with the account in `accounts.json` (`torro_core::account::Account`'s
//! `mail` and `mail_settings`). The password is never here: it lives in the system keychain under
//! the account id, as for every other account, and travels only as `torro_core::account::Secret`.

use std::collections::BTreeMap;
use std::fmt;

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::message::Role;

/// Every mail account id starts with this: `mail:anna@example.org`.
pub const ACCOUNT_PREFIX: &str = "mail:";

/// The longest address taken (RFC 5321: 254 octets in a path).
const ADDRESS_MAX: usize = 254;

/// The id of the mail account of `address`: `mail:<address>`, with the domain in small letters
/// and the part before `@` as given (servers may tell case apart there). Surrounding white space
/// is dropped.
///
/// Refused ([`InvalidAddress`]) is an address that is empty, longer than 254 bytes, without
/// exactly one `@` with something on both sides, with a domain without a dot, or with white
/// space, a control or invisible format character (bidi, zero-width) or a `/` anywhere. The `/`
/// matters beyond mail: collection ids are `<account id>/<URL>` and are split at the first `/`
/// (task M1.1), so an account id must never hold one. Ids start with `mail:`, so they never name
/// a client key's keychain entry (`client-…`).
pub fn account_id(address: &str) -> Result<String, InvalidAddress> {
    let address = address.trim();
    if address.is_empty() {
        return Err(InvalidAddress("an e-mail address is needed"));
    }
    if address.len() > ADDRESS_MAX {
        return Err(InvalidAddress("the address is too long"));
    }
    if address.chars().any(|c| c.is_whitespace() || c.is_control() || is_invisible(c)) {
        return Err(InvalidAddress("the address holds spaces or invisible characters"));
    }
    if address.contains('/') {
        return Err(InvalidAddress("the address holds a /"));
    }
    let Some((local, domain)) = address.split_once('@') else {
        return Err(InvalidAddress("the address has no @"));
    };
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return Err(InvalidAddress("the address needs one @ with a name before and a domain after it"));
    }
    if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return Err(InvalidAddress("the domain of the address is not complete"));
    }
    Ok(format!("{ACCOUNT_PREFIX}{local}@{}", domain.to_lowercase()))
}

/// `id` is a mail account id exactly as [`account_id`] makes it: `mail:` and an address that
/// passes, its domain in small letters. For checking ids clients send (a selection of mail
/// accounts in a client's rights).
pub fn is_account_id(id: &str) -> bool {
    id.strip_prefix(ACCOUNT_PREFIX).is_some_and(|address| account_id(address).is_ok_and(|made| made == id))
}

/// Characters that change what a person reads without showing: zero-width ones, bidi controls,
/// the byte order mark, soft hyphen, word joiner and invisible operators.
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}')
}

/// An address [`account_id`] refuses; the text says why, for people (in English, like the other
/// messages of the core's answers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidAddress(pub &'static str);

impl fmt::Display for InvalidAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for InvalidAddress {}

/// How a connection to a mail server is secured. There is no plain connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Security {
    /// TLS from the first byte: IMAP on 993, SMTP on 465.
    #[serde(rename = "tls")]
    Tls,
    /// A plain start upgraded with STARTTLS before anything is sent: IMAP on 143, SMTP
    /// (submission) on 587. A server that does not offer STARTTLS is refused.
    #[serde(rename = "starttls")]
    StartTls,
}

/// One server of a mail account: host, port and how the connection is secured.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Endpoint {
    /// The host name, e.g. `imap.mailbox.org`.
    pub host: String,
    /// The port, e.g. 993 for IMAP over TLS or 587 for SMTP with STARTTLS.
    pub port: u16,
    /// `tls` or `starttls`.
    pub security: Security,
}

/// Where a mail account's servers are. Stored in `accounts.json`; never with the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MailAccountConfig {
    /// The account's e-mail address as the person typed it; the account id is made from it
    /// ([`account_id`]).
    pub address: String,
    /// The name put next to the address in `From:` (`Anna Example`); empty for none.
    #[serde(default)]
    pub sender_name: String,
    /// The IMAP server mail is read from.
    pub imap: Endpoint,
    /// The SMTP server mail is sent through (submission).
    pub smtp: Endpoint,
    /// The CalDAV address of the same sign-in, when calendars or tasks of the provider are used
    /// (task M7.1 finds it); `None` when the account is mail only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caldav: Option<String>,
}

/// Which notices of new mail the account puts into the inbox (task E4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Notify {
    /// Every new unread mail in the inbox folder, sorted into the lanes by the inbox's rules.
    #[default]
    All,
    /// Only mail the rules put into "For me"; newsletters and automatic mail stay out.
    ForMeOnly,
    /// No entries for this account.
    Off,
}

/// Which services of the account the suite uses. A service switched off is not synced, and
/// neither people nor agents see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Services {
    /// Mail (IMAP and SMTP).
    pub mail: bool,
    /// The calendars of the CalDAV address ([`MailAccountConfig::caldav`]).
    pub calendars: bool,
    /// The task lists of the CalDAV address.
    pub tasks: bool,
}

impl Default for Services {
    /// Mail only: calendars and tasks are chosen when the account is connected.
    fn default() -> Services {
        Services { mail: true, calendars: false, tasks: false }
    }
}

/// How new mail, replies and forwards of an account are written (issue #125).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailFormat {
    /// Formatted (bold, lists, links, quotes): sent as HTML with a plain text copy beside it
    /// (`multipart/alternative`), as mail programs do.
    #[default]
    Html,
    /// Plain text only.
    Plain,
}

/// What the person set for a mail account. Stored in `accounts.json`; changed with
/// `accounts.update_mail_account`, never together with servers or password.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct MailSettings {
    /// Text put under new mail, replies and forwards; `None` for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Which new mail becomes an inbox entry.
    #[serde(default)]
    pub notify: Notify,
    /// Which services the suite uses.
    #[serde(default)]
    pub services: Services,
    /// How the compose window writes new mail, replies and forwards: formatted (HTML) or plain
    /// text. Clients that save drafts decide for themselves (`Draft::html`).
    #[serde(default)]
    pub format: MailFormat,
    /// Folders the person picked for a role where the server offered two candidates with content
    /// (task M2.3 leaves such a role open, M5.2 asks once): the role and the folder's path as the
    /// server names it. Every other role is found anew on each sync and is not stored. An update
    /// (`accounts.update_mail_account`) sets only the roles it names and keeps the others.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub folder_roles: BTreeMap<Role, String>,
    /// The last backup choice of this account; no archive content or secret is stored here.
    #[serde(default, skip_serializing_if = "BackupPreferences::is_default")]
    pub backup: BackupPreferences,
}

/// Last-used backup settings, kept with the mail account rather than general settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct BackupPreferences {
    #[serde(default)]
    pub local: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<NaiveDate>,
    /// An absolute folder chosen by the suite user; `None` uses the private managed folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

impl BackupPreferences {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_choices_round_trip_without_changing_older_account_settings() {
        let old = serde_json::to_value(MailSettings::default()).expect("settings");
        assert!(old.get("backup").is_none());
        let preferences = BackupPreferences {
            local: true,
            since: NaiveDate::from_ymd_opt(2020, 1, 2),
            directory: Some("/selected".into()),
        };
        let mut settings: MailSettings = serde_json::from_value(old).expect("old settings");
        settings.backup = preferences.clone();
        let restored: MailSettings =
            serde_json::from_value(serde_json::to_value(settings).expect("serialized")).expect("restored");
        assert_eq!(restored.backup, preferences);
    }

    #[test]
    fn account_ids_name_the_address_with_its_domain_in_small_letters() {
        assert_eq!(account_id(" Anna@Mailbox.ORG "), Ok("mail:Anna@mailbox.org".into()));
        assert_eq!(account_id("a.b+tag@sub.example.co.uk"), Ok("mail:a.b+tag@sub.example.co.uk".into()));
        assert!(is_account_id("mail:anna@mailbox.org"));
        assert!(!is_account_id("mail:anna@Mailbox.org"), "not as account_id makes it");
        assert!(!is_account_id("caldav:anna@mailbox.org") && !is_account_id("mail:"));
    }

    #[test]
    fn addresses_with_a_slash_or_hidden_characters_are_refused() {
        for address in [
            "",
            "   ",
            "anna",
            "anna@",
            "@mailbox.org",
            "anna@localhost",
            "anna@mailbox.org.",
            "anna@@mailbox.org",
            "an@na@mailbox.org",
            "anna/x@mailbox.org",
            "anna@mailbox.org/inbox",
            "an na@mailbox.org",
            "anna\n@mailbox.org",
            "anna\u{7}@mailbox.org",
            "anna\u{202E}@mailbox.org",
            "anna\u{200B}@mailbox.org",
        ] {
            assert!(account_id(address).is_err(), "{address:?}");
        }
        assert!(account_id(&format!("{}@example.org", "a".repeat(250))).is_err(), "too long");
        assert!(!account_id("anna/x@mailbox.org").expect_err("refused").to_string().is_empty());
    }

    #[test]
    fn configs_and_settings_are_stored_readably_and_settings_have_defaults() {
        let config = MailAccountConfig {
            address: "anna@mailbox.org".into(),
            sender_name: "Anna".into(),
            imap: Endpoint { host: "imap.mailbox.org".into(), port: 993, security: Security::Tls },
            smtp: Endpoint { host: "smtp.mailbox.org".into(), port: 587, security: Security::StartTls },
            caldav: None,
        };
        let json = serde_json::to_value(&config).expect("json");
        assert_eq!(json["imap"]["security"], "tls");
        assert_eq!(json["smtp"]["security"], "starttls");
        assert!(json.get("caldav").is_none());
        assert_eq!(serde_json::from_value::<MailAccountConfig>(json).expect("read back"), config);

        let settings: MailSettings = serde_json::from_str("{}").expect("all defaults");
        assert_eq!(settings, MailSettings::default());
        assert_eq!((settings.notify, settings.services), (Notify::All, Services::default()));
        assert_eq!(settings.format, MailFormat::Html, "formatted unless the person chose plain text");
        assert!(settings.services.mail && !settings.services.calendars && !settings.services.tasks);
        let mut chosen = settings;
        chosen.folder_roles.insert(Role::Sent, "INBOX.Gesendete Objekte".into());
        chosen.notify = Notify::ForMeOnly;
        chosen.format = MailFormat::Plain;
        let json = serde_json::to_value(&chosen).expect("json");
        assert_eq!(json["format"], "plain");
        assert_eq!(json["folder_roles"]["sent"], "INBOX.Gesendete Objekte");
        assert_eq!(json["notify"], "for_me_only");
        assert_eq!(serde_json::from_value::<MailSettings>(json).expect("read back"), chosen);
    }
}
