//! The automated acceptance of issue #961: a job goes on after a restart without repeating a confirmed step, never
//! sends a mail twice, takes no approval as given, stops when rights are withdrawn, keeps its model, starts nothing
//! after a pause, and wins a resume only once. A crash is [`Assistant::crash`]: the answers' tasks end, the journal
//! and the conversations stay on disk, and a second assistant on the same folder picks them up.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use torro_core::ai::Place;
use torro_ipc::api::{Area, AreaRights, Level, Outbound, Rights, Selection};
use torro_ipc::assistant::{EntryContent, JobInfo, JobReason, JobState, StepInfo, StepState};
use torro_ipc::{ApprovalState, Request, Response};

use super::super::Assistant;
use super::super::model::tests::{
    ENGINE, FakeHost, Script, Step, answers, ask, calls, read, reader, says, set_up_with,
};

/// Starts a job with `goal`, the engine's model chosen by "Automatic".
async fn start(assistant: &Arc<Assistant>, goal: &str) -> JobInfo {
    match assistant.launch_job(goal.into(), ENGINE.into(), None, None, Vec::new(), None).await {
        Response::Job { job, .. } => job,
        other => panic!("expected a job, got {other:?}"),
    }
}

async fn job(assistant: &Assistant, id: &str) -> (JobInfo, Vec<StepInfo>) {
    match assistant.get_job(id).await {
        Response::Job { job, steps } => (job, steps),
        other => panic!("expected a job, got {other:?}"),
    }
}

