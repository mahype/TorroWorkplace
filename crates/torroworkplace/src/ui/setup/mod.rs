//! The first-run setup (task W.4, concept `docs/einrichtung.md`): a mode of the control centre
//! that leads once through what the suite needs – Nextcloud first, then the components a person
//! wants, then each chosen component on its own, AI and the web search last – and ends on a summary.
//!
//! `torroworkplace ui --setup [--step <step>]` opens it; `--demo --open setup-<step>` shows a step
//! with sample data. The setup is a guided way through the requests the control centre sends
//! anyway: nothing is stored here, and what a step connects or switches is saved at once, so
//! closing the window halfway loses nothing.
//!
//! The window is the suite's Linear frame (design skill, section 0), with the steps where the
//! sections are:
//!
//! - **wide:** the steps in the sidebar, each with its state at the right (a check mark when
//!   passed, "Skipped"); the current one `selected`, never red. Every step can be clicked.
//! - **medium:** the steps as the rail of icons, the names as tooltips.
//! - **narrow:** no list; the header has "Back", the step's name and how far it is ("4 of 8" with a
//!   progress bar).
//!
//! Under the header the step's page, at most [`size::form_max()`] wide, and at the bottom a fixed
//! footer: a status or problem at the left, "Back", "Skip" and the one primary button at the
//! right. Enter is the primary button, Escape goes back.
//!
//! Which steps there are follows the components that are on (`Settings::components_off`): the
//! step of a component turned off leaves the list. Steps whose pages come with later tasks
//! (W.5–W.8) are not listed yet ([`Step::built`]).
//!
//! The step "Web search" (task WEB.7, `docs/einrichtung.md` step 9) shows the view of Assistant › Settings › Tools
//! (`torro_settings_ui::assistant::web`): a provider's switch is saved at once, the first one switched on also
//! switches the tool `web` on; "Let the assistant use it" gives the assistant's client read access to `web` with
//! "Continue" ([`with_web`]). Only a person does this – the control centre holds the app key.
//!
//! The step "Documents" (issue #303, `docs/einrichtung.md` step 10) only presents the ArtCraft apps of the catalog
//! (`extensions.list_catalog`, which the shell loads while the step shows): each with what it is for and how it stands
//! here – set up as an MCP server, installed on this computer, or "Get <App>" with the link to its releases. It stores
//! nothing; the apps are set up under MCP servers.

mod pages;

pub(super) use pages::name as component_name;

use std::collections::{BTreeMap, BTreeSet};

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, rule, scrollable, svg};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::AppId;
use torro_core::account::Secret;
use torro_core::ai::{AiFunction, AiModel, AiProvider};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, ComponentSwitch, Language, Settings, SetupState, ToolId};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, SidebarMode};
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets::{self, Trail};
use torro_gui::{style, typo};
use torro_ipc::api::{ASSISTANT_CLIENT, Area, AreaRights, Level, Rights, Selection};
use torro_ipc::web::{SearchProviderId, SearchProviderInfo};
use torro_ipc::{AiServer, ClientInfo, Request, Response, SettingKey};
use torro_settings_ui::assistant::web;

use super::connect::caldav::{self, CalDavSignIn};
use super::connect::github::{self, GithubSignIn};
use super::connect::nextcloud::{self, Ended, NextcloudSignIn};
use super::{Action, Data, page_margin, wizard};
use torro_inbox::SourceRef;
use torro_mail::Notify;

/// A step of the setup, in the order it is walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    Welcome,
    Nextcloud,
    Components,
    Calendar,
    Tasks,
    Mail,
    Inbox,
    Ai,
    Web,
    Documents,
    Done,
}

impl Step {
    pub const ALL: [Step; 11] = [
        Step::Welcome,
        Step::Nextcloud,
        Step::Components,
        Step::Calendar,
        Step::Tasks,
        Step::Mail,
        Step::Inbox,
        Step::Ai,
        Step::Web,
        Step::Documents,
        Step::Done,
    ];

    /// The word for `--step` and `--open setup-<step>`.
    pub fn arg(self) -> &'static str {
        match self {
            Step::Welcome => "welcome",
            Step::Nextcloud => "nextcloud",
            Step::Components => "components",
            Step::Calendar => "calendar",
            Step::Tasks => "tasks",
            Step::Mail => "mail",
            Step::Inbox => "inbox",
            Step::Ai => "ai",
            Step::Web => "web",
            Step::Documents => "documents",
            Step::Done => "done",
        }
    }

    /// The step named `arg`, if there is one and its page is built.
    pub fn from_arg(arg: &str) -> Option<Step> {
        Step::ALL.into_iter().filter(|step| step.built()).find(|step| step.arg() == arg)
    }

    /// The words `--step` takes: the steps whose pages are built.
    pub fn args() -> Vec<&'static str> {
        Step::ALL.into_iter().filter(|step| step.built()).map(Step::arg).collect()
    }

    pub fn label(self) -> &'static str {
        match self {
            Step::Welcome => tr("Welcome"),
            Step::Nextcloud => "Nextcloud",
            Step::Components => tr("Components"),
            Step::Calendar => tr("Calendar"),
            Step::Tasks => tr("Tasks"),
            Step::Mail => tr("Mail"),
            Step::Inbox => tr("Notification Hub"),
            Step::Ai => tr("AI"),
            Step::Web => tr("Web search"),
            Step::Documents => tr("Documents"),
            Step::Done => tr("All set"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            // Where the walk starts.
            Step::Welcome => Icon::Flag,
            Step::Nextcloud => Icon::Cloud,
            Step::Components => Icon::LayoutGrid,
            Step::Calendar => Icon::Calendar,
            Step::Tasks => Icon::ListChecks,
            Step::Mail => Icon::Mail,
            Step::Inbox => Icon::Inbox,
            Step::Ai => Icon::Sparkles,
            Step::Web => Icon::Globe,
            Step::Documents => Icon::FileText,
            Step::Done => Icon::CircleCheck,
        }
    }

    /// The component whose step this is; its step is only walked while it is on.
    fn component(self) -> Option<Component> {
        match self {
            Step::Calendar => Some(Component::Calendar),
            Step::Tasks => Some(Component::Tasks),
            Step::Mail => Some(Component::Mail),
            Step::Inbox => Some(Component::Inbox),
            // The web search is a tool of the assistant; so are the documents' apps, for it and its agents.
            Step::Web | Step::Documents => Some(Component::Assistant),
            _ => None,
        }
    }

    /// Whether the step's page exists yet: W.4 built Welcome, Components and Done, W.5 Nextcloud,
    /// W.6 Calendar and Tasks, W.7 Mail and Inbox, W.8 the AI, WEB.7 the web search: all of them now. A step that is not
    /// built is not listed, so the setup never shows an empty page.
    fn built(self) -> bool {
        true
    }
}

/// The steps that are walked with the components in `off` turned off, in order.
pub fn steps(off: &BTreeSet<Component>) -> Vec<Step> {
    Step::ALL
        .into_iter()
        .filter(|step| step.built() && step.component().is_none_or(|component| !off.contains(&component)))
        .collect()
}

/// The setup while it is open.
#[derive(Debug)]
pub struct State {
    step: Step,
    /// The furthest step reached; those before it count as passed.
    reached: Step,
    /// Steps left with "Skip"; going on from one of them with the primary button takes it out.
    skipped: BTreeSet<Step>,
    /// The choices of this setup, shown at once. The settings are only reloaded after the request,
    /// and the demo stores nothing, so the page keeps what was chosen itself.
    language: Language,
    appearance: Appearance,
    off: BTreeSet<Component>,
    /// A choice was made here: from then on the page's choices count, not the reloaded settings.
    touched: bool,
    /// The Nextcloud step (task W.5): the address and the sign-in while it runs.
    nextcloud: NextcloudSignIn,
    /// The Nextcloud step shows the address form although an account exists ("Add another
    /// Nextcloud").
    adding: bool,
    /// The Nextcloud account signed in to in this setup; its services are shown.
    signed_in: Option<String>,
    /// What went wrong on the current step, for the footer: the core's answer or the shell's error.
    error: Option<String>,
    /// Calendars and task lists switched in this setup, by id (task W.6): shown at once, while the
    /// settings follow with the next reload (and never in the demo).
    shown: BTreeMap<String, bool>,
    /// The CalDAV form of the calendar and tasks steps, while it is open.
    caldav: Option<CalDavSignIn>,
    /// The account the CalDAV form connected last, for the footer.
    connected: Option<String>,
    /// GitHub's sign-in on the tasks step: the token and the check.
    github: GithubSignIn,
    /// The token field is open.
    github_token: bool,
    /// The GitHub login signed in to in this setup.
    github_login: Option<String>,
    /// The add-account dialog over the setup (task W.7): the mail path, TorroMail's accounts or
    /// ntfy, exactly as the groups of Accounts open it (task S.30).
    dialog: Option<wizard::State>,
    /// Inbox sources switched in this setup, by their key: shown at once, the core follows.
    muted: BTreeMap<String, bool>,
    /// "Quiet during busy events" as switched here.
    quiet: Option<bool>,
    /// The AI step (task W.8): the servers found on this computer; `None` before the answer.
    ai_found: Option<Vec<AiServer>>,
    /// `FindAiServers` was sent and has not answered.
    ai_searching: bool,
    /// The address of the local server being connected.
    ai_connecting: Option<String>,
    /// The addresses of local servers connected in this setup.
    ai_connected: BTreeSet<String>,
    /// Mail sorting is recommended on; an explicit choice here overrides the stored assignment.
    sort_mail: Option<bool>,
    sort_model: Option<(String, String)>,
    /// The engine chosen here for the assistant's new chats, by id.
    engine: Option<String>,
    /// The web search step (task WEB.7): a key or SearXNG's address being typed, and the address check.
    web: web::State,
    /// Search providers switched in this setup and not yet in the core's list: shown at once, until the core's list
    /// shows them (never in the demo) or a refusal comes back.
    switched: BTreeMap<SearchProviderId, bool>,
    /// The providers as the web step shows them: the core's list with [`State::switched`] laid over it, renewed with
    /// every reload ([`observe`]) and switch. `None` before the first switch: the core's list as it is.
    providers: Option<Vec<SearchProviderInfo>>,
    /// "Let the assistant use it" (ticked at first): "Continue" gives the assistant read access to `web`.
    web_allowed: bool,
    /// The assistant's rights for `web` were sent in this setup: at most once.
    web_granted: bool,
}

impl State {
    /// The setup on `step`, with the choices as `settings` has them.
    pub fn new(step: Step, settings: &Settings) -> State {
        State {
            step,
            reached: step,
            skipped: BTreeSet::new(),
            language: settings.language,
            appearance: settings.appearance,
            off: settings.components_off.clone(),
            touched: false,
            nextcloud: NextcloudSignIn::default(),
            adding: false,
            signed_in: None,
            error: None,
            shown: BTreeMap::new(),
            caldav: None,
            connected: None,
            github: GithubSignIn::default(),
            github_token: false,
            github_login: None,
            dialog: None,
            muted: BTreeMap::new(),
            quiet: None,
            ai_found: None,
            ai_searching: false,
            ai_connecting: None,
            ai_connected: BTreeSet::new(),
            sort_mail: None,
            sort_model: None,
            engine: None,
            web: web::State::default(),
            switched: BTreeMap::new(),
            providers: None,
            web_allowed: true,
            web_granted: false,
        }
    }

