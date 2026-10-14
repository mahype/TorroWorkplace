//! Assistant (task C.6 in `docs/tasks/assistent.md`, concept `docs/assistent.md`, "Rechte" and
//! decisions 3, 5, 7 and 8): what the assistant may use, the engines it answers with and how long it
//! keeps conversations. Opened from the sidebar, from an agent list's "Assistant" row, and directly
//! with `torroworkplace ui --section assistant` – the way the chat's hint "The assistant has no
//! access to Mail. Change…" gets here.
//!
//! The assistant is one client of the kind `assistant` for every engine (decision 5,
//! `api::ASSISTANT_CLIENT`). The page has two states:
//!
//! - **First start** (no such client yet): "What the assistant may use" offered with
//!   `Rights::assistant()` – read only for calendars, tasks and the inbox (decision 3) – in the
//!   same rights view as an agent's detail page (`agents::rights_groups`), with the templates and
//!   that suggestion as starting points. "Turn on assistant" (the header's primary button, so it
//!   stays in sight while the rights are chosen) sends `clients.create_client` (kind
//!   `assistant`, template *Read only*, the narrowest) and, once the core answered with the new
//!   client, `clients.update_client` with the chosen rights. The key in that answer is dropped:
//!   the core runs the assistant's tools itself with the client's rights (engine A,
//!   `Core::handle_as_assistant`), nobody needs it, and the agents page never sees it.
//! - **On**: the status with "Turn off assistant…" behind a confirmation (`clients.remove_client`;
//!   its conversations stay); the engines (the AI accounts, read from `accounts.list_accounts`);
//!   the same rights view for the client, saved like an agent's ([`agents::SAVE_DELAY`] after the
//!   last change, `clients.update_client`); "Keep conversations"
//!   (`settings.set_setting assistant_retention`: 30 days, 90 days, a year or forever, plus a
//!   number of days set elsewhere).
//!
//! The area `assistant` has no row for the assistant's own client ([`agents::shows_area`]): the core
//! refuses it that area whatever its rights say (`api::loops_back`), so a switch would promise what
//! never happens. Rights apply from the assistant's next tool call, also in a conversation that is
//! running: the core reads the client's rights anew for every call.
//!
//! There is no setting for a default engine: each chat picks its engine, and a new chat starts with
//! the one used last (`docs/assistent.md`, "Woher das Modell kommt"). A key for it would be a new
//! `torro_ipc::SettingKey`.
//!
//! Profile and memory (task C.10) are pages below this one, opened from the rows under the status
//! ([`persona`]): the page here only hands them their messages, header, dialog and view.

use chrono::Utc;
use iced::widget::{Space, column, responsive, svg};
use iced::{Element, Length, Subscription, Theme};
use torro_core::i18n::{tr, trf};
use torro_core::settings::Retention;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::style;
use torro_gui::theme::palette;
use torro_gui::widgets::{self, Status};
use torro_ipc::api::{ASSISTANT_CLIENT, Rights, RightsTemplate};
use torro_ipc::{ClientInfo, Request, Response, SettingKey};

use super::accounts::{Tone, status_line};
use super::general::choice;
use super::{Action, Data, Header, Section, chevron, confirm, pressable, setting_row};
use super::{agent_wizard, agents, persona};

/// The name of the assistant's client, as the agents list and the audit log show it (decision 5:
/// one client "Assistant").
pub const CLIENT_NAME: &str = "Assistant";

/// The retention times the page offers, shortest first; a number of days set through the API that
/// is not among them is listed too ([`retention_options`]).
const RETENTIONS: [Retention; 4] = [Retention::Days(30), Retention::Days(90), Retention::Days(365), Retention::Forever];

/// Page state.
#[derive(Debug)]
pub struct State {
    /// What the first start offers until "Turn on assistant": [`Rights::assistant`], then what the
    /// person changed.
    choice: Rights,
    /// The text in the first start's "Add repository".
    repo_input: String,
    /// The rights of the assistant's client once it exists, edited and saved like an agent's: an
    /// agents page state with the assistant's client selected.
    editor: agents::State,
    /// "Turn on assistant" was sent with these rights; they follow once the core created the client.
    creating: Option<Rights>,
    /// "Turn off assistant…" asks first.
    confirm_off: bool,
    /// The pages Profile and Memory below this one (task C.10).
    persona: persona::State,
}

