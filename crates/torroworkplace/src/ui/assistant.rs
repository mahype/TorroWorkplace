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
//!
//! **Voice** (task C.9, `docs/assistent.md`, "Sprache"), once the assistant is on: talking by voice
//! on or off (`settings.set_setting voice_enabled`; only here and in the tray, since only the app
//! key may switch the microphone on), the fixed wake word, "Speak answers", the microphone when the
//! recording program can list them, and "Say something" (`settings.test_voice`), which shows what
//! was understood. Below, the four programs it needs and whether each was found, with a hint how
//! to install what is missing (`settings.voice_status`). The status is asked once when the
//! control centre starts, after every change of a voice setting, and a few times after voice was
//! switched, until the listener reports what it does ([`subscription`]).

use std::time::Duration;

use chrono::Utc;
use iced::widget::{Space, column, responsive, svg, toggler};
use iced::{Element, Length, Subscription, Theme};
use torro_core::i18n::{tr, trf};
use torro_core::settings::Retention;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::style;
use torro_gui::theme::palette;
use torro_gui::widgets::{self, Status};
use torro_ipc::api::{ASSISTANT_CLIENT, Rights, RightsTemplate};
use torro_ipc::{
    ClientInfo, Request, Response, SettingKey, VoiceMissing, VoicePart, VoiceProgram, VoiceState, VoiceStatus,
};

use super::accounts::{Tone, status_line};
use super::general::choice;
use super::{Action, Data, Header, Section, chevron, confirm, pressable, setting_row};
use super::{agent_wizard, agents, persona};

/// The name of the assistant's client, as the agents list and the audit log show it (decision 5:
/// one client "Assistant").
pub const CLIENT_NAME: &str = "Assistant";

/// How often the voice status is asked while it is not known or voice was just switched.
const VOICE_POLL: Duration = Duration::from_secs(2);

/// How many times the voice status is asked after voice was switched, until the listener says it
/// runs (or stopped).
const VOICE_POLLS: u8 = 5;

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
    /// Talking by voice (task C.9).
    voice: Voice,
}

/// What the page knows of talking by voice.
#[derive(Debug, Default)]
struct Voice {
    /// The last answer of `settings.voice_status`; `None` until one came.
    status: Option<VoiceStatus>,
    /// `settings.voice_status` is out.
    asking: bool,
    /// Asking failed; asked again only with "Check again".
    failed: bool,
    /// How many more times the status is asked after voice was switched to `expected`.
    polls: u8,
    /// Whether voice was last switched on (`Some(true)`) or off, until the listener follows.
    expected: Option<bool>,
    /// A voice setting was sent: the status is asked again once it is stored.
    changed: bool,
    /// "Say something" is recording.
    testing: bool,
    /// What the last test heard.
    heard: Option<Heard>,
}

/// The outcome of "Say something".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// These words.
    Text(String),
    /// Nothing understood.
    Nothing,
    /// The test failed, with the core's message.
    Failed(String),
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
            voice: Voice::default(),
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
    /// Ask how voice stands (`settings.voice_status`): on the clock, or "Check again".
    CheckVoice,
    /// "Talk to the assistant" on or off.
    Voice(bool),
    /// "Speak answers" on or off.
    SpeakAnswers(bool),
    /// A microphone's row; `None` for the system's default.
    Microphone(Option<String>),
    /// "Say something".
    TestVoice,
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
        Message::CheckVoice => {
            let voice = &mut state.voice;
            if voice.asking {
                return Action::None;
            }
            voice.asking = true;
            voice.failed = false;
            voice.polls = voice.polls.saturating_sub(1);
            Action::Send(Request::VoiceStatus)
        }
        Message::Voice(on) => {
            // Only switching on needs the programs; off is always possible.
            if on && state.voice.status.as_ref().is_some_and(|status| !status.can_listen()) {
                return Action::None;
            }
            state.voice.expected = Some(on);
            state.voice.polls = VOICE_POLLS;
            set_voice(state, SettingKey::VoiceEnabled, serde_json::Value::Bool(on))
        }
        Message::SpeakAnswers(speak) => set_voice(state, SettingKey::SpeakAnswers, serde_json::Value::Bool(speak)),
        Message::Microphone(name) => {
            set_voice(state, SettingKey::Microphone, name.map_or(serde_json::Value::Null, serde_json::Value::String))
        }
        Message::TestVoice => {
            if state.voice.testing {
                return Action::None;
            }
            state.voice.testing = true;
            state.voice.heard = None;
            Action::Send(Request::TestVoice)
        }
    }
}

