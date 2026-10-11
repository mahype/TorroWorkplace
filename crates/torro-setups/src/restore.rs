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

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use torro_wm::{Window, WindowId, WindowManager, WorkspaceId};

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
    // Starting programs can move the focus, so the target is taken before anything else.
    let target = wm.active_workspace().await?;

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
    let mut loader = Loader { wm, target, elsewhere, used: HashSet::new() };

    // Task 1.15 starts programs with `launcher` and waits for them using `options`.
    let _ = (launcher, options);

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
struct Loader<'a, W> {
    wm: &'a W,
    target: WorkspaceId,
    /// Open windows on other workspaces, in the order the window manager lists them.
    elsewhere: Vec<Window>,
    /// Windows this load has brought over, so two template windows never get the same one.
    used: HashSet<WindowId>,
}

impl<W: WindowManager> Loader<'_, W> {
    /// Puts one template window on the target workspace and returns its live window id.
    /// The error is the reason shown in the report.
    async fn bring(&mut self, window: &TemplateWindow) -> Result<WindowId, String> {
        match window.strategy {
            Strategy::Move => self.move_existing(window).await,
            // Task 1.15 starts the program here and waits for its window.
            Strategy::New => Err("starting programs comes with task 1.15".to_string()),
        }
    }

    /// Moves the first unused open window of the same class to the target workspace.
    async fn move_existing(&mut self, window: &TemplateWindow) -> Result<WindowId, String> {
        let class = window.class.to_lowercase();
        let found = self
            .elsewhere
            .iter()
            .find(|w| !self.used.contains(&w.id) && w.class.to_lowercase() == class)
            .map(|w| w.id.clone());
        let Some(id) = found else {
            return Err("no open window to bring".to_string());
        };
        self.wm.move_to_workspace(&id, &self.target).await.map_err(|err| format!("could not move window: {err}"))?;
        self.used.insert(id.clone());
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
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
    async fn new_fails_with_the_placeholder_reason_and_the_rest_still_loads() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xa", "foot", "5", Rect::default()));
        let template = template(vec![
            template_window("window-1", "foot", Strategy::New),
            template_window("window-2", "foot", Strategy::Move),
        ]);

        let report = load(&wm, &template, LoadMode::Empty).await.unwrap();

        assert_eq!(
            report,
            RestoreReport {
                restored: vec!["window-2".into()],
                failed: vec![("window-1".into(), "starting programs comes with task 1.15".into())],
            }
        );
        assert_eq!(wm.calls(), vec![Call::Move(WindowId("0xa".into()), ws("3"))]);
    }
}
