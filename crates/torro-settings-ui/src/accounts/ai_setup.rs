//! Shared AI connection and model preparation for onboarding and Accounts (issue #527).
//! All I/O is a daemon request. Owner/run tags discard late answers after a page or form closes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use iced::widget::{column, container, responsive};
use iced::{Element, Subscription};
use torro_core::account::Secret;
use torro_core::ai::{AiProvider, AiSlot, LocalProgram};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{Align, Dropdown, DropdownEvent, MenuItem, MenuSection};
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::{
    AiCandidate, AiObservation, AiObservationStatus, AiPreparationKind, AiRecommendation, AiServer, Request, Response,
};

use super::{Action, Asked, Seen};
use crate::parts::setting_row;

static OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct State {
    owner: u64,
    run: u64,
    active: bool,
    flight: Option<Work>,
    found: Vec<AiServer>,
    observations: Vec<AiObservation>,
    recommendation: Option<AiRecommendation>,
    selected: Option<String>,
    base: String,
    choices: BTreeMap<String, usize>,
    expanded: BTreeSet<String>,
    sorting: bool,
    form: Option<(AiProvider, Option<String>, String)>,
    key: Secret,
    install: bool,
    focus: Option<String>,
    ways: bool,
    error: Option<String>,
    queue: Vec<(AiSlot, AiCandidate)>,
    consent: bool,
    allowed_remote: Option<(String, String)>,
    current: Option<(AiSlot, AiCandidate)>,
    pull: Option<String>,
    demo: bool,
    /// The dropdown of "Choose a provider" (issue #554: the suite's dropdown, not Iced's pick list).
    menus: Dropdown<()>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            owner: OWNER.fetch_add(1, Ordering::Relaxed),
            run: 0,
            active: false,
            flight: None,
            found: Vec::new(),
            observations: Vec::new(),
            recommendation: None,
            selected: None,
            base: String::new(),
            choices: BTreeMap::new(),
            expanded: BTreeSet::new(),
            sorting: true,
            form: None,
            key: Secret::default(),
            install: false,
            focus: None,
            ways: false,
            error: None,
            queue: Vec::new(),
            consent: false,
            allowed_remote: None,
            current: None,
            pull: None,
            demo: false,
            menus: Dropdown::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Tick,
    Connect(String),
    Online(AiProvider),
    Other,
    Address(String),
    Key(Secret),
    Submit,
    Back,
    Install(String),
    Select(String),
    OtherModel(AiSlot),
    ChooseModel(AiSlot, usize),
    KeyPage,
    Use,
    Confirm,
    Cancel,
    Retry,
    /// The dropdown of "Choose a provider" opens, closes or is clicked next to.
    Menus(DropdownEvent<()>),
}

#[derive(Debug)]
enum Work {
    Search,
    Recommend,
    Connect,
    List,
    Enable,
    Check,
    Set,
    Pull,
    Pulls,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProviderChoice(AiProvider);
impl std::fmt::Display for ProviderChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.name())
    }
}

fn send(state: &mut State, work: Work, request: Request) -> Action {
    state.run = state.run.wrapping_add(1);
    let changes = matches!(work, Work::Connect | Work::Enable | Work::Set);
    state.flight = Some(work);
    Action::AskAi(vec![(Asked::Setup { owner: state.owner, run: state.run, changes }, request)])
}

pub fn open(state: &mut State) -> Action {
    state.active = true;
    if state.flight.is_some() {
        return Action::None;
    }
    send(state, Work::Search, Request::FindAiServers)
}

pub fn leave(state: &mut State) {
    state.active = false;
    state.run = state.run.wrapping_add(1);
    state.flight = None;
    state.key = Secret::default();
    state.form = None;
    state.consent = false;
    state.allowed_remote = None;
    state.queue.clear();
    state.current = None;
    state.pull = None;
}

pub fn owns(state: &State, owner: u64) -> bool {
    state.owner == owner
}
pub fn active(state: &State) -> bool {
    state.active
}
pub fn connected(state: &State) -> bool {
    state.selected.is_some()
}

/// The host's explicit sorting choice also governs shared preparation, not just Continue.
pub fn allow_sorting(state: &mut State, allowed: bool) {
    state.sorting = allowed;
    if !allowed {
        state.queue.retain(|(slot, _)| *slot != AiSlot::Decide);
        if state.consent {
            state.consent = false;
            state.allowed_remote = None;
            state.queue.clear();
            if state.current.as_ref().is_some_and(|(slot, _)| *slot == AiSlot::Decide) {
                state.current = None;
            }
        }
    }
}

/// Short status for the host's compact overview, without starting a second polling path.
pub fn status(state: &State) -> Option<&'static str> {
    if state.current.is_some() || state.pull.is_some() {
        Some(tr("Preparing model…"))
    } else if matches!(state.flight, Some(Work::Connect)) {
        Some(tr("Connecting…"))
    } else if state.flight.is_some() {
        Some(tr("Looking on this computer…"))
    } else {
        None
    }
}

pub fn subscription(state: &State) -> Subscription<Message> {
    if !state.demo && state.form.is_none() && !state.consent {
        let interval = if !state.active {
            Duration::from_millis(1)
        } else if state.pull.is_some() {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(5)
        };
        iced::time::every(interval).map(|_| Message::Tick)
    } else {
        Subscription::none()
    }
}

fn refresh(state: &mut State) -> Action {
    send(state, Work::Recommend, Request::AiRecommend)
}

fn may_change_account(state: &State) -> bool {
    matches!(state.flight, None | Some(Work::Search | Work::Recommend))
        && state.current.is_none()
        && state.queue.is_empty()
        && state.pull.is_none()
        && !state.consent
}

fn select_account(state: &mut State, id: String, data: &dyn Seen) {
    let base = data.accounts().iter().find(|a| a.id == id).map(super::account_server).unwrap_or_default().to_string();
    select_endpoint(state, id, base);
}

fn select_endpoint(state: &mut State, id: String, base: String) {
    if state.selected.as_ref() != Some(&id) {
        // Candidate indices belong to one account's curated list, never another provider's.
        state.choices.clear();
        state.expanded.clear();
    }
    state.base = base;
    state.selected = Some(id);
}

