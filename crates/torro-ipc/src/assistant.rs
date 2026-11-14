//! The assistant on the wire (task C.1, `docs/assistent.md`): what the `assistant.*` requests and
//! answers carry – engines ([`Engine`]), conversations ([`ConversationSummary`]) and their
//! entries ([`Entry`]), the window to open ([`ChatWindow`]), its profile ([`Profile`]) and its
//! memory ([`Note`], task C.10), its skills ([`Skill`], issue #703) – and the limits every `assistant.*` request keeps
//! ([`limit_refusal`]).
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
//!
//! While a model writes (task C.2), its text so far comes as one more [`EntryContent::Answer`]
//! numbered [`ConversationSummary::last`] + 1, which `last` does not count yet: a window asks again
//! with `after` = `last` and gets it longer, until it is stored under that number and `last` reaches
//! it. A window keeps one entry per number and replaces it when it comes again.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api::{Access, Area, Confirm, Level, Operation, Outbound, Rights, Selection};
use crate::{Request, Response};

/// At most this many characters in a message to the assistant (`assistant.send_message`).
pub const MAX_TEXT_CHARS: usize = 20_000;

/// At most this many characters in a conversation's title (`assistant.rename_conversation`).
pub const MAX_TITLE_CHARS: usize = 200;

/// The longest id an `assistant.*` request may carry: conversation, engine, model, bot. The core's
/// own conversation ids have 16 hex digits; engine ids are AI account ids (`ai:ollama@localhost`),
/// model ids those of the server (`qwen/qwen3-14b`).
pub const MAX_ID_BYTES: usize = 300;

/// The assistant's name until the person gives it another (`assistant.update_profile`).
pub const DEFAULT_NAME: &str = "Torro";

/// At most this many characters in the assistant's name: one line.
pub const MAX_NAME_CHARS: usize = 40;

/// At most this many characters in the profile's short description: one line.
pub const MAX_DESCRIPTION_CHARS: usize = 200;

/// At most this many characters in the personality ([`Profile::personality`]): enough for tone,
/// manner and a few habits, short enough that the rules after it keep the model's attention.
pub const MAX_PERSONALITY_CHARS: usize = 4_000;

/// At most this many characters in one note of the memory ([`Note::text`]).
pub const MAX_NOTE_CHARS: usize = 500;

/// At most this many notes in the memory; `assistant.remember` beyond it is `error` until the
/// person forgets some.
pub const MAX_NOTES: usize = 200;

/// At most this many characters in the text of one skill ([`Skill::text`]).
pub const MAX_SKILL_CHARS: usize = 8_000;

/// At most this many characters in the name of one skill ([`Skill::name`]): one line.
pub const MAX_SKILL_NAME_CHARS: usize = 40;

/// At most this many characters in the description of one skill ([`Skill::description`]): one line.
pub const MAX_SKILL_DESCRIPTION_CHARS: usize = 200;

/// At most this many skills for the assistant and for each bot (issue #703): only one line of each
/// stands in a prompt, the text is fetched with `assistant.get_skill`.
pub const MAX_SKILLS: usize = 30;

/// The picture that shows the first letter of the assistant's name ([`Profile::avatar`]).
pub const LETTER_AVATAR: &str = "letter";

/// Every picture a profile can have ([`Profile::avatar`]): the letter of its name, or one of these
/// symbols of the suite's icon set. Clients must show a picture they do not know as the letter.
pub const AVATARS: &[&str] = &[LETTER_AVATAR, "sparkles", "sun", "moon", "cloud", "radar", "user"];

/// The engine that answers with fixed text, for tests and for building the apps before a model
/// runs (task C.1): [`EngineKind::Test`]. The core offers it only in its tests and when started
/// with `TORRO_TEST_ENGINE=1`.
pub const TEST_ENGINE: &str = "test";

/// Something the assistant can answer with (`assistant.list_engines`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Engine {
    /// What `assistant.send_message` takes as `engine`: an AI account's id
    /// (`ai:ollama@localhost:11434`) or [`TEST_ENGINE`]. An agent's id (`claude-code`,
    /// [`EngineKind::Agent`]) is not offered any more (issue #219) and answers `not-found`.
    pub id: String,
    pub kind: EngineKind,
    /// What to call it: the AI account's label (`Ollama`, `OpenRouter`), the agent's name
    /// (`Claude Code`).
    pub name: String,
    /// Why it cannot answer now, for people (no key on this device, the server did not answer
    /// last time); `None` when nothing is known against it. A local server may still be off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// The models of an engine whose models are fixed (issue #131): an agent's (`opus`, `sonnet`
    /// … for Claude Code), each with whether it takes an effort. Empty for an AI account, whose
    /// models `ai.list_models` lists, and for the test engine.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<torro_core::ai::AiModel>,
}

/// What drives an [`Engine`]. Clients must accept kinds they do not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    /// An AI account (engine A, `docs/ki.md`): the core asks its model and runs the tools itself.
    /// Its models are listed with `ai.list_models`.
    Ai,
    /// A coding agent installed on this computer (engine B, task C.7): Claude Code. The core runs
    /// it without a terminal, with the suite's MCP server as its only tools and the key of the
    /// assistant's own client; no shell, no files. It picks its model itself unless `model`
    /// in `assistant.send_message` names one of its [`Engine::models`] (`sonnet`).
    ///
    /// **Not offered any more** (task C.18, issue #219): an agent was never set up, only found on
    /// the computer, so nobody could see why it answered and nobody could switch it off; Claude is
    /// an AI account with its own key now ([`EngineKind::Ai`]). The kind stays in the schema, so a
    /// client that stored it keeps reading, and `assistant.list_engines` never returns it.
    Agent,
    /// The test engine ([`TEST_ENGINE`]): fixed answers, no model, no tools.
    Test,
}

