//! The assistant of the core (tasks C.1 and C.2, `docs/assistent.md`): its conversations, the
//! engines that answer, and the handlers of the `assistant.*` requests ([`answer_from`]).
//!
//! The logic lives here only ("Wo die Logik liegt"): the app `torroassistant`, the control centre,
//! scripts and agents send requests; the core keeps the conversations and runs the answers in the
//! background, so a conversation shows the same in every window and over API and MCP. A window
//! follows a running answer with `assistant.get_conversation` and `after`.
//!
//! - **Storage** ([`store`]): `<data>/assistant/<id>.jsonl`, the folder 0700, every file 0600,
//!   loaded on first use. Conversations whose last entry is older than the retention time
//!   (`Settings::assistant_retention`, 30 days unless changed) are removed by
//!   [`Assistant::clean_up`], pinned ones and those answering never. Files are read and written on
//!   the blocking threads, under the conversations' lock, so their lines keep the order of their
//!   numbers.
//! - **Engines** ([`Assistant::engines`]): every AI account (engine A, [`model`]), the coding
//!   agents installed while the assistant has its client (engine B, [`engines`]), and the test
//!   engine ([`TEST_ENGINE`]) with fixed answers, offered in tests and when the core runs with
//!   `TORRO_TEST_ENGINE=1` ([`TEST_ENGINE_VAR`]), so the app and the control centre can be built
//!   without a model.
//! - **Running answers**: one per conversation; `assistant.send_message` meanwhile is `busy`,
//!   `assistant.cancel` stops it (the task is aborted, what the model wrote so far stays as an
//!   answer, a `stopped` entry follows). While a model writes, its text shows as a growing answer
//!   numbered after the conversation's `last` ([`Run::growing`]), not counted in `last` until it is
//!   stored: a window asks again with the same `after` and gets it longer.
//! - **Rights** are the core's ([`crate::core::Core::handle_from`]): the area `assistant` for the
//!   caller, and never for the assistant's own client (`torro_ipc::api::loops_back`). An answer
//!   works with the assistant's own client: every tool runs through [`Host::run`], with that
//!   client's rights, collections and approvals, and the data rule ([`data_rule`]) decides what of
//!   the results the model reads.
//! - **Audit**: the core logs the conversation id and the engine, never a text
//!   ([`crate::audit::ids`]); the lines of the assistant's tools name the engine too.
//! - **Profile and memory** (task C.10, [`profile`]): `<config>/assistant/` holds the name,
//!   description and picture, the personality (`persona.md`) and the notes (`memory.md`), all
//!   0600, changed under one lock ([`Assistant::own`]). Every answer reads them anew: the
//!   personality first in the prompt, the notes in its generated part ([`prompt`]). A note marked
//!   [`Note::local_only`] – written by a model on this computer after it read mail – reaches no
//!   remote model or agent ([`data_rule`]) and no client without read access to mail
//!   ([`answer_own`]'s `sees_local`). The assistant's own client may read, add and forget notes,
//!   nothing else of its area (`torro_ipc::api::own_memory`).
//!
//! - **Bots** (task B.1, [`crate::bots`]): a conversation may be with a bot (`bots.ask_bot`, or
//!   `assistant.send_message` with `bot`). It keeps the bot's id ([`ConversationSummary::bot`]), runs
//!   as the bot's client ([`Actor::Bot`]) with the bot's prompt ([`prompt::bot_system`]) and no notes
//!   of the memory, and can be continued only with that bot, and not once it was deleted. The bots
//!   themselves live in `<config>/assistant/bots` ([`Assistant::bots`]).
//! - **Asking a bot** (task B.2, [`delegate`]): the assistant asks a bot only when the person named
//!   it or confirmed it (`assistant.confirm_bot`); the bot then runs in its own conversation with
//!   what both clients may ([`Actor::Delegated`]), and the assistant reads its answer.
//!
//! What goes to a client never names a path; the core's log does.

mod data_rule;
mod delegate;
mod engines;
mod model;
pub(crate) mod profile;
mod prompt;
mod store;
mod tools;

pub use prompt::skill;

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, OnceLock, PoisonError, Weak};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, MutexGuard};
use torro_core::AppId;
use torro_core::i18n::tr;
use torro_core::paths::AppPaths;
use torro_core::settings::Retention;
use torro_ipc::api::Area;
use torro_ipc::assistant::{
    ChatWindow, ConversationSummary, Engine, EngineKind, Entry, EntryContent, MAX_NOTES, Note, Profile, TEST_ENGINE,
};
use torro_ipc::bots::Bot;
use torro_ipc::{Request, Response};
use torro_pim::AccountState;
use torro_setups::launch::{Launcher, SystemLauncher};

use crate::account::ai::AiAccounts;
use crate::bots::Bots;
pub use data_rule::{Place, place_of};
pub use delegate::Asking;
pub use model::{Actor, Delegation, Host, Limits, ModelAccess};
use store::{Conversation, Line};

/// The environment variable that makes a running core offer the test engine: `1`.
pub const TEST_ENGINE_VAR: &str = "TORRO_TEST_ENGINE";

/// The program of the assistant's windows (task C.4): `torroassistant --chat <id>|new`,
/// later `--bot <id>`.
pub const BINARY: &str = "torroassistant";

/// How often the daemon removes conversations past the retention time ([`Assistant::clean_up`]).
pub const CLEAN_UP_EVERY: Duration = Duration::from_secs(60 * 60);

/// How long the test engine takes for each of its two steps in a running core: long enough that
/// a window sees the answer run and can stop it.
const TEST_STEP: Duration = Duration::from_millis(800);

/// Starts a program of the suite with its arguments (`[program, args…]`); the error for people.
type Launch = fn(&[String]) -> Result<(), String>;

/// The assistant's conversations and the answers that run.
pub struct Assistant {
    /// `<data>/assistant`; `None` without a home directory and in tests that pass none: then
    /// nothing can be stored and every change is `error`.
    dir: Option<PathBuf>,
    /// How long each step of the test engine takes; `None` when it is not offered.
    test_engine: Option<Duration>,
    /// Starts `torroassistant` ([`start_app`]); tests record the command instead.
    launch: Launch,
    /// The bounds of an answer with a model.
    limits: Limits,
    /// The coding agents that can answer (engine B).
    agents: engines::Agents,
    /// The core, for answers with a model ([`Assistant::attach`]); weak, since the core holds the
    /// assistant.
    host: OnceLock<Weak<dyn Host>>,
    /// An async lock: it is held while a file is read or written on a blocking thread.
    state: Mutex<State>,
    /// `<config>/assistant`, where the profile and the memory live (task C.10); `None` without a
    /// home directory and in tests that pass none: then the defaults show and every change is
    /// `error`.
    config: Option<PathBuf>,
    /// Held while the profile or the memory is read for a change and written, so two changes never
    /// lose one another.
    own: Mutex<()>,
    /// The bots (task B.1), in `<config>/assistant/bots`; none without a config folder.
    bots: Bots,
}

/// What [`Assistant`] holds in memory.
#[derive(Default)]
struct State {
    /// The folder was read ([`Assistant::state`]).
    loaded: bool,
    conversations: HashMap<String, Conversation>,
    /// The answer that runs per conversation.
    running: HashMap<String, Run>,
    /// The number of the next run, so a stopped run never writes into a newer one.
    next_run: u64,
    /// The bots the person confirmed per assistant conversation (task B.2, `assistant.confirm_bot`):
    /// each allows one ask. In memory only: a restart forgets them, which only ever refuses.
    confirmed: HashMap<String, BTreeSet<String>>,
    /// The bot's conversation per assistant conversation and bot (task B.2): asked again, the bot
    /// goes on there. In memory only: after a restart a new one starts.
    asked: HashMap<(String, String), String>,
    /// The number of the person's last message per conversation (task B.2): sent with the app key,
    /// not by an agent with access to the assistant. Only that message can name a bot. In memory
    /// only: after a restart the person names the bot again.
    said: HashMap<String, u64>,
}

