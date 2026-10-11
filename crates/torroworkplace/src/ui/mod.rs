//! `torroworkplace ui`: the control centre – one place for everything that concerns the
//! whole suite (accounts, workspaces, apps, general settings). Concept: `docs/settings.md`,
//! tasks: `docs/tasks/steuerzentrale.md`.
//!
//! A separate Iced process (app id `torro-workplace`) that talks to the daemon over the
//! socket, like the apps do. The shell here owns navigation and the data; each section is a
//! module with the same shape:
//!
//! ```text
//! pub struct State            page-local UI state (Default)
//! pub enum Message            what the page's widgets report
//! pub fn update(&mut State, Message, &Data) -> Action
//! pub fn view<'a>(&'a State, &'a Data) -> Element<'a, Message>
//! ```
//!
//! Pages never talk to the daemon themselves: they return an [`Action`], the shell sends the
//! request and reloads [`Data`].

mod accounts;
mod apps;
mod general;
mod overview;
mod wizard;
mod workspaces;

use std::time::Duration;

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length, Subscription, Task};
use torro_core::AppId;
use torro_core::i18n::tr;
use torro_core::settings::Settings;
use torro_gui::theme;
use torro_ipc::{AccountInfo, Request, Response, Status, client};
use torro_setups::TemplateSummary;

/// Below this width the sidebar turns into a list and a section opens as its own page.
const NARROW: f32 = 640.0;

/// The sections in sidebar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Overview,
    Accounts,
    Workspaces,
    Apps,
    General,
}

impl Section {
    pub const ALL: [Section; 5] =
        [Section::Overview, Section::Accounts, Section::Workspaces, Section::Apps, Section::General];

    pub fn label(self) -> &'static str {
        match self {
            Section::Overview => tr("Overview"),
            Section::Accounts => tr("Accounts"),
            Section::Workspaces => tr("Workspaces"),
            Section::Apps => tr("Apps"),
            Section::General => tr("General"),
        }
    }

    /// `--section accounts` and friends.
    pub fn from_arg(arg: &str) -> Option<Section> {
        match arg {
            "overview" => Some(Section::Overview),
            "accounts" => Some(Section::Accounts),
            "workspaces" => Some(Section::Workspaces),
            "apps" => Some(Section::Apps),
            "general" => Some(Section::General),
            _ => None,
        }
    }
}

/// Everything the pages show, loaded from the daemon.
#[derive(Debug, Clone, Default)]
pub struct Data {
    pub status: Option<Status>,
    pub accounts: Vec<AccountInfo>,
    pub templates: Vec<TemplateSummary>,
    pub settings: Settings,
    /// Why the last load failed (daemon not running, …).
    pub error: Option<String>,
}

/// What a page wants the shell to do.
#[derive(Debug, Clone)]
pub enum Action {
    None,
    /// Send this request to the daemon, then reload.
    Send(Request),
    /// Open a web page in the browser.
    OpenUrl(String),
    /// Jump to another section.
    Select(Section),
}

pub struct Ui {
    section: Section,
    /// In a narrow window: showing the section list (true) or the page (false).
    narrow_list: bool,
    width: f32,
    data: Data,
    /// The last failed request, kept across reloads until the next success or section change.
    notice: Option<String>,
    overview: overview::State,
    accounts: accounts::State,
    workspaces: workspaces::State,
    apps: apps::State,
    general: general::State,
}

#[derive(Debug, Clone)]
pub enum Message {
    Select(Section),
    /// Narrow window: back to the section list.
    Back,
    Loaded(Box<Data>),
    Sent(Result<Response, String>),
    Resized(f32),
    Tick,
    /// Escape: close whatever dialog the current page has open.
    Escape,
    Overview(overview::Message),
    Accounts(accounts::Message),
    Workspaces(workspaces::Message),
    Apps(apps::Message),
    General(general::Message),
}

