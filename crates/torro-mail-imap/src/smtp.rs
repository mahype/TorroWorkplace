//! Sending (task M2.4) with `lettre` on tokio and rustls (465 TLS, 587 STARTTLS), after the send
//! rule of TorroMail: a journal entry before the attempt and after `DATA`; the outcome is
//! accepted, not accepted or unknown, and unknown is never sent again by itself. Empty until M2.4.
