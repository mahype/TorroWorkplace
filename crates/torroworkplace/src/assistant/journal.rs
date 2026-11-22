//! The journal of the assistant's long-running jobs (issue #961): `<data>/assistant/jobs.sqlite`, mode 0600 in the
//! folder of the conversations (0700). It keeps each job ([`JobInfo`]) and each tool call of it ([`StepInfo`]) with
//! its state, and every change is one transaction that checks the state machine first
//! ([`JobState::may_become`], [`StepState::may_become`]): two callers that resume the same job at once find one of
//! them `conflict`, and a step is only handed to the core while its job runs ([`Journal::step_to`]).
//!
//! What it keeps is little on purpose: no tool input and no result, only the command's name, a digest of the call
//! (to see the same call again, [`call_key`]) and the number of its tool line in the conversation. The result
//! itself stays in the conversation's file, under its 24-hour rule (issue #714), so the journal never keeps what the
//! conversation drops. The goal is the job's first message, also in the conversation; jobs go with their
//! conversation ([`Journal::remove_conversation`]).
//!
//! Like the workflows' journal (`crate::workflows::store`), a file it does not know (another layout, a newer
//! version, damaged) is never repaired or overwritten: every use is `Blocked` and the file stays as it is.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, Transaction, params};
use torro_ipc::api::Area;
use torro_ipc::assistant::{JobInfo, JobReason, JobState, StepInfo, StepState};

/// The layout of the tables, kept in SQLite's `user_version`. A file with a higher number was written by a newer
/// core and is left alone.
pub const SCHEMA_VERSION: i64 = 1;

/// The journal's file name in `<data>/assistant`.
pub const FILE: &str = "jobs.sqlite";

/// Why a change of the journal did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    /// No such job or step.
    NotFound,
    /// The job or step is in a state that does not allow the change (already resumed, no longer running …).
    Conflict(String),
    /// The file cannot be used: missing folder rights, another layout, a newer version, damaged.
    Blocked,
}

impl From<rusqlite::Error> for JournalError {
    fn from(_: rusqlite::Error) -> JournalError {
        JournalError::Blocked
    }
}

/// A job to write down ([`Journal::create`]).
#[derive(Debug, Clone)]
pub struct NewJob {
    pub id: String,
    pub conversation: String,
    pub goal: String,
    pub owner: Option<String>,
    pub engine: String,
    pub model: String,
    /// The areas switched off for its answers: kept, so a resumed answer is as narrow as the first.
    pub without: Vec<Area>,
}

/// The journal; `path` `None` keeps it in memory (tests and a core without a home directory).
pub struct Journal {
    path: Option<PathBuf>,
    connection: Mutex<Option<Connection>>,
}

impl Journal {
    /// The journal at `path`, opened on first use.
    pub fn at(path: Option<PathBuf>) -> Journal {
        Journal { path, connection: Mutex::new(None) }
    }

    /// The journal in the folder `dir` of the conversations ([`FILE`]), or in memory without one.
    pub fn in_dir(dir: Option<&Path>) -> Journal {
        Journal::at(dir.map(|dir| dir.join(FILE)))
    }

