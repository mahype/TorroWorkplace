//! The first-run setup (issue #326, concept `docs/einrichtung.md`, drafts `docs/prototypes/onboarding/`): a mode
//! of the control centre that leads once through five points – Mail and calendar, Cloud, AI, Appearance,
//! Documents – between a welcome and a summary.
//!
//! `torroworkplace ui --setup [--step <step>]` opens it; `--demo --open setup-<step>` shows a step with sample
//! data. The setup is a guided way through the requests the control centre sends anyway: nothing is stored
//! here, and what a step connects or switches is saved at once, so closing the window halfway loses nothing.
//!
//! The window has no sidebar: a header with the suite's logo, "Setup", the stepper of the five points and
//! "Set up later" (design skill, section 0; [`header`]):
//!
//! - **wide:** the points with number and name, passed ones with a check mark, the current one in `text` and
//!   never red; on the AI point three small bars show which of its pages is open. Every point can be clicked.
//! - **medium:** the numbers alone, the names as tooltips.
//! - **narrow:** "‹" back, "3/5 · AI" (on the AI point with "· 1/3") with a progress bar, and "Later".
//!
//! Under the header the step's page, at most [`size::form_max()`] wide (Appearance as wide as the window, it
//! has its preview beside the choices), and at the bottom a fixed footer: a status, a problem or a link at
//! the left, "Back", the step's way to skip it and the one primary button at the right; narrow the buttons
//! stand one under the other. Enter is the primary button, Escape goes back.
//!
//! The AI point has up to three pages ([`AiPage`]): Connect with "Our suggestion for this PC", whose "Use
//! suggestion" prepares both models and goes on to Appearance at once (the download goes on meanwhile);
//! "Choose myself" leads through Chat and Sorting.
//!
//! "Finish" derives which apps are shown from what is connected ([`components`]): on the first run it hides
//! Mail, Calendar, Tasks and the assistant without a connection; run again it only shows, never hides, so a
//! choice made under Apps is kept.

mod pages;

pub(super) use pages::name as component_name;

use std::collections::BTreeSet;

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, rule, scrollable};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::AppId;
use torro_core::ai::{AiAssignment, AiFunction, AiModel, AiProvider, AiSlot};
use torro_core::i18n::tr;
use torro_core::settings::{Component, ComponentSwitch, Language, Settings, SetupState};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, DROPDOWN_SEARCH, Dropdown, DropdownEvent, TileFace, TileSize};
use torro_gui::{style, typo};
use torro_ipc::{Request, Response, SettingKey};
use torro_settings_ui::accounts::ai_setup;
use torro_settings_ui::suite;

use super::accounts::act;
use super::connect::nextcloud::{self, Ended, NextcloudSignIn};
use super::{Action, Data, Section, page_margin, wizard};

/// A step of the setup, in the order it is walked: the welcome, the five points, the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    Welcome,
    Mail,
    Cloud,
    Ai,
    Appearance,
    Documents,
    Done,
}

/// The pages of the point AI (issue #326): Connect always, Chat and Sorting only through "Choose myself".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AiPage {
    #[default]
    Connect,
    Chat,
    Sort,
}

impl AiPage {
    /// Where the page stands among the three, from 1.
    fn at(self) -> usize {
        match self {
            AiPage::Connect => 1,
            AiPage::Chat => 2,
            AiPage::Sort => 3,
        }
    }
}

impl Step {
    pub const ALL: [Step; 7] =
        [Step::Welcome, Step::Mail, Step::Cloud, Step::Ai, Step::Appearance, Step::Documents, Step::Done];

    /// The five points of the stepper, numbered from 1.
    pub const POINTS: [Step; 5] = [Step::Mail, Step::Cloud, Step::Ai, Step::Appearance, Step::Documents];

    /// The word for `--step` and `--open setup-<step>`.
    pub fn arg(self) -> &'static str {
        match self {
            Step::Welcome => "welcome",
            Step::Mail => "mail",
            Step::Cloud => "cloud",
            Step::Ai => "ai",
            Step::Appearance => "appearance",
            Step::Documents => "documents",
            Step::Done => "done",
        }
    }

    /// The step named `arg`, if there is one. The words of the steps before issue #326 lead to the point
    /// that holds them now, so an older caller (`--step calendar`) still opens the setup.
    pub fn from_arg(arg: &str) -> Option<Step> {
        let step = match arg {
            "nextcloud" => Step::Cloud,
            "calendar" | "tasks" | "inbox" => Step::Mail,
            "components" => Step::Welcome,
            "web" => Step::Ai,
            word => return Step::ALL.into_iter().find(|step| step.arg() == word),
        };
        Some(step)
    }

    /// The words `--step` takes.
    pub fn args() -> Vec<&'static str> {
        Step::ALL.into_iter().map(Step::arg).collect()
    }

    pub fn label(self) -> &'static str {
        match self {
            Step::Welcome => tr("Welcome"),
            Step::Mail => tr("Mail and calendar"),
            Step::Cloud => tr("Cloud"),
            Step::Ai => tr("AI"),
            Step::Appearance => tr("Appearance"),
            Step::Documents => tr("Documents"),
            Step::Done => tr("All set"),
        }
    }

    /// The point's number in the stepper (from 1); `None` for the welcome and the summary.
    fn number(self) -> Option<usize> {
        Step::POINTS.iter().position(|point| *point == self).map(|index| index + 1)
    }
}

/// The setup while it is open.
#[derive(Debug)]
pub struct State {
    pub(super) ai_setup: ai_setup::State,
    step: Step,
    /// The page of the point AI.
    ai_page: AiPage,
    /// The steps shown in this setup; a point shown and left counts as passed unless it was skipped.
    visited: BTreeSet<Step>,
    /// Steps left with their way to skip; going on from one of them with the primary button takes it out.
    skipped: BTreeSet<Step>,
    /// The language chosen here, shown at once. The settings are only reloaded after the request, and the
    /// demo stores nothing, so the page keeps what was chosen itself.
    language: Language,
    /// A choice was made here: from then on the page's choice counts, not the reloaded settings.
    touched: bool,
    /// The point Cloud: the address and the Nextcloud sign-in while it runs.
    nextcloud: NextcloudSignIn,
    /// The point Cloud shows the address form although an account exists ("Add another Nextcloud").
    adding: bool,
    /// The Nextcloud account signed in to in this setup; its services are shown.
    signed_in: Option<String>,
    /// What went wrong on the current step, for the footer: the core's answer or the shell's error.
    error: Option<String>,
    /// The mail path on the point Mail and calendar (issue #326): the same path as Accounts › Add, shown
    /// on the page itself – address, password, services.
    mail: wizard::State,
    /// The dialog over the setup that takes accounts over from TorroMail.
    dialog: Option<wizard::State>,
    /// The chat model chosen on the page Chat, by account and model; sent at once.
    chat: Option<(String, String)>,
    /// Mail sorting is recommended on; an explicit choice here overrides the stored assignment.
    sort_mail: Option<bool>,
    sort_model: Option<(String, String)>,
    /// "Use the chat model" on the page Sorting: the chat model sorts, answering in a fixed form.
    sort_by_chat: bool,
    /// The question before sorting turns on a model elsewhere (issue #424), while it is open: the model's
    /// and the provider's names.
    asking_cloud: Option<(String, String)>,
    /// Account/model behind the question; nothing is selected until Use.
    pending_sort: Option<(String, Option<String>, bool)>,
    /// The question is about the chat model ("Use the chat model").
    pending_by_chat: bool,
    confirmed_sort: Option<(String, Option<String>)>,
    /// The dropdown that is open: the chat model or the decision model for mail (issue #554).
    menus: Dropdown<SetupMenu>,
}

/// A dropdown of the setup (issue #554).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SetupMenu {
    /// "Model for chat" on the page Chat.
    Chat,
    /// "Decision model" on the page Sorting.
    SortModel,
}

impl State {
    /// The setup on `step`, with the choices as `settings` has them.
    pub fn new(step: Step, settings: &Settings) -> State {
        let mut ai = ai_setup::State::default();
        ai_setup::hosted(&mut ai);
        State {
            ai_setup: ai,
            step,
            ai_page: AiPage::Connect,
            visited: BTreeSet::from([step]),
            skipped: BTreeSet::new(),
            language: settings.language,
            touched: false,
            nextcloud: NextcloudSignIn::default(),
            adding: false,
            signed_in: None,
            error: None,
            mail: wizard::mail(),
            dialog: None,
            chat: None,
            sort_mail: None,
            sort_model: None,
            sort_by_chat: false,
            asking_cloud: None,
            pending_sort: None,
            pending_by_chat: false,
            confirmed_sort: None,
            menus: Dropdown::default(),
        }
    }

    /// The setup on the page `page` of the point AI (the demo's `setup-ai-chat`, `setup-ai-sort`).
    pub fn on_ai_page(mut self, page: AiPage) -> State {
        self.step = Step::Ai;
        self.ai_page = page;
        self.visited.insert(Step::Ai);
        self
    }

    /// Takes the choices from freshly loaded `settings` while none was made here yet: the setup can open
    /// before the core answered the first time.
    pub fn loaded(&mut self, settings: &Settings) {
        if !self.touched {
            self.language = settings.language;
        }
    }

    /// A sign-in runs or a model is prepared; the shell reloads more often, so the end is seen soon.
    pub fn busy(&self) -> bool {
        self.nextcloud.running()
            || self.mail.busy()
            || self.dialog.as_ref().is_some_and(wizard::State::busy)
            || ai_working(self)
    }

    /// The dialog lies over the setup; keys and the footer belong to it.
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// The step shown.
    pub fn step(&self) -> Step {
        self.step
    }

    /// Whether the shared AI page must keep running: its point is shown, or a model it prepares is still on
    /// its way (after "Use suggestion" the setup goes on while the download runs).
    pub fn ai_running(&self) -> bool {
        self.step == Step::Ai || ai_working(self)
    }

    /// The step after (`forward`) or before the current one, if any.
    fn neighbour(&self, forward: bool) -> Option<Step> {
        let index = Step::ALL.iter().position(|step| *step == self.step)?;
        match forward {
            true => Step::ALL.get(index + 1).copied(),
            false => index.checked_sub(1).and_then(|before| Step::ALL.get(before).copied()),
        }
    }

    /// Whether the point `step` is passed: shown in this setup, not skipped, and not the one shown now.
    fn passed(&self, step: Step) -> bool {
        self.visited.contains(&step) && !self.skipped.contains(&step) && step != self.step
    }

    fn go(&mut self, step: Step) {
        // A model on its way keeps the AI page's work running ([`State::ai_running`]).
        if self.step == Step::Ai && step != Step::Ai && !ai_working(self) {
            ai_setup::leave(&mut self.ai_setup);
        }
        if step != Step::Ai {
            self.ai_page = AiPage::Connect;
        }
        self.step = step;
        self.visited.insert(step);
        self.error = None;
        self.menus.close();
    }

    /// On to the next step, the current one no longer counted as skipped.
    fn forward(&mut self) {
        self.skipped.remove(&self.step);
        if let Some(next) = self.neighbour(true) {
            self.go(next);
        }
    }

