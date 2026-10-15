//! The pages of the setup's steps (task W.4: Welcome, Components and Done; the steps of tasks
//! W.5–W.8 follow here): each with a headline, one sentence in `text_2` and settings groups, plus
//! what the footer says and what its primary button is called.

use iced::widget::{Space, column, toggler};
use iced::{Element, Fill};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, Language};
use torro_gui::icon::Icon;
use torro_gui::layout;
use torro_gui::theme::{self, Appearance, space};
use torro_gui::widgets::{self, TileFace, TileSize};
use torro_gui::{style, typo};

use torro_core::account::Secret;
use torro_ipc::AccountInfo;
use torro_pim::{AccountState, Collection};

use super::{Data, Message, State, Step, app_of};
use crate::ui::{chevron, connect, pressable, setting_row};

/// The page of the current step.
pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    match state.step {
        Step::Welcome => welcome(state),
        Step::Nextcloud => nextcloud(state, data),
        Step::Components => components(state, data),
        Step::Done => done(state, data),
        // Not listed until their tasks build them ([`Step::built`]).
        Step::Calendar => collections(state, data, Kind::Calendars),
        Step::Tasks => collections(state, data, Kind::Lists),
        Step::Mail | Step::Inbox | Step::Ai => Space::new().into(),
    }
}

/// What the footer says at its left on the current step, if anything.
pub fn status(state: &State, data: &Data) -> Option<String> {
    match state.step {
        // The page says it already ("Waiting for the browser …").
        Step::Nextcloud if state.nextcloud.running() => None,
        Step::Nextcloud => super::nextcloud_account(state, data)
            .filter(|_| !state.asks_address(data))
            .map(|account| trf("Signed in as {account}", &[("account", &account.label)])),
        Step::Calendar | Step::Tasks if state.github.checking() => Some(tr("Checking the connection…").to_string()),
        Step::Calendar | Step::Tasks if state.caldav.as_ref().is_some_and(|form| form.connecting()) => {
            Some(tr("Signing in…").to_string())
        }
        Step::Calendar | Step::Tasks if state.caldav.is_none() && state.connected.is_some() => {
            state.connected.as_ref().map(|account| trf("{account} is connected", &[("account", account)]))
        }
        Step::Calendar => {
            let n = shown_count(state, data, Kind::Calendars);
            Some(trf("{n} calendars shown", &[("n", &n)]))
        }
        Step::Tasks => {
            let n = shown_count(state, data, Kind::Lists);
            Some(trf("{n} lists shown", &[("n", &n)]))
        }
        Step::Components => {
            let on = Component::ALL.len() - state.off.len();
            Some(trf("{on} of {all} on", &[("on", &on), ("all", &Component::ALL.len())]))
        }
        _ => None,
    }
}

/// The word of the primary button on the current step, and whether it can be pressed.
pub fn primary(state: &State, data: &Data) -> (&'static str, bool) {
    match state.step {
        Step::Welcome => (tr("Get started"), true),
        Step::Nextcloud if state.nextcloud.running() => (tr("Connect"), false),
        Step::Nextcloud if state.asks_address(data) => (tr("Connect"), state.nextcloud.can_start()),
        Step::Calendar | Step::Tasks if state.in_form() => {
            let ready = match &state.caldav {
                Some(form) => form.can_connect(),
                None => state.github_token && state.github.has_token() && !state.github.checking(),
            };
            (tr("Connect"), ready)
        }
        Step::Done => (tr("Finish"), true),
        _ => (tr("Continue"), true),
    }
}

/// The secondary button left of the primary one, if the step has one: "Skip" where a step can be
/// left out, "Cancel" while the Nextcloud step waits or adds another account. None on the welcome,
/// the choice of components and the end, where there is nothing to skip.
pub fn skip(state: &State, data: &Data) -> Option<(&'static str, Message)> {
    match state.step {
        Step::Welcome | Step::Components | Step::Done => None,
        Step::Nextcloud if state.nextcloud.running() || state.adding => Some((tr("Cancel"), Message::Cancel)),
        Step::Nextcloud if state.asks_address(data) => Some((tr("I don't use Nextcloud"), Message::Skip)),
        Step::Nextcloud => None,
        Step::Calendar | Step::Tasks if state.in_form() => Some((tr("Cancel"), Message::Cancel)),
        _ => Some((tr("Skip"), Message::Skip)),
    }
}

