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

/// The link card and the provider to ask about of a [`Card::Link`] (issue #785), as the chat with people
/// has them: one shape in every window.
pub use torro_chat::{ForeignProvider, LinkCard};

/// At most this many characters in a message to the assistant (`assistant.send_message`).
pub const MAX_TEXT_CHARS: usize = 20_000;

/// At most this many characters in a conversation's title (`assistant.rename_conversation`).
pub const MAX_TITLE_CHARS: usize = 200;

/// The longest id an `assistant.*` request may carry: conversation, engine, model, bot. The core's
/// own conversation ids have 16 hex digits; engine ids are AI account ids (`ai:ollama@localhost`),
/// model ids those of the server (`qwen/qwen3-14b`).
pub const MAX_ID_BYTES: usize = 300;

/// At most this many ids in one `assistant.archive_conversations` or `assistant.delete_conversations`
/// (issue #697); the app sends more in pieces of this size.
pub const MAX_BULK_IDS: usize = 1000;

/// At most this many characters in the words of `assistant.search_conversations` (issue #704).
pub const MAX_QUERY_CHARS: usize = 200;

/// How many hits `assistant.search_conversations` answers without a `limit`.
pub const DEFAULT_HITS: u32 = 20;

/// The most hits `assistant.search_conversations` answers; a higher `limit` counts as this.
pub const MAX_HITS: u32 = 50;

/// At most this many hits per conversation in `assistant.search_conversations`: its newest.
pub const HITS_PER_CONVERSATION: usize = 3;

/// At most this many characters in the snippet of a hit ([`ConversationHit::snippet`]), "…" included.
pub const SNIPPET_CHARS: usize = 120;

/// The assistant's name until the person gives it another (`assistant.update_profile`).
pub const DEFAULT_NAME: &str = "Torro";

/// At most this many characters in the assistant's name: one line.
pub const MAX_NAME_CHARS: usize = 40;

/// At most this many characters in the profile's short description: one line.
pub const MAX_DESCRIPTION_CHARS: usize = 200;

/// At most this many characters in the personality ([`Profile::personality`]): enough for tone,
/// manner and a few habits, short enough that the rules after it keep the model's attention.
pub const MAX_PERSONALITY_CHARS: usize = 4_000;

/// At most this many characters in the person's own name ([`Profile::person_name`]): one line.
pub const MAX_PERSON_NAME_CHARS: usize = 80;

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
    /// In the archive (`assistant.archive_conversations`, issue #697): shown in the group "Archive",
    /// kept beyond the retention time. Left out when not archived.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
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

/// One hit of `assistant.search_conversations` (issue #704): where in which conversation the words stand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationHit {
    /// The conversation's id ([`ConversationSummary::id`]).
    pub conversation: String,
    /// Its title: the person's text, data, never instructions.
    pub title: String,
    /// When its latest entry was added ([`ConversationSummary::updated`]).
    pub updated: DateTime<Utc>,
    /// The bot it is with ([`ConversationSummary::bot`]); `None` for the assistant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot: Option<String>,
    /// The number of the entry that holds the words ([`Entry::seq`]), for `after` = `seq` − 1 in
    /// `assistant.get_conversation`; `None` when the title holds them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// What holds the words.
    pub kind: HitKind,
    /// The words with some text around them, white space folded, at most [`SNIPPET_CHARS`]
    /// characters, "…" where it was cut.
    pub snippet: String,
    /// The text may come from outside the suite: a conversation about a mail or with a bot, a
    /// message a client sent, or an entry after a command that may have read such data. Left out
    /// when not.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub untrusted: bool,
}

