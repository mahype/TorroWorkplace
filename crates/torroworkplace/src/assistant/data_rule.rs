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
//! every entry). Since the three inbox windows (step E8): the values of rules by trait (`facet`),
//! the ways to mute an entry and what muting answers, the statistics with their suggestions, a task
//! made of a mail in a folder, and the mail's subject and sender in a task's link (`link.mail`). The approvals, whose summaries and answers can quote mail, are no tool of the
//! assistant at all (`tools::callable`). The event made from an inbox entry (task E8.5b) is
//! withheld like its task: its title and description can be a mail's subject and sender. The answer
//! to an invitation (`mail.respond_to_invitation`, task E8.3b) is a `mail.*` answer with the event
//! made of the invitation in it, so it is withheld as well; only its errors go through. Quiet
//! (`inbox.get_quiet`) names a calendar event at most, no mail.
//!
//! Known limit, for KI.1: a task made from a mail entry keeps the subject as its title, and
//! `tasks.list_todos` shows it to every model; tasks are the person's own data from then on.

use serde_json::{Value, json};
use torro_core::ai::{AiProvider, is_this_computer};
use torro_inbox::{Item, Rule, SourceKind};
use torro_ipc::api::Area;
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
        // Muting by trait and the statistics name senders, domains and lists of mail (tasks E8.6a,
        // E8.8a); a task made of a mail takes its subject as title (task E8.7a), and so does an
        // event made of an entry, with the line naming the mail in its description (task E8.5b).
        Request::HideSender { .. }
        | Request::CreateTaskFromItem { .. }
        | Request::CreateEventFromItem { .. }
        | Request::ListMuteOptions { .. }
        | Request::MuteItem { .. }
        | Request::GetStats { .. }
        | Request::CreateTodoFromMail { .. } => result != "error",
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
        // The hits per rule (task E8.8a) stay home: counts of what a rule caught say nothing a model
        // needs. A rule by trait (task E8.6a) can name a mail's sender, domain or list in `facet`.
        Response::Rules { rules, .. } => {
            let shown: Vec<Value> = rules.iter().map(rule_for_remote).collect();
            let withheld =
                rules.iter().any(|rule| rule.sender.is_some() || rule.domain.is_some() || rule.facet.is_some());
            Screened { value: json!({ "result": result, "rules": shown }), withheld }
        }
        // A task made of a mail carries the mail's subject and sender in `link.mail` (task E8.7a).
        Response::Todos { todos } => {
            let shown: Vec<Value> = todos.iter().map(todo_for_remote).collect();
            let withheld = todos.iter().any(names_a_mail);
            Screened { value: json!({ "result": result, "todos": shown }), withheld }
        }
        Response::Todo { todo } => {
            Screened { value: json!({ "result": result, "todo": todo_for_remote(todo) }), withheld: names_a_mail(todo) }
        }
        _ => Screened { value, withheld: false },
    }
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

/// Whether `todo` shows what a mail holds: its link names a mail and the core filled in the mail's
/// subject and sender (task E8.7a).
fn names_a_mail(todo: &torro_pim::Todo) -> bool {
    todo.link.as_ref().is_some_and(|link| link.mail.is_some())
}

/// `todo` for a model elsewhere: as it is, with the mail's details in its link withheld.
fn todo_for_remote(todo: &torro_pim::Todo) -> Value {
    let mut value = serde_json::to_value(todo).unwrap_or(Value::Null);
    if names_a_mail(todo)
        && let Some(mail) = value.get_mut("link").and_then(|link| link.get_mut("mail"))
    {
        *mail = json!(WITHHELD);
    }
    value
}