/// An answer that runs.
struct Run {
    id: u64,
    task: tokio::task::AbortHandle,
    /// The engine that answers.
    engine: String,
    /// What the model wrote of its current step so far; empty for the test engine.
    partial: Arc<StdMutex<Partial>>,
}

/// The text a model is writing, before it becomes an answer entry.
#[derive(Debug, Default)]
pub(crate) struct Partial {
    pub text: String,
    /// The model that writes, once known.
    pub model: Option<String>,
    /// When its first piece came.
    pub started: Option<DateTime<Utc>>,
}

impl Run {
    /// The answer as far as the model wrote it, as the entry after `last` it will become; `None`
    /// while it wrote nothing.
    fn growing(&self, last: u64) -> Option<Entry> {
        let partial = self.partial.lock().unwrap_or_else(PoisonError::into_inner);
        if partial.text.trim().is_empty() {
            return None;
        }
        let content = EntryContent::Answer {
            text: partial.text.clone(),
            engine: self.engine.clone(),
            model: partial.model.clone(),
        };
        Some(Entry { seq: last + 1, time: partial.started.unwrap_or_else(Utc::now), content })
    }

    /// Forgets what the model wrote: it is an answer entry now.
    fn clear(&self) {
        self.partial.lock().unwrap_or_else(PoisonError::into_inner).text.clear();
    }
}

