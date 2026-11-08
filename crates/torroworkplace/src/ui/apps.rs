//! The section Apps (issue #411, sub-issue #480): the app catalog. Every app of the suite as a card
//! with its icon, its question and a sentence about it, its version and what can be done with it –
//! the installed apps first, the missing ones below –, a search over them, "Update all" and the update
//! of the main application. Draft: `docs/prototypes/app-catalog/index.html`; the target picture of the
//! plan in issue #411 decides where they differ (no sorting, no list view, no "Reinstall", no card
//! "Coming soon").
//!
//! The page reads and writes nothing itself (skill `torro-api`): the catalog comes from
//! `apps.list_catalog` with the shell's reloads, opening the section asks `apps.check_updates`
//! (decision 2: the core asks GitHub at most every five minutes), and every button is a request –
//! `apps.install_app`, `apps.update_apps`, `apps.open_app`, the setting `component` for "Turn off". The
//! jobs run in a small window of their own ([`super::app_job`], `torroworkplace ui --app-job=…`), which
//! the shell starts once the core answered with the job; uninstalling and the update of the main
//! application ask there first, so the page only opens that window. While a job runs, its card shows
//! how far it is and "Show" opens its window again.
//!
//! An app a package manager looks after (decision 1) shows the command to run instead of a button,
//! with "Copy" beside it. On/off of an app is the menu "…" of its card (decision 3); a turned-off app
//! says "Turned off" where it says "Installed" otherwise.
//!
//! Layout: wide windows lay the cards out in a grid that gets more columns with the width; medium
//! ones too once two cards of `size::panel()` fit (the design review of #411 found one stretched
//! column too wide), else one column; narrow ones one column. One column comes only where two cards
//! do not fit (or the catalog has a single app, which the suite's catalog of all its apps never has),
//! so it is never wider than two cards and needs no cap of its own (issue #548). The
//! narrow header keeps "Check for updates" and "Update all" as icons and the search moves over the
//! cards.

use chrono::{DateTime, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, space as blank, text_input};
use iced::{Alignment, Element, Fill, Length};
use torro_core::AppId;
use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::{Component, ComponentSwitch, Settings};
use torro_gui::datetime::{format_exact, format_moment};
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell;
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, Align, Anchor, MenuItem, MenuSection, Placement, Status, TileFace, TileSize};
use torro_gui::{style, typo, units};
use torro_ipc::{
    AppCatalog, AppJobInfo, AppJobKind, AppJobState, AppJobStep, AppStep, CatalogApp, CatalogState, ManagedBy, Request,
    Response, SettingKey, StepStatus,
};

use super::app_job::Target;
use super::{Action, Data, Header};

/// The window class of the main application in the catalog and in `apps.update_apps`.
pub const MAIN_APP: &str = "torroworkplace";

/// The catalog as the page has it.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Catalog {
    /// Not answered yet.
    #[default]
    Loading,
    /// `apps.list_catalog` did not go through: the page shows why, with "Retry".
    Failed(String),
    /// What the core answered.
    Listed(Box<AppCatalog>),
}

impl Catalog {
    /// The catalog, once the core answered with one.
    pub fn listed(&self) -> Option<&AppCatalog> {
        match self {
            Catalog::Listed(catalog) => Some(catalog),
            _ => None,
        }
    }
}

/// What the shell loads for the section: the catalog and the job that runs, if one does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loaded {
    pub catalog: Catalog,
    pub job: Option<AppJobInfo>,
}

/// The page's own state.
#[derive(Debug, Default)]
pub struct State {
    /// What is typed into "Search apps".
    query: String,
    /// The app (window class) whose menu "…" is open.
    menu: Option<String>,
    /// Where that menu hangs: the "…" of its card.
    anchor: Anchor,
    /// `apps.check_updates` is on its way: the footer says "Checking…".
    checking: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A key in "Search apps".
    Query(String),
    /// "Check for updates" (the arrows in the header) or "Retry" after a failed check.
    Check,
    /// The "…" of an app's card.
    ToggleMenu(String),
    /// A click next to the open menu, or Escape.
    CloseMenu,
    /// "Open": the app's window, by window class (`apps.open_app`).
    Open(String),
    /// "Release notes": the release's page on GitHub.
    ReleaseNotes,
    /// The native installers' download index; no file changes in the catalog.
    Downloads,
    /// "Install": `apps.install_app` for the app.
    Install(String),
    /// "Update to 0.2.0" of one app: `apps.update_apps` with it alone.
    Update(String),
    /// "Update all": `apps.update_apps` with every app that has an update.
    UpdateAll,
    /// "Update…" of the main application: its window, which asks first (the service restarts).
    SelfUpdate,
    /// "Turn off" / "Turn on" in the menu (decision 3): the setting `component`.
    Switch(Component, bool),
    /// "Show in Finder" (or the system's file manager) in the menu: where the app lies.
    Reveal(String),
    /// "Uninstall…" in the menu: the job window, which asks first.
    Uninstall(String),
    /// "Copy" beside the command of a package manager.
    Copy(String),
    /// "Show" on the card of an app whose job runs: its window again.
    ShowJob(String),
    /// A key in the field of a command: the field only shows it, so it can be selected.
    Keep,
}

/// What opening the section asks for: `apps.check_updates`, so the page shows what is new (decision
/// 2). The core answers from its last check within five minutes.
pub fn opened(state: &mut State) -> Action {
    state.menu = None;
    state.checking = true;
    Action::CheckApps
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    let catalog = data.app_catalog.catalog.listed();
    let close = |state: &mut State| state.menu = None;
    match message {
        Message::Query(query) => {
            state.query = query;
            Action::None
        }
        Message::Check => {
            state.checking = true;
            Action::CheckApps
        }
        Message::ToggleMenu(app) => {
            state.menu = if state.menu.as_ref() == Some(&app) { None } else { Some(app) };
            Action::None
        }
        Message::CloseMenu => {
            close(state);
            Action::None
        }
        Message::Open(app) => {
            close(state);
            Action::Send(Request::OpenApp { app, target: None })
        }
        Message::ReleaseNotes => {
            close(state);
            match catalog.and_then(|catalog| catalog.release_url.clone()) {
                Some(url) => Action::OpenUrl(url),
                None => Action::None,
            }
        }
        Message::Downloads => {
            close(state);
            Action::OpenUrl("https://github.com/mahype/TorroWorkplace/releases/latest".into())
        }
        Message::Install(app) => Action::StartAppJob(Request::InstallApp { app }),
        Message::Update(app) if app == MAIN_APP => Action::AppWindow(Target::SelfUpdate),
        Message::Update(app) => Action::StartAppJob(Request::UpdateApps { apps: vec![app] }),
        Message::UpdateAll => match catalog.map(updatable).unwrap_or_default() {
            apps if apps.is_empty() => Action::None,
            apps => Action::StartAppJob(Request::UpdateApps { apps }),
        },
        Message::SelfUpdate => Action::AppWindow(Target::SelfUpdate),
        Message::Switch(component, enabled) => {
            close(state);
            let value = serde_json::to_value(ComponentSwitch { component, enabled }).unwrap_or_default();
            Action::Send(Request::SetSetting { key: SettingKey::Component, value })
        }
        Message::Reveal(app) => {
            close(state);
            // Where the core found it, so the menu and the card never disagree (issue #411).
            let path = catalog.and_then(|catalog| entry_of(catalog, &app)).and_then(|entry| entry.path.clone());
            match AppId::find(&app) {
                Some(app) => Action::Reveal(app, path),
                None => Action::None,
            }
        }
        Message::Uninstall(app) => {
            close(state);
            Action::AppWindow(Target::Uninstall(app))
        }
        Message::Copy(command) => {
            close(state);
            Action::Copy(command)
        }
        Message::ShowJob(id) => Action::AppWindow(Target::Follow(id)),
        Message::Keep => Action::None,
    }
}

