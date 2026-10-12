//! CalDAV and iCalendar for torro-pim.
//!
//! Open standards first: Nextcloud, iCloud, Fastmail, Radicale and every other CalDAV
//! server, events (VEVENT) and tasks (VTODO) alike. Nextcloud comes first and gets its
//! own sign-in (`nextcloud`, Login Flow v2).
//!
//! - `client`: discovery (`current-user-principal`, `calendar-home-set`), collections,
//!   events in a time range and tasks (task K.3 in `docs/tasks/kalender-aufgaben.md`), and which
//!   services an account offers ([`Services`], task M7.1).
//! - `discover`: finding the CalDAV service of a mail address without signing in (task M7.1).
//! - `write`: iCalendar output and ETag-guarded PUT/DELETE (task K.4).
//! - `ical`: reading iCalendar.
//!
//! `sync-collection` and the `torro_pim::Provider` implementation come with the SQLite cache.

mod client;
pub mod discover;
mod ical;
pub mod nextcloud;
pub mod write;

use std::time::Duration;

pub use client::{CalDavClient, Services};
use torro_pim::ProviderError;

/// Where and as whom to connect. The password lives in the system keychain and reaches the
/// client as a `torro_core::account::Secret` ([`CalDavClient::new`]), not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalDavAccount {
    /// Server or principal URL, e.g. `https://cloud.example.org/remote.php/dav/`.
    pub url: String,
    pub username: String,
}

/// Provider name as `torro_pim::Provider::name` reports it.
pub const PROVIDER_NAME: &str = "caldav";

/// The HTTP client for all CalDAV and sign-in requests. Nextcloud shows the user agent
/// as the device name under Settings → Security.
pub fn http_client() -> Result<reqwest::Client, ProviderError> {
    let device = std::env::var("HOSTNAME").ok().filter(|h| !h.is_empty());
    let agent = match device {
        Some(host) => format!("Torro Workplace ({host})"),
        None => "Torro Workplace".to_string(),
    };
    reqwest::Client::builder()
        .user_agent(agent)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|err| ProviderError::Other(err.to_string()))
}

fn http_error(err: reqwest::Error) -> ProviderError {
    if err.is_connect() || err.is_timeout() { ProviderError::Offline } else { ProviderError::Other(err.to_string()) }
}
