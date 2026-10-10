//! How the apps talk to the core.
//!
//! `torroworkplace daemon` owns accounts, sync, the app registry and the MCP server.
//! Every app — TUI or GUI — is a client: it sends requests over a local socket (a Unix
//! domain socket on Linux and macOS, a named pipe on Windows) and gets answers and change
//! notifications back. An app that finds no core running starts it.
//!
//! Scaffold: the socket location and the request vocabulary. Transport and encoding
//! (newline-delimited JSON-RPC is the plan) come with the core.

use std::path::PathBuf;

use torro_core::AppId;
use torro_core::paths::AppPaths;

/// Where the core listens: `core.sock` in its state directory.
pub fn socket_path() -> Option<PathBuf> {
    AppPaths::for_app(AppId::WORKPLACE).map(|p| p.state.join("core.sock"))
}

/// What an app can ask the core for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Version and health; also how a client finds out the core is up.
    Ping,
    /// The suite's apps: installed, running, window class.
    Apps,
    /// Start an app, or focus its window when it already runs.
    LaunchOrFocus { window_class: String },
    /// Collections of all accounts.
    Collections,
    /// Kick off a sync of all accounts now.
    Sync,
}
