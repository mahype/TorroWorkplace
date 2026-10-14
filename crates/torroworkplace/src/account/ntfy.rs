//! ntfy accounts (task E5.12, `docs/tasks/eingang.md` › E5): servers whose topics the inbox
//! subscribes to as the source `ntfy:<host>` (task E5.13, `crate::sources`). Connecting one
//! without a login (`accounts.connect_ntfy`), with a login or a new login
//! (`accounts.set_ntfy_login`), its topics (`accounts.set_ntfy_topic`), removing it
//! (`accounts.remove_account`).
//!
//! The protocol is `torro_ntfy`; here the accounts are kept like the AI accounts
//! ([`super::ai`]): the list in `accounts.json` (kind `ntfy`, the server's base address in
//! `server`, topics and user name in `Account::ntfy`), the secret of a login (a password or an
//! access token `tk_…`) in the system keychain under the account id, loaded once in the background
//! ([`NtfyAccounts::start`]). Nothing is stored before the server said that it is an ntfy server
//! and that the topics can be read.
//!
//! The secret leaves the core only towards the account's own server, in the `Authorization`
//! header, and only over `https` or to this computer (`torro_ntfy::Server::may_send_login`); no
//! request follows a redirect. No answer, error, log line or `Debug` holds it.
//!
//! Topic names are passwords on a public server (open question 10 of E5): only the app key sees
//! them ([`NtfyAccounts::infos`] with `names`), clients see the labels; no error message, approval
//! or log line names one. The subscriptions of the inbox (task E5.13, `crate::sources`) get names
//! and login from [`NtfyAccounts::access`], for the account's server only.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, RwLock, watch};
use torro_core::Account;
use torro_core::account::{self, AccountKind, NtfyConfig, NtfyTopic, Secret};
use torro_core::i18n::{tr, trf};
use torro_ipc::{AccountInfo, NtfyTopicView, NtfyView, Response};
use torro_ntfy::{Login, Ntfy, NtfyError, Server, Topic};
use torro_pim::AccountState;

use super::github::{SystemTokens, TokenStore};
use super::{keychain, with_account};

/// The longest label of a topic, in characters.
pub const LABEL_MAX: usize = 60;

/// The most topics one account subscribes to: they all travel in the address of one request.
pub const TOPICS_MAX: usize = 50;

/// Why connecting an ntfy account or changing its topics failed. No variant holds a secret or a
/// topic's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NtfyAccountError {
    /// What the server or `torro_ntfy` said: a wrong address or topic name, a login that would
    /// travel without TLS, a refusal, no answer.
    Ntfy(NtfyError),
    /// No such ntfy account, or no such topic to unsubscribe; the text says which, for people.
    NotFound(String),
    /// The server has an account already (`accounts.connect_ntfy`).
    Conflict,
    /// The request cannot be carried out as it is (a blank label or secret, no topic); the text
    /// is for people.
    Invalid(String),
    /// The keychain or `accounts.json` could not be written.
    Store(String),
}

impl NtfyAccountError {
    /// The error code of the answer: `not-found`, `conflict`, `error`, and for the server's
    /// errors `torro_ntfy::NtfyError::code` (`auth`, `busy`, `offline`, `error`).
    pub fn code(&self) -> &'static str {
        match self {
            NtfyAccountError::Ntfy(err) => err.code(),
            NtfyAccountError::NotFound(_) => "not-found",
            NtfyAccountError::Conflict => "conflict",
            NtfyAccountError::Invalid(_) | NtfyAccountError::Store(_) => "error",
        }
    }

    /// What to tell people, in the core's language. `torro_ntfy`'s own texts are for logs, so the
    /// server's errors are worded here.
    pub fn message(&self) -> String {
        match self {
            NtfyAccountError::Ntfy(err) => match err {
                NtfyError::InvalidServer => {
                    tr("This is not the address of an ntfy server. Enter it without a topic, like ntfy.sh or \
                     https://push.example.org.")
                    .to_string()
                }
                NtfyError::InvalidTopic => {
                    tr("A topic name has 1 to 64 letters, digits, hyphens or underscores.").to_string()
                }
                NtfyError::InsecureLogin => {
                    tr("A login is only sent over https or to this computer. Use an https address.").to_string()
                }
                NtfyError::Auth => {
                    tr("The ntfy server refused: the topics need a login, or this login may not read them.").to_string()
                }
                NtfyError::Busy => tr("The ntfy server asks to wait. Try again in a moment.").to_string(),
                NtfyError::Offline => {
                    tr("The ntfy server could not be reached. Check the address and the connection.").to_string()
                }
                NtfyError::Server(_) => {
                    tr("The server did not answer like an ntfy server. Check the address.").to_string()
                }
            },
            NtfyAccountError::NotFound(message) | NtfyAccountError::Invalid(message) => message.clone(),
            NtfyAccountError::Conflict => {
                tr("This ntfy server is connected already. Add topics on its account page.").to_string()
            }
            NtfyAccountError::Store(message) => message.clone(),
        }
    }

    /// The error answer of a request ([`Response::Error`]).
    pub fn response(&self) -> Response {
        Response::Error { code: self.code().into(), message: self.message() }
    }
}

