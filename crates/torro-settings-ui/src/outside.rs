//! Settings › API and MCP of an app (issue #531, concept `docs/settings.md` › "Die Regel"): what the
//! programs and agents outside the suite may do in **this** app's area, behind its own gear.
//!
//! ```text
//! API keys               a row per key of a script: its name, what it may here ("Calendar: Read · 2 calendars")
//! MCP clients            a row per MCP client (Claude Code, Codex …), the same
//!                        a row opens the client: the rights editor for this app's areas only
//! Used by the assistant  (the assistant only, issue #666) for [Assistant ▾ | a bot]: per built-in tool and
//!                        MCP server a switch and "Show commands" (`rights::Part::Servers`)
//! Torro Workplace        API clients ↗ · MCP clients ↗, where keys are made and clients connected
//!                        (only with the main application)
//! ```
//!
//! **Used by the assistant** (issue #666): what the app itself may use of the MCP servers and built-in
//! tools, down to single commands. Only the assistant uses them so far, for itself and each bot, which
//! keep their rights of their own; the dropdown chooses whose ([`Message::UsedBy`]). Allowed unless a
//! person switched something off: the core opens the assistant and its bots to every tool and server
//! (`Rights::opened`). Here the page also changes the assistant's or a bot's client – only its tools,
//! servers and their commands ([`merged_used`]), never another right.
//!
//! Every app shows only its own areas ([`areas`]): the calendar `calendar`, the tasks app `tasks` and
//! `github`, the Notification Hub and the App Feed `inbox`, mail `mail`, the assistant `assistant` and
//! `bots`. Contacts and everything else several apps share stay in the control centre, which shows the
//! same rights for all areas together. The rights are stored once, in the core; this page reads them
//! from `clients.list_clients` like the control centre and changes them with `clients.update_client`,
//! so both always show the same.
//!
//! # The boundary in an app process
//!
//! Rights are `clients.*`, the app key's alone (`docs/api.md`, "Was nie nach außen geht", point 2), and
//! no agent ever changes them: there is no MCP tool for this page. Like Assistant › Access the page
//! keeps to a rule of the code, which its tests watch over:
//!
//! - It sends only `clients.update_client`, for a client of the kind `script` or an MCP kind
//!   ([`torro_ipc::api::is_mcp_kind`]) only the app's areas, for the assistant or a bot only its tools and
//!   servers – never for a hook, never `clients.create_client`, `clients.remove_client` or
//!   `clients.new_key`. Making keys and connecting
//!   clients stays in the control centre (in a single app without it: issue #538).
//! - It changes only the rights of [`areas`]: a control of any other area is dropped before it reaches
//!   the editor, and "Ask first" is not shown.
//! - `clients.update_client` carries the whole rights, and the control centre may have changed the
//!   client meanwhile. So the editor opens only once the first fresh `clients.list_clients` of this
//!   opening is in (until then the client's page says "Updating…"), and every save reads the client
//!   anew right before it and changes only the app's areas in what the core holds then ([`send`],
//!   [`merged`]): nothing of another area, "Ask first" or an MCP server goes back as this window last
//!   saw it. Within the app's own areas the page sends what it shows: a change made in the control
//!   centre to the same area since the page loaded the clients is replaced by it (the last one wins).
//! - A change goes out at once (not [`rights::SAVE_DELAY`] later as in the control centre): an app
//!   closes its settings without a word to the host, and a draft that waits would be lost. Only one
//!   `clients.update_client` is on its way at a time, answered on a message of its own, so an older one
//!   never lands after a newer one; a change made meanwhile waits for the editor's clock, and while it
//!   waits no other client can be changed (it would take the draft's place).

use iced::widget::{column, responsive, svg};
use iced::{Element, Length, Theme};
use torro_gui::widgets::{Align, DROPDOWN_SEARCH, Dropdown, DropdownEvent, MenuItem, MenuSection};

use std::sync::Arc;
use torro_core::AppId;
use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::layout;
use torro_gui::theme::palette;
use torro_gui::widgets;
use torro_ipc::api::{ASSISTANT_CLIENT, Area, BOT_CLIENT, CATALOGUE, SCRIPT_CLIENT, is_mcp_kind};
use torro_ipc::tools::ToolId;

use torro_ipc::api::Rights;
use torro_ipc::{ClientInfo, Request, Response};

use crate::parts::{chevron, pressable, setting_row};
use crate::rights::{self, Known, Naming, Pick};
use crate::{Action, Backend, Data, Page, Section};

/// The section, as the list, the path and `--settings api-mcp` name it.
pub const SECTION: Section = Section { id: "api-mcp", icon: Icon::Code, label: "API and MCP" };

/// The areas of the rights `app` shows here (decision 3 of the plan of #528): only its own. Empty for
/// the control centre, which shows every area together, and for any other app.
pub fn areas(app: AppId) -> &'static [Area] {
    match app.window_class {
        "torro-calendar" => &[Area::Calendar],
        "torro-tasks" => &[Area::Tasks, Area::Github],
        "torro-inbox" | "torro-inbox-notifications" => &[Area::Inbox],
        "torro-inbox-mail" => &[Area::Mail, Area::Workflows],
        "torro-assistant" => &[Area::Assistant, Area::Bots],
        _ => &[],
    }
}

/// The sections this page adds to `app`'s settings: [`SECTION`] where the app has [`areas`], else
/// none. What `--settings` takes besides the app's own words, like `crate::account_sections`.
pub fn sections(app: AppId) -> Vec<Section> {
    (!areas(app).is_empty()).then_some(SECTION).into_iter().collect()
}

