//! Assistant › Access (task S.34, concept `docs/settings.md` › "Assistent und Bots"): what the
//! assistant and each of its bots may use, in the assistant's own window.
//!
//! ```text
//! Status            the assistant on or off
//! Start from        while it is off: the starting points of its rights ([`Preset`])
//! What the assistant may use   the rights editor ([`crate::rights`], tools named as MCP tools)
//! Bots              a row per bot; it opens the rights editor of the bot's own client
//! Turn off assistant…          while it is on, after a question; last, as what destroys
//! ```
//!
//! While the assistant has no client the page is its first start: "Start from" and the rights it
//! is offered, which "Turn on assistant" in the header sends along with `clients.create_client`
//! (kind `assistant`), so the client never holds other rights, not even for a moment. With a client
//! every change goes through the editor's draft and is saved [`crate::rights::SAVE_DELAY`] later
//! (`clients.update_client`). "Turn off assistant…" asks first and sends `clients.remove_client`.
//! A bot has no button to remove it: whether it exists is the bot editor's. The areas `assistant`
//! and `bots` have no row here ([`torro_ipc::api::shows_area`]).
//!
//! # The boundary in an app process
//!
//! Rights are `clients.*`, the app key's alone (`docs/api.md`, "Was nie nach außen geht", point 2).
//! The assistant window holds the app key like every app of the suite, and the core lets that key
//! do anything; the boundary is a rule of the code, which tests watch over (`super::tests`):
//!
//! - Of all sections only this one sends `clients.*` beyond reading the list: `clients.update_client`
//!   (the editor), and for the kind `assistant` only `clients.create_client` and
//!   `clients.remove_client`. Never `clients.new_key` – the core runs the assistant's and the bots'
//!   tools itself with their clients' rights, so nobody needs their keys.
//! - Everything goes out as [`crate::Action::Send`], whose answers the host drops unread
//!   (`crate::send`): a key that a core would still hand back with `clients.create_client` never
//!   reaches this section. Since task S.32 the core answers with `Response::Client`, without one.
//!
//! The answers the section needs – saved or failed, the fresh list of clients – come from the
//! host's own messages, which the window hands to [`super::received`] before the host takes them
//! ([`sent`], [`loaded`]).

use iced::widget::{Space, column, row, svg};
use iced::{Element, Theme};

use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::style;
use torro_gui::theme::{palette, size};
use torro_gui::widgets::{self, Status};
use torro_ipc::api::{ASSISTANT_CLIENT, BOT_CLIENT, Rights, RightsTemplate};
use torro_ipc::bots::BotSummary;
use torro_ipc::{ClientInfo, Request, Response};

use super::{Action, avatar};
use crate::parts::{chevron, confirm, pressable, setting_row};
use crate::rights::{self, Known, Naming};
use crate::{Data, Page};

/// The section's id, as the list, `--settings access` and [`crate::Action::Open`] name it.
pub const ID: &str = "access";

/// The name of the assistant's client, as the core lists it and the audit log shows it (decision 5
/// of `docs/assistent.md`: one client "Assistant").
pub const CLIENT_NAME: &str = "Assistant";

/// A starting point of the assistant's first start (task C.6), moved here from the control centre
/// with task S.34.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// [`Rights::assistant`]: read calendars, tasks and the Notification Hub.
    Suggested,
    Template(RightsTemplate),
}

impl Preset {
    /// Every starting point, in the order "Start from" lists them.
    pub const ALL: [Preset; 4] = [
        Preset::Suggested,
        Preset::Template(RightsTemplate::ReadOnly),
        Preset::Template(RightsTemplate::ReadAndWrite),
        Preset::Template(RightsTemplate::FullAccess),
    ];

    /// The rights it stands for. No template opens the area `assistant` (`Area::in_templates`).
    pub fn rights(self) -> Rights {
        match self {
            Preset::Suggested => Rights::assistant(),
            Preset::Template(template) => template.rights(),
        }
    }

