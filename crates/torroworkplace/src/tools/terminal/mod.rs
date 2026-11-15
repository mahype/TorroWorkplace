//! The assistant's terminal (issue #754, `docs/terminal.md`): commands and scripts on this computer, in an allowed
//! folder with `read_write` access, for the assistant's own conversation only.
//!
//! - [`Terminal::check`] looks at a command before anyone is asked: the folder (`not-found` outside the allowed ones),
//!   the paths its text names against what the suite protects, and the patterns of commands that never run
//!   ([`patterns::hardline`]); both are `forbidden` without a question (the hard limits of the issue). What it
//!   answers ([`Checked`]) says which patterns of dangerous commands the text matches and whether it reads from
//!   outside.
//! - [`Terminal::run`], [`Terminal::start`], [`Terminal::read`] and [`Terminal::stop`] run it and answer the
//!   requests, the output with secrets hidden ([`redact`]) and cut to the limit.
//!
//! Who may run a command, when the person is asked and what "Always" keeps is the core's (`Core::terminal_gate`).
//! Without a sandbox every check of the text is a rule of thumb, and the answers say so.

pub mod patterns;
pub mod process;
pub mod redact;

use std::path::{Component, Path, PathBuf};

use torro_core::i18n::{tr, trf};
use torro_core::settings::{AllowedFolder, FolderAccess, TerminalAlways, TerminalMode};
use torro_ipc::Request;
use torro_ipc::Response;
use torro_ipc::tools::{MAX_COMMAND_ARGS, MAX_COMMAND_CHARS};

use super::folders::Allowed;
pub use process::{Limits, Owner};

/// A command as [`Terminal::check`] let it through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The working folder, resolved.
    pub folder: PathBuf,
    /// What runs.
    pub run: process::Run,
    /// The whole command as one line (the program and its arguments quoted where needed, or the shell line): what
    /// the card shows and what "Always" keeps in the mode "every command".
    pub text: String,
    /// The keys of the patterns of dangerous commands it matches ([`patterns::dangerous`]).
    pub dangerous: Vec<&'static str>,
    /// It runs a program that reads from outside the computer ([`patterns::reads_outside`]).
    pub outside: bool,
}

impl Checked {
    /// What allows this command for good or in a conversation in `mode`: the exact command in its folder, or each
    /// pattern it matches (none for a command that matches none).
    pub fn keys(&self, mode: TerminalMode) -> Vec<TerminalAlways> {
        match mode {
            TerminalMode::Every => vec![TerminalAlways::Command {
                command: self.text.clone(),
                folder: self.folder.to_string_lossy().into_owned(),
            }],
            TerminalMode::Dangerous => {
                self.dangerous.iter().map(|key| TerminalAlways::Pattern { key: (*key).to_string() }).collect()
            }
        }
    }
}

/// The terminal: the commands in the background, the limits and what every command's process gets.
pub struct Terminal {
    background: process::Background,
    limits: Limits,
    env: Vec<(String, String)>,
    home: Option<PathBuf>,
}

impl Terminal {
    /// The person's: the service's basics and `PATH`, the home folder, the limits of `torro_ipc::tools`. In tests a
    /// `PATH` of the system's folders only and no home.
    pub fn system() -> Terminal {
        if cfg!(test) {
            let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
            return Terminal::new(Limits::default(), env, None);
        }
        let env = process::environment(&crate::extensions::env::service_env(), &process::path_dirs());
        Terminal::new(Limits::default(), env, dirs::home_dir())
    }

    /// The terminal with `limits`, `env` for every command and `home` for the patterns and the locked paths.
    pub fn new(limits: Limits, env: Vec<(String, String)>, home: Option<PathBuf>) -> Terminal {
        Terminal { background: process::Background::new(limits), limits, env, home }
    }

