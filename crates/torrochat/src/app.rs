//! The state of a window of `torrochat` and what every click and answer does ([`App::update`]).
//!
//! One program, two kinds of window ([`Window`]):
//!
//! - **The contact list** (class `torro-chat`): the people of the account with their status in
//!   groups ("Favorites", "Online", "Away", "Busy", "Do not disturb", "Offline", each folds), the
//!   group chats behind "People | Groups", a search, the unread counts and the own status at the
//!   bottom with a menu that sets it (`presence.set_own_status`). A double click or Enter opens the
//!   chat window of a person or group: the person's direct chat, created first when there is none
//!   (`chat.create_conversation`). "+" starts a direct chat with someone of the server
//!   (`chat.find_people`). The third tab, "Messages" (also Ctrl+Shift+F), searches the messages
//!   with the same field (`chat.search_messages`); a hit opens its conversation's chat window.
//! - **A chat window** (class `torro-chat-conversation`, `--conversation=<id>`): its methods live in
//!   [`chat`].
//!
//! The windows hold no logic of the core: every change is a command of `chat` or `presence` through
//! [`Backend`], and what they show is what the core answered. A chat window is opened through the core
//! (`apps.open_app` with `chat:<id>`), which starts `torrochat --conversation=<id>` or brings the
//! open window to the front.
//!
//! Live: the contact list asks for people and conversations every [`REFRESH`] (the core answers from
//! its memory and fetches in the background); a chat window asks for new messages every
//! [`chat::POLL`] with `after`, which tells the core to keep the conversation live (long polling).

pub(crate) mod chat;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use iced::theme::Mode;
use iced::widget::operation;
use iced::{Point, Size, Subscription, Task, Theme, keyboard};
use torro_chat::{ChatError, Conversation, OwnStatus, Person, SearchHit, Status, StatusChange};
use torro_core::AppId;
use torro_core::app::CHAT_CONVERSATION_CLASS;
use torro_core::i18n::tr;
use torro_gui::design::{self, Follow};
use torro_gui::layout::{self, SizeClass};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance};
use torro_gui::widgets;
use torro_gui::zoom::{self, Zoom};

pub use self::chat::ChatState;
use crate::backend::Backend;
use crate::launch::{Launch, Open, Window};
use crate::people::{self, Group};

/// The contact list's identity: window class `torro-chat`, binary `torrochat`, state in `torrochat/`.
pub const CONTACTS: AppId = AppId::CHAT;

/// A chat window's identity: its own window class (tiling rules let it float, decision 16 of
/// `docs/chat.md`), the binary and state directory of the chat.
pub const CONVERSATION: AppId = AppId {
    window_class: CHAT_CONVERSATION_CLASS,
    binary: AppId::CHAT.binary,
    dir: AppId::CHAT.dir,
    display_name: AppId::CHAT.display_name,
    // A chat window starts with `--conversation=<id>`; no entry of its own.
    args: &[],
};

/// The contact list's size at start: a narrow tile (`docs/chat.md`, "Kontaktliste": 300 wide).
pub const CONTACTS_SIZE: Size = Size::new(320.0, 760.0);

/// A chat window's size at start (`docs/chat.md`, "Chatfenster": 440 × 680).
pub const CONVERSATION_SIZE: Size = Size::new(440.0, 680.0);

/// How often the windows ask for people and conversations. The core answers from memory and fetches
/// itself when its copy is older than 10 s, so asking is cheap.
const REFRESH: Duration = Duration::from_secs(5);

/// How often the notice at the bottom checks whether its time is up.
const NOTICE_TICK: Duration = Duration::from_millis(500);

/// The search field of the contact list.
pub const SEARCH: &str = "chat-search";

/// The field of the dialog "New chat".
pub const NEW_QUERY: &str = "chat-new-query";

/// The field of the dialog "Set status message".
pub const STATUS_TEXT: &str = "chat-status-text";

/// The tabs of the contact list (decision 17, version B); "Messages" turns the search field into a
/// search in messages (stage 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    People,
    Groups,
    Messages,
}