/// The answer to `apps.check_updates`: the catalog it brings replaces the page's; a refusal or no
/// answer keeps what the page shows (the reloads go on).
pub fn checked(state: &mut State, loaded: &mut Loaded, result: &Result<Response, String>) {
    state.checking = false;
    if let Ok(Response::AppCatalog { catalog }) = result {
        loaded.catalog = Catalog::Listed(Box::new(catalog.clone()));
    }
}

/// The answer to `apps.install_app` or `apps.update_apps`: the job's id, whose window the shell
/// opens, or what to say instead – the core's codes in words for people (its own text is English for
/// the log).
pub fn started(result: &Result<Response, String>) -> Result<AppJobInfo, String> {
    match result {
        Ok(Response::AppJob { job }) => Ok(job.clone()),
        Ok(Response::Error { code, message }) => {
            eprintln!("torroworkplace: an app job was refused ({code}): {message}");
            Err(refusal_with(code, message))
        }
        Ok(other) => {
            eprintln!("torroworkplace: unexpected answer to an app job: {other:?}");
            Err(tr("Something went wrong. Try again.").to_string())
        }
        Err(detail) => {
            eprintln!("torroworkplace: no answer to an app job: {detail}");
            Err(tr("Torro Workplace isn't responding. It may still be starting.").to_string())
        }
    }
}

/// What to say when the core refuses an app job: for `conflict` the core's own sentence, which names
/// the reason and the next step ("… is needed by …; uninstall that first") from the suite's names
/// only; for the other codes [`refusal`].
pub fn refusal_with(code: &str, message: &str) -> String {
    match code {
        "conflict" if !message.is_empty() => message.to_string(),
        _ => refusal(code).to_string(),
    }
}

/// The words for a code the core refuses an app job with (`docs/api-reference.md`, `apps.*`).
pub fn refusal(code: &str) -> &'static str {
    match code {
        "busy" => tr("Another app is being installed or updated. Try again when it is done."),
        "offline" => tr("Couldn't reach GitHub."),
        "conflict" => tr("This app can't be changed here right now."),
        "unsupported" => tr("This build of Torro Workplace can't install apps. Use a release."),
        "not-found" => tr("Torro Workplace doesn't know this app."),
        _ => tr("Something went wrong. Try again."),
    }
}

/// The entry of `class` in `catalog`: an app or the main application.
fn entry_of<'a>(catalog: &'a AppCatalog, class: &str) -> Option<&'a CatalogApp> {
    std::iter::once(&catalog.main).chain(&catalog.apps).find(|app| app.window_class == class)
}

/// The apps "Update all" updates: every app the catalog has an update for, never the main
/// application, whose update restarts the service and asks first ([`Message::SelfUpdate`]).
pub fn updatable(catalog: &AppCatalog) -> Vec<String> {
    catalog.apps.iter().filter(|app| app.state == CatalogState::Update).map(|app| app.window_class.clone()).collect()
}

/// How many updates "Update all" takes, for the sidebar's count: the same number as the button, so
/// the two never disagree. The main application's update is not counted; its question card says
/// it, and the sidebar's dot ([`waiting`]).
pub fn updates(data: &Data) -> usize {
    data.app_catalog.catalog.listed().map_or(0, |catalog| updatable(catalog).len())
}

/// Whether any update waits, the main application's too: the dot on the section's icon.
pub fn waiting(data: &Data) -> bool {
    data.app_catalog
        .catalog
        .listed()
        .is_some_and(|catalog| !updatable(catalog).is_empty() || catalog.main.state == CatalogState::Update)
}

/// Whether a job runs now, so the shell reloads faster and the card follows it.
pub fn job_runs(data: &Data) -> bool {
    data.app_catalog.job.as_ref().is_some_and(|job| job.state == AppJobState::Running)
}

/// Escape closes the open menu.
pub fn escape(state: &State) -> Option<Message> {
    state.menu.as_ref().map(|_| Message::CloseMenu)
}

/// The part of the suite an app's switch turns off: mail is Mail, the hub the Notification Hub;
/// the App Feed is a window of the hub and follows its switch, so it has none of its own.
pub fn component_of(app: &str) -> Option<Component> {
    match app {
        "torro-calendar" => Some(Component::Calendar),
        "torro-tasks" => Some(Component::Tasks),
        "torro-inbox-mail" => Some(Component::Mail),
        "torro-inbox" => Some(Component::Inbox),
        "torro-assistant" => Some(Component::Assistant),
        _ => None,
    }
}

/// Whether the app is turned off: its own switch, the App Feed with the hub's.
fn turned_off(app: &str, settings: &Settings) -> bool {
    let component = match app {
        "torro-inbox-notifications" => Some(Component::Inbox),
        app => component_of(app),
    };
    component.is_some_and(|component| !settings.component_on(component))
}

/// The entry of [`AppId::ALL`] a catalog entry is, for its icon and its name.
fn app_id(app: &CatalogApp) -> Option<AppId> {
    AppId::find(&app.window_class).or_else(|| AppId::find(&app.program))
}

/// The name a card shows: "Torro" and the app's name in the switcher, translated ("Torro Calendar",
/// "Torro Mail"); the catalog's English name for an app the suite does not know yet.
pub fn product_name(app: &CatalogApp) -> String {
    match app_id(app) {
        Some(id) if id == AppId::WORKPLACE => "Torro Workplace".to_string(),
        Some(id) => format!("Torro {}", shell::app_name(id)),
        None => app.name.clone(),
    }
}

/// The question an app answers and the sentence about it, from the draft.
pub fn words(app: &str) -> (&'static str, &'static str) {
    match app {
        "torro-calendar" => (
            tr("What's coming up?"),
            tr("Your days and weeks from Nextcloud and other CalDAV calendars, with reminders on every display."),
        ),
        "torro-tasks" => (
            tr("What do I need to do?"),
            tr("Lists from Nextcloud Tasks and GitHub issues in one place, sorted by what is due."),
        ),
        "torro-inbox-mail" => (
            tr("What came in by mail?"),
            tr("Mail for the tiled desktop: IMAP accounts, categories, filters and a backup of every mailbox."),
        ),
        "torro-inbox" => (
            tr("What wants my attention?"),
            tr("Approvals, waiting agents and account problems, all in one list you can clear."),
        ),
        "torro-inbox-notifications" => (
            tr("What did my apps report?"),
            tr("Notifications from GitHub, Nextcloud, Talk, ntfy and your own scripts."),
        ),
        "torro-assistant" => (
            tr("Can you do that for me?"),
            tr("Chat with a local or cloud model that may use your mail, calendar and tasks as you allow."),
        ),
        "torro-chat" => (
            tr("Who is there to talk to?"),
            tr("Your colleagues on Nextcloud Talk: who is online, a window per chat, files and quick replies."),
        ),
        _ => ("", ""),
    }
}

/// Whether `app` matches what is typed into the search: its name, its question or its sentence,
/// without regard to case. An empty search matches every app.
pub fn matches(app: &CatalogApp, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let (question, sentence) = words(&app.window_class);
    [product_name(app).as_str(), question, sentence, app.window_class.as_str()]
        .iter()
        .any(|text| text.to_lowercase().contains(&query))
}

/// Whether the card stands under "Installed": the app is on this computer.
fn is_installed(app: &CatalogApp) -> bool {
    app.installed.is_some()
}

