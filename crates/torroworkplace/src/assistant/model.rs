//! Engine A (task C.2, `docs/assistent.md` "Woher das Modell kommt"): the core asks the model of an
//! AI account and runs the tools it calls itself, until the model answers.
//!
//! First the model, then the place (task S.23): the core picks the model the message names or
//! "Automatic" ([`ModelAccess::automatic`], local models first) and only then says where it runs
//! ([`Host::model`], [`super::data_rule::place_of`]); the prompt, the earlier answers it may read
//! and the mark `local_only` of the memory follow from that place.
//!
//! One answer is a loop of at most [`Limits::max_steps`] steps. Each step sends the conversation
//! with the tools the assistant may use now ([`super::tools::allowed`]; rights are read anew every
//! step, so a change applies from the next one) and streams the model's text into the conversation
//! (a growing answer, [`super::Partial`]). When the model calls tools, each call becomes a request
//! ([`super::tools::request_for`]) that the core runs **as the assistant's own client**
//! ([`Host::run`]): rights, collections, approvals and the audit log are the same as for any client
//! with a key. Its answer goes back to the model screened by the data rule
//! ([`super::data_rule::for_model`]) and shortened ([`super::tools::for_model`]); the conversation
//! keeps a tool line with the result in a few words, and after a command that made an item a card
//! that points at it (issue #225, [`super::cards`]).
//!
//! The prompt ([`prompt::system`]) carries the person's personality and the notes of the memory,
//! read anew for every answer (task C.10). A note the model asks to keep (`assistant_remember`) is
//! marked `local_only` when the model runs on this computer and the conversation read mail before
//! ([`Answering::read_mail`]): what a local model writes then may retell a mail, and the data rule
//! keeps mail from models elsewhere. Once the conversation read anything the tools mark
//! `untrusted` (mail, GitHub, inbox entries of agents, scripts and mail; [`Answering::untrusted`]),
//! every note waits for a person's approval (task C.11), wherever the model runs.
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
use torro_core::ai::{
    AiError, AiModel, BoxFuture, ChatMessage, ChatReply, ChatRequest, Provider, ToolCall, open_weights_only,
};
use torro_core::i18n::{self, tr, trf};
use torro_ipc::api::{Area, Lack};
use torro_ipc::assistant::{Entry, EntryContent, Mode, Note};
use torro_ipc::bots::Bot;
use torro_ipc::{ApprovalState, ClientInfo, Request, Response};

use super::data_rule::{self, DataRule, Place, Reader};
use super::tools::{self, FIND_TOOLS};
use super::{Assistant, Partial, prompt};

/// What the assistant needs of the core to answer with a model. The core implements it
/// (`crate::core::Core`) and hands itself over once it runs ([`Assistant::attach`]); tests bring a
/// fake one.
pub trait Host: Send + Sync {
    /// The assistant's own client (kind `assistant`, `torro_ipc::api::ASSISTANT_CLIENT`) with its
    /// rights as stored now; `None` while a person has not set it up. Read anew on every call.
    fn client(&self) -> Option<ClientInfo>;

