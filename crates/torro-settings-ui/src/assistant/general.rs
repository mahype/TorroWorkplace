//! Assistant › General (task S.17): whether the assistant is on, and what a new chat starts with.
//!
//! **Switching it on and off is Access's** (task S.34): the section with the rights, the only one
//! that sends `clients.*` ([`super::access`]). So the status is shown here and the row leads there
//! with [`Action::Open`] – "Turn it on under Access" while it is off, "Change under Access" while it
//! is on. The group "Engines" is gone with the page it stood on: the engines are AI accounts, and
//! accounts belong to the whole suite.
//!
//! **New chats** (issue #131): the engine, model and effort a new chat starts with – the slot *Chat*
//! (`ai.set_slot chat` since issue #325, task MW.8; read from `aiSlots.chat`) –, or "The one used
//! last" (the slot emptied). The engines come
//! from `assistant.list_engines` – only AI accounts since issue #219; a coding agent would bring
//! its own models –, an AI account's models from
//! `ai.list_models` ([`super::models_request`]); the effort only where the model takes one. Each
//! chat can still pick another. Without any engine the group says where an AI account is added.
//!
//! **One group, three controls (issue #547, the rule of choices):** "Account" and "Model" are Torro
//! dropdowns ([`State::menus`], shown in [`overlay`]; the model's menu searches from
//! `widgets::SEARCH_FROM` models on), "Effort" a segmented control. This is the one place of the
//! window where the slot *Chat* is set: the section AI of the assistant leaves it out
//! (`accounts::ai_parts`), since only here are the effort and "The one used last".
//!
//! **Only models turned on** (task S.26): "Model" offers an AI account's models that are turned on
//! for the suite (`AiModel::enabled`), and "Manage models…" leads to where they are switched: the
//! section AI of these settings, or with the main application the control centre's
//! ([`manage_target`], issue #547). A stored model that is off is the label of its dropdown with
//! "Turned off in … › AI" beside it, because a new chat answers with "Automatic" then. With AI
//! accounts but no model on anywhere – the core lists no engine then – the group says so
//! ([`super::no_model_on`]).

use iced::Element;

use torro_core::ai::AiSlot;
use torro_core::ai::{AiModel, Effort};
use torro_core::i18n::tr;
use torro_core::settings::AssistantDefault;
use torro_gui::widgets::{self, Align, DROPDOWN_SEARCH, Dropdown, DropdownEvent, MenuItem, MenuSection, Status};
use torro_ipc::Request;
use torro_ipc::assistant::EngineKind;

use super::{AI_SECTION, Action, access};
use crate::Data;
use crate::parts::{chevron, pressable, segmented_row, setting_row};

/// The dropdowns of the page (issue #547).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// "Account": the engine of new chats, or "The one used last".
    Engine,
    /// "Model": "Automatic" or one of the engine's models.
    Model,
}

/// What the page keeps between frames: which dropdown is open.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub menus: Dropdown<Choice>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A dropdown opened, closed or was searched; changes nothing.
    Menus(DropdownEvent<Choice>),
    /// The row of "Status": Access, where the assistant is switched and what it may use is chosen.
    Access,
    /// An entry of "Account" or "Model", or a segment of "Effort": the whole default as it is to be
    /// stored, so two windows never leave half of it behind; `None` for the one used last.
    Default(Option<AssistantDefault>),
    /// "No model is turned on" and "Manage models…": where models are switched ([`manage_target`]).
    ManageAi,
    /// "No AI account yet": the section AI of these settings ([`AI_SECTION`], issue #410), where AI
    /// accounts are added – with the main application too, since the section keeps its accounts.
    AddAi,
}

/// Opening a dropdown asks for the focus of its search field; any other message closes it.
pub fn update(state: &mut State, message: Message, _data: &Data) -> Action {
    let message = match message {
        Message::Menus(event) => {
            return if state.menus.update(event) { Action::Focus(DROPDOWN_SEARCH) } else { Action::None };
        }
        other => other,
    };
    state.menus.close();
    match message {
        Message::Menus(_) => Action::None,
        Message::Access => Action::Open(access::ID),
        Message::Default(default) => Action::send(set_chat(default)),
        Message::ManageAi => manage_target(torro_gui::installed::main_app()),
        Message::AddAi => Action::Open(AI_SECTION),
    }
}