/// What a card offers, from the app's state, its switch and the job that runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Offer<'a> {
    /// A job for the app runs: how far it is and "Show".
    Running(&'a AppJobInfo),
    /// A package manager looks after it: the command to run, if there is one now.
    Managed(&'a ManagedBy, Option<&'a str>),
    /// "Update to 0.2.0".
    Update,
    /// Turned off: "Turn on".
    TurnOn,
    /// "Open".
    Open,
    /// "Install · 24.6 MB".
    Install,
    /// Missing, and the release has nothing for this system (or nothing was checked yet).
    Unavailable,
}

/// The apps `job` touches, in the order of its steps: those asked for and what they bring along (the
/// Notification Hub for Mail), each once.
pub fn involved(job: &AppJobInfo) -> Vec<&str> {
    let mut apps: Vec<&str> = Vec::new();
    for step in &job.steps {
        if !apps.contains(&step.app.as_str()) {
            apps.push(&step.app);
        }
    }
    apps
}

/// What the card of `app` offers.
pub fn offer<'a>(app: &'a CatalogApp, settings: &Settings, job: Option<&'a AppJobInfo>) -> Offer<'a> {
    if let Some(job) =
        job.filter(|job| job.state == AppJobState::Running && involved(job).contains(&app.window_class.as_str()))
    {
        return Offer::Running(job);
    }
    match (app.state, &app.managed) {
        (CatalogState::Managed, Some(managed)) => {
            let newer = app.installed.is_some() && app.available.is_some() && app.available != app.installed;
            let command = match (app.installed.is_some(), newer) {
                (false, _) => Some(managed.install.as_str()),
                (true, true) => Some(managed.update.as_str()),
                (true, false) => None,
            };
            Offer::Managed(managed, command)
        }
        (CatalogState::Update, _) => Offer::Update,
        (CatalogState::Available, _) => Offer::Install,
        _ if is_installed(app) && turned_off(&app.window_class, settings) => Offer::TurnOn,
        _ if is_installed(app) => Offer::Open,
        _ => Offer::Unavailable,
    }
}

/// The installed apps that bring `app` along (Mail and App Feed bring the Notification Hub), by
/// name: it cannot be uninstalled while they are there.
fn needed_by(app: &CatalogApp, catalog: &AppCatalog) -> Vec<String> {
    catalog
        .apps
        .iter()
        .filter(|other| is_installed(other) && other.requires.contains(&app.window_class))
        .map(product_name)
        .collect()
}

/// The header: "Apps" with how many are installed and wait for an update, and at the right "Check
/// for updates", the search (not in narrow windows, where it stands over the cards) and "Update all
/// (2)" as the window's one primary button while there are updates.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    let catalog = data.app_catalog.catalog.listed();
    let narrow = !matches!(class, SizeClass::Wide | SizeClass::Medium);
    let check = (!state.checking).then_some(Message::Check);
    let mut actions: Vec<Element<'a, Message>> =
        vec![widgets::icon_button(Icon::RefreshCw, tr("Check for updates"), check)];
    if !narrow {
        actions.push(search(state, Length::Fixed(size::sidebar())));
    }
    let count = catalog.map(updatable).map_or(0, |apps| apps.len());
    if count > 0 {
        let label = trf("Update all ({n})", &[("n", &count)]);
        actions.push(match narrow {
            true => widgets::compact_button(Icon::Download, label, style::button::primary, Some(Message::UpdateAll)),
            false => widgets::text_button(None, label, style::button::primary, Some(Message::UpdateAll)),
        });
    }
    Header { sub_page: None, actions }
}

/// The quiet words beside the title of a wide header: "5 installed · 2 updates".
pub fn note(data: &Data) -> Option<String> {
    data.app_catalog.catalog.listed().map(summary)
}

/// "5 installed · 2 updates", quietly beside the title.
fn summary(catalog: &AppCatalog) -> String {
    let installed = catalog.apps.iter().filter(|app| is_installed(app)).count()
        + usize::from(catalog.main.state == CatalogState::Managed && is_installed(&catalog.main));
    let updates = updatable(catalog).len();
    let installed = trf("{n} installed", &[("n", &installed)]);
    match updates {
        0 => installed,
        1 => format!("{installed} · {}", tr("1 update")),
        n => format!("{installed} · {}", trf("{n} updates", &[("n", &n)])),
    }
}

/// "Search apps", `width` wide: as wide as the sidebar in the header, the page's width over the cards.
fn search(state: &State, width: Length) -> Element<'_, Message> {
    let field = text_input(tr("Search apps"), &state.query)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
        .padding(style::INPUT_PADDING)
        .width(width)
        .style(style::text_input)
        .on_input(Message::Query);
    field.into()
}

/// The page: the main application's update when there is one, the installed apps, the missing ones
/// and the footer with when the catalog was checked; loading and failing as every page.
///
/// `width` is the room of the page inside its margin: the cards stand in as many columns as fit
/// ([`columns_for`]). A failed check of GitHub says so at the top, over the cards, with "Retry".
pub fn view<'a>(state: &'a State, data: &'a Data, class: SizeClass, width: f32) -> Element<'a, Message> {
    let catalog = match &data.app_catalog.catalog {
        Catalog::Loading => return widgets::loading(),
        Catalog::Failed(_) => return widgets::error_state(tr("Couldn't load the apps."), Message::Check),
        Catalog::Listed(catalog) => catalog.as_ref(),
    };
    let job = data.app_catalog.job.as_ref();
    let mut page = column![].spacing(layout::section_gap()).width(Fill);
    if !matches!(class, SizeClass::Wide | SizeClass::Medium) {
        page = page.push(search(state, Fill));
    }
    if let Some(code) = catalog.error.as_deref() {
        page = page.push(check_failed(code));
    }
    if catalog.main.state == CatalogState::Update {
        page = page.push(main_update(&catalog.main));
    }
    let shown: Vec<&CatalogApp> = std::iter::once(&catalog.main)
        .filter(|main| main.state == CatalogState::Managed)
        .chain(&catalog.apps)
        .filter(|app| matches(app, &state.query))
        .collect();
    let (installed, missing): (Vec<&CatalogApp>, Vec<&CatalogApp>) =
        shown.into_iter().partition(|app| is_installed(app));
    if installed.is_empty() && missing.is_empty() {
        let hint = if catalog.apps.is_empty() && catalog.checked.is_some() {
            tr("The newest release has no apps for this system.")
        } else if catalog.apps.is_empty() {
            tr("The catalog lists the apps of the newest release once it has been checked.")
        } else {
            tr("Try another word, or clear the search.")
        };
        let title = if catalog.apps.is_empty() { tr("No apps yet") } else { tr("No app matches") };
        page = page.push(widgets::empty_state(title, hint));
    }
    let columns = columns_for(class, width, catalog.apps.len());
    for (title, apps) in [(tr("Installed"), installed), (trc("app", "Available"), missing)] {
        if apps.is_empty() {
            continue;
        }
        let cards: Vec<Element<'a, Message>> =
            apps.iter().map(|app| card(app, catalog, &data.settings, job, state)).collect();
        page = page.push(column![heading(title, cards.len()), grid(cards, columns)].spacing(space::m()));
    }
    page.push(footer(state, catalog)).into()
}

/// "Installed 5" over a group of cards.
fn heading<'a>(title: &'a str, count: usize) -> Element<'a, Message> {
    row![
        typo::text(typo::caption(), title).style(theme::text_3),
        typo::text(typo::meta().tabular(), count.to_string()).style(theme::text_3),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center)
    .into()
}

