//! Offline checks for every member of a portable mail archive.
//!
//! The checks run in this order, so a damaged archive is reported by its first real fault:
//! ZIP structure (no prefix, no duplicate names, local headers equal to the central directory),
//! manifest version, manifest contract, `SHA256SUMS` against the manifest, then every member's
//! type, size and hash (with the fixed `README.txt` text), and finally `messages.csv`.

use std::collections::HashMap;
use std::io::{BufReader, Read, Seek};

use sha2::{Digest, Sha256};
use zip::ZipArchive;
use zip::read::ZipFile;
use zip::result::ZipError;

use crate::format::{Head, MAX_MESSAGE_BYTES, MAX_MESSAGES, Manifest, Status, parse_manifest};
use crate::writer::README;
use crate::{ArchiveError, MAX_INDEX_BYTES, MAX_MANIFEST_BYTES, VerifiedArchive, hex_digest};

pub(crate) const CSV_HEADER: &str = "folder_id,message_id,path,internal_date,bytes,sha256\n";

/// Metadata that is parsed into memory may expand at most this much over its compressed bytes.
/// Hex digests keep real manifests and checksum lists far below it; a deflate bomb of repeated
/// tiny strings is far above.
const METADATA_RATIO: u64 = 64;
/// Small metadata is accepted whatever its ratio.
const METADATA_FLOOR: u64 = 1024 * 1024;
/// EMLs are only hashed while streaming, so they get the looser general expansion check.
const EML_RATIO: u64 = 10_000;

pub(crate) fn csv_row(message: &crate::format::Message) -> String {
    let date = message.internal_date.map(|date| date.to_rfc3339()).unwrap_or_default();
    format!("{},{},{},{},{},{}\n", message.folder_id, message.id, message.path, date, message.bytes, message.sha256)
}

/// A verified archive with the reader and manifest that were checked, so a restore uses exactly
/// what the verifier saw instead of opening and parsing the file again.
pub(crate) struct Verified<R> {
    pub zip: ZipArchive<R>,
    pub manifest: Manifest,
    pub report: VerifiedArchive,
    /// Lowercase hex SHA-256 of the raw `manifest.json` bytes in the ZIP, as checked against
    /// `SHA256SUMS`.
    pub manifest_sha256: String,
}

/// Verifies a standard ZIP without extracting it or opening an IMAP connection.
///
/// Only the four documented metadata names and generated EML names are accepted, all as regular
/// files. Size and expansion limits apply while reading, so forged ZIP headers cannot bypass them.
pub fn verify_archive<R: Read + Seek>(input: R) -> Result<VerifiedArchive, ArchiveError> {
    verify_into(input).map(|verified| verified.report)
}

/// What a single entry of the ZIP must be.
enum Expected<'a> {
    Readme,
    Manifest,
    Csv,
    Message { bytes: u64, sha256: &'a str },
}

