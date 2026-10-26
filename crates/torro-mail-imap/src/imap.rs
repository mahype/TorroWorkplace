//! IMAP sessions (task M2.3): connecting with TLS or STARTTLS through rustls, signing in, and the
//! commands the core's sync and handlers need (`LIST`, `STATUS`, `SELECT`, `UID SEARCH`,
//! `UID FETCH`, `UID STORE`, `UID MOVE` or its fallback, `APPEND`, `IDLE`, `CONDSTORE`,
//! `GETQUOTAROOT`).
//!
//! The core (task M2.6) keeps two [`Session`]s per account on tokio: one waits in
//! [`Session::idle`] on the inbox, the other runs commands. A session is `Send`, every method is
//! `async` and takes `&mut self`, so one task owns one session at a time.
//!
//! **Own wire layer instead of `async-imap`.** Its parser stops the whole stream at responses it
//! does not know (`* XLIST`, `* NAMESPACE`), it hides response codes such as
//! `[AUTHENTICATIONFAILED]` or `[NONEXISTENT]` inside formatted text, and its `IDLE` panics on a
//! bare `BAD`. The few commands here need a line reader with literals and a small tokenizer;
//! that is less code than working around those gaps.
//!
//! **Security.** TLS only through rustls (aws-lc-rs) with the system's trust store
//! ([`Trust::System`]) or, where that cannot be built, the webpki roots, plus the certificates a
//! person confirmed ([`Trust::Pinned`], see [`crate::tls`]); there is no plain connection. A
//! certificate that is not trusted ends the connection before the login is sent
//! ([`ImapError::Untrusted`]). STARTTLS refuses a server that sends anything after its `OK` before the handshake
//! (command injection). The password is a [`Secret`]: it goes to the server and nowhere else,
//! every server text from the sign-in is stripped of it, and nothing here logs.

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::Instant;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use torro_core::account::Secret;
use torro_mail::{Endpoint, Flag, Role, Security};

/// Which certificates a connection trusts; lives in [`crate::tls`] since task M11.2, because
/// SMTP and CalDAV use it too.
pub use crate::tls::Trust;
use crate::tls::{Fingerprint, Seen, client_config};

/// How long connecting and the TLS handshake may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the server may stay silent while an answer is due.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
/// The longest response line taken (literals not counted); protects memory from a broken server.
const MAX_LINE: usize = 1 << 20;
/// The largest literal taken: a whole message with big attachments fits.
const MAX_LITERAL: usize = 100 << 20;
/// Literals are read in pieces of this size, each with its own [`READ_TIMEOUT`], so a slow but
/// moving download of a big message is not cut off.
const LITERAL_CHUNK: usize = 64 << 10;

/// What went wrong talking to an IMAP server. The text is English, for logs and for the core to
/// put into its own messages; it never holds the password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImapError {
    /// The server refused the sign-in: wrong name or password, an expired or locked account, or a
    /// provider that wants an app password. Carries the server's words. The core shows "Password
    /// was refused" at the account (task M2.6) and does not retry until the password changes.
    Auth(String),
    /// No usable connection: the host is not found, refuses or does not answer, the connection
    /// dropped, the server said `BYE` or is temporarily unavailable. Worth trying again later.
    Offline(String),
    /// The server answered something unexpected or refused a command: a broken response, no
    /// STARTTLS, a TLS handshake that failed for another reason than the certificate, `BAD`, a
    /// `NO` without a known reason.
    Protocol(String),
    /// A folder or message named in the command does not exist (`[NONEXISTENT]`, `[TRYCREATE]`).
    NotFound(String),
    /// The server showed a certificate that is neither trusted by the system nor pinned (task
    /// M11.2): this fingerprint, for the person to compare and confirm. The connection ended in
    /// the handshake, before the login was sent.
    Untrusted(Fingerprint),
}

impl ImapError {
    /// The same error with every occurrence of each non-empty secret blanked out, for texts that
    /// came back from the server while a secret was on the wire.
    fn redacted(self, secrets: &[&str]) -> ImapError {
        let clean = |text: String| redact(&text, secrets);
        match self {
            ImapError::Auth(text) => ImapError::Auth(clean(text)),
            ImapError::Offline(text) => ImapError::Offline(clean(text)),
            ImapError::Protocol(text) => ImapError::Protocol(clean(text)),
            ImapError::NotFound(text) => ImapError::NotFound(clean(text)),
            ImapError::Untrusted(fingerprint) => ImapError::Untrusted(fingerprint),
        }
    }
}

impl fmt::Display for ImapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImapError::Auth(text) => write!(f, "sign-in refused: {text}"),
            ImapError::Offline(text) => write!(f, "mail server not reachable: {text}"),
            ImapError::Protocol(text) => write!(f, "unexpected answer from the mail server: {text}"),
            ImapError::NotFound(text) => write!(f, "not found on the mail server: {text}"),
            ImapError::Untrusted(fingerprint) => {
                write!(f, "the certificate of the mail server is not trusted (SHA-256 {fingerprint})")
            }
        }
    }
}

impl std::error::Error for ImapError {}

/// The capabilities the server announced, in capital letters (`IMAP4REV1`, `AUTH=PLAIN`, `MOVE`).
/// Read again after signing in, because servers announce more then.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities(BTreeSet<String>);

impl Capabilities {
    /// The server announced `name` (any case): `has("condstore")`.
    pub fn has(&self, name: &str) -> bool {
        self.0.contains(&name.to_ascii_uppercase())
    }

    /// All announced capabilities, in capital letters.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    fn from_words<'a>(words: impl IntoIterator<Item = &'a str>) -> Capabilities {
        Capabilities(words.into_iter().filter(|w| !w.is_empty()).map(str::to_ascii_uppercase).collect())
    }
}

/// Where the account's own folders live (`NAMESPACE`, RFC 2342): new folders are made as
/// `prefix` + name, e.g. `INBOX.` + `Sent` on Courier or Cyrus, `` + `Sent` on Dovecot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    /// The prefix, ending in the delimiter when it is not empty.
    pub prefix: String,
    /// The hierarchy delimiter (`/` or `.`); `None` for a flat server.
    pub delimiter: Option<char>,
}

/// One folder as `LIST` (or `XLIST`) names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFolder {
    /// The path exactly as the server sent it (modified UTF-7, delimiters included). Commands use
    /// this, never a decoded name.
    pub path: String,
    /// The hierarchy delimiter; `None` for a flat server.
    pub delimiter: Option<char>,
    /// The attributes as sent, e.g. `\HasNoChildren`, `\Sent`, `\Noselect`.
    pub attributes: Vec<String>,
}

impl ListedFolder {
    /// The folder can be opened (`SELECT`); `\Noselect` and `\NonExistent` ones only hold others.
    pub fn selectable(&self) -> bool {
        !self.has_attribute("\\Noselect") && !self.has_attribute("\\NonExistent")
    }

    /// The folder has `attribute` (any case): `has_attribute("\\Sent")`.
    pub fn has_attribute(&self, attribute: &str) -> bool {
        self.attributes.iter().any(|a| a.eq_ignore_ascii_case(attribute))
    }
}

/// Counts and sync markers of a folder from `STATUS`, without opening it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FolderStatus {
    /// All messages.
    pub total: u32,
    /// Messages without `\Seen`.
    pub unread: u32,
    /// The UID the next message will get, if the server said.
    pub uid_next: Option<u32>,
    /// The folder's UIDVALIDITY, if the server said.
    pub uid_validity: Option<u32>,
    /// The folder's HIGHESTMODSEQ, when the server has CONDSTORE.
    pub highest_modseq: Option<u64>,
}

/// The storage quota of a quota root ([`Session::quota_root`], RFC 9208), in units of 1024 bytes
/// as the server counts the resource `STORAGE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    /// In use.
    pub used_kib: u64,
    /// Allowed.
    pub limit_kib: u64,
}

/// The folder a session has open ([`Session::select`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    /// The path as the server names it.
    pub path: String,
    /// UIDVALIDITY: UIDs stay valid only as long as this does not change.
    pub uid_validity: u32,
    /// The UID the next message will get, if the server said.
    pub uid_next: Option<u32>,
    /// Messages in the folder.
    pub exists: u32,
    /// HIGHESTMODSEQ, when the server has CONDSTORE and keeps mod-sequences for this folder;
    /// `None` otherwise (then sync compares UIDs and flags).
    pub highest_modseq: Option<u64>,
    /// Opened with `EXAMINE`, or the server answered `[READ-ONLY]`: commands that change messages
    /// ([`Session::store_flags`], [`Session::move_messages`], [`Session::delete_messages`]) fail on
    /// it with [`ImapError::Protocol`] before anything is sent. Code that reuses an open folder for
    /// such a command must `select` it again when this is set.
    pub read_only: bool,
    /// The flags the server keeps for good in this folder (`PERMANENTFLAGS`), as sent; `\*` means
    /// it takes new keywords. `None` when the server did not say (then every flag may be kept).
    pub permanent_flags: Option<Vec<String>>,
}

impl Selected {
    /// The UIDs the core knows from an earlier sync (stored with `known`) no longer name the same
    /// messages: the folder was recreated or reset, so everything cached for it must be dropped
    /// and fetched anew. `None` (never synced) is not a change.
    pub fn uid_validity_changed(&self, known: Option<u32>) -> bool {
        known.is_some_and(|known| known != self.uid_validity)
    }
}

/// What [`Session::uid_search`] looks for in the open folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Search {
    /// Every message.
    All,
    /// Messages with this UID or a higher one: what arrived since the last sync (`UID n:*`).
    UidsFrom(u32),
    /// Messages that arrived on or after this day (`SINCE`, the server's internal date); for
    /// fetching the texts of the last 90 days.
    Since(NaiveDate),
}

/// Headers and markers of one message ([`Session::fetch_headers`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedHeader {
    pub uid: u32,
    /// The flags the suite knows; others (keywords, `\Draft`) are left out.
    pub flags: Vec<Flag>,
    /// The message is marked `\Deleted` and waits to be expunged; the core hides it.
    pub deleted: bool,
    /// When the server received it; the fallback when `Date:` is missing or broken.
    pub internal_date: Option<DateTime<Utc>>,
    /// Size in bytes (`RFC822.SIZE`).
    pub size: u32,
    /// The message's mod-sequence, when the server has CONDSTORE.
    pub modseq: Option<u64>,
    /// The body structure names a part with `Content-Disposition: attachment`.
    pub has_attachments: bool,
    /// The whole header block, raw (`mime` or `mail-parser` reads it).
    pub header: Vec<u8>,
}

/// Metadata retained for each backed-up folder occurrence, including every server flag and the
/// original INTERNALDATE offset. Unlike `FetchedHeader`, this omits derived MIME data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupMessage {
    pub uid: u32,
    pub flags: Vec<String>,
    pub internal_date: DateTime<FixedOffset>,
    pub bytes: u64,
    /// Gmail's message id (`X-GM-MSGID`), the same in every label folder that shows the message;
    /// only asked for when the server announces `X-GM-EXT-1`, else `None`.
    pub gm_msgid: Option<u64>,
}

/// Why reading a whole original in pieces ([`BodyChunks`]) failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyError {
    /// The connection or the server failed.
    Imap(ImapError),
    /// The server no longer returns the message: it was expunged while it was read.
    Vanished,
    /// A piece came back shorter than its place before `RFC822.SIZE` asked for.
    Truncated,
    /// Bytes follow after `RFC822.SIZE`: the size was too small, what was read is not the whole
    /// original.
    Oversized,
}

impl From<ImapError> for BodyError {
    fn from(error: ImapError) -> Self {
        BodyError::Imap(error)
    }
}

impl From<BodyError> for ImapError {
    /// For callers without their own reasons: a vanished message is [`ImapError::NotFound`], a body
    /// that does not match its size [`ImapError::Protocol`].
    fn from(error: BodyError) -> Self {
        match error {
            BodyError::Imap(error) => error,
            BodyError::Vanished => ImapError::NotFound("message vanished while its original was read".into()),
            BodyError::Truncated => ImapError::Protocol("original shorter than its RFC822.SIZE".into()),
            BodyError::Oversized => ImapError::Protocol("original longer than its RFC822.SIZE".into()),
        }
    }
}

