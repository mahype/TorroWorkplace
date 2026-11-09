//! Versioned mail workflows: one service behind the UI, API and MCP, with no ambient grants.

mod download;
mod invoice;
pub(crate) mod proposal;
mod runner;
mod security;
mod store;
pub(crate) mod templates;

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use torro_core::i18n::tr;
use torro_ipc::api::{Access, Area, Rights};
use torro_ipc::workflows as wire;
use torro_ipc::{Request, Response};

use crate::clients::Clients;
use crate::mail::service::MailService;
use crate::pim::Reach;

pub(crate) use security::Scope;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NotFound,
    Conflict,
    Forbidden,
    Invalid,
    Unsupported,
    Busy,
    Blocked,
    Offline,
    SenderUnverified,
}

impl Error {
    pub fn response(self) -> Response {
        let (code, message) = match self {
            Self::NotFound => ("not-found", tr("Workflow item is not available.")),
            Self::Conflict => ("conflict", tr("The workflow changed. Review it again.")),
            Self::Forbidden => ("forbidden", tr("Workflow access is not allowed.")),
            Self::Invalid => ("error", tr("The workflow input is invalid.")),
            Self::Unsupported => ("unsupported", tr("This workflow capability is not supported.")),
            Self::Busy => ("busy", tr("The workflow queue is full. Try again later.")),
            Self::Blocked => ("error", tr("The workflow journal cannot be used safely.")),
            Self::Offline => ("offline", tr("The workflow source is currently unavailable.")),
            Self::SenderUnverified => ("forbidden", tr("Sender unverified — manual review required.")),
        };
        Response::Error { code: code.into(), message: message.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("workflow operation failed")
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Authorization {
    digest: String,
    target_fingerprint: String,
    scope: Scope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowRecord {
    workflow: wire::Workflow,
    scope: Scope,
    target_fingerprint: String,
    authorization: Option<Authorization>,
    #[serde(default)]
    state_revision: u64,
    #[serde(default)]
    execution_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuleRecord {
    rule: wire::WorkflowRule,
    scope: Scope,
    #[serde(default)]
    state_revision: u64,
}

/// The private register. Jobs and all three front doors share this service, never an exported file path.
pub struct Service {
    store: store::Store,
    available: bool,
    running: std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
}

impl Service {
    #[cfg(test)]
    pub(crate) fn workflow_for_test(&self, id: &str) -> Result<(Option<String>, bool), Error> {
        let record = self.workflow(&Scope::suite(), None, id)?;
        Ok((record.scope.carrier, record.authorization.is_some()))
    }
    pub fn at(path: PathBuf) -> Self {
        Self { store: store::Store::at(Some(path)), available: true, running: Default::default() }
    }

    pub fn system() -> Self {
        let path =
            crate::mail::MailDirs::for_workplace().map(|dirs| dirs.data.join("workflows").join("journal.sqlite"));
        match path {
            Some(path) => Self::at(path),
            None => Self { available: false, store: store::Store::at(None), running: Default::default() },
        }
    }

    pub fn memory() -> Self {
        Self { available: true, store: store::Store::at(None), running: Default::default() }
    }

    pub async fn answer(&self, request: Request, scope: Scope, clients: &Clients, mail: &MailService) -> Response {
        match Box::pin(self.handle(request, scope, clients, mail, None)).await {
            Ok(response) => response,
            Err(error) => error.response(),
        }
    }

    async fn handle(
        &self,
        request: Request,
        scope: Scope,
        clients: &Clients,
        mail: &MailService,
        approved: Option<&str>,
    ) -> Result<Response, Error> {
        let rights = scope.current(clients)?;
        let rights = rights.as_ref();
        if scope.origin.mode.refuses(request.operation()) {
            return Err(Error::Forbidden);
        }
        if rights.is_some_and(|rights| !rights.allows_request(&request)) {
            return Err(Error::Forbidden);
        }
        if !self.available && !matches!(request, Request::ListWorkflowTemplates | Request::GetWorkflowTemplate { .. }) {
            return Err(Error::Blocked);
        }
        match request {
            Request::ListWorkflowTemplates => Ok(Response::WorkflowTemplates { templates: templates::list()? }),
            Request::GetWorkflowTemplate { id } => Ok(Response::WorkflowTemplate { template: templates::get(&id)? }),
            Request::ListWorkflows => {
                let records: Vec<WorkflowRecord> = self.store.view(|tx| {
                    store::list(tx, "workflow", |owner, account| security::sees(&scope, rights, owner, account))
                })?;
                Ok(Response::Workflows { workflows: records.into_iter().map(|record| record.workflow).collect() })
            }
            Request::GetWorkflow { id } => {
                Ok(Response::Workflow { workflow: self.workflow(&scope, rights, &id)?.workflow })
            }
            Request::CreateWorkflowDraft { spec } => {
                let (account, _, fingerprint) = self.context(mail, rights, &spec.mail, &spec.save_target).await?;
                let record = new_record(spec, scope, account, fingerprint, None)?;
                self.keep(&record)?;
                Ok(Response::WorkflowDraft { workflow: record.workflow })
            }
            Request::InstantiateWorkflowTemplate { template_id, revision, digest, mail: message, save_target } => {
                // Pin the public package before looking up mail; stale metadata cannot fetch or create anything.
                let template = templates::get(&template_id)?;
                if template.revision != revision || template.digest != digest {
                    return Err(Error::Conflict);
                }
                let (account, html, fingerprint) = self.context(mail, rights, &message, &save_target).await?;
                let host = if template_id == "direct_pdf" {
                    Some(download::direct_host(&html).map_err(|_| Error::Unsupported)?)
                } else {
                    None
                };
                let (package, origin) = templates::instantiate(&template_id, revision, &digest, host.as_deref())?;
                let (name, description) = templates::labels(&template_id)?;
                let spec = wire::DraftSpec {
                    name: tr(name).into(),
                    description: tr(description).into(),
                    mail: message,
                    save_target,
                    package,
                };
                let record = new_record(spec, scope, account, fingerprint, Some(origin))?;
                self.keep(&record)?;
                Ok(Response::WorkflowDraft { workflow: record.workflow })
            }
            Request::UpdateWorkflowDraft { id, revision, spec } => {
                let old = self.workflow(&scope, rights, &id)?;
                pinned(&old, revision)?;
                let (account, _, fingerprint) = self.context(mail, rights, &spec.mail, &spec.save_target).await?;
                // Editing through the app does not turn a client's durable carrier into suite authority.
                let mut carrier = old.scope;
                if let Some(rights) = rights {
                    carrier.ceiling = Some(carrier.ceiling.as_ref().ok_or(Error::Forbidden)?.common(rights));
                }
                let mut record = new_record(spec, carrier, account, fingerprint, old.workflow.template_origin)?;
                record.workflow.id = id;
                record.workflow.revision = revision.checked_add(1).ok_or(Error::Conflict)?;
                record.state_revision = old.state_revision;
                record.execution_epoch = old.execution_epoch.checked_add(1).ok_or(Error::Conflict)?;
                self.replace(&scope, rights, revision, &record)?;
                Ok(Response::WorkflowDraft { workflow: record.workflow })
            }
            Request::DuplicateWorkflow { id, revision } => {
                let old = self.workflow(&scope, rights, &id)?;
                pinned(&old, revision)?;
                let (account, _, fingerprint) =
                    self.context(mail, rights, &old.workflow.spec.mail, &old.workflow.spec.save_target).await?;
                let mut carrier = old.scope;
                if let Some(rights) = rights {
                    carrier.ceiling = Some(carrier.ceiling.as_ref().ok_or(Error::Forbidden)?.common(rights));
                }
                let record =
                    new_record(old.workflow.spec, carrier, account, fingerprint, old.workflow.template_origin)?;
                self.keep(&record)?;
                Ok(Response::WorkflowDraft { workflow: record.workflow })
            }
            Request::SaveWorkflow { id, revision } => {
                let mut record = self.workflow(&scope, rights, &id)?;
                pinned(&record, revision)?;
                self.context(mail, rights, &record.workflow.spec.mail, &record.workflow.spec.save_target).await?;
                if !record.workflow.tested {
                    return Err(Error::Conflict);
                }
                // Availability for deliberate manual reuse is not a grant and not an automatic rule.
                if !record.workflow.saved {
                    record.workflow.enabled = true;
                }
                record.workflow.saved = true;
                self.replace(&scope, rights, revision, &record)?;
                Ok(Response::Workflow { workflow: record.workflow })
            }
            Request::SetWorkflowEnabled { id, revision, enabled } => {
                let mut record = self.workflow(&scope, rights, &id)?;
                pinned(&record, revision)?;
                if enabled && (!record.workflow.saved || !record.workflow.tested) {
                    return Err(Error::Forbidden);
                }
                record.workflow.enabled = enabled;
                record.execution_epoch = record.execution_epoch.checked_add(1).ok_or(Error::Conflict)?;
                self.replace(&scope, rights, revision, &record)?;
                Ok(Response::Workflow { workflow: record.workflow })
            }
            Request::RemoveWorkflow { id, revision } => {
                self.store.transaction(|tx| {
                    let record: WorkflowRecord = store::get(tx, "workflow", &id, |owner, account| {
                        security::sees(&scope, rights, owner, account)
                    })?;
                    pinned(&record, revision)?;
                    store::remove(tx, "workflow", &id)
                })?;
                Ok(Response::Ok)
            }
            Request::PreviewWorkflowRun { id, revision, mail: message } => {
                let record = self.workflow(&scope, rights, &id)?;
                pinned(&record, revision)?;
                let (account, _, _) = self.context(mail, rights, &message, &record.workflow.spec.save_target).await?;
                if account != record.workflow.account {
                    return Err(Error::NotFound);
                }
                Ok(Response::WorkflowPreview {
                    preview: wire::WorkflowPreview {
                        workflow: id,
                        revision,
                        mail: message,
                        hosts: record.workflow.spec.package.hosts,
                        save_target: record.workflow.spec.save_target,
                        possible_writes: true,
                        authorization_required: record.authorization.is_none(),
                        matches: Vec::new(),
                        untrusted: true,
                    },
                })
            }
            Request::AuthorizeWorkflow { id, revision } => {
                if scope.carrier.is_some() {
                    return Err(Error::Forbidden);
                }
                let mut record = self.workflow(&scope, rights, &id)?;
                pinned(&record, revision)?;
                let current = record.scope.current(clients)?;
                if current.as_ref().is_some_and(|rights| !rights.area(Area::Workflows).allows(Access::Write)) {
                    return Err(Error::Forbidden);
                }
                let (_, _, target_fingerprint) = self
                    .context(mail, current.as_ref(), &record.workflow.spec.mail, &record.workflow.spec.save_target)
                    .await?;
                if !record.workflow.tested || target_fingerprint != record.target_fingerprint {
                    return Err(Error::Conflict);
                }
                record.authorization = Some(Authorization {
                    digest: record.workflow.digest.clone(),
                    target_fingerprint,
                    scope: record.scope.clone(),
                });
                record.workflow.authorized = true;
                self.replace(&scope, rights, revision, &record)?;
                Ok(Response::Workflow { workflow: record.workflow })
            }
            Request::RevokeWorkflowAuthorization { id } => {
                if scope.carrier.is_some() {
                    return Err(Error::Forbidden);
                }
                let mut record = self.workflow(&scope, rights, &id)?;
                let revision = record.workflow.revision;
                record.authorization = None;
                record.workflow.authorized = false;
                record.workflow.enabled = false;
                record.execution_epoch = record.execution_epoch.checked_add(1).ok_or(Error::Conflict)?;
                self.replace(&scope, rights, revision, &record)?;
                Ok(Response::Workflow { workflow: record.workflow })
            }
            Request::ListWorkflowRules => {
                let records: Vec<RuleRecord> = self.store.view(|tx| {
                    store::list(tx, "rule", |owner, account| security::sees(&scope, rights, owner, account))
                })?;
                Ok(Response::WorkflowRules { rules: records.into_iter().map(|record| record.rule).collect() })
            }
            Request::CreateWorkflowRule { spec } => {
                let workflow = self.check_rule(&scope, rights, &spec)?;
                let carrier = if scope.carrier.is_none() { workflow.scope } else { scope };
                let record = RuleRecord {
                    rule: wire::WorkflowRule {
                        id: crate::mail::random_hex(16).map_err(|_| Error::Blocked)?,
                        revision: 1,
                        spec,
                        enabled: false,
                        activated: None,
                    },
                    scope: carrier,
                    state_revision: 0,
                };
                self.store.transaction(|tx| {
                    store::put(
                        tx,
                        "rule",
                        &record.rule.id,
                        record.scope.owner(),
                        &record.rule.spec.account,
                        "",
                        &record,
                    )
                })?;
                Ok(Response::WorkflowRule { rule: record.rule })
            }
            Request::UpdateWorkflowRule { id, revision, spec } => {
                let workflow = self.check_rule(&scope, rights, &spec)?;
                let mut record = self.rule(&scope, rights, &id)?;
                if record.rule.revision != revision {
                    return Err(Error::Conflict);
                }
                if record.scope.owner() != workflow.scope.owner() {
                    return Err(Error::Forbidden);
                }
                if let Some(rights) = rights {
                    record.scope.ceiling = Some(record.scope.ceiling.as_ref().ok_or(Error::Forbidden)?.common(rights));
                }
                record.rule.spec = spec;
                record.rule.revision = revision.checked_add(1).ok_or(Error::Conflict)?;
                record.rule.enabled = false;
                record.rule.activated = None;
                self.replace_rule(&scope, rights, revision, &record)?;
                Ok(Response::WorkflowRule { rule: record.rule })
            }
            Request::DuplicateWorkflowRule { id, revision } => {
                let mut record = self.rule(&scope, rights, &id)?;
                if record.rule.revision != revision {
                    return Err(Error::Conflict);
                }
                self.check_rule(&scope, rights, &record.rule.spec)?;
                record.rule.id = crate::mail::random_hex(16).map_err(|_| Error::Blocked)?;
                record.rule.revision = 1;
                record.state_revision = 0;
                record.rule.enabled = false;
                record.rule.activated = None;
                self.store.transaction(|tx| {
                    store::put(
                        tx,
                        "rule",
                        &record.rule.id,
                        record.scope.owner(),
                        &record.rule.spec.account,
                        "",
                        &record,
                    )
                })?;
                Ok(Response::WorkflowRule { rule: record.rule })
            }
            Request::SetWorkflowRuleEnabled { id, revision, enabled } => {
                let mut record = self.rule(&scope, rights, &id)?;
                if record.rule.revision != revision {
                    return Err(Error::Conflict);
                }
                if record.rule.enabled == enabled {
                    return Ok(Response::WorkflowRule { rule: record.rule });
                }
                if enabled {
                    let current = record.scope.current(clients)?;
                    if current.as_ref().is_some_and(|rights| !rights.area(Area::Workflows).allows(Access::Write)) {
                        return Err(Error::Forbidden);
                    }
                    let workflow = self.check_rule(&record.scope, current.as_ref(), &record.rule.spec)?;
                    if !workflow.workflow.enabled
                        || !workflow.workflow.saved
                        || !workflow.workflow.tested
                        || workflow
                            .authorization
                            .as_ref()
                            .is_none_or(|authorization| authorization.digest != workflow.workflow.digest)
                    {
                        return Err(Error::Forbidden);
                    }
                    let (_, _, fingerprint) = self
                        .context(
                            mail,
                            current.as_ref(),
                            &workflow.workflow.spec.mail,
                            &workflow.workflow.spec.save_target,
                        )
                        .await?;
                    if workflow
                        .authorization
                        .as_ref()
                        .is_none_or(|authorization| authorization.target_fingerprint != fingerprint)
                    {
                        return Err(Error::Conflict);
                    }
                    // Activation is only a boundary for future events. No enumeration or execution happens here.
                    // Outbox occurrence times are stored at whole-second precision. Keep the
                    // activation boundary at the same precision or events from this second can
                    // be discarded as older than their rule.
                    record.rule.activated = chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp(), 0);
                } else {
                    record.rule.activated = None;
                }
                record.rule.enabled = enabled;
                self.replace_rule(&scope, rights, revision, &record)?;
                Ok(Response::WorkflowRule { rule: record.rule })
            }
            Request::RemoveWorkflowRule { id, revision } => {
                self.store.transaction(|tx| {
                    let record: RuleRecord =
                        store::get(tx, "rule", &id, |owner, account| security::sees(&scope, rights, owner, account))?;
                    if record.rule.revision != revision {
                        return Err(Error::Conflict);
                    }
                    store::remove(tx, "rule", &id)
                })?;
                Ok(Response::Ok)
            }
            Request::RunWorkflow { id, revision, mail: message, test, repeat } => {
                self.enqueue(&scope, clients, mail, &id, revision, &message, test, repeat, approved).await
            }
            Request::ListWorkflowRuns { workflow, mail: message } => {
                self.list_runs(&scope, rights, workflow.as_deref(), message.as_deref(), mail)
            }
            Request::GetWorkflowRun { id } => self.get_run(&scope, rights, &id, mail),
            Request::CancelWorkflowRun { id } => self.cancel_run(&scope, rights, &id),
            request @ (Request::GetWorkflowDocument { .. }
            | Request::OpenWorkflowDocument { .. }
            | Request::ShowWorkflowDocument { .. }) => self.document(request, &scope, rights, mail),
            Request::PreviewWorkflowRule { spec } => self.preview_rule(&scope, rights, mail, &spec).await,
            // Proposal uses the core's model/data-rule host, not a second model configuration here.
            _ => Err(Error::Unsupported),
        }
    }

    fn workflow(&self, scope: &Scope, rights: Option<&Rights>, id: &str) -> Result<WorkflowRecord, Error> {
        self.store
            .view(|tx| store::get(tx, "workflow", id, |owner, account| security::sees(scope, rights, owner, account)))
    }

    fn rule(&self, scope: &Scope, rights: Option<&Rights>, id: &str) -> Result<RuleRecord, Error> {
        self.store.view(|tx| store::get(tx, "rule", id, |owner, account| security::sees(scope, rights, owner, account)))
    }

    fn check_rule(
        &self,
        scope: &Scope,
        rights: Option<&Rights>,
        spec: &wire::RuleSpec,
    ) -> Result<WorkflowRecord, Error> {
        if !security::reads(rights, &spec.account) {
            return Err(Error::NotFound);
        }
        let workflow = self.workflow(scope, rights, &spec.workflow)?;
        pinned(&workflow, workflow.workflow.revision)?;
        if spec.account != workflow.workflow.account {
            return Err(Error::NotFound);
        }
        if spec.trigger == wire::Trigger::CategoryAssigned && spec.category.is_none() {
            return Err(Error::Invalid);
        }
        crate::mail::filters::filter_query(&spec.query).map_err(|_| Error::Invalid)?;
        Ok(workflow)
    }

    fn replace_rule(
        &self,
        scope: &Scope,
        rights: Option<&Rights>,
        revision: u32,
        record: &RuleRecord,
    ) -> Result<(), Error> {
        self.store.transaction(|tx| {
            let old: RuleRecord = store::get(tx, "rule", &record.rule.id, |owner, account| {
                security::sees(scope, rights, owner, account)
            })?;
            if old.rule.revision != revision || old.state_revision != record.state_revision {
                return Err(Error::Conflict);
            }
            let mut updated = record.clone();
            updated.state_revision = updated.state_revision.checked_add(1).ok_or(Error::Conflict)?;
            store::put(tx, "rule", &record.rule.id, record.scope.owner(), &record.rule.spec.account, "", &updated)
        })
    }

    fn keep(&self, record: &WorkflowRecord) -> Result<(), Error> {
        self.store.transaction(|tx| {
            let count: i64 = tx
                .query_row("SELECT COUNT(*) FROM records WHERE kind='workflow'", [], |row| row.get(0))
                .map_err(|_| Error::Blocked)?;
            if count >= 500 {
                return Err(Error::Busy);
            }
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM records WHERE kind='workflow' AND id=?1)",
                    [&record.workflow.id],
                    |row| row.get(0),
                )
                .map_err(|_| Error::Blocked)?;
            if exists {
                return Err(Error::Conflict);
            }
            store::put(tx, "workflow", &record.workflow.id, record.scope.owner(), &record.workflow.account, "", record)
        })
    }

    fn replace(
        &self,
        scope: &Scope,
        rights: Option<&Rights>,
        revision: u32,
        record: &WorkflowRecord,
    ) -> Result<(), Error> {
        self.store.transaction(|tx| {
            let old: WorkflowRecord = store::get(tx, "workflow", &record.workflow.id, |owner, account| {
                security::sees(scope, rights, owner, account)
            })?;
            pinned(&old, revision)?;
            if old.state_revision != record.state_revision {
                return Err(Error::Conflict);
            }
            let mut updated = record.clone();
            updated.state_revision = updated.state_revision.checked_add(1).ok_or(Error::Conflict)?;
            if old.workflow.revision != updated.workflow.revision {
                // The person's activation belongs to the reviewed revision. Reset all references in
                // this transaction so testing/saving/authorizing the edit cannot revive old automation.
                let rules: Vec<RuleRecord> = store::list(tx, "rule", |_, _| true)?;
                for mut rule in rules.into_iter().filter(|rule| rule.rule.spec.workflow == record.workflow.id) {
                    rule.rule.enabled = false;
                    rule.rule.activated = None;
                    rule.state_revision = rule.state_revision.checked_add(1).ok_or(Error::Conflict)?;
                    store::put(tx, "rule", &rule.rule.id, rule.scope.owner(), &rule.rule.spec.account, "", &rule)?;
                }
            }
            store::put(
                tx,
                "workflow",
                &record.workflow.id,
                record.scope.owner(),
                &record.workflow.account,
                "",
                &updated,
            )
        })
    }

    /// Strictly local prerequisite checks. Callers do not get the HTML or canonical path in their response.
    async fn context(
        &self,
        mail: &MailService,
        rights: Option<&Rights>,
        message: &str,
        target: &str,
    ) -> Result<(String, String, String), Error> {
        if rights.is_some_and(|rights| !rights.area(Area::Mail).allows(Access::Read)) {
            return Err(Error::Forbidden);
        }
        let reach = rights.map_or(Reach::Suite, Reach::Client);
        let stores = mail.stores(reach).await;
        let located = crate::mail::read::locate(&stores, message).await.map_err(|_| Error::NotFound)?;
        if !security::reads(rights, &located.account) {
            return Err(Error::NotFound);
        }
        if !security::target(rights, message, target) {
            return Err(Error::Forbidden);
        }
        let fingerprint = target_fingerprint(mail, target)?;
        let id = message.to_string();
        let html = located
            .store
            .run(move |store| store.html(&id))
            .await
            .map_err(|_| Error::Blocked)?
            .flatten()
            .unwrap_or_default();
        Ok((located.account, html, fingerprint))
    }
}

fn target_fingerprint(mail: &MailService, target: &str) -> Result<String, Error> {
    let target = mail.save_targets().get(target).map_err(|_| Error::Blocked)?.ok_or(Error::NotFound)?;
    let folder = std::path::Path::new(&target.spec.folder);
    if !folder.is_absolute()
        || !folder.symlink_metadata().is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
    {
        return Err(Error::Forbidden);
    }
    let canonical = folder.canonicalize().map_err(|_| Error::Forbidden)?;
    let metadata = folder.symlink_metadata().map_err(|_| Error::Forbidden)?;
    #[cfg(unix)]
    let directory_identity = {
        use std::os::unix::fs::MetadataExt;
        vec![metadata.dev(), metadata.ino()]
    };
    #[cfg(not(unix))]
    let directory_identity = {
        let created = metadata
            .created()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .ok_or(Error::Unsupported)?;
        vec![created.as_secs(), u64::from(created.subsec_nanos())]
    };
    let binding = serde_json::to_vec(&(&target, canonical, directory_identity)).map_err(|_| Error::Blocked)?;
    Ok(digest(&binding))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn check_spec(spec: &wire::DraftSpec) -> Result<(), Error> {
    if spec.name.trim().is_empty()
        || !wire::safe_text(&spec.name, 120)
        || !wire::safe_text(&spec.description, 4096)
        || spec.mail.len() > 200
        || spec.save_target.len() > 200
    {
        return Err(Error::Invalid);
    }
    spec.package.validate().map_err(|_| Error::Unsupported)?;
    if spec.package.steps.iter().any(|step| matches!(step, wire::Step::RenderInvoiceCopy { .. }))
        && spec.package.binding.fonts_digest != invoice::binding()
    {
        return Err(Error::Unsupported);
    }
    Ok(())
}

fn new_record(
    spec: wire::DraftSpec,
    scope: Scope,
    account: String,
    target_fingerprint: String,
    template_origin: Option<wire::TemplateOrigin>,
) -> Result<WorkflowRecord, Error> {
    check_spec(&spec)?;
    let encoded = serde_json::to_vec(&spec).map_err(|_| Error::Invalid)?;
    Ok(WorkflowRecord {
        workflow: wire::Workflow {
            id: crate::mail::random_hex(16).map_err(|_| Error::Blocked)?,
            revision: 1,
            digest: digest(&encoded),
            account,
            spec,
            template_origin,
            saved: false,
            enabled: false,
            tested: false,
            authorized: false,
            untrusted: true,
        },
        scope,
        target_fingerprint,
        authorization: None,
        state_revision: 0,
        execution_epoch: 0,
    })
}

fn pinned(record: &WorkflowRecord, revision: u32) -> Result<(), Error> {
    if record.workflow.revision != revision {
        return Err(Error::Conflict);
    }
    check_spec(&record.workflow.spec)?;
    let encoded = serde_json::to_vec(&record.workflow.spec).map_err(|_| Error::Blocked)?;
    if digest(&encoded) != record.workflow.digest {
        return Err(Error::Blocked);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
