//! The assistant of the core (tasks C.1 and C.2, `docs/assistent.md`): its conversations, the
//! engines that answer, and the handlers of the `assistant.*` requests ([`answer`]).
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
//! - **Engines** ([`Assistant::engines`]): every AI account (engine A, [`model`]), and the test
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
//!
//! What goes to a client never names a path; the core's log does.

mod data_rule;
mod model;
mod prompt;
mod store;
mod tools;

pub use prompt::skill;

use std::collections::HashMap;
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
use torro_ipc::assistant::{ChatWindow, Engine, EngineKind, Entry, EntryContent, TEST_ENGINE};
use torro_ipc::{Request, Response};
use torro_pim::AccountState;
use torro_setups::launch::{Launcher, SystemLauncher};

use crate::account::ai::AiAccounts;
pub use data_rule::{Place, place_of};
pub use model::{Host, Limits, ModelAccess};
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
    /// The core, for answers with a model ([`Assistant::attach`]); weak, since the core holds the
    /// assistant.
    host: OnceLock<Weak<dyn Host>>,
    /// An async lock: it is held while a file is read or written on a blocking thread.
    state: Mutex<State>,
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
        Assistant::new(dir, test.then_some(TEST_STEP), start_app)
    }

    /// The assistant on `dir`, offering the test engine with steps of `test_engine`, starting its
    /// windows with `launch`. Reads nothing yet; answers with a model once a core is attached
    /// ([`Assistant::attach`]).
    pub fn new(dir: Option<PathBuf>, test_engine: Option<Duration>, launch: Launch) -> Assistant {
        Assistant {
            dir,
            test_engine,
            launch,
            limits: Limits::default(),
            host: OnceLock::new(),
            state: Mutex::new(State::default()),
        }
    }

    /// The assistant with other bounds for its answers, for tests.
    #[cfg(test)]
    pub(crate) fn with_limits(mut self, limits: Limits) -> Assistant {
        self.limits = limits;
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

    /// `assistant.list_engines`: every AI account in the order they were added, then the test
    /// engine where it is offered.
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
        if self.test_engine.is_some() {
            let name = tr("Test engine").to_string();
            engines.push(Engine { id: TEST_ENGINE.into(), kind: EngineKind::Test, name, problem: None });
        }
        engines
    }

    /// `assistant.list_conversations`: pinned first, then the most recently active.
    pub async fn list(&self) -> Response {
        let state = self.state().await;
        let mut conversations: Vec<_> = state
            .conversations
            .values()
            .map(|conversation| conversation.summary(state.running.contains_key(&conversation.id)))
            .collect();
        conversations.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.cmp(&a.updated)).then(a.id.cmp(&b.id)));
        Response::Conversations { conversations }
    }

    /// `assistant.get_conversation`: the conversation and its entries after `after`, and while a
    /// model writes, its growing answer numbered after the conversation's `last` ([`Run::growing`]).
    pub async fn get(&self, id: &str, after: Option<u64>) -> Response {
        let state = self.state().await;
        let Some(conversation) = state.conversations.get(id) else { return unknown(id) };
        let run = state.running.get(id);
        let mut entries = conversation.entries_after(after);
        if let Some(growing) = run.and_then(|run| run.growing(conversation.last()))
            && growing.seq > after.unwrap_or(0)
        {
            entries.push(growing);
        }
        Response::Conversation { conversation: conversation.summary(run.is_some()), entries }
    }

    /// The title of conversation `id` as the core keeps it, for an approval's summary; `None` for
    /// one it does not have.
    pub async fn title(&self, id: &str) -> Option<String> {
        self.state().await.conversations.get(id).map(|conversation| conversation.title.clone())
    }

    /// What a request would be refused with anyway, checked before anyone is asked to approve it
    /// and again by [`answer`]: its limits, a conversation the core does not have (`not-found`),
    /// for a message an engine that does not answer ([`Assistant::engine`]) or a conversation that
    /// still answers (`busy`), a bot's window (`unsupported`). `None` when it may run.
    pub async fn refusal(&self, ai: &AiAccounts, request: &Request) -> Option<Response> {
        if let Some(refused) = torro_ipc::assistant::limit_refusal(request) {
            return Some(refused);
        }
        if let Some(id) = request.conversation() {
            let state = self.state().await;
            if !state.conversations.contains_key(id) {
                return Some(unknown(id));
            }
            if matches!(request, Request::SendMessage { .. }) && state.running.contains_key(id) {
                return Some(busy());
            }
        }
        match request {
            Request::SendMessage { engine, .. } => self.engine(ai, engine).await.err(),
            Request::OpenChatWindow { window: ChatWindow::Bot(_) } => Some(no_bots()),
            _ => None,
        }
    }

    /// What drives `engine`, or why it cannot answer: `not-found` for no engine of
    /// `assistant.list_engines`, `unsupported` for an AI account while no core is attached
    /// ([`Assistant::attach`]), so nothing is stored for an answer that would never come.
    async fn engine(&self, ai: &AiAccounts, engine: &str) -> Result<EngineKind, Response> {
        if engine == TEST_ENGINE && self.test_engine.is_some() {
            Ok(EngineKind::Test)
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
                Some(conversation) => (conversation.clone(), false),
                None => return unknown(id),
            },
            None => match store::new_id() {
                Ok(id) => (Conversation::new(id, &message.text, now), true),
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
        conversation.apply(Line::Entry { entry });
        let id = conversation.id.clone();
        let history = conversation.entries.clone();
        state.conversations.insert(id.clone(), conversation);
        match kind {
            EngineKind::Test => self.start_test_answer(&mut state, &id, message.engine),
            EngineKind::Ai => {
                let (engine, model, without) = (message.engine, message.model, message.without);
                self.start_model_answer(
                    &mut state,
                    &id,
                    model::Job {
                        conversation: id.clone(),
                        run: 0,
                        engine,
                        model,
                        without,
                        history,
                        partial: Arc::default(),
                    },
                )
                .await;
            }
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
    /// an answer and adds a `stopped` entry. Nothing changes when none runs.
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

    /// `assistant.open_window`: starts `torroassistant --chat <id>` or `--chat new`. Bots do not
    /// exist yet (stage 2): `unsupported`.
    pub async fn open_window(&self, window: ChatWindow) -> Response {
        let target = match window {
            ChatWindow::Conversation(id) if !self.state().await.conversations.contains_key(&id) => return unknown(&id),
            ChatWindow::Conversation(id) => id,
            ChatWindow::New => "new".to_string(),
            ChatWindow::Bot(_) => return no_bots(),
        };
        match (self.launch)(&[BINARY.to_string(), "--chat".into(), target]) {
            Ok(()) => Response::Ok,
            Err(err) => failure(&format!("cannot start {BINARY}: {err}")),
        }
    }
}

/// Runs `work` on a blocking thread; a thread that panicked is an I/O error.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> io::Result<T> + Send + 'static) -> io::Result<T> {
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|err| Err(io::Error::other(err)))
}