    /// The model `model` of the engine `engine` (an AI account id), or for `None` the one
    /// "Automatic" picks among its models ([`ModelAccess::automatic`]), and where that model runs,
    /// for the data rule ([`data_rule::place_of`], task S.23: account **and** model; a model the
    /// account does not describe counts as elsewhere where that matters). Hosts decide with
    /// [`ModelAccess::choose`]. Errors as [`AiError`] says (`NotFound` for no such account or no
    /// model "Automatic" could take, `NoKey` …).
    fn model<'a>(&'a self, engine: &'a str, model: Option<&'a str>) -> BoxFuture<'a, Result<ModelAccess, AiError>>;

    /// What the person lets go to models outside their computers (task C.17, the setting
    /// `mail_to_cloud`), read when an answer starts. The default keeps mail local, so a host that
    /// cannot tell fails closed.
    fn data_rule(&self) -> DataRule {
        DataRule::default()
    }

    /// The assistant's own tools a person switched on now (issue #225, setting `tool_enabled`): a
    /// tool is offered to the model, named in the prompt and left to an agent only when it is here
    /// and in the client's selection ([`super::tools::with_tools_on`]). The default is none, so a
    /// host that cannot tell offers no tool.
    fn tools_on(&self) -> BTreeSet<torro_core::settings::ToolId> {
        BTreeSet::new()
    }

    /// Whether the register of the assistant's own tools (issue #225) keeps the file `id` as made by
    /// `actor`'s client ([`Host::client_of`]: the assistant's own or the bot's) and the file is still
    /// what the core wrote (`crate::tools::Register::get`). Engine B asks before it adds a card
    /// ([`super::cards::of_agent`]): the agent hands over a tool's result as text, so an id read from
    /// it becomes a card only when the register knows it for exactly this client. Reads the file on
    /// a blocking thread, never on the caller's. The default knows no file, so a host without a
    /// register adds no card.
    fn knows_file<'a>(&'a self, actor: &'a Actor, id: &'a str) -> BoxFuture<'a, bool> {
        let _ = (actor, id);
        Box::pin(async { false })
    }

    /// Runs `request` as the assistant's own client, exactly as a request with its key would run
    /// (`Core::handle_from`): its rights of now (`forbidden` when they do not cover it, and always
    /// for the area `assistant` but its memory, `torro_ipc::api::own_memory`), its collections
    /// (`not-found`), and an approval where its rights ask for one ([`Response::Pending`]). Writes
    /// the line of the audit log with the engine `engine` among its ids. `forbidden` when there is
    /// no assistant client. An approval it asks for keeps `engine`, so the log lines of the
    /// person's decision name it too.
    ///
    /// `untrusted`: the conversation read data from outside the suite (what the tools mark
    /// `untrusted`, [`super::tools::untrusted`]). An `assistant.remember` must then wait for a
    /// person's approval ([`Response::Pending`]) whatever the rights say (task C.11): the note
    /// could carry what a mail, an agent's entry or an issue said into every later prompt.
    fn run<'a>(&'a self, request: Request, engine: &'a str, untrusted: bool) -> BoxFuture<'a, Response>;

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

    // Who an answer runs as (task B.1): the methods above for any [`Actor`]. The assistant is
    // answered by them; a bot's client only by a host that runs bots (the core). The defaults fail
    // closed for bots: no client, `forbidden`, no key.

    /// [`Host::client`] for `actor`: the assistant's own client, or the bot's client (kind
    /// `torro_ipc::api::BOT_CLIENT`) with its rights as stored now; `None` when it is not there.
    /// For [`Actor::Delegated`] (task B.2) the bot's client with the rights both it and the
    /// assistant's client have now ([`torro_ipc::api::Rights::common`]), without the areas of
    /// [`Delegation::without`]; `None` when either client is not there.
    fn client_of(&self, actor: &Actor) -> Option<ClientInfo> {
        match actor {
            Actor::Assistant => self.client(),
            Actor::Bot(_) | Actor::Delegated(_) => None,
        }
    }

    /// [`Host::run`] as `actor`'s client: for a bot exactly as a request with the key of its client
    /// would run, so a bot's client gets `forbidden` for every `bots` and `assistant` command
    /// (`torro_ipc::api::loops_back`). `forbidden` when the client is not there.
    ///
    /// For [`Actor::Delegated`] (task B.2) with the rights of [`Host::client_of`] (what both
    /// clients may, without [`Delegation::without`]); an approval it asks for keeps the narrowing
    /// (the assistant's client and the areas, `crate::approvals::Origin`), so a person's "Allow"
    /// runs it with what both clients may then and never more. Its audit line names the bot's
    /// client, the bot, the assistant's client and `engine`.
    fn run_as<'a>(
        &'a self,
        actor: &'a Actor,
        request: Request,
        engine: &'a str,
        untrusted: bool,
    ) -> BoxFuture<'a, Response> {
        match actor {
            Actor::Assistant => self.run(request, engine, untrusted),
            Actor::Bot(_) | Actor::Delegated(_) => Box::pin(async move { forbidden(request.operation().name()) }),
        }
    }

    /// [`Host::approval`] as `actor`'s client: its own approvals only (for [`Actor::Delegated`]
    /// the bot's client's).
    fn approval_as<'a>(&'a self, actor: &'a Actor, id: &'a str) -> BoxFuture<'a, Response> {
        match actor {
            Actor::Assistant => self.approval(id),
            Actor::Bot(_) | Actor::Delegated(_) => Box::pin(async { forbidden("approvals.approval_status") }),
        }
    }

    /// [`Host::lend`] for `actor`'s client: a key for one run of an agent.
    fn lend_as(&self, actor: &Actor, engine: &str, without: &[Area]) -> Option<crate::clients::LentKey> {
        match actor {
            Actor::Assistant => self.lend(engine, without),
            Actor::Bot(_) | Actor::Delegated(_) => None,
        }
    }

    /// [`Host::lend_as`] for one run in the conversation `conversation` (task B.2), what engine B
    /// calls. The core remembers for the key's requests what the run is: for the assistant, its
    /// conversation, so a `bots.ask_bot` with the key is checked against what the person said
    /// there ([`super::Assistant::delegate`]); for [`Actor::Delegated`], the narrowing, so every
    /// request with the key runs with what both clients may. Without that the key could do more
    /// than the run may, so a host that cannot remember it lends none for [`Actor::Delegated`].
    fn lend_in(
        &self,
        actor: &Actor,
        conversation: &str,
        engine: &str,
        without: &[Area],
    ) -> Option<crate::clients::LentKey> {
        let _ = conversation;
        match actor {
            Actor::Delegated(_) => None,
            _ => self.lend_as(actor, engine, without),
        }
    }

    /// [`Host::run_as`] in the permission mode `mode` of the answer (issue #132): the client's rights
    /// narrowed by it ([`Mode::narrow`]), [`Mode::refuses`] answered with `forbidden`
    /// ([`Lack::ReadOnly`]), and an approval it asks for keeps the mode (`crate::approvals::Origin`),
    /// so a person's "Allow" never runs more than the conversation allowed. The default runs
    /// [`Host::run_as`] for [`Mode::Default`] and fails closed otherwise: `forbidden` for every
    /// command beyond reading, as a host that cannot ask before changes must not run them.
    fn run_in<'a>(
        &'a self,
        actor: &'a Actor,
        request: Request,
        engine: &'a str,
        untrusted: bool,
        mode: Mode,
    ) -> BoxFuture<'a, Response> {
        if mode.is_default() || request.operation().access == torro_ipc::api::Access::Read {
            return self.run_as(actor, request, engine, untrusted);
        }
        Box::pin(async move { forbidden(request.operation().name()) })
    }

    /// [`Host::lend_in`] for a run in the permission mode `mode` (issue #132): every request with the
    /// key runs with the client's rights narrowed by it, and its approvals keep it. The default lends
    /// for [`Mode::Default`] only: a host that cannot narrow a key lends none.
    fn lend_in_mode(
        &self,
        actor: &Actor,
        conversation: &str,
        engine: &str,
        without: &[Area],
        mode: Mode,
    ) -> Option<crate::clients::LentKey> {
        match mode {
            Mode::Default => self.lend_in(actor, conversation, engine, without),
            Mode::Ask | Mode::ReadOnly => None,
        }
    }

    /// [`Host::ask_bot`] from `from` (issue #132 adds the permission mode): the bot then runs in that
    /// mode as well ([`Delegation::mode`]). The default asks for [`Mode::Default`] and refuses
    /// (`forbidden`) otherwise.
    fn ask_bot_in<'a>(
        &'a self,
        from: AskedIn<'a>,
        request: Request,
        engine: &'a str,
        untrusted: bool,
    ) -> BoxFuture<'a, Response> {
        if from.mode.is_default() {
            return self.ask_bot(from.main, request, engine, untrusted, from.read_mail, from.without);
        }
        Box::pin(async move { forbidden(request.operation().name()) })
    }

    /// `bots.ask_bot` of the assistant's own client while it answers in its conversation `main`
    /// with `engine` (task B.2): run as [`Host::run`] would, with the conversation known, so the
    /// assistant checks that the person named the bot or confirmed it there
    /// ([`crate::bots::assistant_may_ask`]) and runs the bot for it with what both clients may and
    /// without the areas `without` (the conversation's switched-off ones); `read_mail`: the
    /// conversation read what the data rule keeps from models elsewhere (mail unless the person let
    /// it out, task C.17), so a bot whose model runs elsewhere is not asked. Answers when the bot's
    /// run is done: [`Response::Conversation`] of the bot's conversation with the entries of this
    /// run, or the error answer (`forbidden` with a word for the model when the person did not ask
    /// for the bot). Never waits for an approval of the ask itself: the person's words are the yes.
    /// The audit line names the assistant's client, the bot and `engine`. `forbidden` by default.
    fn ask_bot<'a>(
        &'a self,
        main: &'a str,
        request: Request,
        engine: &'a str,
        untrusted: bool,
        read_mail: bool,
        without: &'a [Area],
    ) -> BoxFuture<'a, Response> {
        let _ = (main, engine, untrusted, read_mail, without);
        Box::pin(async move { forbidden(request.operation().name()) })
    }
}

/// Where a `bots.ask_bot` of engine A comes from ([`Host::ask_bot_in`], issue #132).
#[derive(Debug, Clone, Copy)]
pub struct AskedIn<'a> {
    /// The assistant's conversation, where the person must have asked for the bot.
    pub main: &'a str,
    /// The conversation read mail (the data rule).
    pub read_mail: bool,
    /// The areas switched off there: the bot runs without them.
    pub without: &'a [Area],
    /// The permission mode of the answer: the bot runs in it as well.
    pub mode: Mode,
}

/// Whose client an answer runs as (task B.1), and so whose rights, collections, approvals and audit
/// lines its tools have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    /// The assistant's own client (kind `torro_ipc::api::ASSISTANT_CLIENT`).
    Assistant,
    /// The client of a bot (kind `torro_ipc::api::BOT_CLIENT`), by the client's id as the bot keeps
    /// it ([`torro_ipc::bots::Bot::client`]).
    Bot(String),
    /// The client of a bot that runs for the assistant (task B.2): the bot's client with what both
    /// it and the assistant's client may ([`Delegation`]).
    Delegated(Delegation),
}

/// A bot's run on behalf of the assistant (task B.2, [`super::Assistant::delegate`]): who runs, for
/// whom, and what is switched off. Its rights are never more than both clients have now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    /// The bot's id ([`torro_ipc::bots::Bot::id`]), for the audit log.
    pub bot: String,
    /// The bot's client id ([`torro_ipc::bots::Bot::client`]): the run runs as this client.
    pub client: String,
    /// The assistant's client id: its rights of now bound the run as well.
    pub assistant: String,
    /// Areas taken out of both: those switched off in the assistant's conversation, and for an
    /// agent driving the assistant those its key was lent without (always mail).
    pub without: Vec<Area>,
    /// The permission mode of the assistant's answer that asked (issue #132): the bot runs in it as
    /// well, so asking a bot never gets around it.
    pub mode: Mode,
}

impl Actor {
    /// The actor of a conversation with `bot`, or with the assistant for `None`.
    pub fn of(bot: Option<&Bot>) -> Actor {
        match bot {
            Some(bot) => Actor::Bot(bot.client.clone()),
            None => Actor::Assistant,
        }
    }

    /// The client kind it runs as.
    pub fn kind(&self) -> &'static str {
        match self {
            Actor::Assistant => torro_ipc::api::ASSISTANT_CLIENT,
            Actor::Bot(_) | Actor::Delegated(_) => torro_ipc::api::BOT_CLIENT,
        }
    }
}