pub(crate) fn verify_into<R: Read + Seek>(input: R) -> Result<Verified<R>, ArchiveError> {
    let mut zip = ZipArchive::new(input)?;
    if zip.offset() != 0 {
        return Err(ArchiveError::Invalid("ZIP has data before its first member"));
    }
    zip.validate_header_consistency().map_err(|error| match error {
        ZipError::InvalidArchive(message) if message.contains("duplicate") => {
            ArchiveError::Invalid("duplicate ZIP member")
        }
        ZipError::Io(error) => ArchiveError::Io(error),
        _ => ArchiveError::Invalid("ZIP local headers differ from the central directory"),
    })?;
    if zip.len() > MAX_MESSAGES.saturating_add(4) {
        return Err(ArchiveError::Invalid("too many ZIP members"));
    }
    read_metadata(&mut zip, "manifest.json", MAX_MANIFEST_BYTES, |reader| serde_json::from_reader::<_, Head>(reader))?
        .check()?;
    let manifest = read_metadata(&mut zip, "manifest.json", MAX_MANIFEST_BYTES, |reader| parse_manifest(reader))?;
    manifest.validate()?;
    let sums_bytes = read_metadata(&mut zip, "SHA256SUMS", MAX_INDEX_BYTES, |reader| {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map(|_| bytes)
    })?;
    let sums = parse_sums(&sums_bytes)?;

    let mut expected: HashMap<&str, Expected<'_>> = HashMap::with_capacity(manifest.messages.len() + 3);
    expected.insert("README.txt", Expected::Readme);
    expected.insert("manifest.json", Expected::Manifest);
    expected.insert("messages.csv", Expected::Csv);
    for message in &manifest.messages {
        expected.insert(message.path.as_str(), Expected::Message { bytes: message.bytes, sha256: &message.sha256 });
    }
    if sums.keys().any(|name| !expected.contains_key(name)) {
        return Err(ArchiveError::Invalid("SHA256SUMS lists an unexpected file"));
    }
    if sums.len() != expected.len() {
        return Err(ArchiveError::Invalid("SHA256SUMS is missing a line"));
    }
    let readme_hash = hex_digest(&Sha256::digest(README.as_bytes()));
    let mut matched = 0usize;
    let mut manifest_sha256 = None;
    for index in 0..zip.len() {
        let mut file = zip.by_index(index)?;
        if !regular_file(&file) {
            return Err(ArchiveError::Invalid("ZIP member is not a regular file"));
        }
        let name = file.name()?.into_owned();
        if name == "SHA256SUMS" {
            continue;
        }
        let Some(what) = expected.get(name.as_str()) else {
            return Err(ArchiveError::Invalid("unexpected or unsafe ZIP member"));
        };
        let Some(listed_hash) = sums.get(name.as_str()) else {
            return Err(ArchiveError::Invalid("ZIP member has no checksum"));
        };
        let (ceiling, ratio) = match what {
            Expected::Readme => (README.len() as u64, EML_RATIO),
            Expected::Manifest => (MAX_MANIFEST_BYTES, METADATA_RATIO),
            Expected::Csv => (MAX_INDEX_BYTES, EML_RATIO),
            Expected::Message { bytes, .. } => (*bytes, EML_RATIO),
        };
        check_expansion(file.size(), file.compressed_size(), ceiling.min(MAX_MESSAGE_BYTES), ratio)?;
        let (actual_bytes, actual_hash) = hash_limited(&mut file, ceiling)?;
        if actual_hash != *listed_hash {
            return Err(ArchiveError::Invalid("ZIP member SHA-256 mismatch"));
        }
        match what {
            Expected::Readme if actual_hash != readme_hash => {
                return Err(ArchiveError::Invalid("README.txt differs from the version-one text"));
            }
            Expected::Message { bytes, sha256 } if actual_bytes != *bytes || actual_hash != *sha256 => {
                return Err(ArchiveError::Invalid("EML differs from manifest"));
            }
            Expected::Manifest => manifest_sha256 = Some(actual_hash),
            _ => {}
        }
        matched += 1;
    }
    if matched != expected.len() {
        return Err(ArchiveError::Invalid("ZIP member is missing"));
    }
    check_csv(&mut zip, &manifest)?;
    let manifest_sha256 = manifest_sha256.ok_or(ArchiveError::Invalid("ZIP member is missing"))?;
    let report = VerifiedArchive {
        account: manifest.account.address.clone(),
        source: manifest.source,
        since: manifest.since,
        started_at: manifest.started_at,
        finished_at: manifest.finished_at,
        complete: manifest.status == Status::Complete,
        folders: manifest.counts.folders,
        messages: manifest.counts.messages,
        aliases: manifest.aliases.len() as u64,
        bytes: manifest.counts.bytes,
        warnings: manifest.warnings.clone(),
    };
    Ok(Verified { zip, manifest, report, manifest_sha256 })
}

/// A plain file: no directory, no symbolic link, no device, socket or pipe. A Unix mode without
/// type bits comes from writers that only record permissions.
fn regular_file<R: Read>(file: &ZipFile<'_, R>) -> bool {
    const TYPE_BITS: u32 = 0o170_000;
    const REGULAR: u32 = 0o100_000;
    file.is_file() && file.unix_mode().is_none_or(|mode| matches!(mode & TYPE_BITS, 0 | REGULAR))
}

