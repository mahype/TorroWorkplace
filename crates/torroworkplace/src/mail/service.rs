//! The mail service of the core (task M2.6): keeps every mail account of `accounts.json` in sync
//! with its IMAP server, one account apart from the other, and hands out the account's command
//! connection and store to the mail commands (tasks M3.1 to M3.3).
//!
//! **Per account** ([`MailService`] keys them by account id, like the calendar service):
//!
//! - a worker task with its own IMAP session that waits in `IDLE` on the inbox and syncs it as
//!   soon as the server reports a change (push);
//! - a second session, the **command connection** ([`MailService::command`]): the sync of all
//!   folders runs on it every five minutes ([`sync`](super::sync)), and so do the mail commands;
//!   the sync takes it in small steps, so a command never waits for a whole folder;
//! - its store ([`SharedStore`]): one SQLite file, see [`super::store`];
//! - an indexer task (issue #136) that, after the first sync, fetches the texts of older messages
//!   into the search index, newest first, on the command connection between commands
//!   ([`sync::backfill`]); `mail.index_status` reads how far it is ([`MailService::index_progress`]).
//!
//! **Failures** stay with their account: a server that cannot be reached is tried again after
//! one minute, then two, four, eight and every 15 minutes; a refused password is shown at the
//! account ("Password was refused", in `accounts.list_accounts` and as an entry of the suite in
//! the inbox, task E2.1) and **not** tried again until the account is connected anew
//! ([`MailService::reload`] after `accounts.connect_mail`, task M2.7), so a server never locks
//! the account for too many wrong sign-ins. An account whose mail is switched off
//! (`MailSettings::services.mail`) is listed, never synced.
//!
//! **Changes** ([`MailChange`]): every folder sync that stored new messages, saw flags change on
//! the server or found messages gone sends one to [`MailService::subscribe`]rs: the inbox (task
//! E4.1) and change notices for the apps.
//!
//! **Connecting** (task M2.7, `accounts.connect_mail` in the core): [`MailService::check_sign_in`]
//! signs in to IMAP (and lists the folders, for the roles the server leaves open) and to SMTP
//! without sending anything, before anything is stored; [`MailService::save_account`] keeps the password
//! in the keychain and the account in `accounts.json`, [`MailService::restart`] starts it.
//! Changing settings ([`MailService::update`]) never touches servers or password, and removing an
//! account ([`MailService::remove`]) deletes its password, its entry and its store. A mail account
//! that syncs calendars too has one check for the inbox: its mail part and its calendar part from
//! the calendar service joined ([`MailService::account_checks`]).
//!
//! **Security.** The password comes from the keychain and goes to the IMAP and SMTP servers only,
//! never into an answer, a log or a `Debug` output. Before every connection the host must resolve
//! to public addresses only (the guard of the CalDAV and mail discovery, `is_public`), so an
//! account cannot be used to probe the local network; the connection then goes to exactly the
//! addresses checked, the host name serving TLS alone (task M2.7b), so a name that answers
//! differently the second time (DNS rebinding) reaches nothing new. Rights are not checked here: the core checks
//! a client's reach (`Rights::reaches_mail_account`) before it asks this service.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, Notify, OwnedMutexGuard, RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use torro_core::Account;
use torro_core::account::{self, AccountKind, Secret};
use torro_core::i18n::{tr, trf};
use torro_ipc::mail::{
    IndexState, MailCacheAction, MailCacheEstimate, MailCacheJob, MailCacheJobState, MailCacheMode, MailCachePolicy,
    MailIndexProgress,
};
use torro_ipc::{AccountInfo, FolderChoice, MailView, OpenRoleView, Response, ServiceState};
use torro_mail::{Endpoint, MailAccountConfig, MailSettings, Role};
use torro_mail_imap::folders::{self, FolderList, OpenRole};
use torro_mail_imap::imap::{IdleEvent, ImapError, Session, Trust};
use torro_mail_imap::smtp::{SmtpError, SmtpSubmitter};
use torro_pim::{AccountState, ProviderError};
use torro_pim_caldav::discover::is_public;

use super::send::Outbox;
use super::store::MailStore;
use super::{AttachmentCache, MailDirs, MailStoreError, send, sync};
use crate::account::github::{SystemTokens, TokenStore};
use crate::account::keychain;
use crate::inbox::{AccountCheck, Health, Trouble};
use crate::pim::{NoKeychain, PimService, Reach, SYNC_PROBLEM_AFTER};

/// How many changes wait for a slow subscriber before it misses some ([`MailService::subscribe`]).
const EVENTS: usize = 256;

/// How often fetched attachments older than a day are deleted.
const PURGE_EVERY: Duration = Duration::from_secs(60 * 60);

/// When the sync runs and how patiently it connects again.
#[derive(Debug, Clone, Copy)]
struct Timing {
    /// All folders are synced this often (CONDSTORE or comparing UIDs).
    poll: Duration,
    /// The longest single `IDLE`, below the 29 minutes after which servers may drop an idle client.
    idle_max: Duration,
    /// The first wait after a failure; it doubles up to `retry_max`.
    retry_first: Duration,
    retry_max: Duration,
    /// Run the indexer task, which fetches older texts into the search index (issue #136). Tests
    /// run it by hand ([`MailService::backfill_now`]), so it never fetches behind their backs.
    index: bool,
}

/// The core's timing (task M2.6): all folders every 5 minutes, back off up to 15 minutes.
const SYSTEM_TIMING: Timing = Timing {
    poll: Duration::from_secs(5 * 60),
    idle_max: Duration::from_secs(25 * 60),
    retry_first: Duration::from_secs(60),
    retry_max: Duration::from_secs(15 * 60),
    index: true,
};

/// Why a mail account could not be synced or a command could not run. The texts for people
/// ([`MailError::message`]) never hold the password.
#[derive(Debug)]
pub enum MailError {
    /// No mail account with this id, or its mail is switched off (`services.mail`).
    NotFound,
    /// No password is stored for the account on this device.
    NoPassword,
    /// The server refused the password, now or earlier: nothing is tried until the account is
    /// connected again.
    Auth,
    /// The server cannot be reached, or the connection dropped. Tried again later.
    Offline(String),
    /// The IMAP host resolves to an address that is not on the public internet (loopback, a
    /// private or link-local network); nothing was sent to it.
    NotPublic,
    /// The server answered something unexpected or refused a command (also a folder or message
    /// that does not exist: [`ImapError::NotFound`]).
    Server(ImapError),
    /// The account's store on this computer failed.
    Store(MailStoreError),
}

impl MailError {
    /// The API error code (`docs/api.md`): `not-found`, `auth`, `offline` or `error`.
    pub fn code(&self) -> &'static str {
        match self {
            MailError::NotFound | MailError::Server(ImapError::NotFound(_)) => "not-found",
            MailError::NoPassword | MailError::Auth => "auth",
            MailError::Offline(_) => "offline",
            MailError::NotPublic | MailError::Server(_) | MailError::Store(_) => "error",
        }
    }

    /// What happened, for people, in their language where the core has the words.
    pub fn message(&self) -> String {
        match self {
            MailError::NotFound => tr("No such mail account.").into(),
            MailError::NoPassword => tr("Sign in again: no password is stored on this device").into(),
            MailError::Auth => tr("Password was refused").into(),
            MailError::Offline(_) => tr("The server cannot be reached.").into(),
            MailError::NotPublic => tr("The mail server is not on the public internet.").into(),
            // Server words and folder names stay in the core's log ([`fmt::Display`]): they come
            // from outside and may reach an agent.
            MailError::Server(ImapError::NotFound(_)) => {
                tr("The mail server does not have this folder or message.").into()
            }
            MailError::Server(_) => tr("The mail server answered unexpectedly.").into(),
            MailError::Store(_) => tr("Mail cannot be kept on this device.").into(),
        }
    }

    /// The answer to a request that failed with this error.
    pub fn response(&self) -> Response {
        Response::Error { code: self.code().into(), message: self.message() }
    }

    /// What the inbox makes of it (task E2.1).
    fn trouble(&self) -> Trouble {
        match self {
            MailError::Auth => Trouble::SignInRefused,
            MailError::NoPassword => Trouble::SignInNeeded,
            _ => Trouble::SyncFailing,
        }
    }
}

impl fmt::Display for MailError {
    /// For the core's log: English, with the server's words where there are any.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MailError::NotFound => f.write_str("no such mail account"),
            MailError::NoPassword => f.write_str("no password stored"),
            MailError::Auth => f.write_str("the password was refused"),
            MailError::Offline(why) => write!(f, "offline: {why}"),
            MailError::NotPublic => f.write_str("the IMAP host is not on the public internet"),
            MailError::Server(err) => err.fmt(f),
            MailError::Store(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for MailError {}

impl From<ImapError> for MailError {
    fn from(err: ImapError) -> MailError {
        match err {
            // The server's words may name the login; the account shows its own text.
            ImapError::Auth(_) => MailError::Auth,
            ImapError::Offline(why) => MailError::Offline(why),
            other => MailError::Server(other),
        }
    }
}

impl From<MailStoreError> for MailError {
    fn from(err: MailStoreError) -> MailError {
        MailError::Store(err)
    }
}

/// A server of a mail account's sign-in, named when its check fails (task M2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Imap,
    Smtp,
    CalDav,
}

impl Service {
    /// How people know it: `IMAP`, `SMTP`, `CalDAV` (the same in every language).
    pub fn name(self) -> &'static str {
        match self {
            Service::Imap => "IMAP",
            Service::Smtp => "SMTP",
            Service::CalDav => "CalDAV",
        }
    }
}

/// Why checking the sign-in of a mail account failed (`accounts.connect_mail`, task M2.7). Names
/// the server that failed, never with the server's words or the password: a server may echo the
/// login, and the answer may reach a log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignInError {
    /// The server refused the password (or the login name).
    Refused(Service),
    /// The server cannot be reached.
    Offline(Service),
    /// The host resolves to an address that is not on the public internet; nothing was sent.
    NotPublic(Service),
    /// The server lacks what a secure sign-in needs (SMTP without STARTTLS or a login method).
    Unsupported(Service),
    /// No CalDAV answers at the CalDAV address, or none was given for calendars or tasks.
    NoCalendars,
    /// Anything else: an unexpected answer, a failed TLS handshake, an address that is no URL.
    Failed(Service),
}

impl SignInError {
    /// What a failed IMAP sign-in or folder listing means.
    fn imap(err: ImapError) -> SignInError {
        match err {
            ImapError::Auth(_) => SignInError::Refused(Service::Imap),
            ImapError::Offline(_) => SignInError::Offline(Service::Imap),
            ImapError::Protocol(_) | ImapError::NotFound(_) => SignInError::Failed(Service::Imap),
        }
    }

    /// What a failed check of a host of `service` ([`MailService::check_public`]) means.
    fn host(service: Service, err: MailError) -> SignInError {
        match err {
            MailError::NotPublic => SignInError::NotPublic(service),
            MailError::Offline(_) => SignInError::Offline(service),
            _ => SignInError::Failed(service),
        }
    }

    /// What a failed SMTP sign-in means.
    fn smtp(err: SmtpError) -> SignInError {
        match err {
            SmtpError::Auth => SignInError::Refused(Service::Smtp),
            SmtpError::Offline => SignInError::Offline(Service::Smtp),
            SmtpError::Unsupported => SignInError::Unsupported(Service::Smtp),
            SmtpError::Tls | SmtpError::Rejected { .. } | SmtpError::InvalidEnvelope | SmtpError::Protocol => {
                SignInError::Failed(Service::Smtp)
            }
        }
    }

    /// What a failed CalDAV sign-in (`CalDavClient::services`, `Core::check_caldav`) means.
    pub fn caldav(err: &ProviderError) -> SignInError {
        match err {
            ProviderError::Auth => SignInError::Refused(Service::CalDav),
            ProviderError::Offline => SignInError::Offline(Service::CalDav),
            _ => SignInError::NoCalendars,
        }
    }

    /// The API error code: `auth`, `offline`, `unsupported`, `not-found` or `error`.
    pub fn code(&self) -> &'static str {
        match self {
            SignInError::Refused(_) => "auth",
            SignInError::Offline(_) => "offline",
            SignInError::Unsupported(_) => "unsupported",
            SignInError::NoCalendars => "not-found",
            SignInError::NotPublic(_) | SignInError::Failed(_) => "error",
        }
    }

    /// What happened, for people, naming the server.
    pub fn message(&self) -> String {
        let named = |text: &'static str, service: &Service| trf(text, &[("service", &service.name())]);
        match self {
            SignInError::Refused(service) => named("The {service} server refused the password.", service),
            SignInError::Offline(service) => named("The {service} server cannot be reached.", service),
            SignInError::NotPublic(service) => named("The {service} server is not on the public internet.", service),
            SignInError::Unsupported(service) => named("The {service} server offers no secure sign-in.", service),
            SignInError::NoCalendars => tr("No calendar service answers at this address.").into(),
            SignInError::Failed(service) => named("The {service} server answered unexpectedly.", service),
        }
    }

    /// The answer to `accounts.connect_mail` that failed with this error.
    pub fn response(&self) -> Response {
        Response::Error { code: self.code().into(), message: self.message() }
    }
}

/// What changed in one folder of one account, as a sync found it on the server. Sent to
/// [`MailService::subscribe`]rs; at least one of the lists is not empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailChange {
    /// The account (`mail:<address>`).
    pub account: String,
    /// The folder's id (`f:…`).
    pub folder: String,
    /// The folder's role; `Some(Role::Inbox)` for new mail in the inbox.
    pub role: Option<Role>,
    /// Messages stored for the first time, newest first (message ids `m:…`). A large folder
    /// sends them in several changes, one per batch.
    pub new: Vec<String>,
    /// Messages whose flags changed on the server (read elsewhere, flagged …).
    pub flags: Vec<String>,
    /// Messages gone from the folder: deleted, expunged or moved to another folder (where they
    /// come back under a new id; a move of the core's own keeps the id, issue #205).
    pub removed: Vec<String>,
    /// The folder is synced for the first time, or anew after the server renumbered it: `new`
    /// holds what was there already, not what just arrived. The inbox (task E4.1) takes only
    /// recent mail from such a change.
    pub initial: bool,
}

impl MailChange {
    fn is_empty(&self) -> bool {
        self.new.is_empty() && self.flags.is_empty() && self.removed.is_empty()
    }
}

