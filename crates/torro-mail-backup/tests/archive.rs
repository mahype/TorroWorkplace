//! End-to-end tests for a readable mail backup and its offline integrity checks.

use std::io::{Cursor, Read, Write};
use std::process::Command;

use chrono::{TimeZone, Utc};
use sha2::Digest;
use torro_mail_backup::format::{Inventory, Manifest, Status};
use torro_mail_backup::verify_archive;
use torro_mail_backup::writer::{ArchiveWriter, MessageMetadata};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

fn example() -> Manifest {
    let mut manifest: Manifest =
        serde_json::from_str(include_str!("fixtures/manifest-v1.json")).expect("valid example manifest");
    manifest.status = Status::Incomplete;
    manifest.finished_at = None;
    manifest.messages.clear();
    manifest.counts.messages = 0;
    manifest.counts.bytes = 0;
    manifest.folders[0].inventory = None;
    manifest
}

fn archive() -> Vec<u8> {
    let mut writer = ArchiveWriter::new(Cursor::new(Vec::new()), example()).expect("open writer");
    let eml = include_bytes!("fixtures/sample.eml");
    writer
        .add_message(
            "f000001",
            Cursor::new(eml),
            MessageMetadata {
                uid: Some(7),
                uid_validity: Some(42),
                internal_date: None,
                flags: vec!["\\Seen".into()],
                expected_bytes: Some(eml.len() as u64),
            },
        )
        .expect("stream message");
    writer.manifest_mut().folders[0].inventory = Some(Inventory {
        checked_at: Utc.with_ymd_and_hms(2026, 10, 21, 10, 0, 1).single().expect("time"),
        eligible_messages: 1,
        uid_set_sha256: Some("1561ade0621c5acf44b780521f95a1e0b19b4e5032945b860c4032fc28a3a23b".into()),
    });
    let (cursor, report) = writer
        .finish(Status::Complete, Utc.with_ymd_and_hms(2026, 10, 21, 10, 0, 1).single().expect("time"))
        .expect("finalize and verify");
    assert!(report.complete);
    cursor.into_inner()
}

#[test]
fn generated_zip_has_plain_eml_and_human_readable_index() {
    let bytes = archive();
    let mut zip = ZipArchive::new(Cursor::new(&bytes)).expect("ordinary ZIP");
    let mut eml = Vec::new();
    zip.by_name("mail/f000001/m000000001.eml").expect("EML path").read_to_end(&mut eml).expect("EML bytes");
    assert_eq!(eml, include_bytes!("fixtures/sample.eml"));
    let mut csv = String::new();
    zip.by_name("messages.csv").expect("CSV").read_to_string(&mut csv).expect("CSV bytes");
    assert!(csv.starts_with("folder_id,message_id,path,internal_date,bytes,sha256\n"));
    assert!(csv.contains("f000001,m000000001,mail/f000001/m000000001.eml,,57,"));
    assert_eq!(verify_archive(Cursor::new(bytes)).expect("verified").messages, 1);
}

#[test]
fn generated_archive_opens_with_system_unzip_when_available() {
    if Command::new("unzip").arg("-v").output().is_err() {
        return;
    }
    let path = std::env::temp_dir().join(format!(
        "torro-mail-backup-{}-{}.zip",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_nanos()
    ));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).expect("temporary archive");
    file.write_all(&archive()).expect("archive bytes");
    drop(file);
    let check = Command::new("unzip").arg("-t").arg(&path).output().expect("unzip test");
    let eml =
        Command::new("unzip").arg("-p").arg(&path).arg("mail/f000001/m000000001.eml").output().expect("unzip EML");
    std::fs::remove_file(&path).expect("remove temporary archive");
    assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    assert!(eml.status.success());
    assert_eq!(eml.stdout, include_bytes!("fixtures/sample.eml"));
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
            data = b"folder_id,message_id,path,internal_date,bytes,sha256\n".to_vec();
        }
        if name == "SHA256SUMS" {
            let original = String::from_utf8(data).expect("sums UTF-8");
            let replacement = sha2::Sha256::digest(b"folder_id,message_id,path,internal_date,bytes,sha256\n")
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
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
    assert!(verify_archive(Cursor::new(bad)).is_err());
}

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

#[test]
fn large_message_is_read_in_bounded_chunks() {
    const SIZE: usize = 128 * 1024 * 1024;
    let mut writer = ArchiveWriter::new(Cursor::new(Vec::new()), example()).expect("writer");
    let mut reader = LargeReader { remaining: SIZE, largest_request: 0 };
    writer
        .add_message(
            "f000001",
            &mut reader,
            MessageMetadata {
                uid: Some(7),
                uid_validity: Some(42),
                expected_bytes: Some(SIZE as u64),
                ..Default::default()
            },
        )
        .expect("large EML");
    assert!(reader.largest_request <= 64 * 1024);
    writer.manifest_mut().folders[0].inventory = Some(Inventory {
        checked_at: Utc.with_ymd_and_hms(2026, 10, 21, 10, 0, 1).single().expect("time"),
        eligible_messages: 1,
        uid_set_sha256: Some("1561ade0621c5acf44b780521f95a1e0b19b4e5032945b860c4032fc28a3a23b".into()),
    });
    let (_, report) = writer
        .finish(Status::Complete, Utc.with_ymd_and_hms(2026, 10, 21, 10, 0, 1).single().expect("time"))
        .expect("final archive");
    assert_eq!(report.bytes, SIZE as u64);
}

#[test]
fn short_source_read_poisoned_writer_cannot_publish_complete() {
    let mut writer = ArchiveWriter::new(Cursor::new(Vec::new()), example()).expect("writer");
    let result = writer.add_message(
        "f000001",
        Cursor::new(b"short"),
        MessageMetadata { uid: Some(7), uid_validity: Some(42), expected_bytes: Some(100), ..Default::default() },
    );
    assert!(result.is_err());
    assert!(writer.finish(Status::Complete, Utc::now()).is_err());
}
