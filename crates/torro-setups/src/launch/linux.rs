//! Start commands on Linux.
//!
//! Task 1.12 in `docs/tasks/phase-1.md`. `/proc` and the `.desktop` search paths are
//! parameters so tests can use a fake directory tree.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Field codes of the `Exec=` key (Desktop Entry spec). We start nothing with files or URLs,
/// so every code is removed.
const FIELD_CODES: &str = "fFuUdDnNickvm";

/// Words that mark an argument as a credential, matched case-insensitively against the part
/// before `=`. Mirrors `SECRET` in `apps/workspace-setups/workspace_setups/commands.py`.
const SECRET_WORDS: [&str; 8] = ["token", "password", "passwd", "secret", "apikey", "api-key", "api_key", "auth"];

/// The command line of `pid` from `<proc_root>/<pid>/cmdline` (NUL-separated), with
/// [`strip_secrets`] applied; `None` when unreadable or empty.
pub fn command_from_proc(proc_root: &Path, pid: u32) -> Option<Vec<String>> {
    let raw = fs::read(proc_root.join(pid.to_string()).join("cmdline")).ok()?;
    // Every argument is followed by a NUL, so the last one is the terminator, not a separator.
    let data: &[u8] = raw.strip_suffix(&[0u8]).unwrap_or(&raw);
    let mut args: Vec<String> =
        data.split(|byte| *byte == 0).map(|part| String::from_utf8_lossy(part).into_owned()).collect();
    // Chromium, Electron apps and others rewrite their argv into one space-separated string.
    if let [single] = args.as_slice()
        && single.contains(' ')
        && !Path::new(single).exists()
    {
        args = single.split_whitespace().map(str::to_string).collect();
    }
    let args = strip_secrets(args);
    args.first().is_some_and(|program| !program.is_empty()).then_some(args)
}

/// The `Exec=` line of the `.desktop` entry whose file name or `StartupWMClass` matches
/// `class` (case-insensitive), split into arguments with field codes (`%u`, `%F`, …)
/// removed. Searches `dirs` in order.
pub fn desktop_command(dirs: &[PathBuf], class: &str) -> Option<Vec<String>> {
    let class = class.to_lowercase();
    if class.is_empty() {
        return None;
    }
    // A `StartupWMClass` match is exact and wins at once. A match on the file name or the
    // program name is only a fallback; the first one found in search order is kept.
    let mut by_file_name = None;
    let mut by_executable = None;
    for path in dirs.iter().flat_map(|dir| desktop_files(dir)) {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let fields = desktop_entry_fields(&text);
        let Some(args) = fields.get("Exec").and_then(|exec| exec_arguments(exec)) else {
            continue;
        };
        // Arguments that look like credentials must not end up in the saved template.
        let args = strip_secrets(args);
        if fields.get("StartupWMClass").is_some_and(|wm_class| wm_class.to_lowercase() == class) {
            return Some(args);
        }
        let stem = path.file_stem().map(|stem| stem.to_string_lossy().to_lowercase());
        let executable = args
            .first()
            .and_then(|program| Path::new(program).file_name())
            .map(|name| name.to_string_lossy().to_lowercase());
        if by_file_name.is_none() && stem.as_deref() == Some(class.as_str()) {
            by_file_name = Some(args.clone());
        }
        if by_executable.is_none() && executable.as_deref() == Some(class.as_str()) {
            by_executable = Some(args);
        }
    }
    by_file_name.or(by_executable)
}

/// The standard `.desktop` directories: `$XDG_DATA_HOME/applications`
/// (`~/.local/share/applications`), then each of `$XDG_DATA_DIRS` (`/usr/local/share`,
/// `/usr/share`) + `/applications`.
pub fn desktop_dirs(env: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    // The XDG base directory spec says relative paths in these variables are invalid and
    // must be ignored, and an empty value means "not set".
    let absolute = |value: Option<String>| {
        value.filter(|value| !value.is_empty()).map(PathBuf::from).filter(|path| path.is_absolute())
    };
    let data_home =
        absolute(env("XDG_DATA_HOME")).or_else(|| absolute(env("HOME")).map(|home| home.join(".local/share")));
    let data_dirs: Vec<PathBuf> = match env("XDG_DATA_DIRS").filter(|value| !value.is_empty()) {
        Some(value) => value.split(':').map(PathBuf::from).filter(|path| path.is_absolute()).collect(),
        None => vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")],
    };
    data_home.into_iter().chain(data_dirs).map(|dir| dir.join("applications")).collect()
}

/// Drops arguments that look like credentials: `--password x`, `--password=x`, `--token …`,
/// `token=…`, `secret=…`, `apikey=…` and the like (case-insensitive).
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

