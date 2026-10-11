//! Start commands on Linux.
//!
//! Task 1.12 in `docs/tasks/phase-1.md`. `/proc` and the `.desktop` search paths are
//! parameters so tests can use a fake directory tree.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The command line of `pid` from `<proc_root>/<pid>/cmdline` (NUL-separated), with
/// [`strip_secrets`] applied; `None` when unreadable or empty.
pub fn command_from_proc(proc_root: &Path, pid: u32) -> Option<Vec<String>> {
    let _ = (proc_root, pid);
    None
}

/// The `Exec=` line of the `.desktop` entry whose file name or `StartupWMClass` matches
/// `class` (case-insensitive), split into arguments with field codes (`%u`, `%F`, …)
/// removed. Searches `dirs` in order.
pub fn desktop_command(dirs: &[PathBuf], class: &str) -> Option<Vec<String>> {
    let _ = (dirs, class);
    None
}

/// The standard `.desktop` directories: `$XDG_DATA_HOME/applications`
/// (`~/.local/share/applications`), then each of `$XDG_DATA_DIRS` (`/usr/local/share`,
/// `/usr/share`) + `/applications`.
pub fn desktop_dirs(env: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let _ = env;
    Vec::new()
}

/// Drops arguments that look like credentials: `--password x`, `--password=x`, `--token …`,
/// `token=…`, `secret=…`, `apikey=…` and the like (case-insensitive).
pub fn strip_secrets(args: Vec<String>) -> Vec<String> {
    args
}

/// `pid` and all its ancestors (via `PPid:` in `<proc_root>/<pid>/status`), for matching
/// a new window to the process we started.
pub fn process_ancestors(proc_root: &Path, pid: u32) -> HashSet<u32> {
    let _ = proc_root;
    HashSet::from([pid])
}