    /// The point Cloud shows the address form: there is no Nextcloud account yet, or another one is being
    /// added.
    fn asks_address(&self, data: &Data) -> bool {
        self.adding || nextcloud_account(self, data).is_none()
    }

    /// A request of the mail path or a Nextcloud sign-in waits for the core or the browser.
    fn waiting(&self) -> bool {
        wizard::mail_waiting(&self.mail) || self.nextcloud.running()
    }

    /// The mail path is under way on the point Mail and calendar: something is typed and it has not ended.
    fn mail_running(&self) -> bool {
        !wizard::mail_untouched(&self.mail) && !wizard::mail_done(&self.mail)
    }
}

/// The Nextcloud account the point Cloud shows: the one signed in to in this setup, else the first one there
/// is.
fn nextcloud_account<'a>(state: &State, data: &'a Data) -> Option<&'a torro_ipc::AccountInfo> {
    let listed = || data.accounts.iter().filter(|account| account.kind == "nextcloud" && !account.id.is_empty());
    state.signed_in.as_deref().and_then(|id| listed().find(|account| account.id == id)).or_else(|| listed().next())
}

#[derive(Debug, Clone)]
pub enum Message {
    AiSetup(ai_setup::Message),
    /// A point of the stepper was clicked.
    Go(Step),
    /// The primary button or Enter: on to the next step, on the last one finish.
    Next,
    /// "Back" or Escape.
    Back,
    /// The step's way to skip it ("Skip", "I don't use one", "Skip AI", "Keep the default").
    Skip,
    /// "Set up later": put the setup off and close it.
    Later,
    Language(Language),
    /// "Open …" of an app on the summary (its window class).
    OpenApp(&'static str),
    /// "Open Settings" or "Connect an agent…" on the summary: finish and show that section.
    OpenSection(Section),
    /// The Nextcloud address changed.
    ServerChanged(String),
    /// "Open the page again" while the browser signs in.
    OpenLoginPage,
    /// The login link field is read-only: typing in it changes nothing.
    LoginLinkEdited,
    /// "Add another Nextcloud".
    AddNextcloud,
    /// "Cancel": stop waiting for the browser or adding another Nextcloud.
    Cancel,
    /// The mail path on the point Mail and calendar.
    Mail(wizard::Message),
    /// "Add another account" once the mail path has connected one.
    AddAnotherMail,
    /// "Take over accounts from TorroMail": the dialog at TorroMail's accounts.
    TakeOverTorroMail,
    /// The dialog over the setup.
    Dialog(wizard::Message),
    /// "Use suggestion" beside "Keep" when the suggestion differs from the models chosen.
    UseSuggestion,
    /// "Use" of the suggested model of one slot on the pages Chat and Sorting (it is downloaded first).
    UseSuggested(AiSlot),
    /// "Choose myself": the pages Chat and Sorting.
    ChooseMyself,
    /// A chat model on the page Chat, by account and model. Closes its dropdown.
    ChatModel(String, String),
    /// Enable ("on") or disable ("Rules only") model sorting of mail.
    SortMail(bool),
    /// "Use the chat model" on the page Sorting.
    SortByChat,
    /// "Add key" of Jev on the page Sorting: the key form of the AI page.
    ConnectJev,
    /// "Use" in the question before sorting turns on a model elsewhere (issue #424).
    UseCloud,
    /// "Cancel" in that question, a click next to it, or Escape: the choice stays as it was.
    CancelCloud,
    /// Choose a decision model for mail from the connected AI accounts. Closes its dropdown.
    SortModel(String, String),
    /// The trigger or the menu of a dropdown of the setup (issue #554).
    Menus(DropdownEvent<SetupMenu>),
    /// A choice on the point Appearance: the same as on Settings › Appearance, carried out by the shell.
    Suite(suite::Message),
    /// "Get ArtCraft" on the point Documents or the summary: its releases in the browser.
    GetApp(String),
}

/// What the setup wants the shell to do.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Stay in the setup and carry out the action.
    Stay(Action),
    /// Close the setup (it was finished or put off), carry out the action and show the overview.
    Close(Action),
    /// Close the setup, carry out the action and show the section.
    CloseTo(Action, Section),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Outcome {
    let sorting = mail_sorting_on(state, data);
    ai_setup::allow_sorting(&mut state.ai_setup, sorting);
    let outcome = handle(state, message, data);
    let sorting = mail_sorting_on(state, data);
    ai_setup::allow_sorting(&mut state.ai_setup, sorting);
    // The AI point looks for servers on this computer the first time it is shown.
    match outcome {
        Outcome::Stay(Action::None) if state.step == Step::Ai && !ai_setup::active(&state.ai_setup) => {
            Outcome::Stay(act(ai_setup::open(&mut state.ai_setup)))
        }
        outcome => outcome,
    }
}

/// A model is on its way after "Use suggestion": downloaded, turned on or put into its slot. A question
/// about mail that waits is not work; it stays on the page AI.
pub(super) fn ai_working(state: &State) -> bool {
    ai_setup::preparing(&state.ai_setup) && !ai_setup::awaiting_consent(&state.ai_setup)
}

/// Whether Enter on the page Connect belongs to the shared AI page: its connection form is open or a
/// connection runs. A question about mail or a model on its way does not take it.
fn ai_form_open(state: &State) -> bool {
    ai_setup::primary(&state.ai_setup).is_some()
        && !ai_setup::awaiting_consent(&state.ai_setup)
        && !ai_setup::preparing(&state.ai_setup)
}

fn handle(state: &mut State, message: Message, data: &Data) -> Outcome {
    let stay = Outcome::Stay;
    // A finished sign-in is noticed before the message counts.
    observe(state, data);
    match message {
        Message::AiSetup(message) => {
            let confirm = message == ai_setup::Message::Confirm;
            let action = act(ai_setup::update(&mut state.ai_setup, message, data));
            // "Use" in the question after "Use suggestion": on to Appearance as without the question.
            if confirm && state.step == Step::Ai && state.ai_page == AiPage::Connect && ai_working(state) {
                state.forward();
            }
            stay(action)
        }
        // While a request of the mail path or a Nextcloud sign-in waits, the point stays: answers do not say
        // which request they are for, so only the one that waits may send.
        Message::Go(_) if state.waiting() => stay(Action::None),
        Message::Go(step) => {
            state.go(step);
            stay(Action::None)
        }
        Message::Next => next(state, data),
        Message::Skip => {
            state.skipped.insert(state.step);
            if state.step == Step::Mail {
                state.mail = wizard::mail();
            }
            match state.neighbour(true) {
                Some(next) => state.go(next),
                None => return finish(data, None),
            }
            stay(Action::None)
        }
        Message::Back => {
            match state.step {
                Step::Mail if wizard::mail_back(&state.mail) => {
                    return stay(act(wizard::update(&mut state.mail, wizard::Message::Back, data)));
                }
                // "Back" in the AI page's form closes the form, not the page.
                Step::Ai if ai_form_open(state) => {
                    return stay(act(ai_setup::update(&mut state.ai_setup, ai_setup::Message::Back, data)));
                }
                Step::Ai if state.ai_page == AiPage::Sort => {
                    state.menus.close();
                    state.ai_page = AiPage::Chat;
                }
                Step::Ai if state.ai_page == AiPage::Chat => {
                    state.menus.close();
                    state.ai_page = AiPage::Connect;
                }
                _ => {
                    if let Some(before) = state.neighbour(false) {
                        state.go(before);
                    }
                }
            }
            stay(Action::None)
        }
        // Closing the setup would end a model's preparation before it is in its slot: it waits.
        Message::Later | Message::OpenSection(_) if ai_working(state) => stay(Action::None),
        Message::Later => Outcome::Close(set_setup(SetupState::Skipped)),
        Message::Language(language) => {
            state.touched = true;
            state.language = language;
            stay(set(SettingKey::Language, serde_json::to_value(language).unwrap_or_default()))
        }
        Message::OpenApp(app) => stay(Action::Send(Request::OpenApp { app: app.into(), target: None })),
        Message::OpenSection(section) => finish(data, Some(section)),
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
            } else {
                state.adding = false;
            }
            state.error = None;
            stay(Action::None)
        }
        Message::Mail(message) => stay(act(wizard::update(&mut state.mail, message, data))),
        Message::AddAnotherMail => {
            state.mail = wizard::mail();
            stay(Action::None)
        }
        Message::TakeOverTorroMail if state.waiting() => stay(Action::None),
        Message::TakeOverTorroMail => {
            let (dialog, action) = wizard::State::torromail();
            state.dialog = Some(dialog);
            state.error = None;
            stay(act(action))
        }
        Message::Dialog(wizard::Message::Close) => {
            state.dialog = None;
            stay(Action::None)
        }
        Message::Dialog(message) => match state.dialog.as_mut() {
            Some(dialog) => stay(act(wizard::update(dialog, message, data))),
            None => stay(Action::None),
        },
        Message::UseSuggestion => use_suggestion(state, data, true),
        Message::UseSuggested(slot) => {
            let replace = data.settings.ai_slots.get(slot).is_some();
            stay(act(ai_setup::use_suggestion(&mut state.ai_setup, data, replace, Some(slot))))
        }
        Message::ChooseMyself => {
            state.menus.close();
            state.ai_page = AiPage::Chat;
            stay(Action::None)
        }
        Message::ChatModel(engine, model) => {
            state.menus.close();
            state.chat = Some((engine.clone(), model.clone()));
            stay(Action::Send(Request::SetAiSlot {
                slot: AiSlot::Chat,
                engine: Some(engine),
                model: Some(model),
                effort: None,
            }))
        }
        Message::Menus(event) => match state.menus.update(event) {
            true => stay(Action::Focus(DROPDOWN_SEARCH.into())),
            false => stay(Action::None),
        },
        // Turning on what sorted last in the cloud asks first, as the mail settings do (issue #424).
        Message::SortMail(true) if !mail_sorting_on(state, data) && cloud_to_turn_on(state, data).is_some() => {
            state.asking_cloud = cloud_to_turn_on(state, data);
            let held = mail_sorting_in_effect(data);
            state.pending_sort = held.engine.map(|engine| (engine, held.model, true));
            state.pending_by_chat = false;
            stay(Action::None)
        }
        Message::SortMail(on) => {
            state.asking_cloud = None;
            state.pending_sort = None;
            if !on {
                state.confirmed_sort = None;
                state.sort_by_chat = false;
            }
            state.sort_mail = Some(on);
            stay(Action::None)
        }
        Message::SortByChat => {
            let Some((engine, model)) = chat_choice(state, data) else { return stay(Action::None) };
            state.asking_cloud = None;
            state.pending_sort = None;
            if needs_sort_consent(state, data, &engine, Some(&model)) {
                ask_sort_consent(state, data, engine, Some(model), true);
                state.pending_by_chat = true;
                return stay(Action::None);
            }
            state.sort_by_chat = true;
            state.sort_mail = Some(true);
            stay(Action::None)
        }
        Message::ConnectJev => {
            stay(act(ai_setup::update(&mut state.ai_setup, ai_setup::Message::Online(AiProvider::Jev), data)))
        }
        Message::UseCloud => {
            if state.asking_cloud.take().is_some()
                && let Some((engine, model, enabled)) = state.pending_sort.take()
            {
                match std::mem::take(&mut state.pending_by_chat) {
                    true => state.sort_by_chat = true,
                    false => {
                        state.sort_by_chat = false;
                        state.sort_model = model.clone().map(|model| (engine.clone(), model));
                    }
                }
                state.confirmed_sort = Some((engine, model));
                state.sort_mail = Some(enabled);
            }
            stay(Action::None)
        }
        Message::CancelCloud => {
            state.asking_cloud = None;
            state.pending_sort = None;
            state.pending_by_chat = false;
            stay(Action::None)
        }
        Message::SortModel(engine, model) => {
            state.menus.close();
            state.asking_cloud = None;
            state.pending_sort = None;
            state.pending_by_chat = false;
            state.confirmed_sort = None;
            if needs_sort_consent(state, data, &engine, Some(&model)) {
                ask_sort_consent(state, data, engine, Some(model), true);
                return stay(Action::None);
            }
            state.sort_by_chat = false;
            state.sort_model = Some((engine, model));
            state.sort_mail = Some(true);
            stay(Action::None)
        }
        // The shell carries these out as Settings › Appearance does.
        Message::Suite(_) => stay(Action::None),
        Message::GetApp(url) => stay(Action::OpenUrl(url)),
    }
}