/// A caller that may read mail at all: the suite, or a client with read access to the area
/// `mail`. Which of the accounts it then sees is [`Mailbox::within`].
fn reads_mail(reach: Reach<'_>) -> bool {
    match reach {
        Reach::Suite => true,
        Reach::Client(rights) => rights.area(torro_ipc::api::Area::Mail).allows(torro_ipc::api::Access::Read),
    }
}

/// The store of one account, shared by its sync and the mail commands. Every call runs on a
/// blocking thread ([`SharedStore::run`]), so a slow disk never stalls the runtime.
#[derive(Debug, Clone)]
pub struct SharedStore(Arc<StdMutex<MailStore>>);

impl SharedStore {
    fn new(store: MailStore) -> SharedStore {
        SharedStore(Arc::new(StdMutex::new(store)))
    }

    /// Waits until every handle of this store is dropped, at most `limit`: the sync, the inbox
    /// feed and running commands each hold one for a moment. Windows cannot delete a database
    /// that is still open, so [`MailService::remove`] waits here before deleting the files.
    async fn closed(self, limit: Duration) {
        let store = Arc::downgrade(&self.0);
        drop(self);
        let deadline = tokio::time::Instant::now() + limit;
        while store.strong_count() > 0 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Runs `work` on the store on a blocking thread and hands back its result. One call at a time
    /// per account; keep `work` short (one transaction, or a few).
    pub async fn run<T, F>(&self, work: F) -> Result<T, MailStoreError>
    where
        T: Send + 'static,
        F: FnOnce(&MailStore) -> Result<T, MailStoreError> + Send + 'static,
    {
        let store = Arc::clone(&self.0);
        tokio::task::spawn_blocking(move || {
            // A panic in a query leaves the connection usable; SQLite rolled back what was open.
            let store = store.lock().unwrap_or_else(PoisonError::into_inner);
            work(&store)
        })
        .await
        .map_err(|err| MailStoreError::Io(io::Error::other(format!("the mail store task failed: {err}"))))?
    }
}

/// The command connection of one account, taken by one command at a time
/// ([`MailService::command`]). Other commands, and the sync's next step, wait until it is
/// released or dropped.
///
/// **Release only what worked.** Call [`Command::release`] when every call on the session
/// succeeded, so the next command uses the same connection. Dropping it instead (after an error,
/// or when the future using it is dropped half-way) closes the connection, and the next command
/// connects anew: a session that failed or was cut off in the middle of a command is never used
/// again. Keep it no longer than the command needs, and never wait for a sync while holding it:
/// the sync takes it between its steps.
pub struct Command {
    slot: OwnedMutexGuard<Option<Session>>,
    session: Session,
    #[allow(dead_code, reason = "used from the mail commands, tasks M3.1 to M3.3")]
    store: SharedStore,
}

impl Command {
    /// The signed-in session. No folder is promised to be open: call `Session::select` (or check
    /// `Session::selected`) before message commands.
    pub fn session(&mut self) -> &mut Session {
        &mut self.session
    }

    /// The account's store.
    #[allow(dead_code, reason = "used from the mail commands, tasks M3.1 to M3.3")]
    pub fn store(&self) -> &SharedStore {
        &self.store
    }

    /// Hands the connection back for the next command. Only after every call succeeded.
    pub fn release(self) {
        let Command { mut slot, session, .. } = self;
        *slot = Some(session);
    }
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Command").field("session", &self.session).finish_non_exhaustive()
    }
}

/// How connections are made: which certificates are trusted, which addresses may be connected to,
/// how host names are resolved and how an SMTP sign-in is checked. The core uses
/// [`Connector::system`]; tests a local server with a test CA and a fake SMTP check.
#[derive(Debug, Clone)]
struct Connector {
    trust: Trust,
    allowed: fn(IpAddr) -> bool,
    smtp: SmtpCheck,
    lookup: Lookup,
}

/// The name lookups of tests: host and port in, the addresses out.
#[cfg(test)]
type FakeLookup = Arc<dyn Fn(&str, u16) -> Vec<SocketAddr> + Send + Sync>;

/// How [`Connector::public_addresses`] resolves a host name. Only there: every connection then
/// goes to the addresses it found (task M2.7b).
#[derive(Clone)]
enum Lookup {
    /// The operating system's resolver (`getaddrinfo`), as every connection of the suite.
    System,
    /// Tests: answers from a function, never from a DNS server.
    #[cfg(test)]
    Fake(FakeLookup),
}

impl fmt::Debug for Lookup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lookup::System => f.write_str("System"),
            #[cfg(test)]
            Lookup::Fake(_) => f.write_str("Fake"),
        }
    }
}

/// The SMTP check of tests: server, login and password in, the outcome out.
#[cfg(test)]
type FakeSmtp = Arc<dyn Fn(&Endpoint, &str, &Secret) -> Result<(), SmtpError> + Send + Sync>;

/// How [`Connector::check_smtp`] checks an SMTP sign-in.
#[derive(Clone)]
enum SmtpCheck {
    /// At the server, with `SmtpSubmitter::check_login` (TLS or STARTTLS, nothing sent).
    Server,
    /// Tests: decides without a connection, from server, login and password.
    #[cfg(test)]
    Fake(FakeSmtp),
}

impl fmt::Debug for SmtpCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SmtpCheck::Server => f.write_str("Server"),
            #[cfg(test)]
            SmtpCheck::Fake(_) => f.write_str("Fake"),
        }
    }
}

impl Connector {
    fn system() -> Connector {
        Connector { trust: Trust::System, allowed: is_public, smtp: SmtpCheck::Server, lookup: Lookup::System }
    }

    /// Connections nowhere: no address is allowed ([`MailService::without_home`]).
    fn nowhere() -> Connector {
        Connector { trust: Trust::System, allowed: |_| false, smtp: SmtpCheck::Server, lookup: Lookup::System }
    }

    /// Checks the host of `server` ([`Connector::public_addresses`]), then signs in to SMTP as
    /// `login` at those addresses and says `QUIT` without sending anything.
    async fn check_smtp(&self, server: &Endpoint, login: &str, password: &Secret) -> Result<(), SignInError> {
        let addresses = self
            .public_addresses(&server.host, server.port)
            .await
            .map_err(|err| SignInError::host(Service::Smtp, err))?;
        let checked = match &self.smtp {
            SmtpCheck::Server => {
                SmtpSubmitter::new(server.clone(), login, password.clone()).pinned_to(addresses).check_login().await
            }
            #[cfg(test)]
            SmtpCheck::Fake(check) => check(server, login, password),
        };
        checked.map_err(SignInError::smtp)
    }

    /// Checks the host ([`Connector::public_addresses`]), then connects to those addresses and
    /// signs in.
    async fn connect(&self, config: &MailAccountConfig, login: &str, password: &Secret) -> Result<Session, MailError> {
        let addresses = self.public_addresses(&config.imap.host, config.imap.port).await?;
        Ok(Session::connect_to(&addresses, &config.imap, login, password, &self.trust).await?)
    }

    /// The addresses of `host` at `port`, when it has any and every one is allowed ([`is_public`]
    /// in the core), so the account never connects into the local network. The name is resolved
    /// here only: every connection goes to exactly these addresses, the host name serving for TLS
    /// alone (`Session::connect_to`, `SmtpSubmitter::pinned_to`, task M2.7b), so a name that
    /// answers differently the second time (DNS rebinding) reaches nothing new.
    ///
    /// [`MailError::Offline`] when the name cannot be resolved, [`MailError::NotPublic`] when it
    /// has no address or one that is not allowed.
    async fn public_addresses(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, MailError> {
        let found: Vec<SocketAddr> = match &self.lookup {
            Lookup::System => tokio::net::lookup_host((host, port))
                .await
                .map_err(|err| MailError::Offline(format!("cannot resolve the mail server: {}", err.kind())))?
                .collect(),
            #[cfg(test)]
            Lookup::Fake(lookup) => lookup(host, port),
        };
        if found.is_empty() || !found.iter().all(|address| (self.allowed)(address.ip())) {
            return Err(MailError::NotPublic);
        }
        Ok(found)
    }
}

/// How an account stands: the last sync and what went wrong since.
#[derive(Debug, Default, Clone)]
struct Status {
    last_sync: Option<DateTime<Utc>>,
    problem: Option<String>,
    /// What the inbox makes of the problem, and since when it stands.
    trouble: Option<(Trouble, DateTime<Utc>)>,
}

impl Status {
    fn synced(&mut self, now: DateTime<Utc>) {
        *self = Status { last_sync: Some(now), problem: None, trouble: None };
    }

    /// Notes `err` at `now`; the same trouble keeps the time it started.
    fn failed(&mut self, err: &MailError, now: DateTime<Utc>) {
        let trouble = err.trouble();
        self.problem = Some(err.message());
        self.trouble = match self.trouble {
            Some((same, since)) if same == trouble => Some((trouble, since)),
            _ => Some((trouble, now)),
        };
    }

    /// As the calendar accounts (task E2.1): a refused or missing sign-in is a problem at once,
    /// failing syncs after [`SYNC_PROBLEM_AFTER`]; fine after a sync that worked.
    fn health(&self, now: DateTime<Utc>) -> Health {
        match self.trouble {
            Some((Trouble::SyncFailing, since)) if now - since <= SYNC_PROBLEM_AFTER => Health::Unsure,
            Some((trouble, since)) => Health::Problem { trouble, since },
            None if self.last_sync.is_some() => Health::Fine,
            None => Health::Unsure,
        }
    }
}

/// One mail account as the service holds it: what it needs to connect, its store, its state.
/// Shared by its worker, the commands and `accounts.sync`.
pub(crate) struct Mailbox {
    account: Account,
    config: MailAccountConfig,
    settings: MailSettings,
    password: Option<Secret>,
    /// `None` when the account's mail is switched off, or its store could not be opened (then
    /// `status` says why).
    store: Option<SharedStore>,
    attachment_cache: Option<AttachmentCache>,
    status: StdMutex<Status>,
    /// The command connection; `None` until the first command or after one failed.
    command: Arc<Mutex<Option<Session>>>,
    /// One folder sync at a time per account: the inbox sync of the IDLE connection and the sync
    /// of all folders compare the store with the server and must not overlap.
    sync_lock: Mutex<()>,
    /// Wakes the worker from its wait after a failure (a sync that worked in between).
    resume: Notify,
    /// The inbox's path as the server names it (`INBOX` until the folders were listed).
    inbox: StdMutex<String>,
    /// The roles the server leaves open (with the person's picks applied) and their candidate
    /// folders, as the last folder sync listed them ([`Mailbox::note_folders`], task M2.8). Kept
    /// here so `accounts.list_accounts` answers without asking the server; empty until the first
    /// folder sync of this mailbox.
    open_roles: StdMutex<Vec<OpenRoleView>>,
    /// The last step of the indexer reached the server and found texts to fetch (issue #136):
    /// `indexing` in `mail.index_status`; `waiting` while it is not.
    indexing: AtomicBool,
    events: broadcast::Sender<MailChange>,
    connector: Connector,
    timing: Timing,
}

impl Mailbox {
    /// The account's id (`mail:<address>`).
    pub(crate) fn id(&self) -> &str {
        &self.account.id
    }

    /// Mail is on, there is a password and a store: the account is synced.
    fn syncs(&self) -> bool {
        self.settings.services.mail && self.password.is_some() && self.store.is_some()
    }

    /// The store, or [`MailError::NotFound`] for an account whose mail is off and
    /// [`MailError::Store`] when it could not be opened.
    pub(crate) fn store(&self) -> Result<SharedStore, MailError> {
        match (&self.store, self.settings.services.mail) {
            (Some(store), _) => Ok(store.clone()),
            (None, false) => Err(MailError::NotFound),
            (None, true) => Err(MailError::Store(MailStoreError::Io(io::Error::other("the mail store is not open")))),
        }
    }

    pub(crate) fn attachment_cache(&self) -> Option<AttachmentCache> {
        self.attachment_cache.clone()
    }

    /// `reach` may see this account: the suite always, a client when its rights name it.
    fn within(&self, reach: Reach<'_>) -> bool {
        match reach {
            Reach::Suite => true,
            Reach::Client(rights) => rights.reaches_mail_account(self.id()),
        }
    }

    /// The folders the person picked for roles the server left open (task M2.3).
    pub(crate) fn folder_roles(&self) -> &BTreeMap<Role, String> {
        &self.settings.folder_roles
    }

    pub(crate) fn inbox_path(&self) -> String {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn set_inbox_path(&self, path: &str) {
        *self.inbox.lock().unwrap_or_else(PoisonError::into_inner) = path.to_string();
    }

    /// Notes the roles `list` leaves open, each with its candidates' names and paths (task M2.8),
    /// for [`MailView::open_roles`]. The folder sync calls it with every listing it makes with the
    /// person's picks ([`Mailbox::folder_roles`]), so a picked role is no longer open. Names and
    /// paths come from the server: they are shown, never acted on.
    pub(crate) fn note_folders(&self, list: &FolderList) {
        let name_of = |path: &str| {
            list.folders.iter().find(|folder| folder.path == path).map_or_else(|| path.to_string(), |f| f.name.clone())
        };
        let open = list
            .open
            .iter()
            .map(|open| OpenRoleView {
                role: open.role,
                candidates: open
                    .candidates
                    .iter()
                    .map(|path| FolderChoice { name: name_of(path), path: path.clone() })
                    .collect(),
            })
            .collect();
        *self.open_roles.lock().unwrap_or_else(PoisonError::into_inner) = open;
    }

    /// Notes whether the indexer is fetching texts now ([`Mailbox::indexing`]).
    pub(crate) fn note_indexing(&self, indexing: bool) {
        self.indexing.store(indexing, Ordering::Release);
    }

    /// How far the account's search index is (`mail.index_status`); `None` without a store.
    async fn index_progress(&self) -> Result<Option<MailIndexProgress>, MailError> {
        let Ok(store) = self.store() else { return Ok(None) };
        let (messages, indexed, text_unavailable) = store.run(|store| store.index_breakdown()).await?;
        let state = match (indexed.saturating_add(text_unavailable) >= messages, self.indexing.load(Ordering::Acquire))
        {
            (true, _) => IndexState::Complete,
            (false, true) => IndexState::Indexing,
            (false, false) => IndexState::Waiting,
        };
        Ok(Some(MailIndexProgress { account: self.id().to_string(), messages, indexed, text_unavailable, state }))
    }

    /// One folder sync at a time ([`Mailbox::sync_lock`]).
    pub(crate) async fn one_sync(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.sync_lock.lock().await
    }

    /// Sends `change` to the subscribers, unless it is empty. Nobody listening is fine.
    pub(crate) fn announce(&self, change: MailChange) {
        if !change.is_empty() {
            let _ = self.events.send(change);
        }
    }

    /// The changes of this account alone, for the sync's tests.
    #[cfg(test)]
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<MailChange> {
        self.events.subscribe()
    }

    fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn note_synced(&self) {
        let had_trouble = {
            let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
            let had = status.trouble.is_some();
            status.synced(Utc::now());
            had
        };
        // A worker waiting after a failure need not wait any longer.
        if had_trouble {
            self.resume.notify_one();
        }
    }

    fn note_failure(&self, err: &MailError) {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).failed(err, Utc::now());
    }

