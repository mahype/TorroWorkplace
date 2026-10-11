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
//! pub fn header<'a>(&'a State, &'a Data, SizeClass) -> Header<'a, Message>   (optional)
//! pub fn overlay<'a>(&'a State, &'a Data) -> Option<Element<'a, Message>>      (optional)
//! ```
//!
//! `overlay` is the page's dialog while one is open, built with [`dialog`] or [`confirm`]: the
//! shell lays it over the whole window (sidebar and header included), not just the page area.
//!
//! Pages never talk to the daemon themselves: they return an [`Action`], the shell sends the
//! request and reloads [`Data`].
//!
//! The window is the Linear frame of the suite (`torro_gui::shell`, design skill section 0):
//! wide windows show the sidebar with the app switcher and the sections; medium and narrow ones
//! put the sections into a menu at the top left ("Accounts ▾") and the sidebar into a drawer, the
//! same way the calendar and the tasks app do. The page sits under a 44-high header bar with its
//! title and actions, at most [`size::CONTENT_MAX`] wide.
//!
//! The pages are settings pages: `widgets::group`s of rows built with [`setting_row`] (design
//! skill, section 5), rows that open a page through [`pressable`] with a [`chevron`].

mod accounts;
mod apps;
mod general;
mod overview;
mod wizard;
mod workspaces;

use std::time::Duration;

use iced::theme::Mode;
use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{Text, button, column, container, responsive, row, scrollable, stack, svg};
use iced::{Alignment, Element, Fill, Padding, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::tr;
use torro_core::settings::Settings;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, Switcher};
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, MenuItem, MenuSection, Placement, Trail};
use torro_gui::{style, typo};
use torro_ipc::{AccountInfo, AppInfo, Request, Response, Status, client};
use torro_setups::TemplateSummary;

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

    /// The sections at the top of the sidebar; [`Section::General`] sits at its foot, where Linear
    /// keeps "Settings".
    fn nav() -> impl Iterator<Item = Section> {
        Section::ALL.into_iter().filter(|&section| section != Section::General)
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::Overview => tr("Overview"),
            Section::Accounts => tr("Accounts"),
            Section::Workspaces => tr("Workspaces"),
            Section::Apps => tr("Apps"),
            Section::General => tr("General"),
        }
    }

    /// The icon in the sidebar, the section menu and the header title.
    fn icon(self) -> Icon {
        match self {
            Section::Overview => Icon::LayoutGrid,
            Section::Accounts => Icon::Cloud,
            Section::Workspaces => Icon::Layers,
            Section::Apps => Icon::AppWindow,
            Section::General => Icon::Settings,
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
    /// The suite's apps as the core reports them; `None` until the first answer.
    pub apps: Option<Vec<AppInfo>>,
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

/// What a page puts into the header bar next to the section title. A page module builds it in an
/// optional `pub fn header<'a>(&'a State, &'a Data, SizeClass) -> Header<'a, Message>`; pages
/// without one get [`Header::default`] (just the title).
pub struct Header<'a, M> {
    /// A page below the section that is open now (one account's detail): its name follows the
    /// section title ("Accounts › cloud.example.org", only the name in medium and narrow) and a
    /// back button in front of it sends the message. `None` on the section's own page.
    pub sub_page: Option<(String, M)>,
    /// The page's actions at the right of the bar, left to right; at most one primary button
    /// ("Add account"). Text buttons from `torro_gui::widgets::text_button`; in the narrow and
    /// mini layouts icon-only ones (`widgets::compact_button`) so the section menu keeps its room.
    pub actions: Vec<Element<'a, M>>,
}

impl<M> Default for Header<'_, M> {
    fn default() -> Self {
        Self { sub_page: None, actions: Vec::new() }
    }
}

impl<'a, M: 'a> Header<'a, M> {
    /// The same header with the page's messages wrapped in the shell's (`Message::Accounts`).
    fn map<N: 'a>(self, wrap: fn(M) -> N) -> Header<'a, N> {
        Header {
            sub_page: self.sub_page.map(|(name, back)| (name, wrap(back))),
            actions: self.actions.into_iter().map(|action| action.map(wrap)).collect(),
        }
    }
}

/// The open menu of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// The app switcher at the top of the sidebar.
    Switcher,
    /// The section menu in the header of the medium and narrow layouts.
    Nav,
}

