//! Calendars and tasks in the core: the accounts, their CalDAV connections, an in-memory cache
//! per account and the sync loop. The apps reach it through the socket
//! (`torro_ipc::pim::PimClient`).
//!
//! - Every Nextcloud account in `accounts.json` (`crate::account`, secrets in the keychain), each
//!   with its own connection, sync state, problem and cache (task M1.1). Signing in adds an
//!   account; removing one, a failed sync or a remote wipe touches only that one.
//! - **Collection ids name their account:** `<account-id>/<collection URL>`
//!   (`torro_pim::account`). The cache, the answers and the rights use these ids; the CalDAV
//!   client gets the bare URL. Writes find their account through the id, and only through the
//!   exact id of a collection that account has listed (task M1.4): an id that merely starts with
//!   an account's id is no one's, so no request can make the core send an account's app password
//!   to a URL its server did not name. The CalDAV client also refuses every URL off its server.
//!   Item hrefs stay the server's URLs.
//! - The cache holds all collections, all todos and the events from 3 months back to 12
//!   months ahead, fetched on start, every 5 minutes, after a `Sync` request and after
//!   sign-in. Reads are answered from the cache, so the apps never wait for the network;
//!   ranges outside the window are fetched from the server directly.
//! - Writes go to the server first (ETag-guarded) and update the cache with the answer.
//!   Series are refused until series editing exists, because writing one occurrence back would
//!   overwrite the whole series. The core decides what a series is, not the client (task
//!   A.2b): from the cached item in the version the client names, else from the server. An
//!   existing item is only changed, moved or deleted with the `etag` it was read with. Moving
//!   an item to a collection of another account is refused (`unsupported`) for now.
//! - When a server refuses an app password (401/403), the core asks once whether this device
//!   was marked for a remote wipe and, if so, removes that account, its secret and its cache
//!   (task S.12, `account::remote_wipe`).
//! - An account whose app password is missing from the keychain is still loaded, without a
//!   connection and with a problem ("Sign in again"), so it is listed and can be removed.
//! - An SQLite cache that survives restarts comes later.
//! - Who asks decides what is seen ([`Reach`]): collections hidden in the suite and, for a
//!   client key, collections outside its selection are left out before anything is read, so
//!   their events and tasks are neither fetched nor answered.
//! - **GitHub filter lists** (task G.2) are task lists too, `github:<login>/<list-id>`, never
//!   writable. Their definitions live in `filter-lists.json` (config directory,
//!   `torro_pim_github::lists`); each GitHub account has its own [`FilterSlot`] with the lists and
//!   what GitHub last found for them. The core hands in the account's [`Api`] for every fetch
//!   (`crate::core`), so the token stays with the GitHub accounts. A list is fetched whole: when
//!   it is created or changed, every 5 minutes and on `accounts.sync`. For a client key every
//!   issue and pull request is checked against its repositories ([`Rights::reaches_repo`])
//!   before anything else is done with it, and the lists need read access to `github`. Log lines
//!   name lists and items by id only, never a search or a title.
//! - **A client limited to some repositories** (task G.2b) has its searches checked before
//!   GitHub is asked ([`scope::confined`]): a preview, a new list or a changed one that might
//!   search elsewhere is `forbidden` and GitHub hears nothing of it. It sees a list only when all
//!   its searches pass that check, and it never reads GitHub's own error messages, only fixed ones
//!   ([`torro_pim_github::Error::public_message`]). Filtering each match stays as a second guard.
//! - **Ticking an issue of a filter list** (task G.5, [`PimService::save_filter_todo`]) closes it on
//!   GitHub as completed, unticking reopens it; nothing else of an item there can be changed, and
//!   pull requests are never closed (`unsupported`). The issue is the one the cache holds under
//!   the uid, never what the client sends. A client needs the item's repository (else
//!   `not-found`) and write access to `github` (else `forbidden`), both checked before GitHub is
//!   asked. The answer updates the item in every filter list that holds it, so it stays (done)
//!   until the next refresh – and "Undo" finds it.
//! - **Links** (task G.6, `docs/github.md`, "Verweis"): `tasks.add_link` pins an issue or pull
//!   request to a filter list, or writes a task with its address, link id and GitHub's title into
//!   a writable list ([`PimService::add_link`]). The core keeps what GitHub reports about every
//!   linked item in [`State::links`], fetched in one go ([`PimService::reconcile_links`]: one
//!   GraphQL call per 50 items, with the filter lists' refresh), and hands it out in `link.github`
//!   only to whom may see the repository. Title and state go into the stored task when they
//!   changed (closed or merged = done, never back), ETag-guarded, nothing else of it. Ticking a link
//!   to an issue closes it on GitHub too ([`PimService::save_link_todo`]), under the rights of
//!   G.5 and only in a private list (task G.6c); elsewhere it only marks the task done. A client may neither add nor write a link to a repository outside its selection: GitHub
//!   would otherwise tell it the title through the task. Nor may a client without read access to
//!   `github` set or change one at all (task G.6b, [`PimService::link_refusal`]).
//! - **Only private lists are brought in step** (task G.6b): lists the user owns and nobody else
//!   may write to, as the server reports owner and sharees ([`Slot::private`]). Someone who may
//!   write to a list could otherwise place a link to the user's private issue there and read its
//!   title once the core wrote it in – or have the user's tick close an issue they never meant
//!   (task G.6c).
//! - **What the filter list dialog offers** (task G.9): the repositories a GitHub account reaches
//!   ([`PimService::list_github_repos`]) and a repository's labels, open milestones and assignable
//!   people ([`PimService::describe_github_repo`]), read with the account's token and kept in its
//!   [`FilterSlot`] until the next refresh of its filter lists. A client sees only the
//!   repositories it reaches; one outside them is `not-found` before GitHub is asked, and
//!   GitHub's own words never reach it ([`torro_pim_github::GITHUB_READ_FAILED`]). An answer is
//!   kept only when no refresh and no removal of the account came in between, and at most
//!   [`MAX_DESCRIBED`] repositories' details per account (task G.9b).
//! - **CalDAV accounts of mail providers** (task M7.1): `accounts.discover_services` searches the
//!   service of an address without signing in (`torro_pim_caldav::discover`, guarded against
//!   probing private networks), `accounts.connect_caldav` signs in with a password
//!   ([`PimService::connect_caldav`]), stores it through [`SignIn`] and syncs the account like a
//!   Nextcloud, each with its own state. Revoking an app password, the remote wipe and Nextcloud's
//!   placeholder names stay Nextcloud's; a refused password shows as "Password was refused".
//! - **The calendar part of a mail account** (task M2.7): a mail account with a CalDAV address and
//!   calendars or tasks switched on (`crate::account::syncs_calendars`) has a slot like a CalDAV
//!   account, under its own id (`mail:<address>`), and syncs only the collections of the services
//!   switched on ([`Slot::services`]). The core puts it in place when the account is connected or
//!   its services change ([`PimService::put_mail_account`]), taking over the CalDAV account of the
//!   same sign-in in the same step, so the same collections are never synced twice. Its health
//!   goes into the mail account's one check ([`PimService::calendar_health`]). Like the mail
//!   sync it connects to public addresses only, checked on every connection ([`public_http`],
//!   task M2.7b).

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Duration as Span, Local, Utc};
use tokio::sync::{Mutex, RwLock};
use torro_core::account::{AccountKind, Secret};
use torro_core::{AppId, paths::AppPaths};
use torro_ipc::api::{Access, Area, Rights, Selection};
use torro_ipc::{AccountInfo, Request, Response};
use torro_pim::account::{collection_id, local_id};
use torro_pim::model::{GitHubKind, GitHubState, Link};
use torro_pim::source::{GitHubRepo, GitHubRepoDetails, move_plan};
use torro_pim::{
    AccountState, Collection, CollectionKind, Event, EventTime, PimAccount, ProviderError, Todo, combined_state,
};
use torro_pim_caldav::discover::{self, DiscoverError, Discoverer, NoDns};
use torro_pim_caldav::nextcloud;
use torro_pim_caldav::write::item_url;
use torro_pim_caldav::{CalDavAccount, CalDavClient, Services};
use torro_pim_github::api::{self as github_api, MAX_MATCHES};
use torro_pim_github::lists::{self, FilterFile};
use torro_pim_github::{Api, FilterList, IssueRef, address, item, scope};

use torro_core::i18n::{tr, trf};

use crate::account::github::{SystemTokens, TokenStore};
use crate::account::{self, Connected, Stored};
use crate::approvals::{Facts, Known};
use crate::inbox::{AccountCheck, Health, Trouble};
use crate::notify;
use crate::settings_service::SettingsStore;

/// How often the cache is refreshed in the background.
const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);
/// The cached event window around now.
const WINDOW_BACK_DAYS: i64 = 90;
const WINDOW_AHEAD_DAYS: i64 = 365;

/// How long every sync of an account must fail before the inbox shows it as an account problem
/// (task E2.1, [`Trouble::SyncFailing`]): an outage of a few minutes, or a laptop that wakes up
/// before its network, is nothing to bother the user with.
pub const SYNC_PROBLEM_AFTER: Span = Span::minutes(30);

/// One account in the core: who it is, its connection and what is cached of it.
#[derive(Debug)]
struct Slot {
    /// For an account loaded without its app password ([`Stored::Unusable`]) the password is
    /// empty and `client` is `None`.
    connected: Connected,
    /// `None` when no client could be built for the server address or the app password is
    /// missing. Then nothing is sent to its server, not even the revoke when it is removed.
    client: Option<CalDavClient>,
    last_sync: Option<DateTime<Utc>>,
    problem: Option<String>,
    /// What keeps the account from working and since when, for the inbox (task E2.1): set with
    /// `problem` by a failed sync or a missing app password, cleared by a sync or sign-in that
    /// works. `since` stays while the same trouble goes on, so "failing for 30 minutes" counts
    /// from the first failure this core saw (the time is not stored; a restart starts afresh).
    trouble: Option<(Trouble, DateTime<Utc>)>,
    /// With ids `<account-id>/<url>`; the events and todos name them the same way.
    collections: Vec<Collection>,
    /// The ids of the collections in `collections` that are private to the user: theirs and
    /// writable by nobody else, as the server reports it (task G.6b,
    /// `CalDavClient::collections_with_privacy`). Only there are links resolved with the GitHub
    /// token ([`PimService::reconcile_links`]).
    private: Vec<String>,
    events: Vec<Event>,
    /// The range `events` covers; `None` before the first successful sync.
    window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    todos: Vec<Todo>,
    /// The remote wipe check already ran since the server started refusing the app password.
    /// Nextcloud rate-limits the check, and asking again on every failed request would not
    /// change the answer; a successful sync or a new sign-in clears it.
    wipe_checked: bool,
    /// The services a person chose when connecting this CalDAV account (task M7.1): at the first
    /// sync that works, the collections of a service not chosen are hidden ([`chosen`]), then
    /// this is cleared. `None` for every other account and afterwards.
    choice: Option<Services>,
}

impl Slot {
    fn new(connected: Connected, client: Option<CalDavClient>) -> Slot {
        Slot {
            connected,
            client,
            last_sync: None,
            problem: None,
            trouble: None,
            collections: Vec::new(),
            private: Vec::new(),
            events: Vec::new(),
            window: None,
            todos: Vec::new(),
            wipe_checked: false,
            choice: None,
        }
    }

    /// The slot of an account as the core loads it: with a connection when its app password is
    /// there, else without one and with the problem that says to sign in again (task M1.4).
    fn loaded(stored: Stored, http: Option<&reqwest::Client>) -> Slot {
        match stored {
            Stored::Ready(connected) => {
                let client = http.and_then(|http| account::caldav(http, &connected).ok());
                Slot::new(connected, client)
            }
            Stored::Unusable { account, problem } => {
                let trouble = Some((Trouble::SignInNeeded, Utc::now()));
                Slot {
                    problem: Some(problem),
                    trouble,
                    ..Slot::new(Connected { account, app_password: Secret::default() }, None)
                }
            }
        }
    }

    fn id(&self) -> &str {
        &self.connected.account.id
    }

    fn is_nextcloud(&self) -> bool {
        self.connected.account.kind == AccountKind::Nextcloud
    }

    fn is_mail(&self) -> bool {
        self.connected.account.kind == AccountKind::Mail
    }

    /// The services whose collections this account syncs: for a mail account those switched on in
    /// its settings (task M2.7), `None` (all of them) for every other account.
    fn services(&self) -> Option<Services> {
        let account = &self.connected.account;
        (account.kind == AccountKind::Mail).then(|| {
            let services = account.mail_settings.as_ref().map(|settings| settings.services).unwrap_or_default();
            Services { calendars: services.calendars, tasks: services.tasks }
        })
    }

    /// Drops from the cache the collections of a service switched off ([`Slot::services`]), with
    /// their events and tasks, so nobody sees them before the next sync.
    fn keep_chosen(&mut self) {
        let Some(wanted) = self.services() else { return };
        self.collections.retain(|collection| chosen(wanted, collection.kind));
        let kept: Vec<String> = self.collections.iter().map(|collection| collection.id.clone()).collect();
        self.private.retain(|id| kept.contains(id));
        self.events.retain(|event| kept.contains(&event.collection));
        self.todos.retain(|todo| kept.contains(&todo.collection));
    }

    /// The name the user knows the account by: `anna@cloud.example.org` for a Nextcloud; for a
    /// CalDAV account its login when that is an e-mail address, else `anna@<provider>`.
    fn label(&self) -> String {
        let account = &self.connected.account;
        match (account.kind, &account.display_name) {
            (AccountKind::Mail, _) => {
                account.mail.as_ref().map_or_else(|| account.login_name.clone(), |m| m.address.clone())
            }
            (AccountKind::CalDav, _) if account.login_name.contains('@') => account.login_name.clone(),
            (AccountKind::CalDav, Some(provider)) => format!("{}@{provider}", account.login_name),
            _ => format!("{}@{}", account.login_name, host(&account.server)),
        }
    }

    /// The collection URL in `collection`, an id of this account.
    fn url<'a>(&self, collection: &'a str) -> Option<&'a str> {
        local_id(collection, self.id())
    }

    fn state(&self) -> AccountState {
        AccountState::Connected {
            server: self.connected.account.server.clone(),
            login_name: self.connected.account.login_name.clone(),
            last_sync: self.last_sync,
            problem: self.problem.clone(),
        }
    }

    /// How the account stands for the inbox at `now` (task E2.1): a refused or missing sign-in is
    /// a problem at once, failing syncs only after [`SYNC_PROBLEM_AFTER`]; before that, and before
    /// its first sync, it is unsure; after a sync that worked, fine.
    fn check(&self, now: DateTime<Utc>) -> AccountCheck {
        let health = match self.trouble {
            Some((Trouble::SyncFailing, since)) if now - since <= SYNC_PROBLEM_AFTER => Health::Unsure,
            Some((trouble, since)) => Health::Problem { trouble, since },
            None if self.last_sync.is_some() => Health::Fine,
            None => Health::Unsure,
        };
        AccountCheck { account: self.id().to_string(), label: self.label(), health }
    }

    /// The account with all its cached collections, hidden ones included.
    fn info(&self) -> AccountInfo {
        let account = &self.connected.account;
        AccountInfo {
            id: account.id.clone(),
            kind: account.kind.name().into(),
            label: self.label(),
            state: self.state(),
            collections: self.collections.clone(),
            scopes: None,
            mail: None,
            ntfy: None,
        }
    }
}

#[derive(Debug, Default)]
struct State {
    /// In the order of `accounts.json`; a new account comes last.
    accounts: Vec<Slot>,
    /// Server and login URL while Login Flow v2 runs (one at a time).
    signing_in: Option<(String, String)>,
    /// The filter lists of the GitHub accounts the core knows (task G.2).
    filters: Vec<FilterSlot>,
    /// The last [`FilterSlot::generation`] handed out; it only grows, so a slot made again after
    /// its account was removed never has the number of the old one (task G.9b).
    read_generations: u64,
    /// What GitHub last reported about the items the tasks of the accounts link to (task G.6),
    /// by [`link_key`]. For everyone; a client reads it only through [`Reach::with_details`].
    links: HashMap<String, item::Item>,
    /// The stored accounts were read (or could not be), so `accounts` is all there is.
    loaded: bool,
}

impl State {
    fn slot(&self, id: &str) -> Option<&Slot> {
        self.accounts.iter().find(|slot| slot.id() == id)
    }

    fn slot_mut(&mut self, id: &str) -> Option<&mut Slot> {
        self.accounts.iter_mut().find(|slot| slot.id() == id)
    }

    /// The account that has listed `collection` (an id) and holds it in its cache. Exact, not by
    /// the front part of the id (task M1.4): an id like `<anna>/<URL on another server>` belongs
    /// to no one, so a write to it never carries anna's app password there.
    fn owner(&self, collection: &str) -> Option<&Slot> {
        self.accounts.iter().find(|slot| slot.collections.iter().any(|c| c.id == collection))
    }

    /// The collection URL in `collection`, when an account owns it.
    fn url<'a>(&self, collection: &'a str) -> Option<&'a str> {
        self.owner(collection).and_then(|slot| slot.url(collection))
    }

    /// The cached collections of all accounts, the filter lists last.
    fn collections(&self) -> impl Iterator<Item = &Collection> {
        let filters = self.filters.iter().flat_map(|slot| &slot.lists).map(|list| &list.collection);
        self.accounts.iter().flat_map(|slot| &slot.collections).chain(filters)
    }

    /// The filter list `collection` (an id) and its account.
    fn filter(&self, collection: &str) -> Option<(&str, &FilterCache)> {
        self.filters.iter().find_map(|slot| {
            slot.lists.iter().find(|list| list.collection.id == collection).map(|list| (slot.account.as_str(), list))
        })
    }

    fn filter_mut(&mut self, collection: &str) -> Option<&mut FilterCache> {
        self.filters.iter_mut().flat_map(|slot| &mut slot.lists).find(|list| list.collection.id == collection)
    }

    /// The [`FilterSlot`] of `account`, made (in a new generation) when there is none yet. Only
    /// for what the core does on its own account – refreshing, creating a list – never to keep a
    /// GitHub answer that may have outlived the account ([`PimService::read_through`]).
    fn filter_slot_mut(&mut self, account: &str) -> &mut FilterSlot {
        match self.filters.iter().position(|slot| slot.account == account) {
            Some(at) => &mut self.filters[at],
            None => {
                let generation = self.next_read_generation();
                self.filters.push(FilterSlot {
                    account: account.to_string(),
                    lists: Vec::new(),
                    repos: None,
                    described: VecDeque::new(),
                    generation,
                });
                let last = self.filters.len() - 1;
                &mut self.filters[last]
            }
        }
    }

    fn next_read_generation(&mut self) -> u64 {
        self.read_generations += 1;
        self.read_generations
    }

    /// Drops what the filter list dialog read for `account` and starts a new generation of it, so
    /// an answer GitHub gives to a question asked before is not kept (task G.9b).
    fn renew_reads(&mut self, account: &str) {
        let generation = self.next_read_generation();
        let slot = self.filter_slot_mut(account);
        slot.repos = None;
        slot.described.clear();
        slot.generation = generation;
    }
}

/// How many repositories' details ([`FilterSlot::described`]) the core keeps per GitHub account:
/// a dialog needs a few, and a client reaching every repository must not grow the cache without
/// end (task G.9b).
const MAX_DESCRIBED: usize = 64;

/// The filter lists of one GitHub account (task G.2), and what its filter list dialog last read
/// from GitHub (task G.9).
#[derive(Debug)]
struct FilterSlot {
    /// `github:<login>`.
    account: String,
    lists: Vec<FilterCache>,
    /// The repositories the account reaches, as `tasks.list_github_repos` last read them; `None`
    /// until then. For everyone: a client's view is filtered on the way out. Dropped when the
    /// filter lists are refreshed ([`PimService::refresh_filter_lists`]), so it is never older
    /// than one refresh period.
    repos: Option<Vec<GitHubRepo>>,
    /// What `tasks.describe_github_repo` last read, by repository as asked for in lower case
    /// (GitHub ignores case), the oldest first and at most [`MAX_DESCRIBED`]; dropped like `repos`.
    described: VecDeque<(String, GitHubRepoDetails)>,
    /// Which refresh `repos` and `described` belong to ([`State::renew_reads`]): an answer is kept
    /// only in the generation it was asked in (task G.9b).
    generation: u64,
}

impl FilterSlot {
    /// What [`FilterSlot::described`] holds for `key` (lower case).
    fn described(&self, key: &str) -> Option<GitHubRepoDetails> {
        self.described.iter().find(|(held, _)| held == key).map(|(_, details)| details.clone())
    }

    /// Keeps `details` for `key` (lower case) as the newest entry; the oldest goes when there are
    /// more than [`MAX_DESCRIBED`].
    fn describe(&mut self, key: String, details: GitHubRepoDetails) {
        self.described.retain(|(held, _)| *held != key);
        self.described.push_back((key, details));
        while self.described.len() > MAX_DESCRIBED {
            self.described.pop_front();
        }
    }
}

/// One filter list: its definition, how `tasks.list_lists` shows it and what GitHub last found.
#[derive(Debug)]
struct FilterCache {
    list: FilterList,
    /// Built from `list` and the last fetch ([`FilterCache::rebuild`]).
    collection: Collection,
    /// For everyone; a client's view is filtered on the way out ([`Reach::todo_for`]).
    todos: Vec<Todo>,
    truncated: bool,
    problem: Option<String>,
}

impl FilterCache {
    fn new(account: &str, list: FilterList) -> FilterCache {
        let collection = list.collection(account, false, None);
        FilterCache { list, collection, todos: Vec::new(), truncated: false, problem: None }
    }

    /// A new definition; the items stay until the next fetch.
    fn redefine(&mut self, account: &str, list: FilterList) {
        self.list = list;
        self.rebuild(account);
    }

    fn rebuild(&mut self, account: &str) {
        self.collection = self.list.collection(account, self.truncated, self.problem.clone());
    }

    /// What a fetch of the definition `fetched_for` brought. Dropped when the definition changed
    /// meanwhile, so an old search never overwrites the items of a new one; a failure keeps the
    /// items from before.
    fn apply(
        &mut self,
        account: &str,
        fetched_for: &FilterList,
        result: Result<github_api::Fetched, torro_pim_github::Error>,
    ) {
        if self.list.queries != fetched_for.queries || self.list.pinned != fetched_for.pinned {
            return;
        }
        match result {
            Ok(fetched) => {
                self.todos = item::todos(&fetched.items, &self.collection.id);
                self.truncated = fetched.truncated;
                self.problem = None;
            }
            Err(err) => {
                eprintln!("torroworkplace: filter list {} not refreshed: {}", self.collection.id, err.code());
                self.problem = Some(err.to_string());
            }
        }
        self.rebuild(account);
    }
}

/// Whose request is answered, and so which collections it reaches (`docs/api.md`, "Rechte").
#[derive(Debug, Clone, Copy)]
pub enum Reach<'a> {
    /// The suite's own programs: reads see every collection that is not hidden; writes go to
    /// any collection, and the account list shows the hidden ones too, so they can be shown
    /// again.
    Suite,
    /// A client key: reads and writes reach only the shown collections in its selection
    /// ([`Rights::calendars`], [`Rights::task_lists`]); the account list leaves the others out.
    Client(&'a Rights),
}

/// What a collection is asked for as: a calendar (events) or a task list (todos). A collection
/// with both is either, and a client's selection is checked for the role asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Calendar,
    TaskList,
}

impl Reach<'_> {
    /// `collection` serves as `role`, is not `hidden` and is in the selection. A filter list
    /// needs read access to `github` as well, since its items come with the GitHub token, and
    /// searches that stay inside the repositories of this reach ([`Reach::confines`]): its
    /// searches, in `source.queries`, would name the others.
    fn sees(&self, collection: &Collection, role: Role, hidden: &[String]) -> bool {
        if let Some(source) = &collection.source
            && !(self.reads_github() && self.confines(&source.queries))
        {
            return false;
        }
        let fits = match role {
            Role::Calendar => collection.kind.has_events(),
            Role::TaskList => collection.kind.has_todos(),
        };
        let selected = match (self, role) {
            (Reach::Suite, _) => true,
            (Reach::Client(rights), Role::Calendar) => rights.calendars.contains(&collection.id),
            (Reach::Client(rights), Role::TaskList) => rights.task_lists.contains(&collection.id),
        };
        fits && selected && !hidden.contains(&collection.id)
    }

    /// The suite, or a client with read access to `github`.
    fn reads_github(&self) -> bool {
        match self {
            Reach::Suite => true,
            Reach::Client(rights) => rights.area(Area::Github).allows(Access::Read),
        }
    }

    /// Every one of `queries` (searches as `lists::check_queries` makes them) searches only the
    /// repositories this reach sees ([`scope::confined`]). Always for the suite and for a client
    /// with every repository ([`Selection::All`]); they are not limited.
    fn confines(&self, queries: &[String]) -> bool {
        match self {
            Reach::Suite => true,
            Reach::Client(rights) => {
                rights.github_repos == Selection::All
                    || queries.iter().all(|query| scope::confined(query, |name| rights.reaches_repo(name)))
            }
        }
    }

    /// The message of a GitHub failure for this reach: the whole of it for the suite, the fixed
    /// one of its code for a client ([`torro_pim_github::Error::public_message`]).
    fn message_of(&self, err: &torro_pim_github::Error) -> String {
        match self {
            Reach::Suite => err.to_string(),
            Reach::Client(_) => err.public_message(),
        }
    }

    /// Issues and pull requests of `repo` (`owner/name`) are for these eyes
    /// ([`Rights::reaches_repo`]); always for the suite.
    fn reaches_repo(&self, repo: &str) -> bool {
        match self {
            Reach::Suite => true,
            Reach::Client(rights) => rights.reaches_repo(repo),
        }
    }

    /// An item of a filter list as this reach may see it: not at all outside its repositories,
    /// and without a parent of a repository it may not see, whose id would name it.
    fn todo_for(&self, todo: &Todo) -> Option<Todo> {
        let repo_of = |id: &str| address::issue_ref(id).map(|issue| issue.repo);
        if !repo_of(&todo.uid).is_some_and(|repo| self.reaches_repo(&repo)) {
            return None;
        }
        let parent = todo.parent.clone().filter(|parent| repo_of(parent).is_some_and(|repo| self.reaches_repo(&repo)));
        Some(Todo { parent, ..todo.clone() })
    }

    /// A collection as this reach may see it: a filter list without the pinned items of
    /// repositories it may not see, and for a client with a fixed `problem` instead of GitHub's
    /// words ([`torro_pim_github::public_problem`]).
    fn collection_for(&self, mut collection: Collection) -> Collection {
        if let Some(source) = &mut collection.source {
            source.pinned.retain(|id| address::issue_ref(id).is_some_and(|issue| self.reaches_repo(&issue.repo)));
            if let Reach::Client(_) = self {
                source.problem = source.problem.as_deref().map(torro_pim_github::public_problem);
            }
        }
        collection
    }

    /// A task of an account with what GitHub reports about the item it links to (task G.6), from
    /// `links`: only with read access to `github` and the item's repository in reach, since it
    /// came with the token. Every other task, and a link nothing is known about, as it is.
    fn with_details(&self, mut todo: Todo, links: &HashMap<String, item::Item>) -> Todo {
        let known = todo.link.as_ref().and_then(|link| links.get(&link_key(&link.id)));
        if let Some(item) = known.filter(|item| self.reads_github() && self.reaches_repo(item.repo()))
            && let Some(link) = todo.link.as_mut()
        {
            link.github = details_of(item);
        }
        todo
    }

    /// The suite, or a client with write access to `github` and `repo` (`owner/name`) in reach:
    /// who may close and reopen its issues (tasks G.5, G.6).
    fn closes_issues_of(&self, repo: &str) -> bool {
        match self {
            Reach::Suite => true,
            Reach::Client(rights) => rights.area(Area::Github).allows(Access::Write) && rights.reaches_repo(repo),
        }
    }
}

/// The key of a link id in [`State::links`]: GitHub compares repository names without regard to
/// case, so `github:Mahype/Torro#1` and `github:mahype/torro#1` are one item.
fn link_key(id: &str) -> String {
    id.to_ascii_lowercase()
}

/// What GitHub reports about `item`, as `link.github` of a task carries it.
fn details_of(item: &item::Item) -> Option<torro_pim::model::GitHubDetails> {
    item.to_todo("", 0).link.and_then(|link| link.github)
}

/// Where a CalDAV sign-in (task M7.1) searches the service and keeps the account: the account
/// list, the keychain and the discovery. The daemon's are the user's ([`SignIn::system`]); every
/// other service, tests included, gets [`SignIn::offline`], which stores nothing and asks no DNS.
pub struct SignIn {
    /// `accounts.json`; `None` without a home directory, and then nothing can be stored.
    accounts_file: Option<PathBuf>,
    /// Where the password goes: the system keychain, under the account id.
    secrets: Arc<dyn TokenStore>,
    /// Built by `make_discoverer` on first use, since building its HTTP client costs time every
    /// core (and every test) would pay at start; `None` when no HTTP client could be built, and
    /// searching then fails with a message.
    discoverer: OnceLock<Option<Arc<Discoverer>>>,
    make_discoverer: fn() -> Option<Discoverer>,
}

impl SignIn {
    /// The user's account list and keychain, discovery on the system's DNS.
    fn system() -> SignIn {
        SignIn {
            accounts_file: torro_core::account::accounts_file().ok(),
            secrets: Arc::new(SystemTokens),
            discoverer: OnceLock::new(),
            make_discoverer: || {
                Discoverer::system()
                    .map_err(|err| eprintln!("torroworkplace: no HTTP client for finding CalDAV services: {err}"))
                    .ok()
            },
        }
    }

    /// No account list, a keychain that keeps nothing ([`NoKeychain`]) and a DNS that knows no
    /// name ([`NoDns`]): discovery answers from its table alone, a sign-in stores nothing.
    fn offline() -> SignIn {
        SignIn {
            accounts_file: None,
            secrets: Arc::new(NoKeychain),
            discoverer: OnceLock::new(),
            make_discoverer: || Discoverer::new(Arc::new(NoDns)).ok(),
        }
    }

    /// The discovery of this service, built on first use.
    fn discoverer(&self) -> Option<&Discoverer> {
        self.discoverer.get_or_init(|| (self.make_discoverer)().map(Arc::new)).as_deref()
    }
}

impl fmt::Debug for SignIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignIn").field("accounts_file", &self.accounts_file).finish_non_exhaustive()
    }
}

/// A keychain that keeps nothing: for a service that must never store a secret ([`SignIn::offline`],
/// and the mail service of the core's tests).
pub(crate) struct NoKeychain;

impl TokenStore for NoKeychain {
    fn get(&self, _account: &torro_core::Account) -> Result<Option<Secret>, String> {
        Ok(None)
    }

    fn set(&self, _account: &torro_core::Account, _secret: &Secret) -> Result<(), String> {
        Err("this service keeps no passwords".into())
    }

    fn delete(&self, _account: &torro_core::Account) -> Result<(), String> {
        Ok(())
    }
}

/// How the inbox's Nextcloud sources reach one account's server ([`PimService::nextcloud_access`],
/// tasks E5.8, E5.9).
#[derive(Clone)]
pub struct NextcloudAccess {
    /// The OCS client: the account's server, login name and app password (never shown by `Debug`).
    pub client: torro_nextcloud::Nextcloud,
    /// Which sign-in `client` speaks with: the SHA-256 of the app password, so it changes with
    /// every new sign-in. A source that the server refused (401) waits until this differs instead
    /// of asking again with the same password (Nextcloud's brute-force protection would hit the
    /// person). Kept in memory, never logged.
    pub sign_in: [u8; 32],
}

impl fmt::Debug for NextcloudAccess {
    /// Without [`NextcloudAccess::sign_in`]: it is derived from the app password.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NextcloudAccess").field("client", &self.client).finish_non_exhaustive()
    }
}

/// [`NextcloudAccess::sign_in`] of `app_password`.
fn sign_in_of(app_password: &Secret) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(app_password.expose().as_bytes()).into()
}

/// See the module docs.
#[derive(Debug)]
pub struct PimService {
    http: Option<reqwest::Client>,
    /// The HTTP client of the calendar part of mail accounts (task M2.7b, [`public_http`]): it
    /// resolves every name anew, refuses it unless all its addresses are public and connects to
    /// exactly those, as the mail sync does for IMAP. Its CalDAV address was checked the same way
    /// when the account was connected.
    public_http: Option<reqwest::Client>,
    state: RwLock<State>,
    /// One sync of an account at a time.
    syncing: Mutex<()>,
    /// Where the hidden collections are read from, and cleaned up when an account goes.
    settings: SettingsStore,
    /// `filter-lists.json` in the config directory (task G.2); `None` without a home directory,
    /// and then no filter list can be stored.
    filters_file: Option<PathBuf>,
    /// One change of `filter-lists.json` at a time: read, change, write.
    filters_writing: Mutex<()>,
    /// Where CalDAV sign-ins search and store (task M7.1).
    sign_in: SignIn,
}