impl Assistant {
    /// The user's: `<data>/assistant`, the test engine only with [`TEST_ENGINE_VAR`] set to `1`.
    /// In tests there is no folder and no app is started, so a test that forgets to pass its own
    /// ([`crate::core::Core::with_assistant`]) never touches the user's data.
    pub fn system() -> Assistant {
        if cfg!(test) {
            return Assistant::new(None, None, no_app);
        }
        let dir = AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.data.join("assistant"));
        let test = std::env::var(TEST_ENGINE_VAR).is_ok_and(|value| value == "1");
        let mut assistant = Assistant::new(dir, test.then_some(TEST_STEP), start_app);
        assistant.agents = engines::Agents::system();
        assistant.config = AppPaths::for_app(AppId::WORKPLACE).map(|paths| profile::folder(&paths.config));
        assistant.bots = Bots::new(assistant.config.as_deref().map(crate::bots::folder));
        assistant
    }

    /// The assistant on `dir`, offering the test engine with steps of `test_engine`, starting its
    /// windows with `launch`. Reads nothing yet; answers with a model once a core is attached
    /// ([`Assistant::attach`]). Offers no coding agent ([`Assistant::system`] does).
    pub fn new(dir: Option<PathBuf>, test_engine: Option<Duration>, launch: Launch) -> Assistant {
        Assistant {
            dir,
            test_engine,
            launch,
            limits: Limits::default(),
            agents: engines::Agents::none(),
            host: OnceLock::new(),
            state: Mutex::new(State::default()),
            config: None,
            own: Mutex::new(()),
            bots: Bots::new(None),
        }
    }

    /// The assistant keeping its profile, memory and bots in `dir` (`<config>/assistant`), for tests.
    #[cfg(test)]
    pub(crate) fn with_config(mut self, dir: PathBuf) -> Assistant {
        self.bots = Bots::new(Some(crate::bots::folder(&dir)));
        self.config = Some(dir);
        self
    }

    /// The bots (task B.1).
    pub fn bots(&self) -> &Bots {
        &self.bots
    }

    /// The assistant with other bounds for its answers, for tests.
    #[cfg(test)]
    pub(crate) fn with_limits(mut self, limits: Limits) -> Assistant {
        self.limits = limits;
        self
    }

    /// The assistant with other coding agents, for tests.
    #[cfg(test)]
    pub(crate) fn with_agents(mut self, agents: engines::Agents) -> Assistant {
        self.agents = agents;
        self
    }

    /// Hands the assistant the core it runs tools with (`Core::start`). Until then, and once the
    /// core is gone, an AI account does not answer (`unsupported`). Only the first call counts.
    pub fn attach(&self, host: Weak<dyn Host>) {
        // A second core in one process (tests) keeps the first; nothing else attaches twice.
        let _ = self.host.set(host);
    }

    /// The attached core, while it lives.
    fn host(&self) -> Option<Arc<dyn Host>> {
        self.host.get().and_then(Weak::upgrade)
    }

    /// The state, with the folder read on first use. A folder that cannot be read leaves the
    /// conversations empty and is named on stderr; it is read again on the next use.
    async fn state(&self) -> MutexGuard<'_, State> {
        let mut state = self.state.lock().await;
        if !state.loaded {
            let loaded = match self.dir.clone() {
                Some(dir) => blocking(move || store::load_all(&dir)).await,
                None => Ok(Vec::new()),
            };
            match loaded {
                Ok(loaded) => {
                    for conversation in loaded {
                        state.conversations.insert(conversation.id.clone(), conversation);
                    }
                    state.loaded = true;
                }
                Err(err) => eprintln!("torroworkplace: cannot read the assistant's conversations: {err}"),
            }
        }
        state
    }

    /// Writes `lines` to the file of conversation `id` on a blocking thread; the message of the
    /// `error` answer when that fails ([`failure`]). Called with the state locked, so the lines of
    /// a conversation land in the order of their numbers.
    async fn write(&self, id: &str, lines: Vec<Line>) -> Result<(), &'static str> {
        let Some(dir) = self.dir.clone() else {
            return Err("there is no place to keep conversations (no home directory)");
        };
        let (file, shown) = (id.to_string(), dir.clone());
        blocking(move || store::append(&dir, &file, &lines)).await.map_err(|err| {
            eprintln!("torroworkplace: cannot write the conversation {id} in {}: {err}", shown.display());
            "the conversation cannot be stored"
        })
    }

    /// Removes the file of conversation `id` on a blocking thread; one that is not there is fine.
    async fn remove(&self, id: &str) -> io::Result<()> {
        let Some(dir) = self.dir.clone() else { return Ok(()) };
        let id = id.to_string();
        blocking(move || store::remove(&dir, &id)).await
    }

    /// `assistant.list_engines`: every AI account in the order they were added, then the coding
    /// agents that are installed while the assistant has its own client, then the test engine
    /// where it is offered.
    pub async fn engines(&self, ai: &AiAccounts) -> Vec<Engine> {
        let mut engines: Vec<Engine> = ai
            .infos()
            .await
            .into_iter()
            .map(|account| {
                let problem = match account.state {
                    AccountState::Connected { problem, .. } => problem,
                    _ => None,
                };
                Engine { id: account.id, kind: EngineKind::Ai, name: account.label, problem }
            })
            .collect();
        engines.extend(self.agents.engines(self.has_client()));
        if self.test_engine.is_some() {
            let name = tr("Test engine").to_string();
            engines.push(Engine { id: TEST_ENGINE.into(), kind: EngineKind::Test, name, problem: None });
        }
        engines
    }

    /// The attached core has the assistant's own client now.
    fn has_client(&self) -> bool {
        self.has_client_of(&Actor::Assistant)
    }

    /// The attached core has the client of `actor` now (task B.1).
    fn has_client_of(&self, actor: &Actor) -> bool {
        self.host().is_some_and(|host| host.client_of(actor).is_some())
    }

    /// `assistant.list_conversations`: pinned first, then the most recently active.
    pub async fn list(&self) -> Response {
        let bots = self.bots.ids().await;
        let state = self.state().await;
        let mut conversations: Vec<_> = state
            .conversations
            .values()
            .map(|conversation| with_bots(conversation.summary(state.running.contains_key(&conversation.id)), &bots))
            .collect();
        conversations.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.cmp(&a.updated)).then(a.id.cmp(&b.id)));
        Response::Conversations { conversations }
    }

    /// `assistant.get_conversation`: the conversation and its entries after `after`, and while a
    /// model writes, its growing answer numbered after the conversation's `last` ([`Run::growing`]).
    pub async fn get(&self, id: &str, after: Option<u64>) -> Response {
        let bots = self.bots.ids().await;
        let state = self.state().await;
        let Some(conversation) = state.conversations.get(id) else { return unknown(id) };
        let run = state.running.get(id);
        let mut entries = conversation.entries_after(after);
        if let Some(growing) = run.and_then(|run| run.growing(conversation.last()))
            && growing.seq > after.unwrap_or(0)
        {
            entries.push(growing);
        }
        Response::Conversation { conversation: with_bots(conversation.summary(run.is_some()), &bots), entries }
    }

    /// The title of conversation `id` as the core keeps it, for an approval's summary; `None` for
    /// one it does not have.
    pub async fn title(&self, id: &str) -> Option<String> {
        self.state().await.conversations.get(id).map(|conversation| conversation.title.clone())
    }

    /// What a request would be refused with anyway, checked before anyone is asked to approve it
    /// and again by [`answer_from`]: its limits, a conversation the core does not have (`not-found`),
    /// for a message (also `bots.ask_bot`) an engine that does not answer ([`Assistant::engine`]),
    /// a conversation that still answers (`busy`), a bot that is not there (`not-found`) or a
    /// conversation of another bot or of none ([`Assistant::bot_refusal`]), the window of a
    /// bot that is not there (`not-found`). `None` when it may run.
    pub async fn refusal(&self, ai: &AiAccounts, request: &Request) -> Option<Response> {
        if let Some(refused) =
            torro_ipc::assistant::limit_refusal(request).or_else(|| torro_ipc::bots::limit_refusal(request))
        {
            return Some(refused);
        }
        let named =
            matches!(request, Request::SendMessage { .. } | Request::AskBot { .. } | Request::ConfirmBot { .. });
        let bot = match request.bot().filter(|_| named) {
            Some(id) => match self.bots.get(id).await {
                Some(bot) => Some(bot),
                None => return Some(crate::bots::no_bot(id)),
            },
            None => None,
        };
        if let Some(id) = request.conversation() {
            let state = self.state().await;
            let Some(conversation) = state.conversations.get(id) else { return Some(unknown(id)) };
            if matches!(request, Request::SendMessage { .. } | Request::AskBot { .. }) {
                if let Some(refused) = bot_refusal(conversation, bot.as_ref()) {
                    return Some(refused);
                }
                if state.running.contains_key(id) {
                    return Some(busy());
                }
            }
            // A confirmation is for the assistant's conversations only (task B.2).
            if matches!(request, Request::ConfirmBot { .. })
                && let Some(refused) = bot_refusal(conversation, None)
            {
                return Some(refused);
            }
        }
        let actor = Actor::of(bot.as_ref());
        match (request, &bot) {
            (Request::SendMessage { engine, .. }, _) => self.engine(ai, engine, &actor).await.err(),
            (Request::AskBot { .. }, Some(bot)) => self.engine(ai, &bot.engine, &actor).await.err(),
            (Request::OpenChatWindow { window: ChatWindow::Bot(id) }, _) if self.bots.get(id).await.is_none() => {
                Some(crate::bots::no_bot(id))
            }
            _ => None,
        }
    }

    /// What drives `engine` for `actor`, or why it cannot answer: `not-found` for no engine of
    /// `assistant.list_engines` (an agent that is not installed, or while `actor` has no client),
    /// `unsupported` for an AI account while no core is attached ([`Assistant::attach`]), so
    /// nothing is stored for an answer that would never come.
    async fn engine(&self, ai: &AiAccounts, engine: &str, actor: &Actor) -> Result<EngineKind, Response> {
        if engine == TEST_ENGINE && self.test_engine.is_some() {
            Ok(EngineKind::Test)
        } else if self.agents.find(engine).is_some() && self.has_client_of(actor) {
            Ok(EngineKind::Agent)
        } else if ai.knows(engine).await {
            match self.host() {
                Some(_) => Ok(EngineKind::Ai),
                None => Err(refusal("unsupported", "the assistant answers with AI accounts once the core has started")),
            }
        } else {
            Err(Response::Error { code: "not-found".into(), message: format!("no engine {engine:?}") })
        }
    }

    /// `assistant.send_message` once the engine is known to answer (`kind`): adds the message to
    /// `conversation` or to a new one and starts the answer. Answers with the conversation and the
    /// entries this call added.
    async fn send(self: &Arc<Self>, message: Message, kind: EngineKind, now: DateTime<Utc>) -> Response {
        let mut state = self.state().await;
        let (mut conversation, new) = match &message.conversation {
            Some(id) if state.running.contains_key(id) => return busy(),
            Some(id) => match state.conversations.get(id) {
                Some(conversation) => match bot_refusal(conversation, message.bot.as_ref()) {
                    Some(refused) => return refused,
                    None => (conversation.clone(), false),
                },
                None => return unknown(id),
            },
            None => match store::new_id() {
                Ok(id) => {
                    let mut conversation = Conversation::new(id, &message.text, now);
                    conversation.bot = message.bot.as_ref().map(|bot| bot.id.clone());
                    (conversation, true)
                }
                Err(err) => return failure(&err),
            },
        };
        let before = conversation.last();
        let entry = conversation.next(
            EntryContent::Message {
                text: message.text,
                engine: message.engine.clone(),
                model: message.model.clone(),
                without: message.without.clone(),
            },
            now,
        );
        let mut lines = if new { vec![conversation.started()] } else { Vec::new() };
        lines.push(Line::Entry { entry: entry.clone() });
        if let Err(message) = self.write(&conversation.id, lines).await {
            return failure(message);
        }
        let said = entry.seq;
        conversation.apply(Line::Entry { entry });
        let id = conversation.id.clone();
        if message.person {
            state.said.insert(id.clone(), said);
        }
        let history = conversation.entries.clone();
        state.conversations.insert(id.clone(), conversation);
        let (engine, model, without) = (message.engine, message.model, message.without);
        let actor = match message.on_behalf {
            Some(delegation) => Actor::Delegated(delegation),
            None => Actor::of(message.bot.as_ref()),
        };
        let job = |engine: String| model::Job {
            conversation: id.clone(),
            run: 0,
            engine,
            model,
            without,
            history,
            partial: Arc::default(),
            actor,
            bot: message.bot,
        };
        match kind {
            EngineKind::Test => self.start_test_answer(&mut state, &id, engine),
            EngineKind::Ai => self.start_model_answer(&mut state, &id, job(engine)).await,
            EngineKind::Agent => self.start_agent_answer(&mut state, &id, job(engine)).await,
        }
        let running = state.running.contains_key(&id);
        match state.conversations.get(&id) {
            Some(conversation) => Response::Conversation {
                conversation: conversation.summary(running),
                entries: conversation.entries_after(Some(before)),
            },
            None => unknown(&id),
        }
    }

    /// Stops every answer that runs in a conversation with the bot `bot` (task B.1, once it was
    /// deleted), as `assistant.cancel` does.
    pub async fn stop_bot(&self, bot: &str) {
        let running: Vec<String> = {
            let state = self.state().await;
            state
                .conversations
                .values()
                .filter(|conversation| conversation.bot.as_deref() == Some(bot))
                .filter(|conversation| state.running.contains_key(&conversation.id))
                .map(|conversation| conversation.id.clone())
                .collect()
        };
        for id in running {
            if let Response::Error { message, .. } = self.cancel(&id, Utc::now()).await {
                eprintln!("torroworkplace: cannot stop the answer in conversation {id}: {message}");
            }
        }
    }

    /// Starts the test engine's answer in conversation `id`: after one step a tool line, after
    /// another the answer, both fixed. Registered as the conversation's running answer before it
    /// can write, so `assistant.cancel` stops it.
    fn start_test_answer(self: &Arc<Self>, state: &mut State, id: &str, engine: String) {
        let step = self.test_engine.unwrap_or_default();
        state.next_run += 1;
        let run = state.next_run;
        let (assistant, conversation, by) = (Arc::clone(self), id.to_string(), engine.clone());
        let task = tokio::spawn(async move {
            tokio::time::sleep(step).await;
            let tool = EntryContent::Tool {
                name: "system.ping".into(),
                input: serde_json::json!({}),
                summary: tr("A test step: nothing ran").to_string(),
                error: None,
            };
            if !assistant.add_to_run(&conversation, run, tool, false).await {
                return;
            }
            tokio::time::sleep(step).await;
            let text = tr(
                "This is the test engine. It answers every message the same way, without a model and without tools.",
            );
            let answer = EntryContent::Answer { text: text.to_string(), engine: by, model: None };
            assistant.add_to_run(&conversation, run, answer, true).await;
        });
        let partial = Arc::default();
        state.running.insert(id.to_string(), Run { id: run, task: task.abort_handle(), engine, partial });
    }

    /// Starts the answer `job` with the model of an AI account ([`model::answer`]) in conversation
    /// `id`, registered as its running answer before it can write. Without a core to run tools
    /// with (gone meanwhile), the answer is an error entry at once.
    async fn start_model_answer(self: &Arc<Self>, state: &mut State, id: &str, mut job: model::Job) {
        let Some(host) = self.host() else {
            let error = EntryContent::Error {
                code: "unsupported".into(),
                message: tr("The assistant can answer with AI accounts once Torro Workplace has started.").into(),
            };
            self.add_now(state, id, error).await;
            return;
        };
        state.next_run += 1;
        job.run = state.next_run;
        let (engine, partial) = (job.engine.clone(), Arc::clone(&job.partial));
        let task = tokio::spawn(model::answer(Arc::clone(self), host, job));
        state.running.insert(id.to_string(), Run { id: state.next_run, task: task.abort_handle(), engine, partial });
    }

    /// Starts the answer `job` with the coding agent of its engine ([`engines::answer`]) in
    /// conversation `id`, registered as its running answer before it can write. Without a core
    /// (gone meanwhile) or an agent that is installed, the answer is an error entry at once.
    async fn start_agent_answer(self: &Arc<Self>, state: &mut State, id: &str, mut job: model::Job) {
        let (Some(host), Some(agent)) = (self.host(), self.agents.find(&job.engine)) else {
            let error = EntryContent::Error {
                code: "unsupported".into(),
                message: tr("This agent cannot answer now. Check that it is installed, then try again.").into(),
            };
            self.add_now(state, id, error).await;
            return;
        };
        state.next_run += 1;
        job.run = state.next_run;
        let (engine, partial) = (job.engine.clone(), Arc::clone(&job.partial));
        let task = tokio::spawn(engines::answer(Arc::clone(self), host, job, agent));
        state.running.insert(id.to_string(), Run { id: state.next_run, task: task.abort_handle(), engine, partial });
    }

    /// Ends the run `run` of conversation `id` without another entry, while it is still the
    /// running one.
    async fn end_run(&self, id: &str, run: u64) {
        let mut state = self.state().await;
        if state.running.get(id).is_some_and(|running| running.id == run) {
            state.running.remove(id);
        }
    }

    /// Adds `content` to conversation `id` now, with the state already locked; a failed write is
    /// named on stderr only.
    async fn add_now(&self, state: &mut State, id: &str, content: EntryContent) {
        let Some(entry) = state.conversations.get(id).map(|conversation| conversation.next(content, Utc::now())) else {
            return;
        };
        if self.write(id, vec![Line::Entry { entry: entry.clone() }]).await.is_ok()
            && let Some(conversation) = state.conversations.get_mut(id)
        {
            conversation.apply(Line::Entry { entry });
        }
    }

    /// Adds `content` to conversation `id` while the run `run` is still its running answer; a
    /// `last` one ends the run. An answer takes the place of the growing one. `false` when the run
    /// was stopped or replaced meanwhile, or the entry could not be stored (the run then ends with
    /// nothing more).
    async fn add_to_run(&self, id: &str, run: u64, content: EntryContent, last: bool) -> bool {
        let mut state = self.state().await;
        if state.running.get(id).is_none_or(|running| running.id != run) {
            return false;
        }
        let Some(entry) = state.conversations.get(id).map(|conversation| conversation.next(content, Utc::now())) else {
            state.running.remove(id);
            return false;
        };
        if self.write(id, vec![Line::Entry { entry: entry.clone() }]).await.is_err() {
            state.running.remove(id);
            return false;
        }
        if matches!(entry.content, EntryContent::Answer { .. })
            && let Some(running) = state.running.get(id)
        {
            running.clear();
        }
        if let Some(conversation) = state.conversations.get_mut(id) {
            conversation.apply(Line::Entry { entry });
        }
        if last {
            state.running.remove(id);
        }
        true
    }

    /// `assistant.cancel`: stops the running answer of `id`, keeps what the model wrote so far as
    /// an answer and adds a `stopped` entry, and stops the answers of the bots it asked (task B.2).
    /// Nothing changes when none runs.
    pub async fn cancel(&self, id: &str, now: DateTime<Utc>) -> Response {
        let mut state = self.state().await;
        let Some(conversation) = state.conversations.get(id).cloned() else { return unknown(id) };
        let Some(run) = state.running.remove(id) else { return Response::Ok };
        // Aborted at its next pause; it checks its number under the lock before every write.
        run.task.abort();
        let mut entries: Vec<Entry> = run.growing(conversation.last()).into_iter().collect();
        let after = entries.last().map_or(conversation.last(), |entry| entry.seq);
        entries.push(Entry { seq: after + 1, time: now, content: EntryContent::Stopped });
        let lines: Vec<Line> = entries.into_iter().map(|entry| Line::Entry { entry }).collect();
        if let Err(message) = self.write(id, lines.clone()).await {
            return failure(message);
        }
        if let Some(conversation) = state.conversations.get_mut(id) {
            lines.into_iter().for_each(|line| conversation.apply(line));
        }
        // The bots it asked stop with it (task B.2); a bot's conversation asks none itself.
        let asked: Vec<String> = state
            .asked
            .iter()
            .filter(|((main, _), theirs)| main == id && state.running.contains_key(*theirs))
            .map(|(_, theirs)| theirs.clone())
            .collect();
        drop(state);
        for theirs in asked {
            if let Response::Error { message, .. } = Box::pin(self.cancel(&theirs, now)).await {
                eprintln!("torroworkplace: cannot stop the bot's answer in conversation {theirs}: {message}");
            }
        }
        Response::Ok
    }

    /// `assistant.rename_conversation`; the title trimmed (the limits are checked before).
    pub async fn rename(&self, id: &str, title: &str) -> Response {
        self.change(id, Line::Renamed { title: title.trim().to_string() }).await
    }

    /// `assistant.pin_conversation`.
    pub async fn pin(&self, id: &str, pinned: bool) -> Response {
        self.change(id, Line::Pinned { pinned }).await
    }

    /// Writes `line` to conversation `id` and takes it in.
    async fn change(&self, id: &str, line: Line) -> Response {
        let mut state = self.state().await;
        if !state.conversations.contains_key(id) {
            return unknown(id);
        }
        if let Err(message) = self.write(id, vec![line.clone()]).await {
            return failure(message);
        }
        if let Some(conversation) = state.conversations.get_mut(id) {
            conversation.apply(line);
        }
        Response::Ok
    }

    /// `assistant.delete_conversation`: stops its answer and removes its file for good.
    pub async fn delete(&self, id: &str) -> Response {
        let mut state = self.state().await;
        if !state.conversations.contains_key(id) {
            return unknown(id);
        }
        if let Some(run) = state.running.remove(id) {
            run.task.abort();
        }
        if let Err(err) = self.remove(id).await {
            eprintln!("torroworkplace: cannot delete the conversation {id}: {err}");
            return failure("the conversation cannot be deleted");
        }
        state.conversations.remove(id);
        Response::Ok
    }

    /// Removes every conversation whose last entry is older than `retention` at `now`, except the
    /// pinned ones and those answering (decision 8). Answers how many went; one whose file cannot
    /// be removed stays and is named on stderr.
    pub async fn clean_up(&self, now: DateTime<Utc>, retention: Retention) -> usize {
        let Some(days) = retention.days() else { return 0 };
        if self.dir.is_none() {
            return 0;
        }
        let oldest = now - chrono::Duration::days(i64::from(days));
        let mut state = self.state().await;
        let expired: Vec<String> = state
            .conversations
            .values()
            .filter(|c| !c.pinned && c.updated() < oldest && !state.running.contains_key(&c.id))
            .map(|c| c.id.clone())
            .collect();
        let mut removed = 0;
        for id in expired {
            match self.remove(&id).await {
                Ok(()) => {
                    state.conversations.remove(&id);
                    removed += 1;
                }
                Err(err) => eprintln!("torroworkplace: cannot remove the old conversation {id}: {err}"),
            }
        }
        removed
    }

    /// `assistant.open_window`: starts `torroassistant --chat <id>`, `--chat new` or, for a bot,
    /// `--bot <id>` (task B.3); `not-found` for a conversation or bot the core does not have.
    pub async fn open_window(&self, window: ChatWindow) -> Response {
        let target = match window {
            ChatWindow::Conversation(id) if !self.state().await.conversations.contains_key(&id) => return unknown(&id),
            ChatWindow::Conversation(id) => id,
            ChatWindow::New => "new".to_string(),
            // The app opens the bot's newest conversation or a new one (task B.3).
            ChatWindow::Bot(id) if self.bots.get(&id).await.is_none() => return crate::bots::no_bot(&id),
            ChatWindow::Bot(id) => return self.start(&[BINARY.to_string(), "--bot".into(), id]),
        };
        self.start(&[BINARY.to_string(), "--chat".into(), target])
    }

    /// Starts the assistant app with `command` (its program name first): `Ok`, or `error` when it
    /// does not start.
    fn start(&self, command: &[String]) -> Response {
        match (self.launch)(command) {
            Ok(()) => Response::Ok,
            Err(err) => failure(&format!("cannot start {BINARY}: {err}")),
        }
    }
}

