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
//!   (`chat.find_people`). While the field filters "People", the list ends with "Find on server…"
//!   (also Enter when no one on the list matches): the server's people by name
//!   (`presence.find_people`), each with "Add to list" (`presence.add_person`); people added by hand
//!   carry a mark and "Remove from list" in their menu (`presence.remove_person`). The third tab,
//!   "Messages" (also Ctrl+Shift+F), searches the messages with the same field
//!   (`chat.search_messages`); a hit opens its conversation's chat window.
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
pub(crate) mod files;
pub(crate) mod ring;
pub(crate) mod viewer;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use iced::theme::Mode;
use iced::widget::operation;
use iced::{Point, Size, Subscription, Task, Theme, keyboard};
use torro_chat::{ChatError, Conversation, OwnStatus, Person, SearchHit, Status, StatusChange};
use torro_core::AppId;
use torro_core::app::{CHAT_CONVERSATION_CLASS, CHAT_RING_CLASS, CHAT_VIEWER_CLASS};
use torro_core::i18n::tr;
use torro_gui::design::{self, Follow};
use torro_gui::layout::{self, SizeClass};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance};
use torro_gui::widgets;
use torro_gui::zoom::{self, Zoom};

pub use self::chat::ChatState;
pub use self::ring::RingState;
pub use self::viewer::ViewerState;
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

/// The window "Picture large" (stage 4, decision 24): its own class, so tiling rules let it float.
pub const VIEWER: AppId = AppId {
    window_class: CHAT_VIEWER_CLASS,
    binary: AppId::CHAT.binary,
    dir: AppId::CHAT.dir,
    display_name: AppId::CHAT.display_name,
    // Started with `--viewer=<message>/<file>`; no entry of its own.
    args: &[],
};

/// The contact list's size at start: a narrow, tall tile (`docs/chat.md`, "Kontaktliste": 320 × 760).
pub const CONTACTS_SIZE: Size = Size::new(320.0, 760.0);

/// The width from which the contact list shows its list instead of the glance (#652). Below it the
/// "People | Groups | Messages" tabs no longer fit; the height needed is `STRIP_MAX_HEIGHT`.
pub const CONTACTS_LIST_MIN_WIDTH: f32 = 280.0;

/// A chat window's size at start (`docs/chat.md`, "Chatfenster": 440 × 680).
pub const CONVERSATION_SIZE: Size = Size::new(440.0, 680.0);

/// The window "Picture large" at start (draft B of `docs/prototypes/chat/`: 900 × 620).
pub const VIEWER_SIZE: Size = Size::new(900.0, 620.0);

/// The ringing window (stage 6b, decision 24): its own class, so tiling rules let it float.
pub const RING: AppId = AppId {
    window_class: CHAT_RING_CLASS,
    binary: AppId::CHAT.binary,
    dir: AppId::CHAT.dir,
    display_name: AppId::CHAT.display_name,
    // Started by the core with `--ring=<conversation>`; no entry of its own.
    args: &[],
};

/// The ringing window's size (`docs/chat.md`, "Eingehender Anruf": 340 × 360).
pub const RING_SIZE: Size = Size::new(340.0, 360.0);

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

