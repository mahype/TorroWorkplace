//! Sending mail (task M2.4): SMTP with `lettre` on tokio and rustls, and the send rule of
//! TorroMail around it.
//!
//! **The send rule.** A message that a server may have taken must never go out twice by itself.
//! So every send goes through [`send`]:
//!
//! 1. The [`Journal`] (`<data>/mail/send-journal/`, directory 0700, files 0600) gets an entry
//!    "submitting" for the send's id and is flushed to disk (fsync) **before** the server is
//!    contacted. The entry holds the id and the status, never the message, a recipient or a
//!    subject.
//! 2. The message goes to the server ([`Submit`], for real [`SmtpSubmitter`]).
//! 3. The outcome is written: [`Submission::Accepted`] (the server answered 2xx to the end of
//!    `DATA`), [`Submission::NotAccepted`] (it surely did not take the message: refused before or
//!    with its answer to `DATA`, or never reached) or [`Submission::Unknown`] (the connection broke
//!    after the message was handed over and before the answer). An entry still "submitting" after a
//!    crash reads as unknown.
//! 4. Accepted and unknown are final for that id: [`send`] never contacts the server for it again.
//!    Only "not accepted" may be tried again with the same id.
//! 5. The copy in the Sent folder is a step of its own ([`save_copy`], through [`SentCopy`], which
//!    the core implements with IMAP `APPEND`, task M2.3/M3.3). A failed copy leaves the entry
//!    "copy pending" ([`Journal::pending_copies`]); later only the copy is tried again, never the
//!    mail.
//!
//! **Secrets.** The password travels as [`Secret`] and is only handed to `lettre` for `AUTH`; no
//! error, `Debug` output or journal entry carries it or a server's answer text. `lettre`'s
//! protocol trace (`tracing` feature) is never switched on. There is no plain connection: port 465
//! speaks TLS from the first byte, 587 must offer STARTTLS or nothing is sent (not even the login).
//!
//! **Certificates** (task M11.2). `lettre` takes no certificate check of its own, so a certificate
//! a person confirmed ([`SmtpSubmitter::trusting`]) is checked in two steps: first the connection
//! is secured as always, against the system's trust store; only when that handshake fails, a new
//! connection takes any certificate (its handshake signatures still checked by rustls with the
//! aws-lc-rs provider) and hashes the one the server showed. A pin goes on, anything else ends the
//! connection with [`SmtpError::Untrusted`]. Neither `AUTH` nor `MAIL FROM` is sent before that
//! comparison, with STARTTLS neither.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::future::Future;
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use lettre::Address;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
#[cfg(test)]
use lettre::transport::smtp::client::Certificate;
use lettre::transport::smtp::client::{AsyncSmtpConnection, CertificateStore, TlsParameters};
use lettre::transport::smtp::commands::{Data, Mail, Rcpt};
use lettre::transport::smtp::extension::{ClientId, Extension, MailBodyParameter, MailParameter};
use lettre::transport::smtp::response::Severity;
use serde::{Deserialize, Serialize};
use torro_core::account::Secret;
use torro_mail::{Endpoint, Security};

use crate::tls::Fingerprint;

/// Opening the TCP connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Greeting, TLS, `EHLO` and `AUTH` together.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(60);
/// `MAIL FROM`, every `RCPT TO` and `DATA` together; the message is not handed over yet.
const ENVELOPE_TIMEOUT: Duration = Duration::from_secs(60);
/// Handing over the message and waiting for the answer. RFC 5321 (4.5.3.2.6) asks clients to wait
/// up to ten minutes for it; five keep a broken server from holding the send forever. Running out
/// here makes the outcome unknown.
const DATA_TIMEOUT: Duration = Duration::from_secs(300);
/// `QUIT` after the outcome is known; nothing depends on it.
const QUIT_TIMEOUT: Duration = Duration::from_secs(5);

/// The longest send id the journal takes, in bytes.
const ID_MAX: usize = 100;

/// Where the journal lives below the core's data directory.
const JOURNAL_DIR: [&str; 2] = ["mail", "send-journal"];

/// Who sends a message and who gets it, for SMTP (`MAIL FROM`, `RCPT TO`). This is not the
/// headers: Bcc recipients are here and nowhere in the message. [`crate::mime::build`] makes it
/// together with the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The address the message is sent from (`anna@example.org`), the account's address.
    pub from: String,
    /// Every recipient (To, Cc and Bcc), each address once.
    pub recipients: Vec<String>,
}

/// Why talking to the SMTP server failed. Holds no server text, no address and never the
/// password, so it can go into answers and logs as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpError {
    /// The server refused the login: the password (or user name) is wrong. The core shows
    /// "Password was refused" at the account.
    Auth,
    /// The server could not be reached or the connection broke.
    Offline,
    /// The TLS handshake failed, for example because the certificate is not trusted or names
    /// another host.
    Tls,
    /// The server lacks something sending needs: STARTTLS on a `starttls` endpoint, a login method
    /// (`PLAIN`, `LOGIN`), `SMTPUTF8` for an address with non-ASCII letters or `8BITMIME` for a
    /// message with 8-bit bytes.
    Unsupported,
    /// The server refused the sender, a recipient or the message with this reply code: 4xx is
    /// temporary, 5xx permanent.
    Rejected {
        /// The SMTP reply code, e.g. 550.
        code: u16,
    },
    /// The sender or a recipient is not a usable address, or there is no recipient.
    InvalidEnvelope,
    /// The server answered something that does not fit the protocol.
    Protocol,
    /// The server showed a certificate that is neither trusted by the system nor one of the pins
    /// of [`SmtpSubmitter::trusting`] (task M11.2): this fingerprint, for the person to compare and
    /// confirm. The connection ended before `AUTH`; nothing was sent.
    Untrusted(Fingerprint),
}

impl fmt::Display for SmtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SmtpError::Auth => f.write_str("the mail server refused the login"),
            SmtpError::Offline => f.write_str("the mail server could not be reached"),
            SmtpError::Tls => f.write_str("the secure connection to the mail server failed"),
            SmtpError::Unsupported => f.write_str("the mail server does not support what sending needs"),
            SmtpError::Rejected { code } => write!(f, "the mail server refused the message ({code})"),
            SmtpError::InvalidEnvelope => f.write_str("the sender or a recipient is not a valid address"),
            SmtpError::Protocol => f.write_str("the mail server gave an unexpected answer"),
            SmtpError::Untrusted(fingerprint) => {
                write!(f, "the certificate of the mail server is not trusted (SHA-256 {fingerprint})")
            }
        }
    }
}