impl From<NtfyError> for NtfyAccountError {
    fn from(err: NtfyError) -> NtfyAccountError {
        NtfyAccountError::Ntfy(err)
    }
}

/// What the subscription of one ntfy account needs ([`NtfyAccounts::access`], task E5.13).
/// `Debug` shows the server and the labels, never a topic's name (`torro_ntfy::Topic`) or the
/// secret (`torro_ntfy::Login`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtfyAccess {
    /// The account's server.
    pub server: Server,
    /// The topics subscribed, in the account's order.
    pub topics: Vec<AccessTopic>,
    /// The login, `None` for topics anyone may read.
    pub login: Option<Login>,
}

impl NtfyAccess {
    /// The topics' names, as `torro_ntfy::Ntfy::subscribe` takes them.
    pub fn names(&self) -> Vec<Topic> {
        self.topics.iter().map(|topic| topic.name.clone()).collect()
    }

    /// The label the person gave `name`; `None` for a topic the account does not have.
    pub fn label(&self, name: &Topic) -> Option<&str> {
        self.topics.iter().find(|topic| topic.name == *name).map(|topic| topic.label.as_str())
    }

    /// Whether `other` reads the same stream: the same server, topics and login (labels aside), so
    /// a change of labels alone keeps the subscription open.
    pub fn same_stream(&self, other: &NtfyAccess) -> bool {
        self.server == other.server && self.login == other.login && self.names() == other.names()
    }
}

/// A topic of [`NtfyAccess`]: its name, which only the server sees, and the label the person gave
/// it, which the inbox shows (trait `topic`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessTopic {
    pub name: Topic,
    pub label: String,
}

/// What to do with the secret of an account when it is written ([`NtfyAccounts::store`]).
enum SecretChange<'a> {
    /// Leave the keychain as it is (changing topics).
    Keep,
    /// Store this secret, replacing an old one.
    Set(&'a Secret),
    /// Remove a secret that may be left (an account without a login).
    Delete,
}

/// One ntfy account as the core holds it.
#[derive(Debug)]
struct Slot {
    account: Account,
    /// The secret of its login; `None` without a login, or when the keychain did not give it
    /// (`problem` then says so).
    secret: Option<Secret>,
    /// When the server last took the account (connecting, a new login, a new topic).
    checked: Option<DateTime<Utc>>,
    problem: Option<String>,
}

impl Slot {
    fn config(&self) -> NtfyConfig {
        self.account.ntfy.clone().unwrap_or_default()
    }

    /// The account as `accounts.list_accounts` lists it, with the topics' names only for `names`
    /// (the app key), never with the secret.
    fn info(&self, names: bool) -> AccountInfo {
        let config = self.config();
        let topics = config
            .topics
            .iter()
            .map(|topic| NtfyTopicView { label: topic.label.clone(), topic: names.then(|| topic.topic.clone()) })
            .collect();
        AccountInfo {
            id: self.account.id.clone(),
            kind: AccountKind::Ntfy.name().into(),
            label: label(&self.account),
            state: AccountState::Connected {
                server: self.account.server.clone(),
                login_name: self.account.login_name.clone(),
                last_sync: self.checked,
                problem: self.problem.clone(),
            },
            collections: Vec::new(),
            scopes: None,
            mail: None,
            ntfy: Some(NtfyView {
                server: self.account.server.clone(),
                topics,
                user: config.user,
                login: self.secret.is_some(),
            }),
        }
    }
}

/// What to call an ntfy account: its host (`ntfy.sh`), with the user name of a login with a
/// password before it (`anna@push.example.org`).
pub fn label(account: &Account) -> String {
    let host = account.id.strip_prefix("ntfy:").unwrap_or(&account.id);
    match account.ntfy.as_ref().and_then(|config| config.user.as_deref()) {
        Some(user) => format!("{user}@{host}"),
        None => host.to_string(),
    }
}

/// The core's ntfy accounts: loaded at start ([`NtfyAccounts::start`]), connected through
/// `accounts.connect_ntfy` and `accounts.set_ntfy_login`, their topics changed with
/// `accounts.set_ntfy_topic`, removed with `accounts.remove_account`.
pub struct NtfyAccounts {
    /// `accounts.json`; `None` without a home directory, and then nothing can be stored.
    accounts_file: Option<PathBuf>,
    secrets: Arc<dyn TokenStore>,
    /// In the order of `accounts.json`; a new account comes last.
    slots: RwLock<Vec<Slot>>,
    /// One change at a time, from the check ("is there an account?", "does the server take it?")
    /// to the written list, so two connections of the same server never both pass.
    writing: Mutex<()>,
    /// Counts every change of the accounts, their topics or logins, removals included
    /// ([`NtfyAccounts::changes`]).
    changed: watch::Sender<u64>,
}