    /// Takes the choices from freshly loaded `settings` while none was made here yet: the setup
    /// can open before the core answered the first time.
    pub fn loaded(&mut self, settings: &Settings) {
        if !self.touched {
            self.language = settings.language;
            self.appearance = settings.appearance;
            self.off = settings.components_off.clone();
        }
    }

    /// A sign-in runs, or a provider's switch waits for the core's list; the shell reloads more often, so the end
    /// is seen soon.
    pub fn busy(&self) -> bool {
        self.nextcloud.running()
            || self.github.checking()
            || self.caldav.as_ref().is_some_and(CalDavSignIn::connecting)
            || self.dialog.as_ref().is_some_and(wizard::State::busy)
            || self.ai_searching
            || self.ai_connecting.is_some()
            || web::checking(&self.web)
            || !self.switched.is_empty()
    }

    /// The add-account dialog lies over the setup; keys and the footer belong to it.
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// Whether the inbox source `source` is muted: as switched here, else as it is stored. A mail
    /// account keeps that in its own `notify` (task S.21, `docs/settings.md` › Was wohin gehört),
    /// so its switch reads from there; every other source from `inbox.list_sources`. An old
    /// `inbox.mute_source` rule still wins over `notify` – it would keep the source quiet whatever
    /// `notify` says –, so the switch is off while either of them is.
    fn is_muted(&self, data: &Data, source: &SourceRef) -> bool {
        if let Some(switched) = self.muted.get(&source_key(source)) {
            return *switched;
        }
        let muted = data.sources.iter().any(|info| info.source == *source && info.muted);
        if source.kind != torro_inbox::SourceKind::Mail {
            return muted;
        }
        muted || mail_notify(data, source) == Notify::Off
    }

    /// Whether the apps show the calendar or task list `id`: as switched here, else as stored.
    fn is_shown(&self, data: &Data, id: &str) -> bool {
        self.shown.get(id).copied().unwrap_or_else(|| super::accounts::is_shown(&data.settings, id))
    }

    /// A form of the calendar or tasks step is open, or its check runs: the footer's buttons belong
    /// to it.
    fn in_form(&self) -> bool {
        self.caldav.is_some() || self.github_token || self.github.checking()
    }

    /// The step shown.
    pub fn step(&self) -> Step {
        self.step
    }

    /// The steps walked now.
    fn steps(&self) -> Vec<Step> {
        steps(&self.off)
    }

    /// The position of the current step (from 1) and the number of steps.
    fn position(&self) -> (usize, usize) {
        let steps = self.steps();
        let at = steps.iter().position(|step| *step == self.step).map_or(1, |index| index + 1);
        (at, steps.len())
    }

    /// The step after (`forward`) or before the current one, if any.
    fn neighbour(&self, forward: bool) -> Option<Step> {
        let steps = self.steps();
        let index = steps.iter().position(|step| *step == self.step)?;
        match forward {
            true => steps.get(index + 1).copied(),
            false => index.checked_sub(1).and_then(|before| steps.get(before).copied()),
        }
    }

    /// How a step stands in the list.
    fn mark(&self, step: Step) -> Mark {
        if self.skipped.contains(&step) {
            Mark::Skipped
        } else if step < self.reached {
            Mark::Passed
        } else {
            Mark::Open
        }
    }

    fn go(&mut self, step: Step) {
        self.step = step;
        self.reached = self.reached.max(step);
        self.error = None;
        self.close_forms();
        // A key typed and not saved goes with the step; it is never kept longer than its field shows it.
        self.web = web::State::default();
    }

    /// Closes the forms of the calendar and tasks steps; a check that runs is no longer waited for.
    fn close_forms(&mut self) {
        self.caldav = None;
        self.github_token = false;
        self.github.stop();
    }

    /// The Nextcloud step shows the address form: there is no Nextcloud account yet, or another one
    /// is being added.
    fn asks_address(&self, data: &Data) -> bool {
        self.adding || nextcloud_account(self, data).is_none()
    }
}

/// The Nextcloud account the Nextcloud step shows: the one signed in to in this setup, else the
/// first one there is.
fn nextcloud_account<'a>(state: &State, data: &'a Data) -> Option<&'a torro_ipc::AccountInfo> {
    let listed = || data.accounts.iter().filter(|account| account.kind == "nextcloud" && !account.id.is_empty());
    state.signed_in.as_deref().and_then(|id| listed().find(|account| account.id == id)).or_else(|| listed().next())
}

/// How a step stands in the list of steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Open,
    Passed,
    Skipped,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A step in the list was clicked.
    Go(Step),
    /// The primary button or Enter: on to the next step, on the last one finish.
    Next,
    /// "Back" or Escape.
    Back,
    /// "Skip": on to the next step, the current one marked as skipped.
    Skip,
    /// "Set up later": put the setup off and close it.
    Later,
    Language(Language),
    Appearance(Appearance),
    /// A component's switch or card.
    Component(Component, bool),
    /// "Open" of a component's app on the last step (its window class).
    OpenApp(&'static str),
    /// The Nextcloud address changed.
    ServerChanged(String),
    /// "Open the page again" while the browser signs in.
    OpenLoginPage,
    /// The login link field is read-only: typing in it changes nothing.
    LoginLinkEdited,
    /// "Add another Nextcloud".
    AddNextcloud,
    /// "Cancel": stop waiting for the browser or adding another Nextcloud, or close the form of the
    /// calendar or tasks step.
    Cancel,
    /// A calendar's or task list's switch.
    Shown(String, bool),
    /// "Add a CalDAV account".
    OpenCalDav,
    /// The CalDAV form.
    CalDav(caldav::Message),
    /// "Use GitHub CLI sign-in".
    UseGithubCli,
    /// "Paste a token".
    PasteToken,
    /// The token field changed. A [`Secret`], so the derived `Debug` shows no token.
    TokenChanged(Secret),
    /// "Create a token on GitHub".
    OpenTokenPage,
    /// "Add mail account": the add-account dialog on its mail path.
    AddMail,
    /// "Take over from TorroMail": the add-account dialog at its step TorroMail (task S.30).
    TakeOverTorroMail,
    /// "Subscribe to ntfy topics": the add-account dialog on its ntfy path.
    AddNtfy,
    /// The add-account dialog over the setup.
    Dialog(wizard::Message),
    /// An inbox source's switch: `true` lets it in, `false` mutes it.
    Source(SourceRef, bool),
    /// "Quiet during busy events".
    Quiet(bool),
    /// "Connect" on a server found on this computer: its provider and address.
    ConnectLocalAi(AiProvider, String),
    /// "Add key" on a cloud provider: the add-account dialog at its key.
    AddAiKey(AiProvider),
    /// "Other server": the add-account dialog at the address of an OpenAI-compatible server.
    OtherAiServer,
    /// The AI account a new chat of the assistant starts with, by engine id.
    Engine(String),
    /// Enable or disable model sorting of mail in the recommended AI setup.
    SortMail(bool),
    /// Choose a decision model for mail from the connected AI accounts.
    SortModel(String, String),
    /// The search providers of the web step (task WEB.7): the view of Assistant › Settings › Tools.
    Web(web::Message),
    /// "Let the assistant use it" on the web step.
    AllowWeb(bool),
    /// "Get <App>" on the step Documents: the app's releases (the catalog entry's address) in the browser.
    GetApp(String),
}

/// What the setup wants the shell to do.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Stay in the setup and carry out the action.
    Stay(Action),
    /// Close the setup (it was finished or put off), carry out the action and show the overview.
    Close(Action),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Outcome {
    let outcome = handle(state, message, data);
    // The AI step looks for servers on this computer the first time it is shown.
    match outcome {
        Outcome::Stay(Action::None) if state.step == Step::Ai && state.ai_found.is_none() && !state.ai_searching => {
            state.ai_searching = true;
            Outcome::Stay(Action::Send(Request::FindAiServers))
        }
        outcome => outcome,
    }
}

