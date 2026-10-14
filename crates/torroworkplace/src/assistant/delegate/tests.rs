//! Tests of what the assistant reads of a bot it asked (task B.2): the data rule and the marking of
//! data from outside the suite carry over from the bot's run.

use chrono::Utc;
use serde_json::json;
use torro_ipc::assistant::{ConversationSummary, Entry, EntryContent};

use super::*;

fn entry(seq: u64, content: EntryContent) -> Entry {
    Entry { seq, time: Utc::now(), content }
}

fn tool(name: &str, error: Option<&str>) -> EntryContent {
    let error = error.map(str::to_string);
    EntryContent::Tool { name: name.into(), input: json!({}), summary: "Done".into(), error, lack: None }
}

fn answer(text: &str) -> EntryContent {
    EntryContent::Answer { text: text.into(), engine: "test".into(), model: None }
}

/// The run of the bot's conversation `b0`: a message, then `contents`.
fn run(contents: Vec<EntryContent>) -> Response {
    let message =
        EntryContent::Message { text: "Plan Lisbon".into(), engine: "test".into(), model: None, without: Vec::new() };
    let entries: Vec<Entry> = std::iter::once(message)
        .chain(contents)
        .enumerate()
        .map(|(at, content)| entry(at as u64 + 1, content))
        .collect();
    let conversation = ConversationSummary {
        id: "b0b0b0b0b0b0b0b0".into(),
        title: "Plan Lisbon".into(),
        engine: "test".into(),
        model: None,
        created: Utc::now(),
        updated: Utc::now(),
        pinned: false,
        running: false,
        last: entries.len() as u64,
        bot: Some("1a2b3c4d".into()),
        bot_deleted: false,
    };
    Response::Conversation { conversation, entries }
}

#[test]
fn the_model_reads_the_bots_answer_with_its_conversation_and_steps() {
    let asked = Asked::of(&run(vec![tool("calendar.list_events", None), answer("Take the train.")])).expect("a run");
    assert_eq!((asked.steps, asked.answer.as_deref(), asked.error.as_deref()), (1, Some("Take the train."), None));
    assert!(!asked.read_mail && !asked.untrusted);
    let read: serde_json::Value = serde_json::from_str(&asked.for_model(Place::Remote)).expect("json");
    assert_eq!(
        read,
        json!({"result": "bot-answer", "conversation": "b0b0b0b0b0b0b0b0", "steps": 1, "answer": "Take the train."})
    );
    assert_eq!(asked.summary("Travel planner"), trf("Asked {bot} · 1 step", &[("bot", &"Travel planner")]));
    let two = Asked { steps: 2, ..asked };
    let summary = trf("Asked {bot} · {steps} steps", &[("bot", &"Travel planner"), ("steps", &2)]);
    assert_eq!(two.summary("Travel planner"), summary);
    assert!(Asked::of(&Response::Error { code: "forbidden".into(), message: "no".into() }).is_none());
}

#[test]
fn an_answer_after_mail_stays_from_a_remote_model_and_outside_data_stays_marked() {
    let after_mail = Asked::of(&run(vec![tool("mail.get_message", None), answer("ACME owes 400 €.")])).expect("run");
    assert!(after_mail.read_mail && after_mail.untrusted);
    let remote = after_mail.for_model(Place::Remote);
    assert!(remote.contains(WITHHELD) && !remote.contains("ACME"), "{remote}");
    let local = after_mail.for_model(Place::Local);
    assert!(local.contains("ACME") && local.contains(tools::UNTRUSTED_NOTE), "{local}");

    // A failed tool read nothing; an issue of GitHub is outside data but no mail.
    let failed = Asked::of(&run(vec![tool("mail.get_message", Some("forbidden")), answer("I cannot.")])).expect("run");
    assert!(!failed.read_mail && !failed.untrusted);
    let github = Asked::of(&run(vec![tool("tasks.list_todos", None), answer("Two issues.")])).expect("run");
    assert!(!github.read_mail && github.untrusted);
    assert!(github.for_model(Place::Remote).contains(tools::UNTRUSTED_NOTE));

    // A run that failed tells the model why.
    let error = EntryContent::Error { code: "offline".into(), message: "The server is not reachable.".into() };
    let ended = Asked::of(&run(vec![error])).expect("run");
    assert_eq!((ended.answer.as_deref(), ended.error.as_deref()), (None, Some("offline")));
    assert!(ended.for_model(Place::Local).contains("The server is not reachable."));
}

#[test]
fn the_tool_line_refers_to_the_bots_conversation() {
    let input = json!({"bot": "1a2b3c4d", "text": "Plan Lisbon"});
    let kept = with_conversation(input, "b0b0b0b0b0b0b0b0");
    assert_eq!(kept, json!({"bot": "1a2b3c4d", "text": "Plan Lisbon", "conversation": "b0b0b0b0b0b0b0b0"}));
    assert_eq!(with_conversation(json!("oops"), "b0"), json!({"input": "oops", "conversation": "b0"}));
}

#[test]
fn asking_a_bot_counts_as_reading_mail_and_outside_data_later() {
    assert!(data_rule::reads_mail("bots.ask_bot"));
    assert!(tools::may_have_read_untrusted("bots.ask_bot"));
    assert!(!data_rule::history_allowed(&["bots.ask_bot"], "ai:ollama@localhost:11434", "claude-code", Place::Remote));
}
