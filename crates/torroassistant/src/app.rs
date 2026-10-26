//! The assistant's window (task C.4, `docs/assistent.md`, "Die App `torroassistant`"): its state,
//! what every click and answer does ([`App::update`]), and how it follows the core.
//!
//! Two kinds of window from one program ([`Window`]): the main window (class `torro-assistant`)
//! with the conversations in the sidebar – pinned on top, then the newest, with a search – and the
//! chat beside them; and a single chat window (`--chat <id>|new`, class `torro-assistant-chat`,
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
use torro_gui::theme::{self, Appearance};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets;
use torro_gui::zoom::{self, Zoom};
use torro_ipc::api::{Area, Rights};
use torro_ipc::assistant::Mode as PermissionMode;
use torro_ipc::assistant::{
    Card, CardEntry, ChatWindow, ConversationSummary, Engine, EngineKind, Entry, EntryContent, Profile,
};
use torro_ipc::bots::{Bot, BotSummary};
use torro_ipc::tools::PdfTemplateId;
use torro_ipc::{ApprovalInfo, Request, Response};
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
/// The id of the bot editor's name field: "Save" goes there while the name keeps it back.
pub const BOT_NAME: &str = "assistant-bot-name";
/// The id of the bot editor's scrolling form: a "Save" that finds a problem goes to its top.
pub const BOT_FORM: &str = "assistant-bot-form";
/// The id of the title field in the rename dialog.
pub const RENAME: &str = "assistant-rename";
/// The control centre's AI page where accounts are added and their models turned on.
pub const AI_PAGE: &str = settings::AI_SECTION;

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
    /// The app switcher at the top of the sidebar.
    Switcher,
    /// The engine and model picker of the composer.
    Picker,
    /// The effort of the composer (issue #131).
    Effort,
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
    /// The engine and model picker of the bot editor.
    BotPicker,
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
}

