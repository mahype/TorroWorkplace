//! `torroworkplace ui`: the control centre – one place for everything that concerns the
//! whole suite (accounts, workspaces, apps, agents, approvals, inbox, general settings). Concept: `docs/settings.md`,
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
//! same way the calendar and the tasks app do. In wide windows the sidebar collapses to a rail of
//! icons (U.2: panel button or `[`, remembered in `ui.json`); medium windows always show that rail,
//! and the panel button opens the drawer. On macOS the panel button sits in the band right of the
//! traffic lights and "General" in its top right corner (U.10); elsewhere at the sidebar's top and
//! foot. The page sits under a 44-high header bar with
//! its title and actions, at most [`size::content_max()`] wide.
//!
//! Mini and strip tiles have no window frame: [`glance`] shows the status instead (design skill,
//! section 6).
//!
//! The pages are settings pages: `widgets::group`s of rows built with [`setting_row`] (design
//! skill, section 5), rows that open a page through [`pressable`] with a [`chevron`].

mod accounts;
mod agent_wizard;
mod agents;
mod approvals;
mod apps;
mod general;
mod glance;
mod inbox;
mod launch;
mod overview;
mod server_fields;
mod wizard;
mod workspaces;

use std::time::Duration;

use iced::theme::Mode;
use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{Text, button, column, container, responsive, row, scrollable, stack, svg};
use iced::{Alignment, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::tr;
use torro_core::settings::Settings;
use torro_gui::design::{self, Follow};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, Cover, SidebarMode, Surface, Switcher, TitleButtons};
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets::{self, Align, Anchor, MenuItem, MenuSection, Placement, Trail};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{style, typo};
use torro_inbox::{Rule, SourceInfo};
use torro_ipc::{AccountInfo, AppInfo, ApprovalInfo, ClientInfo, Request, Response, RuleHits, Status, client};
use torro_pim::Collection;
use torro_setups::TemplateSummary;

pub use launch::{Launch, Open};

/// The sections in sidebar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Overview,
    Accounts,
    Workspaces,
    Apps,
    /// The agents, hooks and scripts that use the API, and their rights (task A.6).
    Agents,
    /// What agents wait for and what was decided (task A.8).
    Approvals,
    /// How loud the inbox is and its lasting rules (task E3.7).
    Inbox,
    General,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::Overview,
        Section::Accounts,
        Section::Workspaces,
        Section::Apps,
        Section::Agents,
        Section::Approvals,
        Section::Inbox,
        Section::General,
    ];

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
            Section::Agents => tr("Agents & API"),
            Section::Approvals => tr("Approvals"),
            Section::Inbox => tr("Inbox"),
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
            // The suite has no robot icon; the sparkles stand for AI agents.
            Section::Agents => Icon::Sparkles,
            // A person who confirms: the agent asks, the person decides.
            Section::Approvals => Icon::UserCheck,
            // The same tray as the Inbox app's "For me".
            Section::Inbox => Icon::Inbox,
            Section::General => Icon::Settings,
        }
    }

    /// The word for `--section`.
    pub fn arg(self) -> &'static str {
        match self {
            Section::Overview => "overview",
            Section::Accounts => "accounts",
            Section::Workspaces => "workspaces",
            Section::Apps => "apps",
            Section::Agents => "agents",
            Section::Approvals => "approvals",
            Section::Inbox => "inbox",
            Section::General => "general",
        }
    }

    /// `--section accounts` and friends.
    pub fn from_arg(arg: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|section| section.arg() == arg)
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
    /// The approvals of the last day (`approvals.list_approvals`), open and decided, newest first.
    pub approvals: Vec<ApprovalInfo>,
    /// The agents, hooks and scripts with their rights (`clients.list_clients`), never with keys.
    pub clients: Vec<ClientInfo>,
    /// Every calendar (`calendar.list_calendars`; the app key sees hidden ones too), for the
    /// agents' selections.
    pub calendars: Vec<Collection>,
    /// Every task list, GitHub filter lists included (`tasks.list_lists`), for the agents'
    /// selections.
    pub task_lists: Vec<Collection>,
    /// The inbox sources with entries or muted (`inbox.list_sources`), for the agents' selections.
    pub sources: Vec<SourceInfo>,
    /// The lasting rules of the inbox (`inbox.list_rules`): remembered lanes and muted sources.
    pub rules: Vec<Rule>,
    /// How much each rule caught and since when (`inbox.list_rules`, task E8.8a), by the rule's id;
    /// empty from a core before E8.8a.
    pub rule_hits: Vec<RuleHits>,
    /// Why the last load failed (daemon not running, …).
    pub error: Option<String>,
}

/// What a page wants the shell to do.
#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "a request can carry a whole task and is handled once; boxing would only add an allocation"
)]
pub enum Action {
    None,
    /// Send this request to the daemon, then reload.
    Send(Request),
    /// Open a web page in the browser.
    OpenUrl(String),
    /// Jump to another section.
    Select(Section),
    /// Zoom every window of the suite one step (General › Zoom). Goes through the window's own
    /// zoom, so it applies at once and, with `--demo`, stays in the process.
    Zoom(zoom::Step),
    /// Put this text on the clipboard (a new key, task A.6).
    Copy(String),
    /// Find, connect or disconnect a coding agent's MCP entry (task A.7) on a blocking thread;
    /// the outcome goes to the agents page. The demo answers it itself and touches nothing.
    Setup(crate::client_setup::Job),
    /// List the design packs (`settings.list_designs`) for General › Design, with the look of each
    /// read from its folder ([`general::Pack::new`]). The demo lists its sample packs.
    ListDesigns,
    /// Draw with the design pack of this id (General › Design): this window at once, then saved
    /// (`settings.set_setting design`), which the other windows follow within two seconds. The
    /// demo only draws, with its sample pack.
    Design(String),
    /// Open the designs folder in the system's file manager, created first if it is missing. The
    /// demo opens nothing.
    OpenDesignsFolder,
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
    /// The start parameters: `--demo` keeps the window off the socket for good, `--appearance`
    /// beats the stored appearance.
    launch: Launch,
    section: Section,
    /// The menu that is open, if any.
    menu: Option<Menu>,
    /// Medium and narrow: the sidebar drawer is open.
    drawer: bool,
    /// The sidebar the user chose for the wide layout, kept in `ui.json`.
    sidebar: SidebarMode,
    /// The window size, for the size class in `update` (the panel button and `[` act on the stored
    /// choice only in the wide layout). `view` lays out with the size `responsive` hands over.
    size: Size,
    /// The core has answered at least once (successfully or not); until then the page shows the
    /// loading state instead of empty lists.
    loaded: bool,
    /// The desktop's light/dark preference, for the appearance "System".
    system_mode: Mode,
    /// The stored appearance. Kept apart from `data` so a core that does not answer for a moment
    /// (an empty `Data`) does not flip the colours.
    appearance: Appearance,
    /// The suite-wide zoom (Ctrl + Plus / Minus / 0 and General › Zoom), the factor of the whole
    /// window. `data.settings.zoom` follows it, so the General page shows the level the window
    /// uses, also while a change is being saved.
    zoom: Zoom,
    /// Follows the design pack of `data.settings` (task L.3): the page reloads the settings, so
    /// the design comes with them instead of a request of its own.
    design: Follow,
    /// The design pack chosen on the General page while its saving is on its way (task L.4). A
    /// reload that left before the save still carries the old one; it must not switch back.
    pending_design: Option<String>,
    data: Data,
    /// The last failed request, kept across reloads until the next success or section change.
    notice: Option<String>,
    overview: overview::State,
    accounts: accounts::State,
    workspaces: workspaces::State,
    apps: apps::State,
    agents: agents::State,
    approvals: approvals::State,
    inbox: inbox::State,
    general: general::State,
}

