//! Long-running jobs in the chat (issue #962, technology of #961, "Langlebige Aufträge" in `docs/assistent.md`):
//! what the window decides about a job apart from drawing, kept pure so it can be tested on its own.
//!
//! - Starting one on purpose: the composer's switch "Run as job" makes "Send" ask `assistant.start_job` instead of
//!   `assistant.send_message` ([`start_request`]). A normal message never becomes a job.
//! - The job of the open conversation ([`JobView`]): the newest of its jobs, with its steps; the card under the
//!   history shows its state and reason and offers what fits the state ([`JobView::can_pause`],
//!   [`JobView::can_resume`], [`JobView::uncertain`], [`JobView::wants_model`]).
//! - The requests of its buttons ([`JobAction::request`]), each only ever sent for a person's click; deciding an
//!   unclear step (`assistant.resolve_job_step`) is the person's alone.

use std::collections::HashMap;

use torro_core::i18n::tr;
use torro_ipc::Request;
use torro_ipc::api::Area;
use torro_ipc::assistant::{JobInfo, JobReason, JobState, StepInfo, StepState};

use crate::chat::Choice;

/// The job the open conversation shows: the newest of its jobs and its steps in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobView {
    pub job: JobInfo,
    pub steps: Vec<StepInfo>,
}

impl JobView {
    /// The job still runs or waits: "Stop" in the composer pauses it, and no second job starts here.
    pub fn is_open(&self) -> bool {
        !self.job.state.is_final()
    }

    /// "Pause" fits: an answer of it runs, waits in the queue or waits for an approval.
    pub fn can_pause(&self) -> bool {
        matches!(self.job.state, JobState::Queued | JobState::Running | JobState::AwaitingApproval)
    }

    /// "Resume" fits: it is paused, no step of it is unclear, and its conversation is still there. With the model
    /// gone it can still be resumed with its own model (once that answers again) or with another one.
    pub fn can_resume(&self) -> bool {
        self.job.state == JobState::Paused
            && self.job.reason != Some(JobReason::ConversationGone)
            && self.uncertain().is_empty()
    }

    /// "Cancel job…" fits: it is not over yet.
    pub fn can_cancel(&self) -> bool {
        self.is_open()
    }

    /// The steps whose outcome only a person can tell ("Did this happen?"), in order. Only while the job is
    /// paused: a step of a running job may still get its outcome by itself.
    pub fn uncertain(&self) -> Vec<&StepInfo> {
        if self.job.state != JobState::Paused {
            return Vec::new();
        }
        self.steps.iter().filter(|step| step.state == StepState::Uncertain).collect()
    }

    /// Its model does not answer or is off (`model_unavailable`): the card offers "Continue with another model".
    pub fn wants_model(&self) -> bool {
        self.job.state == JobState::Paused && self.job.reason == Some(JobReason::ModelUnavailable)
    }

    /// How many of its steps are done for sure.
    pub fn done_steps(&self) -> usize {
        self.steps.iter().filter(|step| step.state == StepState::Confirmed).count()
    }
}

/// A button of the job card, or the composer's "Stop" in a job's conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobAction {
    Pause,
    /// Resume with the job's own model.
    Resume,
    /// Resume with another model, chosen on purpose (`model_unavailable`): the job never changes it by itself.
    ResumeWith(Choice),
    /// Cancel for good, after the question.
    Cancel,
    /// "It happened" (`done` true) or "It didn't happen" for the unclear step `step`.
    Resolve {
        step: u32,
        done: bool,
    },
}

impl JobAction {
    /// The request for the job `id`.
    pub fn request(&self, id: &str) -> Request {
        let id = id.to_owned();
        match self {
            JobAction::Pause => Request::PauseJob { id },
            JobAction::Resume => Request::ResumeJob { id, engine: None, model: None },
            JobAction::ResumeWith(choice) => {
                Request::ResumeJob { id, engine: Some(choice.engine.clone()), model: choice.model.clone() }
            }
            JobAction::Cancel => Request::CancelJob { id },
            JobAction::Resolve { step, done } => Request::ResolveJobStep { id, step: *step, done: *done },
        }
    }

