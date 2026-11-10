//! The pages of the setup's steps (issue #326, drafts `docs/prototypes/onboarding/`): each with a headline,
//! one sentence in `text_2`, a box "Why this matters." where the draft has one, and only what the point
//! needs; plus what the footer says and which buttons it has.

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row};
use iced::{Element, Fill, Theme};
use torro_core::AppId;
use torro_core::ai::{AiProvider, AiSlot};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, Language};
use torro_gui::icon::{self, Icon};
use torro_gui::layout;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets::{self, Align, MenuItem, MenuSection, TileFace, TileSize};
use torro_gui::{style, typo};
use torro_ipc::AccountInfo;
use torro_ipc::extensions::CatalogEntry;
use torro_pim::AccountState;
use torro_settings_ui::accounts::{ai_provider, ai_setup};
use torro_settings_ui::suite;

use super::{AiPage, Data, Message, SetupMenu, State, Step, wizard};
use crate::ui::{Section, chevron, connect, pressable, setting_row};

/// The page of the current step.
pub fn view<'a>(state: &'a State, data: &'a Data, looks: &'a suite::State) -> Element<'a, Message> {
    match state.step {
        Step::Welcome => welcome(state),
        Step::Mail => mail(state, data),
        Step::Cloud => cloud(state, data),
        Step::Ai if super::ai_form_open(state) || ai_setup::foreground(&state.ai_setup) => {
            ai_setup::view(&state.ai_setup, data).map(Message::AiSetup)
        }
        Step::Ai => match state.ai_page {
            AiPage::Connect => ai_connect(state, data),
            AiPage::Chat => ai_chat(state, data),
            AiPage::Sort => ai_sort(state, data),
        },
        Step::Appearance => appearance(data, looks),
        Step::Documents => documents(data),
        Step::Done => done(state, data),
    }
}

/// What the footer says at its left on the current step, if anything.
pub fn status(state: &State, data: &Data) -> Option<String> {
    match state.step {
        // The page says it already ("Waiting for the browser …").
        Step::Cloud if state.nextcloud.running() => None,
        Step::Cloud => super::nextcloud_account(state, data)
            .filter(|_| !state.asks_address(data))
            .map(|account| trf("Signed in as {account}", &[("account", &account.label)])),
        Step::Mail => wizard::mail_error(&state.mail).map(str::to_string),
        Step::Ai => ai_setup::status(&state.ai_setup).map(str::to_string),
        _ => None,
    }
}

/// The primary button on the current step: its word, whether it can be pressed, and whether it shows that
/// something runs ([`widgets::busy_button`]).
pub fn primary(state: &State, data: &Data) -> (&'static str, bool, bool) {
    match state.step {
        Step::Welcome => (tr("Get started"), true, false),
        Step::Mail if state.mail_running() => match wizard::mail_primary(&state.mail) {
            Some((label, ready)) => (label, ready, label == tr("Signing in…")),
            None => (tr("Find provider"), false, true),
        },
        Step::Cloud if state.nextcloud.running() => (tr("Connect"), false, false),
        Step::Cloud if state.asks_address(data) => (tr("Connect"), state.nextcloud.can_start(), false),
        Step::Ai if super::ai_form_open(state) => match ai_setup::primary_label(&state.ai_setup) {
            Some((label, enabled)) => (label, enabled, !enabled),
            None => (tr("Connect"), false, false),
        },
        Step::Ai => match state.ai_page {
            AiPage::Connect if ai_setup::preparing(&state.ai_setup) => (tr("Continue"), true, false),
            AiPage::Connect if super::keeps(data) => (tr("Keep my choice"), true, false),
            AiPage::Connect => {
                (tr("Use suggestion"), ai_setup::can_use_suggestion(&state.ai_setup, data, false, None), false)
            }
            AiPage::Chat => (tr("Continue"), true, false),
            AiPage::Sort => (tr("Continue"), !super::ai_models_wait(data), false),
        },
        Step::Done => (tr("Finish"), true, false),
        _ => (tr("Continue"), true, false),
    }
}

/// The step's way to skip it, left of the primary button: "Skip", "I don't use one", "Skip AI", "Keep the
/// default"; "Cancel" while a sign-in waits or a mail path runs. None on the welcome, the pages Chat and
/// Sorting, Documents and the summary.
pub fn skip(state: &State, data: &Data) -> Option<(&'static str, Message)> {
    match state.step {
        Step::Welcome | Step::Documents | Step::Done => None,
        Step::Mail if state.mail_running() => Some((tr("Cancel"), Message::AddAnotherMail)),
        Step::Mail => Some((tr("Skip"), Message::Skip)),
        Step::Cloud if state.nextcloud.running() || state.adding => Some((tr("Cancel"), Message::Cancel)),
        Step::Cloud if state.asks_address(data) => Some((tr("I don't use one"), Message::Skip)),
        Step::Cloud => None,
        Step::Ai if super::ai_form_open(state) => None,
        Step::Ai if state.ai_page == AiPage::Connect => Some((tr("Skip AI"), Message::Skip)),
        Step::Ai => None,
        Step::Appearance => Some((tr("Keep the default"), Message::Skip)),
    }
}

