//! The pages of the setup's steps (task W.4: Welcome, Components and Done; the steps of tasks
//! W.5–W.8, WEB.7 and #303 follow here): each with a headline, one sentence in `text_2` and settings groups, plus
//! what the footer says and what its primary button is called.

use iced::widget::text::Wrapping;
use iced::widget::{Space, checkbox, column, container, pick_list, row, toggler};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::AppId;
use torro_core::ai::AiProvider;
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, Language};
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets::{self, TileFace, TileSize};
use torro_gui::{style, typo};

use torro_core::account::Secret;
use torro_inbox::{SourceKind, SourceRef};
use torro_ipc::extensions::CatalogEntry;
use torro_ipc::{AccountInfo, ServerService};
use torro_pim::{AccountState, Collection};
use torro_settings_ui::assistant::web as web_view;

use super::{Data, Message, State, Step, app_of};
use crate::ui::{chevron, connect, pressable, setting_row};

/// The page of the current step, laid out for `class`.
pub fn view<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    match state.step {
        Step::Welcome => welcome(state),
        Step::Nextcloud => nextcloud(state, data),
        Step::Components => components(state, data),
        Step::Done => done(state, data),
        // Not listed until their tasks build them ([`Step::built`]).
        Step::Calendar => collections(state, data, Kind::Calendars),
        Step::Tasks => collections(state, data, Kind::Lists),
        Step::Mail => mail(data),
        Step::Inbox => inbox(state, data),
        Step::Ai => ai(state, data),
        Step::Web => web(state, data, class),
        Step::Documents => documents(data, class),
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
        Step::Mail => {
            let n = data.accounts.iter().filter(|account| account.kind == "mail").count();
            Some(if n == 1 { tr("1 account").to_string() } else { trf("{n} accounts", &[("n", &n)]) })
        }
        Step::Inbox => {
            let on = inbox_sources(data).iter().filter(|(source, ..)| !state.is_muted(data, source)).count();
            Some(trf("{n} sources on", &[("n", &on)]))
        }
        Step::Ai if state.ai_searching => Some(tr("Looking on this computer…").to_string()),
        Step::Ai if state.ai_connecting.is_some() => Some(tr("Connecting…").to_string()),
        Step::Ai => {
            let n = data.accounts.iter().filter(|account| account.kind == "ai").count() + state.ai_connected.len();
            Some(if n == 1 { tr("1 account").to_string() } else { trf("{n} accounts", &[("n", &n)]) })
        }
        Step::Web if super::web_waits(state, data) => Some(tr("Saving…").to_string()),
        Step::Web => Some(match providers_on(state, data).len() {
            0 => tr("Web search is off").to_string(),
            1 => tr("1 provider on").to_string(),
            n => trf("{n} providers on", &[("n", &n)]),
        }),
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
        // A switch waits for the core, and "Continue" would give the assistant `web` by the core's list.
        Step::Web if super::web_waits(state, data) => (tr("Continue"), false),
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

// ----- Mail and the Notification Hub (task W.7) -------------------------------------------------------

/// A row that opens something: symbol, name, one line under it and the chevron.
fn opener<'a>(symbol: Icon, name: &'a str, detail: &'a str, message: Message) -> Element<'a, Message> {
    let tile = widgets::source_tile(TileFace::Icon(symbol), TileSize::Medium, None);
    pressable(setting_row(Some(tile), name, Some(detail.to_string()), chevron()), Some(message))
}

/// The mail accounts there are, and the two ways to add one; both open the add-account dialog.
fn mail(data: &Data) -> Element<'_, Message> {
    let accounts: Vec<Element<'_, Message>> = data
        .accounts
        .iter()
        .filter(|account| account.kind == "mail")
        .map(|account| {
            let tile = widgets::source_tile(TileFace::Icon(Icon::Mail), TileSize::Medium, None);
            let (detail, pill) = match &account.state {
                AccountState::Connected { problem: Some(problem), .. } => {
                    (problem.clone(), widgets::state_pill(Icon::CircleAlert, tr("Problem")))
                }
                _ => (
                    tr("Mail, and calendars where the provider has them").to_string(),
                    widgets::state_pill(Icon::CircleCheck, tr("Connected")),
                ),
            };
            setting_row(Some(tile), account.label.as_str(), Some(detail), pill)
        })
        .collect();
    let mut parts =
        vec![title(tr("Mail"), tr("Add your mail accounts. Calendars that come with them show up under Calendar."))];
    if accounts.is_empty() {
        parts.push(typo::text(typo::body(), tr("No mail account yet.")).style(theme::text_2).into());
    } else {
        parts.push(widgets::group(tr("Mail accounts"), accounts, None));
    }
    let add = vec![
        opener(Icon::Plus, tr("Add mail account"), tr("Finds the servers from your address"), Message::AddMail),
        opener(
            Icon::MailOpen,
            tr("Take over from TorroMail"),
            tr("Uses the accounts of the TorroMail app"),
            Message::TakeOverTorroMail,
        ),
    ];
    parts.push(widgets::group(
        tr("Add"),
        add,
        Some(tr("Passwords stay in the system keychain; agents never see them.").to_string()),
    ));
    page(parts)
}

