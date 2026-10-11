//! The overview (task S.4 in `docs/tasks/steuerzentrale.md`): status at a glance –
//! daemon running, window manager detected (`data.status.backend`), accounts and their
//! state, last sync, number of templates – and a "Needs your attention" card listing
//! accounts whose state is `Connected { problem: Some(..) }` or that need a new sign-in,
//! each with a button that jumps to Accounts (`Action::Select(Section::Accounts)`).

use iced::Element;
use iced::widget::text;

use super::{Action, Data};

#[derive(Debug, Default)]
pub struct State {}

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    let _ = (state, data);
    match message {}
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let _ = (state, data);
    text("overview").into()
}
