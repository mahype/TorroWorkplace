//! Persistent, cross-process reservations for files containing client keys or executable hooks.
//! The stable lock file is never replaced: readers hold it through the entire file operation,
//! writers reserve paths before writing any secret. Only a missing registry means no reservations.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use torro_core::i18n::tr;
use torro_ipc::Response;

use crate::clients::{create_private_dir, write_private};

#[derive(Debug, Clone)]
pub struct Registry {
    dir: PathBuf,
}

/// An owned OS lock, also safe to keep across asynchronous work. Dropping explicitly unlocks it.
pub struct Guard {
    file: File,
}

impl Drop for Guard {
    fn drop(&mut self) {
        // A concurrent fork may retain this file description until exec. Closing only our copy
        // would leave a transient lock behind after the protected operation has already finished.
        let _ = self.file.unlock();
    }
}

#[derive(Debug)]
pub enum Error {
    Busy,
    Unreadable,
}

impl Error {
    pub fn response(&self) -> Response {
        Response::Error {
            code: match self {
                Self::Busy => "busy",
                Self::Unreadable => "error",
            }
            .into(),
            message: tr("Client configuration protection is unavailable. Try again.").into(),
        }
    }
}

impl Registry {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn system() -> Option<Self> {
        if cfg!(test) {
            return None;
        }
        Some(Self::new(torro_core::paths::AppPaths::for_app(torro_core::AppId::WORKPLACE)?.config))
    }

    fn lock(&self, exclusive: bool) -> Result<Guard, Error> {
        create_private_dir(&self.dir).map_err(|_| Error::Unreadable)?;
        let path = self.dir.join("protected_paths.lock");
        // Refuse a substituted lock instead of silently locking another inode through a symlink.
        match fs::symlink_metadata(&path) {
            Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => return Err(Error::Unreadable),
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(Error::Unreadable),
            _ => {}
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|_| Error::Unreadable)?;
        let locked = if exclusive { file.try_lock() } else { file.try_lock_shared() };
        locked.map_err(|err| match err {
            std::fs::TryLockError::WouldBlock => Error::Busy,
            _ => Error::Unreadable,
        })?;
        Ok(Guard { file })
    }

