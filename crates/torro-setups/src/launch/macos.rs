//! Start commands on macOS.
//!
//! Task X.8 in `docs/tasks/plattformen.md`. A window's process (its pid comes from AeroSpace)
//! tells its program and arguments through `sysinfo`. Programs inside an app bundle
//! (`…/Foo.app/Contents/MacOS/foo`) are started again with `open`, the way Finder and the Dock do
//! it; anything else is started directly. The decisions are pure functions so their tests run on
//! every system.

use std::path::{Path, PathBuf};

/// The app bundle that holds the executable `exe`: the outermost `Foo.app` of a path shaped like
/// `…/Foo.app/Contents/MacOS/foo`. `None` for a program outside a bundle.
///
/// The outermost one, because Electron and Chromium put helper apps inside the main bundle
/// (`Foo.app/Contents/Frameworks/Foo Helper.app/Contents/MacOS/Foo Helper`); the one to start is
/// `Foo.app`. The `Contents/MacOS` part is required so a folder that merely ends in `.app` (say a
/// build output directory) is not taken for a bundle.
pub fn app_bundle(exe: &Path) -> Option<PathBuf> {
    let parts: Vec<String> = exe.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    // The file system is case-insensitive by default, so is the extension. A bare ".app" is a
    // hidden folder, not a bundle.
    let is_app = |name: &str| name.len() > 4 && name.to_lowercase().ends_with(".app");
    let in_contents = |at: usize| parts.get(at + 1).is_some_and(|next| next == "Contents");
    // Some bundle, at any depth, must hold the executable itself: `X.app/Contents/MacOS/<file>`.
    let holds_executable = (0..parts.len()).any(|at| {
        is_app(&parts[at])
            && in_contents(at)
            && parts.get(at + 2).is_some_and(|dir| dir == "MacOS")
            && parts.len() > at + 3
    });
    if !holds_executable {
        return None;
    }
    let outermost = (0..parts.len()).find(|&at| is_app(&parts[at]) && in_contents(at))?;
    Some(parts[..=outermost].iter().collect())
}

/// The command that starts the program with executable `exe` and command line `cmd` (first word
/// included, as the system reports it) again; empty when nothing is known.
///
/// - In an app bundle: `open -n -a <Foo.app> [--args <arguments>]`.
/// - Otherwise: the executable and its arguments.
///
/// `-n` opens a new instance. Without it `open -a` only brings a running app to the front, so a
/// template window whose app is open elsewhere (or open without windows) would never get its
/// window, and the load would run into its timeout. The price: apps that cannot run twice get a
/// second process that hands over to the first and quits, which also opens a window for most of
/// them; Finder is the exception that opens nothing. `--args` passes the arguments on to that new
/// instance (a browser profile, say), which is why `-n` is needed for them at all.
///
/// Arguments that look like credentials are removed, and the process serial number
/// (`-psn_0_12345`) that older launchers add is dropped: both would be wrong in a template.
pub fn command_from_process(exe: Option<&Path>, cmd: &[String]) -> Vec<String> {
    let command = super::program_and_args(exe, cmd);
    let Some(bundle) = command.first().and_then(|program| app_bundle(Path::new(program))) else {
        return command;
    };
    let mut open = vec!["open".to_string(), "-n".to_string(), "-a".to_string(), bundle.to_string_lossy().into_owned()];
    let args: Vec<String> = command.into_iter().skip(1).filter(|arg| !arg.starts_with("-psn_")).collect();
    if !args.is_empty() {
        open.push("--args".to_string());
        open.extend(args);
    }
    open
}

