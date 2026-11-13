//! Keeps the messages of one answer within the model's context window (issue #701, `docs/assistent.md`
//! "Kurzer Kontext"). Engine A sends every step the whole conversation: the person's messages, the answers, and
//! since issue #701 the tool calls of earlier turns with what they returned. A long task fills the window, and a
//! server that gets more than fits cuts the request itself, the rules of the system prompt first.
//!
//! [`compact`] shortens the messages in place, without asking the model, in stages until they fit
//! ([`estimate`]):
//!
//! 1. Results of older tool calls become a short note with their tool line's summary ("12 results").
//! 2. Whole steps between the first and the last message of the person, and between that last message and the
//!    newest steps, give way to a note that says how many steps were left out and which tools they called with
//!    their summaries, never what the tools returned.
//! 3. The results of the newest steps but the last one become the short note too.
//!
//! The system prompt, the first and the last message of the person and the last [`STEPS_KEPT`] steps stay word
//! for word through stage 2. Cuts fall only between steps, so every tool message still follows the assistant
//! message that called it (Anthropic refuses a `tool_use` without its `tool_result` right after it). A result
//! that was marked `untrusted` keeps the mark in its note, so the model still reads what follows as data. The
//! rules that hang on data from outside (a note waits for a person, the web asks again) go by the conversation's
//! tool lines, not by these messages, and stay as they were.
//!
//! Why no summary by the model: it costs a request and time, and it would carry text of a mail into words the
//! model then reads as its own.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use torro_core::ai::{ChatMessage, Role, ToolSpec};

use super::model::command_name;

/// What a left-out result says to the model.
pub const LEFT_OUT: &str = "Result left out to save room; call the tool again if you need it.";

/// The newest steps that stay word for word through stage 2.
const STEPS_KEPT: usize = 4;

/// At most this many calls are named in the note of a left-out stretch; the rest are counted.
const CALLS_NAMED: usize = 40;

/// About how many tokens `messages` and `tools` take: their characters (texts, the calls' names and arguments,
/// the tools' specification as JSON) by three, rounded down. Rough on purpose: servers count differently, and
/// half of the window ([`compact`]'s budget) leaves room for the error and for the answer.
pub fn estimate(messages: &[ChatMessage], tools: &[ToolSpec]) -> u64 {
    let texts: usize = messages
        .iter()
        .map(|message| {
            let calls: usize =
                message.tool_calls.iter().map(|call| call.name.chars().count() + call.arguments.chars().count()).sum();
            message.content.chars().count() + calls
        })
        .sum();
    let specs: usize =
        tools.iter().map(|tool| serde_json::to_string(tool).map_or(0, |spec| spec.chars().count())).sum();
    u64::try_from((texts + specs) / 3).unwrap_or(u64::MAX)
}

/// Shortens `messages` until [`estimate`] of them is at most `budget`, in the stages the module names;
/// `summaries` holds the summary of each call by its id ([`ChatMessage::tool_call_id`]). `true` when anything
/// changed: the caller then drops the thinking of the answer so far, as its blocks are bound to the messages before
/// them. After the last stage the messages are sent as they are, also when they still do not fit.
pub fn compact(messages: &mut Vec<ChatMessage>, summaries: &BTreeMap<String, String>, budget: u64) -> bool {
    let over = |messages: &[ChatMessage]| estimate(messages, &[]) > budget;
    if !over(messages) {
        return false;
    }
    let kept = kept_from(messages);
    let mut changed = thin(messages, summaries, 0, kept);
    if !over(messages) {
        return changed;
    }
    // Older turns first, then the older steps of the newest one.
    if let Some((from, to)) = between_people(messages) {
        changed |= leave_out(messages, summaries, from, to);
    }
    if !over(messages) {
        return changed;
    }
    if let Some((from, to)) = before_kept(messages) {
        changed |= leave_out(messages, summaries, from, to);
    }
    if !over(messages) {
        return changed;
    }
    let (kept, last) = (kept_from(messages), last_step(messages));
    changed | thin(messages, summaries, kept, last)
}

/// Whether `message` is a step: an assistant message that calls tools.
fn is_step(message: &ChatMessage) -> bool {
    message.role == Role::Assistant && !message.tool_calls.is_empty()
}

/// Where the last [`STEPS_KEPT`] steps start; the end of `messages` without a step.
fn kept_from(messages: &[ChatMessage]) -> usize {
    let steps: Vec<usize> = (0..messages.len()).filter(|at| is_step(&messages[*at])).collect();
    steps
        .len()
        .checked_sub(STEPS_KEPT)
        .map_or_else(|| steps.first().copied(), |first| steps.get(first).copied())
        .unwrap_or(messages.len())
}

