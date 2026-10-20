//! Sample conversations for `--demo` (and the tests): a small stand-in for the core that answers
//! the requests the window sends, in memory, without a model and without the network.
//!
//! The conversations show every kind of entry: tool lines ("Plan for tomorrow"), a tool waiting
//! for approval ("Block the afternoon"), an answer the model is still writing ("Draft a reply to
//! Anna", frozen half way so screenshots come out the same), a tool the assistant may not use
//! ("Tasks from Anna's mails"), and a stopped answer. A message sent in the demo gets an answer
//! that grows a little each time the window asks, as a model's would. Their ids are fixed
//! ([`TOOLS`], [`APPROVAL`] …), so `torroassistant --demo --chat <id>` opens one of them.
//!
//! Bots (task B.3): "Travel planner" ([`TRAVEL`]) with a conversation of its own ([`BOT_CHAT`]),
//! which the assistant asked in "Weekend in Porto" ([`ASKED`]), and "Release helper"; a
//! conversation of a bot that was deleted ([`GONE`]). They are made, changed and deleted in memory,
//! so `torroassistant --demo --bot b0000001` opens the travel planner's chat. Each bot has a client
//! with rights (task B.5): the travel planner reads two of the three calendars and one of the two
//! mail accounts, the release helper has none, as the assistant's proposal would leave it; the
//! demo lists calendars, task lists, inbox sources and accounts for the editor's section "Access"
//! and takes `clients.update_client`.
//!
//! Cards (task WZ.8): "Release checklist" ([`CARDS`]) made two PDFs; the first is gone ("No longer
//! available"), the second ([`PDF`]) shows with its buttons, which the demo answers without doing
//! anything (`torroassistant --demo --chat d000000000000006`).

use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Duration, Utc};
use torro_core::ai::{AiModel, Effort};
use torro_core::i18n::{tr, trf};
use torro_inbox::{SourceInfo, SourceRef};
use torro_ipc::api::{Access, Area, AreaRights, BOT_CLIENT, Lack, Level, Rights, Selection};
use torro_ipc::assistant::{
    Card, CardEntry, CardRef, ChatWindow, ConversationSummary, Engine, EngineKind, Entry, EntryContent, LETTER_AVATAR,
    Profile,
};
use torro_ipc::bots::{Bot, Skill};
use torro_ipc::{AccountInfo, ApprovalInfo, ApprovalState, ClientInfo, Request, Response};
use torro_pim::{AccountState, Collection, CollectionKind};

/// "Plan for tomorrow": messages, tool lines and an answer in Markdown. Pinned.
pub const TOOLS: &str = "d000000000000001";
/// "Block the afternoon": a tool waits for the person's approval.
pub const APPROVAL: &str = "d000000000000002";
/// "Draft a reply to Anna": an answer still being written.
pub const STREAMING: &str = "d000000000000003";
/// "Tasks from Anna's mails": a tool the assistant may not use.
pub const FORBIDDEN: &str = "d000000000000004";

/// "Release checklist": two PDFs, the first gone, the second ([`PDF`]) with its card (task WZ.8).
pub const CARDS: &str = "d000000000000006";
/// The PDF of "Release checklist" that is still there.
pub const PDF: &str = "5eedf11e";
/// The PDF of "Release checklist" that was deleted.
const GONE_PDF: &str = "dead0001";

/// "Train to Lisbon": the conversation of the bot "Travel planner".
pub const BOT_CHAT: &str = "d000000000000007";
/// "Weekend in Porto": the assistant asked the travel planner.
pub const ASKED: &str = "d000000000000008";
/// "Train or plane to Vienna?": the assistant proposes the travel planner and waits for the person.
pub const PROPOSAL: &str = "d00000000000000a";
/// "Release notes": a conversation of a bot that was deleted.
pub const GONE: &str = "d000000000000009";

/// The bot "Travel planner".
pub const TRAVEL: &str = "b0000001";
/// The bot "Release helper".
pub const RELEASE: &str = "b0000002";
/// The bot of [`GONE`], which is no more.
const GONE_BOT: &str = "b00000ff";

/// The approval "Block the afternoon" waits for.
pub const APPROVAL_ID: &str = "a000000000000001";

/// The demo's local AI account.
pub const OLLAMA: &str = "ai:ollama@localhost:11434";
/// The demo's AI account on OpenRouter.
pub const OPENROUTER: &str = "ai:openrouter@openrouter.ai";

/// How many characters a growing answer gains each time the window asks.
const STEP: usize = 12;

/// What the demo starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    /// The client "Assistant" exists (else the window shows the first start).
    pub client: bool,
    /// There are AI accounts to answer with.
    pub engines: bool,
    /// The sample conversations are there (else none at all).
    pub conversations: bool,
}

impl Default for Setup {
    fn default() -> Setup {
        Setup { client: true, engines: true, conversations: true }
    }
}

/// The demo core: its conversations and approvals behind a lock, since the window's tasks share it.
#[derive(Debug)]
pub struct Demo {
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    setup: Setup,
    now: DateTime<Utc>,
    conversations: Vec<Conversation>,
    approvals: Vec<ApprovalInfo>,
    /// The bots, by name.
    bots: Vec<Bot>,
    /// The clients of the bots, with their rights.
    clients: Vec<ClientInfo>,
    /// The number of the next conversation (and bot) made in the demo.
    next: u64,
}

