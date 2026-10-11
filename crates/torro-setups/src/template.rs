//! The template format (schema 3) and the crate's error type.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use torro_wm::{Rect, SplitDirection, WmError};

/// Version written into every template. Schema 1 and 2 belong to the Omarchy plugin.
pub const SCHEMA_VERSION: u32 = 3;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
}

impl From<&Template> for TemplateSummary {
    fn from(t: &Template) -> TemplateSummary {
        TemplateSummary { id: t.id.clone(), name: t.name.clone(), windows: t.windows.len(), updated: t.updated }
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
