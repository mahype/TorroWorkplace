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
//! Following an answer: there is no `subscribe` yet, so the window asks `assistant.get_conversation`
//! with `after` = the last number it has, every [`FOLLOW`] while an answer runs and every
//! [`REFRESH`] otherwise, and merges what comes ([`Transcript::merge`]): the answer the model is
//! still writing comes again and again under the same number and replaces itself.
//!
//! First start: without the client "Assistant" (`clients.list_clients`) a new chat asks the person
//! to choose what the assistant may use, with a button that opens the control centre on its page
//! "Assistant" (`torroworkplace ui --section assistant`, task C.6); "Change access" under a refused
//! tool opens the same page. Without an AI account it says so and opens the accounts page
//! (`--section accounts`).
//!
//! Bots (task B.3, [`crate::bots`]): the sidebar lists them under "Bots" with "New bot"; a click
//! opens a bot's chat – its newest conversation or a new one – in which messages go to the bot
//! (`assistant.send_message { bot }`) and the picker starts at the bot's engine and model. The bot
//! editor (profile, personality, instructions, skills, engine) takes the chat's place and saves with
//! `bots.save_bot`; deleting asks first (`bots.delete_bot`). "Access…" opens the control centre at
//! the bot's rights (`--section bot:<id>`). `--bot <id>` is a chat window with one bot
//! ([`Window::Bot`]), titled "{bot} · {title}"; "Open in new window" of a bot starts it through
//! `assistant.open_window`. Its methods live in `app/bots.rs`.
//!
//! Mini and strip tiles show the newest answer and a one-line composer, nothing else.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use iced::theme::Mode;
use iced::widget::markdown;
use iced::widget::operation;
use iced::widget::text_editor::{self, Binding, Content, KeyPress};
use iced::{Size, Subscription, Task, Theme, keyboard};
use torro_core::AppId;
use torro_core::ai::AiModel;
use torro_core::i18n::{tr, trf};
use torro_gui::design::{self, Follow};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, SidebarMode, Switcher};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets;
use torro_gui::zoom::{self, Zoom};
use torro_ipc::api::{Area, Rights};
use torro_ipc::assistant::{ChatWindow, ConversationSummary, Engine, EngineKind, Entry, EntryContent, Profile};
use torro_ipc::bots::{Bot, BotSummary};
use torro_ipc::{ApprovalInfo, Request, Response};

use crate::backend::{Backend, Refusal};
use crate::bots::{BotDraft, DraftEdit, Proposal};
use crate::chat::{self, Choice, Transcript};
use crate::demo;
use crate::launch::{Launch, Open, Window};

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
};

/// The size a chat window opens at: a column of conversation, not the main window's two parts.
pub const CHAT_SIZE: Size = Size::new(560.0, 760.0);

/// How often the window asks for the conversations, the engines, the client and the open
/// conversation while nothing runs. The core is local and cheap to ask.
const REFRESH: Duration = Duration::from_secs(5);

/// How often the window follows an answer that runs: often enough that the text flows.
const FOLLOW: Duration = Duration::from_millis(500);

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
/// The control centre's page "Assistant" (task C.6): what the assistant may use.
pub const ASSISTANT_PAGE: &str = "assistant";
/// The control centre's page of the accounts, where AI accounts are added.
pub const ACCOUNTS_PAGE: &str = "accounts";

/// The id of the scrolling entries of the open conversation.
pub const HISTORY: &str = "assistant-history";

/// What a test can see of the requests the window sent (tasks do not run in tests).
#[cfg(test)]
pub type Sent = Vec<Request>;

/// An open menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Menu {
    /// The app switcher at the top of the sidebar.
    Switcher,
    /// The engine and model picker of the composer.
    Picker,
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

#[derive(Debug, Clone)]
pub enum Message {
    /// The band above the content was pressed: drag the window (see `shell::titlebar`).
    DragWindow,
    /// The window has this size (at start and after every resize).
    Resized(Size),
    /// Every [`REFRESH`]: ask for everything again.
    Tick,
    /// Every [`FOLLOW`] while an answer runs: ask for the open conversation.
    Follow,
    /// The conversations, pinned first, then the newest.
    Listed(Result<Vec<ConversationSummary>, Refusal>),
    /// The engines the assistant can answer with.
    Engines(Result<Vec<Engine>, Refusal>),
    /// The models of an AI account (by engine id).
    Models(String, Result<Vec<AiModel>, Refusal>),
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
    /// "Access…": the control centre at the bot's rights (`--section bot:<id>`).
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
    /// "Set up access", "Change access": the control centre's page "Assistant" (task C.6), where
    /// the person chooses what the assistant may use.
    SetUp,
    /// "Add an AI account", "Settings": the control centre's accounts.
    OpenSettings,
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
    /// Why the last listing failed.
    pub(crate) list_error: Option<String>,
    /// The engines in the picker's order ([`chat::ordered`]).
    pub(crate) engines: Vec<Engine>,
    pub(crate) engines_loaded: bool,
    /// The models of each AI account, by engine id, as listed (the picker filters them).
    pub(crate) models: HashMap<String, Result<Vec<AiModel>, String>>,
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
    /// The tool lines unfolded, by number.
    pub(crate) expanded: HashSet<u64>,
    /// The answers as Markdown blocks, by number, with the text they were made from.
    pub(crate) answers: HashMap<u64, (String, Vec<markdown::Item>)>,

