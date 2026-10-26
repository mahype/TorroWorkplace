//! Engine B (task C.7, `docs/assistent.md` "Woher das Modell kommt"): a coding agent installed on
//! this computer answers in the assistant, without a terminal, with the suite's MCP server as its
//! only tools. Claude Code is such an agent ([`claude`]); Codex is not offered (below).
//!
//! - **Not offered any more** (task C.18, issue #219): an agent was never set up, only *found*, so
//!   nobody could see why Claude answered in the assistant and nobody could switch it off. Claude
//!   is an AI account with its own key now (engine A, `docs/ki.md`). [`Agents::offered`] is `false`
//!   in the product, there is no switch, and this module stays as it is: bringing engine B back
//!   means turning one `false` around.
//! - **Offered** ([`Agents::engines`]) while [`Agents::offered`] — only the test constructor
//!   ([`Agents::new`]) — when the agent's command is found as "Connect agent" finds it
//!   ([`crate::client_setup::Target::locate`]: on the `PATH`, Claude Code also in
//!   `~/.local/bin` and `~/.claude/local`) **and** the assistant has its own client: the agent
//!   runs with that client's key, so without one it could do nothing (decision 5: one client
//!   "Assistant" for every engine).
//! - **One run per message** ([`answer`]): the core makes a folder of its own for the run
//!   (`<state>/assistant-runs/<random>`, 0700) with the MCP configuration (0600, the suite's
//!   server only, no key), starts the agent there with its flags ([`claude::arguments`]: no
//!   built-in tool, no other MCP server, nothing but `mcp__torroworkplace__*`, the system prompt
//!   of task C.3 appended with the personality and the memory of task C.10, without notes marked
//!   `local_only`), writes the message to its input and closes it, and turns what it
//!   writes into entries: text as a growing answer, then answer entries, a tool line per call
//!   (name, input, result in a few words), a card after a command that made an item (issue #225,
//!   only for a file the register keeps as the client's, `super::cards::of_agent`), an error entry
//!   when it fails. The folder is removed
//!   when the run ends, however it ends.
//! - **The key** is lent for the run only ([`crate::clients::Clients::lend`] through
//!   [`Host::lend`]): a key of the assistant's client made for this run, kept in the core's
//!   memory as a hash, put into the agent's environment (`TORRO_TOKEN`, which the MCP server it
//!   starts inherits) and nowhere else: not on the command line, not in the MCP configuration, not
//!   in the conversation, not in the log. It is taken back when the run ends, so a key the agent
//!   might have kept is worth nothing afterwards. So no plain key of the assistant needs to live
//!   on disk or in the keychain for engine B.
//! - **Rights**: requests with that key run as the assistant's client with its rights of now
//!   minus the areas switched off in the conversation and minus `mail`: an agent sends what it
//!   reads to its provider, and the data rule (`super::data_rule`, `docs/ki.md`: mail only to
//!   models on the person's computers) keeps mail from it, unless the person lets mail go to
//!   cloud models (setting `mail_to_cloud`, task C.17; read when the run starts).
//!   Without mail rights the inbox hides the entries of mail accounts too
//!   (`Rights::reaches_source`). The core checks this, not the agent.
//! - **History**: the agent keeps no session (`--no-session-persistence`); every message carries
//!   the conversation so far as text ([`message`]), with the answers the data rule keeps from a
//!   model elsewhere replaced by a note (`super::model::history`). So the conversation lives only
//!   in the core's store, under its retention, and deleting it deletes it.
//! - **Stop and limits**: `assistant.cancel` aborts the run; the child is killed with it
//!   (`kill_on_drop`), the key taken back, the folder removed. A run that writes nothing for
//!   [`IDLE`] or takes longer than [`TOTAL`] is stopped the same way with an error entry. What
//!   the agent writes on stderr becomes the detail of an error entry, cut short and with the key
//!   and the paths of the run's folder and the home directory taken out.
//! - **Checked at its start**: the agent's `init` event lists its tools; one that is not the
//!   suite's stops the run before the model is asked anything, should a version of the agent
//!   ignore a flag.
//!
//! - **Bots** (task B.1): a bot's answer runs the same way with a key of the bot's client, its
//!   prompt ([`prompt::bot_system`]) and no notes of the assistant's memory.
//!
//! While a run lasts, the key is in the agent's environment, which the same user can read (as
//! with every client key an agent holds); it is worth nothing once the run ends.
//!
//! **Codex is not offered.** `codex exec --json` has no way to say "these tools and no other":
//! its shell, its file and image tools, its browser and web search are features that are on
//! unless switched off one by one (`--disable shell_tool`, `--disable unified_exec`, …), the list
//! changes with every version (Codex 0.160 lists features as removed, renamed and under
//! development side by side), and its sandbox `read-only` still lets a shell read every file.
//! "Files and shell always off" (decision 6) cannot be promised that way, so Codex stays an agent
//! for the terminal until it can be started with an allow-list of tools.

