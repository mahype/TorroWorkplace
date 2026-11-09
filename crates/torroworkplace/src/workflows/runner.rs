//! Durable bounded jobs, exact-path commits and recovery. Network secrets exist only inside a running attempt.

use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use torro_ipc::api::{Access, Area, Rights};
use torro_ipc::workflows::{self as wire, RunReason, RunState};
use torro_ipc::{Request, Response};
use torro_mail::save_target::FileFacts;

use super::{
    Error, Scope, Service, WorkflowRecord, digest, download, invoice, pinned, security, store, target_fingerprint,
};
use crate::clients::Clients;
use crate::mail::read;
use crate::mail::save::{self, WorkflowReservation};
use crate::mail::service::MailService;
use crate::pim::Reach;

#[derive(Clone, Serialize, Deserialize)]
struct RunRecord {
    run: wire::WorkflowRun,
    scope: Scope,
    execution_scope: Scope,
    workflow_digest: String,
    target_fingerprint: String,
    raw_hash: String,
    dedup: String,
    due: DateTime<Utc>,
    reservation: Option<WorkflowReservation>,
    execution_epoch: u64,
    #[serde(default)]
    rule_binding: Option<(String, u32, u64)>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    one_time: bool,
    #[serde(default)]
    verified_sender: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Dedup {
    run: String,
    generation: u32,
}

fn active(state: RunState) -> bool {
    matches!(state, RunState::Queued | RunState::Running | RunState::WaitingBody | RunState::WaitingCategory)
}

/// Hash the complete raw message with canonical CRLF line endings, not Message-ID or a lossy MIME rendering.
fn raw_hash(raw: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut at = 0;
    while let Some(&byte) = raw.get(at) {
        if byte == b'\r' {
            hash.update(b"\r\n");
            at += if raw.get(at + 1) == Some(&b'\n') { 2 } else { 1 };
        } else if byte == b'\n' {
            hash.update(b"\r\n");
            at += 1;
        } else {
            hash.update([byte]);
            at += 1;
        }
    }
    hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Service {
    async fn original(&self, mail: &MailService, scope: &Scope, clients: &Clients, id: &str) -> Result<Vec<u8>, Error> {
        let rights = scope.current(clients)?;
        if rights.as_ref().is_some_and(|rights| !rights.area(Area::Mail).allows(Access::Read)) {
            return Err(Error::Forbidden);
        }
        let stores = mail.stores(rights.as_ref().map_or(Reach::Suite, Reach::Client)).await;
        let located = read::locate(&stores, id).await.map_err(|_| Error::NotFound)?;
        if !security::reads(rights.as_ref(), &located.account) {
            return Err(Error::NotFound);
        }
        let place = located.place.clone();
        let cached = located
            .store
            .run(move |store| {
                let Some(original) = store.original_at(&place)? else { return Ok(None) };
                if original.bytes > 50 * 1024 * 1024 {
                    return Ok(None);
                }
                let mut bytes = Vec::new();
                while (bytes.len() as u64) < original.bytes {
                    let Some(chunk) = store.original_chunk(original.seq, bytes.len() as u64, 64 * 1024)? else {
                        return Ok(None);
                    };
                    if chunk.is_empty() {
                        return Ok(None);
                    }
                    bytes.extend(chunk);
                }
                if bytes.len() as u64 != original.bytes || digest(&bytes) != original.sha256 {
                    return Ok(None);
                }
                Ok(Some(bytes))
            })
            .await
            .map_err(|_| Error::Blocked)?;
        let raw = match cached {
            Some(raw) => raw,
            None => read::fetch_raw(mail, &located).await.map_err(|_| Error::Offline)?,
        };
        // The potentially slow IMAP read did not freeze the carrier's rights.
        let now = scope.current(clients)?;
        if !security::reads(now.as_ref(), &located.account) {
            return Err(Error::Forbidden);
        }
        Ok(raw)
    }

    /// Prepare from service-owned objects, never client-supplied display labels. No document GET or write.
    pub(crate) async fn approval(
        &self,
        request: &Request,
        scope: &Scope,
        clients: &Clients,
        mail: &MailService,
    ) -> Result<(String, crate::approvals::WorkflowFacts), Error> {
        let rights = scope.current(clients)?;
        let id = match request {
            Request::RunWorkflow { id, .. }
            | Request::UpdateWorkflowDraft { id, .. }
            | Request::SaveWorkflow { id, .. }
            | Request::DuplicateWorkflow { id, .. }
            | Request::SetWorkflowEnabled { id, .. }
            | Request::RemoveWorkflow { id, .. } => Some(id.as_str()),
            Request::CreateWorkflowRule { spec }
            | Request::PreviewWorkflowRule { spec }
            | Request::UpdateWorkflowRule { spec, .. } => Some(spec.workflow.as_str()),
            _ => None,
        };
        let rule_id = match request {
            Request::DuplicateWorkflowRule { id, .. }
            | Request::SetWorkflowRuleEnabled { id, .. }
            | Request::RemoveWorkflowRule { id, .. }
            | Request::UpdateWorkflowRule { id, .. } => Some(id.as_str()),
            _ => None,
        };
        let rule = rule_id.map(|id| self.rule(scope, rights.as_ref(), id)).transpose()?;
        let id = id.or_else(|| rule.as_ref().map(|rule| rule.rule.spec.workflow.as_str()));
        let record = id.map(|id| self.workflow(scope, rights.as_ref(), id)).transpose()?;
        let mut facts = crate::approvals::WorkflowFacts::default();
        let mut binding = serde_json::to_vec(&(request, scope, &rule)).map_err(|_| Error::Invalid)?;
        if let Some(record) = record {
            pinned(&record, record.workflow.revision)?;
            let message = match request {
                Request::RunWorkflow { mail, .. } => mail.as_str(),
                _ => record.workflow.spec.mail.as_str(),
            };
            let (account, _, fingerprint) =
                self.context(mail, rights.as_ref(), message, &record.workflow.spec.save_target).await?;
            if account != record.workflow.account {
                return Err(Error::NotFound);
            }
            let target = mail
                .save_targets()
                .get(&record.workflow.spec.save_target)
                .map_err(|_| Error::Blocked)?
                .ok_or(Error::NotFound)?;
            facts.name = Some(record.workflow.spec.name.clone());
            facts.account = Some(record.workflow.account.clone());
            facts.target = Some(target.spec.name);
            facts.hosts = record.workflow.spec.package.hosts.clone();
            facts.test = matches!(request, Request::RunWorkflow { test: true, .. });
            facts.repeat = matches!(request, Request::RunWorkflow { repeat: true, .. });
            binding.extend(serde_json::to_vec(&(&record, fingerprint)).map_err(|_| Error::Blocked)?);
            if matches!(request, Request::RunWorkflow { .. }) {
                let raw = self.original(mail, scope, clients, message).await?;
                let hash = raw_hash(&raw);
                binding.extend(hash.as_bytes());
                let key = dedup_key(&account, &hash, &record.workflow.id, &record.workflow.spec.save_target)?;
                let prior = self.store.view(|tx| prior_dedup(tx, &key))?;
                binding.extend(serde_json::to_vec(&(&key, prior)).map_err(|_| Error::Blocked)?);
            }
        }
        let rule_spec = match request {
            Request::CreateWorkflowRule { spec } | Request::UpdateWorkflowRule { spec, .. } => Some(spec),
            _ => rule.as_ref().map(|rule| &rule.rule.spec),
        };
        if let Some(spec) = rule_spec {
            facts.rule_query = Some(spec.query.clone());
            facts.rule_trigger = Some(spec.trigger);
        }
        Ok((digest(&binding), facts))
    }

    pub(crate) async fn answer_approved(
        &self,
        request: Request,
        scope: Scope,
        clients: &Clients,
        mail: &MailService,
        fingerprint: Option<&str>,
    ) -> Response {
        match self.approval(&request, &scope, clients, mail).await {
            Ok((now, _)) if Some(now.as_str()) == fingerprint => {
                match Box::pin(self.handle(request, scope, clients, mail, fingerprint)).await {
                    Ok(response) => response,
                    Err(error) => error.response(),
                }
            }
            Ok(_) => Error::Conflict.response(),
            Err(error) => error.response(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn enqueue(
        &self,
        scope: &Scope,
        clients: &Clients,
        mail: &MailService,
        id: &str,
        revision: u32,
        message: &str,
        test: bool,
        repeat: bool,
        approved: Option<&str>,
    ) -> Result<Response, Error> {
        self.enqueue_bound(scope, clients, mail, id, revision, message, test, repeat, approved, None).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn enqueue_bound(
        &self,
        scope: &Scope,
        clients: &Clients,
        mail: &MailService,
        id: &str,
        revision: u32,
        message: &str,
        test: bool,
        repeat: bool,
        approved: Option<&str>,
        event: Option<(&str, &super::RuleRecord)>,
    ) -> Result<Response, Error> {
        if scope.carrier.is_some() && (test || repeat) && approved.is_none() {
            return Err(Error::Forbidden);
        }
        let rights = scope.current(clients)?;
        let workflow = self.workflow(scope, rights.as_ref(), id)?;
        pinned(&workflow, revision)?;
        let one_time = event.is_none() && (scope.carrier.is_none() || approved.is_some());
        if !test
            && (!workflow.workflow.saved
                || !workflow.workflow.tested
                || !workflow.workflow.enabled
                || (!one_time && workflow.authorization.is_none()))
        {
            return Err(Error::Forbidden);
        }
        let (account, _, fingerprint) =
            self.context(mail, rights.as_ref(), message, &workflow.workflow.spec.save_target).await?;
        if account != workflow.workflow.account {
            return Err(Error::NotFound);
        }
        if fingerprint != workflow.target_fingerprint {
            return Err(Error::Conflict);
        }
        // Running through the app never removes the workflow's original carrier or narrowing.
        let mut carrier = workflow.scope.clone();
        if let Some(rights) = rights.as_ref() {
            carrier.ceiling = Some(carrier.ceiling.as_ref().ok_or(Error::Forbidden)?.common(rights));
        }
        let effective = carrier.current(clients)?;
        if !security::target(effective.as_ref(), message, &workflow.workflow.spec.save_target) {
            return Err(Error::Forbidden);
        }
        let raw = self.original(mail, &carrier, clients, message).await?;
        let hash = raw_hash(&raw);
        let verified_sender = if event.is_some() {
            Some(mail.workflow_verified_sender(&raw).await.ok_or(Error::SenderUnverified)?)
        } else {
            None
        };
        let dedup = dedup_key(&account, &hash, id, &workflow.workflow.spec.save_target)?;
        let request = Request::RunWorkflow { id: id.into(), revision, mail: message.into(), test, repeat };
        let mut approval_base =
            serde_json::to_vec(&(&request, scope, Option::<&super::RuleRecord>::None)).map_err(|_| Error::Blocked)?;
        approval_base.extend(serde_json::to_vec(&(&workflow, &fingerprint)).map_err(|_| Error::Blocked)?);
        approval_base.extend(hash.as_bytes());
        let now = scope.current(clients)?;
        if now.as_ref().is_some_and(|rights| {
            !rights.allows_request(&request)
                || !security::target(Some(rights), message, &workflow.workflow.spec.save_target)
        }) {
            return Err(Error::Forbidden);
        }
        let run = self.store.transaction(|tx| {
            let placeholder = if let Some((event, rule)) = event {
                match store::get::<RunRecord>(tx, "run", &event_job(event, &rule.rule.id), |_, _| true) {
                    Ok(old) if !matches!(old.run.state, RunState::WaitingBody | RunState::WaitingCategory) => return Ok(old.run),
                    Ok(_) => true,
                    Err(Error::NotFound) => false,
                    Err(error) => return Err(error),
                }
            } else { false };
            // Serialize capacity, dedup, generation and insertion in the same durable transaction.
            let prior = prior_dedup(tx, &dedup)?;
            let mut binding = approval_base;
            binding.extend(serde_json::to_vec(&(&dedup, &prior)).map_err(|_| Error::Blocked)?);
            if approved.is_some_and(|approved| digest(&binding) != approved) { return Err(Error::Conflict); }
            if let Some(prior) = &prior {
                let old: RunRecord = store::get(tx, "run", &prior.run, |owner, account| security::sees(scope, rights.as_ref(), owner, account))?;
                if !repeat { return Ok(old.run); }
                if active(old.run.state) { return Err(Error::Conflict); }
            }
            let pending: i64 = tx.query_row("SELECT COUNT(*) FROM records WHERE kind='run' AND state IN ('queued','running','committing','waiting_body','waiting_category')", [], |row| row.get(0)).map_err(|_| Error::Blocked)?;
            if pending >= 100 && !placeholder { return Err(Error::Busy); }
            let generation = prior.map_or(Ok(0), |prior| prior.generation.checked_add(1).ok_or(Error::Conflict))?;
            let run = wire::WorkflowRun {
                id: match event { Some((event, rule)) => event_job(event, &rule.rule.id), None => crate::mail::random_hex(16).map_err(|_| Error::Blocked)? }, workflow: id.into(), revision,
                account: account.clone(), mail: message.into(), save_target: workflow.workflow.spec.save_target.clone(),
                test, generation, state: RunState::Queued, reason: None, attempts: 0, created: Utc::now(), document: None, untrusted: true,
            };
            let record = RunRecord { run: run.clone(), scope: carrier, execution_scope: scope.clone(), workflow_digest: workflow.workflow.digest,
                target_fingerprint: fingerprint, raw_hash: hash, dedup: dedup.clone(), due: Utc::now(), reservation: None,
                execution_epoch: workflow.execution_epoch,
                rule_binding: event.map(|(_, rule)| (rule.rule.id.clone(), rule.rule.revision, rule.state_revision)),
                event: event.map(|(id, _)| id.to_string()), one_time, verified_sender };
            put_run(tx, &record, "queued")?;
            store::put(tx, "dedup", &dedup, record.scope.owner(), &account, "", &Dedup { run: run.id.clone(), generation })?;
            Ok(run)
        })?;
        Ok(Response::WorkflowRun { run })
    }

    pub(super) fn list_runs(
        &self,
        scope: &Scope,
        rights: Option<&Rights>,
        workflow: Option<&str>,
        message: Option<&str>,
        mail: &MailService,
    ) -> Result<Response, Error> {
        let records: Vec<RunRecord> = self
            .store
            .view(|tx| store::list(tx, "run", |owner, account| security::sees(scope, rights, owner, account)))?;
        Ok(Response::WorkflowRuns {
            runs: records
                .into_iter()
                .filter(|record| {
                    workflow.is_none_or(|id| record.run.workflow == id)
                        && message.is_none_or(|id| record.run.mail == id)
                })
                .map(|mut record| {
                    document_status(mail, &mut record.run);
                    record.run
                })
                .collect(),
        })
    }

    fn run_record(&self, scope: &Scope, rights: Option<&Rights>, id: &str) -> Result<RunRecord, Error> {
        self.store.view(|tx| store::get(tx, "run", id, |owner, account| security::sees(scope, rights, owner, account)))
    }

    pub(super) fn get_run(
        &self,
        scope: &Scope,
        rights: Option<&Rights>,
        id: &str,
        mail: &MailService,
    ) -> Result<Response, Error> {
        let mut record = self.run_record(scope, rights, id)?;
        document_status(mail, &mut record.run);
        Ok(Response::WorkflowRun { run: record.run })
    }

    pub(super) fn cancel_run(&self, scope: &Scope, rights: Option<&Rights>, id: &str) -> Result<Response, Error> {
        let run = self.store.transaction(|tx| {
            let mut record: RunRecord =
                store::get(tx, "run", id, |owner, account| security::sees(scope, rights, owner, account))?;
            if active(record.run.state) {
                record.run.state = RunState::Cancelled;
                put_run(tx, &record, "cancelled")?;
            }
            Ok(record.run)
        })?;
        if let Some(cancelled) = self.running.lock().unwrap_or_else(PoisonError::into_inner).get(id) {
            cancelled.store(true, Ordering::Release);
        }
        Ok(Response::WorkflowRun { run })
    }

    pub(super) fn document(
        &self,
        request: Request,
        scope: &Scope,
        rights: Option<&Rights>,
        mail: &MailService,
    ) -> Result<Response, Error> {
        let id = match &request {
            Request::GetWorkflowDocument { id, .. }
            | Request::OpenWorkflowDocument { id }
            | Request::ShowWorkflowDocument { id } => id,
            _ => return Err(Error::Invalid),
        };
        // Metadata filtering happens before the document's private path/body is read.
        let records: Vec<RunRecord> = self
            .store
            .view(|tx| store::list(tx, "run", |owner, account| security::sees(scope, rights, owner, account)))?;
        let record = records
            .into_iter()
            .find(|record| record.run.document.as_ref().is_some_and(|doc| &doc.id == id))
            .ok_or(Error::NotFound)?;
        if !security::target(rights, &record.run.mail, &record.run.save_target) {
            return Err(Error::Forbidden);
        }
        if target_fingerprint(mail, &record.run.save_target)? != record.target_fingerprint {
            return Err(Error::Conflict);
        }
        let document = record.run.document.ok_or(Error::NotFound)?;
        let target =
            mail.save_targets().get(&document.save_target).map_err(|_| Error::Blocked)?.ok_or(Error::NotFound)?;
        let path =
            save::verify_workflow_document(&target.spec, &document.relative_path, &document.sha256, document.bytes)
                .map_err(|_| Error::Conflict)?;
        if let Request::GetWorkflowDocument { offset, limit, .. } = request {
            if limit == 0 || limit > 256 * 1024 || offset > document.bytes {
                return Err(Error::Invalid);
            }
            // Verify the bytes of the same held file before returning a chunk; a path check alone has a race.
            let mut file = std::fs::File::open(path).map_err(|_| Error::NotFound)?;
            let mut bytes = Vec::new();
            (&mut file).take(50 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| Error::Conflict)?;
            if bytes.len() as u64 != document.bytes || digest(&bytes) != document.sha256 {
                return Err(Error::Conflict);
            }
            file.seek(SeekFrom::Start(offset)).map_err(|_| Error::Conflict)?;
            let start = usize::try_from(offset).map_err(|_| Error::Invalid)?;
            let end = start.saturating_add(limit as usize).min(bytes.len());
            let chunk = bytes.get(start..end).ok_or(Error::Invalid)?;
            return Ok(Response::WorkflowDocument {
                document,
                offset,
                base64: base64::engine::general_purpose::STANDARD.encode(chunk),
                eof: end == bytes.len(),
            });
        }
        open_document(&path, matches!(request, Request::ShowWorkflowDocument { .. }))?;
        Ok(Response::Ok)
    }

    pub(super) async fn preview_rule(
        &self,
        scope: &Scope,
        rights: Option<&Rights>,
        mail: &MailService,
        spec: &wire::RuleSpec,
    ) -> Result<Response, Error> {
        let workflow = self.check_rule(scope, rights, spec)?;
        let query = crate::mail::store::SearchQuery {
            query: crate::mail::filters::filter_query(&spec.query).map_err(|_| Error::Invalid)?,
            in_inbox: true,
            categories: spec.category.into_iter().collect(),
            ..Default::default()
        };
        let mut matches = Vec::new();
        for (account, store) in mail.stores(rights.map_or(Reach::Suite, Reach::Client)).await {
            if account == spec.account {
                matches = store
                    .run(move |store| store.search(&query, 500, 0))
                    .await
                    .map_err(|_| Error::Blocked)?
                    .into_iter()
                    .map(|message| message.id)
                    .collect();
                break;
            }
        }
        Ok(Response::WorkflowPreview {
            preview: wire::WorkflowPreview {
                workflow: workflow.workflow.id,
                revision: workflow.workflow.revision,
                mail: workflow.workflow.spec.mail,
                hosts: workflow.workflow.spec.package.hosts,
                save_target: workflow.workflow.spec.save_target,
                possible_writes: true,
                authorization_required: workflow.authorization.is_none(),
                matches,
                untrusted: true,
            },
        })
    }

    /// Durable outbox handoff. Each source event is acknowledged only after all selected jobs (or visible
    /// waits/pauses/skips) have been journaled. Repeating after a crash reuses the same event/run identities.
    pub(crate) async fn ingest(
        &self,
        clients: &Clients,
        mail: &MailService,
        gate: &(dyn Fn(&Scope) -> bool + Sync),
    ) -> Result<(), Error> {
        use crate::mail::store::{WorkflowMailCause, WorkflowMailState};
        if !gate(&Scope::suite()) {
            return Ok(());
        }
        for account in mail.accounts_in(Reach::Suite).await {
            // Waiting events change in place. Always revisit the bounded oldest unacknowledged page;
            // advancing a permanent cursor would lose their eventual ready/failed transitions.
            let events = mail.workflow_events(&account, None, 100).await.map_err(|_| Error::Offline)?;
            for event in &events {
                let taken =
                    self.store.view(|tx| match store::get::<bool>(tx, "event_ack", &event.id, |_, _| true) {
                        Ok(value) => Ok(value),
                        Err(Error::NotFound) => Ok(false),
                        Err(error) => Err(error),
                    })?;
                if taken {
                    mail.acknowledge_workflow_event(&account, &event.id).await.map_err(|_| Error::Offline)?;
                    continue;
                }
                let rules: Vec<super::RuleRecord> =
                    self.store.view(|tx| store::list(tx, "rule", |_, found| found == account))?;
                let mut waiting = false;
                let mut handled = BTreeSet::new();
                for rule in rules.into_iter().filter(|rule| {
                    rule.rule.enabled
                        && rule.rule.activated.is_some_and(|activated| event.occurred_at >= activated)
                        && matches!(
                            (event.cause, rule.rule.spec.trigger),
                            (WorkflowMailCause::Arrival, wire::Trigger::NewMail)
                                | (WorkflowMailCause::CategoryChanged, wire::Trigger::CategoryAssigned)
                        )
                }) {
                    let rights = match rule.scope.current(clients) {
                        Ok(rights) => rights,
                        Err(_) => continue,
                    };
                    if !security::reads(rights.as_ref(), &account) {
                        continue;
                    }
                    let workflow = match self.check_rule(&rule.scope, rights.as_ref(), &rule.rule.spec) {
                        Ok(workflow) => workflow,
                        Err(_) => continue,
                    };
                    handled.insert(rule.rule.id.clone());
                    let id = event_job(&event.id, &rule.rule.id);
                    let old = self.store.view(|tx| match store::get::<RunRecord>(tx, "run", &id, |_, _| true) {
                        Ok(old) => Ok(Some(old)),
                        Err(Error::NotFound) => Ok(None),
                        Err(error) => Err(error),
                    })?;
                    if old
                        .as_ref()
                        .is_some_and(|old| !matches!(old.run.state, RunState::WaitingBody | RunState::WaitingCategory))
                    {
                        continue;
                    }
                    let mut placeholder = RunRecord {
                        run: wire::WorkflowRun {
                            id: id.clone(),
                            workflow: workflow.workflow.id.clone(),
                            revision: workflow.workflow.revision,
                            account: account.clone(),
                            mail: event.message.clone(),
                            save_target: workflow.workflow.spec.save_target.clone(),
                            test: false,
                            generation: 0,
                            state: RunState::Paused,
                            reason: None,
                            attempts: 0,
                            created: event.occurred_at,
                            document: None,
                            untrusted: true,
                        },
                        scope: workflow.scope.clone(),
                        execution_scope: rule.scope.clone(),
                        workflow_digest: workflow.workflow.digest.clone(),
                        target_fingerprint: workflow.target_fingerprint.clone(),
                        raw_hash: String::new(),
                        dedup: String::new(),
                        due: Utc::now(),
                        reservation: None,
                        execution_epoch: workflow.execution_epoch,
                        rule_binding: Some((rule.rule.id.clone(), rule.rule.revision, rule.state_revision)),
                        event: Some(event.id.clone()),
                        one_time: false,
                        verified_sender: None,
                    };
                    let too_old =
                        event.received_at.is_some_and(|received| received < Utc::now() - chrono::Duration::days(7));
                    if event.received_at.is_none() {
                        placeholder.run.reason = Some(RunReason::Unsupported);
                    } else if too_old {
                        placeholder.run.state = RunState::Skipped;
                        placeholder.run.reason = Some(RunReason::TooOld);
                    } else if !gate(&rule.scope) || !workflow.workflow.enabled || workflow.authorization.is_none() {
                        placeholder.run.reason = Some(RunReason::PermissionRevoked);
                    } else if event.state == WorkflowMailState::Failed {
                        placeholder.run.reason = Some(RunReason::Unsupported);
                    } else if matches!(event.state, WorkflowMailState::WaitingBody | WorkflowMailState::WaitingCategory)
                    {
                        placeholder.run.state = if event.state == WorkflowMailState::WaitingBody {
                            RunState::WaitingBody
                        } else {
                            RunState::WaitingCategory
                        };
                        waiting = true;
                    } else if rule.rule.spec.trigger == wire::Trigger::CategoryAssigned
                        && rule.rule.spec.category.is_some_and(|category| event.category != Some(category))
                    {
                        placeholder.run.state = RunState::Skipped;
                    } else {
                        let query = crate::mail::store::SearchQuery {
                            query: crate::mail::filters::filter_query(&rule.rule.spec.query)
                                .map_err(|_| Error::Invalid)?,
                            only: Some(vec![event.message.clone()]),
                            in_inbox: true,
                            categories: if rule.rule.spec.trigger == wire::Trigger::NewMail {
                                rule.rule.spec.category.into_iter().collect()
                            } else {
                                Vec::new()
                            },
                            ..Default::default()
                        };
                        let store = mail.store(&account).await.map_err(|_| Error::Offline)?;
                        let matches =
                            store.read(move |store| store.search(&query, 1, 0)).await.map_err(|_| Error::Blocked)?;
                        if matches.is_empty() {
                            placeholder.run.state = RunState::Skipped;
                        } else if mail
                            .workflow_sender_domain(&account, &event.message)
                            .await
                            .map_err(|_| Error::Offline)?
                            .is_none()
                        {
                            placeholder.run.reason = Some(RunReason::SenderUnverified);
                        } else {
                            match self
                                .enqueue_bound(
                                    &rule.scope,
                                    clients,
                                    mail,
                                    &workflow.workflow.id,
                                    workflow.workflow.revision,
                                    &event.message,
                                    false,
                                    false,
                                    None,
                                    Some((&event.id, &rule)),
                                )
                                .await
                            {
                                Ok(Response::WorkflowRun { run }) if run.id == id => continue,
                                Ok(Response::WorkflowRun { .. }) => {
                                    placeholder.run.state = RunState::Skipped;
                                    placeholder.run.reason = Some(RunReason::Duplicate);
                                }
                                Err(Error::Busy) => {
                                    waiting = true;
                                    continue;
                                }
                                Err(error) => {
                                    placeholder.run.reason = Some(failed_permission(error).reason);
                                }
                                _ => return Err(Error::Blocked),
                            }
                        }
                    }
                    self.store.transaction(|tx| {
                        let count: i64 = tx.query_row("SELECT COUNT(*) FROM records WHERE kind='run' AND state IN ('queued','running','committing','waiting_body','waiting_category')", [], |row| row.get(0)).map_err(|_| Error::Blocked)?;
                        if active(placeholder.run.state) && old.is_none() && count >= 100 { return Err(Error::Busy); }
                        put_run(tx, &placeholder, if active(placeholder.run.state) { "waiting_body" } else { "paused" })
                    })?;
                }
                self.store.transaction(|tx| {
                    let jobs: Vec<RunRecord> = store::list(tx, "run", |_, found| found == account)?;
                    for mut job in jobs.into_iter().filter(|job| {
                        job.event.as_deref() == Some(&event.id)
                            && matches!(job.run.state, RunState::WaitingBody | RunState::WaitingCategory)
                            && job.rule_binding.as_ref().is_some_and(|(rule, _, _)| !handled.contains(rule))
                    }) {
                        job.run.state = RunState::Paused;
                        job.run.reason = Some(RunReason::PermissionRevoked);
                        put_run(tx, &job, "paused")?;
                    }
                    Ok(())
                })?;
                if !waiting {
                    self.store
                        .transaction(|tx| store::put(tx, "event_ack", &event.id, "suite", &account, "", &true))?;
                    mail.acknowledge_workflow_event(&account, &event.id).await.map_err(|_| Error::Offline)?;
                }
            }
        }
        Ok(())
    }

    fn allowed(
        &self,
        job: &RunRecord,
        clients: &Clients,
        mail: &MailService,
        gate: &dyn Fn(&Scope) -> bool,
    ) -> Result<(), Error> {
        if !gate(&job.scope) || !gate(&job.execution_scope) {
            return Err(Error::Forbidden);
        }
        let request = Request::RunWorkflow {
            id: job.run.workflow.clone(),
            revision: job.run.revision,
            mail: job.run.mail.clone(),
            test: job.run.test,
            repeat: job.run.generation > 0,
        };
        let executing = job.execution_scope.current(clients)?;
        if executing.as_ref().is_some_and(|rights| {
            !rights.allows_request(&request)
                || !security::reads(Some(rights), &job.run.account)
                || !security::target(Some(rights), &job.run.mail, &job.run.save_target)
        }) {
            return Err(Error::Forbidden);
        }
        let rights = job.scope.current(clients)?;
        if !security::reads(rights.as_ref(), &job.run.account)
            || !security::target(rights.as_ref(), &job.run.mail, &job.run.save_target)
            || rights.as_ref().is_some_and(|rights| !rights.allows_request(&request))
        {
            return Err(Error::Forbidden);
        }
        let current = self.run_record(&job.scope, rights.as_ref(), &job.run.id)?;
        if current.run.state == RunState::Cancelled {
            return Err(Error::Forbidden);
        }
        let record = self.workflow(&job.scope, rights.as_ref(), &job.run.workflow)?;
        pinned(&record, job.run.revision)?;
        if record.execution_epoch != job.execution_epoch {
            return Err(Error::Forbidden);
        }
        if record.workflow.digest != job.workflow_digest
            || record.scope != job.scope && record.scope.carrier != job.scope.carrier
        {
            return Err(Error::Conflict);
        }
        if target_fingerprint(mail, &job.run.save_target)? != job.target_fingerprint {
            return Err(Error::Conflict);
        }
        if !job.run.test
            && (!record.workflow.enabled
                || !record.workflow.saved
                || !record.workflow.tested
                || (!job.one_time
                    && record.authorization.as_ref().is_none_or(|grant| {
                        grant.digest != job.workflow_digest
                            || grant.target_fingerprint != job.target_fingerprint
                            || grant.scope != record.scope
                    })))
        {
            return Err(Error::Forbidden);
        }
        if let Some((id, revision, state_revision)) = &job.rule_binding {
            let rule = self.rule(&job.execution_scope, executing.as_ref(), id)?;
            if !rule.rule.enabled
                || rule.rule.revision != *revision
                || rule.state_revision != *state_revision
                || rule.rule.spec.workflow != job.run.workflow
                || rule.rule.spec.account != job.run.account
            {
                return Err(Error::Forbidden);
            }
        }
        Ok(())
    }

    /// One claim per worker. The core starts exactly two workers; each holds its claim through converter completion.
    pub(crate) async fn work(
        &self,
        clients: &Clients,
        mail: &MailService,
        gate: &(dyn Fn(&Scope) -> bool + Sync),
    ) -> Result<bool, Error> {
        self.work_using(clients, mail, gate, &download::Downloader::system().map_err(|_| Error::Offline)?).await
    }

    async fn work_using(
        &self,
        clients: &Clients,
        mail: &MailService,
        gate: &(dyn Fn(&Scope) -> bool + Sync),
        downloader: &download::Downloader,
    ) -> Result<bool, Error> {
        let ready = self.store.view(|tx| {
            let records: Vec<RunRecord> = store::list(tx, "run", |_, _| true)?;
            Ok(records
                .into_iter()
                .find(|job| job.run.state == RunState::Queued && job.due <= Utc::now())
                .map(|job| job.run.id))
        })?;
        let Some(ready) = ready else { return Ok(false) };
        let job = self.store.transaction(|tx| {
            let running: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM records WHERE kind='run' AND state IN ('running','committing')",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| Error::Blocked)?;
            if running >= 2 {
                return Ok(None);
            }
            let mut job: RunRecord = store::get(tx, "run", &ready, |_, _| true)?;
            if job.run.state != RunState::Queued || job.due > Utc::now() {
                return Ok(None);
            }
            job.run.state = RunState::Running;
            job.run.attempts = job.run.attempts.saturating_add(1);
            put_run(tx, &job, "running")?;
            Ok(Some(job))
        })?;
        let Some(mut job) = job else { return Ok(false) };
        let cancelled = Arc::new(AtomicBool::new(false));
        self.running.lock().unwrap_or_else(PoisonError::into_inner).insert(job.run.id.clone(), Arc::clone(&cancelled));
        let outcome = self.execute(&mut job, clients, mail, gate, cancelled, downloader).await;
        self.running.lock().unwrap_or_else(PoisonError::into_inner).remove(&job.run.id);
        self.store.transaction(|tx| {
            let current: RunRecord = store::get(tx, "run", &job.run.id, |_, _| true)?;
            if current.run.state == RunState::Cancelled {
                return Ok(());
            }
            let outcome =
                if outcome.is_ok() && !fits(tx, &job)? { Err(failure(RunReason::PermissionRevoked)) } else { outcome };
            match outcome {
                Ok(()) => {
                    job.run.state = RunState::Succeeded;
                    job.run.reason = None;
                    if job.run.test {
                        let mut record: WorkflowRecord = store::get(tx, "workflow", &job.run.workflow, |_, _| true)?;
                        if record.workflow.digest == job.workflow_digest && record.workflow.revision == job.run.revision
                        {
                            record.workflow.tested = true;
                            record.state_revision = record.state_revision.checked_add(1).ok_or(Error::Conflict)?;
                            store::put(
                                tx,
                                "workflow",
                                &record.workflow.id,
                                record.scope.owner(),
                                &record.workflow.account,
                                "",
                                &record,
                            )?;
                        }
                    }
                    put_run(tx, &job, "succeeded")
                }
                Err(failure) if failure.retryable && job.run.attempts < 3 && job.reservation.is_none() => {
                    job.run.state = RunState::Queued;
                    job.run.reason = Some(failure.reason);
                    job.due = Utc::now() + chrono::Duration::seconds(if job.run.attempts == 1 { 5 } else { 30 });
                    put_run(tx, &job, "queued")
                }
                Err(failure) => {
                    job.run.reason = Some(failure.reason);
                    job.run.state = if matches!(
                        failure.reason,
                        RunReason::PermissionRevoked
                            | RunReason::RecoveryUncertain
                            | RunReason::Changed
                            | RunReason::SenderUnverified
                            | RunReason::AuthRequired
                    ) {
                        RunState::Paused
                    } else {
                        RunState::Failed
                    };
                    put_run(tx, &job, "failed")
                }
            }
        })?;
        Ok(true)
    }

    async fn execute(
        &self,
        job: &mut RunRecord,
        clients: &Clients,
        mail: &MailService,
        gate: &(dyn Fn(&Scope) -> bool + Sync),
        cancelled: Arc<AtomicBool>,
        downloader: &download::Downloader,
    ) -> Result<(), download::Failure> {
        let allow = || self.allowed(job, clients, mail, gate).map_err(failed_permission);
        allow()?;
        let raw = self.original(mail, &job.scope, clients, &job.run.mail).await.map_err(failed_permission)?;
        if raw_hash(&raw) != job.raw_hash {
            return Err(failure(RunReason::Changed));
        }
        if job.rule_binding.is_some() && job.verified_sender.is_none() {
            return Err(failure(RunReason::SenderUnverified));
        }
        let parsed = torro_mail_imap::mime::read(&raw).map_err(|_| failure(RunReason::Unsupported))?;
        let html = parsed.html.ok_or_else(|| failure(RunReason::Unsupported))?;
        let rights = job.scope.current(clients).map_err(failed_permission)?;
        let workflow = self.workflow(&job.scope, rights.as_ref(), &job.run.workflow).map_err(failed_permission)?;
        let output = retrieve(&workflow.workflow.spec.package, &html, downloader, &allow, cancelled).await?;
        allow()?;
        let stores = mail.stores(rights.as_ref().map_or(Reach::Suite, Reach::Client)).await;
        let located = read::locate(&stores, &job.run.mail).await.map_err(|_| failure(RunReason::NotFound))?;
        let summary = read::stored_message(&located).await.map_err(|_| failure(RunReason::NotFound))?.summary;
        let target = mail
            .save_targets()
            .get(&job.run.save_target)
            .map_err(|_| failure(RunReason::TargetUnavailable))?
            .ok_or_else(|| failure(RunReason::TargetUnavailable))?;
        let facts = FileFacts {
            mail_date: summary.date.with_timezone(&chrono::Local).date_naive(),
            file_name: Some("invoice.pdf".into()),
            content_type: "application/pdf".into(),
            sender_name: summary.from.as_ref().and_then(|from| from.name.clone()),
            sender_address: summary.from.as_ref().map(|from| from.address.clone()).unwrap_or_default(),
            account: save::account_name(mail, &job.run.account).await,
            subject: summary.subject,
            month_names: save::month_names(),
        };
        // Hash + exact reservation are committed before create_new and sync. No success is published yet.
        self.allowed(job, clients, mail, gate).map_err(failed_permission)?;
        let reservation = self
            .store
            .transaction(|tx| {
                let current: RunRecord = store::get(tx, "run", &job.run.id, |_, _| true)?;
                if current.run.state == RunState::Cancelled {
                    return Err(Error::Forbidden);
                }
                let jobs: Vec<RunRecord> = store::list(tx, "run", |_, _| true)?;
                let excluded: BTreeSet<String> = jobs
                    .into_iter()
                    .filter(|other| other.run.save_target == job.run.save_target)
                    .filter(blocks_reservation)
                    .filter_map(|other| other.reservation.map(|r| r.relative_path))
                    .collect();
                let reservation = save::reserve_workflow(&target.spec, &facts, &output.bytes, &excluded)
                    .map_err(|_| Error::Blocked)?;
                job.run.document = Some(wire::RetrievedDocument {
                    id: crate::mail::random_hex(16).map_err(|_| Error::Blocked)?,
                    save_target: job.run.save_target.clone(),
                    relative_path: reservation.relative_path.clone(),
                    sha256: digest(&output.bytes),
                    bytes: output.bytes.len() as u64,
                    provenance: output.provenance,
                    source_host: output.source_host,
                    profile_version: output.profile_version,
                    missing: false,
                    changed: false,
                });
                job.reservation = Some(reservation.clone());
                put_run(tx, job, "committing")?;
                Ok(reservation)
            })
            .map_err(failed_permission)?;
        self.allowed(job, clients, mail, gate).map_err(failed_permission)?;
        save::commit_workflow(&target.spec, &reservation, &output.bytes)
            .map_err(|_| failure(RunReason::RecoveryUncertain))?;
        self.allowed(job, clients, mail, gate).map_err(failed_permission)?;
        Ok(())
    }

    /// Interrupted downloads are not repeated. A journaled write is reconciled only by its exact path/hash.
    pub(crate) fn recover(
        &self,
        clients: &Clients,
        mail: &MailService,
        gate: &dyn Fn(&Scope) -> bool,
    ) -> Result<(), Error> {
        let interrupted = self.store.view(|tx| {
            let jobs: Vec<RunRecord> = store::list(tx, "run", |_, _| true)?;
            Ok(jobs.into_iter().filter(|job| job.run.state == RunState::Running).collect::<Vec<_>>())
        })?;
        for mut job in interrupted {
            let allowed = self.allowed(&job, clients, mail, gate).is_ok();
            let complete = allowed && job.run.document.as_ref().is_some_and(|doc| verified_path(mail, doc).is_ok());
            job.run.state = if complete { RunState::Succeeded } else { RunState::Paused };
            job.run.reason = if !allowed {
                Some(RunReason::PermissionRevoked)
            } else {
                (!complete).then_some(RunReason::RecoveryUncertain)
            };
            self.store.transaction(|tx| {
                let current: RunRecord = store::get(tx, "run", &job.run.id, |_, _| true)?;
                if current.run.state != RunState::Running {
                    return Ok(());
                }
                let complete = complete && fits(tx, &job)?;
                if !complete && job.run.state == RunState::Succeeded {
                    job.run.state = RunState::Paused;
                    job.run.reason = Some(RunReason::PermissionRevoked);
                }
                if complete && job.run.test {
                    let mut record: WorkflowRecord = store::get(tx, "workflow", &job.run.workflow, |_, _| true)?;
                    pinned(&record, job.run.revision)?;
                    if record.workflow.digest == job.workflow_digest && record.execution_epoch == job.execution_epoch {
                        record.workflow.tested = true;
                        record.state_revision = record.state_revision.checked_add(1).ok_or(Error::Conflict)?;
                        store::put(
                            tx,
                            "workflow",
                            &record.workflow.id,
                            record.scope.owner(),
                            &record.workflow.account,
                            "",
                            &record,
                        )?;
                    }
                }
                put_run(tx, &job, if complete { "succeeded" } else { "paused" })
            })?;
        }
        Ok(())
    }
}

fn put_run(tx: &rusqlite::Transaction<'_>, record: &RunRecord, state: &str) -> Result<(), Error> {
    store::put(tx, "run", &record.run.id, record.scope.owner(), &record.run.account, state, record)
}

fn blocks_reservation(record: &RunRecord) -> bool {
    // Only outstanding/uncertain writes own their path. Finished documents must reach the normal
    // conflict policy again (including SkipIdentical); historical deduplication remains untouched.
    record.reservation.is_some() && record.run.state != RunState::Succeeded
}

/// Recheck concurrently editable service state under the same transaction that publishes completion.
fn fits(tx: &rusqlite::Transaction<'_>, job: &RunRecord) -> Result<bool, Error> {
    let record: WorkflowRecord = match store::get(tx, "workflow", &job.run.workflow, |_, _| true) {
        Ok(record) => record,
        Err(Error::NotFound) => return Ok(false),
        Err(error) => return Err(error),
    };
    if pinned(&record, job.run.revision).is_err()
        || record.workflow.digest != job.workflow_digest
        || record.execution_epoch != job.execution_epoch
    {
        return Ok(false);
    }
    if !job.run.test
        && (!record.workflow.saved
            || !record.workflow.tested
            || !record.workflow.enabled
            || (!job.one_time
                && record.authorization.as_ref().is_none_or(|grant| {
                    grant.digest != job.workflow_digest
                        || grant.target_fingerprint != job.target_fingerprint
                        || grant.scope != record.scope
                })))
    {
        return Ok(false);
    }
    if let Some((id, revision, serial)) = &job.rule_binding {
        let rule: super::RuleRecord = match store::get(tx, "rule", id, |_, _| true) {
            Ok(rule) => rule,
            Err(Error::NotFound) => return Ok(false),
            Err(error) => return Err(error),
        };
        if !rule.rule.enabled || rule.rule.revision != *revision || rule.state_revision != *serial {
            return Ok(false);
        }
    }
    Ok(true)
}

fn dedup_key(account: &str, hash: &str, workflow: &str, target: &str) -> Result<String, Error> {
    Ok(digest(&serde_json::to_vec(&(account, hash, workflow, "invoice", target)).map_err(|_| Error::Blocked)?))
}

fn prior_dedup(tx: &rusqlite::Transaction<'_>, key: &str) -> Result<Option<Dedup>, Error> {
    match store::get(tx, "dedup", key, |_, _| true) {
        Ok(prior) => Ok(Some(prior)),
        Err(Error::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

fn event_job(event: &str, rule: &str) -> String {
    digest(format!("{event}\0{rule}").as_bytes())
}

fn failed_permission(error: Error) -> download::Failure {
    failure(match error {
        Error::Conflict => RunReason::Changed,
        Error::Offline => RunReason::Offline,
        Error::SenderUnverified => RunReason::SenderUnverified,
        _ => RunReason::PermissionRevoked,
    })
}
fn failure(reason: RunReason) -> download::Failure {
    download::Failure { reason, retryable: false }
}

struct Output {
    bytes: Vec<u8>,
    provenance: wire::Provenance,
    source_host: String,
    profile_version: Option<u16>,
}

async fn retrieve(
    package: &wire::Package,
    html: &str,
    downloader: &download::Downloader,
    allow: &(dyn Fn() -> Result<(), download::Failure> + Sync),
    cancelled: Arc<AtomicBool>,
) -> Result<Output, download::Failure> {
    package.validate().map_err(|_| failure(RunReason::Unsupported))?;
    let mut budget = download::Budget::new();
    let mut link = None;
    let mut body: Option<download::Body> = None;
    let mut provenance = wire::Provenance::SupplierPdf;
    let mut profile_version = None;
    for step in &package.steps {
        allow()?;
        match step {
            wire::Step::MailLink { host, label } => {
                link = Some(download::select(
                    html,
                    host,
                    label.as_deref(),
                    package.steps == wire::Package::direct_pdf(host).steps,
                )?);
            }
            wire::Step::Get => {
                body = Some(
                    downloader
                        .fetch(
                            link.take().ok_or_else(|| failure(RunReason::Unsupported))?,
                            &package.hosts,
                            &mut budget,
                            allow,
                        )
                        .await?,
                );
            }
            wire::Step::HtmlLink { host, label } => {
                let source = body.take().ok_or_else(|| failure(RunReason::Unsupported))?;
                let html = std::str::from_utf8(&source.bytes).map_err(|_| failure(RunReason::Unsupported))?;
                link = Some(download::select(html, host, Some(label), false)?);
            }
            wire::Step::ResolveStripeInvoicePdf { version: 1 } => {
                body = Some(
                    downloader
                        .resolve_stripe_invoice_pdf(
                            link.take().ok_or_else(|| failure(RunReason::Unsupported))?,
                            &package.hosts,
                            &mut budget,
                            allow,
                        )
                        .await?,
                );
            }
            wire::Step::RenderInvoiceCopy { version: 1, .. } => {
                let source = body.take().ok_or_else(|| failure(RunReason::Unsupported))?;
                if source
                    .content_type
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/pdf"))
                {
                    body = Some(source);
                } else {
                    let copy = invoice::convert(source.bytes, Some(source.content_type), Some(Arc::clone(&cancelled)))
                        .await
                        .map_err(|_| failure(RunReason::Unsupported))?;
                    if copy.pages == 0 || copy.pages > 50 {
                        return Err(failure(RunReason::Unsupported));
                    }
                    body = Some(download::Body {
                        bytes: copy.bytes,
                        content_type: "application/pdf".into(),
                        source_host: source.source_host,
                    });
                    provenance = wire::Provenance::HtmlCopy;
                    profile_version = Some(1);
                }
            }
            wire::Step::VerifyPdf => {
                let source = body.take().ok_or_else(|| failure(RunReason::UnexpectedType))?;
                if !source
                    .content_type
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/pdf"))
                {
                    return Err(failure(RunReason::UnexpectedType));
                }
                let bytes = tokio::task::spawn_blocking(move || verify_pdf(source.bytes))
                    .await
                    .map_err(|_| failure(RunReason::Unsupported))??;
                body =
                    Some(download::Body { bytes, content_type: source.content_type, source_host: source.source_host });
            }
            wire::Step::SaveToTarget => {}
            _ => return Err(failure(RunReason::Unsupported)),
        }
    }
    allow()?;
    let mut source = body.ok_or_else(|| failure(RunReason::UnexpectedType))?;
    source.bytes = verify_pdf(source.bytes)?;
    Ok(Output { bytes: source.bytes, provenance, source_host: source.source_host, profile_version })
}

fn verify_pdf(bytes: Vec<u8>) -> Result<Vec<u8>, download::Failure> {
    if bytes.len() > 50 * 1024 * 1024
        || !bytes.starts_with(b"%PDF-")
        || !bytes.windows(5).rev().take(1024).any(|slice| slice == b"%%EOF")
    {
        return Err(failure(RunReason::UnexpectedType));
    }
    bounded_pdf::validate(&bytes).map_err(|_| failure(RunReason::Unsupported))?;
    Ok(bytes)
}

fn verified_path(
    mail: &MailService,
    document: &wire::RetrievedDocument,
) -> Result<std::path::PathBuf, save::WorkflowSaveError> {
    let target = mail
        .save_targets()
        .get(&document.save_target)
        .map_err(|_| save::WorkflowSaveError::Unavailable)?
        .ok_or(save::WorkflowSaveError::Unavailable)?;
    save::verify_workflow_document(&target.spec, &document.relative_path, &document.sha256, document.bytes)
}
fn document_status(mail: &MailService, run: &mut wire::WorkflowRun) {
    if let Some(doc) = &mut run.document {
        match verified_path(mail, doc) {
            Ok(_) => {
                doc.missing = false;
                doc.changed = false;
            }
            Err(save::WorkflowSaveError::Unavailable) => doc.missing = true,
            Err(_) => doc.changed = true,
        }
    }
}

fn open_document(path: &std::path::Path, reveal: bool) -> Result<(), Error> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        if reveal {
            command.arg("-R");
        }
        command.arg(path);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(if reveal { path.parent().ok_or(Error::Invalid)? } else { path });
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("explorer.exe");
        if reveal {
            command.arg("/select,");
        }
        command.arg(path);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Err(Error::Unsupported);
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = command.spawn().map_err(|_| Error::Blocked)?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }
}

/// A bounded container check for classic cross-reference PDFs (the shipped invoice adapters' format).
/// It never decodes a stream, font or image. Unsupported compressed xref/object-stream variants fail closed;
/// in particular a small Flate bomb is never expanded in the service merely to validate a download.
mod bounded_pdf {
    use std::collections::{BTreeMap, BTreeSet};

    const MAX_OBJECTS: usize = 100_000;
    const MAX_TOKENS: usize = 1_000_000;
    type Id = (u32, u16);
    type Check<T> = Result<T, ()>;

    #[derive(Clone)]
    enum Value {
        Int(i64),
        Name(Vec<u8>),
        Array(Vec<Value>),
        Dict(BTreeMap<Vec<u8>, Value>),
        Ref(Id),
        Other,
    }
    impl Value {
        fn int(&self) -> Check<usize> {
            match self {
                Self::Int(n) => usize::try_from(*n).map_err(|_| ()),
                _ => Err(()),
            }
        }
        fn reference(&self) -> Check<Id> {
            match self {
                Self::Ref(id) => Ok(*id),
                _ => Err(()),
            }
        }
        fn dict(&self) -> Check<&BTreeMap<Vec<u8>, Value>> {
            match self {
                Self::Dict(dict) => Ok(dict),
                _ => Err(()),
            }
        }
    }
    fn get<'a>(dict: &'a BTreeMap<Vec<u8>, Value>, key: &[u8]) -> Check<&'a Value> {
        dict.get(key).ok_or(())
    }
    fn name(dict: &BTreeMap<Vec<u8>, Value>, key: &[u8], expected: &[u8]) -> bool {
        matches!(dict.get(key), Some(Value::Name(value)) if value == expected)
    }
    fn white(byte: u8) -> bool {
        matches!(byte, 0 | 9 | 10 | 12 | 13 | 32)
    }
    fn delimiter(byte: u8) -> bool {
        white(byte) || b"()<>[]{}/%".contains(&byte)
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        at: usize,
        tokens: usize,
    }
    impl<'a> Parser<'a> {
        fn skip(&mut self) {
            loop {
                while self.bytes.get(self.at).is_some_and(|b| white(*b)) {
                    self.at += 1;
                }
                if self.bytes.get(self.at) != Some(&b'%') {
                    break;
                }
                while self.bytes.get(self.at).is_some_and(|b| !matches!(*b, b'\r' | b'\n')) {
                    self.at += 1;
                }
            }
        }
        fn word(&mut self) -> Check<&'a [u8]> {
            self.skip();
            let start = self.at;
            while self.bytes.get(self.at).is_some_and(|b| !delimiter(*b)) {
                self.at += 1;
            }
            if start == self.at || self.at - start > 64 {
                return Err(());
            }
            self.bytes.get(start..self.at).ok_or(())
        }
        fn number(&mut self) -> Check<usize> {
            let token = self.word()?;
            if token.is_empty() || !token.iter().all(u8::is_ascii_digit) {
                return Err(());
            }
            std::str::from_utf8(token).map_err(|_| ())?.parse().map_err(|_| ())
        }
        fn keyword(&mut self, expected: &[u8]) -> Check<()> {
            if self.word()? == expected { Ok(()) } else { Err(()) }
        }
        fn value(&mut self, depth: usize) -> Check<Value> {
            self.tokens += 1;
            if depth > 64 || self.tokens > MAX_TOKENS {
                return Err(());
            }
            self.skip();
            match self.bytes.get(self.at).copied().ok_or(())? {
                b'/' => {
                    self.at += 1;
                    let mut value = Vec::new();
                    while let Some(byte) = self.bytes.get(self.at).copied().filter(|byte| !delimiter(*byte)) {
                        self.at += 1;
                        if byte == b'#' {
                            let pair = self.bytes.get(self.at..self.at + 2).ok_or(())?;
                            let hi = char::from(*pair.first().ok_or(())?).to_digit(16).ok_or(())?;
                            let lo = char::from(*pair.get(1).ok_or(())?).to_digit(16).ok_or(())?;
                            value.push((hi * 16 + lo) as u8);
                            self.at += 2;
                        } else {
                            value.push(byte);
                        }
                        if value.len() > 256 {
                            return Err(());
                        }
                    }
                    Ok(Value::Name(value))
                }
                b'(' => {
                    self.at += 1;
                    let mut nesting = 1usize;
                    while nesting > 0 {
                        let byte = *self.bytes.get(self.at).ok_or(())?;
                        self.at += 1;
                        match byte {
                            b'\\' => {
                                self.bytes.get(self.at).ok_or(())?;
                                self.at += 1;
                            }
                            b'(' => {
                                nesting += 1;
                                if nesting > 64 {
                                    return Err(());
                                }
                            }
                            b')' => nesting -= 1,
                            _ => {}
                        }
                    }
                    Ok(Value::Other)
                }
                b'<' if self.bytes.get(self.at + 1) == Some(&b'<') => {
                    self.at += 2;
                    let mut dict = BTreeMap::new();
                    loop {
                        self.skip();
                        if self.bytes.get(self.at..self.at + 2) == Some(b">>") {
                            self.at += 2;
                            break;
                        }
                        let Value::Name(key) = self.value(depth + 1)? else { return Err(()) };
                        let value = self.value(depth + 1)?;
                        if dict.insert(key, value).is_some() {
                            return Err(());
                        }
                    }
                    Ok(Value::Dict(dict))
                }
                b'<' => {
                    self.at += 1;
                    while let Some(byte) = self.bytes.get(self.at).copied().filter(|byte| *byte != b'>') {
                        if !white(byte) && !byte.is_ascii_hexdigit() {
                            return Err(());
                        }
                        self.at += 1;
                    }
                    if self.bytes.get(self.at) != Some(&b'>') {
                        return Err(());
                    }
                    self.at += 1;
                    Ok(Value::Other)
                }
                b'[' => {
                    self.at += 1;
                    let mut values = Vec::new();
                    loop {
                        self.skip();
                        if self.bytes.get(self.at) == Some(&b']') {
                            self.at += 1;
                            break;
                        }
                        values.push(self.value(depth + 1)?);
                    }
                    Ok(Value::Array(values))
                }
                _ => {
                    let word = self.word()?;
                    if matches!(word, b"true" | b"false" | b"null") {
                        return Ok(Value::Other);
                    }
                    let text = std::str::from_utf8(word).map_err(|_| ())?;
                    if let Ok(first) = text.parse::<i64>() {
                        let saved = self.at;
                        if let Ok(second) = self.number()
                            && self.keyword(b"R").is_ok()
                        {
                            return Ok(Value::Ref((
                                u32::try_from(first).map_err(|_| ())?,
                                u16::try_from(second).map_err(|_| ())?,
                            )));
                        }
                        self.at = saved;
                        Ok(Value::Int(first))
                    } else {
                        let number = text.parse::<f64>().map_err(|_| ())?;
                        if !number.is_finite()
                            || !text.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.'))
                        {
                            return Err(());
                        }
                        Ok(Value::Other)
                    }
                }
            }
        }
    }

    struct Document<'a> {
        parser: Parser<'a>,
        offsets: BTreeMap<u32, Option<(usize, u16)>>,
        values: BTreeMap<Id, Value>,
        loading: BTreeSet<Id>,
    }
    impl Document<'_> {
        fn object(&mut self, id: Id) -> Check<Value> {
            if let Some(value) = self.values.get(&id) {
                return Ok(value.clone());
            }
            if self.loading.len() > 64 || !self.loading.insert(id) {
                return Err(());
            }
            let (offset, generation) = self.offsets.get(&id.0).copied().flatten().ok_or(())?;
            if generation != id.1 {
                return Err(());
            }
            self.parser.at = offset;
            if self.parser.number()? != id.0 as usize || self.parser.number()? != usize::from(id.1) {
                return Err(());
            }
            self.parser.keyword(b"obj")?;
            let value = self.parser.value(0)?;
            self.parser.skip();
            let position = self.parser.at;
            let stream = self.parser.word()? == b"stream";
            if stream {
                let dict = value.dict()?;
                if name(dict, b"Type", b"ObjStm") || name(dict, b"Type", b"XRef") {
                    return Err(());
                }
                // A stream starts with exactly LF or CRLF, not arbitrary skipped whitespace/comments.
                match self.parser.bytes.get(self.parser.at..self.parser.at + 2) {
                    Some(b"\r\n") => self.parser.at += 2,
                    _ if self.parser.bytes.get(self.parser.at) == Some(&b'\n') => self.parser.at += 1,
                    _ => return Err(()),
                }
                let start = self.parser.at;
                let length = get(dict, b"Length")?;
                let length = match length {
                    Value::Ref(id) => self.object(*id)?.int()?,
                    value => value.int()?,
                };
                self.parser.at = start.checked_add(length).filter(|end| *end <= self.parser.bytes.len()).ok_or(())?;
                self.parser.keyword(b"endstream")?;
                self.parser.keyword(b"endobj")?;
            } else {
                self.parser.at = position;
                self.parser.keyword(b"endobj")?;
            }
            self.loading.remove(&id);
            self.values.insert(id, value.clone());
            Ok(value)
        }

        fn pages(&mut self, id: Id, parent: Option<Id>, seen: &mut BTreeSet<Id>, depth: usize) -> Check<usize> {
            if depth > 64 || seen.len() >= 10_000 || !seen.insert(id) {
                return Err(());
            }
            let value = self.object(id)?;
            let dict = value.dict()?;
            if let Some(parent) = parent
                && get(dict, b"Parent")?.reference()? != parent
            {
                return Err(());
            }
            if name(dict, b"Type", b"Page") {
                return Ok(1);
            }
            if !name(dict, b"Type", b"Pages") {
                return Err(());
            }
            let Value::Array(kids) = get(dict, b"Kids")? else { return Err(()) };
            let count = get(dict, b"Count")?.int()?;
            if count == 0 || count > 10_000 || kids.is_empty() {
                return Err(());
            }
            let mut actual = 0;
            for kid in kids {
                actual += self.pages(kid.reference()?, Some(id), seen, depth + 1)?;
            }
            if actual != count {
                return Err(());
            }
            Ok(actual)
        }
    }

    pub(super) fn validate(bytes: &[u8]) -> Check<()> {
        if bytes.len() > 50 * 1024 * 1024 || !bytes.starts_with(b"%PDF-") {
            return Err(());
        }
        let mut end = bytes.len();
        while bytes.get(end.saturating_sub(1)).is_some_and(|byte| white(*byte)) && end > 0 {
            end -= 1;
        }
        if end < 5 || bytes.get(end - 5..end) != Some(b"%%EOF") {
            return Err(());
        }
        let tail = bytes.get(end.saturating_sub(1024)..end - 5).ok_or(())?;
        let index = tail.windows(9).rposition(|part| part == b"startxref").ok_or(())?;
        let at = end.saturating_sub(1024) + index + 9;
        let mut document = Document {
            parser: Parser { bytes, at, tokens: 0 },
            offsets: BTreeMap::new(),
            values: BTreeMap::new(),
            loading: BTreeSet::new(),
        };
        let mut offset = document.parser.number()?;
        let mut visited = BTreeSet::new();
        let mut root = None;
        loop {
            if visited.len() >= 8 || !visited.insert(offset) || offset >= bytes.len() {
                return Err(());
            }
            document.parser.at = offset;
            document.parser.keyword(b"xref")?;
            let mut revision = BTreeSet::new();
            loop {
                document.parser.skip();
                let saved = document.parser.at;
                if document.parser.word()? == b"trailer" {
                    break;
                }
                document.parser.at = saved;
                let start = document.parser.number()?;
                let count = document.parser.number()?;
                if start.checked_add(count).is_none_or(|end| end > MAX_OBJECTS) {
                    return Err(());
                }
                for n in start..start + count {
                    if !revision.insert(n) {
                        return Err(());
                    }
                    let position = document.parser.number()?;
                    let generation = u16::try_from(document.parser.number()?).map_err(|_| ())?;
                    let live = match document.parser.word()? {
                        b"n" => Some((position, generation)),
                        b"f" => None,
                        _ => return Err(()),
                    };
                    document.offsets.entry(u32::try_from(n).map_err(|_| ())?).or_insert(live);
                }
            }
            let trailer = document.parser.value(0)?;
            let dict = trailer.dict()?;
            if dict.contains_key(b"Encrypt".as_slice())
                || dict.contains_key(b"XRefStm".as_slice())
                || get(dict, b"Size")?.int()? > MAX_OBJECTS
            {
                return Err(());
            }
            if root.is_none() {
                root = dict.get(b"Root".as_slice()).map(Value::reference).transpose()?;
            }
            let Some(previous) = dict.get(b"Prev".as_slice()) else { break };
            offset = previous.int()?;
        }
        let root = root.ok_or(())?;
        // Check all active indirect-object boundaries, including opaque bounded stream lengths, without expansion.
        let ids = document
            .offsets
            .iter()
            .filter_map(|(n, entry)| entry.map(|(_, generation)| (*n, generation)))
            .collect::<Vec<_>>();
        for id in ids {
            document.object(id)?;
        }
        let catalog = document.object(root)?;
        let catalog = catalog.dict()?;
        if !name(catalog, b"Type", b"Catalog") {
            return Err(());
        }
        document.pages(get(catalog, b"Pages")?.reference()?, None, &mut BTreeSet::new(), 0)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Environment {
        dir: tempfile::TempDir,
        mail: MailService,
        clients: Clients,
        service: Service,
        workflow: wire::Workflow,
    }

    async fn environment() -> Environment {
        use crate::mail::service::tests::{mail_account, service_in, store_accounts};
        use crate::mail::store::{Body, NewMessage};
        let dir = tempfile::tempdir().expect("isolated home");
        let secrets = crate::account::github::tests::MemoryTokens::default();
        // No mail worker is started. Every tested raw read comes from the synthetic local original.
        let account = mail_account(
            "workflow@example.org",
            &torro_mail::Endpoint { host: "127.0.0.1".into(), port: 9, security: torro_mail::Security::Tls },
        );
        store_accounts(dir.path(), &secrets, &[(&account, "unused")]);
        let check = Arc::new(crate::mail::check::SenderCheck::offline());
        check.know("torro._domainkey.example.org", &crate::mail::check::tests::key_record());
        let logos = Arc::new(crate::mail::logos::SenderLogos::new(None, Arc::new(crate::mail::service::tests::NoWeb)));
        let mail = service_in(dir.path(), &secrets).with_sender_logos(logos, check);
        mail.reload().await;
        let store = mail.store(&account.id).await.expect("local store");
        let message = store.run(|store| {
            let folder = store.upsert_folder("INBOX", "Inbox", Some(torro_mail::Role::Inbox))?;
            store.note_uidvalidity(&folder, 1)?;
            let id = store.upsert_message(&folder, 1, &NewMessage {
                id: None, uid: 1, rfc_message_id: Some("<workflow-synthetic@example.org>".into()),
                thread_id: "synthetic".into(), from: None, to: Vec::new(), cc: Vec::new(), reply_to: Vec::new(),
                subject: "Synthetic invoice".into(), date: Utc::now(), snippet: String::new(), flags: Vec::new(),
                has_attachments: false, traits: Default::default(), invitation: false,
            })?;
            let raw = b"From: billing@example.org\r\nSubject: Synthetic invoice\r\nMessage-ID: <workflow-synthetic@example.org>\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<a href='https://documents.example.org/invoice.pdf'>Invoice</a>";
            store.save_original("INBOX", 1, 1, Utc::now().fixed_offset(), &[], raw)?;
            store.set_body(&id, &Body { text: "Invoice".into(), from_html: true,
                html: Some("<a href='https://documents.example.org/invoice.pdf'>Invoice</a>".into()), attachments: Vec::new(), invite: None })?;
            Ok(id)
        }).await.expect("synthetic mail");
        let folder = dir.path().join("invoices");
        std::fs::create_dir(&folder).expect("existing human target");
        let target = mail
            .save_targets()
            .create(torro_mail::SaveTargetSpec::new("Invoices", folder.to_string_lossy()), &[])
            .expect("target");
        let clients = crate::clients::tests::open_in(dir.path());
        let service = Service::at(dir.path().join("workflow-journal/journal.sqlite"));
        let spec = wire::DraftSpec {
            name: "Invoice".into(),
            description: "Synthetic".into(),
            mail: message,
            save_target: target.id,
            package: wire::Package::direct_pdf("documents.example.org"),
        };
        let Response::WorkflowDraft { workflow } =
            service.answer(Request::CreateWorkflowDraft { spec }, Scope::suite(), &clients, &mail).await
        else {
            panic!("draft");
        };
        Environment { dir, mail, clients, service, workflow }
    }

    async fn queue(env: &Environment) -> wire::WorkflowRun {
        let Response::WorkflowRun { run } = env
            .service
            .answer(
                Request::RunWorkflow {
                    id: env.workflow.id.clone(),
                    revision: env.workflow.revision,
                    mail: env.workflow.spec.mail.clone(),
                    test: true,
                    repeat: false,
                },
                Scope::suite(),
                &env.clients,
                &env.mail,
            )
            .await
        else {
            panic!("queued");
        };
        run
    }

    async fn automatic_rule(env: &Environment) -> wire::WorkflowRule {
        // Isolate event plumbing from the download test; real successful-test persistence is tested above.
        env.service
            .store
            .transaction(|tx| {
                let mut record: WorkflowRecord = store::get(tx, "workflow", &env.workflow.id, |_, _| true)?;
                record.workflow.tested = true;
                store::put(
                    tx,
                    "workflow",
                    &record.workflow.id,
                    record.scope.owner(),
                    &record.workflow.account,
                    "",
                    &record,
                )
            })
            .expect("isolated tested fixture");
        for request in [
            Request::SaveWorkflow { id: env.workflow.id.clone(), revision: 1 },
            Request::AuthorizeWorkflow { id: env.workflow.id.clone(), revision: 1 },
        ] {
            assert!(matches!(
                env.service.answer(request, Scope::suite(), &env.clients, &env.mail).await,
                Response::Workflow { .. }
            ));
        }
        let spec = wire::RuleSpec {
            workflow: env.workflow.id.clone(),
            account: env.workflow.account.clone(),
            trigger: wire::Trigger::NewMail,
            query: "invoice".into(),
            category: None,
        };
        let Response::WorkflowRule { rule } =
            env.service.answer(Request::CreateWorkflowRule { spec }, Scope::suite(), &env.clients, &env.mail).await
        else {
            panic!("rule");
        };
        let Response::WorkflowRule { rule } = env
            .service
            .answer(
                Request::SetWorkflowRuleEnabled { id: rule.id, revision: 1, enabled: true },
                Scope::suite(),
                &env.clients,
                &env.mail,
            )
            .await
        else {
            panic!("enabled rule");
        };
        rule
    }

    async fn arrival(env: &Environment, uid: u32, age: i64, ready: bool) -> String {
        use crate::mail::store::{Body, NewMessage};
        let store = env.mail.store(&env.workflow.account).await.expect("local store");
        store
            .run(move |store| {
                let folder = store.upsert_folder("INBOX", "Inbox", Some(torro_mail::Role::Inbox))?;
                store.note_workflow_inventory(&folder, 1, 1)?;
                store.complete_workflow_inventory(&folder, 1)?;
                let now = Utc::now();
                let id = store.import_workflow_message_received(
                    &folder,
                    1,
                    &NewMessage {
                        id: None,
                        uid,
                        rfc_message_id: Some(format!("<arrival-{uid}@example.org>")),
                        thread_id: format!("arrival-{uid}"),
                        from: None,
                        to: Vec::new(),
                        cc: Vec::new(),
                        reply_to: Vec::new(),
                        subject: "New synthetic invoice".into(),
                        date: now,
                        snippet: String::new(),
                        flags: Vec::new(),
                        has_attachments: false,
                        traits: Default::default(),
                        invitation: false,
                    },
                    now,
                    Some(now - chrono::Duration::days(age)),
                )?;
                let raw = format!("From: billing@example.org\r\nTo: workflow@example.org\r\nSubject: New synthetic invoice\r\nMessage-ID: <arrival-{uid}@example.org>\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<a href='https://documents.example.org/invoice.pdf'>Invoice</a>");
                store.save_original("INBOX", 1, uid, now.fixed_offset(), &[], raw.as_bytes())?;
                if ready {
                    store.set_body(
                        &id,
                        &Body {
                            text: "Invoice".into(),
                            from_html: true,
                            html: Some("<a href='https://documents.example.org/invoice.pdf'>Invoice</a>".into()),
                            attachments: Vec::new(),
                            invite: None,
                        },
                    )?;
                    store.workflow_body_prepared(&id)?;
                    store.workflow_category_prepared(&id)?;
                }
                Ok(id)
            })
            .await
            .expect("arrival transaction")
    }

    #[test]
    fn raw_dedup_normalizes_line_endings_but_keeps_all_content() {
        assert_eq!(raw_hash(b"From: a\nSubject: b\n\nbody\n"), raw_hash(b"From: a\r\nSubject: b\r\n\r\nbody\r\n"));
        assert_ne!(raw_hash(b"Message-ID: same\n\none"), raw_hash(b"Message-ID: same\n\ntwo"));
    }

    #[test]
    fn damaged_or_empty_pdfs_do_not_become_documents() {
        for bytes in [b"not PDF".as_slice(), b"%PDF-1.4\n%%EOF".as_slice()] {
            assert!(verify_pdf(bytes.to_vec()).is_err());
        }
    }

    fn supplier_pdf() -> Vec<u8> {
        let mut doc = printpdf::PdfDocument::new("Synthetic invoice");
        doc.pages.push(printpdf::PdfPage::new(printpdf::Mm(210.0), printpdf::Mm(297.0), Vec::new()));
        doc.save(&printpdf::PdfSaveOptions::default(), &mut Vec::new())
    }

    #[tokio::test]
    async fn product_stripe_path_uses_only_the_supplied_invoice_file_and_checks_its_structure() {
        let server = httpmock::MockServer::start_async().await;
        let data = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/invoicedata.stripe.com/invoice_pdf_file_url/acct_synthetic/token_synthetic");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(r#"{"file_url":"https://stripe-upload-api.s3.us-west-1.amazonaws.com/invoice.pdf"}"#);
            })
            .await;
        let pdf = supplier_pdf();
        assert!(verify_pdf(pdf.clone()).is_ok(), "the bounded checker accepts the supplier container");
        let file = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/stripe-upload-api.s3.us-west-1.amazonaws.com/invoice.pdf");
                then.status(200).header("content-type", "application/pdf").body(pdf.clone());
            })
            .await;
        let downloader = download::Downloader::for_test(&server.base_url());
        let package = super::super::templates::get("stripe_invoice").expect("built-in").package;
        let html = "<a href='https://invoice.stripe.com/i/acct_synthetic/token_synthetic'>Invoice</a>";
        let result = retrieve(&package, html, &downloader, &|| Ok(()), Arc::new(AtomicBool::new(false)))
            .await
            .expect("product path");
        assert_eq!(result.bytes, pdf);
        assert_eq!(result.provenance, wire::Provenance::SupplierPdf);
        data.assert_calls_async(1).await;
        file.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn a_valid_package_cannot_skip_the_final_pdf_check_or_bypass_revocation() {
        let server = httpmock::MockServer::start_async().await;
        let broken = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/my.paddle.com/synthetic");
                then.status(200).header("content-type", "application/pdf").body("%PDF-1.4\n%%EOF");
            })
            .await;
        let downloader = download::Downloader::for_test(&server.base_url());
        let mut package = super::super::templates::get("paddle_setapp_invoice_copy").expect("built-in").package;
        package.steps.retain(|step| !matches!(step, wire::Step::VerifyPdf));
        assert!(package.validate().is_ok(), "legacy packages still enter the unconditional container check");
        let html = "<a href='https://my.paddle.com/synthetic'>Invoice</a>";
        assert!(retrieve(&package, html, &downloader, &|| Ok(()), Arc::new(AtomicBool::new(false))).await.is_err());
        broken.assert_calls_async(1).await;
        assert!(
            retrieve(
                &package,
                html,
                &downloader,
                &|| Err(failure(RunReason::PermissionRevoked)),
                Arc::new(AtomicBool::new(false))
            )
            .await
            .is_err()
        );
        broken.assert_calls_async(1).await;
    }

    #[test]
    fn classic_pdf_containers_are_checked_without_expanding_any_stream() {
        let pdf = supplier_pdf();
        assert!(bounded_pdf::validate(&pdf).is_ok());
        let mut changed = pdf.clone();
        let at = changed.windows(9).position(|part| part == b"startxref").expect("xref marker");
        if let Some(byte) = changed.get_mut(at + 10) {
            *byte = b'9';
        }
        assert!(bounded_pdf::validate(&changed).is_err());
        assert!(bounded_pdf::validate(b"%PDF-1.4\nstartxref\n0\n%%EOF").is_err());
    }

    #[tokio::test]
    async fn durable_test_download_is_deduplicated_across_workers_and_restart() {
        let env = environment().await;
        let queued = queue(&env).await;
        let server = httpmock::MockServer::start_async().await;
        let pdf = supplier_pdf();
        let file = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/documents.example.org/invoice.pdf");
                then.status(200).header("content-type", "application/pdf").body(pdf.clone());
            })
            .await;
        let downloader = download::Downloader::for_test(&server.base_url());
        let (a, b) = tokio::join!(
            env.service.work_using(&env.clients, &env.mail, &|_| true, &downloader),
            env.service.work_using(&env.clients, &env.mail, &|_| true, &downloader)
        );
        assert_eq!(usize::from(a.expect("worker one")) + usize::from(b.expect("worker two")), 1);
        file.assert_calls_async(1).await;
        let Response::WorkflowRun { run } =
            env.service.get_run(&Scope::suite(), None, &queued.id, &env.mail).expect("run")
        else {
            panic!("run");
        };
        assert_eq!(run.state, RunState::Succeeded);
        let doc = run.document.expect("retrieved document");
        assert_eq!(doc.sha256, digest(&pdf));
        assert!(env.service.workflow(&Scope::suite(), None, &env.workflow.id).expect("workflow").workflow.tested);
        let again = queue(&env).await;
        assert_eq!(again.id, queued.id, "tests and later manual runs share durable deduplication");
        let restarted = Service::at(env.dir.path().join("workflow-journal/journal.sqlite"));
        restarted.recover(&env.clients, &env.mail, &|_| true).expect("recover");
        let response = restarted
            .document(
                Request::GetWorkflowDocument { id: doc.id.clone(), offset: 0, limit: 262144 },
                &Scope::suite(),
                None,
                &env.mail,
            )
            .expect("verified chunk");
        let Response::WorkflowDocument { base64, eof, .. } = response else {
            panic!("chunk");
        };
        assert!(eof);
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(base64).expect("base64"), pdf);
        let target = env.mail.save_targets().get(&doc.save_target).expect("target").expect("target");
        std::fs::remove_file(std::path::Path::new(&target.spec.folder).join(&doc.relative_path))
            .expect("simulate missing document");
        assert!(
            restarted
                .document(
                    Request::GetWorkflowDocument { id: doc.id, offset: 0, limit: 262144 },
                    &Scope::suite(),
                    None,
                    &env.mail
                )
                .is_err()
        );
        assert!(
            !restarted
                .work_using(&env.clients, &env.mail, &|_| true, &downloader)
                .await
                .expect("no implicit redownload")
        );
        file.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn retries_are_durable_bounded_and_cancelled_jobs_never_fetch() {
        let env = environment().await;
        let queued = queue(&env).await;
        let server = httpmock::MockServer::start_async().await;
        let offline = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/documents.example.org/invoice.pdf");
                then.status(503).header("content-type", "text/html");
            })
            .await;
        let downloader = download::Downloader::for_test(&server.base_url());
        for attempt in 1..=3 {
            env.service.work_using(&env.clients, &env.mail, &|_| true, &downloader).await.expect("bounded attempt");
            let record = env.service.run_record(&Scope::suite(), None, &queued.id).expect("durable run");
            assert_eq!(record.run.attempts, attempt);
            assert_eq!(record.run.state, if attempt < 3 { RunState::Queued } else { RunState::Failed });
            assert_eq!(record.run.reason, Some(RunReason::Offline));
            if attempt < 3 {
                let delay = (record.due - Utc::now()).num_seconds();
                assert!((if attempt == 1 { 3..=5 } else { 28..=30 }).contains(&delay));
                env.service
                    .store
                    .transaction(|tx| {
                        let mut record = record.clone();
                        record.due = Utc::now();
                        put_run(tx, &record, "queued")
                    })
                    .expect("advance clock");
            }
        }
        offline.assert_calls_async(3).await;
        let env = environment().await;
        let queued = queue(&env).await;
        env.service.cancel_run(&Scope::suite(), None, &queued.id).expect("cancelled");
        assert!(!env.service.work_using(&env.clients, &env.mail, &|_| true, &downloader).await.expect("no claim"));
        offline.assert_calls_async(3).await;
    }

    #[tokio::test]
    async fn dynamic_command_denial_and_executing_behalf_are_both_enforced() {
        use torro_ipc::api::{AreaRights, Level, RightsTemplate};
        let env = environment().await;
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.areas.insert(Area::Workflows, AreaRights { level: Level::Full, admin: true });
        let (owner, _) =
            env.clients.add("Synthetic owner", torro_ipc::api::MCP_CLIENT, rights.clone()).await.expect("test owner");
        let (assistant, _) = env
            .clients
            .add("Synthetic assistant", torro_ipc::api::MCP_CLIENT, rights.clone())
            .await
            .expect("test behalf");
        let scope =
            Scope { carrier: Some(owner.id.clone()), origin: Default::default(), ceiling: Some(rights.clone()) };
        let Response::WorkflowDraft { workflow } = env
            .service
            .answer(
                Request::CreateWorkflowDraft { spec: env.workflow.spec.clone() },
                scope.clone(),
                &env.clients,
                &env.mail,
            )
            .await
        else {
            panic!("client draft");
        };
        let mut executing = scope.clone();
        executing.origin.behalf = Some(assistant.id.clone());
        let request = Request::RunWorkflow {
            id: workflow.id.clone(),
            revision: 1,
            mail: workflow.spec.mail.clone(),
            test: true,
            repeat: false,
        };
        let (fingerprint, _) =
            env.service.approval(&request, &executing, &env.clients, &env.mail).await.expect("test approval");
        let Response::WorkflowRun { run } =
            env.service.answer_approved(request, executing, &env.clients, &env.mail, Some(&fingerprint)).await
        else {
            panic!("approved run");
        };
        let job = env.service.run_record(&scope, Some(&rights), &run.id).expect("job");
        assert!(env.service.allowed(&job, &env.clients, &env.mail, &|_| true).is_ok());
        rights.denied.insert("workflows.run".into());
        env.clients.update(&owner.id, rights.clone()).await.expect("revoke one command only");
        assert_eq!(env.service.allowed(&job, &env.clients, &env.mail, &|_| true), Err(Error::Forbidden));
        rights.denied.remove("workflows.run");
        env.clients.update(&owner.id, rights).await.expect("restore owner");
        env.clients.remove(&assistant.id).await.expect("behalf removed");
        assert_eq!(env.service.allowed(&job, &env.clients, &env.mail, &|_| true), Err(Error::Forbidden));
    }

    #[tokio::test]
    async fn event_handoff_revisits_waits_and_pauses_unverified_mail_without_any_download() {
        let env = environment().await;
        automatic_rule(&env).await;
        let message = arrival(&env, 2, 0, false).await;
        env.service.ingest(&env.clients, &env.mail, &|_| true).await.expect("durable waiting handoff");
        let Response::WorkflowRuns { runs } =
            env.service.list_runs(&Scope::suite(), None, None, Some(&message), &env.mail).expect("visible wait")
        else {
            panic!("runs");
        };
        assert_eq!(runs.len(), 1);
        assert_eq!(runs.first().expect("wait").state, RunState::WaitingBody);
        let store = env.mail.store(&env.workflow.account).await.expect("store");
        let lookup = message.clone();
        store
            .run(move |store| {
                store.set_body(
                    &lookup,
                    &crate::mail::store::Body {
                        text: "Invoice".into(),
                        from_html: false,
                        html: None,
                        attachments: Vec::new(),
                        invite: None,
                    },
                )?;
                store.workflow_body_prepared(&lookup)?;
                store.workflow_category_prepared(&lookup)
            })
            .await
            .expect("source transitions in place");
        env.service.ingest(&env.clients, &env.mail, &|_| true).await.expect("same event revisited");
        let Response::WorkflowRuns { runs } =
            env.service.list_runs(&Scope::suite(), None, None, Some(&message), &env.mail).expect("manual review")
        else {
            panic!("runs");
        };
        assert_eq!(runs.len(), 1, "wait and pause retain one stable run id");
        assert_eq!(runs.first().expect("pause").state, RunState::Paused);
        assert_eq!(runs.first().expect("pause").reason, Some(RunReason::SenderUnverified));
        assert!(
            env.mail
                .workflow_events(&env.workflow.account, None, 100)
                .await
                .expect("acknowledged after journal")
                .is_empty()
        );
        env.service.ingest(&env.clients, &env.mail, &|_| true).await.expect("replay harmless");
        let old = arrival(&env, 3, 8, true).await;
        env.service.ingest(&env.clients, &env.mail, &|_| true).await.expect("bounded catch-up");
        let Response::WorkflowRuns { runs } =
            env.service.list_runs(&Scope::suite(), None, None, Some(&old), &env.mail).expect("old mail visible")
        else {
            panic!("runs");
        };
        assert_eq!(runs.first().expect("skip").state, RunState::Skipped);
        assert_eq!(
            runs.first().expect("skip").reason,
            Some(RunReason::TooOld),
            "IMAP receipt age wins over a fresh sender Date header"
        );
    }

    #[tokio::test]
    async fn repeat_approval_pins_the_previous_generation_inside_the_enqueue_transaction() {
        use torro_ipc::api::{AreaRights, Level, RightsTemplate};
        let env = environment().await;
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.areas.insert(Area::Workflows, AreaRights { level: Level::Full, admin: true });
        let (client, _) = env
            .clients
            .add("Synthetic repeat client", torro_ipc::api::MCP_CLIENT, rights.clone())
            .await
            .expect("test client");
        let scope = Scope { carrier: Some(client.id), origin: Default::default(), ceiling: Some(rights) };
        let Response::WorkflowDraft { workflow } = env
            .service
            .answer(
                Request::CreateWorkflowDraft { spec: env.workflow.spec.clone() },
                scope.clone(),
                &env.clients,
                &env.mail,
            )
            .await
        else {
            panic!("draft");
        };
        env.service
            .store
            .transaction(|tx| {
                let mut record: WorkflowRecord = store::get(tx, "workflow", &workflow.id, |_, _| true)?;
                record.workflow.tested = true;
                record.workflow.saved = true;
                record.workflow.enabled = true;
                store::put(tx, "workflow", &workflow.id, record.scope.owner(), &record.workflow.account, "", &record)
            })
            .expect("manual-reuse fixture without a lasting grant");
        let request = Request::RunWorkflow {
            id: workflow.id.clone(),
            revision: 1,
            mail: workflow.spec.mail.clone(),
            test: false,
            repeat: false,
        };
        let (pin, _) =
            env.service.approval(&request, &scope, &env.clients, &env.mail).await.expect("initial manual approval");
        let Response::WorkflowRun { run } =
            env.service.answer_approved(request, scope.clone(), &env.clients, &env.mail, Some(&pin)).await
        else {
            panic!("queued manual run");
        };
        let complete = |id: &str| {
            env.service.store.transaction(|tx| {
                let mut job: RunRecord = store::get(tx, "run", id, |_, _| true)?;
                job.run.state = RunState::Succeeded;
                put_run(tx, &job, "succeeded")
            })
        };
        complete(&run.id).expect("completed generation zero fixture");
        let repeated = Request::RunWorkflow {
            id: workflow.id.clone(),
            revision: 1,
            mail: workflow.spec.mail.clone(),
            test: false,
            repeat: true,
        };
        let (pin, _) = env
            .service
            .approval(&repeated, &scope, &env.clients, &env.mail)
            .await
            .expect("two approvals for the same previous generation");
        let Response::WorkflowRun { run } =
            env.service.answer_approved(repeated, scope.clone(), &env.clients, &env.mail, Some(&pin)).await
        else {
            panic!("generation one");
        };
        assert_eq!(run.generation, 1);
        complete(&run.id).expect("completed first repeat");
        assert_eq!(
            env.service
                .enqueue(&scope, &env.clients, &env.mail, &workflow.id, 1, &workflow.spec.mail, false, true, Some(&pin))
                .await
                .err(),
            Some(Error::Conflict),
            "a stale approval cannot create generation two even after generation one finishes"
        );
    }

    #[tokio::test]
    async fn crash_recovery_reconciles_only_the_exact_journaled_path_and_hash() {
        for complete in [true, false] {
            let env = environment().await;
            let queued = queue(&env).await;
            let mut job = env.service.run_record(&Scope::suite(), None, &queued.id).expect("job");
            let target = env.mail.save_targets().get(&queued.save_target).expect("target").expect("target");
            let bytes = supplier_pdf();
            let facts = FileFacts {
                mail_date: Utc::now().date_naive(),
                file_name: Some("invoice.pdf".into()),
                content_type: "application/pdf".into(),
                sender_name: None,
                sender_address: "billing@example.org".into(),
                account: queued.account.clone(),
                subject: "Synthetic invoice".into(),
                month_names: save::month_names(),
            };
            let reservation = save::reserve_workflow(&target.spec, &facts, &bytes, &BTreeSet::new()).expect("reserve");
            job.run.state = RunState::Running;
            job.run.document = Some(wire::RetrievedDocument {
                id: "synthetic-document".into(),
                save_target: queued.save_target,
                relative_path: reservation.relative_path.clone(),
                sha256: digest(&bytes),
                bytes: bytes.len() as u64,
                provenance: wire::Provenance::SupplierPdf,
                source_host: "documents.example.org".into(),
                profile_version: None,
                missing: false,
                changed: false,
            });
            job.reservation = Some(reservation.clone());
            env.service.store.transaction(|tx| put_run(tx, &job, "committing")).expect("journal before create_new");
            if complete {
                save::commit_workflow(&target.spec, &reservation, &bytes).expect("file synced before crash");
            } else {
                std::fs::write(
                    std::path::Path::new(&target.spec.folder).join(&reservation.relative_path),
                    &bytes[..bytes.len() / 2],
                )
                .expect("simulate interrupted write");
            }
            let restarted = Service::at(env.dir.path().join("workflow-journal/journal.sqlite"));
            restarted.recover(&env.clients, &env.mail, &|_| true).expect("reconcile");
            let recovered = restarted.run_record(&Scope::suite(), None, &queued.id).expect("recovered run");
            assert_eq!(recovered.run.state, if complete { RunState::Succeeded } else { RunState::Paused });
            assert_eq!(recovered.run.reason, (!complete).then_some(RunReason::RecoveryUncertain));
            assert_eq!(recovered.reservation.expect("same reservation").relative_path, reservation.relative_path);
            assert_eq!(
                restarted.workflow(&Scope::suite(), None, &env.workflow.id).expect("workflow").workflow.tested,
                complete
            );
            assert!(
                !restarted
                    .store
                    .view(|tx| {
                        let jobs: Vec<RunRecord> = store::list(tx, "run", |_, _| true)?;
                        Ok(jobs.iter().any(|job| job.run.state == RunState::Queued))
                    })
                    .expect("no retries of uncertain writes")
            );
        }
    }

    #[tokio::test]
    async fn verified_automatic_jobs_share_dedup_across_rules_and_bind_the_exact_signed_raw_mail() {
        let env = environment().await;
        automatic_rule(&env).await;
        automatic_rule(&env).await;
        let message = arrival(&env, 2, 0, true).await;
        let store = env.mail.store(&env.workflow.account).await.expect("store");
        let id = message.clone();
        let raw = store
            .run(move |store| {
                let place = store.resolve_message(&id)?.expect("place");
                let original = store.original_at(&place)?.expect("original");
                store.original_bytes(original.seq)
            })
            .await
            .expect("raw read")
            .expect("raw bytes");
        let signed = crate::mail::check::tests::sign(&raw, "example.org", false);
        assert_eq!(env.mail.workflow_verified_sender(&signed).await.as_deref(), Some("example.org"));
        let id = message.clone();
        store
            .run(move |store| {
                store.save_original("INBOX", 1, 2, Utc::now().fixed_offset(), &[], &signed)?;
                store.set_sender_check(&id, Some("example.org"))
            })
            .await
            .expect("exact verified original");
        env.service.ingest(&env.clients, &env.mail, &|_| true).await.expect("automatic handoff");
        let Response::WorkflowRuns { runs } =
            env.service.list_runs(&Scope::suite(), None, None, Some(&message), &env.mail).expect("runs")
        else {
            panic!("runs");
        };
        assert_eq!(runs.iter().filter(|run| run.state == RunState::Queued).count(), 1);
        assert_eq!(runs.iter().filter(|run| run.reason == Some(RunReason::Duplicate)).count(), 1);
        assert!(env.mail.workflow_events(&env.workflow.account, None, 100).await.expect("ack").is_empty());
        let server = httpmock::MockServer::start_async().await;
        let pdf = supplier_pdf();
        let download = server
            .mock_async(|when, then| {
                when.path("/documents.example.org/invoice.pdf");
                then.status(200).header("content-type", "application/pdf").body(pdf);
            })
            .await;
        env.service
            .work_using(&env.clients, &env.mail, &|_| true, &download::Downloader::for_test(&server.base_url()))
            .await
            .expect("signed automatic run");
        download.assert_calls_async(1).await;
        let unsigned = arrival(&env, 3, 0, true).await;
        let lookup = unsigned.clone();
        store
            .run(move |store| store.set_sender_check(&lookup, Some("example.org")))
            .await
            .expect("simulate stale cached sender verdict");
        env.service.ingest(&env.clients, &env.mail, &|_| true).await.expect("must verify exact current bytes");
        let Response::WorkflowRuns { runs } =
            env.service.list_runs(&Scope::suite(), None, None, Some(&unsigned), &env.mail).expect("pauses")
        else {
            panic!("runs");
        };
        assert!(
            runs.iter().all(|run| run.state == RunState::Paused && run.reason == Some(RunReason::SenderUnverified))
        );
        download.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn editing_a_workflow_atomically_invalidates_all_rule_activations() {
        let env = environment().await;
        let first = automatic_rule(&env).await;
        let second = automatic_rule(&env).await;
        let request = Request::SetWorkflowRuleEnabled { id: first.id.clone(), revision: first.revision, enabled: true };
        let (old_pin, _) =
            env.service.approval(&request, &Scope::suite(), &env.clients, &env.mail).await.expect("old activation pin");
        let mut spec = env.workflow.spec.clone();
        spec.name = "Edited invoice".into();
        let response = env
            .service
            .answer(
                Request::UpdateWorkflowDraft { id: env.workflow.id.clone(), revision: 1, spec },
                Scope::suite(),
                &env.clients,
                &env.mail,
            )
            .await;
        assert!(
            matches!(response, Response::WorkflowDraft { workflow } if workflow.revision == 2 && !workflow.tested && !workflow.authorized)
        );
        for id in [&first.id, &second.id] {
            let rule = env.service.rule(&Scope::suite(), None, id).expect("referencing rule");
            assert!(!rule.rule.enabled);
            assert_eq!(rule.rule.activated, None);
            assert_eq!(rule.state_revision, 2, "old in-flight rule bindings no longer match");
        }
        let stale = env.service.answer_approved(request, Scope::suite(), &env.clients, &env.mail, Some(&old_pin)).await;
        assert!(matches!(stale, Response::Error { code, .. } if code == "conflict"));
        env.service
            .store
            .transaction(|tx| {
                let mut workflow: WorkflowRecord = store::get(tx, "workflow", &env.workflow.id, |_, _| true)?;
                workflow.workflow.tested = true;
                store::put(
                    tx,
                    "workflow",
                    &workflow.workflow.id,
                    workflow.scope.owner(),
                    &workflow.workflow.account,
                    "",
                    &workflow,
                )
            })
            .expect("successful second revision test fixture");
        for request in [
            Request::SaveWorkflow { id: env.workflow.id.clone(), revision: 2 },
            Request::AuthorizeWorkflow { id: env.workflow.id.clone(), revision: 2 },
        ] {
            assert!(matches!(
                env.service.answer(request, Scope::suite(), &env.clients, &env.mail).await,
                Response::Workflow { .. }
            ));
        }
        assert!(
            !env.service.rule(&Scope::suite(), None, &first.id).expect("still off after reauthorization").rule.enabled
        );
    }

    #[tokio::test]
    async fn completed_reservations_reuse_skip_identical_and_existing_account_folder_semantics() {
        let mut env = environment().await;
        let target = env.mail.save_targets().get(&env.workflow.spec.save_target).expect("target").expect("target");
        let mut spec = target.spec.clone();
        spec.subfolders = "{account}".into();
        spec.on_conflict = torro_mail::save_target::OnConflict::SkipIdentical;
        env.mail.save_targets().update(&target.id, spec, &[]).expect("target scheme");
        let Response::WorkflowDraft { workflow } = env
            .service
            .answer(
                Request::CreateWorkflowDraft { spec: env.workflow.spec.clone() },
                Scope::suite(),
                &env.clients,
                &env.mail,
            )
            .await
        else {
            panic!("fresh pinned draft");
        };
        env.workflow = workflow;
        let first = queue(&env).await;
        let server = httpmock::MockServer::start_async().await;
        let pdf = supplier_pdf();
        let file = server
            .mock_async(|when, then| {
                when.path("/documents.example.org/invoice.pdf");
                then.status(200).header("content-type", "application/pdf").body(pdf);
            })
            .await;
        let downloader = download::Downloader::for_test(&server.base_url());
        assert!(env.service.work_using(&env.clients, &env.mail, &|_| true, &downloader).await.expect("first write"));
        let mail = arrival(&env, 2, 0, true).await;
        let Response::WorkflowRun { run: second } = env
            .service
            .answer(
                Request::RunWorkflow { id: env.workflow.id.clone(), revision: 1, mail, test: true, repeat: false },
                Scope::suite(),
                &env.clients,
                &env.mail,
            )
            .await
        else {
            panic!("distinct raw mail");
        };
        assert!(
            env.service.work_using(&env.clients, &env.mail, &|_| true, &downloader).await.expect("identical write")
        );
        let first = env.service.run_record(&Scope::suite(), None, &first.id).expect("first result");
        let second = env.service.run_record(&Scope::suite(), None, &second.id).expect("second result");
        assert_eq!(first.run.state, RunState::Succeeded);
        assert_eq!(second.run.state, RunState::Succeeded);
        assert!(second.reservation.as_ref().expect("reservation").skipped);
        let path = &first.run.document.as_ref().expect("first document").relative_path;
        assert_eq!(path, &second.run.document.as_ref().expect("second document").relative_path);
        let expected_account = save::account_name(&env.mail, &env.workflow.account).await;
        assert!(!expected_account.starts_with(torro_mail::ACCOUNT_PREFIX));
        assert_eq!(std::path::Path::new(path).parent(), Some(std::path::Path::new(&expected_account)));
        let mut uncertain = first.clone();
        uncertain.run.state = RunState::Paused;
        uncertain.run.reason = Some(RunReason::RecoveryUncertain);
        assert!(blocks_reservation(&uncertain), "uncertain paths stay reserved");
        assert!(!blocks_reservation(&first), "complete documents go through the target's content policy");
        file.assert_calls_async(2).await;
    }
}
