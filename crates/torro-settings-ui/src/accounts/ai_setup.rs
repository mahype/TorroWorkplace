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
const KEY_FIELD: &str = "ai-setup-key";

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
    /// The first-run setup shows this page and carries "Use suggestion" in its own footer (issue #326):
    /// the page leaves its own button out.
    hosted: bool,
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
            hosted: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Deterministic shared states used only by the non-network demo hosts.
    Demo(&'static str),
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
    state.error = None;
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

/// The host's Enter/Continue must not leave an open connection form or a consent decision.
pub fn primary(state: &State) -> Option<Message> {
    (state.form.is_some() || state.consent || state.current.is_some() || matches!(state.flight, Some(Work::Connect)))
        .then_some(Message::Submit)
}

/// The host describes the same action as Enter, without offering a no-op button during work.
pub fn primary_label(state: &State) -> Option<(&'static str, bool)> {
    primary(state)?;
    Some(if state.consent {
        (tr("Send mail outside this computer?"), false)
    } else if state.current.is_some() || state.pull.is_some() {
        (tr("Preparing model…"), false)
    } else if state.flight.is_some() {
        (tr("Connecting…"), false)
    } else {
        (tr("Connect"), true)
    })
}

/// A pending mail decision replaces the long overview rather than being appended below it.
pub fn awaiting_consent(state: &State) -> bool {
    state.consent
}

/// The first-run host also gives the new cloud form its own short page.
pub fn foreground(state: &State) -> bool {
    state.consent || state.form.as_ref().is_some_and(|(provider, _, _)| *provider == AiProvider::OllamaCloud)
}

fn key_reentry(state: &State) -> Option<&'static str> {
    (state.error.is_some()
        && state.error.as_deref() != Some(tr("Enter an API key to connect Ollama Cloud."))
        && state.key.is_blank()
        && state.form.as_ref().is_some_and(|(provider, _, _)| *provider == AiProvider::OllamaCloud))
    .then(|| tr("Enter the API key again to retry."))
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
        Some(if state.selected.as_deref().and_then(super::ai_provider) == Some(AiProvider::OllamaCloud) {
            tr("Loading models…")
        } else {
            tr("Looking on this computer…")
        })
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
        return if state.menus.update(event) { Action::Focus(widgets::DROPDOWN_SEARCH) } else { Action::None };
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
        Message::Demo(scenario) => {
            demo(state, scenario, data);
            Action::None
        }
        Message::Tick if !state.active => open(state),
        Message::Tick if state.flight.is_some() || state.form.is_some() || state.consent => Action::None,
        Message::Tick if state.pull.is_some() => send(state, Work::Pulls, Request::AiPulls),
        Message::Tick if state.selected.is_some() => refresh(state),
        Message::Retry if state.form.is_some() => update(state, Message::Submit, data),
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
            state.key = Secret::default();
            state.error = None;
            state.form = Some((provider, None, provider.default_url().unwrap_or_default().to_string()));
            Action::None
        }
        Message::Other if may_change_account(state) => {
            state.key = Secret::default();
            state.error = None;
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
            if provider == AiProvider::OllamaCloud && state.key.is_blank() {
                state.error = Some(tr("Enter an API key to connect Ollama Cloud.").into());
                return Action::Focus(KEY_FIELD);
            }
            state.error = None;
            let key = std::mem::take(&mut state.key);
            let request = match program {
                Some(program) => Request::ConnectLocalAi { program, server: Some(address), key },
                None if provider == AiProvider::OllamaCloud => Request::ConnectAi { provider, server: None, key },
                None => Request::ConnectAi { provider, server: Some(address), key },
            };
            send(state, Work::Connect, request)
        }
        Message::Back if may_change_account(state) => {
            state.form = None;
            state.key = Secret::default();
            state.error = None;
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

// ----- The suggestion as the first-run setup takes it (issue #326) ----------------------------------

/// The first-run setup shows this page with "Use suggestion" in its own footer: the page leaves its
/// own button out.
pub fn hosted(state: &mut State) {
    state.hosted = true;
}

/// The account whose suggestion "Use suggestion" of the first-run setup takes: the one chosen here,
/// else the first the recommendation prepares a model for.
pub fn suggested_account(state: &State) -> Option<&str> {
    state
        .selected
        .as_deref()
        .or_else(|| state.recommendation.as_ref()?.preparations.first().map(|p| p.account.as_str()))
}

/// The model suggested for `slot` on the [`suggested_account`], as the page shows it (the candidate
/// chosen under "Other model", else the first): the account and the candidate.
pub fn suggested_model(state: &State, slot: AiSlot) -> Option<(&str, &AiCandidate)> {
    let account = suggested_account(state)?;
    let preparation =
        state.recommendation.as_ref()?.preparations.iter().find(|p| p.account == account && p.slot == slot)?;
    let candidate = preparation.candidates.get(*state.choices.get(slot.key()).unwrap_or(&0))?;
    Some((account, candidate))
}

/// What "Use suggestion" would prepare, slot by slot: the suggested model of each slot on the
/// [`suggested_account`] that can be prepared here – Sorting only while the host allows it, a slot
/// already chosen only with `replace`, and with `only` just that slot.
fn suggestion(state: &State, data: &dyn Seen, replace: bool, only: Option<AiSlot>) -> Vec<(AiSlot, AiCandidate)> {
    let Some(account) = suggested_account(state) else { return Vec::new() };
    state
        .recommendation
        .iter()
        .flat_map(|r| &r.preparations)
        .filter(|p| {
            p.account == account
                && only.is_none_or(|slot| p.slot == slot)
                && (state.sorting || p.slot != AiSlot::Decide)
                && (replace || data.settings().ai_slots.get(p.slot).is_none())
        })
        .filter_map(|p| p.candidates.get(*state.choices.get(p.slot.key()).unwrap_or(&0)).cloned().map(|c| (p.slot, c)))
        .filter(|(_, c)| c.preparation != AiPreparationKind::External)
        .collect()
}

/// Whether "Use suggestion" has something to do now: no model is being prepared or connected, no
/// question waits, and the suggestion has a slot to fill (with `replace` also one already chosen, with
/// `only` that slot alone).
pub fn can_use_suggestion(state: &State, data: &dyn Seen, replace: bool, only: Option<AiSlot>) -> bool {
    !matches!(state.flight, Some(Work::Connect | Work::Enable | Work::Set | Work::Pull | Work::List | Work::Check))
        && state.current.is_none()
        && state.queue.is_empty()
        && !state.consent
        && state.form.is_none()
        && !suggestion(state, data, replace, only).is_empty()
}

/// Whether the suggestion differs from a model already chosen (issue #326, "Run setup again"): a slot
/// is chosen and the suggestion names another account or model for it. The page then keeps the choice
/// and offers the suggestion only on a click.
pub fn differs(state: &State, data: &dyn Seen) -> bool {
    let Some(account) = suggested_account(state) else { return false };
    suggestion(state, data, true, None).iter().any(|(slot, candidate)| {
        data.settings().ai_slots.get(*slot).is_some_and(|held| {
            held.engine != account
                || held.model.as_deref().is_none_or(|model| !super::same_model(model, &candidate.model))
        })
    })
}

/// "Use suggestion" of the first-run setup (issue #326): takes the [`suggested_account`] and prepares
/// its suggested models one after the other, as "Use suggestion" on the page does – downloads, turns
/// on and puts each into its slot; a model for Sorting elsewhere asks first ([`awaiting_consent`]).
/// With `replace` also slots already chosen, with `only` just that slot. Read-only work that runs is
/// superseded.
pub fn use_suggestion(state: &mut State, data: &dyn Seen, replace: bool, only: Option<AiSlot>) -> Action {
    if matches!(state.flight, Some(Work::Search | Work::Recommend)) {
        state.run = state.run.wrapping_add(1);
        state.flight = None;
    }
    if !can_use_suggestion(state, data, replace, only) {
        return Action::None;
    }
    if let Some(account) = suggested_account(state).map(str::to_string)
        && state.selected.as_deref() != Some(account.as_str())
    {
        // The candidates chosen belong to the suggested account: keep them while selecting it.
        let choices = std::mem::take(&mut state.choices);
        select_account(state, account, data);
        state.choices = choices;
    }
    state.active = true;
    state.queue = suggestion(state, data, replace, only);
    state.allowed_remote = None;
    state.consent = state.queue.iter().any(|(slot, c)| *slot == AiSlot::Decide && !c.local);
    if state.consent {
        return Action::None;
    }
    next(state)
}

/// A model is being prepared (downloaded, turned on, put into its slot) or waits to be: the host keeps
/// the page's work running when it shows another page meanwhile.
pub fn preparing(state: &State) -> bool {
    state.current.is_some() || state.pull.is_some() || !state.queue.is_empty()
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
        // An answer the chain cannot go on from ends it, so nothing waits for a model that never comes.
        _ => {
            state.current = None;
            state.queue.clear();
            state.pull = None;
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
    if state.menus.open().is_some() {
        Some(Message::Menus(DropdownEvent::Close))
    } else if state.consent {
        Some(Message::Cancel)
    } else if state.form.is_some() && state.flight.is_none() {
        Some(Message::Back)
    } else {
        None
    }
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
    if state.consent {
        return consent_view(state, data);
    }
    if state.form.as_ref().is_some_and(|(provider, _, _)| *provider == AiProvider::OllamaCloud) {
        return cloud_form(state);
    }
    let cloud = state.selected.as_deref().and_then(super::ai_provider) == Some(AiProvider::OllamaCloud);
    let mut parts = column![].spacing(layout::section_gap());
    if state.flight.is_some() || state.pull.is_some() {
        let status = if matches!(state.flight, Some(Work::Connect)) {
            tr("Connecting…")
        } else if state.current.is_some() {
            tr("Preparing model…")
        } else if state.selected.as_deref().and_then(super::ai_provider) == Some(AiProvider::OllamaCloud) {
            tr("Loading models…")
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
        parts = parts.push(typo::text(typo::body_strong(), name));
        let address = super::wizard::field(tr("Server address"), address)
            .on_input_maybe(state.flight.is_none().then_some(Message::Address))
            .on_submit_maybe(state.flight.is_none().then_some(Message::Submit));
        parts = parts.push(column![typo::text(typo::caption(), tr("Server address")), address].spacing(space::xs()));
        let key = super::wizard::field(tr("API key"), state.key.expose())
            .id(KEY_FIELD)
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
    if let Some(recommendation) = &state.recommendation
        && !cloud
    {
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
            if cloud {
                parts = parts.push(
                    iced::widget::row![
                        widgets::state_pill(widgets::Mark::Icon(Icon::CircleCheck), tr("Connected")),
                        typo::text(typo::meta(), AiProvider::OllamaCloud.seat().label()).style(theme::text_2),
                    ]
                    .spacing(space::s())
                    .align_y(iced::Alignment::Center),
                );
            } else {
                parts = parts.push(typo::text(typo::body_strong(), name.to_string()));
            }
        }
        // Hosted by the first-run setup, the suggestion shows before an account is chosen: its "Use
        // suggestion" takes the suggested one (issue #326).
        let shown = if state.hosted { suggested_account(state) } else { state.selected.as_deref() };
        for preparation in recommendation.preparations.iter().filter(|p| Some(p.account.as_str()) == shown) {
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
                if !candidate.local && !cloud {
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
            s.engine.as_deref() == shown
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
        if suggestions.is_empty()
            && state.selected.as_deref().and_then(super::ai_provider) == Some(AiProvider::OllamaCloud)
        {
            parts = parts
                .push(typo::text(typo::body(), tr("Account connected. No suggested models are available right now.")));
        }
        if !suggestions.is_empty() {
            let title = if cloud { tr("Suggested models") } else { tr("Our suggestion for this PC") };
            let hint = cloud.then(|| tr("Mail needs your explicit permission.").to_string());
            parts = parts.push(widgets::group(title, suggestions, hint));
            if !state.hosted {
                parts = parts.push(widgets::text_button(
                    None,
                    tr("Use suggestion"),
                    style::button::secondary,
                    can_use(state, data).then_some(Message::Use),
                ));
            }
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
    parts.into()
}

fn cloud_form(state: &State) -> Element<'_, Message> {
    let ready = state.flight.is_none();
    let provider = column![
        typo::text(typo::body_strong(), AiProvider::OllamaCloud.name()),
        typo::text(typo::meta(), AiProvider::OllamaCloud.seat().label()).style(theme::text_2),
    ]
    .spacing(space::xs());
    let explanation = column![
        typo::text(typo::body(), tr("Connect directly online. No local installation is needed.")),
        typo::text(typo::body(), tr("Text goes to this provider. Mail stays here unless you allow it.")),
    ]
    .spacing(space::s());
    let key = super::wizard::field(tr("API key"), state.key.expose())
        .id(KEY_FIELD)
        .secure(true)
        .on_input_maybe(ready.then_some(|key| Message::Key(Secret::new(key))))
        .on_submit_maybe(ready.then_some(Message::Submit));
    let mut field = column![
        column![typo::text(typo::caption(), tr("API key")), key].spacing(space::xs()),
        widgets::text_button(
            Some(Icon::ArrowUpRight),
            tr("Create an API key"),
            style::button::ghost,
            Some(Message::KeyPage),
        ),
    ]
    .spacing(space::s());
    if let Some(error) = &state.error {
        field = field.push(crate::parts::notice(error.clone()));
        if let Some(hint) = key_reentry(state) {
            field = field.push(typo::text(typo::meta(), hint).style(theme::text_2));
        }
    }
    column![provider, explanation, field].spacing(layout::section_gap()).into()
}

/// Both hosts put this sole cloud-form action row in their fixed footer, outside the scroller.
pub fn cloud_form_actions(state: &State) -> Option<Element<'_, Message>> {
    if !state.form.as_ref().is_some_and(|(provider, _, _)| *provider == AiProvider::OllamaCloud) || state.consent {
        return None;
    }
    let ready = state.flight.is_none();
    let connect = if ready {
        widgets::text_button(None, tr("Connect"), style::button::primary, Some(Message::Submit))
    } else {
        widgets::busy_button(tr("Connecting…"), style::button::primary)
    };
    let actions = iced::widget::row![
        widgets::text_button(None, tr("Back"), style::button::ghost, ready.then_some(Message::Back)),
        connect,
    ]
    .spacing(space::s());
    Some(actions.into())
}

fn consent_view<'a>(state: &'a State, data: &'a dyn Seen) -> Element<'a, Message> {
    let model = state
        .current
        .as_ref()
        .or_else(|| state.queue.iter().find(|(slot, _)| *slot == AiSlot::Decide))
        .map(|(_, candidate)| candidate.model.as_str())
        .unwrap_or_default();
    let account = data
        .accounts()
        .iter()
        .find(|a| Some(&a.id) == state.selected.as_ref())
        .map(|a| a.label.as_str())
        .or_else(|| state.selected.as_deref().and_then(super::ai_provider).map(AiProvider::name))
        .or(state.selected.as_deref())
        .unwrap_or_default();
    let question = trf(
        "{model} reads your mail at {provider} outside this computer. Use it?",
        &[("model", &model), ("provider", &account)],
    );
    let actions = iced::widget::row![
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Cancel)),
        widgets::text_button(None, tr("Use"), style::button::primary, Some(Message::Confirm)),
    ]
    .spacing(space::s());
    column![
        typo::text(typo::title(), tr("Send mail outside this computer?")),
        column![
            typo::text(typo::body(), question),
            typo::text(typo::body(), tr("Mail goes to this model outside this computer and cannot be taken back."))
                .style(theme::text_2),
        ]
        .spacing(space::s()),
        actions,
    ]
    .spacing(space::l())
    .into()
}

/// Deterministic, non-network states for headless design inspection.
pub fn demo(state: &mut State, scenario: &str, data: &dyn Seen) {
    state.demo = true;
    state.active = true;
    state.flight = None;
    state.found.clear();
    state.observations.clear();
    state.form = None;
    state.selected = None;
    state.key = Secret::default();
    state.error = None;
    state.consent = false;
    state.queue.clear();
    state.current = None;
    state.pull = None;
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
        "cloud-selection" => {
            state.ways = true;
        }
        "cloud-menu" => {
            state.ways = true;
            state.menus.update(DropdownEvent::Toggle(()));
        }
        "cloud-form" | "cloud-running" | "cloud-error" => {
            state.form = Some((AiProvider::OllamaCloud, None, "https://ollama.com/v1".into()));
            if scenario == "cloud-running" {
                state.flight = Some(Work::Connect);
            }
            if scenario == "cloud-error" {
                state.error = Some(torro_core::ai::AiError::Auth.to_string());
            }
        }
        "cloud-empty" | "cloud-connected" | "cloud-consent" => {
            let account = "ai:ollama_cloud@ollama.com".to_string();
            state.selected = Some(account.clone());
            state.base = "https://ollama.com/v1".into();
            recommendation.slots.clear();
            recommendation.preparations.clear();
            if scenario != "cloud-empty" {
                for (slot, model) in [(AiSlot::Chat, "gpt-oss:20b"), (AiSlot::Decide, "clef:27b")] {
                    recommendation.preparations.push(torro_ipc::AiPreparation {
                        account: account.clone(),
                        slot,
                        candidates: vec![AiCandidate {
                            model: model.into(),
                            size_bytes: 0,
                            fits: true,
                            local: false,
                            preparation: AiPreparationKind::Ready,
                            by_chat: false,
                        }],
                    });
                }
            }
            if scenario == "cloud-consent" {
                let candidate = recommendation.preparations[1].candidates[0].clone();
                state.queue = vec![(AiSlot::Decide, candidate)];
                state.consent = true;
            }
        }
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
        // The first-run setup's suggestion (issue #326): Ollama found and connected, both models to download.
        "suggestion" => {
            let account = super::demo::DEMO_OLLAMA.to_string();
            state.found.push(AiServer {
                provider: AiProvider::Ollama,
                program: None,
                url: "http://localhost:11434/v1".into(),
                models: 4,
                account: Some(account.clone()),
            });
            recommendation.preparations =
                [(AiSlot::Chat, "qwen3:14b", 9_276_198_565), (AiSlot::Decide, "clef:8b", 5_100_000_000)]
                    .into_iter()
                    .map(|(slot, model, size_bytes)| torro_ipc::AiPreparation {
                        account: account.clone(),
                        slot,
                        candidates: vec![AiCandidate {
                            model: model.into(),
                            size_bytes,
                            fits: true,
                            local: true,
                            preparation: AiPreparationKind::OllamaPull,
                            by_chat: false,
                        }],
                    })
                    .collect();
        }
        _ => {}
    }
    state.recommendation = Some(recommendation);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_primary_feedback_distinguishes_forms_work_and_explicit_consent() {
        let data = super::super::demo::demo_data();
        let mut state = State::default();
        demo(&mut state, "cloud-form", &data);
        assert!(foreground(&state));
        assert!(cloud_form_actions(&state).is_some(), "hosts own the one fixed Back/Connect row");
        assert_eq!(primary_label(&state), Some((tr("Connect"), true)));
        demo(&mut state, "cloud-running", &data);
        assert!(cloud_form_actions(&state).is_some(), "the same row reports work without a second action");
        assert_eq!(primary_label(&state), Some((tr("Connecting…"), false)));
        demo(&mut state, "cloud-consent", &data);
        assert!(foreground(&state) && awaiting_consent(&state));
        assert!(cloud_form_actions(&state).is_none(), "connection actions never compete with consent");
        assert_eq!(primary_label(&state), Some((tr("Send mail outside this computer?"), false)));
        assert!(matches!(update(&mut state, Message::Submit, &data), Action::None));
        assert!(state.consent && state.flight.is_none(), "Enter never supplies consent");
        update(&mut state, Message::Cancel, &data);
        assert!(cloud_form_actions(&state).is_none());
        assert!(!foreground(&state) && primary_label(&state).is_none());
        demo(&mut state, "download", &data);
        assert_eq!(primary_label(&state), Some((tr("Preparing model…"), false)));
    }

    #[test]
    fn ollama_cloud_form_requires_a_key_and_retries_the_connection_not_discovery() {
        let data = super::super::demo::demo_data();
        let provider = serde_json::from_str("\"ollama_cloud\"").expect("provider");
        let mut state = State::default();
        assert!(matches!(update(&mut state, Message::Menus(DropdownEvent::Toggle(())), &data), Action::Focus(id)
            if id == widgets::DROPDOWN_SEARCH));
        update(&mut state, Message::Online(provider), &data);
        assert!(matches!(update(&mut state, Message::Submit, &data), Action::Focus(_)));
        assert!(state.flight.is_none() && state.error.is_some());
        update(&mut state, Message::Key(Secret::new("synthetic-key")), &data);
        let (owner, run, request) = tag(update(&mut state, Message::Submit, &data));
        assert!(
            matches!(request, Request::ConnectAi { provider: p, server: None, key } if p == provider && !key.is_blank())
        );
        assert!(state.key.is_blank());
        answered(&mut state, owner, run, &Ok(Response::Error { code: "auth".into(), message: "refused".into() }));
        assert!(state.form.is_some() && state.error.is_some() && state.selected.is_none());
        assert_eq!(key_reentry(&state), Some(tr("Enter the API key again to retry.")));
        assert!(matches!(update(&mut state, Message::Retry, &data), Action::Focus(_)));
        assert!(state.flight.is_none(), "retry without a key cannot make another request");
        update(&mut state, Message::Key(Secret::new("replacement")), &data);
        assert!(key_reentry(&state).is_none());
        let (_, _, request) = tag(update(&mut state, Message::Retry, &data));
        assert!(matches!(request, Request::ConnectAi { server: None, .. }));
        state.flight = None;
        update(&mut state, Message::Back, &data);
        assert!(state.key.is_blank() && state.error.is_none() && state.form.is_none());
        state.key = Secret::new("discard on leave");
        state.error = Some("old error".into());
        leave(&mut state);
        assert!(state.key.is_blank() && state.error.is_none());
    }

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
        assert!(overlay(&state).is_none(), "a choice closes the provider menu");
        assert_eq!(escape(&state), Some(Message::Back), "Escape now goes back from the open form");
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
