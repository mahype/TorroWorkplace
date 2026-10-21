//! Publish a verified mail ZIP in the core's private backup directory without exposing partial files.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use torro_mail_backup::{ArchiveError, VerifiedArchive, verify_archive};

use super::MailDirs;

#[derive(Debug)]
pub(crate) enum BackupFileError {
    Io(io::Error),
    Archive(ArchiveError),
    InvalidName,
    Incomplete,
}

impl fmt::Display for BackupFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot publish mail backup: {error}"),
            Self::Archive(error) => write!(f, "mail backup failed verification: {error}"),
            Self::InvalidName => f.write_str("invalid mail backup filename"),
            Self::Incomplete => f.write_str("incomplete mail backup cannot be published"),
        }
    }
}

impl std::error::Error for BackupFileError {}

impl From<io::Error> for BackupFileError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ArchiveError> for BackupFileError {
    fn from(error: ArchiveError) -> Self {
        Self::Archive(error)
    }
}

/// The only publication route for generated backups. The builder writes to a private temporary
/// file in the destination directory; failure, cancellation or invalid output removes that file.
pub(crate) fn write_verified_backup(
    dirs: &MailDirs,
    filename: &str,
    build: impl FnOnce(&mut File) -> Result<(), ArchiveError>,
) -> Result<(PathBuf, VerifiedArchive), BackupFileError> {
    if !valid_filename(filename) {
        return Err(BackupFileError::InvalidName);
    }
    reject_symlink(&dirs.data)?;
    crate::clients::create_private_dir(&dirs.data)?;
    let backup_dir = dirs.data.join("backups");
    reject_symlink(&backup_dir)?;
    crate::clients::create_private_dir(&backup_dir)?;
    let destination = backup_dir.join(filename);
    let mut temporary = tempfile::Builder::new().prefix(".backup-").suffix(".partial").tempfile_in(&backup_dir)?;
    build(temporary.as_file_mut())?;
    temporary.as_file_mut().seek(SeekFrom::Start(0))?;
    let report = verify_archive(temporary.as_file_mut())?;
    if !report.complete {
        return Err(BackupFileError::Incomplete);
    }
    temporary.as_file().sync_all()?;
    temporary.persist_noclobber(&destination).map_err(|error| BackupFileError::Io(error.error))?;
    Ok((destination, report))
}

fn valid_filename(name: &str) -> bool {
    name.len() <= 120
        && !name.starts_with('.')
        && name.ends_with(".zip")
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::other("mail backup directory is a symlink")),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;

    use super::{BackupFileError, write_verified_backup};
    use crate::mail::MailDirs;

    const SAMPLE_ZIP: &[u8] = include_bytes!("../../../torro-mail-backup/tests/fixtures/sample-v1.zip");

    fn dirs(root: &std::path::Path) -> MailDirs {
        MailDirs::new(&root.join("data"), &root.join("cache"))
    }

    #[test]
    fn publishes_only_verified_archive_without_overwriting() {
        let root = tempfile::tempdir().expect("tempdir");
        let dirs = dirs(root.path());
        let (path, report) = write_verified_backup(&dirs, "mail-2026.zip", |file| {
            file.write_all(SAMPLE_ZIP)?;
            Ok(())
        })
        .expect("archive published");
        assert!(report.complete);
        assert_eq!(fs::read(&path).expect("backup"), SAMPLE_ZIP);
        assert!(
            write_verified_backup(&dirs, "mail-2026.zip", |file| {
                file.write_all(SAMPLE_ZIP)?;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).expect("original backup"), SAMPLE_ZIP);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).expect("file mode").permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn failed_or_invalid_backup_leaves_no_published_or_partial_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let dirs = dirs(root.path());
        assert!(
            write_verified_backup(&dirs, "bad.zip", |file| {
                file.write_all(b"partial ZIP")?;
                Err(torro_mail_backup::ArchiveError::Invalid("source read failed"))
            })
            .is_err()
        );
        assert!(
            write_verified_backup(&dirs, "invalid.zip", |file| {
                file.write_all(b"not ZIP")?;
                Ok(())
            })
            .is_err()
        );
        assert!(fs::read_dir(dirs.data.join("backups")).expect("backup folder").next().is_none());
        assert!(matches!(
            write_verified_backup(&dirs, "../outside.zip", |_| Ok(())),
            Err(BackupFileError::InvalidName)
        ));
    }
}