    /// The composer's text.
    pub(crate) editor: Content,
    /// The one-line composer of mini and strip.
    pub(crate) mini: String,
    pub(crate) choice: Option<Choice>,
    pub(crate) chosen: Chosen,
    /// The areas switched off for the next message.
    pub(crate) without: Vec<Area>,
    /// The areas of the open conversation's last message are taken once it is loaded.
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
    pub(crate) appearance: Appearance,
    pub(crate) system_mode: Mode,
    pub(crate) zoom: Zoom,
    pub(crate) design: Follow,
    #[cfg(test)]
    pub(crate) sent: Sent,
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
            list_error: None,
            engines: Vec::new(),
            engines_loaded: false,
            models: HashMap::new(),
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
            expanded: HashSet::new(),
            answers: HashMap::new(),
            editor: Content::new(),
            mini: String::new(),
            choice: None,
            chosen: Chosen::Default,
            without: Vec::new(),
            take_without: true,
            sending: false,
            menu: match open {
                Some(Open::Picker) => Some(Menu::Picker),
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
            appearance: Appearance::System,
            system_mode: Mode::None,
            launch,
            #[cfg(test)]
            sent: Vec::new(),
            #[cfg(test)]
            sections: Vec::new(),
        };
        let size = Task::batch([
            iced::window::latest().and_then(iced::window::size).map(Message::Resized),
            torro_gui::place_traffic_lights(),
        ]);
        let mut tasks = vec![
            app.load_all(),
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
        let open_chat = app.open_current();
        tasks.push(open_chat);
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
            iced::time::every(REFRESH).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            self.zoom.subscription().map(Message::Zoom),
            self.design.subscription().map(Message::Design),
        ];
        // Only while an answer runs (or a message is on its way): a window at rest asks rarely.
        if self.transcript.running() || self.sending {
            subscriptions.push(iced::time::every(FOLLOW).map(|_| Message::Follow));
        }
        if self.dialog.is_some() || self.menu.is_some() || self.drawer || self.notice.is_some() {
            subscriptions.push(widgets::escape().map(|()| Message::Escape));
        }
        if self.notice.is_some() {
            subscriptions.push(iced::time::every(NOTICE_TICK).map(Message::NoticeTick));
        }
        if !self.chat_only && !self.is_glance() {
            subscriptions.push(iced::event::listen_with(window_key));
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
        match message {
            Message::DragWindow => return torro_gui::drag_window(),
            Message::Resized(size) => self.size = size,
            Message::Tick => {
                let mut tasks = vec![self.load_all(), self.fetch_appearance(), self.zoom.fetch().map(Message::Zoom)];
                tasks.push(self.follow());
                return Task::batch(tasks);
            }
            Message::Follow => return self.follow(),
            Message::Listed(result) => return self.listed(result),
            Message::Engines(result) => return self.take_engines(result),
            Message::Models(engine, result) => {
                self.models.insert(engine, result.map_err(|refusal| refusal.message));
                self.choose_default();
            }
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
            Message::BotAccess(id) => self.open_section(&crate::bots::access_section(&id)),
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
                self.menu = if self.menu.as_ref() == Some(&menu) { None } else { Some(menu) };
                // A conversation's menu from the sidebar row leaves the drawer open under it.
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
            Message::SetUp => self.open_section(ASSISTANT_PAGE),
            Message::OpenSettings => self.open_section(ACCOUNTS_PAGE),
            Message::OpenLink(url) => return open_link(url),
            Message::Retry => {
                self.list_error = None;
                self.chat_error = None;
                return Task::batch([self.load_all(), self.open_current()]);
            }
            Message::ToggleSidebar => return self.toggle_sidebar(),
            Message::UiState(state) => self.sidebar = state.sidebar,
            Message::Escape => {
                if self.dialog.is_some() {
                    self.dialog = None;
                } else if self.menu.is_some() || self.drawer {
                    self.close_menus();
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
    /// here, so the tests see them in [`App::sent`].
    fn ask(
        &mut self,
        request: Request,
        map: impl Fn(Result<Response, Refusal>) -> Message + Send + 'static,
    ) -> Task<Message> {
        #[cfg(test)]
        self.sent.push(request.clone());
        let backend = self.backend.clone();
        Task::perform(async move { backend.ask(request).await }, map)
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
        Task::batch([self.load_list(), engines, client, profile, self.load_bots()])
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
        self.ask(Request::ListConversations, |result| {
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
        // Each AI account's models once; the picker asks again when it opens.
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
        let request = Request::AiModels { account: engine.clone(), all: false };
        self.ask(request, move |result| {
            Message::Models(
                engine.clone(),
                result.and_then(|response| match response {
                    Response::AiModels { models } => Ok(models),
                    other => Err(Refusal::unexpected(&other)),
                }),
            )
        })
    }

    /// The choice before the person picks: the open conversation's engine and model, else the
    /// last one used or the first engine ([`chat::default_choice`]). The person's own pick stays.
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
            self.chosen = Chosen::Conversation;
            return;
        }
        // A new chat with a bot starts at the bot's engine and model (task B.3).
        let bot =
            self.bot.as_deref().filter(|_| self.current.is_none()).and_then(|id| crate::bots::find(&self.bots, id));
        if let Some(bot) = bot.filter(|bot| self.engines.iter().any(|engine| engine.id == bot.engine)) {
            self.choice = Some(Choice { engine: bot.engine.clone(), model: bot.model.clone() });
            self.chosen = Chosen::Default;
            return;
        }
        let models = &self.models;
        let listed = |id: &str| models.get(id).and_then(|models| models.as_ref().ok()).map(Vec::as_slice);
        self.choice = chat::default_choice(&self.conversations, &self.engines, listed);
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
                self.after_change();
                if matches!(self.launch.open, Some(Open::Tools | Open::BotAsked)) && self.expanded.is_empty() {
                    let first = self.transcript.entries.iter().find(|e| matches!(e.content, EntryContent::Tool { .. }));
                    self.expanded.extend(first.map(|entry| entry.seq));
                }
                self.choose_default();
                // The entries are anchored at the bottom: offset 0 is the newest entry, also when
                // the list kept the offset of the conversation shown before.
                let newest = operation::snap_to(HISTORY, operation::RelativeOffset::START);
                Task::batch([self.fetch_approvals(), newest])
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
                if self.transcript.merge(summary, entries) {
                    self.after_change();
                }
                let new: Vec<String> = self
                    .transcript
                    .approvals()
                    .into_iter()
                    .filter(|id| !known.contains(*id))
                    .map(str::to_owned)
                    .collect();
                let tasks: Vec<Task<Message>> = new.into_iter().map(|id| self.fetch_approval(id)).collect();
                Task::batch(tasks)
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
    /// answers, and the areas of its last message once.
    fn after_change(&mut self) {
        if let Some(summary) = self.transcript.summary.clone() {
            match self.conversations.iter_mut().find(|c| c.id == summary.id) {
                Some(listed) => *listed = summary,
                None => self.conversations.insert(0, summary),
            }
        }
        let mut seen = HashSet::new();
        for entry in &self.transcript.entries {
            if let EntryContent::Answer { text, .. } = &entry.content {
                seen.insert(entry.seq);
                let fresh = self.answers.get(&entry.seq).is_none_or(|(had, _)| had != text);
                if fresh {
                    self.answers.insert(entry.seq, (text.clone(), torro_gui::markdown::parse(text)));
                }
            }
        }
        self.answers.retain(|seq, _| seen.contains(seq));
        if self.take_without {
            self.take_without = false;
            let usable = self.switchable();
            self.without = self.transcript.last_without().unwrap_or_default();
            if self.access != Access::Unknown {
                self.without.retain(|area| usable.contains(area));
            }
        }
    }

    fn fetch_approvals(&mut self) -> Task<Message> {
        let ids: Vec<String> = self.transcript.approvals().into_iter().map(str::to_owned).collect();
        let tasks: Vec<Task<Message>> = ids.into_iter().map(|id| self.fetch_approval(id)).collect();
        Task::batch(tasks)
    }

    // -- Actions --------------------------------------------------------------------------------

    fn select(&mut self, id: String) -> Task<Message> {
        self.close_menus();
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
        self.expanded.clear();
        self.answers.clear();
        self.without.clear();
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
        let without = if self.chat_bot().is_some() { Vec::new() } else { self.without.clone() };
        let Some(request) =
            chat::send_request(self.current.as_deref(), self.choice.as_ref(), &self.draft(), &without, self.chat_bot())
        else {
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
                    let without = std::mem::take(&mut self.without);
                    self.reset_chat(Some(summary.id.clone()));
                    self.without = without;
                    self.take_without = false;
                    self.transcript = Transcript::new(summary, entries);
                } else {
                    self.transcript.merge(summary, entries);
                }
                self.after_change();
                Task::batch([self.follow(), self.load_list()])
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

    /// The listed models of `engine` the picker offers; `None` while not listed or failed.
    pub(crate) fn offered_models(&self, engine: &str) -> Option<Result<Vec<AiModel>, &str>> {
        self.models.get(engine).map(|listed| match listed {
            Ok(models) => Ok(chat::tool_models(models)),
            Err(err) => Err(err.as_str()),
        })
    }

    /// Opens the control centre on `section` (`torroworkplace ui --section …`, `bot:<id>` for a
    /// bot's rights); nothing starts in the demo or in tests.
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

/// Opens a web address of an answer in the browser; only `http` and `https`, since the text is the
/// model's.
fn open_link(url: String) -> Task<Message> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Task::none();
    }
    Task::future(async move {
        if let Err(err) = webbrowser::open(&url) {
            eprintln!("cannot open the link in a browser: {err}");
        }
    })
    .discard()
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