mod claude;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{Local, Utc};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::Instant;
use torro_core::AppId;
use torro_core::ai::Role;
use torro_core::i18n::{self, tr, trf};
use torro_core::paths::AppPaths;
use torro_ipc::api::Area;
use torro_ipc::assistant::{Engine, EngineKind, EntryContent, Mode};
use torro_ipc::{Key, Response};

use super::data_rule::{DataRule, Place, Reader};
use super::model::{Host, Job, command_name, history};
use super::{Assistant, Partial, prompt, store, tools};
use crate::client_setup::{Env, Method, TOKEN_VARIABLE, Target};

/// A run that writes nothing for this long is stopped: longer than an MCP tool waits for a
/// person's approval (60 s) and than a model thinks between two pieces.
pub const IDLE: Duration = Duration::from_secs(5 * 60);

/// A run is stopped after this long in all.
pub const TOTAL: Duration = Duration::from_secs(20 * 60);

/// At most this many bytes of the agent's stderr are kept for an error entry.
const STDERR_BYTES: usize = 16 * 1024;

/// At most this many characters of the agent's own words go into an error entry.
const DETAIL_CHARS: usize = 300;

/// The areas an agent never gets under the default data rule, whatever the assistant's rights say:
/// its answers go to its provider, and the data rule keeps mail on the person's computers. The
/// person can let mail go to agents (task C.17, [`withheld_from_agents`]).
const NEVER_TO_AGENTS: &[Area] = &[Area::Mail];

/// The areas a run of an agent goes without under `rule`: [`NEVER_TO_AGENTS`], or none once the
/// person let mail go to cloud models.
fn withheld_from_agents(rule: DataRule) -> &'static [Area] {
    match rule.mail_to_cloud {
        true => &[],
        false => NEVER_TO_AGENTS,
    }
}

/// The areas the engine `engine` does not get under the default data rule, whatever the
/// assistant's rights say: [`NEVER_TO_AGENTS`] for a coding agent (`claude-code`), none for an AI
/// account or the test engine. The gate names those of them a key was lent without as the reason of
/// a `forbidden` (issue #133), apart from the areas switched off in a conversation; with mail let out
/// (task C.17) a run is lent with mail, so none of them counts.
pub fn never_given(engine: &str) -> &'static [Area] {
    match Agent::of(engine) {
        Some(_) => NEVER_TO_AGENTS,
        None => &[],
    }
}

/// A coding agent that can drive the assistant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
}

impl Agent {
    /// Every agent offered, in the order `assistant.list_engines` lists them.
    pub const ALL: [Agent; 1] = [Agent::ClaudeCode];

    /// Its engine id for `assistant.send_message`, the same as its client kind: `claude-code`.
    pub fn id(self) -> &'static str {
        self.target().kind()
    }

    /// Its product's name, not translated: `Claude Code`.
    pub fn name(self) -> &'static str {
        self.target().label()
    }

    /// The agent `id` names ([`Agent::id`]).
    pub fn of(id: &str) -> Option<Agent> {
        Agent::ALL.into_iter().find(|agent| agent.id() == id)
    }

    /// The models it offers (issue #131), as `assistant.list_engines` lists them.
    pub fn models(self) -> Vec<torro_core::ai::AiModel> {
        match self {
            Agent::ClaudeCode => claude::models(),
        }
    }

    fn target(self) -> Target {
        match self {
            Agent::ClaudeCode => Target::ClaudeCode,
        }
    }
}

