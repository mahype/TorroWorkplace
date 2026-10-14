//! The data rule of the assistant (task C.2, `docs/ki.md` principle 4): which results of its tools
//! a model may read. Mail stays with models on the person's own computers unless a rule says
//! otherwise; a model elsewhere reads "withheld by your data rule" instead.
//!
//! Per-source rules ("GitHub may go to the cloud, Messenger only local") are task KI.1. Until then
//! the fixed default holds, and every decision goes through the functions here, so KI.1 replaces
//! them and nothing else:
//!
//! - [`place_of`]: where an AI account's model runs ([`Place`]);
//! - [`for_model`]: a tool's answer as a model at a place may read it;
//! - [`history_allowed`]: whether an earlier answer of the conversation may go to the model.
//!
//! What counts as mail content: every answer of a `mail.*` command but those that carry only what
//! the assistant wrote or the folders' names ([`MAIL_ANSWERS_ALLOWED`]), the inbox entries whose
//! source is a mail account, the senders and domains in the inbox's rules, what hiding a sender
//! answers (its rule names the sender), and the task made from an inbox entry (its title can be the
//! subject of a mail; the core does not tell which source the entry had, so it is withheld for
//! every entry). The approvals, whose summaries and answers can quote mail, are no tool of the
//! assistant at all (`tools::callable`).
//!
//! The assistant's memory (task C.10): a note a model on this computer kept after the conversation
//! read mail is marked `local_only` (`super::model`), and a model elsewhere never reads such a note –
//! neither in its prompt (`super::prompt`) nor in what `assistant.list_memories` answers
//! ([`for_model`]).
//!
//! Known limit, for KI.1: a task made from a mail entry keeps the subject as its title, and
//! `tasks.list_todos` shows it to every model; tasks are the person's own data from then on.

use serde_json::{Value, json};
use torro_core::ai::{AiProvider, is_this_computer};
use torro_inbox::{Item, Rule, SourceKind};
use torro_ipc::api::Area;
use torro_ipc::assistant::Note;
use torro_ipc::{Request, Response};

/// What a model reads in place of content its data rule keeps from it. English: it is for the
/// model, not for people.
pub const WITHHELD: &str = "withheld by your data rule";

/// Where the model of an AI account runs, as far as the data rule cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// On a computer of the person: a local server (Ollama, LM Studio), or another OpenAI-compatible
    /// server at an address of this computer.
    Local,
    /// Anywhere else: OpenRouter, a server of a provider, a server in the network that is no
    /// Ollama or LM Studio.
    Remote,
}

/// Where the model of an AI account of `provider` at the base address `server` runs: Ollama and
/// LM Studio are local wherever they run (they run on the person's own machines), OpenRouter never
/// is (an address of this computer would be a proxy to it), any other server only at an address of
/// this computer (`localhost`, `127.0.0.1`, `::1`).
pub fn place_of(provider: AiProvider, server: &str) -> Place {
    match provider {
        AiProvider::Ollama | AiProvider::LmStudio => Place::Local,
        AiProvider::OpenAiCompatible if is_this_computer(server) => Place::Local,
        AiProvider::OpenRouter | AiProvider::OpenAiCompatible => Place::Remote,
    }
}

/// The answers of `mail.*` commands that hold no mail content: what the assistant wrote itself
/// (a draft, that it was sent), the folders' names, `ok` and errors.
const MAIL_ANSWERS_ALLOWED: &[&str] = &["ok", "mail-draft", "mail-sent", "mail-folders", "error"];

/// A tool's answer as a model may read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Screened {
    /// The answer as JSON, with what the rule keeps from the model replaced by [`WITHHELD`].
    pub value: Value,
    /// Something was withheld: the conversation's tool line says so.
    pub withheld: bool,
}

/// `response` of `request` as a model at `place` may read it ([`Place`]): unchanged for a local
/// model; for one elsewhere without mail content (see the module's documentation). A whole answer
/// withheld becomes `{"withheld": WITHHELD}`, an inbox entry of a mail account
/// `{"id": …, "source": …, "withheld": WITHHELD}`, so the model can still name it.
pub fn for_model(request: &Request, response: &Response, place: Place) -> Screened {
    let value = serde_json::to_value(response).unwrap_or_else(|err| json!({"error": err.to_string()}));
    if place == Place::Local {
        return Screened { value, withheld: false };
    }
    let result = value.get("result").and_then(Value::as_str).unwrap_or_default();
    let whole = match request {
        request if request.operation().area() == Area::Mail => !MAIL_ANSWERS_ALLOWED.contains(&result),
        Request::HideSender { .. } | Request::CreateTaskFromItem { .. } => result != "error",
        _ => false,
    };
    if whole {
        return Screened { value: json!({ "withheld": WITHHELD }), withheld: true };
    }
    match response {
        Response::Items { items } => {
            let shown: Vec<Value> = items.iter().map(item_for_remote).collect();
            let withheld = items.iter().any(from_mail);
            Screened { value: json!({ "result": result, "items": shown }), withheld }
        }
        Response::Item { item } => {
            Screened { value: json!({ "result": result, "item": item_for_remote(item) }), withheld: from_mail(item) }
        }
        Response::Rules { rules } => {
            let shown: Vec<Value> = rules.iter().map(rule_for_remote).collect();
            let withheld = rules.iter().any(|rule| rule.sender.is_some() || rule.domain.is_some());
            Screened { value: json!({ "result": result, "rules": shown }), withheld }
        }
        // Notes a local model kept after reading mail (task C.10).
        Response::Memories { notes } => {
            let shown: Vec<&Note> = notes.iter().filter(|note| !note.local_only).collect();
            let withheld = shown.len() < notes.len();
            Screened { value: json!({ "result": result, "notes": shown }), withheld }
        }
        Response::Memory { note } if note.local_only => Screened {
            value: json!({ "result": result, "note": { "id": note.id, "withheld": WITHHELD } }),
            withheld: true,
        },
        _ => Screened { value, withheld: false },
    }
}

