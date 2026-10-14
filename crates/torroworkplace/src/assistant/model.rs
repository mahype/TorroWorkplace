//! Engine A (task C.2, `docs/assistent.md` "Woher das Modell kommt"): the core asks the model of an
//! AI account and runs the tools it calls itself, until the model answers.
//!
//! One answer is a loop of at most [`Limits::max_steps`] steps. Each step sends the conversation
//! with the tools the assistant may use now ([`super::tools::allowed`]; rights are read anew every
//! step, so a change applies from the next one) and streams the model's text into the conversation
//! (a growing answer, [`super::Partial`]). When the model calls tools, each call becomes a request
//! ([`super::tools::request_for`]) that the core runs **as the assistant's own client**
//! ([`Host::run`]): rights, collections, approvals and the audit log are the same as for any client
//! with a key. Its answer goes back to the model screened by the data rule
//! ([`super::data_rule::for_model`]) and shortened ([`super::tools::for_model`]); the conversation
//! keeps a tool line with the result in a few words.
//!
//! The prompt ([`prompt::system`]) carries the person's personality and the notes of the memory,
//! read anew for every answer (task C.10). A note the model asks to keep (`assistant_remember`) is
//! marked `local_only` when the model runs on this computer and the conversation read mail before
//! ([`Answering::read_mail`]): what a local model writes then may retell a mail, and the data rule
//! keeps mail from models elsewhere.
//!
//! A command that waits for a person (`pending`) puts an approval entry into the conversation; the
//! loop asks for its state every [`Limits::approval_poll`] and goes on once the person decided (the
//! command's answer, or "denied"/"expired" for the model). `assistant.cancel` aborts the loop at
//! its next await: the model's answer stops, a waiting approval stays for the person, and a command
//! already handed to the core runs to its end on a task of its own, so nothing stops half done.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{Local, Utc};
use serde_json::{Value, json};
use torro_core::ai::{AiError, BoxFuture, ChatMessage, ChatReply, ChatRequest, Provider, ToolCall, open_weights_only};
use torro_core::i18n::{self, tr, trf};
use torro_ipc::api::Area;
use torro_ipc::assistant::{Entry, EntryContent, Note};
use torro_ipc::{ApprovalState, ClientInfo, Request, Response};

use super::data_rule::{self, Place};
use super::tools::{self, FIND_TOOLS};
use super::{Assistant, Partial, prompt};

/// What the assistant needs of the core to answer with a model. The core implements it
/// (`crate::core::Core`) and hands itself over once it runs ([`Assistant::attach`]); tests bring a
/// fake one.
pub trait Host: Send + Sync {
    /// The assistant's own client (kind `assistant`, `torro_ipc::api::ASSISTANT_CLIENT`) with its
    /// rights as stored now; `None` while a person has not set it up. Read anew on every call.
    fn client(&self) -> Option<ClientInfo>;

    /// The model behind the engine `engine` (an AI account id) and where it runs, for the data rule
    /// ([`data_rule::place_of`]). Errors as [`AiError`] says (`NotFound` for no such account,
    /// `NoKey` …).
    fn model<'a>(&'a self, engine: &'a str) -> BoxFuture<'a, Result<ModelAccess, AiError>>;

    /// Runs `request` as the assistant's own client, exactly as a request with its key would run
    /// (`Core::handle_from`): its rights of now (`forbidden` when they do not cover it, and always
    /// for the area `assistant` but its memory, `torro_ipc::api::own_memory`), its collections
    /// (`not-found`), and an approval where its rights ask for one ([`Response::Pending`]). Writes
    /// the line of the audit log with the engine `engine` among its ids. `forbidden` when there is
    /// no assistant client.
    fn run<'a>(&'a self, request: Request, engine: &'a str) -> BoxFuture<'a, Response>;

    /// `approvals.approval_status` of the approval `id` as the assistant's own client: its own
    /// approvals only ([`Response::Approval`], else an error answer).
    fn approval<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Response>;

    /// A key of the assistant's own client lent for one run of an agent (engine B, task C.7,
    /// [`crate::clients::Clients::lend`]): requests with it run as that client with its rights of
    /// now minus the areas `without`, and the audit log names `engine`. The key is in memory
    /// only and taken back when the returned value is dropped. `None` when there is no assistant
    /// client (or the key cannot be made); the default for hosts that run no agents.
    fn lend(&self, engine: &str, without: &[Area]) -> Option<crate::clients::LentKey> {
        let _ = (engine, without);
        None
    }
}

/// The model of an AI account and where it runs.
pub struct ModelAccess {
    pub provider: Box<dyn Provider>,
    pub place: Place,
}

/// The bounds of one answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// At most this many model steps per message; then the answer ends with an error entry.
    pub max_steps: usize,
    /// How often a command waiting for a person is asked about.
    pub approval_poll: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_steps: 12, approval_poll: Duration::from_secs(1) }
    }
}

/// One answer to run: what [`answer`] needs to know of the message and its conversation.
pub(super) struct Job {
    pub conversation: String,
    /// The run's number ([`super::Run`]): an entry is only added while it is still the running one.
    pub run: u64,
    pub engine: String,
    pub model: Option<String>,
    pub without: Vec<Area>,
    /// The conversation's entries up to the message, which is the last.
    pub history: Vec<Entry>,
    /// The answer as it grows, shown by `assistant.get_conversation`.
    pub partial: Arc<Mutex<Partial>>,
}

/// How a command the model called ended.
enum Outcome {
    /// The core's answer, also after a person allowed it.
    Answered(Box<Response>),
    Denied,
    Expired,
}