/// Where the agents are found and how their runs are bounded.
pub struct Agents {
    /// Whether agents are offered as engines at all (task C.18, issue #219). `false` everywhere in
    /// the product, with no switch and no environment variable; only the test constructor
    /// ([`Agents::new`]) sets it, so the tests of task C.7 keep running and no symbol here goes
    /// unused. Turning engine B back on means setting this `true` in [`Agents::system`] again.
    offered: bool,
    /// Where to look for the agents' commands, and the `torroworkplace` their MCP configuration
    /// runs ([`Env::program`]); `None` offers no agent (tests that bring none, no home directory).
    env: Option<Env>,
    /// The folder the runs' folders go in; `None` offers no agent either.
    runs: Option<PathBuf>,
    idle: Duration,
    total: Duration,
}

impl Agents {
    /// No agents: what an assistant gets unless it is the user's ([`Agents::system`]).
    pub fn none() -> Agents {
        Agents { offered: false, env: None, runs: None, idle: IDLE, total: TOTAL }
    }

    /// The user's: the commands on this process's `PATH` and in the agents' own folders, the runs
    /// in `<state>/assistant-runs` — found, but not offered (issue #219).
    pub fn system() -> Agents {
        let runs = AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join("assistant-runs"));
        Agents { offered: false, env: Env::current(), runs, idle: IDLE, total: TOTAL }
    }

    /// Agents found in `env` and offered, their runs in `runs`, stopped after `idle` without
    /// output or `total` in all; for tests.
    #[cfg(test)]
    pub fn new(env: Env, runs: PathBuf, idle: Duration, total: Duration) -> Agents {
        Agents { offered: true, env: Some(env), runs: Some(runs), idle, total }
    }

    /// The same agents, found but not offered — what the product does ([`Agents::system`]); for
    /// tests.
    #[cfg(test)]
    pub fn not_offered(self) -> Agents {
        Agents { offered: false, ..self }
    }

    /// The command of `agent` when it is installed: found as "Connect agent" finds it, through its
    /// own command, not only its files.
    fn command(&self, agent: Agent) -> Option<PathBuf> {
        self.runs.as_ref()?;
        match agent.target().locate(self.env.as_ref()?)? {
            Method::Cli { executable, .. } => Some(executable),
            Method::Json { .. } | Method::Toml { .. } => None,
        }
    }

    /// The agents that can answer: offered ([`Agents::offered`], never in the product since issue
    /// #219), installed, and only while the assistant has its own `client`.
    pub fn engines(&self, client: bool) -> Vec<Engine> {
        if !self.offered || !client {
            return Vec::new();
        }
        Agent::ALL
            .into_iter()
            .filter(|agent| self.command(*agent).is_some())
            .map(|agent| Engine {
                id: agent.id().into(),
                kind: EngineKind::Agent,
                name: agent.name().into(),
                problem: None,
                models: agent.models(),
            })
            .collect()
    }

    /// The offered and installed agent with the engine id `id`; nothing in the product (issue
    /// #219). Not [`never_given`]'s question: that one asks [`Agent::of`], so an engine id of an
    /// older conversation keeps its data rule even though no agent is offered any more.
    pub fn find(&self, id: &str) -> Option<Agent> {
        if !self.offered {
            return None;
        }
        Agent::of(id).filter(|agent| self.command(*agent).is_some())
    }
}

/// Runs the answer `job` of `assistant` with `agent`, its tools through the suite's MCP server
/// with a key of the assistant's client lent by `host`, until the agent is done, fails, takes
/// too long or the run is stopped. Its last entry ends the run.
pub(super) async fn answer(assistant: Arc<Assistant>, host: Arc<dyn Host>, job: Job, agent: Agent) {
    let mut run = Running {
        assistant,
        host: Arc::clone(&host),
        job,
        agent,
        model: None,
        calls: HashMap::new(),
        answered: false,
        rights: None,
        withheld: NEVER_TO_AGENTS,
    };
    let ended = run.go(host.as_ref()).await;
    match ended {
        Ok(Ended::Stopped) => {}
        Ok(Ended::Done(_)) if run.answered => run.assistant.end_run(&run.job.conversation, run.job.run).await,
        Ok(Ended::Done(text)) if !text.trim().is_empty() => {
            let answer = EntryContent::Answer { text, engine: run.job.engine.clone(), model: run.model.clone() };
            run.add(answer, true).await;
        }
        Ok(Ended::Done(_)) => run.error("error", tr("The agent gave no answer. Try again.")).await,
        Err(failed) => run.error(failed.code(), &failed.message(agent)).await,
    }
}

