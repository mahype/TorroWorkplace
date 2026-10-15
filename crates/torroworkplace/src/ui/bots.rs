//! Assistant › Bots (task B.3 in `docs/tasks/assistent.md`, concept `docs/assistent.md`, "Bots"):
//! what each bot may use. The group on the Assistant page lists the bots ([`bots_group`]); a row
//! opens the bot's page below Assistant, also directly with `torroworkplace ui --section
//! bot:<id>` – the way the assistant app's "Access…" gets here.
//!
//! The bot's page is the same rights view as an agent's (`agents::rights_groups`) for the bot's own
//! client (kind `api::BOT_CLIENT`), edited and saved like an agent's (`agents::SAVE_DELAY` after the
//! last change, `clients.update_client`). A new bot starts without rights; only a person grants
//! it some, here (point 2 of "Was nie nach außen geht": `clients.*` is the app key's alone). The
//! areas `assistant` and `bots` have no row ([`agents::shows_area`]): the core refuses a bot both,
//! no chains. Who the bot is – name, personality, instructions, skills, engine – is edited in the
//! assistant app; "Open chat" starts its chat window there (`assistant.open_window { bot }`).
//!
//! The page reads and writes nothing itself: the bots come with [`Data`] (`bots.list_bots`), their
//! clients with `clients.list_clients`.

use chrono::Utc;
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, responsive, row};
use iced::{Alignment, Element, Fill, Length, Subscription};
use torro_core::i18n::tr;
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, size, space};
use torro_gui::{style, typo, widgets};
use torro_ipc::api::BOT_CLIENT;
use torro_ipc::assistant::ChatWindow;
use torro_ipc::bots::BotSummary;
use torro_ipc::{ClientInfo, Request, Response};

use super::{Action, Data, Header, chevron, pressable, setting_row};
use super::{agents, persona};

/// Page state.
#[derive(Debug, Default)]
pub struct State {
    /// The bot whose page is open (its id); `None` on the Assistant page.
    pub open: Option<String>,
    /// The rights of the open bot's client, edited and saved like an agent's: an agents page state
    /// with that client selected.
    editor: agents::State,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A bot's row, `--section bot:<id>`: the bot's page.
    Open(String),
    /// The back button in the header: the Assistant page, saving what waits.
    Back,
    /// A control of the rights view, and the clock that saves the changed rights.
    Rights(agents::Message),
    /// "Open chat": the bot's chat window in the assistant app.
    OpenChat(String),
}

/// The bot `id` as the core lists it.
pub fn bot<'a>(data: &'a Data, id: &str) -> Option<&'a BotSummary> {
    data.bots.iter().find(|bot| bot.id == id)
}

/// The client of `bot`, when the core still has it.
pub fn client_of<'a>(data: &'a Data, bot: &BotSummary) -> Option<&'a ClientInfo> {
    data.clients.iter().find(|client| client.id == bot.client && client.kind == BOT_CLIENT)
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Open(id) => {
            state.editor.selected = bot(data, &id).and_then(|bot| client_of(data, bot)).map(|client| client.id.clone());
            state.open = Some(id);
            Action::None
        }
        Message::Back => {
            state.open = None;
            // Leaving the page is a good moment to save; the clock would do it a moment later.
            agents::update(&mut state.editor, agents::Message::Back, data)
        }
        Message::Rights(message) => rights(state, message, data),
        Message::OpenChat(id) => Action::Send(Request::OpenChatWindow { window: ChatWindow::Bot(id) }),
    }
}

/// A control of the rights view, on the open bot's client through the agents page's draft and
/// autosave. Other agents page messages are not the rights view's and change nothing here.
fn rights(state: &mut State, message: agents::Message, data: &Data) -> Action {
    use agents::Message as M;
    if let M::Clock(_) | M::MailToCloud(_) = message {
        // The clock saves the draft; the data rule (task C.17) is a setting, sent at once.
        return agents::update(&mut state.editor, message, data);
    }
    let control = matches!(
        message,
        M::Level(..)
            | M::Admin(..)
            | M::Confirm(_)
            | M::Outbound(_)
            | M::All(..)
            | M::Tick(..)
            | M::AddRepo
            | M::RepoInput(_)
    );
    let client = state.open.as_deref().and_then(|id| bot(data, id)).and_then(|bot| client_of(data, bot));
    match client {
        Some(client) if control => {
            state.editor.selected = Some(client.id.clone());
            agents::update(&mut state.editor, message, data)
        }
        _ => Action::None,
    }
}

/// The answer to a request the shell sent: `ok` of a saved change, an error that drops it. A key
/// never reaches the rights editor.
pub fn answered(state: &mut State, result: &Result<Response, String>) {
    if !matches!(result, Ok(Response::ClientKey { .. })) {
        let _ = agents::answered(&mut state.editor, result);
    }
}

