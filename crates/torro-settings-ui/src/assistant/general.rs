//! Assistant › General (task S.17): whether the assistant is on, and what a new chat starts with.
//!
//! **Switching it on and off is Access's** (task S.34): the section with the rights, the only one
//! that sends `clients.*` ([`super::access`]). So the status is shown here and the row leads there
//! with [`Action::Open`] – "Turn it on under Access" while it is off, "Change under Access" while it
//! is on. The group "Engines" is gone with the page it stood on: the engines are AI accounts, and
//! accounts belong to the whole suite.
//!
//! **New chats** (issue #131): the engine, model and effort a new chat starts with
//! (`settings.set_setting assistant_default`), or "The one used last" (`null`). The engines come
//! from `assistant.list_engines` – only AI accounts since issue #219; a coding agent would bring
//! its own models –, an AI account's models from
//! `ai.list_models` ([`super::models_request`]); the effort only where the model takes one. Each
//! chat can still pick another. Without any engine the group says where an AI account is added.
//!
//! **Only models turned on** (task S.26): "Model" offers an AI account's models that are turned on
//! for the suite (`AiModel::enabled`) and ends with "Manage models…", which leads to the control
//! centre's AI section, where they are switched. A stored model that is off stands on top with
//! "Turned off in Settings › AI", because a new chat answers with "Automatic" then. With AI
//! accounts but no model on anywhere – the core lists no engine then – the group says so
//! ([`super::no_model_on`]).

use iced::widget::{column, svg};
use iced::{Element, Theme};

use torro_core::ai::{AiModel, Effort};
use torro_core::i18n::tr;
use torro_core::settings::AssistantDefault;
use torro_gui::icon::{self, Icon};
use torro_gui::layout;
use torro_gui::theme::palette;
use torro_gui::widgets::{self, Status};
use torro_ipc::assistant::EngineKind;
use torro_ipc::{Request, SettingKey};

use super::{AI_SECTION, Action, access};
use crate::Data;
use crate::parts::{chevron, choice, pressable, setting_row};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The row of "Status": Access, where the assistant is switched and what it may use is chosen.
    Access,
    /// A row of "New chats", "Model" or "Effort": the whole default as it is to be stored, so two
    /// windows never leave half of it behind; `None` for the one used last.
    Default(Option<AssistantDefault>),
    /// "No AI account yet", "No model is turned on" and "Manage models…": the AI section of the
    /// control centre ([`AI_SECTION`]), where AI accounts are added and their models switched.
    ManageAi,
}

pub fn update(message: Message, _data: &Data) -> Action {
    match message {
        Message::Access => Action::Open(access::ID),
        Message::Default(default) => Action::send(Request::SetSetting {
            key: SettingKey::AssistantDefault,
            value: AssistantDefault::to_value(default.as_ref()),
        }),
        Message::ManageAi => Action::ControlCentre(AI_SECTION.to_string()),
    }
}

// ----- Views --------------------------------------------------------------------------------

/// The status, and while the assistant is on what a new chat starts with.
pub fn view(data: &Data) -> Element<'_, Message> {
    let mut page = column![status_group(data)].spacing(layout::section_gap());
    if super::is_on(data) {
        page = page.extend(default_groups(data));
    }
    page.into()
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