/// Reads one original of the open folder in pieces of at most 64 KiB (`BODY.PEEK[]<offset.count>`,
/// never marked read), so even a message far beyond the literal limit needs no full buffer. The one
/// place that does this (backup, restore check, offline originals).
///
/// `size` is the message's `RFC822.SIZE`. After the last piece it asks for one byte at `size` and
/// requires an empty answer (RFC 3501, 6.4.5: an origin past the end gives an empty string): a cut
/// original must never pass for the whole one.
///
/// ```ignore
/// let mut body = BodyChunks::new(uid, size);
/// while let Some(piece) = body.next(&mut session).await? { spool.write_all(&piece)?; }
/// ```
#[derive(Debug, Clone)]
pub struct BodyChunks {
    uid: u32,
    size: u64,
    offset: u64,
    done: bool,
}

impl BodyChunks {
    /// Reads message `uid` of `size` bytes from the start.
    pub fn new(uid: u32, size: u64) -> BodyChunks {
        BodyChunks { uid, size, offset: 0, done: false }
    }

    /// The next piece, in order; `None` once every byte up to `size` was returned and the server
    /// confirmed that nothing follows. Errors: [`BodyError::Vanished`] when the server returns no
    /// such message, [`BodyError::Truncated`] for a short piece, [`BodyError::Oversized`] for bytes
    /// after `size`, [`BodyError::Imap`] for the connection. After an error, start anew.
    pub async fn next(&mut self, session: &mut Session) -> Result<Option<Vec<u8>>, BodyError> {
        if self.done {
            return Ok(None);
        }
        if self.offset >= self.size {
            let rest = session.fetch_backup_chunk(self.uid, self.size, 1).await?.ok_or(BodyError::Vanished)?;
            if !rest.is_empty() {
                return Err(BodyError::Oversized);
            }
            self.done = true;
            return Ok(None);
        }
        let count = (self.size - self.offset).min(LITERAL_CHUNK as u64) as usize;
        let piece = session.fetch_backup_chunk(self.uid, self.offset, count).await?.ok_or(BodyError::Vanished)?;
        if piece.len() != count {
            return Err(BodyError::Truncated);
        }
        self.offset += count as u64;
        Ok(Some(piece))
    }
}

/// The flags of one message ([`Session::fetch_flags`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagState {
    pub uid: u32,
    pub flags: Vec<Flag>,
    /// Every IMAP flag and keyword as returned by the server, for retained originals.
    pub raw_flags: Vec<String>,
    /// Marked `\Deleted`, waiting to be expunged.
    pub deleted: bool,
    /// The message's mod-sequence, when the server has CONDSTORE.
    pub modseq: Option<u64>,
}

/// Where an appended message landed (`APPENDUID`, servers with UIDPLUS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendUid {
    pub uid_validity: u32,
    pub uid: u32,
}

/// Why [`Session::append_backup`] stored nothing it could confirm, and whether the server may
/// have stored the message anyway. A restore (#267) acts on that difference: a message that was
/// certainly not stored may be sent again, one that may have been stored must not be sent again
/// without a person's say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendError {
    /// The server answered the `APPEND` with `NO` or `BAD`, before or after the message: nothing
    /// was stored, and the session can be used further.
    Refused(ImapError),
    /// The command failed before the last piece of the message was handed to the connection (busy
    /// or broken connection, a local read error, an invalid argument): part of the message never
    /// left, so the server cannot have stored it. The session is not usable any more.
    NotSent(ImapError),
    /// The last piece of the message was handed to the connection but no tagged answer came back
    /// (writing that piece or the closing line break failed, the connection broke, `BYE`, a broken
    /// answer): the server may have stored it. The session is not usable any more.
    Unconfirmed(ImapError),
}

impl AppendError {
    /// The IMAP error behind it.
    pub fn imap(&self) -> &ImapError {
        match self {
            Self::Refused(error) | Self::NotSent(error) | Self::Unconfirmed(error) => error,
        }
    }
}

impl fmt::Display for AppendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(error) => write!(f, "APPEND refused: {error}"),
            Self::NotSent(error) => write!(f, "APPEND not sent: {error}"),
            Self::Unconfirmed(error) => write!(f, "APPEND unconfirmed: {error}"),
        }
    }
}

impl std::error::Error for AppendError {}

/// `path` as a folder name on the wire. Without `UTF8=ACCEPT` (this crate never enables it) IMAP
/// names folders in modified UTF-7 (RFC 3501 5.1.3), and `LIST` sends them so: a plain ASCII
/// `path` is taken as such a name and stays as it is. Any other `path` is raw text (as an archive
/// or a person may hold it): each run of characters outside printable ASCII becomes `&…-`, `&`
/// becomes `&-`. A name that mixes an encoded part with raw Unicode is therefore taken as text.
pub fn encode_mailbox(path: &str) -> std::borrow::Cow<'_, str> {
    if path.is_ascii() {
        return std::borrow::Cow::Borrowed(path);
    }
    let mut out = String::with_capacity(path.len() * 2);
    let mut run: Vec<u16> = Vec::new();
    for c in path.chars() {
        if (' '..='~').contains(&c) {
            push_utf7_run(&mut out, &mut run);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            let mut units = [0u16; 2];
            run.extend_from_slice(c.encode_utf16(&mut units));
        }
    }
    push_utf7_run(&mut out, &mut run);
    std::borrow::Cow::Owned(out)
}

/// Writes `run` (UTF-16 units) as `&<modified base64>-` and empties it; nothing when it is empty.
fn push_utf7_run(out: &mut String, run: &mut Vec<u16>) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";
    if run.is_empty() {
        return;
    }
    out.push('&');
    let mut bits: u32 = 0;
    let mut count = 0u32;
    for byte in run.iter().flat_map(|unit| unit.to_be_bytes()) {
        bits = (bits << 8) | u32::from(byte);
        count += 8;
        while count >= 6 {
            count -= 6;
            out.push(char::from(ALPHABET[((bits >> count) & 0x3f) as usize]));
        }
        bits &= (1 << count) - 1;
    }
    if count > 0 {
        out.push(char::from(ALPHABET[((bits << (6 - count)) & 0x3f) as usize]));
    }
    out.push('-');
    run.clear();
}

/// Where the server says an appended message landed (`APPENDUID`), if it says.
fn append_uid(outcome: &Outcome) -> Option<AppendUid> {
    if outcome.code_name() != Some("APPENDUID") {
        return None;
    }
    let mut numbers = outcome.code_args().split_whitespace().map(str::parse::<u32>);
    match (numbers.next(), numbers.next()) {
        (Some(Ok(uid_validity)), Some(Ok(uid))) => Some(AppendUid { uid_validity, uid }),
        _ => None,
    }
}

/// How [`Session::idle`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleEvent {
    /// The server reported a change in the open folder (new, removed or changed messages): sync
    /// it now.
    Changed,
    /// Nothing happened within the time asked for; call `idle` again (the core uses at most 25
    /// minutes, below the 29 minutes after which servers may drop an idle client).
    Timeout,
}

/// A signed-in IMAP connection over TLS.
///
/// **A dropped call breaks the session.** When a future of a method is dropped before it
/// finishes (a `tokio::select!` that shuts down, a timeout around it), the connection is in the
/// middle of an exchange; every later call then fails with [`ImapError::Offline`]. Drop the
/// session and connect anew.
pub struct Session {
    wire: Wire<TlsStream<TcpStream>>,
    capabilities: Capabilities,
    selected: Option<Selected>,
    /// The pin the server's certificate matched ([`Session::pinned_certificate`]).
    pinned: Option<Fingerprint>,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("capabilities", &self.capabilities)
            .field("selected", &self.selected)
            .field("pinned", &self.pinned)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Connects to `endpoint`, secures the connection (TLS from the first byte or STARTTLS) and
    /// signs in as `login` with `password`: `AUTHENTICATE PLAIN` when the server offers it (it
    /// carries any character), else `LOGIN`.
    ///
    /// Errors: [`ImapError::Offline`] when the host cannot be reached or does not answer in
    /// time; [`ImapError::Untrusted`] for a certificate `trust` does not take (nothing of the
    /// login was sent); [`ImapError::Protocol`] for a failed handshake, a missing STARTTLS, a server
    /// that sends data before the handshake or offers no way to sign in; [`ImapError::Auth`]
    /// when the server refuses the sign-in (`NO` without a reason or with
    /// `AUTHENTICATIONFAILED`, `AUTHORIZATIONFAILED`, `EXPIRED`, `ALERT`). `NO [UNAVAILABLE]` is
    /// `Offline`. No error holds the password.
    pub async fn connect(
        endpoint: &Endpoint,
        login: &str,
        password: &Secret,
        trust: &Trust,
    ) -> Result<Session, ImapError> {
        let tls = tls_config(trust)?;
        let server_name = server_name(endpoint)?;
        let address = (endpoint.host.as_str(), endpoint.port);
        let tcp = timed(CONNECT_TIMEOUT, TcpStream::connect(address)).await?;
        Session::secure(tcp, tls, server_name, endpoint.security, login, password).await
    }

    /// [`Session::connect`] to one of `addresses` instead of whatever `endpoint`'s host resolves to
    /// now (task M2.7b): the core checks the host's addresses (public internet only) and connects
    /// to exactly those, so a name that answers differently the second time (DNS rebinding) never
    /// reaches another machine. The host name of `endpoint` is still what TLS checks the
    /// certificate against and sends as SNI; its port is not used (each address has its own).
    ///
    /// The addresses are tried in order until one takes the connection. No address at all is
    /// [`ImapError::Offline`]; every other error as for [`Session::connect`].
    pub async fn connect_to(
        addresses: &[SocketAddr],
        endpoint: &Endpoint,
        login: &str,
        password: &Secret,
        trust: &Trust,
    ) -> Result<Session, ImapError> {
        let tls = tls_config(trust)?;
        let server_name = server_name(endpoint)?;
        if addresses.is_empty() {
            return Err(ImapError::Offline("no address to connect to".into()));
        }
        let tcp = timed(CONNECT_TIMEOUT, TcpStream::connect(addresses)).await?;
        Session::secure(tcp, tls, server_name, endpoint.security, login, password).await
    }

    /// Secures the connection `tcp` as `security` asks (TLS from the first byte or STARTTLS, the
    /// certificate checked for `server_name`) and signs in.
    async fn secure(
        tcp: TcpStream,
        tls: (Arc<ClientConfig>, Seen),
        server_name: ServerName<'static>,
        security: Security,
        login: &str,
        password: &Secret,
    ) -> Result<Session, ImapError> {
        let (config, seen) = tls;
        let (wire, greeting) = match security {
            Security::Tls => {
                let mut wire = Wire::new(handshake(config, server_name, tcp, &seen).await?, 1);
                let greeting = wire.greeting().await?;
                (wire, greeting)
            }
            Security::StartTls => {
                let mut plain = Wire::new(tcp, 1);
                if plain.greeting().await?.preauth {
                    return Err(ImapError::Protocol("the server signed in before the connection was secured".into()));
                }
                plain.starttls().await?;
                let (tcp, next_tag) = plain.into_stream()?;
                let wire = Wire::new(handshake(config, server_name, tcp, &seen).await?, next_tag);
                // Anything learned before the handshake could have been forged: ask again.
                (wire, Greeting { preauth: false, capabilities: None })
            }
        };
        // The handshake is done: a pinned certificate passed it with a valid signature.
        let pinned = seen.accepted();
        let mut session = Session { wire, capabilities: Capabilities::default(), selected: None, pinned };
        match greeting.capabilities {
            Some(capabilities) => session.capabilities = capabilities,
            None => session.refresh_capabilities().await?,
        }
        if !greeting.preauth {
            session.sign_in(login, password).await?;
        }
        Ok(session)
    }

    /// The pin of the [`Trust`] the server's certificate matched (task M11.2); `None` when the
    /// system (the trust's base) took the certificate. The core keeps only pins that were used.
    pub fn pinned_certificate(&self) -> Option<Fingerprint> {
        self.pinned
    }

    /// The server's capabilities, as announced after signing in.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// The folder opened by the last successful [`Session::select`], if any.
    pub fn selected(&self) -> Option<&Selected> {
        self.selected.as_ref()
    }

