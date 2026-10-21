//! Portable, versioned mail backup archives shared by the daemon's backup and restore paths.

pub mod format;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{Read, Seek};

use chrono::{DateTime, NaiveDate, Utc};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::format::{FormatError, MAX_MESSAGE_BYTES, MAX_MESSAGES, Manifest, Source, Status};

/// A malformed, oversized or damaged mail archive.
#[derive(Debug)]
pub enum ArchiveError {
    Zip(zip::result::ZipError),
    Io(std::io::Error),
    Json(serde_json::Error),
    Format(FormatError),
    Invalid(&'static str),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zip(err) => write!(f, "ZIP: {err}"),
            Self::Io(err) => write!(f, "I/O: {err}"),
            Self::Json(err) => write!(f, "manifest JSON: {err}"),
            Self::Format(err) => write!(f, "manifest: {err}"),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<zip::result::ZipError> for ArchiveError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Zip(error)
    }
}

impl From<std::io::Error> for ArchiveError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ArchiveError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<FormatError> for ArchiveError {
    fn from(error: FormatError) -> Self {
        Self::Format(error)
    }
}

/// Counts and completeness after an offline integrity check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedArchive {
    pub account: String,
    pub source: Source,
    pub since: Option<NaiveDate>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub complete: bool,
    pub folders: u64,
    pub messages: u64,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

/// Verifies a standard ZIP without extracting it or opening an IMAP connection.
///
/// Only the four documented metadata names and generated EML names are accepted. Size and
/// expansion limits apply while reading, so forged ZIP headers cannot bypass them.
pub fn verify_archive<R: Read + Seek>(input: R) -> Result<VerifiedArchive, ArchiveError> {
    const MAX_MANIFEST_BYTES: u64 = 256 * 1024 * 1024;
    const MAX_INDEX_BYTES: u64 = 512 * 1024 * 1024;
    let mut zip = ZipArchive::new(input)?;
    if zip.len() > MAX_MESSAGES.saturating_add(4) {
        return Err(ArchiveError::Invalid("too many ZIP members"));
    }
    let manifest_bytes = read_limited(&mut zip.by_name("manifest.json")?, MAX_MANIFEST_BYTES)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    manifest.validate()?;
    let sums_bytes = read_limited(&mut zip.by_name("SHA256SUMS")?, MAX_INDEX_BYTES)?;
    let sums = parse_sums(&sums_bytes)?;
    let mut expected: HashMap<&str, Option<(u64, &str)>> = HashMap::with_capacity(manifest.messages.len() + 3);
    expected.insert("README.txt", None);
    expected.insert("manifest.json", None);
    expected.insert("messages.csv", None);
    for message in &manifest.messages {
        expected.insert(message.path.as_str(), Some((message.bytes, message.sha256.as_str())));
    }
    if sums.len() != expected.len() || zip.len() != expected.len() + 1 {
        return Err(ArchiveError::Invalid("ZIP inventory or checksum list is incomplete"));
    }
    let mut seen = HashSet::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index)?;
        let name = file.name()?.into_owned();
        if !seen.insert(name.clone()) {
            return Err(ArchiveError::Invalid("duplicate ZIP member"));
        }
        if name == "SHA256SUMS" {
            if file.size() > MAX_INDEX_BYTES {
                return Err(ArchiveError::Invalid("checksum list is too large"));
            }
            continue;
        }
        let Some(expected_message) = expected.get(name.as_str()) else {
            return Err(ArchiveError::Invalid("unexpected or unsafe ZIP member"));
        };
        let Some(expected_hash) = sums.get(name.as_str()) else {
            return Err(ArchiveError::Invalid("ZIP member has no checksum"));
        };
        let ceiling = match expected_message {
            Some((size, _)) => *size,
            None if name == "manifest.json" => MAX_MANIFEST_BYTES,
            None => MAX_INDEX_BYTES,
        };
        if file.size() > ceiling
            || file.size() > MAX_MESSAGE_BYTES
            || file.size() > file.compressed_size().saturating_mul(10_000).max(1_000_000)
        {
            return Err(ArchiveError::Invalid("ZIP member exceeds size or expansion limit"));
        }
        let (actual_bytes, actual_hash) = hash_limited(&mut file, ceiling)?;
        if actual_hash != *expected_hash {
            return Err(ArchiveError::Invalid("ZIP member SHA-256 mismatch"));
        }
        if let Some((expected_bytes, message_hash)) = expected_message
            && (actual_bytes != *expected_bytes || actual_hash != *message_hash)
        {
            return Err(ArchiveError::Invalid("EML differs from manifest"));
        }
    }
    if seen.len() != expected.len() + 1 {
        return Err(ArchiveError::Invalid("ZIP member is missing"));
    }
    Ok(VerifiedArchive {
        account: manifest.account.address,
        source: manifest.source,
        since: manifest.since,
        started_at: manifest.started_at,
        finished_at: manifest.finished_at,
        complete: manifest.status == Status::Complete,
        folders: manifest.counts.folders,
        messages: manifest.counts.messages,
        bytes: manifest.counts.bytes,
        warnings: manifest.warnings,
    })
}

fn read_limited(reader: &mut impl Read, limit: u64) -> Result<Vec<u8>, ArchiveError> {
    let mut bytes = Vec::new();
    reader.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(ArchiveError::Invalid("ZIP metadata exceeds size limit"));
    }
    Ok(bytes)
}

