//! The commands of the core, shared by every way in: the socket (`ctl`, hotkeys, the control
//! centre, the apps, scripts and agents) and the tray menu. Each [`Request`] gets one
//! [`Response`]; the logic exists only here. Requests from the socket pass the key and rights
//! check first ([`Core::handle_from`]); the tray menu runs inside the core and needs none.

use std::sync::Arc;

use chrono::Local;
use tokio::sync::Mutex;
use torro_core::AppId;
use torro_core::settings::ListOrderEntry;
use torro_ipc::{API_VERSION, AccountInfo, IssuedKey, Key, Request, Response, SettingKey, Status};
use torro_pim::AccountState;
use torro_setups::capture::{auto_name, capture};
use torro_setups::launch::{Launcher, SystemLauncher, command_for};
use torro_setups::restore::{RestoreOptions, restore};
use torro_setups::storage::Store;
use torro_setups::{SetupsError, TemplateSummary};
use torro_wm::{Backend, Events, WindowManager, WmError};

use crate::account::github::{GithubAccounts, GithubError};
use crate::clients::{Caller, Clients, ClientsError, Refused};
use crate::pim::{PimService, Reach};
use crate::settings_service::SettingsStore;

/// `name` next to the daemon's binary, else from `PATH`.
fn sibling(name: &str) -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|path| path.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(name))
}

/// The command line that starts `app`. The control centre is this same program with `ui`, the
/// other apps start without arguments.
fn launch_command(app: AppId, program: String) -> Vec<String> {
    if app == AppId::WORKPLACE { vec![program, "ui".into()] } else { vec![program] }
}

/// Starts the control centre on its accounts page, where a person finishes what a client asked
/// for (`accounts.connect_github`).
fn start_accounts_page() -> Result<(), String> {
    let mut command = launch_command(AppId::WORKPLACE, sibling(AppId::WORKPLACE.binary).to_string_lossy().into_owned());
    command.extend(["--section".to_string(), "accounts".to_string()]);
    SystemLauncher.spawn(&command).map(|_| ()).map_err(|err| err.to_string())
}

/// Shows the person the control centre: an open one comes to the front, else one starts on its
/// accounts page. An open one is never closed and restarted on a client's request, because that
/// would throw away whatever the person is typing there (a half-pasted token, say).
async fn show_accounts_page(wm: &impl WindowManager, start: fn() -> Result<(), String>) -> Result<(), String> {
    // A window manager that cannot list windows has none we could bring forward; start the page.
    let open = wm.windows(None).await.unwrap_or_default();
    match open.iter().find(|w| w.class == AppId::WORKPLACE.window_class) {
        Some(window) => wm.focus_window(&window.id).await.map_err(|err| err.to_string()),
        None => start(),
    }
}

/// What the tray menu is built from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// `None` while a supported backend runs; otherwise what was looked for.
    pub unsupported: Option<String>,
    pub backend: String,
    pub workspace: Option<String>,
    /// Windows on the active workspace; "Hier laden" needs 0.
    pub workspace_windows: usize,
    pub templates: Vec<TemplateSummary>,
    pub busy: bool,
    /// Why the last refresh could not read something, for the info line.
    pub problem: Option<String>,
    /// The accounts as one state (`torro_pim::combined_state`), for the status line.
    pub account: Option<AccountState>,
}

/// The core's state: window manager, template store, settings, the keys that may talk to it,
/// the GitHub accounts, and a lock so only one save or load runs at a time.
pub struct Core {
    backend: Backend,
    store: Store,
    work: Mutex<()>,
    pim: Arc<PimService>,
    settings: SettingsStore,
    clients: Clients,
    /// The GitHub accounts (task G.1), kept apart from the calendar service: they have no
    /// calendars or task lists, and the core lists both kinds together.
    github: Arc<GithubAccounts>,
    /// Starts the control centre on its accounts page ([`start_accounts_page`]); tests count
    /// the calls instead.
    start_accounts: fn() -> Result<(), String>,
}

impl Core {
    /// The core over `backend` and `store`. The settings are the ones `pim` reads
    /// ([`PimService::settings`]), so hiding a collection and filtering it use the same file.
    /// `clients` decides which keys get in ([`Core::handle_from`]). The GitHub accounts are the
    /// user's ([`GithubAccounts::system`]), loaded only by [`Core::start`].
    pub fn new(backend: Backend, store: Store, pim: Arc<PimService>, clients: Clients) -> Core {
        let settings = pim.settings().clone();
        Core {
            backend,
            store,
            work: Mutex::new(()),
            pim,
            settings,
            clients,
            github: Arc::new(GithubAccounts::system()),
            start_accounts: start_accounts_page,
        }
    }

    /// Starts the background work [`Core::new`] leaves out: loads the GitHub accounts and checks
    /// their tokens, then refreshes their filter lists now and every five minutes (task G.2).
    /// The daemon calls it once, next to `PimService::start`.
    pub fn start(&self) {
        self.github.start();
        let (pim, github) = (Arc::clone(&self.pim), Arc::clone(&self.github));
        tokio::spawn(async move {
            // The tokens come from the keychain in the background; the first refresh waits for
            // the accounts that have lists, a little while at most, instead of a whole period.
            let owners = pim.filter_list_owners();
            for _ in 0..TOKEN_WAIT_ROUNDS {
                let mut ready = true;
                for owner in &owners {
                    ready &= github.api(owner).await.is_some();
                }
                if ready {
                    break;
                }
                tokio::time::sleep(TOKEN_WAIT_STEP).await;
            }
            loop {
                refresh_filter_lists(&pim, &github).await;
                tokio::time::sleep(FILTER_LISTS_EVERY).await;
            }
        });
    }

    /// A request from the socket with the key the client sent. Every request from outside the
    /// core goes through here, in this order (`docs/api.md`, "Prüfung im Dienst"):
    ///
    /// 1. key and rights of the area ([`Clients::admit`]): `unauthorized` or `forbidden`;
    /// 2. for a client key, the collections a write or `set_collection_shown` touches: outside
    ///    its reach the answer is `not-found`, as if they did not exist;
    /// 3. approval ([`torro_ipc::api::Rights::needs_approval`]): refused as
    ///    [`Refused::NeedsApproval`] (`forbidden`) until approvals exist (A.3);
    /// 4. the command, which for a client key reads only what it may see
    ///    ([`Core::handle_within`]).
    ///
    /// `Err` when it was refused; the caller sends [`Refused::response`] and must not treat it as
    /// a command that ran (no notification, no refresh). A `not-found` from step 2 is an `Ok`
    /// answer like any other `not-found`, so nothing tells it apart.
    ///
    /// Rights are read anew for every request: a change of rights, a new key or a removed client
    /// applies from the next request on, also in the middle of an agent's session. A request
    /// that already passed runs with the rights it was admitted with.
    ///
    /// For A.3: when an approved command finally runs, steps 1 and 2 must be checked again with
    /// the rights and collections of that moment, since both may have changed while it waited.
    pub async fn handle_from(&self, key: Option<&Key>, request: Request) -> Result<Response, Refused> {
        let Caller::Client { rights, .. } = self.clients.admit(key, &request)? else {
            return Ok(self.handle(request).await);
        };
        let reach = Reach::Client(&rights);
        if let Some(collection) = self.out_of_reach(reach, &request).await {
            return Ok(no_collection(&collection));
        }
        let operation = request.operation();
        if rights.needs_approval(operation.access) {
            return Err(Refused::NeedsApproval { command: operation.name() });
        }
        Ok(self.handle_within(request, reach).await)
    }

    /// Answers `request` without a key check, with all rights: for the tray menu inside the
    /// core and for the app key.
    pub async fn handle(&self, request: Request) -> Response {
        self.handle_within(request, Reach::Suite).await
    }

    /// For a client: the collection `request` would touch outside its reach, if any. Writes of
    /// events and tasks ask the calendar service; showing or hiding a collection needs it in
    /// the client's selection of calendars or task lists.
    async fn out_of_reach(&self, reach: Reach<'_>, request: &Request) -> Option<String> {
        match (reach, request) {
            (Reach::Client(rights), Request::SetCollectionShown { collection, .. }) => {
                let selected = rights.calendars.contains(collection) || rights.task_lists.contains(collection);
                (!selected).then(|| collection.clone())
            }
            // Only shown task lists in the selection: hidden lists and calendars look unknown.
            (reach @ Reach::Client(_), Request::SetSetting { key: SettingKey::ListOrder, value }) => {
                let list = list_of(value)?;
                (!self.pim.sees_task_list(reach, &list).await).then_some(list)
            }
            (reach, request) => self.pim.out_of_reach(reach, request).await,
        }
    }