/// How a run ended without failing.
enum Ended {
    /// The agent is done; its last text, which is an answer entry already unless nothing was.
    Done(String),
    /// `assistant.cancel` stopped it, or the conversation went.
    Stopped,
}

/// Why a run failed, for its error entry.
enum Failed {
    /// The assistant has no client (any more).
    NoClient,
    /// The bot that answers has no client (any more): the person removed it (task B.1).
    NoBotClient,
    /// The folder of the run could not be made.
    Prepare,
    /// The agent could not be started; the system's words.
    Start(String),
    /// Nothing for [`Agents::idle`], or longer than [`Agents::total`].
    TooLong,
    /// The agent said it failed; its words.
    Agent(String),
    /// The agent has tools of its own besides the suite's (their names): it must not run.
    OwnTools(String),
    /// It ended without saying it was done; what it wrote on stderr.
    Ended(String),
}

impl Failed {
    fn code(&self) -> &'static str {
        match self {
            Failed::NoClient | Failed::NoBotClient => "forbidden",
            _ => "error",
        }
    }

    fn message(&self, agent: Agent) -> String {
        let name = agent.name();
        match self {
            Failed::NoClient => {
                tr("The assistant has no access yet. Choose what it may use in Torro Workplace › Assistant.").into()
            }
            Failed::NoBotClient => {
                tr("This bot has no access any more. Save it again, then choose what it may use.").into()
            }
            Failed::Prepare => trf("The assistant could not prepare {agent}. Try again.", &[("agent", &name)]),
            Failed::Start(why) => trf("{agent} could not be started: {why}", &[("agent", &name), ("why", why)]),
            Failed::TooLong => trf("{agent} took too long and was stopped.", &[("agent", &name)]),
            Failed::Agent(why) => trf("{agent} reported an error: {why}", &[("agent", &name), ("why", why)]),
            Failed::OwnTools(tools) => trf(
                "{agent} was stopped before it answered: it offered tools besides Torro Workplace ({tools}).",
                &[("agent", &name), ("tools", tools)],
            ),
            Failed::Ended(why) if why.is_empty() => trf("{agent} stopped without an answer.", &[("agent", &name)]),
            Failed::Ended(why) => trf("{agent} stopped without an answer: {why}", &[("agent", &name), ("why", why)]),
        }
    }
}

/// The state of one run.
struct Running {
    assistant: Arc<Assistant>,
    /// The core: whether a file of a tool line is the client's own, before its card is added (issue #225).
    host: Arc<dyn Host>,
    job: Job,
    agent: Agent,
    /// The model the agent named when it started.
    model: Option<String>,
    /// The tool calls waiting for their results: name and input by the call's id.
    calls: HashMap<String, (String, Value)>,
    /// An answer entry was added.
    answered: bool,
    /// The rights of the client the run uses, as stored when it started: why a tool was
    /// `forbidden` is told from them (issue #133).
    rights: Option<torro_ipc::api::Rights>,
    /// The areas the run goes without whatever the rights say ([`withheld_from_agents`]), as the
    /// data rule was when it started.
    withheld: &'static [Area],
}