    /// Its name, translated.
    pub fn label(self) -> &'static str {
        match self {
            Preset::Suggested => tr("Suggested"),
            Preset::Template(template) => rights::template_label(template),
        }
    }

    /// What it allows, in a few words, translated.
    pub fn detail(self) -> &'static str {
        match self {
            Preset::Suggested => tr("Reads calendars, tasks and the Notification Hub"),
            Preset::Template(RightsTemplate::ReadOnly) => tr("Reads everything, changes nothing"),
            Preset::Template(RightsTemplate::ReadAndWrite) => tr("Changes everything; deleting needs approval"),
            Preset::Template(RightsTemplate::FullAccess) => tr("Changes and deletes everything without asking"),
        }
    }
}

/// The section's state: whose rights are open, the first start's choice and the editor's draft.
#[derive(Debug)]
pub struct State {
    /// The bot (its [`BotSummary::id`]) whose rights are open on a page below the section; `None`
    /// shows the section's own page, the assistant's.
    bot: Option<String>,
    /// What the first start offers until "Turn on assistant": [`Rights::assistant`], then what the
    /// person changed.
    choice: Rights,
    /// "Turn on assistant" went out and the list of clients does not show the client yet.
    creating: bool,
    /// "Turn off the assistant?" is open.
    asking: bool,
    /// The rights editor: its draft on the way to the core, the typed repository, the open areas.
    rights: rights::State,
}

impl Default for State {
    fn default() -> Self {
        State {
            bot: None,
            choice: Rights::assistant(),
            creating: false,
            asking: false,
            rights: rights::State::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A row of "Start from" on the first start.
    Preset(Preset),
    /// "Turn on assistant" in the header of the first start.
    TurnOn,
    /// "Turn off assistant…", then its confirmation.
    AskTurnOff,
    TurnOff,
    /// "Cancel", Escape or a click next to the confirmation.
    CloseDialog,
    /// The row of the bot with this id: its rights on a page below the section.
    OpenBot(String),
    /// Back from a bot's page to the section's own.
    Back,
    /// A control of the rights editor, its clock included ([`subscription`]).
    Rights(rights::Message),
}

/// What a control of the section does:
///
/// - [`Message::Preset`] and every control of the editor on the first start change the choice
///   "Turn on assistant" sends ([`rights::change`]); nothing is sent.
/// - [`Message::TurnOn`] sends `clients.create_client` with the kind `assistant`, [`CLIENT_NAME`] and
///   the choice as its rights (the template, the narrowest, only counts for an older core that
///   ignores them); once only, until the list shows the client or the send failed.
/// - [`Message::TurnOff`] (after [`Message::AskTurnOff`]) sends `clients.remove_client` for the
///   assistant's client; the next first start begins from the suggestion again.
/// - With a client, a control of the editor changes the draft of the client on screen – the
///   assistant's, or the open bot's – and the clock sends `clients.update_client` once it is due
///   ([`rights::update`]). [`Message::OpenBot`] and [`Message::Back`] save a waiting draft at once.
///
/// Nothing else is ever sent, and only as [`crate::Action::Send`].
pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Preset(preset) => {
            if super::client(data).is_none() {
                state.choice = preset.rights();
            }
            Action::None
        }
        Message::TurnOn => {
            if super::client(data).is_some() || state.creating {
                return Action::None;
            }
            state.creating = true;
            Action::send(Request::CreateClient {
                name: CLIENT_NAME.into(),
                kind: ASSISTANT_CLIENT.into(),
                template: RightsTemplate::ReadOnly,
                agent: None,
                rights: Some(state.choice.clone()),
            })
        }
        Message::AskTurnOff => {
            state.asking = super::client(data).is_some();
            Action::None
        }
        Message::TurnOff => {
            state.asking = false;
            let Some(id) = super::client(data).map(|client| client.id.clone()) else { return Action::None };
            // Its rights need not be saved any more, and turned on again it starts from the
            // suggestion, as its first start does.
            rights::forget(&mut state.rights, &id);
            state.choice = Rights::assistant();
            Action::send(Request::RemoveClient { id })
        }
        Message::CloseDialog => {
            state.asking = false;
            Action::None
        }
        Message::OpenBot(id) => {
            let action = rights::save_now(&mut state.rights);
            show(state, Some(id));
            action
        }
        Message::Back => {
            let action = rights::save_now(&mut state.rights);
            show(state, None);
            action
        }
        Message::Rights(message) => edit(state, message, data),
    }
}