#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "an answer can carry a whole task and is handled once; boxing would only add an allocation"
)]
pub enum Message {
    /// The band above the content was pressed: drag the window (see [`shell::titlebar`]).
    DragWindow,
    Select(Section),
    /// Open or close a menu.
    ToggleMenu(Menu),
    /// Close the open menu and the drawer (a click next to them).
    CloseMenu,
    /// Open or close the sidebar drawer (medium and narrow).
    ToggleDrawer,
    /// The panel button at the top of the sidebar, or `[`: collapses or expands the sidebar (wide),
    /// opens or closes the drawer (medium, narrow).
    ToggleSidebar,
    /// The view state stored in `ui.json`, loaded at start.
    UiState(UiState),
    /// The window has this size (at start and after every resize).
    Resized(Size),
    /// The app switcher and its search and "New" buttons.
    Switcher(Switcher),
    Loaded(Box<Data>),
    Sent(Result<Response, String>),
    /// The desktop's light/dark preference (once at start, then whenever it changes).
    SystemMode(Mode),
    /// Reload everything (every few seconds, and "Retry" when the core does not answer).
    Tick,
    /// The zoom keys, the stored zoom and its badge.
    Zoom(zoom::Message),
    /// The fonts of a new design pack are loaded.
    Design(design::Message),
    /// The design packs for General › Design ([`Action::ListDesigns`]); `Err` when the core did
    /// not list them.
    Designs(Result<Vec<general::Pack>, String>),
    /// Escape: close the open menu, or else whatever dialog the current page has open.
    Escape,
    Overview(overview::Message),
    Accounts(accounts::Message),
    Workspaces(workspaces::Message),
    Apps(apps::Message),
    Agents(agents::Message),
    Approvals(approvals::Message),
    Inbox(inbox::Message),
    General(general::Message),
}

/// Runs the control centre with the start parameters of `torroworkplace ui` ([`Launch::parse`]).
/// Demo mode does not start the core either.
pub fn run(launch: Launch) -> iced::Result {
    let socket = torro_ipc::socket_path().filter(|_| !launch.demo);
    if let Some(socket) = &socket {
        torro_gui::source::ensure_core(socket);
    }
    // Before the settings load the fonts: the first frame is drawn with the chosen design.
    design::start(socket.as_deref(), launch.design.as_deref());
    iced::application(move || Ui::new(launch.clone()), Ui::update, Ui::view)
        .settings(torro_gui::settings())
        .title(|ui: &Ui| format!("Torro Workplace · {}", ui.section.label()))
        .theme(Ui::theme)
        .scale_factor(|ui: &Ui| ui.zoom.factor())
        .subscription(Ui::subscription)
        .window(torro_gui::window(AppId::WORKPLACE))
        .run()
}

impl Ui {
    fn new(launch: Launch) -> (Ui, Task<Message>) {
        let (open, demo) = (launch.open, launch.demo);
        let mut ui = Ui {
            section: launch.section,
            launch,
            menu: None,
            drawer: false,
            sidebar: SidebarMode::default(),
            size: torro_gui::window(AppId::WORKPLACE).size,
            loaded: false,
            system_mode: Mode::None,
            appearance: Appearance::System,
            // The demo keeps the zoom in the process, like everything else it changes.
            zoom: Zoom::new(torro_ipc::socket_path().filter(|_| !demo)),
            // Fed by the reloads, so it asks the core nothing itself.
            design: Follow::new(None),
            pending_design: None,
            data: Data::default(),
            notice: None,
            overview: overview::State::default(),
            accounts: accounts::State::default(),
            workspaces: workspaces::State::default(),
            apps: apps::State::default(),
            agents: agents::State::default(),
            approvals: approvals::State::default(),
            inbox: inbox::State::default(),
            general: general::State::default(),
        };
        if demo {
            // No service to ask: the sample data is there from the first frame.
            ui.data = launch::demo_data();
            ui.appearance = ui.data.settings.appearance;
            // The check mark of General › Design at the pack `--design` draws with.
            if let Some(id) = ui.launch.design.clone() {
                ui.data.settings.design = id;
            }
            ui.loaded = true;
        }
        if let Some(open) = open {
            ui.open(open);
        }
        // A start on the General page lists the design packs, as opening it does.
        let designs = match ui.section == Section::General && general::unlisted(&ui.general) {
            true => ui.perform(Action::ListDesigns),
            false => Task::none(),
        };
        // The size, and the traffic lights onto the title band's line (U.11).
        let size = Task::batch([
            iced::window::latest().and_then(iced::window::size).map(Message::Resized),
            torro_gui::place_traffic_lights(),
        ]);
        let task = Task::batch([
            ui.reload(),
            ui.zoom.fetch().map(Message::Zoom),
            iced::system::theme().map(Message::SystemMode),
            ui_state::load_task(AppId::WORKPLACE).map(Message::UiState),
            size,
            designs,
        ]);
        (ui, task)
    }

    /// Goes to the state `--open` asks for, the way a click would: through the pages' messages.
    fn open(&mut self, open: Open) {
        let first_account = accounts::listed(&self.data.accounts).next().map(|account| account.id.clone());
        let first_template = self.data.templates.first().map(|template| template.id.clone());
        let messages = match open {
            Open::Menu => {
                self.menu = Some(Menu::Nav);
                vec![]
            }
            Open::Drawer => {
                self.drawer = true;
                vec![]
            }
            // Without a sidebar the app switcher lives in the drawer.
            Open::Switcher => {
                self.menu = Some(Menu::Switcher);
                self.drawer = true;
                vec![]
            }
            Open::Wizard => vec![Message::Select(Section::Accounts), Message::Accounts(accounts::Message::AddAccount)],
            Open::WizardGithub | Open::WizardToken => {
                let mut messages = vec![
                    Message::Select(Section::Accounts),
                    Message::Accounts(accounts::Message::AddAccount),
                    Message::Accounts(accounts::Message::Wizard(wizard::Message::ChooseProvider(
                        wizard::Provider::GitHub,
                    ))),
                ];
                if open == Open::WizardToken {
                    messages.push(Message::Accounts(accounts::Message::Wizard(wizard::Message::PasteToken)));
                }
                messages
            }
            Open::WizardMail(demo) | Open::WizardAi(demo) => {
                let mut messages =
                    vec![Message::Select(Section::Accounts), Message::Accounts(accounts::Message::AddAccount)];
                messages.extend(
                    wizard::demo_messages(demo)
                        .into_iter()
                        .map(|message| Message::Accounts(accounts::Message::Wizard(message))),
                );
                messages
            }
            Open::ChangePassword => {
                let caldav = accounts::listed(&self.data.accounts).find(|account| account.kind == "caldav");
                let mut messages = vec![Message::Select(Section::Accounts)];
                if let Some(account) = caldav {
                    messages.push(Message::Accounts(accounts::Message::Open(account.id.clone())));
                    messages.push(Message::Accounts(accounts::Message::ChangePassword));
                }
                messages
            }
            Open::AiDetail => {
                let ai = accounts::listed(&self.data.accounts).find(|account| account.kind == "ai");
                let mut messages = vec![Message::Select(Section::Accounts)];
                messages.extend(ai.map(|account| Message::Accounts(accounts::Message::Open(account.id.clone()))));
                messages
            }
            Open::GithubDetail => {
                let github = accounts::listed(&self.data.accounts).find(|account| account.kind == "github");
                let mut messages = vec![Message::Select(Section::Accounts)];
                messages.extend(github.map(|account| Message::Accounts(accounts::Message::Open(account.id.clone()))));
                messages
            }
            Open::MailDetail | Open::MailDetailProblem | Open::MailPassword | Open::MailServers => {
                // The first demo mail account is the healthy one with calendars, the second the
                // one with a problem.
                let nth = usize::from(open == Open::MailDetailProblem);
                let mail = accounts::listed(&self.data.accounts).filter(|account| account.kind == "mail").nth(nth);
                let mut messages = vec![Message::Select(Section::Accounts)];
                if let Some(account) = mail {
                    messages.push(Message::Accounts(accounts::Message::Open(account.id.clone())));
                    match open {
                        Open::MailPassword => messages.push(Message::Accounts(accounts::Message::ChangePassword)),
                        Open::MailServers => messages.push(Message::Accounts(accounts::Message::ChangeServers)),
                        _ => {}
                    }
                }
                messages
            }
            Open::AccountDetail | Open::Remove => {
                let mut messages = vec![Message::Select(Section::Accounts)];
                messages.extend(first_account.map(|id| Message::Accounts(accounts::Message::Open(id))));
                if open == Open::Remove {
                    messages.push(Message::Accounts(accounts::Message::AskRemove));
                }
                messages
            }
            Open::DeleteTemplate | Open::RenameTemplate => {
                let mut messages = vec![Message::Select(Section::Workspaces)];
                messages.extend(first_template.map(|id| {
                    Message::Workspaces(match open {
                        Open::DeleteTemplate => workspaces::Message::AskDelete(id),
                        _ => workspaces::Message::StartRename(id),
                    })
                }));
                messages
            }
            // The demo's sample approvals gone, so the page shows its empty state.
            Open::ApprovalsEmpty => {
                self.data.approvals.clear();
                vec![Message::Select(Section::Approvals)]
            }
            Open::AgentDetail | Open::ScriptDetail | Open::HookDetail | Open::NewKey | Open::Disconnect => {
                let kind = match open {
                    Open::ScriptDetail => "script",
                    Open::HookDetail => torro_ipc::api::HOOK_CLIENT,
                    // The agent with its own rights shows the most: levels, admin, selections.
                    _ => "codex",
                };
                let client = self.data.clients.iter().find(|client| client.kind == kind).map(|c| c.id.clone());
                let mut messages = vec![Message::Select(Section::Agents)];
                messages.extend(client.map(|id| Message::Agents(agents::Message::Open(id))));
                match open {
                    Open::NewKey => messages.push(Message::Agents(agents::Message::NewKey)),
                    Open::Disconnect => messages.push(Message::Agents(agents::Message::AskDisconnect)),
                    _ => {}
                }
                messages
            }
            Open::Inbox => vec![Message::Select(Section::Inbox)],
            // The design packs: the demo's sample packs, one of them broken.
            Open::Design => vec![Message::Select(Section::General)],
            // No rules, so the rules group says where they come from.
            Open::InboxEmpty => {
                self.data.rules.clear();
                vec![Message::Select(Section::Inbox)]
            }
            // No clients, so the page shows its empty state.
            Open::AgentsEmpty => {
                self.data.clients.clear();
                vec![Message::Select(Section::Agents)]
            }
            // The steps of "Connect agent", with the demo's detection: Claude Code to connect, or
            // a script named "nightly-backup" whose key is shown.
            Open::AgentWizard | Open::AgentWizardRights | Open::AgentWizardDone | Open::AgentWizardKey => {
                use agent_wizard::{Choice, Message as Step};
                let wizard = |message| Message::Agents(agents::Message::Wizard(message));
                let mut messages = vec![Message::Select(Section::Agents), Message::Agents(agents::Message::Connect)];
                match open {
                    Open::AgentWizardRights | Open::AgentWizardDone => {
                        let claude = Choice::Agent(crate::client_setup::Target::ClaudeCode);
                        messages.push(wizard(Step::Choose(claude)));
                    }
                    Open::AgentWizardKey => {
                        messages.push(wizard(Step::Choose(Choice::Script)));
                        messages.push(wizard(Step::Name("nightly-backup".into())));
                    }
                    _ => {}
                }
                if matches!(open, Open::AgentWizardDone | Open::AgentWizardKey) {
                    messages.push(wizard(Step::Connect));
                }
                messages
            }
        };
        for message in messages {
            let _ = self.update(message);
        }
    }

