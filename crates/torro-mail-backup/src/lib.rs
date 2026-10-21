//! Portable, versioned mail backup archives shared by the daemon's backup and restore paths.

pub mod format;
pub mod restore;
mod verify;
pub mod writer;

pub use verify::verify_archive;

pub(crate) const MAX_MANIFEST_BYTES: u64 = 256 * 1024 * 1024;
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

pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