/// Opens the section at the assistant (`None`) or at the bot with this id – what "Access" in the
/// Bots section, the bot editor's "What the bot may use", "Change access" under a refused tool and
/// `--settings access [--bot <id>]` ask for. A bot the loaded data does not list shows the
/// assistant's page instead. The editor closes its open areas; a waiting draft is saved as usual.
pub fn show(state: &mut State, bot: Option<String>) {
    state.bot = bot;
    state.asking = false;
    rights::leave(&mut state.rights);
}

/// The bot whose page is open: [`State`]'s bot, as long as the data lists it.
pub fn open_bot<'a>(state: &State, data: &'a Data) -> Option<&'a BotSummary> {
    let id = state.bot.as_deref()?;
    data.bots.iter().find(|bot| bot.id == id)
}

/// The client whose rights the editor shows: the open bot's own client, else the assistant's;
/// `None` on the first start and for a bot whose client the core does not list.
fn shown_client<'a>(state: &State, data: &'a Data) -> Option<&'a ClientInfo> {
    match open_bot(state, data) {
        Some(bot) => data.clients.iter().find(|client| client.id == bot.client && client.kind == BOT_CLIENT),
        None => super::client(data),
    }
}

/// Hands a control of the editor on: the clock always (a draft still goes out after the page moved
/// on), the rest to the client on screen, or on the first start to the choice.
fn edit(state: &mut State, message: rights::Message, data: &Data) -> Action {
    let known = Known::of(data);
    if matches!(message, rights::Message::Clock(_)) {
        return rights::update(&mut state.rights, message, None, &known);
    }
    if let Some(client) = shown_client(state, data) {
        return rights::update(&mut state.rights, message, Some(client), &known);
    }
    if open_bot(state, data).is_none() {
        rights::change(&mut state.rights, &mut state.choice, ASSISTANT_CLIENT, message, &known);
    }
    Action::None
}

/// Sends a waiting draft now: what the window asks before the settings close, so a change made a
/// moment ago is not lost with them. [`crate::Action::None`] while nothing waits.
pub fn save_now(state: &mut State) -> Action {
    rights::save_now(&mut state.rights)
}

/// A send of the sections came back, as the host reports it (`host::Message::Sent`) before it
/// takes it: `changed` whether it carried requests, `result` the fresh data or why it failed.
///
/// A change that went through settles the editor's draft – it is saved and in the fresh data –,
/// one that failed drops it, so the editor shows what the core holds (the host shows the error),
/// and "Turn on assistant" can be pressed again. A plain reload (`changed` is `false`) is only
/// fresh data ([`loaded`]).
pub fn sent(state: &mut State, changed: bool, result: &Result<Data, String>) {
    match result {
        Ok(data) => {
            if changed {
                rights::answered(&mut state.rights, &Ok(Response::Ok));
            }
            loaded(state, data);
        }
        Err(problem) => {
            rights::answered(&mut state.rights, &Err(problem.clone()));
            state.creating = false;
        }
    }
}

/// The sections' data came anew (`host::Message::Loaded`, or the reload after a send): a saved
/// draft is in it now, and "Turn on assistant" is done once it lists the assistant's client.
pub fn loaded(state: &mut State, data: &Data) {
    rights::loaded(&mut state.rights);
    if super::client(data).is_some() {
        state.creating = false;
    }
}