impl std::error::Error for SmtpError {}

/// The outcome of one attempt to hand a message to a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submitted {
    /// The server answered 2xx to the end of `DATA`: it took the message. That is not delivery.
    Accepted,
    /// The server surely did not take the message: it refused (before or in its answer to the end
    /// of `DATA`) or was never reached. It may be sent again.
    NotAccepted(SmtpError),
    /// The message was handed over, but no answer came. The server may have taken it, so it must
    /// never be sent again by itself.
    Unknown(SmtpError),
}

/// Hands one message to a mail server. [`SmtpSubmitter`] does it for real; tests (and the core's
/// tests, task M3.3) put in fakes.
pub trait Submit: Sync {
    /// Sends `message` (the whole RFC 5322 message, CRLF line ends, as [`crate::mime::build`] makes
    /// it) to `envelope.recipients` from `envelope.from`.
    ///
    /// Must answer [`Submitted::NotAccepted`] only when the server surely did not take the
    /// message, and [`Submitted::Unknown`] whenever it might have. Must not try again by itself.
    fn submit(&self, envelope: &Envelope, message: &[u8]) -> impl Future<Output = Submitted> + Send;
}

/// Puts the copy of a sent message into the account's Sent folder. The core implements it with
/// IMAP (task M2.3: `APPEND` with `\Seen`; wired up in M3.3).
pub trait SentCopy: Sync {
    /// Why the copy failed, as the implementation knows it (offline, login refused, no Sent
    /// folder). Must not carry the password.
    type Error: Send;

    /// Appends `message` (exactly the bytes that were sent) to the Sent folder with the flag
    /// `\Seen`.
    ///
    /// Called again after a failure, so it should look for the message's `Message-ID` in the
    /// folder first (an `APPEND` whose answer was lost may have worked) and append only when it is
    /// missing: a second copy is better than none, but none is better than two.
    fn append_sent(&self, message: &[u8]) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Sends through a real SMTP server: connects (TLS on `tls`, STARTTLS on `starttls`, never
/// plain), signs in with `PLAIN` or `LOGIN`, sends, says `QUIT`. One connection per message.
///
/// `Debug` shows the server and the login name, the password only as `Secret(…)`.
#[derive(Debug, Clone)]
pub struct SmtpSubmitter {
    target: Target,
    login: String,
    password: Secret,
    /// Certificates a person confirmed ([`SmtpSubmitter::trusting`]).
    pins: Vec<Fingerprint>,
    /// The root certificate (DER) that replaces the system's trust store in this crate's tests,
    /// so they reach no system service and no network.
    #[cfg(test)]
    test_ca: Option<Vec<u8>>,
}

/// How the certificate of a connection is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    /// As always: the system's trust store and the server's host name.
    System,
    /// Any certificate goes through the handshake (its signatures are still checked); the caller
    /// compares the certificate with the pins before anything else is sent.
    Fingerprint,
}

/// Where [`SmtpSubmitter`] connects to.
#[derive(Debug, Clone)]
enum Target {
    /// A mail server as configured: always TLS or STARTTLS.
    Server(Endpoint),
    /// A mail server as configured, reached at `addresses` only ([`SmtpSubmitter::pinned_to`]);
    /// the server's host name is only for TLS.
    Pinned { server: Endpoint, addresses: Vec<SocketAddr> },
    /// The scripted test server on 127.0.0.1 without TLS. Only in this crate's unit tests, so no
    /// build of the suite can send a password in plain text.
    #[cfg(test)]
    PlainLoopback(u16),
}

impl SmtpSubmitter {
    /// A submitter for `server`, signing in as `login` (often the address) with `password`.
    pub fn new(server: Endpoint, login: impl Into<String>, password: Secret) -> SmtpSubmitter {
        SmtpSubmitter {
            target: Target::Server(server),
            login: login.into(),
            password,
            pins: Vec::new(),
            #[cfg(test)]
            test_ca: None,
        }
    }

    /// This submitter also trusting the certificates `pins` (task M11.2): a server whose
    /// certificate the system does not trust is still used when the SHA-256 of its certificate is
    /// one of them, whatever host it names. Any other untrusted certificate ends the connection
    /// with [`SmtpError::Untrusted`] before the login. Replaces earlier pins; an empty list trusts
    /// the system only (an untrusted certificate is still reported as [`SmtpError::Untrusted`]).
    pub fn trusting(self, pins: Vec<Fingerprint>) -> SmtpSubmitter {
        SmtpSubmitter { pins, ..self }
    }

    /// This submitter connecting to `addresses` only, never to what its server's host name
    /// resolves to at the time (task M2.7b): the core checks the host's addresses (public internet
    /// only) and hands over exactly those, so a name that answers differently the second time (DNS
    /// rebinding) never gets the password. The host name stays what TLS checks the certificate
    /// against and sends as SNI; the port of each address is used, not the server's.
    ///
    /// The addresses are tried in order. An empty list makes every connection fail with
    /// [`SmtpError::Offline`].
    pub fn pinned_to(self, addresses: Vec<SocketAddr>) -> SmtpSubmitter {
        let target = match self.target {
            Target::Server(server) | Target::Pinned { server, .. } => Target::Pinned { server, addresses },
            #[cfg(test)]
            other @ Target::PlainLoopback(_) => other,
        };
        SmtpSubmitter { target, ..self }
    }

    /// Connects and signs in without sending anything, then says `QUIT`: for checking a new
    /// account's SMTP login (task M2.7). [`SmtpError::Auth`] means the password was refused.
    pub async fn check_login(&self) -> Result<(), SmtpError> {
        self.check_login_pinned().await.map(drop)
    }

    /// [`SmtpSubmitter::check_login`], answering which pin of [`SmtpSubmitter::trusting`] the
    /// server's certificate matched: `None` when the system trusted it (task M11.2; the core keeps
    /// only pins that were used). [`SmtpError::Untrusted`] when the certificate is neither.
    pub async fn check_login_pinned(&self) -> Result<Option<Fingerprint>, SmtpError> {
        let (mut connection, pinned) = self.open().await?;
        quit(&mut connection).await;
        Ok(pinned)
    }

