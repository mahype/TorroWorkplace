//! Finding out how a window was started, and starting programs again.
//!
//! Task 1.12 (`linux.rs`) and task 1.15 (`SystemLauncher`) in `docs/tasks/phase-1.md`, task X.8
//! (`macos.rs`, `windows.rs`) in `docs/tasks/plattformen.md`.
//! Reference: `apps/workspace-setups/workspace_setups/commands.py`.
//!
//! `macos.rs` and `windows.rs` build on every system, like the window manager backends: what
//! they decide is pure (paths and argument lists) and is tested everywhere; only reading a
//! running process is limited to its own system.

#[cfg(target_os = "linux")]
pub mod linux;
pub mod macos;
pub mod windows;

use std::path::Path;

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

/// Words that mark an argument as a credential, matched case-insensitively against the part
/// before `=`. Mirrors `SECRET` in `apps/workspace-setups/workspace_setups/commands.py`.
const SECRET_WORDS: [&str; 8] = ["token", "password", "passwd", "secret", "apikey", "api-key", "api_key", "auth"];

/// Flags that contain a secret word but carry no secret (Chromium's keyring backend).
const NOT_SECRET: &[&str] = &["password-store"];

/// Drops arguments that look like credentials: `--password x`, `--password=x`, `--token …`,
/// `token=…`, `secret=…`, `apikey=…` and the like (case-insensitive). The first element is the
/// program and always kept. A saved template must never hold a credential, on any system.
pub fn strip_secrets(args: Vec<String>) -> Vec<String> {
    let mut iter = args.into_iter();
    // The first element is the program, not an argument, so it is always kept.
    let mut kept: Vec<String> = iter.next().into_iter().collect();
    let mut skip_next = false;
    for arg in iter {
        if std::mem::take(&mut skip_next) {
            continue;
        }
        let key = arg.split_once('=').map_or(arg.as_str(), |(key, _)| key);
        if is_secret_key(key) {
            // `--password x` takes its value from the next argument; `password=x` does not.
            skip_next = !arg.contains('=');
            continue;
        }
        kept.push(arg);
    }
    kept
}

fn is_secret_key(key: &str) -> bool {
    let lowered = key.to_lowercase();
    if NOT_SECRET.contains(&lowered.trim_start_matches('-')) {
        return false;
    }
    SECRET_WORDS.iter().any(|word| lowered.contains(word))
}

/// The program and arguments of a running process as the system reports them, with
/// [`strip_secrets`] applied; empty when there is no program to start.
///
/// `exe` is the path of the executable file (macOS `proc_pidpath`, Windows
/// `QueryFullProcessImageName`, both via `sysinfo`). It is preferred to `cmd[0]`, which a process
/// may set freely and which is often just a name (`notepad`). `cmd` is the whole command line
/// including that first word, which is skipped. Without `exe`, `cmd[0]` stands in for it.
fn program_and_args(exe: Option<&Path>, cmd: &[String]) -> Vec<String> {
    let program = match exe {
        Some(exe) => exe.to_string_lossy().into_owned(),
        None => cmd.first().cloned().unwrap_or_default(),
    };
    if program.is_empty() {
        return Vec::new();
    }
    let mut command = vec![program];
    command.extend(cmd.iter().skip(1).cloned());
    strip_secrets(command)
}

/// What the system says about the process `pid`: the path of its executable (if it tells us) and
/// its command line, first word included (empty if it does not tell us, for a process of another
/// user say). `None` when there is no such process. `sysinfo` does the system calls, so this
/// crate needs no `unsafe`.
#[cfg(any(target_os = "macos", windows))]
fn process_info(pid: u32) -> Option<(Option<std::path::PathBuf>, Vec<String>)> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    // Only this process and only what we read; a full refresh would walk the whole process table.
    let wanted = ProcessRefreshKind::nothing().with_exe(UpdateKind::Always).with_cmd(UpdateKind::Always);
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), false, wanted);
    let process = system.process(pid)?;
    let cmd = process.cmd().iter().map(|word| word.to_string_lossy().into_owned()).collect();
    Some((process.exe().map(Path::to_path_buf), cmd))
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