    /// Loads everything from the service; the demo has none and keeps its sample data.
    fn reload(&self) -> Task<Message> {
        if self.launch.demo { Task::none() } else { load() }
    }

    /// The Iced theme: the appearance of `--appearance`, else the stored one (the page reloads it
    /// with every `Data`), with "System" following the desktop.
    fn theme(&self) -> Theme {
        theme::for_appearance(self.launch.appearance.unwrap_or(self.appearance), self.system_mode)
    }

    fn subscription(&self) -> Subscription<Message> {
        // Faster while an account is being set up, so the page follows the browser sign-in.
        let every = if self.accounts.busy() { Duration::from_secs(1) } else { Duration::from_secs(3) };
        let mut all = vec![
            iced::system::theme_changes().map(Message::SystemMode),
            iced::event::listen_with(keys),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            self.zoom.subscription().map(Message::Zoom),
            // Ticks only while a change of an agent's rights waits to be saved.
            agents::subscription(&self.agents).map(Message::Agents),
        ];
        // The demo has nothing to reload.
        if !self.launch.demo {
            all.push(iced::time::every(every).map(|_| Message::Tick));
        }
        Subscription::batch(all)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let action = match message {
            Message::DragWindow => return torro_gui::drag_window(),
            Message::Select(section) => {
                self.select(section);
                self.notice = None;
                // The packs may have changed in the folder since the page was last open.
                if section == Section::General { Action::ListDesigns } else { Action::None }
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
            Message::ToggleSidebar => {
                if !shell::has_sidebar(layout::of(self.size)) {
                    return self.update(Message::ToggleDrawer);
                }
                self.close_menus();
                self.sidebar = self.sidebar.toggled();
                // The demo stores nothing.
                if self.launch.demo {
                    return Task::none();
                }
                return ui_state::save_task(AppId::WORKPLACE, UiState { sidebar: self.sidebar });
            }
            Message::UiState(state) => {
                self.sidebar = state.sidebar;
                Action::None
            }
            Message::Resized(size) => {
                self.size = size;
                Action::None
            }
            Message::Switcher(switcher) => self.switcher(switcher),
            Message::Loaded(data) => {
                self.data = *data;
                self.loaded = true;
                if self.data.error.is_none() {
                    self.appearance = self.data.settings.appearance;
                }
                self.data.settings.zoom = self.zoom.level();
                // A choice being saved stays drawn until a reload carries it.
                if let Some(pending) = &self.pending_design {
                    if self.data.error.is_none() && self.data.settings.design == *pending {
                        self.pending_design = None;
                    } else {
                        self.data.settings.design = pending.clone();
                    }
                }
                // `--design` of the demo wins over the stored one, like `--appearance`.
                let design = match self.data.error.is_none() && self.launch.design.is_none() {
                    true => self.design.apply(&self.data.settings.design),
                    false => None,
                };
                agents::loaded(&mut self.agents);
                // A mail account's page follows changes from outside while nothing is typed.
                accounts::loaded(&mut self.accounts, &self.data);
                // The add-account dialog follows the sign-in through the reloaded accounts.
                if let Some(wizard) = self.accounts.wizard.as_mut() {
                    wizard::observe(wizard, &self.data);
                }
                // No canvas keeps a cache: the next frame draws with the new design, the task
                // loads its fonts.
                if let Some(task) = design {
                    return task.map(Message::Design);
                }
                Action::None
            }
            Message::Sent(result) => {
                // The add-account dialog shows the outcome of its own requests itself.
                let in_dialog = accounts::answered(&mut self.accounts, &result);
                approvals::answered(&mut self.approvals);
                inbox::answered(&mut self.inbox);
                // The "Connect agent" dialog shows its errors itself, and may go on with a job.
                let next = agents::answered(&mut self.agents, &result);
                // A refused design (the core checks it too) goes back to the stored one with the
                // next reload; the notice says why.
                if result.is_err() {
                    self.pending_design = None;
                }
                self.notice = if in_dialog || next.is_some() { None } else { result.err() };
                let next = next.map_or_else(Task::none, |action| self.perform(action));
                return Task::batch([next, self.reload()]);
            }
            Message::SystemMode(mode) => {
                self.system_mode = mode;
                Action::None
            }
            // The zoom is asked for apart from `Data`, so a reload from before a zoom change does
            // not bring back the old level (`zoom::Message::Stored`).
            Message::Tick => return Task::batch([self.reload(), self.zoom.fetch().map(Message::Zoom)]),
            Message::Zoom(message) => return self.zoom(message),
            Message::Design(message) => {
                return self.design.update(message).unwrap_or_else(Task::none).map(Message::Design);
            }
            Message::Designs(designs) => {
                // The samples write "Aa" in each pack's text font, so those fonts are loaded once.
                let fonts = designs
                    .iter()
                    .flatten()
                    .filter_map(|pack| pack.design.as_deref())
                    .map(|design| design::load_fonts(design).map(Message::Design))
                    .collect::<Vec<_>>();
                general::listed(&mut self.general, designs);
                return Task::batch(fonts);
            }
            Message::Escape => {
                if self.menu.is_some() || self.drawer {
                    self.close_menus();
                    return Task::none();
                }
                let page = match self.section {
                    Section::Accounts => accounts::escape(&self.accounts).map(Message::Accounts),
                    Section::Workspaces => workspaces::escape(&self.workspaces).map(Message::Workspaces),
                    Section::Agents => agents::escape(&self.agents).map(Message::Agents),
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
            Message::Agents(m) => agents::update(&mut self.agents, m, &self.data),
            Message::Approvals(m) => approvals::update(&mut self.approvals, m, &self.data),
            Message::Inbox(m) => inbox::update(&mut self.inbox, m, &self.data),
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
                Action::Send(Request::OpenApp { app: app.to_string(), target: None })
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
            // The demo is closed off: no request reaches the service, no page opens and nothing
            // reaches the clipboard, so nothing is stored and nothing starts. A few requests get a
            // sample answer from the process itself (an agent's activity, a new key), so their
            // states can be shown.
            Action::Send(request) if self.launch.demo => match launch::demo_answer(&request, &self.data) {
                Some(response) => self.update(Message::Sent(Ok(response))),
                None => Task::none(),
            },
            Action::OpenUrl(_) | Action::Copy(_) if self.launch.demo => Task::none(),
            Action::Setup(job) if self.launch.demo => {
                self.update(Message::Agents(agents::Message::Setup(crate::client_setup::demo(&job))))
            }
            Action::Send(request) => send(request),
            Action::OpenUrl(url) => {
                if let Err(err) = webbrowser::open(&url) {
                    self.data.error = Some(err.to_string());
                }
                Task::none()
            }
            Action::Select(section) => {
                self.select(section);
                if section == Section::General {
                    return self.perform(Action::ListDesigns);
                }
                Task::none()
            }
            Action::ListDesigns if self.launch.demo => self.update(Message::Designs(Ok(launch::demo_designs()))),
            Action::ListDesigns => Task::perform(list_designs(), Message::Designs),
            Action::Design(id) => self.choose_design(id),
            Action::OpenDesignsFolder if self.launch.demo => Task::none(),
            Action::OpenDesignsFolder => {
                if let Err(problem) = open_designs_folder() {
                    self.notice = Some(problem);
                }
                Task::none()
            }
            Action::Zoom(step) => self.zoom(zoom::Message::Step(step)),
            Action::Copy(text) => iced::clipboard::write(text),
            Action::Setup(job) => Task::perform(crate::client_setup::perform(job), |outcome| {
                Message::Agents(agents::Message::Setup(outcome))
            }),
        }
    }

    /// Draws this window with the design pack `id` at once and saves the choice; the other windows
    /// follow the setting (`design::Follow`). The demo saves nothing and draws its sample pack
    /// from memory, since it has no folder.
    fn choose_design(&mut self, id: String) -> Task<Message> {
        self.data.settings.design = id.clone();
        if self.launch.demo {
            let Some(chosen) = general::design_of(&self.general, &id) else { return Task::none() };
            let fonts = design::load_fonts(&chosen).map(Message::Design);
            design::set((*chosen).clone());
            return fonts;
        }
        self.pending_design = Some(id.clone());
        let fonts = self.design.apply(&id).unwrap_or_else(Task::none).map(Message::Design);
        let save = send(Request::SetSetting { key: torro_ipc::SettingKey::Design, value: serde_json::json!(id) });
        Task::batch([fonts, save])
    }

    /// Hands `message` to the window's zoom. The window keeps its pixels, so the size kept for
    /// `update` changes with the zoom; the General page shows the new level.
    fn zoom(&mut self, message: zoom::Message) -> Task<Message> {
        let before = self.zoom.factor();
        let task = self.zoom.update(message).map(Message::Zoom);
        self.data.settings.zoom = self.zoom.level();
        if self.zoom.factor() == before {
            return task;
        }
        // At once by arithmetic, then exactly from the window.
        self.size = zoom::rescale(self.size, before, self.zoom.factor());
        let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
        Task::batch([task, size])
    }

    /// The window in the layout of its real size: `responsive` hands over the size Iced laid out,
    /// so the first frame already fits the tile (no assumed start width).
    /// The zoom badge lies over everything.
    fn view(&self) -> Element<'_, Message> {
        let window = responsive(move |size| match layout::of(size) {
            // Tiles too small for a window show the status as a glance, without any chrome.
            SizeClass::Mini => glance::mini(&self.data, self.loaded, size),
            SizeClass::Strip => glance::strip(&self.data, self.loaded, size),
            class => self.window(class),
        });
        // The glances lie on the panel even while loading, so the layout alone decides the band.
        let surface = Surface::of(layout::of(self.size));
        let content = self.zoom.view(window);
        let (cover, buttons) = (self.cover(), self.title_buttons());
        widgets::ctrl_guard(shell::titlebar(content, self.zoom.factor(), surface, cover, buttons, Message::DragWindow))
    }

    /// The panel button and "General" in the band above the window (U.10); a glance has neither.
    fn title_buttons(&self) -> TitleButtons<Message> {
        let class = layout::of(self.size);
        if matches!(class, SizeClass::Mini | SizeClass::Strip) {
            return TitleButtons::none();
        }
        let label = shell::sidebar_button_label(class, self.sidebar, self.drawer);
        TitleButtons::none().sidebar(label, Message::ToggleSidebar).settings(Message::Select(Section::General))
    }

    /// What lies over the top of the window, which the title band repeats: the page's dialog, else
    /// the drawer. A glance has neither.
    fn cover(&self) -> Cover {
        let class = layout::of(self.size);
        if matches!(class, SizeClass::Mini | SizeClass::Strip) {
            return Cover::Nothing;
        }
        Cover::dialog(self.page_overlay().is_some()).or(Cover::drawer(class, self.drawer))
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
        let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
            .then(|| self.sidebar(SidebarMode::Rail, &switcher));
        shell::shell(
            class,
            self.sidebar(SidebarMode::Expanded, &switcher),
            self.header(class, &nav),
            self.body(class),
            None,
        )
        .rail(rail)
        .drawer(self.drawer, Message::CloseMenu)
        .overlay(layer)
        .into()
    }

    /// The count after a section in the sidebar and the menu: accounts, templates and the
    /// approvals that wait for the person.
    fn trail(&self, section: Section) -> Trail {
        let count = match section {
            Section::Accounts => accounts::listed(&self.data.accounts).count(),
            Section::Workspaces => self.data.templates.len(),
            Section::Agents => self.data.clients.len(),
            Section::Approvals => approvals::pending(&self.data.approvals).count(),
            _ => 0,
        };
        if count > 0 { Trail::Count(count) } else { Trail::None }
    }

    /// The sidebar in `mode`: app switcher, the sections, and "General" with the version at the
    /// foot. The rail (U.2) has no room for the version line. Where the window has the band (macOS),
    /// "General" sits there in the top right corner instead (U.10).
    fn sidebar(&self, mode: SidebarMode, switcher: &Anchor) -> Element<'_, Message> {
        let item = |section: Section| {
            widgets::sidebar_item_in(
                mode,
                section.icon(),
                section.label(),
                self.trail(section),
                section == self.section,
                Message::Select(section),
            )
        };
        let version = format!("Torro Workplace · {} {}", tr("Version"), env!("CARGO_PKG_VERSION"));
        let version = container(typo::text(typo::meta(), version).style(theme::text_3).wrapping(Wrapping::None))
            .center_y(size::row_sidebar())
            .padding(style::ITEM_PADDING)
            .clip(true);
        let mut footer = Vec::new();
        if !shell::has_band() {
            footer.push(item(Section::General));
        }
        if mode == SidebarMode::Expanded {
            footer.push(version.into());
        }
        shell::sidebar(
            shell::sidebar_head(mode, AppId::WORKPLACE, switcher, Message::Switcher, Message::ToggleSidebar),
            Section::nav().map(item).collect(),
            footer,
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
            Section::Agents => agents::header(&self.agents, &self.data, class).map(Message::Agents),
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
            Section::Agents => agents::overlay(&self.agents, &self.data).map(|e| e.map(Message::Agents)),
            _ => None,
        }
    }

    /// The header bar: the title (wide) or the section menu (medium, narrow; with the drawer button
    /// in narrow, where no rail has it, unless the band has it); on a page below the section a back button and its name; the
    /// page's actions right.
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
                (shell::sidebar_mode(class, self.sidebar).is_none() && !shell::has_band())
                    .then(|| shell::sidebar_toggle(Message::ToggleDrawer))
                    .into_iter()
                    .collect(),
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
            Section::Agents => agents::view(&self.agents, &self.data).map(Message::Agents),
            Section::Approvals => approvals::view(&self.approvals, &self.data).map(Message::Approvals),
            Section::Inbox => inbox::view(&self.inbox, &self.data).map(Message::Inbox),
            Section::General => general::view(&self.general, &self.data).map(Message::General),
        };
        let notices = [&self.notice, &self.data.error].into_iter().flatten().map(|problem| notice(problem.clone()));
        let page = column(notices).push(page).spacing(space::l());
        let page = if class == SizeClass::Medium {
            // Flush with the window: the page starts at the margin (24), where the ink of the header's
            // first symbol starts (16 padding, 6 around the symbol, 2 inside the icon), instead of
            // floating in the middle between header and window edge.
            container(container(page).width(Fill).max_width(size::content_max())).padding(page_margin(class))
        } else {
            container(widgets::content(size::content_max(), page)).padding(page_margin(class))
        };
        scrollable(page)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .height(Fill)
            .into()
    }
}