/// The headline and the sentence under it.
fn title<'a>(headline: &'a str, lead: &'a str) -> Element<'a, Message> {
    column![typo::text(typo::headline(), headline), typo::text(typo::body(), lead).style(theme::text_2)]
        .spacing(space::xs())
        .into()
}

/// The parts of a page with the gap between groups.
fn page<'a>(parts: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    column(parts).spacing(layout::section_gap()).width(Fill).into()
}

fn welcome(state: &State) -> Element<'_, Message> {
    let languages = [Language::System, Language::English, Language::German];
    let language = widgets::segmented(
        [
            (tr("System"), Message::Language(Language::System)),
            // In their own names on purpose: someone who cannot read the current language must
            // still find theirs.
            ("English", Message::Language(Language::English)),
            ("Deutsch", Message::Language(Language::German)),
        ],
        languages.iter().position(|language| *language == state.language).unwrap_or_default(),
    );
    let looks = [Appearance::System, Appearance::Light, Appearance::Dark];
    let appearance = widgets::segmented(
        [
            (tr("System"), Message::Appearance(Appearance::System)),
            (tr("Light"), Message::Appearance(Appearance::Light)),
            (tr("Dark"), Message::Appearance(Appearance::Dark)),
        ],
        looks.iter().position(|look| *look == state.appearance).unwrap_or_default(),
    );
    let preferences = widgets::group(
        tr("Preferences"),
        vec![setting_row(None, tr("Language"), None, language), setting_row(None, tr("Appearance"), None, appearance)],
        Some(tr("Applies to all apps of the suite. A new language shows when an app starts again.").to_string()),
    );
    page(vec![
        title(
            tr("Set up Torro Workplace"),
            tr(
                "Connect your accounts, choose what you need, and add calendars, mail and AI. Every step can be skipped and changed later in Settings.",
            ),
        ),
        preferences,
    ])
}

// ----- Nextcloud (task W.5) -------------------------------------------------------------------------

fn nextcloud<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    if state.nextcloud.running() {
        return page(vec![
            title(tr("Sign in to Nextcloud"), tr("Your browser opened Nextcloud's sign-in page.")),
            connect::nextcloud::waiting(data, Message::OpenLoginPage, |_| Message::LoginLinkEdited),
        ]);
    }
    match super::nextcloud_account(state, data).filter(|_| !state.adding) {
        Some(account) => nextcloud_services(account),
        None => nextcloud_address(state),
    }
}

/// The address form: where the Nextcloud is; the browser does the rest.
fn nextcloud_address(state: &State) -> Element<'_, Message> {
    let field = column![
        typo::text(typo::caption(), tr("Server address")).style(theme::text_2),
        connect::nextcloud::address(&state.nextcloud, Message::ServerChanged, Message::Next),
        typo::text(
            typo::meta(),
            tr("Your browser opens to sign in. Torro Workplace keeps an app password in the system keychain.")
        )
        .style(theme::text_2),
    ]
    .spacing(space::xs());
    let lead = match state.adding {
        true => tr("Connect one more Nextcloud, for example for work."),
        false => tr(
            "Calendars, tasks, notifications and Talk come from your Nextcloud. Without one, add other accounts in the next steps.",
        ),
    };
    page(vec![title(tr("Connect your Nextcloud"), lead), field.into()])
}