impl NtfyAccounts {
    /// The service on the user's account list and keychain, as the daemon runs it. Reads and
    /// sends nothing yet; [`NtfyAccounts::start`] loads the accounts.
    pub fn system() -> NtfyAccounts {
        NtfyAccounts::new(account::accounts_file().ok(), Arc::new(SystemTokens))
    }

    /// The service on `accounts_file` and `secrets`. Tests pass a temp file and a map. Reads and
    /// sends nothing yet.
    pub fn new(accounts_file: Option<PathBuf>, secrets: Arc<dyn TokenStore>) -> NtfyAccounts {
        NtfyAccounts {
            accounts_file,
            secrets,
            slots: RwLock::new(Vec::new()),
            writing: Mutex::new(()),
            changed: watch::Sender::new(0),
        }
    }

    /// Loads the stored ntfy accounts with their secrets, in the background. Asks no server. An
    /// account with a user name whose password is missing is listed with a problem and can be
    /// signed in to again or removed.
    pub fn start(self: &Arc<Self>) {
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(err) = service.load().await {
                eprintln!("torroworkplace: cannot read the ntfy accounts: {err}");
            }
        });
    }

    /// Reads the ntfy accounts of `accounts.json` and their secrets.
    async fn load(&self) -> Result<(), String> {
        let Some(path) = self.accounts_file.clone() else { return Ok(()) };
        let secrets = Arc::clone(&self.secrets);
        let loaded = keychain(move || {
            let accounts = account::load_from(&path).map_err(|err| err.to_string())?;
            Ok(accounts
                .into_iter()
                .filter(|account| account.kind == AccountKind::Ntfy)
                .map(|account| {
                    let secret = secrets.get(&account);
                    (account, secret)
                })
                .collect::<Vec<_>>())
        })
        .await?;
        let mut held = self.slots.write().await;
        for (account, secret) in loaded {
            let has_user = account.ntfy.as_ref().is_some_and(|config| config.user.is_some());
            let (secret, problem) = match secret {
                Ok(Some(secret)) => (Some(secret), None),
                Ok(None) if has_user => (None, Some(tr("Connect again: no login is stored on this device").into())),
                Ok(None) => (None, None),
                Err(err) => (None, Some(format!("{} ({err})", tr("Connect again: the keychain could not be read")))),
            };
            // An account connected while loading is newer than the stored one.
            if !held.iter().any(|slot| slot.account.id == account.id) {
                held.push(Slot { account, secret, checked: None, problem });
            }
        }
        drop(held);
        self.changed.send_modify(|count| *count += 1);
        Ok(())
    }

    /// The accounts as `accounts.list_accounts` lists them, never with their secrets; the topics'
    /// names only with `names` (the app key), else their labels alone.
    pub async fn infos(&self, names: bool) -> Vec<AccountInfo> {
        self.slots.read().await.iter().map(|slot| slot.info(names)).collect()
    }

    /// `id` is one of these accounts.
    pub async fn knows(&self, id: &str) -> bool {
        self.slots.read().await.iter().any(|slot| slot.account.id == id)
    }

    /// The label the account `id` keeps for its topic `topic`, for the approval of
    /// `accounts.set_ntfy_topic` (`crate::approvals::Facts::topic`); `None` for an account or topic
    /// it does not have.
    pub async fn label_of(&self, id: &str, topic: &str) -> Option<String> {
        let slots = self.slots.read().await;
        let slot = slots.iter().find(|slot| slot.account.id == id)?;
        slot.config().topics.into_iter().find(|known| known.topic == topic).map(|known| known.label)
    }

    /// Something about the accounts changed: one was loaded, connected, signed in to again, its
    /// topics changed, or it was removed. The value only counts; read the accounts anew on every
    /// change. For the subscriptions (task E5.13): a removed account or a changed topic ends or
    /// renews its subscription.
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// The ids of the accounts held now (`ntfy:<host>`), in the order of `accounts.json`: whose
    /// topics the inbox subscribes to (task E5.13, `crate::sources`).
    pub async fn ids(&self) -> Vec<String> {
        self.slots.read().await.iter().map(|slot| slot.account.id.clone()).collect()
    }

    /// What the subscription of the account `id` needs (task E5.13): its server, its topics with
    /// their labels and its login; `None` for an account the core does not hold (removed).
    ///
    /// For the core's own use only: the topics' names and the secret go to the account's server and
    /// nowhere else (no answer, no log line). A stored name that is no longer a valid topic is left
    /// out. An account with a user name whose password the keychain did not give has no login here;
    /// its server then refuses topics that need one, and the source shows `signed_out` until the
    /// person signs in again.
    pub async fn access(&self, id: &str) -> Option<NtfyAccess> {
        let slots = self.slots.read().await;
        let slot = slots.iter().find(|slot| slot.account.id == id)?;
        let config = slot.config();
        let server = Server::parse(&slot.account.server).ok()?;
        let topics = config
            .topics
            .iter()
            .filter_map(|topic| {
                Some(AccessTopic { name: Topic::parse(&topic.topic).ok()?, label: topic.label.clone() })
            })
            .collect();
        let login = slot.secret.clone().map(|secret| Login::new(config.user.as_deref(), secret));
        Some(NtfyAccess { server, topics, login })
    }

    /// `accounts.connect_ntfy` of the app key: checks the server (`/v1/health`) and the `topics`
    /// without a login, then adds the account `ntfy:<host>` with them and answers it as the app key
    /// lists it. Nothing is stored when a check fails.
    ///
    /// `server` is what a person typed (`torro_ntfy::Server::parse`; empty is `https://ntfy.sh`).
    /// [`NtfyAccountError::Conflict`] when the server has an account already; topics as
    /// [`checked_topics`] has them, at least one.
    pub async fn connect(&self, server: &str, topics: &[NtfyTopic]) -> Result<AccountInfo, NtfyAccountError> {
        let server = Server::parse(server)?;
        let topics = checked_topics(topics)?;
        if topics.is_empty() {
            return Err(no_topics());
        }
        let _writing = self.writing.lock().await;
        let mut account = Account::ntfy(server.as_str(), None);
        if self.knows(&account.id).await {
            return Err(NtfyAccountError::Conflict);
        }
        ask(&server, None, &topics).await?;
        account.ntfy = Some(NtfyConfig { topics, user: None });
        self.store(&account, SecretChange::Delete).await?;
        Ok(self.put(Slot { account, secret: None, checked: Some(Utc::now()), problem: None }).await)
    }

    /// `accounts.set_ntfy_login`: connects the server with a login, or replaces the login of the
    /// account there. The topics checked with it and kept are the account's (if any) with `topics`
    /// added or relabeled ([`merged`]), at least one. `user` names the user of a password; without
    /// one (or a blank one) `secret` is an access token (`torro_ntfy::Login::new`).
    ///
    /// Refused before anything is sent: a blank secret, and a server that is neither `https` nor
    /// this computer (`NtfyError::InsecureLogin`, code `error`). Then the server is asked
    /// (`/v1/health`, the topics with the login); only when it takes them is the secret stored in
    /// the keychain under the account id, then the account in `accounts.json`.
    pub async fn set_login(
        &self,
        server: &str,
        topics: &[NtfyTopic],
        user: Option<&str>,
        secret: &Secret,
    ) -> Result<AccountInfo, NtfyAccountError> {
        let server = Server::parse(server)?;
        let secret = Secret::new(secret.expose().trim());
        if secret.is_blank() {
            return Err(NtfyAccountError::Invalid(tr("Enter the password or the access token.").into()));
        }
        if !server.may_send_login() {
            return Err(NtfyError::InsecureLogin.into());
        }
        let given = checked_topics(topics)?;
        let _writing = self.writing.lock().await;
        let mut account = Account::ntfy(server.as_str(), user);
        let held = self.slots.read().await.iter().find(|slot| slot.account.id == account.id).map(Slot::config);
        let topics = merged(held.map(|config| config.topics).unwrap_or_default(), given);
        if topics.is_empty() {
            return Err(no_topics());
        }
        if topics.len() > TOPICS_MAX {
            return Err(too_many_topics());
        }
        let user = account.ntfy.as_ref().and_then(|config| config.user.clone());
        let login = Login::new(user.as_deref(), secret.clone());
        ask(&server, Some(login), &topics).await?;
        account.ntfy = Some(NtfyConfig { topics, user });
        self.store(&account, SecretChange::Set(&secret)).await?;
        Ok(self.put(Slot { account, secret: Some(secret), checked: Some(Utc::now()), problem: None }).await)
    }

    /// `accounts.set_ntfy_topic`: with `subscribed` a topic the account does not have is checked
    /// with the server (with the account's login) and added with `label`, which it needs; one it
    /// has takes `label`, or stays as it is without one. Without `subscribed` the topic is removed
    /// ([`NtfyAccountError::NotFound`] when the account does not have it). The secret is not
    /// touched.
    pub async fn set_topic(
        &self,
        id: &str,
        topic: &str,
        label: Option<&str>,
        subscribed: bool,
    ) -> Result<(), NtfyAccountError> {
        let name = Topic::parse(topic)?;
        let label = label.map(checked_label).transpose()?;
        let _writing = self.writing.lock().await;
        let held = self.slots.read().await.iter().find(|slot| slot.account.id == id).map(|slot| {
            let login = slot.secret.clone().map(|secret| Login::new(slot.config().user.as_deref(), secret));
            (slot.account.clone(), login)
        });
        let Some((mut account, login)) = held else { return Err(no_account(id)) };
        let mut config = account.ntfy.clone().unwrap_or_default();
        match (subscribed, config.topics.iter().position(|known| known.topic == name.as_str())) {
            (false, None) => return Err(NtfyAccountError::NotFound(tr("The account has no such topic.").into())),
            (false, Some(at)) => {
                config.topics.remove(at);
            }
            (true, Some(at)) => match label {
                Some(label) => config.topics[at].label = label,
                None => return Ok(()),
            },
            (true, None) => {
                let label = label.ok_or_else(|| NtfyAccountError::Invalid(tr("Give the topic a label.").into()))?;
                if config.topics.len() >= TOPICS_MAX {
                    return Err(too_many_topics());
                }
                let added = NtfyTopic { topic: name.as_str().to_string(), label };
                let ntfy = Ntfy::new(Server::parse(&account.server)?, login)?;
                ntfy.check(&[name]).await?;
                config.topics.push(added);
            }
        }
        account.ntfy = Some(config);
        self.store(&account, SecretChange::Keep).await?;
        let mut slots = self.slots.write().await;
        if let Some(slot) = slots.iter_mut().find(|slot| slot.account.id == id) {
            slot.account = account;
            if subscribed {
                slot.checked = Some(Utc::now());
            }
        }
        drop(slots);
        self.changed.send_modify(|count| *count += 1);
        Ok(())
    }

    /// Removes the account `id`, its secret and its entry in `accounts.json`; the other accounts
    /// stay. The secret goes first, as for the other accounts. Its subscription ends with the next
    /// change ([`NtfyAccounts::changes`]).
    pub async fn remove(&self, id: &str) -> Result<(), String> {
        let _writing = self.writing.lock().await;
        let account = self.slots.read().await.iter().find(|slot| slot.account.id == id).map(|s| s.account.clone());
        let Some(account) = account else { return Err(format!("no account {id:?}")) };
        let (secrets, path) = (Arc::clone(&self.secrets), self.accounts_file.clone());
        keychain(move || {
            secrets.delete(&account)?;
            let Some(path) = path else { return Ok(()) };
            let mut accounts = account::load_from(&path).map_err(|err| err.to_string())?;
            accounts.retain(|stored| stored.id != account.id);
            account::save_to(&path, &accounts).map_err(|err| err.to_string())
        })
        .await?;
        self.slots.write().await.retain(|slot| slot.account.id != id);
        self.changed.send_modify(|count| *count += 1);
        Ok(())
    }

    /// The secret as `change` says, then the account into `accounts.json` (next to the others, in
    /// place of the one with the same id): a stored secret nobody lists is better than a listed
    /// account without one.
    async fn store(&self, account: &Account, change: SecretChange<'_>) -> Result<(), NtfyAccountError> {
        let path = self
            .accounts_file
            .clone()
            .ok_or_else(|| NtfyAccountError::Store(tr("No home directory, so the account cannot be stored.").into()))?;
        let (secrets, account) = (Arc::clone(&self.secrets), account.clone());
        let change = match change {
            SecretChange::Keep => None,
            SecretChange::Set(secret) => Some(Some(secret.clone())),
            SecretChange::Delete => Some(None),
        };
        keychain(move || {
            match &change {
                Some(Some(secret)) => secrets.set(&account, secret)?,
                Some(None) => secrets.delete(&account)?,
                None => {}
            }
            let accounts = account::load_from(&path).map_err(|err| err.to_string())?;
            account::save_to(&path, &with_account(accounts, account)).map_err(|err| err.to_string())
        })
        .await
        .map_err(NtfyAccountError::Store)
    }

    /// Holds `slot` in place of the one with the same id, or last; answers it as the app key lists
    /// it.
    async fn put(&self, slot: Slot) -> AccountInfo {
        let info = slot.info(true);
        let mut slots = self.slots.write().await;
        match slots.iter_mut().find(|held| held.account.id == slot.account.id) {
            Some(held) => *held = slot,
            None => slots.push(slot),
        }
        drop(slots);
        self.changed.send_modify(|count| *count += 1);
        info
    }
}

