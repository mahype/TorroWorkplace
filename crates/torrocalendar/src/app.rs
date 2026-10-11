//! The calendar window: state, navigation, loading and saving, and the Linear-style frame around
//! the views (sidebar with the small month, header bar, view menu). The views (`views/`) and the
//! editor (`editor.rs`) only render and report clicks as [`Message`]s.
//!
//! The window size picks the layout ([`layout::of`], design skill section 6): mini and strip are
//! glances without any frame (`views::glance`); narrow shows a week as one day under the week
//! strip and a month as the agenda, and opens the editor full-window; medium and wide show the
//! chosen view. State (view, date, selection, open editor) survives every change of size.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{Datelike, Duration as Days, Local, NaiveDate, NaiveDateTime, NaiveTime};
use iced::theme::Mode;
use iced::widget::{column, row};
use iced::{Alignment, Color, Element, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::connect::{self, Connect};
use torro_gui::datetime::{format_long_date, month_name, to_utc, week_start};
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, Switcher};
use torro_gui::source::Source;
use torro_gui::style;
use torro_gui::theme::{self, Appearance, space};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail, collection_color, modal};
use torro_pim::{AccountState, Collection, Event, PimSource};

use crate::editor::{self, Editor};
use crate::views::{self, Item, ViewData, glance, mini_month, short_date, week_strip};

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

    /// The icon in the sidebar and the view menu (`calendar.html`).
    fn icon(self) -> Icon {
        match self {
            CalView::Day => Icon::CalendarDays,
            CalView::Week => Icon::CalendarRange,
            CalView::Month => Icon::Calendar,
            CalView::Agenda => Icon::List,
        }
    }

    /// The key that switches to the view ([`shortcut`]), shown as a key hint.
    fn key(self) -> &'static str {
        match self {
            CalView::Day => "D",
            CalView::Week => "W",
            CalView::Month => "M",
            CalView::Agenda => "A",
        }
    }
}

/// The view the narrow layout shows for the chosen `view`: a week as one day (under the week
/// strip), a month as the agenda; seven columns or a month grid do not fit 360–600 px. Every other
/// layout shows the chosen view.
fn shown_in(view: CalView, class: SizeClass) -> CalView {
    match (class, view) {
        (SizeClass::Narrow, CalView::Week) => CalView::Day,
        (SizeClass::Narrow, CalView::Month) => CalView::Agenda,
        _ => view,
    }
}

/// Mini and strip: displays without header, sidebar, menus or dialogs.
fn is_glance(class: SizeClass) -> bool {
    matches!(class, SizeClass::Mini | SizeClass::Strip)
}

/// The menus that open over the window; at most one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// The app switcher at the top of the sidebar.
    Switcher,
    /// "Week ▾" in the medium and narrow header: the views and the calendars.
    View,
}

/// Days the agenda shows from its anchor on.
const AGENDA_DAYS: i64 = 30;

pub struct App {
    source: Source,
    now: NaiveDateTime,
    /// The chosen view. The narrow layout may show another one ([`App::shown`]); this one comes
    /// back when the window is wider again.
    view: CalView,
    /// The date the view is built around.
    anchor: NaiveDate,
    account: Option<AccountState>,
    connect: Connect,
    collections: Vec<Collection>,
    hidden: HashSet<String>,
    events: Vec<Event>,
    loading: bool,
    /// Events have been loaded once; until then the body shows the loading state.
    loaded: bool,
    error: Option<String>,
    /// The open menu, if any.
    menu: Option<Menu>,
    /// The sidebar drawer of the medium and narrow layouts is open.
    drawer: bool,
    /// The first of the month the small month in the sidebar shows. Follows the anchor, but its
    /// own arrows browse it without moving the main view.
    mini_month: NaiveDate,
    /// The window size, for the size class ([`layout::of`]). Kept from the window events rather
    /// than read in `iced::widget::responsive`, so `update` sees a change of layout and can
    /// scroll the time grid back to the morning (the new widget tree loses its scroll position).
    size: Size,
    editor: Option<Editor>,
    /// The time grid has been scrolled to the morning once; it only exists after the first load.
    scrolled: bool,
    appearance: Appearance,
    system_mode: Mode,
}