/// More ways on the current step, between "Skip" and the primary button: "Add another account" once a
/// mail account is connected; on the page Connect "Choose myself", and "Use suggestion" beside "Keep" when
/// the suggestion differs from the models chosen (issue #326, "Run setup again").
pub fn second(state: &State, data: &Data) -> Vec<(&'static str, Option<Message>)> {
    match state.step {
        Step::Mail if wizard::mail_done(&state.mail) => {
            vec![(tr("Add another account"), Some(Message::AddAnotherMail))]
        }
        Step::Ai if state.ai_page == AiPage::Connect && !super::ai_form_open(state) => {
            let mut ways = vec![(tr("Choose myself"), Some(Message::ChooseMyself))];
            if super::keeps(data) && !ai_setup::preparing(&state.ai_setup) && ai_setup::differs(&state.ai_setup, data) {
                let can = ai_setup::can_use_suggestion(&state.ai_setup, data, true, None);
                ways.push((tr("Use suggestion"), can.then_some(Message::UseSuggestion)));
            }
            ways
        }
        _ => Vec::new(),
    }
}

/// A quiet link at the footer's left: "Take over accounts from TorroMail" on the point Mail and calendar,
/// "Connect an agent…" on the summary.
pub fn link(state: &State) -> Option<Element<'_, Message>> {
    let (label, message) = match state.step {
        Step::Mail => (tr("Take over accounts from TorroMail"), Message::TakeOverTorroMail),
        Step::Done => (tr("Connect an agent…"), Message::OpenSection(Section::Mcp)),
        _ => return None,
    };
    Some(widgets::text_button(None, label, style::button::ghost, Some(message)))
}

/// The headline and the sentence under it.
fn title<'a>(headline: &'a str, lead: &'a str) -> Element<'a, Message> {
    column![
        typo::text(typo::headline(), headline),
        typo::text(typo::body(), lead).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)
    ]
    .spacing(space::xs())
    .into()
}

/// The box "Why this matters." with one or two sentences: why the point is worth a minute.
fn why<'a>(heading: &'a str, text: &'a str) -> Element<'a, Message> {
    let content = column![
        typo::text(typo::body_strong(), heading),
        typo::text(typo::body(), text).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)
    ]
    .spacing(space::xs());
    container(content).width(Fill).padding(layout::panel_padding()).style(style::container::chip).into()
}

/// The parts of a page with the gap between groups.
fn page<'a>(parts: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    column(parts).spacing(layout::section_gap()).width(Fill).into()
}

/// A sentence in `text_2` that wraps.
fn sentence<'a>(text: &'a str) -> Element<'a, Message> {
    typo::text(typo::body(), text).style(theme::text_2).wrapping(Wrapping::WordOrGlyph).into()
}

/// What each point is for, one sentence each, as the welcome lists them.
fn point_sentence(step: Step) -> &'static str {
    match step {
        Step::Mail => tr("Your mail accounts, with the calendars and tasks your provider offers."),
        Step::Cloud => tr("The server your files, calendars and contacts live on. Today that is Nextcloud. Optional."),
        Step::Ai => tr("A model to chat with and one that sorts your mail, suggested for this computer."),
        Step::Appearance => tr("Light or dark, layout, colors and font."),
        Step::Documents => tr("Works hand in hand with ArtCraft for documents, slides, sheets and PDFs."),
        Step::Welcome | Step::Done => "",
    }
}

/// The tile with a point's number.
fn number_tile<'a>(step: Step) -> Element<'a, Message> {
    let number = step.number().unwrap_or_default().to_string();
    widgets::source_tile(TileFace::letter(&number), TileSize::Medium, None)
}

fn welcome(state: &State) -> Element<'_, Message> {
    let points = Step::POINTS
        .into_iter()
        .map(|step| {
            setting_row(Some(number_tile(step)), step.label(), Some(point_sentence(step).to_string()), Space::new())
        })
        .collect();
    let languages = [Language::System, Language::English, Language::German];
    let language = widgets::segmented(
        [
            (tr("System"), Message::Language(Language::System)),
            // In their own names on purpose: someone who cannot read the current language must still find
            // theirs.
            ("English", Message::Language(Language::English)),
            ("Deutsch", Message::Language(Language::German)),
        ],
        languages.iter().position(|language| *language == state.language).unwrap_or_default(),
    );
    let language = setting_row(
        None,
        tr("App language"),
        Some(tr("Taken from your system. A new language shows when an app starts again.").to_string()),
        language,
    );
    page(vec![
        title(
            tr("Set up Torro Workplace"),
            tr("Five short steps. Each one can be skipped, and what you connect is saved right away."),
        ),
        widgets::group(tr("The five steps"), points, None),
        widgets::group(tr("Language"), vec![language], None),
    ])
}