#[derive(Debug, Clone)]
struct Conversation {
    summary: ConversationSummary,
    entries: Vec<Entry>,
    growing: Option<Growing>,
}

/// An answer being written: its whole text and how much of it shows.
#[derive(Debug, Clone)]
struct Growing {
    text: String,
    shown: usize,
    /// It never grows (the sample for screenshots).
    frozen: bool,
}

impl Demo {
    /// The demo as it starts at `now`.
    pub fn new(now: DateTime<Utc>, setup: Setup) -> Demo {
        let conversations = if setup.conversations { samples(now) } else { Vec::new() };
        let approvals = if setup.conversations { vec![approval(now)] } else { Vec::new() };
        let bots = if setup.conversations { sample_bots() } else { Vec::new() };
        let clients = if setup.conversations { bot_clients(now) } else { Vec::new() };
        Demo { state: Mutex::new(State { setup, now, conversations, approvals, bots, clients, next: 100 }) }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The answer to `request`, as the core would give it (in short).
    pub fn answer(&self, request: Request) -> Response {
        let mut state = self.lock();
        match request {
            Request::ListEngines => Response::Engines { engines: state.engines() },
            Request::AiModels { account, .. } => match models(&account) {
                Some(models) if state.setup.engines => Response::AiModels { account, models },
                _ => refused("not-found", "no such AI account"),
            },
            Request::ListClients => {
                Response::Clients { clients: state.client().into_iter().chain(state.clients.iter().cloned()).collect() }
            }
            // A bot's rights (task B.5), as the core keeps them: never more than its kind may hold.
            Request::UpdateClient { id, rights } => match state.clients.iter_mut().find(|client| client.id == id) {
                Some(client) => {
                    client.rights = rights.for_kind(&client.kind);
                    Response::Ok
                }
                None => refused("not-found", &format!("no client {id:?}")),
            },
            // What exists, for the bot editor's section "Access".
            Request::CalendarCollections => Response::Collections { collections: calendars() },
            Request::TaskCollections => Response::Collections { collections: task_lists() },
            Request::ListSources => Response::Sources { sources: sources() },
            Request::Accounts => Response::Accounts { accounts: accounts() },
            Request::ListConversations => Response::Conversations { conversations: state.summaries() },
            Request::GetConversation { id, after } => state.get(&id, after.unwrap_or(0)),
            Request::SendMessage { conversation, engine, model, effort, text, without, mode, bot } => {
                state.send(conversation, (engine, model, effort), text, (without, mode), bot)
            }
            // No default for new chats in the demo: the one used last (issue #131).
            Request::Settings => Response::Settings { settings: torro_core::settings::Settings::default() },
            Request::CancelAnswer { conversation } => state.cancel(&conversation),
            Request::RenameConversation { id, title } => state.change(&id, |c| c.summary.title = title),
            Request::PinConversation { id, pinned } => state.change(&id, |c| c.summary.pinned = pinned),
            Request::DeleteConversation { id } => {
                let before = state.conversations.len();
                state.conversations.retain(|c| c.summary.id != id);
                if state.conversations.len() < before { Response::Ok } else { unknown(&id) }
            }
            Request::OpenChatWindow { window } => open_window(&state, window),
            Request::ApprovalStatus { id } => match state.approvals.iter().find(|a| a.id == id) {
                Some(approval) => Response::Approval { approval: approval.clone() },
                None => refused("not-found", "no such approval"),
            },
            Request::DecideApproval { id, allow } => state.decide(&id, allow),
            Request::OpenApp { .. } => Response::Ok,
            // The cards (task WZ.8): the one PDF that is still there; its buttons change nothing here.
            Request::GetCards { conversation } => {
                let now = state.now;
                match state.find(&conversation) {
                    Some(found) => Response::Cards { cards: cards(&found.entries, now) },
                    None => unknown(&conversation),
                }
            }
            Request::OpenFile { file } | Request::SaveFileAs { file, .. } if file == PDF => Response::Ok,
            Request::OpenFile { .. } | Request::SaveFileAs { .. } => refused("not-found", "No such file."),
            // The assistant's profile (task C.10): a name and a symbol of its own.
            Request::GetProfile => Response::Profile { profile: profile() },
            // Bots (task B.3), in memory.
            Request::ListBots => Response::Bots { bots: state.bots.iter().map(Bot::summary).collect() },
            Request::GetBot { id } => match state.bots.iter().find(|bot| bot.id == id) {
                Some(bot) => Response::Bot { bot: bot.clone() },
                None => no_bot(&id),
            },
            request @ Request::SaveBot { .. } => match torro_ipc::bots::limit_refusal(&request) {
                Some(refused) => refused,
                None => state.save_bot(request),
            },
            // The person's yes to asking a bot (task B.2): the next ask of the assistant may go.
            Request::ConfirmBot { conversation, bot } => {
                if !state.bots.iter().any(|known| known.id == bot) {
                    no_bot(&bot)
                } else if state.find(&conversation).is_none() {
                    unknown(&conversation)
                } else {
                    Response::Ok
                }
            }
            Request::DeleteBot { id } => {
                let before = state.bots.len();
                let client = state.bots.iter().find(|bot| bot.id == id).map(|bot| bot.client.clone());
                state.bots.retain(|bot| bot.id != id);
                state.clients.retain(|known| Some(&known.id) != client.as_ref());
                if state.bots.len() < before { Response::Ok } else { no_bot(&id) }
            }
            other => refused("unsupported", &format!("{} is not in the demo", other.operation().name())),
        }
    }
}

impl State {
    fn engines(&self) -> Vec<Engine> {
        if !self.setup.engines {
            return Vec::new();
        }
        let engine = |id: &str, name: &str| Engine {
            id: id.into(),
            kind: EngineKind::Ai,
            name: name.into(),
            problem: None,
            models: Vec::new(),
        };
        // Only AI accounts: coding agents are not offered any more (task C.19, issue #219), and
        // the demo shows what the product can do.
        vec![engine(OPENROUTER, "OpenRouter"), engine(OLLAMA, "Ollama")]
    }

