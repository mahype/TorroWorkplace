//! Long-running jobs of the assistant on the wire (issue #961, "Langlebige Aufträge" in `docs/assistent.md`):
//! a job ([`JobInfo`]) is a task a person or a client started on purpose with `assistant.start_job`; it runs as
//! answers in its conversation, and the core keeps a journal of its tool calls ([`StepInfo`]), so it can go on
//! safely after the core restarted. A normal message never becomes a job.
//!
//! Two state machines, both checked by the core before every change ([`JobState::may_become`],
//! [`StepState::may_become`]):
//!
//! - a job: `queued → running → completed | failed | paused | cancelled`, with `awaiting_approval` while a
//!   command of it waits for a person, and `paused → queued` when it is resumed;
//! - a step: `planned → dispatched → confirmed | failed | uncertain`, and `uncertain → confirmed | failed` once the
//!   outcome was looked up or a person decided. A step that changes something and was handed to the core without
//!   a known outcome is `uncertain` and never runs again by itself.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a job stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Started or resumed, its next answer not running yet.
    Queued,
    /// An answer of the job runs.
    Running,
    /// A command of the job waits for a person's approval; nothing else runs meanwhile.
    AwaitingApproval,
    /// Stopped until someone resumes it (`assistant.resume_job`); [`JobInfo::reason`] says why.
    Paused,
    /// The model answered: the job is done.
    Completed,
    /// It ended with an error that resuming would not mend.
    Failed,
    /// Ended for good by `assistant.cancel_job`.
    Cancelled,
}

impl JobState {
    /// Every state, for tests and the store's checks.
    pub const ALL: [JobState; 7] = [
        JobState::Queued,
        JobState::Running,
        JobState::AwaitingApproval,
        JobState::Paused,
        JobState::Completed,
        JobState::Failed,
        JobState::Cancelled,
    ];

    /// The name on the wire and in the core's journal: `awaiting_approval`.
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::AwaitingApproval => "awaiting_approval",
            JobState::Paused => "paused",
            JobState::Completed => "completed",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    /// The state named `name` ([`JobState::as_str`]); `None` for a name this version does not know.
    pub fn parse(name: &str) -> Option<JobState> {
        JobState::ALL.into_iter().find(|state| state.as_str() == name)
    }

    /// Whether the job is over: nothing of it runs again.
    pub fn is_final(self) -> bool {
        matches!(self, JobState::Completed | JobState::Failed | JobState::Cancelled)
    }

    /// Whether a job in this state may change to `next`. A restart puts an interrupted job back into the queue
    /// (`running`/`awaiting_approval` → `queued`), or pauses it when a step is unclear.
    pub fn may_become(self, next: JobState) -> bool {
        use JobState::*;
        matches!(
            (self, next),
            (Queued, Running | Paused | Cancelled | Failed)
                | (Running, AwaitingApproval | Queued | Paused | Completed | Failed | Cancelled)
                | (AwaitingApproval, Running | Queued | Paused | Failed | Cancelled)
                | (Paused, Queued | Cancelled)
        )
    }
}

/// Why a job is paused or failed: what a person (or the client) has to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobReason {
    /// Someone paused it (`assistant.pause_job`).
    PausedByRequest,
    /// Someone stopped the answer in its conversation (`assistant.cancel`).
    Stopped,
    /// A step that changes something has no known outcome ([`StepState::Uncertain`]): someone decides with
    /// `assistant.resolve_job_step` before it goes on.
    Uncertain,
    /// A command of it still waits for a person's approval after a restart: no approval is taken as given.
    ApprovalPending,
    /// The model chosen for the job does not answer or is no longer on; it never goes on with another one by
    /// itself. Resume it with an `engine` and `model` to choose one.
    ModelUnavailable,
    /// The assistant may no longer run a command the job needs (rights, account, tool or approval withdrawn).
    Forbidden,
    /// The assistant has no client (no access set up) any more.
    NoClient,
    /// The answer reached its step limit; resuming lets it go on.
    StepLimit,
    /// Its conversation is gone.
    ConversationGone,
    /// The answer ended with an error.
    Error,
}