impl PimService {
    /// The service on the user's settings ([`SettingsStore::system`]), account list and keychain,
    /// as the daemon runs it. Does not touch the network; [`PimService::start`] loads the
    /// accounts.
    pub fn new() -> PimService {
        let filters_file = AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.config.join(lists::FILE));
        PimService { filters_file, sign_in: SignIn::system(), ..PimService::with_settings(SettingsStore::system()) }
    }

    /// The service on `settings`; tests pass a store in a temp directory. Does not touch the
    /// network, and a CalDAV sign-in stores nothing ([`SignIn::offline`]).
    pub fn with_settings(settings: SettingsStore) -> PimService {
        let http = match torro_pim_caldav::http_client() {
            Ok(http) => Some(http),
            Err(err) => {
                eprintln!("torroworkplace: no HTTP client: {err}");
                None
            }
        };
        let public_http = match public_http() {
            Ok(http) => Some(http),
            Err(err) => {
                eprintln!("torroworkplace: no HTTP client for the calendars of mail accounts: {err}");
                None
            }
        };
        PimService {
            http,
            public_http,
            state: RwLock::new(State::default()),
            syncing: Mutex::new(()),
            settings,
            filters_file: None,
            filters_writing: Mutex::new(()),
            sign_in: SignIn::offline(),
        }
    }

    /// The HTTP client for the calendars of `account`: for a mail account's calendar part the one
    /// that connects to public addresses only ([`PimService::public_http`]), else the shared one
    /// (a Nextcloud or a typed CalDAV server may be in the person's own network).
    fn http_for(&self, account: &torro_core::Account) -> Option<&reqwest::Client> {
        match account.kind {
            AccountKind::Mail => self.public_http.as_ref(),
            _ => self.http.as_ref(),
        }
    }

    /// The service on `settings` with its filter lists in `filters_file`, for tests.
    #[cfg(test)]
    pub fn with_filter_file(settings: SettingsStore, filters_file: PathBuf) -> PimService {
        PimService { filters_file: Some(filters_file), ..PimService::with_settings(settings) }
    }

    /// Whether the GitHub account `account` has its [`FilterSlot`] (its first refresh ran), for
    /// tests that must not race that refresh.
    #[cfg(test)]
    pub async fn has_filter_slot(&self, account: &str) -> bool {
        self.state.read().await.filters.iter().any(|slot| slot.account == account)
    }

    /// This service with CalDAV sign-ins storing into `accounts_file` and `secrets` and searching
    /// with `discoverer`, for tests.
    #[cfg(test)]
    pub fn with_sign_in(
        self,
        accounts_file: PathBuf,
        secrets: Arc<dyn TokenStore>,
        discoverer: Discoverer,
    ) -> PimService {
        let sign_in = SignIn {
            accounts_file: Some(accounts_file),
            secrets,
            discoverer: OnceLock::from(Some(Arc::new(discoverer))),
            make_discoverer: || None,
        };
        PimService { sign_in, ..self }
    }

    /// The settings this service reads; the core uses the same ones.
    pub fn settings(&self) -> &SettingsStore {
        &self.settings
    }

    /// Gives hidden collections from before several accounts existed their account (task
    /// M1.1), then loads the stored accounts and syncs them now and every few minutes, for as
    /// long as the daemon runs.
    pub fn start(self: &Arc<Self>) {
        // Before anything is read, so no request ever sees the old ids.
        let accounts_file =
            AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.config.join(torro_core::account::ACCOUNTS_FILE));
        if let Some(owner) = accounts_file.as_deref().and_then(account::legacy_owner)
            && let Err(err) = self.settings.qualify_legacy_collections(&owner)
        {
            eprintln!("torroworkplace: cannot update the ids of hidden collections: {err}");
        }
        let service = Arc::clone(self);
        tokio::spawn(async move {
            match account::keychain(account::stored).await {
                Ok(stored) => {
                    let mut state = service.state.write().await;
                    for stored in stored {
                        // Its secret would be a client's key, which must never go to a server.
                        if crate::clients::is_client_keychain_account(&stored.account().id) {
                            eprintln!("torroworkplace: ignoring a stored account: its id is reserved for agent keys");
                            continue;
                        }
                        let http = service.http_for(stored.account());
                        state.accounts.push(Slot::loaded(stored, http));
                    }
                }
                Err(err) => eprintln!("torroworkplace: cannot read the accounts: {err}"),
            }
            service.state.write().await.loaded = true;
            loop {
                let any = !service.state.read().await.accounts.is_empty();
                if any {
                    // Each account logs its own failure.
                    let _ = service.sync().await;
                }
                tokio::time::sleep(SYNC_EVERY).await;
            }
        });
    }

    /// Answers the calendar, task and account requests with only what `reach` sees: the lists
    /// leave out the other collections, and their events and tasks are not read at all. Other
    /// requests are not ours.
    ///
    /// Writes are not checked here; for a client the core asks [`PimService::out_of_reach`]
    /// first.
    pub async fn handle_within(self: &Arc<Self>, request: Request, reach: Reach<'_>) -> Response {
        match request {
            Request::ConnectNextcloud { server } => self.connect(server).await,
            Request::DiscoverServices { address } => self.discover(&address).await,
            Request::ConnectCalDav { address, server, login_name, password, calendars, tasks } => {
                let wanted = Services { calendars, tasks };
                self.connect_caldav(&address, server.as_deref(), &login_name, password, wanted).await
            }
            // `sync` runs the remote wipe check itself.
            Request::Sync => match self.sync().await {
                Ok(()) => Response::Ok,
                Err(err) => provider_error(&err),
            },
            Request::CalendarCollections => {
                Response::Collections { collections: self.seen(reach, Role::Calendar).await }
            }
            Request::TaskCollections => Response::Collections { collections: self.seen(reach, Role::TaskList).await },
            Request::Events { start, end } => {
                let calendars = ids(self.seen(reach, Role::Calendar).await);
                match self.events(start, end, &calendars).await {
                    Ok(events) => Response::Events { events },
                    Err(failed) => self.failed(failed).await,
                }
            }
            Request::Todos => Response::Todos { todos: self.todos_within(reach).await },
            // The core hands in the account's token (`crate::core`); without it GitHub cannot be asked.
            Request::SaveTodo { todo } if self.is_filter_list(&todo.collection).await => {
                self.save_filter_todo(todo, None, reach).await
            }
            Request::DeleteTodo { todo } if self.is_filter_list(&todo.collection).await => {
                error("unsupported", "the items of a GitHub filter list cannot be deleted; they live on GitHub".into())
            }
            Request::SaveEvent { event } => match self.save_event(event).await {
                Ok(event) => Response::Event { event },
                Err(err) => self.write_failed(err).await,
            },
            Request::DeleteEvent { event } => match self.delete_event(event).await {
                Ok(()) => Response::Ok,
                Err(err) => self.write_failed(err).await,
            },
            Request::SaveTodo { todo } => match self.save_todo(todo).await {
                Ok(todo) => Response::Todo { todo },
                Err(err) => self.write_failed(err).await,
            },
            Request::DeleteTodo { todo } => match self.delete_todo(todo).await {
                Ok(()) => Response::Ok,
                Err(err) => self.write_failed(err).await,
            },
            Request::MoveTodo { href, etag, before } => match self.move_todo(&href, &etag, before.as_deref()).await {
                Ok(todos) => Response::Todos { todos },
                Err(err) => self.write_failed(err).await,
            },
            _ => error("error", "not a calendar or task request".into()),
        }
    }

    /// The tasks of every task list `reach` sees, as `tasks.list_todos` answers them: links with
    /// what GitHub reports for whom may see it (task G.6), the items of filter lists checked
    /// against a client's repositories first. Mail details ([`Link::mail`]) are the core's to add
    /// (task E8.7a); here they are always left out, also when a client sent them along.
    pub async fn todos_within(&self, reach: Reach<'_>) -> Vec<Todo> {
        let lists = ids(self.seen(reach, Role::TaskList).await);
        let state = self.state.read().await;
        let mut todos: Vec<Todo> = state
            .accounts
            .iter()
            .flat_map(|slot| &slot.todos)
            .filter(|t| lists.contains(&t.collection))
            .map(|todo| reach.with_details(todo.clone(), &state.links))
            .collect();
        let filters = state.filters.iter().flat_map(|slot| &slot.lists);
        let seen = filters.filter(|list| lists.contains(&list.collection.id));
        todos.extend(seen.flat_map(|list| &list.todos).filter_map(|todo| reach.todo_for(todo)));
        for link in todos.iter_mut().filter_map(|todo| todo.link.as_mut()) {
            link.mail = None;
        }
        todos
    }

    /// The task lists `reach` sees, as `tasks.list_lists` answers them.
    pub async fn task_lists_within(&self, reach: Reach<'_>) -> Vec<Collection> {
        self.seen(reach, Role::TaskList).await
    }

    /// The cached collections of all accounts that `reach` sees as `role`.
    async fn seen(&self, reach: Reach<'_>, role: Role) -> Vec<Collection> {
        let hidden = self.hidden_collections();
        let state = self.state.read().await;
        state.collections().filter(|c| reach.sees(c, role, &hidden)).map(|c| reach.collection_for(c.clone())).collect()
    }

    /// For a client: the collection (or the item's `href`) a write would touch outside its
    /// reach, which the core answers with `not-found`. `None` when the write stays inside, for
    /// requests that are not writes of events or tasks, and always for the suite.
    ///
    /// The `href` counts too: saving an item with the `href` of another collection moves it
    /// and deletes it there ([`moved`]).
    pub async fn out_of_reach(&self, reach: Reach<'_>, request: &Request) -> Option<String> {
        if let Reach::Suite = reach {
            return None;
        }
        let (role, collection, href) = match request {
            Request::SaveEvent { event } | Request::DeleteEvent { event } => {
                (Role::Calendar, &event.collection, event.href.as_deref())
            }
            Request::SaveTodo { todo } | Request::DeleteTodo { todo }
                if self.is_filter_list(&todo.collection).await =>
            {
                // An item of a filter list: the list must be seen and the item's repository too.
                if !self.sees_task_list(reach, &todo.collection).await {
                    return Some(todo.collection.clone());
                }
                return reach.todo_for(todo).is_none().then(|| todo.uid.clone());
            }
            // A link set or changed to a repository outside the client's: refreshing it would
            // write that item's title from GitHub into the task (task G.6).
            Request::SaveTodo { todo } if self.links_elsewhere(reach, todo).await => {
                return todo.link.as_ref().map(|link| link.id.clone());
            }
            // A mail link set or changed to an account the client may not read (task E8.7a): the
            // suite reports a finished task of a mail that waits, so a client could otherwise make
            // it speak of mail it never saw.
            Request::SaveTodo { todo } if self.links_unread_mail(reach, todo).await => {
                return todo.link.as_ref().and_then(Link::mail_ref).map(|(account, _)| account.to_string());
            }
            // The list as `tasks.save_todo` checks it; the mail by account in the core (task E8.7a).
            Request::CreateTodoFromMail { collection, .. } => (Role::TaskList, collection, None),
            Request::SaveTodo { todo } | Request::DeleteTodo { todo } => {
                (Role::TaskList, &todo.collection, todo.href.as_deref())
            }
            // The list as the client sees it (a filter list only inside its repositories), and the
            // item's repository; one GitHub does not name yet is checked again by the handler.
            Request::AddLink { collection, url } => {
                if !self.sees_task_list(reach, collection).await {
                    return Some(collection.clone());
                }
                let outside = address::issue_ref(url).is_some_and(|issue| !reach.reaches_repo(&issue.repo));
                return outside.then(|| url.clone());
            }
            Request::UpdateFilterList { collection, .. } | Request::DeleteFilterList { collection } => {
                return (!self.sees_task_list(reach, collection).await).then(|| collection.clone());
            }
            Request::UnpinIssue { collection, url } => {
                if !self.sees_task_list(reach, collection).await {
                    return Some(collection.clone());
                }
                let outside = address::issue_ref(url).is_some_and(|issue| !reach.reaches_repo(&issue.repo));
                return outside.then(|| url.clone());
            }
            // The handler leaves out the repositories outside the client's; one it names is
            // `not-found` before GitHub is asked (task G.9).
            Request::ListGithubRepos { .. } => return None,
            // Named as the handler names it, so both answers read the same (task G.9b).
            Request::DescribeGithubRepo { repo, .. } => {
                let repo = repo.trim();
                return (!reach.reaches_repo(repo)).then(|| repo.to_string());
            }
            // Nothing exists yet that could be out of reach. Searches outside the client's
            // repositories are `forbidden`, not `not-found`: the handlers check them before GitHub
            // is asked ([`Reach::confines`]). A new task list goes to an account, which every
            // client sees (`accounts.list_accounts`), and never into a client's selection.
            Request::CreateTaskList { .. } | Request::CreateFilterList { .. } | Request::PreviewFilter { .. } => {
                return None;
            }
            // Both tasks name their list only through the cache; one it does not hold, or one in a
            // list the client may not see, is `not-found`. The tasks renumbered with them are in
            // the same list (`move_todo` refuses `before` in another one).
            Request::MoveTodo { href, before, .. } => {
                let seen = ids(self.seen(reach, Role::TaskList).await);
                let state = self.state.read().await;
                let list_of = |href: &str| cached_todo(&state, href).map(|todo| todo.collection.clone());
                return [Some(href.as_str()), before.as_deref()]
                    .into_iter()
                    .flatten()
                    .find(|href| !list_of(href).is_some_and(|list| seen.contains(&list)))
                    .map(str::to_string);
            }
            Request::Ping
            | Request::Status
            | Request::Quit
            | Request::ListTemplates
            | Request::SaveTemplate { .. }
            | Request::UpdateTemplate { .. }
            | Request::DeleteTemplate { .. }
            | Request::LoadTemplate { .. }
            | Request::ConnectNextcloud { .. }
            | Request::ConnectGithub
            | Request::SetGithubToken { .. }
            | Request::DiscoverServices { .. }
            | Request::ConnectCalDav { .. }
            | Request::Sync
            | Request::CalendarCollections
            | Request::TaskCollections
            | Request::Events { .. }
            | Request::Todos
            | Request::Settings
            | Request::UpdateSettings { .. }
            | Request::SetSetting { .. }
            | Request::SetCollectionShown { .. }
            | Request::Accounts
            | Request::RemoveAccount { .. }
            | Request::RenameTemplate { .. }
            | Request::OpenApp { .. }
            | Request::ListApps
            | Request::ListDesigns
            | Request::ListClients
            | Request::CreateClient { .. }
            | Request::UpdateClient { .. }
            | Request::NewClientKey { .. }
            | Request::RemoveClient { .. }
            | Request::ClientActivity { .. }
            | Request::ListApprovals
            | Request::ApprovalStatus { .. }
            | Request::DecideApproval { .. }
            | Request::AskPermission { .. } => return None,
            // The core checks the task list itself, before the entry and before anything else
            // (`Core::task_target_refusal`, task E3.5), and the calendar of an event alike
            // (`Core::event_refusal`, task E8.5b), and the calendar of an answered invitation (E8.3b).
            Request::CreateTaskFromItem { .. }
            | Request::CreateEventFromItem { .. }
            | Request::RespondToInvitation { .. } => return None,
            // No collection: the inbox filters by the client's sources itself (task E1.2).
            Request::ListItems { .. }
            | Request::GetItem { .. }
            | Request::OpenItem { .. }
            | Request::ListSources
            | Request::PostItem { .. }
            | Request::ResolveItems { .. }
            | Request::SetItemDone { .. }
            | Request::SnoozeItem { .. }
            | Request::SetItemLane { .. }
            | Request::MuteSource { .. }
            | Request::HideSender { .. }
            | Request::ListMuteOptions { .. }
            | Request::MuteItem { .. }
            | Request::ListRules
            | Request::RemoveRule { .. }
            | Request::GetStats { .. }
            | Request::GetQuiet => return None,
            // No collection: mail is checked by account in the core (`Rights::mail_accounts`,
            // task M2.1).
            Request::ConnectMail { .. }
            | Request::UpdateMailAccount { .. }
            | Request::TorroMailAccounts
            | Request::ImportTorroMailAccount { .. }
            | Request::MailFolders { .. }
            | Request::SearchMail { .. }
            | Request::GetMail { .. }
            | Request::GetMailThread { .. }
            | Request::GetMailAttachment { .. }
            | Request::SetMailFlags { .. }
            | Request::MoveMail { .. }
            | Request::DeleteMail { .. }
            | Request::SaveDraft { .. }
            | Request::SendDraft { .. } => return None,
            // No collection: AI accounts have none (task KI.1a).
            Request::ConnectAi { .. } | Request::AiModels { .. } | Request::FindAiServers => return None,
            // No collection: ntfy accounts have topics only (task E5.12).
            Request::ConnectNtfy { .. } | Request::SetNtfyLogin { .. } | Request::SetNtfyTopic { .. } => return None,
            // No collection: the assistant's conversations are its own (task C.1).
            Request::ListEngines
            | Request::ListConversations
            | Request::GetConversation { .. }
            | Request::SendMessage { .. }
            | Request::CancelAnswer { .. }
            | Request::RenameConversation { .. }
            | Request::DeleteConversation { .. }
            | Request::PinConversation { .. }
            | Request::OpenChatWindow { .. }
            | Request::VoiceStatus
            | Request::TestVoice => return None,
            // No collection: the assistant's profile and memory are its own (task C.10).
            Request::GetProfile
            | Request::UpdateProfile { .. }
            | Request::ListMemories
            | Request::MyAccess
            | Request::Remember { .. }
            | Request::Forget { .. } => return None,
            // No collection: bots are the assistant's (task B.1).
            Request::ListBots
            | Request::GetBot { .. }
            | Request::SaveBot { .. }
            | Request::DeleteBot { .. }
            | Request::AskBot { .. }
            | Request::ConfirmBot { .. } => return None,
        };
        let seen = ids(self.seen(reach, role).await);
        if !seen.contains(collection) {
            return Some(collection.clone());
        }
        // Resolved, not compared as text: `…/work/../home/x.ics` names an item in `home`.
        let state = self.state.read().await;
        let inside = |href: &str| seen.iter().filter_map(|id| state.url(id)).any(|url| item_url(url, href).is_some());
        href.filter(|href| !inside(href)).map(str::to_string)
    }

    /// The collections the user hid in the control centre. Read on every request: the file is
    /// tiny and the control centre may change it at any time.
    fn hidden_collections(&self) -> Vec<String> {
        self.settings.current().hidden_collections
    }

    /// `id` is a collection of a connected account, hidden or not.
    pub async fn knows_collection(&self, id: &str) -> bool {
        self.state.read().await.collections().any(|c| c.id == id)
    }

    /// `id` is a task list (a collection with todos) of a connected account, hidden or not.
    pub async fn knows_task_list(&self, id: &str) -> bool {
        self.state.read().await.collections().any(|c| c.id == id && c.kind.has_todos())
    }

    /// `id` is a task list that `reach` sees: shown, in the selection and holding todos. What a
    /// client may not see behaves as if it did not exist.
    pub async fn sees_task_list(&self, reach: Reach<'_>, id: &str) -> bool {
        self.seen(reach, Role::TaskList).await.iter().any(|list| list.id == id)
    }

    /// `id` is a calendar that `reach` sees: shown, in the selection and holding events (task
    /// E8.5b). What a client may not see behaves as if it did not exist.
    pub async fn sees_calendar(&self, reach: Reach<'_>, id: &str) -> bool {
        self.seen(reach, Role::Calendar).await.iter().any(|calendar| calendar.id == id)
    }

    /// `id` is a calendar or a task list that `reach` sees ([`PimService::sees_task_list`], and
    /// the same for calendars): shown, in the selection of its kind and, for a filter list, inside
    /// the client's repositories. For `settings.set_collection_shown` from a client.
    pub async fn sees_collection(&self, reach: Reach<'_>, id: &str) -> bool {
        for role in [Role::Calendar, Role::TaskList] {
            if self.seen(reach, role).await.iter().any(|collection| collection.id == id) {
                return true;
            }
        }
        false
    }

    /// Fills the cache of the first account as a sync would, for the core's handler tests; with
    /// no account yet, [`PimService::pretend_signed_in`] first.
    #[cfg(test)]
    pub async fn cache_collections(&self, collections: Vec<Collection>) {
        self.pretend_signed_in().await;
        let mut state = self.state.write().await;
        if let Some(slot) = state.accounts.first_mut() {
            slot.collections = collections;
        }
    }

    /// Puts `events` into the cache of the first account as a sync would, for the core's handler
    /// tests; [`PimService::cache_collections`] first.
    #[cfg(test)]
    pub async fn cache_events(&self, events: Vec<Event>) {
        if let Some(slot) = self.state.write().await.accounts.first_mut() {
            slot.events = events;
        }
    }

    /// [`PimService::cache_events`] with a cache that covers the days around now (with the margin
    /// of [`covers`]), so that `calendar.list_events` answers from it, for the core's tests of quiet
    /// (task E8.5b). The account needs a CalDAV client ([`tests::fake_caldav`]), as every account
    /// that is read.
    #[cfg(test)]
    pub async fn cache_events_of_today(&self, events: Vec<Event>) {
        let now = Utc::now();
        if let Some(slot) = self.state.write().await.accounts.first_mut() {
            slot.window = Some((now - Span::days(2), now + Span::days(2)));
            slot.events = events;
        }
    }

    /// Stores the account [`tests::ANNA`] as a sign-in would, without network or keychain, for
    /// the core's handler tests. Does nothing when it is there already.
    /// Adds `account` with `password` as a connected account with a CalDAV client, as signing in
    /// does, for tests in other modules.
    #[cfg(test)]
    pub async fn pretend_connected(&self, account: torro_core::Account, password: &str) {
        let connected = Connected { account, app_password: Secret::new(password) };
        let client = self.http.as_ref().and_then(|http| account::caldav(http, &connected).ok());
        self.state.write().await.accounts.push(Slot::new(connected, client));
    }

    #[cfg(test)]
    pub async fn pretend_signed_in(&self) {
        let mut state = self.state.write().await;
        if state.slot(tests::ANNA).is_none() {
            state.accounts.push(Slot::new(tests::connected(), None));
        }
    }

    /// The accounts with their state, as `reach` may see them: for the suite with all
    /// collections, hidden ones included, so the control centre can show them again (task S.2);
    /// for a client only with the collections it sees as calendar or task list.
    pub async fn accounts_within(&self, reach: Reach<'_>) -> Vec<AccountInfo> {
        let mut accounts = account_infos(&*self.state.read().await);
        if let Reach::Client(_) = reach {
            let hidden = self.hidden_collections();
            for account in &mut accounts {
                account
                    .collections
                    .retain(|c| reach.sees(c, Role::Calendar, &hidden) || reach.sees(c, Role::TaskList, &hidden));
            }
        }
        accounts
    }

    /// The ids of the Nextcloud accounts (kind `nextcloud`; neither CalDAV nor mail accounts), in
    /// the order of `accounts.json`, also those without their app password. For the inbox's
    /// Nextcloud sources (tasks E5.8, E5.9, `crate::sources`): one source per id. Empty until the
    /// stored accounts are loaded ([`PimService::start`]).
    pub async fn nextcloud_accounts(&self) -> Vec<String> {
        let state = self.state.read().await;
        state.accounts.iter().filter(|slot| slot.is_nextcloud()).map(|slot| slot.id().to_string()).collect()
    }

    /// What the inbox's Nextcloud sources need to ask the server of account `id` over OCS (tasks
    /// E5.8, E5.9), like [`crate::account::github::GithubAccounts::api`] for GitHub: a client with
    /// the account's server, login name and app password, which stays inside it and the core.
    /// `None` for an unknown id, an account of another kind, one without its app password (it
    /// needs a new sign-in) or a server address the client refuses.
    pub async fn nextcloud_access(&self, id: &str) -> Option<NextcloudAccess> {
        let state = self.state.read().await;
        let slot = state.slot(id).filter(|slot| slot.is_nextcloud())?;
        let Connected { account, app_password } = &slot.connected;
        if app_password.is_blank() {
            return None;
        }
        let client =
            torro_nextcloud::Nextcloud::new(&account.server, &account.login_name, app_password.clone()).ok()?;
        Some(NextcloudAccess { client, sign_in: sign_in_of(app_password) })
    }

    /// Removes the account `id` and nothing else: revokes its app password on the server (best
    /// effort, Nextcloud only: a CalDAV account's password is the user's own), forgets the account
    /// and its secret, drops its cache (task S.2) and takes its collections out of the hidden ones
    /// (task A.0d). Answered with [`Response::Ok`]; an unknown id is `not-found` (task M1.4), a
    /// keychain or file that cannot be changed `error`. An account without its app password is
    /// removed without asking its server.
    pub async fn remove_account(&self, id: &str) -> Response {
        let found = self
            .state
            .read()
            .await
            .slot(id)
            .map(|slot| (slot.connected.clone(), slot.client.is_some() && slot.is_nextcloud()));
        let Some((connected, revocable)) = found else {
            return error("not-found", format!("no account {id:?}"));
        };
        // Revoke first: afterwards the secret is gone and the device would stay valid in
        // the user's "Devices & sessions" for good. Offline or refused only costs a log line.
        match &self.http {
            _ if !revocable => {}
            Some(http) => {
                if let Err(err) = account::revoke(http, &connected).await {
                    eprintln!("torroworkplace: app password not revoked on the server: {err}");
                }
            }
            None => eprintln!("torroworkplace: app password not revoked on the server: no HTTP client"),
        }
        let forget = id.to_string();
        match self.forget_locally(id, || account::keychain(move || account::forget(&forget))).await {
            Ok(()) => Response::Ok,
            Err(message) => error("error", message),
        }
    }

    /// Forgets the account `id` locally: its entry in the account list and its secret
    /// (`forget_store`, in production [`account::forget`] on the keychain thread) first, then its
    /// cache, then its collections in the hidden ones of the settings. The other accounts stay.
    /// Shared by removing an account and by the remote wipe.
    async fn forget_locally<Forget, Fut>(&self, id: &str, forget_store: Forget) -> Result<(), String>
    where
        Forget: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), String>>,
    {
        forget_store().await?;
        let collections: Vec<String> = {
            let mut state = self.state.write().await;
            // A sign-in that runs meanwhile goes on; everything of the removed account goes.
            let Some(at) = state.accounts.iter().position(|slot| slot.id() == id) else {
                return Ok(());
            };
            state.accounts.remove(at).collections.into_iter().map(|c| c.id).collect()
        };
        // Only the cached collections are known. The account is gone either way, so a settings
        // file that cannot be written costs a log line, not the removal.
        if let Err(err) = self.settings.forget_collections(&collections) {
            eprintln!("torroworkplace: hidden collections of the removed account stay in the settings: {err}");
        }
        Ok(())
    }

    /// The response for a failed request; an app password the server refused first runs the
    /// remote wipe check for that account.
    async fn failed(&self, failed: Failed) -> Response {
        if failed.err == ProviderError::Auth
            && let Some(account) = &failed.account
        {
            self.on_refused(account).await;
        }
        provider_error(&failed.err)
    }

    /// The response for a failed write: the core's own refusal as it is, a provider failure as
    /// in [`PimService::failed`].
    async fn write_failed(&self, err: WriteError) -> Response {
        match err {
            WriteError::Provider(failed) => self.failed(failed).await,
            WriteError::Refused { code, message } => error(code, message),
        }
    }

    /// The server of account `id` refused its app password: asks once whether this device is to
    /// be wiped and does so for this account (task S.12). Otherwise the account stays and shows
    /// "sign-in refused" until the user signs in again. Only for Nextcloud: other CalDAV servers
    /// know no remote wipe, and asking them would send the password to a path they never named.
    async fn on_refused(&self, id: &str) {
        let Some(http) = self.http.clone() else {
            return;
        };
        let Some((connected, label)) = self
            .state
            .read()
            .await
            .slot(id)
            .filter(|slot| slot.is_nextcloud())
            .map(|slot| (slot.connected.clone(), slot.label()))
        else {
            return;
        };
        let (server, token) = (connected.account.server.as_str(), connected.app_password.expose());
        let forget = id.to_string();
        let wiped = self
            .wipe_if_requested(
                &connected,
                || nextcloud::check_wipe(&http, server, token),
                || account::keychain(move || account::forget(&forget)),
                || nextcloud::confirm_wipe(&http, server, token),
            )
            .await;
        match wiped {
            Ok(Some(true)) => {
                eprintln!("torroworkplace: the server asked to remove {id} from this device; done");
                notify::show(
                    tr("Account removed").into(),
                    trf(
                        "Your Nextcloud account {account} was removed from this device at the server's request.",
                        &[("account", &label)],
                    ),
                );
            }
            Ok(_) => {}
            Err(err) => eprintln!("torroworkplace: {err}"),
        }
    }

    /// The remote wipe for `connected` with the network and keychain steps passed in, so it can
    /// be tested without either. `Ok(None)` = skipped, because the check already ran for this
    /// refusal or the account changed meanwhile; otherwise the result of
    /// [`account::remote_wipe`]. A failed check may run again on the next refusal.
    async fn wipe_if_requested<Check, CheckFut, Forget, ForgetFut, Report, ReportFut>(
        &self,
        connected: &Connected,
        check: Check,
        forget_store: Forget,
        report: Report,
    ) -> Result<Option<bool>, String>
    where
        Check: FnOnce() -> CheckFut,
        CheckFut: Future<Output = Result<bool, ProviderError>>,
        Forget: FnOnce() -> ForgetFut,
        ForgetFut: Future<Output = Result<(), String>>,
        Report: FnOnce() -> ReportFut,
        ReportFut: Future<Output = Result<(), ProviderError>>,
    {
        let id = connected.account.id.as_str();
        {
            // Test and set under one lock, so concurrent failures ask only once.
            let mut state = self.state.write().await;
            match state.slot_mut(id) {
                Some(slot) if !slot.wipe_checked => slot.wipe_checked = true,
                _ => return Ok(None),
            }
        }
        let wipe = || async {
            // The user may have removed the account meanwhile.
            if self.state.read().await.slot(id).is_none() {
                return Err("the account changed meanwhile".to_string());
            }
            self.forget_locally(id, forget_store).await
        };
        let result = account::remote_wipe(check, wipe, report).await;
        if result.is_err()
            && let Some(slot) = self.state.write().await.slot_mut(id)
        {
            slot.wipe_checked = false;
        }
        result.map(Some)
    }

    /// What an app that shows one account state shows ([`combined_state`]): for the tray.
    pub async fn account_state(&self) -> AccountState {
        let infos = account_infos(&*self.state.read().await);
        let accounts: Vec<PimAccount> = infos
            .into_iter()
            .map(|info| PimAccount {
                id: info.id,
                label: info.label,
                kind: info.kind,
                state: info.state,
                scopes: info.scopes,
            })
            .collect();
        combined_state(&accounts)
    }

    /// How every account stands for the inbox at `now` (task E2.1, [`crate::inbox::tend_suite`]);
    /// `None` until the stored accounts were read, so none of them counts as removed before.
    /// Mail accounts are left out: their calendar part is part of the mail account's one check
    /// ([`PimService::calendar_health`], task M2.7), so no account has two entries.
    pub async fn account_checks(&self, now: DateTime<Utc>) -> Option<Vec<AccountCheck>> {
        let state = self.state.read().await;
        state.loaded.then(|| state.accounts.iter().filter(|slot| !slot.is_mail()).map(|slot| slot.check(now)).collect())
    }

    /// How the calendar part of the mail account `id` stands at `now` (task M2.7), for the mail
    /// service, which joins it with the mail part into the account's one check
    /// (`MailService::account_checks`). `None` when the account syncs no calendars here.
    pub async fn calendar_health(&self, id: &str, now: DateTime<Utc>) -> Option<Health> {
        self.state.read().await.slot(id).filter(|slot| slot.is_mail()).map(|slot| slot.check(now).health)
    }

    /// Brings the calendar part of the mail account `account` in line after it was connected or
    /// its services changed (task M2.7); `password` is the account's one password. In one step:
    ///
    /// - `replaces`, the CalDAV account of the same sign-in the mail account took over, leaves
    ///   with its cache (the caller gave its hidden collections their new ids), so the two never
    ///   sync the same collections side by side; the mail account takes its place in the list;
    /// - an account that syncs calendars ([`account::syncs_calendars`]) gets its slot, or the one
    ///   it has gets the new sign-in, its problem cleared and from its cache only what its services
    ///   keep ([`Slot::keep_chosen`]); then it is synced in the background;
    /// - one that does not (mail only, or calendars and tasks switched off) loses its slot and
    ///   cache. The hidden collections stay in the settings, for when they are switched on again.
    ///
    /// `Err` (nothing changed) when another CalDAV account of the same sign-in syncs the same
    /// collections and is not `replaces`: the two must never sync side by side.
    pub async fn put_mail_account(
        self: &Arc<Self>,
        account: torro_core::Account,
        password: Secret,
        replaces: Option<&str>,
    ) -> Result<(), String> {
        let id = account.id.clone();
        let syncs = account::syncs_calendars(&account);
        let same_sign_in = account::caldav_sign_in(&account);
        let connected = Connected { account, app_password: password };
        let http = self.http_for(&connected.account);
        let client = if syncs { http.and_then(|http| account::caldav(http, &connected).ok()) } else { None };
        {
            let mut state = self.state.write().await;
            let beside = same_sign_in.filter(|caldav| syncs && Some(caldav.as_str()) != replaces);
            if let Some(caldav) = beside.filter(|caldav| state.slot(caldav).is_some()) {
                return Err(format!("the calendar account {caldav} syncs the same calendars"));
            }
            let at = replaces.and_then(|old| state.accounts.iter().position(|slot| slot.id() == old));
            if let Some(at) = at {
                state.accounts.remove(at);
            }
            match (syncs, state.slot_mut(&id)) {
                (true, Some(slot)) => {
                    slot.connected = connected;
                    slot.client = client;
                    slot.problem = None;
                    slot.trouble = None;
                    slot.keep_chosen();
                }
                (true, None) => {
                    let slot = Slot::new(connected, client);
                    let at = at.unwrap_or(state.accounts.len()).min(state.accounts.len());
                    state.accounts.insert(at, slot);
                }
                (false, _) => state.accounts.retain(|slot| slot.id() != id),
            }
        }
        if syncs {
            let service = Arc::clone(self);
            tokio::spawn(async move {
                // `sync_account` logs a failure itself.
                let _ = service.sync_account(&id).await;
            });
        }
        Ok(())
    }

    /// Forgets the calendar part of the removed mail account `id` (task M2.7): its slot, its cache
    /// and its collections in the hidden ones and the list orders of the settings. The account
    /// list and the keychain are the mail service's (`MailService::remove`). Nothing happens for
    /// an account that synced no calendars.
    pub async fn forget_mail_account(&self, id: &str) {
        if let Err(err) = self.forget_locally(id, || async { Ok(()) }).await {
            eprintln!("torroworkplace: the calendars of {id} stay cached: {err}");
        }
    }

    /// The label of the mail account whose calendar part is the CalDAV account `caldav_id` would
    /// be: the same login at the same CalDAV host (task M2.7). `None` when no mail account syncs
    /// it.
    async fn mail_account_syncing(&self, caldav_id: &str) -> Option<String> {
        let state = self.state.read().await;
        let slot = state
            .accounts
            .iter()
            .filter(|slot| slot.is_mail())
            .find(|slot| account::caldav_sign_in(&slot.connected.account).is_some_and(|id| id == caldav_id))?;
        Some(slot.label())
    }

    /// Starts Login Flow v2 and finishes it in the background: the account is added, or the one
    /// with the same id gets the new app password, and is synced.
    async fn connect(self: &Arc<Self>, server: String) -> Response {
        let Some(http) = self.http.clone() else {
            return error("error", "no HTTP client".into());
        };
        let flow = match account::start(&http, &server).await {
            Ok(flow) => flow,
            Err(err) => return error("error", err),
        };
        let login_url = flow.login_url.clone();
        self.state.write().await.signing_in = Some((server, login_url.clone()));
        let service = Arc::clone(self);
        tokio::spawn(async move {
            let result = account::finish(&http, &flow).await;
            let id = {
                let mut state = service.state.write().await;
                state.signing_in = None;
                match result {
                    Ok(connected) => {
                        let id = connected.account.id.clone();
                        let client = account::caldav(&http, &connected).ok();
                        match state.slot_mut(&id) {
                            Some(slot) => {
                                slot.connected = connected;
                                slot.client = client;
                                slot.problem = None;
                                slot.trouble = None;
                                slot.wipe_checked = false;
                            }
                            None => state.accounts.push(Slot::new(connected, client)),
                        }
                        id
                    }
                    Err(err) => {
                        // Back to the connect screen; the apps show the state.
                        eprintln!("torroworkplace: sign-in failed: {err}");
                        return;
                    }
                }
            };
            // `sync_account` logs a failure itself.
            let _ = service.sync_account(&id).await;
        });
        Response::LoginStarted { login_url }
    }

    /// `accounts.discover_services` (task M7.1): what [`Discoverer::discover`] finds for
    /// `address`, as [`Response::Discovery`]; the reason a provider is not supported in the
    /// core's language. `error` for an input without an internet domain, `offline` when nothing
    /// answered. Logs nothing: the address is the person's.
    async fn discover(&self, address: &str) -> Response {
        let Some(discoverer) = self.sign_in.discoverer() else {
            return error("error", "no HTTP client".into());
        };
        match discoverer.discover(address).await {
            // The mail servers are the core's to add (`Core::discover_services`, task M2.2).
            Ok(found) => Response::Discovery {
                provider: found.provider,
                urls: found.urls,
                unsupported: found.unsupported.map(|why| tr(why.reason()).to_string()),
                imap: None,
                smtp: None,
                login_hint: None,
                app_password: false,
                mail_unsupported: None,
            },
            Err(err) => discover_error(err),
        }
    }

    /// `accounts.connect_caldav` (task M7.1, app key only): signs in to `login_name` with
    /// `password` at `server`, or else at each address discovery finds for `address` until one
    /// answers ([`CalDavClient::services`]); stores the password in the keychain and the account
    /// in the account list ([`account::store`]); adds the account, or gives the one with the same
    /// id the new password and server while its hidden collections stay; and syncs it in the
    /// background. A new account hides the collections of the services not in `wanted` at its
    /// first sync ([`chosen`]). An address discovery found is signed in to only at the public
    /// addresses its host resolves to then ([`pinned_public`], task M2.7b); a typed `server` as it
    /// is.
    ///
    /// Answered with [`Response::Connected`]. Codes: `auth` (the server refused the password; no
    /// other address is tried then), `not-found`, `unsupported`, `offline`, `error`. No answer,
    /// message or log line holds the password, and nothing is stored before the server took it.
    async fn connect_caldav(
        self: &Arc<Self>,
        address: &str,
        server: Option<&str>,
        login_name: &str,
        password: Secret,
        wanted: Services,
    ) -> Response {
        let login = login_name.trim();
        if login.is_empty() || login.contains('/') {
            return error("error", tr("Enter the user name; it cannot contain \"/\".").into());
        }
        if password.is_blank() {
            return error("error", tr("Enter the password.").into());
        }
        let Some(http) = self.http.clone() else {
            return error("error", "no HTTP client".into());
        };
        let (provider, candidates) = match self.candidates(address, server).await {
            Ok(found) => found,
            Err(answer) => return *answer,
        };
        // A typed server is the person's choice and may be in their own network; an address
        // discovery found is signed in to only where it still resolves publicly, at exactly the
        // addresses checked (task M2.7b, [`pinned_public`]).
        let typed = server.is_some_and(|server| !server.trim().is_empty());
        let mut failure = ("not-found", tr("No calendar service answers at this address."));
        let mut signed_in = None;
        for url in candidates {
            let dav = CalDavAccount { url: url.clone(), username: login.to_string() };
            let Ok(client) = CalDavClient::new(http.clone(), &dav, password.clone()) else { continue };
            let checking = match typed {
                true => Ok(client.clone()),
                false => pinned_public(&dav, password.clone(), discover::is_public).await,
            };
            let checked = match checking {
                Ok(checking) => checking.services().await,
                Err(err) => Err(err),
            };
            match checked {
                Ok(services) => {
                    signed_in = Some((url, client, services));
                    break;
                }
                Err(ProviderError::Auth) => return error("auth", tr("Password was refused").into()),
                Err(ProviderError::Offline) => failure = ("offline", tr("The server cannot be reached.")),
                Err(_) => failure = ("not-found", tr("No calendar service answers at this address.")),
            }
        }
        let Some((url, client, services)) = signed_in else {
            return error(failure.0, failure.1.into());
        };
        let connected =
            Connected { account: torro_core::Account::caldav(&url, login, &provider), app_password: password };
        // The same collections must never be synced twice (task M2.7).
        if let Some(mail) = self.mail_account_syncing(&connected.account.id).await {
            let message = trf("The mail account {account} syncs these calendars already.", &[("account", &mail)]);
            return error("conflict", message);
        }
        let (file, secrets, storing) =
            (self.sign_in.accounts_file.clone(), Arc::clone(&self.sign_in.secrets), connected.clone());
        let stored = account::keychain(move || {
            account::store(file.as_deref(), &*secrets, &storing.account, &storing.app_password)
        })
        .await;
        if let Err(err) = stored {
            eprintln!("torroworkplace: a CalDAV account could not be stored: {err}");
            return error("error", err);
        }
        let id = connected.account.id.clone();
        let info = {
            let mut state = self.state.write().await;
            match state.slot_mut(&id) {
                // "Change password…": only the sign-in changes; what the person hid stays hidden.
                Some(slot) => {
                    slot.connected = connected;
                    slot.client = Some(client);
                    slot.problem = None;
                    slot.trouble = None;
                    slot.info()
                }
                None => {
                    let slot = Slot { choice: Some(wanted), ..Slot::new(connected, Some(client)) };
                    let info = slot.info();
                    state.accounts.push(slot);
                    info
                }
            }
        };
        let service = Arc::clone(self);
        tokio::spawn(async move {
            // `sync_account` logs a failure itself.
            let _ = service.sync_account(&id).await;
        });
        Response::Connected { account: info, calendars: services.calendars, tasks: services.tasks }
    }

    /// Where [`PimService::connect_caldav`] tries to sign in, with the provider's name: the typed
    /// `server` alone ([`discover::server_address`]), else every address discovery finds for
    /// `address`. `Err` is the answer to send: `unsupported` with the reason, `not-found` when
    /// nothing was found, and the errors of [`PimService::discover`], boxed
    /// because a [`Response`] is large.
    async fn candidates(&self, address: &str, server: Option<&str>) -> Result<(String, Vec<String>), Box<Response>> {
        if let Some(server) = server.map(str::trim).filter(|server| !server.is_empty()) {
            let url = discover::server_address(server)
                .map_err(|_| Box::new(error("error", tr("Enter the server as an https address.").into())))?;
            let provider = discover::domain_of(address).map_or_else(|| host_of(&url), |d| discover::provider_name(&d));
            return Ok((provider, vec![url]));
        }
        let Some(discoverer) = self.sign_in.discoverer() else {
            return Err(Box::new(error("error", "no HTTP client".into())));
        };
        let answer = match discoverer.discover(address).await {
            Ok(found) => match found.unsupported {
                Some(why) => Err(error("unsupported", tr(why.reason()).into())),
                None if found.urls.is_empty() => Err(error(
                    "not-found",
                    tr("No calendar service was found for this address. Enter the server address.").into(),
                )),
                None => Ok((found.provider, found.urls)),
            },
            Err(err) => Err(discover_error(err)),
        };
        answer.map_err(Box::new)
    }

    /// Syncs every account, one after the other. A failure of one account is kept in its state
    /// (and logged) and does not keep the others from syncing; the result is the first failure.
    /// Without any account: not connected.
    pub async fn sync(&self) -> Result<(), ProviderError> {
        let ids: Vec<String> = self.state.read().await.accounts.iter().map(|slot| slot.id().to_string()).collect();
        if ids.is_empty() {
            return Err(not_connected());
        }
        let mut first = None;
        for id in ids {
            if let Err(err) = self.sync_account(&id).await {
                first.get_or_insert(err);
            }
        }
        first.map_or(Ok(()), Err)
    }

    /// Fetches collections, todos and the event window of account `id` into its cache. On
    /// failure its cache stays as it was and the problem shows in its state; a refused app
    /// password runs the remote wipe check.
    async fn sync_account(&self, id: &str) -> Result<(), ProviderError> {
        let _running = self.syncing.lock().await;
        let (client, nextcloud, wanted) = self
            .state
            .read()
            .await
            .slot(id)
            .and_then(|slot| Some((slot.client.clone()?, slot.is_nextcloud(), slot.services())))
            .ok_or_else(not_connected)?;
        let result = fetch_all(&client, id, nextcloud, wanted).await;
        let mut state = self.state.write().await;
        let Some(slot) = state.slot_mut(id) else {
            // The account was removed while the sync ran; nothing of it is kept.
            return Err(not_connected());
        };
        match result {
            Ok(Fetched { collections, private, events, window, todos }) => {
                // Before the collections reach the cache, so no app sees them shown first.
                if let Some(wanted) = slot.choice.take() {
                    for collection in collections.iter().filter(|c| !chosen(wanted, c.kind)) {
                        if let Err(err) = self.settings.set_collection_shown(&collection.id, false) {
                            eprintln!("torroworkplace: a service not chosen for {id} stays shown: {err}");
                        }
                    }
                }
                slot.collections = collections;
                slot.private = private;
                slot.events = events;
                slot.window = Some(window);
                slot.todos = todos;
                slot.last_sync = Some(Utc::now());
                slot.problem = None;
                slot.trouble = None;
                slot.wipe_checked = false;
                Ok(())
            }
            Err(err) => {
                eprintln!("torroworkplace: sync of {id} failed: {err}");
                slot.problem = Some(match (&err, slot.connected.account.kind) {
                    (ProviderError::Auth, AccountKind::CalDav | AccountKind::Mail) => {
                        tr("Password was refused").to_string()
                    }
                    _ => err.to_string(),
                });
                slot.trouble = Some(failing(slot.trouble, &err, Utc::now()));
                drop(state);
                if err == ProviderError::Auth {
                    self.on_refused(id).await;
                }
                Err(err)
            }
        }
    }

    /// The events in `calendars` (ids) that overlap `[start, end)`; other calendars are not
    /// read. Each account answers from its cache when its window covers the range, else from
    /// its server; an account without connection adds nothing.
    async fn events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        calendars: &[String],
    ) -> Result<Vec<Event>, Failed> {
        let mut events = Vec::new();
        // (account, client, [(id, url)]) of the accounts whose cache does not cover the range.
        let mut ask = Vec::new();
        {
            let state = self.state.read().await;
            for slot in &state.accounts {
                let Some(client) = &slot.client else { continue };
                let mine: Vec<(String, String)> = calendars
                    .iter()
                    .filter(|id| state.owner(id).is_some_and(|owner| owner.id() == slot.id()))
                    .filter_map(|id| slot.url(id).map(|url| (id.clone(), url.to_string())))
                    .collect();
                if mine.is_empty() {
                    continue;
                }
                match slot.window {
                    Some(window) if covers(window, start, end) => {
                        let wanted =
                            |e: &&Event| mine.iter().any(|(id, _)| *id == e.collection) && overlaps(e, start, end);
                        events.extend(slot.events.iter().filter(wanted).cloned());
                    }
                    _ => ask.push((slot.id().to_string(), client.clone(), mine)),
                }
            }
        }
        // Outside the cached window: ask the servers.
        for (account, client, mine) in ask {
            for (id, url) in mine {
                let found = client.events(&url, start, end).await.map_err(|err| Failed::at(&account, err))?;
                events.extend(found.into_iter().map(|e| Event { collection: id.clone(), ..e }));
            }
        }
        Ok(events)
    }

    async fn save_event(&self, event: Event) -> Result<Event, WriteError> {
        if event.recurring {
            return Err(series(Role::Calendar));
        }
        let place = self.place(&event.collection).await?;
        let (uid, etag) = (event.uid.clone(), event.etag.clone());
        let saved = match moved(event.href.as_deref(), &place.url) {
            // Moving to another calendar of the same account: create there on top of the stored
            // object (P.1), then remove the old one from the calendar that holds it.
            Some(old_href) => {
                let holder = self.holder_of(&old_href, &place).await?;
                let item = Item::new(Role::Calendar, &holder.url, &old_href, etag.as_deref(), &uid);
                self.check_existing(&holder, item).await?;
                let old = Event { href: Some(old_href), collection: holder.url.clone(), ..event.clone() };
                let new = Event { collection: place.url.clone(), ..event };
                let new = place.client.copy_event(&old, new).await.map_err(|err| Failed::at(&place.account, err))?;
                holder.client.delete_event(&old).await.map_err(|err| Failed::at(&holder.account, err))?;
                if let Some(slot) = self.state.write().await.slot_mut(&holder.account) {
                    slot.events.retain(|e| e.href != old.href);
                }
                new
            }
            None => {
                if let Some(href) = event.href.as_deref() {
                    let item = Item::new(Role::Calendar, &place.url, href, etag.as_deref(), &uid);
                    self.check_existing(&place, item).await?;
                }
                let event = Event { collection: place.url.clone(), ..event };
                place.client.save_event(event).await.map_err(|err| Failed::at(&place.account, err))?
            }
        };
        let saved = Event { collection: place.id.clone(), ..saved };
        if let Some(slot) = self.state.write().await.slot_mut(&place.account) {
            slot.events.retain(|e| e.href != saved.href);
            slot.events.push(saved.clone());
        }
        Ok(saved)
    }

    async fn delete_event(&self, event: Event) -> Result<(), WriteError> {
        if event.recurring {
            return Err(series(Role::Calendar));
        }
        let place = self.place(&event.collection).await?;
        if let Some(href) = event.href.as_deref() {
            let item = Item::new(Role::Calendar, &place.url, href, event.etag.as_deref(), &event.uid);
            self.check_existing(&place, item).await?;
        }
        let gone = Event { collection: place.url.clone(), ..event };
        place.client.delete_event(&gone).await.map_err(|err| Failed::at(&place.account, err))?;
        if let Some(slot) = self.state.write().await.slot_mut(&place.account) {
            slot.events.retain(|e| e.href != gone.href);
        }
        Ok(())
    }

    async fn save_todo(&self, todo: Todo) -> Result<Todo, WriteError> {
        let place = self.place(&todo.collection).await?;
        let (uid, etag) = (todo.uid.clone(), todo.etag.clone());
        let saved = match moved(todo.href.as_deref(), &place.url) {
            Some(old_href) => {
                let holder = self.holder_of(&old_href, &place).await?;
                let item = Item::new(Role::TaskList, &holder.url, &old_href, etag.as_deref(), &uid);
                self.check_existing(&holder, item).await?;
                let old = Todo { href: Some(old_href), collection: holder.url.clone(), ..todo.clone() };
                let new = Todo { collection: place.url.clone(), ..todo };
                let new = place.client.copy_todo(&old, new).await.map_err(|err| Failed::at(&place.account, err))?;
                holder.client.delete_todo(&old).await.map_err(|err| Failed::at(&holder.account, err))?;
                if let Some(slot) = self.state.write().await.slot_mut(&holder.account) {
                    slot.todos.retain(|t| t.href != old.href);
                }
                new
            }
            None => {
                if let Some(href) = todo.href.as_deref() {
                    let item = Item::new(Role::TaskList, &place.url, href, etag.as_deref(), &uid);
                    self.check_existing(&place, item).await?;
                }
                let todo = Todo { collection: place.url.clone(), ..todo };
                place.client.save_todo(todo).await.map_err(|err| Failed::at(&place.account, err))?
            }
        };
        let saved = Todo { collection: place.id.clone(), ..saved };
        if let Some(slot) = self.state.write().await.slot_mut(&place.account) {
            slot.todos.retain(|t| t.href != saved.href);
            slot.todos.push(saved.clone());
        }
        Ok(saved)
    }

    async fn delete_todo(&self, todo: Todo) -> Result<(), WriteError> {
        let place = self.place(&todo.collection).await?;
        if let Some(href) = todo.href.as_deref() {
            let item = Item::new(Role::TaskList, &place.url, href, todo.etag.as_deref(), &todo.uid);
            self.check_existing(&place, item).await?;
        }
        let gone = Todo { collection: place.url.clone(), ..todo };
        place.client.delete_todo(&gone).await.map_err(|err| Failed::at(&place.account, err))?;
        if let Some(slot) = self.state.write().await.slot_mut(&place.account) {
            slot.todos.retain(|t| t.href != gone.href);
        }
        Ok(())
    }

    /// Puts the cached task `href` right before the task `before` of the same list, or at the
    /// end (`Request::MoveTodo`, task P.3a), and returns the tasks written, as stored.
    ///
    /// The numbers come from [`move_plan`] over the cached tasks. Each changed task is written
    /// like [`PimService::save_todo`] with its cached etag, which checks it on the server first, so
    /// only its `X-APPLE-SORT-ORDER` changes there; the moved task needs the `etag` the client read
    /// to be the cached one, else `conflict`, so no move rests on an outdated list.
    ///
    /// The writes run one after the other and stop at the first failure, whose code is the answer:
    /// the ones written before stay on the server and in the cache (every successful write updates
    /// it), and the moved task, written last, has not moved. Nothing is rolled back, since undoing
    /// would be more writes that can fail; the cache always shows what the server holds.
    async fn move_todo(&self, href: &str, etag: &str, before: Option<&str>) -> Result<Vec<Todo>, WriteError> {
        let (plan, list) = {
            let state = self.state.read().await;
            let not_found = |href: &str| refused("not-found", format!("no task {href:?}: reload the tasks"));
            let todo = cached_todo(&state, href).ok_or_else(|| not_found(href))?;
            if todo.etag.as_deref() != Some(etag) {
                let uid = todo.uid.clone();
                return Err(Failed { account: None, err: ProviderError::Conflict { uid } }.into());
            }
            let before = match before {
                Some(href) => match cached_todo(&state, href) {
                    Some(found) if found.collection == todo.collection => Some(found),
                    _ => return Err(not_found(href)),
                },
                None => None,
            };
            let list = state.owner(&todo.collection).map(|slot| slot.todos.clone()).unwrap_or_default();
            let plan = move_plan(&list, todo, before).ok_or_else(|| {
                refused(
                    "error",
                    "a task can only go before one with the same parent that is open or done like it".into(),
                )
            })?;
            (plan, todo.collection.clone())
        };
        if plan.is_empty() {
            return Ok(plan);
        }
        // Read-only lists and missing connections are refused before the first write.
        self.place(&list).await?;
        let mut written = Vec::with_capacity(plan.len());
        for todo in plan {
            written.push(self.save_todo(todo).await?);
        }
        Ok(written)
    }

    /// `tasks.create_list` (task G.7): a new task list named `name` in `color` on the server of
    /// the Nextcloud account `account` ([`CalDavClient::create_task_list`]), answered with the list
    /// as `tasks.list_lists` lists it from now on.
    ///
    /// Checked before the server is asked: `error` for an empty name, one over [`MAX_LIST_NAME`]
    /// characters and a colour that is not `#RRGGBB`; `not-found` for an account the core does not
    /// have (the core answers a GitHub account itself); `not-connected` without its app password.
    /// The address is new, of the name and a random part ([`random_suffixes`]); a taken one is
    /// never overwritten. The list goes into the account's cache at once, as one of the user's
    /// private lists ([`Slot::private`]: theirs, shared with nobody), so it is listed before the
    /// next sync. Nothing is added to any client's selection.
    ///
    /// Runs while no sync of the account does: a sync that started before the list existed would
    /// otherwise put back the collections without it when it ends.
    pub async fn create_task_list(&self, account: &str, name: &str, color: Option<&str>) -> Response {
        let name = name.trim();
        if name.is_empty() {
            return error("error", "A task list needs a name.".into());
        }
        if name.chars().count() > MAX_LIST_NAME {
            return error("error", format!("The name is too long; use at most {MAX_LIST_NAME} characters."));
        }
        let color = match lists::check_color(color) {
            Ok(color) => color,
            Err(err) => return error(err.code(), err.to_string()),
        };
        let suffixes = match random_suffixes() {
            Ok(suffixes) => suffixes,
            Err(message) => return error("error", message),
        };
        let syncing = self.syncing.lock().await;
        let client = match self.state.read().await.slot(account) {
            None => return error("not-found", format!("no account {account:?}")),
            Some(slot) => slot.client.clone(),
        };
        let Some(client) = client else {
            return provider_error(&not_connected());
        };
        let made = client.create_task_list(name, color.as_deref(), &suffixes).await;
        let mut state = self.state.write().await;
        match (made, state.slot_mut(account)) {
            (Ok(mut collection), Some(slot)) => {
                collection.id = collection_id(account, &collection.id);
                slot.collections.push(collection.clone());
                slot.private.push(collection.id.clone());
                Response::Collection { collection }
            }
            // Removed while the server made the list: nothing of the account is kept.
            (Ok(_), None) => error("not-found", format!("no account {account:?}")),
            (Err(err), _) => {
                drop((state, syncing));
                self.failed(Failed::at(account, err)).await
            }
        }
    }

    /// Where a write to `collection` (an id) goes. Refused before anything is sent when no
    /// account is connected (`not-connected`), when no account has listed exactly this id
    /// ([`State::owner`], `not-found`) and when the server marks the collection read-only
    /// (contact birthdays, shared calendars without write access).
    async fn place(&self, collection: &str) -> Result<Place, WriteError> {
        let state = self.state.read().await;
        if state.accounts.is_empty() {
            return Err(not_connected().into());
        }
        let not_found = || refused("not-found", format!("no collection {collection:?}"));
        let Some(slot) = state.owner(collection) else {
            return Err(not_found());
        };
        if let Some(c) = slot.collections.iter().find(|c| c.id == collection)
            && !c.writable
        {
            return Err(ProviderError::Other(format!("{} is read-only", c.name)).into());
        }
        let client = slot.client.clone().ok_or_else(not_connected)?;
        let url = slot.url(collection).ok_or_else(not_found)?.to_string();
        Ok(Place { account: slot.id().to_string(), id: collection.to_string(), url, client })
    }

    /// The cached collection that holds the item `href` directly ([`item_url`]), for moving it to
    /// `target`; an error before anything is written when none does.
    ///
    /// Only within the account of `target`: moving to another account is `unsupported` for now.
    /// The new item is built on the stored one ([`CalDavClient::copy_event`]), which the target's
    /// client would have to read from the other server with its own password.
    async fn holder_of(&self, href: &str, target: &Place) -> Result<Place, WriteError> {
        let state = self.state.read().await;
        for slot in &state.accounts {
            let Some(client) = &slot.client else { continue };
            for collection in &slot.collections {
                if let Some(url) = slot.url(&collection.id).filter(|url| item_url(url, href).is_some()) {
                    if slot.id() != target.account {
                        return Err(refused("unsupported", "items cannot be moved to another account yet".into()));
                    }
                    let account = slot.id().to_string();
                    let (id, url) = (collection.id.clone(), url.to_string());
                    return Ok(Place { account, id, url, client: client.clone() });
                }
            }
        }
        Err(ProviderError::Other(format!("{href} is in no known collection")).into())
    }

    /// Checks an item on the server before it is changed, moved away or deleted (task A.2b), so
    /// nothing is sent when it fails:
    ///
    /// - without the `etag` it was read with: `conflict` ("reload the item and pass its etag"),
    ///   because a write without `If-Match` would replace whatever the server holds now;
    /// - a series: `unsupported`, whatever the client says in `recurring`;
    /// - gone from the server: `not-found`; changed since it was read: `conflict`.
    ///
    /// Whether it is a series comes from the cache of the account at `place` when it holds the
    /// item in the version `etag` names (events only: the cache does not record series for
    /// tasks), else from the server ([`CalDavClient::stored_item`]). The write that follows
    /// sends the same `etag` as `If-Match`, so the version checked is the version written.
    async fn check_existing(&self, place: &Place, item: Item<'_>) -> Result<(), WriteError> {
        let Some(url) = item_url(item.collection, item.href) else {
            return Err(ProviderError::Other(format!("{} is not an item in {}", item.href, item.collection)).into());
        };
        let Some(etag) = item.etag.filter(|etag| !etag.trim().is_empty()) else {
            return Err(refused("conflict", format!("{} has no etag: reload the item and pass its etag", item.href)));
        };
        if item.role == Role::Calendar {
            let state = self.state.read().await;
            if let Some(recurring) = state.slot(&place.account).and_then(|slot| cached_series(slot, &url, etag)) {
                return if recurring { Err(series(item.role)) } else { Ok(()) };
            }
        }
        let stored = place.client.stored_item(item.collection, item.href).await;
        match stored.map_err(|err| Failed::at(&place.account, err))? {
            None => Err(refused("not-found", format!("{} is not on the server any more: reload", item.href))),
            Some(stored) if stored.recurring => Err(series(item.role)),
            Some(stored) if stored.etag != etag => {
                Err(Failed::at(&place.account, ProviderError::Conflict { uid: item.uid.to_string() }).into())
            }
            Some(_) => Ok(()),
        }
    }
}

