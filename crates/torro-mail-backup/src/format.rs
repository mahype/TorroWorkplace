//! The version-one manifest and safe internal paths of a mail backup archive.
//!
//! Every list and free-text field is bounded while it is deserialized, not only afterwards in
//! [`Manifest::validate`]: a small compressed manifest must not be able to allocate gigabytes
//! before the checks run.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Read;
use std::marker::PhantomData;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::de::{Error as _, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

/// The format identifier written in every version-one manifest.
pub const FORMAT: &str = "torro-mail-backup";
/// The only manifest version understood by this crate.
pub const VERSION: u32 = 1;
/// A defensive ceiling for an archive's folder table.
pub const MAX_FOLDERS: usize = 100_000;
/// A defensive ceiling for messages and aliases together. Chosen so that this many entries with
/// typical metadata fit into the manifest size limit (checked by a test), and so that verifying
/// the largest allowed archive needs a few hundred megabytes of memory, not gigabytes.
pub const MAX_MESSAGES: usize = 1_000_000;
/// A defensive ceiling on logical message bytes in one archive (one pebibyte).
pub const MAX_MESSAGE_BYTES: u64 = 1 << 50;
/// IMAP flags and keywords per message or alias.
pub const MAX_FLAGS: usize = 256;
/// IMAP flags and keywords in the whole manifest; four per entry on average is generous.
pub const MAX_TOTAL_FLAGS: usize = 4 * MAX_MESSAGES;
/// Entries in `warnings`.
pub const MAX_WARNINGS: usize = 1_000;
/// Bytes of one free-text value: a folder name or path, a flag, a warning, an address.
pub const MAX_TEXT_BYTES: usize = 1_024;

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
pub struct ArchivePath {
    path: String,
    message: u64,
}

impl ArchivePath {
    /// Makes a path without copying any IMAP folder or message text into the ZIP member name.
    pub fn message(folder: u32, message: u64) -> Result<Self, FormatError> {
        if folder == 0 || folder > 999_999 || message == 0 || message > 999_999_999 {
            return Err(FormatError("archive id is out of range"));
        }
        Ok(Self { path: format!("mail/f{folder:06}/m{message:09}.eml"), message })
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
        &self.path
    }

    /// The generated message number; message `n` is entry `n - 1` of `Manifest::messages`.
    pub(crate) fn message_number(&self) -> u64 {
        self.message
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
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional_text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub server: Option<String>,
}

/// One selectable or reported IMAP folder; `path` is data, never a ZIP member name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Folder {
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub id: String,
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub name: String,
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional_text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub delimiter: Option<String>,
    pub selectable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional_text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid_validity: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inventory: Option<Inventory>,
}

/// The final eligible source inventory for a folder, covering its messages and its aliases.
/// Server UID hashes use sorted big-endian four-byte UIDs of both, with SHA-256 of the empty
/// sequence for an empty folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub checked_at: DateTime<Utc>,
    pub eligible_messages: u64,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional_text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub uid_set_sha256: Option<String>,
}

/// One archived folder occurrence of a message, with original IMAP metadata where available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Message {
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub id: String,
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub folder_id: String,
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid_validity: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal_date: Option<DateTime<FixedOffset>>,
    #[serde(default, deserialize_with = "flags")]
    #[schemars(length(max = MAX_FLAGS), inner(length(max = MAX_TEXT_BYTES)))]
    pub flags: Vec<String>,
    pub bytes: u64,
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub sha256: String,
}

/// A folder occurrence in a collection folder such as `\All` that has no EML of its own: the
/// same original bytes are already archived as the message at `same_as`, in another folder.
/// `bytes` and `sha256` repeat that message's values so the alias can be checked on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Alias {
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub folder_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid_validity: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal_date: Option<DateTime<FixedOffset>>,
    #[serde(default, deserialize_with = "flags")]
    #[schemars(length(max = MAX_FLAGS), inner(length(max = MAX_TEXT_BYTES)))]
    pub flags: Vec<String>,
    pub bytes: u64,
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub sha256: String,
    /// The ZIP path (`Message::path`) of the archived message with these original bytes.
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
    pub same_as: String,
}