impl Default for State {
    fn default() -> Self {
        State {
            choice: Rights::assistant(),
            repo_input: String::new(),
            editor: agents::State::default(),
            creating: None,
            confirm_off: false,
            persona: persona::State::default(),
        }
    }
}

/// A starting point of the first start's rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// [`Rights::assistant`]: read calendars, tasks and the inbox.
    Suggested,
    Template(RightsTemplate),
}

impl Preset {
    const ALL: [Preset; 4] = [
        Preset::Suggested,
        Preset::Template(RightsTemplate::ReadOnly),
        Preset::Template(RightsTemplate::ReadAndWrite),
        Preset::Template(RightsTemplate::FullAccess),
    ];

    /// The rights it stands for. No template opens the area `assistant` (`Area::in_templates`).
    pub fn rights(self) -> Rights {
        match self {
            Preset::Suggested => Rights::assistant(),
            Preset::Template(template) => template.rights(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Preset::Suggested => tr("Suggested"),
            Preset::Template(template) => agents::template_label(template),
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Preset::Suggested => tr("Reads calendars, tasks and the inbox"),
            Preset::Template(template) => agent_wizard::template_detail(template),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A control of the rights view (an agents page message), and the clock that saves the
    /// assistant's changed rights ([`subscription`]).
    Rights(agents::Message),
    /// A starting point of the first start.
    Preset(Preset),
    /// "Turn on assistant".
    TurnOn,
    /// A row of "Keep conversations".
    Retention(Retention),
    /// An engine's row, or "No AI account yet": the Accounts page.
    OpenAccounts,
    /// "Turn off assistant…", then its confirmation or "Cancel".
    AskTurnOff,
    TurnOff,
    CloseDialog,
    /// The pages Profile and Memory and their rows here (task C.10).
    Persona(persona::Message),
}

/// The assistant's client as the core lists it; `None` before it was turned on.
pub fn assistant_client(data: &Data) -> Option<&ClientInfo> {
    data.clients.iter().find(|client| client.kind == ASSISTANT_CLIENT)
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Rights(message) => rights(state, message, data),
        Message::Preset(preset) => {
            if assistant_client(data).is_none() {
                state.choice = preset.rights();
            }
            Action::None
        }
        Message::TurnOn => {
            if assistant_client(data).is_some() || state.creating.is_some() {
                return Action::None;
            }
            state.creating = Some(state.choice.clone());
            // The narrowest template; the chosen rights follow as soon as the client exists
            // ([`answered`]), since `clients.create_client` takes a template only.
            Action::Send(Request::CreateClient {
                name: CLIENT_NAME.into(),
                kind: ASSISTANT_CLIENT.into(),
                template: RightsTemplate::ReadOnly,
                agent: None,
            })
        }
        Message::Retention(retention) => {
            Action::Send(Request::SetSetting { key: SettingKey::AssistantRetention, value: retention.to_value() })
        }
        Message::OpenAccounts => Action::Select(Section::Accounts),
        Message::AskTurnOff => {
            state.confirm_off = assistant_client(data).is_some();
            Action::None
        }
        Message::TurnOff => {
            state.confirm_off = false;
            let Some(client) = assistant_client(data) else { return Action::None };
            // Its unsaved rights go with it; turned on again, it starts from the suggestion.
            *state = State::default();
            Action::Send(Request::RemoveClient { id: client.id.clone() })
        }
        Message::CloseDialog => {
            state.confirm_off = false;
            Action::None
        }
        Message::Persona(message) => persona::update(&mut state.persona, message, data),
    }
}

/// A control of the rights view: on the assistant's client through the agents page's draft and
/// autosave, before it exists on the first start's choice. Other agents page messages are not the
/// rights view's and change nothing here.
fn rights(state: &mut State, message: agents::Message, data: &Data) -> Action {
    use agents::Message as M;
    let control = matches!(
        message,
        M::Level(..) | M::Admin(..) | M::Confirm(_) | M::Outbound(_) | M::All(..) | M::Tick(..) | M::AddRepo
    );
    match (assistant_client(data), message) {
        (_, M::Clock(now)) => agents::update(&mut state.editor, M::Clock(now), data),
        (Some(client), message) if control || matches!(message, M::RepoInput(_)) => {
            state.editor.selected = Some(client.id.clone());
            agents::update(&mut state.editor, message, data)
        }
        (None, M::RepoInput(text)) => {
            state.repo_input = text;
            Action::None
        }
        (None, message) if control => {
            agents::change_rights(&mut state.choice, ASSISTANT_CLIENT, message, data, &mut state.repo_input);
            Action::None
        }
        _ => Action::None,
    }
}

/// The answer to a request the shell sent; the shell calls this before the agents page's.
///
/// The new client after "Turn on assistant" ([`Response::ClientKey`] of the kind `assistant`):
/// its key is dropped and the chosen rights go out at once ([`agents::save_rights`], so the page
/// shows them, not the template's, until the reload). Returns `Some` with that request; the shell
/// then hands the answer to nobody else. An error ends "Turn on assistant" (the shell shows it).
/// Every other answer goes to the rights editor (`ok` of a saved change, an error that drops it).
pub fn answered(state: &mut State, result: &Result<Response, String>) -> Option<Action> {
    persona::answered(&mut state.persona, result);
    if state.creating.is_some() {
        match result {
            Ok(Response::ClientKey { client, .. }) if client.kind == ASSISTANT_CLIENT => {
                let rights = state.creating.take()?;
                return Some(agents::save_rights(&mut state.editor, client.id.clone(), rights));
            }
            Err(_) => state.creating = None,
            Ok(_) => {}
        }
    }
    // Only the rights editor's own answers matter to it; a key never reaches it.
    if !matches!(result, Ok(Response::ClientKey { .. })) {
        let _ = agents::answered(&mut state.editor, result);
    }
    None
}

/// The shell loaded [`Data`] anew: saved rights are in it now, and the stored profile and notes.
pub fn loaded(state: &mut State, data: &Data) {
    agents::loaded(&mut state.editor);
    persona::loaded(&mut state.persona, data);
}

/// The clock that saves changed rights; none while nothing waits.
pub fn subscription(state: &State) -> Subscription<Message> {
    agents::subscription(&state.editor).map(Message::Rights)
}

/// What Escape means on this page: close the confirmation; on Profile and Memory what they say.
pub fn escape(state: &State) -> Option<Message> {
    match persona::escape(&state.persona) {
        Some(message) => Some(Message::Persona(message)),
        None => state.confirm_off.then_some(Message::CloseDialog),
    }
}

/// "Turn off the assistant?" while it is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(dialog) = persona::overlay(&state.persona, data) {
        return Some(dialog.map(Message::Persona));
    }
    if !state.confirm_off || assistant_client(data).is_none() {
        return None;
    }
    Some(confirm(
        tr("Turn off the assistant?"),
        tr(
            "It loses access to everything at once. Your conversations stay. Turn it on again to choose what it may use.",
        )
        .to_string(),
        tr("Turn off"),
        Message::TurnOff,
        Message::CloseDialog,
    ))
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The rights the page shows: the client's (or its unsaved change), before it exists the first
/// start's choice.
fn shown<'a>(state: &'a State, data: &'a Data) -> &'a Rights {
    match assistant_client(data) {
        Some(client) => agents::shown_rights(&state.editor, client),
        None => &state.choice,
    }
}

/// The rows of "Keep conversations": [`RETENTIONS`], and `current` among them in order when it is
/// another number of days (set through the API).
pub fn retention_options(current: Retention) -> Vec<Retention> {
    let mut options = RETENTIONS.to_vec();
    if !options.contains(&current) {
        let at = options.iter().position(|option| option.days().is_none_or(|days| Some(days) > current.days()));
        options.insert(at.unwrap_or(options.len()), current);
    }
    options
}

/// "30 days", "1 year", "Forever".
pub fn retention_label(retention: Retention) -> String {
    match retention {
        Retention::Days(365) => tr("1 year").to_string(),
        Retention::Days(days) => trf("{n} days", &[("n", &days)]),
        Retention::Forever => tr("Forever").to_string(),
    }
}

// ----- Views --------------------------------------------------------------------------------

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    if let Some(sub) = persona::view(&state.persona, data) {
        return sub.map(Message::Persona);
    }
    responsive(move |size| page(state, data, agents::control_column(size.width))).height(Length::Shrink).into()
}