/// What approvals show (task A.3b).
impl PimService {
    /// What the cache holds about the event, task or collection `request` names, for the summary
    /// of an approval ([`crate::approvals::summary`]). Looked up by the keys the command runs
    /// with, never by the titles the client sent along:
    ///
    /// - writes of events and tasks with an `href`: the item at that `href`, resolved as the
    ///   write resolves it ([`touched`]), and for a save that moves it the collection losing it
    ///   ([`Facts::moved_from`]); the collection written to by its display name;
    /// - `tasks.move_todo`: the task at `href` and its list, compared as `move_todo` compares;
    /// - `tasks.save_todo` on a GitHub filter list (tasks G.5, G.5b): the item the list holds under
    ///   the `uid`, the one [`PimService::save_filter_todo`] closes or reopens, and for an issue
    ///   (not a pull request) its `owner/repo#n` ([`Facts::issue`]);
    /// - `tasks.save_todo` on a link to an issue (task G.6): the issue it ticks ([`ticked_issue`];
    ///   none in a list that is not private, G.6c) and whether that is all the save changes
    ///   ([`Facts::ticks_only`], G.6b);
    /// - `tasks.add_link`: the list's display name and whether it is a filter list, where the item
    ///   is pinned ([`Facts::pins`], G.6b);
    /// - `tasks.unpin_issue`: the filter list and the pinned item with the same repository and
    ///   number as `url`;
    /// - `settings.set_collection_shown`, `tasks.update_filter_list`, `tasks.delete_filter_list`:
    ///   the collection's display name.
    ///
    /// Empty for every other request. An event outside the cached window (3 months back, 12
    /// ahead), or cached in another version than the request's `etag`, is not held, so its
    /// summary says "unknown event".
    pub async fn facts(&self, request: &Request) -> Facts {
        let state = self.state.read().await;
        let name_of = |id: &str| state.collections().find(|c| c.id == id).map(|c| c.name.clone());
        let named = |id: &str| Facts { name: name_of(id), ..Facts::default() };
        let written = |collection: &str, href: Option<&str>, etag: Option<&str>, role: Role, save: bool| {
            let touched = href.and_then(|href| touched(&state, collection, href, etag, role, save));
            let (item, moved_from) = match touched {
                Some((holder, item)) => (item, (holder != collection).then(|| name_of(&holder).unwrap_or(holder))),
                None => (None, None),
            };
            Facts { item, name: name_of(collection), moved_from, ..Facts::default() }
        };
        match request {
            Request::DeleteEvent { event } => {
                written(&event.collection, event.href.as_deref(), event.etag.as_deref(), Role::Calendar, false)
            }
            Request::SaveEvent { event } => {
                written(&event.collection, event.href.as_deref(), event.etag.as_deref(), Role::Calendar, true)
            }
            Request::DeleteTodo { todo } => {
                written(&todo.collection, todo.href.as_deref(), todo.etag.as_deref(), Role::TaskList, false)
            }
            Request::SaveTodo { todo } if state.filter(&todo.collection).is_some() => {
                let held = state.filter(&todo.collection).and_then(|(_, list)| filter_item(list, &todo.uid));
                // Only an issue that can be ticked: a pull request is refused, so it is not named.
                let issue = held.filter(|_| todo.href.is_some()).and_then(|held| {
                    let issue = closable_issue(held)?;
                    Some(format!("{}#{}", issue.repo, issue.number))
                });
                // Anything but ticking is refused there (`changes_more_than_done`).
                Facts {
                    item: held.map(|todo| Known { title: todo.summary.clone(), start: None }),
                    issue,
                    ticks_only: true,
                    ..named(&todo.collection)
                }
            }
            // Ticking a link to an issue closes or reopens the issue too (task G.6): named like the
            // tick of a filter list's issue, by the link the cache holds, and with the change of
            // the task when the save changes more than whether it is done (G.6b).
            Request::SaveTodo { todo } => {
                let facts = written(&todo.collection, todo.href.as_deref(), todo.etag.as_deref(), Role::TaskList, true);
                let issue = ticked_issue(&state, todo).map(|issue| format!("{}#{}", issue.repo, issue.number));
                let held = todo.href.as_deref().and_then(|href| cached_todo(&state, href));
                let ticks_only = held.is_some_and(|held| only_ticks(todo, held));
                Facts { issue, ticks_only, ..facts }
            }
            Request::AddLink { collection, .. } => {
                Facts { pins: state.filter(collection).is_some(), ..named(collection) }
            }
            Request::MoveTodo { href, .. } => match cached_todo(&state, href) {
                Some(todo) => {
                    Facts { item: Some(Known { title: todo.summary.clone(), start: None }), ..named(&todo.collection) }
                }
                None => Facts::default(),
            },
            Request::UnpinIssue { collection, url } => {
                // Only an item the list really pins, as `unpin_issue` matches it; a search match
                // that is not pinned would make the command fail, so it is not named.
                let pinned = address::issue_ref(url).and_then(|issue| {
                    let (_, list) = state.filter(collection)?;
                    let same = |id: &str| address::issue_ref(id).is_some_and(|i| i.same(&issue));
                    list.list.pinned.iter().any(|id| same(id)).then_some(())?;
                    list.todos.iter().find(|todo| same(&todo.uid))
                });
                Facts {
                    item: pinned.map(|todo| Known { title: todo.summary.clone(), start: None }),
                    ..named(collection)
                }
            }
            Request::SetCollectionShown { collection, .. }
            | Request::UpdateFilterList { collection, .. }
            | Request::DeleteFilterList { collection } => named(collection),
            _ => Facts::default(),
        }
    }
}