/// The `forbidden` answer for `command`, as the core gives it.
fn forbidden(command: &str) -> Response {
    Response::Error { code: "forbidden".into(), message: format!("this client may not use {command}") }
}

/// The model of an AI account and where it runs.
pub struct ModelAccess {
    pub provider: Box<dyn Provider>,
    /// The model that answers ([`AiModel::id`]): the one asked for, or the one "Automatic" picked.
    pub model: String,
    /// Where [`ModelAccess::model`] runs (task S.23).
    pub place: Place,
}

impl ModelAccess {
    /// The model `asked` among the account's `models`, or "Automatic" for `None`
    /// ([`ModelAccess::automatic`]), with where it runs as `place` says (a host passes
    /// [`data_rule::place_of`] for its account). A model the list lacks, or any model when the
    /// list failed, is judged by its id alone ([`AiModel::remote`] unknown), which `place_of`
    /// counts as elsewhere for Ollama. Without a list "Automatic" fails with its error.
    pub fn choose(
        models: Result<Vec<AiModel>, AiError>,
        asked: Option<&str>,
        place: impl Fn(&AiModel) -> Place,
    ) -> Result<(String, Place), AiError> {
        let Some(asked) = asked else {
            let model = ModelAccess::automatic(models?, &place)?;
            return Ok((model.id.clone(), place(&model)));
        };
        let described = models.ok().and_then(|models| models.into_iter().find(|model| model.id == asked));
        let model = described.unwrap_or_else(|| AiModel {
            id: asked.to_string(),
            name: asked.to_string(),
            tools: None,
            open_weights: None,
            context: None,
            reasoning: None,
            remote: None,
            decision: None,
        });
        Ok((model.id.clone(), place(&model)))
    }