pub struct Ui {
    section: Section,
    /// The menu that is open, if any.
    menu: Option<Menu>,
    /// Medium and narrow: the sidebar drawer is open.
    drawer: bool,
    /// The core has answered at least once (successfully or not); until then the page shows the
    /// loading state instead of empty lists.
    loaded: bool,
    /// The desktop's light/dark preference, for the appearance "System".
    system_mode: Mode,
    /// The stored appearance. Kept apart from `data` so a core that does not answer for a moment
    /// (an empty `Data`) does not flip the colours.
    appearance: Appearance,
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
    /// Open or close a menu.
    ToggleMenu(Menu),
    /// Close the open menu and the drawer (a click next to them).
    CloseMenu,
    /// Open or close the sidebar drawer (medium and narrow).
    ToggleDrawer,
    /// The app switcher and its search and "New" buttons.
    Switcher(Switcher),
    Loaded(Box<Data>),
    Sent(Result<Response, String>),
    /// The desktop's light/dark preference (once at start, then whenever it changes).
    SystemMode(Mode),
    /// Reload everything (every few seconds, and "Retry" when the core does not answer).
    Tick,
    /// Escape: close the open menu, or else whatever dialog the current page has open.
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
        .theme(Ui::theme)
        .subscription(Ui::subscription)
        .window(torro_gui::window(AppId::WORKPLACE))
        .run()
}

impl Ui {
    fn new(section: Section) -> (Ui, Task<Message>) {
        let ui = Ui {
            section,
            menu: None,
            drawer: false,
            loaded: false,
            system_mode: Mode::None,
            appearance: Appearance::System,
            data: Data::default(),
            notice: None,
            overview: overview::State::default(),
            accounts: accounts::State::default(),
            workspaces: workspaces::State::default(),
            apps: apps::State::default(),
            general: general::State::default(),
        };
        (ui, Task::batch([load(), iced::system::theme().map(Message::SystemMode)]))
    }

    /// The Iced theme: the stored appearance (the page reloads it with every `Data`), with
    /// "System" following the desktop.
    fn theme(&self) -> Theme {
        theme::for_appearance(self.appearance, self.system_mode)
    }

