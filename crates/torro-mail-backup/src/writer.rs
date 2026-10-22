//! Stream original EML bytes into a portable ZIP and verify it before returning success.
//!
//! Everything that could make `finish` fail after hours of reading (UID rules, flag and entry
//! limits, the manifest size) is checked when an entry is added, before its bytes are written.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};

use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, System, ZipWriter, write::SimpleFileOptions};

use crate::format::{
    Alias, ArchivePath, MAX_FLAGS, MAX_MESSAGE_BYTES, MAX_MESSAGES, MAX_TEXT_BYTES, MAX_TOTAL_FLAGS, Manifest, Message,
    Status,
};
use crate::{ArchiveError, MAX_INDEX_BYTES, MAX_MANIFEST_BYTES, VerifiedArchive, hex_digest, verify_archive};

const CHUNK_BYTES: usize = 64 * 1024;
/// Room in the manifest estimate for what is set after the last entry: status, finish time,
/// warnings added late and slack for number formatting.
const MANIFEST_RESERVE: u64 = 64 * 1024;
/// Room per folder for the inventory a source sets after reading it.
const INVENTORY_RESERVE: u64 = 256;
/// From this declared length on (or when it is unknown) an EML gets ZIP64 headers up front: with
/// a seekable output the writer cannot switch an already started entry over the 4 GiB limit.
const ZIP64_FROM: u64 = 0xFFFF_0000;

/// The fixed `README.txt` of every version-one archive; the verifier requires these exact bytes.
pub(crate) const README: &str = "Torro Workplace mail backup v1\nThis ordinary ZIP is not encrypted.\nOpen mail/*.eml with an RFC 822 mail viewer; MIME attachments are inside each EML.\nmanifest.json describes folders, original dates, flags and completeness.\nIts aliases list further folder entries, such as All Mail, whose bytes are already in the named EML.\nSHA256SUMS lists the EMLs and the three metadata files, but not itself.\nA complete status records a checked source inventory, not an atomic IMAP snapshot.\n";

/// Metadata supplied by the original-message source; the writer supplies generated ids and hashes.
#[derive(Debug, Clone, Default)]
pub struct MessageMetadata {
    /// Origin UID; set together with `uid_validity` or not at all, unique per folder.
    pub uid: Option<u32>,
    /// Must equal the folder's `uid_validity` in the manifest when set.
    pub uid_validity: Option<u32>,
    pub internal_date: Option<DateTime<FixedOffset>>,
    /// At most `MAX_FLAGS` non-empty flags of at most `MAX_TEXT_BYTES` each.
    pub flags: Vec<String>,
    /// A known IMAP literal length or local file length; a short read poisons the writer.
    /// For an alias it must equal the archived message's length when set.
    pub expected_bytes: Option<u64>,
}

/// A one-use writer. `finish` is the only route to a verified, complete archive.
pub struct ArchiveWriter<W: Read + Write + Seek> {
    zip: ZipWriter<W>,
    manifest: Manifest,
    sums: Vec<(String, String)>,
    poisoned: bool,
    /// An upper estimate of the compact manifest's length, kept below `manifest_limit`.
    manifest_bytes: u64,
    manifest_limit: u64,
    /// (folder number, UID) pairs already used by a message or alias.
    uids: HashSet<(u32, u32)>,
    flags: usize,
    /// The first archived message for each original (SHA-256, length), for `archived_path`.
    originals: HashMap<([u8; 32], u64), usize>,
}

impl<W: Read + Write + Seek> ArchiveWriter<W> {
    /// Start from an incomplete manifest with its account and folder table already set.
    pub fn new(output: W, manifest: Manifest) -> Result<Self, ArchiveError> {
        if manifest.status != Status::Incomplete
            || manifest.finished_at.is_some()
            || !manifest.messages.is_empty()
            || !manifest.aliases.is_empty()
        {
            return Err(ArchiveError::Invalid("archive writer needs an unfinished, empty manifest"));
        }
        manifest.validate()?;
        let manifest_bytes = json_len(&manifest)?
            .saturating_add(MANIFEST_RESERVE)
            .saturating_add(INVENTORY_RESERVE.saturating_mul(manifest.folders.len() as u64));
        if manifest_bytes > MAX_MANIFEST_BYTES {
            return Err(ArchiveError::Invalid("manifest would exceed its size limit"));
        }
        Ok(Self {
            zip: ZipWriter::new(output).set_auto_large_file(),
            manifest,
            sums: Vec::new(),
            poisoned: false,
            manifest_bytes,
            manifest_limit: MAX_MANIFEST_BYTES,
            uids: HashSet::new(),
            flags: 0,
            originals: HashMap::new(),
        })
    }