    /// Where the account's own folders live: the first personal namespace of `NAMESPACE`, or,
    /// on servers without it, an empty prefix with the delimiter from `LIST "" ""`.
    pub async fn namespace(&mut self) -> Result<Namespace, ImapError> {
        if self.capabilities.has("NAMESPACE") {
            let data = self.run(&[Arg::Raw("NAMESPACE")], "NAMESPACE").await?;
            if let Some(namespace) = data.iter().find_map(personal_namespace) {
                return Ok(namespace);
            }
        }
        let data = self.run(&[Arg::Raw("LIST"), Arg::Str(""), Arg::Str("")], "LIST").await?;
        let delimiter = parse_list(&data, "LIST").into_iter().next().and_then(|folder| folder.delimiter);
        Ok(Namespace { prefix: String::new(), delimiter })
    }

    /// All folders: `LIST "" "*" RETURN (SPECIAL-USE)` when the server has SPECIAL-USE (plain
    /// `LIST` when it refuses that), else `XLIST "" "*"` when it has XLIST, else plain `LIST`.
    /// Paths come back exactly as the server sent them.
    pub async fn list(&mut self) -> Result<Vec<ListedFolder>, ImapError> {
        let all = [Arg::Raw("LIST"), Arg::Str(""), Arg::Str("*")];
        if self.capabilities.has("SPECIAL-USE") {
            let reply = self.wire.exchange(&[all[0], all[1], all[2], Arg::Raw("RETURN (SPECIAL-USE)")]).await?;
            if reply.outcome.status == Status::Ok {
                return Ok(parse_list(&reply.data, "LIST"));
            }
        } else if self.capabilities.has("XLIST") {
            let reply = self.wire.exchange(&[Arg::Raw("XLIST"), Arg::Str(""), Arg::Str("*")]).await?;
            if reply.outcome.status == Status::Ok {
                return Ok(parse_list(&reply.data, "XLIST"));
            }
        }
        let data = self.run(&all, "LIST").await?;
        Ok(parse_list(&data, "LIST"))
    }

    /// Counts and sync markers of the folder at `path` (`STATUS`), without opening it.
    /// [`ImapError::NotFound`] when it does not exist.
    pub async fn status(&mut self, path: &str) -> Result<FolderStatus, ImapError> {
        let items = if self.condstore() {
            "(MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ)"
        } else {
            "(MESSAGES UNSEEN UIDNEXT UIDVALIDITY)"
        };
        let data = self.run(&[Arg::Raw("STATUS"), Arg::Str(path), Arg::Raw(items)], "STATUS").await?;
        let mut status = FolderStatus::default();
        for tokens in data.iter().filter_map(Response::data).filter(|t| first_is(t, "STATUS")) {
            let Some(items) = tokens.get(2).and_then(Token::list) else { continue };
            for pair in items.chunks(2) {
                let (Some(name), Some(value)) = (pair[0].atom(), pair.get(1)) else { continue };
                match name.to_ascii_uppercase().as_str() {
                    "MESSAGES" => status.total = value.number().unwrap_or(0),
                    "UNSEEN" => status.unread = value.number().unwrap_or(0),
                    "UIDNEXT" => status.uid_next = value.number(),
                    "UIDVALIDITY" => status.uid_validity = value.number(),
                    "HIGHESTMODSEQ" => status.highest_modseq = value.number().filter(|&m| m > 0),
                    _ => {}
                }
            }
        }
        Ok(status)
    }

    /// The storage quota of the quota root `mailbox` belongs to (`GETQUOTAROOT`, RFC 9208): the
    /// resource `STORAGE` of the first `* QUOTA` that names one. `Ok(None)` without sending
    /// anything when the server does not announce `QUOTA`, and when no root limits storage.
    /// `mailbox` goes out as given, like the path of [`Session::status`].
    pub async fn quota_root(&mut self, mailbox: &str) -> Result<Option<Quota>, ImapError> {
        if !self.capabilities.has("QUOTA") {
            return Ok(None);
        }
        let data = self.run(&[Arg::Raw("GETQUOTAROOT"), Arg::Str(mailbox)], "GETQUOTAROOT").await?;
        Ok(data.iter().filter_map(Response::data).filter(|tokens| first_is(tokens, "QUOTA")).find_map(storage_quota))
    }

    /// Makes the folder `path` (as the server will name it, prefix included). With a `role` and
    /// a server that has CREATE-SPECIAL-USE it is created as that role (`CREATE … (USE
    /// (\Sent))`); refuses the server the role, it is created plainly. A folder that already
    /// exists (`[ALREADYEXISTS]`) is fine. `path` goes out as [`encode_mailbox`] makes it.
    pub async fn create_folder(&mut self, path: &str, role: Option<Role>) -> Result<(), ImapError> {
        let path = &*encode_mailbox(path);
        if let Some(role) = role
            && self.capabilities.has("CREATE-SPECIAL-USE")
        {
            let use_role = format!("(USE ({}))", special_use(role));
            let reply = self.wire.exchange(&[Arg::Raw("CREATE"), Arg::Str(path), Arg::Raw(&use_role)]).await?;
            match (reply.outcome.status, reply.outcome.code_name()) {
                (Status::Ok, _) | (Status::No, Some("ALREADYEXISTS")) => return Ok(()),
                // The server refused the role, not necessarily the folder: try without it.
                (Status::Bad, _) | (Status::No, Some("USEATTR")) => {}
                _ => return Err(refused(&reply.outcome, "CREATE")),
            }
        }
        let reply = self.wire.exchange(&[Arg::Raw("CREATE"), Arg::Str(path)]).await?;
        match (reply.outcome.status, reply.outcome.code_name()) {
            (Status::Ok, _) | (Status::No, Some("ALREADYEXISTS")) => Ok(()),
            _ => Err(refused(&reply.outcome, "CREATE")),
        }
    }

    /// Subscribes to the folder at `path`, so other mail programs show it too. `path` goes out as
    /// [`encode_mailbox`] makes it.
    pub async fn subscribe(&mut self, path: &str) -> Result<(), ImapError> {
        self.run(&[Arg::Raw("SUBSCRIBE"), Arg::Str(&encode_mailbox(path))], "SUBSCRIBE").await.map(drop)
    }

    /// Opens the folder at `path` for the message commands (`SELECT`, with `(CONDSTORE)` when the
    /// server has it). Compare [`Selected::uid_validity`] with the stored one
    /// ([`Selected::uid_validity_changed`]) before trusting cached UIDs.
    /// [`ImapError::NotFound`] when it does not exist; [`ImapError::Protocol`] when the server
    /// sends no UIDVALIDITY (nothing could be cached safely).
    pub async fn select(&mut self, path: &str) -> Result<Selected, ImapError> {
        self.open_folder(path, false).await
    }

    /// Opens a folder read-only for an inventory, backup or restore test run. The server must not
    /// accept changes through this selected folder, even if a future caller sends one by mistake.
    pub async fn examine(&mut self, path: &str) -> Result<Selected, ImapError> {
        self.open_folder(path, true).await
    }