// ----- Mail and calendar ------------------------------------------------------------------------------

/// How many calendars and task lists `account` brings, in words: "Mail · 2 calendars · 1 task list".
fn brings(account: &AccountInfo) -> String {
    let calendars = account.collections.iter().filter(|c| c.kind.has_events()).count();
    let lists = account.collections.iter().filter(|c| c.kind.has_todos()).count();
    let calendars = match calendars {
        1 => tr("1 calendar").to_string(),
        n => trf("{n} calendars", &[("n", &n)]),
    };
    let lists = match lists {
        1 => tr("1 task list").to_string(),
        n => trf("{n} task lists", &[("n", &n)]),
    };
    format!("{} · {calendars} · {lists}", tr("Mail"))
}

/// The mail path inline under "Add an account" (the same path as Accounts › Add: address, password,
/// services) and the accounts connected so far.
fn mail<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut parts = vec![
        title(
            tr("Mail and calendar"),
            tr("Connect your mail account. Torro finds the server settings for most providers."),
        ),
        why(
            tr("Why this matters."),
            tr(
                "Most providers bring their calendars and task lists with the account, so one sign-in fills Mail, Calendar and Tasks.",
            ),
        ),
    ];
    let mut add = column![
        typo::text(typo::caption(), tr("Add an account")).style(theme::text_3),
        wizard::mail_page(&state.mail, data).map(Message::Mail),
    ]
    .spacing(layout::item_gap());
    if wizard::mail_untouched(&state.mail) {
        add = add.push(
            typo::text(
                typo::meta(),
                tr("Gmail, iCloud, Outlook and some others need an app password instead of your usual one."),
            )
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph),
        );
    }
    parts.push(add.into());
    let accounts: Vec<Element<'a, Message>> = data
        .accounts
        .iter()
        .filter(|account| account.kind == "mail" && !account.id.is_empty())
        .map(|account| {
            let tile = widgets::source_tile(TileFace::Icon(Icon::Mail), TileSize::Medium, None);
            let (detail, pill) = match &account.state {
                AccountState::Connected { problem: Some(problem), .. } => {
                    (problem.clone(), widgets::state_pill(Icon::CircleAlert, tr("Problem")))
                }
                _ => (brings(account), widgets::state_pill(Icon::CircleCheck, tr("Connected"))),
            };
            setting_row(Some(tile), account.label.as_str(), Some(detail), pill)
        })
        .collect();
    if !accounts.is_empty() {
        let footer = tr("Passwords stay in the system keychain; agents never see them.");
        parts.push(widgets::group(tr("Connected"), accounts, Some(footer.to_string())));
    }
    page(parts)
}

// ----- Cloud ------------------------------------------------------------------------------------------

fn cloud<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let lead = tr(
        "Torro builds on a server for your files, calendars and contacts. Today that is Nextcloud. This step is optional.",
    );
    if state.nextcloud.running() {
        return page(vec![
            title(tr("Cloud"), tr("Your browser opened Nextcloud's sign-in page.")),
            connect::nextcloud::waiting(data, Message::OpenLoginPage, |_| Message::LoginLinkEdited),
        ]);
    }
    let reason = why(
        tr("Why this matters."),
        tr("Your data stays on your own server, and Torro signs in through your browser. It never sees your password."),
    );
    match super::nextcloud_account(state, data).filter(|_| !state.adding) {
        Some(account) => page(vec![title(tr("Cloud"), lead), reason, services(account), another_nextcloud()]),
        None => page(vec![title(tr("Cloud"), lead), reason, address(state)]),
    }
}

/// The address form: where the Nextcloud is; the browser does the rest.
fn address(state: &State) -> Element<'_, Message> {
    let field = column![
        typo::text(typo::caption(), tr("Address")).style(theme::text_2),
        connect::nextcloud::address(&state.nextcloud, Message::ServerChanged, Message::Next),
        typo::text(
            typo::meta(),
            tr("Your browser opens to sign in. Torro Workplace keeps an app password in the system keychain.")
        )
        .style(theme::text_2)
        .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(space::xs());
    column![typo::text(typo::body_strong(), "Nextcloud"), field].spacing(layout::item_gap()).into()
}