    /// Runs `change` in one transaction, committed only when it returns `Ok`.
    fn transaction<T>(
        &self,
        change: impl FnOnce(&Transaction<'_>) -> Result<T, JournalError>,
    ) -> Result<T, JournalError> {
        let mut held = self.connection.lock().unwrap_or_else(PoisonError::into_inner);
        if held.is_none() {
            *held = Some(self.open()?);
        }
        let connection = held.as_mut().ok_or(JournalError::Blocked)?;
        let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let result = change(&transaction)?;
        transaction.commit()?;
        Ok(result)
    }

    /// Opens the file, creating it 0600 when it is not there, and brings its layout up to [`SCHEMA_VERSION`].
    fn open(&self) -> Result<Connection, JournalError> {
        let mut connection = match &self.path {
            Some(path) => {
                let parent = path.parent().ok_or(JournalError::Blocked)?;
                let odd = |meta: fs::Metadata, dir: bool| meta.file_type().is_symlink() || meta.is_dir() != dir;
                if path.symlink_metadata().is_ok_and(|meta| odd(meta, false))
                    || parent.symlink_metadata().is_ok_and(|meta| odd(meta, true))
                {
                    return Err(JournalError::Blocked);
                }
                if !path.exists() {
                    fs::create_dir_all(parent).map_err(|_| JournalError::Blocked)?;
                    let mut options = fs::OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.mode(0o600);
                    }
                    options.open(path).map_err(|_| JournalError::Blocked)?;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| JournalError::Blocked)?;
                }
                Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?
            }
            None => Connection::open_in_memory()?,
        };
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        migrate(&mut connection)?;
        Ok(connection)
    }

    /// Writes down the job `new` as `queued`.
    pub fn create(&self, new: &NewJob, now: DateTime<Utc>) -> Result<JobInfo, JournalError> {
        let without = serde_json::to_string(&new.without).map_err(|_| JournalError::Blocked)?;
        self.transaction(|tx| {
            tx.execute(
                "INSERT INTO jobs(id,conversation,goal,owner,engine,model,without,state,created,updated)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
                params![
                    new.id,
                    new.conversation,
                    new.goal,
                    new.owner,
                    new.engine,
                    new.model,
                    without,
                    JobState::Queued.as_str(),
                    now.to_rfc3339()
                ],
            )?;
            job_in(tx, &new.id)
        })
    }

    /// The job `id`.
    pub fn job(&self, id: &str) -> Result<JobInfo, JournalError> {
        self.transaction(|tx| job_in(tx, id))
    }

    /// The job `id` with its steps in order.
    pub fn get(&self, id: &str) -> Result<(JobInfo, Vec<StepInfo>), JournalError> {
        self.transaction(|tx| Ok((job_in(tx, id)?, steps_in(tx, id)?)))
    }

    /// The areas switched off for the job `id`'s answers.
    pub fn without(&self, id: &str) -> Result<Vec<Area>, JournalError> {
        self.transaction(|tx| {
            let text: String = tx
                .query_row("SELECT without FROM jobs WHERE id=?1", [id], |row| row.get(0))
                .optional()?
                .ok_or(JournalError::NotFound)?;
            serde_json::from_str(&text).map_err(|_| JournalError::Blocked)
        })
    }

    /// Every job, the oldest first; with `conversation` only its jobs.
    pub fn list(&self, conversation: Option<&str>) -> Result<Vec<JobInfo>, JournalError> {
        self.transaction(|tx| {
            let mut statement =
                tx.prepare("SELECT * FROM jobs WHERE ?1 IS NULL OR conversation=?1 ORDER BY created, rowid")?;
            let rows = statement.query_map([conversation], job_of)?;
            rows.map(|row| row?.map_err(|_| JournalError::Blocked)).collect()
        })
    }

    /// The jobs a restart interrupted: queued, running or waiting for an approval.
    pub fn unfinished(&self) -> Result<Vec<JobInfo>, JournalError> {
        let open = [JobState::Queued, JobState::Running, JobState::AwaitingApproval];
        Ok(self.list(None)?.into_iter().filter(|job| open.contains(&job.state)).collect())
    }

    /// The job of `conversation` that is not over yet, paused ones too; `None` when there is none.
    pub fn open_in(&self, conversation: &str) -> Result<Option<JobInfo>, JournalError> {
        Ok(self.list(Some(conversation))?.into_iter().rev().find(|job| !job.state.is_final()))
    }

    /// Moves the job `id` from one of `from` to `to`, with `reason` and `message` (for people) for a paused or
    /// failed one, in one transaction: `conflict` when it is in none of `from` (someone else changed it first) or
    /// the state machine forbids the change. A job that goes on (`queued`, `running`, `completed`) loses its reason.
    pub fn move_job(
        &self,
        id: &str,
        from: &[JobState],
        to: JobState,
        reason: Option<JobReason>,
        message: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<JobInfo, JournalError> {
        self.transaction(|tx| {
            let job = job_in(tx, id)?;
            if !from.contains(&job.state) || !job.state.may_become(to) {
                return Err(JournalError::Conflict(format!("the job is {}", job.state.as_str())));
            }
            let keeps = matches!(to, JobState::Paused | JobState::Failed | JobState::Cancelled);
            let reason = if keeps { reason.map(reason_name) } else { None };
            let message = if keeps { message } else { None };
            tx.execute(
                "UPDATE jobs SET state=?2, reason=?3, message=?4, updated=?5 WHERE id=?1 AND state=?6",
                params![id, to.as_str(), reason, message, now.to_rfc3339(), job.state.as_str()],
            )?;
            job_in(tx, id)
        })
    }

    /// Says why the job `id` waits or ended: `reason` and `message`, while it is paused, failed or cancelled.
    pub fn explain(&self, id: &str, reason: JobReason, message: &str) -> Result<(), JournalError> {
        self.transaction(|tx| {
            tx.execute(
                "UPDATE jobs SET reason=?2, message=?3 WHERE id=?1 AND state IN (?4,?5,?6)",
                params![
                    id,
                    reason_name(reason),
                    message,
                    JobState::Paused.as_str(),
                    JobState::Failed.as_str(),
                    JobState::Cancelled.as_str()
                ],
            )?;
            Ok(())
        })
    }

    /// A person decides the uncertain step `seq` of the job `id`: it happened (`done`, confirmed) or it did not
    /// (failed with `not_done`, so it may run again). `conflict` for a step that is not uncertain.
    pub fn resolve(&self, id: &str, seq: u32, done: bool, now: DateTime<Utc>) -> Result<StepInfo, JournalError> {
        self.transaction(|tx| {
            job_in(tx, id)?;
            let step = step_in(tx, id, seq)?;
            if step.state != StepState::Uncertain {
                return Err(JournalError::Conflict(format!("the step is {}", step.state.as_str())));
            }
            let (to, error) = if done { (StepState::Confirmed, None) } else { (StepState::Failed, Some("not_done")) };
            tx.execute(
                "UPDATE steps SET state=?3, error=?4, updated=?5 WHERE job=?1 AND seq=?2",
                params![id, seq, to.as_str(), error, now.to_rfc3339()],
            )?;
            if done {
                tx.execute(
                    "UPDATE jobs SET last_confirmed=MAX(COALESCE(last_confirmed,0),?2), updated=?3 WHERE id=?1",
                    params![id, seq, now.to_rfc3339()],
                )?;
            }
            step_in(tx, id, seq)
        })
    }

    /// Chooses `engine` and `model` for the paused job `id`: a person's or a client's explicit choice when it
    /// resumes, never the core's own.
    pub fn choose_model(&self, id: &str, engine: &str, model: &str, now: DateTime<Utc>) -> Result<(), JournalError> {
        self.transaction(|tx| {
            let job = job_in(tx, id)?;
            if job.state != JobState::Paused {
                return Err(JournalError::Conflict(format!("the job is {}", job.state.as_str())));
            }
            tx.execute(
                "UPDATE jobs SET engine=?2, model=?3, updated=?4 WHERE id=?1",
                params![id, engine, model, now.to_rfc3339()],
            )?;
            Ok(())
        })
    }

    /// Writes down a new step of the job `id` as `planned` and answers its number: only while the job runs.
    pub fn plan(
        &self,
        id: &str,
        tool: &str,
        key: &str,
        repeatable: bool,
        now: DateTime<Utc>,
    ) -> Result<u32, JournalError> {
        self.transaction(|tx| {
            let job = job_in(tx, id)?;
            if job.state != JobState::Running {
                return Err(JournalError::Conflict(format!("the job is {}", job.state.as_str())));
            }
            let seq: u32 =
                tx.query_row("SELECT COALESCE(MAX(seq),0)+1 FROM steps WHERE job=?1", [id], |row| row.get(0))?;
            tx.execute(
                "INSERT INTO steps(job,seq,tool,call_key,repeatable,state,created,updated)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?7)",
                params![id, seq, tool, key, repeatable, StepState::Planned.as_str(), now.to_rfc3339()],
            )?;
            Ok(seq)
        })
    }

    /// Moves step `seq` of the job `id` to `to`, with the `approval` it waits for and the `error` code it failed
    /// with, in one transaction. A step is handed to the core (`dispatched`) only while its job runs: once a job
    /// is paused or cancelled, no further step starts. A confirmed step becomes the job's last confirmed one.
    pub fn step_to(
        &self,
        id: &str,
        seq: u32,
        to: StepState,
        error: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<StepInfo, JournalError> {
        self.transaction(|tx| {
            let job = job_in(tx, id)?;
            let step = step_in(tx, id, seq)?;
            if !step.state.may_become(to) {
                return Err(JournalError::Conflict(format!("the step is {}", step.state.as_str())));
            }
            if to == StepState::Dispatched && job.state != JobState::Running {
                return Err(JournalError::Conflict(format!("the job is {}", job.state.as_str())));
            }
            tx.execute(
                "UPDATE steps SET state=?3, error=?4, updated=?5 WHERE job=?1 AND seq=?2",
                params![id, seq, to.as_str(), error, now.to_rfc3339()],
            )?;
            if to == StepState::Confirmed {
                tx.execute(
                    "UPDATE jobs SET last_confirmed=MAX(COALESCE(last_confirmed,0),?2), updated=?3 WHERE id=?1",
                    params![id, seq, now.to_rfc3339()],
                )?;
            }
            step_in(tx, id, seq)
        })
    }

    /// Keeps that step `seq` of the job `id` waits for the approval `approval`, and the job with it
    /// (`awaiting_approval`) while it runs.
    pub fn awaits(&self, id: &str, seq: u32, approval: &str, now: DateTime<Utc>) -> Result<(), JournalError> {
        self.transaction(|tx| {
            tx.execute(
                "UPDATE steps SET approval=?3, updated=?4 WHERE job=?1 AND seq=?2",
                params![id, seq, approval, now.to_rfc3339()],
            )?;
            tx.execute(
                "UPDATE jobs SET state=?2, updated=?3 WHERE id=?1 AND state=?4",
                params![id, JobState::AwaitingApproval.as_str(), now.to_rfc3339(), JobState::Running.as_str()],
            )?;
            Ok(())
        })
    }

    /// The approval of step `seq` was decided: the job runs again, unless it was paused or ended meanwhile.
    pub fn decided(&self, id: &str, now: DateTime<Utc>) -> Result<(), JournalError> {
        self.transaction(|tx| {
            tx.execute(
                "UPDATE jobs SET state=?2, updated=?3 WHERE id=?1 AND state=?4",
                params![id, JobState::Running.as_str(), now.to_rfc3339(), JobState::AwaitingApproval.as_str()],
            )?;
            Ok(())
        })
    }

    /// Keeps the number `entry` of the tool line of step `seq` of the job `id`: where its result stands.
    pub fn set_result(&self, id: &str, seq: u32, entry: u64) -> Result<(), JournalError> {
        let entry = i64::try_from(entry).map_err(|_| JournalError::Blocked)?;
        self.transaction(|tx| {
            tx.execute("UPDATE steps SET result=?3 WHERE job=?1 AND seq=?2", params![id, seq, entry])?;
            Ok(())
        })
    }

    /// The confirmed step of the job `id` with the call digest `key`, when there is one: that call is not run again.
    pub fn confirmed_with(&self, id: &str, key: &str) -> Result<Option<StepInfo>, JournalError> {
        self.transaction(|tx| {
            let seq: Option<u32> = tx
                .query_row(
                    "SELECT seq FROM steps WHERE job=?1 AND call_key=?2 AND state=?3 ORDER BY seq LIMIT 1",
                    params![id, key, StepState::Confirmed.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            seq.map(|seq| step_in(tx, id, seq)).transpose()
        })
    }

    /// Removes every job of `conversation` with its steps, as the conversation goes. Answers how many went.
    pub fn remove_conversation(&self, conversation: &str) -> Result<usize, JournalError> {
        self.transaction(|tx| {
            tx.execute("DELETE FROM steps WHERE job IN (SELECT id FROM jobs WHERE conversation=?1)", [conversation])?;
            Ok(tx.execute("DELETE FROM jobs WHERE conversation=?1", [conversation])?)
        })
    }
}

/// The digest of a call (the command's name and its request as the core gets it): the same call again has the same
/// digest, so a confirmed step is never run a second time. Only the digest is kept, never the input.
pub fn call_key(tool: &str, request: &torro_ipc::Request) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(tool.as_bytes());
    hash.update([0]);
    hash.update(serde_json::to_vec(request).unwrap_or_default());
    hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Brings the layout of `connection` up to [`SCHEMA_VERSION`]: a new file gets the tables; one of this version is
/// checked; one of a newer version or another layout is `Blocked` and left as it is.
fn migrate(connection: &mut Connection) -> Result<(), JournalError> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    match version {
        0 => {
            let tables: i64 = connection.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))?;
            if tables != 0 {
                // Something else wrote this file: never overwrite it.
                return Err(JournalError::Blocked);
            }
            let tx = connection.transaction()?;
            tx.execute_batch(
                "CREATE TABLE jobs(
                    id TEXT PRIMARY KEY, conversation TEXT NOT NULL, goal TEXT NOT NULL, owner TEXT,
                    engine TEXT NOT NULL, model TEXT NOT NULL, without TEXT NOT NULL DEFAULT '[]',
                    state TEXT NOT NULL, reason TEXT, message TEXT, last_confirmed INTEGER,
                    created TEXT NOT NULL, updated TEXT NOT NULL);
                 CREATE INDEX job_conversation ON jobs(conversation);
                 CREATE INDEX job_state ON jobs(state);
                 CREATE TABLE steps(
                    job TEXT NOT NULL, seq INTEGER NOT NULL, tool TEXT NOT NULL, call_key TEXT NOT NULL,
                    repeatable INTEGER NOT NULL, state TEXT NOT NULL, approval TEXT, result INTEGER, error TEXT,
                    created TEXT NOT NULL, updated TEXT NOT NULL, PRIMARY KEY(job, seq));
                 CREATE INDEX step_call ON steps(job, call_key);",
            )?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
            Ok(())
        }
        SCHEMA_VERSION => {
            let check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
            if check != "ok" {
                return Err(JournalError::Blocked);
            }
            connection.prepare(
                "SELECT id,conversation,goal,owner,engine,model,without,state,reason,message,\
                 last_confirmed,created,updated FROM jobs LIMIT 0",
            )?;
            connection.prepare(
                "SELECT job,seq,tool,call_key,repeatable,state,approval,result,error,created,updated FROM steps LIMIT 0",
            )?;
            Ok(())
        }
        _ => Err(JournalError::Blocked),
    }
}