/// What a row of the contact list stands for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    /// A person (`<account>/<user id>`): their direct chat.
    Person(String),
    /// A conversation (`<account>/<token>`).
    Conversation(String),
    /// A message a search found (`<conversation>/<number>`): its conversation.
    Hit(String),
}

/// The search in messages of the tab "Messages" (`chat.search_messages`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessageSearch {
    /// What the hits are for, or are being asked for; empty while the field holds too little.
    pub query: String,
    pub hits: Vec<SearchHit>,
    /// The ask for `query` is out.
    pub searching: bool,
    /// Why the search failed, in words for people.
    pub error: Option<String>,
}

/// When a status message ends by itself (`docs/chat.md`, "Fuß").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearAfter {
    HalfHour,
    Hour,
    Today,
    Week,
}

impl ClearAfter {
    pub const ALL: [ClearAfter; 4] = [ClearAfter::HalfHour, ClearAfter::Hour, ClearAfter::Today, ClearAfter::Week];

    pub fn label(self) -> &'static str {
        match self {
            ClearAfter::HalfHour => tr("30 minutes"),
            ClearAfter::Hour => tr("1 hour"),
            ClearAfter::Today => tr("Today"),
            ClearAfter::Week => tr("This week"),
        }
    }

    /// The moment the message ends, from `now`: in 30 minutes or an hour, at the end of the local day,
    /// at the end of the local week (Sunday night).
    pub fn at(self, now: DateTime<Utc>) -> DateTime<Utc> {
        use chrono::{Datelike, Duration as Span, Local};

        let local = now.with_timezone(&Local);
        let end_of = |days: i64| {
            let day = local.date_naive() + Span::days(days);
            let midnight = day.and_hms_opt(23, 59, 59).unwrap_or_default();
            torro_gui::datetime::to_utc(midnight)
        };
        match self {
            ClearAfter::HalfHour => now + Span::minutes(30),
            ClearAfter::Hour => now + Span::hours(1),
            ClearAfter::Today => end_of(0),
            ClearAfter::Week => end_of(i64::from(6 - local.weekday().num_days_from_monday())),
        }
    }
}

/// An open menu.
#[derive(Debug, Clone, PartialEq)]
pub enum Menu {
    /// The own status, over the footer.
    Status,
    /// A row's context menu at the pointer.
    Row { target: Target, at: Point },
    /// The "…" of a chat window's head.
    Head,
    /// A message's menu: at the pointer after a right click (`at`), else under the message's "…".
    /// `confirm`: it asks whether to delete the message.
    Message { id: String, at: Option<Point>, confirm: bool },
}

/// An open dialog of the contact list.
#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    /// "New chat": people of the server by name.
    NewChat { query: String, found: Vec<Person>, searching: bool, error: Option<String> },
    /// "Set status message".
    StatusMessage { text: String, clear: Option<ClearAfter>, saving: bool },
}

/// The short notice at the bottom ("The chat could not be opened").
#[derive(Debug, Clone)]
pub struct Notice {
    pub text: String,
    pub shown: Instant,
}

#[derive(Debug, Clone)]
pub enum Message {
    // -- The window
    CoreReady,
    Resized(Size),
    DragWindow,
    Raise,
    Focus(bool),
    Appearance(Result<Appearance, String>),
    SystemMode(Mode),
    Zoom(zoom::Message),
    Design(design::Message),
    Escape,
    Tick,
    NoticeTick(Instant),
    CloseMenu,
    CloseDialog,

    // -- People and conversations
    People(Result<Vec<Person>, ChatError>),
    Conversations(Result<Vec<Conversation>, ChatError>),
    Own(Result<OwnStatus, ChatError>),
    OpenWorkplace,

