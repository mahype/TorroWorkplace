//! Accounts (task S.5 in `docs/tasks/steuerzentrale.md`): the accounts as cards (provider,
//! label, status as symbol + word, last sync, chevron) with "Add account" in the header; a
//! detail page per account (status with "Sync now" / "Sign in again", the collections with a
//! switch each for showing them in the apps – read-only ones marked –, server and user,
//! "Remove account…" with confirmation via `torro_gui::widgets::modal`). Empty: a sentence
//! and the "Add account" button. The add flow is `wizard.rs` (task S.6), shown as a dialog.
//!
//! Requests: `Sync`, `RemoveAccount { id }`, `UpdateSettings` (hidden collections: the whole
//! `Settings` with the changed `hidden_collections`).

use iced::Element;
use iced::widget::{button, column, text};
use torro_core::i18n::tr;
use torro_gui::widgets::modal;

use super::{Action, Data, wizard};

#[derive(Debug, Default)]
pub struct State {
    /// The add-account dialog while it is open.
    pub wizard: Option<wizard::State>,
}

impl State {
    /// A sign-in is running; the shell reloads more often.
    pub fn busy(&self) -> bool {
        self.wizard.as_ref().is_some_and(wizard::State::busy)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    AddAccount,
    Wizard(wizard::Message),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::AddAccount => {
            state.wizard = Some(wizard::State::default());
            Action::None
        }
        Message::Wizard(wizard::Message::Close) => {
            state.wizard = None;
            Action::None
        }
        Message::Wizard(message) => match state.wizard.as_mut() {
            Some(wizard) => wizard::update(wizard, message, data),
            None => Action::None,
        },
    }
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let list = column![
        text(format!("{} accounts", data.accounts.len())),
        button(text(tr("Add account"))).style(button::primary).on_press(Message::AddAccount),
    ]
    .spacing(16);
    match &state.wizard {
        Some(wizard) => {
            modal(list, wizard::view(wizard, data).map(Message::Wizard), 520.0, Message::Wizard(wizard::Message::Close))
        }
        None => list.into(),
    }
}