    fn client(&self) -> Option<ClientInfo> {
        self.setup.client.then(|| {
            let mut rights = Rights::assistant();
            rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
            ClientInfo {
                id: "a55157a7".into(),
                name: "Assistant".into(),
                kind: torro_ipc::api::ASSISTANT_CLIENT.into(),
                created: self.now - Duration::days(7),
                last_used: Some(self.now),
                rights,
                agent: None,
            }
        })
    }

    /// A conversation's head as the core answers it: whether its bot is gone.
    fn head(&self, summary: &ConversationSummary) -> ConversationSummary {
        let mut head = summary.clone();
        head.bot_deleted = head.bot.as_ref().is_some_and(|id| !self.bots.iter().any(|bot| &bot.id == id));
        head
    }

    fn summaries(&self) -> Vec<ConversationSummary> {
        let mut list: Vec<ConversationSummary> = self.conversations.iter().map(|c| self.head(&c.summary)).collect();
        list.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.cmp(&a.updated)));
        list
    }

    fn find(&mut self, id: &str) -> Option<&mut Conversation> {
        self.conversations.iter_mut().find(|c| c.summary.id == id)
    }

    fn get(&mut self, id: &str, after: u64) -> Response {
        let now = self.now;
        let Some(conversation) = self.find(id) else { return unknown(id) };
        conversation.grow(now);
        let mut entries: Vec<Entry> = conversation.entries.iter().filter(|e| e.seq > after).cloned().collect();
        if let Some(growing) = &conversation.growing {
            let text: String = growing.text.chars().take(growing.shown).collect();
            if !text.is_empty() {
                entries.push(entry(conversation.summary.last + 1, now, answer(&text)));
            }
        }
        let summary = conversation.summary.clone();
        Response::Conversation { conversation: self.head(&summary), entries }
    }

    fn send(
        &mut self,
        conversation: Option<String>,
        (engine, model, effort): (String, Option<String>, Option<Effort>),
        text: String,
        (without, mode): (Vec<Area>, torro_ipc::assistant::Mode),
        bot: Option<String>,
    ) -> Response {
        let now = self.now;
        if let Some(id) = bot.as_deref().filter(|id| !self.bots.iter().any(|bot| bot.id == *id)) {
            return no_bot(id);
        }
        let reply = if bot.is_some() { BOT_REPLY } else { REPLY };
        let id = match conversation {
            Some(id) => id,
            None => {
                self.next += 1;
                let id = format!("d{:015x}", self.next);
                let title: String = text.lines().next().unwrap_or_default().chars().take(60).collect();
                let summary = ConversationSummary {
                    id: id.clone(),
                    title,
                    engine: engine.clone(),
                    model: model.clone(),
                    effort,
                    created: now,
                    updated: now,
                    pinned: false,
                    running: false,
                    last: 0,
                    bot: bot.clone(),
                    bot_deleted: false,
                };
                self.conversations.push(Conversation { summary, entries: Vec::new(), growing: None });
                id
            }
        };
        let Some(conversation) = self.find(&id) else { return unknown(&id) };
        if conversation.summary.bot != bot {
            return refused("error", "this conversation belongs to someone else");
        }
        if conversation.summary.running {
            return refused("busy", "the assistant still answers in this conversation; stop it or wait");
        }
        (conversation.summary.engine, conversation.summary.model, conversation.summary.effort) =
            (engine.clone(), model.clone(), effort);
        let message = conversation.push(now, EntryContent::Message { text, engine, model, effort, without, mode });
        conversation.summary.running = true;
        conversation.growing = Some(Growing { text: tr(reply).to_owned(), shown: 0, frozen: false });
        Response::Conversation { conversation: conversation.summary.clone(), entries: vec![message] }
    }

    /// `bots.save_bot`: a new bot with an id and a client of its own, or the bot `id` replaced.
    fn save_bot(&mut self, request: Request) -> Response {
        let Request::SaveBot { id, name, description, avatar, engine, model, personality, instructions, skills } =
            request
        else {
            return refused("error", "not a bot");
        };
        let id = match id {
            Some(id) if self.bots.iter().any(|bot| bot.id == id) => id,
            Some(id) => return no_bot(&id),
            None => {
                self.next += 1;
                format!("b{:07x}", self.next)
            }
        };
        let client =
            self.bots.iter().find(|bot| bot.id == id).map_or_else(|| format!("c{}", &id[1..]), |b| b.client.clone());
        // A new bot's client starts without rights, as the core makes it.
        if !self.clients.iter().any(|known| known.id == client) {
            self.clients.push(bot_client(&client, &name, Rights::none(), self.now));
        }
        let bot = Bot {
            id: id.clone(),
            name,
            description,
            avatar: avatar.unwrap_or_else(|| LETTER_AVATAR.to_owned()),
            engine,
            model,
            personality,
            instructions,
            skills,
            client,
        };
        self.bots.retain(|old| old.id != id);
        self.bots.push(bot.clone());
        self.bots.sort_by_key(|bot| bot.name.to_lowercase());
        Response::Bot { bot }
    }

    fn cancel(&mut self, id: &str) -> Response {
        let now = self.now;
        let Some(conversation) = self.find(id) else { return unknown(id) };
        if !conversation.summary.running {
            return Response::Ok;
        }
        if let Some(growing) = conversation.growing.take() {
            let text: String = growing.text.chars().take(growing.shown).collect();
            if !text.is_empty() {
                conversation.push(now, answer(&text));
            }
        }
        conversation.push(now, EntryContent::Stopped);
        conversation.summary.running = false;
        Response::Ok
    }

    fn change(&mut self, id: &str, change: impl FnOnce(&mut Conversation)) -> Response {
        match self.find(id) {
            Some(conversation) => {
                change(conversation);
                Response::Ok
            }
            None => unknown(id),
        }
    }

    fn decide(&mut self, id: &str, allow: bool) -> Response {
        let now = self.now;
        let Some(approval) = self.approvals.iter_mut().find(|a| a.id == id) else {
            return refused("not-found", "no such approval");
        };
        if !approval.state.is_pending() {
            return refused("conflict", "the approval is no longer open");
        }
        approval.state =
            if allow { ApprovalState::Done { response: Box::new(Response::Ok) } } else { ApprovalState::Denied };
        approval.decided = Some(now);
        let decided = approval.clone();
        if let Some(conversation) = self.find(APPROVAL) {
            let (summary, error, reply) = if allow {
                (tr("Created · Tue 14:00"), None, tr(ALLOWED_REPLY))
            } else {
                (tr("Denied"), Some("denied".to_owned()), tr(DENIED_REPLY))
            };
            let input = serde_json::json!({"title": tr("Focus time"), "start": "2026-10-14T14:00"});
            let (name, summary) = ("calendar.save_event".into(), summary.into());
            let content = EntryContent::Tool { name, input, summary, error, lack: None };
            conversation.push(now, content);
            conversation.growing = Some(Growing { text: reply.to_owned(), shown: 0, frozen: false });
        }
        Response::Approval { approval: decided }
    }
}