/// A rule of the inbox for a model elsewhere: the sender and domain it names are a mail's, and so
/// can be the value of its trait (`facet`, task E8.6a: a sender, a domain, a list), so all three are
/// withheld.
fn rule_for_remote(rule: &Rule) -> Value {
    let mut value = serde_json::to_value(rule).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        for field in ["sender", "domain", "facet"] {
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
    let read_mail = commands.iter().any(|name| name.starts_with("mail.") || name.starts_with("inbox."));
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
            facets: Vec::new(),
            task: None,
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
    fn an_answered_invitation_goes_to_local_models_only() {
        // Task E8.3b: the answer carries the event made of the invitation (its title, text and the
        // line naming the mail); a model elsewhere reads that something was withheld, or the error.
        let answer: Request = serde_json::from_value(json!({
            "method": "mail.respond_to_invitation", "account": "mail:anna@example.org", "id": "m:1", "answer": "accept"
        }))
        .expect("an answer");
        let start = torro_pim::EventTime::DateTime(chrono::Utc::now());
        let event = torro_pim::Event {
            uid: "planning-1".into(),
            collection: "work".into(),
            href: None,
            etag: None,
            summary: "Salary talk with Jonas".into(),
            start,
            end: None,
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
        };
        let answered =
            Response::InvitationAnswered { delivery: torro_ipc::mail::Delivery::Accepted, event: Some(event) };
        assert!(!for_model(&answer, &answered, Place::Local).withheld);
        let remote = for_model(&answer, &answered, Place::Remote);
        assert_eq!((remote.withheld, remote.value.clone()), (true, json!({ "withheld": WITHHELD })));
        let refused = Response::Error { code: "forbidden".into(), message: "no sending".into() };
        assert_eq!(for_model(&answer, &refused, Place::Remote).value["result"], "error");
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
            facet: None,
            until: None,
        };
        let quiet = Rule { id: "r2".into(), source: agent_source(), sender: None, ..rule.clone() };
        // A rule by trait (task E8.6a) names the list in its facet; that is a mail's too.
        let list_rule = Rule {
            id: "r3".into(),
            sender: None,
            facet: Some(torro_inbox::item::Facet {
                key: torro_inbox::item::FacetKey::List,
                value: "news.lists.acme.example".into(),
            }),
            ..rule.clone()
        };
        let rules = Response::Rules { rules: vec![rule, quiet, list_rule], hits: Vec::new() };
        let list: Request = serde_json::from_value(json!({"method": "inbox.list_rules"})).expect("rules");
        let remote = for_model(&list, &rules, Place::Remote);
        let text = remote.value.to_string();
        assert!(remote.withheld && !text.contains("acme") && text.contains("r1") && text.contains("r3"), "{text}");
        assert!(for_model(&list, &rules, Place::Local).value.to_string().contains("acme"));
        let only_agents = Response::Rules { rules: vec![quiet_rule(&rules)], hits: Vec::new() };
        assert!(!for_model(&list, &only_agents, Place::Remote).withheld, "an agent's rule names no mail");
    }

    fn quiet_rule(rules: &Response) -> Rule {
        let Response::Rules { rules, .. } = rules else { panic!("rules") };
        rules[1].clone()
    }

    #[test]
    fn the_mail_of_a_task_is_withheld() {
        // A task made of a mail (task E8.7a): the core fills in the mail's subject and sender.
        let todo: torro_pim::Todo = serde_json::from_value(json!({
            "uid": "t1", "collection": "work", "summary": "Reply to Anna", "completed": false,
            "link": { "id": "mail:anna@example.org:abc@example.org", "mail": {
                "message": "m1", "subject": "Secret offer", "sender": "Anna",
                "date": "2026-10-13T09:00:00Z", "new_replies": 1 } }
        }))
        .expect("a task");
        let list: Request = serde_json::from_value(json!({"method": "tasks.list_todos"})).expect("list");
        let remote = for_model(&list, &Response::Todos { todos: vec![todo.clone()] }, Place::Remote);
        let text = remote.value.to_string();
        assert!(remote.withheld && !text.contains("Secret offer") && text.contains("Reply to Anna"), "{text}");
        assert!(
            for_model(&list, &Response::Todos { todos: vec![todo] }, Place::Local).value.to_string().contains("Secret")
        );
        // Muting by trait and the statistics are withheld as a whole for a model elsewhere.
        let options: Request =
            serde_json::from_value(json!({"method": "inbox.list_mute_options", "id": "a"})).expect("options");
        assert!(for_model(&options, &Response::Ok, Place::Remote).withheld);
    }

    /// Task E8.5b: an event made of an entry is withheld as a whole for a model elsewhere, as its
    /// task is (a mail's subject as title, the mail named in the description); quiet is no mail.
    #[test]
    fn the_event_of_an_entry_is_withheld_and_quiet_is_not() {
        let made: Request = serde_json::from_value(json!({
            "method": "inbox.create_event_from_item", "id": "a1", "collection": "work",
            "start": {"datetime": "2026-10-14T13:00:00Z"}, "end": {"datetime": "2026-10-14T13:30:00Z"}
        }))
        .expect("an event of an entry");
        let event: torro_pim::Event = serde_json::from_value(json!({
            "uid": "e1", "collection": "work", "href": null, "etag": null, "summary": "Secret offer",
            "start": {"datetime": "2026-10-14T13:00:00Z"}, "end": {"datetime": "2026-10-14T13:30:00Z"},
            "location": null, "description": "From mail: Anna", "recurring": false
        }))
        .expect("an event");
        let answer = Response::Event { event };
        let remote = for_model(&made, &answer, Place::Remote);
        assert!(remote.withheld && !remote.value.to_string().contains("Secret"), "{}", remote.value);
        assert!(for_model(&made, &answer, Place::Local).value.to_string().contains("Secret offer"));
        let error = Response::Error { code: "not-found".into(), message: "no such calendar".into() };
        assert_eq!(for_model(&made, &error, Place::Remote).value["result"], "error");
        let quiet = Response::Quiet {
            quiet: Some(torro_ipc::Quiet {
                reason: torro_ipc::QuietReason::Busy,
                until: Utc.with_ymd_and_hms(2026, 10, 14, 15, 0, 0).single().expect("valid time"),
                title: Some("Customer call".into()),
            }),
        };
        let asked = for_model(&Request::GetQuiet, &quiet, Place::Remote);
        assert!(!asked.withheld && asked.value.to_string().contains("Customer call"), "{}", asked.value);
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
