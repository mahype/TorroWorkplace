//! "New list…" (task G.8 in `docs/tasks/github.md`, section "Neue Liste"): one entry under the
//! lists of the sidebar and in the navigation menu asks first what to make – a **task list** (own
//! tasks on a Nextcloud account) or a **GitHub filter list** – and then shows the form for it.
//!
//! - **The choice** ([`Dialog::new`]): two cards, keys 1 and 2, Escape or a click beside it
//!   closes. A card whose kind of account is missing is off and says so ("Connect a GitHub account
//!   in Settings first."). "GitHub filter list" hands over to the dialog of `filter.rs`
//!   ([`Outcome::GithubFilter`]); until task G.11 rebuilds that dialog, the app opens it for its
//!   first GitHub account.
//! - **"New task list"** ([`Dialog::task_list`]): the account (chips, only with several Nextcloud
//!   accounts), the name (focused), the colour (the five list colours) and how the row will look
//!   in the sidebar. "Create list" sends `tasks.create_list` (`PimSource::create_task_list`) and
//!   shows "Creating…" until the core answers, taking no second press. The answer closes the
//!   dialog ([`Outcome::Created`]); the app then shows the new list and says "List “{name}”
//!   created". A missing name shows under the field after the first try; a refusal of the core
//!   shows over the form with "Retry".
//!
//! The choice is always a dialog over the window; the form too in wide and medium windows, in
//! narrow ones a view of its own over the whole window, like the filter list dialog. Mini and
//! strip show no dialogs at all (the app does not draw them there).
//!
//! The app holds an `Option<Dialog>`, sends its [`Message`]s to [`Dialog::update`] and acts on the
//! [`Outcome`]; the request goes out through the app's [`Source`]. Nothing here reads or writes
//! data itself.

use iced::widget::{Space, column, container, operation, row, rule, scrollable};
use iced::{Alignment, Element, Fill, Padding, Size, Subscription, Task};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::source::Source;
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, Mark, Trail, collection_color};
use torro_gui::{style, typo};
use torro_pim::{Collection, PimSource, ProviderError};

use crate::filter::{field, labelled, list_colors, swatches};
use crate::list::{self, Tone};

/// Id of the name field of "New task list": focused when the form opens and when the name is
/// missing on "Create list".
pub const NAME_INPUT: &str = "new-task-list-name";
/// Width of the line under the head and over the foot.
const LINE: f32 = size::HAIRLINE;
/// The preview of the sidebar row is as wide as a row in the sidebar: the sidebar less its inset.
const PREVIEW_WIDTH: f32 = size::SIDEBAR - 2.0 * space::S;

/// A Nextcloud account a task list can go to, as the form offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountChoice {
    /// The account's id (`nextcloud:anna@cloud.example.org`): what `tasks.create_list` takes.
    pub id: String,
    /// How the sidebar names it (`anna@cloud.example.org`).
    pub label: String,
}

/// What "New list…" can offer, from the app's connected accounts. Built by the app each time the
/// dialog opens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offer {
    /// The connected accounts that hold task lists (Nextcloud), in the order of the sidebar. Empty:
    /// the "Task list" card is off.
    pub task_accounts: Vec<AccountChoice>,
    /// The account chosen at first in "New task list": the one of the list shown, if it is one of
    /// `task_accounts`; otherwise the first of them is.
    pub preferred: Option<String>,
    /// A GitHub account is connected; without one the "GitHub filter list" card is off.
    pub github: bool,
}

/// What can be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Own tasks on a Nextcloud account (`tasks.create_list`). Key 1.
    TaskList,
    /// A GitHub filter list (`filter.rs`). Key 2.
    GithubFilter,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A card of the choice, or its key (1, 2). Nothing happens for a card that is off.
    Pick(Kind),
    /// The arrow in the head of "New task list": back to the choice.
    Back,
    /// A chip of the account row.
    Account(String),
    Name(String),
    Color(Option<String>),
    /// "Create list", Enter in the name field, and "Retry" after a refusal.
    Create,
    /// The core's answer to "Create list": the new list.
    Created(Result<Collection, ProviderError>),
    /// "Cancel", the close button and Escape.
    Cancel,
    /// A click beside the dialog: closes the choice; keeps the form, which a stray click must not
    /// throw away.
    Backdrop,
    /// A click on the preview of the sidebar row: nothing, it only shows how the list will look.
    Preview,
}