impl Conversation {
    /// Stores `content` as the next entry and returns it.
    fn push(&mut self, now: DateTime<Utc>, content: EntryContent) -> Entry {
        self.summary.last += 1;
        self.summary.updated = now;
        let stored = entry(self.summary.last, now, content);
        self.entries.push(stored.clone());
        stored
    }

    /// The answer being written grows a step; written to the end, it is stored and the run ends.
    fn grow(&mut self, now: DateTime<Utc>) {
        let Some(growing) = self.growing.as_mut().filter(|growing| !growing.frozen) else { return };
        growing.shown += STEP;
        if growing.shown >= growing.text.chars().count() {
            let text = growing.text.clone();
            self.growing = None;
            self.push(now, answer(&text));
            self.summary.running = false;
        }
    }
}

/// The answer of a bot to a message sent in the demo.
const BOT_REPLY: &str =
    "This is the demo, so I planned nothing. With an AI account I would answer with what you allowed me.";

/// The answer to a message sent in the demo.
pub(crate) const REPLY: &str = "This is the demo, so nothing was looked up. With an AI account the assistant reads your \
                     calendar, tasks, inbox and mail – as far as you allowed it – and answers here.";
/// The answer after "Allow".
const ALLOWED_REPLY: &str = "Done. Focus time is in your calendar on Tuesday from 14:00 to 18:00.";
/// The answer after "Deny".
const DENIED_REPLY: &str = "Okay, I left your calendar as it is.";

fn entry(seq: u64, time: DateTime<Utc>, content: EntryContent) -> Entry {
    Entry { seq, time, content }
}

fn answer(text: &str) -> EntryContent {
    EntryContent::Answer { text: text.to_owned(), engine: OLLAMA.into(), model: Some("qwen3:14b".into()) }
}

fn message(text: &str) -> EntryContent {
    EntryContent::Message {
        text: text.to_owned(),
        engine: OLLAMA.into(),
        model: Some("qwen3:14b".into()),
        effort: None,
        without: Vec::new(),
        mode: torro_ipc::assistant::Mode::Default,
    }
}

