//! The assistant's long-running jobs (issue #961, "Langlebige Aufträge" in `docs/assistent.md`): started on purpose
//! with `assistant.start_job`, run as answers of the model in their conversation, journaled in
//! [`super::journal`], and resumed safely after a restart ([`Assistant::recover_jobs`]).
//!
//! The runtime is the core's, the model only decides the next step. Rules, in the order the code follows them:
//!
//! - **Explicit:** only `assistant.start_job` makes a job; a normal message runs as it always did, with no journal.
//! - **One model:** the job keeps the model it started with. When that one does not answer or is off, the job
//!   pauses ([`JobReason::ModelUnavailable`]); another model only on an explicit `assistant.resume_job` with
//!   `engine` and `model`, never by itself, so no data goes to another provider unasked.
//! - **Journal around every call:** before a call reaches the core it is written down and handed over (`planned →
//!   dispatched`, only while the job runs), and the core's answer settles it on the task that ran it
//!   ([`settle`]), so a stop of the answer meanwhile still records it. A call already confirmed in the job is not
//!   run again ([`super::model`]'s gate).
//! - **Checked anew:** every call goes through the core with the rights, approvals and tools of now; the gate pauses
//!   the job when the client is gone or its rights no longer cover the call ([`JobReason::Forbidden`]).
//! - **Restart:** a step handed over without a known outcome is looked up where the core can (its approval);
//!   otherwise one that only reads failed (`interrupted`) and may run again, one that changes something (sending a
//!   mail, the terminal) is `uncertain` and never runs again by itself: the job pauses until a person decides
//!   ([`Assistant::resolve_job_step`]). An approval still open stays open: nothing is taken as approved.
//! - **Pause, stop, cancel:** `assistant.pause_job` and a stop of the answer (`assistant.cancel`) pause the job,
//!   `assistant.cancel_job` ends it; either way no further step starts, and a step that changes something and was
//!   running becomes `uncertain` until its outcome comes in.

use std::sync::Arc;

use chrono::Utc;
use torro_core::i18n::{tr, trf};
use torro_ipc::api::{Access, Area, Operation};
use torro_ipc::assistant::{EngineKind, EntryContent, JobInfo, JobReason, JobState, StepInfo, StepState};
use torro_ipc::{ApprovalState, Response};

use super::journal::{Journal, JournalError, NewJob};
use super::model::Outcome;
use super::{Actor, Assistant, Message, busy, failure, refusal, store, unknown};
use crate::account::ai::AiAccounts;

/// The job an answer belongs to: its id and the journal.
#[derive(Clone)]
pub(crate) struct JobLink {
    pub id: String,
    pub journal: Arc<Journal>,
}

/// What the gate of a job's call decided ([`super::model`]).
pub(super) enum Gate {
    /// Hand the call to the core as step `seq`.
    Run(u32),
    /// Do not run it; the call answers this (`None`: the answer ends).
    Answer(Option<String>),
}

/// How a job starts or goes on in [`Assistant::send_with`].
pub(super) enum JobStart {
    /// A new job, written down in the same step as its first message.
    New(NewJob),
    /// The queued job `id` goes on; `automatic` after a restart, so its message is no word of the person's.
    Continue { id: String, automatic: bool },
}

/// Runs `work` on the journal on a blocking thread.
pub(super) async fn on_journal<T: Send + 'static>(
    journal: &Arc<Journal>,
    work: impl FnOnce(&Journal) -> Result<T, JournalError> + Send + 'static,
) -> Result<T, JournalError> {
    let journal = Arc::clone(journal);
    tokio::task::spawn_blocking(move || work(&journal)).await.unwrap_or(Err(JournalError::Blocked))
}

/// [`on_journal`] for the job of `link`.
pub(super) async fn on<T: Send + 'static>(
    link: &JobLink,
    work: impl FnOnce(&Journal) -> Result<T, JournalError> + Send + 'static,
) -> Result<T, JournalError> {
    on_journal(&link.journal, work).await
}

/// Whether a call of `command` that the core checks by `operation` may run again without harm: it only reads.
/// The terminal never counts, whatever its line does.
pub(super) fn repeatable(operation: Operation, command: &str) -> bool {
    operation.access == Access::Read && !torro_ipc::tools::is_terminal_command(command)
}