/// Whether the command `name` (a catalogue name) reads mail content or may: every `mail.*` and
/// `inbox.*` command. An answer after one in the same turn may retell a mail.
pub fn reads_mail(name: &str) -> bool {
    name.starts_with("mail.") || name.starts_with("inbox.")
}

/// An inbox entry from a mail account is mail content.
fn from_mail(item: &Item) -> bool {
    item.source.kind == SourceKind::Mail
}

/// `item` for a model elsewhere: as it is, or for an entry of a mail account only its id and
/// source.
fn item_for_remote(item: &Item) -> Value {
    if from_mail(item) {
        return json!({ "id": item.id, "source": item.source.key(), "withheld": WITHHELD });
    }
    serde_json::to_value(item).unwrap_or(Value::Null)
}

/// A rule of the inbox for a model elsewhere: the sender and domain it names are a mail's.
fn rule_for_remote(rule: &Rule) -> Value {
    let mut value = serde_json::to_value(rule).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        for field in ["sender", "domain"] {
            if let Some(named) = object.get_mut(field) {
                *named = json!(WITHHELD);
            }
        }
    }
    value
}

/// Whether an earlier answer of the conversation, written by `answered_by` in a turn that used the
/// commands `commands` (catalogue names), may go to the model of `engine` at `place`.
///
/// A local model reads everything. A model elsewhere reads earlier answers unless that turn read
/// mail or the inbox and another engine wrote the answer: a local model may have retold a mail
/// there. Its own answers never held mail content (it read [`WITHHELD`] instead), and of another
/// engine the core may no longer know where it ran, so those are withheld.
pub fn history_allowed(commands: &[&str], answered_by: &str, engine: &str, place: Place) -> bool {
    let read_mail = commands.iter().any(|name| reads_mail(name));
    place == Place::Local || !read_mail || answered_by == engine
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use torro_inbox::{Kind, Lane, SourceRef, State};
    use torro_ipc::mail::MailSource;
    use torro_mail::MessageSummary;

    use super::*;

    fn item(id: &str, source: SourceRef, title: &str) -> Item {
        let time = Utc.with_ymd_and_hms(2026, 10, 13, 9, 0, 0).single().expect("valid time");
        Item {
            id: id.into(),
            source,
            thread_key: "t".into(),
            kind: Kind::Notice,
            lane: Lane::ForMe,
            urgent: false,
            title: title.into(),
            body: None,
            from: None,
            agent: None,
            received: time,
            updated: time,
            count: 1,
            state: State::New,
            link: None,
            actions: Vec::new(),
            untrusted: true,
        }
    }

    fn mail_source() -> SourceRef {
        SourceRef { kind: SourceKind::Mail, name: "anna@example.org".into() }
    }

    fn agent_source() -> SourceRef {
        SourceRef { kind: SourceKind::Agent, name: "claude-code".into() }
    }

    #[test]
    fn local_servers_are_ollama_lm_studio_and_this_computer() {
        assert_eq!(place_of(AiProvider::Ollama, "http://localhost:11434/v1"), Place::Local);
        assert_eq!(place_of(AiProvider::Ollama, "http://gpu.local:11434/v1"), Place::Local);
        assert_eq!(place_of(AiProvider::LmStudio, "http://localhost:1234/v1"), Place::Local);
        assert_eq!(place_of(AiProvider::OpenAiCompatible, "http://127.0.0.1:8000/v1"), Place::Local);
        assert_eq!(place_of(AiProvider::OpenAiCompatible, "http://[::1]:8000/v1"), Place::Local);
        assert_eq!(place_of(AiProvider::OpenAiCompatible, "https://ai.example.org/v1"), Place::Remote);
        assert_eq!(place_of(AiProvider::OpenRouter, "https://openrouter.ai/api/v1"), Place::Remote);
        assert_eq!(place_of(AiProvider::OpenRouter, "http://localhost:8080/api/v1"), Place::Remote, "a proxy");
    }

    #[test]
    fn mail_goes_to_local_models_only() {
        let search: Request =
            serde_json::from_value(json!({"method": "mail.search_messages", "limit": 10})).expect("a search");
        let page =
            Response::MailMessages { messages: Vec::<MessageSummary>::new(), cursor: None, source: MailSource::Cache };
        let local = for_model(&search, &page, Place::Local);
        assert!(!local.withheld);
        assert_eq!(local.value["result"], "mail-messages");
        let remote = for_model(&search, &page, Place::Remote);
        assert!(remote.withheld);
        assert_eq!(remote.value, json!({ "withheld": WITHHELD }));
        // Errors, and what the assistant wrote itself, still reach it.
        let error = Response::Error { code: "not-found".into(), message: "no such message".into() };
        assert_eq!(for_model(&search, &error, Place::Remote).value["result"], "error");
    }

    #[test]
    fn inbox_entries_of_mail_accounts_are_withheld_one_by_one() {
        let list: Request = serde_json::from_value(json!({"method": "inbox.list_items"})).expect("a list");
        let items = Response::Items {
            items: vec![item("a1", mail_source(), "Invoice 42"), item("b2", agent_source(), "Build finished")],
        };
        let remote = for_model(&list, &items, Place::Remote);
        assert!(remote.withheld);
        let text = remote.value.to_string();
        assert!(!text.contains("Invoice") && text.contains("Build finished") && text.contains("a1"), "{text}");
        assert_eq!(remote.value["items"][0]["withheld"], WITHHELD);
        assert_eq!(remote.value["result"], "items");

        let local = for_model(&list, &items, Place::Local);
        assert!(!local.withheld && local.value.to_string().contains("Invoice"));

        let one = Response::Item { item: item("b2", agent_source(), "Build finished") };
        assert!(!for_model(&Request::GetItem { id: "b2".into() }, &one, Place::Remote).withheld);
    }

    #[test]
    fn the_senders_in_the_inbox_rules_are_withheld() {
        let rule = Rule {
            id: "r1".into(),
            source: mail_source(),
            sender: Some("boss@acme.example".into()),
            domain: None,
            effect: torro_inbox::RuleEffect::Muted,
            created: Utc.with_ymd_and_hms(2026, 10, 13, 9, 0, 0).single().expect("valid time"),
        };
        let quiet = Rule { id: "r2".into(), source: agent_source(), sender: None, ..rule.clone() };
        let rules = Response::Rules { rules: vec![rule, quiet] };
        let list: Request = serde_json::from_value(json!({"method": "inbox.list_rules"})).expect("rules");
        let remote = for_model(&list, &rules, Place::Remote);
        let text = remote.value.to_string();
        assert!(remote.withheld && !text.contains("acme") && text.contains("r1") && text.contains("r2"), "{text}");
        assert!(for_model(&list, &rules, Place::Local).value.to_string().contains("acme"));
    }

    /// Task C.10: notes a local model kept after reading mail stay with local models.
    #[test]
    fn notes_kept_after_mail_are_withheld_from_remote_models() {
        let created = Utc.with_ymd_and_hms(2026, 10, 14, 9, 0, 0).single().expect("valid time");
        let note = |id: &str, text: &str, local_only| Note { id: id.into(), text: text.into(), created, local_only };
        let notes = Response::Memories {
            notes: vec![note("1a2b3c4d", "Anna works in sales.", false), note("5e6f7a8b", "ACME owes 400 €.", true)],
        };
        let list = Request::ListMemories;
        let remote = for_model(&list, &notes, Place::Remote);
        let text = remote.value.to_string();
        assert!(remote.withheld && !text.contains("ACME") && !text.contains("5e6f7a8b") && text.contains("Anna"));
        assert!(for_model(&list, &notes, Place::Local).value.to_string().contains("ACME"));

        let remember = Request::Remember { text: "x".into(), replaces: None, local_only: true };
        let kept = Response::Memory { note: note("5e6f7a8b", "ACME owes 400 €.", true) };
        let screened = for_model(&remember, &kept, Place::Remote);
        assert!(screened.withheld && !screened.value.to_string().contains("ACME"));
        assert!(reads_mail("mail.get_message") && reads_mail("inbox.list_items") && !reads_mail("tasks.list_todos"));
    }

    #[test]
    fn earlier_answers_after_mail_stay_with_the_engine_that_wrote_them() {
        let (ollama, openrouter) = ("ai:ollama@localhost:11434", "ai:openrouter@openrouter.ai");
        let read_mail = ["calendar.list_events", "mail.get_message"];
        assert!(!history_allowed(&read_mail, ollama, openrouter, Place::Remote));
        assert!(history_allowed(&read_mail, openrouter, openrouter, Place::Remote), "it read the withheld text");
        assert!(history_allowed(&read_mail, openrouter, ollama, Place::Local));
        assert!(history_allowed(&["calendar.list_events"], ollama, openrouter, Place::Remote));
        assert!(!history_allowed(&["inbox.list_items"], ollama, openrouter, Place::Remote));
    }
}
