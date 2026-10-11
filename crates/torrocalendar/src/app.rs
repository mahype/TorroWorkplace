//! The calendar window: state, navigation, loading and saving. The views (`views/`) and the
//! editor (`editor.rs`) only render and report clicks as [`Message`]s.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{Datelike, Duration as Days, Local, NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::{button, column, container, opaque, row, scrollable, stack, text};
use iced::{Element, Length, Subscription, Task};
use torro_core::i18n::{tr, trf};
use torro_gui::connect::{self, Connect};
use torro_gui::datetime::{format_long_date, month_name, to_utc, week_start};
use torro_gui::source::Source;
use torro_gui::widgets::collection_color;
use torro_pim::{AccountState, Collection, Event, PimSource};

use crate::editor::{self, Editor};
use crate::views::{self, Item, ViewData};

/// The four views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalView {
    Day,
    Week,
    Month,
    Agenda,
}

impl CalView {
    const ALL: [CalView; 4] = [CalView::Day, CalView::Week, CalView::Month, CalView::Agenda];

    fn label(self) -> &'static str {
        match self {
            CalView::Day => tr("Day"),
            CalView::Week => tr("Week"),
            CalView::Month => tr("Month"),
            CalView::Agenda => tr("Agenda"),
        }
    }
}

/// Days the agenda shows from its anchor on.
const AGENDA_DAYS: i64 = 30;

pub struct App {
    source: Source,
    now: NaiveDateTime,
    view: CalView,
    /// The date the view is built around.
    anchor: NaiveDate,
    account: Option<AccountState>,
    connect: Connect,
    collections: Vec<Collection>,
    hidden: HashSet<String>,
    events: Vec<Event>,
    loading: bool,
    error: Option<String>,
    editor: Option<Editor>,
}

#[derive(Debug, Clone)]
pub enum Message {
    AccountLoaded(Result<AccountState, String>),
    Connect(connect::Message),
    ConnectStarted(Result<String, String>),
    Loaded(Result<(Vec<Collection>, Vec<Event>), String>),
    SetView(CalView),
    Prev,
    Next,
    Today,
    Refresh,
    ToggleCollection(String),
    /// From a view: create an event on `date`, at `time` (`None`: all-day or no slot).
    NewEvent {
        date: NaiveDate,
        time: Option<NaiveTime>,
    },
    /// From a view: open `App::events[index]`.
    OpenEvent(usize),
    /// From the month view: switch to the day view of `date`.
    ShowDay(NaiveDate),
    Editor(editor::Message),
    Saved(Result<Event, String>),
    Deleted(Result<(), String>),
    Tick,
}

impl App {
    pub fn new(source: Source) -> (App, Task<Message>) {
        let now = Local::now().naive_local();
        let app = App {
            source,
            now,
            view: CalView::Week,
            anchor: now.date(),
            account: None,
            connect: Connect::default(),
            collections: Vec::new(),
            hidden: HashSet::new(),
            events: Vec::new(),
            loading: true,
            error: None,
            editor: None,
        };
        let task = app.fetch_account().chain(views::week::scroll_to_morning());
        (app, task)
    }