/// Records the core's answer `response` to step `seq`: waiting for a person, failed with its code, or done. A step
/// already settled otherwise (a pause marked it) keeps that, unless it was uncertain: then this is its outcome.
pub(super) async fn settle(link: &JobLink, seq: u32, response: &Response) {
    let id = link.id.clone();
    let settled = match response {
        Response::Pending { approval } => {
            let approval = approval.id.clone();
            on(link, move |journal| journal.awaits(&id, seq, &approval, Utc::now())).await
        }
        Response::Error { code, .. } => {
            let code = code.clone();
            on(link, move |journal| journal.step_to(&id, seq, StepState::Failed, Some(&code), Utc::now()).map(|_| ()))
                .await
        }
        _ => {
            on(link, move |journal| journal.step_to(&id, seq, StepState::Confirmed, None, Utc::now()).map(|_| ())).await
        }
    };
    note(settled, "keep the outcome of a step of");
}

/// Records how the approval of step `seq` ended ([`Outcome`]); the job runs again unless it was paused meanwhile.
pub(super) async fn decided(link: &JobLink, seq: u32, outcome: &Outcome) {
    match outcome {
        Outcome::Answered(response) => settle(link, seq, response).await,
        Outcome::Denied | Outcome::Expired => {
            let code = if matches!(outcome, Outcome::Denied) { "denied" } else { "expired" };
            let id = link.id.clone();
            let failed = on(link, move |journal| {
                journal.step_to(&id, seq, StepState::Failed, Some(code), Utc::now()).map(|_| ())
            })
            .await;
            note(failed, "keep the outcome of a step of");
        }
    }
    let id = link.id.clone();
    note(on(link, move |journal| journal.decided(&id, Utc::now())).await, "go on with");
}

/// Keeps that the tool line of step `seq` is the conversation's entry `entry`.
pub(super) async fn keep_result(link: &JobLink, seq: u32, entry: u64) {
    let id = link.id.clone();
    note(on(link, move |journal| journal.set_result(&id, seq, entry)).await, "keep the result of a step of");
}

/// Ends the answer's part of the job: `state` (paused, failed, completed) with `reason` and `message`, unless the
/// job was paused or ended meanwhile.
pub(super) async fn end(link: &JobLink, state: JobState, reason: Option<JobReason>, message: Option<String>) {
    let id = link.id.clone();
    let open = [JobState::Queued, JobState::Running, JobState::AwaitingApproval];
    let moved = on(link, move |journal| {
        journal.move_job(&id, &open, state, reason, message.as_deref(), Utc::now()).map(|_| ())
    })
    .await;
    // Someone paused or cancelled it first: that stands.
    if !matches!(moved, Err(JournalError::Conflict(_))) {
        note(moved, "change the state of");
    }
}

/// Names a journal that cannot be used on stderr; a conflict is expected (someone was first) and says nothing.
fn note<T>(result: Result<T, JournalError>, what: &str) {
    if let Err(JournalError::Blocked) = result {
        eprintln!("torroworkplace: cannot {what} a job of the assistant: its journal cannot be used");
    }
}

/// The answer for a journal error: `not-found`, `conflict` or `error`.
fn journal_refusal(error: JournalError) -> Response {
    match error {
        JournalError::NotFound => refusal("not-found", "no such job"),
        JournalError::Conflict(why) => refusal("conflict", &why),
        JournalError::Blocked => failure("the journal of the jobs cannot be used"),
    }
}

/// The message that lets a job go on: its goal and what is done, so the model does not do it again.
fn continuation(goal: &str, steps: &[StepInfo]) -> String {
    let done: Vec<String> = steps
        .iter()
        .filter(|step| step.state == StepState::Confirmed)
        .map(|step| format!("{} {}", step.seq, step.tool))
        .collect();
    let done = if done.is_empty() { tr("nothing yet").to_string() } else { done.join(", ") };
    trf(
        "Go on with this job where it stopped, and do not repeat what is already done.\n\nGoal: {goal}\nDone so far: \
         {done}",
        &[("goal", &goal), ("done", &done)],
    )
}