    fn subscription(&self) -> Subscription<Message> {
        // Faster while an account is being set up, so the page follows the browser sign-in.
        let every = if self.accounts.busy() { Duration::from_secs(1) } else { Duration::from_secs(3) };
        Subscription::batch([
            iced::time::every(every).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemMode),
            iced::event::listen_with(escape_key),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let action = match message {
            Message::Select(section) => {
                self.select(section);
                self.notice = None;
                Action::None
            }
            Message::ToggleMenu(menu) => {
                self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
                Action::None
            }
            Message::CloseMenu => {
                self.close_menus();
                Action::None
            }
            Message::ToggleDrawer => {
                self.drawer = !self.drawer;
                self.menu = None;
                Action::None
            }
            Message::Switcher(switcher) => self.switcher(switcher),
            Message::Loaded(data) => {
                self.data = *data;
                self.loaded = true;
                if self.data.error.is_none() {
                    self.appearance = self.data.settings.appearance;
                }
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
            Message::SystemMode(mode) => {
                self.system_mode = mode;
                Action::None
            }
            Message::Tick => return load(),
            Message::Escape => {
                if self.menu.is_some() || self.drawer {
                    self.close_menus();
                    return Task::none();
                }
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

    /// Shows `section`; a choice from the menu or the drawer also closes them.
    fn select(&mut self, section: Section) {
        self.section = section;
        self.close_menus();
    }

    fn close_menus(&mut self) {
        self.menu = None;
        self.drawer = false;
    }

    /// What the app switcher's buttons do in the control centre.
    fn switcher(&mut self, switcher: Switcher) -> Action {
        match switcher {
            Switcher::Toggle => {
                self.menu = if self.menu == Some(Menu::Switcher) { None } else { Some(Menu::Switcher) };
                Action::None
            }
            Switcher::OpenApp(app) => {
                self.close_menus();
                Action::Send(Request::OpenApp { app: app.to_string() })
            }
            // The control centre has nothing to search yet; the button only closes open menus.
            Switcher::Search => {
                self.close_menus();
                Action::None
            }
            // "New" here means a new account, the one thing the control centre creates.
            Switcher::New => {
                self.select(Section::Accounts);
                accounts::update(&mut self.accounts, accounts::Message::AddAccount, &self.data)
            }
        }
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
                self.select(section);
                Task::none()
            }
        }
    }

    /// The window in the layout of its real size: `responsive` hands over the size Iced laid out,
    /// so the first frame already fits the tile (no assumed start width).
    fn view(&self) -> Element<'_, Message> {
        responsive(move |size| self.window(layout::of(size))).into()
    }

    fn window(&self, class: SizeClass) -> Element<'_, Message> {
        let (switcher, nav) = (Anchor::new(), Anchor::new());
        let layer = self.menu.map(|menu| match menu {
            Menu::Switcher => widgets::floating(
                &switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(AppId::WORKPLACE, Message::Switcher),
                Message::CloseMenu,
            ),
            Menu::Nav => widgets::floating(&nav, Placement::Below(Align::Start), self.nav_menu(), Message::CloseMenu),
        });
        // The page's dialog lies under an open menu: the switcher still works above it.
        let layer = match (self.page_overlay(), layer) {
            (Some(dialog), Some(menu)) => Some(stack![dialog, menu].into()),
            (dialog, menu) => dialog.or(menu),
        };
        shell::shell(class, self.sidebar(&switcher), self.header(class, &nav), self.body(class), None)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(layer)
            .into()
    }

    /// The count after a section in the sidebar and the menu: accounts and templates.
    fn trail(&self, section: Section) -> Trail {
        let count = match section {
            Section::Accounts => accounts::listed(&self.data.accounts).count(),
            Section::Workspaces => self.data.templates.len(),
            _ => 0,
        };
        if count > 0 { Trail::Count(count) } else { Trail::None }
    }

    /// The sidebar: app switcher, the sections, and "General" with the version at the foot.
    fn sidebar(&self, switcher: &Anchor) -> Element<'_, Message> {
        let item = |section: Section| {
            widgets::sidebar_item(
                section.icon(),
                section.label(),
                self.trail(section),
                section == self.section,
                Message::Select(section),
            )
        };
        let version = format!("Torro Workplace · {} {}", tr("Version"), env!("CARGO_PKG_VERSION"));
        let version = container(typo::text(typo::META, version).style(theme::text_3).wrapping(Wrapping::None))
            .center_y(size::ROW_SIDEBAR)
            .padding(style::ITEM_PADDING)
            .clip(true);
        shell::sidebar(
            shell::app_switcher(AppId::WORKPLACE, switcher, Message::Switcher),
            Section::nav().map(item).collect(),
            vec![item(Section::General), version.into()],
        )
    }