/// Reads the job `id` until `check` holds, at most two seconds.
async fn until(
    assistant: &Assistant,
    id: &str,
    check: impl Fn(&JobInfo, &[StepInfo]) -> bool,
) -> (JobInfo, Vec<StepInfo>) {
    for _ in 0..400 {
        let (job, steps) = job(assistant, id).await;
        if check(&job, &steps) {
            return (job, steps);
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let (job, steps) = job(assistant, id).await;
    panic!("the job never got there: {job:?} {steps:?}");
}

/// How often the core got a request named `command`.
fn ran(host: &FakeHost, command: &str) -> usize {
    host.ran.lock().expect("lock").iter().filter(|(request, _)| request.operation().name() == command).count()
}

/// The assistant's rights with sending mail without a person's approval: what makes a second send possible at all.
fn mailer() -> Rights {
    let mut rights = reader();
    rights.areas.insert(Area::Mail, AreaRights { level: Level::Write, admin: false });
    rights.mail_accounts = Selection::All;
    rights.outbound = Outbound::Free;
    rights
}

fn code(response: &Response) -> &str {
    match response {
        Response::Error { code, .. } => code,
        _ => "",
    }
}

/// The messages of conversation `id`.
async fn messages(assistant: &Assistant, id: &str) -> Vec<String> {
    let (_, entries) = read(assistant, id, None).await;
    entries
        .into_iter()
        .filter_map(|entry| match entry.content {
            EntryContent::Message { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn after_a_restart_a_confirmed_read_is_not_repeated() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![calls("inbox_list_items", json!({})), Step::Hang(Vec::new())]);
    let (first, before) = set_up_with(dir.path(), Some(reader()), Place::Local, script, answers);
    let started = start(&first, "Sort my inbox").await;
    until(&first, &started.id, |_, steps| steps.first().is_some_and(|step| step.result.is_some())).await;
    first.crash().await;

    // The model asks for the same list again: the journal answers it, the core is not asked.
    let again = Script::new(vec![calls("inbox_list_items", json!({})), says("Sorted.")]);
    let (second, after) = set_up_with(dir.path(), Some(reader()), Place::Local, Arc::clone(&again), answers);
    second.recover_jobs(chrono::Utc::now()).await;
    let (job, steps) = until(&second, &started.id, |job, _| job.state.is_final()).await;
    assert_eq!(job.state, JobState::Completed);
    assert_eq!((ran(&before, "inbox.list_items"), ran(&after, "inbox.list_items")), (1, 0));
    assert_eq!(steps.len(), 1, "no second step for the same call");
    assert_eq!((steps[0].state, job.last_confirmed), (StepState::Confirmed, Some(1)));
    let asked = again.asked();
    let told = asked[0].messages.iter().rev().find(|message| message.role == torro_core::ai::Role::User);
    assert!(told.is_some_and(|message| message.content.contains("1 inbox.list_items")), "the model knows what is done");
    let (_, entries) = read(&second, &job.conversation, None).await;
    let done = entries.iter().any(|entry| {
        matches!(&entry.content, EntryContent::Tool { summary, .. } if summary == torro_core::i18n::tr("Already done in this job; not run again"))
    });
    assert!(done, "the chat shows that it did not run again");
}

#[tokio::test]
async fn a_crash_while_a_mail_is_sent_never_sends_it_twice() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![calls("mail_send_draft", json!({"id": "d1"})), says("Sent.")]);
    let (first, before) = set_up_with(dir.path(), Some(mailer()), Place::Local, script, answers);
    before.hang.lock().expect("lock").push("mail.send_draft");
    let started = start(&first, "Send the draft to Anna").await;
    until(&first, &started.id, |_, steps| steps.first().is_some_and(|step| step.state == StepState::Dispatched)).await;
    first.crash().await;

    let again = Script::new(vec![calls("mail_send_draft", json!({"id": "d1"})), says("Sent.")]);
    let (second, after) = set_up_with(dir.path(), Some(mailer()), Place::Local, Arc::clone(&again), answers);
    second.recover_jobs(chrono::Utc::now()).await;
    let (job, steps) = job(&second, &started.id).await;
    assert_eq!((job.state, job.reason), (JobState::Paused, Some(JobReason::Uncertain)));
    assert_eq!((steps[0].state, steps[0].repeatable), (StepState::Uncertain, false));
    assert!(again.asked().is_empty(), "no model was asked");
    assert_eq!(ran(&after, "mail.send_draft"), 0);
    // It stays so: resuming is refused until someone decides.
    assert_eq!(code(&second.go_on_with(&started.id, None, None, None).await), "conflict");
    assert_eq!(job_state(&second, &started.id).await, JobState::Paused);
    let (_, entries) = read(&second, &job.conversation, None).await;
    assert!(
        entries.iter().any(|entry| matches!(&entry.content, EntryContent::Error { code, .. } if code == "paused")),
        "the chat says why it waits"
    );

    // The person checked: it went out. Then the same call is not run again.
    assert!(matches!(second.resolve_job_step(&started.id, 1, true).await, Response::Job { .. }));
    assert!(matches!(second.go_on_with(&started.id, None, None, None).await, Response::Job { .. }));
    let (job, steps) = until(&second, &started.id, |job, _| job.state.is_final()).await;
    assert_eq!((job.state, steps[0].state), (JobState::Completed, StepState::Confirmed));
    assert_eq!(ran(&before, "mail.send_draft") + ran(&after, "mail.send_draft"), 1, "exactly one mail");
}

async fn job_state(assistant: &Assistant, id: &str) -> JobState {
    job(assistant, id).await.0.state
}

/// Deleting a template waits for a person in the fake core.
fn deleter() -> Rights {
    let mut rights = reader();
    rights.areas.insert(Area::Workspaces, AreaRights { level: Level::Full, admin: false });
    rights
}

#[tokio::test]
async fn a_restart_with_an_open_approval_runs_nothing_until_it_is_decided() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![calls("workspaces_delete_template", json!({"id": "old-plans"})), says("Done.")]);
    let (first, before) = set_up_with(dir.path(), Some(deleter()), Place::Local, script, answers);
    *before.approvals.lock().expect("lock") = Some(ApprovalState::Pending);
    let started = start(&first, "Delete my old plans").await;
    until(&first, &started.id, |job, steps| {
        job.state == JobState::AwaitingApproval && steps.first().is_some_and(|step| step.approval.is_some())
    })
    .await;
    first.crash().await;

    let again = Script::new(vec![says("Nothing was deleted.")]);
    let (second, after) = set_up_with(dir.path(), Some(deleter()), Place::Local, Arc::clone(&again), answers);
    *after.approvals.lock().expect("lock") = Some(ApprovalState::Pending);
    second.recover_jobs(chrono::Utc::now()).await;
    let (job, steps) = job(&second, &started.id).await;
    assert_eq!((job.state, job.reason), (JobState::Paused, Some(JobReason::ApprovalPending)));
    assert_eq!(steps[0].state, StepState::Uncertain);
    assert!(again.asked().is_empty() && after.ran.lock().expect("lock").is_empty(), "nothing ran");
    assert_eq!(code(&second.go_on_with(&started.id, None, None, None).await), "conflict", "still open");
    assert_eq!(code(&second.resolve_job_step(&started.id, 1, false).await), "conflict", "no decision while it waits");

    // The person denies it: the step did not run, and the job goes on without it.
    *after.approvals.lock().expect("lock") = Some(ApprovalState::Denied);
    assert!(matches!(second.go_on_with(&started.id, None, None, None).await, Response::Job { .. }));
    let (job, steps) = until(&second, &started.id, |job, _| job.state.is_final()).await;
    assert_eq!(job.state, JobState::Completed);
    assert_eq!((steps[0].state, steps[0].error.as_deref()), (StepState::Failed, Some("denied")));
    assert_eq!(ran(&before, "workspaces.delete_template") + ran(&after, "workspaces.delete_template"), 1);
}