/// Sends a voice setting; the status is asked again once it is stored ([`answered`]).
fn set_voice(state: &mut State, key: SettingKey, value: serde_json::Value) -> Action {
    state.voice.changed = true;
    Action::Send(Request::SetSetting { key, value })
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
    if let Some(next) = voice_answered(&mut state.voice, result) {
        return next;
    }
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

/// The answers of the voice section: `Some` when `result` was one of its own (the status, a
/// transcript), with what to send next; `None` to hand it on. After a voice setting was stored the
/// status is asked again. An error ends a running test or check; the shell shows it.
fn voice_answered(voice: &mut Voice, result: &Result<Response, String>) -> Option<Option<Action>> {
    match result {
        Ok(Response::Voice { voice: status }) => {
            voice.asking = false;
            let follows = match voice.expected {
                Some(true) => status.listener != VoiceState::Off || status.problem.is_some(),
                Some(false) => status.listener == VoiceState::Off,
                None => true,
            };
            if follows {
                voice.polls = 0;
                voice.expected = None;
            }
            voice.status = Some(status.clone());
            Some(None)
        }
        Ok(Response::Transcript { text }) => {
            voice.testing = false;
            voice.heard =
                Some(if text.trim().is_empty() { Heard::Nothing } else { Heard::Text(text.trim().to_string()) });
            Some(None)
        }
        Ok(Response::Ok) if voice.changed && !voice.asking => {
            voice.changed = false;
            voice.asking = true;
            Some(Some(Action::Send(Request::VoiceStatus)))
        }
        Err(message) if voice.testing => {
            voice.testing = false;
            voice.heard = Some(Heard::Failed(message.clone()));
            None
        }
        Err(_) if voice.asking => {
            voice.asking = false;
            voice.failed = true;
            voice.polls = 0;
            None
        }
        _ => None,
    }
}

/// The shell loaded [`Data`] anew: saved rights are in it now, and the stored profile and notes.
pub fn loaded(state: &mut State, data: &Data) {
    agents::loaded(&mut state.editor);
    persona::loaded(&mut state.persona, data);
}

/// The clock that saves changed rights, and the one that asks how voice stands: once at the start
/// and a few times after voice was switched. None while nothing waits.
pub fn subscription(state: &State) -> Subscription<Message> {
    let rights = agents::subscription(&state.editor).map(Message::Rights);
    let voice = &state.voice;
    let unknown = voice.status.is_none() && !voice.failed;
    if voice.asking || !(unknown || voice.polls > 0) {
        return rights;
    }
    Subscription::batch([rights, iced::time::every(VOICE_POLL).map(|_| Message::CheckVoice)])
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
        // Status, who the assistant is (profile and memory) and its engines first, then the rights –
        // the reason the chat's hint opens this page –, voice, and the retention, which is set once
        // and rarely again.
        Some(client) => {
            page = page
                .push(on_group(client, column))
                .push(persona::profile_rows(data).map(Message::Persona))
                .push(engines_group(data))
                .extend(groups)
                .push(voice_group(&state.voice, data, column))
                .extend(microphone_group(&state.voice, data))
                .push(programs_group(&state.voice, column))
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

/// "Voice": on or off with what the listener does, the wake word, "Speak answers" and the test.
fn voice_group<'a>(voice: &'a Voice, data: &'a Data, column: Option<f32>) -> Element<'a, Message> {
    let settings = &data.settings.voice;
    let can_listen = voice.status.as_ref().is_none_or(VoiceStatus::can_listen);
    let (status, detail) = voice_state(settings.enabled, voice.status.as_ref());
    let switch = toggler(settings.enabled)
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle_maybe((settings.enabled || can_listen).then_some(Message::Voice));
    let on = setting_row(
        Some(widgets::status_dot(status)),
        tr("Talk to the assistant"),
        Some(detail),
        agents::in_column(switch, column),
    );
    let word = torro_gui::typo::text(torro_gui::typo::body(), tr("Hey Torro")).style(torro_gui::theme::text_2);
    let wake = setting_row(
        None,
        tr("Wake word"),
        Some(tr("Your own wake word comes later").to_string()),
        agents::in_column(word, column),
    );
    let speak =
        toggler(settings.speak_answers).size(style::CHECK_SIZE).style(style::toggler).on_toggle(Message::SpeakAnswers);
    let speak = setting_row(
        None,
        tr("Speak answers"),
        Some(tr("Read aloud with Piper as well as shown").to_string()),
        agents::in_column(speak, column),
    );
    let test_button = match voice.testing {
        true => widgets::busy_button(tr("Listening…"), style::button::secondary),
        false => widgets::text_button(
            None,
            tr("Say something"),
            style::button::secondary,
            can_listen.then_some(Message::TestVoice),
        ),
    };
    let test =
        setting_row(None, tr("Test the microphone"), Some(heard_text(voice)), agents::in_column(test_button, column));
    let footer = tr(
        "Say “Hey Torro” and your question. Everything stays on this computer. Approvals are only given with a click, never by voice.",
    );
    widgets::group(tr("Voice"), vec![on, wake, speak, test], Some(footer.to_string()))
}

/// The dot and the line under "Talk to the assistant": off, or what the listener does.
pub fn voice_state(enabled: bool, status: Option<&VoiceStatus>) -> (Status, String) {
    let Some(status) = status else {
        return (Status::Inactive, if enabled { tr("On") } else { tr("Off") }.to_string());
    };
    if !status.installed {
        return (Status::Error, tr("The assistant app is not installed").to_string());
    }
    if !enabled {
        let detail = if status.can_listen() { tr("Off") } else { tr("Install the programs below first") };
        return (Status::Inactive, detail.to_string());
    }
    match (status.listener, &status.problem) {
        (VoiceState::Listening, _) => (Status::Ok, tr("Listening for “Hey Torro”").to_string()),
        (VoiceState::Recording, _) => (Status::Ok, tr("Recording …").to_string()),
        (VoiceState::Answering, _) => (Status::Ok, tr("Answering …").to_string()),
        (VoiceState::Off, Some(_)) => {
            (Status::Error, tr("The listener stopped. Check the programs below.").to_string())
        }
        (VoiceState::Off, None) => (Status::Attention, tr("Starting …").to_string()),
    }
}

/// The line under "Test the microphone": what was heard, or how to test.
fn heard_text(voice: &Voice) -> String {
    if voice.testing {
        return tr("Speak now, it stops at a pause").to_string();
    }
    match &voice.heard {
        None => tr("Shows what the assistant understands").to_string(),
        Some(Heard::Text(text)) => trf("Heard: “{text}”", &[("text", text)]),
        Some(Heard::Nothing) => tr("Nothing understood. Speak a little louder or closer.").to_string(),
        Some(Heard::Failed(_)) => tr("The test failed. Check the programs below.").to_string(),
    }
}

/// "Microphone": the system's default and every microphone the recording program lists, a check
/// at the chosen one. None when it lists none.
fn microphone_group<'a>(voice: &'a Voice, data: &'a Data) -> Option<Element<'a, Message>> {
    let microphones = &voice.status.as_ref()?.microphones;
    if microphones.is_empty() {
        return None;
    }
    let chosen = data.settings.voice.microphone.as_deref();
    let mut rows = vec![choice(tr("System default"), chosen.is_none(), Message::Microphone(None))];
    rows.extend(
        microphones
            .iter()
            .map(|name| choice(name.as_str(), chosen == Some(name.as_str()), Message::Microphone(Some(name.clone())))),
    );
    Some(widgets::group(tr("Microphone"), rows, None))
}

/// "Programs on this computer": the four parts with whether each was found, and "Check again".
fn programs_group<'a>(voice: &'a Voice, column: Option<f32>) -> Element<'a, Message> {
    let again = widgets::text_button(
        None,
        tr("Check again"),
        style::button::ghost,
        (!voice.asking).then_some(Message::CheckVoice),
    );
    let again = agents::in_column(again, column);
    let rows = match &voice.status {
        None if voice.asking || !voice.failed => {
            vec![setting_row(None, tr("Looking for the programs …"), None, Space::new())]
        }
        None => vec![setting_row(None, tr("The programs could not be checked"), None, again)],
        Some(status) if !status.installed => vec![setting_row(
            Some(widgets::status_dot(Status::Error)),
            tr("The assistant app is not installed"),
            Some(tr("Install torroassistant next to Torro Workplace").to_string()),
            again,
        )],
        Some(status) => {
            let mut rows: Vec<Element<'a, Message>> = status.programs.iter().map(program_row).collect();
            rows.push(setting_row(None, tr("Installed something?"), None, again));
            rows
        }
    };
    let footer = match voice.status.as_ref().and_then(|status| status.folder.as_deref()) {
        Some(folder) => {
            trf("Everything runs on this computer. Models and voices go into {folder}.", &[("folder", &folder)])
        }
        None => tr("Everything runs on this computer.").to_string(),
    };
    widgets::group(tr("Programs on this computer"), rows, Some(footer))
}

/// One part: its name, the program found or what to install, and a dot.
fn program_row<'a>(program: &VoiceProgram) -> Element<'a, Message> {
    let (status, detail) = program_state(program);
    setting_row(Some(widgets::status_dot(status)), part_label(program.part), Some(detail), Space::new())
}

/// "Speech to text", …
pub fn part_label(part: VoicePart) -> &'static str {
    match part {
        VoicePart::WakeWord => tr("Wake word"),
        VoicePart::Recorder => tr("Recording"),
        VoicePart::SpeechToText => tr("Speech to text"),
        VoicePart::Speech => tr("Speech output"),
    }
}

