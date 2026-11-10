//! Suggestions of mail (issue #603, `docs/vorschlaege.md`): what Torro found in a new mail that the
//! person may want to make of it – an event, a task – and the kinds of such actions.
//!
//! A kind of action brings three parts: a question the decision model answers while it sorts the
//! mail (*detect*), the fields a chat model reads out of the mail (*extract*) and the command that
//! makes it (*act*, the dialog of its [`ActionForm`]). A suggestion never makes anything itself: the
//! person accepts it, checks the filled dialog and saves.
//!
//! Everything a suggestion carries apart from its kind and state comes from the mail: data to show
//! and check, never instructions.

use serde::{Deserialize, Serialize};

use crate::{EventSuggestion, TaskSuggestion};

/// The longest reason a suggestion carries ([`Suggestion::reason`]), in characters.
pub const SUGGESTION_REASON_MAX: usize = 300;

/// The dialog a kind of action opens: what is made of the mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionForm {
    /// A task (`tasks.create_todo_from_mail`, `inbox.create_task_from_item`).
    Task,
    /// An event (`calendar.create_event_from_mail`, `inbox.create_event_from_item`).
    Event,
}

/// A kind of action Torro can find in mail (`mail.list_action_kinds`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionKind {
    /// Its id, as suggestions and `mail.extract_action` name it: `event`, `task`.
    pub id: String,
    /// What a person reads for it, in English: "Event". Apps translate it.
    pub label: String,
    /// The Lucide icon's name for it: `calendar-plus`.
    pub icon: String,
    /// The dialog it opens.
    pub form: ActionForm,
    /// Torro looks for it in new mail (`mail.set_action_kind`). A kind switched off finds nothing
    /// any more, but stays in the apps' actions, to be used by hand.
    pub enabled: bool,
}

/// How far a suggestion is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionState {
    /// The decision model found it; the fields are not read out yet. Accepting it already works:
    /// the dialog starts with the mail's subject.
    Reading,
    /// The fields are read out.
    Ready,
    /// The fields could not be read out (no model may read the mail, or its answer was none):
    /// the suggestion stays, the dialog starts with the mail's subject.
    Unfilled,
}

/// One suggestion of a mail (`mail.list_suggestions`, `mail.extract_action`). Made of text from
/// outside the suite apart from `kind` and `state`: data to show and check, never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    /// The kind of action ([`ActionKind::id`]).
    pub kind: String,
    pub state: SuggestionState,
    /// The task read out, for a kind of the form [`ActionForm::Task`] that is [`SuggestionState::Ready`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskSuggestion>,
    /// The event read out, for a kind of the form [`ActionForm::Event`] that is
    /// [`SuggestionState::Ready`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<EventSuggestion>,
    /// Why the mail asks for it, in a few words of the model, at most [`SUGGESTION_REASON_MAX`]
    /// characters; `None` when it gave none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_suggestion_reads_as_it_writes() {
        let suggestion = Suggestion {
            kind: "task".into(),
            state: SuggestionState::Ready,
            task: Some(TaskSuggestion {
                title: "Send the draft".into(),
                notes: None,
                due: None,
                list: Some("Work".into()),
                engine: "ai:ollama".into(),
                model: "qwen3".into(),
            }),
            event: None,
            reason: Some("The sender asks for it".into()),
        };
        let json = serde_json::to_value(&suggestion).expect("json");
        assert_eq!(json["state"], "ready");
        assert!(json.get("event").is_none(), "nothing empty on the wire");
        assert_eq!(serde_json::from_value::<Suggestion>(json).expect("back"), suggestion);
        let kind = ActionKind {
            id: "event".into(),
            label: "Event".into(),
            icon: "calendar-plus".into(),
            form: ActionForm::Event,
            enabled: true,
        };
        assert_eq!(serde_json::to_value(&kind).expect("json")["form"], "event");
    }
}