    // -- The contact list
    Search(String),
    SearchSubmit,
    FocusSearch,
    /// Ctrl+Shift+F: the tab "Messages" with the field focused.
    SearchMessages,
    /// What `chat.search_messages` found for the query.
    Hits(String, Result<Vec<SearchHit>, ChatError>),
    Tab(Tab),
    Fold(Group),
    Hover(Option<Target>),
    Select(Target),
    Move(i32),
    OpenCursor,
    Open(Target),
    Created(Result<Conversation, ChatError>),
    Opened(Result<(), String>),
    RowMenu(Target, Point),
    Remove(String),
    Removed(Result<(), ChatError>),
    ToggleStatusMenu,
    SetStatus(Status),
    ClearStatusText,
    StatusSet(Result<OwnStatus, ChatError>),
    StatusDialog,
    StatusText(String),
    StatusClear(Option<ClearAfter>),
    SaveStatusText,
    NewChat,
    NewQuery(String),
    Found(String, Result<Vec<Person>, ChatError>),
    StartWith(Person),

    // -- A chat window
    Chat(chat::Msg),
}

pub struct App {
    pub(crate) backend: Backend,
    /// The suite behind the window: appearance, zoom, design (none with `--demo`).
    pub(crate) suite: Source,
    pub(crate) launch: Launch,
    pub(crate) size: Size,
    pub(crate) appearance: Appearance,
    pub(crate) system_mode: Mode,
    pub(crate) zoom: Zoom,
    pub(crate) design: Follow,
    /// The window has the keyboard: a chat window marks read only then.
    pub(crate) focused: bool,

    pub(crate) people: Vec<Person>,
    pub(crate) conversations: Vec<Conversation>,
    /// The first answers came (or failed): before them the list shows "loading".
    pub(crate) people_loaded: bool,
    pub(crate) conversations_loaded: bool,
    /// Why the people could not be listed, while nothing was ever listed.
    pub(crate) error: Option<ChatError>,
    /// People or conversations are being asked for: a tick asks again only after the answers.
    pub(crate) refreshing: u8,
    pub(crate) own: Option<OwnStatus>,

    // The contact list
    pub(crate) tab: Tab,
    /// The field above the list: a filter of people and groups, or the query of "Messages".
    pub(crate) search: String,
    pub(crate) found: MessageSearch,
    /// Groups folded or unfolded by a click; the others as they start ([`App::folded`]).
    pub(crate) folds: BTreeMap<Group, bool>,
    pub(crate) cursor: Option<Target>,
    pub(crate) hover: Option<Target>,
    pub(crate) menu: Option<Menu>,
    pub(crate) dialog: Option<Dialog>,
    /// A direct chat is being created for this person.
    pub(crate) creating: Option<String>,
    pub(crate) notice: Option<Notice>,

    /// The chat window's state; `None` in the contact list.
    pub(crate) chat: Option<ChatState>,
}

impl App {
    pub fn new(backend: Backend, suite: Source, launch: Launch) -> (App, Task<Message>) {
        let chat = match &launch.window {
            Window::Conversation(id) => Some(ChatState::new(id.clone(), launch.open)),
            Window::Contacts => None,
        };
        let open = launch.open;
        let size = if chat.is_some() { CONVERSATION_SIZE } else { CONTACTS_SIZE };
        let mut app = App {
            backend,
            zoom: Zoom::for_source(&suite),
            design: Follow::for_source(&suite),
            suite,
            size,
            appearance: Appearance::System,
            system_mode: Mode::None,
            focused: true,
            people: Vec::new(),
            conversations: Vec::new(),
            people_loaded: false,
            conversations_loaded: false,
            error: None,
            refreshing: 0,
            own: None,
            tab: match open {
                Some(Open::Groups) => Tab::Groups,
                Some(Open::MessageSearch) => Tab::Messages,
                _ => Tab::People,
            },
            search: match open {
                Some(Open::Search) => "e".into(),
                Some(Open::MessageSearch) => "review".into(),
                _ => String::new(),
            },
            found: MessageSearch::default(),
            folds: if open == Some(Open::Search) { BTreeMap::from([(Group::Offline, true)]) } else { BTreeMap::new() },
            cursor: None,
            hover: None,
            menu: match open {
                Some(Open::Status) => Some(Menu::Status),
                _ => None,
            },
            dialog: match open {
                Some(Open::New) => {
                    Some(Dialog::NewChat { query: "be".into(), found: Vec::new(), searching: true, error: None })
                }
                Some(Open::StatusMessage) => Some(Dialog::StatusMessage {
                    text: "In a meeting".into(),
                    clear: Some(ClearAfter::Hour),
                    saving: false,
                }),
                _ => None,
            },
            creating: None,
            notice: None,
            chat,
            launch,
        };
        // `--open message-search` starts with the query typed.
        let search = app.search_messages();
        let suite = app.suite.clone();
        let mut tasks = vec![
            search,
            // The first load waits for a core that is still starting (issue #206).
            Task::perform(async move { suite.ready().await }, |_| Message::CoreReady),
            app.fetch_appearance(),
            app.zoom.fetch().map(Message::Zoom),
            app.design.fetch().map(Message::Design),
            iced::system::theme().map(Message::SystemMode),
            iced::window::latest().and_then(iced::window::size).map(Message::Resized),
            torro_gui::place_traffic_lights(),
        ];
        if app.chat.is_some() {
            tasks.push(operation::focus(chat::EDITOR));
        }
        if let Some(Dialog::NewChat { query, .. }) = &app.dialog {
            let query = query.clone();
            tasks.push(app.find(query));
        }
        (app, Task::batch(tasks))
    }