    /// "Automatic", the model of an account when the message names none: of its models with open
    /// weights (Open First) one that runs on the person's computer (`place`, task S.23), one
    /// elsewhere only when there is no local one; in each, the first that can call tools, else the
    /// first that does not say it cannot.
    pub fn automatic(models: Vec<AiModel>, place: impl Fn(&AiModel) -> Place) -> Result<AiModel, AiError> {
        let mut models: Vec<AiModel> =
            open_weights_only(models).into_iter().filter(|model| model.tools != Some(false)).collect();
        // Stable: the server's order stays within local and elsewhere, with and without a word on tools.
        models.sort_by_key(|model| (place(model) != Place::Local, model.tools != Some(true)));
        models
            .into_iter()
            .next()
            .ok_or_else(|| AiError::NotFound(tr("The AI account offers no model that can use tools.").into()))
    }
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
    /// How hard the model thinks (issue #131): into every [`ChatRequest`] of the answer, and Claude
    /// Code's `--effort`.
    pub effort: Option<torro_core::ai::Effort>,
    pub without: Vec<Area>,
    /// The permission mode of the answer (issue #132): every command, the key lent to an agent and
    /// the tools offered are narrowed by it.
    pub mode: Mode,
    /// The conversation's entries up to the message, which is the last.
    pub history: Vec<Entry>,
    /// The answer as it grows, shown by `assistant.get_conversation`.
    pub partial: Arc<Mutex<Partial>>,
    /// Whose client the answer runs as (task B.1).
    pub actor: Actor,
    /// The bot that answers, as stored when the message came (task B.1): its personality,
    /// instructions and skills make its prompt. `None` for the assistant.
    pub bot: Option<Bot>,
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
    // A bot has its own personality and no memory of the assistant's (task B.1).
    let (persona, memory) = match &job.bot {
        Some(bot) => (bot.personality.clone(), Vec::new()),
        None => assistant.persona_and_notes().await,
    };
    let read_mail = job.history.iter().any(|entry| match &entry.content {
        EntryContent::Tool { name, .. } => data_rule::reads_mail(name),
        _ => false,
    });
    let rule = host.data_rule();
    let read_kept = job.history.iter().any(|entry| match &entry.content {
        EntryContent::Tool { name, .. } => data_rule::kept_from(name, rule),
        _ => false,
    });
    let untrusted = job.history.iter().any(|entry| match &entry.content {
        EntryContent::Tool { name, .. } => tools::may_have_read_untrusted(name),
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
        rule,
        read_mail,
        read_kept,
        untrusted,
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
    /// What the person lets go to models elsewhere, read when the answer started (task C.17).
    rule: DataRule,
    /// The conversation read mail content: a tool of an earlier turn read mail or the inbox, or a
    /// tool of this answer gave what the data rule keeps from models elsewhere. A note a local
    /// model keeps from then on is marked `local_only` ([`Answering::kept_local`]).
    read_mail: bool,
    /// The conversation read what [`Answering::rule`] keeps from models elsewhere: an earlier turn's
    /// tool ([`data_rule::kept_from`]), or this answer's that the rule screened. Then no bot whose
    /// model runs elsewhere is asked ([`Host::ask_bot`]). Under the default rule the same as
    /// [`Answering::read_mail`].
    read_kept: bool,
    /// The conversation read data from outside the suite: a tool of an earlier turn that may have
    /// ([`tools::may_have_read_untrusted`]; its answer is not kept, so counted when it might), or an
    /// answer of this one the tools marked `untrusted`. From then on every note waits for a person
    /// (task C.11, [`Host::run`]).
    untrusted: bool,
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
        // First the model, then where it runs (task S.23): everything below follows from that place.
        let access = self.host.model(&self.job.engine, self.job.model.as_deref()).await.map_err(Ended::Model)?;
        let model = access.model.clone();
        let small = tools::is_small(&model, access.place == Place::Local);
        lock(&self.job.partial).model = Some(model.clone());
        let reader = Reader { place: access.place, rule: self.rule };
        let mut messages = vec![ChatMessage::system(String::new())];
        messages.extend(history(&self.job.history, &self.job.engine, Some(&model), reader));
        for _ in 0..self.limits.max_steps {
            let rights = self.rights();
            let allowed = tools::allowed_in(self.job.actor.kind(), rights.as_ref(), &self.job.without, self.job.mode);
            // The memory and the bots (task B.2) are the assistant's own, not areas the person granted.
            let areas: BTreeSet<Area> = allowed
                .iter()
                .map(|tool| tool.command.area)
                .filter(|area| !matches!(area, Area::Assistant | Area::Bots))
                .collect();
            let areas: Vec<Area> = areas.into_iter().collect();
            let context = prompt::Context {
                now: Local::now(),
                lang: i18n::lang(),
                // The suite keeps no name of the person yet (no setting holds one).
                person: None,
                areas: &areas,
                without: &self.job.without,
                rights: rights.as_ref(),
                withheld: &[],
                engine: &self.job.engine,
                model: &model,
                place: access.place,
                mail_to_cloud: self.rule.mail_to_cloud,
                memory: &self.memory,
            };
            let system = match &self.job.bot {
                Some(bot) => prompt::bot_system(Some(&self.persona), &prompt::BotParts::of(bot), &context),
                None => prompt::system(Some(&self.persona), &context),
            };
            messages[0] = ChatMessage::system(system);
            let request = ChatRequest {
                model: model.clone(),
                messages: messages.clone(),
                tools: tools::offered(&allowed, small, &self.loaded),
                max_tokens: None,
                effort: self.job.effort,
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
            // With its thinking blocks unchanged (task KI.1e): Anthropic refuses an assistant
            // message of a tool round whose blocks were dropped or rebuilt, and the other protocol
            // sends none.
            messages.push(ChatMessage::assistant(reply.text, reply.tool_calls.clone()).with_thinking(reply.thinking));
            for call in &reply.tool_calls {
                let Some(result) = self.call(call, reader).await else { return Ok(()) };
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
    async fn call(&mut self, call: &ToolCall, reader: Reader) -> Option<String> {
        if call.name == FIND_TOOLS {
            return Some(self.find_tools(call));
        }
        let input = tools::input_of(&call.arguments);
        let name = command_name(&call.name);
        let Some(tool) = tools::callable(&call.name) else {
            let message = format!("there is no tool {}", call.name.chars().take(64).collect::<String>());
            return self.refused(name, input, "error", &message, None).await;
        };
        // Making a task of an inbox entry writes a task too (as the core checks its rights); making
        // an event of one writes to a calendar (task E8.5b); making a task of a mail in a folder
        // reads the mail (task E8.7a); answering an invitation writes to a calendar (task E8.3b).
        let touched = match tool.command.name {
            "inbox.create_task_from_item" => vec![tool.command.area, Area::Tasks],
            "inbox.create_event_from_item" => vec![tool.command.area, Area::Calendar],
            "tasks.create_todo_from_mail" => vec![tool.command.area, Area::Mail],
            "mail.respond_to_invitation" => vec![Area::Mail, Area::Calendar],
            // A topic decides what reaches the inbox (task E5.12, approval 16).
            "accounts.set_ntfy_topic" => vec![Area::Accounts, Area::Inbox],
            _ => vec![tool.command.area],
        };
        if let Some(area) = touched.into_iter().find(|area| self.job.without.contains(area)) {
            let lack = Lack::SwitchedOff { area };
            return self.refused(name, input, "forbidden", &lack.message(), Some(lack)).await;
        }
        let request = match tools::request_for(&call.name, &call.arguments) {
            Ok(request) => self.kept_local(request, reader.place),
            Err(message) => return self.refused(name, input, "error", &message, None).await,
        };
        let area = request.operation().area();
        let outcome = self.run(request.clone()).await?;
        // A card after a command that made an item (issue #225): the core's own answer, read before
        // the tool line takes it.
        let card = match &outcome {
            Outcome::Answered(response) => super::cards::after(&name, response),
            Outcome::Denied | Outcome::Expired => None,
        };
        let (result, summary, error) = match outcome {
            // A bot that ran for the assistant (task B.2): its answer, as the data rule allows.
            Outcome::Answered(response) if matches!(request, Request::AskBot { .. }) => {
                match super::delegate::Asked::of(response.as_ref()) {
                    Some(asked) => {
                        let input = super::delegate::with_conversation(input, &asked.conversation);
                        let bot = self.bot_name_of(&request).await;
                        self.read_mail |= asked.read_mail;
                        self.read_kept |= asked.kept(self.rule);
                        self.untrusted |= asked.untrusted;
                        let summary = asked.summary(&bot);
                        let error = asked.error.clone();
                        let line = EntryContent::Tool { name, input, summary, error, lack: None };
                        return self.add(line, false).await.then(|| asked.for_model(reader).to_string());
                    }
                    // Refused: the core's message tells the model what to do (propose the bot).
                    None => {
                        let error = match response.as_ref() {
                            Response::Error { code, .. } => Some(code.clone()),
                            _ => None,
                        };
                        let value = serde_json::to_value(response.as_ref()).unwrap_or(Value::Null);
                        (tools::for_model(area, value), tools::summary(&response, false), error)
                    }
                }
            }
            // Its own access (issue #134): the core knows the client's rights and how the request
            // came; the areas switched off here are this loop's, which checks them before the core.
            Outcome::Answered(response) if matches!(*response, Response::MyAccess { .. }) => {
                let response = match *response {
                    Response::MyAccess { access } => {
                        Response::MyAccess { access: access.switching_off(&self.job.without) }
                    }
                    other => other,
                };
                let value = serde_json::to_value(&response).unwrap_or(Value::Null);
                (tools::for_model(area, value), tools::summary(&response, false), None)
            }
            Outcome::Answered(response) => {
                // What a model elsewhere could not read is mail content: from now on a note a
                // local model keeps may retell it.
                self.read_mail |= data_rule::for_model(&request, &response, Place::Remote).withheld;
                let elsewhere = Reader { place: Place::Remote, rule: self.rule };
                self.read_kept |= data_rule::for_model(&request, &response, elsewhere).withheld;
                let screened = data_rule::for_model(&request, &response, reader);
                // Data from outside the suite (task C.11), judged on the whole answer: also what the
                // data rule kept from this model counts, so no screening can hide it.
                let whole = serde_json::to_value(response.as_ref()).unwrap_or(Value::Null);
                self.untrusted |= tools::untrusted(area, &whole);
                let error = match response.as_ref() {
                    Response::Error { code, .. } => Some(code.clone()),
                    _ => None,
                };
                let summary = tools::summary(&response, screened.withheld);
                let hinted = with_hint(screened.value, error.as_deref(), self.job.bot.is_some());
                (tools::for_model(area, hinted), summary, error)
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
        // Issue #133: a refusal says what is missing, in the line as in the answer the model reads.
        let lack = match error.as_deref() {
            Some("forbidden") => self.lack(request.operation()),
            _ => None,
        };
        let summary = lack.map_or(summary, |lack| tools::lack_summary(lack, &self.job.engine));
        let line = EntryContent::Tool { name, input, summary, error, lack };
        if !self.add(line, false).await {
            return None;
        }
        // The card follows its tool line, so the chat shows what was done, then what came of it.
        if let Some(card) = card
            && !self.add(card, false).await
        {
            return None;
        }
        Some(result)
    }

    /// The name of the bot `request` names, as stored now; its id when it is gone.
    async fn bot_name_of(&self, request: &Request) -> String {
        let id = request.bot().unwrap_or_default();
        self.assistant.bots().get(id).await.map_or_else(|| id.to_string(), |bot| bot.name)
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
        let rights = self.rights();
        let allowed = tools::allowed_in(self.job.actor.kind(), rights.as_ref(), &self.job.without, self.job.mode);
        let matches = tools::find(&allowed, query);
        self.loaded.extend(matches.iter().map(|tool| tool.spec.name.clone()));
        tools::found(&matches).to_string()
    }

    /// A call that never reached the core: its tool line, and the error for the model; `lack` says
    /// why it was `forbidden` (issue #133).
    async fn refused(
        &self,
        name: String,
        input: Value,
        code: &str,
        message: &str,
        lack: Option<Lack>,
    ) -> Option<String> {
        let summary =
            lack.map_or_else(|| tools::error_summary(code), |lack| tools::lack_summary(lack, &self.job.engine));
        let line = EntryContent::Tool { name, input, summary, error: Some(code.into()), lack };
        self.add(line, false).await.then(|| failure(code, message))
    }

    /// Why the core refused `operation` with `forbidden` (issue #133), from the client's rights of
    /// now and the areas switched off here; `None` for the core's fixed refusals. A command of one of
    /// the assistant's own tools the rights allow (issue #225): the tool switched off
    /// ([`Lack::ToolOff`]), else not in the selection ([`Lack::Tool`]).
    fn lack(&self, operation: torro_ipc::api::Operation) -> Option<Lack> {
        let rights = self.host.client_of(&self.job.actor).map(|client| self.job.mode.narrow(&client.rights));
        let kind = self.job.actor.kind();
        let lack = tools::lack_in(kind, rights.as_ref(), operation, &self.job.without, &[], self.job.mode);
        let tool = torro_core::settings::ToolId::of_command(operation.command.name);
        match (lack, tool, rights) {
            (None, Some(tool), Some(_)) if !self.host.tools_on().contains(&tool) => Some(Lack::ToolOff { tool }),
            (None, Some(tool), Some(rights)) if !rights.reaches_tool(tool) => Some(Lack::Tool { tool }),
            (lack, ..) => lack,
        }
    }

    /// The rights of the answer's client now, narrowed by the answer's permission mode (issue #132)
    /// and with only the tools a person switched on (issue #225, [`tools::with_tools_on`]): what the
    /// tools offered and the prompt go by.
    fn rights(&self) -> Option<torro_ipc::api::Rights> {
        let on = self.host.tools_on();
        self.host
            .client_of(&self.job.actor)
            .map(|client| tools::with_tools_on(&self.job.mode.narrow(&client.rights), &on))
    }

    /// Hands `request` to the core as the assistant's client and waits for a person where it must.
    /// The core runs it on a task of its own, so stopping the answer never stops a command half way.
    async fn run(&self, request: Request) -> Option<Outcome> {
        let (host, engine, untrusted) = (Arc::clone(&self.host), self.job.engine.clone(), self.untrusted);
        let (actor, mode) = (self.job.actor.clone(), self.job.mode);
        // What the rule keeps home, not all mail: with mail let out (task C.17) a bot elsewhere may
        // be asked after mail, never after entries of Nextcloud or Talk.
        let (main, without, read_mail) = (self.job.conversation.clone(), self.job.without.clone(), self.read_kept);
        // Asking a bot (task B.2) needs the conversation: the person must have asked for the bot here.
        let ran = tokio::spawn(async move {
            match (&actor, &request) {
                (Actor::Assistant, Request::AskBot { .. }) => {
                    let from = AskedIn { main: &main, read_mail, without: &without, mode };
                    host.ask_bot_in(from, request, &engine, untrusted).await
                }
                _ => host.run_in(&actor, request, &engine, untrusted, mode).await,
            }
        })
        .await;
        let response = ran.unwrap_or_else(|err| failed(&format!("the command did not finish: {err}")));
        let Response::Pending { approval } = response else { return Some(Outcome::Answered(Box::new(response))) };
        if !self.add(EntryContent::Approval { id: approval.id.clone() }, false).await {
            return None;
        }
        loop {
            tokio::time::sleep(self.limits.approval_poll).await;
            match self.host.approval_as(&self.job.actor, &approval.id).await {
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

/// The conversation's entries as messages for the model `model` of `engine` read as `reader` (an
/// agent passes `None`, [`data_rule::history_allowed`]): the person's messages and the answers, one
/// answer message per turn (several servers want the two to take turns), tool lines left out
/// (their results are not kept). An answer the data rule keeps from the model (by the engine and
/// model that wrote it) is replaced by a note; a turn without an answer gets one too.
pub(super) fn history(
    entries: &[Entry],
    engine: &str,
    model: Option<&str>,
    reader: impl Into<Reader>,
) -> Vec<ChatMessage> {
    let reader = reader.into();
    let asked = (engine, model);
    let mut messages = Vec::new();
    let mut turn: Option<Turn> = None;
    for entry in entries {
        match &entry.content {
            EntryContent::Message { text, .. } => {
                if let Some(done) = turn.take() {
                    messages.push(done.answer(asked, reader));
                }
                messages.push(ChatMessage::user(text.clone()));
                turn = Some(Turn::default());
            }
            EntryContent::Answer { text, engine: by, model } => {
                if let Some(turn) = &mut turn {
                    turn.answers.push((text.clone(), by.clone(), model.clone()));
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

/// One earlier turn: the commands it ran and its answers with their engines and models.
#[derive(Default)]
struct Turn {
    commands: Vec<String>,
    answers: Vec<(String, String, Option<String>)>,
}

impl Turn {
    /// The turn's answer for the model `asked` (engine and model) read as `reader`.
    fn answer(self, asked: (&str, Option<&str>), reader: Reader) -> ChatMessage {
        let commands: Vec<&str> = self.commands.iter().map(String::as_str).collect();
        let texts: Vec<String> = self
            .answers
            .into_iter()
            .map(|(text, engine, model)| {
                match data_rule::history_allowed(&commands, (&engine, model.as_deref()), asked, reader) {
                    true => text,
                    false => format!("[An earlier answer, {}.]", data_rule::WITHHELD),
                }
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

/// For `forbidden`, a word to the model what to tell the person; for a `bot`, where its access is
/// set (task B.1).
fn with_hint(mut value: Value, code: Option<&str>, bot: bool) -> Value {
    if code == Some("forbidden")
        && let Some(object) = value.as_object_mut()
    {
        let hint = match bot {
            true => {
                "The person has not allowed this bot this. Tell them; they can change what the bot may use in Torro \
                 Workplace, Assistant, Bots."
            }
            false => {
                "The person has not allowed the assistant this. Tell them; they can change what the assistant may \
                 use in Torro Workplace, Assistant."
            }
        };
        object.insert("hint".into(), json!(hint));
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
        pub(crate) steps: Mutex<VecDeque<Step>>,
        /// Answered when the steps ran out.
        then: Mutex<Option<Step>>,
        pub(crate) asked: Mutex<Vec<ChatRequest>>,
        /// The models it lists; `None`: a model without tools, then one with ([`Scripted`]).
        models: Mutex<Option<Vec<AiModel>>>,
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

        /// Lists `models` in place of the usual two.
        pub(crate) fn listing(self: Arc<Self>, models: Vec<AiModel>) -> Arc<Script> {
            *self.models.lock().expect("lock") = Some(models);
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

    /// A model as a server lists it: whether it calls tools, and whether it runs elsewhere.
    pub(crate) fn model(id: &str, tools: Option<bool>, remote: Option<bool>) -> AiModel {
        AiModel {
            id: id.into(),
            name: id.into(),
            tools,
            open_weights: None,
            context: None,
            reasoning: None,
            remote,
            decision: None,
        }
    }

    /// Where a scripted model runs: one said to run elsewhere does (an Ollama cloud model, task
    /// S.23), every other at `place`, where the test puts the account.
    pub(crate) fn scripted_place(place: Place) -> impl Fn(&AiModel) -> Place {
        move |model| if model.remote == Some(true) { Place::Remote } else { place }
    }

    /// [`Host::model`] of a fake core whose account plays `script` at `place`: the model asked, or
    /// "Automatic", as the core chooses it ([`ModelAccess::choose`]).
    pub(crate) async fn scripted_access(
        script: &Arc<Script>,
        model: Option<&str>,
        place: Place,
    ) -> Result<ModelAccess, AiError> {
        let provider = Scripted(Arc::clone(script));
        let (model, place) = ModelAccess::choose(provider.models().await, model, scripted_place(place))?;
        Ok(ModelAccess { provider: Box::new(provider), model, place })
    }

    pub(crate) struct Scripted(pub(crate) Arc<Script>);

    impl Provider for Scripted {
        fn models(&self) -> BoxFuture<'_, Result<Vec<AiModel>, AiError>> {
            Box::pin(async {
                let listed = self.0.models.lock().expect("lock").clone();
                Ok(listed.unwrap_or_else(|| {
                    vec![model("gemma3:4b", Some(false), None), model("qwen3:8b", Some(true), None)]
                }))
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
        /// Whether each request of `ran` came after the conversation read untrusted data.
        untrusted: Mutex<Vec<bool>>,
        /// Whom each request of `ran` ran as (task B.1).
        actors: Mutex<Vec<Actor>>,
        /// What the person lets go to models elsewhere (task C.17).
        rule: Mutex<DataRule>,
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

        fn model<'a>(
            &'a self,
            _engine: &'a str,
            model: Option<&'a str>,
        ) -> BoxFuture<'a, Result<ModelAccess, AiError>> {
            Box::pin(scripted_access(&self.script, model, self.place))
        }

        fn run<'a>(&'a self, request: Request, engine: &'a str, untrusted: bool) -> BoxFuture<'a, Response> {
            Box::pin(async move {
                let response = (self.answer)(&request);
                self.untrusted.lock().expect("lock").push(untrusted);
                self.ran.lock().expect("lock").push((request, engine.to_string()));
                response
            })
        }

        fn data_rule(&self) -> DataRule {
            *self.rule.lock().expect("lock")
        }

        /// Every approval expires here; deciding is tested with the real core.
        fn approval<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Response> {
            Box::pin(async { Response::Approval { approval: approval(ApprovalState::Expired) } })
        }

        /// A bot's client has the same rights here, with its kind; the runs are kept the same way.
        fn client_of(&self, actor: &Actor) -> Option<ClientInfo> {
            let mut client = self.client()?;
            if let Actor::Bot(id) = actor {
                (client.id, client.kind) = (id.clone(), torro_ipc::api::BOT_CLIENT.into());
            }
            Some(client)
        }

        fn run_as<'a>(
            &'a self,
            actor: &'a Actor,
            request: Request,
            engine: &'a str,
            untrusted: bool,
        ) -> BoxFuture<'a, Response> {
            Box::pin(async move {
                self.actors.lock().expect("lock").push(actor.clone());
                self.run(request, engine, untrusted).await
            })
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
            facets: Vec::new(),
            task: None,
            invite: None,
            reason: None,
            category: None,
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
        set_up_with(dir, rights, place, script, answers)
    }

    /// [`set_up`] with a fake core that answers `answer`.
    fn set_up_with(
        dir: &std::path::Path,
        rights: Option<Rights>,
        place: Place,
        script: Arc<Script>,
        answer: fn(&Request) -> Response,
    ) -> (Arc<Assistant>, Arc<FakeHost>) {
        let limits = Limits { max_steps: 4, approval_poll: Duration::from_millis(5) };
        let assistant = Arc::new(assistant_in(dir, Duration::ZERO).with_limits(limits));
        let host = Arc::new(FakeHost {
            rights,
            place,
            script,
            answer,
            ran: Mutex::new(Vec::new()),
            untrusted: Mutex::new(Vec::new()),
            actors: Mutex::new(Vec::new()),
            rule: Mutex::new(DataRule::default()),
        });
        let weak = Arc::downgrade(&host);
        let weak: std::sync::Weak<dyn Host> = weak;
        assistant.attach(weak);
        (assistant, host)
    }

    async fn ask(assistant: &Arc<Assistant>, conversation: Option<&str>, text: &str, without: Vec<Area>) -> String {
        ask_model(assistant, conversation, text, without, None).await
    }

    /// [`ask`] with `model` of the account, or "Automatic" for `None`.
    async fn ask_model(
        assistant: &Arc<Assistant>,
        conversation: Option<&str>,
        text: &str,
        without: Vec<Area>,
        model: Option<&str>,
    ) -> String {
        let message = Message {
            conversation: conversation.map(str::to_string),
            engine: ENGINE.into(),
            model: model.map(str::to_string),
            effort: None,
            text: text.into(),
            without,
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            on_behalf: None,
            person: false,
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

    // ----- Task S.23: the place of the model, not of the account -----

    /// A cloud model of Ollama: it runs on `ollama.com` through the local Ollama.
    const CLOUD: &str = "glm-5.3:cloud";

    /// An Ollama account playing `steps` that lists a cloud model first, then a local one, both
    /// with tools (task S.23).
    fn mixed(steps: Vec<Step>) -> Arc<Script> {
        Script::new(steps)
            .listing(vec![model(CLOUD, Some(true), Some(true)), model("qwen3:8b", Some(true), Some(false))])
    }

    fn reading_mail() -> Rights {
        let mut rights = reader();
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        rights
    }

    /// Task S.23: in one Ollama account the cloud model reads "withheld" for a mail entry, the
    /// local one the mail; with mail let out (task C.17) the cloud model reads it as before.
    #[tokio::test]
    async fn a_cloud_model_of_a_local_ollama_reads_no_mail_unless_it_is_let_out() {
        for (asked, mail_to_cloud, shown) in [(CLOUD, false, false), ("qwen3:8b", false, true), (CLOUD, true, true)] {
            let dir = tempfile::tempdir().expect("temp dir");
            let script = mixed(vec![calls("inbox_list_items", json!({})), says("One invoice.")]);
            let (assistant, host) = set_up(dir.path(), Some(reading_mail()), Place::Local, Arc::clone(&script));
            *host.rule.lock().expect("lock") = DataRule { mail_to_cloud };
            let id = ask_model(&assistant, None, "What came in?", Vec::new(), Some(asked)).await;
            finished(&assistant, &id).await;
            let result = tool_result(&script, "inbox_list_items");
            assert_eq!(result.contains("Invoice 42"), shown, "{asked}, mail out {mail_to_cloud}: {result}");
            assert_eq!(result.contains(data_rule::WITHHELD), !shown, "{asked}: {result}");
            assert_eq!(script.asked()[0].model, asked);
            let told_remote = script.asked()[0].messages[0].content.contains("(remote)");
            assert_eq!(told_remote, asked == CLOUD, "{asked}: the prompt says where the model runs");
        }
    }

    /// Task S.23: "Automatic" takes a local model of a mixed list, and the cloud model only where
    /// the account has no local one.
    #[tokio::test]
    async fn automatic_takes_a_local_model_first() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = mixed(vec![calls("inbox_list_items", json!({})), says("One invoice.")]);
        let (assistant, _host) = set_up(dir.path(), Some(reading_mail()), Place::Local, Arc::clone(&script));
        let id = ask(&assistant, None, "What came in?", Vec::new()).await;
        let entries = finished(&assistant, &id).await;
        assert_eq!(script.asked()[0].model, "qwen3:8b");
        assert!(tool_result(&script, "inbox_list_items").contains("Invoice 42"), "a local model reads mail");
        let models: Vec<Option<&str>> = entries
            .iter()
            .filter_map(|entry| match &entry.content {
                EntryContent::Answer { model, .. } => Some(model.as_deref()),
                _ => None,
            })
            .collect();
        assert_eq!(models, [Some("qwen3:8b")], "the answer names the model");

        let place = scripted_place(Place::Local);
        let cloud_only = vec![model("gemma3:4b", Some(false), Some(false)), model(CLOUD, Some(true), Some(true))];
        assert_eq!(ModelAccess::automatic(cloud_only, &place).map(|model| model.id), Ok(CLOUD.to_string()));
        let unsure = vec![model("qwen3:14b", None, Some(false)), model(CLOUD, Some(true), Some(true))];
        assert_eq!(ModelAccess::automatic(unsure, &place).map(|model| model.id), Ok("qwen3:14b".to_string()));
        let none = vec![model("gemma3:4b", Some(false), Some(false))];
        assert!(matches!(ModelAccess::automatic(none, &place), Err(AiError::NotFound(_))));
        // A model the list lacks, or a list that failed, is judged by its id alone.
        let (_, at) = ModelAccess::choose(Err(AiError::Offline), Some(CLOUD), |model| {
            data_rule::place_of(torro_core::ai::AiProvider::Ollama, "http://localhost:11434/v1", model)
        })
        .expect("asked");
        assert_eq!(at, Place::Remote);
        assert!(ModelAccess::choose(Err(AiError::Offline), None, &place).is_err(), "Automatic needs the list");
    }

    /// Task S.23: what a local model of the account retold of a mail does not reach its cloud model
    /// in the same conversation; the local model reads its own answer again.
    #[tokio::test]
    async fn an_answer_after_mail_from_a_local_model_is_kept_from_the_cloud_model_of_the_account() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = mixed(vec![
            calls("inbox_list_items", json!({})),
            says("ACME wants 42 euros."),
            says("I cannot see that."),
            says("ACME, 42 euros."),
        ]);
        let (assistant, _host) = set_up(dir.path(), Some(reading_mail()), Place::Local, Arc::clone(&script));
        let id = ask_model(&assistant, None, "What came in?", Vec::new(), Some("qwen3:8b")).await;
        finished(&assistant, &id).await;
        ask_model(&assistant, Some(&id), "Summarise", Vec::new(), Some(CLOUD)).await;
        finished(&assistant, &id).await;
        ask_model(&assistant, Some(&id), "Again", Vec::new(), Some("qwen3:8b")).await;
        finished(&assistant, &id).await;

        let asked = script.asked();
        let text = |at: usize| asked[at].messages.iter().map(|message| message.content.clone()).collect::<String>();
        let cloud = text(2);
        assert_eq!(asked[2].model, CLOUD);
        assert!(!cloud.contains("42 euros") && cloud.contains(data_rule::WITHHELD), "{cloud}");
        assert!(text(3).contains("ACME wants 42 euros."), "the local model reads its own answer");
    }

    /// Task S.23: a note a local model keeps after reading mail is `local_only`, and the cloud model
    /// of the same account never reads such a note in its prompt.
    #[tokio::test]
    async fn a_note_after_mail_stays_with_the_local_model_of_the_account() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = mixed(vec![
            calls("inbox_list_items", json!({})),
            calls("assistant_remember", json!({"text": "ACME sent invoice 42.", "local_only": false})),
            says("Noted."),
            says("Hello."),
            says("Hello again."),
        ]);
        let (assistant, host) = set_up(dir.path(), Some(reading_mail()), Place::Local, Arc::clone(&script));
        let id = ask_model(&assistant, None, "Check my inbox and remember it.", Vec::new(), Some("qwen3:8b")).await;
        finished(&assistant, &id).await;
        let kept: Vec<bool> = remembered(&host)
            .into_iter()
            .map(|request| matches!(request, Request::Remember { local_only: true, .. }))
            .collect();
        assert_eq!(kept, [true], "kept by a local model after mail");

        assert!(matches!(assistant.remember("ACME owes 400 €.", None, true, true).await, Response::Memory { .. }));
        for (asked, shown) in [(CLOUD, false), ("qwen3:8b", true)] {
            let id = ask_model(&assistant, None, "Hello", Vec::new(), Some(asked)).await;
            finished(&assistant, &id).await;
            let last = script.asked().pop().expect("asked");
            assert_eq!(last.model, asked);
            assert_eq!(last.messages[0].content.contains("ACME owes"), shown, "{asked}");
        }
    }

    /// Issue #131: the effort of the message goes into every request of its answer.
    #[tokio::test]
    async fn the_effort_reaches_every_step_of_the_answer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(vec![calls("calendar_list_calendars", json!({})), says("No calendars.")]);
        let (assistant, _host) = set_up(dir.path(), Some(reader()), Place::Remote, Arc::clone(&script));
        let message = Message {
            conversation: None,
            engine: ENGINE.into(),
            model: None,
            effort: Some(torro_core::ai::Effort::Low),
            text: "Which calendars?".into(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            on_behalf: None,
            person: false,
        };
        let Response::Conversation { conversation, .. } = assistant.send(message, EngineKind::Ai, Utc::now()).await
        else {
            panic!("expected a conversation");
        };
        finished(&assistant, &conversation.id).await;
        let efforts: Vec<_> = script.asked().iter().map(|request| request.effort).collect();
        assert_eq!(efforts, [Some(torro_core::ai::Effort::Low); 2]);
    }

    /// Task KI.1e: what a protocol must get back unchanged travels from the answer into the
    /// assistant message of the next request. Without this the tool round of an Anthropic account
    /// fails, and the wire format alone cannot show it: the carrying happens here, in the loop.
    #[tokio::test]
    async fn the_loop_carries_the_blocks_a_protocol_wants_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let thought = serde_json::json!({"type": "thinking", "thinking": "Look it up.", "signature": "EqoBCksg"});
        let call = ToolCall { id: "toolu_1".into(), name: "calendar_list_calendars".into(), arguments: "{}".into() };
        let thinks = Step::Reply(
            Vec::new(),
            ChatReply {
                tool_calls: vec![call],
                finish: Finish::ToolCalls,
                thinking: vec![thought.clone()],
                ..ChatReply::default()
            },
        );
        let script = Script::new(vec![thinks, says("No calendars.")]);
        let (assistant, _host) = set_up(dir.path(), Some(reader()), Place::Remote, Arc::clone(&script));
        let id = ask_model(&assistant, None, "Which calendars?", Vec::new(), None).await;
        finished(&assistant, &id).await;

        let asked = script.asked();
        assert_eq!(asked.len(), 2, "one round of tools: {asked:?}");
        let second = &asked[1].messages;
        let turn = second
            .iter()
            .find(|message| message.role == torro_core::ai::Role::Assistant)
            .expect("the assistant's turn");
        assert_eq!(turn.thinking, vec![thought], "unchanged, as the answer sent it");
        assert_eq!(turn.tool_calls.len(), 1, "beside its call");
        for other in second.iter().filter(|message| message.role != torro_core::ai::Role::Assistant) {
            assert!(other.thinking.is_empty(), "nothing else carries blocks: {other:?}");
        }
        // The first request has no earlier turn, so nothing to carry.
        assert!(asked[0].messages.iter().all(|message| message.thinking.is_empty()));
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

    /// Task C.11: once the conversation read an answer the tools mark `untrusted` (here an inbox
    /// entry), every later request tells the core so – also in a later answer of the conversation,
    /// and for a model elsewhere – so a note waits for a person. Before, it does not. The C.10 mark
    /// `local_only` stays a matter of mail and a model on this computer.
    #[tokio::test]
    async fn requests_after_untrusted_data_say_so() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(vec![
            calls("assistant_remember", json!({"text": "Anna works in sales."})),
            calls("inbox_list_items", json!({})),
            calls("assistant_remember", json!({"text": "ACME sent invoice 42."})),
            says("Noted."),
            calls("assistant_remember", json!({"text": "Call ACME on Monday."})),
            says("Noted too."),
        ]);
        let (assistant, host) = set_up(dir.path(), Some(reader()), Place::Remote, Arc::clone(&script));
        let id = ask(&assistant, None, "Remember Anna, then check my inbox.", Vec::new()).await;
        finished(&assistant, &id).await;
        ask(&assistant, Some(&id), "Remember to call ACME on Monday.", Vec::new()).await;
        finished(&assistant, &id).await;
        let untrusted = host.untrusted.lock().expect("lock").clone();
        assert_eq!(untrusted, [false, false, true, true], "remember, inbox, remember, remember");
        let local: Vec<bool> = remembered(&host)
            .into_iter()
            .map(|request| matches!(request, Request::Remember { local_only: true, .. }))
            .collect();
        assert_eq!(local, [false, false, false], "a model elsewhere never read the mail");
        assert!(tools::may_have_read_untrusted("tasks.describe_github_repo"), "GitHub");
        assert!(tools::may_have_read_untrusted("tasks.list_todos"), "issues of filter lists");
        assert!(tools::may_have_read_untrusted("mail.get_message"));
        assert!(!tools::may_have_read_untrusted("calendar.list_events"));
        assert!(!tools::may_have_read_untrusted("tasks.list_lists"));
        let issue = json!({"result": "todos", "todos": [{"collection": "github:mahype/00ff", "summary": "Hi"}]});
        let linked = json!({"result": "todos", "todos": [{"collection": "a/b", "link": {"id": "github:o/r#1"}}]});
        let own = json!({"result": "todos", "todos": [{"collection": "nextcloud:anna/tasks", "summary": "Hi"}]});
        assert!(tools::untrusted(Area::Tasks, &issue) && tools::untrusted(Area::Tasks, &linked));
        assert!(!tools::untrusted(Area::Tasks, &own));
    }

    /// Task B.1: a bot answers with its own prompt – its personality first, the rules, then its
    /// instructions and skills – reads no note of the assistant's memory and is offered none of its
    /// tools, and its tools run as its own client.
    #[tokio::test]
    async fn a_bot_answers_with_its_prompt_and_its_client() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(vec![calls("calendar_list_calendars", json!({})), says("No calendars.")]);
        let (assistant, host) = set_up(dir.path(), Some(reader()), Place::Local, Arc::clone(&script));
        let change = super::super::profile::Change {
            personality: Some("Speak like a calm butler.".into()),
            ..super::super::profile::Change::default()
        };
        assert!(matches!(assistant.update_profile(change).await, Response::Profile { .. }));
        assert!(matches!(assistant.remember("Anna works in sales.", None, false, true).await, Response::Memory { .. }));
        let bot = Bot {
            id: "1a2b3c4d".into(),
            name: "Travel planner".into(),
            description: String::new(),
            avatar: "letter".into(),
            engine: ENGINE.into(),
            model: None,
            personality: "Cheerful and quick.".into(),
            instructions: "Suggest trains before flights.".into(),
            skills: vec![torro_ipc::bots::Skill { name: "Packing".into(), text: "Pack light.".into() }],
            client: "9f8e7d6c".into(),
        };
        let message = Message {
            conversation: None,
            engine: ENGINE.into(),
            model: None,
            effort: None,
            text: "Which calendars do I have?".into(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: Some(bot),
            on_behalf: None,
            person: false,
        };
        let Response::Conversation { conversation, .. } = assistant.send(message, EngineKind::Ai, Utc::now()).await
        else {
            panic!("a conversation with the bot")
        };
        assert_eq!(conversation.bot.as_deref(), Some("1a2b3c4d"));
        finished(&assistant, &conversation.id).await;

        let asked = script.asked();
        let system = asked[0].messages[0].content.clone();
        let at = |text: &str| system.find(text).unwrap_or_else(|| panic!("{text:?} is missing: {system}"));
        assert!(system.starts_with(prompt::PERSONALITY_HEADING), "{system}");
        assert!(at("> Cheerful and quick.") < at(prompt::skill()), "its personality before the rules");
        assert!(at(prompt::skill()) < at(prompt::BOT_HEADING), "the rules before its instructions");
        assert!(at("> Suggest trains before flights.") < at("> Pack light."), "the instructions, then the skills");
        assert!(at("> Pack light.") < at("## This conversation"), "the conversation last");
        assert!(!system.contains("calm butler") && !system.contains("Anna"), "nothing of the assistant's: {system}");
        let offered: Vec<&str> = asked[0].tools.iter().map(|tool| tool.name.as_str()).collect();
        assert!(offered.contains(&"calendar_list_calendars"), "{offered:?}");
        // Of `assistant` only its own access (issue #134).
        let own = |tool: &&str| *tool == "assistant_my_access";
        assert!(
            offered
                .iter()
                .filter(|tool| !own(tool))
                .all(|tool| !tool.starts_with("assistant_") && !tool.starts_with("bots_")),
            "{offered:?}"
        );
        assert!(offered.contains(&"assistant_my_access"), "{offered:?}");
        assert_eq!(*host.actors.lock().expect("lock"), [Actor::Bot("9f8e7d6c".into())], "as the bot's client");
    }

    /// Issue #133: a refused tool line says why – the area not granted, a level too low, the area
    /// switched off here – and the model reads the reason too.
    #[tokio::test]
    async fn a_refused_tool_says_what_is_missing() {
        fn refuses(request: &Request) -> Response {
            let message = format!("this client may not use {}", request.operation().name());
            Response::Error { code: "forbidden".into(), message }
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let script = Script::new(vec![
            calls("workspaces_delete_template", json!({"id": "old-plans"})),
            calls("accounts_list_accounts", json!({})),
            calls("calendar_list_calendars", json!({})),
            says("I may not."),
        ]);
        let mut rights = reader();
        rights.areas.insert(Area::Workspaces, AreaRights { level: Level::Read, admin: false });
        let (assistant, _host) = set_up_with(dir.path(), Some(rights), Place::Local, Arc::clone(&script), refuses);

        let id = ask(&assistant, None, "Tidy up", vec![Area::Calendar]).await;
        let entries = finished(&assistant, &id).await;
        let lines: Vec<(Option<Lack>, String)> = entries
            .iter()
            .filter_map(|entry| match &entry.content {
                EntryContent::Tool { lack, summary, .. } => Some((*lack, summary.clone())),
                _ => None,
            })
            .collect();
        let named = |text: &'static str, area: &'static str| trf(text, &[("area", &tr(area))]);
        assert_eq!(
            lines,
            [
                (
                    Some(Lack::Level { area: Area::Workspaces, needs: torro_ipc::api::Access::Delete }),
                    named("Needs full access to {area}", "Workspaces")
                ),
                (Some(Lack::Area { area: Area::Accounts }), named("No access to {area}", "Accounts")),
                (
                    Some(Lack::SwitchedOff { area: Area::Calendar }),
                    named("{area} is off in this conversation", "Calendar")
                ),
            ]
        );
        assert!(
            tool_result(&script, "calendar_list_calendars").contains("switched off calendar for this conversation")
        );
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
                    lack: None,
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
    async fn a_task_of_a_mail_needs_mail_switched_on() {
        // Making a task of a mail in a folder reads the mail (task E8.7a): with mail switched off it
        // is refused, though tasks are on.
        let dir = tempfile::tempdir().expect("temp dir");
        let call = ToolCall {
            id: "call_tasks_create_todo_from_mail".into(),
            name: "tasks_create_todo_from_mail".into(),
            arguments: json!({"account": "mail:anna@example.org", "id": "m1", "collection": "c"}).to_string(),
        };
        let step = ChatReply { tool_calls: vec![call], finish: Finish::ToolCalls, ..ChatReply::default() };
        let script = Script::new(vec![Step::Reply(Vec::new(), step), says("Sorry.")]);
        let mut rights = reader();
        rights.areas.insert(Area::Tasks, AreaRights { level: Level::Write, admin: false });
        let (assistant, host) = set_up(dir.path(), Some(rights), Place::Local, Arc::clone(&script));
        let id = ask(&assistant, None, "Make a task of Anna's mail", vec![Area::Mail]).await;
        finished(&assistant, &id).await;
        assert!(host.ran.lock().expect("lock").is_empty(), "nothing reached the core");
        assert!(tool_result(&script, "tasks_create_todo_from_mail").contains("switched off mail"));
    }

    #[tokio::test]
    async fn answering_an_invitation_needs_the_calendar_switched_on() {
        // Answering an invitation puts the event in a calendar (task E8.3b): with the calendar
        // switched off it is refused, though mail is on, and nothing reaches the core.
        let dir = tempfile::tempdir().expect("temp dir");
        let call = ToolCall {
            id: "call_mail_respond_to_invitation".into(),
            name: "mail_respond_to_invitation".into(),
            arguments: json!({"account": "mail:anna@example.org", "id": "m1", "answer": "accept"}).to_string(),
        };
        let step = ChatReply { tool_calls: vec![call], finish: Finish::ToolCalls, ..ChatReply::default() };
        let script = Script::new(vec![Step::Reply(Vec::new(), step), says("Sorry.")]);
        let mut rights = reader();
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Write, admin: false });
        rights.areas.insert(Area::Calendar, AreaRights { level: Level::Write, admin: false });
        let (assistant, host) = set_up(dir.path(), Some(rights), Place::Local, Arc::clone(&script));
        let id = ask(&assistant, None, "Accept Jonas's invitation", vec![Area::Calendar]).await;
        finished(&assistant, &id).await;
        assert!(host.ran.lock().expect("lock").is_empty(), "nothing reached the core");
        assert!(tool_result(&script, "mail_respond_to_invitation").contains("switched off calendar"));
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
            (
                "inbox_create_event_from_item",
                json!({"id": "a1", "collection": "c", "start": "2026-10-14T10:00:00Z", "end": "2026-10-14T11:00:00Z"}),
            ),
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
            [
                Some("forbidden"),
                Some("forbidden"),
                Some("error"),
                Some("error"),
                Some("forbidden"),
                Some("error"),
                Some("forbidden")
            ]
        );
        assert!(host.ran.lock().expect("lock").is_empty(), "nothing reached the core");
        assert!(tool_result(&script, "calendar_list_calendars").contains("switched off calendar"));
        assert!(tool_result(&script, "clients_list_clients").contains("no tool"));
        assert!(tool_result(&script, "approvals_list_approvals").contains("no tool"), "approvals are no tool");
        assert!(tool_result(&script, "inbox_create_task_from_item").contains("switched off tasks"));
        assert!(tool_result(&script, "inbox_create_event_from_item").contains("switched off calendar"));
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
            effort: None,
            text: "Summarise".into(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            on_behalf: None,
            person: false,
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
                    lack: None,
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
