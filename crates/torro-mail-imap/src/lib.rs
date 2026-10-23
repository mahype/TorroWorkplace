//! Talking to mail servers for the core: provider discovery, IMAP, folder roles, SMTP and MIME.
//!
//! The types these modules work with live in `torro-mail` (no I/O there); the core
//! (`torroworkplace`) keeps accounts, cache and rights and calls into this crate. Concept:
//! `docs/mail.md`; tasks: `docs/tasks/mail.md`.
//!
//! Task M2.1 sets up the crate with its dependencies and empty modules, so that the tasks filling
//! them run side by side:
//!
//! - [`discover`]: M2.2, finding IMAP and SMTP of an address (table, MX, autoconfig, ISPDB, SRV),
//! - [`imap`] and [`folders`]: M2.3, connecting, listing folders and their roles, fetching, IDLE,
//! - [`smtp`] and [`mime`]: M2.4, sending with the journal rule, reading and building messages,
//! - [`tls`]: M11.2, which certificates IMAP, SMTP and CalDAV trust, and the ones a person confirmed.
//!
//! Rules for every module (security rules of M2.1): a password travels as
//! `torro_core::account::Secret` and is never part of an error, a log or a `Debug` output; the
//! protocol traces of `async-imap` and `lettre` are never switched on; TLS goes through rustls
//! only.

pub mod discover;
pub mod folders;
pub mod imap;
pub mod mime;
pub mod smtp;
pub mod tls;