/// The cards in rows of `columns`; a last row that is not full keeps the width of the others. One
/// column spans the page: [`columns_for`] gives one only where two cards do not fit or there is a
/// single card, which the catalog of all the suite's apps never is.
fn grid<'a>(cards: Vec<Element<'a, Message>>, columns: usize) -> Element<'a, Message> {
    if columns <= 1 {
        return column(cards).spacing(size::card_gap()).into();
    }
    let mut rows = column![].spacing(size::card_gap());
    let mut cards = cards.into_iter().peekable();
    while cards.peek().is_some() {
        let mut line = row![].spacing(size::card_gap());
        for _ in 0..columns {
            line = match cards.next() {
                Some(card) => line.push(container(card).width(Length::FillPortion(1))),
                None => line.push(Space::new().width(Length::FillPortion(1))),
            };
        }
        rows = rows.push(line);
    }
    rows.into()
}

/// How many cards stand side by side in a wide page `width` wide: as many as fit [`size::panel()`]
/// each with the gaps, at least two, never more than there are cards (and at least one). Both groups
/// take the count of all apps, so their cards are equally wide.
pub fn columns(width: f32, cards: usize) -> usize {
    fit(width).max(2).min(cards.max(1))
}

/// How many cards of at least [`size::panel()`] fit side by side in `width`, with the gaps.
fn fit(width: f32) -> usize {
    let gap = size::card_gap();
    ((width + gap) / (size::panel() + gap)).floor().max(0.0) as usize
}

/// The columns of the page in `class`: wide ones [`columns`]; medium ones two (or more) where two
/// cards of [`size::panel()`] fit, else one; narrow ones one.
pub fn columns_for(class: SizeClass, width: f32, cards: usize) -> usize {
    match class {
        SizeClass::Wide => columns(width, cards),
        SizeClass::Medium if fit(width) >= 2 => columns(width, cards),
        _ => 1,
    }
}

/// A failed check of GitHub, at the top of the page: the attention mark, what happened and "Retry".
/// The catalog of the last check stays under it.
fn check_failed<'a>(code: &str) -> Element<'a, Message> {
    let what = if code == "offline" { tr("Couldn't reach GitHub.") } else { tr("GitHub's answer couldn't be read.") };
    let mark = torro_gui::icon::icon(Icon::TriangleAlert, torro_gui::icon::SIZE)
        .style(|t, _| iced::widget::svg::Style { color: Some(theme::palette(t).attention_text) });
    row![
        mark,
        typo::text(typo::body(), what).style(theme::text_2),
        widgets::text_button(Some(Icon::RefreshCw), tr("Retry"), style::button::ghost, Some(Message::Check)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center)
    .into()
}

/// The question card of the main application's update: "Torro Workplace 0.2.0 is available", what
/// happens, "What's new" and "Update…", which opens the window that asks (the service restarts).
fn main_update<'a>(main: &CatalogApp) -> Element<'a, Message> {
    let version = main.available.clone().unwrap_or_default();
    let title = trf("Torro Workplace {version} is available", &[("version", &version)]);
    let detail = tr(
        "The service restarts; your apps stay open and reconnect. Open windows use the new version once you close them.",
    );
    let actions = vec![
        widgets::PageAction::new(tr("What's new"), Some(Message::ReleaseNotes))
            .icon(Icon::ArrowUpRight)
            .style(style::button::ghost),
        widgets::PageAction::new(tr("Update…"), Some(Message::SelfUpdate)),
    ];
    widgets::question_card(Icon::Download, title, detail, actions)
}

/// One app's card: icon, name and question on top, its state, its sentence, a quiet line with its
/// version ("0.1.0 › 0.2.0"), its window class (what a window rule needs) and more, and its one
/// button with the menu "…". "Release notes" is in that menu: every card would point to the same
/// release.
fn card<'a>(
    app: &'a CatalogApp,
    catalog: &'a AppCatalog,
    settings: &Settings,
    job: Option<&'a AppJobInfo>,
    state: &'a State,
) -> Element<'a, Message> {
    let offer = offer(app, settings, job);
    let (question, sentence) = words(&app.window_class);
    let icon = app_id(app).map_or(Icon::LayoutGrid, shell::app_icon);
    let ident = row![
        widgets::source_tile(TileFace::Icon(icon), TileSize::Large, None),
        column![
            typo::text(typo::body_strong(), product_name(app)).wrapping(Wrapping::None),
            typo::text(typo::meta(), question).style(theme::text_3),
        ]
        .width(Fill),
    ]
    .spacing(space::m())
    .align_y(Alignment::Center);
    let mut parts: Vec<Element<'a, Message>> = vec![ident.into(), widgets::chip_row(vec![pill(app, &offer, settings)])];
    if !sentence.is_empty() {
        parts.push(typo::text(typo::meta(), sentence).style(theme::text_2).wrapping(Wrapping::WordOrGlyph).into());
    }
    let meta = typo::text(typo::meta(), format!("· {}", meta_line(app))).style(theme::text_3);
    let mut quiet = column![row![version_line(app), meta].spacing(space::xs()).align_y(Alignment::Center).wrap()]
        .spacing(space::xs());
    if let Some(needed) = needed_line(app, catalog) {
        quiet = quiet.push(typo::text(typo::meta(), needed).style(theme::text_3).wrapping(Wrapping::WordOrGlyph));
    }
    parts.push(quiet.into());
    parts.push(actions(app, &offer, state));
    widgets::card(parts, false, None)
}

/// "0.1.0 › 0.2.0" for an update (the chevron an icon, the new version in `text`), else the installed
/// version, or the release's for a missing app.
fn version_line<'a>(app: &CatalogApp) -> Element<'a, Message> {
    let figure = |text: String| typo::text(typo::meta().tabular(), text).style(theme::text_3).wrapping(Wrapping::None);
    match (&app.installed, &app.available) {
        (Some(installed), Some(available)) if installed != available && app.state != CatalogState::Installed => row![
            figure(installed.clone()),
            torro_gui::icon::icon(Icon::ChevronRight, torro_gui::icon::SIZE_SM)
                .style(|t, _| iced::widget::svg::Style { color: Some(theme::palette(t).text_3) }),
            typo::text(typo::meta().tabular(), available.clone()).wrapping(Wrapping::None),
        ]
        .spacing(space::xs())
        .align_y(Alignment::Center)
        .into(),
        (Some(installed), _) => figure(installed.clone()).into(),
        (None, Some(available)) => figure(available.clone()).into(),
        (None, None) => figure(String::new()).into(),
    }
}

/// The state of a card in words, the colour only in its dot.
fn pill<'a>(app: &CatalogApp, offer: &Offer<'_>, settings: &Settings) -> Element<'a, Message> {
    match offer {
        Offer::Running(job) => {
            let label = match job.kind {
                AppJobKind::Install => tr("Installing…"),
                AppJobKind::Update => tr("Updating…"),
                AppJobKind::Uninstall => tr("Uninstalling…"),
            };
            widgets::state_pill(Icon::RefreshCw, label)
        }
        Offer::Managed(managed, _) => {
            widgets::state_pill(Icon::Package, trf("Managed by {manager}", &[("manager", &managed.manager)]))
        }
        Offer::Update if turned_off(&app.window_class, settings) => {
            widgets::status_pill(Status::Inactive, tr("Turned off").to_string())
        }
        Offer::Update => widgets::status_pill(Status::Attention, tr("Update available").to_string()),
        Offer::TurnOn => widgets::status_pill(Status::Inactive, tr("Turned off").to_string()),
        Offer::Open => widgets::status_pill(Status::Ok, tr("Installed").to_string()),
        Offer::Install => widgets::status_pill(Status::Inactive, trc("app", "Available").to_string()),
        Offer::Unavailable => widgets::status_pill(Status::Inactive, tr("Not available for this system").to_string()),
    }
}