/// The services of the server, as the account's last sync found them (task W.1), and the way to
/// add another Nextcloud.
fn nextcloud_services(account: &AccountInfo) -> Element<'_, Message> {
    use torro_ipc::ServerService as S;
    let rows: Vec<Element<'_, Message>> = match &account.services {
        None => vec![setting_row(None, tr("Reading what your server offers…"), None, Space::new())],
        Some(services) => [
            (S::Calendars, Icon::Calendar, tr("Calendars"), tr("For the calendar")),
            (S::Tasks, Icon::ListChecks, tr("Tasks"), tr("For the task lists")),
            (S::Notifications, Icon::Bell, tr("Notifications"), tr("For the Notification Hub")),
            (S::Talk, Icon::MessageCircle, "Talk", tr("Chats and mentions for the Notification Hub")),
            (S::Push, Icon::RadioTower, tr("Push"), tr("News arrive at once, without asking")),
            (S::Deck, Icon::LayoutGrid, "Deck", tr("Boards and cards, later in the suite")),
            (S::Notes, Icon::AlignLeft, tr("Notes"), tr("Notes, later in the suite")),
        ]
        .into_iter()
        .map(|(service, symbol, name, purpose)| {
            let there = services.contains(&service);
            let tile = widgets::source_tile(TileFace::Icon(symbol), TileSize::Medium, None);
            let detail = if there { purpose } else { tr("Not on this server") };
            let pill = match there {
                true => widgets::state_pill(Icon::CircleCheck, tr("Available")),
                false => widgets::state_pill(Icon::CircleDashed, tr("Missing")),
            };
            setting_row(Some(tile), name, Some(detail.to_string()), pill)
        })
        .collect(),
    };
    let host = match &account.state {
        AccountState::Connected { server, .. } => server.split_once("://").map_or(server.as_str(), |(_, rest)| rest),
        _ => account.label.as_str(),
    };
    let services = widgets::group(
        host.trim_end_matches('/').to_string(),
        rows,
        Some(
            tr("Calendars and tasks come over CalDAV, which every Nextcloud has; the Calendar and Tasks apps on the server are only their web pages.")
                .to_string(),
        ),
    );
    let plus = widgets::source_tile(TileFace::Icon(Icon::Plus), TileSize::Medium, None);
    let another = pressable(
        setting_row(Some(plus), tr("Add another Nextcloud"), Some(tr("For example for work").to_string()), chevron()),
        Some(Message::AddNextcloud),
    );
    page(vec![
        title(tr("Nextcloud is connected"), tr("This is what your server offers. The next steps use it.")),
        services,
        widgets::group(tr("More accounts"), vec![another], None),
    ])
}

// ----- Calendar and tasks (task W.6) --------------------------------------------------------------

/// Which collections a step shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Calendars,
    Lists,
}

impl Kind {
    fn takes(self, collection: &Collection) -> bool {
        match self {
            Kind::Calendars => collection.kind.has_events(),
            Kind::Lists => collection.kind.has_todos(),
        }
    }
}

/// The accounts whose calendars or task lists the step shows, with those collections: Nextcloud,
/// CalDAV and mail accounts. GitHub's filter lists have their own group on the tasks step.
fn sources(data: &Data, kind: Kind) -> Vec<(&AccountInfo, Vec<&Collection>)> {
    data.accounts
        .iter()
        .filter(|account| matches!(account.kind.as_str(), "nextcloud" | "caldav" | "mail") && !account.id.is_empty())
        .map(|account| (account, account.collections.iter().filter(|c| kind.takes(c)).collect::<Vec<_>>()))
        .filter(|(_, collections)| !collections.is_empty())
        .collect()
}

/// How many calendars or lists the apps will show.
fn shown_count(state: &State, data: &Data, kind: Kind) -> usize {
    sources(data, kind).iter().flat_map(|(_, collections)| collections).filter(|c| state.is_shown(data, &c.id)).count()
}