/// What the server offers, as the account's last sync found it.
fn services(account: &AccountInfo) -> Element<'_, Message> {
    use torro_ipc::ServerService as S;
    let rows: Vec<Element<'_, Message>> = match &account.services {
        None => vec![setting_row(None, tr("Reading what your server offers…"), None, Space::new())],
        Some(services) => {
            let calendars = account.collections.iter().filter(|c| c.kind.has_events()).count();
            let lists = account.collections.iter().filter(|c| c.kind.has_todos()).count();
            let calendars = trf("{n} calendars", &[("n", &calendars)]);
            let lists = trf("{n} lists", &[("n", &lists)]);
            let talk = services.contains(&S::Talk) || services.contains(&S::Notifications);
            let more = services.contains(&S::Deck) || services.contains(&S::Notes);
            [
                (Icon::Calendar, tr("Calendars").to_string(), services.contains(&S::Calendars), calendars),
                (Icon::ListChecks, tr("Tasks").to_string(), services.contains(&S::Tasks), lists),
                (Icon::Bell, tr("Notifications and Talk").to_string(), talk, tr("To the Notification Hub").to_string()),
                (Icon::LayoutGrid, "Deck, Notes".to_string(), more, tr("Later in the suite").to_string()),
            ]
            .into_iter()
            .map(|(symbol, name, there, detail)| {
                let tile = widgets::source_tile(TileFace::Icon(symbol), TileSize::Medium, None);
                let detail = if there { detail } else { tr("Not on this server").to_string() };
                let pill = match there {
                    true => widgets::state_pill(Icon::CircleCheck, tr("Available")),
                    false => widgets::state_pill(Icon::CircleDashed, tr("Missing")),
                };
                setting_row(Some(tile), name, Some(detail), pill)
            })
            .collect()
        }
    };
    let host = match &account.state {
        AccountState::Connected { server, .. } => server.split_once("://").map_or(server.as_str(), |(_, rest)| rest),
        _ => account.label.as_str(),
    };
    let signed_in = setting_row(
        Some(widgets::source_tile(TileFace::Icon(Icon::Cloud), TileSize::Medium, None)),
        host.trim_end_matches('/').to_string(),
        Some(tr("Signed in · Nextcloud").to_string()),
        widgets::state_pill(Icon::CircleCheck, tr("Connected")),
    );
    column![
        widgets::group("Nextcloud", vec![signed_in], None),
        widgets::group(tr("What this server offers"), rows, None)
    ]
    .spacing(layout::section_gap())
    .into()
}

/// "Add another Nextcloud", for example for work.
fn another_nextcloud<'a>() -> Element<'a, Message> {
    let plus = widgets::source_tile(TileFace::Icon(Icon::Plus), TileSize::Medium, None);
    let another = pressable(
        setting_row(Some(plus), tr("Add another Nextcloud"), Some(tr("For example for work").to_string()), chevron()),
        Some(Message::AddNextcloud),
    );
    widgets::group(tr("More accounts"), vec![another], None)
}

// ----- AI ---------------------------------------------------------------------------------------------

/// The account's name for `engine`, else the provider's, else the id.
fn account_name<'a>(data: &'a Data, engine: &'a str) -> &'a str {
    data.accounts
        .iter()
        .find(|account| account.id == engine)
        .map(|account| account.label.as_str())
        .or_else(|| ai_provider(engine).map(AiProvider::name))
        .unwrap_or(engine)
}

/// "qwen3:14b · Ollama" for a slot as stored, or "Not chosen".
fn slot_line(data: &Data, slot: AiSlot) -> String {
    match data.settings.ai_slots.get(slot) {
        Some(held) => {
            let model = held.model.as_deref().unwrap_or(tr("Automatic"));
            format!("{model} · {}", account_name(data, &held.engine))
        }
        None => tr("Not chosen").to_string(),
    }
}

/// The page Connect: the shared AI page (this computer, the programs found, the cloud ways, "Our
/// suggestion for this PC"), and, when models are chosen already, what they are.
fn ai_connect<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut parts = vec![title(
        tr("AI"),
        tr("Torro uses one model to chat and one to sort your mail. Here is what fits this computer."),
    )];
    if super::keeps(data) {
        let rows = vec![
            setting_row(None, tr("Chat"), Some(slot_line(data, AiSlot::Chat)), Space::new()),
            setting_row(None, tr("Sorting"), Some(slot_line(data, AiSlot::Decide)), Space::new()),
        ];
        let footer = match ai_setup::differs(&state.ai_setup, data) {
            true => tr("The suggestion differs from what you chose. “Use suggestion” takes it instead."),
            false => tr("“Keep my choice” goes on with these models."),
        };
        parts.push(widgets::group(tr("Chosen"), rows, Some(footer.to_string())));
    }
    parts.push(ai_setup::view(&state.ai_setup, data).map(Message::AiSetup));
    page(parts)
}

/// The size of a download in words, "9.3 GB"; nothing for a model already there.
fn download(bytes: u64) -> Option<String> {
    (bytes > 0).then(|| trf("Download {size}", &[("size", &torro_gui::units::format_bytes(bytes))]))
}

