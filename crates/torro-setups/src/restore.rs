//! Rebuilding a template on the active workspace.
//!
//! Tasks 1.14–1.16 in `docs/tasks/phase-1.md`. Reference:
//! `apps/workspace-setups/workspace_setups/restore.py`.
//!
//! # Order of work
//!
//! 1. Remember the target workspace first; starting programs may move the focus.
//! 2. Apply the [`LoadMode`]: fail, keep or close what is there.
//! 3. Build the tree **top-down**. For a split, the first leaf of `second` is inserted next
//!    to the first leaf of `first` (`prepare_insert`, open/move, `place` with the split's
//!    ratio); then both halves are built the same way. Example: `TopBottom(LeftRight(A, B),
//!    C)` → A; C below A; B right of A. This way every split is created at the right level
//!    of the tree, in Hyprland's dwindle as well as in i3-like trees.
//! 4. Windows of a [`LayoutTree::Fallback`] and floating windows open without layout
//!    instructions.
//!
//! A window that fails (program missing, no window within the timeout) is reported and
//! skipped; the rest of the template still loads.
//!
//! # Finding the window of a started program
//!
//! Events are read from one subscription that stays open for the whole load. A window that
//! opened is matched to the program we started if its process descends from the spawned pid
//! (checked on Linux via `/proc`). A window of the same class that does not descend from it
//! is a second choice: it is used once no other event is waiting. Windows that match nothing
//! are kept, so a later [`Strategy::Move`] can still take one.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use torro_wm::{Events, Window, WindowId, WindowManager, WmEvent, WorkspaceId};

use crate::launch::Launcher;
use crate::{LoadMode, SetupsError, Strategy, Template, TemplateWindow};

/// Knobs for a load.
#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// How long to wait for a started program's window.
    pub window_timeout: Duration,
}

impl Default for RestoreOptions {
    fn default() -> RestoreOptions {
        RestoreOptions { window_timeout: Duration::from_secs(15) }
    }
}

/// What a load did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreReport {
    /// Template window ids that are in place.
    pub restored: Vec<String>,
    /// Template window ids that failed, with the reason.
    pub failed: Vec<(String, String)>,
}

/// Loads `template` onto the active workspace.
pub async fn restore(
    wm: &impl WindowManager,
    launcher: &impl Launcher,
    template: &Template,
    mode: LoadMode,
    options: &RestoreOptions,
) -> Result<RestoreReport, SetupsError> {
    restore_with(wm, launcher, template, mode, options, default_ancestors()).await
}

/// Maps a window's pid to the pids of it and its ancestors. [`restore`] reads `/proc`; tests
/// pass a closure that describes a process tree.
type Ancestors = Box<dyn Fn(u32) -> HashSet<u32> + Send + Sync>;

#[cfg(target_os = "linux")]
fn default_ancestors() -> Ancestors {
    Box::new(|pid| crate::launch::linux::process_ancestors(std::path::Path::new("/proc"), pid))
}

#[cfg(not(target_os = "linux"))]
fn default_ancestors() -> Ancestors {
    // Without a process table only the window's own pid can be compared.
    Box::new(|pid| HashSet::from([pid]))
}

async fn restore_with(
    wm: &impl WindowManager,
    launcher: &impl Launcher,
    template: &Template,
    mode: LoadMode,
    options: &RestoreOptions,
    ancestors: Ancestors,
) -> Result<RestoreReport, SetupsError> {
    // Starting programs can move the focus, so the target is taken before anything else.
    let target = wm.active_workspace().await?;
    // One subscription for the whole load, opened before the first program starts so no
    // window that opens in between is missed.
    let events = wm.events().await?;

    let occupied = wm.windows(Some(&target)).await?;
    match mode {
        LoadMode::Empty if !occupied.is_empty() => {
            return Err(SetupsError::WorkspaceNotEmpty { windows: occupied.len() });
        }
        LoadMode::Replace => {
            for window in &occupied {
                wm.close_window(&window.id).await?;
            }
        }
        LoadMode::Empty | LoadMode::Add => {}
    }

    // Open windows on other workspaces are the candidates for `Strategy::Move`.
    let elsewhere = wm.windows(None).await?.into_iter().filter(|w| w.workspace != target).collect();
    let mut loader = Loader {
        wm,
        launcher,
        options,
        target,
        elsewhere,
        pending: Vec::new(),
        used: HashSet::new(),
        events,
        ancestors,
    };

    let mut report = RestoreReport::default();
    // Live window per template window id. Task 1.16 arranges the layout from this map.
    let mut live: HashMap<&str, WindowId> = HashMap::new();
    for window in &template.windows {
        match loader.bring(window).await {
            Ok(id) => {
                report.restored.push(window.id.clone());
                live.insert(&window.id, id);
            }
            Err(reason) => report.failed.push((window.id.clone(), reason)),
        }
    }

    // Task 1.16 goes here: walk `template.tree` top-down with `live`, calling
    // `prepare_insert` and `place`, then open the `Fallback` and floating windows.
    Ok(report)
}