/// The primary button or Enter on the current step.
fn next(state: &mut State, data: &Data) -> Outcome {
    let stay = Outcome::Stay;
    match state.step {
        // On the point Cloud the primary button signs in while it asks for an address.
        Step::Cloud if state.nextcloud.running() => stay(Action::None),
        Step::Cloud if state.asks_address(data) => {
            if !state.nextcloud.can_start() {
                return stay(Action::None);
            }
            state.error = None;
            stay(act(state.nextcloud.start(data)))
        }
        // A mail path under way takes the primary button: find the provider, the password, connect.
        Step::Mail if state.mail_running() => {
            stay(act(wizard::update(&mut state.mail, wizard::Message::Continue, data)))
        }
        // The shared AI page's connection form (also Jev's key on the page Sorting) takes the primary button.
        Step::Ai if ai_form_open(state) => {
            stay(act(ai_setup::update(&mut state.ai_setup, ai_setup::Message::Submit, data)))
        }
        Step::Ai => match state.ai_page {
            AiPage::Connect if ai_setup::awaiting_consent(&state.ai_setup) => stay(Action::None),
            // A slot chosen ("Keep") or a model already on its way: on without changing anything.
            AiPage::Connect if keeps(data) || ai_working(state) => {
                state.forward();
                stay(Action::None)
            }
            AiPage::Connect => use_suggestion(state, data, false),
            AiPage::Chat => {
                state.menus.close();
                state.ai_page = AiPage::Sort;
                stay(Action::None)
            }
            AiPage::Sort => sort_and_go_on(state, data),
        },
        Step::Done if ai_working(state) => stay(Action::None),
        Step::Done => finish(data, None),
        _ => {
            state.forward();
            stay(Action::None)
        }
    }
}

/// Whether the page Connect keeps the models chosen ("Keep"): a slot is chosen already.
fn keeps(data: &Data) -> bool {
    data.settings.ai_slots.chat.is_some() || data.settings.ai_slots.decide.is_some()
}

/// "Use suggestion": prepares the suggested models (with `replace` also over the models chosen) and goes on
/// to Appearance at once – the download goes on meanwhile. A model for Sorting elsewhere asks first; the
/// question stays on the page.
fn use_suggestion(state: &mut State, data: &Data, replace: bool) -> Outcome {
    if !ai_setup::can_use_suggestion(&state.ai_setup, data, replace, None) {
        return Outcome::Stay(Action::None);
    }
    let action = act(ai_setup::use_suggestion(&mut state.ai_setup, data, replace, None));
    if !ai_setup::awaiting_consent(&state.ai_setup) {
        state.forward();
    }
    Outcome::Stay(action)
}

/// "Continue" on the page Sorting: what it chose for sorting mail ([`mail_sorting_request`]), after the
/// question where a model elsewhere would read the mail; then on to Appearance.
fn sort_and_go_on(state: &mut State, data: &Data) -> Outcome {
    if state.asking_cloud.is_some() || ai_models_wait(data) {
        return Outcome::Stay(Action::None);
    }
    let assignment = mail_sorting_request(state, data);
    if let Some(request) = &assignment
        && let Some((engine, model)) = sorting_target(request, data)
        && needs_sort_consent(state, data, engine, model)
    {
        let enabled = !matches!(request, Request::SetAiAssignment { enabled: false, .. });
        let by_chat = state.sort_by_chat;
        ask_sort_consent(state, data, engine.to_string(), model.map(str::to_string), enabled);
        state.pending_by_chat = by_chat;
        return Outcome::Stay(Action::None);
    }
    state.forward();
    Outcome::Stay(assignment.map_or(Action::None, Action::Send))
}

/// "Finish" (or a way out of the summary into `section`): which apps are shown follows what is connected
/// ([`components`]), then the setup is recorded as done.
fn finish(data: &Data, section: Option<Section>) -> Outcome {
    let mut requests: Vec<Request> = components(data)
        .into_iter()
        .map(|switch| Request::SetSetting {
            key: SettingKey::Component,
            value: serde_json::to_value(switch).unwrap_or_default(),
        })
        .collect();
    requests.push(setup_request(SetupState::Done));
    let action = match requests.len() {
        1 => requests.pop().map_or(Action::None, Action::Send),
        _ => Action::SendAll(requests),
    };
    match section {
        Some(section) => Outcome::CloseTo(action, section),
        None => Outcome::Close(action),
    }
}

/// The switches "Finish" sends (issue #326): Mail with a mail account, Calendar with a collection that has
/// events, Tasks with a task list or GitHub, the assistant with an AI account; the Notification Hub and
/// Workspaces stay as they are. On the first run (`setup.version` 0) an app without its connection is
/// hidden; run again, an app is only shown, never hidden – it may have been hidden on purpose. Only what
/// changes is sent.
fn components(data: &Data) -> Vec<ComponentSwitch> {
    let first = first_run(&data.settings.setup);
    let listed = || data.accounts.iter().filter(|account| !account.id.is_empty());
    let collections = || listed().flat_map(|account| account.collections.iter());
    // A server that brings calendars or task lists counts before its first sync has listed them: a Nextcloud
    // whose services are not read yet or that has them, a CalDAV account, and a mail account whose sign-in
    // has a CalDAV address.
    let brings = |service: torro_ipc::ServerService| {
        listed().any(|account| match account.kind.as_str() {
            // A CalDAV account counts until its collections are listed; then they say what it has.
            "caldav" => account.collections.is_empty(),
            "mail" => account.collections.is_empty() && account.mail.as_ref().is_some_and(|mail| mail.caldav.is_some()),
            "nextcloud" => account.services.as_ref().is_none_or(|services| services.contains(&service)),
            _ => false,
        })
    };
    [
        (Component::Mail, listed().any(|account| account.kind == "mail")),
        (
            Component::Calendar,
            collections().any(|collection| collection.kind.has_events()) || brings(torro_ipc::ServerService::Calendars),
        ),
        (
            Component::Tasks,
            collections().any(|collection| collection.kind.has_todos())
                || brings(torro_ipc::ServerService::Tasks)
                || listed().any(|a| a.kind == "github"),
        ),
        (Component::Assistant, listed().any(|account| account.kind == "ai")),
    ]
    .into_iter()
    .filter_map(|(component, connected)| {
        let on = !data.settings.components_off.contains(&component);
        match (connected, on) {
            (true, false) => Some(ComponentSwitch { component, enabled: true }),
            (false, true) if first => Some(ComponentSwitch { component, enabled: false }),
            _ => None,
        }
    })
    .collect()
}

/// Whether this is surely the first run of the setup: it is pending and no run was ever finished or put off
/// (version 0). Putting it off records this build's version as finishing does, so a setup put off cannot tell
/// whether it ran before; then it only shows apps and never hides one – hiding is the step a person could
/// lose a choice by.
fn first_run(setup: &torro_core::settings::Setup) -> bool {
    setup.state == SetupState::Pending && setup.version == 0
}

/// Follows a Nextcloud sign-in through a reload of `data` (the shell calls it on every reload while the
/// setup is open): signed in, the point shows the account's services; failed, it asks for the address again
/// and says so. While waiting the footer shows the shell's error, if any.
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

/// The answer to a request the setup sent; `true` when the setup shows it itself (then the shell shows no
/// notice): the dialog's and the mail path's own, and a sign-in that could not start, in the footer.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> bool {
    if let Some(dialog) = state.dialog.as_mut() {
        return wizard::answered(dialog, result);
    }
    // The one that waits takes the answer: the mail path's search or sign-in, else a Nextcloud sign-in. While
    // either waits the point stays ([`State::waiting`]), so no other request of the setup is out.
    if wizard::mail_waiting(&state.mail) {
        return wizard::answered(&mut state.mail, result);
    }
    match state.nextcloud.answered(result) {
        Some(Err(message)) => {
            state.error = Some(message);
            true
        }
        Some(Ok(())) => true,
        None => state.mail_running() && wizard::answered(&mut state.mail, result),
    }
}

/// The request that changes one setting.
fn set(key: SettingKey, value: serde_json::Value) -> Action {
    Action::Send(Request::SetSetting { key, value })
}

/// The request that records where the setup stands.
fn setup_request(state: SetupState) -> Request {
    Request::SetSetting { key: SettingKey::Setup, value: serde_json::to_value(state).unwrap_or_default() }
}

fn set_setup(state: SetupState) -> Action {
    Action::Send(setup_request(state))
}

/// The chat model: as chosen on the page Chat, else the slot Chat as stored, if it names a model.
fn chat_choice(state: &State, data: &Data) -> Option<(String, String)> {
    state.chat.clone().or_else(|| {
        let held = data.settings.ai_slots.chat.as_ref()?;
        Some((held.engine.clone(), held.model.clone()?))
    })
}

/// The models with tools the connected AI accounts offer for chat, enabled ones only.
fn chat_models(data: &Data) -> Vec<(&String, &AiModel)> {
    data.ai_models
        .iter()
        .filter(|(_, model)| model.enabled && model.tools == Some(true))
        .map(|(account, model)| (account, model))
        .collect()
}

/// What sorts mail as the page Sorting chose it: the chat model with "Use the chat model", else the decision
/// model chosen or recommended.
fn sort_choice(state: &State, data: &Data) -> Option<(String, String)> {
    if state.sort_by_chat {
        return chat_choice(state, data);
    }
    selected_mail_decider(state, data).map(|(account, model)| (account.clone(), model.id.clone()))
}

/// Every enabled decision model the menu offers, including those that need consent. Defaults
/// are restricted separately in [`selected_mail_decider`], never by hiding a remote option.
fn mail_deciders(data: &Data) -> Vec<(&String, &AiModel)> {
    data.ai_models
        .iter()
        .filter(|(_, model)| model.enabled && model.decision == Some(true))
        .map(|(account, model)| (account, model))
        .collect()
}

