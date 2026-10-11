//! The tasks window: state, loading, quick add, saving, and the Linear window around it (task
//! T.1): `torro_gui::shell` with the sidebar (filters, lists, "Open Calendar", "Settings") in the
//! wide layout and a navigation menu ("Today ▾") plus drawer in the medium and narrow ones. The
//! list (`list.rs`) and the detail panel (`detail.rs`) only render and report clicks as
//! [`Message`]s.

use std::time::Duration;

use chrono::{Local, NaiveDate};
use iced::theme::Mode;
use iced::widget::{column, container, responsive, row, rule, text, text_input, toggler};
use iced::{Alignment, Element, Fill, Padding, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::connect::{self, Connect};
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, Switcher};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail, collection_color};
use torro_gui::{style, typo};
use torro_ipc::{Request, Response};
use torro_pim::{AccountState, Collection, EventTime, PimSource, Todo};

use crate::detail::{self, Detail};
use crate::list::{self, ListData};

/// Which tasks the list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// Due today or overdue, from all lists.
    Today,
    /// With a due date, from all lists.
    Planned,
    /// Everything.
    All,
    /// One list ([`Collection::id`]).
    List(String),
}

/// The menu that is open over the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// The app switcher at the top of the sidebar.
    Switcher,
    /// The navigation menu of the medium and narrow layouts ("Today ▾").
    Nav,
}

/// The id of the quick add field, so "New" can focus it.
const QUICK_ADD: &str = "quick-add";
/// The one-pixel line between the list and the detail.
const LINE: f32 = 1.0;

pub struct App {
    source: Source,
    today: NaiveDate,
    account: Option<AccountState>,
    connect: Connect,
    collections: Vec<Collection>,
    todos: Vec<Todo>,
    filter: Filter,
    show_completed: bool,
    quick_add: String,
    /// Index into `todos` of the task open in the detail panel.
    selected: Option<usize>,
    detail: Option<Detail>,
    error: Option<String>,
    appearance: Appearance,
    system_mode: Mode,
    /// The open menu, if any.
    menu: Option<Menu>,
    /// Whether the sidebar drawer is open (medium and narrow layouts).
    drawer: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    AccountLoaded(Result<AccountState, String>),
    Connect(connect::Message),
    Loaded(Result<(Vec<Collection>, Vec<Todo>), String>),
    SetFilter(Filter),
    ShowCompleted(bool),
    QuickAddChanged(String),
    QuickAddSubmit,
    /// From the list: tick or untick `todos[index]`.
    ToggleDone(usize),
    /// From the list: open `todos[index]` in the detail panel.
    Select(usize),
    Detail(detail::Message),
    Saved(Result<Todo, String>),
    Deleted(Result<(), String>),
    Refresh,
    Tick,
    /// The desktop's light/dark preference (once at start, then whenever it changes).
    SystemMode(Mode),
    /// The appearance stored in the suite settings; an error keeps the one we have.
    Appearance(Result<Appearance, String>),
    /// From the app switcher at the top of the sidebar.
    Switcher(Switcher),
    /// Open or close the navigation menu ("Today ▾").
    ToggleNav,
    /// Open the sidebar drawer (medium and narrow layouts).
    ShowDrawer,
    /// Close the open menu and the drawer (click beside them, Escape).
    CloseMenu,
    /// Start or raise another app of the suite ([`AppId::binary`]) through the core.
    OpenApp(&'static str),
    /// The core's answer to [`Message::OpenApp`].
    AppOpened(Result<(), String>),
    /// Open the control centre.
    OpenSettings,
}

impl App {
    pub fn new(source: Source) -> (App, Task<Message>) {
        let app = App {
            source,
            today: Local::now().date_naive(),
            account: None,
            connect: Connect,
            collections: Vec::new(),
            todos: Vec::new(),
            filter: Filter::Today,
            show_completed: false,
            quick_add: String::new(),
            selected: None,
            detail: None,
            error: None,
            appearance: Appearance::System,
            system_mode: Mode::None,
            menu: None,
            drawer: false,
        };
        let task =
            Task::batch([app.fetch_account(), app.fetch_appearance(), iced::system::theme().map(Message::SystemMode)]);
        (app, task)
    }