/// The name of `reason` in the journal, as on the wire.
fn reason_name(reason: JobReason) -> String {
    serde_json::to_value(reason).ok().and_then(|value| value.as_str().map(str::to_string)).unwrap_or_default()
}

/// The reason named `name`; `None` for one this version does not know.
fn reason_of(name: &str) -> Option<JobReason> {
    serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
}

fn time_of(text: &str) -> Result<DateTime<Utc>, JournalError> {
    DateTime::parse_from_rfc3339(text).map(|time| time.with_timezone(&Utc)).map_err(|_| JournalError::Blocked)
}

/// The job of a row of `jobs`, or `Blocked` for one this version cannot read (an unknown state).
fn job_of(row: &Row<'_>) -> rusqlite::Result<Result<JobInfo, JournalError>> {
    let state: String = row.get("state")?;
    let reason: Option<String> = row.get("reason")?;
    let (created, updated): (String, String) = (row.get("created")?, row.get("updated")?);
    let read = || -> Result<JobInfo, JournalError> {
        Ok(JobInfo {
            id: row.get("id")?,
            conversation: row.get("conversation")?,
            goal: row.get("goal")?,
            owner: row.get("owner")?,
            engine: row.get("engine")?,
            model: row.get("model")?,
            state: JobState::parse(&state).ok_or(JournalError::Blocked)?,
            reason: reason.as_deref().and_then(reason_of),
            message: row.get("message")?,
            last_confirmed: row.get("last_confirmed")?,
            created: time_of(&created)?,
            updated: time_of(&updated)?,
        })
    };
    Ok(read())
}

