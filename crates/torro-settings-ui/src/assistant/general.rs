//! Assistant › General (task S.17): whether the assistant is on, and what a new chat starts with.
//!
//! **Switching it on and off is the control centre's** (`docs/api.md`, "Was nie nach außen geht"):
//! `clients.create_client` gives out a key, and no app process may ask for one. So the status is
//! shown here and the row points at the control centre with [`Action::ControlCentre`] –
//! "Turn on in Torro Workplace ↗" while it is off, "Change in Torro Workplace ↗" while it is on,
//! both leading to [`super::CONTROL_CENTRE`]. The group "Engines" is gone with the page it stood
//! on: the engines are AI accounts, and accounts belong to the whole suite.
//!
//! **New chats** (issue #131): the engine, model and effort a new chat starts with
//! (`settings.set_setting assistant_default`), or "The one used last" (`null`). The engines come
//! from `assistant.list_engines` – only AI accounts since issue #219; a coding agent would bring
//! its own models –, an AI account's models from
//! `ai.list_models` ([`super::models_request`]); the effort only where the model takes one. Each
//! chat can still pick another. Without any engine the group says where an AI account is added.

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

use super::{AI_SECTION, Action, CONTROL_CENTRE};
use crate::Data;
use crate::parts::{choice, pressable, setting_row};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// "Turn on in Torro Workplace ↗" or "Change in Torro Workplace ↗": the control centre, where
    /// the assistant is switched and what it may use is chosen.
    ControlCentre,
    /// A row of "New chats", "Model" or "Effort": the whole default as it is to be stored, so two
    /// windows never leave half of it behind; `None` for the one used last.
    Default(Option<AssistantDefault>),
    /// "No AI account yet": the accounts of the control centre ([`AI_SECTION`]).
    Accounts,
}

pub fn update(message: Message, _data: &Data) -> Action {
    match message {
        Message::ControlCentre => Action::ControlCentre(CONTROL_CENTRE.to_string()),
        Message::Default(default) => Action::send(Request::SetSetting {
            key: SettingKey::AssistantDefault,
            value: AssistantDefault::to_value(default.as_ref()),
        }),
        Message::Accounts => Action::ControlCentre(AI_SECTION.to_string()),
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

/// "Status": the assistant on or off, and the one way on from here – the whole row leads to the
/// control centre, which switches it. A row, not a button in a row, so the words fit in every size
/// class (design skill, section 6).
fn status_group(data: &Data) -> Element<'_, Message> {
    let on = super::is_on(data);
    let (dot, label, word) = if on {
        (Status::Ok, tr("The assistant is on"), tr("Change in Torro Workplace"))
    } else {
        (Status::Inactive, tr("The assistant is off"), tr("Turn on in Torro Workplace"))
    };
    let row = setting_row(Some(widgets::status_dot(dot)), label, Some(word.to_string()), arrow());
    let footer =
        tr("Turning the assistant on or off, and what it may use, stay in Torro Workplace: its key is kept there.");
    widgets::group(tr("Status"), vec![pressable(row, Some(Message::ControlCentre))], Some(footer.to_string()))
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
        .assistant_default
        .as_ref()
        .filter(|default| data.engines.iter().any(|engine| engine.id == default.engine));
    let footer = tr("What a new chat starts with. Each chat can pick another.");
    if data.engines.is_empty() {
        return vec![widgets::group(tr("New chats"), vec![no_engine_row()], Some(footer.to_string()))];
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
    let rows = default_models(default, data).into_iter().map(|model| {
        let chosen = default.model.as_ref() == Some(&model.id);
        choice(model.name, chosen, Message::Default(Some(pick(Some(model.id)))))
    });
    groups.push(widgets::group(tr("Model"), std::iter::once(automatic).chain(rows).collect(), None));
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
    let detail = tr("Add Ollama on this computer or OpenRouter under Accounts");
    let row = setting_row(None, tr("No AI account yet"), Some(detail.to_string()), arrow());
    pressable(row, Some(Message::Accounts))
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
/// can call tools ([`Data::default_models`]); the stored model first when it is none of them (set
/// in a chat, or closed weights), so its check shows.
pub fn default_models(default: &AssistantDefault, data: &Data) -> Vec<AiModel> {
    let engine = data.engines.iter().find(|engine| engine.id == default.engine);
    let mut models: Vec<AiModel> = match engine.map(|engine| engine.kind) {
        Some(EngineKind::Agent) => engine.map(|engine| engine.models.clone()).unwrap_or_default(),
        Some(EngineKind::Ai) => {
            data.default_models.iter().filter(|model| model.tools != Some(false)).cloned().collect()
        }
        Some(EngineKind::Test) | None => Vec::new(),
    };
    if let Some(stored) = default.model.as_ref().filter(|id| !models.iter().any(|model| &model.id == *id)) {
        let named = AiModel {
            id: stored.clone(),
            name: stored.clone(),
            tools: None,
            open_weights: None,
            context: None,
            reasoning: None,
            remote: None,
            decision: None,
        };
        models.insert(0, named);
    }
    models
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

    /// The status points at the control centre, both ways round, and sends nothing itself.
    #[tokio::test]
    async fn the_status_only_points_at_the_control_centre() {
        let on = data().await;
        let off = turned_off(&on);
        for data in [&on, &off] {
            let action = update(Message::ControlCentre, data);
            assert!(matches!(action, Act::ControlCentre(ref section) if section == "assistant"), "{action:?}");
            let _ = view(data);
        }
        assert!(super::super::is_on(&on) && !super::super::is_on(&off));
    }

    /// Issue #131: the default of new chats is stored whole, its models are the engine's, and the
    /// effort is offered only where the model takes one.
    #[tokio::test]
    async fn the_default_of_new_chats_is_stored_whole() {
        let mut data = data().await;
        let stored = data.settings.assistant_default.clone().expect("the demo sets one");
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
            data.settings.assistant_default = Some(default);
            let _ = view(&data);
        }
        assert_eq!(effort_label(Some(Effort::Medium)), tr("Medium"));
        assert_eq!(effort_label(None), tr("Automatic"));
    }

    /// Without any engine the group says where an AI account is added (task S.25 brings `ai`).
    #[tokio::test]
    async fn without_an_engine_it_points_at_the_accounts() {
        let data = Data { engines: Vec::new(), ..data().await };
        let action = update(Message::Accounts, &data);
        assert!(matches!(action, Act::ControlCentre(ref section) if section == "accounts"), "{action:?}");
        let _ = view(&data);
    }
}