/// Where a write of an item to `collection` with `href` lands, resolved as the write resolves it
/// ([`item_url`]): the collection that holds the item and the item as cached there.
///
/// That is `collection` itself when `href` is an item directly in it. For a save (`save`) of an
/// `href` elsewhere it is the collection of the same account holding it, from which
/// [`PimService::save_event`] and [`PimService::save_todo`] move it ([`PimService::holder_of`]).
/// `None` when nothing holds it: the write then fails without touching anything.
///
/// The item is `None` when the holder's cache has nothing at that address, or has it in another
/// version than `etag`, the one the write will be checked against: the server may hold a newer
/// version than the cache (a client can read one past the cached window), and the summary must
/// not name the old title for it. Of a series the first occurrence.
fn touched(
    state: &State,
    collection: &str,
    href: &str,
    etag: Option<&str>,
    role: Role,
    save: bool,
) -> Option<(String, Option<Known>)> {
    let slot = state.owner(collection)?;
    let at = |id: &str| slot.url(id).and_then(|url| item_url(url, href)).map(|target| (id.to_string(), target));
    let (holder, target) = match at(collection) {
        Some(found) => found,
        None if save => slot.collections.iter().find_map(|c| at(&c.id))?,
        None => return None,
    };
    let is_target = |item_collection: &str, item_href: Option<&str>, item_etag: Option<&str>| {
        item_collection == holder
            && item_etag == etag
            && item_href.zip(slot.url(item_collection)).and_then(|(href, url)| item_url(url, href)).as_ref()
                == Some(&target)
    };
    let item = match role {
        Role::Calendar => slot
            .events
            .iter()
            .filter(|e| is_target(&e.collection, e.href.as_deref(), e.etag.as_deref()))
            .min_by_key(|e| e.start)
            .map(|e| Known { title: e.summary.clone(), start: Some(e.start) }),
        Role::TaskList => slot
            .todos
            .iter()
            .find(|t| is_target(&t.collection, t.href.as_deref(), t.etag.as_deref()))
            .map(|t| Known { title: t.summary.clone(), start: None }),
    };
    Some((holder, item))
}

/// The trouble of an account whose sync just failed with `err` at `now`, after `before`: a refused
/// app password or another failure, since `now` unless the same trouble went on already.
fn failing(
    before: Option<(Trouble, DateTime<Utc>)>,
    err: &ProviderError,
    now: DateTime<Utc>,
) -> (Trouble, DateTime<Utc>) {
    let trouble = if *err == ProviderError::Auth { Trouble::SignInRefused } else { Trouble::SyncFailing };
    match before {
        Some((same, since)) if same == trouble => (trouble, since),
        _ => (trouble, now),
    }
}

/// A request refused before anything changed: an API error code and a message for people.
type Refusal = (&'static str, String);

/// Why a client may not tick an issue of a filter list (task G.5): `forbidden` without write
/// access to `github`, whatever it may do with tasks.
pub const NO_GITHUB_WRITE: &str = "closing or reopening a GitHub issue needs write access to github";

/// How many matches a preview hands out.
const PREVIEW_ITEMS: usize = 20;

/// GitHub filter lists (task G.2). The core resolves the account's [`Api`] and the rights;
/// these methods keep the definitions and the items.
impl PimService {
    /// `collection` is a filter list the core holds.
    pub async fn is_filter_list(&self, collection: &str) -> bool {
        self.state.read().await.filter(collection).is_some()
    }

    /// The GitHub account of the filter list `collection`, if it is one.
    pub async fn filter_list_account(&self, collection: &str) -> Option<String> {
        self.state.read().await.filter(collection).map(|(account, _)| account.to_string())
    }

    /// The accounts that have filter lists in `filter-lists.json`, so the core knows whose
    /// tokens to wait for at start.
    pub fn filter_list_owners(&self) -> Vec<String> {
        self.read_filters()
            .map(|file| file.accounts.into_iter().filter(|(_, lists)| !lists.is_empty()).map(|(id, _)| id).collect())
            .unwrap_or_default()
    }

    /// The filter lists of `account` as `reach` sees them, for `accounts.list_accounts`: for the
    /// suite all, hidden ones included; for a client the ones it sees.
    pub async fn filter_lists_within(&self, account: &str, reach: Reach<'_>) -> Vec<Collection> {
        let hidden = match reach {
            Reach::Suite => Vec::new(),
            Reach::Client(_) => self.hidden_collections(),
        };
        let state = self.state.read().await;
        let lists = state.filters.iter().filter(|slot| slot.account == account).flat_map(|slot| &slot.lists);
        lists
            .map(|list| &list.collection)
            .filter(|c| matches!(reach, Reach::Suite) || reach.sees(c, Role::TaskList, &hidden))
            .map(|c| reach.collection_for(c.clone()))
            .collect()
    }

    /// Reads the filter lists of the GitHub account `account` from `filter-lists.json` into the
    /// cache and fetches each from GitHub with `api`. Without `api` (no token) the lists keep
    /// what they had and show a problem. A file that cannot be read changes nothing.
    pub async fn refresh_filter_lists(&self, account: &str, api: Option<&Api>) {
        // What the filter list dialog read is kept until here (task G.9), and an answer to a
        // question asked before is not kept either (G.9b).
        self.state.write().await.renew_reads(account);
        let file = match self.read_filters() {
            Ok(file) => file,
            Err(err) => {
                eprintln!("torroworkplace: filter lists of {account} not read: {err}");
                return;
            }
        };
        let defined = file.lists(account).to_vec();
        {
            let mut state = self.state.write().await;
            let slot = state.filter_slot_mut(account);
            let mut old = std::mem::take(&mut slot.lists);
            for list in &defined {
                let cache = match old.iter().position(|cache| cache.list.id == list.id) {
                    Some(at) => {
                        let mut cache = old.swap_remove(at);
                        cache.redefine(account, list.clone());
                        cache
                    }
                    None => FilterCache::new(account, list.clone()),
                };
                slot.lists.push(cache);
            }
        }
        for list in &defined {
            self.fetch_filter_list(account, list, api).await;
        }
    }

    /// Forgets the filter lists of `account` in the cache, when the account is removed. The
    /// definitions stay in the file: signing in to the account again brings them back.
    pub async fn forget_filter_lists(&self, account: &str) {
        self.state.write().await.filters.retain(|slot| slot.account != account);
    }

    /// `tasks.create_filter_list` for the GitHub account `account`, which the core knows: stored
    /// with a new id and revision 1, then fetched once, and answered as `reach` may see it. A
    /// search outside the repositories of `reach` is `forbidden` before anything is stored or
    /// asked ([`checked_definition`]).
    pub async fn create_filter_list(
        &self,
        account: &str,
        definition: Definition<'_>,
        api: Option<&Api>,
        reach: Reach<'_>,
    ) -> Response {
        let (name, color, queries) = match checked_definition(definition, reach) {
            Ok(checked) => checked,
            Err((code, message)) => return error(code, message),
        };
        let list = {
            let _writing = self.filters_writing.lock().await;
            let mut file = match self.read_filters() {
                Ok(file) => file,
                Err(err) => return error("error", err),
            };
            let id = match lists::new_id(|id| file.has_id(id)) {
                Ok(id) => id,
                Err(err) => return error("error", err),
            };
            let list = FilterList { id, name, color, queries, pinned: Vec::new(), revision: 1 };
            file.accounts.entry(account.to_string()).or_default().push(list.clone());
            if let Err(err) = self.write_filters(&file) {
                return error("error", err);
            }
            self.state.write().await.filter_slot_mut(account).lists.push(FilterCache::new(account, list.clone()));
            list
        };
        self.fetch_filter_list(account, &list, api).await;
        self.answer_collection(&collection_id(account, &list.id), reach).await
    }

    /// `tasks.update_filter_list`: new name, colour and searches when `revision` is the stored
    /// one, then a new fetch; answered as `reach` may see the list. Searches outside the
    /// repositories of `reach` are `forbidden` and change nothing ([`checked_definition`]).
    pub async fn update_filter_list(
        &self,
        collection: &str,
        revision: u64,
        definition: Definition<'_>,
        api: Option<&Api>,
        reach: Reach<'_>,
    ) -> Response {
        let Some(account) = self.filter_list_account(collection).await else {
            return no_filter_list(collection);
        };
        let (name, color, queries) = match checked_definition(definition, reach) {
            Ok(checked) => checked,
            Err((code, message)) => return error(code, message),
        };
        let changed = self
            .change_filter_list(&account, collection, |list| {
                if list.revision != revision {
                    let message = format!("the list changed meanwhile (revision {}): read it again", list.revision);
                    return Err(("conflict", message));
                }
                *list = FilterList { name, color, queries, revision: revision + 1, ..list.clone() };
                Ok(())
            })
            .await;
        match changed {
            Ok(list) => {
                self.fetch_filter_list(&account, &list, api).await;
                self.answer_collection(collection, reach).await
            }
            Err((code, message)) => error(code, message),
        }
    }

    /// `tasks.delete_filter_list`: the definition and the cached items go, and the list leaves
    /// the hidden collections. Its id is never handed out again ([`lists::new_id`]).
    pub async fn delete_filter_list(&self, collection: &str) -> Response {
        let Some(account) = self.filter_list_account(collection).await else {
            return no_filter_list(collection);
        };
        {
            let _writing = self.filters_writing.lock().await;
            let mut file = match self.read_filters() {
                Ok(file) => file,
                Err(err) => return error("error", err),
            };
            if let Some(lists) = file.accounts.get_mut(&account) {
                lists.retain(|list| collection_id(&account, &list.id) != collection);
            }
            if let Err(err) = self.write_filters(&file) {
                return error("error", err);
            }
            for slot in &mut self.state.write().await.filters {
                slot.lists.retain(|list| list.collection.id != collection);
            }
        }
        if let Err(err) = self.settings.forget_collections(&[collection.to_string()]) {
            eprintln!("torroworkplace: the deleted filter list stays among the hidden collections: {err}");
        }
        Response::Ok
    }

    /// `tasks.unpin_issue`: the item `url` leaves the pinned ones of `collection`; the list is
    /// fetched again, so the item stays while a search finds it.
    pub async fn unpin_issue(&self, collection: &str, url: &str, api: Option<&Api>) -> Response {
        let Some(account) = self.filter_list_account(collection).await else {
            return no_filter_list(collection);
        };
        let Some(issue) = address::issue_ref(url) else {
            return error("not-found", "the list pins no such item".into());
        };
        let changed = self
            .change_filter_list(&account, collection, |list| {
                let before = list.pinned.len();
                list.pinned.retain(|id| !address::issue_ref(id).is_some_and(|pinned| pinned.same(&issue)));
                match list.pinned.len() < before {
                    true => Ok(()),
                    false => Err(("not-found", format!("the list does not pin {}", issue.link_id()))),
                }
            })
            .await;
        match changed {
            Ok(list) => {
                self.fetch_filter_list(&account, &list, api).await;
                Response::Ok
            }
            Err((code, message)) => error(code, message),
        }
    }

    /// `tasks.save_todo` on an item of the GitHub filter list `todo.collection` (task G.5): a
    /// changed `completed` closes the issue on GitHub as completed (`true`) or reopens it
    /// (`false`) with `api` (the list's account; `None` without a token). The item is then updated
    /// in every filter list that holds it and answered as `reach` sees it. Checked in this order,
    /// each before GitHub is asked:
    ///
    /// - without `href` (a new task): `unsupported`, a filter list takes none;
    /// - no item with this `uid` in the list, or (for a client) one outside its repositories:
    ///   `not-found`;
    /// - a client without write access to `github`: `forbidden`;
    /// - a pull request: `unsupported`, pull requests are never closed here;
    /// - another `etag` than the cached one: `conflict` (it changed on GitHub meanwhile);
    /// - any other field changed ([`changes_more_than_done`]): `unsupported`;
    /// - `completed` as cached: the cached item, nothing is sent;
    /// - no token: `not-connected`.
    ///
    /// GitHub's refusals come as `auth` (token refused), `forbidden` (the token may not write
    /// issues there), `not-found`, `offline` or `error`; for a client with fixed messages only
    /// ([`change_message`]). The issue is always the one the cache holds under the `uid`.
    pub async fn save_filter_todo(&self, todo: Todo, api: Option<&Api>, reach: Reach<'_>) -> Response {
        if todo.href.is_none() {
            return error("unsupported", "a GitHub filter list takes no new tasks".into());
        }
        let held = {
            let state = self.state.read().await;
            state
                .filter(&todo.collection)
                .and_then(|(account, list)| Some((account.to_string(), filter_item(list, &todo.uid)?.clone())))
        };
        // As the client may see it: a parent of another repository is left out there too.
        let Some((account, held, shown)) = held.and_then(|(account, held)| {
            let shown = reach.todo_for(&held)?;
            Some((account, held, shown))
        }) else {
            return error("not-found", format!("the list holds no item {:?}", todo.uid));
        };
        if let Reach::Client(rights) = reach
            && !rights.area(Area::Github).allows(Access::Write)
        {
            return error("forbidden", NO_GITHUB_WRITE.into());
        }
        let Some(issue) = closable_issue(&held) else {
            return error("unsupported", "pull requests are never closed or reopened here".into());
        };
        if todo.etag != held.etag {
            return error("conflict", "the issue changed on GitHub meanwhile: read it again".into());
        }
        if changes_more_than_done(&todo, &shown) {
            let message = "of a GitHub issue in a filter list only whether it is done can be changed";
            return error("unsupported", message.into());
        }
        if todo.completed == held.completed {
            return Response::Todo { todo: shown };
        }
        let Some(api) = api else {
            return error("not-connected", format!("{account} has no token on this device: sign in to GitHub again"));
        };
        let item = match api.set_issue_done(&issue, todo.completed).await {
            Ok(item) => item,
            Err(err) => {
                eprintln!("torroworkplace: {} not changed on GitHub: {}", issue.link_id(), err.code());
                return error(err.code(), change_message(reach, &err));
            }
        };
        let saved = self
            .keep_item(&todo.collection, &item)
            .await
            .unwrap_or_else(|| Todo { sort_order: held.sort_order, ..item.to_todo(&todo.collection, 0) });
        match reach.todo_for(&saved) {
            Some(todo) => Response::Todo { todo },
            None => error("not-found", format!("the list holds no item {:?}", todo.uid)),
        }
    }

    /// Puts `item`, as GitHub reported it after a change, into every filter list that holds it,
    /// each copy at its place, and returns the copy of the list `collection`. Until the next
    /// refresh it stays even where a search would no longer find it (a closed issue in an
    /// `is:open` list), so it shows as done and can be reopened.
    async fn keep_item(&self, collection: &str, item: &item::Item) -> Option<Todo> {
        let id = item.id.link_id();
        let mut state = self.state.write().await;
        let mut kept = None;
        for list in state.filters.iter_mut().flat_map(|slot| &mut slot.lists) {
            for todo in list.todos.iter_mut().filter(|todo| todo.uid == id) {
                *todo = Todo { sort_order: todo.sort_order, ..item.to_todo(&todo.collection, 0) };
                if todo.collection == collection {
                    kept = Some(todo.clone());
                }
            }
        }
        kept
    }

    /// `tasks.preview_filter`: the searches run now, nothing is stored; one call to GitHub per
    /// search (its first [`PREVIEW_ITEMS`] matches and GitHub's count).
    ///
    /// For a client limited to some repositories every search must stay inside them
    /// ([`Reach::confines`]), else `forbidden` before GitHub is asked; its failures come with fixed
    /// messages ([`Reach::message_of`]). Every match outside its repositories is still dropped as
    /// it comes in. Should one be dropped (GitHub followed a renamed repository), that search
    /// counts only the matches kept, since GitHub's count would include the others.
    pub async fn preview_filter(&self, queries: &[String], api: &Api, reach: Reach<'_>) -> Response {
        let queries = match lists::check_queries(queries) {
            Ok(queries) => queries,
            Err(err) => return error(err.code(), err.to_string()),
        };
        if !reach.confines(&queries) {
            return error("forbidden", scope::OUTSIDE.into());
        }
        let (mut matches, mut truncated, mut parts) = (0, false, Vec::new());
        for query in &queries {
            let search = match api.search(query, PREVIEW_ITEMS).await {
                Ok(search) => search,
                Err(err) => return error(err.code(), reach.message_of(&err)),
            };
            let found = search.items.len();
            let mine: Vec<item::Item> =
                search.items.into_iter().filter(|item| reach.reaches_repo(item.repo())).collect();
            if mine.len() == found {
                matches += search.total;
                truncated |= search.total > MAX_MATCHES as u64;
            } else {
                matches += mine.len() as u64;
            }
            parts.push(mine);
        }
        let mut all = item::union(parts);
        all.truncate(PREVIEW_ITEMS);
        let todos = item::todos(&all, "").iter().filter_map(|todo| reach.todo_for(todo)).collect();
        Response::FilterPreview { matches, truncated, todos }
    }

    /// `tasks.list_github_repos` for the GitHub account `account`, which the core knows: what
    /// [`FilterSlot::repos`] holds since the last refresh, else asked of GitHub with `api` and
    /// kept. Answered as `reach` may see it: a client only the repositories it reaches
    /// ([`Reach::reaches_repo`]), and its failures with fixed messages ([`read_message`]).
    pub async fn list_github_repos(&self, account: &str, api: &Api, reach: Reach<'_>) -> Response {
        let read = self.read_through(
            account,
            |slot| slot.repos.clone(),
            || api.repos(),
            |slot, repos| slot.repos = Some(repos),
        );
        match read.await {
            Ok(repos) => Response::GithubRepos {
                repos: repos.into_iter().filter(|repo| reach.reaches_repo(&repo.repo)).collect(),
            },
            Err(err) => error(err.code(), read_message(reach, &err)),
        }
    }

    /// What the filter list dialog reads of GitHub for `account` (task G.9): `held` from its
    /// [`FilterSlot`] when that has it, else `fetch`ed and given to `keep` – but only when the slot
    /// is still there in the generation it had before GitHub was asked (task G.9b,
    /// [`FilterSlot::generation`]). A refresh meanwhile would otherwise get an answer older than
    /// itself back, and an account removed meanwhile its slot. Without a slot (the account's first
    /// refresh has not run yet) nothing is kept.
    async fn read_through<T, Fut>(
        &self,
        account: &str,
        held: impl FnOnce(&FilterSlot) -> Option<T>,
        fetch: impl FnOnce() -> Fut,
        keep: impl FnOnce(&mut FilterSlot, T),
    ) -> Result<T, torro_pim_github::Error>
    where
        T: Clone,
        Fut: Future<Output = Result<T, torro_pim_github::Error>>,
    {
        let generation = {
            let state = self.state.read().await;
            let slot = state.filters.iter().find(|slot| slot.account == account);
            if let Some(held) = slot.and_then(held) {
                return Ok(held);
            }
            slot.map(|slot| slot.generation)
        };
        let fetched = fetch().await?;
        let mut state = self.state.write().await;
        let same = state.filters.iter_mut().find(|slot| slot.account == account && Some(slot.generation) == generation);
        if let Some(slot) = same {
            keep(slot, fetched.clone());
        }
        Ok(fetched)
    }

    /// `tasks.describe_github_repo` for the repository `repo` (`owner/name`) of the GitHub
    /// account `account`, which the core knows: what [`FilterSlot::described`] holds since the
    /// last refresh, else asked of GitHub with `api` (one call) and kept ([`PimService::read_through`],
    /// at most [`MAX_DESCRIBED`] repositories). Any repository the token reads, not only those of
    /// [`PimService::list_github_repos`].
    ///
    /// A repository outside the reach of a client is `not-found` before GitHub is asked (the core
    /// checked that already, [`PimService::out_of_reach`]; again here for an approved run), and so
    /// is one GitHub answers under a name outside it (it follows a renamed repository). GitHub's
    /// `NOT_FOUND` is `not-found` with the core's message; a `repo` that is not `owner/name` is
    /// `error`.
    pub async fn describe_github_repo(&self, account: &str, repo: &str, api: &Api, reach: Reach<'_>) -> Response {
        let repo = repo.trim();
        if !reach.reaches_repo(repo) {
            return no_repo(repo);
        }
        let key = repo.to_ascii_lowercase();
        let read = self.read_through(
            account,
            |slot| slot.described(&key),
            || api.describe_repo(repo),
            |slot, details| slot.describe(key.clone(), details),
        );
        let details = match read.await {
            Ok(details) => details,
            Err(torro_pim_github::Error::NotFound) => return no_repo(repo),
            Err(err) => return error(err.code(), read_message(reach, &err)),
        };
        if !reach.reaches_repo(&details.repo) {
            return no_repo(repo);
        }
        let GitHubRepoDetails { repo, labels, milestones, assignees } = details;
        Response::GithubRepo { repo, labels, milestones, assignees }
    }

    /// Fetches the list `list` of `account` and keeps what came, unless its definition changed
    /// meanwhile ([`FilterCache::apply`]).
    async fn fetch_filter_list(&self, account: &str, list: &FilterList, api: Option<&Api>) {
        let result = match api {
            Some(api) => api.fetch(&list.queries, &list.pinned).await,
            None => Err(torro_pim_github::Error::Refused),
        };
        if let Some(cache) = self.state.write().await.filter_mut(&collection_id(account, &list.id)) {
            cache.apply(account, list, result);
        }
    }

    /// Reads `filter-lists.json`, changes the list `collection` of `account` with `change` and
    /// writes it back, then puts the new definition into the cache. `change` may refuse with an
    /// answer; then nothing is written.
    async fn change_filter_list(
        &self,
        account: &str,
        collection: &str,
        change: impl FnOnce(&mut FilterList) -> Result<(), Refusal>,
    ) -> Result<FilterList, Refusal> {
        let _writing = self.filters_writing.lock().await;
        let mut file = self.read_filters().map_err(|err| ("error", err))?;
        let list = file
            .accounts
            .get_mut(account)
            .and_then(|lists| lists.iter_mut().find(|list| collection_id(account, &list.id) == collection))
            .ok_or_else(|| ("not-found", format!("no filter list {collection:?}")))?;
        change(list)?;
        let list = list.clone();
        self.write_filters(&file).map_err(|err| ("error", err))?;
        if let Some(cache) = self.state.write().await.filter_mut(collection) {
            cache.redefine(account, list.clone());
        }
        Ok(list)
    }

    /// The filter list `collection` as `tasks.list_lists` shows it to `reach`
    /// ([`Reach::collection_for`]).
    async fn answer_collection(&self, collection: &str, reach: Reach<'_>) -> Response {
        match self.state.read().await.filter(collection) {
            Some((_, list)) => Response::Collection { collection: reach.collection_for(list.collection.clone()) },
            None => no_filter_list(collection),
        }
    }

    fn read_filters(&self) -> Result<FilterFile, String> {
        match &self.filters_file {
            Some(path) => lists::load(path),
            None => Ok(FilterFile::default()),
        }
    }

    fn write_filters(&self, file: &FilterFile) -> Result<(), String> {
        let path = self.filters_file.as_deref().ok_or("No home directory, so filter lists cannot be stored.")?;
        lists::save(path, file)
    }
}

/// Why a client may not add a GitHub link (task G.6): the item is read with the GitHub token.
pub const NO_GITHUB_READ: &str = "adding a GitHub link needs read access to github";
/// Why a client may not set or change the link of a task (task G.6b): the core fills it in from
/// GitHub with the token.
pub const NO_GITHUB_LINK: &str = "linking a task to GitHub needs read access to github";
/// Why a client may not pin to a filter list: pinning changes the list, as `tasks.unpin_issue` does.
pub const NO_GITHUB_PIN: &str = "pinning to a GitHub filter list needs write access to github";
/// The answer to `tasks.add_link` with something that names no issue or pull request.
const NOT_A_LINK: &str =
    "not a link to a GitHub issue or pull request: paste its address on github.com or write owner/repo#12";
/// The answer when no GitHub account has a token on this device.
const NO_GITHUB_ACCOUNT: &str = "no GitHub account is connected on this device: connect one in Settings";

/// Links to GitHub issues and pull requests (task G.6). The core hands in the [`Api`] of the
/// filter list's account, or of the first GitHub account with a token for a task list, and has
/// checked the reach ([`PimService::out_of_reach`]) and the rights ([`PimService::add_link_refusal`])
/// before a client's request comes here; the handlers check both again for an approved run.
impl PimService {
    /// For a client: why it may not add the link `url` to `collection`, as the answer – `error`
    /// when `url` names no issue or pull request ([`NOT_A_LINK`], so nobody is asked to approve
    /// it, task G.6b), `forbidden` without read access to `github` ([`NO_GITHUB_READ`]), and for a
    /// filter list without write access ([`NO_GITHUB_PIN`]). `None` when it may, and always for
    /// the suite.
    pub async fn add_link_refusal(&self, reach: Reach<'_>, collection: &str, url: &str) -> Option<Response> {
        let Reach::Client(rights) = reach else { return None };
        if address::issue_ref(url).is_none() {
            return Some(error("error", NOT_A_LINK.into()));
        }
        let github = rights.area(Area::Github);
        if !github.allows(Access::Read) {
            return Some(error("forbidden", NO_GITHUB_READ.into()));
        }
        if self.is_filter_list(collection).await && !github.allows(Access::Write) {
            return Some(error("forbidden", NO_GITHUB_PIN.into()));
        }
        None
    }

    /// `tasks.add_link`: the issue or pull request `url` (an address on github.com,
    /// `owner/repo#12` or a link id) in the list `collection`, answered with the task the list
    /// holds as `reach` may see it ([`Response::Todo`]). In this order:
    ///
    /// - `url` names no item: `error` ([`NOT_A_LINK`]);
    /// - a client without the rights ([`PimService::add_link_refusal`]): `forbidden`; with the
    ///   item's repository outside its selection: `not-found`, before GitHub is asked;
    /// - a task list: refused as a write to it is ([`PimService::place`]: unknown `not-found`,
    ///   read-only `error`), and a link to the same item in it is the answer, without GitHub;
    /// - no `api`: `not-connected`; GitHub asked once ([`Api::items`]): its failure with its code
    ///   (for a client a fixed message), an item it does not have or the token does not reach
    ///   `not-found` – also for a client when GitHub names a repository outside its selection (a
    ///   renamed one);
    /// - a filter list: the item is pinned ([`PimService::pin`]); a task list: a new task with
    ///   GitHub's title, the item's address and its link id, done when it is closed or merged
    ///   ([`PimService::save_todo`]).
    ///
    /// What GitHub reported goes into [`State::links`]; logs name the item by its link id only.
    pub async fn add_link(&self, collection: &str, url: &str, api: Option<&Api>, reach: Reach<'_>) -> Response {
        let Some(issue) = address::issue_ref(url) else {
            return error("error", NOT_A_LINK.into());
        };
        if let Some(refused) = self.add_link_refusal(reach, collection, url).await {
            return refused;
        }
        if !reach.reaches_repo(&issue.repo) {
            return no_item(&issue);
        }
        let filter = self.filter_list_account(collection).await;
        if filter.is_none() {
            if let Err(err) = self.place(collection).await {
                return self.write_failed(err).await;
            }
            if let Some(held) = self.held_link(collection, &issue).await {
                return Response::Todo { todo: reach.with_details(held, &self.state.read().await.links) };
            }
        }
        let Some(api) = api else {
            return error("not-connected", NO_GITHUB_ACCOUNT.into());
        };
        let item = match api.items(std::slice::from_ref(&issue)).await {
            Ok(items) => items.into_iter().next(),
            Err(err) => {
                eprintln!("torroworkplace: {} not read from GitHub: {}", issue.link_id(), err.code());
                return error(err.code(), reach.message_of(&err));
            }
        };
        // GitHub follows a renamed repository: what counts is where the item is now.
        let Some(item) = item.filter(|item| reach.reaches_repo(item.repo())) else {
            return no_item(&issue);
        };
        self.state.write().await.links.insert(link_key(&item.id.link_id()), item.clone());
        match filter {
            Some(account) => self.pin(&account, collection, &item, api, reach).await,
            None => self.write_link(collection, &item, reach).await,
        }
    }

    /// Pins `item` to the filter list `collection` of `account`: its link id joins `pinned`, the
    /// revision goes up by one and the list is fetched again. Pinned already: nothing changes. The
    /// answer is the item as the list holds it (as GitHub just reported it when the fetch failed).
    async fn pin(&self, account: &str, collection: &str, item: &item::Item, api: &Api, reach: Reach<'_>) -> Response {
        let mut added = false;
        let changed = self
            .change_filter_list(account, collection, |list| {
                let pinned = |id: &String| address::issue_ref(id).is_some_and(|pinned| pinned.same(&item.id));
                if !list.pinned.iter().any(pinned) {
                    list.pinned.push(item.id.link_id());
                    list.revision += 1;
                    added = true;
                }
                Ok(())
            })
            .await;
        match changed {
            Ok(list) if added => self.fetch_filter_list(account, &list, Some(api)).await,
            Ok(_) => {}
            Err((code, message)) => return error(code, message),
        }
        let held = {
            let state = self.state.read().await;
            let same = |todo: &&Todo| address::issue_ref(&todo.uid).is_some_and(|held| held.same(&item.id));
            state.filter(collection).and_then(|(_, list)| list.todos.iter().find(same).cloned())
        };
        let todo = held.unwrap_or_else(|| item.to_todo(collection, 0));
        match reach.todo_for(&todo) {
            Some(todo) => Response::Todo { todo },
            None => no_item(&item.id),
        }
    }

    /// Writes a new link task for `item` into the task list `collection` through
    /// [`PimService::save_todo`] (its checks and its cache): GitHub's title, the item's address,
    /// its link id, done when it is closed or merged; nothing else is set.
    async fn write_link(&self, collection: &str, item: &item::Item, reach: Reach<'_>) -> Response {
        let todo = Todo {
            uid: String::new(),
            collection: collection.to_string(),
            href: None,
            etag: None,
            summary: item.title.clone(),
            due: None,
            priority: None,
            completed: item.state != GitHubState::Open,
            parent: None,
            description: None,
            url: Some(item.url.clone()),
            categories: Vec::new(),
            sort_order: None,
            link: Some(Link::github(item.repo(), item.id.number)),
        };
        match self.save_todo(todo).await {
            Ok(saved) => Response::Todo { todo: reach.with_details(saved, &self.state.read().await.links) },
            Err(err) => {
                eprintln!("torroworkplace: link to {} not written: {}", item.id.link_id(), write_code(&err));
                self.write_failed(err).await
            }
        }
    }

    /// The cached task of the list `collection` that links to `issue`, if there is one.
    async fn held_link(&self, collection: &str, issue: &IssueRef) -> Option<Todo> {
        let state = self.state.read().await;
        let todos = state.owner(collection).map(|slot| slot.todos.as_slice()).unwrap_or_default();
        todos
            .iter()
            .find(|todo| todo.collection == collection && linked_issue(todo).is_some_and(|i| i.same(issue)))
            .cloned()
    }

    /// For a client: `todo` links to an item outside its repositories with a link it did not
    /// read so (a new task, or another link than the cached one at its `href`). Refreshing that
    /// link would write the item's title from GitHub into the task, where the client reads it.
    async fn links_elsewhere(&self, reach: Reach<'_>, todo: &Todo) -> bool {
        new_link(&*self.state.read().await, todo).is_some_and(|issue| !reach.reaches_repo(&issue.repo))
    }

    /// For a client: `todo` sets or changes a mail link (`mail:<address>:<Message-ID>`, task E8.7a)
    /// to an account it may not read (read access to `mail` and the account in its
    /// `mail_accounts`). Keeping the link it read (the cached task at its `href` has the same id)
    /// is fine, so a client may still tick a task a person made of a mail.
    async fn links_unread_mail(&self, reach: Reach<'_>, todo: &Todo) -> bool {
        let Reach::Client(rights) = reach else { return false };
        let Some((account, _)) = todo.link.as_ref().and_then(Link::mail_ref) else { return false };
        let state = self.state.read().await;
        let held = todo.href.as_deref().and_then(|href| cached_todo(&state, href)).and_then(|held| held.link.as_ref());
        let kept = held.zip(todo.link.as_ref()).is_some_and(|(held, sent)| held.id == sent.id);
        let reads = rights.area(Area::Mail).allows(Access::Read) && rights.reaches_mail_account(account);
        !(kept || reads)
    }

    /// For a client: why it may not save `todo` – `forbidden` ([`NO_GITHUB_LINK`]) when it sets or
    /// changes a link ([`new_link`]) without read access to `github`. Refreshing that link would
    /// write the item's title and state from GitHub into the task, where the client reads it,
    /// even with every repository in its selection (task G.6b). `None` when it may, for every
    /// save that keeps the link it read (ticking a link a person made is fine), and always for
    /// the suite. The core asks before anyone approves; [`PimService::save_link_todo`] again.
    pub async fn link_refusal(&self, reach: Reach<'_>, todo: &Todo) -> Option<Response> {
        if reach.reads_github() || new_link(&*self.state.read().await, todo).is_none() {
            return None;
        }
        Some(error("forbidden", NO_GITHUB_LINK.into()))
    }

    /// `tasks.save_todo` on a task of a writable list (task G.6): stored as
    /// [`PimService::save_todo`] stores it; when that ticks or unticks a link to an issue
    /// ([`ticked_issue`]: in a private list only, task G.6c) and `reach` may close its issues
    /// ([`Reach::closes_issues_of`]), the issue is then closed as completed or reopened with `api`.
    /// GitHub's refusal (or no `api`) is only logged: the task is stored, and the answer's
    /// `link.github` shows the issue as it is (the app shows "Issue closed" only then). A
    /// client that may not set the link is refused first ([`PimService::link_refusal`]), also for
    /// an approved run.
    pub async fn save_link_todo(&self, todo: Todo, api: Option<&Api>, reach: Reach<'_>) -> Response {
        if let Some(refused) = self.link_refusal(reach, &todo).await {
            return refused;
        }
        let ticked = ticked_issue(&*self.state.read().await, &todo);
        let saved = match self.save_todo(todo).await {
            Ok(saved) => saved,
            Err(err) => return self.write_failed(err).await,
        };
        if let Some(issue) = ticked.filter(|issue| reach.closes_issues_of(&issue.repo)) {
            self.tick_issue(&issue, saved.completed, api).await;
        }
        Response::Todo { todo: reach.with_details(saved, &self.state.read().await.links) }
    }

    /// Closes (`done`) or reopens the linked issue `issue` on GitHub and keeps GitHub's answer in
    /// [`State::links`]. A failure is logged with the link id and its code.
    async fn tick_issue(&self, issue: &IssueRef, done: bool, api: Option<&Api>) {
        let Some(api) = api else {
            eprintln!("torroworkplace: {} not changed on GitHub: no token", issue.link_id());
            return;
        };
        match api.set_issue_done(issue, done).await {
            Ok(item) => {
                self.state.write().await.links.insert(link_key(&item.id.link_id()), item);
            }
            Err(err) => eprintln!("torroworkplace: {} not changed on GitHub: {}", issue.link_id(), err.code()),
        }
    }

    /// Brings the links in the accounts' task lists in step with GitHub (task G.6), with the
    /// filter lists' refresh (start, every five minutes, `accounts.sync`):
    ///
    /// - every linked item once, all in one go ([`Api::items`]: one GraphQL call per 50 items,
    ///   aliases with variables); GitHub's answers replace [`State::links`];
    /// - a task whose title differs, or that is open while its item is closed or merged, is
    ///   written with the new title and done ([`PimService::save_todo`], with its cached etag, so a
    ///   task changed elsewhere meanwhile is left for the next round); nothing else of it changes,
    ///   and nothing is reopened from GitHub: a link ticked by hand (a pull request, or without the
    ///   right to close the issue) stays done.
    ///
    /// Only links in the user's private lists count ([`Slot::private`], task G.6b): in a list
    /// shared with them, or one they share with someone who may write to it, another person could
    /// place a link to a private issue and read its title once the core wrote it there. Links
    /// elsewhere stay as written.
    ///
    /// Without `api` or without links nothing happens; a failed call keeps what was known. Logs
    /// name links by id only.
    pub async fn reconcile_links(&self, api: Option<&Api>) {
        let refs: Vec<IssueRef> = {
            let state = self.state.read().await;
            let mut refs: Vec<IssueRef> = Vec::new();
            for issue in private_todos(&state).filter_map(linked_issue) {
                if !refs.iter().any(|known| known.same(&issue)) {
                    refs.push(issue);
                }
            }
            refs
        };
        let Some(api) = api.filter(|_| !refs.is_empty()) else { return };
        let items = match api.items(&refs).await {
            Ok(items) => items,
            Err(err) => {
                eprintln!("torroworkplace: links not refreshed from GitHub: {}", err.code());
                return;
            }
        };
        let changes: Vec<Todo> = {
            let mut state = self.state.write().await;
            state.links = items.into_iter().map(|item| (link_key(&item.id.link_id()), item)).collect();
            private_todos(&state).filter_map(|todo| in_step(todo, &state.links)).collect()
        };
        for todo in changes {
            let id = todo.link.as_ref().map(|link| link.id.clone()).unwrap_or_default();
            if let Err(err) = self.save_todo(todo).await {
                eprintln!("torroworkplace: link {id} not brought in step with GitHub: {}", write_code(&err));
            }
        }
    }

    /// All accounts are read and each synced once or failed to (task G.6): the core waits for it
    /// before it first brings the links in step with GitHub.
    pub async fn cache_ready(&self) -> bool {
        let state = self.state.read().await;
        state.loaded && state.accounts.iter().all(|slot| slot.last_sync.is_some() || slot.problem.is_some())
    }

    /// Puts `todos` into the cache of the first account as a sync would, for the core's handler
    /// tests (task G.6).
    #[cfg(test)]
    pub async fn cache_todos(&self, todos: Vec<Todo>) {
        if let Some(slot) = self.state.write().await.accounts.first_mut() {
            slot.todos = todos;
        }
    }

    /// Makes the list `collection` one that is not private to the user (shared with them, or
    /// writable by someone they share it with), as a sync would find it (task G.6b).
    #[cfg(test)]
    pub async fn share_list(&self, collection: &str) {
        for slot in &mut self.state.write().await.accounts {
            slot.private.retain(|id| id != collection);
        }
    }

    /// Gives the cached collection `collection` the display name `name`, as a sync would.
    #[cfg(test)]
    pub async fn rename_list(&self, collection: &str, name: &str) {
        let mut state = self.state.write().await;
        let lists = state.accounts.iter_mut().flat_map(|slot| &mut slot.collections);
        for list in lists.filter(|list| list.id == collection) {
            list.name = name.into();
        }
    }

    /// The tasks cached for the first account, as stored there (without GitHub's details).
    #[cfg(test)]
    pub async fn cached_todos(&self) -> Vec<Todo> {
        self.state.read().await.accounts.first().map(|slot| slot.todos.clone()).unwrap_or_default()
    }
}

/// The link `todo` sets or changes: the issue it links to, unless the cached task at its `href`
/// links to the same one (a new task, or another link than the one read, is new).
fn new_link(state: &State, todo: &Todo) -> Option<IssueRef> {
    let issue = linked_issue(todo)?;
    let held = todo.href.as_deref().and_then(|href| cached_todo(state, href)).and_then(linked_issue);
    (!held.is_some_and(|held| held.same(&issue))).then_some(issue)
}

/// The cached tasks of all accounts that are in a private list ([`Slot::private`]): the only ones
/// whose links the core brings in step with GitHub (task G.6b).
fn private_todos(state: &State) -> impl Iterator<Item = &Todo> {
    state.accounts.iter().flat_map(|slot| slot.todos.iter().filter(|todo| slot.private.contains(&todo.collection)))
}

/// The GitHub issue or pull request the task `todo` links to (`link.id`
/// `github:<owner>/<repo>#<n>`), if any.
fn linked_issue(todo: &Todo) -> Option<IssueRef> {
    let (repo, number) = todo.link.as_ref()?.github_ref()?;
    Some(IssueRef { repo: repo.to_string(), number })
}

/// The issue that saving `todo` closes or reopens on GitHub (task G.6): the cached task at its
/// `href` links to it, `todo` keeps that link, `completed` changes, and as far as the core knows
/// ([`State::links`]) it is an issue whose state is the other one. `None` for every other save,
/// also for a new task and for a pull request.
///
/// Also `None` for a task in a list that is not private to the user ([`Slot::private`], task
/// G.6c): someone who may write to that list could have placed the link there, and the user's tick
/// would close an issue they never meant. Ticking it there only marks the task done.
fn ticked_issue(state: &State, todo: &Todo) -> Option<IssueRef> {
    let held = cached_todo(state, todo.href.as_deref()?)?;
    if !state.owner(&held.collection).is_some_and(|slot| slot.private.contains(&held.collection)) {
        return None;
    }
    let issue = linked_issue(held)?;
    if held.completed == todo.completed || !linked_issue(todo).is_some_and(|sent| sent.same(&issue)) {
        return None;
    }
    match held.link.as_ref().and_then(|link| state.links.get(&link_key(&link.id))) {
        Some(item) if item.kind != GitHubKind::Issue => None,
        Some(item) if (item.state != GitHubState::Open) == todo.completed => None,
        _ => Some(issue),
    }
}

/// The link task `todo` in step with what GitHub reports in `links`: GitHub's title, and done
/// when the item is closed or merged. `None` when nothing changes, for a task GitHub said nothing
/// about, and for one that was never stored.
fn in_step(todo: &Todo, links: &HashMap<String, item::Item>) -> Option<Todo> {
    let item = links.get(&link_key(&todo.link.as_ref()?.id))?;
    todo.href.as_ref()?;
    let summary = if item.title.trim().is_empty() { todo.summary.clone() } else { item.title.clone() };
    let completed = todo.completed || item.state != GitHubState::Open;
    (summary != todo.summary || completed != todo.completed).then(|| Todo { summary, completed, ..todo.clone() })
}

/// The API error code of a failed write, for a log line.
fn write_code(err: &WriteError) -> &'static str {
    match err {
        WriteError::Refused { code, .. } => code,
        WriteError::Provider(Failed { err: ProviderError::Auth, .. }) => "auth",
        WriteError::Provider(Failed { err: ProviderError::Offline, .. }) => "offline",
        WriteError::Provider(Failed { err: ProviderError::Conflict { .. }, .. }) => "conflict",
        WriteError::Provider(_) => "error",
    }
}