    /// Connects, secures the connection and signs in; with the pin the certificate matched.
    async fn open(&self) -> Result<(AsyncSmtpConnection, Option<Fingerprint>), SmtpError> {
        let attempt = async {
            let (mut connection, pinned) = self.connect().await?;
            let credentials = Credentials::new(self.login.clone(), self.password.expose().to_owned());
            if let Err(error) = connection.auth(&[Mechanism::Plain, Mechanism::Login], &credentials).await {
                connection.abort().await;
                return Err(login_error(&error));
            }
            Ok((connection, pinned))
        };
        tokio::time::timeout(LOGIN_TIMEOUT, attempt).await.unwrap_or(Err(SmtpError::Offline))
    }

    /// The connection up to the point where the login may be sent: encrypted with a certificate
    /// that is trusted or pinned, `EHLO` answered.
    async fn connect(&self) -> Result<(AsyncSmtpConnection, Option<Fingerprint>), SmtpError> {
        // `[127.0.0.1]` instead of the computer's name, as Thunderbird does: the name would tell
        // every mail server (and every reader of the `Received:` headers) what the computer is
        // called.
        let hello = ClientId::Ipv4(Ipv4Addr::LOCALHOST);
        match &self.target {
            Target::Server(server) => self.trusted(server, (server.host.as_str(), server.port), &hello).await,
            Target::Pinned { addresses, .. } if addresses.is_empty() => Err(SmtpError::Offline),
            Target::Pinned { server, addresses } => self.trusted(server, addresses.as_slice(), &hello).await,
            #[cfg(test)]
            Target::PlainLoopback(port) => AsyncSmtpConnection::connect_tokio1(
                (Ipv4Addr::LOCALHOST, *port),
                Some(CONNECT_TIMEOUT),
                &hello,
                None,
                None,
            )
            .await
            .map(|connection| (connection, None))
            .map_err(|error| connect_error(&error)),
        }
    }

    /// A secured connection to `server` at `address` whose certificate the system trusts, or, when
    /// that handshake fails, a new one whose certificate is a pin (see the module documentation).
    /// The pin is answered; a certificate that is not one ends the connection with
    /// [`SmtpError::Untrusted`] before anything but `EHLO` (and `STARTTLS`) was sent.
    async fn trusted<A>(
        &self,
        server: &Endpoint,
        address: A,
        hello: &ClientId,
    ) -> Result<(AsyncSmtpConnection, Option<Fingerprint>), SmtpError>
    where
        A: tokio::net::ToSocketAddrs + Copy,
    {
        match self.secured(server, address, hello, Check::System).await {
            Err(SmtpError::Tls) => {}
            other => return other.map(|connection| (connection, None)),
        }
        let mut connection = self.secured(server, address, hello, Check::Fingerprint).await?;
        let shown = match connection.peer_certificate() {
            Ok(der) => Fingerprint::of(&der),
            Err(_) => {
                connection.abort().await;
                return Err(SmtpError::Tls);
            }
        };
        if self.pins.contains(&shown) {
            return Ok((connection, Some(shown)));
        }
        connection.abort().await;
        Err(SmtpError::Untrusted(shown))
    }

    /// Connects to `address` and secures the connection to `server` as its security asks: TLS
    /// from the first byte, or STARTTLS, which a server must offer or nothing is sent. With
    /// [`Check::System`] the certificate is checked against the server's host name, wherever
    /// `address` points.
    async fn secured<A>(
        &self,
        server: &Endpoint,
        address: A,
        hello: &ClientId,
        check: Check,
    ) -> Result<AsyncSmtpConnection, SmtpError>
    where
        A: tokio::net::ToSocketAddrs,
    {
        match server.security {
            Security::Tls => {
                let tls = self.tls_parameters(&server.host, check)?;
                AsyncSmtpConnection::connect_tokio1(address, Some(CONNECT_TIMEOUT), hello, Some(tls), None)
                    .await
                    .map_err(|error| connect_error(&error))
            }
            Security::StartTls => {
                let mut connection =
                    AsyncSmtpConnection::connect_tokio1(address, Some(CONNECT_TIMEOUT), hello, None, None)
                        .await
                        .map_err(|error| connect_error(&error))?;
                // Without STARTTLS the login would go out in plain text: refuse.
                if !connection.can_starttls() {
                    connection.abort().await;
                    return Err(SmtpError::Unsupported);
                }
                let tls = self.tls_parameters(&server.host, check)?;
                connection.starttls(tls, hello).await.map_err(|error| connect_error(&error))?;
                Ok(connection)
            }
        }
    }

    /// The TLS settings for a connection to `host`. [`Check::Fingerprint`] takes any certificate
    /// for any name; `lettre` still checks the handshake's signatures with rustls' provider
    /// (aws-lc-rs, the one this workspace builds).
    fn tls_parameters(&self, host: &str, check: Check) -> Result<TlsParameters, SmtpError> {
        let builder = TlsParameters::builder(host.to_owned());
        let builder = match check {
            Check::System => self.system_roots(builder)?,
            Check::Fingerprint => builder
                .certificate_store(CertificateStore::None)
                .dangerous_accept_invalid_certs(true)
                .dangerous_accept_invalid_hostnames(true),
        };
        builder.build().map_err(|_| SmtpError::Tls)
    }

    /// The system's trust store (what `TlsParameters::new` uses).
    #[cfg(not(test))]
    fn system_roots(
        &self,
        builder: lettre::transport::smtp::client::TlsParametersBuilder,
    ) -> Result<lettre::transport::smtp::client::TlsParametersBuilder, SmtpError> {
        Ok(builder)
    }

