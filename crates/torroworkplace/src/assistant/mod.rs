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
//!   [`Assistant::clean_up`], pinned ones and those answering never. What a tool answered the model
//!   (issue #701) stays [`store::RESULT_HOURS`] after its call, in every conversation not answering,
//!   pinned ones and any retention too; then `clean_up` drops it from the file (issue #714). The web's
//!   results are never kept ([`tools::kept_for_later`]). Files are read and written on the blocking
//!   threads, under the conversations' lock, so their lines keep the order of their numbers.
//! - **Engines** ([`Assistant::engines`]): every AI account (engine A, [`model`]), the coding
//!   agents while they are offered (engine B, [`engines`] — never in the product since task C.18,
//!   issue #219, so in practice no agent), and the test
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
//!   client's rights, collections and approvals. What the model reads follows from those rights and
//!   the model chosen (issue #664): no data rule holds a result back.
//! - **Audit**: the core logs the conversation id and the engine, never a text
//!   ([`crate::audit::ids`]); the lines of the assistant's tools name the engine too.
//! - **Profile and memory** (task C.10, [`profile`]): `<config>/assistant/` holds the name,
//!   description and picture, the personality (`persona.md`) and the notes (`memory.md`), all
//!   0600, changed under one lock ([`Assistant::own`]). Every answer reads them anew: the
//!   personality first in the prompt, the notes in its generated part ([`prompt`]). A note marked
//!   [`Note::local_only`] by an older version (issue #664: no longer set) reaches no client without
//!   read access to mail ([`answer_own`]'s `sees_local`). The assistant's own client may read, add and forget notes,
//!   nothing else of its area (`torro_ipc::api::own_memory`).
//! - **Skills** (issue #703, [`skills`]): `<config>/assistant/skills/<8 hex>/SKILL.md`, changed under
//!   the same lock. The prompt names each in one line; the text comes with `assistant.get_skill`. A
//!   bot's client reads its bot's own skills ([`skills::read_answer`], answered by the core).
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
//! - **Cards** (issue #225, [`cards`]): after a command that made an item (a PDF of `tools.create_pdf`) the
//!   answer adds a card entry that points at it, with either engine; `assistant.get_cards` shows what it points
//!   at now, resolved by the core with the rights of the conversation's client.
//!
//! - **A chat about a mail** (issue #605, "Chat zu einer Mail" in `docs/assistent.md`): `assistant.send_message`
//!   with `about` starts a conversation that keeps the mail ([`ConversationSummary::about`]) for good; a later
//!   message without it goes on with that mail, one with another is `error` ([`Assistant::about_for`]). Only
//!   with the assistant (not a bot) and engine A, only while the assistant's own client may read that mail
//!   account (`forbidden`), with any model (issue #664); refused, nothing
//!   is stored. The answer reads the mail first and only reads and suggests ([`model`]). The caller's own reach
//!   to the mail the core checks ([`crate::core::Core::handle_from`]).
//!
//! What goes to a client never names a path; the core's log does.

pub(crate) mod cards;
mod compact;
mod delegate;
mod engines;
mod model;
pub(crate) mod profile;
mod prompt;
pub(crate) mod sentences;
pub(crate) mod skills;
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
use torro_core::ai::Effort;
use torro_core::i18n::tr;
use torro_core::paths::AppPaths;
use torro_core::settings::Retention;
use torro_ipc::api::{Access, Area};
use torro_ipc::assistant::{
    ChatWindow, ConversationHit, ConversationSummary, DEFAULT_HITS, Engine, EngineKind, Entry, EntryContent,
    HITS_PER_CONVERSATION, MAX_HITS, MAX_NOTES, MAX_SKILL_CHARS, MAX_SKILL_DESCRIPTION_CHARS, MAX_SKILL_NAME_CHARS,
    MAX_SKILLS, MailRef, Note, Profile, Skill, TEST_ENGINE, split_spoken,
};
use torro_ipc::bots::Bot;
use torro_ipc::{Request, Response};
use torro_pim::AccountState;
use torro_setups::launch::{Launcher, SystemLauncher};

use crate::account::ai::AiAccounts;
use crate::bots::Bots;
pub use delegate::Asking;
pub(crate) use model::readable;
pub(crate) use model::workflow_choice;
pub use model::{Actor, AskedIn, Delegation, Host, Limits, ModelAccess};
use store::{Conversation, Line};
pub use torro_core::ai::Place;

/// The environment variable that makes a running core offer the test engine: `1`.
pub const TEST_ENGINE_VAR: &str = "TORRO_TEST_ENGINE";

/// The program of the assistant's windows (task C.4): `torroassistant --chat <id>|new`,
/// later `--bot <id>`.
pub const BINARY: &str = "torroassistant";

/// How often the daemon removes conversations past the retention time and results past
/// [`store::RESULT_HOURS`] ([`Assistant::clean_up`]).
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