/// What the page loads for `app`: the clients, and what the selections of its areas offer – the
/// calendars, the task lists (whose GitHub filter lists name the repositories), the address books, the
/// inbox sources, the accounts (mail accounts, and the account a calendar or list belongs to).
pub fn wanted(app: AppId) -> Vec<Request> {
    let mut wanted = vec![Request::ListClients, Request::Accounts];
    // What "Used by the assistant" lists (issue #666): the tools, the servers and the bots.
    if uses_servers(app) {
        wanted.extend([Request::ListTools, Request::ListExtensions, Request::ListBots]);
    }
    for area in areas(app) {
        let request = match area {
            Area::Calendar => Request::CalendarCollections,
            Area::Tasks | Area::Github => Request::TaskCollections,
            Area::Contacts => Request::ListAddressBooks { account: None },
            Area::Inbox => Request::ListSources,
            _ => continue,
        };
        if !wanted.contains(&request) {
            wanted.push(request);
        }
    }
    wanted
}

/// Whether `app` itself uses MCP servers and built-in tools, so the page shows "Used by the assistant"
/// (issue #666): the assistant, for itself and its bots. Other apps get it once they use them.
pub fn uses_servers(app: AppId) -> bool {
    app == AppId::ASSISTANT
}

/// The page's state: the app, the client open below the section, the editor and whether its save is on
/// its way.
#[derive(Debug)]
pub struct State {
    app: AppId,
    /// The client open on the page below the section, by id **and** kind (as Access finds its clients):
    /// a client of another kind with the same id is never edited here.
    open: Option<(String, String)>,
    /// The rights editor: its draft on the way to the core, the typed repository, the open areas.
    rights: rights::State,
    /// A `clients.update_client` of this page is on its way.
    sending: bool,
    /// A list of clients came back well since the settings opened ([`waiting`]).
    fresh: bool,
    /// Whose tools and servers "Used by the assistant" shows: `None` the assistant's, else a bot's by its
    /// id (issue #666).
    used: Option<String>,
    /// The dropdown of "Used by the assistant".
    menus: Dropdown<()>,
}

impl State {
    /// The page of `app`.
    pub fn new(app: AppId) -> State {
        State {
            app,
            open: None,
            rights: rights::State::default(),
            sending: false,
            fresh: false,
            used: None,
            menus: Dropdown::default(),
        }
    }

    /// The areas this page shows ([`areas`]).
    pub fn areas(&self) -> &'static [Area] {
        areas(self.app)
    }

    /// The app whose page this is.
    pub fn app(&self) -> AppId {
        self.app
    }
}

/// What the page's controls report.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The row of a client: its id and kind.
    Open(String, String),
    /// Back from a client's page to the list.
    Back,
    /// A row that leads to the control centre at this section (`api`, `mcp`).
    ControlCentre(&'static str),
    /// A control of the rights editor, its clock included ([`subscription`]).
    Rights(rights::Message),
    /// The dropdown of "Used by the assistant" (issue #666).
    UsedMenu(DropdownEvent<()>),
    /// A choice in it: `None` the assistant, else a bot by its id.
    UsedBy(Option<String>),
    /// A switch of "Used by the assistant": a tool, a server or one of their commands.
    Used(rights::Message),
}

/// Whether a client of `kind` is edited here: a script's key or an MCP client. The assistant, the bots
/// and the hooks are not – their rights are the assistant's Access or fixed.
pub fn editable(kind: &str) -> bool {
    kind == SCRIPT_CLIENT || is_mcp_kind(kind)
}

/// The keys of scripts and the MCP clients the data lists, each in the order of the core.
pub fn listed(data: &Data) -> (Vec<&ClientInfo>, Vec<&ClientInfo>) {
    let keys = data.clients.iter().filter(|client| client.kind == SCRIPT_CLIENT).collect();
    let mcp = data.clients.iter().filter(|client| is_mcp_kind(&client.kind)).collect();
    (keys, mcp)
}

/// The client open below the section, while the data lists it with the same id and kind.
fn open_client<'a>(state: &State, data: &'a Data) -> Option<&'a ClientInfo> {
    let (id, kind) = state.open.as_ref()?;
    data.clients.iter().find(|client| client.id == *id && client.kind == *kind && editable(&client.kind))
}

/// Whether the first fresh list of clients of this opening is still missing: until the host saw one
/// answer of `clients.list_clients` come back well ([`clients_fresh`]) the editor stays shut, so rights
/// the window kept from the last time are never sent back over newer ones. A failed answer does not
/// open it.
pub fn waiting(state: &State) -> bool {
    !state.fresh
}

/// The host put in an answer of `clients.list_clients` that came back well: the clients are fresh.
pub fn clients_fresh(state: &mut State) {
    state.fresh = true;
}

/// Whether a control of the editor belongs to one of `areas`: a level, an admin switch, a command or a
/// selection of another area, "Ask first", "Send on my behalf" outside mail and anything about an MCP
/// server are dropped. What only changes what the editor shows passes.
fn belongs(message: &rights::Message, areas: &[Area]) -> bool {
    use rights::Message as M;
    match message {
        M::Level(area, _) | M::Admin(area, _) | M::Expand(area) => areas.contains(area),
        M::Command(name, _) => CATALOGUE.iter().any(|command| command.name == *name && areas.contains(&command.area)),
        M::All(pick, _) | M::Tick(pick, ..) => areas.contains(&pick.area()),
        M::HiddenBook(..) => areas.contains(&Area::Contacts),
        M::AddRepo => areas.contains(&Pick::GithubRepos.area()),
        M::Outbound(_) => areas.contains(&Area::Mail),
        M::ImageContent(_) => areas.contains(&Area::Chat),
        M::Confirm(_)
        | M::ServerLevel(..)
        | M::ExpandServer(_)
        | M::ServerTool(..)
        | M::Server(..)
        | M::Tool(..)
        | M::ExpandTool(_) => false,
        M::RepoInput(_) | M::Clock(_) => true,
    }
}

