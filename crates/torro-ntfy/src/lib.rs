//! The ntfy subscribe protocol (task E5.11, concept `docs/eingang.md`): what the core needs to
//! file the messages of a person's ntfy topics into the inbox as the source `ntfy:<host>`
//! (tasks E5.12, E5.13).
//!
//! - [`server`]: a server address as the person typed it, made uniform ([`Server::parse`], empty is
//!   `https://ntfy.sh`), whether a login may travel to it ([`Server::may_send_login`]), topic names
//!   ([`Topic`]) and the login itself ([`Login`]).
//! - [`subscribe`]: asking the server: [`Ntfy::health`], [`Ntfy::check`] (may these topics be read)
//!   and [`Ntfy::subscribe`], the long-lived JSON stream read line by line ([`Subscription::next`]).
//! - [`message`]: one line of that stream as an [`Event`]; a [`Message`] keeps only what was
//!   checked. Pure functions, tested with the NDJSON fixtures in `tests/fixtures/`.
//!
//! Everything a server sends comes from outside: lines are at most [`MAX_LINE`] bytes, a message
//! keeps id, time, topic, title, text, priority, a few tags and a `click` link (only `http` or
//! `https`); actions, attachments and icons are never read, let alone fetched.
//!
//! Topic names are treated like passwords: on a public server whoever knows one can read it. They
//! appear in no `Debug` output and no error. The login ([`Login`]) goes only over `https` or to
//! this computer, and no request follows a redirect.
//!
//! Errors carry a fixed code for the API ([`NtfyError::code`]). Their text is this crate's own,
//! never the server's, and meant for logs; the core says to people what a code means.

pub mod message;
pub mod server;
pub mod subscribe;

use std::fmt;

pub use message::{Event, Message, MessageId, read_event};
pub use server::{DEFAULT_SERVER, Login, Server, Topic};
pub use subscribe::{MAX_LINE, Ntfy, QUIET, Subscription};

/// Why an address, a topic or a request did not work. No variant holds a topic, a login or the
/// server's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NtfyError {
    /// The address is no ntfy server address: not `http`/`https`, with a path, a query, a login
    /// in it or no host ([`Server::parse`]).
    InvalidServer,
    /// A topic name is not 1–64 of `A–Z a–z 0–9 _ -`, or no topic was given ([`Topic::parse`]).
    InvalidTopic,
    /// A login would travel without TLS to another computer ([`Server::may_send_login`]).
    InsecureLogin,
    /// The server refused (HTTP 401 or 403): the topics need a login, or the login is wrong or
    /// may not read them.
    Auth,
    /// The server asks to wait (HTTP 429).
    Busy,
    /// The server could not be reached, or the stream broke or stayed silent for [`QUIET`].
    Offline,
    /// The server answered, but not as asked (another status, a redirect, an unhealthy server);
    /// the text is this crate's own.
    Server(String),
}

impl NtfyError {
    /// The error code of the API answer (`docs/api.md`): `auth`, `busy`, `offline`, else `error`.
    pub fn code(&self) -> &'static str {
        match self {
            NtfyError::Auth => "auth",
            NtfyError::Busy => "busy",
            NtfyError::Offline => "offline",
            NtfyError::InvalidServer | NtfyError::InvalidTopic | NtfyError::InsecureLogin | NtfyError::Server(_) => {
                "error"
            }
        }
    }
}

impl fmt::Display for NtfyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NtfyError::InvalidServer => f.write_str("The address is not one of an ntfy server."),
            NtfyError::InvalidTopic => f.write_str("A topic name is not valid."),
            NtfyError::InsecureLogin => f.write_str("The login would travel without encryption; use an https address."),
            NtfyError::Auth => f.write_str("The ntfy server refused the topics or the login."),
            NtfyError::Busy => f.write_str("The ntfy server asks to wait."),
            NtfyError::Offline => f.write_str("The ntfy server could not be reached."),
            NtfyError::Server(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for NtfyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_have_their_codes() {
        let codes: Vec<&str> = [
            NtfyError::InvalidServer,
            NtfyError::InvalidTopic,
            NtfyError::InsecureLogin,
            NtfyError::Auth,
            NtfyError::Busy,
            NtfyError::Offline,
            NtfyError::Server("x".into()),
        ]
        .iter()
        .map(NtfyError::code)
        .collect();
        assert_eq!(codes, ["error", "error", "error", "auth", "busy", "offline", "error"]);
    }
}