/// The margin around a page: the design's page padding (24 in the standard), in a narrow window 16,
/// in a tile 12 (design skill, section 6).
fn page_margin(class: SizeClass) -> Padding {
    match class {
        SizeClass::Wide | SizeClass::Medium => layout::page_padding(),
        SizeClass::Narrow => Padding::new(space::l()),
        SizeClass::Mini | SizeClass::Strip => Padding::new(space::m()),
    }
}

/// "Accounts › cloud.example.org": the section in `text_3`, the page below it in `BODY_STRONG`.
fn crumb<'a>(section: Section, name: String) -> Element<'a, Message> {
    let chevron = icon::icon(Icon::ChevronRight, icon::SIZE_SM)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    row![
        typo::text(typo::body(), section.label()).style(theme::text_3).wrapping(Wrapping::None),
        chevron,
        typo::text(typo::body_strong(), name).wrapping(Wrapping::None),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center)
    .into()
}

/// A failed request or load above the page: an alert icon in the error colour, the message in
/// `text` – the colour is only in the icon.
fn notice<'a>(problem: String) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    row![alert, typo::text(typo::body(), problem)].spacing(space::s()).align_y(Alignment::Center).into()
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
    let mut lines = column![one_line(typo::text(typo::body(), label))];
    if let Some(detail) = detail {
        lines = lines.push(one_line(typo::text(typo::meta(), detail).style(theme::text_2)));
    }
    let text = row![].push(mark).push(lines.width(Fill)).spacing(space::m()).align_y(Alignment::Center);
    row_frame(row![text.width(Fill), control.into()].spacing(space::l()).align_y(Alignment::Center), tall)
}