/// Whether a control belongs to "Used by the assistant" (issue #666): a tool's or a server's switch, a
/// command of a built-in tool or a server's tool, and what only changes what is shown.
fn belongs_used(message: &rights::Message) -> bool {
    use rights::Message as M;
    match message {
        M::Tool(..) | M::Server(..) | M::ServerTool(..) | M::ExpandTool(_) | M::ExpandServer(_) | M::Clock(_) => true,
        M::Command(name, _) => ToolId::of_command(name).is_some(),
        _ => false,
    }
}

/// The client "Used by the assistant" shows ([`State::used`]): the assistant's, or the bot's.
fn used_client<'a>(state: &State, data: &'a Data) -> Option<&'a ClientInfo> {
    match &state.used {
        None => data.clients.iter().find(|client| client.kind == ASSISTANT_CLIENT),
        Some(bot) => {
            let client = &data.bots.iter().find(|summary| &summary.id == bot)?.client;
            data.clients.iter().find(|held| &held.id == client && held.kind == BOT_CLIENT)
        }
    }
}

/// What a control of the page does:
///
/// - [`Message::Open`] and [`Message::Back`] show a client or the list; a draft that waits goes out
///   first.
/// - A control of the editor changes the draft of the open client ([`rights::update`]) and sends it at
///   once, unless a save of this page is still on its way; then the editor's clock sends it later. Only
///   for the areas of the app ([`belongs`]), only for a client [`editable`] here, and never before the
///   first fresh list of clients ([`waiting`]).
/// - [`Message::ControlCentre`] opens the control centre, only with the main application.
///
/// Nothing else is ever sent, and only as [`Action::Send`] of `clients.update_client`.
pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    match message {
        Message::Open(id, kind) => {
            let action = save(state);
            state.open = editable(&kind).then_some((id, kind));
            rights::leave(&mut state.rights);
            action
        }
        Message::Back => {
            let action = save(state);
            state.open = None;
            rights::leave(&mut state.rights);
            action
        }
        Message::ControlCentre(section) => match torro_gui::installed::main_app() {
            true => Action::ControlCentre(section.to_string()),
            false => Action::None,
        },
        Message::Rights(message) => edit(state, message, data),
        Message::UsedMenu(event) => match state.menus.update(event) {
            true => Action::Focus(DROPDOWN_SEARCH),
            false => Action::None,
        },
        Message::UsedBy(used) => {
            state.menus.close();
            let action = save(state);
            state.used = used;
            action
        }
        Message::Used(message) => edit_used(state, message, data),
    }
}

/// Hands a switch of "Used by the assistant" on to the editor for the client chosen there
/// ([`used_client`]) and saves it as [`edit`] does.
fn edit_used<A>(state: &mut State, message: rights::Message, data: &Data) -> Action<A> {
    if !uses_servers(state.app) || !belongs_used(&message) || waiting(state) {
        return Action::None;
    }
    let Some(client) = used_client(state, data) else { return Action::None };
    let shows_only = matches!(message, rights::Message::ExpandTool(_) | rights::Message::ExpandServer(_));
    if !shows_only && rights::waiting_for(&state.rights).is_some_and(|id| id != client.id) {
        return Action::None;
    }
    let known = Known::of(data);
    let _: Action<A> = rights::update(&mut state.rights, message, Some(client), &known);
    save(state)
}

/// Hands a control of the editor on (see [`update`]).
fn edit<A>(state: &mut State, message: rights::Message, data: &Data) -> Action<A> {
    let known = Known::of(data);
    if matches!(message, rights::Message::Clock(_)) {
        if state.sending {
            return Action::None;
        }
        let action = rights::update(&mut state.rights, message, None, &known);
        state.sending |= matches!(action, Action::Send(_));
        return action;
    }
    if !belongs(&message, state.areas()) || waiting(state) {
        return Action::None;
    }
    let Some(client) = open_client(state, data) else { return Action::None };
    // A draft of another client still waits for its turn: a change would replace it, and lose it.
    // What only changes what the editor shows goes through.
    let shows_only = matches!(message, rights::Message::RepoInput(_) | rights::Message::Expand(_));
    if !shows_only && rights::waiting_for(&state.rights).is_some_and(|id| id != client.id) {
        return Action::None;
    }
    let _: Action<A> = rights::update(&mut state.rights, message, Some(client), &known);
    save(state)
}

/// `edited` – what the page shows for a client – brought into `fresh`, what the core holds for it now:
/// for each of `areas` the level and admin switch, the selection, the commands switched off and, for
/// mail, "Send on my behalf"; everything else stays as in `fresh`.
pub fn merged(fresh: &Rights, edited: &Rights, areas: &[Area]) -> Rights {
    let mut rights = fresh.clone();
    for &area in areas {
        rights.areas.insert(area, edited.area(area));
    }
    for pick in Pick::ALL.into_iter().filter(|pick| areas.contains(&pick.area())) {
        *pick.of_mut(&mut rights) = pick.of(edited).clone();
    }
    let in_areas =
        |name: &String| CATALOGUE.iter().any(|command| command.name == name.as_str() && areas.contains(&command.area));
    rights.denied.retain(|name| !in_areas(name));
    rights.denied.extend(edited.denied.iter().filter(|name| in_areas(name)).cloned());
    if areas.contains(&Area::Mail) {
        rights.outbound = edited.outbound;
    }
    rights
}

/// `edited` – what "Used by the assistant" shows for the assistant or a bot – brought into `fresh`, what
/// the core holds for it now (issue #666): the built-in tools it reaches, its MCP servers with the level
/// of those without an entry, and which of their commands are switched off; everything else stays as in
/// `fresh`.
pub fn merged_used(fresh: &Rights, edited: &Rights) -> Rights {
    let mut rights = fresh.clone();
    rights.tools = edited.tools.clone();
    rights.extensions = edited.extensions.clone();
    rights.extensions_default = edited.extensions_default;
    let used =
        |name: &String| ToolId::of_command(name).is_some() || torro_ipc::extensions::parse_denied_name(name).is_some();
    rights.denied.retain(|name| !used(name));
    rights.denied.extend(edited.denied.iter().filter(|name| used(name)).cloned());
    rights
}