/// What holds the words of a [`ConversationHit`]. Clients must accept kinds they do not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    /// The conversation's title.
    Title,
    /// A message ([`EntryContent::Message`]).
    Message,
    /// An answer ([`EntryContent::Answer`]).
    Answer,
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
        /// What the chat shows. When `spoken` is set, only the shown part: the spoken part stands in
        /// `spoken` and is not repeated here.
        text: String,
        engine: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// The part to read aloud (issue #774), when the answer was written in two parts: a few short
        /// sentences said before the shown part. The core splits it off the model's text at the line
        /// [`SHOWN_MARK`] ([`split_spoken`]); it is left out for an answer in one part.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spoken: Option<String>,
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
/// they do not know (show nothing, or "No longer available"); an older client reads a kind of a newer
/// core as [`CardRef::Unknown`].
///
/// A card points at an item, it never copies it (`docs/inhaltskarten.md`, "Grundsätze"): what it shows
/// the core looks up anew for every `assistant.get_cards` ([`Card`]), with the rights of the
/// conversation's client. Ids in it are what the core read itself, never what a model wrote: a made-up
/// id resolves to [`Card::Unavailable`].
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
    /// A web address in an answer (issue #785): `{"kind": "link", "url": "https://example.org/a"}`.
    /// The core adds at most [`MAX_LINK_CARDS`] after an answer, in the order they stand in it. Whether
    /// the page's card loads by itself depends on where the address came from (`from`) and on Settings ›
    /// Privacy; an address the model wrote itself only ever gives the question ([`Card::Link::ask`]).
    Link {
        /// The address as it stands in the answer: `https` only.
        url: String,
        /// Where the address came from, as the core saw it when it added the card; left out:
        /// [`LinkOrigin::Model`], which always asks.
        #[serde(default, skip_serializing_if = "LinkOrigin::is_model")]
        from: LinkOrigin,
    },
    /// A mail the assistant found or read (issue #786): `{"kind": "mail", "account": "mail:…", "id": "m:…"}`.
    Mail {
        /// The mail account: `mail:<address>`.
        account: String,
        /// The mail's id (`m:…`).
        id: String,
    },
    /// An event (issue #797), added after `calendar.save_event` or shown from a list:
    /// `{"kind": "event", "uid": "…"}`, for one occurrence of a series with its `recurrence_id`.
    Event {
        /// The event's iCalendar UID.
        uid: String,
        /// Which occurrence of a series ([`torro_pim::Event::recurrence_id`]); left out for an event
        /// that is not part of one, and for a whole series (the next occurrence shows).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recurrence_id: Option<torro_pim::EventTime>,
    },
    /// A task (issue #797), added after `tasks.save_todo` or shown from a list: `{"kind": "todo", "uid": "…"}`.
    Todo {
        /// The task's iCalendar UID.
        uid: String,
    },
    /// A draft of a mail (issue #797), added after `mail.save_draft`: `{"kind": "draft", "id": "m:…"}`.
    Draft {
        /// The draft's id (`m:…`), as `mail.save_draft` answered it.
        id: String,
    },
    /// A contact (issue #789), shown from `contacts.list_contacts` or `contacts.get_contact`:
    /// `{"kind": "contact", "id": "…"}`.
    Contact {
        /// The contact's id, as `contacts.get_contact` takes it.
        id: String,
    },
    /// Several items found together (issue #786, `assistant.show_items`), shown by their number
    /// (`docs/inhaltskarten.md`, "Grenzen"): one to three as full cards, four to ten as short forms, and
    /// with `total` the line "{total} … match" with "Show all". Its items are [`ItemRef`]s, so it never
    /// holds a group: a group given as an item reads as [`ItemRef::Unknown`], which `assistant.show_items`
    /// refuses with `forbidden` ([`items_refusal`]).
    Group {
        /// The items, at most [`MAX_SHOWN_ITEMS`], in the order to show them.
        items: Vec<ItemRef>,
        /// The area the items are of (`mail`, `calendar`, `tasks`, `contacts`): which app "Show all"
        /// opens and what the line of the total names.
        area: crate::api::Area,
        /// How many the search found in all, when that was more than [`MAX_SHOWN_ITEMS`]. Set by the
        /// core from what it read itself, never from a model's input.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
    },
    /// A kind this build does not know (a newer core wrote it): shown as "No longer available".
    #[serde(other)]
    Unknown,
}

/// Where the address of a [`CardRef::Link`] came from (issue #785, decision 1 of the plan): set by the
/// core when it adds the card, from what it saw itself, never from a model. Only an address the person
/// wrote or one that stood in a command's answer of the conversation may load by itself, as Settings ›
/// Privacy says; one the model wrote could carry what an injected instruction wants to send out, so it
/// always asks first, also at "Always show".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkOrigin {
    /// In a message of the person in this conversation.
    Person,
    /// In the answer of a command this conversation ran (a mail it read, a page of the web tool).
    Read,
    /// Anywhere else: the model wrote it. Last, so it takes every value this build does not know.
    #[default]
    #[serde(other)]
    Model,
}

impl LinkOrigin {
    /// Whether it is [`LinkOrigin::Model`], which the wire leaves out.
    pub fn is_model(&self) -> bool {
        *self == LinkOrigin::Model
    }
}

/// At most this many items in one [`CardRef::Group`] (`assistant.show_items`, "Grenzen" in
/// `docs/inhaltskarten.md`).
pub const MAX_SHOWN_ITEMS: usize = 10;

/// At most this many [`CardRef::Link`] cards after one answer (issue #785), as at most three cards per
/// message in the chat with people.
pub const MAX_LINK_CARDS: usize = 3;

/// One item of a [`CardRef::Group`] and of `assistant.show_items` (issue #786): what a command of the
/// conversation found – a mail, an event, a task or a contact – with the same fields as its
/// [`CardRef`]. Never a group, so a group holds no group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemRef {
    /// As [`CardRef::Mail`].
    Mail { account: String, id: String },
    /// As [`CardRef::Event`].
    Event {
        uid: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recurrence_id: Option<torro_pim::EventTime>,
    },
    /// As [`CardRef::Todo`].
    Todo { uid: String },
    /// As [`CardRef::Contact`].
    Contact { id: String },
    /// Any other kind – a group, a file, a link – or one this build does not know: never shown,
    /// refused by `assistant.show_items` ([`items_refusal`]). Left out of the schema, so an agent is
    /// offered only the four kinds it may send.
    #[serde(other)]
    #[schemars(skip)]
    Unknown,
}

impl ItemRef {
    /// The card of the item alone ([`CardRef::Unknown`] for [`ItemRef::Unknown`]): what
    /// `assistant.show_items` adds for one item, and what each item of a group resolves as.
    pub fn card(&self) -> CardRef {
        match self.clone() {
            ItemRef::Mail { account, id } => CardRef::Mail { account, id },
            ItemRef::Event { uid, recurrence_id } => CardRef::Event { uid, recurrence_id },
            ItemRef::Todo { uid } => CardRef::Todo { uid },
            ItemRef::Contact { id } => CardRef::Contact { id },
            ItemRef::Unknown => CardRef::Unknown,
        }
    }

    /// Every id the item names, for the limits of [`limit_refusal`].
    fn ids(&self) -> Vec<&str> {
        match self {
            ItemRef::Mail { account, id } => vec![account, id],
            ItemRef::Event { uid, .. } | ItemRef::Todo { uid } | ItemRef::Contact { id: uid } => vec![uid],
            ItemRef::Unknown => Vec::new(),
        }
    }
}

