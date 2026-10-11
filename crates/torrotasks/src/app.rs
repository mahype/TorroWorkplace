//! The tasks window: state, loading, quick add, saving. The list (`list.rs`) and the detail
//! panel (`detail.rs`) only render and report clicks as [`Message`]s.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{Local, NaiveDate};
use iced::widget::{button, column, container, row, scrollable, text, text_input, toggler};
use iced::{Element, Length, Subscription, Task};
use torro_core::i18n::{tr, trf};
use torro_gui::connect::{self, Connect};
use torro_gui::source::Source;
use torro_gui::widgets::collection_color;
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
        };
        let task = app.fetch_account();
        (app, task)
    }

    pub fn title(&self) -> String {
        trf("Tasks · {filter}", &[("filter", &self.filter_name(&self.filter))])
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let every = if self.connected() { Duration::from_secs(60) } else { Duration::from_secs(2) };
        iced::time::every(every).map(|_| Message::Tick)
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
                if !self.connected() {
                    return self.fetch_account();
                }
                if self.detail.is_none() {
                    return self.load();
                }
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.account {
            None => return container(text(self.error.as_deref().unwrap_or(tr("Loading …")))).padding(24).into(),
            Some(account @ (AccountState::NotConnected | AccountState::SigningIn { .. })) => {
                return self.connect.view(account, tr("Tasks")).map(Message::Connect);
            }
            Some(AccountState::Connected { .. }) => {}
        }
        let colors: HashMap<String, iced::Color> =
            self.collections.iter().map(|c| (c.id.clone(), collection_color(c.color.as_deref()))).collect();
        let header = row![
            text(self.filter_name(&self.filter)).size(20),
            iced::widget::Space::new().width(Length::Fill),
            toggler(self.show_completed).label(tr("Completed")).on_toggle(Message::ShowCompleted),
            button(text("⟳")).style(button::secondary).on_press(Message::Refresh),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center);
        let quick_add = text_input(tr("New task …"), &self.quick_add)
            .on_input(Message::QuickAddChanged)
            .on_submit(Message::QuickAddSubmit)
            .padding(8);
        let data = ListData {
            todos: &self.todos,
            colors,
            filter: &self.filter,
            show_completed: self.show_completed,
            today: self.today,
            selected: self.selected,
        };
        let mut center = column![header, quick_add, list::view(data)].spacing(10).padding(12).width(Length::Fill);
        if let Some(err) = &self.error {
            center = center.push(text(err.clone()).size(13));
        }
        let mut body = row![self.sidebar(), center].height(Length::Fill);
        if let Some(detail) = &self.detail {
            body = body.push(container(detail.view().map(Message::Detail)).width(340).height(Length::Fill));
        }
        body.into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let entry = |filter: Filter, label: String| {
            let style = if filter == self.filter { button::primary } else { button::text };
            button(text(label)).style(style).width(Length::Fill).on_press(Message::SetFilter(filter))
        };
        let mut list = column![
            entry(Filter::Today, tr("Today").into()),
            entry(Filter::Planned, tr("Planned").into()),
            entry(Filter::All, tr("All").into()),
            text(tr("Lists")).size(13),
        ]
        .spacing(4)
        .padding(10);
        for collection in self.todo_lists() {
            let open = self.todos.iter().filter(|t| t.collection == collection.id && !t.completed).count();
            list = list.push(entry(Filter::List(collection.id.clone()), format!("{}  {open}", collection.name)));
        }
        container(scrollable(list)).width(200).height(Length::Fill).into()
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