    /// The password was refused earlier; nothing is tried before the account is connected anew.
    fn refused(&self) -> bool {
        matches!(self.status().trouble, Some((Trouble::SignInRefused, _)))
    }

    /// A new signed-in session. A refused password is noted at the account at once.
    pub(crate) async fn connect(&self) -> Result<Session, MailError> {
        if self.refused() {
            return Err(MailError::Auth);
        }
        let password = self.password.as_ref().ok_or(MailError::NoPassword)?;
        let session = self.connector.connect(&self.config, &self.account.login_name, password).await;
        if let Err(err @ MailError::Auth) = &session {
            self.note_failure(err);
        }
        session
    }

    /// The command connection: the one kept from the last command, or a new one.
    pub(crate) async fn command(&self) -> Result<Command, MailError> {
        let store = self.store()?;
        let mut slot = Arc::clone(&self.command).lock_owned().await;
        let session = match slot.take() {
            Some(session) => session,
            None => self.connect().await?,
        };
        Ok(Command { slot, session, store })
    }

    /// Syncs every folder now ([`sync::sync_account`]) and notes the outcome at the account.
    async fn sync_now(&self) -> Result<(), MailError> {
        let result = sync::sync_account(self).await;
        match &result {
            Ok(()) => self.note_synced(),
            Err(err) => self.note_failure(err),
        }
        result
    }

    /// The account as `accounts.list_accounts` lists it, with its [`MailView`] (task M2.8): what
    /// is stored for it, the open roles of the last folder sync and the mail part's state. The
    /// calendar part's state is the calendar service's; the core adds it when it joins both parts
    /// (`calendars` stays `None` here). Never the password.
    fn info(&self) -> AccountInfo {
        let status = self.status();
        let mail = ServiceState { last_sync: status.last_sync, problem: status.problem.clone() };
        let view = MailView {
            address: self.config.address.clone(),
            sender_name: self.config.sender_name.clone(),
            imap: self.config.imap.clone(),
            smtp: self.config.smtp.clone(),
            caldav: self.config.caldav.clone(),
            settings: self.settings.clone(),
            open_roles: self.open_roles.lock().unwrap_or_else(PoisonError::into_inner).clone(),
            mail,
            calendars: None,
            trusted_certificates: self.config.trusted_certificates.clone(),
        };
        AccountInfo {
            id: self.account.id.clone(),
            kind: AccountKind::Mail.name().into(),
            label: self.config.address.clone(),
            state: AccountState::Connected {
                server: self.account.server.clone(),
                login_name: self.account.login_name.clone(),
                last_sync: status.last_sync,
                problem: status.problem,
            },
            collections: Vec::new(),
            scopes: None,
            mail: Some(view),
            ntfy: None,
            services: None,
        }
    }

    fn check(&self, now: DateTime<Utc>) -> AccountCheck {
        // Mail switched off: nothing to fix, an old entry may close.
        let health = if self.settings.services.mail { self.status().health(now) } else { Health::Fine };
        AccountCheck { account: self.account.id.clone(), label: self.config.address.clone(), health }
    }
}

/// The worker of one account: watches the inbox with `IDLE`, syncs all folders every
/// [`Timing::poll`], and after a failure waits longer each time ([`Timing::retry_first`] doubling
/// up to [`Timing::retry_max`]). A refused or missing password stops it until the account is
/// replaced ([`MailService::reload`]).
async fn run(mailbox: Arc<Mailbox>) {
    let mut wait = mailbox.timing.retry_first;
    loop {
        let err = match watch(&mailbox, &mut wait).await {
            Ok(never) => match never {},
            Err(err) => err,
        };
        // The id, never a server text that could hold more: the log is not for contents.
        eprintln!("torroworkplace: mail sync of {} stopped: {}", mailbox.id(), err.code());
        mailbox.note_failure(&err);
        if matches!(err, MailError::Auth | MailError::NoPassword) {
            // Signing in again would only be refused again, and servers lock accounts after a
            // few refusals: wait for the person, who connects the account anew.
            std::future::pending::<()>().await;
        }
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = mailbox.resume.notified() => {}
        }
        wait = (wait * 2).min(mailbox.timing.retry_max);
    }
}

/// The indexer of one account (issue #136): once the first sync is done, puts the texts of older
/// messages into the search index, [`sync::BACKFILL_BATCH`] at a time, newest first, one after the
/// other; with nothing left it looks again after [`Timing::poll`] (a sync may have stored more),
/// after a failure (offline, refused) it waits as the worker does. Each text takes the command
/// connection for one message, so commands and the sync never wait long for it.
async fn index(mailbox: Arc<Mailbox>) {
    let mut wait = mailbox.timing.retry_first;
    loop {
        if mailbox.status().last_sync.is_none() {
            tokio::time::sleep(mailbox.timing.retry_first).await;
            continue;
        }
        match sync::backfill(&mailbox, sync::BACKFILL_BATCH, Utc::now()).await {
            Ok(0) => tokio::time::sleep(mailbox.timing.poll).await,
            Ok(_) => wait = mailbox.timing.retry_first,
            Err(err) => {
                // The id and the code, never a server text: the log is not for contents.
                eprintln!("torroworkplace: mail indexing of {} paused: {}", mailbox.id(), err.code());
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(mailbox.timing.retry_max);
            }
        }
    }
}

/// Connects, syncs everything once, then waits in `IDLE` on the inbox until the connection fails.
async fn watch(mailbox: &Mailbox, wait: &mut Duration) -> Result<Infallible, MailError> {
    // The first sync of a large mailbox takes a while; the IDLE connection is made afterwards, so
    // it never sits silent long enough for the server to drop it.
    mailbox.sync_now().await?;
    let mut idle = mailbox.connect().await?;
    *wait = mailbox.timing.retry_first;
    let mut next_poll = Instant::now() + mailbox.timing.poll;
    // Opens the inbox on this connection and takes what arrived since the sync above, so nothing
    // that came in between waits for the next sync of all folders.
    sync::sync_inbox(&mut idle, mailbox).await?;
    loop {
        let until_poll = next_poll.saturating_duration_since(Instant::now());
        if until_poll.is_zero() {
            mailbox.sync_now().await?;
            next_poll = Instant::now() + mailbox.timing.poll;
            continue;
        }
        // Never longer than until the next sync of all folders, so the IDLE is never cut off (a
        // session whose command was dropped half-way cannot be used again).
        match idle.idle(until_poll.min(mailbox.timing.idle_max)).await? {
            IdleEvent::Changed => {
                sync::sync_inbox(&mut idle, mailbox).await?;
                mailbox.note_synced();
            }
            IdleEvent::Timeout => {}
        }
    }
}

/// An account with its worker and indexer; dropping it stops both.
struct Entry {
    mailbox: Arc<Mailbox>,
    worker: Option<JoinHandle<()>>,
    indexer: Option<JoinHandle<()>>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        for task in [&self.worker, &self.indexer].into_iter().flatten() {
            task.abort();
        }
    }
}

/// A mail account of `accounts.json` with its servers and what the keychain had for it.
type Stored = (Account, MailAccountConfig, Result<Option<Secret>, String>);

/// The mail accounts of the core. Built by [`MailService::system`]; nothing is read or sent before
/// [`MailService::start`].
pub struct MailService {
    /// `accounts.json`; `None` without a home directory, and then there are no mail accounts.
    accounts_file: Option<PathBuf>,
    secrets: Arc<dyn TokenStore>,
    /// `None` without a home directory: no store, so nothing is synced.
    dirs: Option<MailDirs>,
    connector: Connector,
    timing: Timing,
    /// In the order of `accounts.json`.
    entries: RwLock<Vec<Entry>>,
    /// The stored accounts were read (or could not be), so `entries` is all there is.
    loaded: AtomicBool,
    events: broadcast::Sender<MailChange>,
    /// One change of a mail account in `accounts.json` at a time: read, change, write.
    writing: Mutex<()>,
    /// The calendar service, which holds the calendar part of a mail account (task M2.7);
    /// [`MailService::with_calendars`]. `None` in the mail service's own tests.
    calendars: Option<Arc<PimService>>,
    /// The send journal and the kept Sent copies (task M3.3), opened on first use
    /// ([`MailService::outbox`]).
    outbox: tokio::sync::OnceCell<Arc<Outbox>>,
    /// Mail is turned off (task W.2): the accounts and their stores stay readable, but no worker
    /// fetches or indexes in the background.
    paused: AtomicBool,
    cache_workers: std::sync::Mutex<std::collections::HashSet<String>>,
    cache_jobs_lock: Mutex<()>,
}

struct CacheWorkerGuard {
    service: Arc<MailService>,
    account: String,
}

impl Drop for CacheWorkerGuard {
    fn drop(&mut self) {
        self.service.cache_workers.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.account);
    }
}

impl MailService {
    /// The service on the user's account list, keychain and data directory, as the daemon runs
    /// it. Reads and sends nothing yet.
    pub fn system() -> MailService {
        let accounts_file = account::accounts_file().ok();
        MailService::new(
            accounts_file,
            Arc::new(SystemTokens),
            MailDirs::for_workplace(),
            Connector::system(),
            SYSTEM_TIMING,
        )
    }

    fn new(
        accounts_file: Option<PathBuf>,
        secrets: Arc<dyn TokenStore>,
        dirs: Option<MailDirs>,
        connector: Connector,
        timing: Timing,
    ) -> MailService {
        MailService {
            accounts_file,
            secrets,
            dirs,
            connector,
            timing,
            entries: RwLock::new(Vec::new()),
            loaded: AtomicBool::new(false),
            events: broadcast::channel(EVENTS).0,
            writing: Mutex::new(()),
            calendars: None,
            outbox: tokio::sync::OnceCell::new(),
            paused: AtomicBool::new(false),
            cache_workers: std::sync::Mutex::new(std::collections::HashSet::new()),
            cache_jobs_lock: Mutex::new(()),
        }
    }