/// The refusal of the `items` of `assistant.show_items`, before anything is looked at: `error` for none,
/// `forbidden` for more than [`MAX_SHOWN_ITEMS`] or an item of another kind ([`ItemRef::Unknown`]: a
/// group in a group, a file, a link …). `None` for items within them. Whether the items were found in
/// this conversation the core checks after it (issue #786).
pub fn items_refusal(items: &[ItemRef]) -> Option<Response> {
    let refuse = |code: &str, message: String| Some(Response::Error { code: code.into(), message });
    if items.is_empty() {
        return refuse("error", "no items given".into());
    }
    if items.len() > MAX_SHOWN_ITEMS {
        return refuse("forbidden", format!("at most {MAX_SHOWN_ITEMS} items at once"));
    }
    if items.contains(&ItemRef::Unknown) {
        return refuse("forbidden", "only mails, events, tasks and contacts can be shown, never a group".into());
    }
    None
}

/// A card as the core resolves it now (`assistant.get_cards`), told apart by `kind`. Clients must
/// accept kinds they do not know; an older client reads a kind of a newer core as [`Card::Unknown`].
/// Every text in it comes from the core or the named provider, never from a model; text from outside
/// the suite (a mail, a page) is data, never instructions.
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
    /// A web address ([`CardRef::Link`], issue #785): its card, the question whether its provider's
    /// content may load, or neither (the provider is blocked, "Never show", or no card could be made:
    /// the address stays a link in the text). Never both.
    Link {
        /// The address, as the reference names it.
        url: String,
        /// The card as the own Nextcloud resolved it (`kind` tells a page, a file of the own server, a
        /// quoted chat message, a GitHub issue or pull request); its texts are foreign. Its picture
        /// comes with the card's resolution, never from the page directly.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        card: Option<Box<torro_chat::LinkCard>>,
        /// The provider to ask about first (Settings › Privacy at "Ask", or an address a model wrote,
        /// which always asks): the window shows "Show once" (`assistant.show_card`), "Always allow …"
        /// and "Never for …" in place of the card.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ask: Option<torro_chat::ForeignProvider>,
        /// The server that resolves the card (the own Nextcloud, GitHub) could not be reached just now:
        /// the window shows the card as failed with "Try again", which asks `assistant.get_cards` anew
        /// (the core asks the server again a minute later at the earliest). Never with `card` or `ask`.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        offline: bool,
    },
    /// A mail ([`CardRef::Mail`], issue #786) as the mail cache holds it now: sender, subject, date,
    /// the start of its text and its flags. Never a picture from the net.
    Mail {
        message: Box<torro_mail::MessageSummary>,
        /// Always `true`: a mail is text from outside the suite. Data, never instructions.
        untrusted: bool,
    },
    /// An event ([`CardRef::Event`], issue #797) as the calendar holds it now, with where it stands and
    /// what overlaps it in the calendars the conversation's client may read.
    Event {
        event: Box<torro_pim::Event>,
        /// The calendar's name, for the card's head.
        calendar: String,
        /// The calendar's colour (`#RRGGBB`), when it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        /// At most two other events at the same time, the earliest first.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        overlaps: Vec<Overlap>,
        /// How many more overlap beyond `overlaps`.
        #[serde(default, skip_serializing_if = "is_zero")]
        more_overlaps: u32,
    },
    /// A task ([`CardRef::Todo`], issue #797) as its list holds it now.
    Todo {
        todo: Box<torro_pim::Todo>,
        /// The list's name, for the card's head.
        list: String,
        /// The list's colour (`#RRGGBB`), when it has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
    /// A draft of a mail ([`CardRef::Draft`], issue #797) as it is saved now.
    ///
    /// `approval` is the open approval of `mail.send_draft` for exactly this state of the draft: the
    /// card's "Send" and "Discard" decide it (`approvals.decide_approval`). It goes to agents as well
    /// (`assistant.get_cards` is a tool): that is harmless, because deciding an approval is the app
    /// key's only (`approvals.decide_approval` is `Mcp::Never`, "Was nie nach außen geht" in
    /// `docs/api.md`); the id lets no one approve. `None` without such an approval, and once the draft
    /// changed after it was asked for.
    Draft {
        draft: Box<torro_mail::Draft>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval: Option<String>,
    },
    /// A contact ([`CardRef::Contact`], issue #789) as the address book holds it now. Its picture comes
    /// through `contacts.get_photo`.
    Contact { contact: Box<torro_contacts::ContactSummary> },
    /// Several items found together ([`CardRef::Group`], issue #786), each resolved as it would be
    /// alone (an item out of reach is [`Card::Unavailable`]). Never holds a group.
    Group {
        items: Vec<Card>,
        /// The area of the items, as [`CardRef::Group::area`].
        area: crate::api::Area,
        /// How many the search found in all, when that was more than shown.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
    },
    /// Gone or out of reach: deleted, changed, moved, made by another client, or no longer within
    /// the rights (the tool switched off, the area or the tool taken away). Shown as "No longer
    /// available"; why is not told.
    Unavailable,
    /// A kind this build does not know (a newer core sent it): shown as "No longer available".
    #[serde(other)]
    Unknown,
}

/// Another event at the time of an [`Card::Event`]: its title and time, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Overlap {
    /// Its title: the calendar's text, data.
    pub summary: String,
    pub start: torro_pim::EventTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<torro_pim::EventTime>,
}