/// The inbox sources the connected accounts bring: the source, its symbol, its name and the
/// account it belongs to.
fn inbox_sources(data: &Data) -> Vec<(SourceRef, Icon, &'static str, &str)> {
    let mut sources = Vec::new();
    for account in data.accounts.iter().filter(|account| !account.id.is_empty()) {
        // The account's id is `<kind>:<name>`; the source of the account has that name.
        let Some((_, name)) = account.id.split_once(':') else { continue };
        let source = |kind| SourceRef { kind, name: name.to_string() };
        let label = account.label.as_str();
        match account.kind.as_str() {
            "nextcloud" => {
                sources.push((source(SourceKind::Nextcloud), Icon::Bell, tr("Nextcloud notifications"), label));
                let talk = account.services.as_ref().is_none_or(|services| services.contains(&ServerService::Talk));
                if talk {
                    sources.push((source(SourceKind::Talk), Icon::MessageCircle, "Talk", label));
                }
            }
            "github" => sources.push((source(SourceKind::Github), Icon::Github, "GitHub", label)),
            "mail" => sources.push((source(SourceKind::Mail), Icon::Mail, tr("Mail"), label)),
            "ntfy" => sources.push((source(SourceKind::Ntfy), Icon::RadioTower, "ntfy", label)),
            _ => {}
        }
    }
    sources
}

/// The Notification Hub: a switch per source the accounts bring, ntfy's topics to subscribe, and
/// quiet during busy events.
fn inbox<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let rows: Vec<Element<'a, Message>> = inbox_sources(data)
        .into_iter()
        .map(|(source, symbol, name, account)| {
            let on = !state.is_muted(data, &source);
            let switch = toggler(on)
                .size(style::CHECK_SIZE)
                .style(style::toggler)
                .on_toggle(move |on| Message::Source(source.clone(), on));
            let tile = widgets::source_tile(TileFace::Icon(symbol), TileSize::Medium, None);
            setting_row(Some(tile), name, Some(account.to_string()), switch)
        })
        .collect();
    let mut parts = vec![title(tr("Notification Hub"), tr("One place for what needs you. Choose what comes in."))];
    if rows.is_empty() {
        let none = tr("No sources yet. They come with the accounts you connect.");
        parts.push(typo::text(typo::body(), none).style(theme::text_2).into());
    } else {
        let footer = tr("A source turned off brings nothing new; what is there stays.");
        parts.push(widgets::group(tr("Sources"), rows, Some(footer.to_string())));
    }
    let ntfy = opener(
        Icon::RadioTower,
        tr("Subscribe to ntfy topics"),
        tr("CI, uptime checks and your own scripts"),
        Message::AddNtfy,
    );
    parts.push(widgets::group(tr("More sources"), vec![ntfy], None));
    let quiet = toggler(state.quiet.unwrap_or(data.settings.quiet_when_busy))
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle(Message::Quiet);
    let quiet = setting_row(
        None,
        tr("Quiet during busy events"),
        Some(tr("Only urgent entries pop up while a busy event runs; a summary follows.").to_string()),
        quiet,
    );
    parts.push(widgets::group(tr("Notifications"), vec![quiet], None));
    page(parts)
}