    /// Notes whether mail is turned off (task W.2) before [`MailService::start`]: the workers then
    /// start only while it is on.
    pub fn pause_from_start(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    /// Mail was turned off or on (task W.2): the workers stop, or start again. Nothing happens when
    /// it stays as it was. The accounts are opened anew, so requests keep answering throughout.
    pub async fn set_paused(&self, paused: bool) {
        if self.paused.swap(paused, Ordering::AcqRel) == paused {
            return;
        }
        // Dropping the entries stops their workers; reloading opens them as `paused` says.
        self.entries.write().await.clear();
        self.reload().await;
    }

    /// A service without account list, keychain or data directory, for the cores of tests that
    /// bring no mail service of their own: it knows no account, cannot store one
    /// ([`MailService::stored_accounts`] fails first, before any server is asked) and connects
    /// nowhere.
    pub fn without_home() -> MailService {
        MailService::new(None, Arc::new(NoKeychain), None, Connector::nowhere(), SYSTEM_TIMING)
    }

    /// This service with the calendar service `calendars`, which holds the calendar part of a mail
    /// account (task M2.7): its health joins the account's one check
    /// ([`MailService::account_checks`]).
    pub fn with_calendars(mut self, calendars: Arc<PimService>) -> MailService {
        self.calendars = Some(calendars);
        self
    }

    /// Loads the mail accounts and starts their workers ([`MailService::reload`]), files the Sent
    /// copies still waiting now and every minute ([`send::keep_copying`], task M3.3), and deletes
    /// fetched attachments older than a day and stores of removed accounts now and every hour. The
    /// core calls it once.
    pub fn start(self: &Arc<Self>) {
        let service = Arc::clone(self);
        tokio::spawn(async move {
            service.reload().await;
            tokio::spawn(send::keep_copying(Arc::clone(&service)));
            let Some(dirs) = service.dirs.clone() else { return };
            let cache = AttachmentCache::new(&dirs);
            loop {
                service.remove_unknown_stores(&dirs).await;
                let mut retention = Vec::new();
                for (account, store) in service.stores(Reach::Suite).await {
                    if let Ok(policy) = store.run(|store| store.cache_policy()).await {
                        retention.push((account, policy.attachment_retention_days));
                    }
                }
                let purge = cache.clone();
                match tokio::task::spawn_blocking(move || purge.purge_with_retention(&retention)).await {
                    Ok(Ok(_)) => {}
                    Ok(Err(err)) => eprintln!("torroworkplace: cannot clean up mail attachments: {err}"),
                    Err(err) => eprintln!("torroworkplace: cannot clean up mail attachments: {err}"),
                }
                tokio::time::sleep(PURGE_EVERY).await;
            }
        });
    }

    /// Reads the mail accounts of `accounts.json` anew and brings the workers in line: a new
    /// account is opened and synced, an unchanged one (same account and password) keeps running,
    /// a changed one (servers, login, settings, password) starts over, a removed one is stopped.
    /// Its store stays on disk; deleting it is for whoever removes the account
    /// (`MailStore::remove`, task M2.7). Call it after an account was connected, changed or
    /// removed. An unreadable `accounts.json` keeps what runs.
    pub async fn reload(&self) {
        let stored = match self.read_accounts().await {
            Ok(stored) => stored,
            Err(err) => {
                eprintln!("torroworkplace: cannot read the mail accounts: {err}");
                self.loaded.store(true, Ordering::Release);
                return;
            }
        };
        let mut entries = self.entries.write().await;
        let mut kept: Vec<Entry> = Vec::new();
        for (account, config, secret) in stored {
            let password = match secret {
                Ok(password) => password,
                Err(err) => {
                    eprintln!("torroworkplace: cannot read the password of {}: {err}", account.id);
                    None
                }
            };
            let same = entries.iter().position(|entry| {
                entry.mailbox.account == account && entry.mailbox.password.as_ref() == password.as_ref()
            });
            match same {
                Some(at) => kept.push(entries.remove(at)),
                None => kept.push(self.open(account, config, password).await),
            }
        }
        // What is left was removed or changed; dropping stops its worker.
        *entries = kept;
        drop(entries);
        self.loaded.store(true, Ordering::Release);
    }

    /// The mail accounts of `accounts.json` and their passwords, read on the keychain's thread.
    async fn read_accounts(&self) -> Result<Vec<Stored>, String> {
        let Some(path) = self.accounts_file.clone() else { return Ok(Vec::new()) };
        let secrets = Arc::clone(&self.secrets);
        keychain(move || {
            let accounts = account::load_from(&path).map_err(|err| err.to_string())?;
            Ok(accounts
                .into_iter()
                .filter(|account| account.kind == AccountKind::Mail)
                // Its secret would be a client's key, which must never go to a server.
                .filter(|account| !crate::clients::is_client_keychain_account(&account.id))
                .filter_map(|account| {
                    // A mail account without servers cannot be synced; it is none.
                    let config = account.mail.clone()?;
                    let secret = secrets.get(&account);
                    Some((account, config, secret))
                })
                .collect())
        })
        .await
    }

    /// The entry of a newly read account: its store opened and its worker started when it syncs.
    async fn open(&self, account: Account, config: MailAccountConfig, password: Option<Secret>) -> Entry {
        let mailbox = Arc::new(self.mailbox(account, config, password).await);
        // Mail turned off (task W.2): the account is there, nothing runs for it.
        let runs = mailbox.syncs() && !self.paused.load(Ordering::Acquire);
        let worker = runs.then(|| tokio::spawn(run(Arc::clone(&mailbox))));
        let indexer = (runs && self.timing.index).then(|| tokio::spawn(index(Arc::clone(&mailbox))));
        Entry { mailbox, worker, indexer }
    }

    async fn mailbox(&self, account: Account, config: MailAccountConfig, password: Option<Secret>) -> Mailbox {
        let settings = account.mail_settings.clone().unwrap_or_default();
        let mut status = Status::default();
        let store = match (&self.dirs, settings.services.mail) {
            (_, false) => None,
            (None, true) => {
                status.problem = Some(tr("No home directory, so mail cannot be kept on this device.").into());
                None
            }
            (Some(dirs), true) => {
                let (dirs, id) = (dirs.clone(), account.id.clone());
                match tokio::task::spawn_blocking(move || MailStore::open(&dirs, &id)).await {
                    Ok(Ok(store)) => Some(SharedStore::new(store)),
                    Ok(Err(err)) => {
                        eprintln!("torroworkplace: cannot open the mail store of {}: {err}", account.id);
                        status.failed(&MailError::Store(err), Utc::now());
                        None
                    }
                    Err(err) => {
                        eprintln!("torroworkplace: cannot open the mail store of {}: {err}", account.id);
                        let failed = MailStoreError::Io(io::Error::other(err.to_string()));
                        status.failed(&MailError::Store(failed), Utc::now());
                        None
                    }
                }
            }
        };
        if settings.services.mail && password.is_none() {
            status.failed(&MailError::NoPassword, Utc::now());
        }
        Mailbox {
            account,
            config,
            settings,
            password,
            store,
            attachment_cache: self.dirs.as_ref().map(AttachmentCache::new),
            status: StdMutex::new(status),
            command: Arc::new(Mutex::new(None)),
            sync_lock: Mutex::new(()),
            resume: Notify::new(),
            inbox: StdMutex::new("INBOX".to_string()),
            open_roles: StdMutex::new(Vec::new()),
            indexing: AtomicBool::new(false),
            events: self.events.clone(),
            connector: self.connector.clone(),
            timing: self.timing,
        }
    }

    /// New changes of every account from now on ([`MailChange`]). A subscriber that falls more
    /// than 256 changes behind gets `RecvError::Lagged` and should read what it needs from the
    /// store again.
    #[allow(dead_code, reason = "used from the inbox's mail source, task E4.1")]
    pub fn subscribe(&self) -> broadcast::Receiver<MailChange> {
        self.events.subscribe()
    }

    /// Sends `change` to the subscribers, unless it is empty: for what a mail command did itself,
    /// which no sync reports (issue #205: a message moved out of the inbox, whose store row moved
    /// with it). Nobody listening is fine.
    pub(crate) fn announce(&self, change: MailChange) {
        if !change.is_empty() {
            let _ = self.events.send(change);
        }
    }

    /// The stored accounts were read ([`MailService::start`]), or could not be: until then
    /// [`MailService::infos`] is empty although accounts may exist.
    pub fn loaded(&self) -> bool {
        self.loaded.load(Ordering::Acquire)
    }

    /// The mail accounts as `accounts.list_accounts` lists them, never with their passwords: for
    /// a client only those its rights reach (`Rights::reaches_mail_account`); the others behave as
    /// if they did not exist. Mail accounts have no collections here; each has its [`MailView`]
    /// (task M2.8), answered from memory without asking a server.
    pub async fn infos(&self, reach: Reach<'_>) -> Vec<AccountInfo> {
        self.entries
            .read()
            .await
            .iter()
            .filter(|entry| entry.mailbox.within(reach))
            .map(|entry| entry.mailbox.info())
            .collect()
    }

    /// How every mail account stands for the inbox at `now` (task E2.1,
    /// [`crate::inbox::tend_suite`]); `None` until the stored accounts were read. An account that
    /// syncs calendars too has one check for both parts (task M2.7, [`joined`]): the calendar
    /// service leaves it out of its own checks (`PimService::account_checks`).
    pub async fn account_checks(&self, now: DateTime<Utc>) -> Option<Vec<AccountCheck>> {
        if !self.loaded.load(Ordering::Acquire) {
            return None;
        }
        let mut checks: Vec<AccountCheck> =
            self.entries.read().await.iter().map(|entry| entry.mailbox.check(now)).collect();
        if let Some(calendars) = &self.calendars {
            for check in &mut checks {
                if let Some(health) = calendars.calendar_health(&check.account, now).await {
                    check.health = joined(check.health, health);
                }
            }
        }
        Some(checks)
    }

    /// Checks a sign-in of a mail account before anything is stored (`accounts.connect_mail`,
    /// task M2.7): the IMAP host is public ([`Connector::public_addresses`], and the sign-in goes to
    /// the addresses checked, task M2.7b), IMAP takes `login` and
    /// `password`, and its folders are listed with the person's picks `chosen`; then the same for
    /// SMTP, signing in without sending anything. A refused IMAP sign-in stops there, so a server
    /// that locks accounts after a few refusals is asked once. Answers with the roles the server
    /// leaves open (two candidate folders with mail, `accounts.update_mail_account` picks one).
    ///
    /// Listing the folders may create a missing drafts or sent folder on the server, as every sync
    /// does (`folders::discover`). Nothing is stored here, and no error holds the password or a
    /// server's words.
    pub async fn check_sign_in(
        &self,
        config: &MailAccountConfig,
        login: &str,
        password: &Secret,
        chosen: &BTreeMap<Role, String>,
    ) -> Result<Vec<Role>, SignInError> {
        let imap = &config.imap;
        let addresses = self.check_public(Service::Imap, &imap.host, imap.port).await?;
        let mut session = Session::connect_to(&addresses, imap, login, password, &self.connector.trust)
            .await
            .map_err(SignInError::imap)?;
        let listed = folders::discover(&mut session, chosen).await.map_err(SignInError::imap)?;
        // Only to be polite: the sign-in was checked.
        let _ = session.logout().await;
        self.connector.check_smtp(&config.smtp, login, password).await?;
        Ok(listed.open.into_iter().map(|open| open.role).collect())
    }

    /// `host` (of `service`) has addresses and all are on the public internet, as before every
    /// connection of this service; the CalDAV address of a mail account is checked the same way
    /// before it is signed in to (task M2.7). Answers with those addresses (task M2.7b): connect to
    /// them and nowhere else (`Session::connect_to`, `CalDavClient::pinned`), the host name only
    /// for TLS, so the name cannot point elsewhere between the check and the connection.
    pub async fn check_public(&self, service: Service, host: &str, port: u16) -> Result<Vec<SocketAddr>, SignInError> {
        self.connector.public_addresses(host, port).await.map_err(|err| SignInError::host(service, err))
    }

    /// Every account of `accounts.json`, of every kind, as connecting and changing a mail account
    /// need them (task M2.7: the account to connect again, the CalDAV account of the same sign-in
    /// to take over). An error without a home directory, so nothing is checked that could not be
    /// stored.
    pub async fn stored_accounts(&self) -> Result<Vec<Account>, String> {
        let path = self
            .accounts_file
            .clone()
            .ok_or_else(|| tr("No home directory, so the account cannot be stored.").to_string())?;
        keychain(move || account::load_from(&path).map_err(|err| err.to_string())).await
    }

    /// Stores the mail account `account` with `password` (task M2.7,
    /// [`crate::account::store_taking_over`]): the password in the keychain under its id, the
    /// account in `accounts.json` in place of the one with the same id (servers and password
    /// replaced, settings as `account` brings them) or at the end, and `replaces`, the CalDAV
    /// account of the same sign-in, out of the list in the same write and its password out of the
    /// keychain. Does not start the account: [`MailService::restart`]. Errors never hold the
    /// password.
    pub async fn save_account(
        &self,
        account: &Account,
        password: &Secret,
        replaces: Option<&Account>,
    ) -> Result<(), String> {
        let _writing = self.writing.lock().await;
        let (file, secrets) = (self.accounts_file.clone(), Arc::clone(&self.secrets));
        let (account, password, replaces) = (account.clone(), password.clone(), replaces.cloned());
        keychain(move || {
            crate::account::store_taking_over(file.as_deref(), &*secrets, &account, &password, replaces.as_ref())
        })
        .await
    }

    /// Starts the account `id` anew from `accounts.json` after it was connected (task M2.7), also
    /// when nothing about it changed: a password refused earlier is tried again, which
    /// [`MailService::reload`] alone never does. The other accounts are reloaded as usual.
    pub async fn restart(&self, id: &str) {
        self.entries.write().await.retain(|entry| entry.mailbox.id() != id);
        self.reload().await;
    }

    /// Changes the stored mail account `id` with `edit` (`accounts.update_mail_account`, task
    /// M2.7) and reloads ([`MailService::reload`]), answering with the account as stored. Read,
    /// changed and written under one lock, so two changes never undo each other. `Ok(None)` when no
    /// mail account has this id; nothing is written then.
    ///
    /// Settings only: an `edit` that touches the id, the servers or the login is refused (`Err`)
    /// and nothing is written, so no change of settings can steer the password elsewhere.
    pub async fn update(
        &self,
        id: &str,
        edit: impl FnOnce(&mut Account) + Send + 'static,
    ) -> Result<Option<Account>, String> {
        let updated = {
            let _writing = self.writing.lock().await;
            let path = self
                .accounts_file
                .clone()
                .ok_or_else(|| tr("No home directory, so the account cannot be stored.").to_string())?;
            let id = id.to_string();
            keychain(move || {
                let mut accounts = account::load_from(&path).map_err(|err| err.to_string())?;
                let Some(stored) = accounts.iter_mut().find(|a| a.id == id && a.kind == AccountKind::Mail) else {
                    return Ok(None);
                };
                let before = stored.clone();
                edit(stored);
                if !same_sign_in(&before, stored) {
                    return Err("only the settings of a mail account can be changed this way".to_string());
                }
                let updated = stored.clone();
                account::save_to(&path, &accounts).map_err(|err| err.to_string())?;
                Ok(Some(updated))
            })
            .await?
        };
        if updated.is_some() {
            self.reload().await;
        }
        Ok(updated)
    }

    /// The roles the server of account `id` leaves open, each with its candidate folders, asked
    /// anew on the command connection without the person's picks (task M2.7):
    /// `accounts.update_mail_account` takes a pick only for one of these, and only one of its
    /// candidates. Errors as [`MailService::command`].
    ///
    /// Nothing is noted for the account's view here ([`Mailbox::note_folders`], task M2.8): this
    /// listing ignores the person's picks, so it would show picked roles as open again. The view
    /// takes what the folder sync lists with the picks.
    pub async fn open_roles(&self, id: &str) -> Result<Vec<OpenRole>, MailError> {
        let mut command = self.command(id).await?;
        let listed = folders::discover(command.session(), &BTreeMap::new()).await?;
        command.release();
        Ok(listed.open)
    }

    /// The password of `account` from the keychain, for its calendar part when its services
    /// changed (task M2.7). It stays inside the core.
    pub async fn password(&self, account: &Account) -> Result<Option<Secret>, String> {
        let (secrets, account) = (Arc::clone(&self.secrets), account.clone());
        keychain(move || secrets.get(&account)).await
    }

    /// Removes the mail account `id` (`accounts.remove_account`, task M2.7): its password from the
    /// keychain and its entry in `accounts.json` (the password first, [`crate::account::forget_in`]),
    /// then stops its sync, closes its store and deletes the store's files with `-wal` and `-shm`
    /// (`MailStore::remove`). Its calendar part is the calendar service's
    /// (`PimService::forget_mail_account`). An error stops at that step; what was done stays done.
    pub async fn remove(&self, id: &str) -> Result<(), String> {
        {
            let _writing = self.writing.lock().await;
            let (file, secrets, forget) = (self.accounts_file.clone(), Arc::clone(&self.secrets), id.to_string());
            keychain(move || crate::account::forget_in(file.as_deref(), &*secrets, &forget)).await?;
        }
        let entry = {
            let mut entries = self.entries.write().await;
            entries.iter().position(|entry| entry.mailbox.id() == id).map(|at| entries.remove(at))
        };
        let mut store = None;
        if let Some(mut entry) = entry {
            // The worker holds the store too: it must be gone before the file is deleted.
            if let Some(worker) = entry.worker.take() {
                worker.abort();
                let _ = worker.await;
            }
            store = entry.mailbox.store.clone();
            drop(entry);
        }
        // Others may still hold it for a moment (the inbox feed, a running command).
        if let Some(store) = store {
            store.closed(Duration::from_secs(5)).await;
        }
        let Some(dirs) = self.dirs.clone() else { return Ok(()) };
        // Sent copies that wait for its Sent folder hold whole messages: they go too (task M3.3).
        if let Ok(outbox) = self.outbox().await {
            let account = id.to_string();
            let _ = tokio::task::spawn_blocking(move || outbox.forget_account(&account)).await;
        }
        let account = id.to_string();
        // The account is gone already (keychain, `accounts.json`); a store Windows still holds open
        // is deleted by the next clean-up ([`MailStore::remove_unknown`]), not left forever.
        match tokio::task::spawn_blocking(move || MailStore::remove(&dirs, &account)).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => eprintln!("torroworkplace: the mail store of {id} is deleted later: {err}"),
            Err(err) => eprintln!("torroworkplace: the mail store of {id} is deleted later: {err}"),
        }
        Ok(())
    }