    /// Fresh reservations. Caller holds a guard when acting on the result.
    pub fn paths(&self) -> Result<Vec<PathBuf>, Error> {
        match fs::read(self.dir.join("protected_paths.json")) {
            Ok(bytes) => {
                let paths: Vec<PathBuf> = serde_json::from_slice(&bytes).map_err(|_| Error::Unreadable)?;
                if paths.iter().any(|path| !path.is_absolute()) {
                    return Err(Error::Unreadable);
                }
                Ok(paths)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(_) => Err(Error::Unreadable),
        }
    }

    pub fn read(&self) -> Result<(Guard, Vec<PathBuf>), Error> {
        let guard = self.lock(false)?;
        Ok((guard, self.paths()?))
    }

    /// Reserve both the lexical path and resolved parents/targets. Reservations intentionally survive rollback.
    pub fn reserve(&self, paths: &[PathBuf]) -> Result<Guard, Error> {
        let guard = self.lock(true)?;
        let mut all = self.paths()?;
        for path in paths {
            if !path.is_absolute() {
                return Err(Error::Unreadable);
            }
            all.push(path.clone());
            all.push(resolved(path)?);
        }
        all.sort();
        all.dedup();
        let bytes = serde_json::to_vec(&all).map_err(|_| Error::Unreadable)?;
        write_private(&self.dir.join("protected_paths.json"), &bytes).map_err(|_| Error::Unreadable)?;
        Ok(guard)
    }
}

fn resolved(path: &Path) -> Result<PathBuf, Error> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            // A dangling symlink must not be confused with an ordinary not-yet-created file.
            match fs::symlink_metadata(path) {
                Ok(_) => return Err(Error::Unreadable),
                Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(Error::Unreadable),
                _ => {}
            }
            let parent = path.parent().ok_or(Error::Unreadable)?;
            Ok(resolved(parent)?.join(path.file_name().ok_or(Error::Unreadable)?))
        }
        Err(_) => Err(Error::Unreadable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_are_fresh_and_the_lock_inode_is_stable() {
        let dir = tempfile::tempdir().expect("temp");
        let registry = Registry::new(dir.path().join("config"));
        let (reader, paths) = registry.read().expect("read");
        assert!(paths.is_empty());
        assert!(matches!(registry.reserve(&[dir.path().join("secret")]), Err(Error::Busy)));
        drop(reader);
        let writer = registry.reserve(&[dir.path().join("secret")]).expect("reserved");
        assert!(matches!(registry.read(), Err(Error::Busy)));
        drop(writer);
        assert!(registry.read().expect("fresh").1.contains(&dir.path().join("secret")));
        fs::write(registry.dir.join("protected_paths.json"), "broken").expect("write");
        assert!(matches!(registry.read(), Err(Error::Unreadable)));
    }

    #[cfg(unix)]
    #[test]
    fn dropping_a_guard_unlocks_even_while_a_spawn_inherits_its_file_description() {
        let dir = tempfile::tempdir().expect("temp");
        let registry = Registry::new(dir.path().join("config"));
        for exclusive in [false, true] {
            let guard = registry.lock(exclusive).expect("locked");
            // A fork temporarily shares this open file description until exec closes it.
            // Retaining a duplicate makes that window deterministic without unsafe fork calls.
            let inherited = guard.file.try_clone().expect("inherited file description");
            drop(guard);
            let next = registry.lock(true).expect("released guard must not leave a transient busy lock");
            drop(next);
            drop(inherited);
        }
    }

    #[test]
    fn lock_child() {
        let Some(dir) = std::env::var_os("TORRO_TEST_PROTECTION") else { return };
        let registry = Registry::new(dir.into());
        let (guard, _) = registry.read().expect("shared lock");
        use std::io::Write;
        println!("LOCKED");
        std::io::stdout().flush().expect("flush");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).expect("release");
        drop(guard);
    }

    #[test]
    fn a_separate_process_holds_the_same_stable_lock() {
        use std::io::{BufRead, BufReader, Write};
        use std::process::{Command, Stdio};
        let dir = tempfile::tempdir().expect("temp");
        let registry = Registry::new(dir.path().join("config"));
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "client_setup::protection::tests::lock_child", "--nocapture"])
            .env("TORRO_TEST_PROTECTION", &registry.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("child");
        let mut out = BufReader::new(child.stdout.take().expect("stdout"));
        loop {
            let mut line = String::new();
            assert!(out.read_line(&mut line).expect("read") > 0, "child exited before locking");
            if line.contains("LOCKED") {
                break;
            }
        }
        assert!(matches!(registry.reserve(&[dir.path().join("new-secret")]), Err(Error::Busy)));
        child.stdin.take().expect("stdin").write_all(b"release\n").expect("release");
        assert!(child.wait().expect("wait").success());
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(registry.dir.join("protected_paths.lock")).expect("meta").ino()
        };
        drop(registry.reserve(&[dir.path().join("new-secret")]).expect("reserve"));
        drop(registry.reserve(&[dir.path().join("another-secret")]).expect("reserve"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(registry.dir.join("protected_paths.lock")).expect("meta").ino(), inode);
        }
    }

    #[cfg(unix)]
    #[test]
    fn reservations_keep_symlink_targets_and_reject_dangling_links() {
        let dir = tempfile::tempdir().expect("temp");
        let registry = Registry::new(dir.path().join("config"));
        let real = dir.path().join("real");
        fs::write(&real, "secret").expect("write");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        drop(registry.reserve(std::slice::from_ref(&link)).expect("reserve"));
        let (_, paths) = registry.read().expect("read");
        assert!(paths.contains(&link) && paths.contains(&fs::canonicalize(&real).expect("real")));
        let dangling = dir.path().join("dangling");
        std::os::unix::fs::symlink(dir.path().join("missing"), &dangling).expect("link");
        assert!(registry.reserve(&[dangling]).is_err());
    }
}