impl Running {
    /// Prepares and starts the agent and follows it to its end.
    async fn go(&mut self, host: &dyn Host) -> Result<Ended, Failed> {
        let agents = &self.assistant.agents;
        let (Some(command), Some(runs), Some(env)) = (agents.command(self.agent), &agents.runs, &agents.env) else {
            return Err(Failed::Start(tr("it is not installed").into()));
        };
        let (runs, program, idle, total) = (runs.clone(), env.program.clone(), agents.idle, agents.total);
        let home = env.home.clone();
        let actor = self.job.actor.clone();
        let no_client = || if self.job.bot.is_some() { Failed::NoBotClient } else { Failed::NoClient };
        let Some(client) = host.client_of(&actor) else { return Err(no_client()) };
        let rule = host.data_rule();
        self.withheld = withheld_from_agents(rule);
        let mut without = self.job.without.clone();
        for area in self.withheld {
            if !without.contains(area) {
                without.push(*area);
            }
        }
        // Lent for this conversation (task B.2): the core then knows where the agent asks a bot from,
        // and for a bot that runs for the assistant, what both clients may.
        // In the permission mode of the message (issue #132): the key, the tools and the prompt.
        let mode = self.job.mode;
        let lent = host.lend_in_mode(&actor, &self.job.conversation, &self.job.engine, &without, mode);
        let Some(lent) = lent else { return Err(no_client()) };
        let rights = mode.narrow(&client.rights);
        self.rights = Some(rights.clone());
        // The assistant's own tools (issue #225): only those switched on, in the selection and with
        // write access to `tools` are named in the prompt and left to the agent.
        let rights = tools::with_tools_on(&rights, &host.tools_on());
        let areas = allowed_areas(actor.kind(), &rights, &without, mode);
        let usable = tools::usable_tools(&rights, &areas);
        // The personality and the memory (task C.10); the prompt keeps the notes marked
        // `local_only` from the agent, which runs elsewhere. A bot has its own personality and no
        // memory (task B.1).
        let (persona, memory) = match &self.job.bot {
            Some(bot) => (bot.personality.clone(), Vec::new()),
            None => self.assistant.persona_and_notes().await,
        };
        // The servers whose tools it finds over Torro's MCP server with these rights (issue #302).
        let servers: std::collections::BTreeSet<String> = host
            .extension_tools(&rights)
            .into_iter()
            .filter(|tool| tool.tool_name.is_some())
            .map(|tool| tool.extension)
            .collect();
        let servers: Vec<String> = servers.into_iter().collect();
        let context = prompt::Context {
            now: Local::now(),
            lang: i18n::lang(),
            person: None,
            areas: &areas,
            without: &self.job.without,
            rights: Some(&rights),
            withheld: self.withheld,
            engine: self.agent.name(),
            model: "",
            place: Place::Remote,
            mail_to_cloud: rule.mail_to_cloud,
            memory: &memory,
            servers: &servers,
        };
        let system = match &self.job.bot {
            Some(bot) => prompt::bot_system(Some(&persona), &prompt::BotParts::of(bot), &context),
            None => prompt::system(Some(&persona), &context),
        };
        let folder = RunFolder::make(&runs, &program).map_err(|err| {
            eprintln!("torroworkplace: cannot prepare a run of {} in {}: {err}", self.agent.name(), runs.display());
            Failed::Prepare
        })?;
        // The tools of the areas it does not get are not offered at all (issue #133): the model then
        // never tries them, and the prompt says why.
        let (model, effort) = (self.job.model.as_deref(), self.job.effort);
        let arguments = claude::arguments(&folder.config(), &system, model, effort, &without, mode, &usable);
        let mut child = start(&command, &arguments, folder.path(), lent.key())
            .await
            .map_err(|err| Failed::Start(detail(&err.to_string(), lent.key(), &[folder.path(), &home])))?;
        let input = claude::input(&message(&self.job, rule));
        let stderr = collect(child.stderr.take());
        if let Some(mut stdin) = child.stdin.take() {
            // An agent that does not read is caught by the timeouts; its error is in its output.
            let _ = stdin.write_all(format!("{input}\n").as_bytes()).await;
            let _ = stdin.shutdown().await;
        }
        let Some(stdout) = child.stdout.take() else { return Err(Failed::Ended(String::new())) };
        let ended = match self.follow(stdout, idle, total).await {
            Some(Err(Failed::Agent(why))) => Err(Failed::Agent(detail(&why, lent.key(), &[folder.path(), &home]))),
            Some(ended) => ended,
            None => {
                // The output closed without a result: wait for the agent briefly, then tell what
                // it said.
                let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
                let said = tokio::time::timeout(Duration::from_secs(1), stderr).await;
                let said = said.ok().and_then(Result::ok).unwrap_or_default();
                Err(Failed::Ended(detail(&said, lent.key(), &[folder.path(), &home])))
            }
        };
        // However the run ended: the agent is killed, the key taken back, the folder removed.
        let _ = child.start_kill();
        drop((child, lent, folder));
        ended
    }