/// Where models are turned on and off (task S.26, issue #547): with the main application its control
/// centre at AI, since the AI section of an app window then shows only the accounts; without it the
/// section AI of these settings, which has them all.
pub fn manage_target(main_app: bool) -> Action {
    match main_app {
        true => Action::ControlCentre(AI_SECTION.to_string()),
        false => Action::Open(AI_SECTION),
    }
}

/// `ai.set_slot chat` with the whole default (issue #325), or with no engine for "The one used last".
fn set_chat(default: Option<AssistantDefault>) -> Request {
    let (engine, model, effort) = match default {
        Some(AssistantDefault { engine, model, effort }) => (Some(engine), model, effort),
        None => (None, None, None),
    };
    Request::SetAiSlot { slot: AiSlot::Chat, engine, model, effort }
}

// ----- Views --------------------------------------------------------------------------------

/// The status, and while the assistant is on what a new chat starts with.
pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    // Side by side where the page is wide (issue #464): status and "New chats".
    widgets::page_columns(move || {
        let mut groups = vec![status_group(data)];
        if super::is_on(data) {
            groups.push(default_group(state, data));
        }
        groups
    })
}

/// The open dropdown of "Account" or "Model", under its button.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let key = state.menus.open()?;
    let current = current_default(data);
    let items = match key {
        Choice::Engine => {
            let last = MenuItem::plain(tr("The one used last"), Message::Default(None)).current(current.is_none());
            let engines = data.engines.iter().map(|engine| {
                let effort = current.and_then(|default| default.effort);
                let default = AssistantDefault { engine: engine.id.clone(), model: None, effort };
                let chosen = current.is_some_and(|default| default.engine == engine.id);
                MenuItem::plain(engine.name.as_str(), Message::Default(Some(default))).current(chosen)
            });
            std::iter::once(last).chain(engines).collect()
        }
        Choice::Model => {
            let default = current?;
            let pick = |model: Option<String>| Message::Default(Some(AssistantDefault { model, ..default.clone() }));
            let automatic = MenuItem::plain(tr("Automatic"), pick(None)).current(default.model.is_none());
            let off = default_turned_off(default, data);
            let models = default_models(default, data).into_iter().map(|model| {
                let chosen = default.model.as_ref() == Some(&model.id);
                let item = MenuItem::plain(model.name, pick(Some(model.id))).current(chosen);
                match chosen && off {
                    true => item.detail(turned_off_text()),
                    false => item,
                }
            });
            std::iter::once(automatic).chain(models).collect()
        }
    };
    let sections = vec![MenuSection::new(items)];
    Some(state.menus.view(key, sections, widgets::menu_width(), Align::End, Message::Menus))
}

/// Escape closes the open dropdown first.
pub fn escape(state: &State) -> Option<Message> {
    state.menus.open().map(|_| Message::Menus(DropdownEvent::Close))
}

/// "Status": the assistant on or off, and the one way on from here – the whole row opens Access,
/// which switches it. A row, not a button in a row, so the words fit in every size class (design
/// skill, section 6); a chevron, not the arrow of another window: Access is in this one.
fn status_group(data: &Data) -> Element<'_, Message> {
    let on = super::is_on(data);
    let (dot, label, word) = if on {
        (Status::Ok, tr("The assistant is on"), tr("Change under Access"))
    } else {
        (Status::Inactive, tr("The assistant is off"), tr("Turn it on under Access"))
    };
    let row = setting_row(Some(widgets::status_dot(dot)), label, Some(word.to_string()), chevron());
    let footer = tr("Turning the assistant on or off and what it may use are set under Access.");
    widgets::group(tr("Status"), vec![pressable(row, Some(Message::Access))], Some(footer.to_string()))
}