/// The suggested model of `slot` as a row with "Use", when it is not among the models there are yet (it is
/// downloaded first): its name, "Suggested" and the download.
fn suggested_row<'a>(state: &'a State, data: &'a Data, slot: AiSlot, there: bool) -> Option<Element<'a, Message>> {
    let (account, candidate) = ai_setup::suggested_model(&state.ai_setup, slot)?;
    if there {
        return None;
    }
    let mut detail = vec![tr("Suggested").to_string(), account_name(data, account).to_string()];
    detail.extend(download(candidate.size_bytes));
    let can = ai_setup::can_use_suggestion(&state.ai_setup, data, true, Some(slot));
    let use_it =
        widgets::text_button(None, tr("Use"), style::button::secondary, can.then_some(Message::UseSuggested(slot)));
    Some(setting_row(None, candidate.model.as_str(), Some(detail.join(" · ")), use_it))
}

/// The page Chat: the model the assistant answers with, from the models with tools, the suggestion marked.
fn ai_chat<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut parts = vec![title(
        tr("Model for chat"),
        tr("The assistant answers with this model. Only models that can use tools are listed."),
    )];
    if super::ai_models_wait(data) {
        parts.push(sentence(tr("Checking models…")));
        return page(parts);
    }
    let models = super::chat_models(data);
    let chosen = super::chat_choice(state, data);
    let suggested = ai_setup::suggested_model(&state.ai_setup, AiSlot::Chat);
    let there = suggested.is_some_and(|(account, candidate)| {
        models.iter().any(|(engine, model)| {
            *engine == account && torro_settings_ui::accounts::same_model(&model.id, &candidate.model)
        })
    });
    let mut rows = Vec::new();
    if !models.is_empty() {
        let label = chosen
            .as_ref()
            .and_then(|(engine, id)| models.iter().find(|(e, m)| *e == engine && m.id == *id))
            .map_or_else(|| tr("Choose").to_string(), |(_, model)| model.name.clone());
        let detail = chosen.as_ref().map(|(engine, _)| account_name(data, engine).to_string());
        rows.push(setting_row(
            None,
            tr("Model for chat"),
            detail,
            state.menus.button(SetupMenu::Chat, label, Message::Menus),
        ));
    }
    rows.extend(suggested_row(state, data, AiSlot::Chat, there));
    match rows.is_empty() {
        true => {
            parts.push(sentence(tr("No model that can use tools is connected yet. Connect one on the page before.")))
        }
        false => parts.push(widgets::group(
            tr("Chat"),
            rows,
            Some(tr("Choose how hard it thinks later under Assistant.").to_string()),
        )),
    }
    page(parts)
}

/// A mark for one of the ways on the page Sorting: a ring, filled with a check where it is chosen.
fn radio<'a>(on: bool) -> Element<'a, Message> {
    let which = if on { Icon::CircleCheck } else { Icon::Circle };
    icon::icon(which, icon::SIZE)
        .style(move |t: &Theme, _| iced::widget::svg::Style {
            color: Some(if on { palette(t).text } else { palette(t).text_3 }),
        })
        .into()
}

