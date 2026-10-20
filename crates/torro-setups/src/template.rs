//! The template format (schema 3) and the crate's error type.

use std::fmt;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use torro_wm::{Monitor, Rect, SplitDirection, WmError};

/// Version written into every template. Schema 1 and 2 belong to the Omarchy plugin.
pub const SCHEMA_VERSION: u32 = 3;

/// The highest key slot: slots are the digit keys `0` to `9` ([`Template::shortcut`]).
pub const MAX_SHORTCUT: u8 = 9;

/// One saved workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub schema_version: u32,
    /// File name without `.json`; a slug of the first name, never changes.
    pub id: String,
    /// Shown in the menu; may be renamed.
    pub name: String,
    /// Backend that captured it (`hyprland`, `aerospace`, `glazewm`).
    pub wm: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub source: Source,
    /// Tiling layout over the tiled windows; `None` when nothing was tiled.
    pub tree: Option<LayoutTree>,
    pub windows: Vec<TemplateWindow>,
    /// The digit key (`0` to [`MAX_SHORTCUT`]) that loads the template, held with the modifiers of
    /// the settings (`torro_core::settings::WorkspaceSettings`). At most one template holds a key
    /// (`Store::set_shortcut` takes it from the other one). Absent in files from before issue #243.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<u8>,
    /// Load the template once at every login, and where (issue #243); `None`: never at login.
    /// Absent in files from before issue #243.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<Startup>,
}

/// Loading a template at login (issue #243): onto the workspace the screen `screen` shows, in mode
/// `add`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Startup {
    /// The screen by its window manager name (`DP-1`, from `workspaces.list_screens`). A screen
    /// that is not connected now is allowed: it may be at the next login.
    pub screen: String,
    /// What happens when that screen is missing at login; `main` unless given.
    #[serde(default)]
    pub fallback: ScreenFallback,
}

/// What a load at login does when its screen is missing ([`Startup::fallback`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ScreenFallback {
    /// `"main"`: load onto the main screen instead ([`main_monitor`]).
    #[default]
    Main,
    /// `"skip"`: do not load the template at this login.
    Skip,
}

impl Startup {
    /// The monitor of `monitors` this template loads onto at login: the one named
    /// [`Startup::screen`]; when it is missing the main one ([`main_monitor`]) for
    /// [`ScreenFallback::Main`], none for [`ScreenFallback::Skip`].
    pub fn monitor_in<'a>(&self, monitors: &'a [Monitor]) -> Option<&'a Monitor> {
        let named = monitors.iter().find(|monitor| monitor.name == self.screen);
        match (named, self.fallback) {
            (Some(monitor), _) => Some(monitor),
            (None, ScreenFallback::Main) => main_monitor(monitors),
            (None, ScreenFallback::Skip) => None,
        }
    }
}

/// The main screen: the monitor whose top-left corner is at 0,0, else the first one listed; `None`
/// without monitors. `workspaces.list_screens` marks it, loads at login fall back to it.
pub fn main_monitor(monitors: &[Monitor]) -> Option<&Monitor> {
    monitors.iter().find(|monitor| monitor.rect.x == 0 && monitor.rect.y == 0).or_else(|| monitors.first())
}

/// Where a template was captured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub workspace: String,
    pub monitor: Option<String>,
    /// Monitor rectangle, to scale floating windows onto another monitor.
    pub monitor_rect: Option<Rect>,
}

/// One window of a template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateWindow {
    /// `window-1`, `window-2`, … ; referenced from [`LayoutTree::Window`].
    pub id: String,
    /// App id / class / bundle id, see [`torro_wm::Window::class`].
    pub class: String,
    /// Title at capture time; only a hint for matching and for the UI.
    pub title: String,
    /// How to start it; empty when unknown (then only [`Strategy::Move`] can bring it back).
    pub command: Vec<String>,
    pub strategy: Strategy,
    pub floating: bool,
    /// Position relative to the monitor's top-left corner.
    pub rect: Rect,
}

/// How a window comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    /// Always start the program (terminals, browsers, editors).
    New,
    /// Bring an open window from another workspace; start the program only when none is open
    /// (programs that run once: Spotify, Slack, Signal).
    Move,
}

/// The tiling layout as a binary tree, inferred from window rectangles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum LayoutTree {
    Window {
        /// A [`TemplateWindow::id`].
        id: String,
    },
    Split {
        direction: SplitDirection,
        /// Share of `first`, `0.0 < ratio < 1.0`, four decimals.
        ratio: f32,
        first: Box<LayoutTree>,
        second: Box<LayoutTree>,
    },
    /// Windows whose arrangement could not be split cleanly; restored in this order without
    /// layout instructions.
    Fallback { ids: Vec<String> },
}

/// What a load does with windows already on the target workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LoadMode {
    /// Only load into an empty workspace; otherwise fail with
    /// [`SetupsError::WorkspaceNotEmpty`].
    #[default]
    Empty,
    /// Keep the windows that are there and add the template's.
    Add,
    /// Close the windows that are there first.
    Replace,
}

/// What menus and `ctl setups list` show of a template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSummary {
    pub id: String,
    pub name: String,
    pub windows: usize,
    pub updated: DateTime<Utc>,
    /// [`Template::shortcut`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<u8>,
    /// [`Template::startup`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<Startup>,
    /// The workspace it was captured from ([`Source::workspace`]).
    #[serde(default)]
    pub workspace: String,
    /// The screen it was captured on ([`Source::monitor`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
}