/// `content` as one line of a settings group: 44 high (56 with two lines of text), 16 at the sides.
fn row_frame<'a, M: 'a>(content: impl Into<Element<'a, M>>, tall: bool) -> Element<'a, M> {
    let height = if tall { size::header() + space::m() } else { size::header() };
    container(content).width(Fill).center_y(height).padding([0.0, space::l()]).into()
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
    let card = container(content).padding(space::xl()).width(Fill).style(style::container::popover);
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
    .spacing(space::s())
    .align_y(Alignment::Center);
    let content =
        column![typo::text(typo::title(), title), typo::text(typo::body(), consequence).style(theme::text_2), buttons]
            .spacing(space::l());
    dialog(content, widgets::POPOVER_WIDTH, on_cancel)
}

/// Escape, wherever the focus is (a text input keeps it to itself otherwise), and `[` for the
/// sidebar when no text field took it.
fn keys(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event else { return None };
    match key {
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => Some(Message::Escape),
        key if status == iced::event::Status::Ignored && shell::is_sidebar_key(&key, modifiers) => {
            Some(Message::ToggleSidebar)
        }
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
            if let Ok(Response::Approvals { approvals }) = client::call(&socket, &Request::ListApprovals).await {
                data.approvals = approvals;
            }
            if let Ok(Response::Clients { clients }) = client::call(&socket, &Request::ListClients).await {
                data.clients = clients;
            }
            if let Ok(Response::Collections { collections }) =
                client::call(&socket, &Request::CalendarCollections).await
            {
                data.calendars = collections;
            }
            if let Ok(Response::Collections { collections }) = client::call(&socket, &Request::TaskCollections).await {
                data.task_lists = collections;
            }
            if let Ok(Response::Sources { sources }) = client::call(&socket, &Request::ListSources).await {
                data.sources = sources;
            }
            if let Ok(Response::Rules { rules, hits }) = client::call(&socket, &Request::ListRules).await {
                data.rules = rules;
                data.rule_hits = hits;
            }
            data
        },
        |data| Message::Loaded(Box::new(data)),
    )
}

/// Lists the design packs (`settings.list_designs`) and reads the look of each one that can be
/// used from its folder, the way the windows read the design they draw with.
async fn list_designs() -> Result<Vec<general::Pack>, String> {
    let socket = torro_ipc::socket_path().ok_or_else(|| tr("No home directory").to_string())?;
    let designs = match client::call(&socket, &Request::ListDesigns).await {
        Ok(Response::Designs { designs }) => designs,
        Ok(Response::Error { message, .. }) => return Err(message),
        Ok(other) => return Err(format!("unexpected answer: {other:?}")),
        Err(err) => return Err(err.to_string()),
    };
    // Fonts are opened to check them: off the window's thread.
    let read = move || {
        let dir = torro_core::design::dir();
        designs
            .into_iter()
            .map(|info| {
                // Without a home directory only the standard exists; it needs no folder.
                let folder = dir.as_deref().unwrap_or(std::path::Path::new(""));
                let design = info.valid.then(|| torro_core::design::load(folder, &info.id).ok()).flatten();
                general::Pack::new(info, design)
            })
            .collect()
    };
    tokio::task::spawn_blocking(read).await.map_err(|err| err.to_string())
}