/// What the window knows of the assistant's own client (kind `assistant`).
#[derive(Debug, Clone, PartialEq)]
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
    /// "Allow" (`true`) or "Deny" on the approval card: a person's click.
    Decide(String, bool),
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
    /// The core's answer to a button of a file card; `Ok` with what to say then, the text says what
    /// failed.
    FileDone(Result<Option<&'static str>, Refusal>, &'static str),
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
    /// "Manage models…" at the end of the picker and under "No model is turned on" (task S.26): the
    /// control centre's AI section, where the models of each AI account are turned on and off.
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
    /// The core's answer to a rename or pin; the text says what failed.
    Changed(Result<(), Refusal>, &'static str),
    /// The conversation with this id was deleted (or not).
    Deleted(String, Result<(), Refusal>),
    /// Close the dialog.
    CloseDialog,
    /// What is typed into the sidebar's search.
    Search(String),
    /// The pointer is on the sidebar row of this conversation (`None`: on none).
    Hover(Option<String>),
    /// From the app switcher and the buttons beside it.
    Switcher(Switcher),
    /// Start or raise another app of the suite.
    OpenApp(String),
    AppOpened(Result<(), String>),
    /// "Set up access", "Change access": the settings at Access (task S.34), where the person
    /// chooses what the assistant – or the bot of the open conversation – may use.
    SetUp,
    /// "Add an AI account": the control centre's AI section, where AI accounts are added.
    AddAiAccount,
    /// The gear in the title band, "Settings" in the sidebar: the settings in the content area, or
    /// back to the chat (task S.18).
    ToggleSettings,
    /// One of the settings' own messages ([`Host`]).
    SettingsHost(torro_settings_ui::host::Message),
    /// What a widget of the open settings section reported.
    SettingsPage(settings::Message),
    /// The system's folder chooser of Tools closed (task WZ.6): the folder chosen, `None` when
    /// it was cancelled.
    FolderPicked(Option<PathBuf>),
    /// The system's logo chooser of Tools closed for this template: the picture chosen, `None`
    /// when it was cancelled.
    LogoPicked(PdfTemplateId, Option<PathBuf>),
    /// The system's folder chooser of the allowed folders closed (Tools, task WD.4): the folder
    /// chosen, `None` when it was cancelled.
    AllowedFolderPicked(Option<PathBuf>),
    /// A link in an answer.
    OpenLink(String),
    /// "Retry" in the error state.
    Retry,
    /// The panel button, `[`.
    ToggleSidebar,
    UiState(UiState),
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
    /// What a new chat starts with, as set in the control centre (`settings.assistant_default`);
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
    /// The tool lines unfolded, by number.
    pub(crate) expanded: HashSet<u64>,
    /// The answers as Markdown blocks, by number, with the text they were made from: for the
    /// answer in [`App::stream`] what of it shows ([`crate::stream::display`]).
    pub(crate) answers: HashMap<u64, (String, Vec<markdown::Item>)>,
    /// The answer that appears letter by letter; `None` when every answer shows whole.
    pub(crate) stream: Option<Stream>,
    /// The history is scrolled to the bottom and follows the newest entry; scrolled up, it stays.
    pub(crate) follow_bottom: bool,

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

    pub(crate) menu: Option<Menu>,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) search: String,
    /// The sidebar row under the pointer.
    pub(crate) hover: Option<String>,
    /// Whether the sidebar drawer is open (medium and narrow layouts).
    pub(crate) drawer: bool,
    /// The sidebar the user chose for the wide layout, kept in `ui.json`.
    pub(crate) sidebar: SidebarMode,
    pub(crate) notice: Option<Notice>,
    /// The settings in the content area, while they are open (task S.18).
    pub(crate) settings: Option<SettingsPane>,
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
            expanded: HashSet::new(),
            answers: HashMap::new(),
            stream: None,
            follow_bottom: true,
            editor: Content::new(),
            mini: String::new(),
            choice: None,
            effort: None,
            chosen: Chosen::Default,
            without: Vec::new(),
            mode: PermissionMode::Default,
            take_without: true,
            sending: false,
            menu: match open {
                Some(Open::Picker) => Some(Menu::Picker),
                Some(Open::Effort) => Some(Menu::Effort),
                Some(Open::Mode) => Some(Menu::Mode),
                Some(Open::Switcher) => Some(Menu::Switcher),
                Some(Open::Menu) => Some(Menu::Conversation { id: demo::TOOLS.into(), at: MenuAt::Header }),
                Some(Open::BotMenu) => Some(Menu::Bot { id: demo::TRAVEL.into() }),
                _ => None,
            },
            dialog: None,
            search: String::new(),
            hover: None,
            // Without a sidebar the app switcher lives in the drawer.
            drawer: matches!(open, Some(Open::Drawer | Open::Switcher | Open::BotMenu)),
            sidebar: SidebarMode::default(),
            notice: None,
            settings: None,
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
        }
        if app.current.is_none() {
            tasks.push(operation::focus(EDITOR));
        }
        // `--settings <section>`: the window opens with the settings in its content area; Access in
        // a bot's window (`--bot <id> --settings access`) at that bot.
        if let Some(section) = app.launch.settings.clone() {
            tasks.push(app.open_settings(Some(&section)));
            if let (Some(pane), Window::Bot(bot)) = (app.settings.as_mut(), &app.launch.window)
                && section == settings::access::ID
            {
                settings::show_access(&mut pane.sections, Some(bot.clone()));
            }
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
            iced::system::theme_changes().map(Message::SystemMode),
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
            Message::Resized(size) => self.size = size,
            Message::CoreReady => {
                self.waiting = false;
                // The chat window's conversation too: it waited with the rest (issue #206).
                return Task::batch([self.load_all(), self.open_current()]);
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
                self.default = settings.assistant_default;
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
            Message::Decide(id, allow) => return self.decide(id, allow),
            Message::Cards(id, result) => self.take_cards(&id, result),
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
            Message::Select(id) => return self.select(id),
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
            Message::ManageModels => self.open_section(AI_PAGE),
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
            Message::Changed(Ok(()), _) => return self.load_list(),
            Message::Changed(Err(refusal), failed) => {
                self.note_failure(failed, &refusal);
                return self.load_list();
            }
            Message::Deleted(id, result) => return self.deleted(id, result),
            Message::CloseDialog => self.dialog = None,
            Message::Search(text) => self.search = text,
            Message::Hover(id) => self.hover = id,
            Message::Switcher(Switcher::Toggle) => self.toggle_menu(Menu::Switcher),
            Message::Switcher(Switcher::OpenApp(app)) => return self.update(Message::OpenApp(app.to_owned())),
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
            Message::AddAiAccount => self.open_section(AI_PAGE),
            Message::ToggleSettings => return self.toggle_settings(),
            Message::SettingsHost(message) => {
                let Some(pane) = self.settings.as_mut() else { return Task::none() };
                // The sections see the host's sends and loads first: Access settles its draft by
                // them (task S.34).
                settings::received(&mut pane.sections, &message);
                let outcome = pane.host.update(message);
                return self.settings_outcome(outcome);
            }
            Message::SettingsPage(message) => {
                let outcome = {
                    let Some(pane) = self.settings.as_mut() else { return Task::none() };
                    let action = settings::update(&mut pane.sections, message, pane.host.data());
                    pane.host.act(action)
                };
                return self.settings_outcome(outcome);
            }
            Message::FolderPicked(folder) => {
                let request = folder.map(|folder| settings::tools::folder_request(&folder));
                return self.settings_send(request);
            }
            Message::LogoPicked(template, logo) => {
                let data = self.settings.as_ref().map(|pane| pane.host.data());
                let request = data.zip(logo).map(|(data, logo)| settings::tools::logo_request(data, template, &logo));
                return self.settings_send(request);
            }
            Message::AllowedFolderPicked(folder) => {
                let data = self.settings.as_ref().map(|pane| pane.host.data());
                let request =
                    data.zip(folder).map(|(data, folder)| settings::tools::added_folder_request(data, &folder));
                return self.settings_send(request);
            }
            Message::OpenLink(url) => return open_link(url),
            Message::Retry => {
                self.list_error = None;
                self.chat_error = None;
                return Task::batch([self.load_all(), self.open_current()]);
            }
            Message::ToggleSidebar => return self.toggle_sidebar(),
            Message::UiState(state) => self.sidebar = state.sidebar,
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
                    self.settings = None;
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
                return self.resolve_bot_window();
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
                Task::batch([self.fetch_approvals(), self.fetch_cards(), self.scroll_to_newest()])
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
                if self.transcript.merge(summary, entries) {
                    self.after_change(true);
                }
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
                Task::batch(tasks.into_iter().chain([cards]))
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
        self.answers.get(&seq).is_none_or(|(shown, _)| !shown.is_empty())
    }

    /// The number of the answer still appearing: the entries after it wait until it shows whole.
    pub(crate) fn appearing(&self) -> Option<u64> {
        self.stream.as_ref().map(|stream| stream.seq)
    }

    /// The history scrolled or grew: it follows the newest entry while it is at the bottom, and
    /// stays where the person scrolled to otherwise.
    fn history_scrolled(&mut self, viewport: scrollable::Viewport) -> Task<Message> {
        let hidden = (viewport.content_bounds().height - viewport.bounds().height).max(0.0);
        let at_bottom = hidden - viewport.absolute_offset().y <= theme::space::l();
        let back = at_bottom && !self.follow_bottom;
        self.follow_bottom = at_bottom;
        // Back at the bottom: from now on the history keeps to it as it grows.
        if back { self.scroll_to_newest() } else { Task::none() }
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

    /// The cards of the conversation `id`, when it is still the open one. A refusal (an old core, a
    /// core that is restarting) leaves the cards as they were.
    fn take_cards(&mut self, id: &str, result: Result<Vec<CardEntry>, Refusal>) {
        if self.current.as_deref() != Some(id) {
            return;
        }
        match result {
            Ok(cards) => self.cards = cards.into_iter().map(|entry| (entry.seq, entry.card)).collect(),
            Err(refusal) => eprintln!("{}: cannot read the cards ({}: {})", MAIN.binary, refusal.code, refusal.message),
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
        if self.chosen != Chosen::Person {
            self.chosen = Chosen::Default;
        }
        self.choose_default();
        self.open_current()
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
        operation::focus(EDITOR)
    }

    /// Leaves the open conversation for `current`: nothing of the old one stays.
    fn reset_chat(&mut self, current: Option<String>) {
        self.current = current;
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
        let Some(request) = chat::send_request(
            self.current.as_deref(),
            self.choice.as_ref(),
            effort,
            &self.draft(),
            (&without, mode),
            self.chat_bot(),
        ) else {
            return Task::none();
        };
        self.sending = true;
        self.ask(request, |result| Message::Sent(conversation_of(result)))
    }

    fn sent(&mut self, loaded: Loaded) -> Task<Message> {
        self.sending = false;
        match loaded {
            Ok((summary, entries)) => {
                self.editor = Content::new();
                self.mini.clear();
                if self.current.as_ref() != Some(&summary.id) {
                    let (without, mode) = (std::mem::take(&mut self.without), self.mode);
                    self.reset_chat(Some(summary.id.clone()));
                    (self.without, self.mode) = (without, mode);
                    self.take_without = false;
                    self.transcript = Transcript::new(summary, entries);
                } else {
                    self.transcript.merge(summary, entries);
                }
                self.after_change(true);
                // The person sent something: the history goes to it, wherever it was scrolled.
                Task::batch([self.follow(), self.load_list(), self.scroll_to_newest()])
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

    fn stop(&mut self) -> Task<Message> {
        let Some(id) = self.current.clone().filter(|_| self.transcript.running()) else { return Task::none() };
        self.ask(Request::CancelAnswer { conversation: id }, |result| Message::Stopped(result.map(|_| ())))
    }

    /// "Allow" or "Deny": only while the approval waits and no decision is on its way.
    fn decide(&mut self, id: String, allow: bool) -> Task<Message> {
        let open = self.approvals.get(&id).is_some_and(|approval| approval.state.is_pending());
        if !open || self.deciding.is_some() {
            return Task::none();
        }
        self.deciding = Some(id.clone());
        self.ask(chat::decide_request(&id, allow), move |result| Message::Decided(id.clone(), approval_of(result)))
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
            return Task::none();
        }
        self.open_settings(None)
    }

    /// The settings in the content area, at `section` when one is named (`--settings <section>`,
    /// an unknown word leaves the first section open) – a fresh [`Host`] with the assistant's
    /// sections and its first load, which goes over [`Backend::settings`] and never over this
    /// window's own requests.
    fn open_settings(&mut self, section: Option<&str>) -> Task<Message> {
        let (mut host, load) = Host::new(self.backend.settings(), settings::SECTIONS.to_vec(), settings::wanted());
        if let Some(id) = section {
            host.open(id);
        }
        self.settings = Some(SettingsPane { host, sections: settings::State::default() });
        load.map(Message::SettingsHost)
    }

    /// The settings at Access, at the bot with this id or at the assistant (`None`), task S.34: what
    /// "Set up access", "Change access", "Access…" and "What the bot may use" open. Open settings
    /// switch to it; closed ones open there, with their first load.
    fn open_access(&mut self, bot: Option<String>) -> Task<Message> {
        self.close_menus();
        let load = match self.settings.is_some() {
            true => Task::none(),
            false => self.open_settings(None),
        };
        if let Some(pane) = self.settings.as_mut() {
            settings::show_access(&mut pane.sections, bot);
            pane.host.open(settings::access::ID);
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
            Outcome::Changed => Task::batch([self.fetch_appearance(), self.load_all()]),
            Outcome::Close => {
                let save = self.save_settings().unwrap_or_else(Task::none);
                self.settings = None;
                save
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
            // Tools (task WZ.6): only the app opens the system's choosers; a path chosen there by
            // the person goes out as a setting ([`Message::FolderPicked`], [`Message::LogoPicked`],
            // [`Message::AllowedFolderPicked`]).
            Outcome::App(AppAction::PickFolder) => {
                Task::perform(pick_folder(tr("Choose a folder for PDFs")), Message::FolderPicked)
            }
            Outcome::App(AppAction::PickLogo(template)) => {
                Task::perform(pick_logo(), move |logo| Message::LogoPicked(template, logo))
            }
            Outcome::App(AppAction::PickAllowedFolder) => {
                Task::perform(pick_folder(tr("Choose a folder for the tools")), Message::AllowedFolderPicked)
            }
        }
    }

    /// Sends a request a chooser of the settings ended in through their host, which loads them
    /// again afterwards: `None` when the chooser was cancelled, `Some(None)` for a path that is
    /// not plain text, which cannot be stored; nothing while the settings are closed meanwhile.
    fn settings_send(&mut self, request: Option<Option<Request>>) -> Task<Message> {
        let Some(request) = request else { return Task::none() };
        let Some(request) = request else {
            self.note(tr("This path cannot be used.").to_string());
            return Task::none();
        };
        let outcome = {
            let Some(pane) = self.settings.as_mut() else { return Task::none() };
            pane.host.act(settings::Action::send(request))
        };
        self.settings_outcome(outcome)
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

    fn toggle_menu(&mut self, menu: Menu) {
        self.menu = if self.menu.as_ref() == Some(&menu) { None } else { Some(menu) };
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

/// The conversation the main window starts on in a demo state; a new chat otherwise.
fn start_conversation(open: Option<Open>) -> Option<String> {
    match open {
        Some(Open::Tools | Open::Menu | Open::Rename | Open::Delete) => Some(demo::TOOLS.into()),
        Some(Open::Approval) => Some(demo::APPROVAL.into()),
        Some(Open::Streaming) => Some(demo::STREAMING.into()),
        Some(Open::Forbidden) => Some(demo::FORBIDDEN.into()),
        Some(Open::Bot) => Some(demo::BOT_CHAT.into()),
        Some(Open::BotAsked) => Some(demo::ASKED.into()),
        Some(Open::BotGone) => Some(demo::GONE.into()),
        Some(Open::BotProposal) => Some(demo::PROPOSAL.into()),
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

/// Asks for a folder with the system's chooser titled `title` (Tools: where new PDFs go, task WZ.6;
/// a folder the tools may reach, task WD.4); `None` when it is cancelled.
async fn pick_folder(title: &'static str) -> Option<PathBuf> {
    let chosen = rfd::AsyncFileDialog::new().set_title(title).pick_folder().await;
    chosen.map(|folder| folder.path().to_path_buf())
}

/// Asks for a template's logo, with the system's chooser offering only the pictures a template
/// takes (`.png`, `.jpg`, `.jpeg`); `None` when it is cancelled.
async fn pick_logo() -> Option<PathBuf> {
    let chosen = rfd::AsyncFileDialog::new()
        .set_title(tr("Choose a logo"))
        .add_filter(tr("Pictures"), &settings::tools::LOGO_EXTENSIONS)
        .pick_file()
        .await;
    chosen.map(|logo| logo.path().to_path_buf())
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

mod bots;
#[cfg(test)]
mod tests;
