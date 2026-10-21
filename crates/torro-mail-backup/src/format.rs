//! The version-one manifest and safe internal paths of a mail backup archive.

use std::collections::{HashMap, HashSet};
use std::fmt;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The format identifier written in every version-one manifest.
pub const FORMAT: &str = "torro-mail-backup";
/// The only manifest version understood by this crate.
pub const VERSION: u32 = 1;
/// A defensive ceiling for an archive's folder table.
pub const MAX_FOLDERS: usize = 100_000;
/// A defensive ceiling for an archive's message table.
pub const MAX_MESSAGES: usize = 10_000_000;
/// A defensive ceiling on logical message bytes in one archive (one pebibyte).
pub const MAX_MESSAGE_BYTES: u64 = 1 << 50;

/// A structural error in an archive manifest or a generated path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError(pub &'static str);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for FormatError {}

/// A ZIP member path made only from archive-generated numeric ids.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArchivePath(String);

impl ArchivePath {
    /// Makes a path without copying any IMAP folder or message text into the ZIP member name.
    pub fn message(folder: u32, message: u64) -> Result<Self, FormatError> {
        if folder == 0 || folder > 999_999 || message == 0 || message > 999_999_999 {
            return Err(FormatError("archive id is out of range"));
        }
        Ok(Self(format!("mail/f{folder:06}/m{message:09}.eml")))
    }

    /// Accepts exactly the canonical generated form; rejects traversal, aliases and backslashes.
    pub fn parse(path: &str) -> Result<Self, FormatError> {
        let Some(rest) = path.strip_prefix("mail/f") else {
            return Err(FormatError("message path is not in mail/"));
        };
        let Some((folder, message)) = rest.split_once("/m") else {
            return Err(FormatError("message path has no generated ids"));
        };
        let Some(message) = message.strip_suffix(".eml") else {
            return Err(FormatError("message path is not an EML"));
        };
        if folder.len() != 6
            || message.len() != 9
            || !folder.bytes().all(|b| b.is_ascii_digit())
            || !message.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(FormatError("message path has non-canonical ids"));
        }
        let folder = folder.parse().map_err(|_| FormatError("invalid folder id"))?;
        let message = message.parse().map_err(|_| FormatError("invalid message id"))?;
        let canonical = Self::message(folder, message)?;
        if canonical.as_str() != path {
            return Err(FormatError("message path is not canonical"));
        }
        Ok(canonical)
    }

    /// The safe path to store as a ZIP member name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which bodies the archive was drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Server,
    Local,
}

/// Complete means every eligible message was read and verified against a stable inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Complete,
    Incomplete,
}

/// Only descriptive account data; secret fields are not part of the format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// One selectable or reported IMAP folder; `path` is data, never a ZIP member name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    pub selectable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid_validity: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inventory: Option<Inventory>,
}

/// The final eligible source inventory for a folder. Server UID hashes use sorted big-endian
/// four-byte UIDs, with SHA-256 of the empty sequence for an empty folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub checked_at: DateTime<Utc>,
    pub eligible_messages: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid_set_sha256: Option<String>,
}

/// One archived folder occurrence of a message, with original IMAP metadata where available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub folder_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid_validity: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal_date: Option<DateTime<FixedOffset>>,
    #[serde(default)]
    pub flags: Vec<String>,
    pub bytes: u64,
    pub sha256: String,
}

/// Counts for a quick report, checked against the actual tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub folders: u64,
    pub messages: u64,
    pub bytes: u64,
}