impl Assistant {
    /// `assistant.start_job`: checks the engine (an AI account), the conversation and the model, then writes the
    /// goal as the conversation's message and the job as `running` in one step ([`Assistant::send_with`]).
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn start_job(
        self: &Arc<Self>,
        ai: &AiAccounts,
        goal: String,
        engine: String,
        model: Option<String>,
        conversation: Option<String>,
        without: Vec<Area>,
        from: Option<String>,
    ) -> Response {
        if let Some(refused) = self.ai_engine_refusal(ai, &engine).await {
            return refused;
        }
        self.launch_job(goal, engine, model, conversation, without, from).await
    }

    /// `unsupported` (or `not-found`) unless `engine` is an AI account: a job runs with one only.
    async fn ai_engine_refusal(&self, ai: &AiAccounts, engine: &str) -> Option<Response> {
        match self.engine(ai, engine, &Actor::Assistant).await {
            Ok(EngineKind::Ai) => None,
            Ok(_) => Some(refusal("unsupported", "a job runs with an AI account only")),
            Err(refused) => Some(refused),
        }
    }

    /// [`Assistant::start_job`] once `engine` is known to be an AI account.
    pub(super) async fn launch_job(
        self: &Arc<Self>,
        goal: String,
        engine: String,
        model: Option<String>,
        conversation: Option<String>,
        without: Vec<Area>,
        from: Option<String>,
    ) -> Response {
        let Some(host) = self.host() else {
            return refusal("unsupported", "the assistant runs jobs once Torro Workplace has started");
        };
        if let Some(id) = &conversation {
            {
                let state = self.state().await;
                match state.conversations.get(id) {
                    None => return unknown(id),
                    Some(found) if found.bot.is_some() || found.about.is_some() => {
                        return refusal(
                            "unsupported",
                            "a job runs in a conversation of the assistant, without a bot or a mail",
                        );
                    }
                    Some(_) if state.running.contains_key(id) => return busy(),
                    Some(_) => {}
                }
            }
            let looked = id.clone();
            match on_journal(&self.journal, move |journal| journal.open_in(&looked)).await {
                Ok(None) => {}
                Ok(Some(_)) => return refusal("conflict", "this conversation has a job that is not over"),
                Err(error) => return journal_refusal(error),
            }
        }
        // The model is chosen now and kept: the job never takes another by itself.
        let access = match host.model(&engine, model.as_deref()).await {
            Ok(access) => access,
            Err(error) => return refusal(error.code(), &error.to_string()),
        };
        if model.as_deref().is_some_and(|named| named != access.model) {
            return refusal("unsupported", "that model is not on; turn it on or choose another");
        }
        let id = match store::new_id() {
            Ok(id) => id,
            Err(message) => return failure(&message),
        };
        let new = NewJob {
            id: id.clone(),
            conversation: String::new(),
            goal: goal.clone(),
            owner: from.clone(),
            engine: engine.clone(),
            model: access.model.clone(),
            without: without.clone(),
        };
        let message = Message {
            conversation,
            engine,
            model: Some(access.model),
            effort: None,
            text: goal,
            without,
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            on_behalf: None,
            from,
            spoken: false,
            voice: None,
        };
        match self.send_with(message, None, EngineKind::Ai, Utc::now(), None, Some(JobStart::New(new))).await {
            Response::Conversation { .. } => self.get_job(&id).await,
            other => other,
        }
    }

    /// `assistant.get_job`.
    pub async fn get_job(&self, id: &str) -> Response {
        let id = id.to_string();
        match on_journal(&self.journal, move |journal| journal.get(&id)).await {
            Ok((job, steps)) => Response::Job { job, steps },
            Err(error) => journal_refusal(error),
        }
    }

    /// `assistant.list_jobs`.
    pub async fn list_jobs(&self, conversation: Option<String>) -> Response {
        match on_journal(&self.journal, move |journal| journal.list(conversation.as_deref())).await {
            Ok(jobs) => Response::Jobs { jobs },
            Err(error) => journal_refusal(error),
        }
    }