/// What the assistant may do in one answer (issue #132, `assistant.send_message`'s `mode`), like the
/// permission modes of coding agents, ordered from the widest to the narrowest: it only ever narrows the rights of its client
/// ([`Mode::narrow`]), never widens them, so an agent may choose it too. The core applies it to every
/// command of the answer, to a key lent for it and to the approvals it asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The rights as stored.
    #[default]
    Default,
    /// Every command that changes something waits for the person's approval
    /// ([`Confirm::OnEveryChange`]); sending mail too.
    Ask,
    /// Only commands that change nothing: every area at most read, no admin, no sending.
    ReadOnly,
}

impl Mode {
    /// Every mode, the narrowest first, as the composer lists them.
    pub const ALL: [Mode; 3] = [Mode::ReadOnly, Mode::Ask, Mode::Default];

    /// The narrower of `self` and `other` (the modes are ordered from the widest to the narrowest):
    /// what a bot runs in when the assistant asked it in one mode and its own run names another.
    pub fn stricter(self, other: Mode) -> Mode {
        self.max(other)
    }

    /// The rights as stored ([`Mode::Default`]); `mode` is then left out on the wire.
    pub fn is_default(&self) -> bool {
        *self == Mode::Default
    }

    /// `rights` in this mode: [`Mode::ReadOnly`] caps every area and every MCP server
    /// ([`Rights::extensions`]) at read, switches admin and sending off; [`Mode::Ask`] asks before
    /// every change; [`Mode::Default`] leaves them. Never more than `rights`.
    pub fn narrow(self, rights: &Rights) -> Rights {
        let mut rights = rights.clone();
        match self {
            Mode::Default => {}
            Mode::Ask => rights.confirm = rights.confirm.max(Confirm::OnEveryChange),
            Mode::ReadOnly => {
                for held in rights.areas.values_mut() {
                    held.level = held.level.min(Level::Read);
                    held.admin = false;
                }
                for level in rights.extensions.values_mut() {
                    *level = (*level).min(Level::Read);
                }
                if let Some(level) = &mut rights.extensions_default {
                    *level = (*level).min(Level::Read);
                }
                rights.outbound = Outbound::Never;
            }
        }
        rights
    }

    /// Whether the mode refuses `operation` whatever the rights and the client's kind say: in
    /// [`Mode::ReadOnly`] everything beyond reading, also the assistant's own memory.
    pub fn refuses(self, operation: Operation) -> bool {
        self == Mode::ReadOnly && operation.access != Access::Read
    }
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
    /// The effort of the latest message, when it named one (issue #131).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<torro_core::ai::Effort>,
    pub created: DateTime<Utc>,
    /// When the latest entry was added; the retention time counts from here.
    pub updated: DateTime<Utc>,
    /// Kept beyond the retention time (`assistant.pin_conversation`).
    pub pinned: bool,
    /// An answer is running: `assistant.cancel` stops it, `assistant.send_message` waits (`busy`).
    pub running: bool,
    /// The number of the newest stored entry ([`Entry::seq`]); 0 without entries. Pass it as
    /// `after`. An answer a model is still writing comes after it and is not counted yet.
    pub last: u64,
    /// The bot this conversation is with (task B.1): its id from `bots.list_bots`; `None` for a
    /// conversation with the assistant. Set when it starts, never changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot: Option<String>,
    /// The conversation's bot ([`ConversationSummary::bot`]) was deleted: it can be read, renamed,
    /// pinned and deleted, but no longer continued.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bot_deleted: bool,
    /// The mail this conversation is about (issue #605, "Chat zu einer Mail" in `docs/assistent.md`):
    /// set when it starts with `about`, never changed. Such a conversation may only read and suggest.
    /// Left out for every other conversation. For a client that may not read that mail account the whole
    /// conversation is not there: left out of `assistant.list_conversations`, `not-found` for every command
    /// that names it, since its entries tell what the mail says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<MailRef>,
}

/// A mail a conversation is about (issue #605): its account and id, as `mail.get_message` takes them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct MailRef {
    /// The mail account: `mail:<address>` from `accounts.list_accounts`.
    pub account: String,
    /// The mail's id (`m:…`) from `mail.search_messages`.
    pub id: String,
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
    /// A message of the person, or of the client that sent `assistant.send_message` or
    /// `bots.ask_bot` (then `from` names it).
    Message {
        text: String,
        /// The engine it was sent to.
        engine: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// How hard the model was asked to think (issue #131); `None`: as the model decides.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<torro_core::ai::Effort>,
        /// The areas switched off for its answer: the assistant's rights, narrowed, never widened.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        without: Vec<Area>,
        /// The permission mode of its answer (issue #132); left out for [`Mode::Default`].
        #[serde(default, skip_serializing_if = "Mode::is_default")]
        mode: Mode,
        /// Who sent it when it was not the person (issue #305): the id of the client (as
        /// `clients.list_clients` lists it) that sent `assistant.send_message` or `bots.ask_bot`,
        /// the assistant's own when it asks a bot. Set by the core, never by the sender. Such a
        /// message is data from outside in every later answer as well: the web asks the person again
        /// before it, a note waits for a person, and no address in it counts as one the person wrote.
        /// Left out for the person's own message (the app sent it) and in entries from before the
        /// field, which read as the person's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
    /// The assistant's answer, with the engine and the model that wrote it. One answer can come in
    /// several entries, between the tools it used. While the model writes, the entry after
    /// [`ConversationSummary::last`] grows (see the module's documentation).
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
        /// Why a `forbidden` command was refused (issue #133), when it was the rights, the
        /// conversation's switch or the engine: the chat says it and offers "Change access" only
        /// where that helps ([`crate::api::Lack::in_rights`]). `None` for every other line and for
        /// the core's fixed refusals.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lack: Option<crate::api::Lack>,
    },
    /// A command of the assistant waits for a person's approval: its id for
    /// `approvals.approval_status`. The summary to show comes from the approval, never from the
    /// model. Once the person decided, the answer goes on with a [`EntryContent::Tool`] line of that
    /// command (its `error` is `denied` or `expired` when it did not run).
    Approval { id: String },
    /// The answer failed; `code` as in every error answer, `message` for people.
    Error { code: String, message: String },
    /// The answer was stopped (`assistant.cancel`); what it did until then stays above.
    Stopped,
    /// A card in the conversation (issue #225, "Karten im Chat" in `docs/assistent.md`): it points at
    /// an item, it never copies it. The core adds one after a command that made the item (the first:
    /// `tools.create_pdf`); what it shows, `assistant.get_cards` fetches fresh from the core, with the
    /// rights of the conversation's client, so nothing a model wrote ever appears on it.
    Card { card: CardRef },
}