// ----- AI (task W.8) ----------------------------------------------------------------------------------

/// Whether an AI account of `provider` is connected (its id is `ai:<provider>@<host>`).
fn has_ai_account(data: &Data, provider: AiProvider) -> bool {
    let prefix = format!("ai:{}@", provider.id());
    data.accounts.iter().any(|account| account.kind == "ai" && account.id.starts_with(&prefix))
}

/// Open First: the servers on this computer, then the cloud providers by where they are, then what
/// a new chat of the assistant starts with.
fn ai<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let local = [AiProvider::Ollama, AiProvider::LmStudio].into_iter().map(|provider| local_row(state, data, provider));
    let mut local: Vec<Element<'a, Message>> = local.collect();
    local.push(opener(
        Icon::Plus,
        tr("Other server"),
        tr("vLLM, llama.cpp, LocalAI or a server of your own"),
        Message::OtherAiServer,
    ));
    let mut parts = vec![
        title(
            tr("AI"),
            tr("Local models first: they keep your data on this computer. Add cloud providers if you want more."),
        ),
        widgets::group(
            tr("On this computer"),
            local,
            Some(tr("Models on this computer run here. Nothing you ask them leaves it.").to_string()),
        ),
    ];
    let groups = [
        (tr("Open models"), vec![AiProvider::OpenRouter]),
        (tr("Providers"), vec![AiProvider::Anthropic, AiProvider::OpenAi, AiProvider::Gemini, AiProvider::Mistral]),
        (
            tr("Providers in China"),
            vec![AiProvider::DeepSeek, AiProvider::Qwen, AiProvider::Kimi, AiProvider::Glm, AiProvider::MiniMax],
        ),
    ];
    let count = groups.len();
    for (index, (name, providers)) in groups.into_iter().enumerate() {
        let rows = providers.into_iter().map(|provider| cloud_row(data, provider)).collect();
        let footer = (index + 1 == count).then(|| {
            tr("Cloud providers receive what you send them. Mail goes to local models only unless you change it.")
                .to_string()
        });
        parts.push(widgets::group(name, rows, footer));
    }
    if let Some(choice) = engine_choice(state, data) {
        parts.push(choice);
    }
    page(parts)
}

/// A server on this computer: looking, found with "Connect" (or "Connected"), or not running.
fn local_row<'a>(state: &'a State, data: &'a Data, provider: AiProvider) -> Element<'a, Message> {
    let tile = widgets::source_tile(TileFace::Icon(Icon::AppWindow), TileSize::Medium, None);
    let Some(found) = &state.ai_found else {
        return setting_row(
            Some(tile),
            provider.name(),
            Some(tr("Looking on this computer…").to_string()),
            Space::new(),
        );
    };
    let Some(server) = found.iter().find(|server| server.provider == provider) else {
        return setting_row(
            Some(tile),
            provider.name(),
            Some(tr("Not running on this computer").to_string()),
            Space::new(),
        );
    };
    let rest = server.url.split_once("://").map_or(server.url.as_str(), |(_, rest)| rest);
    let host = rest.split('/').next().unwrap_or(rest);
    let detail = match server.models {
        1 => trf("{host} · 1 model", &[("host", &host)]),
        n => trf("{host} · {n} models", &[("host", &host), ("n", &n)]),
    };
    let connected =
        server.account.is_some() || state.ai_connected.contains(&server.url) || has_ai_account(data, provider);
    let control: Element<'a, Message> = if connected {
        widgets::state_pill(Icon::CircleCheck, tr("Connected"))
    } else if state.ai_connecting.as_deref() == Some(server.url.as_str()) {
        typo::text(typo::meta(), tr("Connecting…")).style(theme::text_2).into()
    } else {
        let press = state.ai_connecting.is_none().then(|| Message::ConnectLocalAi(provider, server.url.clone()));
        widgets::text_button(None, tr("Connect"), style::button::secondary, press)
    };
    setting_row(Some(tile), provider.name(), Some(detail), control)
}