/// `pid` and all its ancestors (via `PPid:` in `<proc_root>/<pid>/status`), for matching
/// a new window to the process we started.
pub fn process_ancestors(proc_root: &Path, pid: u32) -> HashSet<u32> {
    let mut ancestors = HashSet::from([pid]);
    let mut current = pid;
    // Stops at init (pid 1), which every process descends from and which is no use for
    // matching, at a missing status file, and at a cycle.
    while let Some(parent) = parent_pid(proc_root, current) {
        if parent <= 1 || !ancestors.insert(parent) {
            break;
        }
        current = parent;
    }
    ancestors
}

/// Flags that contain a secret word but carry no secret (Chromium's keyring backend).
const NOT_SECRET: &[&str] = &["password-store"];

fn is_secret_key(key: &str) -> bool {
    let lowered = key.to_lowercase();
    if NOT_SECRET.contains(&lowered.trim_start_matches('-')) {
        return false;
    }
    SECRET_WORDS.iter().any(|word| lowered.contains(word))
}

fn parent_pid(proc_root: &Path, pid: u32) -> Option<u32> {
    let status = fs::read_to_string(proc_root.join(pid.to_string()).join("status")).ok()?;
    status.lines().find_map(|line| line.strip_prefix("PPid:"))?.trim().parse().ok()
}

/// The `*.desktop` files in `dir`, sorted so the search is repeatable. Empty when `dir` is
/// missing or unreadable.
fn desktop_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("desktop"))
        .collect();
    files.sort();
    files
}

/// The keys of the `[Desktop Entry]` group. Other groups (`[Desktop Action …]`) are skipped,
/// and the first occurrence of a key wins.
fn desktop_entry_fields(text: &str) -> HashMap<&str, &str> {
    let mut fields = HashMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        if let Some(group) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            in_entry = group == "Desktop Entry";
            continue;
        }
        if !in_entry || line.starts_with([' ', '\t', '#']) {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields.entry(key).or_insert(value.trim());
        }
    }
    fields
}

/// The arguments of an `Exec=` value with field codes handled; `None` when the value does not
/// parse or has no program left.
fn exec_arguments(exec: &str) -> Option<Vec<String>> {
    let mut tokens = split_exec(exec)?.into_iter();
    let program = expand_field_codes(&tokens.next()?)?;
    if program.is_empty() {
        return None;
    }
    let mut args = vec![program];
    args.extend(tokens.filter_map(|token| expand_field_codes(&token)));
    Some(args)
}

/// Splits an `Exec=` value into words the way a POSIX shell would for simple cases: whitespace
/// separates, double quotes group (with `\"`, `` \` ``, `\$`, `\\` escapes), single quotes
/// group literally, and a backslash escapes the next character. `None` for an unterminated
/// quote or escape.
fn split_exec(exec: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), _) if c == open => quote = None,
            (Some('"'), '\\') => {
                let next = chars.next()?;
                if matches!(next, '"' | '`' | '$' | '\\') {
                    current.push(next);
                } else {
                    current.push('\\');
                    current.push(next);
                }
            }
            (Some(_), _) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                current.push(chars.next()?);
                in_word = true;
            }
            (None, _) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            (None, _) => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if in_word {
        words.push(current);
    }
    Some(words)
}

