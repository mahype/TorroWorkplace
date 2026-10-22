//! The suite's apps as the core reports them (`Request::ListApps`): every app of
//! `torro_core::AppId::ALL` except the core itself, and whether it is installed.
//!
//! "Installed" means the binary was found next to the core's executable or on `PATH`, the same
//! places `Request::OpenApp` starts apps from, so a listed app that is installed can be opened.
//!
//! The inbox is three entries (task E8.1): its mail window, its notifications window and its hub,
//! one binary `torroinbox` with three window classes. Mail and App Feed also have programs of their
//! own (issue #279, `torro-inbox-mail`, `torro-inbox-notifications`, [`AppId::program_file`]): the core
//! starts their windows with them when they are there ([`program_for`]), else with `torroinbox` as
//! before, and either one makes the entry installed. `apps.list_apps` still names `torroinbox`.
//!
//! `apps.open_app` with a `target` (task E8.7a) opens a mail or a task, (task WZ.8) a new mail
//! with a file of the tools' register attached, and (issue #243) the core's dialog to edit a
//! workspace template: [`Target`] reads what the request names, the core
//! looks it up, and [`target_command`] builds the command line from what it found ([`Opened`]),
//! never from the target's text.
//!
//! [`Listener`] is the one app the core keeps running itself: the voice listener
//! `torroassistant --listen` (task C.9), started while the setting `voice.enabled` is on and
//! stopped when it goes off. It reports what it does on its output; the tray shows it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;
use torro_core::AppId;
use torro_ipc::{AppInfo, Response, VoiceState};

/// The suite's apps with their install state, looked up now (a handful of file checks).
pub fn list() -> Vec<AppInfo> {
    list_in(&search_dirs())
}

/// [`list`] with the folders to look in; tests pass temporary ones.
fn list_in(dirs: &[PathBuf]) -> Vec<AppInfo> {
    AppId::ALL
        .iter()
        // The core is the service answering this; it is not one of the apps it opens.
        .filter(|app| **app != AppId::WORKPLACE)
        .map(|app| AppInfo {
            binary: app.binary.to_string(),
            name: app.display_name.to_string(),
            window_class: app.window_class.to_string(),
            // Either program opens the window (issue #279); the answer keeps naming `torroinbox`.
            installed: is_installed(app.binary, dirs) || is_installed(app.program_file(), dirs),
        })
        .collect()
}

/// The folders an app binary is looked for in: the folder of this executable, then `PATH`.
pub(crate) fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        dirs.push(dir);
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs
}

/// Whether a file named `binary` (plus `.exe` on Windows) exists in one of `dirs`.
fn is_installed(binary: &str, dirs: &[PathBuf]) -> bool {
    find_in(binary, dirs).is_some()
}

/// The file named `binary` (plus `.exe` on Windows) in the first of `dirs` that has it.
fn find_in(binary: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let file = format!("{binary}{}", std::env::consts::EXE_SUFFIX);
    dirs.iter().map(|dir| dir.join(&file)).find(|path| path.is_file())
}

/// The program that starts `app`'s window (`Core::open_app`), looked up in `dirs` (in the core
/// [`search_dirs`]) folder by folder (issue #279): in each, the entry's own program
/// ([`AppId::program_file`], `torro-inbox-mail`) first, then [`AppId::binary`] (`torroinbox`), so the
/// programs of one installation go together. By its path when found, else [`AppId::binary`] by name,
/// which the system then looks up on `PATH` as before.
///
/// The command line is [`AppId::command`] of it either way (`torro-inbox-mail --view mail`): the own
/// program puts its `--view` in front only when none is given, so the line means the same with both
/// programs and [`target_command`] keeps one rule. For entries without a program of their own this is
/// the binary as before (`torrocalendar`).
pub fn program_for(app: AppId, dirs: &[PathBuf]) -> String {
    let found = dirs.iter().find_map(|dir| {
        let dir = std::slice::from_ref(dir);
        find_in(app.program_file(), dir).or_else(|| find_in(app.binary, dir))
    });
    found.map_or_else(|| app.binary.to_string(), |path| path.to_string_lossy().into_owned())
}