#[tokio::test]
async fn rights_withdrawn_before_a_resume_stop_the_job_before_anything_runs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![calls("inbox_list_items", json!({})), Step::Hang(Vec::new())]);
    let (first, _host) = set_up_with(dir.path(), Some(reader()), Place::Local, script, answers);
    let started = start(&first, "Sort my inbox").await;
    until(&first, &started.id, |_, steps| steps.first().is_some_and(|step| step.state == StepState::Confirmed)).await;
    first.crash().await;

    let mut narrower = reader();
    narrower.areas.remove(&Area::Calendar);
    let again = Script::new(vec![calls("calendar_list_calendars", json!({})), says("Sorted.")]);
    let (second, after) = set_up_with(dir.path(), Some(narrower), Place::Local, again, answers);
    second.recover_jobs(chrono::Utc::now()).await;
    let (job, steps) = until(&second, &started.id, |job, _| job.state == JobState::Paused).await;
    assert_eq!(job.reason, Some(JobReason::Forbidden));
    assert!(after.ran.lock().expect("lock").is_empty(), "nothing reached the core");
    assert_eq!(steps.len(), 1, "the refused call never became a step");

    // Without any client left, nothing runs either.
    *after.rights.lock().expect("lock") = None;
    let more = Script::new(vec![calls("tasks_list_todos", json!({})), says("Done.")]);
    let (third, last) = set_up_with(dir.path(), None, Place::Local, more, answers);
    assert!(matches!(third.go_on_with(&started.id, None, None, None).await, Response::Job { .. }));
    let (job, _) = until(&third, &started.id, |job, _| job.state == JobState::Paused).await;
    assert_eq!(job.reason, Some(JobReason::NoClient));
    assert!(last.ran.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn a_model_that_is_gone_pauses_the_job_and_no_other_takes_over() {
    let dir = tempfile::tempdir().expect("temp dir");
    // The model answers nothing: offline.
    let script = Script::new(Vec::new());
    let (assistant, host) = set_up_with(dir.path(), Some(reader()), Place::Local, Arc::clone(&script), answers);
    let started = start(&assistant, "Sort my inbox").await;
    assert_eq!(started.model, "qwen3:8b");
    let (job, _) = until(&assistant, &started.id, |job, _| job.state == JobState::Paused).await;
    assert_eq!(job.reason, Some(JobReason::ModelUnavailable));
    assert!(job.message.as_deref().is_some_and(|message| message.contains("qwen3:8b")), "{job:?}");

    // Now only another model is on: resuming without a choice is refused, and no model is asked.
    *host.only_on.lock().expect("lock") = Some("gemma3:4b");
    let asked = script.asked().len();
    let refused = assistant.go_on_with(&started.id, None, None, None).await;
    assert_eq!(code(&refused), "unsupported", "{refused:?}");
    assert_eq!(job_state(&assistant, &started.id).await, JobState::Paused);
    assert_eq!(script.asked().len(), asked, "no other model was asked");

    // Chosen on purpose, the other model goes on.
    script.steps.lock().expect("lock").push_back(says("Sorted."));
    let chosen = assistant.go_on_with(&started.id, Some(ENGINE.into()), Some("gemma3:4b".into()), None).await;
    assert!(matches!(chosen, Response::Job { .. }), "{chosen:?}");
    let (job, _) = until(&assistant, &started.id, |job, _| job.state.is_final()).await;
    assert_eq!((job.state, job.model.as_str()), (JobState::Completed, "gemma3:4b"));
    assert!(host.ran.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn after_a_restart_a_model_switched_off_pauses_the_job_before_it_asks_another() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![Step::Hang(Vec::new())]);
    let (first, _host) = set_up_with(dir.path(), Some(reader()), Place::Local, script, answers);
    let started = start(&first, "Sort my inbox").await;
    first.crash().await;
    let again = Script::new(vec![says("Sorted.")]);
    let (second, after) = set_up_with(dir.path(), Some(reader()), Place::Local, Arc::clone(&again), answers);
    *after.only_on.lock().expect("lock") = Some("gemma3:4b");
    second.recover_jobs(chrono::Utc::now()).await;
    let (job, _) = until(&second, &started.id, |job, _| job.state == JobState::Paused).await;
    assert_eq!((job.reason, job.model.as_str()), (Some(JobReason::ModelUnavailable), "qwen3:8b"));
    assert!(again.asked().is_empty(), "the other model never saw the conversation");
}

#[tokio::test]
async fn a_pause_while_a_mail_is_sent_starts_nothing_more_and_keeps_it_uncertain() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![
        calls("mail_send_draft", json!({"id": "d1"})),
        calls("inbox_list_items", json!({})),
        says("Done."),
    ]);
    let (assistant, host) = set_up_with(dir.path(), Some(mailer()), Place::Local, Arc::clone(&script), answers);
    host.hang.lock().expect("lock").push("mail.send_draft");
    let started = start(&assistant, "Send the draft, then sort my inbox").await;
    until(&assistant, &started.id, |_, steps| steps.first().is_some_and(|step| step.state == StepState::Dispatched))
        .await;
    let paused = assistant.pause_job(&started.id, JobReason::PausedByRequest).await;
    let Response::Job { job, steps } = paused else { panic!("expected the job, got {paused:?}") };
    assert_eq!((job.state, job.reason), (JobState::Paused, Some(JobReason::Uncertain)));
    assert_eq!(steps[0].state, StepState::Uncertain);
    // While the command still runs, nobody decides it: its outcome may still come in.
    assert_eq!(code(&assistant.resolve_job_step(&started.id, 1, false).await), "conflict");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(ran(&host, "inbox.list_items"), 0, "no step after the pause");
    assert_eq!(script.asked().len(), 1);
    let (conversation, _) = read(&assistant, &job.conversation, None).await;
    assert!(!conversation.running);

    // Cancelled, it stays uncertain in the journal and nothing more runs.
    let cancelled = assistant.cancel_job(&started.id).await;
    let Response::Job { job, steps } = cancelled else { panic!("expected the job, got {cancelled:?}") };
    assert_eq!((job.state, steps[0].state), (JobState::Cancelled, StepState::Uncertain));
    assert_eq!(code(&assistant.go_on_with(&started.id, None, None, None).await), "conflict");
}