/// The page Sorting: the four ways to sort mail – a decision model here (the suggestion), Jev, the chat
/// model, rules only – one row each, the one chosen marked.
fn ai_sort<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut parts = vec![
        title(tr("Sorting mail"), tr("A model reads each new mail and sorts it into categories.")),
        why(tr("Why this matters."), tr("Sorting sees every new mail. A model on this computer keeps your mail here.")),
    ];
    if super::ai_models_wait(data) {
        parts.push(sentence(tr("Checking decision models…")));
        return page(parts);
    }
    let on = super::mail_sorting_on(state, data);
    let decider = super::selected_mail_decider(state, data);
    let jev_chosen = decider.is_some_and(|(account, _)| ai_provider(account) == Some(AiProvider::Jev));
    let options = mail_model_choices(data);
    let local: Vec<&MailModelChoice> =
        options.iter().filter(|o| ai_provider(&o.account) != Some(AiProvider::Jev)).collect();
    let jev = options.iter().find(|o| ai_provider(&o.account) == Some(AiProvider::Jev));

    // A decision model on this computer or the network: the dropdown, or the suggestion to download.
    let chosen_local = on && !state.sort_by_chat && decider.is_some() && !jev_chosen;
    let control: Element<'a, Message> = match local.is_empty() {
        false => {
            let label = decider
                .filter(|_| !jev_chosen)
                .and_then(|(account, model)| local.iter().find(|o| o.account == *account && o.model == model.id))
                .map_or_else(|| tr("Choose").to_string(), |choice| choice.label.clone());
            state.menus.button(SetupMenu::SortModel, label, Message::Menus)
        }
        true => match ai_setup::suggested_model(&state.ai_setup, AiSlot::Decide) {
            Some(_) => {
                let can = ai_setup::can_use_suggestion(&state.ai_setup, data, true, Some(AiSlot::Decide));
                widgets::text_button(
                    None,
                    tr("Use"),
                    style::button::secondary,
                    can.then_some(Message::UseSuggested(AiSlot::Decide)),
                )
            }
            None => Space::new().into(),
        },
    };
    let suggestion = ai_setup::suggested_model(&state.ai_setup, AiSlot::Decide).map(|(account, candidate)| {
        let mut words =
            vec![candidate.model.clone(), tr("Suggested").to_string(), account_name(data, account).to_string()];
        words.extend(download(candidate.size_bytes));
        words.join(" · ")
    });
    let detail = suggestion.unwrap_or_else(|| tr("A small model made for sorting").to_string());
    let first_local = local.first().map(|o| Message::SortModel(o.account.clone(), o.model.clone()));
    let mut rows = vec![way(chosen_local, tr("Decision model"), detail, control, first_local)];

    // Jev (issue #323): a model elsewhere made for sorting; without an account its key first.
    let seat = AiProvider::Jev.seat().label();
    let jev_detail = trf("{seat} · Made for sorting", &[("seat", &seat)]);
    let (jev_control, jev_press): (Element<'a, Message>, Option<Message>) = match jev {
        Some(choice) => (Space::new().into(), Some(Message::SortModel(choice.account.clone(), choice.model.clone()))),
        None => (widgets::text_button(None, tr("Add key"), style::button::secondary, Some(Message::ConnectJev)), None),
    };
    rows.push(way(on && !state.sort_by_chat && jev_chosen, "Jev", jev_detail, jev_control, jev_press));

    // The chat model, answering in a fixed form.
    let chat = super::chat_choice(state, data);
    let chat_detail = match &chat {
        Some((_, model)) => trf("{model} · Sorts by answering in a fixed form · Slower", &[("model", model)]),
        None => tr("Choose a chat model first").to_string(),
    };
    let chat_press = chat.is_some().then_some(Message::SortByChat);
    rows.push(way(on && state.sort_by_chat, tr("Use the chat model"), chat_detail, Space::new(), chat_press));
    rows.push(way(
        !on,
        tr("Rules only"),
        tr("No AI. Sorts by sender and your rules").to_string(),
        Space::new(),
        Some(Message::SortMail(false)),
    ));
    let footer = match on && !state.sort_by_chat && jev_chosen {
        true => trf(
            "New mail goes to {provider}, only to be sorted. It is not used for chat.",
            &[("provider", &AiProvider::Jev.name())],
        ),
        false => {
            tr("Rules are checked first; the model uses category descriptions only when no rule matches.").to_string()
        }
    };
    parts.push(widgets::group(tr("Sort mail with"), rows, Some(footer)));
    page(parts)
}

/// One way on the page Sorting: the mark, its name, a line under it, a control at the end; the whole row
/// chooses it.
fn way<'a>(
    on: bool,
    name: impl iced::widget::text::IntoFragment<'a>,
    detail: String,
    control: impl Into<Element<'a, Message>>,
    press: Option<Message>,
) -> Element<'a, Message> {
    pressable(setting_row(Some(radio(on)), name, Some(detail), control), press)
}

/// A decision model for mail in its dropdown: chosen by account and model, shown as "provider · model".
#[derive(Debug, Clone, PartialEq, Eq)]
struct MailModelChoice {
    account: String,
    model: String,
    label: String,
}

/// The decision models the connected AI accounts offer for sorting mail, as the dropdown lists them.
fn mail_model_choices(data: &Data) -> Vec<MailModelChoice> {
    super::mail_deciders(data)
        .into_iter()
        .map(|(account, model)| MailModelChoice {
            account: account.clone(),
            model: model.id.clone(),
            label: format!("{} · {}", account_name(data, account), model.name),
        })
        .collect()
}

