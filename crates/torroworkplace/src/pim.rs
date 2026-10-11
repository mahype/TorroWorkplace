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

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Duration as Span, Local, Utc};
use tokio::sync::{Mutex, RwLock};
use torro_core::{AppId, paths::AppPaths};
use torro_ipc::api::Rights;
use torro_ipc::{AccountInfo, Request, Response};
use torro_pim::account::{collection_id, local_id};
use torro_pim::source::move_plan;
use torro_pim::{AccountState, Collection, Event, EventTime, PimAccount, ProviderError, Todo, combined_state};
use torro_pim_caldav::CalDavClient;
use torro_pim_caldav::nextcloud;
use torro_pim_caldav::write::item_url;

use torro_core::i18n::{tr, trf};

use crate::account::{self, Connected, Stored};
use crate::notify;
use crate::settings_service::SettingsStore;

/// How often the cache is refreshed in the background.
const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);
/// The cached event window around now.
const WINDOW_BACK_DAYS: i64 = 90;
const WINDOW_AHEAD_DAYS: i64 = 365;

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
    /// With ids `<account-id>/<url>`; the events and todos name them the same way.
    collections: Vec<Collection>,
    events: Vec<Event>,
    /// The range `events` covers; `None` before the first successful sync.
    window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    todos: Vec<Todo>,
    /// The remote wipe check already ran since the server started refusing the app password.
    /// Nextcloud rate-limits the check, and asking again on every failed request would not
    /// change the answer; a successful sync or a new sign-in clears it.
    wipe_checked: bool,
}

impl Slot {
    fn new(connected: Connected, client: Option<CalDavClient>) -> Slot {
        Slot {
            connected,
            client,
            last_sync: None,
            problem: None,
            collections: Vec::new(),
            events: Vec::new(),
            window: None,
            todos: Vec::new(),
            wipe_checked: false,
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
                Slot { problem: Some(problem), ..Slot::new(Connected { account, app_password: String::new() }, None) }
            }
        }
    }

    fn id(&self) -> &str {
        &self.connected.account.id
    }

    /// `anna@cloud.example.org`: the name the user knows the account by.
    fn label(&self) -> String {
        let account = &self.connected.account;
        format!("{}@{}", account.login_name, host(&account.server))
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

    /// The account with all its cached collections, hidden ones included.
    fn info(&self) -> AccountInfo {
        let account = &self.connected.account;
        AccountInfo {
            id: account.id.clone(),
            kind: "nextcloud".into(),
            label: self.label(),
            state: self.state(),
            collections: self.collections.clone(),
            scopes: None,
        }
    }
}

#[derive(Debug, Default)]
struct State {
    /// In the order of `accounts.json`; a new account comes last.
    accounts: Vec<Slot>,
    /// Server and login URL while Login Flow v2 runs (one at a time).
    signing_in: Option<(String, String)>,
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