/// The program that starts a window showing `opened` (`apps.open_app` with a target, then
/// [`target_command`]), looked up in `dirs`: [`program_for`] the entry of [`Opened::app`], so a mail
/// opens in `torro-inbox-mail` when it is there; for a file always `torroinbox`, because the compose
/// window (`--compose`) is no window of the mail program.
pub fn target_program(opened: &Opened, dirs: &[PathBuf]) -> String {
    match opened {
        Opened::File { .. } => {
            let binary = opened.app().binary;
            find_in(binary, dirs).map_or_else(|| binary.to_string(), |path| path.to_string_lossy().into_owned())
        }
        _ => program_for(opened.app(), dirs),
    }
}

/// Where the listener's program comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Program {
    /// `torroassistant` next to the core or on `PATH`, looked up each time it is needed, so an
    /// app installed later is found.
    Installed,
    /// This file (tests: a fake program).
    #[cfg(all(test, unix))]
    At(PathBuf),
    /// None at all (tests that do not start it).
    Missing,
}

/// The listener as it runs.
#[derive(Debug, Default)]
struct Running {
    child: Option<Child>,
    /// Counts the starts, so the output of a listener stopped meanwhile changes nothing.
    generation: u64,
    state: VoiceState,
    /// Why it is not running although it should, for people.
    problem: Option<String>,
}

/// The voice listener `torroassistant --listen` (task C.9): started and stopped by the core
/// ([`Listener::follow`]), its state read from its output (one JSON [`VoiceState`] per line), and
/// asked as a helper for `settings.voice_status` and `settings.test_voice` ([`Listener::ask`]).
#[derive(Debug)]
pub struct Listener {
    program: Program,
    running: Arc<Mutex<Running>>,
    /// Told whenever the state changes; the daemon then rebuilds the tray menu.
    changes: watch::Sender<()>,
}

impl Listener {
    /// The installed app's listener.
    pub fn system() -> Listener {
        Listener::with(Program::Installed)
    }

    /// A listener that is never installed: tests start none unless they bring a fake.
    pub fn none() -> Listener {
        Listener::with(Program::Missing)
    }

    /// The listener `program`, for tests.
    #[cfg(all(test, unix))]
    pub fn at(program: PathBuf) -> Listener {
        Listener::with(Program::At(program))
    }

    fn with(program: Program) -> Listener {
        Listener { program, running: Arc::default(), changes: watch::Sender::new(()) }
    }

    /// The program, when it is installed.
    pub fn program(&self) -> Option<PathBuf> {
        match &self.program {
            Program::Installed => find_in(AppId::ASSISTANT.binary, &search_dirs()),
            #[cfg(all(test, unix))]
            Program::At(path) => Some(path.clone()),
            Program::Missing => None,
        }
    }

    /// What the listener does now.
    pub fn state(&self) -> VoiceState {
        self.lock().state
    }

    /// Why the listener does not run although voice is on; `None` when nothing went wrong.
    pub fn problem(&self) -> Option<String> {
        self.lock().problem.clone()
    }

    /// Wakes whenever [`Listener::state`] may have changed.
    pub fn changes(&self) -> watch::Receiver<()> {
        self.changes.subscribe()
    }

