//! Nextcloud's own REST API (OCS) for the core: what a Nextcloud account offers
//! ([`Nextcloud::capabilities`]), its notifications ([`Nextcloud::notifications`],
//! [`Nextcloud::delete_notification`]) and its Talk conversations ([`Nextcloud::rooms`],
//! [`Nextcloud::mark_read`]). Task E5.7 in `docs/tasks/eingang.md`; the core files them into the
//! inbox as the sources `nextcloud:<login>@<host>` (E5.8) and `talk:<login>@<host>` (E5.9).
//!
//! Calendars and tasks are CalDAV and live in `torro-pim-caldav`; this crate speaks only OCS.
//!
//! - `ocs`: the client, the OCS envelope and the errors every call shares ([`OcsError`]).
//! - `capabilities`: which apps the server has.
//! - `notifications`: the notifications app (`apps/notifications`).
//! - `talk`: conversations of Talk (`apps/spreed`).
//! - `rich`: Nextcloud's rich object strings (`{user} shared {file}`) as plain text.
//!
//! Everything in an answer comes from outside. Ids and tokens are checked before they name
//! anything in an address, texts are made plain ([`rich::plain`]), and a link is kept only when it
//! points to the account's own server ([`same_origin`], rule 15 of the E5 plan). The app password
//! travels only in the `Authorization` header and only to the account's origin: a redirect to any
//! other origin is not followed, and neither `Debug` nor an error ever shows the password.

mod capabilities;
mod notifications;
mod ocs;
pub mod rich;
mod talk;

pub use capabilities::{Capabilities, NotificationsCapability, TalkCapability};
pub use notifications::{Listing, Notification, NotificationId, Person};
pub use ocs::{MAX_WAIT, Nextcloud, OcsError, THROTTLE_WAIT};
pub use talk::{LastMessage, NotificationLevel, Room, RoomKind, RoomToken};

/// Whether `a` and `b` are web addresses with the same origin (scheme, host and port). Addresses
/// that do not parse, or have no such origin (`mailto:`, `javascript:`), never match.
pub fn same_origin(a: &str, b: &str) -> bool {
    match (reqwest::Url::parse(a), reqwest::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin().is_tuple() && a.origin() == b.origin(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_origin_compares_scheme_host_and_port() {
        assert!(same_origin("https://cloud.example.org/a", "https://cloud.example.org:443/b?c"));
        assert!(!same_origin("https://cloud.example.org", "http://cloud.example.org"));
        assert!(!same_origin("https://cloud.example.org", "https://cloud.example.org.evil.test"));
        assert!(!same_origin("https://cloud.example.org", "https://cloud.example.org:8443"));
        assert!(!same_origin("javascript:alert(1)", "javascript:alert(1)"));
        assert!(!same_origin("not a url", "not a url"));
    }
}