/// Creates the designs folder if it is missing – with the example packs, as the core's first start
/// does (`designs::install_examples`) – and opens it with the system's file manager: `open` on
/// macOS, the Explorer on Windows, `xdg-open` elsewhere (as the inbox opens a mail's folder).
/// Returns what went wrong, for the notice above the page.
fn open_designs_folder() -> Result<(), String> {
    let dir = torro_core::design::dir().ok_or_else(|| tr("No home directory").to_string())?;
    crate::designs::install_examples();
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    let mut child = std::process::Command::new(program).arg(&dir).spawn().map_err(|err| err.to_string())?;
    // Waited for on a thread of its own, so it leaves no zombie behind.
    std::thread::spawn(move || child.wait());
    Ok(())
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

    fn ui(section: Section) -> Ui {
        Ui::new(Launch { section, ..Launch::default() }).0
    }

    /// A control centre started with `torroworkplace ui <words>`.
    fn started(words: &[&str]) -> Ui {
        let args: Vec<String> = words.iter().map(|word| word.to_string()).collect();
        Ui::new(Launch::parse(&args).expect("valid parameters")).0
    }

    #[test]
    fn the_title_band_repeats_what_lies_over_the_window() {
        let mut ui = started(&["--demo"]);
        let _ = ui.update(Message::Resized(Size::new(800.0, 600.0)));
        assert_eq!(ui.cover(), Cover::Nothing);
        ui.drawer = true;
        assert_eq!(ui.cover(), Cover::Drawer, "the drawer over a medium window");
        let _ = ui.update(Message::Resized(Size::new(240.0, 160.0)));
        assert_eq!(ui.cover(), Cover::Nothing, "a glance shows no drawer");

        let mut remove = started(&["--demo", "--open", "remove"]);
        remove.drawer = true;
        for size in [Size::new(1600.0, 1000.0), Size::new(800.0, 600.0), Size::new(420.0, 780.0)] {
            let _ = remove.update(Message::Resized(size));
            assert_eq!(remove.cover(), Cover::Dialog, "{size:?}: the dialog dims the whole window");
        }
    }

    #[test]
    fn the_sidebar_collapses_in_wide_and_opens_the_drawer_elsewhere() {
        let mut ui = started(&["--demo"]);
        assert_eq!(ui.sidebar, SidebarMode::Expanded, "expanded until ui.json says otherwise");
        let _ = ui.update(Message::UiState(UiState { sidebar: SidebarMode::Rail }));
        let _ = ui.update(Message::Resized(Size::new(1440.0, 900.0)));
        let _ = ui.update(Message::ToggleSidebar);
        assert_eq!((ui.sidebar, ui.drawer), (SidebarMode::Expanded, false), "wide: the choice flips");
        for size in [Size::new(800.0, 600.0), Size::new(420.0, 780.0)] {
            let _ = ui.update(Message::Resized(size));
            let _ = ui.update(Message::ToggleSidebar);
            assert!(ui.drawer, "{size:?}: the panel button opens the drawer");
            let _ = ui.update(Message::ToggleSidebar);
            assert!(!ui.drawer, "{size:?}: and closes it");
            assert_eq!(ui.sidebar, SidebarMode::Expanded, "{size:?}: the wide choice stays");
        }
    }

    #[test]
    fn the_bracket_toggles_the_sidebar_unless_a_field_took_it() {
        use iced::keyboard::{Event, Key, Location, Modifiers, key::Physical};
        let press = || {
            iced::Event::Keyboard(Event::KeyPressed {
                key: Key::Character("[".into()),
                modified_key: Key::Character("[".into()),
                physical_key: Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
                location: Location::Standard,
                modifiers: Modifiers::empty(),
                text: Some("[".into()),
                repeat: false,
            })
        };
        let id = iced::window::Id::unique();
        assert!(matches!(keys(press(), iced::event::Status::Ignored, id), Some(Message::ToggleSidebar)));
        assert!(keys(press(), iced::event::Status::Captured, id).is_none(), "typed into a text field");
    }

    #[test]
    fn the_window_builds_with_the_rail_in_every_layout() {
        let mut ui = started(&["--demo"]);
        for sidebar in [SidebarMode::Expanded, SidebarMode::Rail] {
            let _ = ui.update(Message::UiState(UiState { sidebar }));
            for class in [SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
                let _ = ui.window(class);
            }
        }
    }

    fn loaded(appearance: Appearance, error: Option<&str>) -> Message {
        let settings = Settings { appearance, ..Settings::default() };
        Message::Loaded(Box::new(Data { settings, error: error.map(String::from), ..Data::default() }))
    }

    #[test]
    fn theme_follows_the_stored_appearance() {
        let mut ui = ui(Section::Overview);
        let _ = ui.update(Message::SystemMode(Mode::Light));
        assert_eq!(ui.theme(), theme::light(), "System follows the desktop");
        let _ = ui.update(loaded(Appearance::Dark, None));
        assert_eq!(ui.theme(), theme::theme());
        let _ = ui.update(loaded(Appearance::Light, None));
        let _ = ui.update(Message::SystemMode(Mode::Dark));
        assert_eq!(ui.theme(), theme::light(), "an explicit choice ignores the desktop");
    }

    /// The design pack comes with the settings (task L.3): the window draws with it from the next
    /// frame, a broken one gives the standard, an unreachable core changes nothing, and `--design`
    /// of the demo wins.
    /// Held by the tests that switch the process's design, so they do not see each other's.
    static SWITCHING: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn the_stored_design_is_drawn_and_the_demo_parameter_wins() {
        let _switching = SWITCHING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().expect("temp dir");
        // The standard's values under another id, so the tests beside this one see the same tokens.
        for (id, toml) in [
            ("twin", "format = 1\n[about]\nname = \"Torro\"\nversion = \"1.0.0\"\n"),
            ("pale", "format = 1\n[about]\nname = \"Pale\"\nversion = \"1.0.0\"\n[colors.dark]\ntext = \"#202020\"\n"),
        ] {
            std::fs::create_dir_all(dir.path().join(id)).expect("pack folder");
            std::fs::write(dir.path().join(id).join("design.toml"), toml).expect("design.toml");
        }
        let with_design = |id: &str, error: Option<&str>| {
            let settings = Settings { design: id.into(), ..Settings::default() };
            Message::Loaded(Box::new(Data { settings, error: error.map(String::from), ..Data::default() }))
        };
        let mut ui = ui(Section::General);
        ui.design = Follow::new(None).in_dir(dir.path().to_path_buf());
        assert!(ui.update(with_design("twin", None)).units() > 0, "the new design's fonts are loaded");
        assert_eq!(design::active().id, "twin");
        let _ = ui.update(with_design("torro", Some("core is not running")));
        assert_eq!(design::active().id, "twin", "an empty answer keeps the design");
        let _ = ui.update(with_design("pale", None));
        assert!(design::active().is_standard(), "a broken pack is never drawn");

        let mut demo = started(&["--demo", "--design", "torro"]);
        demo.design = Follow::new(None).in_dir(dir.path().to_path_buf());
        let _ = demo.update(with_design("twin", None));
        assert!(design::active().is_standard(), "the parameter beats the stored design");
    }

    /// General › Design (task L.4): opening the page lists the packs; the demo has its samples,
    /// one of them broken.
    #[test]
    fn the_design_state_lists_the_demo_packs() {
        let demo = started(&["--demo", "--open", "design"]);
        assert_eq!(demo.section, Section::General);
        let ids: Vec<&str> = ["torro", "harbor", "paper", "neon"]
            .into_iter()
            .filter(|id| general::design_of(&demo.general, id).is_some())
            .collect();
        assert_eq!(ids, ["torro", "harbor", "paper"], "the broken one has no look to draw with");

        let mut real = ui(Section::Overview);
        assert!(general::unlisted(&real.general));
        assert!(real.update(Message::Select(Section::General)).units() > 0, "opening the page asks the core");
        assert!(real.update(Message::Select(Section::Accounts)).units() == 0, "other pages do not");
    }

    /// A design chosen on the General page is drawn in this window at once and saved; a reload that
    /// left before the save does not switch back, a refusal does.
    #[test]
    fn a_chosen_design_is_drawn_at_once_and_saved() {
        let _switching = SWITCHING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().expect("temp dir");
        // The standard's values under an id of its own, so the tests beside this one see the same
        // tokens and no other test has loaded its fonts.
        std::fs::create_dir_all(dir.path().join("chosen")).expect("pack folder");
        std::fs::write(
            dir.path().join("chosen/design.toml"),
            "format = 1\n[about]\nname = \"Torro\"\nversion = \"1.0.0\"\n",
        )
        .expect("design.toml");
        let with_design = |id: &str| {
            let settings = Settings { design: id.into(), ..Settings::default() };
            Message::Loaded(Box::new(Data { settings, ..Data::default() }))
        };
        let mut ui = ui(Section::General);
        ui.design = Follow::new(None).in_dir(dir.path().to_path_buf());

        let task = ui.update(Message::General(general::Message::Design("chosen".into())));
        assert_eq!(design::active().id, "chosen", "drawn at once, before the core answers");
        assert!(task.units() >= 2, "its fonts and the request that saves it");
        assert_eq!(ui.data.settings.design, "chosen", "the check mark moves at once");

        let _ = ui.update(with_design("torro"));
        assert_eq!(design::active().id, "chosen", "a reload from before the save does not switch back");
        assert_eq!(ui.data.settings.design, "chosen");
        let _ = ui.update(with_design("chosen"));
        assert_eq!(ui.pending_design, None, "saved");

        let _ = ui.update(Message::General(general::Message::Design("torro".into())));
        assert!(design::active().is_standard());
        let _ = ui.update(Message::Sent(Err("the core refused it".into())));
        let _ = ui.update(with_design("chosen"));
        assert_eq!(design::active().id, "chosen", "a refused choice gives way to the stored one");
        assert_eq!(ui.notice.as_deref(), Some("the core refused it"));

        design::set(torro_core::design::Design::standard());
    }

    /// The demo has no folder and no core: a choice draws its sample pack from memory and sends
    /// nothing.
    #[test]
    fn the_demo_draws_a_chosen_sample_and_saves_nothing() {
        let _switching = SWITCHING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut demo = started(&["--demo", "--open", "design"]);
        demo.data.settings.design = "paper".into();
        let _ = demo.update(Message::General(general::Message::Design("torro".into())));
        assert_eq!(demo.data.settings.design, "torro");
        assert!(design::active().is_standard());
        assert_eq!(demo.pending_design, None, "nothing is saved, so nothing waits");
        let _ = demo.update(Message::General(general::Message::OpenDesignsFolder));
        assert_eq!(demo.notice, None, "the demo opens nothing");
    }

    #[test]
    fn an_unreachable_core_keeps_the_colours() {
        let mut ui = ui(Section::Overview);
        let _ = ui.update(loaded(Appearance::Light, None));
        let _ = ui.update(loaded(Appearance::System, Some("core is not running")));
        assert_eq!(ui.theme(), theme::light());
    }

    #[test]
    fn the_page_loads_until_the_core_answers() {
        let mut ui = ui(Section::Accounts);
        assert!(!ui.loaded);
        let _ = ui.update(loaded(Appearance::System, Some("core is not running")));
        assert!(ui.loaded, "a failed answer ends the loading state too");
    }

    #[test]
    fn choosing_a_section_closes_menu_and_drawer() {
        let mut ui = ui(Section::Overview);
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
        let mut ui = ui(Section::Overview);
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
        let mut ui = ui(Section::Overview);
        ui.menu = Some(Menu::Switcher);
        let action = ui.switcher(Switcher::OpenApp(AppId::CALENDAR.binary));
        assert!(matches!(action, Action::Send(Request::OpenApp { ref app, .. }) if app == "torrocalendar"));
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
        assert_eq!(page_margin(SizeClass::Wide), layout::page_padding());
        assert_eq!(page_margin(SizeClass::Medium), Padding::new(space::xl()), "24 in the standard design");
        assert_eq!(page_margin(SizeClass::Narrow), Padding::new(space::l()));
        assert_eq!(page_margin(SizeClass::Mini), Padding::new(space::m()));
    }

    #[test]
    fn the_window_builds_in_every_class_and_state() {
        let mut ui = ui(Section::Accounts);
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
        let mut ui = ui(Section::Accounts);
        assert!(ui.page_overlay().is_none());
        let _ = ui.update(Message::Accounts(accounts::Message::AddAccount));
        assert!(ui.page_overlay().is_some(), "the add-account dialog is a window layer");
        let _ = ui.update(Message::Select(Section::Overview));
        assert!(ui.page_overlay().is_none(), "another section shows no dialog of the accounts page");
    }

    #[test]
    fn the_approvals_section_counts_the_open_ones_and_decides_through_the_core() {
        let mut demo = started(&["--demo", "--section", "approvals"]);
        assert_eq!(demo.section, Section::Approvals);
        let open = demo.data.approvals.iter().filter(|approval| approval.state.is_pending()).count();
        assert!(open > 0 && open < demo.data.approvals.len());
        assert_eq!(demo.trail(Section::Approvals), Trail::Count(open), "decided ones are not counted");
        for class in [SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let _ = demo.window(class);
        }

        let id = demo.data.approvals[0].id.clone();
        let action = approvals::update(
            &mut demo.approvals,
            approvals::Message::Decide { id: id.clone(), allow: false },
            &demo.data,
        );
        assert!(matches!(action, Action::Send(Request::DecideApproval { id: ref sent, allow: false }) if *sent == id));
        assert_eq!(ui(Section::Approvals).perform(action).units(), 1, "outside the demo it is a request");

        let empty = started(&["--demo", "--open", "approvals-empty"]);
        assert_eq!((empty.section, empty.data.approvals.len()), (Section::Approvals, 0));
        assert_eq!(empty.trail(Section::Approvals), Trail::None);
    }

    #[test]
    fn the_inbox_section_shows_rules_and_sends_its_requests_to_the_core() {
        let demo = started(&["--demo", "--section", "inbox"]);
        assert_eq!(demo.section, Section::Inbox);
        assert!(!demo.data.rules.is_empty(), "the demo has rules to show");
        for class in [SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let _ = demo.window(class);
        }
        let open = started(&["--demo", "--open", "inbox"]);
        assert_eq!((open.section, open.data.rules.len()), (Section::Inbox, demo.data.rules.len()));
        let empty = started(&["--demo", "--open", "inbox-empty"]);
        assert_eq!((empty.section, empty.data.rules.len()), (Section::Inbox, 0));

        let mut real = ui(Section::Inbox);
        for message in
            [inbox::Message::QuietWhenBusy(false), inbox::Message::Remove("r1".into()), inbox::Message::OpenInbox]
        {
            let text = format!("{message:?}");
            assert_eq!(real.update(Message::Inbox(message)).units(), 1, "{text}: outside the demo it is a request");
        }
        let _ = real.update(Message::Sent(Err("not-found".into())));
        assert_eq!(real.update(Message::Inbox(inbox::Message::Remove("r1".into()))).units(), 1, "the answer frees it");
    }

    #[test]
    fn the_agents_section_opens_details_and_answers_in_the_demo() {
        let demo = started(&["--demo", "--section", "agents"]);
        assert_eq!(demo.section, Section::Agents);
        assert_eq!(demo.trail(Section::Agents), Trail::Count(demo.data.clients.len()));
        for class in [SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let _ = demo.window(class);
        }

        let detail = started(&["--demo", "--open", "agent-detail"]);
        let codex = detail.data.clients.iter().find(|client| client.kind == "codex").map(|client| client.id.clone());
        assert_eq!(detail.agents.selected, codex);
        assert!(detail.page_overlay().is_none());
        for (state, kind) in [("script-detail", "script"), ("hook-detail", torro_ipc::api::HOOK_CLIENT)] {
            let ui = started(&["--demo", "--open", state]);
            let selected = ui.data.clients.iter().find(|client| Some(&client.id) == ui.agents.selected.as_ref());
            assert_eq!(selected.map(|client| client.kind.as_str()), Some(kind), "{state}");
        }

        let mut key = started(&["--demo", "--open", "new-key"]);
        assert!(key.page_overlay().is_some(), "the demo answers clients.new_key with a made-up key");
        let _ = key.update(Message::Escape);
        assert!(key.page_overlay().is_none(), "Escape closes it and the key is gone");

        let mut disconnect = started(&["--demo", "--open", "disconnect"]);
        assert!(disconnect.page_overlay().is_some());
        let _ = disconnect.update(Message::Escape);
        assert!(disconnect.page_overlay().is_none() && disconnect.agents.selected.is_some());

        let empty = started(&["--demo", "--open", "agents-empty"]);
        assert_eq!((empty.section, empty.data.clients.len()), (Section::Agents, 0));
        assert_eq!(empty.trail(Section::Agents), Trail::None);
    }

    #[test]
    fn connect_agent_runs_through_the_demo_without_touching_anything() {
        for state in ["agent-wizard", "agent-wizard-rights", "agent-wizard-done", "agent-wizard-key"] {
            let mut ui = started(&["--demo", "--open", state]);
            assert_eq!(ui.section, Section::Agents, "{state}");
            assert!(ui.agents.wizard.is_some() && ui.page_overlay().is_some(), "{state}: the dialog is open");
            assert_eq!(ui.data.clients.len(), 4, "{state}: the demo changes nothing");
            let _ = ui.update(Message::Escape);
            assert!(ui.agents.wizard.is_none(), "{state}: Escape closes it");
        }
        let mut real = ui(Section::Agents);
        let detect = real.update(Message::Agents(agents::Message::Connect));
        assert_eq!(detect.units(), 1, "outside the demo detection is a job of its own");
    }

    #[test]
    fn agents_requests_go_to_the_core_outside_the_demo() {
        let mut real = ui(Section::Agents);
        let id = "3fa9c1d2".to_string();
        assert_eq!(real.update(Message::Agents(agents::Message::Open(id.clone()))).units(), 1, "clients.activity");
        assert_eq!(real.perform(Action::Copy("torro_x".into())).units(), 1, "the clipboard");
        let mut demo = started(&["--demo"]);
        assert_eq!(demo.perform(Action::Copy("torro_x".into())).units(), 0, "the demo leaves the clipboard alone");
        let removed = demo.update(Message::Agents(agents::Message::Open(id)));
        assert_eq!(removed.units(), 0);
        assert_eq!(demo.data.clients.len(), 4, "the demo changes nothing");
    }

    #[test]
    fn sections_are_named_by_their_argument() {
        for section in Section::ALL {
            assert_eq!(Section::from_arg(section.arg()), Some(section));
        }
        assert_eq!(Section::from_arg("mail"), None);
    }

    #[test]
    fn the_demo_starts_loaded_with_its_sample_data() {
        let ui = started(&["--demo"]);
        assert!(ui.loaded && ui.data.error.is_none());
        assert!(!ui.data.accounts.is_empty() && !ui.data.templates.is_empty() && ui.data.apps.is_some());
        assert!(!started(&[]).loaded, "without --demo the page waits for the core");
        let apps = started(&["--demo", "--section", "apps"]);
        assert_eq!(apps.section, Section::Apps);
    }

    #[test]
    fn the_open_states_go_where_a_click_would() {
        let wizard = started(&["--demo", "--open", "wizard"]);
        assert_eq!(wizard.section, Section::Accounts);
        assert!(wizard.accounts.wizard.is_some() && wizard.page_overlay().is_some());

        let first = wizard.data.accounts[0].id.clone();
        let detail = started(&["--demo", "--open", "account-detail", "--section", "apps"]);
        assert_eq!((detail.section, detail.accounts.selected.as_ref()), (Section::Accounts, Some(&first)));
        assert!(!detail.accounts.confirm_remove && detail.page_overlay().is_none());

        for state in ["wizard-ways", "wizard-token"] {
            let github = started(&["--demo", "--open", state]);
            assert!(github.accounts.busy() || github.page_overlay().is_some(), "{state}: the dialog is open");
        }
        for state in [
            "wizard-mail",
            "wizard-mail-found",
            "wizard-mail-unsupported",
            "wizard-mail-none",
            "wizard-mail-password",
            "wizard-mail-refused",
            "wizard-mail-services",
            "wizard-mail-done",
            "wizard-ai",
            "wizard-ai-openrouter",
            "wizard-ai-refused",
            "wizard-ai-server",
            "wizard-ai-done",
        ] {
            let mail = started(&["--demo", "--open", state]);
            assert_eq!(mail.section, Section::Accounts, "{state}");
            assert!(mail.accounts.wizard.is_some() && mail.page_overlay().is_some(), "{state}: the dialog is open");
        }
        let password = started(&["--demo", "--open", "change-password"]);
        assert_eq!(password.accounts.selected.as_deref(), Some("caldav:anna@posteo.de@posteo.de"));
        assert!(password.accounts.password.is_some() && password.page_overlay().is_some());
        let ai = started(&["--demo", "--open", "ai-detail"]);
        assert_eq!(ai.accounts.selected.as_deref(), Some("ai:openrouter@openrouter.ai"));
        let github = started(&["--demo", "--open", "scopes-detail"]);
        assert_eq!(github.accounts.selected.as_deref(), Some("github:anna-dev"));

        let remove = started(&["--demo", "--open", "remove"]);
        assert_eq!(remove.accounts.selected.as_ref(), Some(&first));
        assert!(remove.accounts.confirm_remove && remove.page_overlay().is_some());

        let delete = started(&["--demo", "--open", "delete-template"]);
        assert_eq!(delete.section, Section::Workspaces);
        assert!(matches!(workspaces::escape(&delete.workspaces), Some(workspaces::Message::CancelDelete)));
        assert!(delete.page_overlay().is_some());

        let rename = started(&["--demo", "--open", "rename-template"]);
        assert_eq!(rename.section, Section::Workspaces);
        assert!(matches!(workspaces::escape(&rename.workspaces), Some(workspaces::Message::CancelRename)));
        assert!(rename.page_overlay().is_none(), "renaming is in the row, not in a dialog");

        let menu = started(&["--demo", "--open", "menu", "--section", "apps"]);
        assert_eq!((menu.section, menu.menu, menu.drawer), (Section::Apps, Some(Menu::Nav), false));
        let drawer = started(&["--demo", "--open", "drawer"]);
        assert_eq!((drawer.menu, drawer.drawer), (None, true));
        let switcher = started(&["--demo", "--open", "switcher"]);
        assert_eq!((switcher.menu, switcher.drawer), (Some(Menu::Switcher), true));
    }

    #[test]
    fn every_open_state_builds_in_every_class() {
        for (name, _) in launch::Open::ALL {
            let ui = started(&["--demo", "--open", name]);
            for class in [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
                let _ = ui.window(class);
            }
        }
    }

    #[test]
    fn the_demo_sends_no_request_to_a_socket_and_stores_nothing() {
        let mut demo = started(&["--demo"]);
        let sync = || Action::Send(Request::Sync);
        assert_eq!(demo.perform(sync()).units(), 0);
        assert_eq!(demo.perform(Action::OpenUrl("https://example.org".into())).units(), 0);
        assert_eq!(demo.update(Message::Tick).units(), 0, "no reload from a service");
        assert_eq!(demo.update(Message::Sent(Ok(Response::Ok))).units(), 0);
        let accounts = demo.data.accounts.len();
        // What the pages and the shell do on their own account.
        let messages = [
            Message::Apps(apps::Message::Open("torrotasks".into())),
            Message::Switcher(Switcher::OpenApp("torrocalendar")),
            Message::Accounts(accounts::Message::SyncNow),
            Message::Accounts(accounts::Message::SetShown { collection: "work".into(), shown: false }),
            Message::Accounts(accounts::Message::Open(demo.data.accounts[0].id.clone())),
            Message::Accounts(accounts::Message::AskRemove),
            Message::Accounts(accounts::Message::Remove),
            Message::Workspaces(workspaces::Message::Load(demo.data.templates[0].id.clone())),
            Message::Workspaces(workspaces::Message::AskDelete(demo.data.templates[0].id.clone())),
            Message::Workspaces(workspaces::Message::ConfirmDelete),
            Message::Approvals(approvals::Message::Decide { id: demo.data.approvals[0].id.clone(), allow: true }),
            Message::Inbox(inbox::Message::QuietWhenBusy(false)),
            Message::Inbox(inbox::Message::Remove(demo.data.rules[0].id.clone())),
            Message::Inbox(inbox::Message::OpenInbox),
        ];
        for message in messages {
            let text = format!("{message:?}");
            assert_eq!(demo.update(message).units(), 0, "{text}");
        }
        assert_eq!(demo.data.accounts.len(), accounts, "removing an account in the demo changes nothing");
        assert!(demo.data.settings.quiet_when_busy && demo.data.rules.len() == 7, "nor do the inbox's controls");
        // Without --demo the same action is a request.
        assert_eq!(ui(Section::Overview).perform(sync()).units(), 1);
        assert_eq!(ui(Section::Overview).update(Message::Tick).units(), 2, "the reload and the zoom");
    }

    #[test]
    fn the_demo_does_not_listen_to_a_clock_and_the_appearance_parameter_wins() {
        let (demo, real) = (started(&["--demo"]), ui(Section::Overview));
        assert_eq!(demo.subscription().units() + 1, real.subscription().units(), "no reload tick in the demo");

        let mut light = started(&["--demo", "--appearance", "light"]);
        let _ = light.update(Message::SystemMode(Mode::Dark));
        assert_eq!(light.theme(), theme::light());
        let _ = light.update(loaded(Appearance::Dark, None));
        assert_eq!(light.theme(), theme::light(), "a reload of the stored appearance does not beat the parameter");
        assert_eq!(light.appearance, Appearance::Dark);
        let mut dark = started(&["--appearance", "dark"]);
        let _ = dark.update(Message::SystemMode(Mode::Light));
        assert_eq!(dark.theme(), theme::theme());
    }

    #[test]
    fn the_general_zoom_buttons_zoom_this_window_and_the_page_shows_it() {
        use torro_gui::zoom::Step;
        let mut demo = started(&["--demo", "--section", "general"]);
        let _ = demo.update(Message::Resized(Size::new(1440.0, 900.0)));
        for _ in 0..3 {
            let _ = demo.update(Message::General(general::Message::Zoom(Step::In)));
        }
        assert_eq!(demo.zoom.factor(), 1.5);
        assert_eq!(demo.data.settings.zoom.percent(), 150, "the page shows the level");
        assert_eq!(layout::of(demo.size), SizeClass::Medium, "1440 at 150 % is 960 wide");
        // A reload keeps the level of the window, not the one the reload brought.
        let _ = demo.update(loaded(Appearance::System, None));
        assert_eq!(demo.data.settings.zoom.percent(), 150);
        let _ = demo.update(Message::Zoom(zoom::Message::Step(Step::Reset)));
        assert_eq!((demo.zoom.factor(), layout::of(demo.size)), (1.0, SizeClass::Wide));
    }
}
