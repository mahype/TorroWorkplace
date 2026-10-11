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
        let Some((program, args)) = command.split_first() else {
            return Err(SetupsError::Launch("empty command".to_string()));
        };
        let mut process = std::process::Command::new(program);
        process
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // Its own process group: a signal to the daemon's group does not reach the program.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            process.process_group(0);
        }
        let mut child = process.spawn().map_err(|err| SetupsError::Launch(format!("{program}: {err}")))?;
        let pid = child.id();
        // Waiting in a thread reaps the program when it exits, so it never stays a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(pid)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn empty_command_is_an_error() {
        assert!(matches!(SystemLauncher.spawn(&[]), Err(SetupsError::Launch(_))));
    }

    #[test]
    fn missing_program_is_an_error() {
        let result = SystemLauncher.spawn(&["/nonexistent/torro-test-program".to_string()]);
        assert!(matches!(result, Err(SetupsError::Launch(_))));
    }

    #[test]
    fn starts_a_program_and_returns_its_pid() {
        let pid = SystemLauncher.spawn(&["true".to_string()]).unwrap();
        assert!(pid > 0);
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
