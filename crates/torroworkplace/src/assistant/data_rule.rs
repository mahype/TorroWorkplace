//! The data rule of the assistant (task C.2, `docs/ki.md` principle 4): which results of its tools
//! a model may read. Mail stays with models on the person's own computers unless the person lets it
//! go to cloud models (setting `mail_to_cloud`, task C.17, [`DataRule`]); a model
//! elsewhere reads "withheld by your data rule" instead.
//!
//! Per-source rules ("GitHub may go to the cloud, Messenger only local") are task KI.1. Until then
//! the default holds with the one switch for mail, and every decision goes through the functions
//! here, so KI.1 replaces them and nothing else:
//!
//! - [`place_of`]: where a model of an AI account runs ([`Place`]), decided by account **and** model
//!   (task S.23: Ollama's cloud models run on `ollama.com`), once the model is chosen;
//! - [`for_model`]: a tool's answer as a model at a place under the person's rule ([`Reader`]) may
//!   read it;
//! - [`history_allowed`]: whether an earlier answer of the conversation may go to the model, by the
//!   engine and model that wrote it;
//! - [`kept_from`]: whether a command reads what the rule keeps from models elsewhere.
//!
//! With mail let out ([`DataRule::mail_to_cloud`]), a model elsewhere reads mail as a local one does:
//! the answers of `mail.*`, the inbox entries of mail accounts, the senders in the inbox rules of
//! mail accounts, a task made of a mail in a folder and the mail in a task's link. What stays home:
//! the entries and rules of Nextcloud and Talk, the answers made of an inbox entry of any source
//! (hiding a sender, muting, the statistics, a task or event made of an entry: the core does not
//! tell which source the entry had, see below), the notes of the memory marked `local_only` (they
//! may retell those too), and an earlier answer after `inbox.*` or a bot. The rule is read when an
//! answer starts: switched off meanwhile, it holds from the next answer on; a note a model elsewhere
//! kept while mail was let out is not marked and stays readable for it.
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
//! The assistant's memory (task C.10): a note a model on this computer kept after the conversation
//! read mail is marked `local_only` (`super::model`), and a model elsewhere never reads such a note –
//! neither in its prompt (`super::prompt`) nor in what `assistant.list_memories` answers
//! ([`for_model`]).
//!
//! The remote sources of the inbox (task E5.2, decision 15 of E5, `docs/ki.md`: "mail and
//! messengers only local, GitHub may go to the cloud"): entries of Nextcloud and Talk are held back
//! like mail, a model elsewhere reads only their id and source ([`held_back`]); entries of GitHub
//! and ntfy go through, until KI.1 brings rules per source.
//!
//! Known limit, for KI.1: a task made from a mail entry keeps the subject as its title, and
//! `tasks.list_todos` shows it to every model; tasks are the person's own data from then on.

use serde_json::{Value, json};
use torro_core::ai::{AiModel, AiProvider, is_this_computer, openai};
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
    /// On a computer of the person: a model of a local server (Ollama, LM Studio) that computes
    /// there, or of another OpenAI-compatible server at an address of this computer.
    Local,
    /// Anywhere else: OpenRouter, a server of a provider, a server in the network that is no
    /// Ollama or LM Studio, a cloud model of Ollama (task S.23).
    Remote,
}

/// Where `model` of an AI account of `provider` at the base address `server` runs (task S.23: local
/// means the model computes on the person's computer, not that its server is there):
///
/// - Ollama: local only for a model it says runs on its own machine ([`AiModel::remote`] `false`)
///   and whose name is no cloud name ([`openai::ollama_cloud_name`]); a model it did not describe
///   (not listed, or the list failed) counts as elsewhere, so mail stays home in doubt;
/// - LM Studio: local wherever it runs (on the person's own machines), unless the model is known to
///   run elsewhere;
/// - OpenRouter and the other cloud providers (task W.3): never (an address of this computer would
///   be a proxy to one);
/// - any other server: only at an address of this computer (`localhost`, `127.0.0.1`, `::1`), and
///   not for a model known to run elsewhere.
///
/// Ask it once the model is chosen, "Automatic" too (`super::ModelAccess::automatic`).
pub fn place_of(provider: AiProvider, server: &str, model: &AiModel) -> Place {
    let elsewhere = model.remote == Some(true);
    match provider {
        AiProvider::Ollama if model.remote == Some(false) && !openai::ollama_cloud_name(&model.id) => Place::Local,
        AiProvider::LmStudio if !elsewhere => Place::Local,
        AiProvider::OpenAiCompatible if !elsewhere && is_this_computer(server) => Place::Local,
        // Every cloud provider (task W.3), and any other server elsewhere.
        _ => Place::Remote,
    }
}