fn handle(state: &mut State, message: Message, data: &Data) -> Outcome {
    let stay = Outcome::Stay;
    // A finished sign-in is noticed before the message counts.
    observe(state, data);
    match message {
        Message::Go(step) => {
            if state.steps().contains(&step) {
                state.go(step);
            }
            stay(Action::None)
        }
        // On the Nextcloud step the primary button signs in while it asks for an address.
        Message::Next if state.step == Step::Nextcloud && state.asks_address(data) => {
            if !state.nextcloud.can_start() {
                return stay(Action::None);
            }
            state.error = None;
            stay(state.nextcloud.start(data))
        }
        Message::Next if state.step == Step::Nextcloud && state.nextcloud.running() => stay(Action::None),
        // On the calendar and tasks steps the primary button sends the open form.
        Message::Next if state.in_form() => stay(submit_form(state)),
        // Leaving the web step forward gives the assistant its rights for `web`, if ticked; "Skip" does not. While
        // that hangs on a switch the core has not confirmed, the click waits ([`web_waits`]).
        Message::Next if state.step == Step::Web && web_waits(state, data) => stay(Action::None),
        Message::Next if state.step == Step::Ai && ai_models_wait(data) => stay(Action::None),
        Message::Next if state.step == Step::Web => {
            let grant = grant_web(state, data);
            state.skipped.remove(&Step::Web);
            if let Some(next) = state.neighbour(true) {
                state.go(next);
            }
            stay(grant)
        }
        Message::Next if state.step == Step::Ai => {
            let assignment = mail_sorting_request(state, data);
            state.skipped.remove(&Step::Ai);
            if let Some(next) = state.neighbour(true) {
                state.go(next);
            }
            stay(assignment.map_or(Action::None, Action::Send))
        }
        Message::Next => match state.neighbour(true) {
            Some(next) => {
                state.skipped.remove(&state.step);
                state.go(next);
                stay(Action::None)
            }
            None => Outcome::Close(set_setup(SetupState::Done)),
        },
        Message::Skip => {
            state.skipped.insert(state.step);
            match state.neighbour(true) {
                Some(next) => state.go(next),
                None => return Outcome::Close(set_setup(SetupState::Done)),
            }
            stay(Action::None)
        }
        Message::Back => {
            if let Some(before) = state.neighbour(false) {
                state.go(before);
            }
            stay(Action::None)
        }
        Message::Later => Outcome::Close(set_setup(SetupState::Skipped)),
        Message::Language(language) => {
            state.touched = true;
            state.language = language;
            stay(set(SettingKey::Language, serde_json::to_value(language).unwrap_or_default()))
        }
        Message::Appearance(appearance) => {
            state.touched = true;
            state.appearance = appearance;
            stay(set(SettingKey::Appearance, serde_json::to_value(appearance).unwrap_or_default()))
        }
        Message::Component(component, enabled) => {
            state.touched = true;
            match enabled {
                true => state.off.remove(&component),
                false => state.off.insert(component),
            };
            let switch = ComponentSwitch { component, enabled };
            stay(set(SettingKey::Component, serde_json::to_value(switch).unwrap_or_default()))
        }
        Message::OpenApp(app) => stay(Action::Send(Request::OpenApp { app: app.into(), target: None })),
        Message::ServerChanged(server) => {
            state.nextcloud.server = server;
            stay(Action::None)
        }
        Message::OpenLoginPage => match nextcloud::login_url(data) {
            Some(url) if state.nextcloud.running() => stay(Action::OpenUrl(url.to_string())),
            _ => stay(Action::None),
        },
        Message::LoginLinkEdited => stay(Action::None),
        Message::AddNextcloud => {
            state.adding = true;
            state.nextcloud.server.clear();
            state.error = None;
            stay(Action::None)
        }
        Message::Cancel => {
            if state.nextcloud.running() {
                state.nextcloud.stop();
            } else if state.in_form() {
                state.close_forms();
            } else {
                state.adding = false;
            }
            state.error = None;
            stay(Action::None)
        }
        Message::Shown(collection, shown) => {
            state.shown.insert(collection.clone(), shown);
            stay(Action::Send(Request::SetCollectionShown { collection, shown }))
        }
        Message::OpenCalDav => {
            state.close_forms();
            state.caldav = Some(CalDavSignIn::default());
            state.error = None;
            stay(Action::None)
        }
        Message::CalDav(message) => {
            if let Some(form) = state.caldav.as_mut() {
                form.update(message);
            }
            stay(Action::None)
        }
        Message::UseGithubCli if !state.github.checking() => {
            state.close_forms();
            state.error = None;
            stay(state.github.use_cli())
        }
        Message::PasteToken => {
            state.close_forms();
            state.github_token = true;
            state.error = None;
            stay(Action::None)
        }
        Message::TokenChanged(token) => {
            state.github.token = token;
            stay(Action::None)
        }
        Message::OpenTokenPage => stay(Action::OpenUrl(github::NEW_TOKEN_URL.to_string())),
        Message::UseGithubCli => stay(Action::None),
        Message::AddMail => stay(open_dialog(state, Some(wizard::Provider::Mail), data)),
        Message::TakeOverTorroMail => stay(open_dialog(state, None, data)),
        Message::AddNtfy => stay(open_dialog(state, Some(wizard::Provider::Ntfy), data)),
        Message::Dialog(wizard::Message::Close) => {
            state.dialog = None;
            stay(Action::None)
        }
        Message::Dialog(message) => match state.dialog.as_mut() {
            Some(dialog) => stay(wizard::update(dialog, message, data)),
            None => stay(Action::None),
        },
        Message::Source(source, on) => {
            state.muted.insert(source_key(&source), !on);
            stay(source_requests(&source, on, data))
        }
        Message::Quiet(quiet) => {
            state.quiet = Some(quiet);
            stay(set(SettingKey::QuietWhenBusy, serde_json::Value::Bool(quiet)))
        }
        Message::ConnectLocalAi(provider, server) if state.ai_connecting.is_none() => {
            state.ai_connecting = Some(server.clone());
            state.error = None;
            stay(Action::Send(Request::ConnectAi { provider, server: Some(server), key: Secret::default() }))
        }
        Message::ConnectLocalAi(..) => stay(Action::None),
        Message::AddAiKey(provider) => stay(open_ai_dialog(state, provider, data)),
        Message::OtherAiServer => stay(open_ai_dialog(state, AiProvider::OpenAiCompatible, data)),
        Message::Engine(engine) => {
            state.engine = Some(engine.clone());
            let default = torro_core::settings::AssistantDefault { engine, model: None, effort: None };
            stay(set(SettingKey::AssistantDefault, torro_core::settings::AssistantDefault::to_value(Some(&default))))
        }
        Message::SortMail(on) => {
            state.sort_mail = Some(on);
            stay(Action::None)
        }
        Message::SortModel(engine, model) => {
            state.sort_model = Some((engine, model));
            state.sort_mail = Some(true);
            stay(Action::None)
        }
        Message::Web(web::Message::Switch(id, on)) => stay(switch_provider(state, data, id, on)),
        // The check's answer comes back to the setup alone ([`checked`]).
        Message::Web(web::Message::CheckAddress) => {
            let shown = state.providers.as_deref().unwrap_or(&data.search_providers);
            stay(match web::update(&mut state.web, web::Message::CheckAddress, shown) {
                torro_settings_ui::Action::Send(mut requests) if requests.len() == 1 => {
                    requests.pop().map_or(Action::None, Action::CheckAddress)
                }
                other => from_view(other),
            })
        }
        Message::Web(message) => {
            let shown = state.providers.as_deref().unwrap_or(&data.search_providers);
            stay(from_view(web::update(&mut state.web, message, shown)))
        }
        Message::AllowWeb(allowed) => {
            state.web_allowed = allowed;
            stay(Action::None)
        }
        Message::GetApp(url) => stay(Action::OpenUrl(url)),
    }
}

// ----- Web search (task WEB.7) ----------------------------------------------------------------------

/// The search providers as the web step shows them: the core's list with the switches of this setup laid over it.
pub(super) fn providers<'a>(state: &'a State, data: &'a Data) -> &'a [SearchProviderInfo] {
    state.providers.as_deref().unwrap_or(&data.search_providers)
}

/// Lays the switches of this setup over the core's list of providers, so a switch shows at once – also in the
/// demo, which stores nothing. Only until the core's list shows it: a switch the core lists as switched is
/// dropped from [`State::switched`], and without any left the core's list is shown as it is.
fn show_switched(state: &mut State, data: &Data) {
    state.switched.retain(|id, on| {
        data.search_providers.iter().find(|info| info.id == *id).is_none_or(|info| info.enabled != *on)
    });
    if state.switched.is_empty() {
        state.providers = None;
        return;
    }
    let mut shown = data.search_providers.clone();
    for info in &mut shown {
        if let Some(on) = state.switched.get(&info.id) {
            info.enabled = *on;
        }
    }
    state.providers = Some(shown);
}

/// Whether a search can ask the provider `info`: switched on, SearXNG only with an address – the rule of
/// `WebSearch::askable` in the core. Its `enabled` alone is the switch as shown.
pub(super) fn askable(info: &SearchProviderInfo) -> bool {
    info.enabled && (info.id != SearchProviderId::Searxng || info.address.is_some())
}

/// A provider's switch: `settings.set_setting search_provider` at once. The first provider switched on also
/// switches the tool `web` on (`tool_enabled`) – the first a search can ask ([`askable`]) –, unless the settings
/// have it on already: whoever picks a provider wants the web search (`docs/einrichtung.md`, step 9). Never
/// anything of `clients.*`.
fn switch_provider(state: &mut State, data: &Data, id: SearchProviderId, on: bool) -> Action {
    let first = on && !providers(state, data).iter().any(askable);
    state.switched.insert(id, on);
    show_switched(state, data);
    let switch = web::switch_request(id, on);
    match first && !data.settings.tools.is_on(ToolId::Web) {
        true => Action::SendAll(vec![switch, crate::ui::connections::switch_request(ToolId::Web, true)]),
        false => Action::Send(switch),
    }
}

/// The client of the assistant ([`ASSISTANT_CLIENT`]); `None` before the assistant first started.
pub(super) fn assistant_client(data: &Data) -> Option<&ClientInfo> {
    data.clients.iter().find(|client| client.kind == ASSISTANT_CLIENT)
}

/// What "Continue" on the web step sends: with "Let the assistant use it" ticked, a provider a search can ask as
/// the core lists it ([`askable`]; not a switch on its way, which the core may refuse) and the assistant's client
/// there, one `clients.update_client` with its rights and read access to `web` ([`with_web`]) – once in a setup,
/// and only when that changes its rights. Otherwise nothing.
fn grant_web(state: &mut State, data: &Data) -> Action {
    let any_on = data.search_providers.iter().any(askable);
    if !state.web_allowed || state.web_granted || !any_on {
        return Action::None;
    }
    let Some(client) = assistant_client(data) else { return Action::None };
    let rights = with_web(&client.rights);
    if rights == client.rights {
        return Action::None;
    }
    state.web_granted = true;
    Action::Send(Request::UpdateClient { id: client.id.clone(), rights })
}

/// Whether "Continue" on the web step waits for the core: a switch of this setup is not in the core's list yet
/// ([`State::switched`]) and "Continue" would give the assistant `web` – ticked, not given yet, and the assistant's
/// client there without it. "Continue" goes by the core's list ([`grant_web`]), so a click before the reload would
/// pass the step without the right; the button waits instead, and the shell reloads sooner ([`State::busy`]). A
/// refusal ends the wait too ([`answered`]).
pub(super) fn web_waits(state: &State, data: &Data) -> bool {
    let grants = state.web_allowed
        && !state.web_granted
        && assistant_client(data).is_some_and(|client| with_web(&client.rights) != client.rights);
    grants && !state.switched.is_empty()
}

/// `rights` with at least read access in the area `tools` and the tool `web` in its selection; everything else
/// stays as it is (the level, the admin switch, every other tool and right). "All tools" does not include `web` –
/// a person ticks it by name (`ToolId::needs_naming`) – so it becomes every tool by name, `web` among them, and PDF
/// stays reachable: what the rights editor does when `web` is ticked under "All" (task WEB.6).
pub(super) fn with_web(rights: &Rights) -> Rights {
    let mut rights = rights.clone();
    let held = rights.area(Area::Tools);
    if held.level < Level::Read {
        rights.areas.insert(Area::Tools, AreaRights { level: Level::Read, admin: held.admin });
    }
    let web = ToolId::Web.name().to_string();
    rights.tools = match std::mem::replace(&mut rights.tools, Selection::All) {
        Selection::All => Selection::Only(ToolId::ALL.into_iter().map(|tool| tool.name().to_string()).collect()),
        Selection::Only(mut ids) => {
            if !ids.contains(&web) {
                ids.push(web);
            }
            Selection::Only(ids)
        }
    };
    rights
}

/// The view's action as the shell's: one request, several sent side by side, a page to open, or nothing.
fn from_view(action: torro_settings_ui::Action<()>) -> Action {
    match action {
        torro_settings_ui::Action::Send(mut requests) if requests.len() == 1 => {
            requests.pop().map_or(Action::None, Action::Send)
        }
        torro_settings_ui::Action::Send(requests) if !requests.is_empty() => Action::SendAll(requests),
        torro_settings_ui::Action::OpenUrl(url) => Action::OpenUrl(url),
        _ => Action::None,
    }
}

/// Opens the add-account dialog at the AI provider `provider`: its key, or the address of another
/// server. The dialog looks for local servers as it opens, like the "Add" of AI.
fn open_ai_dialog(state: &mut State, provider: AiProvider, data: &Data) -> Action {
    let search = open_dialog(state, Some(wizard::Provider::Ai), data);
    if let Some(dialog) = state.dialog.as_mut() {
        wizard::update(dialog, wizard::Message::ChooseAi(provider), data);
    }
    search
}

/// Opens the add-account dialog over the setup the way Accounts does (task S.30): at the first step
/// of `provider` ([`wizard::State::at`]), or at TorroMail's accounts ([`wizard::State::torromail`],
/// which asks for them as it opens).
fn open_dialog(state: &mut State, provider: Option<wizard::Provider>, data: &Data) -> Action {
    state.close_forms();
    state.error = None;
    let (dialog, action) = match provider {
        Some(provider) => wizard::State::at(provider, data),
        None => wizard::State::torromail(),
    };
    state.dialog = Some(dialog);
    action
}

/// A source's key for the switches kept here: kind and name.
fn source_key(source: &SourceRef) -> String {
    format!("{:?}:{}", source.kind, source.name)
}