/// Where the last step starts; the end of `messages` without one.
fn last_step(messages: &[ChatMessage]) -> usize {
    (0..messages.len()).rev().find(|at| is_step(&messages[*at])).unwrap_or(messages.len())
}

/// The messages of the person: their places in `messages`.
fn people(messages: &[ChatMessage]) -> Vec<usize> {
    (0..messages.len()).filter(|at| messages[*at].role == Role::User).collect()
}

/// The stretch after the first message of the person up to their last one (or the kept steps, when those start
/// before it); `None` when it is empty.
fn between_people(messages: &[ChatMessage]) -> Option<(usize, usize)> {
    let people = people(messages);
    let (first, last) = (*people.first()?, *people.last()?);
    let to = last.min(kept_from(messages));
    (first + 1 < to).then_some((first + 1, to))
}

/// The stretch after the last message of the person up to the kept steps; `None` when it is empty.
fn before_kept(messages: &[ChatMessage]) -> Option<(usize, usize)> {
    let last = *people(messages).last()?;
    let kept = kept_from(messages);
    (last + 1 < kept).then_some((last + 1, kept))
}

/// Replaces the result of every tool message in `from..to` with [`left_out`]; `true` when one got shorter.
fn thin(messages: &mut [ChatMessage], summaries: &BTreeMap<String, String>, from: usize, to: usize) -> bool {
    let mut changed = false;
    for message in messages.iter_mut().take(to).skip(from).filter(|message| message.role == Role::Tool) {
        let summary = message.tool_call_id.as_ref().and_then(|id| summaries.get(id));
        let note = left_out(&message.content, summary.map(String::as_str));
        if note.chars().count() < message.content.chars().count() {
            message.content = note;
            changed = true;
        }
    }
    changed
}

/// The note in place of the result `content`: [`LEFT_OUT`] with the tool line's `summary`, and `untrusted` when
/// the result was marked so, or cannot be read (then it could have been).
fn left_out(content: &str, summary: Option<&str>) -> String {
    let untrusted = serde_json::from_str::<Value>(content)
        .map_or(true, |value| value.get("untrusted").and_then(Value::as_bool) == Some(true));
    let mut note = json!({"note": LEFT_OUT, "summary": summary.unwrap_or_default()});
    if untrusted {
        note["untrusted"] = json!(true);
    }
    note.to_string()
}

/// Leaves out the messages `from..to` (whole steps: `to` is a message of the person or a step) and puts a
/// note ([`note_of`]) in their place: before the text of the assistant message at `to`, or as an assistant message
/// of its own before a message of the person, so the roles still take turns. `true` when it left out any.
fn leave_out(messages: &mut Vec<ChatMessage>, summaries: &BTreeMap<String, String>, from: usize, to: usize) -> bool {
    // A stretch that is only an earlier note stays: leaving it out again would lose what it said.
    if from >= to || to > messages.len() || messages[from..to].iter().all(is_note) {
        return false;
    }
    let gone: Vec<ChatMessage> = messages.drain(from..to).collect();
    let note = note_of(&gone, summaries);
    match messages.get_mut(from) {
        Some(next) if next.role == Role::Assistant => {
            next.content = match next.content.is_empty() {
                true => note,
                false => format!("{note}\n\n{}", next.content),
            };
        }
        _ => messages.insert(from, ChatMessage::assistant(note, Vec::new())),
    }
    true
}

/// How every note of [`note_of`] starts.
const NOTE: &str = "[Left out to save room:";

/// The note at the start of `text` ([`leave_out`] puts it before the text with a blank line), and the rest.
fn split_note(text: &str) -> (Option<&str>, &str) {
    if !text.starts_with(NOTE) {
        return (None, text);
    }
    match text.split_once("\n\n") {
        Some((note, rest)) => (Some(note), rest),
        None => (Some(text), ""),
    }
}

/// Whether `message` is an earlier note of its own and nothing else.
fn is_note(message: &ChatMessage) -> bool {
    message.role == Role::Assistant
        && message.tool_calls.is_empty()
        && matches!(split_note(&message.content), (Some(_), ""))
}