    /// The section menu of the medium and narrow layouts: the sidebar's sections, "General" in a
    /// part of its own as at the sidebar's foot.
    fn nav_menu(&self) -> Element<'_, Message> {
        let entry = |section: Section| {
            MenuItem::new(section.icon(), section.label(), Message::Select(section))
                .trail(self.trail(section))
                .current(section == self.section)
        };
        widgets::menu(vec![
            MenuSection::new(Section::nav().map(entry).collect()),
            MenuSection::new(vec![entry(Section::General)]),
        ])
    }

    /// What the current page adds to the header bar.
    fn page_header(&self, class: SizeClass) -> Header<'_, Message> {
        match self.section {
            Section::Accounts => accounts::header(&self.accounts, &self.data, class).map(Message::Accounts),
            _ => Header::default(),
        }
    }

    /// The dialog the current page has open, laid over the whole window.
    fn page_overlay(&self) -> Option<Element<'_, Message>> {
        match self.section {
            Section::Accounts => accounts::overlay(&self.accounts, &self.data).map(|e| e.map(Message::Accounts)),
            Section::Workspaces => {
                workspaces::overlay(&self.workspaces, &self.data).map(|e| e.map(Message::Workspaces))
            }
            _ => None,
        }
    }

    /// The header bar: the title (wide) or the section menu with the drawer button (medium,
    /// narrow); on a page below the section a back button and its name; the page's actions right.
    fn header(&self, class: SizeClass, nav: &Anchor) -> Element<'_, Message> {
        let Header { sub_page, actions } = self.page_header(class);
        let section = self.section;
        let wide = shell::has_sidebar(class);
        let (leading, title): (Vec<Element<'_, Message>>, Element<'_, Message>) = match sub_page {
            Some((name, back)) => {
                let back = widgets::icon_button(Icon::ChevronLeft, tr("Back"), Some(back));
                let title = if wide { crumb(section, name) } else { shell::header_title(None, name, None) };
                (vec![back], title)
            }
            None if wide => (vec![], shell::header_title(Some(section.icon()), section.label(), None)),
            None => (
                vec![shell::sidebar_toggle(Message::ToggleDrawer)],
                widgets::menu_button(nav, Some(section.icon()), section.label(), Message::ToggleMenu(Menu::Nav)),
            ),
        };
        shell::header_bar(leading, title, actions)
    }

    /// The page: loading until the core answered, the error state when it does not answer (the
    /// overview shows that itself), else the section's view under the notices – at most
    /// `CONTENT_MAX` wide, scrolling inside the margin of the size class.
    fn body(&self, class: SizeClass) -> Element<'_, Message> {
        if !self.loaded {
            return widgets::loading();
        }
        let unreachable = self.data.status.is_none() && self.data.error.is_some();
        if unreachable && self.section != Section::Overview {
            return widgets::error_state(
                tr("Torro Workplace isn't responding. It may still be starting."),
                Message::Tick,
            );
        }
        let page: Element<'_, Message> = match self.section {
            Section::Overview => overview::view(&self.overview, &self.data).map(Message::Overview),
            Section::Accounts => accounts::view(&self.accounts, &self.data).map(Message::Accounts),
            Section::Workspaces => workspaces::view(&self.workspaces, &self.data).map(Message::Workspaces),
            Section::Apps => apps::view(&self.apps, &self.data).map(Message::Apps),
            Section::General => general::view(&self.general, &self.data).map(Message::General),
        };
        let notices = [&self.notice, &self.data.error].into_iter().flatten().map(|problem| notice(problem.clone()));
        let page = column(notices).push(page).spacing(space::L);
        scrollable(container(widgets::content(size::CONTENT_MAX, page)).padding(page_margin(class)))
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .height(Fill)
            .into()
    }
}

/// The margin around a page: 24, in a narrow window 16, in a tile 12 (design skill, section 6).
fn page_margin(class: SizeClass) -> f32 {
    match class {
        SizeClass::Wide | SizeClass::Medium => space::XL,
        SizeClass::Narrow => space::L,
        SizeClass::Mini | SizeClass::Strip => space::M,
    }
}

/// "Accounts › cloud.example.org": the section in `text_3`, the page below it in `BODY_STRONG`.
fn crumb<'a>(section: Section, name: String) -> Element<'a, Message> {
    let chevron = icon::icon(Icon::ChevronRight, icon::SIZE_SM)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    row![
        typo::text(typo::BODY, section.label()).style(theme::text_3).wrapping(Wrapping::None),
        chevron,
        typo::text(typo::BODY_STRONG, name).wrapping(Wrapping::None),
    ]
    .spacing(space::XS)
    .align_y(Alignment::Center)
    .into()
}

/// A failed request or load above the page: an alert icon in the error colour, the message in
/// `text` – the colour is only in the icon.
fn notice<'a>(problem: String) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    row![alert, typo::text(typo::BODY, problem)].spacing(space::S).align_y(Alignment::Center).into()
}

// ----- Building blocks of the pages -----------------------------------------------------------
//
// Local stand-ins for what `torro_gui::widgets` does not offer yet (proposals for torro-gui in the
// report of task W.2): `widgets::group_row` has no mark in front, wraps long labels and cannot be
// pressed, and there is no dialog surface of its own.

/// A row of a settings group (`widgets::group`) with the metrics of `widgets::group_row` – 44
/// high, 56 with a `detail`, 16 at the sides, 16 before the `control` – plus an optional `mark`
/// in front (a status or calendar dot) and label and detail on one line each, clipped instead of
/// wrapped, so a long account name never breaks the row. The label takes the text colour of a
/// surrounding button ([`pressable`]); the detail is `META` in `text_2`.
fn setting_row<'a, M: 'a>(
    mark: Option<Element<'a, M>>,
    label: impl IntoFragment<'a>,
    detail: Option<String>,
    control: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let tall = detail.is_some();
    let mut lines = column![one_line(typo::text(typo::BODY, label))];
    if let Some(detail) = detail {
        lines = lines.push(one_line(typo::text(typo::META, detail).style(theme::text_2)));
    }
    let text = row![].push(mark).push(lines.width(Fill)).spacing(space::M).align_y(Alignment::Center);
    row_frame(row![text.width(Fill), control.into()].spacing(space::L).align_y(Alignment::Center), tall)
}

