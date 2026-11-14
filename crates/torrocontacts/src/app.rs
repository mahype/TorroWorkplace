//! The state of the contacts window and what every click and answer does ([`App::update`]).
//!
//! The window holds no logic of the core: every change is a command of `contacts` through
//! [`Backend`], and what it shows is what the core answered. It loads everything once
//! ([`backend::load`]) and again after every change and every minute ([`REFRESH`]), not while a contact
//! is being edited. Pictures come in batches of [`PHOTO_BATCH`] and are kept per contact and version.
//!
//! What the person sees ([`View`]): all contacts, the favourites, the ones used last, the birthdays of
//! the coming year, a book or a group of a book. The list groups by group, town or book
//! ([`GroupBy`]); the choice and the folded heads are kept in `ui.json`. The search asks the core
//! ([`backend::search`]) [`SEARCH_PAUSE`] after the last key.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate};
use iced::theme::Mode;
use iced::widget::image::Handle;
use iced::widget::{operation, text_editor};
use iced::{Point, Size, Subscription, Task, Theme, keyboard};
use torro_contacts::{
    AddressBook, Birthday, Contact, ContactSummary, ContactsError, ContactsSource, Keep, Moved, PhotoSize,
};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::design::{self, Follow};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, SidebarMode, Switcher};
use torro_gui::source::Source;
use torro_gui::split::{self, Split, SplitEvent};
use torro_gui::theme::{self, Appearance};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets;
use torro_gui::zoom::{self, Zoom};
use torro_ipc::contacts::{ContactLocation, Geo, MAX_PHOTO_BYTES};

use crate::backend::{self, Backend, Loaded};
use crate::contact::{self, AddressMap};
use crate::edit::{self, Draft, List};
use crate::launch::{Launch, Open};
use crate::list::{GroupBy, Key};

/// The window's identity: class `torro-contacts`, binary `torrocontacts`, state in `torrocontacts/`.
pub const APP: AppId = AppId::CONTACTS;

/// How often the window loads again on its own.
pub const REFRESH: Duration = Duration::from_secs(60);

/// How long the search waits after the last key before it asks the core.
pub const SEARCH_PAUSE: Duration = Duration::from_millis(150);

/// How many small pictures one ask fetches.
pub const PHOTO_BATCH: usize = 20;

/// How often the notice at the bottom checks whether its time is up.
const NOTICE_TICK: Duration = Duration::from_millis(500);

/// The longest edge of a picture the window sends (`contacts.set_photo`): larger ones are scaled down.
pub const PHOTO_EDGE: u32 = 512;

/// The search field over the list, and the one of the mini tile.
pub const SEARCH: &str = "contacts-search";

/// The key of the grouping in `ui.json`.
pub const GROUP_BY_KEY: &str = "group_by";

/// The keys of the widths the person drags, in `ui.json`.
pub const SIDEBAR_WIDTH: &str = "sidebar";
pub const PANEL_WIDTH: &str = "panel";

/// What the list shows (the sidebar's rows).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum View {
    #[default]
    All,
    Favorites,
    /// The contacts used last, the latest first, without heads.
    Recent,
    /// The birthdays of the coming year by month.
    Birthdays,
    /// One address book (its id).
    Book(String),
    /// One group of a book (book id, name).
    Group(String, String),
}

/// An open menu.
#[derive(Debug, Clone, PartialEq)]
pub enum Menu {
    /// "Group by" in the header.
    GroupBy,
    /// The navigation of a narrow window, top left.
    Nav,
    /// The "…" of the contact's head.
    More,
    /// "Move to", the books the contact may go to.
    Move,
    /// The kind of a value in the form.
    Kind(List, usize),
    /// "+ Add field" in the form.
    AddField,
    /// The menu of a row at the pointer.
    Row(String, Point),
}

/// An open dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    /// "Delete contact?"
    Delete,
}

/// The contact in the side panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    /// Its id; `None` for a contact being written.
    pub id: Option<String>,
    /// The whole contact once fetched.
    pub contact: Option<Contact>,
    /// The form while it is being edited or written.
    pub draft: Option<Draft>,
    /// A new contact goes into this book (and this group).
    pub new_in: Option<(String, Option<String>)>,
    /// What the group input holds.
    pub group_text: String,
    /// The groups of the contact's book as `contacts.list_groups` names them (book id, names).
    pub known: Option<(String, Vec<String>)>,
    /// A change is out: buttons that change the contact wait.
    pub busy: bool,
    /// Why the last change failed.
    pub error: Option<String>,
    /// The map under each address of the contact (issue #355), by its place in `addresses`.
    pub maps: Vec<AddressMap>,
    /// The contact and version (`id|etag`) `maps` belong to: a new version asks again.
    pub maps_of: Option<String>,
}

impl Detail {
    fn of(id: String) -> Detail {
        Detail {
            id: Some(id),
            contact: None,
            draft: None,
            new_in: None,
            group_text: String::new(),
            known: None,
            busy: false,
            error: None,
            maps: Vec::new(),
            maps_of: None,
        }
    }
}

/// The short notice at the bottom ("Copied").
#[derive(Debug, Clone)]
pub struct Notice {
    pub text: String,
    pub shown: Instant,
}

/// A key the list answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// `/`, Ctrl+F (⌘F): to the search.
    Search,
    Up,
    Down,
    Enter,
    /// Ctrl+E (⌘E): edit the open contact.
    Edit,
    /// `[`: the sidebar.
    Sidebar,
    /// `M`: mail the contact under the cursor (as the row's "Mail").
    Mail,
    /// `C`: call the contact under the cursor (as the row's "Call").
    Call,
}

#[derive(Debug, Clone)]
pub enum Message {
    // -- The window
    CoreReady,
    Resized(Size),
    DragWindow,
    Raise,
    Appearance(Result<Appearance, String>),
    SystemMode(Mode),
    Zoom(zoom::Message),
    Design(design::Message),
    Escape,
    Tick,
    NoticeTick(Instant),
    Key(Press),
    UiState(UiState),
    StoredGroupBy(Option<String>),
    StoredCollapsed(GroupBy, Vec<String>),
    StoredWidths(HashMap<String, f32>),
    ToggleSidebar,
    CloseMenu,
    SidebarWidth(SplitEvent),
    PanelWidth(SplitEvent),
    Switcher(Switcher),
    OpenWorkplace,
    Opened(Result<(), String>),
    Retry,

    // -- Data
    /// A load came back; the number says which, so an older one never overwrites a newer.
    Loaded(u64, Result<Loaded, ContactsError>),
    Photos(Vec<(String, String, Option<Vec<u8>>)>),
    LargePhoto(String, String, Option<Vec<u8>>),

    // -- The list
    Show(View),
    ToggleMenu(Menu),
    GroupBy(GroupBy),
    Fold(Key),
    Search(String),
    SearchDue(u64),
    Found(String, Result<Vec<String>, ContactsError>),
    Hover(Option<String>),
    Open(String),
    RowMenu(String, Point),
    QuickMail(String),
    QuickCall(String),
    QuickCopy(String),

    // -- The contact
    Fetched(String, Result<Contact, ContactsError>),
    /// The groups of a book, for the suggestions of the open contact (`contacts.list_groups`).
    Groups(String, Option<Vec<String>>),
    /// "Retry" after the open contact could not be fetched.
    Refetch,
    Close,
    OpenUri(String),
    CopyText(String),
    ToggleFavorite,
    Favorited(String, bool, Result<(), ContactsError>),
    MoveTo(String),
    Moved(String, Result<Moved, ContactsError>),
    AskDelete,
    Delete,
    Deleted(String, Result<(), ContactsError>),
    CloseDialog,
    GroupText(String),
    AddGroup(String),
    RemoveGroup(String),
    /// The answer to a change of the contact with this id (group, picture).
    Changed(String, Result<Contact, ContactsError>),
    Resolve(Keep),
    Resolved(String, Result<Vec<Contact>, ContactsError>),
    /// `contacts.mark_used` answered; nothing to show either way.
    Used,
    /// The maps of the version `id|etag` of the open contact (issue #355): what the settings say.
    MapSettings(String, Option<backend::Shown>),
    /// Where its address number `usize` is (`contacts.locate_contact`), or the core's error code.
    Located(String, usize, Result<ContactLocation, String>),
    /// The picture of the map of its address number `usize`, `None` when it cannot be had.
    MapPicture(String, usize, Option<Handle>),
    /// "Open in Maps" of the address number `usize` of the open contact.
    OpenInMaps(usize),
    /// `contacts.open_in_maps` answered.
    OpenedInMaps(Result<(), String>),
    /// A line of a switch under an address: Settings › General of the control centre.
    OpenMapSettings,