    /// After fetching, sources set final folder inventories here before `finish`. An inventory
    /// counts the folder's messages and aliases; its UID hash covers the UIDs of both.
    pub fn manifest_mut(&mut self) -> &mut Manifest {
        &mut self.manifest
    }

    /// Copy one RFC-822 original with bounded memory, preserving its bytes exactly.
    ///
    /// Folder, UID, flag and size rules are checked before anything is written; such an error
    /// leaves the writer usable. An error while copying poisons it.
    pub fn add_message<R: Read>(
        &mut self,
        folder_id: &str,
        mut input: R,
        metadata: MessageMetadata,
    ) -> Result<(), ArchiveError> {
        let folder_number = self.check_entry(folder_id, &metadata)?;
        let number = self.manifest.messages.len() as u64 + 1;
        let path = ArchivePath::message(folder_number, number)?.as_str().to_string();
        let large = metadata.expected_bytes.is_none_or(|expected| expected >= ZIP64_FROM);
        let time = zip_time(metadata.internal_date.map_or(self.manifest.started_at, |date| date.to_utc()));
        // Placeholders as long as the final values, so the size estimate is an upper bound.
        let mut message = Message {
            id: format!("m{number:09}"),
            folder_id: folder_id.to_string(),
            path,
            uid: metadata.uid,
            uid_validity: metadata.uid_validity,
            internal_date: metadata.internal_date,
            flags: metadata.flags,
            bytes: metadata.expected_bytes.unwrap_or(u64::MAX),
            sha256: "0".repeat(64),
        };
        let record_bytes = self.reserve_manifest(&message)?;

        self.poisoned = true;
        self.zip.start_file(&message.path, options(time).large_file(large))?;
        let mut digest = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0u8; CHUNK_BYTES];
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            bytes = bytes.checked_add(read as u64).ok_or(ArchiveError::Invalid("message length overflow"))?;
            if self.manifest.counts.bytes.saturating_add(bytes) > MAX_MESSAGE_BYTES {
                return Err(ArchiveError::Invalid("message archive size limit exceeded"));
            }
            self.zip.write_all(&buffer[..read])?;
            digest.update(&buffer[..read]);
        }
        if metadata.expected_bytes.is_some_and(|expected| expected != bytes) {
            return Err(ArchiveError::Invalid("original message length differs from source"));
        }
        let digest: [u8; 32] = digest.finalize().into();
        message.bytes = bytes;
        message.sha256 = hex_digest(&digest);
        self.sums.push((message.path.clone(), message.sha256.clone()));
        self.originals.entry((digest, bytes)).or_insert(self.manifest.messages.len());
        self.record_entry(folder_number, &message.uid, message.flags.len(), record_bytes);
        self.manifest.messages.push(message);
        self.manifest.counts.messages += 1;
        self.manifest.counts.bytes += bytes;
        self.poisoned = false;
        Ok(())
    }

    /// The ZIP path of an archived message in another folder than `folder_id` whose original has
    /// this lowercase hex SHA-256 and length; `None` if there is none yet. A source reading a
    /// collection folder (such as `\All`) last uses it to decide between `add_alias` (found) and
    /// `add_message` (not found).
    pub fn archived_path(&self, folder_id: &str, sha256: &str, bytes: u64) -> Option<&str> {
        let index = *self.originals.get(&(parse_sha256(sha256)?, bytes))?;
        let message = self.manifest.messages.get(index)?;
        (message.folder_id != folder_id).then_some(message.path.as_str())
    }

    /// Record a folder occurrence whose original bytes are already archived as the message at
    /// `same_as` (from `archived_path`), without writing a second EML. Copies that message's
    /// length and SHA-256; UID, date and flags come from `metadata` and follow the same rules as
    /// in `add_message`. The alias counts towards its folder's inventory, not towards
    /// `counts.messages` or `counts.bytes`. Errors leave the writer usable.
    pub fn add_alias(&mut self, folder_id: &str, same_as: &str, metadata: MessageMetadata) -> Result<(), ArchiveError> {
        let folder_number = self.check_entry(folder_id, &metadata)?;
        let target = ArchivePath::parse(same_as)
            .ok()
            .and_then(|path| usize::try_from(path.message_number()).ok())
            .and_then(|number| self.manifest.messages.get(number.wrapping_sub(1)))
            .filter(|message| message.path == same_as)
            .ok_or(ArchiveError::Invalid("alias does not name an archived message"))?;
        if target.folder_id == folder_id {
            return Err(ArchiveError::Invalid("alias must be in another folder than its message"));
        }
        if metadata.expected_bytes.is_some_and(|expected| expected != target.bytes) {
            return Err(ArchiveError::Invalid("alias length differs from its archived message"));
        }
        let alias = Alias {
            folder_id: folder_id.to_string(),
            uid: metadata.uid,
            uid_validity: metadata.uid_validity,
            internal_date: metadata.internal_date,
            flags: metadata.flags,
            bytes: target.bytes,
            sha256: target.sha256.clone(),
            same_as: same_as.to_string(),
        };
        let record_bytes = self.reserve_manifest(&alias)?;
        self.record_entry(folder_number, &alias.uid, alias.flags.len(), record_bytes);
        self.manifest.aliases.push(alias);
        Ok(())
    }

    /// Write human-readable metadata, close ZIP64 and run the offline verifier.
    pub fn finish(mut self, status: Status, finished_at: DateTime<Utc>) -> Result<(W, VerifiedArchive), ArchiveError> {
        if self.poisoned {
            return Err(ArchiveError::Invalid("archive writer was interrupted"));
        }
        self.manifest.status = status;
        self.manifest.finished_at = Some(finished_at);
        self.manifest.counts.folders = self.manifest.folders.len() as u64;
        self.manifest.validate()?;
        let time = zip_time(finished_at);

        self.zip.start_file("README.txt", options(time))?;
        self.zip.write_all(README.as_bytes())?;
        self.sums.push(("README.txt".into(), hex_digest(&Sha256::digest(README.as_bytes()))));

        self.zip.start_file("manifest.json", options(time))?;
        let manifest_hash = {
            let mut hashing = HashingWriter::new(&mut self.zip, self.manifest_limit);
            // Compact: the indentation of a pretty manifest would cost a third of the size limit.
            serde_json::to_writer(&mut hashing, &self.manifest)?;
            hashing.write_all(b"\n")?;
            hashing.finish()
        };
        self.sums.push(("manifest.json".into(), manifest_hash));

        self.zip.start_file("messages.csv", options(time))?;
        let csv_hash = {
            let mut hashing = HashingWriter::new(&mut self.zip, MAX_INDEX_BYTES);
            hashing.write_all(crate::verify::CSV_HEADER.as_bytes())?;
            for message in &self.manifest.messages {
                hashing.write_all(crate::verify::csv_row(message).as_bytes())?;
            }
            hashing.finish()
        };
        self.sums.push(("messages.csv".into(), csv_hash));

        self.zip.start_file("SHA256SUMS", options(time))?;
        let mut sums_bytes = 0u64;
        for (path, hash) in &self.sums {
            let line = format!("{hash}  {path}\n");
            sums_bytes += line.len() as u64;
            if sums_bytes > MAX_INDEX_BYTES {
                return Err(ArchiveError::Invalid("checksum list is too large"));
            }
            self.zip.write_all(line.as_bytes())?;
        }
        let mut output = self.zip.finish()?;
        output.seek(SeekFrom::Start(0))?;
        let report = verify_archive(&mut output)?;
        Ok((output, report))
    }

    /// Everything `add_message` and `add_alias` share before they change any state: the writer
    /// is usable, the folder takes messages, and UID, flag and entry limits hold. Returns the
    /// folder's number for the generated path.
    fn check_entry(&self, folder_id: &str, metadata: &MessageMetadata) -> Result<u32, ArchiveError> {
        if self.poisoned {
            return Err(ArchiveError::Invalid("archive writer was interrupted"));
        }
        let Some(folder) = self.manifest.folders.iter().find(|folder| folder.id == folder_id && folder.selectable)
        else {
            return Err(ArchiveError::Invalid("message folder is unknown or not selectable"));
        };
        let number: u32 = folder
            .id
            .strip_prefix('f')
            .and_then(|digits| digits.parse().ok())
            .ok_or(ArchiveError::Invalid("invalid folder id"))?;
        if self.manifest.messages.len() + self.manifest.aliases.len() >= MAX_MESSAGES {
            return Err(ArchiveError::Invalid("too many messages for archive"));
        }
        if metadata.uid == Some(0)
            || metadata.uid_validity == Some(0)
            || metadata.uid.is_some() != metadata.uid_validity.is_some()
            || (metadata.uid_validity.is_some() && metadata.uid_validity != folder.uid_validity)
        {
            return Err(ArchiveError::Invalid("message UID does not fit its folder"));
        }
        if metadata.uid.is_some_and(|uid| self.uids.contains(&(number, uid))) {
            return Err(ArchiveError::Invalid("duplicate UID in folder"));
        }
        if metadata.flags.len() > MAX_FLAGS
            || self.flags.saturating_add(metadata.flags.len()) > MAX_TOTAL_FLAGS
            || metadata.flags.iter().any(|flag| flag.is_empty() || flag.len() > MAX_TEXT_BYTES)
        {
            return Err(ArchiveError::Invalid("too many or invalid IMAP flags"));
        }
        Ok(number)
    }

    /// Fails early when one more record would push the manifest over its limit.
    fn reserve_manifest(&self, record: &impl Serialize) -> Result<u64, ArchiveError> {
        // One more byte for the separating comma.
        let record_bytes = json_len(record)?.saturating_add(1);
        if self.manifest_bytes.saturating_add(record_bytes) > self.manifest_limit {
            return Err(ArchiveError::Invalid("manifest would exceed its size limit"));
        }
        Ok(record_bytes)
    }

    fn record_entry(&mut self, folder_number: u32, uid: &Option<u32>, flags: usize, record_bytes: u64) {
        if let Some(uid) = uid {
            self.uids.insert((folder_number, *uid));
        }
        self.flags += flags;
        self.manifest_bytes += record_bytes;
    }
}