/// The calendar or the tasks step: per account its calendars or lists with a switch, a CalDAV
/// account to add, and on the tasks step GitHub.
fn collections<'a>(state: &'a State, data: &'a Data, kind: Kind) -> Element<'a, Message> {
    let (headline, lead) = match kind {
        Kind::Calendars => {
            (tr("Calendar"), tr("Choose which calendars the suite shows, and add calendars from other providers."))
        }
        Kind::Lists => {
            (tr("Tasks"), tr("Choose which task lists the suite shows; add CalDAV lists and issues from GitHub."))
        }
    };
    let mut parts = vec![title(headline, lead)];
    let sources = sources(data, kind);
    if sources.is_empty() {
        let none = match kind {
            Kind::Calendars => tr("No calendars yet. Connect a Nextcloud or add a CalDAV account below."),
            Kind::Lists => tr("No task lists yet. Connect a Nextcloud or add a CalDAV account below."),
        };
        parts.push(typo::text(typo::body(), none).style(theme::text_2).into());
    }
    let count = sources.len();
    for (index, (account, collections)) in sources.into_iter().enumerate() {
        let rows = collections.into_iter().map(|collection| collection_row(state, data, collection)).collect();
        let name = match account.kind.as_str() {
            "nextcloud" => trf("Nextcloud · {account}", &[("account", &account.label)]),
            _ => account.label.clone(),
        };
        // Said once, under the last account.
        let footer = (index + 1 == count)
            .then(|| tr("Hidden ones stay on the server; the apps just don't show them.").to_string());
        parts.push(widgets::group(name, rows, footer));
    }
    parts.push(caldav_part(state, kind));
    if kind == Kind::Lists {
        parts.push(github_part(state, data));
    }
    page(parts)
}

/// A calendar or task list: its colour, name, "Read-only" where it is, and the switch.
fn collection_row<'a>(state: &'a State, data: &'a Data, collection: &'a Collection) -> Element<'a, Message> {
    let id = collection.id.clone();
    let switch = toggler(state.is_shown(data, &collection.id))
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle(move |shown| Message::Shown(id.clone(), shown));
    let dot = widgets::color_dot(widgets::collection_color(collection.color.as_deref()), space::s());
    let detail = (!collection.writable).then(|| tr("Read-only").to_string());
    setting_row(Some(dot), collection.name.as_str(), detail, switch)
}

/// "Add a CalDAV account", or its form while it is open.
fn caldav_part(state: &State, kind: Kind) -> Element<'_, Message> {
    let title = match kind {
        Kind::Calendars => tr("Other calendars"),
        Kind::Lists => tr("Other task lists"),
    };
    match &state.caldav {
        Some(form) => column![
            typo::text(typo::caption(), tr("Add a CalDAV account")).style(theme::text_3),
            form.view(Message::CalDav, Message::Next),
        ]
        .spacing(layout::item_gap())
        .into(),
        None => {
            let plus = widgets::source_tile(TileFace::Icon(Icon::Plus), TileSize::Medium, None);
            let add = pressable(
                setting_row(
                    Some(plus),
                    tr("Add a CalDAV account"),
                    Some(tr("mailbox.org, Posteo, Fastmail, iCloud or your own server").to_string()),
                    chevron(),
                ),
                Some(Message::OpenCalDav),
            );
            widgets::group(title, vec![add], None)
        }
    }
}

/// GitHub on the tasks step: connected, or its two ways, or the token field.
fn github_part<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let login = state
        .github_login
        .clone()
        .or_else(|| data.accounts.iter().find(|account| account.kind == "github").map(|account| account.label.clone()));
    if let Some(login) = login {
        let tile = widgets::source_tile(TileFace::Icon(Icon::Github), TileSize::Medium, None);
        let row = setting_row(
            Some(tile),
            trf("Signed in as {account}", &[("account", &login)]),
            Some(tr("Issues and pull requests become filter lists").to_string()),
            widgets::state_pill(Icon::CircleCheck, tr("Connected")),
        );
        return widgets::group("GitHub", vec![row], None);
    }
    if state.github_token {
        return column![
            typo::text(typo::caption(), "GitHub").style(theme::text_3),
            connect::github::token_form(
                &state.github,
                |token| Message::TokenChanged(Secret::new(token)),
                Message::Next,
                Message::OpenTokenPage
            ),
        ]
        .spacing(layout::item_gap())
        .into();
    }
    connect::github::ways("GitHub", Message::UseGithubCli, Message::PasteToken)
}

