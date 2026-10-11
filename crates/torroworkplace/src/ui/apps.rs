//! The apps of the suite (task S.8): one row per app from `torro_core::AppId::ALL` except
//! the core itself – name, window class (for window rules), an "Open" button
//! (`Action::Send(Request::OpenApp { app: binary })`).

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
    text("apps").into()
}