/// A cloud provider: its first letter, its name, where it is (and open weights), and "Add key" or
/// "Connected".
fn cloud_row(data: &Data, provider: AiProvider) -> Element<'_, Message> {
    let tile = widgets::source_tile(TileFace::letter(provider.name()), TileSize::Medium, None);
    let seat = provider.seat().label();
    let detail = match provider {
        AiProvider::OpenRouter => trf("{seat} · open models from many makers", &[("seat", &seat)]),
        _ if provider.open_weights() => trf("{seat} · open weights", &[("seat", &seat)]),
        _ => seat.to_string(),
    };
    let control: Element<'_, Message> = match has_ai_account(data, provider) {
        true => widgets::state_pill(Icon::CircleCheck, tr("Connected")),
        false => widgets::text_button(None, tr("Add key"), style::button::secondary, Some(Message::AddAiKey(provider))),
    };
    setting_row(Some(tile), provider.name(), Some(detail), control)
}

/// "Use for": the AI account a new chat of the assistant starts with, once there is one.
fn engine_choice<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let engines: Vec<Choice> = data
        .engines
        .iter()
        .filter(|engine| engine.kind == torro_ipc::assistant::EngineKind::Ai)
        .map(|engine| Choice { id: engine.id.clone(), name: engine.name.clone() })
        .collect();
    if engines.is_empty() {
        return None;
    }
    let current =
        state.engine.clone().or_else(|| data.settings.assistant_default.as_ref().map(|default| default.engine.clone()));
    let selected = current.and_then(|id| engines.iter().find(|choice| choice.id == id).cloned());
    let pick = pick_list(engines, selected, |choice: Choice| Message::Engine(choice.id))
        .placeholder(tr("Choose"))
        .padding(style::pick_list_padding())
        .text_size(typo::caption().size)
        .text_line_height(typo::caption().line_height)
        .font(typo::caption().font)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu);
    let row = setting_row(None, tr("Assistant"), Some(tr("What a new chat starts with").to_string()), pick);
    Some(widgets::group(
        tr("Use for"),
        vec![row],
        Some(tr("Choose the model and how hard it thinks later under Assistant.").to_string()),
    ))
}

/// An AI account in the "Use for" list: shown by its name, chosen by its engine id.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    id: String,
    name: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

// ----- Web search (task WEB.7) ------------------------------------------------------------------------

/// The names of the providers switched on, in the order they are asked.
pub(super) fn providers_on<'a>(state: &'a State, data: &'a Data) -> Vec<&'a str> {
    super::providers(state, data).iter().filter(|info| info.enabled).map(|info| info.name.as_str()).collect()
}