/// The page in the width it has: its controls in `column` ([`agents::control_column`]).
fn page<'a>(state: &'a State, data: &'a Data, column: Option<f32>) -> Element<'a, Message> {
    let rights = shown(state, data);
    let repo_input = match assistant_client(data) {
        Some(_) => agents::repo_input(&state.editor),
        None => state.repo_input.as_str(),
    };
    let groups =
        agents::rights_groups(repo_input, rights, data, column, ASSISTANT_CLIENT, tr("What the assistant may use"))
            .into_iter()
            .map(|group| group.map(Message::Rights));
    let mut page = column![].spacing(layout::section_gap());
    match assistant_client(data) {
        None => {
            page = page.push(off_group()).push(preset_group(rights)).extend(groups);
        }
        // Status and engines first, then the rights – the reason the chat's hint opens this page –
        // and the retention, which is set once and rarely again.
        Some(client) => {
            page = page
                .push(on_group(client, column))
                .push(persona::profile_rows(data).map(Message::Persona))
                .push(engines_group(data))
                .extend(groups)
                .push(retention_group(data.settings.assistant_retention));
        }
    }
    page.into()
}

/// The header bar: before the assistant is on, "Turn on assistant" – the window's one primary
/// button, where it stays in sight while the rights below are chosen; busy while the core creates
/// the client. Only its icon in narrow windows.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    if let Some(sub) = persona::header(&state.persona, class) {
        return Header {
            sub_page: sub.sub_page.map(|(name, back)| (name, Message::Persona(back))),
            actions: sub.actions.into_iter().map(|action| action.map(Message::Persona)).collect(),
        };
    }
    if assistant_client(data).is_some() {
        return Header::default();
    }
    let label = tr("Turn on assistant");
    let button = match (state.creating.is_some(), class) {
        (true, _) => widgets::busy_button(tr("Turning on…"), style::button::primary),
        (false, SizeClass::Wide | SizeClass::Medium) => {
            widgets::text_button(Some(Icon::Plus), label, style::button::primary, Some(Message::TurnOn))
        }
        (false, SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip) => {
            widgets::compact_button(Icon::Plus, label, style::button::primary, Some(Message::TurnOn))
        }
    };
    Header { sub_page: None, actions: vec![button] }
}