/// The shell loaded [`Data`] anew: saved rights are in it now.
pub fn loaded(state: &mut State) {
    agents::loaded(&mut state.editor);
}

/// The clock that saves changed rights; none while nothing waits.
pub fn subscription(state: &State) -> Subscription<Message> {
    agents::subscription(&state.editor).map(Message::Rights)
}

/// The header of a bot's page: its name after the section's with the back button, and "Open chat"
/// (only its icon in narrow windows). `None` on the Assistant page.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Option<Header<'a, Message>> {
    let id = state.open.as_ref()?;
    let Some(found) = bot(data, id) else {
        return Some(Header { sub_page: Some((tr("Bot").to_string(), Message::Back)), actions: Vec::new() });
    };
    let open = Some(Message::OpenChat(found.id.clone()));
    let button = match class {
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::ArrowUpRight), tr("Open chat"), style::button::secondary, open)
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::ArrowUpRight, tr("Open chat"), style::button::secondary, open)
        }
    };
    Some(Header { sub_page: Some((found.name.clone(), Message::Back)), actions: vec![button] })
}

// ----- Views --------------------------------------------------------------------------------

/// The bot's page; `None` on the Assistant page.
pub fn view<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let id = state.open.as_ref()?;
    Some(
        responsive(move |size| page(state, data, id, agents::control_column(size.width))).height(Length::Shrink).into(),
    )
}

/// The page in the width it has: who the bot is, then what it may use.
fn page<'a>(state: &'a State, data: &'a Data, id: &'a str, column: Option<f32>) -> Element<'a, Message> {
    let Some(found) = bot(data, id) else {
        let row = setting_row(
            None,
            tr("This bot no longer exists"),
            Some(tr("It was deleted, and its access with it.").to_string()),
            Space::new(),
        );
        return widgets::group(tr("Bot"), vec![row], None);
    };
    let client = client_of(data, found);
    let mut page = column![heading(found, client)].spacing(layout::section_gap());
    match client {
        Some(client) => {
            let rights = agents::shown_rights(&state.editor, client);
            let groups = agents::rights_groups(
                agents::repo_input(&state.editor),
                rights,
                data,
                column,
                BOT_CLIENT,
                tr("What the bot may use"),
            );
            page = page.extend(groups.into_iter().map(|group| group.map(Message::Rights)));
        }
        None => {
            let row = setting_row(
                None,
                tr("No access set up"),
                Some(tr("Save the bot once in the assistant. It then starts with access to nothing.").to_string()),
                Space::new(),
            );
            page = page.push(widgets::group(tr("What the bot may use"), vec![row], None));
        }
    }
    page.into()
}

/// The bot's picture, name and description, and when it last worked; under it where the rest of
/// the bot is edited.
fn heading<'a>(bot: &'a BotSummary, client: Option<&ClientInfo>) -> Element<'a, Message> {
    let mut words =
        column![typo::text(typo::headline(), bot.name.as_str()).wrapping(Wrapping::WordOrGlyph)].spacing(space::xs());
    if !bot.description.is_empty() {
        words = words.push(typo::text(typo::body(), bot.description.as_str()).style(theme::text_2));
    }
    let used = client
        .map_or_else(|| tr("Never used").to_string(), |client| agents::last_used_text(client.last_used, Utc::now()));
    words = words.push(typo::text(typo::meta(), used).style(theme::text_3));
    let edited = tr("Its profile, instructions and skills are edited in the assistant.");
    words = words.push(typo::text(typo::meta(), edited).style(theme::text_3).wrapping(Wrapping::WordOrGlyph));
    let face = persona::avatar(&bot.name, &bot.avatar, size::control_lg());
    row![face, container(words).width(Fill)].spacing(space::l()).align_y(Alignment::Start).into()
}