/// Every entry is a regular Unix file with mode 0644. Left to itself, zip records the writing system
/// ("version made by": DOS on Windows, Unix elsewhere), so the same backup would differ byte for
/// byte between machines and the checked-in sample could not be reproduced on Windows.
fn options(time: zip::DateTime) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(time)
        .system(System::Unix)
        .unix_permissions(0o644)
}

/// ZIP stores local wall-clock time without a zone; the archive uses UTC throughout. Dates the
/// format cannot hold (before 1980, after 2107) fall back to its epoch.
fn zip_time(time: DateTime<Utc>) -> zip::DateTime {
    let part = |value: u32| u8::try_from(value).unwrap_or(u8::MAX);
    u16::try_from(time.year())
        .ok()
        .and_then(|year| {
            zip::DateTime::from_date_and_time(
                year,
                part(time.month()),
                part(time.day()),
                part(time.hour()),
                part(time.minute()),
                part(time.second()),
            )
            .ok()
        })
        .unwrap_or_default()
}

fn parse_sha256(hex: &str) -> Option<[u8; 32]> {
    if !crate::format::valid_sha256(hex) {
        return None;
    }
    let mut digest = [0u8; 32];
    let (pairs, _) = hex.as_bytes().as_chunks::<2>();
    for (byte, pair) in digest.iter_mut().zip(pairs) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(digest)
}