/// Asks `server` whether it is an ntfy server that is up, then whether `topics` can be read with
/// `login`. Errors as `torro_ntfy` has them; `InsecureLogin` before anything is sent.
async fn ask(server: &Server, login: Option<Login>, topics: &[NtfyTopic]) -> Result<(), NtfyAccountError> {
    let names = topics.iter().map(|topic| Topic::parse(&topic.topic)).collect::<Result<Vec<_>, _>>()?;
    let ntfy = Ntfy::new(server.clone(), login)?;
    ntfy.health().await?;
    ntfy.check(&names).await?;
    Ok(())
}

/// `topics` as an account keeps them: every name checked (`torro_ntfy::Topic::parse`), every
/// label as [`checked_label`] has it, each name once (the last label given for it wins, in the
/// place of its first), at most [`TOPICS_MAX`]. An empty list stays empty.
pub fn checked_topics(topics: &[NtfyTopic]) -> Result<Vec<NtfyTopic>, NtfyAccountError> {
    let mut checked: Vec<NtfyTopic> = Vec::new();
    for topic in topics {
        let name = Topic::parse(&topic.topic)?;
        let label = checked_label(&topic.label)?;
        match checked.iter_mut().find(|known| known.topic == name.as_str()) {
            Some(known) => known.label = label,
            None => checked.push(NtfyTopic { topic: name.as_str().to_string(), label }),
        }
    }
    if checked.len() > TOPICS_MAX {
        return Err(too_many_topics());
    }
    Ok(checked)
}