impl Assistant {
    /// `assistant.get_profile`: the stored profile, the defaults where nothing is stored or the files
    /// cannot be read (the core's log says why).
    pub async fn profile(&self) -> Profile {
        let default = tr("Your assistant for this computer").to_string();
        let Some(dir) = self.config.clone() else { return profile::default_profile(&default) };
        let fallback = default.clone();
        blocking(move || profile::load_profile(&dir, &default)).await.unwrap_or_else(|err| {
            eprintln!("torroworkplace: cannot read the assistant's profile: {err}");
            profile::default_profile(&fallback)
        })
    }

    /// `assistant.update_profile` once its limits were checked: `change` applied to the stored
    /// profile and written. Answers with the profile as stored now.
    pub async fn update_profile(&self, change: profile::Change) -> Response {
        let Some(dir) = self.config.clone() else { return failure(NO_PLACE) };
        let _own = self.own.lock().await;
        let updated = profile::apply(self.profile().await, change);
        let saved = updated.clone();
        match blocking(move || profile::save_profile(&dir, &saved)).await {
            Ok(()) => Response::Profile { profile: updated },
            Err(err) => {
                eprintln!("torroworkplace: cannot store the assistant's profile: {err}");
                failure("the profile cannot be stored")
            }
        }
    }

    /// Every note of the memory, oldest first; none when the memory cannot be read (the core's log
    /// says why).
    pub async fn notes(&self) -> Vec<Note> {
        let Some(dir) = self.config.clone() else { return Vec::new() };
        blocking(move || profile::load_notes(&dir)).await.unwrap_or_else(|err| {
            eprintln!("torroworkplace: cannot read the assistant's memory: {err}");
            Vec::new()
        })
    }