    /// In tests, the test CA instead of the system's trust store, when one is set.
    #[cfg(test)]
    fn system_roots(
        &self,
        builder: lettre::transport::smtp::client::TlsParametersBuilder,
    ) -> Result<lettre::transport::smtp::client::TlsParametersBuilder, SmtpError> {
        match &self.test_ca {
            Some(ca) => {
                let ca = Certificate::from_der(ca.clone()).map_err(|_| SmtpError::Tls)?;
                Ok(builder.certificate_store(CertificateStore::None).add_root_certificate(ca))
            }
            None => Ok(builder),
        }
    }
}

impl Submit for SmtpSubmitter {
    async fn submit(&self, envelope: &Envelope, message: &[u8]) -> Submitted {
        let (from, recipients) = match envelope_addresses(envelope) {
            Ok(addresses) => addresses,
            Err(error) => return Submitted::NotAccepted(error),
        };
        let mut connection = match self.open().await {
            Ok((connection, _)) => connection,
            Err(error) => return Submitted::NotAccepted(error),
        };
        let outcome = transfer(&mut connection, from, recipients, message).await;
        match outcome {
            Submitted::Accepted => quit(&mut connection).await,
            Submitted::NotAccepted(_) | Submitted::Unknown(_) => connection.abort().await,
        }
        outcome
    }
}

/// `MAIL FROM`, `RCPT TO`, `DATA` and the message on a signed-in connection, classified by
/// whether the server may have taken it.
async fn transfer(
    connection: &mut AsyncSmtpConnection,
    from: Address,
    recipients: Vec<Address>,
    message: &[u8],
) -> Submitted {
    let mut parameters = Vec::new();
    let non_ascii_addresses = std::iter::once(&from).chain(&recipients).any(|address| !address.to_string().is_ascii());
    if non_ascii_addresses {
        if !connection.server_info().supports_feature(Extension::SmtpUtfEight) {
            return Submitted::NotAccepted(SmtpError::Unsupported);
        }
        parameters.push(MailParameter::SmtpUtfEight);
    }
    if !message.is_ascii() {
        if !connection.server_info().supports_feature(Extension::EightBitMime) {
            return Submitted::NotAccepted(SmtpError::Unsupported);
        }
        parameters.push(MailParameter::Body(MailBodyParameter::EightBitMime));
    }

    // Everything up to the server's 354 to `DATA`: the message has not left, so any failure here
    // is "not accepted".
    let envelope = async {
        connection.command(Mail::new(Some(from), parameters)).await?;
        for recipient in recipients {
            connection.command(Rcpt::new(recipient, Vec::new())).await?;
        }
        connection.command(Data).await
    };
    match tokio::time::timeout(ENVELOPE_TIMEOUT, envelope).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => return Submitted::NotAccepted(command_error(&error)),
        Err(_) => return Submitted::NotAccepted(SmtpError::Offline),
    }

    // `lettre` ends the data with CRLF . CRLF itself; a message that already ends in CRLF would
    // get an empty line more.
    let body = message.strip_suffix(b"\r\n").unwrap_or(message);
    match tokio::time::timeout(DATA_TIMEOUT, connection.message(body)).await {
        Ok(Ok(response)) if response.code().severity == Severity::PositiveCompletion => Submitted::Accepted,
        // A 3xx after the data is no acceptance and no refusal.
        Ok(Ok(_)) => Submitted::Unknown(SmtpError::Protocol),
        // A 4xx or 5xx reply to the data is a clear refusal.
        Ok(Err(error)) if error.is_transient() || error.is_permanent() => Submitted::NotAccepted(command_error(&error)),
        // The message (maybe all of it) left, the answer did not come: the server may have it.
        Ok(Err(error)) if error.is_response() => Submitted::Unknown(SmtpError::Protocol),
        Ok(Err(_)) | Err(_) => Submitted::Unknown(SmtpError::Offline),
    }
}

/// Ends the session politely; a failure here changes nothing about the message.
async fn quit(connection: &mut AsyncSmtpConnection) {
    let _ = tokio::time::timeout(QUIT_TIMEOUT, connection.quit()).await;
}

/// The envelope as `lettre` addresses; refuses empty recipients and anything that is not one
/// address (no line breaks or angle brackets can sneak into `MAIL FROM`/`RCPT TO`).
fn envelope_addresses(envelope: &Envelope) -> Result<(Address, Vec<Address>), SmtpError> {
    if envelope.recipients.is_empty() {
        return Err(SmtpError::InvalidEnvelope);
    }
    let parse = |address: &str| address.trim().parse::<Address>().map_err(|_| SmtpError::InvalidEnvelope);
    let from = parse(&envelope.from)?;
    let recipients = envelope.recipients.iter().map(|recipient| parse(recipient)).collect::<Result<Vec<_>, _>>()?;
    Ok((from, recipients))
}

/// The SMTP reply code of an error, if the server sent one.
fn reply_code(error: &lettre::transport::smtp::Error) -> Option<u16> {
    error.status().map(u16::from)
}

/// Errors while connecting, greeting and securing the connection.
fn connect_error(error: &lettre::transport::smtp::Error) -> SmtpError {
    if error.is_tls() || failed_handshake(error) {
        SmtpError::Tls
    } else if let Some(code) = reply_code(error) {
        SmtpError::Rejected { code }
    } else if error.is_client() {
        SmtpError::Unsupported
    } else if error.is_response() {
        SmtpError::Protocol
    } else {
        SmtpError::Offline
    }
}

/// `lettre` reports a failed TLS handshake as a connection error (like an unreachable host);
/// rustls' error inside tells it apart, so a refused certificate is [`SmtpError::Tls`] and gets
/// its second look ([`SmtpSubmitter::trusting`]) while a host that is down does not.
fn failed_handshake(error: &lettre::transport::smtp::Error) -> bool {
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        let inner = cause.downcast_ref::<io::Error>().and_then(io::Error::get_ref);
        if cause.is::<rustls::Error>() || inner.is_some_and(|inner| inner.is::<rustls::Error>()) {
            return true;
        }
        source = cause.source();
    }
    false
}

/// Errors of `AUTH`: a permanent refusal (535 and friends) means the login was wrong; a
/// temporary one (454) is the server's trouble.
fn login_error(error: &lettre::transport::smtp::Error) -> SmtpError {
    if error.is_permanent() {
        SmtpError::Auth
    } else if let Some(code) = reply_code(error) {
        SmtpError::Rejected { code }
    } else if error.is_client() {
        // No login method both sides know.
        SmtpError::Unsupported
    } else if error.is_response() {
        SmtpError::Protocol
    } else {
        SmtpError::Offline
    }
}

/// Errors of `MAIL`, `RCPT`, `DATA` and the reply to the data.
fn command_error(error: &lettre::transport::smtp::Error) -> SmtpError {
    if let Some(code) = reply_code(error) {
        SmtpError::Rejected { code }
    } else if error.is_response() {
        SmtpError::Protocol
    } else if error.is_client() {
        SmtpError::InvalidEnvelope
    } else {
        SmtpError::Offline
    }
}

/// Where a send stands, as the journal keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Submission {
    /// An attempt is running in this process right now. Read after a crash, it is reported as
    /// [`Submission::Unknown`].
    Submitting,
    /// The server took the message. Never sent again.
    Accepted,
    /// The server surely did not take it; [`send`] may try again with the same id.
    NotAccepted,
    /// The server may have taken it. Never sent again by itself: the person decides (a new send
    /// with a new id, after [`Journal::forget`] or not).
    Unknown,
}

/// Where the copy in the Sent folder stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyStatus {
    /// No copy is made: the message was not accepted, or the provider files sent mail itself
    /// ([`CopyPlan::ProviderKeepsIt`]).
    NotNeeded,
    /// Accepted, the copy is still missing; [`save_copy`] tries it (again).
    Pending,
    /// The copy is in the Sent folder.
    Saved,
}