    /// `tools.run_command` or `tools.start_command` checked against the allowed `folders` and the `excluded` paths;
    /// `None` for every other request. The answer for a refused one: `error` for a command outside the limits or
    /// with both or neither of `program` and `shell`, `not-found` (and the folders' other codes) for the folder,
    /// `forbidden` for a protected path or a command that never runs.
    pub fn check(
        &self,
        request: &Request,
        folders: &[AllowedFolder],
        excluded: &[PathBuf],
    ) -> Option<Result<Checked, Response>> {
        let (folder, program, args, shell) = match request {
            Request::RunCommand { folder, program, args, shell }
            | Request::StartCommand { folder, program, args, shell } => (folder, program, args, shell),
            _ => return None,
        };
        Some(self.check_command(folder, program.as_deref(), args, shell.as_deref(), folders, excluded))
    }

    fn check_command(
        &self,
        folder: &str,
        program: Option<&str>,
        args: &[String],
        shell: Option<&str>,
        folders: &[AllowedFolder],
        excluded: &[PathBuf],
    ) -> Result<Checked, Response> {
        let (run, text) = match (program, shell) {
            (Some(program), None) if !program.trim().is_empty() => {
                if args.len() > MAX_COMMAND_ARGS {
                    return Err(error(tr("The command has too many arguments.").to_string()));
                }
                let text =
                    std::iter::once(program).chain(args.iter().map(String::as_str)).map(quoted).collect::<Vec<_>>();
                (process::Run::Program { program: program.to_string(), args: args.to_vec() }, text.join(" "))
            }
            (None, Some(line)) if !line.trim().is_empty() => (process::Run::Shell(line.to_string()), line.to_string()),
            _ => return Err(error(tr("Give either a program with its arguments or a shell line.").to_string())),
        };
        if text.chars().count() > MAX_COMMAND_CHARS || text.contains('\0') {
            return Err(error(tr("The command is too long.").to_string()));
        }
        let allowed = Allowed::of(folders, excluded);
        let folder = allowed.existing(folder, FolderAccess::ReadWrite).map_err(|err| err.response())?;
        if names_locked(&text, &folder, self.home.as_deref(), excluded) {
            return Err(forbidden(
                tr("This command names files the suite protects (its own folders, keys or passwords), so it doesn't run.")
                    .to_string(),
            ));
        }
        if let Some(why) = patterns::hardline(&text, self.home.as_deref()) {
            return Err(forbidden(trf("This command never runs here: {why}.", &[("why", &tr(why))])));
        }
        let dangerous = patterns::dangerous(&text, self.home.as_deref());
        Ok(Checked { folder, run, outside: patterns::reads_outside(&text), dangerous, text })
    }

    /// Runs `checked` until its end ([`Response::CommandOutput`]), at most the time limit.
    pub async fn run(&self, checked: &Checked) -> Response {
        // Room for every character in four bytes, and a little more so a secret at the edge is still seen whole.
        let cap = self.limits.max_chars * 4 + 4096;
        match process::run(&checked.run, &checked.folder, &self.env, &self.limits, cap).await {
            Ok(done) => {
                let (output, cut, redacted) = self.answerable(&done.output);
                Response::CommandOutput {
                    output,
                    exit_code: done.exit_code,
                    truncated: cut || done.cut,
                    timed_out: done.timed_out,
                    redacted,
                    // Decision 4: the output counts as content from outside only when the program reads from there.
                    untrusted: checked.outside,
                }
            }
            Err(err) => not_started(&err),
        }
    }

    /// Starts `checked` in the background for `owner` ([`Response::CommandStarted`]); `busy` beyond the limits.
    pub fn start(&self, owner: Owner, checked: &Checked) -> Response {
        match self.background.start(owner, (&checked.run, checked.outside), &checked.folder, &self.env) {
            Ok(id) => Response::CommandStarted { id },
            Err(process::StartError::Busy) => Response::Error {
                code: "busy".into(),
                message: tr("Too many commands run in the background. Stop one first.").into(),
            },
            Err(process::StartError::Io(err)) => not_started(&err),
        }
    }

    /// `tools.read_command_output` for `owner` ([`Response::CommandStatus`]); `not-found` for another's id.
    pub fn read(&self, owner: &Owner, id: &str, from: Option<u64>) -> Response {
        match self.background.read(owner, id, from) {
            Some(snapshot) => self.status(snapshot),
            None => no_command(),
        }
    }

