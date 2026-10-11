//! The core's Nextcloud account: the stored account and its app password, and signing in
//! with Login Flow v2.
//!
//! Task K.6 in `docs/tasks/kalender-aufgaben.md`. Building blocks that exist:
//! `torro_core::account` (accounts.json + keychain), `torro_pim_caldav::nextcloud`
//! (`normalize_server`, `start_login`, `LoginFlow::poll`, `dav_url`), `torro_pim_caldav::
//! http_client`, `torro_pim_caldav::CalDavClient`. `pim.rs` (task K.5) uses this module.

use torro_core::Account;
use torro_pim_caldav::CalDavClient;
use torro_pim_caldav::nextcloud::LoginFlow;

/// A signed-in account with its secret.
#[derive(Debug, Clone)]
pub struct Connected {
    pub account: Account,
    pub app_password: String,
}

/// The stored Nextcloud account with its secret from the keychain; `None` when there is
/// none (or its secret is gone).
pub fn stored() -> Result<Option<Connected>, String> {
    Err("account::stored not implemented yet (task K.6)".into())
}

/// Starts Login Flow v2 for what the user typed and opens the login URL in the browser
/// (`webbrowser`); a browser that cannot be opened is not an error (the apps show the URL).
pub async fn start(http: &reqwest::Client, server_input: &str) -> Result<LoginFlow, String> {
    let _ = (http, server_input);
    Err("account::start not implemented yet (task K.6)".into())
}

/// Polls every 2 s until the user granted access (at most 20 minutes), then stores the
/// account (replacing any other Nextcloud account) and its app password.
pub async fn finish(http: &reqwest::Client, flow: &LoginFlow) -> Result<Connected, String> {
    let _ = (http, flow);
    Err("account::finish not implemented yet (task K.6)".into())
}

/// Removes the stored account and its secret.
pub fn forget() -> Result<(), String> {
    Err("account::forget not implemented yet (task K.6)".into())
}

/// A CalDAV client for the account (`<server>/remote.php/dav/`).
pub fn caldav(http: &reqwest::Client, connected: &Connected) -> Result<CalDavClient, String> {
    let _ = (http, connected);
    Err("account::caldav not implemented yet (task K.6)".into())
}