/// Handles field codes in one `Exec=` word: `%%` becomes `%`; a word containing any other
/// code (`%u`, `--name=%c`, …) is dropped, returning `None`.
fn expand_field_codes(word: &str) -> Option<String> {
    let mut out = String::with_capacity(word.len());
    let mut has_code = false;
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some(code) if FIELD_CODES.contains(code) => has_code = true,
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    (!has_code).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake `/proc` with `<pid>/cmdline` and `<pid>/status` files.
    fn fake_proc(entries: &[(u32, &[u8], Option<&str>)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (pid, cmdline, status) in entries {
            let process = dir.path().join(pid.to_string());
            fs::create_dir_all(&process).expect("process dir");
            fs::write(process.join("cmdline"), cmdline).expect("cmdline");
            if let Some(status) = status {
                fs::write(process.join("status"), status).expect("status");
            }
        }
        dir
    }

    /// A status file as the kernel writes it, with only the lines we read.
    fn status(name: &str, ppid: u32) -> String {
        format!("Name:\t{name}\nState:\tS (sleeping)\nPPid:\t{ppid}\nUid:\t1000\n")
    }

    /// A `.desktop` directory with the given `(file name, content)` pairs.
    fn desktop_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, content) in files {
            fs::write(dir.path().join(name), content).expect("desktop file");
        }
        dir
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| word.to_string()).collect()
    }

    fn fake_env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn cmdline_is_split_on_nul() {
        let dir = fake_proc(&[(42, b"/usr/bin/foo\0--flag\0value with space\0", None)]);
        assert_eq!(command_from_proc(dir.path(), 42), Some(words(&["/usr/bin/foo", "--flag", "value with space"])));
    }

    #[test]
    fn empty_or_missing_cmdline_gives_none() {
        let dir = fake_proc(&[(1, b"", None), (2, b"\0", None)]);
        assert_eq!(command_from_proc(dir.path(), 1), None, "empty cmdline (kernel thread)");
        assert_eq!(command_from_proc(dir.path(), 2), None, "cmdline with an empty program");
        assert_eq!(command_from_proc(dir.path(), 3), None, "no such process");
    }

    #[test]
    fn cmdline_drops_secrets() {
        let dir = fake_proc(&[(7, b"/usr/bin/app\0--password\0hunter2\0--profile=Work\0", None)]);
        assert_eq!(command_from_proc(dir.path(), 7), Some(words(&["/usr/bin/app", "--profile=Work"])));
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
    fn rewritten_cmdline_is_split_on_spaces() {
        let proc = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proc.path().join("42")).unwrap();
        std::fs::write(proc.path().join("42/cmdline"), b"/usr/lib/chromium/chromium --ozone-platform=wayland\0")
            .unwrap();
        assert_eq!(
            command_from_proc(proc.path(), 42),
            Some(words(&["/usr/lib/chromium/chromium", "--ozone-platform=wayland"]))
        );
    }

    #[test]
    fn strip_secrets_keeps_the_program_name() {
        assert_eq!(strip_secrets(words(&["/opt/auth-helper/run"])), words(&["/opt/auth-helper/run"]));
        assert!(strip_secrets(Vec::new()).is_empty());
    }

    #[test]
    fn desktop_entry_is_found_by_file_name() {
        let dir = desktop_dir(&[("foo.desktop", "[Desktop Entry]\nName=Foo\nExec=foo --bar %U\n")]);
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(desktop_command(&dirs, "foo"), Some(words(&["foo", "--bar"])));
        assert_eq!(desktop_command(&dirs, "FOO"), Some(words(&["foo", "--bar"])), "case-insensitive");
        assert_eq!(desktop_command(&dirs, "bar"), None);
        assert_eq!(desktop_command(&dirs, ""), None);
    }

    #[test]
    fn desktop_entry_is_found_by_startup_wm_class() {
        let dir = desktop_dir(&[(
            "org.example.App.desktop",
            "[Desktop Entry]\nExec=/opt/app/bin/app --x %F\nStartupWMClass=Example\n",
        )]);
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(desktop_command(&dirs, "example"), Some(words(&["/opt/app/bin/app", "--x"])));
    }

    #[test]
    fn startup_wm_class_beats_a_file_name_match() {
        let dir = desktop_dir(&[
            ("mail.desktop", "[Desktop Entry]\nExec=mail-old\n"),
            ("other.desktop", "[Desktop Entry]\nExec=mail-new\nStartupWMClass=mail\n"),
        ]);
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(desktop_command(&dirs, "mail"), Some(words(&["mail-new"])));
    }

    #[test]
    fn earlier_directory_wins_for_a_file_name_match() {
        let user = desktop_dir(&[("foo.desktop", "[Desktop Entry]\nExec=user-foo\n")]);
        let system = desktop_dir(&[("foo.desktop", "[Desktop Entry]\nExec=system-foo\n")]);
        let dirs = [user.path().to_path_buf(), system.path().to_path_buf()];
        assert_eq!(desktop_command(&dirs, "foo"), Some(words(&["user-foo"])));
    }

    #[test]
    fn exec_is_read_from_the_desktop_entry_group_only() {
        let dir = desktop_dir(&[(
            "foo.desktop",
            "[Desktop Entry]\nName=Foo\nExec=foo\n\n[Desktop Action New]\nExec=foo --new-window\n",
        )]);
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(desktop_command(&dirs, "foo"), Some(words(&["foo"])));
    }

    #[test]
    fn exec_field_codes_are_removed() {
        let dir = desktop_dir(&[(
            "codes.desktop",
            "[Desktop Entry]\nExec=app %f %F %u %U %d %D %n %N %i %c %k %v %m --pct=100%% --title=%c %u\n",
        )]);
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(desktop_command(&dirs, "codes"), Some(words(&["app", "--pct=100%"])));
    }

    #[test]
    fn quoted_exec_arguments_are_kept_whole() {
        let exec = r#"Exec="/opt/my app/run" --name "two words" 'single q' "esc \"q\" \$x" "%u" plain\ word"#;
        let dir = desktop_dir(&[("quoted.desktop", &format!("[Desktop Entry]\n{exec}\n"))]);
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(
            desktop_command(&dirs, "quoted"),
            Some(words(&["/opt/my app/run", "--name", "two words", "single q", "esc \"q\" $x", "plain word"]))
        );
    }

    #[test]
    fn exec_secrets_are_removed() {
        let dir = desktop_dir(&[("login.desktop", "[Desktop Entry]\nExec=login --token abc --user me\n")]);
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(desktop_command(&dirs, "login"), Some(words(&["login", "--user", "me"])));
    }

    #[test]
    fn unparsable_or_empty_exec_is_skipped() {
        let dir = desktop_dir(&[
            ("broken.desktop", "[Desktop Entry]\nExec=broken \"unterminated\n"),
            ("empty.desktop", "[Desktop Entry]\nExec=%u\n"),
            ("noexec.desktop", "[Desktop Entry]\nName=none\n"),
        ]);
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(desktop_command(&dirs, "broken"), None);
        assert_eq!(desktop_command(&dirs, "empty"), None);
        assert_eq!(desktop_command(&dirs, "noexec"), None);
    }

    #[test]
    fn missing_directory_gives_none() {
        let dirs = [PathBuf::from("/nonexistent/torro-test/applications")];
        assert_eq!(desktop_command(&dirs, "foo"), None);
    }

    #[test]
    fn ancestors_walk_up_to_init() {
        let dir = fake_proc(&[
            (100, b"", Some(&status("child", 50))),
            (50, b"", Some(&status("parent", 40))),
            (40, b"", Some(&status("grandparent", 1))),
            (1, b"", Some(&status("init", 0))),
        ]);
        assert_eq!(process_ancestors(dir.path(), 100), HashSet::from([100, 50, 40]));
    }

    #[test]
    fn ancestors_stop_at_a_missing_status_file() {
        let dir = fake_proc(&[(200, b"", Some(&status("orphan", 300)))]);
        assert_eq!(process_ancestors(dir.path(), 200), HashSet::from([200, 300]));
    }

    #[test]
    fn ancestors_stop_at_a_cycle() {
        let dir = fake_proc(&[
            (300, b"", Some(&status("a", 301))),
            (301, b"", Some(&status("b", 300))),
            (400, b"", Some(&status("self", 400))),
        ]);
        assert_eq!(process_ancestors(dir.path(), 300), HashSet::from([300, 301]));
        assert_eq!(process_ancestors(dir.path(), 400), HashSet::from([400]));
    }

    #[test]
    fn ancestors_of_an_unknown_pid_are_just_the_pid() {
        let dir = fake_proc(&[]);
        assert_eq!(process_ancestors(dir.path(), 999), HashSet::from([999]));
    }

    #[test]
    fn desktop_dirs_defaults() {
        let env = fake_env(&[("HOME", "/home/anna")]);
        assert_eq!(
            desktop_dirs(env),
            [
                PathBuf::from("/home/anna/.local/share/applications"),
                PathBuf::from("/usr/local/share/applications"),
                PathBuf::from("/usr/share/applications"),
            ]
        );
    }

    #[test]
    fn desktop_dirs_without_home_skips_the_user_directory() {
        assert_eq!(
            desktop_dirs(fake_env(&[])),
            [PathBuf::from("/usr/local/share/applications"), PathBuf::from("/usr/share/applications")]
        );
    }

    #[test]
    fn desktop_dirs_honour_xdg_overrides() {
        let env =
            fake_env(&[("HOME", "/home/anna"), ("XDG_DATA_HOME", "/data/home"), ("XDG_DATA_DIRS", "/opt/a:/opt/b")]);
        assert_eq!(
            desktop_dirs(env),
            [
                PathBuf::from("/data/home/applications"),
                PathBuf::from("/opt/a/applications"),
                PathBuf::from("/opt/b/applications"),
            ]
        );
    }

    #[test]
    fn desktop_dirs_ignore_empty_and_relative_xdg_values() {
        let env = fake_env(&[
            ("HOME", "/home/anna"),
            ("XDG_DATA_HOME", "relative/share"),
            ("XDG_DATA_DIRS", "relative:/opt/a::"),
        ]);
        assert_eq!(
            desktop_dirs(env),
            [PathBuf::from("/home/anna/.local/share/applications"), PathBuf::from("/opt/a/applications")]
        );
        let empty = fake_env(&[("HOME", "/home/anna"), ("XDG_DATA_HOME", ""), ("XDG_DATA_DIRS", "")]);
        assert_eq!(desktop_dirs(empty).len(), 3, "empty values mean not set");
    }
}