/// What [`Dialog::update`] asks the app to do besides running the returned task.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Keep the dialog open.
    Stay,
    /// Close the dialog; nothing was made.
    Close,
    /// "GitHub filter list" was picked: close this dialog and open the one of `filter.rs` for a
    /// new filter list of a GitHub account.
    GithubFilter,
    /// The task list was made: close the dialog, show the list (it is in the sidebar at once),
    /// say "List “{name}” created" and load the lists again.
    Created(Collection),
}

/// "New list…": the choice, or the form "New task list" after it.
#[derive(Debug, Clone)]
pub struct Dialog {
    offer: Offer,
    /// `None` while the choice shows.
    form: Option<Form>,
}

/// The form "New task list".
#[derive(Debug, Clone, PartialEq, Eq)]
struct Form {
    /// The id of the account the list goes to, one of [`Offer::task_accounts`].
    account: String,
    name: String,
    /// `#RRGGBB`, one of the five list colours.
    color: Option<String>,
    /// "Create list" was pressed once: a missing name shows from now on.
    tried: bool,
    /// The request is out; "Creating…" shows and a second press does nothing.
    creating: bool,
    /// What went wrong with the last request, shown over the form with "Retry".
    problem: Option<String>,
}

impl Dialog {
    /// The choice "Task list" / "GitHub filter list" with what `offer` makes possible.
    pub fn new(offer: Offer) -> Dialog {
        Dialog { offer, form: None }
    }

    /// "New task list" straight away (for `--open new-task-list` and after picking it): the
    /// preferred account, no name, the first list colour. `None` without an account for task lists.
    pub fn task_list(offer: Offer) -> Option<Dialog> {
        let mut dialog = Dialog::new(offer);
        dialog.form = Some(dialog.new_form()?);
        Some(dialog)
    }

    /// Whether a card of the choice can be picked.
    pub fn available(&self, kind: Kind) -> bool {
        match kind {
            Kind::TaskList => !self.offer.task_accounts.is_empty(),
            Kind::GithubFilter => self.offer.github,
        }
    }

    /// An empty form on the preferred account; `None` without accounts for task lists.
    fn new_form(&self) -> Option<Form> {
        let accounts = &self.offer.task_accounts;
        let preferred = self.offer.preferred.as_ref().and_then(|id| accounts.iter().find(|a| &a.id == id));
        let account = preferred.or(accounts.first())?;
        Some(Form {
            account: account.id.clone(),
            name: String::new(),
            color: list_colors().into_iter().next(),
            tried: false,
            creating: false,
            problem: None,
        })
    }

    /// Carries out `message`; the request goes out through `source`. The task is the app's to run.
    pub fn update(&mut self, message: Message, source: &Source) -> (Task<Message>, Outcome) {
        let stay = |task| (task, Outcome::Stay);
        match message {
            Message::Pick(kind) if self.form.is_none() && self.available(kind) => match kind {
                Kind::TaskList => {
                    self.form = self.new_form();
                    return stay(operation::focus(NAME_INPUT));
                }
                Kind::GithubFilter => return (Task::none(), Outcome::GithubFilter),
            },
            Message::Cancel => return (Task::none(), Outcome::Close),
            Message::Backdrop if self.form.is_none() => return (Task::none(), Outcome::Close),
            _ => {}
        }
        let Some(form) = self.form.as_mut() else { return stay(Task::none()) };
        match message {
            // Back while the request is out would lose its answer: the button is off then.
            Message::Back if !form.creating => self.form = None,
            Message::Account(id) if self.offer.task_accounts.iter().any(|a| a.id == id) => form.account = id,
            Message::Name(name) => form.name = name,
            Message::Color(color) => form.color = color,
            Message::Create => return stay(form.create(source)),
            Message::Created(Ok(list)) if form.creating => return (Task::none(), Outcome::Created(list)),
            Message::Created(Err(err)) if form.creating => {
                form.creating = false;
                form.problem = Some(problem_text(&err));
            }
            _ => {}
        }
        stay(Task::none())
    }

    /// While the choice shows: its keys 1 and 2. Nothing while the form shows, where they are typed.
    pub fn subscription(&self) -> Subscription<Message> {
        match self.form {
            None => iced::event::listen_with(pick_key),
            Some(_) => Subscription::none(),
        }
    }

