//! General settings (task S.9): language (System / English / Deutsch, footer: applies to
//! apps started afterwards), start at login (toggle). Every change sends
//! `Request::UpdateSettings` with the whole changed `Settings` (autosave, no Save button).

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
    text("general").into()
}
