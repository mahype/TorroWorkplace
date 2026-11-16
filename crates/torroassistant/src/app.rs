//! The assistant's window (task C.4, `docs/assistent.md`, "Die App `torroassistant`"): its state,
//! what every click and answer does ([`App::update`]), and how it follows the core.
//!
//! Two kinds of window from one program ([`Window`]): the main window (class `torro-assistant`)
//! with the conversations in the sidebar – pinned on top, then the newest, with a search – and the
//! chat beside them; from [`SEARCH_FROM`] characters on the search looks through the whole history
//! (`assistant.search_conversations`, issue #704), and a hit opens its conversation at the place; and a
//! single chat window (`--chat <id>|new`, class `torro-assistant-chat`,
//! [`CHAT`]) with the chat alone. "Open in new window" in a conversation's menu, or a double
//! click on it in the sidebar, sends `assistant.open_window`; the core starts the chat window. A
//! chat window whose conversation was deleted meanwhile becomes a new chat, as a workspace
//! template that brings it back expects.
//!
//! The chat: the person's messages, the answers (Markdown), the tools the assistant used as one
//! line each ("Searched mail · 12 results") that unfolds to its input and result, and an approval
//! card when a tool waits for the person, with the summary the core wrote (never the model's text)
//! and "Allow" / "Deny". A click there sends `approvals.decide_approval` with the app key; nothing
//! else ever decides – no key, no timer, no message of the model. "Stop" sends `assistant.cancel`.
//! The composer picks the engine and the model (only models that can call tools, AI accounts on
//! this computer first) and the areas switched off for this conversation ("Without Mail").
//!
//! Models turned on (task S.26): the picker – the composer's and the bot editor's – offers only the
//! models turned on for the suite (`AiModel::enabled`, switched in the control centre under
//! AI, task S.24), asks every AI account for them again each time it opens, and ends with
//! "Manage models…", which opens the control centre's AI section. A model the choice names that is
//! turned off – a bot's, the default of new chats, a conversation's – stands on top of its engine
//! with "Turned off in Settings › AI", since the core answers with "Automatic" then
//! ([`App::turned_off`]). With AI accounts but no model on anywhere the welcome says so
//! ([`App::no_model_on`]): the core lists no engine then, so the window asks
//! `accounts.list_accounts` whether there is an AI account at all.
//!
//! Following an answer: there is no `subscribe` yet, so the window asks `assistant.get_conversation`
//! with `after` = the last number it has, every [`FOLLOW`] while an answer runs ([`FOLLOW_WAITING`]
//! while it waits for an approval) and every [`REFRESH`] otherwise, and merges what comes
//! ([`Transcript::merge`]): the answer the model is still writing comes again and again under the
//! same number and replaces itself.
//!
//! Smooth streaming (issue #135): an answer that comes while the window watches appears letter by
//! letter on the window's frames ([`Stream`], [`crate::stream`]), at a pace that follows what came;
//! one that was there when the conversation opened shows at once. Entries after it wait until it
//! shows whole, so the text never runs on below a tool line. The history follows the newest entry
//! only while it is scrolled to the bottom ([`App::follow_bottom`]); scrolled up, nothing moves.
//!
//! First start: without the client "Assistant" (`clients.list_clients`) a new chat asks the person
//! to choose what the assistant may use, with a button that opens the settings at Access in this
//! window (task S.34, [`App::open_access`]); "Change access" under a refused tool opens Access too,
//! at the bot when a bot's conversation is open. Without an AI account it says so and opens the
//! control centre's AI section (`--section ai`).
//!
//! Settings (task S.18, `docs/settings.md`): the gear in the title band – and "Settings" in the
//! sidebar where there is no band – turns the content area into the assistant's settings and back
//! ([`App::toggle_settings`]). The sections and their pages are `torro_settings_ui::assistant`,
//! shown by the library's [`Host`], which talks to the core through [`Backend::settings`]; the
//! sidebar stays, and a click on a chat or a bot there closes the settings again
//! ([`App::leave_settings`]). `--settings <section>` opens the window with them open. Every way out
//! – the gear, Escape, a click in the sidebar – asks the open section first, so a profile that is
//! typed and not saved is never dropped silently ([`App::settings_question`]).
//!
//! **Only a person opens the settings.** `--settings` is a start parameter, and the links of an
//! answer stay on `http(s)` ([`web_link`]): no text of the model can open a section, name a scheme
//! of its own or point at a path.
//!
//! Bots (task B.3, [`crate::bots`]): the sidebar lists them under "Bots" with "New bot"; a click
//! opens a bot's chat – its newest conversation or a new one – in which messages go to the bot
//! (`assistant.send_message { bot }`) and the picker starts at the bot's engine and model. The bot
//! editor (profile, personality, instructions, skills, engine) takes the chat's place and saves with
//! `bots.save_bot`; deleting asks first (`bots.delete_bot`). "Access…" and the editor's "What the
//! bot may use" open the settings at Access at this bot (task S.34): the bot's rights are chosen
//! there, with the rights editor of `torro-settings-ui`; the editor itself never sends `clients.*`.
//! `--bot <id>` is a chat window with one bot
//! ([`Window::Bot`]), titled "{bot} · {title}"; "Open in new window" of a bot starts it through
//! `assistant.open_window`. Its methods live in `app/bots.rs`.
//!
//! Cards (task WZ.8, `docs/assistent.md` "Datei-Karte"): a card entry points at an item and never
//! copies it, so the window asks `assistant.get_cards` for what each card shows now – when the
//! conversation opens, when a new card comes and on every [`REFRESH`] – and draws a card only once
//! the core resolved it. A file card has three buttons, each a person's click and a request of the
//! window: "Open" (`tools.open_file`), "Save as…" (the system's save dialog, then
//! `tools.save_file_as`, which only the app key may send) and "Send by mail" (`apps.open_app` with
//! `file:<id>`: the mail's compose window with the file attached). A file that went is "No longer
//! available", without buttons.
//!
//! Mini and strip tiles show the newest answer and a one-line composer, nothing else; a card after
//! the newest message shows there as one line.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::theme::Mode;
use iced::widget::markdown;
use iced::widget::operation;
use iced::widget::scrollable;
use iced::widget::text_editor::{self, Binding, Content, KeyPress};
use iced::{Size, Subscription, Task, Theme, keyboard};
use torro_core::AppId;
use torro_core::ai::{AiModel, Effort};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{AssistantDefault, Settings};
use torro_gui::design::{self, Follow};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, SidebarMode, Switcher};
use torro_gui::source::Source;
use torro_gui::split::{self, Limits, Split, SplitEvent};
use torro_gui::theme::{self, Appearance};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets;
use torro_gui::zoom::{self, Zoom};
use torro_ipc::api::{Area, Rights};
use torro_ipc::assistant::Mode as PermissionMode;
use torro_ipc::assistant::{
    Card, CardEntry, ChatWindow, ConversationHit, ConversationSummary, Engine, EngineKind, Entry, EntryContent,
    MAX_BULK_IDS, MAX_HITS, Profile,
};
use torro_ipc::bots::{Bot, BotSummary};
use torro_ipc::{ApprovalInfo, Request, Response, VoiceStatus};
use torro_settings_ui::assistant::{self as settings, AppAction};
use torro_settings_ui::{Host, Outcome};

use crate::backend::{Backend, Refusal};
use crate::bots::{BotDraft, DraftEdit, Proposal};
use crate::chat::{self, Choice, Transcript};
use crate::demo;
use crate::launch::{Launch, Open, Window};
use crate::stream::{self, Smoother};

/// The main window's identity: window class `torro-assistant`, binary `torroassistant`, state in
/// `torroassistant/`.
pub const MAIN: AppId = AppId::ASSISTANT;
/// The key in `ui.json` of the expanded sidebar's width (issue #604).
const SIDE_KEY: &str = "sidebar";

/// A single chat window's identity: its own window class `torro-assistant-chat` (rules of a tiling
/// manager match on it), the binary and state directory of the assistant.
pub const CHAT: AppId = AppId {
    window_class: "torro-assistant-chat",
    binary: AppId::ASSISTANT.binary,
    dir: AppId::ASSISTANT.dir,
    display_name: AppId::ASSISTANT.display_name,
    // A chat window starts with `--chat <id>|new`, which names the conversation; no entry of its own.
    args: &[],
};

/// The size a chat window opens at: a column of conversation, not the main window's two parts.
pub const CHAT_SIZE: Size = Size::new(560.0, 760.0);

/// How often the window asks for the conversations, the engines, the client and the open
/// conversation while nothing runs. The core is local and cheap to ask.
const REFRESH: Duration = Duration::from_secs(5);

/// How often the window follows an answer that runs: often enough that the text comes in small
/// pieces, which [`Stream`] spreads evenly over the frames. Only while an answer runs; the core is
/// local and the request small.
const FOLLOW: Duration = Duration::from_millis(50);

/// How often the window follows an answer that waits for the person's approval: nothing is
/// written meanwhile, only the decision (in the tray or the inbox) is looked for.
const FOLLOW_WAITING: Duration = Duration::from_millis(500);

/// What the first frame after a pause counts: one frame at 60 Hz, so the pause itself is not
/// caught up at once.
const FRAME: Duration = Duration::from_micros(16_667);

/// How often a shown notice checks whether its time is up.
const NOTICE_TICK: Duration = Duration::from_secs(1);

/// The id of the composer's text, which takes the focus in a new chat.
pub const EDITOR: &str = "assistant-composer";
/// The id of the one-line composer of mini and strip.
pub const MINI_INPUT: &str = "assistant-mini";
/// The id of the sidebar's search field.
pub const SEARCH: &str = "assistant-search";

/// From this many characters on (trimmed) the sidebar's search looks through the history of every
/// conversation (issue #704); below, it filters the titles only.
pub const SEARCH_FROM: usize = 2;
/// The id of the bot editor's name field: "Save" goes there while the name keeps it back.
pub const BOT_NAME: &str = "assistant-bot-name";
/// The id of the bot editor's scrolling form: a "Save" that finds a problem goes to its top.
pub const BOT_FORM: &str = "assistant-bot-form";
/// The id of the title field in the rename dialog.
pub const RENAME: &str = "assistant-rename";
/// The section AI of the window's own settings (issue #410), where accounts are added and their
/// models turned on.
pub const AI_PAGE: &str = settings::AI_SECTION;

/// How often the window asks how the conversation by voice stands (issue #747) while it has none of
/// its own: a "Talk" started elsewhere, or "Hey Torro", shows in its conversation.
const TALK_EVERY: Duration = Duration::from_secs(2);

/// How often the window asks while its own conversation by voice runs: the line over the composer
/// follows the listener (listening, recording, answering).
const TALK_OWN_EVERY: Duration = Duration::from_millis(500);
/// The section Voice of the window's settings, where speech recognition is set up (issue #699).
pub const VOICE_PAGE: &str = "voice";

/// The id of the scrolling entries of the open conversation.
pub const HISTORY: &str = "assistant-history";

/// What a test can see of the requests the window sent (tasks do not run in tests).
#[cfg(test)]
pub type Sent = Vec<Request>;

/// How the window takes the answer to a request: the map its task would run, kept in tests next to
/// [`Sent`] so a test answers each request as the window asked for it.
#[cfg(test)]
pub type Reply = Box<dyn Fn(Result<Response, Refusal>) -> Message + Send>;

/// An open menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Menu {
    /// The engine and model picker of the composer.
    Picker,
    /// The permission mode under the composer (issue #132).
    Mode,
    /// The areas of the composer ("Without Mail").
    Areas,
    /// The menu of a conversation: Rename, Pin, Open in new window, Delete; at the header's "…"
    /// or, for a right click in the sidebar, at its row.
    Conversation { id: String, at: MenuAt },
    /// The menu of a bot in the sidebar (right click): New chat, Edit, Access, Open in new window,
    /// Delete.
    Bot { id: String },
    /// The "…" of the sidebar's group "Recent" (issue #697): Archive all…, Delete all….
    Recent,
    /// The "…" of the group "Archive", open or closed (issue #697): Empty archive….
    Archive,
    /// The engine and model picker of the bot editor.
    BotPicker,
}

/// The microphone of the composer (issue #699).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dictation {
    /// Nothing is recorded.
    #[default]
    Idle,
    /// The core records (`assistant.dictate` is out); a press stops it.
    Recording,
    /// The recording was stopped and the speech turns into text.
    Stopping,
}

/// Where a conversation's menu opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAt {
    Header,
    Row,
}

/// An open dialog over the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    /// Rename the conversation `id`; `title` is what is typed.
    Rename { id: String, title: String },
    /// Delete the conversation `id` (named `title`) for good, after asking.
    Delete { id: String, title: String },
    /// Delete the bot `id` (named `name`) for good, after asking.
    DeleteBot { id: String, name: String },
    /// Delete the conversations `ids` for good, after asking (issue #697): those under "Recent"
    /// ("Delete all…") or, with `archive`, the whole archive ("Empty archive…").
    DeleteMany { ids: Vec<String>, archive: bool },
    /// Archive the conversations `ids` under "Recent", after asking ("Archive all…", issue #697).
    ArchiveMany { ids: Vec<String> },
}

/// What the window knows of the assistant's own client (kind `assistant`).
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant, reason = "one value per window; the rights grew with MCP servers (#302)")]
pub enum Access {
    /// Not asked yet, or the core did not answer.
    Unknown,
    /// There is none yet: the first start.
    Missing,
    /// It exists with these rights.
    Granted(Rights),
}

/// Who chose what answers the next message; the window only replaces its own choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chosen {
    /// The window, from the newest conversation or the first engine.
    Default,
    /// The open conversation's engine and model.
    Conversation,
    /// The person, in the picker.
    Person,
}

/// A quiet notice at the bottom of the window (`widgets::toast`), gone after
/// [`widgets::TOAST_FOR`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub shown: Instant,
}

/// One full or partial answer of `assistant.get_conversation` or `assistant.send_message`.
pub type Loaded = Result<(ConversationSummary, Vec<Entry>), Refusal>;

