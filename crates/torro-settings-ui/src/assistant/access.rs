//! Assistant › Access (task S.34, concept `docs/settings.md` › "Assistent und Bots"): what the
//! assistant and each of its bots may use, in the assistant's own window.
//!
//! Two tabs (issue #547, since issue #763 without "Bots"):
//!
//! ```text
//! Access       Status            the assistant on or off
//!              Start from        while it is off: the starting points of its rights ([`Preset`])
//!              What the assistant may use   the rights editor's access group and "Ask first", without
//!                                           the built-in tools and MCP servers (Assistant › API and MCP,
//!                                           issue #666)
//!              Turn off assistant…          while it is on, after a question; last, as what destroys
//! Selections   the calendars, lists, accounts … of each area it may use ([`rights::Part`]), not the tools
//! ```
//!
//! A bot's rights are a page below the section Bots since issue #763 ([`bot_page`], "Bots › Travel
//! planner"), with the same two tabs of its own client; the list of bots shows each one's rights in
//! words ([`bot_rights`]). This module still draws and sends both, as the one with rights. A tab only
//! changes what is shown; it sends nothing.
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
//!   `clients.remove_client`. The one other page is API and MCP of every app (`crate::outside`, issue
//!   #531), which sends only `clients.update_client`: for keys of scripts and MCP clients, and for the
//!   assistant and its bots only their tools and MCP servers (issue #666). Never `clients.new_key` – the
//!   core runs the assistant's and the bots' tools itself with their clients' rights, so nobody needs
//!   their keys.
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
use torro_gui::theme::palette;
use torro_gui::widgets::{self, Status};
use torro_ipc::api::{ASSISTANT_CLIENT, BOT_CLIENT, Rights, RightsTemplate};
use torro_ipc::bots::BotSummary;
use torro_ipc::{ClientInfo, Request, Response};

use super::Action;
use crate::parts::{confirm, pressable, setting_row};
use crate::rights::{self, Known, Naming, Part};
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

/// The tabs of the page (issue #547), the assistant's and a bot's alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Access,
    Selections,
}

impl Tab {
    /// Every tab, in the order of the row; the index is what [`Message::Tab`] carries.
    pub const ALL: [Tab; 2] = [Tab::Access, Tab::Selections];

    /// Its word, translated.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Access => tr("Access"),
            Tab::Selections => tr("Selections"),
        }
    }
}