fn tool(name: &str, input: serde_json::Value, summary: &str, error: Option<&str>) -> EntryContent {
    EntryContent::Tool {
        name: name.into(),
        input,
        summary: summary.into(),
        error: error.map(str::to_owned),
        lack: None,
    }
}

/// A tool line the core refused for `lack` (issue #133), with the words the core gives it.
fn refused_tool(name: &str, input: serde_json::Value, summary: String, lack: Lack) -> EntryContent {
    let error = Some("forbidden".to_owned());
    EntryContent::Tool { name: name.into(), input, summary, error, lack: Some(lack) }
}

/// A conversation with `entries` (numbered from 1), last active `ago` before `now`.
fn sample(id: &str, title: &str, now: DateTime<Utc>, ago: Duration, entries: Vec<EntryContent>) -> Conversation {
    let updated = now - ago;
    let entries: Vec<Entry> = entries.into_iter().zip(1..).map(|(content, seq)| entry(seq, updated, content)).collect();
    let summary = ConversationSummary {
        id: id.into(),
        title: title.into(),
        engine: OLLAMA.into(),
        model: Some("qwen3:14b".into()),
        effort: None,
        created: updated - Duration::minutes(3),
        updated,
        pinned: false,
        running: false,
        last: entries.len() as u64,
        bot: None,
        bot_deleted: false,
    };
    Conversation { summary, entries, growing: None }
}

fn samples(now: DateTime<Utc>) -> Vec<Conversation> {
    let mut plan = sample(
        TOOLS,
        tr("Plan for tomorrow"),
        now,
        Duration::minutes(25),
        vec![
            message(tr("What is on tomorrow, and which mails about it are still open?")),
            tool(
                "calendar.list_events",
                serde_json::json!({"start": "2026-10-15", "end": "2026-10-16"}),
                tr("4 events"),
                None,
            ),
            tool("mail.search_messages", serde_json::json!({"query": "unread"}), tr("12 results"), None),
            answer(tr(PLAN_ANSWER)),
        ],
    );
    plan.summary.pinned = true;

    let mut block = sample(
        APPROVAL,
        tr("Block the afternoon"),
        now,
        Duration::minutes(2),
        vec![
            message(tr("Load the template Writing and block my afternoon in the calendar.")),
            tool("workspaces.load_template", serde_json::json!({"id": "writing"}), tr("Writing"), None),
            EntryContent::Approval { id: APPROVAL_ID.into() },
        ],
    );
    block.summary.running = true;

    let mut draft = sample(
        STREAMING,
        tr("Draft a reply to Anna"),
        now,
        Duration::minutes(1),
        vec![
            message(tr("Draft a reply to Anna's offer: we accept, delivery in November.")),
            tool("mail.search_messages", serde_json::json!({"query": "from:anna offer"}), tr("3 results"), None),
            tool("mail.get_message", serde_json::json!({"id": "m-2041"}), tr("Offer from Anna"), None),
        ],
    );
    draft.summary.running = true;
    let text = tr(DRAFT_ANSWER);
    draft.growing = Some(Growing { text: text.to_owned(), shown: text.chars().count() * 3 / 5, frozen: true });

    let forbidden = sample(
        FORBIDDEN,
        tr("Tasks from Anna's mails"),
        now,
        Duration::hours(2),
        vec![
            message(tr("Make a task for every mail from Anna this week.")),
            tool("mail.search_messages", serde_json::json!({"query": "from:anna"}), tr("5 results"), None),
            refused_tool(
                "tasks.save_todo",
                serde_json::json!({"title": "Answer Anna"}),
                trf("Needs write access to {area}", &[("area", &tr("Tasks"))]),
                Lack::Level { area: Area::Tasks, needs: Access::Write },
            ),
            answer(tr(
                "I found five mails from Anna, but I may not create tasks. You can allow it in the control centre.",
            )),
        ],
    );

    let trip = sample(
        "d000000000000005",
        tr("Trip to Lisbon"),
        now,
        Duration::days(2),
        vec![
            message(tr("Which evenings are free during the trip to Lisbon?")),
            tool(
                "calendar.list_events",
                serde_json::json!({"start": "2026-11-02", "end": "2026-11-06"}),
                tr("7 events"),
                None,
            ),
            answer(tr("Monday and Wednesday evening are free.")),
            EntryContent::Stopped,
        ],
    );

    let mut release = sample(
        CARDS,
        tr("Release checklist"),
        now,
        Duration::days(3),
        vec![
            message(tr("What is still open for the release?")),
            tool("tasks.list_todos", serde_json::json!({"list": "release"}), tr("6 tasks"), None),
            answer(tr("Six tasks are open; two are due this week.")),
            message(tr("Put them in a PDF for the team.")),
            tool("tools.create_pdf", serde_json::json!({"title": "Release checklist"}), "Release checklist.pdf", None),
            EntryContent::Card { card: CardRef::File { id: GONE_PDF.into() } },
            answer(tr("Done: the PDF is on your desktop, Release checklist.pdf.")),
            message(tr("Add the due dates and make a new one.")),
            tool(
                "tools.create_pdf",
                serde_json::json!({"title": "Release checklist", "template": "report"}),
                "Release checklist-2.pdf",
                None,
            ),
            EntryContent::Card { card: CardRef::File { id: PDF.into() } },
            answer(tr("The new one with the due dates is on your desktop: Release checklist-2.pdf.")),
        ],
    );
    release.summary.pinned = true;

    // A bot's own conversation, the assistant asking that bot, and a conversation of a bot that went.
    let mut lisbon = sample(
        BOT_CHAT,
        tr("Train to Lisbon"),
        now,
        Duration::hours(4),
        vec![message(tr("Which trains get me from Hamburg to Lisbon in two days?")), answer(tr(LISBON_ANSWER))],
    );
    lisbon.summary.bot = Some(TRAVEL.into());
    let porto = sample(
        ASKED,
        tr("Weekend in Porto"),
        now,
        Duration::hours(5),
        vec![
            message(tr("Ask the travel planner for a weekend in Porto in November.")),
            tool(
                "bots.ask_bot",
                serde_json::json!({"bot": TRAVEL, "text": "A weekend in Porto in November", "conversation": BOT_CHAT}),
                &trf("Asked {bot} · {steps} steps", &[("bot", &tr("Travel planner")), ("steps", &3)]),
                None,
            ),
            answer(tr("The travel planner suggests the night train to Madrid and on to Porto on Friday.")),
        ],
    );
    let mut gone = sample(
        GONE,
        tr("Release notes"),
        now,
        Duration::days(5),
        vec![message(tr("Write the release notes for 0.9.")), answer(tr("Here are the notes for 0.9."))],
    );
    gone.summary.bot = Some(GONE_BOT.into());
    let vienna = sample(
        PROPOSAL,
        tr("Train or plane to Vienna?"),
        now,
        Duration::hours(6),
        vec![
            message(tr("Should I take the train or the plane to Vienna next week?")),
            tool(
                "bots.ask_bot",
                serde_json::json!({"bot": TRAVEL, "text": "Train or plane from Hamburg to Vienna next week?"}),
                tr("Not allowed for the assistant"),
                Some("forbidden"),
            ),
            answer(tr("Your travel planner knows the night trains well. Shall I ask Travel planner?")),
        ],
    );
    vec![plan, block, draft, forbidden, trip, release, lisbon, porto, gone, vienna]
}