/// The answer that appears letter by letter (issue #135).
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    /// The answer's number in the transcript.
    pub seq: u64,
    pub smoother: Smoother,
    /// The time of the last frame; `None` while nothing is left to show, so the frame after a
    /// pause counts as one ([`FRAME`]).
    pub frame: Option<Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The band above the content was pressed: drag the window (see `shell::titlebar`).
    DragWindow,
    /// Another start of the assistant asked for its main window (issue #279,
    /// [`torro_gui::instance`]): bring it to the front.
    Raise,
    /// The window has this size (at start and after every resize).
    Resized(Size),
    /// Every [`REFRESH`]: ask for everything again.
    Tick,
    /// The core answered at the start, or did not within `CORE_START_WAIT` (issue #206): the first
    /// load follows either way.
    CoreReady,
    /// Every [`FOLLOW`] while an answer runs: ask for the open conversation.
    Follow,
    /// A frame of the window, while an answer appears letter by letter.
    Frame(Instant),
    /// The history scrolled, or its entries grew.
    HistoryScrolled(scrollable::Viewport),
    /// The conversations, pinned first, then the newest.
    Listed(Result<Vec<ConversationSummary>, Refusal>),
    /// The engines the assistant can answer with.
    Engines(Result<Vec<Engine>, Refusal>),
    /// The models of an AI account (by engine id).
    Models(String, Result<Vec<AiModel>, Refusal>),
    /// The suite's settings: the default for new chats (issue #131). Boxed: they are by far the
    /// largest answer, and every other message would carry their size.
    Settings(Result<Box<Settings>, Refusal>),
    /// The rights of the assistant's own client; `Ok(None)` when there is none.
    Client(Result<Option<Rights>, Refusal>),
    /// The assistant's profile (task C.10): its name and picture for the header and the welcome.
    Profile(Result<Profile, Refusal>),
    /// The bots (`bots.list_bots`, task B.3).
    Bots(Result<Vec<BotSummary>, Refusal>),
    /// A bot in the sidebar, "Open chat" under a tool line: the bot's chat, its newest conversation
    /// or a new one.
    OpenBot(String),
    /// "New chat" in a bot's menu: a new conversation with this bot.
    NewBotChat(String),
    /// "New bot": the editor of a new bot.
    NewBot,
    /// "Edit bot": asks the core for the whole bot, then opens the editor.
    EditBot(String),
    BotLoaded(Result<Bot, Refusal>),
    /// A field of the bot editor changed.
    Draft(DraftEdit),
    /// "Save" in the bot editor.
    SaveBot,
    BotSaved(Result<Bot, Refusal>),
    /// "Cancel" in the bot editor: back to the chat, nothing saved.
    CloseEditor,
    /// "Delete bot…": asks first.
    AskDeleteBot(String),
    ConfirmDeleteBot,
    BotDeleted(String, Result<(), Refusal>),
    /// "Access…", "What the bot may use": the settings at Access at this bot (task S.34).
    BotAccess(String),
    /// "Open in new window" of a bot: `assistant.open_window { bot }`.
    OpenBotWindow(String),
    /// "Ask {bot}" on the card of the assistant's proposal: a person's click.
    ConfirmBot(Proposal),
    /// The core's answer to the confirmation.
    BotConfirmed(Proposal, Result<(), Refusal>),
    /// The whole conversation with this id.
    Opened(String, Loaded),
    /// What is new in the conversation with this id since the number asked for.
    Followed(String, Loaded),
    /// An approval the conversation waits for (or waited for).
    Approval(String, Result<ApprovalInfo, Refusal>),
    /// "Allow", "Deny" or, for a command of the terminal, how far it is allowed, on the approval card: a person's
    /// click.
    Decide(String, chat::Answer),
    /// "Show all" or "Show less" of the command on the approval card with this id (issue #754).
    ToggleCommand(String),
    /// The cards of the conversation with this id as the core resolved them (task WZ.8).
    Cards(String, Result<Vec<CardEntry>, Refusal>),
    /// "Open" on the card of the file with this id: a person's click.
    OpenFile(String),
    /// "Save as…" on the card of the file `id`: the system's save dialog, suggesting `name`.
    SaveFileAs {
        id: String,
        name: String,
    },
    /// The save dialog for the file with this id closed: where the copy goes, `None` when it was
    /// cancelled.
    SavePicked(String, Option<PathBuf>),
    /// "Send by mail" on the card of the file with this id: a new mail with it attached.
    MailFile(String),
    /// "Open in Mail" on a suggestion's card (issue #605): the mail window on the mail `id` of
    /// `account`, where the suggestion is accepted.
    OpenMail {
        account: String,
        id: String,
    },
    /// The core's answer to a button of a card; `Ok` with what to say then, the text says what
    /// failed.
    FileDone(Result<Option<&'static str>, Refusal>, &'static str),
    /// A button of a content card (issue #784): sends the request as the person's click; when it fails the window
    /// says the text. Either way the cards are asked for again, so a card shows what the click changed (a link shown
    /// once, a task done, a draft sent).
    CardRequest(Box<Request>, &'static str),
    /// The core's answer to a [`Message::CardRequest`], with the text to say when it failed.
    CardAnswered(Result<(), Refusal>, &'static str),
    /// "Try again" on a card whose server could not be reached (S3 of issue #784): the cards are asked for again
    /// (`assistant.get_cards`).
    RetryCards,
    /// The picture of the link card `seq` of the conversation (issue #785, `assistant.get_card_picture`), as the
    /// core sent it.
    CardPicture(String, u64, Result<torro_ipc::chat::ChatPicture, Refusal>),
    /// The core's answer to the decision.
    Decided(String, Result<ApprovalInfo, Refusal>),
    /// From the sidebar: show the conversation with this id.
    Select(String),
    /// "New chat".
    NewChat,
    /// The composer's text changed.
    Edit(text_editor::Action),
    /// The one-line composer of mini and strip changed.
    MiniInput(String),
    /// "Send", `Enter` in the composer.
    Send,
    /// The core took the message.
    Sent(Loaded),
    /// "Stop": end the answer that runs.
    Stop,
    Stopped(Result<(), Refusal>),
    /// From the picker.
    Pick(Choice),
    /// From the effort's menu (issue #131); `None` for "Automatic".
    PickEffort(Option<Effort>),
    /// From the permission mode's menu (issue #132).
    PickMode(PermissionMode),
    /// How talking by voice stands (`settings.voice_status`, issue #699): whether the microphone
    /// can dictate.
    Voice(Result<VoiceStatus, Refusal>),
    /// The microphone of the composer (issue #699): records, stops a recording, or – without speech
    /// recognition – opens Settings › Voice.
    Dictate,
    /// What was dictated, as text; empty when nothing was understood.
    Dictated(Result<String, Refusal>),
    /// The core took "stop dictating"; the text comes with [`Message::Dictated`].
    DictationStopped,
    /// The speaker of the composer (issue #745): reading answers aloud on or off.
    ToggleReadAloud,
    /// The core took the switch, or refused it (then it goes back).
    ReadAloudSet(bool, Result<(), Refusal>),
    /// "Stop" under the answer being read (issue #745, #774): reading stops at once.
    StopReading,
    /// "Repeat" under an answer in two parts (issue #774): its spoken part, by the answer's number, is
    /// read aloud again (`assistant.speak`).
    RepeatSpoken(u64),
    /// The repeat with this number ([`App::repeating`]) was read to its end, stopped, or failed.
    Repeated(u64, Result<(), Refusal>),
    /// The core took "stop reading"; [`Message::Read`] follows for the answer.
    ReadingStopped,
    /// The answer `seq` of the conversation was read to its end, stopped, or could not be read.
    Read(Option<String>, u64, Result<(), Refusal>),
    /// "Talk" in the composer (issue #747): a conversation by voice in the open conversation.
    Talk,
    /// The core started the conversation by voice, or refused it.
    TalkStarted(Result<Option<torro_ipc::Talk>, Refusal>),
    /// "End talk" over the composer: the microphone closes, reading stops.
    EndTalk,
    /// The core ended the conversation by voice, or refused it.
    TalkEnded(Result<(), Refusal>),
    /// Time to ask how the conversation by voice stands ([`TALK_EVERY`], [`TALK_OWN_EVERY`]).
    TalkTick,
    /// How the conversation by voice stands (`assistant.talk_status`), and whether anything is read
    /// aloud now (issue #774).
    TalkStatus(Result<(Option<torro_ipc::Talk>, bool), Refusal>),
    /// "Manage models…" at the end of the picker and under "No model is turned on" (task S.26): the
    /// section AI of the settings ([`AI_PAGE`]), where the models of each AI account are turned on and off.
    ManageModels,
    /// Whether there is an AI account at all (`accounts.list_accounts`), asked while no AI account
    /// is an engine (task S.26): then no model is on, or there is no AI account.
    AiAccounts(Result<bool, Refusal>),
    /// Switch an area off for this conversation, or on again.
    ToggleArea(Area),
    /// Unfold the tool line with this number, or fold it.
    ToggleTool(u64),
    /// Open or close a menu.
    ToggleMenu(Menu),
    /// A click beside a menu, or Escape.
    CloseMenu,
    /// "Open in new window" for the conversation with this id.
    OpenInWindow(String),
    WindowOpened(Result<(), Refusal>),
    /// "Rename" for the conversation with this id.
    AskRename(String),
    RenameInput(String),
    ConfirmRename,
    /// "Delete" for the conversation with this id: asks first.
    AskDelete(String),
    ConfirmDelete,
    /// "Pin" (`true`) or "Unpin".
    Pin(String, bool),
    /// "Archive" (`true`) or "Restore" for these conversations (issue #697).
    Archive(Vec<String>, bool),
    /// Open or close the sidebar's group "Archive".
    ToggleArchive,
    /// "Archive all…", "Delete all…" at "Recent", "Empty archive…" at "Archive": asks first.
    AskArchiveAll,
    AskDeleteAll,
    AskEmptyArchive,
    /// The question's "Archive … conversations" or "Delete … conversations".
    ConfirmMany,
    /// These conversations were deleted (or not) at once.
    DeletedMany(Vec<String>, Result<(), Refusal>),
    /// The core's answer to a rename or pin; the text says what failed.
    Changed(Result<(), Refusal>, &'static str),
    /// The conversation with this id was deleted (or not).
    Deleted(String, Result<(), Refusal>),
    /// Close the dialog.
    CloseDialog,
    /// What is typed into the sidebar's search.
    Search(String),
    /// The core's hits for the search with these words (issue #704).
    Hits(String, Result<Vec<ConversationHit>, Refusal>),
    /// A hit of the search: open its conversation at the entry with this number (`None`: the title).
    OpenHit(String, Option<u64>),
    /// The pointer is on the sidebar row of this conversation (`None`: on none).
    Hover(Option<String>),
    /// From the tools at the top of the sidebar: search and "New".
    Switcher(Switcher),
    /// Start or raise another app of the suite.
    OpenApp(String),
    AppOpened(Result<(), String>),
    /// "Set up access", "Change access": the settings at Access (task S.34), where the person
    /// chooses what the assistant – or the bot of the open conversation – may use.
    SetUp,
    /// "Add an AI account": the section AI of the settings ([`AI_PAGE`]), where AI accounts are added.
    AddAiAccount,
    /// The gear in the title band, "Settings" in the sidebar: the settings in the content area, or
    /// back to the chat (task S.18).
    ToggleSettings,
    /// One of the settings' own messages ([`Host`]).
    SettingsHost(torro_settings_ui::host::Message),
    /// What a widget of the open settings section reported.
    SettingsPage(settings::Message),
    /// A link in an answer.
    OpenLink(String),
    /// "Retry" in the error state.
    Retry,
    /// The panel button, `[`.
    ToggleSidebar,
    UiState(UiState),
    /// The handle at the edge of the expanded sidebar (issue #604).
    SidebarWidth(SplitEvent),
    /// The widths stored in `ui.json`, loaded at start.
    StoredWidths(HashMap<String, f32>),
    Escape,
    NoticeTick(Instant),
    Appearance(Result<Appearance, String>),
    SystemMode(Mode),
    Zoom(zoom::Message),
    Design(design::Message),
}

/// The settings in the content area while they are open (task S.18): the library's [`Host`] with
/// the assistant's sections, and the state those sections keep (a note being written, what "Say
/// something" heard).
pub struct SettingsPane {
    pub(crate) host: Host,
    pub(crate) sections: settings::State,
}

pub struct App {
    pub(crate) backend: Backend,
    /// The suite behind the window: appearance, zoom, opening other apps (none with `--demo`).
    pub(crate) suite: Source,
    pub(crate) launch: Launch,
    /// The main window or a chat window.
    pub(crate) chat_only: bool,
    /// The one-time states of `--open` that need data are done.
    pub(crate) opened: bool,
    pub(crate) size: Size,

    /// The conversations, as last listed.
    pub(crate) conversations: Vec<ConversationSummary>,
    /// They have come once.
    pub(crate) listed: bool,
    /// Until the core answered at the start ([`Message::CoreReady`], issue #206): the timer waits too,
    /// so it cannot show an error for a core that is still starting.
    waiting: bool,
    /// Why the last listing failed.
    pub(crate) list_error: Option<String>,
    /// The engines in the picker's order ([`chat::ordered`]).
    pub(crate) engines: Vec<Engine>,
    pub(crate) engines_loaded: bool,
    /// The models of each AI account, by engine id, as listed with closed ones too
    /// (`ai.list_models { all }`): the picker offers those turned on ([`App::offered_models`]) and
    /// names the others a choice may still hold.
    pub(crate) models: HashMap<String, Result<Vec<AiModel>, String>>,
    /// There is an AI account (task S.26): `true` once an AI account is an engine, else as
    /// `accounts.list_accounts` last said. With none as an engine, it tells "No model is turned on"
    /// from "No AI account yet".
    pub(crate) ai_accounts: bool,
    /// What a new chat starts with, as set in the control centre (`settings.ai_slots.chat`);
    /// `None`: the one used last.
    pub(crate) default: Option<AssistantDefault>,
    pub(crate) access: Access,
    /// The assistant's profile as last read; `None` until the core answered (or with an old core):
    /// then the window shows [`torro_ipc::assistant::DEFAULT_NAME`] and its letter.
    pub(crate) profile: Option<Profile>,
    /// The bots, as last listed (task B.3).
    pub(crate) bots: Vec<BotSummary>,
    /// They have come once.
    pub(crate) bots_loaded: bool,
    /// The bot a new chat goes to; `None` for the assistant. An open conversation answers with its
    /// own bot ([`App::chat_bot`]).
    pub(crate) bot: Option<String>,
    /// The bot editor, while it is open: it takes the chat's place.
    pub(crate) bot_draft: Option<BotDraft>,
    /// A `--bot` window has looked for the bot's newest conversation.
    pub(crate) bot_resolved: bool,
    /// The one-time states of `--open` that need the bots are done.
    pub(crate) opened_bots: bool,
    /// The proposal whose confirmation is on its way: its card takes no second click.
    pub(crate) confirming: Option<Proposal>,

    /// The open conversation; `None` is a new chat.
    pub(crate) current: Option<String>,
    pub(crate) transcript: Transcript,
    /// Why the open conversation could not be loaded.
    pub(crate) chat_error: Option<String>,
    /// A request for the open conversation is out; the timer does not send another.
    pub(crate) following: bool,
    /// The approvals of the open conversation, by id.
    pub(crate) approvals: HashMap<String, ApprovalInfo>,
    /// The approval whose decision is on its way: its buttons are disabled until the core answered.
    pub(crate) deciding: Option<String>,
    /// The cards of the open conversation as the core resolved them last (`assistant.get_cards`,
    /// task WZ.8), by the number of their entry; a card not resolved yet is not drawn.
    pub(crate) cards: HashMap<u64, Card>,
    /// The pictures of link cards (issue #785, `assistant.get_card_picture`), by conversation and the card's entry,
    /// decoded to draw ([`App::fetch_card_picture`]).
    pub(crate) card_pictures: HashMap<(String, u64), iced::widget::image::Handle>,
    /// The link cards whose picture was asked for, by conversation and entry: asked once per card, also when the core
    /// had none, until the card changes ([`App::take_cards`]).
    pub(crate) card_pictures_asked: HashSet<(String, u64)>,
    /// The tool lines unfolded, by number.
    pub(crate) expanded: HashSet<u64>,
    /// The approval cards whose command of the terminal shows whole, by approval id (issue #754).
    pub(crate) whole_commands: HashSet<String>,
    /// The answers as Markdown blocks, by number, with the text they were made from: for the
    /// answer in [`App::stream`] what of it shows ([`crate::stream::display`]).
    pub(crate) answers: HashMap<u64, (String, Vec<markdown::Item>)>,
    /// The answer that appears letter by letter; `None` when every answer shows whole.
    pub(crate) stream: Option<Stream>,
    /// The history is scrolled to the bottom and follows the newest entry; scrolled up, it stays.
    pub(crate) follow_bottom: bool,
    /// The history as the last scroll event saw it, as (content height, offset): tells growth from scrolling.
    history_seen: Option<(f32, f32)>,

    /// The composer's text.
    pub(crate) editor: Content,
    /// The one-line composer of mini and strip.
    pub(crate) mini: String,
    pub(crate) choice: Option<Choice>,
    /// How hard the model of the next message thinks (issue #131); sent only where it takes one
    /// ([`chat::can_reason`]).
    pub(crate) effort: Option<Effort>,
    pub(crate) chosen: Chosen,
    /// The areas switched off for the next message.
    pub(crate) without: Vec<Area>,
    /// The permission mode of the next message (issue #132): it only narrows the rights.
    pub(crate) mode: PermissionMode,
    /// The areas and the mode of the open conversation's last message are taken once it is loaded.
    pub(crate) take_without: bool,
    /// The message is on its way.
    pub(crate) sending: bool,
    /// How talking by voice stands, as last asked (issue #699); `None` until the core answered.
    pub(crate) voice: Option<VoiceStatus>,
    /// The microphone of the composer.
    pub(crate) dictation: Dictation,
    /// How voice stands was asked once outside mini and strip.
    voice_asked: bool,
    /// Answers are read aloud (issue #745, the setting `voice.read_aloud`).
    pub(crate) read_aloud: bool,
    /// The answer being read aloud now, by its number; [■] shows at it.
    pub(crate) reading: Option<u64>,
    /// The conversation of [`App::reading`]: numbers count per conversation.
    pub(crate) reading_in: Option<String>,
    /// The speaker was switched and the core has not taken it yet: settings loaded meanwhile (every
    /// [`REFRESH`]) may still say the old value, and must not switch it back.
    read_aloud_switching: bool,
    /// New answers that came while the window watches and are not complete yet: each is read once
    /// it is.
    to_read: HashSet<u64>,
    /// A new answer that just began, to read aloud from its first sentence ([`App::read_ready`], issue
    /// #747): its number.
    ready_to_read: Option<u64>,
    /// The message whose answer was asked to be read (issue #747): its conversation and number, so
    /// every answer is read once, also one in several parts.
    read_for: Option<(Option<String>, u64)>,
    /// The conversation by voice as the core last told (issue #747, `assistant.talk_status`).
    pub(crate) talk: Option<torro_ipc::Talk>,
    /// The "Repeat" that reads now, by a number of its own (issue #774): the reading it replaced ends
    /// late, and must not take "Stop" away from it.
    repeating: Option<u64>,
    /// The number of the last "Repeat".
    repeats: u64,
    /// Something was read aloud when the core last told how the conversation by voice stands (issue
    /// #774): "Stop" under its last answer is active.
    pub(crate) talk_speaking: bool,
    /// This window started [`App::talk`] in a new chat: it follows it into the conversation its first
    /// sentence starts.
    talk_started: bool,
    /// "Talk" was pressed and the core has not answered yet: the button shows it is busy.
    pub(crate) talk_starting: bool,

    pub(crate) menu: Option<Menu>,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) search: String,
    /// The core's hits for [`App::search`] (issue #704): `None` while there is no search through the
    /// history (fewer than [`SEARCH_FROM`] characters) or before its first answer.
    pub(crate) hits: Option<Result<Vec<ConversationHit>, Refusal>>,
    /// The words [`App::hits`] answer: the sidebar shows them while the field holds these words, or while the
    /// answer for the words in the field is on its way ([`App::asked`]), so the list does not jump at each key.
    pub(crate) hits_for: String,
    /// The words of the last search sent to the core.
    pub(crate) asked: String,
    /// The hit opened last (conversation, entry): its entry shows marked until another conversation is
    /// chosen or the search is cleared.
    pub(crate) marked: Option<(String, u64)>,
    /// The entry to scroll to once its conversation is opened.
    pub(crate) snap: Option<(String, u64)>,
    /// The sidebar row under the pointer.
    pub(crate) hover: Option<String>,
    /// Whether the sidebar's group "Archive" is open (issue #697): closed at every start, not kept.
    pub(crate) archive_open: bool,
    /// Whether the sidebar drawer is open (medium and narrow layouts).
    pub(crate) drawer: bool,
    /// The sidebar the user chose for the wide layout, kept in `ui.json`.
    pub(crate) sidebar: SidebarMode,
    /// The width the person dragged the expanded sidebar to (issue #604), kept in `ui.json`.
    pub(crate) side: Split,
    pub(crate) notice: Option<Notice>,
    /// The settings in the content area, while they are open (task S.18).
    pub(crate) settings: Option<SettingsPane>,
    /// What the settings showed when they last closed, for the next time they open (issue #394):
    /// they are there at once with it and update in place.
    settings_memory: torro_settings_ui::Memory,
    /// What a demo state presses on its settings page once their first load is in (issue #547,
    /// [`settings_state`]); empty otherwise.
    settings_steps: Vec<settings::Message>,
    pub(crate) appearance: Appearance,
    pub(crate) system_mode: Mode,
    pub(crate) zoom: Zoom,
    pub(crate) design: Follow,
    #[cfg(test)]
    pub(crate) sent: Sent,
    /// How the window takes the answer to each request in [`App::sent`], in the same order.
    #[cfg(test)]
    pub(crate) replies: Vec<Reply>,
    /// The control centre's pages the window opened, for the tests.
    #[cfg(test)]
    pub(crate) sections: Vec<String>,
}

impl App {
    pub fn new(backend: Backend, suite: Source, launch: Launch) -> (App, Task<Message>) {
        let open = launch.open;
        let (chat_only, current) = match &launch.window {
            Window::Main => (false, start_conversation(open)),
            Window::Chat(id) => (true, id.clone()),
            // The bot's newest conversation is known once the conversations are listed.
            Window::Bot(_) => (true, None),
        };
        let bot = match (&launch.window, open) {
            (Window::Bot(id), _) => Some(id.clone()),
            (Window::Main, Some(Open::BotWelcome)) => Some(demo::TRAVEL.to_owned()),
            _ => None,
        };
        let mut app = App {
            backend,
            zoom: Zoom::for_source(&suite),
            design: Follow::for_source(&suite),
            suite,
            chat_only,
            opened: false,
            size: if chat_only { CHAT_SIZE } else { torro_gui::window(MAIN).size },
            conversations: Vec::new(),
            listed: false,
            waiting: true,
            list_error: None,
            engines: Vec::new(),
            engines_loaded: false,
            models: HashMap::new(),
            ai_accounts: false,
            default: None,
            access: Access::Unknown,
            profile: None,
            bots: Vec::new(),
            bots_loaded: false,
            bot,
            bot_draft: None,
            bot_resolved: false,
            opened_bots: false,
            confirming: None,
            current,
            transcript: Transcript::default(),
            chat_error: None,
            following: false,
            approvals: HashMap::new(),
            deciding: None,
            cards: HashMap::new(),
            card_pictures: HashMap::new(),
            card_pictures_asked: HashSet::new(),
            expanded: HashSet::new(),
            whole_commands: HashSet::new(),
            answers: HashMap::new(),
            stream: None,
            follow_bottom: true,
            history_seen: None,
            editor: Content::new(),
            mini: String::new(),
            choice: None,
            effort: None,
            chosen: Chosen::Default,
            without: if open == Some(Open::AreasOff) { vec![Area::Mail, Area::Calendar] } else { Vec::new() },
            mode: PermissionMode::Default,
            take_without: true,
            sending: false,
            voice: None,
            voice_asked: false,
            read_aloud: open == Some(Open::Reading),
            reading: (open == Some(Open::Reading)).then_some(demo::READ_ANSWER),
            reading_in: (open == Some(Open::Reading)).then(|| demo::TOOLS.to_string()),
            read_aloud_switching: false,
            to_read: HashSet::new(),
            ready_to_read: None,
            read_for: None,
            talk: (open == Some(Open::Talk)).then(|| torro_ipc::Talk {
                conversation: Some(demo::TOOLS.to_string()),
                state: torro_ipc::VoiceState::Conversing,
            }),
            talk_speaking: open == Some(Open::Talk),
            repeating: None,
            repeats: 0,
            talk_started: false,
            talk_starting: false,
            dictation: if open == Some(Open::Dictating) { Dictation::Recording } else { Dictation::Idle },
            menu: match open {
                Some(Open::Picker | Open::Effort) => Some(Menu::Picker),
                Some(Open::Mode) => Some(Menu::Mode),
                Some(Open::Menu) => Some(Menu::Conversation { id: demo::TOOLS.into(), at: MenuAt::Header }),
                // A row under "Recent" (issue #697): the menu hangs at its "…", which stays shown.
                Some(Open::RowMenu) => Some(Menu::Conversation { id: demo::STREAMING.into(), at: MenuAt::Row }),
                Some(Open::RecentMenu) => Some(Menu::Recent),
                Some(Open::BotMenu) => Some(Menu::Bot { id: demo::TRAVEL.into() }),
                _ => None,
            },
            dialog: None,
            search: String::new(),
            hits: None,
            hits_for: String::new(),
            asked: String::new(),
            marked: None,
            snap: None,
            hover: None,
            archive_open: open == Some(Open::Archive),
            // Without a sidebar the bot's menu hangs in the drawer.
            drawer: matches!(
                open,
                Some(Open::Drawer | Open::BotMenu | Open::RowMenu | Open::RecentMenu | Open::Archive)
            ),
            sidebar: SidebarMode::default(),
            side: Split::default(),
            notice: None,
            settings: None,
            settings_memory: torro_settings_ui::Memory::default(),
            settings_steps: Vec::new(),
            appearance: Appearance::System,
            system_mode: Mode::None,
            launch,
            #[cfg(test)]
            sent: Vec::new(),
            #[cfg(test)]
            replies: Vec::new(),
            #[cfg(test)]
            sections: Vec::new(),
        };
        let size = Task::batch([
            iced::window::latest().and_then(iced::window::size).map(Message::Resized),
            torro_gui::place_traffic_lights(),
        ]);
        // The first load waits for a core that is still starting (issue #206).
        let suite = app.suite.clone();
        let mut tasks = vec![
            Task::perform(async move { suite.ready().await }, |_| Message::CoreReady),
            app.fetch_appearance(),
            app.zoom.fetch().map(Message::Zoom),
            app.design.fetch().map(Message::Design),
            iced::system::theme().map(Message::SystemMode),
            size,
        ];
        if !app.chat_only {
            tasks.push(ui_state::load_task(MAIN).map(Message::UiState));
            // The demo reads no widths of the user's: its pictures stay the same.
            if !app.launch.demo {
                tasks.push(ui_state::load_splits_task(MAIN).map(Message::StoredWidths));
            }
        }
        if app.current.is_none() {
            tasks.push(operation::focus(EDITOR));
        }
        // `--settings <section>`: the window opens with the settings in its content area; Access in
        // a bot's window (`--bot <id> --settings access`) at that bot's rights, below Bots since
        // issue #763.
        if let Some(section) = app.launch.settings.clone() {
            match (&app.launch.window, section.as_str()) {
                (Window::Bot(bot), settings::access::ID) => {
                    let bot = bot.clone();
                    tasks.push(app.open_access(Some(bot)));
                }
                _ => tasks.push(app.open_settings(Some(&section))),
            }
        }
        // A demo state of the settings (issue #547): their section, and what is pressed on its page
        // once the data is in.
        if let Some((section, steps)) = settings_state(app.launch.open) {
            if app.settings.is_none() {
                tasks.push(app.open_settings(Some(section)));
            }
            app.settings_steps = steps;
        }
        (app, Task::batch(tasks))
    }

    /// The window's title: the conversation's ("New chat" for a new one); the main window says
    /// whose window it is first, a bot's chat window the bot's name ("Travel planner · Train to
    /// Lisbon"). Tiling rules and window lists tell chat windows apart by it.
    pub fn title(&self) -> String {
        let title = self.conversation_title();
        match (self.chat_only, self.chat_bot()) {
            (true, Some(bot)) => trf("{bot} · {title}", &[("bot", &self.bot_name(bot)), ("title", &title)]),
            (true, None) => title,
            (false, _) => trf("Assistant · {title}", &[("title", &title)]),
        }
    }

    /// The open conversation's title, "New chat" for a new one.
    pub(crate) fn conversation_title(&self) -> String {
        let listed = self.current.as_ref().and_then(|id| self.conversations.iter().find(|c| &c.id == id));
        match (self.transcript.summary.as_ref(), listed) {
            (Some(summary), _) if self.current.as_ref() == Some(&summary.id) => summary.title.clone(),
            (_, Some(summary)) => summary.title.clone(),
            _ if self.current.is_some() => tr("Conversation").to_owned(),
            _ => tr("New chat").to_owned(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            torro_gui::theme::system_mode_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            self.zoom.subscription().map(Message::Zoom),
            self.design.subscription().map(Message::Design),
            // A second start of the assistant brings the main window to the front (issue #279); a
            // chat window has no channel, so nothing ever arrives there.
            torro_gui::instance::subscription().map(|()| Message::Raise),
        ];
        // Only while an answer runs (or a message is on its way): a window at rest asks rarely.
        if self.transcript.running() || self.sending {
            let every = if self.waits_for_approval() { FOLLOW_WAITING } else { FOLLOW };
            subscriptions.push(iced::time::every(every).map(|_| Message::Follow));
        }
        // Frames only while letters wait to show: a window at rest draws nothing.
        if self.stream.as_ref().is_some_and(|stream| stream.smoother.revealing()) {
            subscriptions.push(iced::window::frames().map(Message::Frame));
        }
        if self.dialog.is_some()
            || self.menu.is_some()
            || self.drawer
            || self.notice.is_some()
            || self.settings.is_some()
            || self.own_talk().is_some()
        {
            subscriptions.push(widgets::escape().map(|()| Message::Escape));
        }
        if self.notice.is_some() {
            subscriptions.push(iced::time::every(NOTICE_TICK).map(Message::NoticeTick));
        }
        // What the open settings sections need a clock for: Voice asks how the listener stands
        // again after it was switched, until it reports what it does (task S.17). None while
        // nothing waits.
        if let Some(pane) = &self.settings {
            subscriptions.push(settings::subscription(&pane.sections).map(Message::SettingsPage));
        }
        if !self.chat_only && !self.is_glance() {
            subscriptions.push(iced::event::listen_with(window_key));
        }
        if !self.waiting {
            subscriptions.push(iced::time::every(REFRESH).map(|_| Message::Tick));
        }
        // How the conversation by voice stands (issue #747): often while it is this window's own,
        // so the line over the composer follows the listener. Not in mini and strip, nor in the demo
        // state `talk`, which keeps its own.
        if !self.waiting && !self.is_glance() && self.launch.open != Some(Open::Talk) {
            let every = if self.own_talk().is_some() { TALK_OWN_EVERY } else { TALK_EVERY };
            subscriptions.push(iced::time::every(every).map(|_| Message::TalkTick));
        }
        // A download under Settings › AI asks how far it is (issue #410).
        if let Some(pane) = &self.settings {
            subscriptions.push(pane.host.subscription().map(Message::SettingsHost));
        }
        // A drag of the sidebar's edge ends wherever the button is released.
        if self.side.dragging {
            subscriptions.push(split::release().map(|()| Message::SidebarWidth(SplitEvent::Release)));
        }
        Subscription::batch(subscriptions)
    }

    /// The Iced theme: the appearance of `--appearance`, else the stored one, with "System"
    /// following the desktop.
    pub fn theme(&self) -> Theme {
        theme::for_appearance(self.launch.appearance.unwrap_or(self.appearance), self.system_mode)
    }

    /// The factor the whole window draws at: the suite-wide zoom.
    pub fn scale_factor(&self) -> f32 {
        self.zoom.factor()
    }

    /// What the edge of the expanded sidebar moves in (issue #604): the conversation keeps room for
    /// two columns' least.
    pub(crate) fn sidebar_limits(&self) -> Limits {
        shell::sidebar_limits(self.size.width, 2.0 * split::min_pane() + split::divider_width())
    }

    /// The width of the expanded sidebar beside the conversation: dragged or `size::sidebar()`.
    pub(crate) fn sidebar_room(&self) -> f32 {
        self.side.width(self.sidebar_limits())
    }

    /// The width of the expanded sidebar as it shows now: dragged beside the conversation (wide),
    /// `size::sidebar()` as the drawer, which keeps its width.
    pub(crate) fn sidebar_shown(&self) -> f32 {
        match shell::sidebar_mode(self.class(), self.sidebar) {
            Some(SidebarMode::Expanded) => self.sidebar_room(),
            _ => torro_gui::theme::size::sidebar(),
        }
    }

    /// The width of the content beside the sidebar or rail ([`shell::content_width`]), less the
    /// sidebar as wide as it was dragged: what the settings' section list measures against.
    pub(crate) fn content_room(&self) -> f32 {
        let class = self.class();
        let content = shell::content_width(class, self.sidebar, self.size.width);
        match shell::sidebar_mode(class, self.sidebar) {
            Some(SidebarMode::Expanded) if !self.chat_only => {
                (content + torro_gui::theme::size::sidebar() - self.sidebar_room()).max(0.0)
            }
            _ => content,
        }
    }

    /// The width inside the content's border, what the settings' section list measures against:
    /// flush (inset 0) the panel has only the line beside the sidebar, inset its border on both sides.
    pub(crate) fn panes(&self) -> f32 {
        let lines = if layout::inset() <= 0.0 { 1.0 } else { 2.0 };
        (self.content_room() - lines * torro_gui::theme::size::HAIRLINE).max(0.0)
    }

    /// The layout class of the window.
    pub(crate) fn class(&self) -> SizeClass {
        layout::of(self.size)
    }

    /// Mini or strip: the newest answer and a one-line composer, nothing else.
    pub(crate) fn is_glance(&self) -> bool {
        matches!(self.class(), SizeClass::Mini | SizeClass::Strip)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Every way out of the settings asks the open section first (task S.18): a profile that is
        // typed and not saved must not vanish without "Discard changes?"
        // ([`App::settings_question`]).
        if let Some(question) = self.settings_question(&message) {
            return question;
        }
        // A change of rights made a moment ago goes out before the settings go (task S.34).
        if let Some(save) = self.save_before_leaving(&message) {
            return Task::batch([save, self.update(message)]);
        }
        match message {
            Message::DragWindow => return torro_gui::drag_window(),
            Message::Raise => return torro_gui::instance::bring_to_front(),
            Message::Resized(size) => {
                self.size = size;
                // Mini and strip have no [■] at the answer: reading stops as the window shrinks into
                // them, so it never runs on out of reach (issue #745).
                if self.reading.is_some() && self.is_glance() {
                    return self.stop_reading();
                }
                // A window that grows out of mini or strip gets its microphone (issue #699).
                if !self.waiting && self.voice.is_none() && !self.voice_asked && !self.is_glance() {
                    return self.load_voice();
                }
            }
            Message::CoreReady => {
                self.waiting = false;
                // The chat window's conversation too: it waited with the rest (issue #206).
                return Task::batch([self.load_all(), self.open_current(), self.load_voice(), self.fetch_talk()]);
            }
            Message::Tick => {
                let mut tasks = vec![self.load_all(), self.fetch_appearance(), self.zoom.fetch().map(Message::Zoom)];
                tasks.push(self.follow());
                // A file may have gone or come out of reach meanwhile.
                tasks.push(self.fetch_cards());
                return Task::batch(tasks);
            }
            Message::Follow => return self.follow(),
            Message::Frame(now) => self.frame(now),
            Message::HistoryScrolled(viewport) => return self.history_scrolled(viewport),
            Message::Listed(result) => return self.listed(result),
            Message::Engines(result) => return self.take_engines(result),
            Message::Models(engine, result) => {
                self.models.insert(engine, result.map_err(|refusal| refusal.message));
                self.choose_default();
            }
            Message::Settings(Ok(settings)) => {
                // The demo state `reading` shows the speaker on, whatever the demo's settings say.
                if self.launch.open != Some(Open::Reading) && !self.read_aloud_switching {
                    self.read_aloud = settings.voice.read_aloud;
                }
                self.default = settings.ai_slots.chat;
                self.choose_default();
            }
            // An old core or none: new chats start with the one used last.
            Message::Settings(Err(_)) => {}
            Message::Client(result) => {
                self.access = match result {
                    Ok(Some(rights)) => Access::Granted(rights),
                    Ok(None) => Access::Missing,
                    // An old core or none: no reason to ask the person to set anything up.
                    Err(_) => Access::Unknown,
                };
                let usable = self.switchable();
                self.without.retain(|area| usable.contains(area));
            }
            // An old core without profiles, or none: the window keeps what it showed.
            Message::Profile(Ok(profile)) => self.profile = Some(profile),
            Message::Profile(Err(_)) => {}
            Message::Bots(result) => return self.bots_listed(result),
            Message::OpenBot(id) => return self.open_bot(id),
            Message::NewBotChat(id) => return self.new_bot_chat(id),
            Message::NewBot => self.new_bot(),
            Message::EditBot(id) => return self.edit_bot(id),
            Message::BotLoaded(result) => self.bot_loaded(result),
            Message::Draft(edit) => {
                let pick = matches!(edit, DraftEdit::Engine(_));
                if let Some(draft) = self.bot_draft.as_mut() {
                    draft.apply(edit);
                }
                if pick {
                    self.menu = None;
                }
            }
            Message::SaveBot => return self.save_bot(),
            Message::BotSaved(result) => return self.bot_saved(result),
            Message::CloseEditor => {
                self.close_menus();
                self.bot_draft = None;
            }
            Message::AskDeleteBot(id) => self.ask_delete_bot(id),
            Message::ConfirmDeleteBot => return self.delete_bot(),
            Message::BotDeleted(id, result) => return self.bot_deleted(id, result),
            Message::BotAccess(id) => return self.open_access(Some(id)),
            Message::OpenBotWindow(id) => {
                self.close_menus();
                let request = Request::OpenChatWindow { window: ChatWindow::Bot(id) };
                return self.ask(request, |result| Message::WindowOpened(result.map(|_| ())));
            }
            Message::ConfirmBot(proposal) => return self.confirm_bot(proposal),
            Message::BotConfirmed(proposal, result) => return self.bot_confirmed(proposal, result),
            Message::Opened(id, loaded) => return self.opened(id, loaded),
            Message::Followed(id, loaded) => return self.followed(id, loaded),
            Message::Approval(id, Ok(approval)) => {
                self.approvals.insert(id, approval);
            }
            Message::Approval(id, Err(refusal)) => eprintln!("cannot read the approval {id}: {}", refusal.message),
            Message::Decide(id, answer) => return self.decide(id, answer),
            Message::ToggleCommand(id) => {
                if !self.whole_commands.remove(&id) {
                    self.whole_commands.insert(id);
                }
            }
            Message::Cards(id, result) => return self.take_cards(&id, result),
            Message::OpenFile(id) => {
                let failed = tr("Couldn't open the file.");
                let request = chat::open_file_request(&id);
                return self.ask(request, move |result| Message::FileDone(result.map(|_| None), failed));
            }
            // Only the person picks where a copy goes, in the system's dialog (point 9 of "Was nie
            // nach außen geht"); the name ends in `.pdf`, which the core then adds to no other file.
            Message::SaveFileAs { id, name } => {
                return Task::perform(pick_save_path(name), move |path| Message::SavePicked(id.clone(), path));
            }
            Message::SavePicked(_, None) => {}
            Message::SavePicked(id, Some(path)) => {
                let Some(request) = chat::save_file_request(&id, &path) else {
                    self.note(tr("This path cannot be used.").to_owned());
                    return Task::none();
                };
                let failed = tr("Couldn't save the copy.");
                let saved = tr("Copy saved");
                return self.ask(request, move |result| Message::FileDone(result.map(|_| Some(saved)), failed));
            }
            Message::MailFile(id) => {
                let failed = tr("Couldn't open a new mail with the file.");
                let request = chat::mail_file_request(&id);
                return self.ask(request, move |result| Message::FileDone(result.map(|_| None), failed));
            }
            Message::OpenMail { account, id } => {
                let failed = tr("Couldn't open the mail.");
                let request = chat::open_mail_request(&account, &id);
                return self.ask(request, move |result| Message::FileDone(result.map(|_| None), failed));
            }
            Message::CardRequest(request, failed) => {
                return self.ask(*request, move |result| Message::CardAnswered(result.map(|_| ()), failed));
            }
            Message::RetryCards => return self.fetch_cards(),
            // A refusal (no picture, no Nextcloud) keeps the kind's icon, and the card asks no more until it changes.
            Message::CardPicture(conversation, seq, result) => {
                if self.current.as_deref() != Some(conversation.as_str()) {
                    // Left meanwhile: asked anew when the conversation is open again.
                    self.card_pictures_asked.remove(&(conversation, seq));
                } else if let Some(handle) = result.ok().as_ref().and_then(card_picture) {
                    self.card_pictures.insert((conversation, seq), handle);
                }
            }
            Message::CardAnswered(result, failed) => {
                if let Err(refusal) = result {
                    self.note_failure(failed, &refusal);
                }
                return self.fetch_cards();
            }
            Message::FileDone(Ok(said), _) => {
                if let Some(text) = said {
                    self.note(text.to_owned());
                }
            }
            // The file may have gone meanwhile: the cards are asked for again.
            Message::FileDone(Err(refusal), failed) => {
                self.note_failure(failed, &refusal);
                return self.fetch_cards();
            }
            Message::Decided(id, result) => return self.decided(id, result),
            Message::Select(id) => {
                self.marked = None;
                return self.select(id);
            }
            Message::NewChat => return self.new_chat(),
            Message::Edit(action) => self.editor.perform(action),
            Message::MiniInput(text) => self.mini = text,
            Message::Send => return self.send(),
            Message::Sent(loaded) => return self.sent(loaded),
            Message::Stop => return self.stop(),
            Message::Stopped(Ok(())) => return self.follow(),
            Message::Stopped(Err(refusal)) => self.note_failure(tr("Couldn't stop the answer."), &refusal),
            Message::Pick(choice) => {
                self.choice = Some(choice);
                self.chosen = Chosen::Person;
                self.menu = None;
            }
            Message::PickEffort(effort) => {
                self.effort = effort;
                self.chosen = Chosen::Person;
                self.menu = None;
            }
            Message::PickMode(mode) => {
                self.mode = mode;
                self.menu = None;
            }
            Message::Voice(Ok(voice)) => self.voice = Some(voice),
            // An old core or none: the microphone stays as it was.
            Message::Voice(Err(_)) => {}
            Message::Dictate => return self.dictate(),
            Message::Dictated(result) => return self.dictated(result),
            // The demo records nothing: its `dictating` state has no dictation that could answer.
            Message::DictationStopped if self.launch.demo && self.dictation == Dictation::Stopping => {
                self.dictation = Dictation::Idle;
            }
            Message::DictationStopped => {}
            Message::ToggleReadAloud => return self.toggle_read_aloud(),
            Message::ReadAloudSet(on, Err(refusal)) => {
                self.read_aloud_switching = false;
                if self.read_aloud == on {
                    self.read_aloud = !on;
                }
                self.note_failure(tr("Couldn't change reading aloud."), &refusal);
            }
            Message::ReadAloudSet(_, Ok(())) => self.read_aloud_switching = false,
            Message::StopReading => return self.stop_reading(),
            Message::RepeatSpoken(seq) => return self.repeat_spoken(seq),
            Message::Repeated(number, result) => {
                if self.repeating == Some(number) {
                    self.repeating = None;
                    self.reading = None;
                }
                if let Err(refusal) = result {
                    let message = refusal.message.clone();
                    self.note_failure(&message, &refusal);
                }
            }
            // What a "Repeat" replaced ends meanwhile: "Stop" stays with the repeat.
            Message::Read(..) if self.repeating.is_some() => {}
            Message::ReadingStopped => {}
            // A reading stopped by a change of conversation answers late: only its own conversation's
            // answer is meant, and its failure is no longer worth a notice.
            Message::Read(conversation, _, _) if conversation != self.reading_in && self.reading.is_some() => {}
            Message::Read(conversation, seq, result) => {
                if self.reading == Some(seq) && self.reading_in == conversation {
                    self.reading = None;
                }
                if let Err(refusal) = result {
                    let message = refusal.message.clone();
                    self.note_failure(&message, &refusal);
                }
            }
            // The section AI of this window's own settings (issue #410), not the control centre.
            Message::ManageModels => {
                self.close_menus();
                return self.open_settings(Some(AI_PAGE));
            }
            Message::Talk => return self.start_talk(),
            // No listener came up for it (it could not start): say so, the composer stays.
            Message::TalkStarted(Ok(None)) => {
                self.talk_starting = false;
                self.talk = None;
                self.note(tr("Couldn't start the conversation.").to_owned());
            }
            Message::TalkStarted(Ok(talk)) => {
                self.talk_starting = false;
                self.talk = talk;
                // Only one begun in a new chat is followed into the conversation its first sentence starts.
                self.talk_started = self.talk.as_ref().is_some_and(|talk| talk.conversation.is_none());
            }
            Message::TalkStarted(Err(refusal)) => {
                self.talk_starting = false;
                self.note_failure(tr("Couldn't start the conversation."), &refusal);
            }
            Message::EndTalk => {
                self.talk = None;
                self.talk_started = false;
                return self.ask(Request::EndTalk, |result| Message::TalkEnded(result.map(drop)));
            }
            Message::TalkEnded(Ok(())) => {}
            Message::TalkEnded(Err(refusal)) => self.note_failure(tr("Couldn't end the conversation."), &refusal),
            Message::TalkTick => return self.fetch_talk(),
            Message::TalkStatus(Ok((talk, speaking))) => {
                self.talk_speaking = speaking;
                return self.take_talk(talk);
            }
            // An old core or none: no conversation by voice to show.
            Message::TalkStatus(Err(_)) => self.talk_speaking = false,
            Message::AiAccounts(Ok(any)) => self.ai_accounts = any,
            // An old core or none: the window keeps what it said.
            Message::AiAccounts(Err(_)) => {}
            Message::ToggleArea(area) => {
                if let Some(at) = self.without.iter().position(|off| *off == area) {
                    self.without.remove(at);
                } else {
                    self.without.push(area);
                }
            }
            Message::ToggleTool(seq) => {
                if !self.expanded.remove(&seq) {
                    self.expanded.insert(seq);
                }
            }
            Message::ToggleMenu(menu) => {
                let picker = matches!(menu, Menu::Picker | Menu::BotPicker);
                self.menu = if self.menu.as_ref() == Some(&menu) { None } else { Some(menu) };
                // A conversation's menu from the sidebar row leaves the drawer open under it.
                // A picker that opens asks for the models again (task S.26): one turned on in the
                // control centre meanwhile is offered at once. What came last shows until then.
                if picker && self.menu.is_some() {
                    return self.refetch_models();
                }
            }
            Message::CloseMenu => self.close_menus(),
            Message::OpenInWindow(id) => {
                self.close_menus();
                let request = Request::OpenChatWindow { window: ChatWindow::Conversation(id) };
                return self.ask(request, |result| Message::WindowOpened(result.map(|_| ())));
            }
            Message::WindowOpened(Ok(())) => {}
            Message::WindowOpened(Err(refusal)) => self.note_failure(tr("Couldn't open the window."), &refusal),
            Message::AskRename(id) => {
                self.close_menus();
                let title = self.title_of(&id);
                self.dialog = Some(Dialog::Rename { id, title });
                return operation::focus(RENAME);
            }
            Message::RenameInput(text) => {
                if let Some(Dialog::Rename { title, .. }) = self.dialog.as_mut() {
                    *title = text;
                }
            }
            Message::ConfirmRename => return self.rename(),
            Message::AskDelete(id) => {
                self.close_menus();
                let title = self.title_of(&id);
                self.dialog = Some(Dialog::Delete { id, title });
            }
            Message::ConfirmDelete => return self.delete(),
            Message::Pin(id, pinned) => {
                self.close_menus();
                if let Some(conversation) = self.conversations.iter_mut().find(|c| c.id == id) {
                    conversation.pinned = pinned;
                }
                let failed =
                    if pinned { tr("Couldn't pin the conversation.") } else { tr("Couldn't unpin the conversation.") };
                let request = Request::PinConversation { id, pinned };
                return self.ask(request, move |result| Message::Changed(result.map(|_| ()), failed));
            }
            Message::Archive(ids, archived) => return self.archive(ids, archived),
            Message::ToggleArchive => self.archive_open = !self.archive_open,
            Message::AskArchiveAll => {
                self.close_menus();
                let ids = self.recent_ids();
                if !ids.is_empty() {
                    self.dialog = Some(Dialog::ArchiveMany { ids });
                }
            }
            Message::AskDeleteAll => {
                self.close_menus();
                let ids = self.recent_ids();
                if !ids.is_empty() {
                    self.dialog = Some(Dialog::DeleteMany { ids, archive: false });
                }
            }
            Message::AskEmptyArchive => {
                self.close_menus();
                let ids: Vec<String> =
                    chat::archived_group(&self.conversations, "").iter().map(|c| c.id.clone()).collect();
                if !ids.is_empty() {
                    self.dialog = Some(Dialog::DeleteMany { ids, archive: true });
                }
            }
            Message::ConfirmMany => return self.confirm_many(),
            Message::DeletedMany(ids, result) => return self.deleted_many(ids, result),
            Message::Changed(Ok(()), _) => return self.load_list(),
            Message::Changed(Err(refusal), failed) => {
                self.note_failure(failed, &refusal);
                return self.load_list();
            }
            Message::Deleted(id, result) => return self.deleted(id, result),
            Message::CloseDialog => self.dialog = None,
            Message::Search(text) => return self.search_for(text),
            Message::Hits(query, result) => {
                // An answer for words no longer in the field is stale.
                if query == self.search.trim() {
                    // `--open search`: the first hit opened, so its marked entry shows.
                    let first = match (&result, self.launch.open, &self.current) {
                        // One whose answer runs would never settle for a picture.
                        (Ok(hits), Some(Open::Search), None) => hits
                            .iter()
                            .find(|hit| self.conversations.iter().any(|c| c.id == hit.conversation && !c.running))
                            .map(|hit| (hit.conversation.clone(), hit.seq)),
                        _ => None,
                    };
                    self.hits = Some(result);
                    self.hits_for = query;
                    if let Some((id, seq)) = first {
                        return self.open_hit(id, seq);
                    }
                }
            }
            Message::OpenHit(id, seq) => return self.open_hit(id, seq),
            Message::Hover(id) => self.hover = id,
            Message::Switcher(Switcher::Search) => {
                self.menu = None;
                // The search field lives in the expanded sidebar: the drawer brings it where there is none.
                if shell::sidebar_mode(self.class(), self.sidebar) != Some(SidebarMode::Expanded) {
                    self.drawer = true;
                }
                return operation::focus(SEARCH);
            }
            Message::Switcher(Switcher::New) => return self.new_chat(),
            Message::OpenApp(app) => {
                self.close_menus();
                let suite = self.suite.clone();
                return Task::perform(async move { suite.open_app(&app).await }, Message::AppOpened);
            }
            Message::AppOpened(Ok(())) => {}
            Message::AppOpened(Err(err)) => self.note(format!("{} {err}", tr("Couldn't open the app."))),
            Message::SetUp => return self.open_access(self.chat_bot().map(str::to_owned)),
            Message::AddAiAccount => {
                self.close_menus();
                return self.open_settings(Some(AI_PAGE));
            }
            Message::ToggleSettings => return self.toggle_settings(),
            Message::SettingsHost(message) => {
                let Some(pane) = self.settings.as_mut() else { return Task::none() };
                // The sections see the host's sends and loads first: Access settles its draft by
                // them (task S.34).
                settings::received(&mut pane.sections, &message);
                let outcome = pane.host.update(message);
                // A demo state's steps go once the page has its data (issue #547): only then does
                // the profile editor start from the stored profile.
                if pane.host.loaded() {
                    for step in std::mem::take(&mut self.settings_steps) {
                        let action = settings::update(&mut pane.sections, step, pane.host.data());
                        let _ = pane.host.act(action);
                    }
                }
                return self.settings_outcome(outcome);
            }
            Message::SettingsPage(message) => {
                let outcome = {
                    let Some(pane) = self.settings.as_mut() else { return Task::none() };
                    // A row of the section menu that the page drew itself (the way up from a bot,
                    // issue #473) ends the menu, as the host's own rows do; ticks leave it open.
                    if settings::leaves_page(&message) {
                        pane.host.close_menu();
                    }
                    let action = settings::update(&mut pane.sections, message, pane.host.data());
                    pane.host.act(action)
                };
                return self.settings_outcome(outcome);
            }
            Message::OpenLink(url) => return open_link(url),
            Message::Retry => {
                self.list_error = None;
                self.chat_error = None;
                return Task::batch([self.load_all(), self.open_current()]);
            }
            Message::ToggleSidebar => return self.toggle_sidebar(),
            Message::UiState(state) => self.sidebar = state.sidebar,
            Message::SidebarWidth(event) => {
                // The demo stores nothing.
                if self.side.update(event, self.sidebar_limits()) && !self.launch.demo {
                    return ui_state::save_split_task(MAIN, SIDE_KEY.to_owned(), self.side.chosen);
                }
            }
            Message::StoredWidths(stored) => self.side.restore(stored.get(SIDE_KEY).copied()),
            Message::Escape => {
                // Innermost first: a dialog of the window, then what the open settings section
                // has open (its question, a note being written, task S.17), then a menu or the
                // drawer over the content, then the settings themselves, then the notice. The
                // settings only get here with nothing of theirs left to discard
                // ([`App::settings_question`] took that turn).
                if self.dialog.is_some() {
                    self.dialog = None;
                } else if let Some(message) = self.settings.as_ref().and_then(|pane| settings::escape(&pane.sections)) {
                    return self.update(Message::SettingsPage(message));
                } else if self.menu.is_some() || self.drawer {
                    self.close_menus();
                } else if self.settings.is_some() {
                    // Through the host: its open section menu or a page's own menu closes first
                    // (review of #554), else the host answers `Outcome::Close`.
                    return self.update(Message::SettingsHost(torro_settings_ui::host::Message::Back));
                } else if self.own_talk().is_some() {
                    // The conversation by voice ends (issue #747), reading aloud with it.
                    return self.update(Message::EndTalk);
                } else if self.reading.is_some() {
                    // Reading aloud stops first (issue #745), the notice goes with the next Escape.
                    return self.stop_reading();
                } else {
                    self.notice = None;
                }
            }
            Message::NoticeTick(now) => {
                if self.notice.as_ref().is_some_and(|n| now.saturating_duration_since(n.shown) >= widgets::TOAST_FOR) {
                    self.notice = None;
                }
            }
            Message::Appearance(Ok(appearance)) => self.appearance = appearance,
            // The core may be starting up; keep the colours we have.
            Message::Appearance(Err(_)) => {}
            Message::SystemMode(mode) => self.system_mode = mode,
            Message::Design(message) => {
                if let Some(task) = self.design.update(message) {
                    return task.map(Message::Design);
                }
            }
            Message::Zoom(message) => {
                let before = self.zoom.factor();
                let task = self.zoom.update(message).map(Message::Zoom);
                if self.zoom.factor() == before {
                    return task;
                }
                // The window keeps its pixels, so its logical size changes with the zoom.
                self.size = zoom::rescale(self.size, before, self.zoom.factor());
                let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
                return Task::batch([task, size]);
            }
        }
        Task::none()
    }

    // -- Asking the core ------------------------------------------------------------------------

    /// Sends `request` and maps the answer to a message. Every request of the window goes through
    /// here, so the tests see them in [`App::sent`] and answer them with [`App::replies`] (tasks do
    /// not run in tests).
    fn ask(
        &mut self,
        request: Request,
        map: impl Fn(Result<Response, Refusal>) -> Message + Send + 'static,
    ) -> Task<Message> {
        #[cfg(test)]
        {
            self.sent.push(request);
            self.replies.push(Box::new(map));
            Task::none()
        }
        #[cfg(not(test))]
        {
            let backend = self.backend.clone();
            Task::perform(async move { backend.ask(request).await }, map)
        }
    }

    /// The conversations, the engines and the client – what the window shows besides the chat.
    /// The forced states of `--open`: loading never ends, the error comes at once.
    fn load_all(&mut self) -> Task<Message> {
        let engines = self.ask(Request::ListEngines, |result| {
            Message::Engines(result.and_then(|response| match response {
                Response::Engines { engines } => Ok(engines),
                other => Err(Refusal::unexpected(&other)),
            }))
        });
        let client = self.ask(Request::ListClients, |result| {
            Message::Client(result.and_then(|response| {
                match response {
                    Response::Clients { clients } => Ok(clients
                        .into_iter()
                        .find(|client| client.kind == torro_ipc::api::ASSISTANT_CLIENT)
                        .map(|client| client.rights)),
                    other => Err(Refusal::unexpected(&other)),
                }
            }))
        });
        let profile = self.ask(Request::GetProfile, |result| {
            Message::Profile(result.and_then(|response| match response {
                Response::Profile { profile } => Ok(profile),
                other => Err(Refusal::unexpected(&other)),
            }))
        });
        let settings = self.ask(Request::Settings, |result| {
            Message::Settings(result.and_then(|response| match response {
                Response::Settings { settings } => Ok(Box::new(settings)),
                other => Err(Refusal::unexpected(&other)),
            }))
        });
        Task::batch([self.load_list(), engines, client, profile, settings, self.load_bots()])
    }

    /// The assistant's name as the window shows it: the profile's, else the default.
    pub(crate) fn assistant_name(&self) -> &str {
        self.profile.as_ref().map_or(torro_ipc::assistant::DEFAULT_NAME, |profile| profile.name.as_str())
    }

    /// The assistant's picture ([`torro_ipc::assistant::AVATARS`]): the profile's, else the letter.
    pub(crate) fn assistant_avatar(&self) -> &str {
        self.profile.as_ref().map_or(torro_ipc::assistant::LETTER_AVATAR, |profile| profile.avatar.as_str())
    }

    fn load_list(&mut self) -> Task<Message> {
        match self.launch.open {
            Some(Open::Loading) => return Task::none(),
            Some(Open::Error) => {
                return Task::done(Message::Listed(Err(Refusal::new("error", "The core did not answer (demo)"))));
            }
            _ => {}
        }
        // How long the core took to answer, with `TORRO_TIMING` (issue #206).
        let started = std::time::Instant::now();
        self.ask(Request::ListConversations, move |result| {
            torro_core::timing::took("load", started);
            Message::Listed(result.and_then(|response| match response {
                Response::Conversations { conversations } => Ok(conversations),
                other => Err(Refusal::unexpected(&other)),
            }))
        })
    }

    /// The whole open conversation, when there is one.
    fn open_current(&mut self) -> Task<Message> {
        let Some(id) = self.current.clone() else { return Task::none() };
        if self.launch.open == Some(Open::Loading) {
            return Task::none();
        }
        self.following = true;
        let request = Request::GetConversation { id: id.clone(), after: None };
        self.ask(request, move |result| Message::Opened(id.clone(), conversation_of(result)))
    }

    /// What is new in the open conversation, unless a request for it is out already.
    fn follow(&mut self) -> Task<Message> {
        let Some(id) = self.current.clone() else { return Task::none() };
        if self.following || self.transcript.summary.is_none() {
            return Task::none();
        }
        self.following = true;
        let request = Request::GetConversation { id: id.clone(), after: Some(self.transcript.last()) };
        let follow = self.ask(request, move |result| Message::Followed(id.clone(), conversation_of(result)));
        // An approval still open may have been decided elsewhere (tray, inbox).
        let pending: Vec<String> = self
            .transcript
            .approvals()
            .into_iter()
            .filter(|id| self.approvals.get(*id).is_none_or(|approval| approval.state.is_pending()))
            .map(str::to_owned)
            .collect();
        let approvals: Vec<Task<Message>> = pending.into_iter().map(|id| self.fetch_approval(id)).collect();
        Task::batch(approvals.into_iter().chain([follow]))
    }

    fn fetch_approval(&mut self, id: String) -> Task<Message> {
        let request = Request::ApprovalStatus { id: id.clone() };
        self.ask(request, move |result| Message::Approval(id.clone(), approval_of(result)))
    }

    fn fetch_appearance(&self) -> Task<Message> {
        let suite = self.suite.clone();
        Task::perform(async move { theme::stored_appearance(&suite).await }, Message::Appearance)
    }

    // -- Answers --------------------------------------------------------------------------------

    fn listed(&mut self, result: Result<Vec<ConversationSummary>, Refusal>) -> Task<Message> {
        match result {
            Ok(conversations) => {
                self.conversations = conversations;
                self.listed = true;
                self.list_error = None;
                self.choose_default();
                self.open_launched();
                // `--open search` (issue #704): a word of the demo in the field.
                let search = match self.launch.open == Some(Open::Search) && self.search.is_empty() {
                    true => self.search_for(demo::SEARCH_WORD.into()),
                    // A search in the field asks again, so a deleted conversation leaves its hits.
                    false => self.search_for(self.search.clone()),
                };
                return Task::batch([self.resolve_bot_window(), search]);
            }
            Err(refusal) => self.list_error = Some(refusal.message),
        }
        Task::none()
    }

    fn take_engines(&mut self, result: Result<Vec<Engine>, Refusal>) -> Task<Message> {
        let Ok(engines) = result else { return Task::none() };
        self.engines = chat::ordered(engines);
        self.engines_loaded = true;
        self.models.retain(|id, _| self.engines.iter().any(|engine| &engine.id == id));
        self.choose_default();
        // An AI account with no model on is no engine (task S.24): without any, only the accounts
        // tell whether there is one whose models are all off.
        let accounts = if self.engines.iter().any(|engine| engine.kind == EngineKind::Ai) {
            self.ai_accounts = true;
            Task::none()
        } else {
            self.fetch_ai_accounts()
        };
        Task::batch([accounts, self.fetch_all_models()])
    }

    /// Whether there is an AI account at all (`accounts.list_accounts`, task S.26).
    fn fetch_ai_accounts(&mut self) -> Task<Message> {
        self.ask(Request::Accounts, |result| {
            Message::AiAccounts(result.and_then(|response| match response {
                Response::Accounts { accounts } => Ok(accounts.iter().any(|account| account.kind == "ai")),
                other => Err(Refusal::unexpected(&other)),
            }))
        })
    }

    /// The models of every AI account again, as a picker opens (task S.26): the switches may have
    /// changed in the control centre.
    fn refetch_models(&mut self) -> Task<Message> {
        let engines: Vec<String> = self
            .engines
            .iter()
            .filter(|engine| engine.kind == EngineKind::Ai)
            .map(|engine| engine.id.clone())
            .collect();
        Task::batch(engines.into_iter().map(|id| self.fetch_models(id)).collect::<Vec<_>>())
    }

    /// The models of each AI account not listed yet; the picker asks again when it opens.
    fn fetch_all_models(&mut self) -> Task<Message> {
        let missing: Vec<String> = self
            .engines
            .iter()
            .filter(|engine| engine.kind == EngineKind::Ai && !self.models.contains_key(&engine.id))
            .map(|engine| engine.id.clone())
            .collect();
        let tasks: Vec<Task<Message>> = missing.into_iter().map(|id| self.fetch_models(id)).collect();
        Task::batch(tasks)
    }

    fn fetch_models(&mut self, engine: String) -> Task<Message> {
        // Closed models too: what the picker offers is what is turned on (task S.26), and a closed
        // model may be among them.
        let request = Request::AiModels { account: engine.clone(), all: true };
        self.ask(request, move |result| {
            Message::Models(
                engine.clone(),
                result.and_then(|response| match response {
                    Response::AiModels { models, .. } => Ok(models),
                    other => Err(Refusal::unexpected(&other)),
                }),
            )
        })
    }

    /// The choice before the person picks: the open conversation's engine, model and effort, else
    /// the default of the control centre, the last one used or the first engine
    /// ([`chat::preferred_choice`]). The person's own pick stays.
    fn choose_default(&mut self) {
        // A new bot waits for an engine as long as there is none.
        let fallback = chat::default_choice(&self.conversations, &self.engines, |_| None);
        if let Some(draft) = self.bot_draft.as_mut().filter(|draft| draft.choice.is_none()) {
            draft.choice = fallback;
        }
        if self.chosen == Chosen::Person {
            return;
        }
        let summary = self.current.as_ref().and_then(|id| {
            self.transcript
                .summary
                .as_ref()
                .filter(|summary| &summary.id == id)
                .or_else(|| self.conversations.iter().find(|c| &c.id == id))
        });
        if let Some(summary) = summary.filter(|s| self.engines.iter().any(|engine| engine.id == s.engine)) {
            self.choice = Some(Choice { engine: summary.engine.clone(), model: summary.model.clone() });
            self.effort = summary.effort;
            self.chosen = Chosen::Conversation;
            return;
        }
        // A new chat with a bot starts at the bot's engine and model (task B.3).
        let bot =
            self.bot.as_deref().filter(|_| self.current.is_none()).and_then(|id| crate::bots::find(&self.bots, id));
        if let Some(bot) = bot.filter(|bot| self.engines.iter().any(|engine| engine.id == bot.engine)) {
            self.choice = Some(Choice { engine: bot.engine.clone(), model: bot.model.clone() });
            self.effort = None;
            self.chosen = Chosen::Default;
            return;
        }
        // Only models turned on (task S.26): the first one offered, never one that is off.
        let offered: HashMap<String, Vec<AiModel>> =
            self.models.keys().filter_map(|id| Some((id.clone(), self.offered_models(id)?.ok()?))).collect();
        let listed = |id: &str| offered.get(id).map(Vec::as_slice);
        let preferred = chat::preferred_choice(self.default.as_ref(), &self.conversations, &self.engines, listed);
        (self.choice, self.effort) = preferred.map_or((None, None), |(choice, effort)| (Some(choice), effort));
        self.chosen = Chosen::Default;
    }

    fn opened(&mut self, id: String, loaded: Loaded) -> Task<Message> {
        if self.current.as_ref() != Some(&id) {
            return Task::none();
        }
        self.following = false;
        match loaded {
            Ok((summary, entries)) => {
                // Answer numbers count per conversation: what waited to be read was the one before's,
                // and what reads stops (issue #745).
                self.to_read.clear();
                self.ready_to_read = None;
                let elsewhere = self.reading.is_some() && self.reading_in.as_deref() != Some(id.as_str());
                let stopped = if elsewhere { self.stop_reading() } else { Task::none() };
                // An open conversation answers with its own bot (or the assistant).
                self.bot = summary.bot.clone();
                self.transcript = Transcript::new(summary, entries);
                self.chat_error = None;
                // What was there when the conversation opened shows at once.
                self.after_change(false);
                if matches!(self.launch.open, Some(Open::Tools | Open::BotAsked)) && self.expanded.is_empty() {
                    let first = self.transcript.entries.iter().find(|e| matches!(e.content, EntryContent::Tool { .. }));
                    self.expanded.extend(first.map(|entry| entry.seq));
                }
                self.choose_default();
                let scroll = self.scroll_to_hit(&id).unwrap_or_else(|| self.scroll_to_newest());
                Task::batch([self.fetch_approvals(), self.fetch_cards(), scroll, stopped])
            }
            Err(refusal) if refusal.code == "not-found" => self.gone(),
            Err(refusal) => {
                self.chat_error = Some(refusal.message);
                Task::none()
            }
        }
    }

    fn followed(&mut self, id: String, loaded: Loaded) -> Task<Message> {
        if self.current.as_ref() != Some(&id) {
            return Task::none();
        }
        self.following = false;
        match loaded {
            Ok((summary, entries)) => {
                let known: HashSet<String> = self.transcript.approvals().into_iter().map(str::to_owned).collect();
                let cards = self.transcript.cards();
                let last = self.transcript.last();
                if self.transcript.merge(summary, entries) {
                    self.after_change(true);
                }
                let read = self.read_ready();
                // A hit that waited for its entry (issue #704).
                let hit = self.scroll_to_hit(&id).unwrap_or_else(|| {
                    // New entries below a history that was at the bottom: it stays there.
                    if self.follow_bottom && self.transcript.last() > last {
                        self.scroll_to_newest()
                    } else {
                        Task::none()
                    }
                });
                // A new card (after `tools.create_pdf`) is resolved at once, not on the next tick.
                let cards = if self.transcript.cards() == cards { Task::none() } else { self.fetch_cards() };
                let new: Vec<String> = self
                    .transcript
                    .approvals()
                    .into_iter()
                    .filter(|id| !known.contains(*id))
                    .map(str::to_owned)
                    .collect();
                let tasks: Vec<Task<Message>> = new.into_iter().map(|id| self.fetch_approval(id)).collect();
                Task::batch(tasks.into_iter().chain([cards, hit, read]))
            }
            Err(refusal) if refusal.code == "not-found" => self.gone(),
            // The next tick asks again.
            Err(_) => Task::none(),
        }
    }

    /// The open conversation is gone (deleted in another window or by the retention time): a new
    /// chat takes its place, in a chat window too.
    fn gone(&mut self) -> Task<Message> {
        self.note(tr("This conversation was deleted. A new chat starts here.").to_owned());
        self.new_chat()
    }

    /// After the transcript changed: its summary in the sidebar, the Markdown of new or grown
    /// answers, and the areas of its last message once. With `animate` (an answer that came while
    /// the window watches) a new answer appears letter by letter ([`App::stream`]); without, every
    /// answer shows whole.
    fn after_change(&mut self, animate: bool) {
        if let Some(summary) = self.transcript.summary.clone() {
            match self.conversations.iter_mut().find(|c| c.id == summary.id) {
                Some(listed) => *listed = summary,
                None => self.conversations.insert(0, summary),
            }
        }
        // An answer is complete once it is stored, or when no answer runs any more.
        let (last, running) = (self.transcript.last(), self.transcript.running());
        let answers: Vec<(u64, String, bool)> = self
            .transcript
            .entries
            .iter()
            .filter_map(|entry| match &entry.content {
                EntryContent::Answer { text, .. } => Some((entry.seq, text.clone(), entry.seq <= last || !running)),
                _ => None,
            })
            .collect();
        let seen: HashSet<u64> = answers.iter().map(|(seq, ..)| *seq).collect();
        // The streaming answer is gone (stopped, or stored under another number): what it showed
        // goes on under the new number, nothing is written twice.
        let mut moved = self.stream.as_ref().is_some_and(|stream| !seen.contains(&stream.seq));
        // A new answer that came while the window watches is read aloud once the run ended, the last
        // one only: a remark before a tool ("I'll look at your calendar") would be cut off by the
        // answer after it (issue #745). One that was there when the conversation opened is not.
        for (seq, _, _) in &answers {
            if animate && !self.answers.contains_key(seq) && self.stream.as_ref().is_none_or(|s| s.seq != *seq) {
                self.to_read.insert(*seq);
            }
        }
        self.to_read.retain(|seq| seen.contains(seq));
        // Read from its first sentence, as the core reads the answer after the last message while it
        // grows (issue #747, decision 2), parts after a tool with it.
        if let Some(first) = self.to_read.iter().min().copied() {
            self.ready_to_read = Some(first);
            self.to_read.clear();
        }
        for (seq, text, complete) in answers {
            if let Some(stream) = self.stream.as_mut().filter(|stream| stream.seq == seq) {
                stream.smoother.set(&text, complete);
            } else if animate && !self.answers.contains_key(&seq) {
                self.start_stream(seq, &text, complete, std::mem::take(&mut moved));
            } else if self.answers.get(&seq).is_none_or(|(had, _)| *had != text) {
                self.answers.insert(seq, (text.clone(), torro_gui::markdown::parse(&text)));
            }
        }
        self.answers.retain(|seq, _| seen.contains(seq));
        if self.stream.as_ref().is_some_and(|stream| !seen.contains(&stream.seq)) {
            self.stream = None;
        }
        self.refresh_stream();
        if self.take_without {
            self.take_without = false;
            let usable = self.switchable();
            self.without = self.transcript.last_without().unwrap_or_default();
            self.mode = self.transcript.last_mode().unwrap_or_default();
            if self.access != Access::Unknown {
                self.without.retain(|area| usable.contains(area));
            }
        }
    }

    // -- Smooth streaming (issue #135) ----------------------------------------------------------

    /// Lets the new answer `seq` appear letter by letter. One answer appears at a time: one still
    /// appearing shows whole at once – unless it is gone (`moved`), then its letters go on here.
    fn start_stream(&mut self, seq: u64, text: &str, complete: bool, moved: bool) {
        match self.stream.take() {
            Some(mut stream) if moved => {
                stream.seq = seq;
                stream.smoother.set(text, complete);
                self.stream = Some(stream);
                return;
            }
            Some(mut stream) => {
                stream.smoother.finish();
                let whole = stream.smoother.shown().to_owned();
                self.answers.insert(stream.seq, (whole.clone(), torro_gui::markdown::parse(&whole)));
            }
            None => {}
        }
        let mut smoother = Smoother::new();
        smoother.set(text, complete);
        self.stream = Some(Stream { seq, smoother, frame: None });
    }

    /// One frame: the next letters of the streaming answer.
    fn frame(&mut self, now: Instant) {
        let Some(stream) = self.stream.as_mut() else { return };
        let elapsed = stream.frame.map_or(FRAME, |last| now.saturating_duration_since(last));
        stream.smoother.advance(elapsed);
        stream.frame = stream.smoother.revealing().then_some(now);
        self.refresh_stream();
    }

    /// The Markdown of what shows of the streaming answer, parsed again only when that changed;
    /// once the answer shows whole it is an answer like any other and the stream ends.
    fn refresh_stream(&mut self) {
        let Some(stream) = &self.stream else { return };
        let done = stream.smoother.done();
        let shown = stream::display(stream.smoother.shown(), done);
        if self.answers.get(&stream.seq).is_none_or(|(had, _)| *had != shown) {
            let items = torro_gui::markdown::parse(&shown);
            self.answers.insert(stream.seq, (shown.into_owned(), items));
        }
        if done {
            self.stream = None;
        }
    }

    /// Whether the answer `seq` shows anything yet: the streaming answer shows nothing before its
    /// first letter, and then takes no room.
    pub(crate) fn answer_shows(&self, seq: u64) -> bool {
        // A spoken part shows as the answer's lead at once (issue #774), whatever its shown part does.
        let spoken = self.transcript.entries.iter().any(|entry| match &entry.content {
            EntryContent::Answer { spoken: Some(spoken), .. } => entry.seq == seq && !spoken.trim().is_empty(),
            _ => false,
        });
        spoken || self.answers.get(&seq).is_none_or(|(shown, _)| !shown.is_empty())
    }

    /// The number of the answer still appearing: the entries after it wait until it shows whole.
    pub(crate) fn appearing(&self) -> Option<u64> {
        self.stream.as_ref().map(|stream| stream.seq)
    }

    /// The history scrolled or grew: it follows the newest entry while it is at the bottom, and
    /// stays where the person scrolled to otherwise.
    fn history_scrolled(&mut self, viewport: scrollable::Viewport) -> Task<Message> {
        let content = viewport.content_bounds().height;
        let hidden = (content - viewport.bounds().height).max(0.0);
        let offset = viewport.absolute_offset().y;
        let (follow, jump) =
            history_follow(self.follow_bottom, self.history_seen, content, offset, hidden, theme::space::l());
        self.history_seen = Some((content, offset));
        self.follow_bottom = follow;
        // Back at the bottom, or grown below it: the history keeps to the newest entry from now on.
        if jump { self.scroll_to_newest() } else { Task::none() }
    }

    /// The history at its newest entry, following it as it grows (a relative offset of 1 stays at
    /// the bottom; scrolling by hand leaves it).
    fn scroll_to_newest(&mut self) -> Task<Message> {
        self.follow_bottom = true;
        operation::snap_to(HISTORY, operation::RelativeOffset::END)
    }

    /// The answer waits for an approval of the person.
    pub(crate) fn waits_for_approval(&self) -> bool {
        self.transcript
            .approvals()
            .iter()
            .any(|id| self.approvals.get(*id).is_some_and(|approval| approval.state.is_pending()))
    }

    fn fetch_approvals(&mut self) -> Task<Message> {
        let ids: Vec<String> = self.transcript.approvals().into_iter().map(str::to_owned).collect();
        let tasks: Vec<Task<Message>> = ids.into_iter().map(|id| self.fetch_approval(id)).collect();
        Task::batch(tasks)
    }

    /// Asks for the picture of the link card `seq` of `conversation` (issue #785): `assistant.get_card_picture`
    /// small, answered with [`Message::CardPicture`]. [`App::fetch_card_pictures`] calls it once per card.
    pub(crate) fn fetch_card_picture(&mut self, conversation: String, seq: u64) -> Task<Message> {
        let request = Request::GetCardPicture {
            conversation: conversation.clone(),
            seq,
            size: torro_ipc::chat::PreviewSize::Small,
        };
        self.ask(request, move |result| {
            let picture = result.and_then(|response| match response {
                Response::ChatPicture { picture } => Ok(picture),
                other => Err(Refusal::unexpected(&other)),
            });
            Message::CardPicture(conversation.clone(), seq, picture)
        })
    }

    /// Asks for the pictures of the open conversation's link cards that show a card with `image` and were not asked
    /// for yet ([`App::fetch_card_picture`]); each is asked once ([`App::card_pictures_asked`]).
    fn fetch_card_pictures(&mut self) -> Task<Message> {
        let Some(conversation) = self.current.clone() else {
            return Task::none();
        };
        let mut wanted: Vec<u64> = self
            .cards
            .iter()
            .filter(|(_, card)| matches!(card, Card::Link { card: Some(link), .. } if link.image))
            .map(|(seq, _)| *seq)
            .filter(|seq| !self.card_pictures_asked.contains(&(conversation.clone(), *seq)))
            .collect();
        wanted.sort_unstable();
        let tasks: Vec<Task<Message>> = wanted
            .into_iter()
            .map(|seq| {
                self.card_pictures_asked.insert((conversation.clone(), seq));
                self.fetch_card_picture(conversation.clone(), seq)
            })
            .collect();
        Task::batch(tasks)
    }

    /// The cards of the open conversation as they are now (`assistant.get_cards`, task WZ.8); nothing
    /// for a conversation without cards.
    fn fetch_cards(&mut self) -> Task<Message> {
        let Some(id) = self.current.clone().filter(|_| !self.transcript.cards().is_empty()) else {
            return Task::none();
        };
        let request = Request::GetCards { conversation: id.clone() };
        self.ask(request, move |result| {
            Message::Cards(
                id.clone(),
                result.and_then(|response| match response {
                    Response::Cards { cards } => Ok(cards),
                    other => Err(Refusal::unexpected(&other)),
                }),
            )
        })
    }

    /// The cards of the conversation `id`, when it is still the open one, and the pictures of its link cards not
    /// asked for yet. A card that changed (shown once, gone) drops its picture and is asked for anew. A refusal (an
    /// old core, a core that is restarting) leaves the cards as they were.
    fn take_cards(&mut self, id: &str, result: Result<Vec<CardEntry>, Refusal>) -> Task<Message> {
        if self.current.as_deref() != Some(id) {
            return Task::none();
        }
        match result {
            Ok(cards) => {
                let cards: HashMap<u64, Card> = cards.into_iter().map(|entry| (entry.seq, entry.card)).collect();
                for (seq, before) in &self.cards {
                    if cards.get(seq) != Some(before) {
                        let key = (id.to_owned(), *seq);
                        self.card_pictures.remove(&key);
                        self.card_pictures_asked.remove(&key);
                    }
                }
                self.cards = cards;
                self.fetch_card_pictures()
            }
            Err(refusal) => {
                eprintln!("{}: cannot read the cards ({}: {})", MAIN.binary, refusal.code, refusal.message);
                Task::none()
            }
        }
    }

    // -- Actions --------------------------------------------------------------------------------

    fn select(&mut self, id: String) -> Task<Message> {
        self.close_menus();
        self.leave_settings();
        self.bot_draft = None;
        if self.current.as_ref() == Some(&id) {
            return Task::none();
        }
        self.bot = self.conversations.iter().find(|c| c.id == id).and_then(|c| c.bot.clone());
        self.reset_chat(Some(id));
        // What the old conversation reads stops at once, not only once the new one loaded.
        let stopped = self.leave_reading();
        if self.chosen != Chosen::Person {
            self.chosen = Chosen::Default;
        }
        self.choose_default();
        Task::batch([self.open_current(), stopped])
    }

    /// The sidebar's search with `text` (issue #704): from [`SEARCH_FROM`] characters on it asks the core
    /// for hits in every conversation; below, the titles are filtered as before and the hits go.
    fn search_for(&mut self, text: String) -> Task<Message> {
        self.search = text;
        let query = self.search.trim().to_owned();
        if query.chars().count() < SEARCH_FROM {
            self.hits = None;
            if query.is_empty() {
                self.marked = None;
            }
            return Task::none();
        }
        self.asked = query.clone();
        let request = Request::SearchConversations { query: query.clone(), limit: Some(MAX_HITS) };
        self.ask(request, move |result| {
            Message::Hits(
                query.clone(),
                result.and_then(|response| match response {
                    Response::ConversationHits { hits } => Ok(hits),
                    other => Err(Refusal::unexpected(&other)),
                }),
            )
        })
    }

    /// Opens the hit at entry `seq` of the conversation `id` (issue #704): the entry marked and scrolled
    /// to once it is there; a hit of the title opens the conversation as a click on it does.
    fn open_hit(&mut self, id: String, seq: Option<u64>) -> Task<Message> {
        self.marked = seq.map(|seq| (id.clone(), seq));
        self.snap = self.marked.clone();
        if self.current.as_ref() == Some(&id) && self.transcript.summary.is_some() {
            // As a click on the conversation: the chat comes back in place of the settings or an editor.
            self.close_menus();
            self.leave_settings();
            self.bot_draft = None;
            // A newer entry than the window has comes with the next look; the scroll waits for it.
            return self.scroll_to_hit(&id).unwrap_or_else(|| self.follow());
        }
        self.select(id)
    }

    /// Scrolls the history of the open conversation `id` to the hit waiting for it ([`App::snap`]), by its
    /// share of the entries; `None` when none waits or its entry is not there yet (it keeps waiting). The
    /// history then stays there and does not follow.
    fn scroll_to_hit(&mut self, id: &str) -> Option<Task<Message>> {
        let (_, seq) = self.snap.clone().filter(|(snap, _)| snap == id)?;
        let entries = &self.transcript.entries;
        let at = entries.iter().position(|entry| entry.seq == seq)?;
        self.snap = None;
        let share = if entries.len() <= 1 { 0.0 } else { at as f32 / (entries.len() - 1) as f32 };
        self.follow_bottom = false;
        Some(operation::snap_to(HISTORY, operation::RelativeOffset { x: None, y: Some(share) }))
    }

    /// A new chat: with the assistant in the main window, with its bot in a bot's chat window.
    fn new_chat(&mut self) -> Task<Message> {
        self.close_menus();
        self.leave_settings();
        self.bot_draft = None;
        match &self.launch.window {
            Window::Main => self.bot = None,
            Window::Bot(id) => self.bot = Some(id.clone()),
            // A conversation's window stays with whoever answered there.
            Window::Chat(_) => {}
        }
        self.reset_chat(None);
        self.choose_default();
        Task::batch([operation::focus(EDITOR), self.leave_reading()])
    }

    /// Leaves the open conversation for `current`: nothing of the old one stays.
    fn reset_chat(&mut self, current: Option<String>) {
        self.current = current;
        // A conversation by voice begun in a new chat is followed only from that chat (issue #747).
        self.talk_started = false;
        // Answer numbers count per conversation (issue #745); what reads stops in the callers
        // ([`App::leave_reading`]).
        self.to_read.clear();
        self.ready_to_read = None;
        self.transcript = Transcript::default();
        self.chat_error = None;
        self.following = false;
        self.approvals.clear();
        self.deciding = None;
        self.cards.clear();
        self.expanded.clear();
        self.answers.clear();
        self.stream = None;
        self.follow_bottom = true;
        self.without.clear();
        self.mode = PermissionMode::Default;
        self.take_without = true;
    }

    /// The text to send: the one-line composer in mini and strip, else the composer.
    pub(crate) fn draft(&self) -> String {
        if self.is_glance() { self.mini.clone() } else { self.editor.text() }
    }

    fn send(&mut self) -> Task<Message> {
        if self.sending || self.transcript.running() {
            return Task::none();
        }
        if self.bot_gone() {
            return Task::none();
        }
        // A bot answers with its own client's rights: nothing of the assistant's to narrow.
        let (without, mode) = if self.chat_bot().is_some() {
            (Vec::new(), PermissionMode::Default)
        } else {
            (self.without.clone(), self.mode)
        };
        let effort = self.offered_effort();
        let Some(mut request) = chat::send_request(
            self.current.as_deref(),
            self.choice.as_ref(),
            effort,
            &self.draft(),
            (&without, mode),
            self.chat_bot(),
        ) else {
            return Task::none();
        };
        // With the speaker on the answer is read aloud, so it comes in two parts (issue #774): a short
        // spoken one and the shown one. Not in mini and strip, which read nothing.
        if let Request::SendMessage { spoken, .. } = &mut request {
            *spoken = self.read_aloud && !self.is_glance();
        }
        self.sending = true;
        self.ask(request, |result| Message::Sent(conversation_of(result)))
    }

    fn sent(&mut self, loaded: Loaded) -> Task<Message> {
        self.sending = false;
        match loaded {
            Ok((summary, entries)) => {
                self.editor = Content::new();
                self.mini.clear();
                let mut stopped = Task::none();
                if self.current.as_ref() != Some(&summary.id) {
                    let (without, mode) = (std::mem::take(&mut self.without), self.mode);
                    self.reset_chat(Some(summary.id.clone()));
                    stopped = self.leave_reading();
                    (self.without, self.mode) = (without, mode);
                    self.take_without = false;
                    self.transcript = Transcript::new(summary, entries);
                } else {
                    self.transcript.merge(summary, entries);
                }
                self.after_change(true);
                let read = self.read_ready();
                // The person sent something: the history goes to it, wherever it was scrolled.
                Task::batch([self.follow(), self.load_list(), self.scroll_to_newest(), read, stopped])
            }
            Err(refusal) => {
                let what = match refusal.code.as_str() {
                    "busy" if self.chat_bot().is_some() => tr("The bot is still answering in this conversation."),
                    "busy" => tr("The assistant is still answering in this conversation."),
                    _ => tr("Couldn't send the message."),
                };
                self.note_failure(what, &refusal);
                Task::none()
            }
        }
    }

    /// How talking by voice stands (issue #699), for the microphone of the composer: at the start
    /// and after the settings; never in mini and strip, which have no microphone.
    fn load_voice(&mut self) -> Task<Message> {
        if self.is_glance() {
            return Task::none();
        }
        self.voice_asked = true;
        self.ask(Request::VoiceStatus, |result| {
            Message::Voice(result.and_then(|response| match response {
                Response::Voice { voice } => Ok(voice),
                other => Err(Refusal::unexpected(&other)),
            }))
        })
    }

    /// Whether the composer offers the plus with the areas and the permission mode (issue #699): not
    /// in a bot's chat, which answers with its own client's rights – nothing of the assistant's to
    /// switch off.
    pub(crate) fn offers_areas_and_mode(&self) -> bool {
        self.chat_bot().is_none()
    }

    /// Whether the microphone can dictate (issue #699): the programs to record and to turn speech
    /// into text are there. "Hey Torro" need not be on.
    pub(crate) fn can_dictate(&self) -> bool {
        self.voice.as_ref().is_some_and(|voice| voice.installed && voice.can_listen())
    }

    /// The microphone (issue #699): records until a pause, stops a recording, or – without speech
    /// recognition – opens Settings › Voice, where it is set up.
    fn dictate(&mut self) -> Task<Message> {
        match self.dictation {
            Dictation::Stopping => Task::none(),
            Dictation::Recording => {
                self.dictation = Dictation::Stopping;
                self.ask(Request::StopDictation, |_| Message::DictationStopped)
            }
            Dictation::Idle if !self.can_dictate() => {
                self.close_menus();
                self.open_settings(Some(VOICE_PAGE))
            }
            Dictation::Idle => {
                self.dictation = Dictation::Recording;
                self.ask(Request::Dictate, |result| {
                    Message::Dictated(result.and_then(|response| match response {
                        Response::Transcript { text } => Ok(text),
                        other => Err(Refusal::unexpected(&other)),
                    }))
                })
            }
        }
    }

    /// The speaker of the composer (issue #745): switches reading aloud on or off for every chat
    /// window; off also stops what is read now.
    fn toggle_read_aloud(&mut self) -> Task<Message> {
        self.read_aloud = !self.read_aloud;
        self.read_aloud_switching = true;
        let on = self.read_aloud;
        let set = Request::SetSetting { key: torro_ipc::SettingKey::ReadAloud, value: serde_json::Value::Bool(on) };
        let task = self.ask(set, move |result| Message::ReadAloudSet(on, result.map(drop)));
        // Off stops only what this window reads: not "Hey Torro" or another window's preview.
        match on || self.reading.is_none() {
            true => task,
            false => Task::batch([task, self.stop_reading()]),
        }
    }

    /// Stops reading when it belongs to another conversation than the open one (issue #745).
    fn leave_reading(&mut self) -> Task<Message> {
        match self.reading.is_some() && self.reading_in != self.current {
            true => self.stop_reading(),
            false => Task::none(),
        }
    }

    /// Stops reading aloud at once (issue #745): what this window reads, and in its conversation by
    /// voice what the listener reads; the conversation goes on listening.
    fn stop_reading(&mut self) -> Task<Message> {
        self.reading = None;
        self.repeating = None;
        self.talk_speaking = false;
        self.ask(Request::StopSpeaking, |_| Message::ReadingStopped)
    }

    /// "Repeat" (issue #774): reads the spoken part of the answer `seq` of the open conversation again
    /// (`assistant.speak`), in place of what is read now; "Stop" stands under it meanwhile.
    fn repeat_spoken(&mut self, seq: u64) -> Task<Message> {
        let spoken =
            self.transcript.entries.iter().find(|entry| entry.seq == seq).and_then(|entry| match &entry.content {
                torro_ipc::assistant::EntryContent::Answer { spoken: Some(spoken), .. } => Some(spoken.clone()),
                _ => None,
            });
        let Some(text) = spoken.filter(|spoken| !spoken.trim().is_empty()) else { return Task::none() };
        self.reading = Some(seq);
        self.reading_in = self.current.clone();
        self.repeats += 1;
        let number = self.repeats;
        self.repeating = Some(number);
        self.ask(Request::Speak { text }, move |result| Message::Repeated(number, result.map(drop)))
    }

    /// Reads the answer that just began aloud, when reading aloud is on (issue #745): from its first
    /// sentence, while the model still writes (issue #747, `assistant.speak_answer`), once per message.
    /// Not in this window's conversation by voice: the listener reads its answers. A new one replaces
    /// the one being read: the core stops it.
    fn read_ready(&mut self) -> Task<Message> {
        let Some(seq) = self.ready_to_read.take() else { return Task::none() };
        let asked =
            (self.current.clone(), torro_ipc::assistant::answer_start(&self.transcript.entries).unwrap_or_default());
        if !self.read_aloud || self.is_glance() || self.own_talk().is_some() || self.read_for.as_ref() == Some(&asked) {
            return Task::none();
        }
        self.read_for = Some(asked);
        self.repeating = None;
        self.reading = Some(seq);
        self.reading_in = self.current.clone();
        let Some(conversation) = self.current.clone() else { return Task::none() };
        let read = Request::SpeakAnswer { conversation: conversation.clone() };
        self.ask(read, move |result| Message::Read(Some(conversation.clone()), seq, result.map(drop)))
    }

    /// The conversation by voice when it is this window's (issue #747): it runs in the open
    /// conversation, or this window started it in a new chat whose first sentence is not said yet.
    pub(crate) fn own_talk(&self) -> Option<&torro_ipc::Talk> {
        self.talk.as_ref().filter(|talk| match &talk.conversation {
            Some(id) => self.current.as_ref() == Some(id),
            None => self.talk_started && self.current.is_none(),
        })
    }

    /// Whether "Talk" can start a conversation by voice (issue #747): the microphone can record and
    /// turn speech into text, and no answer runs or is on its way.
    pub(crate) fn can_talk(&self) -> bool {
        self.offers_talk()
            && !self.transcript.running()
            && !self.sending
            && !self.talk_starting
            && self.dictation == Dictation::Idle
    }

    /// Whether the composer has "Talk" (issue #747): the microphone can record and turn speech into
    /// text, and the chat is the assistant's – the listener does not talk to a bot.
    pub(crate) fn offers_talk(&self) -> bool {
        self.can_dictate() && self.chat_bot().is_none()
    }

    /// "Talk" (issue #747): a conversation by voice in the open conversation, or in a new one the
    /// first sentence starts.
    fn start_talk(&mut self) -> Task<Message> {
        if !self.can_talk() {
            return Task::none();
        }
        self.close_menus();
        self.talk_starting = true;
        let start = Request::StartTalk { conversation: self.current.clone() };
        self.ask(start, |result| {
            Message::TalkStarted(result.and_then(|response| match response {
                Response::Talk { talk, .. } => Ok(talk),
                other => Err(Refusal::unexpected(&other)),
            }))
        })
    }

    /// Asks how the conversation by voice stands (issue #747); not in mini and strip.
    fn fetch_talk(&mut self) -> Task<Message> {
        if self.is_glance() || self.launch.open == Some(Open::Talk) {
            return Task::none();
        }
        self.ask(Request::TalkStatus, |result| {
            Message::TalkStatus(result.and_then(|response| match response {
                Response::Talk { talk, speaking, .. } => Ok((talk, speaking)),
                other => Err(Refusal::unexpected(&other)),
            }))
        })
    }

    /// The conversation by voice as the core says (issue #747). One this window started in a new chat
    /// has moved into the conversation its first sentence started: the window shows that one.
    fn take_talk(&mut self, talk: Option<torro_ipc::Talk>) -> Task<Message> {
        let followed = self.talk_started && self.current.is_none();
        self.talk = talk;
        match self.talk.as_ref().map(|talk| talk.conversation.clone()) {
            None => self.talk_started = false,
            Some(Some(id)) if followed => return self.select(id),
            Some(_) => {}
        }
        Task::none()
    }

    /// What was dictated goes into the field where the caret stands, nothing is sent; a space in
    /// front where the text does not end in one.
    fn dictated(&mut self, result: Result<String, Refusal>) -> Task<Message> {
        self.dictation = Dictation::Idle;
        match result {
            Ok(text) if text.trim().is_empty() => {
                self.note(tr("Nothing was understood.").to_owned());
                Task::none()
            }
            Ok(text) => {
                // What stands right before the caret decides the space, not the end of the field.
                // The text around the caret – or around a selection, which the text replaces – decides
                // the spaces: one in front after a word, one behind before a word.
                let cursor = self.editor.cursor();
                let (start, end) = match cursor.selection {
                    Some(other) if (other.line, other.column) < (cursor.position.line, cursor.position.column) => {
                        (other, cursor.position)
                    }
                    Some(other) => (cursor.position, other),
                    None => (cursor.position, cursor.position),
                };
                let line = |at: usize| self.editor.line(at).map(|line| line.text.into_owned()).unwrap_or_default();
                let (first, last) = (line(start.line), line(end.line));
                let before = first.get(..start.column).and_then(|text| text.chars().last());
                let after = last.get(end.column..).and_then(|text| text.chars().next());
                let word = |c: Option<char>| c.is_some_and(|c| !c.is_whitespace());
                let mut text = text.trim().to_owned();
                if word(before) {
                    text.insert(0, ' ');
                }
                if word(after) {
                    text.push(' ');
                }
                self.editor.perform(text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(text))));
                operation::focus(EDITOR)
            }
            Err(refusal) if refusal.code == "unsupported" => {
                self.note(tr("Speech recognition isn't set up.").to_owned());
                self.load_voice()
            }
            Err(refusal) => {
                self.note_failure(tr("Couldn't dictate."), &refusal);
                Task::none()
            }
        }
    }

    fn stop(&mut self) -> Task<Message> {
        let Some(id) = self.current.clone().filter(|_| self.transcript.running()) else { return Task::none() };
        self.ask(Request::CancelAnswer { conversation: id }, |result| Message::Stopped(result.map(|_| ())))
    }

    /// "Allow" or "Deny": only while the approval waits and no decision is on its way.
    fn decide(&mut self, id: String, answer: chat::Answer) -> Task<Message> {
        let open = self.approvals.get(&id).is_some_and(|approval| approval.state.is_pending());
        if !open || self.deciding.is_some() {
            return Task::none();
        }
        self.deciding = Some(id.clone());
        self.ask(chat::decide_request(&id, answer), move |result| Message::Decided(id.clone(), approval_of(result)))
    }

    fn decided(&mut self, id: String, result: Result<ApprovalInfo, Refusal>) -> Task<Message> {
        self.deciding = None;
        match result {
            Ok(approval) => {
                self.approvals.insert(id, approval);
                self.follow()
            }
            Err(refusal) => {
                let what = match refusal.code.as_str() {
                    "conflict" => tr("This approval was already decided."),
                    _ => tr("Couldn't decide the approval."),
                };
                self.note_failure(what, &refusal);
                self.fetch_approval(id)
            }
        }
    }

    fn rename(&mut self) -> Task<Message> {
        let Some(Dialog::Rename { id, title }) = self.dialog.clone() else { return Task::none() };
        let title = title.trim().to_owned();
        if title.is_empty() {
            return Task::none();
        }
        self.dialog = None;
        if let Some(conversation) = self.conversations.iter_mut().find(|c| c.id == id) {
            conversation.title = title.clone();
        }
        if let Some(summary) = self.transcript.summary.as_mut().filter(|s| s.id == id) {
            summary.title = title.clone();
        }
        let failed = tr("Couldn't rename the conversation.");
        self.ask(Request::RenameConversation { id, title }, move |result| Message::Changed(result.map(|_| ()), failed))
    }

    fn delete(&mut self) -> Task<Message> {
        let Some(Dialog::Delete { id, .. }) = self.dialog.take() else { return Task::none() };
        let request = Request::DeleteConversation { id: id.clone() };
        self.ask(request, move |result| Message::Deleted(id.clone(), result.map(|_| ())))
    }

    /// The ids under "Recent", whatever the search shows (issue #697): what "Archive all…" and "Delete
    /// all…" take – every conversation that is neither pinned nor archived, those with bots too.
    fn recent_ids(&self) -> Vec<String> {
        chat::sidebar_groups(&self.conversations, "").1.iter().map(|c| c.id.clone()).collect()
    }

    /// "Archive" or "Restore" (issue #697): shown at once, then asked of the core in pieces of
    /// [`MAX_BULK_IDS`]; the list is read anew after each answer.
    fn archive(&mut self, ids: Vec<String>, archived: bool) -> Task<Message> {
        self.close_menus();
        for conversation in self.conversations.iter_mut().filter(|c| ids.contains(&c.id)) {
            conversation.archived = archived;
        }
        let failed = match (archived, ids.len() > 1) {
            (true, false) => tr("Couldn't archive the conversation."),
            (true, true) => tr("Couldn't archive the conversations."),
            (false, false) => tr("Couldn't restore the conversation."),
            (false, true) => tr("Couldn't restore the conversations."),
        };
        let tasks: Vec<Task<Message>> = ids
            .chunks(MAX_BULK_IDS)
            .map(|piece| {
                let request = Request::ArchiveConversations { ids: piece.to_vec(), archived };
                self.ask(request, move |result| Message::Changed(result.map(|_| ()), failed))
            })
            .collect();
        Task::batch(tasks)
    }

    /// The question's yes to "Archive all…", "Delete all…" or "Empty archive…".
    fn confirm_many(&mut self) -> Task<Message> {
        // Only those still listed: one gone meanwhile (another window, a client, the retention) would make
        // the core refuse the whole piece.
        let listed = |ids: Vec<String>| -> Vec<String> {
            ids.into_iter().filter(|id| self.conversations.iter().any(|c| c.id == *id)).collect()
        };
        let dialog = match self.dialog.take() {
            Some(Dialog::ArchiveMany { ids }) => Some(Dialog::ArchiveMany { ids: listed(ids) }),
            Some(Dialog::DeleteMany { ids, archive }) => Some(Dialog::DeleteMany { ids: listed(ids), archive }),
            other => other,
        };
        match dialog {
            Some(Dialog::ArchiveMany { ids }) => self.archive(ids, true),
            Some(Dialog::DeleteMany { ids, .. }) => {
                let tasks: Vec<Task<Message>> = ids
                    .chunks(MAX_BULK_IDS)
                    .map(|piece| {
                        let ids = piece.to_vec();
                        let request = Request::DeleteConversations { ids: ids.clone() };
                        self.ask(request, move |result| Message::DeletedMany(ids.clone(), result.map(|_| ())))
                    })
                    .collect();
                Task::batch(tasks)
            }
            other => {
                self.dialog = other;
                Task::none()
            }
        }
    }

    /// The core's answer to deleting `ids` at once: as [`App::deleted`] for each, so the open one
    /// makes room for a new chat; the list is read anew either way, as some may be gone.
    fn deleted_many(&mut self, ids: Vec<String>, result: Result<(), Refusal>) -> Task<Message> {
        match result {
            Ok(()) => {
                self.conversations.retain(|c| !ids.contains(&c.id));
                self.note(tr("Conversations deleted").to_owned());
                let open = self.current.as_ref().is_some_and(|id| ids.contains(id));
                let task = if open { self.new_chat() } else { Task::none() };
                Task::batch([task, self.load_list()])
            }
            Err(refusal) => {
                self.note_failure(tr("Couldn't delete the conversations."), &refusal);
                // Past the check of the ids the others are gone (`error`): the open one may be among them.
                let open = refusal.code != "not-found" && self.current.as_ref().is_some_and(|id| ids.contains(id));
                let task = if open { self.new_chat() } else { Task::none() };
                Task::batch([task, self.load_list()])
            }
        }
    }

    fn deleted(&mut self, id: String, result: Result<(), Refusal>) -> Task<Message> {
        match result {
            Ok(()) => {
                self.conversations.retain(|c| c.id != id);
                self.note(tr("Conversation deleted").to_owned());
                let task = if self.current.as_ref() == Some(&id) { self.new_chat() } else { Task::none() };
                Task::batch([task, self.load_list()])
            }
            Err(refusal) => {
                self.note_failure(tr("Couldn't delete the conversation."), &refusal);
                Task::none()
            }
        }
    }

    // -- The settings in the content area (task S.18) -------------------------------------------

    /// The gear, or "Settings" in the sidebar: the settings take the content area, or give it back
    /// to the chat. Nothing of the chat is lost – the settings are a second view of the same
    /// window, not a second window. Giving the area back only gets here once the open section has
    /// nothing unsaved left ([`App::settings_question`]).
    fn toggle_settings(&mut self) -> Task<Message> {
        self.close_menus();
        if self.settings.is_some() {
            self.settings = None;
            // Speech recognition may have been set up meanwhile (issue #699).
            return self.load_voice();
        }
        self.open_settings(None)
    }

    /// The settings in the content area, at `section` when one is named (`--settings <section>`,
    /// an unknown word leaves the first section open) – a fresh [`Host`] with the assistant's
    /// sections and its first load, which goes over [`Backend::settings`] and never over this
    /// window's own requests.
    fn open_settings(&mut self, section: Option<&str>) -> Task<Message> {
        // Settings open already leave their state in the memory first, for the new host.
        drop(self.settings.take());
        let (host, load) = Host::with_memory(
            self.backend.settings(),
            settings::SECTIONS.to_vec(),
            settings::wanted(),
            self.settings_memory.clone(),
            // Without the main application the suite\'s pages are this app\'s too (issue #408).
            !torro_gui::installed::main_app(),
        );
        // AI, always, also with the main application (issue #410): the models it chats with.
        let mut host = host.with_accounts(AppId::ASSISTANT, &[], true);
        // What keys and MCP clients may do with the assistant and its bots (issue #531).
        host = host.with_outside(AppId::ASSISTANT);
        // The section list as wide as it was dragged (issue #604); the demo keeps it fixed.
        if !self.launch.demo {
            host = host.with_widths(MAIN);
        }
        if let Some(id) = section {
            host.open(id);
        }
        self.settings = Some(SettingsPane { host, sections: settings::State::default() });
        load.map(Message::SettingsHost)
    }

    /// The rights of the bot with this id or of the assistant (`None`), task S.34: what "Set up
    /// access", "Change access", "Access…" and "What the bot may use" open – a bot's on a page below
    /// Bots, the assistant's under Access (issue #763). Open settings switch to it; closed ones open
    /// there, with their first load.
    fn open_access(&mut self, bot: Option<String>) -> Task<Message> {
        self.close_menus();
        let load = match self.settings.is_some() {
            true => Task::none(),
            false => self.open_settings(None),
        };
        if let Some(pane) = self.settings.as_mut() {
            let section = match bot {
                Some(_) => settings::bots::BOTS,
                None => settings::access::ID,
            };
            settings::show_access(&mut pane.sections, bot);
            pane.host.open(section);
        }
        load
    }

    /// What has to go out before `message` takes the settings away: a change of rights in Access
    /// that waits for its clock ([`settings::save_now`]). The host sends it as any other change; its
    /// answer may come after the settings are gone, which loses nothing – the core has it. `None`
    /// while nothing waits or `message` keeps the settings.
    fn save_before_leaving(&mut self, message: &Message) -> Option<Task<Message>> {
        let leaving = self.leaves_settings(message)
            || matches!(message, Message::SettingsHost(torro_settings_ui::host::Message::Back));
        if !leaving {
            return None;
        }
        self.save_settings()
    }

    /// Sends what the open sections still have to save ([`settings::save_now`]) through the host;
    /// `None` while nothing waits or no settings are open. For every way the settings close:
    /// [`App::save_before_leaving`] for the window's own, [`App::settings_outcome`] for a section
    /// that closes them ("Discard", "Edit" at a bot).
    fn save_settings(&mut self) -> Option<Task<Message>> {
        let pane = self.settings.as_mut()?;
        match pane.host.act(settings::save_now(&mut pane.sections)) {
            Outcome::Work(task) => Some(task.map(Message::SettingsHost)),
            _ => None,
        }
    }

    /// The content area shows the chat or the bot editor again: the settings step aside. A click in
    /// the sidebar – a chat, a bot, "New chat", "New bot" – comes through here, once
    /// [`App::settings_question`] has let the message through.
    pub(crate) fn leave_settings(&mut self) {
        self.settings = None;
    }

    /// The question "Discard changes?" instead of `message`, while `message` would take the
    /// content area back from the settings and the open section holds something unsaved
    /// ([`settings::closing`]); `None` lets `message` run as usual.
    ///
    /// **The question belongs to the section, not to the window:** only it knows what is typed, it
    /// already builds the dialog for its own back button, and after "Discard" it closes the
    /// settings itself ([`Outcome::Close`]) – so the gear, Escape and a click in the sidebar all
    /// ask the same question and let go at the same moment. The window does not remember where the
    /// click wanted to go: once the question is answered the content area is back, and the person
    /// clicks where they meant to.
    fn settings_question(&mut self, message: &Message) -> Option<Task<Message>> {
        let pane = self.settings.as_ref()?;
        if !self.leaves_settings(message) {
            return None;
        }
        let question = settings::closing(&pane.sections, pane.host.data())?;
        Some(self.update(Message::SettingsPage(question)))
    }

    /// Whether `message` would take the content area back from the open settings: the gear, a
    /// click in the sidebar, and Escape once nothing of the window or of the section lies over
    /// them (that is the step of [`Message::Escape`] where the settings close).
    fn leaves_settings(&self, message: &Message) -> bool {
        match message {
            Message::ToggleSettings => true,
            Message::Select(_)
            | Message::NewChat
            | Message::Switcher(Switcher::New)
            | Message::OpenBot(_)
            | Message::NewBotChat(_)
            | Message::NewBot
            | Message::EditBot(_) => true,
            Message::Escape => {
                let section_dialog = self.settings.as_ref().and_then(|pane| settings::escape(&pane.sections)).is_some();
                self.dialog.is_none() && !section_dialog && self.menu.is_none() && !self.drawer
            }
            _ => false,
        }
    }

    /// What the host asks of the window: the browser, the control centre, the bot editor are the
    /// app's; everything else the host did itself.
    fn settings_outcome(&mut self, outcome: Outcome<AppAction>) -> Task<Message> {
        match outcome {
            Outcome::None => Task::none(),
            Outcome::Work(task) => task.map(Message::SettingsHost),
            // A section changed something: the window loads what it shows itself – the engines,
            // the profile, the client, the conversations – right now instead of on its next tick,
            // so a changed default or a new bot is in the chat at once.
            Outcome::Changed => Task::batch([self.fetch_appearance(), self.load_all(), self.load_voice()]),
            Outcome::Close => {
                let save = self.save_settings().unwrap_or_else(Task::none);
                self.settings = None;
                Task::batch([save, self.load_voice()])
            }
            Outcome::OpenUrl(url) => open_link(url),
            Outcome::Answer(result) => {
                // The answer belongs to the section that asked for it ("Say something").
                if let Some(pane) = self.settings.as_mut() {
                    settings::answered(&mut pane.sections, &result);
                }
                Task::none()
            }
            Outcome::ControlCentre(section) => {
                self.open_section(&section);
                Task::none()
            }
            // "Edit" at a bot: its editor takes the content area, so the settings close.
            Outcome::App(AppAction::EditBot(id)) => {
                let save = self.save_settings().unwrap_or_else(Task::none);
                Task::batch([save, self.edit_bot(id)])
            }
        }
    }

    fn toggle_sidebar(&mut self) -> Task<Message> {
        if self.chat_only {
            return Task::none();
        }
        if !shell::has_sidebar(self.class()) {
            self.menu = None;
            self.drawer = !self.drawer;
            return Task::none();
        }
        self.close_menus();
        self.sidebar = self.sidebar.toggled();
        if self.launch.demo {
            return Task::none();
        }
        ui_state::save_task(MAIN, UiState { sidebar: self.sidebar })
    }

    fn close_menus(&mut self) {
        self.menu = None;
        self.drawer = false;
    }

    /// The states of `--open` that need the conversations listed: the dialogs name the title.
    fn open_launched(&mut self) {
        if std::mem::replace(&mut self.opened, true) {
            return;
        }
        let id = demo::TOOLS.to_owned();
        let title = self.title_of(&id);
        match self.launch.open {
            Some(Open::Rename) => self.dialog = Some(Dialog::Rename { id, title }),
            Some(Open::Delete) => self.dialog = Some(Dialog::Delete { id, title }),
            Some(Open::DeleteAll) => self.dialog = Some(Dialog::DeleteMany { ids: self.recent_ids(), archive: false }),
            _ => {}
        }
    }

    // -- Helpers --------------------------------------------------------------------------------

    /// The title of the conversation `id` as listed.
    pub(crate) fn title_of(&self, id: &str) -> String {
        self.conversations.iter().find(|c| c.id == id).map(|c| c.title.clone()).unwrap_or_default()
    }

    /// The areas the composer offers to switch off.
    pub(crate) fn switchable(&self) -> Vec<Area> {
        match &self.access {
            Access::Granted(rights) => chat::switchable_areas(Some(rights)),
            Access::Missing | Access::Unknown => Vec::new(),
        }
    }

    /// The listed models of `engine` the picker offers: an AI account's that can call tools and are
    /// turned on (task S.26), in the account's order; an agent's own. `None` while not listed, the
    /// error's text when the listing failed.
    pub(crate) fn offered_models(&self, engine: &str) -> Option<Result<Vec<AiModel>, &str>> {
        if let Some(agent) = self.engines.iter().find(|e| e.id == engine && e.kind == EngineKind::Agent) {
            return chat::engine_models(agent, None).map(Ok);
        }
        self.models.get(engine).map(|listed| match listed {
            Ok(models) => Ok(chat::tool_models(models).into_iter().filter(|model| model.enabled).collect()),
            Err(err) => Err(err.as_str()),
        })
    }

    /// Whether `choice` names a model of an AI account that is not turned on (task S.26): its
    /// account's list is in and has the model off, or not at all. The core answers with
    /// "Automatic" then, so the picker and the bot editor say "Turned off in Settings › AI".
    /// `false` for "Automatic", for an agent's model and while the list is not in or failed.
    pub(crate) fn turned_off(&self, choice: &Choice) -> bool {
        let Some(model) = &choice.model else { return false };
        let ai = self.engines.iter().any(|engine| engine.id == choice.engine && engine.kind == EngineKind::Ai);
        match self.models.get(&choice.engine) {
            Some(Ok(listed)) if ai => !listed.iter().any(|known| &known.id == model && known.enabled),
            _ => false,
        }
    }

    /// There are AI accounts, but no model is turned on at any of them (task S.26): the core lists
    /// none of them as an engine. The welcome says so instead of "No AI account yet".
    pub(crate) fn no_model_on(&self) -> bool {
        self.engines_loaded && self.ai_accounts && !self.engines.iter().any(|engine| engine.kind == EngineKind::Ai)
    }

    /// The composer offers an effort for the chosen engine and model (issue #131).
    pub(crate) fn effort_offered(&self) -> bool {
        let Some(choice) = &self.choice else { return false };
        let engine = self.engines.iter().find(|engine| engine.id == choice.engine);
        let models = self.offered_models(&choice.engine).and_then(Result::ok);
        chat::can_reason(engine, choice, models.as_deref())
    }

    /// The effort the next message carries: the chosen one where the model takes it, else none.
    pub(crate) fn offered_effort(&self) -> Option<Effort> {
        self.effort.filter(|_| self.effort_offered())
    }

    /// Opens the control centre on `section` (`torroworkplace ui --section …`: the accounts, or
    /// what a settings section points at); nothing starts in the demo or in tests.
    fn open_section(&mut self, section: &str) {
        self.close_menus();
        #[cfg(test)]
        self.sections.push(section.to_owned());
        #[cfg(not(test))]
        if !self.launch.demo {
            torro_gui::connect::open_section(section);
        }
    }

    /// A notice at the bottom of the window.
    fn note(&mut self, text: String) {
        self.notice = Some(Notice { text, shown: Instant::now() });
    }

    /// The notice for something that failed: what did not work; the reason goes to the log.
    fn note_failure(&mut self, what: &str, refusal: &Refusal) {
        eprintln!("{}: {what} ({}: {})", MAIN.binary, refusal.code, refusal.message);
        self.note(what.to_owned());
    }
}