    async fn open_folder(&mut self, path: &str, read_only: bool) -> Result<Selected, ImapError> {
        self.selected = None;
        let command = if read_only { "EXAMINE" } else { "SELECT" };
        let wire_path = encode_mailbox(path);
        let mut args = vec![Arg::Raw(command), Arg::Str(&wire_path)];
        if self.condstore() {
            args.push(Arg::Raw("(CONDSTORE)"));
        }
        // The tagged answer says `[READ-ONLY]` when even a SELECT may not change the folder.
        let reply = self.wire.exchange(&args).await?;
        if reply.outcome.status != Status::Ok {
            return Err(refused(&reply.outcome, command));
        }
        let data = reply.data;
        let mut uid_validity = None;
        let mut selected = Selected {
            path: path.into(),
            uid_validity: 0,
            uid_next: None,
            exists: 0,
            highest_modseq: None,
            read_only: read_only || reply.outcome.code_name() == Some("READ-ONLY"),
            permanent_flags: None,
        };
        for response in &data {
            match response {
                Response::Data(tokens) if tokens.get(1).is_some_and(|t| t.is("EXISTS")) => {
                    selected.exists = tokens[0].number().unwrap_or(0);
                }
                Response::Status(outcome) => match outcome.code_name() {
                    Some("UIDVALIDITY") => uid_validity = outcome.code_number(),
                    Some("UIDNEXT") => selected.uid_next = outcome.code_number(),
                    Some("HIGHESTMODSEQ") => selected.highest_modseq = outcome.code_number().filter(|&m| m > 0),
                    Some("PERMANENTFLAGS") => {
                        let list = outcome.code_args().trim_start_matches('(').trim_end_matches(')');
                        selected.permanent_flags = Some(list.split_whitespace().map(str::to_owned).collect());
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        selected.uid_validity =
            uid_validity.ok_or_else(|| ImapError::Protocol(format!("{command} of {path:?} named no UIDVALIDITY")))?;
        self.selected = Some(selected.clone());
        Ok(selected)
    }

    /// The UIDs in the open folder that match `search`, ascending.
    pub async fn uid_search(&mut self, search: &Search) -> Result<Vec<u32>, ImapError> {
        self.require_selected()?;
        let criteria = match search {
            Search::All => "ALL".to_owned(),
            Search::UidsFrom(from) => format!("UID {from}:*"),
            Search::Since(day) => format!("SINCE {}", day.format("%-d-%b-%Y")),
        };
        let data = self.run(&[Arg::Raw("UID SEARCH"), Arg::Raw(&criteria)], "SEARCH").await?;
        let mut uids: Vec<u32> = data
            .iter()
            .filter_map(Response::data)
            .filter(|tokens| first_is(tokens, "SEARCH"))
            .flat_map(|tokens| tokens[1..].iter().filter_map(Token::number))
            .collect();
        // `UID n:*` always names the highest UID, even one below n.
        if let Search::UidsFrom(from) = search {
            uids.retain(|uid| uid >= from);
        }
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }

    /// The UIDs in the open folder whose headers or text hold every word of `words`
    /// (`UID SEARCH TEXT …`, one `TEXT` per word, as the server compares: usually a substring
    /// without regard to case) and that arrived on or after `since` and before `before` (`SINCE`,
    /// `BEFORE`: the server's internal date, whole days). Ascending. For searching the texts the
    /// core's store no longer keeps (task M3.1).
    ///
    /// Words are sent as IMAP strings (quoted, or as a literal with `CHARSET UTF-8` when one is
    /// not plain ASCII); control characters are dropped, so no word can end the command. No words
    /// and no days searches `ALL`. The caller bounds the number of words (`mail.search_messages`
    /// takes at most 500 characters).
    pub async fn uid_search_text(
        &mut self,
        words: &str,
        since: Option<NaiveDate>,
        before: Option<NaiveDate>,
    ) -> Result<Vec<u32>, ImapError> {
        self.require_selected()?;
        let criteria = text_criteria(words, since, before);
        let mut args = vec![Arg::Raw("UID SEARCH")];
        args.extend(criteria.iter().map(|part| match part {
            SearchPart::Raw(raw) => Arg::Raw(raw),
            SearchPart::Str(text) => Arg::Str(text),
        }));
        let data = self.run(&args, "SEARCH").await?;
        let mut uids: Vec<u32> = data
            .iter()
            .filter_map(Response::data)
            .filter(|tokens| first_is(tokens, "SEARCH"))
            .flat_map(|tokens| tokens[1..].iter().filter_map(Token::number))
            .collect();
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }

    /// Headers, flags, date, size and attachment marker of the messages `uids` in the open
    /// folder, in one `UID FETCH` (`BODY.PEEK[HEADER]`, so nothing is marked read). Messages
    /// that no longer exist are missing from the answer.
    pub async fn fetch_headers(&mut self, uids: &[u32]) -> Result<Vec<FetchedHeader>, ImapError> {
        self.require_selected()?;
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let items = if self.condstore() {
            "(UID FLAGS INTERNALDATE RFC822.SIZE BODYSTRUCTURE MODSEQ BODY.PEEK[HEADER])"
        } else {
            "(UID FLAGS INTERNALDATE RFC822.SIZE BODYSTRUCTURE BODY.PEEK[HEADER])"
        };
        let set = uid_set(uids);
        let data = self.run(&[Arg::Raw("UID FETCH"), Arg::Raw(&set), Arg::Raw(items)], "FETCH").await?;
        Ok(fetched(&data)
            .filter_map(|item| {
                let header = item.get("BODY[HEADER]")?.bytes()?.to_vec();
                let (flags, deleted) = item.get("FLAGS").map(parse_flags).unwrap_or_default();
                Some(FetchedHeader {
                    uid: item.uid,
                    flags,
                    deleted,
                    internal_date: item.get("INTERNALDATE").and_then(Token::text).and_then(|t| internal_date(&t)),
                    size: item.get("RFC822.SIZE").and_then(Token::number).unwrap_or(0),
                    modseq: item.modseq(),
                    has_attachments: item.get("BODYSTRUCTURE").is_some_and(has_attachment),
                    header,
                })
            })
            .collect())
    }

    /// Raw backup metadata without narrowing flags to Torro's ordinary reading set, for all `uids`
    /// in one `UID FETCH` (pass batches of a few hundred). With `X-GM-EXT-1` it also asks for
    /// `X-GM-MSGID` ([`BackupMessage::gm_msgid`]). The caller compares returned UIDs with its
    /// inventory and treats any missing UID as incomplete.
    pub async fn fetch_backup_metadata(&mut self, uids: &[u32]) -> Result<Vec<BackupMessage>, ImapError> {
        self.require_selected()?;
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set = uid_set(uids);
        let items = if self.capabilities.has("X-GM-EXT-1") {
            "(UID FLAGS INTERNALDATE RFC822.SIZE X-GM-MSGID)"
        } else {
            "(UID FLAGS INTERNALDATE RFC822.SIZE)"
        };
        let data = self.run(&[Arg::Raw("UID FETCH"), Arg::Raw(&set), Arg::Raw(items)], "FETCH").await?;
        fetched(&data)
            .map(|item| {
                let flags = item
                    .get("FLAGS")
                    .and_then(Token::list)
                    .ok_or_else(|| ImapError::Protocol("backup FETCH named no FLAGS".into()))?
                    .iter()
                    .map(|token| token.text().ok_or_else(|| ImapError::Protocol("invalid backup flag".into())))
                    .collect::<Result<Vec<_>, _>>()?;
                let internal_date = item
                    .get("INTERNALDATE")
                    .and_then(Token::text)
                    .and_then(|value| DateTime::parse_from_str(value.trim(), "%d-%b-%Y %H:%M:%S %z").ok())
                    .ok_or_else(|| ImapError::Protocol("backup FETCH named no valid INTERNALDATE".into()))?;
                let bytes = item
                    .get("RFC822.SIZE")
                    .and_then(Token::number)
                    .ok_or_else(|| ImapError::Protocol("backup FETCH named no valid RFC822.SIZE".into()))?;
                let gm_msgid = item.get("X-GM-MSGID").and_then(Token::number);
                Ok(BackupMessage { uid: item.uid, flags, internal_date, bytes, gm_msgid })
            })
            .collect()
    }

    /// The sizes (`RFC822.SIZE`) of the messages `uids` of the selected folder, as `(uid, bytes)`
    /// (#314). Lenient where [`Session::fetch_backup_metadata`] is strict: a `FETCH` without a size
    /// or for a UID that was not asked for (a flag another client changed meanwhile) is left out, so
    /// it costs one size, not the whole batch.
    pub async fn fetch_sizes(&mut self, uids: &[u32]) -> Result<Vec<(u32, u64)>, ImapError> {
        self.require_selected()?;
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set = uid_set(uids);
        let data = self.run(&[Arg::Raw("UID FETCH"), Arg::Raw(&set), Arg::Raw("(UID RFC822.SIZE)")], "FETCH").await?;
        let asked: std::collections::BTreeSet<u32> = uids.iter().copied().collect();
        Ok(fetched(&data)
            .filter(|item| asked.contains(&item.uid))
            .filter_map(|item| Some((item.uid, item.get("RFC822.SIZE").and_then(Token::number)?)))
            .collect())
    }

    /// Returns at most 64 KiB of original RFC-822 bytes via IMAP partial `BODY.PEEK[]`. A server
    /// that omits the requested item is reported as a vanished message by the backup caller.
    pub async fn fetch_backup_chunk(
        &mut self,
        uid: u32,
        offset: u64,
        count: usize,
    ) -> Result<Option<Vec<u8>>, ImapError> {
        self.require_selected()?;
        if count == 0 || count > LITERAL_CHUNK || offset.checked_add(count as u64).is_none() {
            return Err(ImapError::Protocol("invalid backup chunk range".into()));
        }
        let set = uid.to_string();
        let item = format!("(UID BODY.PEEK[]<{offset}.{count}>)");
        let returned = format!("BODY[]<{offset}>");
        let data = self.run(&[Arg::Raw("UID FETCH"), Arg::Raw(&set), Arg::Raw(&item)], "FETCH").await?;
        Ok(fetched(&data)
            .filter(|item| item.uid == uid)
            .find_map(|item| item.get(&returned).and_then(Token::bytes).map(<[u8]>::to_vec)))
    }

    /// The flags of every message in the open folder (`UID FETCH 1:* (UID FLAGS)`), or, with
    /// `changed_since` and a server with CONDSTORE, only of those changed after that
    /// mod-sequence (`CHANGEDSINCE`). Without CONDSTORE `changed_since` is ignored and all come
    /// back, so the caller compares.
    pub async fn fetch_flags(&mut self, changed_since: Option<u64>) -> Result<Vec<FlagState>, ImapError> {
        let exists = self.require_selected()?.exists;
        if exists == 0 {
            return Ok(Vec::new());
        }
        let condstore = self.condstore();
        let mut items = String::from(if condstore { "(UID FLAGS MODSEQ)" } else { "(UID FLAGS)" });
        if let Some(modseq) = changed_since.filter(|_| condstore) {
            items.push_str(&format!(" (CHANGEDSINCE {modseq})"));
        }
        let data = self.run(&[Arg::Raw("UID FETCH 1:*"), Arg::Raw(&items)], "FETCH").await?;
        Ok(fetched(&data)
            .filter_map(|item| {
                let flag_list = item.get("FLAGS")?;
                let (flags, deleted) = parse_flags(flag_list);
                let raw_flags = flag_list.list()?.iter().filter_map(Token::atom).map(str::to_string).collect();
                Some(FlagState { uid: item.uid, flags, raw_flags, deleted, modseq: item.modseq() })
            })
            .collect())
    }

    /// The whole message `uid` of the open folder, raw (`BODY.PEEK[]`, not marked read);
    /// `None` when it no longer exists.
    pub async fn fetch_message(&mut self, uid: u32) -> Result<Option<Vec<u8>>, ImapError> {
        self.require_selected()?;
        let set = uid.to_string();
        let data = self.run(&[Arg::Raw("UID FETCH"), Arg::Raw(&set), Arg::Raw("(UID BODY.PEEK[])")], "FETCH").await?;
        Ok(fetched(&data)
            .filter(|item| item.uid == uid)
            .find_map(|item| item.get("BODY[]").and_then(Token::bytes).map(<[u8]>::to_vec)))
    }

    /// Adds and removes flags of the messages `uids` in the open folder (`UID STORE …
    /// +FLAGS.SILENT` / `-FLAGS.SILENT`).
    pub async fn store_flags(&mut self, uids: &[u32], add: &[Flag], remove: &[Flag]) -> Result<(), ImapError> {
        self.require_writable()?;
        if uids.is_empty() {
            return Ok(());
        }
        let set = uid_set(uids);
        for (change, flags) in [("+FLAGS.SILENT", add), ("-FLAGS.SILENT", remove)] {
            if flags.is_empty() {
                continue;
            }
            let list = format!("({})", flags.iter().map(|&flag| flag_atom(flag)).collect::<Vec<_>>().join(" "));
            self.run(&[Arg::Raw("UID STORE"), Arg::Raw(&set), Arg::Raw(change), Arg::Raw(&list)], "STORE").await?;
        }
        Ok(())
    }

    /// Moves the messages `uids` of the open folder to the folder at `target`: `UID MOVE` when
    /// the server has MOVE, else `UID COPY`, `\Deleted` and expunging them
    /// ([`Session::delete_messages`]). [`ImapError::NotFound`] when `target` does not exist.
    pub async fn move_messages(&mut self, uids: &[u32], target: &str) -> Result<(), ImapError> {
        self.require_writable()?;
        if uids.is_empty() {
            return Ok(());
        }
        let set = uid_set(uids);
        if self.capabilities.has("MOVE") {
            return self.run(&[Arg::Raw("UID MOVE"), Arg::Raw(&set), Arg::Str(target)], "MOVE").await.map(drop);
        }
        self.run(&[Arg::Raw("UID COPY"), Arg::Raw(&set), Arg::Str(target)], "COPY").await?;
        self.delete_messages(uids).await
    }

    /// Removes the messages `uids` of the open folder for good: `\Deleted`, then `UID EXPUNGE`
    /// with UIDPLUS. Without UIDPLUS the server only knows `EXPUNGE`, which also removes other
    /// messages someone marked `\Deleted` in this folder; that is what "deleted" means in IMAP,
    /// and older mail programs do the same.
    pub async fn delete_messages(&mut self, uids: &[u32]) -> Result<(), ImapError> {
        self.require_writable()?;
        if uids.is_empty() {
            return Ok(());
        }
        let set = uid_set(uids);
        self.run(&[Arg::Raw("UID STORE"), Arg::Raw(&set), Arg::Raw("+FLAGS.SILENT (\\Deleted)")], "STORE").await?;
        if self.capabilities.has("UIDPLUS") {
            self.run(&[Arg::Raw("UID EXPUNGE"), Arg::Raw(&set)], "EXPUNGE").await?;
        } else {
            self.run(&[Arg::Raw("EXPUNGE")], "EXPUNGE").await?;
        }
        Ok(())
    }

    /// Stores `message` (raw MIME, CRLF line ends) in the folder at `path` with `flags`, and
    /// `\Draft` when `draft`. Returns where it landed when the server says (`APPENDUID`).
    /// [`ImapError::NotFound`] when the folder does not exist (`[TRYCREATE]`). Needs no open
    /// folder.
    pub async fn append(
        &mut self,
        path: &str,
        message: &[u8],
        flags: &[Flag],
        draft: bool,
    ) -> Result<Option<AppendUid>, ImapError> {
        let mut atoms: Vec<&str> = flags.iter().map(|&flag| flag_atom(flag)).collect();
        if draft {
            atoms.push("\\Draft");
        }
        let list = format!("({})", atoms.join(" "));
        let path = encode_mailbox(path);
        let mut args = vec![Arg::Raw("APPEND"), Arg::Str(&path)];
        if !atoms.is_empty() {
            args.push(Arg::Raw(&list));
        }
        args.push(Arg::Literal(message));
        let reply = self.wire.exchange(&args).await?;
        if reply.outcome.status != Status::Ok {
            return Err(refused(&reply.outcome, "APPEND"));
        }
        Ok(append_uid(&reply.outcome))
    }

    /// APPEND an archived EML without buffering it as one `Vec`, retaining its original
    /// INTERNALDATE and the given flags and keywords, which must all be transferable
    /// ([`backup_flag_transferable`]). `input` must provide exactly `bytes`; it is read in pieces
    /// of 64 KiB while they are sent. `path` goes out as [`encode_mailbox`] makes it. The error
    /// says whether the server may have stored the message anyway ([`AppendError`]).
    pub async fn append_backup<R: AsyncRead + Unpin + Send>(
        &mut self,
        path: &str,
        input: &mut R,
        bytes: u64,
        flags: &[String],
        internal_date: Option<DateTime<FixedOffset>>,
    ) -> Result<Option<AppendUid>, AppendError> {
        if flags.iter().any(|flag| !backup_flag_transferable(flag)) {
            return Err(AppendError::NotSent(ImapError::Protocol("archive holds a nontransferable IMAP flag".into())));
        }
        let date = internal_date.map(|date| date.format("%d-%b-%Y %H:%M:%S %z").to_string());
        let path = encode_mailbox(path);
        let reply = self.wire.append_stream(&path, input, bytes, flags, date.as_deref()).await?;
        if reply.outcome.status != Status::Ok {
            return Err(AppendError::Refused(refused(&reply.outcome, "APPEND")));
        }
        Ok(append_uid(&reply.outcome))
    }

    /// Waits up to `max` for the server to report a change in the open folder (`IDLE`, then
    /// `DONE`). On servers without IDLE it sleeps for `max` and asks with `NOOP`. The session is
    /// usable again afterwards. See [`Session`] for what dropping the future does.
    pub async fn idle(&mut self, max: Duration) -> Result<IdleEvent, ImapError> {
        self.require_selected()?;
        if !self.capabilities.has("IDLE") {
            tokio::time::sleep(max).await;
            let data = self.run(&[Arg::Raw("NOOP")], "NOOP").await?;
            return Ok(if data.iter().any(Response::is_change) { IdleEvent::Changed } else { IdleEvent::Timeout });
        }
        self.wire.idle(max).await
    }

    /// Asks the server whether it is still there (`NOOP`); keeps a quiet command connection open.
    pub async fn noop(&mut self) -> Result<(), ImapError> {
        self.run(&[Arg::Raw("NOOP")], "NOOP").await.map(drop)
    }

    /// Signs out (`LOGOUT`) and closes the connection.
    pub async fn logout(mut self) -> Result<(), ImapError> {
        self.run(&[Arg::Raw("LOGOUT")], "LOGOUT").await.map(drop)
    }

    fn condstore(&self) -> bool {
        self.capabilities.has("CONDSTORE")
    }

    fn require_selected(&self) -> Result<&Selected, ImapError> {
        self.selected.as_ref().ok_or_else(|| ImapError::Protocol("no folder is open (select one first)".into()))
    }

    /// The open folder, when it may be changed: an `EXAMINE`d one would refuse, or a server might
    /// ignore the change, so the session says so before sending anything.
    fn require_writable(&self) -> Result<&Selected, ImapError> {
        let selected = self.require_selected()?;
        if selected.read_only {
            return Err(ImapError::Protocol("the folder is open read-only (select it to change messages)".into()));
        }
        Ok(selected)
    }

    /// Runs a command and hands back its untagged responses; any answer but `OK` is an error.
    async fn run(&mut self, args: &[Arg<'_>], what: &str) -> Result<Vec<Response>, ImapError> {
        let reply = self.wire.exchange(args).await?;
        match reply.outcome.status {
            Status::Ok => Ok(reply.data),
            _ => Err(refused(&reply.outcome, what)),
        }
    }

    async fn refresh_capabilities(&mut self) -> Result<(), ImapError> {
        let data = self.run(&[Arg::Raw("CAPABILITY")], "CAPABILITY").await?;
        let words = data
            .iter()
            .filter_map(Response::data)
            .filter(|tokens| first_is(tokens, "CAPABILITY"))
            .flat_map(|tokens| tokens[1..].iter().filter_map(Token::atom));
        self.capabilities = Capabilities::from_words(words);
        Ok(())
    }

    async fn sign_in(&mut self, login: &str, password: &Secret) -> Result<(), ImapError> {
        let secret = password.expose();
        let plain = self.capabilities.has("AUTH=PLAIN");
        if !plain && self.capabilities.has("LOGINDISABLED") {
            return Err(ImapError::Protocol("the server allows no password sign-in on this connection".into()));
        }
        let blob = plain.then(|| base64(format!("\0{login}\0{secret}").as_bytes()));
        let secrets = [secret, blob.as_deref().unwrap_or_default()];
        let reply = match &blob {
            Some(blob) => self.wire.authenticate(blob).await,
            None => self.wire.exchange(&[Arg::Raw("LOGIN"), Arg::Str(login), Arg::Str(secret)]).await,
        }
        .map_err(|error| error.redacted(&secrets))?;
        if reply.outcome.status != Status::Ok {
            return Err(sign_in_error(&reply.outcome).redacted(&secrets));
        }
        match reply.outcome.code_name() {
            Some("CAPABILITY") => self.capabilities = Capabilities::from_words(reply.outcome.code_args().split(' ')),
            _ => self.refresh_capabilities().await?,
        }
        Ok(())
    }
}

/// The name TLS checks the certificate of `endpoint` against (and sends as SNI).
fn server_name(endpoint: &Endpoint) -> Result<ServerName<'static>, ImapError> {
    ServerName::try_from(endpoint.host.clone())
        .map_err(|_| ImapError::Protocol(format!("{:?} is not a host name", endpoint.host)))
}

/// The TLS settings for `trust` ([`client_config`]) and what their certificate check sees.
fn tls_config(trust: &Trust) -> Result<(Arc<ClientConfig>, Seen), ImapError> {
    let (config, seen) =
        client_config(trust).map_err(|error| ImapError::Protocol(format!("TLS could not be set up: {error}")))?;
    Ok((Arc::new(config), seen))
}

/// The TLS handshake. A certificate the check refused is [`ImapError::Untrusted`] with its
/// fingerprint (`seen`); any other certificate or protocol problem [`ImapError::Protocol`]
/// (trying again will not help), everything else [`ImapError::Offline`].
async fn handshake(
    config: Arc<ClientConfig>,
    server_name: ServerName<'static>,
    tcp: TcpStream,
    seen: &Seen,
) -> Result<TlsStream<TcpStream>, ImapError> {
    let outcome = tokio::time::timeout(CONNECT_TIMEOUT, TlsConnector::from(config).connect(server_name, tcp)).await;
    if outcome.as_ref().is_ok_and(Result::is_err)
        && let Some(fingerprint) = seen.rejected()
    {
        return Err(ImapError::Untrusted(fingerprint));
    }
    match outcome {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) if error.kind() == io::ErrorKind::InvalidData => {
            Err(ImapError::Protocol(format!("the secure connection failed: {error}")))
        }
        Ok(Err(error)) => Err(ImapError::Offline(format!("the secure connection failed: {error}"))),
        Err(_) => Err(ImapError::Offline("the secure connection was not set up in time".into())),
    }
}

/// `fut` with a time limit; I/O errors and the limit become [`ImapError::Offline`].
async fn timed<T>(wait: Duration, fut: impl Future<Output = io::Result<T>>) -> Result<T, ImapError> {
    match tokio::time::timeout(wait, fut).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(ImapError::Offline(error.to_string())),
        Err(_) => Err(ImapError::Offline("the server did not answer in time".into())),
    }
}

/// The error for a sign-in the server did not accept. Most servers say "wrong password" with a
/// bare `NO`; `ALERT`/`WEBALERT` only ask to show the text (Gmail's "application-specific
/// password required"), so they count as refused credentials too. Codes that name a problem of
/// the server or the connection do not.
fn sign_in_error(outcome: &Outcome) -> ImapError {
    let text = outcome.describe();
    if outcome.status != Status::No {
        return ImapError::Protocol(text);
    }
    match outcome.code_name() {
        None | Some("AUTHENTICATIONFAILED" | "AUTHORIZATIONFAILED" | "EXPIRED" | "ALERT" | "WEBALERT") => {
            ImapError::Auth(text)
        }
        Some("UNAVAILABLE") => ImapError::Offline(text),
        Some(_) => ImapError::Protocol(text),
    }
}

/// The error for a command the server answered with `NO` or `BAD`.
fn refused(outcome: &Outcome, what: &str) -> ImapError {
    let text = format!("{what}: {}", outcome.describe());
    match outcome.code_name() {
        Some("NONEXISTENT" | "TRYCREATE") => ImapError::NotFound(text),
        Some("UNAVAILABLE") => ImapError::Offline(text),
        _ => ImapError::Protocol(text),
    }
}

/// `text` with every non-empty secret replaced by `…`.
fn redact(text: &str, secrets: &[&str]) -> String {
    secrets.iter().filter(|secret| !secret.is_empty()).fold(text.to_owned(), |text, secret| text.replace(secret, "…"))
}

/// The attribute of a role in `LIST` and `CREATE … USE` (RFC 6154).
fn special_use(role: Role) -> &'static str {
    match role {
        Role::Inbox => "\\Inbox",
        Role::Drafts => "\\Drafts",
        Role::Sent => "\\Sent",
        Role::Archive => "\\Archive",
        Role::Junk => "\\Junk",
        Role::Trash => "\\Trash",
    }
}

fn flag_atom(flag: Flag) -> &'static str {
    match flag {
        Flag::Seen => "\\Seen",
        Flag::Flagged => "\\Flagged",
        Flag::Answered => "\\Answered",
    }
}

