//! A verified, bounded reader for restoring individual RFC-822 originals.

use std::io::{Read, Seek, Write};

use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::format::{MAX_MESSAGE_BYTES, Manifest};
use crate::verify::verify_into;
use crate::{ArchiveError, hex_digest};

/// A ZIP that passed the full offline verifier before any EML is exposed for restore.
///
/// It keeps the reader and the manifest the verifier checked, so nothing is opened or parsed a
/// second time between the check and the restore.
pub struct RestoreArchive<R: Read + Seek> {
    zip: ZipArchive<R>,
    manifest: Manifest,
    manifest_sha256: String,
}

impl<R: Read + Seek> RestoreArchive<R> {
    /// Verifies the whole archive (see `verify_archive`) and keeps it open for `copy_message`.
    pub fn open(input: R) -> Result<Self, ArchiveError> {
        let verified = verify_into(input)?;
        Ok(Self { zip: verified.zip, manifest: verified.manifest, manifest_sha256: verified.manifest_sha256 })
    }

    /// Lowercase hex SHA-256 of the raw `manifest.json` bytes as they are stored in the ZIP (and
    /// listed in `SHA256SUMS`). It names this archive for a restore journal: unlike a hash of the
    /// parsed manifest written out again, it does not change when a later version of this crate
    /// serializes the same manifest differently.
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    /// The verified manifest. An alias has no EML of its own: restore its bytes by copying the
    /// message whose `path` equals the alias's `same_as`.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Extract exactly one declared message into `output`, checking its hash again during the
    /// copy. Callers can spool to a private file; no whole EML is held in memory by this reader.
    pub fn copy_message(&mut self, index: usize, output: &mut impl Write) -> Result<u64, ArchiveError> {
        let message = self.manifest.messages.get(index).ok_or(ArchiveError::Invalid("unknown archive message"))?;
        if message.bytes > MAX_MESSAGE_BYTES {
            return Err(ArchiveError::Invalid("archive message is too large"));
        }
        let mut file = self.zip.by_name(&message.path)?;
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let read = file.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            bytes = bytes.checked_add(read as u64).ok_or(ArchiveError::Invalid("archive size overflow"))?;
            if bytes > message.bytes {
                return Err(ArchiveError::Invalid("archive message exceeded declared size"));
            }
            output.write_all(&chunk[..read])?;
            hash.update(&chunk[..read]);
        }
        if bytes != message.bytes || hex_digest(&hash.finalize()) != message.sha256 {
            return Err(ArchiveError::Invalid("archive message changed during restore"));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const SAMPLE: &[u8] = include_bytes!("../tests/fixtures/sample-v1.zip");

    #[test]
    fn verified_reader_copies_the_original_and_rejects_unknown_indices() {
        let mut archive = RestoreArchive::open(Cursor::new(SAMPLE)).expect("sample verifies");
        assert!(!archive.manifest().messages.is_empty());
        let mut raw = Vec::new();
        let size = archive.copy_message(0, &mut raw).expect("copied");
        assert_eq!(raw.len() as u64, size);
        assert!(raw.starts_with(b"From:"));
        assert!(matches!(
            archive.copy_message(usize::MAX, &mut Vec::new()),
            Err(ArchiveError::Invalid("unknown archive message"))
        ));
    }

    #[test]
    fn the_manifest_digest_is_the_one_of_the_raw_manifest_bytes() {
        let archive = RestoreArchive::open(Cursor::new(SAMPLE)).expect("sample verifies");
        let mut zip = ZipArchive::new(Cursor::new(SAMPLE)).expect("ZIP");
        let mut raw = Vec::new();
        zip.by_name("manifest.json").expect("manifest").read_to_end(&mut raw).expect("read");
        assert_eq!(archive.manifest_sha256(), hex_digest(&Sha256::digest(&raw)));
        let reencoded = serde_json::to_vec_pretty(archive.manifest()).expect("encoded");
        assert_ne!(reencoded, raw, "the digest never depends on serializing the manifest again");
    }

    #[test]
    fn damaged_archive_cannot_be_opened_for_restore() {
        assert!(matches!(RestoreArchive::open(Cursor::new(b"not a ZIP")), Err(ArchiveError::Zip(_))));
    }
}
