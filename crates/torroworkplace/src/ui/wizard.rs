//! The add-account dialog (task S.6 in `docs/tasks/steuerzentrale.md`).
//!
//! Steps, with the subtitle changing per step, errors bottom left, "Cancel" and the primary
//! button bottom right (like TorroMail's setup wizard):
//!
//! 1. **Provider:** a catalog – Nextcloud (calendars, tasks) enabled; GitHub (issues,
//!    projects), Slack and AI providers listed as "Coming soon", disabled.
//! 2. **Where** (Nextcloud): the server address; Enter or "Continue" sends
//!    `Request::ConnectNextcloud { server }`.
//! 3. **Sign in:** "Nextcloud handles the sign-in. You sign in on its own page; Torro
//!    Workplace never sees your password." The browser opens (the core does that); the login
//!    URL from `data.accounts` (`AccountState::SigningIn`) as a fallback link to copy.
//!    While signing in, [`State::busy`] is true and the shell reloads every second.
//! 4. **Done:** as soon as an account in `data.accounts` is `Connected` for that server:
//!    "Your calendars and task lists are ready." and "Done" (→ `Message::Close`).
//!
//! `Message::Close` is handled by `accounts.rs` (closes the dialog); Escape and a click next
//! to the dialog send it too.

use iced::Element;
use iced::widget::{button, column, text};
use torro_core::i18n::tr;

use super::{Action, Data};

#[derive(Debug, Default)]
pub struct State {
    /// A sign-in was started and has not finished.
    pub signing_in: bool,
}

impl State {
    pub fn busy(&self) -> bool {
        self.signing_in
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Cancel, Done, Escape or a click next to the dialog.
    Close,
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    let _ = (state, data);
    match message {
        Message::Close => Action::None,
    }
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let _ = (state, data);
    column![text(tr("Add account")).size(18), button(text(tr("Cancel"))).on_press(Message::Close)]
        .spacing(16)
        .padding(24)
        .into()
}