/// The search on the server of the tab "People" (`presence.find_people`): people of the server by
/// name, to put on the person's own list with "Add to list" (`presence.add_person`). It belongs to
/// what the field held when it was asked; typing something else drops it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerSearch {
    /// What was asked for (trimmed); empty while nothing was asked.
    pub query: String,
    pub found: Vec<Person>,
    /// The ask for `query` is out.
    pub searching: bool,
    /// Why the search failed, in words for people.
    pub error: Option<String>,
    /// The people being added (`<account>/<user id>`): their button turns busy.
    pub adding: Vec<String>,
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
    /// The menu of an item of the shared media (the `index` of the tab's list), at the pointer.
    Shared { index: usize, at: Point },
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
    /// "Find on server…": `presence.find_people` for what the field holds.
    FindOnServer,
    /// What `presence.find_people` found for the query.
    ServerFound(String, Result<Vec<Person>, ChatError>),
    /// "Add to list": `presence.add_person`.
    AddPerson(Person),
    /// The core's answer to "Add to list".
    Added(Person, Result<(), ChatError>),

    // -- A chat window
    Chat(chat::Msg),

    // -- The window "Picture large"
    Viewer(viewer::ViewerMsg),

    // -- The ringing window
    Ring(ring::RingMsg),
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
    /// "Find on server…" of the tab "People".
    pub(crate) server: ServerSearch,
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
    /// The state of the window "Picture large"; `None` in the others.
    pub(crate) viewer: Option<ViewerState>,
    /// The state of the ringing window; `None` in the others.
    pub(crate) ring: Option<RingState>,
}