fn is_zero(count: &u32) -> bool {
    *count == 0
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
    /// The name of the person the assistant works for, as they wrote it (#702): one line, at most
    /// [`MAX_PERSON_NAME_CHARS`] characters; empty when the assistant should not know it. It goes
    /// into every prompt; empty from cores older than this field.
    #[serde(default)]
    pub person_name: String,
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
/// over [`MAX_NOTE_CHARS`], a search that is blank or over [`MAX_QUERY_CHARS`] (issue #704), a skill beyond
/// its limits ([`skill_refusal`]), items of `assistant.show_items` beyond theirs ([`items_refusal`], which
/// answers `forbidden` for too many and for a group in a group), and a change of the profile that changes nothing or breaks its limits
/// ([`profile_refusal`]). `None` for every request within them and for every other request. The
/// core calls it before rights, approvals and handlers, so nobody is asked to approve what would be
/// refused anyway.
pub fn limit_refusal(request: &Request) -> Option<Response> {
    let refuse = |message: String| Some(Response::Error { code: "error".into(), message });
    if assistant_ids(request).into_iter().any(|id| id.len() > MAX_ID_BYTES) {
        return refuse(format!("an id may hold at most {MAX_ID_BYTES} bytes"));
    }
    match request {
        Request::ArchiveConversations { ids, .. } | Request::DeleteConversations { ids } if ids.is_empty() => {
            refuse("no conversations given".into())
        }
        Request::ArchiveConversations { ids, .. } | Request::DeleteConversations { ids }
            if ids.len() > MAX_BULK_IDS =>
        {
            refuse(format!("at most {MAX_BULK_IDS} conversations at once"))
        }
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
        Request::SearchConversations { query, .. } if query.trim().is_empty() => refuse("the query is empty".into()),
        Request::SearchConversations { query, .. } if query.chars().count() > MAX_QUERY_CHARS => {
            refuse(format!("a query may hold at most {MAX_QUERY_CHARS} characters"))
        }
        Request::ShowItems { items, .. } => items_refusal(items),
        Request::SaveSkill { name, description, text, .. } => skill_refusal(name, description, text, true)
            .map(|message| Response::Error { code: "error".into(), message }),
        Request::UpdateProfile { name, description, avatar, personality, person_name } => profile_refusal(
            name.as_deref(),
            description.as_deref(),
            avatar.as_deref(),
            personality.as_deref(),
            person_name.as_deref(),
        )
        .map(|message| Response::Error { code: "error".into(), message }),
        _ => None,
    }
}

/// Why a change of the profile (`assistant.update_profile`) is refused, for people; `None` when it
/// keeps every limit: at least one field, a name that is one line and not blank, a description of
/// one line, both within their lengths, a picture of [`AVATARS`], a personality within
/// [`MAX_PERSONALITY_CHARS`], the person's name one line within [`MAX_PERSON_NAME_CHARS`] (empty
/// clears it). Lengths count characters after trimming.
pub fn profile_refusal(
    name: Option<&str>,
    description: Option<&str>,
    avatar: Option<&str>,
    personality: Option<&str>,
    person_name: Option<&str>,
) -> Option<String> {
    if name.is_none() && description.is_none() && avatar.is_none() && personality.is_none() && person_name.is_none() {
        return Some("nothing to change: give a name, description, avatar, personality or person_name".into());
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
    if let Some(person_name) = person_name
        && (person_name.trim().lines().count() > 1 || chars(person_name) > MAX_PERSON_NAME_CHARS)
    {
        return Some(format!("your name is one line of at most {MAX_PERSON_NAME_CHARS} characters"));
    }
    None
}

/// The line between the spoken and the shown part of an answer that is read aloud (issue #774): the
/// model writes the spoken part first, then this line alone, then what the chat shows. The core
/// splits at it ([`split_spoken`]), so no client sees it.
pub const SHOWN_MARK: &str = "---shown---";

/// At most about this many characters of an answer are read aloud when it has no spoken part
/// (issue #774): the reading ends at the last sentence end before them. A spoken part is read whole,
/// as `assistant.speak` reads it.
pub const SPOKEN_MOST: usize = 600;

/// Splits an answer's text into its spoken and its shown part (issue #774) at the first
/// [`SHOWN_MARK`]: what comes before is spoken (trimmed only at its end, so it stays a start of the
/// text, which matters while it is read sentence by sentence), what comes after is shown (trimmed, it
/// may be empty). Without a mark `(None, text)`; with nothing but blanks before it `(None, shown)`.
///
/// Models do not always write the mark as asked (issue #826), so it is found the way they write it:
/// the word `shown` with dashes around it (hyphens or long dashes, with blanks, bold or backticks),
/// alone on its line, in the middle of a line or at its end, and split over two lines as a Markdown
/// rule `---` above `shown---` or `**shown**`. Dashes with blanks around the word mark only at the end
/// of a line; dashes on the left only mark at the end of a line when they touch the word or stand
/// above it, dashes on the right only when the mark is alone on its line. So `as shown -- see`,
/// `the dates — shown — are fixed` and a list item `- true — shown` do not split.
///
/// While the answer is still written (`running`), an end of the text that may become the mark is
/// left out, so no piece of it flashes up in the chat or is read aloud.
pub fn split_spoken(text: &str, running: bool) -> (Option<String>, String) {
    if let Some((start, end)) = find_mark(text, running) {
        let spoken = text[..start].trim_end().to_owned();
        let shown = text[end..].trim().to_owned();
        // Nothing before the mark is no spoken part: the answer is read as one without it.
        return (Some(spoken).filter(|spoken| !spoken.trim().is_empty()), shown);
    }
    if running && let Some(start) = mark_begun(text) {
        return (None, text[..start].to_owned());
    }
    (None, text.to_owned())
}

/// The word of [`SHOWN_MARK`], found in any case.
const MARK_WORD: &str = "shown";

/// A dash as models write it around the mark: a hyphen, or a long or minus dash.
fn is_dash(c: char) -> bool {
    matches!(c, '-' | '‐' | '‑' | '‒' | '–' | '—' | '―' | '−')
}

/// A blank on the mark's line, or the Markdown models wrap it in (bold, italics, code).
fn is_blank_or_wrap(c: char) -> bool {
    matches!(c, ' ' | '\t' | '*' | '_' | '`')
}

/// Dashes that mark: two hyphens or more, or one long dash; a lone hyphen is a list item or a word.
fn marks(dashes: &str) -> bool {
    dashes.chars().count() >= 2 || dashes.chars().any(|c| c != '-')
}

/// Where the chars from `at` on that `keep` holds end.
fn skip(text: &str, at: usize, keep: impl Fn(char) -> bool) -> usize {
    text[at..].char_indices().find(|(_, c)| !keep(*c)).map_or(text.len(), |(len, _)| at + len)
}

/// Where the chars before `at` that `keep` holds begin.
fn skip_back(text: &str, at: usize, keep: impl Fn(char) -> bool) -> usize {
    text[..at].char_indices().rev().take_while(|(_, c)| keep(*c)).last().map_or(at, |(start, _)| start)
}

/// The bytes of the first mark in `text` ([`split_spoken`]), from the blanks before it to the blanks
/// after it on its line; `running` for an answer still written, whose end is no line end yet.
fn find_mark(text: &str, running: bool) -> Option<(usize, usize)> {
    // ASCII lowercase keeps every byte where it is, so its positions hold in `text`.
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(MARK_WORD) {
        let word = from + found;
        let after = word + MARK_WORD.len();
        from = after;
        let alnum = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
        if alnum(text[..word].chars().next_back()) || alnum(text[after..].chars().next()) {
            continue;
        }
        // Before the word the dashes may stand on the line above: a Markdown rule `---`.
        let inner = skip_back(text, word, |c| c.is_whitespace() || is_blank_or_wrap(c));
        let dashes = skip_back(text, inner, is_dash);
        let left = marks(&text[dashes..inner]);
        // Tight: only wrap between dashes and word (`---shown`, `—**shown**`); broken: the dashes are a rule above.
        let tight_left = !text[inner..word].contains(char::is_whitespace);
        let broken_left = text[inner..word].contains('\n');
        let start = skip_back(text, if left { dashes } else { word }, is_blank_or_wrap);
        let inner = skip(text, after, is_blank_or_wrap);
        let dashes = skip(text, inner, is_dash);
        let right = marks(&text[inner..dashes]);
        let tight_right = !text[after..inner].contains(char::is_whitespace);
        let end = skip(text, if right { dashes } else { after }, is_blank_or_wrap);
        let line_start = start == 0 || text[..start].ends_with('\n');
        // While the answer runs, its end is no line end yet: `list — shown` may go on with ` below`.
        let line_end = text[end..].starts_with(['\n', '\r']) || (end == text.len() && !running);
        // Spaced dashes are prose in the middle of a line (`the dates — shown — are fixed`), so they mark only
        // at a line's end, and dashes on one side only when tight, as a rule above or alone on the line.
        let found = match (left, right) {
            (true, true) => (tight_left && tight_right) || line_end,
            (true, false) => line_end && (tight_left || broken_left || line_start),
            (false, true) => line_start && line_end,
            (false, false) => false,
        };
        if found {
            return Some((start, end));
        }
    }
    None
}

/// Where an end of `text` begins that may still become a mark (dashes, then a start of the word, or
/// a start of the word alone at a line's start), for a running answer ([`split_spoken`]).
fn mark_begun(text: &str) -> Option<usize> {
    // A mark is short; only the last chars can be one in the making.
    let from = text.char_indices().rev().nth(24).map_or(0, |(at, _)| at);
    text.char_indices()
        .skip_while(|(at, _)| *at < from)
        .map(|(at, _)| at)
        .find(|at| begins_mark(&text[*at..], *at == 0 || text[..*at].ends_with('\n')))
}

/// Whether `tail` may grow into a mark: blanks or wrap, dashes, blanks (also a line break), the word
/// as far as it came, then blanks, wrap or dashes. Without dashes the word must start the line.
fn begins_mark(tail: &str, line_start: bool) -> bool {
    let rest = tail.trim_start_matches(is_blank_or_wrap);
    let after_dashes = rest.trim_start_matches(is_dash);
    let dashed = after_dashes.len() < rest.len();
    if !dashed && !line_start {
        return false;
    }
    let word = after_dashes.trim_start_matches(|c: char| c.is_whitespace() || is_blank_or_wrap(c));
    let len = word.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(word.len());
    let (start, end) = word.split_at(len);
    let written = MARK_WORD.get(..start.len()).is_some_and(|part| part.eq_ignore_ascii_case(start));
    let closed = end.chars().all(|c| is_dash(c) || is_blank_or_wrap(c)) && (end.is_empty() || len == MARK_WORD.len());
    // Something must be there besides blanks: dashes or a piece of the word.
    (dashed || !start.is_empty()) && written && closed
}

/// Where the reading aloud of an answer begins (issue #747, `assistant.speak_answer`): after the last
/// message of `entries`, or after an approval that was decided and answered on, so what came before it
/// is not read again and what follows it is. `None` without a message. The core and the chat window
/// both use it, so they agree which answer was read.
pub fn answer_start(entries: &[Entry]) -> Option<u64> {
    let message = entries.iter().rposition(|entry| matches!(entry.content, EntryContent::Message { .. }))?;
    let mut start = entries[message].seq;
    for (at, entry) in entries.iter().enumerate().skip(message + 1) {
        if matches!(entry.content, EntryContent::Approval { .. }) && at + 1 < entries.len() {
            start = entry.seq;
        }
    }
    Some(start)
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
            | Request::SpeakAnswer { conversation: id }
            | Request::OpenChatWindow { window: ChatWindow::Conversation(id) }
            | Request::ConfirmBot { conversation: id, .. }
            | Request::ShowAssistantCard { conversation: id, .. }
            | Request::GetCardPicture { conversation: id, .. } => Some(id),
            Request::SendMessage { conversation, .. }
            | Request::AskBot { conversation, .. }
            | Request::StartTalk { conversation }
            | Request::ShowItems { conversation, .. } => conversation.as_deref(),
            _ => None,
        }
    }
}