/// The stored default of new chats, while its engine is offered.
///
/// A stored default whose engine is not offered reads as "The one used last", because that is what a
/// new chat really starts with (the window skips it, `chat::preferred_choice`). Without this the
/// group would show no choice at all and a model under no engine. Issue #219 puts everyone who had
/// Claude Code there into this state; a removed AI account does the same.
fn current_default(data: &Data) -> Option<&AssistantDefault> {
    data.settings.ai_slots.chat.as_ref().filter(|default| data.engines.iter().any(|engine| engine.id == default.engine))
}

/// "New chats" (issue #131, as dropdowns since issue #547): "Account", and for a default engine its
/// "Model", "Manage models…" and, where the model takes one, its "Effort". Every choice stores the
/// whole default.
fn default_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let footer = tr("What a new chat starts with. Each chat can pick another.");
    if data.engines.is_empty() {
        let row = if super::no_model_on(data) { no_model_row() } else { no_engine_row() };
        return widgets::group(tr("New chats"), vec![row], Some(footer.to_string()));
    }
    let current = current_default(data);
    let engine = current
        .and_then(|default| data.engines.iter().find(|engine| engine.id == default.engine))
        .map_or_else(|| tr("The one used last").to_string(), |engine| engine.name.clone());
    let account = state.menus.button(Choice::Engine, engine, Message::Menus);
    let mut rows = vec![setting_row(None, tr("Account"), None, account)];
    if let Some(default) = current {
        let off = default_turned_off(default, data);
        let label = match &default.model {
            None => tr("Automatic").to_string(),
            Some(id) => default_models(default, data)
                .into_iter()
                .find(|model| &model.id == id)
                .map_or_else(|| id.clone(), |model| model.name),
        };
        let model = state.menus.button(Choice::Model, label, Message::Menus);
        rows.push(setting_row(None, tr("Model"), off.then(|| turned_off_text().to_string()), model));
        // Which models are offered is set in AI (task S.26), the whole suite at once.
        let manage = setting_row(None, tr("Manage models…"), None, chevron());
        rows.push(pressable(manage, Some(Message::ManageAi)));
        if takes_effort(default, data) {
            let efforts: Vec<Option<Effort>> = std::iter::once(None).chain(Effort::ALL.into_iter().map(Some)).collect();
            let active = efforts.iter().position(|effort| *effort == default.effort).unwrap_or(0);
            let options = efforts
                .into_iter()
                .map(|effort| {
                    let stored = AssistantDefault { effort, ..default.clone() };
                    (effort_label(effort).to_string(), Message::Default(Some(stored)))
                })
                .collect();
            let about = tr("How hard the model thinks before it answers. More is slower and uses more tokens.");
            rows.push(segmented_row(tr("Effort"), Some(about.to_string()), options, active));
        }
    }
    widgets::group(tr("New chats"), rows, Some(footer.to_string()))
}

/// Where a model that is off is turned on again, as the line beside it: the control centre with the
/// main application, these settings without (issue #547).
fn turned_off_text() -> &'static str {
    match torro_gui::installed::main_app() {
        true => tr("Turned off in Torro Workplace › AI"),
        false => tr("Turned off in Settings › AI"),
    }
}

/// The row of "New chats" without any engine: where an AI account is added.
fn no_engine_row<'a>() -> Element<'a, Message> {
    let detail = tr("Add Ollama on this computer or OpenRouter under AI");
    let row = setting_row(None, tr("No AI account yet"), Some(detail.to_string()), chevron());
    pressable(row, Some(Message::AddAi))
}