/// The open dropdown of the setup (issue #554): the chat models by account, or the decision models here;
/// the chosen one marked. From twelve entries on it has a search field.
pub(super) fn menu<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let key = state.menus.open()?;
    let sections: Vec<MenuSection<'a, Message>> = match key {
        SetupMenu::SortModel => {
            let chosen = super::selected_mail_decider(state, data).map(|(a, m)| (a.clone(), m.id.clone()));
            let items = mail_model_choices(data)
                .into_iter()
                .filter(|option| ai_provider(&option.account) != Some(AiProvider::Jev))
                .map(|option| {
                    let current = chosen.as_ref().is_some_and(|(a, m)| *a == option.account && *m == option.model);
                    MenuItem::plain(option.label, Message::SortModel(option.account, option.model)).current(current)
                })
                .collect();
            vec![MenuSection::new(items)]
        }
        SetupMenu::Chat => {
            let chosen = super::chat_choice(state, data);
            let suggested = ai_setup::suggested_model(&state.ai_setup, AiSlot::Chat);
            let models = super::chat_models(data);
            let mut accounts: Vec<&String> = models.iter().map(|(account, _)| *account).collect();
            accounts.dedup();
            accounts
                .into_iter()
                .map(|account| {
                    let items = models
                        .iter()
                        .filter(|(engine, _)| *engine == account)
                        .map(|(engine, model)| {
                            let current = chosen.as_ref().is_some_and(|(e, m)| e == *engine && *m == model.id);
                            let marked = suggested.is_some_and(|(a, c)| {
                                a == engine.as_str() && torro_settings_ui::accounts::same_model(&model.id, &c.model)
                            });
                            let name = match marked {
                                true => trf("{model} · Suggested", &[("model", &model.name)]),
                                false => model.name.clone(),
                            };
                            MenuItem::plain(name, Message::ChatModel((*engine).clone(), model.id.clone()))
                                .current(current)
                        })
                        .collect();
                    MenuSection::titled(account_name(data, account).to_string(), items)
                })
                .collect()
        }
    };
    Some(state.menus.view(key, sections, size::props(), Align::End, Message::Menus))
}

// ----- Appearance -------------------------------------------------------------------------------------

/// The building blocks of Settings › Appearance with the preview of the mail window beside them.
fn appearance<'a>(data: &'a Data, looks: &'a suite::State) -> Element<'a, Message> {
    page(vec![
        title(tr("Appearance"), tr("Choose how Torro looks. The preview changes as you choose.")),
        suite::setup_appearance(looks, &data.settings).map(Message::Suite),
        sentence(tr("Change any of this later in Settings › Appearance.")),
    ])
}

// ----- Documents --------------------------------------------------------------------------------------

/// The ArtCraft apps the point presents, by their catalog id, in this order, with their name and what each
/// is for.
pub(super) const APPS: [(&str, &str, &str); 4] = [
    ("wordcraft", "WordCraft", "Letters, reports, contracts"),
    ("deckcraft", "DeckCraft", "Slides and presentations"),
    ("gridcraft", "GridCraft", "Sheets and tables"),
    ("pdfcraft", "PdfCraft", "Fill in and combine PDFs"),
];

/// The catalog entries of the ArtCraft apps there are, in the order of [`APPS`].
pub(super) fn artcraft(data: &Data) -> Vec<&CatalogEntry> {
    let crate::ui::connections::Listing::Entries(entries) = &data.catalog else { return Vec::new() };
    APPS.iter().filter_map(|(id, _, _)| entries.iter().find(|entry| entry.id == *id)).collect()
}

/// Whether an ArtCraft app is set up as an MCP server here.
pub(super) fn artcraft_set_up(data: &Data) -> bool {
    APPS.iter().any(|(id, _, _)| crate::ui::connections::installed(data, id).is_some())
}

/// "Get ArtCraft": the releases of the first ArtCraft app the catalog has; `None` without a catalog.
pub(super) fn get_artcraft(data: &Data) -> Option<Message> {
    artcraft(data).first().map(|entry| Message::GetApp(entry.url.clone()))
}

/// Only a note (issue #326): Torro works with ArtCraft – its four apps, that Torro itself never edits
/// files, "Get ArtCraft", and where they are set up. It stores nothing.
fn documents(data: &Data) -> Element<'_, Message> {
    let rows = APPS
        .iter()
        .map(|(_, name, what)| {
            let tile = widgets::source_tile(TileFace::letter(name), TileSize::Medium, None);
            setting_row(Some(tile), *name, Some(tr(what).to_string()), Space::new())
        })
        .collect();
    let mut parts = vec![
        title(
            tr("Documents"),
            tr(
                "Torro works hand in hand with ArtCraft: the assistant can write documents, slides, sheets and PDFs for you.",
            ),
        ),
        widgets::group("ArtCraft", rows, None),
        why(
            tr("Torro itself never edits your files."),
            tr("ArtCraft works only in folders you allow and saves changes as new files."),
        ),
    ];
    if let Some(get) = get_artcraft(data) {
        parts.push(
            row![widgets::text_button(
                Some(Icon::ArrowUpRight),
                tr("Get ArtCraft"),
                style::button::secondary,
                Some(get)
            )]
            .into(),
        );
    }
    parts.push(sentence(tr("Installed already? Torro finds it, and you set it up in Settings › MCP servers.")));
    page(parts)
}

// ----- Done -------------------------------------------------------------------------------------------

/// The words of a colour mode.
fn mode_word(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::System => tr("System"),
        Appearance::Light => tr("Light"),
        Appearance::Dark => tr("Dark"),
    }
}

/// The words of a spacing level.
fn spacing_word(design: &str) -> &'static str {
    match design {
        "comfort" => tr("Comfort spacing"),
        "dense" => tr("Dense spacing"),
        _ => tr("Standard spacing"),
    }
}