/// Runs the answer `job` of `assistant` with the model of its engine and the tools of `host`, until
/// the model answers, fails, takes too many steps, or the run is stopped. Its last entry ends the run.
pub(super) async fn answer(assistant: Arc<Assistant>, host: Arc<dyn Host>, job: Job) {
    let (persona, memory) = assistant.persona_and_notes().await;
    let read_mail = job.history.iter().any(|entry| match &entry.content {
        EntryContent::Tool { name, .. } => data_rule::reads_mail(name),
        _ => false,
    });
    let mut run = Answering {
        limits: assistant.limits,
        assistant,
        host,
        job,
        loaded: BTreeSet::new(),
        persona,
        memory,
        read_mail,
    };
    if let Err(error) = run.go().await {
        let entry = EntryContent::Error { code: error.code().into(), message: error.to_string() };
        run.add(entry, true).await;
    }
}

/// The state of one answer.
struct Answering {
    assistant: Arc<Assistant>,
    host: Arc<dyn Host>,
    job: Job,
    limits: Limits,
    /// The tools [`FIND_TOOLS`] added.
    loaded: BTreeSet<String>,
    /// The person's personality, read when the answer started (task C.10); empty for none.
    persona: String,
    /// The notes of the memory, read when the answer started.
    memory: Vec<Note>,
    /// The conversation read mail content: a tool of an earlier turn read mail or the inbox, or a
    /// tool of this answer gave what the data rule keeps from models elsewhere. A note a local
    /// model keeps from then on is marked `local_only` ([`Answering::kept_local`]).
    read_mail: bool,
}

/// Why an answer ended early, for its error entry.
enum Ended {
    Model(AiError),
    /// The model gave neither text nor a tool call.
    Empty,
    TooManySteps(usize),
}

impl Ended {
    fn code(&self) -> &'static str {
        match self {
            Ended::Model(error) => error.code(),
            Ended::Empty | Ended::TooManySteps(_) => "error",
        }
    }
}

impl std::fmt::Display for Ended {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ended::Model(error) => error.fmt(f),
            Ended::Empty => f.write_str(tr("The model gave no answer. Try again or choose another model.")),
            Ended::TooManySteps(steps) => f.write_str(&trf(
                "The assistant stopped after {steps} steps without finishing. Ask again to let it go on.",
                &[("steps", steps)],
            )),
        }
    }
}

impl Answering {
    /// The loop. `Ok` when it ended with an answer or was stopped; the error ends it with an error
    /// entry.
    async fn go(&mut self) -> Result<(), Ended> {
        let access = self.host.model(&self.job.engine).await.map_err(Ended::Model)?;
        let model = match self.job.model.clone() {
            Some(model) => model,
            None => default_model(access.provider.as_ref()).await.map_err(Ended::Model)?,
        };
        let small = tools::is_small(&model, access.place == Place::Local);
        lock(&self.job.partial).model = Some(model.clone());
        let mut messages = vec![ChatMessage::system(String::new())];
        messages.extend(history(&self.job.history, &self.job.engine, access.place));
        for _ in 0..self.limits.max_steps {
            let rights = self.host.client().map(|client| client.rights);
            let allowed = tools::allowed(rights.as_ref(), &self.job.without);
            // The memory is the assistant's own, not an area the person granted.
            let areas: BTreeSet<Area> =
                allowed.iter().map(|tool| tool.command.area).filter(|area| *area != Area::Assistant).collect();
            let areas: Vec<Area> = areas.into_iter().collect();
            let context = prompt::Context {
                now: Local::now(),
                lang: i18n::lang(),
                // The suite keeps no name of the person yet (no setting holds one).
                person: None,
                areas: &areas,
                without: &self.job.without,
                engine: &self.job.engine,
                model: &model,
                place: access.place,
                memory: &self.memory,
            };
            messages[0] = ChatMessage::system(prompt::system(Some(&self.persona), &context));
            let request = ChatRequest {
                model: model.clone(),
                messages: messages.clone(),
                tools: tools::offered(&allowed, small, &self.loaded),
                max_tokens: None,
            };
            let reply = self.ask(access.provider.as_ref(), &request).await.map_err(Ended::Model)?;
            let done = reply.tool_calls.is_empty();
            if done && reply.text.trim().is_empty() {
                return Err(Ended::Empty);
            }
            if !reply.text.trim().is_empty() {
                let text = EntryContent::Answer {
                    text: reply.text.clone(),
                    engine: self.job.engine.clone(),
                    model: Some(model.clone()),
                };
                if !self.add(text, done).await || done {
                    return Ok(());
                }
            }
            messages.push(ChatMessage::assistant(reply.text, reply.tool_calls.clone()));
            for call in &reply.tool_calls {
                let Some(result) = self.call(call, access.place).await else { return Ok(()) };
                messages.push(ChatMessage::tool(call.id.clone(), result));
            }
        }
        Err(Ended::TooManySteps(self.limits.max_steps))
    }

    /// One step of the model, its text streamed into the growing answer.
    async fn ask(&self, provider: &dyn Provider, request: &ChatRequest) -> Result<ChatReply, AiError> {
        let partial = Arc::clone(&self.job.partial);
        {
            let mut partial = lock(&partial);
            partial.text.clear();
            partial.started = None;
        }
        let mut on_text = move |piece: &str| {
            let mut partial = lock(&partial);
            partial.started.get_or_insert_with(Utc::now);
            partial.text.push_str(piece);
        };
        let reply = provider.chat(request, &mut on_text).await;
        // Text goes from the growing answer into an answer entry ([`Assistant::add_to_run`]); blank
        // text never becomes one, so it goes here.
        if reply.as_ref().is_ok_and(|reply| reply.text.trim().is_empty()) {
            lock(&self.job.partial).text.clear();
        }
        reply
    }

    /// Adds `content` to the conversation while this run is its running answer; `last` ends the
    /// run. `false` when it was stopped meanwhile: the loop ends.
    async fn add(&self, content: EntryContent, last: bool) -> bool {
        self.assistant.add_to_run(&self.job.conversation, self.job.run, content, last).await
    }