/// Runs the control centre. `args` are the words after `ui` (`--section accounts`).
pub fn run(args: &[String]) -> iced::Result {
    let section = args
        .windows(2)
        .find(|w| w[0] == "--section")
        .and_then(|w| Section::from_arg(&w[1]))
        .unwrap_or(Section::Overview);
    if let Some(socket) = torro_ipc::socket_path() {
        torro_gui::source::ensure_core(&socket);
    }
    iced::application(move || Ui::new(section), Ui::update, Ui::view)
        .settings(torro_gui::settings())
        .title(|ui: &Ui| format!("Torro Workplace · {}", ui.section.label()))
        .theme(|_: &Ui| theme::theme())
        .subscription(Ui::subscription)
        .window(torro_gui::window(AppId::WORKPLACE))
        .run()
}

impl Ui {
    fn new(section: Section) -> (Ui, Task<Message>) {
        let ui = Ui {
            section,
            narrow_list: false,
            width: 1100.0,
            data: Data::default(),
            notice: None,
            overview: overview::State::default(),
            accounts: accounts::State::default(),
            workspaces: workspaces::State::default(),
            apps: apps::State::default(),
            general: general::State::default(),
        };
        (ui, load())
    }

    fn subscription(&self) -> Subscription<Message> {
        // Faster while an account is being set up, so the page follows the browser sign-in.
        let every = if self.accounts.busy() { Duration::from_secs(1) } else { Duration::from_secs(3) };
        Subscription::batch([
            iced::time::every(every).map(|_| Message::Tick),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size.width)),
            iced::event::listen_with(escape_key),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let action = match message {
            Message::Select(section) => {
                self.section = section;
                self.narrow_list = false;
                self.notice = None;
                Action::None
            }
            Message::Back => {
                self.narrow_list = true;
                Action::None
            }
            Message::Loaded(data) => {
                self.data = *data;
                // The add-account dialog follows the sign-in through the reloaded accounts.
                if let Some(wizard) = self.accounts.wizard.as_mut() {
                    wizard::observe(wizard, &self.data);
                }
                Action::None
            }
            Message::Sent(result) => {
                self.notice = result.err();
                return load();
            }
            Message::Resized(width) => {
                self.width = width;
                Action::None
            }
            Message::Tick => return load(),
            Message::Escape => {
                let page = match self.section {
                    Section::Accounts => accounts::escape(&self.accounts).map(Message::Accounts),
                    Section::Workspaces => workspaces::escape(&self.workspaces).map(Message::Workspaces),
                    _ => None,
                };
                return match page {
                    Some(message) => self.update(message),
                    None => Task::none(),
                };
            }
            Message::Overview(m) => overview::update(&mut self.overview, m, &self.data),
            Message::Accounts(m) => accounts::update(&mut self.accounts, m, &self.data),
            Message::Workspaces(m) => workspaces::update(&mut self.workspaces, m, &self.data),
            Message::Apps(m) => apps::update(&mut self.apps, m, &self.data),
            Message::General(m) => general::update(&mut self.general, m, &self.data),
        };
        self.perform(action)
    }

