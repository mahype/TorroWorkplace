//! The commands of the core, shared by every way in: the socket (`ctl`, hotkeys, the control
//! centre, the apps, scripts and agents) and the tray menu. Each [`Request`] gets one
//! [`Response`]; the logic exists only here. Requests from the socket pass the key and rights
//! check first ([`Core::handle_from`]); the tray menu runs inside the core and needs none.

use std::sync::Arc;

use chrono::Local;
use tokio::sync::Mutex;
use torro_core::AppId;
use torro_ipc::{API_VERSION, IssuedKey, Key, Request, Response, Status};
use torro_pim::AccountState;
use torro_setups::capture::{auto_name, capture};
use torro_setups::launch::{Launcher, SystemLauncher, command_for};
use torro_setups::restore::{RestoreOptions, restore};
use torro_setups::storage::Store;
use torro_setups::{SetupsError, TemplateSummary};
use torro_wm::{Backend, Events, WindowManager, WmError};

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
/// and a lock so only one save or load runs at a time.
pub struct Core {
    backend: Backend,
    store: Store,
    work: Mutex<()>,
    pim: Arc<PimService>,
    settings: SettingsStore,
    clients: Clients,
}

impl Core {
    /// The core over `backend` and `store`. The settings are the ones `pim` reads
    /// ([`PimService::settings`]), so hiding a collection and filtering it use the same file.
    /// `clients` decides which keys get in ([`Core::handle_from`]).
    pub fn new(backend: Backend, store: Store, pim: Arc<PimService>, clients: Clients) -> Core {
        let settings = pim.settings().clone();
        Core { backend, store, work: Mutex::new(()), pim, settings, clients }
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
            pim @ (Request::ConnectNextcloud { .. }
            | Request::Sync
            | Request::CalendarCollections
            | Request::TaskCollections
            | Request::Events { .. }
            | Request::Todos
            | Request::SaveEvent { .. }
            | Request::DeleteEvent { .. }
            | Request::SaveTodo { .. }
            | Request::DeleteTodo { .. }) => self.pim.handle_within(pim, reach).await,
            Request::Settings => {
                let mut settings = self.settings.current();
                if client {
                    settings.hidden_collections.clear();
                }
                Response::Settings { settings }
            }
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
            Request::Accounts => Response::Accounts { accounts: self.pim.accounts_within(reach).await },
            Request::RemoveAccount { id } => match self.pim.remove_account(&id).await {
                Ok(()) => Response::Ok,
                Err(message) => Response::Error { code: "error".into(), message },
            },
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

fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The answer for a collection a client may not reach: the same `not-found` as for one that
/// does not exist.
fn no_collection(collection: &str) -> Response {
    Response::Error { code: "not-found".into(), message: format!("no collection {collection:?}") }
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
    /// and `dir/config/clients.json` with a keychain in memory: nothing of the user's.
    fn core(dir: &std::path::Path) -> Core {
        let settings = SettingsStore::at(settings_file(dir));
        Core::new(
            Backend::Unsupported { name: "test".into() },
            Store::at(dir.to_path_buf()),
            Arc::new(PimService::with_settings(settings)),
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
    async fn missing_template_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let response = core(dir.path()).handle(Request::DeleteTemplate { id: "nope".into() }).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "not-found"), "{response:?}");
    }
}