/// What a source's switch sends. Every source but mail is `inbox.mute_source`. A mail account keeps
/// whether its new mail shows up in its own `notify` (task S.21), so its switch sends
/// `accounts.update_mail_account`; switched on it also lifts an older `inbox.mute_source` rule in
/// the same step, which would otherwise keep the source quiet whatever `notify` says – exactly as
/// Notification Hub › Sources does it.
///
/// Switched on it writes back what the account has stored ([`switched_on`]): an account at "Only
/// ‚For me‘" keeps that step, because the reading side counts it as on ([`State::is_muted`]) – it
/// would otherwise lose the choice and let every newsletter into the inbox again. Only an account
/// stored as `Off` (or one the setup cannot see) starts at `All`. The three steps themselves are
/// set in the Notification Hub, under its gear. Without an account for the source nothing is sent;
/// the core would only answer `not-found`.
fn source_requests(source: &SourceRef, on: bool, data: &Data) -> Action {
    if source.kind != torro_inbox::SourceKind::Mail {
        return Action::Send(Request::MuteSource { source: source.clone(), muted: !on });
    }
    let Some(account) = mail_account(data, source) else { return Action::None };
    let update = Request::UpdateMailAccount {
        id: account.id.clone(),
        sender_name: None,
        signature: None,
        signature_format: None,
        notify: Some(if on { switched_on(mail_notify(data, source)) } else { Notify::Off }),
        services: None,
        folder_roles: None,
        format: None,
    };
    let muted = data.sources.iter().any(|info| info.source == *source && info.muted);
    match on && muted {
        true => Action::SendAll(vec![update, Request::MuteSource { source: source.clone(), muted: false }]),
        false => Action::Send(update),
    }
}

/// The mail account the source `source` stands for; `None` for a source without one.
fn mail_account<'a>(data: &'a Data, source: &SourceRef) -> Option<&'a torro_ipc::AccountInfo> {
    let id = format!("mail:{}", source.name);
    data.accounts.iter().find(|account| account.id == id)
}

/// What a mail source's switch writes when it is turned on: the step the account has, so "Only
/// ‚For me‘" survives being switched off and on again; `All` for one that stands at `Off` and has
/// no step to keep.
fn switched_on(stored: Notify) -> Notify {
    match stored {
        Notify::Off => Notify::All,
        kept => kept,
    }
}

/// What the mail account of `source` stores under `notify`; [`Notify::default`] for a source
/// without such an account (none of the setup's rows, but the switch must read something).
fn mail_notify(data: &Data, source: &SourceRef) -> Notify {
    mail_account(data, source)
        .and_then(|account| account.mail.as_ref())
        .map_or_else(Notify::default, |mail| mail.settings.notify)
}

/// Sends the open form of the calendar or tasks step: the CalDAV account, showing what the
/// components that are on need, or the GitHub token. Nothing while it cannot be sent.
fn submit_form(state: &mut State) -> Action {
    let calendars = !state.off.contains(&Component::Calendar);
    let tasks = !state.off.contains(&Component::Tasks);
    if let Some(form) = state.caldav.as_mut() {
        if !form.can_connect() {
            return Action::None;
        }
        state.error = None;
        return form.connect(calendars, tasks);
    }
    if state.github_token && state.github.has_token() && !state.github.checking() {
        state.error = None;
        return state.github.send_token();
    }
    Action::None
}

/// Follows a Nextcloud sign-in through a reload of `data` (the shell calls it on every reload while
/// the setup is open): signed in, the step shows the account's services; failed, it asks for the
/// address again and says so. While waiting the footer shows the shell's error, if any.
pub fn observe(state: &mut State, data: &Data) {
    // The dialog follows its own Nextcloud sign-in the same way.
    if let Some(dialog) = state.dialog.as_mut() {
        wizard::observe(dialog, data);
    }
    // The web step's switches stay shown over the reloaded list until it shows them; SearXNG's check ends once its
    // address is stored.
    show_switched(state, data);
    web::sent(&mut state.web, Ok(web::stored_address(&data.search_providers)));
    if !state.nextcloud.running() {
        return;
    }
    match state.nextcloud.observe(data) {
        Some(Ended::SignedIn(id)) => {
            state.signed_in = Some(id);
            state.adding = false;
            state.error = None;
        }
        Some(Ended::Failed) => state.error = Some(tr("Signing in did not finish. Try again.").to_string()),
        None => state.error = data.error.clone(),
    }
}

/// The answer to a request the setup sent; `true` when the setup shows it itself (then the shell
/// shows no notice): a sign-in that could not start shows its reason in the footer.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> bool {
    if let Some(dialog) = state.dialog.as_mut() {
        return wizard::answered(dialog, result);
    }
    if let Some(answer) = state.caldav.as_mut().and_then(|form| form.answered(result)) {
        match answer {
            Ok(account) => {
                state.caldav = None;
                state.connected = Some(account);
                state.error = None;
            }
            Err(message) => state.error = Some(message),
        }
        return true;
    }
    if let Some(answer) = state.github.answered(result) {
        match answer {
            Ok(login) => {
                state.github_login = Some(login);
                state.github_token = false;
                state.error = None;
            }
            Err(message) => state.error = Some(message),
        }
        return true;
    }
    if let Some(taken) = ai_answered(state, result) {
        return taken;
    }
    // A refusal may be a provider's switch (answers do not say which request they are for): the switches
    // shown go back to what the core holds, and the next reload shows what it stored.
    if result.is_err() {
        state.switched.clear();
        state.providers = None;
    }
    match state.nextcloud.answered(result) {
        Some(Err(message)) => {
            state.error = Some(message);
            true
        }
        Some(Ok(())) => true,
        None => false,
    }
}

/// The answer to the check of a SearXNG address on the web step ([`Action::CheckAddress`]), which comes here
/// alone: a refusal shows its reason under the field; a success ends the check with the next reload, once the
/// address is stored. `true` when the setup shows it (the shell then shows no notice); `false` once no check waits
/// (the step was left), so the shell says why.
pub fn checked(state: &mut State, result: &Result<Response, String>) -> bool {
    if !web::checking(&state.web) {
        return false;
    }
    if let Err(problem) = result {
        web::sent(&mut state.web, Err(problem));
    }
    true
}

/// The answers of the AI step: the servers found on this computer (without an answer every one
/// shows as not running), and a local server connected from its row. `None` when the answer is
/// not the AI step's.
fn ai_answered(state: &mut State, result: &Result<Response, String>) -> Option<bool> {
    if state.ai_searching {
        state.ai_searching = false;
        state.ai_found = Some(match result {
            Ok(Response::AiServers { servers }) => servers.clone(),
            _ => Vec::new(),
        });
        return Some(true);
    }
    let server = state.ai_connecting.take()?;
    match result {
        Ok(Response::Account { .. }) => {
            state.ai_connected.insert(server);
            state.error = None;
        }
        Ok(Response::Error { message, .. }) | Err(message) => state.error = Some(message.clone()),
        Ok(_) => state.error = Some(tr("Torro Workplace did not answer as expected. Try again.").to_string()),
    }
    Some(true)
}

/// The request that changes one setting.
fn set(key: SettingKey, value: serde_json::Value) -> Action {
    Action::Send(Request::SetSetting { key, value })
}

/// The request that records where the setup stands.
fn set_setup(state: SetupState) -> Action {
    set(SettingKey::Setup, serde_json::to_value(state).unwrap_or_default())
}

/// Decision models the core can actually use for mail under the current local/cloud rule.
fn mail_deciders(data: &Data) -> Vec<(&String, &AiModel)> {
    data.ai_deciders
        .iter()
        .filter(|(account, model)| {
            if !model.enabled || model.decision != Some(true) {
                return false;
            }
            if data.settings.mail_to_cloud {
                return true;
            }
            let Some((provider_id, server)) = account.strip_prefix("ai:").and_then(|id| id.split_once('@')) else {
                return false;
            };
            AiProvider::ALL.into_iter().find(|provider| provider.id() == provider_id).is_some_and(|provider| {
                crate::assistant::place_of(provider, server, model) == crate::assistant::Place::Local
            })
        })
        .map(|(account, model)| (account, model))
        .collect()
}

fn ai_models_wait(data: &Data) -> bool {
    data.accounts
        .iter()
        .filter(|account| account.kind == "ai")
        .map(|account| &account.id)
        .ne(data.ai_deciders_from.iter())
}

/// The wizard recommends the first eligible model unless an assignment or an explicit choice exists.
fn selected_mail_decider<'a>(state: &State, data: &'a Data) -> Option<(&'a String, &'a AiModel)> {
    let candidates = mail_deciders(data);
    let stored = data.settings.ai_assignments.get(&AiFunction::InboxCategorize);
    if let Some((engine, model)) = &state.sort_model {
        return candidates.into_iter().find(|(account, candidate)| *account == engine && candidate.id == *model);
    }
    if let Some(stored) = stored
        && let Some(engine) = &stored.engine
    {
        return candidates.into_iter().find(|(account, candidate)| {
            *account == engine && stored.model.as_ref().is_none_or(|model| candidate.id == *model)
        });
    }
    candidates.into_iter().next()
}

fn mail_sorting_request(state: &State, data: &Data) -> Option<Request> {
    let stored = data.settings.ai_assignments.get(&AiFunction::InboxCategorize);
    let enabled = state.sort_mail.unwrap_or_else(|| stored.is_none_or(|assignment| assignment.enabled));
    let (account, model) = selected_mail_decider(state, data)?;
    if stored.is_some_and(|assignment| {
        assignment.enabled == enabled
            && assignment.engine.as_deref() == Some(account.as_str())
            && assignment.model.as_deref() == Some(model.id.as_str())
    }) {
        return None;
    }
    Some(Request::SetAiAssignment {
        function: AiFunction::InboxCategorize,
        engine: Some(account.clone()),
        model: Some(model.id.clone()),
        effort: None,
        enabled,
    })
}

/// Escape: stop waiting for the browser or adding another Nextcloud, else one step back; nothing
/// on the first step.
pub fn escape(state: &State) -> Option<Message> {
    if state.dialog.is_some() {
        return Some(Message::Dialog(wizard::Message::Close));
    }
    if (state.step == Step::Nextcloud && (state.nextcloud.running() || state.adding)) || state.in_form() {
        return Some(Message::Cancel);
    }
    if let Some(message) = web::escape(&state.web).filter(|_| state.step == Step::Web) {
        return Some(Message::Web(message));
    }
    state.neighbour(false).map(|_| Message::Back)
}

/// The app a component opens on the last step, if it has one.
fn app_of(component: Component) -> Option<AppId> {
    match component {
        Component::Calendar => Some(AppId::CALENDAR),
        Component::Tasks => Some(AppId::TASKS),
        Component::Mail => Some(AppId::INBOX_MAIL),
        Component::Inbox => Some(AppId::INBOX),
        Component::Assistant => Some(AppId::ASSISTANT),
        Component::Workspaces => None,
    }
}

// ----- The frame ------------------------------------------------------------------------------

/// The list of steps for the sidebar (`mode` expanded) or the rail: the suite's logo and "Setup"
/// on top, the steps, and the version at the foot of the expanded list.
pub fn sidebar(state: &State, mode: SidebarMode) -> Element<'_, Message> {
    let items = state
        .steps()
        .into_iter()
        .map(|step| {
            let trail = match state.mark(step) {
                Mark::Passed => Trail::Icon(Icon::Check),
                Mark::Open | Mark::Skipped => Trail::None,
            };
            let label = match state.mark(step) {
                // Said in the row itself: the sidebar's trail has no word.
                Mark::Skipped => trf("{step} · Skipped", &[("step", &step.label())]),
                _ => step.label().to_string(),
            };
            widgets::sidebar_item_in(mode, step.icon(), label, trail, step == state.step, Message::Go(step))
        })
        .collect();
    let mut footer = Vec::new();
    if mode == SidebarMode::Expanded {
        let version = format!("Torro Workplace · {} {}", tr("Version"), env!("CARGO_PKG_VERSION"));
        let version = container(typo::text(typo::meta(), version).style(theme::text_3).wrapping(Wrapping::None))
            .center_y(size::row_sidebar())
            .padding(style::ITEM_PADDING)
            .clip(true);
        footer.push(version.into());
    }
    shell::sidebar(head(mode), items, footer)
}