/// The section's state: whose rights are open, the tab, the first start's choice and the editor's
/// draft.
#[derive(Debug)]
pub struct State {
    /// The tab on screen; a bot opened or closed starts at Access again.
    tab: Tab,
    /// The bot (its [`BotSummary::id`]) whose rights are open on a page below the section Bots;
    /// `None` while the assistant's are.
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
            tab: Tab::Access,
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
    /// A tab of the row, by its place in [`Tab::ALL`]: shows that part, sends nothing.
    Tab(usize),
    /// A row of "Start from" on the first start.
    Preset(Preset),
    /// "Turn on assistant" in the header of the first start.
    TurnOn,
    /// "Turn off assistant…", then its confirmation.
    AskTurnOff,
    TurnOff,
    /// "Cancel", Escape or a click next to the confirmation.
    CloseDialog,
    /// The bot with this id: its rights on a page below the section Bots.
    OpenBot(String),
    /// Back from a bot's page to the list of bots.
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
        Message::Tab(index) => {
            state.tab = Tab::ALL.get(index).copied().unwrap_or(Tab::Access);
            Action::None
        }
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

/// Opens the rights of the assistant (`None`) or of the bot with this id – what "Access" in the
/// Bots section, the bot editor's "What the bot may use", "Change access" under a refused tool and
/// `--settings access [--bot <id>]` ask for. A bot the loaded data does not list has no page
/// ([`bot_page`]), so Bots shows its list. The editor closes its open areas; a waiting draft is
/// saved as usual.
pub fn show(state: &mut State, bot: Option<String>) {
    state.bot = bot;
    state.tab = Tab::Access;
    state.asking = false;
    rights::leave(&mut state.rights);
}

/// A control of the assistant's own page (issue #763): a bot's page left open below Bots is closed
/// first – its waiting draft saved at once, as [`Message::Back`] does –, so the editor changes the
/// assistant's rights the page shows and never the bot's. The editor's clock passes as it is: it
/// saves whatever draft waits, whichever page is on screen. The page of a bot sends to [`update`].
pub fn update_assistant(state: &mut State, message: Message, data: &Data) -> Action {
    let clock = matches!(message, Message::Rights(rights::Message::Clock(_)));
    if clock || state.bot.is_none() {
        return update(state, message, data);
    }
    let saved = rights::save_now(&mut state.rights);
    show(state, None);
    match (saved, update(state, message, data)) {
        (Action::Send(mut first), Action::Send(then)) => {
            first.extend(then);
            Action::Send(first)
        }
        (Action::None, then) => then,
        (first, _) => first,
    }
}

/// Shows the assistant's own page again when Access is reached another way than its own controls
/// (the list, General's status row; issue #763): a bot's page left open below Bots closes, so Access
/// opens at its first tab without the bot's open areas. A waiting draft still goes with the clock.
pub fn leave_bot(state: &mut State) {
    if state.bot.is_some() {
        show(state, None);
    }
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

/// The row of tabs over a page.
fn tab_row<'a>(state: &State) -> Element<'a, Message> {
    let labels = Tab::ALL.iter().map(|tab| (tab.label().to_string(), None)).collect();
    let chosen = Tab::ALL.iter().position(|tab| *tab == state.tab).unwrap_or(0);
    widgets::tabs(labels, chosen, Message::Tab)
}

/// The assistant's page – on, or its first start with "Turn on assistant" in the header, the page's
/// one primary button – under its row of tabs.
pub fn page<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Page<'a, Message> {
    let client = super::client(data);
    let body = match state.tab {
        Tab::Access => access_tab(state, data, client),
        Tab::Selections => {
            let rights = client.map_or(&state.choice, |client| rights::shown(&state.rights, client));
            editor(state, data, rights, ASSISTANT_CLIENT, tr("What the assistant may use"), Part::Selections)
        }
    };
    let page = Page::new(column![tab_row(state), body].spacing(layout::section_gap()));
    match client {
        Some(_) => page,
        None => page.actions(vec![turn_on_button(state, class)]),
    }
}

/// A part of the rights editor for `rights` of a client of `kind`, its messages wrapped.
fn editor<'a>(
    state: &'a State,
    data: &'a Data,
    rights: &'a Rights,
    kind: &'a str,
    title: &'a str,
    part: Part,
) -> Element<'a, Message> {
    rights::view_part(&state.rights, rights, kind, Known::of(data), Naming::Tools, title, part).map(Message::Rights)
}

/// The tab Access of the assistant: its status, on the first start "Start from", what it may use
/// and "Ask first", and while it is on last "Turn off assistant…" – what undoes the page stands at
/// its end, after everything it would take away.
fn access_tab<'a>(state: &'a State, data: &'a Data, client: Option<&'a ClientInfo>) -> Element<'a, Message> {
    let title = tr("What the assistant may use");
    match client {
        Some(client) => {
            let rights = rights::shown(&state.rights, client);
            let off =
                widgets::text_button(None, tr("Turn off assistant…"), style::button::danger, Some(Message::AskTurnOff));
            column![
                status_group(true),
                editor(state, data, rights, ASSISTANT_CLIENT, title, Part::Access),
                on_axis(off)
            ]
            .spacing(layout::section_gap())
            .into()
        }
        None => column![
            status_group(false),
            preset_group(&state.choice),
            editor(state, data, &state.choice, ASSISTANT_CLIENT, title, Part::Access),
        ]
        .spacing(layout::section_gap())
        .into(),
    }
}

/// The page of the open bot's rights, below the section Bots (issue #763): its name follows in the
/// path, "Back" leads to the list. `None` while no bot is open, or the data no longer lists it.
pub fn bot_page<'a>(state: &'a State, data: &'a Data) -> Option<Page<'a, Message>> {
    let bot = open_bot(state, data)?;
    let body = column![tab_row(state), bot_rights_tab(state, data, bot)].spacing(layout::section_gap());
    Some(Page::new(body).sub_page(bot.name.clone(), Message::Back))
}

/// A bot's rights in words, as the list of bots shows them under its name (issue #763): the
/// draft's, while one waits. `None` where the core lists no client of the bot.
pub fn bot_rights(state: &State, data: &Data, bot: &BotSummary) -> Option<String> {
    let client = data.clients.iter().find(|client| client.id == bot.client && client.kind == BOT_CLIENT)?;
    Some(rights::rights_summary(rights::shown(&state.rights, client), BOT_CLIENT))
}

