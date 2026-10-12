//! Running the `aerospace` command line program.
//!
//! Task X.6 in `docs/tasks/plattformen.md`. Every request is one run of the program, e.g.
//! `aerospace list-windows --all --json`; the program talks to the AeroSpace server and exits.
//! A failed run prints its reason on stderr and exits non-zero.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::{WindowId, WmError};

/// Environment variable that names the program to run instead of `aerospace` from `PATH`.
/// Tests point it at a fake script; when it is set, nothing else is searched.
pub const PROGRAM_VARIABLE: &str = "TORRO_AEROSPACE_BIN";

/// Where Homebrew installs the program. A daemon started by launchd at login only gets
/// `/usr/bin:/bin:/usr/sbin:/sbin` as `PATH`, so these are tried after `PATH`.
const KNOWN_LOCATIONS: [&str; 2] = ["/opt/homebrew/bin/aerospace", "/usr/local/bin/aerospace"];

/// How long one run may take; an AeroSpace that hangs must not block the daemon.
const TIMEOUT: Duration = Duration::from_secs(5);

/// What the program prints when the AeroSpace app is not running.
const NOT_RUNNING: &str = "Can't connect to AeroSpace server";

/// The program to run: the value of [`PROGRAM_VARIABLE`] if set and not empty (used as is,
/// even if it does not exist), else the first `aerospace` in `PATH`, else the first
/// [`KNOWN_LOCATIONS`] entry that exists. `NotRunning` when none is found.
pub fn find_program(env: impl Fn(&str) -> Option<String>) -> Result<PathBuf, WmError> {
    if let Some(program) = env(PROGRAM_VARIABLE).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(program));
    }
    let from_path = std::env::split_paths(&env("PATH").unwrap_or_default())
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join("aerospace"))
        .find(|candidate| candidate.is_file());
    from_path
        .or_else(|| KNOWN_LOCATIONS.iter().map(PathBuf::from).find(|candidate| candidate.is_file()))
        .ok_or_else(|| WmError::NotRunning("aerospace program not found".into()))
}

/// Runs `program` with `args` and returns its stdout.
///
/// Errors: `NotRunning` when the program is missing or reports that the AeroSpace server does
/// not answer; `Protocol` with the trimmed stderr (or stdout) for any other non-zero exit;
/// `Io` when the run fails or takes longer than [`TIMEOUT`] (the program is then killed).
pub async fn run(program: &Path, args: &[&str]) -> Result<String, WmError> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(TIMEOUT, command.output())
        .await
        .map_err(|_| {
            WmError::Io(format!("aerospace {} did not finish within {} s", describe(args), TIMEOUT.as_secs()))
        })?
        .map_err(|err| spawn_error(program, &err))?;
    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| WmError::Protocol(format!("aerospace {}: answer is not UTF-8: {err}", describe(args))))?;
    if output.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = if stderr.trim().is_empty() { stdout.trim().to_owned() } else { stderr.trim().to_owned() };
    Err(failure(args, reason))
}

/// Whether `program list-workspaces --focused` succeeds with an answer within `timeout`.
/// Synchronous, because [`crate::detect`] is; a run that takes longer is killed.
pub fn answers(program: &Path, timeout: Duration) -> bool {
    let child = std::process::Command::new(program)
        .args(["list-workspaces", "--focused"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success() && has_output(&mut child),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Reads what an exited child printed; a focused workspace always has a name.
fn has_output(child: &mut std::process::Child) -> bool {
    use std::io::Read;

    let mut output = String::new();
    child.stdout.take().is_some_and(|mut stdout| stdout.read_to_string(&mut output).is_ok())
        && !output.trim().is_empty()
}

/// AeroSpace window ids are numbers (macOS window numbers). Anything else cannot name a
/// window, so it is reported as `NotFound`.
pub fn window_arg(window: &WindowId) -> Result<String, WmError> {
    let id = window.0.trim();
    id.parse::<u32>()
        .map(|number| number.to_string())
        .map_err(|_| WmError::NotFound(format!("{id:?} is not an AeroSpace window id")))
}

fn spawn_error(program: &Path, err: &std::io::Error) -> WmError {
    match err.kind() {
        std::io::ErrorKind::NotFound => WmError::NotRunning(format!("{} not found", program.display())),
        _ => WmError::Io(format!("run {}: {err}", program.display())),
    }
}

fn failure(args: &[&str], reason: String) -> WmError {
    if reason.contains(NOT_RUNNING) {
        WmError::NotRunning(reason)
    } else if reason.is_empty() {
        WmError::Protocol(format!("aerospace {} failed", describe(args)))
    } else {
        WmError::Protocol(reason)
    }
}

/// The subcommand for messages; the rest (formats, ids) only adds noise.
fn describe<'a>(args: &[&'a str]) -> &'a str {
    args.first().copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key: &str| pairs.iter().find(|(name, _)| *name == key).map(|(_, value)| (*value).to_string())
    }

    #[test]
    fn variable_wins_even_if_the_file_is_missing() {
        let env = [(PROGRAM_VARIABLE, "/no/such/aerospace"), ("PATH", "/usr/bin")];
        assert_eq!(find_program(fake_env(&env)), Ok(PathBuf::from("/no/such/aerospace")));
    }

    #[test]
    fn window_ids_must_be_numbers() {
        assert_eq!(window_arg(&WindowId(" 432 ".into())), Ok("432".into()));
        assert!(matches!(window_arg(&WindowId("0xabc".into())), Err(WmError::NotFound(_))));
        assert!(matches!(window_arg(&WindowId("432; close".into())), Err(WmError::NotFound(_))));
    }

    #[test]
    fn server_down_is_not_running_and_other_failures_are_protocol_errors() {
        let down = failure(&["list-windows"], "Can't connect to AeroSpace server. Is AeroSpace.app running?".into());
        assert!(matches!(down, WmError::NotRunning(_)), "{down:?}");
        assert_eq!(
            failure(&["focus"], "No windows in the specified direction".into()),
            WmError::Protocol("No windows in the specified direction".into())
        );
        assert_eq!(failure(&["close"], String::new()), WmError::Protocol("aerospace close failed".into()));
    }

    #[tokio::test]
    async fn missing_program_is_not_running() {
        let result = run(Path::new("/no/such/dir/aerospace"), &["list-windows"]).await;
        assert!(matches!(result, Err(WmError::NotRunning(_))), "{result:?}");
    }

    #[test]
    fn missing_program_does_not_answer() {
        assert!(!answers(Path::new("/no/such/dir/aerospace"), Duration::from_millis(200)));
    }
}