/// "Let the assistant use it" with the order the providers are asked in and the approval once per conversation
/// under it – the page's decision and its state, so before the details – then the providers as Assistant ›
/// Settings › Tools shows them. While no provider can be asked (SearXNG only with an address) the box is drawn
/// disabled, keeping its tick for when one is: "Continue" would give nothing then. Without the assistant's client
/// (the assistant never started) there is nobody to give the right to: the sentence stands alone.
fn web<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    let providers = super::providers(state, data);
    let summary = web_view::summary(providers);
    let usable = providers.iter().any(super::askable);
    let allow = match super::assistant_client(data) {
        Some(_) => {
            let on = state.web_allowed;
            let check = checkbox(on)
                .size(style::CHECK_SIZE)
                // `text_2`, not `text`: a black box would be the heaviest mark of the page.
                .style(|t: &Theme, status| style::checkbox(palette(t).text_2)(t, status))
                .on_toggle_maybe(usable.then_some(Message::AllowWeb));
            let row = setting_row(
                Some(check.into()),
                tr("Let the assistant use it"),
                Some(tr("Read access to web search, given when you continue").to_string()),
                Space::new(),
            );
            let press = usable.then_some(Message::AllowWeb(!on));
            widgets::group(tr("Assistant"), vec![pressable(row, press)], Some(summary))
        }
        None => typo::text(typo::body(), summary).style(theme::text_2).into(),
    };
    page(vec![
        title(
            tr("Web search"),
            tr(
                "The assistant can search the web and read pages. Choose where your search words go; nothing is on until you switch it on.",
            ),
        ),
        allow,
        web_view::view(&state.web, providers, class).map(Message::Web),
    ])
}

// ----- Documents (issue #303) --------------------------------------------------------------------------

/// The ArtCraft apps the step presents, by their catalog id, in this order, with their symbol and what each is for.
pub(super) const APPS: [(&str, Icon, &str); 4] = [
    (
        "wordcraft",
        Icon::FileText,
        "Documents (.docx) · Summarize a contract, fix the wording of a letter, draft a reply from your notes",
    ),
    (
        "deckcraft",
        Icon::Presentation,
        "Presentations (.pptx) · Turn an outline into slides, add a chart, tidy up every slide's text",
    ),
    (
        "gridcraft",
        Icon::Sheet,
        "Spreadsheets (.xlsx, .csv) · Read out figures, fill in a table, sort and chart your data",
    ),
    ("pdfcraft", Icon::FileType, "PDF · Find a passage, fill in a form, comment, combine or split files"),
];

/// How an ArtCraft app stands on this computer, for the step Documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AppState {
    /// Installed as an MCP server.
    SetUp,
    /// Its program is there (nothing of the entry is missing), not set up yet.
    Installed,
    /// Its program is missing: "Get <App>".
    Missing,
}

/// How the catalog entry `entry` stands: set up as a server, its program found, or missing.
pub(super) fn app_state(data: &Data, entry: &CatalogEntry) -> AppState {
    if crate::ui::connections::installed(data, &entry.id).is_some() {
        AppState::SetUp
    } else if entry.missing.is_empty() {
        AppState::Installed
    } else {
        AppState::Missing
    }
}

/// "Work with your documents": what the ArtCraft apps let the assistant do, one row each with its state, and where
/// they are set up. Only a presentation: it stores nothing. While the catalog has not answered, "Loading …"; when it
/// cannot be listed, a sentence; "Continue" goes on either way.
fn documents(data: &Data, class: SizeClass) -> Element<'_, Message> {
    let lead = tr(
        "Let the assistant open, read and edit your documents, presentations, spreadsheets and PDFs. The free ArtCraft \
         apps do the work right here on your computer, and Torro keeps your originals safe: every change is saved as a \
         new file.",
    );
    let words = matches!(class, SizeClass::Wide | SizeClass::Medium);
    let apps: Element<'_, Message> = match &data.catalog {
        crate::ui::connections::Listing::Failed(_) => {
            let sentence = tr("The apps can't be listed right now. You find them later under MCP servers.");
            typo::text(typo::body(), sentence).style(theme::text_2).into()
        }
        crate::ui::connections::Listing::Entries(entries) => {
            let rows: Vec<Element<'_, Message>> = APPS
                .iter()
                .filter_map(|(id, symbol, what)| {
                    let entry = entries.iter().find(|entry| entry.id == *id)?;
                    Some(app_row(data, entry, *symbol, tr(what), words))
                })
                .collect();
            match (rows.is_empty(), entries.is_empty()) {
                // Nothing listed yet: the catalog has not answered.
                (true, true) => widgets::loading(),
                // Answered, without the apps: a service of another version.
                (true, false) => {
                    let sentence = tr("This version of Torro Workplace doesn't list the ArtCraft apps yet.");
                    typo::text(typo::body(), sentence).style(theme::text_2).into()
                }
                (false, _) => {
                    let footer = tr(
                        "Open source (MIT or Apache 2.0) and still young: the apps call themselves experimental. Each \
                         one works only in a folder you choose; changes are saved as new files, and anything that \
                         could replace a file asks you first. What the assistant reads goes to the AI model you chose.",
                    );
                    widgets::group(tr("ArtCraft apps"), rows, Some(footer.to_string()))
                }
            }
        }
    };
    let later = typo::text(typo::body(), tr("Installed one? Set it up later in Settings › MCP servers › Catalog."))
        .style(theme::text_2)
        .wrapping(Wrapping::WordOrGlyph);
    page(vec![title(tr("Work with your documents"), lead), apps, later.into()])
}