/// "Bots" on the Assistant page: a row per bot with its picture, name and what it may use, which
/// opens its page; without bots, where they are made.
pub fn bots_group<'a>(data: &'a Data) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = data
        .bots
        .iter()
        .map(|bot| {
            let access = match client_of(data, bot) {
                Some(client) => agents::rights_summary(&client.rights, BOT_CLIENT),
                None => tr("No access set up").to_string(),
            };
            let detail = match bot.description.as_str() {
                "" => access,
                description => format!("{description} · {access}"),
            };
            let face = persona::avatar(&bot.name, &bot.avatar, size::control_sm());
            let row = setting_row(Some(face), bot.name.as_str(), Some(detail), chevron());
            pressable(row, Some(Message::Open(bot.id.clone())))
        })
        .collect();
    if rows.is_empty() {
        let hint = tr("Make one in the assistant app with New bot.");
        rows.push(setting_row(None, tr("No bots yet"), Some(hint.to_string()), Space::new()));
    }
    let footer = tr(
        "Bots are assistants for one field, each with rights of its own. A new bot may use nothing until you allow it here.",
    );
    widgets::group(tr("Bots"), rows, Some(footer.to_string()))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use iced::time::Instant;
    use torro_ipc::api::{Area, Level, Rights};

    use super::*;

    fn summary(id: &str, client: &str) -> BotSummary {
        BotSummary {
            id: id.into(),
            name: "Travel planner".into(),
            description: "Plans trips by train".into(),
            avatar: "letter".into(),
            engine: "test".into(),
            model: None,
            client: client.into(),
        }
    }

    fn bot_client(id: &str, rights: Rights) -> ClientInfo {
        ClientInfo {
            id: id.into(),
            name: "Travel planner".into(),
            kind: BOT_CLIENT.into(),
            created: Utc::now(),
            last_used: None,
            rights,
            agent: None,
        }
    }

    fn data() -> Data {
        Data {
            bots: vec![summary("b0000001", "c0000001"), summary("b0000002", "gone")],
            clients: vec![bot_client("c0000001", Rights::none())],
            ..Data::default()
        }
    }

    /// Runs the clock past the delay and returns the request it sends.
    fn saved(state: &mut State, data: &Data) -> Action {
        let later = Instant::now() + agents::SAVE_DELAY + agents::SAVE_DELAY;
        update(state, Message::Rights(agents::Message::Clock(later)), data)
    }

    #[test]
    fn a_bots_rights_are_saved_on_its_client_with_update_client() {
        let data = data();
        let mut state = State::default();
        let _ = update(&mut state, Message::Open("b0000001".into()), &data);
        assert_eq!(state.open.as_deref(), Some("b0000001"));
        let _ = update(&mut state, Message::Rights(agents::Message::Level(Area::Calendar, Level::Read)), &data);
        // The areas no bot may use change nothing.
        let _ = update(&mut state, Message::Rights(agents::Message::Level(Area::Bots, Level::Full)), &data);
        let _ = update(&mut state, Message::Rights(agents::Message::Level(Area::Assistant, Level::Read)), &data);
        let Action::Send(Request::UpdateClient { id, rights }) = saved(&mut state, &data) else {
            panic!("expected clients.update_client");
        };
        assert_eq!(id, "c0000001", "the bot's own client");
        assert_eq!(rights.area(Area::Calendar).level, Level::Read);
        assert_eq!(
            (rights.area(Area::Bots).level, rights.area(Area::Assistant).level),
            (Level::NoAccess, Level::NoAccess)
        );
        assert!(!agents::shows_area(Area::Bots, BOT_CLIENT) && !agents::shows_area(Area::Assistant, BOT_CLIENT));
    }

    #[test]
    fn a_bot_without_its_client_or_gone_changes_nothing() {
        let data = data();
        let mut state = State::default();
        let _ = update(&mut state, Message::Open("b0000002".into()), &data);
        let _ = update(&mut state, Message::Rights(agents::Message::Level(Area::Calendar, Level::Read)), &data);
        assert!(matches!(saved(&mut state, &data), Action::None), "no client: nothing to save");
        drop(view(&state, &data));
        let _ = update(&mut state, Message::Open("ffffffff".into()), &data);
        drop(view(&state, &data));
        assert!(header(&state, &data, SizeClass::Wide).is_some_and(|header| header.actions.is_empty()));
        let _ = update(&mut state, Message::Back, &data);
        assert!(
            state.open.is_none() && view(&state, &data).is_none() && header(&state, &data, SizeClass::Wide).is_none()
        );
    }

    #[test]
    fn open_chat_asks_the_core_for_the_bots_window() {
        let data = data();
        let mut state = State::default();
        let _ = update(&mut state, Message::Open("b0000001".into()), &data);
        for class in [SizeClass::Wide, SizeClass::Narrow] {
            assert_eq!(header(&state, &data, class).map(|header| header.actions.len()), Some(1), "{class:?}");
        }
        let action = update(&mut state, Message::OpenChat("b0000001".into()), &data);
        assert!(matches!(
            action,
            Action::Send(Request::OpenChatWindow { window: ChatWindow::Bot(ref id) }) if id == "b0000001"
        ));
    }

    #[test]
    fn the_group_and_the_pages_build() {
        let data = data();
        drop(bots_group(&data));
        drop(bots_group(&Data::default()));
        let mut state = State::default();
        let _ = update(&mut state, Message::Open("b0000001".into()), &data);
        for column in [None, Some(agents::segment_slot())] {
            drop(page(&state, &data, "b0000001", column));
            drop(page(&state, &data, "b0000002", column));
        }
    }
}
