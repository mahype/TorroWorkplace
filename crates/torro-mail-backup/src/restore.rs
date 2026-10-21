//! A verified, bounded reader for restoring individual RFC-822 originals.

use std::io::{Read, Seek, SeekFrom, Write};

use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::format::{MAX_MESSAGE_BYTES, Manifest};
use crate::{ArchiveError, MAX_MANIFEST_BYTES, hex_digest, verify_archive};

/// A ZIP that passed the full offline verifier before any EML is exposed for restore.
pub struct RestoreArchive<R: Read + Seek> {
    zip: ZipArchive<R>,
    manifest: Manifest,
}

impl<R: Read + Seek> RestoreArchive<R> {
    pub fn open(mut input: R) -> Result<Self, ArchiveError> {
        verify_archive(&mut input)?;
        input.seek(SeekFrom::Start(0))?;
        let mut zip = ZipArchive::new(input)?;
        let manifest = {
            let mut file = zip.by_name("manifest.json")?;
            if file.size() > MAX_MANIFEST_BYTES {
                return Err(ArchiveError::Invalid("manifest is too large"));
            }
            let mut bytes = Vec::new();
            file.by_ref().take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_MANIFEST_BYTES {
                return Err(ArchiveError::Invalid("manifest is too large"));
            }
            serde_json::from_slice::<Manifest>(&bytes)?
        };
        manifest.validate()?;
        Ok(Self { zip, manifest })
    }

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
        assert!(archive.copy_message(usize::MAX, &mut Vec::new()).is_err());
    }

    #[test]
    fn damaged_archive_cannot_be_opened_for_restore() {
        assert!(RestoreArchive::open(Cursor::new(b"not a ZIP")).is_err());
    }
}