/// The row of "New chats" with AI accounts but no model on at any (task S.26): where models are
/// turned on.
fn no_model_row<'a>() -> Element<'a, Message> {
    let detail = match torro_gui::installed::main_app() {
        true => tr("Turn on models in Torro Workplace › AI."),
        false => tr("Turn on models in Settings › AI."),
    };
    let row = setting_row(None, tr("No model is turned on"), Some(detail.to_string()), chevron());
    pressable(row, Some(Message::ManageAi))
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The models offered for the default's engine (issue #131): an agent's own, an AI account's that
/// can call tools and are turned on (task S.26, [`Data::default_models`]); the stored model first
/// when it is none of them (turned off, set in a chat, or gone from the server), so its check shows
/// – with its name and what is known of it where the account still lists it.
pub fn default_models(default: &AssistantDefault, data: &Data) -> Vec<AiModel> {
    let engine = data.engines.iter().find(|engine| engine.id == default.engine);
    let mut models: Vec<AiModel> = match engine.map(|engine| engine.kind) {
        Some(EngineKind::Agent) => engine.map(|engine| engine.models.clone()).unwrap_or_default(),
        Some(EngineKind::Ai) => {
            data.default_models.iter().filter(|model| model.tools != Some(false) && model.enabled).cloned().collect()
        }
        Some(EngineKind::Test) | None => Vec::new(),
    };
    if let Some(stored) = default.model.as_ref().filter(|id| !models.iter().any(|model| &model.id == *id)) {
        let listed = data.default_models.iter().find(|model| &model.id == stored);
        models.insert(0, listed.cloned().unwrap_or_else(|| AiModel::named(stored)));
    }
    models
}

/// Whether the default names a model of an AI account that is not turned on (task S.26): the
/// account's models are in and have it off, or not at all. A new chat answers with "Automatic"
/// then, which "Model" says beside it. `false` for "Automatic", an agent's model, and while the
/// account's models are not in.
pub fn default_turned_off(default: &AssistantDefault, data: &Data) -> bool {
    let Some(model) = &default.model else { return false };
    let ai = data.engines.iter().any(|engine| engine.id == default.engine && engine.kind == EngineKind::Ai);
    ai && !data.default_models.is_empty()
        && !data.default_models.iter().any(|known| &known.id == model && known.enabled)
}

/// Whether the default's model takes an effort (issue #131), as the chat decides it: an agent left
/// to pick takes one, an AI account left to pick does not, a model not known to lack thinking does.
pub fn takes_effort(default: &AssistantDefault, data: &Data) -> bool {
    let Some(engine) = data.engines.iter().find(|engine| engine.id == default.engine) else { return false };
    match (&default.model, engine.kind) {
        (_, EngineKind::Test) | (None, EngineKind::Ai) => false,
        (None, EngineKind::Agent) => true,
        (Some(id), _) => default_models(default, data)
            .iter()
            .find(|model| &model.id == id)
            .is_some_and(|model| model.reasoning != Some(false)),
    }
}

/// The word for an effort: "Automatic" for `None`, else "Low", "Medium", "High".
pub fn effort_label(effort: Option<Effort>) -> &'static str {
    match effort {
        None => tr("Automatic"),
        Some(Effort::Low) => tr("Low"),
        Some(Effort::Medium) => tr("Medium"),
        Some(Effort::High) => tr("High"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{data, turned_off};
    use super::*;
    use crate::Action as Act;

    /// `update` with a fresh state: what a row sends does not depend on an open dropdown.
    fn update(message: Message, data: &Data) -> Action {
        super::update(&mut State::default(), message, data)
    }

    /// "Manage models…" leads to the control centre or to the section AI, as the main application
    /// is there or not; other tests may change what is installed meanwhile, so either is fine here
    /// and [`manage_target`] is tested on its own.
    fn manages_ai(action: &Action) -> bool {
        matches!(action, Act::Open("ai")) || matches!(action, Act::ControlCentre(section) if section == "ai")
    }

    fn sent(action: Action) -> Vec<Request> {
        match action {
            Act::Send(requests) => requests,
            other => panic!("expected requests, got {other:?}"),
        }
    }

    /// Task S.34: the status opens Access in this window, both ways round, and sends nothing itself.
    #[tokio::test]
    async fn the_status_only_points_at_access() {
        let on = data().await;
        let off = turned_off(&on);
        for data in [&on, &off] {
            assert_eq!(update(Message::Access, data), Act::Open("access"));
            let _ = view(&State::default(), data);
        }
        assert!(super::super::is_on(&on) && !super::super::is_on(&off));
    }

    /// Issue #131: the default of new chats is stored whole, its models are the engine's, and the
    /// effort is offered only where the model takes one.
    #[tokio::test]
    async fn the_default_of_new_chats_is_stored_whole() {
        let mut data = data().await;
        let stored = data.settings.ai_slots.chat.clone().expect("the demo sets one");
        let other = AssistantDefault { model: None, ..stored.clone() };
        // Issue #325: the slot Chat, through `ai.set_slot`, no longer the old setting.
        assert_eq!(
            sent(update(Message::Default(Some(other.clone())), &data)),
            vec![Request::SetAiSlot {
                slot: AiSlot::Chat,
                engine: Some(other.engine.clone()),
                model: None,
                effort: other.effort
            }]
        );
        let last = sent(update(Message::Default(None), &data));
        assert_eq!(last[0], Request::SetAiSlot { slot: AiSlot::Chat, engine: None, model: None, effort: None });

        // An AI account's models come from the load's second round; left to pick, nobody knows
        // whether the model thinks, so no effort is offered.
        let ids: Vec<String> = default_models(&stored, &data).into_iter().map(|model| model.id).collect();
        assert_eq!(ids, ["qwen3:8b"]);
        assert!(takes_effort(&stored, &data), "qwen3 thinks");
        assert!(!takes_effort(&other, &data), "the account picks: unknown");
        let closed = AssistantDefault { model: Some("openai/gpt-5".into()), ..stored.clone() };
        assert_eq!(default_models(&closed, &data)[0].id, "openai/gpt-5", "a stored model shows");

        // An agent brings its own models, each with whether it takes an effort. The demo offers
        // no agent any more (task C.19, issue #219), so the test brings its own: the kind stays in
        // the protocol, so the group must still show one.
        let agent = super::super::tests::agent_engine();
        data.engines.push(agent.clone());
        let on_agent = AssistantDefault { engine: agent.id.clone(), model: None, effort: None };
        let ids: Vec<String> = default_models(&on_agent, &data).into_iter().map(|model| model.id).collect();
        assert_eq!(ids, ["opus", "sonnet", "haiku"]);
        assert!(takes_effort(&on_agent, &data), "an agent left to pick picks one that thinks");
        let quick = AssistantDefault { model: Some("haiku".into()), ..on_agent.clone() };
        assert!(!takes_effort(&quick, &data), "Haiku takes no effort");

        for default in [stored, on_agent, quick, closed] {
            data.settings.ai_slots.chat = Some(default);
            let _ = view(&State::default(), &data);
        }
        assert_eq!(effort_label(Some(Effort::Medium)), tr("Medium"));
        assert_eq!(effort_label(None), tr("Automatic"));
    }

    /// Without any engine the group says where an AI account is added (it has its own section).
    #[tokio::test]
    async fn without_an_engine_it_points_at_ai() {
        let data = Data { engines: Vec::new(), accounts: Vec::new(), ..data().await };
        assert!(!super::super::no_model_on(&data), "no AI account at all");
        // Accounts are added in this window's own section AI, with the main application too.
        let action = update(Message::AddAi, &data);
        assert!(matches!(action, Act::Open(section) if section == "ai"), "{action:?}");
        let _ = view(&State::default(), &data);
    }

    /// Task S.26: "Model" offers only the models turned on; a stored model that is off is shown on
    /// top as turned off, "Automatic" and a model that is on are not.
    #[tokio::test]
    async fn new_chats_offer_only_models_that_are_on() {
        let mut data = data().await;
        let stored = data.settings.ai_slots.chat.clone().expect("the demo sets one");
        assert!(data.default_models.iter().any(|model| model.id == "llama3.3:70b" && !model.enabled));
        let ids: Vec<String> = default_models(&stored, &data).into_iter().map(|model| model.id).collect();
        assert_eq!(ids, ["qwen3:8b"], "llama3.3:70b is off, nimble cannot use tools");
        assert!(!default_turned_off(&stored, &data), "qwen3:8b is on");
        assert!(!default_turned_off(&AssistantDefault { model: None, ..stored.clone() }, &data), "Automatic");

        let off = AssistantDefault { model: Some("llama3.3:70b".into()), ..stored.clone() };
        let ids: Vec<String> = default_models(&off, &data).into_iter().map(|model| model.id).collect();
        assert_eq!(ids, ["llama3.3:70b", "qwen3:8b"], "the stored model stands on top");
        assert!(default_turned_off(&off, &data));
        let unknown = AssistantDefault { model: Some("gone:1b".into()), ..stored.clone() };
        assert!(default_turned_off(&unknown, &data), "a model the account no longer lists is not on either");
        data.settings.ai_slots.chat = Some(off);
        let _ = view(&State::default(), &data);
        let action = update(Message::ManageAi, &data);
        assert!(manages_ai(&action), "Manage models…: {action:?}");

        // Nothing known about the account's models yet: no claim that the model is off.
        let unknown_list = Data { default_models: Vec::new(), ..data.clone() };
        assert!(!default_turned_off(data.settings.ai_slots.chat.as_ref().expect("set"), &unknown_list));
    }

    /// Task S.26: AI accounts but no model on at any – the core lists no engine – reads "No model
    /// is turned on", not "No AI account yet", and leads to AI all the same.
    #[tokio::test]
    async fn with_ai_accounts_but_no_model_on_it_says_so() {
        let data = Data { engines: Vec::new(), ..data().await };
        assert!(data.accounts.iter().any(|account| account.kind == "ai"), "the demo has an AI account");
        assert!(super::super::no_model_on(&data));
        assert!(!super::super::no_model_on(&super::super::tests::data().await), "an engine: models are on");
        let _ = view(&State::default(), &data);
        let action = update(Message::ManageAi, &data);
        assert!(manages_ai(&action), "{action:?}");
    }

    /// Issue #547: with the main application models are switched in its control centre, without it
    /// in the section AI of these settings.
    #[test]
    fn models_are_managed_where_they_are_switched() {
        assert_eq!(manage_target(true), Act::ControlCentre("ai".to_string()));
        assert_eq!(manage_target(false), Act::Open("ai"));
    }

    /// Issue #547: "Account" and "Model" are dropdowns – opening one asks for the focus, Escape
    /// closes it, a choice closes it and stores the whole default; "Effort" is a segmented control
    /// whose segments store it the same way.
    #[tokio::test]
    async fn the_dropdowns_open_close_and_choose() {
        let data = data().await;
        let stored = data.settings.ai_slots.chat.clone().expect("the demo sets one");
        let mut state = State::default();
        assert!(overlay(&state, &data).is_none() && escape(&state).is_none(), "nothing open");
        for key in [Choice::Engine, Choice::Model] {
            let open = super::update(&mut state, Message::Menus(DropdownEvent::Toggle(key)), &data);
            assert_eq!(open, Act::Focus(DROPDOWN_SEARCH));
            assert_eq!(state.menus.open(), Some(&key));
            assert!(overlay(&state, &data).is_some(), "{key:?}");
            assert_eq!(escape(&state), Some(Message::Menus(DropdownEvent::Close)));
            let _ = view(&state, &data);
            let closed = super::update(&mut state, Message::Menus(DropdownEvent::Close), &data);
            assert_eq!(closed, Act::None);
            assert!(overlay(&state, &data).is_none());
        }
        let _ = super::update(&mut state, Message::Menus(DropdownEvent::Toggle(Choice::Model)), &data);
        let automatic = AssistantDefault { model: None, ..stored.clone() };
        let chosen = super::update(&mut state, Message::Default(Some(automatic.clone())), &data);
        assert!(matches!(&sent(chosen)[..], [Request::SetAiSlot { model: None, .. }]));
        assert_eq!((state.menus.open(), escape(&state)), (None, None), "the choice closes it");

        // "The one used last" shows no "Model": its menu has nothing to offer.
        let last = Data { settings: torro_core::settings::Settings::default(), ..data.clone() };
        let _ = super::update(&mut state, Message::Menus(DropdownEvent::Toggle(Choice::Model)), &last);
        assert!(overlay(&state, &last).is_none());
        let _ = view(&state, &last);
    }
}
