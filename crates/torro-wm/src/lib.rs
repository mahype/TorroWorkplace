//! Tiling window managers behind one trait.
//!
//! The core talks to whichever tiling window manager runs — Hyprland on Linux, AeroSpace on
//! macOS, GlazeWM on Windows — through [`WindowManager`]. The types here are neutral: they
//! describe windows, workspaces and splits the way every backend can map them.
//! [`detect`] picks the backend at runtime; [`fake::FakeWm`] stands in for tests.
//! [`focus_process_window`] finds and raises the window of a process chain, e.g. a coding agent's
//! terminal, with whatever the system offers (task E8.9).

pub mod aerospace;
pub mod detect;
pub mod fake;
pub mod glazewm;
pub mod hyprland;
mod types;

use std::future::Future;

pub use detect::detect;
pub use types::{
    Cover, KeyBind, Monitor, NoWindow, Rect, Split, SplitDirection, Window, WindowId, WmError, WmEvent, WorkspaceId,
};

/// Receiving end of a backend's event stream.
pub type Events = tokio::sync::mpsc::Receiver<WmEvent>;

/// What the core needs from a tiling window manager.
///
/// Methods return `Send` futures so the daemon can run them on its tokio runtime;
/// implementors simply write `async fn`.
pub trait WindowManager: Send + Sync {
    /// Short, stable backend name stored in templates: `hyprland`, `aerospace`, `glazewm`.
    fn name(&self) -> &'static str;

    /// The workspace that has focus right now.
    fn active_workspace(&self) -> impl Future<Output = Result<WorkspaceId, WmError>> + Send;

    /// The normal windows on `workspace`, or on all workspaces with `None`. Hidden, unmapped
    /// and special/scratchpad windows are left out.
    fn windows(&self, workspace: Option<&WorkspaceId>) -> impl Future<Output = Result<Vec<Window>, WmError>> + Send;

    /// All monitors, with the workspace each one shows.
    fn monitors(&self) -> impl Future<Output = Result<Vec<Monitor>, WmError>> + Send;

    fn focus_window(&self, window: &WindowId) -> impl Future<Output = Result<(), WmError>> + Send;

    /// Moves `window` to `workspace` without following it there.
    fn move_to_workspace(
        &self,
        window: &WindowId,
        workspace: &WorkspaceId,
    ) -> impl Future<Output = Result<(), WmError>> + Send;

    /// Asks the window to close (like the user pressing the close key).
    fn close_window(&self, window: &WindowId) -> impl Future<Output = Result<(), WmError>> + Send;

    /// Before a window opens: the next new window should split `sibling` in `direction` and
    /// become the second (right or lower) half.
    fn prepare_insert(
        &self,
        sibling: &WindowId,
        direction: SplitDirection,
    ) -> impl Future<Output = Result<(), WmError>> + Send;

    /// After the window appeared: make sure `window` sits next to `sibling` as described by
    /// `split` and the split has `split.ratio`. Hyprland already placed it in
    /// [`prepare_insert`](Self::prepare_insert) and only sets the ratio here; i3-like
    /// backends do the moving here.
    fn place(
        &self,
        window: &WindowId,
        sibling: &WindowId,
        split: Split,
    ) -> impl Future<Output = Result<(), WmError>> + Send;

    /// Starts listening for window and workspace events. Every call opens a new stream.
    fn events(&self) -> impl Future<Output = Result<Events, WmError>> + Send;

    /// Before windows open that should each cover one monitor whole and lie over everything
    /// (task ER2.2, the reminder over the whole screen): tells the window manager where each of
    /// `covers` goes. Hyprland registers a rule per monitor; window managers without such rules
    /// do nothing, and the window covers its screen by itself.
    fn cover_monitors(&self, covers: &[Cover]) -> impl Future<Output = Result<(), WmError>> + Send {
        let _ = covers;
        async { Ok(()) }
    }

    /// Gives the keyboard focus to the monitor `monitor` ([`Monitor::name`], as [`monitors`](Self::monitors)
    /// lists it) without moving any window: afterwards [`active_workspace`](Self::active_workspace) is the
    /// workspace that monitor shows (task WS.1, loading a template onto a chosen monitor).
    ///
    /// Errors: `NotFound` when no monitor has that name (GlazeWM, the fake; Hyprland and AeroSpace report a
    /// refused command as `Protocol`), `Unsupported` without a window manager.
    fn focus_monitor(&self, monitor: &str) -> impl Future<Output = Result<(), WmError>> + Send;