    /// `tools.stop_command` for `owner` ([`Response::CommandStatus`] with what it wrote last); `not-found` as above.
    pub async fn stop(&self, owner: &Owner, id: &str) -> Response {
        match self.background.stop(owner, id).await {
            Some(snapshot) => self.status(snapshot),
            None => no_command(),
        }
    }

    /// Stops the commands of `client` in `conversation` (deleted, or its answer stopped).
    pub async fn end_conversation(&self, client: &str, conversation: &str) {
        self.background.end_conversation(client, conversation).await;
    }

    /// Stops every command in the background (the service ends).
    pub async fn stop_all(&self) {
        self.background.stop_all().await;
    }

    fn status(&self, snapshot: process::Snapshot) -> Response {
        // Read in steps of whole lines up to the limit already: hidden, never cut again, so `next` stays where the
        // text ends (review of #754).
        let (output, redacted) = redact::redact(&snapshot.output);
        let cut = false;
        Response::CommandStatus {
            running: snapshot.running,
            exit_code: snapshot.exit_code,
            output,
            next: snapshot.next,
            truncated: cut || snapshot.truncated,
            redacted,
            untrusted: snapshot.outside,
        }
    }

    /// `output` as it may go out: secrets hidden first, then cut to the limit; whether it was cut and whether
    /// something was hidden.
    fn answerable(&self, output: &str) -> (String, bool, bool) {
        let (hidden, redacted) = redact::redact(output);
        let cut = hidden.chars().count() > self.limits.max_chars;
        let output = match cut {
            true => hidden.chars().take(self.limits.max_chars).collect(),
            false => hidden,
        };
        (output, cut, redacted)
    }
}

/// A word of a command as a shell would read it back: as it is when it is plain, else in single quotes.
fn quoted(word: &str) -> String {
    let plain = !word.is_empty() && word.chars().all(|char| char.is_alphanumeric() || "-_./:=@%+,~".contains(char));
    match plain {
        true => word.to_string(),
        false => format!("'{}'", word.replace('\'', r"'\''")),
    }
}

/// Whether `text` names a path in or of one of `excluded`: each word (split at spaces, quotes, `=` and the shell's
/// signs) with `~`, `$HOME` and `%USERPROFILE%` resolved to `home`, relative ones from `folder`, `.` and `..` worked
/// out, compared without regard to case.
fn names_locked(text: &str, folder: &Path, home: Option<&Path>, excluded: &[PathBuf]) -> bool {
    let locks: Vec<Vec<String>> = excluded.iter().map(|path| folded(path)).collect();
    let home = home.map(Path::to_path_buf);
    let words =
        text.split(|char: char| char.is_whitespace() || "\"'`;|&<>()=,".contains(char)).filter(|word| !word.is_empty());
    words.into_iter().any(|word| {
        let lower = word.to_lowercase();
        let rest = ["~/", "~\\", "$home/", "${home}/", "%userprofile%\\", "%userprofile%/"]
            .iter()
            .find_map(|prefix| lower.starts_with(prefix).then(|| &word[prefix.len()..]));
        let path = match (rest, &home) {
            (Some(rest), Some(home)) => home.join(rest),
            (None, Some(home)) if ["~", "$home", "${home}", "%userprofile%"].contains(&lower.as_str()) => home.clone(),
            _ if Path::new(word).is_absolute() => PathBuf::from(word),
            _ => folder.join(word),
        };
        let parts = folded(&normalized(&path));
        locks.iter().any(|lock| !lock.is_empty() && parts.starts_with(lock))
    })
}