/// Sends what the page saves (`clients.update_client`), each time over the rights the core holds right
/// then ([`merged`]): asks `clients.list_clients`, takes the client by its id and changes only the
/// areas of `app` – for the assistant or a bot only its tools and servers ([`merged_used`]), and only
/// from the assistant's window ([`uses_servers`]; review of #663).
/// A client that is gone fails with what the core says; any other request goes as it is. Stops at the
/// first failure, as `crate::apply` does.
pub async fn send(backend: Arc<dyn Backend>, requests: Vec<Request>, app: AppId) -> Result<(), String> {
    let (areas, used) = (areas(app), uses_servers(app));
    for request in requests {
        let request = match request {
            Request::UpdateClient { id, rights } => {
                let fresh = match backend.call(Request::ListClients).await? {
                    // A key or an MCP client only, checked here too: whatever the page let through, a
                    // hook is never changed from here, the assistant and a bot only from its window.
                    Response::Clients { clients } => clients.into_iter().find(|client| {
                        client.id == id
                            && (editable(&client.kind)
                                || (used && [ASSISTANT_CLIENT, BOT_CLIENT].contains(&client.kind.as_str())))
                    }),
                    _ => None,
                };
                let Some(fresh) = fresh else { return Err(tr("This client no longer exists.").to_string()) };
                let rights = match editable(&fresh.kind) {
                    true => merged(&fresh.rights, &rights, areas),
                    false => merged_used(&fresh.rights, &rights),
                };
                Request::UpdateClient { id, rights }
            }
            other => other,
        };
        backend.call(request).await?;
    }
    Ok(())
}

/// Sends a waiting draft now, unless a save of this page is on its way.
fn save<A>(state: &mut State) -> Action<A> {
    if state.sending {
        return Action::None;
    }
    let action = rights::save_now(&mut state.rights);
    state.sending = matches!(action, Action::Send(_));
    action
}

/// A send of the settings came back (`host::Message::Done`): the page's save is settled – stored, it
/// goes with the next list of clients; failed, the editor shows what the core holds again.
///
/// A change made while it was on its way goes out at once now ([`Action::Send`]), not with the clock:
/// settings closed in the meantime would lose it.
pub fn done<A>(state: &mut State, result: &Result<(), String>) -> Action<A> {
    if !state.sending {
        return Action::None;
    }
    state.sending = false;
    let answer = result.clone().map(|()| torro_ipc::Response::Ok);
    rights::answered(&mut state.rights, &answer);
    save(state)
}

/// The settings' data came anew: a saved draft is in it now. A client that is gone shows the list
/// again, and its rights need not be saved any more.
pub fn loaded(state: &mut State, data: &Data) {
    rights::loaded(&mut state.rights);
    if waiting(state) {
        return;
    }
    if let Some((id, _)) = state.open.clone().filter(|_| open_client(state, data).is_none()) {
        rights::forget(&mut state.rights, &id);
        state.open = None;
        rights::leave(&mut state.rights);
    }
}

/// The page is left for another section: the list shows again when it comes back. A draft that waits
/// stays and goes with the clock.
pub fn leave(state: &mut State) {
    state.open = None;
    state.menus.close();
    rights::leave(&mut state.rights);
}

/// What Escape means here: close the dropdown of "Used by the assistant", else back from a client's page
/// to the list. `None` leaves it to the host.
pub fn escape(state: &State) -> Option<Message> {
    if state.menus.open().is_some() {
        return Some(Message::UsedMenu(DropdownEvent::Close));
    }
    state.open.is_some().then_some(Message::Back)
}

/// The open dropdown of "Used by the assistant" (issue #666): the assistant and each bot.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    state.menus.open()?;
    let assistant = MenuItem::plain(tr("Assistant"), Message::UsedBy(None)).current(state.used.is_none());
    let bots = data.bots.iter().map(|bot| {
        let chosen = state.used.as_deref() == Some(bot.id.as_str());
        MenuItem::plain(bot.name.as_str(), Message::UsedBy(Some(bot.id.clone()))).current(chosen)
    });
    let items = std::iter::once(assistant).chain(bots).collect();
    Some(state.menus.view(&(), vec![MenuSection::new(items)], widgets::menu_width(), Align::End, Message::UsedMenu))
}

/// The editor's clock while a draft waits; none at rest.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    rights::subscription(&state.rights).map(Message::Rights)
}

// ----- Views --------------------------------------------------------------------------------

/// The page: the open client's rights below the section, else the keys and clients.
pub fn page<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
    match state.open.as_ref() {
        Some((_, kind)) => match open_client(state, data) {
            // Known from the last time, but not fresh yet: the editor waits.
            Some(client) if waiting(state) => Page::new(updating(kind)).sub_page(client.name.clone(), Message::Back),
            Some(client) => Page::new(client_page(state, data, client)).sub_page(client.name.clone(), Message::Back),
            // Still on its way: the page waits. Gone: the list, until `loaded` closes the page.
            None if waiting(state) => Page::new(updating(kind)).sub_page(tr("Updating…"), Message::Back),
            None => Page::new(list(state, data)),
        },
        None => Page::new(list(state, data)),
    }
}