    /// Answers `request` with what `reach` may see: for [`Reach::Suite`] everything, for a client
    /// key only its shown collections, in the lists, the account list and the settings
    /// (`hidden_collections` stays empty, so even the ids of hidden collections stay unknown).
    ///
    /// The checks of [`Core::handle_from`] must have passed before.
    pub async fn handle_within(&self, request: Request, reach: Reach<'_>) -> Response {
        let client = matches!(reach, Reach::Client(_));
        match request {
            Request::Ping => Response::Pong { version: version(), api: API_VERSION },
            Request::Status => self.status(reach).await,
            // The core ends only after the answer went out; the daemon takes care of that
            // (`daemon::end_after_answer`), so the core itself just acknowledges it.
            Request::Quit => Response::Ok,
            Request::ListTemplates => match self.store.list() {
                Ok(list) => Response::Templates { templates: list.iter().map(TemplateSummary::from).collect() },
                Err(err) => error(&err),
            },
            Request::SaveTemplate { name } => self.exclusive(self.save(name)).await,
            Request::UpdateTemplate { id } => self.exclusive(self.update(id)).await,
            Request::DeleteTemplate { id } => match self.store.delete(&id) {
                Ok(()) => Response::Ok,
                Err(err) => error(&err),
            },
            Request::LoadTemplate { id, mode } => {
                self.exclusive(async {
                    let template = self.store.load(&id)?;
                    let report =
                        restore(&self.backend, &SystemLauncher, &template, mode, &RestoreOptions::default()).await?;
                    Ok(Response::loaded(&report))
                })
                .await
            }
            Request::Sync => self.sync(reach).await,
            Request::ConnectGithub => self.connect_github(client).await,
            Request::SetGithubToken { token } => {
                let answer = account_answer(self.github.connect_token(token).await);
                self.refresh_connected(answer)
            }
            Request::CreateFilterList { account, name, color, queries } => {
                if !self.github.knows(&account).await {
                    return no_github_account(&account);
                }
                let api = self.github.api(&account).await;
                self.pim.create_filter_list(&account, &name, color.as_deref(), &queries, api.as_ref()).await
            }
            Request::UpdateFilterList { collection, revision, name, color, queries } => {
                let api = self.filter_list_api(&collection).await;
                let (color, api) = (color.as_deref(), api.as_ref());
                self.pim.update_filter_list(&collection, revision, &name, color, &queries, api).await
            }
            Request::DeleteFilterList { collection } => self.pim.delete_filter_list(&collection).await,
            Request::PreviewFilter { account, queries } => {
                if !self.github.knows(&account).await {
                    return no_github_account(&account);
                }
                match self.github.api(&account).await {
                    Some(api) => self.pim.preview_filter(&queries, &api, reach).await,
                    None => Response::Error {
                        code: "not-connected".into(),
                        message: format!("{account} has no token on this device: sign in to GitHub again"),
                    },
                }
            }
            Request::UnpinIssue { collection, url } => {
                let api = self.filter_list_api(&collection).await;
                self.pim.unpin_issue(&collection, &url, api.as_ref()).await
            }
            pim @ (Request::ConnectNextcloud { .. }
            | Request::CalendarCollections
            | Request::TaskCollections
            | Request::Events { .. }
            | Request::Todos
            | Request::SaveEvent { .. }
            | Request::DeleteEvent { .. }
            | Request::SaveTodo { .. }
            | Request::DeleteTodo { .. }
            | Request::MoveTodo { .. }) => self.pim.handle_within(pim, reach).await,
            Request::Settings => {
                let mut settings = self.settings.current();
                if let Reach::Client(rights) = reach {
                    // Not even the ids of lists it may not see (task P.3a).
                    let hidden = std::mem::take(&mut settings.hidden_collections);
                    settings.list_orders.retain(|list, _| rights.task_lists.contains(list) && !hidden.contains(list));
                }
                Response::Settings { settings }
            }
            Request::SetSetting { key: SettingKey::ListOrder, value } => self.set_list_order(&value).await,
            Request::UpdateSettings { settings } => {
                // A client never sees the hidden collections, so what it sends back says nothing
                // about them; they change only through `set_collection_shown`. Kept under the
                // settings lock, so a collection hidden meanwhile stays hidden.
                let updated = match client {
                    true => self.settings.update_keeping_hidden(settings),
                    false => self.settings.update(settings),
                };
                match updated {
                    Ok(()) => Response::Ok,
                    Err(message) => Response::Error { code: "error".into(), message },
                }
            }
            Request::SetSetting { key, value } => match self.settings.set(key, &value) {
                Ok(()) => Response::Ok,
                Err(message) => Response::Error { code: "error".into(), message },
            },
            Request::SetCollectionShown { collection, shown } => self.set_collection_shown(&collection, shown).await,
            Request::Accounts => Response::Accounts { accounts: self.accounts_within(reach).await },
            Request::RemoveAccount { id } if self.github.knows(&id).await => match self.github.remove(&id).await {
                Ok(()) => {
                    self.pim.forget_filter_lists(&id).await;
                    Response::Ok
                }
                Err(message) => Response::Error { code: "error".into(), message },
            },
            Request::RemoveAccount { id } => self.pim.remove_account(&id).await,
            Request::RenameTemplate { id, name } => match self.rename(&id, &name) {
                Ok(template) => Response::Saved { template },
                Err(err) => error(&err),
            },
            Request::OpenApp { app } => match AppId::ALL.iter().find(|a| a.binary == app) {
                Some(app) => match self.open_app(*app).await {
                    Ok(()) => Response::Ok,
                    Err(message) => Response::Error { code: "error".into(), message },
                },
                None => Response::Error { code: "not-found".into(), message: format!("no app {app:?}") },
            },
            Request::ListApps => Response::Apps { apps: crate::apps::list() },
            Request::ListClients => Response::Clients { clients: self.clients.list() },
            Request::CreateClient { name, kind, template } => {
                issued(self.clients.add(&name, &kind, template.rights()).await)
            }
            Request::UpdateClient { id, rights } => done(self.clients.update(&id, rights).await),
            Request::NewClientKey { id } => issued(self.clients.new_key(&id).await),
            Request::RemoveClient { id } => done(self.clients.remove(&id).await),
        }
    }

    /// Every account as `reach` may see it: the calendar service's accounts, then the GitHub
    /// accounts with their filter lists as collections (never with their secrets), and a sign-in
    /// that runs last, as `accounts.list_accounts` promises.
    async fn accounts_within(&self, reach: Reach<'_>) -> Vec<AccountInfo> {
        let mut accounts = self.pim.accounts_within(reach).await;
        let mut github = self.github.infos().await;
        for account in &mut github {
            account.collections = self.pim.filter_lists_within(&account.id, reach).await;
        }
        let at = accounts.iter().position(|account| account.id.is_empty()).unwrap_or(accounts.len());
        accounts.splice(at..at, github);
        accounts
    }

    /// The GitHub access of the account that owns the filter list `collection`, if any.
    async fn filter_list_api(&self, collection: &str) -> Option<torro_pim_github::Api> {
        let account = self.pim.filter_list_account(collection).await?;
        self.github.api(&account).await
    }

    /// After a GitHub account was connected: its filter lists (from an earlier sign-in) are
    /// fetched in the background, so the answer does not wait for GitHub.
    fn refresh_connected(&self, answer: Response) -> Response {
        if let Response::Account { account } = &answer {
            let (pim, github, id) = (Arc::clone(&self.pim), Arc::clone(&self.github), account.id.clone());
            tokio::spawn(async move {
                let api = github.api(&id).await;
                pim.refresh_filter_lists(&id, api.as_ref()).await;
            });
        }
        answer
    }

    /// Syncs the calendar accounts, then checks the GitHub tokens again. Neither stops the
    /// other; the answer is the first failure. Without a Nextcloud account but with a GitHub
    /// one there is something to sync, so the calendar service's `not-connected` does not count.
    async fn sync(&self, reach: Reach<'_>) -> Response {
        let pim = self.pim.handle_within(Request::Sync, reach).await;
        let github = self.github.check_all().await;
        // A list that fails keeps its items and shows its problem; it does not fail the sync.
        refresh_filter_lists(&self.pim, &self.github).await;
        let only_github = matches!(&pim, Response::Error { code, .. } if code == "not-connected")
            && !self.github.infos().await.is_empty();
        match (pim, github) {
            (_, Err(err)) if only_github => github_error(&err),
            (_, Ok(())) if only_github => Response::Ok,
            (Response::Ok, Err(err)) => github_error(&err),
            (pim, _) => pim,
        }
    }

    /// `accounts.connect_github`. The control centre (app key, tray) takes over the GitHub CLI's
    /// sign-in. A client never does: the core shows the person the accounts page
    /// ([`show_accounts_page`]) and answers `opened`.
    async fn connect_github(&self, client: bool) -> Response {
        if !client {
            return self.refresh_connected(account_answer(self.github.connect_cli().await));
        }
        match show_accounts_page(&self.backend, self.start_accounts).await {
            Ok(()) => Response::Opened,
            Err(message) => Response::Error { code: "error".into(), message },
        }
    }