    /// The word of its button while the request is on its way.
    pub fn busy_word(&self) -> &'static str {
        match self {
            JobAction::Pause => tr("Pausing…"),
            JobAction::Resume | JobAction::ResumeWith(_) => tr("Resuming…"),
            JobAction::Cancel => tr("Cancelling…"),
            JobAction::Resolve { .. } => tr("Saving…"),
        }
    }

    /// What the note says when the core refused it, by its code.
    pub fn failure(&self, code: &str) -> &'static str {
        match (self, code) {
            (_, "conflict") => tr("The job changed meanwhile. Look at its state and try again."),
            (_, "busy") => tr("The assistant is still answering in this conversation."),
            (JobAction::Pause, _) => tr("Couldn't pause the job."),
            (JobAction::Resume | JobAction::ResumeWith(_), _) => tr("Couldn't resume the job."),
            (JobAction::Cancel, _) => tr("Couldn't cancel the job."),
            (JobAction::Resolve { .. }, _) => tr("Couldn't save the answer."),
        }
    }
}

/// The request of "Send" with "Run as job" on: `assistant.start_job` with the text as the goal, in `conversation`
/// or a new one, with the chosen engine and model and the areas switched off. `None` for a blank text or without
/// an engine, which the core would refuse anyway.
pub fn start_request(
    conversation: Option<&str>,
    choice: Option<&Choice>,
    text: &str,
    without: &[Area],
) -> Option<Request> {
    let goal = text.trim();
    let choice = choice?;
    if goal.is_empty() {
        return None;
    }
    Some(Request::StartJob {
        goal: goal.to_owned(),
        engine: choice.engine.clone(),
        model: choice.model.clone(),
        conversation: conversation.map(str::to_owned),
        without: without.to_vec(),
    })
}

/// The word of the job's state for its pill.
pub fn state_word(state: JobState) -> &'static str {
    match state {
        JobState::Queued => tr("Queued"),
        JobState::Running => tr("Running"),
        JobState::AwaitingApproval => tr("Waiting for approval"),
        JobState::Paused => tr("Paused"),
        JobState::Completed => tr("Done"),
        JobState::Failed => tr("Failed"),
        JobState::Cancelled => tr("Cancelled"),
    }
}

/// Whether the job waits for the person: for an approval, or paused for a reason only they can mend (a step to
/// decide, a model, access). Not a job paused or stopped by hand: that one waits for nobody.
pub fn waits_for_person(job: &JobInfo) -> bool {
    match job.state {
        JobState::AwaitingApproval => true,
        JobState::Paused => !matches!(job.reason, None | Some(JobReason::PausedByRequest | JobReason::Stopped)),
        _ => false,
    }
}

/// The newest job of each conversation that waits for the person ([`waits_for_person`]), by conversation.
pub fn waiting_by_conversation(jobs: Vec<JobInfo>) -> HashMap<String, JobInfo> {
    let mut newest: HashMap<String, JobInfo> = HashMap::new();
    for job in jobs {
        let newer = newest
            .get(&job.conversation)
            .is_none_or(|known| (job.created, job.updated) > (known.created, known.updated));
        if newer {
            newest.insert(job.conversation.clone(), job);
        }
    }
    newest.retain(|_, job| waits_for_person(job));
    newest
}