/// Whether the core should put a copy into the Sent folder after the server accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyPlan {
    /// Yes: the entry becomes [`CopyStatus::Pending`] and the core calls [`save_copy`].
    Append,
    /// No: the provider files sent mail itself (Gmail, Outlook.com), a copy would be a second one.
    ProviderKeepsIt,
}

/// One entry of the journal: the send's id and its status, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// The send's id as the core gave it.
    pub id: String,
    pub submission: Submission,
    pub copy: CopyStatus,
}

/// Why the journal could not be used. Says nothing about a message; carries no path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    /// The id is empty or longer than 100 bytes.
    InvalidId,
    /// There is no entry for the id.
    NotFound,
    /// A send or copy with this id is running right now in this process.
    Busy,
    /// The journal directory is a symbolic link; it is not used.
    Unsafe,
    /// Reading or writing failed; with a failed write, nothing was sent.
    Io(io::ErrorKind),
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JournalError::InvalidId => f.write_str("the send id is empty or too long"),
            JournalError::NotFound => f.write_str("the send journal has no entry for this id"),
            JournalError::Busy => f.write_str("this message is being sent right now"),
            JournalError::Unsafe => f.write_str("the send journal's directory is a symbolic link"),
            JournalError::Io(kind) => write!(f, "the send journal could not be written ({kind})"),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<io::Error> for JournalError {
    fn from(error: io::Error) -> JournalError {
        JournalError::Io(error.kind())
    }
}

/// The send journal: one small JSON file per send under `<data>/mail/send-journal/`.
///
/// File names are the id in hex (any id works on every platform), contents only the id and the
/// two statuses. Every write goes to a temporary file (0600), is flushed (fsync), renamed over the
/// entry and the directory is flushed, so a crash leaves the old or the new entry, never half of
/// one. The core keeps one `Journal` per data directory and shares it (it is `Sync`): sends with
/// the same id at the same time are refused ([`JournalError::Busy`]).
#[derive(Debug)]
pub struct Journal {
    dir: PathBuf,
    /// Ids with a send or copy running in this process. Lets a "submitting" entry of a running
    /// send be told apart from one a crash left behind.
    running: Mutex<HashSet<String>>,
}

impl Journal {
    /// Opens (and creates) the journal under the core's data directory `data`
    /// (`torro_core::paths::AppPaths::data`): `<data>/mail/send-journal/`, made 0700 on Unix like
    /// `<data>/mail/` when it is created here. A directory that is a symbolic link is refused.
    pub fn open(data: &Path) -> Result<Journal, JournalError> {
        let dir = JOURNAL_DIR.iter().fold(data.to_path_buf(), |path, part| path.join(part));
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&dir)?;
        if fs::symlink_metadata(&dir)?.file_type().is_symlink() {
            return Err(JournalError::Unsafe);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Journal { dir, running: Mutex::new(HashSet::new()) })
    }

    /// The journal's directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The entry of `id`, `None` when there is none. A "submitting" entry that no send of this
    /// process is running (a crash) and an entry that cannot be read are reported as
    /// [`Submission::Unknown`]: when in doubt, the message may be out.
    pub fn entry(&self, id: &str) -> Result<Option<JournalEntry>, JournalError> {
        let mut entry = self.read(id)?;
        if let Some(entry) = entry.as_mut().filter(|entry| entry.submission == Submission::Submitting)
            && !self.is_running(id)
        {
            entry.submission = Submission::Unknown;
        }
        Ok(entry)
    }

    /// Every entry, in no particular order, read as [`Journal::entry`] reads them. For the core's
    /// list of sends that need the person (unknown) and for cleaning up.
    pub fn entries(&self) -> Result<Vec<JournalEntry>, JournalError> {
        let mut entries = Vec::new();
        for item in fs::read_dir(&self.dir)? {
            let path = item?.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()).and_then(id_from_file) else {
                continue;
            };
            if let Some(entry) = self.entry(&id)? {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// The ids of accepted sends whose Sent copy is still missing: the core calls [`save_copy`]
    /// for each (at start and every minute, task M3.3).
    pub fn pending_copies(&self) -> Result<Vec<String>, JournalError> {
        Ok(self
            .entries()?
            .into_iter()
            .filter(|entry| entry.submission == Submission::Accepted && entry.copy == CopyStatus::Pending)
            .map(|entry| entry.id)
            .collect())
    }

    /// Removes the entry of `id` (nothing happens when there is none). The core does it when the
    /// send is done with (accepted and the copy saved or not needed), or when the person has
    /// decided about an unknown send. After that, [`send`] with the same id would send again, so
    /// never forget an accepted send whose draft could be sent once more. Refused while a send or
    /// copy of `id` runs.
    pub fn forget(&self, id: &str) -> Result<(), JournalError> {
        let path = self.path(id)?;
        let _running = self.start(id)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// The entry of `id` as it is on disk; one that cannot be read counts as unknown.
    fn read(&self, id: &str) -> Result<Option<JournalEntry>, JournalError> {
        let bytes = match fs::read(self.path(id)?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let entry = serde_json::from_slice::<JournalEntry>(&bytes).ok().filter(|entry| entry.id == id);
        Ok(Some(entry.unwrap_or_else(|| JournalEntry {
            id: id.to_owned(),
            submission: Submission::Unknown,
            copy: CopyStatus::NotNeeded,
        })))
    }

    fn path(&self, id: &str) -> Result<PathBuf, JournalError> {
        if id.is_empty() || id.len() > ID_MAX {
            return Err(JournalError::InvalidId);
        }
        Ok(self.dir.join(format!("{}.json", id_to_file(id))))
    }

    /// Writes `entry` so that it survives a crash: temporary file 0600, fsync, rename, fsync of
    /// the directory.
    fn write(&self, entry: &JournalEntry) -> Result<(), JournalError> {
        let path = self.path(&entry.id)?;
        let temporary = path.with_extension("tmp");
        let bytes = serde_json::to_vec(entry).map_err(|_| JournalError::Io(io::ErrorKind::InvalidData))?;
        let mut options = OpenOptions::new();
        options.create(true).write(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &path)?;
        #[cfg(unix)]
        fs::File::open(&self.dir)?.sync_all()?;
        Ok(())
    }

    fn is_running(&self, id: &str) -> bool {
        self.running.lock().unwrap_or_else(PoisonError::into_inner).contains(id)
    }

    /// Marks `id` as running until the guard is dropped (also when a send is cancelled).
    fn start(&self, id: &str) -> Result<Running<'_>, JournalError> {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if !running.insert(id.to_owned()) {
            return Err(JournalError::Busy);
        }
        Ok(Running { journal: self, id: id.to_owned() })
    }
}

/// An id marked as running in a [`Journal`].
struct Running<'a> {
    journal: &'a Journal,
    id: String,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.journal.running.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.id);
    }
}

