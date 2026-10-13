//! Bringing the window of a process chain forward on macOS, with or without AeroSpace (task E8.9).
//!
//! Terminals on macOS serve all their windows from one process (Terminal, iTerm2, Ghostty), so
//! the process alone does not say which window shows a coding agent, and without AeroSpace there is
//! no window manager to ask at all. What the system offers instead, without `unsafe` and without
//! Objective-C bindings in the crate:
//!
//! - **The agent's tty** (`/bin/ps -o pid=,tty=`): Terminal and iTerm2 name the tty of every tab
//!   and session in their scripting dictionary, so a JavaScript for Automation run by `osascript`
//!   ([`SCRIPT`]) finds the window that holds it. Their window ids are the macOS window numbers,
//!   which AeroSpace uses as well (checked by hand for both on 2026-10-13). Asking them needs the
//!   person's consent once ("… wants to control Terminal", System Settings › Privacy & Security ›
//!   Automation); macOS asks when the first jump needs it, whoever asked for the jump (an agent
//!   too, through `inbox.open_item`), and the run waits up to [`TIMEOUT`] for the answer. Without
//!   consent the script falls back to the app.
//! - **The app** (`NSRunningApplication`, no consent needed): the nearest process that is a
//!   regular app is activated, which brings its windows forward. That is the most there is for
//!   Ghostty and every other app without such a dictionary.
//!
//! Everything the script gets is an argument (`on run argv`), never part of its text: a fixed
//! mode, the tty as `ps` names it and the process ids as numbers. Nothing of an inbox entry's
//! text reaches it, and nothing runs through a shell.
//!
//! The module builds on every system so its parsers are tested everywhere; only macOS runs it
//! ([`crate::focus_process_window`]).

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use crate::{WindowId, WmError};

use super::geometry::OSASCRIPT;

/// Where macOS keeps `ps`.
pub const PS: &str = "/bin/ps";

/// How long one run may take. Long enough for the person to answer macOS's question whether the
/// core may control Terminal the first time; `osascript` waits for it.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Finds or raises the window of a process chain. Arguments: the mode (`find` or `raise`), the
/// agent's tty (`/dev/ttys003`, or empty when unknown), then the process ids, nearest first.
///
/// The first process that is a regular app (`NSApplicationActivationPolicyRegular`) decides. When
/// it is Terminal or iTerm2 and one of its tabs runs on the tty, the answer is `window <id>`;
/// `raise` also selects the tab and brings the window and the app to the front. Otherwise the
/// answer is `app` (and `raise` activates the app), or `none` when no process is an app. A
/// refused or failing scripting request (no consent for Automation) counts as "not found".
pub const SCRIPT: &str = r#"ObjC.import("AppKit");
function inTerminal(app, tty, raise) {
  var windows = app.windows();
  for (var i = 0; i < windows.length; i++) {
    var tabs = windows[i].tabs();
    for (var j = 0; j < tabs.length; j++) {
      if (tabs[j].tty() !== tty) continue;
      var id = windows[i].id();
      if (raise) { tabs[j].selected = true; windows[i].index = 1; app.activate(); }
      return id;
    }
  }
  return null;
}
function inITerm(app, tty, raise) {
  var windows = app.windows();
  for (var i = 0; i < windows.length; i++) {
    var tabs = windows[i].tabs();
    for (var j = 0; j < tabs.length; j++) {
      var sessions = tabs[j].sessions();
      for (var k = 0; k < sessions.length; k++) {
        if (sessions[k].tty() !== tty) continue;
        var id = windows[i].id();
        if (raise) { windows[i].select(); tabs[j].select(); sessions[k].select(); app.activate(); }
        return id;
      }
    }
  }
  return null;
}
function windowOf(bundle, tty, raise) {
  if (!tty) return null;
  try {
    if (bundle === "com.apple.Terminal") return inTerminal(Application(bundle), tty, raise);
    if (bundle === "com.googlecode.iterm2") return inITerm(Application(bundle), tty, raise);
  } catch (e) {}
  return null;
}
function run(argv) {
  var raise = argv[0] === "raise", tty = argv[1];
  for (var i = 2; i < argv.length; i++) {
    var app = $.NSRunningApplication.runningApplicationWithProcessIdentifier(parseInt(argv[i], 10));
    if (app.isNil() || app.activationPolicy !== $.NSApplicationActivationPolicyRegular) continue;
    var id = windowOf(ObjC.unwrap(app.bundleIdentifier), tty, raise);
    if (id !== null) return "window " + id;
    if (raise) app.activateWithOptions($.NSApplicationActivateIgnoringOtherApps);
    return "app";
  }
  return "none";
}"#;

/// What [`SCRIPT`] answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The window of Terminal or iTerm2 that holds the tty, by its macOS window number.
    Window(WindowId),
    /// A regular app among the processes, but no window of it known by the tty.
    App,
    /// None of the processes is a regular app.
    None,
}

/// The window of Terminal or iTerm2 whose tab runs on the tty of `pids` (nearest first), without
/// raising anything. `None` when no process has a tty, the app is another one, the person did
/// not consent to Automation, or a run fails. Meant for choosing among several windows of one
/// terminal that AeroSpace lists.
pub async fn terminal_window(pids: &[u32]) -> Option<WindowId> {
    let tty = tty_of(pids).await?;
    match ask(Path::new(OSASCRIPT), "find", &tty, pids).await {
        Ok(Answer::Window(id)) => Some(id),
        _ => None,
    }
}

