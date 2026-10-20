//! Neutral types every backend maps its own model onto.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A window as the backend identifies it: Hyprland's address (`0x55d0…`), AeroSpace's
/// window id, GlazeWM's container id. Only meaningful while the window lives.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowId(pub String);

/// A workspace by the name the backend uses (`"3"`, `"web"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceId(pub String);

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// A monitor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Monitor {
    /// Connector or system name, e.g. `DP-1`.
    pub name: String,
    pub rect: Rect,
    pub scale: f32,
    /// The workspace currently shown on it.
    pub active_workspace: Option<WorkspaceId>,
}

/// A window that should cover one monitor whole (task ER2.2): the reminder over the whole screen,
/// told apart by its class and its title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cover {
    /// The window's class (app id), e.g. `torro-reminder`.
    pub class: String,
    /// The window's exact first title, e.g. `Reminder · DP-1`.
    pub title: String,
    /// The monitor it covers, by name ([`Monitor::name`]).
    pub monitor: String,
    /// Whether it takes the keyboard when it appears (the main monitor's does, the others do not).
    pub focus: bool,
}

/// A global key bind that runs a command (task WS.1), e.g. `SUPER + ALT + 1` → `torroworkplace load …`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBind {
    /// The keys in Hyprland's spelling: modifiers and the key joined by ` + `, e.g. `SUPER + ALT + 1`.
    /// [`crate::WindowManager::unbind_keys`] needs exactly this text again (Hyprland compares it
    /// case-sensitively), so callers keep it as they bound it.
    pub keys: String,
    /// The program and its arguments, one element each, never a shell line. The backend quotes each
    /// element for the shell itself, so spaces and quotes in paths and ids are safe.
    pub command: Vec<String>,
}

/// A normal window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub id: WindowId,
    /// What identifies the application: the Wayland app id / X11 class on Linux, the
    /// bundle id on macOS, the executable name on Windows.
    pub class: String,
    pub title: String,
    pub pid: Option<u32>,
    pub workspace: WorkspaceId,
    pub monitor: Option<String>,
    /// Position and size on the desktop (not relative to the monitor).
    pub rect: Rect,
    pub floating: bool,
    pub fullscreen: bool,
}

/// How a split divides its space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SplitDirection {
    /// First child left, second right (the dividing line is vertical).
    LeftRight,
    /// First child on top, second below (the dividing line is horizontal).
    TopBottom,
}

/// One split of the layout tree.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Split {
    pub direction: SplitDirection,
    /// Share of the first child, `0.0 < ratio < 1.0`; `0.5` is an even split.
    pub ratio: f32,
}

/// Something happened in the window manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WmEvent {
    WindowOpened {
        window: WindowId,
        class: String,
        workspace: WorkspaceId,
    },
    WindowClosed {
        window: WindowId,
    },
    WindowMoved {
        window: WindowId,
        workspace: WorkspaceId,
    },
    WorkspaceFocused {
        workspace: WorkspaceId,
    },
    /// The window manager read its configuration again (Hyprland: `configreloaded`). Whatever was
    /// registered at run time – key binds ([`crate::WindowManager::bind_keys`]), window rules
    /// ([`crate::WindowManager::float_dialogs`], [`crate::WindowManager::cover_monitors`]) – is gone
    /// and has to be registered again. Only Hyprland sends it.
    ConfigReloaded,
}

/// Why a window manager call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WmError {
    /// The window manager is not running or its socket is missing.
    NotRunning(String),
    /// No backend for this system or window manager (yet).
    Unsupported(String),
    /// Reading or writing the connection failed.
    Io(String),
    /// The window manager answered something we could not read, or refused a command.
    Protocol(String),
    /// No such window or workspace.
    NotFound(String),
    /// Part of the scaffold that a task in `docs/tasks/` still has to fill in.
    NotImplemented(&'static str),
}

impl fmt::Display for WmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WmError::NotRunning(what) => write!(f, "window manager not running: {what}"),
            WmError::Unsupported(what) => write!(f, "not supported yet: {what}"),
            WmError::Io(err) => write!(f, "window manager connection: {err}"),
            WmError::Protocol(err) => write!(f, "window manager answer: {err}"),
            WmError::NotFound(what) => write!(f, "not found: {what}"),
            WmError::NotImplemented(what) => write!(f, "not implemented yet: {what}"),
        }
    }
}

impl std::error::Error for WmError {}

/// Why [`crate::focus_process_window`] brought no window to the front (task E8.9).
///
/// [`fmt::Display`] says it in English for people, e.g. in the `unsupported` answer of
/// `inbox.open_item`. Nothing in it comes from the window titles or the hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoWindow {
    /// None of the processes owns a window, and none of them is an app the system could bring
    /// forward instead: the terminal was closed, or the agent runs under a tmux server, which
    /// belongs to no window.
    NotFound,
    /// The nearest process with windows owns `count` of them, and neither the title hints nor
    /// the terminal (macOS: Terminal, iTerm2, by the agent's tty) single out one.
    Ambiguous { count: usize },
    /// No window manager answers and this system has no other way to raise a window (Linux
    /// without Hyprland, Windows without GlazeWM). Holds the window manager's reason.
    Unsupported(String),
    /// Raising failed: the window manager refused, or the window closed meanwhile
    /// ([`WmError::NotFound`]).
    Failed(WmError),
}

impl fmt::Display for NoWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NoWindow::NotFound => f.write_str("no window of the agent's processes is open"),
            NoWindow::Ambiguous { count } => write!(
                f,
                "the agent's terminal has {count} windows and none of their titles names the session; give the \
                 session a name the terminal shows in its title"
            ),
            NoWindow::Unsupported(reason) => write!(f, "cannot raise windows here: {reason}"),
            NoWindow::Failed(err) => write!(f, "cannot raise the window: {err}"),
        }
    }
}

impl std::error::Error for NoWindow {}