/// `path` with `.` and `..` worked out without asking the system.
fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The parts of `path` in lower case.
fn folded(path: &Path) -> Vec<String> {
    path.components()
        .filter(|part| !matches!(part, Component::CurDir))
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

fn error(message: String) -> Response {
    Response::Error { code: "error".into(), message }
}

fn forbidden(message: String) -> Response {
    Response::Error { code: "forbidden".into(), message }
}

fn no_command() -> Response {
    Response::Error { code: "not-found".into(), message: tr("No such command in the background.").into() }
}

/// A command that could not start, by the kind of the error only: the system's own text may name a path.
fn not_started(err: &std::io::Error) -> Response {
    let message = match err.kind() {
        std::io::ErrorKind::NotFound => tr("The command could not start: the program was not found."),
        std::io::ErrorKind::PermissionDenied => tr("The command could not start: the program may not be run."),
        _ => tr("The command could not start."),
    };
    error(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &Path) -> Vec<AllowedFolder> {
        vec![AllowedFolder { path: path.to_string_lossy().into_owned(), access: FolderAccess::ReadWrite }]
    }

    fn run(folder: &Path, program: Option<&str>, args: &[&str], shell: Option<&str>) -> Request {
        Request::RunCommand {
            folder: folder.to_string_lossy().into_owned(),
            program: program.map(str::to_string),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            shell: shell.map(str::to_string),
        }
    }

    fn code(answer: Result<Checked, Response>) -> String {
        match answer {
            Err(Response::Error { code, .. }) => code,
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn a_command_is_checked_before_anyone_is_asked() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().canonicalize().expect("resolved");
        let keys = root.join(".ssh");
        std::fs::create_dir(&keys).expect("keys");
        let terminal = Terminal::new(Limits::default(), Vec::new(), Some(root.clone()));
        let (folders, excluded) = (folder(&root), vec![keys.clone()]);
        let check = |request: Request| terminal.check(&request, &folders, &excluded).expect("a command");
        let checked = check(run(&root, Some("ls"), &["-la", "my file"], None)).expect("fine");
        assert_eq!((checked.text.as_str(), checked.folder.clone()), ("ls -la 'my file'", root.clone()));
        assert!(checked.dangerous.is_empty() && !checked.outside);
        let risky = check(run(&root, None, &[], Some("rm -rf build/ && curl x"))).expect("asks, not refused");
        assert_eq!((risky.dangerous.clone(), risky.outside), (vec!["recursive delete"], true));
        assert_eq!(risky.keys(TerminalMode::Dangerous), [TerminalAlways::Pattern { key: "recursive delete".into() }]);
        assert_eq!(code(check(run(&root, Some("ls"), &[], Some("ls")))), "error", "both");
        assert_eq!(code(check(run(&root, None, &[], None))), "error", "neither");
        assert_eq!(code(check(run(Path::new("/"), Some("ls"), &[], None))), "not-found", "outside the folders");
        assert_eq!(code(check(run(&keys, Some("ls"), &[], None))), "not-found", "a locked folder");
        assert_eq!(code(check(run(&root, Some("cat"), &[".ssh/id_ed25519"], None))), "forbidden", "relative");
        assert_eq!(code(check(run(&root, None, &[], Some("cat ~/.SSH/config")))), "forbidden", "by home, any case");
        assert_eq!(code(check(run(&root, None, &[], Some("cat sub/../.ssh/x")))), "forbidden", "with ..");
        assert_eq!(code(check(run(&root, None, &[], Some("rm -rf /")))), "forbidden", "never runs");
        let long = "x".repeat(MAX_COMMAND_CHARS + 1);
        assert_eq!(code(check(run(&root, Some("echo"), &[&long], None))), "error");
        assert!(terminal.check(&Request::Ping, &folders, &excluded).is_none());
    }

    /// Erledigt points 6 and 7 of issue #754: output over the limit is cut and said to be; a token reaches the
    /// model only hidden.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_output_is_hidden_and_cut_before_it_goes_out() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().canonicalize().expect("resolved");
        let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
        let limits = Limits { max_chars: 50, ..Limits::default() };
        let terminal = Terminal::new(limits, env, None);
        let check = |line: &str| {
            terminal.check(&run(&root, None, &[], Some(line)), &folder(&root), &[]).expect("a command").expect("fine")
        };
        let token = "ghp_0123456789abcdefghijABCDEFGHIJ0123";
        let answer = terminal.run(&check(&format!("echo {token}"))).await;
        let Response::CommandOutput { output, exit_code, redacted, truncated, untrusted, .. } = answer else {
            panic!("{answer:?}")
        };
        assert!(!output.contains(token) && redacted && !truncated && !untrusted && exit_code == Some(0), "{output}");
        let answer = terminal.run(&check("yes | head -c 500")).await;
        let Response::CommandOutput { output, truncated, .. } = answer else { panic!("{answer:?}") };
        assert!(truncated && output.chars().count() == 50);
    }
}