/// The keys and the MCP clients with what each may do here, and where they are made.
fn list<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let (keys, mcp) = listed(data);
    let areas = state.areas();
    let row = |client: &'a ClientInfo| {
        // A client's draft – still on its way after "Back" – shows here too, as on its page.
        let summary = rights::area_summary(rights::shown(&state.rights, client), areas);
        let line = setting_row(None, client.name.as_str(), Some(summary), chevron());
        pressable(line, Some(Message::Open(client.id.clone(), client.kind.clone())))
    };
    let mut groups: Vec<Element<'a, Message>> = Vec::new();
    if keys.is_empty() && mcp.is_empty() {
        let line = setting_row(None, tr("No program uses the API or MCP yet."), None, iced::widget::Space::new());
        groups.push(widgets::group(tr("API and MCP"), vec![line], None));
    }
    if !keys.is_empty() {
        groups.push(widgets::group(tr("API keys"), keys.into_iter().map(row).collect(), None));
    }
    if !mcp.is_empty() {
        groups.push(widgets::group(tr("MCP clients"), mcp.into_iter().map(row).collect(), None));
    }
    if uses_servers(state.app) {
        groups.push(used_group(state, data));
    }
    groups.push(foot_list());
    column(groups).spacing(layout::section_gap()).into()
}

/// "Used by the assistant" (issue #666): whose tools and servers – the assistant or a bot, in a
/// dropdown –, then per built-in tool and MCP server a switch and "Show commands" ([`rights::Part::Servers`]).
fn used_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let name = match &state.used {
        None => tr("Assistant").to_string(),
        Some(bot) => {
            data.bots.iter().find(|summary| &summary.id == bot).map_or_else(|| bot.clone(), |b| b.name.clone())
        }
    };
    let chooser = state.menus.button((), name, Message::UsedMenu);
    let who = setting_row(None, tr("For"), Some(tr("The assistant and each bot have their own.").to_string()), chooser);
    let mut parts = vec![widgets::group(tr("Used by the assistant"), vec![who], None)];
    match used_client(state, data) {
        Some(client) if !waiting(state) => {
            let rights = rights::shown(&state.rights, client);
            let editor = rights::view_part(
                &state.rights,
                rights,
                &client.kind,
                Known::of(data),
                Naming::Tools,
                tr("Used by the assistant"),
                rights::Part::Servers,
            );
            parts.push(editor.map(Message::Used));
        }
        _ if waiting(state) => parts.push(updating_group(tr("Used by the assistant"))),
        // No client to show (review of #663): the assistant is off, or the bot chosen is gone.
        _ => {
            let missing = match state.used {
                None => tr("The assistant is off"),
                Some(_) => tr("This bot no longer exists"),
            };
            let line = setting_row(None, missing, None, iced::widget::Space::new());
            parts.push(widgets::group(tr("Built-in tools"), vec![line], None));
        }
    }
    column(parts).spacing(layout::section_gap()).into()
}

/// A client's page: the editor for this app's areas, then the way to its other rights.
fn client_page<'a>(state: &'a State, data: &'a Data, client: &'a ClientInfo) -> Element<'a, Message> {
    let rights = rights::shown(&state.rights, client);
    let naming = if client.kind == SCRIPT_CLIENT { Naming::Endpoints } else { Naming::Tools };
    let areas = state.areas();
    let section = if client.kind == SCRIPT_CLIENT { "api" } else { "mcp" };
    let editor = responsive(move |size| {
        let groups = rights::groups_in(
            &state.rights,
            rights,
            &client.kind,
            &Known::of(data),
            naming,
            tr("Access"),
            rights::control_column(size.width),
            areas,
        );
        column(groups).spacing(layout::section_gap()).into()
    })
    .height(Length::Shrink);
    let editor: Element<'a, rights::Message> = editor.into();
    column![editor.map(Message::Rights), foot(tr("Other rights of this client are set in Torro Workplace."), section)]
        .spacing(layout::section_gap())
        .into()
}

/// "Updating…" in place of a client's editor until the fresh list of clients is in.
fn updating<'a>(kind: &str) -> Element<'a, Message> {
    updating_group(if kind == SCRIPT_CLIENT { tr("API keys") } else { tr("MCP clients") })
}

/// A group `title` that says "Updating…".
fn updating_group<'a>(title: &'a str) -> Element<'a, Message> {
    let line = setting_row(None, tr("Updating…"), None, iced::widget::Space::new());
    widgets::group(title, vec![line], None)
}

/// The foot of the list (issue #665): with the main application one row each to "API clients" and "MCP
/// clients" in Torro Workplace, where keys are made and clients connected; without it the sentence.
fn foot_list<'a>() -> Element<'a, Message> {
    if !torro_gui::installed::main_app() {
        let line =
            setting_row(None, tr("Keys and clients are created in Torro Workplace."), None, iced::widget::Space::new());
        return widgets::group(tr("Torro Workplace"), vec![line], None);
    }
    let row = |label: &'a str, detail: &'a str, section: &'static str| {
        let line = setting_row(None, label, Some(detail.to_string()), arrow());
        pressable(line, Some(Message::ControlCentre(section)))
    };
    let rows = vec![
        row(tr("API clients"), tr("Create keys for scripts and programs"), "api"),
        row(tr("MCP clients"), tr("Connect coding agents and apps"), "mcp"),
    ];
    widgets::group(tr("Torro Workplace"), rows, None)
}

/// The arrow of a row that opens another window.
fn arrow<'a>() -> Element<'a, Message> {
    icon::icon(Icon::ArrowUpRight, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) })
        .into()
}