fn id_to_file(id: &str) -> String {
    id.bytes().map(|byte| format!("{byte:02x}")).collect()
}

fn id_from_file(name: &str) -> Option<String> {
    if !name.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..name.len())
        .step_by(2)
        .map(|start| name.get(start..start + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok()))
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

/// What [`send`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendReport {
    /// Accepted, not accepted or unknown; never "submitting".
    pub submission: Submission,
    /// The Sent copy: [`CopyStatus::Pending`] after an acceptance with [`CopyPlan::Append`] (call
    /// [`save_copy`]), else [`CopyStatus::NotNeeded`], or what an earlier send left.
    pub copy: CopyStatus,
    /// Why this attempt was not accepted or is unknown; `None` when accepted or when nothing was
    /// attempted.
    pub error: Option<SmtpError>,
    /// `false`: the journal already held a final outcome (accepted or unknown) for the id, so the
    /// server was not contacted; `submission` is that outcome.
    pub attempted: bool,
    /// The outcome could not be written to the journal. The entry still says "submitting" and
    /// reads as unknown later, so nothing is sent twice; the core should warn that the journal
    /// needs attention.
    pub journal_failed: bool,
}

/// Sends `message` under the send rule (see the module documentation).
///
/// `id` names this send in the journal (≤ 100 bytes, opaque; the core picks it, e.g. per draft).
/// `envelope` and `message` come from [`crate::mime::build`]; the same bytes later go to
/// [`save_copy`].
///
/// - The journal already says accepted or unknown (or "submitting" from a crash): nothing is
///   sent, the report has `attempted: false` and that outcome.
/// - Else "submitting" is written and flushed first; if that fails the answer is the
///   [`JournalError`] and nothing was sent. Then `smtp` is asked once and the outcome written.
///
/// [`JournalError::Busy`] when a send or copy of the same id runs right now.
pub async fn send(
    journal: &Journal,
    id: &str,
    envelope: &Envelope,
    message: &[u8],
    smtp: &impl Submit,
    copy: CopyPlan,
) -> Result<SendReport, JournalError> {
    let _running = journal.start(id)?;
    // Read as on disk: this send counts as running now, so `entry` would show a crash's
    // "submitting" as running.
    if let Some(earlier) = journal.read(id)? {
        let submission = match earlier.submission {
            Submission::NotAccepted => None,
            Submission::Accepted => Some(Submission::Accepted),
            Submission::Unknown => Some(Submission::Unknown),
            Submission::Submitting => {
                // A crash during an attempt: make it "unknown" on disk too. Failing to write that
                // changes nothing, it reads as unknown either way.
                let _ = journal.write(&JournalEntry { submission: Submission::Unknown, ..earlier.clone() });
                Some(Submission::Unknown)
            }
        };
        if let Some(submission) = submission {
            return Ok(SendReport {
                submission,
                copy: earlier.copy,
                error: None,
                attempted: false,
                journal_failed: false,
            });
        }
    }

    let mut entry = JournalEntry { id: id.to_owned(), submission: Submission::Submitting, copy: CopyStatus::NotNeeded };
    journal.write(&entry)?;

    let (submission, error) = match smtp.submit(envelope, message).await {
        Submitted::Accepted => (Submission::Accepted, None),
        Submitted::NotAccepted(error) => (Submission::NotAccepted, Some(error)),
        Submitted::Unknown(error) => (Submission::Unknown, Some(error)),
    };
    entry.submission = submission;
    entry.copy = match (submission, copy) {
        (Submission::Accepted, CopyPlan::Append) => CopyStatus::Pending,
        _ => CopyStatus::NotNeeded,
    };
    let journal_failed = journal.write(&entry).is_err();
    Ok(SendReport { submission, copy: entry.copy, error, attempted: true, journal_failed })
}

/// What [`save_copy`] did.
#[derive(Debug)]
pub struct CopyReport<E> {
    /// [`CopyStatus::Saved`] when the copy is in the Sent folder (now or before),
    /// [`CopyStatus::Pending`] when it failed and is tried again later, [`CopyStatus::NotNeeded`]
    /// when no copy belongs to this send.
    pub status: CopyStatus,
    /// Why the copy failed this time.
    pub error: Option<E>,
    /// The new status could not be written; the entry stays "pending" and the next try asks
    /// `folder` again (which looks for the `Message-ID` first, see [`SentCopy`]).
    pub journal_failed: bool,
}

/// Puts the Sent copy of the accepted send `id` into the Sent folder through `folder`: right after
/// [`send`] and again for every id of [`Journal::pending_copies`]. Never sends the mail.
///
/// `message` must be the bytes that were sent (the core keeps them until the copy is saved; the
/// journal does not). Does nothing but answer the status when the entry is not "accepted, copy
/// pending". [`JournalError::NotFound`] without an entry, [`JournalError::Busy`] while a send or
/// copy of `id` runs.
pub async fn save_copy<C: SentCopy>(
    journal: &Journal,
    id: &str,
    message: &[u8],
    folder: &C,
) -> Result<CopyReport<C::Error>, JournalError> {
    let _running = journal.start(id)?;
    let Some(mut entry) = journal.read(id)? else {
        return Err(JournalError::NotFound);
    };
    if entry.submission != Submission::Accepted || entry.copy != CopyStatus::Pending {
        return Ok(CopyReport { status: entry.copy, error: None, journal_failed: false });
    }
    match folder.append_sent(message).await {
        Ok(()) => {
            entry.copy = CopyStatus::Saved;
            let journal_failed = journal.write(&entry).is_err();
            Ok(CopyReport { status: CopyStatus::Saved, error: None, journal_failed })
        }
        Err(error) => Ok(CopyReport { status: CopyStatus::Pending, error: Some(error), journal_failed: false }),
    }
}

#[cfg(test)]
#[path = "../tests/support/smtp_server.rs"]
mod smtp_server;

#[cfg(test)]
mod tests {
    use super::smtp_server::{Mode, Server, Shows, test_ca};
    use super::*;

    const PASSWORD: &str = "correct horse battery staple";