/// Counts for a quick report, checked against the actual tables. `messages` and `bytes` count
/// EML files only; aliases add neither.
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
    #[serde(deserialize_with = "text")]
    #[schemars(length(max = MAX_TEXT_BYTES))]
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
    #[serde(deserialize_with = "folders")]
    #[schemars(length(max = MAX_FOLDERS))]
    pub folders: Vec<Folder>,
    #[serde(deserialize_with = "entries")]
    #[schemars(length(max = MAX_MESSAGES))]
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "entries")]
    #[schemars(length(max = MAX_MESSAGES))]
    pub aliases: Vec<Alias>,
    pub counts: Counts,
    #[serde(default, deserialize_with = "warnings")]
    #[schemars(length(max = MAX_WARNINGS), inner(length(max = MAX_TEXT_BYTES)))]
    pub warnings: Vec<String>,
}

/// The two fields every manifest version shares, read without `deny_unknown_fields` so that a
/// newer version is reported as such instead of as a JSON error about an unknown field.
#[derive(Deserialize)]
pub(crate) struct Head {
    #[serde(deserialize_with = "text")]
    pub format: String,
    pub version: u32,
}

impl Head {
    /// Rejects anything but a version-one Torro manifest before the strict parser runs.
    pub(crate) fn check(&self) -> Result<(), crate::ArchiveError> {
        if self.format != FORMAT {
            return Err(FormatError("not a Torro mail backup").into());
        }
        if self.version != VERSION {
            return Err(crate::ArchiveError::UnsupportedVersion(self.version));
        }
        Ok(())
    }
}

/// The fields that say whose archive it is, read without `deny_unknown_fields` so that the rest
/// of the manifest is skipped unchecked ([`crate::archive_account`]).
#[derive(Deserialize)]
pub(crate) struct Owner {
    #[serde(deserialize_with = "text")]
    pub format: String,
    pub account: Account,
}

/// Allowances shared by every list in one manifest being parsed by [`parse_manifest`].
#[derive(Debug, Clone, Copy)]
struct Budget {
    entries: usize,
    flags: usize,
}

thread_local! {
    // Per-list ceilings alone still allow `MAX_MESSAGES` lists of `MAX_FLAGS` tiny strings; the
    // budget caps the whole manifest. `None` outside `parse_manifest` (code building a manifest).
    static BUDGET: Cell<Option<Budget>> = const { Cell::new(None) };
}

/// Parses a manifest from a stream with whole-manifest allowances for entries and flags.
pub(crate) fn parse_manifest(reader: impl Read) -> Result<Manifest, serde_json::Error> {
    parse_with_budget(reader, MAX_MESSAGES, MAX_TOTAL_FLAGS)
}

fn parse_with_budget(reader: impl Read, entries: usize, flags: usize) -> Result<Manifest, serde_json::Error> {
    let _active = BudgetGuard::set(Budget { entries, flags });
    serde_json::from_reader(reader)
}

/// Holds the budget for one parse and clears it on drop, also when the reader panics; a stale
/// budget would otherwise cap later, unrelated deserializations on the same thread.
struct BudgetGuard;

impl BudgetGuard {
    fn set(budget: Budget) -> Self {
        BUDGET.with(|cell| cell.set(Some(budget)));
        Self
    }
}

impl Drop for BudgetGuard {
    fn drop(&mut self) {
        BUDGET.with(|cell| cell.set(None));
    }
}

/// Takes one unit from the running budget; `false` when it is used up.
fn spend(take: fn(&mut Budget) -> &mut usize) -> bool {
    BUDGET.with(|cell| {
        let Some(mut budget) = cell.get() else {
            return true;
        };
        let left = take(&mut budget);
        if *left == 0 {
            return false;
        }
        *left -= 1;
        cell.set(Some(budget));
        true
    })
}

fn text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.len() > MAX_TEXT_BYTES {
        return Err(D::Error::custom("manifest text is too long"));
    }
    Ok(value)
}

fn optional_text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    if value.as_ref().is_some_and(|value| value.len() > MAX_TEXT_BYTES) {
        return Err(D::Error::custom("manifest text is too long"));
    }
    Ok(value)
}

fn folders<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Folder>, D::Error> {
    deserializer.deserialize_seq(Bounded::<Folder>::new(MAX_FOLDERS, None))
}

fn entries<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Vec<T>, D::Error> {
    deserializer.deserialize_seq(Bounded::<T>::new(MAX_MESSAGES, Some(|budget| &mut budget.entries)))
}

fn flags<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    deserializer
        .deserialize_seq(Bounded::<BoundedText>::new(MAX_FLAGS, Some(|budget| &mut budget.flags)))
        .map(|flags| flags.into_iter().map(|flag| flag.0).collect())
}