/// The editor's clock that saves a waiting draft; none while nothing waits.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    rights::subscription(&state.rights).map(Message::Rights)
}

/// What Escape means here: close "Turn off the assistant?". `None` leaves it to the host.
pub fn escape(state: &State) -> Option<Message> {
    state.asking.then_some(Message::CloseDialog)
}

/// "Turn off the assistant?" while it is open and the assistant has a client to turn off.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    super::client(data).filter(|_| state.asking)?;
    Some(confirm(
        tr("Turn off the assistant?"),
        tr("It loses access to everything at once. Your conversations stay. Turn it on again to choose what it may use.")
            .to_string(),
        tr("Turn off"),
        Message::TurnOff,
        Message::CloseDialog,
    ))
}

// ----- Views --------------------------------------------------------------------------------

/// The page: a bot's rights below the section while one is open, else the assistant's – on, or its
/// first start with "Turn on assistant" in the header, the page's one primary button.
pub fn page<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Page<'a, Message> {
    if let Some(bot) = open_bot(state, data) {
        return Page::new(bot_page(state, data, bot)).sub_page(bot.name.clone(), Message::Back);
    }
    match super::client(data) {
        Some(client) => Page::new(assistant_page(state, data, client)),
        None => Page::new(first_start(state, data)).actions(vec![turn_on_button(state, class)]),
    }
}

/// The assistant on: its status, the editor, the bots, and last "Turn off assistant…" – what
/// undoes the page stands at its end, after everything it would take away.
fn assistant_page<'a>(state: &'a State, data: &'a Data, client: &'a ClientInfo) -> Element<'a, Message> {
    let rights = rights::shown(&state.rights, client);
    let editor = rights::view(
        &state.rights,
        rights,
        ASSISTANT_CLIENT,
        Known::of(data),
        Naming::Tools,
        tr("What the assistant may use"),
    );
    let off = widgets::text_button(None, tr("Turn off assistant…"), style::button::danger, Some(Message::AskTurnOff));
    column![status_group(true), editor.map(Message::Rights), bots_group(state, data), on_axis(off)]
        .spacing(layout::section_gap())
        .into()
}

/// The assistant's first start: that it is off, "Start from", the rights it is offered, the bots.
fn first_start<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let editor = rights::view(
        &state.rights,
        &state.choice,
        ASSISTANT_CLIENT,
        Known::of(data),
        Naming::Tools,
        tr("What the assistant may use"),
    );
    column![status_group(false), preset_group(&state.choice), editor.map(Message::Rights), bots_group(state, data)]
        .spacing(layout::section_gap())
        .into()
}

/// A bot's page: the editor of its own client, or, where the core lists none, what to do.
fn bot_page<'a>(state: &'a State, data: &'a Data, bot: &'a BotSummary) -> Element<'a, Message> {
    match shown_client(state, data) {
        Some(client) => {
            let rights = rights::shown(&state.rights, client);
            rights::view(&state.rights, rights, BOT_CLIENT, Known::of(data), Naming::Tools, tr("What the bot may use"))
                .map(Message::Rights)
        }
        None => {
            let detail = tr("Save the bot once more in its editor; until then it may use nothing.");
            let row = setting_row(None, bot.name.as_str(), Some(detail.to_string()), Space::new());
            widgets::group(tr("What the bot may use"), vec![row], None)
        }
    }
}

/// "Status": the assistant on or off, and the sentence that says how it works.
fn status_group<'a>(on: bool) -> Element<'a, Message> {
    let row = match on {
        true => setting_row(Some(widgets::status_dot(Status::Ok)), tr("The assistant is on"), None, Space::new()),
        false => setting_row(
            Some(widgets::status_dot(Status::Inactive)),
            tr("The assistant is off"),
            Some(tr("Choose what it may use below, then turn it on.").to_string()),
            Space::new(),
        ),
    };
    let footer =
        tr("It chats with you and works in Torro Workplace with only what you allow here. You can change it any time.");
    widgets::group(tr("Status"), vec![row], Some(footer.to_string()))
}

