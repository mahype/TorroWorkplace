//! The suite's apps as the core reports them (`Request::ListApps`): every app of
//! `torro_core::AppId::ALL` except the core itself, and whether it is installed.
//!
//! "Installed" means the binary was found next to the core's executable or on `PATH`, the same
//! places `Request::OpenApp` starts apps from, so a listed app that is installed can be opened.
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
            installed: is_installed(app.binary, dirs),
        })
        .collect()
}

/// The folders an app binary is looked for in: the folder of this executable, then `PATH`.
fn search_dirs() -> Vec<PathBuf> {
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

#[cfg(test)]
pub mod tests {
    use std::fs;

    use super::*;

    fn binary(name: &str) -> String {
        format!("{name}{}", std::env::consts::EXE_SUFFIX)
    }

    /// A fake `torroassistant` in `dir`: a shell script with `body`, executable.
    #[cfg(unix)]
    pub fn fake_program(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("torroassistant");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write the program");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make it executable");
        path
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
}