/// What a left-out stretch was: how many messages of the person and steps, and each call by its catalogue name
/// with its summary, after the notes of stretches left out before (they stay word for word, so nothing they said
/// gets lost). Never a result, a text or an argument: those can hold what a mail said.
fn note_of(gone: &[ChatMessage], summaries: &BTreeMap<String, String>) -> String {
    // Only the assistant's own notes: a message of the person that starts like one is their text, not a note.
    let earlier: Vec<&str> = gone
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .filter_map(|message| split_note(&message.content).0)
        .collect();
    let people = gone.iter().filter(|message| message.role == Role::User).count();
    let steps = gone.iter().filter(|message| is_step(message)).count();
    let calls: Vec<String> = gone
        .iter()
        .flat_map(|message| &message.tool_calls)
        .map(|call| {
            let name = command_name(&call.name);
            match summaries.get(&call.id) {
                Some(summary) => format!("{name} ({summary})"),
                None => name,
            }
        })
        .collect();
    let mut note = earlier.iter().map(|note| format!("{note} ")).collect::<String>();
    note.push_str(&format!("{NOTE} {steps} steps of the assistant"));
    if people > 0 {
        note.push_str(&format!(" and {people} messages of the person"));
    }
    if !calls.is_empty() {
        let named = calls.iter().take(CALLS_NAMED).cloned().collect::<Vec<_>>().join("; ");
        note.push_str(&format!(". Calls: {named}"));
        if calls.len() > CALLS_NAMED {
            note.push_str(&format!("; and {} more", calls.len() - CALLS_NAMED));
        }
        note.push_str(". Call a tool again if you need its result");
    }
    note.push_str(".]");
    note
}

#[cfg(test)]
mod tests {
    use torro_core::ai::ToolCall;