    // -- Editing
    Edit,
    NewContact(Option<String>, Option<String>),
    Draft(edit::Msg),
    /// A change of the notes in the form.
    NoteEdit(text_editor::Action),
    ToggleKind(List, usize),
    ToggleAddField,
    Cancel,
    Done,
    Saved(String, Result<Contact, ContactsError>),
    Refetched(Result<Contact, ContactsError>),
    Created(Option<String>, Result<Contact, ContactsError>),
    ChoosePhoto,
    /// A picture chosen or dropped for the contact with this id.
    PhotoChosen(String, Option<Result<Vec<u8>, String>>),
    RemovePhoto,
    FileDropped(PathBuf),
}

pub struct App {
    pub(crate) backend: Backend,
    /// The suite behind the window: appearance, zoom, design, opening other apps (none with `--demo`).
    pub(crate) suite: Source,
    pub(crate) launch: Launch,
    pub(crate) size: Size,
    pub(crate) appearance: Appearance,
    pub(crate) system_mode: Mode,
    pub(crate) zoom: Zoom,
    pub(crate) design: Follow,
    pub(crate) today: NaiveDate,

    pub(crate) sidebar: SidebarMode,
    pub(crate) drawer: bool,
    pub(crate) side: Split,
    pub(crate) panel_split: Split,

    pub(crate) books: Vec<AddressBook>,
    pub(crate) contacts: Vec<ContactSummary>,
    pub(crate) birthdays: Vec<Birthday>,
    pub(crate) recent: Vec<String>,
    /// The first load came (or failed and was shown).
    pub(crate) loaded: bool,
    /// A load is out: the timer asks again only after its answer.
    pub(crate) loading: bool,
    /// Why the first load failed, in words for people; a later failure keeps what was there.
    pub(crate) error: Option<String>,
    /// The number of the last load asked for ([`Message::Loaded`]).
    load_asked: u64,
    /// `--open` was applied once after the first load.
    applied: bool,
    /// `--open edit`: the first contact fetched opens in the form.
    edit_on_fetch: bool,

    pub(crate) view: View,
    pub(crate) group_by: GroupBy,
    pub(crate) collapsed: HashMap<GroupBy, BTreeSet<String>>,
    /// The months folded in "Birthdays"; not kept between starts.
    pub(crate) collapsed_months: BTreeSet<String>,
    pub(crate) search: String,
    search_asked: u64,
    /// The ids the core found for `search` (the text they belong to); `None` while it asks or nothing
    /// is typed.
    pub(crate) found: Option<(String, HashSet<String>)>,
    pub(crate) cursor: Option<String>,
    pub(crate) hover: Option<String>,
    pub(crate) menu: Option<Menu>,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) detail: Option<Detail>,
    /// The notes of the form, which take several lines; [`Draft::note`] follows them.
    pub(crate) note: text_editor::Content,
    pub(crate) notice: Option<Notice>,

    /// Small pictures per contact id: their version and the picture (`None`: not to be had, asked
    /// again only for another version).
    pub(crate) photos: HashMap<String, (String, Option<Handle>)>,
    photos_busy: bool,
    /// The large picture of the open contact: id, version, picture.
    pub(crate) large: Option<(String, String, Handle)>,
}

/// The message of a failed load or change, for people.
pub fn failed(err: &ContactsError) -> String {
    match err {
        ContactsError::Offline => tr("The server cannot be reached. Changes wait until it can.").to_owned(),
        ContactsError::Auth => tr("The server refused the sign-in. Check the account in Torro Workplace.").to_owned(),
        ContactsError::ReadOnly => tr("This address book is read-only.").to_owned(),
        ContactsError::TooLarge => tr("The server refuses a contact this large.").to_owned(),
        ContactsError::NotFound => tr("The contact is gone.").to_owned(),
        ContactsError::Conflict { .. } => tr("This contact changed meanwhile. Check and save again.").to_owned(),
        ContactsError::Unsupported(text) | ContactsError::Other(text) => text.clone(),
    }
}

/// Why the first load failed, for people: as [`failed`], without the changes that wait.
pub fn load_failed(err: &ContactsError) -> String {
    match err {
        ContactsError::Offline => tr("The server cannot be reached.").to_owned(),
        other => failed(other),
    }
}

impl App {
    pub fn new(backend: Backend, suite: Source, launch: Launch) -> (App, Task<Message>) {
        let open = launch.open;
        let app = App {
            backend,
            zoom: Zoom::for_source(&suite),
            design: Follow::for_source(&suite),
            suite,
            size: torro_gui::window(APP).size,
            appearance: Appearance::System,
            system_mode: Mode::None,
            today: Local::now().date_naive(),
            sidebar: SidebarMode::default(),
            drawer: false,
            side: Split::default(),
            panel_split: Split::default(),
            books: Vec::new(),
            contacts: Vec::new(),
            birthdays: Vec::new(),
            recent: Vec::new(),
            loaded: false,
            loading: false,
            error: None,
            applied: false,
            load_asked: 0,
            edit_on_fetch: open == Some(Open::Edit),
            view: View::All,
            group_by: match open {
                Some(Open::ByCity) => GroupBy::City,
                Some(Open::ByBook) => GroupBy::Book,
                _ => GroupBy::Group,
            },
            collapsed: HashMap::new(),
            collapsed_months: BTreeSet::new(),
            search: String::new(),
            search_asked: 0,
            found: None,
            cursor: None,
            hover: None,
            menu: None,
            dialog: None,
            detail: None,
            note: text_editor::Content::new(),
            notice: None,
            photos: HashMap::new(),
            photos_busy: false,
            large: None,
            launch,
        };
        let suite = app.suite.clone();
        let mut tasks = vec![
            // The first load waits for a core that is still starting (issue #206).
            Task::perform(async move { suite.ready().await }, |_| Message::CoreReady),
            app.fetch_appearance(),
            app.zoom.fetch().map(Message::Zoom),
            app.design.fetch().map(Message::Design),
            iced::system::theme().map(Message::SystemMode),
            iced::window::latest().and_then(iced::window::size).map(Message::Resized),
            torro_gui::place_traffic_lights(),
        ];
        // The demo stores nothing and reads nothing of the person's: its pictures stay the same.
        if !app.backend.is_demo() {
            tasks.push(ui_state::load_task(APP).map(Message::UiState));
            tasks.push(ui_state::load_choice_task(APP, GROUP_BY_KEY.into()).map(Message::StoredGroupBy));
            tasks.push(ui_state::load_splits_task(APP).map(Message::StoredWidths));
            for by in GroupBy::ALL {
                tasks.push(
                    ui_state::load_names_task(APP, by.collapsed_key())
                        .map(move |names| Message::StoredCollapsed(by, names)),
                );
            }
        }
        (app, Task::batch(tasks))
    }

    /// The window's title.
    pub fn title(&self) -> String {
        match &self.detail {
            Some(Detail { contact: Some(contact), .. }) => trf("Contacts · {name}", &[("name", &contact.shown_name())]),
            _ => tr("Contacts").to_owned(),
        }
    }

    /// The Iced theme: the appearance of `--appearance`, else the stored one, with "System" following
    /// the desktop.
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

    /// The room under the top row, in layout units.
    pub fn content_size(&self) -> Size {
        shell::below_top_row(self.size, self.zoom.factor())
    }