impl App {
    pub fn new(backend: Backend, suite: Source, launch: Launch) -> (App, Task<Message>) {
        let chat = match &launch.window {
            Window::Conversation(id) => Some(ChatState::new(id.clone(), launch.open)),
            Window::Contacts | Window::Viewer { .. } | Window::Ring(_) | Window::Player(_) => None,
        };
        let viewer = match &launch.window {
            Window::Viewer { message, file } => ViewerState::new(message, file),
            Window::Contacts | Window::Conversation(_) | Window::Ring(_) | Window::Player(_) => None,
        };
        let ring = match &launch.window {
            Window::Ring(conversation) => Some(RingState::new(conversation.clone())),
            Window::Contacts | Window::Conversation(_) | Window::Viewer { .. } | Window::Player(_) => None,
        };
        let open = launch.open;
        let size = match &launch.window {
            Window::Contacts => CONTACTS_SIZE,
            Window::Conversation(_) => CONVERSATION_SIZE,
            // The player is no Iced window (`main` starts it before); as "Picture large", never shown.
            Window::Viewer { .. } | Window::Player(_) => VIEWER_SIZE,
            Window::Ring(_) => RING_SIZE,
        };
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
                Some(Open::Search) => "er".into(),
                Some(Open::FindOnServer) => "hanna".into(),
                Some(Open::ServerFound) => "an".into(),
                Some(Open::MessageSearch) => "review".into(),
                _ => String::new(),
            },
            found: MessageSearch::default(),
            server: ServerSearch::default(),
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
            viewer,
            ring,
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
            // The width of the shared media as the person dragged it; the demo keeps none.
            if !app.backend.is_demo() {
                tasks.push(
                    torro_gui::ui_state::load_splits_task(AppId::CHAT)
                        .map(|widths| files::wrap(files::FilesMsg::Stored(widths))),
                );
            }
        }
        tasks.push(app.viewer_start());
        tasks.push(app.ring_start());
        if let Some(Dialog::NewChat { query, .. }) = &app.dialog {
            let query = query.clone();
            tasks.push(app.find(query));
        }
        if open == Some(Open::ServerFound) {
            tasks.push(app.update(Message::FindOnServer));
        }
        (app, Task::batch(tasks))
    }

    /// The window's title: "Contacts" for the list, the conversation's name for a chat window.
    /// Tiling rules and window lists tell chat windows apart by it.
    pub fn title(&self) -> String {
        if self.ring.is_some() {
            return tr("Incoming call").to_owned();
        }
        if let Some(viewer) = &self.viewer {
            return match viewer.current().filter(|item| !item.name.is_empty()) {
                Some(item) => item.name.clone(),
                None => tr("Picture").to_owned(),
            };
        }
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

    /// The class the window draws in. The ringing window is narrower than a mini tile by design
    /// (340 × 360), so only a low one is a glance. The contact list keeps its list from
    /// [`CONTACTS_LIST_MIN_WIDTH`] on and tall enough (#652); a chat window, the viewer and every
    /// smaller window use [`Self::class`].
    pub fn view_class(&self) -> SizeClass {
        let Size { width, height } = self.size;
        if self.ring.is_some() {
            layout::of(Size::new(torro_gui::theme::size::panel().max(width), height))
        } else if self.viewer.is_none() && self.chat.is_none() && width >= CONTACTS_LIST_MIN_WIDTH {
            layout::of(Size::new(layout::MINI_MAX_WIDTH.max(width), height))
        } else {
            self.class()
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            torro_gui::theme::system_mode_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            self.zoom.subscription().map(Message::Zoom),
            self.design.subscription().map(Message::Design),
            // A second start on this window brings it to the front (issue #279, decision 2 of the chat).
            torro_gui::instance::subscription().map(|()| Message::Raise),
            iced::event::listen_with(window_event),
            iced::time::every(REFRESH).map(|_| Message::Tick),
        ];
        if self.menu.is_some()
            || self.dialog.is_some()
            || self.viewer.is_some()
            || self.ring.is_some()
            || self.chat.as_ref().is_some_and(ChatState::escapes)
        {
            subscriptions.push(widgets::escape().map(|()| Message::Escape));
        }
        if self.notice.is_some() {
            subscriptions.push(iced::time::every(NOTICE_TICK).map(Message::NoticeTick));
        }
        if let Some(ring) = &self.ring {
            subscriptions.push(ring.subscription().map(Message::Ring));
        }
        if let Some(viewer) = &self.viewer {
            subscriptions.push(viewer.subscription().map(Message::Viewer));
        }
        match &self.chat {
            Some(chat) => subscriptions.push(chat.subscription().map(Message::Chat)),
            None if self.viewer.is_some() || self.ring.is_some() => {}
            None if self.dialog.is_none() && self.menu.is_none() => {
                subscriptions.push(iced::event::listen_with(contact_key));
            }
            None => {}
        }
        Subscription::batch(subscriptions)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // The first ask for the appearance went out before the core answered; ask again.
            Message::CoreReady => return Task::batch([self.load(), self.fetch_appearance()]),
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
            // Every tick asks for the appearance too, so a change in the control centre reaches an open window.
            Message::Tick => return Task::batch([self.load(), self.fetch_appearance()]),
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
                // The server's hits belong to the query they were asked for.
                if self.server.query != self.search.trim() {
                    self.server = ServerSearch::default();
                }
                return self.search_messages();
            }
            // Without a match on the list Enter asks the server.
            Message::SearchSubmit if self.tab == Tab::People && self.rows().is_empty() => {
                return self.update(Message::FindOnServer);
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
                // "Online" is the same consent as "Show others when I am online" (decision 10): without the
                // heartbeat the choice switches it on first, then sets the status.
                if status == Status::Online && self.own.as_ref().is_some_and(|own| !own.heartbeat) {
                    let Some(account) = self.own.as_ref().map(|own| own.account.clone()) else { return Task::none() };
                    return Task::perform(self.backend.clone().go_online(account), Message::StatusSet);
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
            Message::FindOnServer => return self.find_on_server(),
            Message::ServerFound(asked, result) => {
                // Only the answer to what was asked last; an older one is dropped.
                if asked != self.server.query {
                    return Task::none();
                }
                self.server.searching = false;
                match result {
                    Ok(found) => {
                        self.server.found = found;
                        self.server.error = None;
                    }
                    Err(err) => {
                        self.server.found.clear();
                        self.server.error = Some(chat_failed(&err));
                    }
                }
            }
            Message::AddPerson(person) => {
                if self.server.adding.contains(&person.id) || self.on_list(&person.id) {
                    return Task::none();
                }
                self.server.adding.push(person.id.clone());
                let (account, id) = (person.account.clone(), person.id.clone());
                return Task::perform(self.backend.clone().add_person(account, id), move |result| {
                    Message::Added(person.clone(), result)
                });
            }
            Message::Added(person, result) => {
                self.server.adding.retain(|id| *id != person.id);
                if result.is_err() {
                    self.notify(tr("The person could not be added to the list.").to_owned());
                    return Task::none();
                }
                // On the list at once, marked as added; the next list of the core confirms it.
                if !self.on_list(&person.id) {
                    self.people.push(Person { added: true, ..person });
                }
                return self.load();
            }

            Message::Chat(message) => return self.chat_update(message),
            Message::Viewer(message) => return self.viewer_update(message),
            Message::Ring(message) => return self.ring_update(message),
        }
        Task::none()
    }

    // -- Asking the core ------------------------------------------------------------------------

    /// People and conversations, unless an earlier ask is still out; the own status once an account
    /// is known.
    fn load(&mut self) -> Task<Message> {
        // The window "Picture large" needs neither people nor conversations.
        if self.viewer.is_some() {
            return Task::none();
        }
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

    /// Asks the core for the stored appearance (light, dark, system); an error keeps the one we have.
    fn fetch_appearance(&self) -> Task<Message> {
        let suite = self.suite.clone();
        Task::perform(async move { theme::stored_appearance(&suite).await }, Message::Appearance)
    }

    fn set_status(&mut self, change: StatusChange) -> Task<Message> {
        let Some(account) = self.own.as_ref().map(|own| own.account.clone()) else { return Task::none() };
        Task::perform(self.backend.clone().set_own_status(account, change), Message::StatusSet)
    }

    /// The account a search for people asks: the one of the own status, else the first known.
    fn search_account(&self) -> Option<String> {
        self.own
            .as_ref()
            .map(|own| own.account.clone())
            .or_else(|| people::accounts(&self.people, &self.conversations).into_iter().next())
            .or_else(|| self.backend.is_demo().then(|| torro_chat::demo::ACCOUNT.to_owned()))
    }

    /// `chat.find_people` for the dialog "New chat", in the account of the own status (or the first).
    fn find(&self, query: String) -> Task<Message> {
        let Some(account) = self.search_account() else { return Task::none() };
        let asked = query.clone();
        Task::perform(self.backend.clone().find_chat_people(account, query.trim().to_owned()), move |result| {
            Message::Found(asked.clone(), result)
        })
    }

    /// `presence.find_people` for what the field holds, while "Find on server…" shows
    /// ([`App::finds_on_server`]). The same query is not asked twice in a row unless it failed.
    fn find_on_server(&mut self) -> Task<Message> {
        if !self.finds_on_server() {
            return Task::none();
        }
        let query = self.search.trim().to_owned();
        if query == self.server.query && self.server.error.is_none() {
            return Task::none();
        }
        let Some(account) = self.search_account() else { return Task::none() };
        let adding = std::mem::take(&mut self.server.adding);
        self.server = ServerSearch { query: query.clone(), searching: true, adding, ..ServerSearch::default() };
        let asked = query.clone();
        Task::perform(self.backend.clone().find_people(account, query), move |result| {
            Message::ServerFound(asked.clone(), result)
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
        if self.viewer.is_some() || self.ring.is_some() {
            return iced::window::latest().and_then(iced::window::close);
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

    /// Whether the list offers "Find on server…": the contact list on "People", the field holding at
    /// least two characters.
    pub(crate) fn finds_on_server(&self) -> bool {
        self.chat.is_none()
            && self.tab == Tab::People
            && self.search.trim().chars().count() >= torro_ipc::chat::MIN_QUERY
    }

    /// Whether the server was asked for what the field holds now: its hits show instead of "Find on
    /// server…".
    pub(crate) fn server_asked(&self) -> bool {
        !self.server.query.is_empty() && self.server.query == self.search.trim()
    }

    /// Whether the person `id` stands on the list already.
    pub(crate) fn on_list(&self, id: &str) -> bool {
        self.people.iter().any(|person| person.id == id)
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