    /// `openssl x509 -fingerprint -sha256` of `tests/fixtures/imap-tls/self-signed.der`.
    const SELF_SIGNED: &str =
        "E4:A8:B0:8A:89:6F:2B:06:7C:E1:92:8D:34:0D:16:14:5A:56:57:E7:9E:7E:4A:62:E1:30:3D:36:3C:21:76:28";

    fn submitter(server: &Server) -> SmtpSubmitter {
        SmtpSubmitter {
            target: Target::PlainLoopback(server.port),
            login: "anna".into(),
            password: Secret::new(PASSWORD),
            pins: Vec::new(),
            test_ca: None,
        }
    }

    /// A submitter for the TLS test server at `127.0.0.1` that trusts the test CA (in place of the
    /// system's store) and `pins`.
    fn tls_submitter(server: &Server, security: Security, pins: Vec<Fingerprint>) -> SmtpSubmitter {
        let endpoint = Endpoint { host: "127.0.0.1".into(), port: server.port, security };
        let mut submitter = SmtpSubmitter::new(endpoint, "anna", Secret::new(PASSWORD)).trusting(pins);
        submitter.test_ca = Some(test_ca());
        submitter
    }

    fn self_signed() -> Fingerprint {
        SELF_SIGNED.parse().expect("fingerprint")
    }

    fn envelope() -> Envelope {
        Envelope {
            from: "anna@example.org".into(),
            recipients: vec!["ben@example.org".into(), "carla@example.org".into()],
        }
    }

    const MESSAGE: &[u8] = b"Subject: Hi\r\nMessage-ID: <1@example.org>\r\n\r\nHello.\r\n.hidden line\r\nBye.\r\n";

    #[tokio::test]
    async fn an_accepted_message_arrives_whole_and_is_journalled_as_accepted() {
        let server = Server::start(Mode::Accept).await;
        let data = tempfile::tempdir().expect("temp dir");
        let journal = Journal::open(data.path()).expect("journal");

        let report =
            send(&journal, "s1", &envelope(), MESSAGE, &submitter(&server), CopyPlan::Append).await.expect("sent");

        assert_eq!(report.submission, Submission::Accepted);
        assert_eq!(
            (report.copy, report.error, report.attempted, report.journal_failed),
            (CopyStatus::Pending, None, true, false)
        );
        let seen = server.received();
        assert_eq!(seen.mail_from, ["MAIL FROM:<anna@example.org>"]);
        assert_eq!(seen.rcpt_to, ["RCPT TO:<ben@example.org>", "RCPT TO:<carla@example.org>"]);
        assert_eq!(seen.data, MESSAGE, "dot-stuffing undone, no line added");
        assert!(seen.quit, "the session ends with QUIT");
        assert_eq!(journal.pending_copies().expect("pending"), ["s1"]);
    }

    #[tokio::test]
    async fn a_refused_recipient_or_message_is_not_accepted_and_may_be_sent_again() {
        for (mode, code) in [(Mode::RejectRecipient, 550), (Mode::RejectData, 554)] {
            let server = Server::start(mode).await;
            let data = tempfile::tempdir().expect("temp dir");
            let journal = Journal::open(data.path()).expect("journal");

            let report =
                send(&journal, "s1", &envelope(), MESSAGE, &submitter(&server), CopyPlan::Append).await.expect("tried");
            assert_eq!(report.submission, Submission::NotAccepted, "{mode:?}");
            assert_eq!((report.error, report.copy), (Some(SmtpError::Rejected { code }), CopyStatus::NotNeeded));
            assert_eq!(journal.entry("s1").expect("read").map(|entry| entry.submission), Some(Submission::NotAccepted));

            send(&journal, "s1", &envelope(), MESSAGE, &submitter(&server), CopyPlan::Append).await.expect("again");
            assert_eq!(server.connections(), 2, "{mode:?}: not accepted may be tried again");
        }
    }

    #[tokio::test]
    async fn a_connection_lost_after_data_is_unknown_and_never_sent_again() {
        let server = Server::start(Mode::DropAfterData).await;
        let data = tempfile::tempdir().expect("temp dir");
        let journal = Journal::open(data.path()).expect("journal");

        let report =
            send(&journal, "s1", &envelope(), MESSAGE, &submitter(&server), CopyPlan::Append).await.expect("tried");
        assert_eq!(report.submission, Submission::Unknown);
        assert_eq!((report.copy, report.attempted), (CopyStatus::NotNeeded, true));
        assert_eq!(server.received().data, MESSAGE, "the server got the whole message");

        let again =
            send(&journal, "s1", &envelope(), MESSAGE, &submitter(&server), CopyPlan::Append).await.expect("asked");
        assert_eq!((again.submission, again.attempted, again.error), (Submission::Unknown, false, None));
        assert_eq!(server.connections(), 1, "no second attempt");
        assert!(journal.pending_copies().expect("pending").is_empty(), "no copy of an unknown send");
    }

    #[tokio::test]
    async fn a_refused_login_names_auth_and_the_password_appears_nowhere() {
        let server = Server::start(Mode::RejectLogin).await;
        let data = tempfile::tempdir().expect("temp dir");
        let journal = Journal::open(data.path()).expect("journal");
        let submitter = submitter(&server);

        assert_eq!(submitter.check_login().await, Err(SmtpError::Auth));
        let report = send(&journal, "s1", &envelope(), MESSAGE, &submitter, CopyPlan::Append).await.expect("tried");
        assert_eq!((report.submission, report.error), (Submission::NotAccepted, Some(SmtpError::Auth)));
        assert!(server.received().auth_sent, "the login was really tried");

        let file = fs::read_to_string(journal.dir().join(format!("{}.json", id_to_file("s1")))).expect("entry");
        for text in [
            format!("{report:?}"),
            format!("{:?}", report.error),
            SmtpError::Auth.to_string(),
            format!("{submitter:?}"),
            format!("{journal:?}"),
            file,
        ] {
            assert!(!text.contains(PASSWORD) && !text.contains("horse"), "password leaked: {text}");
        }
    }

    #[tokio::test]
    async fn a_starttls_server_without_starttls_never_sees_the_login() {
        let server = Server::start(Mode::Accept).await;
        let endpoint = Endpoint { host: "127.0.0.1".into(), port: server.port, security: Security::StartTls };
        let submitter = SmtpSubmitter::new(endpoint, "anna", Secret::new(PASSWORD));

        assert_eq!(submitter.check_login().await, Err(SmtpError::Unsupported));
        assert_eq!(submitter.submit(&envelope(), MESSAGE).await, Submitted::NotAccepted(SmtpError::Unsupported));
        let seen = server.received();
        assert!(!seen.auth_sent && seen.mail_from.is_empty(), "nothing but EHLO in plain text");
    }