    /// Whether a contact is being edited or written: the timer does not load then.
    pub fn editing(&self) -> bool {
        self.detail.as_ref().is_some_and(|detail| detail.draft.is_some())
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::system::theme_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            self.zoom.subscription().map(Message::Zoom),
            self.design.subscription().map(Message::Design),
            // A second start brings this window to the front (issue #279).
            torro_gui::instance::subscription().map(|()| Message::Raise),
            iced::time::every(REFRESH).map(|_| Message::Tick),
        ];
        let glance = matches!(self.class(), SizeClass::Mini | SizeClass::Strip);
        if !glance {
            if self.menu.is_some() || self.dialog.is_some() || self.detail.is_some() || self.drawer {
                subscriptions.push(widgets::escape().map(|()| Message::Escape));
            }
            if self.dialog.is_none() {
                subscriptions.push(iced::event::listen_with(key_press));
            }
            // A picture dropped on the window while a contact is edited becomes its picture.
            if self.editing() {
                subscriptions.push(iced::event::listen_with(file_drop));
            }
        }
        if self.notice.is_some() {
            subscriptions.push(iced::time::every(NOTICE_TICK).map(Message::NoticeTick));
        }
        if self.side.dragging {
            subscriptions.push(split::release().map(|()| Message::SidebarWidth(SplitEvent::Release)));
        }
        if self.panel_split.dragging {
            subscriptions.push(split::release().map(|()| Message::PanelWidth(SplitEvent::Release)));
        }
        Subscription::batch(subscriptions)
    }

    fn fetch_appearance(&self) -> Task<Message> {
        let suite = self.suite.clone();
        Task::perform(async move { theme::stored_appearance(&suite).await }, Message::Appearance)
    }

    /// Loads everything again ([`backend::load`]).
    pub fn load(&mut self) -> Task<Message> {
        self.loading = true;
        self.load_asked += 1;
        let asked = self.load_asked;
        Task::perform(backend::load(self.backend.clone()), move |result| Message::Loaded(asked, result))
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice { text: text.into(), shown: Instant::now() });
    }

    /// The contact of the list with `id`.
    pub fn summary(&self, id: &str) -> Option<&ContactSummary> {
        self.contacts.iter().find(|contact| contact.id == id)
    }

    /// The book with `id`.
    pub fn book(&self, id: &str) -> Option<&AddressBook> {
        self.books.iter().find(|book| book.id == id)
    }

    /// The book a new contact goes into: the one shown, when it may be written, else the first that may.
    pub fn new_book(&self) -> Option<String> {
        let shown = match &self.view {
            View::Book(id) | View::Group(id, _) => Some(id.as_str()),
            _ => None,
        };
        shown
            .and_then(|id| self.book(id))
            .filter(|book| book.writable)
            .or_else(|| self.books.iter().find(|book| book.writable))
            .map(|book| book.id.clone())
    }

    /// The groups of the book `id`, by name.
    pub fn groups_of(&self, id: &str) -> Vec<String> {
        // One name per group without case ("Work" and "work" are one), the first spelling met.
        let mut names: Vec<String> = Vec::new();
        for contact in self.contacts.iter().filter(|c| c.book == id) {
            for group in contact.groups.iter().map(|g| g.trim()).filter(|g| !g.is_empty()) {
                if !names.iter().any(|known| same_group(known, group)) {
                    names.push(group.to_owned());
                }
            }
        }
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// The book a new contact of the group `name` goes into: the one shown when it holds the group, else
    /// the first writable book that does, else [`App::new_book`].
    pub fn book_of_group(&self, name: &str) -> Option<String> {
        let holds = |book: &AddressBook| {
            book.writable
                && self.contacts.iter().any(|c| c.book == book.id && c.groups.iter().any(|g| same_group(g, name)))
        };
        let shown = match &self.view {
            View::Book(id) | View::Group(id, _) => self.book(id).filter(|b| holds(b)),
            _ => None,
        };
        shown.or_else(|| self.books.iter().find(|b| holds(b))).map(|b| b.id.clone()).or_else(|| self.new_book())
    }

    /// The contacts of the view, as far as the search lets them through, in no particular order.
    pub fn shown(&self) -> Vec<&ContactSummary> {
        // Only the answer to what the field holds now; until it comes, the names that hold the text.
        let found = self.found.as_ref().filter(|(text, _)| text.as_str() == self.search.trim()).map(|(_, ids)| ids);
        let searching = !self.search.trim().is_empty();
        let passes = |contact: &&ContactSummary| match (searching, found) {
            (false, _) => true,
            (true, Some(ids)) => ids.contains(&contact.id),
            // The answer is not there yet: the names that start with what is typed.
            (true, None) => contact.display_name.to_lowercase().contains(&self.search.trim().to_lowercase()),
        };
        match &self.view {
            View::All | View::Birthdays => self.contacts.iter().filter(passes).collect(),
            View::Favorites => self.contacts.iter().filter(|c| c.favorite).filter(passes).collect(),
            View::Recent => self.recent.iter().filter_map(|id| self.summary(id)).filter(passes).collect(),
            View::Book(id) => self.contacts.iter().filter(|c| &c.book == id).filter(passes).collect(),
            View::Group(book, name) => self
                .contacts
                .iter()
                .filter(|c| &c.book == book && c.groups.iter().any(|g| same_group(g, name)))
                .filter(passes)
                .collect(),
        }
    }

    /// The birthdays the view "Birthdays" lists, as far as the search lets them through.
    pub fn shown_birthdays(&self) -> Vec<&Birthday> {
        let ids: HashSet<&str> = self.shown().into_iter().map(|c| c.id.as_str()).collect();
        self.birthdays.iter().filter(|b| ids.contains(b.contact.id.as_str())).collect()
    }

    /// The birthdays within the next 14 days: the count of the sidebar.
    pub fn soon(&self) -> usize {
        self.birthdays
            .iter()
            .filter(|b| (b.next - self.today).num_days() <= i64::from(torro_ipc::contacts::BIRTHDAY_DAYS))
            .count()
    }

    /// The ids of the rows in the order the list shows them, folded heads left out: what ↑ and ↓ walk.
    pub fn rows(&self) -> Vec<String> {
        let rows: Vec<String> = match &self.view {
            View::Recent => self.shown().into_iter().map(|c| c.id.clone()).collect(),
            View::Birthdays => {
                let birthdays = self.shown_birthdays();
                crate::list::birthday_heads(&birthdays)
                    .into_iter()
                    .filter(|(key, _, _)| !self.folded(key))
                    .flat_map(|(_, _, members)| members.into_iter().map(|b| b.contact.id.clone()))
                    .collect()
            }
            _ => {
                let shown = self.shown();
                crate::list::heads(&shown, self.group_by, &self.books)
                    .into_iter()
                    .filter(|head| !self.folded(&head.key))
                    .flat_map(|head| head.members.into_iter().map(|c| c.id.clone()))
                    .collect()
            }
        };
        // A contact under two groups stands in the list twice; the cursor walks it once, so ↑ and ↓
        // reach every contact.
        let mut seen = HashSet::new();
        rows.into_iter().filter(|id| seen.insert(id.clone())).collect()
    }

    /// Whether the head `key` is folded in the present grouping.
    pub fn folded(&self, key: &Key) -> bool {
        if matches!(key, Key::Month(..)) {
            return self.collapsed_months.contains(&key.word());
        }
        self.collapsed.get(&self.group_by).is_some_and(|words| words.contains(&key.word()))
    }

    /// Handles `message`; then the maps of the open contact follow it ([`App::start_maps`]): a contact
    /// opened, fetched or changed asks for them, whichever message brought it.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        Task::batch([task, self.start_maps()])
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CoreReady => return self.load(),
            Message::Resized(size) => self.size = size,
            Message::DragWindow => return torro_gui::drag_window(),
            Message::Raise => return torro_gui::instance::bring_to_front(),
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
                if self.zoom.factor() != before {
                    // The window keeps its pixels, so its logical size changes with the zoom.
                    self.size = zoom::rescale(self.size, before, self.zoom.factor());
                }
                return task;
            }
            Message::Escape => return self.escape(),
            Message::Tick => {
                self.today = Local::now().date_naive();
                if !self.loading && !self.editing() {
                    return Task::batch([self.load(), self.fetch_appearance()]);
                }
            }
            Message::NoticeTick(now) => {
                if self.notice.as_ref().is_some_and(|n| now.saturating_duration_since(n.shown) >= widgets::TOAST_FOR) {
                    self.notice = None;
                }
            }
            Message::Key(press) => return self.key(press),
            Message::UiState(state) => self.sidebar = state.sidebar,
            Message::StoredGroupBy(word) => {
                if let Some(by) = word.as_deref().and_then(GroupBy::from_word) {
                    self.group_by = by;
                }
            }
            Message::StoredCollapsed(by, names) => {
                self.collapsed.entry(by).or_insert_with(|| names.into_iter().collect());
            }
            Message::StoredWidths(widths) => {
                self.side.restore(widths.get(SIDEBAR_WIDTH).copied());
                self.panel_split.restore(widths.get(PANEL_WIDTH).copied());
            }
            Message::ToggleSidebar => {
                if self.class() == SizeClass::Wide {
                    self.sidebar = self.sidebar.toggled();
                    if !self.backend.is_demo() {
                        return ui_state::save_task(APP, UiState { sidebar: self.sidebar });
                    }
                } else {
                    self.drawer = !self.drawer;
                }
            }
            Message::CloseMenu => {
                self.menu = None;
                self.drawer = false;
            }
            Message::SidebarWidth(event) => {
                let limits = shell::sidebar_limits(self.content_size().width, split::min_pane());
                if self.side.update(event, limits) && !self.backend.is_demo() {
                    return ui_state::save_split_task(APP, SIDEBAR_WIDTH.into(), self.side.chosen);
                }
            }
            Message::PanelWidth(event) => {
                let limits = torro_gui::panel::beside_limits(self.list_room());
                if self.panel_split.update(event, limits) && !self.backend.is_demo() {
                    return ui_state::save_split_task(APP, PANEL_WIDTH.into(), self.panel_split.chosen);
                }
            }
            Message::Switcher(Switcher::New) => return self.update(Message::NewContact(None, None)),
            Message::Switcher(Switcher::Search) => return operation::focus(SEARCH),
            Message::OpenWorkplace => {
                let suite = self.suite.clone();
                return Task::perform(async move { suite.open_app(AppId::WORKPLACE.binary).await }, Message::Opened);
            }
            Message::Opened(result) => {
                if let Err(err) = result {
                    self.notify(err);
                }
            }
            Message::Retry => {
                self.error = None;
                self.loaded = false;
                return self.load();
            }

            // An answer to an older load than the last one asked for is dropped: the newer one comes.
            Message::Loaded(asked, _) if asked < self.load_asked => {}
            Message::Loaded(_, result) => return self.loaded(result),
            Message::Photos(batch) => {
                self.photos_busy = false;
                for (id, version, bytes) in batch {
                    // A picture the contact no longer has (a load came meanwhile) is not kept.
                    let current =
                        self.summary(&id).and_then(|c| c.photo.as_ref()).is_some_and(|p| p.version == version);
                    if current {
                        let picture = bytes.as_deref().and_then(picture);
                        self.photos.insert(id, (version, picture));
                    }
                }
                return self.fetch_photos();
            }
            Message::LargePhoto(id, version, bytes) => {
                if let Some(handle) = bytes.as_deref().and_then(picture) {
                    self.large = Some((id, version, handle));
                }
            }

            Message::Show(view) => {
                self.view = view;
                self.menu = None;
                self.drawer = false;
                self.cursor = None;
            }
            Message::ToggleMenu(menu) => {
                self.menu = if self.menu.as_ref() == Some(&menu) { None } else { Some(menu) };
            }
            Message::GroupBy(by) => {
                self.group_by = by;
                self.menu = None;
                if !self.backend.is_demo() {
                    return ui_state::save_choice_task(APP, GROUP_BY_KEY.into(), by.word().into());
                }
            }
            Message::Fold(key) => {
                let word = key.word();
                if matches!(key, Key::Month(..)) {
                    if !self.collapsed_months.remove(&word) {
                        self.collapsed_months.insert(word);
                    }
                    return Task::none();
                }
                let by = self.group_by;
                let words = self.collapsed.entry(by).or_default();
                if !words.remove(&word) {
                    words.insert(word);
                }
                if !self.backend.is_demo() {
                    let names: Vec<String> = words.iter().cloned().collect();
                    return ui_state::save_names_task(APP, by.collapsed_key(), names);
                }
            }
            Message::Search(text) => {
                self.search = text;
                self.search_asked += 1;
                if self.search.trim().is_empty() {
                    self.found = None;
                    return Task::none();
                }
                let asked = self.search_asked;
                return Task::perform(
                    async move {
                        tokio::time::sleep(SEARCH_PAUSE).await;
                        asked
                    },
                    Message::SearchDue,
                );
            }
            Message::SearchDue(asked) => {
                if asked != self.search_asked || self.search.trim().is_empty() {
                    return Task::none();
                }
                let text = self.search.trim().to_owned();
                let backend = self.backend.clone();
                let query = text.clone();
                return Task::perform(backend::search(backend, query), move |result| {
                    Message::Found(text.clone(), result)
                });
            }
            Message::Found(text, result) => {
                if text != self.search.trim() {
                    return Task::none();
                }
                match result {
                    Ok(ids) => self.found = Some((text, ids.into_iter().collect())),
                    // The names that hold the text stand in for the core's answer.
                    Err(err) => {
                        self.found = None;
                        self.notify(failed(&err));
                    }
                }
                // The cursor stays where it is while its contact is still found (a search asked again after
                // a load); else it goes to the first row.
                let rows = self.rows();
                if !self.cursor.as_ref().is_some_and(|cursor| rows.contains(cursor)) {
                    self.cursor = rows.first().cloned();
                }
            }
            Message::Hover(id) => self.hover = id,
            Message::Open(id) => {
                self.menu = None;
                return self.open(id);
            }
            Message::RowMenu(id, at) => {
                self.cursor = Some(id.clone());
                self.menu = Some(Menu::Row(id, at));
            }
            Message::QuickMail(id) => {
                self.menu = None;
                let link = self.summary(&id).and_then(|c| c.email.as_deref()).and_then(contact::mail_link);
                return self.quick(id, link);
            }
            Message::QuickCall(id) => {
                self.menu = None;
                let link = self.summary(&id).and_then(|c| c.phone.as_deref()).and_then(contact::tel_link);
                return self.quick(id, link);
            }
            Message::QuickCopy(id) => {
                self.menu = None;
                let text = self.summary(&id).map(copy_text).unwrap_or_default();
                return Task::batch([self.update(Message::CopyText(text)), self.mark_used(id)]);
            }

            Message::Fetched(id, result) => return self.fetched(id, result),
            // A failure leaves the suggestions to the groups of the loaded contacts.
            Message::Groups(_, None) => {}
            Message::Groups(book, Some(names)) => {
                if let Some(detail) = &mut self.detail
                    && detail.contact.as_ref().is_some_and(|c| c.book == book)
                {
                    detail.known = Some((book, names));
                }
            }
            Message::Refetch => {
                if let Some(detail) = &mut self.detail {
                    detail.error = None;
                }
                return self.refetch();
            }
            Message::Close => {
                self.detail = None;
                self.menu = None;
                self.large = None;
            }
            Message::MapSettings(key, shown) => return self.map_settings(&key, shown),
            Message::Located(key, index, result) => return self.located(&key, index, result),
            Message::MapPicture(key, index, picture) => {
                let map = picture.map_or(AddressMap::Unavailable, AddressMap::Shown);
                self.set_map(&key, index, map);
            }
            Message::OpenInMaps(index) => {
                let Some(id) = self.detail.as_ref().and_then(|d| d.id.clone()) else { return Task::none() };
                let backend = self.backend.clone();
                return Task::batch([
                    Task::perform(async move { backend.open_in_maps(id, index).await }, Message::OpenedInMaps),
                    self.cursor.clone().map(|id| self.mark_used(id)).unwrap_or_else(Task::none),
                ]);
            }
            Message::OpenedInMaps(Ok(())) => {}
            Message::OpenedInMaps(Err(_)) => self.notify(tr("The maps app could not be opened.")),
            Message::OpenMapSettings => torro_gui::connect::open_section("general"),
            Message::OpenUri(uri) => {
                if let Err(err) = contact::open_uri(&uri) {
                    self.notify(err);
                }
                if let Some(id) = self.detail.as_ref().and_then(|d| d.id.clone()) {
                    return self.mark_used(id);
                }
            }
            Message::CopyText(text) => {
                self.menu = None;
                if text.is_empty() {
                    return Task::none();
                }
                self.notify(tr("Copied"));
                return iced::clipboard::write(text);
            }
            Message::ToggleFavorite => {
                self.menu = None;
                let Some((id, favorite)) = self.open_contact().map(|c| (c.id.clone(), !c.favorite)) else {
                    return Task::none();
                };
                self.set_busy(true);
                let backend = self.backend.clone();
                let asked = id.clone();
                return Task::perform(async move { backend.set_favorite(asked, favorite).await }, move |result| {
                    Message::Favorited(id.clone(), favorite, result)
                });
            }
            Message::Favorited(id, favorite, result) => {
                if !self.is_open(&id) {
                    return self.answered_elsewhere(result.err());
                }
                self.set_busy(false);
                match result {
                    Ok(()) => {
                        if let Some(contact) =
                            self.detail.as_mut().and_then(|d| d.contact.as_mut()).filter(|c| c.id == id)
                        {
                            contact.favorite = favorite;
                        }
                        if let Some(summary) = self.contacts.iter_mut().find(|c| c.id == id) {
                            summary.favorite = favorite;
                        }
                        return self.load();
                    }
                    Err(err) => self.change_failed(&err),
                }
            }
            Message::MoveTo(book) => {
                self.menu = None;
                let Some((id, etag)) = self.open_contact().map(|c| (c.id.clone(), c.etag.clone().unwrap_or_default()))
                else {
                    return Task::none();
                };
                self.set_busy(true);
                let backend = self.backend.clone();
                let asked = id.clone();
                return Task::perform(async move { backend.move_contact(asked, etag, book).await }, move |result| {
                    Message::Moved(id.clone(), result)
                });
            }
            // An answer about a contact no longer open brings the list up to date; a failure is said all
            // the same, so nothing that did not happen passes for done.
            Message::Moved(id, result) if !self.is_open(&id) => return self.answered_elsewhere(result.err()),
            Message::Deleted(id, result) if !self.is_open(&id) => return self.answered_elsewhere(result.err()),
            Message::Changed(id, result) if !self.is_open(&id) => return self.answered_elsewhere(result.err()),
            Message::Resolved(id, result) if !self.is_open(&id) => return self.answered_elsewhere(result.err()),
            Message::Moved(_, result) => {
                self.set_busy(false);
                match result {
                    Ok(moved) => {
                        if moved.left_in_source {
                            self.notify(tr("The old address book kept the contact too."));
                        }
                        let id = moved.contact.id.clone();
                        if let Some(detail) = &mut self.detail {
                            detail.id = Some(id);
                            detail.contact = Some(moved.contact);
                        }
                        return self.load();
                    }
                    Err(err) => self.change_failed(&err),
                }
            }
            Message::AskDelete => {
                self.menu = None;
                if self.open_contact().is_some() {
                    self.dialog = Some(Dialog::Delete);
                }
            }
            Message::CloseDialog => self.dialog = None,
            Message::Delete => {
                self.dialog = None;
                let Some((id, etag)) = self.open_contact().map(|c| (c.id.clone(), c.etag.clone().unwrap_or_default()))
                else {
                    return Task::none();
                };
                self.set_busy(true);
                let backend = self.backend.clone();
                let asked = id.clone();
                return Task::perform(async move { backend.delete_contact(asked, etag).await }, move |result| {
                    Message::Deleted(id.clone(), result)
                });
            }
            Message::Deleted(_, result) => {
                self.set_busy(false);
                match result {
                    Ok(()) => {
                        self.detail = None;
                        self.large = None;
                        self.notify(tr("Contact deleted"));
                        return self.load();
                    }
                    Err(err) => self.change_failed(&err),
                }
            }
            Message::GroupText(text) => {
                if let Some(detail) = &mut self.detail {
                    detail.group_text = text;
                }
            }
            Message::AddGroup(name) => {
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return Task::none();
                }
                let Some((id, etag)) = self.open_contact().map(|c| (c.id.clone(), c.etag.clone().unwrap_or_default()))
                else {
                    return Task::none();
                };
                if let Some(detail) = &mut self.detail {
                    detail.group_text.clear();
                }
                self.set_busy(true);
                let backend = self.backend.clone();
                let asked = id.clone();
                return Task::perform(async move { backend.add_to_group(asked, etag, name).await }, move |result| {
                    Message::Changed(id.clone(), result)
                });
            }
            Message::RemoveGroup(name) => {
                let Some((id, etag)) = self.open_contact().map(|c| (c.id.clone(), c.etag.clone().unwrap_or_default()))
                else {
                    return Task::none();
                };
                self.set_busy(true);
                let backend = self.backend.clone();
                let asked = id.clone();
                return Task::perform(
                    async move { backend.remove_from_group(asked, etag, name).await },
                    move |result| Message::Changed(id.clone(), result),
                );
            }
            Message::Changed(_, result) => {
                self.set_busy(false);
                match result {
                    Ok(contact) => {
                        // A new picture (or none) shows large at once.
                        let picture = self.large_photo(&contact);
                        if let Some(detail) = &mut self.detail {
                            detail.error = None;
                            // A form open meanwhile (a new picture while editing) saves on the new version.
                            if let Some(draft) = &mut detail.draft {
                                draft.etag = contact.etag.clone();
                            }
                            detail.contact = Some(contact);
                        }
                        return Task::batch([self.load(), picture]);
                    }
                    Err(err) => {
                        self.change_failed(&err);
                        return self.refetch();
                    }
                }
            }
            Message::Resolve(keep) => {
                let Some((id, etag)) = self.open_contact().map(|c| (c.id.clone(), c.etag.clone().unwrap_or_default()))
                else {
                    return Task::none();
                };
                self.set_busy(true);
                let backend = self.backend.clone();
                let asked = id.clone();
                return Task::perform(
                    async move { backend.resolve_conflict(asked, keep, etag).await },
                    move |result| Message::Resolved(id.clone(), result),
                );
            }
            Message::Resolved(_, result) => {
                self.set_busy(false);
                match result {
                    Ok(kept) => {
                        match (&mut self.detail, kept.into_iter().next()) {
                            (Some(detail), Some(first)) => {
                                detail.id = Some(first.id.clone());
                                detail.contact = Some(first);
                                detail.error = None;
                            }
                            // Nothing stays (the server's version was a deletion): the panel closes.
                            (_, None) => {
                                self.detail = None;
                                self.large = None;
                                self.notify(tr("The contact is gone."));
                            }
                            (None, Some(_)) => {}
                        }
                        return self.load();
                    }
                    Err(err) => self.change_failed(&err),
                }
            }
            // "Recent" shows the contact just used at once.
            Message::Used if self.view == View::Recent && !self.loading => return self.load(),
            Message::Used => {}

            Message::Edit => return self.edit(),
            // Unsaved input is never dropped for a new contact: the form is closed first.
            Message::NewContact(..) if self.editing() => {}
            Message::NewContact(book, group) => {
                self.menu = None;
                let Some(book) = book.filter(|b| self.book(b).is_some_and(|b| b.writable)).or_else(|| self.new_book())
                else {
                    self.notify(tr("No address book here may be written."));
                    return Task::none();
                };
                self.large = None;
                self.detail = Some(Detail {
                    id: None,
                    contact: None,
                    draft: Some({
                        self.note = text_editor::Content::new();
                        Draft::empty()
                    }),
                    new_in: Some((book, group)),
                    group_text: String::new(),
                    known: None,
                    busy: false,
                    error: None,
                    maps: Vec::new(),
                    maps_of: None,
                });
            }
            Message::NoteEdit(action) => {
                self.note.perform(action);
                let text = self.note.text();
                if let Some(draft) = self.detail.as_mut().and_then(|d| d.draft.as_mut()) {
                    draft.note = text.trim_end().to_owned();
                }
            }
            Message::Draft(msg) => {
                if let Some(draft) = self.detail.as_mut().and_then(|d| d.draft.as_mut()) {
                    draft.update(msg);
                }
                self.menu = None;
            }
            Message::ToggleKind(list, index) => return self.update(Message::ToggleMenu(Menu::Kind(list, index))),
            Message::ToggleAddField => return self.update(Message::ToggleMenu(Menu::AddField)),
            // While "Saving…" runs, the form waits for the answer: its error or the saved contact.
            Message::Cancel if self.detail.as_ref().and_then(|d| d.draft.as_ref()).is_some_and(|d| d.saving) => {}
            Message::Cancel => {
                self.menu = None;
                match &mut self.detail {
                    Some(detail) if detail.id.is_none() => self.detail = None,
                    Some(detail) => detail.draft = None,
                    None => {}
                }
            }
            Message::Done => return self.done(),
            Message::Saved(id, result) if !self.is_open(&id) => return self.answered_elsewhere(result.err()),
            Message::Saved(_, result) => return self.saved(result),
            Message::Refetched(result) => {
                if let (Ok(contact), Some(detail)) = (result, &mut self.detail)
                    && detail.id.as_deref() == Some(contact.id.as_str())
                {
                    if let Some(draft) = &mut detail.draft {
                        draft.etag = contact.etag.clone();
                    }
                    detail.contact = Some(contact);
                }
            }
            Message::Created(group, result) => match result {
                Ok(created) => {
                    let id = created.id.clone();
                    // Only the form that wrote it takes the new contact; one opened since stays as it is.
                    let writing = self
                        .detail
                        .as_ref()
                        .is_some_and(|d| d.id.is_none() && d.draft.as_ref().is_some_and(|d| d.saving));
                    if let Some(detail) = self.detail.as_mut().filter(|_| writing) {
                        detail.id = Some(id.clone());
                        detail.contact = Some(created.clone());
                        detail.draft = None;
                        detail.new_in = None;
                    }
                    self.notify(tr("Contact created"));
                    let mut tasks = vec![self.load(), self.mark_used(id.clone())];
                    if let Some(group) = group {
                        if writing {
                            self.set_busy(true);
                        }
                        let backend = self.backend.clone();
                        let etag = created.etag.clone().unwrap_or_default();
                        let asked = id.clone();
                        tasks.push(Task::perform(
                            async move { backend.add_to_group(asked, etag, group).await },
                            move |result| Message::Changed(id.clone(), result),
                        ));
                    }
                    return Task::batch(tasks);
                }
                Err(err) => {
                    if let Some(draft) = self.detail.as_mut().and_then(|d| d.draft.as_mut()) {
                        draft.saving = false;
                        draft.error = Some(failed(&err));
                    }
                }
            },
            // The picture belongs to the contact it was chosen for, whichever is open when it comes.
            Message::ChoosePhoto => {
                let Some(id) = self.open_contact().map(|c| c.id.clone()) else { return Task::none() };
                return Task::perform(choose_photo(), move |picture| Message::PhotoChosen(id.clone(), picture));
            }
            Message::PhotoChosen(_, None) => {}
            Message::PhotoChosen(_, Some(Err(err))) => self.notify(err),
            Message::PhotoChosen(id, Some(Ok(_))) if !self.is_open(&id) => {
                self.notify(tr("The picture was not set: another contact is open now."));
            }
            Message::PhotoChosen(_, Some(Ok(bytes))) => return self.set_photo(Some(bytes)),
            Message::RemovePhoto => return self.set_photo(None),
            Message::FileDropped(path) => {
                let Some(id) = self.open_contact().map(|c| c.id.clone()) else { return Task::none() };
                return Task::perform(async move { Some(read_photo(&path)) }, move |picture| {
                    Message::PhotoChosen(id.clone(), picture)
                });
            }
        }
        Task::none()
    }

    /// The width of the content beside the sidebar (or rail), as the person dragged the sidebar.
    pub fn list_room(&self) -> f32 {
        let size = self.content_size();
        let class = self.class();
        match shell::sidebar_mode(class, self.sidebar) {
            Some(SidebarMode::Expanded) => {
                let limits = shell::sidebar_limits(size.width, split::min_pane());
                (size.width - self.side.width(limits) - layout::inset()).max(0.0)
            }
            _ => shell::content_width(class, self.sidebar, size.width),
        }
    }

    /// The width of the open panel beside the list (wide only), else `None`.
    pub fn panel_room(&self) -> Option<f32> {
        (self.detail.is_some() && torro_gui::panel::mode_for(self.class()) == torro_gui::panel::PanelMode::Beside)
            .then(|| self.panel_split.width(torro_gui::panel::beside_limits(self.list_room())))
    }

    /// The width the list gets.
    pub fn list_width(&self) -> f32 {
        crate::list::list_width(self.list_room(), self.panel_room())
    }

    fn escape(&mut self) -> Task<Message> {
        if self.menu.is_some() {
            self.menu = None;
        } else if self.dialog.is_some() {
            self.dialog = None;
        } else if self.drawer {
            self.drawer = false;
        } else if self.editing() {
            return self.update(Message::Cancel);
        } else if self.detail.is_some() {
            self.detail = None;
            self.large = None;
        }
        Task::none()
    }

    fn key(&mut self, press: Press) -> Task<Message> {
        match press {
            Press::Search => operation::focus(SEARCH),
            Press::Sidebar => self.update(Message::ToggleSidebar),
            Press::Edit => self.edit(),
            Press::Mail | Press::Call if self.editing() => Task::none(),
            Press::Mail => match self.cursor.clone() {
                Some(id) => self.update(Message::QuickMail(id)),
                None => Task::none(),
            },
            Press::Call => match self.cursor.clone() {
                Some(id) => self.update(Message::QuickCall(id)),
                None => Task::none(),
            },
            Press::Up | Press::Down => {
                if self.editing() {
                    return Task::none();
                }
                let rows = self.rows();
                let at = self.cursor.as_ref().and_then(|id| rows.iter().position(|row| row == id));
                let down = press == Press::Down;
                let next = match at {
                    None if down => 0,
                    None => rows.len().saturating_sub(1),
                    Some(at) if down => (at + 1).min(rows.len().saturating_sub(1)),
                    Some(at) => at.saturating_sub(1),
                };
                self.cursor = rows.get(next).cloned();
                Task::none()
            }
            Press::Enter => match self.cursor.clone().or_else(|| self.rows().first().cloned()) {
                Some(id) if !self.editing() => self.open(id),
                _ => Task::none(),
            },
        }
    }

    /// The contact open in the panel, once fetched.
    /// The answer to a change of a contact that is no longer open: the list loads again, and a failure is
    /// said in the notice.
    fn answered_elsewhere(&mut self, err: Option<ContactsError>) -> Task<Message> {
        if let Some(err) = err {
            self.notify(failed(&err));
        }
        self.load()
    }

    /// Whether the contact `id` is the one open in the panel.
    pub fn is_open(&self, id: &str) -> bool {
        self.detail.as_ref().and_then(|d| d.id.as_deref()) == Some(id)
    }

    pub fn open_contact(&self) -> Option<&Contact> {
        self.detail.as_ref()?.contact.as_ref()
    }

    fn set_busy(&mut self, busy: bool) {
        if let Some(detail) = &mut self.detail {
            detail.busy = busy;
            if busy {
                detail.error = None;
            }
        }
    }

    fn change_failed(&mut self, err: &ContactsError) {
        match &mut self.detail {
            Some(detail) => detail.error = Some(failed(err)),
            None => self.notify(failed(err)),
        }
    }

    /// Opens the contact `id` in the panel: fetched whole, marked as used, its large picture asked for.
    fn open(&mut self, id: String) -> Task<Message> {
        if self.editing() {
            return Task::none();
        }
        self.cursor = Some(id.clone());
        if self.detail.as_ref().and_then(|d| d.id.as_deref()) == Some(id.as_str()) {
            return Task::none();
        }
        self.detail = Some(Detail::of(id.clone()));
        self.large = None;
        let backend = self.backend.clone();
        let asked = id.clone();
        let fetch = Task::perform(async move { backend.get_contact(asked).await }, move |result| {
            Message::Fetched(id.clone(), result)
        });
        let used = self.cursor.clone().map(|id| self.mark_used(id)).unwrap_or_else(Task::none);
        Task::batch([fetch, used])
    }

    fn refetch(&self) -> Task<Message> {
        let Some(id) = self.detail.as_ref().and_then(|d| d.id.clone()) else { return Task::none() };
        let backend = self.backend.clone();
        let asked = id.clone();
        Task::perform(async move { backend.get_contact(asked).await }, move |result| {
            Message::Fetched(id.clone(), result)
        })
    }

    fn mark_used(&self, id: String) -> Task<Message> {
        let backend = self.backend.clone();
        Task::perform(async move { backend.mark_used(id).await }, |_| Message::Used)
    }

    /// Mail or Call of a row: the system's program, and the contact counts as used.
    fn quick(&mut self, id: String, link: Option<String>) -> Task<Message> {
        let Some(link) = link else { return Task::none() };
        if let Err(err) = contact::open_uri(&link) {
            self.notify(err);
        }
        self.mark_used(id)
    }

    fn fetched(&mut self, id: String, result: Result<Contact, ContactsError>) -> Task<Message> {
        if self.detail.as_ref().and_then(|d| d.id.as_deref()) != Some(id.as_str()) {
            return Task::none();
        }
        match result {
            Ok(contact) => {
                let picture = self.large_photo(&contact);
                let groups = self.list_groups(contact.book.clone());
                let picture = Task::batch([picture, groups]);
                if let Some(detail) = &mut self.detail {
                    detail.contact = Some(contact);
                }
                // `--open edit` turns the form on once, for its picture.
                if std::mem::take(&mut self.edit_on_fetch) {
                    return Task::batch([self.edit(), picture]);
                }
                picture
            }
            Err(err) => {
                if let Some(detail) = &mut self.detail {
                    detail.error = Some(failed(&err));
                }
                Task::none()
            }
        }
    }

    /// The maps of the open contact (issue #355, decision 3 of its plan): once per version (`id|etag`),
    /// when it is open and has addresses. First what the settings say ([`Message::MapSettings`]),
    /// then each address after the other ([`App::located`]), since the core looks up one at a time
    /// (a second lookup within its second would be `busy`). `--open map-…` sets the state at once.
    fn start_maps(&mut self) -> Task<Message> {
        let forced = self.launch.open.and_then(Open::map_state);
        let Some(detail) = &mut self.detail else { return Task::none() };
        let Some(contact) = &detail.contact else { return Task::none() };
        let key = format!("{}|{}", contact.id, contact.etag.as_deref().unwrap_or_default());
        if detail.maps_of.as_deref() == Some(key.as_str()) {
            return Task::none();
        }
        let count = contact.addresses.len();
        detail.maps_of = Some(key.clone());
        detail.maps = vec![forced.clone().unwrap_or(AddressMap::Finding); count];
        if count == 0 || forced.is_some() {
            return Task::none();
        }
        let backend = self.backend.clone();
        Task::perform(async move { backend.settings().await }, move |shown| Message::MapSettings(key.clone(), shown))
    }

    /// The id of the open contact when `key` is still the version its maps are for.
    fn maps_for(&self, key: &str) -> Option<String> {
        let detail = self.detail.as_ref().filter(|d| d.maps_of.as_deref() == Some(key))?;
        detail.contact.as_ref().map(|contact| contact.id.clone())
    }

    fn set_map(&mut self, key: &str, index: usize, map: AddressMap) {
        if self.maps_for(key).is_none() {
            return;
        }
        if let Some(slot) = self.detail.as_mut().and_then(|detail| detail.maps.get_mut(index)) {
            *slot = map;
        }
    }

    /// With maps off every address says so and nothing is asked; without an answer of the core the
    /// maps are not available; else the first address is located.
    fn map_settings(&mut self, key: &str, shown: Option<backend::Shown>) -> Task<Message> {
        if self.maps_for(key).is_none() {
            return Task::none();
        }
        let all = match shown {
            Some(shown) if shown.maps => return self.locate(key, 0),
            Some(_) => AddressMap::MapsOff,
            None => AddressMap::Unavailable,
        };
        if let Some(detail) = &mut self.detail {
            detail.maps.iter_mut().for_each(|map| *map = all.clone());
        }
        Task::none()
    }

    /// Asks where the address number `index` of the open contact is.
    fn locate(&self, key: &str, index: usize) -> Task<Message> {
        let Some(id) = self.maps_for(key) else { return Task::none() };
        let backend = self.backend.clone();
        let key = key.to_owned();
        Task::perform(async move { backend.locate(id, index).await }, move |result| {
            Message::Located(key.clone(), index, result)
        })
    }

    /// The state of the address number `index`, the picture for a place found, and the next address.
    fn located(&mut self, key: &str, index: usize, result: Result<ContactLocation, String>) -> Task<Message> {
        if self.maps_for(key).is_none() {
            return Task::none();
        }
        let (map, picture) = match result {
            Ok(ContactLocation::Found { geo, .. }) => (AddressMap::Finding, self.map_picture(key, index, geo)),
            Ok(ContactLocation::NoConsent) => (AddressMap::LookupOff, Task::none()),
            // Only a client gets it; the app is the person.
            Ok(ContactLocation::NotFound | ContactLocation::NotLookedUp) => (AddressMap::NotFound, Task::none()),
            Err(_) => (AddressMap::Unavailable, Task::none()),
        };
        self.set_map(key, index, map);
        let more = self.detail.as_ref().is_some_and(|detail| index + 1 < detail.maps.len());
        let next = if more { self.locate(key, index + 1) } else { Task::none() };
        Task::batch([picture, next])
    }

    /// The picture of the map at `geo` in the darkness the window is drawn in; the demo draws its sketch.
    fn map_picture(&self, key: &str, index: usize, geo: Geo) -> Task<Message> {
        let theme = self.theme();
        let key = key.to_owned();
        if self.backend.is_demo() {
            return Task::done(Message::MapPicture(key, index, Some(torro_gui::map::demo_map(&theme))));
        }
        let dark = theme.extended_palette().is_dark;
        let backend = self.backend.clone();
        Task::perform(async move { backend.map(geo, dark).await }, move |bytes| {
            Message::MapPicture(key.clone(), index, bytes.as_deref().and_then(picture))
        })
    }

    /// Asks for the groups of `book` (`contacts.list_groups`), the suggestions of the group input; a
    /// failure leaves the suggestions to the groups of the loaded contacts.
    fn list_groups(&self, book: String) -> Task<Message> {
        let backend = self.backend.clone();
        let asked = book.clone();
        Task::perform(async move { backend.list_groups(Some(asked)).await }, move |result| {
            let names = result.ok().map(|groups| {
                let mut names: Vec<String> = groups.into_iter().map(|g| g.name).collect();
                names.sort_by_key(|n| n.to_lowercase());
                names.dedup_by(|a, b| same_group(a, b));
                names
            });
            Message::Groups(book.clone(), names)
        })
    }

    /// Asks for the large picture of `contact`, when it has one and it is not here yet.
    fn large_photo(&mut self, contact: &Contact) -> Task<Message> {
        let Some(photo) = &contact.photo else {
            self.large = None;
            return Task::none();
        };
        if self.large.as_ref().is_some_and(|(id, version, _)| *id == contact.id && *version == photo.version) {
            return Task::none();
        }
        let backend = self.backend.clone();
        let (id, asked, version) = (contact.id.clone(), contact.id.clone(), photo.version.clone());
        Task::perform(async move { backend.get_photo(asked, PhotoSize::Large).await.ok() }, move |bytes| {
            Message::LargePhoto(id.clone(), version.clone(), bytes)
        })
    }

    fn edit(&mut self) -> Task<Message> {
        let Some(detail) = &mut self.detail else { return Task::none() };
        let Some(contact) = &detail.contact else { return Task::none() };
        // A contact in conflict is settled first (its buttons), never edited.
        if detail.draft.is_some() || contact.readonly || contact.state == torro_contacts::ContactState::Conflict {
            return Task::none();
        }
        let draft = Draft::of(contact);
        self.note = text_editor::Content::with_text(&draft.note);
        detail.draft = Some(draft);
        Task::none()
    }

    /// "Done": a new contact is created, a changed one updated with only what changed; nothing changed,
    /// the form just closes.
    fn done(&mut self) -> Task<Message> {
        self.menu = None;
        let Some(detail) = &mut self.detail else { return Task::none() };
        let Some(draft) = &mut detail.draft else { return Task::none() };
        if draft.saving {
            return Task::none();
        }
        draft.tried = true;
        let changes = draft.changes();
        let backend = self.backend.clone();
        if let Some((book, group)) = detail.new_in.clone() {
            if draft.is_blank() {
                self.detail = None;
                return Task::none();
            }
            draft.saving = true;
            draft.error = None;
            return Task::perform(async move { backend.create_contact(book, changes).await }, move |result| {
                Message::Created(group.clone(), result)
            });
        }
        if changes.is_empty() {
            detail.draft = None;
            return Task::none();
        }
        let Some(id) = detail.id.clone() else { return Task::none() };
        let etag = draft.etag.clone().unwrap_or_default();
        draft.saving = true;
        draft.error = None;
        let asked = id.clone();
        Task::perform(async move { backend.update_contact(asked, etag, changes).await }, move |result| {
            Message::Saved(id.clone(), result)
        })
    }

    fn saved(&mut self, result: Result<Contact, ContactsError>) -> Task<Message> {
        let Some(detail) = &mut self.detail else { return Task::none() };
        match result {
            Ok(contact) => {
                detail.contact = Some(contact);
                detail.draft = None;
                detail.error = None;
                self.load()
            }
            // Changed elsewhere meanwhile: the form stays, the contact is fetched again for its new
            // version, and the person checks and saves again.
            Err(err @ ContactsError::Conflict { .. }) => {
                if let Some(draft) = &mut detail.draft {
                    draft.saving = false;
                    draft.error = Some(failed(&err));
                }
                let Some(id) = detail.id.clone() else { return Task::none() };
                let backend = self.backend.clone();
                Task::perform(async move { backend.get_contact(id).await }, Message::Refetched)
            }
            Err(err) => {
                if let Some(draft) = &mut detail.draft {
                    draft.saving = false;
                    draft.error = Some(failed(&err));
                }
                Task::none()
            }
        }
    }

    fn set_photo(&mut self, bytes: Option<Vec<u8>>) -> Task<Message> {
        let Some((id, etag)) = self.open_contact().map(|c| (c.id.clone(), c.etag.clone().unwrap_or_default())) else {
            return Task::none();
        };
        self.set_busy(true);
        self.large = None;
        let backend = self.backend.clone();
        let asked = id.clone();
        Task::perform(async move { backend.set_photo(asked, etag, bytes).await }, move |result| {
            Message::Changed(id.clone(), result)
        })
    }

    fn loaded(&mut self, result: Result<Loaded, ContactsError>) -> Task<Message> {
        self.loading = false;
        match result {
            Ok(loaded) => {
                self.books = loaded.books;
                self.contacts = loaded.contacts;
                self.birthdays = loaded.birthdays;
                self.recent = loaded.recent;
                self.loaded = true;
                self.error = None;
            }
            Err(err) => {
                if !self.loaded {
                    self.error = Some(load_failed(&err));
                    self.loaded = true;
                } else {
                    self.notify(failed(&err));
                }
            }
        }
        // A view whose book is gone falls back to all contacts.
        if let View::Book(id) | View::Group(id, _) = &self.view
            && self.book(id).is_none()
        {
            self.view = View::All;
        }
        // Pictures of contacts that have none any more, or another one, are dropped.
        let pictures: HashMap<String, String> =
            self.contacts.iter().filter_map(|c| c.photo.as_ref().map(|p| (c.id.clone(), p.version.clone()))).collect();
        self.photos.retain(|id, (version, _)| pictures.get(id) == Some(version));
        let open = self.apply_open();
        // The open contact follows the load (a change synced, a conflict found), unless it is edited or
        // a change of it is out.
        let follow = match &self.detail {
            Some(detail) if detail.draft.is_none() && !detail.busy && detail.contact.is_some() => self.refetch(),
            _ => Task::none(),
        };
        // A running search asks again, so a contact new on the list is found too.
        let search = if self.search.trim().is_empty() {
            Task::none()
        } else {
            self.search_asked += 1;
            let asked = self.search_asked;
            self.update(Message::SearchDue(asked))
        };
        Task::batch([open, self.fetch_photos(), follow, search])
    }

    /// `--demo --open <state>`: what the state asks for, once, after the first load. The last three set
    /// what a load could bring (no contacts, no account, a failure).
    fn apply_open(&mut self) -> Task<Message> {
        if self.applied {
            return Task::none();
        }
        self.applied = true;
        let Some(open) = self.launch.open else { return Task::none() };
        let mut tasks = Vec::new();
        match open {
            Open::GroupMenu => self.menu = Some(Menu::GroupBy),
            Open::Search | Open::MiniSearch => tasks.push(self.update(Message::Search("an".into()))),
            Open::New => tasks.push(self.update(Message::NewContact(None, None))),
            Open::Birthdays => self.view = View::Birthdays,
            Open::Empty => {
                self.contacts.clear();
                self.birthdays.clear();
                self.recent.clear();
            }
            Open::NoAccount => {
                self.books.clear();
                self.contacts.clear();
                self.birthdays.clear();
            }
            Open::Error => self.error = Some(load_failed(&ContactsError::Offline)),
            Open::List | Open::ByCity | Open::ByBook => {}
            Open::Contact
            | Open::Edit
            | Open::Conflict
            | Open::Readonly
            | Open::Pending
            | Open::Delete
            | Open::Move
            | Open::MapOff
            | Open::MapLookupOff
            | Open::MapFinding
            | Open::MapNotFound
            | Open::MapUnavailable => {}
        }
        if let Some(uid) = open.contact() {
            let suffix = format!("/{uid}");
            if let Some(id) = self.contacts.iter().find(|c| c.id.ends_with(&suffix)).map(|c| c.id.clone()) {
                tasks.push(self.open(id));
            }
            match open {
                Open::Delete => self.dialog = Some(Dialog::Delete),
                Open::Move => self.menu = Some(Menu::Move),
                _ => {}
            }
        }
        Task::batch(tasks)
    }

    /// The next batch of small pictures: contacts with a picture whose version is not here yet.
    fn fetch_photos(&mut self) -> Task<Message> {
        if self.photos_busy {
            return Task::none();
        }
        let wanted: Vec<(String, String)> = self
            .contacts
            .iter()
            .filter_map(|c| c.photo.as_ref().map(|p| (c.id.clone(), p.version.clone())))
            .filter(|(id, version)| self.photos.get(id).is_none_or(|(have, _)| have != version))
            .take(PHOTO_BATCH)
            .collect();
        if wanted.is_empty() {
            return Task::none();
        }
        self.photos_busy = true;
        let backend = self.backend.clone();
        Task::perform(
            async move {
                let mut batch = Vec::new();
                for (id, version) in wanted {
                    let bytes = backend.get_photo(id.clone(), PhotoSize::Small).await.ok();
                    batch.push((id, version, bytes));
                }
                batch
            },
            Message::Photos,
        )
    }
}