/// The cards of `entries` as the core resolves them (task WZ.8): [`PDF`] as a file on the desktop,
/// every other file gone.
fn cards(entries: &[Entry], now: DateTime<Utc>) -> Vec<CardEntry> {
    entries
        .iter()
        .filter_map(|entry| match &entry.content {
            EntryContent::Card { card: CardRef::File { id } } => Some(CardEntry {
                seq: entry.seq,
                card: if id == PDF { Card::File(pdf(now)) } else { Card::Unavailable },
            }),
            _ => None,
        })
        .collect()
}

/// The PDF of "Release checklist" as the register keeps it.
fn pdf(now: DateTime<Utc>) -> torro_ipc::tools::FileInfo {
    torro_ipc::tools::FileInfo {
        id: PDF.into(),
        name: "Release checklist-2.pdf".into(),
        path: "/Users/anna/Desktop/Release checklist-2.pdf".into(),
        title: "Release checklist".into(),
        template: torro_ipc::tools::PdfTemplateId::Report,
        size: 48_213,
        created: now - Duration::days(3),
    }
}

/// The travel planner's answer in "Train to Lisbon".
const LISBON_ANSWER: &str = "Two days work well:\n\n\
- Day 1: Hamburg to Paris, then the night train to Irún\n\
- Day 2: Irún to Lisbon by day\n\n\
Book the night train early; it fills up on Fridays.";

/// The demo's bots: the travel planner with a skill, and the release helper.
fn sample_bots() -> Vec<Bot> {
    vec![
        Bot {
            id: RELEASE.into(),
            name: tr("Release helper").into(),
            description: tr("Writes release notes and checks what is open").into(),
            avatar: "radar".into(),
            engine: OPENROUTER.into(),
            model: Some("qwen/qwen3-32b".into()),
            personality: String::new(),
            instructions: tr("Read the open tasks of the release list first.").into(),
            skills: Vec::new(),
            client: "c0000002".into(),
        },
        Bot {
            id: TRAVEL.into(),
            name: tr("Travel planner").into(),
            description: tr("Plans trips by train").into(),
            avatar: LETTER_AVATAR.into(),
            engine: OLLAMA.into(),
            model: Some("qwen3:14b".into()),
            personality: crate::bots::Template::Calm.text().into(),
            instructions: tr("Suggest trains before flights. Ask for the budget first.").into(),
            skills: vec![Skill {
                name: tr("Packing").into(),
                text: tr("Pack light: one bag, layers and a rain jacket.").into(),
            }],
            client: "c0000001".into(),
        },
    ]
}

/// The demo's Nextcloud account, whose calendars and task lists the bots can be given.
const CLOUD: &str = "nextcloud:anna@cloud.example.org";
/// The demo's two mail accounts.
const MAIL: &str = "mail:anna@mailbox.org";
const WORK_MAIL: &str = "mail:anna@work.example.net";