    #[tokio::test]
    async fn an_unusable_envelope_is_refused_before_connecting() {
        let server = Server::start(Mode::Accept).await;
        for envelope in [
            Envelope { from: "anna@example.org".into(), recipients: vec![] },
            Envelope {
                from: "anna@example.org>\r\nRCPT TO:<eve@example.org".into(),
                recipients: vec!["b@example.org".into()],
            },
            Envelope { from: "anna@example.org".into(), recipients: vec!["not an address".into()] },
        ] {
            assert_eq!(
                submitter(&server).submit(&envelope, MESSAGE).await,
                Submitted::NotAccepted(SmtpError::InvalidEnvelope)
            );
        }
        assert_eq!(server.connections(), 0);
    }

    #[tokio::test]
    async fn an_unreachable_server_is_offline_and_not_accepted() {
        let port = {
            let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
            listener.local_addr().expect("address").port()
        };
        let submitter = SmtpSubmitter {
            target: Target::PlainLoopback(port),
            login: "anna".into(),
            password: Secret::new(PASSWORD),
            pins: Vec::new(),
            test_ca: None,
        };
        assert_eq!(submitter.submit(&envelope(), MESSAGE).await, Submitted::NotAccepted(SmtpError::Offline));
    }

    /// Task M2.7b: a pinned submitter goes to the address it was given, never to what the host
    /// name (here a closed port on `localhost`) would resolve to; STARTTLS is still required.
    #[tokio::test]
    async fn a_pinned_submitter_connects_to_the_checked_address_only() {
        let server = Server::start(Mode::Accept).await;
        let configured = Endpoint { host: "localhost".into(), port: 9, security: Security::StartTls };
        let pinned = SmtpSubmitter::new(configured.clone(), "anna", Secret::new(PASSWORD))
            .pinned_to(vec![SocketAddr::from((Ipv4Addr::LOCALHOST, server.port))]);
        assert_eq!(pinned.check_login().await, Err(SmtpError::Unsupported), "no STARTTLS, nothing sent");
        assert_eq!(server.connections(), 1, "the checked address was reached");
        assert!(server.received().mail_from.is_empty());

        let nowhere = SmtpSubmitter::new(configured, "anna", Secret::new(PASSWORD)).pinned_to(Vec::new());
        assert_eq!(nowhere.check_login().await, Err(SmtpError::Offline));
        assert!(!format!("{nowhere:?}").contains(PASSWORD));
    }

    /// Task M11.2: a certificate the system trusts needs no pin, over TLS and STARTTLS.
    #[tokio::test]
    async fn a_trusted_certificate_signs_in_without_a_pin() {
        for security in [Security::Tls, Security::StartTls] {
            let server = Server::start_tls(Mode::Accept, security, Shows::TestCa).await;
            let submitter = tls_submitter(&server, security, vec![self_signed()]);
            assert_eq!(submitter.check_login_pinned().await, Ok(None), "{security:?}");
            let seen = server.received();
            assert!(seen.auth_sent && seen.starttls == (security == Security::StartTls), "{security:?}: {seen:?}");
            assert_eq!(server.connections(), 1, "{security:?}: one connection, no second look");
        }
    }

    /// Task M11.2: an untrusted certificate is reported with its fingerprint; the server never
    /// sees the login or an envelope, with no pin and with a wrong one.
    #[tokio::test]
    async fn an_untrusted_certificate_is_reported_and_the_server_sees_no_login() {
        for security in [Security::Tls, Security::StartTls] {
            for pins in [Vec::new(), vec![Fingerprint([0; 32])]] {
                let server = Server::start_tls(Mode::Accept, security, Shows::SelfSigned).await;
                let submitter = tls_submitter(&server, security, pins.clone());

                let untrusted = SmtpError::Untrusted(self_signed());
                assert_eq!(submitter.check_login().await, Err(untrusted), "{security:?} {pins:?}");
                assert_eq!(submitter.submit(&envelope(), MESSAGE).await, Submitted::NotAccepted(untrusted));
                let seen = server.received();
                assert!(!seen.auth_sent && seen.mail_from.is_empty() && seen.rcpt_to.is_empty(), "{seen:?}");
                assert!(seen.data.is_empty(), "{security:?}: nothing was sent");
                assert!(!untrusted.to_string().contains(PASSWORD));
            }
        }
    }

    /// Task M11.2: the pinned certificate (for another host name) signs in and sends, and the pin
    /// that was used is answered.
    #[tokio::test]
    async fn a_pinned_certificate_signs_in_and_sends() {
        for security in [Security::Tls, Security::StartTls] {
            let server = Server::start_tls(Mode::Accept, security, Shows::SelfSigned).await;
            let submitter = tls_submitter(&server, security, vec![Fingerprint([0; 32]), self_signed()]);

            assert_eq!(submitter.check_login_pinned().await, Ok(Some(self_signed())), "{security:?}");
            assert_eq!(submitter.submit(&envelope(), MESSAGE).await, Submitted::Accepted, "{security:?}");
            let seen = server.received();
            assert!(seen.auth_sent, "{security:?}");
            assert_eq!(seen.mail_from, ["MAIL FROM:<anna@example.org>"]);
            assert_eq!(seen.data, MESSAGE);
        }
    }

    /// Task M11.2: a TLS server that is not there stays offline; only a failed handshake gets the
    /// second look at the certificate.
    #[tokio::test]
    async fn an_unreachable_tls_server_is_offline_not_untrusted() {
        let port = {
            let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
            listener.local_addr().expect("address").port()
        };
        for security in [Security::Tls, Security::StartTls] {
            let endpoint = Endpoint { host: "127.0.0.1".into(), port, security };
            let submitter = SmtpSubmitter::new(endpoint, "anna", Secret::new(PASSWORD)).trusting(vec![self_signed()]);
            assert_eq!(submitter.check_login_pinned().await, Err(SmtpError::Offline), "{security:?}");
        }
    }

    #[test]
    fn ids_become_file_names_and_back() {
        for id in ["s1", "m:abc/../x", "Ünïcode ✓"] {
            let name = id_to_file(id);
            assert!(name.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert_eq!(id_from_file(&name).as_deref(), Some(id));
        }
        assert_eq!(id_from_file("abc"), None);
        assert_eq!(id_from_file("zz"), None);
    }
}
