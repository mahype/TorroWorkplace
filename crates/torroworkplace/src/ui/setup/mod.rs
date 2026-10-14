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

use std::collections::BTreeSet;

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, rule, scrollable, svg};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, ComponentSwitch, Language, Settings, SetupState};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, SidebarMode};
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets::{self, Trail};
use torro_gui::{style, typo};
use torro_ipc::{Request, SettingKey};

use super::{Action, Data, page_margin};

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

    /// Whether the step's page exists yet: W.4 builds Welcome, Components and Done, tasks W.5 to
    /// W.8 the others. A step that is not built is not listed, so the setup never shows an empty
    /// page.
    fn built(self) -> bool {
        matches!(self, Step::Welcome | Step::Components | Step::Done)
    }

    /// Whether the footer offers "Skip": not where there is nothing to skip (the welcome, the
    /// choice of components, the end).
    fn skippable(self) -> bool {
        !matches!(self, Step::Welcome | Step::Components | Step::Done)
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
#[derive(Debug, Clone)]
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
        }
    }

    /// Takes the choices from freshly loaded `settings` while none was made here yet: the setup
    /// can open before the core answered the first time.
    pub fn loaded(&mut self, settings: &Settings) {
        if !self.touched {
            *self = State {
                step: self.step,
                reached: self.reached,
                skipped: std::mem::take(&mut self.skipped),
                ..State::new(self.step, settings)
            };
        }
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
    }
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
}

/// What the setup wants the shell to do.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Stay in the setup and carry out the action.
    Stay(Action),
    /// Close the setup (it was finished or put off), carry out the action and show the overview.
    Close(Action),
}

pub fn update(state: &mut State, message: Message) -> Outcome {
    let stay = Outcome::Stay;
    match message {
        Message::Go(step) => {
            if state.steps().contains(&step) {
                state.go(step);
            }
            stay(Action::None)
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
    }
}

/// The request that changes one setting.
fn set(key: SettingKey, value: serde_json::Value) -> Action {
    Action::Send(Request::SetSetting { key, value })
}

/// The request that records where the setup stands.
fn set_setup(state: SetupState) -> Action {
    set(SettingKey::Setup, serde_json::to_value(state).unwrap_or_default())
}

/// Escape: one step back, nothing on the first step.
pub fn escape(state: &State) -> Option<Message> {
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
    let status = pages::status(state, data).map(|text| typo::text(typo::body(), text).style(theme::text_2));
    let mut buttons = row![].spacing(space::s()).align_y(Alignment::Center);
    if class != SizeClass::Narrow && state.neighbour(false).is_some() {
        buttons = buttons.push(widgets::text_button(None, tr("Back"), style::button::ghost, Some(Message::Back)));
    }
    if state.step.skippable() {
        buttons = buttons.push(widgets::text_button(None, tr("Skip"), style::button::secondary, Some(Message::Skip)));
    }
    buttons =
        buttons.push(widgets::text_button(None, pages::primary(state), style::button::primary, Some(Message::Next)));
    let left = container(row![].push(status)).width(Fill).clip(true);
    container(row![left, buttons].spacing(space::l()).align_y(Alignment::Center))
        .width(Fill)
        .center_y(size::header() + space::m())
        .padding(layout::row_padding())
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(step: Step) -> State {
        State::new(step, &Settings::default())
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
        assert_eq!(setup.steps(), [Step::Welcome, Step::Components, Step::Done]);
        assert!(matches!(update(&mut setup, Message::Next), Outcome::Stay(Action::None)));
        assert_eq!((setup.step, setup.position()), (Step::Components, (2, 3)));
        update(&mut setup, Message::Next);
        let finished = update(&mut setup, Message::Next);
        assert!(matches!(finished, Outcome::Close(_)));
        assert_eq!(sent(&finished), Some((SettingKey::Setup, serde_json::json!("done"))));
    }

    #[test]
    fn later_puts_it_off_and_back_stops_at_the_first_step() {
        let mut setup = state(Step::Welcome);
        assert!(escape(&setup).is_none(), "nothing before the welcome");
        update(&mut setup, Message::Back);
        assert_eq!(setup.step, Step::Welcome);
        let later = update(&mut setup, Message::Later);
        assert!(matches!(later, Outcome::Close(_)));
        assert_eq!(sent(&later), Some((SettingKey::Setup, serde_json::json!("skipped"))));
    }

    #[test]
    fn passed_steps_are_checked_and_clicked_ones_can_be_gone_back_to() {
        let mut setup = state(Step::Welcome);
        update(&mut setup, Message::Next);
        update(&mut setup, Message::Next);
        assert_eq!(setup.mark(Step::Welcome), Mark::Passed);
        update(&mut setup, Message::Go(Step::Welcome));
        assert_eq!(setup.step, Step::Welcome);
        assert_eq!(setup.mark(Step::Components), Mark::Passed, "reached before, still passed");
        update(&mut setup, Message::Go(Step::Calendar));
        assert_eq!(setup.step, Step::Welcome, "a step that is not walked cannot be gone to");
    }

    #[test]
    fn components_switch_at_once_and_are_sent_one_by_one() {
        let mut setup = state(Step::Components);
        let off = update(&mut setup, Message::Component(Component::Mail, false));
        assert_eq!(
            sent(&off),
            Some((SettingKey::Component, serde_json::json!({"component": "mail", "enabled": false})))
        );
        assert!(setup.off.contains(&Component::Mail));
        update(&mut setup, Message::Component(Component::Mail, true));
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
        let german = update(&mut setup, Message::Language(Language::German));
        assert_eq!(sent(&german), Some((SettingKey::Language, serde_json::json!("german"))));
        let dark = update(&mut setup, Message::Appearance(Appearance::Dark));
        assert_eq!(sent(&dark), Some((SettingKey::Appearance, serde_json::json!("dark"))));
        assert_eq!((setup.language, setup.appearance), (Language::German, Appearance::Dark));
    }

    #[test]
    fn the_loaded_settings_count_until_a_choice_is_made_here() {
        let mut setup = state(Step::Components);
        let stored = Settings { components_off: BTreeSet::from([Component::Assistant]), ..Settings::default() };
        setup.loaded(&stored);
        assert_eq!((setup.step, setup.off.clone()), (Step::Components, BTreeSet::from([Component::Assistant])));
        update(&mut setup, Message::Component(Component::Mail, false));
        setup.loaded(&Settings::default());
        assert_eq!(setup.off, BTreeSet::from([Component::Assistant, Component::Mail]), "a reload before the save");
    }

    #[test]
    fn steps_are_named_by_their_word() {
        for step in Step::ALL.into_iter().filter(|step| step.built()) {
            assert_eq!(Step::from_arg(step.arg()), Some(step));
        }
        assert_eq!(Step::from_arg("nextcloud"), None, "not built yet");
        assert_eq!(Step::args(), ["welcome", "components", "done"]);
    }
}
