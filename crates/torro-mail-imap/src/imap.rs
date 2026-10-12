//! IMAP sessions (task M2.3): connecting with TLS or STARTTLS through rustls, signing in, and the
//! commands the core's sync and handlers need (`UID SEARCH`, `UID FETCH`, `UID STORE`, `UID MOVE`
//! or its fallback, `APPEND`, `IDLE`, `CONDSTORE`). Errors as an enum of their own (`Auth`,
//! `Offline`, `Protocol`, `NotFound`), never with the password. Empty until M2.3.
