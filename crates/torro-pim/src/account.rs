//! The account state the apps show: not connected, signing in, connected.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The core's account as the apps see it. One Nextcloud account for now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum AccountState {
    /// No account set up; the apps show the connect screen.
    NotConnected,
    /// Login Flow v2 is running: the user signs in at `login_url` in the browser.
    SigningIn { server: String, login_url: String },
    Connected {
        server: String,
        login_name: String,
        /// When data was last fetched from the server.
        last_sync: Option<DateTime<Utc>>,
        /// Why the last sync failed, if it did; the cached data stays usable.
        problem: Option<String>,
    },
}