/// A picture the core sent (PNG or JPEG), decoded here as the chat does, so it draws at once; `None`
/// for bytes that are no picture.
pub fn picture(bytes: &[u8]) -> Option<Handle> {
    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(Handle::from_rgba(decoded.width(), decoded.height(), decoded.into_raw()))
}

/// Whether two group names are one group: the same without case (Unicode, as the list's heads compare
/// them) and the spaces around them.
pub fn same_group(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// What "Copy" of a row puts on the clipboard: the name, the main mail address and number, a line each.
pub fn copy_text(contact: &ContactSummary) -> String {
    [Some(contact.display_name.clone()), contact.email.clone(), contact.phone.clone()]
        .into_iter()
        .flatten()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The keys of the list (see [`Press`]), only those no widget took: a text field types `/` itself.
fn key_press(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else { return None };
    let command = modifiers.command();
    // Ctrl+F and Ctrl+E work from a field too.
    match key.as_ref() {
        keyboard::Key::Character("f") if command => return Some(Message::Key(Press::Search)),
        keyboard::Key::Character("e") if command => return Some(Message::Key(Press::Edit)),
        _ => {}
    }
    if status == iced::event::Status::Captured {
        return None;
    }
    if shell::is_sidebar_key(&key, modifiers) {
        return Some(Message::Key(Press::Sidebar));
    }
    if command || modifiers.alt() {
        return None;
    }
    match key.as_ref() {
        keyboard::Key::Character("/") => Some(Message::Key(Press::Search)),
        keyboard::Key::Character("m") => Some(Message::Key(Press::Mail)),
        keyboard::Key::Character("c") => Some(Message::Key(Press::Call)),
        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Some(Message::Key(Press::Up)),
        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Some(Message::Key(Press::Down)),
        keyboard::Key::Named(keyboard::key::Named::Enter) => Some(Message::Key(Press::Enter)),
        _ => None,
    }
}

/// A file dropped on the window.
fn file_drop(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Window(iced::window::Event::FileDropped(path)) => Some(Message::FileDropped(path)),
        _ => None,
    }
}

/// The system's dialog "Choose picture"; the picture scaled for sending ([`prepare_photo`]). `None`
/// when the person cancels.
async fn choose_photo() -> Option<Result<Vec<u8>, String>> {
    let file = rfd::AsyncFileDialog::new()
        .set_title(tr("Choose picture"))
        .add_filter(tr("Pictures"), &["png", "jpg", "jpeg"])
        .pick_file()
        .await?;
    Some(prepare_photo(&file.read().await))
}

/// The picture at `path`, scaled for sending.
fn read_photo(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|_| tr("The picture could not be read.").to_owned())?;
    prepare_photo(&bytes)
}

/// A picture as the core takes it: decoded (PNG or JPEG), scaled down to at most [`PHOTO_EDGE`] on its
/// longer edge, as JPEG, and no larger than [`MAX_PHOTO_BYTES`].
pub fn prepare_photo(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let picture = image::load_from_memory(bytes).map_err(|_| tr("This is no picture Torro can read.").to_owned())?;
    let picture = if picture.width() > PHOTO_EDGE || picture.height() > PHOTO_EDGE {
        picture.resize(PHOTO_EDGE, PHOTO_EDGE, image::imageops::FilterType::Lanczos3)
    } else {
        picture
    };
    let mut out = std::io::Cursor::new(Vec::new());
    picture
        .to_rgb8()
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|_| tr("The picture could not be saved.").to_owned())?;
    let out = out.into_inner();
    if out.len() > MAX_PHOTO_BYTES {
        return Err(tr("The picture is too large.").to_owned());
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