/// The first start's head: the assistant is off and what to do about it.
fn off_group<'a>() -> Element<'a, Message> {
    let detail = tr("Choose what it may use below, then turn it on.");
    let row = setting_row(None, tr("The assistant is off"), Some(detail.to_string()), Space::new());
    let footer =
        tr("It chats with you and works in Torro Workplace with only what you allow here. You can change it any time.");
    widgets::group(tr("Status"), vec![row], Some(footer.to_string()))
}

/// The head while the assistant is on: since when it was last used, and "Turn off assistant…".
fn on_group<'a>(client: &ClientInfo, column: Option<f32>) -> Element<'a, Message> {
    let off = widgets::text_button(None, tr("Turn off assistant…"), style::button::danger, Some(Message::AskTurnOff));
    let used = agents::last_used_text(client.last_used, Utc::now());
    let row = setting_row(None, tr("The assistant is on"), Some(used), agents::in_column(off, column));
    widgets::group(tr("Status"), vec![row], None)
}

/// The starting points of the first start, with a check at the one the rights equal.
fn preset_group<'a>(rights: &Rights) -> Element<'a, Message> {
    let rows = Preset::ALL
        .into_iter()
        .map(|preset| {
            let mark: Element<'a, Message> = match preset.rights() == *rights {
                true => icon::icon(Icon::Check, icon::SIZE)
                    .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) })
                    .into(),
                false => Space::new().width(icon::SIZE).into(),
            };
            let row = setting_row(None, preset.label(), Some(preset.detail().to_string()), mark);
            pressable(row, Some(Message::Preset(preset)))
        })
        .collect();
    let footer = tr("A starting point: every row below can be changed. None of them sends mail without asking.");
    widgets::group(tr("Start from"), rows, Some(footer.to_string()))
}