/// The command that starts `window` again; empty when it cannot be found out.
///
/// - Linux: `/proc/<pid>/cmdline`, else the `.desktop` entry of the class.
/// - macOS: the process of `window.pid`; a program inside an app bundle is started with `open`
///   (see [`macos`]).
/// - Windows: the process of `window.pid` (see [`windows`]).
/// - Other systems: empty.
///
/// Arguments that look like credentials are removed on every system ([`strip_secrets`]).
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

/// The command that starts `window` again; empty when it cannot be found out. macOS: see
/// [`macos::command_for_pid`].
#[cfg(target_os = "macos")]
pub fn command_for(window: &Window) -> Vec<String> {
    window.pid.and_then(macos::command_for_pid).unwrap_or_default()
}

/// The command that starts `window` again; empty when it cannot be found out. Windows: see
/// [`windows::command_for_pid`].
#[cfg(windows)]
pub fn command_for(window: &Window) -> Vec<String> {
    window.pid.and_then(windows::command_for_pid).unwrap_or_default()
}

/// The command that starts `window` again; empty on systems without a window manager backend.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn command_for(window: &Window) -> Vec<String> {
    let _ = window;
    Vec::new()
}

#[cfg(test)]
mod command_tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn strip_secrets_removes_flag_with_separate_value() {
        let args = words(&["app", "--password", "x", "--keep", "--Token", "abc", "--url", "y"]);
        assert_eq!(strip_secrets(args), words(&["app", "--keep", "--url", "y"]));
    }

    #[test]
    fn strip_secrets_removes_flag_with_equals_value() {
        let args = words(&["app", "--password=x", "--passwd=y", "--api-key=z", "--keep=1"]);
        assert_eq!(strip_secrets(args), words(&["app", "--keep=1"]));
    }

    #[test]
    fn strip_secrets_removes_key_value_words_in_any_case() {
        let args = words(&["app", "token=abc", "SECRET=s", "ApiKey=k", "API_KEY=v", "auth=a", "lang=de"]);
        assert_eq!(strip_secrets(args), words(&["app", "lang=de"]));
    }

    #[test]
    fn strip_secrets_keeps_chromium_password_store() {
        let args = words(&["chromium", "--password-store=gnome-libsecret", "--password=x"]);
        assert_eq!(strip_secrets(args), words(&["chromium", "--password-store=gnome-libsecret"]));
    }

    #[test]
    fn strip_secrets_keeps_the_program_name() {
        assert_eq!(strip_secrets(words(&["/opt/auth-helper/run"])), words(&["/opt/auth-helper/run"]));
        assert!(strip_secrets(Vec::new()).is_empty());
    }

    #[test]
    fn program_comes_from_the_executable_path_not_from_the_first_word() {
        let command = program_and_args(Some(Path::new("/opt/tool/tool")), &words(&["tool", "--flag", "x"]));
        assert_eq!(command, words(&["/opt/tool/tool", "--flag", "x"]));
    }

    #[test]
    fn first_word_stands_in_without_an_executable_path() {
        assert_eq!(program_and_args(None, &words(&["/opt/tool/tool", "-v"])), words(&["/opt/tool/tool", "-v"]));
        assert!(program_and_args(None, &[]).is_empty(), "nothing known, nothing to start");
        assert!(program_and_args(None, &words(&[""])).is_empty());
    }

    #[test]
    fn executable_alone_is_a_command_without_arguments() {
        assert_eq!(program_and_args(Some(Path::new("/opt/tool/tool")), &[]), words(&["/opt/tool/tool"]));
    }

    #[test]
    fn credentials_are_not_kept() {
        let command = program_and_args(Some(Path::new("/opt/tool/tool")), &words(&["tool", "--token", "abc", "--ok"]));
        assert_eq!(command, words(&["/opt/tool/tool", "--ok"]));
    }
}