/// `content` as one line of a settings group: 44 high (56 with two lines of text), 16 at the sides.
fn row_frame<'a, M: 'a>(content: impl Into<Element<'a, M>>, tall: bool) -> Element<'a, M> {
    let height = if tall { size::HEADER + space::M } else { size::HEADER };
    container(content).width(Fill).center_y(height).padding([0.0, space::L]).into()
}

/// `text` on one line, as wide as there is room, cut off at the end instead of wrapping (Iced 0.14
/// has no ellipsis).
fn one_line<'a, M: 'a>(text: Text<'a>) -> Element<'a, M> {
    container(text.wrapping(Wrapping::None)).width(Fill).clip(true).into()
}

/// `row` (a [`setting_row`]) as a button over its whole width: hover lays `hover` under it, the
/// label stays in `text`. `None` draws it disabled (40 %).
fn pressable<'a, M: Clone + 'a>(row: Element<'a, M>, on_press: Option<M>) -> Element<'a, M> {
    button(row).padding(Padding::ZERO).width(Fill).style(style::button::list_row(false)).on_press_maybe(on_press).into()
}

/// The chevron at the end of a row that opens a page or a next step: 16, `text_3`.
fn chevron<'a, M: 'a>() -> Element<'a, M> {
    icon::icon(Icon::ChevronRight, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) })
        .into()
}

/// A dialog over the whole window, for a page's `overlay`: `content` on the dialog surface (`menu`,
/// radius 10, shadow, 24 inside), centred and at most `max_width` wide; the window behind it is
/// dimmed, and a click next to it sends `on_dismiss`. Escape is the page's `escape`.
fn dialog<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>, max_width: f32, on_dismiss: M) -> Element<'a, M> {
    let card = container(content).padding(space::XL).width(Fill).style(style::container::popover);
    // `modal` takes its size from its base; an empty base that fills the window.
    widgets::modal(blank().width(Fill).height(Fill), card, max_width, on_dismiss)
}