/// A long-running job of the assistant (issue #961), as `assistant.get_job` and `assistant.list_jobs` show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JobInfo {
    /// 16 hex digits.
    pub id: String,
    /// The conversation the job runs in.
    pub conversation: String,
    /// What the job is to do, as it was started: data, never instructions.
    pub goal: String,
    /// The client that started it, as `clients.list_clients` lists it; `None` for the person (the app key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The engine (AI account) that answers.
    pub engine: String,
    /// The model chosen when it started or was last resumed with a model: it never changes by itself.
    pub model: String,
    pub state: JobState,
    /// Why it is paused or failed; `None` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<JobReason>,
    /// The reason in words, for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The number of the last step whose outcome is known to be done ([`StepState::Confirmed`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_confirmed: Option<u32>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

/// Where a step of a job stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// Written down, not handed to the core yet.
    Planned,
    /// Handed to the core; its outcome is not known yet.
    Dispatched,
    /// Done: the core answered it.
    Confirmed,
    /// It did not run or failed (refused, denied, an error, or a reading step interrupted).
    Failed,
    /// It changes something and was handed over, but its outcome is unknown (the core restarted, or the job was
    /// paused while it ran). It never runs again by itself.
    Uncertain,
}

impl StepState {
    /// Every state.
    pub const ALL: [StepState; 5] =
        [StepState::Planned, StepState::Dispatched, StepState::Confirmed, StepState::Failed, StepState::Uncertain];

    /// The name on the wire and in the core's journal.
    pub fn as_str(self) -> &'static str {
        match self {
            StepState::Planned => "planned",
            StepState::Dispatched => "dispatched",
            StepState::Confirmed => "confirmed",
            StepState::Failed => "failed",
            StepState::Uncertain => "uncertain",
        }
    }

    /// The state named `name`; `None` for one this version does not know.
    pub fn parse(name: &str) -> Option<StepState> {
        StepState::ALL.into_iter().find(|state| state.as_str() == name)
    }

    /// Whether a step in this state may change to `next`. An uncertain step only becomes confirmed or failed, once
    /// its outcome was looked up or a person decided.
    pub fn may_become(self, next: StepState) -> bool {
        use StepState::*;
        matches!(
            (self, next),
            (Planned, Dispatched | Failed)
                | (Dispatched, Confirmed | Failed | Uncertain)
                | (Uncertain, Confirmed | Failed)
        )
    }
}

/// One tool call of a job, as its journal keeps it: no input and no result, only what tells the step apart and
/// where its result stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StepInfo {
    /// From 1 up within its job.
    pub seq: u32,
    /// The command's catalogue name: `mail.send_draft`.
    pub tool: String,
    pub state: StepState,
    /// Whether running it again does no harm: it only reads. A step that changes something is never repeated by
    /// itself.
    pub repeatable: bool,
    /// The approval it waits or waited for, for `approvals.approval_status`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<String>,
    /// The number of its tool line in the conversation ([`super::Entry::seq`]), once it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<u64>,
    /// The error code when it failed (`forbidden`, `denied`, `interrupted` …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_match_the_wire() {
        for state in JobState::ALL {
            assert_eq!(JobState::parse(state.as_str()), Some(state));
            assert_eq!(serde_json::to_value(state).expect("json"), serde_json::json!(state.as_str()));
        }
        for state in StepState::ALL {
            assert_eq!(StepState::parse(state.as_str()), Some(state));
            assert_eq!(serde_json::to_value(state).expect("json"), serde_json::json!(state.as_str()));
        }
        assert_eq!(JobState::parse("unknown"), None);
    }

    #[test]
    fn final_jobs_never_change_and_uncertain_steps_never_go_back() {
        for state in JobState::ALL.into_iter().filter(|state| state.is_final()) {
            assert!(JobState::ALL.iter().all(|next| !state.may_become(*next)), "{state:?} is final");
        }
        assert!(JobState::Paused.may_become(JobState::Queued));
        assert!(!JobState::Paused.may_become(JobState::Running), "a resume goes through the queue");
        assert!(!StepState::Uncertain.may_become(StepState::Dispatched), "never dispatched again");
        assert!(!StepState::Confirmed.may_become(StepState::Dispatched), "a confirmed step never runs again");
        assert!(!StepState::Planned.may_become(StepState::Confirmed), "only what was handed over is confirmed");
    }
}