/// What `assistant.send_message` asks for, past its limits.
struct Message {
    conversation: Option<String>,
    engine: String,
    model: Option<String>,
    text: String,
    without: Vec<Area>,
}

/// Answers an `assistant.*` request, for the app key and for a client the core admitted
/// ([`crate::core::Core::handle_from`]). What would be refused anyway comes first
/// ([`Assistant::refusal`]); the handlers check again under their lock (a conversation may have
/// been deleted or started answering meanwhile).
pub async fn answer(assistant: &Arc<Assistant>, ai: &AiAccounts, request: Request) -> Response {
    if let Some(refused) = assistant.refusal(ai, &request).await {
        return refused;
    }
    match request {
        Request::ListEngines => Response::Engines { engines: assistant.engines(ai).await },
        Request::ListConversations => assistant.list().await,
        Request::GetConversation { id, after } => assistant.get(&id, after).await,
        Request::SendMessage { conversation, engine, model, text, without } => {
            let kind = match assistant.engine(ai, &engine).await {
                Ok(kind) => kind,
                Err(refused) => return refused,
            };
            assistant.send(Message { conversation, engine, model, text, without }, kind, Utc::now()).await
        }
        Request::CancelAnswer { conversation } => assistant.cancel(&conversation, Utc::now()).await,
        Request::RenameConversation { id, title } => assistant.rename(&id, &title).await,
        Request::DeleteConversation { id } => assistant.delete(&id).await,
        Request::PinConversation { id, pinned } => assistant.pin(&id, pinned).await,
        Request::OpenChatWindow { window } => assistant.open_window(window).await,
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

/// The `unsupported` answer for a bot's window: bots are stage 2.
fn no_bots() -> Response {
    refusal("unsupported", "there are no bots yet")
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
        Assistant::new(Some(dir.join("assistant")), Some(step), no_app)
    }

    /// AI accounts in `dir` that know none and ask no server.
    fn no_ai(dir: &std::path::Path) -> AiAccounts {
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
    async fn windows_open_a_conversation_or_a_new_one_and_bots_wait() {
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
        let assistant = Arc::new(Assistant::new(Some(dir.path().join("assistant")), Some(Duration::ZERO), record));
        let (started, _) = opened(answer(&assistant, &ai, send(None, "Hi")).await);

        let open = |window| Request::OpenChatWindow { window };
        assert_eq!(answer(&assistant, &ai, open(ChatWindow::Conversation(started.id.clone()))).await, Response::Ok);
        assert_eq!(answer(&assistant, &ai, open(ChatWindow::New)).await, Response::Ok);
        assert_eq!(code(&answer(&assistant, &ai, open(ChatWindow::Bot("travel".into()))).await), "unsupported");
        let started_commands = STARTED.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert_eq!(
            started_commands,
            vec![
                vec![BINARY.to_string(), "--chat".into(), started.id.clone()],
                vec![BINARY.to_string(), "--chat".into(), "new".into()],
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
