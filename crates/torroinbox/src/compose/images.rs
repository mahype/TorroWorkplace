//! Local image input of the compose window (issues #461, #969): the system's chooser, and the check that a path the
//! window hands to the core is a plain local file. Pixels pasted into the page and files dropped on it come over
//! the bridge ([`super::session::FromPage`]); nothing here reads the clipboard.

use std::path::{Path, PathBuf};

/// Asks for a picture with the system's chooser; none when it is cancelled.
pub(super) async fn choose() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(super::tr("Insert image"))
        .add_filter(super::tr("Images"), &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf())
}

/// Whether `path` is an absolute path to a plain file, with no symbolic link on the way: a dropped or chosen path
/// is not permission to visit anything else.
pub(super) fn local_file(path: &Path) -> bool {
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        // A native file list is not permission to visit UNC servers or device namespaces.
        return false;
    }
    path.is_absolute()
        && path.ancestors().all(|part| std::fs::symlink_metadata(part).is_ok_and(|m| !m.file_type().is_symlink()))
        && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_local_files_pass() {
        let dir = tempfile::tempdir().expect("temp");
        let root = std::fs::canonicalize(dir.path()).expect("canonical");
        let file = root.join("file");
        std::fs::write(&file, b"file").expect("write");
        assert!(local_file(&file));
        assert!(!local_file(&root), "a folder is no file");
        assert!(!local_file(Path::new("relative.png")), "relative paths are refused");
        assert!(!local_file(&root.join("missing")));
    }

    #[cfg(unix)]
    #[test]
    fn native_file_lists_reject_symlinks_including_parent_components() {
        let dir = tempfile::tempdir().expect("temp");
        let root = std::fs::canonicalize(dir.path()).expect("canonical");
        let file = root.join("file");
        std::fs::write(&file, b"file").expect("write");
        let link = root.join("link");
        std::os::unix::fs::symlink(&file, &link).expect("symlink");
        assert!(!local_file(&link));
        let folder = root.join("folder-link");
        std::os::unix::fs::symlink(&root, &folder).expect("symlink");
        assert!(!local_file(&folder.join("file")));
        assert!(local_file(&file));
    }
}