    /// The dialog in a window of `class` whose logical size is `window`: over the window, the
    /// form as a view of its own in narrow windows ([`Dialog::is_full`]).
    pub fn view(&self, class: SizeClass, window: Size) -> Element<'_, Message> {
        let wide = matches!(class, SizeClass::Wide | SizeClass::Medium);
        match &self.form {
            None => self.choice(wide, window),
            Some(form) if wide => self.form_dialog(form),
            Some(form) => self.form_full(form),
        }
    }

    /// Whether the dialog covers the whole window: the form in a narrow window.
    pub fn is_full(&self, class: SizeClass) -> bool {
        self.form.is_some() && !matches!(class, SizeClass::Wide | SizeClass::Medium)
    }

    // --- view ---

    /// The choice: title and close, the two cards (side by side, one under the other where the
    /// window is narrow) and a quiet line under them. A click beside it closes it.
    fn choice(&self, wide: bool, window: Size) -> Element<'_, Message> {
        let head = row![
            typo::text(typo::TITLE, tr("New list")).width(Fill),
            widgets::icon_button(Icon::X, tr("Close"), Some(Message::Cancel)),
        ]
        .align_y(Alignment::Center)
        .padding(Padding { top: space::L, right: space::M, bottom: space::S, left: space::XL });
        let card = |kind: Kind, icon: Icon, title: &'static str, text: &'static str, off: &'static str, key| {
            let on = self.available(kind);
            let text = if on { text } else { off };
            widgets::ChoiceCard { icon, title, text, key, on_press: on.then_some(Message::Pick(kind)) }
        };
        let cards = vec![
            card(
                Kind::TaskList,
                Icon::ListChecks,
                tr("Task list"),
                tr("Your own tasks, synced with Nextcloud. Add, edit and tick them off here."),
                tr("Connect a Nextcloud account in Settings first."),
                "1",
            ),
            card(
                Kind::GithubFilter,
                Icon::Github,
                tr("GitHub filter list"),
                tr("Issues and pull requests from your repositories, kept current by a search."),
                tr("Connect a GitHub account in Settings first."),
                "2",
            ),
        ];
        // Two cards need about the dialog's width; a narrower window gets them one under the other.
        let stacked = !wide || window.width < widgets::DIALOG_WIDTH + 2.0 * space::L;
        let cards = widgets::choice_cards(cards, stacked);
        let body = column![
            cards,
            typo::text(typo::META, tr("The new list shows in the sidebar right away.")).style(theme::text_3),
        ]
        .spacing(space::L)
        .padding(Padding { top: space::S, right: space::XL, bottom: space::XL, left: space::XL });
        let card = container(scrollable(column![head, body]).direction(vertical()).style(style::scrollable))
            .width(Fill)
            .style(style::container::popover);
        widgets::modal(Space::new().width(Fill).height(Fill), card, widgets::DIALOG_WIDTH, Message::Backdrop)
    }

    /// The form over the window: head with back and close, the fields scrolling where the window
    /// is low, the buttons at the foot.
    fn form_dialog<'a>(&'a self, form: &'a Form) -> Element<'a, Message> {
        let back = (!form.creating).then_some(Message::Back);
        let head = row![
            widgets::icon_button(Icon::ChevronLeft, tr("Back"), back),
            typo::text(typo::TITLE, tr("New task list")).width(Fill),
            widgets::icon_button(Icon::X, tr("Close"), Some(Message::Cancel)),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center)
        .padding(space::M);
        let body = scrollable(container(self.fields(form)).padding(space::XL).width(Fill))
            .direction(vertical())
            .style(style::scrollable);
        let card = column![
            head,
            rule::horizontal(LINE).style(style::rule),
            body,
            rule::horizontal(LINE).style(style::rule),
            container(footer(form)).padding([space::L, space::XL]),
        ];
        let card = container(card).width(Fill).style(style::container::popover);
        widgets::modal(Space::new().width(Fill).height(Fill), card, widgets::DIALOG_WIDTH, Message::Backdrop)
    }

    /// The form in a narrow window: a view of its own with a header bar ("‹ New task list ✕"), the
    /// fields scrolling and the buttons at the bottom.
    fn form_full<'a>(&'a self, form: &'a Form) -> Element<'a, Message> {
        let back = (!form.creating).then_some(Message::Back);
        let head = container(torro_gui::shell::header_bar(
            [widgets::icon_button(Icon::ChevronLeft, tr("Back"), back)],
            torro_gui::shell::header_title(None, tr("New task list"), None),
            [widgets::icon_button(Icon::X, tr("Close"), Some(Message::Cancel))],
        ))
        .center_y(size::HEADER)
        .padding([0.0, space::S]);
        let body = scrollable(container(self.fields(form)).padding(space::L).width(Fill))
            .direction(vertical())
            .style(style::scrollable)
            .height(Fill);
        let view = column![
            head,
            rule::horizontal(LINE).style(style::rule),
            body,
            rule::horizontal(LINE).style(style::rule),
            container(footer(form)).padding([space::M, space::L]),
        ];
        container(view).width(Fill).height(Fill).style(style::container::panel_flush).into()
    }

    /// What went wrong (with "Retry"), the account (only with several), name, colour and the
    /// preview of the sidebar row.
    fn fields<'a>(&'a self, form: &'a Form) -> Element<'a, Message> {
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if let Some(problem) = &form.problem {
            parts.push(problem_view(problem));
        }
        if self.offer.task_accounts.len() > 1 {
            let chips = self.offer.task_accounts.iter().map(|account| {
                let on = account.id == form.account;
                widgets::choice_chip(account.label.as_str(), None, on, Some(Message::Account(account.id.clone())))
            });
            parts.push(labelled(tr("Account"), widgets::chip_row(chips.collect()), None).into());
        }
        let name = field(tr("Groceries, Project X, Reading…"), &form.name)
            .id(NAME_INPUT)
            .on_input(Message::Name)
            .on_submit(Message::Create);
        parts.push(labelled(tr("Name"), name, form.name_problem()).into());
        let colours = list_colors().into_iter().map(Some).collect();
        parts.push(labelled(tr("Color"), swatches(colours, form.color.as_deref(), Message::Color), None).into());
        parts.push(labelled(tr("In the sidebar"), preview(form), None).into());
        column(parts).spacing(space::XL).into()
    }
}