/// A collection of the demo's Nextcloud account. Its name shows nowhere in the window (the editor
/// counts collections), so it stays untranslated.
fn collection(path: &str, name: &str, kind: CollectionKind, color: &str) -> Collection {
    Collection {
        id: format!("{CLOUD}/{path}"),
        name: name.into(),
        kind,
        color: Some(color.into()),
        writable: true,
        source: None,
    }
}

/// The demo's calendars: Work, Private, Family.
fn calendars() -> Vec<Collection> {
    vec![
        collection("work", "Work", CollectionKind::Events, "#5B8CFF"),
        collection("private", "Private", CollectionKind::Events, "#3FB984"),
        collection("family", "Family", CollectionKind::Events, "#E8A33D"),
    ]
}

/// The demo's task lists.
fn task_lists() -> Vec<Collection> {
    vec![
        collection("todo", "Todo", CollectionKind::Todos, "#D96CA8"),
        collection("errands", "Errands", CollectionKind::Todos, "#A78BFA"),
    ]
}

/// The demo's inbox sources with entries.
fn sources() -> Vec<SourceInfo> {
    vec![SourceInfo { source: SourceRef::script("backup"), open: 1, muted: false, health: None }]
}

/// The demo's accounts: the Nextcloud account and two mail accounts.
fn accounts() -> Vec<AccountInfo> {
    let account = |id: &str, kind: &str, label: &str| AccountInfo {
        id: id.into(),
        kind: kind.into(),
        label: label.into(),
        state: AccountState::Connected {
            server: label.split_once('@').map_or(label, |(_, host)| host).into(),
            login_name: label.into(),
            last_sync: None,
            problem: None,
        },
        collections: Vec::new(),
        scopes: None,
        mail: None,
        ntfy: None,
        services: None,
    };
    vec![
        account(CLOUD, "nextcloud", "anna@cloud.example.org"),
        account(MAIL, "mail", "anna@mailbox.org"),
        account(WORK_MAIL, "mail", "anna@work.example.net"),
    ]
}

/// A bot's client `id` named `name` with `rights`.
fn bot_client(id: &str, name: &str, rights: Rights, now: DateTime<Utc>) -> ClientInfo {
    ClientInfo {
        id: id.into(),
        name: name.into(),
        kind: BOT_CLIENT.into(),
        created: now - Duration::days(3),
        last_used: None,
        rights: rights.for_kind(BOT_CLIENT),
        agent: None,
    }
}

/// The clients of the demo's bots: the travel planner reads two calendars and one mail account
/// and may mute sources (the admin switch of the inbox, which is off), the release helper has no
/// rights at all, as a bot the assistant proposed starts.
fn bot_clients(now: DateTime<Utc>) -> Vec<ClientInfo> {
    let mut travel = Rights::none();
    let read = AreaRights { level: Level::Read, admin: false };
    travel.areas.insert(Area::Calendar, read);
    travel.areas.insert(Area::Mail, read);
    travel.areas.insert(Area::Inbox, AreaRights { level: Level::NoAccess, admin: true });
    travel.calendars = Selection::Only(vec![format!("{CLOUD}/work"), format!("{CLOUD}/family")]);
    travel.mail_accounts = Selection::Only(vec![MAIL.into()]);
    vec![
        bot_client("c0000001", tr("Travel planner"), travel, now),
        bot_client("c0000002", tr("Release helper"), Rights::none(), now),
    ]
}

/// The answer of "Plan for tomorrow", in Markdown.
const PLAN_ANSWER: &str = "Tomorrow you have **four events**:\n\n\
- 09:30 Stand-up\n\
- 11:00 Review API concept with Anna\n\
- 14:00 Focus time\n\
- 16:30 Call with the print shop\n\n\
Two open mails belong to them: Anna's notes for the review and the print shop's offer.";

/// The answer of "Draft a reply to Anna", of which a part shows.
const DRAFT_ANSWER: &str = "Here is a draft:\n\n\
Dear Anna,\n\n\
thank you for your offer. We accept it as it stands; delivery in November suits us well. \
Please send the order confirmation to me.\n\n\
Best regards";

/// The approval of "Block the afternoon", made at `now`.
fn approval(now: DateTime<Utc>) -> ApprovalInfo {
    ApprovalInfo {
        id: APPROVAL_ID.into(),
        client: "a55157a7".into(),
        client_name: "Assistant".into(),
        command: "calendar.save_event".into(),
        summary: tr("Create the event \"Focus time\" on Tue 14 Oct, 14:00–18:00 in Work").into(),
        details: Vec::new(),
        created: now - Duration::minutes(2),
        expires: now + Duration::minutes(8),
        decided: None,
        state: ApprovalState::Pending,
    }
}

/// The models of the demo's AI accounts; `None` for any other id.
fn models(account: &str) -> Option<Vec<AiModel>> {
    let model = |id: &str, name: &str, tools: Option<bool>| AiModel {
        id: id.into(),
        name: name.into(),
        tools,
        open_weights: Some(true),
        context: None,
        reasoning: Some(id.starts_with("qwen3")),
        remote: None,
        decision: None,
    };
    match account {
        OLLAMA => Some(vec![
            model("qwen3:14b", "qwen3:14b", Some(true)),
            model("qwen3:8b", "qwen3:8b", Some(true)),
            model("gemma3:4b", "gemma3:4b", Some(false)),
            model("llama3.3:70b", "llama3.3:70b", Some(true)),
        ]),
        OPENROUTER => Some(vec![
            model("qwen/qwen3-32b", "Qwen3 32B", Some(true)),
            model("mistralai/mistral-small-3.2-24b-instruct", "Mistral Small 3.2 24B", Some(true)),
            model("deepseek/deepseek-r1", "DeepSeek R1", Some(false)),
        ]),
        _ => None,
    }
}