    /// Starts the listener when `enabled` and it does not run, stops it when not `enabled`. Must
    /// be called on the core's runtime: its output is read on a task. Without the app nothing
    /// starts, and [`Listener::problem`] says so.
    pub fn follow(&self, enabled: bool) {
        let mut running = self.lock();
        if !enabled {
            if let Some(mut child) = running.child.take() {
                // Ends the process; the microphone closes with it.
                let _ = child.start_kill();
            }
            running.generation += 1;
            running.problem = None;
            self.set(&mut running, VoiceState::Off);
            return;
        }
        if running.child.is_some() {
            return;
        }
        let Some(program) = self.program() else {
            running.problem = Some(format!("{} is not installed", AppId::ASSISTANT.binary));
            self.set(&mut running, VoiceState::Off);
            return;
        };
        let started = Command::new(&program)
            .arg("--listen")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn();
        let mut child = match started {
            Ok(child) => child,
            Err(err) => {
                running.problem = Some(format!("cannot start {}: {err}", program.display()));
                self.set(&mut running, VoiceState::Off);
                return;
            }
        };
        running.generation += 1;
        running.problem = None;
        let generation = running.generation;
        let Some(output) = child.stdout.take() else { return };
        running.child = Some(child);
        drop(running);
        let (shared, changes) = (Arc::clone(&self.running), self.changes.clone());
        tokio::spawn(async move {
            let mut lines = BufReader::new(output).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(state) = serde_json::from_str::<VoiceState>(line.trim()) else { continue };
                let mut running = shared.lock().unwrap_or_else(PoisonError::into_inner);
                if running.generation != generation {
                    return;
                }
                running.state = state;
                changes.send_replace(());
            }
            // The output closed: the listener ended (voice went off, or it could not listen).
            let mut running = shared.lock().unwrap_or_else(PoisonError::into_inner);
            if running.generation != generation {
                return;
            }
            if let Some(mut child) = running.child.take() {
                running.problem = match child.try_wait() {
                    Ok(Some(status)) if status.success() => None,
                    Ok(Some(status)) => Some(format!("the listener stopped ({status})")),
                    _ => {
                        let _ = child.start_kill();
                        Some("the listener stopped".to_string())
                    }
                };
            }
            running.state = VoiceState::Off;
            changes.send_replace(());
        });
    }

    /// Runs the program once with `args` and reads its answer, the last line of its output as a
    /// [`Response`]; waits `wait` at most. For `--voice-status` and `--voice-test`.
    pub async fn ask(&self, args: &[String], wait: Duration) -> Result<Response, String> {
        let program = self.program().ok_or_else(|| format!("{} is not installed", AppId::ASSISTANT.binary))?;
        let child = Command::new(&program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| format!("cannot start {}: {err}", program.display()))?;
        let output = tokio::time::timeout(wait, child.wait_with_output())
            .await
            .map_err(|_| format!("{} did not answer in time", AppId::ASSISTANT.binary))?
            .map_err(|err| err.to_string())?;
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or_default();
        serde_json::from_str(line).map_err(|err| format!("{} answered no voice answer: {err}", AppId::ASSISTANT.binary))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Running> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set(&self, running: &mut Running, state: VoiceState) {
        running.state = state;
        self.changes.send_replace(());
    }
}

/// What `apps.open_app` is asked to show (`target`, task E8.7a), as the request names it. The core
/// looks it up before anything starts (`Core::open_target`): only what it finds goes on the
/// command line ([`Opened`]), never the target's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    /// `mail:<address>:<Message-ID>`: a task's link to a mail, as the account id and the
    /// Message-ID ([`torro_pim::model::Link::mail_ref`]).
    Mail { account: &'a str, message_id: &'a str },
    /// `task:<uid>`: a task by its iCalendar UID.
    Task { uid: &'a str },
    /// `file:<id>`: a file of the tools' register (issue #225), by its id of 8 hex digits, to go
    /// along with a new mail.
    File { id: &'a str },
    /// `template:<id>`: a workspace template (issue #243) by its id, a slug, to edit in the core's
    /// dialog. Always the dialog's mode "Edit", never "Save" (its Cancel deletes the template).
    Template { id: &'a str },
}

/// The start of a task target.
const TASK_TARGET: &str = "task:";

/// The start of a file target.
const FILE_TARGET: &str = "file:";

/// The start of a template target.
const TEMPLATE_TARGET: &str = "template:";

/// How long the id of a file of the register is: 8 hex digits (`crate::tools::files::new_id`).
const FILE_ID_LEN: usize = 8;

impl<'a> Target<'a> {
    /// The target `text` names; `Err` with the forms there are for anything else, for a uid that
    /// is empty or holds control characters, for a file id that is not 8 hex digits, and for a
    /// template id with other characters than `a`–`z`, `0`–`9` and `-` (the form of a template's id).
    pub fn parse(text: &'a str) -> Result<Target<'a>, &'static str> {
        const FORMS: &str = "a target is mail:<address>:<Message-ID>, task:<uid>, file:<id> or template:<id>";
        if let Some(id) = text.strip_prefix(TEMPLATE_TARGET) {
            let slug = !id.is_empty()
                && id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
            return if slug { Ok(Target::Template { id }) } else { Err(FORMS) };
        }
        if let Some(id) = text.strip_prefix(FILE_TARGET) {
            let hex = id.len() == FILE_ID_LEN && id.bytes().all(|byte| byte.is_ascii_hexdigit());
            return if hex { Ok(Target::File { id }) } else { Err(FORMS) };
        }
        if let Some(uid) = text.strip_prefix(TASK_TARGET) {
            return match uid.is_empty() || uid.chars().any(char::is_control) {
                true => Err(FORMS),
                false => Ok(Target::Task { uid }),
            };
        }
        // Only the prefix decides; the parts are borrowed from `text`, not from a temporary link.
        let link = torro_pim::model::Link::new(text);
        let (account, message_id) = link.mail_ref().ok_or(FORMS)?;
        let (account, message_id) = (account.len(), message_id.len());
        Ok(Target::Mail { account: &text[..account], message_id: &text[text.len() - message_id..] })
    }

    /// The entry of the suite this target opens in: the inbox's mail window (its compose window for
    /// a file), the tasks app, or the core (its dialog for a template).
    pub fn app(self) -> AppId {
        match self {
            Target::Mail { .. } | Target::File { .. } => AppId::INBOX_MAIL,
            Target::Task { .. } => AppId::TASKS,
            Target::Template { .. } => AppId::WORKPLACE,
        }
    }

    /// `app` may show it: any window of the inbox (they share a binary) for a mail or a file, the
    /// tasks app for a task, the core (`torroworkplace`) for a template.
    pub fn opens_in(self, app: AppId) -> bool {
        app.binary == self.app().binary
    }
}

/// What a new window shows, as the core found it ([`target_command`]): the store's id of a mail
/// (`m:…`), the uid of a cached task, or the path the tools' register keeps for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    Mail {
        message: String,
    },
    Task {
        uid: String,
    },
    /// A file of the register, to attach to a new mail; never a path from the request.
    File {
        path: String,
    },
    /// A workspace template by the id its store holds, to edit in the core's dialog.
    Template {
        id: String,
    },
}

