//! Finding the IMAP and SMTP servers of a mail address without signing in (task M2.2).
//!
//! Order as in TorroMail: table of known providers → MX (Google, Microsoft: not supported, they
//! need OAuth) → `autoconfig.<domain>` → `/.well-known/autoconfig` → ISPDB
//! (`autoconfig.thunderbird.net`) → SPF → base domain of the MX against the table → SRV
//! `_imaps._tcp` / `_submission._tcp` (RFC 6186). The guards of M7.1 against probing private
//! networks hold for every host found on the way, before it is asked or returned
//! (`torro_pim_caldav::discover`). Empty until M2.2.