/// The answer for an item GitHub does not have, or that a client may not see: `not-found`.
fn no_item(issue: &IssueRef) -> Response {
    error("not-found", format!("GitHub has no issue or pull request {}, or it cannot be reached", issue.link_id()))
}

/// Name, colour and searches of a filter list as `tasks.create_filter_list` and
/// `tasks.update_filter_list` send them, before [`checked_definition`].
#[derive(Debug, Clone, Copy)]
pub struct Definition<'a> {
    pub name: &'a str,
    /// `#RRGGBB`, or none for the app's default.
    pub color: Option<&'a str>,
    /// GitHub searches or addresses of issue lists on github.com.
    pub queries: &'a [String],
}

/// Name, colour and searches of a filter list as they are stored, or the refusal that says what
/// is wrong: `error` for what does not fit, `forbidden` with a fixed message for a search outside
/// the repositories of `reach` ([`Reach::confines`]). Pure, so a refused list reaches neither the
/// file nor GitHub.
fn checked_definition(
    definition: Definition<'_>,
    reach: Reach<'_>,
) -> Result<(String, Option<String>, Vec<String>), Refusal> {
    let refuse = |err: torro_pim_github::Error| (err.code(), err.to_string());
    let name = lists::check_name(definition.name).map_err(refuse)?;
    let color = lists::check_color(definition.color).map_err(refuse)?;
    let queries = lists::check_queries(definition.queries).map_err(refuse)?;
    if !reach.confines(&queries) {
        return Err(("forbidden", scope::OUTSIDE.into()));
    }
    Ok((name, color, queries))
}

/// The item the filter list `list` holds under `uid` (its link id).
fn filter_item<'a>(list: &'a FilterCache, uid: &str) -> Option<&'a Todo> {
    list.todos.iter().find(|todo| todo.uid == uid)
}

/// The issue the filter list item `todo` stands for; `None` for a pull request (or an item without
/// what GitHub reports), which is never closed here (`docs/github.md`, "Abhaken").
fn closable_issue(todo: &Todo) -> Option<IssueRef> {
    let github = todo.link.as_ref()?.github.as_ref()?;
    (github.kind == GitHubKind::Issue).then(|| address::issue_ref(&todo.uid)).flatten()
}

/// Whether `sent` changes more of an item of a filter list than whether it is done, compared with
/// `shown`, the item as the sender may see it: title, notes, due date, priority, parent, tags, web
/// address, where it lives (`href`) and which item it links. What only the core fills in (the
/// link's details, `sort_order`) is not compared, so a link may come back with its id alone.
fn changes_more_than_done(sent: &Todo, shown: &Todo) -> bool {
    let link_id = |todo: &Todo| todo.link.as_ref().map(|link| link.id.clone());
    sent.summary != shown.summary
        || sent.description != shown.description
        || sent.due != shown.due
        || sent.priority != shown.priority
        || sent.parent != shown.parent
        || sent.categories != shown.categories
        || sent.url != shown.url
        || sent.href != shown.href
        || link_id(sent) != link_id(shown)
}

/// Whether saving `sent` over the stored task `held` changes nothing but whether it is done (task
/// G.6b): no field [`changes_more_than_done`] compares, the same list and the same manual position.
fn only_ticks(sent: &Todo, held: &Todo) -> bool {
    !changes_more_than_done(sent, held) && sent.collection == held.collection && sent.sort_order == held.sort_order
}

/// The message of a failed close or reopen for `reach`: GitHub's own words for the suite; for a
/// client the fixed text of its code, [`torro_pim_github::CHANGE_FAILED`] in place of GitHub's
/// words, which may name repositories it may not see.
fn change_message(reach: Reach<'_>, err: &torro_pim_github::Error) -> String {
    match (reach, err) {
        (Reach::Suite, err) => err.to_string(),
        (Reach::Client(_), torro_pim_github::Error::Github(_)) => torro_pim_github::CHANGE_FAILED.into(),
        (Reach::Client(_), err) => err.public_message(),
    }
}

/// The message of a failure to read repositories from GitHub for `reach`: GitHub's own words for
/// the suite; for a client the fixed text of its code, [`torro_pim_github::GITHUB_READ_FAILED`] in
/// place of GitHub's words, which may name repositories it may not see.
fn read_message(reach: Reach<'_>, err: &torro_pim_github::Error) -> String {
    match (reach, err) {
        (Reach::Suite, err) => err.to_string(),
        (Reach::Client(_), torro_pim_github::Error::Github(_)) => torro_pim_github::GITHUB_READ_FAILED.into(),
        (Reach::Client(_), err) => err.public_message(),
    }
}

/// The answer for a repository GitHub does not have, the token does not reach or a client may not
/// see: the same `not-found` for all three (the core answers the last one with it too, task G.9b).
pub(crate) fn no_repo(repo: &str) -> Response {
    error("not-found", format!("no repository {repo:?}"))
}

/// The answer for a collection that is no filter list: `not-found`.
fn no_filter_list(collection: &str) -> Response {
    error("not-found", format!("no filter list {collection:?}"))
}

/// One entry per account with all its cached collections, hidden ones included: the control
/// centre shows a switch for each. A sign-in that runs is listed last with an empty id, so the
/// add-account dialog can follow it.
fn account_infos(state: &State) -> Vec<AccountInfo> {
    let mut infos: Vec<AccountInfo> = state.accounts.iter().map(Slot::info).collect();
    if let Some((server, login_url)) = &state.signing_in {
        infos.push(AccountInfo {
            id: String::new(),
            kind: "nextcloud".into(),
            label: server.clone(),
            state: AccountState::SigningIn { server: server.clone(), login_url: login_url.clone() },
            collections: Vec::new(),
            scopes: None,
            mail: None,
            ntfy: None,
        });
    }
    infos
}

/// `https://cloud.example.org/` → `cloud.example.org`; a sub-path stays, like in the
/// account id (`torro_core::Account::nextcloud`).
fn host(server: &str) -> &str {
    server.split_once("://").map_or(server, |(_, rest)| rest).trim_end_matches('/')
}

/// The cached task stored at `href`, of any account, compared exactly: hrefs come from
/// `tasks.list_todos`, which answers from this cache.
fn cached_todo<'a>(state: &'a State, href: &str) -> Option<&'a Todo> {
    state.accounts.iter().flat_map(|slot| &slot.todos).find(|todo| todo.href.as_deref() == Some(href))
}

/// The ids of `collections`.
fn ids(collections: Vec<Collection>) -> Vec<String> {
    collections.into_iter().map(|c| c.id).collect()
}

/// What one sync of an account brings, with ids `<account-id>/<url>`.
struct Fetched {
    collections: Vec<Collection>,
    /// The ids of the private ones among `collections` ([`Slot::private`]).
    private: Vec<String>,
    events: Vec<Event>,
    window: (DateTime<Utc>, DateTime<Utc>),
    todos: Vec<Todo>,
}

/// Everything the cache of `account` holds, fetched from its server; the server's URLs become
/// collection ids with the account in front. Nextcloud's placeholder names are translated only
/// for a `nextcloud` account: on another server they are what the user named the collection.
/// With `wanted` (a mail account, task M2.7) only the collections of those services are kept, and
/// the others are not even read.
async fn fetch_all(
    client: &CalDavClient,
    account: &str,
    nextcloud: bool,
    wanted: Option<Services>,
) -> Result<Fetched, ProviderError> {
    let today = Local::now().date_naive();
    let start = today - Span::days(WINDOW_BACK_DAYS);
    let end = today + Span::days(WINDOW_AHEAD_DAYS);
    let window = (start.and_time(chrono::NaiveTime::MIN).and_utc(), end.and_time(chrono::NaiveTime::MIN).and_utc());
    let mut listed = client.collections_with_privacy().await?;
    listed.retain(|(collection, _)| wanted.is_none_or(|wanted| chosen(wanted, collection.kind)));
    let private: Vec<String> = listed
        .iter()
        .filter(|(_, private)| *private)
        .map(|(collection, _)| collection_id(account, &collection.id))
        .collect();
    let mut collections: Vec<Collection> = listed.into_iter().map(|(collection, _)| collection).collect();
    let mut events = Vec::new();
    let mut todos = Vec::new();
    for collection in &mut collections {
        if let Some(name) = placeholder_name(&collection.name).filter(|_| nextcloud) {
            collection.name = name.to_string();
        }
        let url = std::mem::take(&mut collection.id);
        let id = collection_id(account, &url);
        if collection.kind.has_events() {
            let found = client.events(&url, window.0, window.1).await?;
            events.extend(found.into_iter().map(|e| Event { collection: id.clone(), ..e }));
        }
        if collection.kind.has_todos() {
            todos.extend(client.todos(&url).await?.into_iter().map(|t| Todo { collection: id.clone(), ..t }));
        }
        collection.id = id;
    }
    Ok(Fetched { collections, private, events, window, todos })
}

/// Whether a collection of `kind` serves a service of `wanted` (task M7.1): one with events and
/// tasks stays when either is chosen.
fn chosen(wanted: Services, kind: CollectionKind) -> bool {
    (wanted.calendars && kind.has_events()) || (wanted.tasks && kind.has_todos())
}

/// The HTTP client of the calendar part of mail accounts (task M2.7b): no proxy, and its DNS is
/// discovery's guard ([`discover::GuardedDns`] with [`discover::is_public`]): a name resolves only
/// when every address is on the public internet, and reqwest connects to exactly what it
/// returned, so checking and connecting can never see different addresses. (An address written
/// as an IP literal is not resolved; a mail account's CalDAV address was refused at connecting if
/// it was a private one.)
fn public_http() -> Result<reqwest::Client, String> {
    let guarded = discover::GuardedDns::new(Arc::new(discover::SystemResolver::new()), discover::is_public);
    reqwest::Client::builder()
        .user_agent("Torro Workplace")
        .no_proxy()
        .dns_resolver(Arc::new(guarded))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|err| err.to_string())
}

/// A client for signing in at the found CalDAV address of `dav` (task M2.7b): its host is resolved
/// once, every address must pass `allowed` ([`discover::is_public`] in the core), and the sign-in
/// goes to those addresses only ([`CalDavClient::pinned`]), so a name that answers differently
/// than it did for discovery (DNS rebinding) never gets the password. `Offline` when the name
/// cannot be resolved, `Other` for an address that is no URL or a host with no allowed address.
async fn pinned_public(
    dav: &CalDavAccount,
    password: Secret,
    allowed: fn(std::net::IpAddr) -> bool,
) -> Result<CalDavClient, ProviderError> {
    let url = reqwest::Url::parse(&dav.url).map_err(|err| ProviderError::Other(err.to_string()))?;
    let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
        return Err(ProviderError::Other(format!("{} names no server", dav.url)));
    };
    // `Url` writes an IPv6 host in brackets; the resolver wants it without.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let found: Vec<std::net::SocketAddr> =
        tokio::net::lookup_host((host, port)).await.map_err(|_| ProviderError::Offline)?.collect();
    if found.is_empty() || !found.iter().all(|address| allowed(address.ip())) {
        return Err(ProviderError::Other(format!("{host} is not on the public internet")));
    }
    CalDavClient::pinned(dav, password, &found)
}

/// The host of `url` (`https://dav.example.org/x/` → `dav.example.org`), else `url` itself.
fn host_of(url: &str) -> String {
    reqwest::Url::parse(url).ok().and_then(|url| url.host_str().map(str::to_string)).unwrap_or_else(|| url.into())
}

/// Nextcloud stores translation keys instead of names for the collections it creates on its
/// own and leaves translating them to the clients.
fn placeholder_name(name: &str) -> Option<&'static str> {
    match name {
        "DEFAULT_TASK_CALENDAR_NAME" => Some(tr("Tasks")),
        "DEFAULT_CALENDAR_NAME" => Some(tr("Personal")),
        _ => None,
    }
}

/// The old href when an item moves to another collection: its href is not an item directly in
/// `collection` (a URL) any more, resolved as the request will be ([`item_url`]).
fn moved(href: Option<&str>, collection: &str) -> Option<String> {
    href.filter(|href| item_url(collection, href).is_none()).map(str::to_string)
}

/// The cached window covers `[start, end)`, with a day of slack for all-day entries
/// whose dates are compared in local time.
fn covers(window: (DateTime<Utc>, DateTime<Utc>), start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    window.0 + Span::days(1) <= start && end + Span::days(1) <= window.1
}

/// The event overlaps `[start, end)`. All-day dates count from local midnight.
fn overlaps(event: &Event, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    let instant = |time: EventTime| match time {
        EventTime::DateTime(at) => at,
        EventTime::Date(date) => torro_local_midnight(date),
    };
    let from = instant(event.start);
    let to = event.end.map(instant).unwrap_or(from);
    from < end && (to > start || (to == from && from >= start))
}

fn torro_local_midnight(date: chrono::NaiveDate) -> DateTime<Utc> {
    date.and_time(chrono::NaiveTime::MIN)
        .and_local_timezone(Local)
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| date.and_time(chrono::NaiveTime::MIN).and_utc())
}

fn not_connected() -> ProviderError {
    ProviderError::Other("not connected to a Nextcloud".into())
}

/// The longest name a new task list may have, as for filter lists.
pub const MAX_LIST_NAME: usize = 200;

/// How many addresses a new task list tries before giving up ([`random_suffixes`]).
const LIST_ATTEMPTS: usize = 5;

/// The random parts for the addresses of a new task list, one per attempt: 8 hex digits each, so
/// two lists of the same name never share an address and a taken one is unlikely to come twice.
fn random_suffixes() -> Result<Vec<String>, String> {
    (0..LIST_ATTEMPTS)
        .map(|_| {
            let mut bytes = [0u8; 4];
            getrandom::fill(&mut bytes).map_err(|err| format!("cannot make an address for the list: {err}"))?;
            Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
        })
        .collect()
}

/// Where a write goes: the account that holds the collection, and its connection.
#[derive(Debug, Clone)]
struct Place {
    account: String,
    /// The collection id, `<account>/<url>`.
    id: String,
    /// The collection URL, which the CalDAV client and [`item_url`] take.
    url: String,
    client: CalDavClient,
}

/// A provider failure and the account whose server answered with it, if one did: a refused app
/// password is checked for a remote wipe of that account only.
#[derive(Debug, PartialEq)]
struct Failed {
    account: Option<String>,
    err: ProviderError,
}

impl Failed {
    /// The answer of the server of `account`.
    fn at(account: &str, err: ProviderError) -> Failed {
        Failed { account: Some(account.to_string()), err }
    }
}

/// Why a write failed: a provider's answer, or the core's own refusal before anything was sent.
#[derive(Debug, PartialEq)]
enum WriteError {
    Provider(Failed),
    /// `code` is one of the API's error codes (`docs/api.md`), `message` is for people.
    Refused {
        code: &'static str,
        message: String,
    },
}

impl From<Failed> for WriteError {
    fn from(failed: Failed) -> WriteError {
        WriteError::Provider(failed)
    }
}

/// A failure no server answered with (not connected, no such collection).
impl From<ProviderError> for WriteError {
    fn from(err: ProviderError) -> WriteError {
        WriteError::Provider(Failed { account: None, err })
    }
}

fn refused(code: &'static str, message: String) -> WriteError {
    WriteError::Refused { code, message }
}

/// The refusal for a series: `unsupported` until series editing exists.
fn series(role: Role) -> WriteError {
    let message = match role {
        Role::Calendar => "recurring events cannot be edited yet",
        Role::TaskList => "recurring tasks cannot be edited yet",
    };
    refused("unsupported", message.into())
}

/// An item on the server that a write is about to change, move away or delete.
#[derive(Debug, Clone, Copy)]
struct Item<'a> {
    role: Role,
    /// The URL of the collection that holds it; `href` must be an item directly in it
    /// ([`item_url`]).
    collection: &'a str,
    href: &'a str,
    /// The version the client read; `None` is refused.
    etag: Option<&'a str>,
    uid: &'a str,
}

impl<'a> Item<'a> {
    fn new(role: Role, collection: &'a str, href: &'a str, etag: Option<&'a str>, uid: &'a str) -> Item<'a> {
        Item { role, collection, href, etag, uid }
    }
}

/// Whether the event at `url` cached for `slot` is a series, when the cache holds it in the
/// version `etag`; `None` when it does not (not cached, or cached in another version), so the
/// server decides. A series is cached once per occurrence, all with the same href and etag.
fn cached_series(slot: &Slot, url: &reqwest::Url, etag: &str) -> Option<bool> {
    let resolves = |e: &Event| {
        let collection = slot.url(&e.collection)?;
        e.href.as_deref().and_then(|href| item_url(collection, href))
    };
    let cached: Vec<&Event> = slot.events.iter().filter(|e| resolves(e).as_ref() == Some(url)).collect();
    let same_version = !cached.is_empty() && cached.iter().all(|e| e.etag.as_deref() == Some(etag));
    same_version.then(|| cached.iter().any(|e| e.recurring))
}

fn error(code: &str, message: String) -> Response {
    Response::Error { code: code.into(), message }
}

/// The answer when discovery could not search at all (task M7.1).
fn discover_error(err: DiscoverError) -> Response {
    match err {
        DiscoverError::NoDomain => error("error", tr("Enter an e-mail address or a domain name.").into()),
        DiscoverError::Offline => error("offline", tr("The server cannot be reached.").into()),
    }
}