impl Opened {
    /// The entry of the suite whose window shows it (for a file: the binary of the compose window).
    pub fn app(&self) -> AppId {
        match self {
            Opened::Mail { .. } | Opened::File { .. } => AppId::INBOX_MAIL,
            Opened::Task { .. } => AppId::TASKS,
            Opened::Template { .. } => AppId::WORKPLACE,
        }
    }
}

/// The command line that starts a window of `program` (the path [`target_program`] found: for a mail
/// `torro-inbox-mail` or `torroinbox`, else [`Opened::app`]'s binary) showing `opened`:
/// `torroinbox --view mail --message=m:…` (or `torro-inbox-mail --view mail --message=m:…`),
/// `torrotasks --task=<uid>`, `torroinbox --compose --attach=<path>` or `torroworkplace ui --template=<id>`
/// (the dialog in its mode "Edit"; the mode "Save", `--saved`, is never built here). One argument each,
/// with `=`, so a value that starts with `-` stays a value; started without a shell.
pub fn target_command(program: String, opened: &Opened) -> Vec<String> {
    let (mut command, argument) = match opened {
        Opened::Mail { message } => (opened.app().command(program), format!("--message={message}")),
        Opened::Task { uid } => (opened.app().command(program), format!("--task={uid}")),
        Opened::Template { id } => (opened.app().command(program), format!("--template={id}")),
        // A file opens the compose window, not the mail window the inbox's mail entry names.
        Opened::File { path } => (vec![program, "--compose".to_string()], format!("--attach={path}")),
    };
    command.push(argument);
    command
}

#[cfg(test)]
pub mod tests {
    use std::fs;

    use super::*;

    fn binary(name: &str) -> String {
        format!("{name}{}", std::env::consts::EXE_SUFFIX)
    }

    /// A fake `torroassistant` in `dir` that runs the shell code `body` ([`fake_at`]).
    #[cfg(unix)]
    pub fn fake_program(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("torroassistant");
        fake_at(&path, body);
        path
    }

    /// Makes `path` a program that runs the shell code `body`: a link to
    /// `tests/fixtures/fake-program`, which runs `<path>.sh`. A new executable file each time
    /// would wait for macOS to check it before its first start, one file after another and at
    /// times longer than a test waits; the fixture is checked once. Again on the same `path`, the
    /// code is replaced.
    #[cfg(unix)]
    pub fn fake_at(path: &Path, body: &str) {
        let mut code = path.as_os_str().to_owned();
        code.push(".sh");
        fs::write(&code, format!("{body}\n")).expect("write the program's code");
        if fs::symlink_metadata(path).is_err() {
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-program");
            std::os::unix::fs::symlink(fixture, path).expect("link the fake program");
        }
    }