/// Old daemons still return useful slots. Project their concrete suggestions into the same
/// preparation flow; program connection requests never fall back to the generic connection.
fn compatible_recommendation(mut recommendation: AiRecommendation) -> AiRecommendation {
    if recommendation.preparations.is_empty() {
        recommendation.preparations = recommendation
            .slots
            .iter()
            .filter_map(|s| {
                let (Some(account), Some(model)) = (&s.engine, &s.model) else {
                    return None;
                };
                Some(torro_ipc::AiPreparation {
                    account: account.clone(),
                    slot: s.slot,
                    candidates: vec![AiCandidate {
                        model: model.clone(),
                        size_bytes: s.download_bytes.unwrap_or_default(),
                        fits: true,
                        local: s.local,
                        preparation: if s.provider == AiProvider::Ollama && s.download_bytes.is_some() {
                            AiPreparationKind::OllamaPull
                        } else {
                            AiPreparationKind::Ready
                        },
                        by_chat: s.by_chat,
                    }],
                })
            })
            .collect();
    }
    recommendation
}

pub fn update(state: &mut State, message: Message, data: &dyn Seen) -> Action {
    if let Message::Menus(event) = message {
        // Eleven providers: a plain menu without search, so nothing asks for the focus.
        state.menus.update(event);
        return Action::None;
    }
    if matches!(message, Message::Online(_)) {
        state.menus.close();
    }
    if !state.active && !matches!(message, Message::Tick) {
        state.active = true;
    }
    if matches!(message, Message::Connect(_) | Message::Online(_) | Message::Other | Message::Back | Message::Select(_))
        && may_change_account(state)
        && state.flight.is_some()
    {
        // Read-only background work can be slow. Supersede its owner/run, without weakening
        // the account lock held by model preparation or a connection already being saved.
        state.run = state.run.wrapping_add(1);
        state.flight = None;
    }
    match message {
        Message::Tick if !state.active => open(state),
        Message::Tick if state.flight.is_some() || state.form.is_some() || state.consent => Action::None,
        Message::Tick if state.pull.is_some() => send(state, Work::Pulls, Request::AiPulls),
        Message::Tick if state.selected.is_some() => refresh(state),
        Message::Tick | Message::Retry => open(state),
        Message::Install(url) => {
            state.install = true;
            Action::OpenUrl(url)
        }
        Message::Connect(url) if may_change_account(state) => {
            state.ways = false;
            if let Some(server) = state.found.iter().find(|s| s.url == url).cloned() {
                if let Some(account) = server.account {
                    select_account(state, account, data);
                    return refresh(state);
                }
                let request = match server.program {
                    Some(program) => Request::ConnectLocalAi { program, server: Some(url), key: Secret::default() },
                    None => Request::ConnectAi { provider: server.provider, server: Some(url), key: Secret::default() },
                };
                return send(state, Work::Connect, request);
            }
            if let Some(observation) = state.observations.iter().find(|s| s.url == url) {
                state.form = Some((observation.provider, observation.program.clone(), url));
            }
            Action::None
        }
        Message::Online(provider) if may_change_account(state) => {
            state.form = Some((provider, None, provider.default_url().unwrap_or_default().to_string()));
            Action::None
        }
        Message::Other if may_change_account(state) => {
            state.form = Some((AiProvider::OpenAiCompatible, None, String::new()));
            Action::None
        }
        Message::Address(address) if state.flight.is_none() => {
            if let Some((_, _, held)) = state.form.as_mut() {
                *held = address;
            }
            Action::None
        }
        Message::Key(key) if state.flight.is_none() => {
            state.key = key;
            Action::None
        }
        Message::Submit if state.flight.is_none() => {
            let Some((provider, program, address)) = state.form.clone() else { return Action::None };
            let key = std::mem::take(&mut state.key);
            let request = match program {
                Some(program) => Request::ConnectLocalAi { program, server: Some(address), key },
                None => Request::ConnectAi { provider, server: Some(address), key },
            };
            send(state, Work::Connect, request)
        }
        Message::Back if may_change_account(state) => {
            state.form = None;
            state.key = Secret::default();
            state.selected = None;
            state.focus = None;
            state.ways = true;
            open(state)
        }
        Message::Select(account) if may_change_account(state) => {
            select_account(state, account, data);
            state.ways = false;
            refresh(state)
        }
        Message::OtherModel(slot) if state.current.is_none() => {
            if !state.expanded.remove(slot.key()) {
                state.expanded.insert(slot.key().to_string());
            }
            Action::None
        }
        Message::ChooseModel(slot, index) if state.current.is_none() && !state.consent => {
            let known = state.recommendation.as_ref().is_some_and(|r| {
                r.preparations.iter().any(|p| {
                    Some(&p.account) == state.selected.as_ref() && p.slot == slot && index < p.candidates.len()
                })
            });
            if known {
                state.choices.insert(slot.key().to_string(), index);
            }
            Action::None
        }
        Message::KeyPage => state
            .form
            .as_ref()
            .and_then(|(p, _, _)| p.key_url())
            .map_or(Action::None, |url| Action::OpenUrl(url.to_string())),
        Message::Use if state.flight.is_none() && state.current.is_none() => {
            state.queue = state
                .recommendation
                .as_ref()
                .into_iter()
                .flat_map(|r| &r.preparations)
                .filter(|p| {
                    Some(&p.account) == state.selected.as_ref()
                        && (state.sorting || p.slot != AiSlot::Decide)
                        && data.settings().ai_slots.get(p.slot).is_none()
                })
                .filter_map(|p| {
                    p.candidates.get(*state.choices.get(p.slot.key()).unwrap_or(&0)).cloned().map(|c| (p.slot, c))
                })
                .filter(|(_, c)| c.preparation != AiPreparationKind::External)
                .collect();
            if state.queue.is_empty() {
                return Action::None;
            }
            state.allowed_remote = None;
            state.consent = state.queue.iter().any(|(slot, c)| *slot == AiSlot::Decide && !c.local);
            if state.consent {
                return Action::None;
            }
            next(state)
        }
        Message::Confirm if state.consent => {
            state.consent = false;
            if let (Some(account), Some((AiSlot::Decide, candidate))) = (&state.selected, &state.current) {
                state.allowed_remote = Some((account.clone(), candidate.model.clone()));
                return send(state, Work::List, Request::AiModels { account: account.clone(), all: true });
            }
            if let (Some(account), Some((_, candidate))) =
                (&state.selected, state.queue.iter().find(|(s, c)| *s == AiSlot::Decide && !c.local))
            {
                state.allowed_remote = Some((account.clone(), candidate.model.clone()));
            }
            next(state)
        }
        Message::Cancel => {
            state.consent = false;
            state.allowed_remote = None;
            state.queue.clear();
            if let Some(id) = state.pull.take() {
                return send(state, Work::Cancel, Request::CancelAiPull { id });
            }
            state.current = None;
            Action::None
        }
        _ => Action::None,
    }
}