    use super::*;

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall { id: id.into(), name: name.into(), arguments: "{}".into() }
    }

    /// A step: an assistant message with `text` that calls `mail_search_messages` as `id`, and its result.
    fn step(id: &str, text: &str, result: &str) -> [ChatMessage; 2] {
        [ChatMessage::assistant(text, vec![call(id, "mail_search_messages")]), ChatMessage::tool(id, result)]
    }

    /// A conversation of three turns: two earlier ones with a step each, and the newest one with `steps` steps.
    fn conversation(steps: usize) -> Vec<ChatMessage> {
        let big = |n: usize| format!("{{\"messages\":[\"{}\"]}}", format!("mail text {n} ").repeat(50));
        let mut messages = vec![ChatMessage::system("rules"), ChatMessage::user("first question")];
        messages.extend(step("call_2", "", &big(2)));
        messages.push(ChatMessage::assistant("first answer", Vec::new()));
        messages.push(ChatMessage::user("second question"));
        messages.extend(step("call_6", "", &big(6)));
        messages.push(ChatMessage::assistant("second answer", Vec::new()));
        messages.push(ChatMessage::user("last question"));
        for n in 0..steps {
            messages.extend(step(&format!("call_now_{n}"), &format!("step {n}"), &big(100 + n)));
        }
        messages
    }

    fn summaries() -> BTreeMap<String, String> {
        let mut summaries = BTreeMap::from([("call_2".to_string(), "2 results".to_string())]);
        summaries.insert("call_6".into(), "6 results".into());
        summaries
    }

    /// Every tool message follows the step that called it, with nothing between.
    fn calls_answered(messages: &[ChatMessage]) {
        let mut open: Vec<String> = Vec::new();
        for message in messages {
            match message.role {
                Role::Tool => {
                    let id = message.tool_call_id.clone().expect("an id");
                    assert_eq!(open.first(), Some(&id), "{messages:#?}");
                    open.remove(0);
                }
                _ => {
                    assert!(open.is_empty(), "a call without its result: {messages:#?}");
                    open = message.tool_calls.iter().map(|call| call.id.clone()).collect();
                }
            }
        }
    }

    #[test]
    fn what_fits_stays_as_it_is() {
        let mut messages = conversation(2);
        let before = messages.clone();
        assert!(!compact(&mut messages, &summaries(), estimate(&before, &[])));
        assert_eq!(messages, before);
    }

    #[test]
    fn the_estimate_counts_texts_calls_and_tools() {
        let messages = vec![ChatMessage::user("abcdef"), ChatMessage::assistant("", vec![call("x", "abc")])];
        // Six characters of text, three of the name and two of the arguments, by three.
        assert_eq!(estimate(&messages, &[]), 3);
        let tool = ToolSpec { name: "a".into(), description: "b".into(), parameters: json!({}) };
        assert!(estimate(&[], &[tool]) > 0);
    }

    /// Stage 1: older results become notes with their summary; the newest steps stay.
    #[test]
    fn older_results_give_way_first() {
        let mut messages = conversation(4);
        let before = messages.clone();
        let budget = estimate(&before, &[]) - 100;
        assert!(compact(&mut messages, &summaries(), budget));
        assert_eq!(messages.len(), before.len(), "nothing left out yet");
        let old: Value = serde_json::from_str(&messages[3].content).expect("a note");
        assert_eq!(old, json!({"note": LEFT_OUT, "summary": "2 results"}));
        assert_eq!(messages[messages.len() - 8..], before[before.len() - 8..], "the last four steps word for word");
        calls_answered(&messages);
    }

    /// Stage 2: whole steps give way to a note that names the calls and their summaries, never their results; the
    /// first and the last message of the person and the newest steps stay word for word.
    #[test]
    fn older_steps_give_way_to_a_note() {
        let mut messages = conversation(7);
        let before = messages.clone();
        let budget = estimate(&before[before.len() - 8..], &[]) + 150;
        assert!(compact(&mut messages, &summaries(), budget));
        calls_answered(&messages);
        let texts: Vec<&str> = messages.iter().map(|message| message.content.as_str()).collect();
        assert_eq!(texts[..2], ["rules", "first question"]);
        assert!(texts.contains(&"last question"));
        assert!(!texts.contains(&"second question"), "{texts:#?}");
        assert_eq!(messages[messages.len() - 7..], before[before.len() - 7..], "the last steps word for word");
        let notes: Vec<&ChatMessage> =
            messages.iter().filter(|message| message.content.starts_with("[Left out to save room")).collect();
        assert_eq!(notes.len(), 2, "{texts:#?}");
        assert!(notes[0].content.contains("mail.search_messages (6 results)"), "{}", notes[0].content);
        assert!(notes[0].content.contains("1 messages of the person"), "{}", notes[0].content);
        for message in &messages {
            assert!(!message.content.contains("mail text 6") && !message.content.contains("mail text 100"));
        }
        // The note of the newest turn goes before the text of the first kept step.
        let kept = messages.iter().position(|message| message.content.ends_with("step 3")).expect("kept step");
        assert!(messages[kept].content.starts_with("[Left out to save room: 3 steps"), "{}", messages[kept].content);
        // The roles still take turns: no two messages of the person or of the assistant in a row.
        for pair in messages.windows(2) {
            assert!(!(pair[0].role == pair[1].role && pair[0].role != Role::Tool), "{texts:#?}");
        }
    }

    /// Compacting again keeps what an earlier note said: a note alone stays, and one in a stretch left out later is
    /// carried into the new note.
    #[test]
    fn earlier_notes_are_kept() {
        let mut messages = conversation(7);
        assert!(compact(&mut messages, &summaries(), 1));
        let first: Vec<String> =
            messages.iter().filter(|message| message.content.starts_with(NOTE)).map(|m| m.content.clone()).collect();
        // Two more steps: the kept ones move on, and the step that carried a note is left out.
        messages.extend(step("call_more_1", "more 1", "{\"a\":1}"));
        messages.extend(step("call_more_2", "more 2", "{\"a\":2}"));
        assert!(compact(&mut messages, &summaries(), 1));
        calls_answered(&messages);
        let notes: Vec<&str> =
            messages.iter().filter(|message| message.content.starts_with(NOTE)).map(|m| m.content.as_str()).collect();
        assert_eq!(notes.len(), 2, "{notes:#?}");
        assert_eq!(notes[0], first[0], "the note of the older turns as it was");
        assert!(
            notes[1].contains("mail.search_messages")
                && notes[1].starts_with(first[1].split("\n\n").next().unwrap_or_default()),
            "{notes:#?}"
        );
        assert!(notes[1].contains("[Left out to save room: 2 steps"), "{notes:#?}");
    }

    /// Stage 3 thins the kept steps but the last; what then still does not fit is sent as it is.
    #[test]
    fn at_the_end_only_the_last_step_keeps_its_result() {
        let mut messages = conversation(5);
        assert!(compact(&mut messages, &summaries(), 1));
        calls_answered(&messages);
        let results: Vec<&ChatMessage> = messages.iter().filter(|message| message.role == Role::Tool).collect();
        let (last, rest) = results.split_last().expect("results");
        assert!(last.content.contains("mail text 104"), "the last step's result stays");
        assert!(rest.iter().all(|result| result.content.contains(LEFT_OUT)), "{rest:#?}");
        assert_eq!(messages[1].content, "first question");
    }

    /// A result marked `untrusted` stays marked in its note; one that does not read counts as untrusted.
    #[test]
    fn untrusted_results_stay_marked() {
        let marked = json!({"untrusted": true, "note": "data", "data": {"text": "x".repeat(400)}}).to_string();
        let note: Value = serde_json::from_str(&left_out(&marked, Some("1 results"))).expect("json");
        assert_eq!(note["untrusted"], true);
        let plain: Value = serde_json::from_str(&left_out("{\"a\":1}", None)).expect("json");
        assert!(plain.get("untrusted").is_none());
        let broken: Value = serde_json::from_str(&left_out("not json", None)).expect("json");
        assert_eq!(broken["untrusted"], true);
    }
}