/// The starting points of the first start, with a check at the one the rights equal.
fn preset_group<'a>(choice: &Rights) -> Element<'a, Message> {
    let rows = Preset::ALL
        .into_iter()
        .map(|preset| {
            let mark: Element<'a, Message> = match preset.rights() == *choice {
                true => icon::icon(Icon::Check, icon::SIZE)
                    .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) })
                    .into(),
                false => Space::new().width(icon::SIZE).into(),
            };
            let row = setting_row(None, preset.label(), Some(preset.detail().to_string()), mark);
            pressable(row, Some(Message::Preset(preset)))
        })
        .collect();
    let footer = tr("A starting point: every row below can be changed. None of them sends mail without asking.");
    widgets::group(tr("Start from"), rows, Some(footer.to_string()))
}

/// A row per bot – its picture, name and rights in words – that opens its page; without bots,
/// where they are made. No button removes a bot here: that is the bot editor's.
fn bots_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = data
        .bots
        .iter()
        .map(|bot| {
            let client = data.clients.iter().find(|client| client.id == bot.client && client.kind == BOT_CLIENT);
            let summary = client.map(|client| rights::rights_summary(rights::shown(&state.rights, client), BOT_CLIENT));
            let face = avatar(&bot.name, &bot.avatar, size::control_sm());
            let row = setting_row(Some(face), bot.name.as_str(), summary, chevron());
            pressable(row, Some(Message::OpenBot(bot.id.clone())))
        })
        .collect();
    if rows.is_empty() {
        let hint = tr("Make one with New bot in the sidebar.");
        rows.push(setting_row(None, tr("No bots yet"), Some(hint.to_string()), Space::new()));
    }
    let footer = tr("Each bot has rights of its own. A new bot may use nothing until you allow it here.");
    widgets::group(tr("Bots"), rows, Some(footer.to_string()))
}

/// "Turn on assistant", the page's primary button: with its word, in narrow windows only its icon,
/// so the section menu keeps its room; while the client is being made, busy.
fn turn_on_button<'a>(state: &State, class: SizeClass) -> Element<'a, Message> {
    if state.creating {
        return widgets::busy_button(tr("Turning on…"), style::button::primary);
    }
    let label = tr("Turn on assistant");
    match class {
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::Plus), label, style::button::primary, Some(Message::TurnOn))
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::Plus, label, style::button::primary, Some(Message::TurnOn))
        }
    }
}