fn next(state: &mut State) -> Action {
    if !state.sorting {
        state.queue.retain(|(slot, _)| *slot != AiSlot::Decide);
    }
    if state.queue.is_empty() {
        state.current = None;
        return refresh(state);
    }
    let (slot, candidate) = state.queue.remove(0);
    if candidate.preparation == AiPreparationKind::External {
        state.queue.clear();
        state.error = Some(tr("Load the suggested model in the program first.").into());
        return Action::None;
    }
    let Some(account) = state.selected.clone() else { return Action::None };
    let request = if candidate.preparation == AiPreparationKind::OllamaPull {
        Request::PullAiModel { account, model: candidate.model.clone() }
    } else {
        Request::AiModels { account, all: true }
    };
    let work = if candidate.preparation == AiPreparationKind::OllamaPull { Work::Pull } else { Work::List };
    state.current = Some((slot, candidate));
    send(state, work, request)
}

/// Only the still-open owner and the exact outstanding run can advance a chain.
pub fn answered(state: &mut State, owner: u64, run: u64, result: &Result<Response, String>) -> Action {
    if !state.active || state.owner != owner || state.run != run {
        return Action::None;
    }
    let Some(work) = state.flight.take() else { return Action::None };
    if !state.sorting && state.current.as_ref().is_some_and(|(slot, _)| *slot == AiSlot::Decide) {
        state.current = None;
        state.pull = None;
        state.allowed_remote = None;
        return next(state);
    }
    state.error = None;
    if matches!(work, Work::Pull) && matches!(result, Ok(Response::Error { code, .. }) if code == "busy") {
        return send(state, Work::Pulls, Request::AiPulls);
    }
    if let Err(message) | Ok(Response::Error { message, .. }) = result {
        state.error = Some(message.clone());
        state.current = None;
        state.queue.clear();
        return Action::None;
    }
    match (work, result) {
        (Work::Search, Ok(Response::AiServers { servers, observations })) => {
            state.found = servers.clone();
            state.observations = observations.clone();
            if state.install && servers.len() == 1 && observations.is_empty() {
                state.focus = Some(servers[0].url.clone());
            }
            refresh(state)
        }
        (Work::Recommend, Ok(Response::AiRecommendation { recommendation })) => {
            state.recommendation = Some(compatible_recommendation(recommendation.clone()));
            Action::None
        }
        (Work::Connect, Ok(Response::Account { account })) => {
            select_endpoint(state, account.id.clone(), super::account_server(account).to_string());
            state.form = None;
            refresh(state)
        }
        (Work::Pull, Ok(Response::AiPulls { pulls })) => {
            state.pull = pulls
                .iter()
                .rev()
                .find(|p| {
                    Some(&p.account) == state.selected.as_ref()
                        && state.current.as_ref().is_some_and(|(_, c)| super::same_model(&c.model, &p.model))
                })
                .map(|p| p.id.clone());
            send(state, Work::Pulls, Request::AiPulls)
        }
        (Work::Pulls, Ok(Response::AiPulls { pulls })) => {
            match pulls.iter().rev().find(|p| {
                state.pull.as_ref().map_or_else(
                    || {
                        Some(&p.account) == state.selected.as_ref()
                            && state.current.as_ref().is_some_and(|(_, c)| super::same_model(&c.model, &p.model))
                    },
                    |id| &p.id == id,
                )
            }) {
                Some(p) if p.state == torro_ipc::AiPullState::Running => {
                    state.pull = Some(p.id.clone());
                    Action::None
                }
                Some(p) if p.state == torro_ipc::AiPullState::Done => {
                    state.pull = None;
                    let Some(account) = state.selected.clone() else { return Action::None };
                    send(state, Work::List, Request::AiModels { account, all: true })
                }
                _ => {
                    state.pull = None;
                    state.current = None;
                    state.queue.clear();
                    state.error = Some(tr("The model download did not finish. Try again.").into());
                    Action::None
                }
            }
        }
        (Work::List, Ok(Response::AiModels { account, models })) if Some(account) == state.selected.as_ref() => {
            let Some((slot, candidate)) = state.current.as_ref() else { return Action::None };
            let Some(model) = models.iter().find(|m| super::same_model(&m.id, &candidate.model)) else {
                state.current = None;
                state.queue.clear();
                state.error = Some(tr("Load the suggested model in the program first.").into());
                return Action::None;
            };
            let remote = super::ai_provider(account).is_none_or(|provider| {
                torro_core::ai::place_of(provider, &state.base, model) != torro_core::ai::Place::Local
            });
            let allowed = state
                .allowed_remote
                .as_ref()
                .is_some_and(|(a, m)| a == account && super::same_model(m, &candidate.model));
            if *slot == AiSlot::Decide && remote && !allowed {
                if let Some((_, candidate)) = state.current.as_mut() {
                    candidate.local = false;
                    candidate.model = model.id.clone();
                }
                state.consent = true;
                return Action::None;
            }
            if let Some((_, candidate)) = state.current.as_mut() {
                candidate.model = model.id.clone();
            }
            if model.enabled {
                send(state, Work::Check, Request::AiAssignments)
            } else {
                send(
                    state,
                    Work::Enable,
                    Request::SetModelEnabled { account: account.clone(), model: model.id.clone(), enabled: true },
                )
            }
        }
        (Work::Enable, Ok(Response::Ok)) => send(state, Work::Check, Request::AiAssignments),
        (Work::Check, Ok(Response::AiAssignments { slots, .. })) => {
            let Some((slot, _)) = &state.current else { return Action::None };
            match slots.iter().find(|s| s.slot == *slot) {
                Some(info) if info.chosen.is_none() => set(state),
                Some(_) => {
                    state.current = None;
                    next(state)
                }
                None => {
                    state.current = None;
                    state.queue.clear();
                    state.error = Some(tr("Torro Workplace did not answer as expected. Try again.").into());
                    Action::None
                }
            }
        }
        (Work::Set, Ok(Response::AiAssignments { .. })) => {
            state.current = None;
            next(state)
        }
        (Work::Cancel, Ok(_)) => {
            state.current = None;
            Action::None
        }
        _ => {
            state.error = Some(tr("Torro Workplace did not answer as expected. Try again.").into());
            Action::None
        }
    }
}