/// The settings section a demo state opens and what it then presses on the page (issue #547), so the
/// profile editor, its save bar, the tabs of Access and a bot's rights can be photographed; `None`
/// for every other state.
fn settings_state(open: Option<Open>) -> Option<(&'static str, Vec<settings::Message>)> {
    use settings::{access, bots, profile};
    // The profile is the tab Profile of General since issue #763.
    let profile_tab = settings::Message::GeneralTab(1);
    let open_profile = settings::Message::Profile(profile::Message::OpenProfile);
    match open? {
        Open::ProfileEditor => Some(("general", vec![profile_tab, open_profile])),
        Open::ProfileChanged => Some((
            "general",
            vec![profile_tab, open_profile, settings::Message::Profile(profile::Message::Name("Jarvis".into()))],
        )),
        Open::AccessSelections => Some((access::ID, vec![settings::Message::Access(access::Message::Tab(1))])),
        // The tab Bots of Access is gone (issue #763): a bot's rights, below Bots.
        Open::AccessBots => {
            Some((bots::BOTS, vec![settings::Message::Bots(bots::Message::Access(crate::demo::TRAVEL.into()))]))
        }
        _ => None,
    }
}

/// The conversation the main window starts on in a demo state; a new chat otherwise.
fn start_conversation(open: Option<Open>) -> Option<String> {
    match open {
        Some(Open::Tools | Open::Menu | Open::Rename | Open::Delete | Open::Reading | Open::Talk) => {
            Some(demo::TOOLS.into())
        }
        Some(Open::Approval) => Some(demo::APPROVAL.into()),
        Some(Open::Terminal) => Some(demo::TERMINAL.into()),
        Some(Open::Streaming) => Some(demo::STREAMING.into()),
        Some(Open::Forbidden) => Some(demo::FORBIDDEN.into()),
        Some(Open::Bot) => Some(demo::BOT_CHAT.into()),
        Some(Open::BotAsked) => Some(demo::ASKED.into()),
        Some(Open::BotGone) => Some(demo::GONE.into()),
        Some(Open::BotProposal) => Some(demo::PROPOSAL.into()),
        Some(Open::Cards) => Some(demo::CONTENT.into()),
        _ => None,
    }
}