/// Flags safe to send as IMAP atoms in a restore. `\Recent` is server-owned and `\Deleted`
/// could expunge a restored message; unsupported system flags are reported and omitted.
pub fn backup_flag_transferable(flag: &str) -> bool {
    if flag.starts_with('\\') {
        return ["\\Seen", "\\Answered", "\\Flagged", "\\Draft"].iter().any(|system| flag.eq_ignore_ascii_case(system));
    }
    !flag.is_empty()
        && flag.bytes().all(|byte| {
            (0x21..=0x7e).contains(&byte) && !matches!(byte, b'(' | b')' | b'{' | b'%' | b'*' | b'"' | b'\\' | b']')
        })
}

/// The suite's flags in a `FLAGS (…)` list, and whether `\Deleted` is among them.
fn parse_flags(token: &Token) -> (Vec<Flag>, bool) {
    let mut flags = Vec::new();
    let mut deleted = false;
    for atom in token.list().unwrap_or_default().iter().filter_map(Token::atom) {
        match atom.to_ascii_lowercase().as_str() {
            "\\seen" => flags.push(Flag::Seen),
            "\\flagged" => flags.push(Flag::Flagged),
            "\\answered" => flags.push(Flag::Answered),
            "\\deleted" => deleted = true,
            _ => {}
        }
    }
    flags.sort_unstable();
    (flags, deleted)
}

/// `INTERNALDATE` (`17-Jul-1996 02:44:25 -0700`, the day possibly padded with a space).
fn internal_date(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_str(text.trim(), "%d-%b-%Y %H:%M:%S %z").ok().map(|date| date.with_timezone(&Utc))
}

/// A `BODYSTRUCTURE` names a part with the disposition `attachment` (`("attachment" (…))`);
/// inline parts do not count.
fn has_attachment(token: &Token) -> bool {
    let Some(items) = token.list() else { return false };
    let disposition = items.first().and_then(Token::text).is_some_and(|t| t.eq_ignore_ascii_case("attachment"))
        && items.get(1).is_some_and(|t| t.list().is_some() || *t == Token::Nil);
    disposition || items.iter().any(has_attachment)
}

/// UIDs as a sequence set: `[1, 2, 3, 5, 7, 8]` → `1:3,5,7:8`.
fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for uid in sorted {
        match ranges.last_mut() {
            Some((_, end)) if end.checked_add(1) == Some(uid) => *end = uid,
            _ => ranges.push((uid, uid)),
        }
    }
    ranges
        .iter()
        .map(|&(start, end)| if start == end { start.to_string() } else { format!("{start}:{end}") })
        .collect::<Vec<_>>()
        .join(",")
}

/// Standard base64 with padding, for `AUTHENTICATE PLAIN`.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The tokens of a `* LIST` / `* XLIST` line as folders: `(attributes) delimiter name`.
fn parse_list(data: &[Response], word: &str) -> Vec<ListedFolder> {
    data.iter()
        .filter_map(Response::data)
        .filter(|tokens| first_is(tokens, word))
        .filter_map(|tokens| {
            let attributes = tokens.get(1)?.list()?.iter().filter_map(Token::atom).map(str::to_owned).collect();
            let delimiter = tokens.get(2)?.text().and_then(|d| d.chars().next());
            let path = tokens.get(3)?.text()?;
            Some(ListedFolder { path, delimiter, attributes })
        })
        .collect()
}

/// The first personal namespace of a `* NAMESPACE ((prefix delimiter) …) … …` line.
fn personal_namespace(response: &Response) -> Option<Namespace> {
    let tokens = response.data().filter(|tokens| first_is(tokens, "NAMESPACE"))?;
    let first = tokens.get(1)?.list()?.first()?.list()?;
    let mut prefix = first.first()?.text()?;
    let delimiter = first.get(1).and_then(Token::text).and_then(|d| d.chars().next());
    if let Some(delimiter) = delimiter
        && !prefix.is_empty()
        && !prefix.ends_with(delimiter)
    {
        prefix.push(delimiter);
    }
    Some(Namespace { prefix, delimiter })
}

/// The resource `STORAGE` of a `* QUOTA root (name usage limit …)` line.
fn storage_quota(tokens: &[Token]) -> Option<Quota> {
    let resources = tokens.get(2)?.list()?;
    let storage = resources.chunks(3).find(|resource| resource[0].is("STORAGE"))?;
    Some(Quota { used_kib: storage.get(1)?.number()?, limit_kib: storage.get(2)?.number()? })
}