fn set(state: &mut State) -> Action {
    let (Some((slot, candidate)), Some(account)) = (&state.current, &state.selected) else { return Action::None };
    send(
        state,
        Work::Set,
        Request::SetAiSlot {
            slot: *slot,
            engine: Some(account.clone()),
            model: Some(candidate.model.clone()),
            effort: None,
        },
    )
}

/// The open dropdown of "Choose a provider" under its button, if it is open (issue #554). The page
/// or dialog that shows [`view`] lays it over itself.
pub fn overlay(state: &State) -> Option<Element<'_, Message>> {
    let key = state.menus.open()?;
    let items = AiProvider::CLOUD
        .iter()
        .map(|provider| MenuItem::plain(ProviderChoice(*provider).to_string(), Message::Online(*provider)))
        .collect();
    Some(state.menus.view(key, vec![MenuSection::new(items)], widgets::menu_width(), Align::End, Message::Menus))
}

/// What Escape means here: close the open dropdown, if one is.
pub fn escape(state: &State) -> Option<Message> {
    state.menus.open().map(|_| Message::Menus(DropdownEvent::Close))
}

pub fn view<'a>(state: &'a State, data: &'a dyn Seen) -> Element<'a, Message> {
    responsive(move |bounds| view_in(state, data, bounds.width < size::form_max())).into()
}

fn setup_row<'a>(
    narrow: bool,
    label: impl iced::widget::text::IntoFragment<'a>,
    detail: Option<String>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    if !narrow {
        return setting_row(None, label, detail, control);
    }
    let mut content = column![typo::text(typo::body_strong(), label)].spacing(space::xs());
    if let Some(detail) = detail {
        content = content.push(typo::text(typo::meta(), detail).style(theme::text_2));
    }
    content = content.push(control.into());
    container(content).padding([space::m(), space::l()]).into()
}

fn can_use(state: &State, data: &dyn Seen) -> bool {
    state.flight.is_none()
        && state.current.is_none()
        && !state.consent
        && state.recommendation.as_ref().is_some_and(|r| {
            r.preparations
                .iter()
                .filter(|p| {
                    Some(&p.account) == state.selected.as_ref()
                        && (state.sorting || p.slot != AiSlot::Decide)
                        && data.settings().ai_slots.get(p.slot).is_none()
                })
                .any(|p| {
                    p.candidates
                        .get(*state.choices.get(p.slot.key()).unwrap_or(&0))
                        .is_some_and(|c| c.preparation != AiPreparationKind::External)
                })
        })
}