    /// The window's title: "Contacts" for the list, the conversation's name for a chat window.
    /// Tiling rules and window lists tell chat windows apart by it.
    pub fn title(&self) -> String {
        match &self.chat {
            Some(chat) => self.chat_name(chat),
            None => tr("Contacts").to_owned(),
        }
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

    /// The window's size class.
    pub fn class(&self) -> SizeClass {
        layout::of(self.size)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::system::theme_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            self.zoom.subscription().map(Message::Zoom),
            self.design.subscription().map(Message::Design),
            // A second start on this window brings it to the front (issue #279, decision 2 of the chat).
            torro_gui::instance::subscription().map(|()| Message::Raise),
            iced::event::listen_with(window_event),
            iced::time::every(REFRESH).map(|_| Message::Tick),
        ];
        if self.menu.is_some() || self.dialog.is_some() || self.chat.as_ref().is_some_and(ChatState::escapes) {
            subscriptions.push(widgets::escape().map(|()| Message::Escape));
        }
        if self.notice.is_some() {
            subscriptions.push(iced::time::every(NOTICE_TICK).map(Message::NoticeTick));
        }
        match &self.chat {
            Some(chat) => subscriptions.push(chat.subscription().map(Message::Chat)),
            None if self.dialog.is_none() && self.menu.is_none() => {
                subscriptions.push(iced::event::listen_with(contact_key));
            }
            None => {}
        }
        Subscription::batch(subscriptions)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CoreReady => return self.load(),
            Message::Resized(size) => self.size = size,
            Message::DragWindow => return torro_gui::drag_window(),
            Message::Raise => return torro_gui::instance::bring_to_front(),
            Message::Focus(focused) => {
                self.focused = focused;
                if focused {
                    return self.chat_update(chat::Msg::Seen);
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
                return task;
            }
            Message::Escape => return self.escape(),
            Message::Tick => return self.load(),
            Message::NoticeTick(now) => {
                if self.notice.as_ref().is_some_and(|n| now.saturating_duration_since(n.shown) >= widgets::TOAST_FOR) {
                    self.notice = None;
                }
            }
            Message::CloseMenu => self.menu = None,
            Message::CloseDialog => self.dialog = None,

            Message::People(result) => return self.people_came(result),
            Message::Conversations(result) => {
                self.refreshing = self.refreshing.saturating_sub(1);
                self.conversations_loaded = true;
                if let Ok(conversations) = result {
                    self.conversations = conversations;
                    return self.chat_update(chat::Msg::Listed);
                }
            }
            Message::Own(result) => match result {
                Ok(own) => self.own = Some(own),
                // A server without the user status: no footer, the list stays.
                Err(_) => self.own = None,
            },
            Message::OpenWorkplace => {
                let suite = self.suite.clone();
                let binary = AppId::WORKPLACE.binary;
                return Task::perform(async move { suite.open_app(binary).await }, Message::Opened);
            }

            Message::Search(text) => {
                self.search = text;
                self.cursor = self.rows().first().cloned();
                return self.search_messages();
            }
            Message::SearchSubmit => return self.open_cursor_or_first(),
            Message::FocusSearch => return operation::focus(SEARCH),
            Message::SearchMessages => {
                self.tab = Tab::Messages;
                self.cursor = None;
                return Task::batch([self.search_messages(), operation::focus(SEARCH)]);
            }
            Message::Hits(asked, result) => {
                // Only the answer to what the field holds now; an older one is dropped.
                if asked != self.found.query {
                    return Task::none();
                }
                self.found.searching = false;
                match result {
                    Ok(hits) => {
                        self.found.hits = hits;
                        self.found.error = None;
                    }
                    Err(err) => {
                        self.found.hits.clear();
                        self.found.error = Some(chat_failed(&err));
                    }
                }
                if self.tab == Tab::Messages {
                    self.cursor = self.rows().first().cloned();
                }
            }
            Message::Tab(tab) => {
                self.tab = tab;
                self.cursor = None;
                return self.search_messages();
            }
            Message::Fold(group) => {
                let folded = self.folded(group);
                self.folds.insert(group, !folded);
            }
            Message::Hover(target) => self.hover = target,
            Message::Select(target) => self.cursor = Some(target),
            Message::Move(step) => self.move_cursor(step),
            Message::OpenCursor => return self.open_cursor_or_first(),
            Message::Open(target) => {
                self.menu = None;
                self.cursor = Some(target.clone());
                return self.open(target);
            }
            Message::Created(result) => {
                self.creating = None;
                match result {
                    Ok(conversation) => {
                        let id = conversation.id.clone();
                        if !self.conversations.iter().any(|known| known.id == id) {
                            self.conversations.push(conversation);
                        }
                        return self.open_window(id);
                    }
                    Err(err) => self.notify(chat_failed(&err)),
                }
            }
            Message::Opened(result) => {
                if result.is_err() {
                    self.notify(tr("The chat could not be opened.").to_owned());
                }
            }
            Message::RowMenu(target, at) => {
                self.cursor = Some(target.clone());
                self.menu = Some(Menu::Row { target, at });
            }
            Message::Remove(person) => {
                self.menu = None;
                let Some(account) = self.people.iter().find(|p| p.id == person).map(|p| p.account.clone()) else {
                    return Task::none();
                };
                self.people.retain(|p| p.id != person);
                return Task::perform(self.backend.clone().remove_person(account, person), Message::Removed);
            }
            Message::Removed(result) => {
                if result.is_err() {
                    self.notify(tr("The person could not be removed from the list.").to_owned());
                }
                return self.load();
            }
            Message::ToggleStatusMenu => {
                self.menu = if self.menu == Some(Menu::Status) { None } else { Some(Menu::Status) };
            }
            Message::SetStatus(status) => {
                self.menu = None;
                // "Online" only lasts with the heartbeat (decision 10): the core would refuse it.
                if status == Status::Online && self.own.as_ref().is_some_and(|own| !own.heartbeat) {
                    self.notify(tr("Turn on “Show others when I am online” first.").to_owned());
                    return Task::none();
                }
                return self.set_status(StatusChange { status: Some(status), ..StatusChange::default() });
            }
            Message::ClearStatusText => {
                self.menu = None;
                return self.set_status(StatusChange { message: Some(String::new()), ..StatusChange::default() });
            }
            Message::StatusSet(result) => match result {
                Ok(own) => {
                    self.own = Some(own);
                    self.dialog = None;
                }
                Err(err) => {
                    if let Some(Dialog::StatusMessage { saving, .. }) = &mut self.dialog {
                        *saving = false;
                    }
                    self.notify(status_failed(&err));
                }
            },
            Message::StatusDialog => {
                self.menu = None;
                let text = self.own.as_ref().and_then(|own| own.message.clone()).unwrap_or_default();
                self.dialog = Some(Dialog::StatusMessage { text, clear: None, saving: false });
                return operation::focus(STATUS_TEXT);
            }
            Message::StatusText(new) => {
                if let Some(Dialog::StatusMessage { text, .. }) = &mut self.dialog {
                    // One line of at most 80 characters, as the server takes it.
                    *text = new.replace(['\n', '\r'], " ").chars().take(torro_ipc::chat::MAX_STATUS_MESSAGE).collect();
                }
            }
            Message::StatusClear(choice) => {
                if let Some(Dialog::StatusMessage { clear, .. }) = &mut self.dialog {
                    *clear = if *clear == choice { None } else { choice };
                }
            }
            Message::SaveStatusText => {
                let Some(Dialog::StatusMessage { text, clear, saving }) = &mut self.dialog else {
                    return Task::none();
                };
                if *saving {
                    return Task::none();
                }
                *saving = true;
                let message = text.trim().to_owned();
                let clear_at = clear.filter(|_| !message.is_empty()).map(|clear| clear.at(Utc::now()));
                return self.set_status(StatusChange { message: Some(message), clear_at, ..StatusChange::default() });
            }
            Message::NewChat => {
                self.menu = None;
                self.dialog =
                    Some(Dialog::NewChat { query: String::new(), found: Vec::new(), searching: false, error: None });
                return operation::focus(NEW_QUERY);
            }
            Message::NewQuery(text) => {
                let Some(Dialog::NewChat { query, found, searching, error }) = &mut self.dialog else {
                    return Task::none();
                };
                *query = text.clone();
                *error = None;
                if text.trim().chars().count() < torro_ipc::chat::MIN_QUERY {
                    found.clear();
                    *searching = false;
                    return Task::none();
                }
                *searching = true;
                return self.find(text);
            }
            Message::Found(asked, result) => {
                let Some(Dialog::NewChat { query, found, searching, error }) = &mut self.dialog else {
                    return Task::none();
                };
                // Only the answer to what the field holds now; an older one is dropped.
                if *query != asked {
                    return Task::none();
                }
                *searching = false;
                match result {
                    Ok(people) => *found = people,
                    Err(err) => {
                        found.clear();
                        *error = Some(chat_failed(&err));
                    }
                }
            }
            Message::StartWith(person) => {
                self.dialog = None;
                // `chat.create_conversation` gives the existing direct chat when there is one.
                return self.open(Target::Person(person.id));
            }

            Message::Chat(message) => return self.chat_update(message),
        }
        Task::none()
    }

