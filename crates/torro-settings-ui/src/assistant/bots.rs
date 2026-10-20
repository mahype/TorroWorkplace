//! Assistant › Bots (task S.17; the group of the control centre's Assistant page, task B.3,
//! concept `docs/assistent.md`, "Bots"): the bots, and the two ways to each one.
//!
//! A bot is an assistant for one field, with rights of its own. Both halves of it live where they
//! belong (`docs/settings.md`, "Die Regel"):
//!
//! - **"Edit"** opens the bot's editor in the assistant ([`super::AppAction::EditBot`]), where who
//!   the bot is – name, picture, personality, instructions, skills, engine – is written.
//! - **"Access"** opens the section Access at this bot (task S.34, [`super::access`]), where what
//!   the bot may use is chosen: a bot's rights are `clients.*`, which only Access sends, and a new
//!   bot may use nothing until a person allows it there. [`super::update`] takes the message, so
//!   Access opens at the bot; [`update`] alone only opens the section.
//!
//! The section reads and writes nothing itself: the bots come with [`Data`] (`bots.list_bots`).

use iced::widget::{Space, row};
use iced::{Element, Length};

use torro_core::i18n::tr;
use torro_gui::icon::Icon;
use torro_gui::layout::SizeClass;
use torro_gui::theme::{size, space};
use torro_gui::{style, widgets};

use super::{Action, AppAction, access, avatar};
use crate::Data;
use crate::parts::setting_row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// "Edit" at the bot with this id: its editor in the assistant.
    Edit(String),
    /// "Access" at the bot with this id: the section Access at this bot.
    Access(String),
}

pub fn update(message: Message, _data: &Data) -> Action {
    match message {
        Message::Edit(id) => Action::App(AppAction::EditBot(id)),
        // Which bot is the sections' state ([`super::show_access`]); here only the way there.
        Message::Access(_) => Action::Open(access::ID),
    }
}

// ----- Views --------------------------------------------------------------------------------

/// A row per bot with its picture, name and description, and the two ways to it; without bots,
/// where they are made. In a narrow window the two ways are icons with a tooltip, so the name
/// keeps its room (design skill, section 6).
pub fn view(data: &Data, class: SizeClass) -> Element<'_, Message> {
    if !super::is_on(data) {
        return super::off_group(tr("Bots"));
    }
    let words = matches!(class, SizeClass::Wide | SizeClass::Medium);
    let mut rows: Vec<Element<'_, Message>> = data.bots.iter().map(|bot| bot_row(bot, words)).collect();
    if rows.is_empty() {
        let hint = tr("Make one with New bot in the sidebar.");
        rows.push(setting_row(None, tr("No bots yet"), Some(hint.to_string()), Space::new()));
    }
    let footer = tr(
        "Bots are assistants for one field, each with rights of its own. A new bot may use nothing until you allow it under Access.",
    );
    widgets::group(tr("Bots"), rows, Some(footer.to_string()))
}

/// One bot: its picture, name and description, "Edit" in the assistant and "Access" in these
/// settings, with their words or as icons (`words`).
fn bot_row(bot: &torro_ipc::bots::BotSummary, words: bool) -> Element<'_, Message> {
    let (edit, access) = (Message::Edit(bot.id.clone()), Message::Access(bot.id.clone()));
    let buttons = match words {
        true => row![
            widgets::text_button(None, tr("Edit"), style::button::secondary, Some(edit)),
            widgets::text_button(None, tr("Access"), style::button::ghost, Some(access)),
        ],
        false => row![
            widgets::icon_button(Icon::SquarePen, tr("Edit"), Some(edit)),
            widgets::icon_button(Icon::UserCheck, tr("Access"), Some(access)),
        ],
    };
    let buttons = buttons.spacing(space::s()).width(Length::Shrink);
    let description = Some(bot.description.clone()).filter(|text| !text.is_empty());
    let face = avatar(&bot.name, &bot.avatar, size::control_sm());
    setting_row(Some(face), bot.name.as_str(), description, buttons)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{data, turned_off};
    use super::*;
    use crate::Action as Act;

    /// "Edit" is the app's work, "Access" the section Access in this window (task S.34).
    #[tokio::test]
    async fn each_bot_leads_to_its_editor_and_to_its_access() {
        let data = data().await;
        let bot = data.bots.first().expect("the demo has a bot");
        let edit = update(Message::Edit(bot.id.clone()), &data);
        assert_eq!(edit, Act::App(AppAction::EditBot(bot.id.clone())));
        assert_eq!(update(Message::Access(bot.id.clone()), &data), Act::Open("access"));
    }

    /// With bots, without bots, and while the assistant is off; with words and as icons.
    #[tokio::test]
    async fn the_list_builds_in_every_state() {
        let data = data().await;
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            let _ = view(&data, class);
            let _ = view(&Data { bots: Vec::new(), ..data.clone() }, class);
            let _ = view(&turned_off(&data), class);
        }
    }
}