#[tokio::test]
async fn a_stop_in_the_chat_pauses_the_job() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![Step::Hang(vec!["Working"])]);
    let (assistant, _host) = set_up_with(dir.path(), Some(reader()), Place::Local, script, answers);
    let started = start(&assistant, "Sort my inbox").await;
    assert_eq!(assistant.cancel(&started.conversation, chrono::Utc::now()).await, Response::Ok);
    let (job, _) = job(&assistant, &started.id).await;
    assert_eq!((job.state, job.reason), (JobState::Paused, Some(JobReason::Stopped)));
}

#[tokio::test]
async fn of_two_resumes_at_once_one_goes_on() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![Step::Hang(Vec::new())]);
    let (assistant, _host) = set_up_with(dir.path(), Some(reader()), Place::Local, Arc::clone(&script), answers);
    let started = start(&assistant, "Sort my inbox").await;
    assistant.pause_job(&started.id, JobReason::PausedByRequest).await;
    script.steps.lock().expect("lock").extend([says("Sorted."), says("Sorted again.")]);
    let (a, b) = tokio::join!(
        assistant.go_on_with(&started.id, None, None, None),
        assistant.go_on_with(&started.id, None, None, None)
    );
    let won = [&a, &b].iter().filter(|response| matches!(response, Response::Job { .. })).count();
    assert_eq!(won, 1, "{a:?} {b:?}");
    let lost = if matches!(a, Response::Job { .. }) { &b } else { &a };
    assert!(matches!(code(lost), "conflict" | "busy"), "{lost:?}");
    until(&assistant, &started.id, |job, _| job.state.is_final()).await;
    assert_eq!(messages(&assistant, &started.conversation).await.len(), 2, "the goal and one continuation");
}