/// The models of the AI accounts are not loaded yet for the accounts there are now: the pages Chat and
/// Sorting wait for them.
fn ai_models_wait(data: &Data) -> bool {
    data.accounts
        .iter()
        .filter(|account| account.kind == "ai")
        .map(|account| &account.id)
        .ne(data.ai_models_from.iter())
}

/// The exception stored for sorting mail (`ai.set_assignment inbox.categorize`), on or off; `None`
/// when none names a model.
fn stored_mail_exception(data: &Data) -> Option<&AiAssignment> {
    data.settings.ai_assignments.get(&AiFunction::InboxCategorize).filter(|held| held.engine.is_some())
}

/// What sorts mail now, as the core weighs it (issue #325) and the mail settings show it: the
/// exception while one is stored – one switched off keeps sorting off –, else the slot Sorting as an
/// assignment that is on; nothing assigned when neither is set.
fn mail_sorting_in_effect(data: &Data) -> AiAssignment {
    if let Some(own) = stored_mail_exception(data) {
        return own.clone();
    }
    match data.settings.ai_slots.get(AiSlot::Decide) {
        Some(slot) => AiAssignment {
            function: AiFunction::InboxCategorize,
            engine: Some(slot.engine.clone()),
            model: slot.model.clone(),
            effort: slot.effort,
            enabled: true,
        },
        None => AiAssignment::none(AiFunction::InboxCategorize),
    }
}

/// Whether the switch "Use model sorting" stands on: as the person set it here, else as it is now;
/// on while nothing sorts yet, which is the recommendation.
fn mail_sorting_on(state: &State, data: &Data) -> bool {
    let held = mail_sorting_in_effect(data);
    state.sort_mail.unwrap_or(held.engine.is_none() || held.enabled)
}

/// The wizard recommends the first eligible model unless one sorts already or one was chosen here.
fn selected_mail_decider<'a>(state: &State, data: &'a Data) -> Option<(&'a String, &'a AiModel)> {
    if let Some((engine, model)) = &state.sort_model {
        return data
            .ai_models
            .iter()
            .find(|(account, candidate)| {
                account == engine && candidate.id == *model && candidate.enabled && candidate.decision == Some(true)
            })
            .map(|(account, model)| (account, model));
    }
    let candidates: Vec<_> = mail_deciders(data)
        .into_iter()
        .filter(|(account, model)| {
            data.settings.mail_to_cloud_in(AiSlot::Decide)
                || sorting_place(data, account, Some(&model.id)) == crate::assistant::Place::Local
        })
        .collect();
    let held = mail_sorting_in_effect(data);
    if let Some(engine) = &held.engine {
        return candidates.into_iter().find(|(account, candidate)| {
            *account == engine && held.model.as_ref().is_none_or(|model| candidate.id == *model)
        });
    }
    candidates.into_iter().next()
}

/// What the switch "on" would turn on in the cloud without the person's consent (issue #424): the
/// exception switched off that "on" switches on again when the step offers no model of its own, while
/// Sorting's permission is off – if it computes elsewhere, as the data rule weighs it (`place_of`): a
/// listed model by where it runs, one the list lacks in doubt elsewhere, naming none ("Automatic") by
/// the account's seat. Its model's and provider's names for the question; `None` when nothing needs
/// asking, as for a model here the step does not offer (a chat model, one turned off).
fn cloud_to_turn_on(state: &State, data: &Data) -> Option<(String, String)> {
    if data.settings.mail_to_cloud_in(AiSlot::Decide) || sort_choice(state, data).is_some() {
        return None;
    }
    let held = stored_mail_exception(data).filter(|held| !held.enabled)?;
    let engine = held.engine.clone()?;
    let listed = held
        .model
        .as_deref()
        .and_then(|id| data.ai_models.iter().find(|(account, model)| *account == engine && model.id == id));
    let elsewhere = sorting_place(data, &engine, held.model.as_deref()) != crate::assistant::Place::Local;
    if !elsewhere {
        return None;
    }
    let account = data
        .accounts
        .iter()
        .find(|account| account.id == engine)
        .map_or_else(|| engine.clone(), |account| account.label.clone());
    let model = match (listed, &held.model) {
        (Some((_, model)), _) => model.name.clone(),
        (None, Some(id)) => id.clone(),
        (None, None) => account.clone(),
    };
    Some((model, account))
}

/// Resolve the same account address and model evidence as the daemon, never the provider's seat.
fn sorting_place(data: &Data, engine: &str, model: Option<&str>) -> crate::assistant::Place {
    use torro_core::ai::{Place, automatic_place_of, place_of};
    let provider = torro_settings_ui::accounts::ai_provider(engine);
    let server = data.accounts.iter().find(|account| account.id == engine).map_or("", |account| match &account.state {
        torro_pim::AccountState::Connected { server, .. } | torro_pim::AccountState::SigningIn { server, .. } => {
            server.as_str()
        }
        torro_pim::AccountState::NotConnected => "",
    });
    let Some(provider) = provider else { return Place::Remote };
    match model {
        None => automatic_place_of(provider, server),
        Some(id) => {
            let listed = data.ai_models.iter().find(|(account, model)| account == engine && model.id == id);
            let unknown = AiModel::named(id);
            place_of(provider, server, listed.map_or(&unknown, |(_, model)| model))
        }
    }
}

fn sorting_target<'a>(request: &'a Request, data: &Data) -> Option<(&'a str, Option<&'a str>)> {
    if let Request::SetAiAssignment { engine, model, enabled: false, .. } = request {
        let held = mail_sorting_in_effect(data);
        // Switching off the current target is not a new remote selection. Preparing a different
        // disabled target is: it could be switched on later with a saved permission.
        if engine == &held.engine && model == &held.model {
            return None;
        }
    }
    match request {
        Request::SetAiSlot { engine: Some(engine), model, .. }
        | Request::SetAiAssignment { engine: Some(engine), model, .. } => Some((engine, model.as_deref())),
        _ => None,
    }
}

fn needs_sort_consent(state: &State, data: &Data, engine: &str, model: Option<&str>) -> bool {
    if sorting_place(data, engine, model) == crate::assistant::Place::Local {
        return false;
    }
    let matches = |choice: &(String, Option<String>)| choice.0 == engine && choice.1.as_deref() == model;
    if state.confirmed_sort.as_ref().is_some_and(matches) {
        return false;
    }
    let held = mail_sorting_in_effect(data);
    !(data.settings.mail_to_cloud_in(AiSlot::Decide)
        && held.engine.as_deref() == Some(engine)
        && held.model.as_deref() == model)
}

fn ask_sort_consent(state: &mut State, data: &Data, engine: String, model: Option<String>, enabled: bool) {
    let account = data.accounts.iter().find(|account| account.id == engine);
    let name = model
        .as_deref()
        .and_then(|id| data.ai_models.iter().find(|(account, model)| *account == engine && model.id == id))
        .map(|(_, model)| model.name.clone())
        .or_else(|| model.clone())
        .unwrap_or_else(|| tr("Automatic").to_string());
    let provider = account.map_or_else(|| engine.clone(), |account| account.label.clone());
    state.asking_cloud = Some((name, provider));
    state.pending_sort = Some((engine, model, enabled));
}

/// What "Continue" on the page Sorting sends for sorting mail, as main's mail settings weigh it (issue
/// #325): one request, so none can go through while another is refused.
///
/// - On, with the model chosen or recommended: into the slot Sorting – or, while an exception is
///   stored, into that exception switched on, which stands before the slot anyway.
/// - Off: the exception switched off, with the model chosen here or what sorts now – also one the
///   step does not offer (a model elsewhere), so "off" is never lost.
/// - Nothing changed: nothing sent.
fn mail_sorting_request(state: &State, data: &Data) -> Option<Request> {
    let enabled = mail_sorting_on(state, data);
    let held = mail_sorting_in_effect(data);
    let chosen = sort_choice(state, data);
    let exception = |engine: String, model: Option<String>, enabled: bool| {
        let effort = if held.engine.as_deref() == Some(engine.as_str()) { held.effort } else { None };
        Request::SetAiAssignment { function: AiFunction::InboxCategorize, engine: Some(engine), model, effort, enabled }
    };
    if !enabled {
        let (engine, model) = match chosen {
            Some((engine, model)) => (engine, Some(model)),
            None => (held.engine.clone()?, held.model.clone()),
        };
        if !held.enabled && held.engine.as_deref() == Some(engine.as_str()) && held.model == model {
            return None;
        }
        return Some(exception(engine, model, false));
    }
    // What sorted before, switched on again, when the step does not offer that model (one elsewhere):
    // the exception it is stored as, on once more – "on" is never lost either.
    let Some((engine, model)) = chosen else {
        let engine = held.engine.clone().filter(|_| !held.enabled && stored_mail_exception(data).is_some())?;
        return Some(exception(engine, held.model.clone(), true));
    };
    if held.enabled
        && held.engine.as_deref() == Some(engine.as_str())
        && held.model.as_deref() == Some(model.as_str())
        && (data.settings.mail_to_cloud_in(AiSlot::Decide)
            || sorting_place(data, &engine, Some(&model)) == crate::assistant::Place::Local)
    {
        return None;
    }
    if stored_mail_exception(data).is_some() {
        return Some(exception(engine, Some(model), true));
    }
    let effort =
        data.settings.ai_slots.decide.as_ref().filter(|slot| slot.engine == engine).and_then(|slot| slot.effort);
    Some(Request::SetAiSlot { slot: AiSlot::Decide, engine: Some(engine), model: Some(model), effort })
}

/// Escape: close the open dropdown or dialog, the question about mail, the AI page's form, stop waiting for
/// the browser or adding another Nextcloud, walk back on the mail path or the AI pages, else one step back;
/// nothing on the welcome.
pub fn escape(state: &State) -> Option<Message> {
    if state.menus.open().is_some() {
        return Some(Message::Menus(DropdownEvent::Close));
    }
    if let Some(dialog) = &state.dialog {
        return Some(Message::Dialog(wizard::escape(dialog).unwrap_or(wizard::Message::Close)));
    }
    if state.asking_cloud.is_some() {
        return Some(Message::CancelCloud);
    }
    if state.step == Step::Ai
        && let Some(close) = ai_setup::escape(&state.ai_setup)
    {
        return Some(Message::AiSetup(close));
    }
    if state.step == Step::Cloud && (state.nextcloud.running() || state.adding) {
        return Some(Message::Cancel);
    }
    // The mail path under way at its first step is given up, as its "Cancel" does; not while it waits.
    if state.step == Step::Mail
        && state.mail_running()
        && !wizard::mail_back(&state.mail)
        && !wizard::mail_waiting(&state.mail)
    {
        return Some(Message::AddAnotherMail);
    }
    let walks_back = (state.step == Step::Mail && wizard::mail_back(&state.mail))
        || (state.step == Step::Ai && state.ai_page != AiPage::Connect);
    (walks_back || state.neighbour(false).is_some()).then_some(Message::Back)
}