/// A conversation from an answer of `assistant.get_conversation` or `assistant.send_message`.
fn conversation_of(result: Result<Response, Refusal>) -> Loaded {
    match result? {
        Response::Conversation { conversation, entries } => Ok((conversation, entries)),
        other => Err(Refusal::unexpected(&other)),
    }
}

/// An approval from an answer of `approvals.approval_status` or `approvals.decide_approval`.
fn approval_of(result: Result<Response, Refusal>) -> Result<ApprovalInfo, Refusal> {
    match result? {
        Response::Approval { approval } => Ok(approval),
        other => Err(Refusal::unexpected(&other)),
    }
}

/// The web address of `url`, or `None` when it is none: **only** `http` and `https`, because the
/// text an answer is written in is the model's (task S.18). A scheme of its own (`torro://…`), a
/// start parameter, a path, a `file://` – anything else – opens nothing, so no answer can reach
/// the settings, a file or another program.
pub(crate) fn web_link(url: &str) -> Option<&str> {
    (url.starts_with("http://") || url.starts_with("https://")).then_some(url)
}

/// Opens a web address of an answer in the browser; only what [`web_link`] allows.
fn open_link(url: String) -> Task<Message> {
    if web_link(&url).is_none() {
        return Task::none();
    }
    Task::future(async move {
        if let Err(err) = webbrowser::open(&url) {
            eprintln!("cannot open the link in a browser: {err}");
        }
    })
    .discard()
}