    /// Reads the agent's output to its end: `None` when it closed without saying it was done.
    async fn follow(
        &mut self,
        stdout: tokio::process::ChildStdout,
        idle: Duration,
        total: Duration,
    ) -> Option<Result<Ended, Failed>> {
        let deadline = Instant::now() + total;
        let mut lines = BufReader::new(stdout).lines();
        loop {
            let wait = idle.min(deadline.saturating_duration_since(Instant::now()));
            let line = match tokio::time::timeout(wait, lines.next_line()).await {
                Err(_) => return Some(Err(Failed::TooLong)),
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None) | Err(_)) => return None,
            };
            for event in claude::events(&line) {
                if let Some(ended) = self.take(event).await {
                    return Some(ended);
                }
            }
        }
    }

    /// Takes one event into the conversation; `Some` when the run is over.
    async fn take(&mut self, event: claude::Event) -> Option<Result<Ended, Failed>> {
        match event {
            claude::Event::Started { tools, .. } if tools.iter().any(|tool| !tool.starts_with(claude::TOOL_PREFIX)) => {
                // A version that ignores one of the flags: stopped before the model is asked.
                let own: Vec<String> =
                    tools.into_iter().filter(|tool| !tool.starts_with(claude::TOOL_PREFIX)).take(5).collect();
                return Some(Err(Failed::OwnTools(own.join(", "))));
            }
            claude::Event::Started { model, .. } if model.is_empty() => {}
            claude::Event::Started { model, .. } => {
                lock(&self.job.partial).model = Some(model.clone());
                self.model = Some(model);
            }
            claude::Event::Piece(piece) => {
                let mut partial = lock(&self.job.partial);
                partial.started.get_or_insert_with(Utc::now);
                partial.text.push_str(&piece);
            }
            claude::Event::Text(text) if text.trim().is_empty() => {}
            claude::Event::Text(text) => {
                let answer = EntryContent::Answer { text, engine: self.job.engine.clone(), model: self.model.clone() };
                if !self.add(answer, false).await {
                    return Some(Ok(Ended::Stopped));
                }
                self.answered = true;
                lock(&self.job.partial).started = None;
            }
            claude::Event::ToolUse { id, name, input } => {
                self.calls.insert(id, (name, input));
            }
            claude::Event::ToolResult { id, text, error } => {
                let (name, input) = self.calls.remove(&id).unwrap_or_else(|| (String::new(), Value::Null));
                let lack = if error { self.lack(&name, &text) } else { None };
                let line = tool_line(&name, input, &text, error, lack, self.agent.name());
                // A card after a command that made an item (issue #225), only for a file the register keeps
                // as this client's: the result is the agent's text.
                let card = match (&line, error) {
                    (EntryContent::Tool { name, .. }, false) => {
                        super::cards::of_agent(self.host.as_ref(), &self.job.actor, name, &text).await
                    }
                    _ => None,
                };
                if !self.add(line, false).await {
                    return Some(Ok(Ended::Stopped));
                }
                if let Some(card) = card
                    && !self.add(card, false).await
                {
                    return Some(Ok(Ended::Stopped));
                }
            }
            claude::Event::Done { error: Some(why), .. } => return Some(Err(Failed::Agent(why))),
            claude::Event::Done { error: None, text } => return Some(Ok(Ended::Done(text))),
        }
        None
    }

    /// Why the tool `tool` (Claude Code's name of it) was refused with the error `text` (issue #133):
    /// for `forbidden`, what the rights, the conversation's switches and the areas the run goes
    /// without ([`Running::withheld`]) say. A command of one of the assistant's own tools the rights
    /// allow (issue #225), as in engine A: the tool switched off now ([`Lack::ToolOff`]), else not in
    /// the selection ([`Lack::Tool`]).
    ///
    /// [`Lack::ToolOff`]: torro_ipc::api::Lack::ToolOff
    /// [`Lack::Tool`]: torro_ipc::api::Lack::Tool
    fn lack(&self, tool: &str, text: &str) -> Option<torro_ipc::api::Lack> {
        use torro_ipc::api::Lack;
        if error_code(text) != "forbidden" {
            return None;
        }
        let tool = tools::callable(tool.strip_prefix(claude::TOOL_PREFIX).unwrap_or(tool))?;
        let operation = torro_ipc::api::Operation { command: tool.command, access: tool.command.access };
        let kind = self.job.actor.kind();
        let rights = self.rights.as_ref();
        let lack = tools::lack_in(kind, rights, operation, &self.job.without, self.withheld, self.job.mode);
        match (lack, torro_core::settings::ToolId::of_command(operation.command.name), rights) {
            (None, Some(tool), Some(_)) if !self.host.tools_on().contains(&tool) => Some(Lack::ToolOff { tool }),
            (None, Some(tool), Some(rights)) if !rights.reaches_tool(tool) => Some(Lack::Tool { tool }),
            (lack, ..) => lack,
        }
    }

    /// Adds `content` while this run is the conversation's running answer; `last` ends it.
    async fn add(&self, content: EntryContent, last: bool) -> bool {
        self.assistant.add_to_run(&self.job.conversation, self.job.run, content, last).await
    }

    /// Ends the run with an error entry.
    async fn error(&self, code: &str, message: &str) {
        self.add(EntryContent::Error { code: code.into(), message: message.into() }, true).await;
    }
}