/// The quiet line under the sentence, after the version: the window class (for window rules) and the
/// download's size of a missing app.
fn meta_line(app: &CatalogApp) -> String {
    let mut parts = vec![app.window_class.clone()];
    if !is_installed(app)
        && let Some(bytes) = app.size
    {
        parts.push(units::format_bytes(bytes));
    }
    parts.join(" · ")
}

/// "Needed by Torro Mail, Torro App Feed" under the meta line of an app others bring along, on a
/// line of its own so the version line never breaks in the middle.
fn needed_line(app: &CatalogApp, catalog: &AppCatalog) -> Option<String> {
    let needed = needed_by(app, catalog);
    (!needed.is_empty()).then(|| trf("Needed by {apps}", &[("apps", &needed.join(", "))]))
}

/// The card's one button (or the command of its package manager, or how far its job is) and "…".
fn actions<'a>(app: &'a CatalogApp, offer: &Offer<'a>, state: &'a State) -> Element<'a, Message> {
    let class = &app.window_class;
    let button =
        |label: String, message: Option<Message>| widgets::text_button(None, label, style::button::secondary, message);
    let main: Element<'a, Message> = match offer {
        Offer::Running(job) => {
            let percent = trf("{n}%", &[("n", &job.percent)]);
            row![
                widgets::progress_bar(usize::from(job.percent), 100),
                typo::text(typo::meta().tabular(), percent).style(theme::text_3).wrapping(Wrapping::None),
                blank::horizontal(),
                widgets::text_button(None, tr("Show"), style::button::ghost, Some(Message::ShowJob(job.id.clone()))),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .width(Fill)
            .into()
        }
        Offer::Managed(_, Some(command)) if *command == "https://github.com/mahype/TorroWorkplace/releases/latest" => {
            let mut actions = row![].spacing(space::s());
            if is_installed(app) {
                actions = actions.push(button(tr("Open").to_string(), Some(Message::Open(class.clone()))));
            }
            actions.push(button(tr("Downloads").to_string(), Some(Message::Downloads))).into()
        }
        Offer::Managed(_, Some(command)) => command_field(command),
        Offer::Managed(_, None) => button(tr("Open").to_string(), Some(Message::Open(class.clone()))),
        Offer::Update => {
            let version = app.available.clone().unwrap_or_default();
            button(trf("Update to {version}", &[("version", &version)]), Some(Message::Update(class.clone())))
        }
        Offer::TurnOn => {
            let component =
                component_of(class).or_else(|| (class == "torro-inbox-notifications").then_some(Component::Inbox));
            button(tr("Turn on").to_string(), component.map(|component| Message::Switch(component, true)))
        }
        Offer::Open => button(tr("Open").to_string(), Some(Message::Open(class.clone()))),
        Offer::Install => {
            let label = match app.size {
                Some(bytes) => trf("Install · {size}", &[("size", &units::format_bytes(bytes))]),
                None => tr("Install").to_string(),
            };
            button(label, Some(Message::Install(class.clone())))
        }
        Offer::Unavailable => button(tr("Install").to_string(), None),
    };
    let more = widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::ToggleMenu(class.clone())));
    let more = match state.menu.as_ref() == Some(class) {
        true => widgets::anchored(&state.anchor, more),
        false => more,
    };
    let main: Element<'a, Message> = match offer {
        Offer::Running(_) | Offer::Managed(_, Some(_)) => container(main).width(Fill).into(),
        _ => row![main, blank::horizontal()].width(Fill).into(),
    };
    row![main, more].spacing(space::s()).align_y(Alignment::Center).into()
}

/// The command of a package manager in a field that can be selected but not changed, and "Copy".
fn command_field<'a>(command: &'a str) -> Element<'a, Message> {
    let field = text_input("", command)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().tabular().font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        // Without an edit handler the field cannot take the focus, so the text could not be selected.
        .on_input(|_| Message::Keep);
    let copy = widgets::icon_button(Icon::Copy, tr("Copy command"), Some(Message::Copy(command.to_string())));
    row![field, copy].spacing(space::s()).align_y(Alignment::Center).width(Fill).into()
}

/// The footer: when the catalog was last checked and where from, calm also after a failed check
/// (that one is said at the top, [`check_failed`]), or "Checking…" while it is asked.
fn footer<'a>(state: &'a State, catalog: &'a AppCatalog) -> Element<'a, Message> {
    if state.checking {
        return widgets::refresh_note(tr("Checking for updates…"));
    }
    let quiet = |text: String| typo::text(typo::meta(), text).style(theme::text_3);
    let checked = catalog.checked.map_or_else(|| tr("Not checked yet").to_string(), |at| checked_words(at, Utc::now()));
    let text = format!("{checked} · {}", tr("from the releases on GitHub"));
    let text: Element<'a, Message> = match catalog.checked {
        Some(at) => widgets::with_tooltip(quiet(text), format_exact(at)),
        None => quiet(text).into(),
    };
    let which = if catalog.error.is_some() { Icon::Clock } else { Icon::Check };
    let mark = torro_gui::icon::icon(which, torro_gui::icon::SIZE_SM)
        .style(|t, _| iced::widget::svg::Style { color: Some(theme::palette(t).text_3) });
    row![mark, text].spacing(space::s()).align_y(Alignment::Center).into()
}

/// "Checked just now" within a minute, else "Checked Today 15:39" ([`format_moment`]).
pub fn checked_words(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    if now.signed_duration_since(at) < chrono::Duration::minutes(1) {
        return tr("Checked just now").to_string();
    }
    trf("Checked {moment}", &[("moment", &format_moment(at, now))])
}

/// The menu "…" of the open card, under its button: Open · Release notes · Turn off (Turn on) ·
/// Show in Finder · ── · Uninstall… (for a package manager's app its command to remove it).
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let class = state.menu.as_ref()?;
    let catalog = data.app_catalog.catalog.listed()?;
    let app = entry_of(catalog, class)?;
    let menu = widgets::menu(menu_sections(app, catalog, &data.settings));
    Some(widgets::floating(&state.anchor, Placement::Below(Align::End), menu, Message::CloseMenu))
}

/// The entries of an app's menu "…".
pub fn menu_sections<'a>(app: &CatalogApp, catalog: &AppCatalog, settings: &Settings) -> Vec<MenuSection<'a, Message>> {
    let class = app.window_class.clone();
    let installed = is_installed(app);
    let mut first = Vec::new();
    if installed {
        first.push(MenuItem::new(Icon::ArrowUpRight, tr("Open"), Message::Open(class.clone())));
    }
    if catalog.release_url.is_some() {
        first.push(MenuItem::new(Icon::ExternalLink, tr("Release notes"), Message::ReleaseNotes));
    }
    if installed && let Some(component) = component_of(&class) {
        first.push(match settings.component_on(component) {
            true => MenuItem::new(Icon::EyeOff, tr("Turn off"), Message::Switch(component, false)),
            false => MenuItem::new(Icon::Eye, tr("Turn on"), Message::Switch(component, true)),
        });
    }
    if installed {
        first.push(MenuItem::new(Icon::Folder, reveal_label(), Message::Reveal(class.clone())));
    }
    let mut sections = vec![MenuSection::new(first)];
    let last = match (&app.managed, installed) {
        (Some(managed), true) => {
            Some(MenuItem::new(Icon::Copy, tr("Copy uninstall command"), Message::Copy(managed.uninstall.clone())))
        }
        (None, true) if needed_by(app, catalog).is_empty() => {
            Some(MenuItem::new(Icon::Trash2, tr("Uninstall…"), Message::Uninstall(class)))
        }
        _ => None,
    };
    sections.extend(last.map(|item| MenuSection::new(vec![item])));
    sections.retain(|section| !section.items.is_empty());
    sections
}