/// Asks where a copy of a file card's PDF goes, with the system's save dialog suggesting `name` and
/// offering PDFs only; `None` when it is cancelled. The dialog asks before replacing a file.
async fn pick_save_path(name: String) -> Option<PathBuf> {
    let chosen = rfd::AsyncFileDialog::new()
        .set_title(tr("Save a copy of the PDF"))
        .set_file_name(name)
        .add_filter(torro_ipc::tools::ToolId::Pdf.title(), &["pdf"])
        .save_file()
        .await;
    chosen.map(|file| file.path().to_path_buf())
}

/// The main window's keys: `[` for the sidebar when no text field took it, and Ctrl + N (⌘ N) for
/// a new chat, also from the composer (the key types nothing there).
fn window_key(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }) = event else {
        return None;
    };
    if modifiers.command() && !modifiers.shift() && !modifiers.alt() && key.to_latin(physical_key) == Some('n') {
        return Some(Message::NewChat);
    }
    (status == iced::event::Status::Ignored && shell::is_sidebar_key(&key, modifiers)).then_some(Message::ToggleSidebar)
}

/// The composer's keys: `Enter` sends, `Shift + Enter` starts a new line, the rest as usual.
pub(crate) fn composer_binding(press: KeyPress) -> Option<Binding<Message>> {
    let enter = matches!(press.key.as_ref(), keyboard::Key::Named(keyboard::key::Named::Enter));
    if enter && !press.modifiers.shift() {
        return Some(Binding::Custom(Message::Send));
    }
    Binding::from_key_press(press)
}