/// The command that starts the process `pid` again, see [`command_from_process`]; `None` when
/// there is no such process or the system does not tell us its program.
#[cfg(target_os = "macos")]
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
    fn bundle_is_found_from_the_executable_inside() {
        assert_eq!(
            app_bundle(Path::new("/Applications/Foo.app/Contents/MacOS/foo")),
            Some(PathBuf::from("/Applications/Foo.app"))
        );
        assert_eq!(
            app_bundle(Path::new("/Users/anna/Applications/Visual Studio Code.app/Contents/MacOS/Electron")),
            Some(PathBuf::from("/Users/anna/Applications/Visual Studio Code.app"))
        );
    }

    #[test]
    fn the_outermost_bundle_wins_over_helper_apps() {
        let helper = "/Applications/Foo.app/Contents/Frameworks/Foo Helper.app/Contents/MacOS/Foo Helper";
        assert_eq!(app_bundle(Path::new(helper)), Some(PathBuf::from("/Applications/Foo.app")));
    }

    #[test]
    fn the_extension_is_not_case_sensitive() {
        assert_eq!(
            app_bundle(Path::new("/Applications/Foo.APP/Contents/MacOS/foo")),
            Some(PathBuf::from("/Applications/Foo.APP"))
        );
    }

    #[test]
    fn programs_outside_a_bundle_have_none() {
        assert_eq!(app_bundle(Path::new("/usr/bin/vim")), None);
        assert_eq!(app_bundle(Path::new("/opt/homebrew/bin/foot")), None);
        assert_eq!(app_bundle(Path::new("relative/foo")), None);
        assert_eq!(app_bundle(Path::new("")), None);
    }

    #[test]
    fn a_folder_called_app_is_no_bundle() {
        // Not shaped like a bundle: no `Contents/MacOS`, or only `.app` as a name.
        assert_eq!(app_bundle(Path::new("/Users/anna/build.app/bin/foo")), None);
        assert_eq!(app_bundle(Path::new("/Users/anna/.app/Contents/MacOS/foo")), None);
        assert_eq!(app_bundle(Path::new("/Applications/Foo.app/Contents/Resources/tool")), None);
        assert_eq!(app_bundle(Path::new("/Applications/Foo.app/Contents/MacOS")), None);
        assert_eq!(app_bundle(Path::new("/Applications/Foo.app")), None);
    }

    #[test]
    fn a_bundle_program_is_started_with_open() {
        let exe = Path::new("/Applications/Foo.app/Contents/MacOS/foo");
        assert_eq!(
            command_from_process(Some(exe), &words(&["/Applications/Foo.app/Contents/MacOS/foo"])),
            words(&["open", "-n", "-a", "/Applications/Foo.app"])
        );
    }

    #[test]
    fn arguments_of_a_bundle_program_follow_args() {
        let exe = Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
        let cmd = words(&[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "--profile-directory=Profile 2",
            "-psn_0_12345",
        ]);
        assert_eq!(
            command_from_process(Some(exe), &cmd),
            words(&["open", "-n", "-a", "/Applications/Google Chrome.app", "--args", "--profile-directory=Profile 2"])
        );
    }

    #[test]
    fn a_program_outside_a_bundle_keeps_path_and_arguments() {
        let exe = Path::new("/opt/homebrew/bin/foot");
        assert_eq!(
            command_from_process(Some(exe), &words(&["foot", "--title", "work"])),
            words(&["/opt/homebrew/bin/foot", "--title", "work"])
        );
    }

    #[test]
    fn credentials_are_removed_in_both_forms() {
        let bundle = Path::new("/Applications/Foo.app/Contents/MacOS/foo");
        assert_eq!(
            command_from_process(Some(bundle), &words(&["foo", "--password", "x", "--lang=de"])),
            words(&["open", "-n", "-a", "/Applications/Foo.app", "--args", "--lang=de"])
        );
        assert_eq!(
            command_from_process(Some(Path::new("/usr/bin/foo")), &words(&["foo", "--token=abc", "--lang=de"])),
            words(&["/usr/bin/foo", "--lang=de"])
        );
    }

    #[test]
    fn nothing_known_gives_an_empty_command() {
        assert!(command_from_process(None, &[]).is_empty());
    }

    /// The test process is not in a bundle: its command is its own program and arguments.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_own_process_is_found_by_pid() {
        let command = command_for_pid(std::process::id()).expect("the own process is known");
        let own = std::env::current_exe().expect("current exe");
        let program = std::fs::canonicalize(&command[0]).expect("the program exists");
        assert_eq!(program, std::fs::canonicalize(own).expect("current exe exists"));
        let args = super::super::strip_secrets(std::env::args().collect());
        assert_eq!(command[1..], args[1..]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_pid_that_does_not_exist_gives_none() {
        // Above the pid limit of macOS (99999), so no process can have it.
        assert_eq!(command_for_pid(u32::MAX - 1), None);
    }
}