    /// Deletes the stores of accounts no longer in `accounts.json` (task M2.7: one Windows kept open
    /// while its account was removed). Nothing is deleted when the accounts cannot be read.
    async fn remove_unknown_stores(&self, dirs: &MailDirs) {
        let Ok(stored) = self.read_accounts().await else { return };
        let accounts: Vec<String> = stored.into_iter().map(|(account, _, _)| account.id).collect();
        let dirs = dirs.clone();
        // An hour old at least: a store opened while the accounts were read is not touched.
        let min_age = PURGE_EVERY;
        match tokio::task::spawn_blocking(move || MailStore::remove_unknown(&dirs, &accounts, min_age)).await {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => eprintln!("torroworkplace: cannot clean up old mail stores: {err}"),
            Err(err) => eprintln!("torroworkplace: cannot clean up old mail stores: {err}"),
        }
    }

    /// `reach` sees mail accounts whose mail is on, so `accounts.sync` has something to do here
    /// (also when they cannot be synced: then the sync answers why).
    pub async fn syncs_any(&self, reach: Reach<'_>) -> bool {
        self.entries
            .read()
            .await
            .iter()
            .any(|entry| entry.mailbox.settings.services.mail && entry.mailbox.within(reach))
    }

    /// Syncs every account whose mail is on and that `reach` sees, side by side (`accounts.sync`):
    /// all folders, as the worker does every five minutes. A client never starts the sync of an
    /// account its rights do not reach, nor hears of its failure. An account that fails keeps its
    /// cached mail and shows its problem; the others are synced anyway. Answers with the first
    /// failure: [`MailError::Auth`] for a refused password (without asking its server again),
    /// [`MailError::NoPassword`] when none is stored.
    pub async fn sync_all(&self, reach: Reach<'_>) -> Result<(), MailError> {
        let mailboxes: Vec<Arc<Mailbox>> = self
            .entries
            .read()
            .await
            .iter()
            .filter(|entry| entry.mailbox.settings.services.mail && entry.mailbox.within(reach))
            .map(|entry| Arc::clone(&entry.mailbox))
            .collect();
        let runs: Vec<JoinHandle<Result<(), MailError>>> =
            mailboxes.into_iter().map(|mailbox| tokio::spawn(async move { mailbox.sync_now().await })).collect();
        let mut first = None;
        for run in runs {
            let result =
                run.await.unwrap_or_else(|err| Err(MailError::Offline(format!("the sync task failed: {err}"))));
            if let Err(err) = result {
                first.get_or_insert(err);
            }
        }
        first.map_or(Ok(()), Err)
    }

    /// The command connection of `account` for one command (tasks M3.1 to M3.3): the kept
    /// session, or a new one, signed in. Waits while another command or a step of the sync holds
    /// it. See [`Command`] for when to release it.
    ///
    /// Errors: [`MailError::NotFound`] for an unknown account or one whose mail is off;
    /// [`MailError::Auth`] when its password was refused (also earlier: nothing is sent then);
    /// [`MailError::NoPassword`], [`MailError::Offline`], [`MailError::NotPublic`] when no
    /// connection could be made.
    pub async fn command(&self, account: &str) -> Result<Command, MailError> {
        self.mailbox_of(account).await?.command().await
    }

    /// A new signed-in session of `account` of its own, outside the command connection: for work
    /// that reads or writes for minutes or hours (a mail backup or restore, #267) and must not hold
    /// up the sync, reading and sending, which share [`MailService::command`]. Signed in exactly as
    /// the account's other connections (checked public addresses, the password from the core's
    /// keychain, which never leaves the core). The caller ends it with `Session::logout`.
    ///
    /// Errors as for [`MailService::command`].
    pub(crate) async fn own_session(&self, account: &str) -> Result<Session, MailError> {
        let mailbox = self.mailbox_of(account).await?;
        // Same rule as the command connection: an account whose store is not open is not used.
        mailbox.store()?;
        mailbox.connect().await
    }

    /// The store of `account`, to read the cached mail without a connection (task M3.1).
    /// [`MailError::NotFound`] for an unknown account or one whose mail is off.
    pub async fn store(&self, account: &str) -> Result<SharedStore, MailError> {
        self.mailbox_of(account).await?.store()
    }

    pub async fn cache_policy(&self, account: &str) -> Result<MailCachePolicy, MailError> {
        Ok(self.store(account).await?.run(|store| store.cache_policy()).await?)
    }

    pub async fn cache_estimate(&self, account: &str, mode: MailCacheMode) -> Result<MailCacheEstimate, MailError> {
        Ok(self.store(account).await?.run(move |store| store.cache_estimate(mode)).await?)
    }