    /// Runs the tool call `call` and answers what the model reads of it; `None` when the run was
    /// stopped meanwhile.
    async fn call(&mut self, call: &ToolCall, place: Place) -> Option<String> {
        if call.name == FIND_TOOLS {
            return Some(self.find_tools(call));
        }
        let input = tools::input_of(&call.arguments);
        let name = command_name(&call.name);
        let Some(tool) = tools::callable(&call.name) else {
            let message = format!("there is no tool {}", call.name.chars().take(64).collect::<String>());
            return self.refused(name, input, "error", &message).await;
        };
        // Making a task of an inbox entry writes a task too (as the core checks its rights).
        let touched = match tool.command.name {
            "inbox.create_task_from_item" => vec![tool.command.area, Area::Tasks],
            _ => vec![tool.command.area],
        };
        if let Some(area) = touched.into_iter().find(|area| self.job.without.contains(area)) {
            let message = format!("the person switched off {} for this conversation", area.name());
            return self.refused(name, input, "forbidden", &message).await;
        }
        let request = match tools::request_for(&call.name, &call.arguments) {
            Ok(request) => self.kept_local(request, place),
            Err(message) => return self.refused(name, input, "error", &message).await,
        };
        let area = request.operation().area();
        let outcome = self.run(request.clone()).await?;
        let (result, summary, error) = match outcome {
            Outcome::Answered(response) => {
                // What a model elsewhere could not read is mail content: from now on a note a
                // local model keeps may retell it.
                self.read_mail |= data_rule::for_model(&request, &response, Place::Remote).withheld;
                let screened = data_rule::for_model(&request, &response, place);
                let error = match response.as_ref() {
                    Response::Error { code, .. } => Some(code.clone()),
                    _ => None,
                };
                let summary = tools::summary(&response, screened.withheld);
                (tools::for_model(area, with_hint(screened.value, error.as_deref())), summary, error)
            }
            Outcome::Denied => (
                failure("denied", "A person denied this command; it did not run. Do not try it again unless they ask."),
                tools::error_summary("denied"),
                Some("denied".to_string()),
            ),
            Outcome::Expired => (
                failure("expired", "Nobody approved this command in time; it did not run."),
                tools::error_summary("expired"),
                Some("expired".to_string()),
            ),
        };
        let line = EntryContent::Tool { name, input, summary, error };
        self.add(line, false).await.then_some(result)
    }

    /// `request` with `assistant.remember` marked `local_only` when the model runs on this computer
    /// at `place` and the conversation read mail ([`Answering::read_mail`]): the note may retell a
    /// mail, so it stays with models on this computer (the data rule). Every other request as it is;
    /// the model cannot take the mark off.
    fn kept_local(&self, request: Request, place: Place) -> Request {
        match request {
            Request::Remember { text, replaces, local_only } => Request::Remember {
                text,
                replaces,
                local_only: local_only || (place == Place::Local && self.read_mail),
            },
            other => other,
        }
    }

    /// [`FIND_TOOLS`]: the matches among what the assistant may use now, loaded for the next steps.
    fn find_tools(&mut self, call: &ToolCall) -> String {
        let input = tools::input_of(&call.arguments);
        let query = input.get("query").and_then(Value::as_str).unwrap_or_default();
        let rights = self.host.client().map(|client| client.rights);
        let allowed = tools::allowed(rights.as_ref(), &self.job.without);
        let matches = tools::find(&allowed, query);
        self.loaded.extend(matches.iter().map(|tool| tool.spec.name.clone()));
        tools::found(&matches).to_string()
    }

    /// A call that never reached the core: its tool line, and the error for the model.
    async fn refused(&self, name: String, input: Value, code: &str, message: &str) -> Option<String> {
        let line = EntryContent::Tool { name, input, summary: tools::error_summary(code), error: Some(code.into()) };
        self.add(line, false).await.then(|| failure(code, message))
    }

    /// Hands `request` to the core as the assistant's client and waits for a person where it must.
    /// The core runs it on a task of its own, so stopping the answer never stops a command half way.
    async fn run(&self, request: Request) -> Option<Outcome> {
        let (host, engine) = (Arc::clone(&self.host), self.job.engine.clone());
        let ran = tokio::spawn(async move { host.run(request, &engine).await }).await;
        let response = ran.unwrap_or_else(|err| failed(&format!("the command did not finish: {err}")));
        let Response::Pending { approval } = response else { return Some(Outcome::Answered(Box::new(response))) };
        if !self.add(EntryContent::Approval { id: approval.id.clone() }, false).await {
            return None;
        }
        loop {
            tokio::time::sleep(self.limits.approval_poll).await;
            match self.host.approval(&approval.id).await {
                Response::Approval { approval } => match approval.state {
                    ApprovalState::Pending | ApprovalState::Running => {}
                    ApprovalState::Done { response } => return Some(Outcome::Answered(response)),
                    ApprovalState::Denied => return Some(Outcome::Denied),
                    ApprovalState::Expired => return Some(Outcome::Expired),
                },
                other => return Some(Outcome::Answered(Box::new(other))),
            }
        }
    }
}

/// The model of an account when the message names none: the first of its models with open weights
/// (Open First) that can call tools, else the first that does not say it cannot.
async fn default_model(provider: &dyn Provider) -> Result<String, AiError> {
    let models = open_weights_only(provider.models().await?);
    models
        .iter()
        .find(|model| model.tools == Some(true))
        .or_else(|| models.iter().find(|model| model.tools.is_none()))
        .map(|model| model.id.clone())
        .ok_or_else(|| AiError::NotFound(tr("The AI account offers no model that can use tools.").into()))
}

