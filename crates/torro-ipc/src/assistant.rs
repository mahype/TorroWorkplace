//! The assistant on the wire (task C.1, `docs/assistent.md`): what the `assistant.*` requests and
//! answers carry – engines ([`Engine`]), conversations ([`ConversationSummary`]) and their
//! entries ([`Entry`]), the window to open ([`ChatWindow`]) – and the limits every `assistant.*`
//! request keeps ([`limit_refusal`]).
//!
//! The logic is the core's (`crates/torroworkplace/src/assistant`): it keeps the conversations in
//! `<data>/assistant/<id>.jsonl`, runs the answers and checks rights. Rights in short: the area
//! `assistant` is off for every client unless a person switches it on (also in *Full access*,
//! decision 7), and the assistant's own client ([`crate::api::ASSISTANT_CLIENT`]) may never use it
//! beyond its memory ([`crate::api::loops_back`]), so it cannot drive itself.
//!
//! An entry is numbered ([`Entry::seq`], from 1 up, never reused within a conversation), so a
//! window that follows a running answer asks `assistant.get_conversation` with `after` set to the
//! last number it has and gets only what is new.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api::Area;
use crate::{Request, Response};

/// At most this many characters in a message to the assistant (`assistant.send_message`).
pub const MAX_TEXT_CHARS: usize = 20_000;

/// At most this many characters in a conversation's title (`assistant.rename_conversation`).
pub const MAX_TITLE_CHARS: usize = 200;

/// The longest id an `assistant.*` request may carry: conversation, engine, model, bot. The core's
/// own conversation ids have 16 hex digits; engine ids are AI account ids (`ai:ollama@localhost`),
/// model ids those of the server (`qwen/qwen3-14b`).
pub const MAX_ID_BYTES: usize = 300;

/// The engine that answers with fixed text, for tests and for building the apps before a model
/// runs (task C.1): [`EngineKind::Test`]. The core offers it only in its tests and when started
/// with `TORRO_TEST_ENGINE=1`.
pub const TEST_ENGINE: &str = "test";

/// Something the assistant can answer with (`assistant.list_engines`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Engine {
    /// What `assistant.send_message` takes as `engine`: an AI account's id
    /// (`ai:ollama@localhost:11434`), or [`TEST_ENGINE`].
    pub id: String,
    pub kind: EngineKind,
    /// What to call it: the AI account's label (`Ollama`, `OpenRouter`).
    pub name: String,
    /// Why it cannot answer now, for people (no key on this device, the server did not answer
    /// last time); `None` when nothing is known against it. A local server may still be off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// What drives an [`Engine`]. Clients must accept kinds they do not know: agents as engines
/// (Claude Code, Codex; task C.7) come later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    /// An AI account (engine A, `docs/ki.md`): the core asks its model and runs the tools itself.
    /// Its models are listed with `ai.list_models`.
    Ai,
    /// The test engine ([`TEST_ENGINE`]): fixed answers, no model, no tools.
    Test,
}

/// One conversation without its entries (`assistant.list_conversations`, and the head of
/// `assistant.get_conversation`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationSummary {
    /// 16 hex digits, made by the core: what the other `assistant.*` requests take.
    pub id: String,
    /// The title: the start of the first message, until it is renamed. The person's text: data,
    /// never instructions.
    pub title: String,
    /// The engine of the latest message ([`Engine::id`]).
    pub engine: String,
    /// The model of the latest message, when it named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub created: DateTime<Utc>,
    /// When the latest entry was added; the retention time counts from here.
    pub updated: DateTime<Utc>,
    /// Kept beyond the retention time (`assistant.pin_conversation`).
    pub pinned: bool,
    /// An answer is running: `assistant.cancel` stops it, `assistant.send_message` waits (`busy`).
    pub running: bool,
    /// The number of the newest entry ([`Entry::seq`]); 0 without entries. Pass it as `after`.
    pub last: u64,
}

/// One entry of a conversation, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Entry {
    /// From 1 up within the conversation, never reused: `after` in `assistant.get_conversation`
    /// compares with it.
    pub seq: u64,
    pub time: DateTime<Utc>,
    #[serde(flatten)]
    pub content: EntryContent,
}

