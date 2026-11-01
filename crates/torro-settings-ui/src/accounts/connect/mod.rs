//! Building blocks for connecting accounts (task W.5, `docs/einrichtung.md`): the ways in which the
//! add-account dialog (`ui/wizard.rs`) and the first-run setup (`ui/setup`) connect a service, so
//! both use one implementation. Each block keeps what its way has typed and learned, sends the
//! requests and tells how it ended; the dialog and the setup frame it with their own steps and
//! footer.
//!
//! The Nextcloud sign-in ([`nextcloud`], task W.5), GitHub's two ways ([`github`]) and a CalDAV
//! account with a password ([`caldav`], task W.6); the ways for mail, ntfy and AI follow from the
//! dialog when the setup's steps need them (tasks W.7, W.8).

pub mod caldav;
pub mod github;
pub mod nextcloud;
