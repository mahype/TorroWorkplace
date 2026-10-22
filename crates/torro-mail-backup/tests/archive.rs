//! End-to-end tests for a readable mail backup and its offline integrity checks.

use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::process::Command;

use chrono::{TimeZone, Utc};
use sha2::Digest;
use torro_mail_backup::format::{Inventory, Manifest, Status};
use torro_mail_backup::writer::{ArchiveWriter, MessageMetadata};
use torro_mail_backup::{ArchiveError, verify_archive};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const EML: &[u8] = include_bytes!("fixtures/sample.eml");
const EML_PATH: &str = "mail/f000001/m000000001.eml";
const CSV_HEADER: &str = "folder_id,message_id,path,internal_date,bytes,sha256\n";
const INBOX_UIDS: &str = "1561ade0621c5acf44b780521f95a1e0b19b4e5032945b860c4032fc28a3a23b";
const ALL_MAIL_UIDS: &str = "e084a105a7b7d599b8346d3cba5ac51b756d7e29c7ef77fd9eff31fdeae31389";

fn fixture(json: &str) -> Manifest {
    serde_json::from_str(json).expect("valid fixture manifest")
}

/// The fixture's account and folders as an unfinished manifest, ready for a writer.
fn empty(mut manifest: Manifest) -> Manifest {
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

fn finished() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 21, 10, 0, 1).single().expect("time")
}

fn inventory(count: u64, uids: &str) -> Option<Inventory> {
    Some(Inventory { checked_at: finished(), eligible_messages: count, uid_set_sha256: Some(uids.into()) })
}

/// The metadata of the fixture's single message, so the archive reproduces `manifest-v1.json`.
fn sample_metadata(template: &Manifest) -> MessageMetadata {
    let message = &template.messages[0];
    MessageMetadata {
        uid: message.uid,
        uid_validity: message.uid_validity,
        internal_date: message.internal_date,
        flags: message.flags.clone(),
        expected_bytes: Some(EML.len() as u64),
    }
}

fn archive() -> Vec<u8> {
    let template = fixture(include_str!("fixtures/manifest-v1.json"));
    let mut writer = ArchiveWriter::new(Cursor::new(Vec::new()), empty(template.clone())).expect("open writer");
    writer.add_message("f000001", Cursor::new(EML), sample_metadata(&template)).expect("stream message");
    writer.manifest_mut().folders[0].inventory = inventory(1, INBOX_UIDS);
    let (cursor, report) = writer.finish(Status::Complete, finished()).expect("finalize and verify");
    assert!(report.complete);
    cursor.into_inner()
}

fn read_member(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("ordinary ZIP");
    let mut data = Vec::new();
    zip.by_name(name).expect("member").read_to_end(&mut data).expect("member bytes");
    data
}

#[test]
fn generated_zip_has_plain_eml_and_human_readable_index() {
    let bytes = archive();
    assert_eq!(read_member(&bytes, EML_PATH), EML);
    let csv = String::from_utf8(read_member(&bytes, "messages.csv")).expect("CSV text");
    assert!(csv.starts_with(CSV_HEADER));
    assert!(csv.contains("f000001,m000000001,mail/f000001/m000000001.eml,2026-10-20T09:00:00+00:00,57,"));
    let manifest = read_member(&bytes, "manifest.json");
    assert!(!manifest.windows(2).any(|pair| pair == b"\n "), "the manifest is written compactly");
    let parsed: Manifest = serde_json::from_slice(&manifest).expect("manifest JSON");
    assert_eq!(parsed, fixture(include_str!("fixtures/manifest-v1.json")), "same content as the documented fixture");
    assert_eq!(verify_archive(Cursor::new(bytes)).expect("verified").messages, 1);
}

#[test]
fn entry_dates_come_from_the_archive_not_from_the_clock() {
    let bytes = archive();
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("ordinary ZIP");
    let date = |zip: &mut ZipArchive<Cursor<Vec<u8>>>, name: &str| {
        let time = zip.by_name(name).expect("member").last_modified().expect("date");
        (time.year(), time.month(), time.day(), time.hour(), time.minute())
    };
    assert_eq!(date(&mut zip, EML_PATH), (2026, 10, 20, 9, 0), "an EML carries its INTERNALDATE");
    assert_eq!(date(&mut zip, "manifest.json"), (2026, 10, 21, 10, 0), "metadata carries the finish time");
}