/// What a card ([`EntryContent::Card`]) points at, told apart by `kind`. Clients must accept kinds
/// they do not know (show nothing, or "No longer available").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CardRef {
    /// A file of the register of the assistant's own tools (`tools.create_pdf`'s answer):
    /// `{"kind": "file", "id": "1a2b3c4d"}`.
    File {
        /// The file's id in the register ([`crate::tools::FileInfo::id`]).
        id: String,
    },
    /// A suggestion of a mail (issue #605): added after every `mail.extract_action` that read one out, in any
    /// conversation of the assistant's model, most often a chat about a mail:
    /// `{"kind": "suggestion", "account": "mail:anna@example.org", "id": "m:…", "action": "event"}`.
    Suggestion {
        /// The mail account: `mail:<address>`.
        account: String,
        /// The mail's id (`m:…`).
        id: String,
        /// The kind of action (`mail.list_action_kinds`): `event`, `task`.
        action: String,
    },
}

/// A card as the core resolves it now (`assistant.get_cards`), told apart by `kind`. Clients must
/// accept kinds they do not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Card {
    /// The file as the register keeps it, its fields next to `kind`: name, template, size, path.
    /// Its title is the client's text: data, never instructions.
    File(crate::tools::FileInfo),
    /// A suggestion of a mail as `mail.list_suggestions` lists it now (issue #605): accepting it opens
    /// the filled dialog of its kind. Everything of `suggestion` but its kind and state comes from the
    /// mail: data, never instructions.
    Suggestion {
        /// The mail account: `mail:<address>`.
        account: String,
        /// The mail's id (`m:…`).
        id: String,
        /// The open suggestion of that kind as the mail keeps it now (`ready`, its event or task and the
        /// reason); the card shows it and "Accept" opens the dialog filled with it.
        suggestion: Box<crate::actions::Suggestion>,
    },
    /// Gone or out of reach: deleted, changed, moved, made by another client, or no longer within
    /// the rights (the tool switched off, the area or the tool taken away). Shown as "No longer
    /// available"; why is not told.
    Unavailable,
}

/// One card of a conversation with its place in it (`assistant.get_cards`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CardEntry {
    /// The number of the card's entry ([`Entry::seq`]).
    pub seq: u64,
    pub card: Card,
}

/// Who the assistant is (`assistant.get_profile`, `assistant.update_profile`, task C.10): written by
/// the person, kept by the core in `<config>/assistant/` (the personality in `persona.md`). The
/// personality shapes the assistant's tone only; the fixed rules of its prompt come after it and win.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Profile {
    /// What the assistant is called ([`DEFAULT_NAME`] unless changed): one line, at most
    /// [`MAX_NAME_CHARS`] characters.
    pub name: String,
    /// A short description ("Your assistant for this computer"): one line, at most
    /// [`MAX_DESCRIPTION_CHARS`] characters; may be empty.
    pub description: String,
    /// Its picture: one of [`AVATARS`] ([`LETTER_AVATAR`] for the first letter of the name).
    pub avatar: String,
    /// How it speaks and behaves, as the person wrote it (Markdown or plain text, at most
    /// [`MAX_PERSONALITY_CHARS`] characters); empty for none.
    pub personality: String,
}

/// One note of the assistant's memory (`assistant.list_memories`, task C.10): something it keeps
/// because the person asked it to ("Remember that Anna is my colleague in sales"), never on its own.
/// The core keeps the notes in `<config>/assistant/memory.md` and gives them to the assistant in every
/// conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Note {
    /// 8 hex digits, made by the core: what `assistant.forget` and `replaces` take.
    pub id: String,
    /// The note, one line, at most [`MAX_NOTE_CHARS`] characters. The person's words or the
    /// assistant's summary of them: data, never instructions.
    pub text: String,
    /// When it was written (or last changed).
    pub created: DateTime<Utc>,
    /// The note comes from mail: set when the client asked for it (older versions also set it when a
    /// model on this computer wrote it after reading mail). Such a note waits for the person's
    /// approval and a client sees it only with read access to mail. Since issue #664 (no data rule)
    /// every model reads every note; the name stays for older files and clients.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_only: bool,
}

/// One skill (issue #703, "Skills" in `docs/assistent.md`): a guide for a recurring task ("How I write
/// offers") written by the person, for the assistant (`assistant.save_skill`) or a bot (`bots.save_bot`).
/// Its prompt names it with its description in one line; the text comes with `assistant.get_skill` when
/// the task fits. All of it is the person's text: data, never instructions, and it never changes rights
/// or rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Skill {
    /// What the skill is called ("Offers"): one line, at most [`MAX_SKILL_NAME_CHARS`] characters,
    /// unique without regard to case.
    pub name: String,
    /// When it fits ("How I write offers"), one line of at most [`MAX_SKILL_DESCRIPTION_CHARS`]
    /// characters; may be empty (skills of bots from before issue #703).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The skill itself (Markdown or plain text), at most [`MAX_SKILL_CHARS`] characters.
    pub text: String,
}