    // -- Asking the core ------------------------------------------------------------------------

    /// People and conversations, unless an earlier ask is still out; the own status once an account
    /// is known.
    fn load(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        if self.refreshing == 0 {
            self.refreshing = 2;
            let backend = self.backend.clone();
            tasks.push(Task::perform(backend.people(), Message::People));
            let backend = self.backend.clone();
            tasks.push(Task::perform(backend.conversations(), Message::Conversations));
        }
        tasks.push(self.chat_update(chat::Msg::Refresh));
        Task::batch(tasks)
    }

    fn people_came(&mut self, result: Result<Vec<Person>, ChatError>) -> Task<Message> {
        self.refreshing = self.refreshing.saturating_sub(1);
        self.people_loaded = true;
        match result {
            Ok(people) => {
                self.people = people;
                self.error = None;
            }
            // Keep what was listed; the error shows only while nothing ever came.
            Err(err) => self.error = Some(err),
        }
        // The own status of the first account; asked once, then whenever it was set.
        match people::accounts(&self.people, &self.conversations).into_iter().next() {
            Some(account) if self.own.is_none() && self.chat.is_none() => {
                Task::perform(self.backend.clone().own_status(account), Message::Own)
            }
            _ => Task::none(),
        }
    }

    fn fetch_appearance(&self) -> Task<Message> {
        let suite = self.suite.clone();
        Task::perform(async move { theme::stored_appearance(&suite).await }, Message::Appearance)
    }