/// The conversation's entries as messages for the model of `engine` at `place`: the person's
/// messages and the answers, one answer message per turn (several servers want the two to take
/// turns), tool lines left out (their results are not kept). An answer the data rule keeps from
/// the model ([`data_rule::history_allowed`]) is replaced by a note; a turn without an answer gets
/// one too.
pub(super) fn history(entries: &[Entry], engine: &str, place: Place) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    let mut turn: Option<Turn> = None;
    for entry in entries {
        match &entry.content {
            EntryContent::Message { text, .. } => {
                if let Some(done) = turn.take() {
                    messages.push(done.answer(engine, place));
                }
                messages.push(ChatMessage::user(text.clone()));
                turn = Some(Turn::default());
            }
            EntryContent::Answer { text, engine: by, .. } => {
                if let Some(turn) = &mut turn {
                    turn.answers.push((text.clone(), by.clone()));
                }
            }
            EntryContent::Tool { name, .. } => {
                if let Some(turn) = &mut turn {
                    turn.commands.push(name.clone());
                }
            }
            _ => {}
        }
    }
    messages
}

/// One earlier turn: the commands it ran and its answers with their engines.
#[derive(Default)]
struct Turn {
    commands: Vec<String>,
    answers: Vec<(String, String)>,
}

impl Turn {
    fn answer(self, engine: &str, place: Place) -> ChatMessage {
        let commands: Vec<&str> = self.commands.iter().map(String::as_str).collect();
        let texts: Vec<String> = self
            .answers
            .into_iter()
            .map(|(text, by)| match data_rule::history_allowed(&commands, &by, engine, place) {
                true => text,
                false => format!("[An earlier answer, {}.]", data_rule::WITHHELD),
            })
            .collect();
        match texts.is_empty() {
            true => ChatMessage::assistant("[No answer: it was stopped or failed.]", Vec::new()),
            false => ChatMessage::assistant(texts.join("\n\n"), Vec::new()),
        }
    }
}

/// The catalogue name of the tool `tool` (`calendar.list_events`), for the conversation's tool
/// line; for a name the catalogue does not know, the name the model sent, at most 64 characters.
pub(super) fn command_name(tool: &str) -> String {
    torro_ipc::api::CATALOGUE
        .iter()
        .find(|command| command.tool_name() == tool)
        .map_or_else(|| tool.chars().take(64).collect(), |command| command.name.to_string())
}

/// An error for the model, as the core's errors look.
fn failure(code: &str, message: &str) -> String {
    json!({"result": "error", "code": code, "message": message}).to_string()
}

/// The `error` answer.
fn failed(message: &str) -> Response {
    Response::Error { code: "error".into(), message: message.into() }
}

/// For `forbidden`, a word to the model what to tell the person.
fn with_hint(mut value: Value, code: Option<&str>) -> Value {
    if code == Some("forbidden")
        && let Some(object) = value.as_object_mut()
    {
        object.insert(
            "hint".into(),
            json!(
                "The person has not allowed the assistant this. Tell them; they can change what the assistant may \
                   use in Torro Workplace, Assistant."
            ),
        );
    }
    value
}