    /// `settings.set_setting` with `list_order` (task P.3a): like every setting, but the list must
    /// be a task list (not a calendar) of a connected account, else `not-found` and nothing is written.
    async fn set_list_order(&self, value: &serde_json::Value) -> Response {
        if let Some(list) = list_of(value)
            && !self.pim.knows_task_list(&list).await
        {
            return no_collection(&list);
        }
        match self.settings.set(SettingKey::ListOrder, value) {
            Ok(()) => Response::Ok,
            Err(message) => Response::Error { code: "error".into(), message },
        }
    }

    /// Shows or hides a collection the account has; an unknown id is `not-found` and is not
    /// written to the settings.
    async fn set_collection_shown(&self, collection: &str, shown: bool) -> Response {
        if !self.pim.knows_collection(collection).await {
            return Response::Error { code: "not-found".into(), message: format!("no collection {collection:?}") };
        }
        match self.settings.set_collection_shown(collection, shown) {
            Ok(()) => Response::Ok,
            Err(message) => Response::Error { code: "error".into(), message },
        }
    }

    /// Window manager events, to refresh the menu when windows or workspaces change.
    pub async fn events(&self) -> Result<Events, WmError> {
        self.backend.events().await
    }

    /// The current state for the tray menu. Never fails; problems end up in `problem`.
    pub async fn snapshot(&self) -> Snapshot {
        let mut snapshot = Snapshot {
            backend: self.backend.name().to_string(),
            busy: self.work.try_lock().is_err(),
            ..Snapshot::default()
        };
        if let Backend::Unsupported { name } = &self.backend {
            snapshot.unsupported = Some(name.clone());
        } else {
            match self.backend.active_workspace().await {
                Ok(workspace) => {
                    match self.backend.windows(Some(&workspace)).await {
                        Ok(windows) => snapshot.workspace_windows = windows.len(),
                        Err(err) => snapshot.problem = Some(err.to_string()),
                    }
                    snapshot.workspace = Some(workspace.0);
                }
                Err(err) => snapshot.problem = Some(err.to_string()),
            }
        }
        match self.store.list() {
            Ok(list) => snapshot.templates = list.iter().map(TemplateSummary::from).collect(),
            Err(err) => snapshot.problem = Some(err.to_string()),
        }
        snapshot.account = Some(self.pim.account_state().await);
        snapshot
    }

    /// Brings the app's window to the front, or starts the app when it has none.
    pub async fn open_app(&self, app: AppId) -> Result<(), String> {
        if let Ok(windows) = self.backend.windows(None).await
            && let Some(window) = windows.iter().find(|w| w.class == app.window_class)
        {
            return self.backend.focus_window(&window.id).await.map_err(|err| err.to_string());
        }
        let program = sibling(app.binary).to_string_lossy().into_owned();
        SystemLauncher.spawn(&launch_command(app, program)).map(|_| ()).map_err(|err| err.to_string())
    }

    /// The status for `reach`. A client without read access to `workspaces` gets no workspace
    /// name and 0 templates: `system.status` is open to every key, the workspaces are not.
    async fn status(&self, reach: Reach<'_>) -> Response {
        let snapshot = self.snapshot().await;
        let workspaces = match reach {
            Reach::Suite => true,
            Reach::Client(rights) => rights.allows(Request::ListTemplates.operation()),
        };
        Response::Status(Status {
            version: version(),
            backend: snapshot.backend,
            workspace: snapshot.workspace.filter(|_| workspaces),
            templates: if workspaces { snapshot.templates.len() } else { 0 },
            busy: snapshot.busy,
        })
    }

    async fn save(&self, name: Option<String>) -> Result<Response, SetupsError> {
        let now = Local::now();
        let name = match name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            Some(name) => name,
            None => auto_name(&self.backend.active_workspace().await?.0, now),
        };
        let id = self.store.unique_id(&name)?;
        let mut template = capture(&self.backend, id, command_for, now).await?;
        template.name = name;
        self.store.save(&template)?;
        Ok(Response::Saved { template: TemplateSummary::from(&template) })
    }

    /// Gives a template a new name; empty names are refused.
    fn rename(&self, id: &str, name: &str) -> Result<TemplateSummary, SetupsError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(SetupsError::Format("a template needs a name".into()));
        }
        let mut template = self.store.load(id)?;
        template.name = name.to_string();
        template.updated = chrono::Utc::now();
        self.store.save(&template)?;
        Ok(TemplateSummary::from(&template))
    }

    /// Captures the active workspace again under an existing template, keeping its name and
    /// creation time.
    async fn update(&self, id: String) -> Result<Response, SetupsError> {
        let old = self.store.load(&id)?;
        let mut template = capture(&self.backend, id, command_for, Local::now()).await?;
        template.name = old.name;
        template.created = old.created;
        self.store.save(&template)?;
        Ok(Response::Saved { template: TemplateSummary::from(&template) })
    }

    /// Runs a save or load unless another one is running.
    async fn exclusive(&self, work: impl Future<Output = Result<Response, SetupsError>>) -> Response {
        let Ok(_guard) = self.work.try_lock() else {
            return Response::Error { code: "busy".into(), message: "another save or load is running".into() };
        };
        work.await.unwrap_or_else(|err| error(&err))
    }
}

/// How often the filter lists are fetched anew (`docs/github.md`, "Technik").
const FILTER_LISTS_EVERY: std::time::Duration = std::time::Duration::from_secs(5 * 60);
/// At start, how long to wait for the tokens of the accounts with filter lists: 30 × 1 s.
const TOKEN_WAIT_ROUNDS: u32 = 30;
const TOKEN_WAIT_STEP: std::time::Duration = std::time::Duration::from_secs(1);

/// Fetches the filter lists of every GitHub account the core knows, one account after the other.
async fn refresh_filter_lists(pim: &PimService, github: &GithubAccounts) {
    for account in github.infos().await {
        let api = github.api(&account.id).await;
        pim.refresh_filter_lists(&account.id, api.as_ref()).await;
    }
}

/// The answer for an account id that is no GitHub account of this core.
fn no_github_account(account: &str) -> Response {
    Response::Error { code: "not-found".into(), message: format!("no GitHub account {account:?}") }
}

fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The answer for a collection a client may not reach: the same `not-found` as for one that
/// does not exist.
fn no_collection(collection: &str) -> Response {
    Response::Error { code: "not-found".into(), message: format!("no collection {collection:?}") }
}

/// The task list a `list_order` value names; `None` for a value that does not fit, which the
/// settings refuse with `error`.
fn list_of(value: &serde_json::Value) -> Option<String> {
    serde_json::from_value::<ListOrderEntry>(value.clone()).ok().map(|entry| entry.list)
}

/// The answer to a new client or key: the key, this once.
fn issued(result: Result<(torro_ipc::ClientInfo, Key), ClientsError>) -> Response {
    match result {
        Ok((client, key)) => Response::ClientKey { client, key: IssuedKey::new(key) },
        Err(err) => clients_error(&err),
    }
}

fn done(result: Result<(), ClientsError>) -> Response {
    result.map_or_else(|err| clients_error(&err), |()| Response::Ok)
}

/// The answer to a GitHub sign-in: the account, or the error with its code. Never the token.
fn account_answer(result: Result<AccountInfo, GithubError>) -> Response {
    match result {
        Ok(account) => Response::Account { account },
        Err(err) => github_error(&err),
    }
}

fn github_error(err: &GithubError) -> Response {
    Response::Error { code: err.code().into(), message: err.to_string() }
}

fn clients_error(err: &ClientsError) -> Response {
    Response::Error { code: err.code().into(), message: err.to_string() }
}

/// A failure as a response with a stable code for scripts.
fn error(err: &SetupsError) -> Response {
    let code = match err {
        SetupsError::WorkspaceNotEmpty { .. } => "workspace-not-empty",
        SetupsError::NotFound(_) => "not-found",
        SetupsError::Wm(WmError::Unsupported(_)) => "unsupported",
        _ => "error",
    };
    Response::Error { code: code.into(), message: err.to_string() }
}

#[cfg(test)]
mod tests {
    use torro_ipc::api::{Area, AreaRights, Level, Rights, RightsTemplate, Selection};

    use super::*;

    /// Templates in `dir`, settings in `dir/config/settings.json`, keys in `dir/state/app.key`
    /// and `dir/config/clients.json` with a keychain in memory, filter lists in
    /// `dir/config/filter-lists.json`: nothing of the user's.
    fn core(dir: &std::path::Path) -> Core {
        let settings = SettingsStore::at(settings_file(dir));
        Core::new(
            Backend::Unsupported { name: "test".into() },
            Store::at(dir.to_path_buf()),
            Arc::new(PimService::with_filter_file(settings, filters_file(dir))),
            crate::clients::tests::open_in(dir),
        )
    }