/// What the person lets go to models outside their computers (task C.17): the setting
/// `mail_to_cloud` as it is when a run starts. The default keeps mail local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DataRule {
    /// Mail may go to models elsewhere as well.
    pub mail_to_cloud: bool,
}

/// Who reads a tool's answer: a model at `place` under the person's `rule`. A [`Place`] alone is
/// read under the default rule (mail local).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reader {
    pub place: Place,
    pub rule: DataRule,
}

impl From<Place> for Reader {
    fn from(place: Place) -> Reader {
        Reader { place, rule: DataRule::default() }
    }
}

impl Reader {
    /// Whether this reader reads mail content: a local model, or any once mail is let out.
    fn reads_mail(self) -> bool {
        self.place == Place::Local || self.rule.mail_to_cloud
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

/// `response` of `request` as `reader` may read it ([`Reader`], or a [`Place`] under the default
/// rule): unchanged for a local model; for one elsewhere without what the rule keeps home (see the
/// module's documentation). A whole answer withheld becomes `{"withheld": WITHHELD}`, an inbox
/// entry of a mail account `{"id": …, "source": …, "withheld": WITHHELD}`, so the model can still
/// name it.
pub fn for_model(request: &Request, response: &Response, reader: impl Into<Reader>) -> Screened {
    let reader = reader.into();
    let value = serde_json::to_value(response).unwrap_or_else(|err| json!({"error": err.to_string()}));
    if reader.place == Place::Local {
        return Screened { value, withheld: false };
    }
    let mail = reader.reads_mail();
    let result = value.get("result").and_then(Value::as_str).unwrap_or_default();
    let whole = match request {
        request if request.operation().area() == Area::Mail => !mail && !MAIL_ANSWERS_ALLOWED.contains(&result),
        // A task made of a mail in a folder (task E8.7a) holds that mail only.
        Request::CreateTodoFromMail { .. } => !mail && result != "error",
        // Muting by trait and the statistics name senders, domains and lists of mail (tasks E8.6a,
        // E8.8a); a task made of an entry takes its title (task E8.7a), and so does an event made of
        // one, with the line naming the mail in its description (task E8.5b); an AI's suggestion of a task or
        // event retells the entry (issue #120). They work on entries of
        // every source, Talk and Nextcloud too, so they stay home also with mail let out.
        Request::HideSender { .. }
        | Request::CreateTaskFromItem { .. }
        | Request::CreateEventFromItem { .. }
        | Request::SuggestTaskFromItem { .. }
        | Request::SuggestEventFromItem { .. }
        | Request::ListMuteOptions { .. }
        | Request::MuteItem { .. }
        | Request::GetStats { .. } => result != "error",
        _ => false,
    };
    if whole {
        return Screened { value: json!({ "withheld": WITHHELD }), withheld: true };
    }
    match response {
        Response::Items { items } => {
            let shown: Vec<Value> = items.iter().map(|item| item_for_remote(item, mail)).collect();
            let withheld = items.iter().any(|item| held_back_under(item, mail));
            Screened { value: json!({ "result": result, "items": shown }), withheld }
        }
        Response::Item { item } => Screened {
            value: json!({ "result": result, "item": item_for_remote(item, mail) }),
            withheld: held_back_under(item, mail),
        },
        // The hits per rule (task E8.8a) stay home: counts of what a rule caught say nothing a model
        // needs. A rule by trait (task E8.6a) can name a mail's sender, domain or list in `facet`.
        Response::Rules { rules, .. } => {
            let shown: Vec<Value> = rules.iter().map(|rule| rule_for_remote(rule, mail)).collect();
            let withheld = rules.iter().any(|rule| rule_held_back(rule, mail));
            Screened { value: json!({ "result": result, "rules": shown }), withheld }
        }
        // A task made of a mail carries the mail's subject and sender in `link.mail` (task E8.7a).
        Response::Todos { .. } | Response::Todo { .. } if mail => Screened { value, withheld: false },
        Response::Todos { todos } => {
            let shown: Vec<Value> = todos.iter().map(todo_for_remote).collect();
            let withheld = todos.iter().any(names_a_mail);
            Screened { value: json!({ "result": result, "todos": shown }), withheld }
        }
        Response::Todo { todo } => {
            Screened { value: json!({ "result": result, "todo": todo_for_remote(todo) }), withheld: names_a_mail(todo) }
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
/// `inbox.*` command, and asking a bot (task B.2: the bot may have read mail, and the assistant's
/// tool line does not keep what it read). An answer after one in the same turn may retell a mail.
pub fn reads_mail(name: &str) -> bool {
    name.starts_with("mail.") || name.starts_with("inbox.") || name == "bots.ask_bot"
}

/// Whether the command `name` reads what `rule` keeps from models elsewhere ([`reads_mail`]): with
/// mail let out ([`DataRule::mail_to_cloud`]) only `inbox.*` and asking a bot, which may still have read
/// entries of Nextcloud and Talk.
pub fn kept_from(name: &str, rule: DataRule) -> bool {
    reads_mail(name) && !(rule.mail_to_cloud && name.starts_with("mail."))
}

/// An inbox entry from a mail account is mail content; one of Nextcloud or Talk (task E5.2) is held
/// back the same way, messages and notifications of people the person works with. GitHub and ntfy
/// go through (module docs). The AI's suggestions of a task or event (issue #120, `crate::suggest`)
/// ask the same: such an entry goes only to a local model. The default rule; the assistant asks
/// [`held_back_under`] with the person's switch for mail.
pub fn held_back(item: &Item) -> bool {
    held_back_under(item, false)
}

/// [`held_back`] with `mail` let out or not (task C.17): an entry of a mail account only while it is
/// not, one of Nextcloud or Talk whatever the switch says.
fn held_back_under(item: &Item, mail: bool) -> bool {
    match item.source.kind {
        SourceKind::Mail => !mail,
        SourceKind::Nextcloud | SourceKind::Talk => true,
        _ => false,
    }
}

/// `item` for a model elsewhere: as it is, or for an entry held back ([`held_back`]) only its id
/// and source.
fn item_for_remote(item: &Item, mail: bool) -> Value {
    if held_back_under(item, mail) {
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

/// Whether a rule of the inbox names what stays home: a sender, a domain or a trait's value
/// (`facet`, task E8.6a: a sender, a domain, a list), unless it is a rule of a mail account and
/// `mail` is let out.
fn rule_held_back(rule: &Rule, mail: bool) -> bool {
    let names = rule.sender.is_some() || rule.domain.is_some() || rule.facet.is_some();
    names && !(mail && rule.source.kind == SourceKind::Mail)
}

/// A rule of the inbox for a model elsewhere: the sender and domain it names are a mail's, and so
/// can be the value of its trait, so all three are withheld ([`rule_held_back`]).
fn rule_for_remote(rule: &Rule, mail: bool) -> Value {
    let mut value = serde_json::to_value(rule).unwrap_or(Value::Null);
    if !rule_held_back(rule, mail) {
        return value;
    }
    if let Some(object) = value.as_object_mut() {
        for field in ["sender", "domain", "facet"] {
            if let Some(named) = object.get_mut(field) {
                *named = json!(WITHHELD);
            }
        }
    }
    value
}

/// Whether an earlier answer of the conversation, written by the engine and model `answered_by`
/// (the model `None` when the answer does not name it) in a turn that used the commands `commands`
/// (catalogue names), may go to the model `asked` (engine and model) read as `reader`. An agent
/// (Claude Code) asks with the model `None`: it names its model only once it runs, and every model
/// of it runs elsewhere, so its engine alone tells its own answers.
///
/// A local model reads everything. A model elsewhere reads earlier answers unless that turn read
/// what the rule keeps home ([`kept_from`]: mail unless it is let out, the inbox, a bot) and another
/// engine or model wrote the answer: a local model may have retold it there, also a local model of
/// the same account (task S.23: Ollama runs local and cloud models). Its own answers never held
/// such content (it read [`WITHHELD`] instead), and of another model the core may no longer know
/// where it ran, so those are withheld; an answer without its model counts as another model's.
pub fn history_allowed(
    commands: &[&str],
    answered_by: (&str, Option<&str>),
    asked: (&str, Option<&str>),
    reader: impl Into<Reader>,
) -> bool {
    let reader = reader.into();
    let read_kept = commands.iter().any(|name| kept_from(name, reader.rule));
    let same = answered_by.0 == asked.0 && (asked.1.is_none() || answered_by.1 == asked.1);
    reader.place == Place::Local || !read_kept || same
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
            invite: None,
            reason: None,
            category: None,
        }
    }

    fn mail_source() -> SourceRef {
        SourceRef { kind: SourceKind::Mail, name: "anna@example.org".into() }
    }

    fn agent_source() -> SourceRef {
        SourceRef { kind: SourceKind::Agent, name: "claude-code".into() }
    }

    /// A model as a server lists it, with what it says about where the model runs.
    fn model(id: &str, remote: Option<bool>) -> AiModel {
        AiModel {
            id: id.into(),
            name: id.into(),
            tools: None,
            open_weights: None,
            context: None,
            reasoning: None,
            remote,
        }
    }

    #[test]
    fn local_servers_are_ollama_lm_studio_and_this_computer() {
        let local = model("qwen3:8b", Some(false));
        let unknown = model("qwen3:8b", None);
        assert_eq!(place_of(AiProvider::Ollama, "http://localhost:11434/v1", &local), Place::Local);
        assert_eq!(place_of(AiProvider::Ollama, "http://gpu.local:11434/v1", &local), Place::Local);
        assert_eq!(place_of(AiProvider::LmStudio, "http://localhost:1234/v1", &unknown), Place::Local);
        assert_eq!(place_of(AiProvider::OpenAiCompatible, "http://127.0.0.1:8000/v1", &unknown), Place::Local);
        assert_eq!(place_of(AiProvider::OpenAiCompatible, "http://[::1]:8000/v1", &unknown), Place::Local);
        assert_eq!(place_of(AiProvider::OpenAiCompatible, "https://ai.example.org/v1", &unknown), Place::Remote);
        assert_eq!(place_of(AiProvider::OpenRouter, "https://openrouter.ai/api/v1", &unknown), Place::Remote);
        assert_eq!(place_of(AiProvider::OpenRouter, "http://localhost:8080/api/v1", &local), Place::Remote, "a proxy");
        assert_eq!(
            place_of(AiProvider::Anthropic, "http://localhost:8080/v1", &local),
            Place::Remote,
            "a proxy to Anthropic is still Anthropic"
        );
        for cloud in AiProvider::CLOUD {
            assert_eq!(place_of(cloud, cloud.default_url().unwrap_or_default(), &local), Place::Remote, "{cloud:?}");
        }
    }

    /// Task S.23: Ollama's cloud models run on `ollama.com`, though Ollama runs here.
    #[test]
    fn a_cloud_model_of_ollama_runs_elsewhere() {
        let ollama = "http://localhost:11434/v1";
        assert_eq!(place_of(AiProvider::Ollama, ollama, &model("glm-5.3:cloud", Some(true))), Place::Remote);
        assert_eq!(place_of(AiProvider::Ollama, ollama, &model("kimi-k2.6:cloud", Some(false))), Place::Remote);
        assert_eq!(place_of(AiProvider::Ollama, ollama, &model("my-glm:latest", Some(true))), Place::Remote, "copied");
        assert_eq!(place_of(AiProvider::Ollama, ollama, &model("qwen3:8b", None)), Place::Remote, "not described");
        assert_eq!(place_of(AiProvider::Ollama, ollama, &model("qwen3:8b", Some(false))), Place::Local);
        let lm_studio = "http://localhost:1234/v1";
        assert_eq!(place_of(AiProvider::LmStudio, lm_studio, &model("qwen3-8b", Some(true))), Place::Remote);
    }

    #[test]
    fn mail_goes_to_local_models_only() {
        let search: Request =
            serde_json::from_value(json!({"method": "mail.search_messages", "limit": 10})).expect("a search");
        let page = Response::MailMessages {
            messages: Vec::<MessageSummary>::new(),
            cursor: None,
            source: MailSource::Cache,
            complete: true,
            incomplete_reason: None,
            highlights: Vec::new(),
            total_estimate: None,
        };
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
            alarms: Vec::new(),
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

    /// Task E5.2: Nextcloud and Talk stay with local models like mail, GitHub and ntfy go through.
    #[test]
    fn nextcloud_and_talk_entries_are_withheld_and_github_and_ntfy_are_not() {
        let list: Request = serde_json::from_value(json!({"method": "inbox.list_items"})).expect("a list");
        let cloud = "anna@cloud.example.org";
        let items = Response::Items {
            items: vec![
                item("n1", SourceRef::nextcloud(cloud), "Jonas shared Budget.ods"),
                item("t1", SourceRef::talk(cloud), "Lena: are you coming?"),
                item("g1", SourceRef::github("anna-berg"), "Review requested"),
                item("y1", SourceRef::ntfy("ntfy.sh"), "Backup failed"),
            ],
        };
        let remote = for_model(&list, &items, Place::Remote);
        assert!(remote.withheld);
        let text = remote.value.to_string();
        assert!(!text.contains("Budget") && !text.contains("Lena"), "{text}");
        assert!(text.contains("Review requested") && text.contains("Backup failed"), "{text}");
        for (n, id) in ["n1", "t1"].into_iter().enumerate() {
            assert_eq!(remote.value["items"][n]["withheld"], WITHHELD);
            assert_eq!(remote.value["items"][n]["id"], id);
        }
        assert!(!for_model(&list, &items, Place::Local).withheld);
        let one = Response::Item { item: item("t1", SourceRef::talk(cloud), "Lena: are you coming?") };
        assert!(for_model(&Request::GetItem { id: "t1".into() }, &one, Place::Remote).withheld);
    }

    /// Task E5.12: a topic's name is a password on a public server. The assistant lists accounts
    /// with its own client key, which gets the labels only, so a model elsewhere never reads a name.
    #[tokio::test]
    async fn a_remote_model_reads_ntfy_topics_by_their_labels_only() {
        use crate::account::ntfy::tests::{fake_ntfy, service, topic};
        let dir = tempfile::tempdir().expect("temp dir");
        let server = fake_ntfy().await;
        let ntfy = service(dir.path(), &crate::account::github::tests::MemoryTokens::default());
        let base = format!("http://127.0.0.1:{}", server.port());
        ntfy.connect(&base, &[topic("open_backups", "Backups")]).await.expect("connected");

        let accounts = Response::Accounts { accounts: ntfy.infos(false).await };
        let screened = for_model(&Request::Accounts, &accounts, Place::Remote).value.to_string();
        assert!(!screened.contains("open_backups") && screened.contains("Backups"), "{screened}");
    }

    /// Task E8.3c: the invitation a mail entry carries is the mail's content and goes with the rest
    /// of the entry – a model elsewhere reads neither its title nor when it is.
    #[test]
    fn the_invitation_of_a_mail_entry_is_withheld_with_it() {
        let start = Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).single().expect("valid time");
        let invite = torro_mail::Invite {
            title: "Board meeting".into(),
            start,
            end: None,
            all_day: false,
            answered: Some(torro_mail::InvitationAnswer::Accept),
        };
        let entry = Item { invite: Some(invite), ..item("a1", mail_source(), "Invitation") };
        let one = Response::Item { item: entry.clone() };
        let remote = for_model(&Request::GetItem { id: "a1".into() }, &one, Place::Remote);
        let text = remote.value.to_string();
        assert!(remote.withheld && !text.contains("Board meeting") && !text.contains("2026-10-20"), "{text}");
        let list: Request = serde_json::from_value(json!({"method": "inbox.list_items"})).expect("a list");
        let many = for_model(&list, &Response::Items { items: vec![entry] }, Place::Remote).value.to_string();
        assert!(!many.contains("Board meeting") && !many.contains("invite"), "{many}");
        let local = for_model(&Request::GetItem { id: "a1".into() }, &one, Place::Local);
        assert!(local.value.to_string().contains("Board meeting"), "a local model reads it");
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

    /// Issue #120: an AI's suggestion retells the entry, a mail too: a model elsewhere reads that
    /// it was withheld, or the error.
    #[test]
    fn a_suggestion_of_an_entry_is_withheld_from_remote_models() {
        let asked = Request::SuggestTaskFromItem { id: "a1".into() };
        let suggestion = torro_ipc::TaskSuggestion {
            title: "Reply to the secret offer".into(),
            notes: None,
            due: None,
            engine: "ai:ollama@localhost:11434".into(),
            model: "qwen3:8b".into(),
        };
        let answer = Response::TaskSuggestion { suggestion };
        let remote = for_model(&asked, &answer, Place::Remote);
        assert!(remote.withheld && !remote.value.to_string().contains("secret"), "{}", remote.value);
        assert!(for_model(&asked, &answer, Place::Local).value.to_string().contains("secret offer"));
        let none = Response::Error { code: "not-connected".into(), message: "Set up an AI first.".into() };
        let event = Request::SuggestEventFromItem { id: "a1".into() };
        assert_eq!(for_model(&event, &none, Place::Remote).value["result"], "error");
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

    /// Task C.17: with mail let out, a model elsewhere reads mail, the mail entries of the inbox,
    /// the senders in its rules and the mail of a task; the entries of Nextcloud and Talk and the
    /// notes kept after mail stay home.
    #[test]
    fn mail_let_out_reaches_remote_models_and_messages_stay_home() {
        let out = Reader { place: Place::Remote, rule: DataRule { mail_to_cloud: true } };
        let search: Request =
            serde_json::from_value(json!({"method": "mail.search_messages", "limit": 10})).expect("a search");
        let page = Response::MailMessages {
            messages: Vec::<MessageSummary>::new(),
            cursor: None,
            source: MailSource::Cache,
            complete: true,
            incomplete_reason: None,
            highlights: Vec::new(),
            total_estimate: None,
        };
        assert!(for_model(&search, &page, Place::Remote).withheld, "under the default rule");
        let screened = for_model(&search, &page, out);
        assert!(!screened.withheld);
        assert_eq!(screened.value["result"], "mail-messages");

        let list: Request = serde_json::from_value(json!({"method": "inbox.list_items"})).expect("a list");
        let cloud = "anna@cloud.example.org";
        let items = Response::Items {
            items: vec![
                item("a1", mail_source(), "Invoice 42"),
                item("t1", SourceRef::talk(cloud), "Lena: are you coming?"),
                item("n1", SourceRef::nextcloud(cloud), "Jonas shared Budget.ods"),
            ],
        };
        let screened = for_model(&list, &items, out);
        let text = screened.value.to_string();
        assert!(screened.withheld && text.contains("Invoice 42"), "{text}");
        assert!(!text.contains("Lena") && !text.contains("Budget"), "{text}");

        let created = Utc.with_ymd_and_hms(2026, 10, 14, 9, 0, 0).single().expect("valid time");
        let kept = Note { id: "5e6f7a8b".into(), text: "ACME owes 400 €.".into(), created, local_only: true };
        let notes = Response::Memories { notes: vec![kept] };
        assert!(!for_model(&Request::ListMemories, &notes, out).value.to_string().contains("ACME"));

        let rule = Rule {
            id: "r1".into(),
            source: mail_source(),
            sender: Some("boss@acme.example".into()),
            domain: None,
            effect: torro_inbox::RuleEffect::Muted,
            created,
            facet: None,
            until: None,
        };
        let talk_rule =
            Rule { id: "r2".into(), source: SourceRef::talk(cloud), sender: Some("lena".into()), ..rule.clone() };
        let rules: Request = serde_json::from_value(json!({"method": "inbox.list_rules"})).expect("rules");
        let screened = for_model(&rules, &Response::Rules { rules: vec![rule, talk_rule], hits: Vec::new() }, out);
        let text = screened.value.to_string();
        assert!(screened.withheld && text.contains("acme") && !text.contains("lena"), "{text}");

        // Answers made of an entry of any source stay home: it may be one of Talk.
        for method in [
            json!({"method": "inbox.list_mute_options", "id": "t1"}),
            json!({"method": "inbox.get_stats", "range": "week"}),
        ] {
            let request: Request = serde_json::from_value(method).expect("a request");
            assert!(for_model(&request, &Response::Ok, out).withheld, "{request:?}");
        }

        let ollama = ("ai:ollama@localhost:11434", Some("qwen3:8b"));
        let openrouter = ("ai:openrouter@openrouter.ai", Some("qwen/qwen3-14b"));
        assert!(history_allowed(&["mail.get_message"], ollama, openrouter, out));
        assert!(!history_allowed(&["inbox.list_items"], ollama, openrouter, out), "Talk may be in it");
        assert!(!kept_from("mail.get_message", out.rule) && kept_from("mail.get_message", DataRule::default()));
        assert!(kept_from("inbox.list_items", out.rule) && kept_from("bots.ask_bot", out.rule));
    }

    #[test]
    fn earlier_answers_after_mail_stay_with_the_engine_that_wrote_them() {
        let (ollama, openrouter) = ("ai:ollama@localhost:11434", "ai:openrouter@openrouter.ai");
        let (local, remote) = ((ollama, Some("qwen3:8b")), (openrouter, Some("qwen/qwen3-14b")));
        let read_mail = ["calendar.list_events", "mail.get_message"];
        assert!(!history_allowed(&read_mail, local, remote, Place::Remote));
        assert!(history_allowed(&read_mail, remote, remote, Place::Remote), "it read the withheld text");
        assert!(history_allowed(&read_mail, remote, local, Place::Local));
        assert!(history_allowed(&["calendar.list_events"], local, remote, Place::Remote));
        assert!(!history_allowed(&["inbox.list_items"], local, remote, Place::Remote));
        // An agent names no model before it runs: its engine tells its own answers.
        let agent = ("claude-code", Some("claude-sonnet-4-5"));
        assert!(history_allowed(&read_mail, agent, ("claude-code", None), Place::Remote));
        assert!(!history_allowed(&read_mail, local, ("claude-code", None), Place::Remote));
    }

    /// Task S.23: within one Ollama account the model counts – a cloud model never reads what a
    /// local one retold of a mail, and an answer that does not name its model counts as another's.
    #[test]
    fn earlier_answers_after_mail_stay_with_the_model_that_wrote_them() {
        let ollama = "ai:ollama@localhost:11434";
        let read_mail = ["mail.get_message"];
        let cloud = (ollama, Some("glm-5.3:cloud"));
        assert!(!history_allowed(&read_mail, (ollama, Some("qwen3:8b")), cloud, Place::Remote));
        assert!(!history_allowed(&read_mail, (ollama, None), cloud, Place::Remote), "no model named");
        assert!(history_allowed(&read_mail, (ollama, Some("glm-5.3:cloud")), cloud, Place::Remote));
        assert!(history_allowed(&read_mail, cloud, (ollama, Some("qwen3:8b")), Place::Local));
    }
}