// ----- The frame ------------------------------------------------------------------------------

/// The header: the logo with "Setup", the stepper of the five points and "Set up later"; narrow "‹", where
/// the walk stands with a progress bar, and "Later".
pub fn header(state: &State, class: SizeClass) -> Element<'_, Message> {
    // On the summary "Finish" is the way out; putting it off there would only confuse.
    let later_word = if class == SizeClass::Narrow { tr("Later") } else { tr("Set up later") };
    let later = (state.step != Step::Done && !ai_working(state))
        .then(|| widgets::text_button(None, later_word, style::button::ghost, Some(Message::Later)));
    if class == SizeClass::Narrow {
        let back =
            state.neighbour(false).map(|_| widgets::icon_button(Icon::ChevronLeft, tr("Back"), Some(Message::Back)));
        let at = state.step.number();
        let mut title = row![].spacing(space::s()).align_y(Alignment::Center);
        let count = match at {
            Some(at) => format!("{at}/{}", Step::POINTS.len()),
            None => String::new(),
        };
        if !count.is_empty() {
            title = title.push(typo::text(typo::meta().tabular(), count).style(theme::text_3));
        }
        title = title.push(typo::text(typo::body_strong(), state.step.label()).wrapping(Wrapping::None));
        if state.step == Step::Ai {
            let page = format!("· {}/3", state.ai_page.at());
            title = title.push(typo::text(typo::meta().tabular(), page).style(theme::text_3));
        }
        let done = match state.step {
            Step::Welcome => 0,
            Step::Done => Step::POINTS.len(),
            step => step.number().unwrap_or_default(),
        };
        title = title.push(widgets::progress_bar(done, Step::POINTS.len()));
        return shell::header_bar(back, container(title).width(Fill).clip(true), later);
    }
    let logo = shell::app_mark(AppId::WORKPLACE);
    let lead: Element<'_, Message> = match class {
        SizeClass::Wide => row![logo, typo::text(typo::body_strong(), tr("Setup")).wrapping(Wrapping::None)]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into(),
        _ => logo,
    };
    // The stepper takes the room between the logo and "Set up later", centred in it.
    let stepper = container(stepper(state, class == SizeClass::Wide)).width(Fill).center_x(Fill).clip(true);
    row![lead, stepper].push(later).spacing(space::l()).align_y(Alignment::Center).into()
}

/// The five points side by side, joined by a line: each a button with its number (a check mark once
/// passed) and, with `names`, its name; without them the name is the tooltip. The current point in `text`
/// and medium weight, the others in `text_2`; never red. On the point AI three small bars after its name
/// say which of its pages is open.
fn stepper(state: &State, names: bool) -> Element<'_, Message> {
    let mut points = row![].spacing(space::s()).align_y(Alignment::Center);
    for (index, point) in Step::POINTS.into_iter().enumerate() {
        if index > 0 {
            let line = container(rule::horizontal(1).style(style::rule)).width(space::l()).center_y(size::control_sm());
            points = points.push(line);
        }
        let current = point == state.step;
        let face = match state.passed(point) {
            true => TileFace::Icon(Icon::Check),
            false => TileFace::letter(&(index + 1).to_string()),
        };
        let mut content =
            row![widgets::source_tile(face, TileSize::Small, None)].spacing(space::s()).align_y(Alignment::Center);
        if names {
            let name = match current {
                true => typo::text(typo::body_strong(), point.label()).style(theme::text),
                false => typo::text(typo::body(), point.label()).style(theme::text_2),
            };
            content = content.push(name.wrapping(Wrapping::None));
        }
        if current && point == Step::Ai {
            content = content.push(widgets::progress_bar(state.ai_page.at(), 3));
        }
        // The content centred in a control's height, so number, name and the joining lines share one axis.
        let press = button(container(content).center_y(size::control_sm()))
            .padding(style::button_padding())
            .style(style::button::ghost)
            .on_press(Message::Go(point));
        points = points.push(match names {
            true => Element::from(press),
            false => widgets::with_tooltip(press, point.label()),
        });
    }
    points.into()
}

/// The dialog over the whole window, while it is open; else the question before sorting mail elsewhere
/// (issue #424); else the open dropdown (issue #554), of the setup, of the AI page or of Appearance.
pub fn overlay<'a>(state: &'a State, data: &'a Data, looks: &'a suite::State) -> Option<Element<'a, Message>> {
    if let Some(dialog) = state.dialog.as_ref() {
        let content = wizard::view(dialog, data).map(Message::Dialog);
        let layer = super::dialog(content, size::form_max(), Message::Dialog(wizard::Message::Close));
        return Some(match wizard::overlay(dialog) {
            Some(menu) => iced::widget::stack![layer, menu.map(Message::Dialog)].into(),
            None => layer,
        });
    }
    if let Some((model, provider)) = &state.asking_cloud {
        return Some(torro_settings_ui::parts::cloud_consent(model, provider, Message::UseCloud, Message::CancelCloud));
    }
    if let Some(menu) = pages::menu(state, data) {
        return Some(menu);
    }
    match state.step {
        Step::Ai => ai_setup::overlay(&state.ai_setup).map(|menu| menu.map(Message::AiSetup)),
        Step::Mail => wizard::overlay(&state.mail).map(|menu| menu.map(Message::Mail)),
        Step::Appearance => suite::overlay(looks, &data.settings).map(|layer| layer.map(Message::Suite)),
        _ => None,
    }
}

/// The page of the current step over the fixed footer. `looks` are the designs, fonts and colour schemes
/// the point Appearance offers (the control centre's, listed for Settings › Appearance).
pub fn body<'a>(state: &'a State, data: &'a Data, looks: &'a suite::State, class: SizeClass) -> Element<'a, Message> {
    let page = pages::view(state, data, looks);
    let wide = state.step == Step::Appearance;
    let inner = container(page).width(Fill);
    let inner = if wide { inner } else { inner.max_width(size::form_max()) };
    let page = container(inner).width(Fill).padding(page_margin(class));
    let page = scrollable(page)
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .height(Fill);
    column![page, rule::horizontal(1).style(style::rule), footer(state, data, class)].into()
}

/// The footer: the step's status, a problem or a link at the left; "Back", the way to skip the step, a
/// second way where the step has one, and the primary button. Narrow they stand one under the other, the
/// primary button first.
fn footer<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    if state.step == Step::Ai && ai_setup::awaiting_consent(&state.ai_setup) {
        return container(typo::text(typo::meta(), tr("Mail needs your explicit permission.")).style(theme::text_2))
            .padding(layout::row_padding())
            .into();
    }
    let status: Option<Element<'a, Message>> = match &state.error {
        Some(error) => Some(problem(error.clone())),
        None => pages::status(state, data).map(|text| typo::text(typo::body(), text).style(theme::text_2).into()),
    };
    let mut buttons: Vec<Element<'a, Message>> = Vec::new();
    let cloud_actions = match state.step == Step::Ai && state.ai_page == AiPage::Connect {
        true => ai_setup::cloud_form_actions(&state.ai_setup),
        false => None,
    };
    let narrow = class == SizeClass::Narrow;
    if let Some(actions) = cloud_actions {
        buttons.push(actions.map(Message::AiSetup));
    } else {
        // Narrow, the header has "Back"; within the mail path and the AI pages it stays here.
        let walks_back = (state.step == Step::Mail && wizard::mail_back(&state.mail))
            || (state.step == Step::Ai && state.ai_page != AiPage::Connect);
        if (walks_back || (!narrow && state.neighbour(false).is_some())) && state.step != Step::Welcome {
            buttons.push(widgets::text_button(None, tr("Back"), style::button::ghost, Some(Message::Back)));
        }
        if let Some((label, message)) = pages::skip(state, data) {
            buttons.push(widgets::text_button(None, label, style::button::secondary, Some(message)));
        }
        for (label, message) in pages::second(state, data) {
            buttons.push(widgets::text_button(None, label, style::button::secondary, message));
        }
        let (label, enabled, busy) = pages::primary(state, data);
        buttons.push(match busy {
            true => widgets::busy_button(label, style::button::primary),
            false => widgets::text_button(None, label, style::button::primary, enabled.then_some(Message::Next)),
        });
    }
    let link = pages::link(state);
    let left = container(column![].push(status).push(link).spacing(space::xs())).width(Fill).clip(true);
    if narrow {
        buttons.reverse();
        let stacked = column(buttons.into_iter().map(|button| container(button).width(Fill).center_x(Fill).into()))
            .spacing(space::s());
        return container(column![left, stacked].spacing(space::s()))
            .width(Fill)
            .padding(layout::panel_padding())
            .into();
    }
    container(row![left, row(buttons).spacing(space::s()).align_y(Alignment::Center)].spacing(space::l()))
        .width(Fill)
        .center_y(size::header() + space::m())
        .padding(layout::row_padding())
        .into()
}