    /// The note `id` when the caller may see it: every note when `sees_local`, else none marked
    /// [`Note::local_only`].
    pub async fn note(&self, id: &str, sees_local: bool) -> Option<Note> {
        self.notes().await.into_iter().find(|note| note.id == id && (sees_local || !note.local_only))
    }

    /// What `assistant.remember` or `assistant.forget` would be refused with anyway, checked
    /// before anyone is asked to approve it and again by the handlers: a note the caller does not
    /// see (`not-found`, also one kept from it, [`Assistant::note`]), a full memory for a new note
    /// (`error`). `None` for every other request.
    pub async fn memory_refusal(&self, request: &Request, sees_local: bool) -> Option<Response> {
        match request {
            Request::Forget { id } | Request::Remember { replaces: Some(id), .. } => {
                self.note(id, sees_local).await.is_none().then(|| no_note(id))
            }
            Request::Remember { replaces: None, .. } if self.notes().await.len() >= MAX_NOTES => Some(full()),
            _ => None,
        }
    }

    /// `assistant.list_memories`: every note the caller may see ([`Assistant::note`]).
    pub async fn list_memories(&self, sees_local: bool) -> Response {
        let notes = self.notes().await.into_iter().filter(|note| sees_local || !note.local_only).collect();
        Response::Memories { notes }
    }

    /// `assistant.remember`: a new note with `text` (one line, cleaned), or the note `replaces`
    /// changed in its place with a new time; `local_only` as asked, and kept for a note that
    /// replaces one marked so. Answers with the note as stored.
    pub async fn remember(&self, text: &str, replaces: Option<&str>, local_only: bool, sees_local: bool) -> Response {
        let Some(dir) = self.config.clone() else { return failure(NO_PLACE) };
        let _own = self.own.lock().await;
        let mut notes = self.notes().await;
        let text = profile::clean_line(text, torro_ipc::assistant::MAX_NOTE_CHARS);
        let note = match replaces {
            Some(id) => {
                let Some(old) = notes.iter_mut().find(|note| note.id == id && (sees_local || !note.local_only)) else {
                    return no_note(id);
                };
                old.text = text;
                old.created = Utc::now();
                old.local_only |= local_only;
                old.clone()
            }
            None if notes.len() >= MAX_NOTES => return full(),
            None => {
                let id = match profile::new_note_id(&notes) {
                    Ok(id) => id,
                    Err(err) => return failure(&err),
                };
                let note = Note { id, text, created: Utc::now(), local_only };
                notes.push(note.clone());
                note
            }
        };
        match blocking(move || profile::save_notes(&dir, &notes)).await {
            Ok(()) => Response::Memory { note },
            Err(err) => {
                eprintln!("torroworkplace: cannot store the assistant's memory: {err}");
                failure("the memory cannot be stored")
            }
        }
    }

    /// `assistant.forget`: removes the note `id` the caller may see for good.
    pub async fn forget(&self, id: &str, sees_local: bool) -> Response {
        let Some(dir) = self.config.clone() else { return failure(NO_PLACE) };
        let _own = self.own.lock().await;
        let mut notes = self.notes().await;
        let before = notes.len();
        notes.retain(|note| note.id != id || (!sees_local && note.local_only));
        if notes.len() == before {
            return no_note(id);
        }
        match blocking(move || profile::save_notes(&dir, &notes)).await {
            Ok(()) => Response::Ok,
            Err(err) => {
                eprintln!("torroworkplace: cannot store the assistant's memory: {err}");
                failure("the memory cannot be stored")
            }
        }
    }

    /// What an answer reads of the profile and the memory: the personality and every note. The
    /// prompt keeps notes marked [`Note::local_only`] from remote models itself ([`prompt::system`]).
    async fn persona_and_notes(&self) -> (String, Vec<Note>) {
        (self.profile().await.personality, self.notes().await)
    }
}