    /// The cached collections of all accounts.
    fn collections(&self) -> impl Iterator<Item = &Collection> {
        self.accounts.iter().flat_map(|slot| &slot.collections)
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
    /// `collection` serves as `role`, is not `hidden` and is in the selection.
    fn sees(&self, collection: &Collection, role: Role, hidden: &[String]) -> bool {
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
}

/// See the module docs.
#[derive(Debug)]
pub struct PimService {
    http: Option<reqwest::Client>,
    state: RwLock<State>,
    /// One sync of an account at a time.
    syncing: Mutex<()>,
    /// Where the hidden collections are read from, and cleaned up when an account goes.
    settings: SettingsStore,
}

impl PimService {
    /// The service on the user's settings ([`SettingsStore::system`]), as the daemon runs it.
    /// Does not touch the network; [`PimService::start`] loads the accounts.
    pub fn new() -> PimService {
        PimService::with_settings(SettingsStore::system())
    }

    /// The service on `settings`; tests pass a store in a temp directory. Does not touch the
    /// network.
    pub fn with_settings(settings: SettingsStore) -> PimService {
        let http = match torro_pim_caldav::http_client() {
            Ok(http) => Some(http),
            Err(err) => {
                eprintln!("torroworkplace: no HTTP client: {err}");
                None
            }
        };
        PimService { http, state: RwLock::new(State::default()), syncing: Mutex::new(()), settings }
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
                        state.accounts.push(Slot::loaded(stored, service.http.as_ref()));
                    }
                }
                Err(err) => eprintln!("torroworkplace: cannot read the accounts: {err}"),
            }
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
            Request::Todos => {
                let lists = ids(self.seen(reach, Role::TaskList).await);
                let state = self.state.read().await;
                let todos = state
                    .accounts
                    .iter()
                    .flat_map(|slot| &slot.todos)
                    .filter(|t| lists.contains(&t.collection))
                    .cloned()
                    .collect();
                Response::Todos { todos }
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

    /// The cached collections of all accounts that `reach` sees as `role`.
    async fn seen(&self, reach: Reach<'_>, role: Role) -> Vec<Collection> {
        let hidden = self.hidden_collections();
        let state = self.state.read().await;
        state.collections().filter(|c| reach.sees(c, role, &hidden)).cloned().collect()
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
            Request::SaveTodo { todo } | Request::DeleteTodo { todo } => {
                (Role::TaskList, &todo.collection, todo.href.as_deref())
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
            | Request::ListClients
            | Request::CreateClient { .. }
            | Request::UpdateClient { .. }
            | Request::NewClientKey { .. }
            | Request::RemoveClient { .. } => return None,
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

    /// Stores the account [`tests::ANNA`] as a sign-in would, without network or keychain, for
    /// the core's handler tests. Does nothing when it is there already.
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

    /// Removes the account `id` and nothing else: revokes its app password on the server (best
    /// effort), forgets the account and its secret, drops its cache (task S.2) and takes its
    /// collections out of the hidden ones (task A.0d). Answered with [`Response::Ok`]; an
    /// unknown id is `not-found` (task M1.4), a keychain or file that cannot be changed `error`.
    /// An account without its app password is removed without asking its server.
    pub async fn remove_account(&self, id: &str) -> Response {
        let found = self.state.read().await.slot(id).map(|slot| (slot.connected.clone(), slot.client.is_some()));
        let Some((connected, reachable)) = found else {
            return error("not-found", format!("no account {id:?}"));
        };
        // Revoke first: afterwards the secret is gone and the device would stay valid in
        // the user's "Devices & sessions" for good. Offline or refused only costs a log line.
        match &self.http {
            _ if !reachable => {}
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
    /// "sign-in refused" until the user signs in again.
    async fn on_refused(&self, id: &str) {
        let Some(http) = self.http.clone() else {
            return;
        };
        let Some((connected, label)) =
            self.state.read().await.slot(id).map(|slot| (slot.connected.clone(), slot.label()))
        else {
            return;
        };
        let (server, token) = (connected.account.server.as_str(), connected.app_password.as_str());
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
            .map(|info| PimAccount { id: info.id, label: info.label, kind: info.kind, state: info.state })
            .collect();
        combined_state(&accounts)
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
        let client = self.state.read().await.slot(id).and_then(|slot| slot.client.clone()).ok_or_else(not_connected)?;
        let result = fetch_all(&client, id).await;
        let mut state = self.state.write().await;
        let Some(slot) = state.slot_mut(id) else {
            // The account was removed while the sync ran; nothing of it is kept.
            return Err(not_connected());
        };
        match result {
            Ok(Fetched { collections, events, window, todos }) => {
                slot.collections = collections;
                slot.events = events;
                slot.window = Some(window);
                slot.todos = todos;
                slot.last_sync = Some(Utc::now());
                slot.problem = None;
                slot.wipe_checked = false;
                Ok(())
            }
            Err(err) => {
                eprintln!("torroworkplace: sync of {id} failed: {err}");
                slot.problem = Some(err.to_string());
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
    events: Vec<Event>,
    window: (DateTime<Utc>, DateTime<Utc>),
    todos: Vec<Todo>,
}

/// Everything the cache of `account` holds, fetched from its server; the server's URLs become
/// collection ids with the account in front.
async fn fetch_all(client: &CalDavClient, account: &str) -> Result<Fetched, ProviderError> {
    let today = Local::now().date_naive();
    let start = today - Span::days(WINDOW_BACK_DAYS);
    let end = today + Span::days(WINDOW_AHEAD_DAYS);
    let window = (start.and_time(chrono::NaiveTime::MIN).and_utc(), end.and_time(chrono::NaiveTime::MIN).and_utc());
    let mut collections = client.collections().await?;
    let mut events = Vec::new();
    let mut todos = Vec::new();
    for collection in &mut collections {
        if let Some(name) = placeholder_name(&collection.name) {
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
    Ok(Fetched { collections, events, window, todos })
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
            app_password: "secret".into(),
        }
    }

    /// A service without HTTP client and without a settings file, so no test reaches the network
    /// or the user's settings.
    fn service() -> Arc<PimService> {
        service_with(SettingsStore::without_file())
    }

    fn service_with(settings: SettingsStore) -> Arc<PimService> {
        Arc::new(PimService { http: None, state: RwLock::new(State::default()), syncing: Mutex::new(()), settings })
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
        let infos = account_infos(&State { accounts: vec![slot], signing_in: None });
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
            app_password: "other".into(),
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
            app_password: "other".into(),
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
    struct Seen {
        method: String,
        path: String,
        if_match: Option<String>,
        if_none_match: Option<String>,
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
    type Reply = (u16, Option<&'static str>, &'static str);

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
    fn healthy_server(seen: &Seen) -> Reply {
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
    async fn fake_server(
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
                    let seen = Seen {
                        method: first.next().unwrap_or_default().to_string(),
                        path: first.next().unwrap_or_default().to_string(),
                        if_match: header("if-match"),
                        if_none_match: header("if-none-match"),
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
    fn lines(record: &std::sync::Mutex<Vec<Seen>>) -> Vec<String> {
        record.lock().expect("record").iter().map(Seen::line).collect()
    }

    const WORK: &str = "/remote.php/dav/calendars/anna/work/";
    const HOME: &str = "/remote.php/dav/calendars/anna/home/";

    /// Anna's account on the fake server at `base` (its id names the server's port, so two fake
    /// servers are two accounts), with the collections `work` and `home` and an empty cache.
    fn slot_on(base: &str) -> Slot {
        let origin = base.trim_end_matches("/remote.php/dav/");
        let connected =
            Connected { account: torro_core::Account::nextcloud(origin, "anna"), app_password: "secret".into() };
        let http = reqwest::Client::builder().no_proxy().build().expect("http client");
        let dav = torro_pim_caldav::CalDavAccount { url: base.into(), username: "anna".into() };
        let client = CalDavClient::new(http, &dav, "secret".into()).expect("client");
        let id = connected.account.id.clone();
        let collections = [WORK, HOME]
            .iter()
            .map(|path| of_kind(&collection_id(&id, &format!("{origin}{path}")), torro_pim::CollectionKind::Both))
            .collect();
        Slot { collections, ..Slot::new(connected, Some(client)) }
    }

    /// A service connected to the fake server at `base`.
    async fn service_on(base: &str) -> Arc<PimService> {
        let service = service();
        service.state.write().await.accounts.push(slot_on(base));
        service
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
}
