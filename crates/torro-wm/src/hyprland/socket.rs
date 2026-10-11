//! Hyprland's request socket `.socket.sock`.
//!
//! Task 1.3 in `docs/tasks/phase-1.md`. One request per connection: connect, write the
//! command (e.g. `j/clients`, `dispatch hl.dsp.focus({ … })`), read until EOF.

use std::path::{Path, PathBuf};

use crate::WmError;

/// `<runtime_dir>/hypr/<signature>`; an error when either is missing or empty.
pub fn socket_dir(runtime_dir: Option<&str>, signature: Option<&str>) -> Result<PathBuf, WmError> {
    let _ = (runtime_dir, signature);
    Err(WmError::NotImplemented("hyprland socket_dir (task 1.3)"))
}

/// Sends `command` to `<dir>/.socket.sock` and returns the whole answer. Unix only; on other
/// systems an `Unsupported` error.
pub async fn request(dir: &Path, command: &str) -> Result<String, WmError> {
    let _ = (dir, command);
    Err(WmError::NotImplemented("hyprland request (task 1.3)"))
}