/// The top of the list of steps: the logo with "Setup" (expanded), the logo alone (rail).
fn head<'a>(mode: SidebarMode) -> Element<'a, Message> {
    let logo = shell::app_mark(AppId::WORKPLACE);
    let content: Element<'a, Message> = match mode {
        SidebarMode::Expanded => row![logo, typo::text(typo::body_strong(), tr("Setup")).wrapping(Wrapping::None)]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into(),
        SidebarMode::Rail => container(logo).center_x(Fill).into(),
    };
    container(content).width(Fill).center_y(size::control()).padding(style::ITEM_PADDING).into()
}

/// The header bar: where the walk stands and "Set up later"; in the narrow layout with "Back" in
/// front and a progress bar.
pub fn header(state: &State, class: SizeClass) -> Element<'_, Message> {
    let (at, of) = state.position();
    let label = state.step.label();
    // On the last step "Finish" is the way out; putting it off there would only confuse.
    let later = (state.step != Step::Done)
        .then(|| widgets::text_button(None, tr("Set up later"), style::button::ghost, Some(Message::Later)));
    if class == SizeClass::Narrow {
        let back =
            state.neighbour(false).map(|_| widgets::icon_button(Icon::ChevronLeft, tr("Back"), Some(Message::Back)));
        let count = typo::text(typo::meta().tabular(), format!("{at}/{of}")).style(theme::text_3);
        let title =
            row![typo::text(typo::body_strong(), label).wrapping(Wrapping::None), widgets::progress_bar(at, of), count]
                .spacing(space::s())
                .align_y(Alignment::Center);
        return shell::header_bar(back, title, later);
    }
    let chevron = icon::icon(Icon::ChevronRight, icon::SIZE_SM)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let step = trf("Step {at} of {of}", &[("at", &at), ("of", &of)]);
    let title = row![
        typo::text(typo::body().tabular(), step).style(theme::text_3).wrapping(Wrapping::None),
        chevron,
        typo::text(typo::body_strong(), label).wrapping(Wrapping::None),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center);
    shell::header_bar([], title, later)
}

/// The add-account dialog over the whole window, while it is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let dialog = state.dialog.as_ref()?;
    let content = wizard::view(dialog, data).map(Message::Dialog);
    Some(super::dialog(content, size::form_max(), Message::Dialog(wizard::Message::Close)))
}

/// The page of the current step over the fixed footer.
pub fn body<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    let page = pages::view(state, data, class);
    let page = container(container(page).width(Fill).max_width(size::form_max())).padding(page_margin(class));
    let page = scrollable(page)
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .height(Fill);
    column![page, rule::horizontal(1).style(style::rule), footer(state, data, class)].into()
}

/// The footer: the step's status at the left; "Back" (not in narrow, where the header has it),
/// "Skip" where the step can be skipped, and the primary button.
fn footer<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    let status: Option<Element<'a, Message>> = match &state.error {
        Some(error) => Some(problem(error.clone())),
        None => pages::status(state, data).map(|text| typo::text(typo::body(), text).style(theme::text_2).into()),
    };
    let mut buttons = row![].spacing(space::s()).align_y(Alignment::Center);
    if class != SizeClass::Narrow && state.neighbour(false).is_some() {
        buttons = buttons.push(widgets::text_button(None, tr("Back"), style::button::ghost, Some(Message::Back)));
    }
    if let Some((label, message)) = pages::skip(state, data) {
        buttons = buttons.push(widgets::text_button(None, label, style::button::secondary, Some(message)));
    }
    let (label, enabled) = pages::primary(state, data);
    buttons = buttons.push(widgets::text_button(None, label, style::button::primary, enabled.then_some(Message::Next)));
    let left = container(row![].push(status)).width(Fill).clip(true);
    container(row![left, buttons].spacing(space::l()).align_y(Alignment::Center))
        .width(Fill)
        .center_y(size::header() + space::m())
        .padding(layout::row_padding())
        .into()
}