    /// The job of `conversation` that is not over and not paused, when there is one.
    pub(super) async fn active_job(&self, conversation: &str) -> Option<JobInfo> {
        let looked = conversation.to_string();
        on_journal(&self.journal, move |journal| journal.open_in(&looked))
            .await
            .ok()
            .flatten()
            .filter(|job| job.state != JobState::Paused)
    }

    /// `assistant.pause_job`: pauses the job `id` for `reason` and stops its answer; a step that changes something
    /// and was running becomes `uncertain`.
    pub async fn pause_job(&self, id: &str, reason: JobReason) -> Response {
        let message = match reason {
            JobReason::Stopped => tr("The answer was stopped, so the job is paused. Resume it to go on."),
            _ => tr("The job is paused. Resume it to go on."),
        };
        let open = [JobState::Queued, JobState::Running, JobState::AwaitingApproval];
        self.halt(id, &open, JobState::Paused, reason, message).await
    }

    /// `assistant.cancel_job`: ends the job `id` for good and stops its answer.
    pub async fn cancel_job(&self, id: &str) -> Response {
        let open = [JobState::Queued, JobState::Running, JobState::AwaitingApproval, JobState::Paused];
        self.halt(id, &open, JobState::Cancelled, JobReason::PausedByRequest, tr("The job was cancelled.")).await
    }

    /// Moves the job `id` from one of `from` to `to` first, so no further step starts, then stops its answer and
    /// settles the steps it interrupted ([`Assistant::interrupt_steps`]).
    async fn halt(&self, id: &str, from: &[JobState], to: JobState, reason: JobReason, message: &str) -> Response {
        let (looked, from, text) = (id.to_string(), from.to_vec(), message.to_string());
        let reason = (to == JobState::Paused).then_some(reason);
        let moved = on_journal(&self.journal, move |journal| {
            journal.move_job(&looked, &from, to, reason, Some(&text), Utc::now())
        })
        .await;
        let job = match moved {
            Ok(job) => job,
            Err(error) => return journal_refusal(error),
        };
        if let Some(Response::Error { message, .. }) = self.stop(&job.conversation, None, Utc::now()).await {
            eprintln!("torroworkplace: cannot stop the answer of a job: {message}");
        }
        let unclear = self.interrupt_steps(&job.id).await;
        if to == JobState::Paused && unclear {
            self.explain(&job.id, JobReason::Uncertain, &uncertain_message()).await;
        }
        self.get_job(&job.id).await
    }

    /// Settles the steps of the job `id` an interruption left open: planned ones and running ones that only read
    /// failed (`interrupted`), running ones that change something become `uncertain`. Whether one is now.
    async fn interrupt_steps(&self, id: &str) -> bool {
        let looked = id.to_string();
        let settled = on_journal(&self.journal, move |journal| {
            let (_, steps) = journal.get(&looked)?;
            let mut unclear = false;
            for step in steps {
                let now = Utc::now();
                match step.state {
                    StepState::Planned => {
                        journal.step_to(&looked, step.seq, StepState::Failed, Some("interrupted"), now)?;
                    }
                    StepState::Dispatched if step.repeatable => {
                        journal.step_to(&looked, step.seq, StepState::Failed, Some("interrupted"), now)?;
                    }
                    StepState::Dispatched => {
                        // Settled meanwhile on the task that ran it: then that is its outcome.
                        match journal.step_to(&looked, step.seq, StepState::Uncertain, None, now) {
                            Ok(_) => unclear = true,
                            Err(JournalError::Conflict(_)) => {}
                            Err(error) => return Err(error),
                        }
                    }
                    StepState::Uncertain => unclear = true,
                    StepState::Confirmed | StepState::Failed => {}
                }
            }
            Ok(unclear)
        })
        .await;
        match settled {
            Ok(unclear) => unclear,
            Err(error) => {
                note::<()>(Err(error), "settle the steps of");
                false
            }
        }
    }

    /// Says why the paused job `id` waits: `reason` and `message`.
    async fn explain(&self, id: &str, reason: JobReason, message: &str) {
        let (id, message) = (id.to_string(), message.to_string());
        note(on_journal(&self.journal, move |journal| journal.explain(&id, reason, &message)).await, "explain");
    }