/// The confirmation before something is removed, as a [`dialog`]: `title` in `TITLE`, the
/// `consequence` in `text_2`, and at the right "Cancel" and the `confirm` button (danger). A
/// click next to it cancels.
fn confirm<'a, M: Clone + 'a>(
    title: &'a str,
    consequence: String,
    confirm: &'a str,
    on_confirm: M,
    on_cancel: M,
) -> Element<'a, M> {
    let buttons = row![
        blank::horizontal(),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(on_cancel.clone())),
        widgets::text_button(None, confirm, style::button::danger, Some(on_confirm)),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);
    let content =
        column![typo::text(typo::TITLE, title), typo::text(typo::BODY, consequence).style(theme::text_2), buttons]
            .spacing(space::L);
    dialog(content, widgets::POPOVER_WIDTH, on_cancel)
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
            if let Ok(Response::Apps { apps }) = client::call(&socket, &Request::ListApps).await {
                data.apps = Some(apps);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(appearance: Appearance, error: Option<&str>) -> Message {
        let settings = Settings { appearance, ..Settings::default() };
        Message::Loaded(Box::new(Data { settings, error: error.map(String::from), ..Data::default() }))
    }

    #[test]
    fn theme_follows_the_stored_appearance() {
        let (mut ui, _) = Ui::new(Section::Overview);
        let _ = ui.update(Message::SystemMode(Mode::Light));
        assert_eq!(ui.theme(), theme::light(), "System follows the desktop");
        let _ = ui.update(loaded(Appearance::Dark, None));
        assert_eq!(ui.theme(), theme::theme());
        let _ = ui.update(loaded(Appearance::Light, None));
        let _ = ui.update(Message::SystemMode(Mode::Dark));
        assert_eq!(ui.theme(), theme::light(), "an explicit choice ignores the desktop");
    }

    #[test]
    fn an_unreachable_core_keeps_the_colours() {
        let (mut ui, _) = Ui::new(Section::Overview);
        let _ = ui.update(loaded(Appearance::Light, None));
        let _ = ui.update(loaded(Appearance::System, Some("core is not running")));
        assert_eq!(ui.theme(), theme::light());
    }

    #[test]
    fn the_page_loads_until_the_core_answers() {
        let (mut ui, _) = Ui::new(Section::Accounts);
        assert!(!ui.loaded);
        let _ = ui.update(loaded(Appearance::System, Some("core is not running")));
        assert!(ui.loaded, "a failed answer ends the loading state too");
    }

    #[test]
    fn choosing_a_section_closes_menu_and_drawer() {
        let (mut ui, _) = Ui::new(Section::Overview);
        let _ = ui.update(Message::ToggleDrawer);
        let _ = ui.update(Message::ToggleMenu(Menu::Nav));
        assert_eq!(ui.menu, Some(Menu::Nav));
        let _ = ui.update(Message::Select(Section::Workspaces));
        assert_eq!(ui.section, Section::Workspaces);
        assert_eq!((ui.menu, ui.drawer), (None, false));
        let _ = ui.update(Message::ToggleMenu(Menu::Nav));
        let _ = ui.update(Message::ToggleMenu(Menu::Nav));
        assert_eq!(ui.menu, None, "the trigger closes its own menu");
    }

    #[test]
    fn escape_closes_the_menu_before_the_dialog_below_it() {
        let (mut ui, _) = Ui::new(Section::Overview);
        let _ = ui.update(Message::Switcher(Switcher::New));
        assert_eq!(ui.section, Section::Accounts, "New in the switcher adds an account");
        assert!(ui.accounts.wizard.is_some());
        let _ = ui.update(Message::Switcher(Switcher::Toggle));
        assert_eq!(ui.menu, Some(Menu::Switcher));
        let _ = ui.update(Message::Escape);
        assert_eq!(ui.menu, None);
        assert!(ui.accounts.wizard.is_some(), "the dialog stays for the next Escape");
        let _ = ui.update(Message::Escape);
        assert!(ui.accounts.wizard.is_none());
    }

    #[test]
    fn the_switcher_opens_other_apps_through_the_core() {
        let (mut ui, _) = Ui::new(Section::Overview);
        ui.menu = Some(Menu::Switcher);
        let action = ui.switcher(Switcher::OpenApp(AppId::CALENDAR.binary));
        assert!(matches!(action, Action::Send(Request::OpenApp { ref app }) if app == "torrocalendar"));
        assert_eq!(ui.menu, None);
    }

    #[test]
    fn sections_have_distinct_icons() {
        for (i, a) in Section::ALL.iter().enumerate() {
            for b in &Section::ALL[i + 1..] {
                assert_ne!(a.icon(), b.icon(), "{a:?} and {b:?}");
            }
        }
        assert!(Section::nav().all(|section| section != Section::General), "General sits at the foot");
    }

    #[test]
    fn page_margin_follows_the_size_class() {
        assert_eq!(page_margin(SizeClass::Wide), space::XL);
        assert_eq!(page_margin(SizeClass::Medium), space::XL);
        assert_eq!(page_margin(SizeClass::Narrow), space::L);
        assert_eq!(page_margin(SizeClass::Mini), space::M);
    }

    #[test]
    fn the_window_builds_in_every_class_and_state() {
        let (mut ui, _) = Ui::new(Section::Accounts);
        for class in [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let _ = ui.window(class);
        }
        let _ = ui.update(loaded(Appearance::System, None));
        let _ = ui.update(Message::ToggleMenu(Menu::Nav));
        for class in [SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let _ = ui.window(class);
        }
        let _ = ui.update(Message::Accounts(accounts::Message::AddAccount));
        for class in [SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let _ = ui.window(class);
        }
    }

    #[test]
    fn page_dialogs_lie_over_the_whole_window() {
        let (mut ui, _) = Ui::new(Section::Accounts);
        assert!(ui.page_overlay().is_none());
        let _ = ui.update(Message::Accounts(accounts::Message::AddAccount));
        assert!(ui.page_overlay().is_some(), "the add-account dialog is a window layer");
        let _ = ui.update(Message::Select(Section::Overview));
        assert!(ui.page_overlay().is_none(), "another section shows no dialog of the accounts page");
    }
}