/// "Show in Finder" on macOS, "Show in Explorer" on Windows, "Show in folder" elsewhere, where the
/// file manager has no one name.
pub fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        tr("Show in Finder")
    } else if cfg!(target_os = "windows") {
        tr("Show in Explorer")
    } else {
        tr("Show in folder")
    }
}

/// Shows where `app` lies in the system's file manager: `path` is where the core found it
/// ([`CatalogApp::path`]: its bundle on macOS, its program elsewhere), so the menu shows what the card
/// says is installed. A local action without a request, as opening a folder of Appearance is
/// (`docs/api.md`, footnote 9): nothing is read or changed. Returns what went wrong, for the notice.
pub fn reveal(app: AppId, path: Option<&str>) -> Result<(), String> {
    let missing =
        || trf("Couldn't find {app} on this computer.", &[("app", &format!("Torro {}", shell::app_name(app)))]);
    let program = path.map(std::path::PathBuf::from).filter(|path| path.is_absolute() && path.exists());
    let program = program.ok_or_else(missing)?;
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg("-R").arg(&program);
        command
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = {
        #[cfg(target_os = "windows")]
        let command = {
            let mut command = std::process::Command::new("explorer");
            command.arg(format!("/select,{}", program.display()));
            command
        };
        #[cfg(not(target_os = "windows"))]
        let command = {
            let mut command = std::process::Command::new("xdg-open");
            command.arg(program.parent().unwrap_or(std::path::Path::new("/")));
            command
        };
        command
    };
    let mut child = command.spawn().map_err(|err| err.to_string())?;
    // Waited for on a thread of its own, so it leaves no zombie behind.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The demo's catalog (`--demo --section apps`): checked just now, Calendar and Mail with an update,
/// Tasks and the App Feed up to date, the Notification Hub from a package manager, the Assistant
/// missing, and the main application up to date.
pub fn demo_catalog(now: DateTime<Utc>) -> AppCatalog {
    let entry = |app: AppId, installed: Option<&str>, available: &str, state: CatalogState| CatalogApp {
        window_class: app.window_class.into(),
        program: app.program_file().into(),
        name: app.display_name.into(),
        installed: installed.map(String::from),
        available: Some(available.into()),
        state,
        managed: None,
        size: Some(25_795_000),
        requires: Vec::new(),
        path: installed.map(|_| format!("/Applications/{}.app", app.display_name)),
    };
    let hub = CatalogApp {
        managed: Some(ManagedBy {
            manager: "AUR".into(),
            install: "yay -S torroinbox-bin".into(),
            update: "yay -Syu".into(),
            uninstall: "sudo pacman -R torroinbox-bin".into(),
        }),
        ..entry(AppId::INBOX, Some("0.1.0"), "0.2.0", CatalogState::Managed)
    };
    let mail = CatalogApp {
        requires: vec![AppId::INBOX.window_class.into()],
        ..entry(AppId::INBOX_MAIL, Some("0.1.0"), "0.2.0", CatalogState::Update)
    };
    let feed = CatalogApp {
        requires: vec![AppId::INBOX.window_class.into()],
        ..entry(AppId::INBOX_NOTIFICATIONS, Some("0.2.0"), "0.2.0", CatalogState::Installed)
    };
    AppCatalog {
        checked: Some(now),
        latest: Some("0.2.0".into()),
        release_url: Some("https://github.com/mahype/TorroWorkplace/releases/tag/v0.2.0".into()),
        error: None,
        main: entry(AppId::WORKPLACE, Some("0.2.0"), "0.2.0", CatalogState::Installed),
        apps: vec![
            entry(AppId::CALENDAR, Some("0.1.0"), "0.2.0", CatalogState::Update),
            entry(AppId::TASKS, Some("0.2.0"), "0.2.0", CatalogState::Installed),
            mail,
            hub,
            feed,
            entry(AppId::ASSISTANT, None, "0.2.0", CatalogState::Available),
        ],
        job: None,
    }
}

/// Native installers and the older package managers (`--open apps-managed`), with the existing
/// cards and copyable commands. The main application is visible too when externally managed.
pub fn demo_managed(catalog: &mut AppCatalog) {
    for (index, app) in std::iter::once(&mut catalog.main).chain(&mut catalog.apps).enumerate() {
        let package = format!("{}-bin", app.program);
        let (manager, uninstall) = match index {
            0 => ("Windows Setup", "control appwiz.cpl".to_string()),
            1 => ("DEB", format!("sudo apt remove {package}")),
            2 => ("RPM", format!("sudo dnf remove {package}")),
            3 => ("pacman", format!("sudo pacman -R {package}")),
            4 => ("AUR", format!("sudo pacman -R {package}")),
            5 => ("Homebrew", format!("brew uninstall mahype/torro/{}", app.program)),
            _ => ("Windows Setup", "control appwiz.cpl".to_string()),
        };
        let downloads = "https://github.com/mahype/TorroWorkplace/releases/latest";
        let (install, update) = match manager {
            "AUR" => (format!("yay -S {package}"), "yay -Syu".to_string()),
            "Homebrew" => (
                format!("brew install mahype/torro/{}", app.program),
                format!("brew upgrade mahype/torro/{}", app.program),
            ),
            _ => (downloads.to_string(), downloads.to_string()),
        };
        app.managed = Some(ManagedBy { manager: manager.into(), install, update, uninstall });
        app.state = CatalogState::Managed;
    }
}

/// The demo's job: the Assistant being installed, at its step "Install" (62 %).
pub fn demo_job(done: bool) -> AppJobInfo {
    let app = AppId::ASSISTANT.window_class.to_string();
    let step = |step: AppStep, status: StepStatus, detail: Option<&str>| AppJobStep {
        app: app.clone(),
        step,
        status,
        detail: detail.map(String::from),
    };
    let (install, connect) =
        if done { (StepStatus::Done, StepStatus::Done) } else { (StepStatus::Now, StepStatus::Todo) };
    let folder = if cfg!(target_os = "macos") { "/Applications" } else { "~/.local/bin" };
    let signed = if cfg!(target_os = "macos") { "Developer ID" } else { "SHA-256" };
    AppJobInfo {
        id: "demo-install".into(),
        apps: vec![app.clone()],
        kind: AppJobKind::Install,
        steps: vec![
            step(AppStep::Download, StepStatus::Done, Some("25795000")),
            step(AppStep::Verify, StepStatus::Done, Some(signed)),
            step(AppStep::Install, install, Some(folder)),
            step(AppStep::Connect, connect, None),
        ],
        percent: if done { 100 } else { 62 },
        state: if done { AppJobState::Done } else { AppJobState::Running },
        error: None,
        restart: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_with(catalog: AppCatalog) -> Data {
        Data { app_catalog: Loaded { catalog: Catalog::Listed(Box::new(catalog)), job: None }, ..Data::default() }
    }

    fn catalog() -> AppCatalog {
        demo_catalog(Utc::now())
    }

    /// Opening the section asks the core to check GitHub (decision 2); the footer says so until the
    /// answer came, which replaces the catalog.
    #[test]
    fn opening_the_section_checks_for_updates() {
        let mut state = State { menu: Some("torro-calendar".into()), ..State::default() };
        assert!(matches!(opened(&mut state), Action::CheckApps));
        assert!(state.checking && state.menu.is_none());
        let mut loaded = Loaded::default();
        checked(&mut state, &mut loaded, &Ok(Response::AppCatalog { catalog: catalog() }));
        assert!(!state.checking);
        assert_eq!(loaded.catalog.listed().map(|catalog| catalog.apps.len()), Some(6));
        // A refusal keeps what was there.
        let mut state = State { checking: true, ..State::default() };
        checked(&mut state, &mut loaded, &Err("no answer".into()));
        assert!(!state.checking && loaded.catalog.listed().is_some());
    }

    #[test]
    fn managed_main_app_has_an_openable_menu_and_never_a_catalog_update() {
        let mut catalog = catalog();
        demo_managed(&mut catalog);
        assert_eq!(catalog.main.state, CatalogState::Managed);
        assert!(updatable(&catalog).is_empty());
        assert_eq!(summary(&catalog), trf("{n} installed", &[("n", &6)]));
        let class = catalog.main.window_class.clone();
        let data = data_with(catalog);
        let mut state = State::default();
        let _ = update(&mut state, Message::ToggleMenu(class), &data);
        assert!(overlay(&state, &data).is_some());
        assert!(
            matches!(update(&mut state, Message::Downloads, &data), Action::OpenUrl(ref url) if url == "https://github.com/mahype/TorroWorkplace/releases/latest")
        );
    }

    /// Each button of a card is one request: install and update start a job, "Update all" takes
    /// every app with an update but never the main application, whose update and every uninstall
    /// open the window that asks first.
    #[test]
    fn the_buttons_send_the_catalog_requests() {
        let data = data_with(catalog());
        let mut state = State::default();
        let mut send = |message| update(&mut state, message, &data);
        assert!(matches!(
            send(Message::Install("torro-assistant".into())),
            Action::StartAppJob(Request::InstallApp { ref app }) if app == "torro-assistant"
        ));
        assert!(matches!(
            send(Message::Update("torro-calendar".into())),
            Action::StartAppJob(Request::UpdateApps { ref apps }) if apps == &["torro-calendar"]
        ));
        assert!(matches!(
            send(Message::UpdateAll),
            Action::StartAppJob(Request::UpdateApps { ref apps }) if apps == &["torro-calendar", "torro-inbox-mail"]
        ));
        assert!(matches!(send(Message::Update(MAIN_APP.into())), Action::AppWindow(Target::SelfUpdate)));
        assert!(matches!(send(Message::SelfUpdate), Action::AppWindow(Target::SelfUpdate)));
        assert!(matches!(
            send(Message::Uninstall("torro-tasks".into())),
            Action::AppWindow(Target::Uninstall(ref app)) if app == "torro-tasks"
        ));
        assert!(matches!(
            send(Message::Open("torro-tasks".into())),
            Action::Send(Request::OpenApp { ref app, target: None }) if app == "torro-tasks"
        ));
        assert!(matches!(send(Message::ReleaseNotes), Action::OpenUrl(ref url) if url.contains("/releases/")));
        assert!(matches!(send(Message::ShowJob("j1".into())), Action::AppWindow(Target::Follow(ref id)) if id == "j1"));
        assert!(matches!(send(Message::Copy("yay -Syu".into())), Action::Copy(ref text) if text == "yay -Syu"));
        assert!(
            matches!(send(Message::Reveal("torro-tasks".into())), Action::Reveal(app, Some(_)) if app == AppId::TASKS)
        );
        let Action::Send(Request::SetSetting { key, value }) = send(Message::Switch(Component::Calendar, false)) else {
            panic!("expected the setting");
        };
        assert_eq!(key, SettingKey::Component);
        assert_eq!(value, serde_json::json!({"component": "calendar", "enabled": false}));
        assert!(matches!(send(Message::Check), Action::CheckApps));
        // Nothing to update: "Update all" sends nothing.
        let mut none = catalog();
        none.apps.iter_mut().for_each(|app| app.state = CatalogState::Installed);
        assert!(matches!(update(&mut State::default(), Message::UpdateAll, &data_with(none)), Action::None));
    }

    /// The menu opens at one card at a time, Escape and the entries close it; the search filters by
    /// name, question and sentence.
    #[test]
    fn the_menu_and_the_search_follow_the_clicks() {
        let data = data_with(catalog());
        let mut state = State::default();
        let _ = update(&mut state, Message::ToggleMenu("torro-tasks".into()), &data);
        assert_eq!(state.menu.as_deref(), Some("torro-tasks"));
        assert!(matches!(escape(&state), Some(Message::CloseMenu)));
        assert!(overlay(&state, &data).is_some());
        let _ = update(&mut state, Message::ToggleMenu("torro-tasks".into()), &data);
        assert!(state.menu.is_none() && escape(&state).is_none());
        let _ = update(&mut state, Message::ToggleMenu("torro-tasks".into()), &data);
        let _ = update(&mut state, Message::Open("torro-tasks".into()), &data);
        assert!(state.menu.is_none(), "an entry closes the menu");

        let listed = data.app_catalog.catalog.listed().expect("listed");
        let found = |query: &str| listed.apps.iter().filter(|app| matches(app, query)).count();
        assert_eq!(found(""), 6);
        assert_eq!(found("CALDAV"), 1, "without regard to case, in the sentence");
        assert_eq!(found(&tr("What do I need to do?").to_lowercase()), 1, "the question");
        assert_eq!(found("zzz"), 0);
        let _ = update(&mut state, Message::Query("mail".into()), &data);
        drop(view(&state, &data, SizeClass::Wide, 1200.0));
    }

    /// Each card offers one thing: the command for a package manager's app, "Update to" for an
    /// update, "Turn on" for an app turned off, "Open", "Install", and how far a job is while it runs.
    #[test]
    fn a_card_offers_what_its_state_allows() {
        let catalog = catalog();
        let settings = Settings::default();
        let of = |class: &str| catalog.apps.iter().find(|app| app.window_class == class).expect("listed");
        assert_eq!(offer(of("torro-calendar"), &settings, None), Offer::Update);
        assert_eq!(offer(of("torro-tasks"), &settings, None), Offer::Open);
        assert_eq!(offer(of("torro-assistant"), &settings, None), Offer::Install);
        let Offer::Managed(managed, command) = offer(of("torro-inbox"), &settings, None) else { panic!("managed") };
        assert_eq!((managed.manager.as_str(), command), ("AUR", Some("yay -Syu")), "an update: its command");
        let off = Settings {
            components_off: [Component::Tasks, Component::Inbox].into_iter().collect(),
            ..Settings::default()
        };
        assert_eq!(offer(of("torro-tasks"), &off, None), Offer::TurnOn);
        assert_eq!(offer(of("torro-inbox-notifications"), &off, None), Offer::TurnOn, "the App Feed follows the hub");
        let job = demo_job(false);
        assert_eq!(offer(of("torro-assistant"), &settings, Some(&job)), Offer::Running(&job));
        let done = demo_job(true);
        assert_eq!(offer(of("torro-assistant"), &settings, Some(&done)), Offer::Install, "a finished job lets go");
        let missing = CatalogApp { installed: None, state: CatalogState::Unavailable, ..of("torro-tasks").clone() };
        assert_eq!(offer(&missing, &settings, None), Offer::Unavailable);
    }

    /// The menu after the target picture: Open, Release notes, Turn off, Show in Finder, then
    /// Uninstall…; the hub that Mail needs cannot be uninstalled, a package manager's app gives its
    /// command, a missing app has only its release notes.
    #[test]
    fn the_menu_has_what_the_app_allows() {
        let catalog = catalog();
        let settings = Settings::default();
        let labels = |class: &str| -> Vec<String> {
            let app = catalog.apps.iter().find(|app| app.window_class == class).expect("listed");
            menu_sections(app, &catalog, &settings)
                .into_iter()
                .flat_map(|section| section.items)
                .map(|item| item.label.to_string())
                .collect()
        };
        assert_eq!(
            labels("torro-tasks"),
            [tr("Open"), tr("Release notes"), tr("Turn off"), reveal_label(), tr("Uninstall…")]
        );
        assert_eq!(
            labels("torro-inbox-notifications"),
            [tr("Open"), tr("Release notes"), reveal_label(), tr("Uninstall…")]
        );
        assert_eq!(
            labels("torro-inbox"),
            [tr("Open"), tr("Release notes"), tr("Turn off"), reveal_label(), tr("Copy uninstall command")]
        );
        assert_eq!(labels("torro-assistant"), [tr("Release notes")]);
        let hub = catalog.apps.iter().find(|app| app.window_class == "torro-inbox").expect("hub");
        let alone = CatalogApp { managed: None, state: CatalogState::Installed, ..hub.clone() };
        let names = needed_by(&alone, &catalog);
        assert_eq!(names.len(), 2, "Mail and the App Feed need it: {names:?}");
        let sections = menu_sections(&alone, &catalog, &settings);
        assert!(sections.iter().flat_map(|s| &s.items).all(|item| item.label != tr("Uninstall…")));
        let off = Settings { components_off: [Component::Tasks].into_iter().collect(), ..Settings::default() };
        let tasks = catalog.apps.iter().find(|app| app.window_class == "torro-tasks").expect("tasks");
        let items: Vec<String> = menu_sections(tasks, &catalog, &off)
            .into_iter()
            .flat_map(|s| s.items)
            .map(|i| i.label.to_string())
            .collect();
        assert!(items.contains(&tr("Turn on").to_string()));
    }

    /// The header counts what is installed and waits; "Update all" is there only with updates, the
    /// sidebar counts the main application's update too.
    #[test]
    fn the_header_and_the_sidebar_count_the_updates() {
        let mut catalog = catalog();
        assert_eq!(
            summary(&catalog),
            format!("{} · {}", trf("{n} installed", &[("n", &5)]), trf("{n} updates", &[("n", &2)]))
        );
        assert_eq!(updates(&data_with(catalog.clone())), 2);
        catalog.main.state = CatalogState::Update;
        // The sidebar counts what "Update all" takes; the main application's update gives the dot.
        assert_eq!(updates(&data_with(catalog.clone())), 2, "as many as Update all");
        assert!(waiting(&data_with(catalog.clone())));
        let mut only_main = catalog.clone();
        only_main.apps.iter_mut().for_each(|app| app.state = CatalogState::Installed);
        assert_eq!(updates(&data_with(only_main.clone())), 0);
        assert!(waiting(&data_with(only_main)), "the dot alone");
        let data = data_with(catalog.clone());
        let state = State::default();
        assert_eq!(header(&state, &data, SizeClass::Wide).actions.len(), 3, "check, search, update all");
        assert_eq!(header(&state, &data, SizeClass::Narrow).actions.len(), 2, "the search moves over the cards");
        catalog.apps.iter_mut().for_each(|app| app.state = CatalogState::Installed);
        assert_eq!(header(&state, &data_with(catalog), SizeClass::Wide).actions.len(), 2, "no updates, no button");
        assert_eq!(updates(&Data::default()), 0);
    }

    /// Wide pages get two cards a row and more as they grow; never more columns than cards.
    #[test]
    fn the_grid_grows_with_the_width() {
        assert_eq!(columns(700.0, 6), 2);
        assert_eq!(columns(3.0 * size::panel() + 2.0 * size::card_gap(), 6), 3);
        assert_eq!(columns(5000.0, 3), 3);
        assert_eq!(columns(5000.0, 0), 1);
        // Medium: two columns once two cards fit, else one.
        let two = 2.0 * size::panel() + size::card_gap();
        assert_eq!(columns_for(SizeClass::Medium, two, 6), 2);
        assert_eq!(columns_for(SizeClass::Medium, two - 1.0, 6), 1);
        assert_eq!(columns_for(SizeClass::Narrow, 5000.0, 6), 1);
        assert_eq!(columns_for(SizeClass::Wide, 700.0, 6), 2);
    }

    /// A refused job says why in words for people; the job's id comes back otherwise.
    #[test]
    fn a_refused_job_says_why() {
        let job = demo_job(false);
        assert_eq!(started(&Ok(Response::AppJob { job: job.clone() })), Ok(job));
        let busy = Ok(Response::Error { code: "busy".into(), message: "an app job runs".into() });
        assert_eq!(
            started(&busy),
            Err(tr("Another app is being installed or updated. Try again when it is done.").into())
        );
        let offline = Ok(Response::Error { code: "offline".into(), message: "x".into() });
        assert_eq!(started(&offline), Err(tr("Couldn't reach GitHub.").into()));
        assert!(started(&Err("gone".into())).is_err());
        // A conflict says the core's reason, which names the next step.
        let needed = Ok(Response::Error { code: "conflict".into(), message: "Needed by Mail.".into() });
        assert_eq!(started(&needed), Err("Needed by Mail.".into()));
    }

    /// Review of #411: a job that brings the Notification Hub along touches it too, in step order,
    /// although it lists only Mail.
    #[test]
    fn a_job_touches_what_it_brings_along() {
        let step = |app: &str, step| AppJobStep { app: app.into(), step, status: StepStatus::Todo, detail: None };
        let job = AppJobInfo {
            apps: vec!["torro-inbox-mail".into()],
            steps: vec![
                step("torro-inbox", AppStep::Download),
                step("torro-inbox", AppStep::Install),
                step("torro-inbox-mail", AppStep::Download),
                step("torro-inbox-mail", AppStep::Connect),
            ],
            ..demo_job(false)
        };
        assert_eq!(involved(&job), ["torro-inbox", "torro-inbox-mail"]);
    }

    /// The footer says when the catalog was checked, "just now" within a minute.
    #[test]
    fn the_footer_says_when_it_was_checked() {
        let now = Utc::now();
        assert_eq!(checked_words(now - chrono::Duration::seconds(20), now), tr("Checked just now"));
        let earlier = now - chrono::Duration::days(3);
        assert_eq!(checked_words(earlier, now), trf("Checked {moment}", &[("moment", &format_moment(earlier, now))]));
    }

    /// Every state of the page builds: loading, failed, offline, a running job, every size class.
    #[test]
    fn every_state_of_the_page_builds() {
        let state = State::default();
        for catalog in [Catalog::Loading, Catalog::Failed("x".into())] {
            let data = Data { app_catalog: Loaded { catalog, job: None }, ..Data::default() };
            drop(view(&state, &data, SizeClass::Wide, 1200.0));
        }
        let mut offline = catalog();
        offline.error = Some("offline".into());
        offline.main.state = CatalogState::Update;
        let mut data = data_with(offline);
        data.app_catalog.job = Some(demo_job(false));
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            drop(view(&state, &data, class, 700.0));
            drop(header(&state, &data, class));
        }
        let checking = State { checking: true, ..State::default() };
        drop(view(&checking, &data, SizeClass::Wide, 1200.0));
        assert!(job_runs(&data));
    }

    /// Every app of the suite has its question and sentence, and its card its name.
    #[test]
    fn every_app_has_its_words() {
        for app in AppId::ALL.iter().filter(|app| **app != AppId::WORKPLACE) {
            let (question, sentence) = words(app.window_class);
            assert!(!question.is_empty() && !sentence.is_empty(), "{}", app.window_class);
        }
        let catalog = catalog();
        let names: Vec<String> = catalog.apps.iter().map(product_name).collect();
        assert_eq!(names[0], format!("Torro {}", tr("Calendar")));
        assert_eq!(product_name(&catalog.main), "Torro Workplace");
    }
}