/// Answers the requests of the assistant's profile and memory (task C.10): `get_profile`,
/// `update_profile`, `list_memories`, `remember` and `forget`, for the app key and for a client the
/// core admitted. `sees_local`: the caller may see notes kept from remote models – the app key, and
/// a client with read access to mail; for any other a note marked so behaves as if it were not
/// there. What would be refused anyway comes first ([`Assistant::memory_refusal`]).
pub async fn answer_own(assistant: &Assistant, request: Request, sees_local: bool) -> Response {
    if let Some(refused) = torro_ipc::assistant::limit_refusal(&request) {
        return refused;
    }
    if let Some(refused) = assistant.memory_refusal(&request, sees_local).await {
        return refused;
    }
    match request {
        Request::GetProfile => Response::Profile { profile: assistant.profile().await },
        Request::UpdateProfile { name, description, avatar, personality } => {
            assistant.update_profile(profile::Change { name, description, avatar, personality }).await
        }
        Request::ListMemories => assistant.list_memories(sees_local).await,
        Request::Remember { text, replaces, local_only } => {
            assistant.remember(&text, replaces.as_deref(), local_only, sees_local).await
        }
        Request::Forget { id } => assistant.forget(&id, sees_local).await,
        other => failure(&format!("{} is no command of the assistant's profile", other.operation().name())),
    }
}

/// Why a profile or a note cannot be changed without a home directory.
const NO_PLACE: &str = "there is no place to keep the assistant's profile and memory (no home directory)";

/// The `not-found` answer for note `id`.
fn no_note(id: &str) -> Response {
    Response::Error { code: "not-found".into(), message: format!("no note {id:?}") }
}

/// The `error` answer for a new note in a full memory.
fn full() -> Response {
    failure(&format!("the memory is full ({MAX_NOTES} notes); forget some first"))
}

/// Runs `work` on a blocking thread; a thread that panicked is an I/O error.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> io::Result<T> + Send + 'static) -> io::Result<T> {
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|err| Err(io::Error::other(err)))
}

/// What `assistant.send_message` (or `bots.ask_bot`) asks for, past its limits.
struct Message {
    conversation: Option<String>,
    engine: String,
    model: Option<String>,
    text: String,
    without: Vec<Area>,
    /// The bot that answers, as stored now (task B.1); `None` for the assistant.
    bot: Option<Bot>,
    /// The bot runs for the assistant (task B.2): as [`Actor::Delegated`], with what both clients
    /// may. `None`: as its own client (or the assistant as its own).
    on_behalf: Option<Delegation>,
    /// The person wrote it (task B.2): sent with the app key, not by a client. Only the person's
    /// message can name a bot for the assistant to ask ([`State::said`]).
    person: bool,
}

/// Why a message for `bot` (`None`: the assistant) cannot go into `conversation` (task B.1): a
/// conversation belongs to the bot it started with for good, so `error` when the message names no
/// bot for a conversation with one, a bot for one with the assistant, or another bot. `None` when
/// they match.
fn bot_refusal(conversation: &Conversation, bot: Option<&Bot>) -> Option<Response> {
    match (conversation.bot.as_deref(), bot.map(|bot| bot.id.as_str())) {
        (None, None) => None,
        (Some(theirs), Some(mine)) if theirs == mine => None,
        (Some(_), None) => Some(failure("this conversation is with a bot; pass its id as `bot`")),
        (None, Some(_)) => Some(failure("this conversation is with the assistant, not with a bot")),
        (Some(_), Some(_)) => Some(failure("this conversation is with another bot")),
    }
}

/// `summary` with [`ConversationSummary::bot_deleted`] set when its bot is none of `bots`.
fn with_bots(mut summary: ConversationSummary, bots: &[String]) -> ConversationSummary {
    summary.bot_deleted = summary.bot.as_ref().is_some_and(|bot| !bots.contains(bot));
    summary
}

/// [`answer_from`] as for a client, for tests: a message counts as no person's.
#[cfg(test)]
pub async fn answer(assistant: &Arc<Assistant>, ai: &AiAccounts, request: Request) -> Response {
    answer_from(assistant, ai, request, false).await
}

/// Answers an `assistant.*` request and `bots.ask_bot` (task B.1), for the app key and for a client
/// the core admitted ([`crate::core::Core::handle_from`]). `person`: the app key sent it (task B.2),
/// so a message is the person's own, and only such a message can name a bot for the assistant to
/// ask. What would be refused anyway comes first ([`Assistant::refusal`]); the handlers check again
/// under their lock (a conversation may have been deleted or started answering meanwhile).
pub async fn answer_from(assistant: &Arc<Assistant>, ai: &AiAccounts, request: Request, person: bool) -> Response {
    if let Some(refused) = assistant.refusal(ai, &request).await {
        return refused;
    }
    match request {
        Request::ListEngines => Response::Engines { engines: assistant.engines(ai).await },
        Request::ListConversations => assistant.list().await,
        Request::GetConversation { id, after } => assistant.get(&id, after).await,
        Request::SendMessage { conversation, engine, model, text, without, bot } => {
            let bot = match bot {
                Some(id) => match assistant.bots.get(&id).await {
                    Some(bot) => Some(bot),
                    None => return crate::bots::no_bot(&id),
                },
                None => None,
            };
            // Without a model, the bot's own when it answers with its own engine.
            let model =
                model.or_else(|| bot.as_ref().filter(|bot| bot.engine == engine).and_then(|bot| bot.model.clone()));
            let kind = match assistant.engine(ai, &engine, &Actor::of(bot.as_ref())).await {
                Ok(kind) => kind,
                Err(refused) => return refused,
            };
            let message = Message { conversation, engine, model, text, without, bot, on_behalf: None, person };
            assistant.send(message, kind, Utc::now()).await
        }
        Request::AskBot { bot, text, conversation } => {
            let Some(bot) = assistant.bots.get(&bot).await else { return crate::bots::no_bot(&bot) };
            let kind = match assistant.engine(ai, &bot.engine, &Actor::of(Some(&bot))).await {
                Ok(kind) => kind,
                Err(refused) => return refused,
            };
            let (engine, model) = (bot.engine.clone(), bot.model.clone());
            let message = Message {
                conversation,
                engine,
                model,
                text,
                without: Vec::new(),
                bot: Some(bot),
                on_behalf: None,
                person: false,
            };
            assistant.send(message, kind, Utc::now()).await
        }
        Request::CancelAnswer { conversation } => assistant.cancel(&conversation, Utc::now()).await,
        Request::RenameConversation { id, title } => assistant.rename(&id, &title).await,
        Request::DeleteConversation { id } => assistant.delete(&id).await,
        Request::PinConversation { id, pinned } => assistant.pin(&id, pinned).await,
        Request::OpenChatWindow { window } => assistant.open_window(window).await,
        Request::ConfirmBot { conversation, bot } => assistant.confirm_bot(&conversation, &bot).await,
        other => failure(&format!("{} is no command of the assistant", other.operation().name())),
    }
}

/// `torroassistant` with `arguments[1..]`, from next to the core's binary or `PATH`, like
/// `apps.open_app`. A missing app fails here, as any app that is not installed.
fn start_app(arguments: &[String]) -> Result<(), String> {
    let mut command = arguments.to_vec();
    if let Some(program) = command.first_mut() {
        *program = crate::core::sibling(program).to_string_lossy().into_owned();
    }
    SystemLauncher.spawn(&command).map(|_| ()).map_err(|err| err.to_string())
}

/// What tests start instead of the app: nothing.
fn no_app(_: &[String]) -> Result<(), String> {
    Err("no apps in tests".into())
}

/// The `busy` answer for a message to a conversation that still answers.
fn busy() -> Response {
    refusal("busy", "the assistant still answers in this conversation; stop it or wait")
}

/// The `not-found` answer for conversation `id`.
fn unknown(id: &str) -> Response {
    Response::Error { code: "not-found".into(), message: format!("no conversation {id:?}") }
}

/// An error answer with `code`.
fn refusal(code: &str, message: &str) -> Response {
    Response::Error { code: code.into(), message: message.into() }
}

