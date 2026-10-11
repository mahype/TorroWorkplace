//! Start commands on Windows.
//!
//! Task X.8 in `docs/tasks/plattformen.md`. A window's process (GlazeWM gives the window handle,
//! `torro-wm` turns it into a pid) tells its executable path and command line through
//! `sysinfo`, which uses `QueryFullProcessImageName` and the process parameters, without
//! `unsafe` here. The command is that path followed by the arguments. The decision is a pure
//! function so its test runs on every system.

use std::path::Path;

/// The command that starts the program with executable `exe` and command line `cmd` (first word
/// included, as the system reports it) again: the full path of the executable, then the
/// arguments. Arguments that look like credentials are removed. Empty when nothing is known.
///
/// No `cmd` and no `exe` happens for processes we may not inspect (elevated ones, say). With
/// `exe` only, the program starts without arguments.
pub fn command_from_process(exe: Option<&Path>, cmd: &[String]) -> Vec<String> {
    super::program_and_args(exe, cmd)
}

/// The command that starts the process `pid` again, see [`command_from_process`]; `None` when
/// there is no such process or the system does not tell us its program.
#[cfg(windows)]
pub fn command_for_pid(pid: u32) -> Option<Vec<String>> {
    let (exe, cmd) = super::process_info(pid)?;
    let command = command_from_process(exe.as_deref(), &cmd);
    (!command.is_empty()).then_some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn path_comes_from_the_executable_and_arguments_follow() {
        let exe = Path::new(r"C:\Program Files\Mozilla Firefox\firefox.exe");
        let cmd = words(&["firefox.exe", "-P", "work", "-no-remote"]);
        assert_eq!(
            command_from_process(Some(exe), &cmd),
            words(&[r"C:\Program Files\Mozilla Firefox\firefox.exe", "-P", "work", "-no-remote"])
        );
    }

    #[test]
    fn executable_without_a_command_line_starts_without_arguments() {
        let exe = Path::new(r"C:\Windows\System32\notepad.exe");
        assert_eq!(command_from_process(Some(exe), &[]), words(&[r"C:\Windows\System32\notepad.exe"]));
    }

    #[test]
    fn credentials_are_removed() {
        let exe = Path::new(r"C:\Tools\sync.exe");
        let cmd = words(&["sync.exe", "--token", "abc", "/folder", "D:\\Data"]);
        assert_eq!(command_from_process(Some(exe), &cmd), words(&[r"C:\Tools\sync.exe", "/folder", "D:\\Data"]));
    }

    #[test]
    fn nothing_known_gives_an_empty_command() {
        assert!(command_from_process(None, &[]).is_empty());
    }

    /// The test process: its command is its own executable and arguments.
    #[cfg(windows)]
    #[test]
    fn the_own_process_is_found_by_pid() {
        let command = command_for_pid(std::process::id()).expect("the own process is known");
        let own = std::env::current_exe().expect("current exe");
        let program = std::fs::canonicalize(&command[0]).expect("the program exists");
        assert_eq!(program, std::fs::canonicalize(own).expect("current exe exists"));
        let args = super::super::strip_secrets(std::env::args().collect());
        assert_eq!(command[1..], args[1..]);
    }

    #[cfg(windows)]
    #[test]
    fn a_pid_that_does_not_exist_gives_none() {
        assert_eq!(command_for_pid(u32::MAX - 1), None);
    }
}