    fn set_status(&mut self, change: StatusChange) -> Task<Message> {
        let Some(account) = self.own.as_ref().map(|own| own.account.clone()) else { return Task::none() };
        Task::perform(self.backend.clone().set_own_status(account, change), Message::StatusSet)
    }

    /// `chat.find_people` for the dialog "New chat", in the account of the own status (or the first).
    fn find(&self, query: String) -> Task<Message> {
        let account = self
            .own
            .as_ref()
            .map(|own| own.account.clone())
            .or_else(|| people::accounts(&self.people, &self.conversations).into_iter().next())
            .or_else(|| self.backend.is_demo().then(|| torro_chat::demo::ACCOUNT.to_owned()));
        let Some(account) = account else { return Task::none() };
        let asked = query.clone();
        Task::perform(self.backend.clone().find_chat_people(account, query.trim().to_owned()), move |result| {
            Message::Found(asked.clone(), result)
        })
    }

    /// `chat.search_messages` for the field, while the tab "Messages" shows and the field holds at
    /// least two characters; with less the hits go. The same query is not asked twice in a row.
    fn search_messages(&mut self) -> Task<Message> {
        if self.tab != Tab::Messages || self.chat.is_some() {
            return Task::none();
        }
        let query = self.search.trim().to_owned();
        if query.chars().count() < torro_ipc::chat::MIN_QUERY {
            self.found = MessageSearch::default();
            return Task::none();
        }
        if query == self.found.query && self.found.error.is_none() {
            return Task::none();
        }
        self.found.query = query.clone();
        self.found.searching = true;
        self.found.error = None;
        let asked = query.clone();
        Task::perform(self.backend.clone().search_messages(query), move |result| Message::Hits(asked.clone(), result))
    }