/// The `error` answer.
fn failure(message: &str) -> Response {
    refusal("error", message)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex as StdMutex;

    use chrono::TimeZone;
    use torro_ipc::assistant::{ConversationSummary, Entry};

    pub(crate) use super::model::tests::{Script, Scripted, calls, says};
    use super::*;

    /// An assistant in `dir/assistant` with the test engine taking `step` per step, starting no app.
    pub(crate) fn assistant_in(dir: &std::path::Path, step: Duration) -> Assistant {
        Assistant::new(Some(dir.join("assistant")), Some(step), no_app).with_config(dir.join("config/assistant"))
    }

    /// AI accounts in `dir` that know none and ask no server.
    pub(crate) fn no_ai(dir: &std::path::Path) -> AiAccounts {
        let keys = Arc::new(crate::account::github::tests::MemoryTokens::default());
        AiAccounts::new(Some(dir.join("accounts.json")), keys, None, Vec::new())
    }

    fn send(conversation: Option<&str>, text: &str) -> Request {
        Request::SendMessage {
            conversation: conversation.map(str::to_string),
            engine: TEST_ENGINE.into(),
            model: None,
            text: text.into(),
            without: vec![Area::Mail],
            bot: None,
        }
    }

    fn opened(response: Response) -> (ConversationSummary, Vec<Entry>) {
        match response {
            Response::Conversation { conversation, entries } => (conversation, entries),
            other => panic!("expected a conversation, got {other:?}"),
        }
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn kinds(entries: &[Entry]) -> Vec<&'static str> {
        entries
            .iter()
            .map(|entry| match entry.content {
                EntryContent::Message { .. } => "message",
                EntryContent::Answer { .. } => "answer",
                EntryContent::Tool { .. } => "tool",
                EntryContent::Approval { .. } => "approval",
                EntryContent::Error { .. } => "error",
                EntryContent::Stopped => "stopped",
            })
            .collect()
    }

    /// Asks for conversation `id` until no answer runs, at most two seconds.
    async fn finished(assistant: &Arc<Assistant>, ai: &AiAccounts, id: &str) -> (ConversationSummary, Vec<Entry>) {
        for _ in 0..200 {
            let (summary, entries) =
                opened(answer(assistant, ai, Request::GetConversation { id: id.into(), after: None }).await);
            if !summary.running {
                return (summary, entries);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the answer of {id} never finished");
    }

    #[tokio::test]
    async fn a_message_starts_a_conversation_and_the_test_engine_answers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::ZERO)), no_ai(dir.path()));

        let (started, added) = opened(answer(&assistant, &ai, send(None, "What is on tomorrow?\nThanks")).await);
        assert!(store::is_id(&started.id), "{}", started.id);
        assert_eq!(started.title, "What is on tomorrow?");
        assert_eq!((started.engine.as_str(), started.last), (TEST_ENGINE, 1));
        assert_eq!(kinds(&added), ["message"]);
        assert!(
            matches!(&added[0].content, EntryContent::Message { without, .. } if *without == vec![Area::Mail]),
            "{added:?}"
        );

        let (done, entries) = finished(&assistant, &ai, &started.id).await;
        assert_eq!(kinds(&entries), ["message", "tool", "answer"]);
        assert_eq!(entries.iter().map(|entry| entry.seq).collect::<Vec<_>>(), [1, 2, 3]);
        assert!(
            matches!(&entries[2].content, EntryContent::Answer { engine, model: None, .. } if engine == TEST_ENGINE),
            "the answer names its engine"
        );
        assert_eq!(done.last, 3);

        // Read from `after`: only what is newer.
        let after = Request::GetConversation { id: started.id.clone(), after: Some(1) };
        assert_eq!(kinds(&opened(answer(&assistant, &ai, after).await).1), ["tool", "answer"]);
        let none = Request::GetConversation { id: started.id.clone(), after: Some(3) };
        assert_eq!(opened(answer(&assistant, &ai, none).await).1, Vec::new());

        // The next message continues it; its answer comes after.
        let (_, added) = opened(answer(&assistant, &ai, send(Some(&started.id), "And the day after?")).await);
        assert_eq!(added.iter().map(|entry| entry.seq).collect::<Vec<_>>(), [4]);
        let (_, entries) = finished(&assistant, &ai, &started.id).await;
        assert_eq!(entries.len(), 6);

        // Everything is on disk: a new core reads the same.
        let again = Arc::new(assistant_in(dir.path(), Duration::ZERO));
        let (_, reread) = opened(answer(&again, &ai, Request::GetConversation { id: started.id, after: None }).await);
        assert_eq!(reread, entries);
    }

    #[tokio::test]
    async fn unknown_conversations_and_engines_are_not_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::ZERO)), no_ai(dir.path()));
        let unknown = "0123456789abcdef";
        for request in [
            send(Some(unknown), "Hi"),
            Request::GetConversation { id: unknown.into(), after: None },
            Request::CancelAnswer { conversation: unknown.into() },
            Request::RenameConversation { id: unknown.into(), title: "T".into() },
            Request::DeleteConversation { id: unknown.into() },
            Request::PinConversation { id: unknown.into(), pinned: true },
            Request::OpenChatWindow { window: ChatWindow::Conversation(unknown.into()) },
        ] {
            assert_eq!(code(&answer(&assistant, &ai, request.clone()).await), "not-found", "{request:?}");
            let before = assistant.refusal(&ai, &request).await;
            assert_eq!(before.as_ref().map(code), Some("not-found"), "{request:?} is refused before an approval");
        }
        let other = Request::SendMessage {
            conversation: None,
            engine: "ai:ollama@localhost:11434".into(),
            model: None,
            text: "Hi".into(),
            without: Vec::new(),
            bot: None,
        };
        assert_eq!(code(&answer(&assistant, &ai, other).await), "not-found", "no such engine");
        assert_eq!(code(&answer(&assistant, &ai, send(None, " ")).await), "error", "limits first");

        // Without the test engine offered, `test` is no engine either.
        let plain = Arc::new(Assistant::new(Some(dir.path().join("plain")), None, no_app));
        assert_eq!(code(&answer(&plain, &ai, send(None, "Hi")).await), "not-found");
        assert_eq!(answer(&plain, &ai, Request::ListEngines).await, Response::Engines { engines: Vec::new() });
        assert_eq!(
            answer(&plain, &ai, Request::ListConversations).await,
            Response::Conversations { conversations: vec![] }
        );
    }

    #[tokio::test]
    async fn the_engines_are_the_ai_accounts_and_the_test_engine() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = crate::account::github::tests::MemoryTokens::default();
        let ollama = crate::account::ai::tests::fake_ollama().await;
        let ai = crate::account::ai::tests::service(dir.path(), &keys, Vec::new());
        let account = ai
            .connect(
                torro_core::ai::AiProvider::Ollama,
                Some(&ollama.url("/v1")),
                torro_core::account::Secret::default(),
            )
            .await
            .expect("connected");
        let assistant = Arc::new(assistant_in(dir.path(), Duration::ZERO));

        let engines = assistant.engines(&ai).await;
        assert_eq!(
            engines,
            vec![
                Engine { id: account.id.clone(), kind: EngineKind::Ai, name: account.label.clone(), problem: None },
                Engine {
                    id: TEST_ENGINE.into(),
                    kind: EngineKind::Test,
                    name: tr("Test engine").into(),
                    problem: None
                },
            ]
        );
        // Without a core to run tools with (none attached here) an AI account does not answer, and
        // nothing is stored.
        let to_ollama = Request::SendMessage {
            conversation: None,
            engine: account.id,
            model: Some("qwen3:8b".into()),
            text: "Hi".into(),
            without: Vec::new(),
            bot: None,
        };
        assert_eq!(code(&answer(&assistant, &ai, to_ollama).await), "unsupported");
        assert_eq!(
            answer(&assistant, &ai, Request::ListConversations).await,
            Response::Conversations { conversations: vec![] }
        );
    }

    #[tokio::test]
    async fn cancel_stops_a_running_answer_and_a_busy_conversation_waits() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::from_secs(60))), no_ai(dir.path()));
        let (started, _) = opened(answer(&assistant, &ai, send(None, "Plan my week")).await);
        assert!(started.running);
        assert_eq!(code(&answer(&assistant, &ai, send(Some(&started.id), "And?")).await), "busy");

        let cancel = Request::CancelAnswer { conversation: started.id.clone() };
        assert_eq!(answer(&assistant, &ai, cancel.clone()).await, Response::Ok);
        let (stopped, entries) =
            opened(answer(&assistant, &ai, Request::GetConversation { id: started.id.clone(), after: None }).await);
        assert!(!stopped.running);
        assert_eq!(kinds(&entries), ["message", "stopped"]);
        // Nothing runs any more: cancelling again changes nothing, a new message is taken.
        assert_eq!(answer(&assistant, &ai, cancel).await, Response::Ok);
        assert_eq!(
            opened(answer(&assistant, &ai, Request::GetConversation { id: started.id.clone(), after: Some(2) }).await)
                .1,
            vec![]
        );
        let (_, added) = opened(answer(&assistant, &ai, send(Some(&started.id), "Again")).await);
        assert_eq!(added.iter().map(|entry| entry.seq).collect::<Vec<_>>(), [3]);
    }

    #[tokio::test]
    async fn conversations_are_renamed_pinned_listed_and_deleted_for_good() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::ZERO)), no_ai(dir.path()));
        let (first, _) = opened(answer(&assistant, &ai, send(None, "First")).await);
        finished(&assistant, &ai, &first.id).await;
        let (second, _) = opened(answer(&assistant, &ai, send(None, "Second")).await);
        finished(&assistant, &ai, &second.id).await;

        let rename = Request::RenameConversation { id: first.id.clone(), title: "  Trip to Lisbon ".into() };
        assert_eq!(answer(&assistant, &ai, rename).await, Response::Ok);
        assert_eq!(
            answer(&assistant, &ai, Request::PinConversation { id: first.id.clone(), pinned: true }).await,
            Response::Ok
        );
        let listed = |response: Response| match response {
            Response::Conversations { conversations } => conversations,
            other => panic!("expected conversations, got {other:?}"),
        };
        let list = listed(answer(&assistant, &ai, Request::ListConversations).await);
        assert_eq!(
            list.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            [first.id.as_str(), second.id.as_str()],
            "pinned first"
        );
        assert_eq!((list[0].title.as_str(), list[0].pinned), ("Trip to Lisbon", true));

        let file = dir.path().join("assistant").join(format!("{}.jsonl", second.id));
        assert!(file.exists());
        assert_eq!(answer(&assistant, &ai, Request::DeleteConversation { id: second.id.clone() }).await, Response::Ok);
        assert!(!file.exists(), "deleting removes the file");
        let again = Arc::new(assistant_in(dir.path(), Duration::ZERO));
        let list = listed(answer(&again, &ai, Request::ListConversations).await);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].title.as_str(), list[0].pinned), ("Trip to Lisbon", true), "kept on disk");
    }

    #[tokio::test]
    async fn retention_removes_old_unpinned_conversations_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::from_secs(60))), no_ai(dir.path()));
        let then = Utc.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).single().expect("valid time");
        let mut ids = Vec::new();
        for text in ["old", "old and pinned", "old and answering", "recent"] {
            let (summary, _) = opened(answer(&assistant, &ai, send(None, text)).await);
            ids.push(summary.id);
        }
        // Not answering, except the third.
        for id in [&ids[0], &ids[1], &ids[3]] {
            assert_eq!(answer(&assistant, &ai, Request::CancelAnswer { conversation: id.clone() }).await, Response::Ok);
        }
        assert_eq!(
            answer(&assistant, &ai, Request::PinConversation { id: ids[1].clone(), pinned: true }).await,
            Response::Ok
        );

        // 31 days after the last entry of all but the recent one, which is 29 days old then.
        let now = Utc::now();
        {
            let mut state = assistant.state().await;
            for (id, age) in [(&ids[0], 31), (&ids[1], 31), (&ids[2], 31), (&ids[3], 29)] {
                let conversation = state.conversations.get_mut(id).expect("held");
                conversation.created = then;
                for entry in &mut conversation.entries {
                    entry.time = now - chrono::Duration::days(age);
                }
            }
        }
        assert_eq!(assistant.clean_up(now, Retention::Forever).await, 0, "forever keeps everything");
        assert_eq!(assistant.clean_up(now, Retention::Days(30)).await, 1);
        let kept: Vec<String> = match assistant.list().await {
            Response::Conversations { conversations } => conversations.into_iter().map(|c| c.id).collect(),
            other => panic!("expected conversations, got {other:?}"),
        };
        assert!(!kept.contains(&ids[0]), "old and unpinned goes");
        for id in &ids[1..] {
            assert!(kept.contains(id), "pinned, answering and recent stay");
        }
        assert!(!dir.path().join("assistant").join(format!("{}.jsonl", ids[0])).exists(), "its file too");
    }

    #[tokio::test]
    async fn windows_open_a_conversation_a_new_one_or_a_bots_chat() {
        static STARTED: StdMutex<Vec<Vec<String>>> = StdMutex::new(Vec::new());
        fn record(command: &[String]) -> Result<(), String> {
            STARTED.lock().unwrap_or_else(PoisonError::into_inner).push(command.to_vec());
            Ok(())
        }
        fn missing(_: &[String]) -> Result<(), String> {
            Err("No such file or directory".into())
        }
        let dir = tempfile::tempdir().expect("temp dir");
        let ai = no_ai(dir.path());
        let assistant = Arc::new(
            Assistant::new(Some(dir.path().join("assistant")), Some(Duration::ZERO), record)
                .with_config(dir.path().join("config/assistant")),
        );
        let (started, _) = opened(answer(&assistant, &ai, send(None, "Hi")).await);
        let clients = crate::clients::tests::open_in(dir.path());
        let save = Request::SaveBot {
            id: None,
            name: "Travel planner".into(),
            description: String::new(),
            avatar: None,
            engine: TEST_ENGINE.into(),
            model: None,
            personality: String::new(),
            instructions: String::new(),
            skills: Vec::new(),
        };
        let bot = match crate::bots::answer(&assistant, &clients, save).await {
            Response::Bot { bot } => bot,
            other => panic!("expected a bot, got {other:?}"),
        };

        let open = |window| Request::OpenChatWindow { window };
        assert_eq!(answer(&assistant, &ai, open(ChatWindow::Conversation(started.id.clone()))).await, Response::Ok);
        assert_eq!(answer(&assistant, &ai, open(ChatWindow::New)).await, Response::Ok);
        assert_eq!(answer(&assistant, &ai, open(ChatWindow::Bot(bot.id.clone()))).await, Response::Ok);
        let unknown_bot = open(ChatWindow::Bot("0a0b0c0d".into()));
        assert_eq!(code(&answer(&assistant, &ai, unknown_bot.clone()).await), "not-found");
        assert_eq!(
            assistant.refusal(&ai, &unknown_bot).await.as_ref().map(code),
            Some("not-found"),
            "before an approval"
        );
        let started_commands = STARTED.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert_eq!(
            started_commands,
            vec![
                vec![BINARY.to_string(), "--chat".into(), started.id.clone()],
                vec![BINARY.to_string(), "--chat".into(), "new".into()],
                vec![BINARY.to_string(), "--bot".into(), bot.id.clone()],
            ]
        );

        let not_installed = Arc::new(Assistant::new(Some(dir.path().join("assistant")), Some(Duration::ZERO), missing));
        assert_eq!(code(&answer(&not_installed, &ai, open(ChatWindow::New)).await), "error");
    }

    #[tokio::test]
    async fn without_a_folder_nothing_is_stored() {
        let dir = tempfile::tempdir().expect("temp dir");
        let ai = no_ai(dir.path());
        let assistant = Arc::new(Assistant::new(None, Some(Duration::ZERO), no_app));
        assert_eq!(code(&answer(&assistant, &ai, send(None, "Hi")).await), "error");
        assert_eq!(
            answer(&assistant, &ai, Request::ListConversations).await,
            Response::Conversations { conversations: vec![] }
        );
    }
}