/// `label` trimmed, when it is a single line of 1 to [`LABEL_MAX`] characters without control
/// characters.
pub fn checked_label(label: &str) -> Result<String, NtfyAccountError> {
    let label = label.trim();
    let fits = !label.is_empty() && label.chars().count() <= LABEL_MAX && !label.chars().any(char::is_control);
    match fits {
        true => Ok(label.to_string()),
        false => Err(NtfyAccountError::Invalid(
            trf("Give every topic a label of up to {n} characters.", &[("n", &LABEL_MAX)]).to_string(),
        )),
    }
}

/// `held` with `given` added: a topic of both takes the label given, a new one comes last.
fn merged(mut held: Vec<NtfyTopic>, given: Vec<NtfyTopic>) -> Vec<NtfyTopic> {
    for topic in given {
        match held.iter_mut().find(|known| known.topic == topic.topic) {
            Some(known) => known.label = topic.label,
            None => held.push(topic),
        }
    }
    held
}

fn no_topics() -> NtfyAccountError {
    NtfyAccountError::Invalid(tr("Add at least one topic.").into())
}

fn too_many_topics() -> NtfyAccountError {
    NtfyAccountError::Invalid(trf("An account subscribes to at most {n} topics.", &[("n", &TOPICS_MAX)]).to_string())
}