/// A provider failure with the code `torro_ipc::pim::PimClient` maps back.
fn provider_error(err: &ProviderError) -> Response {
    let code = match err {
        ProviderError::Auth => "auth",
        ProviderError::Offline => "offline",
        ProviderError::Conflict { .. } => "conflict",
        ProviderError::Other(message) if message.starts_with("not connected") => "not-connected",
        ProviderError::Other(_) => "error",
    };
    error(code, err.to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The account most tests use; the core's handler tests sign it in with
    /// [`PimService::pretend_signed_in`].
    pub(crate) const ANNA: &str = "nextcloud:anna@cloud.example.org";

    pub(crate) fn connected() -> Connected {
        Connected {
            account: torro_core::Account::nextcloud("https://cloud.example.org/", "anna"),
            app_password: Secret::new("secret"),
        }
    }

    /// A service without HTTP client and without a settings file, so no test reaches the network
    /// or the user's settings.
    fn service() -> Arc<PimService> {
        service_with(SettingsStore::without_file())
    }

    fn service_with(settings: SettingsStore) -> Arc<PimService> {
        Arc::new(PimService { http: None, ..PimService::with_settings(settings) })
    }

    fn event(start: DateTime<Utc>, hours: i64) -> Event {
        Event {
            uid: "u".into(),
            collection: cal("work"),
            href: Some(format!("{}u.ics", url("work"))),
            etag: Some("\"1\"".into()),
            summary: "Workshop".into(),
            start: EventTime::DateTime(start),
            end: Some(EventTime::DateTime(start + Span::hours(hours))),
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
        }
    }

    fn todo_in(collection: &str, href: Option<String>) -> Todo {
        Todo {
            uid: "t".into(),
            collection: collection.into(),
            href,
            etag: None,
            summary: "Water plants".into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
        }
    }

    const CALENDARS: &str = "https://cloud.example.org/remote.php/dav/calendars/anna/";

    /// The URL of anna's calendar `name`, as the server lists it.
    fn url(name: &str) -> String {
        format!("{CALENDARS}{name}/")
    }

    /// The collection id of anna's calendar `name`: `<account>/<url>`.
    fn cal(name: &str) -> String {
        collection_id(ANNA, &url(name))
    }

    /// `name` of [`cal`]`(name)`, for short assertions.
    fn short(id: &str) -> String {
        id.trim_start_matches(&format!("{ANNA}/{CALENDARS}")).trim_end_matches('/').to_string()
    }

    fn collection(id: &str) -> Collection {
        Collection {
            id: id.into(),
            name: id.into(),
            kind: torro_pim::CollectionKind::Events,
            color: None,
            writable: true,
            source: None,
        }
    }

    fn of_kind(id: &str, kind: torro_pim::CollectionKind) -> Collection {
        Collection { kind, ..collection(id) }
    }

    /// Anna's account without connection, holding `collections`.
    fn slot_with(collections: Vec<Collection>) -> Slot {
        Slot { collections, ..Slot::new(connected(), None) }
    }

    /// All collections the service caches, of every account.
    async fn cached(service: &PimService) -> Vec<Collection> {
        service.state.read().await.collections().cloned().collect()
    }

    /// An open issue as GitHub's search lists it.
    fn found_issue(repo: &str, number: u64) -> item::Item {
        item::Item {
            id: IssueRef { repo: repo.into(), number },
            kind: GitHubKind::Issue,
            state: GitHubState::Open,
            title: format!("{repo}#{number}"),
            body: String::new(),
            url: format!("https://github.com/{repo}/issues/{number}"),
            updated_at: "2026-10-01T10:00:00Z".into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: None,
            milestone_due: None,
            parent: None,
            draft: false,
            review: None,
            checks: None,
            conflict: false,
        }
    }

    #[tokio::test]
    async fn a_filter_list_hands_out_its_items_in_the_order_of_the_search() {
        let service = service();
        let list = FilterList {
            id: "bugs".into(),
            name: "Bugs".into(),
            color: None,
            queries: vec!["is:open sort:updated-desc".into()],
            pinned: Vec::new(),
            revision: 1,
        };
        let account = "github:mahype";
        let mut cache = FilterCache::new(account, list.clone());
        // Neither by title nor by number: the order GitHub answered in (#9, #2, #30, #5).
        let items: Vec<_> = [9, 2, 30, 5].into_iter().map(|n| found_issue("mahype/TorroWorkplace", n)).collect();
        cache.apply(account, &list, Ok(github_api::Fetched { items, truncated: false }));
        let id = cache.collection.id.clone();
        service.state.write().await.filter_slot_mut(account).lists.push(cache);

        let Response::Todos { todos } = service.handle_within(Request::Todos, Reach::Suite).await else {
            panic!("todos")
        };
        let shown: Vec<_> = todos.iter().filter(|t| t.collection == id).collect();
        let numbers: Vec<_> = shown.iter().map(|t| t.uid.rsplit('#').next().unwrap_or_default()).collect();
        assert_eq!(numbers, ["9", "2", "30", "5"]);
        assert_eq!(shown.iter().map(|t| t.sort_order).collect::<Vec<_>>(), [Some(0), Some(1), Some(2), Some(3)]);
    }

    #[tokio::test]
    async fn without_account_reads_are_empty_and_writes_fail() {
        let service = service();
        assert_eq!(service.account_state().await, AccountState::NotConnected);
        assert_eq!(service.handle_within(Request::Todos, Reach::Suite).await, Response::Todos { todos: vec![] });
        let now = Utc::now();
        assert_eq!(
            service.handle_within(Request::Events { start: now, end: now + Span::days(1) }, Reach::Suite).await,
            Response::Events { events: vec![] }
        );
        let response = service.handle_within(Request::SaveEvent { event: event(now, 1) }, Reach::Suite).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "not-connected"), "{response:?}");
        assert_eq!(service.sync().await, Err(not_connected()));
    }

    #[tokio::test]
    async fn recurring_occurrences_are_refused() {
        let service = service();
        let response = service
            .handle_within(
                Request::DeleteEvent { event: Event { recurring: true, ..event(Utc::now(), 1) } },
                Reach::Suite,
            )
            .await;
        assert!(
            matches!(response, Response::Error { ref code, ref message } if code == "unsupported" && message.contains("recurring")),
            "{response:?}"
        );
    }

    #[test]
    fn window_and_overlap() {
        let now = Utc::now();
        let window = (now - Span::days(90), now + Span::days(365));
        assert!(covers(window, now, now + Span::days(7)));
        assert!(!covers(window, now - Span::days(100), now));
        let e = event(now, 2);
        assert!(overlaps(&e, now - Span::hours(1), now + Span::hours(1)));
        assert!(!overlaps(&e, now + Span::hours(2), now + Span::hours(3)));
    }

    #[test]
    fn nextcloud_placeholder_names_are_translated() {
        assert_eq!(placeholder_name("DEFAULT_TASK_CALENDAR_NAME"), Some(tr("Tasks")));
        assert_eq!(placeholder_name("Persönlich"), None);
    }

    #[test]
    fn no_account_and_no_sign_in_lists_nothing() {
        assert_eq!(account_infos(&State::default()), vec![]);
    }

    #[test]
    fn a_running_sign_in_is_listed_last_without_id() {
        let state = State {
            accounts: vec![slot_with(vec![])],
            signing_in: Some(("cloud.example.org".into(), "https://cloud.example.org/login/v2/flow/x".into())),
            filters: Vec::new(),
            loaded: true,
            ..State::default()
        };
        let infos = account_infos(&state);
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].id, ANNA);
        assert_eq!(
            infos[1],
            AccountInfo {
                id: String::new(),
                kind: "nextcloud".into(),
                label: "cloud.example.org".into(),
                state: AccountState::SigningIn {
                    server: "cloud.example.org".into(),
                    login_url: "https://cloud.example.org/login/v2/flow/x".into(),
                },
                collections: vec![],
                scopes: None,
                mail: None,
                ntfy: None,
            }
        );
    }

    #[test]
    fn a_connected_account_lists_all_collections() {
        let now = Utc::now();
        let slot = Slot {
            last_sync: Some(now),
            problem: Some("server not reachable".into()),
            ..slot_with(vec![collection(&cal("work")), collection(&cal("home"))])
        };
        let infos = account_infos(&State { accounts: vec![slot], ..State::default() });
        assert_eq!(
            infos,
            vec![AccountInfo {
                id: ANNA.into(),
                kind: "nextcloud".into(),
                label: "anna@cloud.example.org".into(),
                state: AccountState::Connected {
                    server: "https://cloud.example.org".into(),
                    login_name: "anna".into(),
                    last_sync: Some(now),
                    problem: Some("server not reachable".into()),
                },
                collections: vec![collection(&cal("work")), collection(&cal("home"))],
                scopes: None,
                mail: None,
                ntfy: None,
            }]
        );
    }

    fn seen_by(reach: Reach<'_>, all: &[Collection], role: Role, hidden: &[String]) -> Vec<String> {
        all.iter().filter(|c| reach.sees(c, role, hidden)).map(|c| c.id.clone()).collect()
    }

    #[test]
    fn calendars_and_task_lists_are_told_apart_and_hidden_ones_left_out() {
        use torro_pim::CollectionKind::{Both, Events, Todos};
        let all = vec![of_kind("work", Both), of_kind("birthdays", Events), of_kind("home", Todos)];
        assert_eq!(seen_by(Reach::Suite, &all, Role::Calendar, &[]), ["work", "birthdays"]);
        assert_eq!(seen_by(Reach::Suite, &all, Role::TaskList, &[]), ["work", "home"]);
        let hidden = vec!["work".to_string()];
        assert_eq!(seen_by(Reach::Suite, &all, Role::TaskList, &hidden), ["home"]);
    }

    #[test]
    fn a_client_sees_its_selection_per_role_and_nothing_hidden() {
        use torro_ipc::api::{RightsTemplate, Selection};
        use torro_pim::CollectionKind::{Both, Events, Todos};
        let all = vec![of_kind("work", Both), of_kind("birthdays", Events), of_kind("home", Todos)];
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.calendars = Selection::Only(vec!["birthdays".into()]);
        rights.task_lists = Selection::Only(vec!["work".into()]);
        let client = Reach::Client(&rights);
        assert_eq!(seen_by(client, &all, Role::Calendar, &[]), ["birthdays"]);
        assert_eq!(seen_by(client, &all, Role::TaskList, &[]), ["work"]);
        assert!(seen_by(client, &all, Role::TaskList, &["work".to_string()]).is_empty());
    }

    /// A connected service with anna's calendars `work` and `home`, one event in each, inside the
    /// cached window, and a todo in each.
    async fn filled_service(settings: SettingsStore) -> Arc<PimService> {
        let service = service_with(settings);
        let now = Utc::now();
        let http = reqwest::Client::new();
        let mut slot =
            Slot::new(connected(), Some(account::caldav(&http, &connected()).expect("a client, no network")));
        slot.collections = vec![
            of_kind(&cal("work"), torro_pim::CollectionKind::Both),
            of_kind(&cal("home"), torro_pim::CollectionKind::Both),
        ];
        slot.window = Some((now - Span::days(90), now + Span::days(365)));
        for name in ["work", "home"] {
            let id = cal(name);
            slot.events.push(Event {
                collection: id.clone(),
                href: Some(format!("{}e.ics", url(name))),
                ..event(now, 1)
            });
            slot.todos.push(Todo { uid: id.clone(), ..todo_in(&id, Some(format!("{}t.ics", url(name)))) });
        }
        service.state.write().await.accounts.push(slot);
        service
    }

    fn collections_of(response: Response) -> Vec<String> {
        match response {
            Response::Collections { collections } => collections.iter().map(|c| short(&c.id)).collect(),
            Response::Events { events } => events.iter().map(|e| short(&e.collection)).collect(),
            Response::Todos { todos } => todos.iter().map(|t| short(&t.collection)).collect(),
            other => panic!("expected a list, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_client_reads_only_its_shown_selection() {
        use torro_ipc::api::{RightsTemplate, Selection};
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = SettingsStore::at(dir.path().join("settings.json"));
        let service = filled_service(settings.clone()).await;
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.calendars = Selection::Only(vec![cal("work")]);
        let client = Reach::Client(&rights);
        let now = Utc::now();
        let events = || Request::Events { start: now - Span::days(1), end: now + Span::days(1) };

        assert_eq!(collections_of(service.handle_within(Request::CalendarCollections, client).await), ["work"]);
        assert_eq!(collections_of(service.handle_within(events(), client).await), ["work"]);
        assert_eq!(collections_of(service.handle_within(Request::Todos, client).await), ["work", "home"]);
        assert_eq!(
            collections_of(service.handle_within(events(), Reach::Suite).await),
            ["work", "home"],
            "the suite sees all"
        );

        settings.set_collection_shown(&cal("work"), false).expect("hide work");
        assert!(collections_of(service.handle_within(events(), client).await).is_empty());
        assert_eq!(collections_of(service.handle_within(Request::TaskCollections, client).await), ["home"]);
        assert_eq!(
            collections_of(service.handle_within(Request::Todos, Reach::Suite).await),
            ["home"],
            "hidden for the suite too"
        );
    }

    #[tokio::test]
    async fn a_selection_with_the_bare_url_of_before_reaches_nothing() {
        use torro_ipc::api::{RightsTemplate, Selection};
        let service = filled_service(SettingsStore::without_file()).await;
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.calendars = Selection::Only(vec![url("work")]);
        assert!(
            collections_of(service.handle_within(Request::CalendarCollections, Reach::Client(&rights)).await)
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_client_lists_accounts_without_collections_it_may_not_see() {
        use torro_ipc::api::{RightsTemplate, Selection};
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = SettingsStore::at(dir.path().join("settings.json"));
        let service = filled_service(settings.clone()).await;
        settings.set_collection_shown(&cal("home"), false).expect("hide home");
        let rights = RightsTemplate::ReadOnly.rights();
        let listed =
            |accounts: Vec<AccountInfo>| accounts[0].collections.iter().map(|c| short(&c.id)).collect::<Vec<_>>();

        assert_eq!(listed(service.accounts_within(Reach::Client(&rights)).await), ["work"]);
        assert_eq!(
            listed(service.accounts_within(Reach::Suite).await),
            ["work", "home"],
            "the control centre lists hidden ones"
        );
        let mut none = rights.clone();
        none.calendars = Selection::Only(vec![]);
        none.task_lists = Selection::Only(vec![]);
        assert!(listed(service.accounts_within(Reach::Client(&none)).await).is_empty());
    }

    #[tokio::test]
    async fn writes_outside_a_clients_reach_are_named() {
        use torro_ipc::api::{RightsTemplate, Selection};
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = SettingsStore::at(dir.path().join("settings.json"));
        let service = filled_service(settings.clone()).await;
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.calendars = Selection::Only(vec![cal("work")]);
        let client = Reach::Client(&rights);
        let now = Utc::now();
        let save = |collection: &str, href: Option<&str>| Request::SaveEvent {
            event: Event { collection: collection.into(), href: href.map(str::to_string), ..event(now, 1) },
        };
        let (work, home) = (cal("work"), cal("home"));
        let (work_url, home_url) = (url("work"), url("home"));
        let service = &service;
        let out = |request| async move { service.out_of_reach(client, &request).await };

        assert_eq!(out(save(&work, None)).await, None);
        assert_eq!(out(save(&work, Some(&format!("{work_url}e.ics")))).await, None);
        assert_eq!(out(save(&work, Some("/remote.php/dav/calendars/anna/work/e.ics"))).await, None);
        assert_eq!(out(save(&home, None)).await, Some(home.clone()));
        assert_eq!(out(save("nowhere", None)).await, Some("nowhere".into()));
        assert_eq!(out(save(&work_url, None)).await, Some(work_url.clone()), "the bare URL is no id any more");
        // Moving an event out of a calendar it may not see would delete it there.
        assert_eq!(out(save(&work, Some(&format!("{home_url}e.ics")))).await, Some(format!("{home_url}e.ics")));
        // Hrefs that start like `work` but resolve elsewhere: the API check of A.2.
        for href in [
            format!("{work_url}../home/e.ics"),
            format!("{work_url}%2e%2e/home/e.ics"),
            format!("{work_url}%2E%2E/home/e.ics"),
            format!("{work_url}..\\home\\e.ics"),
            format!("{work_url}/e.ics"),
            format!("{work_url}sub/e.ics"),
            format!("{work_url}../../../files/anna/notes.txt"),
            "/remote.php/dav/files/anna/notes.txt".to_string(),
            "https://evil.example.org/remote.php/dav/calendars/anna/work/e.ics".to_string(),
            // The id is no URL: an href must not be resolved against it.
            format!("{work}e.ics"),
        ] {
            assert_eq!(out(save(&work, Some(&href))).await, Some(href.clone()), "{href}");
        }
        assert_eq!(service.out_of_reach(Reach::Suite, &save(&home, None)).await, None);

        settings.set_collection_shown(&work, false).expect("hide work");
        assert_eq!(out(save(&work, None)).await, Some(work.clone()));
        let todo = Request::DeleteTodo { todo: service.state.read().await.accounts[0].todos[1].clone() };
        assert_eq!(out(todo).await, None, "task lists: all");
    }

    #[tokio::test]
    async fn a_move_deletes_only_in_the_collection_that_holds_the_item() {
        let dir = tempfile::tempdir().expect("temp dir");
        let service = filled_service(SettingsStore::at(dir.path().join("settings.json"))).await;
        let place = service.place(&cal("work")).await.expect("work is a place to write to");
        let holder = service.holder_of(&format!("{}e.ics", url("home")), &place).await.expect("home holds it");
        assert_eq!((holder.id, holder.url), (cal("home"), url("home")));
        let outside = format!("{}../../files/anna/notes.txt", url("home"));
        assert!(service.holder_of(&outside, &place).await.is_err(), "no collection holds it, so nothing is written");
        let moved_out = Event { href: Some(outside), ..event(Utc::now(), 1) };
        let response = service.handle_within(Request::SaveEvent { event: moved_out }, Reach::Suite).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "error"), "{response:?}");
    }

    #[tokio::test]
    async fn approvals_learn_the_cached_item_at_the_href_not_the_title_the_client_sends() {
        let service = filled_service(SettingsStore::without_file()).await;
        let start = {
            let mut state = service.state.write().await;
            let slot = &mut state.accounts[0];
            for (collection, name) in slot.collections.iter_mut().zip(["Work", "Home"]) {
                collection.name = name.into();
            }
            slot.events[0].summary = "Board meeting".into();
            slot.todos[1].summary = "File taxes".into();
            slot.events[0].start
        };
        let (work, home) = (cal("work"), cal("home"));
        let board = format!("{}e.ics", url("work"));
        let lunch = |collection: &str, href: Option<&str>| Event {
            collection: collection.into(),
            href: href.map(str::to_string),
            summary: "Lunch".into(),
            ..event(Utc::now(), 1)
        };
        let known = |title: &str, start| Some(Known { title: title.into(), start });

        // Deleting: the item at the href, whatever the client calls it.
        let facts = service.facts(&Request::DeleteEvent { event: lunch(&work, Some(&board)) }).await;
        assert_eq!(
            facts,
            Facts {
                item: known("Board meeting", Some(start)),
                name: Some("Work".into()),
                moved_from: None,
                ..Facts::default()
            }
        );
        // Written another way, the same address: the same item.
        let same = "/remote.php/dav/calendars/anna/work/e.ics";
        let facts = service.facts(&Request::DeleteEvent { event: lunch(&work, Some(same)) }).await;
        assert_eq!(facts.item, known("Board meeting", Some(start)));
        // A version the cache does not hold (the server may have a newer one): unknown.
        let newer = Event { etag: Some("\"2\"".into()), ..lunch(&work, Some(&board)) };
        assert_eq!(service.facts(&Request::DeleteEvent { event: newer }).await.item, None);
        // Nothing cached there, or not an item of the calendar: unknown.
        let missing = format!("{}gone.ics", url("work"));
        assert_eq!(service.facts(&Request::DeleteEvent { event: lunch(&work, Some(&missing)) }).await.item, None);
        let outside = format!("{}../home/e.ics", url("work"));
        let facts = service.facts(&Request::DeleteEvent { event: lunch(&work, Some(&outside)) }).await;
        assert_eq!((facts.item, facts.moved_from), (None, None), "a delete never follows an href elsewhere");

        // Saving into `home` with work's href moves it: the item comes from work, which loses it.
        let facts = service.facts(&Request::SaveEvent { event: lunch(&home, Some(&board)) }).await;
        assert_eq!(
            facts,
            Facts {
                item: known("Board meeting", Some(start)),
                name: Some("Home".into()),
                moved_from: Some("Work".into()),
                ..Facts::default()
            }
        );
        let created = service.facts(&Request::SaveEvent { event: lunch(&home, None) }).await;
        assert_eq!(created, Facts { item: None, name: Some("Home".into()), moved_from: None, ..Facts::default() });

        // Tasks, and reordering by the exact href.
        let taxes = format!("{}t.ics", url("home"));
        let todo = Todo { summary: "Water plants".into(), ..todo_in(&home, Some(taxes.clone())) };
        let facts = service.facts(&Request::DeleteTodo { todo }).await;
        assert_eq!(
            facts,
            Facts { item: known("File taxes", None), name: Some("Home".into()), moved_from: None, ..Facts::default() }
        );
        let reorder = Request::MoveTodo { href: taxes, etag: "1".into(), before: None };
        assert_eq!(
            service.facts(&reorder).await,
            Facts { item: known("File taxes", None), name: Some("Home".into()), moved_from: None, ..Facts::default() }
        );
        let reorder = Request::MoveTodo { href: "/elsewhere.ics".into(), etag: "1".into(), before: None };
        assert_eq!(service.facts(&reorder).await, Facts::default());

        // Collections by name; an unknown one has none.
        let hide = |collection: &str| Request::SetCollectionShown { collection: collection.into(), shown: false };
        assert_eq!(service.facts(&hide(&work)).await.name, Some("Work".into()));
        assert_eq!(service.facts(&hide("nowhere")).await.name, None);
        assert_eq!(service.facts(&Request::Sync).await, Facts::default());
    }

    #[tokio::test]
    async fn a_write_to_a_collection_of_no_account_is_not_found() {
        let service = filled_service(SettingsStore::without_file()).await;
        for collection in [
            url("work"),
            "nextcloud:bob@cloud.example.org/https://cloud.example.org/x/".into(),
            // Anna's id in front, but a collection she never listed.
            format!("{ANNA}/{CALENDARS}elsewhere/"),
            format!("{ANNA}/https://evil.example.org/remote.php/dav/calendars/anna/work/"),
        ] {
            let event = Event { collection, href: None, etag: None, ..event(Utc::now(), 1) };
            let response = service.handle_within(Request::SaveEvent { event }, Reach::Suite).await;
            assert!(matches!(response, Response::Error { ref code, .. } if code == "not-found"), "{response:?}");
        }
    }

    #[tokio::test]
    async fn only_cached_collections_are_known() {
        let service = service();
        assert!(!service.knows_collection(&cal("work")).await);
        service.state.write().await.accounts.push(slot_with(vec![collection(&cal("work"))]));
        assert!(service.knows_collection(&cal("work")).await);
        assert!(!service.knows_collection(&cal("home")).await);
        assert!(!service.knows_collection(&url("work")).await, "nor by the bare URL");
    }

    #[test]
    fn host_strips_scheme_and_slash() {
        assert_eq!(host("https://cloud.example.org/"), "cloud.example.org");
        assert_eq!(host("https://example.org/nextcloud"), "example.org/nextcloud");
        assert_eq!(host("cloud.example.org"), "cloud.example.org");
    }

    #[tokio::test]
    async fn removing_an_unknown_account_is_not_found_and_keeps_the_state() {
        let service = service();
        assert_eq!(code_of(&service.remove_account(ANNA).await), "not-found");
        service.state.write().await.accounts.push(slot_with(vec![]));
        assert_eq!(code_of(&service.remove_account("nextcloud:bob@cloud.example.org").await), "not-found");
        assert_eq!(code_of(&service.remove_account("").await), "not-found");
        assert_eq!(service.accounts_within(Reach::Suite).await.len(), 1);
    }

    #[tokio::test]
    async fn an_account_without_its_app_password_is_listed_with_a_problem_and_can_be_removed() {
        let service = service();
        let bob = torro_core::Account::nextcloud("https://other.example.org", "bob");
        let problem = "Sign in again: no app password is stored on this device".to_string();
        {
            let http = reqwest::Client::new();
            let mut state = service.state.write().await;
            state.accounts.push(Slot::loaded(Stored::Ready(connected()), Some(&http)));
            state.accounts.push(Slot::loaded(Stored::Unusable { account: bob, problem: problem.clone() }, Some(&http)));
            assert!(state.accounts[0].client.is_some() && state.accounts[1].client.is_none(), "nothing to send with");
        }
        let infos = service.accounts_within(Reach::Suite).await;
        assert_eq!(infos.len(), 2, "the account without secret is listed next to the others");
        assert!(matches!(&infos[0].state, AccountState::Connected { problem: None, .. }));
        assert!(
            matches!(&infos[1].state, AccountState::Connected { problem: Some(p), .. } if *p == problem),
            "{:?}",
            infos[1].state
        );
        // Removing it forgets it like any other (keychain stand-in: the secret is gone already).
        service.forget_locally("nextcloud:bob@other.example.org", || async { Ok(()) }).await.expect("removed");
        let left: Vec<String> = service.accounts_within(Reach::Suite).await.into_iter().map(|a| a.id).collect();
        assert_eq!(left, [ANNA]);
    }

    /// A service whose server just refused anna's app password, with a filled cache.
    async fn refused_service() -> Arc<PimService> {
        let service = service();
        let slot = Slot { problem: Some(ProviderError::Auth.to_string()), ..slot_with(vec![collection("work")]) };
        service.state.write().await.accounts.push(slot);
        service
    }

    /// Runs the remote wipe with a scripted check answer and returns its result plus the steps
    /// taken; keychain and network are stand-ins.
    async fn wipe(
        service: &PimService,
        check: Result<bool, ProviderError>,
    ) -> (Result<Option<bool>, String>, Vec<&'static str>) {
        let steps = std::cell::RefCell::new(Vec::new());
        let result = service
            .wipe_if_requested(
                &connected(),
                || {
                    steps.borrow_mut().push("check");
                    async { check }
                },
                || {
                    steps.borrow_mut().push("forget");
                    async { Ok(()) }
                },
                || {
                    steps.borrow_mut().push("report");
                    async { Ok(()) }
                },
            )
            .await;
        (result, steps.into_inner())
    }

    #[tokio::test]
    async fn a_remote_wipe_removes_account_secret_and_cache_and_reports_it() {
        let service = refused_service().await;
        let (result, steps) = wipe(&service, Ok(true)).await;
        assert_eq!(result, Ok(Some(true)));
        assert_eq!(steps, ["check", "forget", "report"]);
        assert_eq!(service.account_state().await, AccountState::NotConnected);
        assert!(service.accounts_within(Reach::Suite).await.is_empty());
        assert!(cached(&service).await.is_empty());
    }

    #[tokio::test]
    async fn a_remote_wipe_removes_only_the_refused_account() {
        let service = refused_service().await;
        let bob = Connected {
            account: torro_core::Account::nextcloud("https://other.example.org", "bob"),
            app_password: Secret::new("other"),
        };
        service
            .state
            .write()
            .await
            .accounts
            .push(Slot { collections: vec![collection("bob")], ..Slot::new(bob, None) });
        let (result, _) = wipe(&service, Ok(true)).await;
        assert_eq!(result, Ok(Some(true)));
        let left: Vec<String> = service.accounts_within(Reach::Suite).await.into_iter().map(|a| a.id).collect();
        assert_eq!(left, ["nextcloud:bob@other.example.org"]);
        assert_eq!(cached(&service).await, vec![collection("bob")]);
    }

    #[tokio::test]
    async fn without_a_remote_wipe_the_account_stays_and_is_checked_only_once() {
        let service = refused_service().await;
        let (result, steps) = wipe(&service, Ok(false)).await;
        assert_eq!(result, Ok(Some(false)));
        assert_eq!(steps, ["check"]);
        assert_eq!(service.accounts_within(Reach::Suite).await.len(), 1);
        assert_eq!(cached(&service).await, vec![collection("work")]);
        // The next refused request does not ask the server again.
        let (result, steps) = wipe(&service, Ok(true)).await;
        assert_eq!(result, Ok(None));
        assert!(steps.is_empty());
        assert_eq!(service.accounts_within(Reach::Suite).await.len(), 1);
    }

    #[tokio::test]
    async fn a_failed_wipe_check_keeps_the_account_and_may_be_retried() {
        let service = refused_service().await;
        let (result, steps) = wipe(&service, Err(ProviderError::Offline)).await;
        assert!(result.is_err());
        assert_eq!(steps, ["check"]);
        assert_eq!(service.accounts_within(Reach::Suite).await.len(), 1);
        assert_eq!(cached(&service).await, vec![collection("work")]);
        let (_, steps) = wipe(&service, Ok(false)).await;
        assert_eq!(steps, ["check"], "a failed check runs again on the next refusal");
    }

    #[tokio::test]
    async fn a_removed_account_leaves_no_hidden_collections_behind_and_the_others_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = SettingsStore::at(dir.path().join("settings.json"));
        let bob_work = "nextcloud:bob@other.example.org/https://other.example.org/dav/work/";
        for hidden in [cal("work").as_str(), "elsewhere", bob_work] {
            settings.set_collection_shown(hidden, false).expect("hide");
        }
        let service = service_with(settings.clone());
        let bob = Connected {
            account: torro_core::Account::nextcloud("https://other.example.org", "bob"),
            app_password: Secret::new("other"),
        };
        {
            let mut state = service.state.write().await;
            state.accounts.push(slot_with(vec![collection(&cal("work")), collection(&cal("home"))]));
            state.accounts.push(Slot { collections: vec![collection(bob_work)], ..Slot::new(bob, None) });
        }
        service.forget_locally(ANNA, || async { Ok(()) }).await.expect("forgotten");
        assert_eq!(settings.current().hidden_collections, ["elsewhere", bob_work]);
        let left: Vec<String> = service.accounts_within(Reach::Suite).await.into_iter().map(|a| a.id).collect();
        assert_eq!(left, ["nextcloud:bob@other.example.org"]);
        assert_eq!(cached(&service).await, vec![collection(bob_work)]);
    }

    #[tokio::test]
    async fn a_failed_forget_keeps_account_and_hidden_collections() {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = SettingsStore::at(dir.path().join("settings.json"));
        settings.set_collection_shown("work", false).expect("hide work");
        let service = service_with(settings.clone());
        service.state.write().await.accounts.push(slot_with(vec![collection("work")]));
        let result = service.forget_locally(ANNA, || async { Err("keychain locked".to_string()) }).await;
        assert!(result.is_err());
        assert_eq!(settings.current().hidden_collections, ["work"]);
        assert_eq!(service.accounts_within(Reach::Suite).await.len(), 1);
    }

    #[tokio::test]
    async fn no_wipe_check_for_an_account_that_is_gone() {
        let service = service();
        let (result, steps) = wipe(&service, Ok(true)).await;
        assert_eq!(result, Ok(None));
        assert!(steps.is_empty());
    }

    #[test]
    fn moving_is_detected_by_href() {
        let collection = "https://cloud/dav/calendars/a/work/";
        assert_eq!(moved(Some("https://cloud/dav/calendars/a/work/u.ics"), collection), None);
        assert_eq!(
            moved(Some("https://cloud/dav/calendars/a/home/u.ics"), collection),
            Some("https://cloud/dav/calendars/a/home/u.ics".into())
        );
        assert_eq!(moved(None, collection), None);
    }

    // Against a fake CalDAV server on the loopback interface, so the tests see exactly which
    // requests reach the server and with which preconditions. Nothing leaves the machine.

    /// One request the fake server received.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct Seen {
        method: String,
        path: String,
        if_match: Option<String>,
        if_none_match: Option<String>,
        /// The request body, as text.
        body: String,
    }

    impl Seen {
        /// `"PUT /…/e.ics If-Match \"1\""`, for short assertions.
        fn line(&self) -> String {
            let precondition = match (&self.if_match, &self.if_none_match) {
                (Some(etag), _) => format!(" If-Match {etag}"),
                (None, Some(any)) => format!(" If-None-Match {any}"),
                (None, None) => String::new(),
            };
            format!("{} {}{precondition}", self.method, self.path)
        }
    }

    /// The fake server's answer: status, `ETag` header, body.
    pub(crate) type Reply = (u16, Option<&'static str>, &'static str);

    const SINGLE: &str =
        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:u\r\nDTSTART:20261011T080000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    const SERIES: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:u\r\nDTSTART:20261011T080000Z\r\nRRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    const TASK_SERIES: &str = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:t\r\nSUMMARY:Water plants\r\nRRULE:FREQ=DAILY\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    /// Answers every write with a new version `"2"` and a GET with `item` in version `"1"`.
    fn server_holding(item: &'static str) -> impl Fn(&Seen) -> Reply + Send + Sync + 'static {
        move |seen| match seen.method.as_str() {
            "GET" => (200, Some("\"1\""), item),
            "PUT" => (201, Some("\"2\""), ""),
            "DELETE" => (204, None, ""),
            _ => (405, None, ""),
        }
    }

    const PRINCIPAL: &str = r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/remote.php/dav/</d:href><d:propstat><d:prop><d:current-user-principal><d:href>/remote.php/dav/principals/users/anna/</d:href></d:current-user-principal></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
    const HOME_SET: &str = r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/remote.php/dav/principals/users/anna/</d:href><d:propstat><d:prop><c:calendar-home-set><d:href>/remote.php/dav/calendars/anna/</d:href></c:calendar-home-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
    const ONE_CALENDAR: &str = r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/remote.php/dav/calendars/anna/work/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>Work</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
    const NOTHING: &str = r#"<d:multistatus xmlns:d="DAV:"/>"#;

    /// A Nextcloud with the one calendar `work` and nothing in it: enough for a whole sync.
    pub(crate) fn healthy_server(seen: &Seen) -> Reply {
        match (seen.method.as_str(), seen.path.as_str()) {
            ("PROPFIND", "/remote.php/dav/") => (207, None, PRINCIPAL),
            ("PROPFIND", "/remote.php/dav/principals/users/anna/") => (207, None, HOME_SET),
            ("PROPFIND", "/remote.php/dav/calendars/anna/") => (207, None, ONE_CALENDAR),
            ("REPORT", _) => (207, None, NOTHING),
            _ => (404, None, ""),
        }
    }

    /// A CalDAV server on `127.0.0.1` that answers with `reply` and records every request,
    /// one request per connection. Returns its DAV base URL and the record.
    pub(crate) async fn fake_server(
        reply: impl Fn(&Seen) -> Reply + Send + Sync + 'static,
    ) -> (String, Arc<std::sync::Mutex<Vec<Seen>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let base = format!("http://{}/remote.php/dav/", listener.local_addr().expect("address"));
        let record = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (log, reply) = (Arc::clone(&record), Arc::new(reply));
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let (log, reply) = (Arc::clone(&log), Arc::clone(&reply));
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let head_end = loop {
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break at + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let header = |name: &str| {
                        head.lines()
                            .filter_map(|line| line.split_once(':'))
                            .find(|(key, _)| key.trim().eq_ignore_ascii_case(name))
                            .map(|(_, value)| value.trim().to_string())
                    };
                    let length: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
                    while buf.len() < head_end + length {
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let mut first = head.lines().next().unwrap_or_default().split(' ');
                    let body = buf.get(head_end..).unwrap_or_default();
                    let seen = Seen {
                        method: first.next().unwrap_or_default().to_string(),
                        path: first.next().unwrap_or_default().to_string(),
                        if_match: header("if-match"),
                        if_none_match: header("if-none-match"),
                        body: String::from_utf8_lossy(body).to_string(),
                    };
                    let (status, etag, body) = reply(&seen);
                    log.lock().expect("record").push(seen);
                    let etag = etag.map(|etag| format!("ETag: {etag}\r\n")).unwrap_or_default();
                    let answer = format!(
                        "HTTP/1.1 {status} Fake\r\n{etag}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(answer.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (base, record)
    }

    /// The requests the fake server saw so far, as [`Seen::line`]s.
    pub(crate) fn lines(record: &std::sync::Mutex<Vec<Seen>>) -> Vec<String> {
        record.lock().expect("record").iter().map(Seen::line).collect()
    }

    const WORK: &str = "/remote.php/dav/calendars/anna/work/";
    const HOME: &str = "/remote.php/dav/calendars/anna/home/";

    /// Anna's account on the fake server at `base` (its id names the server's port, so two fake
    /// servers are two accounts), with the collections `work` and `home` and an empty cache.
    fn slot_on(base: &str) -> Slot {
        let origin = base.trim_end_matches("/remote.php/dav/");
        let connected =
            Connected { account: torro_core::Account::nextcloud(origin, "anna"), app_password: Secret::new("secret") };
        let http = reqwest::Client::builder().no_proxy().build().expect("http client");
        let dav = torro_pim_caldav::CalDavAccount { url: base.into(), username: "anna".into() };
        let client = CalDavClient::new(http, &dav, Secret::new("secret")).expect("client");
        let id = connected.account.id.clone();
        let collections: Vec<Collection> = [WORK, HOME]
            .iter()
            .map(|path| of_kind(&collection_id(&id, &format!("{origin}{path}")), torro_pim::CollectionKind::Both))
            .collect();
        // Anna's own lists, shared with nobody.
        let private = collections.iter().map(|c| c.id.clone()).collect();
        Slot { collections, private, ..Slot::new(connected, Some(client)) }
    }

    /// A service connected to the fake server at `base`.
    async fn service_on(base: &str) -> Arc<PimService> {
        let service = service();
        service.state.write().await.accounts.push(slot_on(base));
        service
    }

    /// A service on a fake CalDAV server that takes every write, for the core's tests of
    /// `inbox.create_task_from_item` (task E3.5), with the ids of its task lists `work` and `home`
    /// and what the server saw ([`FakeCalDav::lines`]).
    pub(crate) struct FakeCalDav {
        pub service: Arc<PimService>,
        /// The id of anna's account on the server.
        pub account: String,
        pub work: String,
        pub home: String,
        record: Arc<std::sync::Mutex<Vec<Seen>>>,
    }

    impl FakeCalDav {
        /// The requests the server saw, `"PUT /…/x.ics If-None-Match *"`.
        pub fn lines(&self) -> Vec<String> {
            lines(&self.record)
        }
    }

    /// See [`FakeCalDav`]; the service reads and writes `settings`.
    pub(crate) async fn fake_caldav(settings: SettingsStore) -> FakeCalDav {
        fake_caldav_on(settings, server_holding("")).await
    }

    /// [`fake_caldav`] on a server that answers discovery and makes every new task list
    /// (`MKCALENDAR`, task G.7) at the first address asked for.
    pub(crate) async fn fake_caldav_making_lists(settings: SettingsStore) -> FakeCalDav {
        fake_caldav_on(settings, making_lists(0, 201)).await
    }

    async fn fake_caldav_on(
        settings: SettingsStore,
        reply: impl Fn(&Seen) -> Reply + Send + Sync + 'static,
    ) -> FakeCalDav {
        let (base, record) = fake_server(reply).await;
        let service = service_with(settings);
        service.state.write().await.accounts.push(slot_on(&base));
        let (work, _) = on_server(&base, WORK, "");
        let (home, _) = on_server(&base, HOME, "");
        FakeCalDav { service, account: account_on(&base), work, home, record }
    }

    /// The id of anna's account on the fake server at `base`.
    fn account_on(base: &str) -> String {
        torro_core::Account::nextcloud(base.trim_end_matches("/remote.php/dav/"), "anna").id
    }

    /// The collection id of the collection at `path` of the account at `base` and the href of
    /// `name` in it.
    fn on_server(base: &str, path: &str, name: &str) -> (String, String) {
        let origin = base.trim_end_matches("/remote.php/dav/");
        let id = torro_core::Account::nextcloud(origin, "anna").id;
        let url = format!("{origin}{path}");
        (collection_id(&id, &url), format!("{url}{name}"))
    }

    /// An event in `work` with `href` and `etag`, as list_events returns it.
    fn listed_event(base: &str, etag: Option<&str>) -> Event {
        let (collection, href) = on_server(base, WORK, "e.ics");
        Event { collection, href: Some(href), etag: etag.map(str::to_string), ..event(Utc::now(), 1) }
    }

    /// A task in `work` with `href` and `etag`, as list_todos returns it.
    fn listed_todo(base: &str, etag: Option<&str>) -> Todo {
        let (collection, href) = on_server(base, WORK, "t.ics");
        Todo { etag: etag.map(str::to_string), ..todo_in(&collection, Some(href)) }
    }

    /// Puts two occurrences of `event` into the cache of its account, as the expanded sync does
    /// for a series.
    async fn cache_occurrences(service: &PimService, event: &Event, recurring: bool) {
        let mut state = service.state.write().await;
        let account = state.owner(&event.collection).expect("an account").id().to_string();
        let slot = state.slot_mut(&account).expect("its slot");
        let now = Utc::now();
        slot.window = Some((now - Span::days(90), now + Span::days(365)));
        for days in [0, 7] {
            let start = EventTime::DateTime(now + Span::days(days));
            slot.events.push(Event { recurring, start, ..event.clone() });
        }
    }

    fn code_of(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            _ => "ok",
        }
    }

    // CalDAV accounts of mail providers (task M7.1). The password must never show in an answer,
    // an error, a `Debug` output, a file or a request to anywhere but its own server.

    const PASSWORD: &str = "hunter2-mailbox";

    /// A calendar `work` with events only and a list `tasks` with tasks only, the list named like
    /// Nextcloud's placeholder: on another server it is what the user called it.
    const SPLIT_HOME: &str = r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/remote.php/dav/calendars/anna/work/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>Work</d:displayname><c:supported-calendar-component-set><c:comp name="VEVENT"/></c:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/anna/tasks/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>DEFAULT_TASK_CALENDAR_NAME</d:displayname><c:supported-calendar-component-set><c:comp name="VTODO"/></c:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;

    /// A CalDAV server with [`SPLIT_HOME`] and nothing in it.
    pub(crate) fn split_server(seen: &Seen) -> Reply {
        match (seen.method.as_str(), seen.path.as_str()) {
            ("PROPFIND", "/remote.php/dav/") => (207, None, PRINCIPAL),
            ("PROPFIND", "/remote.php/dav/principals/users/anna/") => (207, None, HOME_SET),
            ("PROPFIND", "/remote.php/dav/calendars/anna/") => (207, None, SPLIT_HOME),
            ("REPORT", _) => (207, None, NOTHING),
            _ => (404, None, ""),
        }
    }

    /// A service with an HTTP client, its settings, account list and keychain (in memory) in
    /// `dir`, and discovery that asks no DNS.
    fn caldav_service(dir: &std::path::Path) -> (Arc<PimService>, crate::account::github::tests::MemoryTokens) {
        let secrets = crate::account::github::tests::MemoryTokens::default();
        let discoverer = Discoverer::new(Arc::new(NoDns)).expect("discovery");
        let service = PimService::with_settings(SettingsStore::at(dir.join("settings.json"))).with_sign_in(
            dir.join(torro_core::account::ACCOUNTS_FILE),
            Arc::new(secrets.clone()),
            discoverer,
        );
        (Arc::new(service), secrets)
    }

    /// Anna's sign-in at `server` with `password`, choosing `calendars` and `tasks`.
    fn sign_in_at(server: Option<&str>, password: &str, calendars: bool, tasks: bool) -> Request {
        Request::ConnectCalDav {
            address: "anna@example.org".into(),
            server: server.map(str::to_string),
            login_name: "anna".into(),
            password: Secret::new(password),
            calendars,
            tasks,
        }
    }

    #[tokio::test]
    async fn a_caldav_account_signs_in_stores_its_password_and_hides_what_was_not_chosen() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (base, record) = fake_server(split_server).await;
        let (service, secrets) = caldav_service(dir.path());

        let response = service.handle_within(sign_in_at(Some(&base), PASSWORD, true, false), Reach::Suite).await;
        let Response::Connected { account, calendars, tasks } = response.clone() else {
            panic!("connected, got {response:?}")
        };
        assert_eq!((calendars, tasks), (true, true), "what the server offers, whatever was chosen");
        let id = torro_core::Account::caldav(&base, "anna", "example.org").id;
        assert_eq!(
            (account.id.as_str(), account.kind.as_str(), account.label.as_str()),
            (id.as_str(), "caldav", "anna@example.org")
        );
        assert!(!serde_json::to_string(&response).expect("json").contains(PASSWORD));
        assert_eq!(secrets.get_plain(&id).as_deref(), Some(PASSWORD), "the password is in the keychain");
        let listed = std::fs::read_to_string(dir.path().join(torro_core::account::ACCOUNTS_FILE)).expect("listed");
        assert!(listed.contains(&id) && !listed.contains(PASSWORD), "{listed}");

        service.sync_account(&id).await.expect("synced");
        let hidden = service.settings.current().hidden_collections;
        let tasks_list = collection_id(&id, &format!("{}calendars/anna/tasks/", base));
        let work = collection_id(&id, &format!("{}calendars/anna/work/", base));
        assert_eq!(hidden, [tasks_list], "tasks were not chosen");
        let state = service.state.read().await;
        let names: Vec<&str> = state.accounts[0].collections.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Work", "DEFAULT_TASK_CALENDAR_NAME"], "no Nextcloud placeholder names elsewhere");
        assert!(state.accounts[0].collections.iter().any(|c| c.id == work));
        assert!(!format!("{:?}", state.accounts).contains(PASSWORD), "no Debug output shows it");
        assert!(lines(&record).iter().all(|line| line.starts_with("PROPFIND") || line.starts_with("REPORT")));
    }

    #[tokio::test]
    async fn a_refused_password_is_auth_and_nothing_is_stored() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (base, _) = fake_server(|_| (401, None, "")).await;
        let (service, secrets) = caldav_service(dir.path());
        let response = service.handle_within(sign_in_at(Some(&base), PASSWORD, true, true), Reach::Suite).await;
        let Response::Error { code, message } = &response else { panic!("refused, got {response:?}") };
        assert_eq!((code.as_str(), message.as_str()), ("auth", tr("Password was refused")));
        assert!(!format!("{response:?}").contains(PASSWORD));
        let id = torro_core::Account::caldav(&base, "anna", "example.org").id;
        assert_eq!(secrets.get_plain(&id), None);
        assert!(!dir.path().join(torro_core::account::ACCOUNTS_FILE).exists());
        assert!(service.state.read().await.accounts.is_empty());
    }

    #[tokio::test]
    async fn signing_in_again_replaces_only_the_password() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (base, _) = fake_server(split_server).await;
        let (service, secrets) = caldav_service(dir.path());
        let first = service.handle_within(sign_in_at(Some(&base), PASSWORD, true, false), Reach::Suite).await;
        let Response::Connected { account, .. } = first else { panic!("connected, got {first:?}") };
        service.sync_account(&account.id).await.expect("synced");
        let hidden = service.settings.current().hidden_collections;
        assert_eq!(hidden.len(), 1);

        // "Change password…" chooses nothing: what the person hid stays hidden, nothing new hides.
        let again = service.handle_within(sign_in_at(Some(&base), "new-password", false, false), Reach::Suite).await;
        assert!(matches!(again, Response::Connected { .. }), "{again:?}");
        service.sync_account(&account.id).await.expect("synced");
        assert_eq!(service.settings.current().hidden_collections, hidden);
        assert_eq!(secrets.get_plain(&account.id).as_deref(), Some("new-password"));
        assert_eq!(service.state.read().await.accounts.len(), 1);
        let listed =
            torro_core::account::load_from(&dir.path().join(torro_core::account::ACCOUNTS_FILE)).expect("read");
        assert_eq!(listed.len(), 1);
    }

    #[tokio::test]
    async fn a_nextcloud_and_a_caldav_account_sync_apart_and_only_nextcloud_is_asked_to_wipe() {
        let (good, good_record) = fake_server(healthy_server).await;
        let (refusing, refusing_record) = fake_server(|_| (401, None, "")).await;
        // With an HTTP client, so a remote wipe check could be sent; only Nextcloud may get one.
        let service = Arc::new(PimService::with_settings(SettingsStore::without_file()));
        let mailbox = Connected {
            account: torro_core::Account::caldav(&refusing, "anna@example.org", "example.org"),
            app_password: Secret::new(PASSWORD),
        };
        let http = reqwest::Client::builder().no_proxy().build().expect("http client");
        let client = account::caldav(&http, &mailbox).expect("client");
        {
            let mut state = service.state.write().await;
            state.accounts.push(slot_on(&good));
            state.accounts.push(Slot::new(mailbox, Some(client)));
        }

        assert_eq!(service.sync().await, Err(ProviderError::Auth), "the failure of one account is reported");
        let state = service.state.read().await;
        let (nextcloud, caldav) = (&state.accounts[0], &state.accounts[1]);
        assert_eq!((nextcloud.problem.as_deref(), nextcloud.last_sync.is_some()), (None, true));
        assert_eq!(caldav.problem.as_deref(), Some(tr("Password was refused")));
        assert!(matches!(caldav.trouble, Some((Trouble::SignInRefused, _))));
        assert!(lines(&good_record).iter().any(|line| line.starts_with("REPORT")));
        assert_eq!(lines(&refusing_record), ["PROPFIND /remote.php/dav/"], "no wipe check, no revoke");
        drop(state);
        let kinds: Vec<String> = service.accounts_within(Reach::Suite).await.into_iter().map(|a| a.kind).collect();
        assert_eq!(kinds, ["nextcloud", "caldav"]);
    }

    #[tokio::test]
    async fn discovery_answers_from_the_table_and_refuses_what_is_no_domain() {
        let service = service();
        let ask = |address: &str| Request::DiscoverServices { address: address.into() };
        let gmail = service.handle_within(ask("anna@gmail.com"), Reach::Suite).await;
        let reason = tr(discover::Unsupported::Google.reason()).to_string();
        let calendars_only = |provider: &str, urls: Vec<String>, unsupported: Option<String>| Response::Discovery {
            provider: provider.into(),
            urls,
            unsupported,
            imap: None,
            smtp: None,
            login_hint: None,
            app_password: false,
            mail_unsupported: None,
        };
        assert_eq!(gmail, calendars_only("Gmail", vec![], Some(reason)));
        let mailbox = service.handle_within(ask("anna@mailbox.org"), Reach::Suite).await;
        assert_eq!(mailbox, calendars_only("mailbox.org", vec!["https://dav.mailbox.org/".into()], None));
        for refused in ["anna@192.168.1.1", "anna@localhost", ""] {
            assert_eq!(code_of(&service.handle_within(ask(refused), Reach::Suite).await), "error", "{refused:?}");
        }
    }

    /// Task M2.7b: a CalDAV address discovery found is signed in to only where its host resolves
    /// publicly, and then at exactly those addresses, under the host's name.
    #[tokio::test]
    async fn a_found_caldav_address_is_signed_in_to_at_its_checked_addresses_only() {
        let (base, record) = fake_server(split_server).await;
        let dav = CalDavAccount { url: base.replace("127.0.0.1", "localhost"), username: "anna".into() };
        let refused = pinned_public(&dav, Secret::new(PASSWORD), discover::is_public).await;
        assert!(matches!(refused, Err(ProviderError::Other(_))), "localhost is this computer");
        assert!(record.lock().expect("record").is_empty(), "nothing was asked");

        let client = pinned_public(&dav, Secret::new(PASSWORD), |ip| ip.is_loopback()).await.expect("a client");
        assert_eq!(client.services().await.expect("signed in"), Services { calendars: true, tasks: true });
        assert!(!record.lock().expect("record").is_empty(), "the server at the checked address was asked");
    }

    #[tokio::test]
    async fn a_sign_in_without_a_usable_address_or_login_stores_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (service, secrets) = caldav_service(dir.path());
        let ask = |request: Request| {
            let service = Arc::clone(&service);
            async move { service.handle_within(request, Reach::Suite).await }
        };
        let with = |address: &str, login_name: &str| Request::ConnectCalDav {
            address: address.into(),
            server: None,
            login_name: login_name.into(),
            password: Secret::new(PASSWORD),
            calendars: true,
            tasks: true,
        };
        assert_eq!(code_of(&ask(with("anna@gmail.com", "anna")).await), "unsupported");
        assert_eq!(code_of(&ask(sign_in_at(None, PASSWORD, true, true)).await), "not-found", "nothing found");
        let plain = sign_in_at(Some("http://dav.example.org/"), PASSWORD, true, true);
        assert_eq!(code_of(&ask(plain).await), "error", "no password over plain http");
        assert_eq!(code_of(&ask(with("anna@example.org", "a/b")).await), "error");
        assert_eq!(code_of(&ask(sign_in_at(Some("dav.example.org"), " ", true, true)).await), "error");
        let answer = ask(sign_in_at(Some("http://dav.example.org/"), PASSWORD, true, true)).await;
        assert!(!format!("{answer:?}").contains(PASSWORD));
        assert!(!dir.path().join(torro_core::account::ACCOUNTS_FILE).exists());
        assert!(
            service.state.read().await.accounts.is_empty()
                && secrets.get_plain("caldav:anna@dav.example.org").is_none()
        );
    }

    /// The mail account `anna@example.org` with the CalDAV address `dav` and `services` on.
    fn mail_with_calendars(dav: &str, services: torro_mail::Services) -> torro_core::Account {
        let server =
            |host: &str| torro_mail::Endpoint { host: host.into(), port: 993, security: torro_mail::Security::Tls };
        let config = torro_mail::MailAccountConfig {
            address: "anna@example.org".into(),
            sender_name: String::new(),
            imap: server("imap.example.org"),
            smtp: server("smtp.example.org"),
            caldav: Some(dav.into()),
        };
        let settings = torro_mail::MailSettings { services, ..torro_mail::MailSettings::default() };
        torro_core::Account::mail(config, "anna", "example.org", settings).expect("a valid address")
    }

    /// Task M2.7: the calendar part of a mail account takes the place of the CalDAV account of the
    /// same sign-in, syncs only the services switched on, has no check of its own, and leaves when
    /// they are switched off.
    #[tokio::test]
    async fn a_mail_account_syncs_only_its_services_in_place_of_the_caldav_account() {
        let (dav, record) = fake_server(split_server).await;
        let service = Arc::new(PimService::with_settings(SettingsStore::without_file()));
        let old = torro_core::Account::caldav(&dav, "anna", "example.org");
        service.pretend_connected(old.clone(), PASSWORD).await;
        service.state.write().await.loaded = true;

        let calendars_only = torro_mail::Services { mail: true, calendars: true, tasks: false };
        let mail = mail_with_calendars(&dav, calendars_only);
        assert_eq!(account::caldav_sign_in(&mail).as_deref(), Some(old.id.as_str()), "the same sign-in");
        assert!(account::syncs_calendars(&mail));
        let beside = service.put_mail_account(mail.clone(), Secret::new(PASSWORD), None).await;
        assert!(beside.is_err(), "never beside the CalDAV account of the same sign-in");
        service.put_mail_account(mail.clone(), Secret::new(PASSWORD), Some(&old.id)).await.expect("taken over");
        let ids: Vec<String> = service.state.read().await.accounts.iter().map(|slot| slot.id().to_string()).collect();
        assert_eq!(ids, [mail.id.as_str()], "never both");
        service.sync_account(&mail.id).await.expect("synced");
        let state = service.state.read().await;
        let names: Vec<&str> = state.accounts[0].collections.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Work"], "the task list stays out while tasks are off");
        assert_eq!(state.accounts[0].label(), "anna@example.org");
        drop(state);
        let reports: Vec<String> = lines(&record).into_iter().filter(|line| line.starts_with("REPORT")).collect();
        assert!(!reports.is_empty() && reports.iter().all(|line| line.contains("/work/")), "{reports:?}");

        assert_eq!(service.account_checks(Utc::now()).await, Some(vec![]), "its check is the mail account's");
        assert!(service.calendar_health(&mail.id, Utc::now()).await.is_some());

        let off = torro_mail::Services { mail: true, calendars: false, tasks: false };
        let mail = mail_with_calendars(&dav, off);
        assert!(!account::syncs_calendars(&mail));
        service.put_mail_account(mail.clone(), Secret::new(PASSWORD), None).await.expect("put");
        assert!(service.state.read().await.accounts.is_empty(), "calendars off: no slot, no cache");
    }

    /// Task M2.7b: the calendar part of a mail account resolves its server anew for every
    /// connection and goes nowhere when the name points into the local network (`localhost` here),
    /// unlike a Nextcloud or a typed CalDAV server, which may be at home.
    #[tokio::test]
    async fn the_calendars_of_a_mail_account_are_fetched_from_public_addresses_only() {
        let (dav, record) = fake_server(split_server).await;
        let service = Arc::new(PimService::with_settings(SettingsStore::without_file()));
        let on = torro_mail::Services { mail: true, calendars: true, tasks: true };
        let mail = mail_with_calendars(&dav.replace("127.0.0.1", "localhost"), on);
        service.put_mail_account(mail.clone(), Secret::new(PASSWORD), None).await.expect("put");
        assert!(service.sync_account(&mail.id).await.is_err(), "refused");
        assert!(record.lock().expect("record").is_empty(), "nothing was sent: {:?}", lines(&record));
        assert!(!format!("{:?}", service.state.read().await.accounts[0].problem).contains(PASSWORD));

        let caldav = torro_core::Account::caldav(&dav.replace("127.0.0.1", "localhost"), "anna", "example.org");
        service.pretend_connected(caldav.clone(), PASSWORD).await;
        service.sync_account(&caldav.id).await.expect("a CalDAV account may be at home");
    }

    #[test]
    fn a_collection_with_both_stays_while_either_service_is_chosen() {
        let both = Services { calendars: true, tasks: true };
        let calendars = Services { calendars: true, tasks: false };
        let neither = Services::default();
        assert!(chosen(calendars, CollectionKind::Both) && chosen(calendars, CollectionKind::Events));
        assert!(!chosen(calendars, CollectionKind::Todos));
        assert!(chosen(both, CollectionKind::Todos));
        assert!(!chosen(neither, CollectionKind::Both));
    }

    // Several accounts (task M1.1).

    #[tokio::test]
    async fn each_account_syncs_on_its_own_and_a_failing_one_keeps_its_cache() {
        let (good, good_record) = fake_server(healthy_server).await;
        let (bad, _) = fake_server(|_| (500, None, "")).await;
        let service = service();
        {
            let mut state = service.state.write().await;
            state.accounts.push(slot_on(&good));
            state.accounts.push(slot_on(&bad));
        }
        let before_bad = service.state.read().await.accounts[1].collections.clone();

        assert!(service.sync().await.is_err(), "the failure of one account is reported");
        let state = service.state.read().await;
        let (good_slot, bad_slot) = (&state.accounts[0], &state.accounts[1]);
        let (work, _) = on_server(&good, WORK, "");
        assert_eq!(good_slot.collections.iter().map(|c| c.id.clone()).collect::<Vec<_>>(), [work]);
        assert_eq!((good_slot.problem.as_deref(), good_slot.last_sync.is_some()), (None, true));
        assert_eq!(bad_slot.collections, before_bad, "the cache of the failing account stays");
        assert!(bad_slot.problem.is_some() && bad_slot.last_sync.is_none());
        assert!(lines(&good_record).iter().any(|line| line.starts_with("REPORT")), "{:?}", lines(&good_record));
        drop(state);

        let infos = service.accounts_within(Reach::Suite).await;
        assert!(matches!(&infos[0].state, AccountState::Connected { problem: None, .. }));
        assert!(matches!(&infos[1].state, AccountState::Connected { problem: Some(_), .. }));
        assert!(matches!(service.account_state().await, AccountState::Connected { problem: None, .. }));
    }

    #[tokio::test]
    async fn a_write_goes_to_the_account_that_holds_the_collection() {
        let (first, first_record) = fake_server(server_holding(SINGLE)).await;
        let (second, second_record) = fake_server(server_holding(SINGLE)).await;
        let service = service();
        {
            let mut state = service.state.write().await;
            state.accounts.push(slot_on(&first));
            state.accounts.push(slot_on(&second));
        }
        let (home, _) = on_server(&second, HOME, "");
        let new = Event { collection: home.clone(), href: None, etag: None, uid: "new".into(), ..event(Utc::now(), 1) };
        let response = service.handle_within(Request::SaveEvent { event: new }, Reach::Suite).await;
        let Response::Event { event: saved } = response else { panic!("saved, got {response:?}") };
        assert_eq!(saved.collection, home, "answered with the id, not the URL");
        assert!(lines(&first_record).is_empty(), "nothing reached the other account: {:?}", lines(&first_record));
        assert_eq!(lines(&second_record), [format!("PUT {HOME}new.ics If-None-Match *")]);
        assert_eq!(service.state.read().await.accounts[1].events, vec![saved.clone()], "cached at its account");

        let deleted = service.handle_within(Request::DeleteEvent { event: saved }, Reach::Suite).await;
        assert_eq!(deleted, Response::Ok);
        // The saved version is cached as a single event of that account, so no read first.
        assert_eq!(lines(&second_record)[1..], [format!("DELETE {HOME}new.ics If-Match \"2\"")]);
        assert!(lines(&first_record).is_empty());

        // Moving between accounts would read the old item with the new account's password.
        let listed = listed_event(&first, Some("\"1\""));
        let moved = Event { collection: home, ..listed };
        let response = service.handle_within(Request::SaveEvent { event: moved }, Reach::Suite).await;
        assert_eq!(code_of(&response), "unsupported", "{response:?}");
        assert!(lines(&first_record).is_empty(), "nothing sent: {:?}", lines(&first_record));
    }

    #[tokio::test]
    async fn a_client_sees_only_the_collections_of_its_selection_in_either_account() {
        use torro_ipc::api::{RightsTemplate, Selection};
        let (first, _) = fake_server(server_holding(SINGLE)).await;
        let (second, _) = fake_server(server_holding(SINGLE)).await;
        let service = service();
        {
            let mut state = service.state.write().await;
            state.accounts.push(slot_on(&first));
            state.accounts.push(slot_on(&second));
        }
        let (second_work, _) = on_server(&second, WORK, "");
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.calendars = Selection::Only(vec![second_work.clone()]);
        let client = Reach::Client(&rights);
        let Response::Collections { collections } = service.handle_within(Request::CalendarCollections, client).await
        else {
            panic!("collections")
        };
        assert_eq!(collections.into_iter().map(|c| c.id).collect::<Vec<_>>(), [second_work]);
        let (first_work, _) = on_server(&first, WORK, "");
        let save = Request::SaveEvent { event: Event { collection: first_work.clone(), ..event(Utc::now(), 1) } };
        assert_eq!(
            service.out_of_reach(client, &save).await,
            Some(first_work),
            "the same calendar of the other account"
        );
    }

    // Secrets stay with their server (task M1.4).

    #[tokio::test]
    async fn a_write_to_anna_with_the_url_of_bobs_server_reaches_no_server() {
        let (anna, anna_record) = fake_server(server_holding(SINGLE)).await;
        let (bob, bob_record) = fake_server(server_holding(SINGLE)).await;
        let service = service();
        {
            let mut state = service.state.write().await;
            state.accounts.push(slot_on(&anna));
            state.accounts.push(slot_on(&bob));
        }
        let anna_id = service.state.read().await.accounts[0].id().to_string();
        let (_, bob_href) = on_server(&bob, WORK, "e.ics");
        let bob_url = bob_href.trim_end_matches("e.ics").to_string();
        // `<anna>/<URL of bob's server>`: the front part names anna, the URL is bob's.
        let posing = collection_id(&anna_id, &bob_url);
        let new = Event { collection: posing.clone(), href: None, etag: None, ..event(Utc::now(), 1) };
        let changed = Event { collection: posing.clone(), href: Some(bob_href.clone()), ..event(Utc::now(), 1) };
        let task = Todo { etag: Some("\"1\"".into()), ..todo_in(&posing, Some(bob_href.clone())) };
        let (anna_work, _) = on_server(&anna, WORK, "");
        // Moving "within anna" from bob's server: the href is in no collection of hers.
        let moved = Event { collection: anna_work, href: Some(bob_href), ..event(Utc::now(), 1) };
        for request in [
            Request::SaveEvent { event: new },
            Request::SaveEvent { event: changed.clone() },
            Request::DeleteEvent { event: changed },
            Request::SaveTodo { todo: task.clone() },
            Request::DeleteTodo { todo: task },
        ] {
            let response = service.handle_within(request.clone(), Reach::Suite).await;
            assert_eq!(code_of(&response), "not-found", "{request:?}: {response:?}");
        }
        let response = service.handle_within(Request::SaveEvent { event: moved }, Reach::Suite).await;
        assert_ne!(code_of(&response), "ok", "{response:?}");
        assert!(lines(&anna_record).is_empty(), "nothing reached anna's server: {:?}", lines(&anna_record));
        assert!(lines(&bob_record).is_empty(), "nothing reached bob's server: {:?}", lines(&bob_record));
    }

    #[tokio::test]
    async fn the_caldav_client_sends_nothing_off_its_server() {
        let (anna, anna_record) = fake_server(server_holding(SINGLE)).await;
        let (bob, bob_record) = fake_server(server_holding(SINGLE)).await;
        let client = slot_on(&anna).client.expect("anna's client");
        let (_, bob_href) = on_server(&bob, WORK, "e.ics");
        let bob_url = bob_href.trim_end_matches("e.ics").to_string();
        let new = Event { collection: bob_url.clone(), href: None, etag: None, ..event(Utc::now(), 1) };
        let stored = Event { collection: bob_url.clone(), href: Some(bob_href.clone()), ..event(Utc::now(), 1) };
        assert!(client.save_event(new.clone()).await.is_err(), "PUT");
        assert!(client.save_event(stored.clone()).await.is_err(), "GET, then PUT");
        assert!(client.copy_event(&stored, new).await.is_err(), "GET of the old one");
        assert!(client.delete_event(&stored).await.is_err(), "DELETE");
        assert!(client.stored_item(&bob_url, &bob_href).await.is_err(), "GET");
        assert!(client.events(&bob_url, Utc::now(), Utc::now() + Span::days(1)).await.is_err(), "REPORT");
        assert!(client.todos(&bob_url).await.is_err(), "REPORT");
        assert!(lines(&bob_record).is_empty(), "nothing reached bob's server: {:?}", lines(&bob_record));
        assert!(lines(&anna_record).is_empty(), "nor anna's: {:?}", lines(&anna_record));
    }

    #[tokio::test]
    async fn a_calendar_listed_on_another_server_is_left_out() {
        const FOREIGN: &str = r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>https://evil.example.org/remote.php/dav/calendars/anna/work/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>Elsewhere</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
        let (base, record) = fake_server(|seen| match (seen.method.as_str(), seen.path.as_str()) {
            ("PROPFIND", "/remote.php/dav/calendars/anna/") => (207, None, FOREIGN),
            _ => healthy_server(seen),
        })
        .await;
        let client = slot_on(&base).client.expect("client");
        assert_eq!(client.collections().await, Ok(vec![]));
        assert_eq!(lines(&record).len(), 3, "discovery only: {:?}", lines(&record));
    }

    // Series and overwriting (task A.2b).

    #[tokio::test]
    async fn a_series_cannot_be_overwritten_by_claiming_it_is_a_single_event() {
        let (base, record) = fake_server(server_holding(SERIES)).await;
        let service = service_on(&base).await;
        let listed = listed_event(&base, Some("\"1\""));
        cache_occurrences(&service, &listed, true).await;
        let single = Event { recurring: false, summary: "Replaced".into(), ..listed.clone() };

        // The cache knows this version is a series: refused without asking the server.
        for request in [Request::SaveEvent { event: single.clone() }, Request::DeleteEvent { event: single.clone() }] {
            let response = service.handle_within(request, Reach::Suite).await;
            assert_eq!(code_of(&response), "unsupported", "{response:?}");
        }
        // The same with a relative href, which resolves to the same item.
        let relative = Event { href: Some(format!("{WORK}e.ics")), ..single.clone() };
        let response = service.handle_within(Request::SaveEvent { event: relative }, Reach::Suite).await;
        assert_eq!(code_of(&response), "unsupported", "{response:?}");
        assert!(lines(&record).is_empty(), "nothing reached the server: {:?}", lines(&record));

        // A version the cache does not hold: the server is asked, and nothing is written.
        let newer = Event { etag: Some("\"1b\"".into()), ..single };
        let response = service.handle_within(Request::SaveEvent { event: newer }, Reach::Suite).await;
        assert_eq!(code_of(&response), "unsupported", "{response:?}");
        assert_eq!(lines(&record), [format!("GET {WORK}e.ics")]);
    }

    #[tokio::test]
    async fn an_uncached_series_is_read_from_the_server() {
        let (base, record) = fake_server(server_holding(SERIES)).await;
        let service = service_on(&base).await;
        let event = listed_event(&base, Some("\"1\""));
        for request in [Request::SaveEvent { event: event.clone() }, Request::DeleteEvent { event }] {
            let response = service.handle_within(request, Reach::Suite).await;
            assert_eq!(code_of(&response), "unsupported", "{response:?}");
        }
        assert_eq!(lines(&record), [format!("GET {WORK}e.ics"), format!("GET {WORK}e.ics")], "only reads");
    }

    #[tokio::test]
    async fn a_recurring_task_is_neither_changed_nor_deleted() {
        let (base, record) = fake_server(server_holding(TASK_SERIES)).await;
        let service = service_on(&base).await;
        let todo = listed_todo(&base, Some("\"1\""));
        let done = Todo { completed: true, ..todo.clone() };
        for request in [Request::SaveTodo { todo: done }, Request::DeleteTodo { todo }] {
            let response = service.handle_within(request, Reach::Suite).await;
            assert!(
                matches!(&response, Response::Error { code, message } if code == "unsupported" && message.contains("recurring tasks")),
                "{response:?}"
            );
        }
        assert_eq!(lines(&record), [format!("GET {WORK}t.ics"), format!("GET {WORK}t.ics")], "only reads");
    }

    #[tokio::test]
    async fn changing_deleting_or_moving_without_etag_is_refused() {
        let (base, record) = fake_server(server_holding(SINGLE)).await;
        let service = service_on(&base).await;
        let event = listed_event(&base, None);
        let todo = listed_todo(&base, None);
        let (home, _) = on_server(&base, HOME, "");
        let moved_event = Event { collection: home.clone(), ..event.clone() };
        let moved_todo = Todo { collection: home, ..todo.clone() };
        let blank_etag = Event { etag: Some(String::new()), ..event.clone() };
        for request in [
            Request::SaveEvent { event: event.clone() },
            Request::DeleteEvent { event },
            Request::SaveEvent { event: moved_event },
            Request::SaveEvent { event: blank_etag },
            Request::SaveTodo { todo: todo.clone() },
            Request::DeleteTodo { todo },
            Request::SaveTodo { todo: moved_todo },
        ] {
            let response = service.handle_within(request.clone(), Reach::Suite).await;
            assert!(
                matches!(&response, Response::Error { code, message }
                    if code == "conflict" && message.contains("reload the item and pass its etag")),
                "{request:?}: {response:?}"
            );
        }
        assert!(lines(&record).is_empty(), "nothing reached the server: {:?}", lines(&record));
    }

    #[tokio::test]
    async fn a_normal_update_with_its_etag_is_written_with_if_match() {
        let (base, record) = fake_server(server_holding(SINGLE)).await;
        let service = service_on(&base).await;
        let listed = listed_event(&base, Some("\"1\""));
        cache_occurrences(&service, &listed, false).await;

        // The cache holds this version as a single event: no check; the stored object is read
        // only to write the change onto it (task P.1).
        let renamed = Event { summary: "Renamed".into(), ..listed.clone() };
        let response = service.handle_within(Request::SaveEvent { event: renamed }, Reach::Suite).await;
        let Response::Event { event: saved } = response else { panic!("expected the saved event, got {response:?}") };
        assert_eq!(saved.etag.as_deref(), Some("\"2\""));
        assert_eq!(saved.collection, listed.collection, "answered with the collection id");
        assert_eq!(lines(&record), [format!("GET {WORK}e.ics"), format!("PUT {WORK}e.ics If-Match \"1\"")]);

        // A task: the cache does not know series of tasks, so the server is read first.
        let todo = listed_todo(&base, Some("\"1\""));
        let done = Todo { completed: true, ..todo };
        let response = service.handle_within(Request::SaveTodo { todo: done }, Reach::Suite).await;
        assert!(matches!(response, Response::Todo { .. }), "{response:?}");
        // Deleting with the new etag, and creating, which must not overwrite anything.
        let response = service.handle_within(Request::DeleteEvent { event: saved }, Reach::Suite).await;
        assert_eq!(response, Response::Ok);
        let new = Event { href: None, etag: None, uid: "new".into(), ..listed };
        let response = service.handle_within(Request::SaveEvent { event: new }, Reach::Suite).await;
        assert!(matches!(response, Response::Event { .. }), "{response:?}");
        assert_eq!(
            lines(&record)[2..],
            [
                format!("GET {WORK}t.ics"),
                format!("GET {WORK}t.ics"),
                format!("PUT {WORK}t.ics If-Match \"1\""),
                // The saved version is cached as a single event, so no read before the delete.
                format!("DELETE {WORK}e.ics If-Match \"2\""),
                format!("PUT {WORK}new.ics If-None-Match *"),
            ]
        );
    }

    #[tokio::test]
    async fn moving_a_single_event_checks_it_and_then_moves_it() {
        let (base, record) = fake_server(server_holding(SINGLE)).await;
        let service = service_on(&base).await;
        let listed = listed_event(&base, Some("\"1\""));
        let (home, _) = on_server(&base, HOME, "");
        let moved = Event { collection: home.clone(), ..listed };
        let response = service.handle_within(Request::SaveEvent { event: moved }, Reach::Suite).await;
        let Response::Event { event: saved } = response else { panic!("moved, got {response:?}") };
        assert_eq!(saved.collection, home);
        assert_eq!(
            lines(&record),
            [
                format!("GET {WORK}e.ics"),
                // The new item is written onto the old one, so nothing the model lacks is lost.
                format!("GET {WORK}e.ics"),
                format!("PUT {HOME}u.ics If-None-Match *"),
                format!("DELETE {WORK}e.ics If-Match \"1\""),
            ]
        );
    }

    #[tokio::test]
    async fn moving_a_series_out_of_its_calendar_is_refused() {
        let (base, record) = fake_server(server_holding(SERIES)).await;
        let service = service_on(&base).await;
        let listed = listed_event(&base, Some("\"1\""));
        cache_occurrences(&service, &listed, true).await;
        let (home, _) = on_server(&base, HOME, "");
        let moved = Event { collection: home, recurring: false, ..listed };
        let response = service.handle_within(Request::SaveEvent { event: moved }, Reach::Suite).await;
        assert_eq!(code_of(&response), "unsupported", "{response:?}");
        assert!(lines(&record).is_empty(), "nothing created, nothing deleted: {:?}", lines(&record));
    }

    // Manual order (task P.3a).

    const TASK: &str =
        "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:t\r\nSUMMARY:Water plants\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    /// The task `uid` in `path` of the account at `base`, numbered `sort_order`, cached in
    /// version `"1"` as a sync leaves it.
    fn cached_task(base: &str, path: &str, uid: &str, sort_order: Option<i64>) -> Todo {
        let (collection, href) = on_server(base, path, &format!("{uid}.ics"));
        Todo { uid: uid.into(), etag: Some("\"1\"".into()), sort_order, ..todo_in(&collection, Some(href)) }
    }

    /// A service on the fake server at `base` whose cache holds `tasks`.
    async fn service_with_tasks(base: &str, tasks: Vec<Todo>) -> Arc<PimService> {
        let service = service_on(base).await;
        service.state.write().await.accounts[0].todos = tasks;
        service
    }

    fn move_request(task: &Todo, before: Option<&Todo>) -> Request {
        Request::MoveTodo {
            href: task.href.clone().expect("href"),
            etag: task.etag.clone().expect("etag"),
            before: before.and_then(|b| b.href.clone()),
        }
    }

    /// `(uid, sort_order)` of the cached tasks, in the order the cache holds them.
    async fn numbers(service: &PimService) -> Vec<(String, Option<i64>)> {
        let state = service.state.read().await;
        let mut numbers: Vec<_> = state.accounts[0].todos.iter().map(|t| (t.uid.clone(), t.sort_order)).collect();
        numbers.sort();
        numbers
    }

    #[tokio::test]
    async fn a_move_writes_only_the_moved_task_when_there_is_room() {
        let (base, record) = fake_server(server_holding(TASK)).await;
        let tasks: Vec<Todo> =
            [("a", 100), ("b", 200), ("c", 300)].iter().map(|(u, n)| cached_task(&base, WORK, u, Some(*n))).collect();
        let service = service_with_tasks(&base, tasks.clone()).await;

        let response = service.handle_within(move_request(&tasks[2], Some(&tasks[0])), Reach::Suite).await;
        let Response::Todos { todos } = response else { panic!("the written tasks, got {response:?}") };
        assert_eq!(todos.iter().map(|t| (t.uid.as_str(), t.sort_order)).collect::<Vec<_>>(), [("c", Some(49))]);
        assert_eq!(todos[0].etag.as_deref(), Some("\"2\""), "as stored");
        assert_eq!(todos[0].collection, tasks[2].collection, "answered with the list id");
        assert_eq!(
            lines(&record),
            [format!("GET {WORK}c.ics"), format!("GET {WORK}c.ics"), format!("PUT {WORK}c.ics If-Match \"1\"")]
        );
        let state = service.state.read().await;
        let cached = state.accounts[0].todos.iter().find(|t| t.uid == "c").expect("still cached");
        assert_eq!((cached.sort_order, cached.etag.as_deref()), (Some(49), Some("\"2\"")));
        drop(state);

        // Where it is already: nothing to write.
        let again = service.handle_within(move_request(&todos[0], Some(&tasks[0])), Reach::Suite).await;
        assert_eq!(again, Response::Todos { todos: vec![] });
        assert_eq!(lines(&record).len(), 3);
    }

    #[tokio::test]
    async fn moves_to_the_end_and_renumbering_write_with_the_etags() {
        let (base, record) = fake_server(server_holding(TASK)).await;
        let tasks: Vec<Todo> = [("p", 1), ("o", 2), ("n", 3), ("k", 10), ("z", 20)]
            .iter()
            .map(|(u, n)| cached_task(&base, WORK, u, Some(*n)))
            .collect();
        let service = service_with_tasks(&base, tasks.clone()).await;
        let response = service.handle_within(move_request(&tasks[4], Some(&tasks[1])), Reach::Suite).await;
        let Response::Todos { todos } = response else { panic!("the written tasks, got {response:?}") };
        let written: Vec<&str> = todos.iter().map(|t| t.uid.as_str()).collect();
        assert_eq!(written, ["n", "o", "z"], "from the end up, the moved task last");
        let puts: Vec<String> = lines(&record).into_iter().filter(|line| line.starts_with("PUT")).collect();
        assert_eq!(
            puts,
            [
                format!("PUT {WORK}n.ics If-Match \"1\""),
                format!("PUT {WORK}o.ics If-Match \"1\""),
                format!("PUT {WORK}z.ics If-Match \"1\""),
            ]
        );
        assert_eq!(
            numbers(&service).await,
            [
                ("k".into(), Some(10)),
                ("n".into(), Some(4)),
                ("o".into(), Some(3)),
                ("p".into(), Some(1)),
                ("z".into(), Some(2))
            ]
        );

        // p to the end: the last number plus the gap.
        let response = service.handle_within(move_request(&tasks[0], None), Reach::Suite).await;
        let Response::Todos { todos } = response else { panic!("the written tasks, got {response:?}") };
        assert_eq!(todos.iter().map(|t| (t.uid.as_str(), t.sort_order)).collect::<Vec<_>>(), [("p", Some(1034))]);
    }

    #[tokio::test]
    async fn a_failed_write_in_the_middle_stops_the_move_and_the_cache_shows_the_server() {
        let (base, record) = fake_server(|seen| match seen.method.as_str() {
            "PUT" if seen.path.ends_with("/o.ics") => (412, None, ""),
            _ => server_holding(TASK)(seen),
        })
        .await;
        let tasks: Vec<Todo> = [("p", 1), ("o", 2), ("n", 3), ("k", 10), ("z", 20)]
            .iter()
            .map(|(u, n)| cached_task(&base, WORK, u, Some(*n)))
            .collect();
        let service = service_with_tasks(&base, tasks.clone()).await;
        let response = service.handle_within(move_request(&tasks[4], Some(&tasks[1])), Reach::Suite).await;
        assert_eq!(code_of(&response), "conflict", "{response:?}");
        // n was written and is cached in its new version; o failed; z, the moved task, was not sent.
        assert!(!lines(&record).iter().any(|line| line.contains("z.ics")), "{:?}", lines(&record));
        assert_eq!(
            numbers(&service).await,
            [
                ("k".into(), Some(10)),
                ("n".into(), Some(4)),
                ("o".into(), Some(2)),
                ("p".into(), Some(1)),
                ("z".into(), Some(20))
            ]
        );
        let state = service.state.read().await;
        let etag = |uid: &str| state.accounts[0].todos.iter().find(|t| t.uid == uid).and_then(|t| t.etag.clone());
        assert_eq!((etag("n").as_deref(), etag("o").as_deref()), (Some("\"2\""), Some("\"1\"")));
    }

    #[tokio::test]
    async fn a_stale_etag_another_list_or_another_level_is_refused_before_anything_is_sent() {
        let (base, record) = fake_server(server_holding(TASK)).await;
        let a = cached_task(&base, WORK, "a", Some(1));
        let b = cached_task(&base, WORK, "b", Some(2));
        let elsewhere = cached_task(&base, HOME, "h", Some(1));
        let child = Todo { parent: Some("a".into()), ..cached_task(&base, WORK, "child", Some(3)) };
        let service = service_with_tasks(&base, vec![a.clone(), b.clone(), elsewhere.clone(), child.clone()]).await;
        let stale = Todo { etag: Some("\"0\"".into()), ..b.clone() };
        let unknown = Todo { href: Some(format!("{}x.ics", url("work"))), ..b.clone() };

        let ask = |request| {
            let service = Arc::clone(&service);
            async move { service.handle_within(request, Reach::Suite).await }
        };
        assert_eq!(code_of(&ask(move_request(&stale, Some(&a))).await), "conflict");
        assert_eq!(code_of(&ask(move_request(&b, Some(&elsewhere))).await), "not-found", "another list");
        assert_eq!(code_of(&ask(move_request(&unknown, Some(&a))).await), "not-found", "no such task");
        assert_eq!(code_of(&ask(move_request(&b, Some(&unknown))).await), "not-found", "no such before");
        assert_eq!(code_of(&ask(move_request(&b, Some(&child))).await), "error", "another parent");
        assert!(lines(&record).is_empty(), "nothing reached the server: {:?}", lines(&record));
    }

    #[tokio::test]
    async fn a_client_moves_only_within_the_lists_it_sees() {
        use torro_ipc::api::{RightsTemplate, Selection};
        let service = filled_service(SettingsStore::without_file()).await;
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.task_lists = Selection::Only(vec![cal("work")]);
        let client = Reach::Client(&rights);
        let (work, home) = (format!("{}t.ics", url("work")), format!("{}t.ics", url("home")));
        let request = |href: &str, before: Option<&str>| Request::MoveTodo {
            href: href.into(),
            etag: "\"1\"".into(),
            before: before.map(str::to_string),
        };
        assert_eq!(service.out_of_reach(client, &request(&work, None)).await, None);
        assert_eq!(service.out_of_reach(client, &request(&home, None)).await, Some(home.clone()));
        assert_eq!(service.out_of_reach(client, &request(&work, Some(&home))).await, Some(home.clone()));
        let unknown = format!("{}nope.ics", url("work"));
        assert_eq!(service.out_of_reach(client, &request(&unknown, None)).await, Some(unknown.clone()));
        assert_eq!(service.out_of_reach(Reach::Suite, &request(&home, None)).await, None);
    }

    #[tokio::test]
    async fn a_changed_or_vanished_item_is_not_written() {
        let (base, record) = fake_server(|seen| match seen.method.as_str() {
            "GET" if seen.path.ends_with("gone.ics") => (404, None, ""),
            "GET" => (200, Some("\"3\""), SINGLE),
            _ => (500, None, ""),
        })
        .await;
        let service = service_on(&base).await;
        let stale = listed_event(&base, Some("\"1\""));
        let response = service.handle_within(Request::SaveEvent { event: stale.clone() }, Reach::Suite).await;
        assert_eq!(code_of(&response), "conflict", "{response:?}");
        let gone = Event { href: stale.href.map(|href| href.replace("e.ics", "gone.ics")), ..stale };
        let response = service.handle_within(Request::DeleteEvent { event: gone }, Reach::Suite).await;
        assert_eq!(code_of(&response), "not-found", "{response:?}");
        assert_eq!(lines(&record), [format!("GET {WORK}e.ics"), format!("GET {WORK}gone.ics")], "only reads");
    }

    #[tokio::test]
    async fn account_checks_tell_the_inbox_which_accounts_need_the_user() {
        let service = service();
        let t0 = Utc::now();
        assert_eq!(service.account_checks(t0).await, None, "nothing counts before the accounts are read");

        let bob = torro_core::Account::nextcloud("https://other.example.org", "bob");
        let unusable = Stored::Unusable { account: bob, problem: "Sign in again".into() };
        {
            let mut state = service.state.write().await;
            state.loaded = true;
            state.accounts.push(slot_with(Vec::new()));
            state.accounts.push(Slot::loaded(unusable, None));
        }
        let health = |checks: Option<Vec<AccountCheck>>| -> Vec<Health> {
            checks.expect("loaded").into_iter().map(|check| check.health).collect()
        };
        let checks = service.account_checks(t0).await.expect("loaded");
        assert_eq!(checks[0].label, "anna@cloud.example.org");
        assert_eq!(checks[0].health, Health::Unsure, "not synced since the core started");
        assert!(matches!(checks[1].health, Health::Problem { trouble: Trouble::SignInNeeded, .. }));

        // Synced once, then offline: unsure for 30 minutes, a problem since the first failure after.
        let offline = ProviderError::Offline;
        {
            let mut state = service.state.write().await;
            state.accounts[0].last_sync = Some(t0);
            state.accounts.truncate(1);
        }
        assert_eq!(health(service.account_checks(t0).await), [Health::Fine]);
        let first = t0 + Span::minutes(5);
        service.state.write().await.accounts[0].trouble = Some(failing(None, &offline, first));
        let again = failing(Some((Trouble::SyncFailing, first)), &offline, first + Span::minutes(5));
        assert_eq!(again, (Trouble::SyncFailing, first), "the same trouble keeps its start");
        assert_eq!(health(service.account_checks(first + SYNC_PROBLEM_AFTER).await), [Health::Unsure]);
        let late = first + SYNC_PROBLEM_AFTER + Span::minutes(1);
        assert_eq!(
            health(service.account_checks(late).await),
            [Health::Problem { trouble: Trouble::SyncFailing, since: first }]
        );

        // A refused app password is a problem at once, from the refusal on.
        let refused = failing(Some((Trouble::SyncFailing, first)), &ProviderError::Auth, late);
        assert_eq!(refused, (Trouble::SignInRefused, late));
        service.state.write().await.accounts[0].trouble = Some(refused);
        assert_eq!(
            health(service.account_checks(late).await),
            [Health::Problem { trouble: Trouble::SignInRefused, since: late }]
        );
    }

    // New task lists (task G.7).

    /// What Nextcloud (Sabre) answers an `MKCALENDAR` at an address that is taken, with `405`.
    const TAKEN: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:error xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns">
  <s:sabredav-version>4.7.0</s:sabredav-version>
  <s:exception>Sabre\DAV\Exception\MethodNotAllowed</s:exception>
  <s:message>The resource you tried to create already exists</s:message>