/// The compact JSON length of `value`, without building the string.
fn json_len(value: &impl Serialize) -> Result<u64, ArchiveError> {
    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.0)
}

struct ByteCounter(u64);

impl Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct HashingWriter<W: Write> {
    writer: W,
    digest: Sha256,
    limit: u64,
    bytes: u64,
}

impl<W: Write> HashingWriter<W> {
    fn new(writer: W, limit: u64) -> Self {
        Self { writer, digest: Sha256::new(), limit, bytes: 0 }
    }

    fn finish(self) -> String {
        hex_digest(&self.digest.finalize())
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes.saturating_add(buf.len() as u64) > self.limit {
            return Err(std::io::Error::other("ZIP metadata size limit exceeded"));
        }
        let written = self.writer.write(buf)?;
        self.bytes += written as u64;
        self.digest.update(&buf[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{ArchiveWriter, MessageMetadata, json_len, parse_sha256};
    use crate::ArchiveError;
    use crate::format::{Manifest, Status};

    fn empty() -> Manifest {
        let mut manifest: Manifest =
            serde_json::from_str(include_str!("../tests/fixtures/manifest-v1-alias.json")).expect("fixture");
        manifest.status = Status::Incomplete;
        manifest.finished_at = None;
        manifest.messages.clear();
        manifest.aliases.clear();
        manifest.counts.messages = 0;
        manifest.counts.bytes = 0;
        for folder in &mut manifest.folders {
            folder.inventory = None;
        }
        manifest
    }

    fn writer() -> ArchiveWriter<Cursor<Vec<u8>>> {
        ArchiveWriter::new(Cursor::new(Vec::new()), empty()).expect("writer")
    }

    fn uid(uid: u32, validity: u32) -> MessageMetadata {
        MessageMetadata { uid: Some(uid), uid_validity: Some(validity), ..Default::default() }
    }

    #[test]
    fn uid_rules_fail_before_writing_and_keep_the_writer_usable() {
        let mut writer = writer();
        let only_uid = MessageMetadata { uid: Some(7), ..Default::default() };
        assert!(matches!(
            writer.add_message("f000001", b"x".as_slice(), only_uid),
            Err(ArchiveError::Invalid("message UID does not fit its folder"))
        ));
        assert!(matches!(
            writer.add_message("f000001", b"x".as_slice(), uid(7, 43)),
            Err(ArchiveError::Invalid("message UID does not fit its folder"))
        ));
        writer.add_message("f000001", b"x".as_slice(), uid(7, 42)).expect("first UID");
        assert!(matches!(
            writer.add_message("f000001", b"y".as_slice(), uid(7, 42)),
            Err(ArchiveError::Invalid("duplicate UID in folder"))
        ));
        let many = MessageMetadata { flags: vec!["$k".into(); crate::format::MAX_FLAGS + 1], ..Default::default() };
        assert!(matches!(
            writer.add_message("f000001", b"y".as_slice(), many),
            Err(ArchiveError::Invalid("too many or invalid IMAP flags"))
        ));
        writer.add_message("f000001", b"y".as_slice(), uid(8, 42)).expect("writer stayed usable");
        assert_eq!(writer.manifest_mut().messages.len(), 2);
    }

    #[test]
    fn manifest_limit_is_enforced_when_adding_not_when_finishing() {
        let mut writer = writer();
        writer.manifest_limit = writer.manifest_bytes + 400;
        writer.add_message("f000001", b"x".as_slice(), uid(1, 42)).expect("fits");
        let error = writer.add_message("f000001", b"y".as_slice(), uid(2, 42)).expect_err("would not fit");
        assert!(matches!(error, ArchiveError::Invalid("manifest would exceed its size limit")));
        assert!(!writer.poisoned, "rejected before the ZIP entry started");
    }

    #[test]
    fn manifest_estimate_is_an_upper_bound_of_the_written_manifest() {
        let mut writer = writer();
        for number in 1..=20 {
            let metadata = MessageMetadata { flags: vec!["\\Seen".into()], ..uid(number, 42) };
            writer.add_message("f000001", format!("message {number}").as_bytes(), metadata).expect("message");
        }
        let actual = json_len(&writer.manifest).expect("length");
        assert!(actual <= writer.manifest_bytes, "{actual} > {}", writer.manifest_bytes);
    }

    #[test]
    fn aliases_reuse_an_archived_message_from_another_folder() {
        let mut writer = writer();
        writer.add_message("f000001", b"raw".as_slice(), uid(7, 42)).expect("message");
        let hash = writer.manifest_mut().messages[0].sha256.clone();
        assert_eq!(writer.archived_path("f000002", &hash, 3), Some("mail/f000001/m000000001.eml"));
        assert_eq!(writer.archived_path("f000001", &hash, 3), None, "same folder needs its own EML");
        assert_eq!(writer.archived_path("f000002", &hash, 4), None);
        assert_eq!(writer.archived_path("f000002", "not hex", 3), None);
        assert!(matches!(
            writer.add_alias("f000002", "mail/f000001/m000000002.eml", uid(10, 99)),
            Err(ArchiveError::Invalid("alias does not name an archived message"))
        ));
        assert!(matches!(
            writer.add_alias("f000001", "mail/f000001/m000000001.eml", uid(10, 42)),
            Err(ArchiveError::Invalid("alias must be in another folder than its message"))
        ));
        let wrong_size = MessageMetadata { expected_bytes: Some(4), ..uid(10, 99) };
        assert!(matches!(
            writer.add_alias("f000002", "mail/f000001/m000000001.eml", wrong_size),
            Err(ArchiveError::Invalid("alias length differs from its archived message"))
        ));
        writer.add_alias("f000002", "mail/f000001/m000000001.eml", uid(10, 99)).expect("alias");
        assert!(matches!(
            writer.add_alias("f000002", "mail/f000001/m000000001.eml", uid(10, 99)),
            Err(ArchiveError::Invalid("duplicate UID in folder"))
        ));
        let manifest = writer.manifest_mut();
        assert_eq!((manifest.counts.messages, manifest.aliases.len()), (1, 1));
        assert_eq!(manifest.aliases[0].sha256, hash);
    }

    #[test]
    fn sha256_parsing_round_trips() {
        let hex = "775fb4368f2ec68c62a9bc2ce285f01f46ab8f66304d88bfd2e2c2825d0b7fb3";
        let digest = parse_sha256(hex).expect("digest");
        assert_eq!(crate::hex_digest(&digest), hex);
        assert!(parse_sha256(&hex.to_uppercase()).is_none());
    }
}