/// A problem in the footer: an alert icon in the error colour, the text in `text` – the colour is only in
/// the icon.
fn problem<'a>(text: String) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).error_text) });
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

    /// The setup on the page Sorting of the point AI.
    fn sort_state() -> State {
        state(Step::Ai).on_ai_page(AiPage::Sort)
    }

    /// The demo's data: a connected Nextcloud among other accounts, the models of its AI accounts loaded.
    fn data() -> Data {
        let mut data = super::super::launch::demo_data();
        data.ai_models_from =
            data.accounts.iter().filter(|account| account.kind == "ai").map(|account| account.id.clone()).collect();
        data
    }

    /// The demo's data without models chosen for the slots.
    fn unchosen() -> Data {
        let mut data = data();
        data.settings.ai_slots = Default::default();
        data.settings.ai_assignments.clear();
        data
    }

    fn sent(outcome: &Outcome) -> Option<(SettingKey, serde_json::Value)> {
        match outcome {
            Outcome::Stay(Action::Send(Request::SetSetting { key, value }))
            | Outcome::Close(Action::Send(Request::SetSetting { key, value })) => Some((*key, value.clone())),
            _ => None,
        }
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

    /// The requests of a closing outcome.
    fn closing(outcome: &Outcome) -> Vec<Request> {
        match outcome {
            Outcome::Close(Action::Send(request)) | Outcome::CloseTo(Action::Send(request), _) => vec![request.clone()],
            Outcome::Close(Action::SendAll(all)) | Outcome::CloseTo(Action::SendAll(all), _) => all.clone(),
            other => panic!("expected a closing outcome, got {other:?}"),
        }
    }

    #[test]
    fn the_old_words_of_the_steps_lead_to_their_point() {
        for step in Step::ALL {
            assert_eq!(Step::from_arg(step.arg()), Some(step));
        }
        assert_eq!(Step::from_arg("nextcloud"), Some(Step::Cloud));
        for word in ["calendar", "tasks", "inbox"] {
            assert_eq!(Step::from_arg(word), Some(Step::Mail), "{word}");
        }
        assert_eq!(Step::from_arg("components"), Some(Step::Welcome));
        assert_eq!(Step::from_arg("web"), Some(Step::Ai));
        assert_eq!(Step::from_arg("later"), None);
        assert_eq!(Step::POINTS.map(|step| step.number()), [Some(1), Some(2), Some(3), Some(4), Some(5)]);
        assert_eq!((Step::Welcome.number(), Step::Done.number()), (None, None));
    }

    /// The walk: the welcome, the five points and the summary; "Keep" on the point AI goes on with the
    /// models the demo has chosen, "Finish" records the setup as done.
    #[test]
    fn the_walk_goes_through_the_five_points_and_ends_done() {
        let data = data();
        let mut setup = state(Step::Welcome);
        let walk = [Step::Mail, Step::Cloud, Step::Ai, Step::Appearance, Step::Documents, Step::Done];
        for step in walk {
            update(&mut setup, Message::Next, &data);
            assert_eq!(setup.step, step);
        }
        assert!(setup.passed(Step::Mail) && setup.passed(Step::Ai) && !setup.passed(Step::Done));
        let finished = update(&mut setup, Message::Next, &data);
        let sent = closing(&finished);
        let done = setup_request(SetupState::Done);
        assert_eq!(sent.last(), Some(&done), "{sent:?}");
    }

    #[test]
    fn later_puts_it_off_and_back_stops_at_the_welcome() {
        let mut setup = state(Step::Welcome);
        assert!(escape(&setup).is_none(), "nothing before the welcome");
        update(&mut setup, Message::Back, &data());
        assert_eq!(setup.step, Step::Welcome);
        let later = update(&mut setup, Message::Later, &data());
        assert!(matches!(later, Outcome::Close(_)));
        assert_eq!(sent(&later), Some((SettingKey::Setup, serde_json::json!("skipped"))));
    }

    #[test]
    fn a_point_is_clicked_and_a_skipped_one_is_not_passed() {
        let data = data();
        let mut setup = state(Step::Mail);
        update(&mut setup, Message::Skip, &data);
        assert_eq!(setup.step, Step::Cloud);
        update(&mut setup, Message::Go(Step::Documents), &data);
        assert_eq!(setup.step, Step::Documents);
        assert!(!setup.passed(Step::Mail), "skipped");
        assert!(setup.passed(Step::Cloud), "reached beyond it");
        update(&mut setup, Message::Go(Step::Mail), &data);
        assert_eq!(setup.step, Step::Mail);
    }

    #[test]
    fn the_language_is_sent_as_the_settings_take_it() {
        let mut setup = state(Step::Welcome);
        let german = update(&mut setup, Message::Language(Language::German), &data());
        assert_eq!(sent(&german), Some((SettingKey::Language, serde_json::json!("german"))));
        setup.loaded(&Settings::default());
        assert_eq!(setup.language, Language::German, "a reload before the save keeps the choice");
    }

    /// Issue #326: the mail path stands on the page itself. Typed, the primary button searches the provider
    /// – the same request as Accounts › Add – and the setup stays; untouched, it goes on.
    #[test]
    fn the_mail_path_runs_inline_and_continue_goes_on_without_it() {
        let data = data();
        let mut setup = state(Step::Mail);
        assert_eq!(pages::primary(&setup, &data).0, tr("Continue"));
        let _ = update(&mut setup, Message::Mail(wizard::Message::AddressChanged("anna@mailbox.org".into())), &data);
        assert!(setup.mail_running());
        assert_eq!(pages::primary(&setup, &data), (tr("Find provider"), true, false));
        let search = update(&mut setup, Message::Next, &data);
        assert!(!matches!(search, Outcome::Stay(Action::None)), "{search:?}");
        assert_eq!(setup.step, Step::Mail);
        let _ = update(&mut setup, Message::AddAnotherMail, &data);
        assert!(!setup.mail_running(), "Cancel starts the path afresh");
        update(&mut setup, Message::Next, &data);
        assert_eq!(setup.step, Step::Cloud);
        assert!(pages::link(&state(Step::Mail)).is_some(), "TorroMail's accounts in the footer");
    }

    /// Issue #326: "Use suggestion" prepares the suggested models for both slots and goes on to Appearance at
    /// once; the work goes on meanwhile.
    #[test]
    fn use_suggestion_prepares_both_slots_and_goes_on_to_appearance() {
        let data = unchosen();
        let mut setup = state(Step::Ai);
        ai_setup::demo(&mut setup.ai_setup, "suggestion", &data);
        assert_eq!(pages::primary(&setup, &data).0, tr("Use suggestion"));
        assert!(ai_setup::can_use_suggestion(&setup.ai_setup, &data, false, None), "the demo suggests models");
        let used = update(&mut setup, Message::Next, &data);
        assert!(matches!(used, Outcome::Stay(Action::AskAi(_))), "{used:?}");
        assert_eq!(setup.step, Step::Appearance, "straight on to Appearance");
        assert!(ai_setup::preparing(&setup.ai_setup) && setup.ai_running(), "the models are on their way");
    }

    /// Chosen models are kept: "Keep" goes on and sends nothing; "Choose myself" leads through Chat and
    /// Sorting, "Back" walks back through them.
    #[test]
    fn keep_goes_on_and_choose_myself_walks_the_pages() {
        let data = data();
        assert!(data.settings.ai_slots.chat.is_some() || data.settings.ai_slots.decide.is_some(), "the demo chose");
        let mut setup = state(Step::Ai);
        assert_eq!(pages::primary(&setup, &data).0, tr("Keep my choice"));
        let kept = update(&mut setup, Message::Next, &data);
        assert!(matches!(kept, Outcome::Stay(Action::None)), "{kept:?}");
        assert_eq!(setup.step, Step::Appearance);
        let mut setup = state(Step::Ai);
        update(&mut setup, Message::ChooseMyself, &data);
        assert_eq!(setup.ai_page, AiPage::Chat);
        update(&mut setup, Message::Next, &data);
        assert_eq!(setup.ai_page, AiPage::Sort);
        assert!(matches!(escape(&setup), Some(Message::Back)));
        update(&mut setup, Message::Back, &data);
        assert_eq!((setup.step, setup.ai_page), (Step::Ai, AiPage::Chat));
    }

    /// The page Chat puts the model chosen into the slot Chat at once.
    #[test]
    fn a_chat_model_goes_into_the_slot_chat() {
        let mut data = data();
        data.ai_models = super::super::launch::demo_ai_models();
        let mut setup = state(Step::Ai).on_ai_page(AiPage::Chat);
        let opened = update(&mut setup, Message::Menus(DropdownEvent::Toggle(SetupMenu::Chat)), &data);
        assert!(matches!(opened, Outcome::Stay(Action::Focus(_))), "{opened:?}");
        assert!(pages::menu(&setup, &data).is_some());
        let engine = torro_settings_ui::accounts::demo::DEMO_OLLAMA.to_string();
        let sent = requests(update(&mut setup, Message::ChatModel(engine.clone(), "llama3.1:8b".into()), &data));
        assert!(
            matches!(&sent[..], [Request::SetAiSlot { slot: AiSlot::Chat, engine: Some(e), model: Some(m), .. }] if *e == engine && m == "llama3.1:8b"),
            "{sent:?}"
        );
        assert_eq!(setup.menus.open(), None, "a choice closes the dropdown");
    }

    /// "Use the chat model" sorts with the chat model; "Rules only" switches sorting off.
    #[test]
    fn sorting_can_use_the_chat_model_or_rules_only() {
        let mut data = unchosen();
        data.ai_models = super::super::launch::demo_ai_models();
        let engine = torro_settings_ui::accounts::demo::DEMO_OLLAMA.to_string();
        let mut setup = sort_state();
        update(&mut setup, Message::ChatModel(engine.clone(), "llama3.1:8b".into()), &data);
        update(&mut setup, Message::SortByChat, &data);
        // The demo's Ollama is not among its accounts, so it counts as elsewhere: the question first.
        assert!(setup.asking_cloud.is_some() && !setup.sort_by_chat);
        update(&mut setup, Message::UseCloud, &data);
        assert!(setup.sort_by_chat);
        let sent = requests(update(&mut setup, Message::Next, &data));
        assert!(
            matches!(&sent[..], [Request::SetAiSlot { slot: AiSlot::Decide, model: Some(m), .. }] if m == "llama3.1:8b"),
            "{sent:?}"
        );
        assert_eq!(setup.step, Step::Appearance);
        let mut rules = sort_state();
        update(&mut rules, Message::SortMail(false), &data);
        assert!(!mail_sorting_on(&rules, &data));
    }

    /// Jev's key form on the page Sorting takes the primary button and "Back" like the page Connect does.
    #[test]
    fn the_key_form_of_jev_takes_enter_and_back_on_the_page_sorting() {
        let data = unchosen();
        let mut setup = sort_state();
        update(&mut setup, Message::ConnectJev, &data);
        assert!(ai_form_open(&setup), "the key form is open");
        assert_eq!(pages::primary(&setup, &data).0, tr("Connect"));
        update(&mut setup, Message::Back, &data);
        assert!(!ai_form_open(&setup) && setup.ai_page == AiPage::Sort, "Back closes the form, the page stays");
        update(&mut setup, Message::ConnectJev, &data);
        let submitted = update(&mut setup, Message::Next, &data);
        assert!(matches!(submitted, Outcome::Stay(Action::AskAi(_))), "Enter connects: {submitted:?}");
        assert_eq!((setup.step, setup.ai_page), (Step::Ai, AiPage::Sort));
    }

    /// Issue #326: on the first run "Finish" hides the apps without their connection and shows those with
    /// one; run again it only shows, never hides.
    #[test]
    fn finish_derives_the_apps_from_the_connections() {
        let mut data = data();
        data.accounts.retain(|account| account.kind != "mail" && account.kind != "ai");
        data.settings.components_off = BTreeSet::from([Component::Calendar]);
        data.settings.setup.version = 0;
        let switches = components(&data);
        let has_events = data.accounts.iter().flat_map(|a| &a.collections).any(|c| c.kind.has_events())
            || data.accounts.iter().any(|a| matches!(a.kind.as_str(), "nextcloud" | "caldav"));
        assert!(switches.contains(&ComponentSwitch { component: Component::Mail, enabled: false }), "{switches:?}");
        assert!(switches.contains(&ComponentSwitch { component: Component::Assistant, enabled: false }));
        assert_eq!(
            switches.contains(&ComponentSwitch { component: Component::Calendar, enabled: true }),
            has_events,
            "a calendar shows the app Calendar"
        );
        assert!(switches.iter().all(|switch| !matches!(switch.component, Component::Inbox | Component::Workspaces)));
        data.settings.setup = torro_core::settings::Setup { state: SetupState::Pending, version: 3 };
        assert!(components(&data).iter().all(|switch| switch.enabled), "run again: only shown");
        // Put off, it cannot tell whether it ran before: it only shows.
        data.settings.setup = torro_core::settings::Setup { state: SetupState::Skipped, version: 4 };
        assert!(components(&data).iter().all(|switch| switch.enabled));
        data.settings.setup = torro_core::settings::Setup::default();
        // A Nextcloud whose services are not read yet brings its calendars and lists already.
        let mut fresh = data.clone();
        fresh.accounts.retain(|account| account.kind != "nextcloud");
        if let Some(mut cloud) = super::super::launch::demo_data().accounts.into_iter().find(|a| a.kind == "nextcloud")
        {
            cloud.collections.clear();
            cloud.services = None;
            fresh.accounts.push(cloud);
            let switches = components(&fresh);
            assert!(!switches.contains(&ComponentSwitch { component: Component::Calendar, enabled: false }));
            assert!(!switches.contains(&ComponentSwitch { component: Component::Tasks, enabled: false }));
        }
        // A mail account with a CalDAV address brings calendars and lists before its first sync lists them; one
        // without it brings none.
        let mut only_mail = data.clone();
        only_mail.accounts =
            super::super::launch::demo_data().accounts.into_iter().filter(|account| account.kind == "mail").collect();
        only_mail.settings.components_off.clear();
        let calendar_off = ComponentSwitch { component: Component::Calendar, enabled: false };
        let tasks_off = ComponentSwitch { component: Component::Tasks, enabled: false };
        if let Some(account) = only_mail.accounts.iter_mut().find(|account| account.mail.is_some()) {
            account.collections.clear();
            if let Some(mail) = account.mail.as_mut() {
                mail.caldav = Some("https://dav.mailbox.org/".into());
            }
            let switches = components(&only_mail);
            assert!(!switches.contains(&calendar_off) && !switches.contains(&tasks_off), "{switches:?}");
            only_mail.accounts.iter_mut().filter_map(|a| a.mail.as_mut()).for_each(|mail| mail.caldav = None);
            only_mail.accounts.iter_mut().for_each(|a| a.collections.clear());
            let switches = components(&only_mail);
            assert!(switches.contains(&calendar_off) && switches.contains(&tasks_off), "{switches:?}");
        } else {
            panic!("the demo has a mail account");
        }
        let finished = finish(&data, Some(Section::Mcp));
        assert!(matches!(finished, Outcome::CloseTo(_, Section::Mcp)), "Connect an agent… finishes too");
    }

    /// While a model is on its way after "Use suggestion", the setup waits before it closes: finishing would end
    /// the preparation before the model is in its slot. A question about mail is no work: nothing waits for it.
    #[test]
    fn finishing_waits_while_a_model_is_prepared() {
        let data = unchosen();
        let mut setup = state(Step::Ai);
        ai_setup::demo(&mut setup.ai_setup, "suggestion", &data);
        update(&mut setup, Message::Next, &data);
        assert!(ai_working(&setup));
        update(&mut setup, Message::Go(Step::Done), &data);
        assert_eq!(pages::primary(&setup, &data), (tr("Preparing model…"), false, true));
        assert!(matches!(update(&mut setup, Message::Next, &data), Outcome::Stay(Action::None)));
        assert!(matches!(update(&mut setup, Message::Later, &data), Outcome::Stay(Action::None)));
        let mut asking = state(Step::Ai);
        ai_setup::demo(&mut asking.ai_setup, "cloud-consent", &data);
        assert!(!ai_working(&asking), "a question is no work");
    }

    /// The mail path takes the answer to its own request also while another point is shown.
    #[test]
    fn the_mail_path_takes_its_answer_on_another_point() {
        let data = data();
        let mut setup = state(Step::Mail);
        update(&mut setup, Message::Mail(wizard::Message::AddressChanged("anna@mailbox.org".into())), &data);
        update(&mut setup, Message::Next, &data);
        update(&mut setup, Message::Go(Step::Cloud), &data);
        assert_eq!(setup.step, Step::Mail, "the point stays while the search waits");
        let refused: Result<Response, String> = Err("offline".into());
        assert!(answered(&mut setup, &refused), "the mail path shows its refusal itself");
    }

    /// Issue #554: the setup's choices are dropdowns: opening one asks for the search field's focus and shows
    /// it over the page, Escape closes it before it goes back, a choice closes it.
    #[test]
    fn the_dropdowns_of_the_setup_open_and_close() {
        let mut data = data();
        data.ai_models = super::super::launch::demo_ai_models();
        let mut setup = sort_state();
        let _ = update(&mut setup, Message::Menus(DropdownEvent::Toggle(SetupMenu::SortModel)), &data);
        assert_eq!(setup.menus.open(), Some(&SetupMenu::SortModel));
        assert!(pages::menu(&setup, &data).is_some());
        assert!(matches!(escape(&setup), Some(Message::Menus(DropdownEvent::Close))), "Escape closes the menu first");
        let _ = update(&mut setup, Message::Menus(DropdownEvent::Close), &data);
        assert!(pages::menu(&setup, &data).is_none());
    }

    /// The demo's data without its Nextcloud accounts.
    fn without_nextcloud() -> Data {
        let mut data = data();
        data.accounts.retain(|account| account.kind != "nextcloud");
        data
    }

    /// The point Cloud asks for the address without a Nextcloud; "I don't use one" skips it.
    #[test]
    fn without_a_nextcloud_the_cloud_asks_for_the_address() {
        let data = without_nextcloud();
        let mut setup = state(Step::Cloud);
        assert!(setup.asks_address(&data));
        assert_eq!(pages::primary(&setup, &data), (tr("Connect"), false, false), "no address yet");
        assert!(matches!(pages::skip(&setup, &data), Some((_, Message::Skip))));
        update(&mut setup, Message::ServerChanged("cloud.example.org".into()), &data);
        assert!(pages::primary(&setup, &data).1);
        update(&mut setup, Message::Skip, &data);
        assert_eq!(setup.step, Step::Ai);
    }

    /// The shared AI page's connection form takes Enter and the primary button; a question about mail does
    /// not let Enter pass it.
    #[test]
    fn ollama_cloud_forms_and_shared_consent_cannot_be_skipped_with_enter() {
        let data = unchosen();
        let mut setup = state(Step::Ai);
        ai_setup::demo(&mut setup.ai_setup, "cloud-form", &data);
        assert_eq!(pages::primary(&setup, &data), (tr("Connect"), true, false));
        assert!(matches!(update(&mut setup, Message::Next, &data), Outcome::Stay(Action::Focus(_))));
        assert_eq!(setup.step, Step::Ai);
        ai_setup::demo(&mut setup.ai_setup, "cloud-running", &data);
        assert_eq!(pages::primary(&setup, &data), (tr("Connecting…"), false, true));
        assert!(matches!(update(&mut setup, Message::Next, &data), Outcome::Stay(Action::None)));
        assert_eq!(setup.step, Step::Ai);
        ai_setup::demo(&mut setup.ai_setup, "cloud-consent", &data);
        assert!(matches!(update(&mut setup, Message::Next, &data), Outcome::Stay(Action::None)));
        assert_eq!(setup.step, Step::Ai);
    }

    #[test]
    fn a_sorting_opt_out_also_applies_to_the_shared_suggestion_flow() {
        use torro_settings_ui::accounts::ai_setup;
        let mut data = data();
        data.settings.ai_slots.decide = None;
        data.settings.ai_assignments.remove(&AiFunction::InboxCategorize);
        let mut setup = sort_state();
        ai_setup::demo(&mut setup.ai_setup, "consent", &data);
        update(&mut setup, Message::AiSetup(ai_setup::Message::Cancel), &data);
        update(&mut setup, Message::SortMail(false), &data);
        assert!(!mail_sorting_on(&setup, &data));
        assert!(matches!(
            update(&mut setup, Message::AiSetup(ai_setup::Message::Use), &data),
            Outcome::Stay(Action::None)
        ));
        assert!(
            matches!(
                update(&mut setup, Message::AiSetup(ai_setup::Message::Confirm), &data),
                Outcome::Stay(Action::None)
            ),
            "sorting stays opted out, even if a stale confirmation message arrives"
        );
    }

    /// The AI step recommends a local model for sorting mail and stores it as main's mail settings
    /// do (issue #325, #397), one request each: the model chosen or recommended goes into the slot
    /// Sorting, or into an exception stored, switched on; "off" is the exception switched off, also for
    /// a model the step does not offer, and "on" switches such an exception on again; nothing changed
    /// sends nothing.
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
        data.settings.ai_slots.decide = None;
        data.settings.ai_assignments.remove(&AiFunction::InboxCategorize);
        data.settings.decide_mail_to_cloud = Some(false);
        let mut local_account =
            data.accounts.iter().find(|account| account.kind == "ai").expect("demo AI account").clone();
        local_account.id = local.clone();
        local_account.label = "Ollama".into();
        local_account.state = torro_pim::AccountState::Connected {
            server: "http://localhost:11434/v1".into(),
            login_name: String::new(),
            last_sync: None,
            problem: None,
        };
        data.accounts.push(local_account);
        data.ai_models_from =
            data.accounts.iter().filter(|account| account.kind == "ai").map(|account| account.id.clone()).collect();
        data.ai_models = vec![
            (cloud.clone(), model("cloud", true, true)),
            (local.clone(), model("off", false, false)),
            (local.clone(), model("nimble", false, true)),
            (local.clone(), model("another", false, true)),
        ];
        let mut waiting = data.clone();
        waiting.ai_models_from.clear();
        let mut not_ready = sort_state();
        assert!(!pages::primary(&not_ready, &waiting).1, "continue waits until model choices are loaded");
        assert!(matches!(update(&mut not_ready, Message::Next, &waiting), Outcome::Stay(Action::AskAi(_))));
        assert_eq!(not_ready.step, Step::Ai);
        assert_eq!(mail_deciders(&data).len(), 3, "remote models are selectable; disabled models are not");
        let mut setup = sort_state();
        let Outcome::Stay(Action::Send(Request::SetAiSlot { slot, engine, model, .. })) =
            update(&mut setup, Message::Next, &data)
        else {
            panic!("the recommended model goes into the slot Sorting when continuing");
        };
        assert_eq!((slot, engine.as_deref(), model.as_deref()), (AiSlot::Decide, Some(local.as_str()), Some("nimble")));
        assert_eq!(setup.step, Step::Appearance);

        let mut chosen = sort_state();
        update(&mut chosen, Message::SortModel(local.clone(), "another".into()), &data);
        update(&mut chosen, Message::SortMail(false), &data);
        let Outcome::Stay(Action::Send(Request::SetAiAssignment { engine, model, enabled, .. })) =
            update(&mut chosen, Message::Next, &data)
        else {
            panic!("the explicit off choice is the exception switched off");
        };
        assert_eq!((engine.as_deref(), model.as_deref(), enabled), (Some(local.as_str()), Some("another"), false));

        // What sorts already, chosen again: nothing to send.
        let mut slotted = data.clone();
        slotted.settings.ai_slots.decide =
            Some(torro_core::ai::SlotModel { engine: local.clone(), model: Some("another".into()), effort: None });
        let mut again = sort_state();
        assert!(matches!(update(&mut again, Message::Next, &slotted), Outcome::Stay(Action::None)));

        let off = AiAssignment {
            function: AiFunction::InboxCategorize,
            engine: Some(local.clone()),
            model: Some("another".into()),
            effort: None,
            enabled: false,
        };
        data.settings.ai_assignments.insert(AiFunction::InboxCategorize, off);
        let mut reopened = sort_state();
        assert!(!mail_sorting_on(&reopened, &data), "the switch shows the stored off");
        assert!(matches!(update(&mut reopened, Message::Next, &data), Outcome::Stay(Action::None)));
        assert_eq!(reopened.step, Step::Appearance, "repeating setup preserves the stored off choice");

        // On again with that exception stored: one request, the exception switched on with the model.
        let mut on_again = sort_state();
        update(&mut on_again, Message::SortMail(true), &data);
        let sent = requests(update(&mut on_again, Message::Next, &data));
        assert!(
            matches!(&sent[..], [Request::SetAiAssignment { model: Some(model), enabled: true, .. }] if model == "another"),
            "{sent:?}"
        );

        // A model elsewhere sorts, which the step does not offer: "off" still switches it off.
        let mut elsewhere = data.clone();
        elsewhere.settings.ai_assignments.remove(&AiFunction::InboxCategorize);
        elsewhere.settings.ai_slots.decide =
            Some(torro_core::ai::SlotModel { engine: cloud.clone(), model: Some("cloud".into()), effort: None });
        let mut off = sort_state();
        update(&mut off, Message::SortMail(false), &elsewhere);
        let sent = requests(update(&mut off, Message::Next, &elsewhere));
        assert!(
            matches!(&sent[..], [Request::SetAiAssignment { engine: Some(engine), enabled: false, .. }] if *engine == cloud),
            "{sent:?}"
        );

        // An exception switched off with a model elsewhere, not offered here: "on" switches it on.
        let mut cloud_off = data.clone();
        cloud_off.settings.ai_assignments.insert(
            AiFunction::InboxCategorize,
            AiAssignment {
                function: AiFunction::InboxCategorize,
                engine: Some(cloud.clone()),
                model: Some("cloud".into()),
                effort: None,
                enabled: false,
            },
        );
        // Issue #424: without Sorting's permission it asks first; "Cancel" leaves it off.
        let mut on = sort_state();
        update(&mut on, Message::SortMail(true), &cloud_off);
        assert!(
            overlay(&on, &cloud_off, &suite::State::default()).is_some()
                && matches!(escape(&on), Some(Message::CancelCloud))
        );
        update(&mut on, Message::CancelCloud, &cloud_off);
        assert!(!mail_sorting_on(&on, &cloud_off) && overlay(&on, &cloud_off, &suite::State::default()).is_none());
        update(&mut on, Message::SortMail(true), &cloud_off);
        update(&mut on, Message::UseCloud, &cloud_off);
        let sent = requests(update(&mut on, Message::Next, &cloud_off));
        assert!(
            matches!(&sent[..], [Request::SetAiAssignment { engine: Some(engine), enabled: true, .. }] if *engine == cloud),
            "{sent:?}"
        );
        // A model here the step does not offer (turned off) asks nothing: no mail leaves.
        let mut here_off = cloud_off.clone();
        if let Some(held) = here_off.settings.ai_assignments.get_mut(&AiFunction::InboxCategorize) {
            held.engine = Some(local.clone());
            held.model = Some("off".into());
        }
        let mut on = sort_state();
        update(&mut on, Message::SortMail(true), &here_off);
        assert!(on.asking_cloud.is_none() && mail_sorting_on(&on, &here_off));
        // With the permission on, it was said yes to already: on without a question.
        let mut allowed = cloud_off.clone();
        allowed.settings.decide_mail_to_cloud = Some(true);
        let mut on = sort_state();
        update(&mut on, Message::SortMail(true), &allowed);
        assert!(on.asking_cloud.is_none() && mail_sorting_on(&on, &allowed));

        data.settings.decide_mail_to_cloud = Some(true);
        assert_eq!(mail_deciders(&data).len(), 3, "the cloud model needs Sorting's mail permission");
        data.ai_models.clear();
        let mut empty = sort_state();
        assert!(matches!(update(&mut empty, Message::Next, &data), Outcome::Stay(Action::None)));
        assert_eq!(empty.step, Step::Appearance, "no model is not silently assigned");
    }

    #[test]
    fn network_sorting_selection_waits_for_bound_consent_and_next_cannot_bypass_it() {
        for provider in AiProvider::LOCAL.into_iter().chain([AiProvider::OllamaCloud]) {
            let mut data = data();
            let mut account = data.accounts.iter().find(|account| account.kind == "ai").expect("AI").clone();
            let engine = format!("ai:{}@192.168.1.20:8080", provider.id());
            account.id = engine.clone();
            account.label = format!("{} (192.168.1.20:8080)", provider.name());
            account.state = torro_pim::AccountState::Connected {
                server: "http://192.168.1.20:8080/v1".into(),
                login_name: String::new(),
                last_sync: None,
                problem: None,
            };
            data.accounts.push(account);
            data.ai_models_from =
                data.accounts.iter().filter(|account| account.kind == "ai").map(|account| account.id.clone()).collect();
            data.ai_models = ["first", "second"]
                .into_iter()
                .map(|id| {
                    (
                        engine.clone(),
                        AiModel { remote: Some(false), decision: Some(true), enabled: true, ..AiModel::named(id) },
                    )
                })
                .collect();
            data.settings.ai_assignments.clear();
            data.settings.ai_slots.decide =
                Some(torro_core::ai::SlotModel { engine: engine.clone(), model: Some("first".into()), effort: None });
            for permission in [false, true] {
                data.settings.decide_mail_to_cloud = Some(permission);
                assert_eq!(mail_deciders(&data).len(), 2, "the actual menu offers LAN models before consent too");
                if !permission {
                    assert!(
                        selected_mail_decider(&sort_state(), &data).is_none(),
                        "remote is never a new default without permission"
                    );
                }
                if !permission {
                    let mut waiting = sort_state();
                    torro_settings_ui::accounts::ai_setup::demo(&mut waiting.ai_setup, "medium", &data);
                    update(&mut waiting, Message::SortModel(engine.clone(), "first".into()), &data);
                    assert!(waiting.asking_cloud.is_some());
                    update(&mut waiting, Message::UseCloud, &data);
                    let sent = requests(update(&mut waiting, Message::Next, &data));
                    assert!(
                        matches!(&sent[..], [Request::SetAiSlot { model: Some(model), .. }] if model == "first"),
                        "Use must also grant the missing permission for the unchanged model"
                    );
                }
                let mut setup = sort_state();
                torro_settings_ui::accounts::ai_setup::demo(&mut setup.ai_setup, "medium", &data);
                assert!(matches!(
                    update(&mut setup, Message::SortModel(engine.clone(), "second".into()), &data),
                    Outcome::Stay(Action::None)
                ));
                assert!(setup.sort_model.is_none() && setup.asking_cloud.is_some());
                assert!(matches!(update(&mut setup, Message::Next, &data), Outcome::Stay(Action::None)));
                assert_eq!(setup.step, Step::Ai);
                update(&mut setup, Message::CancelCloud, &data);
                assert!(setup.sort_model.is_none() && setup.confirmed_sort.is_none());
                update(&mut setup, Message::SortModel(engine.clone(), "second".into()), &data);
                update(&mut setup, Message::UseCloud, &data);
                assert_eq!(setup.sort_model, Some((engine.clone(), "second".into())));
                // Changing even to another model at the same account invalidates this consent.
                update(&mut setup, Message::SortModel(engine.clone(), "unlisted".into()), &data);
                assert!(setup.asking_cloud.is_some() && setup.confirmed_sort.is_none());
                update(&mut setup, Message::CancelCloud, &data);
                // The old selection is retained, but needs a fresh confirmation at Continue.
                assert!(matches!(update(&mut setup, Message::Next, &data), Outcome::Stay(Action::None)));
                assert_eq!(setup.step, Step::Ai);
                update(&mut setup, Message::UseCloud, &data);
                let sent = requests(update(&mut setup, Message::Next, &data));
                assert!(
                    matches!(&sent[..], [Request::SetAiSlot { engine: Some(sent_engine), model: Some(model), .. }] if sent_engine == &engine && model == "second")
                );
            }
            assert_eq!(sorting_place(&data, &engine, None), crate::assistant::Place::Remote);
            data.settings.decide_mail_to_cloud = Some(false);
            data.settings.ai_slots.decide = None;
            data.settings.ai_assignments.insert(
                AiFunction::InboxCategorize,
                AiAssignment {
                    function: AiFunction::InboxCategorize,
                    engine: Some(engine.clone()),
                    model: None,
                    effort: None,
                    enabled: false,
                },
            );
            let mut setup = sort_state();
            torro_settings_ui::accounts::ai_setup::demo(&mut setup.ai_setup, "medium", &data);
            update(&mut setup, Message::SortMail(true), &data);
            assert!(setup.asking_cloud.is_some(), "LAN Automatic asks too");
            assert!(matches!(update(&mut setup, Message::Next, &data), Outcome::Stay(Action::None)));
            update(&mut setup, Message::UseCloud, &data);
            let sent = requests(update(&mut setup, Message::Next, &data));
            assert!(matches!(&sent[..], [Request::SetAiAssignment { model: None, enabled: true, .. }]));
        }
    }

    #[test]
    fn preparing_a_disabled_network_target_requires_consent_but_switching_off_does_not() {
        let mut data = data();
        let engine = torro_settings_ui::accounts::demo::DEMO_NETWORK.to_string();
        data.settings.ai_slots.decide = None;
        data.settings.ai_assignments.clear();
        data.settings.decide_mail_to_cloud = Some(true);
        data.ai_models = vec![(
            engine.clone(),
            AiModel { decision: Some(true), remote: Some(false), enabled: true, ..AiModel::named("nimble") },
        )];
        let mut setup = sort_state();
        torro_settings_ui::accounts::ai_setup::demo(&mut setup.ai_setup, "medium", &data);
        update(&mut setup, Message::SortMail(false), &data);
        assert!(matches!(update(&mut setup, Message::Next, &data), Outcome::Stay(Action::None)));
        assert_eq!(setup.step, Step::Ai);
        assert!(setup.asking_cloud.is_some(), "a disabled target can send mail after a later re-enable");
        update(&mut setup, Message::CancelCloud, &data);
        assert!(setup.sort_model.is_none());
        update(&mut setup, Message::Next, &data);
        update(&mut setup, Message::UseCloud, &data);
        assert_eq!(setup.sort_mail, Some(false), "confirming the destination does not enable sorting");
        let sent = requests(update(&mut setup, Message::Next, &data));
        assert!(
            matches!(&sent[..], [Request::SetAiAssignment { engine: Some(sent), model: Some(model), enabled: false, .. }] if sent == &engine && model == "nimble")
        );

        data.settings.ai_slots.decide =
            Some(torro_core::ai::SlotModel { engine, model: Some("nimble".into()), effort: None });
        data.settings.decide_mail_to_cloud = Some(false);
        let mut setup = sort_state();
        torro_settings_ui::accounts::ai_setup::demo(&mut setup.ai_setup, "medium", &data);
        update(&mut setup, Message::SortMail(false), &data);
        let sent = requests(update(&mut setup, Message::Next, &data));
        assert!(matches!(&sent[..], [Request::SetAiAssignment { enabled: false, .. }]));
        assert!(setup.asking_cloud.is_none(), "turning the current target off is always safe");
    }
}