/// A text button moved left by its own padding, so its label stands on the page's content axis.
fn on_axis<'a>(button: Element<'a, Message>) -> Element<'a, Message> {
    row![Space::new(), button].spacing(-style::button_padding().left).into()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use torro_ipc::api::{Area, Level};

    use super::super::tests::{data, turned_off};
    use super::*;
    use crate::Action as Act;

    /// The requests of an action; none for anything but a send.
    fn requests(action: Action) -> Vec<Request> {
        match action {
            Act::Send(requests) => requests,
            Act::None => Vec::new(),
            other => panic!("Access only ever sends: {other:?}"),
        }
    }

    /// The demo's bot and its own client.
    fn travel(data: &Data) -> (&BotSummary, &ClientInfo) {
        let bot = data.bots.first().expect("the demo has a bot");
        let client = data.clients.iter().find(|client| client.id == bot.client).expect("its client");
        (bot, client)
    }

    /// Saves the waiting draft as the clock would once it is due.
    fn saved(state: &mut State) -> Vec<Request> {
        requests(save_now(state))
    }

    /// With the assistant on, the page is its editor with "Turn off assistant…" and the bots, and
    /// the header holds no button; every size builds.
    #[tokio::test]
    async fn with_a_client_the_page_edits_the_assistant() {
        let data = data().await;
        let state = State::default();
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            let page = page(&state, &data, class);
            assert!(page.sub_page.is_none() && page.actions.is_empty(), "no Turn on for an assistant that is on");
            let _ = page.body;
        }
        let mut state = State::default();
        let action = update(&mut state, Message::Rights(rights::Message::Level(Area::Mail, Level::Read)), &data);
        assert_eq!(action, Act::None, "the change waits for the clock");
        let client = super::super::client(&data).expect("the demo's assistant is on");
        assert_eq!(rights::shown(&state.rights, client).area(Area::Mail).level, Level::Read, "shown at once");
        let sent = saved(&mut state);
        assert!(
            matches!(&sent[..], [Request::UpdateClient { id, rights }] if *id == client.id && rights.area(Area::Mail).level == Level::Read),
            "{sent:?}"
        );
    }

    /// Without a client the page is the first start: "Start from" and the editor change the choice,
    /// "Turn on assistant" sends it with the client, once, and asks for no key.
    #[tokio::test]
    async fn without_a_client_turning_on_sends_the_choice_without_a_key() {
        let data = turned_off(&data().await);
        let mut state = State::default();
        {
            let first = page(&state, &data, SizeClass::Wide);
            assert_eq!(first.actions.len(), 1, "Turn on assistant in the header");
            let _ = first.body;
        }

        let _ = update(&mut state, Message::Preset(Preset::Template(RightsTemplate::ReadOnly)), &data);
        assert_eq!(state.choice, RightsTemplate::ReadOnly.rights());
        let change = update(&mut state, Message::Rights(rights::Message::Level(Area::Calendar, Level::Write)), &data);
        assert_eq!(change, Act::None, "nothing is saved before the client exists");
        assert!(saved(&mut state).is_empty(), "and no draft waits");
        assert_eq!(state.choice.area(Area::Calendar).level, Level::Write);

        let sent = requests(update(&mut state, Message::TurnOn, &data));
        let [Request::CreateClient { name, kind, agent, rights, .. }] = &sent[..] else { panic!("{sent:?}") };
        assert_eq!((name.as_str(), kind.as_str(), agent), (CLIENT_NAME, ASSISTANT_CLIENT, &None));
        assert_eq!(rights.as_ref(), Some(&state.choice), "the chosen rights, in the same request");
        assert!(state.creating);
        assert!(requests(update(&mut state, Message::TurnOn, &data)).is_empty(), "one click at a time");
        assert_eq!(page(&state, &data, SizeClass::Narrow).actions.len(), 1, "the busy button");

        // The core took it; the fresh list shows the client and the page becomes its editor.
        let on = super::super::tests::data().await;
        sent_ok(&mut state, &on);
        assert!(!state.creating);
        // A failed send lets the button be pressed again.
        let mut again = State::default();
        let _ = update(&mut again, Message::TurnOn, &data);
        super::sent(&mut again, true, &Err("not connected".into()));
        assert!(!again.creating);
    }

    fn sent_ok(state: &mut State, data: &Data) {
        super::sent(state, true, &Ok(data.clone()));
    }

    /// "Turn off assistant…" asks first, then removes the assistant's client – nothing else.
    #[tokio::test]
    async fn turning_off_asks_and_removes_only_the_assistant() {
        let data = data().await;
        let mut state = State { choice: RightsTemplate::FullAccess.rights(), ..State::default() };
        assert!(requests(update(&mut state, Message::AskTurnOff, &data)).is_empty(), "asks first");
        assert!(overlay(&state, &data).is_some() && escape(&state).is_some());
        let _ = update(&mut state, Message::CloseDialog, &data);
        assert!(overlay(&state, &data).is_none());
        let _ = update(&mut state, Message::AskTurnOff, &data);
        let sent = requests(update(&mut state, Message::TurnOff, &data));
        let client = super::super::client(&data).expect("on");
        assert_eq!(sent, [Request::RemoveClient { id: client.id.clone() }]);
        assert_eq!(state.choice, Rights::assistant(), "turned on again, it starts from the suggestion");
        let off = turned_off(&data);
        let _ = update(&mut state, Message::AskTurnOff, &off);
        assert!(overlay(&state, &off).is_none(), "nothing to turn off");
        assert!(requests(update(&mut state, Message::TurnOff, &off)).is_empty());
    }

    /// The row of a bot opens the editor of the bot's own client below the section; its changes
    /// go to that client, "Back" saves them at once.
    #[tokio::test]
    async fn a_bot_row_opens_the_editor_of_the_bot() {
        let data = data().await;
        let (bot, client) = travel(&data);
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenBot(bot.id.clone()), &data);
        assert_eq!(open_bot(&state, &data).map(|open| open.id.as_str()), Some(bot.id.as_str()));
        {
            let below = page(&state, &data, SizeClass::Wide);
            assert_eq!(below.sub_page.as_ref().map(|(name, _)| name.as_str()), Some(bot.name.as_str()));
            assert!(below.actions.is_empty(), "a bot's page has no button, none to remove it either");
        }

        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Tasks, Level::Read)), &data);
        let sent = requests(update(&mut state, Message::Back, &data));
        assert!(
            matches!(&sent[..], [Request::UpdateClient { id, rights }] if *id == client.id && rights.area(Area::Tasks).level == Level::Read),
            "{sent:?}"
        );
        assert!(open_bot(&state, &data).is_none(), "back on the assistant's page");

        // A bot the data does not list shows the assistant's page.
        show(&mut state, Some("gone".into()));
        assert!(open_bot(&state, &data).is_none());
        assert!(page_of(&state, &data).sub_page.is_none());
        // A bot whose client is missing says what to do and changes nothing.
        let without =
            Data { clients: data.clients.iter().filter(|c| c.id != client.id).cloned().collect(), ..data.clone() };
        show(&mut state, Some(bot.id.clone()));
        let _ = page_of(&state, &without).body;
        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Tasks, Level::Write)), &without);
        assert!(saved(&mut state).is_empty());
    }

    fn page_of<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
        page(state, data, SizeClass::Wide)
    }

    /// A draft goes once the core took it and the reload brought it, and drops when the send
    /// failed; a plain reload settles nothing that is still on its way.
    #[tokio::test]
    async fn the_draft_follows_the_host_s_answers() {
        let data = data().await;
        let client = super::super::client(&data).expect("on").clone();
        let mut state = State::default();
        let level = |state: &State| rights::shown(&state.rights, &client).area(Area::Mail).level;

        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Mail, Level::Read)), &data);
        let _ = saved(&mut state);
        super::sent(&mut state, false, &Ok(data.clone()));
        assert_eq!(level(&state), Level::Read, "a reload of another section is no answer");
        super::sent(&mut state, true, &Ok(data.clone()));
        assert_eq!(level(&state), client.rights.area(Area::Mail).level, "saved: the core's rights show");

        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Mail, Level::Read)), &data);
        let _ = saved(&mut state);
        super::sent(&mut state, true, &Err("not connected".into()));
        assert_eq!(level(&state), client.rights.area(Area::Mail).level, "failed: dropped");
        assert!(subscription(&state).units() == 0, "no clock at rest");
    }

    /// Task S.34: the boundary in the app process. Whatever the section is asked to do, it sends
    /// `clients.update_client`, and `clients.create_client` and `clients.remove_client` only for
    /// the assistant – never a new key, never through `Ask`, whose answer would reach it.
    #[tokio::test]
    async fn access_sends_only_what_it_may() {
        let on = data().await;
        let off = turned_off(&on);
        let (bot, bot_client) = travel(&on);
        let assistant = super::super::client(&on).expect("on").id.clone();
        // A bot whose summary names a client of another kind: its page changes nothing.
        let mut foreign = on.clone();
        if let Some(client) = foreign.clients.iter_mut().find(|client| client.id == bot_client.id) {
            client.kind = "script".into();
        }
        let messages = || {
            vec![
                Message::Preset(Preset::Suggested),
                Message::Rights(rights::Message::Level(Area::Calendar, Level::Write)),
                Message::Rights(rights::Message::Admin(Area::Settings, true)),
                Message::Rights(rights::Message::Command("calendar.list_events", false)),
                Message::Rights(rights::Message::Confirm(torro_ipc::api::Confirm::OnEveryChange)),
                Message::Rights(rights::Message::All(rights::Pick::Calendars, false)),
                Message::Rights(rights::Message::Tick(rights::Pick::Calendars, "x".into(), true)),
                Message::Rights(rights::Message::Outbound(torro_ipc::api::Outbound::Never)),
                Message::TurnOn,
                Message::AskTurnOff,
                Message::TurnOff,
                Message::OpenBot(bot.id.clone()),
                Message::Rights(rights::Message::Level(Area::Tasks, Level::Read)),
                Message::Rights(rights::Message::All(rights::Pick::TaskLists, true)),
                Message::Back,
                Message::CloseDialog,
            ]
        };
        let mut on_foreign = State::default();
        let _ = update(&mut on_foreign, Message::OpenBot(bot.id.clone()), &foreign);
        let level = Message::Rights(rights::Message::Level(Area::Tasks, Level::Read));
        assert_eq!(update(&mut on_foreign, level, &foreign), Act::None);
        assert!(saved(&mut on_foreign).is_empty(), "no other kind of client is changed here");

        let mut sent = Vec::new();
        for data in [&on, &off] {
            let mut state = State::default();
            for message in messages() {
                sent.extend(requests(update(&mut state, message, data)));
                sent.extend(saved(&mut state));
            }
        }
        assert!(!sent.is_empty());
        for request in &sent {
            match request {
                Request::UpdateClient { id, .. } => {
                    assert!(*id == assistant || *id == bot_client.id, "only the assistant's and the bot's: {id}")
                }
                Request::CreateClient { kind, .. } => assert_eq!(kind, ASSISTANT_CLIENT),
                Request::RemoveClient { id } => assert_eq!(*id, assistant),
                other => panic!("Access sent {other:?}"),
            }
        }
        // And in its code: no new key, no data rule, no way an answer could come back to it.
        let forbidden = [
            ["NewClient", "Key"].concat(),
            ["Mail", "ToCloud"].concat(),
            ["mail_to", "_cloud"].concat(),
            ["::Ask", "("].concat(),
            ["Control", "Centre"].concat(),
            ["Open", "Url"].concat(),
        ];
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assistant/access.rs");
        let text = std::fs::read_to_string(&file).expect("the section reads");
        let code =
            text.lines().take_while(|line| line.trim() != "#[cfg(test)]").filter(|l| !l.trim_start().starts_with("//"));
        let found: Vec<&str> = code.filter(|line| forbidden.iter().any(|word| line.contains(word.as_str()))).collect();
        assert!(found.is_empty(), "{found:?}");
    }

    /// The starting points: their rights, names and lines; the suggestion first.
    #[test]
    fn the_presets_start_from_the_suggestion() {
        assert_eq!(Preset::ALL[0], Preset::Suggested);
        assert_eq!(Preset::Suggested.rights(), Rights::assistant());
        assert_eq!(
            Preset::Template(RightsTemplate::ReadOnly).label(),
            rights::template_label(RightsTemplate::ReadOnly)
        );
        for preset in Preset::ALL {
            assert!(!preset.label().is_empty() && !preset.detail().is_empty());
        }
    }
}