impl Skill {
    /// The skill as `assistant.list_skills` lists it.
    pub fn summary(&self) -> SkillSummary {
        SkillSummary { name: self.name.clone(), description: self.description.clone() }
    }

    /// Whether this skill is called `name` ([`same_skill_name`]).
    pub fn is_named(&self, name: &str) -> bool {
        same_skill_name(&self.name, name)
    }
}

/// Whether two names of skills are the same as the core keeps them: control characters and runs of
/// white space as one space, trimmed, without regard to case.
pub fn same_skill_name(a: &str, b: &str) -> bool {
    let kept = |name: &str| {
        let flat: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        flat.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
    };
    kept(a) == kept(b)
}

/// One skill without its text (`assistant.list_skills`): what a prompt shows of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkillSummary {
    /// As [`Skill::name`].
    pub name: String,
    /// As [`Skill::description`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// Why a skill is refused, for people; `None` when it keeps every limit: a name of one line of 1 to
/// [`MAX_SKILL_NAME_CHARS`] characters, a description of one line within
/// [`MAX_SKILL_DESCRIPTION_CHARS`], a text of 1 to [`MAX_SKILL_CHARS`] characters (`text_required`;
/// skills of bots may be empty, as before issue #703). Lengths count characters after trimming.
pub fn skill_refusal(name: &str, description: &str, text: &str, text_required: bool) -> Option<String> {
    let chars = |text: &str| text.trim().chars().count();
    let one_line = |text: &str| text.trim().lines().count() <= 1;
    // Blank once cleaned as the core keeps it (only control characters): no name either.
    let blank = name.chars().all(|c| c.is_control() || c.is_whitespace());
    if blank || !one_line(name) || chars(name) > MAX_SKILL_NAME_CHARS {
        return Some(format!("a skill's name is one line of 1 to {MAX_SKILL_NAME_CHARS} characters"));
    }
    if !one_line(description) || chars(description) > MAX_SKILL_DESCRIPTION_CHARS {
        return Some(format!("a skill's description is one line of at most {MAX_SKILL_DESCRIPTION_CHARS} characters"));
    }
    if text_required && text.trim().is_empty() {
        return Some("the skill's text is empty".into());
    }
    if chars(text) > MAX_SKILL_CHARS {
        return Some(format!("a skill may hold at most {MAX_SKILL_CHARS} characters"));
    }
    None
}

/// What a client may do now (`assistant.my_access`, issue #134): its rights as they apply to the
/// request that asks, so the assistant, a bot or an agent can tell the person what it may do, and
/// a test can compare it with what the person set (`clients.list_clients`).
///
/// For the assistant's engines and the bots the core narrows the stored rights the way it checks
/// every other command: without the areas switched off in the conversation
/// ([`MyAccess::switched_off`]), and for a bot that runs for the
/// assistant to what both clients may (`Rights::common`, task B.2). Only the person changes rights,
/// with the app key; nothing here is a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MyAccess {
    /// The rights in force now: level and admin switch per area, the calendars, task lists, GitHub
    /// repositories, inbox sources and mail accounts it sees, sending and approval.
    pub rights: Rights,
    /// The assistant's engine the request came through: an AI account's id
    /// (`ai:ollama@localhost:11434`) or an agent (`claude-code`); `None` for a client's own key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// The areas the person switched off for this conversation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub switched_off: Vec<Area>,
    /// The permission mode of the answer that asks (issue #132); [`MyAccess::rights`] are narrowed by
    /// it already. Left out for [`Mode::Default`].
    #[serde(default, skip_serializing_if = "Mode::is_default")]
    pub mode: Mode,
}

impl MyAccess {
    /// `rights` as they are for a client's own key: nothing switched off.
    pub fn of(rights: Rights, engine: Option<String>) -> MyAccess {
        MyAccess { rights, engine, switched_off: Vec::new(), mode: Mode::Default }
    }

    /// `self` in the permission mode `mode` (issue #132): the rights narrowed ([`Mode::narrow`]) and
    /// the mode named.
    pub fn in_mode(mut self, mode: Mode) -> MyAccess {
        self.rights = mode.narrow(&self.rights);
        self.mode = mode;
        self
    }

    /// `self` with the areas `areas` switched off as well: closed ([`close`]) and listed in
    /// [`MyAccess::switched_off`] once, in the order of the areas. Engine A
    /// checks the conversation's switches itself before a call reaches the core, so it adds them to
    /// the core's answer here.
    pub fn switching_off(mut self, areas: &[Area]) -> MyAccess {
        for area in areas {
            close(&mut self.rights, *area);
        }
        self.switched_off.extend(areas);
        self.switched_off.sort();
        self.switched_off.dedup();
        self
    }
}