/// Brings forward what shows `pids` (nearest first) without a window manager: the window of
/// Terminal or iTerm2 with the processes' tty (tab selected), else the nearest process that is a
/// regular app. `Ok(false)` when none of the processes is an app (closed, or under a tmux
/// server); `Err` when `osascript` cannot run or answers something else.
pub async fn raise(pids: &[u32]) -> Result<bool, WmError> {
    let tty = tty_of(pids).await.unwrap_or_default();
    Ok(ask(Path::new(OSASCRIPT), "raise", &tty, pids).await? != Answer::None)
}

/// Runs [`SCRIPT`] in `mode` with `tty` and `pids`.
async fn ask(osascript: &Path, mode: &str, tty: &str, pids: &[u32]) -> Result<Answer, WmError> {
    let mut args = vec!["-l".to_string(), "JavaScript".into(), "-e".into(), SCRIPT.into(), mode.into(), tty.into()];
    args.extend(pids.iter().map(u32::to_string));
    let (success, stdout) = output(osascript, &args).await?;
    if !success {
        return Err(WmError::Protocol("osascript could not raise the window".into()));
    }
    parse_answer(&stdout).ok_or_else(|| WmError::Protocol(format!("osascript answered {:?}", stdout.trim())))
}

/// Reads [`SCRIPT`]'s answer; `None` for anything it does not print.
pub fn parse_answer(answer: &str) -> Option<Answer> {
    match answer.trim() {
        "app" => Some(Answer::App),
        "none" => Some(Answer::None),
        other => {
            let id = other.strip_prefix("window ")?.trim().parse::<u32>().ok()?;
            Some(Answer::Window(WindowId(id.to_string())))
        }
    }
}

/// The tty of the nearest of `pids` that has one, as `/dev/ttys003`; `None` when none has one
/// or `ps` fails.
async fn tty_of(pids: &[u32]) -> Option<String> {
    if pids.is_empty() {
        return None;
    }
    let list = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let args = ["-o".to_string(), "pid=,tty=".into(), "-p".into(), list];
    // `ps` exits with 1 when some of the processes are gone; the others are still listed.
    let (_, stdout) = output(Path::new(PS), &args).await.ok()?;
    tty_in(&stdout, pids)
}

/// Of the lines `<pid> <tty>` that `ps -o pid=,tty=` prints, the tty of the first of `pids`
/// that has one, as a device path. `??` (no tty) and anything that is not a plain tty name
/// (letters and digits) are skipped, so only a real device name reaches the script.
pub fn tty_in(ps: &str, pids: &[u32]) -> Option<String> {
    let ttys: Vec<(u32, &str)> = ps
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            Some((pid, fields.next()?))
        })
        .collect();
    let plain = |tty: &&str| tty.starts_with("tty") && tty.chars().all(|c| c.is_ascii_alphanumeric());
    pids.iter()
        .find_map(|pid| ttys.iter().find(|(listed, _)| listed == pid).map(|(_, tty)| *tty).filter(plain))
        .map(|tty| format!("/dev/{tty}"))
}

/// Runs `program` with `args` (no shell) and returns whether it succeeded and its stdout. `Io`
/// when it cannot start or takes longer than [`TIMEOUT`] (it is killed then).
async fn output(program: &Path, args: &[String]) -> Result<(bool, String), WmError> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(TIMEOUT, command.output())
        .await
        .map_err(|_| WmError::Io(format!("{} did not finish within {} s", program.display(), TIMEOUT.as_secs())))?
        .map_err(|err| WmError::Io(format!("run {}: {err}", program.display())))?;
    Ok((output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tty_of_the_nearest_process_that_has_one() {
        let ps = "  41355 ??      \n  41535 ttys008 \n  41536 ttys008\n";
        assert_eq!(tty_in(ps, &[4_500_001, 41536, 41535, 41355]).as_deref(), Some("/dev/ttys008"));
        assert_eq!(tty_in(ps, &[41355]), None, "the terminal app itself has none");
        assert_eq!(tty_in("", &[41536]), None);
        assert_eq!(tty_in("12 ttys001; rm\n", &[12]), None, "only a plain device name");
        assert_eq!(tty_in("12 ../../x\n", &[12]), None);
        assert_eq!(tty_in("not a pid ttys001\n", &[12]), None);
    }

    #[test]
    fn the_scripts_answers() {
        assert_eq!(parse_answer("window 13732\n"), Some(Answer::Window(WindowId("13732".into()))));
        assert_eq!(parse_answer("app\n"), Some(Answer::App));
        assert_eq!(parse_answer("none"), Some(Answer::None));
        assert_eq!(parse_answer("window x"), None);
        assert_eq!(parse_answer("execution error: not allowed"), None);
    }

    /// The script never holds anything that varies: all of it comes as arguments.
    #[test]
    fn the_script_takes_everything_as_arguments() {
        assert!(SCRIPT.contains("function run(argv)"));
        assert!(!SCRIPT.contains("${"), "no template slots");
        for app in ["com.apple.Terminal", "com.googlecode.iterm2"] {
            assert!(SCRIPT.contains(app), "{app} is chosen by the system's bundle id");
        }
    }

    #[tokio::test]
    async fn without_processes_nothing_runs() {
        assert_eq!(tty_of(&[]).await, None);
        assert_eq!(terminal_window(&[]).await, None);
    }
}