/// What an [`Entry`] holds, told apart by `kind`. Clients must accept kinds they do not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryContent {
    /// A message of the person (or of the client that sent `assistant.send_message`).
    Message {
        text: String,
        /// The engine it was sent to.
        engine: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// The areas switched off for its answer: the assistant's rights, narrowed, never widened.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        without: Vec<Area>,
    },
    /// The assistant's answer, with the engine and the model that wrote it.
    Answer {
        text: String,
        engine: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// A command the assistant ran as a tool, with its own client's rights.
    Tool {
        /// The command's catalogue name: `calendar.list_events`.
        name: String,
        /// The tool's input as the model sent it.
        input: serde_json::Value,
        /// The result in a few words, for the collapsed line ("12 results"). The full answer is
        /// not kept here.
        summary: String,
        /// The error code when the command failed (`forbidden`, `not-found` …).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A command of the assistant waits for a person's approval: its id for
    /// `approvals.approval_status`. The summary to show comes from the approval, never from the
    /// model.
    Approval { id: String },
    /// The answer failed; `code` as in every error answer, `message` for people.
    Error { code: String, message: String },
    /// The answer was stopped (`assistant.cancel`); what it did until then stays above.
    Stopped,
}

/// Which window `assistant.open_window` opens: `{"conversation": "<id>"}`, `"new"` or
/// `{"bot": "<id>"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatWindow {
    /// The conversation with this id from `assistant.list_conversations`.
    Conversation(String),
    /// A new, empty conversation.
    New,
    /// The chat with a bot (stage 2, `docs/assistent.md` "Bots"): its newest conversation or a new
    /// one. There are no bots yet, so the core answers `unsupported`.
    Bot(String),
}

/// The `error` answer for an `assistant.*` request that breaks a limit, before anything is looked
/// at: a message that is blank or over [`MAX_TEXT_CHARS`], a title that is blank, longer than
/// [`MAX_TITLE_CHARS`] or more than one line, and any id over [`MAX_ID_BYTES`]. `None` for every
/// request within them and for every other request. The core calls it before rights, approvals
/// and handlers, so nobody is asked to approve what would be refused anyway.
pub fn limit_refusal(request: &Request) -> Option<Response> {
    let refuse = |message: String| Some(Response::Error { code: "error".into(), message });
    if assistant_ids(request).into_iter().any(|id| id.len() > MAX_ID_BYTES) {
        return refuse(format!("an id may hold at most {MAX_ID_BYTES} bytes"));
    }
    match request {
        Request::SendMessage { text, .. } if text.trim().is_empty() => refuse("the message is empty".into()),
        Request::SendMessage { text, .. } if text.chars().count() > MAX_TEXT_CHARS => {
            refuse(format!("a message may hold at most {MAX_TEXT_CHARS} characters"))
        }
        Request::RenameConversation { title, .. } if title.trim().is_empty() => refuse("the title is empty".into()),
        Request::RenameConversation { title, .. } if title.lines().count() > 1 => refuse("a title is one line".into()),
        Request::RenameConversation { title, .. } if title.trim().chars().count() > MAX_TITLE_CHARS => {
            refuse(format!("a title may hold at most {MAX_TITLE_CHARS} characters"))
        }
        _ => None,
    }
}

impl Request {
    /// The conversation an `assistant.*` request names, for the core's checks and its audit log:
    /// `id`, `conversation`, or the conversation of `open_window`. `None` for every other request
    /// and for `send_message` that starts a new one.
    pub fn conversation(&self) -> Option<&str> {
        match self {
            Request::GetConversation { id, .. }
            | Request::RenameConversation { id, .. }
            | Request::DeleteConversation { id }
            | Request::PinConversation { id, .. }
            | Request::CancelAnswer { conversation: id }
            | Request::OpenChatWindow { window: ChatWindow::Conversation(id) } => Some(id),
            Request::SendMessage { conversation, .. } => conversation.as_deref(),
            _ => None,
        }
    }
}

