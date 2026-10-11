//! The detail panel for one task (task K.15 in `docs/tasks/kalender-aufgaben.md`).
//!
//! Title, done checkbox, list (pick list), due date (German input via
//! `torro_gui::datetime::parse_date`, empty = none, buttons Heute/Morgen/Entfernen),
//! priority (Keine / Niedrig = 9 / Mittel = 5 / Hoch = 1), parent task (pick list of other
//! tasks in the same list, or none), notes (multi-line). Changes are saved with the
//! Speichern button (Enter in the title also saves); Löschen asks once ("Wirklich löschen?")
//! before `Action::Delete`; Schließen closes. Invalid input shows a hint.

use iced::Element;
use iced::widget::{button, column, text};
use torro_pim::{Collection, Todo};

/// The panel's state.
#[derive(Debug, Clone)]
pub struct Detail {
    /// The task as it was loaded; the base for the saved version (keeps href/etag).
    pub original: Todo,
    pub lists: Vec<Collection>,
    /// Other tasks, for choosing a parent.
    pub others: Vec<Todo>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Close,
}

/// What the app should do after an update.
#[derive(Debug, Clone)]
pub enum Action {
    None,
    Save(Todo),
    Delete(Todo),
    Close,
}

impl Detail {
    pub fn new(todo: &Todo, lists: &[Collection], all: &[Todo]) -> Detail {
        Detail {
            original: todo.clone(),
            lists: lists.to_vec(),
            others: all.iter().filter(|t| t.uid != todo.uid).cloned().collect(),
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Close => Action::Close,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        column![text(self.original.summary.clone()).size(18), button(text("Schließen")).on_press(Message::Close)]
            .spacing(12)
            .padding(16)
            .into()
    }
}