    pub fn title(&self) -> String {
        trf("Tasks · {filter}", &[("filter", &self.filter_name(&self.filter))])
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let every = if self.connected() { Duration::from_secs(60) } else { Duration::from_secs(2) };
        let mut subscriptions = vec![
            iced::time::every(every).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemMode),
        ];
        // Escape closes an open menu or drawer; nothing listens while none is open.
        if self.menu.is_some() || self.drawer {
            subscriptions.push(widgets::escape().map(|()| Message::CloseMenu));
        }
        Subscription::batch(subscriptions)
    }

    /// The Iced theme: the stored appearance, with "System" following the desktop.
    pub fn theme(&self) -> Theme {
        theme::for_appearance(self.appearance, self.system_mode)
    }

    /// Asks the core for the stored appearance. Done on every tick, so a change in the control
    /// centre reaches the open window within a minute (two seconds while no account is set up).
    fn fetch_appearance(&self) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { theme::stored_appearance(&source).await }, Message::Appearance)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AccountLoaded(Ok(account)) => {
                let was_connected = self.connected();
                self.account = Some(account);
                if self.connected() && !was_connected {
                    return self.load();
                }
            }
            Message::AccountLoaded(Err(err)) => self.error = Some(err),
            Message::Connect(message) => match self.connect.update(message) {
                // The account is set up in the control centre; the polling in `Tick` picks it up.
                connect::Action::OpenSettings => connect::open_settings(),
            },
            Message::Loaded(result) => match result {
                Ok((collections, todos)) => {
                    self.collections = collections;
                    self.todos = todos;
                    self.error = None;
                    // Keep the detail panel on the same task after a reload.
                    let selected_uid = self.detail.as_ref().map(|d| d.original.uid.clone());
                    self.selected = selected_uid.and_then(|uid| self.todos.iter().position(|t| t.uid == uid));
                    if self.selected.is_none() {
                        self.detail = None;
                    }
                }
                Err(err) => self.error = Some(err),
            },
            Message::SetFilter(filter) => {
                self.filter = filter;
                self.selected = None;
                self.detail = None;
                self.close_menus();
            }
            Message::ShowCompleted(show) => self.show_completed = show,
            Message::QuickAddChanged(value) => self.quick_add = value,
            Message::QuickAddSubmit => {
                let summary = self.quick_add.trim().to_string();
                let Some(collection) = self.target_list() else {
                    self.error = Some(tr("No task list yet").into());
                    return Task::none();
                };
                if summary.is_empty() {
                    return Task::none();
                }
                self.quick_add.clear();
                let todo = Todo {
                    uid: String::new(),
                    collection,
                    href: None,
                    etag: None,
                    summary,
                    due: (self.filter == Filter::Today).then_some(EventTime::Date(self.today)),
                    priority: None,
                    completed: false,
                    parent: None,
                    description: None,
                };
                return self.save(todo);
            }
            Message::ToggleDone(index) => {
                if let Some(todo) = self.todos.get(index) {
                    let mut todo = todo.clone();
                    todo.completed = !todo.completed;
                    return self.save(todo);
                }
            }
            Message::Select(index) => {
                if let Some(todo) = self.todos.get(index) {
                    self.selected = Some(index);
                    self.detail = Some(Detail::new(todo, &self.writable_lists(), &self.todos));
                }
            }
            Message::Detail(message) => {
                let Some(detail) = self.detail.as_mut() else { return Task::none() };
                match detail.update(message) {
                    detail::Action::None => {}
                    detail::Action::Close => {
                        self.detail = None;
                        self.selected = None;
                    }
                    detail::Action::Save(todo) => return self.save(todo),
                    detail::Action::Delete(todo) => {
                        self.detail = None;
                        self.selected = None;
                        let source = self.source.clone();
                        return Task::perform(
                            async move { source.delete_todo(todo).await.map_err(|e| e.to_string()) },
                            Message::Deleted,
                        );
                    }
                }
            }
            Message::Saved(result) => match result {
                Ok(saved) => {
                    // The panel keeps editing the stored version, with its new etag.
                    if self.detail.as_ref().is_some_and(|d| d.original.uid == saved.uid) {
                        self.detail = Some(Detail::new(&saved, &self.writable_lists(), &self.todos));
                    }
                    return self.load();
                }
                Err(err) => self.error = Some(trf("Save failed: {error}", &[("error", &err)])),
            },
            Message::Deleted(result) => match result {
                Ok(()) => return self.load(),
                Err(err) => self.error = Some(trf("Delete failed: {error}", &[("error", &err)])),
            },
            Message::Refresh => {
                let source = self.source.clone();
                let sync =
                    Task::perform(async move { source.sync().await.map_err(|e| e.to_string()) }, |_| Message::Tick);
                return sync.chain(self.load());
            }
            Message::Tick => {
                self.today = Local::now().date_naive();
                let appearance = self.fetch_appearance();
                if !self.connected() {
                    return Task::batch([appearance, self.fetch_account()]);
                }
                if self.detail.is_none() {
                    return Task::batch([appearance, self.load()]);
                }
                return appearance;
            }
            Message::SystemMode(mode) => self.system_mode = mode,
            Message::Appearance(Ok(appearance)) => self.appearance = appearance,
            // The core may be starting up; keep the colours we have.
            Message::Appearance(Err(_)) => {}
            Message::Switcher(Switcher::Toggle) => self.toggle_menu(Menu::Switcher),
            Message::Switcher(Switcher::OpenApp(app)) => return self.update(Message::OpenApp(app)),
            Message::Switcher(Switcher::New) => {
                // "New" is a new task: the quick add field takes it.
                self.close_menus();
                return iced::widget::operation::focus(QUICK_ADD);
            }
            // The app has no search yet; the button only closes what is open.
            Message::Switcher(Switcher::Search) => self.close_menus(),
            Message::ToggleNav => self.toggle_menu(Menu::Nav),
            Message::ShowDrawer => {
                self.menu = None;
                self.drawer = true;
            }
            Message::CloseMenu => self.close_menus(),
            Message::OpenApp(app) => {
                self.close_menus();
                let source = self.source.clone();
                return Task::perform(open_app(source, app), Message::AppOpened);
            }
            Message::AppOpened(Ok(())) => {}
            Message::AppOpened(Err(err)) => {
                self.error = Some(trf("Could not open the app: {error}", &[("error", &err)]))
            }
            Message::OpenSettings => {
                self.close_menus();
                connect::open_settings();
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.account {
            None => {
                return match &self.error {
                    Some(err) => widgets::error_state(err.as_str(), Message::Tick),
                    None => widgets::loading(),
                };
            }
            Some(account @ (AccountState::NotConnected | AccountState::SigningIn { .. })) => {
                return self.connect.view(account, tr("Tasks")).map(Message::Connect);
            }
            Some(AccountState::Connected { .. }) => {}
        }
        responsive(move |size| self.window(layout::of(size))).into()
    }

    /// The whole window in the layout of `class`.
    fn window(&self, class: SizeClass) -> Element<'_, Message> {
        let (switcher, nav) = (Anchor::new(), Anchor::new());
        let sidebar = self.sidebar(&switcher);
        let header = self.header(class, &nav);
        let layer = self.menu.map(|menu| match menu {
            Menu::Switcher => widgets::floating(
                &switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(AppId::TASKS, Message::Switcher),
                Message::CloseMenu,
            ),
            Menu::Nav => widgets::floating(&nav, Placement::Below(Align::Start), self.nav_menu(), Message::CloseMenu),
        });
        shell::shell(class, sidebar, header, self.body(class), None)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(layer)
            .into()
    }

    /// The header: the filter's title (wide) or the navigation menu (medium, narrow), then the
    /// "Completed" switch and refresh.
    fn header<'a>(&'a self, class: SizeClass, nav: &Anchor) -> Element<'a, Message> {
        let name = self.filter_name(&self.filter);
        let open = trf("{n} open", &[("n", &list::open_count(&self.todos, &self.filter, self.today))]);
        let completed = toggler(self.show_completed)
            .label(tr("Completed"))
            .size(style::CHECK_SIZE)
            .text_size(typo::BODY.size)
            .text_line_height(typo::BODY.line_height)
            .style(style::toggler)
            .on_toggle(Message::ShowCompleted);
        let trailing = [completed.into(), widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::Refresh))];
        let icon = filter_icon(&self.filter);
        match class {
            SizeClass::Wide => shell::header_bar([], shell::header_title(icon, name, Some(open)), trailing),
            SizeClass::Medium => {
                let menu = widgets::menu_button(nav, icon, name, Message::ToggleNav);
                let title =
                    row![menu, typo::text(typo::BODY, open).style(theme::text_3).wrapping(text::Wrapping::None)]
                        .spacing(space::S)
                        .align_y(Alignment::Center);
                shell::header_bar([shell::sidebar_toggle(Message::ShowDrawer)], title, trailing)
            }
            SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => shell::header_bar(
                [shell::sidebar_toggle(Message::ShowDrawer)],
                widgets::menu_button(nav, icon, name, Message::ToggleNav),
                trailing,
            ),
        }
    }

    /// The content below the header: the list, and the detail beside it (wide, medium) or
    /// instead of it (narrow).
    fn body(&self, class: SizeClass) -> Element<'_, Message> {
        let Some(detail) = &self.detail else {
            return widgets::content(size::CONTENT_MAX, self.list(class));
        };
        let detail = detail.view().map(Message::Detail);
        let width = match class {
            SizeClass::Wide => size::LIST,
            SizeClass::Medium => size::LIST_MEDIUM,
            SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => return detail,
        };
        row![container(self.list(class)).width(width).height(Fill), rule::vertical(LINE).style(style::rule), detail]
            .into()
    }

    /// Quick add, an error line if there is one, and the grouped list.
    fn list(&self, class: SizeClass) -> Element<'_, Message> {
        let quick_add = text_input(tr("New task …"), &self.quick_add)
            .id(QUICK_ADD)
            .on_input(Message::QuickAddChanged)
            .on_submit(Message::QuickAddSubmit)
            .padding(style::INPUT_PADDING)
            .style(style::text_input);
        let mut top = column![quick_add].spacing(space::S);
        if let Some(err) = &self.error {
            top = top.push(
                typo::text(typo::META, err.clone())
                    .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
            );
        }
        let p = theme::palette(&self.theme());
        let data = ListData {
            todos: &self.todos,
            colors: self.collections.iter().map(|c| (c.id.clone(), collection_color(c.color.as_deref()))).collect(),
            names: self.collections.iter().map(|c| (c.id.clone(), c.name.clone())).collect(),
            filter: &self.filter,
            show_completed: self.show_completed,
            today: self.today,
            selected: self.selected,
            show_list: matches!(class, SizeClass::Wide | SizeClass::Medium) && !matches!(self.filter, Filter::List(_)),
            attention: p.attention_text,
        };
        column![container(top).padding(Padding::new(space::S).bottom(space::XS)), list::view(data)]
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// The filters with their icon and open count, then one entry per task list (ring, count).
    fn nav_entries(&self) -> (Vec<NavEntry>, Vec<NavEntry>) {
        let entry = |filter: Filter, count: bool| NavEntry {
            mark: filter_icon(&filter).map_or(Mark::Icon(Icon::ListTodo), Mark::Icon),
            label: self.filter_name(&filter),
            count: count.then(|| list::open_count(&self.todos, &filter, self.today)),
            active: filter == self.filter,
            filter,
        };
        let views = vec![entry(Filter::Today, true), entry(Filter::Planned, true), entry(Filter::All, false)];
        let lists = self
            .todo_lists()
            .into_iter()
            .map(|collection| {
                let filter = Filter::List(collection.id.clone());
                NavEntry {
                    mark: Mark::Ring(collection_color(collection.color.as_deref())),
                    label: collection.name,
                    count: Some(list::open_count(&self.todos, &filter, self.today)),
                    active: filter == self.filter,
                    filter,
                }
            })
            .collect();
        (views, lists)
    }

    fn sidebar<'a>(&'a self, switcher: &Anchor) -> Element<'a, Message> {
        let (views, lists) = self.nav_entries();
        let row = |e: NavEntry| {
            let trail = e.trail();
            widgets::sidebar_item(e.mark, e.label, trail, e.active, Message::SetFilter(e.filter))
        };
        let mut items: Vec<Element<'a, Message>> = views.into_iter().map(row).collect();
        if !lists.is_empty() {
            items.push(widgets::sidebar_group(tr("Lists"), None));
            items.extend(lists.into_iter().map(row));
        }
        let footer = vec![
            widgets::sidebar_item(
                Icon::Calendar,
                tr("Open Calendar"),
                Trail::Icon(Icon::ArrowUpRight),
                false,
                Message::OpenApp(AppId::CALENDAR.binary),
            ),
            widgets::sidebar_item(Icon::Settings, tr("Settings"), Trail::None, false, Message::OpenSettings),
        ];
        shell::sidebar(shell::app_switcher(AppId::TASKS, switcher, Message::Switcher), items, footer)
    }

    /// The navigation menu of the medium and narrow layouts: the sidebar's rows as a menu.
    fn nav_menu(&self) -> Element<'_, Message> {
        let (views, lists) = self.nav_entries();
        let item = |e: NavEntry| {
            let trail = e.trail();
            MenuItem::new(e.mark, e.label, Message::SetFilter(e.filter)).trail(trail).current(e.active)
        };
        let mut sections = vec![MenuSection::new(views.into_iter().map(item).collect())];
        if !lists.is_empty() {
            sections.push(MenuSection::titled(tr("Lists"), lists.into_iter().map(item).collect()));
        }
        sections.push(MenuSection::new(vec![
            MenuItem::new(Icon::Calendar, tr("Open Calendar"), Message::OpenApp(AppId::CALENDAR.binary))
                .trail(Trail::Icon(Icon::ArrowUpRight)),
            MenuItem::new(Icon::Settings, tr("Settings"), Message::OpenSettings),
        ]));
        widgets::menu(sections)
    }

    fn toggle_menu(&mut self, menu: Menu) {
        self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
    }

    fn close_menus(&mut self) {
        self.menu = None;
        self.drawer = false;
    }

    fn filter_name(&self, filter: &Filter) -> String {
        match filter {
            Filter::Today => tr("Today").into(),
            Filter::Planned => tr("Planned").into(),
            Filter::All => tr("All").into(),
            Filter::List(id) => {
                self.collections.iter().find(|c| &c.id == id).map_or_else(|| tr("List").into(), |c| c.name.clone())
            }
        }
    }

    fn todo_lists(&self) -> Vec<Collection> {
        self.collections.iter().filter(|c| c.kind.has_todos()).cloned().collect()
    }

    /// Where quick add puts a new task: the open list when it is writable, else the first
    /// writable task list.
    fn target_list(&self) -> Option<String> {
        let writable = self.writable_lists();
        match &self.filter {
            Filter::List(id) if writable.iter().any(|c| &c.id == id) => Some(id.clone()),
            _ => writable.first().map(|c| c.id.clone()),
        }
    }

    fn writable_lists(&self) -> Vec<Collection> {
        self.todo_lists().into_iter().filter(|c| c.writable).collect()
    }

    fn connected(&self) -> bool {
        matches!(self.account, Some(AccountState::Connected { .. }))
    }

    fn fetch_account(&self) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { source.account().await.map_err(|e| e.to_string()) }, Message::AccountLoaded)
    }

    fn save(&self, todo: Todo) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { source.save_todo(todo).await.map_err(|e| e.to_string()) }, Message::Saved)
    }

    fn load(&self) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(
            async move {
                let collections = source.collections().await.map_err(|e| e.to_string())?;
                let todos = source.todos().await.map_err(|e| e.to_string())?;
                Ok((collections, todos))
            },
            Message::Loaded,
        )
    }
}