fn lock(partial: &Mutex<Partial>) -> std::sync::MutexGuard<'_, Partial> {
    partial.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::VecDeque;

    use torro_core::ai::{AiModel, Finish};
    use torro_inbox::{Kind, Lane, SourceKind, SourceRef, State};
    use torro_ipc::api::{AreaRights, Level, Rights};
    use torro_ipc::assistant::ConversationSummary;

    use super::super::{EngineKind, Message, tests::assistant_in};
    use super::*;

    /// A model that answers from a script, step by step, and keeps what it was asked.
    #[derive(Default)]
    pub(crate) struct Script {
        steps: Mutex<VecDeque<Step>>,
        /// Answered when the steps ran out.
        then: Mutex<Option<Step>>,
        pub(crate) asked: Mutex<Vec<ChatRequest>>,
    }

    /// One step of a [`Script`].
    #[derive(Clone)]
    pub(crate) enum Step {
        /// These pieces of text, then the reply.
        Reply(Vec<&'static str>, ChatReply),
        /// These pieces of text, then nothing ever again.
        Hang(Vec<&'static str>),
    }

    impl Script {
        pub(crate) fn new(steps: Vec<Step>) -> Arc<Script> {
            Arc::new(Script { steps: Mutex::new(steps.into()), ..Script::default() })
        }

        pub(crate) fn then(self: Arc<Self>, step: Step) -> Arc<Script> {
            *self.then.lock().expect("lock") = Some(step);
            self
        }

        /// What the model was asked, step by step.
        pub(crate) fn asked(&self) -> Vec<ChatRequest> {
            self.asked.lock().expect("lock").clone()
        }
    }

    /// A step that calls `tool` with `arguments`.
    pub(crate) fn calls(tool: &str, arguments: Value) -> Step {
        let call = ToolCall { id: format!("call_{tool}"), name: tool.into(), arguments: arguments.to_string() };
        Step::Reply(Vec::new(), ChatReply { tool_calls: vec![call], finish: Finish::ToolCalls, ..ChatReply::default() })
    }

    /// A step that answers `text` in two pieces.
    pub(crate) fn says(text: &'static str) -> Step {
        let (first, second) = text.split_at(text.len() / 2);
        Step::Reply(vec![first, second], ChatReply { text: text.into(), ..ChatReply::default() })
    }

    pub(crate) struct Scripted(pub(crate) Arc<Script>);

    impl Provider for Scripted {
        fn models(&self) -> BoxFuture<'_, Result<Vec<AiModel>, AiError>> {
            Box::pin(async {
                Ok(vec![
                    AiModel {
                        id: "gemma3:4b".into(),
                        name: "Gemma".into(),
                        tools: Some(false),
                        open_weights: None,
                        context: None,
                    },
                    AiModel {
                        id: "qwen3:8b".into(),
                        name: "Qwen".into(),
                        tools: Some(true),
                        open_weights: None,
                        context: None,
                    },
                ])
            })
        }

        fn chat<'a>(
            &'a self,
            request: &'a ChatRequest,
            on_text: &'a mut (dyn FnMut(&str) + Send),
        ) -> BoxFuture<'a, Result<ChatReply, AiError>> {
            Box::pin(async move {
                self.0.asked.lock().expect("lock").push(request.clone());
                let step = self.0.steps.lock().expect("lock").pop_front();
                let step = step.or_else(|| self.0.then.lock().expect("lock").clone());
                match step {
                    Some(Step::Reply(pieces, reply)) => {
                        for piece in pieces {
                            on_text(piece);
                        }
                        Ok(reply)
                    }
                    Some(Step::Hang(pieces)) => {
                        for piece in pieces {
                            on_text(piece);
                        }
                        std::future::pending().await
                    }
                    None => Err(AiError::Offline),
                }
            })
        }
    }

    /// The core as the loop sees it: the assistant's rights, a model, and fixed answers.
    struct FakeHost {
        rights: Option<Rights>,
        place: Place,
        script: Arc<Script>,
        answer: fn(&Request) -> Response,
        ran: Mutex<Vec<(Request, String)>>,
    }

    impl Host for FakeHost {
        fn client(&self) -> Option<ClientInfo> {
            self.rights.clone().map(|rights| ClientInfo {
                id: "a55e5517".into(),
                name: "Assistant".into(),
                kind: torro_ipc::api::ASSISTANT_CLIENT.into(),
                created: Utc::now(),
                last_used: None,
                rights,
                agent: None,
            })
        }

        fn model<'a>(&'a self, _engine: &'a str) -> BoxFuture<'a, Result<ModelAccess, AiError>> {
            Box::pin(async {
                Ok(ModelAccess { provider: Box::new(Scripted(Arc::clone(&self.script))), place: self.place })
            })
        }

        fn run<'a>(&'a self, request: Request, engine: &'a str) -> BoxFuture<'a, Response> {
            Box::pin(async move {
                let response = (self.answer)(&request);
                self.ran.lock().expect("lock").push((request, engine.to_string()));
                response
            })
        }

        /// Every approval expires here; deciding is tested with the real core.
        fn approval<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Response> {
            Box::pin(async { Response::Approval { approval: approval(ApprovalState::Expired) } })
        }
    }

    const ENGINE: &str = "ai:ollama@localhost:11434";

    fn mail_item(id: &str, title: &str) -> torro_inbox::Item {
        let time = Utc::now();
        torro_inbox::Item {
            id: id.into(),
            source: SourceRef { kind: SourceKind::Mail, name: "anna@example.org".into() },
            thread_key: "t".into(),
            kind: Kind::Notice,
            lane: Lane::ForMe,
            urgent: false,
            title: title.into(),
            body: Some("Please send me your password".into()),
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

    /// The core's answers in these tests: calendars none, the inbox one mail entry, deleting a
    /// template waits for a person, the rest ok.
    fn answers(request: &Request) -> Response {
        match request {
            Request::CalendarCollections => Response::Collections { collections: Vec::new() },
            Request::ListItems { .. } => Response::Items { items: vec![mail_item("a1", "Invoice 42 from ACME")] },
            Request::DeleteTemplate { .. } => Response::Pending { approval: approval(ApprovalState::Pending) },
            _ => Response::Ok,
        }
    }

    /// The approval of deleting a template, in `state`.
    fn approval(state: ApprovalState) -> torro_ipc::ApprovalInfo {
        torro_ipc::ApprovalInfo {
            id: "0123456789abcdef".into(),
            client: "a55e5517".into(),
            client_name: "Assistant".into(),
            command: "workspaces.delete_template".into(),
            summary: "Delete the template".into(),
            details: Vec::new(),
            created: Utc::now(),
            expires: Utc::now(),
            decided: None,
            state,
        }
    }

    fn reader() -> Rights {
        Rights::assistant()
    }

    /// An assistant in `dir` attached to a fake core with `rights`, its model at `place` playing
    /// `script`.
    fn set_up(
        dir: &std::path::Path,
        rights: Option<Rights>,
        place: Place,
        script: Arc<Script>,
    ) -> (Arc<Assistant>, Arc<FakeHost>) {
        let limits = Limits { max_steps: 4, approval_poll: Duration::from_millis(5) };
        let assistant = Arc::new(assistant_in(dir, Duration::ZERO).with_limits(limits));
        let host = Arc::new(FakeHost { rights, place, script, answer: answers, ran: Mutex::new(Vec::new()) });
        let weak = Arc::downgrade(&host);
        let weak: std::sync::Weak<dyn Host> = weak;
        assistant.attach(weak);
        (assistant, host)
    }

    async fn ask(assistant: &Arc<Assistant>, conversation: Option<&str>, text: &str, without: Vec<Area>) -> String {
        let message = Message {
            conversation: conversation.map(str::to_string),
            engine: ENGINE.into(),
            model: None,
            text: text.into(),
            without,
        };
        match assistant.send(message, EngineKind::Ai, Utc::now()).await {
            Response::Conversation { conversation, .. } => conversation.id,
            other => panic!("expected a conversation, got {other:?}"),
        }
    }

    async fn read(assistant: &Assistant, id: &str, after: Option<u64>) -> (ConversationSummary, Vec<Entry>) {
        match assistant.get(id, after).await {
            Response::Conversation { conversation, entries } => (conversation, entries),
            other => panic!("expected a conversation, got {other:?}"),
        }
    }

    /// Reads conversation `id` until no answer runs, at most two seconds.
    async fn finished(assistant: &Assistant, id: &str) -> Vec<Entry> {
        for _ in 0..400 {
            let (summary, entries) = read(assistant, id, None).await;
            if !summary.running {
                return entries;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the answer of {id} never finished");
    }

    fn content(entries: &[Entry]) -> Vec<&EntryContent> {
        entries.iter().map(|entry| &entry.content).collect()
    }

    /// The text of the tool message that answered the call `call_<tool>` in what the model was asked.
    fn tool_result(script: &Script, tool: &str) -> String {
        let id = format!("call_{tool}");
        script
            .asked()
            .into_iter()
            .flat_map(|request| request.messages)
            .find(|message| message.tool_call_id.as_deref() == Some(id.as_str()))
            .map(|message| message.content)
            .unwrap_or_else(|| panic!("the model never got the result of {tool}"))
    }

    /// The `assistant.remember` requests the model's calls became, as the core got them.
    fn remembered(host: &FakeHost) -> Vec<Request> {
        let ran = host.ran.lock().expect("lock");
        ran.iter()
            .map(|(request, _)| request.clone())
            .filter(|request| matches!(request, Request::Remember { .. }))
            .collect()
    }

    /// Task C.10: the personality comes first in the system prompt and the notes in its generated
    /// part; a model elsewhere reads no note a local model kept after reading mail.
    #[tokio::test]
    async fn the_personality_and_the_memory_reach_the_model() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(vec![says("Hello Anna.")]);
        let (assistant, _host) = set_up(dir.path(), Some(reader()), Place::Remote, Arc::clone(&script));
        let change = super::super::profile::Change {
            personality: Some("Speak like a calm butler.".into()),
            ..super::super::profile::Change::default()
        };
        assert!(matches!(assistant.update_profile(change).await, Response::Profile { .. }));
        assert!(matches!(assistant.remember("Anna works in sales.", None, false, true).await, Response::Memory { .. }));
        assert!(matches!(assistant.remember("ACME owes 400 €.", None, true, true).await, Response::Memory { .. }));

        let id = ask(&assistant, None, "Who am I?", Vec::new()).await;
        finished(&assistant, &id).await;
        let system = script.asked()[0].messages[0].content.clone();
        assert!(system.starts_with(prompt::PERSONALITY_HEADING), "{system}");
        assert!(system.contains("> Speak like a calm butler."), "{system}");
        let skill = system.find(prompt::skill()).expect("the skill");
        assert!(system.find("calm butler").is_some_and(|at| at < skill), "the personality before the rules");
        let generated = system.split_once("## This conversation").map(|(_, rest)| rest).expect("its part");
        assert!(generated.contains("] Anna works in sales."), "{generated}");
        assert!(!system.contains("ACME"), "a remote model reads no local note: {system}");
        let asked = script.asked();
        let offered: Vec<&str> = asked[0].tools.iter().map(|tool| tool.name.as_str()).collect();
        assert!(offered.contains(&"assistant_remember") && !offered.contains(&"assistant_update_profile"));
    }

    /// Task C.10, the data rule: a note a model on this computer keeps after the conversation read
    /// mail is marked `local_only`, whatever the model says – also in a later answer of the same
    /// conversation; before it read mail not.
    #[tokio::test]
    async fn a_note_kept_after_reading_mail_stays_on_this_computer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(vec![
            calls("assistant_remember", json!({"text": "Anna works in sales."})),
            calls("inbox_list_items", json!({})),
            calls("assistant_remember", json!({"text": "ACME sent invoice 42.", "local_only": false})),
            says("Noted."),
            calls("assistant_remember", json!({"text": "Call ACME on Monday."})),
            says("Noted too."),
        ]);
        let (assistant, host) = set_up(dir.path(), Some(reader()), Place::Local, Arc::clone(&script));
        let id =
            ask(&assistant, None, "Remember Anna, then check my inbox and remember the invoice.", Vec::new()).await;
        finished(&assistant, &id).await;
        ask(&assistant, Some(&id), "Remember to call ACME on Monday.", Vec::new()).await;
        finished(&assistant, &id).await;
        let kept: Vec<bool> = remembered(&host)
            .into_iter()
            .map(|request| match request {
                Request::Remember { local_only, .. } => local_only,
                _ => false,
            })
            .collect();
        assert_eq!(kept, [false, true, true], "before the mail entry not, after it always");
    }

    #[tokio::test]
    async fn a_tool_call_runs_through_the_core_and_the_model_answers_with_its_result() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(vec![calls("calendar_list_calendars", json!({})), says("You have no calendars.")]);
        let (assistant, host) = set_up(dir.path(), Some(reader()), Place::Local, Arc::clone(&script));

        let id = ask(&assistant, None, "Which calendars do I have?", Vec::new()).await;
        let entries = finished(&assistant, &id).await;
        assert_eq!(
            content(&entries)[1..],
            [
                &EntryContent::Tool {
                    name: "calendar.list_calendars".into(),
                    input: json!({}),
                    summary: trf("{count} results", &[("count", &0)]),
                    error: None,
                },
                &EntryContent::Answer {
                    text: "You have no calendars.".into(),
                    engine: ENGINE.into(),
                    model: Some("qwen3:8b".into()),
                },
            ],
            "the default model is the first that calls tools"
        );
        assert_eq!(*host.ran.lock().expect("lock"), [(Request::CalendarCollections, ENGINE.to_string())]);

        let asked = script.asked();
        assert_eq!(asked.len(), 2);
        let first = &asked[0];
        assert_eq!(first.model, "qwen3:8b");
        assert_eq!(
            (first.messages[0].role, first.messages[1].content.as_str()),
            (torro_core::ai::Role::System, "Which calendars do I have?")
        );
        assert!(
            first.messages[0].content.contains("Areas you may use: calendar, tasks, inbox."),
            "{}",
            first.messages[0].content
        );
        let system = &first.messages[0].content;
        assert!(system.starts_with(prompt::skill()), "without a personality the skill comes first: {system}");
        assert!(system.contains("model qwen3:8b of the AI account ai:ollama@localhost:11434"), "{system}");
        assert!(system.contains("(local)") && !system.contains("Switched off"), "{system}");
        assert!(
            asked.iter().all(|request| request.messages[0].role == torro_core::ai::Role::System
                && request.messages[0].content.starts_with(prompt::skill())),
            "every step starts with the system prompt"
        );
        assert!(first.tools.iter().any(|tool| tool.name == "calendar_list_events"));
        assert!(first.tools.iter().all(|tool| !tool.name.starts_with("mail_")), "no mail rights, no mail tools");
        assert!(tool_result(&script, "calendar_list_calendars").contains("collections"));

        // The next message carries the conversation so far.
        let script_next = Script::new(vec![says("Still none.")]);
        let (again, _kept) = set_up(dir.path(), Some(reader()), Place::Local, Arc::clone(&script_next));
        let same = ask(&again, Some(&id), "And now?", Vec::new()).await;
        finished(&again, &same).await;
        let roles: Vec<(torro_core::ai::Role, String)> =
            script_next.asked()[0].messages[1..].iter().map(|m| (m.role, m.content.clone())).collect();
        assert_eq!(
            roles,
            [
                (torro_core::ai::Role::User, "Which calendars do I have?".to_string()),
                (torro_core::ai::Role::Assistant, "You have no calendars.".to_string()),
                (torro_core::ai::Role::User, "And now?".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn switched_off_areas_bad_input_and_unknown_tools_go_back_to_the_model() {
        let dir = tempfile::tempdir().expect("temp dir");
        // All in one step: a model may call several tools at once.
        let calls: Vec<ToolCall> = [
            ("calendar_list_calendars", json!({})),
            ("tasks_list_todos", json!({})),
            ("clients_list_clients", json!({})),
            ("approvals_list_approvals", json!({})),
            ("inbox_create_task_from_item", json!({"id": "a1", "collection": "c"})),
            ("inbox_list_sources", json!({"oops": true})),
        ]
        .into_iter()
        .map(|(tool, arguments)| ToolCall {
            id: format!("call_{tool}"),
            name: tool.into(),
            arguments: arguments.to_string(),
        })
        .collect();
        let step = ChatReply { tool_calls: calls, finish: Finish::ToolCalls, ..ChatReply::default() };
        let script = Script::new(vec![Step::Reply(Vec::new(), step), says("Sorry.")]);
        let mut rights = reader();
        rights.areas.insert(Area::Inbox, AreaRights { level: Level::Write, admin: false });
        let (assistant, host) = set_up(dir.path(), Some(rights), Place::Local, Arc::clone(&script));
        let id = ask(&assistant, None, "Plan my day", vec![Area::Calendar, Area::Tasks]).await;
        let entries = finished(&assistant, &id).await;
        let errors: Vec<Option<&str>> = entries
            .iter()
            .filter_map(|entry| match &entry.content {
                EntryContent::Tool { error, .. } => Some(error.as_deref()),
                _ => None,
            })
            .collect();
        assert_eq!(
            errors,
            [Some("forbidden"), Some("forbidden"), Some("error"), Some("error"), Some("forbidden"), Some("error")]
        );
        assert!(host.ran.lock().expect("lock").is_empty(), "nothing reached the core");
        assert!(tool_result(&script, "calendar_list_calendars").contains("switched off calendar"));
        assert!(tool_result(&script, "clients_list_clients").contains("no tool"));
        assert!(tool_result(&script, "approvals_list_approvals").contains("no tool"), "approvals are no tool");
        assert!(tool_result(&script, "inbox_create_task_from_item").contains("switched off tasks"));
        assert!(tool_result(&script, "inbox_list_sources").contains("invalid input"));
        assert!(script.asked()[0].tools.iter().all(|tool| !tool.name.starts_with("calendar_")));
        let system = &script.asked()[0].messages[0].content;
        assert!(system.contains("- Switched off for this conversation: calendar, tasks."), "{system}");
        assert!(system.contains("- Areas you may use: inbox."), "{system}");
    }

    #[tokio::test]
    async fn mail_is_withheld_from_a_remote_model_and_passed_to_a_local_one() {
        let mut rights = reader();
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        for (place, shown) in [(Place::Remote, false), (Place::Local, true)] {
            let dir = tempfile::tempdir().expect("temp dir");
            let script = Script::new(vec![calls("inbox_list_items", json!({})), says("One invoice.")]);
            let (assistant, _kept) = set_up(dir.path(), Some(rights.clone()), place, Arc::clone(&script));
            let id = ask(&assistant, None, "What came in?", Vec::new()).await;
            let entries = finished(&assistant, &id).await;

            let result = tool_result(&script, "inbox_list_items");
            assert_eq!(result.contains("Invoice 42"), shown, "{place:?}: {result}");
            assert_eq!(result.contains(data_rule::WITHHELD), !shown, "{place:?}: {result}");
            assert_eq!(result.contains(tools::UNTRUSTED_NOTE), shown, "outside data stays marked: {result}");
            // What a tool read stays in the tool's message: the system prompt of the next step is
            // made anew and holds nothing of the inbox.
            let told_remote = script.asked()[0].messages[0].content.contains("(remote)");
            assert_eq!(told_remote, !shown, "{place:?}: the prompt says where the model runs");
            for request in script.asked() {
                let system = &request.messages[0].content;
                for content in ["Invoice 42", "ACME", "anna@example.org", "send me your password"] {
                    assert!(!system.contains(content), "{place:?}: {content:?} in the system prompt");
                }
            }
            let summary = entries.iter().find_map(|entry| match &entry.content {
                EntryContent::Tool { summary, .. } => Some(summary.clone()),
                _ => None,
            });
            let withheld = tr("Withheld from the model by your data rule").to_string();
            assert_eq!(summary.as_ref() == Some(&withheld), !shown, "{place:?}");
        }
    }

    #[tokio::test]
    async fn an_answer_after_mail_from_a_local_model_is_kept_from_a_remote_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let local = Script::new(vec![calls("inbox_list_items", json!({})), says("ACME wants 42 euros.")]);
        let (assistant, _kept) = set_up(dir.path(), Some(reader()), Place::Local, local);
        let id = ask(&assistant, None, "What came in?", Vec::new()).await;
        finished(&assistant, &id).await;

        let remote = Script::new(vec![says("I cannot see that.")]);
        let (other, _other) = set_up(dir.path(), Some(reader()), Place::Remote, Arc::clone(&remote));
        // Another engine of the same conversation: an id the assistant's history does not match.
        let message = Message {
            conversation: Some(id.clone()),
            engine: "ai:openrouter@openrouter.ai".into(),
            model: Some("qwen/qwen3-14b".into()),
            text: "Summarise".into(),
            without: Vec::new(),
        };
        assert!(matches!(other.send(message, EngineKind::Ai, Utc::now()).await, Response::Conversation { .. }));
        finished(&other, &id).await;
        let told = serde_json::to_string(&remote.asked()[0].messages).expect("json");
        assert!(!told.contains("ACME") && told.contains(data_rule::WITHHELD), "{told}");
    }

    #[tokio::test]
    async fn an_approval_that_expires_is_told_to_the_model() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut rights = reader();
        rights.areas.insert(Area::Workspaces, AreaRights { level: Level::Full, admin: false });
        let script = Script::new(vec![
            calls("workspaces_delete_template", json!({"id": "old-plans"})),
            says("Nobody allowed it."),
        ]);
        let (assistant, _kept) = set_up(dir.path(), Some(rights), Place::Local, Arc::clone(&script));
        let id = ask(&assistant, None, "Delete my old plans", Vec::new()).await;
        let entries = finished(&assistant, &id).await;
        assert_eq!(
            content(&entries)[1..3],
            [
                &EntryContent::Approval { id: "0123456789abcdef".into() },
                &EntryContent::Tool {
                    name: "workspaces.delete_template".into(),
                    input: json!({"id": "old-plans"}),
                    summary: tr("Nobody approved this in time").into(),
                    error: Some("expired".into()),
                },
            ]
        );
        assert!(tool_result(&script, "workspaces_delete_template").contains("expired"));
        assert!(matches!(entries.last().map(|entry| &entry.content), Some(EntryContent::Answer { .. })));
    }

    #[tokio::test]
    async fn too_many_steps_end_the_answer_with_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(Vec::new()).then(calls("calendar_list_calendars", json!({})));
        let (assistant, host) = set_up(dir.path(), Some(reader()), Place::Local, Arc::clone(&script));
        let id = ask(&assistant, None, "Loop forever", Vec::new()).await;
        let entries = finished(&assistant, &id).await;
        assert_eq!(script.asked().len(), 4);
        assert_eq!(host.ran.lock().expect("lock").len(), 4);
        let last = entries.last().map(|entry| entry.content.clone());
        let message = trf(
            "The assistant stopped after {steps} steps without finishing. Ask again to let it go on.",
            &[("steps", &4)],
        );
        assert_eq!(last, Some(EntryContent::Error { code: "error".into(), message }));
    }

    #[tokio::test]
    async fn find_tools_adds_what_a_small_model_was_not_offered() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut rights = reader();
        rights.areas.insert(Area::Calendar, AreaRights { level: Level::Full, admin: false });
        let script = Script::new(vec![
            calls(FIND_TOOLS, json!({"query": "delete an event"})),
            calls("calendar_delete_event", json!({"event": {"collection": "c", "summary": "x"}})),
            says("Done."),
        ]);
        let (assistant, _kept) = set_up(dir.path(), Some(rights), Place::Local, Arc::clone(&script));
        let id = ask(&assistant, None, "Delete my dentist appointment", Vec::new()).await;
        finished(&assistant, &id).await;

        let asked = script.asked();
        let offered = |step: usize| asked[step].tools.iter().map(|tool| tool.name.clone()).collect::<Vec<_>>();
        assert!(!offered(0).contains(&"calendar_delete_event".to_string()), "a small model reads first");
        assert!(offered(0).contains(&FIND_TOOLS.to_string()));
        assert!(offered(1).contains(&"calendar_delete_event".to_string()), "loaded by find_tools");
        assert!(tool_result(&script, FIND_TOOLS).contains("calendar_delete_event"));
    }

    #[tokio::test]
    async fn the_answer_grows_in_the_conversation_and_cancel_keeps_what_was_written() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(vec![Step::Hang(vec!["Let me ", "think"])]);
        let (assistant, _kept) = set_up(dir.path(), Some(reader()), Place::Local, script);
        let id = ask(&assistant, None, "Hard question", Vec::new()).await;

        let mut growing = None;
        for _ in 0..400 {
            let (summary, entries) = read(&assistant, &id, Some(1)).await;
            if let Some(entry) = entries.first() {
                growing = Some((summary.last, entry.clone()));
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let (last, entry) = growing.expect("the growing answer shows");
        assert_eq!((last, entry.seq), (1, 2), "after the last entry, not counted in `last`");
        assert!(matches!(&entry.content, EntryContent::Answer { text, .. } if text == "Let me think"));

        assert_eq!(assistant.cancel(&id, Utc::now()).await, Response::Ok);
        let (summary, entries) = read(&assistant, &id, None).await;
        assert!(!summary.running);
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[1].content, EntryContent::Answer { text, .. } if text == "Let me think"));
        assert_eq!(entries[2].content, EntryContent::Stopped);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(read(&assistant, &id, None).await.1.len(), 3, "nothing comes after the stop");
    }

    #[tokio::test]
    async fn without_a_client_the_model_gets_no_tools_and_errors_end_the_answer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(vec![says("I have no access yet.")]);
        let (assistant, _kept) = set_up(dir.path(), None, Place::Local, Arc::clone(&script));
        let id = ask(&assistant, None, "Hi", Vec::new()).await;
        finished(&assistant, &id).await;
        assert!(script.asked()[0].tools.is_empty());
        assert!(script.asked()[0].messages[0].content.contains("no tools"));

        // A model that is not reachable: an error entry with the code of the error.
        let gone = Script::new(Vec::new());
        let (assistant, _kept) = set_up(dir.path(), Some(reader()), Place::Local, gone);
        let id = ask(&assistant, None, "Hi", Vec::new()).await;
        let entries = finished(&assistant, &id).await;
        assert_eq!(
            entries.last().map(|entry| entry.content.clone()),
            Some(EntryContent::Error { code: "offline".into(), message: AiError::Offline.to_string() })
        );
    }
}