/// A bot's page under its tabs: the part of the editor of its own client, or, where the core lists
/// none, what to do.
fn bot_rights_tab<'a>(state: &'a State, data: &'a Data, bot: &'a BotSummary) -> Element<'a, Message> {
    match shown_client(state, data) {
        Some(client) => {
            let rights = rights::shown(&state.rights, client);
            let part = match state.tab {
                Tab::Selections => Part::Selections,
                Tab::Access => Part::Access,
            };
            editor(state, data, rights, BOT_CLIENT, tr("What the bot may use"), part)
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

    /// With the assistant on, the page is its editor with "Turn off assistant…", and the header
    /// holds no button; every size builds.
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

    /// A bot opens the editor of the bot's own client on a page of its own ([`bot_page`]); its
    /// changes go to that client, "Back" saves them at once.
    #[tokio::test]
    async fn a_bot_row_opens_the_editor_of_the_bot() {
        let data = data().await;
        let (bot, client) = travel(&data);
        let mut state = State::default();
        let _ = update(&mut state, Message::OpenBot(bot.id.clone()), &data);
        assert_eq!(open_bot(&state, &data).map(|open| open.id.as_str()), Some(bot.id.as_str()));
        {
            let below = bot_page(&state, &data).expect("the bot's page");
            assert_eq!(below.sub_page.as_ref().map(|(name, _)| name.as_str()), Some(bot.name.as_str()));
            assert!(below.actions.is_empty(), "a bot's page has no button, none to remove it either");
        }
        assert!(page_of(&state, &data).sub_page.is_none(), "Access itself shows the assistant");

        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Tasks, Level::Read)), &data);
        let sent = requests(update(&mut state, Message::Back, &data));
        assert!(
            matches!(&sent[..], [Request::UpdateClient { id, rights }] if *id == client.id && rights.area(Area::Tasks).level == Level::Read),
            "{sent:?}"
        );
        assert!(open_bot(&state, &data).is_none(), "back on the assistant's page");

        // A bot the data does not list has no page.
        show(&mut state, Some("gone".into()));
        assert!(open_bot(&state, &data).is_none());
        assert!(bot_page(&state, &data).is_none());
        // A bot whose client is missing says what to do and changes nothing; its rights have no
        // words.
        let without =
            Data { clients: data.clients.iter().filter(|c| c.id != client.id).cloned().collect(), ..data.clone() };
        show(&mut state, Some(bot.id.clone()));
        assert!(bot_page(&state, &without).is_some_and(|page| page.sub_page.is_some()));
        assert!(bot_rights(&state, &without, bot).is_none());
        assert!(bot_rights(&state, &data, bot).is_some_and(|words| !words.is_empty()));
        let _ = update(&mut state, Message::Rights(rights::Message::Level(Area::Tasks, Level::Write)), &without);
        assert!(saved(&mut state).is_empty());
    }

    fn page_of<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
        page(state, data, SizeClass::Wide)
    }

    /// Issue #547, narrowed by issue #763: the assistant's page and a bot's have the tabs Access and
    /// Selections; a tab sends nothing, and opening or leaving a bot starts at Access again.
    #[tokio::test]
    async fn the_tabs_show_a_part_and_send_nothing() {
        let data = data().await;
        let (bot, _) = travel(&data);
        let mut state = State::default();
        assert_eq!(state.tab, Tab::Access);
        assert_eq!(Tab::ALL, [Tab::Access, Tab::Selections]);
        for (index, tab) in Tab::ALL.into_iter().enumerate() {
            assert_eq!(update(&mut state, Message::Tab(index), &data), Act::None, "{tab:?}");
            assert_eq!(state.tab, tab);
            for on in [&data, &turned_off(&data)] {
                for class in [SizeClass::Wide, SizeClass::Narrow] {
                    let _ = page(&state, on, class).body;
                }
            }
        }
        assert_eq!(update(&mut state, Message::Tab(7), &data), Act::None);
        assert_eq!(state.tab, Tab::Access, "an index past the row is Access");

        let _ = update(&mut state, Message::Tab(1), &data);
        let _ = update(&mut state, Message::OpenBot(bot.id.clone()), &data);
        assert_eq!(state.tab, Tab::Access, "a bot opens at Access");
        let _ = update(&mut state, Message::Tab(1), &data);
        assert_eq!(state.tab, Tab::Selections);
        let _ = bot_page(&state, &data).map(|page| page.body);
        let _ = update(&mut state, Message::Back, &data);
        assert_eq!(state.tab, Tab::Access, "back at Access");
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
