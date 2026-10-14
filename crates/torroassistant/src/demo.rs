//! Sample conversations for `--demo` (and the tests): a small stand-in for the core that answers
//! the requests the window sends, in memory, without a model and without the network.
//!
//! The conversations show every kind of entry: tool lines ("Plan for tomorrow"), a tool waiting
//! for approval ("Block the afternoon"), an answer the model is still writing ("Draft a reply to
//! Anna", frozen half way so screenshots come out the same), a tool the assistant may not use
//! ("Tasks from Anna's mails"), and a stopped answer. A message sent in the demo gets an answer
//! that grows a little each time the window asks, as a model's would. Their ids are fixed
//! ([`TOOLS`], [`APPROVAL`] …), so `torroassistant --demo --chat <id>` opens one of them.

use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Duration, Utc};
use torro_core::ai::AiModel;
use torro_core::i18n::tr;
use torro_ipc::api::{Area, AreaRights, Level, Rights};
use torro_ipc::assistant::{ChatWindow, ConversationSummary, Engine, EngineKind, Entry, EntryContent, Profile};
use torro_ipc::{ApprovalInfo, ApprovalState, ClientInfo, Request, Response};

/// "Plan for tomorrow": messages, tool lines and an answer in Markdown. Pinned.
pub const TOOLS: &str = "d000000000000001";
/// "Block the afternoon": a tool waits for the person's approval.
pub const APPROVAL: &str = "d000000000000002";
/// "Draft a reply to Anna": an answer still being written.
pub const STREAMING: &str = "d000000000000003";
/// "Tasks from Anna's mails": a tool the assistant may not use.
pub const FORBIDDEN: &str = "d000000000000004";

/// The approval "Block the afternoon" waits for.
pub const APPROVAL_ID: &str = "a000000000000001";

/// The demo's local AI account.
pub const OLLAMA: &str = "ai:ollama@localhost:11434";
/// The demo's AI account on OpenRouter.
pub const OPENROUTER: &str = "ai:openrouter@openrouter.ai";

/// How many characters a growing answer gains each time the window asks.
const STEP: usize = 28;

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
    /// The number of the next conversation made in the demo.
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
        Demo { state: Mutex::new(State { setup, now, conversations, approvals, next: 100 }) }
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
                Some(models) if state.setup.engines => Response::AiModels { models },
                _ => refused("not-found", "no such AI account"),
            },
            Request::ListClients => Response::Clients { clients: state.client().into_iter().collect() },
            Request::ListConversations => Response::Conversations { conversations: state.summaries() },
            Request::GetConversation { id, after } => state.get(&id, after.unwrap_or(0)),
            Request::SendMessage { conversation, engine, model, text, without } => {
                state.send(conversation, engine, model, text, without)
            }
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
            // The assistant's profile (task C.10): a name and a symbol of its own.
            Request::GetProfile => Response::Profile { profile: profile() },
            other => refused("unsupported", &format!("{} is not in the demo", other.operation().name())),
        }
    }
}

impl State {
    fn engines(&self) -> Vec<Engine> {
        if !self.setup.engines {
            return Vec::new();
        }
        let engine =
            |id: &str, name: &str| Engine { id: id.into(), kind: EngineKind::Ai, name: name.into(), problem: None };
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

    fn summaries(&self) -> Vec<ConversationSummary> {
        let mut list: Vec<ConversationSummary> = self.conversations.iter().map(|c| c.summary.clone()).collect();
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
        Response::Conversation { conversation: conversation.summary.clone(), entries }
    }

    fn send(
        &mut self,
        conversation: Option<String>,
        engine: String,
        model: Option<String>,
        text: String,
        without: Vec<Area>,
    ) -> Response {
        let now = self.now;
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
                    created: now,
                    updated: now,
                    pinned: false,
                    running: false,
                    last: 0,
                };
                self.conversations.push(Conversation { summary, entries: Vec::new(), growing: None });
                id
            }
        };
        let Some(conversation) = self.find(&id) else { return unknown(&id) };
        if conversation.summary.running {
            return refused("busy", "the assistant still answers in this conversation; stop it or wait");
        }
        let message = conversation.push(now, EntryContent::Message { text, engine, model, without });
        conversation.summary.running = true;
        conversation.growing = Some(Growing { text: tr(REPLY).to_owned(), shown: 0, frozen: false });
        Response::Conversation { conversation: conversation.summary.clone(), entries: vec![message] }
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
            let content =
                EntryContent::Tool { name: "calendar.save_event".into(), input, summary: summary.into(), error };
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
        without: Vec::new(),
    }
}

fn tool(name: &str, input: serde_json::Value, summary: &str, error: Option<&str>) -> EntryContent {
    EntryContent::Tool { name: name.into(), input, summary: summary.into(), error: error.map(str::to_owned) }
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
        created: updated - Duration::minutes(3),
        updated,
        pinned: false,
        running: false,
        last: entries.len() as u64,
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
            tool(
                "tasks.save_todo",
                serde_json::json!({"title": "Answer Anna"}),
                "forbidden: the assistant has no access to tasks",
                Some("forbidden"),
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
        "d000000000000006",
        tr("Release checklist"),
        now,
        Duration::days(3),
        vec![
            message(tr("What is still open for the release?")),
            tool("tasks.list_todos", serde_json::json!({"list": "release"}), tr("6 tasks"), None),
            answer(tr("Six tasks are open; two are due this week.")),
        ],
    );
    release.summary.pinned = true;
    vec![plan, block, draft, forbidden, trip, release]
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
        ChatWindow::Bot(_) => return refused("unsupported", "there are no bots yet"),
    };
    #[cfg(not(test))]
    {
        let started = std::env::current_exe().and_then(|program| {
            std::process::Command::new(program)
                .args(["--demo", "--chat", &target])
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
    let _ = target;
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
            text: "Hello".into(),
            without: Vec::new(),
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