    /// What a client on the socket gets back for `request` with `key`.
    async fn ask(core: &Core, key: Option<&Key>, request: Request) -> Response {
        core.handle_from(key, request).await.unwrap_or_else(Refused::response)
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn requests_from_outside_need_the_app_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let app_key = crate::clients::tests::app_key(dir.path());

        assert!(matches!(ask(&core, None, Request::Ping).await, Response::Pong { .. }));
        assert_eq!(code(&ask(&core, None, Request::ListTemplates).await), "unauthorized");
        let wrong = Key::new("torro_app_wrong");
        assert_eq!(code(&ask(&core, Some(&wrong), Request::ListTemplates).await), "unauthorized");
        assert_eq!(ask(&core, Some(&app_key), Request::ListTemplates).await, Response::Templates { templates: vec![] });
    }

    /// A client with `rights`, added the way the control centre does it.
    async fn client(core: &Core, rights: Rights) -> (String, Key) {
        let (client, key) = core.clients.add("Claude Code", "claude-code", rights).await.expect("added");
        (client.id, key)
    }

    #[tokio::test]
    async fn a_client_key_runs_what_its_rights_allow_until_removed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let (id, key) = client(&core, RightsTemplate::ReadOnly.rights()).await;

        assert_eq!(
            ask(&core, Some(&key), Request::Ping).await,
            Response::Pong { version: version(), api: API_VERSION }
        );
        assert_eq!(ask(&core, Some(&key), Request::Accounts).await, Response::Accounts { accounts: vec![] });
        assert_eq!(code(&ask(&core, Some(&key), Request::SaveTemplate { name: None }).await), "forbidden");
        core.clients.remove(&id).await.expect("removed");
        assert_eq!(code(&ask(&core, Some(&key), Request::Accounts).await), "unauthorized");
    }

    #[tokio::test]
    async fn the_status_tells_a_client_about_templates_only_with_access_to_workspaces() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let now = chrono::Utc::now();
        let template = torro_setups::Template {
            schema_version: torro_setups::SCHEMA_VERSION,
            id: "dev".into(),
            name: "Dev".into(),
            wm: "hyprland".into(),
            created: now,
            updated: now,
            source: torro_setups::Source { workspace: "1".into(), monitor: None, monitor_rect: None },
            tree: None,
            windows: vec![],
        };
        core.store.save(&template).expect("saved");
        let templates = |answer: Response| match answer {
            Response::Status(status) => status.templates,
            other => panic!("status answers with Status, got {other:?}"),
        };

        let (_, reader) = client(&core, RightsTemplate::ReadOnly.rights()).await;
        assert_eq!(templates(ask(&core, Some(&reader), Request::Status).await), 1);
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.areas.remove(&Area::Workspaces);
        let (_, outsider) = client(&core, rights).await;
        assert_eq!(templates(ask(&core, Some(&outsider), Request::Status).await), 0);
    }

    #[tokio::test]
    async fn changed_rights_apply_to_the_next_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let app_key = crate::clients::tests::app_key(dir.path());
        let (id, key) = client(&core, RightsTemplate::ReadOnly.rights()).await;
        let rename = || Request::RenameTemplate { id: "dev".into(), name: "Dev".into() };
        assert_eq!(code(&ask(&core, Some(&key), rename()).await), "forbidden");

        let update = Request::UpdateClient { id, rights: RightsTemplate::ReadAndWrite.rights() };
        assert_eq!(ask(&core, Some(&app_key), update).await, Response::Ok);
        assert_eq!(code(&ask(&core, Some(&key), rename()).await), "not-found", "runs now: no template dev");
    }

    #[tokio::test]
    async fn clients_commands_are_for_the_app_key_only_and_show_a_key_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let app_key = crate::clients::tests::app_key(dir.path());
        let create = Request::CreateClient {
            name: "Script".into(),
            kind: "script".into(),
            template: RightsTemplate::FullAccess,
        };
        let Response::ClientKey { client, key } = ask(&core, Some(&app_key), create.clone()).await else {
            panic!("create answers with the key");
        };
        assert_eq!(client.rights, RightsTemplate::FullAccess.rights());
        let key = key.into_key();

        let listed = ask(&core, Some(&app_key), Request::ListClients).await;
        assert_eq!(listed, Response::Clients { clients: vec![client.clone()] });
        let json = serde_json::to_string(&listed).expect("serializes");
        assert!(!json.contains(key.expose()) && !json.contains("keySha256"), "{json}");

        // Full access in every area does not open `clients` to a client key.
        for request in [
            Request::ListClients,
            create,
            Request::UpdateClient { id: client.id.clone(), rights: RightsTemplate::FullAccess.rights() },
            Request::NewClientKey { id: client.id.clone() },
            Request::RemoveClient { id: client.id.clone() },
        ] {
            assert_eq!(code(&ask(&core, Some(&key), request).await), "forbidden");
        }

        let new_key = Request::NewClientKey { id: client.id.clone() };
        let Response::ClientKey { key: new, .. } = ask(&core, Some(&app_key), new_key).await else {
            panic!("a new key is answered with the key");
        };
        assert_eq!(code(&ask(&core, Some(&key), Request::Status).await), "unauthorized");
        assert!(matches!(ask(&core, Some(&new.into_key()), Request::Status).await, Response::Status(_)));

        let remove = || Request::RemoveClient { id: client.id.clone() };
        assert_eq!(ask(&core, Some(&app_key), remove()).await, Response::Ok);
        assert_eq!(code(&ask(&core, Some(&app_key), remove()).await), "not-found");
        let nameless = Request::CreateClient { name: " ".into(), kind: "script".into(), template: Default::default() };
        assert_eq!(code(&ask(&core, Some(&app_key), nameless).await), "error");
    }

    #[tokio::test]
    async fn deleting_waits_for_approval_unless_confirm_says_never() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let delete = || Request::DeleteTemplate { id: "dev".into() };
        let (_, key) = client(&core, RightsTemplate::ReadAndWrite.rights()).await;
        let Err(refused) = core.handle_from(Some(&key), delete()).await else { panic!("refused") };
        assert_eq!(refused, Refused::NeedsApproval { command: "workspaces.delete_template" });
        assert_eq!(code(&refused.response()), "forbidden");

        let (_, key) = client(&core, RightsTemplate::FullAccess.rights()).await;
        assert_eq!(code(&ask(&core, Some(&key), delete()).await), "not-found", "runs: no template dev");
    }

    #[tokio::test]
    async fn admin_commands_of_a_client_always_wait_for_approval() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.areas.insert(Area::Settings, AreaRights { level: Level::Full, admin: true });
        let (_, key) = client(&core, rights).await;
        let set = Request::SetSetting { key: torro_ipc::SettingKey::StartAtLogin, value: serde_json::json!(true) };
        let refused = core.handle_from(Some(&key), set).await;
        assert_eq!(refused, Err(Refused::NeedsApproval { command: "settings.set_setting" }));
        assert!(!settings_file(dir.path()).exists(), "nothing written");
    }

    /// A client with full access to everything, without approvals, limited to the calendar
    /// `/cal/work/` and the task list `/tasks/work/`.
    async fn limited_client(core: &Core) -> Key {
        let mut rights = RightsTemplate::FullAccess.rights();
        for area in Area::ALL {
            rights.areas.insert(area, AreaRights { level: Level::Full, admin: true });
        }
        rights.calendars = Selection::Only(vec![id("/cal/work/")]);
        rights.task_lists = Selection::Only(vec![id("/tasks/work/")]);
        client(core, rights).await.1
    }

    fn event_in(collection: &str, href: Option<&str>) -> torro_pim::Event {
        torro_pim::Event {
            uid: "u".into(),
            collection: collection.into(),
            href: href.map(str::to_string),
            etag: None,
            summary: "Standup".into(),
            start: torro_pim::EventTime::DateTime(chrono::Utc::now()),
            end: None,
            location: None,
            description: None,
            recurring: false,
        }
    }

    #[tokio::test]
    async fn a_collection_outside_the_selection_is_not_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        core.pim.cache_collections(vec![calendar(&id("/cal/work/")), calendar(&id("/cal/home/"))]).await;
        let key = limited_client(&core).await;

        for request in [
            Request::SaveEvent { event: event_in(&id("/cal/home/"), None) },
            Request::DeleteEvent { event: event_in(&id("/cal/home/"), Some("/cal/home/u.ics")) },
            Request::SaveEvent { event: event_in(&id("/cal/work/"), Some("/cal/home/u.ics")) },
            Request::SetCollectionShown { collection: id("/cal/home/"), shown: true },
        ] {
            assert_eq!(code(&ask(&core, Some(&key), request.clone()).await), "not-found", "{request:?}");
        }
        // Inside the selection the request gets as far as the approval admin commands need.
        let shown = Request::SetCollectionShown { collection: id("/cal/work/"), shown: false };
        assert_eq!(code(&ask(&core, Some(&key), shown).await), "forbidden");
        assert!(!settings_file(dir.path()).exists());
    }

    /// The hidden calendar `/cal/secret/` appears in no answer to a client key, whatever it
    /// asks; the app key still sees it where the control centre needs it.
    #[tokio::test]
    async fn a_hidden_collection_appears_in_no_answer_to_a_client() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        core.pim.pretend_signed_in().await;
        core.pim.cache_collections(vec![calendar(&id("/cal/work/")), calendar(&id("/cal/secret/"))]).await;
        let hide = Request::SetCollectionShown { collection: id("/cal/secret/"), shown: false };
        assert_eq!(core.handle(hide).await, Response::Ok);
        let (_, key) = client(&core, RightsTemplate::ReadOnly.rights()).await;
        let now = chrono::Utc::now();

        let reads = [
            Request::Status,
            Request::CalendarCollections,
            Request::TaskCollections,
            Request::Events { start: now, end: now + chrono::Duration::days(1) },
            Request::Todos,
            Request::Accounts,
            Request::Settings,
        ];
        for request in reads {
            let answer = ask(&core, Some(&key), request.clone()).await;
            assert!(!matches!(answer, Response::Error { .. }), "{request:?}: {answer:?}");
            let json = serde_json::to_string(&answer).expect("serializes");
            assert!(!json.contains(&id("/cal/secret/")), "{request:?}: {json}");
        }
        let accounts = serde_json::to_string(&ask(&core, Some(&key), Request::Accounts).await).expect("json");
        assert!(accounts.contains(&id("/cal/work/")), "shown collections are listed: {accounts}");

        let app_key = crate::clients::tests::app_key(dir.path());
        for request in [Request::Settings, Request::Accounts] {
            let answer = serde_json::to_string(&ask(&core, Some(&app_key), request).await).expect("json");
            assert!(answer.contains(&id("/cal/secret/")), "the control centre sees what is hidden: {answer}");
        }
    }

    #[tokio::test]
    async fn settings_from_a_client_keep_the_hidden_collections() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        core.pim.cache_collections(vec![calendar(&id("/cal/secret/"))]).await;
        core.handle(Request::SetCollectionShown { collection: id("/cal/secret/"), shown: false }).await;
        let reach_rights = RightsTemplate::FullAccess.rights();
        let mut changed = settings(&core).await;
        changed.hidden_collections.clear();
        changed.start_at_login = false;
        let answer =
            core.handle_within(Request::UpdateSettings { settings: changed }, Reach::Client(&reach_rights)).await;
        assert_eq!(answer, Response::Ok);
        assert_eq!(settings(&core).await.hidden_collections, [id("/cal/secret/")]);
    }

    #[tokio::test]
    async fn a_list_is_ordered_by_hand_only_when_known_and_clients_see_only_their_lists() {
        use torro_core::settings::ListOrder;
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let list = |path: &str| torro_pim::Collection { kind: torro_pim::CollectionKind::Todos, ..calendar(&id(path)) };
        core.pim.cache_collections(vec![list("/tasks/work/"), list("/tasks/home/")]).await;
        let manual = |path: &str| Request::SetSetting {
            key: SettingKey::ListOrder,
            value: serde_json::json!({"list": id(path), "order": "manual"}),
        };
        assert_eq!(core.handle(manual("/tasks/work/")).await, Response::Ok);
        assert_eq!(core.handle(manual("/tasks/home/")).await, Response::Ok);
        assert_eq!(settings(&core).await.list_order(&id("/tasks/work/")), ListOrder::Manual);
        assert_eq!(code(&core.handle(manual("/tasks/nope/")).await), "not-found");
        core.pim.cache_collections(vec![list("/tasks/work/"), list("/tasks/home/"), calendar(&id("/cal/work/"))]).await;
        assert_eq!(code(&core.handle(manual("/cal/work/")).await), "not-found", "a calendar has no list order");
        let wrong = Request::SetSetting { key: SettingKey::ListOrder, value: serde_json::json!({"order": "manual"}) };
        assert_eq!(code(&core.handle(wrong).await), "error");

        // limited to /tasks/work/: the order of home is neither shown nor reachable.
        let key = limited_client(&core).await;
        let Response::Settings { settings } = ask(&core, Some(&key), Request::Settings).await else {
            panic!("settings")
        };
        assert_eq!(settings.list_orders.keys().cloned().collect::<Vec<_>>(), [id("/tasks/work/")]);
        assert_eq!(code(&ask(&core, Some(&key), manual("/tasks/home/")).await), "not-found");
        assert_eq!(code(&ask(&core, Some(&key), manual("/tasks/work/")).await), "forbidden", "admin: approval");
        let moved = Request::MoveTodo { href: "/tasks/home/t.ics".into(), etag: "\"1\"".into(), before: None };
        assert_eq!(code(&ask(&core, Some(&key), moved).await), "not-found", "no such task, or not its list");
    }

    #[tokio::test]
    async fn a_client_with_all_lists_cannot_probe_hidden_lists_or_calendars() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let list = |path: &str| torro_pim::Collection { kind: torro_pim::CollectionKind::Todos, ..calendar(&id(path)) };
        core.pim
            .cache_collections(vec![list("/tasks/work/"), list("/tasks/secret/"), calendar(&id("/cal/work/"))])
            .await;
        core.handle(Request::SetCollectionShown { collection: id("/tasks/secret/"), shown: false }).await;
        let mut rights = RightsTemplate::FullAccess.rights();
        for area in Area::ALL {
            rights.areas.insert(area, AreaRights { level: Level::Full, admin: true });
        }
        rights.task_lists = Selection::All;
        let key = client(&core, rights).await.1;
        let manual = |path: &str| Request::SetSetting {
            key: SettingKey::ListOrder,
            value: serde_json::json!({"list": id(path), "order": "manual"}),
        };
        // Admin needs an approval ("forbidden") for a list it sees; the others never get that far.
        assert_eq!(code(&ask(&core, Some(&key), manual("/tasks/work/")).await), "forbidden");
        assert_eq!(code(&ask(&core, Some(&key), manual("/tasks/secret/")).await), "not-found", "hidden list");
        assert_eq!(code(&ask(&core, Some(&key), manual("/cal/work/")).await), "not-found", "a calendar");
    }

    #[tokio::test]
    async fn refused_requests_change_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let request = Request::SetSetting { key: torro_ipc::SettingKey::StartAtLogin, value: serde_json::json!(true) };
        assert_eq!(code(&ask(&core, None, request).await), "unauthorized");
        assert!(!settings_file(dir.path()).exists());
    }

    #[tokio::test]
    async fn a_damaged_clients_json_still_lets_the_app_in() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = crate::clients::tests::clients_file(dir.path());
        std::fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        std::fs::write(&file, "{ not json").expect("damage");

        let core = core(dir.path());
        let app_key = crate::clients::tests::app_key(dir.path());
        assert!(matches!(ask(&core, Some(&app_key), Request::Status).await, Response::Status(_)));
        assert_eq!(code(&ask(&core, None, Request::Status).await), "unauthorized");
    }

    fn settings_file(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join("config").join("settings.json")
    }

    async fn settings(core: &Core) -> torro_core::settings::Settings {
        match core.handle(Request::Settings).await {
            Response::Settings { settings } => settings,
            other => panic!("Settings is answered with Settings, got {other:?}"),
        }
    }

    /// The collection id of `path` on the test account's server: `<account-id>/<url>` (M1.1).
    fn id(path: &str) -> String {
        torro_pim::account::collection_id(crate::pim::tests::ANNA, &format!("https://cloud.example.org{path}"))
    }

    fn calendar(id: &str) -> torro_pim::Collection {
        torro_pim::Collection {
            id: id.into(),
            name: id.into(),
            kind: torro_pim::CollectionKind::Events,
            color: None,
            writable: true,
            source: None,
        }
    }

    #[tokio::test]
    async fn ping_and_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let core = core(dir.path());
        assert!(matches!(core.handle(Request::Ping).await, Response::Pong { .. }));
        assert_eq!(core.handle(Request::ListTemplates).await, Response::Templates { templates: vec![] });
    }

    #[tokio::test]
    async fn unsupported_backend_reports_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let core = core(dir.path());
        let response = core.handle(Request::SaveTemplate { name: None }).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "unsupported"), "{response:?}");
        let snapshot = core.snapshot().await;
        assert_eq!(snapshot.unsupported.as_deref(), Some("test"));
        assert!(!snapshot.busy);
    }

    #[tokio::test]
    async fn apps_are_listed_without_the_core() {
        let dir = tempfile::tempdir().unwrap();
        let Response::Apps { apps } = core(dir.path()).handle(Request::ListApps).await else {
            panic!("ListApps is answered with Apps");
        };
        let binaries: Vec<_> = apps.iter().map(|app| app.binary.as_str()).collect();
        assert_eq!(binaries, ["torrotasks", "torrocalendar"]);
    }

    #[test]
    fn the_control_centre_starts_with_ui_and_the_apps_without_arguments() {
        assert_eq!(launch_command(AppId::WORKPLACE, "/bin/torroworkplace".into()), ["/bin/torroworkplace", "ui"]);
        assert_eq!(launch_command(AppId::TASKS, "/bin/torrotasks".into()), ["/bin/torrotasks"]);
    }

    #[tokio::test]
    async fn quitting_is_for_the_app_key_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let app_key = crate::clients::tests::app_key(dir.path());
        let (_, key) = client(&core, RightsTemplate::FullAccess.rights()).await;

        assert_eq!(code(&ask(&core, Some(&key), Request::Quit).await), "forbidden");
        assert_eq!(ask(&core, Some(&app_key), Request::Quit).await, Response::Ok);
    }

    #[tokio::test]
    async fn without_account_there_are_no_calendars_or_lists() {
        let dir = tempfile::tempdir().unwrap();
        let core = core(dir.path());
        assert_eq!(core.handle(Request::CalendarCollections).await, Response::Collections { collections: vec![] });
        assert_eq!(core.handle(Request::TaskCollections).await, Response::Collections { collections: vec![] });
    }

    #[tokio::test]
    async fn a_setting_is_set_and_read_back() {
        use torro_core::settings::Language;
        use torro_core::theme::Appearance;
        use torro_ipc::SettingKey;

        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let set = |key, value| Request::SetSetting { key, value };
        assert_eq!(core.handle(set(SettingKey::Language, serde_json::json!("german"))).await, Response::Ok);
        assert_eq!(core.handle(set(SettingKey::Appearance, serde_json::json!("dark"))).await, Response::Ok);
        let stored = settings(&core).await;
        assert_eq!((stored.language, stored.appearance), (Language::German, Appearance::Dark));
        assert!(settings_file(dir.path()).exists(), "written to the given file");
    }

    #[tokio::test]
    async fn a_setting_value_of_the_wrong_type_is_an_error_and_not_written() {
        let dir = tempfile::tempdir().expect("temp dir");
        let request = Request::SetSetting { key: torro_ipc::SettingKey::StartAtLogin, value: serde_json::json!("yes") };
        let response = core(dir.path()).handle(request).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "error"), "{response:?}");
        assert!(!settings_file(dir.path()).exists());
    }

    #[tokio::test]
    async fn a_hidden_collection_is_stored_and_left_out() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        core.pim.cache_collections(vec![calendar(&id("/cal/work/")), calendar(&id("/cal/home/"))]).await;
        let shown = |collection: &str, shown| Request::SetCollectionShown { collection: collection.into(), shown };

        assert_eq!(core.handle(shown(&id("/cal/home/"), false)).await, Response::Ok);
        assert_eq!(settings(&core).await.hidden_collections, [id("/cal/home/")]);
        assert_eq!(
            core.handle(Request::CalendarCollections).await,
            Response::Collections { collections: vec![calendar(&id("/cal/work/"))] }
        );

        assert_eq!(core.handle(shown(&id("/cal/home/"), true)).await, Response::Ok);
        assert!(settings(&core).await.hidden_collections.is_empty());
    }

    #[tokio::test]
    async fn unknown_collection_is_not_found_and_not_written() {
        let dir = tempfile::tempdir().expect("temp dir");
        let response =
            core(dir.path()).handle(Request::SetCollectionShown { collection: id("/cal/nope/"), shown: false }).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "not-found"), "{response:?}");
        assert!(!settings_file(dir.path()).exists());
    }

    #[tokio::test]
    async fn removing_an_unknown_account_is_not_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let response = core(dir.path()).handle(Request::RemoveAccount { id: "nextcloud:nobody@nowhere".into() }).await;
        assert_eq!(code(&response), "not-found");
    }

    #[tokio::test]
    async fn settings_with_a_bare_collection_url_are_refused_and_not_written() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core(dir.path());
        let mut settings = settings(&core).await;
        settings.hidden_collections = vec!["https://cloud.example.org/remote.php/dav/calendars/anna/work/".into()];
        let response = core.handle(Request::UpdateSettings { settings: settings.clone() }).await;
        assert_eq!(code(&response), "error");
        assert!(!settings_file(dir.path()).exists());
        settings.hidden_collections = vec![id("/cal/work/")];
        assert_eq!(core.handle(Request::UpdateSettings { settings }).await, Response::Ok);
    }

    use crate::account::github::tests::{MemoryTokens, NO_CLI, accounts_file, fake_github, service, viewer};

    /// How often a test core started the control centre's accounts page.
    static ACCOUNT_PAGES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn count_account_page() -> Result<(), String> {
        ACCOUNT_PAGES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// [`core`] with GitHub accounts on the fake GitHub at `graphql`, without a GitHub CLI, the
    /// keychain `tokens`, and a counter instead of starting the control centre.
    fn core_with_github(dir: &std::path::Path, tokens: &MemoryTokens, graphql: &str) -> Core {
        Core {
            github: Arc::new(service(dir, tokens, graphql, NO_CLI)),
            start_accounts: count_account_page,
            ..core(dir)
        }
    }

    const TOKEN: &str = "ghp_coreSECRET";

    #[tokio::test]
    async fn the_control_centre_connects_github_lists_it_and_removes_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tokens = MemoryTokens::default();
        let (url, _) = fake_github(|_| (200, Some("repo"), viewer("mahype"))).await;
        let core = core_with_github(dir.path(), &tokens, &url);
        let app_key = crate::clients::tests::app_key(dir.path());

        let set = Request::SetGithubToken { token: torro_ipc::Secret::new(TOKEN) };
        let Response::Account { account } = ask(&core, Some(&app_key), set).await else {
            panic!("a token is answered with the account");
        };
        assert_eq!(
            (account.id.as_str(), account.scopes.as_deref()),
            ("github:mahype", Some(&["repo".to_string()][..]))
        );
        let listed = ask(&core, Some(&app_key), Request::Accounts).await;
        assert_eq!(listed, Response::Accounts { accounts: vec![account.clone()] });
        for answer in [serde_json::to_string(&listed).expect("json"), format!("{listed:?}")] {
            assert!(!answer.contains("SECRET"), "{answer}");
        }
        assert_eq!(tokens.get_plain("github:mahype").as_deref(), Some(TOKEN));
        assert!(!std::fs::read_to_string(accounts_file(dir.path())).expect("stored").contains("SECRET"));

        let remove = Request::RemoveAccount { id: "github:mahype".into() };
        assert_eq!(ask(&core, Some(&app_key), remove.clone()).await, Response::Ok);
        assert_eq!(ask(&core, Some(&app_key), Request::Accounts).await, Response::Accounts { accounts: vec![] });
        assert_eq!(code(&ask(&core, Some(&app_key), remove).await), "not-found", "gone, like any unknown id");
    }

    #[tokio::test]
    async fn no_client_key_hands_in_a_github_token() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tokens = MemoryTokens::default();
        let (url, record) = fake_github(|_| (200, Some("repo"), viewer("mahype"))).await;
        let core = core_with_github(dir.path(), &tokens, &url);
        let mut rights = RightsTemplate::FullAccess.rights();
        for area in Area::ALL {
            rights.areas.insert(area, AreaRights { level: Level::Full, admin: true });
        }
        let (_, key) = client(&core, rights).await;
        let set = Request::SetGithubToken { token: torro_ipc::Secret::new(TOKEN) };
        let Err(refused) = core.handle_from(Some(&key), set).await else { panic!("refused") };
        assert_eq!(refused, Refused::Forbidden { command: "accounts.set_github_token" });
        assert!(record.lock().expect("record").is_empty(), "nothing went to GitHub");
        assert_eq!(tokens.get_plain("github:mahype"), None);
        assert!(!format!("{refused:?} {:?}", refused.response()).contains("SECRET"));
    }

    #[tokio::test]
    async fn without_a_cli_sign_in_connecting_github_is_not_connected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core_with_github(dir.path(), &MemoryTokens::default(), "http://127.0.0.1:9/graphql");
        let answer = core.handle(Request::ConnectGithub).await;
        assert_eq!(code(&answer), "not-connected");
        let Response::Error { message, .. } = answer else { unreachable!() };
        assert!(message.contains("gh auth login"), "says what to do: {message}");
    }

    #[tokio::test]
    async fn a_client_asking_to_connect_github_only_opens_the_accounts_page() {
        let dir = tempfile::tempdir().expect("temp dir");
        let core = core_with_github(dir.path(), &MemoryTokens::default(), "http://127.0.0.1:9/graphql");
        let rights = RightsTemplate::FullAccess.rights();
        let before = ACCOUNT_PAGES.load(std::sync::atomic::Ordering::SeqCst);
        // Without a GitHub CLI the control centre's way would be `not-connected`: the CLI is not
        // asked for a client.
        assert_eq!(core.handle_within(Request::ConnectGithub, Reach::Client(&rights)).await, Response::Opened);
        assert_eq!(ACCOUNT_PAGES.load(std::sync::atomic::Ordering::SeqCst), before + 1);
        // From the socket it is an admin command: it waits for approval like any other.
        let mut admin = RightsTemplate::FullAccess.rights();
        admin.areas.insert(Area::Accounts, AreaRights { level: Level::Full, admin: true });
        let (_, key) = client(&core, admin).await;
        let refused = core.handle_from(Some(&key), Request::ConnectGithub).await;
        assert_eq!(refused, Err(Refused::NeedsApproval { command: "accounts.connect_github" }));
    }

    static FAKE_PAGES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn count_fake_page() -> Result<(), String> {
        FAKE_PAGES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    #[tokio::test]
    async fn an_open_control_centre_comes_to_the_front_and_is_never_closed() {
        use torro_wm::fake::{Call, FakeWm};
        let wm = FakeWm::new("1");
        let other = wm.open_window("foot", None);
        let open = wm.open_window(AppId::WORKPLACE.window_class, None);
        let before = FAKE_PAGES.load(std::sync::atomic::Ordering::SeqCst);

        assert_eq!(show_accounts_page(&wm, count_fake_page).await, Ok(()));
        assert_eq!(wm.calls(), vec![Call::Focus(open.clone())], "only focused, nothing closed");
        assert!(wm.all_windows().iter().any(|w| w.id == open));
        assert!(wm.all_windows().iter().any(|w| w.id == other));
        assert_eq!(FAKE_PAGES.load(std::sync::atomic::Ordering::SeqCst), before, "no second control centre");

        // None open: the page starts.
        let empty = FakeWm::new("1");
        assert_eq!(show_accounts_page(&empty, count_fake_page).await, Ok(()));
        assert!(empty.calls().is_empty());
        assert_eq!(FAKE_PAGES.load(std::sync::atomic::Ordering::SeqCst), before + 1);
    }

    #[tokio::test]
    async fn sync_checks_the_github_tokens_and_reports_a_refused_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let tokens = MemoryTokens::default();
        let accepted = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let gate = Arc::clone(&accepted);
        let (url, _) = fake_github(move |_| match gate.load(std::sync::atomic::Ordering::SeqCst) {
            true => (200, Some("repo"), viewer("mahype")),
            false => (401, None, String::new()),
        })
        .await;
        let core = core_with_github(dir.path(), &tokens, &url);
        let set = Request::SetGithubToken { token: torro_ipc::Secret::new(TOKEN) };
        assert!(matches!(core.handle(set).await, Response::Account { .. }));
        assert_eq!(core.handle(Request::Sync).await, Response::Ok);

        accepted.store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(code(&core.handle(Request::Sync).await), "auth");
        let Response::Accounts { accounts } = core.handle(Request::Accounts).await else { panic!("accounts") };
        assert!(
            matches!(&accounts[0].state, AccountState::Connected { problem: Some(p), .. } if p.starts_with("Sign in again"))
        );
    }

    #[tokio::test]
    async fn missing_template_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let response = core(dir.path()).handle(Request::DeleteTemplate { id: "nope".into() }).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "not-found"), "{response:?}");
    }

    // --- GitHub filter lists (task G.2) ------------------------------------------------------

    fn filters_file(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join("config").join(torro_pim_github::lists::FILE)
    }

    /// One issue as GitHub's GraphQL API sends it: `(repository, number, title, parent)`.
    type Issue = (&'static str, u64, &'static str, Option<(&'static str, u64)>);

    fn node((repo, number, title, parent): Issue) -> serde_json::Value {
        serde_json::json!({
            "__typename": "Issue", "number": number, "title": title, "body": "", "state": "OPEN",
            "url": format!("https://github.com/{repo}/issues/{number}"), "updatedAt": "2026-10-10T08:00:00Z",
            "repository": {"nameWithOwner": repo}, "labels": {"nodes": []}, "assignees": {"nodes": []},
            "milestone": null,
            "parent": parent.map(|(repo, number)| serde_json::json!({"number": number, "repository": {"nameWithOwner": repo}})),
        })
    }

    /// The issues GitHub "has" in a test, by search: every search finds `found`, the pinned
    /// lookup finds `pinned`.
    #[derive(Default)]
    struct World {
        found: Vec<Issue>,
        pinned: Vec<Issue>,
    }

    /// A fake GitHub that knows `mahype` and answers searches and pinned lookups from `world`.
    async fn fake_world(world: Arc<std::sync::Mutex<World>>) -> String {
        let (url, _) = fake_github(move |seen| {
            let world = world.lock().expect("world");
            let body = if seen.body.contains("viewer") {
                viewer("mahype")
            } else if seen.body.contains("search(") {
                let nodes: Vec<_> = world.found.iter().copied().map(node).collect();
                serde_json::json!({"data": {"search": {"issueCount": nodes.len(),
                    "pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": nodes}}})
                .to_string()
            } else {
                let data: serde_json::Map<_, _> = world
                    .pinned
                    .iter()
                    .enumerate()
                    .map(|(k, issue)| (format!("i{k}"), serde_json::json!({"issueOrPullRequest": node(*issue)})))
                    .collect();
                serde_json::json!({ "data": data }).to_string()
            };
            (200, Some("repo"), body)
        })
        .await;
        url
    }

    /// A core with the GitHub account `github:mahype` connected on a fake GitHub with `world`.
    async fn core_with_world(dir: &std::path::Path, world: &Arc<std::sync::Mutex<World>>) -> Core {
        let url = fake_world(Arc::clone(world)).await;
        let core = core_with_github(dir, &MemoryTokens::default(), &url);
        let set = Request::SetGithubToken { token: torro_ipc::Secret::new(TOKEN) };
        assert!(matches!(core.handle(set).await, Response::Account { .. }));
        core
    }

    fn create(name: &str, queries: &[&str]) -> Request {
        Request::CreateFilterList {
            account: "github:mahype".into(),
            name: name.into(),
            color: None,
            queries: queries.iter().map(|q| q.to_string()).collect(),
        }
    }

    fn created(answer: Response) -> torro_pim::Collection {
        match answer {
            Response::Collection { collection } => collection,
            other => panic!("a list is answered with the collection, got {other:?}"),
        }
    }

    async fn todo_ids(core: &Core, key: Option<&Key>) -> Vec<String> {
        let answer = match key {
            Some(key) => ask(core, Some(key), Request::Todos).await,
            None => core.handle(Request::Todos).await,
        };
        match answer {
            Response::Todos { todos } => todos.into_iter().map(|todo| todo.uid).collect(),
            other => panic!("todos, got {other:?}"),
        }
    }

    async fn list_ids(core: &Core, key: Option<&Key>, request: Request) -> Vec<String> {
        let answer = match key {
            Some(key) => ask(core, Some(key), request).await,
            None => core.handle(request).await,
        };
        match answer {
            Response::Collections { collections } => collections.into_iter().map(|c| c.id).collect(),
            other => panic!("collections, got {other:?}"),
        }
    }

    const ISSUE_1: Issue = ("mahype/TorroWorkplace", 1, "Fix the week view", None);
    const ISSUE_2: Issue = ("mahype/TorroWorkplace", 2, "Write the docs", None);
    const PINNED_7: Issue = ("mahype/TorroWorkplace", 7, "Release notes", None);

    #[tokio::test]
    async fn a_filter_list_holds_what_its_searches_find_now_and_its_pinned_items() {
        let dir = tempfile::tempdir().expect("temp dir");
        let world = Arc::new(std::sync::Mutex::new(World { found: vec![ISSUE_1, ISSUE_2], pinned: vec![PINNED_7] }));
        let core = core_with_world(dir.path(), &world).await;

        let address = "https://github.com/mahype/TorroWorkplace/issues?q=is%3Aopen+label%3Abug";
        let list = created(core.handle(create("Bugs", &[address])).await);
        assert!(list.id.starts_with("github:mahype/"), "{}", list.id);
        assert!(!list.writable);
        let source = list.source.clone().expect("a definition");
        assert_eq!(source.queries, ["repo:mahype/TorroWorkplace is:issue is:open label:bug"]);
        assert_eq!((source.revision, source.problem), (1, None));
        assert_eq!(list_ids(&core, None, Request::TaskCollections).await, std::slice::from_ref(&list.id));
        assert!(list_ids(&core, None, Request::CalendarCollections).await.is_empty(), "no calendar");
        assert_eq!(todo_ids(&core, None).await, ["github:mahype/TorroWorkplace#1", "github:mahype/TorroWorkplace#2"]);
        let accounts = serde_json::to_string(&core.handle(Request::Accounts).await).expect("json");
        assert!(accounts.contains(&list.id), "the account lists its filter lists: {accounts}");

        // Issue 2 no longer matches: the next refresh drops it.
        world.lock().expect("world").found = vec![ISSUE_1];
        assert_eq!(core.handle(Request::Sync).await, Response::Ok);
        assert_eq!(todo_ids(&core, None).await, ["github:mahype/TorroWorkplace#1"]);

        // A pinned issue is in the list whatever the searches find.
        let path = filters_file(dir.path());
        let mut file = torro_pim_github::lists::load(&path).expect("stored");
        let stored = file.accounts.get_mut("github:mahype").and_then(|lists| lists.first_mut()).expect("the list");
        stored.pinned.push("github:mahype/TorroWorkplace#7".into());
        torro_pim_github::lists::save(&path, &file).expect("pinned");
        assert_eq!(core.handle(Request::Sync).await, Response::Ok);
        assert_eq!(todo_ids(&core, None).await, ["github:mahype/TorroWorkplace#1", "github:mahype/TorroWorkplace#7"]);

        let unpin = || Request::UnpinIssue {
            collection: list.id.clone(),
            url: "https://github.com/mahype/TorroWorkplace/issues/7".into(),
        };
        assert_eq!(core.handle(unpin()).await, Response::Ok);
        assert_eq!(todo_ids(&core, None).await, ["github:mahype/TorroWorkplace#1"]);
        assert_eq!(code(&core.handle(unpin()).await), "not-found");

        // Items of a filter list are not changed through save_todo yet (G.5).
        let Response::Todos { todos } = core.handle(Request::Todos).await else { panic!("todos") };
        let done = torro_pim::Todo { completed: true, ..todos[0].clone() };
        assert_eq!(code(&core.handle(Request::SaveTodo { todo: done.clone() }).await), "unsupported");
        assert_eq!(code(&core.handle(Request::DeleteTodo { todo: done }).await), "unsupported");

        let update = |revision| Request::UpdateFilterList {
            collection: list.id.clone(),
            revision,
            name: "Open bugs".into(),
            color: Some("#D73A4A".into()),
            queries: vec!["is:open label:bug".into()],
        };
        let changed = created(core.handle(update(1)).await);
        assert_eq!((changed.name.as_str(), changed.source.map(|s| s.revision)), ("Open bugs", Some(2)));
        assert_eq!(code(&core.handle(update(1)).await), "conflict", "a stale revision changes nothing");

        let delete = || Request::DeleteFilterList { collection: list.id.clone() };
        assert_eq!(core.handle(delete()).await, Response::Ok);
        assert!(list_ids(&core, None, Request::TaskCollections).await.is_empty());
        assert!(todo_ids(&core, None).await.is_empty());
        assert_eq!(code(&core.handle(delete()).await), "not-found");
    }

    #[tokio::test]
    async fn a_filter_list_is_checked_before_it_is_stored() {
        let dir = tempfile::tempdir().expect("temp dir");
        let world = Arc::new(std::sync::Mutex::new(World::default()));
        let core = core_with_world(dir.path(), &world).await;
        assert_eq!(code(&core.handle(create(" ", &["is:open"])).await), "error");
        assert_eq!(code(&core.handle(create("Bugs", &[])).await), "error");
        assert_eq!(code(&core.handle(create("Bugs", &["https://github.com/mahype"])).await), "error");
        let other = Request::CreateFilterList {
            account: "github:someone".into(),
            name: "Bugs".into(),
            color: None,
            queries: vec!["is:open".into()],
        };
        assert_eq!(code(&core.handle(other).await), "not-found");
        assert!(!filters_file(dir.path()).exists(), "nothing stored");
    }

    /// A client with full access everywhere but only the repository `mahype/TorroWorkplace`.
    fn repo_limited() -> Rights {
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.github_repos = Selection::Only(vec!["mahype/TorroWorkplace".into()]);
        rights
    }

    #[tokio::test]
    async fn a_client_sees_only_matches_in_its_repositories_whatever_the_search() {
        let dir = tempfile::tempdir().expect("temp dir");
        let secret: Issue = ("mahype/secret", 3, "Private plans", None);
        let child: Issue = ("mahype/TorroWorkplace", 4, "Sub-issue", Some(("mahype/secret", 3)));
        let world = Arc::new(std::sync::Mutex::new(World { found: vec![ISSUE_1, secret, child], pinned: vec![] }));
        let core = core_with_world(dir.path(), &world).await;
        let list = created(core.handle(create("All", &["is:open"])).await);
        let (_, key) = client(&core, repo_limited()).await;

        let Response::Todos { todos } = ask(&core, Some(&key), Request::Todos).await else { panic!("todos") };
        let ids: Vec<&str> = todos.iter().map(|todo| todo.uid.as_str()).collect();
        assert_eq!(ids, ["github:mahype/TorroWorkplace#1", "github:mahype/TorroWorkplace#4"]);
        assert_eq!(todos[1].parent, None, "the parent would name the other repository");

        let preview = Request::PreviewFilter { account: "github:mahype".into(), queries: vec!["is:open".into()] };
        let Response::FilterPreview { matches, todos, .. } = ask(&core, Some(&key), preview.clone()).await else {
            panic!("a preview")
        };
        assert_eq!((matches, todos.len()), (2, 2));
        let Response::FilterPreview { matches, .. } = core.handle(preview).await else { panic!("a preview") };
        assert_eq!(matches, 3, "the suite sees everything");

        // Rewriting the search to the other repository shows nothing more.
        let rewrite = Request::UpdateFilterList {
            collection: list.id.clone(),
            revision: 1,
            name: "All".into(),
            color: None,
            queries: vec!["repo:mahype/secret".into()],
        };
        assert!(matches!(ask(&core, Some(&key), rewrite).await, Response::Collection { .. }));
        world.lock().expect("world").found = vec![secret];
        assert_eq!(core.handle(Request::Sync).await, Response::Ok);
        for request in [Request::Todos, Request::TaskCollections, Request::Accounts] {
            let answer = serde_json::to_string(&ask(&core, Some(&key), request.clone()).await).expect("json");
            assert!(!answer.contains("Private plans") && !answer.contains("secret#3"), "{request:?}: {answer}");
        }
        assert_eq!(todo_ids(&core, None).await, ["github:mahype/secret#3"], "the suite still sees it");

        // Unpinning an item of another repository is as unknown as a list it may not see.
        let unpin = Request::UnpinIssue { collection: list.id.clone(), url: "mahype/secret#3".into() };
        assert_eq!(code(&ask(&core, Some(&key), unpin).await), "not-found");
    }

    #[tokio::test]
    async fn a_list_outside_a_clients_selection_or_without_github_rights_does_not_exist_for_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let world = Arc::new(std::sync::Mutex::new(World { found: vec![ISSUE_1], pinned: vec![] }));
        let core = core_with_world(dir.path(), &world).await;
        let list = created(core.handle(create("Bugs", &["is:open"])).await);

        let mut other = RightsTemplate::FullAccess.rights();
        other.task_lists = Selection::Only(vec![id("/tasks/work/")]);
        let (other_id, key) = client(&core, other.clone()).await;
        assert!(list_ids(&core, Some(&key), Request::TaskCollections).await.is_empty());
        assert!(todo_ids(&core, Some(&key)).await.is_empty());
        for request in [
            Request::UpdateFilterList {
                collection: list.id.clone(),
                revision: 1,
                name: "Mine".into(),
                color: None,
                queries: vec!["is:open".into()],
            },
            Request::DeleteFilterList { collection: list.id.clone() },
            Request::UnpinIssue { collection: list.id.clone(), url: "mahype/TorroWorkplace#1".into() },
        ] {
            assert_eq!(code(&ask(&core, Some(&key), request.clone()).await), "not-found", "{request:?}");
        }

        // A list the client creates itself does not enter its selection.
        let mine = created(ask(&core, Some(&key), create("Mine", &["is:open"])).await);
        assert!(!list_ids(&core, Some(&key), Request::TaskCollections).await.contains(&mine.id));
        let stored = core.clients.list().into_iter().find(|c| c.id == other_id).expect("the client");
        assert_eq!(stored.rights, other, "its rights are as the person set them");

        // Without read access to `github` no filter list and none of its items.
        let mut tasks_only = RightsTemplate::FullAccess.rights();
        tasks_only.areas.remove(&Area::Github);
        let (_, key) = client(&core, tasks_only).await;
        assert!(list_ids(&core, Some(&key), Request::TaskCollections).await.is_empty());
        assert!(todo_ids(&core, Some(&key)).await.is_empty());
        let preview = Request::PreviewFilter { account: "github:mahype".into(), queries: vec!["is:open".into()] };
        assert_eq!(code(&ask(&core, Some(&key), preview).await), "forbidden");
    }

    #[tokio::test]
    async fn filter_lists_come_back_when_the_account_is_signed_in_again() {
        let dir = tempfile::tempdir().expect("temp dir");
        let world = Arc::new(std::sync::Mutex::new(World { found: vec![ISSUE_1], pinned: vec![] }));
        let core = core_with_world(dir.path(), &world).await;
        let list = created(core.handle(create("Bugs", &["is:open"])).await);
        let remove = Request::RemoveAccount { id: "github:mahype".into() };
        assert_eq!(core.handle(remove).await, Response::Ok);
        assert!(list_ids(&core, None, Request::TaskCollections).await.is_empty());
        let set = Request::SetGithubToken { token: torro_ipc::Secret::new(TOKEN) };
        assert!(matches!(core.handle(set).await, Response::Account { .. }));
        assert_eq!(core.handle(Request::Sync).await, Response::Ok);
        assert_eq!(list_ids(&core, None, Request::TaskCollections).await, [list.id]);
    }
}