impl Request {
    /// Every conversation a request names: [`Request::conversation`], or the `ids` of
    /// `assistant.archive_conversations` and `assistant.delete_conversations` (issue #697). For the
    /// core's checks, which must see each one before anything changes, and its audit log.
    pub fn conversation_ids(&self) -> Vec<&str> {
        match self {
            Request::ArchiveConversations { ids, .. } | Request::DeleteConversations { ids } => {
                ids.iter().map(String::as_str).collect()
            }
            _ => self.conversation().into_iter().collect(),
        }
    }
}

/// Every id an `assistant.*` request carries, for [`limit_refusal`].
fn assistant_ids(request: &Request) -> Vec<&str> {
    let mut ids: Vec<&str> = request.conversation_ids();
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
        Request::ShowItems { items, .. } => ids.extend(items.iter().flat_map(ItemRef::ids)),
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
            spoken: false,
        }
    }

    fn code(response: Option<Response>) -> Option<String> {
        response.map(|response| match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        })
    }

    /// Issue #704: a search needs words, within [`MAX_QUERY_CHARS`]; its `limit` the core keeps in bounds.
    #[test]
    fn a_search_keeps_its_limits() {
        let search = |query: &str| Request::SearchConversations { query: query.into(), limit: Some(500) };
        assert_eq!(code(limit_refusal(&search("flight"))), None, "a high limit is kept in bounds, not refused");
        assert_eq!(code(limit_refusal(&search(" \n"))), Some("error".into()), "no words");
        assert_eq!(code(limit_refusal(&search(&"ä".repeat(MAX_QUERY_CHARS)))), None, "characters, not bytes");
        assert_eq!(code(limit_refusal(&search(&"a".repeat(MAX_QUERY_CHARS + 1)))), Some("error".into()));
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
            spoken: false,
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
            spoken: false,
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
            spoken: false,
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
                    person_name: None,
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

    /// Issue #702: the person's name is one line within its length; empty clears it and counts as
    /// a change.
    #[test]
    fn the_persons_name_keeps_its_limits() {
        let named = |person_name: &str| Request::UpdateProfile {
            name: None,
            description: None,
            avatar: None,
            personality: None,
            person_name: Some(person_name.into()),
        };
        assert_eq!(code(limit_refusal(&named("Anna Berg"))), None);
        assert_eq!(code(limit_refusal(&named(""))), None, "empty clears the name");
        assert_eq!(code(limit_refusal(&named("Anna\nBerg"))), Some("error".into()), "two lines");
        assert_eq!(code(limit_refusal(&named(&"a".repeat(MAX_PERSON_NAME_CHARS)))), None);
        assert_eq!(code(limit_refusal(&named(&"a".repeat(MAX_PERSON_NAME_CHARS + 1)))), Some("error".into()));
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

    #[test]
    fn bulk_requests_name_every_conversation_and_keep_their_limits() {
        let archive = |ids: Vec<String>| Request::ArchiveConversations { ids, archived: true };
        let delete = |ids: Vec<String>| Request::DeleteConversations { ids };
        assert_eq!(archive(vec!["a".into(), "b".into()]).conversation_ids(), ["a", "b"]);
        assert_eq!(delete(vec!["a".into()]).conversation_ids(), ["a"]);
        assert_eq!(Request::DeleteConversation { id: "c1".into() }.conversation_ids(), ["c1"]);
        assert!(Request::Ping.conversation_ids().is_empty());

        assert_eq!(code(limit_refusal(&archive(Vec::new()))), Some("error".into()), "no ids");
        assert_eq!(code(limit_refusal(&delete(Vec::new()))), Some("error".into()), "no ids");
        let many = |n: usize| (0..n).map(|i| format!("{i:016x}")).collect::<Vec<_>>();
        assert_eq!(code(limit_refusal(&delete(many(MAX_BULK_IDS)))), None);
        assert_eq!(code(limit_refusal(&delete(many(MAX_BULK_IDS + 1)))), Some("error".into()));
        assert_eq!(code(limit_refusal(&archive(many(MAX_BULK_IDS + 1)))), Some("error".into()));
        assert_eq!(code(limit_refusal(&delete(vec!["x".repeat(MAX_ID_BYTES + 1)]))), Some("error".into()), "long id");
    }

    #[test]
    fn archived_is_left_out_unless_set() {
        let summary = ConversationSummary {
            id: "c1".into(),
            title: "T".into(),
            engine: "e".into(),
            model: None,
            effort: None,
            created: Utc::now(),
            updated: Utc::now(),
            pinned: false,
            archived: false,
            running: false,
            last: 0,
            bot: None,
            bot_deleted: false,
            about: None,
        };
        let value = serde_json::to_value(&summary).expect("serializes");
        assert!(value.get("archived").is_none());
        let back: ConversationSummary = serde_json::from_value(value).expect("reads without the field");
        assert!(!back.archived);
        let archived = ConversationSummary { archived: true, ..summary };
        assert_eq!(serde_json::to_value(&archived).expect("serializes")["archived"], true);
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

    /// Issue #784: the new kinds of cards read as `docs/inhaltskarten.md` names them, and a kind a newer core
    /// writes reads as unknown instead of failing the whole conversation.
    #[test]
    fn content_cards_read_as_documented_and_unknown_kinds_survive() {
        let read = |json: serde_json::Value| serde_json::from_value::<CardRef>(json).expect("a card reference");
        assert_eq!(
            read(serde_json::json!({"kind": "link", "url": "https://example.org/a"})),
            CardRef::Link { url: "https://example.org/a".into(), from: LinkOrigin::Model },
            "an address of unknown origin asks first"
        );
        assert_eq!(
            read(serde_json::json!({"kind": "link", "url": "https://example.org/a", "from": "person"})),
            CardRef::Link { url: "https://example.org/a".into(), from: LinkOrigin::Person }
        );
        assert_eq!(
            read(serde_json::json!({"kind": "mail", "account": "mail:anna@example.org", "id": "m:4"})),
            CardRef::Mail { account: "mail:anna@example.org".into(), id: "m:4".into() }
        );
        assert_eq!(
            read(serde_json::json!({"kind": "event", "uid": "e1"})),
            CardRef::Event { uid: "e1".into(), recurrence_id: None }
        );
        assert_eq!(read(serde_json::json!({"kind": "todo", "uid": "t1"})), CardRef::Todo { uid: "t1".into() });
        assert_eq!(read(serde_json::json!({"kind": "draft", "id": "m:9"})), CardRef::Draft { id: "m:9".into() });
        assert_eq!(read(serde_json::json!({"kind": "contact", "id": "c1"})), CardRef::Contact { id: "c1".into() });
        let group = read(serde_json::json!({
            "kind": "group", "area": "mail", "total": 12,
            "items": [{"kind": "mail", "account": "mail:anna@example.org", "id": "m:4"}, {"kind": "group"}]
        }));
        let CardRef::Group { items, area, total } = &group else { panic!("a group: {group:?}") };
        assert_eq!((area, total), (&crate::api::Area::Mail, &Some(12)));
        assert_eq!(items[1], ItemRef::Unknown, "a group in a group reads as unknown");
        assert_eq!(items[0].card(), CardRef::Mail { account: "mail:anna@example.org".into(), id: "m:4".into() });
        assert_eq!(read(serde_json::json!({"kind": "map", "lat": 1.0})), CardRef::Unknown, "a newer kind");

        let shown: Card = serde_json::from_value(serde_json::json!({"kind": "photo", "id": "p1"})).expect("reads");
        assert_eq!(shown, Card::Unknown);
        let link = Card::Link {
            url: "https://example.org/a".into(),
            card: None,
            ask: Some(ForeignProvider { key: "example.org".into(), name: "example.org".into() }),
            offline: false,
        };
        let json = serde_json::to_value(&link).expect("serializes");
        assert_eq!((&json["kind"], &json["ask"]["key"]), (&"link".into(), &"example.org".into()));
        assert!(json.get("offline").is_none(), "left out while reachable");
        assert_eq!(serde_json::from_value::<Card>(json).expect("reads back"), link);
        let offline = Card::Link { url: "https://example.org/a".into(), card: None, ask: None, offline: true };
        let json = serde_json::to_value(&offline).expect("serializes");
        assert_eq!(json["offline"], true);
        assert_eq!(serde_json::from_value::<Card>(json).expect("reads back"), offline);
        let entry = CardEntry {
            seq: 3,
            card: Card::Group { items: vec![Card::Unavailable], area: crate::api::Area::Tasks, total: None },
        };
        let json = serde_json::to_value(&entry).expect("serializes");
        assert_eq!(
            json["card"],
            serde_json::json!({"kind": "group", "items": [{"kind": "unavailable"}], "area": "tasks"})
        );
    }

    /// Issue #786: `assistant.show_items` takes one to ten mails, events, tasks or contacts; a group in a group,
    /// any other kind and more than ten are `forbidden` before anything is looked at.
    #[test]
    fn shown_items_keep_their_limits_and_never_hold_a_group() {
        let mail = |n: usize| ItemRef::Mail { account: "mail:anna@example.org".into(), id: format!("m:{n}") };
        let show = |items: Vec<ItemRef>| Request::ShowItems { conversation: None, items };
        assert_eq!(code(limit_refusal(&show(vec![mail(1)]))), None);
        assert_eq!(code(limit_refusal(&show((0..MAX_SHOWN_ITEMS).map(mail).collect()))), None);
        assert_eq!(code(limit_refusal(&show(Vec::new()))), Some("error".into()), "no items");
        assert_eq!(code(limit_refusal(&show((0..=MAX_SHOWN_ITEMS).map(mail).collect()))), Some("forbidden".into()));
        let nested: Request = serde_json::from_value(serde_json::json!({
            "method": "assistant.show_items",
            "items": [{"kind": "group", "area": "mail", "items": [{"kind": "mail", "account": "a", "id": "m:1"}]}]
        }))
        .expect("a group as an item still parses");
        assert_eq!(code(limit_refusal(&nested)), Some("forbidden".into()), "no group in a group");
        let file = show(vec![mail(1), ItemRef::Unknown]);
        assert_eq!(code(limit_refusal(&file)), Some("forbidden".into()), "another kind");
        let long = show(vec![ItemRef::Contact { id: "c".repeat(MAX_ID_BYTES + 1) }]);
        assert_eq!(code(limit_refusal(&long)), Some("error".into()), "an id beyond its limit");
        let card = Request::ShowAssistantCard { conversation: "c1".into(), seq: 4 };
        assert_eq!(card.conversation(), Some("c1"));
        assert_eq!(show(vec![mail(1)]).conversation(), None, "the conversation that answers now");
    }

    #[test]
    fn an_answer_splits_at_the_mark_line() {
        let text = "Nine events, have a look.\n\n---shown---\n\n- Mon 9:00 Standup\n- Tue 10:00 Review\n";
        let (spoken, shown) = split_spoken(text, false);
        assert_eq!(spoken.as_deref(), Some("Nine events, have a look."));
        assert_eq!(shown, "- Mon 9:00 Standup\n- Tue 10:00 Review");
        assert!(text.starts_with(spoken.as_deref().unwrap_or_default()), "the spoken part is a start of the text");
        // The mark with spaces around still counts, and nothing after it is an empty shown part.
        assert_eq!(split_spoken("Yes.\n  ---shown---  ", false), (Some("Yes.".into()), String::new()));
        // In backticks it counts as well; nothing before it is no spoken part.
        assert_eq!(split_spoken("Yes.\n`---shown---`\nMo", false), (Some("Yes.".into()), "Mo".into()));
        assert_eq!(split_spoken("\n---shown---\n- Mon", false), (None, "- Mon".into()));
        assert_eq!(split_spoken("Yes.\n`---sh", true), (None, "Yes.\n".into()));
        // Only the first mark splits; a later one belongs to what is shown.
        assert_eq!(split_spoken("A.\n---shown---\nB\n---shown---\nC", false).1, "B\n---shown---\nC");
    }

    #[test]
    fn an_answer_without_the_mark_stays_whole() {
        let text = "Just one part, - with a dash.\nAnd a list:\n- one";
        assert_eq!(split_spoken(text, false), (None, text.to_owned()));
        // The word in a sentence is no mark, not even with dashes on one side of it.
        for text in [
            "As shown -- see the list.",
            "The dates — shown in the list — are fixed.",
            "Here is the list — shown below.",
            "A showroom --- open",
            "Shown: the list\n- one",
            "---\nShown are the dates.",
            "Options:\n- `true` — shown\n- `false` — hidden",
            "hidden -- shown\nvisible",
            "The dates — shown — are fixed.",
        ] {
            assert_eq!(split_spoken(text, false), (None, text.to_owned()), "{text}");
        }
    }

    /// The mark as a model really wrote it (issue #826, from stored answers of `glm-5.3-flash` via Ollama,
    /// words masked): the dashes as a Markdown rule on the line above the word.
    #[test]
    fn a_mark_split_over_two_lines_splits() {
        let text = "Two dates today, have a look.\n\n---\nshown---\n\n**The dates**\n- Mon 9:00 Standup";
        assert_eq!(
            split_spoken(text, false),
            (Some("Two dates today, have a look.".into()), "**The dates**\n- Mon 9:00 Standup".into())
        );
        assert_eq!(split_spoken("All done?\n\n---\nshown---", false), (Some("All done?".into()), String::new()));
        let text = "Here it is.\n\n---\n**shown**\n\n- one\n- two";
        assert_eq!(split_spoken(text, false), (Some("Here it is.".into()), "- one\n- two".into()));
    }

    #[test]
    fn a_mark_written_loosely_splits() {
        for text in [
            "Yes. ---shown---\nMo",
            "Yes.\n--- shown ---\nMo",
            "Yes.\n—shown—\nMo",
            "Yes. – shown –\nMo",
            "Yes.\n**---shown---**\nMo",
            "Yes.\n---SHOWN---\nMo",
            "Yes.\n`--- shown ---`\nMo",
            "Yes.\nshown---\nMo",
            "Yes. ---shown\nMo",
            "Yes. ---shown--- Mo",
        ] {
            assert_eq!(split_spoken(text, false), (Some("Yes.".into()), "Mo".into()), "{text}");
        }
    }

    #[test]
    fn a_loose_mark_being_written_is_left_out() {
        for (text, shown) in [
            ("Yes. ---sh", "Yes."),
            ("Yes. —", "Yes."),
            ("Yes.\n\n---\n", "Yes.\n\n"),
            ("Yes.\n\n---\nsho", "Yes.\n\n"),
            ("Yes.\n\n---\n**sh", "Yes.\n\n"),
            ("Yes.\nsho", "Yes.\n"),
        ] {
            assert_eq!(split_spoken(text, true), (None, shown.into()), "{text}");
        }
        // The word at the end waits for its line to end: it may go on as prose.
        assert_eq!(split_spoken("Yes.\n\n---\nshown", true), (None, "Yes.\n\n".into()));
        assert_eq!(split_spoken("Yes.\n\n---\nshown\n", true), (Some("Yes.".into()), String::new()));
        assert_eq!(split_spoken("Yes. – shown –", true), (None, "Yes.".into()));
        assert_eq!(split_spoken("Here is the list — shown", true).0, None);
        // What cannot become the mark stays.
        for text in ["A list:\n- one", "It is **bold**", "Text - more", "A well-known one", "Yes.\nShould"] {
            assert_eq!(split_spoken(text, true).1, text, "{text}");
        }
    }

    #[test]
    fn a_mark_being_written_is_left_out() {
        assert_eq!(split_spoken("Nine events.\n---sh", true), (None, "Nine events.\n".into()));
        assert_eq!(split_spoken("Nine events.\n-", true), (None, "Nine events.\n".into()));
        // Done, the same text is shown as it is; a list item is no mark.
        assert_eq!(split_spoken("Nine events.\n---sh", false).1, "Nine events.\n---sh");
        assert_eq!(split_spoken("A list:\n- one", true).1, "A list:\n- one");
        // A mark written in full splits while running as well.
        assert_eq!(split_spoken("Hi.\n---shown---\nMo", true), (Some("Hi.".into()), "Mo".into()));
    }
}