    /// The window manager's version for people and for feature checks, e.g. `0.56.0`; `None` when the
    /// backend cannot tell or the window manager does not answer. Only Hyprland tells (task WS.1).
    fn version(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }

    /// Registers global key binds, each running its [`KeyBind::command`] when pressed (task WS.1).
    ///
    /// Binds live until [`unbind_keys`](Self::unbind_keys), the end of the session or – on Hyprland – the
    /// next reload of its configuration ([`WmEvent::ConfigReloaded`]); callers register them again then.
    /// Binding keys that are already bound adds a second bind (Hyprland runs both), so callers unbind
    /// old keys first. Stops at the first refused bind; the binds before it stay.
    ///
    /// Errors: `Unsupported` where the window manager has no run-time binds (every backend but
    /// Hyprland; macOS and Windows bind keys in the tray instead, task WS.4), `Protocol` for a refused
    /// bind or a command that cannot be put on a shell line (empty, or with a NUL character).
    fn bind_keys(&self, binds: &[KeyBind]) -> impl Future<Output = Result<(), WmError>> + Send {
        let _ = binds;
        async { Err(WmError::Unsupported("key binds through the window manager".into())) }
    }

    /// Removes binds made with [`bind_keys`](Self::bind_keys), by their [`KeyBind::keys`] exactly as they
    /// were bound (Hyprland compares case-sensitively). Stops at the first refused unbind.
    ///
    /// Errors: as [`bind_keys`](Self::bind_keys).
    fn unbind_keys(&self, keys: &[String]) -> impl Future<Output = Result<(), WmError>> + Send {
        let _ = keys;
        async { Err(WmError::Unsupported("key binds through the window manager".into())) }
    }

    /// Before a small dialog window of the class `class` (app id) opens: it should float, centred on its
    /// monitor, instead of being tiled (task WS.1, the dialog after "Save workspace…"). Hyprland registers
    /// one named rule per class, so registering again replaces it; like every run-time rule it goes with a
    /// reload of the configuration. Window managers without such rules do nothing.
    fn float_dialogs(&self, class: &str) -> impl Future<Output = Result<(), WmError>> + Send {
        let _ = class;
        async { Ok(()) }
    }
}

/// The backend chosen at runtime.
#[derive(Debug)]
pub enum Backend {
    Hyprland(hyprland::Hyprland),
    GlazeWm(glazewm::GlazeWm),
    AeroSpace(aerospace::AeroSpace),
    /// No supported tiling window manager found; `name` says what we looked for, for the
    /// tray menu ("AeroSpace (macOS)", …).
    Unsupported {
        name: String,
    },
}