#[tokio::test]
async fn a_normal_message_makes_no_job_and_a_deleted_conversation_takes_its_jobs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![says("Hello."), Step::Hang(Vec::new())]);
    let (assistant, _host) = set_up_with(dir.path(), Some(reader()), Place::Local, script, answers);
    let chat = ask(&assistant, None, "Hi", Vec::new()).await;
    for _ in 0..400 {
        if !read(&assistant, &chat, None).await.0.running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(assistant.list_jobs(None).await, Response::Jobs { jobs: Vec::new() });
    let started = start(&assistant, "Sort my inbox").await;
    assert_eq!(
        code(
            &assistant
                .launch_job("Again".into(), ENGINE.into(), None, Some(started.conversation.clone()), Vec::new(), None)
                .await
        ),
        "busy"
    );
    assert_eq!(assistant.delete(&started.conversation).await, Response::Ok);
    assert_eq!(assistant.list_jobs(None).await, Response::Jobs { jobs: Vec::new() });
    assert_eq!(code(&assistant.get_job(&started.id).await), "not-found");
    // The request is the catalogue's, so the core logs and checks it like any other of the assistant.
    let request = Request::GetJob { id: started.id };
    assert_eq!(request.operation().name(), "assistant.get_job");
}

#[tokio::test]
async fn an_approval_that_cannot_be_read_leaves_the_step_uncertain() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = Script::new(vec![calls("workspaces_delete_template", json!({"id": "old-plans"})), says("Done.")]);
    let (assistant, host) = set_up_with(dir.path(), Some(deleter()), Place::Local, Arc::clone(&script), answers);
    host.approvals_gone.store(true, std::sync::atomic::Ordering::SeqCst);
    let started = start(&assistant, "Delete my old plans").await;
    let (job, steps) = until(&assistant, &started.id, |job, _| job.state == JobState::Paused).await;
    assert_eq!((job.reason, steps[0].state), (Some(JobReason::Uncertain), StepState::Uncertain));
    assert_eq!(script.asked().len(), 1, "the job did not go on as if it had failed");
}