/// A row of the sidebar and the navigation menu.
struct NavEntry {
    mark: Mark,
    label: String,
    filter: Filter,
    /// Open tasks, where the row shows a counter.
    count: Option<usize>,
    active: bool,
}

impl NavEntry {
    fn trail(&self) -> Trail {
        self.count.map_or(Trail::None, Trail::Count)
    }
}

/// The icon of a filter; lists have none (they show their colour as a ring).
fn filter_icon(filter: &Filter) -> Option<Icon> {
    match filter {
        Filter::Today => Some(Icon::Sun),
        Filter::Planned => Some(Icon::CalendarClock),
        Filter::All => Some(Icon::Layers),
        Filter::List(_) => None,
    }
}

/// Asks the core to start or raise `app`. Demo data has no core, so there it does nothing.
async fn open_app(source: Source, app: &'static str) -> Result<(), String> {
    let Source::Core(client) = source else {
        return Ok(());
    };
    match torro_ipc::client::call(&client.socket, &Request::OpenApp { app: app.to_string() }).await {
        Ok(Response::Ok) => Ok(()),
        Ok(Response::Error { message, .. }) => Err(message),
        Ok(other) => Err(format!("unexpected answer: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_app() -> App {
        App::new(Source::from_args(["--demo".to_string()])).0
    }

    #[test]
    fn theme_follows_appearance_and_desktop() {
        let mut app = demo_app();
        let _ = app.update(Message::SystemMode(Mode::Light));
        assert_eq!(app.theme(), theme::light(), "System follows the desktop");
        let _ = app.update(Message::Appearance(Ok(Appearance::Dark)));
        assert_eq!(app.theme(), theme::theme(), "Dark ignores a light desktop");
        let _ = app.update(Message::Appearance(Ok(Appearance::Light)));
        let _ = app.update(Message::SystemMode(Mode::Dark));
        assert_eq!(app.theme(), theme::light(), "Light ignores a dark desktop");
    }

    #[test]
    fn a_failed_settings_request_keeps_the_appearance() {
        let mut app = demo_app();
        let _ = app.update(Message::Appearance(Ok(Appearance::Light)));
        let _ = app.update(Message::Appearance(Err("core is not running".into())));
        assert_eq!(app.theme(), theme::light());
    }

    #[test]
    fn menus_toggle_and_a_filter_closes_them() {
        let mut app = demo_app();
        let _ = app.update(Message::ToggleNav);
        assert_eq!(app.menu, Some(Menu::Nav));
        let _ = app.update(Message::ToggleNav);
        assert_eq!(app.menu, None, "a second click closes the menu");
        let _ = app.update(Message::Switcher(Switcher::Toggle));
        assert_eq!(app.menu, Some(Menu::Switcher));
        let _ = app.update(Message::ShowDrawer);
        assert!(app.drawer && app.menu.is_none(), "the drawer replaces an open menu");
        let _ = app.update(Message::SetFilter(Filter::Planned));
        assert!(!app.drawer && app.menu.is_none());
        assert_eq!(app.filter, Filter::Planned);
    }

    #[test]
    fn escape_closes_menu_and_drawer() {
        let mut app = demo_app();
        let _ = app.update(Message::ToggleNav);
        let _ = app.update(Message::CloseMenu);
        assert_eq!(app.menu, None);
        let _ = app.update(Message::ShowDrawer);
        let _ = app.update(Message::CloseMenu);
        assert!(!app.drawer);
    }

    #[test]
    fn opening_an_app_in_demo_needs_no_core() {
        let source = Source::from_args(["--demo".to_string()]);
        let opened = iced::futures::executor::block_on(open_app(source, AppId::CALENDAR.binary));
        assert_eq!(opened, Ok(()));
    }

    #[test]
    fn filters_have_icons_and_lists_rings() {
        assert_eq!(filter_icon(&Filter::Today), Some(Icon::Sun));
        assert_eq!(filter_icon(&Filter::List("work".into())), None);
    }

    #[test]
    fn without_a_preference_the_system_appearance_is_dark() {
        assert_eq!(demo_app().theme(), theme::theme());
    }
}