/// The areas a client of `kind` (the assistant's or a bot's) has tools in, without `without` and in
/// the permission mode `mode`: what the prompt names.
fn allowed_areas(kind: &str, rights: &torro_ipc::api::Rights, without: &[Area], mode: Mode) -> Vec<Area> {
    // The memory (task C.10) and the bots (task B.2) are the assistant's own, not areas the person
    // granted.
    let mut areas: Vec<Area> = tools::allowed_in(kind, Some(rights), without, mode)
        .iter()
        .map(|tool| tool.command.area)
        .filter(|area| !matches!(area, Area::Assistant | Area::Bots))
        .collect();
    areas.sort();
    areas.dedup();
    areas
}

/// The text the agent reads: the conversation so far (the agent keeps no session), as the data
/// `rule` lets it go to a model elsewhere, and the person's new message, which is the last entry of
/// the job.
fn message(job: &Job, rule: DataRule) -> String {
    // An agent names its model only once it runs, and every model of it runs elsewhere (task S.23).
    let mut messages = history(&job.history, &job.engine, None, Reader { place: Place::Remote, rule });
    let new = match messages.pop() {
        Some(message) if message.role == Role::User => message.content,
        Some(message) => {
            messages.push(message);
            String::new()
        }
        None => String::new(),
    };
    if messages.is_empty() {
        return new;
    }
    let earlier: Vec<String> = messages
        .into_iter()
        .map(|message| match message.role {
            Role::User => format!("Person:\n{}", message.content),
            _ => format!("You:\n{}", message.content),
        })
        .collect();
    format!(
        "Earlier in this conversation, for context (you keep no memory of it yourself):\n\n{}\n\n---\n\nThe \
         person's new message:\n\n{new}",
        earlier.join("\n\n")
    )
}

/// A tool line for the call of `tool` (Claude Code's name of it) with `input` and its result
/// `text`: the command's catalogue name, the result in a few words, the error code of an error
/// (`forbidden: …` as the MCP server writes it; `pending` for one that waits for a person), and
/// why it was `forbidden` (`lack`, issue #133) in words that name the agent `engine`.
fn tool_line(
    tool: &str,
    input: Value,
    text: &str,
    error: bool,
    lack: Option<torro_ipc::api::Lack>,
    engine: &str,
) -> EntryContent {
    let tool = tool.strip_prefix(claude::TOOL_PREFIX).unwrap_or(tool);
    let name = if tool.is_empty() { "unknown".to_string() } else { command_name(tool) };
    let input = if input.is_null() { serde_json::json!({}) } else { input };
    if error {
        let code = error_code(text);
        let summary = lack.map_or_else(|| tools::error_summary(&code), |lack| tools::lack_summary(lack, engine));
        return EntryContent::Tool { name, input, summary, error: Some(code), lack };
    }
    let summary = match serde_json::from_str::<Response>(text) {
        Ok(response) => tools::summary(&response, false),
        Err(_) => tr("Done").into(),
    };
    EntryContent::Tool { name, input, summary, error: None, lack: None }
}