#[test]
fn entries_are_regular_unix_files_whatever_system_writes_them() {
    let mut zip = ZipArchive::new(Cursor::new(archive())).expect("ordinary ZIP");
    for index in 0..zip.len() {
        let file = zip.by_index(index).expect("member");
        assert_eq!(file.unix_mode(), Some(0o100_644), "entry {index} is a regular file with mode 0644");
    }
    // The verifier still accepts these entries as plain files.
    assert_eq!(verify_archive(Cursor::new(archive())).expect("verified").messages, 1);
}

#[test]
fn checked_in_sample_is_what_the_writer_produces() {
    let generated = archive();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample-v1.zip");
    if std::env::var_os("TORRO_BLESS").is_some_and(|value| value == "1") {
        std::fs::write(path, &generated).expect("write sample fixture");
    }
    assert_eq!(std::fs::read(path).expect("sample fixture"), generated, "regenerate with TORRO_BLESS=1");
}

fn system_unzip(bytes: &[u8], member: &str) -> Option<(std::process::Output, std::process::Output)> {
    Command::new("unzip").arg("-v").output().ok()?;
    let mut file = tempfile::NamedTempFile::new().expect("temporary archive");
    file.write_all(bytes).expect("archive bytes");
    let check = Command::new("unzip").arg("-t").arg(file.path()).output().expect("unzip test");
    let eml = Command::new("unzip").arg("-p").arg(file.path()).arg(member).output().expect("unzip EML");
    Some((check, eml))
}

#[test]
fn generated_archive_opens_with_system_unzip_when_available() {
    let Some((check, eml)) = system_unzip(&archive(), EML_PATH) else {
        return;
    };
    assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    assert!(eml.status.success());
    assert_eq!(eml.stdout, EML);
}

#[test]
fn collection_folder_alias_reuses_the_eml_and_verifies() {
    let template = fixture(include_str!("fixtures/manifest-v1-alias.json"));
    let mut writer = ArchiveWriter::new(Cursor::new(Vec::new()), empty(template.clone())).expect("writer");
    writer.add_message("f000001", Cursor::new(EML), sample_metadata(&template)).expect("message");
    let sha256 = hex(&sha2::Sha256::digest(EML));
    let same_as = writer.archived_path("f000002", &sha256, EML.len() as u64).expect("found in INBOX").to_string();
    let alias = &template.aliases[0];
    let metadata = MessageMetadata {
        uid: alias.uid,
        uid_validity: alias.uid_validity,
        internal_date: alias.internal_date,
        flags: alias.flags.clone(),
        expected_bytes: Some(EML.len() as u64),
    };
    writer.add_alias("f000002", &same_as, metadata).expect("alias");
    writer.manifest_mut().folders[0].inventory = inventory(1, INBOX_UIDS);
    writer.manifest_mut().folders[1].inventory = inventory(1, ALL_MAIL_UIDS);
    let (cursor, report) = writer.finish(Status::Complete, finished()).expect("verified alias archive");
    assert_eq!((report.messages, report.aliases, report.bytes), (1, 1, EML.len() as u64));
    let bytes = cursor.into_inner();
    let parsed: Manifest = serde_json::from_slice(&read_member(&bytes, "manifest.json")).expect("manifest");
    assert_eq!(parsed, template, "same content as the documented alias fixture");
    let zip = ZipArchive::new(Cursor::new(bytes.as_slice())).expect("ZIP");
    assert_eq!(zip.len(), 5, "no second EML for the alias");
    let csv = String::from_utf8(read_member(&bytes, "messages.csv")).expect("CSV");
    assert_eq!(csv.lines().count(), 2, "messages.csv lists EML files only");
    if let Some((check, eml)) = system_unzip(&bytes, EML_PATH) {
        assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
        assert_eq!(eml.stdout, EML);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn changed_csv_with_recomputed_checksum_is_rejected() {
    let bytes = archive();
    let mut source = ZipArchive::new(Cursor::new(bytes)).expect("ZIP");
    let mut destination = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for i in 0..source.len() {
        let mut member = source.by_index(i).expect("member");
        let name = member.name().expect("name").to_string();
        let mut data = Vec::new();
        member.read_to_end(&mut data).expect("data");
        if name == "messages.csv" {
            data = CSV_HEADER.as_bytes().to_vec();
        }
        if name == "SHA256SUMS" {
            let original = String::from_utf8(data).expect("sums UTF-8");
            let replacement = hex(&sha2::Sha256::digest(CSV_HEADER.as_bytes()));
            data = original
                .lines()
                .map(|line| {
                    if line.ends_with("  messages.csv") {
                        format!("{replacement}  messages.csv\n")
                    } else {
                        format!("{line}\n")
                    }
                })
                .collect::<String>()
                .into_bytes();
        }
        destination.start_file(name, options).expect("member start");
        destination.write_all(&data).expect("member write");
    }
    let bad = destination.finish().expect("ZIP finish").into_inner();
    let error = verify_archive(Cursor::new(bad)).expect_err("CSV no longer matches");
    assert!(
        matches!(error, ArchiveError::Invalid("messages.csv has extra rows" | "messages.csv is truncated")),
        "{error}"
    );
}

/// A source that produces `remaining` bytes and remembers the largest buffer it was asked to fill.
struct LargeReader {
    remaining: usize,
    largest_request: usize,
}

impl Read for LargeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.largest_request = self.largest_request.max(buf.len());
        let count = self.remaining.min(buf.len());
        for (index, byte) in buf[..count].iter_mut().enumerate() {
            *byte = ((self.remaining + index) % 251) as u8;
        }
        self.remaining -= count;
        Ok(count)
    }
}