/// The state of one load: where the windows go and which ones this load has already taken.
struct Loader<'a, W, L> {
    wm: &'a W,
    launcher: &'a L,
    options: &'a RestoreOptions,
    target: WorkspaceId,
    /// Open windows on other workspaces when the load started, in the order the window
    /// manager lists them.
    elsewhere: Vec<Window>,
    /// Windows that opened during this load and matched no template window yet. A later
    /// [`Strategy::Move`] may still take one.
    pending: Vec<Window>,
    /// Windows this load has brought over, so two template windows never get the same one.
    used: HashSet<WindowId>,
    /// The subscription opened at the start of the load.
    events: Events,
    ancestors: Ancestors,
}

impl<W: WindowManager, L: Launcher> Loader<'_, W, L> {
    /// Puts one template window on the target workspace and returns its live window id.
    /// The error is the reason shown in the report.
    async fn bring(&mut self, window: &TemplateWindow) -> Result<WindowId, String> {
        // Windows that opened since the last call may be what a later `Move` wants.
        self.absorb().await;
        match window.strategy {
            Strategy::Move => {
                if let Some(open) = self.find_open(window) {
                    return self.claim(open).await;
                }
                if window.command.is_empty() {
                    return Err("no open window to bring".to_string());
                }
                self.start(window).await
            }
            Strategy::New => self.start(window).await,
        }
    }

    /// The first unused open window of the same class on another workspace.
    fn find_open(&self, window: &TemplateWindow) -> Option<Window> {
        let class = window.class.to_lowercase();
        self.elsewhere
            .iter()
            .chain(&self.pending)
            .find(|w| w.workspace != self.target && !self.used.contains(&w.id) && w.class.to_lowercase() == class)
            .cloned()
    }

    /// Starts the program of `template` and returns the window that belongs to it.
    async fn start(&mut self, template: &TemplateWindow) -> Result<WindowId, String> {
        if template.command.is_empty() {
            return Err("no start command".to_string());
        }
        let pid = self.launcher.spawn(&template.command).map_err(|err| err.to_string())?;

        let class = template.class.to_lowercase();
        let timeout = self.options.window_timeout;
        let mut fallback = None;
        let found = tokio::time::timeout(timeout, self.scan(pid, &class, &mut fallback)).await;
        let found = match found {
            Ok(Some(window)) => {
                // A process match beats a class match; the class match goes back to the pending list.
                if let Some(other) = fallback.take() {
                    self.pending.push(other);
                }
                Some(window)
            }
            // Timeout or closed subscription: settle for the class match, if there was one.
            Ok(None) | Err(_) => fallback,
        };
        let Some(window) = found else {
            return Err(format!("no window appeared within {} s", timeout.as_secs_f64()));
        };
        self.claim(window).await
    }

    /// Reads events until a window descends from `pid` (returned at once) or the queue runs
    /// dry after a class match was stored in `fallback`. Returns `None` when no process match
    /// came in, or when the subscription closed.
    async fn scan(&mut self, pid: u32, class: &str, fallback: &mut Option<Window>) -> Option<Window> {
        loop {
            let event = if fallback.is_some() {
                // Only events that are already queued can still beat the class match.
                self.events.try_recv().ok()?
            } else {
                self.events.recv().await?
            };
            let WmEvent::WindowOpened { window, .. } = event else {
                continue;
            };
            let Some(window) = self.lookup(&window).await else {
                continue;
            };
            if self.used.contains(&window.id) {
                continue;
            }
            if self.started_by(&window, pid) {
                return Some(window);
            }
            if fallback.is_none() && window.class.to_lowercase() == class {
                *fallback = Some(window);
            } else {
                self.pending.push(window);
            }
        }
    }

    /// Takes the windows that opened since the last look into `pending`, without waiting.
    async fn absorb(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            if let WmEvent::WindowOpened { window, .. } = event
                && let Some(window) = self.lookup(&window).await
            {
                self.pending.push(window);
            }
        }
    }

    /// The window with `id` as the window manager lists it now. The opened event has no pid.
    async fn lookup(&self, id: &WindowId) -> Option<Window> {
        self.wm.windows(None).await.ok()?.into_iter().find(|w| &w.id == id)
    }

    /// True when the window's process is `pid` or one of its descendants.
    fn started_by(&self, window: &Window, pid: u32) -> bool {
        window.pid.is_some_and(|window_pid| (self.ancestors)(window_pid).contains(&pid))
    }

    /// Moves `window` to the target if it is elsewhere and records it as used.
    async fn claim(&mut self, window: Window) -> Result<WindowId, String> {
        if window.workspace != self.target {
            self.wm
                .move_to_workspace(&window.id, &self.target)
                .await
                .map_err(|err| format!("could not move window: {err}"))?;
        }
        self.used.insert(window.id.clone());
        Ok(window.id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use chrono::Utc;
    use torro_wm::Rect;
    use torro_wm::fake::{Call, FakeWm, window};

    use super::*;
    use crate::{SCHEMA_VERSION, Source};

    /// Task 1.14 never starts programs; a call here is a bug.
    struct NoLaunch;

    impl Launcher for NoLaunch {
        fn spawn(&self, command: &[String]) -> Result<u32, SetupsError> {
            panic!("restore must not start {command:?} in task 1.14")
        }
    }

    /// Starts nothing for real. `act` decides what a started program does, typically by
    /// opening windows in the fake; every command is recorded.
    struct Scripted<F> {
        act: F,
        started: Mutex<Vec<Vec<String>>>,
    }

    impl<F> Scripted<F>
    where
        F: Fn(&[String]) -> Result<u32, SetupsError> + Send + Sync,
    {
        fn new(act: F) -> Scripted<F> {
            Scripted { act, started: Mutex::new(Vec::new()) }
        }

        fn started(&self) -> Vec<Vec<String>> {
            self.started.lock().unwrap_or_else(PoisonError::into_inner).clone()
        }
    }

    impl<F> Launcher for Scripted<F>
    where
        F: Fn(&[String]) -> Result<u32, SetupsError> + Send + Sync,
    {
        fn spawn(&self, command: &[String]) -> Result<u32, SetupsError> {
            self.started.lock().unwrap_or_else(PoisonError::into_inner).push(command.to_vec());
            (self.act)(command)
        }
    }

    /// The pid `Scripted` launchers return for the program they start.
    const SPAWNED_PID: u32 = 4000;
    /// A window whose process is a child of the spawned one.
    const CHILD_PID: u32 = 4001;
    /// A window of some other process.
    const UNRELATED_PID: u32 = 9000;

    /// `CHILD_PID` descends from `SPAWNED_PID`; every other pid is its own only.
    fn ancestry(pid: u32) -> HashSet<u32> {
        if pid == CHILD_PID { HashSet::from([CHILD_PID, SPAWNED_PID]) } else { HashSet::from([pid]) }
    }

    fn template_window(id: &str, class: &str, strategy: Strategy) -> TemplateWindow {
        TemplateWindow {
            id: id.into(),
            class: class.into(),
            title: String::new(),
            command: Vec::new(),
            strategy,
            floating: false,
            rect: Rect::default(),
        }
    }

    fn started_window(id: &str, class: &str, command: &[&str]) -> TemplateWindow {
        TemplateWindow {
            command: command.iter().map(|word| word.to_string()).collect(),
            ..template_window(id, class, Strategy::New)
        }
    }

    fn template(windows: Vec<TemplateWindow>) -> Template {
        let now = Utc::now();
        Template {
            schema_version: SCHEMA_VERSION,
            id: "dev".into(),
            name: "Dev".into(),
            wm: "fake".into(),
            created: now,
            updated: now,
            source: Source { workspace: "1".into(), monitor: None, monitor_rect: None },
            tree: None,
            windows,
        }
    }

    async fn load(wm: &FakeWm, template: &Template, mode: LoadMode) -> Result<RestoreReport, SetupsError> {
        restore(wm, &NoLaunch, template, mode, &RestoreOptions::default()).await
    }

    /// Loads in `LoadMode::Empty` with a short wait and the test process tree.
    async fn start_load(
        wm: &FakeWm,
        launcher: &impl Launcher,
        template: &Template,
    ) -> Result<RestoreReport, SetupsError> {
        let options = RestoreOptions { window_timeout: Duration::from_millis(50) };
        restore_with(wm, launcher, template, LoadMode::Empty, &options, Box::new(ancestry)).await
    }

    fn ws(name: &str) -> WorkspaceId {
        WorkspaceId(name.into())
    }

    #[tokio::test]
    async fn empty_on_empty_workspace_loads() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xa", "foot", "5", Rect::default()));
        let template = template(vec![template_window("window-1", "foot", Strategy::Move)]);

        let report = load(&wm, &template, LoadMode::Empty).await.unwrap();

        assert_eq!(report.restored, vec!["window-1".to_string()]);
        assert_eq!(wm.calls(), vec![Call::Move(WindowId("0xa".into()), ws("3"))]);
    }

    #[tokio::test]
    async fn empty_on_busy_workspace_fails_without_touching_anything() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xa", "foot", "3", Rect::default()));
        wm.add_window(window("0xb", "foot", "5", Rect::default()));
        let template = template(vec![template_window("window-1", "foot", Strategy::Move)]);

        let result = load(&wm, &template, LoadMode::Empty).await;

        assert_eq!(result, Err(SetupsError::WorkspaceNotEmpty { windows: 1 }));
        assert!(wm.calls().is_empty());
    }

    #[tokio::test]
    async fn replace_closes_the_windows_on_the_target_first() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xa", "foot", "3", Rect::default()));
        wm.add_window(window("0xb", "chromium", "3", Rect::default()));
        wm.add_window(window("0xc", "foot", "5", Rect::default()));
        let template = template(vec![template_window("window-1", "foot", Strategy::Move)]);

        let report = load(&wm, &template, LoadMode::Replace).await.unwrap();

        assert_eq!(report.restored, vec!["window-1".to_string()]);
        assert_eq!(
            wm.calls(),
            vec![
                Call::Close(WindowId("0xa".into())),
                Call::Close(WindowId("0xb".into())),
                Call::Move(WindowId("0xc".into()), ws("3")),
            ]
        );
        assert_eq!(wm.all_windows().iter().filter(|w| w.workspace == ws("3")).count(), 1);
    }

    #[tokio::test]
    async fn add_keeps_the_windows_on_the_target() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xa", "foot", "3", Rect::default()));
        wm.add_window(window("0xc", "foot", "5", Rect::default()));
        let template = template(vec![template_window("window-1", "foot", Strategy::Move)]);

        let report = load(&wm, &template, LoadMode::Add).await.unwrap();

        // The foot already on the target is not a candidate; the one on workspace 5 is.
        assert_eq!(report.restored, vec!["window-1".to_string()]);
        assert_eq!(wm.calls(), vec![Call::Move(WindowId("0xc".into()), ws("3"))]);
        assert!(wm.all_windows().iter().any(|w| w.id == WindowId("0xa".into()) && w.workspace == ws("3")));
    }

    #[tokio::test]
    async fn move_matches_the_class_case_insensitively() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xs", "Spotify", "5", Rect::default()));
        let template = template(vec![template_window("window-1", "spotify", Strategy::Move)]);

        let report = load(&wm, &template, LoadMode::Empty).await.unwrap();

        assert_eq!(report, RestoreReport { restored: vec!["window-1".into()], failed: vec![] });
        assert_eq!(wm.calls(), vec![Call::Move(WindowId("0xs".into()), ws("3"))]);
    }

    #[tokio::test]
    async fn two_windows_of_one_class_take_two_different_windows() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xa", "foot", "5", Rect::default()));
        wm.add_window(window("0xb", "foot", "6", Rect::default()));
        let template = template(vec![
            template_window("window-1", "foot", Strategy::Move),
            template_window("window-2", "foot", Strategy::Move),
        ]);

        let report = load(&wm, &template, LoadMode::Empty).await.unwrap();

        assert_eq!(report.restored, vec!["window-1".to_string(), "window-2".to_string()]);
        assert_eq!(
            wm.calls(),
            vec![Call::Move(WindowId("0xa".into()), ws("3")), Call::Move(WindowId("0xb".into()), ws("3"))]
        );
    }

    #[tokio::test]
    async fn move_without_a_match_fails() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xa", "chromium", "5", Rect::default()));
        let template = template(vec![template_window("window-1", "foot", Strategy::Move)]);

        let report = load(&wm, &template, LoadMode::Empty).await.unwrap();

        assert_eq!(
            report,
            RestoreReport { restored: vec![], failed: vec![("window-1".into(), "no open window to bring".into())] }
        );
        assert!(wm.calls().is_empty());
    }

    #[tokio::test]
    async fn new_starts_the_program_and_restores_its_window() {
        let wm = Arc::new(FakeWm::new("3"));
        let opener = Arc::clone(&wm);
        let launcher = Scripted::new(move |_: &[String]| {
            opener.open_window("foot", Some(CHILD_PID));
            Ok(SPAWNED_PID)
        });
        let template = template(vec![started_window("window-1", "foot", &["foot"])]);

        let report = start_load(&wm, &launcher, &template).await.unwrap();

        assert_eq!(report, RestoreReport { restored: vec!["window-1".into()], failed: vec![] });
        assert_eq!(launcher.started(), vec![vec!["foot".to_string()]]);
        assert_eq!(wm.all_windows().len(), 1);
        assert!(wm.calls().is_empty(), "the window already is on the target");
    }

    #[tokio::test]
    async fn new_without_a_command_fails_and_the_rest_still_loads() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xa", "foot", "5", Rect::default()));
        let launcher = Scripted::new(|_: &[String]| Ok(SPAWNED_PID));
        let template = template(vec![
            template_window("window-1", "foot", Strategy::New),
            template_window("window-2", "foot", Strategy::Move),
        ]);

        let report = start_load(&wm, &launcher, &template).await.unwrap();

        assert_eq!(
            report,
            RestoreReport {
                restored: vec!["window-2".into()],
                failed: vec![("window-1".into(), "no start command".into())],
            }
        );
        assert!(launcher.started().is_empty());
        assert_eq!(wm.calls(), vec![Call::Move(WindowId("0xa".into()), ws("3"))]);
    }

    #[tokio::test]
    async fn new_times_out_when_no_window_appears() {
        let wm = FakeWm::new("3");
        let launcher = Scripted::new(|_: &[String]| Ok(SPAWNED_PID));
        let template = template(vec![started_window("window-1", "foot", &["silent"])]);

        let report = start_load(&wm, &launcher, &template).await.unwrap();

        assert_eq!(
            report,
            RestoreReport {
                restored: vec![],
                failed: vec![("window-1".into(), "no window appeared within 0.05 s".into())],
            }
        );
        assert_eq!(launcher.started(), vec![vec!["silent".to_string()]]);
    }

    #[tokio::test]
    async fn descendant_of_the_started_pid_beats_an_earlier_class_match() {
        let wm = Arc::new(FakeWm::new("3"));
        let opener = Arc::clone(&wm);
        // Both windows open on workspace 5, so the one that gets used shows up as a move.
        let launcher = Scripted::new(move |_: &[String]| {
            opener.focus_workspace("5");
            opener.open_window("foot", Some(UNRELATED_PID));
            opener.open_window("foot", Some(CHILD_PID));
            opener.focus_workspace("3");
            Ok(SPAWNED_PID)
        });
        let template = template(vec![started_window("window-1", "foot", &["foot"])]);

        let report = start_load(&wm, &launcher, &template).await.unwrap();

        assert_eq!(report.restored, vec!["window-1".to_string()]);
        assert_eq!(wm.calls(), vec![Call::Move(WindowId("0xfake2".into()), ws("3"))]);
    }

    #[tokio::test]
    async fn class_match_is_the_second_choice_and_is_moved_to_the_target() {
        let wm = Arc::new(FakeWm::new("3"));
        let opener = Arc::clone(&wm);
        // A window of the class from some other process, e.g. a browser that reused its instance.
        let launcher = Scripted::new(move |_: &[String]| {
            opener.focus_workspace("5");
            opener.open_window("foot", Some(UNRELATED_PID));
            opener.focus_workspace("3");
            Ok(SPAWNED_PID)
        });
        let template = template(vec![started_window("window-1", "Foot", &["foot"])]);

        let report = start_load(&wm, &launcher, &template).await.unwrap();

        assert_eq!(report.restored, vec!["window-1".to_string()]);
        assert_eq!(wm.calls(), vec![Call::Move(WindowId("0xfake1".into()), ws("3"))]);
    }

    #[tokio::test]
    async fn a_window_of_another_class_is_not_taken_by_a_new_entry() {
        let wm = Arc::new(FakeWm::new("3"));
        let opener = Arc::clone(&wm);
        let launcher = Scripted::new(move |_: &[String]| {
            opener.open_window("chromium", None);
            Ok(SPAWNED_PID)
        });
        let template = template(vec![started_window("window-1", "foot", &["foot"])]);

        let report = start_load(&wm, &launcher, &template).await.unwrap();

        assert_eq!(report.failed, vec![("window-1".into(), "no window appeared within 0.05 s".into())]);
        assert!(wm.calls().is_empty());
    }

    #[tokio::test]
    async fn move_falls_back_to_starting_the_program() {
        let wm = Arc::new(FakeWm::new("3"));
        let opener = Arc::clone(&wm);
        let launcher = Scripted::new(move |_: &[String]| {
            opener.open_window("foot", Some(CHILD_PID));
            Ok(SPAWNED_PID)
        });
        let template = template(vec![TemplateWindow {
            command: vec!["foot".into()],
            ..template_window("window-1", "foot", Strategy::Move)
        }]);

        let report = start_load(&wm, &launcher, &template).await.unwrap();

        assert_eq!(report, RestoreReport { restored: vec!["window-1".into()], failed: vec![] });
        assert_eq!(launcher.started(), vec![vec!["foot".to_string()]]);
        assert!(wm.calls().is_empty());
    }

    #[tokio::test]
    async fn one_failing_window_does_not_stop_the_others() {
        let wm = Arc::new(FakeWm::new("3"));
        let opener = Arc::clone(&wm);
        let launcher = Scripted::new(move |command: &[String]| match command[0].as_str() {
            "missing" => Err(SetupsError::Launch("no such file".into())),
            "foot" => {
                opener.open_window("foot", Some(CHILD_PID));
                Ok(SPAWNED_PID)
            }
            _ => Ok(SPAWNED_PID),
        });
        let template = template(vec![
            started_window("window-1", "tool", &["missing"]),
            started_window("window-2", "other", &["silent"]),
            started_window("window-3", "foot", &["foot"]),
        ]);

        let report = start_load(&wm, &launcher, &template).await.unwrap();

        assert_eq!(report.restored, vec!["window-3".to_string()]);
        assert_eq!(
            report.failed,
            vec![
                ("window-1".into(), "could not start program: no such file".into()),
                ("window-2".into(), "no window appeared within 0.05 s".into()),
            ]
        );
    }

    #[tokio::test]
    async fn window_seen_while_waiting_stays_available_for_a_later_move() {
        let wm = Arc::new(FakeWm::new("3"));
        let opener = Arc::clone(&wm);
        // Spotify opens elsewhere while `foot` is awaited; the later `Move` takes it.
        let launcher = Scripted::new(move |_: &[String]| {
            opener.focus_workspace("5");
            opener.open_window("spotify", None);
            opener.focus_workspace("3");
            opener.open_window("foot", Some(CHILD_PID));
            Ok(SPAWNED_PID)
        });
        let template = template(vec![
            started_window("window-1", "foot", &["foot"]),
            template_window("window-2", "spotify", Strategy::Move),
        ]);

        let report = start_load(&wm, &launcher, &template).await.unwrap();

        assert_eq!(report.restored, vec!["window-1".to_string(), "window-2".to_string()]);
        assert_eq!(wm.calls(), vec![Call::Move(WindowId("0xfake1".into()), ws("3"))]);
    }
}
