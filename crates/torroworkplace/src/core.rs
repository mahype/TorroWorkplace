//! The commands of the core, shared by every way in: the socket (`ctl`, hotkeys, later the
//! control centre) and the tray menu. Each [`Request`] gets one [`Response`]; the logic
//! exists only here.

use std::sync::Arc;

use chrono::Local;
use tokio::sync::Mutex;
use torro_core::AppId;
use torro_ipc::{Request, Response, Status};
use torro_pim::AccountState;
use torro_setups::capture::{auto_name, capture};
use torro_setups::launch::{Launcher, SystemLauncher, command_for};
use torro_setups::restore::{RestoreOptions, restore};
use torro_setups::storage::Store;
use torro_setups::{SetupsError, TemplateSummary};
use torro_wm::{Backend, Events, WindowManager, WmError};

use crate::pim::PimService;

/// `name` next to the daemon's binary, else from `PATH`.
fn sibling(name: &str) -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|path| path.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(name))
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
    /// The Nextcloud account, for the status line.
    pub account: Option<AccountState>,
}

/// The core's state: window manager, template store, and a lock so only one save or load
/// runs at a time.
pub struct Core {
    backend: Backend,
    store: Store,
    work: Mutex<()>,
    pim: Arc<PimService>,
}

impl Core {
    pub fn new(backend: Backend, store: Store, pim: Arc<PimService>) -> Core {
        Core { backend, store, work: Mutex::new(()), pim }
    }

    pub async fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: version() },
            Request::Status => self.status().await,
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
            | Request::DeleteTodo { .. }) => self.pim.handle(pim).await,
            Request::Settings => Response::Settings { settings: crate::settings_service::current() },
            Request::UpdateSettings { settings } => match crate::settings_service::update(settings) {
                Ok(()) => Response::Ok,
                Err(message) => Response::Error { code: "error".into(), message },
            },
            Request::SetSetting { key, value } => match crate::settings_service::set(key, &value) {
                Ok(()) => Response::Ok,
                Err(message) => Response::Error { code: "error".into(), message },
            },
            Request::SetCollectionShown { collection, shown } => self.set_collection_shown(&collection, shown).await,
            Request::Accounts => Response::Accounts { accounts: self.pim.accounts().await },
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
        }
    }

    /// Shows or hides a collection the account has; an unknown id is `not-found` and is not
    /// written to the settings.
    async fn set_collection_shown(&self, collection: &str, shown: bool) -> Response {
        if !self.pim.knows_collection(collection).await {
            return Response::Error { code: "not-found".into(), message: format!("no collection {collection:?}") };
        }
        match crate::settings_service::set_collection_shown(collection, shown) {
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
        SystemLauncher.spawn(&[program]).map(|_| ()).map_err(|err| err.to_string())
    }

    async fn status(&self) -> Response {
        let snapshot = self.snapshot().await;
        Response::Status(Status {
            version: version(),
            backend: snapshot.backend,
            workspace: snapshot.workspace,
            templates: snapshot.templates.len(),
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
    use super::*;

    fn core(dir: &std::path::Path) -> Core {
        Core::new(
            Backend::Unsupported { name: "test".into() },
            Store::at(dir.to_path_buf()),
            Arc::new(PimService::new()),
        )
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

    #[tokio::test]
    async fn without_account_there_are_no_calendars_or_lists() {
        let dir = tempfile::tempdir().unwrap();
        let core = core(dir.path());
        assert_eq!(core.handle(Request::CalendarCollections).await, Response::Collections { collections: vec![] });
        assert_eq!(core.handle(Request::TaskCollections).await, Response::Collections { collections: vec![] });
    }

    // The successful paths of SetSetting and SetCollectionShown write the user's settings file;
    // `settings_service` tests the changes themselves. Here only the refusals, which write nothing.
    #[tokio::test]
    async fn unknown_collection_is_not_found_and_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let response = core(dir.path())
            .handle(Request::SetCollectionShown { collection: "/cal/nope/".into(), shown: false })
            .await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "not-found"), "{response:?}");
    }

    #[tokio::test]
    async fn a_setting_value_of_the_wrong_type_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let request = Request::SetSetting { key: torro_ipc::SettingKey::StartAtLogin, value: serde_json::json!("yes") };
        let response = core(dir.path()).handle(request).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "error"), "{response:?}");
    }

    #[tokio::test]
    async fn missing_template_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let response = core(dir.path()).handle(Request::DeleteTemplate { id: "nope".into() }).await;
        assert!(matches!(response, Response::Error { ref code, .. } if code == "not-found"), "{response:?}");
    }
}