/// The portable description of one ZIP archive; the message bodies remain separate EML files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub status: Status,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<NaiveDate>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub account: Account,
    pub folders: Vec<Folder>,
    pub messages: Vec<Message>,
    pub counts: Counts,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl Manifest {
    /// Checks the format contract without opening any ZIP member or contacting an account.
    pub fn validate(&self) -> Result<(), FormatError> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(FormatError("unknown mail backup format or version"));
        }
        if self.account.address.is_empty() || self.finished_at.is_some_and(|finished| finished < self.started_at) {
            return Err(FormatError("invalid account or time range"));
        }
        if self.status == Status::Complete && self.finished_at.is_none() {
            return Err(FormatError("complete archive has no finish time"));
        }
        if self.folders.len() > MAX_FOLDERS || self.messages.len() > MAX_MESSAGES {
            return Err(FormatError("archive has too many entries"));
        }
        let mut folder_ids = HashMap::with_capacity(self.folders.len());
        let mut folder_paths = HashSet::with_capacity(self.folders.len());
        for folder in &self.folders {
            if folder.path.is_empty()
                || !valid_folder_id(&folder.id)
                || folder.uid_validity == Some(0)
                || !folder_paths.insert(folder.path.as_str())
                || folder_ids.insert(folder.id.as_str(), (folder.uid_validity, folder.selectable)).is_some()
            {
                return Err(FormatError("invalid or duplicate folder"));
            }
        }
        let mut paths = HashSet::with_capacity(self.messages.len());
        let mut ids = HashSet::with_capacity(self.messages.len());
        let mut uids = HashSet::with_capacity(self.messages.len());
        let mut folder_uids: HashMap<&str, Vec<u32>> = HashMap::new();
        let mut folder_counts: HashMap<&str, u64> = HashMap::new();
        let mut total_bytes = 0u64;
        for message in &self.messages {
            if !folder_ids.contains_key(message.folder_id.as_str())
                || !valid_message_id(&message.id)
                || !ids.insert(message.id.as_str())
                || !paths.insert(message.path.as_str())
                || !folder_ids.get(message.folder_id.as_str()).is_some_and(|(_, selectable)| *selectable)
            {
                return Err(FormatError("invalid or duplicate message"));
            }
            ArchivePath::parse(&message.path)?;
            if message.path != format!("mail/{}/{}.eml", message.folder_id, message.id) {
                return Err(FormatError("message path does not match its ids"));
            }
            if message.uid == Some(0)
                || message.uid_validity == Some(0)
                || message.uid.is_some() != message.uid_validity.is_some()
                || (message.uid_validity.is_some()
                    && folder_ids.get(message.folder_id.as_str()).and_then(|(validity, _)| *validity)
                        != message.uid_validity)
                || message.uid.is_some_and(|uid| !uids.insert((message.folder_id.as_str(), message.uid_validity, uid)))
            {
                return Err(FormatError("invalid or duplicate folder UID"));
            }
            if message.sha256.len() != 64
                || !message.sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(FormatError("invalid message SHA-256"));
            }
            *folder_counts.entry(message.folder_id.as_str()).or_default() += 1;
            if let Some(uid) = message.uid {
                folder_uids.entry(message.folder_id.as_str()).or_default().push(uid);
            }
            total_bytes = total_bytes.checked_add(message.bytes).ok_or(FormatError("message sizes overflow"))?;
        }
        for folder in &self.folders {
            let Some(inventory) = &folder.inventory else {
                if self.status == Status::Complete && folder.selectable {
                    return Err(FormatError("complete archive has no folder inventory"));
                }
                continue;
            };
            let actual_count = folder_counts.get(folder.id.as_str()).copied().unwrap_or(0);
            if inventory.eligible_messages != actual_count
                || inventory.checked_at < self.started_at
                || self.finished_at.is_some_and(|finished| inventory.checked_at > finished)
            {
                return Err(FormatError("folder inventory does not match archive"));
            }
            match self.source {
                Source::Server => {
                    let mut selected = folder_uids.remove(folder.id.as_str()).unwrap_or_default();
                    if selected.len() as u64 != actual_count || folder.uid_validity.is_none() {
                        return Err(FormatError("server inventory lacks folder UIDs"));
                    }
                    selected.sort_unstable();
                    let mut hash = Sha256::new();
                    for uid in selected {
                        hash.update(uid.to_be_bytes());
                    }
                    let expected = crate::hex_digest(&hash.finalize());
                    if inventory.uid_set_sha256.as_deref() != Some(expected.as_str()) {
                        return Err(FormatError("server UID inventory does not match archive"));
                    }
                }
                Source::Local if inventory.uid_set_sha256.is_some() => {
                    return Err(FormatError("local inventory must not claim a server UID set"));
                }
                Source::Local => {}
            }
        }
        if total_bytes > MAX_MESSAGE_BYTES
            || self.counts.folders != self.folders.len() as u64
            || self.counts.messages != self.messages.len() as u64
            || self.counts.bytes != total_bytes
        {
            return Err(FormatError("manifest counts do not match"));
        }
        Ok(())
    }
}

