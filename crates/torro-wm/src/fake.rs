//! A window manager in memory, for tests.
//!
//! [`FakeWm`] keeps a list of windows, records every command it gets as a [`Call`] and
//! sends events to subscribers like a real backend. Tests set it up with
//! [`FakeWm::add_window`], simulate an application showing up with
//! [`FakeWm::open_window`] and check [`FakeWm::calls`] afterwards. Key binds and window rules are
//! only recorded; [`FakeWm::reload_config`] sends [`WmEvent::ConfigReloaded`] like Hyprland does.

use std::sync::{Mutex, MutexGuard};

use tokio::sync::mpsc;

use crate::{
    Events, KeyBind, Monitor, Rect, Split, SplitDirection, Window, WindowId, WindowManager, WmError, WmEvent,
    WorkspaceId,
};

/// A command the fake received.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Focus(WindowId),
    Move(WindowId, WorkspaceId),
    Close(WindowId),
    PrepareInsert(WindowId, SplitDirection),
    Place {
        window: WindowId,
        sibling: WindowId,
        split: Split,
    },
    /// [`WindowManager::focus_monitor`] with the monitor's name, also when no monitor has it.
    FocusMonitor(String),
    BindKeys(Vec<KeyBind>),
    UnbindKeys(Vec<String>),
    /// [`WindowManager::float_dialogs`] with the class.
    FloatDialogs(String),
}

#[derive(Debug, Default)]
struct State {
    active: Option<WorkspaceId>,
    monitors: Vec<Monitor>,
    windows: Vec<Window>,
    calls: Vec<Call>,
    subscribers: Vec<mpsc::Sender<WmEvent>>,
    next_id: u64,
}

/// See the module docs.
#[derive(Debug, Default)]
pub struct FakeWm {
    state: Mutex<State>,
}

/// A tiled window for tests, with a made-up title and no pid.
pub fn window(id: &str, class: &str, workspace: &str, rect: Rect) -> Window {
    Window {
        id: WindowId(id.to_string()),
        class: class.to_string(),
        title: format!("{class} window"),
        pid: None,
        workspace: WorkspaceId(workspace.to_string()),
        monitor: Some("DP-1".to_string()),
        rect,
        floating: false,
        fullscreen: false,
    }
}

impl FakeWm {
    /// An empty fake with `active` as the focused workspace, shown on one monitor `DP-1`
    /// at 0,0 with 3840×1080.
    pub fn new(active: &str) -> FakeWm {
        let fake = FakeWm::default();
        {
            let mut state = fake.lock();
            state.active = Some(WorkspaceId(active.to_string()));
            state.monitors = vec![Monitor {
                name: "DP-1".into(),
                rect: Rect { x: 0, y: 0, width: 3840, height: 1080 },
                scale: 1.0,
                active_workspace: state.active.clone(),
            }];
        }
        fake
    }

    /// Replaces the monitors.
    pub fn set_monitors(&self, monitors: Vec<Monitor>) {
        self.lock().monitors = monitors;
    }

    /// Adds a window that already exists; sends no event.
    pub fn add_window(&self, window: Window) {
        self.lock().windows.push(window);
    }

    /// Simulates an application opening a window on the active workspace: adds it and
    /// sends [`WmEvent::WindowOpened`]. Returns its id (`0xfake1`, `0xfake2`, …).
    pub fn open_window(&self, class: &str, pid: Option<u32>) -> WindowId {
        let mut state = self.lock();
        state.next_id += 1;
        let id = WindowId(format!("0xfake{}", state.next_id));
        let workspace = state.active.clone().unwrap_or_else(|| WorkspaceId("1".into()));
        let mut new = window(&id.0, class, &workspace.0, Rect::default());
        new.pid = pid;
        state.windows.push(new);
        let event = WmEvent::WindowOpened { window: id.clone(), class: class.to_string(), workspace };
        send(&mut state, event);
        id
    }

    /// Switches the focused workspace and sends [`WmEvent::WorkspaceFocused`].
    pub fn focus_workspace(&self, workspace: &str) {
        let mut state = self.lock();
        let workspace = WorkspaceId(workspace.to_string());
        state.active = Some(workspace.clone());
        send(&mut state, WmEvent::WorkspaceFocused { workspace });
    }