fn job_in(tx: &Transaction<'_>, id: &str) -> Result<JobInfo, JournalError> {
    tx.query_row("SELECT * FROM jobs WHERE id=?1", [id], job_of).optional()?.ok_or(JournalError::NotFound)?
}

fn step_of(row: &Row<'_>) -> rusqlite::Result<Result<StepInfo, JournalError>> {
    let state: String = row.get("state")?;
    let (created, updated): (String, String) = (row.get("created")?, row.get("updated")?);
    let read = || -> Result<StepInfo, JournalError> {
        Ok(StepInfo {
            seq: row.get("seq")?,
            tool: row.get("tool")?,
            state: StepState::parse(&state).ok_or(JournalError::Blocked)?,
            repeatable: row.get("repeatable")?,
            approval: row.get("approval")?,
            result: row.get::<_, Option<i64>>("result")?.and_then(|entry| u64::try_from(entry).ok()),
            error: row.get("error")?,
            created: time_of(&created)?,
            updated: time_of(&updated)?,
        })
    };
    Ok(read())
}

fn step_in(tx: &Transaction<'_>, id: &str, seq: u32) -> Result<StepInfo, JournalError> {
    tx.query_row("SELECT * FROM steps WHERE job=?1 AND seq=?2", params![id, seq], step_of)
        .optional()?
        .ok_or(JournalError::NotFound)?
}