fn first_is(tokens: &[Token], word: &str) -> bool {
    tokens.first().is_some_and(|t| t.is(word))
}

/// One `* n FETCH (…)` response: its UID and the named items.
struct FetchItem<'a> {
    uid: u32,
    items: &'a [Token],
}

impl FetchItem<'_> {
    /// The value of the item `name` (`FLAGS`, `BODY[HEADER]`), any case.
    fn get(&self, name: &str) -> Option<&Token> {
        self.items.chunks(2).find(|pair| pair[0].is(name)).and_then(|pair| pair.get(1))
    }

    fn modseq(&self) -> Option<u64> {
        self.get("MODSEQ").and_then(Token::list).and_then(|list| list.first()).and_then(Token::number)
    }
}

/// The FETCH responses among `data` that carry a UID (unsolicited flag updates without one are
/// skipped).
fn fetched(data: &[Response]) -> impl Iterator<Item = FetchItem<'_>> {
    data.iter().filter_map(Response::data).filter_map(|tokens| {
        if !tokens.get(1)?.is("FETCH") {
            return None;
        }
        let items = tokens.get(2)?.list()?;
        let uid = items.chunks(2).find(|pair| pair[0].is("UID")).and_then(|pair| pair.get(1)?.number())?;
        Some(FetchItem { uid, items })
    })
}

// --- The wire: reading responses, sending commands --------------------------------------------

/// A value in a server response.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// An atom or number: `FETCH`, `\Seen`, `12`, `BODY[HEADER]`.
    Atom(String),
    /// A quoted string or a literal, as bytes.
    Str(Vec<u8>),
    Nil,
    List(Vec<Token>),
}

impl Token {
    fn atom(&self) -> Option<&str> {
        match self {
            Token::Atom(atom) => Some(atom),
            _ => None,
        }
    }

    /// The token is the atom `word`, any case.
    fn is(&self, word: &str) -> bool {
        self.atom().is_some_and(|atom| atom.eq_ignore_ascii_case(word))
    }

    /// An atom or string as text (invalid UTF-8 replaced).
    fn text(&self) -> Option<String> {
        match self {
            Token::Atom(atom) => Some(atom.clone()),
            Token::Str(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        }
    }

    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Token::Str(bytes) => Some(bytes),
            Token::Atom(atom) => Some(atom.as_bytes()),
            _ => None,
        }
    }

    fn number<T: std::str::FromStr>(&self) -> Option<T> {
        self.atom()?.parse().ok()
    }

    fn list(&self) -> Option<&[Token]> {
        match self {
            Token::List(items) => Some(items),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    No,
    Bad,
    Bye,
    Preauth,
}

impl Status {
    fn from_word(word: &[u8]) -> Option<Status> {
        match word.to_ascii_uppercase().as_slice() {
            b"OK" => Some(Status::Ok),
            b"NO" => Some(Status::No),
            b"BAD" => Some(Status::Bad),
            b"BYE" => Some(Status::Bye),
            b"PREAUTH" => Some(Status::Preauth),
            _ => None,
        }
    }
}

/// A status response: `OK [UIDVALIDITY 3857529045] UIDs valid`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Outcome {
    status: Status,
    /// The response code inside `[…]`, without the brackets: `UIDVALIDITY 3857529045`.
    code: Option<String>,
    /// The human text after the code.
    text: String,
}

impl Outcome {
    fn parse(status: Status, rest: &[u8]) -> Outcome {
        let rest = String::from_utf8_lossy(rest);
        let rest = rest.trim();
        if let Some(inner) = rest.strip_prefix('[')
            && let Some((code, text)) = inner.split_once(']')
        {
            return Outcome { status, code: Some(code.trim().to_owned()), text: text.trim().to_owned() };
        }
        Outcome { status, code: None, text: rest.to_owned() }
    }

    /// The code's name in capital letters: `UIDVALIDITY`.
    fn code_name(&self) -> Option<&str> {
        let name = self.code.as_deref()?.split(' ').next()?;
        // Codes are atoms; servers send them in capitals, so a lowercase one is not matched.
        Some(name)
    }

    /// What follows the code's name: `3857529045`.
    fn code_args(&self) -> &str {
        self.code.as_deref().and_then(|code| code.split_once(' ')).map_or("", |(_, args)| args.trim())
    }

    fn code_number<T: std::str::FromStr>(&self) -> Option<T> {
        self.code_args().split_whitespace().next()?.parse().ok()
    }

    /// The text for an error: the server's words, or the code when there are none.
    fn describe(&self) -> String {
        match (&self.code, self.text.is_empty()) {
            (Some(code), true) => format!("[{code}]"),
            (Some(code), false) => format!("[{code}] {}", self.text),
            (None, false) => self.text.clone(),
            (None, true) => format!("{:?} without a reason", self.status),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Response {
    /// `+ …`: go on with the literal or the authentication.
    Continue,
    Tagged {
        tag: String,
        outcome: Outcome,
    },
    /// `* OK/NO/BAD/BYE/PREAUTH …`.
    Status(Outcome),
    /// Any other untagged response, as tokens: `* 3 EXISTS`, `* LIST (…) "/" INBOX`.
    Data(Vec<Token>),
}

impl Response {
    fn data(&self) -> Option<&[Token]> {
        match self {
            Response::Data(tokens) => Some(tokens),
            _ => None,
        }
    }

    /// The response tells of new, removed or changed messages in the open folder.
    fn is_change(&self) -> bool {
        let Some(tokens) = self.data() else { return false };
        first_is(tokens, "VANISHED")
            || tokens.get(1).is_some_and(|t| t.is("EXISTS") || t.is("EXPUNGE") || t.is("FETCH") || t.is("RECENT"))
    }

    fn parse(raw: &[u8]) -> Result<Response, ImapError> {
        let raw = raw.strip_suffix(b"\r\n").or_else(|| raw.strip_suffix(b"\n")).unwrap_or(raw);
        if raw.first() == Some(&b'+') {
            return Ok(Response::Continue);
        }
        let (tag, rest) = split_word(raw);
        if tag.is_empty() {
            return Err(ImapError::Protocol("the server sent an empty line".into()));
        }
        let (word, after) = split_word(rest);
        let status = Status::from_word(word);
        if tag == b"*" {
            return match status {
                Some(status) => Ok(Response::Status(Outcome::parse(status, after))),
                None => Ok(Response::Data(Tokenizer::tokens(rest)?)),
            };
        }
        let status =
            status.ok_or_else(|| ImapError::Protocol("the server sent a response it does not explain".into()))?;
        Ok(Response::Tagged { tag: String::from_utf8_lossy(tag).into_owned(), outcome: Outcome::parse(status, after) })
    }
}

/// The text up to the first space, and what follows it.
fn split_word(bytes: &[u8]) -> (&[u8], &[u8]) {
    match bytes.iter().position(|&b| b == b' ') {
        Some(at) => (&bytes[..at], &bytes[at + 1..]),
        None => (bytes, &[]),
    }
}

/// Splits the data of a response into tokens; literals (`{n}\r\n` and n bytes) are inline.
struct Tokenizer<'a> {
    input: &'a [u8],
    at: usize,
}

impl Tokenizer<'_> {
    fn tokens(input: &[u8]) -> Result<Vec<Token>, ImapError> {
        Tokenizer { input, at: 0 }.sequence(false)
    }

    fn broken(what: &str) -> ImapError {
        ImapError::Protocol(format!("the server sent a broken response ({what})"))
    }

    fn sequence(&mut self, nested: bool) -> Result<Vec<Token>, ImapError> {
        let mut tokens = Vec::new();
        loop {
            while self.input.get(self.at) == Some(&b' ') {
                self.at += 1;
            }
            match self.input.get(self.at) {
                None if nested => return Err(Self::broken("an open list")),
                None => return Ok(tokens),
                Some(b')') if nested => {
                    self.at += 1;
                    return Ok(tokens);
                }
                Some(b')') => return Err(Self::broken("a list closed twice")),
                Some(b'(') => {
                    self.at += 1;
                    tokens.push(Token::List(self.sequence(true)?));
                }
                Some(b'"') => tokens.push(Token::Str(self.quoted()?)),
                Some(b'{') => tokens.push(Token::Str(self.literal()?)),
                Some(_) => tokens.push(self.atom()),
            }
        }
    }

    fn quoted(&mut self) -> Result<Vec<u8>, ImapError> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            match self.input.get(self.at) {
                None => return Err(Self::broken("an open quote")),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    let escaped = self.input.get(self.at + 1).ok_or_else(|| Self::broken("an open quote"))?;
                    out.push(*escaped);
                    self.at += 2;
                }
                Some(&byte) => {
                    out.push(byte);
                    self.at += 1;
                }
            }
        }
    }

    fn literal(&mut self) -> Result<Vec<u8>, ImapError> {
        let rest = &self.input[self.at + 1..];
        let close = rest.iter().position(|&b| b == b'}').ok_or_else(|| Self::broken("a literal"))?;
        let size: usize = std::str::from_utf8(&rest[..close])
            .ok()
            .and_then(|digits| digits.trim_end_matches('+').parse().ok())
            .ok_or_else(|| Self::broken("a literal size"))?;
        let start = self.at + 1 + close + 1;
        let start = match self.input.get(start..start + 2) {
            Some(b"\r\n") => start + 2,
            _ => match self.input.get(start) {
                Some(b'\n') => start + 1,
                _ => return Err(Self::broken("a literal")),
            },
        };
        let bytes = self.input.get(start..start + size).ok_or_else(|| Self::broken("a short literal"))?;
        self.at = start + size;
        Ok(bytes.to_vec())
    }

    /// Everything up to a space or parenthesis; `[…]` belongs to the atom even with spaces in it
    /// (`BODY[HEADER.FIELDS (DATE)]`).
    fn atom(&mut self) -> Token {
        let start = self.at;
        let mut in_brackets = false;
        while let Some(&byte) = self.input.get(self.at) {
            match byte {
                b'[' => in_brackets = true,
                b']' => in_brackets = false,
                b' ' | b'(' | b')' if !in_brackets => break,
                b'\r' | b'\n' => break,
                _ => {}
            }
            self.at += 1;
        }
        if self.at == start {
            // A stray byte such as a line end: skip it rather than loop.
            self.at += 1;
        }
        let atom = String::from_utf8_lossy(&self.input[start..self.at]).into_owned();
        if atom.eq_ignore_ascii_case("NIL") { Token::Nil } else { Token::Atom(atom) }
    }
}

/// The size of the literal a response line ends with: `… {42}\r\n` → 42.
fn literal_size(line: &[u8]) -> Option<usize> {
    let line = line.strip_suffix(b"\r\n").or_else(|| line.strip_suffix(b"\n")).unwrap_or(line);
    let inner = line.strip_suffix(b"}")?;
    let open = inner.iter().rposition(|&b| b == b'{')?;
    std::str::from_utf8(&inner[open + 1..]).ok()?.trim_end_matches('+').parse().ok()
}