/// "New chats", and for a default engine its "Model" and, where the model takes one, its "Effort"
/// (issue #131): each row a check, a press stores the whole default.
fn default_groups(data: &Data) -> Vec<Element<'_, Message>> {
    // A stored default whose engine is not offered reads as "The one used last", because that is
    // what a new chat really starts with (the window skips it, `chat::preferred_choice`). Without
    // this the group would show no choice at all and a model under no engine. Issue #219 puts
    // everyone who had Claude Code there into this state; a removed AI account does the same.
    let current = data
        .settings
        .ai_slots
        .chat
        .as_ref()
        .filter(|default| data.engines.iter().any(|engine| engine.id == default.engine));
    let footer = tr("What a new chat starts with. Each chat can pick another.");
    if data.engines.is_empty() {
        let row = if super::no_model_on(data) { no_model_row() } else { no_engine_row() };
        return vec![widgets::group(tr("New chats"), vec![row], Some(footer.to_string()))];
    }
    let last = choice(tr("The one used last"), current.is_none(), Message::Default(None));
    let engines = data.engines.iter().map(|engine| {
        let chosen = current.is_some_and(|default| default.engine == engine.id);
        let effort = current.and_then(|default| default.effort);
        let default = AssistantDefault { engine: engine.id.clone(), model: None, effort };
        choice(engine.name.as_str(), chosen, Message::Default(Some(default)))
    });
    let mut groups =
        vec![widgets::group(tr("New chats"), std::iter::once(last).chain(engines).collect(), Some(footer.into()))];
    let Some(default) = current else { return groups };
    let pick = |model: Option<String>| AssistantDefault { model, ..default.clone() };
    let automatic = choice(tr("Automatic"), default.model.is_none(), Message::Default(Some(pick(None))));
    let off = default_turned_off(default, data);
    let rows = default_models(default, data).into_iter().map(|model| {
        let chosen = default.model.as_ref() == Some(&model.id);
        let message = Message::Default(Some(pick(Some(model.id))));
        match chosen && off {
            true => off_choice(model.name, message),
            false => choice(model.name, chosen, message),
        }
    });
    // Which models are offered is set in AI (task S.26), the whole suite at once.
    let manage = pressable(setting_row(None, tr("Manage models…"), None, arrow()), Some(Message::ManageAi));
    let rows = std::iter::once(automatic).chain(rows).chain([manage]).collect();
    groups.push(widgets::group(tr("Model"), rows, None));
    if takes_effort(default, data) {
        let rows = std::iter::once(None)
            .chain(Effort::ALL.into_iter().map(Some))
            .map(|effort| {
                let stored = AssistantDefault { effort, ..default.clone() };
                choice(effort_label(effort), default.effort == effort, Message::Default(Some(stored)))
            })
            .collect();
        let footer = tr("How hard the model thinks before it answers. More is slower and uses more tokens.");
        groups.push(widgets::group(tr("Effort"), rows, Some(footer.into())));
    }
    groups
}

/// The row of "New chats" without any engine: where an AI account is added.
fn no_engine_row<'a>() -> Element<'a, Message> {
    let detail = tr("Add Ollama on this computer or OpenRouter under AI");
    let row = setting_row(None, tr("No AI account yet"), Some(detail.to_string()), arrow());
    pressable(row, Some(Message::ManageAi))
}

/// The row of "New chats" with AI accounts but no model on at any (task S.26): where models are
/// turned on.
fn no_model_row<'a>() -> Element<'a, Message> {
    let detail = tr("Turn on models in Settings › AI.");
    let row = setting_row(None, tr("No model is turned on"), Some(detail.to_string()), arrow());
    pressable(row, Some(Message::ManageAi))
}

/// The stored model that is turned off (task S.26), as a row of "Model": its name, why a new chat
/// answers with "Automatic" instead, and the check of the stored choice.
fn off_choice<'a>(name: String, on_press: Message) -> Element<'a, Message> {
    let check = icon::icon(Icon::Check, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) });
    let detail = tr("Turned off in Settings › AI").to_string();
    pressable(setting_row(None, name, Some(detail), check), Some(on_press))
}

/// The arrow at the end of a row that leads to another window, as the section list has it
/// ("Appearance ↗"): 16, `text_3`.
fn arrow<'a, M: 'a>() -> Element<'a, M> {
    icon::icon(Icon::ArrowUpRight, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) })
        .into()
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
            let _ = view(data);
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
        assert_eq!(
            sent(update(Message::Default(Some(other.clone())), &data)),
            vec![Request::SetSetting {
                key: SettingKey::AssistantDefault,
                value: AssistantDefault::to_value(Some(&other))
            }]
        );
        let last = sent(update(Message::Default(None), &data));
        assert_eq!(last[0], Request::SetSetting { key: SettingKey::AssistantDefault, value: serde_json::Value::Null });

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
            let _ = view(&data);
        }
        assert_eq!(effort_label(Some(Effort::Medium)), tr("Medium"));
        assert_eq!(effort_label(None), tr("Automatic"));
    }

    /// Without any engine the group says where an AI account is added (it has its own section).
    #[tokio::test]
    async fn without_an_engine_it_points_at_ai() {
        let data = Data { engines: Vec::new(), accounts: Vec::new(), ..data().await };
        assert!(!super::super::no_model_on(&data), "no AI account at all");
        let action = update(Message::ManageAi, &data);
        assert!(matches!(action, Act::ControlCentre(ref section) if section == "ai"), "{action:?}");
        let _ = view(&data);
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
        let _ = view(&data);
        let action = update(Message::ManageAi, &data);
        assert!(matches!(action, Act::ControlCentre(ref section) if section == "ai"), "Manage models…");

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
        let _ = view(&data);
        let action = update(Message::ManageAi, &data);
        assert!(matches!(action, Act::ControlCentre(ref section) if section == "ai"), "{action:?}");
    }
}