/// The dot and the line of a part: the program found, or how to get what is missing. The wake word
/// and speech are optional (speech to text stands in for the one, the window for the other), so
/// they are only orange when missing.
pub fn program_state(program: &VoiceProgram) -> (Status, String) {
    let optional = matches!(program.part, VoicePart::WakeWord | VoicePart::Speech);
    let Some(missing) = program.missing else {
        return (Status::Ok, program.program.clone().unwrap_or_default());
    };
    let hint = match (program.part, missing) {
        (VoicePart::WakeWord, VoiceMissing::Program) => {
            tr("Optional: pip install openwakeword. Until then speech to text listens for the wake word.")
        }
        (VoicePart::WakeWord, _) => {
            tr("Put hey_torro.onnx into the voice folder. Until then speech to text listens for the wake word.")
        }
        (VoicePart::Recorder, _) => recorder_hint(),
        (VoicePart::SpeechToText, VoiceMissing::Program) => tr("Install whisper.cpp, so that whisper-cli is found"),
        (VoicePart::SpeechToText, _) => tr("Put a whisper.cpp model such as ggml-base.bin into the voice folder"),
        (VoicePart::Speech, VoiceMissing::Program) => tr("Optional: install Piper to hear the answers"),
        (VoicePart::Speech, VoiceMissing::Model) => {
            tr("Put a Piper voice (.onnx with its .json) into the voice folder")
        }
        (VoicePart::Speech, VoiceMissing::Player) => tr("Install a program that plays sound, such as paplay"),
    };
    (if optional { Status::Attention } else { Status::Error }, hint.to_string())
}