/// One line per point with its state and a way on ("Open …", "Set up", "Get ArtCraft"), the quiet link
/// "Connect an agent…" in the footer, and "Finish".
fn done<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let installed = |app: AppId| {
        data.apps.as_ref().is_some_and(|apps| apps.iter().any(|a| a.window_class == app.window_class && a.installed))
    };
    let open = |label: &'static str, app: AppId| -> Element<'a, Message> {
        widgets::text_button(
            Some(Icon::ArrowUpRight),
            label,
            style::button::ghost,
            installed(app).then_some(Message::OpenApp(app.window_class)),
        )
    };
    let set_up = |step: Step| -> Element<'a, Message> {
        widgets::text_button(None, tr("Set up"), style::button::ghost, Some(Message::Go(step)))
    };
    let listed = || data.accounts.iter().filter(|account| !account.id.is_empty());

    let mail = listed().find(|account| account.kind == "mail");
    let mail_row = match mail {
        Some(account) => line(
            true,
            Step::Mail,
            format!("{} · {}", account.label, brings(account)),
            open(tr("Open Mail"), AppId::INBOX_MAIL),
        ),
        None => line(false, Step::Mail, tr("Not set up").to_string(), set_up(Step::Mail)),
    };
    let cloud = super::nextcloud_account(state, data);
    let cloud_row = match cloud {
        Some(account) => {
            let calendars = account.collections.iter().filter(|c| c.kind.has_events()).count();
            let lists = account.collections.iter().filter(|c| c.kind.has_todos()).count();
            let detail = trf(
                "Nextcloud · {host} · {calendars} calendars · {lists} lists",
                &[("host", &account.label), ("calendars", &calendars), ("lists", &lists)],
            );
            line(true, Step::Cloud, detail, open(tr("Open Calendar"), AppId::CALENDAR))
        }
        None => line(false, Step::Cloud, tr("Not set up").to_string(), set_up(Step::Cloud)),
    };
    let slots = &data.settings.ai_slots;
    let ai_row = match (slots.chat.is_some() || slots.decide.is_some(), ai_setup::preparing(&state.ai_setup)) {
        (_, true) => {
            line(true, Step::Ai, tr("Preparing model…").to_string(), open(tr("Open Assistant"), AppId::ASSISTANT))
        }
        (true, false) => {
            let model = |slot| data.settings.ai_slots.get(slot).and_then(|held| held.model.clone());
            let mut words = Vec::new();
            if let Some(chat) = model(AiSlot::Chat) {
                words.push(trf("{model} for chat", &[("model", &chat)]));
            }
            if let Some(decide) = model(AiSlot::Decide) {
                words.push(trf("{model} sorts mail", &[("model", &decide)]));
            }
            if words.is_empty() {
                words.push(tr("Connected").to_string());
            }
            line(true, Step::Ai, words.join(" · "), open(tr("Open Assistant"), AppId::ASSISTANT))
        }
        (false, false) => line(false, Step::Ai, tr("Not set up").to_string(), set_up(Step::Ai)),
    };
    let settings = &data.settings;
    let mut look = vec![mode_word(settings.appearance).to_string(), spacing_word(&settings.design).to_string()];
    // The colour scheme and the font only where they are not the suite's own (`torro`): a name, not an id.
    look.extend(
        [settings.color_scheme.clone(), settings.font.clone()]
            .into_iter()
            .filter(|word| !word.is_empty() && word != torro_core::design::STANDARD_ID),
    );
    let settings_button = widgets::text_button(
        Some(Icon::ArrowUpRight),
        tr("Open Settings"),
        style::button::ghost,
        Some(Message::OpenSection(Section::Appearance)),
    );
    let look_row = line(true, Step::Appearance, look.join(" · "), settings_button);
    let documents_row = match artcraft_set_up(data) {
        true => line(true, Step::Documents, tr("ArtCraft is set up").to_string(), Space::new()),
        false => {
            let get = widgets::text_button(
                Some(Icon::ArrowUpRight),
                tr("Get ArtCraft"),
                style::button::ghost,
                get_artcraft(data),
            );
            line(false, Step::Documents, tr("ArtCraft not installed yet").to_string(), get)
        }
    };
    page(vec![
        title(tr("You're all set"), tr("This is what Torro is connected to now. Change any of it later in Settings.")),
        widgets::group(tr("Your setup"), vec![mail_row, cloud_row, ai_row, look_row, documents_row], None),
    ])
}

/// A line of the summary: a check mark or a dashed ring, the point's name, its state under it, a way on.
fn line<'a>(set: bool, step: Step, detail: String, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let face = if set { TileFace::Icon(Icon::Check) } else { TileFace::Icon(Icon::CircleDashed) };
    let tile = widgets::source_tile(face, TileSize::Medium, None);
    setting_row(Some(tile), step.label(), Some(detail), control)
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