    /// `assistant.resolve_job_step`: a person says whether the uncertain step `step` of the job `id` happened.
    pub async fn resolve_job_step(&self, id: &str, step: u32, done: bool) -> Response {
        let looked = id.to_string();
        let resolved = on_journal(&self.journal, move |journal| {
            journal.resolve(&looked, step, done, Utc::now())?;
            let (job, steps) = journal.get(&looked)?;
            let unclear = steps.iter().any(|step| step.state == StepState::Uncertain);
            if job.state == JobState::Paused && job.reason == Some(JobReason::Uncertain) && !unclear {
                let message = tr("The unclear step was decided. Resume the job to go on.");
                journal.explain(&looked, JobReason::PausedByRequest, message)?;
            }
            Ok(())
        })
        .await;
        match resolved {
            Ok(()) => self.get_job(id).await,
            Err(error) => journal_refusal(error),
        }
    }

    /// Looks up the approval of each uncertain step of the job `id` (issue #961): decided, that is its outcome; still
    /// open, the job waits (`Err` with the step). Nothing is taken as approved.
    async fn reconcile(&self, link: &JobLink) -> Result<Vec<StepInfo>, StepInfo> {
        let host = self.host();
        let looked = link.id.clone();
        let steps = on(link, move |journal| journal.get(&looked)).await.map(|(_, steps)| steps).unwrap_or_default();
        for step in steps.iter().filter(|step| step.state == StepState::Uncertain) {
            let (Some(host), Some(approval)) = (&host, &step.approval) else { continue };
            // Gone or not the assistant's: the outcome stays unknown.
            if let Response::Approval { approval } = host.approval_as(&Actor::Assistant, approval).await {
                match approval.state {
                    ApprovalState::Pending | ApprovalState::Running => return Err(step.clone()),
                    ApprovalState::Done { response } => settle(link, step.seq, &response).await,
                    ApprovalState::Denied => decided(link, step.seq, &Outcome::Denied).await,
                    ApprovalState::Expired => decided(link, step.seq, &Outcome::Expired).await,
                }
            }
        }
        let looked = link.id.clone();
        Ok(on(link, move |journal| journal.get(&looked)).await.map(|(_, steps)| steps).unwrap_or_default())
    }

    /// `assistant.resume_job`: looks up what it can of the uncertain steps, checks the model (the job's, or the one
    /// given on purpose), then claims the job (`paused → queued`, so of two resumes one wins) and lets it go on.
    pub(super) async fn resume_job(
        self: &Arc<Self>,
        ai: &AiAccounts,
        id: &str,
        engine: Option<String>,
        model: Option<String>,
        from: Option<String>,
    ) -> Response {
        if let Some(engine) = &engine
            && let Some(refused) = self.ai_engine_refusal(ai, engine).await
        {
            return refused;
        }
        self.go_on_with(id, engine, model, from).await
    }