/// How to get a recording program on this system.
fn recorder_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        tr("Install SoX (brew install sox) or ffmpeg")
    } else if cfg!(windows) {
        tr("Install SoX or ffmpeg")
    } else {
        tr("Install pulseaudio-utils (parec) or alsa-utils (arecord)")
    }
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

    /// A page whose voice status is known, so only the rights' clock can tick.
    fn voice_known() -> State {
        State { voice: Voice { status: Some(VoiceStatus::default()), ..Voice::default() }, ..State::default() }
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
        let mut state = voice_known();
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
        let mut state = voice_known();
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

    fn status(listener: VoiceState, missing: Option<VoiceMissing>) -> VoiceStatus {
        let part = |part| VoiceProgram { part, program: Some("x".into()), missing: None };
        VoiceStatus {
            listener,
            installed: true,
            problem: None,
            programs: vec![
                VoiceProgram { missing: Some(VoiceMissing::Program), program: None, ..part(VoicePart::WakeWord) },
                part(VoicePart::Recorder),
                VoiceProgram { missing, ..part(VoicePart::SpeechToText) },
                part(VoicePart::Speech),
            ],
            microphones: vec!["alsa_input.usb".into()],
            folder: Some("/home/anna/.local/share/torroassistant/voice".into()),
        }
    }

    fn voice_on(data: &mut Data) {
        data.settings.voice.enabled = true;
    }

    /// Task C.9: the switch, "Speak answers" and the microphone send `settings.set_setting`; the
    /// status is asked again once a change is stored.
    #[test]
    fn voice_settings_send_set_setting() {
        let data = with_assistant(Rights::assistant());
        let mut state = State::default();
        let sent = |action: Action| match action {
            Action::Send(Request::SetSetting { key, value }) => (key, value),
            other => panic!("expected settings.set_setting, got {other:?}"),
        };
        assert_eq!(
            sent(update(&mut state, Message::Voice(true), &data)),
            (SettingKey::VoiceEnabled, serde_json::json!(true))
        );
        assert_eq!(
            sent(update(&mut state, Message::Voice(false), &data)),
            (SettingKey::VoiceEnabled, serde_json::json!(false))
        );
        assert_eq!(
            sent(update(&mut state, Message::SpeakAnswers(false), &data)),
            (SettingKey::SpeakAnswers, serde_json::json!(false))
        );
        assert_eq!(
            sent(update(&mut state, Message::Microphone(Some("alsa_input.usb".into())), &data)),
            (SettingKey::Microphone, serde_json::json!("alsa_input.usb"))
        );
        assert_eq!(
            sent(update(&mut state, Message::Microphone(None), &data)),
            (SettingKey::Microphone, serde_json::Value::Null)
        );
        assert!(matches!(answered(&mut state, &Ok(Response::Ok)), Some(Action::Send(Request::VoiceStatus))));
        assert!(answered(&mut state, &Ok(Response::Ok)).is_none(), "once");
    }

    /// Without the programs voice cannot be switched on; off always works.
    #[test]
    fn voice_needs_the_programs_to_switch_on() {
        let data = with_assistant(Rights::assistant());
        let mut state = State::default();
        let missing = status(VoiceState::Off, Some(VoiceMissing::Model));
        assert!(answered(&mut state, &Ok(Response::Voice { voice: missing.clone() })).is_none());
        assert!(!missing.can_listen());
        assert!(matches!(update(&mut state, Message::Voice(true), &data), Action::None));
        assert!(matches!(update(&mut state, Message::Voice(false), &data), Action::Send(_)));
        assert_eq!(voice_state(false, Some(&missing)).1, tr("Install the programs below first"));
    }

    #[test]
    fn the_status_is_asked_at_the_start_and_after_switching() {
        let data = with_assistant(Rights::assistant());
        let mut state = State::default();
        assert_eq!(subscription(&state).units(), 1, "the voice clock: nothing known yet");
        assert!(matches!(update(&mut state, Message::CheckVoice, &data), Action::Send(Request::VoiceStatus)));
        assert!(matches!(update(&mut state, Message::CheckVoice, &data), Action::None), "one at a time");
        assert_eq!(subscription(&state).units(), 0, "waits for the answer");
        let _ = answered(&mut state, &Ok(Response::Voice { voice: status(VoiceState::Off, None) }));
        assert_eq!(subscription(&state).units(), 0, "known");

        let _ = update(&mut state, Message::Voice(true), &data);
        assert_eq!(subscription(&state).units(), 1, "until the listener runs");
        let _ = answered(&mut state, &Ok(Response::Voice { voice: status(VoiceState::Off, None) }));
        assert_eq!(subscription(&state).units(), 1, "still starting");
        let _ = answered(&mut state, &Ok(Response::Voice { voice: status(VoiceState::Listening, None) }));
        assert_eq!(subscription(&state).units(), 0);

        let mut failed = State::default();
        let _ = update(&mut failed, Message::CheckVoice, &data);
        assert!(answered(&mut failed, &Err("offline".into())).is_none(), "the shell shows it");
        assert_eq!(subscription(&failed).units(), 0, "not again by itself");
    }

    #[test]
    fn say_something_shows_what_was_heard() {
        let data = with_assistant(Rights::assistant());
        let mut state = State::default();
        assert!(matches!(update(&mut state, Message::TestVoice, &data), Action::Send(Request::TestVoice)));
        assert!(matches!(update(&mut state, Message::TestVoice, &data), Action::None), "one test at a time");
        assert_eq!(heard_text(&state.voice), tr("Speak now, it stops at a pause"));
        let text = Response::Transcript { text: " What is on today? ".into() };
        assert!(answered(&mut state, &Ok(text)).is_none());
        assert_eq!(heard_text(&state.voice), trf("Heard: “{text}”", &[("text", &"What is on today?")]));
        let _ = update(&mut state, Message::TestVoice, &data);
        let _ = answered(&mut state, &Ok(Response::Transcript { text: String::new() }));
        assert_eq!(heard_text(&state.voice), tr("Nothing understood. Speak a little louder or closer."));
        let _ = update(&mut state, Message::TestVoice, &data);
        assert!(answered(&mut state, &Err("no recorder".into())).is_none());
        assert_eq!(heard_text(&state.voice), tr("The test failed. Check the programs below."));
    }

    #[test]
    fn voice_states_and_programs_read_as_words() {
        let listening = status(VoiceState::Listening, None);
        assert_eq!(voice_state(true, Some(&listening)), (Status::Ok, tr("Listening for “Hey Torro”").to_string()));
        assert_eq!(voice_state(true, Some(&status(VoiceState::Recording, None))).1, tr("Recording …"));
        assert_eq!(voice_state(true, Some(&status(VoiceState::Off, None))).0, Status::Attention);
        let stopped = VoiceStatus { problem: Some("the listener stopped".into()), ..status(VoiceState::Off, None) };
        assert_eq!(voice_state(true, Some(&stopped)).0, Status::Error);
        let missing = VoiceStatus { installed: false, ..VoiceStatus::default() };
        assert_eq!(voice_state(false, Some(&missing)).1, tr("The assistant app is not installed"));
        assert_eq!(voice_state(false, None).1, tr("Off"));

        let program = |part, missing| VoiceProgram { part, program: Some("whisper-cli".into()), missing };
        assert_eq!(program_state(&program(VoicePart::SpeechToText, None)), (Status::Ok, "whisper-cli".to_string()));
        assert_eq!(program_state(&program(VoicePart::SpeechToText, Some(VoiceMissing::Model))).0, Status::Error);
        assert_eq!(program_state(&program(VoicePart::WakeWord, Some(VoiceMissing::Program))).0, Status::Attention);
        assert_eq!(program_state(&program(VoicePart::Speech, Some(VoiceMissing::Player))).0, Status::Attention);
        assert_eq!(part_label(VoicePart::SpeechToText), tr("Speech to text"));
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
        // Voice in each of its states.
        let mut on = data.clone();
        voice_on(&mut on);
        for answer in [
            Ok(Response::Voice { voice: status(VoiceState::Listening, None) }),
            Ok(Response::Voice { voice: VoiceStatus::default() }),
            Ok(Response::Transcript { text: "What is on today?".into() }),
        ] {
            let mut state = State::default();
            let _ = answered(&mut state, &answer);
            for column in [None, Some(agents::segment_slot())] {
                drop(page(&state, &on, column));
                drop(page(&state, &data, column));
            }
        }
    }
}