fn view_in<'a>(state: &'a State, data: &'a dyn Seen, narrow: bool) -> Element<'a, Message> {
    let mut parts = column![].spacing(layout::section_gap());
    if state.flight.is_some() || state.pull.is_some() {
        let status = if matches!(state.flight, Some(Work::Connect)) {
            tr("Connecting…")
        } else if state.current.is_some() {
            tr("Preparing model…")
        } else {
            tr("Looking on this computer…")
        };
        parts = parts.push(typo::text(typo::meta(), status).style(theme::text_2));
    }
    if let Some(error) = &state.error {
        parts = parts.push(
            column![
                typo::text(typo::body(), error).style(theme::text_2),
                widgets::text_button(
                    None,
                    tr("Retry"),
                    style::button::secondary,
                    state.flight.is_none().then_some(Message::Retry)
                )
            ]
            .spacing(space::s()),
        );
    }
    if let Some((provider, program, address)) = &state.form {
        let name = program.as_deref().and_then(LocalProgram::from_id).map_or(provider.name(), LocalProgram::name);
        let address = super::wizard::field(tr("Server address"), address)
            .on_input_maybe(state.flight.is_none().then_some(Message::Address))
            .on_submit_maybe(state.flight.is_none().then_some(Message::Submit));
        let key = super::wizard::field(tr("API key"), state.key.expose())
            .secure(true)
            .on_input_maybe(state.flight.is_none().then_some(|key| Message::Key(Secret::new(key))))
            .on_submit_maybe(state.flight.is_none().then_some(Message::Submit));
        if program.as_deref() == Some("jan") {
            parts = parts.push(typo::text(typo::body(), tr("Computation location unknown")));
        }
        if provider.key_url().is_some() {
            parts = parts.push(widgets::text_button(
                Some(Icon::ArrowUpRight),
                tr("Create an API key"),
                style::button::ghost,
                Some(Message::KeyPage),
            ));
        }
        return parts
            .push(typo::text(typo::body_strong(), name))
            .push(column![typo::text(typo::caption(), tr("Server address")), address].spacing(space::xs()))
            .push(column![typo::text(typo::caption(), tr("API key")), key].spacing(space::xs()))
            .push(widgets::text_button(
                None,
                tr("Back"),
                style::button::ghost,
                state.flight.is_none().then_some(Message::Back),
            ))
            .push(widgets::text_button(
                None,
                tr("Connect"),
                style::button::secondary,
                state.flight.is_none().then_some(Message::Submit),
            ))
            .into();
    }
    if let Some(recommendation) = &state.recommendation {
        let memory = recommendation
            .hardware
            .memory_bytes
            .map(|bytes| format!("{:.0} GB", bytes as f64 / 1_073_741_824.0))
            .unwrap_or_else(|| tr("Not measured").into());
        let gpu =
            recommendation.hardware.gpu.as_ref().and_then(|gpu| gpu.name.as_deref()).unwrap_or(tr("Not measured"));
        parts = parts.push(
            column![
                typo::text(typo::body_strong(), tr("Your computer")),
                typo::text(typo::body(), recommendation.hardware.cpu.as_deref().unwrap_or(tr("Unknown"))),
                typo::text(typo::meta(), format!("{} · {memory} · {gpu}", recommendation.tier_label))
                    .style(theme::text_2),
            ]
            .spacing(space::s()),
        );
    }
    if state.focus.is_some() && state.selected.is_none() {
        parts = parts
            .push(typo::text(typo::body_strong(), tr("Prepare model")))
            .push(typo::text(typo::body(), tr("Connect this program to prepare its recommended model.")));
    }
    let mut rows = Vec::new();
    for server in &state.found {
        let name = server
            .program
            .as_deref()
            .and_then(LocalProgram::from_id)
            .map_or(server.provider.name(), LocalProgram::name);
        let count =
            if server.models == 1 { tr("1 model").to_string() } else { trf("{n} models", &[("n", &server.models)]) };
        let detail = format!(
            "{} · {count}{}",
            server.url,
            if server.models == 0 { format!(" · {}", tr("No models available")) } else { String::new() }
        );
        let detail = if server.program.as_deref() == Some("jan") {
            format!("{detail} · {}", tr("Computation location unknown"))
        } else {
            detail
        };
        let connected = server.account.is_some();
        let label = if connected {
            tr("Connected")
        } else if state.focus.as_ref() == Some(&server.url) {
            tr("Prepare model")
        } else {
            tr("Connect")
        };
        rows.push(setup_row(
            narrow,
            name,
            Some(detail),
            widgets::text_button(
                None,
                label,
                style::button::secondary,
                may_change_account(state).then(|| Message::Connect(server.url.clone())),
            ),
        ));
    }
    for observation in &state.observations {
        let name = observation
            .program
            .as_deref()
            .and_then(LocalProgram::from_id)
            .map_or(observation.provider.name(), LocalProgram::name);
        let status = match observation.status {
            AiObservationStatus::AuthRequired => tr("API key required"),
            AiObservationStatus::ModelsUnavailable => tr("Model list unavailable"),
        };
        let detail = format!(
            "{} · {status}{}",
            observation.url,
            if observation.program.as_deref() == Some("jan") {
                format!(" · {}", tr("Computation location unknown"))
            } else {
                String::new()
            }
        );
        rows.push(setup_row(
            narrow,
            name,
            Some(detail),
            widgets::text_button(
                None,
                tr("Connect"),
                style::button::secondary,
                may_change_account(state).then(|| Message::Connect(observation.url.clone())),
            ),
        ));
    }
    if !rows.is_empty() {
        parts = parts.push(widgets::group(tr("On this computer"), rows, None));
    }
    let local = widgets::group(
        tr("On this computer"),
        [
            ("Ollama", "https://ollama.com/download"),
            ("LM Studio", "https://lmstudio.ai/download"),
            ("Strata", "https://github.com/Niko1221/Strata"),
        ]
        .into_iter()
        .filter(|(name, _)| {
            !state.found.iter().any(|s| s.provider.name() == *name)
                && !state.observations.iter().any(|s| s.provider.name() == *name)
        })
        .map(|(name, url)| {
            setup_row(
                narrow,
                name,
                None,
                widgets::text_button(
                    Some(Icon::ArrowUpRight),
                    tr("Get program"),
                    style::button::secondary,
                    Some(Message::Install(url.into())),
                ),
            )
        })
        .collect(),
        Some(tr("Torro checks automatically while this page is open. GPT4All: enable the Local API Server.").into()),
    );
    let provider = state.menus.button((), tr("Choose provider"), Message::Menus);
    let online = widgets::group(
        tr("Online service"),
        vec![setup_row(narrow, tr("Choose a provider"), Some(tr("Connect with an API key").into()), provider)],
        Some(tr("Text goes to this provider. Mail stays here unless you allow it.").into()),
    );
    if state.selected.is_none()
        && state.focus.is_none()
        && (state.ways || (state.found.is_empty() && state.observations.is_empty()))
    {
        let small = state.recommendation.as_ref().is_none_or(|r| r.tier == "small");
        parts = if small { parts.push(online).push(local) } else { parts.push(local).push(online) };
        parts = parts.push(widgets::text_button(None, tr("Other server"), style::button::ghost, Some(Message::Other)));
    }
    if state.selected.is_some() || state.focus.is_some() || !state.found.is_empty() || !state.observations.is_empty() {
        parts = parts.push(widgets::text_button(
            None,
            tr("Other provider"),
            style::button::ghost,
            may_change_account(state).then_some(Message::Back),
        ));
    }
    for account in data.accounts().iter().filter(|a| a.kind == "ai") {
        parts = parts.push(widgets::choice_chip(
            account.label.clone(),
            None,
            state.selected.as_ref() == Some(&account.id),
            may_change_account(state).then_some(Message::Select(account.id.clone())),
        ));
    }
    if let Some(recommendation) = &state.recommendation {
        let mut suggestions = Vec::new();
        if let Some(account) = &state.selected {
            let fallback = match super::ai_provider(account) {
                Some(provider) => provider.name(),
                None => account.as_str(),
            };
            let name = data.accounts().iter().find(|a| &a.id == account).map_or(fallback, |a| a.label.as_str());
            parts = parts.push(typo::text(typo::body_strong(), name.to_string()));
        }
        for preparation in recommendation.preparations.iter().filter(|p| Some(&p.account) == state.selected.as_ref()) {
            let index = *state.choices.get(preparation.slot.key()).unwrap_or(&0);
            if let Some(candidate) = preparation.candidates.get(index) {
                let fit =
                    if candidate.fits { tr("Fits this computer") } else { tr("May be too large for this computer") };
                let detail = if candidate.size_bytes == 0 {
                    candidate.model.clone()
                } else {
                    format!("{} · ~{:.1} GB · {fit}", candidate.model, candidate.size_bytes as f64 / 1_000_000_000.0)
                };
                let detail =
                    if candidate.by_chat { format!("{detail} · {}", tr("Slower chat fallback")) } else { detail };
                let choose: Element<'a, Message> = if preparation.candidates.len() > 1 {
                    widgets::text_button(
                        None,
                        tr("Other model"),
                        style::button::ghost,
                        state.current.is_none().then_some(Message::OtherModel(preparation.slot)),
                    )
                } else {
                    iced::widget::Space::new().into()
                };
                suggestions.push(setup_row(
                    narrow,
                    if preparation.slot == AiSlot::Chat { tr("Chat") } else { tr("Sorting") },
                    Some(detail),
                    choose,
                ));
                if state.expanded.contains(preparation.slot.key()) && preparation.candidates.len() > 1 {
                    let choices = preparation
                        .candidates
                        .iter()
                        .enumerate()
                        .map(|(at, c)| {
                            widgets::choice_chip(
                                c.model.clone(),
                                None,
                                at == index,
                                state.current.is_none().then_some(Message::ChooseModel(preparation.slot, at)),
                            )
                        })
                        .collect();
                    suggestions.push(widgets::chip_row(choices));
                }
                if candidate.preparation == AiPreparationKind::External {
                    let fallback = state
                        .selected
                        .as_deref()
                        .and_then(super::ai_provider)
                        .map_or(tr("AI program"), AiProvider::name);
                    let program = data
                        .accounts()
                        .iter()
                        .find(|a| Some(&a.id) == state.selected.as_ref())
                        .map_or(fallback, |a| a.label.as_str());
                    suggestions.push(setup_row(
                        narrow,
                        trf("Load in {program}", &[("program", &program)]),
                        Some(candidate.model.clone()),
                        typo::text(typo::meta(), tr("Checks automatically")).style(theme::text_2),
                    ));
                }
                if !candidate.local {
                    suggestions.push(setup_row(
                        narrow,
                        tr("Computation location unknown"),
                        Some(tr("Mail needs your explicit permission.").into()),
                        iced::widget::Space::new(),
                    ));
                }
            }
        }
        for suggestion in recommendation.slots.iter().filter(|s| {
            s.engine.as_ref() == state.selected.as_ref()
                && !recommendation
                    .preparations
                    .iter()
                    .any(|p| Some(&p.account) == s.engine.as_ref() && p.slot == s.slot)
        }) {
            let model = suggestion.model.as_deref().unwrap_or(tr("Automatic"));
            suggestions.push(setup_row(
                narrow,
                if suggestion.slot == AiSlot::Chat { tr("Chat") } else { tr("Sorting") },
                Some(format!("{model} · {}", suggestion.reason)),
                iced::widget::Space::new(),
            ));
        }
        if !suggestions.is_empty() {
            parts = parts.push(widgets::group(tr("Our suggestion for this PC"), suggestions, None)).push(
                widgets::text_button(
                    None,
                    tr("Use suggestion"),
                    style::button::secondary,
                    can_use(state, data).then_some(Message::Use),
                ),
            );
        }
    }
    if state.pull.is_some() {
        parts = parts.push(widgets::text_button(
            None,
            tr("Cancel download"),
            style::button::secondary,
            Some(Message::Cancel),
        ));
    }
    if state.consent {
        if let Some((_, candidate)) =
            state.current.as_ref().or_else(|| state.queue.iter().find(|(slot, _)| *slot == AiSlot::Decide))
        {
            let account = data
                .accounts()
                .iter()
                .find(|a| Some(&a.id) == state.selected.as_ref())
                .map(|a| a.label.as_str())
                .or(state.selected.as_deref())
                .unwrap_or_default();
            parts = parts.push(typo::text(typo::body_strong(), format!("{} · {account}", candidate.model)));
        }
        parts = parts
            .push(typo::text(
                typo::body(),
                tr("Mail goes to this model outside this computer and cannot be taken back."),
            ))
            .push(widgets::text_button(None, tr("Use"), style::button::secondary, Some(Message::Confirm)))
            .push(widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::Cancel)));
    }
    parts.into()
}