/// What a component is for, one line on its card.
fn purpose(component: Component, data: &Data) -> String {
    match component {
        Component::Calendar => tr("Calendars from Nextcloud and other CalDAV servers").into(),
        Component::Tasks => tr("Task lists from CalDAV, issues from GitHub").into(),
        Component::Mail => tr("Mail accounts, or the ones from TorroMail").into(),
        Component::Inbox => tr("Notifications, mentions and reviews in one place").into(),
        Component::Assistant => tr("Chat with a local or a cloud AI").into(),
        Component::Workspaces => match window_manager(data) {
            Some(name) => trf("Save and restore window layouts · {wm} found", &[("wm", &name)]),
            None => tr("Save and restore window layouts · needs a tiling window manager").into(),
        },
    }
}

/// The tiling window manager the core works with, by its product name; `None` without one.
fn window_manager(data: &Data) -> Option<String> {
    let backend = data.status.as_ref()?.backend.as_str();
    let name = match backend {
        "" | "unsupported" => return None,
        "hyprland" => "Hyprland",
        "aerospace" => "AeroSpace",
        "glazewm" => "GlazeWM",
        other => other,
    };
    Some(name.to_string())
}

/// A component's name.
pub fn name(component: Component) -> &'static str {
    match component {
        Component::Calendar => tr("Calendar"),
        Component::Tasks => tr("Tasks"),
        Component::Mail => tr("Mail"),
        Component::Inbox => tr("Notification Hub"),
        Component::Assistant => tr("Assistant"),
        Component::Workspaces => tr("Workspaces"),
    }
}

/// A component's symbol, as the suite shows its app.
fn symbol(component: Component) -> Icon {
    match component {
        Component::Calendar => Icon::Calendar,
        Component::Tasks => Icon::ListChecks,
        Component::Mail => Icon::Mail,
        Component::Inbox => Icon::Inbox,
        Component::Assistant => Icon::User,
        Component::Workspaces => Icon::Layers,
    }
}

fn components<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let rows = Component::ALL
        .into_iter()
        .map(|component| {
            let on = !state.off.contains(&component);
            let switch = toggler(on)
                .size(style::CHECK_SIZE)
                .style(style::toggler)
                .on_toggle(move |on| Message::Component(component, on));
            let tile = widgets::source_tile(TileFace::Icon(symbol(component)), TileSize::Medium, None);
            setting_row(Some(tile), name(component), Some(purpose(component, data)), switch)
        })
        .collect();
    let components = widgets::group(
        tr("Components"),
        rows,
        Some(
            tr("Turned off, a part of the suite leaves the app switcher and the tray, and nothing runs for it in the background. Accounts and data stay.")
                .to_string(),
        ),
    );
    page(vec![
        title(
            tr("What do you want to use?"),
            tr("Turn on what you need; the next steps set up each of them. You can change this later under Apps."),
        ),
        components,
    ])
}

fn done<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let installed = |app: AppId| {
        data.apps.as_ref().is_some_and(|apps| apps.iter().any(|a| a.window_class == app.window_class && a.installed))
    };
    let rows = Component::ALL
        .into_iter()
        .map(|component| {
            let on = !state.off.contains(&component);
            let detail = if on { tr("On") } else { tr("Off") };
            let mark: Element<'a, Message> =
                widgets::source_tile(TileFace::Icon(symbol(component)), TileSize::Medium, None);
            let control: Element<'a, Message> = match app_of(component).filter(|_| on) {
                Some(app) => widgets::text_button(
                    Some(Icon::ArrowUpRight),
                    tr("Open"),
                    style::button::ghost,
                    installed(app).then_some(Message::OpenApp(app.window_class)),
                ),
                None => Space::new().into(),
            };
            setting_row(Some(mark), name(component), Some(detail.to_string()), control)
        })
        .collect();
    let summary = widgets::group(tr("Components"), rows, Some(tr("Change any of this later in Settings.").to_string()));
    page(vec![title(tr("You're set up"), tr("Torro Workplace is ready. Here is what you turned on.")), summary])
}