/// The picture of a link card as the core sent it (`assistant.get_card_picture`), decoded to draw: PNG, JPEG, GIF or
/// WebP, as the chat window decodes its previews; `None` for data that is no such picture (the kind's icon stays).
fn card_picture(picture: &torro_ipc::chat::ChatPicture) -> Option<iced::widget::image::Handle> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(&picture.base64.0).ok()?;
    let pixels = image::load_from_memory(&bytes).ok()?.into_rgba8();
    let (width, height) = pixels.dimensions();
    Some(iced::widget::image::Handle::from_rgba(width, height, pixels.into_raw()))
}

/// Whether the history follows its newest entry after a scroll event, and whether it jumps to the end now:
/// `(follow, jump)`. It follows when it is at the bottom (within `near`), and when its content grew
/// while the offset stayed (an answer streaming in, a card loading): that is no scrolling by the person.
/// A person who scrolled up (the offset changed, away from the bottom) stops it; back at the bottom
/// starts it again.
fn history_follow(
    following: bool,
    seen: Option<(f32, f32)>,
    content: f32,
    offset: f32,
    hidden: f32,
    near: f32,
) -> (bool, bool) {
    let at_bottom = hidden - offset <= near;
    let grew = following && seen.is_some_and(|(before, at)| content > before && (offset - at).abs() < 0.5);
    let follow = at_bottom || grew;
    (follow, follow && (!following || grew))
}

mod bots;
#[cfg(test)]
mod tests;