/// A file that remembers the largest single read and write the archive code asked of it.
struct TrackedFile {
    file: std::fs::File,
    largest_read: usize,
    largest_write: usize,
}

impl Read for TrackedFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.largest_read = self.largest_read.max(buf.len());
        self.file.read(buf)
    }
}

impl Write for TrackedFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.largest_write = self.largest_write.max(buf.len());
        self.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Seek for TrackedFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

#[test]
fn large_message_streams_through_a_file_with_bounded_buffers() {
    const SIZE: usize = 64 * 1024 * 1024;
    const BOUND: usize = 1024 * 1024;
    let template = fixture(include_str!("fixtures/manifest-v1.json"));
    let output = TrackedFile { file: tempfile::tempfile().expect("temporary file"), largest_read: 0, largest_write: 0 };
    let mut writer = ArchiveWriter::new(output, empty(template)).expect("writer");
    let mut reader = LargeReader { remaining: SIZE, largest_request: 0 };
    let metadata = MessageMetadata {
        uid: Some(7),
        uid_validity: Some(42),
        expected_bytes: Some(SIZE as u64),
        ..Default::default()
    };
    writer.add_message("f000001", &mut reader, metadata).expect("large EML");
    assert!(reader.largest_request <= 64 * 1024, "source read in 64 KiB chunks");
    writer.manifest_mut().folders[0].inventory = inventory(1, INBOX_UIDS);
    let (output, report) = writer.finish(Status::Complete, finished()).expect("final archive");
    assert_eq!(report.bytes, SIZE as u64);
    assert!(output.file.metadata().expect("size").len() < SIZE as u64, "written to the file, compressed");
    assert!(output.largest_write <= BOUND, "largest write {} bytes", output.largest_write);
    assert!(output.largest_read <= BOUND, "largest verifier read {} bytes", output.largest_read);
}

#[test]
fn short_source_read_poisoned_writer_cannot_publish_complete() {
    let template = fixture(include_str!("fixtures/manifest-v1.json"));
    let mut writer = ArchiveWriter::new(Cursor::new(Vec::new()), empty(template)).expect("writer");
    let result = writer.add_message(
        "f000001",
        Cursor::new(b"short"),
        MessageMetadata { uid: Some(7), uid_validity: Some(42), expected_bytes: Some(100), ..Default::default() },
    );
    assert!(matches!(result, Err(ArchiveError::Invalid("original message length differs from source"))));
    assert!(matches!(
        writer.finish(Status::Complete, Utc::now()),
        Err(ArchiveError::Invalid("archive writer was interrupted"))
    ));
}