    pub async fn set_cache_policy(&self, policy: MailCachePolicy) -> Result<MailCachePolicy, MailError> {
        if policy.attachment_retention_days == 0 || policy.attachment_retention_days > 365 {
            return Err(MailError::Store(MailStoreError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "attachment retention must be 1–365 days",
            ))));
        }
        let store = self.store(&policy.account).await?;
        let saved = policy.clone();
        store.run(move |store| store.set_cache_policy(&saved)).await?;
        Ok(policy)
    }

    pub async fn cache_job_status(&self, account: &str) -> Result<Option<MailCacheJob>, MailError> {
        let store = self.store(account).await?;
        let mut job = store.run(|store| store.cache_job()).await?;
        if let Some(held) = &mut job
            && held.state == MailCacheJobState::Running
        {
            let active = self.cache_workers.lock().unwrap_or_else(PoisonError::into_inner).contains(account);
            if !active {
                held.state = MailCacheJobState::Paused;
                let saved = held.clone();
                store.run(move |store| store.set_cache_job(&saved)).await?;
            }
        }
        Ok(job)
    }

    pub async fn start_cache_job(
        self: &Arc<Self>,
        account: &str,
        action: MailCacheAction,
    ) -> Result<MailCacheJob, MailError> {
        let _one = self.cache_jobs_lock.lock().await;
        let store = self.store(account).await?;
        if let Some(existing) = self.cache_job_status(account).await?
            && existing.state == MailCacheJobState::Running
        {
            return Ok(existing);
        }
        let policy = store.run(|store| store.cache_policy()).await?;
        let estimate = store.run(move |store| store.cache_estimate(policy.mode)).await?;
        let total = match action {
            MailCacheAction::ApplyPolicy => estimate.bodies_to_fetch + estimate.bodies_to_remove,
            MailCacheAction::FetchMissingBodies => estimate.bodies_to_fetch,
            MailCacheAction::FetchAttachments => store.run(|store| store.attachment_message_count()).await?,
            MailCacheAction::CleanCache => estimate.bodies_to_remove,
            MailCacheAction::RebuildIndex => store.run(|store| store.reset_index()).await?,
        };
        let estimated_bytes = match action {
            MailCacheAction::ApplyPolicy => estimate.estimated_download_bytes + estimate.estimated_reclaim_bytes,
            MailCacheAction::FetchMissingBodies => estimate.estimated_download_bytes,
            MailCacheAction::FetchAttachments => total.saturating_mul(128 * 1024),
            MailCacheAction::CleanCache => estimate.estimated_reclaim_bytes,
            MailCacheAction::RebuildIndex => estimate.current_bodies.saturating_mul(16 * 1024),
        };
        let job = MailCacheJob {
            id: format!("cache:{}", super::random_hex(12)?),
            account: account.to_string(),
            action,
            state: MailCacheJobState::Running,
            total,
            processed: 0,
            cursor: None,
            estimated_bytes,
            error: None,
        };
        let saved = job.clone();
        store.run(move |store| store.set_cache_job(&saved)).await?;
        self.spawn_cache_worker(account);
        Ok(job)
    }

    pub async fn control_cache_job(
        self: &Arc<Self>,
        account: &str,
        id: &str,
        resume: bool,
    ) -> Result<Option<MailCacheJob>, MailError> {
        let _one = self.cache_jobs_lock.lock().await;
        let store = self.store(account).await?;
        let Some(mut job) = self.cache_job_status(account).await? else { return Ok(None) };
        if job.id != id {
            return Ok(None);
        }
        if resume && job.state != MailCacheJobState::Complete {
            job.state = MailCacheJobState::Running;
            job.error = None;
        } else if !resume && job.state == MailCacheJobState::Running {
            job.state = MailCacheJobState::Paused;
        }
        let saved = job.clone();
        store.run(move |store| store.set_cache_job(&saved)).await?;
        if resume && job.state == MailCacheJobState::Running {
            self.spawn_cache_worker(account);
        }
        Ok(Some(job))
    }

    fn spawn_cache_worker(self: &Arc<Self>, account: &str) {
        let mut active = self.cache_workers.lock().unwrap_or_else(PoisonError::into_inner);
        if !active.insert(account.to_string()) {
            return;
        }
        let service = Arc::clone(self);
        let account = account.to_string();
        tokio::spawn(async move {
            let guard = CacheWorkerGuard { service: Arc::clone(&service), account: account.clone() };
            service.run_cache_job(&account).await;
            drop(guard);
            if let Ok(store) = service.store(&account).await
                && let Ok(Some(job)) = store.run(|store| store.cache_job()).await
                && job.state == MailCacheJobState::Running
            {
                service.spawn_cache_worker(&account);
            }
        });
    }

    async fn run_cache_job(&self, account: &str) {
        let Ok(mailbox) = self.mailbox_of(account).await else { return };
        let Ok(store) = mailbox.store() else { return };
        loop {
            let Ok(Some(mut job)) = store.run(|store| store.cache_job()).await else { return };
            if job.state != MailCacheJobState::Running {
                return;
            }
            let original = job.clone();
            let result: Result<bool, MailError> = match job.action {
                MailCacheAction::ApplyPolicy => {
                    let mode = match store.run(|store| store.cache_policy()).await {
                        Ok(policy) => policy.mode,
                        Err(err) => {
                            self.fail_cache_job(&store, original, job, err.into()).await;
                            return;
                        }
                    };
                    match store.run(move |store| store.prune_bodies_for(mode, Utc::now(), 100)).await {
                        Ok(count) if count > 0 => {
                            job.processed += count as u64;
                            Ok(true)
                        }
                        Ok(_) => match store.run(move |store| store.cached_body_missing(job.cursor, mode, 1)).await {
                            Ok(rows) => match rows.into_iter().next() {
                                Some((cursor, place)) => sync::cache_body(&mailbox, &place).await.map(|_| {
                                    job.cursor = Some(cursor);
                                    job.processed += 1;
                                    true
                                }),
                                None => Ok(false),
                            },
                            Err(err) => Err(err.into()),
                        },
                        Err(err) => Err(err.into()),
                    }
                }
                MailCacheAction::CleanCache => {
                    let mode = match store.run(|store| store.cache_policy()).await {
                        Ok(policy) => policy.mode,
                        Err(err) => {
                            self.fail_cache_job(&store, original, job, err.into()).await;
                            return;
                        }
                    };
                    store
                        .run(move |store| store.prune_bodies_for(mode, Utc::now(), 100))
                        .await
                        .map(|count| {
                            job.processed += count as u64;
                            count > 0
                        })
                        .map_err(Into::into)
                }
                MailCacheAction::RebuildIndex => {
                    sync::backfill(&mailbox, sync::BACKFILL_BATCH, Utc::now()).await.map(|count| {
                        job.processed += count as u64;
                        count > 0
                    })
                }
                MailCacheAction::FetchMissingBodies => {
                    let mode = match store.run(|store| store.cache_policy()).await {
                        Ok(policy) => policy.mode,
                        Err(err) => {
                            self.fail_cache_job(&store, original, job, err.into()).await;
                            return;
                        }
                    };
                    match store.run(move |store| store.cached_body_missing(job.cursor, mode, 1)).await {
                        Ok(rows) => match rows.into_iter().next() {
                            Some((cursor, place)) => sync::cache_body(&mailbox, &place).await.map(|_| {
                                job.cursor = Some(cursor);
                                job.processed += 1;
                                true
                            }),
                            None => Ok(false),
                        },
                        Err(err) => Err(err.into()),
                    }
                }
                MailCacheAction::FetchAttachments => {
                    match store.run(move |store| store.attachment_messages(job.cursor, 1)).await {
                        Ok(rows) => match rows.into_iter().next() {
                            Some((cursor, place)) => {
                                sync::prefetch_message_attachments(&mailbox, &place).await.map(|_| {
                                    job.cursor = Some(cursor);
                                    job.processed += 1;
                                    true
                                })
                            }
                            None => Ok(false),
                        },
                        Err(err) => Err(err.into()),
                    }
                }
            };
            match result {
                Ok(more) => {
                    if !more {
                        job.state = MailCacheJobState::Complete;
                    }
                    let _ = store.run(move |store| store.change_cache_job(&original, &job)).await;
                    if !more {
                        return;
                    }
                }
                Err(err) => {
                    self.fail_cache_job(&store, original, job, err).await;
                    return;
                }
            }
        }
    }

    async fn fail_cache_job(&self, store: &SharedStore, original: MailCacheJob, mut job: MailCacheJob, err: MailError) {
        job.state = MailCacheJobState::Failed;
        job.error = Some(err.message());
        let _ = store.run(move |store| store.change_cache_job(&original, &job)).await;
    }

    /// Last sync and problem for a previously scoped mail account; never exposes credentials.
    pub async fn account_sync_status(&self, account: &str) -> (Option<DateTime<Utc>>, Option<String>) {
        match self.mailbox_of(account).await {
            Ok(mailbox) => {
                let status = mailbox.status();
                (status.last_sync, status.problem)
            }
            Err(_) => (None, None),
        }
    }

    /// The stores of the mail accounts `reach` sees, in the order of `accounts.json`, each with
    /// its account id (task M3.1): for a client only the accounts its rights reach
    /// (`Rights::reaches_mail_account`), so an id is looked up in these and nowhere else. Accounts
    /// whose mail is off, or whose store could not be opened, hold no mail and are left out.
    pub async fn stores(&self, reach: Reach<'_>) -> Vec<(String, SharedStore)> {
        self.entries
            .read()
            .await
            .iter()
            .filter(|entry| entry.mailbox.settings.services.mail && entry.mailbox.within(reach))
            .filter_map(|entry| Some((entry.mailbox.id().to_string(), entry.mailbox.store().ok()?)))
            .collect()
    }

    /// How far the search index of each mail account `reach` sees is (`mail.index_status`, issue
    /// #136), in the order of `accounts.json`; accounts without a store (mail off, not opened)
    /// are left out.
    pub async fn index_progress(&self, reach: Reach<'_>) -> Result<Vec<MailIndexProgress>, MailError> {
        let mailboxes: Vec<Arc<Mailbox>> = self
            .entries
            .read()
            .await
            .iter()
            .filter(|entry| entry.mailbox.settings.services.mail && entry.mailbox.within(reach))
            .map(|entry| Arc::clone(&entry.mailbox))
            .collect();
        let mut progress = Vec::with_capacity(mailboxes.len());
        for mailbox in mailboxes {
            progress.extend(mailbox.index_progress().await?);
        }
        Ok(progress)
    }

    /// How much mail each category holds and how much of it is unread, over every account in
    /// `reach` (task E12.4): the counts behind `inbox.list_categories`, as `(unread, all)`. With
    /// `only` just that account. The inbox folder only ([`MailStore::unread_by_category`]).
    pub async fn unread_by_category(
        &self,
        reach: Reach<'_>,
        only: Option<&str>,
    ) -> Result<std::collections::BTreeMap<torro_mail::Category, (u32, u32)>, MailError> {
        let mut counts: std::collections::BTreeMap<torro_mail::Category, (u32, u32)> = Default::default();
        for mailbox in self.reachable(reach, only).await {
            let Ok(store) = mailbox.store() else { continue };
            for (category, unread, all) in store.run(|store| store.unread_by_category()).await? {
                let held = counts.entry(category).or_insert((0, 0));
                held.0 += unread;
                held.1 += all;
            }
        }
        Ok(counts)
    }

    /// How much mail of `category` every account holds, in any folder (task E15.2): what removing
    /// the category moves. Every account, whatever a client may read – the person removes it for
    /// all of them. A store that cannot be opened is an error, never a 0.
    pub async fn count_category(&self, category: torro_mail::Category) -> Result<u64, MailError> {
        let mut count = 0;
        for mailbox in self.every_mailbox().await {
            count += mailbox.store()?.run(move |store| store.count_category(category)).await?;
        }
        Ok(count)
    }

    /// Files the mail of `category` as `replacement` in every account (task E15.2), keeping where
    /// each category came from. A store that cannot be opened stops it with an error, so the
    /// removal stays noted and is taken up again.
    pub async fn replace_category(
        &self,
        category: torro_mail::Category,
        replacement: torro_mail::Category,
    ) -> Result<usize, MailError> {
        let mut changed = 0;
        for mailbox in self.every_mailbox().await {
            changed += mailbox.store()?.run(move |store| store.replace_category(category, replacement)).await?;
        }
        Ok(changed)
    }

    /// Every mailbox the service holds, its mail on or off: a category is removed from all of
    /// them.
    async fn every_mailbox(&self) -> Vec<Arc<Mailbox>> {
        self.entries.read().await.iter().map(|entry| Arc::clone(&entry.mailbox)).collect()
    }

    /// How far the backlog of each account in `reach` has been sorted (task E12.4): the window the
    /// run uses is the inbox of the last `days` days, at most `limit` messages.
    pub async fn category_progress(
        &self,
        reach: Reach<'_>,
        since: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<(String, u64, u64)>, MailError> {
        let mut progress = Vec::new();
        for mailbox in self.reachable(reach, None).await {
            let Ok(store) = mailbox.store() else { continue };
            let (messages, done) = store.run(move |store| store.category_counts(since, limit)).await?;
            progress.push((mailbox.id().to_string(), messages, done));
        }
        Ok(progress)
    }

    /// Count the selected locally known mail in each reachable account and capture a high-water
    /// sequence for each. The caller checks that explicitly named accounts are in reach.
    pub async fn category_job_preview(
        &self,
        reach: Reach<'_>,
        scope: &torro_ipc::inbox::CategoryJobScope,
    ) -> Result<Vec<(String, torro_ipc::inbox::CategoryJobPreview, i64)>, MailError> {
        let mut result = Vec::new();
        for mailbox in self.reachable(reach, None).await {
            if !scope.accounts.is_empty() && !scope.accounts.iter().any(|id| id == mailbox.id()) {
                continue;
            }
            let store = mailbox.store()?;
            let selected = scope.clone();
            let (preview, maximum) = store.run(move |store| store.category_job_preview(&selected)).await?;
            result.push((mailbox.id().to_string(), preview, maximum));
        }
        Ok(result)
    }

    /// Up to `limit` eligible mail IDs of one account, after its durable cursor.
    pub async fn category_job_batch(
        &self,
        account: &str,
        scope: &torro_ipc::inbox::CategoryJobScope,
        cursor: i64,
        maximum: i64,
        limit: u32,
    ) -> Result<Vec<(i64, String)>, MailError> {
        let store = self.mailbox_of(account).await?.store()?;
        let selected = scope.clone();
        Ok(store.run(move |store| store.category_job_batch(&selected, cursor, maximum, limit)).await?)
    }

    /// Counts every category assignment in accounts this caller may read.
    pub async fn category_clear_preview(&self, reach: Reach<'_>) -> Result<Vec<(String, u64, i64)>, MailError> {
        let mut accounts = Vec::new();
        for mailbox in self.reachable(reach, None).await {
            let (count, maximum) = mailbox.store()?.run(|store| store.category_clear_preview()).await?;
            accounts.push((mailbox.id().to_string(), count, maximum));
        }
        Ok(accounts)
    }

    /// Recount only the accounts frozen into a reset job, without adding newly connected ones.
    pub async fn category_clear_remaining(&self, accounts: &[String]) -> Result<Vec<(String, u64, i64)>, MailError> {
        let mut result = Vec::new();
        for account in accounts {
            let mailbox = self.mailbox_of(account).await?;
            let (count, maximum) = mailbox.store()?.run(|store| store.category_clear_preview()).await?;
            result.push((account.clone(), count, maximum));
        }
        Ok(result)
    }

    /// Clear one bounded account batch, idempotently, leaving every mail and folder in place.
    pub async fn clear_category_batch(
        &self,
        account: &str,
        cursor: i64,
        maximum: i64,
    ) -> Result<(i64, u64), MailError> {
        let store = self.mailbox_of(account).await?.store()?;
        Ok(store.run(move |store| store.clear_category_batch(cursor, maximum, 500)).await?)
    }

    pub async fn clear_model_answers_batch(&self, account: &str) -> Result<u64, MailError> {
        let store = self.mailbox_of(account).await?.store()?;
        Ok(store.run(|store| store.clear_model_answers_batch(500)).await?)
    }

    /// Write an automatic answer only while the mail still has the category the job read.
    pub async fn set_category_if_unchanged(
        &self,
        account: &str,
        message: &str,
        expected: Option<torro_mail::MailCategory>,
        category: torro_mail::MailCategory,
    ) -> Result<bool, MailError> {
        let store = self.mailbox_of(account).await?.store()?;
        let id = message.to_string();
        Ok(store.run(move |store| store.set_category_if_unchanged(&id, expected, category)).await?)
    }

    /// Keeps `category` with every message of `messages` in the account `account` (task E12.4);
    /// `None` takes it away. Messages the store does not know are skipped.
    pub async fn set_category(
        &self,
        account: &str,
        messages: &[String],
        category: Option<torro_mail::MailCategory>,
    ) -> Result<(), MailError> {
        if messages.is_empty() {
            return Ok(());
        }
        let mailbox = self.mailbox_of(account).await?;
        let store = mailbox.store()?;
        let messages = messages.to_vec();
        store
            .run(move |store| {
                for message in &messages {
                    store.set_category(message, category)?;
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Keeps the two answers that come with a category (task E12.6).
    pub async fn set_model_answers(
        &self,
        account: &str,
        message: &str,
        needs_reply: Option<bool>,
        urgency: Option<u8>,
    ) -> Result<(), MailError> {
        let store = self.mailbox_of(account).await?.store()?;
        let message = message.to_string();
        store.run(move |store| store.set_model_answers(&message, needs_reply, urgency).map(|_| ())).await?;
        Ok(())
    }

    /// What a decision model reads of the mail `message` (task E12.6): its headers and the start
    /// of its text, as far as the store holds them. `None` when the store has no such message.
    /// The text is not fetched from the server for this – a mail whose text is not kept yet is
    /// decided on its headers, and a later run sees more.
    pub async fn content_for_category(
        &self,
        account: &str,
        message: &str,
    ) -> Result<Option<crate::categorize::Content>, MailError> {
        let store = self.mailbox_of(account).await?.store()?;
        let wanted = message.to_string();
        let held = store.run(move |store| store.message(&wanted)).await?;
        let Some(message) = held else { return Ok(None) };
        let summary = &message.summary;
        let bulk = message.traits.list_id.is_some()
            || message.traits.precedence.is_some()
            || message.traits.auto_submitted.as_deref().is_some_and(|value| value != "no");
        Ok(Some(crate::categorize::Content {
            subject: summary.subject.clone(),
            from: summary.from.as_ref().map(|from| match &from.name {
                Some(name) => format!("{name} <{}>", from.address),
                None => from.address.clone(),
            }),
            list: message.traits.list_id.clone(),
            bulk,
            date: Some(summary.date),
            // The snippet when the text was never fetched: better than nothing, and a later run
            // sees the whole text once the indexer has it.
            text: message.text.clone().unwrap_or_else(|| summary.snippet.clone()),
        }))
    }

    /// What sorting one mail of `account` again needs (task E15.4): what a model reads, and what
    /// the sorting rules and the headers look at, with the category it has now. `None` when the
    /// store has no such message.
    pub async fn sorting_facts(&self, account: &str, message: &str) -> Result<Option<SortingFacts>, MailError> {
        let store = self.mailbox_of(account).await?.store()?;
        let wanted = message.to_string();
        let held = store.run(move |store| store.message(&wanted)).await?;
        let Some(held) = held else { return Ok(None) };
        let Some(content) = self.content_for_category(account, message).await? else { return Ok(None) };
        Ok(Some(SortingFacts {
            sender: held.summary.from.as_ref().map(|from| from.address.trim().to_lowercase()),
            invite: held.summary.invite.is_some(),
            auto_submitted: held.traits.auto_submitted.clone(),
            category: held.summary.category,
            content,
        }))
    }

    /// The mail of `account` in the window the backlog run uses, category or not, newest first
    /// (task E15.4): what sorting again goes through.
    pub async fn in_window(&self, account: &str, since: DateTime<Utc>, limit: u32) -> Result<Vec<String>, MailError> {
        let store = self.mailbox_of(account).await?.store()?;
        Ok(store.run(move |store| store.in_window(since, limit)).await?)
    }

    /// The mail of `account` that has no category yet, newest first, within the window the
    /// backlog run uses (task E12.6).
    pub async fn uncategorised(
        &self,
        account: &str,
        since: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<String>, MailError> {
        let store = self.mailbox_of(account).await?.store()?;
        Ok(store.run(move |store| store.uncategorised(since, limit)).await?)
    }

    /// The ids of the mail accounts in `reach` that sync mail (task E12.6).
    pub async fn accounts_in(&self, reach: Reach<'_>) -> Vec<String> {
        self.reachable(reach, None).await.iter().map(|mailbox| mailbox.id().to_string()).collect()
    }

    /// The mailboxes in `reach` that sync mail, or just `only` when it is given and in reach.
    ///
    /// **Read access to `mail` is checked here**, not only the account list: what this answers
    /// goes to commands of the area `inbox` (`inbox.list_categories`,
    /// `inbox.get_categorization`, `inbox.catch_up_categories`, task E12.4), so the dispatcher
    /// has weighed `inbox`, never `mail`. A key with the inbox but without mail must see no
    /// number about any mailbox – it gets an empty list, and so zeros everywhere.
    async fn reachable(&self, reach: Reach<'_>, only: Option<&str>) -> Vec<Arc<Mailbox>> {
        if !reads_mail(reach) {
            return Vec::new();
        }
        self.entries
            .read()
            .await
            .iter()
            .filter(|entry| entry.mailbox.settings.services.mail && entry.mailbox.within(reach))
            .filter(|entry| only.is_none_or(|wanted| entry.mailbox.id() == wanted))
            .map(|entry| Arc::clone(&entry.mailbox))
            .collect()
    }

    /// One step of the indexer of `account` by hand ([`sync::backfill`] of up to `limit` texts),
    /// for tests, whose services run no indexer task.
    #[cfg(test)]
    pub(crate) async fn backfill_now(&self, account: &str, limit: u32) -> Result<usize, MailError> {
        let mailbox = self.mailbox_of(account).await?;
        sync::backfill(&mailbox, limit, Utc::now()).await
    }

    /// The ids (`mail:<address>`) of the mail accounts whose mail is on and that `reach` sees, in
    /// the order of `accounts.json` (task M3.2): where the opaque ids of a request (`m:…`, `f:…`)
    /// are looked up, one store after the other. A client never learns of the others.
    pub async fn accounts_within(&self, reach: Reach<'_>) -> Vec<String> {
        self.entries
            .read()
            .await
            .iter()
            .filter(|entry| entry.mailbox.settings.services.mail && entry.mailbox.within(reach))
            .map(|entry| entry.mailbox.id().to_string())
            .collect()
    }

    /// The servers, address and sender name of `account` (task M3.2: the sender of a draft). It
    /// holds no password. [`MailError::NotFound`] for an unknown account or one whose mail is off.
    pub async fn config(&self, account: &str) -> Result<MailAccountConfig, MailError> {
        Ok(self.mailbox_of(account).await?.config.clone())
    }

    /// The managed folder for attachments ([`AttachmentCache`]): where `mail.get_attachment` puts
    /// files (task M3.1) and the only place a client key may attach files from (security rule 1
    /// of task M2.1). `None` without a home directory; then a client key attaches no file at all.
    pub fn attachment_cache(&self) -> Option<AttachmentCache> {
        self.dirs.as_ref().map(AttachmentCache::new)
    }

    /// Notes `list` at the account `id` as its folder sync would ([`Mailbox::note_folders`]), for
    /// tests whose fake server leaves no role open.
    #[cfg(test)]
    pub(crate) async fn note_folders_of(&self, id: &str, list: &FolderList) {
        if let Some(entry) = self.entries.read().await.iter().find(|entry| entry.mailbox.id() == id) {
            entry.mailbox.note_folders(list);
        }
    }

    /// The account's settings (task M3.3: the signature put under a sent message).
    /// [`MailError::NotFound`] for an unknown account or one whose mail is off.
    pub(crate) async fn settings(&self, account: &str) -> Result<MailSettings, MailError> {
        Ok(self.mailbox_of(account).await?.settings.clone())
    }

    /// Who hands the mail of `account` to its SMTP server (task M3.3): its SMTP server, login and
    /// password, after the host was found to be public like every IMAP host
    /// ([`Connector::public_addresses`]); it connects to the addresses checked and nowhere else
    /// (task M2.7b). Nothing is sent yet. [`MailError::Auth`] when the password was
    /// refused before (the same password signs in to SMTP), [`MailError::NoPassword`],
    /// [`MailError::NotPublic`] and [`MailError::Offline`] as for connecting.
    pub(crate) async fn smtp(&self, account: &str) -> Result<SmtpSubmitter, MailError> {
        let mailbox = self.mailbox_of(account).await?;
        if mailbox.refused() {
            return Err(MailError::Auth);
        }
        let password = mailbox.password.clone().ok_or(MailError::NoPassword)?;
        let server = &mailbox.config.smtp;
        let addresses = self.connector.public_addresses(&server.host, server.port).await?;
        Ok(SmtpSubmitter::new(server.clone(), mailbox.account.login_name.clone(), password).pinned_to(addresses))
    }

    /// The SMTP server of `account` refused its password (task M3.3): the account shows "Password
    /// was refused" as for a refused IMAP sign-in, and nothing signs in with that password again
    /// (IMAP or SMTP, [`MailService::smtp`]) until the account is connected anew, so no server
    /// locks the account for repeated tries. An unknown account is nothing to note.
    pub(crate) async fn note_password_refused(&self, account: &str) {
        if let Ok(mailbox) = self.mailbox_of(account).await {
            mailbox.note_failure(&MailError::Auth);
        }
    }

    /// The send journal and the Sent copies waiting to be filed (task M3.3, [`Outbox`]), opened
    /// under `<data>/mail/` on first use and shared by every send, so two sends of the same draft
    /// never run at once. [`MailError::Store`] without a home directory or when it cannot be made.
    pub(crate) async fn outbox(&self) -> Result<Arc<Outbox>, MailError> {
        let dirs = self.dirs.clone().ok_or_else(|| {
            MailError::Store(MailStoreError::Io(io::Error::other("no home directory to keep sent mail in")))
        })?;
        let opened = self
            .outbox
            .get_or_try_init(|| async move {
                match tokio::task::spawn_blocking(move || Outbox::open(&dirs)).await {
                    Ok(opened) => opened.map(Arc::new),
                    Err(err) => Err(MailStoreError::Io(io::Error::other(err.to_string()))),
                }
            })
            .await?;
        Ok(Arc::clone(opened))
    }

    async fn mailbox_of(&self, account: &str) -> Result<Arc<Mailbox>, MailError> {
        self.entries
            .read()
            .await
            .iter()
            .find(|entry| entry.mailbox.id() == account && entry.mailbox.settings.services.mail)
            .map(|entry| Arc::clone(&entry.mailbox))
            .ok_or(MailError::NotFound)
    }
}

/// The health of a mail account with two parts, mail and calendars (task M2.7): a problem of
/// either is the account's (the mail part's first), else unsure while either is, else fine.
fn joined(mail: Health, calendars: Health) -> Health {
    match (mail, calendars) {
        (problem @ Health::Problem { .. }, _) | (_, problem @ Health::Problem { .. }) => problem,
        (Health::Unsure, _) | (_, Health::Unsure) => Health::Unsure,
        (Health::Fine, Health::Fine) => Health::Fine,
    }
}

/// `after` signs in where `before` did: the same id, kind, login and servers (IMAP, SMTP, CalDAV).
/// Only [`MailService::save_account`] may change these, after the servers took the password.
fn same_sign_in(before: &Account, after: &Account) -> bool {
    let servers = |account: &Account| {
        account
            .mail
            .as_ref()
            .map(|mail| (mail.address.clone(), mail.imap.clone(), mail.smtp.clone(), mail.caldav.clone()))
    };
    before.id == after.id
        && before.kind == after.kind
        && before.server == after.server
        && before.login_name == after.login_name
        && servers(before) == servers(after)
}

/// What sorting a mail again looks at (task E15.4, [`MailService::sorting_facts`]): all of it from
/// outside but `category`.
#[derive(Debug, Clone)]
pub struct SortingFacts {
    /// The sender's address in small letters; `None` without one.
    pub sender: Option<String>,
    /// The mail invites to an event.
    pub invite: bool,
    /// What `Auto-Submitted` says, if anything.
    pub auto_submitted: Option<String>,
    /// The category it has now.
    pub category: Option<torro_mail::MailCategory>,
    /// What a model reads of it.
    pub content: crate::categorize::Content,
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use torro_mail::{Endpoint, Flag, Security, Services};

    use super::*;
    use crate::account::github::tests::{MemoryTokens, accounts_file};
    use crate::mail::sync::fake::{self, FakeImap};

    pub(crate) const PASSWORD: &str = "hunter2-imap";

    /// Fast enough for tests: no sync of all folders on its own (only IDLE), retries after 50 ms.
    const TEST_TIMING: Timing = Timing {
        poll: Duration::from_secs(3600),
        idle_max: Duration::from_secs(3600),
        retry_first: Duration::from_millis(50),
        retry_max: Duration::from_millis(200),
        index: false,
    };

    /// The fake server's connections: its test CA, and loopback allowed besides the public
    /// internet. Only tests have this way to `127.0.0.1`. SMTP takes `smtp_password` for any login
    /// at any server, without a connection; what it was asked is kept in `smtp_asked`.
    fn test_connector(smtp_password: &str, smtp_asked: Arc<StdMutex<Vec<String>>>) -> Connector {
        let accepted = Secret::new(smtp_password);
        let check = move |server: &Endpoint, login: &str, password: &Secret| {
            smtp_asked.lock().expect("record").push(format!("{}:{} {login}", server.host, server.port));
            if *password == accepted { Ok(()) } else { Err(SmtpError::Auth) }
        };
        Connector {
            trust: fake::trust(),
            allowed: |ip| is_public(ip) || ip.is_loopback(),
            smtp: SmtpCheck::Fake(Arc::new(check)),
            lookup: Lookup::System,
        }
    }

    /// The service on `dir`: accounts in `dir/config/accounts.json`, stores below `dir`, the
    /// keychain `secrets`, connections to the fake server only; SMTP takes [`PASSWORD`].
    pub(crate) fn service_in(dir: &Path, secrets: &MemoryTokens) -> MailService {
        service_with_smtp(dir, secrets, PASSWORD).0
    }

    /// [`service_in`] with an SMTP server that takes `smtp_password`, and the record of what it was
    /// asked (`host:port login`).
    pub(crate) fn service_with_smtp(
        dir: &Path,
        secrets: &MemoryTokens,
        smtp_password: &str,
    ) -> (MailService, Arc<StdMutex<Vec<String>>>) {
        let dirs = MailDirs::new(&dir.join("data"), &dir.join("cache"));
        let asked = Arc::new(StdMutex::new(Vec::new()));
        let connector = test_connector(smtp_password, Arc::clone(&asked));
        let service =
            MailService::new(Some(accounts_file(dir)), Arc::new(secrets.clone()), Some(dirs), connector, TEST_TIMING);
        (service, asked)
    }

    /// The mail account `address` on `server`, signed in to as `address`.
    pub(crate) fn mail_account(address: &str, server: &Endpoint) -> Account {
        let config = MailAccountConfig {
            address: address.into(),
            sender_name: String::new(),
            imap: server.clone(),
            smtp: Endpoint { host: "smtp.example.org".into(), port: 587, security: Security::StartTls },
            caldav: None,
            trusted_certificates: Vec::new(),
        };
        Account::mail(config, address, "example.org", MailSettings::default()).expect("a valid address")
    }

    /// Stores `accounts` in `dir` and `password` for each of them.
    pub(crate) fn store_accounts(dir: &Path, secrets: &MemoryTokens, accounts: &[(&Account, &str)]) {
        let file = accounts_file(dir);
        std::fs::create_dir_all(file.parent().expect("config dir")).expect("config dir");
        let list: Vec<Account> = accounts.iter().map(|(account, _)| (*account).clone()).collect();
        account::save_to(&file, &list).expect("accounts stored");
        for (account, password) in accounts {
            secrets.put(&account.id, password);
        }
    }

    /// A mailbox for `account` without a worker, for the sync's own tests.
    pub(crate) async fn mailbox_in(dir: &Path, account: Account, password: &str) -> Mailbox {
        let service = service_in(dir, &MemoryTokens::default());
        let config = account.mail.clone().expect("a mail account");
        service.mailbox(account, config, Some(Secret::new(password))).await
    }

    /// Waits up to ten seconds for `check` to hold.
    pub(crate) async fn eventually<F, Fut>(what: &str, mut check: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let until = Instant::now() + Duration::from_secs(10);
        while !check().await {
            assert!(Instant::now() < until, "timed out waiting for: {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn info_of(service: &MailService, id: &str) -> AccountInfo {
        service.infos(Reach::Suite).await.into_iter().find(|info| info.id == id).expect("listed")
    }

    fn state_of(info: &AccountInfo) -> (Option<DateTime<Utc>>, Option<String>) {
        match &info.state {
            AccountState::Connected { last_sync, problem, .. } => (*last_sync, problem.clone()),
            other => panic!("a mail account is connected, got {other:?}"),
        }
    }

    async fn synced(service: &MailService, id: &str) -> bool {
        state_of(&info_of(service, id).await).0.is_some()
    }

    async fn subjects(service: &MailService, id: &str) -> Vec<String> {
        let store = service.store(id).await.expect("store");
        store
            .run(|store| {
                let query = crate::mail::store::SearchQuery::default();
                Ok(store.search(&query, 500, 0)?.into_iter().map(|summary| summary.subject).collect())
            })
            .await
            .expect("searched")
    }

    #[tokio::test]
    async fn new_mail_arrives_through_idle_and_is_announced() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Before", 1, None), &[]);
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir.path(), &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir.path(), &secrets);
        let mut changes = service.subscribe();
        service.reload().await;

        let first = changes.recv().await.expect("the first sync is announced");
        assert!(first.initial && first.new.len() == 1 && first.role == Some(Role::Inbox), "{first:?}");
        eventually("the inbox watched", || async { idling(&server) }).await;
        server.add("INBOX", &fake::mail("Pushed", 0, None), &[]);
        let pushed = changes.recv().await.expect("the new mail is announced");
        assert_eq!((pushed.account.as_str(), pushed.new.len(), pushed.initial), (account.id.as_str(), 1, false));
        assert_eq!(subjects(&service, &account.id).await, ["Pushed", "Before"]);
        let lines = server.lines();
        let idle = lines.iter().position(|line| line.ends_with(" IDLE")).expect("idled");
        assert_eq!(lines[idle + 1], "DONE", "the IDLE ended for the new mail: {lines:?}");
    }

    /// A connection of the fake waits in IDLE (it may have just ended it again).
    fn idling(server: &FakeImap) -> bool {
        server.lines().iter().any(|line| line.ends_with(" IDLE"))
    }

    #[tokio::test]
    async fn a_flag_changed_elsewhere_reaches_the_store_through_idle_and_the_full_sync() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let inbox_uid = server.add("INBOX", &fake::mail("In the inbox", 1, None), &[]);
        let archive_uid = server.add("Archive", &fake::mail("Archived", 2, None), &["\\Seen"]);
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir.path(), &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir.path(), &secrets);
        service.reload().await;
        eventually("the inbox watched", || async { idling(&server) }).await;
        let mut changes = service.subscribe();

        server.set_flags("INBOX", inbox_uid, &["\\Seen", "\\Flagged"]);
        let change = changes.recv().await.expect("announced");
        assert_eq!((change.flags.len(), change.new.len(), change.role), (1, 0, Some(Role::Inbox)));
        let flags_of = |id: String| {
            let service = &service;
            let account = account.id.clone();
            async move {
                let store = service.store(&account).await.expect("store");
                store.run(move |store| Ok(store.summary(&id)?.map(|s| s.flags))).await.expect("read").expect("there")
            }
        };
        assert_eq!(flags_of(change.flags[0].clone()).await, [Flag::Seen, Flag::Flagged]);

        // The archive is not watched: the sync of all folders (every five minutes, or now) sees it.
        server.set_flags("Archive", archive_uid, &[]);
        service.sync_all(Reach::Suite).await.expect("synced");
        let change = changes.recv().await.expect("announced");
        assert_eq!((change.flags.len(), change.role), (1, Some(Role::Archive)));
        assert!(flags_of(change.flags[0].clone()).await.is_empty());
    }

    #[tokio::test]
    async fn a_dropped_connection_is_made_again_and_the_sync_goes_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir.path(), &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir.path(), &secrets);
        service.reload().await;
        eventually("the first sync", || synced(&service, &account.id)).await;
        let logins = server.logins();

        server.drop_connections();
        server.add("INBOX", &fake::mail("After the break", 0, None), &[]);
        eventually("the mail after the break", || async {
            subjects(&service, &account.id).await == ["After the break"]
        })
        .await;
        assert!(server.logins() > logins, "signed in again");
        eventually("no problem left", || async { state_of(&info_of(&service, &account.id).await).1.is_none() }).await;
    }

    #[tokio::test]
    async fn two_accounts_stay_apart_and_a_refused_password_is_a_problem_at_its_account_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (good, bad) = (FakeImap::start(PASSWORD).await, FakeImap::start("another password").await);
        good.add("INBOX", &fake::mail("For Anna", 0, None), &[]);
        bad.add("INBOX", &fake::mail("For Bernd", 0, None), &[]);
        let secrets = MemoryTokens::default();
        let anna = mail_account("anna@example.org", &good.endpoint);
        let bernd = mail_account("bernd@example.org", &bad.endpoint);
        store_accounts(dir.path(), &secrets, &[(&anna, PASSWORD), (&bernd, PASSWORD)]);
        let service = service_in(dir.path(), &secrets);
        service.reload().await;

        eventually("Anna's sync", || synced(&service, &anna.id)).await;
        eventually("Bernd's problem", || async { state_of(&info_of(&service, &bernd.id).await).1.is_some() }).await;
        assert_eq!(subjects(&service, &anna.id).await, ["For Anna"]);
        assert!(subjects(&service, &bernd.id).await.is_empty(), "nothing of Anna's in Bernd's store");
        let (last_sync, problem) = state_of(&info_of(&service, &bernd.id).await);
        assert_eq!((last_sync, problem.as_deref()), (None, Some(tr("Password was refused"))));
        let checks = service.account_checks(Utc::now()).await.expect("loaded");
        assert_eq!(checks[0].health, Health::Fine);
        assert!(matches!(checks[1].health, Health::Problem { trouble: Trouble::SignInRefused, .. }), "{checks:?}");

        // Not tried again on its own, nor by a sync: the server would lock the account.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(bad.logins(), 1);
        let err = service.sync_all(Reach::Suite).await.expect_err("Bernd's account fails the sync");
        assert_eq!(err.code(), "auth");
        assert_eq!(bad.logins(), 1);
        assert!(synced(&service, &anna.id).await, "Anna's account synced anyway");
        let listed = format!("{:?}", service.infos(Reach::Suite).await);
        assert!(!listed.contains(PASSWORD) && !listed.contains("another password"), "{listed}");
        assert!(!bad.lines().iter().any(|line| line.contains(PASSWORD)), "the log of the fake shows no password");
    }

    #[tokio::test]
    async fn an_account_with_mail_switched_off_is_listed_but_never_synced() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let mut account = mail_account("anna@example.org", &server.endpoint);
        account.mail_settings = Some(MailSettings {
            services: Services { mail: false, calendars: true, tasks: true },
            ..Default::default()
        });
        store_accounts(dir.path(), &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir.path(), &secrets);
        service.reload().await;

        assert_eq!(service.infos(Reach::Suite).await.len(), 1);
        assert!(!service.syncs_any(Reach::Suite).await);
        service.sync_all(Reach::Suite).await.expect("nothing to sync");
        assert_eq!(service.store(&account.id).await.map(drop).map_err(|err| err.code()), Err("not-found"));
        assert_eq!(service.command(&account.id).await.map(drop).map_err(|err| err.code()), Err("not-found"));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(server.logins(), 0);
        let checks = service.account_checks(Utc::now()).await.expect("loaded");
        assert_eq!(checks[0].health, Health::Fine);
    }

    /// Task W.2: with mail turned off the account is there but nothing fetches; turned on again,
    /// its worker syncs.
    #[tokio::test]
    async fn mail_turned_off_keeps_the_accounts_and_fetches_nothing_until_it_is_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir.path(), &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir.path(), &secrets);
        service.pause_from_start(true);
        service.reload().await;
        assert_eq!(service.infos(Reach::Suite).await.len(), 1, "the account is listed");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(server.logins(), 0, "nothing is fetched");

        service.set_paused(false).await;
        eventually("the first sync", || synced(&service, &account.id)).await;
        service.set_paused(true).await;
        assert_eq!(service.infos(Reach::Suite).await.len(), 1, "still listed");
    }

    #[tokio::test]
    async fn an_account_without_a_password_asks_to_sign_in_again_and_sends_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir.path(), &secrets, &[]);
        account::save_to(&accounts_file(dir.path()), std::slice::from_ref(&account)).expect("stored");
        let service = service_in(dir.path(), &secrets);
        service.reload().await;

        assert!(service.syncs_any(Reach::Suite).await, "accounts.sync has something to say");
        let err = service.sync_all(Reach::Suite).await.expect_err("no password");
        assert_eq!(
            (err.code(), err.message()),
            ("auth", tr("Sign in again: no password is stored on this device").into())
        );
        let checks = service.account_checks(Utc::now()).await.expect("loaded");
        assert!(matches!(checks[0].health, Health::Problem { trouble: Trouble::SignInNeeded, .. }), "{checks:?}");
        assert_eq!(server.logins(), 0);
    }

    #[tokio::test]
    async fn reloading_keeps_what_did_not_change_and_stops_what_was_removed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir.path(), &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir.path(), &secrets);
        assert!(service.account_checks(Utc::now()).await.is_none(), "not loaded yet");
        service.reload().await;
        eventually("the first sync", || synced(&service, &account.id)).await;
        // The worker signs in a second time for IDLE only after the first sync: count once it idles,
        // or a loaded machine counts before that login and sees it later as one made by the reload.
        eventually("the worker idles", || async { server.lines().iter().any(|line| line.ends_with(" IDLE")) }).await;
        let logins = server.logins();
        service.reload().await;
        assert!(synced(&service, &account.id).await, "the same account keeps its state");
        assert_eq!(server.logins(), logins, "and its connections");

        store_accounts(dir.path(), &secrets, &[]);
        service.reload().await;
        assert!(service.infos(Reach::Suite).await.is_empty());
        assert_eq!(service.store(&account.id).await.map(drop).map_err(|err| err.code()), Err("not-found"));
    }

    #[tokio::test]
    async fn the_command_connection_is_kept_when_released_and_made_anew_when_dropped() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let account = mail_account("anna@example.org", &server.endpoint);
        let mailbox = mailbox_in(dir.path(), account, PASSWORD).await;

        let mut command = mailbox.command().await.expect("connected");
        command.session().noop().await.expect("noop");
        command.release();
        let mut command = mailbox.command().await.expect("the same connection");
        command.session().noop().await.expect("noop");
        assert_eq!(server.logins(), 1);
        drop(command);
        mailbox.command().await.expect("a new connection").release();
        assert_eq!(server.logins(), 2, "a dropped command leaves no session behind");
    }

    #[tokio::test]
    async fn the_core_connects_only_to_public_mail_servers() {
        let connector = Connector::system();
        for host in ["127.0.0.1", "10.0.0.8", "192.168.1.2", "::1", "169.254.1.1"] {
            let refused = connector.public_addresses(host, 993).await.expect_err("refused");
            assert!(matches!(refused, MailError::NotPublic), "{host}: {refused:?}");
        }
        assert!(
            test_connector(PASSWORD, Arc::default()).public_addresses("127.0.0.1", 993).await.is_ok(),
            "only the tests' way allows it"
        );
    }

    /// Task M2.7b: the name is resolved once, for the check, and the session goes to what that
    /// found. The IMAP server is configured as `localhost` on a port where nothing listens; only
    /// the checked address leads to the fake, whose certificate names `localhost` (TLS checks the
    /// host name, not the address). A lookup that finds a private address connects nowhere.
    #[tokio::test]
    async fn the_session_goes_to_the_checked_address_under_the_host_name() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let lookups = Arc::new(StdMutex::new(Vec::new()));
        let answer = Arc::new(StdMutex::new(SocketAddr::from(([127, 0, 0, 1], server.endpoint.port))));
        let (asked, answering) = (Arc::clone(&lookups), Arc::clone(&answer));
        let mut connector = test_connector(PASSWORD, Arc::default());
        connector.lookup = Lookup::Fake(Arc::new(move |host: &str, port: u16| {
            asked.lock().expect("record").push(format!("{host}:{port}"));
            vec![*answering.lock().expect("answer")]
        }));
        let dirs = MailDirs::new(&dir.path().join("data"), &dir.path().join("cache"));
        let service = MailService::new(
            Some(accounts_file(dir.path())),
            Arc::new(MemoryTokens::default()),
            Some(dirs),
            connector,
            TEST_TIMING,
        );
        let configured = Endpoint { host: "localhost".into(), port: 9, security: Security::Tls };
        let account = mail_account("anna@example.org", &configured);
        let config = account.mail.clone().expect("a mail account");
        let mailbox = service.mailbox(account, config, Some(Secret::new(PASSWORD))).await;

        mailbox.connect().await.expect("signed in at the checked address").logout().await.expect("bye");
        assert_eq!((server.logins(), lookups.lock().expect("record").clone()), (1, vec!["localhost:9".to_string()]));

        // The name now points into the local network: refused before anything is sent.
        *answer.lock().expect("answer") = SocketAddr::from(([10, 0, 0, 8], server.endpoint.port));
        assert!(matches!(mailbox.connect().await, Err(MailError::NotPublic)));
        assert_eq!(server.logins(), 1);
    }

    #[test]
    fn errors_carry_api_codes_and_say_what_happened() {
        assert_eq!(MailError::NotFound.code(), "not-found");
        assert_eq!(MailError::Auth.code(), "auth");
        assert_eq!(MailError::Auth.message(), tr("Password was refused"));
        assert_eq!(MailError::from(ImapError::Auth("NO [AUTHENTICATIONFAILED] anna".into())).code(), "auth");
        assert_eq!(MailError::from(ImapError::Offline("gone".into())).code(), "offline");
        assert_eq!(MailError::from(ImapError::NotFound("Old".into())).code(), "not-found");
        let refused = MailError::from(ImapError::Protocol("SELECT \"Geheimprojekt\": NO [ALERT] hi".into()));
        assert!(!refused.message().contains("Geheimprojekt"), "server words stay in the log");
        assert!(refused.to_string().contains("Geheimprojekt"), "the log has them");
        assert_eq!(
            MailError::NotPublic.response(),
            Response::Error {
                code: "error".into(),
                message: tr("The mail server is not on the public internet.").into()
            }
        );
    }

    /// Task M2.7: changing settings never changes where the password goes.
    #[tokio::test]
    async fn an_update_changes_settings_and_never_the_servers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemoryTokens::default();
        // The reload after an update starts the sync: against the fake, never the network.
        let server = FakeImap::start(PASSWORD).await;
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir.path(), &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir.path(), &secrets);

        let signed = service.update(&account.id, |a| {
            a.mail_settings.get_or_insert_with(MailSettings::default).signature = Some("Anna".into());
        });
        let updated = signed.await.expect("written").expect("found");
        assert_eq!(updated.mail_settings.and_then(|s| s.signature).as_deref(), Some("Anna"));
        for steer in [
            (|a: &mut Account| a.mail.as_mut().expect("mail").imap.host = "evil.example".into()) as fn(&mut Account),
            |a| a.mail.as_mut().expect("mail").caldav = Some("https://evil.example/".into()),
            |a| a.login_name = "someone".into(),
        ] {
            assert!(service.update(&account.id, steer).await.is_err());
        }
        assert_eq!(service.update("mail:bob@example.org", |_| {}).await, Ok(None));
        let stored = account::load_from(&accounts_file(dir.path())).expect("read");
        assert_eq!(stored[0].mail, account.mail, "the servers as they were");
        assert_eq!(
            service.password(&account).await.expect("read").map(|p| p.expose().to_string()).as_deref(),
            Some(PASSWORD)
        );
    }

    #[test]
    fn the_parts_of_a_mail_account_make_one_health() {
        let since = Utc::now();
        let problem = Health::Problem { trouble: Trouble::SignInRefused, since };
        assert_eq!(joined(Health::Fine, Health::Fine), Health::Fine);
        assert_eq!(joined(Health::Fine, Health::Unsure), Health::Unsure);
        assert_eq!(joined(Health::Fine, problem), problem);
        assert_eq!(joined(problem, Health::Unsure), problem);
        let failing = Health::Problem { trouble: Trouble::SyncFailing, since };
        assert_eq!(joined(problem, failing), problem, "the mail part's first");
        assert_eq!(SignInError::Refused(Service::Smtp).code(), "auth");
        assert!(SignInError::Refused(Service::Smtp).message().contains("SMTP"));
        assert_eq!(SignInError::caldav(&ProviderError::Offline), SignInError::Offline(Service::CalDav));
    }

    /// A folder listing that leaves `Sent` open between `Sent` and `INBOX.Gesendet` (task M2.8).
    pub(crate) fn sent_left_open() -> FolderList {
        let folder = |path: &str, name: &str| folders::RemoteFolder {
            path: path.into(),
            name: name.into(),
            delimiter: Some('.'),
            role: None,
            selectable: true,
        };
        FolderList {
            folders: vec![folder("Sent", "Sent"), folder("INBOX.Gesendet", "Gesendet")],
            open: vec![OpenRole { role: Role::Sent, candidates: vec!["Sent".into(), "INBOX.Gesendet".into()] }],
            created: Vec::new(),
        }
    }

    /// The account's view (task M2.8) holds what is stored and the open roles of the last folder
    /// listing, with the names to show; the next folder sync notes them anew.
    #[tokio::test]
    async fn the_view_shows_the_open_roles_of_the_last_folder_sync() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let account = mail_account("anna@example.org", &server.endpoint);
        let mailbox = mailbox_in(dir.path(), account, PASSWORD).await;
        let view = mailbox.info().mail.expect("a mail view");
        assert_eq!(
            (view.address.as_str(), &view.imap, view.open_roles.len()),
            ("anna@example.org", &server.endpoint, 0)
        );
        assert_eq!((view.mail.clone(), view.calendars.clone()), (ServiceState::default(), None), "before any sync");

        mailbox.note_folders(&sent_left_open());
        let view = mailbox.info().mail.expect("a mail view");
        let choice = |name: &str, path: &str| FolderChoice { name: name.into(), path: path.into() };
        assert_eq!(
            view.open_roles,
            [OpenRoleView {
                role: Role::Sent,
                candidates: vec![choice("Sent", "Sent"), choice("Gesendet", "INBOX.Gesendet")]
            }]
        );

        // The fake gives every role a folder of its own: after a folder sync nothing is open.
        sync::sync_account(&mailbox).await.expect("synced");
        assert!(mailbox.info().mail.expect("a mail view").open_roles.is_empty());
        assert!(!format!("{:?}", mailbox.info()).contains(PASSWORD));
    }

    #[test]
    fn failing_syncs_become_a_problem_after_a_while_and_a_refused_password_at_once() {
        let now = Utc::now();
        let mut status = Status::default();
        assert_eq!(status.health(now), Health::Unsure);
        status.failed(&MailError::Offline("gone".into()), now);
        assert_eq!(status.health(now), Health::Unsure);
        let later = now + SYNC_PROBLEM_AFTER + chrono::Duration::minutes(1);
        status.failed(&MailError::Offline("still gone".into()), later);
        assert_eq!(status.health(later), Health::Problem { trouble: Trouble::SyncFailing, since: now });
        status.failed(&MailError::Auth, later);
        assert_eq!(status.health(later), Health::Problem { trouble: Trouble::SignInRefused, since: later });
        status.synced(later);
        assert_eq!(status.health(later), Health::Fine);
    }
}