fn steps_in(tx: &Transaction<'_>, id: &str) -> Result<Vec<StepInfo>, JournalError> {
    let mut statement = tx.prepare("SELECT * FROM steps WHERE job=?1 ORDER BY seq")?;
    let rows = statement.query_map([id], step_of)?;
    rows.map(|row| row?).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_job(id: &str, conversation: &str) -> NewJob {
        NewJob {
            id: id.into(),
            conversation: conversation.into(),
            goal: "Sort the invoices".into(),
            owner: None,
            engine: "ai:ollama@localhost:11434".into(),
            model: "qwen3:8b".into(),
            without: vec![Area::Mail],
        }
    }

    fn running(journal: &Journal, id: &str) {
        journal.move_job(id, &[JobState::Queued], JobState::Running, None, None, Utc::now()).expect("running");
    }

    #[test]
    fn jobs_and_steps_survive_a_reopen_without_inputs_or_results() {
        let dir = tempfile::tempdir().expect("temp");
        let journal = Journal::in_dir(Some(dir.path()));
        journal.create(&new_job("j1", "c1"), Utc::now()).expect("created");
        running(&journal, "j1");
        let request = torro_ipc::Request::ListConversations;
        let key = call_key("assistant.list_conversations", &request);
        let seq = journal.plan("j1", "inbox.list_items", &key, true, Utc::now()).expect("planned");
        journal.step_to("j1", seq, StepState::Dispatched, None, Utc::now()).expect("dispatched");
        journal.step_to("j1", seq, StepState::Confirmed, None, Utc::now()).expect("confirmed");
        journal.set_result("j1", seq, 7).expect("result");
        drop(journal);

        let path = dir.path().join(FILE);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("file").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let again = Journal::in_dir(Some(dir.path()));
        let (job, steps) = again.get("j1").expect("kept");
        assert_eq!((job.state, job.last_confirmed), (JobState::Running, Some(1)));
        assert_eq!(again.without("j1").expect("without"), vec![Area::Mail]);
        assert_eq!(steps.len(), 1);
        assert_eq!((steps[0].state, steps[0].result), (StepState::Confirmed, Some(7)));
        assert_eq!(again.confirmed_with("j1", &key).expect("found").map(|step| step.seq), Some(1));
        let bytes = fs::read(&path).expect("read");
        assert!(!String::from_utf8_lossy(&bytes).contains("assistant.list_conversations\"}"), "no request is kept");
    }

    #[test]
    fn transitions_follow_the_state_machine_and_a_resume_is_won_once() {
        let journal = Journal::in_dir(None);
        journal.create(&new_job("j1", "c1"), Utc::now()).expect("created");
        let refused = journal.move_job("j1", &[JobState::Queued], JobState::Completed, None, None, Utc::now());
        assert!(matches!(refused, Err(JournalError::Conflict(_))), "queued never completes at once");
        running(&journal, "j1");
        let paused = journal
            .move_job(
                "j1",
                &[JobState::Running],
                JobState::Paused,
                Some(JobReason::Stopped),
                Some("Stopped"),
                Utc::now(),
            )
            .expect("paused");
        assert_eq!((paused.reason, paused.message.as_deref()), (Some(JobReason::Stopped), Some("Stopped")));
        // Two resumes: the first wins, the second finds it queued.
        let first = journal.move_job("j1", &[JobState::Paused], JobState::Queued, None, None, Utc::now());
        let second = journal.move_job("j1", &[JobState::Paused], JobState::Queued, None, None, Utc::now());
        assert_eq!(first.expect("won").reason, None, "a job that goes on loses its reason");
        assert!(matches!(second, Err(JournalError::Conflict(_))));
        assert_eq!(
            journal.move_job("nope", &[JobState::Paused], JobState::Queued, None, None, Utc::now()).err(),
            Some(JournalError::NotFound)
        );
    }

    #[test]
    fn a_step_starts_only_while_its_job_runs_and_never_goes_back() {
        let journal = Journal::in_dir(None);
        journal.create(&new_job("j1", "c1"), Utc::now()).expect("created");
        assert!(journal.plan("j1", "mail.send_draft", "k", false, Utc::now()).is_err(), "not running yet");
        running(&journal, "j1");
        let seq = journal.plan("j1", "mail.send_draft", "k", false, Utc::now()).expect("planned");
        journal
            .move_job("j1", &[JobState::Running], JobState::Paused, Some(JobReason::PausedByRequest), None, Utc::now())
            .expect("paused");
        assert!(
            matches!(
                journal.step_to("j1", seq, StepState::Dispatched, None, Utc::now()),
                Err(JournalError::Conflict(_))
            ),
            "a paused job hands nothing more to the core"
        );
        journal.move_job("j1", &[JobState::Paused], JobState::Queued, None, None, Utc::now()).expect("queued");
        running(&journal, "j1");
        journal.step_to("j1", seq, StepState::Dispatched, None, Utc::now()).expect("dispatched");
        journal.step_to("j1", seq, StepState::Uncertain, None, Utc::now()).expect("uncertain");
        assert!(journal.step_to("j1", seq, StepState::Dispatched, None, Utc::now()).is_err(), "never dispatched again");
        let step = journal.step_to("j1", seq, StepState::Confirmed, None, Utc::now()).expect("reconciled");
        assert_eq!(step.state, StepState::Confirmed);
        assert_eq!(journal.job("j1").expect("job").last_confirmed, Some(seq));
    }

    #[test]
    fn jobs_go_with_their_conversation() {
        let journal = Journal::in_dir(None);
        journal.create(&new_job("j1", "c1"), Utc::now()).expect("created");
        journal.create(&new_job("j2", "c2"), Utc::now()).expect("created");
        running(&journal, "j1");
        journal.plan("j1", "inbox.list_items", "k", true, Utc::now()).expect("planned");
        assert_eq!(journal.open_in("c1").expect("open").map(|job| job.id), Some("j1".to_string()));
        assert_eq!(journal.remove_conversation("c1").expect("removed"), 1);
        assert_eq!(journal.get("j1").err(), Some(JournalError::NotFound));
        assert_eq!(journal.list(None).expect("list").len(), 1);
        assert_eq!(journal.unfinished().expect("open").len(), 1, "the queued one of c2");
    }

    #[test]
    fn a_newer_or_foreign_file_stays_untouched() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join(FILE);
        Journal::in_dir(Some(dir.path())).create(&new_job("j1", "c1"), Utc::now()).expect("created");
        let connection = Connection::open(&path).expect("db");
        connection.pragma_update(None, "user_version", 99).expect("future");
        drop(connection);
        let before = fs::read(&path).expect("before");
        assert_eq!(Journal::in_dir(Some(dir.path())).list(None).err(), Some(JournalError::Blocked));
        assert_eq!(fs::read(&path).expect("after"), before);

        let other = tempfile::tempdir().expect("temp");
        let foreign = other.path().join(FILE);
        Connection::open(&foreign).expect("db").execute_batch("CREATE TABLE notes(x TEXT);").expect("table");
        let before = fs::read(&foreign).expect("before");
        assert_eq!(Journal::in_dir(Some(other.path())).list(None).err(), Some(JournalError::Blocked));
        assert_eq!(fs::read(&foreign).expect("after"), before);
    }
}