/// The foot of a page: where keys, clients and other rights are made. With the main application a
/// row that opens it at `section` with the arrow of another window; without it only the sentence that
/// they are made in Torro Workplace (issue #538 brings them to a single app).
fn foot<'a>(sentence: &'static str, section: &'static str) -> Element<'a, Message> {
    if !torro_gui::installed::main_app() {
        let line =
            setting_row(None, tr("Keys and clients are created in Torro Workplace."), None, iced::widget::Space::new());
        return widgets::group(tr("Torro Workplace"), vec![line], None);
    }
    let line = setting_row(None, tr("Open Torro Workplace"), Some(sentence.to_string()), arrow());
    widgets::group(tr("Torro Workplace"), vec![pressable(line, Some(Message::ControlCentre(section)))], None)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use iced::time::Instant;
    use torro_ipc::api::{ASSISTANT_CLIENT, BOT_CLIENT, Confirm, HOOK_CLIENT, Level, Outbound, Rights};

    use super::*;
    use crate::demo::Demo;
    use crate::{Action as Act, Backend};

    type Action = crate::Action<()>;

    /// The demo's data as an app with these areas loads it, with a hook client besides.
    async fn data(app: AppId) -> Data {
        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        let mut data = crate::load(backend, wanted(app)).await;
        let mut hook = data.clients.iter().find(|client| client.kind == SCRIPT_CLIENT).expect("a key").clone();
        hook.id = "h0000001".into();
        hook.kind = HOOK_CLIENT.into();
        data.clients.push(hook);
        data
    }

    fn requests(action: Action) -> Vec<Request> {
        match action {
            Act::Send(requests) => requests,
            Act::None | Act::ControlCentre(_) => Vec::new(),
            other => panic!("API and MCP only ever sends: {other:?}"),
        }
    }

    /// The clock once a draft is due.
    fn later() -> Message {
        Message::Rights(rights::Message::Clock(Instant::now() + Duration::from_secs(1)))
    }

    /// Every app but the control centre has the section, with its own areas only.
    #[test]
    fn every_app_but_the_control_centre_has_the_section() {
        // The chat (issue #643) has no settings of its own yet, so no section here either.
        for app in AppId::ALL {
            let without = *app == AppId::WORKPLACE || *app == AppId::CHAT;
            assert_eq!(sections(*app).is_empty(), without, "{}", app.window_class);
            if !without {
                assert_eq!(sections(*app)[0].id, "api-mcp");
                assert!(wanted(*app).contains(&Request::ListClients));
            }
        }
        assert_eq!(areas(AppId::TASKS), [Area::Tasks, Area::Github]);
        assert_eq!(areas(AppId::INBOX_MAIL), [Area::Mail, Area::Workflows]);
        assert_eq!(areas(AppId::INBOX_NOTIFICATIONS), areas(AppId::INBOX));
        assert!(AppId::ALL.iter().all(|app| !areas(*app).contains(&Area::Contacts)), "contacts stay shared");
    }

    /// Issue #666: in the assistant's window "Used by the assistant" switches a tool or a server for the
    /// assistant, or for the bot the dropdown chose, and sends only that client's tools and servers; a
    /// control of another area is not taken there, and another app has no such group.
    #[tokio::test]
    async fn used_by_the_assistant_switches_tools_and_servers_of_the_one_chosen() {
        let data = data(AppId::ASSISTANT).await;
        assert!(wanted(AppId::ASSISTANT).contains(&Request::ListBots));
        assert!(!wanted(AppId::CALENDAR).contains(&Request::ListBots));
        let assistant = data.clients.iter().find(|client| client.kind == ASSISTANT_CLIENT).expect("assistant");
        let mut state = State::new(AppId::ASSISTANT);
        clients_fresh(&mut state);
        let pdf_off = || Message::Used(rights::Message::Tool(ToolId::Pdf, false));
        let sent = requests(update(&mut state, pdf_off(), &data));
        assert!(
            matches!(&sent[..], [Request::UpdateClient { id, rights }]
                if *id == assistant.id && !rights.reaches_tool(ToolId::Pdf) && rights.areas == assistant.rights.areas),
            "{sent:?}"
        );
        let _ = done::<()>(&mut state, &Ok(()));
        let level = Message::Used(rights::Message::Level(Area::Calendar, Level::Full));
        assert!(requests(update(&mut state, level, &data)).is_empty(), "no area under Used by the assistant");

        // The bot chosen in the dropdown: its own client, the assistant's stays as it is.
        let bot = data.bots.iter().find(|bot| bot.id == "b0000001").expect("the travel planner");
        let _ = update::<()>(&mut state, Message::UsedBy(Some(bot.id.clone())), &data);
        let server = Message::Used(rights::Message::Server("time".into(), false));
        let sent = requests(update(&mut state, server, &data));
        assert!(
            matches!(&sent[..], [Request::UpdateClient { id, rights }]
                if *id == bot.client && rights.extension("time") == Level::NoAccess && rights.extension("later") == Level::Full),
            "{sent:?}"
        );

        let mut calendar = State::new(AppId::CALENDAR);
        clients_fresh(&mut calendar);
        assert!(requests(update(&mut calendar, pdf_off(), &data)).is_empty(), "the calendar has no such group");
    }

    /// The rows say what a client may do in the app's areas, in words.
    #[tokio::test]
    async fn the_rows_say_what_a_client_may_do_here() {
        let data = data(AppId::CALENDAR).await;
        let (keys, mcp) = listed(&data);
        assert_eq!(keys.len(), 1, "the hook, the assistant and the bots are not listed");
        assert!(mcp.iter().all(|client| is_mcp_kind(&client.kind)) && mcp.len() == 2);
        let read =
            format!("{}: {} · {}", tr("Calendar"), tr("Read"), torro_core::i18n::trf("{n} calendars", &[("n", &2)]));
        assert_eq!(rights::area_summary(&keys[0].rights, areas(AppId::CALENDAR)), read);
        let codex = mcp.iter().find(|client| client.kind == "codex").expect("Codex");
        assert_eq!(rights::area_summary(&codex.rights, areas(AppId::CALENDAR)), tr("No access"));
        let state = State::new(AppId::CALENDAR);
        let _ = page(&state, &data).body;
    }

    /// Until the first fresh list of clients of this opening is in, the editor is shut: a change is
    /// dropped and the page says "Updating…"; then it goes out at once.
    #[tokio::test]
    async fn the_editor_waits_for_the_fresh_list_of_clients() {
        let data = data(AppId::CALENDAR).await;
        let claude = data.clients.iter().find(|client| client.kind == "claude-code").expect("Claude Code").clone();
        let mut state = State::new(AppId::CALENDAR);
        let _ = update::<()>(&mut state, Message::Open(claude.id.clone(), claude.kind.clone()), &data);
        let level = || Message::Rights(rights::Message::Level(Area::Calendar, Level::Read));
        assert!(requests(update(&mut state, level(), &data)).is_empty(), "nothing before the fresh list");
        assert!(requests(update(&mut state, later(), &data)).is_empty(), "and no draft waits");
        let _ = page(&state, &data).body;
        assert!(waiting(&state), "a new page waits, also when the data knows the client");

        clients_fresh(&mut state);
        let sent = requests(update(&mut state, level(), &data));
        assert!(
            matches!(&sent[..], [Request::UpdateClient { id, rights }] if *id == claude.id && rights.area(Area::Calendar).level == Level::Read),
            "{sent:?}"
        );
        // One at a time: a change while the first is on its way goes out with its answer.
        let write = Message::Rights(rights::Message::Level(Area::Calendar, Level::Write));
        assert!(requests(update(&mut state, write, &data)).is_empty());
        assert!(requests(update(&mut state, later(), &data)).is_empty(), "the first is still on its way");
        let sent = requests(done(&mut state, &Ok(())));
        assert!(
            matches!(&sent[..], [Request::UpdateClient { rights, .. }] if rights.area(Area::Calendar).level == Level::Write)
        );
    }

    /// Issue #531: the boundary in the app process. Whatever the page is asked to do, it sends
    /// `clients.update_client` only, only for a key or an MCP client, and changes only the app's
    /// areas; the assistant, a bot and a hook stay untouched, also when asked for by their ids.
    #[tokio::test]
    async fn the_page_sends_only_what_it_may() {
        for app in AppId::ALL.iter().copied().filter(|app| !areas(*app).is_empty()) {
            let data = data(app).await;
            let editable_ids: Vec<&str> =
                data.clients.iter().filter(|client| editable(&client.kind)).map(|client| client.id.as_str()).collect();
            let mut opens = Vec::new();
            for client in &data.clients {
                opens.push(Message::Open(client.id.clone(), client.kind.clone()));
                // The id of the assistant, a bot or a hook with a kind this page edits.
                opens.push(Message::Open(client.id.clone(), SCRIPT_CLIENT.into()));
                opens.push(Message::Open(client.id.clone(), "claude-code".into()));
            }
            let edits = || {
                let mut edits: Vec<rights::Message> = Vec::new();
                for area in Area::ALL {
                    edits.push(rights::Message::Level(area, Level::Full));
                    edits.push(rights::Message::Admin(area, true));
                }
                edits.extend([
                    rights::Message::Confirm(Confirm::OnEveryChange),
                    rights::Message::Outbound(Outbound::Free),
                    rights::Message::Command("calendar.list_events", false),
                    rights::Message::Command("contacts.list_contacts", false),
                    rights::Message::Command("mail.search_messages", false),
                    rights::Message::ServerLevel("time".into(), Level::Full),
                    rights::Message::ServerTool("time".into(), "convert_time".into(), false),
                ]);
                for pick in Pick::ALL {
                    edits.push(rights::Message::All(pick, false));
                    edits.push(rights::Message::Tick(pick, "x".into(), true));
                }
                edits.push(rights::Message::RepoInput("torro/workplace".into()));
                edits.push(rights::Message::AddRepo);
                edits
            };
            let mut state = State::new(app);
            clients_fresh(&mut state);
            let mut sent = Vec::new();
            for open in opens {
                sent.extend(requests(update(&mut state, open, &data)));
                for edit in edits() {
                    sent.extend(requests(update(&mut state, Message::Rights(edit), &data)));
                    sent.extend(requests(done(&mut state, &Ok(()))));
                    sent.extend(requests(update(&mut state, later(), &data)));
                    sent.extend(requests(done(&mut state, &Ok(()))));
                }
                sent.extend(requests(update(&mut state, Message::ControlCentre("mcp"), &data)));
                sent.extend(requests(update(&mut state, Message::Back, &data)));
                sent.extend(requests(done(&mut state, &Ok(()))));
            }
            assert!(!sent.is_empty(), "{}", app.window_class);
            for request in &sent {
                let Request::UpdateClient { id, rights } = request else {
                    panic!("{}: sent {request:?}", app.window_class)
                };
                assert!(editable_ids.contains(&id.as_str()), "{}: {id}", app.window_class);
                let client = data.clients.iter().find(|client| client.id == *id).expect("a listed client");
                assert!(editable(&client.kind));
                assert_unchanged_outside(&client.rights, rights, areas(app), app);
            }
            let untouched = [ASSISTANT_CLIENT, BOT_CLIENT, HOOK_CLIENT];
            let theirs: Vec<&str> = data
                .clients
                .iter()
                .filter(|client| untouched.contains(&client.kind.as_str()))
                .map(|client| client.id.as_str())
                .collect();
            assert!(
                sent.iter().all(
                    |request| !matches!(request, Request::UpdateClient { id, .. } if theirs.contains(&id.as_str()))
                )
            );
        }
    }

    /// A save changes only the app's areas in what the core holds right then: what the control centre
    /// changed meanwhile in another area, in "Ask first" or for a server stays (review of #531).
    #[tokio::test]
    async fn a_save_keeps_what_the_core_holds_outside_the_app() {
        let demo = Arc::new(Demo::new());
        let backend: Arc<dyn Backend> = demo.clone();
        let data = crate::load(Arc::clone(&backend), wanted(AppId::CALENDAR)).await;
        let claude = data.clients.iter().find(|client| client.kind == "claude-code").expect("Claude Code");
        // What this window last saw, changed in the calendar, and stale everywhere else.
        let mut edited = claude.rights.clone();
        edited.areas.insert(Area::Calendar, torro_ipc::api::AreaRights { level: Level::Read, admin: false });
        edited.areas.insert(Area::Mail, torro_ipc::api::AreaRights { level: Level::NoAccess, admin: true });
        edited.confirm = Confirm::Never;
        edited.denied.insert("calendar.list_events".into());
        edited.denied.insert("mail.search_messages".into());
        let request = Request::UpdateClient { id: claude.id.clone(), rights: edited.clone() };
        send(backend, vec![request], AppId::CALENDAR).await.expect("saved");
        let sent = demo.sent();
        let Some(Request::UpdateClient { rights, .. }) = sent.last() else { panic!("{sent:?}") };
        assert_eq!(rights.area(Area::Calendar).level, Level::Read);
        assert!(rights.denied.contains("calendar.list_events"));
        assert_eq!(rights.area(Area::Mail), claude.rights.area(Area::Mail), "the core's mail rights stay");
        assert!(!rights.denied.contains("mail.search_messages"));
        assert_eq!(rights.confirm, claude.rights.confirm);
        assert_eq!(*rights, merged(&claude.rights, &edited, areas(AppId::CALENDAR)));
        // A client that is gone is not made up again.
        let gone = Request::UpdateClient { id: "gone".into(), rights: edited.clone() };
        let backend: Arc<dyn Backend> = demo.clone();
        assert!(send(backend, vec![gone], AppId::CALENDAR).await.is_err());
        // The assistant only in its tools and servers (issue #666), also when asked by its id with other
        // rights (review of PR #550).
        let assistant = data.clients.iter().find(|client| client.kind == ASSISTANT_CLIENT).expect("the assistant");
        let theirs = Request::UpdateClient { id: assistant.id.clone(), rights: edited.clone() };
        let backend: Arc<dyn Backend> = demo.clone();
        assert!(send(backend, vec![theirs.clone()], AppId::CALENDAR).await.is_err(), "not from another app");
        let backend: Arc<dyn Backend> = demo.clone();
        send(backend, vec![theirs], AppId::ASSISTANT).await.expect("saved");
        let sent = demo.sent();
        let Some(Request::UpdateClient { rights, .. }) = sent.last() else { panic!("{sent:?}") };
        assert_eq!(*rights, merged_used(&assistant.rights, &edited));
        assert_eq!(rights.areas, assistant.rights.areas, "no area of the assistant changes from here");
        assert_eq!(rights.confirm, assistant.rights.confirm);
        // Nor a client the core does not list as a key, an MCP client, the assistant or a bot.
        let before = demo.sent().len();
        let theirs = Request::UpdateClient { id: "h0000001".into(), rights: edited };
        let backend: Arc<dyn Backend> = demo.clone();
        assert!(send(backend, vec![theirs], AppId::ASSISTANT).await.is_err());
        assert!(!demo.sent()[before..].iter().any(|request| matches!(request, Request::UpdateClient { .. })));
    }

    /// A draft that waits for its turn is not replaced by a change of another client.
    #[tokio::test]
    async fn a_waiting_draft_keeps_its_place() {
        let data = data(AppId::CALENDAR).await;
        let (keys, mcp) = listed(&data);
        let (key, claude) = (keys[0].clone(), mcp[0].clone());
        let mut state = State::new(AppId::CALENDAR);
        clients_fresh(&mut state);
        let level = |level| Message::Rights(rights::Message::Level(Area::Calendar, level));
        let _ = update::<()>(&mut state, Message::Open(key.id.clone(), key.kind.clone()), &data);
        assert_eq!(requests(update(&mut state, level(Level::Write), &data)).len(), 1, "the first goes at once");
        assert!(requests(update(&mut state, level(Level::Full), &data)).is_empty(), "the second waits");
        let _ = update::<()>(&mut state, Message::Open(claude.id.clone(), claude.kind.clone()), &data);
        assert!(requests(update(&mut state, level(Level::Read), &data)).is_empty(), "dropped, not in its place");
        let sent = requests(done(&mut state, &Ok(())));
        assert!(
            matches!(&sent[..], [Request::UpdateClient { id, rights }] if *id == key.id && rights.area(Area::Calendar).level == Level::Full),
            "{sent:?}"
        );
    }

    /// Nothing outside `areas` of `sent` differs from `held`: no other area, no "Ask first", no MCP
    /// server, no command of another area switched off, no other selection.
    fn assert_unchanged_outside(held: &Rights, sent: &Rights, areas: &[Area], app: AppId) {
        let app = app.window_class;
        for area in Area::ALL.into_iter().filter(|area| !areas.contains(area)) {
            assert_eq!(held.area(area), sent.area(area), "{app}: {area:?}");
        }
        assert_eq!(held.confirm, sent.confirm, "{app}");
        assert_eq!(held.extensions, sent.extensions, "{app}");
        for name in sent.denied.symmetric_difference(&held.denied) {
            let command = CATALOGUE.iter().find(|command| command.name == name).expect("a command");
            assert!(areas.contains(&command.area), "{app}: {name}");
        }
        for pick in Pick::ALL.into_iter().filter(|pick| !areas.contains(&pick.area())) {
            assert_eq!(pick.of(held), pick.of(sent), "{app}: {pick:?}");
        }
        if !areas.contains(&Area::Mail) {
            assert_eq!(held.outbound, sent.outbound, "{app}");
        }
    }
}
