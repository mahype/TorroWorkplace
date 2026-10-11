//! Tiling window managers behind one trait.
//!
//! The core talks to whichever tiling window manager runs — Hyprland on Linux, AeroSpace on
//! macOS, GlazeWM on Windows — through [`WindowManager`]. The types here are neutral: they
//! describe windows, workspaces and splits the way every backend can map them.
//! [`detect`] picks the backend at runtime; [`fake::FakeWm`] stands in for tests.

pub mod detect;
pub mod fake;
pub mod glazewm;
pub mod hyprland;
mod types;

use std::future::Future;

pub use detect::detect;
pub use types::{Monitor, Rect, Split, SplitDirection, Window, WindowId, WmError, WmEvent, WorkspaceId};

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
}

/// The backend chosen at runtime.
#[derive(Debug)]
pub enum Backend {
    Hyprland(hyprland::Hyprland),
    GlazeWm(glazewm::GlazeWm),
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
            Backend::Unsupported { .. } => "unsupported",
        }
    }

    async fn active_workspace(&self) -> Result<WorkspaceId, WmError> {
        match self {
            Backend::Hyprland(wm) => wm.active_workspace().await,
            Backend::GlazeWm(wm) => wm.active_workspace().await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn windows(&self, workspace: Option<&WorkspaceId>) -> Result<Vec<Window>, WmError> {
        match self {
            Backend::Hyprland(wm) => wm.windows(workspace).await,
            Backend::GlazeWm(wm) => wm.windows(workspace).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn monitors(&self) -> Result<Vec<Monitor>, WmError> {
        match self {
            Backend::Hyprland(wm) => wm.monitors().await,
            Backend::GlazeWm(wm) => wm.monitors().await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn focus_window(&self, window: &WindowId) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.focus_window(window).await,
            Backend::GlazeWm(wm) => wm.focus_window(window).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn move_to_workspace(&self, window: &WindowId, workspace: &WorkspaceId) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.move_to_workspace(window, workspace).await,
            Backend::GlazeWm(wm) => wm.move_to_workspace(window, workspace).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn close_window(&self, window: &WindowId) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.close_window(window).await,
            Backend::GlazeWm(wm) => wm.close_window(window).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn prepare_insert(&self, sibling: &WindowId, direction: SplitDirection) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.prepare_insert(sibling, direction).await,
            Backend::GlazeWm(wm) => wm.prepare_insert(sibling, direction).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn place(&self, window: &WindowId, sibling: &WindowId, split: Split) -> Result<(), WmError> {
        match self {
            Backend::Hyprland(wm) => wm.place(window, sibling, split).await,
            Backend::GlazeWm(wm) => wm.place(window, sibling, split).await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }

    async fn events(&self) -> Result<Events, WmError> {
        match self {
            Backend::Hyprland(wm) => wm.events().await,
            Backend::GlazeWm(wm) => wm.events().await,
            Backend::Unsupported { name } => Err(WmError::Unsupported(name.clone())),
        }
    }
}