/// One ArtCraft app: its tile, its name with "Experimental", what it is for below (wrapping), and at the end how it
/// stands – "Set up" or "Installed" with a dot, or "Get <App>" to its releases.
fn app_row<'a>(data: &Data, entry: &'a CatalogEntry, symbol: Icon, what: &'a str, words: bool) -> Element<'a, Message> {
    let tile = widgets::source_tile(TileFace::Icon(symbol), TileSize::Medium, None);
    let mut name = row![widgets::ellipsized(typo::body(), entry.name.as_str(), theme::text)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if entry.experimental {
        name = name.push(widgets::pill(None, tr("Experimental")));
    }
    let lines =
        column![name, typo::text(typo::meta(), what).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)].width(Fill);
    let get = Message::GetApp(entry.url.clone());
    let label = trf("Get {app}", &[("app", &entry.name)]);
    let control: Element<'a, Message> = match (app_state(data, entry), words) {
        // Pills as the step Mail shows its accounts; only a set-up app has the check.
        (AppState::SetUp, _) => widgets::state_pill(Icon::CircleCheck, tr("Set up")),
        (AppState::Installed, _) => widgets::state_pill(Icon::Circle, tr("Installed")),
        (AppState::Missing, true) => {
            widgets::text_button(Some(Icon::ArrowUpRight), label, style::button::ghost, Some(get))
        }
        (AppState::Missing, false) => widgets::icon_button(Icon::ArrowUpRight, label, Some(get)),
    };
    // As high as a settings row with a detail while the text takes one line; it grows with more.
    let strut = Space::new().height(size::header() - space::m());
    let text = row![strut, tile, lines].spacing(space::m()).width(Fill).align_y(Alignment::Center);
    let content = row![text, control].spacing(space::l()).align_y(Alignment::Center);
    container(content).width(Fill).padding([space::m(), space::l()]).into()
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
    let mut rows = Vec::new();
    for component in Component::ALL {
        let on = !state.off.contains(&component);
        let detail = match (on, component) {
            // The web search is the assistant's tool, so it is said in the assistant's line.
            (true, Component::Assistant) => format!("{} · {}", tr("On"), web_detail(state, data)),
            (true, _) => tr("On").to_string(),
            (false, _) => tr("Off").to_string(),
        };
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
        rows.push(setting_row(Some(mark), name(component), Some(detail), control));
    }
    let summary = widgets::group(tr("Components"), rows, Some(tr("Change any of this later in Settings.").to_string()));
    page(vec![title(tr("You're set up"), tr("Torro Workplace is ready. Here is what you turned on.")), summary])
}

/// The web search in the assistant's line on the last step: "Web search · DuckDuckGo" with the providers switched
/// on, or "Web search off".
pub(super) fn web_detail(state: &State, data: &Data) -> String {
    let names = providers_on(state, data);
    match names.is_empty() {
        true => tr("Web search off").to_string(),
        false => trf("Web search · {providers}", &[("providers", &names.join(", "))]),
    }
}