fn hash_limited(reader: &mut impl Read, limit: u64) -> Result<(u64, String), ArchiveError> {
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read as u64).ok_or(ArchiveError::Invalid("ZIP member size overflow"))?;
        if total > limit {
            return Err(ArchiveError::Invalid("ZIP member exceeds size limit"));
        }
        hash.update(&buffer[..read]);
    }
    Ok((total, hex_digest(&hash.finalize())))
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn parse_sums(bytes: &[u8]) -> Result<HashMap<&str, &str>, ArchiveError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ArchiveError::Invalid("SHA256SUMS is not UTF-8"))?;
    let mut sums = HashMap::new();
    for line in text.lines() {
        let Some((hash, name)) = line.split_once("  ") else {
            return Err(ArchiveError::Invalid("invalid SHA256SUMS line"));
        };
        if hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || name.is_empty()
            || sums.insert(name, hash).is_some()
        {
            return Err(ArchiveError::Invalid("invalid or duplicate SHA256SUMS entry"));
        }
    }
    Ok(sums)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use sha2::{Digest, Sha256};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::verify_archive;

    const MANIFEST: &[u8] = include_bytes!("../tests/fixtures/manifest-v1.json");
    const EML: &[u8] = include_bytes!("../tests/fixtures/sample.eml");
    const SAMPLE_ZIP: &[u8] = include_bytes!("../tests/fixtures/sample-v1.zip");

    fn archive_with_manifest(manifest: &[u8], eml: &[u8], extra: Option<(&str, &[u8])>) -> Vec<u8> {
        let readme = b"Torro Workplace mail backup v1\nThis ordinary ZIP is not encrypted.\nOpen mail/*.eml with an RFC 822 mail viewer; MIME attachments are inside each EML.\nmanifest.json describes folders, original dates, flags and completeness.\nSHA256SUMS lists the EMLs and the three metadata files, but not itself.\nA complete status records a checked source inventory, not an atomic IMAP snapshot.\n";
        let csv = b"folder_id,message_id,path,internal_date,bytes,sha256\nf000001,m000000001,mail/f000001/m000000001.eml,2026-10-20T09:00:00+00:00,57,775fb4368f2ec68c62a9bc2ce285f01f46ab8f66304d88bfd2e2c2825d0b7fb3\n";
        let files: [(&str, &[u8]); 4] = [
            ("README.txt", readme),
            ("manifest.json", manifest),
            ("messages.csv", csv),
            ("mail/f000001/m000000001.eml", eml),
        ];
        let mut sums = String::new();
        for (name, data) in files {
            sums.push_str(&format!("{}  {name}\n", super::hex_digest(&Sha256::digest(data))));
        }
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, data) in files {
            writer.start_file(name, options).expect("start member");
            writer.write_all(data).expect("member bytes");
        }
        writer.start_file("SHA256SUMS", options).expect("sum member");
        writer.write_all(sums.as_bytes()).expect("sums");
        if let Some((name, data)) = extra {
            writer.start_file(name, options).expect("extra member");
            writer.write_all(data).expect("extra bytes");
        }
        writer.finish().expect("finish").into_inner()
    }

    fn archive(eml: &[u8], extra: Option<(&str, &[u8])>) -> Vec<u8> {
        archive_with_manifest(MANIFEST, eml, extra)
    }

    #[test]
    fn ordinary_zip_with_eml_and_checksums_verifies() {
        let bytes = archive(EML, None);
        let report = verify_archive(Cursor::new(bytes)).expect("valid ZIP");
        assert!(report.complete);
        assert_eq!(report.source, super::Source::Server);
        assert!(report.since.is_none());
        assert_eq!(report.messages, 1);
        assert_eq!(report.bytes, 57);
    }

    #[test]
    fn checked_in_sample_zip_verifies() {
        let report = verify_archive(Cursor::new(SAMPLE_ZIP)).expect("standard ZIP fixture");
        assert_eq!(report.account, "anna@example.org");
        assert_eq!(report.messages, 1);
    }

    #[test]
    fn modified_eml_is_rejected_even_if_zip_crc_is_valid() {
        let bytes = archive(b"Different original message", None);
        assert!(verify_archive(Cursor::new(bytes)).is_err());
    }

    #[test]
    fn unexpected_or_traversing_member_is_rejected() {
        let bytes = archive(EML, Some(("../secrets.txt", b"bad")));
        assert!(verify_archive(Cursor::new(bytes)).is_err());
    }

    #[test]
    fn unknown_version_is_rejected_even_with_matching_checksums() {
        let changed = std::str::from_utf8(MANIFEST).expect("UTF-8 fixture").replace("\"version\": 1", "\"version\": 2");
        let bytes = archive_with_manifest(changed.as_bytes(), EML, None);
        assert!(verify_archive(Cursor::new(bytes)).is_err());
    }

    #[test]
    fn incomplete_status_is_reported_without_calling_it_complete() {
        let changed = std::str::from_utf8(MANIFEST)
            .expect("UTF-8 fixture")
            .replace("\"status\": \"complete\"", "\"status\": \"incomplete\"");
        let bytes = archive_with_manifest(changed.as_bytes(), EML, None);
        let report = verify_archive(Cursor::new(bytes)).expect("internally consistent archive");
        assert!(!report.complete);
    }
}