impl Form {
    /// Why the name cannot be sent, once "Create list" was pressed.
    fn name_problem(&self) -> Option<&'static str> {
        (self.tried && self.name.trim().is_empty()).then(|| tr("Enter a name."))
    }

    /// Sends the list, or – without a name – shows the error and focuses the field. Nothing while
    /// a request is out.
    fn create(&mut self, source: &Source) -> Task<Message> {
        self.tried = true;
        if self.name_problem().is_some() {
            return operation::focus(NAME_INPUT);
        }
        if self.creating {
            return Task::none();
        }
        self.creating = true;
        self.problem = None;
        let (source, (account, name, color)) = (source.clone(), self.request());
        Task::perform(async move { source.create_task_list(account, name, color).await }, Message::Created)
    }

    /// What `tasks.create_list` gets: the account's id, the name without the spaces around it, the
    /// colour.
    fn request(&self) -> (String, String, Option<String>) {
        (self.account.clone(), self.name.trim().to_string(), self.color.clone())
    }
}

/// The foot: "Cancel" and the one primary button, "Create list" or "Creating…" while the request
/// is out.
fn footer(form: &Form) -> Element<'_, Message> {
    let create = if form.creating {
        widgets::busy_button(tr("Creating…"), style::button::primary)
    } else {
        widgets::text_button(None, tr("Create list"), style::button::primary, Some(Message::Create))
    };
    row![
        Space::new().width(Fill),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Cancel)),
        create,
    ]
    .spacing(space::S)
    .align_y(Alignment::Center)
    .into()
}

/// The row the list will have in the sidebar: its ring in the chosen colour, its name ("New list"
/// until one is typed) and no open tasks, as a chosen row.
fn preview(form: &Form) -> Element<'_, Message> {
    let name = match form.name.trim() {
        "" => tr("New list"),
        name => name,
    };
    let ring = Mark::Ring(collection_color(form.color.as_deref()));
    let row = widgets::sidebar_item(ring, name, Trail::Count(0), true, Message::Preview);
    container(row).width(PREVIEW_WIDTH).into()
}