fn valid_folder_id(id: &str) -> bool {
    id.len() == 7 && id.starts_with('f') && id[1..].bytes().all(|byte| byte.is_ascii_digit()) && id != "f000000"
}

fn valid_message_id(id: &str) -> bool {
    id.len() == 10 && id.starts_with('m') && id[1..].bytes().all(|byte| byte.is_ascii_digit()) && id != "m000000000"
}

#[cfg(test)]
mod tests {
    use super::{ArchivePath, Manifest};

    const EXAMPLE: &str = include_str!("../tests/fixtures/manifest-v1.json");

    #[test]
    fn example_manifest_is_valid_and_has_no_secret_fields() {
        let manifest: Manifest = serde_json::from_str(EXAMPLE).expect("example manifest");
        manifest.validate().expect("valid manifest");
        let json = serde_json::to_value(manifest).expect("serialize");
        let account = json.get("account").expect("account");
        assert!(account.get("password").is_none());
        assert!(account.get("token").is_none());
    }

    #[test]
    fn archive_paths_are_generated_from_numbers_only() {
        assert_eq!(ArchivePath::message(7, 12).expect("path").as_str(), "mail/f000007/m000000012.eml");
        assert!(ArchivePath::parse("../secret.eml").is_err());
        assert!(ArchivePath::parse("mail/f000007/../../secret.eml").is_err());
        assert!(ArchivePath::parse("mail\\f000007\\m000000012.eml").is_err());
    }

    #[test]
    fn duplicate_message_paths_are_rejected() {
        let mut manifest: Manifest = serde_json::from_str(EXAMPLE).expect("example manifest");
        let duplicate = manifest.messages.first().expect("message").clone();
        manifest.messages.push(duplicate);
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn incorrect_totals_are_rejected() {
        let mut manifest: Manifest = serde_json::from_str(EXAMPLE).expect("example manifest");
        manifest.counts.messages += 1;
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn message_uid_validity_must_match_its_folder() {
        let mut manifest: Manifest = serde_json::from_str(EXAMPLE).expect("example manifest");
        manifest.messages[0].uid_validity = Some(43);
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn non_selectable_folder_cannot_contain_messages() {
        let mut manifest: Manifest = serde_json::from_str(EXAMPLE).expect("example manifest");
        manifest.folders[0].selectable = false;
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn complete_server_claim_needs_matching_uid_inventory() {
        let mut manifest: Manifest = serde_json::from_str(EXAMPLE).expect("example manifest");
        manifest.folders[0].inventory.as_mut().expect("inventory").eligible_messages = 2;
        assert!(manifest.validate().is_err());
        manifest.folders[0].inventory.as_mut().expect("inventory").eligible_messages = 1;
        manifest.folders[0].inventory.as_mut().expect("inventory").uid_set_sha256 = Some("0".repeat(64));
        assert!(manifest.validate().is_err());
        manifest.folders[0].inventory = None;
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn manifest_schema_matches_fixture() {
        let generated = serde_json::to_string_pretty(&schemars::schema_for!(Manifest)).expect("schema") + "\n";
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/manifest-v1.schema.json");
        if std::env::var_os("TORRO_BLESS").is_some_and(|value| value == "1") {
            std::fs::write(path, &generated).expect("write schema fixture");
        }
        assert_eq!(std::fs::read_to_string(path).expect("schema fixture"), generated);
    }
}
