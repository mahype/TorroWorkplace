//! Finding out how a window was started, and starting programs again.
//!
//! Task 1.12 (`linux.rs`) and task 1.15 (`SystemLauncher`) in `docs/tasks/phase-1.md`.
//! Reference: `apps/workspace-setups/workspace_setups/commands.py`.

#[cfg(target_os = "linux")]
pub mod linux;

use torro_wm::Window;

use crate::SetupsError;

/// Starts programs. [`SystemLauncher`] does it for real; tests pass a fake that opens a
/// window in [`torro_wm::fake::FakeWm`] instead.
pub trait Launcher: Send + Sync {
    /// Starts `command` detached from the daemon (own process group, output discarded) and
    /// returns its pid.
    fn spawn(&self, command: &[String]) -> Result<u32, SetupsError>;
}

/// Starts real processes.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn spawn(&self, command: &[String]) -> Result<u32, SetupsError> {
        let _ = command;
        Err(SetupsError::NotImplemented("SystemLauncher::spawn (task 1.15)"))
    }
}

/// The command that starts `window` again; empty when it cannot be found out. Linux:
/// `/proc/<pid>/cmdline`, else the `.desktop` entry of the class. Other systems: empty
/// for now.
#[cfg(target_os = "linux")]
pub fn command_for(window: &Window) -> Vec<String> {
    if let Some(pid) = window.pid
        && let Some(command) = linux::command_from_proc(std::path::Path::new("/proc"), pid)
    {
        return command;
    }
    let dirs = linux::desktop_dirs(|key| std::env::var(key).ok());
    linux::desktop_command(&dirs, &window.class).unwrap_or_default()
}

/// The command that starts `window` again; empty when it cannot be found out. Linux:
/// `/proc/<pid>/cmdline`, else the `.desktop` entry of the class. Other systems: empty
/// for now.
#[cfg(not(target_os = "linux"))]
pub fn command_for(window: &Window) -> Vec<String> {
    let _ = window;
    Vec::new()
}