    fn perform(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::None => Task::none(),
            Action::Send(request) => send(request),
            Action::OpenUrl(url) => {
                if let Err(err) = webbrowser::open(&url) {
                    self.data.error = Some(err.to_string());
                }
                Task::none()
            }
            Action::Select(section) => {
                self.section = section;
                self.narrow_list = false;
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let narrow = self.width < NARROW;
        if narrow && self.narrow_list {
            return container(self.sidebar(true)).width(Length::Fill).height(Length::Fill).into();
        }
        let page = self.page(narrow);
        if narrow {
            return page;
        }
        row![self.sidebar(false), page].height(Length::Fill).into()
    }

    /// The section list: a 220 px sidebar, or the whole window when narrow.
    fn sidebar(&self, full: bool) -> Element<'_, Message> {
        let mut list = column![].spacing(4).padding(16);
        for section in Section::ALL {
            let selected = section == self.section && !full;
            list = list.push(
                button(text(section.label()).size(14))
                    .style(move |theme, status| sidebar_item(theme, status, selected))
                    .width(Length::Fill)
                    .padding([8, 12])
                    .on_press(Message::Select(section)),
            );
        }
        let version = text(format!("Torro Workplace · {} {}", tr("Version"), env!("CARGO_PKG_VERSION")))
            .size(12)
            .color(theme::MUTED);
        let body = column![scrollable(list).height(Length::Fill), container(version).padding(16)];
        let width = if full { Length::Fill } else { Length::Fixed(220.0) };
        container(body)
            .width(width)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(theme::SURFACE)),
                ..container::Style::default()
            })
            .into()
    }

    /// The current section's page with its title; a back button when narrow.
    fn page(&self, narrow: bool) -> Element<'_, Message> {
        let content: Element<'_, Message> = match self.section {
            Section::Overview => overview::view(&self.overview, &self.data).map(Message::Overview),
            Section::Accounts => accounts::view(&self.accounts, &self.data).map(Message::Accounts),
            Section::Workspaces => workspaces::view(&self.workspaces, &self.data).map(Message::Workspaces),
            Section::Apps => apps::view(&self.apps, &self.data).map(Message::Apps),
            Section::General => general::view(&self.general, &self.data).map(Message::General),
        };
        let mut header = row![].spacing(8).align_y(iced::Alignment::Center);
        if narrow {
            header = header.push(button(text("‹").size(14)).style(button::text).on_press(Message::Back));
        }
        header = header.push(text(self.section.label()).size(18));
        let mut page = column![header].spacing(16).padding(24).width(Length::Fill);
        for problem in [&self.notice, &self.data.error].into_iter().flatten() {
            page = page.push(text(problem.clone()).size(14).color(theme::attention()));
        }
        page.push(scrollable(content).height(Length::Fill)).into()
    }
}

/// A sidebar entry: the selected one sits on a lighter surface instead of Torro red, so the
/// page keeps its one loud moment (its primary button).
fn sidebar_item(theme: &iced::Theme, status: button::Status, selected: bool) -> button::Style {
    let base = button::text(theme, status);
    let background = match (selected, status) {
        (true, _) => Some(iced::Background::Color(theme::SURFACE_2)),
        (false, button::Status::Hovered | button::Status::Pressed) => Some(iced::Background::Color(theme::SURFACE_2)),
        _ => None,
    };
    button::Style {
        background,
        text_color: if selected { theme::TEXT } else { theme::MUTED },
        border: iced::border::rounded(8),
        ..base
    }
}

/// Escape, wherever the focus is (a text input keeps it to itself otherwise).
fn escape_key(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            ..
        }) => Some(Message::Escape),
        _ => None,
    }
}

/// Loads everything the pages show.
fn load() -> Task<Message> {
    Task::perform(
        async {
            let Some(socket) = torro_ipc::socket_path() else {
                return Data { error: Some(tr("No home directory").into()), ..Data::default() };
            };
            let mut data = Data::default();
            match client::call(&socket, &Request::Status).await {
                Ok(Response::Status(status)) => data.status = Some(status),
                Ok(other) => data.error = Some(format!("{other:?}")),
                Err(err) => {
                    data.error = Some(err.to_string());
                    return data;
                }
            }
            if let Ok(Response::Accounts { accounts }) = client::call(&socket, &Request::Accounts).await {
                data.accounts = accounts;
            }
            if let Ok(Response::Templates { templates }) = client::call(&socket, &Request::ListTemplates).await {
                data.templates = templates;
            }
            if let Ok(Response::Settings { settings }) = client::call(&socket, &Request::Settings).await {
                data.settings = settings;
            }
            data
        },
        |data| Message::Loaded(Box::new(data)),
    )
}

/// Sends one request and reports the answer; errors come back as text.
fn send(request: Request) -> Task<Message> {
    Task::perform(
        async move {
            let socket = torro_ipc::socket_path().ok_or_else(|| tr("No home directory").to_string())?;
            match client::call(&socket, &request).await {
                Ok(Response::Error { message, .. }) => Err(message),
                Ok(response) => Ok(response),
                Err(err) => Err(err.to_string()),
            }
        },
        Message::Sent,
    )
}
