//! Portable, versioned mail backup archives shared by the daemon's backup and restore paths.

pub mod format;
pub mod restore;
mod verify;
pub mod writer;

pub use verify::{archive_account, verify_archive};

/// Ceiling for `manifest.json`; `format::MAX_MESSAGES` typical entries fit (a test checks it).
pub(crate) const MAX_MANIFEST_BYTES: u64 = 512 * 1024 * 1024;
/// Ceiling for `messages.csv` and `SHA256SUMS`.
pub(crate) const MAX_INDEX_BYTES: u64 = 512 * 1024 * 1024;

use std::fmt;

use chrono::{DateTime, NaiveDate, Utc};

use crate::format::{FormatError, Source};

/// A malformed, oversized or damaged mail archive.
#[derive(Debug)]
pub enum ArchiveError {
    Zip(zip::result::ZipError),
    Io(std::io::Error),
    Json(serde_json::Error),
    Format(FormatError),
    /// A `torro-mail-backup` manifest of a version this crate cannot read, e.g. from a newer Torro.
    UnsupportedVersion(u32),
    Invalid(&'static str),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zip(err) => write!(f, "ZIP: {err}"),
            Self::Io(err) => write!(f, "I/O: {err}"),
            Self::Json(err) => write!(f, "manifest JSON: {err}"),
            Self::Format(err) => write!(f, "manifest: {err}"),
            Self::UnsupportedVersion(version) => write!(f, "unsupported mail backup version {version}"),
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
    /// EML files in the archive.
    pub messages: u64,
    /// Collection-folder occurrences that reuse an EML (`Manifest::aliases`).
    pub aliases: u64,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

/// `bytes` as lowercase hex, as SHA-256 sums are written in archives and their checks.
pub fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}