/// The error code at the start of a tool error (`forbidden: this client may not …`); `error`
/// when it starts with none.
fn error_code(text: &str) -> String {
    let code = text.split_once(':').map(|(code, _)| code.trim()).unwrap_or_default();
    let valid = (1..=32).contains(&code.len()) && code.chars().all(|c| c.is_ascii_lowercase() || c == '-');
    if valid { code.to_string() } else { "error".to_string() }
}

/// The folder of one run with its MCP configuration; removed when dropped.
struct RunFolder(PathBuf);

impl RunFolder {
    /// A new folder in `runs` (0700) holding the MCP configuration for `program` (0600).
    fn make(runs: &Path, program: &Path) -> std::io::Result<RunFolder> {
        let id = store::new_id().map_err(std::io::Error::other)?;
        let folder = RunFolder(runs.join(id));
        crate::clients::create_private_dir(folder.path())?;
        let config = serde_json::to_vec_pretty(&claude::mcp_config(program)).map_err(std::io::Error::other)?;
        crate::clients::write_private(&folder.config(), &config)
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        Ok(folder)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn config(&self) -> PathBuf {
        self.0.join("mcp.json")
    }
}

impl Drop for RunFolder {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.0)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("torroworkplace: cannot remove the folder of an assistant run {}: {err}", self.0.display());
        }
    }
}

/// Starts `command` with `arguments` in `folder`, the key in its environment only. The child is
/// killed when it is dropped: a run that is stopped takes the agent with it. A command that is
/// being written right now (an update of the agent) is tried again a few times.
async fn start(command: &Path, arguments: &[OsString], folder: &Path, key: &Key) -> std::io::Result<Child> {
    let mut tries = 0;
    loop {
        match spawn(command, arguments, folder, key) {
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 5 => {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            started => return started,
        }
    }
}

fn spawn(command: &Path, arguments: &[OsString], folder: &Path, key: &Key) -> std::io::Result<Child> {
    Command::new(command)
        .args(arguments)
        .current_dir(folder)
        .env(TOKEN_VARIABLE, key.expose())
        // Started from a terminal agent's session, the core must not make this one look nested.
        .env_remove("CLAUDECODE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
}

/// Reads up to [`STDERR_BYTES`] of `stderr` on a task of its own, so a full pipe never blocks
/// the agent; the rest is read and dropped.
fn collect(stderr: Option<tokio::process::ChildStderr>) -> tokio::task::JoinHandle<String> {
    tokio::spawn(async move {
        let Some(mut stderr) = stderr else { return String::new() };
        let mut kept = Vec::new();
        let mut buffer = [0u8; 4096];
        while let Ok(read) = stderr.read(&mut buffer).await {
            if read == 0 {
                break;
            }
            let room = STDERR_BYTES.saturating_sub(kept.len());
            kept.extend_from_slice(&buffer[..read.min(room)]);
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
}

/// The agent's own words for an error entry: the key and the `paths` taken out (what goes to a
/// client names no path), on one line, the last [`DETAIL_CHARS`] characters (where an error
/// usually is).
fn detail(text: &str, key: &Key, paths: &[&Path]) -> String {
    let mut text = match key.expose() {
        "" => text.to_string(),
        key => text.replace(key, "[key]"),
    };
    for path in paths {
        let path = path.to_string_lossy();
        if !path.is_empty() {
            text = text.replace(path.as_ref(), "[…]");
        }
    }
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = flat.chars().count();
    match count > DETAIL_CHARS {
        true => format!("…{}", flat.chars().skip(count - DETAIL_CHARS).collect::<String>()),
        false => flat,
    }
}

fn lock(partial: &Mutex<Partial>) -> std::sync::MutexGuard<'_, Partial> {
    partial.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(all(test, unix))]
mod tests;