/// A refusal of the core over the form: what happened and "Retry".
fn problem_view(problem: &str) -> Element<'_, Message> {
    let line = row![
        list::toned(Icon::CircleAlert, icon::SIZE, Tone::Error),
        typo::text(typo::BODY, problem).style(theme::text_2).width(Fill),
        widgets::text_button(None, tr("Retry"), style::button::ghost, Some(Message::Create)),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);
    container(line).padding(space::M).width(Fill).style(style::container::group).into()
}

/// The vertical scrollbar of the dialogs.
fn vertical() -> scrollable::Direction {
    scrollable::Direction::Vertical(style::scrollbar())
}

/// Keys 1 and 2 pick a card of the choice; not with Ctrl, Alt or the logo key, and not when a
/// widget took the key already.
fn pick_key(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event as Key, Key as Name};
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(Key::KeyPressed { key, modifiers, .. }) = event else { return None };
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key.as_ref() {
        Name::Character("1") => Some(Message::Pick(Kind::TaskList)),
        Name::Character("2") => Some(Message::Pick(Kind::GithubFilter)),
        _ => None,
    }
}

/// What the form says when the core refused: the sign-in, the connection, else the core's own
/// message ("No account …", "This account has no task lists …").
fn problem_text(err: &ProviderError) -> String {
    match err {
        ProviderError::Auth => tr("The server refused the sign-in. Check the account in Settings.").into(),
        ProviderError::Offline => tr("The server can't be reached. Check the connection and try again.").into(),
        other => trf("Couldn't create the list: {error}", &[("error", other)]),
    }
}

