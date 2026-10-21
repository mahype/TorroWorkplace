//! Stream original EML bytes into a portable ZIP and verify it before returning success.

use std::io::{Read, Seek, SeekFrom, Write};

use chrono::{DateTime, FixedOffset, Utc};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::format::{ArchivePath, MAX_MESSAGE_BYTES, MAX_MESSAGES, Manifest, Message, Status};
use crate::{ArchiveError, MAX_INDEX_BYTES, MAX_MANIFEST_BYTES, VerifiedArchive, hex_digest, verify_archive};

const CHUNK_BYTES: usize = 64 * 1024;

pub(crate) const README: &str = "Torro Workplace mail backup v1\nThis ordinary ZIP is not encrypted.\nOpen mail/*.eml with an RFC 822 mail viewer; MIME attachments are inside each EML.\nmanifest.json describes folders, original dates, flags and completeness.\nSHA256SUMS lists the EMLs and the three metadata files, but not itself.\nA complete status records a checked source inventory, not an atomic IMAP snapshot.\n";

/// Metadata supplied by the original-message source; the writer supplies generated ids and hashes.
#[derive(Debug, Clone, Default)]
pub struct MessageMetadata {
    pub uid: Option<u32>,
    pub uid_validity: Option<u32>,
    pub internal_date: Option<DateTime<FixedOffset>>,
    pub flags: Vec<String>,
    /// A known IMAP literal length or local file length; a short read poisons the writer.
    pub expected_bytes: Option<u64>,
}

/// A one-use writer. `finish` is the only route to a verified, complete archive.
pub struct ArchiveWriter<W: Read + Write + Seek> {
    zip: ZipWriter<W>,
    manifest: Manifest,
    sums: Vec<(String, String)>,
    poisoned: bool,
}

impl<W: Read + Write + Seek> ArchiveWriter<W> {
    /// Start from an incomplete manifest with its account and folder table already set.
    pub fn new(output: W, manifest: Manifest) -> Result<Self, ArchiveError> {
        if manifest.status != Status::Incomplete || manifest.finished_at.is_some() || !manifest.messages.is_empty() {
            return Err(ArchiveError::Invalid("archive writer needs an unfinished, empty manifest"));
        }
        manifest.validate()?;
        Ok(Self { zip: ZipWriter::new(output).set_auto_large_file(), manifest, sums: Vec::new(), poisoned: false })
    }

    /// After fetching, sources set final folder inventories here before `finish`.
    pub fn manifest_mut(&mut self) -> &mut Manifest {
        &mut self.manifest
    }

    /// Copy one RFC-822 original with bounded memory, preserving its bytes exactly.
    pub fn add_message<R: Read>(
        &mut self,
        folder_id: &str,
        mut input: R,
        metadata: MessageMetadata,
    ) -> Result<(), ArchiveError> {
        if self.poisoned {
            return Err(ArchiveError::Invalid("archive writer was interrupted"));
        }
        let Some(folder) = self.manifest.folders.iter().find(|folder| folder.id == folder_id && folder.selectable)
        else {
            return Err(ArchiveError::Invalid("message folder is unknown or not selectable"));
        };
        if self.manifest.messages.len() >= MAX_MESSAGES {
            return Err(ArchiveError::Invalid("too many messages for archive"));
        }
        let number = self.manifest.messages.len() as u64 + 1;
        let folder_number: u32 = folder.id[1..].parse().map_err(|_| ArchiveError::Invalid("invalid folder id"))?;
        let path = ArchivePath::message(folder_number, number)?.as_str().to_string();
        self.poisoned = true;
        self.zip.start_file(&path, options())?;
        let mut digest = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0u8; CHUNK_BYTES];
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            bytes = bytes.checked_add(read as u64).ok_or(ArchiveError::Invalid("message length overflow"))?;
            if bytes > MAX_MESSAGE_BYTES || self.manifest.counts.bytes.saturating_add(bytes) > MAX_MESSAGE_BYTES {
                return Err(ArchiveError::Invalid("message archive size limit exceeded"));
            }
            self.zip.write_all(&buffer[..read])?;
            digest.update(&buffer[..read]);
        }
        if metadata.expected_bytes.is_some_and(|expected| expected != bytes) {
            return Err(ArchiveError::Invalid("original message length differs from source"));
        }
        let hash = hex_digest(&digest.finalize());
        self.sums.push((path.clone(), hash.clone()));
        self.manifest.messages.push(Message {
            id: format!("m{number:09}"),
            folder_id: folder_id.to_string(),
            path,
            uid: metadata.uid,
            uid_validity: metadata.uid_validity,
            internal_date: metadata.internal_date,
            flags: metadata.flags,
            bytes,
            sha256: hash,
        });
        self.manifest.counts.messages += 1;
        self.manifest.counts.bytes += bytes;
        self.poisoned = false;
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

        let readme_hash = self.write_bytes("README.txt", README.as_bytes())?;
        self.sums.push(("README.txt".into(), readme_hash));
        self.zip.start_file("manifest.json", options())?;
        let manifest_hash = {
            let mut hashing = HashingWriter::new(&mut self.zip, MAX_MANIFEST_BYTES);
            serde_json::to_writer_pretty(&mut hashing, &self.manifest)?;
            hashing.write_all(b"\n")?;
            hashing.finish()
        };
        self.sums.push(("manifest.json".into(), manifest_hash));

        self.zip.start_file("messages.csv", options())?;
        let csv_hash = {
            let mut hashing = HashingWriter::new(&mut self.zip, MAX_INDEX_BYTES);
            hashing.write_all(crate::verify::CSV_HEADER.as_bytes())?;
            for message in &self.manifest.messages {
                hashing.write_all(crate::verify::csv_row(message).as_bytes())?;
            }
            hashing.finish()
        };
        self.sums.push(("messages.csv".into(), csv_hash));

        self.zip.start_file("SHA256SUMS", options())?;
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

    fn write_bytes(&mut self, path: &str, data: &[u8]) -> Result<String, ArchiveError> {
        self.zip.start_file(path, options())?;
        self.zip.write_all(data)?;
        Ok(hex_digest(&Sha256::digest(data)))
    }
}

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
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