fn warnings<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    deserializer
        .deserialize_seq(Bounded::<BoundedText>::new(MAX_WARNINGS, None))
        .map(|warnings| warnings.into_iter().map(|warning| warning.0).collect())
}

/// A list element string checked against [`MAX_TEXT_BYTES`] as soon as it is read.
struct BoundedText(String);

impl<'de> Deserialize<'de> for BoundedText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        text(deserializer).map(Self)
    }
}

/// A sequence visitor that stops at `max` elements and, while parsing a whole manifest, also
/// draws each element from a shared [`Budget`].
struct Bounded<T> {
    max: usize,
    budget: Option<fn(&mut Budget) -> &mut usize>,
    item: PhantomData<T>,
}

impl<T> Bounded<T> {
    fn new(max: usize, budget: Option<fn(&mut Budget) -> &mut usize>) -> Self {
        Self { max, budget, item: PhantomData }
    }
}

impl<'de, T: Deserialize<'de>> Visitor<'de> for Bounded<T> {
    type Value = Vec<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a list of at most {} items", self.max)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
        // Never trust a size hint for the allocation; grow as elements really arrive.
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            if items.len() == self.max {
                return Err(A::Error::custom("manifest list is too long"));
            }
            if self.budget.is_some_and(|take| !spend(take)) {
                return Err(A::Error::custom("manifest has too many entries or flags"));
            }
            items.push(item);
        }
        Ok(items)
    }
}

/// UID facts shared by messages and aliases, so both follow the same folder rules.
struct Occurrence<'a> {
    folder_id: &'a str,
    uid: Option<u32>,
    uid_validity: Option<u32>,
    flags: &'a [String],
    sha256: &'a str,
}

impl Message {
    fn occurrence(&self) -> Occurrence<'_> {
        Occurrence {
            folder_id: &self.folder_id,
            uid: self.uid,
            uid_validity: self.uid_validity,
            flags: &self.flags,
            sha256: &self.sha256,
        }
    }
}

impl Alias {
    fn occurrence(&self) -> Occurrence<'_> {
        Occurrence {
            folder_id: &self.folder_id,
            uid: self.uid,
            uid_validity: self.uid_validity,
            flags: &self.flags,
            sha256: &self.sha256,
        }
    }
}

/// Folder facts collected while checking the entries, compared with each folder's inventory.
#[derive(Default)]
struct FolderTally {
    entries: u64,
    uids: Vec<u32>,
}

/// The running state of [`Manifest::validate`] over messages and aliases.
struct EntryCheck<'a> {
    /// Folder id → (UIDVALIDITY, selectable).
    folders: HashMap<&'a str, (Option<u32>, bool)>,
    uids: HashSet<(&'a str, u32)>,
    tallies: HashMap<&'a str, FolderTally>,
    flags: usize,
}