/// Every id an `assistant.*` request carries, for [`limit_refusal`].
fn assistant_ids(request: &Request) -> Vec<&str> {
    let mut ids: Vec<&str> = request.conversation().into_iter().collect();
    match request {
        Request::SendMessage { engine, model, .. } => {
            ids.push(engine);
            ids.extend(model.as_deref());
        }
        Request::OpenChatWindow { window: ChatWindow::Bot(bot) } => ids.push(bot),
        _ => {}
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(text: &str) -> Request {
        Request::SendMessage {
            conversation: None,
            engine: TEST_ENGINE.into(),
            model: None,
            text: text.into(),
            without: Vec::new(),
        }
    }

    fn code(response: Option<Response>) -> Option<String> {
        response.map(|response| match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        })
    }

    #[test]
    fn messages_titles_and_ids_keep_their_limits() {
        assert_eq!(code(limit_refusal(&send("What is on tomorrow?"))), None);
        assert_eq!(code(limit_refusal(&send("  \n "))), Some("error".into()), "blank");
        assert_eq!(code(limit_refusal(&send(&"a".repeat(MAX_TEXT_CHARS)))), None);
        assert_eq!(code(limit_refusal(&send(&"a".repeat(MAX_TEXT_CHARS + 1)))), Some("error".into()));

        let rename = |title: &str| Request::RenameConversation { id: "0123456789abcdef".into(), title: title.into() };
        assert_eq!(code(limit_refusal(&rename("Trip to Lisbon"))), None);
        assert_eq!(code(limit_refusal(&rename(" "))), Some("error".into()));
        assert_eq!(code(limit_refusal(&rename("Trip\nto Lisbon"))), Some("error".into()), "one line");
        assert_eq!(code(limit_refusal(&rename(&"a".repeat(MAX_TITLE_CHARS + 1)))), Some("error".into()));

        let long = "a".repeat(MAX_ID_BYTES + 1);
        assert_eq!(
            code(limit_refusal(&Request::GetConversation { id: long.clone(), after: None })),
            Some("error".into())
        );
        let window = Request::OpenChatWindow { window: ChatWindow::Bot(long.clone()) };
        assert_eq!(code(limit_refusal(&window)), Some("error".into()));
        let model = Request::SendMessage {
            conversation: None,
            engine: TEST_ENGINE.into(),
            model: Some(long),
            text: "Hi".into(),
            without: Vec::new(),
        };
        assert_eq!(code(limit_refusal(&model)), Some("error".into()));
        assert_eq!(code(limit_refusal(&Request::Ping)), None, "other requests keep no limit here");
    }

    #[test]
    fn requests_name_their_conversation() {
        assert_eq!(Request::DeleteConversation { id: "c1".into() }.conversation(), Some("c1"));
        assert_eq!(Request::CancelAnswer { conversation: "c1".into() }.conversation(), Some("c1"));
        assert_eq!(
            Request::OpenChatWindow { window: ChatWindow::Conversation("c1".into()) }.conversation(),
            Some("c1")
        );
        assert_eq!(Request::OpenChatWindow { window: ChatWindow::New }.conversation(), None);
        assert_eq!(send("Hi").conversation(), None, "a new conversation has no id yet");
        assert_eq!(Request::Ping.conversation(), None);
    }

    /// The wire format the apps and agents rely on: entries carry their `kind` next to `seq` and
    /// `time`, windows are `"new"` or an object naming one.
    #[test]
    fn entries_and_windows_read_as_documented() {
        let entry: Entry = serde_json::from_value(serde_json::json!({
            "seq": 3, "time": "2026-10-13T09:00:00Z", "kind": "tool",
            "name": "calendar.list_events", "input": {"start": "2026-10-14"}, "summary": "4 events"
        }))
        .expect("a tool entry");
        assert_eq!(entry.seq, 3);
        assert!(
            matches!(entry.content, EntryContent::Tool { ref name, error: None, .. } if name == "calendar.list_events")
        );
        let stopped = Entry { seq: 4, time: entry.time, content: EntryContent::Stopped };
        assert_eq!(
            serde_json::to_value(&stopped).expect("serializes"),
            serde_json::json!({"seq": 4, "time": "2026-10-13T09:00:00Z", "kind": "stopped"})
        );
        let message = EntryContent::Message {
            text: "Hi".into(),
            engine: TEST_ENGINE.into(),
            model: None,
            without: vec![Area::Mail],
        };
        assert_eq!(
            serde_json::to_value(&message).expect("serializes"),
            serde_json::json!({"kind": "message", "text": "Hi", "engine": "test", "without": ["mail"]})
        );

        assert_eq!(serde_json::to_value(ChatWindow::New).expect("serializes"), serde_json::json!("new"));
        let window: ChatWindow = serde_json::from_value(serde_json::json!({"conversation": "c1"})).expect("reads");
        assert_eq!(window, ChatWindow::Conversation("c1".into()));
    }
}