    /// Opens the chat window of `target`: a conversation at once; a person's direct chat, created
    /// first when there is none yet; the conversation of a message a search found.
    fn open(&mut self, target: Target) -> Task<Message> {
        match target {
            Target::Conversation(id) => self.open_window(id),
            Target::Hit(id) => match torro_chat::split_message(&id) {
                Some((conversation, _)) => self.open_window(conversation.to_owned()),
                None => Task::none(),
            },
            Target::Person(id) => {
                let known = self.people.iter().find(|person| person.id == id);
                if let Some(conversation) = known.and_then(|person| people::direct_chat(&self.conversations, person)) {
                    return self.open_window(conversation.id.clone());
                }
                if self.creating.is_some() {
                    return Task::none();
                }
                let Some((account, _)) = torro_chat::split_person(&id) else { return Task::none() };
                let account = account.to_owned();
                self.creating = Some(id.clone());
                Task::perform(self.backend.clone().create_conversation(account, id), Message::Created)
            }
        }
    }

    /// Asks the core to open the chat window of `conversation` (or bring it to the front).
    fn open_window(&self, conversation: String) -> Task<Message> {
        let appearance = self.launch.appearance.map(|appearance| match appearance {
            Appearance::Light => "light",
            Appearance::Dark => "dark",
            Appearance::System => "system",
        });
        Task::perform(self.backend.clone().open_chat(conversation, appearance), Message::Opened)
    }

    fn notify(&mut self, text: String) {
        self.notice = Some(Notice { text, shown: Instant::now() });
    }

    /// Escape closes the topmost thing: a menu, a dialog, a chat window's reply or suggestions.
    fn escape(&mut self) -> Task<Message> {
        if self.menu.take().is_some() || self.dialog.take().is_some() {
            return Task::none();
        }
        self.chat_update(chat::Msg::Escape)
    }

    // -- The contact list -----------------------------------------------------------------------

    /// Whether `group` is folded: as the person clicked it, else "Offline" once it holds more than
    /// [`people::FOLD_OFFLINE_FROM`] people.
    pub(crate) fn folded(&self, group: Group) -> bool {
        match self.folds.get(&group) {
            Some(folded) => *folded,
            None => {
                group == Group::Offline
                    && self.people.iter().filter(|person| Group::of(person) == Group::Offline).count()
                        > people::FOLD_OFFLINE_FROM
            }
        }
    }