    /// Waits up to five seconds until `listener` reports `state`.
    #[cfg(unix)]
    pub async fn until(listener: &Listener, changes: &mut watch::Receiver<()>, state: VoiceState) {
        let waited = tokio::time::timeout(Duration::from_secs(5), async {
            while listener.state() != state {
                if changes.changed().await.is_err() {
                    return;
                }
            }
        })
        .await;
        assert!(waited.is_ok() && listener.state() == state, "expected {state:?}, is {:?}", listener.state());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_listener_starts_reports_and_stops() {
        let dir = tempfile::tempdir().expect("temp dir");
        let listener = Listener::at(fake_program(dir.path(), "echo '\"listening\"'; echo '\"recording\"'; sleep 30"));
        let mut changes = listener.changes();
        listener.follow(true);
        until(&listener, &mut changes, VoiceState::Recording).await;
        listener.follow(true);
        assert_eq!(listener.state(), VoiceState::Recording, "started once");
        listener.follow(false);
        assert_eq!(listener.state(), VoiceState::Off);
        assert_eq!(listener.problem(), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_listener_that_stops_says_so() {
        let dir = tempfile::tempdir().expect("temp dir");
        let listener = Listener::at(fake_program(dir.path(), "echo '\"listening\"'; exit 3"));
        let mut changes = listener.changes();
        listener.follow(true);
        let stopped = tokio::time::timeout(Duration::from_secs(5), async {
            while listener.problem().is_none() {
                if changes.changed().await.is_err() {
                    return;
                }
            }
        })
        .await;
        assert!(stopped.is_ok(), "it reports the stop");
        assert_eq!(listener.state(), VoiceState::Off);
    }

    #[tokio::test]
    async fn without_the_app_nothing_starts_and_it_says_why() {
        let listener = Listener::none();
        listener.follow(true);
        assert_eq!(listener.state(), VoiceState::Off);
        assert!(listener.problem().is_some_and(|problem| problem.contains("torroassistant")));
        assert!(listener.ask(&[], Duration::from_secs(1)).await.is_err());
        listener.follow(false);
        assert_eq!(listener.problem(), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn helpers_answer_with_their_last_line() {
        let dir = tempfile::tempdir().expect("temp dir");
        let answer = serde_json::to_string(&Response::Transcript { text: "Hi".into() }).expect("json");
        let listener = Listener::at(fake_program(dir.path(), &format!("echo noise; echo '{answer}'")));
        assert_eq!(listener.ask(&[], Duration::from_secs(5)).await, Ok(Response::Transcript { text: "Hi".into() }));
        let slow = Listener::at(fake_program(dir.path(), "sleep 5"));
        assert!(slow.ask(&[], Duration::from_millis(200)).await.is_err(), "it does not wait for ever");
    }

    #[test]
    fn detects_installed_apps_in_the_given_dirs() {
        let next_to_exe = tempfile::tempdir().expect("temp dir");
        let on_path = tempfile::tempdir().expect("temp dir");
        let empty = tempfile::tempdir().expect("temp dir");
        fs::write(on_path.path().join(binary("torrocalendar")), b"").expect("write binary");
        // A directory with the right name is not an installed binary.
        fs::create_dir(next_to_exe.path().join(binary("torrotasks"))).expect("create dir");

        let dirs = vec![next_to_exe.path().to_path_buf(), empty.path().to_path_buf(), on_path.path().to_path_buf()];
        assert!(is_installed("torrocalendar", &dirs));
        assert!(!is_installed("torrotasks", &dirs));
        assert!(!is_installed("torrocalendar", &[empty.path().to_path_buf()]));
        assert!(!is_installed("torrocalendar", &[]));
    }

    #[test]
    fn lists_every_app_but_the_core_with_its_state() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(binary("torrotasks")), b"").expect("write binary");
        let apps = list_in(&[dir.path().to_path_buf()]);
        assert_eq!(apps.len(), AppId::ALL.len() - 1);
        assert!(apps.iter().all(|app| app.binary != AppId::WORKPLACE.binary));
        let tasks = apps.iter().find(|app| app.binary == "torrotasks").expect("tasks listed");
        assert_eq!(
            tasks,
            &AppInfo {
                binary: "torrotasks".into(),
                name: AppId::TASKS.display_name.into(),
                window_class: AppId::TASKS.window_class.into(),
                installed: true,
            }
        );
        let calendar = apps.iter().find(|app| app.binary == "torrocalendar").expect("calendar listed");
        assert!(!calendar.installed);
    }

    /// Task E8.7a: targets are read in their two forms, open in their app and become one argument.
    #[test]
    fn targets_name_their_app_and_become_one_argument() {
        let mail = Target::parse("mail:anna@mailbox.org:<a:1@example.org>").expect("mail");
        assert_eq!(mail, Target::Mail { account: "mail:anna@mailbox.org", message_id: "<a:1@example.org>" });
        assert!(mail.opens_in(AppId::INBOX_MAIL) && mail.opens_in(AppId::INBOX) && !mail.opens_in(AppId::TASKS));
        let task = Target::parse("task:7d1e-5b2a").expect("task");
        assert_eq!(task, Target::Task { uid: "7d1e-5b2a" });
        assert!(task.opens_in(AppId::TASKS) && !task.opens_in(AppId::INBOX_MAIL));
        for bad in ["", "task:", "task:a\nb", "inbox:0a1b", "mail:anna@mailbox.org", "https://example.org"] {
            assert!(Target::parse(bad).is_err(), "{bad:?}");
        }
        let opened = Opened::Mail { message: "m:0a1b".into() };
        assert_eq!(
            target_command("/bin/torroinbox".into(), &opened),
            ["/bin/torroinbox", "--view", "mail", "--message=m:0a1b"]
        );
        let opened = Opened::Task { uid: "--version".into() };
        assert_eq!(target_command("torrotasks".into(), &opened), ["torrotasks", "--task=--version"]);
    }

    /// Task WZ.8: `file:<id>` names a file of the register by its 8 hex digits and opens in the
    /// inbox's binary, as a new mail in the compose window with the path the register keeps.
    #[test]
    fn a_file_target_opens_a_new_mail_with_the_file() {
        let file = Target::parse("file:1a2b3c4d").expect("file");
        assert_eq!(file, Target::File { id: "1a2b3c4d" });
        assert!(file.opens_in(AppId::INBOX) && file.opens_in(AppId::INBOX_MAIL) && !file.opens_in(AppId::TASKS));
        for bad in ["file:", "file:1a2b3c4", "file:1a2b3c4d5", "file:../../x", "file:1a2b3c4g", "file:/tmp/a.pdf"] {
            assert!(Target::parse(bad).is_err(), "{bad:?}");
        }
        let opened = Opened::File { path: "/home/anna/Desktop/Offer.pdf".into() };
        assert_eq!(opened.app().binary, "torroinbox");
        assert_eq!(
            target_command("/bin/torroinbox".into(), &opened),
            ["/bin/torroinbox", "--compose", "--attach=/home/anna/Desktop/Offer.pdf"]
        );
    }

    /// Issue #243: `template:<id>` opens the core's dialog to edit the template, never in the mode
    /// that deletes it on Cancel.
    #[test]
    fn a_template_target_opens_the_dialog_to_edit_it() {
        let template = Target::parse("template:deep-work-2").expect("template");
        assert_eq!(template, Target::Template { id: "deep-work-2" });
        assert!(template.opens_in(AppId::WORKPLACE) && !template.opens_in(AppId::TASKS));
        for bad in ["template:", "template:Dev", "template:../x", "template:a b", "template:a --saved"] {
            assert!(Target::parse(bad).is_err(), "{bad:?}");
        }
        let opened = Opened::Template { id: "dev".into() };
        let command = target_command("/bin/torroworkplace".into(), &opened);
        assert_eq!(command, ["/bin/torroworkplace", "ui", "--template=dev"]);
        assert!(!command.iter().any(|arg| arg.contains("saved")), "never the mode Save");
    }

    /// The path `program` in `dir`, as the core's command line names it.
    fn at(dir: &Path, program: &str) -> String {
        dir.join(binary(program)).to_string_lossy().into_owned()
    }

    /// Issue #279: Mail and App Feed start with their own program when it is installed, else with
    /// `torroinbox` as before; the arguments of the entry stay the same, other entries are untouched.
    #[test]
    fn mail_and_app_feed_start_with_their_own_program_when_it_is_there() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dirs = [dir.path().to_path_buf()];
        let mail = AppId::INBOX_MAIL;
        assert_eq!(program_for(mail, &dirs), "torroinbox", "nothing installed: by name, from PATH");

        fs::write(dir.path().join(binary("torroinbox")), b"").expect("write binary");
        let inbox = at(dir.path(), "torroinbox");
        assert_eq!(mail.command(program_for(mail, &dirs)), [inbox.as_str(), "--view", "mail"]);

        fs::write(dir.path().join(binary("torro-inbox-mail")), b"").expect("write binary");
        let own = at(dir.path(), "torro-inbox-mail");
        assert_eq!(mail.command(program_for(mail, &dirs)), [own.as_str(), "--view", "mail"]);
        let feed = AppId::INBOX_NOTIFICATIONS;
        assert_eq!(program_for(feed, &dirs), inbox, "App Feed without its program");
        fs::write(dir.path().join(binary("torro-inbox-notifications")), b"").expect("write binary");
        assert_eq!(program_for(feed, &dirs), at(dir.path(), "torro-inbox-notifications"));
        assert_eq!(program_for(AppId::INBOX, &dirs), inbox, "the hub is torroinbox");
        assert_eq!(program_for(AppId::CALENDAR, &dirs), "torrocalendar");

        // The programs of one installation go together: the first folder with either wins.
        let later = tempfile::tempdir().expect("temp dir");
        fs::write(later.path().join(binary("torro-inbox-mail")), b"").expect("write binary");
        let first = tempfile::tempdir().expect("temp dir");
        fs::write(first.path().join(binary("torroinbox")), b"").expect("write binary");
        let dirs = [first.path().to_path_buf(), later.path().to_path_buf()];
        assert_eq!(program_for(mail, &dirs), at(first.path(), "torroinbox"));
    }

    /// Issue #279: a mail opens in the mail program with its `--message=…` when it is installed, else
    /// in `torroinbox`; a file always opens the compose window of `torroinbox`.
    #[test]
    fn a_mail_target_starts_the_own_program_when_it_is_there() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dirs = [dir.path().to_path_buf()];
        fs::write(dir.path().join(binary("torroinbox")), b"").expect("write binary");
        let inbox = at(dir.path(), "torroinbox");
        let mail = Opened::Mail { message: "m:0a1b".into() };
        let file = Opened::File { path: "/home/anna/Offer.pdf".into() };
        let command = target_command(target_program(&mail, &dirs), &mail);
        assert_eq!(command, [inbox.as_str(), "--view", "mail", "--message=m:0a1b"]);

        fs::write(dir.path().join(binary("torro-inbox-mail")), b"").expect("write binary");
        let own = at(dir.path(), "torro-inbox-mail");
        let command = target_command(target_program(&mail, &dirs), &mail);
        assert_eq!(command, [own.as_str(), "--view", "mail", "--message=m:0a1b"]);
        let command = target_command(target_program(&file, &dirs), &file);
        assert_eq!(command, [inbox.as_str(), "--compose", "--attach=/home/anna/Offer.pdf"], "compose stays");
        let task = Opened::Task { uid: "7d1e".into() };
        assert_eq!(target_command(target_program(&task, &dirs), &task), ["torrotasks", "--task=7d1e"]);
    }

    /// Issue #279: with only `torro-inbox-mail` installed, Mail is installed and still listed with
    /// the binary `torroinbox`; the hub and App Feed are not.
    #[test]
    fn mail_is_installed_with_only_its_own_program() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(binary("torro-inbox-mail")), b"").expect("write binary");
        let apps = list_in(&[dir.path().to_path_buf()]);
        let inbox: Vec<(&str, &str, bool)> = apps
            .iter()
            .filter(|app| app.window_class.starts_with("torro-inbox"))
            .map(|app| (app.window_class.as_str(), app.binary.as_str(), app.installed))
            .collect();
        assert_eq!(
            inbox,
            [
                ("torro-inbox", "torroinbox", false),
                ("torro-inbox-mail", "torroinbox", true),
                ("torro-inbox-notifications", "torroinbox", false),
            ]
        );
    }

    #[test]
    fn the_inbox_is_listed_as_three_windows() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(binary("torroinbox")), b"").expect("write binary");
        let apps = list_in(&[dir.path().to_path_buf()]);
        let inbox: Vec<(&str, &str, bool)> = apps
            .iter()
            .filter(|app| app.binary == "torroinbox")
            .map(|app| (app.window_class.as_str(), app.name.as_str(), app.installed))
            .collect();
        assert_eq!(
            inbox,
            [
                ("torro-inbox", AppId::INBOX.display_name, true),
                ("torro-inbox-mail", AppId::INBOX_MAIL.display_name, true),
                ("torro-inbox-notifications", AppId::INBOX_NOTIFICATIONS.display_name, true),
            ]
        );
    }
}