</d:error>"#;

    /// A Nextcloud with anna's calendar home ([`healthy_server`]) whose first `taken` `MKCALENDAR`s
    /// find their address taken; the others are answered with `then`.
    fn making_lists(taken: usize, then: u16) -> impl Fn(&Seen) -> Reply + Send + Sync + 'static {
        let tried = std::sync::atomic::AtomicUsize::new(0);
        move |seen| match seen.method.as_str() {
            "MKCALENDAR" if tried.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < taken => (405, None, TAKEN),
            "MKCALENDAR" => (then, None, ""),
            _ => healthy_server(seen),
        }
    }

    /// The `MKCALENDAR`s the fake server saw: path and body.
    fn mkcalendars(record: &std::sync::Mutex<Vec<Seen>>) -> Vec<(String, String)> {
        let seen = record.lock().expect("record");
        seen.iter()
            .filter(|seen| seen.method == "MKCALENDAR")
            .map(|seen| (seen.path.clone(), seen.body.clone()))
            .collect()
    }

    /// `path` is a new list's address in anna's calendar home: `<slug>-<8 hex digits>/`.
    fn new_list_address(path: &str, slug: &str) -> bool {
        let Some(rest) = path.strip_prefix(&format!("/remote.php/dav/calendars/anna/{slug}-")) else { return false };
        let Some(random) = rest.strip_suffix('/') else { return false };
        random.len() == 8 && random.chars().all(|c| c.is_ascii_hexdigit())
    }

    #[tokio::test]
    async fn a_new_task_list_is_made_on_the_server_and_listed_at_once() {
        let (base, record) = fake_server(making_lists(0, 201)).await;
        let service = service_on(&base).await;
        let account = account_on(&base);

        let response = service.create_task_list(&account, "  Groceries & more ", Some("#3FB984")).await;
        let Response::Collection { collection } = response else { panic!("a new list, got {response:?}") };
        assert_eq!(
            lines(&record)[..2],
            ["PROPFIND /remote.php/dav/".to_string(), "PROPFIND /remote.php/dav/principals/users/anna/".to_string()],
            "the calendar home as the server names it"
        );
        let made = mkcalendars(&record);
        assert_eq!(made.len(), 1);
        let (path, body) = &made[0];
        assert!(new_list_address(path, "groceries-more"), "{path}");
        for part in ["<d:displayname>Groceries &amp; more</d:displayname>", "#3FB984FF", r#"<c:comp name="VTODO"/>"#] {
            assert!(body.contains(part), "{part} in {body}");
        }

        let origin = base.trim_end_matches("/remote.php/dav/");
        let expected = Collection {
            id: collection_id(&account, &format!("{origin}{path}")),
            name: "Groceries & more".into(),
            kind: torro_pim::CollectionKind::Todos,
            color: Some("#3FB984".into()),
            writable: true,
            source: None,
        };
        assert_eq!(collection, expected);
        let Response::Collections { collections: listed } =
            service.handle_within(Request::TaskCollections, Reach::Suite).await
        else {
            panic!("task lists answer with Collections");
        };
        assert!(listed.contains(&expected), "listed before the next sync: {listed:?}");
        assert!(
            service.state.read().await.accounts[0].private.contains(&expected.id),
            "anna's own, shared with nobody"
        );

        let Response::Collection { collection: again } =
            service.create_task_list(&account, "Groceries & more", None).await
        else {
            panic!("a second list of the same name");
        };
        assert_ne!(again.id, expected.id, "a new address every time");
        assert_eq!(again.color, None);
        assert!(!mkcalendars(&record)[1].1.contains("calendar-color"), "the server's default colour");
    }

    #[tokio::test]
    async fn a_taken_address_is_never_overwritten_but_another_one_tried() {
        let (base, record) = fake_server(making_lists(2, 201)).await;
        let service = service_on(&base).await;
        let response = service.create_task_list(&account_on(&base), "Groceries", None).await;
        let Response::Collection { collection } = response else { panic!("made at the third address: {response:?}") };
        let paths: Vec<String> = mkcalendars(&record).into_iter().map(|(path, _)| path).collect();
        assert_eq!(paths.len(), 3, "{paths:?}");
        assert!(paths.iter().all(|path| new_list_address(path, "groceries")), "{paths:?}");
        assert!(paths[0] != paths[1] && paths[1] != paths[2] && paths[0] != paths[2], "{paths:?}");
        assert!(collection.id.ends_with(&paths[2]), "{} at {}", collection.id, paths[2]);
        assert!(lines(&record).iter().all(|line| !line.starts_with("PUT") && !line.starts_with("DELETE")));

        // Every address taken: an error after a few tries, and nothing cached.
        let (base, record) = fake_server(making_lists(usize::MAX, 201)).await;
        let service = service_on(&base).await;
        let before = cached(&service).await;
        let response = service.create_task_list(&account_on(&base), "Groceries", None).await;
        assert_eq!(code_of(&response), "error", "{response:?}");
        assert_eq!(mkcalendars(&record).len(), LIST_ATTEMPTS);
        assert_eq!(cached(&service).await, before);
    }

    #[tokio::test]
    async fn a_new_task_list_is_checked_before_the_server_is_asked() {
        let (base, record) = fake_server(making_lists(0, 201)).await;
        let service = service_on(&base).await;
        let account = account_on(&base);
        let long = "x".repeat(MAX_LIST_NAME + 1);
        for (name, color, code) in [
            ("", None, "error"),
            ("   ", None, "error"),
            (long.as_str(), None, "error"),
            ("Groceries", Some("green"), "error"),
            ("Groceries", Some("#3FB98"), "error"),
        ] {
            let response = service.create_task_list(&account, name, color).await;
            assert_eq!(code_of(&response), code, "{name:?} {color:?}: {response:?}");
        }
        let unknown = service.create_task_list("nextcloud:bob@cloud.example.org", "Groceries", None).await;
        assert_eq!(code_of(&unknown), "not-found");
        assert!(lines(&record).is_empty(), "nothing reached the server: {:?}", lines(&record));

        // An account without its app password on this device.
        let without = self::service();
        without.pretend_signed_in().await;
        assert_eq!(code_of(&without.create_task_list(ANNA, "Groceries", None).await), "not-connected");
    }

    #[tokio::test]
    async fn a_refused_new_task_list_is_an_error_with_the_servers_reason() {
        let (base, _) = fake_server(making_lists(0, 507)).await;
        let service = service_on(&base).await;
        let full = service.create_task_list(&account_on(&base), "Groceries", None).await;
        assert_eq!(code_of(&full), "error", "{full:?}");

        let (base, _) = fake_server(making_lists(0, 401)).await;
        let service = service_on(&base).await;
        // The remote wipe check ran already for this refusal, so no further request goes out.
        service.state.write().await.accounts[0].wipe_checked = true;
        let refused = service.create_task_list(&account_on(&base), "Groceries", None).await;
        assert_eq!(code_of(&refused), "auth", "{refused:?}");

        // A server that is not there.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let gone = format!("http://{}/remote.php/dav/", listener.local_addr().expect("address"));
        drop(listener);
        let service = service_on(&gone).await;
        let offline = service.create_task_list(&account_on(&gone), "Groceries", None).await;
        assert_eq!(code_of(&offline), "offline", "{offline:?}");
        let collections = cached(&service).await;
        assert_eq!(collections.len(), 2, "work and home only: {collections:?}");
    }

    /// Whether the fake server was asked for the remote wipe check.
    fn wipe_checked_on(record: &std::sync::Mutex<Vec<Seen>>) -> bool {
        lines(record).iter().any(|line| line.contains("/core/wipe/check"))
    }

    #[tokio::test]
    async fn a_forbidden_new_task_list_is_no_sign_in_problem() {
        // With a network client, so a refused app password would ask the server about a wipe.
        let with_network = |base: &str| {
            let http = reqwest::Client::builder().no_proxy().build().expect("http client");
            let service =
                Arc::new(PimService { http: Some(http), ..PimService::with_settings(SettingsStore::without_file()) });
            let slot = slot_on(base);
            (service, slot)
        };

        // 403: the app password works, the place takes no new list (task G.9b).
        let (base, record) = fake_server(making_lists(0, 403)).await;
        let (service, slot) = with_network(&base);
        service.state.write().await.accounts.push(slot);
        let refused = service.create_task_list(&account_on(&base), "Groceries", None).await;
        assert_eq!(
            refused,
            error("error", torro_pim_caldav::write::NO_NEW_LISTS_HERE.into()),
            "no `auth`, so no \"sign in again\""
        );
        assert!(!wipe_checked_on(&record), "no remote wipe check: {:?}", lines(&record));
        assert!(!service.state.read().await.accounts[0].wipe_checked);
        assert_eq!(service.accounts_within(Reach::Suite).await.len(), 1, "the account stays");

        // 401 refuses the app password: that one asks.
        let (base, record) = fake_server(making_lists(0, 401)).await;
        let (service, slot) = with_network(&base);
        service.state.write().await.accounts.push(slot);
        let refused = service.create_task_list(&account_on(&base), "Groceries", None).await;
        assert_eq!(code_of(&refused), "auth", "{refused:?}");
        assert!(wipe_checked_on(&record), "the remote wipe check ran: {:?}", lines(&record));
    }

    // What the filter list dialog reads of GitHub (task G.9b).

    const GITHUB_ACCOUNT: &str = "github:mahype";

    fn repo_list(name: &str) -> Vec<GitHubRepo> {
        vec![GitHubRepo { repo: name.into(), private: false }]
    }

    /// Reads the repositories of [`GITHUB_ACCOUNT`] through the cache, with `fetch` as GitHub.
    async fn read_repos<Fut>(service: &PimService, fetch: impl FnOnce() -> Fut) -> Vec<GitHubRepo>
    where
        Fut: Future<Output = Result<Vec<GitHubRepo>, torro_pim_github::Error>>,
    {
        service
            .read_through(GITHUB_ACCOUNT, |slot| slot.repos.clone(), fetch, |slot, repos| slot.repos = Some(repos))
            .await
            .expect("repositories")
    }

    /// The repositories the cache holds for [`GITHUB_ACCOUNT`]; `None` without a slot too.
    async fn held_repos(service: &PimService) -> Option<Vec<GitHubRepo>> {
        let state = service.state.read().await;
        state.filters.iter().find(|slot| slot.account == GITHUB_ACCOUNT).and_then(|slot| slot.repos.clone())
    }

    #[tokio::test]
    async fn a_github_answer_from_before_a_refresh_is_not_kept() {
        let service = service();
        service.refresh_filter_lists(GITHUB_ACCOUNT, None).await;

        let old = read_repos(&service, || async {
            // The filter lists are refreshed while GitHub answers.
            service.refresh_filter_lists(GITHUB_ACCOUNT, None).await;
            Ok(repo_list("mahype/old"))
        })
        .await;
        assert_eq!(old, repo_list("mahype/old"), "the asker still gets its answer");
        assert_eq!(held_repos(&service).await, None, "older than the refresh, so not kept");

        let new = read_repos(&service, || async { Ok(repo_list("mahype/new")) }).await;
        assert_eq!(new, repo_list("mahype/new"));
        assert_eq!(held_repos(&service).await, Some(repo_list("mahype/new")), "kept when nothing came between");
        let held = read_repos(&service, || async { panic!("GitHub is not asked again") }).await;
        assert_eq!(held, repo_list("mahype/new"));
    }

    #[tokio::test]
    async fn a_github_answer_for_a_removed_account_leaves_no_slot_behind() {
        let service = service();
        service.refresh_filter_lists(GITHUB_ACCOUNT, None).await;
        read_repos(&service, || async {
            service.forget_filter_lists(GITHUB_ACCOUNT).await;
            Ok(repo_list("mahype/old"))
        })
        .await;
        assert!(!service.has_filter_slot(GITHUB_ACCOUNT).await, "no slot made again for a removed account");

        // Removed and connected again meanwhile: a new slot, which the old answer does not reach.
        service.refresh_filter_lists(GITHUB_ACCOUNT, None).await;
        read_repos(&service, || async {
            service.forget_filter_lists(GITHUB_ACCOUNT).await;
            service.refresh_filter_lists(GITHUB_ACCOUNT, None).await;
            Ok(repo_list("mahype/old"))
        })
        .await;
        assert_eq!(held_repos(&service).await, None);

        // Before the account's first refresh there is no slot: answered, nothing kept.
        let service = self::service();
        assert_eq!(read_repos(&service, || async { Ok(repo_list("mahype/x")) }).await, repo_list("mahype/x"));
        assert!(!service.has_filter_slot(GITHUB_ACCOUNT).await);
    }

    #[test]
    fn at_most_so_many_repositories_are_described_the_oldest_go_first() {
        let mut state = State::default();
        let slot = state.filter_slot_mut(GITHUB_ACCOUNT);
        let details = |n: usize| GitHubRepoDetails { repo: format!("o/r{n}"), ..GitHubRepoDetails::default() };
        for n in 0..=MAX_DESCRIBED {
            slot.describe(format!("o/r{n}"), details(n));
        }
        assert_eq!(slot.described.len(), MAX_DESCRIBED);
        assert_eq!(slot.described("o/r0"), None, "the oldest went");
        assert_eq!(slot.described("o/r1"), Some(details(1)));
        assert_eq!(slot.described(&format!("o/r{MAX_DESCRIBED}")), Some(details(MAX_DESCRIBED)));

        // Described again: newest, and still once.
        slot.describe("o/r1".into(), details(1));
        slot.describe("o/new".into(), details(0));
        assert_eq!(slot.described.len(), MAX_DESCRIBED);
        assert_eq!(slot.described("o/r1"), Some(details(1)), "kept as the newest");
        assert_eq!(slot.described("o/r2"), None);
    }
}
