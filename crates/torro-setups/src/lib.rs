//! Workspace templates.
//!
//! A template describes one workspace: which applications, how they were tiled, how each
//! window comes back. [`capture`] makes one from the active workspace, [`storage`] keeps
//! them on disk, [`restore`] rebuilds one on the active workspace. Everything talks to the
//! window manager through [`torro_wm::WindowManager`], so the same code serves Hyprland,
//! AeroSpace and GlazeWM.
//!
//! The Omarchy plugin in `apps/workspace-setups` is the reference for what these modules do.

pub mod capture;
pub mod launch;
pub mod layout;
pub mod restore;
pub mod storage;
mod template;

pub use template::{
    LayoutTree, LoadMode, MAX_SHORTCUT, SCHEMA_VERSION, ScreenFallback, SetupsError, Source, Startup, Strategy,
    Template, TemplateSummary, TemplateWindow, main_monitor,
};