    /// [`Assistant::resume_job`] once an `engine` given is known to be an AI account.
    pub(super) async fn go_on_with(
        self: &Arc<Self>,
        id: &str,
        engine: Option<String>,
        model: Option<String>,
        from: Option<String>,
    ) -> Response {
        let Some(host) = self.host() else {
            return refusal("unsupported", "the assistant runs jobs once Torro Workplace has started");
        };
        let looked = id.to_string();
        let mut job = match on_journal(&self.journal, move |journal| journal.job(&looked)).await {
            Ok(job) => job,
            Err(error) => return journal_refusal(error),
        };
        if job.state != JobState::Paused {
            return refusal("conflict", &format!("the job is {}", job.state.as_str()));
        }
        let link = JobLink { id: job.id.clone(), journal: Arc::clone(&self.journal) };
        let steps = match self.reconcile(&link).await {
            Ok(steps) => steps,
            Err(step) => {
                self.explain(&job.id, JobReason::ApprovalPending, &approval_message()).await;
                return refusal(
                    "conflict",
                    &format!("step {} ({}) still waits for a person's approval", step.seq, step.tool),
                );
            }
        };
        if let Some(step) = steps.iter().find(|step| step.state == StepState::Uncertain) {
            self.explain(&job.id, JobReason::Uncertain, &uncertain_message()).await;
            return refusal(
                "conflict",
                &format!(
                    "step {} ({}) may or may not have happened; check it, then decide it with resolve_job_step",
                    step.seq, step.tool
                ),
            );
        }
        // Another model only on purpose; the job's own only when it is still the one that answers.
        let chosen = engine.is_some();
        let (engine, model) = match engine {
            Some(engine) => (engine, model),
            None => (job.engine.clone(), Some(job.model.clone())),
        };
        let access = match host.model(&engine, model.as_deref()).await {
            Ok(access) => access,
            Err(error) => {
                if !chosen {
                    let message = trf(
                        "The model {model} is not available ({code}). The job is paused: resume it once the model is \
                         back, or resume it with another model.",
                        &[("model", &job.model), ("code", &error.code())],
                    );
                    self.explain(&job.id, JobReason::ModelUnavailable, &message).await;
                }
                return refusal(error.code(), &error.to_string());
            }
        };
        if model.as_deref().is_some_and(|named| named != access.model) {
            return refusal(
                "unsupported",
                "that model is not on; the job does not go on with another by itself: resume it with an engine and \
                 model",
            );
        }
        if chosen {
            let (looked, engine_chosen, model) = (job.id.clone(), engine.clone(), access.model.clone());
            let engine = engine_chosen.clone();
            if let Err(error) =
                on_journal(&self.journal, move |journal| journal.choose_model(&looked, &engine, &model, Utc::now()))
                    .await
            {
                return journal_refusal(error);
            }
            (job.engine, job.model) = (engine_chosen, access.model);
        }
        {
            let state = self.state().await;
            if !state.conversations.contains_key(&job.conversation) {
                drop(state);
                let looked = job.id.clone();
                let gone = tr("The conversation of this job is gone.");
                let _ = on_journal(&self.journal, move |journal| {
                    journal.move_job(&looked, &[JobState::Paused], JobState::Cancelled, None, Some(gone), Utc::now())
                })
                .await;
                return unknown(&job.conversation);
            }
            if state.running.contains_key(&job.conversation) {
                return busy();
            }
        }
        let looked = job.id.clone();
        let claimed = on_journal(&self.journal, move |journal| {
            journal.move_job(&looked, &[JobState::Paused], JobState::Queued, None, None, Utc::now())
        })
        .await;
        if let Err(error) = claimed {
            return journal_refusal(error);
        }
        self.go_on(&job, &steps, from, false).await
    }

    /// Lets the queued job `job` go on in its conversation with its continuation ([`continuation`]): `from` is who
    /// asked, `automatic` after a restart. Should it not start, the job is paused again with the reason.
    async fn go_on(
        self: &Arc<Self>,
        job: &JobInfo,
        steps: &[StepInfo],
        from: Option<String>,
        automatic: bool,
    ) -> Response {
        let looked = job.id.clone();
        let without = on_journal(&self.journal, move |journal| journal.without(&looked)).await.unwrap_or_default();
        let message = Message {
            conversation: Some(job.conversation.clone()),
            engine: job.engine.clone(),
            model: Some(job.model.clone()),
            effort: None,
            text: continuation(&job.goal, steps),
            without,
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            on_behalf: None,
            from,
            spoken: false,
            voice: None,
        };
        let start = JobStart::Continue { id: job.id.clone(), automatic };
        match self.send_with(message, None, EngineKind::Ai, Utc::now(), None, Some(start)).await {
            Response::Conversation { .. } => self.get_job(&job.id).await,
            other => {
                let (looked, text) = (job.id.clone(), tr("The job could not go on. Resume it to try again."));
                let _ = on_journal(&self.journal, move |journal| {
                    journal.move_job(
                        &looked,
                        &[JobState::Queued],
                        JobState::Paused,
                        Some(JobReason::Error),
                        Some(text),
                        Utc::now(),
                    )
                })
                .await;
                other
            }
        }
    }

