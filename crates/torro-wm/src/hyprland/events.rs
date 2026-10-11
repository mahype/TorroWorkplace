//! Hyprland's event socket `.socket2.sock`.
//!
//! Task 1.7 in `docs/tasks/phase-1.md`. Hyprland writes one line per event,
//! `name>>data`, e.g. `openwindow>>55d0a1b2c3d4,3,foot,~`. Addresses come without `0x`;
//! [`crate::WindowId`] carries them with `0x`, as `j/clients` does.

use std::path::Path;

use crate::{Events, WmError, WmEvent};

/// One event line as a [`WmEvent`]; `None` for events we do not use.
pub fn parse_event(line: &str) -> Option<WmEvent> {
    let _ = line;
    None
}

/// Connects to `<dir>/.socket2.sock` and forwards parsed events into the returned channel
/// from a background task until the socket closes or the receiver is dropped.
pub async fn subscribe(dir: &Path) -> Result<Events, WmError> {
    let _ = dir;
    Err(WmError::NotImplemented("hyprland events (task 1.7)"))
}
