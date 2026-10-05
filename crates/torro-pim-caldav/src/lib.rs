//! CalDAV and iCalendar for torro-pim.
//!
//! Open standards first: Nextcloud, iCloud, Fastmail, Radicale and every other CalDAV
//! server, events (VEVENT) and tasks (VTODO) alike. Scaffold only; discovery
//! (`current-user-principal`, `calendar-home-set`), `sync-collection`, iCalendar parsing
//! and ETag-guarded writes come with the MVP.

/// Where and as whom to connect. The password lives in the system keychain, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalDavAccount {
    /// Server or principal URL, e.g. `https://cloud.example.org/remote.php/dav`.
    pub url: String,
    pub username: String,
}

/// Provider name as `torro_pim::Provider::name` reports it.
pub const PROVIDER_NAME: &str = "caldav";