    /// Picks up the jobs a restart interrupted (issue #961), once the core runs: settles their steps
    /// ([`Assistant::interrupt_steps`] after looking up their approvals) and lets each go on, unless a step is
    /// unclear or waits for a person: then the job pauses and its conversation says why. A job whose conversation
    /// is gone is cancelled.
    pub async fn recover_jobs(self: &Arc<Self>) {
        let jobs = match on_journal(&self.journal, Journal::unfinished).await {
            Ok(jobs) => jobs,
            Err(error) => return note::<()>(Err(error), "pick up"),
        };
        for job in jobs {
            if !self.state().await.conversations.contains_key(&job.conversation) {
                let looked = job.id.clone();
                let open = [JobState::Queued, JobState::Running, JobState::AwaitingApproval];
                let gone = tr("The conversation of this job is gone.");
                let moved = on_journal(&self.journal, move |journal| {
                    journal.move_job(&looked, &open, JobState::Cancelled, None, Some(gone), Utc::now())
                })
                .await;
                note(moved, "cancel");
                continue;
            }
            // Handed over before the restart: an approval may tell how it ended; else it is interrupted.
            let unclear = self.interrupt_steps(&job.id).await;
            let link = JobLink { id: job.id.clone(), journal: Arc::clone(&self.journal) };
            let reconciled = if unclear { self.reconcile(&link).await } else { Ok(Vec::new()) };
            let paused = match reconciled {
                Err(_) => Some((JobReason::ApprovalPending, approval_message())),
                Ok(steps) if steps.iter().any(|step| step.state == StepState::Uncertain) => {
                    Some((JobReason::Uncertain, uncertain_message()))
                }
                Ok(_) => None,
            };
            let looked = job.id.clone();
            if let Some((reason, message)) = paused {
                let open = [JobState::Queued, JobState::Running, JobState::AwaitingApproval];
                let text = message.clone();
                let moved = on_journal(&self.journal, move |journal| {
                    journal.move_job(&looked, &open, JobState::Paused, Some(reason), Some(&text), Utc::now())
                })
                .await;
                note(moved, "pause");
                let mut state = self.state().await;
                let entry = EntryContent::Error { code: "paused".into(), message };
                self.add_now(&mut state, &job.conversation, entry).await;
                continue;
            }
            if job.state != JobState::Queued {
                let open = [JobState::Running, JobState::AwaitingApproval];
                let moved = on_journal(&self.journal, move |journal| {
                    journal.move_job(&looked, &open, JobState::Queued, None, None, Utc::now())
                })
                .await;
                if moved.is_err() {
                    note(moved, "queue");
                    continue;
                }
            }
            let looked = job.id.clone();
            let steps = on_journal(&self.journal, move |journal| journal.get(&looked))
                .await
                .map(|(_, steps)| steps)
                .unwrap_or_default();
            // Its owner's job goes on in their name; no word of the person's (a restart is no message of theirs).
            if let Response::Error { message, .. } = self.go_on(&job, &steps, job.owner.clone(), true).await {
                eprintln!("torroworkplace: cannot go on with a job of the assistant after the restart: {message}");
            }
        }
    }

    /// Writes down the job `start` for `conversation` (a new one) or lets the queued one run: `running` before its
    /// first message is written, so the job and the message never part.
    pub(super) async fn begin_job(&self, start: JobStart, conversation: &str) -> Result<JobLink, Response> {
        let conversation = conversation.to_string();
        let begun = on_journal(&self.journal, move |journal| {
            let now = Utc::now();
            let id = match start {
                JobStart::New(mut new) => {
                    new.conversation = conversation;
                    journal.create(&new, now)?;
                    new.id
                }
                JobStart::Continue { id, .. } => id,
            };
            journal.move_job(&id, &[JobState::Queued], JobState::Running, None, None, now)?;
            Ok(id)
        })
        .await;
        match begun {
            Ok(id) => Ok(JobLink { id, journal: Arc::clone(&self.journal) }),
            Err(error) => Err(journal_refusal(error)),
        }
    }
}

/// Why a job waits after a step with an unclear outcome, for people.
fn uncertain_message() -> String {
    tr("A step of this job changes something, and it is not known whether it happened. It does not run again by \
        itself: check it, mark it as done or not done, then resume the job.")
    .to_string()
}

/// Why a job waits after a restart while an approval is open, for people.
fn approval_message() -> String {
    tr("A command of this job still waits for approval. Nothing runs until it is decided and the job is resumed.")
        .to_string()
}

#[cfg(test)]
mod tests;