impl<'a> EntryCheck<'a> {
    fn add(&mut self, entry: Occurrence<'a>) -> Result<(), FormatError> {
        let Some((folder_validity, selectable)) = self.folders.get(entry.folder_id).copied() else {
            return Err(FormatError("entry folder is unknown"));
        };
        if !selectable {
            return Err(FormatError("non-selectable folder cannot contain messages"));
        }
        if entry.uid == Some(0)
            || entry.uid_validity == Some(0)
            || entry.uid.is_some() != entry.uid_validity.is_some()
            || (entry.uid_validity.is_some() && folder_validity != entry.uid_validity)
            || entry.uid.is_some_and(|uid| !self.uids.insert((entry.folder_id, uid)))
        {
            return Err(FormatError("invalid or duplicate folder UID"));
        }
        if !valid_sha256(entry.sha256) {
            return Err(FormatError("invalid message SHA-256"));
        }
        self.flags = self.flags.saturating_add(entry.flags.len());
        if entry.flags.len() > MAX_FLAGS
            || self.flags > MAX_TOTAL_FLAGS
            || entry.flags.iter().any(|flag| flag.is_empty() || flag.len() > MAX_TEXT_BYTES)
        {
            return Err(FormatError("too many or invalid IMAP flags"));
        }
        let tally = self.tallies.entry(entry.folder_id).or_default();
        tally.entries += 1;
        if let Some(uid) = entry.uid {
            tally.uids.push(uid);
        }
        Ok(())
    }
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
        if self.folders.len() > MAX_FOLDERS
            || self.messages.len().saturating_add(self.aliases.len()) > MAX_MESSAGES
            || self.warnings.len() > MAX_WARNINGS
        {
            return Err(FormatError("archive has too many entries"));
        }
        if !self.texts_fit() {
            return Err(FormatError("manifest text is too long"));
        }
        let mut check = EntryCheck {
            folders: HashMap::with_capacity(self.folders.len()),
            uids: HashSet::with_capacity(self.messages.len() + self.aliases.len()),
            tallies: HashMap::new(),
            flags: 0,
        };
        let mut folder_paths = HashSet::with_capacity(self.folders.len());
        for folder in &self.folders {
            if folder.path.is_empty()
                || !valid_folder_id(&folder.id)
                || folder.uid_validity == Some(0)
                || !folder_paths.insert(folder.path.as_str())
                || check.folders.insert(folder.id.as_str(), (folder.uid_validity, folder.selectable)).is_some()
            {
                return Err(FormatError("invalid or duplicate folder"));
            }
        }
        let mut by_path: HashMap<&str, &Message> = HashMap::with_capacity(self.messages.len());
        let mut ids = HashSet::with_capacity(self.messages.len());
        let mut total_bytes = 0u64;
        for message in &self.messages {
            if !valid_message_id(&message.id) || !ids.insert(message.id.as_str()) {
                return Err(FormatError("invalid or duplicate message"));
            }
            ArchivePath::parse(&message.path)?;
            if message.path != format!("mail/{}/{}.eml", message.folder_id, message.id) {
                return Err(FormatError("message path does not match its ids"));
            }
            if by_path.insert(message.path.as_str(), message).is_some() {
                return Err(FormatError("invalid or duplicate message"));
            }
            check.add(message.occurrence())?;
            total_bytes = total_bytes.checked_add(message.bytes).ok_or(FormatError("message sizes overflow"))?;
        }
        for alias in &self.aliases {
            let Some(target) = by_path.get(alias.same_as.as_str()) else {
                return Err(FormatError("alias does not name an archived message"));
            };
            if target.sha256 != alias.sha256 || target.bytes != alias.bytes {
                return Err(FormatError("alias differs from its archived message"));
            }
            if target.folder_id == alias.folder_id {
                return Err(FormatError("alias must be in another folder than its message"));
            }
            check.add(alias.occurrence())?;
        }
        self.validate_inventories(&mut check.tallies)?;
        if total_bytes > MAX_MESSAGE_BYTES
            || self.counts.folders != self.folders.len() as u64
            || self.counts.messages != self.messages.len() as u64
            || self.counts.bytes != total_bytes
        {
            return Err(FormatError("manifest counts do not match"));
        }
        Ok(())
    }

    /// Repeats the deserializer's text limits for manifests built in code.
    fn texts_fit(&self) -> bool {
        let fits = |text: &str| text.len() <= MAX_TEXT_BYTES;
        fits(&self.format)
            && fits(&self.account.address)
            && self.account.server.as_deref().is_none_or(fits)
            && self.warnings.iter().all(|warning| fits(warning))
            && self.folders.iter().all(|folder| {
                fits(&folder.name)
                    && fits(&folder.path)
                    && folder.delimiter.as_deref().is_none_or(fits)
                    && folder.role.as_deref().is_none_or(fits)
            })
    }

    fn validate_inventories(&self, tallies: &mut HashMap<&str, FolderTally>) -> Result<(), FormatError> {
        for folder in &self.folders {
            let Some(inventory) = &folder.inventory else {
                if self.status == Status::Complete && folder.selectable {
                    return Err(FormatError("complete archive has no folder inventory"));
                }
                continue;
            };
            let tally = tallies.remove(folder.id.as_str()).unwrap_or_default();
            if inventory.eligible_messages != tally.entries
                || inventory.checked_at < self.started_at
                || self.finished_at.is_some_and(|finished| inventory.checked_at > finished)
            {
                return Err(FormatError("folder inventory does not match archive"));
            }
            match self.source {
                Source::Server => {
                    let mut uids = tally.uids;
                    if uids.len() as u64 != tally.entries || folder.uid_validity.is_none() {
                        return Err(FormatError("server inventory lacks folder UIDs"));
                    }
                    uids.sort_unstable();
                    let mut hash = Sha256::new();
                    for uid in uids {
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
        Ok(())
    }
}

pub(crate) fn valid_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_folder_id(id: &str) -> bool {
    id.strip_prefix('f').is_some_and(|digits| digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        && id != "f000000"
}

fn valid_message_id(id: &str) -> bool {
    id.strip_prefix('m').is_some_and(|digits| digits.len() == 9 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        && id != "m000000000"
}

#[cfg(test)]
mod tests {
    use super::{Alias, ArchivePath, FormatError, MAX_FLAGS, MAX_MESSAGES, Manifest, parse_with_budget};

    const EXAMPLE: &str = include_str!("../tests/fixtures/manifest-v1.json");
    const WITH_ALIAS: &str = include_str!("../tests/fixtures/manifest-v1-alias.json");

    fn example() -> Manifest {
        serde_json::from_str(EXAMPLE).expect("example manifest")
    }

    fn with_alias() -> Manifest {
        serde_json::from_str(WITH_ALIAS).expect("alias manifest")
    }

    fn rejects(manifest: &Manifest, reason: &'static str) {
        assert_eq!(manifest.validate(), Err(FormatError(reason)));
    }

    #[test]
    fn example_manifest_is_valid_and_has_no_secret_fields() {
        let manifest = example();
        manifest.validate().expect("valid manifest");
        let json = serde_json::to_value(manifest).expect("serialize");
        let account = json.get("account").expect("account");
        assert!(account.get("password").is_none());
        assert!(account.get("token").is_none());
        assert!(json.get("aliases").is_none(), "an archive without aliases keeps the plain v1 shape");
    }

    #[test]
    fn archive_paths_are_generated_from_numbers_only() {
        assert_eq!(ArchivePath::message(7, 12).expect("path").as_str(), "mail/f000007/m000000012.eml");
        assert_eq!(ArchivePath::parse("mail/f000007/m000000012.eml").expect("path").message_number(), 12);
        assert!(ArchivePath::parse("../secret.eml").is_err());
        assert!(ArchivePath::parse("mail/f000007/../../secret.eml").is_err());
        assert!(ArchivePath::parse("mail\\f000007\\m000000012.eml").is_err());
        assert!(ArchivePath::parse("/mail/f000007/m000000012.eml").is_err());
    }

    #[test]
    fn duplicate_message_paths_are_rejected() {
        let mut manifest = example();
        let duplicate = manifest.messages[0].clone();
        manifest.messages.push(duplicate);
        rejects(&manifest, "invalid or duplicate message");
    }

    #[test]
    fn incorrect_totals_are_rejected() {
        let mut manifest = example();
        manifest.counts.messages += 1;
        rejects(&manifest, "manifest counts do not match");
    }

    #[test]
    fn message_uid_validity_must_match_its_folder() {
        let mut manifest = example();
        manifest.messages[0].uid_validity = Some(43);
        rejects(&manifest, "invalid or duplicate folder UID");
    }

    #[test]
    fn non_selectable_folder_cannot_contain_messages() {
        let mut manifest = example();
        manifest.folders[0].selectable = false;
        rejects(&manifest, "non-selectable folder cannot contain messages");
    }

    #[test]
    fn complete_server_claim_needs_matching_uid_inventory() {
        let mut manifest = example();
        manifest.folders[0].inventory.as_mut().expect("inventory").eligible_messages = 2;
        rejects(&manifest, "folder inventory does not match archive");
        manifest.folders[0].inventory.as_mut().expect("inventory").eligible_messages = 1;
        manifest.folders[0].inventory.as_mut().expect("inventory").uid_set_sha256 = Some("0".repeat(64));
        rejects(&manifest, "server UID inventory does not match archive");
        manifest.folders[0].inventory = None;
        rejects(&manifest, "complete archive has no folder inventory");
    }

    #[test]
    fn alias_fixture_counts_aliases_in_the_collection_inventory() {
        let manifest = with_alias();
        manifest.validate().expect("valid alias manifest");
        assert_eq!(manifest.aliases.len(), 1);
        assert_eq!(manifest.counts.messages, 1, "an alias has no EML of its own");
        let mut uncounted = manifest.clone();
        uncounted.folders[1].inventory.as_mut().expect("inventory").eligible_messages = 0;
        rejects(&uncounted, "folder inventory does not match archive");
        let mut other_uid = manifest;
        other_uid.aliases[0].uid = Some(11);
        rejects(&other_uid, "server UID inventory does not match archive");
    }

    #[test]
    fn aliases_must_point_at_an_identical_message_in_another_folder() {
        let alias = |change: fn(&mut Alias)| {
            let mut manifest = with_alias();
            change(&mut manifest.aliases[0]);
            manifest
        };
        rejects(
            &alias(|a| a.same_as = "mail/f000001/m000000002.eml".into()),
            "alias does not name an archived message",
        );
        rejects(&alias(|a| a.same_as = "../m000000001.eml".into()), "alias does not name an archived message");
        rejects(&alias(|a| a.sha256 = "0".repeat(64)), "alias differs from its archived message");
        rejects(&alias(|a| a.bytes += 1), "alias differs from its archived message");
        rejects(&alias(|a| a.folder_id = "f000001".into()), "alias must be in another folder than its message");
        rejects(&alias(|a| a.folder_id = "f000009".into()), "entry folder is unknown");
        rejects(&alias(|a| a.uid_validity = None), "invalid or duplicate folder UID");
    }

    #[test]
    fn flag_lists_are_bounded_while_parsing_and_in_validation() {
        let mut manifest = example();
        manifest.messages[0].flags = vec!["$k".into(); MAX_FLAGS + 1];
        rejects(&manifest, "too many or invalid IMAP flags");
        let json = serde_json::to_string(&manifest).expect("serialize");
        let error = serde_json::from_str::<Manifest>(&json).expect_err("too many flags");
        assert!(error.to_string().contains("manifest list is too long"), "{error}");
        manifest.messages[0].flags = vec![String::new()];
        rejects(&manifest, "too many or invalid IMAP flags");
    }

    #[test]
    fn whole_manifest_flag_budget_stops_many_small_lists() {
        let mut manifest = with_alias();
        manifest.messages[0].flags = vec!["\\Seen".into(), "$a".into()];
        manifest.aliases[0].flags = vec!["\\Seen".into(), "$b".into()];
        let json = serde_json::to_vec(&manifest).expect("serialize");
        parse_with_budget(json.as_slice(), MAX_MESSAGES, 4).expect("within budget");
        let error = parse_with_budget(json.as_slice(), MAX_MESSAGES, 3).expect_err("budget exhausted");
        assert!(error.to_string().contains("too many entries or flags"), "{error}");
        let error = parse_with_budget(json.as_slice(), 1, 4).expect_err("entry budget exhausted");
        assert!(error.to_string().contains("too many entries or flags"), "{error}");
        // Outside a budgeted parse only the per-list ceilings apply.
        serde_json::from_slice::<Manifest>(&json).expect("ordinary parse");
    }

    #[test]
    fn a_panicking_reader_leaves_no_budget_behind() {
        struct Panics;
        impl std::io::Read for Panics {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("reader failed");
            }
        }
        let outcome = std::panic::catch_unwind(|| parse_with_budget(Panics, 0, 0));
        assert!(outcome.is_err(), "the reader panicked");
        assert!(super::BUDGET.with(|cell| cell.get()).is_none(), "the budget is cleared after a panic");
        let mut manifest = with_alias();
        manifest.messages[0].flags = vec!["\\Seen".into()];
        let json = serde_json::to_vec(&manifest).expect("serialize");
        serde_json::from_slice::<Manifest>(&json).expect("later parses are not capped");
    }

    #[test]
    fn long_texts_and_many_warnings_are_rejected() {
        let mut manifest = example();
        manifest.folders[0].name = "x".repeat(super::MAX_TEXT_BYTES + 1);
        rejects(&manifest, "manifest text is too long");
        let json = serde_json::to_string(&manifest).expect("serialize");
        assert!(serde_json::from_str::<Manifest>(&json).is_err());
        let mut manifest = example();
        manifest.warnings = vec!["w".into(); super::MAX_WARNINGS + 1];
        rejects(&manifest, "archive has too many entries");
    }

    #[test]
    fn typical_entries_up_to_the_ceiling_fit_into_the_manifest_limit() {
        let mut manifest = with_alias();
        let message = &mut manifest.messages[0];
        message.uid = Some(u32::MAX);
        message.bytes = 1 << 40;
        message.flags = vec!["\\Seen".into(), "\\Flagged".into(), "$Forwarded".into(), "$Label1".into()];
        let record = serde_json::to_vec(&manifest.messages[0]).expect("record").len() as u64 + 1;
        let alias = serde_json::to_vec(&manifest.aliases[0]).expect("alias").len() as u64 + 1;
        assert!(record.max(alias) * MAX_MESSAGES as u64 + 64 * 1024 * 1024 <= crate::MAX_MANIFEST_BYTES);
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