#[derive(Debug, Clone)]
pub enum Message {
    AccountLoaded(Result<AccountState, String>),
    Connect(connect::Message),
    Loaded(Result<(Vec<Collection>, Vec<Event>), String>),
    SetView(CalView),
    Prev,
    Next,
    Today,
    Refresh,
    /// The error state's "Retry": ask for the account again, or reload the events.
    Retry,
    ToggleCollection(String),
    /// The app switcher (top of the sidebar).
    Switcher(Switcher),
    /// Open `Menu`, or close it when it is open.
    ToggleMenu(Menu),
    /// Close the open menu and the drawer (click beside them, Escape).
    CloseMenu,
    /// Open or close the sidebar drawer (medium and narrow layouts).
    ToggleDrawer,
    /// From the small month: show `date` in the current view.
    GoTo(NaiveDate),
    /// The small month's arrows: browse it by this many months.
    MiniMonth(i32),
    /// "Settings" in the sidebar: open the control centre.
    OpenSettings,
    /// The core answered `Request::OpenApp`.
    AppOpened(Result<(), String>),
    /// The window has this size (at start and after every resize).
    Resized(Size),
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
    /// Keyboard: a new event on the shown day at 09:00.
    NewEventHere,
    Saved(Result<Event, String>),
    Deleted(Result<(), String>),
    Tick,
    /// The desktop's light/dark preference (once at start, then whenever it changes).
    SystemMode(Mode),
    /// The appearance stored in the suite settings; an error keeps the one we have.
    Appearance(Result<Appearance, String>),
}