impl From<&Template> for TemplateSummary {
    fn from(t: &Template) -> TemplateSummary {
        TemplateSummary {
            id: t.id.clone(),
            name: t.name.clone(),
            windows: t.windows.len(),
            updated: t.updated,
            shortcut: t.shortcut,
            startup: t.startup.clone(),
            workspace: t.source.workspace.clone(),
            monitor: t.source.monitor.clone(),
        }
    }
}

/// Why a template operation failed.
#[derive(Debug, Clone, PartialEq)]
pub enum SetupsError {
    /// Load in [`LoadMode::Empty`] found windows on the target workspace.
    WorkspaceNotEmpty {
        windows: usize,
    },
    /// No template with this id.
    NotFound(String),
    /// No home directory, so no data directory.
    NoHome,
    Io(String),
    /// A template file that is not what we wrote.
    Format(String),
    /// Starting a program failed.
    Launch(String),
    Wm(WmError),
    /// Part of the scaffold that a task in `docs/tasks/` still has to fill in.
    NotImplemented(&'static str),
}

impl fmt::Display for SetupsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupsError::WorkspaceNotEmpty { windows } => write!(f, "workspace is not empty ({windows} windows)"),
            SetupsError::NotFound(id) => write!(f, "no template {id:?}"),
            SetupsError::NoHome => f.write_str("no home directory"),
            SetupsError::Io(err) => write!(f, "templates: {err}"),
            SetupsError::Format(err) => write!(f, "template file damaged: {err}"),
            SetupsError::Launch(err) => write!(f, "could not start program: {err}"),
            SetupsError::Wm(err) => err.fmt(f),
            SetupsError::NotImplemented(what) => write!(f, "not implemented yet: {what}"),
        }
    }
}

impl std::error::Error for SetupsError {}

impl From<WmError> for SetupsError {
    fn from(err: WmError) -> SetupsError {
        SetupsError::Wm(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, x: i32, y: i32) -> Monitor {
        Monitor {
            name: name.into(),
            rect: Rect { x, y, width: 1920, height: 1080 },
            scale: 1.0,
            active_workspace: None,
        }
    }

    /// A template written before issue #243 has neither key nor startup; it reads as one without
    /// both and is written back without the fields.
    #[test]
    fn a_template_from_before_keys_and_startup_round_trips() {
        let old = r#"{"schemaVersion":3,"id":"dev","name":"Dev","wm":"hyprland",
            "created":"2026-10-01T09:00:00Z","updated":"2026-10-01T09:00:00Z",
            "source":{"workspace":"3","monitor":"DP-1","monitorRect":null},"tree":null,"windows":[]}"#;
        let template: Template = serde_json::from_str(old).unwrap();
        assert_eq!((template.shortcut, template.startup.clone()), (None, None));
        let written = serde_json::to_string(&template).unwrap();
        assert!(!written.contains("shortcut") && !written.contains("startup"), "{written}");
        assert_eq!(serde_json::from_str::<Template>(&written).unwrap(), template);

        let summary = TemplateSummary::from(&template);
        assert_eq!((summary.workspace.as_str(), summary.monitor.as_deref()), ("3", Some("DP-1")));
    }

    #[test]
    fn startup_reads_without_fallback_as_main() {
        let startup: Startup = serde_json::from_str(r#"{"screen":"DP-1"}"#).unwrap();
        assert_eq!(startup, Startup { screen: "DP-1".into(), fallback: ScreenFallback::Main });
        let skip: Startup = serde_json::from_str(r#"{"screen":"HDMI-A-1","fallback":"skip"}"#).unwrap();
        assert_eq!(skip.fallback, ScreenFallback::Skip);
    }

    #[test]
    fn the_main_screen_is_at_the_origin_else_the_first() {
        let monitors = [monitor("HDMI-A-1", 1920, 0), monitor("DP-1", 0, 0)];
        assert_eq!(main_monitor(&monitors).map(|m| m.name.as_str()), Some("DP-1"));
        let shifted = [monitor("HDMI-A-1", 1920, 0), monitor("DP-1", 3840, 0)];
        assert_eq!(main_monitor(&shifted).map(|m| m.name.as_str()), Some("HDMI-A-1"));
        assert_eq!(main_monitor(&[]), None);
    }

    #[test]
    fn a_missing_screen_falls_back_to_the_main_one_or_none() {
        let monitors = [monitor("DP-1", 0, 0), monitor("HDMI-A-1", 1920, 0)];
        let on = |screen: &str, fallback| Startup { screen: screen.into(), fallback };
        let found = on("HDMI-A-1", ScreenFallback::Skip).monitor_in(&monitors);
        assert_eq!(found.map(|m| m.name.as_str()), Some("HDMI-A-1"));
        let main = on("DP-3", ScreenFallback::Main).monitor_in(&monitors);
        assert_eq!(main.map(|m| m.name.as_str()), Some("DP-1"));
        assert_eq!(on("DP-3", ScreenFallback::Skip).monitor_in(&monitors), None);
    }

    #[test]
    fn layout_tree_round_trips_as_tagged_json() {
        let tree = LayoutTree::Split {
            direction: SplitDirection::LeftRight,
            ratio: 0.35,
            first: Box::new(LayoutTree::Window { id: "window-1".into() }),
            second: Box::new(LayoutTree::Fallback { ids: vec!["window-2".into(), "window-3".into()] }),
        };
        let json = serde_json::to_string(&tree).unwrap();
        assert!(json.contains(r#""type":"split""#), "{json}");
        assert!(json.contains(r#""direction":"left-right""#), "{json}");
        assert_eq!(serde_json::from_str::<LayoutTree>(&json).unwrap(), tree);
    }
}
