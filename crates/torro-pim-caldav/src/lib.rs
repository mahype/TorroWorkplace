//! CalDAV and iCalendar for torro-pim, CardDAV for torro-contacts.
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
//! - `ical`: reading iCalendar; [`parse_events`] also reads the event of an invitation that came
//!   by mail (task E8.3b), [`ObjectFacts`] what a stored object says about the occurrences the
//!   server expanded from it (task TE.3).
//! - `recur`: the rule of a series, `RRULE` to `torro_pim::model::Repeat` and back (task TE.3).
//! - `series`: changing and deleting one occurrence, the following ones or a whole series on the
//!   stored object (task TE.4); `write` sends it ([`CalDavClient::save_in_series`]).
//! - `carddav`: address books and contacts (RFC 6352), changes by `sync-collection` (RFC 6578):
//!   the `torro_contacts::ContactsProvider` of the client (issue #380).
//!
//! For calendars, `sync-collection` and the `torro_pim::Provider` implementation come with the SQLite
//! cache.

mod carddav;
mod client;
pub mod discover;
mod ical;
pub mod nextcloud;
pub mod recur;
pub mod series;
pub mod write;

use std::time::Duration;

pub use client::{CalDavClient, RawObject, Services};
pub use ical::{Exception, ObjectFacts, parse_events};
pub use series::{SeriesError, SeriesFailure};
use torro_pim::ProviderError;
pub use write::SeriesWrite;

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

/// How long a connection to the server may take to open, for every request.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long any request may take in all, answer included. Generous on purpose: a sync of a large
/// calendar or address book (`calendar-query`, `sync-collection`, a PROPFIND over every card) is one
/// long answer even from a healthy server.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// How long one request of a save may take in all (issue #876): the GET of the stored object, the
/// PUT or DELETE, the ETag asked afterwards, a new list's MKCALENDAR. Each touches one small object,
/// so a healthy server answers well within it, Nextcloud included when it mails the invitations of
/// an event during the PUT. A server that holds the connection but never answers costs at most this
/// long per step instead of [`REQUEST_TIMEOUT`]; the timeout counts as offline and the change waits
/// for the next sync.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// The HTTP client for all CalDAV and sign-in requests. Nextcloud shows the user agent
/// as the device name under Settings → Security. Requests of a save narrow its
/// [`REQUEST_TIMEOUT`] to [`WRITE_TIMEOUT`] themselves.
pub fn http_client() -> Result<reqwest::Client, ProviderError> {
    let device = std::env::var("HOSTNAME").ok().filter(|h| !h.is_empty());
    let agent = match device {
        Some(host) => format!("Torro Workplace ({host})"),
        None => "Torro Workplace".to_string(),
    };
    reqwest::Client::builder()
        .user_agent(agent)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|err| ProviderError::Other(err.to_string()))
}

fn http_error(err: reqwest::Error) -> ProviderError {
    if err.is_connect() || err.is_timeout() { ProviderError::Offline } else { ProviderError::Other(err.to_string()) }
}

#[cfg(test)]
pub(crate) mod testing {
    //! A server that hangs, for the tests of the limits (issue #876).

    /// The base URL of a server that accepts every connection and never answers, like one stopped
    /// with `docker pause`: the connection stands, the answer never comes. Connections stay open
    /// while the test runs.
    pub(crate) async fn silent_server() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let address = listener.local_addr().expect("its address");
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        format!("http://{address}/remote.php/dav/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_gives_up_long_before_a_sync() {
        assert!(WRITE_TIMEOUT <= Duration::from_secs(10), "a few seconds for a save");
        assert!(WRITE_TIMEOUT < REQUEST_TIMEOUT);
        assert!(CONNECT_TIMEOUT <= WRITE_TIMEOUT);
    }
}