    /// The rows the list shows now, top to bottom: the people of unfolded groups or the group chats,
    /// as the search keeps them.
    pub(crate) fn rows(&self) -> Vec<Target> {
        match self.tab {
            Tab::People => people::groups(&self.people, &self.conversations, &self.search)
                .into_iter()
                .filter(|(group, _)| !self.folded(*group) || !self.search.trim().is_empty())
                .flat_map(|(_, members)| members.into_iter().map(|person| Target::Person(person.id.clone())))
                .collect(),
            Tab::Groups => people::group_chats(&self.conversations, &self.search)
                .into_iter()
                .map(|conversation| Target::Conversation(conversation.id.clone()))
                .collect(),
            Tab::Messages => self.found.hits.iter().map(|hit| Target::Hit(hit.id.clone())).collect(),
        }
    }

    fn move_cursor(&mut self, step: i32) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let at = self.cursor.as_ref().and_then(|cursor| rows.iter().position(|row| row == cursor));
        let next = match at {
            Some(at) => (at as i64 + i64::from(step)).clamp(0, rows.len() as i64 - 1) as usize,
            None if step < 0 => rows.len() - 1,
            None => 0,
        };
        self.cursor = rows.get(next).cloned();
    }

    /// Enter: the row under the cursor, else the first row the search kept.
    fn open_cursor_or_first(&mut self) -> Task<Message> {
        let rows = self.rows();
        let target = self.cursor.clone().filter(|cursor| rows.contains(cursor)).or_else(|| rows.first().cloned());
        match target {
            Some(target) => self.update(Message::Open(target)),
            None => Task::none(),
        }
    }

    /// The name of the conversation of a chat window: from the list, else the person's, else
    /// "Conversation".
    pub(crate) fn chat_name(&self, chat: &ChatState) -> String {
        match self.conversations.iter().find(|c| c.id == chat.id) {
            Some(conversation) => conversation.name.clone(),
            None => tr("Conversation").to_owned(),
        }
    }
}

/// What the person reads when a command of the chat failed, in words without blame (torro-design,
/// "Texte"): what happened and what to do.
pub(crate) fn chat_failed(err: &ChatError) -> String {
    match err {
        ChatError::Offline => tr("The server cannot be reached. Try again when you are online.").to_owned(),
        ChatError::Auth => tr("The sign-in was refused. Sign in again in Torro Workplace.").to_owned(),
        ChatError::NotConnected => tr("This account has no chat on its server.").to_owned(),
        ChatError::NotFound => tr("This conversation is no longer available.").to_owned(),
        ChatError::Busy => tr("Too many calls. Try again later.").to_owned(),
        ChatError::Pending => tr("The command waits for approval.").to_owned(),
        ChatError::Unsupported(text) | ChatError::Forbidden(text) | ChatError::Other(text) => text.clone(),
    }
}

/// What the person reads when the own status could not be set.
fn status_failed(err: &ChatError) -> String {
    match err {
        ChatError::Unsupported(text) | ChatError::Forbidden(text) => text.clone(),
        other => chat_failed(other),
    }
}

/// The window's focus, for marking read only while the person looks.
fn window_event(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Window(iced::window::Event::Focused) => Some(Message::Focus(true)),
        iced::Event::Window(iced::window::Event::Unfocused) => Some(Message::Focus(false)),
        _ => None,
    }
}

/// The contact list's keys: ↑ ↓ move, Enter opens, Ctrl+F (⌘F) goes to the search, Ctrl+Shift+F
/// (⇧⌘F) to the search in messages. Arrows and Enter only when no field took them.
fn contact_key(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }) = event else {
        return None;
    };
    if modifiers.command() && !modifiers.alt() && key.to_latin(physical_key) == Some('f') {
        return Some(if modifiers.shift() { Message::SearchMessages } else { Message::FocusSearch });
    }
    use keyboard::key::Named;
    match key.as_ref() {
        keyboard::Key::Named(Named::ArrowDown) => Some(Message::Move(1)),
        keyboard::Key::Named(Named::ArrowUp) => Some(Message::Move(-1)),
        keyboard::Key::Named(Named::Enter) if status == iced::event::Status::Ignored => Some(Message::OpenCursor),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