/// Takes `area` out of `rights` with what it selects: an area the client does not get here names
/// none of its calendars, lists, address books, repositories, mail accounts, tools or MCP servers either, and
/// without mail no mail account's inbox source.
fn close(rights: &mut Rights, area: Area) {
    rights.areas.remove(&area);
    let nothing = || Selection::Only(Vec::new());
    match area {
        Area::Calendar => rights.calendars = nothing(),
        Area::Tasks => rights.task_lists = nothing(),
        Area::Contacts => {
            rights.address_books = nothing();
            rights.hidden_address_books.clear();
        }
        Area::Github => rights.github_repos = nothing(),
        Area::Inbox => rights.inbox_sources = nothing(),
        Area::Tools => rights.tools = nothing(),
        // Switched off for a conversation, the area takes every server's tools with it: their
        // command is `extensions.call_tool` (issue #302).
        Area::Extensions => rights.close_extensions(),
        Area::Mail => {
            rights.mail_accounts = nothing();
            if let Selection::Only(keys) = &mut rights.inbox_sources {
                keys.retain(|key| !key.starts_with("mail:"));
            }
        }
        // Without the chat no Talk entry of the inbox either (decision 6 in `docs/chat.md`): they
        // hold the names of conversations and the start of messages.
        Area::Chat => {
            rights.chat_accounts = nothing();
            match &mut rights.inbox_sources {
                Selection::Only(keys) => keys.retain(|key| !key.starts_with("talk:")),
                // "All sources" would still reach them: nothing but what is named stays reachable.
                all @ Selection::All => *all = nothing(),
            }
        }
        Area::Presence => rights.presence_accounts = nothing(),
        _ => {}
    }
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
    /// one (`torroassistant --bot <id>`, task B.3).
    Bot(String),
}

/// The `error` answer for an `assistant.*` request that breaks a limit, before anything is looked
/// at: a message that is blank or over [`MAX_TEXT_CHARS`], a title that is blank, longer than
/// [`MAX_TITLE_CHARS`] or more than one line, any id over [`MAX_ID_BYTES`], a note that is blank or
/// over [`MAX_NOTE_CHARS`], a skill beyond its limits ([`skill_refusal`]), and a change of the profile that changes nothing or breaks its limits
/// ([`profile_refusal`]). `None` for every request within them and for every other request. The
/// core calls it before rights, approvals and handlers, so nobody is asked to approve what would be
/// refused anyway.
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
        Request::Remember { text, .. } if text.trim().is_empty() => refuse("the note is empty".into()),
        Request::Remember { text, .. } if text.trim().chars().count() > MAX_NOTE_CHARS => {
            refuse(format!("a note may hold at most {MAX_NOTE_CHARS} characters"))
        }
        Request::SaveSkill { name, description, text, .. } => skill_refusal(name, description, text, true)
            .map(|message| Response::Error { code: "error".into(), message }),
        Request::UpdateProfile { name, description, avatar, personality } => {
            profile_refusal(name.as_deref(), description.as_deref(), avatar.as_deref(), personality.as_deref())
                .map(|message| Response::Error { code: "error".into(), message })
        }
        _ => None,
    }
}

/// Why a change of the profile (`assistant.update_profile`) is refused, for people; `None` when it
/// keeps every limit: at least one field, a name that is one line and not blank, a description of
/// one line, both within their lengths, a picture of [`AVATARS`], a personality within
/// [`MAX_PERSONALITY_CHARS`]. Lengths count characters after trimming.
pub fn profile_refusal(
    name: Option<&str>,
    description: Option<&str>,
    avatar: Option<&str>,
    personality: Option<&str>,
) -> Option<String> {
    if name.is_none() && description.is_none() && avatar.is_none() && personality.is_none() {
        return Some("nothing to change: give a name, description, avatar or personality".into());
    }
    let chars = |text: &str| text.trim().chars().count();
    if let Some(name) = name {
        if name.trim().is_empty() {
            return Some("the name is empty".into());
        }
        if name.trim().lines().count() > 1 || chars(name) > MAX_NAME_CHARS {
            return Some(format!("a name is one line of at most {MAX_NAME_CHARS} characters"));
        }
    }
    if let Some(description) = description
        && (description.trim().lines().count() > 1 || chars(description) > MAX_DESCRIPTION_CHARS)
    {
        return Some(format!("a description is one line of at most {MAX_DESCRIPTION_CHARS} characters"));
    }
    if let Some(avatar) = avatar
        && !AVATARS.contains(&avatar)
    {
        return Some(format!("no avatar {avatar:?}; one of: {}", AVATARS.join(", ")));
    }
    if personality.is_some_and(|personality| chars(personality) > MAX_PERSONALITY_CHARS) {
        return Some(format!("a personality may hold at most {MAX_PERSONALITY_CHARS} characters"));
    }
    None
}

impl Request {
    /// The conversation an `assistant.*` request or `bots.ask_bot` names, for the core's checks and
    /// its audit log: `id`, `conversation` (also of `confirm_bot` and `get_cards`), or the conversation
    /// of `open_window`. `None` for every
    /// other request and for `send_message` and `ask_bot` that start a new one.
    pub fn conversation(&self) -> Option<&str> {
        match self {
            Request::GetConversation { id, .. }
            | Request::GetCards { conversation: id }
            | Request::RenameConversation { id, .. }
            | Request::DeleteConversation { id }
            | Request::PinConversation { id, .. }
            | Request::CancelAnswer { conversation: id }
            | Request::OpenChatWindow { window: ChatWindow::Conversation(id) }
            | Request::ConfirmBot { conversation: id, .. } => Some(id),
            Request::SendMessage { conversation, .. } | Request::AskBot { conversation, .. } => conversation.as_deref(),
            _ => None,
        }
    }
}