impl WindowManager for Backend {
    fn name(&self) -> &'static str {
        match self {
            Backend::Hyprland(wm) => wm.name(),
            Backend::GlazeWm(wm) => wm.name(),
            Backend::AeroSpace(wm) => wm.name(),
            Backend::Unsupported { .. } => "unsupported",
        }
    }

    async fn active_workspace(&self) -> Result<WorkspaceId, WmError> {
        match self {
            Backend::Hyprland(wm) => wm.active_workspace().await,
            Backend::GlazeWm(wm) => wm.active_workspace().await,
            Backend::AeroSpace(wm) => wm.active_workspace().await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn windows(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>, WmError> {
        match self {
            Backend::Hyprland(wm) => wm.windows(workspace).await,
            Backend::GlazeWm(wm) => wm.windows(workspace).await,
            Backend::AeroSpace(wm) => wm.windows(workspace).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn monitors(&self) -> Result<Vec<Monitor>, WmError> {
        match self {
            Backend::Hyprland(wm) => wm.monitors().await,
            Backend::GlazeWm(wm) => wm.monitors().await,
            Backend::AeroSpace(wm) => wm.monitors().await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn focus_window(&self, window: &WindowId) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.focus_window(window).await,
            Backend::GlazeWm(wm) => wm.focus_window(window).await,
            Backend::AeroSpace(wm) => wm.focus_window(window).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn move_to_workspace(&self, window: &WindowId, workspace: &WorkspaceId) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.move_to_workspace(window, workspace).await,
            Backend::GlazeWm(wm) => wm.move_to_workspace(window, workspace).await,
            Backend::AeroSpace(wm) => wm.move_to_workspace(window, workspace).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn close_window(&self, window: &WindowId) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.close_window(window).await,
            Backend::GlazeWm(wm) => wm.close_window(window).await,
            Backend::AeroSpace(wm) => wm.close_window(window).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn prepare_insert(&self, sibling: &WindowId, direction: SplitDirection) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.prepare_insert(sibling, direction).await,
            Backend::GlazeWm(wm) => wm.prepare_insert(sibling, direction).await,
            Backend::AeroSpace(wm) => wm.prepare_insert(sibling, direction).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn place(&self, window: &WindowId, sibling: &WindowId, split: Split) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.place(window, sibling, split).await,
            Backend::GlazeWm(wm) => wm.place(window, sibling, split).await,
            Backend::AeroSpace(wm) => wm.place(window, sibling, split).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn events(&self) -> Result<Events, WmError> {
        match self {
            Backend::Hyprland(wm) => wm.events().await,
            Backend::GlazeWm(wm) => wm.events().await,
            Backend::AeroSpace(wm) => wm.events().await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn cover_monitors(&self, covers: &[Cover]) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.cover_monitors(covers).await,
            Backend::GlazeWm(_) | Backend::AeroSpace(_) | Backend::Unsupported { .. } => Ok(()),
        }
    }

    async fn focus_monitor(&self, monitor: &str) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.focus_monitor(monitor).await,
            Backend::GlazeWm(wm) => wm.focus_monitor(monitor).await,
            Backend::AeroSpace(wm) => wm.focus_monitor(monitor).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn version(&self) -> Option<String> {
        match self {
            Backend::Hyprland(wm) => wm.version().await,
            Backend::GlazeWm(wm) => wm.version().await,
            Backend::AeroSpace(wm) => wm.version().await,
            Backend::Unsupported { .. } => None,
        }
    }

    async fn bind_keys(&self, binds: &[KeyBind]) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.bind_keys(binds).await,
            Backend::GlazeWm(wm) => wm.bind_keys(binds).await,
            Backend::AeroSpace(wm) => wm.bind_keys(binds).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn unbind_keys(&self, keys: &[String]) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.unbind_keys(keys).await,
            Backend::GlazeWm(wm) => wm.unbind_keys(keys).await,
            Backend::AeroSpace(wm) => wm.unbind_keys(keys).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn float_dialogs(&self, class: &str) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.float_dialogs(class).await,
            Backend::GlazeWm(wm) => wm.float_dialogs(class).await,
            Backend::AeroSpace(wm) => wm.float_dialogs(class).await,
            Backend::Unsupported { .. } => Ok(()),
        }
    }
}

/// Of `windows`, the one that shows what runs under `pids` (task E8.9).
///
/// `pids` is a chain of processes, nearest first, like a coding agent's parents: the windows of
/// the nearest process that owns any are the candidates (the terminal is a nearer parent than
/// the launcher that started it). One candidate is the window. Of several – a terminal that
/// serves all its windows from one process, the normal case on macOS – `hints` decide, the most
/// telling first (a session's name, then its project folder): a hint keeps the candidates whose
/// title holds it, ignoring case. One left is the window; several go on to the next hint; none
/// leaves the hint out. Blank hints count for nothing. Hints are only compared, never run.
///
/// `Err` holds the candidates it could not decide between, in the window manager's order; it is
/// empty when none of `pids` owns a window.
pub fn pick_window<'a>(windows: &'a [Window], pids: &[u32], hints: &[&str]) -> Result<&'a Window, Vec<&'a Window>> {
    let owned = |pid: u32| windows.iter().filter(move |window| window.pid == Some(pid));
    let Some(pid) = pids.iter().copied().find(|pid| owned(*pid).next().is_some()) else {
        return Err(Vec::new());
    };
    let mut candidates: Vec<&Window> = owned(pid).collect();
    for hint in hints.iter().map(|hint| hint.trim().to_lowercase()).filter(|hint| !hint.is_empty()) {
        if candidates.len() == 1 {
            break;
        }
        let named: Vec<&Window> =
            candidates.iter().copied().filter(|window| window.title.to_lowercase().contains(&hint)).collect();
        if !named.is_empty() {
            candidates = named;
        }
    }
    match candidates.as_slice() {
        [window] => Ok(window),
        _ => Err(candidates),
    }
}

/// Brings the window that shows what runs under `pids` (nearest first) to the front (task E8.9),
/// e.g. the terminal of a coding agent from the hook's parent processes.
///
/// With a window manager that lists windows, [`pick_window`] chooses with `hints`, and the
/// window manager focuses the choice. Where that leaves several windows, macOS asks Terminal or
/// iTerm2 which of them holds the processes' tty ([`aerospace::apps::terminal_window`]). Where
/// no window manager answers or none of the processes owns a listed window, macOS brings the
/// app forward itself ([`aerospace::apps::raise`]: the tab with the tty in Terminal and iTerm2,
/// else the nearest regular app); Linux and Windows have no such way yet.
///
/// Nothing here starts a shell or puts `hints` into a command: they are only compared with
/// window titles, and macOS's script gets the tty and the process ids as arguments.
///
/// Errors ([`NoWindow`]): `NotFound` when none of the processes has a window or an app,
/// `Ambiguous` when several windows remain, `Unsupported` without a window manager on Linux and
/// Windows, `Failed` when focusing or the system fails (`WmError::NotFound`: the window closed
/// meanwhile).
pub async fn focus_process_window(wm: &impl WindowManager, pids: &[u32], hints: &[&str]) -> Result<(), NoWindow> {
    let windows = match wm.windows(None).await {
        Ok(windows) => windows,
        Err(err) => {
            return match raise_by_system(pids).await {
                Ok(true) => Ok(()),
                Ok(false) => Err(NoWindow::NotFound),
                Err(WmError::Unsupported(_)) => Err(NoWindow::Unsupported(err.to_string())),
                Err(failed) => Err(NoWindow::Failed(failed)),
            };
        }
    };
    match pick_window(&windows, pids, hints) {
        Ok(window) => wm.focus_window(&window.id).await.map_err(NoWindow::Failed),
        // The window manager may not list the app's windows (hidden, another space): the system may.
        Err(candidates) if candidates.is_empty() => match raise_by_system(pids).await {
            Ok(true) => Ok(()),
            Ok(false) | Err(WmError::Unsupported(_)) => Err(NoWindow::NotFound),
            Err(failed) => Err(NoWindow::Failed(failed)),
        },
        Err(candidates) => match terminal_window(pids).await {
            Some(id) if candidates.iter().any(|window| window.id == id) => {
                wm.focus_window(&id).await.map_err(NoWindow::Failed)
            }
            _ => Err(NoWindow::Ambiguous { count: candidates.len() }),
        },
    }
}

/// The window of Terminal or iTerm2 that holds the tty of `pids` (macOS only).
#[cfg(all(target_os = "macos", not(test)))]
async fn terminal_window(pids: &[u32]) -> Option<WindowId> {
    aerospace::apps::terminal_window(pids).await
}

/// Tests never ask the real system (it may wait seconds for a consent dialog): no terminal holds
/// the tty of the test's processes, which run nowhere.
#[cfg(all(target_os = "macos", test))]
async fn terminal_window(_pids: &[u32]) -> Option<WindowId> {
    None
}

/// Only macOS can tell a terminal's window by a tty.
#[cfg(not(target_os = "macos"))]
async fn terminal_window(_pids: &[u32]) -> Option<WindowId> {
    None
}

/// Raises what shows `pids` without a window manager: `Ok(false)` when there is nothing to raise.
#[cfg(all(target_os = "macos", not(test)))]
async fn raise_by_system(pids: &[u32]) -> Result<bool, WmError> {
    aerospace::apps::raise(pids).await
}

/// Tests never drive the real system: the test's processes run nowhere, so nothing comes forward.
#[cfg(all(target_os = "macos", test))]
async fn raise_by_system(_pids: &[u32]) -> Result<bool, WmError> {
    Ok(false)
}

/// Linux and Windows raise windows only through their window manager (`Unsupported`).
#[cfg(not(target_os = "macos"))]
async fn raise_by_system(_pids: &[u32]) -> Result<bool, WmError> {
    Err(WmError::Unsupported("raising a window without a window manager".into()))
}

#[cfg(test)]
mod tests {
    use super::fake::{Call, FakeWm};
    use super::*;

    /// Processes no system has (macOS hands out pids below 100 000, Linux at most 4 194 304).
    const PIDS: [u32; 3] = [4_500_001, 4_500_002, 4_500_003];

    fn titled(id: &str, pid: u32, title: &str) -> Window {
        let mut window = fake::window(id, "com.apple.Terminal", "1", Rect::default());
        window.pid = Some(pid);
        window.title = title.into();
        window
    }

    /// Terminal.app's titles as AeroSpace listed them by hand on 2026-10-13: folder, the title
    /// Claude Code sets (`✳` and the session's name), the command, the size.
    fn terminal() -> Vec<Window> {
        vec![
            titled("13726", PIDS[1], "proj-alpha — ✳ Alpha job — claude --name Alpha job — 80×24"),
            titled("13732", PIDS[1], "proj-beta — ✳ Reply with ok — claude — 80×24"),
            titled("13740", PIDS[1], "proj-beta — ✳ Release notes — claude — 80×24"),
            titled("13725", PIDS[1], "wagesve — -zsh — 80×24"),
        ]
    }

    fn ids(windows: Vec<&Window>) -> Vec<&str> {
        windows.iter().map(|window| window.id.0.as_str()).collect()
    }

    #[test]
    fn the_nearest_process_with_a_window_has_it() {
        let windows = [titled("a", 7, "editor"), titled("b", PIDS[2], "launcher"), titled("c", PIDS[1], "foot")];
        assert_eq!(pick_window(&windows, &PIDS, &[]).map(|w| w.id.0.as_str()), Ok("c"), "not the launcher");
        assert_eq!(pick_window(&windows, &PIDS[2..], &["foot"]).map(|w| w.id.0.as_str()), Ok("b"));
        assert_eq!(pick_window(&windows, &[4_500_009], &[]).map_err(ids), Err(Vec::new()), "no window");
    }

    #[test]
    fn among_several_windows_the_title_decides() {
        let windows = terminal();
        let pick = |hints: &[&str]| pick_window(&windows, &PIDS, hints).map(|w| w.id.0.as_str()).map_err(ids);
        assert_eq!(pick(&["Reply with ok", "proj-beta"]), Ok("13732"), "the session's name");
        assert_eq!(pick(&["alpha JOB"]), Ok("13726"), "ignoring case");
        assert_eq!(pick(&["", "  ", "proj-alpha"]), Ok("13726"), "blank hints count for nothing");
        // No session name in any title: the project leaves two, which stay undecided.
        assert_eq!(pick(&["Not shown", "proj-beta"]), Err(vec!["13732", "13740"]));
        assert_eq!(pick(&[]), Err(vec!["13726", "13732", "13740", "13725"]));
        // A name two sessions share is narrowed by the project.
        let mut twins = terminal();
        twins[0].title = "proj-alpha — ✳ Release notes — claude — 80×24".into();
        let pick = pick_window(&twins, &PIDS, &["Release notes", "proj-beta"]).map(|w| w.id.0.as_str());
        assert_eq!(pick, Ok("13740"));
    }

    #[tokio::test]
    async fn the_chosen_window_is_focused() {
        let wm = FakeWm::new("1");
        terminal().into_iter().for_each(|window| wm.add_window(window));
        assert_eq!(focus_process_window(&wm, &PIDS, &["Release notes"]).await, Ok(()));
        assert_eq!(wm.calls(), vec![Call::Focus(WindowId("13740".into()))]);
    }

    #[tokio::test]
    async fn no_single_window_is_no_jump_with_its_reason() {
        let wm = FakeWm::new("1");
        terminal().into_iter().for_each(|window| wm.add_window(window));
        let ambiguous = focus_process_window(&wm, &PIDS, &["Not shown", "proj-beta"]).await;
        assert_eq!(ambiguous, Err(NoWindow::Ambiguous { count: 2 }));
        assert!(ambiguous.expect_err("no jump").to_string().contains("2 windows"));
        assert_eq!(focus_process_window(&wm, &[4_500_009], &[]).await, Err(NoWindow::NotFound));
        assert!(wm.calls().is_empty(), "nothing focused");

        // The terminal closed.
        let gone = FakeWm::new("1");
        gone.add_window(titled("13726", PIDS[0], "foot"));
        gone.close_window(&WindowId("13726".into())).await.expect("closed");
        assert_eq!(focus_process_window(&gone, &PIDS, &[]).await, Err(NoWindow::NotFound));
    }

    #[tokio::test]
    async fn without_a_window_manager_only_macos_raises_by_itself() {
        let none = Backend::Unsupported { name: "test".into() };
        let answer = focus_process_window(&none, &PIDS, &["Release notes"]).await;
        if cfg!(target_os = "macos") {
            // None of the processes runs, so no app comes forward.
            assert_eq!(answer, Err(NoWindow::NotFound));
        } else {
            assert_eq!(answer, Err(NoWindow::Unsupported("not supported yet: test".into())));
        }
    }
}
