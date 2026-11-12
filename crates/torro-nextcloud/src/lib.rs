//! Nextcloud's own REST API (OCS) for the core: what a Nextcloud account offers
//! ([`Nextcloud::capabilities`]), its notifications ([`Nextcloud::notifications`],
//! [`Nextcloud::delete_notification`]) and its Talk conversations ([`Nextcloud::rooms`],
//! [`Nextcloud::mark_read`]). Task E5.7 in `docs/tasks/eingang.md`; the core files them into the
//! inbox as the sources `nextcloud:<login>@<host>` (E5.8) and `talk:<login>@<host>` (E5.9).
//! For the chat app (#643, `docs/chat.md`, `docs/praesenz.md`) also the chat of a conversation
//! ([`Nextcloud::messages`], [`Nextcloud::wait_messages`], [`Nextcloud::send_message`] …), the
//! status of people ([`Nextcloud::statuses`], [`Nextcloud::heartbeat`] …), the person behind the
//! account ([`Nextcloud::user`]) and the search for other users ([`Nextcloud::find_sharees`]).
//!
//! Calendars and tasks are CalDAV and live in `torro-pim-caldav`; this crate speaks only OCS.
//!
//! - `ocs`: the client, the OCS envelope and the errors every call shares ([`OcsError`]), and
//!   the error of Talk's chat calls ([`TalkError`]).
//! - `capabilities`: which apps the server has, and Talk's and the status app's settings.
//! - `notifications`: the notifications app (`apps/notifications`).
//! - `talk`: conversations and chat of Talk (`apps/spreed`).
//! - `user_status`: the user status app (`apps/user_status`).
//! - `files`: WebDAV upload (also in pieces), sharing into a conversation, downloads, previews,
//!   link cards and what a conversation shared (#643, stage 4).
//! - `people`: the current user and the search for others (`cloud/user`, sharees, autocomplete).
//! - `rich`: Nextcloud's rich object strings (`{user} shared {file}`) as plain text.
//!
//! Everything in an answer comes from outside. Ids and tokens are checked before they name
//! anything in an address (a user id goes in as one percent-encoded path segment), texts are made
//! plain ([`rich::plain`]), and a link is kept only when it
//! points to the account's own server ([`same_origin`], rule 15 of the E5 plan). The app password
//! travels only in the `Authorization` header and only to the account's origin: a redirect to any
//! other origin is not followed, and neither `Debug` nor an error ever shows the password.

mod capabilities;
mod files;
mod notifications;
mod ocs;
mod people;
pub mod rich;
mod talk;
mod user_status;

pub use capabilities::{
    Capabilities, DEFAULT_ATTACHMENTS_FOLDER, DEFAULT_MAX_LENGTH, NotificationsCapability, TalkCapability,
    UserStatusCapability,
};
pub use files::{
    CHUNK_MAX, CHUNK_MIN, CHUNKS_MAX, DavPath, FilesError, PREVIEW_MAX_BYTES, Picture, Reference, ShareMeta, Uploaded,
    numbered,
};
pub use notifications::{Listing, Notification, NotificationId, Person};
pub use ocs::{MAX_WAIT, Nextcloud, OcsError, THROTTLE_WAIT, TalkError};
pub use people::User;
pub use talk::{
    ActorType, CONTEXT_MAX, CallPeer, ChatMessage, FoundMessage, LastMessage, MESSAGES_MAX, Mention, MentionKind,
    MentionSuggestion, MessageKind, Messages, NotificationLevel, Parent, Room, RoomKind, RoomToken, SendOptions,
    SharedFile, WAIT_MAX_SECS,
};
pub use user_status::{StatusKind, UserStatus};

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