/// Every id an `assistant.*` request carries, for [`limit_refusal`].
fn assistant_ids(request: &Request) -> Vec<&str> {
    let mut ids: Vec<&str> = request.conversation().into_iter().collect();
    match request {
        Request::SendMessage { engine, model, bot, about, .. } => {
            ids.push(engine);
            ids.extend(model.as_deref());
            ids.extend(bot.as_deref());
            if let Some(about) = about {
                ids.extend([about.account.as_str(), about.id.as_str()]);
            }
        }
        Request::CheckAbout { engine, model, about } => {
            ids.extend([engine.as_str(), about.account.as_str(), about.id.as_str()]);
            ids.extend(model.as_deref());
        }
        Request::OpenChatWindow { window: ChatWindow::Bot(bot) } | Request::ConfirmBot { bot, .. } => ids.push(bot),
        Request::Forget { id } | Request::Remember { replaces: Some(id), .. } => ids.push(id),
        Request::GetSkill { name } | Request::DeleteSkill { name } => ids.push(name),
        Request::SaveSkill { replaces: Some(name), .. } => ids.push(name),
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
            effort: None,
            text: text.into(),
            without: Vec::new(),
            mode: Mode::Default,
            bot: None,
            about: None,
        }
    }

    fn code(response: Option<Response>) -> Option<String> {
        response.map(|response| match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        })
    }

    /// Issue #132: a mode only narrows: read only caps every area at read without admin or sending,
    /// ask asks before every change, and neither ever opens what the rights keep closed.
    #[test]
    fn a_mode_only_narrows() {
        let mut rights = Rights::assistant();
        rights.areas.insert(Area::Mail, crate::api::AreaRights { level: Level::Full, admin: true });
        rights.areas.insert(Area::Tasks, crate::api::AreaRights { level: Level::Write, admin: false });
        rights.outbound = Outbound::Free;
        rights.confirm = Confirm::Never;
        rights.extensions = [("github".to_string(), Level::Full), ("time".to_string(), Level::Read)].into();
        let read = Mode::ReadOnly.narrow(&rights);
        assert!(read.areas.values().all(|held| held.level <= Level::Read && !held.admin));
        // Issue #302: every MCP server at read at most, none opened.
        assert_eq!(read.extensions, [("github".to_string(), Level::Read), ("time".to_string(), Level::Read)].into());
        assert_eq!(read.outbound, Outbound::Never);
        assert_eq!(read.area(Area::Github), rights.area(Area::Github), "closed stays closed");
        let ask = Mode::Ask.narrow(&rights);
        assert_eq!((ask.confirm, ask.areas.clone()), (Confirm::OnEveryChange, rights.areas.clone()));
        assert_eq!(Mode::Default.narrow(&rights), rights);
        for mode in Mode::ALL {
            let narrowed = mode.narrow(&rights);
            for command in crate::api::CATALOGUE {
                let operation = Operation { command, access: command.access };
                assert!(!narrowed.allows(operation) || rights.allows(operation), "{mode:?} widened {}", command.name);
                // What every client may (asking the person, `approvals.ask_permission`) is no grant; the core
                // refuses it in a read-only answer as well (`Mode::refuses`).
                // A server's tool is no grant of its area either: its level is the server's, and the
                // core refuses it by the tool's access (issue #302).
                let granted =
                    command.area.grantable() && command.name != "extensions.call_tool" && narrowed.allows(operation);
                assert!(!granted || !mode.refuses(operation), "{mode:?} {}", command.name);
            }
        }
        assert_eq!(serde_json::to_value(Mode::ReadOnly).expect("json"), "read_only");
        assert_eq!(serde_json::to_value(Mode::Ask).expect("json"), "ask");
        let access = MyAccess::of(rights.clone(), None).in_mode(Mode::ReadOnly);
        assert_eq!((access.mode, access.rights), (Mode::ReadOnly, read));
    }

    /// Issue #134: an area switched off is closed with what it selects, and listed once.
    #[test]
    fn closed_areas_name_nothing_of_theirs() {
        let mut rights = Rights::assistant();
        rights.areas.insert(Area::Mail, crate::api::AreaRights { level: crate::api::Level::Read, admin: false });
        rights.mail_accounts = Selection::Only(vec!["mail:anna@mailbox.org".into()]);
        rights.inbox_sources = Selection::Only(vec!["mail:anna@mailbox.org".into(), "agent:claude-code".into()]);
        let access =
            MyAccess::of(rights, Some("claude-code".into())).switching_off(&[Area::Tasks, Area::Mail, Area::Tasks]);
        assert_eq!(access.switched_off.as_slice(), &[Area::Tasks, Area::Mail][..]);
        assert!(!access.rights.areas.contains_key(&Area::Mail) && !access.rights.areas.contains_key(&Area::Tasks));
        assert_eq!(access.rights.mail_accounts, Selection::Only(Vec::new()), "no address");
        assert_eq!(access.rights.task_lists, Selection::Only(Vec::new()));
        assert_eq!(access.rights.inbox_sources, Selection::Only(vec!["agent:claude-code".into()]));
        assert_eq!(access.rights.calendars, Selection::All, "the rest stays");

        // Issue #302: `extensions` switched off takes every server's level with it; issue #666: the
        // open level for servers without an entry too.
        let mut servers = Rights::assistant();
        servers.extensions = [("time".to_string(), crate::api::Level::Read)].into();
        let access = MyAccess::of(servers, None).switching_off(&[Area::Extensions]);
        assert!(access.rights.extensions.is_empty());
        assert_eq!(access.rights.extension("new"), crate::api::Level::NoAccess);
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
            model: Some(long.clone()),
            effort: None,
            text: "Hi".into(),
            without: Vec::new(),
            mode: Mode::Default,
            bot: None,
            about: None,
        };
        assert_eq!(code(limit_refusal(&model)), Some("error".into()));
        let bot = Request::SendMessage {
            conversation: None,
            engine: TEST_ENGINE.into(),
            model: None,
            effort: None,
            text: "Hi".into(),
            without: Vec::new(),
            mode: Mode::Default,
            bot: Some(long),
            about: None,
        };
        assert_eq!(code(limit_refusal(&bot)), Some("error".into()));
        // Issue #605: the mail a chat is about keeps the same limit.
        let about = |account: &str, id: &str| Request::SendMessage {
            conversation: None,
            engine: TEST_ENGINE.into(),
            model: None,
            effort: None,
            text: "Hi".into(),
            without: Vec::new(),
            mode: Mode::Default,
            bot: None,
            about: Some(MailRef { account: account.into(), id: id.into() }),
        };
        assert_eq!(code(limit_refusal(&about("mail:anna@example.org", "m:4"))), None);
        assert_eq!(
            code(limit_refusal(&about("mail:anna@example.org", &"m".repeat(MAX_ID_BYTES + 1)))),
            Some("error".into())
        );
        assert_eq!(code(limit_refusal(&about(&"a".repeat(MAX_ID_BYTES + 1), "m:4"))), Some("error".into()));
        // And so does the check before such a chat.
        let check = |engine: &str, id: &str| Request::CheckAbout {
            engine: engine.into(),
            model: None,
            about: MailRef { account: "mail:anna@example.org".into(), id: id.into() },
        };
        assert_eq!(code(limit_refusal(&check(TEST_ENGINE, "m:4"))), None);
        assert_eq!(code(limit_refusal(&check(TEST_ENGINE, &"m".repeat(MAX_ID_BYTES + 1)))), Some("error".into()));
        assert_eq!(code(limit_refusal(&check(&"a".repeat(MAX_ID_BYTES + 1), "m:4"))), Some("error".into()));
        assert_eq!(code(limit_refusal(&Request::Ping)), None, "other requests keep no limit here");
    }

    #[test]
    fn notes_keep_their_limits() {
        let remember =
            |text: &str, replaces: Option<String>| Request::Remember { text: text.into(), replaces, local_only: false };
        assert_eq!(code(limit_refusal(&remember("Anna works in sales.", None))), None);
        assert_eq!(code(limit_refusal(&remember(" \n ", None))), Some("error".into()), "blank");
        assert_eq!(code(limit_refusal(&remember(&"a".repeat(MAX_NOTE_CHARS), None))), None);
        assert_eq!(code(limit_refusal(&remember(&"a".repeat(MAX_NOTE_CHARS + 1), None))), Some("error".into()));
        let long = "a".repeat(MAX_ID_BYTES + 1);
        assert_eq!(code(limit_refusal(&remember("Hi", Some(long.clone())))), Some("error".into()), "a long id");
        assert_eq!(code(limit_refusal(&Request::Forget { id: long })), Some("error".into()));
        assert_eq!(code(limit_refusal(&Request::Forget { id: "1a2b3c4d".into() })), None);
        // Issue #703: a skill keeps the limits of `skill_refusal`, and its text may not be empty.
        let skill = |name: &str, description: &str, text: &str| Request::SaveSkill {
            name: name.into(),
            description: description.into(),
            text: text.into(),
            replaces: None,
        };
        assert_eq!(code(limit_refusal(&skill("Offers", "How I write offers", "1. Greeting"))), None);
        assert_eq!(code(limit_refusal(&skill(" ", "", "x"))), Some("error".into()), "a name");
        assert_eq!(code(limit_refusal(&skill("a\nb", "", "x"))), Some("error".into()), "one line");
        let name = "a".repeat(MAX_SKILL_NAME_CHARS + 1);
        assert_eq!(code(limit_refusal(&skill(&name, "", "x"))), Some("error".into()));
        let description = "a".repeat(MAX_SKILL_DESCRIPTION_CHARS + 1);
        assert_eq!(code(limit_refusal(&skill("Offers", &description, "x"))), Some("error".into()));
        assert_eq!(code(limit_refusal(&skill("Offers", "one\ntwo", "x"))), Some("error".into()));
        assert_eq!(code(limit_refusal(&skill("Offers", "", " "))), Some("error".into()), "a text");
        let text = "a".repeat(MAX_SKILL_CHARS);
        assert_eq!(code(limit_refusal(&skill("Offers", "", &text))), None);
        let text = "a".repeat(MAX_SKILL_CHARS + 1);
        assert_eq!(code(limit_refusal(&skill("Offers", "", &text))), Some("error".into()));
        let long = "a".repeat(MAX_ID_BYTES + 1);
        assert_eq!(code(limit_refusal(&Request::GetSkill { name: long.clone() })), Some("error".into()));
        assert_eq!(code(limit_refusal(&Request::DeleteSkill { name: long })), Some("error".into()));
    }

    #[test]
    fn a_profile_change_keeps_its_limits() {
        let update =
            |name: Option<&str>, description: Option<&str>, avatar: Option<&str>, personality: Option<&str>| {
                Request::UpdateProfile {
                    name: name.map(str::to_string),
                    description: description.map(str::to_string),
                    avatar: avatar.map(str::to_string),
                    personality: personality.map(str::to_string),
                }
            };
        assert_eq!(code(limit_refusal(&update(None, None, None, None))), Some("error".into()), "nothing to change");
        assert_eq!(code(limit_refusal(&update(Some("Jarvis"), Some(""), Some(LETTER_AVATAR), Some("")))), None);
        assert_eq!(code(limit_refusal(&update(Some("  "), None, None, None))), Some("error".into()), "blank name");
        assert_eq!(code(limit_refusal(&update(Some("Jar\nvis"), None, None, None))), Some("error".into()));
        let name = "a".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(code(limit_refusal(&update(Some(&name), None, None, None))), Some("error".into()));
        let description = "a".repeat(MAX_DESCRIPTION_CHARS + 1);
        assert_eq!(code(limit_refusal(&update(None, Some(&description), None, None))), Some("error".into()));
        assert_eq!(code(limit_refusal(&update(None, Some("one\ntwo"), None, None))), Some("error".into()));
        assert_eq!(code(limit_refusal(&update(None, None, Some("bot"), None))), Some("error".into()), "no such avatar");
        for avatar in AVATARS {
            assert_eq!(code(limit_refusal(&update(None, None, Some(avatar), None))), None, "{avatar}");
        }
        let personality = "a".repeat(MAX_PERSONALITY_CHARS);
        assert_eq!(code(limit_refusal(&update(None, None, None, Some(&personality)))), None);
        let personality = "a".repeat(MAX_PERSONALITY_CHARS + 1);
        assert_eq!(code(limit_refusal(&update(None, None, None, Some(&personality)))), Some("error".into()));
    }

    /// The wire format of a note: `local_only` only when set.
    #[test]
    fn notes_read_as_documented() {
        let note: Note = serde_json::from_value(serde_json::json!({
            "id": "1a2b3c4d", "text": "Anna works in sales.", "created": "2026-10-14T09:00:00Z"
        }))
        .expect("a note");
        assert!(!note.local_only);
        assert_eq!(
            serde_json::to_value(Note { local_only: true, ..note.clone() }).expect("serializes")["local_only"],
            serde_json::json!(true)
        );
        assert!(serde_json::to_value(&note).expect("serializes").get("local_only").is_none());
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
            effort: None,
            without: vec![Area::Mail],
            mode: Mode::Default,
            from: None,
        };
        assert_eq!(
            serde_json::to_value(&message).expect("serializes"),
            serde_json::json!({"kind": "message", "text": "Hi", "engine": "test", "without": ["mail"]})
        );
        let thought = EntryContent::Message {
            text: "Hi".into(),
            engine: "claude-code".into(),
            model: Some("opus".into()),
            effort: Some(torro_core::ai::Effort::High),
            without: Vec::new(),
            mode: Mode::Default,
            from: Some("1f2e3d4c".into()),
        };
        assert_eq!(
            serde_json::to_value(&thought).expect("serializes"),
            serde_json::json!({
                "kind": "message", "text": "Hi", "engine": "claude-code", "model": "opus", "effort": "high",
                "from": "1f2e3d4c"
            })
        );
        // A line from before `from` (issue #305) reads as the person's message.
        let before: Entry = serde_json::from_str(
            r#"{"seq":1,"time":"2026-10-13T09:00:00Z","kind":"message","text":"Hi","engine":"test"}"#,
        )
        .expect("an old message line");
        assert!(matches!(before.content, EntryContent::Message { from: None, .. }), "{before:?}");

        assert_eq!(serde_json::to_value(ChatWindow::New).expect("serializes"), serde_json::json!("new"));
        let window: ChatWindow = serde_json::from_value(serde_json::json!({"conversation": "c1"})).expect("reads");
        assert_eq!(window, ChatWindow::Conversation("c1".into()));
    }

    /// The wire format of cards (issue #225): the entry names what it points at, never its content;
    /// a resolved file carries its fields next to `kind`.
    #[test]
    fn cards_read_as_documented() {
        let entry: Entry = serde_json::from_value(serde_json::json!({
            "seq": 5, "time": "2026-10-19T09:00:00Z", "kind": "card", "card": {"kind": "file", "id": "1a2b3c4d"}
        }))
        .expect("a card entry");
        assert_eq!(entry.content, EntryContent::Card { card: CardRef::File { id: "1a2b3c4d".into() } });

        let file = crate::tools::FileInfo {
            id: "1a2b3c4d".into(),
            name: "Report.pdf".into(),
            path: "/Users/anna/Desktop/Report.pdf".into(),
            title: "Report".into(),
            template: crate::tools::PdfTemplateId::Report,
            size: 23_456,
            created: entry.time,
        };
        let shown = CardEntry { seq: 5, card: Card::File(file.clone()) };
        let json = serde_json::to_value(&shown).expect("serializes");
        assert_eq!(
            (&json["seq"], &json["card"]["kind"], &json["card"]["name"]),
            (&5.into(), &"file".into(), &"Report.pdf".into())
        );
        assert_eq!(serde_json::from_value::<CardEntry>(json).expect("reads back"), shown);
        let gone = CardEntry { seq: 7, card: Card::Unavailable };
        assert_eq!(
            serde_json::to_value(&gone).expect("serializes"),
            serde_json::json!({"seq": 7, "card": {"kind": "unavailable"}})
        );
        // Issue #605: a suggestion card names the mail and the kind of action, and shows the suggestion.
        let entry: Entry = serde_json::from_value(serde_json::json!({
            "seq": 6, "time": "2026-10-19T09:00:00Z", "kind": "card",
            "card": {"kind": "suggestion", "account": "mail:anna@example.org", "id": "m:4", "action": "event"}
        }))
        .expect("a suggestion card");
        let pointed =
            CardRef::Suggestion { account: "mail:anna@example.org".into(), id: "m:4".into(), action: "event".into() };
        assert_eq!(entry.content, EntryContent::Card { card: pointed });
        let suggestion = crate::actions::Suggestion {
            kind: "event".into(),
            state: crate::actions::SuggestionState::Ready,
            task: None,
            event: None,
            reason: Some("The sender proposes it".into()),
        };
        let shown = CardEntry {
            seq: 6,
            card: Card::Suggestion {
                account: "mail:anna@example.org".into(),
                id: "m:4".into(),
                suggestion: Box::new(suggestion),
            },
        };
        let json = serde_json::to_value(&shown).expect("serializes");
        assert_eq!(
            (&json["card"]["kind"], &json["card"]["suggestion"]["kind"]),
            (&"suggestion".into(), &"event".into())
        );
        assert_eq!(serde_json::from_value::<CardEntry>(json).expect("reads back"), shown);
        assert_eq!(Request::GetCards { conversation: "c1".into() }.conversation(), Some("c1"));
        let long = Request::GetCards { conversation: "x".repeat(MAX_ID_BYTES + 1) };
        assert!(limit_refusal(&long).is_some(), "the id's limit holds");
    }
}
