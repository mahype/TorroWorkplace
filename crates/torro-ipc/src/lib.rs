//! How clients talk to the core.
//!
//! `torroworkplace daemon` owns the window manager connection, the templates and, later,
//! accounts and sync. Every client — `torroworkplace ctl`, hotkeys, later the control
//! centre and the apps — sends [`Request`]s over a local socket and gets one [`Response`]
//! each: `core.sock` in the core's state directory (a Unix domain socket; Windows named
//! pipes follow with the GlazeWM backend).
//!
//! Wire format: one JSON object per line in each direction, one request per connection.
//!
//! Task 1.17 in `docs/tasks/phase-1.md` fills in [`server`] and [`client`].

pub mod client;
pub mod server;

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_setups::LoadMode;
use torro_setups::TemplateSummary;
use torro_setups::restore::RestoreReport;

/// Where the core listens: `core.sock` in its state directory.
pub fn socket_path() -> Option<PathBuf> {
    AppPaths::for_app(AppId::WORKPLACE).map(|p| p.state.join("core.sock"))
}

/// What a client can ask the core for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "method")]
pub enum Request {
    /// Version and health; also how a client finds out the core is up.
    Ping,
    /// Short status for bars and scripts.
    Status,
    /// All templates.
    ListTemplates,
    /// Saves the active workspace as a new template; `None` picks an automatic name.
    SaveTemplate {
        name: Option<String>,
    },
    /// Overwrites a template with the active workspace, keeping its name and settings.
    UpdateTemplate {
        id: String,
    },
    DeleteTemplate {
        id: String,
    },
    /// Loads a template onto the active workspace.
    LoadTemplate {
        id: String,
        mode: LoadMode,
    },
}

/// The core's answer to one [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "result")]
pub enum Response {
    Pong {
        version: String,
    },
    Status(Status),
    Templates {
        templates: Vec<TemplateSummary>,
    },
    Saved {
        template: TemplateSummary,
    },
    Loaded {
        restored: usize,
        failed: Vec<(String, String)>,
    },
    /// Done, nothing to report.
    Ok,
    /// The request failed; `code` is stable for scripts (`workspace-not-empty`, `not-found`,
    /// `unsupported`, `busy`, `error`), `message` is for people.
    Error {
        code: String,
        message: String,
    },
}

impl Response {
    pub fn loaded(report: &RestoreReport) -> Response {
        Response::Loaded { restored: report.restored.len(), failed: report.failed.clone() }
    }
}

/// What `status --json` prints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub version: String,
    /// Backend name (`hyprland`) or `unsupported`.
    pub backend: String,
    pub workspace: Option<String>,
    pub templates: usize,
    /// A load or save is running.
    pub busy: bool,
}

/// Why talking to the core failed.
#[derive(Debug)]
pub enum IpcError {
    /// Another core already listens on the socket.
    AlreadyRunning,
    /// No core listens on the socket.
    NotRunning,
    /// Not available on this system yet (Windows).
    Unsupported,
    Io(std::io::Error),
    /// A line that is not a request/response.
    Protocol(String),
    /// Part of the scaffold that a task in `docs/tasks/` still has to fill in.
    NotImplemented(&'static str),
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpcError::AlreadyRunning => f.write_str("torroworkplace is already running"),
            IpcError::NotRunning => {
                f.write_str("torroworkplace is not running (start it with `torroworkplace daemon`)")
            }
            IpcError::Unsupported => f.write_str("not supported on this system yet"),
            IpcError::Io(err) => write!(f, "socket: {err}"),
            IpcError::Protocol(err) => write!(f, "unexpected message: {err}"),
            IpcError::NotImplemented(what) => write!(f, "not implemented yet: {what}"),
        }
    }
}

impl std::error::Error for IpcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_tagged_json() {
        let req = Request::LoadTemplate { id: "dev".into(), mode: LoadMode::Add };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"method":"load-template","id":"dev","mode":"add"}"#);
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), req);
    }
}
