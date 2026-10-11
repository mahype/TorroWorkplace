//! Workspace templates (task S.7): list (name, windows, last updated), rename inline, delete
//! with confirmation (`torro_gui::widgets::modal`), load into the active workspace (Add).
//! Requests: `RenameTemplate`, `DeleteTemplate`, `LoadTemplate { mode: Add }`. Empty: one
//! sentence explaining how to save a workspace from the tray menu.

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
    text("workspaces").into()
}