/// `assistant.open_window` in the demo: another demo process with that chat, so "Open in new
/// window" can be tried without a core. Nothing starts in tests.
fn open_window(state: &State, window: ChatWindow) -> Response {
    let target = match window {
        ChatWindow::Conversation(id) if !state.conversations.iter().any(|c| c.summary.id == id) => {
            return unknown(&id);
        }
        ChatWindow::Conversation(id) => id,
        ChatWindow::New => "new".to_owned(),
        ChatWindow::Bot(id) if !state.bots.iter().any(|bot| bot.id == id) => return no_bot(&id),
        ChatWindow::Bot(id) => format!("--bot={id}"),
    };
    // `--chat <id>` or `--bot=<id>` (one word, so both fit the same call).
    let flag = if target.starts_with("--bot=") { None } else { Some("--chat") };
    #[cfg(not(test))]
    {
        let started = std::env::current_exe().and_then(|program| {
            std::process::Command::new(program)
                .arg("--demo")
                .args(flag)
                .arg(&target)
                .stdin(std::process::Stdio::null())
                .spawn()
                .map(|mut child| {
                    std::thread::spawn(move || child.wait());
                })
        });
        if let Err(err) = started {
            return refused("error", &format!("cannot start the window: {err}"));
        }
    }
    #[cfg(test)]
    let _ = (flag, target);
    Response::Ok
}

/// The demo's assistant (task C.10): "Torro" with the sparkles, as a person might set it up.
pub fn profile() -> Profile {
    Profile {
        name: "Torro".into(),
        description: tr("Your assistant for this computer").into(),
        avatar: "sparkles".into(),
        personality: String::new(),
    }
}

fn refused(code: &str, message: &str) -> Response {
    Response::Error { code: code.into(), message: message.into() }
}

fn unknown(id: &str) -> Response {
    refused("not-found", &format!("no conversation {id:?}"))
}

fn no_bot(id: &str) -> Response {
    refused("not-found", &format!("no bot {id:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> Demo {
        Demo::new(Utc::now(), Setup::default())
    }

    fn conversation(response: Response) -> (ConversationSummary, Vec<Entry>) {
        match response {
            Response::Conversation { conversation, entries } => (conversation, entries),
            other => panic!("expected a conversation, got {other:?}"),
        }
    }

    #[test]
    fn a_sent_message_gets_an_answer_that_grows_until_stored() {
        let demo = demo();
        let send = Request::SendMessage {
            conversation: None,
            engine: OLLAMA.into(),
            model: None,
            effort: None,
            text: "Hello".into(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
        };
        let (summary, entries) = conversation(demo.answer(send));
        assert_eq!((summary.last, summary.running, entries.len()), (1, true, 1));
        let ask = || Request::GetConversation { id: summary.id.clone(), after: Some(1) };
        let (_, first) = conversation(demo.answer(ask()));
        assert_eq!(first.iter().map(|entry| entry.seq).collect::<Vec<_>>(), [2], "the growing answer is last + 1");
        let mut rounds = 0;
        loop {
            let (now, _) = conversation(demo.answer(ask()));
            rounds += 1;
            if !now.running {
                assert_eq!(now.last, 2, "stored under its number");
                break;
            }
            assert!(rounds < 100, "it ends");
        }
    }

    #[test]
    fn deciding_the_approval_goes_on_once() {
        let demo = demo();
        let decided = demo.answer(Request::DecideApproval { id: APPROVAL_ID.into(), allow: true });
        assert!(
            matches!(decided, Response::Approval { approval } if matches!(approval.state, ApprovalState::Done { .. }))
        );
        let again = demo.answer(Request::DecideApproval { id: APPROVAL_ID.into(), allow: false });
        assert!(matches!(again, Response::Error { code, .. } if code == "conflict"));
        let (_, entries) = conversation(demo.answer(Request::GetConversation { id: APPROVAL.into(), after: Some(3) }));
        assert!(
            matches!(&entries[0].content, EntryContent::Tool { name, error: None, .. } if name == "calendar.save_event")
        );
    }

    #[test]
    fn the_frozen_answer_stays_until_stopped() {
        let demo = demo();
        let ask = || Request::GetConversation { id: STREAMING.into(), after: Some(3) };
        let (_, first) = conversation(demo.answer(ask()));
        let (_, second) = conversation(demo.answer(ask()));
        assert_eq!(first, second, "screenshots come out the same");
        assert_eq!(demo.answer(Request::CancelAnswer { conversation: STREAMING.into() }), Response::Ok);
        let (summary, entries) = conversation(demo.answer(ask()));
        assert!(!summary.running);
        assert!(matches!(entries.last().map(|entry| &entry.content), Some(EntryContent::Stopped)));
    }
}
