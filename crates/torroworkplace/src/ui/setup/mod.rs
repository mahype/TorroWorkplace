//! The first-run setup (task W.4, concept `docs/einrichtung.md`): a mode of the control centre
//! that leads once through what the suite needs – Nextcloud first, then the components a person
//! wants, then each chosen component on its own, AI last – and ends on a summary.
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

mod pages;

use std::collections::{BTreeMap, BTreeSet};

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, rule, scrollable, svg};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::AppId;
use torro_core::account::Secret;
use torro_core::ai::AiProvider;
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, ComponentSwitch, Language, Settings, SetupState};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, SidebarMode};
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets::{self, Trail};
use torro_gui::{style, typo};
use torro_ipc::{AiServer, Request, Response, SettingKey};

use super::connect::caldav::{self, CalDavSignIn};
use super::connect::github::{self, GithubSignIn};
use super::connect::nextcloud::{self, Ended, NextcloudSignIn};
use super::{Action, Data, page_margin, wizard};
use torro_inbox::SourceRef;

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
    Done,
}

impl Step {
    pub const ALL: [Step; 9] = [
        Step::Welcome,
        Step::Nextcloud,
        Step::Components,
        Step::Calendar,
        Step::Tasks,
        Step::Mail,
        Step::Inbox,
        Step::Ai,
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
            _ => None,
        }
    }

    /// Whether the step's page exists yet: W.4 built Welcome, Components and Done, W.5 Nextcloud,
    /// W.6 Calendar and Tasks, W.7 Mail and Inbox, W.8 the AI: all of them now. A step that is not
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
    /// ntfy, exactly as Accounts › Add account has them.
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
    /// The engine chosen here for the assistant's new chats, by id.
    engine: Option<String>,
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
            engine: None,
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

    /// A sign-in runs; the shell reloads more often, so the end is seen soon.
    pub fn busy(&self) -> bool {
        self.nextcloud.running()
            || self.github.checking()
            || self.caldav.as_ref().is_some_and(CalDavSignIn::connecting)
            || self.dialog.as_ref().is_some_and(wizard::State::busy)
            || self.ai_searching
            || self.ai_connecting.is_some()
    }

    /// The add-account dialog lies over the setup; keys and the footer belong to it.
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// Whether the inbox source `source` is muted: as switched here, else as the core lists it.
    fn is_muted(&self, data: &Data, source: &SourceRef) -> bool {
        self.muted
            .get(&source_key(source))
            .copied()
            .unwrap_or_else(|| data.sources.iter().any(|info| info.source == *source && info.muted))
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
    /// "Take over from TorroMail": the add-account dialog with TorroMail's accounts on top.
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
            stay(Action::Send(Request::MuteSource { source, muted: !on }))
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
    }
}

/// Opens the add-account dialog at the AI provider `provider`: its key, or the address of another
/// server. The dialog looks for local servers as it opens, like Accounts › Add account.
fn open_ai_dialog(state: &mut State, provider: AiProvider, data: &Data) -> Action {
    let search = open_dialog(state, Some(wizard::Provider::Ai), data);
    if let Some(dialog) = state.dialog.as_mut() {
        wizard::update(dialog, wizard::Message::ChooseAi(provider), data);
    }
    search
}

/// Opens the add-account dialog over the setup: on the path of `provider`, or on the catalog with
/// TorroMail's accounts on top (it asks for them as it opens).
fn open_dialog(state: &mut State, provider: Option<wizard::Provider>, data: &Data) -> Action {
    state.close_forms();
    state.error = None;
    match provider {
        Some(provider) => {
            let mut dialog = wizard::State::new();
            let action = wizard::update(&mut dialog, wizard::Message::ChooseProvider(provider), data);
            state.dialog = Some(dialog);
            action
        }
        None => {
            let (dialog, ask) = wizard::State::open();
            state.dialog = Some(dialog);
            ask
        }
    }
}

/// A source's key for the switches kept here: kind and name.
fn source_key(source: &SourceRef) -> String {
    format!("{:?}:{}", source.kind, source.name)
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
    match state.nextcloud.answered(result) {
        Some(Err(message)) => {
            state.error = Some(message);
            true
        }
        Some(Ok(())) => true,
        None => false,
    }
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

/// Escape: stop waiting for the browser or adding another Nextcloud, else one step back; nothing
/// on the first step.
pub fn escape(state: &State) -> Option<Message> {
    if state.dialog.is_some() {
        return Some(Message::Dialog(wizard::Message::Close));
    }
    if (state.step == Step::Nextcloud && (state.nextcloud.running() || state.adding)) || state.in_form() {
        return Some(Message::Cancel);
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
    let page = pages::view(state, data);
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
            Step::Done,
        ];
        assert_eq!(setup.steps(), walk);
        assert!(matches!(update(&mut setup, Message::Next, &data()), Outcome::Stay(Action::None)));
        assert_eq!((setup.step, setup.position()), (Step::Nextcloud, (2, 9)));
        update(&mut setup, Message::Next, &data());
        assert_eq!(setup.step, Step::Components, "a Nextcloud is connected: the step goes on");
        for _ in 0..6 {
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
        for _ in 0..8 {
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
        let engine = update(&mut setup, Message::Engine("ai:ollama@localhost:11434".into()), &data());
        assert!(matches!(sent(&engine), Some((SettingKey::AssistantDefault, _))));
    }

    #[test]
    fn steps_are_named_by_their_word() {
        for step in Step::ALL.into_iter().filter(|step| step.built()) {
            assert_eq!(Step::from_arg(step.arg()), Some(step));
        }
        assert_eq!(Step::from_arg("later"), None, "no such step");
        assert_eq!(
            Step::args(),
            ["welcome", "nextcloud", "components", "calendar", "tasks", "mail", "inbox", "ai", "done"]
        );
    }
}