/// A problem in the footer: an alert icon in the error colour, the text in `text` – the colour is
/// only in the icon.
fn problem<'a>(text: String) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    row![alert, typo::text(typo::body(), text).wrapping(Wrapping::None)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(step: Step) -> State {
        State::new(step, &Settings::default())
    }

    /// The demo's data: a connected Nextcloud among other accounts.
    fn data() -> Data {
        super::super::launch::demo_data()
    }

    fn sent(outcome: &Outcome) -> Option<(SettingKey, serde_json::Value)> {
        match outcome {
            Outcome::Stay(Action::Send(Request::SetSetting { key, value }))
            | Outcome::Close(Action::Send(Request::SetSetting { key, value })) => Some((*key, value.clone())),
            _ => None,
        }
    }

    #[test]
    fn the_walk_goes_through_the_built_steps_and_ends_done() {
        let mut setup = state(Step::Welcome);
        let walk = [
            Step::Welcome,
            Step::Nextcloud,
            Step::Components,
            Step::Calendar,
            Step::Tasks,
            Step::Mail,
            Step::Inbox,
            Step::Ai,
            Step::Web,
            Step::Documents,
            Step::Done,
        ];
        assert_eq!(setup.steps(), walk);
        assert!(matches!(update(&mut setup, Message::Next, &data()), Outcome::Stay(Action::None)));
        assert_eq!((setup.step, setup.position()), (Step::Nextcloud, (2, 11)));
        update(&mut setup, Message::Next, &data());
        assert_eq!(setup.step, Step::Components, "a Nextcloud is connected: the step goes on");
        for _ in 0..8 {
            update(&mut setup, Message::Next, &data());
        }
        assert_eq!(setup.step, Step::Done);
        let finished = update(&mut setup, Message::Next, &data());
        assert!(matches!(finished, Outcome::Close(_)));
        assert_eq!(sent(&finished), Some((SettingKey::Setup, serde_json::json!("done"))));
    }

    #[test]
    fn later_puts_it_off_and_back_stops_at_the_first_step() {
        let mut setup = state(Step::Welcome);
        assert!(escape(&setup).is_none(), "nothing before the welcome");
        update(&mut setup, Message::Back, &data());
        assert_eq!(setup.step, Step::Welcome);
        let later = update(&mut setup, Message::Later, &data());
        assert!(matches!(later, Outcome::Close(_)));
        assert_eq!(sent(&later), Some((SettingKey::Setup, serde_json::json!("skipped"))));
    }

    #[test]
    fn passed_steps_are_checked_and_clicked_ones_can_be_gone_back_to() {
        let mut setup = state(Step::Welcome);
        for _ in 0..10 {
            update(&mut setup, Message::Next, &data());
        }
        assert_eq!(setup.step, Step::Done);
        assert_eq!(setup.mark(Step::Welcome), Mark::Passed);
        update(&mut setup, Message::Go(Step::Welcome), &data());
        assert_eq!(setup.step, Step::Welcome);
        assert_eq!(setup.mark(Step::Components), Mark::Passed, "reached before, still passed");
        update(&mut setup, Message::Component(Component::Mail, false), &data());
        update(&mut setup, Message::Go(Step::Mail), &data());
        assert_eq!(setup.step, Step::Welcome, "a step that is not walked cannot be gone to");
    }

    #[test]
    fn components_switch_at_once_and_are_sent_one_by_one() {
        let mut setup = state(Step::Components);
        let off = update(&mut setup, Message::Component(Component::Mail, false), &data());
        assert_eq!(
            sent(&off),
            Some((SettingKey::Component, serde_json::json!({"component": "mail", "enabled": false})))
        );
        assert!(setup.off.contains(&Component::Mail));
        update(&mut setup, Message::Component(Component::Mail, true), &data());
        assert!(setup.off.is_empty());
    }

    #[test]
    fn a_component_turned_off_takes_its_step_out_of_the_walk() {
        let all = steps(&BTreeSet::new());
        assert!(all.iter().all(|step| step.built()));
        for step in Step::ALL.into_iter().filter(|step| step.component().is_some()) {
            let off = BTreeSet::from([step.component().expect("a component")]);
            assert!(!steps(&off).contains(&step), "{step:?}");
        }
    }

    #[test]
    fn language_and_appearance_are_sent_as_the_settings_take_them() {
        let mut setup = state(Step::Welcome);
        let german = update(&mut setup, Message::Language(Language::German), &data());
        assert_eq!(sent(&german), Some((SettingKey::Language, serde_json::json!("german"))));
        let dark = update(&mut setup, Message::Appearance(Appearance::Dark), &data());
        assert_eq!(sent(&dark), Some((SettingKey::Appearance, serde_json::json!("dark"))));
        assert_eq!((setup.language, setup.appearance), (Language::German, Appearance::Dark));
    }

    #[test]
    fn the_loaded_settings_count_until_a_choice_is_made_here() {
        let mut setup = state(Step::Components);
        let stored = Settings { components_off: BTreeSet::from([Component::Assistant]), ..Settings::default() };
        setup.loaded(&stored);
        assert_eq!((setup.step, setup.off.clone()), (Step::Components, BTreeSet::from([Component::Assistant])));
        update(&mut setup, Message::Component(Component::Mail, false), &data());
        setup.loaded(&Settings::default());
        assert_eq!(setup.off, BTreeSet::from([Component::Assistant, Component::Mail]), "a reload before the save");
    }

    /// The demo's data without its Nextcloud accounts.
    fn without_nextcloud() -> Data {
        let mut data = data();
        data.accounts.retain(|account| account.kind != "nextcloud");
        data
    }

    #[test]
    fn without_a_nextcloud_the_step_asks_for_the_address_and_signs_in() {
        let mut setup = state(Step::Nextcloud);
        let empty = without_nextcloud();
        assert!(setup.asks_address(&empty));
        assert_eq!(pages::primary(&setup, &empty), (tr("Connect"), false), "no address yet");
        assert!(matches!(update(&mut setup, Message::Next, &empty), Outcome::Stay(Action::None)));
        assert_eq!(setup.step, Step::Nextcloud, "nothing to connect: the step stays");
        assert!(matches!(pages::skip(&setup, &empty), Some((_, Message::Skip))), "I don't use Nextcloud");

        update(&mut setup, Message::ServerChanged("cloud.example.org".into()), &empty);
        let started = update(&mut setup, Message::Next, &empty);
        assert!(matches!(started, Outcome::Stay(Action::Send(Request::ConnectNextcloud { .. }))));
        assert!(setup.busy());
        assert!(!pages::primary(&setup, &empty).1, "one sign-in at a time");
        assert_eq!(escape(&setup).map(|m| format!("{m:?}")), Some("Cancel".into()));

        // The core lists the sign-in, then the new account: the step shows its services.
        let mut waiting = empty.clone();
        waiting.accounts.push(super::super::launch::demo_signing_in());
        observe(&mut setup, &waiting);
        assert!(setup.busy());
        observe(&mut setup, &data());
        assert!(!setup.busy());
        assert_eq!(setup.signed_in.as_deref(), Some("nextcloud:anna@cloud.example.org"));
        assert!(!setup.asks_address(&data()));
        assert_eq!(pages::primary(&setup, &data()), (tr("Continue"), true));
    }

    #[test]
    fn a_sign_in_that_cannot_start_says_why_and_cancel_stops_waiting() {
        let empty = without_nextcloud();
        let mut setup = state(Step::Nextcloud);
        update(&mut setup, Message::ServerChanged("nowhere.example".into()), &empty);
        update(&mut setup, Message::Next, &empty);
        assert!(answered(&mut setup, &Err("no Nextcloud at nowhere.example".into())));
        assert_eq!((setup.busy(), setup.error.as_deref()), (false, Some("no Nextcloud at nowhere.example")));
        assert!(!answered(&mut setup, &Err("something else".into())), "not the setup's answer");

        update(&mut setup, Message::Next, &empty);
        update(&mut setup, Message::Cancel, &empty);
        assert!(!setup.busy() && setup.asks_address(&empty), "back to the address");
    }

    #[test]
    fn another_nextcloud_can_be_added_and_given_up() {
        let mut setup = state(Step::Nextcloud);
        assert!(!setup.asks_address(&data()), "the demo has a Nextcloud");
        assert!(pages::skip(&setup, &data()).is_none(), "connected: nothing to skip");
        update(&mut setup, Message::AddNextcloud, &data());
        assert!(setup.asks_address(&data()));
        assert!(matches!(pages::skip(&setup, &data()), Some((_, Message::Cancel))));
        update(&mut setup, Message::Cancel, &data());
        assert!(!setup.asks_address(&data()));
    }

    #[test]
    fn a_calendar_switch_shows_at_once_and_is_sent() {
        let data = data();
        let mut setup = state(Step::Calendar);
        let id = data.accounts.iter().flat_map(|a| &a.collections).find(|c| c.kind.has_events()).map(|c| c.id.clone());
        let id = id.expect("the demo has a calendar");
        let hide = update(&mut setup, Message::Shown(id.clone(), false), &data);
        assert!(
            matches!(hide, Outcome::Stay(Action::Send(Request::SetCollectionShown { ref collection, shown: false })) if *collection == id)
        );
        assert!(!setup.is_shown(&data, &id), "before the settings are reloaded");
    }

    #[test]
    fn the_caldav_form_connects_what_the_components_need() {
        let mut setup = state(Step::Calendar);
        update(&mut setup, Message::Component(Component::Tasks, false), &data());
        update(&mut setup, Message::OpenCalDav, &data());
        assert!(setup.in_form());
        assert_eq!(pages::primary(&setup, &data()), (tr("Connect"), false), "nothing typed yet");
        assert!(matches!(update(&mut setup, Message::Next, &data()), Outcome::Stay(Action::None)));
        assert_eq!(setup.step, Step::Calendar, "the form stays");

        update(&mut setup, Message::CalDav(caldav::Message::Address("anna@mailbox.org".into())), &data());
        update(&mut setup, Message::CalDav(caldav::Message::Password(Secret::new("secret"))), &data());
        match update(&mut setup, Message::Next, &data()) {
            Outcome::Stay(Action::Send(Request::ConnectCalDav { calendars, tasks, .. })) => {
                assert_eq!((calendars, tasks), (true, false), "tasks are off");
            }
            other => panic!("{other:?}"),
        }
        assert!(setup.busy());
        assert!(answered(&mut setup, &Err("Password was refused".into())));
        assert_eq!(setup.error.as_deref(), Some("Password was refused"));
        assert!(setup.caldav.is_some(), "the form stays for another try");

        update(&mut setup, Message::CalDav(caldav::Message::Password(Secret::new("right"))), &data());
        update(&mut setup, Message::Next, &data());
        let connected = super::super::launch::demo_answer(
            &Request::ConnectCalDav {
                address: "anna@mailbox.org".into(),
                server: None,
                login_name: "anna@mailbox.org".into(),
                password: Secret::new("right"),
                calendars: true,
                tasks: false,
            },
            &data(),
        );
        assert!(answered(&mut setup, &connected.ok_or("the demo answers").map_err(String::from)));
        assert!(setup.caldav.is_none() && setup.connected.is_some() && setup.error.is_none());
    }

    #[test]
    fn github_signs_in_on_the_tasks_step_and_a_form_is_cancelled() {
        let mut setup = state(Step::Tasks);
        let cli = update(&mut setup, Message::UseGithubCli, &data());
        assert!(matches!(cli, Outcome::Stay(Action::Send(Request::ConnectGithub))));
        assert!(setup.busy() && setup.in_form());
        assert_eq!(escape(&setup).map(|m| format!("{m:?}")), Some("Cancel".into()));
        update(&mut setup, Message::Cancel, &data());
        assert!(!setup.busy(), "no longer waited for");

        update(&mut setup, Message::PasteToken, &data());
        update(&mut setup, Message::TokenChanged(Secret::new("ghp_x")), &data());
        assert!(matches!(
            update(&mut setup, Message::Next, &data()),
            Outcome::Stay(Action::Send(Request::SetGithubToken { .. }))
        ));
        let account = data().accounts.into_iter().find(|account| account.kind == "github").expect("the demo's");
        assert!(answered(&mut setup, &Ok(Response::Account { account })));
        assert!(setup.github_login.is_some() && !setup.in_form());
        assert_eq!(setup.step, Step::Tasks, "signing in does not leave the step");
    }

    #[test]
    fn mail_is_added_in_the_dialog_over_the_setup() {
        let mut setup = state(Step::Mail);
        update(&mut setup, Message::AddMail, &data());
        assert!(setup.dialog_open());
        assert_eq!(escape(&setup).map(|m| format!("{m:?}")), Some("Dialog(Close)".into()));
        // The dialog's search runs and its answer goes to the dialog, not to the setup's footer.
        let address = wizard::Message::AddressChanged("anna@mailbox.org".into());
        update(&mut setup, Message::Dialog(address), &data());
        let search = update(&mut setup, Message::Dialog(wizard::Message::Continue), &data());
        assert!(matches!(search, Outcome::Stay(Action::Send(Request::DiscoverServices { .. }))));
        assert!(answered(&mut setup, &Err("offline".into())));
        assert!(setup.error.is_none());
        update(&mut setup, Message::Dialog(wizard::Message::Close), &data());
        assert!(!setup.dialog_open());

        let torromail = update(&mut setup, Message::TakeOverTorroMail, &data());
        assert!(matches!(torromail, Outcome::Stay(Action::Send(Request::TorroMailAccounts))));
    }

    #[test]
    fn inbox_sources_are_muted_by_their_switch() {
        let data = data();
        let mut setup = state(Step::Inbox);
        let github = data.accounts.iter().find(|a| a.kind == "github").expect("the demo's GitHub");
        let login = github.id.split_once(':').map(|(_, name)| name.to_string()).expect("an id with a kind");
        let source = SourceRef { kind: torro_inbox::SourceKind::Github, name: login };
        assert!(!setup.is_muted(&data, &source));
        let off = update(&mut setup, Message::Source(source.clone(), false), &data);
        assert!(matches!(off, Outcome::Stay(Action::Send(Request::MuteSource { muted: true, .. }))));
        assert!(setup.is_muted(&data, &source), "at once");
        let quiet = update(&mut setup, Message::Quiet(false), &data);
        assert_eq!(sent(&quiet), Some((SettingKey::QuietWhenBusy, serde_json::json!(false))));
    }

    /// Task S.21: the source "Mail" reads its state from the account's `notify`, not from the
    /// muted sources. Switching it on sends `accounts.update_mail_account` and lifts an older
    /// `inbox.mute_source` rule in the same step; switching it off sends only the account's setting
    /// – the one place it is stored from now on.
    #[test]
    fn the_mail_source_reads_and_writes_notify() {
        let mut data = data();
        let mut setup = state(Step::Inbox);
        let account = data.accounts.iter().find(|a| a.kind == "mail").expect("the demo's mail").clone();
        let name = account.id.split_once(':').map(|(_, name)| name.to_string()).expect("an id with a kind");
        let source = SourceRef { kind: torro_inbox::SourceKind::Mail, name };
        assert!(!setup.is_muted(&data, &source), "the demo's account shows new mail");

        // Off is stored at the account, not as a muted source.
        let off = update(&mut setup, Message::Source(source.clone(), false), &data);
        let Outcome::Stay(Action::Send(Request::UpdateMailAccount { id, notify, services, .. })) = off else {
            panic!("expected accounts.update_mail_account, got {off:?}");
        };
        assert_eq!((id.as_str(), notify, services), (account.id.as_str(), Some(Notify::Off), None));
        assert!(setup.is_muted(&data, &source), "at once");

        // `notify: off` alone keeps the row off, without any muted source.
        let mut quiet = state(Step::Inbox);
        if let Some(mail) = data.accounts.iter_mut().find(|a| a.id == account.id).and_then(|a| a.mail.as_mut()) {
            mail.settings.notify = Notify::Off;
        }
        assert!(quiet.is_muted(&data, &source), "read from notify");

        // An older mute rule is lifted together with the account's setting.
        data.sources.push(torro_inbox::SourceInfo { source: source.clone(), open: 0, muted: true, health: None });
        let on = update(&mut quiet, Message::Source(source.clone(), true), &data);
        let Outcome::Stay(Action::SendAll(requests)) = on else { panic!("expected two requests, got {on:?}") };
        assert!(
            matches!(&requests[0], Request::UpdateMailAccount { notify: Some(Notify::All), .. }),
            "{:?}",
            requests[0]
        );
        assert!(
            matches!(&requests[1], Request::MuteSource { source: sent, muted: false } if *sent == source),
            "{:?}",
            requests[1]
        );
        assert_eq!(requests.len(), 2);

        // A mail source whose account the core does not list sends nothing.
        let without = Data { accounts: Vec::new(), ..data.clone() };
        let none = update(&mut state(Step::Inbox), Message::Source(source, true), &without);
        assert!(matches!(none, Outcome::Stay(Action::None)), "{none:?}");
    }

    /// Found in the review of task S.22: switching a mail source on writes back the step the
    /// account has. "Only ‚For me‘" counts as on ([`State::is_muted`]), so writing `All` would
    /// quietly undo that choice and let every newsletter into the inbox again.
    #[test]
    fn switching_a_mail_source_on_keeps_only_for_me() {
        assert_eq!(switched_on(Notify::ForMeOnly), Notify::ForMeOnly, "the choice stays");
        assert_eq!(switched_on(Notify::All), Notify::All);
        assert_eq!(switched_on(Notify::Off), Notify::All, "an account that was off has no step to keep");

        let mut data = data();
        let account = data.accounts.iter().find(|a| a.kind == "mail").expect("the demo's mail").clone();
        let name = account.id.split_once(':').map(|(_, name)| name.to_string()).expect("an id with a kind");
        let source = SourceRef { kind: torro_inbox::SourceKind::Mail, name };
        if let Some(mail) = data.accounts.iter_mut().find(|a| a.id == account.id).and_then(|a| a.mail.as_mut()) {
            mail.settings.notify = Notify::ForMeOnly;
        }
        let on = update(&mut state(Step::Inbox), Message::Source(source, true), &data);
        let Outcome::Stay(Action::Send(Request::UpdateMailAccount { notify, .. })) = on else {
            panic!("expected accounts.update_mail_account, got {on:?}");
        };
        assert_eq!(notify, Some(Notify::ForMeOnly), "the newsletters stay out");
    }

    #[test]
    fn the_ai_step_looks_on_this_computer_once_and_connects_from_the_row() {
        let mut setup = state(Step::Inbox);
        let entered = update(&mut setup, Message::Next, &data());
        assert_eq!(setup.step, Step::Ai);
        assert!(matches!(entered, Outcome::Stay(Action::Send(Request::FindAiServers))), "{entered:?}");
        let found = super::super::launch::demo_answer(&Request::FindAiServers, &data()).ok_or("the demo answers");
        assert!(answered(&mut setup, &found.map_err(String::from)));
        assert_eq!(setup.ai_found.as_ref().map(Vec::len), Some(1));
        assert!(matches!(update(&mut setup, Message::Back, &data()), Outcome::Stay(Action::None)));
        assert!(matches!(update(&mut setup, Message::Next, &data()), Outcome::Stay(Action::None)), "looked once");

        let press = Message::ConnectLocalAi(AiProvider::Ollama, "http://localhost:11434/v1".into());
        assert!(matches!(update(&mut setup, press, &data()), Outcome::Stay(Action::Send(Request::ConnectAi { .. }))));
        assert!(setup.busy());
        assert!(answered(&mut setup, &Err("The AI server could not be reached.".into())));
        assert_eq!(setup.error.as_deref(), Some("The AI server could not be reached."));

        let key = update(&mut setup, Message::AddAiKey(AiProvider::DeepSeek), &data());
        assert!(matches!(key, Outcome::Stay(Action::Send(Request::FindAiServers))), "the dialog looks too");
        assert!(setup.dialog_open());
        let engine = update(&mut setup, Message::Engine("ai:openrouter@openrouter.ai".into()), &data());
        assert!(matches!(sent(&engine), Some((SettingKey::AssistantDefault, _))));
    }

    #[test]
    fn the_ai_step_recommends_a_local_decision_model_for_mail() {
        let local = "ai:ollama@localhost:11434".to_string();
        let cloud = "ai:openrouter@openrouter.ai".to_string();
        let model = |id: &str, remote: bool, enabled: bool| AiModel {
            decision: Some(true),
            remote: Some(remote),
            enabled,
            ..AiModel::named(id)
        };
        let mut data = data();
        let mut local_account =
            data.accounts.iter().find(|account| account.kind == "ai").expect("demo AI account").clone();
        local_account.id = local.clone();
        local_account.label = "Ollama".into();
        data.accounts.push(local_account);
        data.ai_deciders_from =
            data.accounts.iter().filter(|account| account.kind == "ai").map(|account| account.id.clone()).collect();
        data.ai_deciders = vec![
            (cloud.clone(), model("cloud", true, true)),
            (local.clone(), model("off", false, false)),
            (local.clone(), model("nimble", false, true)),
            (local.clone(), model("another", false, true)),
        ];
        let mut waiting = data.clone();
        waiting.ai_deciders_from.clear();
        let mut not_ready = state(Step::Ai);
        assert!(!pages::primary(&not_ready, &waiting).1, "continue waits until model choices are loaded");
        assert!(matches!(
            update(&mut not_ready, Message::Next, &waiting),
            Outcome::Stay(Action::Send(Request::FindAiServers))
        ));
        assert_eq!(not_ready.step, Step::Ai);
        assert_eq!(mail_deciders(&data).len(), 2, "cloud and disabled models are not recommended");
        let mut setup = state(Step::Ai);
        let Outcome::Stay(Action::Send(Request::SetAiAssignment { engine, model, enabled, .. })) =
            update(&mut setup, Message::Next, &data)
        else {
            panic!("the recommended model is saved when continuing");
        };
        assert_eq!((engine.as_deref(), model.as_deref(), enabled), (Some(local.as_str()), Some("nimble"), true));
        assert_eq!(setup.step, Step::Web);

        let mut chosen = state(Step::Ai);
        update(&mut chosen, Message::SortModel(local.clone(), "another".into()), &data);
        update(&mut chosen, Message::SortMail(false), &data);
        let Outcome::Stay(Action::Send(Request::SetAiAssignment { model, enabled, .. })) =
            update(&mut chosen, Message::Next, &data)
        else {
            panic!("the explicit off choice is saved");
        };
        assert_eq!((model.as_deref(), enabled), (Some("another"), false));

        data.settings.ai_assignments.insert(
            AiFunction::InboxCategorize,
            torro_core::ai::AiAssignment {
                function: AiFunction::InboxCategorize,
                engine: Some(local.clone()),
                model: Some("another".into()),
                effort: None,
                enabled: false,
            },
        );
        let mut reopened = state(Step::Ai);
        assert!(matches!(update(&mut reopened, Message::Next, &data), Outcome::Stay(Action::None)));
        assert_eq!(reopened.step, Step::Web, "repeating setup preserves the stored off choice");

        data.settings.mail_to_cloud = true;
        assert_eq!(mail_deciders(&data).len(), 3, "the cloud model needs explicit mail permission");
        data.ai_deciders.clear();
        let mut empty = state(Step::Ai);
        assert!(matches!(update(&mut empty, Message::Next, &data), Outcome::Stay(Action::None)));
        assert_eq!(empty.step, Step::Web, "no model is not silently assigned");
    }

    /// The requests of an outcome that stays, one or several.
    fn requests(outcome: Outcome) -> Vec<Request> {
        match outcome {
            Outcome::Stay(Action::Send(request)) => vec![request],
            Outcome::Stay(Action::SendAll(requests)) => requests,
            Outcome::Stay(Action::None) => Vec::new(),
            other => panic!("expected requests, got {other:?}"),
        }
    }

    fn duckduckgo_on() -> Message {
        Message::Web(web::Message::Switch(SearchProviderId::Duckduckgo, true))
    }

    /// The assistant's client of the demo: PDF with write access.
    fn assistant(data: &Data) -> ClientInfo {
        assistant_client(data).expect("the demo's assistant").clone()
    }

    /// Task WEB.7: the first provider switched on sends its switch and switches the tool `web` on; a second one only
    /// its switch. Both show at once, also on the last step.
    #[test]
    fn the_first_provider_on_switches_the_tool_on_too() {
        let data = data();
        let mut setup = state(Step::Web);
        let first = requests(update(&mut setup, duckduckgo_on(), &data));
        assert_eq!(
            first,
            vec![
                web::switch_request(SearchProviderId::Duckduckgo, true),
                crate::ui::connections::switch_request(ToolId::Web, true)
            ]
        );
        assert!(first.iter().all(|request| request.operation().area() == Area::Settings), "nothing of clients.*");
        let second =
            requests(update(&mut setup, Message::Web(web::Message::Switch(SearchProviderId::Wikipedia, true)), &data));
        assert_eq!(second, vec![web::switch_request(SearchProviderId::Wikipedia, true)]);
        assert_eq!(pages::providers_on(&setup, &data), ["DuckDuckGo", "Wikipedia"], "shown at once");
        assert_eq!(pages::status(&setup, &data).as_deref(), Some(tr("Saving…")), "until the core lists them");
        let both = with_both(&data);
        observe(&mut setup, &both);
        assert_eq!(pages::status(&setup, &both), Some(trf("{n} providers on", &[("n", &2)])));
        let mut on = data.clone();
        on.settings.tools.set_on(ToolId::Web, true);
        let mut again = state(Step::Web);
        assert_eq!(requests(update(&mut again, duckduckgo_on(), &on)).len(), 1, "the tool is on already");
    }

    /// `data` as the core lists it once DuckDuckGo's and Wikipedia's switches are stored.
    fn with_both(data: &Data) -> Data {
        let mut stored = with_duckduckgo(data);
        for info in &mut stored.search_providers {
            info.enabled |= info.id == SearchProviderId::Wikipedia;
        }
        stored
    }

    /// `data` as the core lists it once DuckDuckGo's switch is stored.
    fn with_duckduckgo(data: &Data) -> Data {
        let mut stored = data.clone();
        for info in &mut stored.search_providers {
            info.enabled |= info.id == SearchProviderId::Duckduckgo;
        }
        stored
    }

    /// With "Let the assistant use it" ticked, "Continue" sends one `clients.update_client` that keeps the
    /// assistant's tools and rights and adds read access to `web`; once, however often the step is passed.
    #[test]
    fn continue_gives_the_assistant_web_and_keeps_its_tools() {
        let data = with_duckduckgo(&data());
        let before = assistant(&data);
        assert_eq!(before.rights.tools, Selection::Only(vec!["pdf".into()]));
        let mut setup = state(Step::Web);
        update(&mut setup, duckduckgo_on(), &data);
        let sent = requests(update(&mut setup, Message::Next, &data));
        let [Request::UpdateClient { id, rights }] = sent.as_slice() else { panic!("one update_client, got {sent:?}") };
        assert_eq!(*id, before.id);
        assert_eq!(rights.tools, Selection::Only(vec!["pdf".into(), "web".into()]), "PDF stays");
        assert_eq!(rights.area(Area::Tools), before.rights.area(Area::Tools), "write access stays write");
        assert_eq!(Rights { tools: before.rights.tools.clone(), ..rights.clone() }, before.rights, "nothing else");
        assert!(rights.reaches_tool(ToolId::Web) && rights.reaches_tool(ToolId::Pdf));
        assert_eq!(setup.step, Step::Documents);
        update(&mut setup, Message::Back, &data);
        assert!(requests(update(&mut setup, Message::Next, &data)).is_empty(), "at most once");
    }

    /// Unticked, or without the assistant's client, "Continue" sends nothing; without a provider on neither.
    #[test]
    fn without_the_tick_or_the_client_continue_sends_nothing() {
        let mut none_on = state(Step::Web);
        assert!(requests(update(&mut none_on, Message::Next, &data())).is_empty(), "no provider: nothing to use");

        let data = with_duckduckgo(&data());
        let mut setup = state(Step::Web);
        update(&mut setup, duckduckgo_on(), &data);
        update(&mut setup, Message::AllowWeb(false), &data);
        assert!(requests(update(&mut setup, Message::Next, &data)).is_empty());

        let mut without = data.clone();
        without.clients.retain(|client| client.kind != ASSISTANT_CLIENT);
        let mut setup = state(Step::Web);
        update(&mut setup, duckduckgo_on(), &without);
        assert!(requests(update(&mut setup, Message::Next, &without)).is_empty());
        let _ = pages::view(&setup, &without, SizeClass::Wide);
    }

    /// Found in the review of #305: a switch shows at once, but only until the core answers. Refused, it goes
    /// back to what the core holds; stored, the core's list shows it. "Continue" goes by the core's list, so it
    /// never gives the assistant `web` while no provider is on.
    #[test]
    fn a_refused_switch_goes_back_and_continue_goes_by_the_core() {
        let data = data();
        let duckduckgo = |setup: &State, data: &Data| {
            providers(setup, data).iter().any(|info| info.id == SearchProviderId::Duckduckgo && info.enabled)
        };
        let mut setup = state(Step::Web);
        update(&mut setup, duckduckgo_on(), &data);
        assert!(duckduckgo(&setup, &data), "shown at once");
        assert!(!answered(&mut setup, &Err("refused".into())), "the shell says why");
        assert!(!duckduckgo(&setup, &data), "back to what the core holds");
        observe(&mut setup, &data);
        assert!(!duckduckgo(&setup, &data), "and the next reload does not bring it back");
        assert!(requests(update(&mut setup, Message::Next, &data)).is_empty(), "no provider on: nothing given");

        // Shown but not yet stored: "Continue" waits for the core's word ([`continue_waits_for_the_cores_word`]).
        let mut early = state(Step::Web);
        update(&mut early, duckduckgo_on(), &data);
        assert!(requests(update(&mut early, Message::Next, &data)).is_empty());
        assert_eq!(early.step, Step::Web, "the click waits, it does not pass the step");

        // Stored: the core's list carries it, the overlay is done.
        let stored = with_duckduckgo(&data);
        let mut setup = state(Step::Web);
        update(&mut setup, duckduckgo_on(), &data);
        observe(&mut setup, &stored);
        assert!(setup.switched.is_empty() && setup.providers.is_none(), "confirmed");
        assert!(duckduckgo(&setup, &stored));
    }

    /// Found in the second review of #305: a click on "Continue" before the core confirmed a switch is not lost.
    /// While a switch waits and "Continue" would give the assistant `web`, the button waits (pressed, it stays on
    /// the step and sends nothing); once the core lists the switch, "Continue" gives `web` and goes on. Unticked
    /// nothing hangs on the core's word, so the button goes on at once.
    #[test]
    fn continue_waits_for_the_cores_word() {
        let data = data();
        let mut setup = state(Step::Web);
        update(&mut setup, duckduckgo_on(), &data);
        assert_eq!(pages::primary(&setup, &data), (tr("Continue"), false), "waits for the core");
        assert_eq!(pages::status(&setup, &data).as_deref(), Some(tr("Saving…")));
        assert!(setup.busy(), "the shell reloads sooner");
        assert!(requests(update(&mut setup, Message::Next, &data)).is_empty());
        assert_eq!(setup.step, Step::Web, "Enter does not pass the waiting button by");

        let stored = with_duckduckgo(&data);
        observe(&mut setup, &stored);
        assert_eq!(pages::primary(&setup, &stored), (tr("Continue"), true), "confirmed");
        assert!(!setup.busy());
        let sent = requests(update(&mut setup, Message::Next, &stored));
        assert!(matches!(sent.as_slice(), [Request::UpdateClient { .. }]), "{sent:?}");
        assert_eq!(setup.step, Step::Documents);

        let mut unticked = state(Step::Web);
        update(&mut unticked, duckduckgo_on(), &data);
        update(&mut unticked, Message::AllowWeb(false), &data);
        assert_eq!(pages::primary(&unticked, &data), (tr("Continue"), true), "nothing to wait for");
        update(&mut unticked, Message::Next, &data);
        assert_eq!(unticked.step, Step::Documents);
    }

    /// Found in the second review of #305: SearXNG without an address is switched on but cannot be asked
    /// (`WebSearch::askable`), so it counts for neither "Continue" nor the first provider: alone it gives the
    /// assistant nothing, and the next provider switched on still switches the tool on.
    #[test]
    fn searxng_without_an_address_counts_for_nothing() {
        let mut data = data();
        for info in &mut data.search_providers {
            if info.id == SearchProviderId::Searxng {
                (info.enabled, info.address) = (true, None);
            }
        }
        let mut setup = state(Step::Web);
        assert!(requests(update(&mut setup, Message::Next, &data)).is_empty(), "nothing can be asked");

        let mut setup = state(Step::Web);
        let sent = requests(update(&mut setup, duckduckgo_on(), &data));
        assert!(
            sent.contains(&crate::ui::connections::switch_request(ToolId::Web, true)),
            "the first askable: {sent:?}"
        );

        // With its address it counts.
        for info in &mut data.search_providers {
            if info.id == SearchProviderId::Searxng {
                info.address = Some("https://search.example.org".into());
            }
        }
        let mut setup = state(Step::Web);
        let sent = requests(update(&mut setup, Message::Next, &data));
        assert!(matches!(sent.as_slice(), [Request::UpdateClient { .. }]), "{sent:?}");
        let mut setup = state(Step::Web);
        assert_eq!(requests(update(&mut setup, duckduckgo_on(), &data)).len(), 1, "not the first");
    }

    /// Found in the review of #305: while SearXNG's address is checked, the refusal of another request is not
    /// taken for the check's; only the check's own answer ends it ([`checked`]).
    #[test]
    fn only_the_checks_own_answer_ends_it() {
        let data = data();
        let mut setup = state(Step::Web);
        update(&mut setup, Message::Web(web::Message::TypeAddress("https://search.example.org".into())), &data);
        let check = update(&mut setup, Message::Web(web::Message::CheckAddress), &data);
        let Outcome::Stay(Action::CheckAddress(request)) = check else { panic!("the check apart, got {check:?}") };
        assert_eq!(request, web::address_request(Some("https://search.example.org")));
        update(&mut setup, duckduckgo_on(), &data);
        assert!(!answered(&mut setup, &Err("the switch was refused".into())), "the shell says it");
        assert!(setup.busy(), "the check still waits");
        assert!(checked(&mut setup, &Err("This SearXNG instance does not answer in JSON.".into())));
        assert!(!setup.busy());
        assert!(!checked(&mut setup, &Err("late".into())), "no check waits: the shell says it");
    }

    /// "Skip" leaves the web search off and sends nothing; Escape drops a key being typed first.
    #[test]
    fn skipping_the_web_step_sends_nothing() {
        let data = data();
        let mut setup = state(Step::Web);
        assert_eq!(pages::status(&setup, &data).as_deref(), Some(tr("Web search is off")));
        let key = web::Message::TypeKey(SearchProviderId::Brave, Secret::new("BSA-1"));
        assert!(requests(update(&mut setup, Message::Web(key), &data)).is_empty());
        assert_eq!(escape(&setup).map(|m| format!("{m:?}")), Some("Web(Discard)".into()));
        assert!(requests(update(&mut setup, Message::Skip, &data)).is_empty());
        assert_eq!((setup.step, setup.mark(Step::Web)), (Step::Documents, Mark::Skipped));
        assert!(escape(&setup).is_some_and(|m| matches!(m, Message::Back)), "the typed key went with the step");
    }

    /// "All tools" does not include `web`: given it, the assistant gets every tool by name, PDF stays reachable.
    /// An area without access gets read access; one with more keeps it.
    #[test]
    fn web_is_added_by_name_and_all_tools_stays_reachable() {
        let mut all = Rights::assistant();
        all.tools = Selection::All;
        all.areas.insert(Area::Tools, AreaRights { level: Level::Full, admin: true });
        let given = with_web(&all);
        assert_eq!(given.tools, Selection::Only(ToolId::ALL.into_iter().map(|t| t.name().to_string()).collect()));
        assert_eq!(given.area(Area::Tools), AreaRights { level: Level::Full, admin: true });
        assert!(given.reaches_tool(ToolId::Pdf) && given.reaches_tool(ToolId::Web));

        let mut closed = Rights::assistant();
        closed.areas.remove(&Area::Tools);
        closed.tools = Selection::Only(Vec::new());
        let given = with_web(&closed);
        assert_eq!(
            (given.area(Area::Tools).level, given.tools.clone()),
            (Level::Read, Selection::Only(vec!["web".into()]))
        );
        assert_eq!(with_web(&given), given, "already there: unchanged");
    }

    /// A refused SearXNG address shows under its field, not as the shell's notice; the last step names the
    /// providers that are on.
    #[test]
    fn a_refused_address_stays_on_the_page() {
        let data = data();
        let mut setup = state(Step::Web);
        update(&mut setup, Message::Web(web::Message::TypeAddress("https://search.example.org".into())), &data);
        let check = update(&mut setup, Message::Web(web::Message::CheckAddress), &data);
        assert!(matches!(check, Outcome::Stay(Action::CheckAddress(_))), "{check:?}");
        assert!(setup.busy());
        assert!(checked(&mut setup, &Err("This SearXNG instance does not answer in JSON.".into())));
        assert!(!setup.busy());
        assert!(!answered(&mut setup, &Err("another refusal".into())), "no check waits: the shell says it");

        update(&mut setup, duckduckgo_on(), &data);
        setup.go(Step::Done);
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            let _ = pages::view(&setup, &data, class);
        }
        assert_eq!(pages::providers_on(&setup, &data), ["DuckDuckGo"]);
        assert_eq!(pages::web_detail(&setup, &data), trf("Web search · {providers}", &[("providers", &"DuckDuckGo")]));
        assert_eq!(pages::web_detail(&state(Step::Done), &data), tr("Web search off"));
    }

    #[test]
    fn steps_are_named_by_their_word() {
        for step in Step::ALL.into_iter().filter(|step| step.built()) {
            assert_eq!(Step::from_arg(step.arg()), Some(step));
        }
        assert_eq!(Step::from_arg("later"), None, "no such step");
        assert_eq!(
            Step::args(),
            [
                "welcome",
                "nextcloud",
                "components",
                "calendar",
                "tasks",
                "mail",
                "inbox",
                "ai",
                "web",
                "documents",
                "done"
            ]
        );
    }

    /// Issue #303: the step Documents is the assistant's, so it is walked only while the assistant is on, right
    /// before the end.
    #[test]
    fn documents_come_after_the_web_search_with_the_assistant() {
        let all = steps(&BTreeSet::new());
        let at = all.iter().position(|step| *step == Step::Documents).expect("walked");
        assert_eq!((all[at - 1], all[at + 1]), (Step::Web, Step::Done));
        let without = steps(&BTreeSet::from([Component::Assistant]));
        assert!(!without.contains(&Step::Documents) && !without.contains(&Step::Web));
    }

    /// Issue #303: each ArtCraft app shows how it stands – set up as a server, its program found, or missing –, and
    /// "Get <App>" opens the releases the catalog entry names; nothing is stored.
    #[test]
    fn documents_show_each_app_as_it_stands() {
        let mut data = data();
        super::super::launch::demo_documents(&mut data);
        let entry = |id: &str| data.catalog.entries().iter().find(|entry| entry.id == id).cloned().expect("listed");
        let states: Vec<pages::AppState> =
            pages::APPS.iter().map(|(id, ..)| pages::app_state(&data, &entry(id))).collect();
        use pages::AppState::{Installed, Missing, SetUp};
        assert_eq!(states, [SetUp, Missing, Installed, Missing]);
        let deckcraft = entry("deckcraft");
        let mut setup = state(Step::Documents);
        match update(&mut setup, Message::GetApp(deckcraft.url.clone()), &data) {
            Outcome::Stay(Action::OpenUrl(url)) => assert_eq!(url, "https://github.com/storytold/deckcraft/releases"),
            other => panic!("expected the releases in the browser, got {other:?}"),
        }
        assert_eq!(pages::primary(&setup, &data), (tr("Continue"), true));
        assert!(pages::skip(&setup, &data).is_some(), "it can be skipped like any step");
        assert!(matches!(update(&mut setup, Message::Next, &data), Outcome::Stay(Action::None)), "stores nothing");
        assert_eq!(setup.step, Step::Done);
    }

    /// Review of PR #316: before the catalog answers the step shows "Loading …", a failed list a sentence, and a
    /// catalog without the ArtCraft apps (a service of another version) another; otherwise the four apps in order.
    #[test]
    fn documents_say_how_the_catalog_answered() {
        use super::super::connections::Listing;
        use pages::Listed;
        let mut data = data();
        let Listed::Apps(apps) = pages::listed(&data) else { panic!("the demo lists the apps") };
        let ids: Vec<&str> = apps.iter().map(|(entry, ..)| entry.id.as_str()).collect();
        assert_eq!(ids, ["wordcraft", "deckcraft", "gridcraft", "pdfcraft"]);
        let catalog = data.catalog.entries().to_vec();
        data.catalog = Listing::Entries(Vec::new());
        assert_eq!(pages::listed(&data), Listed::Loading);
        data.catalog = Listing::Failed("unsupported".into());
        assert_eq!(pages::listed(&data), Listed::Failed);
        data.catalog = Listing::Entries(catalog.into_iter().filter(|entry| !entry.id.ends_with("craft")).collect());
        assert_eq!(pages::listed(&data), Listed::NotInCatalog);
    }
}