/// A command argument.
#[derive(Debug, Clone, Copy)]
enum Arg<'a> {
    /// Sent as it is: keywords, sequence sets, parenthesized item lists.
    Raw(&'a str),
    /// A string: quoted when it is plain ASCII, a literal otherwise. Line breaks and NUL are
    /// refused before anything is sent.
    Str(&'a str),
    /// Bytes as a literal (a message to append).
    Literal(&'a [u8]),
}

/// One piece of the criteria of [`Session::uid_search_text`], owned until the command is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchPart {
    /// A keyword or a date (`TEXT`, `SINCE 1-Jan-2026`).
    Raw(String),
    /// A word to search for, sent as an IMAP string.
    Str(String),
}

/// The criteria of [`Session::uid_search_text`]: `CHARSET UTF-8` when a word is not plain ASCII,
/// `TEXT <word>` per word, then `SINCE` and `BEFORE`; `ALL` when that is nothing.
fn text_criteria(words: &str, since: Option<NaiveDate>, before: Option<NaiveDate>) -> Vec<SearchPart> {
    let words: Vec<String> = words
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect();
    let mut parts = Vec::new();
    if words.iter().any(|word| !word.is_ascii()) {
        parts.push(SearchPart::Raw("CHARSET UTF-8".into()));
    }
    for word in words {
        parts.push(SearchPart::Raw("TEXT".into()));
        parts.push(SearchPart::Str(word));
    }
    let day = |day: NaiveDate| day.format("%-d-%b-%Y").to_string();
    parts.extend(since.map(|since| SearchPart::Raw(format!("SINCE {}", day(since)))));
    parts.extend(before.map(|before| SearchPart::Raw(format!("BEFORE {}", day(before)))));
    if parts.is_empty() {
        parts.push(SearchPart::Raw("ALL".into()));
    }
    parts
}

/// What the server answered to one command.
#[derive(Debug)]
struct Reply {
    /// The untagged responses that came with it.
    data: Vec<Response>,
    outcome: Outcome,
}

/// What a greeting said.
struct Greeting {
    /// `* PREAUTH`: already signed in.
    preauth: bool,
    /// Capabilities from a `[CAPABILITY …]` code.
    capabilities: Option<Capabilities>,
}

/// One connection's reading and writing, over a plain or TLS stream.
struct Wire<S> {
    io: BufReader<S>,
    next_tag: u32,
    /// An exchange started and did not finish (its future was dropped, or it failed midway):
    /// where the server is in the conversation is unknown.
    busy: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> Wire<S> {
    fn new(stream: S, next_tag: u32) -> Wire<S> {
        Wire { io: BufReader::new(stream), next_tag, busy: false }
    }

    /// The stream for the TLS handshake after STARTTLS, and the next tag. Refused when the
    /// server already sent more: that data would be read as if it came over TLS.
    fn into_stream(self) -> Result<(S, u32), ImapError> {
        if !self.io.buffer().is_empty() {
            return Err(ImapError::Protocol("the server sent data before the connection was secured".into()));
        }
        Ok((self.io.into_inner(), self.next_tag))
    }

    async fn greeting(&mut self) -> Result<Greeting, ImapError> {
        match self.read_response().await? {
            Response::Status(outcome) if matches!(outcome.status, Status::Ok | Status::Preauth) => {
                let capabilities = (outcome.code_name() == Some("CAPABILITY"))
                    .then(|| Capabilities::from_words(outcome.code_args().split(' ')));
                Ok(Greeting { preauth: outcome.status == Status::Preauth, capabilities })
            }
            Response::Status(outcome) if outcome.status == Status::Bye => {
                Err(ImapError::Offline(format!("the server turned the connection away: {}", outcome.describe())))
            }
            _ => Err(ImapError::Protocol("the server did not greet as an IMAP server".into())),
        }
    }

    async fn starttls(&mut self) -> Result<(), ImapError> {
        let reply = self.exchange(&[Arg::Raw("STARTTLS")]).await?;
        match reply.outcome.status {
            Status::Ok => Ok(()),
            _ => Err(ImapError::Protocol(format!("the server does not offer STARTTLS: {}", reply.outcome.describe()))),
        }
    }

    fn start(&mut self) -> Result<String, ImapError> {
        if self.busy {
            return Err(ImapError::Offline("the connection was interrupted in the middle of a command".into()));
        }
        self.busy = true;
        let tag = format!("a{}", self.next_tag);
        self.next_tag += 1;
        Ok(tag)
    }

    /// Sends one command and reads up to its tagged answer. Literals wait for the server's `+`;
    /// a tagged answer instead ends the command early. `BYE` (except for `LOGOUT`) is
    /// [`ImapError::Offline`].
    async fn exchange(&mut self, args: &[Arg<'_>]) -> Result<Reply, ImapError> {
        for arg in args {
            if let Arg::Str(text) = arg
                && text.bytes().any(|b| matches!(b, b'\r' | b'\n' | 0))
            {
                return Err(ImapError::Protocol("a name or password holds a line break".into()));
            }
        }
        let logout = matches!(args.first(), Some(Arg::Raw("LOGOUT")));
        let tag = self.start()?;
        let mut data = Vec::new();
        let mut line = tag.clone().into_bytes();
        for arg in args {
            line.push(b' ');
            let literal = match arg {
                Arg::Raw(raw) => {
                    line.extend_from_slice(raw.as_bytes());
                    None
                }
                Arg::Str(text) if text.bytes().all(|b| (0x20..0x7f).contains(&b)) => {
                    line.push(b'"');
                    for byte in text.bytes() {
                        if matches!(byte, b'"' | b'\\') {
                            line.push(b'\\');
                        }
                        line.push(byte);
                    }
                    line.push(b'"');
                    None
                }
                Arg::Str(text) => Some(text.as_bytes()),
                Arg::Literal(bytes) => Some(*bytes),
            };
            if let Some(bytes) = literal {
                line.extend_from_slice(format!("{{{}}}\r\n", bytes.len()).as_bytes());
                self.write(&line).await?;
                line.clear();
                if let Some(outcome) = self.await_continue(&tag, &mut data).await? {
                    self.busy = false;
                    return Ok(Reply { data, outcome });
                }
                self.write(bytes).await?;
            }
        }
        line.extend_from_slice(b"\r\n");
        self.write(&line).await?;
        let outcome = self.finish(&tag, &mut data, logout).await?;
        Ok(Reply { data, outcome })
    }

    /// The restore variant of APPEND: writes the literal from an async source in bounded blocks.
    /// A failure before the last block is written means the server cannot have stored the message
    /// ([`AppendError::NotSent`]): that block never left. From writing the last block on only a
    /// tagged answer tells ([`AppendError::Unconfirmed`]): a write can fail after its bytes left
    /// (a flush error), so a failure there is never taken as proof that nothing arrived. A tagged
    /// `NO`/`BAD` comes back as the reply.
    async fn append_stream<R: AsyncRead + Unpin + Send>(
        &mut self,
        path: &str,
        input: &mut R,
        bytes: u64,
        flags: &[String],
        internal_date: Option<&str>,
    ) -> Result<Reply, AppendError> {
        if path.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)) {
            return Err(AppendError::NotSent(ImapError::Protocol("a folder name holds a line break".into())));
        }
        let tag = self.start().map_err(AppendError::NotSent)?;
        let sent = self.append_head(&tag, path, flags, internal_date, bytes).await.map_err(AppendError::NotSent)?;
        let mut data = match sent {
            Ok(data) => data,
            Err(reply) => return Ok(reply),
        };
        let mut left = bytes;
        let mut chunk = vec![0u8; LITERAL_CHUNK];
        while left > 0 {
            let count = left.min(LITERAL_CHUNK as u64) as usize;
            input
                .read_exact(&mut chunk[..count])
                .await
                .map_err(|_| AppendError::NotSent(ImapError::Offline("restore source ended during APPEND".into())))?;
            let last = left == count as u64;
            self.write(&chunk[..count])
                .await
                .map_err(|error| if last { AppendError::Unconfirmed(error) } else { AppendError::NotSent(error) })?;
            left -= count as u64;
        }
        self.write(b"\r\n").await.map_err(AppendError::Unconfirmed)?;
        let outcome = self.finish(&tag, &mut data, false).await.map_err(AppendError::Unconfirmed)?;
        Ok(Reply { data, outcome })
    }

    /// The command line of [`Wire::append_stream`] up to the message literal, and the server's `+`
    /// for it: `Ok(data)` with the untagged responses so far, or `Err(reply)` when the server
    /// answered the command (tagged) instead.
    async fn append_head(
        &mut self,
        tag: &str,
        path: &str,
        flags: &[String],
        internal_date: Option<&str>,
        bytes: u64,
    ) -> Result<Result<Vec<Response>, Reply>, ImapError> {
        let mut data = Vec::new();
        let mut line = format!("{tag} APPEND ").into_bytes();
        if path.bytes().all(|byte| (0x20..0x7f).contains(&byte)) {
            line.push(b'"');
            for byte in path.bytes() {
                if matches!(byte, b'"' | b'\\') {
                    line.push(b'\\');
                }
                line.push(byte);
            }
            line.push(b'"');
        } else {
            line.extend_from_slice(format!("{{{}}}\r\n", path.len()).as_bytes());
            self.write(&line).await?;
            line.clear();
            if let Some(outcome) = self.await_continue(tag, &mut data).await? {
                self.busy = false;
                return Ok(Err(Reply { data, outcome }));
            }
            self.write(path.as_bytes()).await?;
        }
        if !flags.is_empty() {
            line.extend_from_slice(format!(" ({})", flags.join(" ")).as_bytes());
        }
        if let Some(date) = internal_date {
            line.extend_from_slice(format!(" \"{date}\"").as_bytes());
        }
        line.extend_from_slice(format!(" {{{bytes}}}\r\n").as_bytes());
        self.write(&line).await?;
        if let Some(outcome) = self.await_continue(tag, &mut data).await? {
            self.busy = false;
            return Ok(Err(Reply { data, outcome }));
        }
        Ok(Ok(data))
    }

    /// `AUTHENTICATE PLAIN` with the base64 `blob`; a further challenge is cancelled with `*`.
    async fn authenticate(&mut self, blob: &str) -> Result<Reply, ImapError> {
        let tag = self.start()?;
        let mut data = Vec::new();
        self.write(format!("{tag} AUTHENTICATE PLAIN\r\n").as_bytes()).await?;
        if let Some(outcome) = self.await_continue(&tag, &mut data).await? {
            self.busy = false;
            return Ok(Reply { data, outcome });
        }
        self.write(format!("{blob}\r\n").as_bytes()).await?;
        loop {
            match self.read_response().await? {
                Response::Continue => self.write(b"*\r\n").await?,
                Response::Tagged { tag: answered, outcome } if answered == tag => {
                    self.busy = false;
                    return Ok(Reply { data, outcome });
                }
                Response::Status(outcome) if outcome.status == Status::Bye => return Err(bye(&outcome)),
                other => data.push(other),
            }
        }
    }

    /// `IDLE` until a change, `max`, or an error; then `DONE`.
    async fn idle(&mut self, max: Duration) -> Result<IdleEvent, ImapError> {
        let tag = self.start()?;
        let mut data = Vec::new();
        self.write(format!("{tag} IDLE\r\n").as_bytes()).await?;
        if let Some(outcome) = self.await_continue(&tag, &mut data).await? {
            self.busy = false;
            return Err(refused(&outcome, "IDLE"));
        }
        let deadline = Instant::now() + max;
        let mut changed = data.iter().any(Response::is_change);
        while !changed {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match self.read_response_within(left).await? {
                None => break,
                Some(Response::Status(outcome)) if outcome.status == Status::Bye => return Err(bye(&outcome)),
                Some(response) => changed = response.is_change(),
            }
        }
        self.write(b"DONE\r\n").await?;
        let outcome = self.finish(&tag, &mut data, false).await?;
        if outcome.status != Status::Ok {
            return Err(refused(&outcome, "IDLE"));
        }
        Ok(if changed { IdleEvent::Changed } else { IdleEvent::Timeout })
    }

    /// Reads until the server's `+`. `Some` when it answered the command (tagged) instead.
    async fn await_continue(&mut self, tag: &str, data: &mut Vec<Response>) -> Result<Option<Outcome>, ImapError> {
        loop {
            match self.read_response().await? {
                Response::Continue => return Ok(None),
                Response::Tagged { tag: answered, outcome } if answered == tag => return Ok(Some(outcome)),
                Response::Status(outcome) if outcome.status == Status::Bye => return Err(bye(&outcome)),
                other => data.push(other),
            }
        }
    }

    /// Reads until the tagged answer to `tag`, keeping the untagged responses in `data`.
    async fn finish(&mut self, tag: &str, data: &mut Vec<Response>, logout: bool) -> Result<Outcome, ImapError> {
        loop {
            match self.read_response().await? {
                Response::Tagged { tag: answered, outcome } if answered == tag => {
                    self.busy = false;
                    return Ok(outcome);
                }
                Response::Status(outcome) if outcome.status == Status::Bye && !logout => return Err(bye(&outcome)),
                other => data.push(other),
            }
        }
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<(), ImapError> {
        let io = &mut self.io;
        timed(READ_TIMEOUT, async move {
            io.write_all(bytes).await?;
            io.flush().await
        })
        .await
    }

    async fn read_response(&mut self) -> Result<Response, ImapError> {
        self.read_response_within(READ_TIMEOUT)
            .await?
            .ok_or_else(|| ImapError::Offline("the server did not answer in time".into()))
    }

    /// The next response; `None` when nothing at all arrived within `wait` (the stream is then
    /// untouched). Once a response has begun, the rest of it gets [`READ_TIMEOUT`] per read.
    async fn read_response_within(&mut self, wait: Duration) -> Result<Option<Response>, ImapError> {
        match tokio::time::timeout(wait, self.io.fill_buf()).await {
            Err(_) => return Ok(None),
            Ok(Err(error)) => return Err(ImapError::Offline(error.to_string())),
            Ok(Ok(_)) => {}
        }
        let mut raw = Vec::new();
        loop {
            let start = raw.len();
            self.read_line(&mut raw).await?;
            let Some(size) = literal_size(&raw[start..]) else { break };
            if size > MAX_LITERAL {
                return Err(ImapError::Protocol("the server sent a literal too large to take".into()));
            }
            self.read_exact(&mut raw, size).await?;
        }
        Response::parse(&raw).map(Some)
    }

    /// Appends one line, line end included, to `out`.
    async fn read_line(&mut self, out: &mut Vec<u8>) -> Result<(), ImapError> {
        let mut taken = 0;
        loop {
            let (used, done) = {
                let buffer = timed(READ_TIMEOUT, self.io.fill_buf()).await?;
                if buffer.is_empty() {
                    return Err(ImapError::Offline("the server closed the connection".into()));
                }
                match buffer.iter().position(|&b| b == b'\n') {
                    Some(end) => {
                        out.extend_from_slice(&buffer[..=end]);
                        (end + 1, true)
                    }
                    None => {
                        out.extend_from_slice(buffer);
                        (buffer.len(), false)
                    }
                }
            };
            self.io.consume(used);
            taken += used;
            if done {
                return Ok(());
            }
            if taken > MAX_LINE {
                return Err(ImapError::Protocol("the server sent a line too long to take".into()));
            }
        }
    }

    /// Appends exactly `size` bytes to `out`.
    async fn read_exact(&mut self, out: &mut Vec<u8>, size: usize) -> Result<(), ImapError> {
        let mut left = size;
        while left > 0 {
            let start = out.len();
            let piece = left.min(LITERAL_CHUNK);
            out.resize(start + piece, 0);
            timed(READ_TIMEOUT, self.io.read_exact(&mut out[start..])).await?;
            left -= piece;
        }
        Ok(())
    }
}

fn bye(outcome: &Outcome) -> ImapError {
    ImapError::Offline(format!("the server closed the connection: {}", outcome.describe()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(text: &str) -> Token {
        Token::Atom(text.into())
    }

    #[test]
    fn responses_split_into_tokens_with_literals_quotes_and_brackets() {
        let raw = b"* 12 FETCH (UID 7 FLAGS (\\Seen $Junk) BODY[HEADER] {9}\r\nSubject:\n MODSEQ (42) X \"a \\\"q\\\"\" NIL)\r\n";
        let Response::Data(tokens) = Response::parse(raw).expect("parsed") else { panic!("data") };
        assert_eq!(tokens[..2], [atom("12"), atom("FETCH")]);
        let items = tokens[2].list().expect("list");
        assert_eq!(items[0..2], [atom("UID"), atom("7")]);
        assert_eq!(items[3], Token::List(vec![atom("\\Seen"), atom("$Junk")]));
        assert_eq!(items[4], atom("BODY[HEADER]"));
        assert_eq!(items[5], Token::Str(b"Subject:\n".to_vec()));
        assert_eq!(items[7], Token::List(vec![atom("42")]));
        assert_eq!(items[9], Token::Str(b"a \"q\"".to_vec()));
        assert_eq!(items[10], Token::Nil);

        let Response::Data(tokens) =
            Response::parse(b"* 1 FETCH (BODY[HEADER.FIELDS (DATE FROM)] \"x\")\r\n").expect("parsed")
        else {
            panic!("data")
        };
        assert_eq!(tokens[2].list().expect("list")[0], atom("BODY[HEADER.FIELDS (DATE FROM)]"));
        assert!(Response::parse(b"* LIST (\\Noselect \"/\" x\r\n").is_err(), "open list");
    }

    #[test]
    fn status_responses_keep_their_code_and_text() {
        let response = Response::parse(b"* OK [UIDVALIDITY 3857529045] UIDs valid\r\n").expect("parsed");
        let Response::Status(outcome) = response else { panic!("status") };
        assert_eq!((outcome.code_name(), outcome.code_number::<u32>()), (Some("UIDVALIDITY"), Some(3_857_529_045)));
        assert_eq!(outcome.text, "UIDs valid");

        let response = Response::parse(b"a3 NO [AUTHENTICATIONFAILED] Invalid credentials\r\n").expect("parsed");
        let Response::Tagged { tag, outcome } = response else { panic!("tagged") };
        assert_eq!((tag.as_str(), outcome.status), ("a3", Status::No));
        assert_eq!(outcome.describe(), "[AUTHENTICATIONFAILED] Invalid credentials");
        assert!(matches!(sign_in_error(&outcome), ImapError::Auth(_)));
        let bare = Outcome::parse(Status::No, b"LOGIN failed");
        assert!(matches!(sign_in_error(&bare), ImapError::Auth(_)), "a bare NO is a wrong password");
        let busy = Outcome::parse(Status::No, b"[UNAVAILABLE] try later");
        assert!(matches!(sign_in_error(&busy), ImapError::Offline(_)));
        let bad = Outcome::parse(Status::Bad, b"syntax");
        assert!(matches!(sign_in_error(&bad), ImapError::Protocol(_)));
        assert_eq!(Response::parse(b"+ go ahead\r\n").expect("parsed"), Response::Continue);
        assert!(Response::parse(b"a1 MAYBE\r\n").is_err());
    }

    #[test]
    fn literal_sizes_are_read_from_line_ends() {
        assert_eq!(literal_size(b"* 1 FETCH (BODY[] {120}\r\n"), Some(120));
        assert_eq!(literal_size(b"* LIST () \"/\" {5+}\r\n"), Some(5));
        assert_eq!(literal_size(b"* OK done\r\n"), None);
        assert_eq!(literal_size(b"* OK {x}\r\n"), None);
    }

    #[test]
    fn text_searches_send_each_word_as_a_string_and_days_as_dates() {
        let raw = |text: &str| SearchPart::Raw(text.into());
        let word = |text: &str| SearchPart::Str(text.into());
        let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d);
        assert_eq!(
            text_criteria("  invoice  2025 ", day(2025, 3, 7), day(2026, 1, 15)),
            [
                raw("TEXT"),
                word("invoice"),
                raw("TEXT"),
                word("2025"),
                raw("SINCE 7-Mar-2025"),
                raw("BEFORE 15-Jan-2026")
            ]
        );
        assert_eq!(text_criteria("Müller", None, None), [raw("CHARSET UTF-8"), raw("TEXT"), word("Müller")]);
        assert_eq!(text_criteria("a\u{0}b \u{7}", None, None), [raw("TEXT"), word("ab")], "no control characters");
        assert_eq!(text_criteria(" ", None, None), [raw("ALL")]);
        assert_eq!(text_criteria("", None, day(2026, 1, 1)), [raw("BEFORE 1-Jan-2026")]);
    }

    #[test]
    fn uid_sets_are_compressed_into_ranges() {
        assert_eq!(uid_set(&[8, 1, 2, 3, 5, 7, 3]), "1:3,5,7:8");
        assert_eq!(uid_set(&[42]), "42");
        assert_eq!(uid_set(&[u32::MAX, u32::MAX - 1]), format!("{}:{}", u32::MAX - 1, u32::MAX));
    }

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        let vectors =
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE=")];
        for (plain, encoded) in vectors {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
        assert_eq!(base64(b"\0anna\0p\xc3\xa4ss"), "AGFubmEAcMOkc3M=");
    }

    #[test]
    fn secrets_are_blanked_and_never_debug_printed() {
        let error = ImapError::Auth("LOGIN \"anna\" \"hunter2\" refused".into()).redacted(&["hunter2", ""]);
        assert_eq!(error, ImapError::Auth("LOGIN \"anna\" \"…\" refused".into()));
        assert!(!format!("{error} {error:?}").contains("hunter2"));
    }

    #[test]
    fn flags_dates_and_attachments_are_read_from_fetch_items() {
        let flags = Token::List(vec![atom("\\Seen"), atom("\\Deleted"), atom("\\FLAGGED"), atom("$Label1")]);
        assert_eq!(parse_flags(&flags), (vec![Flag::Seen, Flag::Flagged], true));
        let date = internal_date(" 7-Jul-2026 02:44:25 -0700").expect("a date");
        assert_eq!(date.to_rfc3339(), "2026-07-07T09:44:25+00:00");

        let Response::Data(tokens) = Response::parse(
            b"* 1 FETCH (BODYSTRUCTURE ((\"text\" \"plain\" (\"charset\" \"utf-8\") NIL NIL \"7bit\" 5 1 NIL NIL NIL)(\"application\" \"pdf\" (\"name\" \"a.pdf\") NIL NIL \"base64\" 100 NIL (\"attachment\" (\"filename\" \"a.pdf\")) NIL) \"mixed\"))\r\n",
        )
        .expect("parsed") else {
            panic!("data")
        };
        let structure = &tokens[2].list().expect("items")[1];
        assert!(has_attachment(structure));
        let Response::Data(tokens) = Response::parse(
            b"* 1 FETCH (BODYSTRUCTURE (\"image\" \"png\" NIL NIL NIL \"base64\" 10 NIL (\"inline\" NIL) NIL))\r\n",
        )
        .expect("parsed") else {
            panic!("data")
        };
        assert!(!has_attachment(&tokens[2].list().expect("items")[1]), "inline images do not count");
    }

    /// A connection that answers `script` and takes `limit` bytes; the write that would go beyond
    /// them hands over what fits and then fails, as a connection that breaks midway.
    struct Breaking {
        script: io::Cursor<Vec<u8>>,
        written: usize,
        limit: usize,
    }

    impl AsyncRead for Breaking {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            let this = &mut *self;
            let start = usize::try_from(this.script.position()).unwrap_or(usize::MAX);
            let rest = this.script.get_ref().get(start..).unwrap_or_default();
            let count = rest.len().min(buf.remaining());
            buf.put_slice(&rest[..count]);
            this.script.set_position((start + count) as u64);
            std::task::Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for Breaking {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<io::Result<usize>> {
            let room = self.limit - self.written;
            if room == 0 {
                return std::task::Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
            }
            let count = room.min(buf.len());
            self.written += count;
            std::task::Poll::Ready(Ok(count))
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn an_append_that_breaks_in_its_last_block_may_have_arrived() {
        let bytes = LITERAL_CHUNK + 10;
        let head = format!("a1 APPEND \"INBOX\" {{{bytes}}}\r\n").len();
        let message = vec![b'x'; bytes];
        let append = |limit: usize| {
            let message = message.clone();
            async move {
                let stream = Breaking { script: io::Cursor::new(b"+ go\r\n".to_vec()), written: 0, limit };
                let mut wire = Wire::new(stream, 1);
                let mut input = io::Cursor::new(message);
                wire.append_stream("INBOX", &mut input, bytes as u64, &[], None).await.map(|_| ())
            }
        };
        // The first of two blocks breaks: its rest and the whole second block never left.
        let early = append(head + 3).await;
        assert!(matches!(early, Err(AppendError::NotSent(_))), "{early:?}");
        // The last block breaks: all of it may have left before the failure was noticed.
        let late = append(head + LITERAL_CHUNK + 3).await;
        assert!(matches!(late, Err(AppendError::Unconfirmed(_))), "{late:?}");
        // Only the closing line break is missing: the message is out.
        let end = append(head + bytes).await;
        assert!(matches!(end, Err(AppendError::Unconfirmed(_))), "{end:?}");
    }

    #[test]
    fn capabilities_are_compared_in_any_case() {
        let capabilities = Capabilities::from_words("IMAP4rev1 auth=PLAIN Move ".split(' '));
        assert!(capabilities.has("MOVE") && capabilities.has("Auth=Plain") && !capabilities.has("IDLE"));
        assert_eq!(capabilities.iter().collect::<Vec<_>>(), ["AUTH=PLAIN", "IMAP4REV1", "MOVE"]);
    }
}