/// An answer entry with its spoken part split off (issue #774, [`split_spoken`]): the core keeps the
/// model's text as it came, mark and all, so the model reads its own answers back unchanged, and every
/// client gets the two parts apart. `running` for the growing answer. Other entries pass unchanged.
fn split_answer(mut entry: Entry, running: bool) -> Entry {
    if let EntryContent::Answer { text, spoken, .. } = &mut entry.content
        && spoken.is_none()
    {
        let (said, shown) = split_spoken(text, running);
        *spoken = said;
        *text = shown;
    }
    entry
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
            spoken: None,
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

    /// The assistant with other coding agents, for tests (the agents' tests run on Unix only).
    #[cfg(all(test, unix))]
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
    /// agents while they are offered (none in the product, task C.18, issue #219), then the test
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
                Engine { id: account.id, kind: EngineKind::Ai, name: account.label, problem, models: Vec::new() }
            })
            .collect();
        engines.extend(self.agents.engines(self.has_client()));
        if self.test_engine.is_some() {
            let name = tr("Test engine").to_string();
            engines.push(Engine {
                id: TEST_ENGINE.into(),
                kind: EngineKind::Test,
                name,
                problem: None,
                models: Vec::new(),
            });
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

    /// `assistant.search_conversations` (issue #704) within `scope`: the conversations whose title,
    /// messages or answers hold every word of `query` ([`Conversation::find`]), the most recently active
    /// first (then by id), each with at most [`HITS_PER_CONVERSATION`] hits, its newest, until `limit`
    /// hits ([`DEFAULT_HITS`] without one, kept within 1..=[`MAX_HITS`]). No index: the conversations are
    /// in memory already, and a deleted or removed one is gone from there with its file.
    pub async fn search(&self, query: &str, limit: Option<u32>, scope: &Scope<'_>) -> Response {
        let words = store::words_of(query);
        let limit = limit.unwrap_or(DEFAULT_HITS).clamp(1, MAX_HITS) as usize;
        let state = self.state().await;
        let mut conversations: Vec<&Conversation> = state
            .conversations
            .values()
            .filter(|conversation| !(scope.own_only && conversation.bot.is_some()))
            .filter(|conversation| conversation.about.as_ref().is_none_or(|about| (scope.reads_about)(about)))
            .collect();
        conversations.sort_by(|a, b| b.updated().cmp(&a.updated()).then(a.id.cmp(&b.id)));
        let mut hits = Vec::new();
        for conversation in conversations {
            let (title, entries): (Vec<_>, Vec<_>) =
                conversation.find(&words).into_iter().partition(|found| found.seq.is_none());
            let mut found: Vec<_> = entries
                .into_iter()
                .rev()
                .filter(|found| scope.readable.is_none_or(|readable| found.after.iter().all(|name| readable(name))))
                .take(HITS_PER_CONVERSATION)
                .collect();
            // The title only when nothing else is left: it repeats the first message mostly.
            if found.is_empty() {
                found = title;
            }
            for found in found {
                if hits.len() >= limit {
                    return Response::ConversationHits { hits };
                }
                hits.push(ConversationHit {
                    conversation: conversation.id.clone(),
                    title: conversation.title.clone(),
                    updated: conversation.updated(),
                    bot: conversation.bot.clone(),
                    seq: found.seq,
                    kind: found.kind,
                    snippet: found.snippet,
                    untrusted: found.untrusted,
                });
            }
        }
        Response::ConversationHits { hits }
    }

    /// `assistant.get_conversation`: the conversation and its entries after `after`, and while a
    /// model writes, its growing answer numbered after the conversation's `last` ([`Run::growing`]).
    pub async fn get(&self, id: &str, after: Option<u64>) -> Response {
        let bots = self.bots.ids().await;
        let state = self.state().await;
        let Some(conversation) = state.conversations.get(id) else { return unknown(id) };
        let run = state.running.get(id);
        let mut entries: Vec<Entry> =
            conversation.entries_after(after).into_iter().map(|entry| split_answer(entry, false)).collect();
        if let Some(growing) = run.and_then(|run| run.growing(conversation.last()))
            && growing.seq > after.unwrap_or(0)
        {
            entries.push(split_answer(growing, true));
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
    /// for a message (also `bots.ask_bot`) an engine that does not answer ([`Assistant::engine`]) and for a
    /// message about a mail what [`Assistant::about_for`] refuses (issue #605), the same for
    /// `assistant.check_about` ([`Assistant::check_about_refusal`]), a conversation that still answers
    /// (`busy`), a bot that is not there (`not-found`) or a conversation of another bot or of none
    /// ([`Assistant::bot_refusal`]), the window of a bot that is not there (`not-found`), a choice of the model
    /// that [`Assistant::choose_refusal`] refuses (issue #852). `None` when it may run.
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
        // Several at once (issue #697): an unknown one is refused before anyone is asked, as a single one is.
        if matches!(request, Request::ArchiveConversations { .. } | Request::DeleteConversations { .. }) {
            let state = self.state().await;
            if let Some(id) = request.conversation_ids().into_iter().find(|id| !state.conversations.contains_key(*id)) {
                return Some(unknown(id));
            }
        }
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
            (Request::SendMessage { .. }, _) => self.message_refusal(ai, request, bot.clone(), &actor).await,
            (Request::CheckAbout { engine, model, about }, _) => {
                self.check_about_refusal(ai, engine, model.clone(), about.clone()).await
            }
            (Request::AskBot { .. }, Some(bot)) => self.engine(ai, &bot.engine, &actor).await.err(),
            (Request::ChooseModel { id, engine, .. }, _) => self.choose_refusal(ai, id, engine).await,
            (Request::OpenChatWindow { window: ChatWindow::Bot(id) }, _) if self.bots.get(id).await.is_none() => {
                Some(crate::bots::no_bot(id))
            }
            _ => None,
        }
    }

    /// Why the `assistant.send_message` `request` would be refused anyway, checked before anyone is asked to approve
    /// it: an engine that does not answer for `actor` ([`Assistant::engine`]), and for a chat about a mail (issue
    /// #605) what [`Assistant::about_for`] refuses (the assistant's read access to the account, a bot or an agent, mail switched off, another mail than the conversation's). `bot`: the bot it names, as stored.
    async fn message_refusal(
        &self,
        ai: &AiAccounts,
        request: &Request,
        bot: Option<Bot>,
        actor: &Actor,
    ) -> Option<Response> {
        let Request::SendMessage { conversation, engine, model, effort, text, without, mode, about, .. } = request
        else {
            return None;
        };
        let message = Message {
            conversation: conversation.clone(),
            engine: engine.clone(),
            model: model.clone(),
            effort: *effort,
            text: text.clone(),
            without: without.clone(),
            mode: *mode,
            bot,
            on_behalf: None,
            from: None,
            spoken: false,
        };
        self.engine_and_about_refusal(ai, &message, about.clone(), actor).await
    }

    /// Why `assistant.check_about` (issue #605) is refused: what [`Assistant::message_refusal`] would refuse for a
    /// new conversation of the assistant about `about` with `engine` and `model` – no bot, nothing switched off,
    /// the default mode. Nothing is stored and no model is asked. `None` when such a chat would be taken.
    async fn check_about_refusal(
        &self,
        ai: &AiAccounts,
        engine: &str,
        model: Option<String>,
        about: MailRef,
    ) -> Option<Response> {
        let message = Message {
            conversation: None,
            engine: engine.to_string(),
            model,
            effort: None,
            text: String::new(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            on_behalf: None,
            from: None,
            spoken: false,
        };
        self.engine_and_about_refusal(ai, &message, Some(about), &Actor::Assistant).await
    }

    /// An engine of `message` that does not answer for `actor` ([`Assistant::engine`]), else what
    /// [`Assistant::about_for`] refuses for `about`; `None` when neither refuses.
    async fn engine_and_about_refusal(
        &self,
        ai: &AiAccounts,
        message: &Message,
        about: Option<MailRef>,
        actor: &Actor,
    ) -> Option<Response> {
        let kind = match self.engine(ai, &message.engine, actor).await {
            Ok(kind) => kind,
            Err(refused) => return Some(refused),
        };
        self.about_for(message, about, kind).await.err()
    }

    /// What drives `engine` for `actor`, or why it cannot answer: `not-found` for no engine of
    /// `assistant.list_engines` (an agent, which is not offered any more — task C.18, issue #219 —
    /// so a bot still set to one is refused, like a bot whose AI account was removed),
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

    /// The mail of conversation `id` (issue #605); `None` for one about none and one the core does not have.
    pub async fn about(&self, id: &str) -> Option<MailRef> {
        self.state().await.conversations.get(id).and_then(|conversation| conversation.about.clone())
    }

    /// The mail `message` is about (issue #605), or why it may not be sent; checked before anything is stored.
    /// `about` as the request names it: for a new conversation the mail it is about, for one that goes on the
    /// same mail it keeps (`error` for another, or for a mail where it is about none); left out, it goes on with
    /// the conversation's own. `Ok(None)` for a message about no mail. A message about a mail is refused
    ///
    /// - `unsupported` with a bot or a coding agent (engine B, `kind`): only the assistant's model reads it;
    /// - `error` with mail switched off for the conversation (`without`);
    /// - `forbidden` when the assistant's own client may not read mail or not that account
    ///   ([`torro_ipc::api::Rights::reaches_mail_account`]), also when there is no such client or core yet.
    async fn about_for(
        &self,
        message: &Message,
        about: Option<MailRef>,
        kind: EngineKind,
    ) -> Result<Option<MailRef>, Response> {
        let kept = match &message.conversation {
            Some(id) => match self.state().await.conversations.get(id) {
                Some(conversation) => Some(conversation.about.clone()),
                None => return Err(unknown(id)),
            },
            None => None,
        };
        let about = match (about, kept) {
            (Some(asked), Some(kept)) if kept.as_ref() != Some(&asked) => {
                return Err(failure("this conversation is about another mail or none; start a new one for this mail"));
            }
            (asked, kept) => asked.or(kept.flatten()),
        };
        let Some(about) = about else { return Ok(None) };
        if message.bot.is_some() {
            return Err(refusal("unsupported", "a chat about a mail is with the assistant only, not with a bot"));
        }
        if kind == EngineKind::Agent {
            return Err(refusal("unsupported", "a chat about a mail needs an AI account; agents are not given mail"));
        }
        if message.without.contains(&Area::Mail) {
            return Err(failure("a chat about a mail reads mail; do not switch mail off for it"));
        }
        let host = self.host();
        let rights = host.as_ref().and_then(|host| host.client_of(&Actor::Assistant)).map(|client| client.rights);
        let reads = rights.is_some_and(|rights| {
            rights.area(Area::Mail).allows(Access::Read) && rights.reaches_mail_account(&about.account)
        });
        if !reads {
            return Err(refusal(
                "forbidden",
                "the assistant may not read this mail account; a person can allow it in the assistant's access",
            ));
        }
        Ok(Some(about))
    }

    /// `assistant.send_message` once the engine is known to answer (`kind`): adds the message to
    /// `conversation` or to a new one and starts the answer. Answers with the conversation and the
    /// entries this call added. A conversation about a mail goes on with it ([`Assistant::send_about`]).
    async fn send(self: &Arc<Self>, message: Message, kind: EngineKind, now: DateTime<Utc>) -> Response {
        self.send_about(message, None, kind, now).await
    }

    /// [`Assistant::send`] about the mail `about` (issue #605, checked by [`Assistant::about_for`]): a new
    /// conversation keeps it for good; one that goes on keeps its own, and `error` when `about` names another.
    async fn send_about(
        self: &Arc<Self>,
        message: Message,
        about: Option<MailRef>,
        kind: EngineKind,
        now: DateTime<Utc>,
    ) -> Response {
        let mut state = self.state().await;
        let (mut conversation, new) = match &message.conversation {
            Some(id) if state.running.contains_key(id) => return busy(),
            Some(id) => match state.conversations.get(id) {
                Some(conversation) => match bot_refusal(conversation, message.bot.as_ref()) {
                    Some(refused) => return refused,
                    // Checked before, again under the lock: the mail a conversation is about never changes.
                    None if about.is_some() && about != conversation.about => {
                        return failure("this conversation is about another mail or none; start a new one for it");
                    }
                    None => (conversation.clone(), false),
                },
                None => return unknown(id),
            },
            None => match store::new_id() {
                Ok(id) => {
                    let mut conversation = Conversation::new(id, &message.text, now);
                    conversation.bot = message.bot.as_ref().map(|bot| bot.id.clone());
                    conversation.about = about;
                    (conversation, true)
                }
                Err(err) => return failure(&err),
            },
        };
        let about = conversation.about.clone();
        let before = conversation.last();
        let entry = conversation.next(
            EntryContent::Message {
                text: message.text,
                engine: message.engine.clone(),
                model: message.model.clone(),
                effort: message.effort,
                without: message.without.clone(),
                mode: message.mode,
                from: message.from.clone(),
            },
            now,
        );
        let mut lines = if new { vec![conversation.started()] } else { Vec::new() };
        // A message brings an archived conversation back (issue #697): it is in use again.
        if conversation.archived {
            lines.push(Line::Archived { archived: false });
            conversation.apply(Line::Archived { archived: false });
        }
        lines.push(Line::Entry { entry: entry.clone() });
        if let Err(message) = self.write(&conversation.id, lines).await {
            return failure(message);
        }
        let said = entry.seq;
        conversation.apply(Line::Entry { entry });
        let id = conversation.id.clone();
        if message.from.is_none() {
            state.said.insert(id.clone(), said);
        }
        let (history, results, untrusted) =
            (conversation.entries.clone(), conversation.results.clone(), conversation.untrusted);
        state.conversations.insert(id.clone(), conversation);
        let (engine, model, effort, without, mode, spoken) =
            (message.engine, message.model, message.effort, message.without, message.mode, message.spoken);
        let actor = match message.on_behalf {
            Some(delegation) => Actor::Delegated(delegation),
            None => Actor::of(message.bot.as_ref()),
        };
        let job = |engine: String| model::Job {
            conversation: id.clone(),
            run: 0,
            engine,
            model,
            effort,
            without,
            mode,
            history,
            results,
            untrusted,
            partial: Arc::default(),
            actor,
            bot: message.bot,
            about,
            spoken,
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
                lack: None,
            };
            if !assistant.add_to_run(&conversation, run, tool, false, None).await {
                return;
            }
            tokio::time::sleep(step).await;
            let text = tr(
                "This is the test engine. It answers every message the same way, without a model and without tools.",
            );
            let answer = EntryContent::Answer { text: text.to_string(), engine: by, model: None, spoken: None };
            assistant.add_to_run(&conversation, run, answer, true, None).await;
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
    /// nothing more). `result` is what the model read of a tool line (issue #701): written with the entry, so a
    /// later turn sends it again ([`store::Line::Result`]).
    async fn add_to_run(&self, id: &str, run: u64, content: EntryContent, last: bool, result: Option<String>) -> bool {
        let mut state = self.state().await;
        if state.running.get(id).is_none_or(|running| running.id != run) {
            return false;
        }
        let Some(entry) = state.conversations.get(id).map(|conversation| conversation.next(content, Utc::now())) else {
            state.running.remove(id);
            return false;
        };
        let mut lines = vec![Line::Entry { entry: entry.clone() }];
        if let Some(text) = result {
            lines.push(Line::Result { seq: entry.seq, text });
        }
        if self.write(id, lines.clone()).await.is_err() {
            state.running.remove(id);
            return false;
        }
        if matches!(entry.content, EntryContent::Answer { .. })
            && let Some(running) = state.running.get(id)
        {
            running.clear();
        }
        if let Some(conversation) = state.conversations.get_mut(id) {
            lines.into_iter().for_each(|line| conversation.apply(line));
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

    /// `assistant.choose_model` (issue #852): who answers the next message of conversation `id`. `not-found` for
    /// an unknown conversation, `error` for one with a bot (it keeps the bot's model), and an engine that does
    /// not answer the assistant as [`Assistant::engine`] refuses it.
    pub async fn choose(
        &self,
        ai: &AiAccounts,
        id: &str,
        engine: String,
        model: Option<String>,
        effort: Option<Effort>,
    ) -> Response {
        if let Some(refused) = self.choose_refusal(ai, id, &engine).await {
            return refused;
        }
        self.change(id, Line::Chosen { engine, model, effort }).await
    }

    /// Why `assistant.choose_model` for conversation `id` and `engine` would be refused (issue #852): `not-found`
    /// for an unknown conversation, `error` for one with a bot, else what [`Assistant::engine`] says of `engine`
    /// for the assistant, and `unsupported` for a coding agent in a chat about a mail, as its next message would
    /// be ([`Assistant::about_for`]). `None` when the choice may be stored.
    async fn choose_refusal(&self, ai: &AiAccounts, id: &str, engine: &str) -> Option<Response> {
        let about = match self.state().await.conversations.get(id) {
            None => return Some(unknown(id)),
            Some(conversation) if conversation.bot.is_some() => {
                return Some(refusal("error", "a conversation with a bot keeps the bot's model"));
            }
            Some(conversation) => conversation.about.is_some(),
        };
        match self.engine(ai, engine, &Actor::of(None)).await {
            Err(refused) => Some(refused),
            Ok(EngineKind::Agent) if about => {
                Some(refusal("unsupported", "a chat about a mail needs an AI account; agents are not given mail"))
            }
            Ok(_) => None,
        }
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

    /// `assistant.archive_conversations` (issue #697): every id is checked first, so one unknown changes
    /// nothing. Doubles and conversations already in that state are skipped; an answer that runs goes on.
    /// A file that cannot be written stops there: the ones before keep their new state.
    pub async fn archive(&self, ids: &[String], archived: bool) -> Response {
        let mut state = self.state().await;
        if let Some(id) = ids.iter().find(|id| !state.conversations.contains_key(id.as_str())) {
            return unknown(id);
        }
        let mut seen = BTreeSet::new();
        for id in ids {
            if !seen.insert(id.as_str()) || state.conversations.get(id).is_some_and(|c| c.archived == archived) {
                continue;
            }
            let line = Line::Archived { archived };
            if let Err(message) = self.write(id, vec![line.clone()]).await {
                return failure(message);
            }
            if let Some(conversation) = state.conversations.get_mut(id) {
                conversation.apply(line);
            }
        }
        Response::Ok
    }

    /// `assistant.delete_conversations` (issue #697): every id is checked first, so one unknown deletes
    /// nothing; then each goes as with [`Assistant::delete`], its answer stopped. A file that cannot be
    /// removed keeps its conversation, the others stay deleted, and the answer is `error` with how many are
    /// left.
    pub async fn delete_many(&self, ids: &[String]) -> Response {
        if let Some(id) = {
            let state = self.state().await;
            ids.iter().find(|id| !state.conversations.contains_key(id.as_str())).cloned()
        } {
            return unknown(&id);
        }
        let mut seen = BTreeSet::new();
        let mut left = 0;
        for id in ids {
            if !seen.insert(id.as_str()) {
                continue;
            }
            match self.delete(id).await {
                Response::Ok => {}
                // Deleted meanwhile by another client: gone all the same.
                Response::Error { code, .. } if code == "not-found" => {}
                _ => left += 1,
            }
        }
        match left {
            0 => Response::Ok,
            n => {
                Response::Error { code: "error".into(), message: format!("{n} of the conversations cannot be deleted") }
            }
        }
    }

    /// Removes every conversation whose last entry is older than `retention` at `now`, except the
    /// pinned and archived ones (issue #697) and those answering (decision 8). Answers how many went; one whose file cannot
    /// be removed stays and is named on stderr.
    ///
    /// Then drops the tool results older than [`store::RESULT_HOURS`] from every conversation not
    /// answering (issue #714), pinned ones and [`Retention::Forever`] too; an answering one keeps
    /// them until a later round, as its run has copied them already. The daemon calls this at its
    /// start and every [`CLEAN_UP_EVERY`]. A file that cannot be written anew keeps its results
    /// and is named on stderr; the next round tries again.
    pub async fn clean_up(&self, now: DateTime<Utc>, retention: Retention) -> usize {
        let Some(dir) = self.dir.clone() else { return 0 };
        let mut state = self.state().await;
        let mut removed = 0;
        if let Some(days) = retention.days() {
            let oldest = now - chrono::Duration::days(i64::from(days));
            let expired: Vec<String> = state
                .conversations
                .values()
                .filter(|c| !c.pinned && !c.archived && c.updated() < oldest && !state.running.contains_key(&c.id))
                .map(|c| c.id.clone())
                .collect();
            for id in expired {
                match self.remove(&id).await {
                    Ok(()) => {
                        state.conversations.remove(&id);
                        removed += 1;
                    }
                    Err(err) => eprintln!("torroworkplace: cannot remove the old conversation {id}: {err}"),
                }
            }
        }
        let oldest = now - chrono::Duration::hours(store::RESULT_HOURS);
        let expired: Vec<(String, BTreeSet<u64>)> = state
            .conversations
            .values()
            .filter(|c| !state.running.contains_key(&c.id))
            .map(|c| (c.id.clone(), c.expired_results(oldest)))
            .filter(|(_, seqs)| !seqs.is_empty())
            .collect();
        for (id, seqs) in expired {
            let (dir, file, dropping) = (dir.clone(), id.clone(), seqs.clone());
            match blocking(move || store::drop_results(&dir, &file, &dropping)).await {
                Ok(()) => {
                    if let Some(conversation) = state.conversations.get_mut(&id) {
                        // The file keeps a note for each marked result it dropped (issue #724).
                        conversation.untrusted |= conversation
                            .results
                            .iter()
                            .any(|(seq, text)| seqs.contains(seq) && store::marked_untrusted(text));
                        conversation.results.retain(|seq, _| !seqs.contains(seq));
                    }
                }
                Err(err) => eprintln!("torroworkplace: cannot drop the old results of the conversation {id}: {err}"),
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

    /// The assistant's skills by name; none when they cannot be read (the core's log says why).
    pub async fn skills(&self) -> Vec<skills::Stored> {
        let Some(dir) = self.config.as_deref().map(skills::folder) else { return Vec::new() };
        blocking(move || skills::load_all(&dir)).await.unwrap_or_else(|err| {
            eprintln!("torroworkplace: cannot read the assistant's skills: {err}");
            Vec::new()
        })
    }

    /// The assistant's skill `name` (without regard to case), as stored.
    pub async fn skill(&self, name: &str) -> Option<Skill> {
        self.skills().await.into_iter().map(|stored| stored.skill).find(|skill| skill.is_named(name))
    }

    /// What `assistant.get_skill`, `save_skill` or `delete_skill` would be refused with anyway,
    /// checked before anyone is asked to approve it and again by the handlers: a skill that is not
    /// there (`not-found`), a name that belongs to another skill (`conflict`), a new skill beyond
    /// [`MAX_SKILLS`] (`error`). `None` for every other request.
    pub async fn skill_refusal(&self, request: &Request) -> Option<Response> {
        let (name, replaces) = match request {
            Request::GetSkill { name } | Request::DeleteSkill { name } => {
                return self.skill(name).await.is_none().then(|| no_skill(name));
            }
            Request::SaveSkill { name, replaces, .. } => (name, replaces.as_deref()),
            _ => return None,
        };
        let skills = self.skills().await;
        let named = |wanted: &str| skills.iter().any(|stored| stored.skill.is_named(wanted));
        match replaces {
            Some(old) if !named(old) => Some(no_skill(old)),
            Some(old) if !torro_ipc::assistant::same_skill_name(old, name) && named(name) => Some(taken(name)),
            None if named(name) => Some(taken(name)),
            None if skills.len() >= MAX_SKILLS => Some(skills_full()),
            _ => None,
        }
    }

    /// `assistant.save_skill` once its limits were checked: a new skill, or the skill `replaces` in
    /// its folder with the new name; names and description on one line, the text without control
    /// characters. Answers with the skill as stored.
    pub async fn save_skill(&self, skill: Skill, replaces: Option<&str>) -> Response {
        let Some(dir) = self.config.as_deref().map(skills::folder) else { return failure(NO_PLACE) };
        let _own = self.own.lock().await;
        let request = Request::SaveSkill {
            name: skill.name.clone(),
            description: String::new(),
            text: String::new(),
            replaces: replaces.map(str::to_string),
        };
        if let Some(refused) = self.skill_refusal(&request).await {
            return refused;
        }
        let stored = self.skills().await;
        let id = match replaces.and_then(|old| stored.iter().find(|kept| kept.skill.is_named(old))) {
            Some(old) => old.id.clone(),
            None => {
                let taken = stored.iter().map(|kept| kept.id.clone()).collect::<Vec<_>>();
                match crate::bots::store::new_id(&taken) {
                    Ok(id) => id,
                    Err(err) => return failure(&err),
                }
            }
        };
        let skill = Skill {
            name: profile::clean_line(&skill.name, MAX_SKILL_NAME_CHARS),
            description: profile::clean_line(&skill.description, MAX_SKILL_DESCRIPTION_CHARS),
            text: profile::clean_text(&skill.text, MAX_SKILL_CHARS),
        };
        let saved = skills::Stored { id, skill: skill.clone() };
        match blocking(move || skills::save(&dir, &saved)).await {
            Ok(()) => Response::Skill { skill },
            Err(err) => {
                eprintln!("torroworkplace: cannot store the assistant's skill: {err}");
                failure("the skill cannot be stored")
            }
        }
    }

    /// `assistant.delete_skill`: removes the skill `name` for good.
    pub async fn delete_skill(&self, name: &str) -> Response {
        let Some(dir) = self.config.as_deref().map(skills::folder) else { return failure(NO_PLACE) };
        let _own = self.own.lock().await;
        let Some(stored) = self.skills().await.into_iter().find(|stored| stored.skill.is_named(name)) else {
            return no_skill(name);
        };
        match blocking(move || skills::remove(&dir, &stored.id)).await {
            Ok(()) => Response::Ok,
            Err(err) => {
                eprintln!("torroworkplace: cannot remove the assistant's skill: {err}");
                failure("the skill cannot be deleted")
            }
        }
    }

    /// What an answer reads of the profile and the memory: the personality and every note, those
    /// from mail too ([`Note::local_only`], issue #664: every model reads them).
    async fn persona_and_notes(&self) -> (String, Vec<Note>) {
        (self.profile().await.personality, self.notes().await)
    }

    /// The assistant's skills by name, as an answer's prompt names them (issue #703).
    async fn skill_summaries(&self) -> Vec<torro_ipc::assistant::SkillSummary> {
        self.skills().await.iter().map(|stored| stored.skill.summary()).collect()
    }
}

/// Answers the requests of the assistant's profile and memory (task C.10): `get_profile`,
/// `update_profile`, `list_memories`, `remember` and `forget`, and of its skills (issue #703:
/// `list_skills`, `get_skill`, `save_skill`, `delete_skill`; a bot's client gets its bot's from the
/// core instead), for the app key and for a client the
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
    if let Some(refused) = assistant.skill_refusal(&request).await {
        return refused;
    }
    match request {
        Request::GetProfile => Response::Profile { profile: assistant.profile().await },
        Request::UpdateProfile { name, description, avatar, personality, person_name } => {
            assistant.update_profile(profile::Change { name, description, avatar, personality, person_name }).await
        }
        Request::ListMemories => assistant.list_memories(sees_local).await,
        Request::Remember { text, replaces, local_only } => {
            assistant.remember(&text, replaces.as_deref(), local_only, sees_local).await
        }
        Request::Forget { id } => assistant.forget(&id, sees_local).await,
        request @ (Request::ListSkills | Request::GetSkill { .. }) => {
            let skills: Vec<Skill> = assistant.skills().await.into_iter().map(|stored| stored.skill).collect();
            skills::read_answer(&skills, &request).unwrap_or_else(|| failure("no skill to read"))
        }
        Request::SaveSkill { name, description, text, replaces } => {
            assistant.save_skill(Skill { name, description, text }, replaces.as_deref()).await
        }
        Request::DeleteSkill { name } => assistant.delete_skill(&name).await,
        other => failure(&format!("{} is no command of the assistant's profile", other.operation().name())),
    }
}

/// Why a profile or a note cannot be changed without a home directory.
const NO_PLACE: &str = "there is no place to keep the assistant's profile and memory (no home directory)";

/// The `not-found` answer for note `id`.
fn no_note(id: &str) -> Response {
    Response::Error { code: "not-found".into(), message: format!("no note {id:?}") }
}

/// The `not-found` answer for skill `name`.
fn no_skill(name: &str) -> Response {
    Response::Error { code: "not-found".into(), message: format!("no skill {:?}", name.trim()) }
}

/// The `conflict` answer for a skill whose name another skill has.
fn taken(name: &str) -> Response {
    Response::Error { code: "conflict".into(), message: format!("another skill is called {:?}", name.trim()) }
}

/// The `error` answer for a new skill beyond [`MAX_SKILLS`].
fn skills_full() -> Response {
    failure(&format!("the assistant has {MAX_SKILLS} skills, the most it may have; delete some first"))
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
    /// How hard the model thinks (issue #131); `None`: as it decides.
    effort: Option<Effort>,
    text: String,
    without: Vec<Area>,
    /// The permission mode of the answer (issue #132); [`torro_ipc::assistant::Mode::Default`] for a
    /// bot asked by the assistant, which runs in the assistant's mode through its delegation.
    mode: torro_ipc::assistant::Mode,
    /// The bot that answers, as stored now (task B.1); `None` for the assistant.
    bot: Option<Bot>,
    /// The bot runs for another client (task B.2: the assistant; issue #698: a client from outside): as
    /// [`Actor::Delegated`], with what both clients may. `None`: as its own client (or the assistant as its own).
    on_behalf: Option<Delegation>,
    /// The client that sent it ([`EntryContent::Message`]'s `from`, issue #305); `None`: the person
    /// wrote it (task B.2), sent with the app key. Only the person's message can name a bot for the
    /// assistant to ask ([`State::said`]), and only it is their word in any later answer.
    from: Option<String>,
    /// The person spoke it (issue #747): the answer is short, to be read aloud.
    spoken: bool,
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

/// What a search of the conversations may answer (issue #704, [`Assistant::search`]), set by the core
/// for the caller.
pub struct Scope<'a> {
    /// Only the assistant's own conversations, none with a bot: for the assistant's own client.
    pub own_only: bool,
    /// Whether the caller may still read what a command (its catalogue name, as a tool line keeps it) returned
    /// ([`readable`], issue #713); a hit after a command it may not is left out. `None`: every hit, for callers
    /// that may read the whole conversation anyway.
    pub readable: Option<&'a (dyn Fn(&str) -> bool + Sync)>,
    /// Whether the caller may read the mail a conversation is about (issue #605): one about another is not
    /// there for it.
    pub reads_about: &'a (dyn Fn(&MailRef) -> bool + Sync),
}

impl Scope<'static> {
    /// Every conversation and hit, as for the app key.
    pub fn all() -> Scope<'static> {
        Scope { own_only: false, readable: None, reads_about: &|_| true }
    }
}

/// [`answer_from`] as for a client, for tests: a message counts as no person's.
#[cfg(test)]
pub async fn answer(assistant: &Arc<Assistant>, ai: &AiAccounts, request: Request) -> Response {
    answer_from(assistant, ai, request, Some(TEST_CLIENT.into())).await
}

/// The client [`answer`] sends as, in tests.
#[cfg(test)]
pub const TEST_CLIENT: &str = "7e57c11e";

/// Answers an `assistant.*` request and `bots.ask_bot` (task B.1), for the app key and for a client
/// the core admitted ([`crate::core::Core::handle_from`]). `from`: the client that sent it, `None`
/// for the app key (task B.2): then a message is the person's own, and only such a message can name
/// a bot for the assistant to ask; a client's message keeps its id (issue #305), and a bot it asks or writes
/// to runs with what both may (issue #698, [`for_caller`]). What would be
/// refused anyway comes first ([`Assistant::refusal`]); the handlers check again
/// under their lock (a conversation may have been deleted or started answering meanwhile).
pub async fn answer_from(
    assistant: &Arc<Assistant>,
    ai: &AiAccounts,
    request: Request,
    from: Option<String>,
) -> Response {
    if let Some(refused) = assistant.refusal(ai, &request).await {
        return refused;
    }
    match request {
        Request::ListEngines => Response::Engines { engines: assistant.engines(ai).await },
        Request::ListConversations => assistant.list().await,
        // The core answers it with the caller's scope (issue #704); this is the app key's.
        Request::SearchConversations { query, limit } => assistant.search(&query, limit, &Scope::all()).await,
        Request::GetConversation { id, after } => assistant.get(&id, after).await,
        Request::SendMessage { conversation, engine, model, effort, text, without, mode, bot, about, spoken } => {
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
            // A client's chat with a bot (issue #698): the bot runs with what both may.
            let on_behalf = bot.as_ref().and_then(|bot| for_caller(bot, from.as_deref(), &without, mode));
            let actor = on_behalf.clone().map_or_else(|| Actor::of(bot.as_ref()), Actor::Delegated);
            let kind = match assistant.engine(ai, &engine, &actor).await {
                Ok(kind) => kind,
                Err(refused) => return refused,
            };
            let message =
                Message { conversation, engine, model, effort, text, without, mode, bot, on_behalf, from, spoken };
            // A chat about a mail (issue #605): refused before anything is stored.
            let about = match assistant.about_for(&message, about, kind).await {
                Ok(about) => about,
                Err(refused) => return refused,
            };
            assistant.send_about(message, about, kind, Utc::now()).await
        }
        // Checked by [`Assistant::refusal`] above (issue #605): nothing more to do.
        Request::CheckAbout { .. } => Response::Ok,
        Request::AskBot { bot, text, conversation } => {
            let Some(bot) = assistant.bots.get(&bot).await else { return crate::bots::no_bot(&bot) };
            let mode = torro_ipc::assistant::Mode::Default;
            // A client asks the bot (issue #698): it runs with what both may, never with more than the caller.
            let on_behalf = for_caller(&bot, from.as_deref(), &[], mode);
            let actor = on_behalf.clone().map_or_else(|| Actor::of(Some(&bot)), Actor::Delegated);
            let kind = match assistant.engine(ai, &bot.engine, &actor).await {
                Ok(kind) => kind,
                Err(refused) => return refused,
            };
            let (engine, model) = (bot.engine.clone(), bot.model.clone());
            let message = Message {
                conversation,
                engine,
                model,
                effort: None,
                text,
                without: Vec::new(),
                mode,
                bot: Some(bot),
                on_behalf,
                from,
                spoken: false,
            };
            assistant.send(message, kind, Utc::now()).await
        }
        Request::CancelAnswer { conversation } => assistant.cancel(&conversation, Utc::now()).await,
        Request::RenameConversation { id, title } => assistant.rename(&id, &title).await,
        Request::ChooseModel { id, engine, model, effort } => assistant.choose(ai, &id, engine, model, effort).await,
        Request::DeleteConversation { id } => assistant.delete(&id).await,
        Request::PinConversation { id, pinned } => assistant.pin(&id, pinned).await,
        Request::ArchiveConversations { ids, archived } => assistant.archive(&ids, archived).await,
        Request::DeleteConversations { ids } => assistant.delete_many(&ids).await,
        Request::OpenChatWindow { window } => assistant.open_window(window).await,
        Request::ConfirmBot { conversation, bot } => assistant.confirm_bot(&conversation, &bot).await,
        other => failure(&format!("{} is no command of the assistant", other.operation().name())),
    }
}

/// The delegation of `bot`'s run for the client `from` (issue #698): a client from outside that asks the bot
/// or writes to it reaches through it only what both clients may ([`Actor::Delegated`]), without `without`
/// and in `mode`; the bot's own conversation counts for the web's yes. `None` for the app key (`from` is
/// `None`): the person's own chat with the bot runs as the bot's client.
fn for_caller(bot: &Bot, from: Option<&str>, without: &[Area], mode: torro_ipc::assistant::Mode) -> Option<Delegation> {
    Some(Delegation {
        bot: bot.id.clone(),
        client: bot.client.clone(),
        caller: from?.to_string(),
        without: without.to_vec(),
        mode,
        conversation: None,
    })
}

/// `torroassistant` with `arguments[1..]`, started as `apps.open_app` starts apps (issue #279,
/// [`crate::apps::args_start`]): on macOS through its bundle, `open -n -a <bundle> --args …`, else the
/// program next to the core's binary or on `PATH`. Not waited for, as the core's own windows
/// (`core::workplace_start`). A missing app fails here, as any app that is not installed.
fn start_app(arguments: &[String]) -> Result<(), String> {
    let args = arguments.get(1..).unwrap_or_default();
    let bundle = crate::apps::installed_bundle(AppId::ASSISTANT);
    let start = crate::apps::args_start(AppId::ASSISTANT, args, &crate::apps::search_dirs(), bundle.as_deref());
    let (crate::apps::Start::Program(command) | crate::apps::Start::Open(command)) = start;
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

    /// A running answer in conversation `id` whose text a test writes itself (issue #747): its run's
    /// number and the text as it grows. [`finish_growing`] stores the answer and ends the run.
    pub(crate) async fn growing(assistant: &Assistant, id: &str) -> (u64, Arc<StdMutex<Partial>>) {
        let mut state = assistant.state().await;
        state.next_run += 1;
        let run = state.next_run;
        let task = tokio::spawn(std::future::pending::<()>());
        let partial: Arc<StdMutex<Partial>> = Arc::default();
        let engine = TEST_ENGINE.to_string();
        state
            .running
            .insert(id.into(), Run { id: run, task: task.abort_handle(), engine, partial: Arc::clone(&partial) });
        (run, partial)
    }

    /// Stores `text` as the answer of the run `run` of [`growing`] and ends it.
    pub(crate) async fn finish_growing(assistant: &Assistant, id: &str, run: u64, text: &str) {
        let answer = EntryContent::Answer { text: text.into(), engine: TEST_ENGINE.into(), model: None, spoken: None };
        assert!(assistant.add_to_run(id, run, answer, true, None).await, "the run still runs");
    }

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
            effort: None,
            text: text.into(),
            without: vec![Area::Mail],
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            about: None,
            spoken: false,
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
                EntryContent::Card { .. } => "card",
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
            Request::ChooseModel { id: unknown.into(), engine: TEST_ENGINE.into(), model: None, effort: None },
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
            effort: None,
            text: "Hi".into(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            about: None,
            spoken: false,
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
                Engine {
                    id: account.id.clone(),
                    kind: EngineKind::Ai,
                    name: account.label.clone(),
                    problem: None,
                    models: Vec::new()
                },
                Engine {
                    id: TEST_ENGINE.into(),
                    kind: EngineKind::Test,
                    name: tr("Test engine").into(),
                    problem: None,
                    models: Vec::new()
                },
            ]
        );
        // Without a core to run tools with (none attached here) an AI account does not answer, and
        // nothing is stored.
        let to_ollama = Request::SendMessage {
            conversation: None,
            engine: account.id,
            model: Some("qwen3:8b".into()),
            effort: None,
            text: "Hi".into(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            about: None,
            spoken: false,
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

    /// Issue #852: a choice of the model is the conversation's until its next message, also for a core that
    /// reads the file again; an engine that does not answer and a bot's conversation are refused before anyone
    /// is asked to approve it.
    #[tokio::test]
    async fn a_chosen_model_answers_next() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::ZERO)), no_ai(dir.path()));
        let (started, _) = opened(answer(&assistant, &ai, send(None, "First")).await);
        finished(&assistant, &ai, &started.id).await;

        let choose = |engine: &str| Request::ChooseModel {
            id: started.id.clone(),
            engine: engine.into(),
            model: Some("small".into()),
            effort: Some(Effort::Low),
        };
        assert_eq!(answer(&assistant, &ai, choose(TEST_ENGINE)).await, Response::Ok);
        let summary = |response: Response| match response {
            Response::Conversations { conversations } => conversations.into_iter().next().expect("one"),
            other => panic!("expected the list, got {other:?}"),
        };
        let listed = summary(answer(&assistant, &ai, Request::ListConversations).await);
        assert_eq!((listed.model.as_deref(), listed.effort), (Some("small"), Some(Effort::Low)));
        let again = Arc::new(assistant_in(dir.path(), Duration::ZERO));
        assert_eq!(summary(answer(&again, &ai, Request::ListConversations).await), listed, "read again");

        let unknown = choose("ai:ollama@localhost:11434");
        assert_eq!(code(&answer(&assistant, &ai, unknown.clone()).await), "not-found", "no such engine");
        assert_eq!(assistant.refusal(&ai, &unknown).await.as_ref().map(code), Some("not-found"));

        if let Some(conversation) = assistant.state().await.conversations.get_mut(&started.id) {
            conversation.bot = Some("1a2b3c4d".into());
        }
        assert_eq!(code(&answer(&assistant, &ai, choose(TEST_ENGINE)).await), "error", "a bot keeps its model");
        assert_eq!(assistant.refusal(&ai, &choose(TEST_ENGINE)).await.as_ref().map(code), Some("error"));
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

    /// Issue #697: conversations are archived and restored several at once; the state stays on disk, an answer that
    /// runs goes on, one unknown id changes nothing, and a message brings an archived one back.
    #[tokio::test]
    async fn conversations_are_archived_restored_and_brought_back_by_a_message() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::from_secs(60))), no_ai(dir.path()));
        let (first, _) = opened(answer(&assistant, &ai, send(None, "First")).await);
        let (second, _) = opened(answer(&assistant, &ai, send(None, "Second")).await);
        assert_eq!(
            answer(&assistant, &ai, Request::CancelAnswer { conversation: second.id.clone() }).await,
            Response::Ok
        );
        let archived = |assistant: Arc<Assistant>| async move {
            match assistant.list().await {
                Response::Conversations { conversations } => {
                    conversations.into_iter().map(|c| (c.id, c.archived, c.running)).collect::<Vec<_>>()
                }
                other => panic!("expected conversations, got {other:?}"),
            }
        };

        let unknown =
            Request::ArchiveConversations { ids: vec![first.id.clone(), "ffffffffffffffff".into()], archived: true };
        assert_eq!(code(&answer(&assistant, &ai, unknown).await), "not-found");
        assert!(archived(assistant.clone()).await.iter().all(|(_, archived, _)| !archived), "nothing changed");

        let both = vec![first.id.clone(), second.id.clone(), first.id.clone()];
        let archive = Request::ArchiveConversations { ids: both.clone(), archived: true };
        assert_eq!(answer(&assistant, &ai, archive.clone()).await, Response::Ok);
        assert_eq!(answer(&assistant, &ai, archive).await, Response::Ok, "already archived: skipped");
        let list = archived(assistant.clone()).await;
        assert!(list.iter().all(|(_, archived, _)| *archived), "{list:?}");
        assert!(list.iter().any(|(id, _, running)| *id == first.id && *running), "its answer goes on");

        // Read anew from the files.
        let again = Arc::new(assistant_in(dir.path(), Duration::from_secs(60)));
        assert!(archived(again.clone()).await.iter().all(|(_, archived, _)| *archived), "kept on disk");

        let restore = Request::ArchiveConversations { ids: vec![first.id.clone()], archived: false };
        assert_eq!(answer(&again, &ai, restore).await, Response::Ok);
        let list = archived(again.clone()).await;
        assert!(list.iter().any(|(id, archived, _)| *id == first.id && !archived));
        assert!(list.iter().any(|(id, archived, _)| *id == second.id && *archived));

        let (back, _) = opened(answer(&again, &ai, send(Some(&second.id), "Once more")).await);
        assert!(!back.archived, "a message brings it back");
        let reread = Arc::new(assistant_in(dir.path(), Duration::from_secs(60)));
        assert!(archived(reread).await.iter().all(|(_, archived, _)| !archived), "also on disk");
    }

    /// Issue #697: several conversations go at once, an answer that runs stops, and one unknown id deletes nothing.
    #[tokio::test]
    async fn several_conversations_are_deleted_at_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::from_secs(60))), no_ai(dir.path()));
        let mut ids = Vec::new();
        for text in ["First", "Second", "Third"] {
            ids.push(opened(answer(&assistant, &ai, send(None, text)).await).0.id);
        }
        let file = |id: &str| dir.path().join("assistant").join(format!("{id}.jsonl"));

        let unknown = Request::DeleteConversations { ids: vec![ids[0].clone(), "ffffffffffffffff".into()] };
        // Refused before anyone would be asked to approve it.
        assert_eq!(assistant.refusal(&ai, &unknown).await.as_ref().map(code), Some("not-found"));
        let known = Request::DeleteConversations { ids: vec![ids[0].clone()] };
        assert_eq!(assistant.refusal(&ai, &known).await, None);
        assert_eq!(code(&answer(&assistant, &ai, unknown).await), "not-found");
        assert!(file(&ids[0]).exists(), "nothing deleted");

        let two = Request::DeleteConversations { ids: vec![ids[0].clone(), ids[1].clone(), ids[0].clone()] };
        assert_eq!(answer(&assistant, &ai, two).await, Response::Ok);
        assert!(!file(&ids[0]).exists() && !file(&ids[1]).exists(), "their files go");
        assert!(assistant.state().await.running.keys().all(|id| *id == ids[2]), "their answers stop");
        match assistant.list().await {
            Response::Conversations { conversations } => {
                assert_eq!(conversations.into_iter().map(|c| c.id).collect::<Vec<_>>(), [ids[2].clone()]);
            }
            other => panic!("expected conversations, got {other:?}"),
        }
    }

    /// Issue #697, decision 1: archived conversations are kept beyond the retention time.
    #[tokio::test]
    async fn retention_keeps_archived_conversations() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::ZERO)), no_ai(dir.path()));
        let mut ids = Vec::new();
        for text in ["old", "old and archived"] {
            let (summary, _) = opened(answer(&assistant, &ai, send(None, text)).await);
            finished(&assistant, &ai, &summary.id).await;
            ids.push(summary.id);
        }
        let archive = Request::ArchiveConversations { ids: vec![ids[1].clone()], archived: true };
        assert_eq!(answer(&assistant, &ai, archive).await, Response::Ok);
        let now = Utc::now();
        {
            let mut state = assistant.state().await;
            for id in &ids {
                for entry in &mut state.conversations.get_mut(id).expect("held").entries {
                    entry.time = now - chrono::Duration::days(31);
                }
            }
        }
        assert_eq!(assistant.clean_up(now, Retention::Days(30)).await, 1);
        let state = assistant.state().await;
        assert!(!state.conversations.contains_key(&ids[0]), "old goes");
        assert!(state.conversations.contains_key(&ids[1]), "archived stays");
    }

    /// Writes a conversation `id` titled `title` with `contents`, its last entry a few minutes old, with what
    /// tool lines answered (`results`, by the position in `contents`), changed by `setup` before.
    pub(crate) fn written(
        folder: &std::path::Path,
        id: &str,
        title: &str,
        contents: Vec<EntryContent>,
        results: &[(usize, &str)],
        setup: impl FnOnce(&mut Conversation),
    ) {
        let now = Utc::now();
        let mut conversation = Conversation::new(id.into(), title, now - chrono::Duration::hours(1));
        setup(&mut conversation);
        let mut lines = vec![conversation.started()];
        for (at, content) in contents.into_iter().enumerate() {
            let entry = conversation.next(content, now - chrono::Duration::minutes(50 - at as i64));
            conversation.apply(Line::Entry { entry: entry.clone() });
            lines.push(Line::Entry { entry: entry.clone() });
            if let Some((_, text)) = results.iter().find(|(position, _)| *position == at) {
                lines.push(Line::Result { seq: entry.seq, text: (*text).into() });
            }
        }
        store::append(folder, id, &lines).expect("written");
    }

    pub(crate) fn message(text: &str) -> EntryContent {
        EntryContent::Message {
            text: text.into(),
            engine: TEST_ENGINE.into(),
            model: None,
            effort: None,
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            from: None,
        }
    }

    pub(crate) fn said(text: &str) -> EntryContent {
        EntryContent::Answer { text: text.into(), engine: TEST_ENGINE.into(), model: None, spoken: None }
    }

    pub(crate) fn tool(name: &str) -> EntryContent {
        EntryContent::Tool {
            name: name.into(),
            input: serde_json::json!({}),
            summary: "1".into(),
            error: None,
            lack: None,
        }
    }

    pub(crate) fn hits(response: Response) -> Vec<ConversationHit> {
        match response {
            Response::ConversationHits { hits } => hits,
            other => panic!("expected hits, got {other:?}"),
        }
    }

    /// Issue #774: `get_conversation` gives an answer written in two parts apart, the stored one and the
    /// growing one, and nobody sees the mark; the core keeps the model's text as it came.
    #[tokio::test]
    async fn an_answer_comes_in_its_spoken_and_its_shown_part() {
        const ID: &str = "0000000000000774";
        let dir = tempfile::tempdir().expect("temp dir");
        let raw = "Nine events, have a look.\n---shown---\n- Mon 9:00 Standup";
        written(&dir.path().join("assistant"), ID, "Week", vec![message("What is on?"), said(raw)], &[], |_| {});
        let assistant = assistant_in(dir.path(), Duration::ZERO);
        let entries = |response: Response| match response {
            Response::Conversation { entries, .. } => entries,
            other => panic!("expected a conversation, got {other:?}"),
        };
        let stored = entries(assistant.get(ID, None).await);
        let parts = |entry: &Entry| match &entry.content {
            EntryContent::Answer { text, spoken, .. } => (spoken.clone(), text.clone()),
            other => panic!("expected an answer, got {other:?}"),
        };
        assert_eq!(parts(&stored[1]), (Some("Nine events, have a look.".into()), "- Mon 9:00 Standup".into()));
        let kept = assistant.state().await.conversations.get(ID).map(|conversation| conversation.entries_after(None));
        assert!(kept.is_some_and(
            |kept| matches!(&kept[1].content, EntryContent::Answer { text, spoken: None, .. } if text == raw)
        ));

        let (_, partial) = growing(&assistant, ID).await;
        partial.lock().unwrap_or_else(PoisonError::into_inner).text = "Two more.\n---sh".into();
        let running = entries(assistant.get(ID, Some(2)).await);
        assert_eq!(parts(&running[0]), (None, "Two more.\n".into()), "the mark being written is left out");
        partial.lock().unwrap_or_else(PoisonError::into_inner).text = "Two more.\n---shown---\nTue".into();
        let running = entries(assistant.get(ID, Some(2)).await);
        assert_eq!(parts(&running[0]), (Some("Two more.".into()), "Tue".into()));
    }

    fn search(query: &str) -> Request {
        Request::SearchConversations { query: query.into(), limit: None }
    }

    /// Issue #704: the words are found in messages, answers and titles, in any case, never in what a tool
    /// answered; a deleted conversation and one past its retention are found no more.
    #[tokio::test]
    async fn the_search_finds_messages_answers_and_titles() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("assistant");
        let trip = "0000000000000001";
        let invoices = "0000000000000002";
        let old = "0000000000000003";
        written(
            &folder,
            trip,
            "Trip to Lisbon",
            vec![
                message("When do I fly?"),
                tool("calendar.list_events"),
                said("Book the   FLIGHT\non the 14th, the hotel is booked."),
            ],
            &[(1, "Flight secretword on the 14th")],
            |_| {},
        );
        written(&folder, invoices, "Invoices October", vec![message("The flight was refunded")], &[], |c| {
            c.title = "Invoices October".into();
        });
        written(&folder, old, "Old plans", vec![message("A flight long ago")], &[], |_| {});
        let (assistant, ai) = (Arc::new(assistant_in(dir.path(), Duration::ZERO)), no_ai(dir.path()));

        let found = hits(answer(&assistant, &ai, search("flight")).await);
        assert_eq!(found.len(), 3, "{found:?}");
        let trip_hit = found.iter().find(|hit| hit.conversation == trip).expect("the trip");
        assert_eq!((trip_hit.seq, trip_hit.kind), (Some(3), torro_ipc::assistant::HitKind::Answer));
        assert_eq!(trip_hit.snippet, "Book the FLIGHT on the 14th, the hotel is booked.", "white space folded");
        assert_eq!(trip_hit.title, "Trip to Lisbon");
        assert!(!trip_hit.untrusted, "the calendar is the suite's own");
        let message_hit = found.iter().find(|hit| hit.conversation == invoices).expect("the invoices");
        assert_eq!(message_hit.kind, torro_ipc::assistant::HitKind::Message);

        let both = hits(answer(&assistant, &ai, search("Hotel flight")).await);
        assert_eq!(both.iter().map(|hit| hit.conversation.as_str()).collect::<Vec<_>>(), [trip], "every word");
        assert!(hits(answer(&assistant, &ai, search("secretword")).await).is_empty(), "never a tool's result");
        let titled = hits(answer(&assistant, &ai, search("lisbon")).await);
        assert_eq!((titled.len(), titled[0].seq, titled[0].kind), (1, None, torro_ipc::assistant::HitKind::Title));
        let one = Request::SearchConversations { query: "flight".into(), limit: Some(1) };
        assert_eq!(hits(answer(&assistant, &ai, one).await).len(), 1, "the limit holds");
        let none = Request::SearchConversations { query: "flight".into(), limit: Some(0) };
        assert_eq!(hits(answer(&assistant, &ai, none).await).len(), 1, "at least one");

        assert_eq!(answer(&assistant, &ai, Request::DeleteConversation { id: invoices.into() }).await, Response::Ok);
        {
            let mut state = assistant.state().await;
            let conversation = state.conversations.get_mut(old).expect("held");
            for entry in &mut conversation.entries {
                entry.time = Utc::now() - chrono::Duration::days(31);
            }
        }
        assert_eq!(assistant.clean_up(Utc::now(), Retention::Days(30)).await, 1);
        let left = hits(answer(&assistant, &ai, search("flight")).await);
        assert_eq!(left.iter().map(|hit| hit.conversation.as_str()).collect::<Vec<_>>(), [trip], "deleted and expired");
    }

    /// Issue #704: what came from outside stays `untrusted` – a chat about a mail, an answer after reading mail,
    /// a client's message – and a scope narrows: no bot's conversation for the assistant, no hit after an area it
    /// may not read, none about a mail it may not read.
    #[tokio::test]
    async fn the_search_marks_outside_text_and_keeps_its_scope() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("assistant");
        let mail = MailRef { account: "mail:anna@example.org".into(), id: "m:1".into() };
        let about = mail.clone();
        written(&folder, "0000000000000001", "About the mail", vec![said("The invoice is due")], &[], |c| {
            c.about = Some(about);
        });
        written(
            &folder,
            "0000000000000002",
            "Inbox",
            vec![message("What is new?"), said("No invoice yet"), tool("mail.get_message"), said("The invoice came")],
            &[],
            |_| {},
        );
        let mut sent = message("Send me the invoice");
        if let EntryContent::Message { from, .. } = &mut sent {
            *from = Some("7e57c11e".into());
        }
        written(&folder, "0000000000000003", "Script", vec![sent], &[], |_| {});
        written(&folder, "0000000000000004", "Bot", vec![said("The bot's invoice")], &[], |c| {
            c.bot = Some("b1".into());
        });
        let assistant = assistant_in(dir.path(), Duration::ZERO);

        let all = hits(assistant.search("invoice", None, &Scope::all()).await);
        let marked = |id: &str| -> Vec<bool> {
            all.iter().filter(|hit| hit.conversation == id).map(|hit| hit.untrusted).collect()
        };
        assert_eq!(marked("0000000000000001"), [true], "about a mail");
        assert_eq!(marked("0000000000000002"), [true, false], "newest first; after reading mail");
        assert_eq!(marked("0000000000000003"), [true], "a client's message");
        assert_eq!(marked("0000000000000004"), [true], "a bot's");

        let no_mail = |name: &str| !name.starts_with("mail.");
        let narrow = Scope { own_only: true, readable: Some(&no_mail), reads_about: &|_| false };
        let seen: Vec<(String, Option<u64>)> = hits(assistant.search("invoice", None, &narrow).await)
            .into_iter()
            .map(|hit| (hit.conversation, hit.seq))
            .collect();
        assert_eq!(
            seen,
            [("0000000000000002".to_string(), Some(2)), ("0000000000000003".to_string(), Some(1))],
            "no bot, no mail it may not read, nothing after reading mail"
        );
        // The title stays findable when every hit in the history is left out.
        let after_mail: Vec<(String, Option<u64>)> = hits(assistant.search("inbox", None, &narrow).await)
            .into_iter()
            .map(|hit| (hit.conversation, hit.seq))
            .collect();
        assert_eq!(after_mail, [("0000000000000002".to_string(), None)], "the title, though no entry is left");
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

    /// Issue #714: a result older than a day goes from a pinned conversation too, also when conversations are kept
    /// for good; the conversation and its newer result stay.
    #[tokio::test]
    async fn old_results_go_even_in_pinned_conversations() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("assistant");
        let now = Utc::now();
        let mut conversation = Conversation::new("0123456789abcdef".into(), "Read my mail", now);
        conversation.pinned = true;
        let mut lines = vec![conversation.started(), Line::Pinned { pinned: true }];
        for (hours, text) in [(25, "the mail of yesterday"), (1, "the mail of this morning")] {
            let tool = EntryContent::Tool {
                name: "mail.get_message".into(),
                input: serde_json::json!({}),
                summary: "1 mail".into(),
                error: None,
                lack: None,
            };
            let entry = conversation.next(tool, now - chrono::Duration::hours(hours));
            let result = Line::Result { seq: entry.seq, text: text.into() };
            conversation.apply(Line::Entry { entry: entry.clone() });
            conversation.apply(result.clone());
            lines.extend([Line::Entry { entry }, result]);
        }
        store::append(&folder, &conversation.id, &lines).expect("written");

        let assistant = assistant_in(dir.path(), Duration::ZERO);
        assert_eq!(assistant.clean_up(now, Retention::Forever).await, 0, "the conversation stays");
        assert_eq!(
            assistant.state().await.conversations.get(&conversation.id).map(|c| c.results.len()),
            Some(1),
            "held without it"
        );

        let again = assistant_in(dir.path(), Duration::ZERO);
        let state = again.state().await;
        let read = state.conversations.get(&conversation.id).expect("still there");
        assert!(read.pinned);
        assert_eq!(read.entries, conversation.entries, "every entry stays");
        assert_eq!(read.results, std::collections::BTreeMap::from([(2, "the mail of this morning".to_string())]));
    }

    /// Issue #724: a result marked `untrusted` leaves its mark when it goes after a day: in memory and, read anew,
    /// from the file.
    #[tokio::test]
    async fn a_dropped_untrusted_result_leaves_its_mark() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("assistant");
        let now = Utc::now();
        let mut conversation = Conversation::new("0123456789abcdef".into(), "Look at the issue", now);
        let tool = EntryContent::Tool {
            name: "tasks.save_todo".into(),
            input: serde_json::json!({}),
            summary: "1 task".into(),
            error: None,
            lack: None,
        };
        let entry = conversation.next(tool, now - chrono::Duration::hours(25));
        let result = Line::Result { seq: entry.seq, text: "{\"untrusted\":true,\"data\":\"an issue\"}".into() };
        conversation.apply(Line::Entry { entry: entry.clone() });
        conversation.apply(result.clone());
        store::append(&folder, &conversation.id, &[conversation.started(), Line::Entry { entry }, result])
            .expect("written");

        let assistant = assistant_in(dir.path(), Duration::ZERO);
        assistant.clean_up(now, Retention::Forever).await;
        let state = assistant.state().await;
        let held = state.conversations.get(&conversation.id).expect("there");
        assert!(held.results.is_empty() && held.untrusted);
        drop(state);

        let again = assistant_in(dir.path(), Duration::ZERO);
        let state = again.state().await;
        let read = state.conversations.get(&conversation.id).expect("still there");
        assert!(read.results.is_empty() && read.untrusted);
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
        let bot = match crate::bots::answer(&assistant, &clients, save, true).await {
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