    /// Simulates the window manager reloading its configuration: sends [`WmEvent::ConfigReloaded`].
    pub fn reload_config(&self) {
        send(&mut self.lock(), WmEvent::ConfigReloaded);
    }

    /// Every command received so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    /// The windows as they are now, on all workspaces.
    pub fn all_windows(&self) -> Vec<Window> {
        self.lock().windows.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A test that panicked while holding the lock already failed; keep the state usable.
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn send(state: &mut State, event: WmEvent) {
    state.subscribers.retain(|tx| tx.try_send(event.clone()).is_ok());
}

impl WindowManager for FakeWm {
    fn name(&self) -> &'static str {
        "fake"
    }

    async fn active_workspace(&self) -> Result<WorkspaceId, WmError> {
        self.lock().active.clone().ok_or_else(|| WmError::NotFound("no active workspace".into()))
    }

    async fn windows(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>, WmError> {
        let state = self.lock();
        Ok(state.windows.iter().filter(|w| workspace.is_none_or(|ws| &w.workspace == ws)).cloned().collect())
    }

    async fn monitors(&self) -> Result<Vec<Monitor>, WmError> {
        Ok(self.lock().monitors.clone())
    }

    async fn focus_window(&self, window: &WindowId) -> Result<(), WmError> {
        let mut state = self.lock();
        state.calls.push(Call::Focus(window.clone()));
        if state.windows.iter().any(|w| &w.id == window) { Ok(()) } else { Err(WmError::NotFound(window.0.clone())) }
    }

    async fn move_to_workspace(&self, window: &WindowId, workspace: &WorkspaceId) -> Result<(), WmError> {
        let mut state = self.lock();
        state.calls.push(Call::Move(window.clone(), workspace.clone()));
        let Some(found) = state.windows.iter_mut().find(|w| &w.id == window) else {
            return Err(WmError::NotFound(window.0.clone()));
        };
        found.workspace = workspace.clone();
        let event = WmEvent::WindowMoved { window: window.clone(), workspace: workspace.clone() };
        send(&mut state, event);
        Ok(())
    }

    async fn close_window(&self, window: &WindowId) -> Result<(), WmError> {
        let mut state = self.lock();
        state.calls.push(Call::Close(window.clone()));
        let before = state.windows.len();
        state.windows.retain(|w| &w.id != window);
        if state.windows.len() == before {
            return Err(WmError::NotFound(window.0.clone()));
        }
        send(&mut state, WmEvent::WindowClosed { window: window.clone() });
        Ok(())
    }

    async fn prepare_insert(&self, sibling: &WindowId, direction: SplitDirection) -> Result<(), WmError> {
        self.lock().calls.push(Call::PrepareInsert(sibling.clone(), direction));
        Ok(())
    }

    async fn place(&self, window: &WindowId, sibling: &WindowId, split: Split) -> Result<(), WmError> {
        self.lock().calls.push(Call::Place { window: window.clone(), sibling: sibling.clone(), split });
        Ok(())
    }

    async fn events(&self) -> Result<Events, WmError> {
        let (tx, rx) = mpsc::channel(64);
        self.lock().subscribers.push(tx);
        Ok(rx)
    }

    /// The workspace the monitor shows becomes the active one, with [`WmEvent::WorkspaceFocused`] when it
    /// changes. `NotFound` for a name no monitor has, and for a monitor that shows no workspace.
    async fn focus_monitor(&self, monitor: &str) -> Result<(), WmError> {
        let mut state = self.lock();
        state.calls.push(Call::FocusMonitor(monitor.to_string()));
        let shown = state.monitors.iter().find(|m| m.name == monitor).map(|m| m.active_workspace.clone());
        let Some(Some(workspace)) = shown else {
            return Err(WmError::NotFound(format!("no monitor named {monitor:?} with a workspace")));
        };
        if state.active.as_ref() != Some(&workspace) {
            state.active = Some(workspace.clone());
            send(&mut state, WmEvent::WorkspaceFocused { workspace });
        }
        Ok(())
    }

    async fn bind_keys(&self, binds: &[KeyBind]) -> Result<(), WmError> {
        self.lock().calls.push(Call::BindKeys(binds.to_vec()));
        Ok(())
    }

    async fn unbind_keys(&self, keys: &[String]) -> Result<(), WmError> {
        self.lock().calls.push(Call::UnbindKeys(keys.to_vec()));
        Ok(())
    }

    async fn float_dialogs(&self, class: &str) -> Result<(), WmError> {
        self.lock().calls.push(Call::FloatDialogs(class.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_calls_and_sends_events() {
        let wm = FakeWm::new("3");
        wm.add_window(window("0xa", "foot", "3", Rect::default()));
        wm.add_window(window("0xb", "chromium", "5", Rect::default()));
        let mut events = wm.events().await.unwrap();

        assert_eq!(wm.windows(Some(&WorkspaceId("3".into()))).await.unwrap().len(), 1);
        wm.move_to_workspace(&WindowId("0xb".into()), &WorkspaceId("3".into())).await.unwrap();
        let opened = wm.open_window("foot", Some(42));

        assert_eq!(wm.windows(Some(&WorkspaceId("3".into()))).await.unwrap().len(), 3);
        assert_eq!(wm.calls(), vec![Call::Move(WindowId("0xb".into()), WorkspaceId("3".into()))]);
        assert!(matches!(events.recv().await, Some(WmEvent::WindowMoved { .. })));
        assert!(matches!(events.recv().await, Some(WmEvent::WindowOpened { window, .. }) if window == opened));
    }

    fn monitor(name: &str, x: i32, workspace: &str) -> Monitor {
        Monitor {
            name: name.into(),
            rect: Rect { x, y: 0, width: 1920, height: 1080 },
            scale: 1.0,
            active_workspace: Some(WorkspaceId(workspace.into())),
        }
    }

    #[tokio::test]
    async fn focus_monitor_makes_its_workspace_active() {
        let wm = FakeWm::new("1");
        wm.set_monitors(vec![monitor("DP-1", 0, "1"), monitor("HDMI-A-1", 1920, "7")]);
        let mut events = wm.events().await.expect("events");

        wm.focus_monitor("HDMI-A-1").await.expect("focused");
        assert_eq!(wm.active_workspace().await, Ok(WorkspaceId("7".into())));
        assert_eq!(events.recv().await, Some(WmEvent::WorkspaceFocused { workspace: WorkspaceId("7".into()) }));

        // Focusing it again changes nothing and sends nothing.
        wm.focus_monitor("HDMI-A-1").await.expect("focused");
        assert!(matches!(wm.focus_monitor("DP-9").await, Err(WmError::NotFound(_))));
        assert_eq!(wm.active_workspace().await, Ok(WorkspaceId("7".into())));
        wm.reload_config();
        assert_eq!(events.recv().await, Some(WmEvent::ConfigReloaded), "no second WorkspaceFocused before it");
        assert_eq!(
            wm.calls(),
            vec![
                Call::FocusMonitor("HDMI-A-1".into()),
                Call::FocusMonitor("HDMI-A-1".into()),
                Call::FocusMonitor("DP-9".into())
            ]
        );
    }

    #[tokio::test]
    async fn binds_and_rules_are_recorded() {
        let wm = FakeWm::new("1");
        let bind = KeyBind { keys: "SUPER + ALT + 1".into(), command: vec!["torroworkplace".into()] };
        wm.bind_keys(std::slice::from_ref(&bind)).await.expect("bound");
        wm.unbind_keys(std::slice::from_ref(&bind.keys)).await.expect("unbound");
        wm.float_dialogs("torro-dialog").await.expect("floated");
        assert_eq!(
            wm.calls(),
            vec![
                Call::BindKeys(vec![bind.clone()]),
                Call::UnbindKeys(vec![bind.keys]),
                Call::FloatDialogs("torro-dialog".into())
            ]
        );
        assert_eq!(wm.version().await, None);
    }
}