    pub fn title(&self) -> String {
        trf("Calendar · {heading}", &[("heading", &self.heading())])
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // Faster while waiting for a sign-in in the browser.
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
            Message::AccountLoaded(Err(err)) => {
                self.loading = false;
                self.error = Some(err);
            }
            Message::Connect(message) => {
                if let connect::Action::Connect(server) = self.connect.update(message) {
                    let source = self.source.clone();
                    return Task::perform(
                        async move { source.connect_nextcloud(server).await.map_err(|e| e.to_string()) },
                        Message::ConnectStarted,
                    );
                }
            }
            Message::ConnectStarted(result) => {
                self.connect.busy = false;
                match result {
                    Ok(url) => self.connect.status = Some(trf("Sign in in your browser … ({url})", &[("url", &url)])),
                    Err(err) => self.connect.status = Some(err),
                }
                return self.fetch_account();
            }
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok((collections, events)) => {
                        self.collections = collections;
                        self.events = events;
                        self.error = None;
                    }
                    Err(err) => self.error = Some(err),
                }
            }
            Message::SetView(view) => {
                self.view = view;
                return self.load().chain(self.scroll_to_morning());
            }
            Message::Prev => {
                self.anchor = self.step(-1);
                return self.load().chain(self.scroll_to_morning());
            }
            Message::Next => {
                self.anchor = self.step(1);
                return self.load().chain(self.scroll_to_morning());
            }
            Message::Today => {
                self.anchor = self.now.date();
                return self.load().chain(self.scroll_to_morning());
            }
            Message::Refresh => {
                let source = self.source.clone();
                let sync =
                    Task::perform(async move { source.sync().await.map_err(|e| e.to_string()) }, |_| Message::Tick);
                return sync.chain(self.load());
            }
            Message::ToggleCollection(id) => {
                if !self.hidden.remove(&id) {
                    self.hidden.insert(id);
                }
            }
            Message::NewEvent { date, time } => {
                self.editor = Some(Editor::new_event(&self.writable_calendars(), date, time));
            }
            Message::OpenEvent(index) => {
                if let Some(event) = self.events.get(index) {
                    self.editor = Some(Editor::edit(event, &self.writable_calendars()));
                }
            }
            Message::ShowDay(date) => {
                self.anchor = date;
                self.view = CalView::Day;
                return self.load();
            }
            Message::Editor(message) => {
                let Some(editor) = self.editor.as_mut() else { return Task::none() };
                match editor.update(message) {
                    editor::Action::None => {}
                    editor::Action::Close => self.editor = None,
                    editor::Action::Save(event) => {
                        let source = self.source.clone();
                        return Task::perform(
                            async move { source.save_event(event).await.map_err(|e| e.to_string()) },
                            Message::Saved,
                        );
                    }
                    editor::Action::Delete(event) => {
                        let source = self.source.clone();
                        return Task::perform(
                            async move { source.delete_event(event).await.map_err(|e| e.to_string()) },
                            Message::Deleted,
                        );
                    }
                }
            }
            Message::Saved(result) => match result {
                Ok(_) => {
                    self.editor = None;
                    return self.load();
                }
                Err(err) => self.error = Some(trf("Save failed: {error}", &[("error", &err)])),
            },
            Message::Deleted(result) => match result {
                Ok(()) => {
                    self.editor = None;
                    return self.load();
                }
                Err(err) => self.error = Some(trf("Delete failed: {error}", &[("error", &err)])),
            },
            Message::Tick => {
                self.now = Local::now().naive_local();
                if !self.connected() {
                    return self.fetch_account();
                }
                if self.editor.is_none() {
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
                return self.connect.view(account, tr("Calendar")).map(Message::Connect);
            }
            Some(AccountState::Connected { .. }) => {}
        }
        let main = column![self.toolbar(), row![self.sidebar(), self.current_view()].height(Length::Fill)];
        let main = match &self.error {
            Some(err) => main.push(container(text(err.clone()).size(13)).padding([4, 12])),
            None => main,
        };
        match &self.editor {
            Some(editor) => {
                stack![main, opaque(container(editor.view().map(Message::Editor)).center(Length::Fill))].into()
            }
            None => main.into(),
        }
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let mut views = row![].spacing(4);
        for view in CalView::ALL {
            let label = button(text(view.label())).on_press(Message::SetView(view));
            views = views.push(if view == self.view {
                label.style(button::primary)
            } else {
                label.style(button::secondary)
            });
        }
        row![
            button(text(tr("Today"))).style(button::secondary).on_press(Message::Today),
            button(text("‹")).style(button::secondary).on_press(Message::Prev),
            button(text("›")).style(button::secondary).on_press(Message::Next),
            text(self.heading()).size(18),
            iced::widget::Space::new().width(Length::Fill),
            views,
            button(text(tr("+ Event")))
                .style(button::primary)
                .on_press(Message::NewEvent { date: self.anchor, time: Some(NaiveTime::MIN + Days::hours(9)) }),
            button(text("⟳")).style(button::secondary).on_press(Message::Refresh),
        ]
        .spacing(8)
        .padding(10)
        .align_y(iced::Alignment::Center)
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut list = column![text(tr("Calendars")).size(13)].spacing(6).padding(10);
        for collection in self.collections.iter().filter(|c| c.kind.has_events()) {
            let shown = !self.hidden.contains(&collection.id);
            let id = collection.id.clone();
            list = list.push(
                iced::widget::checkbox(shown)
                    .label(collection.name.clone())
                    .on_toggle(move |_| Message::ToggleCollection(id.clone())),
            );
        }
        container(scrollable(list)).width(200).height(Length::Fill).into()
    }

    fn current_view(&self) -> Element<'_, Message> {
        let colors: HashMap<&str, iced::Color> =
            self.collections.iter().map(|c| (c.id.as_str(), collection_color(c.color.as_deref()))).collect();
        let items = self
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| !self.hidden.contains(&e.collection))
            .map(|(index, event)| Item {
                index,
                event,
                color: colors.get(event.collection.as_str()).copied().unwrap_or(collection_color(None)),
            })
            .collect();
        let (start, end) = self.range();
        let data = ViewData { items, start, end, anchor: self.anchor, now: self.now };
        match self.view {
            CalView::Day | CalView::Week => views::week::view(data),
            CalView::Month => views::month::view(data),
            CalView::Agenda => views::agenda::view(data),
        }
    }

    /// First and last+1 day the current view shows.
    fn range(&self) -> (NaiveDate, NaiveDate) {
        match self.view {
            CalView::Day => (self.anchor, self.anchor + Days::days(1)),
            CalView::Week => {
                let start = week_start(self.anchor);
                (start, start + Days::days(7))
            }
            CalView::Month => {
                let first = self.anchor.with_day(1).unwrap_or(self.anchor);
                let start = week_start(first);
                (start, start + Days::days(42))
            }
            CalView::Agenda => (self.anchor, self.anchor + Days::days(AGENDA_DAYS)),
        }
    }

    /// The anchor one view-length back (`-1`) or ahead (`1`).
    fn step(&self, direction: i64) -> NaiveDate {
        match self.view {
            CalView::Day => self.anchor + Days::days(direction),
            CalView::Week => self.anchor + Days::days(7 * direction),
            CalView::Month => {
                let months = self.anchor.year() * 12 + self.anchor.month0() as i32 + direction as i32;
                NaiveDate::from_ymd_opt(months.div_euclid(12), months.rem_euclid(12) as u32 + 1, 1)
                    .unwrap_or(self.anchor)
            }
            CalView::Agenda => self.anchor + Days::days(AGENDA_DAYS * direction),
        }
    }

    fn heading(&self) -> String {
        match self.view {
            CalView::Day => format_long_date(self.anchor),
            CalView::Week => {
                let start = week_start(self.anchor);
                trf(
                    "Week {week} · {month} {year}",
                    &[
                        ("week", &start.iso_week().week()),
                        ("month", &month_name(start.month())),
                        ("year", &start.year()),
                    ],
                )
            }
            CalView::Month => format!("{} {}", month_name(self.anchor.month()), self.anchor.year()),
            CalView::Agenda => trf("From {date}", &[("date", &format_long_date(self.anchor))]),
        }
    }

    /// Day and week open at 07:00 instead of midnight.
    fn scroll_to_morning(&self) -> Task<Message> {
        match self.view {
            CalView::Day | CalView::Week => views::week::scroll_to_morning(),
            CalView::Month | CalView::Agenda => Task::none(),
        }
    }

    fn connected(&self) -> bool {
        matches!(self.account, Some(AccountState::Connected { .. }))
    }

    fn writable_calendars(&self) -> Vec<Collection> {
        self.collections.iter().filter(|c| c.kind.has_events()).cloned().collect()
    }

    fn fetch_account(&self) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { source.account().await.map_err(|e| e.to_string()) }, Message::AccountLoaded)
    }

    fn load(&mut self) -> Task<Message> {
        self.loading = true;
        let (start, end) = self.range();
        let (start, end) = (to_utc(start.and_time(NaiveTime::MIN)), to_utc(end.and_time(NaiveTime::MIN)));
        let source = self.source.clone();
        Task::perform(
            async move {
                let collections = source.collections().await.map_err(|e| e.to_string())?;
                let events = source.events(start, end).await.map_err(|e| e.to_string())?;
                Ok((collections, events))
            },
            Message::Loaded,
        )
    }
}