/// "List “{name}” created", the card the app shows after [`Outcome::Created`].
pub fn created_text(name: &str) -> String {
    trf("List “{name}” created", &[("name", &name)])
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::NaiveDate;
    use torro_pim::demo::{DemoSource, HOME_ACCOUNT, WORK_ACCOUNT};

    use super::*;

    fn demo() -> Source {
        Source::Demo(Arc::new(DemoSource::new(NaiveDate::from_ymd_opt(2026, 10, 12).expect("valid date"))))
    }

    fn choice(id: &str) -> AccountChoice {
        AccountChoice { id: id.into(), label: id.rsplit(':').next().unwrap_or(id).into() }
    }

    /// Both Nextcloud accounts of the demo and its GitHub account.
    fn offer() -> Offer {
        Offer { task_accounts: vec![choice(WORK_ACCOUNT), choice(HOME_ACCOUNT)], preferred: None, github: true }
    }

    fn form(dialog: &Dialog) -> &Form {
        dialog.form.as_ref().expect("the form shows")
    }

    #[test]
    fn the_choice_leads_to_the_right_dialog() {
        let mut dialog = Dialog::new(offer());
        assert!(dialog.form.is_none(), "the choice first");
        let (_, outcome) = dialog.update(Message::Pick(Kind::GithubFilter), &demo());
        assert_eq!(outcome, Outcome::GithubFilter, "the filter list dialog takes over");
        let (_, outcome) = dialog.update(Message::Pick(Kind::TaskList), &demo());
        assert_eq!(outcome, Outcome::Stay);
        let form = form(&dialog);
        assert_eq!((form.account.as_str(), form.name.as_str()), (WORK_ACCOUNT, ""));
        assert_eq!(form.color, list_colors().first().cloned(), "the first list colour");
        // Back to the choice, and Escape closes either.
        let _ = dialog.update(Message::Back, &demo());
        assert!(dialog.form.is_none());
        assert_eq!(dialog.update(Message::Cancel, &demo()).1, Outcome::Close);
    }

    #[test]
    fn a_card_without_its_account_is_off() {
        let mut dialog = Dialog::new(Offer::default());
        assert!(!dialog.available(Kind::TaskList) && !dialog.available(Kind::GithubFilter));
        assert_eq!(dialog.update(Message::Pick(Kind::GithubFilter), &demo()).1, Outcome::Stay);
        let _ = dialog.update(Message::Pick(Kind::TaskList), &demo());
        assert!(dialog.form.is_none(), "nothing to make a task list in");
        assert!(Dialog::task_list(Offer::default()).is_none());
        // A click beside the choice closes it; beside the form it does not.
        assert_eq!(dialog.update(Message::Backdrop, &demo()).1, Outcome::Close);
        let mut dialog = Dialog::task_list(offer()).expect("the form");
        assert_eq!(dialog.update(Message::Backdrop, &demo()).1, Outcome::Stay);
        assert!(dialog.form.is_some());
    }

    #[test]
    fn the_preferred_account_is_chosen_first() {
        let preferred = Offer { preferred: Some(HOME_ACCOUNT.into()), ..offer() };
        let dialog = Dialog::task_list(preferred).expect("the form");
        assert_eq!(form(&dialog).account, HOME_ACCOUNT);
        let unknown = Offer { preferred: Some("github:mahype".into()), ..offer() };
        assert_eq!(form(&Dialog::task_list(unknown).expect("the form")).account, WORK_ACCOUNT);
    }

    #[test]
    fn create_sends_account_name_and_colour() {
        let mut dialog = Dialog::task_list(offer()).expect("the form");
        let (_, outcome) = dialog.update(Message::Create, &demo());
        assert_eq!(outcome, Outcome::Stay);
        assert!(!form(&dialog).creating, "nothing is sent without a name");
        assert_eq!(form(&dialog).name_problem(), Some(tr("Enter a name.")));

        let colour = list_colors()[2].clone();
        let _ = dialog.update(Message::Account(HOME_ACCOUNT.into()), &demo());
        let _ = dialog.update(Message::Account("nextcloud:someone@else".into()), &demo());
        let _ = dialog.update(Message::Name("  Reading ".into()), &demo());
        let _ = dialog.update(Message::Color(Some(colour.clone())), &demo());
        let (task, _) = dialog.update(Message::Create, &demo());
        assert!(task.units() > 0 && form(&dialog).creating, "Creating…");
        let request = form(&dialog).request();
        assert_eq!(request, (HOME_ACCOUNT.to_string(), "Reading".to_string(), Some(colour.clone())));
        assert_eq!(dialog.update(Message::Create, &demo()).0.units(), 0, "no second request");
        assert_eq!(dialog.update(Message::Back, &demo()).1, Outcome::Stay);
        assert!(dialog.form.is_some(), "no way back while the request is out");

        // What the demo source (like the core) answers to that request.
        let (account, name, color) = request;
        let list = iced::futures::executor::block_on(demo().create_task_list(account, name, color))
            .expect("the demo makes task lists");
        assert!(list.id.starts_with(HOME_ACCOUNT), "{} is on the chosen account", list.id);
        assert_eq!((list.name.as_str(), list.color.as_deref()), ("Reading", Some(colour.as_str())));
        let (_, outcome) = dialog.update(Message::Created(Ok(list.clone())), &demo());
        assert_eq!(outcome, Outcome::Created(list.clone()));
        assert_eq!(created_text("Reading"), trf("List “{name}” created", &[("name", &"Reading")]));
    }

    #[test]
    fn a_refusal_shows_with_retry_and_keeps_the_form() {
        let mut dialog = Dialog::task_list(offer()).expect("the form");
        let _ = dialog.update(Message::Name("Reading".into()), &demo());
        let _ = dialog.update(Message::Create, &demo());
        let (_, outcome) = dialog.update(Message::Created(Err(ProviderError::Offline)), &demo());
        assert_eq!(outcome, Outcome::Stay);
        let now = form(&dialog);
        assert!(!now.creating);
        assert_eq!(now.problem, Some(problem_text(&ProviderError::Offline)));
        assert_eq!(now.name, "Reading", "what was typed stays");
        // "Retry" sends again and clears the problem meanwhile.
        let (task, _) = dialog.update(Message::Create, &demo());
        assert!(task.units() > 0 && form(&dialog).creating && form(&dialog).problem.is_none());
    }

    #[test]
    fn the_dialog_builds_in_every_layout() {
        let mut choice_only = Dialog::new(Offer { github: false, ..offer() });
        let mut form = Dialog::task_list(offer()).expect("the form");
        let _ = form.update(Message::Created(Err(ProviderError::Auth)), &demo());
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            for size in [Size::new(1600.0, 1000.0), Size::new(800.0, 600.0), Size::new(480.0, 900.0)] {
                let _ = choice_only.view(class, size);
                let _ = form.view(class, size);
            }
        }
        assert!(form.is_full(SizeClass::Narrow) && !form.is_full(SizeClass::Medium));
        assert!(!choice_only.is_full(SizeClass::Narrow), "the choice is always a dialog");
        let _ = choice_only.update(Message::Pick(Kind::TaskList), &demo());
        assert!(choice_only.is_full(SizeClass::Narrow));
    }
}