/// Parses one metadata member through a reader that stops at the size and expansion limits.
/// The ZIP reader only consumes the member's compressed bytes, so a limit derived from
/// `compressed_size` also holds for a forged uncompressed size.
fn read_metadata<R: Read + Seek, T, E: Into<ArchiveError>>(
    zip: &mut ZipArchive<R>,
    name: &str,
    ceiling: u64,
    parse: impl FnOnce(&mut dyn Read) -> Result<T, E>,
) -> Result<T, ArchiveError> {
    let mut file = zip.by_name(name)?;
    if !regular_file(&file) {
        return Err(ArchiveError::Invalid("ZIP member is not a regular file"));
    }
    check_expansion(file.size(), file.compressed_size(), ceiling, METADATA_RATIO)?;
    let limit = ceiling.min(file.compressed_size().saturating_mul(METADATA_RATIO).max(METADATA_FLOOR));
    let mut reader = BufReader::new(Limited { inner: &mut file, left: limit, exceeded: false });
    let parsed = parse(&mut reader);
    if reader.get_ref().exceeded {
        return Err(ArchiveError::Invalid("ZIP metadata exceeds size or expansion limit"));
    }
    parsed.map_err(Into::into)
}

/// A reader that fails instead of returning more than `left` bytes.
struct Limited<R> {
    inner: R,
    left: u64,
    exceeded: bool,
}

impl<R: Read> Read for Limited<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            // Probe one byte: only a stream that really continues is over the limit.
            let mut probe = [0u8; 1];
            if self.inner.read(&mut probe)? == 0 {
                return Ok(0);
            }
            self.exceeded = true;
            return Err(std::io::Error::other("ZIP metadata exceeds size limit"));
        }
        let room = usize::try_from(self.left).unwrap_or(usize::MAX).min(buf.len());
        let read = self.inner.read(&mut buf[..room])?;
        self.left -= read as u64;
        Ok(read)
    }
}

fn check_expansion(size: u64, compressed: u64, ceiling: u64, ratio: u64) -> Result<(), ArchiveError> {
    if size > ceiling || size > compressed.saturating_mul(ratio).max(METADATA_FLOOR) {
        return Err(ArchiveError::Invalid("ZIP member exceeds size or expansion limit"));
    }
    Ok(())
}

fn check_csv<R: Read + Seek>(zip: &mut ZipArchive<R>, manifest: &Manifest) -> Result<(), ArchiveError> {
    let mut csv = zip.by_name("messages.csv")?;
    check_csv_piece(&mut csv, CSV_HEADER.as_bytes())?;
    for message in &manifest.messages {
        check_csv_piece(&mut csv, csv_row(message).as_bytes())?;
    }
    if csv.read(&mut [0u8; 1])? != 0 {
        return Err(ArchiveError::Invalid("messages.csv has extra rows"));
    }
    Ok(())
}