/// The engines: each AI account with its state, or where to add one. A row opens the Accounts page.
fn engines_group<'a>(data: &'a Data) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = data
        .accounts
        .iter()
        .filter(|account| account.kind == "ai" && !account.id.is_empty())
        .map(|account| {
            let line = status_line(&account.state);
            let status = match line.tone {
                Tone::Ok => Status::Ok,
                Tone::Attention => Status::Attention,
                Tone::Error => Status::Error,
            };
            let row =
                setting_row(Some(widgets::status_dot(status)), account.label.as_str(), Some(line.text), chevron());
            pressable(row, Some(Message::OpenAccounts))
        })
        .collect();
    if rows.is_empty() {
        let detail = tr("Add Ollama on this computer or OpenRouter under Accounts");
        let row = setting_row(None, tr("No AI account yet"), Some(detail.to_string()), chevron());
        rows.push(pressable(row, Some(Message::OpenAccounts)));
    }
    let footer = tr(
        "The assistant answers with the models of these accounts. Each chat picks one; a new chat starts with the one you used last.",
    );
    widgets::group(tr("Engines"), rows, Some(footer.to_string()))
}

/// "Keep conversations": a row per retention time, a check at the stored one.
fn retention_group<'a>(current: Retention) -> Element<'a, Message> {
    let rows = retention_options(current)
        .into_iter()
        .map(|retention| choice(retention_label(retention), retention == current, Message::Retention(retention)))
        .collect();
    let footer = tr("Conversations are deleted this long after their last message. Pinned ones are kept.");
    widgets::group(tr("Keep conversations"), rows, Some(footer.to_string()))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use iced::time::Instant;
    use torro_ipc::api::{Area, AreaRights, Level, Selection};
    use torro_ipc::{IssuedKey, Key};

    use super::*;

    fn assistant(rights: Rights) -> ClientInfo {
        ClientInfo {
            id: "a51c3e88".into(),
            name: CLIENT_NAME.into(),
            kind: ASSISTANT_CLIENT.into(),
            created: Utc::now(),
            last_used: None,
            rights,
            agent: None,
        }
    }

    fn with_assistant(rights: Rights) -> Data {
        Data { clients: vec![assistant(rights)], ..Data::default() }
    }

    /// Runs the clock past the delay and returns the request it sends.
    fn saved(state: &mut State, data: &Data) -> Action {
        let later = Instant::now() + agents::SAVE_DELAY + agents::SAVE_DELAY;
        update(state, Message::Rights(agents::Message::Clock(later)), data)
    }

    #[test]
    fn turning_on_creates_the_client_and_then_sends_the_chosen_rights() {
        let off = Data::default();
        let mut state = State::default();
        assert_eq!(shown(&state, &off), &Rights::assistant(), "decision 3: read calendars, tasks, inbox");

        let action = update(&mut state, Message::TurnOn, &off);
        let Action::Send(Request::CreateClient { name, kind, template, agent }) = action else {
            panic!("expected clients.create_client, got {action:?}");
        };
        assert_eq!(
            (name.as_str(), kind.as_str(), template, agent),
            (CLIENT_NAME, ASSISTANT_CLIENT, RightsTemplate::ReadOnly, None)
        );
        assert!(matches!(update(&mut state, Message::TurnOn, &off), Action::None), "sent once");

        let key = IssuedKey::new(Key::new("torro_a51c3e88_00ff"));
        let created = assistant(RightsTemplate::ReadOnly.rights());
        let next = answered(&mut state, &Ok(Response::ClientKey { client: created.clone(), key }));
        let Some(Action::Send(Request::UpdateClient { id, rights })) = next else {
            panic!("expected clients.update_client, got {next:?}");
        };
        assert_eq!((id.as_str(), &rights), ("a51c3e88", &Rights::assistant()));
        // Until the reload the page shows the chosen rights, not the template's.
        let on = Data { clients: vec![created], ..Data::default() };
        assert_eq!(shown(&state, &on), &Rights::assistant());
        assert!(answered(&mut state, &Ok(Response::Ok)).is_none());
        loaded(&mut state, &on);
        assert_eq!(shown(&state, &on), &RightsTemplate::ReadOnly.rights(), "then what the core holds");
    }

    #[test]
    fn the_first_start_changes_its_choice_and_turns_on_with_it() {
        let off = Data::default();
        let mut state = State::default();
        let _ = update(&mut state, Message::Rights(agents::Message::Level(Area::Mail, Level::Read)), &off);
        let _ = update(&mut state, Message::Rights(agents::Message::Level(Area::Assistant, Level::Full)), &off);
        assert_eq!(shown(&state, &off).area(Area::Mail).level, Level::Read);
        assert_eq!(shown(&state, &off).area(Area::Assistant), AreaRights::NONE, "never its own area");
        let _ = update(&mut state, Message::TurnOn, &off);
        let key = IssuedKey::new(Key::new("torro_a51c3e88_00ff"));
        let client = assistant(RightsTemplate::ReadOnly.rights());
        let Some(Action::Send(Request::UpdateClient { rights, .. })) =
            answered(&mut state, &Ok(Response::ClientKey { client, key }))
        else {
            panic!("the chosen rights follow");
        };
        assert_eq!(rights.area(Area::Mail).level, Level::Read);

        let mut preset = State::default();
        let _ = update(&mut preset, Message::Preset(Preset::Template(RightsTemplate::FullAccess)), &off);
        assert_eq!(shown(&preset, &off), &RightsTemplate::FullAccess.rights());
        assert_eq!(shown(&preset, &off).area(Area::Assistant), AreaRights::NONE, "no template opens it");
    }

    #[test]
    fn a_failed_turn_on_can_be_tried_again_and_shows_no_key_anywhere() {
        let off = Data::default();
        let mut state = State::default();
        let _ = update(&mut state, Message::TurnOn, &off);
        assert!(answered(&mut state, &Err("conflict".into())).is_none(), "the shell shows the error");
        assert!(state.creating.is_none());
        assert!(matches!(update(&mut state, Message::TurnOn, &off), Action::Send(Request::CreateClient { .. })));
        // Another page's new key is not the assistant's.
        let key = IssuedKey::new(Key::new("torro_s1_00ff"));
        let script = ClientInfo { kind: "script".into(), ..assistant(RightsTemplate::ReadOnly.rights()) };
        assert!(answered(&mut state, &Ok(Response::ClientKey { client: script, key })).is_none());
    }

    #[test]
    fn editing_the_assistants_rights_sends_update_client() {
        let data = with_assistant(Rights::assistant());
        let mut state = State::default();
        let _ = update(&mut state, Message::Rights(agents::Message::Level(Area::Tasks, Level::Write)), &data);
        let _ = update(&mut state, Message::Rights(agents::Message::All(agents::Pick::Calendars, false)), &data);
        assert_eq!(shown(&state, &data).area(Area::Tasks).level, Level::Write, "shown at once");
        let Action::Send(Request::UpdateClient { id, rights }) = saved(&mut state, &data) else {
            panic!("expected clients.update_client");
        };
        assert_eq!(id, "a51c3e88");
        assert_eq!(rights.area(Area::Tasks).level, Level::Write);
        assert_eq!(rights.calendars, Selection::Only(Vec::new()), "no calendars known: none ticked");
        assert_eq!(rights.area(Area::Inbox).level, Level::Read, "the rest unchanged");
        assert_eq!(subscription(&state).units(), 0, "nothing waits any more");
    }

    #[test]
    fn the_assistant_area_is_hidden_and_unchangeable_for_the_assistant() {
        assert!(!agents::shows_area(Area::Assistant, ASSISTANT_CLIENT));
        assert!(agents::shows_area(Area::Assistant, "claude-code"), "other agents may get it, switched on by hand");
        assert!(agents::shows_area(Area::Calendar, ASSISTANT_CLIENT));
        let data = with_assistant(Rights::assistant());
        let mut state = State::default();
        for message in
            [agents::Message::Level(Area::Assistant, Level::Full), agents::Message::Admin(Area::Assistant, true)]
        {
            let _ = update(&mut state, Message::Rights(message), &data);
        }
        assert_eq!(subscription(&state).units(), 0, "nothing to save");
        assert!(matches!(saved(&mut state, &data), Action::None));
    }

    #[test]
    fn keeping_conversations_sends_set_setting() {
        let data = with_assistant(Rights::assistant());
        let mut state = State::default();
        for (retention, value) in
            [(Retention::Days(90), serde_json::json!(90)), (Retention::Forever, serde_json::json!("forever"))]
        {
            let action = update(&mut state, Message::Retention(retention), &data);
            let Action::Send(Request::SetSetting { key, value: sent }) = action else {
                panic!("expected settings.set_setting, got {action:?}");
            };
            assert_eq!((key, sent), (SettingKey::AssistantRetention, value));
        }
    }

    #[test]
    fn retention_rows_include_a_number_set_elsewhere_in_order() {
        assert_eq!(retention_options(Retention::Days(30)), RETENTIONS);
        let custom = retention_options(Retention::Days(14));
        assert_eq!(custom[0], Retention::Days(14));
        let long = retention_options(Retention::Days(500));
        assert_eq!(long[3], Retention::Days(500), "after a year, before forever: {long:?}");
        assert_eq!(long[4], Retention::Forever);
        assert_eq!(retention_label(Retention::Days(30)), trf("{n} days", &[("n", &30)]));
        assert_eq!(retention_label(Retention::Days(365)), tr("1 year"));
        assert_eq!(retention_label(Retention::Forever), tr("Forever"));
    }

    #[test]
    fn turning_off_is_confirmed_and_removes_the_client() {
        let data = with_assistant(Rights::assistant());
        let mut state = State::default();
        let _ = update(&mut state, Message::AskTurnOff, &data);
        assert!(overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::CloseDialog)));
        let action = update(&mut state, Message::TurnOff, &data);
        assert!(matches!(action, Action::Send(Request::RemoveClient { ref id }) if id == "a51c3e88"));
        assert!(overlay(&state, &data).is_none());
        assert_eq!(state.choice, Rights::assistant(), "turned on again, it starts from the suggestion");

        let mut off = State::default();
        let _ = update(&mut off, Message::AskTurnOff, &Data::default());
        assert!(overlay(&off, &Data::default()).is_none(), "nothing to turn off");
    }

    #[test]
    fn turn_on_sits_in_the_header_until_the_assistant_is_on() {
        let state = State::default();
        for class in [SizeClass::Wide, SizeClass::Narrow] {
            assert_eq!(header(&state, &Data::default(), class).actions.len(), 1, "{class:?}");
        }
        assert!(header(&state, &with_assistant(Rights::assistant()), SizeClass::Wide).actions.is_empty());
    }

    #[test]
    fn the_page_builds_in_every_state() {
        let mut state = State::default();
        drop(view(&state, &Data::default()));
        for (pick, column) in
            agents::Pick::ALL.into_iter().zip([None, Some(agents::segment_slot())].into_iter().cycle())
        {
            let _ = update(&mut state, Message::Rights(agents::Message::All(pick, false)), &Data::default());
            drop(page(&state, &Data::default(), column));
        }
        let _ = update(&mut state, Message::TurnOn, &Data::default());
        drop(page(&state, &Data::default(), None));
        let data = with_assistant(RightsTemplate::FullAccess.rights());
        for column in [None, Some(agents::segment_slot())] {
            drop(page(&State::default(), &data, column));
        }
    }
}