/// Keyboard shortcuts while no dialog is open: N new event, T today, ←/→ back/forward,
/// D/W/M/A the views. Keys a text input already used are ignored.
fn shortcut(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event as Key, Key as Name, key::Named};
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(Key::KeyPressed { key, modifiers, .. }) = event else { return None };
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key.as_ref() {
        Name::Named(Named::ArrowLeft) => Some(Message::Prev),
        Name::Named(Named::ArrowRight) => Some(Message::Next),
        Name::Character(c) => match c {
            "n" => Some(Message::NewEventHere),
            "t" => Some(Message::Today),
            "d" => Some(Message::SetView(CalView::Day)),
            "w" => Some(Message::SetView(CalView::Week)),
            "m" => Some(Message::SetView(CalView::Month)),
            "a" => Some(Message::SetView(CalView::Agenda)),
            _ => None,
        },
        _ => None,
    }
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
            connect: Connect,
            collections: Vec::new(),
            hidden: HashSet::new(),
            events: Vec::new(),
            loading: true,
            loaded: false,
            error: None,
            menu: None,
            drawer: false,
            mini_month: mini_month::shift(now.date(), 0),
            size: torro_gui::window(AppId::CALENDAR).size,
            editor: None,
            scrolled: false,
            appearance: Appearance::System,
            system_mode: Mode::None,
        };
        let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
        let task = Task::batch([
            app.fetch_account(),
            app.fetch_appearance(),
            iced::system::theme().map(Message::SystemMode),
            size,
        ]);
        (app, task)
    }

    pub fn title(&self) -> String {
        trf("Calendar · {heading}", &[("heading", &self.heading())])
    }

    pub fn subscription(&self) -> Subscription<Message> {
        // Faster while waiting for a sign-in in the browser.
        let every = if self.connected() { Duration::from_secs(60) } else { Duration::from_secs(2) };
        let tick = iced::time::every(every).map(|_| Message::Tick);
        let system = iced::system::theme_changes().map(Message::SystemMode);
        let keys = match self.editor {
            // Escape closes the editor.
            Some(_) => editor::subscription().map(Message::Editor),
            None => iced::event::listen_with(shortcut),
        };
        let resize = iced::window::resize_events().map(|(_, size)| Message::Resized(size));
        let mut all = vec![tick, system, resize];
        // Mini and strip are displays: no keys, nothing open to close.
        if !is_glance(self.class()) {
            all.push(keys);
            if self.menu.is_some() || self.drawer {
                all.push(widgets::escape().map(|()| Message::CloseMenu));
            }
        }
        Subscription::batch(all)
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
            Message::AccountLoaded(Err(err)) => {
                self.loading = false;
                self.error = Some(load_failed(&err));
            }
            Message::Connect(message) => match self.connect.update(message) {
                // The account is set up in the control centre; the polling in `Tick` picks it up.
                connect::Action::OpenSettings => connect::open_settings(),
            },
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok((collections, events)) => {
                        self.collections = collections;
                        self.events = events;
                        self.loaded = true;
                        self.error = None;
                        if !self.scrolled {
                            self.scrolled = true;
                            return self.scroll_to_morning();
                        }
                    }
                    Err(err) => self.error = Some(load_failed(&err)),
                }
            }
            Message::SetView(view) => {
                self.view = view;
                self.close_menus();
                return self.load().chain(self.scroll_to_morning());
            }
            Message::Prev => return self.go_to(self.step(-1)),
            Message::Next => return self.go_to(self.step(1)),
            Message::Today => return self.go_to(self.now.date()),
            Message::GoTo(date) => {
                self.close_menus();
                return self.go_to(date);
            }
            Message::MiniMonth(months) => self.mini_month = mini_month::shift(self.mini_month, months),
            Message::Retry => {
                self.error = None;
                if !self.connected() {
                    self.account = None;
                    return self.fetch_account();
                }
                return self.load();
            }
            Message::Switcher(Switcher::Toggle) => return self.update(Message::ToggleMenu(Menu::Switcher)),
            Message::Switcher(Switcher::OpenApp(binary)) => {
                self.close_menus();
                let source = self.source.clone();
                return Task::perform(async move { source.open_app(binary).await }, Message::AppOpened);
            }
            // The calendar has no search yet; the button only closes what is open.
            Message::Switcher(Switcher::Search) => self.close_menus(),
            Message::Switcher(Switcher::New) => {
                self.close_menus();
                return self.update(Message::NewEventHere);
            }
            Message::ToggleMenu(menu) => {
                self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
            }
            Message::CloseMenu => self.close_menus(),
            Message::ToggleDrawer => {
                self.menu = None;
                self.drawer = !self.drawer;
            }
            Message::OpenSettings => {
                self.close_menus();
                connect::open_settings();
            }
            // Nothing to show in the window: the other app either comes up or it does not.
            Message::AppOpened(Err(err)) => eprintln!("torrocalendar: cannot open the app: {err}"),
            Message::AppOpened(Ok(())) => {}
            Message::Resized(size) => {
                let before = self.class();
                self.size = size;
                // Another layout may need other days (the narrow day loads its week for the strip,
                // mini and strip load from today on), and it builds a different widget tree, in
                // which Iced drops the time grid's scroll position: load again and open the grid at
                // the morning instead of midnight.
                if self.class() != before && self.loaded {
                    return self.load().chain(self.scroll_to_morning());
                }
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
            Message::NewEventHere => {
                let time = Some(NaiveTime::MIN + Days::hours(9));
                self.editor = Some(Editor::new_event(&self.writable_calendars(), self.anchor, time));
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
                self.view = CalView::Day;
                return self.go_to(date);
            }
            Message::Editor(message) => {
                let Some(editor) = self.editor.as_mut() else { return Task::none() };
                match editor.update(message) {
                    editor::Action::None => {}
                    editor::Action::Close => self.editor = None,
                    // The first field with a problem, after a press on Save.
                    editor::Action::Focus(id) => return iced::widget::operation::focus(id),
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
                let appearance = self.fetch_appearance();
                if !self.connected() {
                    return Task::batch([appearance, self.fetch_account()]);
                }
                if self.editor.is_none() {
                    return Task::batch([appearance, self.load()]);
                }
                return appearance;
            }
            Message::SystemMode(mode) => self.system_mode = mode,
            Message::Appearance(Ok(appearance)) => self.appearance = appearance,
            // The core may be starting up; keep the colours we have.
            Message::Appearance(Err(_)) => {}
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.account {
            None => {
                return match &self.error {
                    Some(err) => widgets::error_state(err.as_str(), Message::Retry),
                    None => widgets::loading(),
                };
            }
            Some(account @ (AccountState::NotConnected | AccountState::SigningIn { .. })) => {
                return self.connect.view(account, tr("Calendar")).map(Message::Connect);
            }
            Some(AccountState::Connected { .. }) => {}
        }
        let class = self.class();
        if is_glance(class) {
            // An open editor stays in the state and comes back with a larger window.
            return self.glance(class);
        }
        let window = self.window(class);
        match &self.editor {
            Some(editor) if class == SizeClass::Narrow => editor.view_full_window().map(Message::Editor),
            Some(editor) => modal(
                window,
                editor.view().map(Message::Editor),
                editor::WIDTH,
                Message::Editor(editor::Message::Cancel),
            ),
            None => window,
        }
    }

    /// Mini or strip: the error and the loading state as in every layout, then the glance at
    /// today and the days after it (`views::glance`).
    fn glance(&self, class: SizeClass) -> Element<'_, Message> {
        if let Some(err) = &self.error {
            return widgets::error_state(err.as_str(), Message::Retry);
        }
        if !self.loaded {
            return widgets::loading();
        }
        let (start, end) = self.load_range();
        let data = ViewData { items: self.items(), start, end, anchor: self.now.date(), now: self.now };
        match class {
            SizeClass::Strip => glance::strip(&data, self.size),
            _ => glance::mini(&data, self.size),
        }
    }

    /// The whole window in the layout for `class`: sidebar, header and the current view in the
    /// shell, the drawer and the open menu above it.
    fn window(&self, class: SizeClass) -> Element<'_, Message> {
        let (switcher, views) = (Anchor::new(), Anchor::new());
        let layer = self.menu.and_then(|menu| match menu {
            Menu::Switcher => Some(widgets::floating(
                &switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(AppId::CALENDAR, Message::Switcher),
                Message::CloseMenu,
            )),
            // Only the medium and narrow header have the "Week ▾" button the menu hangs from.
            Menu::View => (!shell::has_sidebar(class))
                .then(|| widgets::floating(&views, Placement::Below(Align::End), self.view_menu(), Message::CloseMenu)),
        });
        shell::shell(class, self.sidebar(class, &switcher), self.header(class, &views), self.body(), None)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(layer)
            .into()
    }

    /// The header bar (`calendar.html`, `.header`):
    ///
    /// - **Wide:** "October 2026 Week 41", "Today", ‹ ›; at the right the views as a segmented
    ///   control, refresh and "New event".
    /// - **Medium:** the sidebar toggle in front; the views (and calendars) in "Week ▾".
    /// - **Narrow:** without the note and "Today" (the small month in the drawer and the T key go
    ///   to today), the day as "Wed, 7 Oct", and "New event" reduced to "+". "Day ▾" names the
    ///   view shown ([`App::shown`]).
    fn header<'a>(&'a self, class: SizeClass, views: &Anchor) -> Element<'a, Message> {
        let (title, note) = self.title_parts();
        let prev = widgets::icon_button(Icon::ChevronLeft, tr("Previous"), Some(Message::Prev));
        let next = widgets::icon_button(Icon::ChevronRight, tr("Next"), Some(Message::Next));
        let refresh = widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::Refresh));
        let new_event = Message::NewEvent { date: self.anchor, time: Some(NaiveTime::MIN + Days::hours(9)) };
        let select = || widgets::select_button(views, self.shown().label(), Message::ToggleMenu(Menu::View));
        if class == SizeClass::Wide || class == SizeClass::Medium {
            let today = widgets::text_button(None, tr("Today"), style::button::ghost, Some(Message::Today));
            let title = row![
                shell::header_title(None, title, note),
                row![today, prev, next].spacing(space::XS).align_y(Alignment::Center)
            ]
            .spacing(space::L)
            .align_y(Alignment::Center);
            let primary =
                widgets::text_button(Some(Icon::Plus), tr("New event"), style::button::primary, Some(new_event));
            if class == SizeClass::Wide {
                let segments = CalView::ALL.map(|view| (view.label(), Message::SetView(view)));
                let active = CalView::ALL.iter().position(|view| *view == self.view).unwrap_or_default();
                shell::header_bar([], title, [widgets::segmented(segments, active), refresh, primary])
            } else {
                shell::header_bar([shell::sidebar_toggle(Message::ToggleDrawer)], title, [select(), refresh, primary])
            }
        } else {
            // The short date: the long one ("Monday, 12 October 2026") squeezes the buttons.
            let title = if self.shown() == CalView::Day { short_date(self.anchor) } else { title };
            let title = row![shell::header_title(None, title, None), row![prev, next].spacing(space::XS)]
                .spacing(space::S)
                .align_y(Alignment::Center);
            let primary = widgets::compact_button(Icon::Plus, tr("New event"), style::button::primary, Some(new_event));
            shell::header_bar([shell::sidebar_toggle(Message::ToggleDrawer)], title, [select(), refresh, primary])
        }
    }

    /// The sidebar (`calendar.html`, `.sidebar`): app switcher, small month, the views with their
    /// keys, the calendars with their colour, visibility and number of loaded events, and at the
    /// bottom "Open Tasks ↗" and "Settings".
    ///
    /// As a drawer (medium, narrow) it leaves out the views: they are in the header's "Week ▾"
    /// menu, and without them the drawer still fits a window 600 high.
    fn sidebar<'a>(&'a self, class: SizeClass, switcher: &Anchor) -> Element<'a, Message> {
        let shown = matches!(self.shown(), CalView::Day | CalView::Week).then(|| self.range());
        let mut items = vec![mini_month::view(self.mini_month, shown, self.now.date())];
        if shell::has_sidebar(class) {
            items.push(widgets::sidebar_group(tr("Views"), None));
            items.extend(CalView::ALL.map(|view| {
                widgets::sidebar_item(
                    view.icon(),
                    view.label(),
                    Trail::Kbd(view.key()),
                    view == self.view,
                    Message::SetView(view),
                )
            }));
        }
        let calendars = self.calendars();
        if !calendars.is_empty() {
            items.push(widgets::sidebar_group(tr("Calendars"), None));
            items.extend(calendars.into_iter().map(|(calendar, colour, count)| {
                widgets::sidebar_item(
                    Mark::Check(colour, !self.hidden.contains(&calendar.id)),
                    calendar.name.as_str(),
                    Trail::Count(count),
                    false,
                    Message::ToggleCollection(calendar.id.clone()),
                )
            }));
        }
        let footer = vec![
            widgets::sidebar_item(
                Icon::SquareCheck,
                tr("Open Tasks"),
                Trail::Icon(Icon::ArrowUpRight),
                false,
                Message::Switcher(Switcher::OpenApp(AppId::TASKS.binary)),
            ),
            widgets::sidebar_item(Icon::Settings, tr("Settings"), Trail::None, false, Message::OpenSettings),
        ];
        shell::sidebar(shell::app_switcher(AppId::CALENDAR, switcher, Message::Switcher), items, footer)
    }

    /// The menu under "Week ▾": the views with their keys, then the calendars with a check box
    /// that shows and hides them (the menu stays open for that).
    fn view_menu(&self) -> Element<'_, Message> {
        let views = CalView::ALL
            .map(|view| {
                MenuItem::new(view.icon(), view.label(), Message::SetView(view))
                    .trail(Trail::Kbd(view.key()))
                    .current(view == self.shown())
            })
            .to_vec();
        let mut sections = vec![MenuSection::titled(tr("View"), views)];
        let calendars: Vec<_> = self
            .calendars()
            .into_iter()
            .map(|(calendar, colour, count)| {
                MenuItem::new(
                    Mark::Check(colour, !self.hidden.contains(&calendar.id)),
                    calendar.name.as_str(),
                    Message::ToggleCollection(calendar.id.clone()),
                )
                .trail(Trail::Count(count))
            })
            .collect();
        if !calendars.is_empty() {
            sections.push(MenuSection::titled(tr("Calendars"), calendars));
        }
        widgets::menu(sections)
    }

    /// The event calendars with their colour and how many of the loaded events they hold.
    fn calendars(&self) -> Vec<(&Collection, Color, usize)> {
        self.collections
            .iter()
            .filter(|c| c.kind.has_events())
            .map(|c| {
                let count = self.events.iter().filter(|e| e.collection == c.id).count();
                (c, collection_color(c.color.as_deref()), count)
            })
            .collect()
    }

    /// The content under the header: the error with "Retry", the calm loading state before the
    /// first events arrive, then the current view.
    fn body(&self) -> Element<'_, Message> {
        if let Some(err) = &self.error {
            return widgets::error_state(err.as_str(), Message::Retry);
        }
        if !self.loaded {
            return widgets::loading();
        }
        self.current_view()
    }

    fn current_view(&self) -> Element<'_, Message> {
        let (start, end) = self.range();
        let data = ViewData { items: self.items(), start, end, anchor: self.anchor, now: self.now };
        match self.shown() {
            CalView::Day if self.class() == SizeClass::Narrow => {
                column![week_strip::view(&data), views::week::day_under_strip(data)].into()
            }
            CalView::Day | CalView::Week => views::week::view(data),
            CalView::Month => views::month::view(data),
            CalView::Agenda => views::agenda::view(data),
        }
    }

    /// The loaded events of the shown calendars, with their calendar's colour.
    fn items(&self) -> Vec<Item<'_>> {
        let colors: HashMap<&str, iced::Color> =
            self.collections.iter().map(|c| (c.id.as_str(), collection_color(c.color.as_deref()))).collect();
        self.events
            .iter()
            .enumerate()
            .filter(|(_, e)| !self.hidden.contains(&e.collection))
            .map(|(index, event)| Item {
                index,
                event,
                color: colors.get(event.collection.as_str()).copied().unwrap_or(collection_color(None)),
            })
            .collect()
    }

    /// The layout class of the window.
    fn class(&self) -> SizeClass {
        layout::of(self.size)
    }

    /// The view on screen: the chosen one, or what the narrow layout shows for it ([`shown_in`]).
    fn shown(&self) -> CalView {
        shown_in(self.view, self.class())
    }

    /// The days [`App::load`] fetches: those the view shows; for the narrow day its whole week,
    /// for the dots of the week strip; for mini and strip today and the [`AGENDA_DAYS`] after it,
    /// so the glance finds the next event when today is done.
    fn load_range(&self) -> (NaiveDate, NaiveDate) {
        match self.class() {
            SizeClass::Mini | SizeClass::Strip => {
                let today = self.now.date();
                (today, today + Days::days(AGENDA_DAYS))
            }
            SizeClass::Narrow if self.shown() == CalView::Day => {
                let start = week_start(self.anchor);
                (start, start + Days::days(7))
            }
            _ => self.range(),
        }
    }

    /// First and last+1 day the current view shows.
    fn range(&self) -> (NaiveDate, NaiveDate) {
        match self.shown() {
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
        match self.shown() {
            CalView::Day => self.anchor + Days::days(direction),
            CalView::Week => self.anchor + Days::days(7 * direction),
            CalView::Month => mini_month::shift(self.anchor, direction as i32),
            CalView::Agenda => self.anchor + Days::days(AGENDA_DAYS * direction),
        }
    }

    fn heading(&self) -> String {
        match self.shown() {
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

    /// The header title and its quiet note: "October 2026" and "Week 41" for the week (the month
    /// of its Thursday, as ISO weeks count), the long date for a day, the month otherwise.
    fn title_parts(&self) -> (String, Option<String>) {
        let month_year = |date: NaiveDate| format!("{} {}", month_name(date.month()), date.year());
        match self.shown() {
            CalView::Day => (format_long_date(self.anchor), None),
            CalView::Week => {
                let thursday = week_start(self.anchor) + Days::days(3);
                (month_year(thursday), Some(trf("Week {week}", &[("week", &thursday.iso_week().week())])))
            }
            CalView::Month | CalView::Agenda => (month_year(self.anchor), None),
        }
    }

    /// Moves the view to `date` (the small month follows) and loads it.
    fn go_to(&mut self, date: NaiveDate) -> Task<Message> {
        self.anchor = date;
        self.mini_month = mini_month::shift(date, 0);
        self.load().chain(self.scroll_to_morning())
    }

    fn close_menus(&mut self) {
        self.menu = None;
        self.drawer = false;
    }

    /// Day and week open at 07:00 instead of midnight.
    fn scroll_to_morning(&self) -> Task<Message> {
        match self.shown() {
            CalView::Day | CalView::Week => views::week::scroll_to_morning(),
            CalView::Month | CalView::Agenda => Task::none(),
        }
    }

    fn connected(&self) -> bool {
        matches!(self.account, Some(AccountState::Connected { .. }))
    }

    /// All event calendars; the editor offers the writable ones and opens events of the
    /// others read-only.
    fn writable_calendars(&self) -> Vec<Collection> {
        self.collections.iter().filter(|c| c.kind.has_events()).cloned().collect()
    }

    fn fetch_account(&self) -> Task<Message> {
        let source = self.source.clone();
        Task::perform(async move { source.account().await.map_err(|e| e.to_string()) }, Message::AccountLoaded)
    }

    fn load(&mut self) -> Task<Message> {
        self.loading = true;
        let (start, end) = self.load_range();
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

/// The error state's text when the account or the events could not be loaded.
fn load_failed(err: &str) -> String {
    trf("The calendar could not be loaded: {error}", &[("error", &err)])
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

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    #[test]
    fn the_small_month_browses_without_moving_the_view() {
        let mut app = demo_app();
        let anchor = app.anchor;
        let _ = app.update(Message::MiniMonth(2));
        assert_eq!(app.anchor, anchor, "the main view stays");
        assert_eq!(app.mini_month, mini_month::shift(anchor, 2));
        let _ = app.update(Message::GoTo(date(2026, 3, 18)));
        assert_eq!((app.anchor, app.mini_month), (date(2026, 3, 18), date(2026, 3, 1)), "a day click moves both");
        let _ = app.update(Message::Today);
        assert_eq!(app.mini_month, mini_month::shift(app.now.date(), 0), "the small month follows the anchor");
    }

    #[test]
    fn menus_and_drawer_open_one_at_a_time_and_close_together() {
        let mut app = demo_app();
        let _ = app.update(Message::Switcher(Switcher::Toggle));
        assert_eq!(app.menu, Some(Menu::Switcher));
        let _ = app.update(Message::ToggleMenu(Menu::View));
        assert_eq!(app.menu, Some(Menu::View), "the view menu replaces the switcher");
        let _ = app.update(Message::ToggleDrawer);
        assert_eq!((app.menu, app.drawer), (None, true), "opening the drawer closes the menu");
        let _ = app.update(Message::CloseMenu);
        assert_eq!((app.menu, app.drawer), (None, false));
    }

    #[test]
    fn choosing_a_view_closes_the_menu_and_keeps_it() {
        let mut app = demo_app();
        let _ = app.update(Message::ToggleMenu(Menu::View));
        let _ = app.update(Message::SetView(CalView::Month));
        assert_eq!((app.view, app.menu), (CalView::Month, None));
    }

    #[test]
    fn toggling_a_calendar_keeps_the_menu_open() {
        let mut app = demo_app();
        let _ = app.update(Message::ToggleMenu(Menu::View));
        let _ = app.update(Message::ToggleCollection("work".into()));
        assert_eq!(app.menu, Some(Menu::View));
        assert!(app.hidden.contains("work"));
    }

    #[test]
    fn the_week_title_names_the_month_of_its_thursday_and_the_week() {
        let mut app = demo_app();
        // Week 40 of 2026 runs from Mon 28 Sep to Sun 4 Oct; its Thursday is 1 October.
        app.anchor = date(2026, 9, 29);
        let (title, note) = app.title_parts();
        assert_eq!(title, format!("{} 2026", month_name(10)));
        assert_eq!(note, Some(trf("Week {week}", &[("week", &40)])));
    }

    #[test]
    fn the_layout_follows_the_window_size() {
        let mut app = demo_app();
        let _ = app.update(Message::Resized(Size::new(420.0, 780.0)));
        assert_eq!(layout::of(app.size), SizeClass::Narrow);
        let _ = app.update(Message::Resized(Size::new(1440.0, 900.0)));
        assert_eq!(layout::of(app.size), SizeClass::Wide);
    }

    #[test]
    fn retry_clears_the_error_and_loads_again() {
        let mut app = demo_app();
        let _ = app.update(Message::AccountLoaded(Ok(AccountState::Connected {
            server: "demo".into(),
            login_name: "demo".into(),
            last_sync: None,
            problem: None,
        })));
        let _ = app.update(Message::Loaded(Err("offline".into())));
        assert_eq!(app.error, Some(load_failed("offline")));
        let _ = app.update(Message::Retry);
        assert_eq!((app.error.clone(), app.loading), (None, true));
    }

    const NARROW: Size = Size::new(420.0, 780.0);
    const WIDE: Size = Size::new(1440.0, 900.0);
    const MINI: Size = Size::new(240.0, 160.0);

    #[test]
    fn narrow_shows_a_week_as_a_day_and_a_month_as_the_agenda_and_keeps_the_choice() {
        let mut app = demo_app();
        let _ = app.update(Message::Resized(NARROW));
        assert_eq!((app.view, app.shown()), (CalView::Week, CalView::Day));
        let _ = app.update(Message::SetView(CalView::Month));
        assert_eq!((app.view, app.shown()), (CalView::Month, CalView::Agenda));
        let _ = app.update(Message::Resized(WIDE));
        assert_eq!((app.view, app.shown()), (CalView::Month, CalView::Month), "the month comes back");
    }

    #[test]
    fn the_narrow_day_steps_by_a_day_and_loads_its_week() {
        let mut app = demo_app();
        let _ = app.update(Message::Resized(NARROW));
        app.anchor = date(2026, 10, 7);
        assert_eq!(app.range(), (date(2026, 10, 7), date(2026, 10, 8)));
        assert_eq!(app.step(1), date(2026, 10, 8));
        assert_eq!(app.load_range(), (date(2026, 10, 5), date(2026, 10, 12)), "Monday to Sunday for the strip");
    }

    #[test]
    fn mini_and_strip_load_from_today_and_keep_the_view_and_the_date() {
        let mut app = demo_app();
        let _ = app.update(Message::SetView(CalView::Month));
        app.anchor = date(2026, 3, 18);
        let today = app.now.date();
        for size in [MINI, Size::new(1200.0, 200.0)] {
            let _ = app.update(Message::Resized(size));
            assert_eq!(app.load_range(), (today, today + Days::days(AGENDA_DAYS)));
            assert_eq!((app.view, app.anchor), (CalView::Month, date(2026, 3, 18)));
        }
    }

    #[test]
    fn an_open_editor_survives_a_trip_through_mini() {
        let mut app = demo_app();
        let _ = app.update(Message::NewEvent { date: date(2026, 10, 7), time: None });
        let _ = app.update(Message::Resized(MINI));
        assert!(app.editor.is_some());
        let _ = app.update(Message::Resized(NARROW));
        assert!(app.editor.is_some(), "and opens full-window in narrow");
    }

    #[test]
    fn without_a_preference_the_system_appearance_is_dark() {
        assert_eq!(demo_app().theme(), theme::theme());
    }
}