/// Deterministic, non-network states for headless design inspection.
pub fn demo(state: &mut State, scenario: &str, data: &dyn Seen) {
    state.demo = true;
    state.active = true;
    state.flight = None;
    state.found.clear();
    state.observations.clear();
    let Some(Response::AiRecommendation { mut recommendation }) = super::demo::demo_answer(&Request::AiRecommend, data)
    else {
        return;
    };
    recommendation.tier = if scenario == "small" { "small" } else { "medium" }.into();
    recommendation.tier_label = if scenario == "small" { tr("Small") } else { tr("Medium") }.into();
    recommendation.hardware = torro_core::ai::hardware::Hardware {
        cpu: Some(tr("Demo computer").into()),
        memory_bytes: Some(if scenario == "small" { 4 } else { 16 } * (1 << 30)),
        gpu: None,
        unified_memory: false,
    };
    match scenario {
        "found" => state.found.push(AiServer {
            provider: AiProvider::Ollama,
            program: None,
            url: "http://localhost:11434/v1".into(),
            models: 3,
            account: None,
        }),
        "auth" => state.observations.push(AiObservation {
            provider: AiProvider::OpenAiCompatible,
            program: Some("jan".into()),
            url: "http://127.0.0.1:1337/v1".into(),
            status: AiObservationStatus::AuthRequired,
            account: None,
        }),
        "external" | "ready" | "warning" => {
            let account = "ai:lm_studio@localhost:1234".to_string();
            state.selected = Some(account.clone());
            recommendation.preparations = vec![torro_ipc::AiPreparation {
                account,
                slot: AiSlot::Chat,
                candidates: vec![AiCandidate {
                    model: "qwen3-4b".into(),
                    size_bytes: 2_497_293_931,
                    fits: true,
                    local: true,
                    preparation: if scenario == "ready" {
                        AiPreparationKind::Ready
                    } else {
                        AiPreparationKind::External
                    },
                    by_chat: false,
                }],
            }];
            if scenario == "warning" {
                let mut large = recommendation.preparations[0].candidates[0].clone();
                large.model = "qwen3-32b".into();
                large.size_bytes = 20_201_253_829;
                large.fits = false;
                recommendation.preparations[0].candidates.push(large);
                state.choices.insert(AiSlot::Chat.key().into(), 1);
                state.expanded.insert(AiSlot::Chat.key().into());
            }
        }
        "form" => {
            state.form = Some((AiProvider::OpenAiCompatible, Some("jan".into()), "http://127.0.0.1:1337/v1".into()));
        }
        "consent" | "download" => {
            let remote = scenario == "consent";
            let account =
                if remote { "ai:openai_compatible@localhost:1337" } else { "ai:ollama@localhost:11434" }.to_string();
            state.selected = Some(account.clone());
            let candidate = AiCandidate {
                model: "qwen3:4b".into(),
                size_bytes: 2_497_293_931,
                fits: true,
                local: !remote,
                preparation: if remote { AiPreparationKind::Ready } else { AiPreparationKind::OllamaPull },
                by_chat: remote,
            };
            let slot = if remote { AiSlot::Decide } else { AiSlot::Chat };
            recommendation.preparations =
                vec![torro_ipc::AiPreparation { account, slot, candidates: vec![candidate.clone()] }];
            if remote {
                state.queue = vec![(slot, candidate)];
                state.consent = true;
            } else {
                state.current = Some((slot, candidate));
                state.pull = Some("demo-download".into());
            }
        }
        "error" => state.error = Some(tr("The local AI search could not start.").into()),
        _ => {}
    }
    state.recommendation = Some(recommendation);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(action: Action) -> (u64, u64, Request) {
        let Action::AskAi(mut asks) = action else { panic!("owned request") };
        let (Asked::Setup { owner, run, .. }, request) = asks.remove(0) else { panic!("setup tag") };
        (owner, run, request)
    }

    /// Issue #554: "Choose a provider" is the suite's dropdown; Escape and a choice close it.
    #[test]
    fn the_provider_dropdown_opens_and_closes() {
        let data = super::super::demo::demo_data();
        let mut state = State::default();
        assert!(overlay(&state).is_none());
        update(&mut state, Message::Menus(DropdownEvent::Toggle(())), &data);
        assert!(overlay(&state).is_some());
        assert_eq!(escape(&state), Some(Message::Menus(DropdownEvent::Close)));
        update(&mut state, Message::Online(AiProvider::OpenRouter), &data);
        assert!(overlay(&state).is_none() && escape(&state).is_none(), "a choice closes it");
    }

    #[test]
    fn polling_is_single_flight_and_leaving_invalidates_old_answers() {
        let data = super::super::demo::demo_data();
        let mut state = State::default();
        let (owner, run, request) = tag(open(&mut state));
        assert!(matches!(request, Request::FindAiServers));
        assert!(matches!(update(&mut state, Message::Tick, &data), Action::None));
        leave(&mut state);
        let found = Ok(Response::AiServers { servers: vec![], observations: vec![] });
        assert!(matches!(answered(&mut state, owner, run, &found), Action::None));
        let (_, next_run, _) = tag(open(&mut state));
        assert_ne!(next_run, run);
        assert!(matches!(answered(&mut state, owner, run, &found), Action::None));
        assert!(state.flight.is_some());
    }

    #[test]
    fn installing_focuses_one_server_without_connecting_and_a_form_pauses_polling() {
        let data = super::super::demo::demo_data();
        let mut state = State::default();
        update(&mut state, Message::Install("https://ollama.com/download".into()), &data);
        let (owner, run, _) = tag(open(&mut state));
        let server = AiServer {
            provider: AiProvider::OpenAiCompatible,
            program: Some("jan".into()),
            url: "http://127.0.0.1:1337/v1".into(),
            models: 1,
            account: None,
        };
        let (_, r, request) = tag(answered(
            &mut state,
            owner,
            run,
            &Ok(Response::AiServers { servers: vec![server.clone()], observations: vec![] }),
        ));
        assert!(matches!(request, Request::AiRecommend), "a discovery never connects or downloads");
        assert_eq!(state.focus.as_deref(), Some(server.url.as_str()));
        answered(&mut state, owner, r, &Err("offline".into()));
        update(&mut state, Message::Online(AiProvider::OpenRouter), &data);
        assert!(matches!(update(&mut state, Message::Tick, &data), Action::None));
    }

    #[test]
    fn named_program_connections_never_fall_back_after_unsupported() {
        let data = super::super::demo::demo_data();
        let mut state = State { active: true, ..State::default() };
        state.found.push(AiServer {
            provider: AiProvider::OpenAiCompatible,
            program: Some("jan".into()),
            url: "http://127.0.0.1:1337/v1".into(),
            models: 1,
            account: None,
        });
        let (owner, run, request) = tag(update(&mut state, Message::Connect("http://127.0.0.1:1337/v1".into()), &data));
        assert!(matches!(request, Request::ConnectLocalAi { program, .. } if program == "jan"));
        let result = Ok(Response::Error { code: "unsupported".into(), message: "old daemon".into() });
        assert!(matches!(answered(&mut state, owner, run, &result), Action::None));
        assert!(state.selected.is_none());
    }

    #[test]
    fn external_models_are_never_pulled_and_remote_sorting_waits_for_consent() {
        let mut data = super::super::demo::demo_data();
        data.settings.ai_slots.decide = None;
        data.settings.ai_slots.chat = None;
        let mut state =
            State { active: true, selected: Some("ai:openai_compatible@localhost:1337".into()), ..State::default() };
        let mut recommendation = super::super::demo::demo_answer(&Request::AiRecommend, &data).expect("demo");
        let Response::AiRecommendation { recommendation: r } = &mut recommendation else { panic!("recommendation") };
        r.preparations = vec![torro_ipc::AiPreparation {
            account: state.selected.clone().expect("selected"),
            slot: AiSlot::Decide,
            candidates: vec![AiCandidate {
                model: "qwen3-4b".into(),
                size_bytes: 2_497_293_931,
                fits: true,
                local: false,
                preparation: AiPreparationKind::External,
                by_chat: true,
            }],
        }];
        state.recommendation = Some(r.clone());
        assert!(!can_use(&state, &data));
        assert!(matches!(update(&mut state, Message::Use, &data), Action::None));
        assert!(
            !state.consent && state.flight.is_none() && state.error.is_none(),
            "external loading is a normal waiting state"
        );
        r.preparations[0].candidates[0].preparation = AiPreparationKind::Ready;
        state.recommendation = Some(r.clone());
        assert!(can_use(&state, &data));
        update(&mut state, Message::Use, &data);
        assert!(state.consent && state.flight.is_none());
        let (_, _, request) = tag(update(&mut state, Message::Confirm, &data));
        assert!(
            matches!(request, Request::AiModels { .. }),
            "even ready external models are freshly listed, never pulled"
        );
    }

    #[test]
    fn changed_location_waits_for_consent_and_newer_slot_choices_survive() {
        let candidate = AiCandidate {
            model: "qwen3:4b".into(),
            size_bytes: 2_497_293_931,
            fits: true,
            local: true,
            preparation: AiPreparationKind::Ready,
            by_chat: true,
        };
        let account = "ai:ollama@localhost:11434".to_string();
        let mut state = State {
            active: true,
            selected: Some(account.clone()),
            current: Some((AiSlot::Decide, candidate)),
            flight: Some(Work::List),
            ..State::default()
        };
        let owner = state.owner;
        let models = vec![torro_core::ai::AiModel {
            remote: Some(true),
            enabled: true,
            ..torro_core::ai::AiModel::named("qwen3:4b")
        }];
        assert!(matches!(
            answered(&mut state, owner, 0, &Ok(Response::AiModels { account: account.clone(), models })),
            Action::None
        ));
        assert!(state.consent && state.flight.is_none(), "fresh remote evidence cannot silently grant mail permission");
        state.consent = false;
        state.flight = Some(Work::Check);
        let answer = Response::AiAssignments {
            assignments: vec![],
            slots: vec![torro_ipc::AiSlotInfo {
                slot: AiSlot::Decide,
                label: tr("Sorting").into(),
                chosen: Some(torro_core::ai::SlotModel { engine: "newer-choice".into(), model: None, effort: None }),
                mail_to_cloud: false,
            }],
        };
        let (_, _, request) = tag(answered(&mut state, owner, 0, &Ok(answer)));
        assert!(matches!(request, Request::AiRecommend), "a newer occupied slot is not overwritten");
        assert!(state.current.is_none());
    }

    #[test]
    fn preparation_cannot_move_to_another_account_between_download_polls() {
        let data = super::super::demo::demo_data();
        let candidate = AiCandidate {
            model: "nimble".into(),
            size_bytes: 9_527_504_198,
            fits: true,
            local: true,
            preparation: AiPreparationKind::Ready,
            by_chat: false,
        };
        let original = "ai:ollama@localhost:11434".to_string();
        let mut state = State {
            active: true,
            selected: Some(original.clone()),
            base: "http://localhost:11434/v1".into(),
            current: Some((AiSlot::Chat, candidate.clone())),
            queue: vec![(AiSlot::Decide, candidate.clone())],
            pull: Some("p1".into()),
            ..State::default()
        };
        let elsewhere = "ai:lm_studio@192.168.1.20:1234".to_string();
        for message in
            [Message::Select(elsewhere.clone()), Message::Back, Message::Other, Message::Online(AiProvider::OpenRouter)]
        {
            assert!(matches!(update(&mut state, message, &data), Action::None));
            assert_eq!(state.selected.as_ref(), Some(&original));
        }
        let mut lan = State {
            active: true,
            selected: Some(elsewhere.clone()),
            base: "http://192.168.1.20:1234/v1".into(),
            current: Some((AiSlot::Decide, candidate)),
            flight: Some(Work::List),
            ..State::default()
        };
        let owner = lan.owner;
        let models = vec![torro_core::ai::AiModel { enabled: true, ..torro_core::ai::AiModel::named("nimble") }];
        assert!(matches!(
            answered(&mut lan, owner, 0, &Ok(Response::AiModels { account: elsewhere, models })),
            Action::None
        ));
        assert!(lan.consent, "a stale local candidate never overrides the actual LAN address");
    }

    #[test]
    fn an_old_daemons_slots_remain_visible_and_usable_without_program_fallback() {
        let mut data = super::super::demo::demo_data();
        data.settings.ai_slots.chat = None;
        let account = "ai:openrouter@openrouter.ai".to_string();
        let mut state =
            State { active: true, selected: Some(account.clone()), flight: Some(Work::Recommend), ..State::default() };
        let old = AiRecommendation {
            hardware: Default::default(),
            tier: "small".into(),
            tier_label: tr("Small").into(),
            preparations: vec![],
            slots: vec![torro_ipc::SlotSuggestion {
                slot: AiSlot::Chat,
                engine: Some(account.clone()),
                provider: AiProvider::OpenRouter,
                model: Some("qwen/qwen3-32b".into()),
                local: false,
                seat: tr("Cloud").into(),
                download_bytes: None,
                by_chat: false,
                reason: tr("Ready").into(),
            }],
        };
        let owner = state.owner;
        answered(&mut state, owner, 0, &Ok(Response::AiRecommendation { recommendation: old }));
        assert!(can_use(&state, &data));
        drop(view_in(&state, &data, true));
        let (_, _, request) = tag(update(&mut state, Message::Use, &data));
        assert!(matches!(request,Request::AiModels { account: id, .. } if id == account));
    }

    #[test]
    fn switching_accounts_never_reuses_another_accounts_candidate_index() {
        let mut data = super::super::demo::demo_data();
        data.settings.ai_slots.chat = None;
        let mut state = State::default();
        demo(&mut state, "ready", &data);
        let first = state.selected.clone().expect("first account");
        let recommendation = state.recommendation.as_mut().expect("recommendation");
        let mut choices = recommendation.preparations[0].clone();
        choices.candidates = vec![choices.candidates[0].clone(); 3];
        recommendation.preparations[0] = choices;
        let second = "ai:openrouter@openrouter.ai".to_string();
        let mut other = recommendation.preparations[0].clone();
        other.account = second.clone();
        other.candidates.truncate(1);
        recommendation.preparations.push(other);
        update(&mut state, Message::ChooseModel(AiSlot::Chat, 2), &data);
        let response =
            Ok(Response::AiRecommendation { recommendation: state.recommendation.clone().expect("recommendation") });
        let (owner, run, _) = tag(update(&mut state, Message::Select(second.clone()), &data));
        answered(&mut state, owner, run, &response);
        assert!(can_use(&state, &data), "the second account's only candidate remains usable");
        let (_, _, request) = tag(update(&mut state, Message::Use, &data));
        assert!(matches!(request, Request::AiModels { account, .. } if account == second));
        assert_ne!(state.selected.as_ref(), Some(&first));
    }

    #[test]
    fn a_slow_background_recommendation_cannot_block_or_overwrite_a_connection_form() {
        let data = super::super::demo::demo_data();
        let mut state = State::default();
        let (owner, run, _) = tag(open(&mut state));
        let (_, old_run, _) =
            tag(answered(&mut state, owner, run, &Ok(Response::AiServers { servers: vec![], observations: vec![] })));
        update(&mut state, Message::Online(AiProvider::OpenRouter), &data);
        assert!(state.form.is_some(), "a pending recommendation must not block provider selection");
        update(&mut state, Message::Key(Secret::new("private-key")), &data);
        let response = super::super::demo::demo_answer(&Request::AiRecommend, &data).expect("recommendation");
        assert!(matches!(answered(&mut state, owner, old_run, &Ok(response)), Action::None));
        assert!(state.recommendation.is_none(), "the superseded background answer is discarded");
        let (_, _, request) = tag(update(&mut state, Message::Submit, &data));
        assert!(matches!(request, Request::ConnectAi { provider: AiProvider::OpenRouter, .. }));
    }

    #[test]
    fn opting_out_of_sorting_excludes_local_suggestions_and_stops_late_preparation() {
        let mut data = super::super::demo::demo_data();
        data.settings.ai_slots.decide = None;
        let mut state = State::default();
        demo(&mut state, "consent", &data);
        update(&mut state, Message::Cancel, &data);
        let recommendation = state.recommendation.as_mut().expect("recommendation");
        recommendation.preparations[0].candidates[0].local = true;
        allow_sorting(&mut state, false);
        assert!(!can_use(&state, &data));
        assert!(matches!(update(&mut state, Message::Use, &data), Action::None));
        assert!(state.flight.is_none() && state.current.is_none() && !state.consent);

        allow_sorting(&mut state, true);
        let (owner, run, request) = tag(update(&mut state, Message::Use, &data));
        assert!(matches!(request, Request::AiModels { .. }));
        allow_sorting(&mut state, false);
        let account = state.selected.clone().expect("account");
        let answer = Ok(Response::AiModels { account, models: vec![] });
        let (_, _, request) = tag(answered(&mut state, owner, run, &answer));
        assert!(matches!(request, Request::AiRecommend), "no model enable, download or slot request follows");
        assert!(state.current.is_none() && state.queue.is_empty());
    }
}