/// The newest job of a conversation's list (`assistant.list_jobs` lists the oldest first).
pub fn newest(jobs: Vec<JobInfo>) -> Option<JobInfo> {
    jobs.into_iter().max_by_key(|job| (job.created, job.updated))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn job(conversation: &str, state: JobState, reason: Option<JobReason>) -> JobInfo {
        let now = Utc::now();
        JobInfo {
            id: format!("{conversation}-job"),
            conversation: conversation.into(),
            goal: "Sort the invoices".into(),
            owner: None,
            engine: "ai:ollama".into(),
            model: "qwen3:14b".into(),
            state,
            reason,
            message: None,
            last_confirmed: None,
            created: now,
            updated: now,
        }
    }

    fn step(seq: u32, state: StepState) -> StepInfo {
        let now = Utc::now();
        StepInfo {
            seq,
            tool: "mail.send_draft".into(),
            state,
            repeatable: false,
            approval: None,
            result: None,
            error: None,
            created: now,
            updated: now,
        }
    }

    #[test]
    fn the_buttons_follow_the_state() {
        let running = JobView { job: job("c", JobState::Running, None), steps: vec![step(1, StepState::Uncertain)] };
        assert!(running.can_pause() && running.can_cancel() && !running.can_resume());
        assert!(running.uncertain().is_empty(), "a running job's step may still get its outcome by itself");
        let unclear = JobView {
            job: job("c", JobState::Paused, Some(JobReason::Uncertain)),
            steps: vec![step(1, StepState::Confirmed), step(2, StepState::Uncertain)],
        };
        assert!(!unclear.can_resume() && !unclear.can_pause());
        assert_eq!(unclear.uncertain().len(), 1);
        assert_eq!(unclear.done_steps(), 1);
        let gone = JobView { job: job("c", JobState::Paused, Some(JobReason::ModelUnavailable)), steps: Vec::new() };
        assert!(gone.wants_model() && gone.can_resume());
        let over = JobView { job: job("c", JobState::Completed, None), steps: Vec::new() };
        assert!(!over.is_open() && !over.can_cancel() && !over.can_resume());
    }

    #[test]
    fn each_button_asks_for_exactly_its_command() {
        let choice = Choice { engine: "ai:other".into(), model: Some("m".into()) };
        assert_eq!(JobAction::Pause.request("j"), Request::PauseJob { id: "j".into() });
        assert_eq!(JobAction::Resume.request("j"), Request::ResumeJob { id: "j".into(), engine: None, model: None });
        assert_eq!(
            JobAction::ResumeWith(choice).request("j"),
            Request::ResumeJob { id: "j".into(), engine: Some("ai:other".into()), model: Some("m".into()) }
        );
        assert_eq!(JobAction::Cancel.request("j"), Request::CancelJob { id: "j".into() });
        assert_eq!(
            JobAction::Resolve { step: 3, done: false }.request("j"),
            Request::ResolveJobStep { id: "j".into(), step: 3, done: false }
        );
    }

    #[test]
    fn a_job_starts_only_with_a_goal_and_an_engine() {
        let choice = Choice { engine: "ai:ollama".into(), model: None };
        assert_eq!(start_request(None, Some(&choice), "  \n", &[]), None);
        assert_eq!(start_request(None, None, "Sort", &[]), None);
        let Some(Request::StartJob { goal, conversation, .. }) =
            start_request(Some("c1"), Some(&choice), " Sort the invoices \n", &[])
        else {
            panic!("a start_job")
        };
        assert_eq!((goal.as_str(), conversation.as_deref()), ("Sort the invoices", Some("c1")));
    }

    #[test]
    fn only_jobs_that_need_the_person_are_marked() {
        let jobs = vec![
            job("a", JobState::Paused, Some(JobReason::Stopped)),
            job("b", JobState::Paused, Some(JobReason::Uncertain)),
            job("c", JobState::AwaitingApproval, None),
            job("d", JobState::Running, None),
            job("e", JobState::Paused, Some(JobReason::PausedByRequest)),
        ];
        let mut marked: Vec<String> = waiting_by_conversation(jobs).into_keys().collect();
        marked.sort();
        assert_eq!(marked, ["b", "c"]);
    }
}