fn no_account(id: &str) -> NtfyAccountError {
    NtfyAccountError::NotFound(trf("There is no ntfy account {id}.", &[("id", &id)]).to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use httpmock::MockServer;
    use serde_json::json;

    use super::*;
    use crate::account::github::tests::{MemoryTokens, accounts_file};

    pub(crate) const PASSWORD: &str = "ntfy-SECRET-password";
    pub(crate) const TOKEN: &str = "tk_SECRETtoken0123456789abcdef";

    /// The service on `dir/config/accounts.json` and `secrets`.
    pub(crate) fn service(dir: &std::path::Path, secrets: &MemoryTokens) -> NtfyAccounts {
        NtfyAccounts::new(Some(accounts_file(dir)), Arc::new(secrets.clone()))
    }

    pub(crate) fn topic(name: &str, label: &str) -> NtfyTopic {
        NtfyTopic { topic: name.into(), label: label.into() }
    }

    /// An ntfy server that is healthy, lets anyone read `open_backups` and reads `secret_alerts`
    /// only with the user `anna` and [`PASSWORD`] or with [`TOKEN`].
    pub(crate) async fn fake_ntfy() -> MockServer {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/v1/health");
                then.status(200).json_body(json!({"healthy": true}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/open_backups/json").query_param("poll", "1");
                then.status(200).body("");
            })
            .await;
        // Basic anna:PASSWORD, and the token.
        for authorization in ["Basic YW5uYTpudGZ5LVNFQ1JFVC1wYXNzd29yZA==", &format!("Bearer {TOKEN}")] {
            for path in ["/secret_alerts/json", "/open_backups,secret_alerts/json", "/secret_alerts,open_backups/json"]
            {
                server
                    .mock_async(|when, then| {
                        when.method("GET").path(path).query_param("poll", "1").header("authorization", authorization);
                        then.status(200).body("");
                    })
                    .await;
            }
        }
        server
            .mock_async(|when, then| {
                when.method("GET").path_includes("secret_alerts");
                then.status(403).json_body(json!({"code": 40301, "http": 403, "error": "forbidden"}));
            })
            .await;
        server
    }

    fn base(server: &MockServer) -> String {
        format!("http://127.0.0.1:{}", server.port())
    }

    #[test]
    fn topics_and_labels_are_checked_and_each_name_kept_once() {
        let checked = checked_topics(&[topic("a", " First "), topic("b", "Second"), topic("a", "Again")]);
        assert_eq!(checked, Ok(vec![topic("a", "Again"), topic("b", "Second")]));
        assert_eq!(checked_topics(&[topic("a/b", "x")]), Err(NtfyAccountError::Ntfy(NtfyError::InvalidTopic)));
        for label in ["", "  ", "two\nlines", &"x".repeat(LABEL_MAX + 1)] {
            let refused = checked_topics(&[topic("a", label)]);
            assert!(matches!(refused, Err(NtfyAccountError::Invalid(_))), "{label:?}");
        }
        let many: Vec<NtfyTopic> = (0..=TOPICS_MAX).map(|n| topic(&format!("t{n}"), "x")).collect();
        assert!(matches!(checked_topics(&many), Err(NtfyAccountError::Invalid(_))));
        // No error names a topic.
        let error = checked_topics(&[topic("secret_topic", "")]).expect_err("blank label");
        assert!(!format!("{error:?} {}", error.message()).contains("secret_topic"));
    }

    #[tokio::test]
    async fn open_topics_are_connected_without_a_login_and_listed_with_names_for_the_app_key_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemoryTokens::default();
        let server = fake_ntfy().await;
        let ntfy = service(dir.path(), &secrets);

        let info = ntfy.connect(&base(&server), &[topic("open_backups", "Backups")]).await.expect("connected");
        let host = format!("127.0.0.1:{}", server.port());
        assert_eq!(
            (info.id.as_str(), info.kind.as_str(), info.label.as_str()),
            (&*format!("ntfy:{host}"), "ntfy", &*host)
        );
        let view = info.ntfy.clone().expect("an ntfy view");
        assert_eq!((view.server.as_str(), view.login, view.user), (base(&server).as_str(), false, None));
        assert_eq!(view.topics, [NtfyTopicView { label: "Backups".into(), topic: Some("open_backups".into()) }]);
        assert_eq!(secrets.get_plain(&info.id), None, "no login, nothing in the keychain");

        let clients = ntfy.infos(false).await;
        let shown = serde_json::to_string(&clients).expect("json");
        assert!(!shown.contains("open_backups") && shown.contains("Backups"), "{shown}");
        assert!(!format!("{:?}", ntfy.infos(true).await).contains("open_backups"), "no Debug shows a name");

        let again = ntfy.connect(&base(&server), &[topic("open_backups", "Backups")]).await;
        assert_eq!(again.map(|_| ()), Err(NtfyAccountError::Conflict));
        let stored = account::load_from(&accounts_file(dir.path())).expect("stored");
        assert_eq!(stored.len(), 1);
    }

    #[tokio::test]
    async fn a_topic_that_needs_a_login_is_auth_and_a_login_connects_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemoryTokens::default();
        let server = fake_ntfy().await;
        let ntfy = service(dir.path(), &secrets);

        let refused = ntfy.connect(&base(&server), &[topic("secret_alerts", "Alerts")]).await;
        assert_eq!(refused.clone().map(|_| ()), Err(NtfyAccountError::Ntfy(NtfyError::Auth)));
        assert_eq!(refused.expect_err("auth").code(), "auth");
        assert!(ntfy.infos(true).await.is_empty() && !accounts_file(dir.path()).exists(), "nothing stored");

        let wrong = ntfy
            .set_login(&base(&server), &[topic("secret_alerts", "Alerts")], Some("anna"), &Secret::new("wrong"))
            .await;
        assert_eq!(wrong.map(|_| ()), Err(NtfyAccountError::Ntfy(NtfyError::Auth)));

        let info = ntfy
            .set_login(&base(&server), &[topic("secret_alerts", "Alerts")], Some("anna"), &Secret::new(PASSWORD))
            .await
            .expect("signed in");
        assert_eq!(info.label, format!("anna@127.0.0.1:{}", server.port()));
        let view = info.ntfy.clone().expect("view");
        assert_eq!((view.user.as_deref(), view.login), (Some("anna"), true));
        assert_eq!(secrets.get_plain(&info.id).as_deref(), Some(PASSWORD));

        // A new login with a token keeps the topics and adds the one given.
        let info = ntfy
            .set_login(&base(&server), &[topic("open_backups", "Backups")], None, &Secret::new(format!(" {TOKEN} ")))
            .await
            .expect("token");
        let labels: Vec<String> = info.ntfy.expect("view").topics.into_iter().map(|topic| topic.label).collect();
        assert_eq!(labels, ["Alerts", "Backups"]);
        assert_eq!(secrets.get_plain(&info.id).as_deref(), Some(TOKEN), "trimmed, replaced");
        for shown in [
            std::fs::read_to_string(accounts_file(dir.path())).expect("stored"),
            format!("{:?}", ntfy.infos(true).await),
            serde_json::to_string(&ntfy.infos(true).await).expect("json"),
        ] {
            assert!(!shown.contains("SECRET"), "{shown}");
        }
    }

    #[tokio::test]
    async fn a_login_never_travels_over_plain_http_to_another_computer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemoryTokens::default();
        let ntfy = service(dir.path(), &secrets);
        // Refused before anything is sent: nothing answers at this address.
        let refused = ntfy
            .set_login("http://push.example.org", &[topic("alerts", "Alerts")], Some("anna"), &Secret::new(PASSWORD))
            .await
            .expect_err("refused");
        assert_eq!((refused.code(), &refused), ("error", &NtfyAccountError::Ntfy(NtfyError::InsecureLogin)));
        assert!(!format!("{refused:?} {}", refused.message()).contains("SECRET"));
        let blank = ntfy.set_login("ntfy.sh", &[topic("alerts", "Alerts")], None, &Secret::new("  ")).await;
        assert!(matches!(blank, Err(NtfyAccountError::Invalid(_))));
        let path = ntfy.connect("https://ntfy.sh/alerts", &[topic("alerts", "Alerts")]).await;
        assert_eq!(path.map(|_| ()), Err(NtfyAccountError::Ntfy(NtfyError::InvalidServer)));
        let none = ntfy.connect("ntfy.sh", &[]).await;
        assert!(matches!(none, Err(NtfyAccountError::Invalid(_))));
        assert!(secrets.get_plain("ntfy:push.example.org").is_none() && ntfy.infos(true).await.is_empty());
    }

    #[tokio::test]
    async fn the_subscription_gets_server_topics_and_login_and_nothing_once_removed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemoryTokens::default();
        let server = fake_ntfy().await;
        let ntfy = service(dir.path(), &secrets);
        let topics = [topic("secret_alerts", "Alerts"), topic("open_backups", "Backups")];
        let id = ntfy.set_login(&base(&server), &topics, None, &Secret::new(TOKEN)).await.expect("signed in").id;
        assert_eq!(ntfy.ids().await, std::slice::from_ref(&id));

        let access = ntfy.access(&id).await.expect("access");
        assert_eq!(access.server.as_str(), base(&server));
        let names: Vec<&str> = access.topics.iter().map(|topic| topic.name.as_str()).collect();
        assert_eq!(names, ["secret_alerts", "open_backups"]);
        assert_eq!(access.label(&Topic::parse("open_backups").expect("topic")), Some("Backups"));
        assert_eq!(access.login, Some(Login::Token(Secret::new(TOKEN))));
        let shown = format!("{access:?}");
        assert!(!shown.contains("secret_alerts") && !shown.contains("SECRET") && shown.contains("Alerts"), "{shown}");

        // A new label is the same stream, a topic less is another.
        ntfy.set_topic(&id, "open_backups", Some("Nightly"), true).await.expect("relabeled");
        let relabeled = ntfy.access(&id).await.expect("access");
        assert!(relabeled.same_stream(&access) && relabeled != access);
        ntfy.set_topic(&id, "open_backups", None, false).await.expect("unsubscribed");
        assert!(!ntfy.access(&id).await.expect("access").same_stream(&access));

        ntfy.remove(&id).await.expect("removed");
        assert_eq!(ntfy.access(&id).await, None);
        assert!(ntfy.ids().await.is_empty());
    }

    #[tokio::test]
    async fn topics_are_added_relabeled_and_removed_and_accounts_load_and_go() {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemoryTokens::default();
        let server = fake_ntfy().await;
        let ntfy = service(dir.path(), &secrets);
        let mut changes = ntfy.changes();
        let id = ntfy.connect(&base(&server), &[topic("open_backups", "Backups")]).await.expect("connected").id;
        assert!(changes.has_changed().expect("open"), "a connection is a change");
        changes.mark_unchanged();

        // A new topic needs a label, and the server must take it.
        let unlabeled = ntfy.set_topic(&id, "another", None, true).await;
        assert!(matches!(unlabeled, Err(NtfyAccountError::Invalid(_))));
        let refused = ntfy.set_topic(&id, "secret_alerts", Some("Alerts"), true).await;
        assert_eq!(refused, Err(NtfyAccountError::Ntfy(NtfyError::Auth)), "no login for it");
        ntfy.set_topic(&id, "open_backups", Some("Nightly backups"), true).await.expect("relabeled");
        assert_eq!(ntfy.label_of(&id, "open_backups").await.as_deref(), Some("Nightly backups"));
        assert!(changes.has_changed().expect("open"));
        assert_eq!(ntfy.set_topic(&id, "open_backups", None, true).await, Ok(()), "nothing to change");
        let missing = ntfy.set_topic(&id, "never_had", None, false).await;
        assert!(matches!(missing, Err(NtfyAccountError::NotFound(_))));
        let nobody = ntfy.set_topic("ntfy:nowhere", "open_backups", None, false).await;
        assert!(matches!(nobody, Err(NtfyAccountError::NotFound(_))));

        // A fresh service reads what was stored; a user without a password is a problem.
        let mut stored = account::load_from(&accounts_file(dir.path())).expect("stored");
        let mut anna = Account::ntfy("https://push.example.org", Some("anna"));
        anna.ntfy.as_mut().expect("config").topics.push(topic("secret_alerts", "Alerts"));
        stored.push(anna.clone());
        account::save_to(&accounts_file(dir.path()), &stored).expect("saved");
        let loaded = service(dir.path(), &secrets);
        loaded.load().await.expect("loaded");
        let infos = loaded.infos(true).await;
        assert_eq!(infos.len(), 2);
        assert!(matches!(&infos[1].state, AccountState::Connected { problem: Some(problem), .. }
            if problem == tr("Connect again: no login is stored on this device")));
        assert_eq!(loaded.label_of(&id, "open_backups").await.as_deref(), Some("Nightly backups"));

        ntfy.set_topic(&id, "open_backups", None, false).await.expect("unsubscribed");
        assert_eq!(ntfy.label_of(&id, "open_backups").await, None);
        secrets.put(&anna.id, PASSWORD);
        loaded.remove(&anna.id).await.expect("removed");
        assert_eq!(secrets.get_plain(&anna.id), None, "the secret goes with it");
        assert!(!loaded.knows(&anna.id).await && loaded.remove(&anna.id).await.is_err());
        let left = account::load_from(&accounts_file(dir.path())).expect("read");
        assert_eq!(left.iter().map(|account| account.id.as_str()).collect::<Vec<_>>(), [id.as_str()]);
    }
}