fn check_csv_piece(reader: &mut impl Read, expected: &[u8]) -> Result<(), ArchiveError> {
    let mut actual = vec![0u8; expected.len()];
    reader.read_exact(&mut actual).map_err(|_| ArchiveError::Invalid("messages.csv is truncated"))?;
    if actual != expected {
        return Err(ArchiveError::Invalid("messages.csv differs from manifest"));
    }
    Ok(())
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

fn parse_sums(bytes: &[u8]) -> Result<HashMap<&str, &str>, ArchiveError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ArchiveError::Invalid("SHA256SUMS is not UTF-8"))?;
    let mut sums = HashMap::new();
    for line in text.lines() {
        let Some((hash, name)) = line.split_once("  ") else {
            return Err(ArchiveError::Invalid("invalid SHA256SUMS line"));
        };
        if !crate::format::valid_sha256(hash) || name.is_empty() || sums.insert(name, hash).is_some() {
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

    use super::{CSV_HEADER, csv_row, verify_archive};
    use crate::ArchiveError;
    use crate::format::{FormatError, Manifest};
    use crate::writer::README;

    const MANIFEST: &[u8] = include_bytes!("../tests/fixtures/manifest-v1.json");
    const EML: &[u8] = include_bytes!("../tests/fixtures/sample.eml");
    const EML_PATH: &str = "mail/f000001/m000000001.eml";
    const SAMPLE_ZIP: &[u8] = include_bytes!("../tests/fixtures/sample-v1.zip");

    /// The members of a hand-built archive, in order, before `SHA256SUMS` is computed.
    struct Members(Vec<(String, Vec<u8>)>);

    impl Members {
        fn standard(manifest: &[u8], eml: &[u8]) -> Self {
            let parsed: Manifest = serde_json::from_slice(manifest).expect("fixture manifest");
            let mut csv = CSV_HEADER.to_string();
            for message in &parsed.messages {
                csv.push_str(&csv_row(message));
            }
            Self(vec![
                ("README.txt".into(), README.as_bytes().to_vec()),
                ("manifest.json".into(), manifest.to_vec()),
                ("messages.csv".into(), csv.into_bytes()),
                (EML_PATH.into(), eml.to_vec()),
            ])
        }

        fn rename(mut self, from: &str, to: &str) -> Self {
            for member in &mut self.0 {
                if member.0 == from {
                    member.0 = to.into();
                }
            }
            self
        }

        fn sums(&self) -> String {
            self.0
                .iter()
                .map(|(name, data)| format!("{}  {name}\n", crate::hex_digest(&Sha256::digest(data))))
                .collect()
        }

        /// Writes the members, `SHA256SUMS` (possibly changed) and optional extra members.
        fn zip(&self, sums: impl FnOnce(String) -> String, extra: &[(&str, &[u8])]) -> Vec<u8> {
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for (name, data) in &self.0 {
                writer.start_file(name.as_str(), options).expect("start member");
                writer.write_all(data).expect("member bytes");
            }
            writer.start_file("SHA256SUMS", options).expect("sum member");
            writer.write_all(sums(self.sums()).as_bytes()).expect("sums");
            for (name, data) in extra {
                writer.start_file(*name, options).expect("extra member");
                writer.write_all(data).expect("extra bytes");
            }
            writer.finish().expect("finish").into_inner()
        }
    }

    fn archive(eml: &[u8], extra: &[(&str, &[u8])]) -> Vec<u8> {
        Members::standard(MANIFEST, eml).zip(|sums| sums, extra)
    }

    fn reason(bytes: Vec<u8>) -> ArchiveError {
        verify_archive(Cursor::new(bytes)).expect_err("archive must be rejected")
    }

    fn invalid(bytes: Vec<u8>, expected: &str) {
        let error = reason(bytes);
        assert!(matches!(error, ArchiveError::Invalid(actual) if actual == expected), "{error}");
    }

    /// Replaces every occurrence of `from` by the equally long `to`.
    fn patch(bytes: &mut [u8], from: &[u8], to: &[u8], occurrences: usize) {
        assert_eq!(from.len(), to.len());
        let mut changed = 0;
        let mut index = 0;
        while index + from.len() <= bytes.len() && changed < occurrences {
            if &bytes[index..index + from.len()] == from {
                bytes[index..index + from.len()].copy_from_slice(to);
                changed += 1;
                index += from.len();
            } else {
                index += 1;
            }
        }
        assert_eq!(changed, occurrences, "patched occurrences");
    }

    #[test]
    fn ordinary_zip_with_eml_and_checksums_verifies() {
        let report = verify_archive(Cursor::new(archive(EML, &[]))).expect("valid ZIP");
        assert!(report.complete);
        assert_eq!(report.source, crate::format::Source::Server);
        assert!(report.since.is_none());
        assert_eq!((report.messages, report.aliases, report.bytes), (1, 0, 57));
    }

    #[test]
    fn checked_in_sample_zip_verifies() {
        let report = verify_archive(Cursor::new(SAMPLE_ZIP)).expect("standard ZIP fixture");
        assert_eq!(report.account, "anna@example.org");
        assert_eq!(report.messages, 1);
    }

    #[test]
    fn modified_eml_is_rejected_even_if_zip_crc_and_checksum_list_match() {
        invalid(archive(b"Different original message", &[]), "EML differs from manifest");
    }

    #[test]
    fn unexpected_traversing_absolute_or_windows_members_are_rejected() {
        for name in ["../secrets.txt", "/mail/f000001/m000000001.eml", "C:\\x", "mail\\f000001\\m000000001.eml"] {
            invalid(archive(EML, &[(name, b"bad")]), "unexpected or unsafe ZIP member");
        }
    }

    #[test]
    fn readme_name_is_case_sensitive_and_its_text_fixed() {
        let renamed = Members::standard(MANIFEST, EML).rename("README.txt", "README.TXT");
        invalid(renamed.zip(|sums| sums, &[]), "SHA256SUMS lists an unexpected file");
        let renamed = Members::standard(MANIFEST, EML).rename("README.txt", "README.TXT");
        let sums_for_original = Members::standard(MANIFEST, EML).sums();
        invalid(renamed.zip(|_| sums_for_original, &[]), "unexpected or unsafe ZIP member");
        let mut changed = Members::standard(MANIFEST, EML);
        changed.0[0].1 = b"Run this script first.\n".to_vec();
        invalid(changed.zip(|sums| sums, &[]), "README.txt differs from the version-one text");
    }

    #[test]
    fn symbolic_link_member_is_rejected_even_with_matching_bytes() {
        let mut members = Members::standard(MANIFEST, EML);
        members.0.retain(|(name, _)| name != EML_PATH);
        let sums = Members::standard(MANIFEST, EML).sums();
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, data) in &members.0 {
            writer.start_file(name.as_str(), options).expect("member");
            writer.write_all(data).expect("bytes");
        }
        let target = std::str::from_utf8(EML).expect("text sample");
        writer.add_symlink(EML_PATH, target, options).expect("symlink");
        writer.start_file("SHA256SUMS", options).expect("sums");
        writer.write_all(sums.as_bytes()).expect("sums bytes");
        invalid(writer.finish().expect("finish").into_inner(), "ZIP member is not a regular file");
    }

    #[test]
    fn local_header_that_differs_from_the_central_directory_is_rejected() {
        let mut bytes = archive(EML, &[]);
        // The first occurrence of a name is its local header; the central directory follows later.
        patch(&mut bytes, b"messages.csv", b"messages.txt", 1);
        invalid(bytes, "ZIP local headers differ from the central directory");
    }

    #[test]
    fn duplicate_member_is_rejected() {
        // ZipWriter refuses duplicates, so rename both headers of a distinct member afterwards.
        let mut bytes = archive(EML, &[("ZZZZZZ.txt", b"second copy")]);
        patch(&mut bytes, b"ZZZZZZ.txt", b"README.txt", 2);
        invalid(bytes, "duplicate ZIP member");
    }

    #[test]
    fn data_before_the_first_member_is_rejected() {
        let mut bytes = b"#!/bin/sh\nexit 0\n".to_vec();
        bytes.extend(archive(EML, &[]));
        invalid(bytes, "ZIP has data before its first member");
    }

    #[test]
    fn extra_or_missing_checksum_lines_are_rejected() {
        let extra = |sums: String| sums + &format!("{}  mail/f000001/m000000002.eml\n", "0".repeat(64));
        invalid(Members::standard(MANIFEST, EML).zip(extra, &[]), "SHA256SUMS lists an unexpected file");
        let missing = |sums: String| {
            sums.lines().filter(|line| !line.ends_with("messages.csv")).map(|line| format!("{line}\n")).collect()
        };
        invalid(Members::standard(MANIFEST, EML).zip(missing, &[]), "SHA256SUMS is missing a line");
    }

    #[test]
    fn member_missing_from_the_zip_is_rejected() {
        let mut members = Members::standard(MANIFEST, EML);
        let sums = members.sums();
        members.0.retain(|(name, _)| name != EML_PATH);
        invalid(members.zip(|_| sums, &[]), "ZIP member is missing");
    }

    #[test]
    fn deflate_bomb_behind_a_forged_size_stops_at_the_declared_length() {
        let bomb = vec![0u8; 8 * 1024 * 1024];
        let mut members = Members::standard(MANIFEST, EML);
        let sums = members.sums();
        members.0.retain(|(name, _)| name != EML_PATH);
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, data) in &members.0 {
            writer.start_file(name.as_str(), stored).expect("member");
            writer.write_all(data).expect("bytes");
        }
        writer
            .start_file(EML_PATH, SimpleFileOptions::default().compression_method(CompressionMethod::Deflated))
            .expect("bomb");
        writer.write_all(&bomb).expect("bomb bytes");
        writer.start_file("SHA256SUMS", stored).expect("sums");
        writer.write_all(sums.as_bytes()).expect("sums bytes");
        let mut bytes = writer.finish().expect("finish").into_inner();
        // Claim the manifest's 57 bytes in the local header (offset 22) and the central
        // directory (offset 24), consistently, so only the reading limit can catch it.
        let real = u32::try_from(bomb.len()).expect("size").to_le_bytes();
        patch(&mut bytes, &real, &57u32.to_le_bytes(), 2);
        let compressed = bytes.len();
        assert!(compressed < 64 * 1024, "a real bomb: {compressed} bytes expand to 8 MiB");
        // The ZIP reader itself refuses to return more than the declared size; `hash_limited`
        // is the second line behind it.
        let error = reason(bytes);
        assert!(
            matches!(&error, ArchiveError::Io(io) if io.to_string().contains("larger than its declared")),
            "{error}"
        );
    }

    #[test]
    fn forged_expansion_and_oversized_metadata_are_rejected_before_reading() {
        assert!(super::check_expansion(20_000_000, 100, super::MAX_INDEX_BYTES, super::EML_RATIO).is_err());
        assert!(
            super::check_expansion(
                super::MAX_MANIFEST_BYTES + 1,
                super::MAX_MANIFEST_BYTES,
                super::MAX_MANIFEST_BYTES,
                super::METADATA_RATIO
            )
            .is_err()
        );
        assert!(super::check_expansion(5_000_000, 10_000, super::MAX_MANIFEST_BYTES, super::METADATA_RATIO).is_err());
    }

    #[test]
    fn highly_compressed_manifest_is_rejected_by_its_expansion_ratio() {
        let text = std::str::from_utf8(MANIFEST).expect("UTF-8 fixture");
        let padded = text.replace("\"warnings\": []", &format!("\"warnings\": [\"{}\"]", " ".repeat(1000))).replacen(
            '{',
            &format!("{{{}", " ".repeat(4 * 1024 * 1024)),
            1,
        );
        let members = Members::standard(padded.as_bytes(), EML);
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, data) in &members.0 {
            writer.start_file(name.as_str(), deflated).expect("member");
            writer.write_all(data).expect("bytes");
        }
        writer.start_file("SHA256SUMS", deflated).expect("sums");
        writer.write_all(members.sums().as_bytes()).expect("sums bytes");
        invalid(writer.finish().expect("finish").into_inner(), "ZIP member exceeds size or expansion limit");
    }

    #[test]
    fn unknown_version_is_reported_as_such_even_with_new_fields() {
        let changed = std::str::from_utf8(MANIFEST)
            .expect("UTF-8 fixture")
            .replace("\"version\": 1", "\"version\": 2, \"encryption\": \"age\"");
        let error = reason(Members::standard_unparsed(changed.as_bytes()).zip(|sums| sums, &[]));
        assert!(matches!(error, ArchiveError::UnsupportedVersion(2)), "{error}");
        let other = std::str::from_utf8(MANIFEST).expect("UTF-8 fixture").replace("torro-mail-backup", "other-tool");
        let error = reason(Members::standard_unparsed(other.as_bytes()).zip(|sums| sums, &[]));
        assert!(matches!(error, ArchiveError::Format(FormatError("not a Torro mail backup"))), "{error}");
    }

    impl Members {
        /// Like `standard`, for manifests this crate cannot parse; the CSV is the fixture's.
        fn standard_unparsed(manifest: &[u8]) -> Self {
            let mut members = Self::standard(MANIFEST, EML);
            members.0[1].1 = manifest.to_vec();
            members
        }
    }

    #[test]
    fn incomplete_status_is_reported_without_calling_it_complete() {
        let changed = std::str::from_utf8(MANIFEST)
            .expect("UTF-8 fixture")
            .replace("\"status\": \"complete\"", "\"status\": \"incomplete\"");
        let bytes = Members::standard(changed.as_bytes(), EML).zip(|sums| sums, &[]);
        let report = verify_archive(Cursor::new(bytes)).expect("internally consistent archive");
        assert!(!report.complete);
    }

    #[test]
    fn manifest_with_too_many_flags_fails_while_parsing() {
        let flags = vec!["\"$k\""; crate::format::MAX_FLAGS + 1].join(",");
        let changed = std::str::from_utf8(MANIFEST).expect("UTF-8 fixture").replace("\"\\\\Seen\"", &flags);
        let error = reason(Members::standard_unparsed(changed.as_bytes()).zip(|sums| sums, &[]));
        assert!(matches!(&error, ArchiveError::Json(json) if json.to_string().contains("too long")), "{error}");
    }
}
